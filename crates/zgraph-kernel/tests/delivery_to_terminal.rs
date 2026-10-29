//! 投递到非自环终态实例（01 §1.2.2）：`RUN_DELIVERY_TO_TERMINAL` 冷峻失败。

mod common;

use std::sync::Arc;

use zgraph_kernel::{FakeExecutor, InstanceStatus, RunStatus};

/// 门控环：`start -> a -> b -[gate]-> a`。
/// 首次 `start -> a` 让 a 执行完成；b 完成后经门控边再次投递 a 时，
/// a 的当前实例已是终态 → fail-fast。
const GATED_CYCLE_JSON: &str = r#"{
  "version": "1.0",
  "metadata": {"name": "gated-cycle"},
  "entry_points": ["start"],
  "nodes": [
    {"id": "start", "type": "start", "inputs": [], "config": {}},
    {"id": "a", "type": "python_node",
     "inputs": [
       {"name": "i", "type": "any", "required": true},
       {"name": "x", "type": "any", "required": true}
     ],
     "trigger": "any",
     "config": {"module": "workers.cycle", "function": "a"}},
    {"id": "b", "type": "python_node",
     "inputs": [{"name": "i", "type": "any", "required": true}],
     "config": {"module": "workers.cycle", "function": "b"}}
  ],
  "edges": [
    {"from": "start", "to_node": "a", "to_input_port": "i"},
    {"from": "a", "to_node": "b", "to_input_port": "i"},
    {"from": "b", "to_node": "a", "to_input_port": "x",
     "gate": {"function": "manual", "params": {"default": true}}}
  ]
}"#;

#[tokio::test]
async fn delivery_to_terminal_instance_fails_run() {
    let graph = common::compile_text(GATED_CYCLE_JSON);
    let executor = Arc::new(FakeExecutor::new());
    let (outcome, sink) =
        common::run_with_sink(common::spec(graph, common::entry_inputs(&[])), executor).await;

    assert_eq!(outcome.status, RunStatus::Failed);
    let error = outcome.error.expect("run error");
    assert_eq!(error.code, "RUN_DELIVERY_TO_TERMINAL");
    assert_eq!(outcome.output, None);
    // 已完成实例保留 Completed（冷峻收尾不修改终态实例）。
    assert_eq!(
        common::last_status(&sink, "a"),
        Some(InstanceStatus::Completed)
    );
    assert_eq!(
        common::last_status(&sink, "b"),
        Some(InstanceStatus::Completed)
    );
    // 注意：投递到终态是寻址阶段失败（01 §1.4 先解析目标再写 GateResult），
    // 因此这条边不产生 GateResultRecord；已产生的边记录都属于成功发布。
    assert!(sink
        .gate_results()
        .iter()
        .all(|record| record.to_node != "a" || record.node_id != "b"));
    common::assert_lifecycle_once(&sink);
    let finished = sink.finished().into_iter().next().expect("finished");
    assert_eq!(finished.status, RunStatus::Failed);
    assert_eq!(
        finished.error.expect("finished error").code,
        "RUN_DELIVERY_TO_TERMINAL"
    );
}
