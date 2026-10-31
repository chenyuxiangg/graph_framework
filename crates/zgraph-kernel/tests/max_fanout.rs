//! W2b：运行期每激活扇出上限（`NODE_FANOUT_EXCEEDED` / DR-004 §3）。
//!
//! 静态层（DSL R16）在加载期就拒绝 `out_degree > max_fanout` 的图，因此运行期
//! 防御层只能用 `test_hooks.force_max_fanout` 构造反例（或程序化构图）。
//! 需要 `feature = "test-hooks"`（见 `Cargo.toml` 的 `[[test]] required-features`）。
//!
//! 阶段 1：DSL `max_fanout` 字段尚未合入本基线，全部用 inline DSL JSON +
//! `force_max_fanout` 驱动；编译映射（DSL → `CompiledNode.max_fanout`）在阶段 2 补。

mod common;

use std::sync::Arc;

use zgraph_kernel::{
    FakeExecutor, InstanceStatus, RunStatus, TestHooks, CODE_NODE_FANOUT_EXCEEDED,
};

/// 扇出图：`start -> fan`，`fan` 有两条出边（`fan -> a`、`fan -> b`），
/// `a`/`b` 各自汇入 `done` 的两个端口。
const FANOUT_JSON: &str = r#"{
  "version": "1.0",
  "metadata": {"name": "max-fanout"},
  "entry_points": ["start"],
  "nodes": [
    {"id": "start", "type": "start", "inputs": [], "config": {}},
    {"id": "fan", "type": "python_node",
     "inputs": [{"name": "i", "type": "any", "required": true}],
     "config": {"module": "workers.fanout", "function": "fan"}},
    {"id": "a", "type": "python_node",
     "inputs": [{"name": "i", "type": "any", "required": true}],
     "config": {"module": "workers.fanout", "function": "a"}},
    {"id": "b", "type": "python_node",
     "inputs": [{"name": "i", "type": "any", "required": true}],
     "config": {"module": "workers.fanout", "function": "b"}},
    {"id": "done", "type": "end",
     "inputs": [{"name": "ra", "type": "any", "required": true},
                {"name": "rb", "type": "any", "required": true}],
     "config": {}}
  ],
  "edges": [
    {"from": "start", "to_node": "fan", "to_input_port": "i"},
    {"from": "fan", "to_node": "a", "to_input_port": "i"},
    {"from": "fan", "to_node": "b", "to_input_port": "i"},
    {"from": "a", "to_node": "done", "to_input_port": "ra"},
    {"from": "b", "to_node": "done", "to_input_port": "rb"}
  ]
}"#;

/// 扇出图 + 强制上限钩子的 `RunSpec`。
fn fanout_spec(force_max_fanout: Option<u32>) -> zgraph_kernel::RunSpec {
    let options = common::options_with(|options| {
        options.tick_interval_ms = 10;
        options.with_test_hooks(TestHooks {
            force_max_fanout,
            ..TestHooks::default()
        });
    });
    let mut spec = common::spec(common::compile_text(FANOUT_JSON), common::entry_inputs(&[]));
    spec.options = options;
    spec
}

/// `force=Some(1)` + 2 条放行出边 → Failed：Crashed(attempt=1) + 新错误码，
/// 该激活无 GateResult、不应用任何边（`a`/`b` 从未实例化）。
#[tokio::test]
async fn forced_limit_one_rejects_activation_before_any_edge() {
    let spec = fanout_spec(Some(1));
    let executor = Arc::new(FakeExecutor::new());
    let (outcome, sink) = common::run_with_sink(spec, executor).await;

    assert_eq!(outcome.status, RunStatus::Failed);
    let error = outcome.error.expect("run error");
    assert_eq!(error.code, CODE_NODE_FANOUT_EXCEEDED);
    assert_eq!(
        error.message,
        "node 'fan' fan-out 2 exceeded max_fanout (1) in a single activation"
    );

    // RunFinished.error 与 outcome 一致（且恰一条 RunFinished）。
    let finished = sink.finished();
    assert_eq!(finished.len(), 1, "RunFinished exactly once");
    assert_eq!(finished[0].status, RunStatus::Failed);
    assert_eq!(
        finished[0].error.as_ref().expect("RunFinished.error").code,
        CODE_NODE_FANOUT_EXCEEDED
    );

    // 源节点：原实例 Completed 之后，新建 attempt=1 的 Crashed 实例。
    let fan_snapshots = common::snapshots_of(&sink, "fan");
    let crashed = fan_snapshots.last().expect("fan snapshot");
    assert_eq!(crashed.status, InstanceStatus::Crashed);
    assert_eq!(crashed.attempt, 1);
    assert_eq!(
        crashed.error_code.as_deref(),
        Some(CODE_NODE_FANOUT_EXCEEDED)
    );

    // 该激活不产生任何 GateResult（只有 start 自己的 `start -> fan` 一条）。
    let fan_gate_results: Vec<_> = sink
        .gate_results()
        .into_iter()
        .filter(|record| record.node_id == "fan")
        .collect();
    assert!(
        fan_gate_results.is_empty(),
        "rejected activation must not emit GateResult: {fan_gate_results:?}"
    );
    assert_eq!(
        sink.gate_results().len(),
        1,
        "only start's edge is recorded"
    );

    // 不应用任何边：扇出目标从未实例化。
    assert!(common::snapshots_of(&sink, "a").is_empty());
    assert!(common::snapshots_of(&sink, "b").is_empty());

    common::assert_lifecycle_once(&sink);
}

/// 边界 `actual == limit`（force=2，actual=2）→ Completed。
#[tokio::test]
async fn forced_limit_equal_to_actual_completes() {
    let spec = fanout_spec(Some(2));
    let executor = Arc::new(FakeExecutor::new());
    let (outcome, sink) = common::run_with_sink(spec, executor).await;

    assert_eq!(
        outcome.status,
        RunStatus::Completed,
        "error: {:?}",
        outcome.error
    );
    assert_eq!(
        common::last_status(&sink, "fan"),
        Some(InstanceStatus::Completed)
    );
    assert_eq!(
        common::last_status(&sink, "a"),
        Some(InstanceStatus::Completed)
    );
    assert_eq!(
        common::last_status(&sink, "b"),
        Some(InstanceStatus::Completed)
    );
    assert_eq!(
        common::last_status(&sink, "done"),
        Some(InstanceStatus::Completed)
    );
    common::assert_lifecycle_once(&sink);
}

/// 不设 force 钩子 → 无上限（v0.1.0 行为）：2 条出边照常投递，run Completed。
#[tokio::test]
async fn no_hook_keeps_v0_1_0_behavior() {
    let spec = fanout_spec(None);
    let executor = Arc::new(FakeExecutor::new());
    let (outcome, sink) = common::run_with_sink(spec, executor).await;

    assert_eq!(
        outcome.status,
        RunStatus::Completed,
        "error: {:?}",
        outcome.error
    );
    // 5 条边全部放行：start->fan、fan->a、fan->b、a->done、b->done。
    assert_eq!(sink.gate_results().len(), 5);
    assert!(sink.gate_results().iter().all(|record| record.passed));
    common::assert_lifecycle_once(&sink);
}
