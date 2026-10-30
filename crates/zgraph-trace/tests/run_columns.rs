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
