//! W3a（AC-16 a–g）：人工评审 human lifecycle + `zgraph decide` 的 e2e。
//!
//! 唯一语义依据：`doc/decisions/DR-006-w3a-human-decide-freeze.md`（§2 触发模型 / §3 状态机 /
//! §4 决策应用与拒绝矩阵 / §6 trace v5 / §7 kernel+CLI 契约 / §8 兼容矩阵 / §10 AC-16），
//! 以及 spec 07 AC-16、spec 00/01/02/03/04/06 的 W3a 冻结段。
//!
//! 数据通路（禁止 mock）：
//!
//! ```text
//! tests/fixtures/graphs/human_gate/*.json（DSL 不变；builtin.human 保留 python_node 符号）
//!   → 真 `zgraph` 二进制（真 kernel + 真 Python worker 子进程 + 真 SQLite trace）
//!   → `zgraph decide` 写 human_decisions → run consumer 接续
//!   → TraceReader / 只读 SQL / trace / replay 断言
//! ```
//!
//! 本文件当前交付 **阶段 1（不依赖 cli decide 分支）**：
//!
//! 1. `human_gate/` 四个 fixture 的静态断言（真 `zgraph validate`：`builtin.human` 保留符号、
//!    R14 只做格式校验、hash 冻结、gate 形状冻结）；
//! 2. `gate.function="human"` **不是**内置 gate op（仍 `DSL_SYMBOL_NOT_FOUND`，DR-006 §8）；
//! 3. trace schema v4 seed：只读 `trace`/`replay` 对 v4 拒绝（exit 4）且**不迁移**；真 CLI `run`
//!    打开 v4 库 → v4→v5 迁移（备份/逐列数据保留/`human_decisions` 建表）且旧 run
//!    `trace --json` 与「迁移前 v4-era CLI 冻结 golden」逐字节一致。
//!
//! 阶段 2（cli decide/consumer 合并后补）：AC-16 a–d/f/g 的真链路用例；测试名与计数由
//! W3a-docs 回写 07。
//!
//! 稳定性约定：每用例独立 `tempfile::TempDir`/trace DB；等待用轮询 + 宽松上限，不做 sleep
//! 猜测；不新增 ignore、不放宽既有断言。

#![cfg(unix)]

#[path = "cli_support/mod.rs"]
mod cli_support;
#[path = "common/mod.rs"]
mod common;

use std::path::{Path, PathBuf};
use std::process::Output;

use rusqlite::Connection;
use serde_json::{json, Value};

use cli_support::{
    arg, assert_exit, json_stdout, python_program, read_only_connect, repo_root, run_cli,
    stderr_text, write_json,
};
use common::graphs_root;

// ---------------------------------------------------------------------------
// 常量与 fixture
// ---------------------------------------------------------------------------

/// `tests/fixtures/graphs/human_gate/` 下的四个 AC-16 资产。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct HumanFixture {
    /// 文件名。
    name: &'static str,
    /// 冻结 `graph_hash`（W3a 不改 DSL；hash 必须与 W2b 一致）。
    hash: &'static str,
}

/// AC-16 a：approve 真链路图。
const APPROVE: HumanFixture = HumanFixture {
    name: "approve.json",
    hash: "19eaac303e22378b",
};

/// AC-16 b 主变体：reject + W2a defer 返工。
const REWORK: HumanFixture = HumanFixture {
    name: "rework.json",
    hash: "f171f44afd5f5e0f",
};

/// AC-16 b 终止变体：rejected settle 边 → end。
const REJECT_END: HumanFixture = HumanFixture {
    name: "reject_end.json",
    hash: "b16a486e84cde959",
};

/// AC-16 d：决策等待期限 200ms。
const TIMEOUT: HumanFixture = HumanFixture {
    name: "timeout.json",
    hash: "16bf677686ae3bd7",
};

/// 全部 human 图 fixture。
const HUMAN_FIXTURES: [HumanFixture; 4] = [APPROVE, REWORK, REJECT_END, TIMEOUT];

/// 无决策 run 的图（`tests/fixtures/trace/seed_graph.json`，v3/v4 seed 共用）。
const SEED_GRAPH: &str = "seed_graph.json";

/// `schema_v4_seed.sql` 中固定化的旧 run id（与 v3 seed 同一归一化规则）。
const SEED_RUN_ID: &str = "0192f0c4-7a1b-7c3d-8e4f-000000000001";

/// seed 图入口输入（`start` 声明 `message`）。
const SEED_INPUTS: &str = r#"{"start":{"message":"seed"}}"#;

/// v4 seed 的冻结形态：1 run / 7 events / 2 instances / 4 transitions / 1 gate_result。
const V4_SEED_EVENTS: i64 = 7;

// ---------------------------------------------------------------------------
// 路径与 fixture 读取
// ---------------------------------------------------------------------------

/// `tests/fixtures/graphs/human_gate/<name>`。
fn human_gate_graph(name: &str) -> PathBuf {
    graphs_root().join("human_gate").join(name)
}

/// `tests/fixtures/trace/<name>`。
fn trace_fixture(name: &str) -> PathBuf {
    repo_root().join("tests/fixtures/trace").join(name)
}

/// 读图 JSON（形状断言用）。
fn read_graph(path: &Path) -> Value {
    let text = std::fs::read_to_string(path)
        .unwrap_or_else(|error| panic!("读取图 {} 失败: {error}", path.display()));
    serde_json::from_str(&text).unwrap_or_else(|error| panic!("图不是合法 JSON: {error}"))
}

/// 按节点 id 取节点定义。
fn node<'a>(graph: &'a Value, node_id: &str) -> &'a Value {
    graph["nodes"]
        .as_array()
        .expect("graph.nodes 数组")
        .iter()
        .find(|node| node["id"] == json!(node_id))
        .unwrap_or_else(|| panic!("node {node_id} 不存在"))
}

/// 按 `(from, to_node)` 取边定义。
fn edge<'a>(graph: &'a Value, from: &str, to_node: &str) -> &'a Value {
    graph["edges"]
        .as_array()
        .expect("graph.edges 数组")
        .iter()
        .find(|edge| edge["from"] == json!(from) && edge["to_node"] == json!(to_node))
        .unwrap_or_else(|| panic!("edge {from}->{to_node} 不存在"))
}

// ---------------------------------------------------------------------------
// SQL / CLI 辅助
// ---------------------------------------------------------------------------

/// `PRAGMA user_version`。
fn user_version(conn: &Connection) -> i64 {
    conn.query_row("PRAGMA user_version", [], |row| row.get(0))
        .expect("read user_version")
}

/// 目录下的迁移备份（`.bak.v{old}-{ts}`）。
fn backup_files(dir: &Path) -> Vec<PathBuf> {
    std::fs::read_dir(dir)
        .expect("read dir")
        .map(|entry| entry.expect("dir entry").path())
        .filter(|path| {
            path.file_name()
                .map(|name| name.to_string_lossy().contains(".bak.v"))
                .unwrap_or(false)
        })
        .collect()
}

/// 用 `schema_v4_seed.sql` 在临时目录建一个真实 v4 库（返回 `(db, old_run_id)`）。
fn seeded_v4_db(dir: &Path) -> (PathBuf, String) {
    let sql = std::fs::read_to_string(trace_fixture("schema_v4_seed.sql"))
        .expect("read schema_v4_seed.sql");
    let db = dir.join("seed_v4.db");
    let conn = Connection::open(&db).expect("create v4 seed db");
    conn.execute_batch(&sql)
        .expect("execute schema_v4_seed.sql");
    assert_eq!(user_version(&conn), 4, "seed 必须是 v4 库");
    let run_id: String = conn
        .query_row("SELECT run_id FROM runs", [], |row| row.get(0))
        .expect("seed 有一条 run");
    assert_eq!(run_id, SEED_RUN_ID, "seed run id 必须固定");
    assert_eq!(
        conn.query_row("SELECT COUNT(*) FROM events", [], |row| row
            .get::<_, i64>(0))
            .expect("events count"),
        V4_SEED_EVENTS,
        "v4 seed 事件数冻结"
    );
    (db, run_id)
}

/// 一张表里属于某个 run 的行（逐列 `Value` 快照，用于断言迁移不改历史行）。
fn run_rows(
    conn: &Connection,
    table: &str,
    order_by: &str,
    run_id: &str,
) -> Vec<Vec<rusqlite::types::Value>> {
    let sql = format!("SELECT * FROM {table} WHERE run_id = ?1 ORDER BY {order_by}");
    let mut statement = conn.prepare(&sql).expect("prepare rows query");
    let column_count = statement.column_count();
    let mut rows = statement.query([run_id]).expect("query rows");
    let mut out = Vec::new();
    while let Some(row) = rows.next().expect("row") {
        let mut values = Vec::with_capacity(column_count);
        for index in 0..column_count {
            values.push(
                row.get::<_, rusqlite::types::Value>(index)
                    .expect("column value"),
            );
        }
        out.push(values);
    }
    out
}

/// 旧 run 的逐列快照（迁移前后必须完全一致：`events` 不改写、两表重建逐列复制）。
fn old_run_snapshot(
    conn: &Connection,
    run_id: &str,
) -> Vec<(&'static str, Vec<Vec<rusqlite::types::Value>>)> {
    vec![
        ("runs", run_rows(conn, "runs", "run_id", run_id)),
        ("events", run_rows(conn, "events", "seq", run_id)),
        (
            "instances",
            run_rows(conn, "instances", "instance_id", run_id),
        ),
        ("transitions", run_rows(conn, "transitions", "seq", run_id)),
        (
            "gate_results",
            run_rows(conn, "gate_results", "seq", run_id),
        ),
    ]
}

/// NDJSON stdout → 事件数组。
fn ndjson_events(output: &Output) -> Vec<Value> {
    let stdout = String::from_utf8_lossy(&output.stdout);
    stdout
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str(line).expect("NDJSON line is JSON"))
        .collect()
}

/// 冻结的 v4-era `trace --json` golden（迁移前由 v4 CLI 生成，见 GENERATION.md §5）。
fn expected_v4_trace() -> Vec<Value> {
    let text = std::fs::read_to_string(trace_fixture("v4_expected_trace.json"))
        .expect("read v4_expected_trace.json");
    serde_json::from_str(&text).expect("expected trace is JSON array")
}

/// 真 `zgraph trace <run> --json`。
fn trace_json(db: &Path, run_id: &str) -> Output {
    run_cli(
        &[
            "trace".to_string(),
            run_id.to_string(),
            "--db".to_string(),
            arg(db),
            "--json".to_string(),
        ],
        &repo_root(),
    )
}

/// 真 `zgraph replay --mode=events --json`。
fn replay_events_json(db: &Path, run_id: &str) -> Output {
    run_cli(
        &[
            "replay".to_string(),
            "--run".to_string(),
            run_id.to_string(),
            "--db".to_string(),
            arg(db),
            "--mode".to_string(),
            "events".to_string(),
            "--json".to_string(),
        ],
        &repo_root(),
    )
}

/// 在 v4 库上跑 seed 图（真 CLI `run`，顺带触发 v4→v5 迁移）。
fn run_seed_graph(db: &Path, cwd: &Path) -> Output {
    run_cli(
        &[
            "run".to_string(),
            arg(&trace_fixture(SEED_GRAPH)),
            "--inputs".to_string(),
            SEED_INPUTS.to_string(),
            "--db".to_string(),
            arg(db),
            "--python".to_string(),
            python_program(),
            "--json".to_string(),
        ],
        cwd,
    )
}

// ---------------------------------------------------------------------------
// 1. human_gate fixtures：真 validate 通过 + hash/gate 形状冻结
// ---------------------------------------------------------------------------

/// AC-16 a/b/d 资产：四个 `builtin.human` 图必须通过真 `zgraph validate`（R1–R16 不变）。
///
/// `builtin.human` 是 `python_node` 保留符号：R14 只做 `module/function` 格式校验、
/// **不做 import/签名校验**（DR-006 §2.1 / 02 §2.5），因此 validate 无需 Python 资产。
#[test]
fn human_gate_fixtures_validate_with_frozen_hash_and_shape() {
    for fixture in HUMAN_FIXTURES {
        // 子目录：根目录新增 JSON 会破坏 dsl `every_fixture_is_referenced` golden 扫描。
        assert!(
            !graphs_root().join(fixture.name).exists(),
            "{} 不得放 graphs 根目录（dsl golden 扫描）",
            fixture.name
        );
        let path = human_gate_graph(fixture.name);
        assert!(path.is_file(), "missing fixture {}", path.display());
        let output = run_cli(
            &["validate".to_string(), arg(&path), "--json".to_string()],
            &repo_root(),
        );
        assert_exit(&output, 0, "human_gate 图 validate");
        let payload = json_stdout(&output);
        assert_eq!(payload["ok"], json!(true), "{}: {payload}", fixture.name);
        assert_eq!(payload["error_count"], json!(0), "{}", fixture.name);
        let hash = payload["graph_hash"]
            .as_str()
            .expect("合法图必须输出 graph_hash");
        assert_eq!(hash.len(), 16, "{} graph_hash 16 hex: {hash}", fixture.name);
        assert_eq!(hash, fixture.hash, "{} graph_hash 冻结", fixture.name);
    }
}

/// AC-16 a/b/d 图的 gate/retry/timeout 形状冻结（防止无意改图导致断言失真）。
#[test]
fn human_gate_fixtures_have_frozen_human_symbol_and_edges() {
    // approve：approved settle 边 → end。
    let approve = read_graph(&human_gate_graph(APPROVE.name));
    let human = node(&approve, "approve_human");
    assert_eq!(human["type"], json!("python_node"));
    assert_eq!(human["config"]["module"], json!("builtin"));
    assert_eq!(human["config"]["function"], json!("human"));
    let approved_edge = edge(&approve, "approve_human", "done");
    assert_eq!(approved_edge["gate"]["function"], json!("check_output"));
    assert_eq!(approved_edge["gate"]["params"]["path"], json!("decision"));
    assert_eq!(approved_edge["gate"]["params"]["equals"], json!("approved"));
    assert!(
        approved_edge.get("on_gate_false").is_none(),
        "approved → end 必须是 settle 边"
    );

    // rework：approved → end；rejected（defer）回边 → prep；max_attempts 预算 2。
    let rework = read_graph(&human_gate_graph(REWORK.name));
    assert_eq!(
        node(&rework, "rework_human")["config"]["function"],
        json!("human")
    );
    let ok_edge = edge(&rework, "rework_human", "done");
    assert_eq!(ok_edge["gate"]["params"]["equals"], json!("approved"));
    let rework_edge = edge(&rework, "rework_human", "prep");
    assert_eq!(rework_edge["to_input_port"], json!("feedback"));
    assert_eq!(
        rework_edge["gate"]["params"]["equals"],
        json!("rejected"),
        "reject 回边必须显式写 rejected（禁止恒 false defer 边）"
    );
    assert_eq!(
        rework_edge["on_gate_false"],
        json!("defer"),
        "返工回边必须 defer（W2a attempt-per-activation）"
    );
    assert_eq!(
        node(&rework, "prep")["config"]["retry"]["max_attempts"],
        json!(2),
        "prep 的 max_attempts 预算冻结"
    );
    // prep 的 feedback 端口必须 optional：首轮 human 决策前无 feedback 来源。
    let prep_ports = node(&rework, "prep")["inputs"]
        .as_array()
        .expect("prep.inputs");
    let feedback = prep_ports
        .iter()
        .find(|port| port["name"] == json!("feedback"))
        .expect("prep.feedback 端口");
    assert_eq!(feedback["required"], json!(false));

    // reject_end：rejected settle 边 → end（无 defer）。
    let reject_end = read_graph(&human_gate_graph(REJECT_END.name));
    let reject_edge = edge(&reject_end, "reject_human", "done");
    assert_eq!(reject_edge["gate"]["params"]["equals"], json!("rejected"));
    assert!(
        reject_edge.get("on_gate_false").is_none(),
        "reject_end 必须是 settle 边（终止变体，无 defer 重入）"
    );

    // timeout：human 节点声明决策等待期限 200ms。
    let timeout = read_graph(&human_gate_graph(TIMEOUT.name));
    assert_eq!(
        node(&timeout, "timeout_human")["config"]["timeout_ms"],
        json!(200)
    );
}

/// DR-006 §8：`builtin.human` 是**节点**符号，不是 gate op；`gate.function="human"` 仍报
/// `DSL_SYMBOL_NOT_FOUND`（不参与 gate canonical、也不因保留符号而放行）。
#[test]
fn gate_function_human_is_not_a_builtin_gate_op() {
    let dir = tempfile::tempdir().expect("tempdir");
    let graph = json!({
        "version": "1.0",
        "metadata": {"name": "gate-human-negative"},
        "entry_points": ["start"],
        "nodes": [
            {"id": "start", "type": "start", "inputs": [], "config": {}},
            {"id": "done", "type": "end",
             "inputs": [{"name": "r", "type": "any", "required": true}], "config": {}}
        ],
        "edges": [
            {"from": "start", "to_node": "done", "to_input_port": "r",
             "gate": {"function": "human"}}
        ]
    });
    let path = write_json(dir.path(), "gate_human.json", &graph);
    let output = run_cli(
        &["validate".to_string(), arg(&path), "--json".to_string()],
        dir.path(),
    );
    assert_exit(&output, 2, "gate.function=human 应视为非法 gate op");
    let payload = json_stdout(&output);
    assert_eq!(payload["ok"], json!(false));
    assert_eq!(payload["error_count"], json!(1));
    assert_eq!(payload["errors"][0]["code"], json!("DSL_SYMBOL_NOT_FOUND"));
    assert_eq!(payload["errors"][0]["location"]["edge_index"], json!(0));
}

// ---------------------------------------------------------------------------
// 2. trace v4 seed：只读拒绝（不迁移）
// ---------------------------------------------------------------------------

/// AC-16 e：只读 `trace`/`replay` 对 v4 库 exit 4 且**不迁移**（DR-006 §6.1 / 06 §6.7）。
#[test]
fn v4_seed_readonly_trace_and_replay_are_rejected_without_migration() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (db, run_id) = seeded_v4_db(dir.path());

    for (label, output) in [
        ("trace", trace_json(&db, &run_id)),
        ("replay", replay_events_json(&db, &run_id)),
    ] {
        assert_exit(&output, 4, &format!("只读 {label} 对 v4 库应 exit 4"));
        let stderr = stderr_text(&output);
        assert!(
            stderr.contains("unsupported schema version=4, expected 5"),
            "{label} stderr 应说明 v4 < 5 且不迁移: {stderr}"
        );
        assert!(
            String::from_utf8_lossy(&output.stdout).trim().is_empty(),
            "{label} --json 失败时 stdout 必须为空"
        );
    }

    // 未迁移：user_version 不变、无备份、数据行数不变。
    let conn = read_only_connect(&db);
    assert_eq!(user_version(&conn), 4, "只读拒绝不得迁移");
    let runs: i64 = conn
        .query_row("SELECT COUNT(*) FROM runs", [], |row| row.get(0))
        .expect("runs count");
    assert_eq!(runs, 1, "只读拒绝不得写库");
    let events: i64 = conn
        .query_row("SELECT COUNT(*) FROM events", [], |row| row.get(0))
        .expect("events count");
    assert_eq!(events, V4_SEED_EVENTS);
    assert!(
        backup_files(dir.path()).is_empty(),
        "只读拒绝不得生成迁移备份"
    );
}

// ---------------------------------------------------------------------------
// 3. trace v4→v5 迁移：备份/元数据/逐列数据保留/回放 golden
// ---------------------------------------------------------------------------

/// AC-16 e：真 CLI `run` 打开 v4 库 → 迁移 v5（备份恰一份 + 两表重建 + `human_decisions`），
/// 旧 run 历史行逐列保留、`trace --json` 与**迁移前** v4-era golden 逐字节一致。
#[test]
fn v4_seed_migrates_to_v5_preserving_history_and_replay() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (db, old_run_id) = seeded_v4_db(dir.path());
    let before = {
        let conn = read_only_connect(&db);
        old_run_snapshot(&conn, &old_run_id)
    };
    assert!(
        backup_files(dir.path()).is_empty(),
        "迁移前不应存在 .bak 备份"
    );

    // 真 CLI `run` 触发迁移并产生新 run（seed 图：无 human、无决策）。
    let output = run_seed_graph(&db, dir.path());
    assert_exit(&output, 0, "v4 seed 上触发迁移的 run");
    let payload = json_stdout(&output);
    assert_eq!(payload["status"], json!("Completed"));
    let new_run_id = payload["run_id"].as_str().expect("new run_id").to_string();
    assert_ne!(new_run_id, old_run_id, "迁移后的新 run 必须是新 id");

    let conn = read_only_connect(&db);
    assert_eq!(user_version(&conn), 5, "迁移后 user_version=5");
    let versions: Vec<i64> = {
        let mut statement = conn
            .prepare("SELECT version FROM schema_migrations ORDER BY version")
            .expect("prepare migrations");
        statement
            .query_map([], |row| row.get(0))
            .expect("query migrations")
            .map(|row| row.expect("version"))
            .collect()
    };
    assert_eq!(versions, vec![1, 2, 3, 4, 5], "schema_migrations 1..5");

    // 备份 = 迁移前 v4 快照。
    let backups = backup_files(dir.path());
    assert_eq!(backups.len(), 1, "v4→v5 恰一份备份: {backups:?}");
    let backup_name = backups[0]
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    assert!(
        backup_name.contains(".bak.v4-"),
        "备份文件名应含 .bak.v4-: {backup_name}"
    );
    let backup_conn = Connection::open(&backups[0]).expect("open backup");
    assert_eq!(user_version(&backup_conn), 4, "备份保持 v4");

    // v5 新表/索引存在；无决策 run 不产生决策行。
    let table_exists = |name: &str| -> bool {
        conn.query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name=?1",
            [name],
            |row| row.get::<_, i64>(0),
        )
        .expect("table count")
            == 1
    };
    assert!(table_exists("human_decisions"), "v5 必须建 human_decisions");
    let pending_index: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='index' AND name='idx_human_decisions_pending'",
            [],
            |row| row.get(0),
        )
        .expect("index count");
    assert_eq!(pending_index, 1, "human_decisions pending 索引存在");
    let decisions: i64 = conn
        .query_row("SELECT COUNT(*) FROM human_decisions", [], |row| row.get(0))
        .expect("decisions count");
    assert_eq!(decisions, 0, "无 human 图不得产生决策行");

    // 历史行逐列保留（events 不改写；instances/transitions 重建后逐列复制）。
    let after = old_run_snapshot(&conn, &old_run_id);
    assert_eq!(before, after, "迁移必须逐列保留旧 run 的历史数据");
    let old_status: String = conn
        .query_row(
            "SELECT status FROM runs WHERE run_id = ?1",
            [&old_run_id],
            |row| row.get(0),
        )
        .expect("old run status");
    assert_eq!(old_status, "Completed", "旧 run 状态不变");

    // 旧 run 回放：与 v4-era（迁移前）冻结 golden 一致；replay 与 trace 逐字节一致。
    let trace = trace_json(&db, &old_run_id);
    assert_exit(&trace, 0, "迁移后 trace 旧 run");
    assert_eq!(
        ndjson_events(&trace),
        expected_v4_trace(),
        "旧 run trace 必须与迁移前 v4 golden 一致（迁移不改写历史事件）"
    );
    let replay = replay_events_json(&db, &old_run_id);
    assert_exit(&replay, 0, "迁移后 replay 旧 run");
    assert_eq!(
        replay.stdout, trace.stdout,
        "L1 replay 与 trace 的 NDJSON 必须逐字节一致"
    );

    // 新 run（v5 无决策）同样：无 HumanDecision 行，replay == trace。
    let new_trace = trace_json(&db, &new_run_id);
    assert_exit(&new_trace, 0, "新 run trace");
    let new_events = ndjson_events(&new_trace);
    assert_eq!(new_events.len(), V4_SEED_EVENTS as usize, "新 run 事件数");
    assert!(
        new_events
            .iter()
            .all(|line| line["kind"] != json!("HumanDecision")),
        "无决策 run 的 trace --json 不得出现 HumanDecision 行"
    );
    let new_replay = replay_events_json(&db, &new_run_id);
    assert_exit(&new_replay, 0, "新 run replay");
    assert_eq!(
        new_replay.stdout, new_trace.stdout,
        "无决策 run 的 replay 与 trace 逐字节一致"
    );
}
