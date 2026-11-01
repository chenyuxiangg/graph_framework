//! W4b（AC-17 a–j）：动态 N / 节点级 `foreach` 展开的 e2e。
//!
//! 唯一语义依据：`doc/decisions/DR-007-w4a-dynamic-fanout-freeze.md`（§2 语法 / §3 R17-R18 /
//! §6 运行期模型 / §8 trace v6 / §11 AC-17 / §12 兼容矩阵）与 spec 07 AC-17。
//!
//! 数据通路（禁止 mock）：真 `zgraph` 二进制（真 kernel + 真 Python worker 子进程 +
//! 真 SQLite trace）→ TraceReader / 只读 SQL / `trace` / `replay` 断言。
//!
//! 本文件分两阶段交付（与 kernel 的「整包广播字段提取」fix 并行）：
//!
//! - **阶段 1（不依赖展开可达性）**：
//!   1. DSL 静态层：`valid_foreach.json` 通过（R17/R18）+ 冻结 hash；`bad_foreach_*`
//!      全部反例的 CLI 错误码/reason/location 冻结；`foreach` 参与 canonical hash（仅随
//!      图 JSON，缺省不补齐）；
//!   2. trace v5 seed：只读 `trace`/`replay` 对 v5 拒绝（exit 4，不迁移）；真 CLI `run`
//!      打开 v5 → v5→v6 迁移（备份/元数据/`instances` item 列/`foreach_expansions` 建表/
//!      旧 run 逐列保留）且旧 run `trace --json` 与「迁移前 v5-era CLI golden」逐字节一致；
//!   3. v3 seed → v3→v4→v5→v6 链式迁移，v0.1.0 L1 golden 不变；
//!   4. 无 foreach 默认图：hash/validate/run/trace 输出不变（无 `ForeachExpansion` 行、
//!      `foreach_expansions` 空、`instances.item_index=0`/`item_key=NULL`）。
//!
//! - **阶段 2（kernel fix 合并后）**：AC-17 a/b/c/d/e/f/g/h/j 真链路（entry 列表、上游列表、
//!   空列表、item 失败、key duplicate/canonical、max_items/not-array、取消传播、region 内
//!   per-item human、`RUN_FOREACH_ITEM_INCOMPLETE`）与 L1/L2 回放。
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

use rusqlite::{params, Connection};
use serde_json::{json, Value};

use zgraph_trace::{
    InstanceStatus as TraceInstanceStatus, RunStatus as TraceRunStatus, TraceReader,
};

use cli_support::{
    arg, assert_exit, json_stdout, python_program, read_only_connect, repo_root, run_cli,
    stderr_text, write_json,
};
use common::graphs_root;

// ---------------------------------------------------------------------------
// 常量与路径
// ---------------------------------------------------------------------------

/// `valid_foreach.json` 的冻结 `graph_hash`（`foreach` 显式参与 canonical projection）。
const VALID_FOREACH_HASH: &str = "10cd8fff9c8f85fb";

/// 无决策 run 的图（`tests/fixtures/trace/seed_graph.json`；v3/v4/v5 seed 共用）。
const SEED_GRAPH: &str = "seed_graph.json";

/// seed 图中的固定 run id（各 seed 同一归一化规则）。
const SEED_RUN_ID: &str = "0192f0c4-7a1b-7c3d-8e4f-000000000001";

/// seed 图入口输入。
const SEED_INPUTS: &str = r#"{"start":{"message":"seed"}}"#;

/// seed 图事件数（无 foreach 默认 run）。
const SEED_EVENTS: i64 = 7;

/// R18 反例（`code=DSL_FOREACH_INVALID`，`actual.reason` 闭集值）。
///
/// reason 闭集共 11 值；`region_overlap` 在冻结优先级下会先被 row8 `external_edge`
/// 拦截（dsl `rules.rs::r18_region_overlap_is_checked_after_boundary_rows` 已锁定该
/// 优先级），因此 e2e 通过 fixture 覆盖其余 10 个 reason。
const BAD_FOREACH_REGION_CASES: [(&str, &str); 10] = [
    ("bad_foreach_join_not_found.json", "join_not_found"),
    ("bad_foreach_join_in_region.json", "join_in_region"),
    ("bad_foreach_join_type.json", "join_type_unsupported"),
    (
        "bad_foreach_forbidden_node_type.json",
        "forbidden_node_type",
    ),
    ("bad_foreach_nested.json", "nested_foreach"),
    ("bad_foreach_no_exit.json", "no_exit"),
    ("bad_foreach_multiple_exits.json", "multiple_exits"),
    ("bad_foreach_external_edge.json", "external_edge"),
    ("bad_foreach_join_edge_gated.json", "join_edge_gated"),
    ("bad_foreach_items_port_missing.json", "items_port_missing"),
];

/// R17 形状反例（复用 `DSL_PARSE_ERROR`；message 片段冻结）。
const BAD_FOREACH_SHAPE_CASES: [(&str, &str); 8] = [
    (
        "bad_foreach_not_object.json",
        "field 'foreach' expects object",
    ),
    (
        "bad_foreach_unknown_key.json",
        "unknown field 'mode' in foreach of node 'F'",
    ),
    (
        "bad_foreach_missing_items_port.json",
        "foreach of node 'F' missing required field 'items_port'",
    ),
    (
        "bad_foreach_items_port_undeclared.json",
        "'foreach.items_port' of node 'F' must be a declared input port",
    ),
    (
        "bad_foreach_key_field.json",
        "'foreach.key_field' must be a non-empty string",
    ),
    ("bad_foreach_join.json", "'foreach.join' must be a node id"),
    (
        "bad_foreach_max_items.json",
        "'foreach.max_items' must be an integer >= 1",
    ),
    (
        "bad_foreach_on_item_error.json",
        "'foreach.on_item_error' must be one of ['fail']",
    ),
];

/// 默认（无 foreach）代表图与冻结 `graph_hash`（W4b 不改 DSL 投影的缺省路径）。
const DEFAULT_GRAPHS: [(&str, &str); 5] = [
    ("customer_ticket.json", "ec320e4244b365ab"),
    ("valid_parallel.json", "b15f162f7f87fd58"),
    ("valid_gate_defer.json", "2ff1339bd3d6d928"),
    ("valid_max_fanout.json", "e05d882cc55aa275"),
    ("valid_cycle.json", "0e8911b6b4003c7e"),
];

/// `tests/fixtures/graphs/<name>`。
fn graph_path(name: &str) -> PathBuf {
    graphs_root().join(name)
}

/// `tests/fixtures/trace/<name>`。
fn trace_fixture(name: &str) -> PathBuf {
    repo_root().join("tests/fixtures/trace").join(name)
}

/// 真 `zgraph validate <fixture> --json`。
fn validate_json(path: &Path) -> Output {
    run_cli(
        &["validate".to_string(), arg(path), "--json".to_string()],
        &repo_root(),
    )
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

/// 备份文件名中是否含指定版本前缀（如 `.bak.v5-`）。
fn has_backup_version(backups: &[PathBuf], version: &str) -> bool {
    let needle = format!(".bak.v{version}-");
    backups.iter().any(|path| {
        path.file_name()
            .map(|name| name.to_string_lossy().contains(&needle))
            .unwrap_or(false)
    })
}

/// `schema_migrations` 版本序列。
fn migration_versions(conn: &Connection) -> Vec<i64> {
    let mut statement = conn
        .prepare("SELECT version FROM schema_migrations ORDER BY version")
        .expect("prepare migrations");
    statement
        .query_map([], |row| row.get(0))
        .expect("query migrations")
        .map(|row| row.expect("version"))
        .collect()
}

/// 表/索引是否存在。
fn sqlite_object_exists(conn: &Connection, kind: &str, name: &str) -> bool {
    conn.query_row(
        "SELECT COUNT(*) FROM sqlite_master WHERE type = ?1 AND name = ?2",
        params![kind, name],
        |row| row.get::<_, i64>(0),
    )
    .expect("sqlite_master count")
        == 1
}

/// 表列名（`PRAGMA table_info`）。
fn table_columns(conn: &Connection, table: &str) -> Vec<String> {
    let mut statement = conn
        .prepare(&format!("PRAGMA table_info({table})"))
        .expect("prepare table_info");
    statement
        .query_map([], |row| row.get::<_, String>(1))
        .expect("query columns")
        .map(|row| row.expect("column name"))
        .collect()
}

/// 用 `schema_v5_seed.sql` 建真实 v5 库（返回 `(db, old_run_id)`）。
fn seeded_v5_db(dir: &Path) -> (PathBuf, String) {
    let sql = std::fs::read_to_string(trace_fixture("schema_v5_seed.sql"))
        .expect("read schema_v5_seed.sql");
    let db = dir.join("seed_v5.db");
    let conn = Connection::open(&db).expect("create v5 seed db");
    conn.execute_batch(&sql)
        .expect("execute schema_v5_seed.sql");
    assert_eq!(user_version(&conn), 5, "seed 必须是 v5 库");
    let run_id: String = conn
        .query_row("SELECT run_id FROM runs", [], |row| row.get(0))
        .expect("seed 有一条 run");
    assert_eq!(run_id, SEED_RUN_ID, "seed run id 必须固定");
    let events: i64 = conn
        .query_row("SELECT COUNT(*) FROM events", [], |row| row.get(0))
        .expect("events count");
    assert_eq!(events, SEED_EVENTS, "seed 事件数冻结");
    assert!(
        !table_columns(&conn, "instances")
            .iter()
            .any(|column| column == "item_index"),
        "v5 seed 不得含 v6 item 列"
    );
    (db, run_id)
}

/// 用 `schema_v3_seed.sql` 建真实 v3 库。
fn seeded_v3_db(dir: &Path) -> (PathBuf, String) {
    let sql = std::fs::read_to_string(trace_fixture("schema_v3_seed.sql"))
        .expect("read schema_v3_seed.sql");
    let db = dir.join("seed_v3.db");
    let conn = Connection::open(&db).expect("create v3 seed db");
    conn.execute_batch(&sql)
        .expect("execute schema_v3_seed.sql");
    assert_eq!(user_version(&conn), 3, "seed 必须是 v3 库");
    (db, SEED_RUN_ID.to_string())
}

/// v5 `instances` 的**原有列**逐列快照（迁移必须逐列保留；v6 新列的默认值另行断言）。
fn v5_instance_rows(conn: &Connection, run_id: &str) -> Vec<Vec<rusqlite::types::Value>> {
    let sql = "SELECT instance_id, run_id, node_id, node_type, attempt, status, input, \
               output_json, error, error_code, created_seq, started_at, finished_at, finished_seq \
               FROM instances WHERE run_id = ?1 ORDER BY instance_id";
    let mut statement = conn.prepare(sql).expect("prepare instances");
    let column_count = statement.column_count();
    let mut rows = statement.query([run_id]).expect("query instances");
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

/// 某 run 的事件行快照（迁移不改写 events：`seq/kind/severity/at/instance_id/node_id/payload`）。
fn event_rows(conn: &Connection, run_id: &str) -> Vec<Vec<rusqlite::types::Value>> {
    let sql = "SELECT seq, run_id, kind, severity, at, instance_id, node_id, payload \
               FROM events WHERE run_id = ?1 ORDER BY seq";
    let mut statement = conn.prepare(sql).expect("prepare events");
    let column_count = statement.column_count();
    let mut rows = statement.query([run_id]).expect("query events");
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

/// 在旧库上跑 seed 图（真 CLI `run`，顺带触发迁移到当前 v6）。
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

/// NDJSON stdout → 事件数组。
fn ndjson_lines(output: &Output) -> Vec<Value> {
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str(line).expect("NDJSON line is JSON"))
        .collect()
}

/// 冻结 golden（v5/v3 seed 迁移后旧 run 的 `trace --json`）。
fn expected_trace(name: &str) -> Vec<Value> {
    let text =
        std::fs::read_to_string(trace_fixture(name)).unwrap_or_else(|_| panic!("read {name}"));
    serde_json::from_str(&text).expect("expected trace is JSON array")
}

// ---------------------------------------------------------------------------
// 1. DSL 静态层：valid_foreach / bad_foreach_* / hash 仅随图 JSON
// ---------------------------------------------------------------------------

/// `valid_foreach.json` 通过 R17/R18，且 `graph_hash` 冻结（`foreach` 显式参与投影）。
#[test]
fn valid_foreach_passes_with_frozen_hash() {
    let output = validate_json(&graph_path("valid_foreach.json"));
    assert_exit(&output, 0, "valid_foreach validate");
    let payload = json_stdout(&output);
    assert_eq!(payload["ok"], json!(true));
    assert_eq!(payload["error_count"], json!(0));
    let hash = payload["graph_hash"].as_str().expect("graph_hash");
    assert_eq!(hash.len(), 16);
    assert_eq!(
        hash, VALID_FOREACH_HASH,
        "foreach 显式参与 canonical hash 且冻结"
    );
}

/// 22 个 `bad_foreach_*` 反例：退出码 2 + 错误码/定位/reason（R18）/message 片段（R17/R6）。
#[test]
fn bad_foreach_fixtures_have_frozen_codes_and_locations() {
    // R18：新码 DSL_FOREACH_INVALID，actual.reason 闭集，location.node_id=F。
    for (name, reason) in BAD_FOREACH_REGION_CASES {
        let output = validate_json(&graph_path(name));
        assert_exit(&output, 2, name);
        let payload = json_stdout(&output);
        assert_eq!(payload["error_count"], json!(1), "{name}: {payload}");
        let error = &payload["errors"][0];
        assert_eq!(error["code"], json!("DSL_FOREACH_INVALID"), "{name}");
        assert_eq!(
            error["message"],
            json!(format!("foreach region of node 'F' is invalid: {reason}")),
            "{name}"
        );
        assert_eq!(error["expected"], json!("valid foreach region"), "{name}");
        assert_eq!(error["actual"]["node_id"], json!("F"), "{name}");
        assert_eq!(error["actual"]["reason"], json!(reason), "{name}");
        assert_eq!(error["location"]["node_id"], json!("F"), "{name}");
    }

    // R17：复用 DSL_PARSE_ERROR（message 片段冻结）。
    for (name, message_fragment) in BAD_FOREACH_SHAPE_CASES {
        let output = validate_json(&graph_path(name));
        assert_exit(&output, 2, name);
        let payload = json_stdout(&output);
        assert_eq!(payload["error_count"], json!(1), "{name}: {payload}");
        let error = &payload["errors"][0];
        assert_eq!(error["code"], json!("DSL_PARSE_ERROR"), "{name}");
        let message = error["message"].as_str().expect("message");
        assert!(
            message.contains(message_fragment),
            "{name}: message {message:?} 应含 {message_fragment:?}"
        );
        assert!(
            error["location"]["node_id"].is_string(),
            "{name}: 节点级定位"
        );
    }

    // 节点类型不支持（R17）。
    let output = validate_json(&graph_path("bad_foreach_node_type.json"));
    assert_exit(&output, 2, "bad_foreach_node_type");
    assert_eq!(
        json_stdout(&output)["errors"][0]["message"],
        json!("node 'start' of type 'start' does not support foreach")
    );

    // 子图内 foreach → R6 收敛 DSL_SUBGRAPH_INVALID（message 前缀 subgraph）。
    let output = validate_json(&graph_path("bad_foreach_subgraph.json"));
    assert_exit(&output, 2, "bad_foreach_subgraph");
    let payload = json_stdout(&output);
    assert_eq!(payload["errors"][0]["code"], json!("DSL_SUBGRAPH_INVALID"));
    assert!(
        payload["errors"][0]["message"]
            .as_str()
            .is_some_and(|message| message.contains("subgraph 'inner'")),
        "{payload}"
    );
}

/// `graph_hash` 只随图 JSON：同图重复 validate hash 稳定；删除 `foreach` 后 hash 改变
/// （缺省不补齐/不参与投影）；`foreach` 相关运行期数据不进入 hash。
#[test]
fn graph_hash_depends_only_on_graph_json_including_foreach() {
    let first = validate_json(&graph_path("valid_foreach.json"));
    assert_exit(&first, 0, "valid_foreach validate #1");
    let hash = json_stdout(&first)["graph_hash"].clone();

    // 同一图的临时副本（字节相同）→ hash 相同。
    let source = std::fs::read_to_string(graph_path("valid_foreach.json")).expect("read graph");
    let dir = tempfile::tempdir().expect("tempdir");
    let copy = write_json(
        dir.path(),
        "copy.json",
        &serde_json::from_str::<Value>(&source).expect("graph JSON"),
    );
    let copied = validate_json(&copy);
    assert_exit(&copied, 0, "copy validate");
    assert_eq!(
        json_stdout(&copied)["graph_hash"],
        hash,
        "同一图 JSON hash 稳定"
    );

    // 仅删除 F 的 `foreach`（图仍合法：F 变普通 python_node）→ hash 必须改变。
    let mut stripped: Value = serde_json::from_str(&source).expect("graph JSON");
    stripped["nodes"]
        .as_array_mut()
        .expect("nodes")
        .iter_mut()
        .for_each(|node| {
            if node["id"] == json!("F") {
                node.as_object_mut().expect("node").remove("foreach");
            }
        });
    let stripped_path = write_json(dir.path(), "stripped.json", &stripped);
    let without = validate_json(&stripped_path);
    assert_exit(&without, 0, "无 foreach 副本仍合法");
    assert_ne!(
        json_stdout(&without)["graph_hash"],
        hash,
        "foreach 参与 canonical hash（删除后 hash 必须变化）"
    );
}

// ---------------------------------------------------------------------------
// 2. trace v5 seed：只读拒绝（不迁移）
// ---------------------------------------------------------------------------

/// AC-17 i：只读 `trace`/`replay` 对 v5 库 exit 4（expected 6）且不迁移。
#[test]
fn v5_seed_readonly_trace_and_replay_are_rejected_without_migration() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (db, run_id) = seeded_v5_db(dir.path());

    for (label, output) in [
        ("trace", trace_json(&db, &run_id)),
        ("replay", replay_events_json(&db, &run_id)),
    ] {
        assert_exit(&output, 4, &format!("只读 {label} 对 v5 库应 exit 4"));
        let stderr = stderr_text(&output);
        assert!(
            stderr.contains("unsupported schema version=5, expected 6"),
            "{label} stderr 应说明 v5 < 6 且不迁移: {stderr}"
        );
        assert!(
            String::from_utf8_lossy(&output.stdout).trim().is_empty(),
            "{label} --json 失败时 stdout 必须为空"
        );
    }
    let conn = read_only_connect(&db);
    assert_eq!(user_version(&conn), 5, "只读拒绝不得迁移");
    assert!(
        !sqlite_object_exists(&conn, "table", "foreach_expansions"),
        "只读拒绝不得建 v6 表"
    );
    assert!(
        backup_files(dir.path()).is_empty(),
        "只读拒绝不得生成迁移备份"
    );
}

// ---------------------------------------------------------------------------
// 3. trace v5→v6 迁移：备份/元数据/item 列/新表/历史行保留/回放 golden
// ---------------------------------------------------------------------------

/// AC-17 i：真 CLI `run` 打开 v5 库 → v5→v6 迁移；历史 run 逐列保留（item 列默认 0/NULL）、
/// `foreach_expansions` 建表、旧 run `trace --json` 与迁移前 v5-era golden 逐字节一致。
#[test]
fn v5_seed_migrates_to_v6_preserving_history_and_replay() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (db, old_run_id) = seeded_v5_db(dir.path());
    let before_instances = {
        let conn = read_only_connect(&db);
        v5_instance_rows(&conn, &old_run_id)
    };
    let before_events = {
        let conn = read_only_connect(&db);
        event_rows(&conn, &old_run_id)
    };

    let output = run_seed_graph(&db, dir.path());
    assert_exit(&output, 0, "v5 seed 上触发迁移的 run");
    let payload = json_stdout(&output);
    assert_eq!(payload["status"], json!("Completed"));
    assert_ne!(
        payload["run_id"].as_str().expect("new run_id"),
        old_run_id,
        "迁移后的新 run 必须是新 id"
    );

    let conn = read_only_connect(&db);
    assert_eq!(user_version(&conn), 6, "迁移后 user_version=6");
    assert_eq!(
        migration_versions(&conn),
        vec![1, 2, 3, 4, 5, 6],
        "schema_migrations 1..6"
    );

    // 备份恰一份 v5 快照。
    let backups = backup_files(dir.path());
    assert_eq!(backups.len(), 1, "v5→v6 恰一份备份: {backups:?}");
    assert!(has_backup_version(&backups, "5"), "备份应含 .bak.v5-");
    let backup_conn = Connection::open(&backups[0]).expect("open backup");
    assert_eq!(user_version(&backup_conn), 5, "备份保持 v5");

    // v6 新列/新表/索引。
    let instance_columns = table_columns(&conn, "instances");
    assert!(instance_columns.iter().any(|column| column == "item_index"));
    assert!(instance_columns.iter().any(|column| column == "item_key"));
    assert!(sqlite_object_exists(&conn, "table", "foreach_expansions"));
    assert!(sqlite_object_exists(
        &conn,
        "index",
        "idx_foreach_expansions_run"
    ));

    // 旧 run 历史行逐列保留；v6 新列默认 item_index=0 / item_key NULL。
    assert_eq!(
        v5_instance_rows(&conn, &old_run_id),
        before_instances,
        "迁移必须逐列保留旧 instances 行"
    );
    assert_eq!(
        event_rows(&conn, &old_run_id),
        before_events,
        "迁移不得改写 events"
    );
    let item_defaults: Vec<(i64, Option<String>)> = {
        let mut statement = conn
            .prepare(
                "SELECT item_index, item_key FROM instances WHERE run_id = ?1 ORDER BY instance_id",
            )
            .expect("prepare item defaults");
        statement
            .query_map([&old_run_id], |row| Ok((row.get(0)?, row.get(1)?)))
            .expect("query item defaults")
            .map(|row| row.expect("item default row"))
            .collect()
    };
    assert!(
        item_defaults
            .iter()
            .all(|(item_index, item_key)| *item_index == 0 && item_key.is_none()),
        "旧行 item_index=0 / item_key=NULL: {item_defaults:?}"
    );
    drop(conn);

    // 旧 run 回放：与迁移前 v5-era golden 一致；replay == trace 逐字节。
    let trace = trace_json(&db, &old_run_id);
    assert_exit(&trace, 0, "迁移后 trace 旧 run");
    assert_eq!(
        ndjson_lines(&trace),
        expected_trace("v5_expected_trace.json"),
        "v5→v6 迁移不得改写历史事件（与迁移前 v5 golden 逐字节一致）"
    );
    let replay = replay_events_json(&db, &old_run_id);
    assert_exit(&replay, 0, "迁移后 replay 旧 run");
    assert_eq!(replay.stdout, trace.stdout, "replay 与 trace 逐字节一致");

    // 无 foreach run：不产生 expansion；新 run 的 item 列默认 0/NULL。
    let reader = TraceReader::open_readonly(&db).expect("open trace");
    let new_run_id = json_stdout(&output)["run_id"]
        .as_str()
        .expect("new run_id")
        .to_string();
    assert!(
        reader
            .foreach_expansions(&new_run_id)
            .expect("expansions")
            .is_empty(),
        "无 foreach run 不得有 expansion 行"
    );
    assert!(
        reader
            .instances(&new_run_id)
            .expect("instances")
            .iter()
            .all(|row| row.item_index == 0 && row.item_key.is_none()),
        "无 foreach run 的 item 列默认 0/NULL"
    );
}

// ---------------------------------------------------------------------------
// 4. v3 seed → v3→v4→v5→v6 链式迁移：v0.1.0 L1 golden 不变
// ---------------------------------------------------------------------------

/// AC-17 i：v3 seed → 真 CLI run 触发 v3→v4→v5→v6 链式迁移；v0.1.0 L1 golden 不变、
/// 逐版本备份 3/4/5 各一份、旧 payload 保持 v3 形态。
#[test]
fn v3_seed_chain_to_v6_preserves_v01_golden() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (db, old_run_id) = seeded_v3_db(dir.path());

    // 只读 v3：expected 6（不迁移）。
    let readonly = trace_json(&db, &old_run_id);
    assert_exit(&readonly, 4, "只读 trace 对 v3 库应 exit 4");
    assert!(
        stderr_text(&readonly).contains("unsupported schema version=3, expected 6"),
        "stderr: {}",
        stderr_text(&readonly)
    );

    let output = run_seed_graph(&db, dir.path());
    assert_exit(&output, 0, "v3 seed 上触发链式迁移的 run");
    assert_eq!(json_stdout(&output)["status"], json!("Completed"));

    let conn = read_only_connect(&db);
    assert_eq!(user_version(&conn), 6, "迁移后 user_version=6");
    assert_eq!(migration_versions(&conn), vec![1, 2, 3, 4, 5, 6]);
    assert!(sqlite_object_exists(&conn, "table", "foreach_expansions"));
    drop(conn);

    // 链式迁移每版本恰一份备份（v3→v4、v4→v5、v5→v6）。
    let backups = backup_files(dir.path());
    for version in ["3", "4", "5"] {
        assert!(
            has_backup_version(&backups, version),
            "链式迁移应含 .bak.v{version}-*: {backups:?}"
        );
    }

    // v0.1.0 L1 golden 不变；replay == trace。
    let trace = trace_json(&db, &old_run_id);
    assert_exit(&trace, 0, "迁移后 trace 旧 run");
    assert_eq!(
        ndjson_lines(&trace),
        expected_trace("v0.1.0_expected_trace.json"),
        "v3→v6 链式迁移后 v0.1.0 L1 golden 必须不变"
    );
    let replay = replay_events_json(&db, &old_run_id);
    assert_exit(&replay, 0, "迁移后 replay 旧 run");
    assert_eq!(replay.stdout, trace.stdout, "replay 与 trace 逐字节一致");

    // 旧 payload 保持 v3 形态（不因链式迁移被改写）。
    let reader = TraceReader::open_readonly(&db).expect("open trace");
    let events = reader
        .events(&old_run_id, &Default::default())
        .expect("events");
    let gate_event = events
        .iter()
        .find(|event| event.kind == "GateResult")
        .expect("旧 run 有 GateResult");
    assert!(
        gate_event.payload["record"].get("deferred").is_none(),
        "旧 payload 不得被改写为 v4 形态: {:?}",
        gate_event.payload
    );
}

// ---------------------------------------------------------------------------
// 5. 默认（无 foreach）图：hash/validate/run/trace 不变
// ---------------------------------------------------------------------------

/// AC-17 i / §12：无 foreach 代表图 `graph_hash`/validate 结果不变（W4b 不改缺省投影）。
#[test]
fn default_graph_hashes_and_validate_unchanged() {
    for (name, expected_hash) in DEFAULT_GRAPHS {
        let output = validate_json(&graph_path(name));
        assert_exit(&output, 0, &format!("默认图 {name} validate"));
        let payload = json_stdout(&output);
        assert_eq!(payload["ok"], json!(true), "{name}");
        assert_eq!(
            payload["graph_hash"],
            json!(expected_hash),
            "{name}: W4b 不得改变无 foreach 图的 graph_hash"
        );
    }
}

/// AC-17 i / §12：无 foreach 图的真 run/trace 输出不变——无 `ForeachExpansion` 行、
/// `foreach_expansions` 空、`instances.item_index=0`/`item_key=NULL`、replay == trace。
#[test]
fn default_run_has_no_foreach_artifacts() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db = dir.path().join("default.db");
    let output = run_seed_graph(&db, dir.path());
    assert_exit(&output, 0, "默认图 run");
    let payload = json_stdout(&output);
    assert_eq!(payload["status"], json!("Completed"));
    let run_id = payload["run_id"].as_str().expect("run_id").to_string();

    let reader = TraceReader::open_readonly(&db).expect("open trace");
    assert!(
        reader
            .foreach_expansions(&run_id)
            .expect("expansions")
            .is_empty(),
        "无 foreach run 不得有 expansion 行"
    );
    let instances = reader.instances(&run_id).expect("instances");
    assert!(!instances.is_empty(), "seed 图必须有实例");
    assert!(
        instances
            .iter()
            .all(|row| row.item_index == 0 && row.item_key.is_none()),
        "无 foreach run 的 item 列默认 0/NULL: {instances:?}"
    );
    assert!(
        instances
            .iter()
            .all(|row| row.status != TraceInstanceStatus::WaitingHuman),
        "无 human 图不得 WaitingHuman"
    );
    let run_row = common::run_row(&reader, &run_id);
    assert_eq!(run_row.status, TraceRunStatus::Completed);
    drop(reader);

    let trace = trace_json(&db, &run_id);
    assert_exit(&trace, 0, "默认 run trace --json");
    let lines = ndjson_lines(&trace);
    assert_eq!(lines.len(), SEED_EVENTS as usize);
    assert!(
        lines
            .iter()
            .all(|line| line["kind"] != json!("ForeachExpansion")),
        "无 foreach run 的 trace --json 不得追加 ForeachExpansion 行"
    );
    let replay = replay_events_json(&db, &run_id);
    assert_exit(&replay, 0, "默认 run replay");
    assert_eq!(replay.stdout, trace.stdout, "replay 与 trace 逐字节一致");
}
