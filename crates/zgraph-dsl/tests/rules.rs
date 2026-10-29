//! R1–R14 的规则级测试：每条规则至少一个正例与一个反例。
//!
//! 通过公开 API（`load_graph_value` / `load_graph_with_options`）驱动，
//! 断言错误码、定位与 expected/actual；fixtures 在 `fixtures.rs` 中逐文件覆盖。

use std::path::PathBuf;

use serde_json::{json, Value};
use zgraph_dsl::{
    load_graph, load_graph_value, load_graph_with_options, DslErrorCode, DslErrors, LoadOptions,
    LoadedGraph,
};

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("repo root")
}

fn fixtures_dir() -> PathBuf {
    repo_root().join("tests/fixtures/graphs")
}

fn assets_dir() -> PathBuf {
    repo_root().join("tests/fixtures/assets")
}

fn fixture(name: &str) -> PathBuf {
    fixtures_dir().join(name)
}

fn load_fixture(name: &str) -> Result<LoadedGraph, DslErrors> {
    let bytes = std::fs::read(fixture(name)).expect("fixture readable");
    load_graph(&bytes)
}

fn load_fixture_with_assets(name: &str) -> Result<LoadedGraph, DslErrors> {
    let bytes = std::fs::read(fixture(name)).expect("fixture readable");
    load_graph_with_options(&bytes, &LoadOptions::new().with_assets_dir(assets_dir()))
}

fn load(value: Value) -> Result<LoadedGraph, DslErrors> {
    load_graph_value(value, &LoadOptions::new())
}

fn expect_error(value: Value) -> DslErrors {
    load(value).expect_err("graph must be rejected")
}

fn expect_code(value: Value, code: DslErrorCode) -> DslErrors {
    let errors = expect_error(value);
    assert_eq!(errors.first().code, code, "errors: {errors}");
    errors
}

fn minimal_graph() -> Value {
    json!({
        "version": "1.0",
        "metadata": {"name": "minimal"},
        "entry_points": ["start"],
        "nodes": [{"id": "start", "type": "start", "inputs": [], "config": {}}],
        "edges": []
    })
}

fn graph_with(nodes: Value, edges: Value) -> Value {
    json!({
        "version": "1.0",
        "metadata": {"name": "case"},
        "entry_points": ["start"],
        "nodes": nodes,
        "edges": edges
    })
}

fn input_port(name: &str, required: bool) -> Value {
    json!({"name": name, "type": "string", "required": required})
}

fn python_node(id: &str, inputs: Value) -> Value {
    json!({
        "id": id,
        "type": "python_node",
        "inputs": inputs,
        "config": {"module": "m", "function": id}
    })
}

fn node(id: &str, node_type: &str, inputs: Value, config: Value) -> Value {
    json!({"id": id, "type": node_type, "inputs": inputs, "config": config})
}

fn end_node(id: &str, port: &str) -> Value {
    json!({
        "id": id,
        "type": "end",
        "inputs": [{"name": port, "type": "any", "required": true}],
        "config": {}
    })
}

fn edge(from: &str, to_node: &str, to_input_port: &str) -> Value {
    json!({"from": from, "to_node": to_node, "to_input_port": to_input_port})
}

fn gated_edge(from: &str, to_node: &str, to_input_port: &str, gate: Value) -> Value {
    json!({
        "from": from,
        "to_node": to_node,
        "to_input_port": to_input_port,
        "gate": gate
    })
}

fn start_node(id: &str) -> Value {
    node(id, "start", json!([]), json!({}))
}

// ---------------------------------------------------------------- R1

#[test]
fn r1_invalid_json_reports_parse_error_with_location() {
    let errors = load_graph(b"{,\"version\": \"1.0\"}").expect_err("invalid JSON");
    assert_eq!(errors.first().code, DslErrorCode::ParseError);
    assert!(errors.first().message.contains("line"));
    assert!(errors.first().location.is_empty());
}

#[test]
fn r1_top_level_must_be_object() {
    let errors = load_graph(b"[1, 2, 3]").expect_err("array is not a graph");
    assert_eq!(errors.first().code, DslErrorCode::ParseError);
}

// ---------------------------------------------------------------- R2

#[test]
fn r2_missing_top_level_field_is_document_level() {
    let mut graph = minimal_graph();
    graph.as_object_mut().expect("object").remove("edges");
    let errors = expect_code(graph, DslErrorCode::MissingField);
    assert!(
        errors.first().location.is_empty(),
        "R2 文档级 location 必须为空: {:?}",
        errors.first().location
    );
    assert!(errors.first().message.contains("edges"));
}

#[test]
fn r2_unknown_top_level_field_is_rejected() {
    let mut graph = minimal_graph();
    graph
        .as_object_mut()
        .expect("object")
        .insert("extra".to_string(), json!(true));
    let errors = expect_code(graph, DslErrorCode::ParseError);
    assert!(errors.first().message.contains("unknown field 'extra'"));
}

#[test]
fn r2_empty_nodes_or_entry_points_are_rejected() {
    let mut graph = minimal_graph();
    graph["nodes"] = json!([]);
    expect_code(graph, DslErrorCode::ParseError);

    let mut graph = minimal_graph();
    graph["entry_points"] = json!([]);
    expect_code(graph, DslErrorCode::ParseError);
}

#[test]
fn r2_minimal_graph_is_valid() {
    let loaded = load(minimal_graph()).expect("minimal graph valid");
    assert_eq!(loaded.definition.node_count(), 1);
}

// ---------------------------------------------------------------- R3

#[test]
fn r3_version_must_be_exactly_1_0() {
    let mut graph = minimal_graph();
    graph["version"] = json!("1.1");
    let errors = expect_code(graph, DslErrorCode::ParseError);
    assert!(errors.first().message.contains("unsupported DSL version"));
}

#[test]
fn r3_metadata_name_required_and_created_at_optional() {
    let mut graph = minimal_graph();
    graph["metadata"] = json!({});
    expect_code(graph, DslErrorCode::MissingField);

    let mut graph = minimal_graph();
    graph["metadata"] = json!({"name": "with-time", "created_at": "2026-10-29T10:00:00Z"});
    load(graph).expect("UTC RFC3339 created_at is valid");

    let mut graph = minimal_graph();
    graph["metadata"] = json!({"name": "bad-time", "created_at": "2026-10-29 10:00:00"});
    expect_code(graph, DslErrorCode::ParseError);
}

// ---------------------------------------------------------------- R4

#[test]
fn r4_end_requires_declared_required_input() {
    let graph = graph_with(
        json!([
            start_node("start"),
            node("end", "end", json!([]), json!({}))
        ]),
        json!([]),
    );
    expect_code(graph, DslErrorCode::InvalidPort);

    let graph = graph_with(
        json!([
            start_node("start"),
            node("end", "end", json!([input_port("r", false)]), json!({}))
        ]),
        json!([]),
    );
    expect_code(graph, DslErrorCode::InvalidPort);
}

#[test]
fn r4_on_error_is_an_unsupported_field() {
    let mut start = start_node("start");
    start["on_error"] = json!({"route": "fallback"});
    let graph = graph_with(json!([start]), json!([]));
    let errors = expect_code(graph, DslErrorCode::UnsupportedField);
    assert!(errors.first().message.contains("on_error"));
}

#[test]
fn r4_unknown_node_type_is_reported() {
    let graph = graph_with(
        json!([node("start", "human_node", json!([]), json!({}))]),
        json!([]),
    );
    let errors = expect_code(graph, DslErrorCode::UnsupportedNodeType);
    assert_eq!(errors.first().location.node_id.as_deref(), Some("start"));
}

#[test]
fn r4_node_id_charset_and_port_duplicates() {
    let graph = graph_with(
        json!([node("bad id", "start", json!([]), json!({}))]),
        json!([]),
    );
    expect_code(graph, DslErrorCode::ParseError);

    let graph = graph_with(
        json!([
            start_node("start"),
            python_node("n1", json!([input_port("x", true), input_port("x", true)]))
        ]),
        json!([]),
    );
    expect_code(graph, DslErrorCode::InvalidPort);
}

#[test]
fn r4_config_shape_rules() {
    // agent_call 缺 agent_id
    let graph = graph_with(
        json!([
            start_node("start"),
            node("a", "agent_call", json!([input_port("x", true)]), json!({}))
        ]),
        json!([edge("start", "a", "x")]),
    );
    expect_code(graph, DslErrorCode::MissingField);

    // timeout_ms 必须 >= 1
    let graph = graph_with(
        json!([
            start_node("start"),
            node(
                "a",
                "python_node",
                json!([input_port("x", true)]),
                json!({"module": "m", "function": "f", "timeout_ms": 0})
            )
        ]),
        json!([edge("start", "a", "x")]),
    );
    expect_code(graph, DslErrorCode::ParseError);

    // exec_class 枚举
    let graph = graph_with(
        json!([
            start_node("start"),
            node(
                "a",
                "python_node",
                json!([input_port("x", true)]),
                json!({"module": "m", "function": "f", "exec_class": "gpu"})
            )
        ]),
        json!([edge("start", "a", "x")]),
    );
    expect_code(graph, DslErrorCode::ParseError);

    // start 的 trigger 只允许 all
    let mut start = start_node("start");
    start["trigger"] = json!("any");
    expect_code(
        graph_with(json!([start]), json!([])),
        DslErrorCode::ParseError,
    );
}

#[test]
fn r4_start_may_declare_inputs_and_multiple_starts_are_allowed() {
    let start_in = node("start", "start", json!([input_port("x", true)]), json!({}));
    let graph = json!({
        "version": "1.0",
        "metadata": {"name": "starts"},
        "entry_points": ["start"],
        "nodes": [start_in],
        "edges": []
    });
    load(graph).expect("start may declare run-input contract");

    let graph = json!({
        "version": "1.0",
        "metadata": {"name": "two-starts"},
        "entry_points": ["s1", "s2"],
        "nodes": [start_node("s1"), start_node("s2")],
        "edges": []
    });
    load(graph).expect("multiple start nodes allowed");
}

// ---------------------------------------------------------------- R5

#[test]
fn r5_duplicate_node_id() {
    let graph = graph_with(json!([start_node("start"), start_node("start")]), json!([]));
    let errors = expect_code(graph, DslErrorCode::DuplicateNodeId);
    assert_eq!(errors.first().location.node_id.as_deref(), Some("start"));
}

#[test]
fn r5_distinct_node_ids_pass() {
    let graph = graph_with(
        json!([
            start_node("start"),
            python_node("n1", json!([input_port("x", true)]))
        ]),
        json!([edge("start", "n1", "x")]),
    );
    load(graph).expect("distinct ids");
}

// ---------------------------------------------------------------- R6

#[test]
fn r6_valid_subgraph_definition_passes() {
    load_fixture("valid_subgraph.json").expect("valid subgraph fixture");
}

#[test]
fn r6_subgraph_without_end_is_invalid() {
    let errors = load_fixture("bad_subgraph_bad.json").expect_err("no end node");
    assert_eq!(errors.first().code, DslErrorCode::SubgraphInvalid);
}

#[test]
fn r6_subgraph_reference_must_exist() {
    let mut graph = minimal_graph();
    graph["nodes"] = json!([
        start_node("start"),
        node(
            "sub",
            "subgraph",
            json!([input_port("q", true)]),
            json!({"subgraph_id": "missing"})
        )
    ]);
    graph["edges"] = json!([edge("start", "sub", "q")]);
    let errors = expect_code(graph, DslErrorCode::SubgraphInvalid);
    assert!(errors.first().message.contains("missing"));
}

#[test]
fn r6_output_mapping_must_reference_end_port() {
    let mut graph = minimal_graph();
    graph["subgraphs"] = json!([{
        "id": "inner",
        "nodes": [
            python_node("pin", json!([input_port("p", true)])),
            end_node("inner_end", "o")
        ],
        "edges": [edge("pin", "inner_end", "o")],
        "input_mapping": {"q": "pin.p"},
        "output_mapping": {"r": "pin.p"}
    }]);
    expect_code(graph, DslErrorCode::SubgraphInvalid);
}

// ---------------------------------------------------------------- R7

#[test]
fn r7_entry_point_must_exist_and_not_be_end() {
    let mut graph = minimal_graph();
    graph["entry_points"] = json!(["ghost"]);
    let errors = expect_code(graph, DslErrorCode::InvalidEntry);
    assert!(errors.first().message.contains("unknown_node"));

    let graph = graph_with(
        json!([start_node("start"), end_node("end", "r")]),
        json!([]),
    );
    let mut graph = graph;
    graph["entry_points"] = json!(["start", "end"]);
    let errors = expect_code(graph, DslErrorCode::InvalidEntry);
    assert!(errors.first().message.contains("end_node"));
}

#[test]
fn r7_every_start_must_be_listed_in_entry_points() {
    let graph = graph_with(json!([start_node("start"), start_node("other")]), json!([]));
    let errors = expect_code(graph, DslErrorCode::InvalidEntry);
    assert!(errors.first().message.contains("start_not_listed"));
}

// ---------------------------------------------------------------- R8

#[test]
fn r8_edge_endpoint_rules() {
    // 未知 from
    let graph = graph_with(
        json!([
            start_node("start"),
            python_node("n1", json!([input_port("x", true)]))
        ]),
        json!([edge("ghost", "n1", "x")]),
    );
    expect_code(graph, DslErrorCode::InvalidEdgeRef);

    // end 作源
    let graph = graph_with(
        json!([
            start_node("start"),
            python_node("n1", json!([input_port("x", true)])),
            end_node("end", "r")
        ]),
        json!([edge("start", "n1", "x"), edge("end", "n1", "x")]),
    );
    let errors = expect_code(graph, DslErrorCode::InvalidEdgeRef);
    assert!(errors.first().message.contains("end_as_source"));

    // entry 作目标（同时命中 start_as_target 与 entry_as_target，断言后者存在）
    let graph = graph_with(
        json!([
            start_node("start"),
            python_node("n1", json!([input_port("x", true)]))
        ]),
        json!([edge("start", "n1", "x"), edge("n1", "start", "x")]),
    );
    let errors = expect_code(graph, DslErrorCode::InvalidEdgeRef);
    assert!(errors
        .errors
        .iter()
        .any(|error| error.message.contains("entry_as_target")));
}

#[test]
fn r8_legal_edge_passes() {
    let graph = graph_with(
        json!([
            start_node("start"),
            python_node("n1", json!([input_port("x", true)]))
        ]),
        json!([edge("start", "n1", "x")]),
    );
    load(graph).expect("legal edge");
}

// ---------------------------------------------------------------- R9

#[test]
fn r9_undeclared_input_port() {
    let graph = graph_with(
        json!([
            start_node("start"),
            python_node("n1", json!([input_port("x", true)]))
        ]),
        json!([edge("start", "n1", "zzz")]),
    );
    let errors = expect_code(graph, DslErrorCode::InvalidPort);
    assert_eq!(errors.first().location.edge_index, Some(0));
    assert_eq!(errors.first().location.port.as_deref(), Some("zzz"));
}

#[test]
fn r9_from_output_port_is_reserved() {
    let graph = graph_with(
        json!([
            start_node("start"),
            python_node("n1", json!([input_port("x", true)]))
        ]),
        json!([{
            "from": "start",
            "to_node": "n1",
            "to_input_port": "x",
            "from_output_port": "out"
        }]),
    );
    let errors = expect_code(graph, DslErrorCode::UnsupportedField);
    assert_eq!(errors.first().location.edge_index, Some(0));
}

#[test]
fn r9_gate_params_must_be_object() {
    let graph = graph_with(
        json!([
            start_node("start"),
            python_node("n1", json!([input_port("x", true)]))
        ]),
        json!([gated_edge(
            "start",
            "n1",
            "x",
            json!({"function": "manual", "params": "nope"})
        )]),
    );
    expect_code(graph, DslErrorCode::ParseError);
}

// ---------------------------------------------------------------- R10 / R11

#[test]
fn r10_duplicate_from_to_pair() {
    let graph = graph_with(
        json!([
            start_node("start"),
            python_node("n1", json!([input_port("x", true), input_port("y", true)]))
        ]),
        json!([edge("start", "n1", "x"), edge("start", "n1", "y")]),
    );
    let errors = expect_code(graph, DslErrorCode::DuplicateEdge);
    assert_eq!(errors.first().location.edge_index, Some(1));
}

#[test]
fn r11_duplicate_port_source_is_forbidden() {
    let graph = graph_with(
        json!([
            start_node("start"),
            python_node("n1", json!([input_port("x", true)])),
            python_node("m1", json!([input_port("y", true)]))
        ]),
        json!([edge("start", "n1", "x"), edge("m1", "n1", "x")]),
    );
    let errors = expect_code(graph, DslErrorCode::DuplicatePortSource);
    assert_eq!(errors.first().location.edge_index, Some(1));
    assert_eq!(errors.first().location.port.as_deref(), Some("x"));
}

#[test]
fn r10_r11_distinct_pairs_and_ports_pass() {
    let graph = graph_with(
        json!([
            start_node("start"),
            python_node("a", json!([input_port("x", true)])),
            python_node("b", json!([input_port("x", true)])),
            end_node("end", "r")
        ]),
        json!([
            edge("start", "a", "x"),
            edge("start", "b", "x"),
            edge("a", "end", "r")
        ]),
    );
    load(graph).expect("two sources into different nodes are legal");
}

// ---------------------------------------------------------------- R12

#[test]
fn r12_required_input_without_source() {
    let graph = graph_with(
        json!([
            start_node("start"),
            python_node("n1", json!([input_port("x", true), input_port("y", true)]))
        ]),
        json!([edge("start", "n1", "x")]),
    );
    let errors = expect_code(graph, DslErrorCode::UnconnectedInput);
    assert_eq!(errors.first().location.port.as_deref(), Some("y"));
}

#[test]
fn r12_any_trigger_needs_at_least_one_required_source() {
    let mut n1 = python_node("n1", json!([input_port("x", true), input_port("y", true)]));
    n1["trigger"] = json!("any");
    let graph = graph_with(
        json!([start_node("start"), n1]),
        json!([edge("start", "n1", "x")]),
    );
    load(graph).expect("any trigger: one required source is enough");

    let mut n1 = python_node("n1", json!([input_port("x", true), input_port("y", true)]));
    n1["trigger"] = json!("any");
    let graph = graph_with(json!([start_node("start"), n1]), json!([]));
    expect_code(graph, DslErrorCode::UnconnectedInput);
}

#[test]
fn r12_entry_nodes_are_exempt() {
    let graph = graph_with(
        json!([
            node("start", "start", json!([input_port("x", true)]), json!({})),
            python_node("n1", json!([input_port("x", true)]))
        ]),
        json!([edge("start", "n1", "x")]),
    );
    load(graph).expect("entry inputs are injected via RunSpec, not edges");
}

// ---------------------------------------------------------------- R13

#[test]
fn r13_ungated_cycle_is_detected() {
    let graph = graph_with(
        json!([
            start_node("start"),
            python_node("a", json!([input_port("x", true), input_port("z", true)])),
            python_node("b", json!([input_port("y", true)]))
        ]),
        json!([
            edge("start", "a", "x"),
            edge("a", "b", "y"),
            edge("b", "a", "z")
        ]),
    );
    let errors = expect_code(graph, DslErrorCode::CycleDetected);
    assert!(errors.first().message.contains("ungated data cycle"));
}

#[test]
fn r13_ungated_self_loop_is_detected_but_gated_self_loop_is_legal() {
    let graph = graph_with(
        json!([
            start_node("start"),
            python_node(
                "loop",
                json!([input_port("seed", true), input_port("again", true)])
            )
        ]),
        json!([edge("start", "loop", "seed"), edge("loop", "loop", "again")]),
    );
    expect_code(graph, DslErrorCode::CycleDetected);

    let graph = graph_with(
        json!([
            start_node("start"),
            python_node(
                "loop",
                json!([input_port("seed", true), input_port("again", true)])
            )
        ]),
        json!([
            edge("start", "loop", "seed"),
            gated_edge(
                "loop",
                "loop",
                "again",
                json!({"function": "check_output", "params": {"path": "more", "equals": true}})
            )
        ]),
    );
    load(graph).expect("gated self-loop is legal");
}

// ---------------------------------------------------------------- R14

#[test]
fn r14_gate_op_must_be_a_builtin_op() {
    let graph = graph_with(
        json!([
            start_node("start"),
            python_node("n1", json!([input_port("x", true)]))
        ]),
        json!([gated_edge(
            "start",
            "n1",
            "x",
            json!({"function": "gates.hr.need_human"})
        )]),
    );
    let errors = expect_code(graph, DslErrorCode::SymbolNotFound);
    assert_eq!(errors.first().location.edge_index, Some(0));
}

#[test]
fn r14_all_builtin_gate_ops_and_params_are_accepted() {
    let graph = graph_with(
        json!([
            start_node("start"),
            python_node("a", json!([input_port("x", true)])),
            python_node("b", json!([input_port("x", true)])),
            python_node("c", json!([input_port("x", true)]))
        ]),
        json!([
            gated_edge(
                "start",
                "a",
                "x",
                json!({"function": "check_output", "params": {"path": "ok", "equals": true}})
            ),
            gated_edge(
                "start",
                "b",
                "x",
                json!({"function": "compare", "params": {"path": "score", "op": "gte", "value": 3}})
            ),
            gated_edge(
                "start",
                "c",
                "x",
                json!({"function": "manual", "params": {"prompt": "approve?"}})
            )
        ]),
    );
    load(graph).expect("all builtin ops with valid params");
}

#[test]
fn r14_gate_param_contract_mismatch() {
    let graph = graph_with(
        json!([
            start_node("start"),
            python_node("n1", json!([input_port("x", true)]))
        ]),
        json!([gated_edge(
            "start",
            "n1",
            "x",
            json!({"function": "compare", "params": {"path": "score", "value": 1}})
        )]),
    );
    let errors = expect_code(graph, DslErrorCode::SignatureMismatch);
    assert_eq!(errors.first().location.edge_index, Some(0));
}

#[test]
fn r14_python_symbol_format_is_checked_without_import() {
    let mut bad_module = python_node("n1", json!([input_port("x", true)]));
    bad_module["config"] = json!({"module": "9bad", "function": "f"});
    let graph = graph_with(
        json!([start_node("start"), bad_module]),
        json!([edge("start", "n1", "x")]),
    );
    let errors = expect_code(graph, DslErrorCode::SymbolNotFound);
    assert_eq!(errors.first().location.node_id.as_deref(), Some("n1"));

    let mut bad_function = python_node("n1", json!([input_port("x", true)]));
    bad_function["config"] = json!({"module": "m", "function": "a.b"});
    let graph = graph_with(
        json!([start_node("start"), bad_function]),
        json!([edge("start", "n1", "x")]),
    );
    expect_code(graph, DslErrorCode::SymbolNotFound);
}

#[test]
fn r14_repeated_python_symbol_is_idempotent() {
    let graph = graph_with(
        json!([
            start_node("start"),
            python_node("n1", json!([input_port("x", true)])),
            python_node("n2", json!([input_port("x", true)]))
        ]),
        json!([edge("start", "n1", "x"), edge("start", "n2", "x")]),
    );
    // n1/n2 的符号均为 "m.n1" / "m.n2"，不同名；再加一个同名节点验证幂等。
    let mut graph = graph;
    graph["nodes"].as_array_mut().expect("nodes").push(json!({
        "id": "n3",
        "type": "python_node",
        "inputs": [input_port("x", true)],
        "config": {"module": "m", "function": "n1"}
    }));
    graph["edges"]
        .as_array_mut()
        .expect("edges")
        .push(edge("start", "n3", "x"));
    load(graph).expect("same symbol, same kind is idempotent");
}

#[test]
fn r14_symbol_conflict_between_kinds() {
    let errors = load_fixture("bad_symbol_conflict.json").expect_err("conflict fixture");
    assert_eq!(errors.first().code, DslErrorCode::SymbolConflict);
}

#[test]
fn r14_agent_assets_are_checked_only_with_assets_dir() {
    // 无 assets-dir：跳过 R14c（validate 可独立工作）。
    load_fixture("bad_agent_not_found.json").expect("R14c skipped without --assets-dir");

    let errors = load_fixture_with_assets("bad_agent_not_found.json").expect_err("missing agent");
    assert_eq!(errors.first().code, DslErrorCode::AgentNotFound);
    assert!(errors.first().message.contains("ghost_agent"));

    load_fixture_with_assets("customer_ticket.json").expect("all agents present");
}

// ---------------------------------------------------------------- 聚合 / fail-fast / 序列化

#[test]
fn aggregation_within_rule_and_fail_fast_across_rules() {
    // 同一规则内两个错误都上报。
    let graph = graph_with(
        json!([
            start_node("start"),
            node("e1", "end", json!([]), json!({})),
            node("e2", "end", json!([]), json!({}))
        ]),
        json!([]),
    );
    let errors = expect_error(graph);
    assert_eq!(errors.errors.len(), 2, "both end nodes reported: {errors}");

    // 跨规则 fail-fast：R4 错误存在时不再报 R5（重复 id）。
    let graph = graph_with(
        json!([
            start_node("start"),
            start_node("start"),
            node("e1", "end", json!([]), json!({}))
        ]),
        json!([]),
    );
    let errors = expect_error(graph);
    assert!(!errors.contains(DslErrorCode::DuplicateNodeId));
    assert!(errors.contains(DslErrorCode::InvalidPort));
}

#[test]
fn error_count_is_capped_and_truncated() {
    let mut nodes = vec![start_node("start")];
    for index in 0..25 {
        nodes.push(node(&format!("e{index}"), "end", json!([]), json!({})));
    }
    let errors = expect_error(graph_with(json!(nodes), json!([])));
    assert!(errors.truncated);
    assert_eq!(errors.error_count, 25);
    assert_eq!(errors.errors.len(), zgraph_dsl::MAX_DSL_ERRORS);
}

#[test]
fn error_object_serializes_stable_shape() {
    let errors = expect_error(graph_with(
        json!([
            start_node("start"),
            python_node("n1", json!([input_port("x", true)]))
        ]),
        json!([edge("start", "n1", "zzz")]),
    ));
    let value = serde_json::to_value(errors.first()).expect("serialize");
    assert_eq!(value["code"], json!("DSL_INVALID_PORT"));
    assert!(value["message"].is_string());
    assert_eq!(value["location"]["edge_index"], json!(0));
    assert_eq!(value["location"]["port"], json!("zzz"));
    assert!(value["expected"].is_array());
    assert_eq!(value["actual"], json!("zzz"));
}

// ---------------------------------------------------------------- REV-005 回归

fn with_root(mut graph: Value, key: &str, value: Value) -> Value {
    graph
        .as_object_mut()
        .expect("object")
        .insert(key.to_string(), value);
    graph
}

#[test]
fn rev005_on_error_only_node_level_is_unsupported_field() {
    // 节点级：保留字段。
    let mut start = start_node("start");
    start["on_error"] = json!({"route": "fallback"});
    let errors = expect_code(
        graph_with(json!([start]), json!([])),
        DslErrorCode::UnsupportedField,
    );
    assert_eq!(errors.first().location.node_id.as_deref(), Some("start"));

    // 顶层：未知字段（文档级）。
    let graph = with_root(minimal_graph(), "on_error", json!({"route": "fallback"}));
    let errors = expect_code(graph, DslErrorCode::ParseError);
    assert!(errors.first().location.is_empty());
    assert!(errors.first().message.contains("on_error"));

    // 节点 config：未知字段（节点级）。
    let mut agent = node(
        "a",
        "agent_call",
        json!([input_port("x", true)]),
        json!({"agent_id": "message_parser", "on_error": {}}),
    );
    agent["on_error"] = Value::Null;
    if let Value::Object(map) = &mut agent {
        map.remove("on_error");
    }
    let graph = graph_with(
        json!([start_node("start"), agent]),
        json!([edge("start", "a", "x")]),
    );
    let errors = expect_code(graph, DslErrorCode::ParseError);
    assert_eq!(errors.first().location.node_id.as_deref(), Some("a"));

    // 边：未知字段（边级）。
    let graph = graph_with(
        json!([
            start_node("start"),
            python_node("n1", json!([input_port("x", true)]))
        ]),
        json!([{
            "from": "start",
            "to_node": "n1",
            "to_input_port": "x",
            "on_error": {"route": "fallback"}
        }]),
    );
    let errors = expect_code(graph, DslErrorCode::ParseError);
    assert_eq!(errors.first().location.edge_index, Some(0));

    // 输入端口：未知字段（节点级）。
    let graph = graph_with(
        json!([
            start_node("start"),
            python_node(
                "n1",
                json!([{"name": "x", "type": "string", "required": true, "on_error": true}])
            )
        ]),
        json!([edge("start", "n1", "x")]),
    );
    let errors = expect_code(graph, DslErrorCode::ParseError);
    assert_eq!(errors.first().location.node_id.as_deref(), Some("n1"));

    // gate：未知字段（边级）。
    let graph = graph_with(
        json!([
            start_node("start"),
            python_node("n1", json!([input_port("x", true)]))
        ]),
        json!([gated_edge(
            "start",
            "n1",
            "x",
            json!({"function": "manual", "on_error": {"route": "fallback"}})
        )]),
    );
    let errors = expect_code(graph, DslErrorCode::ParseError);
    assert_eq!(errors.first().location.edge_index, Some(0));
}

#[test]
fn rev005_from_output_port_is_unsupported_regardless_of_value() {
    for raw in [json!("out"), json!(123), json!(null), json!({"p": 1})] {
        let graph = graph_with(
            json!([
                start_node("start"),
                python_node("n1", json!([input_port("x", true)]))
            ]),
            json!([{
                "from": "start",
                "to_node": "n1",
                "to_input_port": "x",
                "from_output_port": raw
            }]),
        );
        let errors = expect_code(graph, DslErrorCode::UnsupportedField);
        assert_eq!(errors.first().location.edge_index, Some(0));
        assert!(errors.first().location.port.is_none());
        assert_eq!(errors.first().actual, Some(raw));
    }
}

#[test]
fn rev005_subgraph_id_must_be_non_empty_string() {
    let subgraph_body = |id: Value| {
        json!([{
            "id": id,
            "nodes": [
                python_node("pin", json!([input_port("p", true)])),
                end_node("inner_end", "o")
            ],
            "edges": [edge("pin", "inner_end", "o")],
            "input_mapping": {"q": "pin.p"},
            "output_mapping": {"r": "inner_end.o"}
        }])
    };

    let mut graph = minimal_graph();
    graph["subgraphs"] = subgraph_body(json!(""));
    let errors = expect_code(graph, DslErrorCode::MissingField);
    assert!(errors.first().location.is_empty());
    assert!(errors.first().message.contains("subgraphs[0]"));

    let mut graph = minimal_graph();
    graph["subgraphs"] = subgraph_body(json!(7));
    let errors = expect_code(graph, DslErrorCode::ParseError);
    assert!(errors.first().location.is_empty());
    assert!(errors.first().message.contains("subgraphs[0]"));

    let mut graph = minimal_graph();
    graph["subgraphs"] = json!([{
        "nodes": [],
        "edges": [],
        "input_mapping": {},
        "output_mapping": {}
    }]);
    let errors = expect_code(graph, DslErrorCode::MissingField);
    assert!(errors.first().message.contains("subgraphs[0]"));
}

#[test]
fn rev005_location_contract_for_node_and_edge_errors() {
    // 节点未知字段 → node_id。
    let mut start = start_node("start");
    start["bogus"] = json!(true);
    let errors = expect_code(
        graph_with(json!([start]), json!([])),
        DslErrorCode::ParseError,
    );
    assert_eq!(errors.first().location.node_id.as_deref(), Some("start"));
    assert!(errors.first().location.edge_index.is_none());

    // 边未知字段 → edge_index。
    let graph = graph_with(
        json!([
            start_node("start"),
            python_node("n1", json!([input_port("x", true)]))
        ]),
        json!([{
            "from": "start",
            "to_node": "n1",
            "to_input_port": "x",
            "bogus": 1
        }]),
    );
    let errors = expect_code(graph, DslErrorCode::ParseError);
    assert_eq!(errors.first().location.edge_index, Some(0));
    assert!(errors.first().location.node_id.is_none());

    // 序列化后 location 只含契约键。
    let value = serde_json::to_value(errors.first()).expect("serialize");
    let keys: std::collections::BTreeSet<&str> = value["location"]
        .as_object()
        .expect("location")
        .keys()
        .map(String::as_str)
        .collect();
    let allowed: std::collections::BTreeSet<&str> =
        ["node_id", "edge_index", "port"].into_iter().collect();
    assert!(keys.is_subset(&allowed));
}

#[test]
fn rev005_missing_edge_field_is_reported_once() {
    let graph = graph_with(
        json!([
            start_node("start"),
            python_node("n1", json!([input_port("x", true)]))
        ]),
        json!([{"to_node": "n1", "to_input_port": "x"}]),
    );
    let errors = expect_error(graph);
    assert_eq!(errors.error_count, 1, "{errors:?}");
    assert_eq!(errors.first().code, DslErrorCode::MissingField);
    assert_eq!(errors.first().location.edge_index, Some(0));
    assert!(errors.first().message.contains("'from'"));
}

#[test]
fn rev005_created_at_requires_valid_utc_rfc3339() {
    let with_created_at = |value: Value| {
        let mut graph = minimal_graph();
        graph["metadata"] = json!({"name": "timestamp", "created_at": value});
        graph
    };
    for invalid in [
        "2026-99-99T99:99:99Z",
        "2026-13-01T00:00:00Z",
        "2026-00-10T00:00:00Z",
        "2026-02-30T00:00:00Z",
        "2027-02-29T00:00:00Z",
        "2026-10-32T00:00:00Z",
        "2026-10-29T24:00:00Z",
        "2026-10-29T23:60:00Z",
        "2026-10-29T23:59:61Z",
        "2026-10-29T10:00:00+08:00",
        "2026-10-29T10:00:00+00:00",
        "2026-10-29T10:00:00",
    ] {
        let errors = expect_code(with_created_at(json!(invalid)), DslErrorCode::ParseError);
        assert!(
            errors.first().message.contains("created_at"),
            "{invalid}: {errors}"
        );
        assert!(errors.first().location.is_empty());
    }
    for valid in [
        "2026-10-29T10:00:00Z",
        "2028-02-29T00:00:00Z",
        "2026-10-29T10:00:00.123Z",
    ] {
        load(with_created_at(json!(valid))).unwrap_or_else(|errors| panic!("{valid}: {errors}"));
    }
}
