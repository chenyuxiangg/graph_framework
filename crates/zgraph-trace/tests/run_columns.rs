//! `runs.worker_versions_json`/`runs.model_params_json` 写端原语与读取回显
//! （ISSUE-082；06 §6.2.4）。

mod common;

use serde_json::json;
use zgraph_trace::RunStatus;

const WORKER_VERSIONS: &str = r#"[{"python_version":"3.11.9","sdk_version":"0.1.0"}]"#;
const MODEL_PARAMS: &str =
    r#"[{"provider":"deepseek","name":"deepseek-chat","temperature":0.2,"max_tokens":2048}]"#;

#[test]
fn update_run_columns_writes_both_columns_and_reads_back() {
    let fx = common::Fixture::new();
    fx.handle.send(common::started("run-meta"));
    fx.flush();

    let before = fx
        .reader()
        .run("run-meta")
        .expect("query run")
        .expect("run row exists");
    let updated = fx
        .writer
        .update_run_columns("run-meta", WORKER_VERSIONS, MODEL_PARAMS)
        .expect("update run columns");
    assert_eq!(updated, 1, "existing run must be updated");

    let reader = fx.reader();
    let row = reader
        .run("run-meta")
        .expect("query run")
        .expect("run row exists");
    assert_eq!(
        row.worker_versions,
        Some(json!([{"python_version": "3.11.9", "sdk_version": "0.1.0"}]))
    );
    assert_eq!(
        row.model_params,
        Some(json!([{
            "provider": "deepseek", "name": "deepseek-chat",
            "temperature": 0.2, "max_tokens": 2048
        }]))
    );
    // 仅更新两列：状态/事件计数等既有列不变。
    assert_eq!(row.status, RunStatus::Running);
    assert_eq!(row.event_count, before.event_count);
    assert_eq!(row.last_seq, before.last_seq);
    assert_eq!(row.global_vars, before.global_vars);
    drop(reader);
    fx.close().expect("close");
}

#[test]
fn update_run_columns_is_idempotent_and_unknown_run_updates_nothing() {
    let fx = common::Fixture::new();
    fx.handle.send(common::started("run-meta"));
    fx.flush();

    assert_eq!(
        fx.writer
            .update_run_columns("missing", "[]", "[]")
            .expect("unknown run"),
        0,
        "unknown run updates no row (not an error)"
    );
    // 重复写：后写覆盖（run 结束时只调用一次，幂等重试不报错）。
    fx.writer
        .update_run_columns("run-meta", "[]", "[]")
        .expect("first write");
    assert_eq!(
        fx.writer
            .update_run_columns("run-meta", WORKER_VERSIONS, MODEL_PARAMS)
            .expect("overwrite"),
        1
    );
    let reader = fx.reader();
    let row = reader
        .run("run-meta")
        .expect("query")
        .expect("run row exists");
    assert_eq!(
        row.worker_versions,
        Some(json!([{"python_version": "3.11.9", "sdk_version": "0.1.0"}]))
    );
    drop(reader);
    fx.close().expect("close");
}

/// REV-048 C1：元数据列更新失败是**尽力写**——返回 Err 但不置 writer fatal：
/// 既有数据完整、flush/close 成功，写线程继续服务（可再次成功更新）。
#[test]
fn update_run_columns_failure_is_best_effort_and_writer_survives() {
    let fx = common::Fixture::new();
    fx.handle.send(common::started("run-meta"));
    fx.flush();
    let before = fx
        .reader()
        .run("run-meta")
        .expect("query run")
        .expect("run row exists");

    // 确定性失败注入：UPDATE runs 两列时 ABORT。
    let conn = rusqlite::Connection::open(&fx.path).expect("open db");
    conn.execute_batch(
        "CREATE TRIGGER fail_run_columns BEFORE UPDATE OF worker_versions_json ON runs \
         BEGIN SELECT RAISE(ABORT, 'injected metadata failure'); END;",
    )
    .expect("create trigger");
    drop(conn);

    assert!(
        fx.writer
            .update_run_columns("run-meta", WORKER_VERSIONS, MODEL_PARAMS)
            .is_err(),
        "injected failure must surface as Err"
    );
    assert!(
        fx.handle.take_error().is_none(),
        "metadata failure must NOT poison the writer (best-effort)"
    );
    // 写线程继续可用：flush 成功（未 fail-stop），既有 run/event 数据完整。
    fx.flush();
    let after_failure = fx
        .reader()
        .run("run-meta")
        .expect("query run")
        .expect("run row exists");
    assert_eq!(after_failure.status, before.status);
    assert_eq!(after_failure.event_count, before.event_count);
    assert_eq!(after_failure.worker_versions, None);

    // 去掉 trigger 后同一 writer 可正常更新（证明未进入 fatal）。
    let conn = rusqlite::Connection::open(&fx.path).expect("open db");
    conn.execute_batch("DROP TRIGGER fail_run_columns")
        .expect("drop trigger");
    drop(conn);
    assert_eq!(
        fx.writer
            .update_run_columns("run-meta", WORKER_VERSIONS, MODEL_PARAMS)
            .expect("retry after trigger removal"),
        1
    );
    let row = fx
        .reader()
        .run("run-meta")
        .expect("query run")
        .expect("run row exists");
    assert_eq!(
        row.worker_versions,
        Some(json!([{"python_version": "3.11.9", "sdk_version": "0.1.0"}]))
    );
    // 尽力写失败未污染 close（无 fail-stop）。
    fx.close()
        .expect("close succeeds after best-effort failure");
}
