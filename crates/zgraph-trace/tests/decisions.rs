//! `human_decisions` 读写 API（schema v5 / DR-006 §5/§6.3）：`DecisionStore`、
//! `DecisionConsumer`、`TraceReader::human_decisions`。
//!
//! 覆盖：单事务 INSERT / 唯一键 → `Conflict` / precheck（C3 顺序所需三态）/
//! `pending` 确定性排序 / `mark_consumed` 条件更新与幂等 / 打开语义（不建库、
//! 不迁移、版本不符报错）/ `TraceWriter` 不写该表。

mod common;

use std::path::{Path, PathBuf};

use rusqlite::Connection;
use zgraph_trace::{
    DecisionConsumer, DecisionError, DecisionStore, EventQuery, InstanceStatus, NewHumanDecision,
    RunStatus, TraceError, TraceReader, TraceWriter, TRACE_SCHEMA_VERSION,
};

/// 建一个 v5 库并写入 `run-h`（`inst-wait`=WaitingHuman、`inst-done`=Completed），
/// 关闭写端后按需把 run 状态改成 `Paused`。
fn seeded_db(run_status: RunStatus) -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("trace.db");
    let writer = TraceWriter::open(&path).expect("open writer");
    let handle = writer.handle();
    handle.send(common::started("run-h"));
    handle.send(common::transition(
        "run-h",
        "inst-wait",
        "review",
        InstanceStatus::Pending,
        InstanceStatus::WaitingHuman,
        common::ts(10),
    ));
    handle.send(common::transition(
        "run-h",
        "inst-done",
        "done",
        InstanceStatus::Pending,
        InstanceStatus::Completed,
        common::ts(20),
    ));
    if run_status == RunStatus::Failed {
        handle.send(common::finished("run-h", RunStatus::Failed));
    }
    writer.flush().expect("flush");
    writer.close().expect("close writer");
    if run_status == RunStatus::Paused {
        let conn = Connection::open(&path).expect("open for pause");
        conn.execute(
            "UPDATE runs SET status = 'Paused' WHERE run_id = 'run-h'",
            [],
        )
        .expect("pause run");
    }
    (dir, path)
}

fn decision(
    instance_id: &str,
    decision: &str,
    decision_id: &str,
    decided_at: &str,
) -> NewHumanDecision {
    NewHumanDecision {
        run_id: "run-h".to_string(),
        instance_id: instance_id.to_string(),
        decision: decision.to_string(),
        approver: "alice".to_string(),
        reason: Some("lgtm".to_string()),
        decision_id: decision_id.to_string(),
        decided_at: decided_at.to_string(),
    }
}

fn count_decisions(path: &Path) -> i64 {
    let conn = Connection::open(path).expect("open for count");
    conn.query_row("SELECT count(*) FROM human_decisions", [], |row| row.get(0))
        .expect("count decisions")
}

#[test]
fn decision_store_inserts_row_and_reader_orders_by_decided_at_then_decision_id() {
    let (_dir, path) = seeded_db(RunStatus::Running);
    let mut store = DecisionStore::open(&path).expect("open store");
    // 插入顺序与 decided_at 顺序相反：读取必须按 (decided_at, decision_id) 升序。
    store
        .insert(decision(
            "inst-done",
            "rejected",
            "decision-b",
            "2026-01-01T00:00:02.000Z",
        ))
        .expect("insert second decision");
    store
        .insert(decision(
            "inst-wait",
            "approved",
            "decision-a",
            "2026-01-01T00:00:01.000Z",
        ))
        .expect("insert first decision");

    let reader = TraceReader::open_readonly(&path).expect("reader");
    let rows = reader.human_decisions("run-h").expect("human decisions");
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].instance_id, "inst-wait");
    assert_eq!(rows[0].decision, "approved");
    assert_eq!(rows[0].approver, "alice");
    assert_eq!(rows[0].reason.as_deref(), Some("lgtm"));
    assert_eq!(rows[0].decision_id, "decision-a");
    assert_eq!(rows[0].decided_at, "2026-01-01T00:00:01.000Z");
    assert_eq!(rows[0].consumed_at, None, "decide 不写 consumed_at");
    assert_eq!(rows[1].instance_id, "inst-done");
    // 无决策的 run 返回空 Vec（trace 输出层不追加 HumanDecision 行）。
    assert!(reader
        .human_decisions("run-missing")
        .expect("missing run")
        .is_empty());
}

#[test]
fn duplicate_instance_decision_is_conflict_and_not_idempotent() {
    let (_dir, path) = seeded_db(RunStatus::Running);
    let mut store = DecisionStore::open(&path).expect("open store");
    store
        .insert(decision(
            "inst-wait",
            "approved",
            "decision-1",
            "2026-01-01T00:00:01.000Z",
        ))
        .expect("first insert");

    // 唯一键 (run_id, instance_id) 冲突 → Conflict（不是幂等成功）。
    let error = store
        .insert(decision(
            "inst-wait",
            "rejected",
            "decision-2",
            "2026-01-01T00:00:02.000Z",
        ))
        .expect_err("duplicate instance must conflict");
    assert!(
        matches!(error, DecisionError::Conflict(_)),
        "expected Conflict, got {error:?}"
    );
    assert_eq!(error.code(), "CONFLICT");
    assert_eq!(count_decisions(&path), 1, "冲突不得产生第二行");

    // decision_id UNIQUE 冲突（不同实例复用 id）同样映射为 Conflict。
    let error = store
        .insert(decision(
            "inst-done",
            "approved",
            "decision-1",
            "2026-01-01T00:00:03.000Z",
        ))
        .expect_err("duplicate decision_id must conflict");
    assert!(
        matches!(error, DecisionError::Conflict(_)),
        "expected Conflict, got {error:?}"
    );
    assert_eq!(count_decisions(&path), 1);
}

#[test]
fn decision_store_rejects_invalid_decision_value_as_trace_storage_error() {
    let (_dir, path) = seeded_db(RunStatus::Running);
    let mut store = DecisionStore::open(&path).expect("open store");
    // CHECK(decision IN ('approved','rejected')) 失败 → 存储错误（不是 Conflict）。
    let error = store
        .insert(decision(
            "inst-wait",
            "maybe",
            "decision-bad",
            "2026-01-01T00:00:01.000Z",
        ))
        .expect_err("invalid decision must fail");
    assert!(
        matches!(error, DecisionError::Trace(TraceError::Storage(_))),
        "expected Trace(Storage), got {error:?}"
    );
    assert_eq!(error.code(), "STORAGE_ERROR");
    assert_eq!(count_decisions(&path), 0);
}

#[test]
fn decision_store_insert_requires_existing_run_and_instance() {
    let (_dir, path) = seeded_db(RunStatus::Running);
    let mut store = DecisionStore::open(&path).expect("open store");
    let mut missing_instance = decision(
        "inst-missing",
        "approved",
        "decision-1",
        "2026-01-01T00:00:01.000Z",
    );
    missing_instance.run_id = "run-h".to_string();
    // instance_id FK 指向 instances → 不存在的实例是存储错误（不是 Conflict）。
    let error = store
        .insert(missing_instance)
        .expect_err("missing instance must fail FK");
    assert!(
        matches!(error, DecisionError::Trace(TraceError::Storage(_))),
        "expected Trace(Storage), got {error:?}"
    );
    assert_eq!(count_decisions(&path), 0);
}

#[test]
fn precheck_reports_decision_existence_run_status_and_instance_status() {
    let (_dir, path) = seeded_db(RunStatus::Running);
    let mut store = DecisionStore::open(&path).expect("open store");

    let precheck = store.precheck("run-h", "inst-wait").expect("precheck");
    assert!(!precheck.decision_exists, "尚无决策行");
    assert_eq!(precheck.run_status, Some(RunStatus::Running));
    assert_eq!(precheck.instance_status, Some(InstanceStatus::WaitingHuman));

    // 无决策行时实例状态非 WaitingHuman（C3 第 4 步 → HUMAN_INSTANCE_NOT_WAITING）。
    let precheck = store.precheck("run-h", "inst-done").expect("precheck");
    assert_eq!(precheck.instance_status, Some(InstanceStatus::Completed));

    // 决策行存在性优先于 run/instance 状态（二次 decide 恒 HUMAN_DECISION_EXISTS）。
    store
        .insert(decision(
            "inst-wait",
            "approved",
            "decision-1",
            "2026-01-01T00:00:01.000Z",
        ))
        .expect("insert");
    let precheck = store.precheck("run-h", "inst-wait").expect("precheck");
    assert!(precheck.decision_exists);

    // 目标不存在：run → None；instance → None（实例必须属于该 run）。
    let precheck = store
        .precheck("run-missing", "inst-wait")
        .expect("precheck");
    assert!(!precheck.decision_exists);
    assert_eq!(precheck.run_status, None);
    assert_eq!(precheck.instance_status, None);
    let precheck = store.precheck("run-h", "inst-missing").expect("precheck");
    assert_eq!(precheck.run_status, Some(RunStatus::Running));
    assert_eq!(precheck.instance_status, None);
}

#[test]
fn precheck_reports_paused_and_terminal_run_statuses() {
    let (_dir, path) = seeded_db(RunStatus::Paused);
    let store = DecisionStore::open(&path).expect("open paused store");
    let precheck = store.precheck("run-h", "inst-wait").expect("precheck");
    assert_eq!(
        precheck.run_status,
        Some(RunStatus::Paused),
        "Paused run 允许 decide（DR-006 §4.4 行 8）"
    );

    let (_dir, path) = seeded_db(RunStatus::Failed);
    let store = DecisionStore::open(&path).expect("open failed store");
    let precheck = store.precheck("run-h", "inst-wait").expect("precheck");
    assert_eq!(precheck.run_status, Some(RunStatus::Failed));
}

#[test]
fn decision_consumer_pending_is_ordered_and_mark_consumed_is_idempotent() {
    let (_dir, path) = seeded_db(RunStatus::Running);
    let mut store = DecisionStore::open(&path).expect("open store");
    store
        .insert(decision(
            "inst-done",
            "rejected",
            "decision-b",
            "2026-01-01T00:00:02.000Z",
        ))
        .expect("insert second");
    store
        .insert(decision(
            "inst-wait",
            "approved",
            "decision-a",
            "2026-01-01T00:00:01.000Z",
        ))
        .expect("insert first");

    let consumer = DecisionConsumer::open(&path).expect("open consumer");
    let pending = consumer.pending("run-h").expect("pending");
    assert_eq!(
        pending
            .iter()
            .map(|row| row.instance_id.as_str())
            .collect::<Vec<_>>(),
        vec!["inst-wait", "inst-done"],
        "pending 必须按 (decided_at, decision_id) 升序"
    );

    let first_consumed_at = "2026-01-01T00:00:09.000Z";
    assert!(
        consumer
            .mark_consumed("run-h", "inst-wait", first_consumed_at)
            .expect("mark consumed"),
        "首次标记必须命中"
    );
    // 幂等：已消费行不再更新（返回 false，consumed_at 保持首次值）。
    assert!(
        !consumer
            .mark_consumed("run-h", "inst-wait", "2026-01-01T00:00:10.000Z")
            .expect("second mark"),
        "已消费行不得再次更新"
    );
    assert!(
        !consumer
            .mark_consumed("run-h", "inst-missing", first_consumed_at)
            .expect("unknown row"),
        "不存在的行返回 false"
    );

    let reader = TraceReader::open_readonly(&path).expect("reader");
    let rows = reader.human_decisions("run-h").expect("human decisions");
    assert_eq!(rows[0].consumed_at.as_deref(), Some(first_consumed_at));
    assert_eq!(rows[1].consumed_at, None);
    assert_eq!(
        consumer.pending("run-h").expect("pending").len(),
        1,
        "pending 只返回未消费行"
    );
    assert_eq!(
        reader.schema_version().expect("version"),
        TRACE_SCHEMA_VERSION
    );
}

#[test]
fn decide_insert_is_visible_while_trace_writer_is_open() {
    // C13：decide 与 trace 写线程是不同连接/事务；WAL 下并发且提交后立即可见。
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("trace.db");
    let writer = TraceWriter::open(&path).expect("open writer");
    let handle = writer.handle();
    handle.send(common::started("run-h"));
    handle.send(common::transition(
        "run-h",
        "inst-wait",
        "review",
        InstanceStatus::Pending,
        InstanceStatus::WaitingHuman,
        common::ts(10),
    ));
    writer.flush().expect("flush");

    let mut store = DecisionStore::open(&path).expect("open store");
    store
        .insert(decision(
            "inst-wait",
            "approved",
            "decision-live",
            "2026-01-01T00:00:01.000Z",
        ))
        .expect("insert while writer open");

    let consumer = DecisionConsumer::open(&path).expect("open consumer");
    assert_eq!(
        consumer.pending("run-h").expect("pending").len(),
        1,
        "decide 提交后 consumer 下一轮即可见"
    );
    writer.close().expect("close writer");
}

#[test]
fn decision_store_open_requires_existing_v5_database_and_does_not_migrate() {
    // 库不存在：报错且不创建文件（DR-006 §5.4 / N20）。
    let dir = tempfile::tempdir().expect("tempdir");
    let missing = dir.path().join("missing.db");
    let error = DecisionStore::open(&missing)
        .err()
        .expect("missing db must fail");
    assert!(
        matches!(&error, TraceError::Storage(message) if message.contains("database not found")),
        "unexpected error: {error:?}"
    );
    assert!(!missing.exists(), "DecisionStore::open 不得创建空库");

    // v4 库：不迁移，明确报错（提示由 run 迁移；期望版本取当前常量 6）。
    let (_dir, path) = seeded_db(RunStatus::Running);
    {
        let conn = Connection::open(&path).expect("open for downgrade");
        conn.execute_batch("PRAGMA user_version = 4;")
            .expect("set v4");
    }
    let error = DecisionStore::open(&path)
        .err()
        .expect("v4 db must be rejected");
    assert!(
        matches!(&error, TraceError::Storage(message)
            if message.contains("unsupported schema version=4, expected 6")),
        "unexpected error: {error:?}"
    );
    let error = DecisionConsumer::open(&path)
        .err()
        .expect("consumer v4 db must be rejected");
    assert!(
        matches!(&error, TraceError::Storage(message)
            if message.contains("unsupported schema version=4, expected 6")),
        "unexpected error: {error:?}"
    );
    let conn = Connection::open(&path).expect("inspect");
    let version: i64 = conn
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .expect("version");
    assert_eq!(version, 4, "decision 写者不得迁移库");
}

#[test]
fn decision_store_rejects_too_new_schema() {
    let (_dir, path) = seeded_db(RunStatus::Running);
    {
        let conn = Connection::open(&path).expect("open for upgrade");
        conn.execute_batch("PRAGMA user_version = 99;")
            .expect("set future version");
    }
    match DecisionStore::open(&path) {
        Ok(_) => panic!("expected SchemaTooNew"),
        Err(TraceError::SchemaTooNew { found, supported }) => {
            assert_eq!(found, 99);
            assert_eq!(supported, TRACE_SCHEMA_VERSION);
        }
        Err(other) => panic!("expected SchemaTooNew, got {other:?}"),
    }
}

#[test]
fn waiting_human_transition_severity_is_info() {
    let (_dir, path) = seeded_db(RunStatus::Running);
    let reader = TraceReader::open_readonly(&path).expect("reader");
    let events = reader
        .events("run-h", &EventQuery::default())
        .expect("events");
    let transition = events
        .iter()
        .find(|event| {
            event.kind == "Transition" && event.instance_id.as_deref() == Some("inst-wait")
        })
        .expect("WaitingHuman transition event");
    assert_eq!(
        transition.severity, "info",
        "WaitingHuman 是非错误状态，severity 必须是 info（不得误归 error）"
    );
    let payload = &transition.payload["record"];
    assert_eq!(payload["to"], serde_json::json!("WaitingHuman"));
}
