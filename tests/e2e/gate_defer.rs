//! W2a（v0.2 / M7）deferrable gate + 有限返工 e2e（AC-14 a/b/f/g/h）。
//!
//! 语义基准：`doc/decisions/DR-005-w2a-defer-gate-freeze.md`（R15 / 运行期决策矩阵 /
//! dormant+确定性死锁 / trace v4 / AC-14）、`doc/spec-v1.2/07-验收用例.md` AC-14、
//! `doc/design/software-team-sop-v1.md` v1.1。数据通路为**真链路**（禁止 mock）：
//!
//! ```text
//! tests/fixtures/graphs/defer_gate/*.json（R13/R15 已通过）
//!   → 真 `zgraph` 二进制 run（真 kernel + 真 Python worker 子进程 + 真 SQLite trace）
//!   → TraceReader / 只读 SQL 断言（attempt/deferred/verdict/死锁/迁移）
//! ```
//!
//! 本文件分三部分：
//!
//! 1. **fixtures 静态断言**（不依赖 W2a kernel）：三个 defer 图 `zgraph validate`
//!    通过（R13/R15）且 `on_gate_false` 形状符合冻结；
//! 2. **AC-14 h**（不依赖 W2a kernel）：`tests/fixtures/trace/schema_v3_seed.sql`
//!    建 v3 库 → 真 `zgraph run` 触发 v3→v4 迁移 → 备份/审计/旧行默认值/旧 payload
//!    保持 v3 形态 → `trace`/`replay --mode=events` 对照冻结 golden；只读 v3 拒绝；
//! 3. **AC-14 a/b/f/g 运行时用例**：Phase 2A 先写全断言但 `#[ignore]`（W2a kernel
//!    `feat/kernel-deferrable-gate` 尚未合并），Phase 2B 合并后去掉 ignore 并跑全量。
//!
//! v3 seed/golden 的生成方式与归一化规则见
//! [`tests/fixtures/trace/GENERATION.md`](../../fixtures/trace/GENERATION.md)（W1 遗留
//! 缺口 ISSUE-104，由 W2a e2e 交付）。
//!
//! 稳定性约定：每个用例独立 `TempDir`/trace DB；不做 sleep/时序猜测；CLI 子进程退出
//! 即判定；fake/真实图形路径均相对 repo root，避免绝对路径进入 fixture。

#![cfg(unix)]

#[path = "cli_support/mod.rs"]
mod cli_support;
#[path = "common/mod.rs"]
mod common;

use std::path::{Path, PathBuf};
use std::process::Output;

use serde_json::Value;

use zgraph_trace::{EventQuery, InstanceRow, InstanceStatus as TraceInstanceStatus, TraceReader};

use cli_support::{arg, assert_exit, json_stdout, repo_root, run_cli, stderr_text};
use common::{graphs_root, run_row};

/// v3 seed 中固定化的旧 run id（见 `tests/fixtures/trace/GENERATION.md`）。
const SEED_RUN_ID: &str = "0192f0c4-7a1b-7c3d-8e4f-000000000001";

/// seed 图的入口输入（`start` 声明 `message`）。
const SEED_INPUTS: &str = r#"{"start":{"message":"seed"}}"#;

/// `defer_self_loop.json` 的入口输入。
const SELF_LOOP_INPUTS: &str = r#"{"start":{"seed":"seed"}}"#;

/// `defer_ab_cycle.json` / `defer_blocked.json` 的入口输入。
const CYCLE_INPUTS: &str = r#"{"start":{"seed":"seed"}}"#;

// ---------------------------------------------------------------------------
// 路径与 CLI / DB 辅助
// ---------------------------------------------------------------------------

/// `tests/fixtures/graphs/defer_gate`（子目录：根目录 JSON 会破坏 dsl golden 扫描）。
fn defer_gate_dir() -> PathBuf {
    graphs_root().join("defer_gate")
}

/// `tests/fixtures/graphs/defer_gate/<name>`。
fn defer_gate_graph(name: &str) -> PathBuf {
    defer_gate_dir().join(name)
}

/// `tests/fixtures/trace/<name>`。
fn trace_fixture(name: &str) -> PathBuf {
    repo_root().join("tests/fixtures/trace").join(name)
}

/// `tests/fixtures/trace/seed_graph.json`（v3 seed 生成图）。
fn seed_graph_path() -> PathBuf {
    trace_fixture("seed_graph.json")
}

/// 读图 JSON（断言 defer 形状用）。
fn read_graph(path: &Path) -> Value {
    let text = std::fs::read_to_string(path)
        .unwrap_or_else(|error| panic!("读取图 {} 失败: {error}", path.display()));
    serde_json::from_str(&text).unwrap_or_else(|error| panic!("图不是合法 JSON: {error}"))
}

/// 图边数组。
fn edges(graph: &Value) -> &Vec<Value> {
    graph["edges"].as_array().expect("graph.edges 数组")
}

/// 真 CLI `validate <graph>`（R1–R15；defer 图的 R13/R15 均在此覆盖）。
fn validate_graph(graph: &Path) -> Output {
    run_cli(&["validate".to_string(), arg(graph)], &repo_root())
}

/// 解析 `✓ graph valid: N nodes, M edges, ... (graph_hash=h)`。
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
    let hash = tokens
        .iter()
        .find_map(|token| token.find("graph_hash=").map(|index| &token[index..]))
        .map(|token| {
            token
                .trim_start_matches("graph_hash=")
                .trim_end_matches(')')
                .to_string()
        })
        .unwrap_or_else(|| panic!("validate 输出缺少 graph_hash: {line}"));
    (number_before("nodes,"), number_before("edges,"), hash)
}

/// 真 `zgraph run` 参数：defer 图（无 agent 资产；`--python-path` 提供 fixture stub）。
fn defer_run_args(graph: &Path, inputs: &str, db: &Path) -> Vec<String> {
    vec![
        "run".to_string(),
        arg(graph),
        "--inputs".to_string(),
        inputs.to_string(),
        "--db".to_string(),
        arg(db),
        "--python".to_string(),
        common::python_program(),
        "--python-path".to_string(),
        arg(&repo_root().join("python")),
        "--python-path".to_string(),
        arg(&defer_gate_dir().join("py")),
        "--json".to_string(),
    ]
}

/// 跑一次 defer 图，返回 CLI 输出与 trace DB 路径。
fn run_defer_graph(dir: &Path, name: &str, inputs: &str, tag: &str) -> (Output, PathBuf) {
    let db = dir.join(format!("{tag}.db"));
    let output = run_cli(&defer_run_args(&defer_gate_graph(name), inputs, &db), dir);
    (output, db)
}

/// 用 seed 图在给定 DB 上跑一次 run（顺带触发 v3→v4 迁移）。
fn run_seed_graph(dir: &Path, db: &Path) -> Output {
    run_cli(&defer_run_args(&seed_graph_path(), SEED_INPUTS, db), dir)
}

/// 读 SQL 文本并从零建库。
fn load_seed_sql(db: &Path) {
    let sql = std::fs::read_to_string(trace_fixture("schema_v3_seed.sql"))
        .expect("read schema_v3_seed.sql");
    let conn = rusqlite::Connection::open(db).expect("create seed db");
    conn.execute_batch(&sql)
        .expect("execute schema_v3_seed.sql");
}

/// 建一个固定化的 v3 seed 库，返回 `(db, old_run_id)`。
fn seeded_v3_db(dir: &Path) -> (PathBuf, String) {
    let db = dir.join("seed_v3.db");
    load_seed_sql(&db);
    let conn = cli_support::read_only_connect(&db);
    assert_eq!(user_version(&conn), 3, "seed 必须是 v3 库");
    let run_id: String = conn
        .query_row("SELECT run_id FROM runs", [], |row| row.get(0))
        .expect("seed has one run");
    assert_eq!(run_id, SEED_RUN_ID, "seed run id 必须固定");
    (db, run_id)
}

/// `PRAGMA user_version`。
fn user_version(conn: &rusqlite::Connection) -> i64 {
    conn.query_row("PRAGMA user_version", [], |row| row.get(0))
        .expect("read user_version")
}

/// 表列名（`PRAGMA table_info`）。
fn table_columns(conn: &rusqlite::Connection, table: &str) -> Vec<String> {
    let mut stmt = conn
        .prepare(&format!("PRAGMA table_info({table})"))
        .expect("prepare table_info");
    let rows = stmt
        .query_map([], |row| row.get::<_, String>(1))
        .expect("query columns");
    rows.map(|row| row.expect("column name")).collect()
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

/// NDJSON stdout → 事件数组（`trace --json` 契约：一行一个 JSON）。
fn ndjson_events(output: &Output) -> Vec<Value> {
    let stdout = String::from_utf8_lossy(&output.stdout);
    stdout
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str(line).expect("NDJSON line is JSON"))
        .collect()
}

/// 读冻结的 expected trace（JSON 数组）。
fn expected_trace() -> Vec<Value> {
    let text = std::fs::read_to_string(trace_fixture("v0.1.0_expected_trace.json"))
        .expect("read expected");
    serde_json::from_str(&text).expect("expected trace is JSON array")
}

// ---------------------------------------------------------------------------
// 1. fixtures 静态断言（R13/R15 + defer 形状）
// ---------------------------------------------------------------------------

/// 三个 defer fixture：位于子目录、`zgraph validate` 通过（R13/R15）、defer 边形状冻结。
#[test]
fn defer_fixtures_validate_with_r13_r15_and_expected_shape() {
    let cases: [(&str, usize, usize, usize); 3] = [
        ("defer_self_loop.json", 2, 2, 1),
        ("defer_ab_cycle.json", 3, 3, 2),
        ("defer_blocked.json", 3, 3, 2),
    ];
    for (name, node_count, edge_count, defer_count) in cases {
        // 子目录：根目录新增 JSON 会破坏 dsl `every_fixture_is_referenced` golden 扫描。
        assert!(
            !graphs_root().join(name).exists(),
            "{name} 不得放 graphs 根目录（dsl golden 扫描）"
        );
        let path = defer_gate_graph(name);
        assert!(path.is_file(), "missing fixture {}", path.display());
        let (actual_nodes, actual_edges, hash) = parse_validate_summary(&validate_graph(&path));
        assert_eq!(
            (actual_nodes, actual_edges),
            (node_count, edge_count),
            "{name} 规模"
        );
        assert_eq!(hash.len(), 16, "{name} graph_hash 应为 16 hex: {hash}");

        let graph = read_graph(&path);
        assert_eq!(graph["entry_points"], serde_json::json!(["start"]));
        let deferred: Vec<&Value> = edges(&graph)
            .iter()
            .filter(|edge| edge.get("on_gate_false").is_some())
            .collect();
        assert_eq!(deferred.len(), defer_count, "{name} defer 边数");
        for edge in deferred {
            assert_eq!(
                edge["on_gate_false"], "defer",
                "{name} 只接受冻结值 defer: {edge}"
            );
            assert!(
                edge.get("gate").is_some(),
                "{name} defer 边必须带 gate（R13 不豁免无 gate 环）: {edge}"
            );
        }
    }
}

// ---------------------------------------------------------------------------
// 2. AC-14 h：v3 seed → v4 迁移 / 只读拒绝 / L1 回放
// ---------------------------------------------------------------------------

/// h：v3 seed 由真 `zgraph run` 触发 v3→v4 迁移；备份/审计/旧行默认值/旧 payload 形态。
#[test]
fn v3_seed_is_migrated_to_v4_on_run_and_history_is_preserved() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (db, old_run_id) = seeded_v3_db(dir.path());
    assert!(
        backup_files(dir.path()).is_empty(),
        "迁移前不应存在 .bak 备份"
    );

    let output = run_seed_graph(dir.path(), &db);
    assert_exit(&output, 0, "v3 seed 上触发迁移的 run");
    let payload = json_stdout(&output);
    assert_eq!(payload["status"], serde_json::json!("Completed"));
    let new_run_id = payload["run_id"].as_str().expect("new run_id").to_string();
    assert_ne!(new_run_id, old_run_id, "迁移后的新 run 必须是新 id");

    let conn = cli_support::connect(&db);
    assert_eq!(user_version(&conn), 4, "迁移后 user_version=4");
    let versions: Vec<i64> = {
        let mut stmt = conn
            .prepare("SELECT version FROM schema_migrations ORDER BY version")
            .expect("prepare migrations");
        stmt.query_map([], |row| row.get(0))
            .expect("query migrations")
            .map(|row| row.expect("version"))
            .collect()
    };
    assert_eq!(versions, vec![1, 2, 3, 4], "schema_migrations 1..4");

    // 备份 = 迁移前 v3 快照（user_version=3、无 v4 列）。
    let backups = backup_files(dir.path());
    assert_eq!(backups.len(), 1, "v3→v4 恰一份备份: {backups:?}");
    let backup_name = backups[0]
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    assert!(
        backup_name.contains(".bak.v3-"),
        "备份文件名应含 .bak.v3-: {backup_name}"
    );
    let backup_conn = rusqlite::Connection::open(&backups[0]).expect("open backup");
    assert_eq!(user_version(&backup_conn), 3, "备份保持 v3");
    assert!(
        !table_columns(&backup_conn, "gate_results")
            .iter()
            .any(|column| column == "deferred"),
        "备份是加列前的快照"
    );

    // 旧行：只补 SQL 列默认值，不改写历史 events.payload（06 §6.2.8 / ISSUE-105 口径）。
    let old_columns: (i64, i64, Option<String>) = conn
        .query_row(
            "SELECT deferred, attempt, verdict_json FROM gate_results WHERE run_id = ?1",
            [&old_run_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .expect("old gate row");
    assert_eq!(old_columns, (0, 0, None), "旧行默认 0/0/NULL");
    let old_payload_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM events WHERE run_id = ?1 AND kind = 'GateResult'",
            [&old_run_id],
            |row| row.get(0),
        )
        .expect("old gate events");
    assert_eq!(old_payload_count, 1, "seed 恰一条历史 GateResult");
    drop(conn);

    let reader = TraceReader::open_readonly(&db).expect("reader");
    let old_events = reader
        .events(&old_run_id, &EventQuery::default())
        .expect("old events");
    let old_gate = old_events
        .iter()
        .find(|event| event.kind == "GateResult")
        .expect("old GateResult event");
    let old_record = old_gate.payload["record"]
        .as_object()
        .expect("old payload.record");
    for forbidden in ["deferred", "attempt", "verdict"] {
        assert!(
            !old_record.contains_key(forbidden),
            "历史 payload 不得合成 {forbidden}（trace --json 不合成默认键）"
        );
    }

    // L1 golden：旧 run 的 trace --json == 冻结 expected；replay events 与 trace 逐字节一致。
    assert_eq!(
        ndjson_events(&run_cli(
            &[
                "trace".to_string(),
                old_run_id.clone(),
                "--db".to_string(),
                arg(&db),
                "--json".to_string(),
            ],
            dir.path(),
        )),
        expected_trace(),
        "旧 run trace 必须与冻结 golden 一致"
    );
    let trace_out = run_cli(
        &[
            "trace".to_string(),
            old_run_id.clone(),
            "--db".to_string(),
            arg(&db),
            "--json".to_string(),
        ],
        dir.path(),
    );
    let replay_out = run_cli(
        &[
            "replay".to_string(),
            "--run".to_string(),
            old_run_id.clone(),
            "--db".to_string(),
            arg(&db),
            "--mode".to_string(),
            "events".to_string(),
            "--json".to_string(),
        ],
        dir.path(),
    );
    assert_exit(&trace_out, 0, "trace 旧 run");
    assert_exit(&replay_out, 0, "L1 replay 旧 run");
    assert_eq!(
        replay_out.stdout, trace_out.stdout,
        "L1 replay 与 trace 的 NDJSON 必须逐字节一致"
    );

    // v4 后新写入 run：payload 含三键（值为 W2a cli 真实接线，Phase 2B 再断言非默认值）。
    let new_events = reader
        .events(&new_run_id, &EventQuery::default())
        .expect("new events");
    let new_gate = new_events
        .iter()
        .find(|event| event.kind == "GateResult")
        .expect("new GateResult event");
    let new_record = new_gate.payload["record"]
        .as_object()
        .expect("new payload.record");
    assert!(
        new_record.get("deferred").is_some_and(Value::is_boolean),
        "v4 新事件 payload.record.deferred 必须是 bool: {new_record:?}"
    );
    assert!(
        new_record
            .get("attempt")
            .is_some_and(|value| value.as_u64().is_some()),
        "v4 新事件 payload.record.attempt 必须是整数: {new_record:?}"
    );
    assert!(
        new_record.contains_key("verdict"),
        "v4 新事件 payload.record.verdict 键必须存在（可 null）: {new_record:?}"
    );
    drop(reader);
}

/// h：只读 `trace` 对 v3 库拒绝且**不迁移**（exit 4，forward-only）。
#[test]
fn v3_seed_readonly_trace_is_rejected_without_migration() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (db, run_id) = seeded_v3_db(dir.path());
    let output = run_cli(
        &[
            "trace".to_string(),
            run_id,
            "--db".to_string(),
            arg(&db),
            "--json".to_string(),
        ],
        dir.path(),
    );
    assert_exit(&output, 4, "只读 trace 对 v3 库应 exit 4");
    let stderr = stderr_text(&output);
    assert!(
        stderr.contains("unsupported schema version=3, expected 4"),
        "stderr 应说明 v3 < 4 且不迁移: {stderr}"
    );
    let conn = cli_support::read_only_connect(&db);
    assert_eq!(user_version(&conn), 3, "只读拒绝不得迁移");
    let runs: i64 = conn
        .query_row("SELECT COUNT(*) FROM runs", [], |row| row.get(0))
        .expect("runs count");
    assert_eq!(runs, 1, "只读拒绝不得写库");
    assert!(
        backup_files(dir.path()).is_empty(),
        "只读拒绝不得生成迁移备份"
    );
}

// ---------------------------------------------------------------------------
// 3. AC-14 a/b/f/g 运行时用例（Phase 2A 先写全断言，等 W2a kernel 合并后打开）
// ---------------------------------------------------------------------------

/// 节点按 attempt 取全部实例行（按 attempt 升序）。
fn instances_of(reader: &TraceReader, run_id: &str, node_id: &str) -> Vec<InstanceRow> {
    let mut rows: Vec<InstanceRow> = reader
        .instances(run_id)
        .expect("query instances")
        .into_iter()
        .filter(|row| row.node_id == node_id)
        .collect();
    rows.sort_by_key(|row| row.attempt);
    rows
}

/// 某条边的 gate 记录（按 seq 升序）。
fn gate_rows_for_edge(
    reader: &TraceReader,
    run_id: &str,
    edge_index: i64,
) -> Vec<zgraph_trace::GateResultRow> {
    reader
        .gate_results(run_id)
        .expect("query gate_results")
        .into_iter()
        .filter(|row| row.edge_index == edge_index)
        .collect()
}

/// AC-14 a：defer 自环返工（attempt 0/1；同一自环边两行 gate；dormant 正常完成）。
#[test]
#[ignore = "W2a kernel（feat/kernel-deferrable-gate）未合并；Phase 2B 打开（AC-14 a）"]
fn self_loop_defer_reentry_completes_with_attempts_and_gate_rows() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (output, db) = run_defer_graph(dir.path(), "defer_self_loop.json", SELF_LOOP_INPUTS, "a");
    assert_exit(&output, 0, "defer 自环 run");
    let payload = json_stdout(&output);
    assert_eq!(payload["status"], serde_json::json!("Completed"));
    let run_id = payload["run_id"].as_str().expect("run_id").to_string();

    let reader = TraceReader::open_readonly(&db).expect("reader");
    let run = run_row(&reader, &run_id);
    assert_eq!(run.status, zgraph_trace::RunStatus::Completed);
    assert_eq!(run.error_code, None, "deferred 不得变 error");

    let loop_instances = instances_of(&reader, &run_id, "loop");
    assert_eq!(
        loop_instances
            .iter()
            .map(|row| row.attempt)
            .collect::<Vec<_>>(),
        vec![0, 1],
        "自环两次实例 attempt 0/1"
    );
    for row in &loop_instances {
        assert_eq!(row.status, TraceInstanceStatus::Completed);
    }
    let first_input = loop_instances[0].input.as_ref().expect("attempt0 input");
    let second_input = loop_instances[1].input.as_ref().expect("attempt1 input");
    assert!(
        second_input["seed"].is_object(),
        "attempt1 必须按端口继承 seed（settle 入边）: {second_input:?}"
    );
    assert_eq!(
        second_input["again"],
        loop_instances[0]
            .output_json
            .clone()
            .expect("attempt0 output"),
        "attempt1 的 again 必须是 attempt0 整包 output"
    );
    assert!(
        first_input["again"].is_null(),
        "attempt0 的 again 首轮为 null"
    );

    let gates = gate_rows_for_edge(&reader, &run_id, 1);
    assert_eq!(gates.len(), 2, "同一自环边两行 GateResult");
    assert!(gates[0].passed);
    assert!(!gates[0].deferred, "放行边 deferred=false");
    assert_eq!(gates[0].attempt, 0, "发布者实例 attempt");
    assert_eq!(gates[0].verdict, Some(serde_json::json!(true)));
    assert!(!gates[1].passed);
    assert!(gates[1].deferred, "gate=false + defer → deferred=true");
    assert_eq!(gates[1].attempt, 1);
    assert_eq!(gates[1].verdict, Some(serde_json::json!(false)));
}

/// AC-14 b：A→B→A 两条 gate 环边均 defer，attempt-per-activation 重入。
#[test]
#[ignore = "W2a kernel（feat/kernel-deferrable-gate）未合并；Phase 2B 打开（AC-14 b）"]
fn ab_cycle_defer_reentry_completes_with_attempts() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (output, db) = run_defer_graph(dir.path(), "defer_ab_cycle.json", CYCLE_INPUTS, "b");
    assert_exit(&output, 0, "A→B→A defer run");
    let payload = json_stdout(&output);
    assert_eq!(payload["status"], serde_json::json!("Completed"));
    let run_id = payload["run_id"].as_str().expect("run_id").to_string();

    let reader = TraceReader::open_readonly(&db).expect("reader");
    assert_eq!(
        run_row(&reader, &run_id).status,
        zgraph_trace::RunStatus::Completed
    );
    for node_id in ["A", "B"] {
        let rows = instances_of(&reader, &run_id, node_id);
        assert_eq!(
            rows.iter().map(|row| row.attempt).collect::<Vec<_>>(),
            vec![0, 1],
            "{node_id} attempt 0/1"
        );
        assert!(rows
            .iter()
            .all(|row| row.status == TraceInstanceStatus::Completed));
    }

    // A→B（edge 1）两行均放行；B→A（edge 2）第一行放行、第二行 false+deferred（dormant）。
    let a_to_b = gate_rows_for_edge(&reader, &run_id, 1);
    assert_eq!(a_to_b.len(), 2);
    assert!(a_to_b.iter().all(|row| row.passed && !row.deferred));
    let b_to_a = gate_rows_for_edge(&reader, &run_id, 2);
    assert_eq!(b_to_a.len(), 2);
    assert!(b_to_a[0].passed && !b_to_a[0].deferred);
    assert!(!b_to_a[1].passed && b_to_a[1].deferred);
    assert_eq!(b_to_a[1].attempt, 1);
}

/// AC-14 f：自环 gate 恒 false（defer）→ 只挂账、dormant，run 正常 Completed。
#[test]
#[ignore = "W2a kernel（feat/kernel-deferrable-gate）未合并；Phase 2B 打开（AC-14 f）"]
fn self_loop_defer_with_false_gate_completes_dormant() {
    let dir = tempfile::tempdir().expect("tempdir");
    // 变体：同一 fixture 把自环 gate 的 equals 改为 false（不新增第 4 个图文件）。
    let mut graph = read_graph(&defer_gate_graph("defer_self_loop.json"));
    graph["edges"][1]["gate"]["params"]["equals"] = serde_json::json!(false);
    let variant = cli_support::write_json(dir.path(), "dormant.json", &graph);
    let db = dir.path().join("f.db");
    let output = run_cli(&defer_run_args(&variant, SELF_LOOP_INPUTS, &db), dir.path());
    assert_exit(&output, 0, "dormant defer run");
    let payload = json_stdout(&output);
    assert_eq!(payload["status"], serde_json::json!("Completed"));
    let run_id = payload["run_id"].as_str().expect("run_id").to_string();

    let reader = TraceReader::open_readonly(&db).expect("reader");
    let run = run_row(&reader, &run_id);
    assert_eq!(run.status, zgraph_trace::RunStatus::Completed);
    assert_eq!(run.error_code, None, "dormant 不得误报 RUN_DEADLOCK");
    let loop_instances = instances_of(&reader, &run_id, "loop");
    assert_eq!(loop_instances.len(), 1, "gate 恒 false → 不重入");
    assert_eq!(loop_instances[0].attempt, 0);
    let gates = gate_rows_for_edge(&reader, &run_id, 1);
    assert_eq!(gates.len(), 1);
    assert!(!gates[0].passed && gates[0].deferred);
    assert_eq!(gates[0].verdict, Some(serde_json::json!(false)));
}

/// AC-14 g：defer 前进边目标永不实例化 → 静默后立即确定性 RUN_DEADLOCK。
#[test]
#[ignore = "W2a kernel（feat/kernel-deferrable-gate）未合并；Phase 2B 打开（AC-14 g）"]
fn blocked_deferred_receipt_fails_with_run_deadlock() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (output, db) = run_defer_graph(dir.path(), "defer_blocked.json", CYCLE_INPUTS, "g");
    assert_exit(&output, 1, "blocked defer run 应 exit 1");
    let payload = json_stdout(&output);
    assert_eq!(payload["ok"], serde_json::json!(false));
    assert_eq!(payload["status"], serde_json::json!("Failed"));
    assert_eq!(payload["error"]["code"], serde_json::json!("RUN_DEADLOCK"));
    let run_id = payload["run_id"].as_str().expect("run_id").to_string();

    let reader = TraceReader::open_readonly(&db).expect("reader");
    let run = run_row(&reader, &run_id);
    assert_eq!(run.status, zgraph_trace::RunStatus::Failed);
    assert_eq!(run.error_code.as_deref(), Some("RUN_DEADLOCK"));
    assert!(
        instances_of(&reader, &run_id, "b").is_empty(),
        "b 从未实例化（receipt blocked）"
    );
    let a_instances = instances_of(&reader, &run_id, "a");
    assert_eq!(a_instances.len(), 1);
    assert_eq!(a_instances[0].status, TraceInstanceStatus::Completed);

    let gates = reader.gate_results(&run_id).expect("gates");
    assert_eq!(gates.len(), 2, "自环 + a→b 各一条 deferred gate");
    assert!(gates.iter().all(|row| !row.passed && row.deferred));
    let error_message = run.error_message.as_deref().unwrap_or_default();
    assert!(
        error_message.contains("blocked") && error_message.contains("b"),
        "RUN_DEADLOCK message 应含 blocked 与 target node: {error_message}"
    );
}
