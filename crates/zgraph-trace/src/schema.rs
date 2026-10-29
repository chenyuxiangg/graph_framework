//! SQLite schema、连接级 PRAGMA 与 forward-only 迁移（06 §6.2/§6.7）。
//!
//! v2 = spec 06 的 6+2 张表（`runs`/`instances`/`events`/`transitions`/
//! `gate_results`/`streaming_events` + `schema_migrations`/`graphs`）
//! + architecture §5.2 的增量：`runs` 新增 5 列与两张记录表。
//!
//! 版本策略：`PRAGMA user_version` 为权威；`0` = 新库（单事务建 v2），
//! `1` = spec 06 v1 库（补列 + 补表后升到 v2），`2` = 就绪，`>2` = 拒绝。

use std::path::Path;
use std::time::Duration;

use rusqlite::{params, Connection, OpenFlags, Transaction, TransactionBehavior};

use crate::error::TraceError;

/// 本 crate 实现的 trace schema 版本（06 §6.7 + architecture §5.2）。
pub const TRACE_SCHEMA_VERSION: i64 = 2;

/// SQLite `busy_timeout`（毫秒，06 §6.2.1）。
pub const DB_BUSY_TIMEOUT_MS: u32 = 5_000;

/// 写线程遇 `SQLITE_BUSY` 的总尝试次数（06 §6.2.1 `DB_WRITE_RETRY`）。
pub const DB_WRITE_RETRY: u32 = 3;

/// 每次 busy 重试的等待间隔（毫秒，06 §6.2.1）。
pub const DB_WRITE_RETRY_INTERVAL_MS: u64 = 50;

/// 无损/流式统一通道容量（M2 单通道简化，06 §6.4.1；M5 可拆双通道）。
pub const TRACE_CHANNEL_CAP: usize = 8_192;

/// 单批最大 events 行数（00 §0.5 `TRACE_BATCH_ROWS`）。
pub const TRACE_BATCH_ROWS: usize = 256;

/// 批量提交最大延迟，同时是流式聚合窗口（00 §0.5 / 06 §6.5.2）。
pub const TRACE_BATCH_INTERVAL_MS: u64 = 100;

/// 写线程刷新 `runs.heartbeat_at` 的间隔（06 §6.4.3）。
pub const HEARTBEAT_INTERVAL_MS: u64 = 5_000;

/// 写线程周期 `wal_checkpoint(PASSIVE)` 间隔（06 §6.4.3）。
pub const WAL_CHECKPOINT_INTERVAL_MS: u64 = 5_000;

/// 流式聚合窗口（毫秒，06 §6.5.2 `STREAM_TOKEN_AGGREGATE_MS`）。
pub const STREAM_TOKEN_AGGREGATE_MS: u64 = 100;

/// token/reasoning 单段最大字符数（06 §6.5.2 `STREAM_SEGMENT_MAX_CHARS`）。
pub const STREAM_SEGMENT_MAX_CHARS: usize = 4_096;

/// token/reasoning 单段最大 UTF-8 字节数（06 §6.5.2 `STREAM_SEGMENT_MAX_BYTES`）。
pub const STREAM_SEGMENT_MAX_BYTES: usize = 16_384;

/// 单条 `events.payload` 最大字节数（06 §6.4.2 `TRACE_PAYLOAD_MAX_BYTES`）。
pub const TRACE_PAYLOAD_MAX_BYTES: usize = 65_536;

/// v1 → v2 需要补到 `runs` 的列（architecture §5.2）。
const RUNS_V2_COLUMNS: &[(&str, &str)] = &[
    ("seed", "seed INTEGER NOT NULL DEFAULT 0"),
    ("worker_versions_json", "worker_versions_json TEXT"),
    ("model_params_json", "model_params_json TEXT"),
    ("record_mode", "record_mode TEXT"),
    (
        "killed_workers",
        "killed_workers INTEGER NOT NULL DEFAULT 0",
    ),
];

/// v2 完整 DDL（全部 `IF NOT EXISTS`，单事务执行；06 §6.2 + v2 增量）。
const DDL_V2: &str = r#"
CREATE TABLE IF NOT EXISTS schema_migrations (
  version     INTEGER PRIMARY KEY,
  description TEXT    NOT NULL,
  applied_at  TEXT    NOT NULL
);

CREATE TABLE IF NOT EXISTS graphs (
  graph_hash  TEXT PRIMARY KEY,
  graph_name  TEXT,
  dsl_version TEXT NOT NULL,
  dsl_json    TEXT NOT NULL,
  created_at  TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS runs (
  run_id           TEXT PRIMARY KEY,
  graph_hash       TEXT NOT NULL REFERENCES graphs(graph_hash),
  status           TEXT NOT NULL
                   CHECK (status IN ('Running','Paused','Completed','Failed','Cancelled')),
  user_id          TEXT,
  started_at       TEXT NOT NULL,
  finished_at      TEXT,
  entry_inputs     TEXT,
  output_json      TEXT,
  error_code       TEXT,
  error_message    TEXT,
  owner_pid        INTEGER,
  owner_host       TEXT,
  heartbeat_at     TEXT,
  interrupted      INTEGER NOT NULL DEFAULT 0 CHECK (interrupted IN (0,1)),
  recovered_at     TEXT,
  last_seq         INTEGER NOT NULL DEFAULT 0,
  event_count      INTEGER NOT NULL DEFAULT 0,
  streaming_dropped INTEGER NOT NULL DEFAULT 0,
  abandoned_tasks  INTEGER NOT NULL DEFAULT 0,
  trace_backpressure INTEGER NOT NULL DEFAULT 0,
  seed             INTEGER NOT NULL DEFAULT 0,
  worker_versions_json TEXT,
  model_params_json TEXT,
  record_mode      TEXT,
  killed_workers   INTEGER NOT NULL DEFAULT 0
);

CREATE INDEX IF NOT EXISTS idx_runs_status_started ON runs(status, started_at);
CREATE INDEX IF NOT EXISTS idx_runs_unfinished ON runs(status) WHERE status IN ('Running','Paused');

CREATE TABLE IF NOT EXISTS instances (
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

CREATE INDEX IF NOT EXISTS idx_instances_run_node   ON instances(run_id, node_id);
CREATE INDEX IF NOT EXISTS idx_instances_run_status ON instances(run_id, status);
CREATE INDEX IF NOT EXISTS idx_instances_error_code ON instances(error_code) WHERE error_code IS NOT NULL;

CREATE TABLE IF NOT EXISTS events (
  seq         INTEGER PRIMARY KEY AUTOINCREMENT,
  run_id      TEXT NOT NULL REFERENCES runs(run_id) ON DELETE CASCADE,
  kind        TEXT NOT NULL CHECK (kind IN
              ('RunStarted','Transition','GateResult','NodeTimedOut',
               'NodeAbandoned','Streaming','RunFinished')),
  severity    TEXT NOT NULL DEFAULT 'info' CHECK (severity IN ('info','warn','error')),
  at          TEXT NOT NULL,
  instance_id TEXT,
  node_id     TEXT,
  payload     TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_events_run_seq  ON events(run_id, seq);
CREATE INDEX IF NOT EXISTS idx_events_kind     ON events(kind, seq);
CREATE INDEX IF NOT EXISTS idx_events_instance ON events(instance_id, seq) WHERE instance_id IS NOT NULL;

CREATE TABLE IF NOT EXISTS transitions (
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

CREATE INDEX IF NOT EXISTS idx_transitions_run_seq    ON transitions(run_id, seq);
CREATE INDEX IF NOT EXISTS idx_transitions_instance   ON transitions(instance_id, seq);
CREATE INDEX IF NOT EXISTS idx_transitions_run_status ON transitions(run_id, to_status);

CREATE TABLE IF NOT EXISTS gate_results (
  gate_result_id INTEGER PRIMARY KEY AUTOINCREMENT,
  seq            INTEGER NOT NULL UNIQUE REFERENCES events(seq) ON DELETE CASCADE,
  run_id         TEXT NOT NULL,
  instance_id    TEXT NOT NULL,
  node_id        TEXT NOT NULL,
  edge_index     INTEGER NOT NULL,
  to_node        TEXT NOT NULL,
  to_port        TEXT NOT NULL,
  gate_fn        TEXT,
  passed         INTEGER NOT NULL CHECK (passed IN (0,1)),
  error          TEXT,
  at             TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_gate_run_seq  ON gate_results(run_id, seq);
CREATE INDEX IF NOT EXISTS idx_gate_run_edge ON gate_results(run_id, edge_index);
CREATE INDEX IF NOT EXISTS idx_gate_passed   ON gate_results(run_id, passed);

CREATE TABLE IF NOT EXISTS streaming_events (
  streaming_event_id INTEGER PRIMARY KEY AUTOINCREMENT,
  seq            INTEGER NOT NULL UNIQUE REFERENCES events(seq) ON DELETE CASCADE,
  run_id         TEXT NOT NULL,
  instance_id    TEXT NOT NULL,
  kind           TEXT NOT NULL CHECK (kind IN
                 ('token','reasoning','tool_call_start','tool_call_end','progress')),
  segment_index  INTEGER NOT NULL,
  text           TEXT,
  text_chars     INTEGER,
  text_bytes     INTEGER,
  truncated      INTEGER NOT NULL DEFAULT 0 CHECK (truncated IN (0,1)),
  delta_count    INTEGER,
  call_id        TEXT,
  tool_name      TEXT,
  ok             INTEGER,
  summary        TEXT,
  percent        INTEGER,
  first_at       TEXT NOT NULL,
  last_at        TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_stream_run_seq   ON streaming_events(run_id, seq);
CREATE INDEX IF NOT EXISTS idx_stream_instance  ON streaming_events(run_id, instance_id, segment_index);
CREATE INDEX IF NOT EXISTS idx_stream_tool      ON streaming_events(run_id, call_id) WHERE call_id IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_stream_kind      ON streaming_events(run_id, kind, segment_index);

CREATE TABLE IF NOT EXISTS recorded_outputs (
  recorded_output_id INTEGER PRIMARY KEY AUTOINCREMENT,
  run_id        TEXT NOT NULL REFERENCES runs(run_id) ON DELETE CASCADE,
  instance_id   TEXT NOT NULL,
  node_id       TEXT NOT NULL,
  attempt       INTEGER NOT NULL,
  output_hash   TEXT NOT NULL,
  output_json   TEXT,
  recorded_at   TEXT NOT NULL,
  UNIQUE (run_id, instance_id, attempt)
);

CREATE INDEX IF NOT EXISTS idx_recorded_outputs_run ON recorded_outputs(run_id);

CREATE TABLE IF NOT EXISTS llm_calls (
  llm_call_id   INTEGER PRIMARY KEY AUTOINCREMENT,
  run_id        TEXT NOT NULL REFERENCES runs(run_id) ON DELETE CASCADE,
  instance_id   TEXT NOT NULL,
  call_index    INTEGER NOT NULL,
  request_json  TEXT NOT NULL,
  response_json TEXT,
  status        TEXT NOT NULL,
  recorded_at   TEXT NOT NULL,
  UNIQUE (run_id, instance_id, call_index)
);

CREATE INDEX IF NOT EXISTS idx_llm_calls_run ON llm_calls(run_id);
"#;

/// 打开（必要时创建）写库，完成版本检查与迁移并应用写连接 PRAGMA。
pub(crate) fn open_writer_connection(path: &Path) -> Result<Connection, TraceError> {
    ensure_parent_dir(path)?;
    let mut conn = Connection::open(path).map_err(db_err)?;
    let version = user_version(&conn)?;
    if version > TRACE_SCHEMA_VERSION {
        return Err(TraceError::SchemaTooNew {
            found: version,
            supported: TRACE_SCHEMA_VERSION,
        });
    }
    if version < 0 {
        return Err(TraceError::Storage(format!(
            "invalid schema version {version} in {}",
            path.display()
        )));
    }
    apply_writer_pragmas(&conn)?;
    match version {
        0 => create_v2(&mut conn),
        1 => upgrade_v1_to_v2(&mut conn),
        _ => Ok(()),
    }?;
    Ok(conn)
}

/// 打开只读连接（`SQLITE_OPEN_READ_ONLY`；不创建、不迁移、不标定，06 §6.6）。
pub(crate) fn open_readonly_connection(path: &Path) -> Result<Connection, TraceError> {
    let metadata = std::fs::metadata(path).map_err(|err| match err.kind() {
        std::io::ErrorKind::NotFound => {
            TraceError::Storage(format!("database not found: {}", path.display()))
        }
        _ => TraceError::Storage(format!("cannot stat {}: {err}", path.display())),
    })?;
    if metadata.is_dir() {
        return Err(TraceError::Storage(format!(
            "database path is a directory: {}",
            path.display()
        )));
    }
    let conn =
        Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY).map_err(|err| {
            TraceError::Storage(format!("database not found: {} ({err})", path.display()))
        })?;
    conn.busy_timeout(Duration::from_millis(u64::from(DB_BUSY_TIMEOUT_MS)))
        .map_err(db_err)?;
    conn.execute_batch("PRAGMA query_only = ON; PRAGMA temp_store = MEMORY;")
        .map_err(db_err)?;
    ensure_trace_schema(&conn, path)?;
    Ok(conn)
}

/// 读取 `PRAGMA user_version`。
pub(crate) fn user_version(conn: &Connection) -> Result<i64, TraceError> {
    conn.query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))
        .map_err(db_err)
}

/// 当前 UTC RFC3339 毫秒字符串（`Z` 结尾；写线程仅用于 heartbeat 与迁移记录）。
pub(crate) fn now_rfc3339_millis() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

/// 把 rusqlite 错误收敛为 [`TraceError::Storage`]。
pub(crate) fn db_err(err: rusqlite::Error) -> TraceError {
    TraceError::Storage(err.to_string())
}

fn create_v2(conn: &mut Connection) -> Result<(), TraceError> {
    let tx = begin_immediate(conn)?;
    tx.execute_batch(DDL_V2).map_err(db_err)?;
    record_migration(&tx)?;
    tx.commit().map_err(db_err)?;
    Ok(())
}

fn upgrade_v1_to_v2(conn: &mut Connection) -> Result<(), TraceError> {
    let tx = begin_immediate(conn)?;
    tx.execute_batch(DDL_V2).map_err(db_err)?;
    let existing = table_columns(&tx, "runs")?;
    for (name, definition) in RUNS_V2_COLUMNS {
        if !existing.iter().any(|column| column == name) {
            let sql = format!("ALTER TABLE runs ADD COLUMN {definition};");
            tx.execute_batch(&sql).map_err(db_err)?;
        }
    }
    record_migration(&tx)?;
    tx.commit().map_err(db_err)?;
    Ok(())
}

fn begin_immediate(conn: &mut Connection) -> Result<Transaction<'_>, TraceError> {
    conn.transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(db_err)
}

fn record_migration(tx: &Transaction<'_>) -> Result<(), TraceError> {
    tx.execute(
        "INSERT OR REPLACE INTO schema_migrations (version, description, applied_at) \
         VALUES (?1, ?2, ?3)",
        params![
            TRACE_SCHEMA_VERSION,
            "v2: trace schema + reproducibility columns/tables",
            now_rfc3339_millis()
        ],
    )
    .map_err(db_err)?;
    // user_version 存于 DB 头，事务内写入可随回滚一起撤销。
    tx.execute_batch(&format!("PRAGMA user_version = {TRACE_SCHEMA_VERSION};"))
        .map_err(db_err)?;
    Ok(())
}

fn apply_writer_pragmas(conn: &Connection) -> Result<(), TraceError> {
    conn.busy_timeout(Duration::from_millis(u64::from(DB_BUSY_TIMEOUT_MS)))
        .map_err(db_err)?;
    // journal_mode 返回一行；用 query_row 显式消费结果。
    conn.query_row("PRAGMA journal_mode = WAL", [], |_row| Ok(()))
        .map_err(db_err)?;
    conn.execute_batch(
        "PRAGMA synchronous = NORMAL;\n\
         PRAGMA foreign_keys = ON;\n\
         PRAGMA wal_autocheckpoint = 1000;\n\
         PRAGMA temp_store = MEMORY;",
    )
    .map_err(db_err)?;
    Ok(())
}

fn ensure_trace_schema(conn: &Connection, path: &Path) -> Result<(), TraceError> {
    let count: i64 = conn
        .query_row(
            "SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name = 'runs'",
            [],
            |row| row.get(0),
        )
        .map_err(db_err)?;
    if count == 0 {
        return Err(TraceError::Storage(format!(
            "database not found: no trace schema in {}",
            path.display()
        )));
    }
    Ok(())
}

fn ensure_parent_dir(path: &Path) -> Result<(), TraceError> {
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent).map_err(|err| {
            TraceError::Storage(format!(
                "cannot create database directory {}: {err}",
                parent.display()
            ))
        })?;
    }
    Ok(())
}

fn table_columns(conn: &Connection, table: &str) -> Result<Vec<String>, TraceError> {
    let sql = format!("PRAGMA table_info({table})");
    let mut stmt = conn.prepare(&sql).map_err(db_err)?;
    let rows = stmt
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(db_err)?;
    let mut columns = Vec::new();
    for row in rows {
        columns.push(row.map_err(db_err)?);
    }
    Ok(columns)
}
