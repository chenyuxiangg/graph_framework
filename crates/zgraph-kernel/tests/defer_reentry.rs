//! W2a0 attempt-per-activation：自环/回边重入、按端口 buffer 继承与超限失败。
//!
//! 语义基准：DR-005 §3.2 决策矩阵行 6/7、§3.3 应用算法、§3.5 超限 message；
//! 覆盖 AC-14 a/b/e（kernel 单测口径）。

mod common;

use std::collections::{BTreeMap, HashMap, VecDeque};
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};

use serde_json::{json, Value};
use zgraph_kernel::{
    GateResultRecord, InstanceStatus, MemoryTraceSink, NodeError, NodeExecution, NodeExecutor,
    RunStatus,
};

/// 自环 defer 图（AC-14 a）：`loop.seed` 由 settle 边供源，`loop.again` 只能由自环重投。
const SELF_LOOP_JSON: &str = r#"{
  "version": "1.0",
  "metadata": {"name": "defer-self-loop"},
  "entry_points": ["start"],
  "nodes": [
    {"id": "start", "type": "start",
     "inputs": [{"name": "payload", "type": "any", "required": true}], "config": {}},
    {"id": "loop", "type": "python_node",
     "inputs": [
       {"name": "seed", "type": "any", "required": true},
       {"name": "again", "type": "any", "required": false}
     ],
     "config": {"module": "workers.reentry", "function": "loop",
                "retry": {"max_attempts": 3}}}
  ],
  "edges": [
    {"from": "start", "to_node": "loop", "to_input_port": "seed"},
    {"from": "loop", "to_node": "loop", "to_input_port": "again",
     "gate": {"function": "check_output", "params": {"path": "continue", "equals": true}},
     "on_gate_false": "defer"}
  ]
}"#;

/// A→B→A 回边返工图（AC-14 b）：两条环边都带 gate + defer。
///
/// `a.seed` 是 settle 端口（重入时按端口继承）；`a.feedback`/`b.from_a` 是 defer 端口
/// （重入时必须由当前这一代重投，不得继承上一代值）。
const CYCLE_REWORK_JSON: &str = r#"{
  "version": "1.0",
  "metadata": {"name": "defer-rework-cycle"},
  "entry_points": ["start"],
  "nodes": [
    {"id": "start", "type": "start",
     "inputs": [{"name": "payload", "type": "any", "required": true}], "config": {}},
    {"id": "a", "type": "python_node",
     "inputs": [
       {"name": "seed", "type": "any", "required": true},
       {"name": "feedback", "type": "any", "required": false}
     ],
     "config": {"module": "workers.reentry", "function": "a",
                "retry": {"max_attempts": 3}}},
    {"id": "b", "type": "python_node",
     "inputs": [{"name": "from_a", "type": "any", "required": true}],
     "config": {"module": "workers.reentry", "function": "b"}}
  ],
  "edges": [
    {"from": "start", "to_node": "a", "to_input_port": "seed"},
    {"from": "a", "to_node": "b", "to_input_port": "from_a",
     "gate": {"function": "check_output", "params": {"path": "done", "equals": true}},
     "on_gate_false": "defer"},
    {"from": "b", "to_node": "a", "to_input_port": "feedback",
     "gate": {"function": "check_output", "params": {"path": "again", "equals": true}},
     "on_gate_false": "defer"}
  ]
}"#;

/// 一次节点调用记录（用于断言执行序与输入快照）。
#[derive(Debug, Clone)]
struct Call {
    node_id: String,
    attempt: u32,
    inputs: Value,
}

/// 按节点脚本返回输出，并记录每次调用的 `(node_id, attempt, inputs)` 顺序。
struct RecordingExecutor {
    scripts: Mutex<HashMap<String, VecDeque<Value>>>,
    calls: Mutex<Vec<Call>>,
}

impl RecordingExecutor {
    /// `scripts = [(node_id, [output, ...]), ...]`，每个节点按调用序消费。
    fn new(scripts: &[(&str, &[Value])]) -> Self {
        let scripts = scripts
            .iter()
            .map(|(node, outputs)| ((*node).to_string(), outputs.iter().cloned().collect()))
            .collect();
        Self {
            scripts: Mutex::new(scripts),
            calls: Mutex::new(Vec::new()),
        }
    }

    /// 已发生的调用序列（按发生顺序）。
    fn calls(&self) -> Vec<Call> {
        self.calls.lock().expect("calls lock").clone()
    }
}

impl NodeExecutor for RecordingExecutor {
    fn execute<'a>(
        &'a self,
        execution: NodeExecution<'a>,
    ) -> Pin<Box<dyn Future<Output = Result<Value, NodeError>> + Send + 'a>> {
        let node_id = execution.node.id.clone();
        let attempt = execution.ctx.attempt();
        let inputs = serde_json::to_value(execution.inputs).unwrap_or(Value::Null);
        self.calls.lock().expect("calls lock").push(Call {
            node_id: node_id.clone(),
            attempt,
            inputs,
        });
        let output = self
            .scripts
            .lock()
            .expect("scripts lock")
            .get_mut(&node_id)
            .and_then(VecDeque::pop_front)
            .unwrap_or_else(|| json!({}));
        Box::pin(async move { Ok(output) })
    }
}

/// 某节点每个 attempt 的**最终**状态（快照按迁移逐条记录，这里按 attempt 折叠）。
fn attempt_statuses(sink: &MemoryTraceSink, node_id: &str) -> Vec<(u32, InstanceStatus)> {
    let mut by_attempt: BTreeMap<u32, InstanceStatus> = BTreeMap::new();
    for snapshot in common::snapshots_of(sink, node_id) {
        by_attempt.insert(snapshot.attempt, snapshot.status);
    }
    by_attempt.into_iter().collect()
}

/// 指定边号的全部 GateResult 记录（按记录顺序）。
fn gate_records(sink: &MemoryTraceSink, edge_index: usize) -> Vec<GateResultRecord> {
    sink.gate_results()
        .into_iter()
        .filter(|record| record.edge_index == edge_index)
        .collect()
}

/// 断言一次调用：节点、attempt 与输入快照（输入按端口名排序无关，`Value` 比较）。
fn assert_call(call: &Call, node_id: &str, attempt: u32, inputs: Value) {
    assert_eq!(call.node_id, node_id);
    assert_eq!(call.attempt, attempt);
    assert_eq!(
        call.inputs, inputs,
        "node {node_id} attempt {attempt} inputs"
    );
}

#[tokio::test]
async fn defer_self_loop_reentry_inherits_settle_port_and_uses_source_attempt() {
    let executor = Arc::new(RecordingExecutor::new(&[(
        "loop",
        &[json!({"continue": true}), json!({"continue": false})],
    )]));
    let spec = common::spec(
        common::compile_text(SELF_LOOP_JSON),
        common::entry_inputs(&[("start", "payload", json!({"n": 1}))]),
    );
    let (outcome, sink) = common::run_with_sink(spec, executor.clone()).await;

    assert_eq!(outcome.status, RunStatus::Completed);
    assert_eq!(outcome.error, None);
    assert_eq!(
        attempt_statuses(&sink, "loop"),
        vec![
            (0, InstanceStatus::Completed),
            (1, InstanceStatus::Completed)
        ]
    );
    let calls = executor.calls();
    assert_eq!(calls.len(), 2, "自环放行一次 → 两个实例各执行一次");
    assert_call(
        &calls[0],
        "loop",
        0,
        json!({"seed": {"payload": {"n": 1}}, "again": null}),
    );
    // attempt1：seed 由非 defer 入边按端口继承；again 由自环重投（不继承上一代）。
    assert_call(
        &calls[1],
        "loop",
        1,
        json!({"seed": {"payload": {"n": 1}}, "again": {"continue": true}}),
    );
    let records = gate_records(&sink, 1);
    assert_eq!(records.len(), 2);
    assert_eq!(
        records
            .iter()
            .map(|record| record.passed)
            .collect::<Vec<_>>(),
        vec![true, false]
    );
    assert_eq!(
        records
            .iter()
            .map(|record| record.deferred)
            .collect::<Vec<_>>(),
        vec![false, true],
        "AC-14 a 修正口径：attempt0 deferred=false、attempt1 deferred=true"
    );
    assert_eq!(
        records
            .iter()
            .map(|record| record.attempt)
            .collect::<Vec<_>>(),
        vec![0, 1]
    );
    assert!(!sink
        .snapshots()
        .iter()
        .any(|snapshot| snapshot.status == InstanceStatus::Skipped));
    common::assert_lifecycle_once(&sink);
}

#[tokio::test]
async fn defer_self_loop_over_max_attempts_crashes_instance_without_gate_result() {
    let text = SELF_LOOP_JSON.replace(r#""max_attempts": 3"#, r#""max_attempts": 2"#);
    let executor = Arc::new(RecordingExecutor::new(&[(
        "loop",
        &[json!({"continue": true}), json!({"continue": true})],
    )]));
    let spec = common::spec(
        common::compile_text(&text),
        common::entry_inputs(&[("start", "payload", json!({"n": 1}))]),
    );
    let (outcome, sink) = common::run_with_sink(spec, executor).await;

    assert_eq!(outcome.status, RunStatus::Failed);
    let error = outcome.error.expect("run error");
    assert_eq!(error.code, "NODE_INTERNAL");
    assert!(
        error
            .message
            .contains("self-loop of node 'loop' exceeded max_attempts (2)"),
        "message: {}",
        error.message
    );
    assert!(
        error.message.contains("on edge 1"),
        "自环超限 message 需含边号: {}",
        error.message
    );
    assert_eq!(
        attempt_statuses(&sink, "loop"),
        vec![
            (0, InstanceStatus::Completed),
            (1, InstanceStatus::Completed),
            (2, InstanceStatus::Crashed)
        ],
        "超限实例 attempt = max_attempts"
    );
    let crashed = sink
        .snapshots()
        .into_iter()
        .find(|snapshot| {
            snapshot.node_id == "loop"
                && snapshot.attempt == 2
                && snapshot.status == InstanceStatus::Crashed
        })
        .expect("crashed attempt 2");
    assert_eq!(crashed.error_code.as_deref(), Some("NODE_INTERNAL"));
    // 冻结口径：超限发生在 attempt1 的 publish（它请求 attempt2 时超限），fail_run 先
    // 关闭 sink → 该次 GateResult 不落 trace；attempt0 的放行记录仍在。
    let records = gate_records(&sink, 1);
    assert_eq!(records.len(), 1, "超限那次 emit 不产生 GateResult");
    assert_eq!(
        records
            .iter()
            .map(|record| record.attempt)
            .collect::<Vec<_>>(),
        vec![0]
    );
}

#[tokio::test]
async fn defer_cycle_rework_uses_attempt_per_activation_and_current_inputs() {
    let executor = Arc::new(RecordingExecutor::new(&[
        (
            "a",
            &[
                json!({"done": true, "gen": 0}),
                json!({"done": true, "gen": 1}),
            ],
        ),
        ("b", &[json!({"again": true}), json!({"again": false})]),
    ]));
    let spec = common::spec(
        common::compile_text(CYCLE_REWORK_JSON),
        common::entry_inputs(&[("start", "payload", json!({"n": 1}))]),
    );
    let (outcome, sink) = common::run_with_sink(spec, executor.clone()).await;

    assert_eq!(outcome.status, RunStatus::Completed);
    assert_eq!(outcome.error, None, "不得出现 RUN_DELIVERY_TO_TERMINAL");
    assert_eq!(
        attempt_statuses(&sink, "a"),
        vec![
            (0, InstanceStatus::Completed),
            (1, InstanceStatus::Completed)
        ]
    );
    assert_eq!(
        attempt_statuses(&sink, "b"),
        vec![
            (0, InstanceStatus::Completed),
            (1, InstanceStatus::Completed)
        ]
    );

    // 调用序与每步输入：defer 端口必须用当前这一代的值（不得继承上一代）。
    let calls = executor.calls();
    assert_eq!(calls.len(), 4);
    assert_call(
        &calls[0],
        "a",
        0,
        json!({"seed": {"payload": {"n": 1}}, "feedback": null}),
    );
    assert_call(
        &calls[1],
        "b",
        0,
        json!({"from_a": {"done": true, "gen": 0}}),
    );
    assert_call(
        &calls[2],
        "a",
        1,
        json!({"seed": {"payload": {"n": 1}}, "feedback": {"again": true}}),
    );
    assert_call(
        &calls[3],
        "b",
        1,
        json!({"from_a": {"done": true, "gen": 1}}),
    );

    let a_to_b = gate_records(&sink, 1);
    assert_eq!(a_to_b.len(), 2);
    assert!(a_to_b
        .iter()
        .all(|record| record.passed && !record.deferred));
    assert_eq!(
        a_to_b
            .iter()
            .map(|record| record.attempt)
            .collect::<Vec<_>>(),
        vec![0, 1]
    );
    let b_to_a = gate_records(&sink, 2);
    assert_eq!(b_to_a.len(), 2);
    assert_eq!(
        b_to_a
            .iter()
            .map(|record| record.passed)
            .collect::<Vec<_>>(),
        vec![true, false]
    );
    assert_eq!(
        b_to_a
            .iter()
            .map(|record| record.deferred)
            .collect::<Vec<_>>(),
        vec![false, true]
    );
    assert_eq!(
        b_to_a
            .iter()
            .map(|record| record.attempt)
            .collect::<Vec<_>>(),
        vec![0, 1]
    );
    common::assert_lifecycle_once(&sink);
}

#[tokio::test]
async fn defer_cycle_over_max_attempts_reports_reentry_message() {
    let text = CYCLE_REWORK_JSON.replace(r#""max_attempts": 3"#, r#""max_attempts": 2"#);
    let executor = Arc::new(RecordingExecutor::new(&[
        (
            "a",
            &[
                json!({"done": true, "gen": 0}),
                json!({"done": true, "gen": 1}),
            ],
        ),
        ("b", &[json!({"again": true}), json!({"again": true})]),
    ]));
    let spec = common::spec(
        common::compile_text(&text),
        common::entry_inputs(&[("start", "payload", json!({"n": 1}))]),
    );
    let (outcome, sink) = common::run_with_sink(spec, executor.clone()).await;

    assert_eq!(outcome.status, RunStatus::Failed);
    let error = outcome.error.expect("run error");
    assert_eq!(error.code, "NODE_INTERNAL");
    assert!(
        error
            .message
            .contains("defer re-entry of node 'a' exceeded max_attempts (2) on edge 2 (attempt 2)"),
        "message: {}",
        error.message
    );
    assert_eq!(
        attempt_statuses(&sink, "a"),
        vec![
            (0, InstanceStatus::Completed),
            (1, InstanceStatus::Completed),
            (2, InstanceStatus::Crashed)
        ]
    );
    let crashed = sink
        .snapshots()
        .into_iter()
        .find(|snapshot| {
            snapshot.node_id == "a"
                && snapshot.attempt == 2
                && snapshot.status == InstanceStatus::Crashed
        })
        .expect("crashed attempt 2");
    assert_eq!(crashed.error_code.as_deref(), Some("NODE_INTERNAL"));
    // 超限发生在 b1 的 publish（它请求 a attempt2 时超限），fail_run 先关闭 sink →
    // 该次 GateResult 不落 trace；只保留 b0 的放行记录。
    let records = gate_records(&sink, 2);
    assert_eq!(records.len(), 1);
    assert_eq!(
        records
            .iter()
            .map(|record| record.attempt)
            .collect::<Vec<_>>(),
        vec![0]
    );
    assert_eq!(executor.calls().len(), 4, "a0/b0/a1/b1 各执行一次");
}
