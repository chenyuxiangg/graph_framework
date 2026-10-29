//! Fixtures 全覆盖测试：每个文件都被引用，坏图命中期望错误码。
//!
//! 对应 02 §2.4 / 07 AC-04（M1 口径）：17 个 DSL 码各一个反例 + R3 版本反例 +
//! 4 个合法图。fixture 目录：`tests/fixtures/graphs/`；资产：`tests/fixtures/assets/`。

use std::collections::BTreeSet;
use std::path::PathBuf;

use zgraph_dsl::{load_graph, load_graph_with_options, DslErrorCode, LoadOptions, LoadedGraph};

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

fn load(name: &str, with_assets: bool) -> Result<LoadedGraph, zgraph_dsl::DslErrors> {
    let bytes = std::fs::read(fixtures_dir().join(name)).expect("fixture readable");
    if with_assets {
        load_graph_with_options(&bytes, &LoadOptions::new().with_assets_dir(assets_dir()))
    } else {
        load_graph(&bytes)
    }
}

/// 坏图 fixture → 期望错误码（17 码 + `bad_version` 的 R3 反例）。
const BAD_CASES: [(&str, DslErrorCode); 18] = [
    ("bad_parse_error.json", DslErrorCode::ParseError),
    ("bad_missing_version.json", DslErrorCode::MissingField),
    ("bad_version.json", DslErrorCode::ParseError),
    ("bad_dup_node_id.json", DslErrorCode::DuplicateNodeId),
    ("bad_entry.json", DslErrorCode::InvalidEntry),
    ("bad_edge_ref.json", DslErrorCode::InvalidEdgeRef),
    ("bad_port.json", DslErrorCode::InvalidPort),
    ("bad_dup_edge.json", DslErrorCode::DuplicateEdge),
    (
        "bad_dup_port_source.json",
        DslErrorCode::DuplicatePortSource,
    ),
    ("bad_unconnected_input.json", DslErrorCode::UnconnectedInput),
    ("bad_subgraph_bad.json", DslErrorCode::SubgraphInvalid),
    ("bad_cycle.json", DslErrorCode::CycleDetected),
    ("bad_symbol.json", DslErrorCode::SymbolNotFound),
    ("bad_signature.json", DslErrorCode::SignatureMismatch),
    ("bad_symbol_conflict.json", DslErrorCode::SymbolConflict),
    (
        "bad_unknown_node_type.json",
        DslErrorCode::UnsupportedNodeType,
    ),
    ("bad_reserved_field.json", DslErrorCode::UnsupportedField),
    ("bad_agent_not_found.json", DslErrorCode::AgentNotFound),
];

/// 合法 fixture 名单（全部必须通过 R1–R14）。
const VALID_CASES: [&str; 4] = [
    "customer_ticket.json",
    "valid_parallel.json",
    "valid_cycle.json",
    "valid_subgraph.json",
];

fn fixture_needs_assets(name: &str) -> bool {
    matches!(name, "customer_ticket.json" | "bad_agent_not_found.json")
}

#[test]
fn every_fixture_is_referenced_and_matches_expected_code() {
    let mut referenced: BTreeSet<String> = BTreeSet::new();
    for (name, expected) in BAD_CASES {
        referenced.insert(name.to_string());
        let errors = load(name, fixture_needs_assets(name)).expect_err(name);
        assert_eq!(errors.first().code, expected, "fixture {name}: {errors}");
    }
    for name in VALID_CASES {
        referenced.insert(name.to_string());
        load(name, fixture_needs_assets(name)).unwrap_or_else(|errors| panic!("{name}: {errors}"));
    }
    let on_disk: BTreeSet<String> = std::fs::read_dir(fixtures_dir())
        .expect("fixtures dir")
        .filter_map(Result::ok)
        .map(|entry| entry.file_name().to_string_lossy().to_string())
        .filter(|name| name.ends_with(".json"))
        .collect();
    assert_eq!(
        on_disk, referenced,
        "every fixture must be referenced by a test"
    );
}

#[test]
fn bad_fixtures_carry_structured_location() {
    for (name, _) in BAD_CASES {
        let errors = load(name, fixture_needs_assets(name)).expect_err(name);
        let first = errors.first();
        // R1（JSON 解析）与文档级错误允许 location 为空；其余必须有定位。
        let document_level =
            matches!(first.code, DslErrorCode::ParseError) && name == "bad_parse_error.json";
        if !document_level {
            assert!(
                !first.location.is_empty(),
                "fixture {name} must carry node/edge/port/field location: {first:?}"
            );
        }
        if document_level {
            continue;
        }
        // 错误对象必须带 expected 或 actual 之一（02 §2.7.1）。
        assert!(
            first.expected.is_some() || first.actual.is_some(),
            "fixture {name} must carry expected/actual: {first:?}"
        );
    }
}

#[test]
fn customer_ticket_fixture_matches_spec_shape() {
    let loaded = load("customer_ticket.json", true).expect("valid customer ticket graph");
    assert_eq!(loaded.definition.node_count(), 6);
    assert_eq!(loaded.definition.edge_count(), 6);
    assert_eq!(loaded.definition.subgraph_count(), 0);
    assert_eq!(loaded.graph_hash.len(), 16);
    let end = loaded
        .definition
        .nodes
        .iter()
        .find(|node| node.id == "end_success")
        .expect("end node");
    assert!(
        !end.inputs.is_empty(),
        "C-15 regression: end declares inputs"
    );
}

#[test]
fn valid_cycle_fixture_contains_gated_self_loop() {
    let loaded = load("valid_cycle.json", false).expect("valid cycle graph");
    let self_loop = loaded
        .definition
        .edges
        .iter()
        .find(|edge| edge.from == edge.to_node)
        .expect("self loop edge");
    assert!(self_loop.gate.is_some(), "self loop must be gated");
}
