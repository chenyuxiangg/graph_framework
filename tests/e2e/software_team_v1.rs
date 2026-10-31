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
//!   （`NODE_PYTHON_EXCEPTION`）→ run Failed（不把 gate=false 的 Completed(null) 当失败）。
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

/// `zgraph run` 的 smoke 参数：真 CLI + fake LLM + 真 Python worker 子进程。
///
/// `--python-path` 顺序：仓库 `python/`（`zgraph_worker` + `workers.software_team.parse_verdict`）
/// → fixture `py/`（工作 agent stub `software_team_stub`）；fake 脚本按调用序消费。
fn smoke_run_args(graph: &Path, db: &Path, fake: &str) -> Vec<String> {
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
        format!("--fake-llm={}", assets.join(fake).display()),
        "--json".to_string(),
    ]
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
