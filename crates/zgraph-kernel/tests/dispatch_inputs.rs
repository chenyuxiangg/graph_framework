//! 派发 inputs 构造：全部声明端口、未到为 null、all/any 就绪语义。

mod common;

use std::sync::Arc;

use serde_json::json;
use zgraph_kernel::{FakeExecutor, InstanceStatus, RunStatus};

#[tokio::test]
async fn all_trigger_fills_every_declared_port_with_null_for_missing() {
    let text = r#"{
      "version": "1.0",
      "metadata": {"name": "all-ports"},
      "entry_points": ["start"],
      "nodes": [
        {"id": "start", "type": "start", "inputs": [], "config": {}},
        {"id": "a", "type": "python_node",
         "inputs": [{"name": "i", "type": "any", "required": true}],
         "config": {"module": "workers.ports", "function": "a"}},
        {"id": "b", "type": "python_node",
         "inputs": [{"name": "i", "type": "any", "required": true}],
         "config": {"module": "workers.ports", "function": "b"}},
        {"id": "n", "type": "python_node",
         "inputs": [
           {"name": "x", "type": "any", "required": true},
           {"name": "y", "type": "any", "required": true},
           {"name": "z", "type": "any", "required": false}
         ],
         "trigger": "all",
         "config": {"module": "workers.ports", "function": "n"}},
        {"id": "done", "type": "end",
         "inputs": [{"name": "r", "type": "any", "required": true}], "config": {}}
      ],
      "edges": [
        {"from": "start", "to_node": "a", "to_input_port": "i"},
        {"from": "start", "to_node": "b", "to_input_port": "i"},
        {"from": "a", "to_node": "n", "to_input_port": "x"},
        {"from": "b", "to_node": "n", "to_input_port": "y"},
        {"from": "n", "to_node": "done", "to_input_port": "r"}
      ]
    }"#;
    let graph = common::compile_text(text);
    let executor = Arc::new(
        FakeExecutor::new()
            .respond("a", json!({"a": 1}))
            .respond("b", json!({"b": 1}))
            .respond("n", json!({"n": 1})),
    );
    let (outcome, sink) =
        common::run_with_sink(common::spec(graph, common::entry_inputs(&[])), executor).await;

    assert_eq!(outcome.status, RunStatus::Completed);
    // 全部声明端口都在 inputs 中；未连接的 optional 端口为 null。
    assert_eq!(
        common::running_input(&sink, "n"),
        Some(json!({"x": {"a": 1}, "y": {"b": 1}, "z": null}))
    );
    // all：x/y 都到齐才 Ready → Running。
    let statuses = common::statuses(&sink, "n");
    assert_eq!(statuses.first(), Some(&InstanceStatus::Pending));
    assert_eq!(statuses.last(), Some(&InstanceStatus::Completed));
    assert_eq!(outcome.output, Some(json!({"r": {"n": 1}})));
}

#[tokio::test]
async fn any_trigger_dispatches_with_unarrived_required_null() {
    let text = r#"{
      "version": "1.0",
      "metadata": {"name": "any-trigger"},
      "entry_points": ["start"],
      "nodes": [
        {"id": "start", "type": "start", "inputs": [], "config": {}},
        {"id": "a", "type": "python_node",
         "inputs": [{"name": "i", "type": "any", "required": true}],
         "config": {"module": "workers.any", "function": "a"}},
        {"id": "b", "type": "python_node",
         "inputs": [{"name": "i", "type": "any", "required": true}],
         "config": {"module": "workers.any", "function": "b"}},
        {"id": "n", "type": "python_node",
         "inputs": [
           {"name": "x", "type": "any", "required": true},
           {"name": "y", "type": "any", "required": true}
         ],
         "trigger": "any",
         "config": {"module": "workers.any", "function": "n"}},
        {"id": "done", "type": "end",
         "inputs": [{"name": "r", "type": "any", "required": true}], "config": {}}
      ],
      "edges": [
        {"from": "start", "to_node": "a", "to_input_port": "i"},
        {"from": "start", "to_node": "b", "to_input_port": "i"},
        {"from": "a", "to_node": "n", "to_input_port": "x"},
        {"from": "b", "to_node": "n", "to_input_port": "y",
         "gate": {"function": "check_output", "params": {"path": "ok", "equals": true}}},
        {"from": "n", "to_node": "done", "to_input_port": "r"}
      ]
    }"#;
    let graph = common::compile_text(text);
    let executor = Arc::new(
        FakeExecutor::new()
            .respond("a", json!({"a": 1}))
            .respond("b", json!({"ok": false}))
            .respond("n", json!({"n": 1})),
    );
    let (outcome, sink) =
        common::run_with_sink(common::spec(graph, common::entry_inputs(&[])), executor).await;

    assert_eq!(outcome.status, RunStatus::Completed);
    // any：x 到达即派发；未到的 required y 显式补 null（00 §0.7）。
    assert_eq!(
        common::running_input(&sink, "n"),
        Some(json!({"x": {"a": 1}, "y": null}))
    );
    assert_eq!(outcome.output, Some(json!({"r": {"n": 1}})));
}
