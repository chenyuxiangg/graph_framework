//! W1（v0.2 / M7）software_team SOP v1 e2e：codegen 参考图 validate + fake LLM 端到端。
//!
//! 语义基准：`doc/design/software-team-sop-v1.md` §3（D→W→R→P→下游）、§4.1（设计角色
//! 依赖矩阵）、§5.1–§5.3（P 契约 / approved gate / 失败语义）、§7（N 与 codegen 规则）、
//! §11（v1 落地验收）、附录 A/B；`doc/decisions/DR-004-v02-scope-and-waves.md` §3 W1。
//!
//! 数据通路（真链路，禁止 mock kernel / worker / trace / CLI）：
//!
//! ```text
//! tests/fixtures/graphs/software_team_v1/codegen.py（stdlib 纯函数）
//!   → 图 JSON（full = 60 节点/100 边；smoke = 11 节点/18 边）
//!   → 真 `zgraph` 二进制 `validate --assets-dir tests/fixtures/graphs/software_team_v1`
//!     （R1–R14 全跑，含 R14c agent 资产索引）
//!   → 真 `zgraph run --fake-llm=<script>`：fake LLM + 真 kernel + 真 Python worker
//!     （工作 agent stub `software_team_stub` / 解析节点 `workers.software_team.parse_verdict`）
//!     + 真 SQLite trace → TraceReader
//! ```
//!
//! 断言口径：
//!
//! - **规模**：full 恰 60 节点/100 边、smoke 恰 11 节点/18 边（§4.1 参考规模）；
//! - **确定性**：同 profile 两次生成字节级一致，`graph_hash` 相同（DR-004 W1 DoD）；
//! - **结构**：11 个设计角色 + 16 个工作/评审/解析节点 + `end`；每个 item 的 5 条内部边
//!   与 approved gate 投递边齐备（§7 规则，测试侧用 §4.1 矩阵独立复算）；
//! - **approved 链路**：smoke 的 planer → 3 工作 → 3 评审 → 3 解析 → archer 全 Completed；
//!   每 item 独立实例；`P.artifact` 原样透传 W 产物（不是评审摘要）；approved gate 放行；
//! - **失败路径**：评审 rework / 非法 JSON → `P` 抛错 → 实例 Crashed
//!   （`NODE_PYTHON_EXCEPTION`）；评审 `max_iterations` 耗尽 → `HARNESS_MAX_ITERATIONS`；
//!   两种都 → run Failed（不把 gate=false 的 Completed(null) 当失败）；
//! - **full 图符号/资产加载**（REV-065 S1）：32 个 `python_node` 的 `module.function` 在
//!   运行期 `PYTHONPATH` 下真 import 可取属性；13 个 agent 资产 JSON 可解析、id 与图引用一致；
//! - **手动 real-LLM smoke**（DR-004 §3 W1 / S1）：`#[ignore]`，**不进 CI**；显式
//!   `ZGRAPH_REAL_LLM=1` + `DEEPSEEK_API_KEY` 时用真模型跑同一 smoke 图，只验证 prompt
//!   约束（评审纯 JSON）与链路能到 Completed，**不能替代**上面的 fake 确定性断言。
//!
//! fake 剧本按**调用序**消费（05 §5.3.4 / harness `fake.rs`）：smoke 的执行顺序是确定的
//! （planer 唯一先执行 → 3 个评审并发但结论内容相同 → archer 等 3 个 approved 到齐），
//! 因此不用 sleep / 不断言评审之间的先后。
//!
//! 稳定性约定：codegen 用 `python3`/`ZGRAPH_PYTHON` 执行（与 e2e 其它 Python 断言同口径，
//! 不写 `__pycache__`）；每个用例独立 `tempfile::TempDir` 与独立 trace DB；CLI 自带完成
//! 语义（子进程退出即判定），不猜时序。

#![cfg(unix)]

#[path = "cli_support/mod.rs"]
mod cli_support;
#[path = "common/mod.rs"]
mod common;

use std::collections::{BTreeSet, HashSet};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::{json, Value};

use zgraph_trace::{
    GateResultRow, InstanceRow, InstanceStatus as TraceInstanceStatus, TraceReader,
};

use cli_support::{arg, assert_exit, json_stdout, repo_root, run_cli, stderr_text};

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
///
/// **单一来源**：`tests/fixtures/graphs/software_team_v1/codegen.py` 的 `FULL_ROLES`
/// 是生成图的权威数据；本常量是**独立手写副本**，只用设计稿 §4.1 作为 oracle 校验
/// codegen 产物（防 codegen 与设计稿同时漂移），不是运行期数据源。
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

/// smoke profile 的 3 个 planer 工作 item（**合成单阶段链路，不是 §4.1 依赖矩阵的子集**）。
const SMOKE_ITEMS: &[&str] = &["specification", "requirement_analysis", "iter_plan"];

/// `HARNESS_MAX_ITERATIONS` 失败路径的评审 agent 变体（`max_iterations=1` + `echo` 工具）。
const REVIEW_AGENT_EXHAUST: &str = "design_reviewer_exhaust";

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
    generate_graph_with(dir, profile, &[])
}

/// 执行 codegen 并追加额外参数（如 `--review-agent design_reviewer_exhaust`）。
fn generate_graph_with(dir: &Path, profile: &str, extra_args: &[&str]) -> PathBuf {
    let out = dir.join(format!("graph_{profile}.json"));
    let output = Command::new(common::python_program())
        .arg(codegen_path())
        .args(["--profile", profile])
        .arg("--out")
        .arg(&out)
        .args(extra_args)
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
        .find(|node| node["id"].as_str() == Some(node_id))
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
    edge["gate"]["function"].as_str() == Some("check_output")
        && edge["gate"]["params"]["path"].as_str() == Some("verdict")
        && edge["gate"]["params"]["equals"].as_str() == Some("approved")
}

/// 断言一个设计角色节点：entry 声明 `requirement`，其余每个依赖 item 一个端口。
fn assert_design_node(graph: &Value, role_id: &str, deps: &[&str]) {
    let node = node_by_id(graph, role_id);
    assert_eq!(node["type"].as_str(), Some("agent_call"), "{role_id} 类型");
    assert_eq!(
        node["config"]["agent_id"].as_str(),
        Some(role_id),
        "{role_id}.agent_id"
    );
    assert_eq!(node["trigger"].as_str(), Some("all"), "{role_id}.trigger");
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
            port["required"].as_bool(),
            Some(true),
            "{role_id} 端口必须 required"
        );
    }
    if deps.is_empty() {
        assert_eq!(input_names(node), vec!["requirement".to_string()]);
        assert_eq!(node["inputs"][0]["type"].as_str(), Some("string"));
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
    assert_eq!(work["type"].as_str(), Some("python_node"), "{base} 类型");
    assert_eq!(
        work["config"]["module"].as_str(),
        Some("software_team_stub")
    );
    let expected_function = format!("work_{role_id}_{item}");
    assert_eq!(
        work["config"]["function"].as_str(),
        Some(expected_function.as_str())
    );
    assert_eq!(input_names(work), vec!["task".to_string()]);

    let review = node_by_id(graph, &format!("{base}_review"));
    assert_eq!(review["type"].as_str(), Some("agent_call"));
    assert_eq!(
        review["config"]["agent_id"].as_str(),
        Some("design_reviewer")
    );
    assert_eq!(review["trigger"].as_str(), Some("all"));

    let parse = node_by_id(graph, &format!("{base}_parse"));
    assert_eq!(parse["type"].as_str(), Some("python_node"));
    assert_eq!(
        parse["config"]["module"].as_str(),
        Some("workers.software_team.parse_verdict")
    );
    assert_eq!(parse["config"]["function"].as_str(), Some("run"));
    assert_eq!(parse["trigger"].as_str(), Some("all"));

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

/// R14b 加载期只校验 `module.function` 格式、不 import；本辅助在**运行期**用同口径的
/// `PYTHONPATH`（仓库 `python/` + fixture `py/`）真正 import 并取属性，补齐 full 图的
/// 符号可加载性（REV-065 S1）。
fn assert_python_symbols_importable(symbols: &[(String, String)]) {
    let payload = serde_json::to_string(symbols).expect("serialize python symbols");
    let script = r#"
import importlib
import json
import sys

for module_name, function_name in json.loads(sys.argv[1]):
    module = importlib.import_module(module_name)
    attribute = getattr(module, function_name)
    assert callable(attribute), f"{module_name}.{function_name} is not callable"
print("ok")
"#;
    let python_path =
        std::env::join_paths([common::repo_root().join("python"), fixture_dir().join("py")])
            .expect("join PYTHONPATH");
    let output = Command::new(common::python_program())
        .args(["-c", script, &payload])
        .env("PYTHONPATH", python_path)
        .env(common::PYTHONDONTWRITEBYTECODE, "1")
        .current_dir(common::repo_root())
        .output()
        .expect("spawn python import check");
    assert!(
        output.status.success(),
        "full 图 python_node 符号导入失败（exit={:?}）\nstdout: {}\nstderr: {}",
        output.status.code(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
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
    assert_eq!(graph["version"].as_str(), Some("1.0"));
    assert_eq!(graph["entry_points"], serde_json::json!(["planer"]));
    assert_eq!(nodes(&graph).len(), FULL_NODE_COUNT);
    assert_eq!(edges(&graph).len(), FULL_EDGE_COUNT);
    unique_edge_triples(&graph);

    // 节点类型规模：11 设计角色 + 16 评审（agent_call）、16 工作 + 16 解析（python_node）、1 end。
    let type_counts = |node_type: &str| {
        nodes(&graph)
            .iter()
            .filter(|node| node["type"].as_str() == Some(node_type))
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
            target["type"].as_str(),
            Some("python_node"),
            "gate 边目标应为设计角色或 end"
        );
        assert!(!to_port.is_empty(), "gate 边必须有目标端口");
    }

    // `end` 等 release 的 3 个 item（附录 B：≥1 required、trigger=all）。
    let end = node_by_id(&graph, "end");
    assert_eq!(end["type"].as_str(), Some("end"));
    assert_eq!(end["trigger"].as_str(), Some("all"));
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
// full 图符号/资产加载（REV-065 S1；不 run full 图）
// ---------------------------------------------------------------------------

/// REV-065 S1：full 图的 32 个 `python_node` 符号可 import、12 个被引用 agent 资产可解析。
///
/// `zgraph validate` 的 R14b 只校验 `module.function` **格式**、不 import（02 §2.6.2），
/// 因此这里用运行期同口径 `PYTHONPATH` 真正 import 并取可调用属性；agent 资产则断言
/// `agents/*.json` 可解析、文件名与 `id` 一致、`role_prompt_file` 存在、图引用的
/// `agent_id` 全部命中（不跑 full 图）。
#[test]
fn full_graph_python_symbols_and_agent_assets_resolve() {
    let dir = tempfile::tempdir().expect("tempdir");
    let graph_path = generate_graph(dir.path(), PROFILE_FULL);
    let graph = read_graph(&graph_path);

    // 32 个 python_node：16 个工作 agent stub + 16 个解析节点（共享 parse_verdict.run）。
    let python_nodes: Vec<(String, String)> = nodes(&graph)
        .iter()
        .filter(|node| node["type"].as_str() == Some("python_node"))
        .map(|node| {
            (
                node["config"]["module"]
                    .as_str()
                    .expect("module")
                    .to_string(),
                node["config"]["function"]
                    .as_str()
                    .expect("function")
                    .to_string(),
            )
        })
        .collect();
    assert_eq!(python_nodes.len(), 32, "16 工作 agent + 16 解析节点");
    let distinct: BTreeSet<&(String, String)> = python_nodes.iter().collect();
    assert_eq!(
        distinct.len(),
        16 + 1,
        "16 个不同工作项函数 + 1 个共享解析函数"
    );
    assert_python_symbols_importable(&python_nodes);

    // agent 资产：13 个文件可解析（12 个被 full 图引用 + design_reviewer_exhaust 失败路径变体）。
    let agents_dir = fixture_dir().join("agents");
    let mut assets: BTreeSet<String> = BTreeSet::new();
    for entry in std::fs::read_dir(&agents_dir).expect("读取 agents 目录") {
        let path = entry.expect("agents 目录项").path();
        if path.extension().and_then(|extension| extension.to_str()) != Some("json") {
            continue;
        }
        let text = std::fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("读取资产 {} 失败: {error}", path.display()));
        let asset: Value = serde_json::from_str(&text)
            .unwrap_or_else(|error| panic!("资产 {} 不是合法 JSON: {error}", path.display()));
        let asset_id = asset["id"].as_str().expect("asset.id");
        assert_eq!(
            Some(asset_id),
            path.file_stem().and_then(|stem| stem.to_str()),
            "资产文件名应与 id 一致: {}",
            path.display()
        );
        let prompt_file = asset["role_prompt_file"]
            .as_str()
            .expect("asset.role_prompt_file");
        assert!(
            agents_dir.join(prompt_file).is_file(),
            "资产 {asset_id} 的 role_prompt_file 不存在: {prompt_file}"
        );
        assert!(
            assets.insert(asset_id.to_string()),
            "agent 资产 id 重复: {asset_id}"
        );
    }
    let referenced: BTreeSet<String> = nodes(&graph)
        .iter()
        .filter_map(|node| node["config"]["agent_id"].as_str())
        .map(str::to_string)
        .collect();
    assert_eq!(referenced.len(), 12, "11 个设计角色 + 共享 design_reviewer");
    for agent_id in &referenced {
        assert!(
            assets.contains(agent_id),
            "图引用的 agent 资产不存在: {agent_id}"
        );
    }
    assert_eq!(
        assets.len(),
        referenced.len() + 1,
        "assets 目录 = 12 个被引用资产 + design_reviewer_exhaust 变体"
    );
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
            .filter(|node| node["type"].as_str() == Some(node_type))
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
            .any(|edge| edge["from"].as_str() == Some("archer")),
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
            edge["to_node"].as_str(),
            Some("archer"),
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

// ---------------------------------------------------------------------------
// smoke run：fake LLM 端到端（approved 链路 + 失败路径）
// ---------------------------------------------------------------------------

/// smoke 入口输入（`planer.requirement` 由 run 注入；SOP 附录 B entry 语义）。
const SMOKE_INPUTS: &str = r#"{"planer":{"requirement":"构建一个最小软件团队 SOP 冒烟链路"}}"#;

/// fake 剧本 `fake_llm_approved.json` 的 `calls[0]`：planer 任务包（planer 唯一先执行）。
const PLANER_CONTENT: &str = "阶段任务包：需求分析 → 规格说明（smoke）";

/// fake 剧本 `calls[1..=3]`：3 个评审共用的 approved 结论（内容相同 → 并发次序无关）。
const REVIEW_CONTENT: &str =
    "{\"verdict\": \"approved\", \"comments\": \"设计覆盖了需求项 R1–R5\"}";

/// 评审结论里的 comments（解析节点 `P` 从 `review.content` 解析）。
const REVIEW_COMMENTS: &str = "设计覆盖了需求项 R1–R5";

/// fake 剧本 `calls[4]`：archer 叶节点任务包（3 个 approved item 到齐后才执行）。
const ARCHER_CONTENT: &str = "已收到 3 份 approved 设计产物，进入下一阶段（smoke 叶节点）";

/// approved 剧本（success 路径）。
const FAKE_APPROVED: &str = "fake_llm_approved.json";

/// rework 剧本（`verdict != approved` → `P` 抛错）。
const FAKE_REWORK: &str = "fake_llm_rework.json";

/// 非法 JSON 剧本（`review.content` 不可解析 → `P` 抛错）。
const FAKE_INVALID_JSON: &str = "fake_llm_invalid_json.json";

/// 预算耗尽剧本：`calls[1..]` 都是 `tool_calls`（评审 `max_iterations=1` → `HARNESS_MAX_ITERATIONS`）。
const FAKE_MAX_ITERATIONS: &str = "fake_llm_max_iterations.json";

/// `zgraph run` 的 smoke 公共参数（真 CLI + 真 kernel + 真 Python worker 子进程）。
///
/// `--python-path` 顺序：仓库 `python/`（`zgraph_worker` + `workers.software_team.parse_verdict`）
/// → fixture `py/`（工作 agent stub `software_team_stub`）；LLM provider 由调用方追加。
fn smoke_run_base_args(graph: &Path, db: &Path) -> Vec<String> {
    let assets = fixture_dir();
    vec![
        "run".to_string(),
        arg(graph),
        "--inputs".to_string(),
        SMOKE_INPUTS.to_string(),
        "--db".to_string(),
        arg(db),
        "--assets-dir".to_string(),
        arg(&assets),
        "--tools-dir".to_string(),
        arg(&assets.join("tools")),
        "--python".to_string(),
        common::python_program(),
        "--python-path".to_string(),
        arg(&common::repo_root().join("python")),
        "--python-path".to_string(),
        arg(&assets.join("py")),
        "--json".to_string(),
    ]
}

/// fake-LLM smoke 参数：在公共参数上追加 `--fake-llm=<脚本>`（按调用序消费）。
fn smoke_run_args(graph: &Path, db: &Path, fake: &str) -> Vec<String> {
    let mut args = smoke_run_base_args(graph, db);
    args.push(format!("--fake-llm={}", fixture_dir().join(fake).display()));
    args
}

/// real-LLM 手动 smoke 参数：在公共参数上追加 `--real`（真实 DeepSeek，需 API key）。
fn real_smoke_run_args(graph: &Path, db: &Path) -> Vec<String> {
    let mut args = smoke_run_base_args(graph, db);
    args.push("--real".to_string());
    args
}

/// 跑一次 smoke；返回 CLI 输出与 trace DB 路径。
fn run_smoke(dir: &Path, graph: &Path, fake: &str) -> (Output, PathBuf) {
    let db = dir.join(format!("{fake}.db"));
    let output = run_cli(&smoke_run_args(graph, &db, fake), dir);
    (output, db)
}

/// `--json` 输出里的 run_id。
fn payload_run_id(payload: &Value) -> String {
    payload["run_id"]
        .as_str()
        .expect("--json run_id")
        .to_string()
}

/// 断言 agent_call 的四字段整包（05 §5.9.2），返回 `content`。
fn assert_agent_package(value: &Value) -> String {
    let object = value.as_object().expect("agent output 应为对象");
    let mut keys: Vec<&str> = object.keys().map(String::as_str).collect();
    keys.sort_unstable();
    assert_eq!(
        keys,
        vec!["content", "iterations", "stop_reason", "usage"],
        "agent 输出必须恰好是四字段整包"
    );
    let content = value["content"]
        .as_str()
        .expect("content 字符串")
        .to_string();
    assert!(!content.is_empty(), "content 不得为空");
    assert!(value["iterations"].as_u64().is_some_and(|n| n >= 1));
    assert!(
        value["usage"]["total_tokens"]
            .as_u64()
            .is_some_and(|total| total > 0),
        "usage.total_tokens 必须 > 0"
    );
    content
}

/// 按节点 id 取**唯一** Completed 实例（per-item 实例断言：每个节点恰一条 trace 行）。
fn completed_instance(reader: &TraceReader, run_id: &str, node_id: &str) -> InstanceRow {
    let rows: Vec<InstanceRow> = reader
        .instances(run_id)
        .expect("query instances")
        .into_iter()
        .filter(|row| row.node_id == node_id)
        .collect();
    assert_eq!(rows.len(), 1, "节点 {node_id} 应恰一个实例");
    let row = rows.into_iter().next().expect("instance row");
    assert_eq!(
        row.status,
        TraceInstanceStatus::Completed,
        "节点 {node_id} 应 Completed"
    );
    assert_eq!(row.error_code, None, "节点 {node_id} 不应有错误码");
    row
}

/// 断言一个 item 的 W/R/P 链路 + 产物透传（SOP §3 / §11.2–§11.3）。
fn assert_approved_item_chain(
    reader: &TraceReader,
    run_id: &str,
    item: &str,
    planer_output: &Value,
) {
    let base = format!("planer_{item}");

    // W：工作 agent 收到 D 的整包任务包，产出该 item 的 artifact。
    let work_output = completed_instance(reader, run_id, &base)
        .output_json
        .clone()
        .expect("工作 agent output");
    assert_eq!(work_output["kind"], json!("software_team_work"));
    assert_eq!(work_output["role"], json!("planer"));
    assert_eq!(work_output["item"], json!(item));
    assert_eq!(
        work_output["task"], *planer_output,
        "W 必须收到 D 的整包任务包"
    );

    // R：评审 agent 消费 approved 剧本（内容相同，并发次序不影响断言）。
    let review_output = completed_instance(reader, run_id, &format!("{base}_review"))
        .output_json
        .clone()
        .expect("评审 agent output");
    let review_content = assert_agent_package(&review_output);
    assert_eq!(
        review_content, REVIEW_CONTENT,
        "3 个评审都消费 approved 剧本"
    );
    let review_json: Value =
        serde_json::from_str(&review_content).expect("评审 content 应为合法 JSON");
    assert_eq!(review_json["verdict"], json!("approved"));
    assert_eq!(review_json["comments"], json!(REVIEW_COMMENTS));

    // P：解析 verdict + **原样透传** W 的完整产物（不是评审摘要）。
    let parse_output = completed_instance(reader, run_id, &format!("{base}_parse"))
        .output_json
        .clone()
        .expect("解析节点 output");
    assert_eq!(parse_output["verdict"], json!("approved"));
    assert_eq!(parse_output["comments"], json!(REVIEW_COMMENTS));
    assert_eq!(
        parse_output["artifact"], work_output,
        "P 必须原样透传 W 的完整产物"
    );
    assert_ne!(
        parse_output["artifact"], review_output,
        "P.artifact 不得是评审输出（评审摘要）"
    );
    assert_eq!(
        parse_output["artifact"]["task"], *planer_output,
        "artifact 必须保留上游任务包"
    );
}

/// approved 链路：planer → 3 工作 → 3 评审 → 3 解析 → archer，产物透传 + approved gate。
#[test]
fn smoke_run_approved_pipeline_propagates_artifacts_and_enforces_gate() {
    let dir = tempfile::tempdir().expect("tempdir");
    let graph = generate_graph(dir.path(), PROFILE_SMOKE);
    let (output, db) = run_smoke(dir.path(), &graph, FAKE_APPROVED);
    assert_exit(&output, 0, "smoke approved run");
    let payload = json_stdout(&output);
    assert_eq!(payload["ok"], json!(true));
    assert_eq!(payload["status"], json!("Completed"));
    assert_eq!(
        payload["run_output"],
        Value::Null,
        "无 end 节点 → run_output=null（01 §1.9；smoke 以 archer 叶节点收尾）"
    );
    let run_id = payload_run_id(&payload);

    let reader = TraceReader::open_readonly(&db).expect("打开 trace 只读连接");
    common::assert_lifecycle_shape(&reader, &run_id);
    let run = common::run_row(&reader, &run_id);
    assert_eq!(run.status, zgraph_trace::RunStatus::Completed);
    assert_eq!(run.error_code, None);

    // per-item 实例：11 个节点各恰一条 Completed 实例（每 item 独立 W/R/P）。
    let instances = reader.instances(&run_id).expect("query instances");
    assert_eq!(instances.len(), SMOKE_NODE_COUNT, "11 个节点各一个实例");
    assert!(
        instances
            .iter()
            .all(|row| row.status == TraceInstanceStatus::Completed),
        "Completed run 的所有实例都应 Completed: {instances:?}"
    );

    // planer 是唯一先执行节点 → fake 剧本 calls[0] 必然分给它（调用序确定）。
    let planer_output = completed_instance(&reader, &run_id, "planer")
        .output_json
        .expect("planer output");
    assert_eq!(assert_agent_package(&planer_output), PLANER_CONTENT);

    for item in SMOKE_ITEMS {
        assert_approved_item_chain(&reader, &run_id, item, &planer_output);
    }

    // archer 叶节点：3 个 approved item 到齐才执行（trigger=all），输入是 P 的整包输出。
    let archer = completed_instance(&reader, &run_id, "archer");
    let archer_input = archer.input.as_ref().expect("archer input 快照");
    for item in SMOKE_ITEMS {
        assert_eq!(
            archer_input[item]["verdict"],
            json!("approved"),
            "{item} 必须是 approved 投递"
        );
        assert_eq!(
            archer_input[item]["artifact"]["kind"],
            json!("software_team_work"),
            "{item} 必须携带工作 agent 产物"
        );
    }
    let archer_output = archer.output_json.expect("archer output");
    assert_eq!(assert_agent_package(&archer_output), ARCHER_CONTENT);

    // approved gate 生效：3 条 P→archer 投递均 check_output 放行。
    let gates: Vec<GateResultRow> = reader.gate_results(&run_id).expect("query gate_results");
    let to_archer: Vec<&GateResultRow> = gates
        .iter()
        .filter(|gate| gate.to_node == "archer")
        .collect();
    assert_eq!(to_archer.len(), SMOKE_ITEMS.len(), "3 条 approved 投递");
    for gate in to_archer {
        assert!(gate.passed, "approved gate 应放行: {gate:?}");
        assert_eq!(gate.gate_fn.as_deref(), Some("check_output"));
        assert!(
            SMOKE_ITEMS.contains(&gate.to_port.as_str()),
            "目标端口应为 item 端口: {}",
            gate.to_port
        );
        assert!(gate.error.is_none(), "放行的 gate 不应有错误: {gate:?}");
    }
    assert!(
        gates.iter().all(|gate| gate.passed),
        "Completed run 不应有被拦截的投递"
    );
}

/// 评审失败路径（rework / 非法 JSON）的统一断言。
///
/// SOP §5.2：`P` 对非 approved / 非法 JSON **抛 Python 异常** → 实例 Crashed
/// （`NODE_PYTHON_EXCEPTION`）→ run Failed；**不允许**把 gate=false 的「run Completed
/// (null)」当成失败（§5.3）。
fn assert_parse_failure_path(dir: &Path, graph: &Path, fake: &str, exception_fragment: &str) {
    let (output, db) = run_smoke(dir, graph, fake);
    assert_exit(&output, 1, "评审失败 run 应 exit 1");
    let payload = json_stdout(&output);
    assert_eq!(payload["ok"], json!(false));
    assert_eq!(payload["status"], json!("Failed"));
    assert_eq!(payload["run_output"], Value::Null);
    assert_eq!(payload["error"]["code"], json!("NODE_PYTHON_EXCEPTION"));
    let run_id = payload_run_id(&payload);

    let reader = TraceReader::open_readonly(&db).expect("打开 trace 只读连接");
    common::assert_lifecycle_shape(&reader, &run_id);
    let run = common::run_row(&reader, &run_id);
    assert_eq!(run.status, zgraph_trace::RunStatus::Failed);
    assert_eq!(
        run.error_code.as_deref(),
        Some("NODE_PYTHON_EXCEPTION"),
        "runs.error_code"
    );

    let instances = reader.instances(&run_id).expect("query instances");
    let parse_rows: Vec<&InstanceRow> = instances
        .iter()
        .filter(|row| row.node_id.ends_with("_parse"))
        .collect();
    assert!(!parse_rows.is_empty(), "至少创建了一个解析节点实例");
    let crashed: Vec<&InstanceRow> = parse_rows
        .iter()
        .copied()
        .filter(|row| row.status == TraceInstanceStatus::Crashed)
        .collect();
    assert!(
        !crashed.is_empty(),
        "解析节点必须 Crashed（NODE_PYTHON_EXCEPTION），实际: {parse_rows:?}"
    );
    for row in &crashed {
        assert_eq!(
            row.error_code.as_deref(),
            Some("NODE_PYTHON_EXCEPTION"),
            "{} 实例错误码",
            row.node_id
        );
        let error = row.error.as_ref().expect("Crashed 实例应有 error JSON");
        assert_eq!(error["kind"], json!("PythonException"));
        let exception = error["exception"].as_str().expect("error.exception 摘要");
        assert!(
            exception.contains(exception_fragment),
            "异常摘要应含 {exception_fragment:?}: {exception}"
        );
    }
    assert!(
        parse_rows
            .iter()
            .all(|row| row.status != TraceInstanceStatus::Completed),
        "失败的解析节点不得 Completed"
    );

    // 下游 archer 不得执行；approved gate 不得放行任何投递（失败由 P 抛错承担）。
    assert!(
        instances
            .iter()
            .filter(|row| row.node_id == "archer")
            .all(|row| row.status != TraceInstanceStatus::Completed),
        "archer 不得 Completed"
    );
    let gates = reader.gate_results(&run_id).expect("query gate_results");
    // 普通投递（gate_fn=None）可能已放行（planer→W/R 等），但 approved gate（`check_output`）
    // 不得放行任何投递；解析未产出 verdict 时也不应有边投递给 archer。
    let approved_gates: Vec<&GateResultRow> = gates
        .iter()
        .filter(|gate| gate.gate_fn.as_deref() == Some("check_output"))
        .collect();
    assert!(
        approved_gates.iter().all(|gate| !gate.passed),
        "P 抛错时不应有放行的 approved gate: {approved_gates:?}"
    );
    assert!(
        gates.iter().all(|gate| gate.to_node != "archer"),
        "解析未产出 verdict 时不应有投递指向 archer: {gates:?}"
    );
}

/// 评审 `verdict != approved`（rework）→ `P` 抛 ValueError → NODE_PYTHON_EXCEPTION / run Failed。
#[test]
fn smoke_rework_verdict_crashes_parse_and_fails_run() {
    let dir = tempfile::tempdir().expect("tempdir");
    let graph = generate_graph(dir.path(), PROFILE_SMOKE);
    assert_parse_failure_path(dir.path(), &graph, FAKE_REWORK, "not approved");
}

/// 评审 `content` 非法 JSON → `P` 抛 ValueError → NODE_PYTHON_EXCEPTION / run Failed。
#[test]
fn smoke_invalid_review_json_crashes_parse_and_fails_run() {
    let dir = tempfile::tempdir().expect("tempdir");
    let graph = generate_graph(dir.path(), PROFILE_SMOKE);
    assert_parse_failure_path(dir.path(), &graph, FAKE_INVALID_JSON, "not valid JSON");
}

/// 评审 agent `max_iterations` 耗尽 → `HARNESS_MAX_ITERATIONS` → 实例 Crashed → run Failed。
///
/// 证据（REV-065 C1；DR-004 W1 验收要求的失败路径）：
///
/// - 评审 agent 用独立变体 `design_reviewer_exhaust`（`max_iterations=1`，`tools=["echo"]`），
///   不动既有 `design_reviewer` 资产；
/// - fake 剧本 `calls[0]` 给 planer 文本响应，`calls[1..]` 都是 `tool_calls`（`echo`）：
///   评审完成 1 次 LLM 调用 + 1 次工具执行后，下一轮预算检查命中上限
///   （05 §5.5.2：`max_iterations` 是 LLM 调用轮次预算）；
/// - 断言 run/CLI/实例/trace 四面一致，且 approved gate 未放行、archer 未执行。
#[test]
fn smoke_reviewer_max_iterations_crashes_review_and_fails_run() {
    let dir = tempfile::tempdir().expect("tempdir");
    let graph = generate_graph_with(
        dir.path(),
        PROFILE_SMOKE,
        &["--review-agent", REVIEW_AGENT_EXHAUST],
    );
    let (output, db) = run_smoke(dir.path(), &graph, FAKE_MAX_ITERATIONS);
    assert_exit(&output, 1, "max_iterations 耗尽 run 应 exit 1");
    let payload = json_stdout(&output);
    assert_eq!(payload["ok"], json!(false));
    assert_eq!(payload["status"], json!("Failed"));
    assert_eq!(payload["run_output"], Value::Null);
    assert_eq!(
        payload["error"]["code"],
        json!("HARNESS_MAX_ITERATIONS"),
        "CLI --json.error.code"
    );
    let run_id = payload_run_id(&payload);

    let reader = TraceReader::open_readonly(&db).expect("打开 trace 只读连接");
    common::assert_lifecycle_shape(&reader, &run_id);
    let run = common::run_row(&reader, &run_id);
    assert_eq!(run.status, zgraph_trace::RunStatus::Failed);
    assert_eq!(
        run.error_code.as_deref(),
        Some("HARNESS_MAX_ITERATIONS"),
        "runs.error_code"
    );

    // 3 个评审实例都创建；至少一个 Crashed（其余并发实例被 run Failed 取消）。
    let instances = reader.instances(&run_id).expect("query instances");
    let review_rows: Vec<&InstanceRow> = instances
        .iter()
        .filter(|row| row.node_id.ends_with("_review"))
        .collect();
    assert_eq!(review_rows.len(), SMOKE_ITEMS.len(), "3 个评审实例");
    let crashed: Vec<&InstanceRow> = review_rows
        .iter()
        .copied()
        .filter(|row| row.status == TraceInstanceStatus::Crashed)
        .collect();
    assert!(
        !crashed.is_empty(),
        "至少一个评审实例应 Crashed: {review_rows:?}"
    );
    for row in &crashed {
        assert_eq!(
            row.error_code.as_deref(),
            Some("HARNESS_MAX_ITERATIONS"),
            "{} 实例错误码",
            row.node_id
        );
        let error = row.error.as_ref().expect("Crashed 实例应有 error JSON");
        assert_eq!(error["code"], json!("HARNESS_MAX_ITERATIONS"));
        assert_eq!(error["kind"], json!("Harness"));
        assert_eq!(error["harness_kind"], json!("MaxIterations"));
        assert_eq!(
            error["detail"]["max_iterations"],
            json!(1),
            "生效上限来自 design_reviewer_exhaust"
        );
        assert_eq!(
            error["detail"]["pending_tool_calls"],
            json!(["echo"]),
            "证据：失败前已执行 tool loop（不是未调用就失败）"
        );
    }
    assert!(
        review_rows
            .iter()
            .all(|row| row.status != TraceInstanceStatus::Completed),
        "评审实例不得 Completed"
    );

    // 评审没有产出 verdict → 解析节点全部未执行；approved gate 未放行；archer 未执行。
    assert!(
        instances
            .iter()
            .filter(|row| row.node_id.ends_with("_parse"))
            .all(|row| row.status != TraceInstanceStatus::Completed),
        "解析节点不得 Completed"
    );
    assert!(
        instances
            .iter()
            .filter(|row| row.node_id == "archer")
            .all(|row| row.status != TraceInstanceStatus::Completed),
        "archer 不得 Completed"
    );
    let gates = reader.gate_results(&run_id).expect("query gate_results");
    assert!(
        gates
            .iter()
            .filter(|gate| gate.gate_fn.as_deref() == Some("check_output"))
            .all(|gate| !gate.passed),
        "approved gate 不得放行: {gates:?}"
    );
    assert!(
        gates.iter().all(|gate| gate.to_node != "archer"),
        "评审失败时不应有投递指向 archer: {gates:?}"
    );
}

// ---------------------------------------------------------------------------
// real-LLM 手动 smoke（`#[ignore]`，不进 CI；DR-004 §3 W1 / S1）
// ---------------------------------------------------------------------------

/// real-LLM smoke 的显式开关（与 `harness_ac08` 的 `ac08b_real_llm_manual` 同口径）。
const ZGRAPH_REAL_LLM_ENV: &str = "ZGRAPH_REAL_LLM";

/// 真实 DeepSeek 的 API key 环境变量（与 agent 资产 `model.api_key_env` 一致）。
const DEEPSEEK_API_KEY_ENV: &str = "DEEPSEEK_API_KEY";

/// **手动** real-LLM smoke：fake LLM 链路之外，用真实 DeepSeek 跑一遍 smoke 图。
///
/// 定位（DR-004 §3 W1 / S1）：
///
/// - **不进 CI**（`#[ignore]`），默认不执行；只有显式 `--ignored` 且设置
///   `ZGRAPH_REAL_LLM=1` 与 `DEEPSEEK_API_KEY` 时才真正发起网络调用，否则打印
///   `[skip]` 并返回；
/// - **不能替代** fake 链路的确定性断言：fake 用例负责编排/门控/产物透传/失败路径；
///   本用例只验证真实模型下 prompt 约束（评审 content 为纯 JSON）与整条 smoke 链路
///   能走到 `Completed`，用于人工验收 prompt 质量/模型行为；
/// - 只断言**模型无关**的结构事实：run `Completed`、11 个实例 `Completed`、3 个评审
///   content 可解析且 `verdict=approved`、`P.artifact` 原样透传 W 产物、approved gate
///   放行；不断言 comments 文案（模型输出不稳定）。
///
/// 手动执行：
///
/// ```bash
/// ZGRAPH_REAL_LLM=1 DEEPSEEK_API_KEY=<key> \
///   CARGO_HOME=<repo>/.cargo-home ZGRAPH_PYTHON=<repo>/.venv/bin/python \
///   cargo test -p zgraph-e2e --test software_team_v1 -- --ignored --nocapture real_llm_smoke
/// ```
#[test]
#[ignore = "manual: 需 ZGRAPH_REAL_LLM=1 与 DEEPSEEK_API_KEY（真实 DeepSeek 网络调用，不进 CI）"]
fn real_llm_smoke_manual() {
    if std::env::var(ZGRAPH_REAL_LLM_ENV).as_deref() != Ok("1") {
        eprintln!("[skip] 未设置 ZGRAPH_REAL_LLM=1，跳过真实模型 smoke");
        return;
    }
    if std::env::var(DEEPSEEK_API_KEY_ENV)
        .map(|key| key.trim().is_empty())
        .unwrap_or(true)
    {
        eprintln!("[skip] 缺少 DEEPSEEK_API_KEY，跳过真实模型 smoke");
        return;
    }

    let dir = tempfile::tempdir().expect("tempdir");
    let graph = generate_graph(dir.path(), PROFILE_SMOKE);
    let db = dir.path().join("real_llm_smoke.db");
    let output = run_cli(&real_smoke_run_args(&graph, &db), dir.path());
    assert!(
        output.status.success(),
        "real-LLM smoke 未 Completed（exit={:?}）\nstdout: {}\nstderr: {}",
        output.status.code(),
        String::from_utf8_lossy(&output.stdout),
        stderr_text(&output)
    );
    let payload = json_stdout(&output);
    assert_eq!(payload["ok"], json!(true));
    assert_eq!(payload["status"], json!("Completed"));
    let run_id = payload_run_id(&payload);

    let reader = TraceReader::open_readonly(&db).expect("打开 trace 只读连接");
    common::assert_lifecycle_shape(&reader, &run_id);
    let instances = reader.instances(&run_id).expect("query instances");
    assert_eq!(
        instances.len(),
        SMOKE_NODE_COUNT,
        "11 个节点各一个实例: {instances:?}"
    );
    assert!(
        instances
            .iter()
            .all(|row| row.status == TraceInstanceStatus::Completed),
        "real-LLM smoke 的实例应全 Completed: {instances:?}"
    );

    let planer_content = assert_agent_package(
        &completed_instance(&reader, &run_id, "planer")
            .output_json
            .expect("planer output"),
    );
    assert!(
        !planer_content.trim().is_empty(),
        "真实模型的设计角色 content 不应为空"
    );

    for item in SMOKE_ITEMS {
        let review_content = assert_agent_package(
            &completed_instance(&reader, &run_id, &format!("planer_{item}_review"))
                .output_json
                .expect("评审 output"),
        );
        let review_json: Value = serde_json::from_str(&review_content).unwrap_or_else(|error| {
            panic!("真实模型评审 content 必须是 JSON（design_reviewer prompt 约束）: {error}\n{review_content}")
        });
        assert_eq!(
            review_json["verdict"],
            json!("approved"),
            "真实模型评审未输出 approved（v1 无返工通道 → P 会抛错）: {review_content}"
        );

        let work_output = completed_instance(&reader, &run_id, &format!("planer_{item}"))
            .output_json
            .expect("工作 agent output");
        let parse_output = completed_instance(&reader, &run_id, &format!("planer_{item}_parse"))
            .output_json
            .expect("解析节点 output");
        assert_eq!(
            parse_output["artifact"], work_output,
            "真实模型评审链路下 P.artifact 仍须原样透传 W 产物"
        );
    }

    let gates = reader.gate_results(&run_id).expect("query gate_results");
    let approved_gates: Vec<&GateResultRow> = gates
        .iter()
        .filter(|gate| gate.gate_fn.as_deref() == Some("check_output"))
        .collect();
    assert_eq!(
        approved_gates.len(),
        SMOKE_ITEMS.len(),
        "3 条 approved gate 投递: {approved_gates:?}"
    );
    assert!(
        approved_gates.iter().all(|gate| gate.passed),
        "真实模型 smoke 的 approved gate 应全部放行: {approved_gates:?}"
    );

    let archer_content = assert_agent_package(
        &completed_instance(&reader, &run_id, "archer")
            .output_json
            .expect("archer output"),
    );
    assert!(
        !archer_content.trim().is_empty(),
        "archer 叶节点 content 不应为空"
    );
    eprintln!(
        "[ok] real-LLM smoke: run={run_id} Completed, {} 个 approved item 走到 archer",
        SMOKE_ITEMS.len()
    );
}

// ---------------------------------------------------------------------------
// W2a（SOP v1.1）：codegen `--mode approved|rework`（AC-14 c/e 的静态部分）
// ---------------------------------------------------------------------------

/// W2a rework profile 的 `--mode` 名称。
const MODE_REWORK: &str = "rework";

/// rework full（60 节点 / 116 边）与 smoke（11 / 21）规模（SOP v1.1 §7.2）。
const FULL_REWORK_EDGE_COUNT: usize = 116;
const SMOKE_REWORK_EDGE_COUNT: usize = 21;

/// rework W/R/P 的 `config.retry.max_attempts`（DR-005 §6.4）。
const REWORK_MAX_ATTEMPTS: i64 = 3;

/// W1 approved 三基线 `graph_hash`（DR-004 §10 / REV-065；approved 必须逐字节不变）。
const W1_FULL_HASH: &str = "3d8d735b1d5e8a1b";
const W1_SMOKE_HASH: &str = "df9341c04063b80a";
const W1_EXHAUST_HASH: &str = "a8e508deb9c6cf44";

/// rework 解析节点函数（P v2）。
const PARSE_FUNCTION_REWORK: &str = "parse_verdict_rework";

/// 生成一个 mode 的图（`--mode` 追加在 codegen 参数尾部）。
fn generate_mode_graph(dir: &Path, profile: &str, mode: &str) -> PathBuf {
    generate_graph_with(dir, profile, &["--mode", mode])
}

/// approved profile 三基线 hash 逐字节不变（W1 回归硬条件，AC-14 d）。
#[test]
fn approved_codegen_hashes_match_w1_baselines() {
    let dir = tempfile::tempdir().expect("tempdir");
    let cases: [(&str, &[&str], &str); 3] = [
        (PROFILE_FULL, &[], W1_FULL_HASH),
        (PROFILE_SMOKE, &[], W1_SMOKE_HASH),
        (
            PROFILE_SMOKE,
            &["--review-agent", REVIEW_AGENT_EXHAUST],
            W1_EXHAUST_HASH,
        ),
    ];
    for (index, (profile, extra, expected_hash)) in cases.iter().enumerate() {
        let case_dir = dir.path().join(format!("approved-{index}"));
        std::fs::create_dir_all(&case_dir).expect("case dir");
        let graph = generate_graph_with(&case_dir, profile, extra);
        let (_, _, hash) = parse_validate_summary(&validate_graph(&graph));
        assert_eq!(
            hash, *expected_hash,
            "approved {profile} {extra:?} 的 graph_hash 必须与 W1 基线一致"
        );
    }
}

/// rework 图的 `(role, item)` 清单（full = §4.1 全部 item；smoke = planer 的 3 个）。
fn rework_items(profile: &str) -> Vec<(&'static str, &'static str)> {
    if profile == PROFILE_FULL {
        let mut pairs = Vec::new();
        for (role, _deps, items) in DESIGN_MATRIX {
            for item in *items {
                pairs.push((*role, *item));
            }
        }
        pairs
    } else {
        SMOKE_ITEMS.iter().map(|item| ("planer", *item)).collect()
    }
}

/// 找一个边三元组；缺失即失败。
fn find_edge<'a>(graph: &'a Value, from: &str, to_node: &str, port: &str) -> &'a Value {
    edges(graph)
        .iter()
        .find(|edge| edge_triple(edge) == (from.to_string(), to_node.to_string(), port.to_string()))
        .unwrap_or_else(|| panic!("缺少边 {from}->{to_node}.{port}"))
}

/// 断言一个 item 的 rework 节点/边增量（SOP §7.2 a–e）。
fn assert_rework_item(graph: &Value, role: &str, item: &str) {
    let base = format!("{role}_{item}");
    let work = node_by_id(graph, &base);
    assert_eq!(
        input_names(work),
        vec!["task".to_string(), "feedback".to_string()],
        "{base} W 端口"
    );
    assert_eq!(
        work["inputs"][1]["required"],
        json!(false),
        "{base}.feedback 必须 required:false"
    );
    assert_eq!(
        work["config"]["retry"]["max_attempts"],
        json!(REWORK_MAX_ATTEMPTS)
    );
    let review = node_by_id(graph, &format!("{base}_review"));
    assert_eq!(
        review["config"]["retry"]["max_attempts"],
        json!(REWORK_MAX_ATTEMPTS)
    );
    let parse = node_by_id(graph, &format!("{base}_parse"));
    assert_eq!(parse["config"]["function"], PARSE_FUNCTION_REWORK);
    assert_eq!(
        parse["config"]["retry"]["max_attempts"],
        json!(REWORK_MAX_ATTEMPTS)
    );

    for (from, to_node, port) in [
        (base.clone(), format!("{base}_review"), "result"),
        (base.clone(), format!("{base}_parse"), "artifact"),
        (format!("{base}_review"), format!("{base}_parse"), "review"),
        (format!("{base}_parse"), base.clone(), "feedback"),
    ] {
        let edge = find_edge(graph, &from, &to_node, port);
        assert_eq!(
            edge["on_gate_false"], "defer",
            "{base} {from}->{to_node}.{port} 必须 defer"
        );
    }
    let feedback = find_edge(graph, &format!("{base}_parse"), &base, "feedback");
    assert_eq!(feedback["gate"]["function"], "check_output");
    assert_eq!(feedback["gate"]["params"]["path"], "verdict");
    assert_eq!(feedback["gate"]["params"]["equals"], "rework");

    // 环外 D→W.task / D→R.spec 保持缺省 settle（不写 on_gate_false）。
    for (to_node, port) in [(base.clone(), "task"), (format!("{base}_review"), "spec")] {
        let edge = find_edge(graph, role, &to_node, port);
        assert!(
            edge.get("on_gate_false").is_none(),
            "{base}: 环外边 {role}->{to_node}.{port} 不得 defer"
        );
    }
}

/// rework 全域 defer 策略：W/R/P 出边全 defer、其它源不得 defer、回边数=item 数。
fn assert_rework_policy(graph: &Value, profile: &str) {
    let pairs = rework_items(profile);
    let mut work_ids: BTreeSet<String> = BTreeSet::new();
    let mut review_ids: BTreeSet<String> = BTreeSet::new();
    let mut parse_ids: BTreeSet<String> = BTreeSet::new();
    for (role, item) in &pairs {
        let base = format!("{role}_{item}");
        work_ids.insert(base.clone());
        review_ids.insert(format!("{base}_review"));
        parse_ids.insert(format!("{base}_parse"));
        assert_rework_item(graph, role, item);
    }

    let mut defer_edges = 0usize;
    let mut feedback_edges = 0usize;
    for edge in edges(graph) {
        let source = edge["from"].as_str().expect("edge.from");
        let ring_source =
            work_ids.contains(source) || review_ids.contains(source) || parse_ids.contains(source);
        if ring_source {
            assert_eq!(
                edge["on_gate_false"], "defer",
                "环上源 {source} 的出边必须 defer: {edge}"
            );
            defer_edges += 1;
            if edge["to_input_port"] == "feedback" {
                feedback_edges += 1;
            }
        } else {
            assert!(
                edge.get("on_gate_false").is_none(),
                "环外源 {source} 的出边不得 defer: {edge}"
            );
        }
    }
    assert_eq!(
        feedback_edges,
        pairs.len(),
        "P→W.feedback 回边数必须等于 item 数"
    );
    let gated = edges(graph)
        .iter()
        .filter(|edge| edge.get("gate").is_some())
        .count();
    assert_eq!(
        defer_edges,
        gated + 3 * pairs.len(),
        "defer 边 = 全部 gate 边（含回边）+ 每条 item 的 3 条内部环边"
    );
}

/// rework codegen：规模/确定性/`zgraph validate`（含 R15）/defer 统一规则。
#[test]
fn rework_codegen_validates_with_expected_scale_and_full_defer_policy() {
    let dir = tempfile::tempdir().expect("tempdir");
    let cases: [(&str, usize, usize); 2] = [
        (PROFILE_FULL, FULL_NODE_COUNT, FULL_REWORK_EDGE_COUNT),
        (PROFILE_SMOKE, SMOKE_NODE_COUNT, SMOKE_REWORK_EDGE_COUNT),
    ];
    for (index, (profile, node_count, edge_count)) in cases.iter().enumerate() {
        let first_dir = dir.path().join(format!("rework-{index}-a"));
        let second_dir = dir.path().join(format!("rework-{index}-b"));
        std::fs::create_dir_all(&first_dir).expect("first dir");
        std::fs::create_dir_all(&second_dir).expect("second dir");

        let first = generate_mode_graph(&first_dir, profile, MODE_REWORK);
        let second = generate_mode_graph(&second_dir, profile, MODE_REWORK);
        assert_eq!(
            std::fs::read(&first).expect("read first"),
            std::fs::read(&second).expect("read second"),
            "rework {profile} 两次生成应字节级一致"
        );

        let (actual_nodes, actual_edges, first_hash) =
            parse_validate_summary(&validate_graph(&first));
        let (_, _, second_hash) = parse_validate_summary(&validate_graph(&second));
        assert_eq!(
            (actual_nodes, actual_edges),
            (*node_count, *edge_count),
            "rework {profile} 规模"
        );
        assert_eq!(first_hash, second_hash, "rework {profile} hash 确定性");

        let graph = read_graph(&first);
        assert_eq!(
            graph["metadata"]["name"],
            format!("software_team_v1_{profile}_rework"),
            "rework metadata 后缀"
        );
        assert_eq!(graph["entry_points"], serde_json::json!(["planer"]));
        assert_rework_policy(&graph, profile);
    }
}

// ---------------------------------------------------------------------------
// W2a（SOP v1.1）：rework → approved 双轮与超限（AC-14 c/e，runtime）
// ---------------------------------------------------------------------------

/// rework 双轮剧本（8 calls：planer → round1×3 rework → round2×3 approved → archer）。
const FAKE_REWORK_ROUNDTRIP: &str = "fake_llm_rework_roundtrip.json";

/// 恒定 rework 剧本（planer + 3 轮×3 rework；W attempt=3 超限失败）。
const FAKE_REWORK_EXHAUST: &str = "fake_llm_rework_exhaust.json";

/// round1 评审 comments（用于锁定「新一轮 P 消费新一轮 R」）。
const REWORK_ROUND1_COMMENTS: &str = "round1: 缺少错误码定义";

/// round2 评审 comments。
const REWORK_ROUND2_COMMENTS: &str = "round2: 已补齐错误码定义";

/// 某节点的全部实例（按 attempt 升序）。
fn instances_for(reader: &TraceReader, run_id: &str, node_id: &str) -> Vec<InstanceRow> {
    let mut rows: Vec<InstanceRow> = reader
        .instances(run_id)
        .expect("query instances")
        .into_iter()
        .filter(|row| row.node_id == node_id)
        .collect();
    rows.sort_by_key(|row| row.attempt);
    rows
}

/// 某节点实例的 attempt 列表。
fn attempts_of(reader: &TraceReader, run_id: &str, node_id: &str) -> Vec<u32> {
    instances_for(reader, run_id, node_id)
        .iter()
        .map(|row| row.attempt)
        .collect()
}

/// 解析节点输出的 `(verdict, comments, artifact)`。
fn parse_output_fields(instance: &InstanceRow) -> (String, String, Value) {
    let output = instance.output_json.as_ref().expect("parse output");
    (
        output["verdict"].as_str().expect("verdict").to_string(),
        output["comments"].as_str().expect("comments").to_string(),
        output["artifact"].clone(),
    )
}

/// 断言一条 gate_result 的 `(attempt, passed, deferred, verdict)`。
fn assert_gate(row: &GateResultRow, expected: (u32, bool, bool, Option<Value>), context: &str) {
    assert_eq!(
        (row.attempt, row.passed, row.deferred, row.verdict.clone()),
        expected,
        "{context}: {row:?}"
    );
}

/// 断言一个 item 的 rework 双轮链路（W/R/P attempts 0/1、gate 真实值、最终 artifact）。
fn assert_rework_chain(reader: &TraceReader, run_id: &str, item: &str) {
    let base = format!("planer_{item}");
    let work_id = base.clone();
    let review_id = format!("{base}_review");
    let parse_id = format!("{base}_parse");
    for node_id in [&work_id, &review_id, &parse_id] {
        assert_eq!(
            attempts_of(reader, run_id, node_id),
            vec![0, 1],
            "{node_id} attempts 0/1"
        );
    }
    let parses = instances_for(reader, run_id, &parse_id);
    assert_eq!(parses[0].status, TraceInstanceStatus::Completed);
    assert_eq!(parses[1].status, TraceInstanceStatus::Completed);
    let (v0, c0, _) = parse_output_fields(&parses[0]);
    assert_eq!(
        (v0.as_str(), c0.as_str()),
        ("rework", REWORK_ROUND1_COMMENTS)
    );
    let (v1, c1, a1) = parse_output_fields(&parses[1]);
    assert_eq!(
        (v1.as_str(), c1.as_str()),
        ("approved", REWORK_ROUND2_COMMENTS)
    );
    // P 第二轮必须消费第二轮 R 的 review（调用序锁定，防 stale-input）。
    let p1_input = parses[1].input.as_ref().expect("P attempt1 input");
    let review_content = p1_input["review"]["content"]
        .as_str()
        .expect("P attempt1 review.content");
    assert!(
        review_content.contains("\"approved\"") && review_content.contains(REWORK_ROUND2_COMMENTS),
        "P attempt1 必须消费新一轮 R 的 approved review，实际: {review_content}"
    );
    // 最终 artifact = W attempt1 的整包产物（内含 P attempt0 作为 feedback）。
    let works = instances_for(reader, run_id, &work_id);
    assert_eq!(
        works[1].output_json.as_ref().expect("W attempt1 output"),
        &a1,
        "P attempt1 的 artifact 必须来自 W attempt1"
    );
    assert_eq!(
        a1["feedback"]["verdict"],
        json!("rework"),
        "W attempt1 feedback 来自 P attempt0"
    );
    assert_eq!(
        a1["task"],
        works[0].output_json.as_ref().expect("W attempt0 output")["task"]
    );

    let gates: Vec<GateResultRow> = reader.gate_results(run_id).expect("gate_results");
    let forward: Vec<&GateResultRow> = gates
        .iter()
        .filter(|row| row.node_id == parse_id && row.to_node == "archer" && row.to_port == item)
        .collect();
    assert_eq!(forward.len(), 2, "{item} P→archer 两行");
    assert_gate(
        forward[0],
        (0, false, true, Some(json!("rework"))),
        "round1 approved 前进边 false 挂账",
    );
    assert_gate(
        forward[1],
        (1, true, false, Some(json!("approved"))),
        "round2 approved 前进边放行",
    );
    let feedback: Vec<&GateResultRow> = gates
        .iter()
        .filter(|row| row.node_id == parse_id && row.to_port == "feedback")
        .collect();
    assert_eq!(feedback.len(), 2, "{item} P→W.feedback 两行");
    assert_gate(
        feedback[0],
        (0, true, false, Some(json!("rework"))),
        "round1 rework 回边放行",
    );
    assert_gate(
        feedback[1],
        (1, false, true, Some(json!("approved"))),
        "round2 approved 回边 false 挂账（dormant）",
    );
    // 无 gate 的 settle 边：passed/deferred/verdict 不参与 defer 语义。
    for row in gates.iter().filter(|row| row.gate_fn.is_none()) {
        assert!(row.passed && !row.deferred && row.verdict.is_none());
    }
}

/// AC-14 c：SOP rework → approved 双轮；attempts 0/1、gate 真实值、下游最终 artifact。
#[test]
fn smoke_rework_roundtrip_completes_with_attempts_and_final_artifact() {
    let dir = tempfile::tempdir().expect("tempdir");
    let graph = generate_mode_graph(dir.path(), PROFILE_SMOKE, MODE_REWORK);
    let (output, db) = run_smoke(dir.path(), &graph, FAKE_REWORK_ROUNDTRIP);
    assert_exit(&output, 0, "smoke rework roundtrip run");
    let payload = json_stdout(&output);
    assert_eq!(payload["status"], json!("Completed"));
    assert_eq!(payload["run_output"], Value::Null, "smoke 无 end 节点");
    let run_id = payload_run_id(&payload);

    let reader = TraceReader::open_readonly(&db).expect("reader");
    common::assert_lifecycle_shape(&reader, &run_id);
    let run = common::run_row(&reader, &run_id);
    assert_eq!(run.status, zgraph_trace::RunStatus::Completed);
    assert_eq!(run.error_code, None);
    assert_eq!(
        attempts_of(&reader, &run_id, "planer"),
        vec![0],
        "planer 只执行一次"
    );

    for item in SMOKE_ITEMS {
        assert_rework_chain(&reader, &run_id, item);
    }

    // archer 拿最终 artifact（最新 W attempt 的整包，feedback.verdict=rework 证明是返工后 attempt）。
    let archer = instances_for(&reader, &run_id, "archer");
    assert_eq!(archer.len(), 1);
    assert_eq!(archer[0].status, TraceInstanceStatus::Completed);
    let archer_input = archer[0].input.as_ref().expect("archer input");
    for item in SMOKE_ITEMS {
        assert_eq!(archer_input[item]["verdict"], json!("approved"));
        assert_eq!(
            archer_input[item]["artifact"]["feedback"]["verdict"],
            json!("rework"),
            "{item}: 下游必须拿到返工后最终 W attempt 的产物"
        );
    }
}

/// AC-14 e：恒定 rework → W attempt=3 超限 → Failed NODE_INTERNAL；超限 GateResult 不入库。
#[test]
fn smoke_rework_exhaust_fails_after_max_attempts_without_overlimit_gate_result() {
    let dir = tempfile::tempdir().expect("tempdir");
    let graph = generate_mode_graph(dir.path(), PROFILE_SMOKE, MODE_REWORK);
    let (output, db) = run_smoke(dir.path(), &graph, FAKE_REWORK_EXHAUST);
    assert_exit(&output, 1, "超限 run 应 exit 1");
    let payload = json_stdout(&output);
    assert_eq!(payload["ok"], json!(false));
    assert_eq!(payload["status"], json!("Failed"));
    assert_eq!(payload["error"]["code"], json!("NODE_INTERNAL"));
    let run_id = payload_run_id(&payload);

    let reader = TraceReader::open_readonly(&db).expect("reader");
    let run = common::run_row(&reader, &run_id);
    assert_eq!(run.status, zgraph_trace::RunStatus::Failed);
    assert_eq!(run.error_code.as_deref(), Some("NODE_INTERNAL"));
    let message = run.error_message.as_deref().unwrap_or_default();
    assert!(
        message.contains("exceeded max_attempts (3)"),
        "超限 message: {message}"
    );

    let instances = reader.instances(&run_id).expect("instances");
    assert!(
        instances.iter().all(|row| !matches!(
            row.status,
            TraceInstanceStatus::Pending
                | TraceInstanceStatus::Ready
                | TraceInstanceStatus::Running
        )),
        "Failed run 不得残留非终态实例"
    );
    let crashed: Vec<&InstanceRow> = instances
        .iter()
        .filter(|row| row.status == TraceInstanceStatus::Crashed)
        .collect();
    assert_eq!(crashed.len(), 1, "恰一个 W 超限 Crashed: {crashed:?}");
    let crashed = crashed[0];
    let crashed_item = crashed
        .node_id
        .strip_prefix("planer_")
        .expect("crashed W 节点应形如 planer_<item>");
    assert!(SMOKE_ITEMS.contains(&crashed_item));
    assert_eq!(crashed.attempt, 3, "Crashed attempt = max_attempts");
    assert_eq!(crashed.error_code.as_deref(), Some("NODE_INTERNAL"));
    assert_eq!(
        crashed.error.as_ref().expect("error json")["kind"],
        json!("Internal")
    );

    // 并发下其它 item 可能只推进到第 2 轮（run 因首个超限失败而终止）；能确定的硬事实：
    // ① 任何节点的 attempt 连续从 0 开始（不跳号）；② 超限 victim 必然完成 3 轮 W/R/P；
    // ③ run 失败后无 Pending/Ready/Running（上面已断言）。
    let victim_base = crashed.node_id.clone();
    for item in SMOKE_ITEMS {
        let base = format!("planer_{item}");
        for node_id in [&base, &format!("{base}_review"), &format!("{base}_parse")] {
            let attempts = attempts_of(&reader, &run_id, node_id);
            let expected: Vec<u32> = (0..attempts.len() as u32).collect();
            assert_eq!(attempts, expected, "{node_id} attempt 必须从 0 连续");
        }
        let parses = instances_for(&reader, &run_id, &format!("{base}_parse"));
        assert!(
            parses
                .iter()
                .all(|row| row.status == TraceInstanceStatus::Completed
                    || row.status == TraceInstanceStatus::Cancelled),
            "{item} P 实例只应 Completed/Cancelled: {parses:?}"
        );
    }
    assert_eq!(
        attempts_of(&reader, &run_id, &victim_base),
        vec![0, 1, 2, 3],
        "超限 victim W：三轮 + Crashed attempt3"
    );
    assert_eq!(
        attempts_of(&reader, &run_id, &format!("{victim_base}_review")),
        vec![0, 1, 2],
        "victim R 完成三轮评审"
    );
    let victim_parses = instances_for(&reader, &run_id, &format!("{victim_base}_parse"));
    assert_eq!(
        victim_parses
            .iter()
            .map(|row| row.attempt)
            .collect::<Vec<_>>(),
        vec![0, 1, 2],
        "victim P 三轮（P2 产出 rework 后触发 W 超限）"
    );
    assert!(
        victim_parses
            .iter()
            .all(|row| row.status == TraceInstanceStatus::Completed),
        "victim P 在反馈求值前均 Completed: {victim_parses:?}"
    );

    // 超限那次 GateResult 不入库：crashed W 的 feedback 边只有前两轮（attempt 0/1）两行。
    let crash_parse_id = format!("{}_parse", crashed.node_id);
    let gates: Vec<GateResultRow> = reader.gate_results(&run_id).expect("gate_results");
    let feedback: Vec<&GateResultRow> = gates
        .iter()
        .filter(|row| row.node_id == crash_parse_id)
        .filter(|row| row.to_port == "feedback")
        .collect();
    assert_eq!(
        feedback.len(),
        2,
        "超限前只有前两轮 feedback GateResult: {feedback:?}"
    );
    assert_eq!(
        feedback.iter().map(|row| row.attempt).collect::<Vec<_>>(),
        vec![0, 1],
        "不得出现 attempt=2（超限那次）的 GateResult"
    );
}
