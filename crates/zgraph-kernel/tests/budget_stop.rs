//! v0.5 F3a2（DR-018 §3）：预算超限 → node/instance Failed、run Failed、
//! `error_code=BUDGET_EXCEEDED`、`stop_reason=Some("budget_exceeded")`、无 Running 残留。

mod common;

use std::sync::Arc;

use zgraph_kernel::{FakeExecutor, InstanceStatus, NodeError, RunStatus, CODE_BUDGET_EXCEEDED};

#[tokio::test]
async fn budget_exceeded_maps_to_failed_run_with_domain_code_and_stop_reason() {
    let executor = Arc::new(FakeExecutor::new().fail(
        "work",
        NodeError::BudgetExceeded {
            message: "run token budget exceeded: used 100 >= max_tokens 100".to_string(),
        },
    ));
    let (outcome, sink) = common::run_with_sink(common::linear_spec(), executor).await;

    assert_eq!(outcome.status, RunStatus::Failed, "{:?}", outcome.error);
    let error = outcome.error.expect("failed run carries error");
    assert_eq!(error.code, CODE_BUDGET_EXCEEDED);
    assert!(
        error.message.contains("budget"),
        "message: {}",
        error.message
    );
    assert_eq!(
        outcome.stop_reason.as_deref(),
        Some("budget_exceeded"),
        "RunFinished.stop_reason must map budget stop"
    );
    assert_eq!(
        common::last_status(&sink, "work"),
        Some(InstanceStatus::Crashed),
        "node must be terminal (Failed), never left Running"
    );
    // snapshots 是历史记录：按 node 取**最后**一次状态，确保没有终态残留 Running。
    let mut last_by_node = std::collections::BTreeMap::new();
    for snapshot in sink.snapshots() {
        last_by_node.insert(snapshot.node_id, snapshot.status);
    }
    assert!(
        last_by_node
            .values()
            .all(|status| *status != InstanceStatus::Running),
        "no instance may remain Running after budget stop: {last_by_node:?}"
    );
}

#[tokio::test]
async fn non_budget_failure_has_no_budget_stop_reason() {
    let executor = Arc::new(FakeExecutor::new().fail(
        "work",
        NodeError::BusinessError {
            code: "BIZ_X".to_string(),
            message: "boom".to_string(),
        },
    ));
    let (outcome, _sink) = common::run_with_sink(common::linear_spec(), executor).await;
    assert_eq!(outcome.status, RunStatus::Failed);
    assert_eq!(outcome.stop_reason, None);
}
