//! `zgraph replay` CLI 契约测试（DR-002 §3.3；M5 wave2）。
//!
//! 覆盖：
//! - `--mode=events`（L1）：输出与 `zgraph trace` 完全一致（复用同一实现）；
//! - `--mode=outputs`（L2）：`--record=full` 记录 → 重放成功且输出与原始一致；
//!   无 full 记录 / 非 Completed / hash 篡改 → exit 3；输出篡改 → exit 1；
//!   不写新 run、不改原 trace、不启动 worker（以 `ZGRAPH_PYTHON` 指向不存在解释器仍成功证明）。
//!
//! 稳定性约定（AGENTS §8）：每个用例独立 `tempfile::TempDir` 与独立 DB；无网络；时序
//! 断言只用退出码/DB 状态，不做 sleep 猜测。

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use rusqlite::OptionalExtension;
use serde_json::{json, Value};

// ---------------------------------------------------------------------------
// 路径与命令辅助
// ---------------------------------------------------------------------------

/// 仓库根（本包位于 `<root>/crates/zgraph-cli`）。
fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("repo root resolves")
}

/// 仓库 Python SDK 路径。
fn python_path() -> PathBuf {
    repo_root().join("python")
}

/// Python 解释器：`ZGRAPH_PYTHON` 优先，缺省 `python3`。
fn python_program() -> String {
    std::env::var("ZGRAPH_PYTHON").unwrap_or_else(|_| "python3".to_string())
}

/// AC-08 fixture 目录（graph/agents/tools/fake_llm）。
fn ac08_dir() -> PathBuf {
    repo_root().join("tests/fixtures/graphs/harness_ac08")
}

/// 运行 CLI 二进制并捕获输出。
fn run_cli(args: &[String], cwd: &Path) -> Output {
    run_cli_env(args, cwd, &[])
}

/// 带额外环境变量运行 CLI。
fn run_cli_env(args: &[String], cwd: &Path, envs: &[(&str, &str)]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_zgraph"))
        .args(args)
        .envs(envs.iter().copied())
        .current_dir(cwd)
        .output()
        .expect("spawn zgraph binary")
}

/// 写文件（图/Python 模块）。
fn write_text(dir: &Path, name: &str, text: &str) -> PathBuf {
    let path = dir.join(name);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("create parent dir");
    }
    std::fs::write(&path, text).expect("write file");
    path
}

/// 路径 → CLI 参数。
fn arg(path: &Path) -> String {
    path.display().to_string()
}

/// 断言退出码并输出 stdout/stderr。
fn assert_exit(output: &Output, expected: i32, context: &str) {
    assert_eq!(
        output.status.code(),
        Some(expected),
        "{context}: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

/// stdout JSON。
fn json_stdout(output: &Output) -> Value {
    let text = String::from_utf8_lossy(&output.stdout);
    serde_json::from_str(text.trim()).unwrap_or_else(|error| {
        panic!(
            "stdout is not JSON ({error}): {text}\nstderr: {}",
            String::from_utf8_lossy(&output.stderr)
        )
    })
}

/// stderr 文本。
fn stderr_text(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

// ---------------------------------------------------------------------------
// trace DB 辅助（只读断言；篡改用独立连接）
// ---------------------------------------------------------------------------

/// 打开 trace DB。
fn connect(db: &Path) -> rusqlite::Connection {
    rusqlite::Connection::open(db).expect("open trace db")
}

/// 计数某表行数。
fn count(db: &Path, table: &str) -> i64 {
    connect(db)
        .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
            row.get(0)
        })
        .expect("count rows")
}

/// 某 run 的行值（`runs.output_json` 原文）。
fn run_output(db: &Path, run_id: &str) -> Option<Value> {
    let text: Option<String> = connect(db)
        .query_row(
            "SELECT output_json FROM runs WHERE run_id = ?1",
            [run_id],
            |row| row.get(0),
        )
        .optional()
        .expect("run output")
        .flatten();
    text.map(|text| serde_json::from_str(&text).expect("run output json"))
}

/// 某 run 的 `recorded_outputs` 行数。
fn recorded_count(db: &Path) -> i64 {
    count(db, "recorded_outputs")
}

/// 篡改 DB（测试用；模拟记录损坏/输出被改）。
fn tamper(db: &Path, sql: &str) {
    connect(db)
        .execute(sql, [])
        .unwrap_or_else(|error| panic!("tamper sql failed ({error}): {sql}"));
}

// ---------------------------------------------------------------------------
// 造 run 数据（真实链路：fake LLM + 真 Python worker + 真 SQLite）
// ---------------------------------------------------------------------------

/// AC-08 图基础参数。
fn ac08_args(db: &Path) -> Vec<String> {
    vec![
        "run".to_string(),
        arg(&ac08_dir().join("graph.json")),
        "--db".to_string(),
        arg(db),
        "--assets-dir".to_string(),
        arg(&ac08_dir()),
        "--inputs".to_string(),
        r#"{"start":{"message":"2+3"}}"#.to_string(),
        "--python".to_string(),
        python_program(),
        "--python-path".to_string(),
        arg(&python_path()),
        "--fake-llm".to_string(),
        format!("--fake-llm={}", ac08_dir().join("fake_llm.json").display()),
    ]
}

/// 跑一次 `--record=full` 的 AC-08 run，返回 `(run_id, run_output, db)`。
fn create_full_run(dir: &Path) -> (String, Value, PathBuf) {
    let db = dir.join("full.db");
    let mut args = ac08_args(&db);
    args.push("--record=full".to_string());
    args.push("--json".to_string());
    let output = run_cli(&args, dir);
    assert_exit(&output, 0, "create full-record run");
    let payload = json_stdout(&output);
    let run_id = payload["run_id"].as_str().expect("run_id").to_string();
    (run_id, payload["run_output"].clone(), db)
}

/// 崩溃图（python 节点抛异常）跑一次 full 记录，返回 `(run_id, db)`。
fn create_failed_run(dir: &Path) -> (String, PathBuf) {
    write_text(
        dir,
        "boom.py",
        "def run(inputs, ctx):\n    raise RuntimeError(\"boom\")\n",
    );
    let graph = json!({
        "version": "1.0",
        "metadata": {"name": "boom"},
        "entry_points": ["start"],
        "nodes": [
            {"id": "start", "type": "start",
             "inputs": [{"name": "x", "type": "any", "required": true}], "config": {}},
            {"id": "work", "type": "python_node",
             "inputs": [{"name": "x", "type": "any", "required": true}],
             "config": {"module": "boom", "function": "run"}},
            {"id": "end", "type": "end",
             "inputs": [{"name": "r", "type": "any", "required": true}], "config": {}}
        ],
        "edges": [
            {"from": "start", "to_node": "work", "to_input_port": "x"},
            {"from": "work", "to_node": "end", "to_input_port": "r"}
        ]
    })
    .to_string();
    let graph_path = write_text(dir, "boom.json", &graph);
    let db = dir.join("failed.db");
    let output = run_cli(
        &[
            "run".to_string(),
            arg(&graph_path),
            "--db".to_string(),
            arg(&db),
            "--inputs".to_string(),
            r#"{"start":{"x":1}}"#.to_string(),
            "--python".to_string(),
            python_program(),
            "--python-path".to_string(),
            arg(&python_path()),
            "--python-path".to_string(),
            arg(dir),
            "--fake-llm".to_string(),
            "--record=full".to_string(),
            "--json".to_string(),
        ],
        dir,
    );
    assert_exit(&output, 1, "create failed run");
    let payload = json_stdout(&output);
    (payload["run_id"].as_str().expect("run_id").to_string(), db)
}

/// 一次 replay 调用参数（默认 outputs 模式 + --json）。
fn replay_args(run_id: &str, db: &Path, extra: &[&str]) -> Vec<String> {
    let mut args = vec![
        "replay".to_string(),
        "--run".to_string(),
        run_id.to_string(),
        "--db".to_string(),
        arg(db),
    ];
    args.extend(extra.iter().map(|value| value.to_string()));
    args
}

// ---------------------------------------------------------------------------
// 1. events（L1）：与 trace 输出完全一致
// ---------------------------------------------------------------------------

#[test]
fn replay_events_matches_trace_output_exactly() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (run_id, _, db) = create_full_run(dir.path());

    let trace = run_cli(
        &[
            "trace".to_string(),
            run_id.clone(),
            "--db".to_string(),
            arg(&db),
        ],
        dir.path(),
    );
    assert_exit(&trace, 0, "trace human");
    let replay = run_cli(&replay_args(&run_id, &db, &["--mode=events"]), dir.path());
    assert_exit(&replay, 0, "replay events human");
    assert_eq!(
        String::from_utf8_lossy(&replay.stdout),
        String::from_utf8_lossy(&trace.stdout),
        "L1 replay must reuse the trace timeline formatting"
    );

    let trace_json = run_cli(
        &[
            "trace".to_string(),
            run_id.clone(),
            "--db".to_string(),
            arg(&db),
            "--json".to_string(),
        ],
        dir.path(),
    );
    assert_exit(&trace_json, 0, "trace json");
    let replay_json = run_cli(
        &replay_args(&run_id, &db, &["--mode=events", "--json"]),
        dir.path(),
    );
    assert_exit(&replay_json, 0, "replay events json");
    assert_eq!(
        String::from_utf8_lossy(&replay_json.stdout),
        String::from_utf8_lossy(&trace_json.stdout),
        "L1 replay NDJSON must match trace --json"
    );
}

// ---------------------------------------------------------------------------
// 2. outputs（L2）：full 记录成功重放，不写库、不启动 worker
// ---------------------------------------------------------------------------

#[test]
fn replay_outputs_full_record_succeeds_without_workers() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (run_id, original_output, db) = create_full_run(dir.path());
    assert_eq!(recorded_count(&db), 1, "only the agent node is recorded");
    let runs_before = count(&db, "runs");
    let events_before = count(&db, "events");

    // `ZGRAPH_PYTHON` 指向不存在的解释器：若重放启动 worker 必失败；成功即证明未 spawn。
    let replay = run_cli_env(
        &replay_args(&run_id, &db, &["--json", "--mode=outputs"]),
        dir.path(),
        &[("ZGRAPH_PYTHON", "/nonexistent/python")],
    );
    assert_exit(&replay, 0, "L2 replay outputs");
    let payload = json_stdout(&replay);
    assert_eq!(payload["ok"], json!(true));
    assert_eq!(payload["command"], json!("replay"));
    assert_eq!(payload["mode"], json!("outputs"));
    assert_eq!(payload["run_id"], json!(run_id));
    assert_eq!(payload["status"], json!("Completed"));
    assert_eq!(payload["matched"], json!(true));
    assert_eq!(payload["recorded_outputs"], json!(1));
    assert_eq!(payload["replayed_output"], original_output);
    assert_eq!(payload["original_output"], original_output);
    assert!(payload.get("error").is_none());

    // 不写新 run / 不改原 trace。
    assert_eq!(
        count(&db, "runs"),
        runs_before,
        "replay must not create runs"
    );
    assert_eq!(
        count(&db, "events"),
        events_before,
        "replay must not write the original trace"
    );
    assert_eq!(
        run_output(&db, &run_id),
        Some(original_output),
        "replay must not modify runs.output_json"
    );
}

// ---------------------------------------------------------------------------
// 2b. ISSUE-087：global_vars 记录与重放（schema v3）
// ---------------------------------------------------------------------------

/// 读 `runs.global_vars_json` 原始文本。
fn global_vars_text(db: &Path, run_id: &str) -> Option<String> {
    connect(db)
        .query_row(
            "SELECT global_vars_json FROM runs WHERE run_id = ?1",
            [run_id],
            |row| row.get(0),
        )
        .optional()
        .expect("query global_vars_json")
}

#[test]
fn run_records_empty_global_vars_snapshot() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (run_id, _, db) = create_full_run(dir.path());
    // CLI 暂无 `--global-vars` 入口 → 记录空 object（canonical `{}`）。
    assert_eq!(
        global_vars_text(&db, &run_id).as_deref(),
        Some("{}"),
        "run must always record a global_vars snapshot"
    );
}

#[test]
fn replay_outputs_reads_recorded_global_vars() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (run_id, _, db) = create_full_run(dir.path());
    // 模拟一个带 global_vars 的库（库 API 驱动的 run；CLI 当前无该入口）。
    tamper(
        &db,
        "UPDATE runs SET global_vars_json = '{\"threshold\":5}'",
    );

    let replay = run_cli(
        &replay_args(&run_id, &db, &["--json", "--mode=outputs"]),
        dir.path(),
    );
    assert_exit(&replay, 0, "L2 replay with recorded global_vars");
    let payload = json_stdout(&replay);
    assert_eq!(payload["matched"], json!(true));
    assert_eq!(payload["ok"], json!(true));
}

#[test]
fn replay_outputs_legacy_run_without_global_vars_warns_and_falls_back() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (run_id, _, db) = create_full_run(dir.path());
    // v2→v3 迁移前的历史 run：列为 NULL → 空 map 兼容 + stderr 告警。
    tamper(&db, "UPDATE runs SET global_vars_json = NULL");

    let replay = run_cli(
        &replay_args(&run_id, &db, &["--json", "--mode=outputs"]),
        dir.path(),
    );
    assert_exit(&replay, 0, "legacy run stays replayable");
    assert_eq!(json_stdout(&replay)["matched"], json!(true));
    let stderr = String::from_utf8_lossy(&replay.stderr);
    assert!(
        stderr.contains("has no recorded global_vars"),
        "legacy replay must warn: {stderr}"
    );
}

#[test]
fn replay_outputs_rejects_non_object_global_vars_snapshot() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (run_id, _, db) = create_full_run(dir.path());
    tamper(&db, "UPDATE runs SET global_vars_json = '42'");

    let replay = run_cli(&replay_args(&run_id, &db, &["--mode=outputs"]), dir.path());
    assert_exit(&replay, 3, "corrupt global_vars snapshot");
    let stderr = String::from_utf8_lossy(&replay.stderr);
    assert!(
        stderr.contains("error[REPLAY_RECORD_CORRUPT]"),
        "unexpected stderr: {stderr}"
    );
}

#[test]
fn replay_outputs_requires_current_schema() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (run_id, _, db) = create_full_run(dir.path());
    // replay 只读、不迁移（06 §6.6/§6.7）：v2 库必须先由写端迁移。
    tamper(&db, "PRAGMA user_version = 2");

    let replay = run_cli(&replay_args(&run_id, &db, &["--mode=outputs"]), dir.path());
    assert_exit(&replay, 4, "v2 database needs writer migration first");
    let stderr = String::from_utf8_lossy(&replay.stderr);
    assert!(
        stderr.contains("error[STORAGE_ERROR]") && stderr.contains("unsupported schema version=2"),
        "unexpected stderr: {stderr}"
    );
}

// ---------------------------------------------------------------------------
// 3. 记录类错误：exit 3
// ---------------------------------------------------------------------------

#[test]
fn replay_outputs_hash_mode_exits_3_with_record_missing() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db = dir.path().join("hash.db");
    let mut args = ac08_args(&db);
    args.push("--json".to_string());
    let output = run_cli(&args, dir.path());
    assert_exit(&output, 0, "create hash run");
    let run_id = json_stdout(&output)["run_id"]
        .as_str()
        .expect("run_id")
        .to_string();

    let replay = run_cli(&replay_args(&run_id, &db, &["--mode=outputs"]), dir.path());
    assert_exit(&replay, 3, "replay without full record");
    let stderr = stderr_text(&replay);
    assert!(stderr.contains("error[REPLAY_RECORD_MISSING]"), "{stderr}");
}

#[test]
fn replay_outputs_non_completed_run_exits_3() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (run_id, db) = create_failed_run(dir.path());
    let replay = run_cli(&replay_args(&run_id, &db, &["--mode=outputs"]), dir.path());
    assert_exit(&replay, 3, "replay non-completed run");
    let stderr = stderr_text(&replay);
    assert!(
        stderr.contains("error[REPLAY_RUN_NOT_COMPLETED]"),
        "{stderr}"
    );
}

#[test]
fn replay_outputs_corrupt_hash_exits_3() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (run_id, _, db) = create_full_run(dir.path());
    tamper(&db, "UPDATE recorded_outputs SET output_hash = 'deadbeef'");
    let replay = run_cli(&replay_args(&run_id, &db, &["--mode=outputs"]), dir.path());
    assert_exit(&replay, 3, "replay corrupt hash");
    let stderr = stderr_text(&replay);
    assert!(stderr.contains("error[REPLAY_RECORD_CORRUPT]"), "{stderr}");
    assert!(stderr.contains("hash mismatch"), "{stderr}");
}

// ---------------------------------------------------------------------------
// 4. 输出不一致：exit 1 + REPLAY_OUTPUT_MISMATCH
// ---------------------------------------------------------------------------

#[test]
fn replay_outputs_output_mismatch_exits_1() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (run_id, _, db) = create_full_run(dir.path());
    tamper(&db, r#"UPDATE runs SET output_json = '{"tampered":true}'"#);
    let replay = run_cli(
        &replay_args(&run_id, &db, &["--mode=outputs", "--json"]),
        dir.path(),
    );
    assert_exit(&replay, 1, "replay output mismatch");
    let payload = json_stdout(&replay);
    assert_eq!(payload["ok"], json!(false));
    assert_eq!(payload["matched"], json!(false));
    assert_eq!(payload["status"], json!("Completed"));
    assert_eq!(
        payload["error"]["code"],
        json!("REPLAY_OUTPUT_MISMATCH"),
        "{payload}"
    );
    assert_eq!(payload["original_output"], json!({"tampered": true}));
    assert!(payload["replayed_output"].is_object());

    // human 模式同样 exit 1 且 stderr 带差异摘要。
    let human = run_cli(&replay_args(&run_id, &db, &["--mode=outputs"]), dir.path());
    assert_exit(&human, 1, "replay output mismatch (human)");
    assert!(
        stderr_text(&human).contains("error[REPLAY_OUTPUT_MISMATCH]"),
        "{}",
        stderr_text(&human)
    );
}

// ---------------------------------------------------------------------------
// 5. 用法 / 存储错误
// ---------------------------------------------------------------------------

#[test]
fn replay_usage_errors_exit_2() {
    let dir = tempfile::tempdir().expect("tempdir");
    let output = run_cli(&["replay".to_string()], dir.path());
    assert_exit(&output, 2, "replay without --run");
    assert!(stderr_text(&output).contains("requires --run"));
    let output = run_cli(
        &[
            "replay".to_string(),
            "--run".to_string(),
            "r1".to_string(),
            "--mode".to_string(),
            "all".to_string(),
        ],
        dir.path(),
    );
    assert_exit(&output, 2, "invalid mode");
    assert!(stderr_text(&output).contains("invalid --mode"));
}

#[test]
fn replay_missing_run_or_db_exits_4() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (_, _, db) = create_full_run(dir.path());
    let missing = run_cli(
        &replay_args("00000000-0000-0000-0000-000000000000", &db, &[]),
        dir.path(),
    );
    assert_exit(&missing, 4, "replay missing run");
    assert!(stderr_text(&missing).contains("error[STORAGE_ERROR]"));

    let no_db = run_cli(
        &replay_args("any", &dir.path().join("nope.db"), &[]),
        dir.path(),
    );
    assert_exit(&no_db, 4, "replay missing db");
    assert!(stderr_text(&no_db).contains("error[STORAGE_ERROR]"));
}
