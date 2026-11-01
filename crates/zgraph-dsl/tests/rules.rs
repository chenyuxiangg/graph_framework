//! R1–R16 的规则级测试：每条规则至少一个正例与一个反例（R15 为 W2a、R16 为 W2b 新增）。
//!
//! 通过公开 API（`load_graph_value` / `load_graph_with_options`）驱动，
//! 断言错误码、定位与 expected/actual；fixtures 在 `fixtures.rs` 中逐文件覆盖。

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use serde_json::{json, Value};
use zgraph_dsl::{
    load_graph, load_graph_text, load_graph_value, load_graph_with_options, DslErrorCode,
    DslErrors, LoadOptions, LoadedGraph, OnGateFalse, OnItemError,
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
        assert_eq!(
            errors.first().actual,
            Some(json!({"field": "from_output_port", "value": raw}))
        );
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
        "2026-10-29T10:00:00-00:00",
        "2026-10-29T10:00:00.123+08:00",
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
        "2026-10-29T10:00:00+00:00",
        "2026-10-29T10:00:00.123+00:00",
    ] {
        load(with_created_at(json!(valid))).unwrap_or_else(|errors| panic!("{valid}: {errors}"));
    }
}

// ---------------------------------------------------------------- REV-005 复审

fn subgraph_body(extra_key: Option<(&str, Value)>) -> Value {
    let mut subgraph = json!({
        "id": "inner",
        "nodes": [
            python_node("pin", json!([input_port("p", true)])),
            end_node("inner_end", "o")
        ],
        "edges": [edge("pin", "inner_end", "o")],
        "input_mapping": {"q": "pin.p"},
        "output_mapping": {"r": "inner_end.o"}
    });
    if let Some((key, value)) = extra_key {
        subgraph[key] = value;
    }
    subgraph
}

fn graph_referencing_inner(subgraph: Value) -> Value {
    let mut graph = graph_with(
        json!([
            start_node("start"),
            node(
                "sub",
                "subgraph",
                json!([input_port("q", true)]),
                json!({"subgraph_id": "inner"})
            )
        ]),
        json!([edge("start", "sub", "q")]),
    );
    graph["subgraphs"] = json!([subgraph]);
    graph
}

#[test]
fn rev005_2_valid_subgraph_header_errors_locate_subgraph_id_without_cascade() {
    for extra in [
        ("bogus", json!(true)),
        ("on_error", json!({"route": "fallback"})),
    ] {
        let graph = graph_referencing_inner(subgraph_body(Some(extra)));
        let errors = expect_error(graph);
        assert_eq!(errors.error_count, 1, "{errors}");
        assert_eq!(errors.first().code, DslErrorCode::ParseError);
        assert_eq!(
            errors.first().location.node_id.as_deref(),
            Some("inner"),
            "{errors}"
        );
        assert!(
            !errors.first().message.contains("not defined"),
            "must not cascade undefined-reference error: {errors}"
        );
    }
}

#[test]
fn rev005_2_node_on_error_actual_contains_field_and_value() {
    let mut start = start_node("start");
    start["on_error"] = json!({"route": "fallback"});
    let errors = expect_code(
        graph_with(json!([start]), json!([])),
        DslErrorCode::UnsupportedField,
    );
    assert_eq!(errors.first().location.node_id.as_deref(), Some("start"));
    assert_eq!(errors.first().expected, Some(json!("<absent>")));
    assert_eq!(
        errors.first().actual,
        Some(json!({"field": "on_error", "value": {"route": "fallback"}}))
    );
}

fn temp_assets_dir(tag: &str, files: &[(&str, &str)]) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("zgraph-dsl-test-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("agents")).expect("create temp agents dir");
    for (name, content) in files {
        std::fs::write(dir.join("agents").join(name), content).expect("write asset file");
    }
    dir
}

fn load_with_assets(graph: Value, assets_dir: &Path) -> Result<LoadedGraph, DslErrors> {
    load_graph_value(graph, &LoadOptions::new().with_assets_dir(assets_dir))
}

fn agent_graph(agent_id: &str) -> Value {
    graph_with(
        json!([
            start_node("start"),
            node(
                "a1",
                "agent_call",
                json!([input_port("x", true)]),
                json!({"agent_id": agent_id})
            )
        ]),
        json!([edge("start", "a1", "x")]),
    )
}

#[test]
fn rev005_2_asset_index_errors_are_document_level_with_index_path() {
    let duplicate_dir = temp_assets_dir(
        "dup",
        &[("a.json", r#"{"id":"dup"}"#), ("b.json", r#"{"id":"dup"}"#)],
    );
    let errors = load_with_assets(agent_graph("dup"), &duplicate_dir).expect_err("duplicate index");
    assert_eq!(errors.first().code, DslErrorCode::AgentNotFound);
    assert!(errors.first().location.is_empty(), "{errors}");
    assert!(
        errors.first().message.contains("assets.agents[1]"),
        "{errors}"
    );

    let invalid_dir = temp_assets_dir("invalid", &[("broken.json", "not-json")]);
    let errors = load_with_assets(minimal_graph(), &invalid_dir).expect_err("invalid asset");
    assert_eq!(errors.first().code, DslErrorCode::AgentNotFound);
    assert!(errors.first().location.is_empty(), "{errors}");
    assert!(
        errors.first().message.contains("assets.agents[0]"),
        "{errors}"
    );
}

#[test]
fn rev005_2_missing_agent_reports_first_referencing_node_once() {
    let graph = graph_with(
        json!([
            start_node("start"),
            node(
                "first",
                "agent_call",
                json!([input_port("x", true)]),
                json!({"agent_id": "ghost_agent"})
            ),
            node(
                "second",
                "agent_call",
                json!([input_port("x", true)]),
                json!({"agent_id": "ghost_agent"})
            )
        ]),
        json!([edge("start", "first", "x"), edge("start", "second", "x")]),
    );
    let assets = repo_root().join("tests/fixtures/assets");
    let errors = load_with_assets(graph, &assets).expect_err("missing agent");
    assert_eq!(errors.error_count, 1, "{errors}");
    assert_eq!(errors.first().code, DslErrorCode::AgentNotFound);
    assert_eq!(
        errors.first().location.node_id.as_deref(),
        Some("first"),
        "{errors}"
    );
}

// ---------------------------------------------------------------- R15 / on_gate_false（W2a）

/// R15 冻结 `expected`（DR-005 §2.5.2；02 §2.4.2）。
const R15_EXPECTED: &str =
    "re-entrant defer source (cycle through source containing a defer edge, or defer self-loop)";

/// 内置 `check_output` gate（R14 合法 params）。
fn review_gate(path: &str, equals: Value) -> Value {
    json!({"function": "check_output", "params": {"path": path, "equals": equals}})
}

/// 带 gate 的 `on_gate_false="defer"` 边。
fn defer_edge(from: &str, to_node: &str, to_input_port: &str, gate: Value) -> Value {
    let mut edge = gated_edge(from, to_node, to_input_port, gate);
    edge["on_gate_false"] = json!("defer");
    edge
}

/// 单节点 gated 自环图（无 defer，R15 不得误报）。
fn gated_self_loop_graph() -> Value {
    graph_with(
        json!([
            start_node("start"),
            python_node(
                "loop",
                json!([input_port("seed", true), input_port("again", true)])
            )
        ]),
        json!([
            edge("start", "loop", "seed"),
            gated_edge("loop", "loop", "again", review_gate("more", json!(true)))
        ]),
    )
}

/// SOP 返工图：`P --rework(defer)--> W -> R -> P` 环 +前进边 `P --approved(defer)--> end`。
fn defer_rework_graph() -> Value {
    graph_with(
        json!([
            start_node("start"),
            python_node(
                "p",
                json!([input_port("task", true), input_port("feedback", true)])
            ),
            python_node("w", json!([input_port("comments", true)])),
            python_node("r", json!([input_port("work", true)])),
            end_node("done", "result")
        ]),
        json!([
            edge("start", "p", "task"),
            defer_edge(
                "p",
                "done",
                "result",
                review_gate("verdict", json!("approved"))
            ),
            defer_edge(
                "p",
                "w",
                "comments",
                review_gate("verdict", json!("rework"))
            ),
            edge("w", "r", "work"),
            edge("r", "p", "feedback")
        ]),
    )
}

#[test]
fn on_gate_false_helpers_are_stable() {
    assert_eq!(OnGateFalse::default(), OnGateFalse::Settle);
    assert_eq!(OnGateFalse::ALL.len(), 2);
    for policy in OnGateFalse::ALL {
        assert_eq!(OnGateFalse::from_name(policy.as_str()), Some(policy));
    }
    assert_eq!(OnGateFalse::from_name("ignore"), None);
}

#[test]
fn on_gate_false_is_none_when_absent_and_kept_when_explicit() {
    let loaded = load(gated_self_loop_graph()).expect("gated self-loop valid");
    assert_eq!(loaded.definition.edges[1].on_gate_false, None);

    let mut graph = gated_self_loop_graph();
    graph["edges"][1]["on_gate_false"] = json!("settle");
    let loaded = load(graph).expect("explicit settle valid");
    assert_eq!(
        loaded.definition.edges[1].on_gate_false,
        Some(OnGateFalse::Settle)
    );
}

#[test]
fn on_gate_false_unknown_value_is_parse_error_at_edge() {
    let mut graph = gated_self_loop_graph();
    graph["edges"][1]["on_gate_false"] = json!("ignore");
    let errors = expect_code(graph, DslErrorCode::ParseError);
    assert_eq!(errors.error_count, 1, "{errors}");
    assert_eq!(errors.first().location.edge_index, Some(1));
    assert_eq!(errors.first().location.node_id, None);
    assert!(errors.first().message.contains("on_gate_false"), "{errors}");
    assert_eq!(
        errors.first().expected,
        Some(json!(["settle", "defer"])),
        "{errors}"
    );
    assert_eq!(errors.first().actual, Some(json!("ignore")));
}

#[test]
fn on_gate_false_non_string_types_are_parse_errors() {
    // N3（REV-069）：所有非字符串 JSON 类型都落 R4 类型桶（`expected:"string"`），
    // 不得被误当成枚举值或静默忽略。
    for value in [
        json!(null),
        json!(1),
        json!(1.5),
        json!(true),
        json!({"x": 1}),
        json!([]),
    ] {
        let mut graph = gated_self_loop_graph();
        graph["edges"][1]["on_gate_false"] = value.clone();
        let errors = expect_code(graph, DslErrorCode::ParseError);
        assert_eq!(errors.error_count, 1, "value {value}: {errors}");
        assert_eq!(
            errors.first().location.edge_index,
            Some(1),
            "value {value}: {errors}"
        );
        assert_eq!(errors.first().location.node_id, None, "value {value}");
        assert!(
            errors.first().message.contains("expects string"),
            "value {value}: {errors}"
        );
        assert_eq!(
            errors.first().expected,
            Some(json!("string")),
            "value {value}"
        );
        assert_eq!(errors.first().actual, Some(value.clone()), "value {value}");
    }
}

#[test]
fn r15_defer_edges_on_defer_cycle_are_legal() {
    let loaded = load(defer_rework_graph()).expect("SOP rework graph is valid");
    let policies: Vec<Option<OnGateFalse>> = loaded
        .definition
        .edges
        .iter()
        .map(|edge| edge.on_gate_false)
        .collect();
    assert_eq!(
        policies,
        vec![
            None,
            Some(OnGateFalse::Defer),
            Some(OnGateFalse::Defer),
            None,
            None
        ]
    );
}

#[test]
fn r15_defer_source_is_authorized_by_any_defer_edge_in_its_scc() {
    // 环 p -> w -> r -> p 内的 defer 边是 w -> r；源 p 虽不是该 defer 边的端点，
    // 但 p 位于「含 defer 边的环」上（DR-004 D1 措辞）→ p -> done 的 defer 合法。
    let graph = graph_with(
        json!([
            start_node("start"),
            python_node(
                "p",
                json!([input_port("task", true), input_port("feedback", true)])
            ),
            python_node("w", json!([input_port("comments", true)])),
            python_node("r", json!([input_port("work", true)])),
            end_node("done", "result")
        ]),
        json!([
            edge("start", "p", "task"),
            gated_edge("p", "w", "comments", review_gate("go", json!(true))),
            defer_edge("w", "r", "work", review_gate("ok", json!(true))),
            edge("r", "p", "feedback"),
            defer_edge("p", "done", "result", review_gate("ok", json!(true)))
        ]),
    );
    load(graph).expect("source on a defer-bearing cycle is re-entrant");
}

#[test]
fn r15_defer_edge_source_without_cycle_is_rejected() {
    let graph = graph_with(
        json!([
            start_node("start"),
            python_node("a", json!([input_port("x", true)])),
            end_node("done", "result")
        ]),
        json!([
            edge("start", "a", "x"),
            defer_edge("a", "done", "result", review_gate("ok", json!(true)))
        ]),
    );
    let errors = expect_code(graph, DslErrorCode::DeferNotReentrant);
    assert_eq!(errors.error_count, 1, "{errors}");
    let first = errors.first();
    assert_eq!(first.location.edge_index, Some(1));
    assert_eq!(first.location.node_id.as_deref(), Some("a"));
    assert_eq!(
        first.message,
        "defer edge source 'a' is not re-entrant: must lie on a cycle containing a defer edge"
    );
    // N1/N2（REV-069）：锁定 DR-005 §2.5.2 冻结载荷（源不在任何环上 → size 1、无 defer 边）。
    assert_eq!(first.expected, Some(json!(R15_EXPECTED)), "{first:?}");
    assert_eq!(
        first.actual,
        Some(json!({"node_id": "a", "scc_size": 1, "defer_edges": []})),
        "{first:?}"
    );
}

#[test]
fn r15_gated_cycle_without_defer_edge_does_not_authorize_defer() {
    // SCC {a,b} 大小 2，但环内没有 defer 边（gate 边为 settle）→ defer a→done 非法。
    let graph = graph_with(
        json!([
            start_node("start"),
            python_node("a", json!([input_port("x", true), input_port("z", true)])),
            python_node("b", json!([input_port("y", true)])),
            end_node("done", "result")
        ]),
        json!([
            edge("start", "a", "x"),
            gated_edge("a", "b", "y", review_gate("go", json!(true))),
            edge("b", "a", "z"),
            defer_edge("a", "done", "result", review_gate("ok", json!(true)))
        ]),
    );
    let errors = expect_code(graph, DslErrorCode::DeferNotReentrant);
    assert_eq!(errors.error_count, 1, "{errors}");
    let first = errors.first();
    assert_eq!(first.location.edge_index, Some(3));
    assert_eq!(first.location.node_id.as_deref(), Some("a"));
    assert_eq!(first.expected, Some(json!(R15_EXPECTED)), "{first:?}");
    assert_eq!(
        first.actual,
        Some(json!({"node_id": "a", "scc_size": 2, "defer_edges": []})),
        "{first:?}"
    );
}

#[test]
fn r15_defer_self_loop_is_reentrant() {
    let graph = graph_with(
        json!([
            start_node("start"),
            python_node(
                "loop",
                json!([input_port("seed", true), input_port("again", true)])
            ),
            end_node("done", "result")
        ]),
        json!([
            edge("start", "loop", "seed"),
            defer_edge("loop", "loop", "again", review_gate("more", json!(true))),
            // 源 loop 有 defer 自环 → 该前进 defer 边同样合法。
            defer_edge("loop", "done", "result", review_gate("ok", json!(true)))
        ]),
    );
    let loaded = load(graph).expect("defer self-loop is re-entrant");
    assert_eq!(
        loaded.definition.edges[1].on_gate_false,
        Some(OnGateFalse::Defer)
    );
    assert_eq!(
        loaded.definition.edges[2].on_gate_false,
        Some(OnGateFalse::Defer)
    );
}

#[test]
fn r15_first_violating_defer_edge_in_array_order_is_reported() {
    // 两条互不相关的违规 defer 边 → 只报 edges 数组序的首个（index 1）。
    let graph = graph_with(
        json!([
            start_node("start"),
            python_node("a", json!([input_port("x", true)])),
            python_node("b", json!([input_port("y", true)])),
            end_node("done", "result"),
            end_node("done2", "other")
        ]),
        json!([
            edge("start", "a", "x"),
            defer_edge("a", "done", "result", review_gate("ok", json!(true))),
            edge("start", "b", "y"),
            defer_edge("b", "done2", "other", review_gate("ok", json!(true)))
        ]),
    );
    let errors = expect_code(graph, DslErrorCode::DeferNotReentrant);
    assert_eq!(errors.error_count, 1, "{errors}");
    let first = errors.first();
    assert_eq!(first.location.edge_index, Some(1));
    assert_eq!(first.location.node_id.as_deref(), Some("a"));
    assert_eq!(first.expected, Some(json!(R15_EXPECTED)), "{first:?}");
    assert_eq!(
        first.actual,
        Some(json!({"node_id": "a", "scc_size": 1, "defer_edges": []})),
        "{first:?}"
    );
}

#[test]
fn r15_r13_still_reports_ungated_cycle_first() {
    // R13 保持原样：无 gate 环先报 DSL_CYCLE_DETECTED（R15 不改变其语义）。
    let graph = graph_with(
        json!([
            start_node("start"),
            python_node("a", json!([input_port("x", true), input_port("z", true)])),
            python_node("b", json!([input_port("y", true)])),
            end_node("done", "result")
        ]),
        json!([
            edge("start", "a", "x"),
            edge("a", "b", "y"),
            edge("b", "a", "z"),
            defer_edge("a", "done", "result", review_gate("ok", json!(true)))
        ]),
    );
    expect_code(graph, DslErrorCode::CycleDetected);
}

#[test]
fn r15_violation_inside_subgraph_maps_to_subgraph_invalid() {
    let mut graph = graph_with(
        json!([
            start_node("start"),
            node(
                "sub",
                "subgraph",
                json!([input_port("q", true)]),
                json!({"subgraph_id": "inner"})
            ),
            end_node("done", "r")
        ]),
        json!([edge("start", "sub", "q"), edge("sub", "done", "r")]),
    );
    graph["subgraphs"] = json!([{
        "id": "inner",
        "nodes": [
            python_node("pin", json!([input_port("p", true)])),
            end_node("inner_end", "o")
        ],
        "edges": [defer_edge(
            "pin",
            "inner_end",
            "o",
            review_gate("ok", json!(true))
        )],
        "input_mapping": {"q": "pin.p"},
        "output_mapping": {"r": "inner_end.o"}
    }]);
    let errors = expect_code(graph, DslErrorCode::SubgraphInvalid);
    assert_eq!(errors.error_count, 1, "{errors}");
    assert!(
        errors.first().message.contains("subgraph 'inner'"),
        "{errors}"
    );
    assert!(
        errors.first().message.contains("not re-entrant"),
        "{errors}"
    );
    assert_eq!(errors.first().location.node_id.as_deref(), Some("pin"));
    assert_eq!(errors.first().location.edge_index, Some(0));
    // 子图收敛只换 code/message 前缀，冻结载荷原样保留。
    assert_eq!(
        errors.first().expected,
        Some(json!(R15_EXPECTED)),
        "{errors}"
    );
    assert_eq!(
        errors.first().actual,
        Some(json!({"node_id": "pin", "scc_size": 1, "defer_edges": []})),
        "{errors}"
    );
}

#[test]
fn r15_non_defer_graphs_have_no_false_positives() {
    for name in [
        "customer_ticket.json",
        "valid_parallel.json",
        "valid_cycle.json",
        "valid_subgraph.json",
    ] {
        load_fixture(name).unwrap_or_else(|errors| panic!("{name}: {errors}"));
    }
    load(gated_self_loop_graph()).expect("gated cycle without defer is unaffected");
    load_fixture("valid_gate_defer.json").expect("explicit defer rework fixture is valid");
}

// ---------------------------------------------------------------- R16（W2b）

/// 给节点加 `max_fanout` 键（R16 测试图构造用）。
fn with_max_fanout(mut node: Value, limit: Value) -> Value {
    node["max_fanout"] = limit;
    node
}

/// `start` 出边 2 条（→ a / → b）的合法图；`max_fanout` 可缺省或指定。
fn fanout_graph(max_fanout: Option<Value>) -> Value {
    let start = match max_fanout {
        Some(limit) => with_max_fanout(start_node("start"), limit),
        None => start_node("start"),
    };
    graph_with(
        json!([
            start,
            python_node("a", json!([input_port("x", true)])),
            python_node("b", json!([input_port("x", true)])),
            json!({
                "id": "done",
                "type": "end",
                "inputs": [
                    {"name": "ra", "type": "any", "required": true},
                    {"name": "rb", "type": "any", "required": true}
                ],
                "config": {}
            })
        ]),
        json!([
            edge("start", "a", "x"),
            edge("start", "b", "x"),
            edge("a", "done", "ra"),
            edge("b", "done", "rb")
        ]),
    )
}

/// `start` 与 `p` 各自超限的图（nodes 顺序 = [start, p, b, c, done]）。
///
/// `start` 出度 2、`p` 出度 3，二者 `max_fanout` 均为 1；用于锁定 R16 首错
/// 按 `nodes` 数组序取第一条。
fn double_fanout_graph() -> Value {
    graph_with(
        json!([
            with_max_fanout(start_node("start"), json!(1)),
            with_max_fanout(python_node("p", json!([input_port("x", true)])), json!(1)),
            python_node("b", json!([input_port("x", true), input_port("y", true)])),
            python_node("c", json!([input_port("z", true)])),
            end_node("done", "r")
        ]),
        json!([
            edge("start", "p", "x"),
            edge("start", "b", "x"),
            edge("p", "b", "y"),
            edge("p", "c", "z"),
            edge("p", "done", "r")
        ]),
    )
}

#[test]
fn r16_max_fanout_is_optional_and_parsed() {
    let loaded = load(fanout_graph(Some(json!(2)))).expect("out-degree == max_fanout is legal");
    let start = loaded
        .definition
        .nodes
        .iter()
        .find(|node| node.id == "start")
        .expect("start node");
    assert_eq!(start.max_fanout, Some(2));
    // 未写 max_fanout 的节点保持 None（缺省无上限，v0.1.0 行为）。
    let missing = loaded
        .definition
        .nodes
        .iter()
        .find(|node| node.id == "a")
        .expect("a node");
    assert_eq!(missing.max_fanout, None);

    let plain = load(fanout_graph(None)).expect("missing max_fanout means no limit");
    let start = plain
        .definition
        .nodes
        .iter()
        .find(|node| node.id == "start")
        .expect("start node");
    assert_eq!(start.max_fanout, None);
}

#[test]
fn r16_max_fanout_none_is_not_serialized_as_null() {
    let loaded = load(fanout_graph(None)).expect("valid graph");
    let start = loaded
        .definition
        .nodes
        .iter()
        .find(|node| node.id == "start")
        .expect("start node");
    let value = serde_json::to_value(start).expect("serialize node");
    assert!(
        value.get("max_fanout").is_none(),
        "None must be skipped by serde: {value}"
    );

    let loaded = load(fanout_graph(Some(json!(2)))).expect("valid graph");
    let start = loaded
        .definition
        .nodes
        .iter()
        .find(|node| node.id == "start")
        .expect("start node");
    let value = serde_json::to_value(start).expect("serialize node");
    assert_eq!(value.get("max_fanout"), Some(&json!(2)), "{value}");
}

#[test]
fn r16_max_fanout_values_must_be_integers_at_least_one() {
    let invalid_values = [
        json!(0),
        Value::from(-1_i64),
        json!(1.5),
        json!("2"),
        Value::Null,
        json!(true),
        json!(4_294_967_296_u64),
    ];
    for value in invalid_values {
        let errors = expect_code(fanout_graph(Some(value.clone())), DslErrorCode::ParseError);
        assert_eq!(errors.error_count, 1, "value {value}: {errors}");
        let first = errors.first();
        assert_eq!(
            first.message,
            format!("'max_fanout' must be >= 1, got {value}"),
            "value {value}: {errors}"
        );
        assert_eq!(
            first.location.node_id.as_deref(),
            Some("start"),
            "value {value}: {errors}"
        );
        assert_eq!(first.location.edge_index, None, "value {value}: {errors}");
        assert_eq!(first.expected, Some(json!(">= 1")), "value {value}");
        assert_eq!(first.actual, Some(value.clone()), "value {value}");
    }
}

#[test]
fn r16_out_degree_exceeding_max_fanout_is_parse_error() {
    let errors = expect_code(fanout_graph(Some(json!(1))), DslErrorCode::ParseError);
    assert_eq!(errors.error_count, 1, "{errors}");
    let first = errors.first();
    assert_eq!(
        first.message,
        "node 'start' out-degree 2 exceeds max_fanout 1"
    );
    assert_eq!(first.location.node_id.as_deref(), Some("start"));
    assert_eq!(first.location.edge_index, None);
    assert_eq!(first.location.port, None);
    assert_eq!(first.expected, Some(json!("out-degree <= 1")), "{errors}");
    assert_eq!(first.actual, Some(json!("out-degree 2")), "{errors}");
}

#[test]
fn r16_gated_edges_count_toward_out_degree() {
    // 出度统计覆盖全部边（含 gate 边）：1 条普通边 + 1 条 gate 边 = 2。
    let mut graph = fanout_graph(Some(json!(2)));
    graph["edges"][1] = gated_edge("start", "b", "x", review_gate("ok", json!(true)));
    load(graph.clone()).expect("2 out-edges (one gated) within max_fanout 2");

    graph["nodes"][0]["max_fanout"] = json!(1);
    let errors = expect_code(graph, DslErrorCode::ParseError);
    assert_eq!(
        errors.first().message,
        "node 'start' out-degree 2 exceeds max_fanout 1"
    );
}

#[test]
fn r16_first_violating_node_in_nodes_order_is_reported() {
    let mut graph = double_fanout_graph();
    let errors = expect_code(graph.clone(), DslErrorCode::ParseError);
    assert_eq!(errors.error_count, 1, "{errors}");
    assert_eq!(errors.first().location.node_id.as_deref(), Some("start"));
    assert_eq!(
        errors.first().message,
        "node 'start' out-degree 2 exceeds max_fanout 1"
    );

    // 把 `p` 提到 nodes[0]：首错随之变为 `p`（不聚合，取 nodes 数组序首错）。
    let p = graph["nodes"]
        .as_array_mut()
        .expect("nodes array")
        .remove(1);
    graph["nodes"]
        .as_array_mut()
        .expect("nodes array")
        .insert(0, p);
    let errors = expect_code(graph, DslErrorCode::ParseError);
    assert_eq!(errors.error_count, 1, "{errors}");
    assert_eq!(errors.first().location.node_id.as_deref(), Some("p"));
    assert_eq!(
        errors.first().message,
        "node 'p' out-degree 3 exceeds max_fanout 1"
    );
}

#[test]
fn r16_violation_inside_subgraph_maps_to_subgraph_invalid() {
    let mut graph = graph_with(
        json!([
            start_node("start"),
            node(
                "sub",
                "subgraph",
                json!([input_port("q", true)]),
                json!({"subgraph_id": "inner"})
            ),
            end_node("done", "r")
        ]),
        json!([edge("start", "sub", "q"), edge("sub", "done", "r")]),
    );
    graph["subgraphs"] = json!([{
        "id": "inner",
        "nodes": [
            with_max_fanout(
                python_node("pin", json!([input_port("p", true)])),
                json!(1)
            ),
            json!({
                "id": "inner_end1",
                "type": "end",
                "inputs": [{"name": "o1", "type": "any", "required": true}],
                "config": {}
            }),
            json!({
                "id": "inner_end2",
                "type": "end",
                "inputs": [{"name": "o2", "type": "any", "required": true}],
                "config": {}
            })
        ],
        "edges": [
            edge("pin", "inner_end1", "o1"),
            edge("pin", "inner_end2", "o2")
        ],
        "input_mapping": {"q": "pin.p"},
        "output_mapping": {"r": "inner_end1.o1"}
    }]);
    let errors = expect_code(graph, DslErrorCode::SubgraphInvalid);
    assert_eq!(errors.error_count, 1, "{errors}");
    let first = errors.first();
    // R6 口径：code/message 前缀收敛，冻结载荷与内部定位原样保留。
    assert_eq!(
        first.message,
        "subgraph 'inner': node 'pin' out-degree 2 exceeds max_fanout 1"
    );
    assert_eq!(first.location.node_id.as_deref(), Some("pin"));
    assert_eq!(first.location.edge_index, None);
    assert_eq!(first.expected, Some(json!("out-degree <= 1")), "{errors}");
    assert_eq!(first.actual, Some(json!("out-degree 2")), "{errors}");
}

#[test]
fn r16_runs_after_r15() {
    let graph = graph_with(
        json!([
            start_node("start"),
            with_max_fanout(python_node("p", json!([input_port("x", true)])), json!(1)),
            python_node("q", json!([input_port("z", true)])),
            end_node("done", "r")
        ]),
        json!([
            edge("start", "p", "x"),
            edge("p", "q", "z"),
            defer_edge("p", "done", "r", review_gate("ok", json!(true)))
        ]),
    );
    // `p` 出度 2 > max_fanout 1，同时 defer 边源不可重入 → R15 先报。
    let errors = expect_code(graph, DslErrorCode::DeferNotReentrant);
    assert_eq!(errors.first().location.node_id.as_deref(), Some("p"));
}

#[test]
fn r16_runs_before_r14() {
    let mut graph = fanout_graph(Some(json!(1)));
    graph["edges"][0] = gated_edge(
        "start",
        "a",
        "x",
        json!({"function": "gates.hr.need_human"}),
    );
    // 未知 gate op（R14）与扇出超限（R16）同时存在 → R16 先报 DSL_PARSE_ERROR。
    let errors = expect_code(graph, DslErrorCode::ParseError);
    assert_eq!(
        errors.first().message,
        "node 'start' out-degree 2 exceeds max_fanout 1"
    );
}

#[test]
fn r16_max_fanout_is_allowed_on_every_node_type() {
    // 白名单覆盖全部 5 种节点类型（start/end/python_node/agent_call/subgraph）。
    let mut graph = graph_with(
        json!([
            with_max_fanout(start_node("start"), json!(2)),
            with_max_fanout(python_node("py", json!([input_port("x", true)])), json!(1)),
            with_max_fanout(
                node(
                    "ag",
                    "agent_call",
                    json!([input_port("y", true)]),
                    json!({"agent_id": "agent-1"})
                ),
                json!(1)
            ),
            with_max_fanout(
                node(
                    "sub",
                    "subgraph",
                    json!([input_port("q", true)]),
                    json!({"subgraph_id": "inner"})
                ),
                json!(1)
            ),
            json!({
                "id": "done",
                "type": "end",
                "inputs": [
                    {"name": "ra", "type": "any", "required": true},
                    {"name": "rb", "type": "any", "required": true},
                    {"name": "rc", "type": "any", "required": true}
                ],
                "config": {},
                "max_fanout": 3
            })
        ]),
        json!([
            edge("start", "py", "x"),
            edge("start", "ag", "y"),
            edge("py", "done", "ra"),
            edge("ag", "done", "rb"),
            edge("sub", "done", "rc")
        ]),
    );
    graph["entry_points"] = json!(["start", "sub"]);
    graph["subgraphs"] = json!([{
        "id": "inner",
        "nodes": [
            python_node("pin", json!([input_port("p", true)])),
            end_node("inner_end", "o")
        ],
        "edges": [edge("pin", "inner_end", "o")],
        "input_mapping": {"q": "pin.p"},
        "output_mapping": {"r": "inner_end.o"}
    }]);
    let loaded = load(graph).expect("max_fanout is allowed on every node type");
    let limits: Vec<(&str, Option<u32>)> = loaded
        .definition
        .nodes
        .iter()
        .map(|node| (node.id.as_str(), node.max_fanout))
        .collect();
    assert_eq!(
        limits,
        vec![
            ("start", Some(2)),
            ("py", Some(1)),
            ("ag", Some(1)),
            ("sub", Some(1)),
            ("done", Some(3)),
        ]
    );
    let inner = &loaded.definition.subgraphs[0].nodes;
    assert_eq!(
        inner[0].max_fanout, None,
        "subgraph node without max_fanout"
    );
}

// ---------------------------------------------------------------- R17/R18（W4a）

/// 给节点加 `foreach`（R17/R18 测试图构造用）。
fn with_foreach(mut node: Value, spec: Value) -> Value {
    node["foreach"] = spec;
    node
}

/// 合法 `foreach` 形状（含可选字段；region = {F, W, E}，exit = E，join = J）。
fn valid_foreach_spec() -> Value {
    json!({
        "items_port": "items",
        "key_field": "id",
        "join": "J",
        "max_items": 3,
        "on_item_error": "fail"
    })
}

/// `start -> F(foreach) -> W -> E -> J -> end` 的合法 foreach 图（P-1）。
fn foreach_graph(spec: Value) -> Value {
    graph_with(
        json!([
            start_node("start"),
            with_foreach(python_node("F", json!([input_port("items", true)])), spec),
            python_node("W", json!([input_port("task", true)])),
            python_node("E", json!([input_port("work", true)])),
            python_node("J", json!([input_port("results", true)])),
            end_node("end", "result")
        ]),
        json!([
            edge("start", "F", "items"),
            edge("F", "W", "task"),
            edge("W", "E", "work"),
            edge("E", "J", "results"),
            edge("J", "end", "result")
        ]),
    )
}

/// 断言 R18 首错载荷（W4a 冻结模板）。
fn expect_foreach_reason(graph: Value, f_node: &str, reason: &str) {
    let errors = expect_code(graph, DslErrorCode::ForeachInvalid);
    assert_eq!(errors.error_count, 1, "{errors}");
    let first = errors.first();
    assert_eq!(
        first.message,
        format!("foreach region of node '{f_node}' is invalid: {reason}")
    );
    assert_eq!(first.location.node_id.as_deref(), Some(f_node));
    assert_eq!(first.location.edge_index, None);
    assert_eq!(
        first.expected,
        Some(json!("valid foreach region")),
        "{errors}"
    );
    assert_eq!(
        first.actual,
        Some(json!({"node_id": f_node, "reason": reason})),
        "{errors}"
    );
}

#[test]
fn r17_foreach_is_parsed_and_optional() {
    let loaded = load_fixture("valid_foreach.json").expect("valid foreach fixture");
    let fanout = loaded
        .definition
        .nodes
        .iter()
        .find(|node| node.id == "F")
        .expect("F node");
    let spec = fanout.foreach.as_ref().expect("foreach parsed");
    assert_eq!(spec.items_port, "items");
    assert_eq!(spec.key_field.as_deref(), Some("id"));
    assert_eq!(spec.join, "J");
    assert_eq!(spec.max_items, Some(3));
    assert_eq!(spec.on_item_error, Some(OnItemError::Fail));
    assert!(loaded
        .definition
        .nodes
        .iter()
        .filter(|node| node.id != "F")
        .all(|node| node.foreach.is_none()));

    // 作者未写 → None；serde 不输出 null（"是否写了"可判定）。
    let mut graph = foreach_graph(valid_foreach_spec());
    graph["nodes"][1]
        .as_object_mut()
        .expect("node object")
        .remove("foreach");
    let loaded = load(graph).expect("without foreach the node is a plain node");
    let fanout = loaded
        .definition
        .nodes
        .iter()
        .find(|node| node.id == "F")
        .expect("F node");
    assert_eq!(fanout.foreach, None);
    let value = serde_json::to_value(fanout).expect("serialize node");
    assert!(
        value.get("foreach").is_none(),
        "None must be skipped by serde: {value}"
    );

    let loaded = load(foreach_graph(valid_foreach_spec())).expect("valid graph");
    let fanout = loaded
        .definition
        .nodes
        .iter()
        .find(|node| node.id == "F")
        .expect("F node");
    let value = serde_json::to_value(fanout).expect("serialize node");
    assert_eq!(value.get("foreach"), Some(&valid_foreach_spec()), "{value}");
}

#[test]
fn r17_foreach_is_allowed_only_on_executable_node_types() {
    // python_node / agent_call（含 builtin.human）允许。
    load(foreach_graph(valid_foreach_spec())).expect("python_node allows foreach");
    let mut agent = foreach_graph(valid_foreach_spec());
    agent["nodes"][1]["type"] = json!("agent_call");
    agent["nodes"][1]["config"] = json!({"agent_id": "agent-1"});
    load(agent).expect("agent_call allows foreach");
    let mut human = foreach_graph(valid_foreach_spec());
    human["nodes"][1]["config"] = json!({"module": "builtin", "function": "human"});
    load(human).expect("builtin.human is a python_node and allows foreach");

    // start / end / subgraph：R17 DSL_PARSE_ERROR（R4 阶段先于 R5/R6/R7）。
    for (id, node_type, config) in [
        ("S", "start", json!({})),
        ("done_end", "end", json!({})),
        ("sub", "subgraph", json!({"subgraph_id": "inner"})),
    ] {
        let graph = graph_with(
            json!([with_foreach(
                node(id, node_type, json!([input_port("items", true)]), config),
                json!({"items_port": "items", "join": "done"})
            )]),
            json!([]),
        );
        let errors = expect_code(graph, DslErrorCode::ParseError);
        assert_eq!(errors.error_count, 1, "{errors}");
        assert_eq!(
            errors.first().message,
            format!("node '{id}' of type '{node_type}' does not support foreach"),
            "{errors}"
        );
        assert_eq!(
            errors.first().expected,
            Some(json!("python_node|agent_call"))
        );
        assert_eq!(errors.first().actual, Some(json!(node_type)));
        assert_eq!(errors.first().location.node_id.as_deref(), Some(id));
    }
}

#[test]
fn r17_field_value_boundaries_are_validated() {
    let cases: [(Value, &str, Value, Value); 8] = [
        (
            json!({"items_port": "missing", "join": "J"}),
            "'foreach.items_port' of node 'F' must be a declared input port, got \"missing\"",
            json!(["items"]),
            json!("missing"),
        ),
        (
            json!({"items_port": 1, "join": "J"}),
            "'foreach.items_port' of node 'F' must be a declared input port, got 1",
            json!(["items"]),
            json!(1),
        ),
        (
            json!({"items_port": "items", "join": "J", "key_field": ""}),
            "'foreach.key_field' must be a non-empty string, got \"\"",
            json!("non-empty string"),
            json!(""),
        ),
        (
            json!({"items_port": "items", "join": "J", "key_field": 7}),
            "'foreach.key_field' must be a non-empty string, got 7",
            json!("non-empty string"),
            json!(7),
        ),
        (
            json!({"items_port": "items", "join": ""}),
            "'foreach.join' must be a node id, got \"\"",
            json!("node id"),
            json!(""),
        ),
        (
            json!({"items_port": "items", "join": "J", "max_items": 0}),
            "'foreach.max_items' must be an integer >= 1, got 0",
            json!(">= 1"),
            json!(0),
        ),
        (
            json!({"items_port": "items", "join": "J", "max_items": 1.5}),
            "'foreach.max_items' must be an integer >= 1, got 1.5",
            json!(">= 1"),
            json!(1.5),
        ),
        (
            json!({"items_port": "items", "join": "J", "on_item_error": "skip"}),
            "'foreach.on_item_error' must be one of ['fail'], got \"skip\"",
            json!(["fail"]),
            json!("skip"),
        ),
    ];
    for (foreach, message, expected, actual) in cases {
        let errors = expect_code(foreach_graph(foreach), DslErrorCode::ParseError);
        assert_eq!(errors.error_count, 1, "{errors}");
        let first = errors.first();
        assert_eq!(first.message, message, "{errors}");
        assert_eq!(first.expected, Some(expected.clone()), "{errors}");
        assert_eq!(first.actual, Some(actual.clone()), "{errors}");
        assert_eq!(first.location.node_id.as_deref(), Some("F"));
    }

    // on_item_error 非字符串 → R17。
    let errors = expect_code(
        foreach_graph(json!({"items_port": "items", "join": "J", "on_item_error": 1})),
        DslErrorCode::ParseError,
    );
    assert_eq!(
        errors.first().message,
        "'foreach.on_item_error' must be one of ['fail'], got 1"
    );
}

#[test]
fn r17_foreach_aggregates_field_errors_in_frozen_order() {
    // items_port 未声明 + on_item_error 非法 → 同节点聚合，按表顺序 items_port 先报。
    let errors = expect_code(
        foreach_graph(json!({"items_port": "missing", "join": "J", "on_item_error": "skip"})),
        DslErrorCode::ParseError,
    );
    assert_eq!(errors.error_count, 2, "{errors}");
    assert!(
        errors.first().message.starts_with("'foreach.items_port'"),
        "{errors}"
    );
    assert!(
        errors.errors[1]
            .message
            .starts_with("'foreach.on_item_error'"),
        "{errors}"
    );

    // 未知键聚合 + 缺必填 items_port/join。未知键之间的先后取决于 `serde_json`
    // 的 Map 实现（`preserve_order` feature 由工作区统一开启，单 crate 测试关闭），
    // 因此只强断言集合与「未知键先于缺必填」的阶段顺序。
    let errors = expect_code(
        foreach_graph(json!({"mode": "x", "extra": 1})),
        DslErrorCode::ParseError,
    );
    assert_eq!(errors.error_count, 4, "{errors}");
    let messages: BTreeSet<&str> = errors.errors.iter().map(|e| e.message.as_str()).collect();
    assert_eq!(
        messages,
        BTreeSet::from([
            "unknown field 'extra' in foreach of node 'F'",
            "unknown field 'mode' in foreach of node 'F'",
            "foreach of node 'F' missing required field 'items_port'",
            "foreach of node 'F' missing required field 'join'",
        ])
    );
    assert!(
        errors.errors[..2]
            .iter()
            .all(|error| error.message.starts_with("unknown field")),
        "{errors}"
    );
    assert_eq!(
        errors.first().expected,
        Some(json!([
            "items_port",
            "key_field",
            "join",
            "max_items",
            "on_item_error"
        ]))
    );

    // 非 object → 单错（`{actual_type}` 模板）。
    let errors = expect_code(foreach_graph(json!(["items"])), DslErrorCode::ParseError);
    assert_eq!(errors.error_count, 1, "{errors}");
    assert_eq!(
        errors.first().message,
        "field 'foreach' expects object, got array"
    );
    assert_eq!(errors.first().expected, Some(json!("object")));
    assert_eq!(errors.first().actual, Some(json!(["items"])));
}

#[test]
fn r18_valid_regions_pass() {
    // P-1：`start -> F -> W -> E -> J -> end`（region={F,W,E}）。
    load(foreach_graph(valid_foreach_spec())).expect("P-1 valid region");
    // P-2：单节点 region（E = F），F 直接投递 J。
    let graph = graph_with(
        json!([
            start_node("start"),
            with_foreach(
                python_node("F", json!([input_port("items", true)])),
                json!({"items_port": "items", "join": "J"})
            ),
            python_node("J", json!([input_port("results", true)])),
            end_node("end", "result")
        ]),
        json!([
            edge("start", "F", "items"),
            edge("F", "J", "results"),
            edge("J", "end", "result")
        ]),
    );
    load(graph).expect("P-2 single-node region");

    // P-3：串行 foreach（J 是另一个 foreach 的 F）。
    let graph = graph_with(
        json!([
            start_node("start"),
            with_foreach(
                python_node("F1", json!([input_port("items", true)])),
                json!({"items_port": "items", "join": "F2"})
            ),
            python_node("W1", json!([input_port("task", true)])),
            python_node("E1", json!([input_port("work", true)])),
            with_foreach(
                python_node("F2", json!([input_port("results", true)])),
                json!({"items_port": "results", "join": "J2"})
            ),
            python_node("E2", json!([input_port("done", true)])),
            python_node("J2", json!([input_port("out", true)])),
            end_node("end", "result")
        ]),
        json!([
            edge("start", "F1", "items"),
            edge("F1", "W1", "task"),
            edge("W1", "E1", "work"),
            edge("E1", "F2", "results"),
            edge("F2", "E2", "done"),
            edge("E2", "J2", "out"),
            edge("J2", "end", "result")
        ]),
    );
    load(graph).expect("P-3 serial foreach");

    // join 可以是 end（数组按普通端口值进入 end 聚合）。
    let graph = graph_with(
        json!([
            start_node("start"),
            with_foreach(
                python_node("F", json!([input_port("items", true)])),
                json!({"items_port": "items", "join": "done"})
            ),
            python_node("E", json!([input_port("work", true)])),
            end_node("done", "results")
        ]),
        json!([
            edge("start", "F", "items"),
            edge("F", "E", "work"),
            edge("E", "done", "results")
        ]),
    );
    load(graph).expect("join may be an end node");
}

#[test]
fn r18_reason_closed_set_is_reported_by_fixtures() {
    let cases = [
        ("bad_foreach_join_not_found.json", "join_not_found"),
        ("bad_foreach_join_in_region.json", "join_in_region"),
        ("bad_foreach_join_type.json", "join_type_unsupported"),
        (
            "bad_foreach_forbidden_node_type.json",
            "forbidden_node_type",
        ),
        ("bad_foreach_nested.json", "nested_foreach"),
        ("bad_foreach_no_exit.json", "no_exit"),
        ("bad_foreach_multiple_exits.json", "multiple_exits"),
        ("bad_foreach_external_edge.json", "external_edge"),
        ("bad_foreach_join_edge_gated.json", "join_edge_gated"),
        ("bad_foreach_items_port_missing.json", "items_port_missing"),
    ];
    for (fixture, reason) in cases {
        let errors = load_fixture(fixture).expect_err(fixture);
        assert_eq!(
            errors.first().code,
            DslErrorCode::ForeachInvalid,
            "{fixture}: {errors}"
        );
        assert_eq!(
            errors.first().message,
            format!("foreach region of node 'F' is invalid: {reason}"),
            "{fixture}: {errors}"
        );
        assert_eq!(
            errors.first().actual,
            Some(json!({"node_id": "F", "reason": reason})),
            "{fixture}: {errors}"
        );
    }
}

#[test]
fn r18_region_reasons_are_reported_inline() {
    // join 不存在 / join = F / join 类型不支持。
    let mut graph = foreach_graph(valid_foreach_spec());
    graph["nodes"][1]["foreach"]["join"] = json!("ghost");
    expect_foreach_reason(graph, "F", "join_not_found");

    let mut graph = foreach_graph(valid_foreach_spec());
    graph["nodes"][1]["foreach"]["join"] = json!("F");
    expect_foreach_reason(graph, "F", "join_in_region");

    let mut graph = foreach_graph(valid_foreach_spec());
    graph["nodes"][1]["foreach"]["join"] = json!("start");
    expect_foreach_reason(graph, "F", "join_type_unsupported");

    // region 内含 end（forbidden）/ 其它 foreach（nested）。
    let mut graph = foreach_graph(valid_foreach_spec());
    graph["nodes"].as_array_mut().expect("nodes").push(json!({
        "id": "side",
        "type": "end",
        "inputs": [{"name": "s", "type": "any", "required": true}],
        "config": {}
    }));
    graph["edges"].as_array_mut().expect("edges").push(json!({
        "from": "F", "to_node": "side", "to_input_port": "s"
    }));
    expect_foreach_reason(graph, "F", "forbidden_node_type");

    let mut graph = foreach_graph(valid_foreach_spec());
    graph["nodes"].as_array_mut().expect("nodes").push(json!({
        "id": "N",
        "type": "python_node",
        "inputs": [{"name": "y", "type": "any", "required": true}],
        "config": {"module": "m", "function": "nested"},
        "foreach": {"items_port": "y", "join": "J"}
    }));
    graph["edges"].as_array_mut().expect("edges").push(json!({
        "from": "F", "to_node": "N", "to_input_port": "y"
    }));
    expect_foreach_reason(graph, "F", "nested_foreach");

    // 无出口 / 多出口 / join 边带 gate / items_port 无来源。
    let mut graph = foreach_graph(valid_foreach_spec());
    graph["edges"] = json!([
        {"from": "start", "to_node": "F", "to_input_port": "items"},
        {"from": "F", "to_node": "W", "to_input_port": "task"},
        {"from": "W", "to_node": "E", "to_input_port": "work"},
        {"from": "start", "to_node": "J", "to_input_port": "results"},
        {"from": "J", "to_node": "end", "to_input_port": "result"}
    ]);
    expect_foreach_reason(graph, "F", "no_exit");

    let mut graph = foreach_graph(valid_foreach_spec());
    graph["nodes"][4]["inputs"]
        .as_array_mut()
        .expect("J inputs")
        .push(json!({"name": "results2", "type": "any", "required": true}));
    graph["edges"].as_array_mut().expect("edges").push(json!({
        "from": "W", "to_node": "J", "to_input_port": "results2"
    }));
    expect_foreach_reason(graph, "F", "multiple_exits");

    let mut graph = foreach_graph(valid_foreach_spec());
    graph["edges"][3]["gate"] = review_gate("ok", json!(true));
    expect_foreach_reason(graph, "F", "join_edge_gated");

    let mut graph = foreach_graph(valid_foreach_spec());
    graph["nodes"][1]["inputs"] = json!([
        {"name": "items", "type": "any", "required": false},
        {"name": "task_seed", "type": "any", "required": true}
    ]);
    graph["edges"][0]["to_input_port"] = json!("task_seed");
    expect_foreach_reason(graph, "F", "items_port_missing");
}

#[test]
fn r18_first_invalid_foreach_in_nodes_order_is_reported() {
    // 两个非法 foreach：F 在前 → 报 F 的 join_edge_gated；F2 移到最前 → 报 join_not_found。
    let mut graph = foreach_graph(valid_foreach_spec());
    graph["edges"][3]["gate"] = review_gate("ok", json!(true));
    graph["nodes"].as_array_mut().expect("nodes").push(json!({
        "id": "F2",
        "type": "python_node",
        "inputs": [{"name": "items2", "type": "any", "required": false}],
        "config": {"module": "m", "function": "f2"},
        "foreach": {"items_port": "items2", "join": "ghost"}
    }));
    graph["entry_points"] = json!(["start", "F2"]);
    let errors = expect_code(graph.clone(), DslErrorCode::ForeachInvalid);
    assert_eq!(errors.error_count, 1, "{errors}");
    assert_eq!(errors.first().location.node_id.as_deref(), Some("F"));
    assert_eq!(
        errors.first().actual,
        Some(json!({"node_id": "F", "reason": "join_edge_gated"}))
    );

    let f2 = graph["nodes"].as_array_mut().expect("nodes").remove(6);
    graph["nodes"].as_array_mut().expect("nodes").insert(0, f2);
    let errors = expect_code(graph, DslErrorCode::ForeachInvalid);
    assert_eq!(errors.first().location.node_id.as_deref(), Some("F2"));
    assert_eq!(
        errors.first().actual,
        Some(json!({"node_id": "F2", "reason": "join_not_found"}))
    );
}

#[test]
fn r18_region_algorithm_is_deterministic() {
    let graph = foreach_graph(valid_foreach_spec());
    let compact = serde_json::to_string(&graph).expect("compact");
    let pretty = serde_json::to_string_pretty(&graph).expect("pretty");
    let first = load_graph_text(&compact).expect("compact valid");
    let second = load_graph_text(&pretty).expect("pretty valid");
    assert_eq!(first.graph_hash, second.graph_hash);

    // 同一非法图重复加载 → 同一 reason / message（BFS 按 nodes+edges 序确定）。
    let mut invalid = graph;
    invalid["nodes"][1]["foreach"]["join"] = json!("start");
    let mut messages = BTreeSet::new();
    for _ in 0..5 {
        let errors = load(invalid.clone()).expect_err("invalid region");
        messages.insert(errors.first().message.clone());
    }
    assert_eq!(messages.len(), 1, "{messages:?}");
}

#[test]
fn r18_region_overlap_is_checked_after_boundary_rows() {
    // DR-007 §3.2 row 顺序下，「本应 overlap」的图会先被 row8（external_edge）拦截：
    // F2 从 region 外向 W 投递，使 F 的 region 边界不闭合。这里锁定该冻结优先级。
    let mut graph = foreach_graph(valid_foreach_spec());
    graph["nodes"].as_array_mut().expect("nodes").push(json!({
        "id": "F2",
        "type": "python_node",
        "inputs": [{"name": "seed", "type": "any", "required": true}],
        "config": {"module": "m", "function": "f2"},
        "foreach": {"items_port": "seed", "join": "J2"}
    }));
    graph["nodes"].as_array_mut().expect("nodes").push(json!({
        "id": "J2",
        "type": "python_node",
        "inputs": [{"name": "q", "type": "any", "required": true}],
        "config": {"module": "m", "function": "j2"}
    }));
    graph["nodes"][2]["inputs"]
        .as_array_mut()
        .expect("W inputs")
        .push(json!({"name": "extra", "type": "any", "required": true}));
    graph["nodes"][5]["inputs"]
        .as_array_mut()
        .expect("end inputs")
        .push(json!({"name": "result2", "type": "any", "required": true}));
    graph["entry_points"] = json!(["start", "F2", "J2"]);
    graph["edges"].as_array_mut().expect("edges").extend([
        json!({"from": "F2", "to_node": "W", "to_input_port": "extra"}),
        json!({"from": "J2", "to_node": "end", "to_input_port": "result2"}),
    ]);
    let errors = expect_code(graph, DslErrorCode::ForeachInvalid);
    assert_eq!(errors.first().location.node_id.as_deref(), Some("F"));
    assert_eq!(
        errors.first().actual,
        Some(json!({"node_id": "F", "reason": "external_edge"})),
        "{errors}"
    );
}

#[test]
fn r18_runs_after_r15() {
    // defer 边源不可重入（R15）与非法 region 并存 → R15 先报。
    let mut graph = foreach_graph(valid_foreach_spec());
    graph["nodes"][1]["foreach"]["join"] = json!("F");
    graph["edges"][3]["on_gate_false"] = json!("defer");
    graph["edges"][3]["gate"] = review_gate("ok", json!(true));
    let errors = expect_code(graph, DslErrorCode::DeferNotReentrant);
    assert_eq!(errors.first().location.node_id.as_deref(), Some("E"));
}

#[test]
fn r18_runs_before_r16_and_r14() {
    // R18（join_in_region）先于 R16（F 出度 2 > max_fanout=1）。
    let mut graph = foreach_graph(valid_foreach_spec());
    graph["nodes"].as_array_mut().expect("nodes").push(json!({
        "id": "W2",
        "type": "python_node",
        "inputs": [{"name": "task", "type": "any", "required": true}],
        "config": {"module": "m", "function": "w2"}
    }));
    graph["edges"].as_array_mut().expect("edges").push(json!({
        "from": "F", "to_node": "W2", "to_input_port": "task"
    }));
    graph["nodes"][1]["foreach"]["join"] = json!("F");
    graph["nodes"][1]["max_fanout"] = json!(1);
    expect_foreach_reason(graph, "F", "join_in_region");

    // R18 先于 R14：非法 region + 未知 gate op。
    let mut graph = foreach_graph(valid_foreach_spec());
    graph["nodes"][1]["foreach"]["join"] = json!("F");
    graph["edges"][0]["gate"] = json!({"function": "gates.hr.need_human"});
    expect_foreach_reason(graph, "F", "join_in_region");
}

#[test]
fn r18_subgraph_foreach_maps_to_subgraph_invalid() {
    let mut graph = graph_with(
        json!([
            start_node("start"),
            node(
                "sub",
                "subgraph",
                json!([input_port("q", true)]),
                json!({"subgraph_id": "inner"})
            ),
            end_node("done", "r")
        ]),
        json!([edge("start", "sub", "q"), edge("sub", "done", "r")]),
    );
    graph["subgraphs"] = json!([{
        "id": "inner",
        "nodes": [
            with_foreach(
                python_node("pin", json!([input_port("p", true)])),
                json!({"items_port": "p", "join": "pin"})
            ),
            end_node("inner_end", "o")
        ],
        "edges": [edge("pin", "inner_end", "o")],
        "input_mapping": {"q": "pin.p"},
        "output_mapping": {"r": "inner_end.o"}
    }]);
    let errors = expect_code(graph, DslErrorCode::SubgraphInvalid);
    assert_eq!(errors.error_count, 1, "{errors}");
    assert_eq!(
        errors.first().message,
        "subgraph 'inner': node 'pin' declares foreach, which is not supported inside subgraphs"
    );
    assert_eq!(errors.first().location.node_id.as_deref(), Some("pin"));
    assert_eq!(errors.first().location.edge_index, None);
}
