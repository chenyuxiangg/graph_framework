//! `subgraph` 节点运行期拒绝（04 §4.4.4 / AC-04 附加 2）：
//! 加载校验通过、`RunStarted` 后立即 `NODE_TYPE_UNSUPPORTED`，不创建 subgraph 实例。

mod common;

use std::collections::HashMap;
use std::sync::Arc;

use zgraph_kernel::{FakeExecutor, RunStatus};

#[tokio::test]
async fn subgraph_run_is_rejected_before_instance_creation() {
    let graph = common::compile_fixture("valid_subgraph.json");
    let executor = Arc::new(FakeExecutor::new());
    let (outcome, sink) =
        common::run_with_sink(common::spec(graph, HashMap::new()), executor).await;

    assert_eq!(outcome.status, RunStatus::Failed);
    let error = outcome.error.expect("run error");
    assert_eq!(error.code, "NODE_TYPE_UNSUPPORTED");
    assert!(
        error.message.contains("subgraph"),
        "message: {}",
        error.message
    );
    assert_eq!(outcome.output, None);

    // RunStarted 第一、RunFinished 最后且恰一次。
    common::assert_lifecycle_once(&sink);
    let finished = sink.finished().into_iter().next().expect("finished");
    assert_eq!(finished.status, RunStatus::Failed);
    assert_eq!(finished.error.expect("error").code, "NODE_TYPE_UNSUPPORTED");

    // 不创建任何实例（尤其不是 subgraph 实例）。
    assert!(
        sink.snapshots().is_empty(),
        "no instances may be created before the subgraph rejection: {:?}",
        sink.snapshots()
    );
    assert!(sink.gate_results().is_empty());
    assert!(sink.timed_out().is_empty());
}
