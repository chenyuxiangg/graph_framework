//! `zgraph decide` CLI 契约测试（W3a / DR-006 §4.5/§7.2）。
//!
//! 用 `zgraph_trace::TraceWriter` 造真实 schema v5 临时库（run + `WaitingHuman`
//! 实例），再通过 `CARGO_BIN_EXE_zgraph` 运行真实二进制。覆盖：全部退出码
//! （0/1/2/4/5）、`--json` 成功/失败形状、人类输出、空库**不创建**、`user_version`
//! 非 5 **不迁移**、唯一键（串行第二次 + 并发恰一成功）、预检顺序
//! （决策行先于 run/instance 状态判定）。
//!
//! 稳定性约定（AGENTS §8）：每个用例独立 `tempfile::TempDir` 与独立 DB；并发用例
//! 只断言退出码集合与行数（不做 sleep 猜测）；无网络。

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use rusqlite::Connection;
use serde_json::Value;
use tempfile::TempDir;
use zgraph_trace::{
    InstanceSnapshot, InstanceStatus, RecordMode, RunFinishedInfo, RunMetrics, RunStartedInfo,
    RunStatus, TraceHandle, TraceInput, TraceWriter, TransitionRecord,
};

const RUN_ID: &str = "run-decide-1";
const OTHER_RUN_ID: &str = "run-decide-2";
const GRAPH_HASH: &str = "0123456789abcdef";
const WAIT_INSTANCE: &str = "inst-wait";
const DONE_INSTANCE: &str = "inst-done";
const OTHER_INSTANCE: &str = "inst-other";

/// 一次测试使用的库（`_dir` 持有 tempdir，`Drop` 时清理）。
struct Fixture {
    _dir: TempDir,
    path: PathBuf,
}

impl Fixture {
    /// 库文件路径。
    fn path(&self) -> &Path {
        &self.path
    }
}

fn path_str(path: &Path) -> &str {
    path.to_str().expect("utf8 path")
}

fn zgraph(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_zgraph"))
        .args(args)
        .output()
        .expect("zgraph binary runs")
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).to_string()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).to_string()
}

fn code(output: &Output) -> i32 {
    output.status.code().expect("exit code")
}

fn json_value(output: &Output) -> Value {
    serde_json::from_str(stdout(output).trim()).expect("stdout is one JSON object")
}

fn run_started(run_id: &str) -> TraceInput {
    TraceInput::RunStarted(RunStartedInfo {
        run_id: run_id.to_string(),
        graph_hash: GRAPH_HASH.to_string(),
        graph_name: "human_gate".to_string(),
        dsl_json: "{}".to_string(),
        user_id: "local".to_string(),
        entry_inputs: serde_json::json!({}),
        global_vars: serde_json::json!({}),
        seed: 7,
        record_mode: RecordMode::Hash,
        owner_pid: std::process::id(),
        owner_host: "test-host".to_string(),
        started_at: "2026-10-31T10:00:00.000Z".to_string(),
    })
}

fn run_finished(run_id: &str) -> TraceInput {
    TraceInput::RunFinished(RunFinishedInfo {
        run_id: run_id.to_string(),
        status: RunStatus::Completed,
        output_summary: Some(serde_json::json!({"result": "ok"})),
        error: None,
        metrics: RunMetrics {
            streaming_dropped: 0,
            abandoned_tasks: 0,
            trace_backpressure: 0,
            killed_workers: 0,
        },
        at: "2026-10-31T10:00:01.000Z".to_string(),
    })
}

/// `Pending → to` 迁移（`reason` 固定，仅用于构造实例行）。
///
/// `node_id` 取实例名：`instances` 的唯一键是 `(run_id, node_id, attempt)`，同一 run
/// 内多个测试实例必须对应不同节点。
fn transition(run_id: &str, instance: &str, to: InstanceStatus) -> TraceInput {
    TraceInput::Transition {
        record: TransitionRecord {
            run_id: run_id.to_string(),
            instance_id: instance.to_string(),
            node_id: instance.to_string(),
            attempt: 0,
            from: InstanceStatus::Pending,
            to,
            reason: Some("test_setup".to_string()),
            ts: "2026-10-31T10:00:00.500Z".to_string(),
        },
        snapshot: InstanceSnapshot {
            run_id: run_id.to_string(),
            instance_id: instance.to_string(),
            node_id: instance.to_string(),
            node_type: "python_node".to_string(),
            attempt: 0,
            item_index: 0,
            item_key: None,
            status: to,
            input: Some(serde_json::json!({"spec": "review me"})),
            output: None,
            error: None,
            error_code: None,
            created_at: "2026-10-31T10:00:00.400Z".to_string(),
            started_at: None,
            finished_at: None,
        },
    }
}

/// 造库：`RUN_ID`（可选已完成）+ `WaitingHuman`/`Completed`/他 run 实例各一。
fn fixture(run_finished_at_end: bool) -> Fixture {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("trace.db");
    let writer = TraceWriter::open(&path).expect("open trace writer");
    let handle: TraceHandle = writer.handle();
    handle.send(run_started(RUN_ID));
    handle.send(run_started(OTHER_RUN_ID));
    handle.send(transition(
        RUN_ID,
        WAIT_INSTANCE,
        InstanceStatus::WaitingHuman,
    ));
    handle.send(transition(RUN_ID, DONE_INSTANCE, InstanceStatus::Completed));
    handle.send(transition(
        OTHER_RUN_ID,
        OTHER_INSTANCE,
        InstanceStatus::WaitingHuman,
    ));
    if run_finished_at_end {
        handle.send(run_finished(RUN_ID));
    }
    writer.flush().expect("flush");
    writer.close().expect("close");
    Fixture { _dir: dir, path }
}

/// 查询决策行：`(decision, approver, reason, decision_id, decided_at, consumed_at)`。
type DecisionRow = (
    String,
    String,
    Option<String>,
    String,
    String,
    Option<String>,
);

fn decision_rows(path: &Path) -> Vec<DecisionRow> {
    let conn = Connection::open(path).expect("open db");
    let mut stmt = conn
        .prepare(
            "SELECT decision, approver, reason, decision_id, decided_at, consumed_at \
             FROM human_decisions ORDER BY decided_at, decision_id",
        )
        .expect("prepare");
    let rows = stmt
        .query_map([], |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
                row.get(5)?,
            ))
        })
        .expect("query")
        .collect::<Result<Vec<DecisionRow>, _>>()
        .expect("collect");
    rows
}

fn user_version(path: &Path) -> i64 {
    Connection::open(path)
        .expect("open db")
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .expect("user_version")
}

/// 标准 decide 参数（默认 approved / alice / 无 reason）。
fn decide_args(db: &Path) -> Vec<String> {
    vec![
        "decide".to_string(),
        RUN_ID.to_string(),
        "--instance".to_string(),
        WAIT_INSTANCE.to_string(),
        "--decision".to_string(),
        "approved".to_string(),
        "--approver".to_string(),
        "alice".to_string(),
        "--db".to_string(),
        path_str(db).to_string(),
    ]
}

fn run_args(args: &[String]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_zgraph"))
        .args(args)
        .output()
        .expect("zgraph binary runs")
}

// ---------------------------------------------------------------------------
// 成功路径
// ---------------------------------------------------------------------------

#[test]
fn decide_records_waiting_human_decision_and_prints_summary() {
    let fx = fixture(false);
    let output = run_args(&decide_args(fx.path()));
    assert_eq!(code(&output), 0, "stderr={}", stderr(&output));
    assert!(
        stderr(&output).is_empty(),
        "human success must not write stderr: {}",
        stderr(&output)
    );
    let text = stdout(&output);
    let prefix = format!(
        "decision recorded: run={RUN_ID} instance={WAIT_INSTANCE} decision=approved decision_id="
    );
    assert!(text.starts_with(&prefix), "{text}");
    let decision_id = text.trim().strip_prefix(&prefix).expect("decision id");
    assert_eq!(decision_id.len(), 36, "{decision_id}");
    assert_eq!(&decision_id[14..15], "7", "UUIDv7: {decision_id}");

    let rows = decision_rows(fx.path());
    assert_eq!(rows.len(), 1, "{rows:?}");
    let (decision, approver, reason, row_decision_id, decided_at, consumed_at) = &rows[0];
    assert_eq!(decision, "approved");
    assert_eq!(approver, "alice");
    assert_eq!(*reason, None, "未提供 --reason → NULL");
    assert_eq!(row_decision_id, decision_id);
    assert!(decided_at.ends_with('Z'), "{decided_at}");
    assert_eq!(*consumed_at, None, "decide 不写 consumed_at");
    assert_eq!(user_version(fx.path()), 7, "decide 不迁移/不改版本");
}

#[test]
fn decide_json_success_has_documented_shape() {
    let fx = fixture(false);
    let mut args = decide_args(fx.path());
    args.push("--json".to_string());
    let output = run_args(&args);
    assert_eq!(code(&output), 0, "stderr={}", stderr(&output));
    let value = json_value(&output);
    assert_eq!(value["ok"], serde_json::json!(true));
    assert_eq!(value["run_id"], serde_json::json!(RUN_ID));
    assert_eq!(value["instance_id"], serde_json::json!(WAIT_INSTANCE));
    assert_eq!(value["decision"], serde_json::json!("approved"));
    assert!(value["decision_id"].is_string());
    assert!(value["decided_at"]
        .as_str()
        .expect("decided_at")
        .ends_with('Z'));
    // stdout 只有一行 JSON。
    assert_eq!(stdout(&output).trim().lines().count(), 1);
}

#[test]
fn decide_rejected_persists_reason_verbatim() {
    let fx = fixture(false);
    let mut args = decide_args(fx.path());
    let decision_at = args
        .iter()
        .position(|arg| arg == "approved")
        .expect("decision");
    args[decision_at] = "rejected".to_string();
    let reason_at = args.iter().position(|arg| arg == "--db").expect("--db");
    args.insert(reason_at, "  keep me  ".to_string());
    args.insert(reason_at, "--reason".to_string());
    let output = run_args(&args);
    assert_eq!(code(&output), 0, "stderr={}", stderr(&output));
    let rows = decision_rows(fx.path());
    assert_eq!(rows[0].0, "rejected");
    assert_eq!(rows[0].2.as_deref(), Some("  keep me  "), "reason 原样保存");
}

// ---------------------------------------------------------------------------
// 冲突与预检（DR-006 §4.5 C3）
// ---------------------------------------------------------------------------

#[test]
fn second_decide_is_conflict_and_keeps_single_row() {
    let fx = fixture(false);
    let output = run_args(&decide_args(fx.path()));
    assert_eq!(code(&output), 0, "stderr={}", stderr(&output));

    let second = run_args(&decide_args(fx.path()));
    assert_eq!(code(&second), 5, "stderr={}", stderr(&second));
    assert!(
        stderr(&second).contains("error[HUMAN_DECISION_EXISTS]"),
        "{}",
        stderr(&second)
    );
    assert!(stdout(&second).is_empty(), "{}", stdout(&second));
    assert_eq!(decision_rows(fx.path()).len(), 1);
}

#[test]
fn decision_exists_precedes_run_status_checks() {
    // ① 先于一切：即使 run 已 Completed，二次 decide 仍 HUMAN_DECISION_EXISTS。
    let fx = fixture(false);
    assert_eq!(code(&run_args(&decide_args(fx.path()))), 0);
    Connection::open(fx.path())
        .expect("open db")
        .execute(
            "UPDATE runs SET status = 'Completed' WHERE run_id = ?1",
            [RUN_ID],
        )
        .expect("mark completed");
    let second = run_args(&decide_args(fx.path()));
    assert_eq!(code(&second), 5, "stderr={}", stderr(&second));
    assert!(
        stderr(&second).contains("HUMAN_DECISION_EXISTS"),
        "{}",
        stderr(&second)
    );
}

#[test]
fn run_not_found_exits_1_with_json_code() {
    let fx = fixture(false);
    let mut args = decide_args(fx.path());
    args[1] = "missing-run".to_string();
    args.push("--json".to_string());
    let output = run_args(&args);
    assert_eq!(code(&output), 1, "stderr={}", stderr(&output));
    let value = json_value(&output);
    assert_eq!(value["ok"], serde_json::json!(false));
    assert_eq!(value["code"], serde_json::json!("HUMAN_RUN_NOT_ACTIVE"));
    assert!(value["message"]
        .as_str()
        .expect("message")
        .contains("missing-run"));
}

#[test]
fn terminal_run_exits_1_not_active() {
    let fx = fixture(true);
    let output = run_args(&decide_args(fx.path()));
    assert_eq!(code(&output), 1, "stderr={}", stderr(&output));
    assert!(
        stderr(&output).contains("error[HUMAN_RUN_NOT_ACTIVE]"),
        "{}",
        stderr(&output)
    );
    assert!(decision_rows(fx.path()).is_empty());
}

#[test]
fn missing_instance_and_foreign_run_instance_exit_1() {
    let fx = fixture(false);
    // 不存在的实例。
    let mut args = decide_args(fx.path());
    args[3] = "inst-missing".to_string();
    args.push("--json".to_string());
    let output = run_args(&args);
    assert_eq!(code(&output), 1, "stderr={}", stderr(&output));
    assert_eq!(
        json_value(&output)["code"],
        serde_json::json!("HUMAN_INSTANCE_NOT_FOUND")
    );

    // 属于另一个 run 的实例 → 对目标 run 而言「不存在」。
    let mut args = decide_args(fx.path());
    args[3] = OTHER_INSTANCE.to_string();
    let output = run_args(&args);
    assert_eq!(code(&output), 1, "stderr={}", stderr(&output));
    assert!(
        stderr(&output).contains("HUMAN_INSTANCE_NOT_FOUND"),
        "{}",
        stderr(&output)
    );
    assert!(decision_rows(fx.path()).is_empty());
}

#[test]
fn instance_not_waiting_human_exits_5() {
    let fx = fixture(false);
    let mut args = decide_args(fx.path());
    args[3] = DONE_INSTANCE.to_string();
    args.push("--json".to_string());
    let output = run_args(&args);
    assert_eq!(code(&output), 5, "stderr={}", stderr(&output));
    let value = json_value(&output);
    assert_eq!(
        value["code"],
        serde_json::json!("HUMAN_INSTANCE_NOT_WAITING")
    );
    assert!(value["message"]
        .as_str()
        .expect("message")
        .contains("Completed"));
    assert!(decision_rows(fx.path()).is_empty());
}

// ---------------------------------------------------------------------------
// 存储/版本（不创建空库、不迁移）
// ---------------------------------------------------------------------------

#[test]
fn missing_database_exits_4_and_is_not_created() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("missing.db");
    let output = run_args(&decide_args(&path));
    assert_eq!(code(&output), 4, "stderr={}", stderr(&output));
    assert!(
        stderr(&output).contains("error[HUMAN_DB_ERROR]"),
        "{}",
        stderr(&output)
    );
    assert!(!path.exists(), "decide must not create an empty database");
}

#[test]
fn old_schema_version_exits_4_without_migrating() {
    let fx = fixture(false);
    Connection::open(fx.path())
        .expect("open db")
        .execute_batch("PRAGMA user_version = 4;")
        .expect("downgrade version");
    let output = run_args(&decide_args(fx.path()));
    assert_eq!(code(&output), 4, "stderr={}", stderr(&output));
    let text = stderr(&output);
    assert!(text.contains("error[HUMAN_DB_ERROR]"), "{text}");
    assert!(text.contains("unsupported schema version=4"), "{text}");
    assert!(text.contains("zgraph run"), "migration hint: {text}");
    assert_eq!(user_version(fx.path()), 4, "decide 不迁移");
    assert!(decision_rows(fx.path()).is_empty());
}

#[test]
fn newer_schema_version_exits_4() {
    let fx = fixture(false);
    Connection::open(fx.path())
        .expect("open db")
        .execute_batch("PRAGMA user_version = 8;")
        .expect("upgrade version");
    let output = run_args(&decide_args(fx.path()));
    assert_eq!(code(&output), 4, "stderr={}", stderr(&output));
    assert!(
        stderr(&output).contains("error[HUMAN_DB_ERROR]"),
        "{}",
        stderr(&output)
    );
    assert!(stderr(&output).contains("8"), "{}", stderr(&output));
}

// ---------------------------------------------------------------------------
// 用法与并发
// ---------------------------------------------------------------------------

#[test]
fn usage_errors_exit_2() {
    let cases: Vec<Vec<&str>> = vec![
        vec!["decide"],
        vec!["decide", RUN_ID],
        vec!["decide", RUN_ID, "--instance", WAIT_INSTANCE],
        vec![
            "decide",
            RUN_ID,
            "--instance",
            WAIT_INSTANCE,
            "--decision",
            "maybe",
            "--approver",
            "a",
        ],
        vec![
            "decide",
            RUN_ID,
            "--instance",
            WAIT_INSTANCE,
            "--decision",
            "approved",
            "--approver",
            "",
        ],
        vec![
            "decide",
            RUN_ID,
            "--instance",
            WAIT_INSTANCE,
            "--decision",
            "approved",
            "--approver",
            "   ",
        ],
        vec!["decide", RUN_ID, "--bogus"],
    ];
    for args in cases {
        let output = zgraph(&args);
        assert_eq!(code(&output), 2, "args={args:?} stderr={}", stderr(&output));
    }
}

#[test]
fn help_exits_zero_and_prints_usage() {
    let output = zgraph(&["decide", "--help"]);
    assert_eq!(code(&output), 0, "stderr={}", stderr(&output));
    let text = stdout(&output);
    assert!(text.contains("zgraph decide <run_id>"), "{text}");
    assert!(text.contains("--decision approved|rejected"), "{text}");
}

#[test]
fn concurrent_decides_same_instance_exactly_one_wins() {
    let fx = fixture(false);
    const PARALLEL: usize = 4;
    let children: Vec<_> = (0..PARALLEL)
        .map(|index| {
            let mut args = decide_args(fx.path());
            if index > 0 {
                // 不同 approver，证明「胜出者只有唯一一行」。
                let at = args
                    .iter()
                    .position(|arg| arg == "alice")
                    .expect("approver");
                args[at] = format!("approver-{index}");
            }
            Command::new(env!("CARGO_BIN_EXE_zgraph"))
                .args(&args)
                .spawn()
                .expect("spawn decide")
        })
        .collect();
    let codes: Vec<i32> = children
        .into_iter()
        .map(|mut child| child.wait().expect("wait").code().expect("exit code"))
        .collect();
    let successes = codes.iter().filter(|code| **code == 0).count();
    assert_eq!(successes, 1, "exactly one success, got {codes:?}");
    assert!(
        codes.iter().all(|code| matches!(code, 0 | 5)),
        "only 0/5 allowed, got {codes:?}"
    );
    assert_eq!(decision_rows(fx.path()).len(), 1, "唯一键保证恰一行");
}
