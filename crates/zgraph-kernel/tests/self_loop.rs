//! 自环：每次投递新建实例 attempt+1；超过 `max_attempts` → 新实例 Crashed → run Failed。

mod common;

use std::sync::Arc;

use serde_json::json;
use zgraph_kernel::{FakeExecutor, InstanceStatus, RunStatus};

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
