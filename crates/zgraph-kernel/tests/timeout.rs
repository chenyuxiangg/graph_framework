//! 超时（D1：kill 语义）：TimedOut → run Failed、NodeTimedOut 事件、
//! `abandoned_tasks` 恒 0、`killed_workers` 计数、`NodeAbandoned` 不产生。

mod common;

use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::json;
use zgraph_kernel::{FakeExecutor, InstanceStatus, NodeError, RunStatus};

#[tokio::test]
async fn node_timeout_kills_and_fails_run_quickly() {
    let options = common::options_with(|options| {
        options.tick_interval_ms = 10;
        options.deadlock_grace_ms = 5_000;
    });
    let spec = {
        let mut spec = common::linear_spec_with_work_timeout(100);
        spec.options = options;
        spec
    };
    let executor = Arc::new(FakeExecutor::new().delay(
        "work",
        Duration::from_secs(30),
        json!({"never": true}),
    ));

    let started = Instant::now();
    let (outcome, sink) = common::run_with_sink(spec, executor).await;
    let elapsed = started.elapsed();

    assert!(
        elapsed < Duration::from_secs(5),
        "run must not wait for the 30s executor (took {elapsed:?})"
    );
    assert_eq!(outcome.status, RunStatus::Failed);
    assert_eq!(outcome.error.expect("run error").code, "NODE_TIMEOUT");
    assert_eq!(
        common::last_status(&sink, "work"),
        Some(InstanceStatus::TimedOut)
    );
    let timed_out = sink.timed_out();
    assert_eq!(timed_out.len(), 1, "NodeTimedOut event exactly once");
    assert!(
        timed_out[0].elapsed_ms >= 90,
        "elapsed: {}",
        timed_out[0].elapsed_ms
    );
    assert!(timed_out[0].at.ends_with('Z'));
    assert_eq!(timed_out[0].run_id, outcome.run_id);

    // D1：不产生 NodeAbandoned、abandoned_tasks 恒 0、killed_workers 计数。
    assert!(
        sink.abandoned().is_empty(),
        "NodeAbandoned must not be produced"
    );
    assert_eq!(outcome.metrics.abandoned_tasks, 0);
    assert!(
        outcome.metrics.killed_workers >= 1,
        "timeout execution task counted as killed: {:?}",
        outcome.metrics
    );
    common::assert_lifecycle_once(&sink);
    let finished = sink.finished().into_iter().next().expect("finished");
    assert_eq!(finished.status, RunStatus::Failed);
    assert_eq!(
        finished.metrics.killed_workers,
        outcome.metrics.killed_workers
    );
    // end 未执行。
    assert_eq!(common::last_status(&sink, "done"), None);
}

#[tokio::test]
async fn timed_out_transition_snapshot_carries_timeout_error() {
    // REV-007/timeout 修复：deadline 触发时实例 error/error_code 必须随迁移 snapshot
    // 进入 trace（否则 instances.error/error_code、transitions.error_code/error 全 NULL）。
    let options = common::options_with(|options| {
        options.tick_interval_ms = 10;
        options.deadlock_grace_ms = 5_000;
    });
    let mut spec = common::linear_spec_with_work_timeout(100);
    spec.options = options;
    let executor = Arc::new(FakeExecutor::new().delay("work", Duration::from_secs(30), json!({})));
    let (outcome, sink) = common::run_with_sink(spec, executor).await;
    assert_eq!(outcome.status, RunStatus::Failed);
    assert_eq!(outcome.error.expect("run error").code, "NODE_TIMEOUT");

    // 迁移记录：Running -> TimedOut。
    let transition = sink
        .transitions()
        .into_iter()
        .find(|record| record.node_id == "work" && record.to == InstanceStatus::TimedOut)
        .expect("TimedOut transition recorded");
    assert_eq!(transition.from, InstanceStatus::Running);

    // 迁移时传入的快照必须带 Timeout 变体 + error_code。
    let snapshot = common::snapshots_of(&sink, "work")
        .into_iter()
        .find(|snapshot| snapshot.status == InstanceStatus::TimedOut)
        .expect("TimedOut snapshot");
    assert_eq!(snapshot.error_code.as_deref(), Some("NODE_TIMEOUT"));
    let timed_out_event = sink.timed_out().into_iter().next().expect("NodeTimedOut");
    match snapshot.error {
        Some(zgraph_kernel::NodeError::Timeout { elapsed_ms }) => {
            assert!(elapsed_ms >= 90, "elapsed_ms: {elapsed_ms}");
            assert_eq!(
                elapsed_ms, timed_out_event.elapsed_ms,
                "snapshot error elapsed_ms matches NodeTimedOut event"
            );
        }
        other => panic!("expected NodeError::Timeout, got {other:?}"),
    }

    // run 级错误仍为 NODE_TIMEOUT；D1 语义不变。
    assert_eq!(outcome.metrics.abandoned_tasks, 0);
    assert!(outcome.metrics.killed_workers >= 1);
    assert!(sink.abandoned().is_empty());
}

const AGENT_TIMEOUT_JSON: &str = r#"{
  "version": "1.0",
  "metadata": {"name": "agent-timeout"},
  "entry_points": ["start"],
  "nodes": [
    {"id": "start", "type": "start", "inputs": [], "config": {}},
    {"id": "agent", "type": "agent_call",
     "inputs": [{"name": "i", "type": "any", "required": true}],
     "config": {"agent_id": "agent-a"}},
    {"id": "done", "type": "end",
     "inputs": [{"name": "r", "type": "any", "required": true}], "config": {}}
  ],
  "edges": [
    {"from": "start", "to_node": "agent", "to_input_port": "i"},
    {"from": "agent", "to_node": "done", "to_input_port": "r"}
  ]
}"#;

#[tokio::test]
async fn executor_reported_timeout_snapshot_carries_timeout_error() {
    // ISSUE-086：`agent_call` 执行器自报 Timeout 走 `fail_timeout()`（不经 watchdog），
    // 迁移前同样必须回填实例 error/error_code，与 watchdog 路径落库口径一致
    // （06 §6.2.5：instances.error/error_code 取自迁移 snapshot）。
    let graph = common::compile_text(AGENT_TIMEOUT_JSON);
    let executor =
        Arc::new(FakeExecutor::new().fail("agent", NodeError::Timeout { elapsed_ms: 12 }));
    let (outcome, sink) =
        common::run_with_sink(common::spec(graph, common::entry_inputs(&[])), executor).await;

    assert_eq!(outcome.status, RunStatus::Failed);
    assert_eq!(outcome.error.expect("run error").code, "NODE_TIMEOUT");
    assert_eq!(
        common::last_status(&sink, "agent"),
        Some(InstanceStatus::TimedOut)
    );

    // 迁移记录：Running -> TimedOut。
    let transition = sink
        .transitions()
        .into_iter()
        .find(|record| record.node_id == "agent" && record.to == InstanceStatus::TimedOut)
        .expect("TimedOut transition recorded");
    assert_eq!(transition.from, InstanceStatus::Running);

    // 迁移时传入的快照必须带 Timeout 变体 + error_code（非 NULL）。
    let snapshot = common::snapshots_of(&sink, "agent")
        .into_iter()
        .find(|snapshot| snapshot.status == InstanceStatus::TimedOut)
        .expect("TimedOut snapshot");
    assert_eq!(snapshot.error_code.as_deref(), Some("NODE_TIMEOUT"));
    let timed_out_event = sink.timed_out().into_iter().next().expect("NodeTimedOut");
    match snapshot.error {
        Some(NodeError::Timeout { elapsed_ms }) => {
            assert_eq!(elapsed_ms, 12, "executor-reported elapsed_ms preserved");
            assert_eq!(
                elapsed_ms, timed_out_event.elapsed_ms,
                "snapshot error elapsed_ms matches NodeTimedOut event"
            );
        }
        other => panic!("expected NodeError::Timeout, got {other:?}"),
    }
}
