//! schema 迁移与只读打开语义（06 §6.7 / §6.6）。

mod common;

use std::path::Path;

use rusqlite::Connection;
use zgraph_trace::{TraceError, TraceReader, TraceWriter, TRACE_SCHEMA_VERSION};

const EXPECTED_TABLES: &[&str] = &[
    "events",
    "gate_results",
    "graphs",
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
    "idx_gate_passed",
    "idx_gate_run_edge",
    "idx_gate_run_seq",
    "idx_instances_error_code",
    "idx_instances_run_node",
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

fn table_columns(conn: &Connection, table: &str) -> Vec<String> {
    let mut stmt = conn
        .prepare(&format!("PRAGMA table_info({table})"))
        .expect("prepare table_info");
    let rows = stmt
        .query_map([], |row| row.get::<_, String>(1))
        .expect("query columns");
    rows.map(|row| row.expect("column name")).collect()
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

#[test]
fn fresh_database_migrates_to_v2_with_all_tables_and_indexes() {
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
    assert_eq!(user_version(&conn), TRACE_SCHEMA_VERSION);
    let migrated: i64 = conn
        .query_row(
            "SELECT count(*) FROM schema_migrations WHERE version = 2",
            [],
            |row| row.get(0),
        )
        .expect("migration row");
    assert_eq!(migrated, 1);
    let runs_columns = table_columns(&conn, "runs");
    for column in [
        "seed",
        "worker_versions_json",
        "model_params_json",
        "record_mode",
        "killed_workers",
    ] {
        assert!(
            runs_columns.iter().any(|name| name == column),
            "runs missing v2 column {column}; have {runs_columns:?}"
        );
    }
    drop(conn);
    drop(reader);
    writer.close().expect("close writer");
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
fn v1_database_is_upgraded_to_v2_in_place() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("v1.db");
    create_v1_database(&path);

    let writer = TraceWriter::open(&path).expect("upgrade v1 db");
    let handle = writer.handle();
    handle.send(common::started("run-v1"));
    handle.send(common::finished(
        "run-v1",
        zgraph_trace::RunStatus::Completed,
    ));
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

    let conn = Connection::open(&path).expect("inspect");
    let runs_columns = table_columns(&conn, "runs");
    for column in [
        "seed",
        "worker_versions_json",
        "model_params_json",
        "record_mode",
    ] {
        assert!(
            runs_columns.iter().any(|name| name == column),
            "missing upgraded column {column}"
        );
    }
    assert!(sqlite_names(&conn, "table")
        .iter()
        .any(|name| name == "llm_calls"));
    drop(conn);
    drop(reader);
    writer.close().expect("close");
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
