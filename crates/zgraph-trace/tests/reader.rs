//! 只读 reader 的过滤、排序、stats 与并发读语义（06 §6.6/§6.10.4/§6.11）。

mod common;

use rusqlite::Connection;
use serde_json::json;
use zgraph_trace::{
    EventQuery, InstanceStatus, RunFinishedInfo, RunMetrics, RunStatus, TraceError, TraceInput,
    TRACE_SCHEMA_VERSION,
};

#[test]
fn event_query_supports_since_limit_kinds_and_streaming_toggle() {
    let fx = common::Fixture::new();
    let handle = &fx.handle;
    handle.send(common::started("run-query"));
    handle.send(common::transition(
        "run-query",
        "inst-q",
        "node",
        InstanceStatus::Pending,
        InstanceStatus::Ready,
        common::ts(10),
    ));
    handle.send(common::token(
        "run-query",
        "inst-q",
        Some("node"),
        "stream",
        common::ts(20),
    ));
    handle.send(common::transition(
        "run-query",
        "inst-q",
        "node",
        InstanceStatus::Ready,
        InstanceStatus::Running,
        common::ts(30),
    ));
    handle.send(common::finished("run-query", RunStatus::Completed));
    fx.flush();

    let reader = fx.reader();
    let all = reader
        .events("run-query", &EventQuery::default())
        .expect("all events");
    assert_eq!(
        all.iter()
            .map(|event| event.kind.as_str())
            .collect::<Vec<_>>(),
        vec![
            "RunStarted",
            "Transition",
            "Transition",
            "Streaming",
            "RunFinished"
        ],
        "Streaming is flushed before RunFinished and ordered by seq"
    );

    let since = reader
        .events(
            "run-query",
            &EventQuery {
                since_seq: u64::try_from(all[2].seq).expect("seq fits u64"),
                ..EventQuery::default()
            },
        )
        .expect("since query");
    assert_eq!(since.len(), 2);
    assert_eq!(since[0].kind, "Streaming");
    assert_eq!(since[1].kind, "RunFinished");

    let limited = reader
        .events(
            "run-query",
            &EventQuery {
                limit: 2,
                ..EventQuery::default()
            },
        )
        .expect("limited query");
    assert_eq!(
        limited.iter().map(|event| event.seq).collect::<Vec<_>>(),
        all.iter()
            .take(2)
            .map(|event| event.seq)
            .collect::<Vec<_>>()
    );

    let kinds = reader
        .events(
            "run-query",
            &EventQuery {
                kinds: Some(vec!["Transition".to_string()]),
                ..EventQuery::default()
            },
        )
        .expect("kinds query");
    assert_eq!(kinds.len(), 2);
    assert!(kinds.iter().all(|event| event.kind == "Transition"));

    let no_streaming = reader
        .events(
            "run-query",
            &EventQuery {
                include_streaming: false,
                ..EventQuery::default()
            },
        )
        .expect("no streaming query");
    assert_eq!(no_streaming.len(), 4);
    assert!(no_streaming.iter().all(|event| event.kind != "Streaming"));

    let excluded = reader
        .events(
            "run-query",
            &EventQuery {
                kinds: Some(vec!["Streaming".to_string()]),
                include_streaming: false,
                ..EventQuery::default()
            },
        )
        .expect("streaming excluded");
    assert!(excluded.is_empty());

    let empty_kinds = reader
        .events(
            "run-query",
            &EventQuery {
                kinds: Some(Vec::new()),
                ..EventQuery::default()
            },
        )
        .expect("empty kinds = no filter");
    assert_eq!(empty_kinds.len(), 5);
    fx.close().expect("close");
}

#[test]
fn stats_reports_counts_duration_and_db_info() {
    let fx = common::Fixture::new();
    let handle = &fx.handle;
    handle.send(common::started("run-stats"));
    handle.send(common::started("run-other"));
    for (from, to) in [
        (InstanceStatus::Pending, InstanceStatus::Ready),
        (InstanceStatus::Ready, InstanceStatus::Running),
        (InstanceStatus::Running, InstanceStatus::Completed),
    ] {
        handle.send(common::transition(
            "run-stats",
            "inst-a",
            "node-a",
            from,
            to,
            common::ts(10),
        ));
    }
    handle.send(common::transition(
        "run-stats",
        "inst-b",
        "node-b",
        InstanceStatus::Pending,
        InstanceStatus::Skipped,
        common::ts(11),
    ));
    handle.send(common::gate("run-stats", "inst-a", "node-a", 0, true, None));
    handle.send(TraceInput::NodeTimedOut {
        run_id: "run-stats".to_string(),
        instance_id: "inst-c".to_string(),
        elapsed_ms: 500,
        at: common::ts(200),
    });
    handle.send(TraceInput::NodeAbandoned {
        run_id: "run-stats".to_string(),
        instance_id: "inst-c".to_string(),
        at: common::ts(210),
    });
    handle.send(common::token(
        "run-stats",
        "inst-a",
        Some("node-a"),
        "tok",
        common::ts(220),
    ));
    handle.send(TraceInput::RunFinished(RunFinishedInfo {
        run_id: "run-stats".to_string(),
        status: RunStatus::Failed,
        output_summary: None,
        error: None,
        metrics: RunMetrics {
            streaming_dropped: 2,
            abandoned_tasks: 0,
            trace_backpressure: 5,
            killed_workers: 1,
        },
        at: common::ts(1842),
    }));
    fx.flush();

    let reader = fx.reader();
    let stats = reader.stats("run-stats").expect("stats");
    assert_eq!(stats.run_id, "run-stats");
    assert_eq!(stats.status, RunStatus::Failed);
    assert_eq!(stats.duration_ms, Some(1842));
    assert_eq!(stats.events_total, 10);
    assert_eq!(stats.kinds.get("RunStarted"), Some(&1));
    assert_eq!(stats.kinds.get("Transition"), Some(&4));
    assert_eq!(stats.kinds.get("GateResult"), Some(&1));
    assert_eq!(stats.kinds.get("NodeTimedOut"), Some(&1));
    assert_eq!(stats.kinds.get("NodeAbandoned"), Some(&1));
    assert_eq!(stats.kinds.get("Streaming"), Some(&1));
    assert_eq!(stats.kinds.get("RunFinished"), Some(&1));
    assert_eq!(stats.instances.get("Completed"), Some(&1));
    assert_eq!(stats.instances.get("Skipped"), Some(&1));
    assert_eq!(stats.streaming_segments, 1);
    assert_eq!(stats.streaming_dropped, 2);
    assert_eq!(stats.timeouts, 1);
    assert_eq!(stats.abandoned, 1);
    assert_eq!(stats.trace_backpressure, 5);
    assert!(stats.db_path.ends_with("trace.db"));
    assert!(stats.db_size_bytes > 0);
    assert_eq!(stats.db_runs, 2, "db_runs counts all runs in the database");
    fx.close().expect("close");
}

#[test]
fn missing_run_is_reported_predictably() {
    let fx = common::Fixture::new();
    fx.handle.send(common::started("run-present"));
    fx.flush();
    let reader = fx.reader();
    assert_eq!(reader.run("run-absent").expect("run query"), None);
    assert!(reader
        .events("run-absent", &EventQuery::default())
        .expect("events")
        .is_empty());
    assert!(reader
        .instances("run-absent")
        .expect("instances")
        .is_empty());
    assert!(reader
        .transitions("run-absent")
        .expect("transitions")
        .is_empty());
    assert!(reader.gate_results("run-absent").expect("gates").is_empty());
    assert!(reader
        .streaming_events("run-absent")
        .expect("streaming")
        .is_empty());
    assert!(reader
        .recorded_outputs("run-absent")
        .expect("outputs")
        .is_empty());
    assert!(reader
        .llm_calls("run-absent")
        .expect("llm calls")
        .is_empty());
    match reader.stats("run-absent") {
        Err(TraceError::NotFound(message)) => assert!(message.contains("run-absent")),
        other => panic!("expected NotFound, got {other:?}"),
    }
    fx.close().expect("close");
}

#[test]
fn graph_row_and_run_graph_name_are_available() {
    let fx = common::Fixture::new();
    fx.handle.send(common::started("run-graph"));
    fx.flush();
    let reader = fx.reader();
    let graph = reader
        .graph("0123456789abcdef")
        .expect("graph query")
        .expect("graph row exists");
    assert_eq!(graph.graph_hash, "0123456789abcdef");
    assert_eq!(graph.graph_name.as_deref(), Some("demo_graph"));
    assert_eq!(graph.dsl_version, "1.0");
    assert_eq!(graph.dsl_json, r#"{"version":"1.0"}"#);
    assert_eq!(graph.created_at, common::STARTED_AT);
    assert_eq!(
        reader.graph("fedcba9876543210").expect("graph query"),
        None,
        "missing graph is not an error"
    );
    let run = reader
        .run("run-graph")
        .expect("run query")
        .expect("run exists");
    assert_eq!(
        run.graph_name.as_deref(),
        Some("demo_graph"),
        "run() joins graphs for graph_name"
    );
    fx.close().expect("close");
}

#[test]
fn run_without_graph_row_reports_none_graph_name() {
    let fx = common::Fixture::new();
    fx.handle.send(common::started("run-present"));
    fx.flush();
    // graph 行缺失的 run：关闭 FK 直接插入，验证 LEFT JOIN 容错（不报错）。
    let conn = Connection::open(&fx.path).expect("inspect connection");
    conn.execute_batch(
        "PRAGMA foreign_keys = OFF;\
         INSERT INTO runs (run_id, graph_hash, status, started_at) \
         VALUES ('run-orphan', 'deadbeefdeadbeef', 'Running', '2026-10-28T09:12:03.104Z');",
    )
    .expect("insert orphan run");
    drop(conn);
    let reader = fx.reader();
    let run = reader
        .run("run-orphan")
        .expect("run query")
        .expect("orphan run exists");
    assert_eq!(run.graph_name, None, "missing graphs row yields None");
    assert_eq!(run.graph_hash, "deadbeefdeadbeef");
    assert_eq!(run.status, RunStatus::Running);
    assert_eq!(run.record_mode, None);
    fx.close().expect("close");
}

#[test]
fn readonly_readers_observe_committed_writes_after_flush() {
    let fx = common::Fixture::new();
    fx.handle.send(common::started("run-live"));
    fx.flush();
    let first = fx.reader();
    let second = fx.reader();
    assert_eq!(
        first.schema_version().expect("first version"),
        TRACE_SCHEMA_VERSION
    );
    assert_eq!(
        second.schema_version().expect("second version"),
        TRACE_SCHEMA_VERSION
    );
    assert_eq!(
        second
            .events("run-live", &EventQuery::default())
            .expect("second events")
            .len(),
        1
    );

    fx.handle.send(common::transition(
        "run-live",
        "inst-live",
        "node",
        InstanceStatus::Pending,
        InstanceStatus::Ready,
        common::ts(10),
    ));
    fx.flush();
    let first_events = first
        .events("run-live", &EventQuery::default())
        .expect("first events after flush");
    assert_eq!(first_events.len(), 2);
    assert_eq!(
        first_events
            .iter()
            .map(|event| event.kind.as_str())
            .collect::<Vec<_>>(),
        vec!["RunStarted", "Transition"]
    );
    let instances = first.instances("run-live").expect("instances");
    assert_eq!(instances.len(), 1);
    assert_eq!(instances[0].status, InstanceStatus::Ready);
    assert_eq!(
        instances[0].input,
        Some(json!({"in": 1})),
        "snapshot from transition persists"
    );
    drop(first);
    drop(second);
    fx.close().expect("close");
}
