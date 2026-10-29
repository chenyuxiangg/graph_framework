//! Pause/Resume/Cancel（03 §3.8）：幂等、状态订阅、终态后 no-op、RunFinished 恰一次。

mod common;

use std::sync::Arc;
use std::time::Duration;

use serde_json::json;
use zgraph_kernel::{
    start_run, FakeExecutor, InstanceStatus, MemoryTraceSink, NodeError, NodeExecutor, RunStatus,
};

const CHAIN_JSON: &str = r#"{
  "version": "1.0",
  "metadata": {"name": "chain"},
  "entry_points": ["start"],
  "nodes": [
    {"id": "start", "type": "start", "inputs": [], "config": {}},
    {"id": "a", "type": "python_node",
     "inputs": [{"name": "i", "type": "any", "required": true}],
     "config": {"module": "workers.chain", "function": "a"}},
    {"id": "b", "type": "python_node",
     "inputs": [{"name": "i", "type": "any", "required": true}],
     "config": {"module": "workers.chain", "function": "b"}},
    {"id": "done", "type": "end",
     "inputs": [{"name": "r", "type": "any", "required": true}], "config": {}}
  ],
  "edges": [
    {"from": "start", "to_node": "a", "to_input_port": "i"},
    {"from": "a", "to_node": "b", "to_input_port": "i"},
    {"from": "b", "to_node": "done", "to_input_port": "r"}
  ]
}"#;

fn chain_spec() -> zgraph_kernel::RunSpec {
    common::spec(common::compile_text(CHAIN_JSON), common::entry_inputs(&[]))
}

#[tokio::test]
async fn pause_freezes_dispatch_and_resume_completes() {
    let executor = Arc::new(
        FakeExecutor::new()
            .delay("a", Duration::from_millis(200), json!({"a": 1}))
            .respond("b", json!({"b": 1})),
    );
    let sink = Arc::new(MemoryTraceSink::new());
    let handle = start_run(None, chain_spec(), executor, sink.clone()).expect("started");
    let mut status_rx = handle.subscribe_status();

    common::wait_for_running(&sink, "a").await;
    assert_eq!(handle.status(), RunStatus::Running);
    assert!(handle.pause(), "pause command accepted");
    common::wait_for_run_status(&mut status_rx, RunStatus::Paused).await;

    // 在飞 a 继续跑完；投递仍应用（b 变 Ready），但 Paused 期间不派发。
    tokio::time::sleep(Duration::from_millis(350)).await;
    assert_eq!(handle.status(), RunStatus::Paused);
    assert!(
        !common::statuses(&sink, "b").contains(&InstanceStatus::Running),
        "b must not be dispatched while paused: {:?}",
        common::statuses(&sink, "b")
    );
    assert_eq!(common::last_status(&sink, "b"), Some(InstanceStatus::Ready));
    assert!(sink.finished().is_empty(), "paused run must not finish");

    assert!(handle.resume(), "resume command accepted");
    let outcome = handle.wait().await;
    assert_eq!(outcome.status, RunStatus::Completed);
    assert_eq!(
        common::last_status(&sink, "b"),
        Some(InstanceStatus::Completed)
    );
    assert_eq!(outcome.output, Some(json!({"r": {"b": 1}})));
    common::assert_lifecycle_once(&sink);
}

#[tokio::test]
async fn cancel_is_idempotent_and_finishes_exactly_once() {
    let text = r#"{
      "version": "1.0",
      "metadata": {"name": "cancel"},
      "entry_points": ["start"],
      "nodes": [
        {"id": "start", "type": "start", "inputs": [], "config": {}},
        {"id": "slow", "type": "python_node",
         "inputs": [{"name": "i", "type": "any", "required": true}],
         "config": {"module": "workers.cancel", "function": "slow"}},
        {"id": "done", "type": "end",
         "inputs": [{"name": "r", "type": "any", "required": true}], "config": {}}
      ],
      "edges": [
        {"from": "start", "to_node": "slow", "to_input_port": "i"},
        {"from": "slow", "to_node": "done", "to_input_port": "r"}
      ]
    }"#;
    let graph = common::compile_text(text);
    let executor = Arc::new(FakeExecutor::new().delay("slow", Duration::from_secs(30), json!({})));
    let sink = Arc::new(MemoryTraceSink::new());
    let handle = start_run(
        None,
        common::spec(graph, common::entry_inputs(&[])),
        executor,
        sink.clone(),
    )
    .expect("started");
    let status_rx = handle.subscribe_status();
    common::wait_for_running(&sink, "slow").await;

    assert!(handle.cancel(), "first cancel accepted");
    assert!(handle.cancel(), "second cancel accepted (queued no-op)");
    assert!(handle.cancel(), "third cancel is a no-op");

    let outcome = handle.wait().await;
    assert_eq!(outcome.status, RunStatus::Cancelled);
    assert_eq!(outcome.error, None);
    assert_eq!(outcome.output, None);
    assert_eq!(
        common::last_status(&sink, "slow"),
        Some(InstanceStatus::Cancelled)
    );
    assert_eq!(status_rx.borrow().clone(), RunStatus::Cancelled);
    common::assert_lifecycle_once(&sink);
    let finished = sink.finished().into_iter().next().expect("finished");
    assert_eq!(finished.status, RunStatus::Cancelled);
    assert!(outcome.metrics.killed_workers >= 1);
    // done 未被创建。
    assert_eq!(common::last_status(&sink, "done"), None);
}

#[tokio::test]
async fn control_after_terminal_is_noop() {
    let executor = Arc::new(FakeExecutor::new());
    let sink = Arc::new(MemoryTraceSink::new());
    let handle = start_run(
        None,
        common::spec(
            common::compile_text(common::LINEAR_JSON),
            common::entry_inputs(&[("start", "x", json!(1))]),
        ),
        executor,
        sink.clone(),
    )
    .expect("started");
    let mut status_rx = handle.subscribe_status();
    common::wait_for_run_status(&mut status_rx, RunStatus::Completed).await;

    // 终态后控制命令 no-op；不得把 Completed 改成 Cancelled/Failed。
    let _ = handle.pause();
    let _ = handle.resume();
    let _ = handle.cancel();
    let outcome = handle.wait().await;
    assert_eq!(outcome.status, RunStatus::Completed);
    common::assert_lifecycle_once(&sink);
    assert_eq!(
        sink.finished().into_iter().next().expect("finished").status,
        RunStatus::Completed
    );
}

#[tokio::test]
async fn cooperative_cancel_from_executor_returns_cancelled_error() {
    /// 执行器在取消标志置位后返回 `NodeError::Cancelled`（协作取消路径）。
    struct CooperativeCancel;

    impl NodeExecutor for CooperativeCancel {
        fn execute<'a>(
            &'a self,
            execution: zgraph_kernel::NodeExecution<'a>,
        ) -> std::pin::Pin<
            Box<dyn std::future::Future<Output = Result<serde_json::Value, NodeError>> + Send + 'a>,
        > {
            Box::pin(async move {
                for _ in 0..100 {
                    execution.ctx.check_cancelled()?;
                    tokio::time::sleep(Duration::from_millis(5)).await;
                }
                Ok(json!({}))
            })
        }
    }

    let graph = common::compile_text(CHAIN_JSON);
    let sink = Arc::new(MemoryTraceSink::new());
    let handle = start_run(
        None,
        common::spec(graph, common::entry_inputs(&[])),
        Arc::new(CooperativeCancel),
        sink.clone(),
    )
    .expect("started");
    common::wait_for_running(&sink, "a").await;
    assert!(handle.cancel());
    let outcome = handle.wait().await;
    assert_eq!(outcome.status, RunStatus::Cancelled);
    common::assert_lifecycle_once(&sink);
}
