//! `CompiledGraph::compile` 与边索引的集成测试。

mod common;

use std::collections::BTreeMap;

use zgraph_dsl::{ExecClass, GateOp, NodeType, OnGateFalse, Trigger};
use zgraph_kernel::{CompiledGraph, CompiledHandler};

#[test]
fn compiles_customer_ticket_fixture() {
    let graph = common::compile_fixture("customer_ticket.json");
    assert_eq!(graph.graph_hash.len(), 16);
    assert_eq!(graph.metadata.name, "客服工单处理流");
    assert!(!graph.dsl_json.is_empty());
    assert_eq!(
        graph.node_order,
        vec![
            "input_parser",
            "kb_search",
            "answer_generator",
            "human_escalation_check",
            "human_handoff",
            "end_success",
        ]
    );
    assert_eq!(graph.entry_points, vec!["input_parser".to_string()]);
    assert_eq!(graph.edges.len(), 6);
    for (index, edge) in graph.edges.iter().enumerate() {
        assert_eq!(edge.index, index, "edge index equals array position");
    }
    // 边索引：出边/入边/节点入边。
    let index = graph.edge_index();
    assert_eq!(index.outgoing_edges("input_parser"), &[0, 2]);
    assert_eq!(index.incoming_edge("kb_search", "query"), Some(0));
    assert_eq!(index.incoming_edge("kb_search", "missing"), None);
    assert_eq!(index.incoming_edges("answer_generator"), &[1, 2]);
    assert_eq!(index.incoming_edges("input_parser"), &[] as &[usize]);
}

#[test]
fn compiles_handlers_and_timeouts() {
    let graph = common::compile_fixture("customer_ticket.json");
    let parser = graph.nodes.get("input_parser").expect("node");
    assert_eq!(parser.node_type, NodeType::AgentCall);
    assert_eq!(parser.timeout_ms, 60_000);
    assert_eq!(parser.max_attempts, zgraph_dsl::DEFAULT_MAX_ATTEMPTS);
    assert_eq!(parser.trigger, Trigger::All);
    match &parser.handler {
        CompiledHandler::Agent {
            agent_id,
            input_mapping,
            max_iterations,
        } => {
            assert_eq!(agent_id, "message_parser");
            assert_eq!(input_mapping, &BTreeMap::new());
            assert_eq!(*max_iterations, None);
        }
        other => panic!("unexpected handler: {other:?}"),
    }

    let kb = graph.nodes.get("kb_search").expect("node");
    assert_eq!(kb.timeout_ms, zgraph_dsl::DEFAULT_NODE_TIMEOUT_MS);

    let check = graph.nodes.get("human_escalation_check").expect("node");
    match &check.handler {
        CompiledHandler::Python {
            module,
            function,
            exec_class,
        } => {
            assert_eq!(module, "gates.hr");
            assert_eq!(function, "need_human_check");
            assert_eq!(*exec_class, ExecClass::Io);
        }
        other => panic!("unexpected handler: {other:?}"),
    }

    let end = graph.nodes.get("end_success").expect("node");
    assert_eq!(end.node_type, NodeType::End);
    assert!(matches!(end.handler, CompiledHandler::End));
}

#[test]
fn compiles_gate_op_from_dsl() {
    let graph = common::compile_fixture("customer_ticket.json");
    let edge = graph.edges.get(4).expect("gated edge");
    assert_eq!(edge.from, "human_escalation_check");
    assert_eq!(edge.to_node, "human_handoff");
    assert_eq!(edge.to_port, "context");
    let gate = edge.gate.as_ref().expect("gate");
    assert_eq!(gate.function, "check_output");
    assert_eq!(gate.op, GateOp::CheckOutput);
    assert_eq!(
        gate.params,
        serde_json::json!({"path": "need_human", "equals": true})
    );
    // 无 gate 边保持 None。
    assert!(graph.edges.first().expect("edge").gate.is_none());
}

#[test]
fn compiles_retry_and_timeout_overrides() {
    let text = r#"{
      "version": "1.0",
      "metadata": {"name": "overrides"},
      "entry_points": ["start"],
      "nodes": [
        {"id": "start", "type": "start", "inputs": [], "config": {}},
        {"id": "n", "type": "python_node",
         "inputs": [{"name": "x", "type": "any", "required": true}],
         "config": {"module": "m", "function": "f", "timeout_ms": 1234,
                    "retry": {"max_attempts": 5}}},
        {"id": "e", "type": "end", "inputs": [{"name": "r", "type": "any", "required": true}],
         "config": {}}
      ],
      "edges": [
        {"from": "start", "to_node": "n", "to_input_port": "x"},
        {"from": "n", "to_node": "e", "to_input_port": "r"}
      ]
    }"#;
    let graph = common::compile_text(text);
    let node = graph.nodes.get("n").expect("node");
    assert_eq!(node.timeout_ms, 1_234);
    // ISSUE-076：declared 保留"未声明"语义，`timeout_ms` 维持历史回退。
    assert_eq!(node.declared_timeout_ms, Some(1_234));
    assert_eq!(node.max_attempts, 5);
}

/// 未声明 `timeout_ms` 的节点：`declared_timeout_ms = None`（供执行器 hint 回退），
/// `timeout_ms` 仍为历史默认值（spec 05 §5.2.6）。
#[test]
fn compiles_undeclared_timeout_as_none_with_default_fallback() {
    let graph = common::compile_fixture("customer_ticket.json");
    for node_id in ["kb_search", "human_escalation_check", "end_success"] {
        let node = graph.nodes.get(node_id).expect("node");
        assert_eq!(node.declared_timeout_ms, None, "node {node_id}");
        assert_eq!(
            node.timeout_ms,
            zgraph_dsl::DEFAULT_NODE_TIMEOUT_MS,
            "node {node_id}"
        );
    }
    // agent_call 显式声明 → declared 保留 DSL 原值。
    let declared = graph.nodes.get("input_parser").expect("input_parser");
    assert_eq!(declared.declared_timeout_ms, Some(60_000));
    assert_eq!(declared.timeout_ms, 60_000);
}

#[test]
fn compiles_subgraphs_map() {
    let graph = common::compile_fixture("valid_subgraph.json");
    assert_eq!(graph.subgraphs.len(), 1);
    let inner = graph.subgraphs.get("inner").expect("inner subgraph");
    assert_eq!(inner.node_order, vec!["pin", "inner_end"]);
    assert_eq!(inner.entry_points, Vec::<String>::new());
    assert_eq!(inner.graph_hash, graph.graph_hash);
    let sub_node = graph.nodes.get("sub").expect("subgraph node");
    match &sub_node.handler {
        CompiledHandler::Subgraph { subgraph_id } => assert_eq!(subgraph_id, "inner"),
        other => panic!("unexpected handler: {other:?}"),
    }
}

#[test]
fn compiles_parallel_fixture_entry_order() {
    let graph = common::compile_fixture("valid_parallel.json");
    assert_eq!(graph.entry_points, vec!["start".to_string()]);
    assert_eq!(graph.node_order, vec!["start", "a", "b", "done"]);
    assert_eq!(
        graph.edge_index().outgoing_edges("start"),
        &[0, 1],
        "outgoing edges keep DSL order"
    );
}

#[test]
fn compile_returns_arc_with_stable_hash() {
    let first = common::compile_fixture("customer_ticket.json");
    let loaded =
        zgraph_dsl::load_graph(&common::fixture_bytes("customer_ticket.json")).expect("load");
    let second = CompiledGraph::compile(&loaded).expect("compile");
    assert_eq!(first.graph_hash, second.graph_hash);
    assert_eq!(first.dsl_json, second.dsl_json);
}

/// W2a0：`CompiledEdge.on_gate_false` 编译期补齐——缺省 `Settle`、显式 `defer` 保留。
#[test]
fn compiles_on_gate_false_policy_and_defaults() {
    let text = r#"{
      "version": "1.0",
      "metadata": {"name": "defer-policy"},
      "entry_points": ["start"],
      "nodes": [
        {"id": "start", "type": "start", "inputs": [], "config": {}},
        {"id": "loop", "type": "python_node",
         "inputs": [
           {"name": "seed", "type": "any", "required": true},
           {"name": "again", "type": "any", "required": false}
         ],
         "config": {"module": "m", "function": "f"}}
      ],
      "edges": [
        {"from": "start", "to_node": "loop", "to_input_port": "seed"},
        {"from": "loop", "to_node": "loop", "to_input_port": "again",
         "gate": {"function": "manual", "params": {}},
         "on_gate_false": "defer"}
      ]
    }"#;
    let graph = common::compile_text(text);
    assert_eq!(graph.edges[0].on_gate_false, OnGateFalse::Settle);
    assert_eq!(graph.edges[1].on_gate_false, OnGateFalse::Defer);

    // 显式 "settle" 与缺省同语义（hash 差异属 DSL 层，本测试只锁定编译产物）。
    let explicit = text.replace(
        r#""on_gate_false": "defer""#,
        r#""on_gate_false": "settle""#,
    );
    let graph = common::compile_text(&explicit);
    assert_eq!(graph.edges[1].on_gate_false, OnGateFalse::Settle);
}
