//! 自环：每次投递新建实例 attempt+1；超过 `max_attempts` → 新实例 Crashed → run Failed。
//!
//! 另覆盖 ISSUE-061（REV-026）：被 gate 拦截的自环投递**不得**创建 attempt+1
//! 幻影实例，也不得因"自环超过 `max_attempts`"把本应 Completed 的 run 误判 Failed。

mod common;

use std::collections::{BTreeMap, VecDeque};
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};

use serde_json::{json, Value};
use zgraph_kernel::{
    FakeExecutor, InstanceStatus, MemoryTraceSink, NodeError, NodeExecution, NodeExecutor,
    RunStatus,
};

const SELF_LOOP_JSON: &str = r#"{
  "version": "1.0",
  "metadata": {"name": "self-loop"},
  "entry_points": ["start"],
  "nodes": [
    {"id": "start", "type": "start", "inputs": [], "config": {}},
    {"id": "looper", "type": "python_node",
     "inputs": [
       {"name": "x", "type": "any", "required": true},
       {"name": "y", "type": "any", "required": true}
     ],
     "trigger": "any",
     "config": {"module": "workers.loop", "function": "run",
                "retry": {"max_attempts": 2}}}
  ],
  "edges": [
    {"from": "start", "to_node": "looper", "to_input_port": "x"},
    {"from": "looper", "to_node": "looper", "to_input_port": "y",
     "gate": {"function": "compare", "params": {"path": "again", "op": "eq", "value": true}}}
  ]
}"#;

/// 把 `SELF_LOOP_JSON` 的 `retry.max_attempts` 替换为给定值。
fn self_loop_json(max_attempts: u32) -> String {
    SELF_LOOP_JSON.replace(
        r#""max_attempts": 2"#,
        &format!(r#""max_attempts": {max_attempts}"#),
    )
}

/// 按调用顺序返回预设 output 的脚本执行器（用尽后返回 `{"again": false}`）。
///
/// 用于构造"第一次自环放行、第二次被 gate 拦截"的场景：`FakeExecutor`
/// 只能给每个节点一个固定 output，无法表达逐次变化。
struct ScriptedExecutor {
    outputs: Mutex<VecDeque<Value>>,
}

impl ScriptedExecutor {
    fn new(outputs: impl IntoIterator<Item = Value>) -> Self {
        Self {
            outputs: Mutex::new(outputs.into_iter().collect()),
        }
    }
}

impl NodeExecutor for ScriptedExecutor {
    fn execute<'a>(
        &'a self,
        _execution: NodeExecution<'a>,
    ) -> Pin<Box<dyn Future<Output = Result<Value, NodeError>> + Send + 'a>> {
        let output = self
            .outputs
            .lock()
            .expect("script lock")
            .pop_front()
            .unwrap_or_else(|| json!({"again": false}));
        Box::pin(async move { Ok(output) })
    }
}

/// 某节点每个 attempt 的**最终**状态（快照按迁移逐条记录，这里按 attempt 折叠）。
fn final_statuses(sink: &MemoryTraceSink, node_id: &str) -> Vec<(u32, InstanceStatus)> {
    let mut by_attempt: BTreeMap<u32, InstanceStatus> = BTreeMap::new();
    for snapshot in common::snapshots_of(sink, node_id) {
        by_attempt.insert(snapshot.attempt, snapshot.status);
    }
    by_attempt.into_iter().collect()
}

/// 自环边（`edge_index = 1`）的 gate 记录，按 EdgeIdx 过滤。
fn self_loop_gate_passed(sink: &MemoryTraceSink) -> Vec<bool> {
    sink.gate_results()
        .into_iter()
        .filter(|record| record.edge_index == 1)
        .map(|record| record.passed)
        .collect()
}

#[tokio::test]
async fn self_loop_beyond_max_attempts_fails_run() {
    let graph = common::compile_text(SELF_LOOP_JSON);
    let executor = Arc::new(FakeExecutor::new().respond("looper", json!({"again": true})));
    let (outcome, sink) =
        common::run_with_sink(common::spec(graph, common::entry_inputs(&[])), executor).await;

    assert_eq!(outcome.status, RunStatus::Failed);
    let error = outcome.error.expect("run error");
    assert_eq!(error.code, "NODE_INTERNAL");
    assert!(
        error.message.contains("max_attempts"),
        "message: {}",
        error.message
    );
    assert_eq!(outcome.output, None);

    let looper_snapshots = common::snapshots_of(&sink, "looper");
    let attempts: Vec<u32> = {
        let mut values: Vec<u32> = looper_snapshots
            .iter()
            .map(|snapshot| snapshot.attempt)
            .collect();
        values.sort_unstable();
        values.dedup();
        values
    };
    assert_eq!(
        attempts,
        vec![0, 1, 2],
        "attempt 0/1 executed, attempt 2 crashed"
    );
    // 两次正常执行 Completed，第三次创建即 Crashed。
    let completed = looper_snapshots
        .iter()
        .filter(|snapshot| snapshot.status == InstanceStatus::Completed)
        .count();
    assert_eq!(completed, 2);
    let crashed = looper_snapshots
        .iter()
        .filter(|snapshot| snapshot.status == InstanceStatus::Crashed)
        .collect::<Vec<_>>();
    assert_eq!(crashed.len(), 1);
    assert_eq!(crashed[0].error_code.as_deref(), Some("NODE_INTERNAL"));
    common::assert_lifecycle_once(&sink);
}

/// ISSUE-061 最小反例：自环 gate 恒 false，`max_attempts=2/3`。
///
/// 修复前：`apply_publish` 在 gate 判定前 `resolve_target` → 被拦截的投递仍
/// 创建 attempt+1 幻影实例（Pending→Skipped）；`max_attempts=2` 时 `resolve_self_loop`
/// 先判超限 → 本应 Completed 的 run 误报 `Failed/NODE_INTERNAL`。
#[tokio::test]
async fn blocked_self_loop_delivery_creates_no_phantom_instance() {
    for max_attempts in [2_u32, 3] {
        let graph = common::compile_text(&self_loop_json(max_attempts));
        let executor = Arc::new(ScriptedExecutor::new([json!({"again": false})]));
        let (outcome, sink) =
            common::run_with_sink(common::spec(graph, common::entry_inputs(&[])), executor).await;

        assert_eq!(
            outcome.status,
            RunStatus::Completed,
            "max_attempts={max_attempts}: {:?}",
            outcome.error
        );
        assert!(
            outcome.error.is_none(),
            "max_attempts={max_attempts}: blocked delivery must not fail run"
        );
        assert_eq!(
            final_statuses(&sink, "looper"),
            vec![(0, InstanceStatus::Completed)],
            "max_attempts={max_attempts}: blocked self-loop must not create attempt+1"
        );
        assert_eq!(
            self_loop_gate_passed(&sink),
            vec![false],
            "max_attempts={max_attempts}: blocked delivery still records one GateResult"
        );
        assert!(
            sink.snapshots()
                .iter()
                .all(|snapshot| snapshot.status != InstanceStatus::Crashed),
            "max_attempts={max_attempts}: no Crashed instance expected"
        );
        common::assert_lifecycle_once(&sink);
    }
}

/// ISSUE-061 原始反例（REV-015）：`max_attempts=2/3`，自环第一次放行（attempt1）、
/// 第二次被拦截。
///
/// - `max_attempts=2`：修复前 attempt1 的**被拦截**投递先 `resolve_target` →
///   attempt 2 ≥ 2 → 幻影 Crashed + run `Failed/NODE_INTERNAL`；修复后 run 应 Completed。
/// - `max_attempts=3`：修复前出现 attempt2 Skipped 幻影实例；修复后仅 attempt 0/1。
#[tokio::test]
async fn blocked_self_loop_delivery_exactly_at_max_attempts_still_completes() {
    for max_attempts in [2_u32, 3] {
        let graph = common::compile_text(&self_loop_json(max_attempts));
        let executor = Arc::new(ScriptedExecutor::new([
            json!({"again": true}),
            json!({"again": false}),
        ]));
        let (outcome, sink) =
            common::run_with_sink(common::spec(graph, common::entry_inputs(&[])), executor).await;

        assert_eq!(
            outcome.status,
            RunStatus::Completed,
            "max_attempts={max_attempts}: {:?}",
            outcome.error
        );
        assert!(outcome.error.is_none());
        assert_eq!(
            final_statuses(&sink, "looper"),
            vec![
                (0, InstanceStatus::Completed),
                (1, InstanceStatus::Completed)
            ],
            "max_attempts={max_attempts}: only the two executed attempts exist"
        );
        assert_eq!(
            self_loop_gate_passed(&sink),
            vec![true, false],
            "max_attempts={max_attempts}: first delivery passed, second blocked"
        );
        assert!(
            sink.snapshots()
                .iter()
                .all(|snapshot| snapshot.status != InstanceStatus::Crashed),
            "max_attempts={max_attempts}: no Crashed instance expected"
        );
        common::assert_lifecycle_once(&sink);
    }
}
