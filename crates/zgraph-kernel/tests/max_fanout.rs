//! W2b：运行期每激活扇出上限（`NODE_FANOUT_EXCEEDED` / DR-004 §3）。
//!
//! 静态层（DSL R16）在加载期就拒绝 `out_degree > max_fanout` 的图，因此运行期
//! 防御层只能用 `test_hooks.force_max_fanout` 构造反例（或程序化构图）。
//! 需要 `feature = "test-hooks"`（见 `Cargo.toml` 的 `[[test]] required-features`）。
//!
//! 覆盖：普通边超限 / 边界 `actual == limit` / 默认无上限，以及 defer + force 组合
//!（`actual` 只计 `passed`：defer 边放行计入、defer 边 gate=false 不计数但照常写
//! `GateResult(deferred=true)`）。
//!
//! 本文件聚焦**运行期**防御层（inline DSL JSON + `force_max_fanout` 反例，以及
//! `valid_max_fanout.json` 的声明上限边界正例）；DSL `max_fanout` → `CompiledNode`
//! 的编译映射断言在 `tests/compile.rs`。

mod common;

use std::sync::Arc;

use serde_json::json;
use zgraph_kernel::{
    FakeExecutor, InstanceStatus, NodeError, RunStatus, TestHooks, CODE_NODE_FANOUT_EXCEEDED,
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

/// inline DSL 图 + 强制上限钩子的 `RunSpec`。
fn spec_with_force(json: &str, force_max_fanout: Option<u32>) -> zgraph_kernel::RunSpec {
    let options = common::options_with(|options| {
        options.tick_interval_ms = 10;
        options.with_test_hooks(TestHooks {
            force_max_fanout,
            ..TestHooks::default()
        });
    });
    let mut spec = common::spec(common::compile_text(json), common::entry_inputs(&[]));
    spec.options = options;
    spec
}

/// `force=Some(1)` + 2 条放行出边 → Failed：Crashed(attempt=1) + 新错误码，
/// 该激活无 GateResult、不应用任何边（`a`/`b` 从未实例化）。
#[tokio::test]
async fn forced_limit_one_rejects_activation_before_any_edge() {
    let spec = spec_with_force(FANOUT_JSON, Some(1));
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
    let spec = spec_with_force(FANOUT_JSON, Some(2));
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
    let spec = spec_with_force(FANOUT_JSON, None);
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

// ---------------------------------------------------------------------------
// defer + force 组合（合同 B2：`actual` 只计 `passed`；defer 边放行也计入、
// defer 边 gate=false 不计数但照常写 `GateResult(deferred=true)`）
// ---------------------------------------------------------------------------
//
// R15 要求 defer 边的源节点可重入，因此两个图都用 `p -> p` 的 defer 自环
// （gate=false，运行期只挂账）取得可重入性；`p` 用 `trigger=any` 保证
// `start -> p` 的 `task` 到达即可首次就绪（`feedback`/`again` 是环/自环端口）。

/// `p` 的 3 条出边全部 `on_gate_false="defer"`：
/// - `p -> p`（gate=false → 不 passed，只写 deferred receipt）；
/// - `p -> done`、`p -> a`（无 gate → passed）。
///
/// 超限激活里 `actual = 2`（gate=false 的 defer 边不计）。
const DEFER_FANOUT_JSON: &str = r#"{
  "version": "1.0",
  "metadata": {"name": "defer-max-fanout"},
  "entry_points": ["start"],
  "nodes": [
    {"id": "start", "type": "start", "inputs": [], "config": {}},
    {"id": "p", "type": "python_node",
     "trigger": "any",
     "inputs": [{"name": "task", "type": "any", "required": true},
                {"name": "again", "type": "any", "required": true}],
     "config": {"module": "workers.defer", "function": "p"}},
    {"id": "a", "type": "python_node",
     "inputs": [{"name": "i", "type": "any", "required": true}],
     "config": {"module": "workers.defer", "function": "a"}},
    {"id": "done", "type": "end",
     "inputs": [{"name": "r", "type": "any", "required": true}],
     "config": {}}
  ],
  "edges": [
    {"from": "start", "to_node": "p", "to_input_port": "task"},
    {"from": "p", "to_node": "p", "to_input_port": "again",
     "gate": {"function": "check_output", "params": {"path": "ok", "equals": false}},
     "on_gate_false": "defer"},
    {"from": "p", "to_node": "done", "to_input_port": "r", "on_gate_false": "defer"},
    {"from": "p", "to_node": "a", "to_input_port": "i", "on_gate_false": "defer"}
  ]
}"#;

/// `p` 的 2 条 defer 出边：`p -> p` gate=false（不计数、写 GateResult）、
/// `p -> done` 无 gate（passed）。`actual == limit` 的边界对照。
const DEFER_BOUNDARY_JSON: &str = r#"{
  "version": "1.0",
  "metadata": {"name": "defer-max-fanout-boundary"},
  "entry_points": ["start"],
  "nodes": [
    {"id": "start", "type": "start", "inputs": [], "config": {}},
    {"id": "p", "type": "python_node",
     "trigger": "any",
     "inputs": [{"name": "task", "type": "any", "required": true},
                {"name": "again", "type": "any", "required": true}],
     "config": {"module": "workers.defer", "function": "p"}},
    {"id": "done", "type": "end",
     "inputs": [{"name": "r", "type": "any", "required": true}],
     "config": {}}
  ],
  "edges": [
    {"from": "start", "to_node": "p", "to_input_port": "task"},
    {"from": "p", "to_node": "p", "to_input_port": "again",
     "gate": {"function": "check_output", "params": {"path": "ok", "equals": false}},
     "on_gate_false": "defer"},
    {"from": "p", "to_node": "done", "to_input_port": "r", "on_gate_false": "defer"}
  ]
}"#;

/// 2 条 passed 的 defer 放行 + 1 条 gate=false 的 defer 边：force=1 → Failed，
/// `actual` 精确为 2（gate=false 不计）；该激活 3 条边的 GateResult 全不产生。
#[tokio::test]
async fn defer_passed_edges_count_toward_fanout_limit() {
    let spec = spec_with_force(DEFER_FANOUT_JSON, Some(1));
    let executor = Arc::new(FakeExecutor::new().respond("p", json!({"ok": true})));
    let (outcome, sink) = common::run_with_sink(spec, executor).await;

    assert_eq!(outcome.status, RunStatus::Failed);
    let error = outcome.error.expect("run error");
    assert_eq!(error.code, CODE_NODE_FANOUT_EXCEEDED);
    assert_eq!(
        error.message,
        "node 'p' fan-out 2 exceeded max_fanout (1) in a single activation"
    );

    let p_snapshots = common::snapshots_of(&sink, "p");
    let crashed = p_snapshots.last().expect("p snapshot");
    assert_eq!(crashed.status, InstanceStatus::Crashed);
    assert_eq!(crashed.attempt, 1);
    assert_eq!(
        crashed.error_code.as_deref(),
        Some(CODE_NODE_FANOUT_EXCEEDED)
    );
    // actual 精确为 2：gate=false 的 defer 自环不计入（否则会是 3）。
    assert_eq!(
        crashed.error,
        Some(NodeError::FanoutExceeded {
            limit: 1,
            actual: 2,
        })
    );

    // 超限校验先于任何边的应用：`p` 的 3 条 defer 边都不产生 GateResult
    //（含本来会以 deferred=true 落库的 gate=false 自环）；只有 start 的 1 条。
    let p_gate_results: Vec<_> = sink
        .gate_results()
        .into_iter()
        .filter(|record| record.node_id == "p")
        .collect();
    assert!(
        p_gate_results.is_empty(),
        "rejected activation must not emit GateResult: {p_gate_results:?}"
    );
    assert_eq!(
        sink.gate_results().len(),
        1,
        "only start's edge is recorded"
    );
    assert!(common::snapshots_of(&sink, "a").is_empty());
    assert!(common::snapshots_of(&sink, "done").is_empty());
    common::assert_lifecycle_once(&sink);
}

/// 1 条 passed + 1 条 gate=false 的 defer 边：`actual == limit == 1` → Completed，
/// 且 gate=false 的 defer 边照常产生 `GateResult{passed:false, deferred:true}`。
#[tokio::test]
async fn defer_false_side_emits_gate_result_but_is_not_counted() {
    let spec = spec_with_force(DEFER_BOUNDARY_JSON, Some(1));
    let executor = Arc::new(FakeExecutor::new().respond("p", json!({"ok": true})));
    let (outcome, sink) = common::run_with_sink(spec, executor).await;

    assert_eq!(
        outcome.status,
        RunStatus::Completed,
        "error: {:?}",
        outcome.error
    );
    assert_eq!(
        common::last_status(&sink, "p"),
        Some(InstanceStatus::Completed)
    );
    assert_eq!(
        common::last_status(&sink, "done"),
        Some(InstanceStatus::Completed)
    );

    let records: Vec<_> = sink
        .gate_results()
        .into_iter()
        .filter(|record| record.node_id == "p")
        .collect();
    assert_eq!(records.len(), 2);
    assert_eq!(records.iter().filter(|record| record.passed).count(), 1);
    assert_eq!(records.iter().filter(|record| !record.passed).count(), 1);
    // passed 的 defer 边：deferred=false（deferred 只表示「本次 gate=false 且 defer」）。
    let passed = records
        .iter()
        .find(|record| record.passed)
        .expect("passed record");
    assert!(!passed.deferred);
    // gate=false 的 defer 边：deferred=true，且不参与扇出计数。
    let blocked = records
        .iter()
        .find(|record| !record.passed)
        .expect("blocked record");
    assert!(
        blocked.deferred,
        "on_gate_false=defer 的 gate=false 必须 deferred"
    );
    common::assert_lifecycle_once(&sink);
}

/// 声明上限边界正例：fixture `valid_max_fanout.json`（start out-degree=2、
/// max_fanout=2）加载期经 R16 通过；运行期 `actual == limit` → Completed。
/// 不设 force 钩子，同时证明「有 max_fanout 但未超限」不改变 v0.1.0 行为。
#[tokio::test]
async fn declared_limit_at_boundary_completes() {
    let mut spec = common::spec(
        common::compile_fixture("valid_max_fanout.json"),
        common::entry_inputs(&[]),
    );
    assert_eq!(
        spec.graph.nodes.get("start").expect("start").max_fanout,
        Some(2),
        "DSL max_fanout 必须编译进 CompiledNode"
    );
    spec.options = common::options_with(|options| options.tick_interval_ms = 10);
    let executor = Arc::new(FakeExecutor::new());
    let (outcome, sink) = common::run_with_sink(spec, executor).await;

    assert_eq!(
        outcome.status,
        RunStatus::Completed,
        "error: {:?}",
        outcome.error
    );
    assert_eq!(
        common::last_status(&sink, "start"),
        Some(InstanceStatus::Completed)
    );
    assert_eq!(
        common::last_status(&sink, "done"),
        Some(InstanceStatus::Completed)
    );
    common::assert_lifecycle_once(&sink);
}
