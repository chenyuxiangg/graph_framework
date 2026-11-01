//! schema 迁移与只读打开语义（06 §6.7 / §6.6；v3 = ISSUE-087、v4 = W2a、v5 = W3a、v6 = W4a）。
//!
//! 覆盖（DR-007 §8.1/§8.2 / 06 §6.2.5/§6.2.15/§6.7 的 W4b 测试清单）：
//! - 新库 v6：DDL/索引/`schema_migrations` 1..6/无备份；`item_index`/`item_key`
//!   默认 0/NULL；唯一键 `(run_id,node_id,attempt,item_index)`；`foreach_expansions`；
//! - v1→v6 / v2→v6 / v3→v6 / v4→v6 / v5→v6 逐版本链：每版本恰一份备份、原行逐列保留；
//! - v5→v6：旧行 item 列取 `0/NULL`、`human_decisions`/gates/events 保留、唯一键生效、
//!   `foreach_expansions` 可写；只读 v5 拒绝且不迁移；迁移失败注入 → 恢复 v5 备份。

mod common;

use std::path::{Path, PathBuf};

use rusqlite::Connection;
use zgraph_trace::{
    EventQuery, RunStatus, TraceError, TraceReader, TraceWriter, TRACE_SCHEMA_VERSION,
};

const EXPECTED_TABLES: &[&str] = &[
    "events",
    "foreach_expansions",
    "gate_results",
    "graphs",
    "human_decisions",
    "instances",
    "llm_calls",
    "recorded_outputs",
    "runs",
    "schema_migrations",
    "streaming_events",
    "transitions",
];

const EXPECTED_INDEXES: &[&str] = &[
    "idx_events_instance",
    "idx_events_kind",
    "idx_events_run_seq",
    "idx_foreach_expansions_run",
    "idx_gate_passed",
    "idx_gate_run_edge",
    "idx_gate_run_seq",
    "idx_human_decisions_pending",
    "idx_instances_error_code",
    "idx_instances_run_node",
    "idx_instances_run_node_item",
    "idx_instances_run_status",
    "idx_llm_calls_run",
    "idx_recorded_outputs_run",
    "idx_runs_status_started",
    "idx_runs_unfinished",
    "idx_stream_instance",
    "idx_stream_kind",
    "idx_stream_run_seq",
    "idx_stream_tool",
    "idx_transitions_instance",
    "idx_transitions_run_seq",
    "idx_transitions_run_status",
];

/// W4a 前的 v4 `instances`/`transitions` 表形状（status CHECK 无 `WaitingHuman`，
/// 无 item 列）。测试用它把「当前版本建出的库」降级成真实 v4 库。
const V4_INSTANCES_REBUILD: &str = r#"
CREATE TABLE instances_v4 (
  instance_id  TEXT PRIMARY KEY,
  run_id       TEXT NOT NULL REFERENCES runs(run_id) ON DELETE CASCADE,
  node_id      TEXT NOT NULL,
  node_type    TEXT NOT NULL CHECK (node_type IN
               ('start','end','python_node','agent_call','subgraph')),
  attempt      INTEGER NOT NULL DEFAULT 0,
  status       TEXT NOT NULL CHECK (status IN
               ('Pending','Ready','Running','Completed','Crashed','TimedOut','Cancelled','Skipped')),
  input        TEXT,
  output_json  TEXT,
  error        TEXT,
  error_code   TEXT,
  created_seq  INTEGER,
  started_at   TEXT,
  finished_at  TEXT,
  finished_seq INTEGER,
  UNIQUE (run_id, node_id, attempt)
);
INSERT INTO instances_v4 (
  instance_id, run_id, node_id, node_type, attempt, status, input, output_json,
  error, error_code, created_seq, started_at, finished_at, finished_seq
)
SELECT
  instance_id, run_id, node_id, node_type, attempt, status, input, output_json,
  error, error_code, created_seq, started_at, finished_at, finished_seq
FROM instances;
DROP TABLE instances;
ALTER TABLE instances_v4 RENAME TO instances;
CREATE INDEX IF NOT EXISTS idx_instances_run_node   ON instances(run_id, node_id);
CREATE INDEX IF NOT EXISTS idx_instances_run_status ON instances(run_id, status);
CREATE INDEX IF NOT EXISTS idx_instances_error_code ON instances(error_code) WHERE error_code IS NOT NULL;
"#;

const V4_TRANSITIONS_REBUILD: &str = r#"
CREATE TABLE transitions_v4 (
  transition_id INTEGER PRIMARY KEY AUTOINCREMENT,
  seq           INTEGER NOT NULL UNIQUE REFERENCES events(seq) ON DELETE CASCADE,
  run_id        TEXT NOT NULL,
  instance_id   TEXT NOT NULL,
  node_id       TEXT NOT NULL,
  attempt       INTEGER NOT NULL,
  from_status   TEXT NOT NULL CHECK (from_status IN
                ('Pending','Ready','Running','Completed','Crashed','TimedOut','Cancelled','Skipped')),
  to_status     TEXT NOT NULL CHECK (to_status IN
                ('Pending','Ready','Running','Completed','Crashed','TimedOut','Cancelled','Skipped')),
  reason        TEXT,
  error_code    TEXT,
  error         TEXT,
  at            TEXT NOT NULL
);
INSERT INTO transitions_v4 (
  transition_id, seq, run_id, instance_id, node_id, attempt,
  from_status, to_status, reason, error_code, error, at
)
SELECT
  transition_id, seq, run_id, instance_id, node_id, attempt,
  from_status, to_status, reason, error_code, error, at
FROM transitions;
DROP TABLE transitions;
ALTER TABLE transitions_v4 RENAME TO transitions;
CREATE INDEX IF NOT EXISTS idx_transitions_run_seq    ON transitions(run_id, seq);
CREATE INDEX IF NOT EXISTS idx_transitions_instance   ON transitions(instance_id, seq);
CREATE INDEX IF NOT EXISTS idx_transitions_run_status ON transitions(run_id, to_status);
"#;

/// W3a（v5）的 `instances` 表形状：有 `WaitingHuman` CHECK，但**无** item 列，
/// 唯一键仍是 `(run_id, node_id, attempt)`。用于构造真实 v5 库。
const V5_INSTANCES_REBUILD: &str = r#"
CREATE TABLE instances_v5 (
  instance_id  TEXT PRIMARY KEY,
  run_id       TEXT NOT NULL REFERENCES runs(run_id) ON DELETE CASCADE,
  node_id      TEXT NOT NULL,
  node_type    TEXT NOT NULL CHECK (node_type IN
               ('start','end','python_node','agent_call','subgraph')),
  attempt      INTEGER NOT NULL DEFAULT 0,
  status       TEXT NOT NULL CHECK (status IN
               ('Pending','Ready','Running','WaitingHuman','Completed','Crashed','TimedOut','Cancelled','Skipped')),
  input        TEXT,
  output_json  TEXT,
  error        TEXT,
  error_code   TEXT,
  created_seq  INTEGER,
  started_at   TEXT,
  finished_at  TEXT,
  finished_seq INTEGER,
  UNIQUE (run_id, node_id, attempt)
);
INSERT INTO instances_v5 (
  instance_id, run_id, node_id, node_type, attempt, status, input, output_json,
  error, error_code, created_seq, started_at, finished_at, finished_seq
)
SELECT
  instance_id, run_id, node_id, node_type, attempt, status, input, output_json,
  error, error_code, created_seq, started_at, finished_at, finished_seq
FROM instances;
DROP TABLE instances;
ALTER TABLE instances_v5 RENAME TO instances;
CREATE INDEX IF NOT EXISTS idx_instances_run_node   ON instances(run_id, node_id);
CREATE INDEX IF NOT EXISTS idx_instances_run_status ON instances(run_id, status);
CREATE INDEX IF NOT EXISTS idx_instances_error_code ON instances(error_code) WHERE error_code IS NOT NULL;
"#;

fn table_columns(conn: &Connection, table: &str) -> Vec<String> {
    let mut stmt = conn
        .prepare(&format!("PRAGMA table_info({table})"))
        .expect("prepare table_info");
    let rows = stmt
        .query_map([], |row| row.get::<_, String>(1))
        .expect("query columns");
    rows.map(|row| row.expect("column name")).collect()
}

/// `table` 上 UNIQUE 索引的列（用于断言 v6 唯一键 = `(run_id,node_id,attempt,item_index)`）。
fn unique_index_columns(conn: &Connection, table: &str) -> Vec<String> {
    let mut stmt = conn
        .prepare(&format!("PRAGMA index_list({table})"))
        .expect("prepare index_list");
    let mut rows = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, String>(3)?,
            ))
        })
        .expect("query index_list");
    let mut unique_indexes = Vec::new();
    for row in rows.by_ref() {
        let (name, unique, origin) = row.expect("index entry");
        // origin='u' = UNIQUE 约束；跳过 'pk' 主键 autoindex 与普通索引。
        if unique == 1 && origin == "u" {
            unique_indexes.push(name);
        }
    }
    let mut columns = Vec::new();
    for name in unique_indexes {
        let mut info = conn
            .prepare(&format!("PRAGMA index_info('{name}')"))
            .expect("prepare index_info");
        let mut info_rows = info
            .query_map([], |row| row.get::<_, String>(2))
            .expect("query index_info");
        for column in info_rows.by_ref() {
            columns.push(column.expect("index column"));
        }
    }
    columns
}

fn user_version(conn: &Connection) -> i64 {
    conn.query_row("PRAGMA user_version", [], |row| row.get(0))
        .expect("read user_version")
}

fn sqlite_names(conn: &Connection, kind: &str) -> Vec<String> {
    let mut stmt = conn
        .prepare("SELECT name FROM sqlite_master WHERE type = ?1 ORDER BY name")
        .expect("prepare sqlite_master");
    let rows = stmt
        .query_map([kind], |row| row.get::<_, String>(0))
        .expect("query sqlite_master");
    rows.map(|row| row.expect("name")).collect()
}

fn table_exists(conn: &Connection, table: &str) -> bool {
    sqlite_names(conn, "table").iter().any(|name| name == table)
}

/// 表内容的逐列快照（`Value` 精确比较；用于断言迁移不改历史行）。
fn table_rows(conn: &Connection, sql: &str) -> Vec<Vec<rusqlite::types::Value>> {
    let mut stmt = conn.prepare(sql).expect("prepare rows query");
    let column_count = stmt.column_count();
    let mut rows = stmt.query([]).expect("query rows");
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

fn raw_i64(value: &rusqlite::types::Value) -> i64 {
    match value {
        rusqlite::types::Value::Integer(value) => *value,
        other => panic!("expected integer column: {other:?}"),
    }
}

fn raw_text(value: &rusqlite::types::Value) -> &str {
    match value {
        rusqlite::types::Value::Text(value) => value.as_str(),
        other => panic!("expected text column: {other:?}"),
    }
}

fn raw_opt_text(value: &rusqlite::types::Value) -> Option<&str> {
    match value {
        rusqlite::types::Value::Null => None,
        rusqlite::types::Value::Text(value) => Some(value.as_str()),
        other => panic!("expected nullable text column: {other:?}"),
    }
}

/// v4/v5 兼容的实例列清单（不含 item 列；在 v6 表上同样可读）。
const INSTANCE_COLUMNS_V5: &str =
    "instance_id, run_id, node_id, node_type, attempt, status, input, \
     output_json, error, error_code, created_seq, started_at, finished_at, finished_seq";
const INSTANCE_COLUMNS_V6: &str = "instance_id, run_id, node_id, node_type, attempt, item_index, \
     item_key, status, input, output_json, error, error_code, created_seq, started_at, \
     finished_at, finished_seq";
const TRANSITION_COLUMNS: &str = "transition_id, seq, run_id, instance_id, node_id, attempt, \
     from_status, to_status, reason, error_code, error, at";
const EVENT_COLUMNS: &str = "seq, run_id, kind, severity, at, instance_id, node_id, payload";
const HUMAN_DECISION_COLUMNS: &str = "run_id, instance_id, decision, approver, reason, \
     decision_id, decided_at, consumed_at";

fn instance_rows_v5(conn: &Connection) -> Vec<Vec<rusqlite::types::Value>> {
    table_rows(
        conn,
        &format!("SELECT {INSTANCE_COLUMNS_V5} FROM instances ORDER BY instance_id"),
    )
}

fn instance_rows_v6(conn: &Connection) -> Vec<Vec<rusqlite::types::Value>> {
    table_rows(
        conn,
        &format!("SELECT {INSTANCE_COLUMNS_V6} FROM instances ORDER BY instance_id"),
    )
}

fn transition_rows(conn: &Connection) -> Vec<Vec<rusqlite::types::Value>> {
    table_rows(
        conn,
        &format!("SELECT {TRANSITION_COLUMNS} FROM transitions ORDER BY transition_id"),
    )
}

fn event_rows(conn: &Connection) -> Vec<Vec<rusqlite::types::Value>> {
    table_rows(
        conn,
        &format!("SELECT {EVENT_COLUMNS} FROM events ORDER BY seq"),
    )
}

fn human_decision_rows(conn: &Connection) -> Vec<Vec<rusqlite::types::Value>> {
    table_rows(
        conn,
        &format!(
            "SELECT {HUMAN_DECISION_COLUMNS} FROM human_decisions ORDER BY run_id, instance_id"
        ),
    )
}

fn gate_result_rows(conn: &Connection) -> Vec<Vec<rusqlite::types::Value>> {
    table_rows(
        conn,
        "SELECT gate_result_id, seq, run_id, instance_id, node_id, edge_index, to_node, \
         to_port, gate_fn, passed, error, at, deferred, attempt, verdict_json \
         FROM gate_results ORDER BY gate_result_id",
    )
}

/// 断言迁移后 `TraceReader` 的事件投影与迁移前 `events` 表逐字段一致（golden）。
fn assert_trace_events_match(
    reader: &TraceReader,
    run_id: &str,
    expected: &[Vec<rusqlite::types::Value>],
) {
    let events = reader
        .events(run_id, &EventQuery::default())
        .expect("trace events");
    assert_eq!(events.len(), expected.len(), "old run event 数不变");
    for (event, raw) in events.iter().zip(expected.iter()) {
        assert_eq!(event.seq, raw_i64(&raw[0]), "events.seq 不变");
        assert_eq!(event.run_id, raw_text(&raw[1]));
        assert_eq!(event.kind, raw_text(&raw[2]));
        assert_eq!(event.severity, raw_text(&raw[3]));
        assert_eq!(event.at, raw_text(&raw[4]));
        assert_eq!(event.instance_id.as_deref(), raw_opt_text(&raw[5]));
        assert_eq!(event.node_id.as_deref(), raw_opt_text(&raw[6]));
        assert_eq!(
            event.payload.to_string(),
            raw_text(&raw[7]),
            "trace payload 投影必须与迁移前存储一致"
        );
    }
}

/// 断言 `INSERT` 被 CHECK 约束拒绝（旧 v4 库不应接受 `WaitingHuman`）。
fn assert_check_rejects(conn: &Connection, sql: &str, what: &str) {
    match conn.execute_batch(sql) {
        Ok(()) => panic!("{what}: expected CHECK violation, insert succeeded"),
        Err(rusqlite::Error::SqliteFailure(inner, _)) => assert_eq!(
            inner.code,
            rusqlite::ErrorCode::ConstraintViolation,
            "{what}: unexpected sqlite error {inner:?}"
        ),
        Err(other) => panic!("{what}: unexpected error {other:?}"),
    }
}

/// 断言 `INSERT` 被 UNIQUE 约束拒绝（v6 定位键）。
fn assert_unique_rejects(conn: &Connection, sql: &str, what: &str) {
    match conn.execute_batch(sql) {
        Ok(()) => panic!("{what}: expected UNIQUE violation, insert succeeded"),
        Err(rusqlite::Error::SqliteFailure(inner, _)) => assert_eq!(
            inner.code,
            rusqlite::ErrorCode::ConstraintViolation,
            "{what}: unexpected sqlite error {inner:?}"
        ),
        Err(other) => panic!("{what}: unexpected error {other:?}"),
    }
}

#[test]
fn fresh_database_migrates_to_current_version_with_all_tables_and_indexes() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("trace.db");
    let writer = TraceWriter::open(&path).expect("open writer");
    writer.flush().expect("flush");

    let reader = TraceReader::open_readonly(&path).expect("open reader");
    assert_eq!(
        reader.schema_version().expect("schema version"),
        TRACE_SCHEMA_VERSION
    );

    let conn = Connection::open(&path).expect("inspect connection");
    assert_eq!(user_version(&conn), 6, "v6 = W4a");
    let tables = sqlite_names(&conn, "table");
    for expected in EXPECTED_TABLES {
        assert!(
            tables.iter().any(|name| name == expected),
            "missing table {expected}; have {tables:?}"
        );
    }
    let indexes = sqlite_names(&conn, "index");
    for expected in EXPECTED_INDEXES {
        assert!(
            indexes.iter().any(|name| name == expected),
            "missing index {expected}; have {indexes:?}"
        );
    }
    for version in 1..=TRACE_SCHEMA_VERSION {
        let migrated: i64 = conn
            .query_row(
                "SELECT count(*) FROM schema_migrations WHERE version = ?1",
                [version],
                |row| row.get(0),
            )
            .expect("migration row");
        assert_eq!(migrated, 1, "missing schema_migrations row v{version}");
    }
    let runs_columns = table_columns(&conn, "runs");
    for column in [
        "seed",
        "worker_versions_json",
        "model_params_json",
        "record_mode",
        "killed_workers",
        "global_vars_json",
    ] {
        assert!(
            runs_columns.iter().any(|name| name == column),
            "runs missing column {column}; have {runs_columns:?}"
        );
    }
    let gate_columns = table_columns(&conn, "gate_results");
    for column in ["deferred", "attempt", "verdict_json"] {
        assert!(
            gate_columns.iter().any(|name| name == column),
            "gate_results missing column {column}; have {gate_columns:?}"
        );
    }
    // v5 `human_decisions` 列与 pending 部分索引（06 §6.2.14）。
    assert_eq!(
        table_columns(&conn, "human_decisions"),
        vec![
            "run_id",
            "instance_id",
            "decision",
            "approver",
            "reason",
            "decision_id",
            "decided_at",
            "consumed_at",
        ]
    );
    let pending_index_sql: String = conn
        .query_row(
            "SELECT sql FROM sqlite_master WHERE type = 'index' AND name = 'idx_human_decisions_pending'",
            [],
            |row| row.get(0),
        )
        .expect("pending index sql");
    assert!(
        pending_index_sql.contains("WHERE consumed_at IS NULL"),
        "pending 索引必须是部分索引: {pending_index_sql}"
    );
    // v6：item 列 + 唯一键 `(run_id,node_id,attempt,item_index)` + expansions 表。
    let instance_columns = table_columns(&conn, "instances");
    for column in ["item_index", "item_key"] {
        assert!(
            instance_columns.iter().any(|name| name == column),
            "instances missing column {column}; have {instance_columns:?}"
        );
    }
    assert_eq!(
        unique_index_columns(&conn, "instances"),
        vec!["run_id", "node_id", "attempt", "item_index"],
        "v6 唯一键必须是 item 定位键"
    );
    assert_eq!(
        table_columns(&conn, "foreach_expansions"),
        vec![
            "run_id",
            "expansion_id",
            "node_id",
            "source_instance_id",
            "item_count",
            "keys_json",
            "at",
        ]
    );
    drop(conn);
    drop(reader);
    writer.close().expect("close writer");
    assert!(
        backup_files(dir.path()).is_empty(),
        "fresh database must not create migration backups"
    );
}

#[test]
fn fresh_v6_database_accepts_waiting_human_and_default_item_columns() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("trace.db");
    let writer = TraceWriter::open(&path).expect("open writer");
    let handle = writer.handle();
    handle.send(common::started("run-waiting"));
    handle.send(common::transition(
        "run-waiting",
        "inst-human",
        "review",
        zgraph_trace::InstanceStatus::Pending,
        zgraph_trace::InstanceStatus::WaitingHuman,
        common::ts(10),
    ));
    writer.flush().expect("flush");
    writer.close().expect("close writer");

    let reader = TraceReader::open_readonly(&path).expect("reader");
    let instance = reader
        .instances("run-waiting")
        .expect("instances")
        .into_iter()
        .find(|row| row.instance_id == "inst-human")
        .expect("human instance");
    assert_eq!(instance.status, zgraph_trace::InstanceStatus::WaitingHuman);
    assert_eq!(instance.item_index, 0, "非 foreach 实例 item_index=0");
    assert_eq!(instance.item_key, None, "无 key_field 时 item_key=NULL");
    assert_eq!(
        instance.status.as_str(),
        "WaitingHuman",
        "存储值必须是 CamelCase 字面量"
    );
    assert!(reader
        .foreach_expansions("run-waiting")
        .expect("expansions")
        .is_empty());
}

#[test]
fn newer_schema_is_rejected_without_touching_the_database() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("future.db");
    {
        let conn = Connection::open(&path).expect("create future db");
        conn.execute_batch("PRAGMA user_version = 99;")
            .expect("set user_version");
    }
    match TraceWriter::open(&path) {
        Ok(_) => panic!("expected SchemaTooNew, got successful open"),
        Err(TraceError::SchemaTooNew { found, supported }) => {
            assert_eq!(found, 99);
            assert_eq!(supported, TRACE_SCHEMA_VERSION);
        }
        Err(other) => panic!("expected SchemaTooNew, got {other:?}"),
    }
    let conn = Connection::open(&path).expect("reopen");
    assert_eq!(user_version(&conn), 99, "rejected db must not be migrated");
    assert!(
        sqlite_names(&conn, "table").is_empty(),
        "rejected db must not gain tables"
    );
}

#[test]
fn readonly_open_of_missing_database_errors_and_creates_no_file() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("missing.db");
    let error = match TraceReader::open_readonly(&path) {
        Ok(_) => panic!("readonly open of missing db must fail"),
        Err(error) => error,
    };
    match &error {
        TraceError::Storage(message) => {
            assert!(
                message.contains("database not found"),
                "unexpected message: {message}"
            )
        }
        other => panic!("expected Storage, got {other:?}"),
    }
    assert!(!path.exists(), "readonly open must not create the db file");
}

#[test]
fn readonly_reader_rejects_newer_schema_without_migrating() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("trace.db");
    let writer = TraceWriter::open(&path).expect("create current db");
    writer.close().expect("close writer");
    let future = TRACE_SCHEMA_VERSION + 1;
    {
        let conn = Connection::open(&path).expect("inspect");
        conn.execute_batch(&format!("PRAGMA user_version = {future};"))
            .expect("set newer version");
    }
    match TraceReader::open_readonly(&path) {
        Ok(_) => panic!("readonly reader must reject newer schema"),
        Err(TraceError::SchemaTooNew { found, supported }) => {
            assert_eq!(found, future);
            assert_eq!(supported, TRACE_SCHEMA_VERSION);
        }
        Err(other) => panic!("expected SchemaTooNew, got {other:?}"),
    }
    let conn = Connection::open(&path).expect("inspect after rejection");
    assert_eq!(
        user_version(&conn),
        future,
        "readonly reader must not migrate the database"
    );
}

#[test]
fn readonly_reader_rejects_v1_schema_without_migrating() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("v1.db");
    create_v1_database(&path);
    match TraceReader::open_readonly(&path) {
        Ok(_) => panic!("readonly reader must reject older schema"),
        Err(TraceError::Storage(message)) => assert!(
            message.contains(&format!(
                "unsupported schema version=1, expected {TRACE_SCHEMA_VERSION}"
            )),
            "unexpected message: {message}"
        ),
        Err(other) => panic!("expected Storage, got {other:?}"),
    }
    let conn = Connection::open(&path).expect("inspect after rejection");
    assert_eq!(
        user_version(&conn),
        1,
        "readonly reader must not migrate the database"
    );
}

#[test]
fn readonly_reader_rejects_v4_schema_without_migrating() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("v4.db");
    create_v4_database(&path);
    match TraceReader::open_readonly(&path) {
        Ok(_) => panic!("readonly reader must reject v4 schema"),
        Err(TraceError::Storage(message)) => assert!(
            message.contains(&format!(
                "unsupported schema version=4, expected {TRACE_SCHEMA_VERSION}"
            )),
            "unexpected message: {message}"
        ),
        Err(other) => panic!("expected Storage, got {other:?}"),
    }
    let conn = Connection::open(&path).expect("inspect after rejection");
    assert_eq!(user_version(&conn), 4, "只读拒绝不得迁移");
    assert!(
        !table_exists(&conn, "human_decisions"),
        "只读拒绝不得创建 v5 表"
    );
    assert!(
        backup_files(dir.path()).is_empty(),
        "只读拒绝不得生成迁移备份"
    );
}

#[test]
fn readonly_reader_rejects_v5_schema_without_migrating() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("v5.db");
    create_v5_database(&path);
    match TraceReader::open_readonly(&path) {
        Ok(_) => panic!("readonly reader must reject v5 schema"),
        Err(TraceError::Storage(message)) => assert!(
            message.contains(&format!(
                "unsupported schema version=5, expected {TRACE_SCHEMA_VERSION}"
            )),
            "unexpected message: {message}"
        ),
        Err(other) => panic!("expected Storage, got {other:?}"),
    }
    let conn = Connection::open(&path).expect("inspect after rejection");
    assert_eq!(user_version(&conn), 5, "只读拒绝不得迁移");
    assert!(
        !table_exists(&conn, "foreach_expansions"),
        "只读拒绝不得创建 v6 表"
    );
    assert!(
        !table_columns(&conn, "instances")
            .iter()
            .any(|name| name == "item_index"),
        "只读拒绝不得重建 instances"
    );
    assert!(
        backup_files(dir.path()).is_empty(),
        "只读拒绝不得生成迁移备份"
    );
}

#[test]
fn readonly_open_of_file_without_trace_schema_errors_and_writes_nothing() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("empty.db");
    std::fs::write(&path, b"").expect("create empty db file");
    let error = match TraceReader::open_readonly(&path) {
        Ok(_) => panic!("readonly open of non-trace db must fail"),
        Err(error) => error,
    };
    assert!(matches!(error, TraceError::Storage(_)));
    assert_eq!(
        std::fs::metadata(&path).expect("metadata").len(),
        0,
        "readonly open must not write the db file"
    );
    assert!(!Path::new(&format!("{}-wal", path.display())).exists());
}

#[test]
fn v1_database_is_upgraded_to_current_version_in_place() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("v1.db");
    create_v1_database(&path);

    let writer = TraceWriter::open(&path).expect("upgrade v1 db");
    let handle = writer.handle();
    handle.send(common::started("run-v1"));
    handle.send(common::finished("run-v1", RunStatus::Completed));
    writer.flush().expect("flush");
    let reader = TraceReader::open_readonly(&path).expect("reader");
    assert_eq!(
        reader.schema_version().expect("version"),
        TRACE_SCHEMA_VERSION
    );
    let run = reader.run("run-v1").expect("query").expect("run exists");
    assert_eq!(run.seed, 42);
    assert_eq!(run.record_mode.as_deref(), Some("hash"));
    assert_eq!(run.killed_workers, 1);
    assert_eq!(run.global_vars, Some(serde_json::json!({})));

    let conn = Connection::open(&path).expect("inspect");
    let runs_columns = table_columns(&conn, "runs");
    for column in [
        "seed",
        "worker_versions_json",
        "model_params_json",
        "record_mode",
        "global_vars_json",
    ] {
        assert!(
            runs_columns.iter().any(|name| name == column),
            "missing upgraded column {column}"
        );
    }
    assert!(table_exists(&conn, "llm_calls"));
    assert!(table_exists(&conn, "human_decisions"), "v1→v5 链建 human");
    assert!(
        table_exists(&conn, "foreach_expansions"),
        "v1→v6 链建 foreach_expansions"
    );
    assert!(table_columns(&conn, "instances")
        .iter()
        .any(|name| name == "item_index"));
    drop(conn);
    drop(reader);
    writer.close().expect("close");

    // 06 §6.7：每个版本迁移前各留一份备份（v1→v2 … v5→v6）。
    let backups = backup_files(dir.path());
    assert_eq!(
        backups.len(),
        5,
        "expected five migration backups: {backups:?}"
    );
    for old_version in 1..=5 {
        assert!(
            backups
                .iter()
                .any(|path| backup_name(path).contains(&format!(".bak.v{old_version}-"))),
            "missing .bak.v{old_version}- backup: {backups:?}"
        );
    }
}

#[test]
fn v2_database_is_upgraded_to_current_version_with_backup_and_preserves_rows() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("v2.db");
    create_v2_database(&path);

    let writer = TraceWriter::open(&path).expect("upgrade v2 db");
    writer.close().expect("close writer");

    let reader = TraceReader::open_readonly(&path).expect("reader");
    assert_eq!(
        reader.schema_version().expect("version"),
        TRACE_SCHEMA_VERSION
    );
    let run = reader
        .run("run-v2")
        .expect("query")
        .expect("legacy run preserved");
    assert_eq!(run.status, RunStatus::Completed);
    assert_eq!(
        run.global_vars, None,
        "pre-v3 run has no global_vars record (compat: replay falls back to empty map)"
    );

    // 备份 = 迁移前快照（user_version=2、无 global_vars_json/v4/v5/v6 增量）。
    let backups = backup_files(dir.path());
    assert_eq!(backups.len(), 4, "v2→v6 逐版本四份备份: {backups:?}");
    let backup = backups
        .iter()
        .find(|path| backup_name(path).contains(".bak.v2-"))
        .unwrap_or_else(|| panic!("missing v2 backup: {backups:?}"));
    let backup_conn = Connection::open(backup).expect("open backup");
    assert_eq!(user_version(&backup_conn), 2, "backup keeps old version");
    assert!(
        !table_columns(&backup_conn, "runs")
            .iter()
            .any(|name| name == "global_vars_json"),
        "backup was taken before the v3 column was added"
    );
    assert!(!table_exists(&backup_conn, "foreach_expansions"));
    let run_id: String = backup_conn
        .query_row("SELECT run_id FROM runs", [], |row| row.get(0))
        .expect("backup keeps run rows");
    assert_eq!(run_id, "run-v2");
}

#[test]
fn v3_database_is_upgraded_to_v6_with_gate_result_defaults_and_backup() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("v3.db");
    create_v3_database(&path);

    // 迁移前：v3 库无 v4 列，但保留一条 v0.1.0 时代的 gate 结果行。
    let conn = Connection::open(&path).expect("inspect v3");
    assert_eq!(user_version(&conn), 3);
    let v3_columns = table_columns(&conn, "gate_results");
    for column in ["deferred", "attempt", "verdict_json"] {
        assert!(
            !v3_columns.iter().any(|name| name == column),
            "v3 db must not have {column}"
        );
    }
    let legacy_rows: i64 = conn
        .query_row("SELECT COUNT(*) FROM gate_results", [], |row| row.get(0))
        .expect("legacy gate rows");
    assert_eq!(legacy_rows, 1);
    drop(conn);

    // 打开写端触发 v3→v4→v5→v6：补列后旧行取默认值，新行写入三字段。
    let writer = TraceWriter::open(&path).expect("upgrade v3 db");
    let handle = writer.handle();
    handle.send(common::gate_with_v4(
        "run-v3",
        "inst-p",
        "review",
        1,
        false,
        None,
        true,
        2,
        Some(serde_json::json!({"x": 1})),
    ));
    writer.flush().expect("flush");
    writer.close().expect("close writer");

    let reader = TraceReader::open_readonly(&path).expect("reader");
    assert_eq!(
        reader.schema_version().expect("version"),
        TRACE_SCHEMA_VERSION
    );
    let run = reader
        .run("run-v3")
        .expect("query")
        .expect("legacy run preserved");
    assert_eq!(run.status, RunStatus::Completed);
    let gates = reader.gate_results("run-v3").expect("gates");
    assert_eq!(gates.len(), 2, "legacy row + new row");
    assert!(!gates[0].deferred, "legacy row defaults to deferred=false");
    assert_eq!(gates[0].attempt, 0, "legacy row defaults to attempt=0");
    assert_eq!(gates[0].verdict, None, "legacy row verdict_json is NULL");
    assert!(!gates[0].passed);
    assert!(gates[1].deferred);
    assert_eq!(gates[1].attempt, 2);
    assert_eq!(gates[1].verdict, Some(serde_json::json!({"x": 1})));
    drop(reader);

    // 迁移后：v4/v5/v6 增量存在、审计行 6 写入、备份为 v3/v4/v5 快照。
    let conn = Connection::open(&path).expect("inspect v6");
    assert_eq!(user_version(&conn), TRACE_SCHEMA_VERSION);
    let v4_columns = table_columns(&conn, "gate_results");
    for column in ["deferred", "attempt", "verdict_json"] {
        assert!(
            v4_columns.iter().any(|name| name == column),
            "missing migrated column {column}"
        );
    }
    let description: String = conn
        .query_row(
            "SELECT description FROM schema_migrations WHERE version = 6",
            [],
            |row| row.get(0),
        )
        .expect("v6 audit row");
    assert!(
        description.contains("W4a"),
        "v6 audit description should name W4a: {description}"
    );
    drop(conn);

    let backups = backup_files(dir.path());
    assert_eq!(backups.len(), 3, "v3→v6 逐版本三份备份: {backups:?}");
    let backup = backups
        .iter()
        .find(|path| backup_name(path).contains(".bak.v3-"))
        .unwrap_or_else(|| panic!("missing v3 backup: {backups:?}"));
    let backup_conn = Connection::open(backup).expect("open backup");
    assert_eq!(user_version(&backup_conn), 3, "backup keeps old version");
    assert!(
        !table_columns(&backup_conn, "gate_results")
            .iter()
            .any(|name| name == "deferred"),
        "backup was taken before the v4 columns were added"
    );
    let backup_rows: i64 = backup_conn
        .query_row("SELECT COUNT(*) FROM gate_results", [], |row| row.get(0))
        .expect("backup keeps gate rows");
    assert_eq!(backup_rows, 1);
}

#[test]
fn v4_schema_rejects_waiting_human_statuses_before_migration() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("v4-probe.db");
    create_v4_database(&path);
    let conn = Connection::open(&path).expect("inspect v4");
    assert_eq!(user_version(&conn), 4);
    assert!(
        !table_exists(&conn, "human_decisions"),
        "v4 库不应有 human_decisions"
    );
    assert!(
        !table_columns(&conn, "instances")
            .iter()
            .any(|name| name == "item_index"),
        "v4 库不应有 item 列"
    );
    assert_check_rejects(
        &conn,
        "INSERT INTO instances (instance_id, run_id, node_id, node_type, attempt, status) \
         VALUES ('inst-probe', 'run-v4', 'probe', 'python_node', 0, 'WaitingHuman');",
        "v4 instances CHECK",
    );
    // 为 transitions 造一条合法的事件信封（满足 FK），再验证 to_status CHECK。
    conn.execute(
        "INSERT INTO events (run_id, kind, severity, at, payload) \
         VALUES ('run-v4', 'Transition', 'info', '2026-01-01T00:00:00.000Z', '{}')",
        [],
    )
    .expect("insert probe event");
    let probe_seq = conn.last_insert_rowid();
    assert_check_rejects(
        &conn,
        &format!(
            "INSERT INTO transitions (seq, run_id, instance_id, node_id, attempt, from_status, \
             to_status, at) VALUES ({probe_seq}, 'run-v4', 'inst-v4', 'node-v4', 0, 'Pending', \
             'WaitingHuman', '2026-01-01T00:00:00.000Z');"
        ),
        "v4 transitions CHECK",
    );
}

#[test]
fn v4_database_is_upgraded_to_v6_preserving_instances_transitions_and_events() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("v4.db");
    create_v4_database(&path);

    // 迁移前：v4 快照（版本=4、无 human/expansions/item 列）。
    let before = {
        let conn = Connection::open(&path).expect("inspect v4");
        assert_eq!(user_version(&conn), 4);
        assert!(!table_exists(&conn, "human_decisions"));
        let snapshot = (
            instance_rows_v5(&conn),
            transition_rows(&conn),
            event_rows(&conn),
        );
        assert_eq!(snapshot.0.len(), 1, "seed 恰一条实例");
        assert_eq!(snapshot.1.len(), 1, "seed 恰一条迁移");
        assert_eq!(
            snapshot.2.len(),
            4,
            "seed：RunStarted + Transition + GateResult + RunFinished"
        );
        snapshot
    };

    // 打开写端触发 v4→v5→v6（备份 → 单事务重建 + 建 human/expansions）。
    let writer = TraceWriter::open(&path).expect("upgrade v4 db");
    writer.flush().expect("flush");

    let reader = TraceReader::open_readonly(&path).expect("reader");
    assert_eq!(
        reader.schema_version().expect("version"),
        TRACE_SCHEMA_VERSION
    );
    let instances = reader.instances("run-v4").expect("instances");
    assert_eq!(instances.len(), before.0.len());
    assert_eq!(instances[0].item_index, 0, "旧行 item_index=0");
    assert_eq!(instances[0].item_key, None, "旧行 item_key=NULL");
    assert!(reader
        .foreach_expansions("run-v4")
        .expect("expansions")
        .is_empty());

    // v6 数据面可用：human_decisions/expansions/item 实例写入。
    let conn = Connection::open(&path).expect("inspect v6");
    assert_eq!(user_version(&conn), TRACE_SCHEMA_VERSION);
    assert_eq!(
        table_rows(
            &conn,
            &format!("SELECT {INSTANCE_COLUMNS_V5} FROM instances ORDER BY instance_id")
        ),
        before.0,
        "v4→v6 必须逐列保留 instances（item 列取默认值）"
    );
    assert_eq!(
        table_rows(
            &conn,
            &format!("SELECT {TRANSITION_COLUMNS} FROM transitions ORDER BY transition_id")
        ),
        before.1,
        "v4→v6 必须逐列保留 transitions"
    );
    assert_eq!(
        table_rows(
            &conn,
            &format!("SELECT {EVENT_COLUMNS} FROM events ORDER BY seq")
        ),
        before.2,
        "v4→v6 不得改写 events（含 payload）"
    );
    // trace 读取 golden：迁移后 TraceReader 的事件投影与迁移前 events 表逐字段一致。
    assert_trace_events_match(&reader, "run-v4", &before.2);
    // 唯一键 `(run_id,node_id,attempt,item_index)` 在迁移后生效。
    assert_unique_rejects(
        &conn,
        "INSERT INTO instances (instance_id, run_id, node_id, node_type, attempt, item_index, \
         status) VALUES ('inst-dup', 'run-v4', 'node-seed', 'python_node', 0, 0, 'Completed');",
        "v6 unique key",
    );
    conn.execute_batch(
        "INSERT INTO instances (instance_id, run_id, node_id, node_type, attempt, item_index, \
         item_key, status) VALUES ('inst-item-1', 'run-v4', 'node-seed', 'python_node', 0, 1, \
         'id-1', 'Completed');",
    )
    .expect("同一 (run,node,attempt) 不同 item_index 允许共存");
    drop(conn);
    drop(reader);
    writer.close().expect("close writer");

    // 备份 = 逐版本迁移前快照（v4→v5 与 v5→v6 各一份；W4b 机械同步期望）。
    let backups = backup_files(dir.path());
    assert_eq!(backups.len(), 2, "v4→v6 逐版本两份备份: {backups:?}");
    let backup = backups
        .iter()
        .find(|path| backup_name(path).contains(".bak.v4-"))
        .unwrap_or_else(|| panic!("missing .bak.v4- backup: {backups:?}"));
    let backup_conn = Connection::open(backup).expect("open backup");
    assert_eq!(user_version(&backup_conn), 4, "备份保持 v4");
    assert!(!table_exists(&backup_conn, "human_decisions"));
    assert_eq!(
        table_rows(
            &backup_conn,
            &format!("SELECT {INSTANCE_COLUMNS_V5} FROM instances ORDER BY instance_id")
        ),
        before.0,
        "备份保留迁移前 instances 行"
    );
    assert_check_rejects(
        &backup_conn,
        "INSERT INTO instances (instance_id, run_id, node_id, node_type, attempt, status) \
         VALUES ('inst-probe', 'run-v4', 'probe', 'python_node', 0, 'WaitingHuman');",
        "v4 backup CHECK",
    );
}

#[test]
fn v5_database_is_upgraded_to_v6_with_item_defaults_unique_key_and_expansions_table() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("v5.db");
    create_v5_database(&path);

    // 迁移前：真实 v5 形状（有 human_decisions/WaitingHuman，无 item 列/expansions）。
    let before = {
        let conn = Connection::open(&path).expect("inspect v5");
        assert_eq!(user_version(&conn), 5);
        assert!(table_exists(&conn, "human_decisions"));
        assert!(!table_exists(&conn, "foreach_expansions"));
        let instance_columns = table_columns(&conn, "instances");
        for column in ["item_index", "item_key"] {
            assert!(
                !instance_columns.iter().any(|name| name == column),
                "v5 库不应有 {column}"
            );
        }
        let snapshot = (
            instance_rows_v5(&conn),
            transition_rows(&conn),
            event_rows(&conn),
            human_decision_rows(&conn),
            gate_result_rows(&conn),
        );
        assert_eq!(
            snapshot.0.len(),
            2,
            "seed：Completed + WaitingHuman 两条实例"
        );
        assert_eq!(snapshot.3.len(), 1, "seed 恰一条 human decision");
        assert_eq!(snapshot.4.len(), 1, "seed 恰一条 gate result");
        snapshot
    };

    let writer = TraceWriter::open(&path).expect("upgrade v5 db");
    writer.close().expect("close writer");

    let reader = TraceReader::open_readonly(&path).expect("reader");
    assert_eq!(
        reader.schema_version().expect("version"),
        TRACE_SCHEMA_VERSION
    );

    let conn = Connection::open(&path).expect("inspect v6");
    assert_eq!(user_version(&conn), 6);
    // 1..6 审计行齐备。
    for version in 1..=6 {
        let migrated: i64 = conn
            .query_row(
                "SELECT count(*) FROM schema_migrations WHERE version = ?1",
                [version],
                |row| row.get(0),
            )
            .expect("migration row");
        assert_eq!(migrated, 1, "missing schema_migrations row v{version}");
    }
    // 旧行逐列保留，item 列取默认 0/NULL。
    assert_eq!(
        instance_rows_v5(&conn),
        before.0,
        "v5→v6 必须逐列保留 instances"
    );
    assert_eq!(transition_rows(&conn), before.1);
    assert_eq!(event_rows(&conn), before.2);
    assert_eq!(
        human_decision_rows(&conn),
        before.3,
        "human_decisions 结构/数据在 v6 不变"
    );
    assert_eq!(
        gate_result_rows(&conn),
        before.4,
        "gate_results 逐列保留（v6 未重建该表）"
    );
    for row in instance_rows_v6(&conn) {
        assert_eq!(raw_i64(&row[5]), 0, "旧行 item_index=0");
        assert_eq!(
            raw_opt_text(&row[6]),
            None,
            "旧行 item_key=NULL（按索引派生）"
        );
    }
    // WaitingHuman 状态与 human decision 仍可读。
    let waiting = reader
        .instances("run-v5")
        .expect("instances")
        .into_iter()
        .find(|row| row.instance_id == "inst-waiting")
        .expect("WaitingHuman 实例保留");
    assert_eq!(waiting.status, zgraph_trace::InstanceStatus::WaitingHuman);
    let decisions = reader.human_decisions("run-v5").expect("decisions");
    assert_eq!(decisions.len(), 1);
    assert_eq!(decisions[0].instance_id, "inst-waiting");
    // trace 读取 golden：迁移后 TraceReader 的事件投影与迁移前 events 表逐字段一致。
    assert_trace_events_match(&reader, "run-v5", &before.2);
    // foreach_expansions 存在且为空（无 foreach 旧 run）。
    assert!(reader
        .foreach_expansions("run-v5")
        .expect("expansions")
        .is_empty());
    // v6 唯一键生效；同 (run,node,attempt) 不同 item_index 可共存。
    assert_unique_rejects(
        &conn,
        "INSERT INTO instances (instance_id, run_id, node_id, node_type, attempt, item_index, \
         status) VALUES ('inst-dup', 'run-v5', 'node-seed', 'python_node', 0, 0, 'Completed');",
        "v6 unique key",
    );
    conn.execute_batch(
        "INSERT INTO instances (instance_id, run_id, node_id, node_type, attempt, item_index, \
         item_key, status) VALUES ('inst-item-7', 'run-v5', 'node-seed', 'python_node', 0, 7, \
         'id-7', 'Completed');
         INSERT INTO foreach_expansions (run_id, expansion_id, node_id, source_instance_id, \
         item_count, keys_json, at) VALUES ('run-v5', 'expansion-1', 'node-seed', NULL, 0, \
         '[]', '2026-01-01T00:00:02.000Z');",
    )
    .expect("v6 数据面可写");
    drop(conn);
    drop(reader);

    // 备份恰一份 v5 快照（user_version=5、无 item 列/expansions、旧数据）。
    let backups = backup_files(dir.path());
    assert_eq!(backups.len(), 1, "v5→v6 恰一份备份: {backups:?}");
    let backup = &backups[0];
    assert!(backup_name(backup).contains(".bak.v5-"));
    let backup_conn = Connection::open(backup).expect("open backup");
    assert_eq!(user_version(&backup_conn), 5, "备份保持 v5");
    assert!(
        !table_columns(&backup_conn, "instances")
            .iter()
            .any(|name| name == "item_index"),
        "备份是 v6 重建前的快照"
    );
    assert!(!table_exists(&backup_conn, "foreach_expansions"));
    assert_eq!(human_decision_rows(&backup_conn), before.3);
    // v5 备份表没有 item 列（上面已断言），旧唯一键为 3 列。
    assert_eq!(
        unique_index_columns(&backup_conn, "instances"),
        vec!["run_id", "node_id", "attempt"],
        "v5 备份保持旧唯一键"
    );
}

#[test]
fn failed_v5_to_v6_migration_restores_backup_and_leaves_v5_untouched() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("locked-v5.db");
    create_v5_database(&path);
    {
        // 故障注入：v5→v6 迁移在写 schema_migrations 审计行时 ABORT（单事务回滚）。
        let conn = Connection::open(&path).expect("install trigger");
        conn.execute_batch(
            "CREATE TRIGGER abort_schema_migration BEFORE INSERT ON schema_migrations \
             BEGIN SELECT RAISE(ABORT, 'injected v6 migration failure'); END;",
        )
        .expect("create abort trigger");
    }

    let error = match TraceWriter::open(&path) {
        Ok(_) => panic!("migration must fail under injected abort"),
        Err(error) => error,
    };
    assert!(
        matches!(&error, TraceError::Storage(message) if message.contains("injected v6 migration failure")),
        "unexpected error: {error:?}"
    );

    // 失败后：版本/表/数据都回到 v5（恢复备份 + 事务回滚）。
    let conn = Connection::open(&path).expect("inspect restored db");
    assert_eq!(user_version(&conn), 5, "restored db keeps v5");
    assert!(
        !table_exists(&conn, "foreach_expansions"),
        "回滚后 v6 表必须不存在"
    );
    assert!(
        !table_columns(&conn, "instances")
            .iter()
            .any(|name| name == "item_index"),
        "回滚后 v6 列必须不存在"
    );
    assert!(table_exists(&conn, "human_decisions"));
    drop(conn);
    assert!(
        backup_files(dir.path())
            .iter()
            .any(|path| backup_name(path).contains(".bak.v5-")),
        "failed migration must leave the v5 backup for recovery"
    );

    // 清掉故障注入后重试：同一库可正常迁移到当前版本。
    let conn = Connection::open(&path).expect("drop trigger");
    conn.execute_batch("DROP TRIGGER abort_schema_migration;")
        .expect("drop trigger");
    drop(conn);
    let writer = TraceWriter::open(&path).expect("retry migration");
    writer.close().expect("close writer");
    let reader = TraceReader::open_readonly(&path).expect("reader");
    assert_eq!(
        reader.schema_version().expect("version"),
        TRACE_SCHEMA_VERSION
    );
    assert!(reader.run("run-v5").expect("query").is_some());
}

#[test]
fn failed_v4_migration_restores_backup_and_leaves_v4_untouched() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("locked.db");
    create_v4_database(&path);
    {
        // 故障注入：v4→v5 迁移在写 schema_migrations 审计行时 ABORT（单事务回滚）。
        let conn = Connection::open(&path).expect("install trigger");
        conn.execute_batch(
            "CREATE TRIGGER abort_schema_migration BEFORE INSERT ON schema_migrations \
             BEGIN SELECT RAISE(ABORT, 'injected v5 migration failure'); END;",
        )
        .expect("create abort trigger");
    }

    let error = match TraceWriter::open(&path) {
        Ok(_) => panic!("migration must fail under injected abort"),
        Err(error) => error,
    };
    assert!(
        matches!(&error, TraceError::Storage(message) if message.contains("injected v5 migration failure")),
        "unexpected error: {error:?}"
    );

    // 失败后：版本/表/数据都回到迁移前（恢复备份 + 事务回滚）。
    let conn = Connection::open(&path).expect("inspect restored db");
    assert_eq!(user_version(&conn), 4, "restored db keeps v4");
    assert!(
        !table_exists(&conn, "human_decisions"),
        "回滚后 v5 表必须不存在"
    );
    assert!(instance_rows_v5(&conn).len() == 1, "实例行保留");
    drop(conn);
    assert!(
        backup_files(dir.path())
            .iter()
            .any(|path| backup_name(path).contains(".bak.v4-")),
        "failed migration must leave the v4 backup for recovery"
    );

    // 清掉故障注入后重试：同一库可正常迁移到当前版本。
    let conn = Connection::open(&path).expect("drop trigger");
    conn.execute_batch("DROP TRIGGER abort_schema_migration;")
        .expect("drop trigger");
    drop(conn);
    let writer = TraceWriter::open(&path).expect("retry migration");
    writer.close().expect("close writer");
    let reader = TraceReader::open_readonly(&path).expect("reader");
    assert_eq!(
        reader.schema_version().expect("version"),
        TRACE_SCHEMA_VERSION
    );
    assert!(reader.run("run-v4").expect("query").is_some());
}

#[test]
fn failed_migration_restores_backup_and_leaves_v2_untouched() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("locked-v2.db");
    create_v2_database(&path);
    {
        // 故障注入：v2→v3 迁移在写 schema_migrations 审计行时 ABORT（单事务回滚）。
        let conn = Connection::open(&path).expect("install trigger");
        conn.execute_batch(
            "CREATE TRIGGER abort_schema_migration BEFORE INSERT ON schema_migrations \
             BEGIN SELECT RAISE(ABORT, 'injected migration failure'); END;",
        )
        .expect("create abort trigger");
    }

    let error = match TraceWriter::open(&path) {
        Ok(_) => panic!("migration must fail under injected abort"),
        Err(error) => error,
    };
    assert!(
        matches!(&error, TraceError::Storage(message) if message.contains("injected migration failure")),
        "unexpected error: {error:?}"
    );

    // 失败后：版本/列/数据都回到迁移前（恢复备份 + 事务回滚）。
    let conn = Connection::open(&path).expect("inspect restored db");
    assert_eq!(user_version(&conn), 2, "restored db keeps v2");
    assert!(
        !table_columns(&conn, "runs")
            .iter()
            .any(|name| name == "global_vars_json"),
        "rolled back v3 column must be gone"
    );
    let runs: i64 = conn
        .query_row("SELECT COUNT(*) FROM runs", [], |row| row.get(0))
        .expect("run rows preserved");
    assert_eq!(runs, 1);
    drop(conn);
    assert!(
        backup_files(dir.path())
            .iter()
            .any(|path| backup_name(path).contains(".bak.v2-")),
        "failed migration must leave the v2 backup for recovery"
    );

    // 清掉故障注入后重试：同一库可正常迁移到当前版本。
    let conn = Connection::open(&path).expect("drop trigger");
    conn.execute_batch("DROP TRIGGER abort_schema_migration;")
        .expect("drop trigger");
    drop(conn);
    let writer = TraceWriter::open(&path).expect("retry migration");
    writer.close().expect("close writer");
    let reader = TraceReader::open_readonly(&path).expect("reader");
    assert_eq!(
        reader.schema_version().expect("version"),
        TRACE_SCHEMA_VERSION
    );
    assert!(reader.run("run-v2").expect("query").is_some());
}

/// 目录下的迁移备份文件完整路径（`.bak.v{old}-{ts}`）。
fn backup_files(dir: &Path) -> Vec<PathBuf> {
    std::fs::read_dir(dir)
        .expect("read dir")
        .map(|entry| entry.expect("dir entry").path())
        .filter(|path| backup_name(path).contains(".bak.v"))
        .collect()
}

/// 备份文件名（断言用）。
fn backup_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// 把当前 v6 库降级为真实 v5 形状：删 `foreach_expansions`、重建 `instances`
/// （无 item 列、唯一键 3 列）、删 v6 审计行、`user_version=5`（保留 human_decisions）。
fn downgrade_to_v5_shape(conn: &Connection) {
    conn.execute_batch("DROP TABLE IF EXISTS foreach_expansions;")
        .expect("drop foreach_expansions");
    conn.execute_batch(V5_INSTANCES_REBUILD)
        .expect("rebuild instances to v5 shape");
    conn.execute_batch("DELETE FROM schema_migrations WHERE version = 6; PRAGMA user_version = 5;")
        .expect("downgrade to v5");
    assert_eq!(user_version(conn), 5);
}

/// 把 v5 形状降级为真实 v4 形状：删 `human_decisions`、重建 `instances`/
/// `transitions`（CHECK 无 `WaitingHuman`）、删 v5/v6 审计行、`user_version=4`。
fn downgrade_to_v4_shape(conn: &Connection) {
    conn.execute_batch("DROP TABLE IF EXISTS human_decisions;")
        .expect("drop human_decisions");
    conn.execute_batch(V4_INSTANCES_REBUILD)
        .expect("rebuild instances to v4 shape");
    conn.execute_batch(V4_TRANSITIONS_REBUILD)
        .expect("rebuild transitions to v4 shape");
    conn.execute_batch(
        "DELETE FROM schema_migrations WHERE version IN (5, 6); PRAGMA user_version = 4;",
    )
    .expect("downgrade to v4");
    assert_eq!(user_version(conn), 4);
}

/// 用当前版本建库并写一条 Completed run（实例/迁移/事件/gate），再关闭写端。
fn seed_current_database(path: &Path, run_id: &str) {
    let writer = TraceWriter::open(path).expect("create current db");
    let handle = writer.handle();
    handle.send(common::started(run_id));
    handle.send(common::transition(
        run_id,
        &format!("inst-{run_id}"),
        "node-seed",
        zgraph_trace::InstanceStatus::Pending,
        zgraph_trace::InstanceStatus::Completed,
        common::ts(10),
    ));
    handle.send(common::gate(
        run_id,
        &format!("inst-{run_id}"),
        "node-seed",
        0,
        false,
        None,
    ));
    handle.send(common::finished(run_id, RunStatus::Completed));
    writer.flush().expect("flush");
    writer.close().expect("close writer");
}

/// 造一个真实 v5 库（W3a）：在 seed 之上补 WaitingHuman 实例 + human decision，
/// 再降级为 v5 表形状（无 item 列/expansions）。
fn create_v5_database(path: &Path) {
    seed_current_database(path, "run-v5");
    let conn = Connection::open(path).expect("open for v5 fixture");
    // 先降级到 v5 表形状，再写 W3a 数据：human_decisions 对 instances 有
    // ON DELETE CASCADE，若在重建 instances 之前写入会被级联删除。
    downgrade_to_v5_shape(&conn);
    conn.execute_batch(
        "INSERT INTO instances (instance_id, run_id, node_id, node_type, attempt, status, input) \
         VALUES ('inst-waiting', 'run-v5', 'review', 'python_node', 0, 'WaitingHuman', \
         '{\"spec\":\"v5\"}');
         INSERT INTO human_decisions (run_id, instance_id, decision, approver, reason, \
         decision_id, decided_at, consumed_at) VALUES ('run-v5', 'inst-waiting', 'approved', \
         'alice', 'lgtm', 'decision-v5', '2026-01-01T00:00:01.000Z', NULL);",
    )
    .expect("seed human decision");
    assert_eq!(
        human_decision_rows(&conn).len(),
        1,
        "v5 fixture 必须带一条 human decision"
    );
}

/// 用指定 run id 建库，再降到真实 v4 表形状（W3a 前的形状）。
fn seed_and_downgrade_to_v4(path: &Path, run_id: &str) {
    seed_current_database(path, run_id);
    let conn = Connection::open(path).expect("open for downgrade");
    downgrade_to_v5_shape(&conn);
    downgrade_to_v4_shape(&conn);
}

/// 造一个 v4 库：当前版本建库 + 写 `run-v4`，再降到真实 v4 表形状。
fn create_v4_database(path: &Path) {
    seed_and_downgrade_to_v4(path, "run-v4");
}

/// 造一个 v3 库：v4 形状再删 v4 gate 列与 v4 审计行。
fn create_v3_database(path: &Path) {
    seed_and_downgrade_to_v4(path, "run-v3");
    let conn = Connection::open(path).expect("open for downgrade");
    conn.execute_batch(
        "ALTER TABLE gate_results DROP COLUMN verdict_json;\
         ALTER TABLE gate_results DROP COLUMN attempt;\
         ALTER TABLE gate_results DROP COLUMN deferred;\
         DELETE FROM schema_migrations WHERE version = 4;\
         PRAGMA user_version = 3;",
    )
    .expect("downgrade to v3");
    assert_eq!(user_version(&conn), 3);
}

/// 造一个 v2 库：v3 库再删 `runs.global_vars_json` 与 v3 审计行。
fn create_v2_database(path: &Path) {
    seed_and_downgrade_to_v4(path, "run-v2");
    let conn = Connection::open(path).expect("open for downgrade");
    conn.execute_batch(
        "ALTER TABLE gate_results DROP COLUMN verdict_json;\
         ALTER TABLE gate_results DROP COLUMN attempt;\
         ALTER TABLE gate_results DROP COLUMN deferred;\
         ALTER TABLE runs DROP COLUMN global_vars_json;\
         DELETE FROM schema_migrations WHERE version IN (3, 4);\
         PRAGMA user_version = 2;",
    )
    .expect("downgrade to v2");
    assert_eq!(user_version(&conn), 2);
}

fn create_v1_database(path: &Path) {
    let conn = Connection::open(path).expect("create v1 db");
    conn.execute_batch(
        "CREATE TABLE schema_migrations (version INTEGER PRIMARY KEY, description TEXT NOT NULL, \
         applied_at TEXT NOT NULL);\
         CREATE TABLE graphs (graph_hash TEXT PRIMARY KEY, graph_name TEXT, dsl_version TEXT NOT NULL, \
         dsl_json TEXT NOT NULL, created_at TEXT NOT NULL);\
         CREATE TABLE runs (\
           run_id TEXT PRIMARY KEY,\
           graph_hash TEXT NOT NULL REFERENCES graphs(graph_hash),\
           status TEXT NOT NULL CHECK (status IN ('Running','Paused','Completed','Failed','Cancelled')),\
           user_id TEXT, started_at TEXT NOT NULL, finished_at TEXT, entry_inputs TEXT, \
           output_json TEXT, error_code TEXT, error_message TEXT, owner_pid INTEGER, \
           owner_host TEXT, heartbeat_at TEXT,\
           interrupted INTEGER NOT NULL DEFAULT 0 CHECK (interrupted IN (0,1)),\
           recovered_at TEXT, last_seq INTEGER NOT NULL DEFAULT 0,\
           event_count INTEGER NOT NULL DEFAULT 0,\
           streaming_dropped INTEGER NOT NULL DEFAULT 0,\
           abandoned_tasks INTEGER NOT NULL DEFAULT 0,\
           trace_backpressure INTEGER NOT NULL DEFAULT 0);\
         PRAGMA user_version = 1;",
    )
    .expect("create v1 schema");
    assert_eq!(user_version(&conn), 1);
}
