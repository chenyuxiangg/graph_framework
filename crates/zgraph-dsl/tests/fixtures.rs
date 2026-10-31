//! Fixtures 全覆盖测试：每个文件都被引用，坏图命中期望错误码**与定位**。
//!
//! 对应 02 §2.4 / §2.7.1 / §2.7.3 与 07 AC-04（M1 口径）：18 个 DSL 码反例、
//! R3 版本/时间戳反例、REV-005 回归反例。fixture 目录为 `tests/fixtures/graphs/`，
//! 资产目录为 `tests/fixtures/assets/`。

use std::collections::BTreeSet;
use std::path::PathBuf;

use zgraph_dsl::{
    load_graph, load_graph_with_options, DslErrorCode, DslErrors, LoadOptions, LoadedGraph,
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

fn load(name: &str, with_assets: bool) -> Result<LoadedGraph, DslErrors> {
    let bytes = std::fs::read(fixtures_dir().join(name)).expect("fixture readable");
    if with_assets {
        load_graph_with_options(&bytes, &LoadOptions::new().with_assets_dir(assets_dir()))
    } else {
        load_graph(&bytes)
    }
}

/// 期望的 `location`（02 §2.7.3：仅 `node_id`/`edge_index`/`port`）。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct Loc {
    node: Option<&'static str>,
    edge: Option<usize>,
    port: Option<&'static str>,
}

const DOC_LEVEL: Loc = Loc {
    node: None,
    edge: None,
    port: None,
};

const fn node(id: &'static str) -> Loc {
    Loc {
        node: Some(id),
        edge: None,
        port: None,
    }
}

const fn edge(index: usize) -> Loc {
    Loc {
        node: None,
        edge: Some(index),
        port: None,
    }
}

/// 坏图 fixture → 期望错误码 + 期望定位（REV-005 后为强断言）。
const BAD_CASES: [(&str, DslErrorCode, Loc); 33] = [
    // R1/R2/R3 文档级
    ("bad_parse_error.json", DslErrorCode::ParseError, DOC_LEVEL),
    (
        "bad_missing_version.json",
        DslErrorCode::MissingField,
        DOC_LEVEL,
    ),
    ("bad_version.json", DslErrorCode::ParseError, DOC_LEVEL),
    (
        "bad_created_at_invalid.json",
        DslErrorCode::ParseError,
        DOC_LEVEL,
    ),
    (
        "bad_created_at_offset.json",
        DslErrorCode::ParseError,
        DOC_LEVEL,
    ),
    // R4–R13
    (
        "bad_dup_node_id.json",
        DslErrorCode::DuplicateNodeId,
        node("start"),
    ),
    ("bad_entry.json", DslErrorCode::InvalidEntry, node("ghost")),
    (
        "bad_edge_ref.json",
        DslErrorCode::InvalidEdgeRef,
        Loc {
            node: Some("ghost"),
            edge: Some(0),
            port: None,
        },
    ),
    (
        "bad_port.json",
        DslErrorCode::InvalidPort,
        Loc {
            node: Some("n1"),
            edge: Some(0),
            port: Some("zzz"),
        },
    ),
    (
        "bad_dup_edge.json",
        DslErrorCode::DuplicateEdge,
        Loc {
            node: Some("start"),
            edge: Some(1),
            port: None,
        },
    ),
    (
        "bad_dup_port_source.json",
        DslErrorCode::DuplicatePortSource,
        Loc {
            node: Some("n1"),
            edge: Some(1),
            port: Some("x"),
        },
    ),
    (
        "bad_unconnected_input.json",
        DslErrorCode::UnconnectedInput,
        Loc {
            node: Some("n1"),
            port: Some("y"),
            edge: None,
        },
    ),
    (
        "bad_subgraph_bad.json",
        DslErrorCode::SubgraphInvalid,
        node("inner"),
    ),
    (
        "bad_cycle.json",
        DslErrorCode::CycleDetected,
        Loc {
            node: Some("a"),
            edge: Some(1),
            port: None,
        },
    ),
    (
        "bad_missing_edge_field.json",
        DslErrorCode::MissingField,
        edge(0),
    ),
    // R14
    ("bad_symbol.json", DslErrorCode::SymbolNotFound, edge(0)),
    (
        "bad_signature.json",
        DslErrorCode::SignatureMismatch,
        edge(0),
    ),
    (
        "bad_symbol_conflict.json",
        DslErrorCode::SymbolConflict,
        edge(1),
    ),
    (
        "bad_unknown_node_type.json",
        DslErrorCode::UnsupportedNodeType,
        node("start"),
    ),
    (
        "bad_reserved_field.json",
        DslErrorCode::UnsupportedField,
        node("start"),
    ),
    (
        "bad_agent_not_found.json",
        DslErrorCode::AgentNotFound,
        node("agent_node"),
    ),
    // REV-005 回归：on_error 只在节点级是保留字段
    (
        "bad_on_error_top_level.json",
        DslErrorCode::ParseError,
        DOC_LEVEL,
    ),
    (
        "bad_on_error_config.json",
        DslErrorCode::ParseError,
        node("a"),
    ),
    ("bad_on_error_edge.json", DslErrorCode::ParseError, edge(0)),
    (
        "bad_on_error_port.json",
        DslErrorCode::ParseError,
        node("n1"),
    ),
    ("bad_on_error_gate.json", DslErrorCode::ParseError, edge(0)),
    // REV-005 回归：from_output_port 出现即报错，不看取值
    (
        "bad_from_output_port_nonstring.json",
        DslErrorCode::UnsupportedField,
        edge(0),
    ),
    // REV-005 复审：有效 SubgraphId 的子图头错误 → node_id = SubgraphId（且不级联）
    (
        "bad_subgraph_header_unknown.json",
        DslErrorCode::ParseError,
        node("inner"),
    ),
    (
        "bad_subgraph_header_on_error.json",
        DslErrorCode::ParseError,
        node("inner"),
    ),
    // REV-005 回归：子图 id 必须是非空 string
    (
        "bad_subgraph_id_empty.json",
        DslErrorCode::MissingField,
        DOC_LEVEL,
    ),
    (
        "bad_subgraph_id_type.json",
        DslErrorCode::ParseError,
        DOC_LEVEL,
    ),
    // W2a：边级 on_gate_false
    (
        "bad_defer_not_cycle.json",
        DslErrorCode::DeferNotReentrant,
        Loc {
            node: Some("a"),
            edge: Some(1),
            port: None,
        },
    ),
    ("bad_defer_value.json", DslErrorCode::ParseError, edge(1)),
];

/// 合法 fixture 名单（全部必须通过 R1–R15）。
const VALID_CASES: [&str; 6] = [
    "customer_ticket.json",
    "valid_parallel.json",
    "valid_cycle.json",
    "valid_subgraph.json",
    "valid_created_at_offset.json",
    "valid_gate_defer.json",
];

fn fixture_needs_assets(name: &str) -> bool {
    matches!(name, "customer_ticket.json" | "bad_agent_not_found.json")
}

fn assert_location(name: &str, expected: Loc, first: &zgraph_dsl::DslError) {
    assert_eq!(
        first.location.node_id.as_deref(),
        expected.node,
        "fixture {name}: node_id mismatch ({first:?})"
    );
    assert_eq!(
        first.location.edge_index, expected.edge,
        "fixture {name}: edge_index mismatch ({first:?})"
    );
    assert_eq!(
        first.location.port.as_deref(),
        expected.port,
        "fixture {name}: port mismatch ({first:?})"
    );
}

#[test]
fn every_fixture_is_referenced_and_matches_expected_code_and_location() {
    let mut referenced = BTreeSet::new();
    for (name, expected, location) in BAD_CASES {
        referenced.insert(name.to_string());
        let errors = load(name, fixture_needs_assets(name)).expect_err(name);
        assert_eq!(errors.first().code, expected, "fixture {name}: {errors}");
        // 每个坏图恰好一个错误（除显式聚合用例），错误序列稳定。
        assert_eq!(errors.error_count, 1, "fixture {name}: {errors}");
        assert_location(name, location, errors.first());
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
fn bad_fixtures_carry_expected_or_actual_and_document_level_are_empty() {
    for (name, code, location) in BAD_CASES {
        let errors = load(name, fixture_needs_assets(name)).expect_err(name);
        let first = errors.first();
        assert_eq!(first.code, code, "fixture {name}: {errors}");
        if location == DOC_LEVEL {
            // R1/R2/R3 与子图头定义错误：02 §2.7.3 文档级 location = {}。
            assert!(
                first.location.node_id.is_none()
                    && first.location.edge_index.is_none()
                    && first.location.port.is_none(),
                "fixture {name} must be document-level: {first:?}"
            );
        }
        if code != DslErrorCode::ParseError || location != DOC_LEVEL {
            assert!(
                first.expected.is_some() || first.actual.is_some(),
                "fixture {name} must carry expected/actual: {first:?}"
            );
        }
        // 子图 id 反例必须把路径写进 message（location 无 field 键）。
        if name.starts_with("bad_subgraph_id") {
            assert!(
                first.message.contains("subgraphs[0]"),
                "fixture {name}: {first:?}"
            );
        }
    }
}

#[test]
fn location_serializes_only_contract_keys() {
    let allowed: BTreeSet<&str> = ["node_id", "edge_index", "port"].into_iter().collect();
    for (name, _, location) in BAD_CASES {
        let errors = load(name, fixture_needs_assets(name)).expect_err(name);
        let value = serde_json::to_value(errors.first()).expect("serialize error");
        let keys: BTreeSet<&str> = value["location"]
            .as_object()
            .expect("location object")
            .keys()
            .map(String::as_str)
            .collect();
        assert!(
            keys.is_subset(&allowed),
            "fixture {name}: location keys {keys:?} (expected) {location:?}"
        );
        if location != DOC_LEVEL {
            assert!(
                !keys.is_empty(),
                "fixture {name}: location must not be empty"
            );
        }
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
