//! CLI L2 重放 + 崩溃标定 + ISSUE-076 CLI 端到端（M5 wave2，REV-030 合入后）。
//!
//! 用**真实 `zgraph` 二进制**（见 `cli_support` 的发现顺序）覆盖三块：
//!
//! 1. **L2 结果重放**（DR-002 §3.3）：`zgraph run --record=full` → `zgraph replay
//!    --mode=outputs` 退出码 0、重放输出==原始、**不产生新 run、不修改原 trace、
//!    不启动 worker 子进程**；L1 `--mode=events` 与 `zgraph trace` 输出逐字节一致；
//!    hash 模式 → `REPLAY_RECORD_MISSING` exit 3；篡改 `recorded_outputs.output_hash`
//!    → `REPLAY_RECORD_CORRUPT` exit 3；篡改 `runs.output_json` →
//!    `REPLAY_OUTPUT_MISMATCH` exit 1；非 Completed → `REPLAY_RUN_NOT_COMPLETED` exit 3。
//! 2. **崩溃标定**（06 §6.8，ISSUE-084）：CLI 起长 run（慢 fake LLM，~133s）→ 轮询
//!    到 DB `Running` → `kill -9` CLI → 同库再起一次 `zgraph run`：`--json.recovered[]`
//!    含遗留 run、stderr `marked interrupted` warning；RunRow `Failed`/`interrupted=1`/
//!    `recovered_at`/`error_code=NULL`/无 `RunFinished`；`zgraph trace` 输出
//!    `trace truncated` 且 exit 1；标定不改 `instances` 行数。
//! 3. **ISSUE-076 CLI 端到端**：agent 资产 `node_timeout_ms=1800` + DSL 不声明
//!    `timeout_ms` + 慢 fake（chunk_delay）→ `zgraph run` 约 1.8s 内 exit 1
//!    `NODE_TIMEOUT`、实例 TimedOut、`NodeTimedOut` 恰一次（证明 CLI 组合执行器
//!    如实转发 `NodeExecutor::timeout_hint`，否则会 10s 后正常 Completed）。
//!
//! **「不启动 worker」证据**（Lead 口径，不依赖 `ps`）：L2 原始 run 用临时
//! `gates/hr.py` 覆盖包把 worker pid 追加到 `ZGRAPH_L2_MARKER_LOG`；replay 用
//! `RecordedExecutor` 直接返回记录输出、不执行 python 节点，marker 行数在 replay
//! 前后不变 = 未启动 worker 的可判定证据（`cli_support::write_l2_marker_py`）。
//!
//! **「不产生新 run / 不修改 trace」证据**：`rusqlite` 只读查 `runs` 计数，并在
//! **任何数据库连接/查询之前**取 DB **主文件 + `-wal` + `-shm`** 字节快照，replay
//! 后逐一比较完全一致（replay 全程只读打开 DB；ISSUE-089：只比主文件会漏掉「只写
//! WAL 不落主文件」的情况，且快照必须早于任何读-写连接）。
//!
//! 稳定性约定：每个用例独立 `tempfile::TempDir`；kill/轮询用 20ms 轮询 + 15s 宽松
//! 上限，不用 sleep 猜时序；篡改用例直接改临时 DB（不触碰产品代码）。所有 CLI 子进程
//! 都经 `cli_support` 的公共 helper 注入 `PYTHONDONTWRITEBYTECODE=1`（ISSUE-089：
//! 运行 fixtures 不在仓库里留 ignored `__pycache__`）。

#![cfg(unix)]

#[path = "cli_support/mod.rs"]
mod cli_support;
#[path = "common/mod.rs"]
mod common;

use std::os::unix::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Output, Stdio};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use zgraph_trace::{InstanceStatus as TraceInstanceStatus, TraceReader};

use cli_support::{
    ac01_run_args, agent_graph_text, arg, assert_exit, connect, db_files_snapshot, io_graph_text,
    json_stdout, python_program, python_sdk_dir, read_lines, read_only_connect,
    recorded_outputs_count, repo_root, run_cli, run_cli_env, run_output_json, running_run,
    runs_count, slow_text_script, stderr_text, write_agent_assets, write_json, write_l2_marker_py,
    write_text, zgraph_command,
};

/// AC-01 入口输入（07 AC-01 步骤；与 `cli_ac01.rs` 一致）。
const AC01_INPUTS: &str = r#"{"input_parser":{"user_message":"我要退款"}}"#;

/// L2 marker py 覆盖包读取的 worker marker 环境变量。
const MARKER_ENV: &str = "ZGRAPH_L2_MARKER_LOG";

/// 轮询「长 run 出现 Running 行」的上限（kill -9 用例）。
const RUN_POLL_TIMEOUT: Duration = Duration::from_secs(15);

/// 长 run 的 AgentConfig 回退超时（180s；慢 fake 总时长约 133s，保证 kill 前不超时）。
const LONG_NODE_TIMEOUT_MS: u64 = 180_000;

/// 长 run 慢 fake 的 chunk 延迟与总字符数（约 133s，足以轮询到 Running 后 kill）。
const LONG_CHUNK_DELAY_MS: u64 = 200;

/// 长 run 慢 fake 的回应字符数。
const LONG_RESPONSE_CHARS: usize = 2_000;

/// CLI 层 ISSUE-076：AgentConfig.node_timeout_ms 回退值（毫秒）。
const HINT_NODE_TIMEOUT_MS: u64 = 1_800;

/// CLI 层 ISSUE-076：慢 fake chunk 延迟（100ms）。
const HINT_CHUNK_DELAY_MS: u64 = 100;

/// CLI 层 ISSUE-076：慢 fake 总字符数（300/3*100ms = 10s；hint 生效则 ~1.8s 结束）。
const HINT_RESPONSE_CHARS: usize = 300;

/// CLI 层 ISSUE-076 的宽松上限：hint 生效应远小于此；未生效需 ~10s 才 Completed。
const HINT_CASE_LIMIT: Duration = Duration::from_secs(8);

/// 一次 `--record=full` 原始 run 的产物（db/run_id/worker marker 日志）。
struct RecordedRun {
    db: PathBuf,
    run_id: String,
    marker: PathBuf,
}

/// 只读快照：DB 磁盘字节（主文件 + `-wal`/`-shm`）/ `runs` 计数 / worker marker 行。
struct DbSnapshot {
    /// `(后缀, 字节)`；文件缺失记 `None`（见 [`db_files_snapshot`]）。
    files: Vec<(String, Option<Vec<u8>>)>,
    runs: i64,
    marker: Vec<String>,
}

/// 用 AC-01 图 + 临时 marker py 覆盖包跑一次 `--record=full`。
fn record_ac01_run(dir: &Path) -> RecordedRun {
    let db = dir.join("l2.db");
    let marker = dir.join("l2_marker.log");
    let marker_text = marker.to_str().expect("marker path is UTF-8").to_string();
    let py_override = write_l2_marker_py(dir);
    let args = ac01_run_args(&db, AC01_INPUTS, "full", &[py_override]);
    let output = run_cli_env(&args, dir, &[(MARKER_ENV, marker_text.as_str())]);
    assert_exit(&output, 0, "L2 原始 run --record=full");
    let payload = json_stdout(&output);
    assert_eq!(payload["status"], json!("Completed"));
    let run_id = payload["run_id"].as_str().expect("run_id").to_string();
    assert_eq!(
        read_lines(&marker).len(),
        1,
        "原始 run 应执行 1 次 python 节点（worker pid marker）"
    );
    // ISSUE-089：marker 覆盖包在临时目录内，worker import 它时不得写 `__pycache__`；
    // 这里直接断言环境变量确实传到了 worker 子进程（公共 helper 统一注入）。
    assert!(
        !dir.join("py_override")
            .join("gates")
            .join("__pycache__")
            .exists(),
        "worker 不得写 __pycache__（PYTHONDONTWRITEBYTECODE=1 未生效）"
    );
    RecordedRun { db, run_id, marker }
}

/// replay 参数（`mode=None` → 默认 events/L1）。
fn replay_args(run_id: &str, db: &Path, mode: Option<&str>) -> Vec<String> {
    let mut args = vec![
        "replay".to_string(),
        "--run".to_string(),
        run_id.to_string(),
        "--db".to_string(),
        arg(db),
        "--json".to_string(),
    ];
    if let Some(mode) = mode {
        args.push("--mode".to_string());
        args.push(mode.to_string());
    }
    args
}

/// 捕获只读快照。
///
/// **顺序强制（ISSUE-089）**：先取文件字节（主文件 + `-wal`/`-shm`），再打开任何
/// 数据库连接。读连接在 WAL 模式下也会创建/更新 `-shm`，若先查询再取字节，快照会
/// 混入连接自身的副作用，「replay 前后不变」的证据被削弱。
fn snapshot(recorded: &RecordedRun) -> DbSnapshot {
    let files = db_files_snapshot(&recorded.db);
    DbSnapshot {
        files,
        runs: runs_count(&recorded.db),
        marker: read_lines(&recorded.marker),
    }
}

/// 断言 replay 未产生新 run / 未修改原 trace（主文件 + WAL/SHM）/ 未启动 worker。
fn assert_replay_is_read_only(recorded: &RecordedRun, before: &DbSnapshot) {
    // 与 `snapshot` 同序：先比较文件字节，再打开只读连接查计数。
    let after_files = db_files_snapshot(&recorded.db);
    assert_eq!(
        after_files, before.files,
        "replay 不得修改原 trace DB（主文件 + -wal + -shm 字节）"
    );
    assert_eq!(
        runs_count(&recorded.db),
        before.runs,
        "replay 不得产生新 run"
    );
    assert_eq!(
        read_lines(&recorded.marker),
        before.marker,
        "replay 不得启动 worker 子进程（marker 行数不变）"
    );
}

// ---------------------------------------------------------------------------
// 1. L2 结果重放
// ---------------------------------------------------------------------------

/// L2：`--record=full` 原始 run → replay outputs exit 0、输出一致、只读、不启动 worker。
#[test]
fn l2_replay_outputs_matches_without_new_run_or_worker() {
    let dir = tempfile::tempdir().expect("tempdir");
    let recorded = record_ac01_run(dir.path());
    let before = snapshot(&recorded);
    assert!(
        recorded_outputs_count(&recorded.db, &recorded.run_id) >= 4,
        "AC-01 应有 >=4 条记录输出（3 agent + 1 python）"
    );
    let original = run_output_json(&recorded.db, &recorded.run_id).expect("original output_json");

    // 额外证据（REV-030 契约）：replay 不 spawn worker，故 Python 解释器指向不存在路径
    // 仍应成功。
    let output = run_cli_env(
        &replay_args(&recorded.run_id, &recorded.db, Some("outputs")),
        dir.path(),
        &[("ZGRAPH_PYTHON", "/nonexistent/python-for-replay")],
    );
    assert_exit(&output, 0, "L2 replay outputs");
    let payload = json_stdout(&output);
    assert_eq!(payload["ok"], json!(true));
    assert_eq!(payload["command"], json!("replay"));
    assert_eq!(payload["mode"], json!("outputs"));
    assert_eq!(payload["matched"], json!(true));
    assert_eq!(
        payload["original_output"], payload["replayed_output"],
        "replay 输出应等于原始 runs.output_json"
    );
    assert_eq!(
        run_output_json(&recorded.db, &recorded.run_id),
        Some(original),
        "原 run 输出不得被 replay 修改"
    );
    assert_replay_is_read_only(&recorded, &before);
}

/// L1：`replay --mode=events` 与 `zgraph trace --json` 输出逐字节一致，且只读。
#[test]
fn l1_replay_events_matches_trace_and_is_read_only() {
    let dir = tempfile::tempdir().expect("tempdir");
    let recorded = record_ac01_run(dir.path());
    let before = snapshot(&recorded);

    let trace_out = run_cli(
        &[
            "trace".to_string(),
            recorded.run_id.clone(),
            "--db".to_string(),
            arg(&recorded.db),
            "--json".to_string(),
        ],
        dir.path(),
    );
    let replay_out = run_cli(
        &replay_args(&recorded.run_id, &recorded.db, Some("events")),
        dir.path(),
    );
    assert_exit(&trace_out, 0, "trace 回放");
    assert_exit(&replay_out, 0, "L1 replay events");
    assert_eq!(
        replay_out.stdout, trace_out.stdout,
        "L1 replay 应与 zgraph trace 的 NDJSON 逐字节一致"
    );
    let stdout = String::from_utf8_lossy(&replay_out.stdout);
    assert!(stdout.contains("RunFinished"), "L1 回放应含 RunFinished");
    assert_replay_is_read_only(&recorded, &before);
}

/// `--record=hash`（无 full 记录）→ replay outputs exit 3 `REPLAY_RECORD_MISSING`。
#[test]
fn l2_replay_without_full_record_exits_3() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db = dir.path().join("hash_only.db");
    let args = ac01_run_args(&db, AC01_INPUTS, "hash", &[]);
    let run = run_cli(&args, dir.path());
    assert_exit(&run, 0, "hash 模式原始 run");
    let run_id = json_stdout(&run)["run_id"]
        .as_str()
        .expect("run_id")
        .to_string();

    let output = run_cli(&replay_args(&run_id, &db, Some("outputs")), dir.path());
    assert_exit(&output, 3, "无 full 记录 replay");
    assert!(
        stderr_text(&output).contains("REPLAY_RECORD_MISSING"),
        "stderr: {}",
        stderr_text(&output)
    );
    assert_eq!(runs_count(&db), 1, "replay 不得产生新 run");
}

/// 删除 full run 的一条 `recorded_outputs`（不完整）→ exit 3 `REPLAY_RECORD_MISSING`。
#[test]
fn l2_replay_missing_recorded_output_exits_3() {
    let dir = tempfile::tempdir().expect("tempdir");
    let recorded = record_ac01_run(dir.path());
    let conn = connect(&recorded.db);
    let deleted = conn
        .execute(
            "DELETE FROM recorded_outputs WHERE run_id = ?1 AND node_id = 'human_escalation_check'",
            [recorded.run_id.as_str()],
        )
        .expect("delete recorded output");
    assert_eq!(deleted, 1, "应删除 1 条记录");
    drop(conn);

    let output = run_cli(
        &replay_args(&recorded.run_id, &recorded.db, Some("outputs")),
        dir.path(),
    );
    assert_exit(&output, 3, "记录不完整 replay");
    assert!(
        stderr_text(&output).contains("REPLAY_RECORD_MISSING"),
        "stderr: {}",
        stderr_text(&output)
    );
}

/// 篡改 `recorded_outputs.output_hash` → replay outputs exit 3 `REPLAY_RECORD_CORRUPT`。
#[test]
fn l2_replay_corrupt_recorded_hash_exits_3() {
    let dir = tempfile::tempdir().expect("tempdir");
    let recorded = record_ac01_run(dir.path());
    let before_runs = runs_count(&recorded.db);
    let tampered_hash = "0".repeat(64);
    let conn = connect(&recorded.db);
    let changed = conn
        .execute(
            "UPDATE recorded_outputs SET output_hash = ?1 \
             WHERE run_id = ?2 AND node_id = 'human_escalation_check'",
            rusqlite::params![tampered_hash, recorded.run_id],
        )
        .expect("tamper recorded_outputs hash");
    assert_eq!(changed, 1, "应篡改 1 条记录");
    drop(conn);

    let output = run_cli(
        &replay_args(&recorded.run_id, &recorded.db, Some("outputs")),
        dir.path(),
    );
    assert_exit(&output, 3, "篡改 hash 后 replay");
    assert!(
        stderr_text(&output).contains("REPLAY_RECORD_CORRUPT"),
        "stderr: {}",
        stderr_text(&output)
    );
    assert_eq!(
        runs_count(&recorded.db),
        before_runs,
        "replay 不得产生新 run"
    );
}

/// 篡改 `runs.output_json`（记录本身合法）→ replay outputs exit 1 `REPLAY_OUTPUT_MISMATCH`。
#[test]
fn l2_replay_output_mismatch_exits_1() {
    let dir = tempfile::tempdir().expect("tempdir");
    let recorded = record_ac01_run(dir.path());
    let tampered = r#"{"result":"tampered"}"#.to_string();
    let conn = connect(&recorded.db);
    conn.execute(
        "UPDATE runs SET output_json = ?1 WHERE run_id = ?2",
        rusqlite::params![tampered, recorded.run_id],
    )
    .expect("tamper runs.output_json");
    drop(conn);

    let output = run_cli(
        &replay_args(&recorded.run_id, &recorded.db, Some("outputs")),
        dir.path(),
    );
    assert_exit(&output, 1, "输出不一致 replay");
    let payload = json_stdout(&output);
    assert_eq!(payload["ok"], json!(false));
    assert_eq!(payload["matched"], json!(false));
    assert_eq!(payload["error"]["code"], json!("REPLAY_OUTPUT_MISMATCH"));
    // `--json` 模式下错误码在 stdout payload（stderr 只承载人类可读模式的行）；
    // 退出码 1 + payload.error.code + matched=false 是本用例的权威断言。
    assert_eq!(
        payload["original_output"],
        json!({"result": "tampered"}),
        "payload.original_output 应反映被篡改的 runs.output_json"
    );
}

/// 非 Completed run → replay outputs exit 3 `REPLAY_RUN_NOT_COMPLETED`。
#[test]
fn l2_replay_non_completed_run_exits_3() {
    let dir = tempfile::tempdir().expect("tempdir");
    let recorded = record_ac01_run(dir.path());
    let conn = connect(&recorded.db);
    conn.execute(
        "UPDATE runs SET status = 'Failed' WHERE run_id = ?1",
        [recorded.run_id.as_str()],
    )
    .expect("tamper runs.status");
    drop(conn);

    let output = run_cli(
        &replay_args(&recorded.run_id, &recorded.db, Some("outputs")),
        dir.path(),
    );
    assert_exit(&output, 3, "非 Completed run replay");
    assert!(
        stderr_text(&output).contains("REPLAY_RUN_NOT_COMPLETED"),
        "stderr: {}",
        stderr_text(&output)
    );
}

// ---------------------------------------------------------------------------
// 2. 崩溃标定（06 §6.8 / ISSUE-084）
// ---------------------------------------------------------------------------

/// 轮询 DB 直到出现 `Running`/`Paused` run（返回 run_id 与 owner_pid）。
fn wait_running_run(db: &Path) -> (String, i64) {
    let deadline = Instant::now() + RUN_POLL_TIMEOUT;
    loop {
        if let Some(found) = running_run(db) {
            return found;
        }
        assert!(
            Instant::now() < deadline,
            "长 run 未在 {RUN_POLL_TIMEOUT:?} 内出现 Running 行（db={}）",
            db.display()
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// 某 run 的 `instances` 行数（标定不得修改 instances，06 §6.8.2）。
fn instance_count(db: &Path, run_id: &str) -> i64 {
    read_only_connect(db)
        .query_row(
            "SELECT COUNT(*) FROM instances WHERE run_id = ?1",
            [run_id],
            |row| row.get(0),
        )
        .expect("count instances")
}

/// 断言遗留 run 已被标定（只改 runs；无 RunFinished；error_code 保持 NULL）。
fn assert_interrupted_row(db: &Path, run_id: &str) {
    let reader = TraceReader::open_readonly(db).expect("open trace");
    let row = reader
        .run(run_id)
        .expect("query interrupted run")
        .expect("interrupted run row");
    assert_eq!(row.status, zgraph_trace::RunStatus::Failed);
    assert!(row.interrupted, "interrupted=1");
    assert!(row.recovered_at.is_some(), "recovered_at 已写入");
    assert_eq!(row.finished_at, None, "标定不写 finished_at");
    assert_eq!(row.error_code, None, "标定 error_code 保持 NULL");
    assert_eq!(
        row.error_message.as_deref(),
        Some("run interrupted: owner process exited")
    );
    let events = common::events(&reader, run_id);
    assert_eq!(common::count_kind(&events, "RunStarted"), 1);
    assert_eq!(
        common::count_kind(&events, "RunFinished"),
        0,
        "标定不得合成 RunFinished"
    );
}

/// `zgraph trace` 对 interrupted run：exit 1 + stderr `trace truncated`，NDJSON 无 RunFinished。
fn assert_trace_truncated(db: &Path, run_id: &str) {
    let output = run_cli(
        &[
            "trace".to_string(),
            run_id.to_string(),
            "--db".to_string(),
            arg(db),
            "--json".to_string(),
        ],
        &repo_root(),
    );
    assert_exit(&output, 1, "interrupted run 的 trace 回放");
    assert!(
        stderr_text(&output).contains("trace truncated"),
        "stderr: {}",
        stderr_text(&output)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("RunStarted"), "截断回放应保留已写事件");
    assert!(!stdout.contains("RunFinished"));
}

/// 起长 run（慢 fake 总时长约 133s）：graph/assets/fake 脚本落在 `dir`，返回子进程句柄。
///
/// 用 [`zgraph_command`]（而非裸 `Command`）以继承 e2e 统一环境
/// （`PYTHONDONTWRITEBYTECODE=1`，ISSUE-089）。
fn spawn_long_run(dir: &Path, db: &Path) -> Child {
    let graph = write_text(dir, "long.json", &agent_graph_text("long_agent", None));
    let assets = write_agent_assets(dir, "long_agent", LONG_NODE_TIMEOUT_MS);
    let script = write_json(
        dir,
        "long_fake.json",
        &slow_text_script(LONG_CHUNK_DELAY_MS, LONG_RESPONSE_CHARS),
    );
    zgraph_command()
        .args([
            "run".to_string(),
            arg(&graph),
            "--inputs".to_string(),
            r#"{"start":{"message":"hi"}}"#.to_string(),
            "--db".to_string(),
            arg(db),
            "--assets-dir".to_string(),
            arg(&assets),
            "--tools-dir".to_string(),
            arg(&assets.join("tools")),
            "--python".to_string(),
            python_program(),
            "--python-path".to_string(),
            arg(&python_sdk_dir()),
            format!("--fake-llm={}", script.display()),
            "--json".to_string(),
        ])
        .current_dir(dir)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn long zgraph run")
}

/// 同库再起一次 run（启动时标定遗留 run），返回捕获输出。
fn second_run_after_kill(dir: &Path, db: &Path) -> Output {
    let graph = write_text(dir, "io.json", &io_graph_text());
    run_cli(
        &[
            "run".to_string(),
            arg(&graph),
            "--inputs".to_string(),
            r#"{"start":{"x":1}}"#.to_string(),
            "--db".to_string(),
            arg(db),
            "--json".to_string(),
        ],
        dir,
    )
}

/// 断言第二次 run 的 `recovered[]` 与 stderr 标定 warning。
fn assert_recovered_after_kill(output: &Output, leftover_run_id: &str) {
    assert_exit(output, 0, "标定后的第二次 run");
    let payload = json_stdout(output);
    assert_eq!(payload["status"], json!("Completed"));
    let recovered = payload["recovered"].as_array().expect("recovered[]");
    assert_eq!(recovered.len(), 1, "应标定 1 个遗留 run：{payload}");
    assert_eq!(recovered[0]["run_id"], json!(leftover_run_id));
    assert_eq!(recovered[0]["previous_status"], json!("Running"));
    assert!(
        recovered[0]["recovered_at"]
            .as_str()
            .is_some_and(|text| !text.is_empty()),
        "recovered_at 非空"
    );
    let stderr = stderr_text(output);
    assert!(
        stderr.contains(leftover_run_id) && stderr.contains("marked interrupted"),
        "stderr 应有标定 warning：{stderr}"
    );
}

/// kill -9 长 run → 同库再 run：recovered[] / RunRow 标定 / trace truncated 全链路。
#[test]
fn interrupted_run_is_marked_on_next_start_and_trace_is_truncated() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db = dir.path().join("crash.db");
    let mut child = spawn_long_run(dir.path(), &db);

    let (run_id, owner_pid) = wait_running_run(&db);
    assert_eq!(
        owner_pid,
        i64::from(child.id()),
        "runs.owner_pid 应等于 CLI 进程 pid"
    );
    child.kill().expect("SIGKILL 长 run");
    let status = child.wait().expect("wait killed cli");
    assert_eq!(status.signal(), Some(9), "kill -9 语义");
    let instances_before = instance_count(&db, &run_id);

    let second = second_run_after_kill(dir.path(), &db);
    assert_recovered_after_kill(&second, &run_id);
    assert_eq!(runs_count(&db), 2, "应恰有 2 个 run（遗留 + 新增）");
    assert_interrupted_row(&db, &run_id);
    assert_eq!(
        instance_count(&db, &run_id),
        instances_before,
        "标定不得修改 instances 行数"
    );
    assert_trace_truncated(&db, &run_id);
}

// ---------------------------------------------------------------------------
// 3. ISSUE-076：CLI 层 timeout_hint 端到端
// ---------------------------------------------------------------------------

/// 构造慢 fake + agent 资产 + 图，执行一次 `zgraph run`，返回输出与耗时。
fn run_hint_case(dir: &Path) -> (Output, Duration) {
    let db = dir.join("hint.db");
    let assets = write_agent_assets(dir, "slow_agent", HINT_NODE_TIMEOUT_MS);
    let script = write_json(
        dir,
        "hint_fake.json",
        &slow_text_script(HINT_CHUNK_DELAY_MS, HINT_RESPONSE_CHARS),
    );
    let graph = write_text(
        dir,
        "hint_graph.json",
        &agent_graph_text("slow_agent", None),
    );
    let started = Instant::now();
    let output = run_cli(
        &[
            "run".to_string(),
            arg(&graph),
            "--inputs".to_string(),
            r#"{"start":{"message":"hi"}}"#.to_string(),
            "--db".to_string(),
            arg(&db),
            "--assets-dir".to_string(),
            arg(&assets),
            "--tools-dir".to_string(),
            arg(&assets.join("tools")),
            "--python".to_string(),
            python_program(),
            "--python-path".to_string(),
            arg(&python_sdk_dir()),
            format!("--fake-llm={}", script.display()),
            "--json".to_string(),
        ],
        dir,
    );
    (output, started.elapsed())
}

/// ISSUE-076 断言：exit 1 `NODE_TIMEOUT` 且耗时与 hint 同量级（证明回退链生效）。
fn assert_hint_timeout(output: &Output, elapsed: Duration) -> Value {
    assert_exit(output, 1, "CLI 层 ISSUE-076 超时回退");
    let payload = json_stdout(output);
    assert_eq!(payload["status"], json!("Failed"));
    assert_eq!(payload["error"]["code"], json!("NODE_TIMEOUT"));
    assert_eq!(payload["run_output"], Value::Null);
    assert!(
        elapsed >= Duration::from_millis(HINT_NODE_TIMEOUT_MS - 300),
        "超时不应早于 hint：elapsed={elapsed:?}"
    );
    assert!(
        elapsed < HINT_CASE_LIMIT,
        "hint 未生效（慢 fake 走完需约 10s）：elapsed={elapsed:?}"
    );
    payload
}

/// ISSUE-076 trace 断言：实例 TimedOut / `NodeTimedOut` 恰一次 / run NODE_TIMEOUT。
///
/// 同时覆盖 ISSUE-086（06 §6.2.5）：CLI 端到端走 Harness 自报超时路径时，
/// `instances.error_code` 必须为 `NODE_TIMEOUT`、`instances.error` 必须非 NULL
/// （kernel `fail_timeout` 已与 watchdog 同口径回填，e2e 做 CLI 层回归）。
fn assert_hint_trace(db: &Path, run_id: &str) {
    let reader = TraceReader::open_readonly(db).expect("open trace");
    let row = common::run_row(&reader, run_id);
    assert_eq!(row.status, zgraph_trace::RunStatus::Failed);
    assert_eq!(row.error_code.as_deref(), Some("NODE_TIMEOUT"));
    let instance = common::instance(&reader, run_id, "worker");
    assert_eq!(instance.status, TraceInstanceStatus::TimedOut);
    assert_eq!(
        instance.error_code.as_deref(),
        Some("NODE_TIMEOUT"),
        "ISSUE-086：CLI 自报超时路径的 instances.error_code"
    );
    let error = instance
        .error
        .as_ref()
        .expect("ISSUE-086：instances.error 不得为 NULL");
    assert_eq!(error["code"], json!("NODE_TIMEOUT"));
    assert_eq!(error["kind"], json!("Timeout"));
    let events = common::events(&reader, run_id);
    assert_eq!(
        common::count_kind(&events, "NodeTimedOut"),
        1,
        "NodeTimedOut 恰一次"
    );
}

/// CLI 层 ISSUE-076：DSL 未声明 timeout_ms → AgentConfig.node_timeout_ms 经 CLI 组合
/// 执行器转发给内核；实例 TimedOut / run NODE_TIMEOUT 且耗时与 hint 同量级。
#[test]
fn cli_agent_config_timeout_hint_is_end_to_end() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (output, elapsed) = run_hint_case(dir.path());
    let payload = assert_hint_timeout(&output, elapsed);
    let run_id = payload["run_id"].as_str().expect("run_id");
    assert_hint_trace(&dir.path().join("hint.db"), run_id);
}
