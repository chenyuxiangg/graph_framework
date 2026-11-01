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
//! 本文件按 AC-16 组织：
//!
//! - **静态层**：`human_gate/` 四个 fixture 的真 `zgraph validate`（`builtin.human` 保留符号、
//!   R14 只做格式校验、hash/gate 形状冻结）；`gate.function="human"` **不是**内置 gate op；
//!   trace schema v4 seed（只读拒绝不迁移、v4→v5→v6 迁移逐列保留历史、迁移前 v4 golden 不变）；
//! - **AC-16 a**：approve 真链路（挂起 → trace 可见 `WaitingHuman` → `decide` → 自动 Completed）
//!   与 L2 `replay --mode=outputs`（human 实例 `recorded_outputs` 记录 + 重放不重新要求人工
//!   决策，DR-006 §2.2/§8）；
//! - **AC-16 b**：reject → 实例 Completed（不 Failed）→ W2a defer attempt+1 重入 → 二次
//!   WaitingHuman → approve；终止变体 rejected settle → end；
//! - **AC-16 c/d**：decide 预检顺序/全部退出码/`--json`；决策期限到期 TimedOut/NODE_TIMEOUT；
//! - **AC-16 e**：v3 seed → v5 链式迁移 L1 golden 不变、v4 seed 迁移与回放；
//! - **AC-16 f**：decide×run 并发（恰一成功/无 busy 泄漏）+ 库 API（无 SQLite/无 consumer）；
//! - **AC-16 g**：默认图 hash/validate/trace golden 不变、human 图 + `--db :memory:` 启动前拒绝。
//!
//! 稳定性约定：每用例独立 `tempfile::TempDir`/trace DB；等待用轮询 + 宽松上限，不做 sleep
//! 猜测；不新增 ignore、不放宽既有断言。

#![cfg(unix)]

#[path = "cli_support/mod.rs"]
mod cli_support;
#[path = "common/mod.rs"]
mod common;

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use rusqlite::{params, Connection, OpenFlags};
use serde_json::{json, Value};

use zgraph_kernel::{
    start_run, GateResultRecord, HumanDecision, InstanceId, InstanceSnapshot,
    InstanceStatus as KernelInstanceStatus, NodeId, RunControl, RunFinishedInfo, RunId,
    RunStartedInfo, RunStatus as KernelRunStatus, StreamingEvent, TraceSink, TransitionRecord,
};
use zgraph_trace::{
    GateResultRow, HumanDecisionRow, InstanceStatus as TraceInstanceStatus, RecordedOutputRow,
    RunStatus as TraceRunStatus, TraceReader,
};

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
    hash: "e72f1858e15778ce",
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
    query_rows(conn, &sql, run_id)
}

/// 执行带 `?1 = run_id` 的行查询并返回逐列 `Value` 快照。
fn query_rows(conn: &Connection, sql: &str, run_id: &str) -> Vec<Vec<rusqlite::types::Value>> {
    let mut statement = conn.prepare(sql).expect("prepare rows query");
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

/// v6 之前（W4a 前）的 `instances` 列快照：迁移对比只用历史列；v6 新增的
/// `item_index`/`item_key` 对旧行恒为 `0/NULL`（由 trace crate 迁移测试断言）。
fn instance_rows_legacy(conn: &Connection, run_id: &str) -> Vec<Vec<rusqlite::types::Value>> {
    query_rows(
        conn,
        "SELECT instance_id, run_id, node_id, node_type, attempt, status, input, \
         output_json, error, error_code, created_seq, started_at, finished_at, \
         finished_seq FROM instances WHERE run_id = ?1 ORDER BY instance_id",
        run_id,
    )
}

/// 旧 run 的逐列快照（迁移前后必须完全一致：`events` 不改写、两表重建逐列复制）。
fn old_run_snapshot(
    conn: &Connection,
    run_id: &str,
) -> Vec<(&'static str, Vec<Vec<rusqlite::types::Value>>)> {
    vec![
        ("runs", run_rows(conn, "runs", "run_id", run_id)),
        ("events", run_rows(conn, "events", "seq", run_id)),
        ("instances", instance_rows_legacy(conn, run_id)),
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

/// 在 v4 库上跑 seed 图（真 CLI `run`，顺带触发 v4→v5→v6 迁移）。
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
    assert_eq!(
        ok_edge["on_gate_false"],
        json!("defer"),
        "approved → end 必须是 defer 边：首轮 rejected 时不得 settle/skip end（否则次轮 approved 投递到终态）"
    );
    // prep → human 也是 defer 边：承载 W2a attempt-per-activation 的 human 重入。
    let prep_edge = edge(&rework, "prep", "rework_human");
    assert_eq!(
        prep_edge["on_gate_false"],
        json!("defer"),
        "prep → human 必须是 defer 边（human 重入需要 attempt+1）"
    );
    assert_eq!(prep_edge["gate"]["params"]["path"], json!("node_id"));
    assert_eq!(prep_edge["gate"]["params"]["equals"], json!("prep"));
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
            stderr.contains("unsupported schema version=4, expected 6"),
            "{label} stderr 应说明 v4 < 6 且不迁移: {stderr}"
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
// 3. trace v4→v5→v6 迁移：备份/元数据/逐列数据保留/回放 golden
// ---------------------------------------------------------------------------

/// AC-16 e：真 CLI `run` 打开 v4 库 → 迁移 v6（逐版本备份 + 两表重建 + `human_decisions`），
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
    assert_eq!(user_version(&conn), 6, "迁移后 user_version=6");
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
    assert_eq!(versions, vec![1, 2, 3, 4, 5, 6], "schema_migrations 1..6");

    // 备份 = 逐版本迁移前快照（v4→v5 与 v5→v6 各一份，W4b 机械同步）。
    let backups = backup_files(dir.path());
    assert_eq!(backups.len(), 2, "v4→v6 逐版本两份备份: {backups:?}");
    let backup = backups
        .iter()
        .find(|path| {
            path.file_name()
                .map(|name| name.to_string_lossy().contains(".bak.v4-"))
                .unwrap_or(false)
        })
        .unwrap_or_else(|| panic!("missing .bak.v4- backup: {backups:?}"));
    let backup_conn = Connection::open(backup).expect("open backup");
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

// ===========================================================================
// 阶段 2：AC-16 a–g 真链路（真 zgraph CLI + 真 Python worker + 真 SQLite）
// ===========================================================================

/// run/decide 子进程等待上限（真 worker 冷启动 + decision consumer 200ms 轮询余量）。
const CHILD_TIMEOUT: Duration = Duration::from_secs(30);

/// 挂起可见性轮询间隔（DR-006 N11：轮询 DB 可见后再 decide，不做 sleep 猜测）。
const POLL_INTERVAL: Duration = Duration::from_millis(10);

/// AC-16 f：同实例并发 decide 数（合同要求 2–10）。
const CONCURRENT_DECIDERS: usize = 5;

/// AC-16 f：并发用例重复轮次（合同要求重复 3 次无 flake）。
const CONCURRENCY_ROUNDS: usize = 3;

/// `zgraph run` 子进程守卫：`Drop` 时兜底 kill，避免断言 panic 后泄漏挂起的 run。
struct RunProcess {
    child: Option<Child>,
}

impl RunProcess {
    /// 以真 `zgraph run --json` 启动一个挂起等待决策的 run。
    fn spawn(cwd: &Path, db: &Path, graph: &Path, inputs: &str, record_full: bool) -> Self {
        let mut args = vec![
            "run".to_string(),
            arg(graph),
            "--inputs".to_string(),
            inputs.to_string(),
            "--db".to_string(),
            arg(db),
            "--python".to_string(),
            python_program(),
            "--python-path".to_string(),
            arg(&repo_root().join("python")),
        ];
        if record_full {
            args.push("--record=full".to_string());
        }
        args.push("--json".to_string());
        let child = cli_support::zgraph_command()
            .args(&args)
            .current_dir(cwd)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn zgraph run");
        Self { child: Some(child) }
    }

    /// run 进程 pid（挂起期间做「无 worker 子进程」断言）。
    fn pid(&self) -> u32 {
        self.child.as_ref().expect("run child alive").id()
    }

    /// 等待 run 退出并收集 stdout/stderr（超时 → SIGKILL 后 panic）。
    fn wait(mut self, context: &str) -> Output {
        let child = self.child.take().expect("run child");
        wait_child_output(child, context, CHILD_TIMEOUT)
    }
}

impl Drop for RunProcess {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

/// 等待子进程退出（线程阻塞读），超时则 SIGKILL 后 panic（不静默跳过）。
fn wait_child_output(child: Child, context: &str, timeout: Duration) -> Output {
    let pid = child.id() as i32;
    let (sender, receiver) = mpsc::channel();
    let thread = std::thread::spawn(move || {
        let _ = sender.send(child.wait_with_output());
    });
    match receiver.recv_timeout(timeout) {
        Ok(Ok(output)) => {
            let _ = thread.join();
            output
        }
        Ok(Err(error)) => panic!("{context}: 等待子进程失败: {error}"),
        Err(_) => {
            let _ = nix::sys::signal::kill(
                nix::unistd::Pid::from_raw(pid),
                nix::sys::signal::Signal::SIGKILL,
            );
            let _ = thread.join();
            panic!("{context}: 子进程未在 {timeout:?} 内退出（已 SIGKILL）");
        }
    }
}

/// `zgraph decide` 参数（`--json` 由调用方决定）。
fn decide_args(
    db: &Path,
    run_id: &str,
    instance_id: &str,
    decision: &str,
    approver: &str,
    reason: Option<&str>,
    json: bool,
) -> Vec<String> {
    let mut args = vec![
        "decide".to_string(),
        run_id.to_string(),
        "--instance".to_string(),
        instance_id.to_string(),
        "--decision".to_string(),
        decision.to_string(),
        "--approver".to_string(),
        approver.to_string(),
        "--db".to_string(),
        arg(db),
    ];
    if let Some(reason) = reason {
        args.push("--reason".to_string());
        args.push(reason.to_string());
    }
    if json {
        args.push("--json".to_string());
    }
    args
}

/// 同步执行 `zgraph decide`（写一行决策；不等待 run）。
fn decide_cli(
    cwd: &Path,
    db: &Path,
    run_id: &str,
    instance_id: &str,
    decision: &str,
    approver: &str,
    reason: Option<&str>,
) -> Output {
    // 阶段 2 用例统一断言 `--json` 形状（AC-16 c 的 CLI 契约）。
    run_cli(
        &decide_args(db, run_id, instance_id, decision, approver, reason, true),
        cwd,
    )
}

/// 启动一个并发 `zgraph decide` 子进程（AC-16 f）。
fn spawn_decide(
    cwd: &Path,
    db: &Path,
    run_id: &str,
    instance_id: &str,
    decision: &str,
    approver: &str,
) -> Child {
    cli_support::zgraph_command()
        .args(decide_args(
            db,
            run_id,
            instance_id,
            decision,
            approver,
            None,
            true,
        ))
        .current_dir(cwd)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn zgraph decide")
}

/// 断言 decide 失败：退出码 + `--json` 的 `{ok:false, code, message}` 形状。
fn assert_decide_failure(output: &Output, exit: i32, code: &str, context: &str) {
    assert_exit(output, exit, context);
    let payload = json_stdout(output);
    assert_eq!(payload["ok"], json!(false), "{context}");
    assert_eq!(payload["code"], json!(code), "{context}: {payload}");
    assert!(
        payload["message"]
            .as_str()
            .is_some_and(|message| !message.is_empty()),
        "{context}: message 非空"
    );
}

/// 只读打开 trace DB（运行中可安全轮询；DB 未就绪/暂不可读 → `None`）。
fn try_read_only(db: &Path) -> Option<Connection> {
    if !db.is_file() {
        return None;
    }
    Connection::open_with_flags(db, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .ok()
        .inspect(|conn| {
            let _ = conn.busy_timeout(Duration::from_millis(1_000));
        })
}

/// 轮询直到指定节点/attempt 的实例在 trace 中可见为 `WaitingHuman`。
///
/// 返回 `(run_id, instance_id)`；这是 DR-006 N11 的「先可见再 decide」口径
/// （decide 只会看到已提交的 trace 状态）。
fn poll_waiting_human(db: &Path, node_id: &str, attempt: u32) -> (String, String) {
    let deadline = Instant::now() + CHILD_TIMEOUT;
    loop {
        if let Some((run_id, _owner_pid)) = cli_support::running_run(db) {
            if let Some(conn) = try_read_only(db) {
                let found: Option<String> = conn
                    .query_row(
                        "SELECT instance_id FROM instances \
                         WHERE run_id = ?1 AND node_id = ?2 AND attempt = ?3 \
                           AND status = 'WaitingHuman' LIMIT 1",
                        params![run_id, node_id, attempt],
                        |row| row.get(0),
                    )
                    .ok();
                if let Some(instance_id) = found {
                    return (run_id, instance_id);
                }
            }
        }
        assert!(
            Instant::now() < deadline,
            "node {node_id} attempt {attempt} 未在 {CHILD_TIMEOUT:?} 内进入 WaitingHuman"
        );
        std::thread::sleep(POLL_INTERVAL);
    }
}

/// 断言 run 挂起期间没有 worker 子进程（`pgrep -P <run_pid>` 为空）。
///
/// human 节点不派发、不启动 worker（DR-006 §2.1）；挂起可见后其上游 worker 已收敛。
/// 用轮询容忍 worker 退出的瞬时窗口；命令不可用视为测试环境错误（不静默跳过）。
fn assert_no_worker_child(run_pid: u32) {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let output = Command::new("pgrep")
            .arg("-P")
            .arg(run_pid.to_string())
            .output()
            .unwrap_or_else(|error| {
                panic!("无法执行 pgrep（断言 run 挂起期间无 worker 子进程）: {error}")
            });
        if output.stdout.is_empty() {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "run 挂起期间不应有 worker 子进程: {}",
            String::from_utf8_lossy(&output.stdout)
        );
        std::thread::sleep(POLL_INTERVAL);
    }
}

/// 某 run/节点/attempt 的存储侧状态：`(status, output_json, error_code)`。
fn instance_state(
    db: &Path,
    run_id: &str,
    node_id: &str,
    attempt: u32,
) -> (String, Option<String>, Option<String>) {
    let conn = read_only_connect(db);
    conn.query_row(
        "SELECT status, output_json, error_code FROM instances \
         WHERE run_id = ?1 AND node_id = ?2 AND attempt = ?3",
        params![run_id, node_id, attempt],
        |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Option<String>>(1)?,
                row.get::<_, Option<String>>(2)?,
            ))
        },
    )
    .unwrap_or_else(|error| panic!("instance {node_id}/{attempt} 查询失败: {error}"))
}

/// 某 run 的全部实例 `(node_id, attempt, status)`（按 node/attempt 排序）。
fn instance_states(db: &Path, run_id: &str) -> Vec<(String, u32, String)> {
    let conn = read_only_connect(db);
    let mut statement = conn
        .prepare(
            "SELECT node_id, attempt, status FROM instances \
             WHERE run_id = ?1 ORDER BY node_id, attempt",
        )
        .expect("prepare instances");
    statement
        .query_map([run_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, u32>(1)?,
                row.get::<_, String>(2)?,
            ))
        })
        .expect("query instances")
        .map(|row| row.expect("instance row"))
        .collect()
}

/// `TraceReader::human_decisions` 包装（按 `(decided_at, decision_id)` 升序）。
fn decision_rows(reader: &TraceReader, run_id: &str) -> Vec<HumanDecisionRow> {
    reader
        .human_decisions(run_id)
        .expect("query human_decisions")
}

/// 某节点的 gate_results。
fn gate_rows(reader: &TraceReader, run_id: &str, node_id: &str) -> Vec<GateResultRow> {
    reader
        .gate_results(run_id)
        .expect("query gate_results")
        .into_iter()
        .filter(|gate| gate.node_id == node_id)
        .collect()
}

/// 某节点的迁移 `(from, to, reason)`（按 seq 升序）。
fn transition_pairs(
    reader: &TraceReader,
    run_id: &str,
    node_id: &str,
) -> Vec<(String, String, String)> {
    reader
        .transitions(run_id)
        .expect("query transitions")
        .into_iter()
        .filter(|row| row.node_id == node_id)
        .map(|row| {
            (
                row.from_status.as_str().to_string(),
                row.to_status.as_str().to_string(),
                row.reason.clone().unwrap_or_default(),
            )
        })
        .collect()
}

// ---------------------------------------------------------------------------
// AC-16 a：approve 真链路（人工停等 → decide → 自动接续）
// ---------------------------------------------------------------------------

/// AC-16 a：真 CLI run 挂起 → trace 可见 WaitingHuman 后 decide approved → 自动 Completed。
///
/// 断言链（DR-006 §2.2 / §3.2 / §4.1 / §6.4 / §8）：无 worker 子进程、无 Ready/Running
/// 迁移、`instances.input` 完整快照、实例 output `{decision,reason}` 非 NULL、approved
/// 出边 gate passed、`human_decisions.consumed_at` 非空、trace/`--json` 决策行、
/// `replay --mode=events --json` 与 `trace --json` stdout 逐字节一致。
#[test]
fn ac16a_approve_waits_then_decide_completes_with_trace_evidence() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db = dir.path().join("approve.db");
    let run = RunProcess::spawn(
        dir.path(),
        &db,
        &human_gate_graph(APPROVE.name),
        r#"{"start":{"message":"hello"}}"#,
        true,
    );

    // 1) 挂起可见：run Running、human 实例 WaitingHuman。
    let (run_id, instance_id) = poll_waiting_human(&db, "approve_human", 0);

    // 2) 挂起期间无 worker 子进程（human 不派发）。
    assert_no_worker_child(run.pid());

    // 3) 挂起期间 trace 事实：input 完整快照 = 该代上游产物；human 不经 Ready/Running。
    {
        let conn = read_only_connect(&db);
        let status: String = conn
            .query_row(
                "SELECT status FROM runs WHERE run_id = ?1",
                [&run_id],
                |row| row.get(0),
            )
            .expect("run status");
        assert_eq!(status, "Running", "等待人工时 run 保持 Running");

        let (human_status, input_json): (String, Option<String>) = conn
            .query_row(
                "SELECT status, input FROM instances WHERE instance_id = ?1",
                [&instance_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .expect("human instance");
        assert_eq!(human_status, "WaitingHuman");
        let input: Value = serde_json::from_str(
            input_json
                .as_deref()
                .expect("WaitingHuman 前必须写 instances.input"),
        )
        .expect("input snapshot JSON");
        let ports = input.as_object().expect("input 快照为对象");
        assert_eq!(
            ports.keys().cloned().collect::<Vec<String>>(),
            vec!["artifact".to_string()],
            "input 快照必须是全部声明端口"
        );
        let prep_output: String = conn
            .query_row(
                "SELECT output_json FROM instances WHERE run_id = ?1 AND node_id = 'prep'",
                [&run_id],
                |row| row.get(0),
            )
            .expect("prep output");
        assert_eq!(
            ports["artifact"],
            serde_json::from_str::<Value>(&prep_output).expect("prep output JSON"),
            "人工看到的是这一代上游产物"
        );
    }
    // 迁移：只有 Pending→WaitingHuman（无 Ready/Running；决策后 WaitingHuman→Completed）。
    {
        let reader = TraceReader::open_readonly(&db).expect("open trace");
        let transitions = transition_pairs(&reader, &run_id, "approve_human");
        assert_eq!(
            transitions,
            vec![(
                "Pending".to_string(),
                "WaitingHuman".to_string(),
                "human_waiting".to_string()
            )],
            "挂起瞬间 human 只有 Pending→WaitingHuman(human_waiting)"
        );
    }

    // 4) decide approved（trace 可见后）。
    let decide = decide_cli(
        dir.path(),
        &db,
        &run_id,
        &instance_id,
        "approved",
        "alice",
        Some("LGTM"),
    );
    assert_exit(&decide, 0, "decide approved");
    let decide_payload = json_stdout(&decide);
    assert_eq!(decide_payload["ok"], json!(true));
    assert_eq!(decide_payload["run_id"], json!(run_id));
    assert_eq!(decide_payload["instance_id"], json!(instance_id));
    assert_eq!(decide_payload["decision"], json!("approved"));
    let decision_id = decide_payload["decision_id"]
        .as_str()
        .expect("decision_id")
        .to_string();
    assert_eq!(decision_id.len(), 36, "decision_id 为 UUID 字符串");
    let decided_at = decide_payload["decided_at"]
        .as_str()
        .expect("decided_at")
        .to_string();
    assert!(decided_at.ends_with('Z'), "decided_at 为 UTC RFC3339 Z");

    // 5) run 自动接续 Completed。
    let output = run.wait("approve run");
    assert_exit(&output, 0, "decide 后 run 应 Completed 退出 0");
    let payload = json_stdout(&output);
    assert_eq!(payload["status"], json!("Completed"));
    assert_eq!(payload["run_id"], json!(run_id));
    assert_eq!(
        payload["run_output"],
        json!({"result": {"decision": "approved", "reason": "LGTM"}}),
        "approved 出边 → end 聚合人工 output"
    );
    assert_eq!(
        payload["killed_workers"],
        json!(0),
        "human 无 worker 可 kill"
    );
    let stderr = stderr_text(&output);
    assert!(
        stderr.contains("human decision applied: instance="),
        "stderr 实时决策行: {stderr}"
    );
    assert!(stderr.contains("decision=approved"), "stderr: {stderr}");

    // 6) 存储侧事实。
    let reader = TraceReader::open_readonly(&db).expect("open trace");
    let run_row = common::run_row(&reader, &run_id);
    assert_eq!(run_row.status, TraceRunStatus::Completed, "runs.status");
    assert_eq!(run_row.error_code, None, "Completed run error_code NULL");
    assert_eq!(
        run_row.record_mode.as_deref(),
        Some("full"),
        "--record=full 必须落库（L2 记录口径）"
    );
    common::assert_lifecycle_shape(&reader, &run_id);

    let human = common::instance(&reader, &run_id, "approve_human");
    assert_eq!(human.status, TraceInstanceStatus::Completed);
    assert_eq!(human.error_code, None);
    assert_eq!(
        human.output_json,
        Some(json!({"decision": "approved", "reason": "LGTM"})),
        "先写 output 再 transition：instances.output 非 NULL 且为 {{decision,reason}}"
    );
    assert!(
        human.input.is_some(),
        "WaitingHuman 前写入的 input 快照保留"
    );

    let human_transitions = transition_pairs(&reader, &run_id, "approve_human");
    assert_eq!(
        human_transitions,
        vec![
            (
                "Pending".to_string(),
                "WaitingHuman".to_string(),
                "human_waiting".to_string()
            ),
            (
                "WaitingHuman".to_string(),
                "Completed".to_string(),
                "human_decided".to_string()
            ),
        ],
        "迁移矩阵：Pending→WaitingHuman→Completed（不经 Ready/Running）"
    );

    let gates = gate_rows(&reader, &run_id, "approve_human");
    assert_eq!(gates.len(), 1, "approved 边恰一条 GateResult");
    assert_eq!(gates[0].to_node, "done");
    assert!(gates[0].passed, "approved 出边 gate passed");

    let decisions = decision_rows(&reader, &run_id);
    assert_eq!(decisions.len(), 1, "恰一行决策");
    let decision = &decisions[0];
    assert_eq!(decision.run_id, run_id);
    assert_eq!(decision.instance_id, instance_id);
    assert_eq!(decision.decision, "approved");
    assert_eq!(decision.approver, "alice");
    assert_eq!(decision.reason.as_deref(), Some("LGTM"));
    assert_eq!(decision.decision_id, decision_id);
    assert_eq!(decision.decided_at, decided_at);
    assert!(
        decision.consumed_at.is_some(),
        "run consumer 已消费：consumed_at 非空"
    );
    drop(reader);

    // 7) trace --json 追加 HumanDecision 行（事件行之后；不写入 events.kind）。
    let trace = trace_json(&db, &run_id);
    assert_exit(&trace, 0, "trace --json");
    let lines = ndjson_events(&trace);
    let decision_lines: Vec<&Value> = lines
        .iter()
        .filter(|line| line["kind"] == json!("HumanDecision"))
        .collect();
    assert_eq!(decision_lines.len(), 1, "恰一条 HumanDecision 行");
    let line = decision_lines[0];
    assert_eq!(line["run_id"], json!(run_id));
    assert_eq!(line["instance_id"], json!(instance_id));
    assert_eq!(line["decision"], json!("approved"));
    assert_eq!(line["approver"], json!("alice"));
    assert_eq!(line["reason"], json!("LGTM"));
    assert_eq!(line["decision_id"], json!(decision_id));
    assert_eq!(line["decided_at"], json!(decided_at));
    assert!(line["consumed_at"].is_string(), "consumed_at 非 null");
    assert_eq!(
        lines.last().map(|line| line["kind"].clone()),
        Some(json!("HumanDecision")),
        "决策行追加在事件行之后（DR-006 §6.4）"
    );
    assert_eq!(
        lines.len() - decision_lines.len(),
        common::events(
            &TraceReader::open_readonly(&db).expect("open trace"),
            &run_id
        )
        .len(),
        "既有事件行形状/数量保持不变（决策行只在输出层追加）"
    );

    // 人类可读 trace 决策摘要。
    let human_trace = run_cli(
        &[
            "trace".to_string(),
            run_id.clone(),
            "--db".to_string(),
            arg(&db),
        ],
        &repo_root(),
    );
    assert_exit(&human_trace, 0, "trace 人类输出");
    let human_stdout = String::from_utf8_lossy(&human_trace.stdout);
    assert!(
        human_stdout.contains("human_decisions: 1 (approved 1 / rejected 0; pending 0)"),
        "人类可读决策摘要: {human_stdout}"
    );
    assert!(human_stdout.contains("decision=approved"));

    // 8) L1 replay 与 trace 逐字节一致（含 HumanDecision 行）。
    let replay = replay_events_json(&db, &run_id);
    assert_exit(&replay, 0, "replay --mode=events");
    assert_eq!(
        replay.stdout, trace.stdout,
        "replay --mode=events --json 与 trace --json 的 stdout 必须逐字节一致"
    );
}

// ---------------------------------------------------------------------------
// AC-16 b：reject + W2a rework / reject→终止
// ---------------------------------------------------------------------------

/// AC-16 b 主变体：reject → 实例 Completed（不 Failed）→ defer 回边 attempt+1 重入
/// prep/human → 第二次 WaitingHuman → approve → Completed；attempt 0/1、deferred 行、
/// 无 RUN_DELIVERY_TO_TERMINAL/死锁；两行决策（不同 instance_id，均 consumed）。
#[test]
fn ac16b_reject_rework_reenters_then_approve_completes() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db = dir.path().join("rework.db");
    let run = RunProcess::spawn(
        dir.path(),
        &db,
        &human_gate_graph(REWORK.name),
        r#"{"start":{"message":"hello"}}"#,
        false,
    );

    // 第一轮：reject。
    let (run_id, instance0) = poll_waiting_human(&db, "rework_human", 0);
    let reject = decide_cli(
        dir.path(),
        &db,
        &run_id,
        &instance0,
        "rejected",
        "bob",
        Some("needs rework"),
    );
    assert_exit(&reject, 0, "第一轮 decide rejected");

    // reject 后实例正常 Completed（不隐式 Failed），并出现第二次等待（attempt=1）。
    let (_, instance1) = poll_waiting_human(&db, "rework_human", 1);
    assert_ne!(instance0, instance1, "重入必须产生新 instance_id");
    assert_eq!(
        instance_state(&db, &run_id, "rework_human", 0),
        (
            "Completed".to_string(),
            Some(r#"{"decision":"rejected","reason":"needs rework"}"#.to_string()),
            None
        ),
        "reject 决策为正常 Completed，output 为 {{decision,reason}}"
    );
    let (prep_status, _, _) = instance_state(&db, &run_id, "prep", 1);
    assert_eq!(prep_status, "Completed", "prep 已 attempt+1 重入并完成");

    // 第二轮：approve → run Completed。
    let approve = decide_cli(
        dir.path(),
        &db,
        &run_id,
        &instance1,
        "approved",
        "alice",
        Some("ok now"),
    );
    assert_exit(&approve, 0, "第二轮 decide approved");
    let output = run.wait("rework run");
    assert_exit(&output, 0, "rework run 应 Completed 退出 0");
    let payload = json_stdout(&output);
    assert_eq!(payload["status"], json!("Completed"));
    assert_eq!(
        payload["run_output"],
        json!({"result": {"decision": "approved", "reason": "ok now"}})
    );

    // 存储侧：attempt 0/1；deferred 行；无 delivery-to-terminal/死锁。
    let reader = TraceReader::open_readonly(&db).expect("open trace");
    let run_row = common::run_row(&reader, &run_id);
    assert_eq!(run_row.status, TraceRunStatus::Completed);
    assert_eq!(
        run_row.error_code, None,
        "rework 不得产生 RUN_DELIVERY_TO_TERMINAL/RUN_DEADLOCK"
    );
    assert_eq!(run_row.error_message, None);
    common::assert_lifecycle_shape(&reader, &run_id);

    let instances: Vec<(String, u32, String)> = instance_states(&db, &run_id);
    for (node, attempts) in [
        ("rework_human", vec![(0, "Completed"), (1, "Completed")]),
        ("prep", vec![(0, "Completed"), (1, "Completed")]),
    ] {
        let actual: Vec<(u32, String)> = instances
            .iter()
            .filter(|(id, _, _)| id == node)
            .map(|(_, attempt, status)| (*attempt, status.clone()))
            .collect();
        let expected: Vec<(u32, String)> = attempts
            .into_iter()
            .map(|(attempt, status)| (attempt, status.to_string()))
            .collect();
        assert_eq!(actual, expected, "{node} attempt 0/1 均 Completed");
    }
    assert_eq!(
        instance_state(&db, &run_id, "done", 0).0,
        "Completed",
        "第二次 approved 经 defer 边投递到 end（end 未被首轮 skip）"
    );

    // gate_results：rejected 边 attempt0 放行、attempt1 false+deferred；
    // approved 边 attempt0 false+deferred、attempt1 放行。
    let gates = gate_rows(&reader, &run_id, "rework_human");
    assert!(
        gates.iter().any(|gate| gate.deferred),
        "必须记录 deferred 行（W2a 返工承载）: {gates:?}"
    );
    let rejected_edges: Vec<&GateResultRow> =
        gates.iter().filter(|gate| gate.to_node == "prep").collect();
    assert_eq!(rejected_edges.len(), 2, "rejected 边两个 attempt 各一条");
    assert!(
        rejected_edges
            .iter()
            .any(|gate| gate.attempt == 0 && gate.passed && !gate.deferred),
        "attempt0 rejected 放行（正常投递回 prep）"
    );
    assert!(
        rejected_edges
            .iter()
            .any(|gate| gate.attempt == 1 && !gate.passed && gate.deferred),
        "attempt1 approved 时 rejected 边只挂账（deferred）"
    );
    let approved_edges: Vec<&GateResultRow> =
        gates.iter().filter(|gate| gate.to_node == "done").collect();
    assert_eq!(approved_edges.len(), 2, "approved 边两个 attempt 各一条");
    assert!(
        approved_edges
            .iter()
            .any(|gate| gate.attempt == 0 && !gate.passed && gate.deferred),
        "attempt0 rejected 时 approved 边只挂账（end 不 skip）"
    );
    assert!(
        approved_edges
            .iter()
            .any(|gate| gate.attempt == 1 && gate.passed && !gate.deferred),
        "attempt1 approved 边放行"
    );

    // 两行决策（不同 instance，均 consumed；按 decided_at 顺序 reject→approve）。
    let decisions = decision_rows(&reader, &run_id);
    assert_eq!(decisions.len(), 2, "rework 两轮各一行决策");
    assert_eq!(decisions[0].decision, "rejected");
    assert_eq!(decisions[1].decision, "approved");
    assert_eq!(decisions[0].instance_id, instance0);
    assert_eq!(decisions[1].instance_id, instance1);
    assert!(
        decisions.iter().all(|row| row.consumed_at.is_some()),
        "两行决策均被 consumer 消费"
    );
    assert!(
        instance_states(&db, &run_id)
            .iter()
            .all(|(_, _, status)| status != "Failed" && status != "TimedOut"),
        "reject/rework 不得产生 Failed/TimedOut 实例"
    );
}

/// AC-16 b 终止变体：rejected **settle** 边 → end；reject 后 run 正常 Completed、无重入。
#[test]
fn ac16b_reject_end_settles_to_end_without_reentry() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db = dir.path().join("reject_end.db");
    let run = RunProcess::spawn(
        dir.path(),
        &db,
        &human_gate_graph(REJECT_END.name),
        r#"{"start":{"message":"nope"}}"#,
        false,
    );
    let (run_id, instance_id) = poll_waiting_human(&db, "reject_human", 0);
    let reject = decide_cli(
        dir.path(),
        &db,
        &run_id,
        &instance_id,
        "rejected",
        "bob",
        Some("not approved"),
    );
    assert_exit(&reject, 0, "reject_end decide rejected");
    let output = run.wait("reject_end run");
    assert_exit(&output, 0, "reject_end run 应 Completed 退出 0");
    let payload = json_stdout(&output);
    assert_eq!(payload["status"], json!("Completed"));
    assert_eq!(
        payload["run_output"],
        json!({"result": {"decision": "rejected", "reason": "not approved"}}),
        "rejected settle 边 → end 聚合"
    );

    let reader = TraceReader::open_readonly(&db).expect("open trace");
    let run_row = common::run_row(&reader, &run_id);
    assert_eq!(run_row.status, TraceRunStatus::Completed);
    assert_eq!(run_row.error_code, None);
    // 无 attempt 重入：所有实例 attempt 均为 0。
    assert!(
        instance_states(&db, &run_id)
            .iter()
            .all(|(_, attempt, _)| *attempt == 0),
        "终止变体不得 attempt 重入"
    );
    let gates = gate_rows(&reader, &run_id, "reject_human");
    assert_eq!(gates.len(), 1);
    assert_eq!(gates[0].to_node, "done");
    assert!(gates[0].passed, "rejected settle 边放行");
    assert!(!gates[0].deferred, "settle 边不是 defer");
    let decisions = decision_rows(&reader, &run_id);
    assert_eq!(decisions.len(), 1);
    assert_eq!(decisions[0].decision, "rejected");
    assert!(decisions[0].consumed_at.is_some());
}

// ---------------------------------------------------------------------------
// AC-16 c：冲突 / 幂等 / 非 waiting / 退出码 + --json（预检顺序冻结）
// ---------------------------------------------------------------------------

/// AC-16 c：decide 预检顺序（DR-006 §4.5）与全部退出码/`--json` 形状。
///
/// 顺序：① 已有决策行（exit 5 `HUMAN_DECISION_EXISTS`）→ ② run 非活跃（exit 1
/// `HUMAN_RUN_NOT_ACTIVE`）→ ③ instance 不存在（exit 1 `HUMAN_INSTANCE_NOT_FOUND`）→
/// ④ instance 非 WaitingHuman（exit 5 `HUMAN_INSTANCE_NOT_WAITING`）；参数 exit 2；
/// DB 不存在/版本不符 exit 4（不创建空库、不迁移）。
#[test]
fn ac16c_conflict_precheck_order_and_exit_codes() {
    let dir = tempfile::tempdir().expect("tempdir");

    // 0) 用法错误 → exit 2（stderr 报错；`--json` 也不写 stdout 契约）。
    let usage_cases: [(&str, &[&str]); 3] = [
        (
            "缺 --approver",
            &[
                "decide",
                "run-x",
                "--instance",
                "i",
                "--decision",
                "approved",
            ],
        ),
        (
            "非法 --decision",
            &[
                "decide",
                "run-x",
                "--instance",
                "i",
                "--decision",
                "maybe",
                "--approver",
                "a",
            ],
        ),
        (
            "空 approver",
            &[
                "decide",
                "run-x",
                "--instance",
                "i",
                "--decision",
                "approved",
                "--approver",
                "   ",
            ],
        ),
    ];
    for (label, raw) in usage_cases {
        let mut args: Vec<String> = raw.iter().map(|value| value.to_string()).collect();
        args.push("--db".to_string());
        args.push(arg(&dir.path().join("usage.db")));
        args.push("--json".to_string());
        let output = run_cli(&args, dir.path());
        assert_exit(&output, 2, label);
        assert!(
            String::from_utf8_lossy(&output.stdout).trim().is_empty(),
            "{label}: 用法错误不写 stdout"
        );
        assert!(!stderr_text(&output).is_empty(), "{label}: stderr 必有说明");
    }

    // 1) DB 不存在 → exit 4 `HUMAN_DB_ERROR`，且**不创建空库**（N20）。
    let missing = dir.path().join("missing.db");
    let missing_out = decide_cli(
        dir.path(),
        &missing,
        "run-x",
        "inst-x",
        "approved",
        "alice",
        None,
    );
    assert_decide_failure(&missing_out, 4, "HUMAN_DB_ERROR", "DB 不存在");
    assert!(!missing.exists(), "decide 不得创建空库");

    // 2) v<5 库 → exit 4 `HUMAN_DB_ERROR`（不迁移；提示先由 run 迁移）。
    let (v4_db, _seed_run) = seeded_v4_db(dir.path());
    let v4_out = decide_cli(
        dir.path(),
        &v4_db,
        SEED_RUN_ID,
        "inst-x",
        "approved",
        "alice",
        None,
    );
    assert_decide_failure(&v4_out, 4, "HUMAN_DB_ERROR", "v4 库只读拒绝");
    let v4_payload = json_stdout(&v4_out);
    let v4_message = v4_payload["message"].as_str().expect("message");
    assert!(
        v4_message.contains("unsupported schema version=4, expected 6"),
        "message: {v4_message}"
    );
    assert!(
        v4_message.contains("run 'zgraph run' to migrate"),
        "应提示先由 run 迁移: {v4_message}"
    );
    {
        let conn = read_only_connect(&v4_db);
        assert_eq!(user_version(&conn), 4, "decide 不得迁移 v4 库");
    }

    // 3) 真 run 挂起，覆盖状态类预检。
    let db = dir.path().join("decide.db");
    let run = RunProcess::spawn(
        dir.path(),
        &db,
        &human_gate_graph(APPROVE.name),
        r#"{"start":{"message":"hello"}}"#,
        false,
    );
    let (run_id, instance_id) = poll_waiting_human(&db, "approve_human", 0);
    let start_instance: String = {
        let conn = read_only_connect(&db);
        conn.query_row(
            "SELECT instance_id FROM instances WHERE run_id = ?1 AND node_id = 'start'",
            [&run_id],
            |row| row.get(0),
        )
        .expect("start instance")
    };

    // 3a) instance 不存在（run 活跃）→ exit 1 `HUMAN_INSTANCE_NOT_FOUND`。
    let not_found = decide_cli(
        dir.path(),
        &db,
        &run_id,
        "0192f0c4-dead-7c3d-8e4f-00000000dead",
        "approved",
        "alice",
        None,
    );
    assert_decide_failure(&not_found, 1, "HUMAN_INSTANCE_NOT_FOUND", "instance 不存在");

    // 3b) 非 WaitingHuman（start 实例已 Completed 且无决策行）→ exit 5。
    let not_waiting = decide_cli(
        dir.path(),
        &db,
        &run_id,
        &start_instance,
        "approved",
        "alice",
        None,
    );
    assert_decide_failure(
        &not_waiting,
        5,
        "HUMAN_INSTANCE_NOT_WAITING",
        "instance 非 WaitingHuman",
    );

    // 4) 首次 decide → exit 0；同实例二次/三次（含不同 decision）恒 exit 5 `EXISTS`。
    let first = decide_cli(
        dir.path(),
        &db,
        &run_id,
        &instance_id,
        "approved",
        "alice",
        Some("first"),
    );
    assert_exit(&first, 0, "首次 decide");
    let first_payload = json_stdout(&first);
    assert_eq!(first_payload["ok"], json!(true));
    assert_eq!(first_payload["run_id"], json!(run_id));
    assert_eq!(first_payload["instance_id"], json!(instance_id));
    assert_eq!(first_payload["decision"], json!("approved"));
    assert!(first_payload["decision_id"].is_string());
    assert!(first_payload["decided_at"].is_string());
    for (decision, label) in [("approved", "二次同值"), ("rejected", "二次异值")] {
        let duplicate = decide_cli(
            dir.path(),
            &db,
            &run_id,
            &instance_id,
            decision,
            "mallory",
            None,
        );
        assert_decide_failure(&duplicate, 5, "HUMAN_DECISION_EXISTS", label);
    }

    // 5) run 终态后：已有决策行仍 exit 5 先于状态判断；无决策行的实例 → exit 1。
    let output = run.wait("decide conflict run");
    assert_exit(&output, 0, "decide 后 run Completed");
    let after_exit = decide_cli(
        dir.path(),
        &db,
        &run_id,
        &instance_id,
        "approved",
        "alice",
        None,
    );
    assert_decide_failure(
        &after_exit,
        5,
        "HUMAN_DECISION_EXISTS",
        "run Completed 后二次 decide（存在性优先）",
    );
    let finished_run = decide_cli(
        dir.path(),
        &db,
        &run_id,
        &start_instance,
        "approved",
        "alice",
        None,
    );
    assert_decide_failure(
        &finished_run,
        1,
        "HUMAN_RUN_NOT_ACTIVE",
        "run 已终态、instance 无决策行",
    );
    let unknown_run = decide_cli(
        dir.path(),
        &db,
        "0192f0c4-dead-7c3d-8e4f-000000000000",
        &instance_id,
        "approved",
        "alice",
        None,
    );
    assert_decide_failure(&unknown_run, 1, "HUMAN_RUN_NOT_ACTIVE", "run 不存在");

    // 6) 恰一行决策（重复未插入）。
    let reader = TraceReader::open_readonly(&db).expect("open trace");
    assert_eq!(
        decision_rows(&reader, &run_id).len(),
        1,
        "重复/冲突 decide 不得产生第二行"
    );
    let run_row = common::run_row(&reader, &run_id);
    assert_eq!(run_row.status, TraceRunStatus::Completed);
    assert_eq!(run_row.error_code, None, "冲突 decide 不得改 run 结果");
}

// ---------------------------------------------------------------------------
// AC-16 d：决策期限超时
// ---------------------------------------------------------------------------

/// AC-16 d：`timeout_ms=200` 的决策等待期限到期 → TimedOut/NODE_TIMEOUT/run Failed，
/// 无 worker 可 kill（`killed_workers` 不增）；之后 decide → exit 1。
#[test]
fn ac16d_decision_deadline_times_out_run() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db = dir.path().join("timeout.db");
    let run = RunProcess::spawn(
        dir.path(),
        &db,
        &human_gate_graph(TIMEOUT.name),
        r#"{"start":{"message":"hello"}}"#,
        false,
    );
    let (run_id, instance_id) = poll_waiting_human(&db, "timeout_human", 0);
    let waiting_seen_at = Instant::now();
    let output = run.wait("timeout run");

    // 期限从进入 WaitingHuman 起计：≥ 声明值 200ms（watchdog tick 粒度可更长）。
    let elapsed = waiting_seen_at.elapsed();
    assert!(
        elapsed >= Duration::from_millis(150),
        "决策期限 200ms 不应提前触发（可见后 {elapsed:?}）"
    );
    assert!(
        elapsed < Duration::from_secs(10),
        "期限到期后应尽快失败（{elapsed:?}）"
    );

    assert_exit(&output, 1, "timeout run 应 Failed 退出 1");
    let payload = json_stdout(&output);
    assert_eq!(payload["status"], json!("Failed"));
    assert_eq!(payload["error"]["code"], json!("NODE_TIMEOUT"));
    assert_eq!(
        payload["killed_workers"],
        json!(0),
        "human 无 worker 可 kill"
    );

    let reader = TraceReader::open_readonly(&db).expect("open trace");
    let run_row = common::run_row(&reader, &run_id);
    assert_eq!(run_row.status, TraceRunStatus::Failed);
    assert_eq!(run_row.error_code.as_deref(), Some("NODE_TIMEOUT"));
    assert_eq!(run_row.killed_workers, 0, "killed_workers 不增");
    let human = common::instance(&reader, &run_id, "timeout_human");
    assert_eq!(human.status, TraceInstanceStatus::TimedOut);
    assert_eq!(human.error_code.as_deref(), Some("NODE_TIMEOUT"));
    let error = human
        .error
        .as_ref()
        .expect("TimedOut 实例必须带 error JSON");
    assert_eq!(error["kind"], json!("Timeout"));
    let events = common::events(&reader, &run_id);
    assert_eq!(
        common::count_kind(&events, "NodeTimedOut"),
        1,
        "NodeTimedOut 恰一次"
    );
    assert!(
        gate_rows(&reader, &run_id, "timeout_human").is_empty(),
        "超时路径不 publish，不产生 GateResult"
    );
    assert!(decision_rows(&reader, &run_id).is_empty());
    drop(reader);

    // 到期后 decide → 目标不存在（run 终态）→ exit 1 `HUMAN_RUN_NOT_ACTIVE`。
    let late = decide_cli(
        dir.path(),
        &db,
        &run_id,
        &instance_id,
        "approved",
        "alice",
        None,
    );
    assert_decide_failure(&late, 1, "HUMAN_RUN_NOT_ACTIVE", "期限到期后 decide");
}

// ---------------------------------------------------------------------------
// AC-16 e：v3 seed → v5 链式迁移（L1 golden 不变）
// ---------------------------------------------------------------------------

/// 用 `schema_v3_seed.sql` 在临时目录建真实 v3 库（返回 `(db, old_run_id)`）。
fn seeded_v3_db(dir: &Path) -> (PathBuf, String) {
    let sql = std::fs::read_to_string(trace_fixture("schema_v3_seed.sql"))
        .expect("read schema_v3_seed.sql");
    let db = dir.join("seed_v3.db");
    let conn = Connection::open(&db).expect("create v3 seed db");
    conn.execute_batch(&sql)
        .expect("execute schema_v3_seed.sql");
    assert_eq!(user_version(&conn), 3, "seed 必须是 v3 库");
    let run_id: String = conn
        .query_row("SELECT run_id FROM runs", [], |row| row.get(0))
        .expect("seed 有一条 run");
    assert_eq!(run_id, SEED_RUN_ID, "seed run id 必须固定");
    (db, run_id)
}

/// 冻结的 v3 seed 迁移后 `trace --json` golden（W2a/AC-14 h 资产）。
fn expected_v01_trace() -> Vec<Value> {
    let text = std::fs::read_to_string(trace_fixture("v0.1.0_expected_trace.json"))
        .expect("read v0.1.0_expected_trace.json");
    serde_json::from_str(&text).expect("expected trace is JSON array")
}

/// AC-16 e：v3 seed（v0.1.0 形态）→ 真 CLI run 打开 → v3→v4→v5→v6 链式迁移；
/// 旧 run 的 L1 `trace --json` 与 v0.1.0 golden 逐字节一致、replay == trace。
#[test]
fn ac16e_v3_seed_chain_to_v5_preserves_v01_golden() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (db, old_run_id) = seeded_v3_db(dir.path());

    // 只读 trace/replay 对 v3 拒绝且不迁移（forward-only）。
    for output in [
        trace_json(&db, &old_run_id),
        replay_events_json(&db, &old_run_id),
    ] {
        assert_exit(&output, 4, "只读命令对 v3 库应 exit 4");
        assert!(
            stderr_text(&output).contains("unsupported schema version=3, expected 6"),
            "stderr: {}",
            stderr_text(&output)
        );
    }
    {
        let conn = read_only_connect(&db);
        assert_eq!(user_version(&conn), 3, "只读拒绝不得迁移");
    }

    // 真 CLI run 触发 v3→v4→v5→v6 链式迁移。
    let output = run_seed_graph(&db, dir.path());
    assert_exit(&output, 0, "v3 seed 上触发链式迁移的 run");
    assert_eq!(json_stdout(&output)["status"], json!("Completed"));

    {
        let conn = read_only_connect(&db);
        assert_eq!(user_version(&conn), 6, "迁移后 user_version=6");
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
        assert_eq!(versions, vec![1, 2, 3, 4, 5, 6], "schema_migrations 1..6");
        let decisions: i64 = conn
            .query_row("SELECT COUNT(*) FROM human_decisions", [], |row| row.get(0))
            .expect("decisions count");
        assert_eq!(decisions, 0, "无 human 图不产生决策行");
    }
    // 链式迁移每版本恰一份备份（v3→v4、v4→v5、v5→v6 各一份）。
    let backups = backup_files(dir.path());
    let has_v3 = backups.iter().any(|path| {
        path.file_name()
            .map(|name| name.to_string_lossy().contains(".bak.v3-"))
            .unwrap_or(false)
    });
    let has_v4 = backups.iter().any(|path| {
        path.file_name()
            .map(|name| name.to_string_lossy().contains(".bak.v4-"))
            .unwrap_or(false)
    });
    let has_v5 = backups.iter().any(|path| {
        path.file_name()
            .map(|name| name.to_string_lossy().contains(".bak.v5-"))
            .unwrap_or(false)
    });
    assert!(
        has_v3 && has_v4 && has_v5,
        "链式迁移应有 .bak.v3-*、.bak.v4-* 与 .bak.v5-*: {backups:?}"
    );

    // 旧 run（v0.1.0 形态）L1 golden 不变；replay == trace 逐字节。
    let trace = trace_json(&db, &old_run_id);
    assert_exit(&trace, 0, "迁移后 trace 旧 run");
    assert_eq!(
        ndjson_events(&trace),
        expected_v01_trace(),
        "v3 seed 旧 run 的 L1 golden 必须与 v5 前一致"
    );
    let replay = replay_events_json(&db, &old_run_id);
    assert_exit(&replay, 0, "迁移后 replay 旧 run");
    assert_eq!(replay.stdout, trace.stdout, "replay 与 trace 逐字节一致");

    // 旧 payload 保持 v3 形态、SQL 三列为迁移默认值 0/0/NULL。
    let reader = TraceReader::open_readonly(&db).expect("open trace");
    let events = common::events(&reader, &old_run_id);
    let gate_event = events
        .iter()
        .find(|event| event.kind == "GateResult")
        .expect("旧 run 有 GateResult");
    assert!(
        gate_event.payload["record"].get("deferred").is_none(),
        "旧 payload 不得被改写为 v4 形态: {:?}",
        gate_event.payload
    );
    drop(reader);
    let conn = read_only_connect(&db);
    let (deferred, attempt, verdict): (i64, i64, Option<String>) = conn
        .query_row(
            "SELECT deferred, attempt, verdict_json FROM gate_results WHERE run_id = ?1",
            [&old_run_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .expect("old gate row");
    assert_eq!(
        (deferred, attempt, verdict),
        (0, 0, None),
        "旧 gate_results 三列仅补默认值"
    );
}

// ---------------------------------------------------------------------------
// AC-16 f：decide × run 并发（WAL busy）与库 API 场景
// ---------------------------------------------------------------------------

/// AC-16 f（CLI）：run 挂起 + 同实例 5 个并发 decide → 恰 1 个 exit 0、其余 exit 5，
/// 恰 1 行决策且 consumed、run 正常接续；重复 3 轮无 flake、无 SQLITE_BUSY 泄漏。
#[test]
fn ac16f_concurrent_decide_exactly_one_wins() {
    for round in 1..=CONCURRENCY_ROUNDS {
        let dir = tempfile::tempdir().expect("tempdir");
        let db = dir.path().join(format!("concurrent_{round}.db"));
        let run = RunProcess::spawn(
            dir.path(),
            &db,
            &human_gate_graph(APPROVE.name),
            r#"{"start":{"message":"race"}}"#,
            false,
        );
        let (run_id, instance_id) = poll_waiting_human(&db, "approve_human", 0);

        // 5 个 decide 进程同时启动（同 run/instance）。
        let children: Vec<Child> = (1..=CONCURRENT_DECIDERS)
            .map(|index| {
                spawn_decide(
                    dir.path(),
                    &db,
                    &run_id,
                    &instance_id,
                    "approved",
                    &format!("racer-{index}"),
                )
            })
            .collect();
        let outputs: Vec<Output> = children
            .into_iter()
            .map(|child| wait_child_output(child, "并发 decide", CHILD_TIMEOUT))
            .collect();

        let mut winners = 0;
        for output in &outputs {
            match output.status.code() {
                Some(0) => {
                    winners += 1;
                    let payload = json_stdout(output);
                    assert_eq!(payload["ok"], json!(true));
                }
                Some(5) => {
                    let payload = json_stdout(output);
                    assert_eq!(payload["ok"], json!(false));
                    assert_eq!(
                        payload["code"],
                        json!("HUMAN_DECISION_EXISTS"),
                        "并发落败者必须是 HUMAN_DECISION_EXISTS: {payload}"
                    );
                }
                other => panic!(
                    "并发 decide 只允许 exit 0/5，实际 {other:?}\nstdout: {}\nstderr: {}",
                    String::from_utf8_lossy(&output.stdout),
                    stderr_text(output)
                ),
            }
        }
        assert_eq!(winners, 1, "第 {round} 轮：同实例并发 decide 恰一个成功");

        // run 正常接续并完成；stderr 无 SQLITE_BUSY/consumer 存储告警。
        let run_output = run.wait("并发 decide 后的 run");
        assert_exit(&run_output, 0, "并发 decide 后 run 应正常 Completed");
        assert_eq!(json_stdout(&run_output)["status"], json!("Completed"));
        let run_stderr = stderr_text(&run_output);
        for forbidden in [
            "SQLITE_BUSY",
            "database is locked",
            "cannot poll human decisions",
            "cannot mark human decision consumed",
        ] {
            assert!(
                !run_stderr.contains(forbidden),
                "第 {round} 轮：并发 decide 不得泄漏存储错误 `{forbidden}`: {run_stderr}"
            );
        }

        let reader = TraceReader::open_readonly(&db).expect("open trace");
        let decisions = decision_rows(&reader, &run_id);
        assert_eq!(decisions.len(), 1, "第 {round} 轮：human_decisions 恰一行");
        assert!(
            decisions[0].consumed_at.is_some(),
            "第 {round} 轮：决策行已消费（无悬挂）"
        );
        assert!(
            decisions[0].approver.starts_with("racer-"),
            "记录的是某个并发进程的 approver"
        );
        let run_row = common::run_row(&reader, &run_id);
        assert_eq!(run_row.status, TraceRunStatus::Completed);
        assert_eq!(
            run_row.error_code, None,
            "并发 decide 不得造成存储错误（无 SQLITE_BUSY 泄漏）"
        );
        assert_eq!(run_row.killed_workers, 0);
    }
}

/// 库 API 用例的 **no-op 决策 sink**：记录实例快照/迁移/run 终态，但**不实现**
/// `human_decided`（验证 trait 默认 no-op 合法，DR-006 §7.3 #7：applied 不可观测时
/// 行为仍正确）。
#[derive(Default)]
struct SnapshotOnlySink {
    snapshots: Mutex<Vec<InstanceSnapshot>>,
    transitions: Mutex<Vec<(String, String, String)>>,
    finished: Mutex<Option<RunFinishedInfo>>,
}

impl SnapshotOnlySink {
    /// 轮询等待某节点/attempt 进入 `WaitingHuman` 的快照（异步：不阻塞 current-thread runtime）。
    async fn wait_waiting(
        &self,
        node_id: &str,
        attempt: u32,
        timeout: Duration,
    ) -> InstanceSnapshot {
        let deadline = Instant::now() + timeout;
        loop {
            if let Some(snapshot) = self
                .snapshots
                .lock()
                .expect("lock")
                .iter()
                .rev()
                .find(|snapshot| {
                    snapshot.node_id == node_id
                        && snapshot.attempt == attempt
                        && snapshot.status == KernelInstanceStatus::WaitingHuman
                })
                .cloned()
            {
                return snapshot;
            }
            assert!(
                Instant::now() < deadline,
                "node {node_id} attempt {attempt} 未在 {timeout:?} 内 WaitingHuman"
            );
            tokio::time::sleep(POLL_INTERVAL).await;
        }
    }

    /// 某节点/attempt 的最新快照。
    fn latest(&self, node_id: &str, attempt: u32) -> Option<InstanceSnapshot> {
        self.snapshots
            .lock()
            .expect("lock")
            .iter()
            .rev()
            .find(|snapshot| snapshot.node_id == node_id && snapshot.attempt == attempt)
            .cloned()
    }

    /// 某节点的迁移 `(from, to)`。
    fn transitions_of(&self, node_id: &str) -> Vec<(String, String)> {
        self.transitions
            .lock()
            .expect("lock")
            .iter()
            .filter(|(node, _, _)| node == node_id)
            .map(|(_, from, to)| (from.clone(), to.clone()))
            .collect()
    }

    /// run 终态（`RunFinished`）。
    fn finished(&self) -> Option<RunFinishedInfo> {
        self.finished.lock().expect("lock").clone()
    }
}

impl TraceSink for SnapshotOnlySink {
    fn run_started(&self, _info: RunStartedInfo) {}

    fn instance_snapshot(&self, snapshot: InstanceSnapshot) {
        self.snapshots.lock().expect("lock").push(snapshot);
    }

    fn transition(&self, record: TransitionRecord, snapshot: InstanceSnapshot) {
        self.transitions.lock().expect("lock").push((
            record.node_id.clone(),
            record.from.as_str().to_string(),
            record.to.as_str().to_string(),
        ));
        self.snapshots.lock().expect("lock").push(snapshot);
    }

    fn gate_result(&self, _record: GateResultRecord) {}

    fn node_timed_out(
        &self,
        _run_id: &RunId,
        _instance_id: &InstanceId,
        _elapsed_ms: u64,
        _at: String,
    ) {
    }

    fn node_abandoned(&self, _run_id: &RunId, _instance_id: &InstanceId, _at: String) {}

    fn streaming(
        &self,
        _run_id: &RunId,
        _instance_id: &InstanceId,
        _node_id: &NodeId,
        _event: StreamingEvent,
        _at: String,
    ) -> bool {
        true
    }

    fn run_finished(&self, info: RunFinishedInfo) {
        *self.finished.lock().expect("lock") = Some(info);
    }

    // 不实现 `human_decided`：使用 trait 默认 no-op（DR-006 §7.3 #7 冻结边界）。
}

/// AC-16 f（库 API）：无 SQLite、`TraceSink::human_decided` 默认 no-op，调用方直接经
/// `RunControl::HumanDecision` 驱动 human 等待/决策；真实 worker 执行上游节点。
#[tokio::test]
async fn ac16f_library_api_drives_human_decision_without_trace_db() {
    let dir = tempfile::tempdir().expect("tempdir");
    let graph = common::compile_fixture("human_gate/approve.json");
    let inputs = common::entry_inputs(&[("start", "message", json!("library"))]);
    let sink = Arc::new(SnapshotOnlySink::default());
    let handle = start_run(
        None,
        common::spec(graph, inputs),
        common::executor(common::RUN_TIMEOUT),
        sink.clone(),
    )
    .expect("start_run");

    // 等待进入 WaitingHuman（无 DB；靠 sink 的实例快照观察）。
    let waiting = sink.wait_waiting("approve_human", 0, CHILD_TIMEOUT).await;
    assert_eq!(
        sink.transitions_of("approve_human"),
        vec![("Pending".to_string(), "WaitingHuman".to_string())],
        "库 API 同样不经 Ready/Running"
    );
    let input = waiting.input.as_ref().expect("input snapshot");
    let prep_output = sink
        .latest("prep", 0)
        .and_then(|snapshot| snapshot.output)
        .expect("prep output");
    assert_eq!(
        input["artifact"], prep_output,
        "等待上下文 = 该代上游产物（全部声明端口）"
    );

    // 调用方直接提交决策（无 consumer、无 DB）。
    assert!(
        handle.control(RunControl::HumanDecision {
            instance_id: waiting.instance_id.clone(),
            decision: HumanDecision::Approved,
            approver: "lib-approver".to_string(),
            reason: Some("lib ok".to_string()),
            decision_id: "0190f0d0-0000-7000-8000-000000000042".to_string(),
            decided_at: "2026-11-01T00:00:00.000Z".to_string(),
        }),
        "control 通道应接受 HumanDecision"
    );

    let outcome = handle.wait().await;
    assert_eq!(outcome.status, KernelRunStatus::Completed);
    assert_eq!(
        outcome.output,
        Some(json!({"result": {"decision": "approved", "reason": "lib ok"}})),
        "库 API run_output 与真链路一致"
    );
    let human = sink.latest("approve_human", 0).expect("human snapshot");
    assert_eq!(human.status, KernelInstanceStatus::Completed);
    assert_eq!(
        human.output,
        Some(json!({"decision": "approved", "reason": "lib ok"}))
    );
    assert_eq!(
        sink.transitions_of("approve_human"),
        vec![
            ("Pending".to_string(), "WaitingHuman".to_string()),
            ("WaitingHuman".to_string(), "Completed".to_string()),
        ]
    );
    let finished = sink.finished().expect("RunFinished 恰一次");
    assert_eq!(finished.status, KernelRunStatus::Completed);
    assert_eq!(
        finished.output_summary,
        Some(json!({"result": {"decision": "approved", "reason": "lib ok"}}))
    );
    // 无 SQLite 依赖：库 API 不写任何 DB/文件。
    assert_eq!(
        std::fs::read_dir(dir.path()).expect("read tempdir").count(),
        0,
        "库 API 用例不得产生任何文件"
    );
}

/// AC-16 f（库 API / `trace_enabled=false`）：无 sink 也必须保持纯等待（非静默、
/// 无 RUN_DEADLOCK）；取消 → Cancelled。
#[tokio::test]
async fn ac16f_library_api_trace_disabled_still_waits_and_cancels() {
    let dir = tempfile::tempdir().expect("tempdir");
    let graph = common::compile_fixture("human_gate/approve.json");
    let inputs = common::entry_inputs(&[("start", "message", json!("library"))]);
    let spec = common::spec_with(graph, inputs, |options| {
        options.trace_enabled = false;
        options.deadlock_grace_ms = 200;
        options.tick_interval_ms = 50;
    });
    let handle = start_run(
        None,
        spec,
        common::executor(common::RUN_TIMEOUT),
        Arc::new(zgraph_kernel::NullTraceSink),
    )
    .expect("start_run");
    // 纯 WaitingHuman 超过 deadlock_grace 仍 Running（不是死锁、不提前完成）。
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert_eq!(handle.status(), KernelRunStatus::Running);
    handle.cancel();
    let outcome = handle.wait().await;
    assert_eq!(outcome.status, KernelRunStatus::Cancelled);
    assert_eq!(outcome.output, None);
    // `trace_enabled=false` 不需要 DB：库 API 不产生文件。
    assert_eq!(dir.path().read_dir().expect("read dir").count(), 0);
}

// ---------------------------------------------------------------------------
// AC-16 g：默认图 / v0.1.0 golden 不变 / human 图 + `:memory:` 保护
// ---------------------------------------------------------------------------

/// 默认（无 human）代表图与冻结 `graph_hash`（W3a 不改 DSL；缺失字段不参与投影）。
const DEFAULT_GRAPHS: [(&str, &str); 6] = [
    ("customer_ticket.json", "ec320e4244b365ab"),
    ("valid_parallel.json", "b15f162f7f87fd58"),
    ("valid_gate_defer.json", "2ff1339bd3d6d928"),
    ("valid_cycle.json", "0e8911b6b4003c7e"),
    ("valid_max_fanout.json", "e05d882cc55aa275"),
    ("valid_subgraph.json", "e903b92cf900587c"),
];

/// AC-16 g：默认图（v0.1.0/W2a/W2b）`graph_hash` 与 validate 结果不变。
#[test]
fn ac16g_default_graph_hashes_and_validate_unchanged() {
    for (name, expected_hash) in DEFAULT_GRAPHS {
        let output = run_cli(
            &[
                "validate".to_string(),
                arg(&graphs_root().join(name)),
                "--json".to_string(),
            ],
            &repo_root(),
        );
        assert_exit(&output, 0, &format!("默认图 {name} validate"));
        let payload = json_stdout(&output);
        assert_eq!(payload["ok"], json!(true), "{name}");
        assert_eq!(payload["error_count"], json!(0), "{name}");
        assert_eq!(
            payload["graph_hash"],
            json!(expected_hash),
            "{name}: W3a 不得改变 graph_hash（DSL 无变化）"
        );
    }
}

/// AC-16 g：无 human 图的真 run 不产生 `WaitingHuman`/`human_decisions`/HumanDecision 行，
/// `replay --mode=events` 与 `trace --json` 仍逐字节一致（trace v5 默认输出不变）。
#[test]
fn ac16g_default_run_trace_has_no_human_artifacts() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db = dir.path().join("default.db");
    let output = run_seed_graph(&db, dir.path());
    assert_exit(&output, 0, "默认图 run");
    let payload = json_stdout(&output);
    assert_eq!(payload["status"], json!("Completed"));
    let run_id = payload["run_id"].as_str().expect("run_id").to_string();

    let reader = TraceReader::open_readonly(&db).expect("open trace");
    assert!(
        decision_rows(&reader, &run_id).is_empty(),
        "无 human 图 human_decisions 必须为空"
    );
    assert!(
        reader
            .instances(&run_id)
            .expect("query instances")
            .iter()
            .all(|row| row.status != TraceInstanceStatus::WaitingHuman),
        "无 human 图不得出现 WaitingHuman 实例"
    );
    drop(reader);

    let trace = trace_json(&db, &run_id);
    assert_exit(&trace, 0, "默认 run trace --json");
    let lines = ndjson_events(&trace);
    assert!(
        lines
            .iter()
            .all(|line| line["kind"] != json!("HumanDecision")),
        "无决策 run 的 trace --json 不得追加 HumanDecision 行"
    );
    assert_eq!(
        lines.len(),
        V4_SEED_EVENTS as usize,
        "事件行数与 v4/v5 前一致"
    );
    let replay = replay_events_json(&db, &run_id);
    assert_exit(&replay, 0, "默认 run replay");
    assert_eq!(replay.stdout, trace.stdout, "replay 与 trace 逐字节一致");
}

/// AC-16 g / DR-006 §7.3 #7：human 图 + CLI `--db :memory:` → **启动前预检拒绝**
/// `RUN_ENTRY_INVALID`（exit 3，reason `trace_required_for_human`），不产生 run/DB 文件。
#[test]
fn ac16g_human_graph_with_memory_db_is_rejected_before_run() {
    let dir = tempfile::tempdir().expect("tempdir");
    let output = run_cli(
        &[
            "run".to_string(),
            arg(&human_gate_graph(APPROVE.name)),
            "--inputs".to_string(),
            r#"{"start":{"message":"hello"}}"#.to_string(),
            "--db".to_string(),
            ":memory:".to_string(),
            "--python".to_string(),
            python_program(),
            "--python-path".to_string(),
            arg(&repo_root().join("python")),
            "--json".to_string(),
        ],
        dir.path(),
    );
    assert_exit(&output, 3, ":memory: + human 图应启动前拒绝（exit 3）");
    let stderr = stderr_text(&output);
    assert!(
        stderr.contains("RUN_ENTRY_INVALID"),
        "stderr 应带 RUN_ENTRY_INVALID: {stderr}"
    );
    assert!(
        stderr.contains("trace_required_for_human"),
        "stderr 应带 reason trace_required_for_human: {stderr}"
    );
    assert!(
        String::from_utf8_lossy(&output.stdout).trim().is_empty(),
        "--json 失败路径不写 stdout"
    );
    // 不产生 run/DB：工作目录不得出现 `.zgraph` 或任何 db 文件。
    let entries: Vec<String> = std::fs::read_dir(dir.path())
        .expect("read dir")
        .map(|entry| {
            entry
                .expect("dir entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    assert!(
        entries.is_empty(),
        "启动前拒绝不得产生 run/DB 文件: {entries:?}"
    );
}

// ---------------------------------------------------------------------------
// AC-16 a/e 补充：L2 `replay --mode=outputs`（human 实例记录与重放）
// ---------------------------------------------------------------------------
//
// DR-006 §2.2/§8 L2 行冻结：`--record=full` 下 human 实例输出按 `(node_id, attempt)`
// 记录，`replay --mode=outputs` **不重新要求人工决策**、结果与原 run 一致。
// 记录由 CLI bridge 在 human `WaitingHuman→Completed` 迁移时补发（fix/cli-human-record）。

/// 某 run 的 `recorded_outputs` 行（按 `instance_id, attempt` 升序）。
fn recorded_rows(reader: &TraceReader, run_id: &str) -> Vec<RecordedOutputRow> {
    reader
        .recorded_outputs(run_id)
        .expect("query recorded_outputs")
}

/// 断言某条记录：`output_json` 与期望一致、`output_hash` 为 64-hex blake3 且可复核。
fn assert_recorded_output(record: &RecordedOutputRow, expected_output: &Value, context: &str) {
    assert_eq!(
        record.output_json.as_ref(),
        Some(expected_output),
        "{context}: recorded output_json"
    );
    assert_eq!(
        record.output_hash.len(),
        64,
        "{context}: blake3 hex 应为 64 字符"
    );
    assert!(
        record
            .output_hash
            .chars()
            .all(|character| character.is_ascii_hexdigit()),
        "{context}: output_hash 必须是 64 hex"
    );
    assert_eq!(
        record.output_hash,
        zgraph_cli::run_cmd::output_hash(expected_output),
        "{context}: output_hash 口径与 run_cmd::output_hash 一致"
    );
}

/// AC-16 a（L2）：approve 真链路 `--record=full` → human 实例 `recorded_outputs` 行
/// （output_json/hash）→ `replay --mode=outputs --json` exit0/matched=true、
/// 不产生新 run、不修改原 trace、无 REPLAY_RECORD_MISSING。
#[test]
fn ac16a_human_run_l2_outputs_replay_matches_recorded_outputs() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db = dir.path().join("approve_l2.db");
    let run = RunProcess::spawn(
        dir.path(),
        &db,
        &human_gate_graph(APPROVE.name),
        r#"{"start":{"message":"l2"}}"#,
        true,
    );
    let (run_id, instance_id) = poll_waiting_human(&db, "approve_human", 0);
    let decide = decide_cli(
        dir.path(),
        &db,
        &run_id,
        &instance_id,
        "approved",
        "l2-approver",
        Some("L2 ok"),
    );
    assert_exit(&decide, 0, "L2 用例 decide approved");
    let output = run.wait("L2 approve run");
    assert_exit(&output, 0, "L2 用例 run Completed");

    // ISSUE-089：先取 DB 字节快照，再开任何只读连接（读连接也会更新 -shm）。
    let files_before = cli_support::db_files_snapshot(&db);

    let expected_output = json!({"decision": "approved", "reason": "L2 ok"});
    let expected_run_output = json!({"result": {"decision": "approved", "reason": "L2 ok"}});
    {
        let reader = TraceReader::open_readonly(&db).expect("open trace");
        let records = recorded_rows(&reader, &run_id);
        let mut nodes: Vec<(&str, u32)> = records
            .iter()
            .map(|record| (record.node_id.as_str(), record.attempt))
            .collect();
        nodes.sort_unstable();
        assert_eq!(
            nodes,
            vec![("approve_human", 0), ("prep", 0)],
            "human 实例必须有 L2 记录（DR-006 §2.2）；start/end 为内置节点不记录"
        );
        let human = records
            .iter()
            .find(|record| record.node_id == "approve_human" && record.attempt == 0)
            .expect("human recorded_outputs 行");
        assert_recorded_output(human, &expected_output, "human/attempt0");
    }

    let runs_before = cli_support::runs_count(&db);
    let replay = run_cli(
        &[
            "replay".to_string(),
            "--run".to_string(),
            run_id.clone(),
            "--mode".to_string(),
            "outputs".to_string(),
            "--db".to_string(),
            arg(&db),
            "--json".to_string(),
        ],
        dir.path(),
    );
    assert_exit(
        &replay,
        0,
        "human 图 L2 replay（不得 REPLAY_RECORD_MISSING）",
    );
    let payload = json_stdout(&replay);
    assert_eq!(payload["ok"], json!(true));
    assert_eq!(payload["command"], json!("replay"));
    assert_eq!(payload["mode"], json!("outputs"));
    assert_eq!(payload["run_id"], json!(run_id));
    assert_eq!(payload["record_mode"], json!("full"));
    assert_eq!(payload["status"], json!("Completed"));
    assert_eq!(payload["matched"], json!(true));
    assert_eq!(
        payload["recorded_outputs"],
        json!(2),
        "prep + human 两条记录"
    );
    assert_eq!(payload["original_output"], expected_run_output);
    assert_eq!(
        payload["original_output"], payload["replayed_output"],
        "L2 重放输出必须与原 run 输出一致（retrieve human 记录、不重新要求人工决策）"
    );
    assert!(
        !stderr_text(&replay).contains("REPLAY_RECORD_MISSING"),
        "stderr 不得出现 REPLAY_RECORD_MISSING: {}",
        stderr_text(&replay)
    );

    // 只读：不产生新 run、不修改原 trace 字节（先文件字节、后计数，ISSUE-089）。
    assert_eq!(
        cli_support::db_files_snapshot(&db),
        files_before,
        "L2 replay 不得修改原 trace DB（主文件 + -wal + -shm）"
    );
    assert_eq!(
        cli_support::runs_count(&db),
        runs_before,
        "replay 不得产生新 run"
    );
    let reader = TraceReader::open_readonly(&db).expect("open trace");
    let decisions = decision_rows(&reader, &run_id);
    assert_eq!(decisions.len(), 1, "replay 不得重新写决策");
    assert!(decisions[0].consumed_at.is_some());
}

/// AC-16 b/e（L2，attempt>0）：rework 两轮决策后 human attempt 0/1 均有 L2 记录，
/// `replay --mode=outputs` matched（重放不重新要求人工决策）。
#[test]
fn ac16b_rework_l2_outputs_replay_uses_both_human_attempts() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db = dir.path().join("rework_l2.db");
    let run = RunProcess::spawn(
        dir.path(),
        &db,
        &human_gate_graph(REWORK.name),
        r#"{"start":{"message":"l2 rework"}}"#,
        true,
    );
    let (run_id, instance0) = poll_waiting_human(&db, "rework_human", 0);
    let reject = decide_cli(
        dir.path(),
        &db,
        &run_id,
        &instance0,
        "rejected",
        "bob",
        Some("needs rework"),
    );
    assert_exit(&reject, 0, "rework L2 第一轮 reject");
    let (_, instance1) = poll_waiting_human(&db, "rework_human", 1);
    let approve = decide_cli(
        dir.path(),
        &db,
        &run_id,
        &instance1,
        "approved",
        "alice",
        Some("ok now"),
    );
    assert_exit(&approve, 0, "rework L2 第二轮 approve");
    let output = run.wait("rework L2 run");
    assert_exit(&output, 0, "rework L2 run Completed");
    let expected_run_output = json!({"result": {"decision": "approved", "reason": "ok now"}});

    let files_before = cli_support::db_files_snapshot(&db);
    {
        let reader = TraceReader::open_readonly(&db).expect("open trace");
        let records = recorded_rows(&reader, &run_id);
        let human0 = records
            .iter()
            .find(|record| record.node_id == "rework_human" && record.attempt == 0)
            .expect("human attempt0 记录");
        let human1 = records
            .iter()
            .find(|record| record.node_id == "rework_human" && record.attempt == 1)
            .expect("human attempt1 记录（defer 重入后逐 attempt 记录）");
        assert_recorded_output(
            human0,
            &json!({"decision": "rejected", "reason": "needs rework"}),
            "rework_human/attempt0",
        );
        assert_recorded_output(
            human1,
            &json!({"decision": "approved", "reason": "ok now"}),
            "rework_human/attempt1",
        );
        assert_ne!(
            human0.instance_id, human1.instance_id,
            "attempt 0/1 是不同实例、各自记录"
        );
    }

    let replay = run_cli(
        &[
            "replay".to_string(),
            "--run".to_string(),
            run_id.clone(),
            "--mode".to_string(),
            "outputs".to_string(),
            "--db".to_string(),
            arg(&db),
            "--json".to_string(),
        ],
        dir.path(),
    );
    assert_exit(&replay, 0, "rework 图 L2 replay");
    let payload = json_stdout(&replay);
    assert_eq!(payload["matched"], json!(true));
    assert_eq!(payload["status"], json!("Completed"));
    assert_eq!(payload["original_output"], expected_run_output);
    assert_eq!(payload["replayed_output"], expected_run_output);
    assert_eq!(
        payload["recorded_outputs"],
        json!(4),
        "prep×2 + human×2 共四条记录"
    );
    assert_eq!(
        cli_support::db_files_snapshot(&db),
        files_before,
        "L2 replay 不得修改原 trace"
    );
    assert_eq!(cli_support::runs_count(&db), 1, "L2 replay 不得产生新 run");
}

/// AC-16 e/a（L2 反例）：人为删除 human 记录（模拟旧 run 缺记录）→ 预检 exit3
/// `REPLAY_RECORD_MISSING`、快速拒绝（不进入重跑/不挂起）、不产生新 run。
#[test]
fn ac16a_l2_replay_rejects_missing_human_record() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db = dir.path().join("missing_human_record.db");
    let run = RunProcess::spawn(
        dir.path(),
        &db,
        &human_gate_graph(APPROVE.name),
        r#"{"start":{"message":"old-run"}}"#,
        true,
    );
    let (run_id, instance_id) = poll_waiting_human(&db, "approve_human", 0);
    let decide = decide_cli(
        dir.path(),
        &db,
        &run_id,
        &instance_id,
        "approved",
        "alice",
        None,
    );
    assert_exit(&decide, 0, "missing-record 用例 decide");
    assert_exit(&run.wait("missing-record run"), 0, "missing-record run");

    // 模拟「旧 run / 未补录 human 记录」：直接删除 human 的 recorded_outputs 行。
    {
        let conn = cli_support::connect(&db);
        let deleted = conn
            .execute(
                "DELETE FROM recorded_outputs WHERE run_id = ?1 AND node_id = 'approve_human'",
                [&run_id],
            )
            .expect("delete human recorded output");
        assert_eq!(deleted, 1, "应恰删除一条 human 记录");
    }
    let runs_before = cli_support::runs_count(&db);

    let started = Instant::now();
    let replay = run_cli(
        &[
            "replay".to_string(),
            "--run".to_string(),
            run_id.clone(),
            "--mode".to_string(),
            "outputs".to_string(),
            "--db".to_string(),
            arg(&db),
            "--json".to_string(),
        ],
        dir.path(),
    );
    let elapsed = started.elapsed();
    assert_exit(
        &replay,
        3,
        "缺 human 记录 → exit 3（REPLAY_RECORD_MISSING）",
    );
    let stderr = stderr_text(&replay);
    assert!(stderr.contains("REPLAY_RECORD_MISSING"), "stderr: {stderr}");
    assert!(
        stderr.contains("approve_human"),
        "错误消息应指出缺失节点: {stderr}"
    );
    assert!(
        elapsed < Duration::from_secs(5),
        "缺记录必须快速拒绝、不进入重跑挂起（{elapsed:?}）"
    );
    assert_eq!(
        cli_support::runs_count(&db),
        runs_before,
        "预检失败不得产生新 run"
    );
}

/// AC-16 a（`--record=hash`）：human 行同样被记录（fix 的行集合变化属设计预期），
/// 但 `output_json` 为 NULL；L2 replay 因 record_mode≠full 快速 exit3。
#[test]
fn ac16a_human_hash_record_mode_has_no_output_json() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db = dir.path().join("hash_mode.db");
    let run = RunProcess::spawn(
        dir.path(),
        &db,
        &human_gate_graph(APPROVE.name),
        r#"{"start":{"message":"hash"}}"#,
        false,
    );
    let (run_id, instance_id) = poll_waiting_human(&db, "approve_human", 0);
    let decide = decide_cli(
        dir.path(),
        &db,
        &run_id,
        &instance_id,
        "approved",
        "alice",
        Some("hash mode"),
    );
    assert_exit(&decide, 0, "hash 模式 decide");
    assert_exit(&run.wait("hash 模式 run"), 0, "hash 模式 run");

    let reader = TraceReader::open_readonly(&db).expect("open trace");
    let run_row = common::run_row(&reader, &run_id);
    assert_eq!(
        run_row.record_mode.as_deref(),
        Some("hash"),
        "默认 record 模式为 hash"
    );
    let records = recorded_rows(&reader, &run_id);
    let human = records
        .iter()
        .find(|record| record.node_id == "approve_human" && record.attempt == 0)
        .expect("hash 模式同样记录 human 行（行集合变化属 fix 设计预期）");
    assert_eq!(
        human.output_json, None,
        "hash 模式只写 output_hash、不写 output_json"
    );
    assert_eq!(human.output_hash.len(), 64, "hash 恒写 64-hex");
    drop(reader);

    // L2 预检只看 run.record_mode：hash → exit3（且不重跑/不挂起）。
    let replay = run_cli(
        &[
            "replay".to_string(),
            "--run".to_string(),
            run_id,
            "--mode".to_string(),
            "outputs".to_string(),
            "--db".to_string(),
            arg(&db),
            "--json".to_string(),
        ],
        dir.path(),
    );
    assert_exit(&replay, 3, "hash 模式 L2 replay 应 exit3");
    let stderr = stderr_text(&replay);
    assert!(stderr.contains("REPLAY_RECORD_MISSING"), "stderr: {stderr}");
    assert!(
        stderr.contains("requires --record=full"),
        "应提示 --record=full: {stderr}"
    );
}
