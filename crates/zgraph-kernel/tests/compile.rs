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

/// W2b：DSL 节点级 `max_fanout` 原样编译进 `CompiledNode.max_fanout`；
/// 未写该字段的节点保持 `None`（无运行期上限，v0.1.0 行为）。
#[test]
fn compiles_max_fanout_from_dsl() {
    let graph = common::compile_fixture("valid_max_fanout.json");
    // `start` 有 2 条出边且 max_fanout=2（R16 边界允许）→ 原样带出。
    let start = graph.nodes.get("start").expect("node");
    assert_eq!(start.max_fanout, Some(2));
    assert_eq!(
        graph.edge_index().outgoing_edges("start").len(),
        2,
        "fixture 处于 R16 边界（out-degree == max_fanout）"
    );
    // 未声明 max_fanout 的节点 → None。
    for node_id in ["a", "b", "done"] {
        let node = graph.nodes.get(node_id).expect("node");
        assert_eq!(node.max_fanout, None, "node {node_id}");
    }
}

/// W2b：子图节点同样编译 `max_fanout`（R16 在子图内部独立校验）。
#[test]
fn compiles_subgraph_node_max_fanout() {
    let text = r#"{
      "version": "1.0",
      "metadata": {"name": "subgraph-max-fanout"},
      "entry_points": ["start"],
      "nodes": [
        {"id": "start", "type": "start", "inputs": [], "config": {}},
        {"id": "sub", "type": "subgraph",
         "inputs": [{"name": "q", "type": "string", "required": true}],
         "config": {"subgraph_id": "inner"}},
        {"id": "done", "type": "end",
         "inputs": [{"name": "r", "type": "any", "required": true}], "config": {}}
      ],
      "edges": [
        {"from": "start", "to_node": "sub", "to_input_port": "q"},
        {"from": "sub", "to_node": "done", "to_input_port": "r"}
      ],
      "subgraphs": [
        {
          "id": "inner",
          "nodes": [
            {"id": "pin", "type": "python_node",
             "inputs": [{"name": "p", "type": "string", "required": true}],
             "max_fanout": 1,
             "config": {"module": "workers.inner", "function": "pin"}},
            {"id": "inner_end", "type": "end",
             "inputs": [{"name": "o", "type": "any", "required": true}], "config": {}}
          ],
          "edges": [
            {"from": "pin", "to_node": "inner_end", "to_input_port": "o"}
          ],
          "input_mapping": {"q": "pin.p"},
          "output_mapping": {"r": "inner_end.o"}
        }
      ]
    }"#;
    let graph = common::compile_text(text);
    let inner = graph.subgraphs.get("inner").expect("inner subgraph");
    assert_eq!(inner.nodes.get("pin").expect("pin").max_fanout, Some(1));
    assert_eq!(inner.nodes.get("inner_end").expect("end").max_fanout, None);
    // 主图 `sub` 未声明 → None（子图的 max_fanout 不冒泡到父图节点）。
    assert_eq!(graph.nodes.get("sub").expect("sub").max_fanout, None);
}

/// W3a0：`builtin.human` 精确映射为 `CompiledHandler::Human`（不构造 Python handler）；
/// 其它 `builtin.*` 仍是普通 `Python` handler（不存在隐含前缀）。
#[test]
fn compiles_builtin_human_handler() {
    let text = r#"{
      "version": "1.0",
      "metadata": {"name": "human-compile"},
      "entry_points": ["start"],
      "nodes": [
        {"id": "start", "type": "start", "inputs": [], "config": {}},
        {"id": "human", "type": "python_node",
         "inputs": [{"name": "payload", "type": "any", "required": true}],
         "config": {"module": "builtin", "function": "human", "timeout_ms": 250}},
        {"id": "other_builtin", "type": "python_node",
         "inputs": [{"name": "x", "type": "any", "required": true}],
         "config": {"module": "builtin", "function": "other"}},
        {"id": "worker", "type": "python_node",
         "inputs": [{"name": "p", "type": "any", "required": true}],
         "config": {"module": "workers.human", "function": "run"}}
      ],
      "edges": [
        {"from": "start", "to_node": "human", "to_input_port": "payload"},
        {"from": "start", "to_node": "other_builtin", "to_input_port": "x"},
        {"from": "human", "to_node": "worker", "to_input_port": "p"}
      ]
    }"#;
    let graph = common::compile_text(text);

    let human = graph.nodes.get("human").expect("human node");
    assert!(
        matches!(human.handler, CompiledHandler::Human),
        "builtin.human must compile to Human handler: {:?}",
        human.handler
    );
    // 期限走 `declared_timeout_ms`（human 缺省无期限，声明值才登记 deadline）。
    assert_eq!(human.declared_timeout_ms, Some(250));
    assert_eq!(human.max_attempts, zgraph_dsl::DEFAULT_MAX_ATTEMPTS);

    let other = graph.nodes.get("other_builtin").expect("other builtin");
    match &other.handler {
        CompiledHandler::Python {
            module, function, ..
        } => {
            assert_eq!(module, "builtin");
            assert_eq!(function, "other");
        }
        other => panic!("builtin.<other> must stay Python handler: {other:?}"),
    }

    let worker = graph.nodes.get("worker").expect("worker");
    assert!(matches!(worker.handler, CompiledHandler::Python { .. }));
}

/// W4a：`foreach` 区域编译（fixture `valid_foreach.json`，DSL R18 已校验）。
///
/// 断言 `CompiledForeach` 的 region/exit/join 映射与 `CompiledGraph.foreach_owner`
/// 归属（region 外节点不在地图中）。
#[test]
fn compiles_foreach_region_from_dsl() {
    let graph = common::compile_fixture("valid_foreach.json");

    let fanout = graph.nodes.get("F").expect("F");
    let foreach = fanout.foreach.as_ref().expect("F.foreach");
    assert_eq!(foreach.items_port, "items");
    assert_eq!(foreach.key_field.as_deref(), Some("id"));
    assert_eq!(foreach.join, "J");
    assert_eq!(foreach.max_items, Some(3));
    assert_eq!(foreach.exit_node, "E");
    assert_eq!(foreach.exit_edge, 3, "E -> J is edge index 3");
    assert_eq!(foreach.join_port, "results");
    assert_eq!(
        foreach.region,
        vec!["F", "W", "E"],
        "region keeps node order"
    );

    // region 归属（含 F 自身）；region 外节点不在表中。
    assert_eq!(graph.foreach_owner.get("F").map(String::as_str), Some("F"));
    assert_eq!(graph.foreach_owner.get("W").map(String::as_str), Some("F"));
    assert_eq!(graph.foreach_owner.get("E").map(String::as_str), Some("F"));
    assert!(!graph.foreach_owner.contains_key("J"));
    assert!(!graph.foreach_owner.contains_key("start"));
    assert!(!graph.foreach_owner.contains_key("end"));

    // 非 foreach 节点保持 None；无 foreach 的图没有 region 归属。
    assert!(graph.nodes.get("W").expect("W").foreach.is_none());
    let parallel = common::compile_fixture("valid_parallel.json");
    assert!(parallel.foreach_owner.is_empty());
    assert!(parallel.nodes.values().all(|node| node.foreach.is_none()));
}
