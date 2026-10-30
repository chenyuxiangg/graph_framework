//! 中止标定 API（06 §6.8.1/§6.8.2，ISSUE-084）与 `RunRow` 判活列（ISSUE-048）。
//!
//! 覆盖：候选只含 Running/Paused 与字段正确；标定只命中候选、单事务原子性；
//! 标定后 reader 可见 Failed/interrupted/recovered_at/error_message 且 error_code=NULL；
//! events/instances/transitions 行数不变（不合成事件）；close 后重开可见；
//! 写线程既有错误通道不被控制消息破坏。

mod common;

use std::path::Path;
use std::time::Duration;

use rusqlite::Connection;
use zgraph_trace::{
    EventQuery, InstanceStatus, RecoveredRun, RunStatus, TraceInput, TraceReader, TraceWriter,
    TransitionRecord, INTERRUPTED_ERROR_MESSAGE, TRACE_PAYLOAD_MAX_BYTES,
};

const RECOVERED_AT: &str = "2026-10-28T10:00:00.000Z";
const PAUSED_HEARTBEAT: &str = "2026-10-28T09:59:00.000Z";

/// 直接连接（测试用）：改状态/建 trigger；设置 busy_timeout 防止与写线程事务抢占。
fn inspect_connection(path: &Path) -> Connection {
    let conn = Connection::open(path).expect("direct connection");
    conn.busy_timeout(Duration::from_secs(5))
        .expect("busy_timeout");
    conn
}

/// 把 run 改成 Paused 并写入可区分的 owner/heartbeat（trace 无暂停事件，只能直改库）。
fn pause_run(fx: &common::Fixture, run_id: &str) {
    let conn = inspect_connection(&fx.path);
    let changed = conn
        .execute(
            "UPDATE runs SET status = 'Paused', owner_pid = 4242, owner_host = 'other-host', \
             heartbeat_at = ?2 WHERE run_id = ?1",
            rusqlite::params![run_id, PAUSED_HEARTBEAT],
        )
        .expect("pause run");
    assert_eq!(changed, 1, "pause update must hit exactly one run");
}

/// 行数快照：`(events, instances, transitions)`。
fn row_counts(reader: &TraceReader, run_id: &str) -> (usize, usize, usize) {
    (
        reader
            .events(run_id, &EventQuery::default())
            .expect("events")
            .len(),
        reader.instances(run_id).expect("instances").len(),
        reader.transitions(run_id).expect("transitions").len(),
    )
}

/// 在 `run_id` 的 UPDATE 上注入 ABORT，用于验证标定事务整体回滚。
fn install_failing_trigger(fx: &common::Fixture, run_id: &str) {
    let conn = inspect_connection(&fx.path);
    conn.execute_batch(&format!(
        "CREATE TRIGGER mark_failure BEFORE UPDATE ON runs \
         WHEN OLD.run_id = '{run_id}' \
         BEGIN SELECT RAISE(ABORT, 'injected mark failure'); END;"
    ))
    .expect("install trigger");
}

#[test]
fn interrupted_candidates_include_only_running_and_paused_runs() {
    let fx = common::Fixture::new();
    let handle = &fx.handle;
    for run_id in [
        "run-running",
        "run-paused",
        "run-completed",
        "run-cancelled",
    ] {
        handle.send(common::started(run_id));
    }
    handle.send(common::finished("run-completed", RunStatus::Completed));
    handle.send(common::finished("run-cancelled", RunStatus::Cancelled));
    fx.flush();
    pause_run(&fx, "run-paused");

    let candidates = fx.writer.interrupted_candidates().expect("candidates");
    let mut ids: Vec<&str> = candidates.iter().map(|item| item.run_id.as_str()).collect();
    ids.sort_unstable();
    assert_eq!(ids, vec!["run-paused", "run-running"]);

    let running = candidates
        .iter()
        .find(|item| item.run_id == "run-running")
        .expect("running candidate");
    assert_eq!(running.status, RunStatus::Running);
    assert_eq!(running.owner_pid, Some(std::process::id()));
    assert_eq!(running.owner_host.as_deref(), Some("test-host"));
    assert_eq!(running.heartbeat_at.as_deref(), Some(common::STARTED_AT));

    let paused = candidates
        .iter()
        .find(|item| item.run_id == "run-paused")
        .expect("paused candidate");
    assert_eq!(paused.status, RunStatus::Paused);
    assert_eq!(paused.owner_pid, Some(4242));
    assert_eq!(paused.owner_host.as_deref(), Some("other-host"));
    assert_eq!(paused.heartbeat_at.as_deref(), Some(PAUSED_HEARTBEAT));
    fx.close().expect("close");
}

/// 混合状态夹具：Running + Paused（候选）+ Completed（非候选），带 1 实例/迁移/2 事件。
fn mixed_run_fixture() -> common::Fixture {
    let fx = common::Fixture::new();
    let handle = &fx.handle;
    handle.send(common::started("run-running"));
    handle.send(common::started("run-paused"));
    handle.send(common::started("run-completed"));
    handle.send(common::transition(
        "run-running",
        "inst-a",
        "node-a",
        InstanceStatus::Pending,
        InstanceStatus::Running,
        common::ts(10),
    ));
    handle.send(common::finished("run-completed", RunStatus::Completed));
    fx.flush();
    pause_run(&fx, "run-paused");
    fx
}

#[test]
fn mark_interrupted_updates_only_running_and_paused_candidates() {
    let fx = mixed_run_fixture();
    let requested =
        ["run-running", "run-paused", "run-completed", "run-missing"].map(str::to_string);
    let recovered = fx
        .writer
        .mark_interrupted(&requested, RECOVERED_AT)
        .expect("mark interrupted");
    let mut marked: Vec<&str> = recovered.iter().map(|item| item.run_id.as_str()).collect();
    marked.sort_unstable();
    assert_eq!(marked, vec!["run-paused", "run-running"]);
    assert!(recovered
        .iter()
        .all(|item: &RecoveredRun| item.recovered_at == RECOVERED_AT));
    assert_eq!(
        recovered
            .iter()
            .find(|item| item.run_id == "run-running")
            .map(|item| item.previous_status),
        Some(RunStatus::Running)
    );
    assert_eq!(
        recovered
            .iter()
            .find(|item| item.run_id == "run-paused")
            .map(|item| item.previous_status),
        Some(RunStatus::Paused)
    );
    assert!(fx
        .writer
        .interrupted_candidates()
        .expect("candidates")
        .is_empty());
    fx.close().expect("close");
}

#[test]
fn mark_interrupted_is_reader_visible_without_synthetic_rows() {
    let fx = mixed_run_fixture();
    let before = row_counts(&fx.reader(), "run-running");
    let requested = ["run-running", "run-paused", "run-completed"].map(str::to_string);
    fx.writer
        .mark_interrupted(&requested, RECOVERED_AT)
        .expect("mark interrupted");

    let reader = fx.reader();
    let run = reader
        .run("run-running")
        .expect("run query")
        .expect("run exists");
    assert_eq!(run.status, RunStatus::Failed);
    assert!(run.interrupted, "interrupted flag set");
    assert_eq!(run.recovered_at.as_deref(), Some(RECOVERED_AT));
    assert_eq!(
        run.error_message.as_deref(),
        Some(INTERRUPTED_ERROR_MESSAGE)
    );
    assert_eq!(
        run.error_code, None,
        "interruption is not a domain error code"
    );
    assert_eq!(run.finished_at, None, "no synthetic RunFinished");
    assert_eq!(run.output_json, None);
    assert_eq!(run.event_count, before.0 as i64);
    assert_eq!(
        row_counts(&reader, "run-running"),
        before,
        "marking must not modify events/instances/transitions"
    );

    let completed = reader
        .run("run-completed")
        .expect("query")
        .expect("completed run");
    assert_eq!(completed.status, RunStatus::Completed);
    assert!(!completed.interrupted);
    assert_eq!(completed.recovered_at, None);
    assert_eq!(
        completed.error_message, None,
        "non-candidate run stays untouched"
    );

    let kinds: Vec<String> = reader
        .events("run-running", &EventQuery::default())
        .expect("events")
        .into_iter()
        .map(|event| event.kind)
        .collect();
    assert!(
        !kinds.iter().any(|kind| kind == "RunFinished"),
        "no RunFinished synthesized: {kinds:?}"
    );
    fx.close().expect("close");
}

#[test]
fn mark_interrupted_rolls_back_whole_transaction_on_failure() {
    let fx = common::Fixture::new();
    fx.handle.send(common::started("run-good"));
    fx.handle.send(common::started("run-bad"));
    fx.flush();
    install_failing_trigger(&fx, "run-bad");

    let requested = ["run-good", "run-bad"].map(str::to_string);
    let error = fx
        .writer
        .mark_interrupted(&requested, RECOVERED_AT)
        .expect_err("injected trigger must fail the transaction");
    assert_eq!(error.code(), "STORAGE_ERROR");
    assert!(
        fx.handle.take_error().is_some(),
        "fatal write error still surfaces through take_error"
    );

    let reader = fx.reader();
    for run_id in ["run-good", "run-bad"] {
        let run = reader.run(run_id).expect("run query").expect("run exists");
        assert_eq!(run.status, RunStatus::Running, "{run_id} rolled back");
        assert!(!run.interrupted);
        assert_eq!(run.recovered_at, None);
        assert_eq!(run.error_message, None);
    }
    assert!(fx.writer.close().is_err(), "fatal writer fails close");
}

#[test]
fn mark_interrupted_is_durable_after_close_and_reopen() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("trace.db");
    let writer = TraceWriter::open(&path).expect("open writer");
    writer.handle().send(common::started("run-crash"));
    writer.flush().expect("flush");

    let requested = ["run-crash".to_string()];
    let recovered = writer
        .mark_interrupted(&requested, RECOVERED_AT)
        .expect("mark interrupted");
    assert_eq!(recovered.len(), 1);
    assert_eq!(
        writer
            .mark_interrupted(&requested, RECOVERED_AT)
            .expect("mark idempotent"),
        Vec::new(),
        "already-terminal run is skipped on re-entry"
    );
    writer.close().expect("close");

    let reader = TraceReader::open_readonly(&path).expect("reopen readonly");
    let run = reader
        .run("run-crash")
        .expect("run query")
        .expect("run exists");
    assert_eq!(run.status, RunStatus::Failed);
    assert!(run.interrupted);
    assert_eq!(run.recovered_at.as_deref(), Some(RECOVERED_AT));
    assert_eq!(
        run.error_message.as_deref(),
        Some(INTERRUPTED_ERROR_MESSAGE)
    );
    assert_eq!(run.error_code, None);
}

#[test]
fn empty_mark_returns_empty_without_queueing() {
    let dir = tempfile::tempdir().expect("tempdir");
    let writer = TraceWriter::open(dir.path().join("trace.db")).expect("open writer");
    assert_eq!(
        writer
            .mark_interrupted(&[], RECOVERED_AT)
            .expect("empty mark"),
        Vec::new()
    );
    writer.handle().send(common::started("run-ok"));
    writer.flush().expect("writer still healthy");
    writer.close().expect("close");
}

#[test]
fn control_messages_surface_existing_fatal_error_channel() {
    let fx = common::Fixture::new();
    fx.handle.send(common::started("run-fatal"));
    let mut snapshot = common::snapshot("run-fatal", "inst-fatal", "node", InstanceStatus::Running);
    snapshot.error_code = None;
    fx.handle.send(TraceInput::Transition {
        record: TransitionRecord {
            run_id: "run-fatal".to_string(),
            instance_id: "inst-fatal".to_string(),
            node_id: "node".to_string(),
            attempt: 0,
            from: InstanceStatus::Pending,
            to: InstanceStatus::Running,
            reason: Some("x".repeat(TRACE_PAYLOAD_MAX_BYTES + 1)),
            ts: common::ts(10),
        },
        snapshot,
    });
    assert!(fx.writer.flush().is_err(), "oversized payload is fatal");
    assert!(fx.handle.take_error().is_some());

    assert!(
        fx.writer.interrupted_candidates().is_err(),
        "candidates observe the same fatal error"
    );
    assert!(
        fx.writer
            .mark_interrupted(&["run-fatal".to_string()], RECOVERED_AT)
            .is_err(),
        "mark observes the same fatal error"
    );
    assert!(fx.writer.close().is_err());
}

#[test]
fn run_row_exposes_owner_and_heartbeat_for_liveness() {
    let fx = common::Fixture::new();
    fx.handle.send(common::started("run-live"));
    fx.flush();
    let reader = fx.reader();
    let run = reader
        .run("run-live")
        .expect("run query")
        .expect("run exists");
    assert_eq!(run.status, RunStatus::Running);
    assert_eq!(run.owner_pid, Some(std::process::id()));
    assert_eq!(run.owner_host.as_deref(), Some("test-host"));
    assert_eq!(run.heartbeat_at.as_deref(), Some(common::STARTED_AT));
    fx.close().expect("close");
}
