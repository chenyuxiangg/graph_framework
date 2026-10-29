//! Skipped 级联方向：无论"被拦截边"还是"放行边"最后结算，
//! 目标节点全部入边结算后若不能就绪都必须转 Skipped（REV-007 阻塞项 1）。
//!
//! 构造：`join`（trigger=all，两个 required 端口）的两条入边一条被 gate 永久拦截、
//! 另一条放行；拦截/放行两条路径的结算先后各测一次。修复前"放行边最后结算"会
//! 留下 Pending，超过 grace 后被误判 RUN_DEADLOCK。

mod common;

use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::json;
use zgraph_kernel::{FakeExecutor, InstanceStatus, RunOptions, RunStatus};

const JOIN_JSON: &str = r#"{
  "version": "1.0",
  "metadata": {"name": "cascade-direction"},
  "entry_points": ["start"],
  "nodes": [
    {"id": "start", "type": "start", "inputs": [], "config": {}},
    {"id": "a", "type": "python_node",
     "inputs": [{"name": "i", "type": "any", "required": true}],
     "config": {"module": "workers.cascade_dir", "function": "a"}},
    {"id": "b", "type": "python_node",
     "inputs": [{"name": "i", "type": "any", "required": true}],
     "config": {"module": "workers.cascade_dir", "function": "b"}},
    {"id": "join", "type": "python_node",
     "inputs": [
       {"name": "x", "type": "any", "required": true},
       {"name": "y", "type": "any", "required": true}
     ],
     "trigger": "all",
     "config": {"module": "workers.cascade_dir", "function": "join"}},
    {"id": "done", "type": "end",
     "inputs": [{"name": "r", "type": "any", "required": true}], "config": {}}
  ],
  "edges": [
    {"from": "start", "to_node": "a", "to_input_port": "i"},
    {"from": "start", "to_node": "b", "to_input_port": "i"},
    {"from": "a", "to_node": "join", "to_input_port": "x",
     "gate": {"function": "check_output", "params": {"path": "ok", "equals": true}}},
    {"from": "b", "to_node": "join", "to_input_port": "y"},
    {"from": "join", "to_node": "done", "to_input_port": "r"}
  ]
}"#;

fn cascade_options() -> RunOptions {
    // 若级联缺失，100ms grace + 10ms tick 会快速暴露 RUN_DEADLOCK（而不是等 5s）。
    common::options_with(|options| {
        options.deadlock_grace_ms = 100;
        options.tick_interval_ms = 10;
    })
}

fn spec() -> zgraph_kernel::RunSpec {
    let mut spec = common::spec(common::compile_text(JOIN_JSON), common::entry_inputs(&[]));
    spec.options = cascade_options();
    spec
}

/// 断言：join 与 done 都 Skipped、run Completed（output=None）、无超时/死锁。
fn assert_skipped_to_completion(
    sink: &zgraph_kernel::MemoryTraceSink,
    outcome: &zgraph_kernel::RunOutcome,
) {
    assert_eq!(outcome.status, RunStatus::Completed);
    assert_eq!(outcome.error, None);
    assert_eq!(outcome.output, None);
    assert_eq!(
        common::last_status(sink, "join"),
        Some(InstanceStatus::Skipped)
    );
    assert_eq!(
        common::last_status(sink, "done"),
        Some(InstanceStatus::Skipped)
    );
    let join_transitions = sink
        .transitions()
        .into_iter()
        .filter(|record| record.node_id == "join")
        .collect::<Vec<_>>();
    assert_eq!(join_transitions.len(), 1);
    assert_eq!(join_transitions[0].from, InstanceStatus::Pending);
    assert_eq!(join_transitions[0].to, InstanceStatus::Skipped);
    assert_eq!(
        join_transitions[0].reason.as_deref(),
        Some("all_sources_blocked")
    );
    assert!(sink.timed_out().is_empty(), "no NodeTimedOut expected");
    common::assert_lifecycle_once(sink);
}

#[tokio::test]
async fn blocked_edge_settles_first_then_passed_edge_must_cascade() {
    // a 的边被拦截且先结算（立即完成）；b 的放行边后结算（延迟 50ms）。
    let executor = Arc::new(
        FakeExecutor::new()
            .respond("a", json!({"ok": false}))
            .delay("b", Duration::from_millis(50), json!({"b": 1})),
    );
    let started = Instant::now();
    let (outcome, sink) = common::run_with_sink(spec(), executor).await;
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "must complete promptly, not wait for deadlock grace"
    );
    assert_skipped_to_completion(&sink, &outcome);
}

#[tokio::test]
async fn passed_edge_settles_first_then_blocked_edge_must_cascade() {
    // b 的放行边先结算（立即完成）；a 的拦截边后结算（延迟 50ms）。
    let executor = Arc::new(
        FakeExecutor::new()
            .delay("a", Duration::from_millis(50), json!({"ok": false}))
            .respond("b", json!({"b": 1})),
    );
    let (outcome, sink) = common::run_with_sink(spec(), executor).await;
    assert_skipped_to_completion(&sink, &outcome);
}
