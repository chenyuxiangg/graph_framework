//! `graph_hash` 契约测试（02 §2.8）：稳定、canonical、内容敏感。

use std::path::PathBuf;

use serde_json::json;
use zgraph_dsl::{canonical_json, graph_hash, load_graph, load_graph_text};

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("repo root")
}

fn customer_ticket_bytes() -> Vec<u8> {
    std::fs::read(repo_root().join("tests/fixtures/graphs/customer_ticket.json")).expect("fixture")
}

fn graph_json(pretty: bool) -> String {
    let graph = json!({
        "version": "1.0",
        "metadata": {"name": "hash-case", "description": "d1", "author": "a1"},
        "entry_points": ["start"],
        "nodes": [
            {"id": "start", "type": "start", "inputs": [], "config": {}},
            {
                "id": "n1",
                "type": "python_node",
                "inputs": [{"name": "x", "type": "string", "required": true}],
                "config": {"module": "m", "function": "f", "timeout_ms": 1000}
            },
            {"id": "end", "type": "end", "inputs": [{"name": "r", "type": "any", "required": true}], "config": {}}
        ],
        "edges": [
            {"from": "start", "to_node": "n1", "to_input_port": "x"},
            {"from": "n1", "to_node": "end", "to_input_port": "r"}
        ]
    });
    if pretty {
        serde_json::to_string_pretty(&graph).expect("pretty json")
    } else {
        serde_json::to_string(&graph).expect("compact json")
    }
}

#[test]
fn hash_is_stable_across_formatting_and_key_order() {
    let pretty = load_graph_text(&graph_json(true)).expect("pretty valid");
    let compact = load_graph_text(&graph_json(false)).expect("compact valid");
    assert_eq!(pretty.graph_hash, compact.graph_hash);
}

#[test]
fn hash_ignores_non_semantic_metadata() {
    let base = graph_json(false);
    let mut changed = graph_json(false).replace("\"description\":\"d1\"", "\"description\":\"d2\"");
    changed = changed.replace("\"author\":\"a1\"", "\"author\":\"a2\"");
    let base_loaded = load_graph_text(&base).expect("base");
    let changed_loaded = load_graph_text(&changed).expect("changed metadata");
    assert_eq!(base_loaded.graph_hash, changed_loaded.graph_hash);
}

#[test]
fn hash_changes_when_node_content_changes() {
    let base = load_graph_text(&graph_json(false)).expect("base");
    let changed = graph_json(false).replace("\"timeout_ms\":1000", "\"timeout_ms\":2000");
    let changed_loaded = load_graph_text(&changed).expect("changed timeout");
    assert_ne!(base.graph_hash, changed_loaded.graph_hash);
}

#[test]
fn hash_changes_when_input_port_description_changes() {
    // 端口 description 属于 §2.8.3 白名单（节点内容变化必变）。
    let mut graph = serde_json::from_str::<serde_json::Value>(&graph_json(false)).expect("json");
    let base = load_graph_text(&graph_json(false)).expect("base");
    graph["nodes"][1]["inputs"][0]["description"] = json!("documented port");
    let changed = zgraph_dsl::load_graph_value(graph, &Default::default()).expect("changed port");
    assert_ne!(base.graph_hash, changed.graph_hash);
}

#[test]
fn hash_is_16_lowercase_hex() {
    let loaded = load_graph(&customer_ticket_bytes()).expect("fixture");
    assert_eq!(loaded.graph_hash.len(), 16);
    assert!(loaded
        .graph_hash
        .chars()
        .all(|ch| ch.is_ascii_hexdigit() && !ch.is_ascii_uppercase()));
}

#[test]
fn canonical_json_has_sorted_keys_and_no_whitespace() {
    let graph = json!({
        "version": "1.0",
        "metadata": {"name": "sorted"},
        "entry_points": ["start"],
        "nodes": [{"id": "start", "type": "start", "inputs": [], "config": {}}],
        "edges": []
    });
    let canonical = canonical_json(&graph);
    assert!(!canonical.contains('\n'));
    assert!(!canonical.contains(' '));
    let edges = canonical.find("\"edges\"").expect("edges key");
    let metadata = canonical.find("\"metadata\"").expect("metadata key");
    assert!(edges < metadata, "keys sorted: {canonical}");
    let hash = graph_hash(&graph);
    assert_eq!(hash.len(), 16);
}

#[test]
fn golden_hash_for_customer_ticket_fixture() {
    let loaded = load_graph(&customer_ticket_bytes()).expect("fixture");
    // golden：锁定 canonical JSON 实现；fixture 内容变更时需同步更新。
    assert_eq!(loaded.graph_hash, "ec320e4244b365ab");
}

fn fixture(name: &str) -> serde_json::Value {
    let bytes = std::fs::read(repo_root().join("tests/fixtures/graphs").join(name))
        .unwrap_or_else(|error| panic!("fixture {name}: {error}"));
    serde_json::from_slice(&bytes).expect("fixture is valid JSON")
}

fn load_value(graph: serde_json::Value) -> zgraph_dsl::LoadedGraph {
    zgraph_dsl::load_graph_value(graph, &Default::default()).expect("graph is valid")
}

#[test]
fn golden_hash_for_valid_cycle_fixture_is_unchanged_by_w2a() {
    // golden 捕获于 W2a 改动前（v0.1.0 语义）：新增的 `on_gate_false` 不得改变
    // 既有图的 hash（不补齐默认值、未写不投影）。
    let loaded = load_value(fixture("valid_cycle.json"));
    assert_eq!(loaded.graph_hash, "0e8911b6b4003c7e");
    assert!(!loaded.canonical_json().contains("on_gate_false"));
}

#[test]
fn omitted_on_gate_false_is_not_backfilled_by_projection() {
    let base = load_value(fixture("valid_cycle.json"));
    let mut explicit = fixture("valid_cycle.json");
    explicit["edges"][1]["on_gate_false"] = json!("settle");
    let loaded = load_value(explicit);
    assert_ne!(
        base.graph_hash, loaded.graph_hash,
        "explicit settle must differ from omitted (no default backfill)"
    );
    assert!(!base.canonical_json().contains("on_gate_false"));
    assert!(
        loaded
            .canonical_json()
            .contains("\"on_gate_false\":\"settle\""),
        "canonical projection must keep the explicit value: {}",
        loaded.canonical_json()
    );
}

#[test]
fn on_gate_false_defer_changes_hash_and_differs_from_settle() {
    let base = load_value(fixture("valid_cycle.json"));
    let mut settle = fixture("valid_cycle.json");
    settle["edges"][1]["on_gate_false"] = json!("settle");
    let settle = load_value(settle);

    // 自环 defer 合法（R15 的可重入自环分支），确保 hash 用例本身是合法图。
    let mut defer = fixture("valid_cycle.json");
    defer["edges"][1]["on_gate_false"] = json!("defer");
    let defer = load_value(defer);

    assert_ne!(base.graph_hash, defer.graph_hash);
    assert_ne!(settle.graph_hash, defer.graph_hash);
    assert!(defer
        .canonical_json()
        .contains("\"on_gate_false\":\"defer\""));
}

#[test]
fn golden_hash_for_gate_defer_fixture() {
    let loaded = load_value(fixture("valid_gate_defer.json"));
    // golden：锁定含 `on_gate_false` 的 canonical 投影（W2a）。
    assert_eq!(loaded.graph_hash, "2ff1339bd3d6d928");
}
