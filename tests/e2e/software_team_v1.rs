//! W1（v0.2 / M7）software_team SOP v1 e2e：codegen 参考图 validate + 规模/确定性断言。
//!
//! 语义基准：`doc/design/software-team-sop-v1.md` §4.1（设计角色依赖矩阵）、§7（N 与
//! codegen 规则）、§11.1（`zgraph validate` 通过参考图）、附录 A/B；
//! `doc/decisions/DR-004-v02-scope-and-waves.md` §3 W1。
//!
//! 数据通路（真链路）：
//!
//! ```text
//! tests/fixtures/graphs/software_team_v1/codegen.py（stdlib 纯函数）
//!   → 图 JSON（full = 60 节点/100 边；smoke = 11 节点/18 边）
//!   → 真 `zgraph` 二进制 `validate --assets-dir tests/fixtures/graphs/software_team_v1`
//!     （R1–R14 全跑，含 R14c agent 资产索引）
//! ```
//!
//! 断言口径：
//!
//! - **规模**：full 恰 60 节点/100 边、smoke 恰 11 节点/18 边（§4.1 参考规模）；
//! - **确定性**：同 profile 两次生成字节级一致，`graph_hash` 相同（DR-004 W1 DoD）；
//! - **结构**：11 个设计角色 + 16 个工作/评审/解析节点 + `end`；每个 item 的 5 条内部边
//!   与 approved gate 投递边齐备（§7 规则，测试侧用 §4.1 矩阵独立复算）；
//! - **smoke 同构**：单阶段链路 `planer → 3 工作 → 3 评审 → 3 解析 → archer`，`archer`
//!   是无出边的叶节点（其 run 语义由 fake LLM 用例覆盖）。
//!
//! 稳定性约定：codegen 用 `python3`/`ZGRAPH_PYTHON` 执行（与 e2e 其它 Python 断言同口径，
//! 不写 `__pycache__`）；每个用例独立 `tempfile::TempDir`。

#![cfg(unix)]

#[path = "cli_support/mod.rs"]
mod cli_support;
#[path = "common/mod.rs"]
mod common;

use std::collections::{BTreeSet, HashSet};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::Value;

use cli_support::{arg, assert_exit, repo_root, run_cli, stderr_text};

/// e2e fixture 目录名（`tests/fixtures/graphs/software_team_v1`）。
const FIXTURE_GRAPH_DIR: &str = "software_team_v1";

/// codegen profile。
const PROFILE_FULL: &str = "full";
const PROFILE_SMOKE: &str = "smoke";

/// §4.1 参考规模（节点数 / 边数）。
const FULL_NODE_COUNT: usize = 60;
const FULL_EDGE_COUNT: usize = 100;
const SMOKE_NODE_COUNT: usize = 11;
const SMOKE_EDGE_COUNT: usize = 18;

/// §4.1 依赖矩阵（测试侧独立 oracle：`(role, deps, items)`）。
const DESIGN_MATRIX: &[(&str, &[&str], &[&str])] = &[
    ("planer", &[], &["specification"]),
    (
        "archer",
        &["planer.specification"],
        &["arch", "org", "iter_plan"],
    ),
    (
        "mde",
        &["archer.arch", "archer.org", "archer.iter_plan"],
        &["module_design", "module_iter_plan"],
    ),
    (
        "te",
        &["archer.arch", "archer.org", "archer.iter_plan"],
        &["test_plan"],
    ),
    ("mte", &["te.test_plan"], &["module_cases"]),
    ("ste", &["te.test_plan"], &["sys_cases"]),
    (
        "fo",
        &["mde.module_design", "mde.module_iter_plan"],
        &["feature_process"],
    ),
    (
        "mto",
        &["mte.module_cases", "fo.feature_process"],
        &["module_report"],
    ),
    (
        "sto",
        &["ste.sys_cases", "mto.module_report"],
        &["sys_report"],
    ),
    ("qa", &["sto.sys_report"], &["qa_verdict"]),
    (
        "release",
        &["qa.qa_verdict"],
        &["notes", "changelog", "guide"],
    ),
];

/// full profile 的 `end` 依赖（release 的 3 个 item）。
const FULL_END_DEPS: &[&str] = &["release.notes", "release.changelog", "release.guide"];

/// smoke profile 的 3 个 planer 工作 item（单阶段链路）。
const SMOKE_ITEMS: &[&str] = &["specification", "requirement_analysis", "iter_plan"];

// ---------------------------------------------------------------------------
// fixture / codegen / validate 辅助
// ---------------------------------------------------------------------------

/// `tests/fixtures/graphs/software_team_v1`（agent 资产 + prompts + tools + codegen + py）。
fn fixture_dir() -> PathBuf {
    common::graphs_root().join(FIXTURE_GRAPH_DIR)
}

/// codegen 脚本路径。
fn codegen_path() -> PathBuf {
    fixture_dir().join("codegen.py")
}

/// 执行 codegen，返回生成的图 JSON 路径（`--out`）。
fn generate_graph(dir: &Path, profile: &str) -> PathBuf {
    let out = dir.join(format!("graph_{profile}.json"));
    let output = Command::new(common::python_program())
        .arg(codegen_path())
        .args(["--profile", profile])
        .arg("--out")
        .arg(&out)
        .env(common::PYTHONDONTWRITEBYTECODE, "1")
        .current_dir(common::repo_root())
        .output()
        .expect("spawn codegen.py");
    assert!(
        output.status.success(),
        "codegen {profile} 失败（exit={:?}）\nstdout: {}\nstderr: {}",
        output.status.code(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(out.is_file(), "codegen 未写出 {}", out.display());
    out
}

/// 真 `zgraph validate <graph> --assets-dir <fixture>`（R1–R14 全跑，含 agent 资产索引）。
fn validate_graph(graph: &Path) -> Output {
    run_cli(
        &[
            "validate".to_string(),
            arg(graph),
            "--assets-dir".to_string(),
            arg(&fixture_dir()),
        ],
        &repo_root(),
    )
}

/// 解析 `validate` 人类可读输出：`✓ graph valid: N nodes, M edges, ... (graph_hash=h)`。
fn parse_validate_summary(output: &Output) -> (usize, usize, String) {
    assert_exit(output, 0, "zgraph validate");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let line = stdout
        .lines()
        .find(|line| line.contains("graph valid:"))
        .unwrap_or_else(|| panic!("validate stdout 缺少 'graph valid' 行: {stdout}"));
    let tokens: Vec<&str> = line.split_whitespace().collect();
    let number_before = |marker: &str| -> usize {
        tokens
            .windows(2)
            .find(|pair| pair[1] == marker)
            .and_then(|pair| pair[0].parse::<usize>().ok())
            .unwrap_or_else(|| panic!("validate 输出缺少 {marker} 计数: {line}"))
    };
    let nodes = number_before("nodes,");
    let edges = number_before("edges,");
    let hash = tokens
        .iter()
        .find_map(|token| token.find("graph_hash=").map(|index| &token[index..]))
        .map(|token| {
            token
                .trim_start_matches("graph_hash=")
                .trim_end_matches(')')
                .to_string()
        })
        .unwrap_or_else(|| {
            panic!(
                "validate 输出缺少 graph_hash: {line}\nstderr: {}",
                stderr_text(output)
            )
        });
    assert!(!hash.is_empty(), "graph_hash 不得为空");
    (nodes, edges, hash)
}

/// 读取生成的图 JSON。
fn read_graph(path: &Path) -> Value {
    let text = std::fs::read_to_string(path)
        .unwrap_or_else(|error| panic!("读取生成的图 {} 失败: {error}", path.display()));
    serde_json::from_str(&text).unwrap_or_else(|error| panic!("生成的图不是合法 JSON: {error}"))
}

// ---------------------------------------------------------------------------
// 图结构断言辅助
// ---------------------------------------------------------------------------

/// 节点数组。
fn nodes(graph: &Value) -> &Vec<Value> {
    graph["nodes"].as_array().expect("graph.nodes 数组")
}

/// 边数组。
fn edges(graph: &Value) -> &Vec<Value> {
    graph["edges"].as_array().expect("graph.edges 数组")
}

/// 按 id 取节点。
fn node_by_id<'a>(graph: &'a Value, node_id: &str) -> &'a Value {
    nodes(graph)
        .iter()
        .find(|node| node["id"] == Value::from(node_id))
        .unwrap_or_else(|| panic!("图中缺少节点 {node_id}"))
}

/// 节点输入端口名（声明顺序）。
fn input_names(node: &Value) -> Vec<String> {
    node["inputs"]
        .as_array()
        .expect("node.inputs 数组")
        .iter()
        .map(|port| port["name"].as_str().expect("port.name 字符串").to_string())
        .collect()
}

/// 边三元组 `(from, to_node, to_input_port)`。
fn edge_triple(edge: &Value) -> (String, String, String) {
    (
        edge["from"].as_str().expect("edge.from").to_string(),
        edge["to_node"].as_str().expect("edge.to_node").to_string(),
        edge["to_input_port"]
            .as_str()
            .expect("edge.to_input_port")
            .to_string(),
    )
}

/// 是否存在指定三元组的边。
fn has_edge(graph: &Value, from: &str, to_node: &str, to_port: &str) -> bool {
    edges(graph).iter().any(|edge| {
        edge_triple(edge) == (from.to_string(), to_node.to_string(), to_port.to_string())
    })
}

/// 边上的 gate 是否恰为 approved 不变量 `check_output{path:"verdict", equals:"approved"}`。
fn is_approved_gate(edge: &Value) -> bool {
    edge["gate"]["function"] == Value::from("check_output")
        && edge["gate"]["params"]["path"] == Value::from("verdict")
        && edge["gate"]["params"]["equals"] == Value::from("approved")
}

/// 断言一个设计角色节点：entry 声明 `requirement`，其余每个依赖 item 一个端口。
fn assert_design_node(graph: &Value, role_id: &str, deps: &[&str]) {
    let node = node_by_id(graph, role_id);
    assert_eq!(node["type"], Value::from("agent_call"), "{role_id} 类型");
    assert_eq!(
        node["config"]["agent_id"],
        Value::from(role_id),
        "{role_id}.agent_id"
    );
    assert_eq!(node["trigger"], Value::from("all"), "{role_id}.trigger");
    let expected: Vec<String> = if deps.is_empty() {
        vec!["requirement".to_string()]
    } else {
        deps.iter()
            .map(|dep| dep.rsplit('.').next().expect("dep item").to_string())
            .collect()
    };
    let mut expected_sorted = expected.clone();
    expected_sorted.sort();
    let mut actual = input_names(node);
    actual.sort();
    assert_eq!(actual, expected_sorted, "{role_id} 输入端口");
    for port in node["inputs"].as_array().expect("inputs") {
        assert_eq!(
            port["required"],
            Value::from(true),
            "{role_id} 端口必须 required"
        );
    }
    if deps.is_empty() {
        assert_eq!(input_names(node), vec!["requirement".to_string()]);
        assert_eq!(node["inputs"][0]["type"], Value::from("string"));
    } else {
        assert_eq!(
            input_names(node),
            expected,
            "{role_id} 端口顺序应与依赖顺序一致"
        );
    }
}

/// 断言一个 item 的 5 条内部边（§7 第 2 步：D→W/W→R/R→P/W→P/D→R）。
fn assert_item_pipeline(graph: &Value, role_id: &str, item: &str) {
    let base = format!("{role_id}_{item}");
    let work = node_by_id(graph, &base);
    assert_eq!(work["type"], Value::from("python_node"), "{base} 类型");
    assert_eq!(work["config"]["module"], Value::from("software_team_stub"));
    assert_eq!(
        work["config"]["function"],
        Value::from(format!("work_{role_id}_{item}"))
    );
    assert_eq!(input_names(work), vec!["task".to_string()]);

    let review = node_by_id(graph, &format!("{base}_review"));
    assert_eq!(review["type"], Value::from("agent_call"));
    assert_eq!(review["config"]["agent_id"], Value::from("design_reviewer"));
    assert_eq!(review["trigger"], Value::from("all"));

    let parse = node_by_id(graph, &format!("{base}_parse"));
    assert_eq!(parse["type"], Value::from("python_node"));
    assert_eq!(
        parse["config"]["module"],
        Value::from("workers.software_team.parse_verdict")
    );
    assert_eq!(parse["config"]["function"], Value::from("run"));
    assert_eq!(parse["trigger"], Value::from("all"));

    assert!(has_edge(graph, role_id, &base, "task"), "{base}: D→W.task");
    assert!(
        has_edge(graph, role_id, &format!("{base}_review"), "spec"),
        "{base}: D→R.spec"
    );
    assert!(
        has_edge(graph, &base, &format!("{base}_review"), "result"),
        "{base}: W→R.result"
    );
    assert!(
        has_edge(graph, &base, &format!("{base}_parse"), "artifact"),
        "{base}: W→P.artifact"
    );
    assert!(
        has_edge(
            graph,
            &format!("{base}_review"),
            &format!("{base}_parse"),
            "review"
        ),
        "{base}: R→P.review"
    );
}

/// 断言 `(from_node, to_node)` 唯一（附录 B），返回去重后的三元组集合。
fn unique_edge_triples(graph: &Value) -> BTreeSet<(String, String, String)> {
    let mut from_to: HashSet<(String, String)> = HashSet::new();
    let mut to_port: HashSet<(String, String)> = HashSet::new();
    let mut triples: BTreeSet<(String, String, String)> = BTreeSet::new();
    for edge in edges(graph) {
        let (from, to_node, port) = edge_triple(edge);
        assert!(
            from_to.insert((from.clone(), to_node.clone())),
            "重复边 (from={from}, to_node={to_node})"
        );
        assert!(
            to_port.insert((to_node.clone(), port.clone())),
            "重复端口 (to_node={to_node}, to_input_port={port})"
        );
        triples.insert((from, to_node, port));
    }
    triples
}

// ---------------------------------------------------------------------------
// full profile：60 节点 / 100 边，两次生成一致
// ---------------------------------------------------------------------------

/// full codegen：规模/结构/确定性/`zgraph validate`（含 agent 资产索引）全绿。
#[test]
fn full_codegen_validates_with_60_nodes_100_edges_and_stable_hash() {
    let dir = tempfile::tempdir().expect("tempdir");
    let first_dir = dir.path().join("first");
    let second_dir = dir.path().join("second");
    std::fs::create_dir_all(&first_dir).expect("first dir");
    std::fs::create_dir_all(&second_dir).expect("second dir");

    let first = generate_graph(&first_dir, PROFILE_FULL);
    let second = generate_graph(&second_dir, PROFILE_FULL);

    // 确定性：同参数两次生成字节级一致（无随机数/时间戳）。
    let first_bytes = std::fs::read(&first).expect("read first graph");
    let second_bytes = std::fs::read(&second).expect("read second graph");
    assert_eq!(first_bytes, second_bytes, "同参数两次 codegen 应字节级一致");

    // 真 CLI validate（R1–R14，含 R14c 资产索引）→ 规模与 graph_hash。
    let (first_nodes, first_edges, first_hash) = parse_validate_summary(&validate_graph(&first));
    let (second_nodes, second_edges, second_hash) =
        parse_validate_summary(&validate_graph(&second));
    assert_eq!(
        (first_nodes, first_edges),
        (FULL_NODE_COUNT, FULL_EDGE_COUNT)
    );
    assert_eq!(
        (second_nodes, second_edges),
        (FULL_NODE_COUNT, FULL_EDGE_COUNT)
    );
    assert_eq!(
        first_hash, second_hash,
        "同参数两次生成的 graph_hash 必须一致"
    );

    let graph = read_graph(&first);
    assert_eq!(graph["version"], Value::from("1.0"));
    assert_eq!(graph["entry_points"], serde_json::json!(["planer"]));
    assert_eq!(nodes(&graph).len(), FULL_NODE_COUNT);
    assert_eq!(edges(&graph).len(), FULL_EDGE_COUNT);
    unique_edge_triples(&graph);

    // 节点类型规模：11 设计角色 + 16 评审（agent_call）、16 工作 + 16 解析（python_node）、1 end。
    let type_counts = |node_type: &str| {
        nodes(&graph)
            .iter()
            .filter(|node| node["type"] == Value::from(node_type))
            .count()
    };
    assert_eq!(type_counts("agent_call"), 11 + 16, "agent_call 节点数");
    assert_eq!(type_counts("python_node"), 16 + 16, "python_node 节点数");
    assert_eq!(type_counts("end"), 1, "end 节点数");

    // §4.1 矩阵：每个设计角色 + 每个 item 的 D→W→R→P 管线。
    for (role_id, deps, items) in DESIGN_MATRIX {
        assert_design_node(&graph, role_id, deps);
        for item in *items {
            assert_item_pipeline(&graph, role_id, item);
        }
    }

    // approved gate 投递边：每个被依赖 item 恰一条（20 条）。
    let gated: Vec<&Value> = edges(&graph)
        .iter()
        .filter(|edge| edge.get("gate").is_some())
        .collect();
    let expected_gated = DESIGN_MATRIX
        .iter()
        .flat_map(|(_, deps, _)| deps.iter())
        .count()
        + FULL_END_DEPS.len();
    assert_eq!(gated.len(), expected_gated, "gate 边数");
    for edge in gated {
        assert!(
            is_approved_gate(edge),
            "gate 必须为 approved 不变量: {edge}"
        );
        let (from, to_node, to_port) = edge_triple(edge);
        assert!(
            from.ends_with("_parse"),
            "gate 边必须从解析节点发出: {from}"
        );
        let target = node_by_id(&graph, &to_node);
        assert_ne!(
            target["type"],
            Value::from("python_node"),
            "gate 边目标应为设计角色或 end"
        );
        assert!(!to_port.is_empty(), "gate 边必须有目标端口");
    }

    // `end` 等 release 的 3 个 item（附录 B：≥1 required、trigger=all）。
    let end = node_by_id(&graph, "end");
    assert_eq!(end["type"], Value::from("end"));
    assert_eq!(end["trigger"], Value::from("all"));
    assert_eq!(
        input_names(end),
        FULL_END_DEPS
            .iter()
            .map(|dep| dep.rsplit('.').next().expect("item").to_string())
            .collect::<Vec<_>>()
    );
    for dep in FULL_END_DEPS {
        let (role_id, item) = dep.rsplit_once('.').expect("role.item");
        assert!(
            has_edge(&graph, &format!("{role_id}_{item}_parse"), "end", item),
            "{dep} 的 approved 投递边应指向 end.{item}"
        );
    }

    // 非 entry 节点入边 ≥ 1（附录 B）；`planer` 是唯一 entry（requirement 由 run 注入）。
    for node in nodes(&graph) {
        let node_id = node["id"].as_str().expect("node.id");
        if node_id == "planer" {
            assert!(
                !edges(&graph)
                    .iter()
                    .any(|edge| edge["to_node"] == node["id"]),
                "planer 不应有入边"
            );
            continue;
        }
        assert!(
            edges(&graph)
                .iter()
                .any(|edge| edge["to_node"] == node["id"]),
            "非 entry 节点 {node_id} 必须有入边"
        );
    }
}

// ---------------------------------------------------------------------------
// smoke profile：11 节点 / 18 边（单阶段链路，同构）
// ---------------------------------------------------------------------------

/// smoke codegen：`validate` 通过 + 单阶段链路结构（planer → 3 工作 → 3 评审 → 3 解析 → archer）。
#[test]
fn smoke_codegen_is_isomorphic_single_stage_pipeline() {
    let dir = tempfile::tempdir().expect("tempdir");
    let graph_path = generate_graph(dir.path(), PROFILE_SMOKE);
    let (validated_nodes, validated_edges, _hash) =
        parse_validate_summary(&validate_graph(&graph_path));
    assert_eq!(
        (validated_nodes, validated_edges),
        (SMOKE_NODE_COUNT, SMOKE_EDGE_COUNT)
    );

    let graph = read_graph(&graph_path);
    assert_eq!(nodes(&graph).len(), SMOKE_NODE_COUNT);
    assert_eq!(edges(&graph).len(), SMOKE_EDGE_COUNT);
    unique_edge_triples(&graph);

    // 节点类型规模：planer + 3 评审 + archer = 5 agent_call；3 工作 + 3 解析 = 6 python_node。
    let type_counts = |node_type: &str| {
        nodes(&graph)
            .iter()
            .filter(|node| node["type"] == Value::from(node_type))
            .count()
    };
    assert_eq!(type_counts("agent_call"), 5, "agent_call 节点数");
    assert_eq!(type_counts("python_node"), 6, "python_node 节点数");
    assert_eq!(type_counts("end"), 0, "smoke 以 archer 叶节点收尾，无 end");

    // planer 扇出 3 个 item（同构：与 full 的 D→W→R→P→下游结构一致，只是规模缩小）。
    assert_design_node(&graph, "planer", &[]);
    for item in SMOKE_ITEMS {
        assert_item_pipeline(&graph, "planer", item);
    }

    // archer 叶节点：等 3 个 approved item 到齐，无出边。
    assert_design_node(&graph, "archer", SMOKE_ITEMS);
    assert!(
        !edges(&graph)
            .iter()
            .any(|edge| edge["from"] == Value::from("archer")),
        "archer 是叶节点，不得有出边"
    );
    let gated: Vec<&Value> = edges(&graph)
        .iter()
        .filter(|edge| edge.get("gate").is_some())
        .collect();
    assert_eq!(
        gated.len(),
        SMOKE_ITEMS.len(),
        "3 个 item 各一条 approved gate 投递边"
    );
    for edge in gated {
        assert!(
            is_approved_gate(edge),
            "gate 必须为 approved 不变量: {edge}"
        );
        assert_eq!(
            edge["to_node"],
            Value::from("archer"),
            "gate 投递目标应为 archer"
        );
    }
    for item in SMOKE_ITEMS {
        assert!(
            has_edge(&graph, &format!("planer_{item}_parse"), "archer", item),
            "planer.{item} 的 approved 投递边应指向 archer.{item}"
        );
    }
}
