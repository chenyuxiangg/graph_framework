//! W4a/W4b：`foreach` 动态展开运行期（controller / item 实例 / join / 4 个
//! `RUN_FOREACH_*` / 取消 / Pause / per-item attempt / region 内 human）。
//!
//! 语义依据：`doc/decisions/DR-007-w4a-dynamic-fanout-freeze.md` §6/§7 + spec 01
//! §1.2.5、03 §3.4.6。全部经库 API in-process 驱动。

mod common;

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use zgraph_kernel::{
    start_run, FakeExecutor, HumanDecision, InstanceStatus, MemoryTraceSink, NodeError,
    NodeExecution, NodeExecutor, RunControl, RunHandle, RunOptions, RunSpec, RunStatus,
};

// ---------------------------------------------------------------------------
// 图模板
// ---------------------------------------------------------------------------

/// 主流水线：`start -> prep(list) -> F(foreach) -> W -> E -> end`（J = end）。
///
/// `__FOREACH_EXTRA__`：foreach 额外键（`,"key_field":"id"` / `,"max_items":2`）；
/// `__WE_GATE__`：`W -> E` 边的 gate（用于构造 E Skipped 的 ITEM_INCOMPLETE）。
const PIPELINE_TEMPLATE: &str = r#"{
  "version": "1.0",
  "metadata": {"name": "foreach-pipeline"},
  "entry_points": ["start"],
  "nodes": [
    {"id": "start", "type": "start", "inputs": [], "config": {}},
    {"id": "prep", "type": "python_node",
     "inputs": [{"name": "seed", "type": "any", "required": true}],
     "config": {"module": "workers.f", "function": "prep"}},
    {"id": "F", "type": "python_node",
     "inputs": [{"name": "items", "type": "any", "required": true}],
     "config": {"module": "workers.f", "function": "fanout"},
     "foreach": {"items_port": "items", "join": "end"__FOREACH_EXTRA__}},
    {"id": "W", "type": "python_node",
     "inputs": [{"name": "task", "type": "any", "required": true}],
     "config": {"module": "workers.f", "function": "work"}},
    {"id": "E", "type": "python_node",
     "inputs": [{"name": "work", "type": "any", "required": true}],
     "config": {"module": "workers.f", "function": "exit"}},
    {"id": "end", "type": "end",
     "inputs": [{"name": "results", "type": "any", "required": true}],
     "config": {}}
  ],
  "edges": [
    {"from": "start", "to_node": "prep", "to_input_port": "seed"},
    {"from": "prep", "to_node": "F", "to_input_port": "items"},
    {"from": "F", "to_node": "W", "to_input_port": "task"},
    {"from": "W", "to_node": "E", "to_input_port": "work"__WE_GATE__},
    {"from": "E", "to_node": "end", "to_input_port": "results"}
  ]
}"#;

/// region 内 human（每 item 独立等待实例）：`start -> prep -> F(foreach) -> H(human) -> end`。
const HUMAN_TEMPLATE: &str = r#"{
  "version": "1.0",
  "metadata": {"name": "foreach-human"},
  "entry_points": ["start"],
  "nodes": [
    {"id": "start", "type": "start", "inputs": [], "config": {}},
    {"id": "prep", "type": "python_node",
     "inputs": [{"name": "seed", "type": "any", "required": true}],
     "config": {"module": "workers.f", "function": "prep"}},
    {"id": "F", "type": "python_node",
     "inputs": [{"name": "items", "type": "any", "required": true}],
     "config": {"module": "workers.f", "function": "fanout"},
     "foreach": {"items_port": "items", "join": "end"}},
    {"id": "H", "type": "python_node",
     "inputs": [{"name": "payload", "type": "any", "required": true}],
     "config": {"module": "builtin", "function": "human"}},
    {"id": "end", "type": "end",
     "inputs": [{"name": "results", "type": "any", "required": true}],
     "config": {}}
  ],
  "edges": [
    {"from": "start", "to_node": "prep", "to_input_port": "seed"},
    {"from": "prep", "to_node": "F", "to_input_port": "items"},
    {"from": "F", "to_node": "H", "to_input_port": "payload"},
    {"from": "H", "to_node": "end", "to_input_port": "results"}
  ]
}"#;

/// controller 全部入边 settle=false → `Skipped`（不展开、不建 item 实例）。
const CONTROLLER_SKIP_TEMPLATE: &str = r#"{
  "version": "1.0",
  "metadata": {"name": "foreach-controller-skip"},
  "entry_points": ["start"],
  "nodes": [
    {"id": "start", "type": "start", "inputs": [], "config": {}},
    {"id": "prep", "type": "python_node",
     "inputs": [{"name": "seed", "type": "any", "required": true}],
     "config": {"module": "workers.f", "function": "prep"}},
    {"id": "F", "type": "python_node",
     "inputs": [{"name": "items", "type": "any", "required": true}],
     "config": {"module": "workers.f", "function": "fanout"},
     "foreach": {"items_port": "items", "join": "end"}},
    {"id": "end", "type": "end",
     "inputs": [{"name": "results", "type": "any", "required": true}],
     "config": {}}
  ],
  "edges": [
    {"from": "start", "to_node": "prep", "to_input_port": "seed"},
    {"from": "prep", "to_node": "F", "to_input_port": "items",
     "gate": {"function": "check_output", "params": {"path": "ok", "equals": true}}},
    {"from": "F", "to_node": "end", "to_input_port": "results"}
  ]
}"#;

/// controller 部分到达且静默 → `RUN_DEADLOCK`（message 含 F/controller 状态）。
const CONTROLLER_DEADLOCK_TEMPLATE: &str = r#"{
  "version": "1.0",
  "metadata": {"name": "foreach-controller-deadlock"},
  "entry_points": ["start"],
  "nodes": [
    {"id": "start", "type": "start", "inputs": [], "config": {}},
    {"id": "prep", "type": "python_node",
     "inputs": [{"name": "seed", "type": "any", "required": true}],
     "config": {"module": "workers.f", "function": "prep"}},
    {"id": "X", "type": "python_node", "trigger": "any",
     "inputs": [{"name": "seed", "type": "any", "required": true},
                {"name": "loop", "type": "any", "required": false}],
     "config": {"module": "workers.f", "function": "x"}},
    {"id": "F", "type": "python_node", "trigger": "all",
     "inputs": [{"name": "items", "type": "any", "required": true},
                {"name": "other", "type": "any", "required": true}],
     "config": {"module": "workers.f", "function": "fanout"},
     "foreach": {"items_port": "items", "join": "end"}},
    {"id": "end", "type": "end",
     "inputs": [{"name": "results", "type": "any", "required": true}],
     "config": {}}
  ],
  "edges": [
    {"from": "start", "to_node": "prep", "to_input_port": "seed"},
    {"from": "start", "to_node": "X", "to_input_port": "seed"},
    {"from": "prep", "to_node": "F", "to_input_port": "items"},
    {"from": "X", "to_node": "X", "to_input_port": "loop",
     "gate": {"function": "check_output", "params": {"path": "loop", "equals": true}},
     "on_gate_false": "defer"},
    {"from": "X", "to_node": "F", "to_input_port": "other",
     "gate": {"function": "check_output", "params": {"path": "ok", "equals": true}},
     "on_gate_false": "defer"},
    {"from": "F", "to_node": "end", "to_input_port": "results"}
  ]
}"#;

/// 展开后仍有投递到 F → `RUN_DELIVERY_TO_TERMINAL`（F trigger=any，X 延迟投递）。
const LATE_DELIVERY_TEMPLATE: &str = r#"{
  "version": "1.0",
  "metadata": {"name": "foreach-late-delivery"},
  "entry_points": ["start"],
  "nodes": [
    {"id": "start", "type": "start", "inputs": [], "config": {}},
    {"id": "prep", "type": "python_node",
     "inputs": [{"name": "seed", "type": "any", "required": true}],
     "config": {"module": "workers.f", "function": "prep"}},
    {"id": "X", "type": "python_node",
     "inputs": [{"name": "seed", "type": "any", "required": true}],
     "config": {"module": "workers.f", "function": "x"}},
    {"id": "F", "type": "python_node", "trigger": "any",
     "inputs": [{"name": "items", "type": "any", "required": true},
                {"name": "extra", "type": "any", "required": true}],
     "config": {"module": "workers.f", "function": "fanout"},
     "foreach": {"items_port": "items", "join": "end"}},
    {"id": "end", "type": "end",
     "inputs": [{"name": "results", "type": "any", "required": true}],
     "config": {}}
  ],
  "edges": [
    {"from": "start", "to_node": "prep", "to_input_port": "seed"},
    {"from": "start", "to_node": "X", "to_input_port": "seed"},
    {"from": "prep", "to_node": "F", "to_input_port": "items"},
    {"from": "X", "to_node": "F", "to_input_port": "extra"},
    {"from": "F", "to_node": "end", "to_input_port": "results"}
  ]
}"#;

/// per-item defer rework：`W -> W` 自环（gate 读 output.more，defer），attempt 按 item 独立。
const REWORK_TEMPLATE: &str = r#"{
  "version": "1.0",
  "metadata": {"name": "foreach-rework"},
  "entry_points": ["start"],
  "nodes": [
    {"id": "start", "type": "start", "inputs": [], "config": {}},
    {"id": "prep", "type": "python_node",
     "inputs": [{"name": "seed", "type": "any", "required": true}],
     "config": {"module": "workers.f", "function": "prep"}},
    {"id": "F", "type": "python_node",
     "inputs": [{"name": "items", "type": "any", "required": true}],
     "config": {"module": "workers.f", "function": "fanout"},
     "foreach": {"items_port": "items", "join": "end"}},
    {"id": "W", "type": "python_node",
     "inputs": [{"name": "task", "type": "any", "required": true},
                {"name": "more", "type": "any", "required": false}],
     "config": {"module": "workers.f", "function": "work", "retry": {"max_attempts": 3}}},
    {"id": "E", "type": "python_node",
     "inputs": [{"name": "work", "type": "any", "required": true}],
     "config": {"module": "workers.f", "function": "exit"}},
    {"id": "end", "type": "end",
     "inputs": [{"name": "results", "type": "any", "required": true}],
     "config": {}}
  ],
  "edges": [
    {"from": "start", "to_node": "prep", "to_input_port": "seed"},
    {"from": "prep", "to_node": "F", "to_input_port": "items"},
    {"from": "F", "to_node": "W", "to_input_port": "task"},
    {"from": "W", "to_node": "W", "to_input_port": "more",
     "gate": {"function": "check_output", "params": {"path": "more", "equals": true}},
     "on_gate_false": "defer"},
    {"from": "W", "to_node": "E", "to_input_port": "work"},
    {"from": "E", "to_node": "end", "to_input_port": "results"}
  ]
}"#;

fn pipeline_json(foreach_extra: &str, we_gate: &str) -> String {
    PIPELINE_TEMPLATE
        .replace("__FOREACH_EXTRA__", foreach_extra)
        .replace("__WE_GATE__", we_gate)
}

fn foreach_options() -> RunOptions {
    common::options_with(|options| {
        options.tick_interval_ms = 5;
        options.deadlock_grace_ms = 80;
    })
}

fn spec_for(json: &str, options: RunOptions) -> RunSpec {
    let mut spec = common::spec(common::compile_text(json), common::entry_inputs(&[]));
    spec.options = options;
    spec
}

// ---------------------------------------------------------------------------
// 执行器
// ---------------------------------------------------------------------------

type Responder = Box<dyn Fn(&NodeExecution<'_>) -> Result<Value, NodeError> + Send + Sync>;

/// 按节点名定制返回值的执行器（未配置的节点返回 inputs 原样对象）。
struct ScriptedExecutor {
    responders: HashMap<String, Responder>,
}

impl ScriptedExecutor {
    fn new() -> Self {
        Self {
            responders: HashMap::new(),
        }
    }

    fn respond(
        mut self,
        node: &str,
        responder: impl Fn(&NodeExecution<'_>) -> Result<Value, NodeError> + Send + Sync + 'static,
    ) -> Self {
        self.responders
            .insert(node.to_string(), Box::new(responder));
        self
    }
}

impl NodeExecutor for ScriptedExecutor {
    fn execute<'a>(
        &'a self,
        execution: NodeExecution<'a>,
    ) -> Pin<Box<dyn Future<Output = Result<Value, NodeError>> + Send + 'a>> {
        let result = match self.responders.get(&execution.node.id) {
            Some(responder) => responder(&execution),
            None => Ok(Value::Object(
                execution
                    .inputs
                    .iter()
                    .map(|(port, value)| (port.clone(), value.clone()))
                    .collect(),
            )),
        };
        Box::pin(async move { result })
    }
}

/// 记录派发节点的执行器（断言 region 内 human 不派发 worker）。
struct CountingExecutor {
    dispatched: Mutex<Vec<String>>,
    inner: FakeExecutor,
}

impl CountingExecutor {
    fn new(inner: FakeExecutor) -> Self {
        Self {
            dispatched: Mutex::new(Vec::new()),
            inner,
        }
    }

    fn dispatched(&self) -> Vec<String> {
        self.dispatched.lock().expect("dispatched").clone()
    }
}

impl NodeExecutor for CountingExecutor {
    fn execute<'a>(
        &'a self,
        execution: NodeExecution<'a>,
    ) -> Pin<Box<dyn Future<Output = Result<Value, NodeError>> + Send + 'a>> {
        self.dispatched
            .lock()
            .expect("dispatched")
            .push(execution.node.id.clone());
        self.inner.execute(execution)
    }
}

// ---------------------------------------------------------------------------
// 断言辅助
// ---------------------------------------------------------------------------

fn last_status_of(
    sink: &MemoryTraceSink,
    node_id: &str,
    item_index: u32,
) -> Option<InstanceStatus> {
    sink.snapshots()
        .into_iter()
        .rev()
        .find(|snapshot| snapshot.node_id == node_id && snapshot.item_index == item_index)
        .map(|snapshot| snapshot.status)
}

/// 每个实例的最终状态（instance_id → 最后快照状态）。
fn last_status_by_instance(sink: &MemoryTraceSink) -> HashMap<String, InstanceStatus> {
    let mut map = HashMap::new();
    for snapshot in sink.snapshots() {
        map.insert(snapshot.instance_id, snapshot.status);
    }
    map
}

fn completed_snapshot(
    sink: &MemoryTraceSink,
    node_id: &str,
) -> Option<zgraph_kernel::InstanceSnapshot> {
    sink.snapshots().into_iter().rev().find(|snapshot| {
        snapshot.node_id == node_id && snapshot.status == InstanceStatus::Completed
    })
}

async fn wait_for_status(
    sink: &MemoryTraceSink,
    node_id: &str,
    status: InstanceStatus,
) -> zgraph_kernel::InstanceSnapshot {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let latest = sink
            .snapshots()
            .into_iter()
            .rev()
            .find(|snapshot| snapshot.node_id == node_id && snapshot.status == status);
        if let Some(snapshot) = latest {
            return snapshot;
        }
        assert!(
            Instant::now() < deadline,
            "node {node_id} did not reach {} in time",
            status.as_str()
        );
        tokio::time::sleep(Duration::from_millis(2)).await;
    }
}

async fn wait_for_waiting_item(
    sink: &MemoryTraceSink,
    node_id: &str,
    item_index: u32,
) -> zgraph_kernel::InstanceSnapshot {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(snapshot) = sink.snapshots().into_iter().rev().find(|snapshot| {
            snapshot.node_id == node_id
                && snapshot.item_index == item_index
                && snapshot.status == InstanceStatus::WaitingHuman
        }) {
            return snapshot;
        }
        assert!(
            Instant::now() < deadline,
            "node {node_id} item {item_index} did not reach WaitingHuman"
        );
        tokio::time::sleep(Duration::from_millis(2)).await;
    }
}

fn decide(handle: &RunHandle, instance_id: &str, reason: &str) -> bool {
    handle.control(RunControl::HumanDecision {
        instance_id: instance_id.to_string(),
        decision: HumanDecision::Approved,
        approver: "alice".to_string(),
        reason: Some(reason.to_string()),
        decision_id: format!("decision-{instance_id}"),
        decided_at: "2026-11-01T00:00:00.000Z".to_string(),
    })
}

/// 主流水线的 E item 输出（Echo 链）：`{"work":{"task":{"items":<item>}}}`。
fn expected_exit_output(item: &Value) -> Value {
    json!({"work": {"task": {"items": item}}})
}

// ---------------------------------------------------------------------------
// 1) 展开 + 有序 join
// ---------------------------------------------------------------------------

#[tokio::test]
async fn expands_items_and_joins_in_order() {
    let items = json!(["a", "b", "c"]);
    let executor = Arc::new(ScriptedExecutor::new().respond("prep", move |_| Ok(items.clone())));
    let (outcome, sink) = common::run_with_sink(
        spec_for(&pipeline_json("", ""), foreach_options()),
        executor,
    )
    .await;
    assert_eq!(
        outcome.status,
        RunStatus::Completed,
        "error: {:?}",
        outcome.error
    );

    // expansion：count=3、keys 全 None（按索引派生）、source 为 prep 实例。
    let expansions = sink.foreach_expansions();
    assert_eq!(expansions.len(), 1);
    assert_eq!(expansions[0].node_id, "F");
    assert_eq!(expansions[0].item_count, 3);
    assert_eq!(expansions[0].keys, vec![None, None, None]);
    assert!(expansions[0].source_instance_id.is_some());

    // item 实例：F/W/E 各 3 个（item_index 0..2），全部 Completed 且 attempt=0。
    for node in ["F", "W", "E"] {
        for item_index in 0..3 {
            assert_eq!(
                last_status_of(&sink, node, item_index),
                Some(InstanceStatus::Completed),
                "node {node} item {item_index}"
            );
        }
        let snapshot = sink
            .snapshots()
            .into_iter()
            .find(|snapshot| snapshot.node_id == node)
            .expect("snapshot");
        assert_eq!(snapshot.item_key, None, "无 key_field → item_key NULL");
    }

    // join GateResult（C2）：恰一条 E→end，source=最后完成的 E item 实例。
    let join_gates: Vec<_> = sink
        .gate_results()
        .into_iter()
        .filter(|record| record.to_node == "end")
        .collect();
    assert_eq!(join_gates.len(), 1);
    assert!(join_gates[0].passed);
    assert_eq!(join_gates[0].gate_fn, None, "E→J 边不允许 gate");
    let last_exit_completed = sink
        .transitions()
        .into_iter()
        .rev()
        .find(|record| record.node_id == "E" && record.to == InstanceStatus::Completed)
        .expect("E completed transition");
    assert_eq!(join_gates[0].instance_id, last_exit_completed.instance_id);

    // join 数组按 item_index 升序；end 聚合 run_output。
    let expected: Vec<Value> = ["a", "b", "c"]
        .iter()
        .map(|item| expected_exit_output(&json!(item)))
        .collect();
    let end_instance = completed_snapshot(&sink, "end").expect("end completed");
    assert_eq!(
        end_instance.input,
        Some(json!({"results": expected})),
        "J(end) 输入端口为有序数组"
    );
    assert_eq!(outcome.output, Some(json!({"results": expected})));
    common::assert_lifecycle_once(&sink);
}

// ---------------------------------------------------------------------------
// 2) 空列表：expansion(count=0) + J 收 []，无 item 实例/无 join GateResult
// ---------------------------------------------------------------------------

#[tokio::test]
async fn empty_list_injects_empty_array_without_items() {
    let executor = Arc::new(ScriptedExecutor::new().respond("prep", |_| Ok(json!([]))));
    let (outcome, sink) = common::run_with_sink(
        spec_for(&pipeline_json("", ""), foreach_options()),
        executor,
    )
    .await;
    assert_eq!(
        outcome.status,
        RunStatus::Completed,
        "error: {:?}",
        outcome.error
    );

    let expansions = sink.foreach_expansions();
    assert_eq!(expansions.len(), 1);
    assert_eq!(expansions[0].item_count, 0);
    assert!(expansions[0].keys.is_empty());
    // 无 item 实例（region 内节点从未实例化）。
    for node in ["F", "W", "E"] {
        assert!(common::snapshots_of(&sink, node).is_empty(), "node {node}");
    }
    // N=0 无 GateResult/Delivery（C2）：不存在指向 end 的 GateResult。
    assert!(sink
        .gate_results()
        .iter()
        .all(|record| record.to_node != "end"));
    // J 的 instances.input 可见 []。
    let end_instance = completed_snapshot(&sink, "end").expect("end completed");
    assert_eq!(end_instance.input, Some(json!({"results": []})));
    assert_eq!(outcome.output, Some(json!({"results": []})));
    common::assert_lifecycle_once(&sink);
}

// ---------------------------------------------------------------------------
// 3-6) 展开校验（run 级失败，无 item 实例）
// ---------------------------------------------------------------------------

#[tokio::test]
async fn non_array_items_fails_before_instances() {
    let executor =
        Arc::new(ScriptedExecutor::new().respond("prep", |_| Ok(json!({"not": "a list"}))));
    let (outcome, sink) = common::run_with_sink(
        spec_for(&pipeline_json("", ""), foreach_options()),
        executor,
    )
    .await;
    assert_eq!(outcome.status, RunStatus::Failed);
    let error = outcome.error.expect("run error");
    assert_eq!(error.code, "RUN_FOREACH_ITEMS_NOT_ARRAY");
    assert_eq!(
        error.message,
        "foreach node 'F' items_port 'items' is not an array (got object)"
    );
    assert!(sink.foreach_expansions().is_empty());
    for node in ["F", "W", "E"] {
        assert!(common::snapshots_of(&sink, node).is_empty(), "node {node}");
    }
}

#[tokio::test]
async fn max_items_exceeded_fails_before_instances() {
    let executor =
        Arc::new(ScriptedExecutor::new().respond("prep", |_| Ok(json!(["a", "b", "c"]))));
    let (outcome, sink) = common::run_with_sink(
        spec_for(&pipeline_json(r#", "max_items": 2"#, ""), foreach_options()),
        executor,
    )
    .await;
    assert_eq!(outcome.status, RunStatus::Failed);
    let error = outcome.error.expect("run error");
    assert_eq!(error.code, "RUN_FOREACH_ITEMS_EXCEEDED");
    assert_eq!(error.message, "foreach node 'F' items 3 exceed max_items 2");
    assert!(sink.foreach_expansions().is_empty());
}

#[tokio::test]
async fn duplicate_keys_fail_before_instances() {
    let executor = Arc::new(
        ScriptedExecutor::new().respond("prep", |_| Ok(json!([{"id": 1}, {"id": 2}, {"id": 1}]))),
    );
    let (outcome, sink) = common::run_with_sink(
        spec_for(
            &pipeline_json(r#", "key_field": "id""#, ""),
            foreach_options(),
        ),
        executor,
    )
    .await;
    assert_eq!(outcome.status, RunStatus::Failed);
    let error = outcome.error.expect("run error");
    assert_eq!(error.code, "RUN_FOREACH_ITEM_KEY_DUPLICATE");
    assert_eq!(
        error.message,
        "foreach node 'F' item key '1' is duplicated at indexes 0,2"
    );
    assert!(sink.foreach_expansions().is_empty());
    assert!(common::snapshots_of(&sink, "F").is_empty());
}

#[tokio::test]
async fn item_key_canonical_forms() {
    let executor = Arc::new(ScriptedExecutor::new().respond("prep", |_| {
        Ok(json!([
            {"id": 1},
            {"id": 1.5},
            {"id": "x"},
            {"id": true},
            {"id": null},
            {"other": 1}
        ]))
    }));
    let (outcome, sink) = common::run_with_sink(
        spec_for(
            &pipeline_json(r#", "key_field": "id""#, ""),
            foreach_options(),
        ),
        executor,
    )
    .await;
    assert_eq!(
        outcome.status,
        RunStatus::Completed,
        "error: {:?}",
        outcome.error
    );
    let expansions = sink.foreach_expansions();
    assert_eq!(expansions.len(), 1);
    assert_eq!(
        expansions[0].keys,
        vec![
            Some("1".to_string()),
            Some("1.5".to_string()),
            Some("x".to_string()),
            Some("true".to_string()),
            None,
            None,
        ]
    );
}

// ---------------------------------------------------------------------------
// 7-8) item 失败 / E Skipped
// ---------------------------------------------------------------------------

#[tokio::test]
async fn item_executor_failure_is_cold() {
    let executor = Arc::new(
        ScriptedExecutor::new()
            .respond("prep", |_| Ok(json!(["a", "b", "c"])))
            .respond("W", |execution| {
                if execution.ctx.item_index() == 1 {
                    Err(NodeError::BusinessError {
                        code: "BIZ_ITEM".to_string(),
                        message: "item boom".to_string(),
                    })
                } else {
                    Ok(Value::Object(
                        execution
                            .inputs
                            .iter()
                            .map(|(port, value)| (port.clone(), value.clone()))
                            .collect(),
                    ))
                }
            }),
    );
    let (outcome, sink) = common::run_with_sink(
        spec_for(&pipeline_json("", ""), foreach_options()),
        executor,
    )
    .await;
    assert_eq!(outcome.status, RunStatus::Failed);
    assert_eq!(
        outcome.error.expect("run error").code,
        "NODE_BUSINESS_ERROR"
    );
    // 无 join 投递。
    assert!(sink
        .gate_results()
        .iter()
        .all(|record| record.to_node != "end"));
    // 失败 item 的实例 Crashed；其余 item 的实例只会是已完成或已取消（冷峻级联）。
    assert_eq!(last_status_of(&sink, "W", 1), Some(InstanceStatus::Crashed));
    let statuses = last_status_by_instance(&sink);
    let other_ids: Vec<(u32, String)> = sink
        .snapshots()
        .into_iter()
        .filter(|snapshot| snapshot.node_id == "W" && snapshot.item_index != 1)
        .map(|snapshot| (snapshot.item_index, snapshot.instance_id))
        .collect();
    for (_, instance_id) in other_ids {
        assert!(
            matches!(
                statuses.get(&instance_id),
                Some(InstanceStatus::Cancelled) | Some(InstanceStatus::Completed)
            ),
            "instance {instance_id} must be terminal"
        );
    }
}

#[tokio::test]
async fn skipped_exit_fails_item_incomplete() {
    let gate =
        r#", "gate": {"function": "check_output", "params": {"path": "ok", "equals": true}}"#;
    let executor = Arc::new(ScriptedExecutor::new().respond("prep", |_| Ok(json!(["a", "b"]))));
    let (outcome, sink) = common::run_with_sink(
        spec_for(&pipeline_json("", gate), foreach_options()),
        executor,
    )
    .await;
    assert_eq!(outcome.status, RunStatus::Failed);
    let error = outcome.error.expect("run error");
    assert_eq!(error.code, "RUN_FOREACH_ITEM_INCOMPLETE");
    assert_eq!(
        error.message,
        "foreach node 'F' item 0 exit node 'E' was skipped"
    );
    assert_eq!(last_status_of(&sink, "E", 0), Some(InstanceStatus::Skipped));
    assert!(sink
        .gate_results()
        .iter()
        .all(|record| record.to_node != "end"));
}

// ---------------------------------------------------------------------------
// 9-10) 取消 / Pause
// ---------------------------------------------------------------------------

#[tokio::test]
async fn cancel_propagates_to_item_instances() {
    let executor = Arc::new(
        FakeExecutor::new()
            .respond("prep", json!(["a", "b", "c"]))
            .delay("W", Duration::from_millis(200), json!({"w": 1})),
    );
    let sink = Arc::new(MemoryTraceSink::new());
    let spec = spec_for(&pipeline_json("", ""), foreach_options());
    let handle = start_run(None, spec, executor, sink.clone()).expect("started");
    wait_for_status(&sink, "W", InstanceStatus::Running).await;
    assert!(handle.cancel());
    let outcome = handle.wait().await;
    assert_eq!(outcome.status, RunStatus::Cancelled);

    let statuses = last_status_by_instance(&sink);
    let w_ids: Vec<String> = sink
        .snapshots()
        .into_iter()
        .filter(|snapshot| snapshot.node_id == "W")
        .map(|snapshot| snapshot.instance_id)
        .collect();
    assert!(!w_ids.is_empty());
    for instance_id in w_ids {
        assert_eq!(
            statuses.get(&instance_id),
            Some(&InstanceStatus::Cancelled),
            "instance {instance_id}"
        );
    }
    common::assert_lifecycle_once(&sink);
}

#[tokio::test]
async fn pause_freezes_item_dispatch() {
    let executor = Arc::new(
        FakeExecutor::new()
            .respond("prep", json!(["a", "b"]))
            .delay("W", Duration::from_millis(60), json!({"w": 1})),
    );
    let sink = Arc::new(MemoryTraceSink::new());
    let spec = spec_for(&pipeline_json("", ""), foreach_options());
    let handle = start_run(None, spec, executor, sink.clone()).expect("started");
    wait_for_status(&sink, "W", InstanceStatus::Running).await;
    assert!(handle.pause());
    tokio::time::sleep(Duration::from_millis(150)).await;
    assert_eq!(handle.status(), RunStatus::Paused, "Pause 冻结 item 派发");
    assert!(handle.resume());
    let outcome = handle.wait().await;
    assert_eq!(outcome.status, RunStatus::Completed);
    common::assert_lifecycle_once(&sink);
}

// ---------------------------------------------------------------------------
// 11) region 内 human：每 item 独立 WaitingHuman/decide
// ---------------------------------------------------------------------------

#[tokio::test]
async fn human_region_waits_per_item() {
    let executor = Arc::new(CountingExecutor::new(
        FakeExecutor::new().respond("prep", json!(["a", "b"])),
    ));
    let sink = Arc::new(MemoryTraceSink::new());
    let spec = spec_for(HUMAN_TEMPLATE, foreach_options());
    let handle = start_run(None, spec, executor.clone(), sink.clone()).expect("started");

    let first = wait_for_waiting_item(&sink, "H", 0).await;
    let second = wait_for_waiting_item(&sink, "H", 1).await;
    assert_ne!(first.instance_id, second.instance_id);
    // human 不派发 worker（执行器只见过 prep/F）。
    assert!(executor.dispatched().iter().all(|node| node != "H"));
    assert!(sink.foreach_expansions()[0].item_count == 2);

    assert!(decide(&handle, &first.instance_id, "ok-0"));
    assert!(decide(&handle, &second.instance_id, "ok-1"));
    let outcome = handle.wait().await;
    assert_eq!(
        outcome.status,
        RunStatus::Completed,
        "error: {:?}",
        outcome.error
    );

    let records = sink.human_decided();
    assert_eq!(records.len(), 2);
    assert!(records.iter().all(|record| record.applied));
    assert_ne!(records[0].instance_id, records[1].instance_id);
    let expected = json!([
        {"decision": "approved", "reason": "ok-0"},
        {"decision": "approved", "reason": "ok-1"}
    ]);
    assert_eq!(outcome.output, Some(json!({"results": expected})));
    common::assert_lifecycle_once(&sink);
}

// ---------------------------------------------------------------------------
// 12) per-item defer rework（attempt 按 item 独立）
// ---------------------------------------------------------------------------

#[tokio::test]
async fn per_item_defer_rework_scopes_attempt() {
    let executor = Arc::new(
        ScriptedExecutor::new()
            .respond("prep", |_| Ok(json!(["a", "b"])))
            .respond("W", |execution| {
                Ok(json!({"more": execution.ctx.attempt() == 0}))
            }),
    );
    let (outcome, sink) =
        common::run_with_sink(spec_for(REWORK_TEMPLATE, foreach_options()), executor).await;
    assert_eq!(
        outcome.status,
        RunStatus::Completed,
        "error: {:?}",
        outcome.error
    );

    // 每个 item 的 W 有 attempt 0 与 1 两个实例，且都 Completed。
    for item_index in 0..2 {
        let attempts: Vec<u32> = sink
            .snapshots()
            .into_iter()
            .filter(|snapshot| snapshot.node_id == "W" && snapshot.item_index == item_index)
            .map(|snapshot| snapshot.attempt)
            .collect();
        assert!(attempts.contains(&0), "item {item_index} attempt 0");
        assert!(attempts.contains(&1), "item {item_index} attempt 1");
        assert_eq!(
            last_status_of(&sink, "W", item_index),
            Some(InstanceStatus::Completed)
        );
    }
    // W -> W 自环 gate：每个 item 一条 passed（attempt0）+ 一条 deferred（attempt1）。
    let loop_gates: Vec<_> = sink
        .gate_results()
        .into_iter()
        .filter(|record| record.node_id == "W" && record.to_node == "W")
        .collect();
    assert_eq!(loop_gates.len(), 4);
    assert_eq!(loop_gates.iter().filter(|record| record.passed).count(), 2);
    assert_eq!(
        loop_gates.iter().filter(|record| record.deferred).count(),
        2
    );
    assert!(outcome.output.is_some());
}

// ---------------------------------------------------------------------------
// 13-15) controller 边界：skip / deadlock / 展开后投递
// ---------------------------------------------------------------------------

#[tokio::test]
async fn controller_skips_when_all_inputs_blocked() {
    let executor = Arc::new(ScriptedExecutor::new().respond("prep", |_| Ok(json!({"ok": false}))));
    let (outcome, sink) = common::run_with_sink(
        spec_for(CONTROLLER_SKIP_TEMPLATE, foreach_options()),
        executor,
    )
    .await;
    assert_eq!(
        outcome.status,
        RunStatus::Completed,
        "error: {:?}",
        outcome.error
    );
    assert!(
        sink.foreach_expansions().is_empty(),
        "controller Skipped 不展开"
    );
    assert!(common::snapshots_of(&sink, "F").is_empty());
    assert_eq!(
        common::last_status(&sink, "end"),
        Some(InstanceStatus::Skipped)
    );
}

#[tokio::test]
async fn controller_collecting_deadlocks() {
    let executor = Arc::new(ScriptedExecutor::new().respond("prep", |_| Ok(json!(["a"]))));
    let (outcome, sink) = common::run_with_sink(
        spec_for(CONTROLLER_DEADLOCK_TEMPLATE, foreach_options()),
        executor,
    )
    .await;
    assert_eq!(outcome.status, RunStatus::Failed);
    let error = outcome.error.expect("run error");
    assert_eq!(error.code, "RUN_DEADLOCK");
    assert!(
        error
            .message
            .contains("foreach controller of node 'F' is Collecting"),
        "message: {}",
        error.message
    );
    assert!(sink.foreach_expansions().is_empty());
}

#[tokio::test]
async fn delivery_after_expansion_is_terminal() {
    let executor = Arc::new(
        FakeExecutor::new()
            .respond("prep", json!(["a", "b"]))
            .delay("F", Duration::from_millis(300), json!({"item": true}))
            .delay("X", Duration::from_millis(40), json!({"x": 1})),
    );
    let (outcome, sink) = common::run_with_sink(
        spec_for(LATE_DELIVERY_TEMPLATE, foreach_options()),
        executor,
    )
    .await;
    assert_eq!(outcome.status, RunStatus::Failed);
    assert_eq!(
        outcome.error.expect("run error").code,
        "RUN_DELIVERY_TO_TERMINAL"
    );
    assert_eq!(sink.foreach_expansions().len(), 1);
}

// ---------------------------------------------------------------------------
// REV-090 修复回归（C1 Pause→Resume 展开 / N1 source / N2 展开后 defer 投递）
// ---------------------------------------------------------------------------

/// 串行 foreach 链：`F1(foreach join=F2) -> F2(foreach join=end)`（region 不相交）。
const SERIAL_TEMPLATE: &str = r#"{
  "version": "1.0",
  "metadata": {"name": "foreach-serial"},
  "entry_points": ["start"],
  "nodes": [
    {"id": "start", "type": "start", "inputs": [], "config": {}},
    {"id": "prep", "type": "python_node",
     "inputs": [{"name": "seed", "type": "any", "required": true}],
     "config": {"module": "workers.f", "function": "prep"}},
    {"id": "F1", "type": "python_node",
     "inputs": [{"name": "items", "type": "any", "required": true}],
     "config": {"module": "workers.f", "function": "f1"},
     "foreach": {"items_port": "items", "join": "F2"}},
    {"id": "F2", "type": "python_node",
     "inputs": [{"name": "items", "type": "any", "required": true}],
     "config": {"module": "workers.f", "function": "f2"},
     "foreach": {"items_port": "items", "join": "end"}},
    {"id": "end", "type": "end",
     "inputs": [{"name": "results", "type": "any", "required": true}],
     "config": {}}
  ],
  "edges": [
    {"from": "start", "to_node": "prep", "to_input_port": "seed"},
    {"from": "prep", "to_node": "F1", "to_input_port": "items"},
    {"from": "F1", "to_node": "F2", "to_input_port": "items"},
    {"from": "F2", "to_node": "end", "to_input_port": "results"}
  ]
}"#;

/// N1：F 的两条入边（items 先到、other 后到）——source 必须是使 Ready 的 other 投递。
const SOURCE_TEMPLATE: &str = r#"{
  "version": "1.0",
  "metadata": {"name": "foreach-source"},
  "entry_points": ["start"],
  "nodes": [
    {"id": "start", "type": "start", "inputs": [], "config": {}},
    {"id": "prep", "type": "python_node",
     "inputs": [{"name": "seed", "type": "any", "required": true}],
     "config": {"module": "workers.f", "function": "prep"}},
    {"id": "X", "type": "python_node",
     "inputs": [{"name": "seed", "type": "any", "required": true}],
     "config": {"module": "workers.f", "function": "x"}},
    {"id": "F", "type": "python_node", "trigger": "all",
     "inputs": [{"name": "items", "type": "any", "required": true},
                {"name": "other", "type": "any", "required": true}],
     "config": {"module": "workers.f", "function": "fanout"},
     "foreach": {"items_port": "items", "join": "end"}},
    {"id": "end", "type": "end",
     "inputs": [{"name": "results", "type": "any", "required": true}],
     "config": {}}
  ],
  "edges": [
    {"from": "start", "to_node": "prep", "to_input_port": "seed"},
    {"from": "start", "to_node": "X", "to_input_port": "seed"},
    {"from": "prep", "to_node": "F", "to_input_port": "items"},
    {"from": "X", "to_node": "F", "to_input_port": "other"},
    {"from": "F", "to_node": "end", "to_input_port": "results"}
  ]
}"#;

/// N1 变体：`trigger=any`，items 使 controller Ready（Pause 中），extra 在 Ready 之后
/// 到达——source 必须是 items 的投递，不得被更晚的 extra 投递覆盖。
const SOURCE_PAUSE_TEMPLATE: &str = r#"{
  "version": "1.0",
  "metadata": {"name": "foreach-source-pause"},
  "entry_points": ["start"],
  "nodes": [
    {"id": "start", "type": "start", "inputs": [], "config": {}},
    {"id": "prep", "type": "python_node",
     "inputs": [{"name": "seed", "type": "any", "required": true}],
     "config": {"module": "workers.f", "function": "prep"}},
    {"id": "X", "type": "python_node",
     "inputs": [{"name": "seed", "type": "any", "required": true}],
     "config": {"module": "workers.f", "function": "x"}},
    {"id": "F", "type": "python_node", "trigger": "any",
     "inputs": [{"name": "items", "type": "any", "required": true},
                {"name": "extra", "type": "any", "required": true}],
     "config": {"module": "workers.f", "function": "fanout"},
     "foreach": {"items_port": "items", "join": "end"}},
    {"id": "end", "type": "end",
     "inputs": [{"name": "results", "type": "any", "required": true}],
     "config": {}}
  ],
  "edges": [
    {"from": "start", "to_node": "prep", "to_input_port": "seed"},
    {"from": "start", "to_node": "X", "to_input_port": "seed"},
    {"from": "prep", "to_node": "F", "to_input_port": "items"},
    {"from": "X", "to_node": "F", "to_input_port": "extra"},
    {"from": "F", "to_node": "end", "to_input_port": "results"}
  ]
}"#;

/// N2：F 已展开后仍有 defer 边 gate=false 投递到 F → RUN_DELIVERY_TO_TERMINAL。
///
/// `X -> X` 是 defer 自环（满足 R15 可重入），`X -> F` 为 defer 且 gate=false。
const LATE_DEFER_TEMPLATE: &str = r#"{
  "version": "1.0",
  "metadata": {"name": "foreach-late-defer"},
  "entry_points": ["start"],
  "nodes": [
    {"id": "start", "type": "start", "inputs": [], "config": {}},
    {"id": "prep", "type": "python_node",
     "inputs": [{"name": "seed", "type": "any", "required": true}],
     "config": {"module": "workers.f", "function": "prep"}},
    {"id": "X", "type": "python_node", "trigger": "any",
     "inputs": [{"name": "seed", "type": "any", "required": true},
                {"name": "loop", "type": "any", "required": false}],
     "config": {"module": "workers.f", "function": "x"}},
    {"id": "F", "type": "python_node", "trigger": "any",
     "inputs": [{"name": "items", "type": "any", "required": true},
                {"name": "extra", "type": "any", "required": true}],
     "config": {"module": "workers.f", "function": "fanout"},
     "foreach": {"items_port": "items", "join": "end"}},
    {"id": "end", "type": "end",
     "inputs": [{"name": "results", "type": "any", "required": true}],
     "config": {}}
  ],
  "edges": [
    {"from": "start", "to_node": "prep", "to_input_port": "seed"},
    {"from": "start", "to_node": "X", "to_input_port": "seed"},
    {"from": "prep", "to_node": "F", "to_input_port": "items"},
    {"from": "X", "to_node": "X", "to_input_port": "loop",
     "gate": {"function": "check_output", "params": {"path": "loop", "equals": true}},
     "on_gate_false": "defer"},
    {"from": "X", "to_node": "F", "to_input_port": "extra",
     "gate": {"function": "check_output", "params": {"path": "ok", "equals": true}},
     "on_gate_false": "defer"},
    {"from": "F", "to_node": "end", "to_input_port": "results"}
  ]
}"#;

/// C1 回归 1：Pause 时 F 的上游仍在飞；上游完成把 controller 置 Ready 后 Resume
/// 必须展开（不得误判 RUN_DEADLOCK）。
#[tokio::test]
async fn pause_during_upstream_expands_after_resume() {
    let executor =
        Arc::new(FakeExecutor::new().delay("prep", Duration::from_millis(120), json!(["a", "b"])));
    let sink = Arc::new(MemoryTraceSink::new());
    let spec = spec_for(&pipeline_json("", ""), foreach_options());
    let handle = start_run(None, spec, executor, sink.clone()).expect("started");
    wait_for_status(&sink, "prep", InstanceStatus::Running).await;
    assert!(handle.pause());
    // 上游在 Pause 期间完成：controller 变 Ready 但展开被冻结；不得死锁/结束。
    tokio::time::sleep(Duration::from_millis(400)).await;
    assert_eq!(handle.status(), RunStatus::Paused);
    assert!(
        sink.foreach_expansions().is_empty(),
        "Pause 冻结 controller 展开"
    );
    assert!(handle.resume());
    let outcome = handle.wait().await;
    assert_eq!(
        outcome.status,
        RunStatus::Completed,
        "Resume 后必须展开 Ready controller（不得 RUN_DEADLOCK）：{:?}",
        outcome.error
    );
    assert_eq!(sink.foreach_expansions().len(), 1);
    assert_eq!(sink.foreach_expansions()[0].item_count, 2);
    assert_eq!(
        common::last_status(&sink, "end"),
        Some(InstanceStatus::Completed)
    );
    common::assert_lifecycle_once(&sink);
}

/// C1 回归 2：串行 F1→F2；Pause 时 F1 item 完成并把 join 投递给 F2（F2 controller
/// 变 Ready），Resume 后按 `node_order` 重扫展开 F2。
#[tokio::test]
async fn serial_foreach_pause_resume_expands_second_controller() {
    let executor = Arc::new(
        FakeExecutor::new()
            .respond("prep", json!(["a", "b"]))
            .delay("F1", Duration::from_millis(120), json!({"item": true})),
    );
    let sink = Arc::new(MemoryTraceSink::new());
    let spec = spec_for(SERIAL_TEMPLATE, foreach_options());
    let handle = start_run(None, spec, executor, sink.clone()).expect("started");
    wait_for_status(&sink, "F1", InstanceStatus::Running).await;
    assert!(handle.pause());
    // F1 item 完成（在飞照常）→ join 投递给 F2 controller（Ready，Pause 冻结展开）。
    tokio::time::sleep(Duration::from_millis(400)).await;
    assert_eq!(handle.status(), RunStatus::Paused);
    assert_eq!(
        sink.foreach_expansions().len(),
        1,
        "Pause 期间只有 F1 已展开"
    );
    assert!(
        common::snapshots_of(&sink, "F2").is_empty(),
        "Pause 期间 F2 不得展开"
    );
    assert!(handle.resume());
    let outcome = handle.wait().await;
    assert_eq!(
        outcome.status,
        RunStatus::Completed,
        "串行链 Resume 后应级联展开 F2：{:?}",
        outcome.error
    );
    let expansions = sink.foreach_expansions();
    assert_eq!(expansions.len(), 2);
    assert_eq!(expansions[1].node_id, "F2");
    assert_eq!(expansions[1].item_count, 2);
    assert_eq!(
        common::last_status(&sink, "end"),
        Some(InstanceStatus::Completed)
    );
    common::assert_lifecycle_once(&sink);
}

/// N1：`source_instance_id` 只记录使 controller 变 `Ready` 的那条投递（本例 X），
/// 不被后续/更早的其它 passed 投递覆盖。
#[tokio::test]
async fn expansion_source_is_the_ready_causing_delivery() {
    let executor = Arc::new(
        FakeExecutor::new()
            .respond("prep", json!(["a", "b"]))
            .delay("X", Duration::from_millis(60), json!({"x": 1})),
    );
    let (outcome, sink) =
        common::run_with_sink(spec_for(SOURCE_TEMPLATE, foreach_options()), executor).await;
    assert_eq!(
        outcome.status,
        RunStatus::Completed,
        "error: {:?}",
        outcome.error
    );
    let expansions = sink.foreach_expansions();
    assert_eq!(expansions.len(), 1);

    let instance_of = |node: &str| {
        sink.snapshots()
            .into_iter()
            .find(|snapshot| snapshot.node_id == node)
            .map(|snapshot| snapshot.instance_id)
            .expect("instance")
    };
    let x_instance = instance_of("X");
    let prep_instance = instance_of("prep");
    assert_eq!(
        expansions[0].source_instance_id.as_deref(),
        Some(x_instance.as_str()),
        "source 必须是使 controller Ready 的 X 投递"
    );
    assert_ne!(
        expansions[0].source_instance_id.as_deref(),
        Some(prep_instance.as_str()),
        "更早的 items 投递不得成为 source"
    );
}

/// N2：F 已 `Expanded` 后，defer 边 gate=false 的投递不再只挂 receipt，
/// 而按 DR-007 §6.6/§6.7 → `RUN_DELIVERY_TO_TERMINAL`。
#[tokio::test]
async fn defer_false_delivery_after_expansion_is_terminal() {
    let executor = Arc::new(
        FakeExecutor::new()
            .respond("prep", json!(["a", "b"]))
            .delay("F", Duration::from_millis(300), json!({"item": true}))
            .delay("X", Duration::from_millis(40), json!({"x": 1})),
    );
    let (outcome, sink) =
        common::run_with_sink(spec_for(LATE_DEFER_TEMPLATE, foreach_options()), executor).await;
    assert_eq!(outcome.status, RunStatus::Failed);
    assert_eq!(
        outcome.error.expect("run error").code,
        "RUN_DELIVERY_TO_TERMINAL",
        "展开后 defer gate=false 投递同样按单次展开口径拒绝"
    );
    assert_eq!(sink.foreach_expansions().len(), 1);
}

/// N1 回归（Pause 窗口）：controller 在 Pause 中变 Ready 后、Resume 前又有 passed
/// 投递到达；`source_instance_id` 必须保持为「使 Ready 的 items 投递」，不被覆盖。
#[tokio::test]
async fn expansion_source_not_overwritten_while_paused() {
    let executor = Arc::new(
        FakeExecutor::new()
            .delay("prep", Duration::from_millis(100), json!(["a", "b"]))
            .delay("X", Duration::from_millis(250), json!({"x": 1})),
    );
    let sink = Arc::new(MemoryTraceSink::new());
    let spec = spec_for(SOURCE_PAUSE_TEMPLATE, foreach_options());
    let handle = start_run(None, spec, executor, sink.clone()).expect("started");
    wait_for_status(&sink, "prep", InstanceStatus::Running).await;
    assert!(handle.pause());
    // prep 100ms 完成 → controller Ready（items）；X 250ms 完成 → Ready 后的 passed 投递。
    tokio::time::sleep(Duration::from_millis(400)).await;
    assert_eq!(handle.status(), RunStatus::Paused);
    assert!(sink.foreach_expansions().is_empty());
    assert!(handle.resume());
    let outcome = handle.wait().await;
    assert_eq!(
        outcome.status,
        RunStatus::Completed,
        "error: {:?}",
        outcome.error
    );

    let instance_of = |node: &str| {
        sink.snapshots()
            .into_iter()
            .find(|snapshot| snapshot.node_id == node)
            .map(|snapshot| snapshot.instance_id)
            .expect("instance")
    };
    let expansions = sink.foreach_expansions();
    assert_eq!(expansions.len(), 1);
    assert_eq!(
        expansions[0].source_instance_id.as_deref(),
        Some(instance_of("prep").as_str()),
        "source 必须是使 Ready 的 items 投递，不被 Pause 窗口内更晚的 extra 覆盖"
    );
    assert_ne!(
        expansions[0].source_instance_id.as_deref(),
        Some(instance_of("X").as_str())
    );
}

// ---------------------------------------------------------------------------
// REV-092（W4b A）：items_port 整包广播字段提取（unwrap）
// ---------------------------------------------------------------------------

/// entry 注入 → start 整包广播：`start(items=[..]) -> F(foreach) -> end`。
///
/// v1 下 start 输出 = 入口注入 object `{"items":[...]}`，边投递把它整包写给 F 的
/// items 端口；内核按「object 含与 items_port 同名键」unwrap 后展开。
const ENTRY_TEMPLATE: &str = r#"{
  "version": "1.0",
  "metadata": {"name": "foreach-entry"},
  "entry_points": ["start"],
  "nodes": [
    {"id": "start", "type": "start",
     "inputs": [{"name": "items", "type": "any", "required": true}], "config": {}},
    {"id": "F", "type": "python_node",
     "inputs": [{"name": "items", "type": "any", "required": true}],
     "config": {"module": "workers.f", "function": "fanout"},
     "foreach": {"items_port": "items", "join": "end"}},
    {"id": "end", "type": "end",
     "inputs": [{"name": "results", "type": "any", "required": true}], "config": {}}
  ],
  "edges": [
    {"from": "start", "to_node": "F", "to_input_port": "items"},
    {"from": "F", "to_node": "end", "to_input_port": "results"}
  ]
}"#;

/// 真 worker 形状的 object 输出 `{"items":[...], "meta":1}` → unwrap 同名键后展开。
#[tokio::test]
async fn object_items_port_unwraps_same_key() {
    let executor = Arc::new(
        ScriptedExecutor::new()
            .respond("prep", |_| Ok(json!({"items": ["a", "b", "c"], "meta": 1}))),
    );
    let (outcome, sink) = common::run_with_sink(
        spec_for(&pipeline_json("", ""), foreach_options()),
        executor,
    )
    .await;
    assert_eq!(
        outcome.status,
        RunStatus::Completed,
        "error: {:?}",
        outcome.error
    );
    let expansions = sink.foreach_expansions();
    assert_eq!(expansions.len(), 1);
    assert_eq!(expansions[0].item_count, 3);
    let expected: Vec<Value> = ["a", "b", "c"]
        .iter()
        .map(|item| expected_exit_output(&json!(item)))
        .collect();
    assert_eq!(outcome.output, Some(json!({"results": expected})));
}

/// entry 注入 `{"items":[a,b,c]}` → start 整包广播 object → F.items unwrap → 展开 3。
#[tokio::test]
async fn entry_injection_object_unwraps_for_foreach() {
    let mut spec = common::spec(
        common::compile_text(ENTRY_TEMPLATE),
        common::entry_inputs(&[("start", "items", json!(["a", "b", "c"]))]),
    );
    spec.options = foreach_options();
    let (outcome, sink) = common::run_with_sink(spec, Arc::new(FakeExecutor::new())).await;
    assert_eq!(
        outcome.status,
        RunStatus::Completed,
        "error: {:?}",
        outcome.error
    );
    let expansions = sink.foreach_expansions();
    assert_eq!(expansions.len(), 1);
    assert_eq!(expansions[0].item_count, 3);
    assert_eq!(
        common::last_status(&sink, "end"),
        Some(InstanceStatus::Completed)
    );
}

/// 负例：object 无同名键 / 同名键非 array / 标量 → 仍 `RUN_FOREACH_ITEMS_NOT_ARRAY`，
/// 且不产生 expansion / item 实例。
#[tokio::test]
async fn items_port_non_array_shapes_stay_not_array() {
    let cases = [
        (json!({"other": ["a", "b"]}), "object"),
        (json!({"items": {"nested": 1}}), "object"),
        (json!("not-a-list"), "string"),
    ];
    for (port_value, expected_type) in cases {
        let executor =
            Arc::new(ScriptedExecutor::new().respond("prep", move |_| Ok(port_value.clone())));
        let (outcome, sink) = common::run_with_sink(
            spec_for(&pipeline_json("", ""), foreach_options()),
            executor,
        )
        .await;
        assert_eq!(outcome.status, RunStatus::Failed);
        let error = outcome.error.expect("run error");
        assert_eq!(error.code, "RUN_FOREACH_ITEMS_NOT_ARRAY");
        assert_eq!(
            error.message,
            format!("foreach node 'F' items_port 'items' is not an array (got {expected_type})")
        );
        assert!(sink.foreach_expansions().is_empty());
        assert!(common::snapshots_of(&sink, "F").is_empty());
    }
}

/// max_items 校验作用于 unwrap 后的列表。
#[tokio::test]
async fn max_items_applies_after_unwrap() {
    let executor = Arc::new(
        ScriptedExecutor::new().respond("prep", |_| Ok(json!({"items": ["a", "b", "c"]}))),
    );
    let (outcome, sink) = common::run_with_sink(
        spec_for(&pipeline_json(r#", "max_items": 2"#, ""), foreach_options()),
        executor,
    )
    .await;
    assert_eq!(outcome.status, RunStatus::Failed);
    let error = outcome.error.expect("run error");
    assert_eq!(error.code, "RUN_FOREACH_ITEMS_EXCEEDED");
    assert_eq!(error.message, "foreach node 'F' items 3 exceed max_items 2");
    assert!(sink.foreach_expansions().is_empty());
}

/// key_field 校验作用于 unwrap 后的列表（同 id=1 重复）。
#[tokio::test]
async fn key_duplicate_applies_after_unwrap() {
    let executor = Arc::new(
        ScriptedExecutor::new().respond("prep", |_| Ok(json!({"items": [{"id": 1}, {"id": 1}]}))),
    );
    let (outcome, sink) = common::run_with_sink(
        spec_for(
            &pipeline_json(r#", "key_field": "id""#, ""),
            foreach_options(),
        ),
        executor,
    )
    .await;
    assert_eq!(outcome.status, RunStatus::Failed);
    let error = outcome.error.expect("run error");
    assert_eq!(error.code, "RUN_FOREACH_ITEM_KEY_DUPLICATE");
    assert_eq!(
        error.message,
        "foreach node 'F' item key '1' is duplicated at indexes 0,1"
    );
    assert!(sink.foreach_expansions().is_empty());
}
