//! W3a0：人工等待生命周期 + `builtin.human` 决策应用（DR-006 §2/§3/§4；REV-080）。
//!
//! 覆盖：编译映射（见 `tests/compile.rs`）、`Pending→WaitingHuman` 旁路（不派发/无
//! 死锁/非静默）、决策 applied（output/transition/publish/GateResult）、applied=false
//! 三值、期限超时、取消/冷峻/Pause 交互。全部经库 API `RunControl::HumanDecision`
//! 驱动（无 SQLite 依赖，DR-006 §7.3 #7）。

mod common;

use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use zgraph_kernel::{
    start_run, FakeExecutor, HumanDecision, HumanDecisionRecord, InstanceSnapshot, InstanceStatus,
    MemoryTraceSink, NodeError, NodeExecution, NodeExecutor, RunControl, RunHandle, RunOptions,
    RunSpec, RunStatus,
};

// ---------------------------------------------------------------------------
// 图与辅助
// ---------------------------------------------------------------------------

/// 单人审批图：`start -> prep(python) -> review(builtin.human)`，review 的
/// approved/rejected 出边分别落到两个 end（reject 也是正常 Completed，去向由图决定）。
const HUMAN_JSON: &str = r#"{
  "version": "1.0",
  "metadata": {"name": "human-gate"},
  "entry_points": ["start"],
  "nodes": [
    {"id": "start", "type": "start", "inputs": [], "config": {}},
    {"id": "prep", "type": "python_node",
     "inputs": [{"name": "seed", "type": "any", "required": true}],
     "config": {"module": "workers.human", "function": "prep"}},
    {"id": "review", "type": "python_node",
     "inputs": [{"name": "payload", "type": "any", "required": true}],
     "config": {"module": "builtin", "function": "human"}},
    {"id": "end_approved", "type": "end",
     "inputs": [{"name": "r", "type": "any", "required": true}], "config": {}},
    {"id": "end_rejected", "type": "end",
     "inputs": [{"name": "r", "type": "any", "required": true}], "config": {}}
  ],
  "edges": [
    {"from": "start", "to_node": "prep", "to_input_port": "seed"},
    {"from": "prep", "to_node": "review", "to_input_port": "payload"},
    {"from": "review", "to_node": "end_approved", "to_input_port": "r",
     "gate": {"function": "check_output", "params": {"path": "decision", "equals": "approved"}}},
    {"from": "review", "to_node": "end_rejected", "to_input_port": "r",
     "gate": {"function": "check_output", "params": {"path": "decision", "equals": "rejected"}}}
  ]
}"#;

/// 双人审批链：`start -> prep -> review1(human) -> review2(human) -> done`。
/// 用于在 run 存活期间提交迟到/重复决策（不依赖终态后的控制通道行为）。
const HUMAN_CHAIN_JSON: &str = r#"{
  "version": "1.0",
  "metadata": {"name": "human-chain"},
  "entry_points": ["start"],
  "nodes": [
    {"id": "start", "type": "start", "inputs": [], "config": {}},
    {"id": "prep", "type": "python_node",
     "inputs": [{"name": "seed", "type": "any", "required": true}],
     "config": {"module": "workers.human", "function": "prep"}},
    {"id": "review1", "type": "python_node",
     "inputs": [{"name": "payload", "type": "any", "required": true}],
     "config": {"module": "builtin", "function": "human"}},
    {"id": "review2", "type": "python_node",
     "inputs": [{"name": "payload", "type": "any", "required": true}],
     "config": {"module": "builtin", "function": "human"}},
    {"id": "done", "type": "end",
     "inputs": [{"name": "r", "type": "any", "required": true}], "config": {}}
  ],
  "edges": [
    {"from": "start", "to_node": "prep", "to_input_port": "seed"},
    {"from": "prep", "to_node": "review1", "to_input_port": "payload"},
    {"from": "review1", "to_node": "review2", "to_input_port": "payload"},
    {"from": "review2", "to_node": "done", "to_input_port": "r"}
  ]
}"#;

/// W2a defer 返工图：`prep`/`review` 互指（均在含 defer 边的 SCC 内，满足 R15）。
/// rejected 决策经 defer 回边重入 `prep`（attempt-per-activation），产生新 review 实例。
const HUMAN_REWORK_JSON: &str = r#"{
  "version": "1.0",
  "metadata": {"name": "human-rework"},
  "entry_points": ["start"],
  "nodes": [
    {"id": "start", "type": "start", "inputs": [], "config": {}},
    {"id": "prep", "type": "python_node", "trigger": "any",
     "inputs": [{"name": "seed", "type": "any", "required": true},
                {"name": "feedback", "type": "any", "required": true}],
     "config": {"module": "workers.human", "function": "prep"}},
    {"id": "review", "type": "python_node",
     "inputs": [{"name": "payload", "type": "any", "required": true}],
     "config": {"module": "builtin", "function": "human"}},
    {"id": "done", "type": "end",
     "inputs": [{"name": "r", "type": "any", "required": true}], "config": {}}
  ],
  "edges": [
    {"from": "start", "to_node": "prep", "to_input_port": "seed"},
    {"from": "prep", "to_node": "review", "to_input_port": "payload",
     "gate": {"function": "check_output", "params": {"path": "ok", "equals": true}},
     "on_gate_false": "defer"},
    {"from": "review", "to_node": "prep", "to_input_port": "feedback",
     "gate": {"function": "check_output", "params": {"path": "decision", "equals": "rejected"}},
     "on_gate_false": "defer"},
    {"from": "review", "to_node": "done", "to_input_port": "r",
     "gate": {"function": "check_output", "params": {"path": "decision", "equals": "approved"}},
     "on_gate_false": "defer"}
  ]
}"#;

/// 冷峻失败图：`start` 同时投递 human 等待节点与一个会 Crashed 的执行节点。
const HUMAN_COLD_FAIL_JSON: &str = r#"{
  "version": "1.0",
  "metadata": {"name": "human-cold-fail"},
  "entry_points": ["start"],
  "nodes": [
    {"id": "start", "type": "start", "inputs": [], "config": {}},
    {"id": "review", "type": "python_node",
     "inputs": [{"name": "payload", "type": "any", "required": true}],
     "config": {"module": "builtin", "function": "human"}},
    {"id": "boom", "type": "python_node",
     "inputs": [{"name": "x", "type": "any", "required": true}],
     "config": {"module": "workers.human", "function": "boom"}}
  ],
  "edges": [
    {"from": "start", "to_node": "review", "to_input_port": "payload"},
    {"from": "start", "to_node": "boom", "to_input_port": "x"}
  ]
}"#;

/// 记录派发次数的执行器（验证 human 节点从不派发/从不启动 worker）。
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
        self.dispatched.lock().expect("dispatched lock").clone()
    }
}

impl NodeExecutor for CountingExecutor {
    fn execute<'a>(
        &'a self,
        execution: NodeExecution<'a>,
    ) -> Pin<Box<dyn Future<Output = Result<Value, NodeError>> + Send + 'a>> {
        self.dispatched
            .lock()
            .expect("dispatched lock")
            .push(execution.node.id.clone());
        self.inner.execute(execution)
    }
}

/// 默认选项：小 tick，便于测试期限/死锁判定；`deadlock_grace_ms` 尽量小。
fn human_options(deadlock_grace_ms: u64) -> RunOptions {
    common::options_with(|options| {
        options.tick_interval_ms = 5;
        options.deadlock_grace_ms = deadlock_grace_ms;
    })
}

/// inline DSL 图 + 选项的 `RunSpec`（无入口注入）。
fn human_spec(json: &str, options: RunOptions) -> RunSpec {
    let mut spec = common::spec(common::compile_text(json), common::entry_inputs(&[]));
    spec.options = options;
    spec
}

/// 轮询等待某节点**最新快照**达到指定状态（超时 panic）。
///
/// 必须看「最新快照」而非历史里出现过的状态：rework 场景下旧实例的
/// `WaitingHuman` 快照会留在列表里，只有最新快照代表实例当前状态。
async fn wait_for_status(
    sink: &MemoryTraceSink,
    node_id: &str,
    status: InstanceStatus,
) -> InstanceSnapshot {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let latest = sink
            .snapshots()
            .into_iter()
            .rev()
            .find(|snapshot| snapshot.node_id == node_id);
        if let Some(snapshot) = latest {
            if snapshot.status == status {
                return snapshot;
            }
        }
        assert!(
            Instant::now() < deadline,
            "node {node_id} did not reach {} in time",
            status.as_str()
        );
        tokio::time::sleep(Duration::from_millis(2)).await;
    }
}

/// 轮询等待 run 状态（超时 panic）。
async fn wait_for_run_status(handle: &RunHandle, status: RunStatus) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while handle.status() != status {
        assert!(
            Instant::now() < deadline,
            "run did not reach {status:?} in time (current {:?})",
            handle.status()
        );
        tokio::time::sleep(Duration::from_millis(2)).await;
    }
}

/// 轮询等待 `human_decided` 确认记录达到预期条数（控制通道异步处理）。
async fn wait_for_human_decisions(
    sink: &MemoryTraceSink,
    expected: usize,
) -> Vec<HumanDecisionRecord> {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let records = sink.human_decided();
        if records.len() >= expected {
            return records;
        }
        assert!(
            Instant::now() < deadline,
            "expected {expected} human_decided records, got {}",
            records.len()
        );
        tokio::time::sleep(Duration::from_millis(2)).await;
    }
}

/// 轮询等待某节点的指定 attempt 进入 `WaitingHuman`（rework 场景）。
async fn wait_for_attempt_waiting(
    sink: &MemoryTraceSink,
    node_id: &str,
    attempt: u32,
) -> InstanceSnapshot {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(snapshot) = sink.snapshots().into_iter().rev().find(|snapshot| {
            snapshot.node_id == node_id
                && snapshot.attempt == attempt
                && snapshot.status == InstanceStatus::WaitingHuman
        }) {
            return snapshot;
        }
        assert!(
            Instant::now() < deadline,
            "node {node_id} attempt {attempt} did not reach WaitingHuman in time"
        );
        tokio::time::sleep(Duration::from_millis(2)).await;
    }
}

/// 提交一次决策（固定 approver/decision_id/decided_at，便于断言透传）。
fn decide(
    handle: &RunHandle,
    instance_id: &str,
    decision: HumanDecision,
    reason: Option<&str>,
) -> bool {
    handle.control(RunControl::HumanDecision {
        instance_id: instance_id.to_string(),
        decision,
        approver: "alice".to_string(),
        reason: reason.map(str::to_string),
        decision_id: "0190f0d0-0000-7000-8000-000000000001".to_string(),
        decided_at: "2026-10-31T12:00:00.000Z".to_string(),
    })
}

// ---------------------------------------------------------------------------
// 1) 等待生命周期：不派发、非静默、无死锁
// ---------------------------------------------------------------------------

#[tokio::test]
async fn human_node_waits_without_dispatch_or_deadlock() {
    let executor = Arc::new(CountingExecutor::new(FakeExecutor::new()));
    let sink = Arc::new(MemoryTraceSink::new());
    let spec = human_spec(HUMAN_JSON, human_options(30));
    let handle = start_run(None, spec, executor.clone(), sink.clone()).expect("started");

    let waiting = wait_for_status(&sink, "review", InstanceStatus::WaitingHuman).await;

    // 迁移前写入 input 快照：全部声明端口（这里只有 payload 一个端口）。
    let input = waiting.input.as_ref().expect("input snapshot");
    let object = input.as_object().expect("input object");
    assert_eq!(object.len(), 1, "input snapshot carries all declared ports");
    assert!(object.contains_key("payload"));

    // 不经 Ready/Running；不派发 worker（执行器只见过 prep）。
    let review_transitions: Vec<_> = sink
        .transitions()
        .into_iter()
        .filter(|record| record.node_id == "review")
        .collect();
    assert_eq!(review_transitions.len(), 1);
    assert_eq!(review_transitions[0].from, InstanceStatus::Pending);
    assert_eq!(review_transitions[0].to, InstanceStatus::WaitingHuman);
    assert_eq!(
        review_transitions[0].reason.as_deref(),
        Some("human_waiting")
    );
    assert_eq!(executor.dispatched(), vec!["prep".to_string()]);
    assert!(sink
        .transitions()
        .iter()
        .all(|record| record.node_id != "review" || record.to != InstanceStatus::Running));

    // 纯等待不静默、不触发 RUN_DEADLOCK（超过 deadlock_grace_ms 仍 Running）。
    tokio::time::sleep(Duration::from_millis(120)).await;
    assert_eq!(handle.status(), RunStatus::Running);
    assert!(sink.human_decided().is_empty());

    // 取消：WaitingHuman → Cancelled，RunFinished{Cancelled} 恰一次。
    assert!(handle.cancel());
    let outcome = handle.wait().await;
    assert_eq!(outcome.status, RunStatus::Cancelled);
    assert_eq!(
        common::last_status(&sink, "review"),
        Some(InstanceStatus::Cancelled)
    );
    assert!(common::snapshots_of(&sink, "end_approved").is_empty());
    common::assert_lifecycle_once(&sink);
}

// ---------------------------------------------------------------------------
// 2) 决策 applied：output / transition / publish / GateResult
// ---------------------------------------------------------------------------

#[tokio::test]
async fn approved_decision_completes_and_publishes() {
    let executor = Arc::new(CountingExecutor::new(FakeExecutor::new()));
    let sink = Arc::new(MemoryTraceSink::new());
    let spec = human_spec(HUMAN_JSON, human_options(30));
    let handle = start_run(None, spec, executor.clone(), sink.clone()).expect("started");
    let waiting = wait_for_status(&sink, "review", InstanceStatus::WaitingHuman).await;

    assert!(decide(
        &handle,
        &waiting.instance_id,
        HumanDecision::Approved,
        Some("looks good")
    ));
    let outcome = handle.wait().await;
    assert_eq!(outcome.status, RunStatus::Completed);
    assert_eq!(executor.dispatched(), vec!["prep".to_string()]);

    // output 形状 + 快照在 transition 时已携带 output（C1 顺序冻结）。
    let completed = common::snapshots_of(&sink, "review")
        .into_iter()
        .find(|snapshot| snapshot.status == InstanceStatus::Completed)
        .expect("completed snapshot");
    assert_eq!(
        completed.output,
        Some(json!({"decision": "approved", "reason": "looks good"}))
    );
    let decided_transition = sink
        .transitions()
        .into_iter()
        .find(|record| record.node_id == "review" && record.to == InstanceStatus::Completed)
        .expect("human_decided transition");
    assert_eq!(decided_transition.reason.as_deref(), Some("human_decided"));

    // 出边 gate：approved 放行、rejected 未放行；approved 分支 end 完成。
    let review_gates: Vec<_> = sink
        .gate_results()
        .into_iter()
        .filter(|record| record.node_id == "review")
        .collect();
    assert_eq!(review_gates.len(), 2);
    let approved_gate = review_gates
        .iter()
        .find(|record| record.to_node == "end_approved")
        .expect("approved gate");
    assert!(approved_gate.passed);
    let rejected_gate = review_gates
        .iter()
        .find(|record| record.to_node == "end_rejected")
        .expect("rejected gate");
    assert!(!rejected_gate.passed);
    assert_eq!(
        common::last_status(&sink, "end_approved"),
        Some(InstanceStatus::Completed)
    );
    assert_eq!(
        common::last_status(&sink, "end_rejected"),
        Some(InstanceStatus::Skipped)
    );

    // 确认记录：applied=true、reject_reason=None、字段原样透传。
    let records = sink.human_decided();
    assert_eq!(records.len(), 1);
    let record = &records[0];
    assert_eq!(record.instance_id, waiting.instance_id);
    assert_eq!(record.decision, HumanDecision::Approved);
    assert_eq!(record.approver, "alice");
    assert_eq!(record.reason.as_deref(), Some("looks good"));
    assert_eq!(record.decision_id, "0190f0d0-0000-7000-8000-000000000001");
    assert_eq!(record.decided_at, "2026-10-31T12:00:00.000Z");
    assert!(record.applied);
    assert_eq!(record.reject_reason, None);

    // end 聚合输出 = approved end 的 inputs object。
    assert_eq!(
        outcome.output,
        Some(json!({"r": {"decision": "approved", "reason": "looks good"}}))
    );
    common::assert_lifecycle_once(&sink);
}

// ---------------------------------------------------------------------------
// 3) rejected 是正常 Completed（不隐式 Failed），去向交给图
// ---------------------------------------------------------------------------

#[tokio::test]
async fn rejected_decision_is_normal_completion() {
    let executor = Arc::new(FakeExecutor::new());
    let sink = Arc::new(MemoryTraceSink::new());
    let spec = human_spec(HUMAN_JSON, human_options(30));
    let handle = start_run(None, spec, executor, sink.clone()).expect("started");
    let waiting = wait_for_status(&sink, "review", InstanceStatus::WaitingHuman).await;

    assert!(decide(
        &handle,
        &waiting.instance_id,
        HumanDecision::Rejected,
        None
    ));
    let outcome = handle.wait().await;
    assert_eq!(outcome.status, RunStatus::Completed);
    assert!(outcome.error.is_none());

    let completed = common::snapshots_of(&sink, "review")
        .into_iter()
        .find(|snapshot| snapshot.status == InstanceStatus::Completed)
        .expect("completed snapshot");
    assert_eq!(
        completed.output,
        Some(json!({"decision": "rejected", "reason": null}))
    );
    assert_eq!(
        common::last_status(&sink, "end_rejected"),
        Some(InstanceStatus::Completed)
    );
    assert_eq!(
        common::last_status(&sink, "end_approved"),
        Some(InstanceStatus::Skipped)
    );
    common::assert_lifecycle_once(&sink);
}

// ---------------------------------------------------------------------------
// 4) applied=false 三值：not_waiting_human / instance_missing / not_current
// ---------------------------------------------------------------------------

#[tokio::test]
async fn late_decisions_report_not_waiting_and_instance_missing() {
    let executor = Arc::new(FakeExecutor::new());
    let sink = Arc::new(MemoryTraceSink::new());
    let spec = human_spec(HUMAN_CHAIN_JSON, human_options(30));
    let handle = start_run(None, spec, executor, sink.clone()).expect("started");

    let first = wait_for_status(&sink, "review1", InstanceStatus::WaitingHuman).await;
    assert!(decide(
        &handle,
        &first.instance_id,
        HumanDecision::Approved,
        None
    ));
    // review1 完成并投递到 review2 → review2 等待，run 存活。
    wait_for_status(&sink, "review2", InstanceStatus::WaitingHuman).await;
    assert_eq!(
        common::last_status(&sink, "review1"),
        Some(InstanceStatus::Completed)
    );

    // 迟到/重复决策：实例存在但已非 WaitingHuman。
    assert!(decide(
        &handle,
        &first.instance_id,
        HumanDecision::Approved,
        None
    ));
    // 目标不存在。
    assert!(decide(
        &handle,
        "missing-instance",
        HumanDecision::Approved,
        None
    ));

    // 两次迟到决策各恰一条 applied=false 确认；状态不变。
    {
        let records = wait_for_human_decisions(&sink, 3).await;
        assert_eq!(records.len(), 3);
        assert!(records[0].applied);
        assert!(!records[1].applied);
        assert_eq!(
            records[1].reject_reason.as_deref(),
            Some("not_waiting_human")
        );
        assert!(!records[2].applied);
        assert_eq!(
            records[2].reject_reason.as_deref(),
            Some("instance_missing")
        );
    }

    let second = wait_for_status(&sink, "review2", InstanceStatus::WaitingHuman).await;
    assert!(decide(
        &handle,
        &second.instance_id,
        HumanDecision::Approved,
        None
    ));
    let outcome = handle.wait().await;
    assert_eq!(outcome.status, RunStatus::Completed);
    assert_eq!(
        sink.human_decided().len(),
        4,
        "review2 approval is 4th record"
    );
    // 状态不被迟到决策改变：review1 仍 Completed 且只完成一次。
    assert_eq!(
        common::last_status(&sink, "review1"),
        Some(InstanceStatus::Completed)
    );
    common::assert_lifecycle_once(&sink);
}

/// W2a 返工：rejected 触发 defer 重入产生 review attempt=1（新实例），
/// 对旧实例（attempt=0）的迟到决策回报 `not_current`。
#[tokio::test]
async fn stale_decision_after_rework_reports_not_current() {
    let executor = Arc::new(FakeExecutor::new().respond("prep", json!({"ok": true})));
    let sink = Arc::new(MemoryTraceSink::new());
    let spec = human_spec(HUMAN_REWORK_JSON, human_options(30));
    let handle = start_run(None, spec, executor, sink.clone()).expect("started");

    let first = wait_for_status(&sink, "review", InstanceStatus::WaitingHuman).await;
    assert_eq!(first.attempt, 0);
    assert!(decide(
        &handle,
        &first.instance_id,
        HumanDecision::Rejected,
        Some("rework")
    ));

    // defer 回边 → prep attempt=1 → 新的 review 实例 attempt=1 等待
    //（旧实例 attempt=0 已完成，历史快照仍可在列表中查到）。
    let second = wait_for_attempt_waiting(&sink, "review", 1).await;
    let first_completed = common::snapshots_of(&sink, "review")
        .into_iter()
        .find(|snapshot| snapshot.attempt == 0 && snapshot.status == InstanceStatus::Completed)
        .expect("attempt 0 completed");
    assert_eq!(first_completed.instance_id, first.instance_id);
    assert_eq!(second.attempt, 1);
    assert_ne!(second.instance_id, first.instance_id);
    assert_eq!(
        second.status,
        InstanceStatus::WaitingHuman,
        "new attempt waits again"
    );

    // 旧实例已被新实例替换 → not_current（不改状态、不 fail run）。
    assert!(decide(
        &handle,
        &first.instance_id,
        HumanDecision::Approved,
        None
    ));
    let records = wait_for_human_decisions(&sink, 2).await;
    assert_eq!(records.len(), 2);
    assert!(records[0].applied);
    assert!(!records[1].applied);
    assert_eq!(records[1].reject_reason.as_deref(), Some("not_current"));
    assert_eq!(handle.status(), RunStatus::Running);

    // 新实例 approve → done（defer 出边放行）→ run Completed。
    assert!(decide(
        &handle,
        &second.instance_id,
        HumanDecision::Approved,
        None
    ));
    let outcome = handle.wait().await;
    assert_eq!(outcome.status, RunStatus::Completed);
    assert_eq!(
        common::last_status(&sink, "review"),
        Some(InstanceStatus::Completed)
    );
    assert_eq!(
        common::last_status(&sink, "done"),
        Some(InstanceStatus::Completed)
    );
    common::assert_lifecycle_once(&sink);
}

// ---------------------------------------------------------------------------
// 5) 决策期限（声明 timeout_ms 才计时；缺省无期限）
// ---------------------------------------------------------------------------

/// 给 HUMAN_JSON 的 human 节点加 `timeout_ms`。
fn human_json_with_timeout(timeout_ms: u64) -> String {
    HUMAN_JSON.replace(
        r#""config": {"module": "builtin", "function": "human"}"#,
        &format!(
            r#""config": {{"module": "builtin", "function": "human", "timeout_ms": {timeout_ms}}}"#
        ),
    )
}

#[tokio::test]
async fn decision_deadline_times_out_without_killing_workers() {
    let executor = Arc::new(FakeExecutor::new());
    let sink = Arc::new(MemoryTraceSink::new());
    let text = human_json_with_timeout(60);
    let spec = human_spec(&text, human_options(30));
    let handle = start_run(None, spec, executor, sink.clone()).expect("started");

    let outcome = handle.wait().await;
    assert_eq!(outcome.status, RunStatus::Failed);
    let error = outcome.error.expect("run error");
    assert_eq!(error.code, "NODE_TIMEOUT");
    // human 没有 worker 可 kill。
    assert_eq!(outcome.metrics.killed_workers, 0);
    assert_eq!(
        common::last_status(&sink, "review"),
        Some(InstanceStatus::TimedOut)
    );
    let timed_out = common::snapshots_of(&sink, "review")
        .into_iter()
        .find(|snapshot| snapshot.status == InstanceStatus::TimedOut)
        .expect("timed out snapshot");
    assert_eq!(timed_out.error_code.as_deref(), Some("NODE_TIMEOUT"));
    assert!(matches!(timed_out.error, Some(NodeError::Timeout { .. })));
    let events = sink.timed_out();
    assert_eq!(events.len(), 1);
    assert!(
        events[0].elapsed_ms >= 50,
        "elapsed {}",
        events[0].elapsed_ms
    );
    common::assert_lifecycle_once(&sink);
}

// ---------------------------------------------------------------------------
// 6) Pause 交互（决定仍应用、不恢复派发；期限不暂停计时）
// ---------------------------------------------------------------------------

#[tokio::test]
async fn paused_run_applies_decision_without_dispatching() {
    let executor = Arc::new(FakeExecutor::new());
    let sink = Arc::new(MemoryTraceSink::new());
    let spec = human_spec(HUMAN_JSON, human_options(30));
    let handle = start_run(None, spec, executor, sink.clone()).expect("started");

    let waiting = wait_for_status(&sink, "review", InstanceStatus::WaitingHuman).await;
    assert!(handle.pause());
    wait_for_run_status(&handle, RunStatus::Paused).await;

    assert!(decide(
        &handle,
        &waiting.instance_id,
        HumanDecision::Approved,
        None
    ));
    // 决策应用照常（实例 Completed + publish），但派发冻结：end 停在 Ready。
    let completed = wait_for_status(&sink, "review", InstanceStatus::Completed).await;
    assert_eq!(completed.node_id, "review");
    assert_eq!(handle.status(), RunStatus::Paused);
    assert_eq!(
        common::last_status(&sink, "end_approved"),
        Some(InstanceStatus::Ready)
    );

    assert!(handle.resume());
    let outcome = handle.wait().await;
    assert_eq!(outcome.status, RunStatus::Completed);
    assert_eq!(
        common::last_status(&sink, "end_approved"),
        Some(InstanceStatus::Completed)
    );
    common::assert_lifecycle_once(&sink);
}

#[tokio::test]
async fn decision_deadline_keeps_running_while_paused() {
    let executor = Arc::new(FakeExecutor::new());
    let sink = Arc::new(MemoryTraceSink::new());
    let text = human_json_with_timeout(60);
    let spec = human_spec(&text, human_options(30));
    let handle = start_run(None, spec, executor, sink.clone()).expect("started");

    wait_for_status(&sink, "review", InstanceStatus::WaitingHuman).await;
    assert!(handle.pause());
    wait_for_run_status(&handle, RunStatus::Paused).await;

    let outcome = handle.wait().await;
    assert_eq!(outcome.status, RunStatus::Failed);
    assert_eq!(outcome.error.expect("run error").code, "NODE_TIMEOUT");
    assert_eq!(outcome.metrics.killed_workers, 0);
    common::assert_lifecycle_once(&sink);
}

// ---------------------------------------------------------------------------
// 7) 冷峻失败级联 WaitingHuman → Cancelled
// ---------------------------------------------------------------------------

#[tokio::test]
async fn cold_failure_cancels_waiting_human() {
    let executor = Arc::new(FakeExecutor::new().fail(
        "boom",
        NodeError::Internal {
            message: "boom".to_string(),
        },
    ));
    let sink = Arc::new(MemoryTraceSink::new());
    let spec = human_spec(HUMAN_COLD_FAIL_JSON, human_options(30));
    let handle = start_run(None, spec, executor, sink.clone()).expect("started");

    let outcome = handle.wait().await;
    assert_eq!(outcome.status, RunStatus::Failed);
    assert_eq!(
        common::last_status(&sink, "review"),
        Some(InstanceStatus::Cancelled)
    );
    assert!(sink.human_decided().is_empty());
    common::assert_lifecycle_once(&sink);
}
