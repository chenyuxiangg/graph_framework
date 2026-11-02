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
//! - **阶段 2（AC-17 a–j 真链路，kernel「items_port 整包字段提取」fix 合并后）**：
//!   entry 列表 3 items（item 实例/有序 join/C2 join 来源）、上游列表决定 N、空列表
//!   （N=0：无 item 实例/expansion count=0/join `[]`）、item 失败（节点错误码 + 其它 item
//!   Cancelled）、key duplicate 与 canonical（number/bool/null/object/array/string）、
//!   max_items/not-array（含 unwrap 三变体）、取消传播（SIGINT）、region 内 per-item human
//!   （2×WaitingHuman 分别 decide + 有序 join + `:memory:` 拒绝）、E Skipped →
//!   `RUN_FOREACH_ITEM_INCOMPLETE`、controller 全部 settle=false → Skipped；trace
//!   `ForeachExpansion` 行顺序、`--stats` 不新增字段、L1 replay 逐字节一致、L2 per-item
//!   outputs 回放、旧 v5 库迁移后端到端 foreach + L2。
//!
//! 稳定性约定：每用例独立 `tempfile::TempDir`/trace DB；等待用轮询 + 宽松上限，不做 sleep
//! 猜测；不新增 ignore、不放宽既有断言。

#![cfg(unix)]

#[path = "cli_support/mod.rs"]
mod cli_support;
#[path = "common/mod.rs"]
mod common;

use std::path::{Path, PathBuf};
use std::process::{Child, Output, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use rusqlite::{params, Connection, OpenFlags};
use serde_json::{json, Value};

use zgraph_trace::{
    ForeachExpansionRow, GateResultRow, InstanceRow, InstanceStatus as TraceInstanceStatus,
    RunStatus as TraceRunStatus, TraceReader,
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

/// AC-17 i：只读 `trace`/`replay` 对 v5 库 exit 4（expected 7）且不迁移。
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
            stderr.contains("unsupported schema version=5, expected 7"),
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
    assert_eq!(user_version(&conn), 7, "迁移后 user_version=7");
    assert_eq!(
        migration_versions(&conn),
        vec![1, 2, 3, 4, 5, 6, 7],
        "schema_migrations 1..7"
    );

    // 备份 = 逐版本迁移前快照（v5→v6、v6→v7 各一份）。
    let backups = backup_files(dir.path());
    assert_eq!(backups.len(), 2, "v5→v7 逐版本两份备份: {backups:?}");
    assert!(has_backup_version(&backups, "5"), "备份应含 .bak.v5-");
    assert!(has_backup_version(&backups, "6"), "备份应含 .bak.v6-");
    let backup = backups
        .iter()
        .find(|path| {
            path.file_name()
                .map(|name| name.to_string_lossy().contains(".bak.v5-"))
                .unwrap_or(false)
        })
        .unwrap_or_else(|| panic!("missing .bak.v5- backup: {backups:?}"));
    let backup_conn = Connection::open(backup).expect("open backup");
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
// 4. v3 seed → v3→…→v7 链式迁移：v0.1.0 L1 golden 不变
// ---------------------------------------------------------------------------

/// AC-17 i：v3 seed → 真 CLI run 触发 v3→…→v7 链式迁移；v0.1.0 L1 golden 不变、
/// 逐版本备份 3/4/5/6 各一份、旧 payload 保持 v3 形态。
#[test]
fn v3_seed_chain_to_v6_preserves_v01_golden() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (db, old_run_id) = seeded_v3_db(dir.path());

    // 只读 v3：expected 7（不迁移）。
    let readonly = trace_json(&db, &old_run_id);
    assert_exit(&readonly, 4, "只读 trace 对 v3 库应 exit 4");
    assert!(
        stderr_text(&readonly).contains("unsupported schema version=3, expected 7"),
        "stderr: {}",
        stderr_text(&readonly)
    );

    let output = run_seed_graph(&db, dir.path());
    assert_exit(&output, 0, "v3 seed 上触发链式迁移的 run");
    assert_eq!(json_stdout(&output)["status"], json!("Completed"));

    let conn = read_only_connect(&db);
    assert_eq!(user_version(&conn), 7, "迁移后 user_version=7");
    assert_eq!(migration_versions(&conn), vec![1, 2, 3, 4, 5, 6, 7]);
    assert!(sqlite_object_exists(&conn, "table", "foreach_expansions"));
    drop(conn);

    // 链式迁移每版本恰一份备份（v3→v4 … v6→v7）。
    let backups = backup_files(dir.path());
    for version in ["3", "4", "5", "6"] {
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

// ===========================================================================
// 阶段 2：AC-17 a–j 真链路（真 CLI + 真 Python worker + 真 SQLite）
// ===========================================================================
//
// kernel `items_port` 取值（Lead 裁决 A / REV-092）：端口值是 array → 直接用；是 object
// 且含**与 `items_port` 同名键** → 取该键值（必须 array）；否则 NOT_ARRAY。下方图按此
// 构造：entry `{"items":[...]}` / 上游 python 输出 `{"items":[...]}`。

/// 子进程等待上限（真 worker 冷启动 + 取消/决策轮询余量）。
const CHILD_TIMEOUT: Duration = Duration::from_secs(30);

/// 挂起/可见性轮询间隔。
const POLL_INTERVAL: Duration = Duration::from_millis(10);

/// `tests/fixtures/graphs/foreach_dynamic/<name>`。
fn foreach_fixture(name: &str) -> PathBuf {
    graphs_root().join("foreach_dynamic").join(name)
}

/// e2e fixture worker 目录（`foreach_dynamic` 模块；真 CLI 以 `--python-path` 注入）。
fn foreach_python_path() -> PathBuf {
    repo_root().join("tests/e2e/fixtures/py")
}

/// 真 CLI `run` 参数（真 Python worker：仓库 `python/` + e2e fixture `py/`）。
fn foreach_run_args(db: &Path, graph: &Path, inputs: &str, record_full: bool) -> Vec<String> {
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
        "--python-path".to_string(),
        arg(&foreach_python_path()),
    ];
    if record_full {
        args.push("--record=full".to_string());
    }
    args.push("--json".to_string());
    args
}

/// 同步跑一次 `zgraph run <foreach_dynamic 图>`。
fn run_foreach(cwd: &Path, db: &Path, fixture: &str, inputs: &str, record_full: bool) -> Output {
    run_cli(
        &foreach_run_args(db, &foreach_fixture(fixture), inputs, record_full),
        cwd,
    )
}

/// 挂起/取消场景的 `zgraph run` 子进程守卫（Drop 兜底 kill）。
struct ForeachRun {
    child: Option<Child>,
}

impl ForeachRun {
    /// 启动 `zgraph run`（piped stdout/stderr）；`record_full` 控制 L2 记录。
    fn spawn(cwd: &Path, db: &Path, fixture: &str, inputs: &str, record_full: bool) -> Self {
        let child = cli_support::zgraph_command()
            .args(foreach_run_args(
                db,
                &foreach_fixture(fixture),
                inputs,
                record_full,
            ))
            .current_dir(cwd)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn zgraph run");
        Self { child: Some(child) }
    }

    /// run 进程 pid。
    fn pid(&self) -> u32 {
        self.child.as_ref().expect("child alive").id()
    }

    /// 等待退出（超时 SIGKILL 后 panic）。
    fn wait(mut self, context: &str) -> Output {
        let child = self.child.take().expect("child");
        wait_child_output(child, context, CHILD_TIMEOUT)
    }
}

impl Drop for ForeachRun {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

/// 等待子进程退出（读线程 + 超时 SIGKILL，不静默跳过）。
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

/// 对 run 子进程发 SIGINT（CLI 取消路径 → exit 130）。
fn send_sigint(pid: u32) {
    nix::sys::signal::kill(
        nix::unistd::Pid::from_raw(pid as i32),
        nix::sys::signal::Signal::SIGINT,
    )
    .expect("send SIGINT to zgraph run");
}

/// 只读打开 trace DB（未就绪/暂不可读 → `None`）。
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

/// 轮询直到 `node_id` 有 `count` 个 `WaitingHuman` item 实例可见；
/// 返回按 `item_index` 升序的 `(item_index, instance_id)`。
fn poll_waiting_items(db: &Path, node_id: &str, count: usize) -> Vec<(u32, String)> {
    let deadline = Instant::now() + CHILD_TIMEOUT;
    loop {
        if let Some(conn) = try_read_only(db) {
            // DB 文件可能已创建但 schema/迁移尚未落库（CLI 启动早期）→ 容忍并继续轮询。
            if let Ok(mut statement) = conn.prepare(
                "SELECT item_index, instance_id FROM instances \
                 WHERE node_id = ?1 AND status = 'WaitingHuman' ORDER BY item_index",
            ) {
                if let Ok(iter) =
                    statement.query_map([node_id], |row| Ok((row.get(0)?, row.get(1)?)))
                {
                    let rows: Vec<(u32, String)> = iter.filter_map(Result::ok).collect();
                    if rows.len() >= count {
                        return rows;
                    }
                }
            }
        }
        assert!(
            Instant::now() < deadline,
            "node {node_id} 未在 {CHILD_TIMEOUT:?} 内出现 {count} 个 WaitingHuman item"
        );
        std::thread::sleep(POLL_INTERVAL);
    }
}

/// 轮询直到 `node_id` 有 item 实例处于 `Running`（取消传播用例的稳定触发点）。
fn poll_item_running(db: &Path, node_id: &str) {
    let deadline = Instant::now() + CHILD_TIMEOUT;
    loop {
        if let Some(conn) = try_read_only(db) {
            let running: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM instances WHERE node_id = ?1 AND status = 'Running'",
                    [node_id],
                    |row| row.get(0),
                )
                .unwrap_or(0);
            if running >= 1 {
                return;
            }
        }
        assert!(
            Instant::now() < deadline,
            "node {node_id} 未在 {CHILD_TIMEOUT:?} 内进入 Running"
        );
        std::thread::sleep(POLL_INTERVAL);
    }
}

/// 某 run 的展开事实（按 `(at, expansion_id)` 升序）。
fn expansions(reader: &TraceReader, run_id: &str) -> Vec<ForeachExpansionRow> {
    reader
        .foreach_expansions(run_id)
        .expect("query foreach_expansions")
}

/// 某节点的全部 item 实例（按 `item_index` 升序）。
fn item_instances(reader: &TraceReader, run_id: &str, node_id: &str) -> Vec<InstanceRow> {
    let mut rows: Vec<InstanceRow> = reader
        .instances(run_id)
        .expect("query instances")
        .into_iter()
        .filter(|row| row.node_id == node_id)
        .collect();
    rows.sort_by_key(|row| (row.item_index, row.attempt));
    rows
}

/// 某节点的 gate_results（按 seq/插入序）。
fn node_gates(reader: &TraceReader, run_id: &str, node_id: &str) -> Vec<GateResultRow> {
    reader
        .gate_results(run_id)
        .expect("query gate_results")
        .into_iter()
        .filter(|gate| gate.node_id == node_id)
        .collect()
}

/// 真 `zgraph decide`（复用 W3a；每 item 独立 instance_id）。
fn decide_item(
    cwd: &Path,
    db: &Path,
    run_id: &str,
    instance_id: &str,
    decision: &str,
    approver: &str,
    reason: Option<&str>,
) -> Output {
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
        "--json".to_string(),
    ];
    if let Some(reason) = reason {
        args.push("--reason".to_string());
        args.push(reason.to_string());
    }
    run_cli(&args, cwd)
}

/// `trace --json` 的 kind 序列。
fn trace_kinds(output: &Output) -> Vec<String> {
    ndjson_lines(output)
        .into_iter()
        .map(|line| line["kind"].as_str().expect("kind string").to_string())
        .collect()
}

/// 展开行的 `keys`（`None` → `Null` 语义）。
fn expansion_keys(row: &ForeachExpansionRow) -> Value {
    row.keys.clone().unwrap_or(Value::Null)
}

// ---------------------------------------------------------------------------
// AC-17 a：entry 列表 3 items 展开 + 有序 join + trace/L1/L2
// ---------------------------------------------------------------------------

/// AC-17 a：entry `start.items` 3 items → F/W/E 各 3 个 item 实例（0..2）、join 有序数组、
/// run Completed；expansion（count/keys/source=start 实例）、C2 的 join 来源=最后完成的
/// E item、trace ForeachExpansion 行、L1 replay 逐字节一致、L2 per-item outputs 回放匹配。
#[test]
fn ac17a_entry_items_expand_per_item_and_join_in_order() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db = dir.path().join("a.db");
    // item1 延迟 500ms → 最后完成（C2 join 来源确定；余量抵御并行负载下的 worker 冷启动抖动）。
    let inputs = r#"{"start":{"items":[{"id":"a"},{"id":"b","delay_ms":500},{"id":"c"}]}}"#;
    let output = run_foreach(dir.path(), &db, "entry_items3.json", inputs, true);
    assert_exit(&output, 0, "AC-17 a run");
    let payload = json_stdout(&output);
    assert_eq!(payload["status"], json!("Completed"));
    let run_id = payload["run_id"].as_str().expect("run_id").to_string();
    let expected_results = json!([
        {"item": {"id": "a"}, "ok": true},
        {"item": {"id": "b", "delay_ms": 500}, "ok": true},
        {"item": {"id": "c"}, "ok": true},
    ]);
    assert_eq!(
        payload["run_output"],
        json!({"result": {"count": 3, "results": expected_results}}),
        "join 数组必须按 item_index 升序"
    );

    let reader = TraceReader::open_readonly(&db).expect("open trace");
    // expansion：count=3、keys 全按索引、source=start 实例（唯一入边投递来源）。
    let expansion_rows = expansions(&reader, &run_id);
    assert_eq!(expansion_rows.len(), 1, "恰一次展开");
    assert_eq!(expansion_rows[0].node_id, "F");
    assert_eq!(expansion_rows[0].item_count, 3);
    assert_eq!(
        expansion_keys(&expansion_rows[0]),
        json!([{"$index": 0}, {"$index": 1}, {"$index": 2}])
    );
    let start_instance = item_instances(&reader, &run_id, "start")
        .into_iter()
        .next()
        .expect("start 实例");
    assert_eq!(
        expansion_rows[0].source_instance_id.as_deref(),
        Some(start_instance.instance_id.as_str()),
        "expansion.source_instance_id = 使 controller Ready 的投递来源（start 实例）"
    );

    // item 实例：F/W/E 各 item_index 0..2 全部 Completed、attempt=0、无 key_field → NULL。
    for node in ["F", "W", "E"] {
        let rows = item_instances(&reader, &run_id, node);
        assert_eq!(
            rows.iter().map(|row| row.item_index).collect::<Vec<u32>>(),
            vec![0, 1, 2],
            "{node} item_index"
        );
        assert!(
            rows.iter()
                .all(|row| row.status == TraceInstanceStatus::Completed && row.attempt == 0),
            "{node} 全部 Completed/attempt=0"
        );
        assert!(
            rows.iter().all(|row| row.item_key.is_none()),
            "{node} 无 key_field → item_key NULL"
        );
    }
    // 无 controller「非 item」实例：F 的实例全部是 item 实例。
    assert_eq!(item_instances(&reader, &run_id, "F").len(), 3);

    // join：J 的 input/result 有序；end 聚合。
    let join = item_instances(&reader, &run_id, "J")
        .into_iter()
        .next()
        .expect("J 实例");
    assert_eq!(join.item_index, 0, "join 节点为普通实例 item_index=0");
    assert_eq!(
        join.input.clone().expect("J input")["results"],
        expected_results,
        "J 收到的 results = 有序 E 输出数组"
    );
    let end_input = item_instances(&reader, &run_id, "end")
        .into_iter()
        .next()
        .expect("end 实例")
        .input
        .expect("end input");
    assert_eq!(end_input["result"]["results"], expected_results);

    // C2：E→J 的 GateResult 来源 = 最后完成的 E item（item1，delay 120ms）。
    let e_items = item_instances(&reader, &run_id, "E");
    let join_gates: Vec<GateResultRow> = node_gates(&reader, &run_id, "E")
        .into_iter()
        .filter(|gate| gate.to_node == "J")
        .collect();
    assert_eq!(join_gates.len(), 1, "join 投递恰一条 GateResult");
    let gate = &join_gates[0];
    assert_eq!(gate.attempt, 0);
    let last = e_items
        .iter()
        .max_by(|left, right| {
            left.finished_at
                .cmp(&right.finished_at)
                .then(left.item_index.cmp(&right.item_index))
        })
        .expect("E item 实例");
    assert_eq!(
        last.item_index,
        1,
        "item1（delay 500ms）应是最后完成的 E item: {:?}",
        e_items
            .iter()
            .map(|row| (row.item_index, row.finished_at.clone()))
            .collect::<Vec<_>>()
    );
    assert_eq!(
        gate.instance_id, last.instance_id,
        "C2：join GateResult.instance_id = 最后完成使 join 满 N 的 E item 实例"
    );
    assert!(item_instances(&reader, &run_id, "W")
        .iter()
        .all(|row| row.status == TraceInstanceStatus::Completed));
    drop(reader);

    // trace --json：事件行之后追加恰一条 ForeachExpansion（不写 events.kind）。
    let trace = trace_json(&db, &run_id);
    assert_exit(&trace, 0, "AC-17 a trace --json");
    let kinds = trace_kinds(&trace);
    assert_eq!(
        kinds
            .iter()
            .filter(|kind| *kind == "ForeachExpansion")
            .count(),
        1,
        "ForeachExpansion 恰一行: {kinds:?}"
    );
    assert_eq!(
        kinds.last().map(String::as_str),
        Some("ForeachExpansion"),
        "展开行追加在事件行之后: {kinds:?}"
    );
    let expansion_line = ndjson_lines(&trace).pop().expect("last line");
    assert_eq!(expansion_line["node_id"], json!("F"));
    assert_eq!(expansion_line["item_count"], json!(3));
    assert_eq!(
        expansion_line["source_instance_id"],
        json!(start_instance.instance_id)
    );
    assert_eq!(
        expansion_line["keys"],
        json!([{"$index": 0}, {"$index": 1}, {"$index": 2}])
    );
    assert!(expansion_line["expansion_id"].is_string());

    // L1 replay 与 trace --json 逐字节一致。
    let replay = replay_events_json(&db, &run_id);
    assert_exit(&replay, 0, "AC-17 a replay events");
    assert_eq!(replay.stdout, trace.stdout, "L1 replay 与 trace 逐字节一致");

    // L2 per-item outputs：--record=full 下 F/W/E 每 item + J 均有记录，重放匹配。
    let reader = TraceReader::open_readonly(&db).expect("open trace");
    let records = reader.recorded_outputs(&run_id).expect("recorded outputs");
    assert_eq!(records.len(), 10, "F/W/E × 3 items + J = 10 条 L2 记录");
    drop(reader);
    let l2 = run_cli(
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
    assert_exit(&l2, 0, "AC-17 a L2 outputs replay");
    let l2_payload = json_stdout(&l2);
    assert_eq!(l2_payload["ok"], json!(true));
    assert_eq!(l2_payload["matched"], json!(true));
    assert_eq!(l2_payload["recorded_outputs"], json!(10));
    assert_eq!(
        l2_payload["original_output"], l2_payload["replayed_output"],
        "L2 重放输出与原 run 一致（per-item 记录回放）"
    );
    assert_eq!(cli_support::runs_count(&db), 1, "L2 replay 不得产生新 run");
}

// ---------------------------------------------------------------------------
// AC-17 b：上游输出列表决定 N（同一张图，运行期数据决定）
// ---------------------------------------------------------------------------

/// AC-17 b：同一张图（同一 graph_hash）两次 run 分别展开 N=2 / N=4；join 顺序=item_index。
#[test]
fn ac17b_upstream_list_decides_n_at_runtime() {
    let dir = tempfile::tempdir().expect("tempdir");
    let cases = [
        (json!(["x", "y"]), 2usize),
        (json!(["x", "y", "z", "w"]), 4usize),
    ];
    let mut hashes = Vec::new();
    for (index, (seed, expected_n)) in cases.into_iter().enumerate() {
        let db = dir.path().join(format!("b{index}.db"));
        let inputs = json!({"start": {"seed": seed}}).to_string();
        let output = run_foreach(dir.path(), &db, "upstream_items.json", &inputs, false);
        assert_exit(&output, 0, "AC-17 b run");
        let payload = json_stdout(&output);
        assert_eq!(payload["status"], json!("Completed"));
        let run_id = payload["run_id"].as_str().expect("run_id").to_string();
        let results: Vec<Value> = (0..expected_n)
            .map(|item_index| {
                json!({"item": seed.as_array().expect("seed").get(item_index).expect("seed item").clone(), "ok": true})
            })
            .collect();
        assert_eq!(
            payload["run_output"],
            json!({"result": {"count": expected_n, "results": results}}),
            "N={expected_n} 时 join 数组长度/顺序"
        );

        let reader = TraceReader::open_readonly(&db).expect("open trace");
        let rows = expansions(&reader, &run_id);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].item_count as usize, expected_n);
        assert_eq!(
            expansion_keys(&rows[0]),
            Value::Array(
                (0..expected_n)
                    .map(|item_index| json!({"$index": item_index as u64}))
                    .collect()
            )
        );
        let w_items = item_instances(&reader, &run_id, "W");
        assert_eq!(w_items.len(), expected_n, "W item 实例数 = 运行期 N");
        drop(reader);
        hashes.push(
            common::run_row(
                &TraceReader::open_readonly(&db).expect("open trace"),
                &run_id,
            )
            .graph_hash,
        );
    }
    assert_eq!(
        hashes[0], hashes[1],
        "N 由运行期数据决定：同一张图 graph_hash 不变（非 codegen）"
    );
}

// ---------------------------------------------------------------------------
// AC-17 c：空列表（N=0，无 item 实例、expansion count=0、join []）
// ---------------------------------------------------------------------------

/// AC-17 c：`items=[]` → 无 item 实例、`foreach_expansions` 一行 count=0/keys=[]、
/// J(end) 输入 `[]`、无 GateResult（无 E 实例）、run Completed。
#[test]
fn ac17c_empty_list_completes_with_empty_join_array() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db = dir.path().join("c.db");
    let output = run_foreach(
        dir.path(),
        &db,
        "empty_items.json",
        r#"{"start":{"items":[]}}"#,
        false,
    );
    assert_exit(&output, 0, "AC-17 c run");
    let payload = json_stdout(&output);
    assert_eq!(payload["status"], json!("Completed"));
    let run_id = payload["run_id"].as_str().expect("run_id").to_string();
    assert_eq!(
        payload["run_output"],
        json!({"results": []}),
        "空列表 join 直接投递 [] 给 end"
    );

    let reader = TraceReader::open_readonly(&db).expect("open trace");
    let rows = expansions(&reader, &run_id);
    assert_eq!(rows.len(), 1, "N=0 仍写一条 expansion 行");
    assert_eq!(rows[0].node_id, "F");
    assert_eq!(rows[0].item_count, 0);
    assert_eq!(expansion_keys(&rows[0]), json!([]));
    for node in ["F", "W", "E"] {
        assert!(
            item_instances(&reader, &run_id, node).is_empty(),
            "N=0 不得创建 {node} item 实例"
        );
    }
    let end_input = item_instances(&reader, &run_id, "end")
        .into_iter()
        .next()
        .expect("end 实例")
        .input
        .expect("end input");
    assert_eq!(end_input, json!({"results": []}), "J(end) 输入为 []");
    assert!(
        node_gates(&reader, &run_id, "E").is_empty(),
        "N=0 无 E 实例 → 无 join GateResult"
    );
    assert!(!reader
        .instances(&run_id)
        .expect("instances")
        .iter()
        .any(|row| row.status == TraceInstanceStatus::Crashed));
    drop(reader);

    // trace：expansion 行 count=0（REV-092 P3①）；L1 replay 一致。
    let trace = trace_json(&db, &run_id);
    assert_exit(&trace, 0, "AC-17 c trace");
    let lines = ndjson_lines(&trace);
    assert_eq!(
        lines.last().expect("last")["kind"],
        json!("ForeachExpansion")
    );
    assert_eq!(lines.last().expect("last")["item_count"], json!(0));
    assert_eq!(lines.last().expect("last")["keys"], json!([]));
    let replay = replay_events_json(&db, &run_id);
    assert_exit(&replay, 0, "AC-17 c replay");
    assert_eq!(replay.stdout, trace.stdout);
}

// ---------------------------------------------------------------------------
// AC-17 d：item 失败（冷峻）— run Failed（节点错误码）、其它 item 取消、无 join 投递
// ---------------------------------------------------------------------------

/// AC-17 d：某 item 节点抛 Python 异常 → run Failed（error_code=节点错误码
/// `NODE_PYTHON_EXCEPTION`）、该 item Crashed、其它 item Cancelled、无 join GateResult。
#[test]
fn ac17d_item_failure_fails_run_and_cancels_other_items() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db = dir.path().join("d.db");
    let inputs = r#"{"start":{"items":[{"id":"boom","fail":true},{"id":1,"slow":true},{"id":2,"slow":true},{"id":3,"slow":true}]}}"#;
    let output = run_foreach(dir.path(), &db, "item_failure.json", inputs, false);
    assert_exit(&output, 1, "AC-17 d run 应 Failed 退出 1");
    let payload = json_stdout(&output);
    assert_eq!(payload["status"], json!("Failed"));
    assert_eq!(
        payload["error"]["code"],
        json!("NODE_PYTHON_EXCEPTION"),
        "run error.code = 节点错误码（非 FOREACH 码）"
    );
    let run_id = payload["run_id"].as_str().expect("run_id").to_string();

    let reader = TraceReader::open_readonly(&db).expect("open trace");
    let run_row = common::run_row(&reader, &run_id);
    assert_eq!(run_row.status, TraceRunStatus::Failed);
    assert_eq!(run_row.error_code.as_deref(), Some("NODE_PYTHON_EXCEPTION"));
    assert_eq!(expansions(&reader, &run_id).len(), 1, "展开已发生");
    let w_items = item_instances(&reader, &run_id, "W");
    assert_eq!(w_items.len(), 4, "4 个 item 均有 W 实例");
    assert_eq!(
        w_items[0].status,
        TraceInstanceStatus::Crashed,
        "失败 item 的 W 实例 Crashed"
    );
    assert_eq!(
        w_items[0].error_code.as_deref(),
        Some("NODE_PYTHON_EXCEPTION")
    );
    for (index, row) in w_items.iter().enumerate().skip(1) {
        assert_eq!(
            row.status,
            TraceInstanceStatus::Cancelled,
            "其它 item 的 W 实例应 Cancelled（item {index}）"
        );
    }
    assert!(
        node_gates(&reader, &run_id, "E")
            .iter()
            .all(|gate| gate.to_node != "J"),
        "失败路径不得产生 join 投递（无 E→J GateResult）"
    );
    drop(reader);
}

// ---------------------------------------------------------------------------
// AC-17 e / j：key_field 重复 / key canonical
// ---------------------------------------------------------------------------

/// AC-17 e：`key_field=id` 重复 → 展开前失败 `RUN_FOREACH_ITEM_KEY_DUPLICATE`；
/// 无 item 实例、无 expansion 行、run Failed（exit 1）。
#[test]
fn ac17e_duplicate_key_fails_before_instances() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db = dir.path().join("e.db");
    let output = run_foreach(
        dir.path(),
        &db,
        "dup_keys.json",
        r#"{"start":{"items":[{"id":"x"},{"id":"x"}]}}"#,
        false,
    );
    assert_exit(&output, 1, "AC-17 e run 应 Failed 退出 1");
    let payload = json_stdout(&output);
    assert_eq!(payload["status"], json!("Failed"));
    assert_eq!(
        payload["error"]["code"],
        json!("RUN_FOREACH_ITEM_KEY_DUPLICATE")
    );
    let run_id = payload["run_id"].as_str().expect("run_id").to_string();

    let reader = TraceReader::open_readonly(&db).expect("open trace");
    assert_eq!(
        common::run_row(&reader, &run_id).error_message.as_deref(),
        Some("foreach node 'F' item key 'x' is duplicated at indexes 0,1"),
        "message 冻结"
    );
    assert!(
        expansions(&reader, &run_id).is_empty(),
        "展开校验失败无 expansion 行"
    );
    for node in ["F", "W", "E"] {
        assert!(item_instances(&reader, &run_id, node).is_empty());
    }
    drop(reader);
}

/// AC-17 j（key canonical）：number→`"1"`、bool→`"true"`、null/object/array→
/// `{"$index":i}`、string 原值；非 NULL 重复 → KEY_DUPLICATE；真实 string key 不与
/// 索引对象撞表示。
#[test]
fn ac17j_key_canonical_maps_and_duplicates() {
    let dir = tempfile::tempdir().expect("tempdir");
    // 成功：canonical 映射。
    let ok_db = dir.path().join("k_ok.db");
    let ok = run_foreach(
        dir.path(),
        &ok_db,
        "keys_canonical.json",
        r#"{"start":{"items":[{"id":1},{"id":true},{"id":null},{"id":{"a":1}},"s"]}}"#,
        false,
    );
    assert_exit(&ok, 0, "canonical keys run");
    let ok_payload = json_stdout(&ok);
    assert_eq!(ok_payload["status"], json!("Completed"));
    let ok_run = ok_payload["run_id"].as_str().expect("run_id").to_string();
    let reader = TraceReader::open_readonly(&ok_db).expect("open trace");
    assert_eq!(
        expansion_keys(&expansions(&reader, &ok_run)[0]),
        json!(["1", "true", {"$index": 2}, {"$index": 3}, {"$index": 4}]),
        "number/bool canonical；null/object → 索引表示；string 原值"
    );
    drop(reader);

    // 非 NULL 重复（canonical number）→ KEY_DUPLICATE。
    let dup_db = dir.path().join("k_dup.db");
    let dup = run_foreach(
        dir.path(),
        &dup_db,
        "keys_canonical.json",
        r#"{"start":{"items":[{"id":1},{"id":1}]}}"#,
        false,
    );
    assert_exit(&dup, 1, "canonical 重复 run");
    let dup_payload = json_stdout(&dup);
    assert_eq!(
        dup_payload["error"]["code"],
        json!("RUN_FOREACH_ITEM_KEY_DUPLICATE")
    );
    let dup_run = dup_payload["run_id"].as_str().expect("run_id").to_string();
    let reader = TraceReader::open_readonly(&dup_db).expect("open trace");
    assert_eq!(
        common::run_row(&reader, &dup_run).error_message.as_deref(),
        Some("foreach node 'F' item key '1' is duplicated at indexes 0,1")
    );
    drop(reader);

    // 真实 string key `{"$index":0}`（JSON 文本）与索引对象不撞表示。
    let text_db = dir.path().join("k_text.db");
    let text = run_foreach(
        dir.path(),
        &text_db,
        "keys_canonical.json",
        r#"{"start":{"items":[{"id":"{\"$index\":0}"},{"id":null}]}}"#,
        false,
    );
    assert_exit(&text, 0, "string-key run");
    let text_run = json_stdout(&text)["run_id"]
        .as_str()
        .expect("run_id")
        .to_string();
    let reader = TraceReader::open_readonly(&text_db).expect("open trace");
    assert_eq!(
        expansion_keys(&expansions(&reader, &text_run)[0]),
        json!(["{\"$index\":0}", {"$index": 1}]),
        "string key 保持字符串、索引派生为对象"
    );
}

// ---------------------------------------------------------------------------
// AC-17 f：max_items 超限 / items_port 非数组（含 unwrap 两种负例）
// ---------------------------------------------------------------------------

/// AC-17 f：N > max_items → `RUN_FOREACH_ITEMS_EXCEEDED`；items_port 非数组三变体
/// （无同名键 object / 同名键非 array / 同名键标量）→ `RUN_FOREACH_ITEMS_NOT_ARRAY`；
/// 均无 item 实例、无 expansion 行。
#[test]
fn ac17f_max_items_and_not_array_fail_before_instances() {
    let dir = tempfile::tempdir().expect("tempdir");

    // max_items=2，items 4 → EXCEEDED。
    let exceeded_db = dir.path().join("f_exceeded.db");
    let exceeded = run_foreach(
        dir.path(),
        &exceeded_db,
        "exceeded.json",
        r#"{"start":{"items":[1,2,3,4]}}"#,
        false,
    );
    assert_exit(&exceeded, 1, "max_items 超限 run");
    let exceeded_payload = json_stdout(&exceeded);
    assert_eq!(
        exceeded_payload["error"]["code"],
        json!("RUN_FOREACH_ITEMS_EXCEEDED")
    );
    let exceeded_run = exceeded_payload["run_id"]
        .as_str()
        .expect("run_id")
        .to_string();
    let reader = TraceReader::open_readonly(&exceeded_db).expect("open trace");
    assert_eq!(
        common::run_row(&reader, &exceeded_run)
            .error_message
            .as_deref(),
        Some("foreach node 'F' items 4 exceed max_items 2")
    );
    assert!(expansions(&reader, &exceeded_run).is_empty());
    assert!(item_instances(&reader, &exceeded_run, "F").is_empty());
    drop(reader);

    // NOT_ARRAY 三变体。
    let not_array_cases = [
        (
            r#"{"start":{"items":{"bad":1}}}"#,
            "foreach node 'F' items_port 'items' is not an array (got object)",
        ),
        (
            r#"{"start":{"items":{"items":{"x":1}}}}"#,
            "foreach node 'F' items_port 'items' is not an array (got object)",
        ),
        (
            r#"{"start":{"items":"oops"}}"#,
            "foreach node 'F' items_port 'items' is not an array (got string)",
        ),
    ];
    for (index, (inputs, expected_message)) in not_array_cases.into_iter().enumerate() {
        let db = dir.path().join(format!("f_not_array_{index}.db"));
        let output = run_foreach(dir.path(), &db, "not_array.json", inputs, false);
        assert_exit(&output, 1, "NOT_ARRAY run");
        let payload = json_stdout(&output);
        assert_eq!(
            payload["error"]["code"],
            json!("RUN_FOREACH_ITEMS_NOT_ARRAY"),
            "variant {index}"
        );
        let run_id = payload["run_id"].as_str().expect("run_id").to_string();
        let reader = TraceReader::open_readonly(&db).expect("open trace");
        assert_eq!(
            common::run_row(&reader, &run_id).error_message.as_deref(),
            Some(expected_message),
            "variant {index} 冻结 message"
        );
        assert!(expansions(&reader, &run_id).is_empty(), "variant {index}");
        assert!(item_instances(&reader, &run_id, "F").is_empty());
    }
}

/// AC-17 f 边界（C3）：F 唯一入边 gate=false（全部 settle=false）→ controller Skipped、
/// 不展开、无 item 实例；run Completed（正常 skip 级联）。
#[test]
fn ac17f_controller_all_edges_settle_false_skips_without_expansion() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db = dir.path().join("controller_skip.db");
    let output = run_foreach(
        dir.path(),
        &db,
        "controller_skip.json",
        r#"{"start":{"items":[1,2]}}"#,
        false,
    );
    assert_exit(&output, 0, "controller skip run");
    let payload = json_stdout(&output);
    assert_eq!(payload["status"], json!("Completed"));
    let run_id = payload["run_id"].as_str().expect("run_id").to_string();

    let reader = TraceReader::open_readonly(&db).expect("open trace");
    assert!(
        expansions(&reader, &run_id).is_empty(),
        "controller Skipped 不展开 → 无 expansion 行"
    );
    for node in ["F", "W", "E"] {
        assert!(item_instances(&reader, &run_id, node).is_empty());
    }
    let start_gates = node_gates(&reader, &run_id, "start");
    assert_eq!(start_gates.len(), 1, "F 入边照常记 GateResult（to_node=F）");
    assert_eq!(start_gates[0].to_node, "F");
    assert!(!start_gates[0].passed, "gate=false");
    drop(reader);
}

// ---------------------------------------------------------------------------
// AC-17 g：取消传播（全部 item Cancelled、run Cancelled）
// ---------------------------------------------------------------------------

/// AC-17 g：item 在飞时对 run 进程发 SIGINT → CLI 取消路径（exit 130）；
/// 全部 W/E item 实例 Cancelled、run Cancelled、无 join 投递、`RunFinished{Cancelled}`。
#[test]
fn ac17g_cancel_propagates_to_all_items() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db = dir.path().join("cancel.db");
    let inputs = r#"{"start":{"items":[{"id":0,"slow":true},{"id":1,"slow":true},{"id":2,"slow":true},{"id":3,"slow":true}]}}"#;
    let run = ForeachRun::spawn(dir.path(), &db, "cancel.json", inputs, false);
    let pid = run.pid();
    poll_item_running(&db, "W");
    send_sigint(pid);
    let output = run.wait("AC-17 g cancel run");
    assert_exit(&output, 130, "SIGINT 取消应 exit 130");
    let payload = json_stdout(&output);
    assert_eq!(payload["status"], json!("Cancelled"));
    let run_id = payload["run_id"].as_str().expect("run_id").to_string();

    let reader = TraceReader::open_readonly(&db).expect("open trace");
    assert_eq!(
        common::run_row(&reader, &run_id).status,
        TraceRunStatus::Cancelled
    );
    for node in ["W", "E"] {
        let rows = item_instances(&reader, &run_id, node);
        assert!(!rows.is_empty(), "{node} 应有 item 实例");
        assert!(
            rows.iter()
                .all(|row| row.status == TraceInstanceStatus::Cancelled),
            "{node} 全部 item 实例应 Cancelled: {:?}",
            rows.iter()
                .map(|row| (row.item_index, row.status))
                .collect::<Vec<_>>()
        );
    }
    assert!(
        item_instances(&reader, &run_id, "F")
            .iter()
            .all(|row| row.status.is_terminal()),
        "取消后不得残留非终态 F item 实例"
    );
    assert!(
        node_gates(&reader, &run_id, "E")
            .iter()
            .all(|gate| gate.to_node != "end"),
        "取消路径不得产生 join 投递"
    );
    let finished = common::run_finished_payload(&reader, &run_id);
    assert_eq!(finished["status"], json!("Cancelled"));
    drop(reader);
}

// ---------------------------------------------------------------------------
// AC-17 h：region 内 human（每 item 独立 WaitingHuman / decide / join 有序）
// ---------------------------------------------------------------------------

/// AC-17 h：region 内 `builtin.human`，N=2 → 2 个 WaitingHuman item 实例（item_index 0/1），
/// 分别 decide → join 有序结果、`human_decisions` 2 行不同 instance 且 consumed；
/// trace 顺序 events → HumanDecision×2 → ForeachExpansion；human 图 `:memory:` 仍拒绝。
#[test]
fn ac17h_human_region_two_items_decide_separately_and_join_ordered() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db = dir.path().join("human.db");
    let run = ForeachRun::spawn(
        dir.path(),
        &db,
        "human_items2.json",
        r#"{"start":{"items":[{"id":"a"},{"id":"b"}]}}"#,
        false,
    );
    let waiting = poll_waiting_items(&db, "H", 2);
    assert_eq!(
        waiting
            .iter()
            .map(|(index, _)| *index)
            .collect::<Vec<u32>>(),
        vec![0, 1],
        "2 个 WaitingHuman item 实例 item_index 0/1"
    );
    let run_id: String = {
        let conn = read_only_connect(&db);
        conn.query_row(
            "SELECT run_id FROM runs WHERE status IN ('Running','Paused')",
            [],
            |row| row.get(0),
        )
        .expect("running run")
    };
    let decide0 = decide_item(
        dir.path(),
        &db,
        &run_id,
        &waiting[0].1,
        "approved",
        "alice",
        Some("first"),
    );
    assert_exit(&decide0, 0, "item 0 decide approved");
    let decide1 = decide_item(
        dir.path(),
        &db,
        &run_id,
        &waiting[1].1,
        "rejected",
        "bob",
        Some("second"),
    );
    assert_exit(&decide1, 0, "item 1 decide rejected");

    let output = run.wait("AC-17 h run");
    assert_exit(&output, 0, "AC-17 h run 应 Completed");
    let payload = json_stdout(&output);
    assert_eq!(payload["status"], json!("Completed"));
    let expected_results = json!([
        {"decision": "approved", "reason": "first"},
        {"decision": "rejected", "reason": "second"},
    ]);
    assert_eq!(
        payload["run_output"],
        json!({"results": expected_results}),
        "join 有序结果 = 各 item human output 按 item_index 升序"
    );

    let reader = TraceReader::open_readonly(&db).expect("open trace");
    assert_eq!(
        item_instances(&reader, &run_id, "H")
            .iter()
            .map(|row| (row.item_index, row.status))
            .collect::<Vec<(u32, TraceInstanceStatus)>>(),
        vec![
            (0, TraceInstanceStatus::Completed),
            (1, TraceInstanceStatus::Completed)
        ]
    );
    let rows = expansions(&reader, &run_id);
    assert_eq!(rows[0].item_count, 2);
    assert_eq!(expansion_keys(&rows[0]), json!(["a", "b"]), "key_field=id");
    let decisions = reader.human_decisions(&run_id).expect("decisions");
    assert_eq!(decisions.len(), 2, "每 item 一行决策");
    assert_ne!(decisions[0].instance_id, decisions[1].instance_id);
    assert!(decisions.iter().all(|row| row.consumed_at.is_some()));
    drop(reader);

    // trace 顺序：events → HumanDecision×2 → ForeachExpansion；L1 replay 逐字节一致。
    let trace = trace_json(&db, &run_id);
    assert_exit(&trace, 0, "AC-17 h trace");
    let kinds = trace_kinds(&trace);
    let human_positions: Vec<usize> = kinds
        .iter()
        .enumerate()
        .filter(|(_, kind)| kind.as_str() == "HumanDecision")
        .map(|(index, _)| index)
        .collect();
    assert_eq!(human_positions.len(), 2);
    assert_eq!(
        kinds.last().map(String::as_str),
        Some("ForeachExpansion"),
        "展开行在 HumanDecision 之后: {kinds:?}"
    );
    let expansion_index = kinds.len() - 1;
    assert!(
        human_positions.iter().all(|index| *index < expansion_index),
        "HumanDecision 行先于 ForeachExpansion: {kinds:?}"
    );
    let replay = replay_events_json(&db, &run_id);
    assert_exit(&replay, 0, "AC-17 h replay");
    assert_eq!(replay.stdout, trace.stdout);

    // human 图 + `:memory:` 启动前预检拒绝（AC-17 h 尾）。
    let memory = run_cli(
        &[
            "run".to_string(),
            arg(&foreach_fixture("human_items2.json")),
            "--inputs".to_string(),
            r#"{"start":{"items":[{"id":"a"}]}}"#.to_string(),
            "--db".to_string(),
            ":memory:".to_string(),
            "--python".to_string(),
            python_program(),
            "--python-path".to_string(),
            arg(&repo_root().join("python")),
            "--python-path".to_string(),
            arg(&foreach_python_path()),
            "--json".to_string(),
        ],
        dir.path(),
    );
    assert_exit(&memory, 3, ":memory: + human foreach 图应启动前拒绝");
    let stderr = stderr_text(&memory);
    assert!(stderr.contains("RUN_ENTRY_INVALID"), "stderr: {stderr}");
    assert!(
        stderr.contains("trace_required_for_human"),
        "stderr: {stderr}"
    );
}

// ---------------------------------------------------------------------------
// AC-17 j：E Skipped → RUN_FOREACH_ITEM_INCOMPLETE（无 Crashed、其它 item Cancelled）
// ---------------------------------------------------------------------------

/// AC-17 j：某 item 的 E 全部入边 gate=false → E Skipped → `RUN_FOREACH_ITEM_INCOMPLETE`
/// （run Failed、无实例 Crashed、其它 item Cancelled、无 join 投递）。
#[test]
fn ac17j_exit_skipped_fails_item_incomplete_without_crash_or_join() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db = dir.path().join("exit_skipped.db");
    let output = run_foreach(
        dir.path(),
        &db,
        "exit_skipped.json",
        r#"{"start":{"items":[{"id":0},{"skip":true},{"id":2}]}}"#,
        false,
    );
    assert_exit(&output, 1, "AC-17 j run 应 Failed 退出 1");
    let payload = json_stdout(&output);
    assert_eq!(payload["status"], json!("Failed"));
    assert_eq!(
        payload["error"]["code"],
        json!("RUN_FOREACH_ITEM_INCOMPLETE")
    );
    let run_id = payload["run_id"].as_str().expect("run_id").to_string();

    let reader = TraceReader::open_readonly(&db).expect("open trace");
    assert_eq!(
        common::run_row(&reader, &run_id).error_message.as_deref(),
        Some("foreach node 'F' item 1 exit node 'E' was skipped"),
        "message 冻结"
    );
    let instances = reader.instances(&run_id).expect("instances");
    assert!(
        instances
            .iter()
            .all(|row| row.status != TraceInstanceStatus::Crashed),
        "ITEM_INCOMPLETE 不得产生 Crashed 实例"
    );
    let e_items = item_instances(&reader, &run_id, "E");
    assert_eq!(
        e_items
            .iter()
            .map(|row| row.item_index)
            .collect::<Vec<u32>>(),
        vec![0, 1, 2],
        "E 每个 item 一个实例"
    );
    assert_eq!(
        e_items[1].status,
        TraceInstanceStatus::Skipped,
        "E item1（gate=false）Skipped"
    );
    // 其余 item 在冷峻失败时已被取消或已先完成（两者都是合法终态；不得残留非终态）。
    for row in [&e_items[0], &e_items[2]] {
        assert!(
            matches!(
                row.status,
                TraceInstanceStatus::Cancelled | TraceInstanceStatus::Completed
            ),
            "E item{} 终态为 Cancelled/Completed，实际 {:?}",
            row.item_index,
            row.status
        );
    }
    assert!(
        item_instances(&reader, &run_id, "W")
            .iter()
            .all(|row| row.status.is_terminal()),
        "失败后不得残留非终态 W item 实例"
    );
    assert!(
        node_gates(&reader, &run_id, "E").is_empty(),
        "E 无实例完成 → 无 join 投递（无 E 相关 GateResult）"
    );
    drop(reader);
}

// ---------------------------------------------------------------------------
// AC-17 i 补充：trace 输出顺序 / --stats 不新增字段 / L1 replay / v5 迁移后 foreach 端到端
// ---------------------------------------------------------------------------

/// AC-17 i / REV-091 P3：`trace --json` 仅在事件行后追加 ForeachExpansion；
/// `--stats` 不新增字段；人类可读输出含 `foreach node='F' items=3`；L1 replay 逐字节一致。
#[test]
fn ac17i_trace_expansion_output_order_and_stats_unchanged() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db = dir.path().join("trace_i.db");
    let output = run_foreach(
        dir.path(),
        &db,
        "entry_items3.json",
        r#"{"start":{"items":[{"id":"a"},{"id":"b"},{"id":"c"}]}}"#,
        false,
    );
    assert_exit(&output, 0, "AC-17 i run");
    let run_id = json_stdout(&output)["run_id"]
        .as_str()
        .expect("run_id")
        .to_string();

    let trace = trace_json(&db, &run_id);
    assert_exit(&trace, 0, "trace --json");
    let lines = ndjson_lines(&trace);
    let expansion_lines: Vec<&Value> = lines
        .iter()
        .filter(|line| line["kind"] == json!("ForeachExpansion"))
        .collect();
    assert_eq!(expansion_lines.len(), 1);
    let event_kinds = [
        "RunStarted",
        "Transition",
        "GateResult",
        "NodeTimedOut",
        "NodeAbandoned",
        "Streaming",
        "RunFinished",
    ];
    assert!(
        lines
            .iter()
            .take(lines.len() - 1)
            .all(|line| event_kinds.contains(&line["kind"].as_str().expect("kind"))),
        "展开行之前的行必须是既有 7 类事件（events.kind 不变）"
    );

    // --stats：字段集合冻结（不新增 human/foreach 统计）。
    let stats = run_cli(
        &[
            "trace".to_string(),
            run_id.clone(),
            "--db".to_string(),
            arg(&db),
            "--stats".to_string(),
            "--json".to_string(),
        ],
        dir.path(),
    );
    assert_exit(&stats, 0, "trace --stats");
    let stats_payload = json_stdout(&stats);
    let mut keys: Vec<&str> = stats_payload
        .as_object()
        .expect("stats object")
        .keys()
        .map(String::as_str)
        .collect();
    keys.sort_unstable();
    assert_eq!(
        keys,
        vec![
            "abandoned",
            "db",
            "duration_ms",
            "events",
            "instances",
            "run_id",
            "status",
            "streaming",
            "timeouts",
            "trace_backpressure",
        ],
        "--stats 不新增字段（REV-091 P3）: {stats_payload}"
    );

    // 人类可读输出含展开摘要。
    let human = run_cli(
        &[
            "trace".to_string(),
            run_id.clone(),
            "--db".to_string(),
            arg(&db),
        ],
        dir.path(),
    );
    assert_exit(&human, 0, "trace 人类输出");
    let stdout = String::from_utf8_lossy(&human.stdout);
    assert!(
        stdout.contains("foreach node='F' items=3 keys=["),
        "人类可读展开摘要: {stdout}"
    );

    let replay = replay_events_json(&db, &run_id);
    assert_exit(&replay, 0, "replay events");
    assert_eq!(replay.stdout, trace.stdout, "L1 replay 与 trace 逐字节一致");
}

/// AC-17 i：旧 v5 库上端到端跑 foreach 图（真 CLI run 触发 v5→v6 迁移）→ 迁移+展开+join
/// 全部成立；旧 run L1 golden 不变；新 run L2 per-item outputs 回放匹配。
#[test]
fn ac17i_v5_migrated_db_foreach_run_and_l2() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (db, old_run_id) = seeded_v5_db(dir.path());
    let output = run_foreach(
        dir.path(),
        &db,
        "entry_items3.json",
        r#"{"start":{"items":[{"id":"a"},{"id":"b"},{"id":"c"}]}}"#,
        true,
    );
    assert_exit(&output, 0, "旧 v5 库上的 foreach run");
    let payload = json_stdout(&output);
    assert_eq!(payload["status"], json!("Completed"));
    let new_run_id = payload["run_id"].as_str().expect("run_id").to_string();
    assert_ne!(new_run_id, old_run_id);

    let conn = read_only_connect(&db);
    assert_eq!(user_version(&conn), 7, "v5→v7 迁移完成");
    assert_eq!(migration_versions(&conn), vec![1, 2, 3, 4, 5, 6, 7]);
    assert!(sqlite_object_exists(&conn, "table", "foreach_expansions"));
    assert_eq!(backup_files(dir.path()).len(), 2, "v5→v7 逐版本两份备份");
    drop(conn);

    let reader = TraceReader::open_readonly(&db).expect("open trace");
    let rows = expansions(&reader, &new_run_id);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].item_count, 3);
    assert_eq!(
        item_instances(&reader, &new_run_id, "W")
            .iter()
            .map(|row| row.item_index)
            .collect::<Vec<u32>>(),
        vec![0, 1, 2]
    );
    drop(reader);

    // 旧 run：与迁移前 v5 golden 一致。
    let old_trace = trace_json(&db, &old_run_id);
    assert_exit(&old_trace, 0, "迁移后旧 run trace");
    assert_eq!(
        ndjson_lines(&old_trace),
        expected_trace("v5_expected_trace.json"),
        "旧 v5 run L1 golden 不变"
    );

    // 新 run：L2 per-item outputs 回放匹配。
    let l2 = run_cli(
        &[
            "replay".to_string(),
            "--run".to_string(),
            new_run_id.clone(),
            "--mode".to_string(),
            "outputs".to_string(),
            "--db".to_string(),
            arg(&db),
            "--json".to_string(),
        ],
        dir.path(),
    );
    assert_exit(&l2, 0, "迁移后 foreach run L2 replay");
    let l2_payload = json_stdout(&l2);
    assert_eq!(l2_payload["matched"], json!(true));
    assert_eq!(l2_payload["recorded_outputs"], json!(10));
    assert_eq!(l2_payload["original_output"], l2_payload["replayed_output"]);
}
