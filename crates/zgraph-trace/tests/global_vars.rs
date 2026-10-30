//! `runs.global_vars_json`（v3 / ISSUE-087）：canonical JSON 落库与读取回显。

mod common;

use serde_json::json;
use zgraph_trace::{RunStartedInfo, TraceInput, TraceWriter, TRACE_SCHEMA_VERSION};

/// 直接读 `runs.global_vars_json` 原始文本（校验 canonical 落库字节）。
fn raw_global_vars(path: &std::path::Path, run_id: &str) -> Option<String> {
    let conn = rusqlite::Connection::open(path).expect("open db");
    conn.query_row(
        "SELECT global_vars_json FROM runs WHERE run_id = ?1",
        [run_id],
        |row| row.get(0),
    )
    .expect("query global_vars_json")
}

#[test]
fn global_vars_is_persisted_as_canonical_json_and_read_back() {
    let fx = common::Fixture::new();
    // 乱序 + 嵌套：落库前必须递归按键排序（canonical JSON）。
    let global_vars = json!({
        "tenant": "acme",
        "limits": {"z": 3, "a": 1},
        "flags": ["beta", "alpha"],
    });
    let mut started = match common::started("run-gv") {
        TraceInput::RunStarted(info) => info,
        other => panic!("expected RunStarted, got {other:?}"),
    };
    started.global_vars = global_vars.clone();
    fx.handle.send(TraceInput::RunStarted(started));
    fx.flush();

    let raw = raw_global_vars(&fx.path, "run-gv").expect("global_vars_json written");
    assert_eq!(
        raw, r#"{"flags":["beta","alpha"],"limits":{"a":1,"z":3},"tenant":"acme"}"#,
        "canonical JSON: object keys recursively sorted"
    );

    let reader = fx.reader();
    assert_eq!(
        reader.schema_version().expect("schema version"),
        TRACE_SCHEMA_VERSION
    );
    let run = reader.run("run-gv").expect("query").expect("run row");
    assert_eq!(
        run.global_vars,
        Some(global_vars),
        "reader exposes the recorded global_vars snapshot"
    );
    fx.close().expect("close");
}

#[test]
fn empty_global_vars_is_recorded_as_empty_object() {
    let fx = common::Fixture::new();
    fx.handle.send(common::started("run-gv-empty"));
    fx.flush();

    assert_eq!(
        raw_global_vars(&fx.path, "run-gv-empty").as_deref(),
        Some("{}")
    );
    let reader = fx.reader();
    let run = reader.run("run-gv-empty").expect("query").expect("run row");
    assert_eq!(run.global_vars, Some(json!({})));
    fx.close().expect("close");
}

#[test]
fn non_object_global_vars_is_rejected_as_fatal_write_error() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("trace.db");
    let writer = TraceWriter::open(&path).expect("open writer");
    let handle = writer.handle();
    handle.send(TraceInput::RunStarted(RunStartedInfo {
        run_id: "run-bad-gv".to_string(),
        graph_hash: "0123456789abcdef".to_string(),
        graph_name: "bad".to_string(),
        dsl_json: "{}".to_string(),
        user_id: "local".to_string(),
        entry_inputs: json!({}),
        global_vars: json!(42),
        seed: 0,
        record_mode: zgraph_trace::RecordMode::Hash,
        owner_pid: std::process::id(),
        owner_host: "test-host".to_string(),
        started_at: common::STARTED_AT.to_string(),
    }));
    assert!(
        writer.flush().is_err(),
        "non-object global_vars is an unrecoverable write error"
    );
    let error = handle.take_error().expect("writer records the error");
    assert!(
        error.contains("global_vars must be a JSON object"),
        "unexpected error: {error}"
    );
    assert!(writer.close().is_err());
    // 事务回滚：坏行不落库（用只读连接确认 runs 为空）。
    let reader = zgraph_trace::TraceReader::open_readonly(&path).expect("reader");
    assert!(
        reader.run("run-bad-gv").expect("query").is_none(),
        "failed RunStarted must be rolled back"
    );
}
