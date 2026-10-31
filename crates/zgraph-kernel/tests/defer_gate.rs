//! W2a0 defer 边（gate=false）：挂账不结算、dormant 完成、确定性死锁与 `end` 特例。
//!
//! 语义基准：DR-005 §3.2 决策矩阵行 2/5/8 与 §3.4 dormant/死锁判定表；覆盖 AC-14 f/g。
//! 所有测试图都通过完整 DSL 校验（含 R15 defer 可重入），与 e2e 冻结图同构。

mod common;

use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use zgraph_kernel::{
    start_run, FakeExecutor, GateResultRecord, InstanceStatus, MemoryTraceSink, RunOutcome,
    RunStatus,
};

/// 自环 defer + gate 恒 false（AC-14 f 冻结变体）：receipt 目标 loop 已完成 → dormant。
const DORMANT_SELF_LOOP_JSON: &str = r#"{
  "version": "1.0",
  "metadata": {"name": "defer-dormant"},
  "entry_points": ["start"],
  "nodes": [
    {"id": "start", "type": "start",
     "inputs": [{"name": "payload", "type": "any", "required": true}], "config": {}},
    {"id": "loop", "type": "python_node",
     "inputs": [
       {"name": "seed", "type": "any", "required": true},
       {"name": "again", "type": "any", "required": false}
     ],
     "config": {"module": "workers.defer", "function": "loop"}}
  ],
  "edges": [
    {"from": "start", "to_node": "loop", "to_input_port": "seed"},
    {"from": "loop", "to_node": "loop", "to_input_port": "again",
     "gate": {"function": "check_output", "params": {"path": "continue", "equals": true}},
     "on_gate_false": "defer"}
  ]
}"#;

/// A↔B 两条 gate+defer 边、首轮输出都不满足 gate：两个 receipt 的目标都已 Completed。
const CYCLE_FALSE_JSON: &str = r#"{
  "version": "1.0",
  "metadata": {"name": "defer-cycle-false"},
  "entry_points": ["start"],
  "nodes": [
    {"id": "start", "type": "start", "inputs": [], "config": {}},
    {"id": "a", "type": "python_node",
     "inputs": [
       {"name": "i", "type": "any", "required": true},
       {"name": "back", "type": "any", "required": false}
     ],
     "config": {"module": "workers.defer", "function": "a"}},
    {"id": "b", "type": "python_node",
     "inputs": [
       {"name": "i", "type": "any", "required": true},
       {"name": "fwd", "type": "any", "required": false}
     ],
     "config": {"module": "workers.defer", "function": "b"}}
  ],
  "edges": [
    {"from": "start", "to_node": "a", "to_input_port": "i"},
    {"from": "start", "to_node": "b", "to_input_port": "i"},
    {"from": "a", "to_node": "b", "to_input_port": "fwd",
     "gate": {"function": "check_output", "params": {"path": "done", "equals": true}},
     "on_gate_false": "defer"},
    {"from": "b", "to_node": "a", "to_input_port": "back",
     "gate": {"function": "check_output", "params": {"path": "again", "equals": true}},
     "on_gate_false": "defer"}
  ]
}"#;

/// defer 边目标 b 另有一条 settle 入边先到：b 保持 Pending（不结算、不 Skipped）。
const PENDING_TARGET_JSON: &str = r#"{
  "version": "1.0",
  "metadata": {"name": "defer-pending"},
  "entry_points": ["start"],
  "nodes": [
    {"id": "start", "type": "start", "inputs": [], "config": {}},
    {"id": "a", "type": "python_node",
     "inputs": [
       {"name": "i", "type": "any", "required": true},
       {"name": "back", "type": "any", "required": false}
     ],
     "config": {"module": "workers.defer", "function": "a"}},
    {"id": "c", "type": "python_node",
     "inputs": [{"name": "i", "type": "any", "required": true}],
     "config": {"module": "workers.defer", "function": "c"}},
    {"id": "b", "type": "python_node",
     "inputs": [
       {"name": "p", "type": "any", "required": true},
       {"name": "k", "type": "any", "required": true}
     ],
     "config": {"module": "workers.defer", "function": "b"}}
  ],
  "edges": [
    {"from": "start", "to_node": "a", "to_input_port": "i"},
    {"from": "start", "to_node": "c", "to_input_port": "i"},
    {"from": "c", "to_node": "b", "to_input_port": "k"},
    {"from": "a", "to_node": "b", "to_input_port": "p",
     "gate": {"function": "check_output", "params": {"path": "ok", "equals": true}},
     "on_gate_false": "defer"},
    {"from": "b", "to_node": "a", "to_input_port": "back"}
  ]
}"#;

/// settle 边回归：gate=false 仍结算 + Skipped 级联（deferred=false，v0.1.0 行为不变）。
const SETTLE_JSON: &str = r#"{
  "version": "1.0",
  "metadata": {"name": "settle-blocked"},
  "entry_points": ["start"],
  "nodes": [
    {"id": "start", "type": "start", "inputs": [], "config": {}},
    {"id": "a", "type": "python_node",
     "inputs": [{"name": "i", "type": "any", "required": true}],
     "config": {"module": "workers.defer", "function": "a"}},
    {"id": "b", "type": "python_node",
     "inputs": [{"name": "i", "type": "any", "required": true}],
     "config": {"module": "workers.defer", "function": "b"}}
  ],
  "edges": [
    {"from": "start", "to_node": "a", "to_input_port": "i"},
    {"from": "a", "to_node": "b", "to_input_port": "i",
     "gate": {"function": "check_output", "params": {"path": "ok", "equals": true}}}
  ]
}"#;

/// defer 边目标 end 已 Completed → RUN_DELIVERY_TO_TERMINAL（不 attempt+1；B2 特例）。
///
/// 边序：c 先发布 end.p（end 同步 Completed）再发布 a.i，保证 a 发布时 end 已终态。
const END_TERMINAL_JSON: &str = r#"{
  "version": "1.0",
  "metadata": {"name": "defer-end-terminal"},
  "entry_points": ["start"],
  "nodes": [
    {"id": "start", "type": "start", "inputs": [], "config": {}},
    {"id": "c", "type": "python_node",
     "inputs": [{"name": "i", "type": "any", "required": true}],
     "config": {"module": "workers.defer", "function": "c"}},
    {"id": "a", "type": "python_node",
     "inputs": [
       {"name": "i", "type": "any", "required": true},
       {"name": "loop", "type": "any", "required": false}
     ],
     "config": {"module": "workers.defer", "function": "a"}},
    {"id": "end", "type": "end",
     "inputs": [
       {"name": "p", "type": "any", "required": true},
       {"name": "q", "type": "any", "required": false}
     ], "config": {}}
  ],
  "edges": [
    {"from": "start", "to_node": "c", "to_input_port": "i"},
    {"from": "c", "to_node": "end", "to_input_port": "p"},
    {"from": "c", "to_node": "a", "to_input_port": "i"},
    {"from": "a", "to_node": "a", "to_input_port": "loop",
     "gate": {"function": "manual", "params": {}}, "on_gate_false": "defer"},
    {"from": "a", "to_node": "end", "to_input_port": "q",
     "gate": {"function": "manual", "params": {"default": true}},
     "on_gate_false": "defer"}
  ]
}"#;

/// blocked deferred（AC-14 g 的 R15-valid 冻结变体）：a 有 defer 自环保证可重入，
/// a→b 的 receipt 目标 b 从未实例化 → 确定性 RUN_DEADLOCK（a 自环的 receipt 为 dormant）。
const BLOCKED_JSON: &str = r#"{
  "version": "1.0",
  "metadata": {"name": "defer-blocked"},
  "entry_points": ["start"],
  "nodes": [
    {"id": "start", "type": "start", "inputs": [], "config": {}},
    {"id": "a", "type": "python_node",
     "inputs": [
       {"name": "seed", "type": "any", "required": true},
       {"name": "again", "type": "any", "required": false}
     ],
     "config": {"module": "workers.defer", "function": "a"}},
    {"id": "b", "type": "python_node",
     "inputs": [{"name": "x", "type": "any", "required": true}],
     "config": {"module": "workers.defer", "function": "b"}}
  ],
  "edges": [
    {"from": "start", "to_node": "a", "to_input_port": "seed"},
    {"from": "a", "to_node": "a", "to_input_port": "again",
     "gate": {"function": "manual", "params": {}}, "on_gate_false": "defer"},
    {"from": "a", "to_node": "b", "to_input_port": "x",
     "gate": {"function": "check_output", "params": {"path": "ok", "equals": true}},
     "on_gate_false": "defer"}
  ]
}"#;

/// 编译 + 运行一个内联图（入口注入覆盖 required 端口）。
async fn run_graph(
    text: &str,
    executor: FakeExecutor,
    inputs: &[(&str, &str, Value)],
) -> (RunOutcome, Arc<MemoryTraceSink>) {
    let spec = common::spec(common::compile_text(text), common::entry_inputs(inputs));
    common::run_with_sink(spec, Arc::new(executor)).await
}

/// 某节点每个 attempt 的**最终**状态（快照按迁移逐条记录，这里按 attempt 折叠）。
fn final_statuses(sink: &MemoryTraceSink, node_id: &str) -> Vec<(u32, InstanceStatus)> {
    let mut by_attempt: std::collections::BTreeMap<u32, InstanceStatus> =
        std::collections::BTreeMap::new();
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

/// 是否出现过某状态的实例快照。
fn has_status(sink: &MemoryTraceSink, status: InstanceStatus) -> bool {
    sink.snapshots()
        .iter()
        .any(|snapshot| snapshot.status == status)
}

/// 轮询等待某节点进入给定状态（超时 panic）。
async fn wait_for_status(sink: &MemoryTraceSink, node_id: &str, status: InstanceStatus) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    loop {
        if common::statuses(sink, node_id).contains(&status) {
            return;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "node {node_id} did not reach {status:?} in time"
        );
        tokio::time::sleep(Duration::from_millis(2)).await;
    }
}

#[tokio::test]
async fn defer_gate_false_self_loop_is_dormant_and_completes() {
    let executor = FakeExecutor::new().respond("loop", json!({"continue": false}));
    let (outcome, sink) = run_graph(
        DORMANT_SELF_LOOP_JSON,
        executor,
        &[("start", "payload", json!({"n": 1}))],
    )
    .await;

    assert_eq!(outcome.status, RunStatus::Completed);
    assert_eq!(outcome.error, None);
    assert_eq!(
        final_statuses(&sink, "loop"),
        vec![(0, InstanceStatus::Completed)],
        "defer false 不得重入、不得 Skipped"
    );
    assert!(!has_status(&sink, InstanceStatus::Skipped));
    assert_eq!(
        common::running_input(&sink, "loop"),
        Some(json!({"seed": {"payload": {"n": 1}}, "again": null}))
    );
    let records = gate_records(&sink, 1);
    assert_eq!(records.len(), 1);
    assert!(!records[0].passed);
    assert!(records[0].deferred, "defer 边 false 必须 deferred=true");
    assert_eq!(records[0].attempt, 0);
    assert_eq!(records[0].verdict, Some(json!(false)));
    common::assert_lifecycle_once(&sink);
}

#[tokio::test]
async fn defer_gate_false_non_self_loop_is_dormant_without_skip() {
    let executor = FakeExecutor::new()
        .respond("a", json!({"done": false}))
        .respond("b", json!({"again": false}));
    let (outcome, sink) = run_graph(CYCLE_FALSE_JSON, executor, &[]).await;

    assert_eq!(outcome.status, RunStatus::Completed);
    assert_eq!(outcome.error, None);
    for node in ["a", "b"] {
        assert_eq!(
            final_statuses(&sink, node),
            vec![(0, InstanceStatus::Completed)],
            "{node}: defer false 不结算 → 不 Skipped、不重入"
        );
    }
    assert!(!has_status(&sink, InstanceStatus::Skipped));
    for edge_index in [2, 3] {
        let records = gate_records(&sink, edge_index);
        assert_eq!(records.len(), 1, "edge {edge_index}");
        assert!(!records[0].passed);
        assert!(records[0].deferred);
        assert_eq!(records[0].attempt, 0);
        assert_eq!(records[0].verdict, Some(json!(false)));
    }
    for edge_index in [0, 1] {
        let records = gate_records(&sink, edge_index);
        assert_eq!(records.len(), 1);
        assert!(records[0].passed);
        assert!(!records[0].deferred);
        assert_eq!(records[0].verdict, None, "无 gate 边 verdict=None");
    }
}

#[tokio::test]
async fn defer_gate_false_keeps_pending_target_unsettled_until_cancel() {
    let graph = common::compile_text(PENDING_TARGET_JSON);
    let options = common::options_with(|options| {
        // 宽限期远大于取消时机：证明「不结算 → 不 Skipped」独立于死锁看门狗。
        options.deadlock_grace_ms = 60_000;
        options.tick_interval_ms = 10;
    });
    let spec = common::spec_with(graph, common::entry_inputs(&[]), options);
    let executor = Arc::new(FakeExecutor::new().respond("a", json!({"ok": false})));
    let sink = Arc::new(MemoryTraceSink::new());
    let handle = start_run(None, spec, executor, sink.clone()).expect("started");

    wait_for_status(&sink, "b", InstanceStatus::Pending).await;
    assert!(handle.cancel());
    let outcome = handle.wait().await;

    assert_eq!(outcome.status, RunStatus::Cancelled);
    assert_eq!(outcome.error, None);
    assert_eq!(
        common::last_status(&sink, "b"),
        Some(InstanceStatus::Cancelled)
    );
    assert!(
        !common::statuses(&sink, "b").contains(&InstanceStatus::Skipped),
        "defer 边 false 不结算 → b 不得 Skipped"
    );
    let records = gate_records(&sink, 3);
    assert_eq!(records.len(), 1);
    assert!(!records[0].passed);
    assert!(records[0].deferred);
    assert_eq!(records[0].attempt, 0);
    common::assert_lifecycle_once(&sink);
}

#[tokio::test]
async fn settle_gate_false_still_settles_and_skips_without_deferred() {
    let executor = FakeExecutor::new().respond("a", json!({"ok": false}));
    let (outcome, sink) = run_graph(SETTLE_JSON, executor, &[]).await;

    assert_eq!(outcome.status, RunStatus::Completed);
    assert_eq!(
        common::last_status(&sink, "b"),
        Some(InstanceStatus::Skipped)
    );
    let records = gate_records(&sink, 1);
    assert_eq!(records.len(), 1);
    assert!(!records[0].passed);
    assert!(
        !records[0].deferred,
        "settle 边 false 的 deferred 必须为 false"
    );
    assert_eq!(records[0].attempt, 0);
    let transition = sink
        .transitions()
        .into_iter()
        .find(|record| record.node_id == "b")
        .expect("b transition");
    assert_eq!(transition.from, InstanceStatus::Pending);
    assert_eq!(transition.to, InstanceStatus::Skipped);
    assert_eq!(transition.reason.as_deref(), Some("all_sources_blocked"));
}

#[tokio::test]
async fn defer_gate_true_to_completed_end_fails_delivery_to_terminal() {
    let executor = FakeExecutor::new()
        .respond("c", json!({"c": 1}))
        .respond("a", json!({"a": 1}));
    let (outcome, sink) = run_graph(END_TERMINAL_JSON, executor, &[]).await;

    assert_eq!(outcome.status, RunStatus::Failed);
    let error = outcome.error.expect("run error");
    assert_eq!(error.code, "RUN_DELIVERY_TO_TERMINAL");
    assert!(
        error.message.contains("edge 4"),
        "message: {}",
        error.message
    );
    assert!(
        error.message.contains("'end'"),
        "message: {}",
        error.message
    );
    assert_eq!(
        final_statuses(&sink, "end"),
        vec![(0, InstanceStatus::Completed)],
        "defer 边目标为 end 已 Completed 时不得 attempt+1"
    );
    assert_eq!(
        final_statuses(&sink, "a"),
        vec![(0, InstanceStatus::Completed)]
    );
    assert!(
        gate_records(&sink, 4).is_empty(),
        "寻址失败（RUN_DELIVERY_TO_TERMINAL）不产生 GateResult"
    );
    assert!(!has_status(&sink, InstanceStatus::Crashed));
    common::assert_lifecycle_once(&sink);
}

#[tokio::test]
async fn blocked_deferred_receipt_fails_immediately_with_deadlock() {
    let options = common::options_with(|options| {
        // 宽限 60s：确定性死锁必须在静默后立即触发，不等看门狗。
        options.deadlock_grace_ms = 60_000;
        options.tick_interval_ms = 1_000;
    });
    let spec = common::spec_with(
        common::compile_text(BLOCKED_JSON),
        common::entry_inputs(&[]),
        options,
    );
    let executor = Arc::new(FakeExecutor::new().respond("a", json!({"ok": false})));
    let started = Instant::now();
    let (outcome, sink) = common::run_with_sink(spec, executor).await;

    assert!(
        started.elapsed() < Duration::from_secs(2),
        "确定性 RUN_DEADLOCK 不得等 60s 宽限（耗时 {:?}）",
        started.elapsed()
    );
    assert_eq!(outcome.status, RunStatus::Failed);
    let error = outcome.error.expect("run error");
    assert_eq!(error.code, "RUN_DEADLOCK");
    assert!(
        error.message.contains("edge(s) [2]"),
        "message 需含 edge 索引: {}",
        error.message
    );
    assert!(
        error.message.contains("[b]"),
        "message 需含 target node: {}",
        error.message
    );
    assert_eq!(common::last_status(&sink, "b"), None, "b 从未实例化");
    assert_eq!(
        final_statuses(&sink, "a"),
        vec![(0, InstanceStatus::Completed)]
    );
    // a 自环的 receipt 目标 a 已 Completed → dormant；只有 a→b 的 receipt 阻塞。
    assert!(gate_records(&sink, 1)[0].deferred);
    assert!(gate_records(&sink, 2)[0].deferred);
    common::assert_lifecycle_once(&sink);
}
