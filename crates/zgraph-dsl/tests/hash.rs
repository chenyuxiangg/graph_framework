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
