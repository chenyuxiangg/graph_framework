//! 线性图 / end 聚合 / 全 Skipped / 无 end 的集成测试。

mod common;

use std::sync::Arc;

use serde_json::json;
use zgraph_kernel::{FakeExecutor, InstanceStatus, RunStatus};

#[tokio::test]
async fn linear_graph_completes_with_run_output() {
    let executor = Arc::new(FakeExecutor::new().respond("work", json!({"answer": 42})));
    let (outcome, sink) = common::run_with_sink(common::linear_spec(), executor).await;

    assert_eq!(outcome.status, RunStatus::Completed);
    assert_eq!(outcome.error, None);
    // 单 end：run_output = end.inputs；work→done 的端口 r 收到 work 整包 output。
    assert_eq!(outcome.output, Some(json!({"r": {"answer": 42}})));
    assert_eq!(
        common::last_status(&sink, "start"),
        Some(InstanceStatus::Completed)
    );
    assert_eq!(
        common::last_status(&sink, "work"),
        Some(InstanceStatus::Completed)
    );
    assert_eq!(
        common::last_status(&sink, "done"),
        Some(InstanceStatus::Completed)
    );
    assert_eq!(outcome.metrics.abandoned_tasks, 0);
    assert_eq!(outcome.metrics.killed_workers, 0);
    assert_eq!(sink.abandoned().len(), 0);
    common::assert_lifecycle_once(&sink);

    // sink 调用有序：RunStarted 第一、RunFinished 最后且仅一次（I5/I6）。
    let log = sink.call_log();
    assert_eq!(log.first(), Some(&zgraph_kernel::TraceCall::RunStarted));
    assert_eq!(log.last(), Some(&zgraph_kernel::TraceCall::RunFinished));
    assert_eq!(
        log.iter()
            .filter(|call| **call == zgraph_kernel::TraceCall::RunStarted)
            .count(),
        1
    );
    assert_eq!(
        log.iter()
            .filter(|call| **call == zgraph_kernel::TraceCall::RunFinished)
            .count(),
        1
    );
    let finished = sink.finished().into_iter().next().expect("finished");
    assert_eq!(finished.status, RunStatus::Completed);
    assert_eq!(finished.output_summary, outcome.output);
    assert_eq!(finished.metrics, outcome.metrics);
    assert_eq!(finished.run_id, outcome.run_id);
}

#[tokio::test]
async fn run_started_carries_metadata_and_rfc3339_millis_timestamps() {
    let executor = Arc::new(FakeExecutor::new());
    let (outcome, sink) = common::run_with_sink(common::linear_spec(), executor).await;
    assert_eq!(outcome.status, RunStatus::Completed);

    let started = sink.started_infos().into_iter().next().expect("started");
    assert_eq!(started.run_id, outcome.run_id);
    assert_eq!(started.graph_name, "linear");
    assert_eq!(started.user_id, "u1");
    assert_eq!(started.seed, 0);
    assert_eq!(started.record_mode, "hash");
    assert_ne!(started.owner_pid, 0);
    assert!(!started.owner_host.is_empty());
    assert_eq!(started.entry_inputs, json!({"start": {"x": {"v": 1}}}));
    assert!(!started.dsl_json.is_empty());
    assert_eq!(started.graph_hash.len(), 16);

    for ts in [
        &started.started_at,
        &outcome.started_at,
        &outcome.finished_at,
    ] {
        let parsed = chrono::DateTime::parse_from_rfc3339(ts).expect("valid RFC3339");
        assert!(ts.ends_with('Z'));
        assert_eq!(parsed.offset().local_minus_utc(), 0);
        // 毫秒精度：允许任意毫秒，但必须是 3 位小数。
        assert_eq!(ts.len(), 24, "RFC3339 millis timestamp: {ts}");
    }
}

#[tokio::test]
async fn multiple_end_nodes_merge_in_node_order() {
    let text = r#"{
      "version": "1.0",
      "metadata": {"name": "multi-end"},
      "entry_points": ["start"],
      "nodes": [
        {"id": "start", "type": "start", "inputs": [], "config": {}},
        {"id": "a", "type": "python_node",
         "inputs": [{"name": "i", "type": "any", "required": true}],
         "config": {"module": "workers.multi", "function": "a"}},
        {"id": "b", "type": "python_node",
         "inputs": [{"name": "i", "type": "any", "required": true}],
         "config": {"module": "workers.multi", "function": "b"}},
        {"id": "end_one", "type": "end",
         "inputs": [{"name": "x", "type": "any", "required": true}], "config": {}},
        {"id": "end_two", "type": "end",
         "inputs": [{"name": "x", "type": "any", "required": true}], "config": {}}
      ],
      "edges": [
        {"from": "start", "to_node": "a", "to_input_port": "i"},
        {"from": "start", "to_node": "b", "to_input_port": "i"},
        {"from": "a", "to_node": "end_one", "to_input_port": "x"},
        {"from": "b", "to_node": "end_two", "to_input_port": "x"}
      ]
    }"#;
    let graph = common::compile_text(text);
    let executor = Arc::new(
        FakeExecutor::new()
            .respond("a", json!({"from": "a"}))
            .respond("b", json!({"from": "b"})),
    );
    let (outcome, sink) =
        common::run_with_sink(common::spec(graph, common::entry_inputs(&[])), executor).await;

    assert_eq!(outcome.status, RunStatus::Completed);
    // 浅合并、后完成 end 覆盖同名 key（node_order: end_one 在 end_two 之前）。
    assert_eq!(outcome.output, Some(json!({"x": {"from": "b"}})));
    assert_eq!(
        common::last_status(&sink, "end_one"),
        Some(InstanceStatus::Completed)
    );
    assert_eq!(
        common::last_status(&sink, "end_two"),
        Some(InstanceStatus::Completed)
    );
}

#[tokio::test]
async fn all_sources_blocked_yields_completed_without_output() {
    let text = r#"{
      "version": "1.0",
      "metadata": {"name": "blocked"},
      "entry_points": ["start"],
      "nodes": [
        {"id": "start", "type": "start", "inputs": [], "config": {}},
        {"id": "a", "type": "python_node",
         "inputs": [{"name": "i", "type": "any", "required": true}],
         "config": {"module": "workers.blocked", "function": "a"}},
        {"id": "done", "type": "end",
         "inputs": [{"name": "r", "type": "any", "required": true}], "config": {}}
      ],
      "edges": [
        {"from": "start", "to_node": "a", "to_input_port": "i"},
        {"from": "a", "to_node": "done", "to_input_port": "r",
         "gate": {"function": "check_output", "params": {"path": "ok", "equals": true}}}
      ]
    }"#;
    let graph = common::compile_text(text);
    let executor = Arc::new(FakeExecutor::new().respond("a", json!({"ok": false})));
    let (outcome, sink) =
        common::run_with_sink(common::spec(graph, common::entry_inputs(&[])), executor).await;

    assert_eq!(outcome.status, RunStatus::Completed);
    assert_eq!(outcome.output, None);
    assert_eq!(
        common::last_status(&sink, "done"),
        Some(InstanceStatus::Skipped)
    );
    let gate_results = sink.gate_results();
    // 两条出边各一条记录：start→a 无 gate（恒真）、a→done 有 gate（拦截）。
    assert_eq!(gate_results.len(), 2);
    let blocked: Vec<_> = gate_results
        .iter()
        .filter(|record| !record.passed)
        .collect();
    assert_eq!(blocked.len(), 1);
    assert_eq!(blocked[0].error, None);
    assert_eq!(blocked[0].gate_fn.as_deref(), Some("check_output"));
    let ungated = gate_results
        .iter()
        .find(|record| record.gate_fn.is_none())
        .expect("ungated edge still records a gate result");
    assert!(ungated.passed);
}

#[tokio::test]
async fn graph_without_end_completes_with_none_output() {
    let text = r#"{
      "version": "1.0",
      "metadata": {"name": "no-end"},
      "entry_points": ["start"],
      "nodes": [
        {"id": "start", "type": "start",
         "inputs": [{"name": "x", "type": "any", "required": true}], "config": {}},
        {"id": "leaf", "type": "python_node",
         "inputs": [{"name": "i", "type": "any", "required": true}],
         "config": {"module": "workers.leaf", "function": "run"}}
      ],
      "edges": [
        {"from": "start", "to_node": "leaf", "to_input_port": "i"}
      ]
    }"#;
    let graph = common::compile_text(text);
    let executor = Arc::new(FakeExecutor::new());
    let spec = common::spec(graph, common::entry_inputs(&[("start", "x", json!(1))]));
    let (outcome, sink) = common::run_with_sink(spec, executor).await;
    assert_eq!(outcome.status, RunStatus::Completed);
    assert_eq!(outcome.output, None);
    assert_eq!(
        common::last_status(&sink, "leaf"),
        Some(InstanceStatus::Completed)
    );
}

#[tokio::test]
async fn start_node_output_is_injected_input_object() {
    let executor = Arc::new(FakeExecutor::new());
    let (outcome, sink) = common::run_with_sink(common::linear_spec(), executor).await;
    assert_eq!(outcome.status, RunStatus::Completed);
    // work 的端口 x 收到 start 输出（入口注入整包 object）。
    // start 输出 = 入口注入 object（{"x": {"v": 1}}）；work 端口 x 收到整包。
    assert_eq!(
        common::running_input(&sink, "work"),
        Some(json!({"x": {"x": {"v": 1}}}))
    );
    let work_snapshot = common::snapshots_of(&sink, "work")
        .into_iter()
        .find(|snapshot| snapshot.status == InstanceStatus::Completed)
        .expect("completed snapshot");
    assert_eq!(
        work_snapshot.output,
        Some(json!({"echo": {"x": {"x": {"v": 1}}}})),
        "FakeExecutor 默认 echo 全部 inputs"
    );
}
