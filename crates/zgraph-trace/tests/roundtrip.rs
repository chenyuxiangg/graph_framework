//! 写入/读取 roundtrip、seq 语义、severity 推导、payload 截断、多 run 全局序。

mod common;

use rusqlite::Connection;
use serde_json::json;
use zgraph_trace::{
    EventQuery, InstanceStatus, LlmCallRecord, RecordMode, RecordedOutput, RunFinishedInfo,
    RunMetrics, RunStatus, TraceInput,
};

fn event_severities(events: &[zgraph_trace::EventRow]) -> Vec<&str> {
    events.iter().map(|event| event.severity.as_str()).collect()
}

#[test]
fn full_run_roundtrip_persists_events_details_and_run_state() {
    let fx = common::Fixture::new();
    let handle = &fx.handle;
    let run_id = "run-1";
    let instance_id = "inst-start";
    handle.send(common::started(run_id));
    handle.send(common::transition(
        run_id,
        instance_id,
        "start",
        InstanceStatus::Pending,
        InstanceStatus::Ready,
        common::ts(10),
    ));
    handle.send(common::transition(
        run_id,
        instance_id,
        "start",
        InstanceStatus::Ready,
        InstanceStatus::Running,
        common::ts(20),
    ));
    handle.send(common::transition(
        run_id,
        instance_id,
        "start",
        InstanceStatus::Running,
        InstanceStatus::Completed,
        common::ts(100),
    ));
    handle.send(common::gate(run_id, instance_id, "start", 0, false, None));
    handle.send(common::gate(
        run_id,
        instance_id,
        "start",
        1,
        false,
        Some("gate exploded"),
    ));
    handle.send(TraceInput::NodeTimedOut {
        run_id: run_id.to_string(),
        instance_id: "inst-slow".to_string(),
        elapsed_ms: 1_000,
        at: common::ts(300),
    });
    handle.send(TraceInput::NodeAbandoned {
        run_id: run_id.to_string(),
        instance_id: "inst-slow".to_string(),
        at: common::ts(310),
    });
    handle.send(TraceInput::InstanceSnapshot(common::snapshot(
        run_id,
        "inst-slow",
        "slow_node",
        InstanceStatus::Running,
    )));
    handle.send(TraceInput::RecordedOutput(RecordedOutput {
        run_id: run_id.to_string(),
        instance_id: instance_id.to_string(),
        node_id: "start".to_string(),
        attempt: 0,
        output_hash: "fedcba9876543210".to_string(),
        output_json: Some(json!({"result": "ok"})),
        recorded_at: common::ts(120),
    }));
    handle.send(TraceInput::LlmCall(LlmCallRecord {
        run_id: run_id.to_string(),
        instance_id: "inst-agent".to_string(),
        call_index: 0,
        request_json: json!({"model": "fake", "messages": []}),
        response_json: Some(json!({"content": "hi"})),
        status: "ok".to_string(),
        recorded_at: common::ts(130),
    }));
    handle.send(common::finished(run_id, RunStatus::Failed));
    fx.flush();

    let reader = fx.reader();
    let run = reader.run(run_id).expect("query run").expect("run exists");
    assert_eq!(run.run_id, run_id);
    assert_eq!(run.graph_hash, "0123456789abcdef");
    assert_eq!(run.status, RunStatus::Failed);
    assert_eq!(run.user_id, "local");
    assert_eq!(run.started_at, common::STARTED_AT);
    let finished_at = common::ts(1842);
    assert_eq!(run.finished_at.as_deref(), Some(finished_at.as_str()));
    assert_eq!(
        run.entry_inputs,
        Some(json!({"start": {"in": 1}})),
        "entry_inputs roundtrip"
    );
    assert_eq!(run.output_json, Some(json!({"result": "ok"})));
    assert_eq!(run.error_code.as_deref(), Some("NODE_TIMEOUT"));
    assert_eq!(run.error_message.as_deref(), Some("node timed out"));
    assert!(!run.interrupted, "interrupted defaults to false");
    assert_eq!(run.seed, 42);
    assert_eq!(run.record_mode.as_deref(), Some(RecordMode::Hash.as_str()));
    assert_eq!(
        (run.streaming_dropped, run.abandoned_tasks),
        (3, 0),
        "RunMetrics persisted"
    );
    assert_eq!(
        (run.trace_backpressure, run.killed_workers),
        (5, 1),
        "v2 metrics persisted"
    );

    let events = reader
        .events(run_id, &EventQuery::default())
        .expect("query events");
    let kinds: Vec<&str> = events.iter().map(|event| event.kind.as_str()).collect();
    assert_eq!(
        kinds,
        vec![
            "RunStarted",
            "Transition",
            "Transition",
            "Transition",
            "GateResult",
            "GateResult",
            "NodeTimedOut",
            "NodeAbandoned",
            "RunFinished",
        ]
    );
    assert_eq!(
        event_severities(&events),
        vec!["info", "info", "info", "info", "warn", "error", "error", "warn", "error",],
        "06 §6.3.3 severity derivation"
    );
    for pair in events.windows(2) {
        assert!(
            pair[0].seq < pair[1].seq,
            "events must be ordered by seq ASC"
        );
    }
    let timed_out = &events[6];
    assert_eq!(timed_out.instance_id.as_deref(), Some("inst-slow"));
    assert_eq!(timed_out.node_id, None);
    assert_eq!(timed_out.payload, json!({"elapsed_ms": 1000}));
    assert_eq!(events[7].payload, json!({"at": common::ts(310)}));
    assert_eq!(events[8].payload["status"], json!("Failed"));
    assert_eq!(events[8].payload["error"]["code"], json!("NODE_TIMEOUT"));

    let last_seq = events.last().expect("events non-empty").seq;
    assert_eq!(run.last_seq, last_seq, "runs.last_seq tracks events.seq");
    assert_eq!(
        run.event_count,
        i64::try_from(events.len()).expect("len fits i64"),
        "runs.event_count tracks events rows"
    );

    let transitions = reader.transitions(run_id).expect("query transitions");
    assert_eq!(transitions.len(), 3);
    let transition_seqs: Vec<i64> = events
        .iter()
        .filter(|event| event.kind == "Transition")
        .map(|event| event.seq)
        .collect();
    assert_eq!(
        transitions.iter().map(|row| row.seq).collect::<Vec<_>>(),
        transition_seqs,
        "transitions.seq must be 1:1 with events.seq"
    );
    assert_eq!(transitions[2].to_status, InstanceStatus::Completed);
    assert_eq!(transitions[2].at, common::ts(100));

    let instances = reader.instances(run_id).expect("query instances");
    assert_eq!(instances.len(), 2);
    let start = instances
        .iter()
        .find(|row| row.instance_id == instance_id)
        .expect("start instance");
    assert_eq!(start.status, InstanceStatus::Completed);
    assert_eq!(start.output_json, Some(json!({"result": "ok"})));
    assert_eq!(start.created_seq, None, "M2 allows NULL created_seq");
    let started_at = common::ts(5);
    assert_eq!(start.started_at.as_deref(), Some(started_at.as_str()));
    assert_eq!(
        start.finished_seq,
        Some(transitions[2].seq),
        "finished_seq backfilled with the terminal events.seq"
    );
    let slow = instances
        .iter()
        .find(|row| row.instance_id == "inst-slow")
        .expect("snapshot-only instance");
    assert_eq!(slow.status, InstanceStatus::Running);
    assert_eq!(slow.node_id, "slow_node");
    assert_eq!(slow.created_seq, None);

    let gates = reader.gate_results(run_id).expect("query gates");
    assert_eq!(gates.len(), 2);
    assert_eq!(gates[0].edge_index, 0);
    assert!(!gates[0].passed);
    assert_eq!(gates[0].error, None);
    assert_eq!(gates[1].error.as_deref(), Some("gate exploded"));
    assert_eq!(
        gates.iter().map(|row| row.seq).collect::<Vec<_>>(),
        events
            .iter()
            .filter(|event| event.kind == "GateResult")
            .map(|event| event.seq)
            .collect::<Vec<_>>(),
        "gate_results.seq must be 1:1 with events.seq"
    );

    let outputs = reader.recorded_outputs(run_id).expect("recorded outputs");
    assert_eq!(outputs.len(), 1);
    assert_eq!(outputs[0].output_hash, "fedcba9876543210");
    assert_eq!(outputs[0].output_json, Some(json!({"result": "ok"})));
    let calls = reader.llm_calls(run_id).expect("llm calls");
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].call_index, 0);
    assert_eq!(
        calls[0].request_json,
        json!({"model": "fake", "messages": []})
    );
    assert_eq!(calls[0].response_json, Some(json!({"content": "hi"})));
    assert_eq!(calls[0].status, "ok");

    let conn = Connection::open(&fx.path).expect("inspect connection");
    let (graph_name, dsl_version): (String, String) = conn
        .query_row(
            "SELECT graph_name, dsl_version FROM graphs WHERE graph_hash = ?1",
            ["0123456789abcdef"],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("graph row");
    assert_eq!(graph_name, "demo_graph");
    assert_eq!(dsl_version, "1.0");
    let heartbeat_at: String = conn
        .query_row(
            "SELECT heartbeat_at FROM runs WHERE run_id = ?1",
            [run_id],
            |row| row.get(0),
        )
        .expect("heartbeat column");
    assert_eq!(
        heartbeat_at,
        common::ts(1842),
        "RunFinished refreshes heartbeat_at in the same transaction"
    );
    drop(conn);
    fx.close().expect("close writer");
}

#[test]
fn severity_table_matches_spec_for_all_branches() {
    let fx = common::Fixture::new();
    let handle = &fx.handle;
    let run_id = "run-sev";
    handle.send(common::started(run_id));
    for (from, to) in [
        (InstanceStatus::Pending, InstanceStatus::Skipped),
        (InstanceStatus::Ready, InstanceStatus::Running),
        (InstanceStatus::Running, InstanceStatus::Cancelled),
        (InstanceStatus::Running, InstanceStatus::Crashed),
        (InstanceStatus::Running, InstanceStatus::TimedOut),
    ] {
        handle.send(common::transition(
            run_id,
            "inst-sev",
            "node",
            from,
            to,
            common::ts(10),
        ));
    }
    handle.send(common::gate(run_id, "inst-sev", "node", 0, true, None));
    handle.send(common::finished(run_id, RunStatus::Completed));
    fx.flush();

    let reader = fx.reader();
    let events = reader
        .events(run_id, &EventQuery::default())
        .expect("events");
    assert_eq!(
        event_severities(&events),
        vec![
            "info",  // RunStarted
            "info",  // Pending -> Skipped
            "info",  // Ready -> Running
            "info",  // Running -> Cancelled
            "error", // Running -> Crashed
            "error", // Running -> TimedOut
            "info",  // GateResult passed
            "info",  // RunFinished Completed
        ]
    );
}

#[test]
fn instance_snapshot_only_upserts_without_event_row() {
    let fx = common::Fixture::new();
    let handle = &fx.handle;
    handle.send(common::started("run-snap"));
    handle.send(TraceInput::InstanceSnapshot(common::snapshot(
        "run-snap",
        "inst-pending",
        "lonely",
        InstanceStatus::Pending,
    )));
    fx.flush();
    let reader = fx.reader();
    let events = reader
        .events("run-snap", &EventQuery::default())
        .expect("events");
    assert_eq!(events.len(), 1, "snapshot must not create an events row");
    let instances = reader.instances("run-snap").expect("instances");
    assert_eq!(instances.len(), 1);
    assert_eq!(instances[0].status, InstanceStatus::Pending);
    assert_eq!(instances[0].created_seq, None);
    assert_eq!(instances[0].input, Some(json!({"in": 1})));
    assert_eq!(instances[0].started_at, None);
    assert_eq!(instances[0].finished_at, None);
    fx.close().expect("close");
}

#[test]
fn seq_is_global_monotonic_across_runs_and_progress_columns_track_it() {
    let fx = common::Fixture::new();
    let handle = &fx.handle;
    handle.send(common::started("run-a"));
    handle.send(common::transition(
        "run-a",
        "inst-a",
        "node-a",
        InstanceStatus::Pending,
        InstanceStatus::Ready,
        common::ts(10),
    ));
    handle.send(common::started("run-b"));
    handle.send(common::transition(
        "run-b",
        "inst-b",
        "node-b",
        InstanceStatus::Pending,
        InstanceStatus::Ready,
        common::ts(11),
    ));
    handle.send(common::transition(
        "run-b",
        "inst-b",
        "node-b",
        InstanceStatus::Ready,
        InstanceStatus::Running,
        common::ts(12),
    ));
    handle.send(common::transition(
        "run-a",
        "inst-a",
        "node-a",
        InstanceStatus::Ready,
        InstanceStatus::Running,
        common::ts(13),
    ));
    handle.send(common::finished("run-a", RunStatus::Completed));
    handle.send(common::finished("run-b", RunStatus::Completed));
    fx.flush();

    let reader = fx.reader();
    let conn = Connection::open(&fx.path).expect("inspect connection");
    let mut stmt = conn
        .prepare("SELECT seq FROM events ORDER BY seq ASC")
        .expect("prepare");
    let seqs: Vec<i64> = stmt
        .query_map([], |row| row.get(0))
        .expect("query")
        .map(|row| row.expect("seq"))
        .collect();
    for pair in seqs.windows(2) {
        assert!(pair[0] < pair[1], "global seq must be strictly increasing");
    }
    drop(stmt);
    drop(conn);

    for run_id in ["run-a", "run-b"] {
        let events = reader
            .events(run_id, &EventQuery::default())
            .expect("events");
        let run = reader.run(run_id).expect("run").expect("exists");
        assert_eq!(run.last_seq, events.last().expect("events").seq);
        assert_eq!(
            run.event_count,
            i64::try_from(events.len()).expect("len fits i64")
        );
        let ascending: Vec<i64> = events.iter().map(|event| event.seq).collect();
        let mut sorted = ascending.clone();
        sorted.sort_unstable();
        assert_eq!(ascending, sorted, "per-run events must be seq ascending");
        assert_eq!(events.first().expect("first").kind, "RunStarted");
        assert_eq!(events.last().expect("last").kind, "RunFinished");
    }
    fx.close().expect("close");
}

#[test]
fn oversized_payload_is_truncated_but_detail_stays_intact() {
    let fx = common::Fixture::new();
    let huge_error = "x".repeat(70_000);
    let handle = &fx.handle;
    handle.send(common::started("run-trunc"));
    handle.send(common::gate(
        "run-trunc",
        "inst-trunc",
        "node",
        0,
        false,
        Some(&huge_error),
    ));
    handle.send(common::finished("run-trunc", RunStatus::Completed));
    fx.flush();

    let reader = fx.reader();
    let events = reader
        .events("run-trunc", &EventQuery::default())
        .expect("events");
    let gate_event = events
        .iter()
        .find(|event| event.kind == "GateResult")
        .expect("gate event");
    assert_eq!(gate_event.payload["_truncated"], json!(true));
    let bytes = gate_event.payload["bytes"]
        .as_u64()
        .expect("bytes field present");
    assert!(
        bytes > u64::try_from(zgraph_trace::TRACE_PAYLOAD_MAX_BYTES).expect("max fits"),
        "reported payload size must exceed the limit, got {bytes}"
    );
    let gates = reader.gate_results("run-trunc").expect("gates");
    assert_eq!(
        gates[0].error.as_deref().map(str::len),
        Some(huge_error.len()),
        "gate_results.error is not truncated (only events.payload is)"
    );
    fx.close().expect("close");
}

#[test]
fn run_finished_defaults_and_metrics_are_optional_on_success() {
    let fx = common::Fixture::new();
    let handle = &fx.handle;
    handle.send(common::started("run-ok"));
    handle.send(TraceInput::RunFinished(RunFinishedInfo {
        run_id: "run-ok".to_string(),
        status: RunStatus::Completed,
        output_summary: None,
        error: None,
        metrics: RunMetrics::default(),
        at: common::ts(500),
    }));
    fx.flush();
    let reader = fx.reader();
    let run = reader.run("run-ok").expect("run").expect("exists");
    assert_eq!(run.status, RunStatus::Completed);
    assert_eq!(run.output_json, None);
    assert_eq!(run.error_code, None);
    assert_eq!(run.error_message, None);
    assert!(!run.interrupted);
    assert_eq!(run.streaming_dropped, 0);
    assert_eq!(run.trace_backpressure, 0);
    assert_eq!(run.killed_workers, 0);
    let events = reader
        .events("run-ok", &EventQuery::default())
        .expect("events");
    assert_eq!(events[1].payload, json!({"status": "Completed"}));
    assert_eq!(events[1].severity, "info");
    fx.close().expect("close");
}

/// W2a / schema v4：`GateResult` 的 deferred/attempt/verdict 读写与 payload 形状。
#[test]
fn gate_result_v4_fields_roundtrip_and_defaults_are_backward_compatible() {
    let fx = common::Fixture::new();
    let run_id = "run-gate-v4";
    let handle = &fx.handle;
    handle.send(common::started(run_id));
    // defer 边被 gate 拦截：passed=false 且 deferred=true（severity 仍 warn）。
    handle.send(common::gate_with_v4(
        run_id,
        "inst-review",
        "reviewer",
        3,
        false,
        None,
        true,
        2,
        Some(json!({"x": 1})),
    ));
    // v0.1.0 默认语义：settle 边 false → deferred=false / attempt=0 / verdict=None。
    handle.send(common::gate(
        run_id,
        "inst-review",
        "reviewer",
        0,
        false,
        None,
    ));
    // 放行边（无 verdict 提取）。
    handle.send(common::gate(
        run_id,
        "inst-review",
        "reviewer",
        1,
        true,
        None,
    ));
    fx.flush();

    let reader = fx.reader();
    let gates = reader.gate_results(run_id).expect("gates");
    assert_eq!(gates.len(), 3);
    assert!(gates[0].deferred, "deferred roundtrip");
    assert_eq!(gates[0].attempt, 2, "attempt roundtrip");
    assert_eq!(gates[0].verdict, Some(json!({"x": 1})), "verdict roundtrip");
    assert!(!gates[0].passed);
    for gate in &gates[1..] {
        assert!(!gate.deferred, "settle gate defaults to deferred=false");
        assert_eq!(gate.attempt, 0, "settle gate defaults to attempt=0");
        assert_eq!(gate.verdict, None, "no verdict extraction → NULL");
    }
    assert!(!gates[1].passed);
    assert!(gates[2].passed);

    // events.payload.record 同步携带三键（06 §6.2.8 canonical 形状）。
    let events = reader
        .events(run_id, &EventQuery::default())
        .expect("events");
    let gate_events: Vec<&zgraph_trace::EventRow> = events
        .iter()
        .filter(|event| event.kind == "GateResult")
        .collect();
    assert_eq!(gate_events.len(), 3);
    let payloads: Vec<&serde_json::Value> = gate_events
        .iter()
        .map(|event| &event.payload["record"])
        .collect();
    assert_eq!(payloads[0]["deferred"], json!(true));
    assert_eq!(payloads[0]["attempt"], json!(2));
    assert_eq!(payloads[0]["verdict"], json!({"x": 1}));
    assert_eq!(payloads[1]["deferred"], json!(false));
    assert_eq!(payloads[1]["attempt"], json!(0));
    assert_eq!(payloads[1]["verdict"], json!(null));
    assert_eq!(payloads[2]["deferred"], json!(false));
    assert_eq!(payloads[2]["attempt"], json!(0));
    assert_eq!(payloads[2]["verdict"], json!(null));

    // deferred 不改变 severity 推导：passed=false + error=None 仍 warn。
    let severities: Vec<&str> = gate_events
        .iter()
        .map(|event| event.severity.as_str())
        .collect();
    assert_eq!(severities, vec!["warn", "warn", "info"]);
    fx.close().expect("close");
}
