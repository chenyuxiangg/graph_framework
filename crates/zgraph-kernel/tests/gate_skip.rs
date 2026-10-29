//! 门控拦截、Skipped 级联与 publish 顺序/每边一次。

mod common;

use std::sync::Arc;

use serde_json::json;
use zgraph_kernel::{FakeExecutor, InstanceStatus, RunStatus};

const BRANCHED_JSON: &str = r#"{
  "version": "1.0",
  "metadata": {"name": "branched"},
  "entry_points": ["start"],
  "nodes": [
    {"id": "start", "type": "start", "inputs": [], "config": {}},
    {"id": "a", "type": "python_node",
     "inputs": [{"name": "i", "type": "any", "required": true}],
     "config": {"module": "workers.branch", "function": "a"}},
    {"id": "b", "type": "python_node",
     "inputs": [{"name": "i", "type": "any", "required": true}],
     "config": {"module": "workers.branch", "function": "b"}},
    {"id": "c", "type": "python_node",
     "inputs": [{"name": "i", "type": "any", "required": true}],
     "config": {"module": "workers.branch", "function": "c"}},
    {"id": "end_b", "type": "end",
     "inputs": [{"name": "r", "type": "any", "required": true}], "config": {}},
    {"id": "end_c", "type": "end",
     "inputs": [{"name": "r", "type": "any", "required": true}], "config": {}}
  ],
  "edges": [
    {"from": "start", "to_node": "a", "to_input_port": "i"},
    {"from": "a", "to_node": "b", "to_input_port": "i",
     "gate": {"function": "check_output", "params": {"path": "ok", "equals": true}}},
    {"from": "a", "to_node": "c", "to_input_port": "i"},
    {"from": "b", "to_node": "end_b", "to_input_port": "r"},
    {"from": "c", "to_node": "end_c", "to_input_port": "r"}
  ]
}"#;

#[tokio::test]
async fn gate_block_skips_one_branch_and_keeps_the_other() {
    let graph = common::compile_text(BRANCHED_JSON);
    let executor = Arc::new(
        FakeExecutor::new()
            .respond("a", json!({"ok": false}))
            .respond("b", json!({"b": 1}))
            .respond("c", json!({"c": 1})),
    );
    let (outcome, sink) =
        common::run_with_sink(common::spec(graph, common::entry_inputs(&[])), executor).await;

    assert_eq!(outcome.status, RunStatus::Completed);
    assert_eq!(
        common::last_status(&sink, "a"),
        Some(InstanceStatus::Completed)
    );
    assert_eq!(
        common::last_status(&sink, "b"),
        Some(InstanceStatus::Skipped)
    );
    assert_eq!(
        common::last_status(&sink, "c"),
        Some(InstanceStatus::Completed)
    );
    // 被拦截分支的下游级联 Skipped；另一分支的 end 正常完成。
    assert_eq!(
        common::last_status(&sink, "end_b"),
        Some(InstanceStatus::Skipped)
    );
    assert_eq!(
        common::last_status(&sink, "end_c"),
        Some(InstanceStatus::Completed)
    );
    // 单 end 完成：run_output = end_c.inputs。
    assert_eq!(outcome.output, Some(json!({"r": {"c": 1}})));

    // 每条出边恰好一条 GateResultRecord（含无 gate 边）；按 EdgeIndex 升序记录。
    let gate_results = sink.gate_results();
    // 边 0/1/2/4 的源节点都执行过 publish（b 被 Skipped，故 3 无记录）。
    assert_eq!(gate_results.len(), 4);
    let a_records: Vec<usize> = gate_results
        .iter()
        .filter(|record| record.node_id == "a")
        .map(|record| record.edge_index)
        .collect();
    assert_eq!(
        a_records,
        vec![1, 2],
        "publish must iterate edges ascending"
    );
    assert_eq!(
        gate_results
            .iter()
            .filter(|record| record.node_id == "a" && !record.passed)
            .count(),
        1
    );
    assert_eq!(
        gate_results
            .iter()
            .filter(|record| record.gate_fn.is_none())
            .count(),
        3,
        "ungated edges (start→a, a→c, c→end_c) are still recorded"
    );
    for record in &gate_results {
        assert_eq!(record.to_node, record.to_node.clone());
        assert_eq!(record.run_id, outcome.run_id);
        assert!(record.ts.ends_with('Z'));
    }
}

#[tokio::test]
async fn skip_cascades_through_multiple_downstream_nodes() {
    let text = r#"{
      "version": "1.0",
      "metadata": {"name": "cascade"},
      "entry_points": ["start"],
      "nodes": [
        {"id": "start", "type": "start", "inputs": [], "config": {}},
        {"id": "a", "type": "python_node",
         "inputs": [{"name": "i", "type": "any", "required": true}],
         "config": {"module": "workers.cascade", "function": "a"}},
        {"id": "b", "type": "python_node",
         "inputs": [{"name": "i", "type": "any", "required": true}],
         "config": {"module": "workers.cascade", "function": "b"}},
        {"id": "c", "type": "python_node",
         "inputs": [{"name": "i", "type": "any", "required": true}],
         "config": {"module": "workers.cascade", "function": "c"}},
        {"id": "done", "type": "end",
         "inputs": [{"name": "r", "type": "any", "required": true}], "config": {}}
      ],
      "edges": [
        {"from": "start", "to_node": "a", "to_input_port": "i"},
        {"from": "a", "to_node": "b", "to_input_port": "i",
         "gate": {"function": "check_output", "params": {"path": "ok", "equals": true}}},
        {"from": "b", "to_node": "c", "to_input_port": "i"},
        {"from": "c", "to_node": "done", "to_input_port": "r"}
      ]
    }"#;
    let graph = common::compile_text(text);
    let executor = Arc::new(FakeExecutor::new().respond("a", json!({"ok": false})));
    let (outcome, sink) =
        common::run_with_sink(common::spec(graph, common::entry_inputs(&[])), executor).await;

    assert_eq!(outcome.status, RunStatus::Completed);
    assert_eq!(outcome.output, None, "all ends skipped → no output");
    for node_id in ["b", "c", "done"] {
        assert_eq!(
            common::last_status(&sink, node_id),
            Some(InstanceStatus::Skipped),
            "{node_id} must be skipped via cascade"
        );
        let transitions = sink
            .transitions()
            .into_iter()
            .filter(|record| record.node_id == node_id)
            .collect::<Vec<_>>();
        assert_eq!(
            transitions.len(),
            1,
            "skipped node has exactly one transition"
        );
        assert_eq!(transitions[0].from, InstanceStatus::Pending);
        assert_eq!(transitions[0].to, InstanceStatus::Skipped);
        assert_eq!(
            transitions[0].reason.as_deref(),
            Some("all_sources_blocked")
        );
    }
}

#[tokio::test]
async fn any_trigger_node_runs_even_when_one_incoming_edge_is_blocked() {
    // join 有两条入边：a 的（放行）与 d 的（门控拦截）；trigger=any 时 x 到齐即可执行。
    let text = r#"{
      "version": "1.0",
      "metadata": {"name": "join"},
      "entry_points": ["start"],
      "nodes": [
        {"id": "start", "type": "start", "inputs": [], "config": {}},
        {"id": "a", "type": "python_node",
         "inputs": [{"name": "i", "type": "any", "required": true}],
         "config": {"module": "workers.join", "function": "a"}},
        {"id": "d", "type": "python_node",
         "inputs": [{"name": "i", "type": "any", "required": true}],
         "config": {"module": "workers.join", "function": "d"}},
        {"id": "join", "type": "python_node",
         "inputs": [{"name": "x", "type": "any", "required": true},
                    {"name": "y", "type": "any", "required": true}],
         "trigger": "any",
         "config": {"module": "workers.join", "function": "join"}},
        {"id": "done", "type": "end",
         "inputs": [{"name": "r", "type": "any", "required": true}], "config": {}}
      ],
      "edges": [
        {"from": "start", "to_node": "a", "to_input_port": "i"},
        {"from": "start", "to_node": "d", "to_input_port": "i"},
        {"from": "a", "to_node": "join", "to_input_port": "x"},
        {"from": "d", "to_node": "join", "to_input_port": "y",
         "gate": {"function": "check_output", "params": {"path": "ok", "equals": true}}},
        {"from": "join", "to_node": "done", "to_input_port": "r"}
      ]
    }"#;
    let graph = common::compile_text(text);
    let executor = Arc::new(
        FakeExecutor::new()
            .respond("a", json!({"a": 1}))
            .respond("d", json!({"ok": false}))
            .respond("join", json!({"joined": true})),
    );
    let (outcome, sink) =
        common::run_with_sink(common::spec(graph, common::entry_inputs(&[])), executor).await;

    assert_eq!(outcome.status, RunStatus::Completed);
    // d 自身执行完成，只是它的出边被门控拦截（拦截的是边，不是节点）。
    assert_eq!(
        common::last_status(&sink, "d"),
        Some(InstanceStatus::Completed)
    );
    // join trigger=any：x 已到即可派发，y 未到补 null。
    assert_eq!(
        common::last_status(&sink, "join"),
        Some(InstanceStatus::Completed)
    );
    assert_eq!(
        common::running_input(&sink, "join"),
        Some(json!({"x": {"a": 1}, "y": null}))
    );
    assert_eq!(outcome.output, Some(json!({"r": {"joined": true}})));
}
