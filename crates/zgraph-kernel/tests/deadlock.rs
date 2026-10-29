//! 死锁检测不误报：正常门控全拦截 → 级联 Skipped → Completed（AC-11 正例）。

mod common;

use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::json;
use zgraph_kernel::{FakeExecutor, InstanceStatus, RunStatus};

const GATED_JSON: &str = r#"{
  "version": "1.0",
  "metadata": {"name": "gated"},
  "entry_points": ["start"],
  "nodes": [
    {"id": "start", "type": "start", "inputs": [], "config": {}},
    {"id": "a", "type": "python_node",
     "inputs": [{"name": "i", "type": "any", "required": true}],
     "config": {"module": "workers.gated", "function": "a"}},
    {"id": "b", "type": "python_node",
     "inputs": [{"name": "i", "type": "any", "required": true}],
     "config": {"module": "workers.gated", "function": "b"}}
  ],
  "edges": [
    {"from": "start", "to_node": "a", "to_input_port": "i"},
    {"from": "a", "to_node": "b", "to_input_port": "i",
     "gate": {"function": "check_output", "params": {"path": "ok", "equals": true}}}
  ]
}"#;

#[tokio::test]
async fn all_gates_blocked_completes_without_deadlock_false_positive() {
    let options = common::options_with(|options| {
        options.deadlock_grace_ms = 50;
        options.tick_interval_ms = 10;
    });
    let spec = {
        let mut spec = common::spec(common::compile_text(GATED_JSON), common::entry_inputs(&[]));
        spec.options = options;
        spec
    };
    let executor = Arc::new(FakeExecutor::new().respond("a", json!({"ok": false})));

    let started = Instant::now();
    let (outcome, sink) = common::run_with_sink(spec, executor).await;
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "normal blocked run must finish promptly"
    );
    assert_eq!(outcome.status, RunStatus::Completed);
    assert_eq!(outcome.error, None);
    assert_eq!(
        common::last_status(&sink, "b"),
        Some(InstanceStatus::Skipped)
    );
    assert_eq!(outcome.output, None);
    common::assert_lifecycle_once(&sink);
}

#[tokio::test]
async fn manual_gate_default_false_blocks_branch() {
    let text = r#"{
      "version": "1.0",
      "metadata": {"name": "manual"},
      "entry_points": ["start"],
      "nodes": [
        {"id": "start", "type": "start", "inputs": [], "config": {}},
        {"id": "a", "type": "python_node",
         "inputs": [{"name": "i", "type": "any", "required": true}],
         "config": {"module": "workers.manual", "function": "a"}},
        {"id": "b", "type": "python_node",
         "inputs": [{"name": "i", "type": "any", "required": true}],
         "config": {"module": "workers.manual", "function": "b"}}
      ],
      "edges": [
        {"from": "start", "to_node": "a", "to_input_port": "i"},
        {"from": "a", "to_node": "b", "to_input_port": "i",
         "gate": {"function": "manual", "params": {}}}
      ]
    }"#;
    let graph = common::compile_text(text);
    let executor = Arc::new(FakeExecutor::new());
    let (outcome, sink) =
        common::run_with_sink(common::spec(graph, common::entry_inputs(&[])), executor).await;
    assert_eq!(outcome.status, RunStatus::Completed);
    assert_eq!(
        common::last_status(&sink, "b"),
        Some(InstanceStatus::Skipped)
    );
    let gate_results = sink.gate_results();
    let manual = gate_results
        .iter()
        .find(|record| record.gate_fn.as_deref() == Some("manual"))
        .expect("manual gate record");
    assert!(!manual.passed);
    assert_eq!(manual.error, None);
}
