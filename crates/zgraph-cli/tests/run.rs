//! `zgraph run` CLI 契约测试（06 §6.10.3 / §6.10.5 / §6.10.6；07 AC-01/AC-08 链路）。
//!
//! 测试通过**真实二进制**（`CARGO_BIN_EXE_zgraph`）执行：参数/退出码/输出形状与
//! trace 落库都按 CLI 契约断言，不绕过 `dispatch`/输出层。
//!
//! 稳定性约定（AGENTS §8）：每个用例独立 `tempfile::TempDir` 与独立 DB；时序断言只用
//! 轮询/宽松上限；真实 worker 用例通过 `--python-path <repo>/python` 注入 SDK 路径，
//! 解释器优先取 `ZGRAPH_PYTHON`（缺省 `python3`），全程无网络。

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, Instant};

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

/// 仓库 Python SDK 路径（`python/zgraph_worker`）。
fn python_path() -> PathBuf {
    repo_root().join("python")
}

/// Python 解释器：`ZGRAPH_PYTHON` 优先，缺省 `python3`（与 e2e 口径一致）。
fn python_program() -> String {
    std::env::var("ZGRAPH_PYTHON").unwrap_or_else(|_| "python3".to_string())
}

/// `tests/fixtures/graphs/harness_ac08`（AC-08 资产：graph/agents/tools/fake_llm）。
fn ac08_dir() -> PathBuf {
    repo_root().join("tests/fixtures/graphs/harness_ac08")
}

/// 运行 CLI 二进制并捕获 stdout/stderr。
fn run_cli(args: &[String], cwd: &Path) -> Output {
    run_cli_env(args, cwd, &[])
}

/// 带额外环境变量运行 CLI（验证 `ZGRAPH_*` 默认值）。
fn run_cli_env(args: &[String], cwd: &Path, envs: &[(&str, &str)]) -> Output {
    run_cli_full(args, cwd, envs, &[])
}

/// 带额外环境变量 + 移除环境变量运行 CLI（解释器优先级测试用）。
fn run_cli_full(args: &[String], cwd: &Path, envs: &[(&str, &str)], removed: &[&str]) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_zgraph"));
    command
        .args(args)
        .envs(envs.iter().copied())
        .current_dir(cwd);
    for name in removed {
        command.env_remove(name);
    }
    command.output().expect("spawn zgraph binary")
}

/// 以文本写文件（图/脚本/Python 模块）。
fn write_text(dir: &Path, name: &str, text: &str) -> PathBuf {
    let path = dir.join(name);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("create parent dir");
    }
    std::fs::write(&path, text).expect("write file");
    path
}

/// 把路径转成 CLI 参数字符串。
fn arg(path: &Path) -> String {
    path.display().to_string()
}

/// 断言退出码并附带 stdout/stderr 便于排障。
fn assert_exit(output: &Output, expected: i32, context: &str) {
    let actual = output.status.code();
    assert_eq!(
        actual,
        Some(expected),
        "{context}: exit={actual:?}\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

/// stdout 必须是一份完整 JSON（`--json` 契约）。
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
// 图与 DB 辅助
// ---------------------------------------------------------------------------

/// 只有 `start -> end` 的内建图（不 spawn worker；用于参数/退出码/输出形状）。
fn io_graph() -> String {
    json!({
        "version": "1.0",
        "metadata": {"name": "io"},
        "entry_points": ["start"],
        "nodes": [
            {"id": "start", "type": "start",
             "inputs": [{"name": "x", "type": "any", "required": true}], "config": {}},
            {"id": "end", "type": "end",
             "inputs": [{"name": "r", "type": "any", "required": true}], "config": {}}
        ],
        "edges": [{"from": "start", "to_node": "end", "to_input_port": "r"}]
    })
    .to_string()
}

/// `start(x) -> work(python_node) -> end(r)` 图。
fn python_graph(module: &str, function: &str) -> String {
    json!({
        "version": "1.0",
        "metadata": {"name": "py"},
        "entry_points": ["start"],
        "nodes": [
            {"id": "start", "type": "start",
             "inputs": [{"name": "x", "type": "any", "required": true}], "config": {}},
            {"id": "work", "type": "python_node",
             "inputs": [{"name": "x", "type": "any", "required": true}],
             "config": {"module": module, "function": function}},
            {"id": "end", "type": "end",
             "inputs": [{"name": "r", "type": "any", "required": true}], "config": {}}
        ],
        "edges": [
            {"from": "start", "to_node": "work", "to_input_port": "x"},
            {"from": "work", "to_node": "end", "to_input_port": "r"}
        ]
    })
    .to_string()
}

/// `start(seconds) -> work(workers.slow) -> end(r)` 慢图（SIGINT 取消用）。
fn sleep_graph() -> String {
    json!({
        "version": "1.0",
        "metadata": {"name": "sleep"},
        "entry_points": ["start"],
        "nodes": [
            {"id": "start", "type": "start",
             "inputs": [{"name": "seconds", "type": "number", "required": true}], "config": {}},
            {"id": "work", "type": "python_node",
             "inputs": [{"name": "seconds", "type": "number", "required": true}],
             "config": {"module": "workers.slow", "function": "run", "timeout_ms": 60000}},
            {"id": "end", "type": "end",
             "inputs": [{"name": "r", "type": "any", "required": true}], "config": {}}
        ],
        "edges": [
            {"from": "start", "to_node": "work", "to_input_port": "seconds"},
            {"from": "work", "to_node": "end", "to_input_port": "r"}
        ]
    })
    .to_string()
}

/// 打开 trace DB（写端已 close/checkpoint，可只读查询）。
fn connect(db: &Path) -> rusqlite::Connection {
    rusqlite::Connection::open(db).expect("open trace db")
}

/// run 行数（`不产生 run` 断言）。
fn runs_count(db: &Path) -> i64 {
    connect(db)
        .query_row("SELECT COUNT(*) FROM runs", [], |row| row.get(0))
        .expect("count runs")
}

/// 某 run 的状态。
fn run_status(db: &Path) -> String {
    connect(db)
        .query_row("SELECT status FROM runs", [], |row| row.get(0))
        .expect("run status")
}

/// `runs.record_mode` 存储值（architecture §5.2 / 06 §6.2.4）。
fn run_record_mode(db: &Path) -> String {
    connect(db)
        .query_row("SELECT record_mode FROM runs", [], |row| {
            row.get::<_, Option<String>>(0)
        })
        .expect("record_mode")
        .expect("record_mode is set by RunStarted")
}

/// 某类 events 行数。
fn event_count(db: &Path, kind: &str) -> i64 {
    connect(db)
        .query_row(
            "SELECT COUNT(*) FROM events WHERE kind = ?1",
            [kind],
            |row| row.get(0),
        )
        .expect("count events")
}

/// `recorded_outputs` 行：`(node_id, attempt, output_hash, output_json 原文)`。
fn recorded_outputs(db: &Path) -> Vec<(String, i64, String, Option<String>)> {
    let conn = connect(db);
    let mut stmt = conn
        .prepare(
            "SELECT node_id, attempt, output_hash, output_json FROM recorded_outputs \
             ORDER BY node_id ASC, attempt ASC",
        )
        .expect("prepare recorded_outputs");
    let rows = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, Option<String>>(3)?,
            ))
        })
        .expect("query recorded_outputs");
    rows.map(|row| row.expect("recorded_outputs row")).collect()
}

/// 某节点最后一条实例快照的状态字符串。
fn instance_status(db: &Path, node_id: &str) -> Option<String> {
    connect(db)
        .query_row(
            "SELECT status FROM instances WHERE node_id = ?1 ORDER BY rowid DESC LIMIT 1",
            [node_id],
            |row| row.get(0),
        )
        .optional()
        .expect("query instance status")
}

/// `llm_calls` 行：`(run_id, call_index, request 原文, response 原文, status, recorded_at)`。
fn llm_calls(db: &Path) -> Vec<(String, i64, String, Option<String>, String, String)> {
    let conn = connect(db);
    let mut stmt = conn
        .prepare(
            "SELECT run_id, call_index, request_json, response_json, status, recorded_at \
             FROM llm_calls ORDER BY instance_id ASC, call_index ASC",
        )
        .expect("prepare llm_calls");
    let rows = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, Option<String>>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, String>(5)?,
            ))
        })
        .expect("query llm_calls");
    rows.map(|row| row.expect("llm_calls row")).collect()
}

/// 某节点最后一个实例的 output JSON。
fn instance_output(db: &Path, node_id: &str) -> Option<Value> {
    let text: Option<String> = connect(db)
        .query_row(
            "SELECT output_json FROM instances WHERE node_id = ?1 ORDER BY rowid DESC LIMIT 1",
            [node_id],
            |row| row.get(0),
        )
        .optional()
        .expect("query instance output");
    text.map(|text| serde_json::from_str(&text).expect("instance output is JSON"))
}

// ---------------------------------------------------------------------------
// 1. 参数与输出形状（内建图，不 spawn worker）
// ---------------------------------------------------------------------------

#[test]
fn help_lists_run_command() {
    let dir = tempfile::tempdir().expect("tempdir");
    let help = run_cli(&["--help".to_string()], dir.path());
    assert_exit(&help, 0, "zgraph --help");
    assert!(String::from_utf8_lossy(&help.stdout).contains("run <graph.json>"));

    let run_help = run_cli(&["run".to_string(), "--help".to_string()], dir.path());
    assert_exit(&run_help, 0, "zgraph run --help");
    let text = String::from_utf8_lossy(&run_help.stdout);
    assert!(text.contains("usage: zgraph run"), "{text}");
    assert!(text.contains("--fake-llm"), "{text}");
    assert!(text.contains("--record"), "{text}");
    assert!(text.contains("$PYO3_PYTHON"), "{text}");
}

#[test]
fn run_json_success_io_graph() {
    let dir = tempfile::tempdir().expect("tempdir");
    let graph = write_text(dir.path(), "io.json", &io_graph());
    let db = dir.path().join("trace.db");
    let output = run_cli(
        &[
            "run".to_string(),
            arg(&graph),
            "--db".to_string(),
            arg(&db),
            "--inputs".to_string(),
            r#"{"start":{"x":1}}"#.to_string(),
            "--json".to_string(),
        ],
        dir.path(),
    );
    assert_exit(&output, 0, "run --json io graph");
    let payload = json_stdout(&output);
    assert_eq!(payload["ok"], json!(true));
    assert_eq!(payload["command"], json!("run"));
    assert_eq!(payload["status"], json!("Completed"));
    assert!(payload["run_id"].as_str().is_some_and(|id| !id.is_empty()));
    assert!(payload["duration_ms"].as_u64().is_some());
    assert!(payload["event_count"].as_i64().unwrap_or(0) > 0);
    assert_eq!(payload["streaming_dropped"], json!(0));
    assert_eq!(payload["abandoned_tasks"], json!(0));
    assert_eq!(payload["trace_backpressure"], json!(0));
    assert_eq!(payload["killed_workers"], json!(0));
    assert_eq!(payload["db"], json!(arg(&db)));
    assert_eq!(payload["run_output"], json!({"r": {"x": 1}}));
    assert_eq!(payload["recovered"], json!([]));
    assert_eq!(payload["warnings"], json!([]));
    assert!(
        payload.get("error").is_none(),
        "success must not carry error"
    );
    assert_eq!(runs_count(&db), 1);
    assert_eq!(run_status(&db), "Completed");
    assert_eq!(event_count(&db, "RunStarted"), 1);
    assert_eq!(event_count(&db, "RunFinished"), 1);
}

#[test]
fn run_human_mode_prints_lifecycle_and_run_output() {
    let dir = tempfile::tempdir().expect("tempdir");
    let graph = write_text(dir.path(), "io.json", &io_graph());
    let db = dir.path().join("trace.db");
    let output = run_cli(
        &[
            "run".to_string(),
            arg(&graph),
            "--db".to_string(),
            arg(&db),
            "--inputs".to_string(),
            r#"{"start":{"x":2}}"#.to_string(),
        ],
        dir.path(),
    );
    assert_exit(&output, 0, "run human mode");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let run_output: Value = serde_json::from_str(stdout.trim()).expect("stdout is run_output JSON");
    assert_eq!(run_output, json!({"r": {"x": 2}}));
    let stderr = stderr_text(&output);
    assert!(stderr.contains("created (graph=io hash="), "{stderr}");
    assert!(stderr.contains("[seq 1] RunStarted"), "{stderr}");
    assert!(stderr.contains("Transition"), "{stderr}");
    assert!(stderr.contains("RunFinished"), "{stderr}");
    assert!(stderr.contains("ok run="), "{stderr}");
    assert!(stderr.contains("status=Completed"), "{stderr}");
    assert!(stderr.contains("events="), "{stderr}");
}

#[test]
fn run_quiet_suppresses_progress_only() {
    let dir = tempfile::tempdir().expect("tempdir");
    let graph = write_text(dir.path(), "io.json", &io_graph());
    let db = dir.path().join("quiet.db");
    let output = run_cli(
        &[
            "run".to_string(),
            arg(&graph),
            "--db".to_string(),
            arg(&db),
            "--inputs".to_string(),
            r#"{"start":{"x":4}}"#.to_string(),
            "-q".to_string(),
        ],
        dir.path(),
    );
    assert_exit(&output, 0, "run -q");
    assert!(
        stderr_text(&output).is_empty(),
        "-q suppresses progress lines"
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let run_output: Value = serde_json::from_str(stdout.trim()).expect("stdout run_output JSON");
    assert_eq!(run_output, json!({"r": {"x": 4}}));
}

#[test]
fn run_accepts_shorthand_and_at_file_inputs() {
    let dir = tempfile::tempdir().expect("tempdir");
    let graph = write_text(dir.path(), "io.json", &io_graph());
    let shorthand_db = dir.path().join("shorthand.db");
    let shorthand = run_cli(
        &[
            "run".to_string(),
            arg(&graph),
            "--db".to_string(),
            arg(&shorthand_db),
            "--inputs".to_string(),
            r#"{"x":7}"#.to_string(),
            "--json".to_string(),
        ],
        dir.path(),
    );
    assert_exit(&shorthand, 0, "single-entry shorthand");
    assert_eq!(
        json_stdout(&shorthand)["run_output"],
        json!({"r": {"x": 7}})
    );

    let inputs_file = write_text(dir.path(), "inputs.json", r#"{"start":{"x":9}}"#);
    let file_db = dir.path().join("file.db");
    let from_file = run_cli(
        &[
            "run".to_string(),
            arg(&graph),
            "--db".to_string(),
            arg(&file_db),
            "--inputs".to_string(),
            format!("@{}", inputs_file.display()),
            "--json".to_string(),
        ],
        dir.path(),
    );
    assert_exit(&from_file, 0, "@file inputs");
    assert_eq!(
        json_stdout(&from_file)["run_output"],
        json!({"r": {"x": 9}})
    );
}

#[test]
fn run_real_flag_smoke_without_agent_nodes() {
    let dir = tempfile::tempdir().expect("tempdir");
    let graph = write_text(dir.path(), "io.json", &io_graph());
    let db = dir.path().join("real.db");
    let output = run_cli(
        &[
            "run".to_string(),
            arg(&graph),
            "--db".to_string(),
            arg(&db),
            "--inputs".to_string(),
            r#"{"start":{"x":3}}"#.to_string(),
            "--real".to_string(),
            "--json".to_string(),
        ],
        dir.path(),
    );
    assert_exit(&output, 0, "--real provider construction");
    assert_eq!(json_stdout(&output)["status"], json!("Completed"));
}

// ---------------------------------------------------------------------------
// 2. 退出码映射：入口输入 3 / DSL 2 / 存储 4
// ---------------------------------------------------------------------------

#[test]
fn run_rejects_invalid_entry_inputs_without_run() {
    let dir = tempfile::tempdir().expect("tempdir");
    let graph = write_text(dir.path(), "io.json", &io_graph());
    let cases: [(&str, &str); 5] = [
        ("non-object", "[]"),
        ("non-entry-node", r#"{"end":{"r":1}}"#),
        ("undeclared-port", r#"{"start":{"nope":1}}"#),
        ("missing-required", "{}"),
        ("malformed-json", "{"),
    ];
    for (name, inputs) in cases {
        let db = dir.path().join(format!("{name}.db"));
        let output = run_cli(
            &[
                "run".to_string(),
                arg(&graph),
                "--db".to_string(),
                arg(&db),
                "--inputs".to_string(),
                inputs.to_string(),
                "--json".to_string(),
            ],
            dir.path(),
        );
        assert_exit(&output, 3, name);
        let stderr = stderr_text(&output);
        assert!(
            stderr.contains("error[RUN_ENTRY_INVALID]")
                || stderr.contains("error[RUN_ENTRY_INPUT_MISSING]"),
            "{name}: {stderr}"
        );
        assert!(output.stdout.is_empty(), "{name}: stdout must be empty");
        assert_eq!(runs_count(&db), 0, "{name}: must not create a run");
    }
}

#[test]
fn run_dsl_error_exits_2_without_run() {
    let dir = tempfile::tempdir().expect("tempdir");
    let bad_graph = repo_root().join("tests/fixtures/graphs/bad_parse_error.json");
    let db = dir.path().join("bad.db");
    let output = run_cli(
        &[
            "run".to_string(),
            arg(&bad_graph),
            "--db".to_string(),
            arg(&db),
            "--inputs".to_string(),
            "{}".to_string(),
        ],
        dir.path(),
    );
    assert_exit(&output, 2, "bad graph");
    let stderr = stderr_text(&output);
    assert!(stderr.contains("error[DSL_"), "{stderr}");
    assert_eq!(runs_count(&db), 0);
}

#[test]
fn run_unreadable_graph_exits_2() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db = dir.path().join("missing.db");
    let output = run_cli(
        &[
            "run".to_string(),
            arg(&dir.path().join("nope.json")),
            "--db".to_string(),
            arg(&db),
            "--inputs".to_string(),
            "{}".to_string(),
        ],
        dir.path(),
    );
    assert_exit(&output, 2, "missing graph file");
    assert_eq!(runs_count(&db), 0);
}

#[test]
fn run_storage_error_exits_4() {
    let dir = tempfile::tempdir().expect("tempdir");
    // DB 路径的父级是普通文件 → 打开/建库失败（06 §6.9.1 / §6.10.5）。
    let blocker = write_text(dir.path(), "blocker", "not a directory");
    let db = blocker.join("trace.db");
    let output = run_cli(
        &[
            "run".to_string(),
            arg(&dir.path().join("unused.json")),
            "--db".to_string(),
            arg(&db),
            "--inputs".to_string(),
            "{}".to_string(),
        ],
        dir.path(),
    );
    assert_exit(&output, 4, "unwritable db");
    assert!(stderr_text(&output).contains("error[STORAGE_ERROR]"));
}

#[test]
fn run_invalid_fake_llm_script_exits_2() {
    let dir = tempfile::tempdir().expect("tempdir");
    let graph = write_text(dir.path(), "io.json", &io_graph());
    let script = write_text(dir.path(), "bad_script.json", r#"{"version":1}"#);
    let db = dir.path().join("script.db");
    let output = run_cli(
        &[
            "run".to_string(),
            arg(&graph),
            "--db".to_string(),
            arg(&db),
            "--inputs".to_string(),
            r#"{"start":{"x":1}}"#.to_string(),
            format!("--fake-llm={}", script.display()),
        ],
        dir.path(),
    );
    assert_exit(&output, 2, "invalid fake-llm script");
    assert!(stderr_text(&output).contains("--fake-llm"));
    assert_eq!(runs_count(&db), 0);
}

// ---------------------------------------------------------------------------
// 3. 失败 run：--json 结构与退出码 1（真 worker 抛异常）
// ---------------------------------------------------------------------------

#[test]
fn run_failed_run_emits_json_error_and_exits_1() {
    let dir = tempfile::tempdir().expect("tempdir");
    let module = write_text(
        dir.path(),
        "boom.py",
        "def run(inputs, ctx):\n    raise RuntimeError(\"boom\")\n",
    );
    let module_dir = module.parent().expect("module dir");
    let graph = write_text(dir.path(), "boom_graph.json", &python_graph("boom", "run"));
    let db = dir.path().join("boom.db");
    let output = run_cli(
        &[
            "run".to_string(),
            arg(&graph),
            "--db".to_string(),
            arg(&db),
            "--inputs".to_string(),
            r#"{"start":{"x":1}}"#.to_string(),
            "--python".to_string(),
            python_program(),
            "--python-path".to_string(),
            arg(&python_path()),
            "--python-path".to_string(),
            arg(module_dir),
            "--json".to_string(),
        ],
        dir.path(),
    );
    assert_exit(&output, 1, "python exception run");
    let payload = json_stdout(&output);
    assert_eq!(payload["ok"], json!(false));
    assert_eq!(payload["status"], json!("Failed"));
    assert_eq!(payload["run_output"], Value::Null);
    assert_eq!(payload["error"]["code"], json!("NODE_PYTHON_EXCEPTION"));
    assert!(
        payload["error"]["message"]
            .as_str()
            .is_some_and(|message| message.contains("boom")),
        "{payload}"
    );
    assert_eq!(runs_count(&db), 1);
    assert_eq!(run_status(&db), "Failed");

    // human 模式失败：stdout `null`、stderr 有 summary 与 error 行。
    let human_db = dir.path().join("boom_human.db");
    let human = run_cli(
        &[
            "run".to_string(),
            arg(&graph),
            "--db".to_string(),
            arg(&human_db),
            "--inputs".to_string(),
            r#"{"start":{"x":1}}"#.to_string(),
            "--python".to_string(),
            python_program(),
            "--python-path".to_string(),
            arg(&python_path()),
            "--python-path".to_string(),
            arg(module_dir),
        ],
        dir.path(),
    );
    assert_exit(&human, 1, "python exception run (human mode)");
    assert_eq!(String::from_utf8_lossy(&human.stdout).trim(), "null");
    let stderr = stderr_text(&human);
    assert!(stderr.contains("status=Failed"), "{stderr}");
    assert!(stderr.contains("error[NODE_PYTHON_EXCEPTION]"), "{stderr}");
}

// ---------------------------------------------------------------------------
// 4. 真链路：fake LLM + 真 Python worker + 真 trace（AC-08 图）
// ---------------------------------------------------------------------------

/// AC-08 图的基础参数（graph/assets/tools/python/python-path/inputs）。
fn ac08_args(db: &Path) -> Vec<String> {
    vec![
        "run".to_string(),
        arg(&ac08_dir().join("graph.json")),
        "--db".to_string(),
        arg(db),
        "--assets-dir".to_string(),
        arg(&ac08_dir()),
        "--inputs".to_string(),
        r#"{"start":{"message":"请计算 2 + 3 的结果"}}"#.to_string(),
        "--python".to_string(),
        python_program(),
        "--python-path".to_string(),
        arg(&python_path()),
    ]
}

/// 用 fixture 的 fake-llm 脚本跑 AC-08 图。
fn run_ac08(dir: &Path, extra: &[&str]) -> (Output, PathBuf) {
    let db = dir.join("trace.db");
    let mut args = ac08_args(&db);
    let script = ac08_dir().join("fake_llm.json");
    args.push("--fake-llm".to_string());
    args.push(format!("--fake-llm={}", script.display()));
    args.extend(extra.iter().map(|value| value.to_string()));
    (run_cli(&args, dir), db)
}

#[test]
fn run_ac08_real_chain_streams_and_records_hash() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (output, db) = run_ac08(dir.path(), &["--json", "--stream"]);
    assert_exit(&output, 0, "AC-08 real chain");
    let payload = json_stdout(&output);
    assert_eq!(payload["ok"], json!(true));
    assert_eq!(payload["status"], json!("Completed"));
    let run_output = &payload["run_output"]["result"];
    assert_eq!(run_output["content"], json!("{\"answer\":\"5\"}"));
    assert_eq!(run_output["stop_reason"], json!("EndTurn"));
    assert_eq!(run_output["iterations"], json!(2));
    assert!(
        run_output["usage"]["total_tokens"].as_u64().unwrap_or(0) > 0,
        "{run_output}"
    );
    assert!(payload["event_count"].as_i64().unwrap_or(0) > 0);
    // 流式文本实时到 stderr（--stream；token 段拼接为最终 content）。
    assert!(
        stderr_text(&output).contains("answer"),
        "--stream should print token text: {}",
        stderr_text(&output)
    );
    // trace：生命周期骨架 + 真 worker 工具调用 + agent 实例完成。
    assert_eq!(event_count(&db, "RunStarted"), 1);
    assert_eq!(event_count(&db, "RunFinished"), 1);
    assert!(event_count(&db, "Streaming") > 0);
    assert_eq!(run_record_mode(&db), "hash", "default --record=hash");
    assert_eq!(
        connect(&db)
            .query_row(
                "SELECT status FROM instances WHERE node_id = 'answer_writer' \
                 ORDER BY rowid DESC LIMIT 1",
                [],
                |row| row.get::<_, String>(0),
            )
            .expect("answer_writer instance"),
        "Completed"
    );
    // 默认 `--record=hash`：只落 hash（无 output_json），64 hex 且可复算。
    let rows = recorded_outputs(&db);
    assert!(!rows.is_empty(), "hash mode must record outputs");
    let agent_output = instance_output(&db, "answer_writer").expect("agent output_json");
    let expected_hash = zgraph_cli::run_cmd::output_hash(&agent_output);
    assert_eq!(expected_hash.len(), 64);
    let agent_rows: Vec<_> = rows.iter().filter(|row| row.0 == "answer_writer").collect();
    assert_eq!(agent_rows.len(), 1, "one attempt for a single agent call");
    assert_eq!(
        agent_rows[0].3, None,
        "hash mode must not store output_json"
    );
    assert_eq!(agent_rows[0].2, expected_hash);
}

#[test]
fn run_ac08_record_full_stores_agent_output_json() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (output, db) = run_ac08(dir.path(), &["--json", "--record=full"]);
    assert_exit(&output, 0, "AC-08 record full");
    let payload = json_stdout(&output);
    let agent_output = instance_output(&db, "answer_writer").expect("agent output_json");
    for key in ["content", "stop_reason", "iterations", "usage"] {
        assert!(
            agent_output.get(key).is_some(),
            "agent output must be the frozen four-field package: {agent_output}"
        );
    }
    let rows = recorded_outputs(&db);
    let agent_row = rows
        .iter()
        .find(|row| row.0 == "answer_writer")
        .expect("recorded agent output");
    let recorded = agent_row
        .3
        .as_ref()
        .expect("full mode must store output_json");
    let recorded: Value = serde_json::from_str(recorded).expect("recorded output json");
    assert_eq!(recorded, agent_output, "recorded json == instance output");
    assert_eq!(agent_row.2, zgraph_cli::run_cmd::output_hash(&agent_output));
    assert_eq!(run_record_mode(&db), "full");
    assert!(
        agent_row.1 == 0,
        "single successful attempt starts at attempt 0: {agent_row:?}"
    );
    // llm_calls：fake 脚本两轮（tool_calls + final text）→ 迭代 2 次 → 2 行。
    let calls = llm_calls(&db);
    assert_eq!(
        calls.len(),
        2,
        "one llm_calls row per LLM iteration: {calls:?}"
    );
    assert_eq!(calls[0].1, 0, "call_index starts at 0");
    assert_eq!(calls[1].1, 1);
    let expected_run_id = payload["run_id"].as_str().expect("run_id");
    for (run_id, _, request, response, status, recorded_at) in &calls {
        assert_eq!(run_id, expected_run_id, "llm_calls.run_id");
        assert_eq!(status, "ok");
        assert!(response.is_some(), "successful call records the response");
        assert!(recorded_at.ends_with('Z'), "recorded_at is RFC3339 ms Z");
        assert!(
            !request.contains("api_key"),
            "request snapshot must be sanitized: {request}"
        );
        // request 是合法 JSON 且含脱敏后的 model/messages 结构。
        let request: Value = serde_json::from_str(request).expect("request_json is JSON");
        assert!(request["model"]["name"].is_string());
        assert!(request["messages"].is_array());
    }
}

#[test]
fn run_ac08_record_none_writes_no_recorded_outputs() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (output, db) = run_ac08(dir.path(), &["--json", "--record", "none"]);
    assert_exit(&output, 0, "AC-08 record none");
    assert_eq!(json_stdout(&output)["status"], json!("Completed"));
    assert!(
        recorded_outputs(&db).is_empty(),
        "record=none must not write recorded_outputs"
    );
    assert_eq!(run_record_mode(&db), "none");
    assert_eq!(event_count(&db, "RunFinished"), 1);
}

#[test]
fn run_bare_fake_llm_uses_builtin_default_script() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db = dir.path().join("default.db");
    let mut args = ac08_args(&db);
    args.push("--fake-llm".to_string());
    args.push("--json".to_string());
    let output = run_cli(&args, dir.path());
    assert_exit(&output, 0, "bare --fake-llm");
    let payload = json_stdout(&output);
    assert_eq!(payload["status"], json!("Completed"));
    let result = &payload["run_output"]["result"];
    assert_eq!(result["content"], json!("{\"intent\":\"refund\"}"));
    assert_eq!(result["iterations"], json!(1));
}

#[test]
fn run_uses_env_defaults_for_db_assets_and_python() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db = dir.path().join("env.db");
    let graph = ac08_dir().join("graph.json");
    let script = ac08_dir().join("fake_llm.json");
    let python = python_program();
    let assets = arg(&ac08_dir());
    let db_arg = arg(&db);
    let args = vec![
        "run".to_string(),
        arg(&graph),
        "--inputs".to_string(),
        r#"{"start":{"message":"2+3"}}"#.to_string(),
        "--python-path".to_string(),
        arg(&python_path()),
        format!("--fake-llm={}", script.display()),
        "--json".to_string(),
    ];
    let output = run_cli_env(
        &args,
        dir.path(),
        &[
            ("ZGRAPH_DB", db_arg.as_str()),
            ("ZGRAPH_ASSETS", assets.as_str()),
            ("ZGRAPH_PYTHON", python.as_str()),
        ],
    );
    assert_exit(&output, 0, "env defaults for db/assets/python");
    let payload = json_stdout(&output);
    assert_eq!(payload["status"], json!("Completed"));
    assert_eq!(payload["db"], json!(db_arg));
    assert!(db.exists(), "ZGRAPH_DB must be honoured");
    assert!(!recorded_outputs(&db).is_empty());
}

// ---------------------------------------------------------------------------
// 4b. Python 解释器优先级：--python > PYO3_PYTHON > ZGRAPH_PYTHON > python3
// ---------------------------------------------------------------------------

/// 解析优先级用的 `start -> echo -> end` 参数（不含 `--python`）。
fn echo_args(dir: &Path, db: &Path) -> Vec<String> {
    let graph = dir.join("echo.json");
    std::fs::write(&graph, python_graph("workers.echo", "run")).expect("write echo graph");
    vec![
        "run".to_string(),
        arg(&graph),
        "--db".to_string(),
        arg(db),
        "--inputs".to_string(),
        r#"{"start":{"x":1}}"#.to_string(),
        "--python-path".to_string(),
        arg(&python_path()),
        "--json".to_string(),
    ]
}

#[test]
fn python_flag_overrides_both_env_vars() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db = dir.path().join("flag.db");
    let mut args = echo_args(dir.path(), &db);
    args.insert(6, python_program());
    args.insert(6, "--python".to_string());
    let output = run_cli_full(
        &args,
        dir.path(),
        &[
            ("PYO3_PYTHON", "/nonexistent/pyo3-python"),
            ("ZGRAPH_PYTHON", "/nonexistent/zgraph-python"),
        ],
        &[],
    );
    assert_exit(&output, 0, "--python must win over both env vars");
    assert_eq!(run_status(&db), "Completed");
}

#[test]
fn pyo3_python_env_precedes_zgraph_python_env_then_python3() {
    let dir = tempfile::tempdir().expect("tempdir");
    let real = python_program();

    // a) PYO3_PYTHON 真实、ZGRAPH_PYTHON 非法 → 用 PYO3 的解释器，run 成功。
    let a_db = dir.path().join("a.db");
    let a = run_cli_full(
        &echo_args(dir.path(), &a_db),
        dir.path(),
        &[
            ("PYO3_PYTHON", real.as_str()),
            ("ZGRAPH_PYTHON", "/nonexistent/zgraph-python"),
        ],
        &[],
    );
    assert_exit(&a, 0, "PYO3_PYTHON should win over ZGRAPH_PYTHON");

    // b) PYO3_PYTHON 非法、ZGRAPH_PYTHON 真实 → 若 PYO3 优先，worker spawn 失败。
    let b_db = dir.path().join("b.db");
    let b = run_cli_full(
        &echo_args(dir.path(), &b_db),
        dir.path(),
        &[
            ("PYO3_PYTHON", "/nonexistent/pyo3-python"),
            ("ZGRAPH_PYTHON", real.as_str()),
        ],
        &[],
    );
    assert_exit(&b, 1, "PYO3_PYTHON takes precedence (spawn fails)");
    let payload = json_stdout(&b);
    assert_eq!(payload["status"], json!("Failed"));
    assert_eq!(payload["error"]["code"], json!("NODE_WORKER_CRASH"));

    // c) 两个 env 都移除 → 回退 python3（若测试机没有 python3，本测试如实失败）。
    let c_db = dir.path().join("c.db");
    let c = run_cli_full(
        &echo_args(dir.path(), &c_db),
        dir.path(),
        &[],
        &["PYO3_PYTHON", "ZGRAPH_PYTHON"],
    );
    assert_exit(&c, 0, "missing env vars fall back to python3");
}

// ---------------------------------------------------------------------------
// 4c. `--db :memory:`（06 §6.9.1：接受并警告，跨进程不可读）
// ---------------------------------------------------------------------------

#[test]
fn run_memory_db_outputs_outcome_and_warns() {
    let dir = tempfile::tempdir().expect("tempdir");
    let graph = write_text(dir.path(), "io.json", &io_graph());
    let json_run = run_cli(
        &[
            "run".to_string(),
            arg(&graph),
            "--db".to_string(),
            ":memory:".to_string(),
            "--inputs".to_string(),
            r#"{"start":{"x":5}}"#.to_string(),
            "--json".to_string(),
        ],
        dir.path(),
    );
    assert_exit(&json_run, 0, ":memory: json run");
    let payload = json_stdout(&json_run);
    assert_eq!(payload["ok"], json!(true));
    assert_eq!(payload["status"], json!("Completed"));
    assert_eq!(payload["db"], json!(":memory:"));
    assert_eq!(payload["run_output"], json!({"r": {"x": 5}}));
    // trace 跨进程不可读：event_count 记为 0（输出仍完整）。
    assert_eq!(payload["event_count"], json!(0));
    let stderr = stderr_text(&json_run);
    assert!(stderr.contains("warning"), "{stderr}");
    assert!(stderr.contains(":memory:"), "{stderr}");

    let human_run = run_cli(
        &[
            "run".to_string(),
            arg(&graph),
            "--db".to_string(),
            ":memory:".to_string(),
            "--inputs".to_string(),
            r#"{"start":{"x":6}}"#.to_string(),
        ],
        dir.path(),
    );
    assert_exit(&human_run, 0, ":memory: human run");
    let stdout = String::from_utf8_lossy(&human_run.stdout);
    let run_output: Value = serde_json::from_str(stdout.trim()).expect("stdout run_output JSON");
    assert_eq!(run_output, json!({"r": {"x": 6}}));
    let stderr = stderr_text(&human_run);
    assert!(stderr.contains("warning"), "{stderr}");
    assert!(stderr.contains("status=Completed"), "{stderr}");
}

#[test]
fn trace_memory_db_exits_4_with_storage_error() {
    let dir = tempfile::tempdir().expect("tempdir");
    let output = run_cli(
        &[
            "trace".to_string(),
            "any-run".to_string(),
            "--db".to_string(),
            ":memory:".to_string(),
        ],
        dir.path(),
    );
    assert_exit(&output, 4, "trace :memory:");
    assert!(
        stderr_text(&output).contains("error[STORAGE_ERROR]"),
        "{}",
        stderr_text(&output)
    );
}

// ---------------------------------------------------------------------------
// 4d. ISSUE-076：AgentConfig.node_timeout_ms 经 CLI 包装层转发到内核 deadline
// ---------------------------------------------------------------------------

#[test]
fn run_agent_config_timeout_hint_is_forwarded_end_to_end() {
    let dir = tempfile::tempdir().expect("tempdir");
    // 图：复制 AC-08 图但删除 agent 节点的 DSL timeout_ms（让执行器 hint 生效）。
    let graph_src = std::fs::read_to_string(ac08_dir().join("graph.json")).expect("graph fixture");
    let mut graph: Value = serde_json::from_str(&graph_src).expect("graph json");
    for node in graph["nodes"].as_array_mut().expect("nodes") {
        if node["type"] == json!("agent_call") {
            node["config"]
                .as_object_mut()
                .expect("agent config")
                .remove("timeout_ms");
        }
    }
    let graph_path = write_text(dir.path(), "graph.json", &graph.to_string());
    // 资产：AgentConfig.node_timeout_ms=1000（合法下界），tools=[]。
    let agent_src = std::fs::read_to_string(ac08_dir().join("agents/answer_writer.json"))
        .expect("agent fixture");
    let mut agent: Value = serde_json::from_str(&agent_src).expect("agent json");
    agent["tools"] = json!([]);
    agent["node_timeout_ms"] = json!(1000);
    write_text(
        dir.path(),
        "assets/agents/answer_writer.json",
        &agent.to_string(),
    );
    let prompt = std::fs::read_to_string(ac08_dir().join("agents/prompt.md")).expect("prompt");
    write_text(dir.path(), "assets/agents/prompt.md", &prompt);
    std::fs::create_dir_all(dir.path().join("assets/tools")).expect("tools dir");
    // fake LLM：20 字符 × 200ms/字符 ≈ 4s，远超 1000ms 的 AgentConfig 超时。
    let script = write_text(
        dir.path(),
        "fake_llm.json",
        &json!({
            "version": 1,
            "chunk_chars": 1,
            "chunk_delay_ms": 200,
            "calls": [{"kind": "text", "content": "0123456789abcdefghij",
                       "stop_reason": "EndTurn"}]
        })
        .to_string(),
    );
    let db = dir.path().join("hint.db");
    let output = run_cli(
        &[
            "run".to_string(),
            arg(&graph_path),
            "--db".to_string(),
            arg(&db),
            "--assets-dir".to_string(),
            arg(&dir.path().join("assets")),
            "--tools-dir".to_string(),
            arg(&dir.path().join("assets/tools")),
            "--inputs".to_string(),
            r#"{"start":{"message":"hi"}}"#.to_string(),
            "--python".to_string(),
            python_program(),
            "--python-path".to_string(),
            arg(&python_path()),
            format!("--fake-llm={}", script.display()),
            "--json".to_string(),
        ],
        dir.path(),
    );
    assert_exit(&output, 1, "timeout_hint run must fail by NodeTimeout");
    let payload = json_stdout(&output);
    assert_eq!(payload["status"], json!("Failed"));
    assert_eq!(
        payload["error"]["code"],
        json!("NODE_TIMEOUT"),
        "AgentConfig.node_timeout_ms must become the kernel deadline (ISSUE-076): {payload}"
    );
    assert_eq!(
        instance_status(&db, "answer_writer").as_deref(),
        Some("TimedOut"),
        "instance must be TimedOut, not the 180s default timeout"
    );
    assert!(event_count(&db, "NodeTimedOut") >= 1);
}

// ---------------------------------------------------------------------------
// 5. SIGINT：Ctrl-C → cancel → 退出码 130（06 §6.10.5）
// ---------------------------------------------------------------------------

/// 等待子进程退出并收集输出（宽松上限，避免测试挂死）。
#[cfg(unix)]
fn wait_and_collect(
    mut child: Child,
    timeout: Duration,
) -> (std::process::ExitStatus, String, String) {
    let mut stdout = child.stdout.take().expect("child stdout");
    let mut stderr = child.stderr.take().expect("child stderr");
    let started = Instant::now();
    loop {
        if let Some(status) = child.try_wait().expect("try_wait") {
            let mut out = String::new();
            let mut err = String::new();
            stdout.read_to_string(&mut out).expect("read stdout");
            stderr.read_to_string(&mut err).expect("read stderr");
            return (status, out, err);
        }
        assert!(
            started.elapsed() < timeout,
            "child did not exit within {timeout:?}"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[cfg(unix)]
#[test]
fn run_sigint_cancels_and_exits_130() {
    let dir = tempfile::tempdir().expect("tempdir");
    let graph = write_text(dir.path(), "sleep.json", &sleep_graph());
    let db = dir.path().join("sleep.db");
    let child = Command::new(env!("CARGO_BIN_EXE_zgraph"))
        .args([
            "run".to_string(),
            arg(&graph),
            "--db".to_string(),
            arg(&db),
            "--inputs".to_string(),
            r#"{"start":{"seconds":5}}"#.to_string(),
            "--python".to_string(),
            python_program(),
            "--python-path".to_string(),
            arg(&python_path()),
        ])
        .current_dir(dir.path())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn zgraph run");
    // 等 handler 安装 + run 进入 Running（python worker 在飞），再发 SIGINT。
    std::thread::sleep(Duration::from_millis(1_200));
    let killed = Command::new("kill")
        .arg("-INT")
        .arg(child.id().to_string())
        .status()
        .expect("send SIGINT");
    assert!(killed.success(), "kill -INT failed: {killed:?}");

    let (status, stdout, stderr) = wait_and_collect(child, Duration::from_secs(30));
    assert_eq!(
        status.code(),
        Some(130),
        "SIGINT exit code; stderr: {stderr}"
    );
    assert!(stderr.contains("status=Cancelled"), "{stderr}");
    assert_eq!(runs_count(&db), 1);
    assert_eq!(run_status(&db), "Cancelled");
    assert_eq!(
        event_count(&db, "RunFinished"),
        1,
        "RunFinished must be emitted exactly once"
    );
    assert_eq!(stdout.trim(), "null", "cancelled run has no run_output");
}
