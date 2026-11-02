//! v0.4/D5b2：工具调用落库后的 `trace` / `replay --mode=tools` CLI 契约测试。
//!
//! 覆盖（DR-014 §3/§4/§7、07 AC-24 c/d/g/i）：
//! - `trace` 人类工具行（events/HumanDecision/ForeachExpansion 之后；不打印完整
//!   args/result）与 `--json` `{"kind":"ToolCall",...}` NDJSON 行（仍为 NDJSON，
//!   不做顶层数组）；无工具调用默认输出不变；`--stats` additive 计数；
//!   `replay --mode=events --json` 与 `trace --json` 逐字节一致。
//! - `replay --mode=tools`（recorded）：按 `(started_at, call_id)` 输出；缺记录/
//!   截断/JSON 或 sha 不一致 → `REPLAY_TOOL_RECORD_*` exit 3；`--replay-tools=reexec`
//!   → 用法错误 exit 2（v0.4 未实现）；**不执行任何工具/不 spawn/不改 workspace**
//!   （DB 主文件字节、行数、workspace 快照均不变，`ZGRAPH_PYTHON=/nonexistent` 仍成功）。
//!
//! 稳定性：每个用例独立 `TempDir`/DB；无网络；不做 sleep 猜测。

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::{json, Value};
use tempfile::TempDir;
use zgraph_trace::{
    BashToolCall, RecordMode, RunFinishedInfo, RunMetrics, RunStartedInfo, RunStatus,
    StreamingEvent, ToolCallRecord, ToolCallSource, ToolCallStatus, ToolSideEffect, TraceInput,
    TraceWriter,
};

const RUN_ID: &str = "run-tools-trace";
const GRAPH_HASH: &str = "0123456789abcdef";

struct Fixture {
    _dir: TempDir,
    path: PathBuf,
}

impl Fixture {
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

fn zgraph_env(args: &[&str], envs: &[(&str, &str)]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_zgraph"))
        .args(args)
        .envs(envs.iter().copied())
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

fn json_lines(output: &Output) -> Vec<Value> {
    stdout(output)
        .lines()
        .map(|line| serde_json::from_str(line).expect("each stdout line is JSON"))
        .collect()
}

/// 一条工具调用记录（builtin__bash：副作用/bash 元数据；时间用调用方给定）。
fn bash_tool_call(call_id: &str, started_at: &str) -> ToolCallRecord {
    ToolCallRecord {
        run_id: RUN_ID.to_string(),
        instance_id: "inst-agent-0".to_string(),
        node_id: "agent_node".to_string(),
        attempt: 0,
        call_id: call_id.to_string(),
        tool_name: "builtin__bash".to_string(),
        source: ToolCallSource::Builtin,
        status: ToolCallStatus::Ok,
        args: json!({"command": "echo hi"}),
        result: Some(json!({"ok": true, "stdout": "hi"})),
        error: None,
        started_at: started_at.to_string(),
        finished_at: started_at.to_string(),
        duration_ms: 7,
        side_effect: None,
        bash: Some(BashToolCall {
            command: "echo hi".to_string(),
            cwd: "/workspace".to_string(),
            exit_code: Some(0),
            signal: None,
            stdout: "hi\n".to_string(),
            stderr: String::new(),
            sandbox_profile_sha256: Some("p".repeat(64)),
        }),
    }
}

/// python 工具（error + 副作用路径）——验证 source/status 映射与字段透传。
fn python_tool_call(call_id: &str, started_at: &str) -> ToolCallRecord {
    ToolCallRecord {
        run_id: RUN_ID.to_string(),
        instance_id: "inst-agent-0".to_string(),
        node_id: "agent_node".to_string(),
        attempt: 0,
        call_id: call_id.to_string(),
        tool_name: "search".to_string(),
        source: ToolCallSource::Python,
        status: ToolCallStatus::Error,
        args: json!({"query": "q"}),
        result: None,
        error: Some(json!({"error": {"reason": "boom"}})),
        started_at: started_at.to_string(),
        finished_at: started_at.to_string(),
        duration_ms: 3,
        side_effect: Some(ToolSideEffect {
            path: "out/result.txt".to_string(),
            before_sha256: None,
            after_sha256: Some("after".to_string()),
            bytes_before: None,
            bytes_after: Some(5),
        }),
        bash: None,
    }
}

fn run_started(record_mode: RecordMode) -> TraceInput {
    TraceInput::RunStarted(RunStartedInfo {
        run_id: RUN_ID.to_string(),
        graph_hash: GRAPH_HASH.to_string(),
        graph_name: "tool-demo".to_string(),
        dsl_json: "{}".to_string(),
        user_id: "local".to_string(),
        entry_inputs: json!({}),
        global_vars: json!({}),
        seed: 7,
        record_mode,
        owner_pid: std::process::id(),
        owner_host: "test-host".to_string(),
        started_at: "2026-11-02T10:00:00.000Z".to_string(),
    })
}

fn run_finished() -> TraceInput {
    TraceInput::RunFinished(RunFinishedInfo {
        run_id: RUN_ID.to_string(),
        status: RunStatus::Completed,
        output_summary: Some(json!({})),
        error: None,
        metrics: RunMetrics {
            streaming_dropped: 0,
            abandoned_tasks: 0,
            trace_backpressure: 0,
            killed_workers: 0,
        },
        at: "2026-11-02T10:00:09.000Z".to_string(),
    })
}

fn streaming_tool_call(call_id: &str, tool_name: &str, at: &str) -> TraceInput {
    TraceInput::Streaming {
        run_id: RUN_ID.to_string(),
        instance_id: "inst-agent-0".to_string(),
        node_id: Some("agent_node".to_string()),
        event: StreamingEvent::ToolCallStart {
            call_id: call_id.to_string(),
            tool_name: tool_name.to_string(),
            args_summary: json!({"keys": ["command"], "bytes": 20}),
        },
        at: at.to_string(),
    }
}

/// 写一个 v7 库：2 次工具调用（+ 对应 2 个 streaming tool_call_start 事件）。
///
/// 记录顺序故意与 `(started_at, call_id)` 不同（python 调用更早），验证输出排序。
fn fixture() -> Fixture {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("trace.db");
    let writer = TraceWriter::open(&path).expect("open trace writer");
    let handle = writer.handle();
    handle.send(run_started(RecordMode::Hash));
    handle.send(streaming_tool_call(
        "t1",
        "builtin__bash",
        "2026-11-02T10:00:02.000Z",
    ));
    handle.send(streaming_tool_call(
        "t2",
        "search",
        "2026-11-02T10:00:01.000Z",
    ));
    // 先写 t1（较晚），再写 t2（较早）→ 输出必须按 (started_at, call_id)。
    handle.send(TraceInput::ToolCall(bash_tool_call(
        "t1",
        "2026-11-02T10:00:02.000Z",
    )));
    handle.send(TraceInput::ToolCall(python_tool_call(
        "t2",
        "2026-11-02T10:00:01.000Z",
    )));
    handle.send(run_finished());
    writer.flush().expect("flush");
    writer.close().expect("close");
    Fixture { _dir: dir, path }
}

/// 无工具调用的 v7 库（默认输出不变）。
fn fixture_without_tools() -> Fixture {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("trace.db");
    let writer = TraceWriter::open(&path).expect("open trace writer");
    let handle = writer.handle();
    handle.send(run_started(RecordMode::Hash));
    handle.send(run_finished());
    writer.flush().expect("flush");
    writer.close().expect("close");
    Fixture { _dir: dir, path }
}

fn db_connection(path: &Path) -> rusqlite::Connection {
    rusqlite::Connection::open(path).expect("open db")
}

fn tool_call_count(path: &Path) -> i64 {
    db_connection(path)
        .query_row("SELECT count(*) FROM tool_calls", [], |row| row.get(0))
        .expect("tool_calls count")
}

// ---------------------------------------------------------------------------
// trace 输出（DR-014 §3；Lead 裁决保持 NDJSON）
// ---------------------------------------------------------------------------

#[test]
fn human_trace_appends_tool_lines_without_full_args() {
    let fx = fixture();
    let output = zgraph(&["trace", RUN_ID, "--db", path_str(fx.path())]);
    assert_eq!(code(&output), 0, "stderr={}", stderr(&output));
    let text = stdout(&output);

    assert!(text.contains("tool_calls: 2"), "{text}");
    assert!(
        text.contains("builtin__bash")
            && text.contains("source=builtin")
            && text.contains("status=ok"),
        "{text}"
    );
    assert!(text.contains("bash_exit=0"), "{text}");
    assert!(
        text.contains("search") && text.contains("source=python") && text.contains("status=error"),
        "{text}"
    );
    assert!(text.contains("truncated=false"), "{text}");
    // 人类输出不打印完整 args/result（bash command / python query 原文）。
    assert!(
        !text.contains("echo hi"),
        "human 输出不得打印完整 args: {text}"
    );
    assert!(!text.contains("\"query\":\"q\""), "{text}");
    // 工具行位于事件时间线之后。
    let finished = text.find("RunFinished").expect("RunFinished row");
    let tools = text.find("tool_calls: 2").expect("tool section");
    assert!(finished < tools, "tool lines follow the event timeline");
}

#[test]
fn json_trace_appends_tool_call_ndjson_lines_in_deterministic_order() {
    let fx = fixture();
    let output = zgraph(&["trace", RUN_ID, "--db", path_str(fx.path()), "--json"]);
    assert_eq!(code(&output), 0, "stderr={}", stderr(&output));
    let lines = json_lines(&output);
    let kinds: Vec<&str> = lines
        .iter()
        .map(|line| line["kind"].as_str().expect("kind"))
        .collect();
    assert_eq!(
        kinds,
        vec![
            "RunStarted",
            "Streaming",
            "Streaming",
            "RunFinished",
            "ToolCall",
            "ToolCall",
        ],
        "保持纯 NDJSON：既有事件行后追加 ToolCall 行（不做顶层数组）"
    );
    // 排序：started_at 更早的 python 调用（t2）先输出；同 started_at 再按 call_id。
    let first = &lines[4];
    assert_eq!(first["kind"], json!("ToolCall"));
    assert_eq!(first["call_id"], json!("t2"));
    assert_eq!(first["tool_name"], json!("search"));
    assert_eq!(first["source"], json!("python"));
    assert_eq!(first["status"], json!("error"));
    assert_eq!(first["attempt"], json!(0));
    assert_eq!(first["args"], json!({"query": "q"}));
    assert_eq!(first["error"], json!({"error": {"reason": "boom"}}));
    assert_eq!(first["result"], Value::Null);
    assert_eq!(first["truncated"], json!(false));
    assert_eq!(first["duration_ms"], json!(3));
    assert_eq!(first["side_effect_path"], json!("out/result.txt"));
    assert_eq!(first["side_effect_bytes_after"], json!(5));

    let second = &lines[5];
    assert_eq!(second["call_id"], json!("t1"));
    assert_eq!(second["tool_name"], json!("builtin__bash"));
    assert_eq!(second["bash_exit_code"], json!(0));
    assert_eq!(second["bash_command"], json!("echo hi"));
    assert_eq!(second["bash_cwd"], json!("/workspace"));
    assert_eq!(second["bash_signal"], Value::Null);
}

#[test]
fn stats_adds_tool_calls_count_only_when_present() {
    let fx = fixture();
    let db = path_str(fx.path());
    let human = zgraph(&["trace", RUN_ID, "--db", db, "--stats"]);
    assert_eq!(code(&human), 0, "{}", stderr(&human));
    assert!(
        stdout(&human).contains("tool_calls 2"),
        "{}",
        stdout(&human)
    );

    let json = zgraph(&["trace", RUN_ID, "--db", db, "--stats", "--json"]);
    assert_eq!(code(&json), 0, "{}", stderr(&json));
    let value: Value = serde_json::from_str(stdout(&json).trim()).expect("stats json");
    assert_eq!(value["tool_calls"], json!(2));

    // 无工具调用的 run：默认输出与 v6 一致（不出现该行/键）。
    let plain = fixture_without_tools();
    let plain_db = path_str(plain.path());
    let human = zgraph(&["trace", RUN_ID, "--db", plain_db, "--stats"]);
    assert!(!stdout(&human).contains("tool_calls"), "{}", stdout(&human));
    let json = zgraph(&["trace", RUN_ID, "--db", plain_db, "--stats", "--json"]);
    let value: Value = serde_json::from_str(stdout(&json).trim()).expect("stats json");
    assert!(value.get("tool_calls").is_none(), "{value}");
}

#[test]
fn no_tool_call_outputs_are_unchanged() {
    let fx = fixture_without_tools();
    let db = path_str(fx.path());
    let human = zgraph(&["trace", RUN_ID, "--db", db]);
    assert_eq!(code(&human), 0, "{}", stderr(&human));
    assert!(!stdout(&human).contains("tool_calls"), "{}", stdout(&human));

    let json = zgraph(&["trace", RUN_ID, "--db", db, "--json"]);
    let lines = json_lines(&json);
    assert!(
        lines.iter().all(|line| line["kind"] != json!("ToolCall")),
        "{lines:?}"
    );
}

#[test]
fn events_mode_replay_is_byte_identical_to_trace_json_with_tool_rows() {
    let fx = fixture();
    let db = path_str(fx.path());
    let trace = zgraph(&["trace", RUN_ID, "--db", db, "--json"]);
    let replay = zgraph(&[
        "replay",
        "--run",
        RUN_ID,
        "--mode=events",
        "--json",
        "--db",
        db,
    ]);
    assert_eq!(code(&trace), 0, "{}", stderr(&trace));
    assert_eq!(code(&replay), 0, "{}", stderr(&replay));
    assert_eq!(stdout(&trace), stdout(&replay));
    assert!(stdout(&trace).contains("\"kind\":\"ToolCall\""));
}

// ---------------------------------------------------------------------------
// replay --mode=tools（DR-014 §4/§7；recorded-only）
// ---------------------------------------------------------------------------

fn replay_tools(fx: &Fixture, extra: &[&str]) -> Output {
    let mut args = vec![
        "replay",
        "--run",
        RUN_ID,
        "--mode=tools",
        "--db",
        path_str(fx.path()),
    ];
    args.extend(extra.iter().copied());
    zgraph(&args)
}

#[test]
fn replay_tools_json_restores_recorded_calls_in_order() {
    let fx = fixture();
    let output = replay_tools(&fx, &["--json"]);
    assert_eq!(code(&output), 0, "stderr={}", stderr(&output));
    let lines = json_lines(&output);
    assert_eq!(lines.len(), 2, "one NDJSON line per recorded call");
    assert_eq!(lines[0]["call_id"], json!("t2"));
    assert_eq!(lines[1]["call_id"], json!("t1"));
    assert_eq!(lines[0]["args"], json!({"query": "q"}));
    assert_eq!(lines[1]["bash_exit_code"], json!(0));
}

#[test]
fn replay_tools_human_outputs_header_and_lines() {
    let fx = fixture();
    let output = replay_tools(&fx, &[]);
    assert_eq!(code(&output), 0, "stderr={}", stderr(&output));
    let text = stdout(&output);
    assert!(text.contains(&format!("run {RUN_ID}")), "{text}");
    assert!(
        text.contains("tool_calls: 2 (recorded; no tools executed)"),
        "{text}"
    );
    assert!(
        text.contains("builtin__bash") && text.contains("bash_exit=0"),
        "{text}"
    );
    assert!(
        !text.contains("echo hi"),
        "human 输出不得打印完整 args: {text}"
    );
}

#[test]
fn replay_tools_explicit_recorded_option_is_accepted() {
    let fx = fixture();
    let output = replay_tools(&fx, &["--replay-tools=recorded", "--json"]);
    assert_eq!(code(&output), 0, "stderr={}", stderr(&output));
    assert_eq!(json_lines(&output).len(), 2);
}

#[test]
fn replay_tools_missing_record_is_fail_closed() {
    let fx = fixture();
    db_connection(fx.path())
        .execute("DELETE FROM tool_calls WHERE call_id = 't1'", [])
        .expect("delete record");
    let output = replay_tools(&fx, &["--json"]);
    assert_eq!(code(&output), 3, "stderr={}", stderr(&output));
    assert!(
        stderr(&output).contains("REPLAY_TOOL_RECORD_MISSING"),
        "{}",
        stderr(&output)
    );
    assert!(stdout(&output).is_empty(), "{}", stdout(&output));
}

#[test]
fn replay_tools_truncated_record_is_fail_closed() {
    let fx = fixture();
    db_connection(fx.path())
        .execute(
            "UPDATE tool_calls SET truncated = 1 WHERE call_id = 't1'",
            [],
        )
        .expect("mark truncated");
    let output = replay_tools(&fx, &[]);
    assert_eq!(code(&output), 3, "stderr={}", stderr(&output));
    assert!(
        stderr(&output).contains("REPLAY_TOOL_RECORD_TRUNCATED"),
        "{}",
        stderr(&output)
    );
}

#[test]
fn replay_tools_sha_or_json_mismatch_is_fail_closed() {
    // sha 篡改 → MISMATCH。
    let fx = fixture();
    db_connection(fx.path())
        .execute(
            "UPDATE tool_calls SET args_sha256 = 'bad' WHERE call_id = 't1'",
            [],
        )
        .expect("tamper sha");
    let output = replay_tools(&fx, &[]);
    assert_eq!(code(&output), 3, "stderr={}", stderr(&output));
    assert!(
        stderr(&output).contains("REPLAY_TOOL_RECORD_MISMATCH"),
        "{}",
        stderr(&output)
    );

    // 非法 JSON → MISMATCH。
    let fx = fixture();
    db_connection(fx.path())
        .execute(
            "UPDATE tool_calls SET args_json = '{not json' WHERE call_id = 't2'",
            [],
        )
        .expect("corrupt json");
    let output = replay_tools(&fx, &[]);
    assert_eq!(code(&output), 3, "stderr={}", stderr(&output));
    assert!(
        stderr(&output).contains("REPLAY_TOOL_RECORD_MISMATCH"),
        "{}",
        stderr(&output)
    );
}

#[test]
fn replay_tools_reexec_is_usage_error_not_implemented() {
    let fx = fixture();
    let output = replay_tools(&fx, &["--replay-tools=reexec"]);
    assert_eq!(code(&output), 2, "stderr={}", stderr(&output));
    let text = stderr(&output);
    assert!(text.contains("reexec"), "{text}");
    assert!(
        text.contains("not implemented in v0.4") || text.contains("v0.4"),
        "must state v0.4 not implemented: {text}"
    );
}

#[test]
fn replay_tools_has_no_side_effects() {
    let fx = fixture();
    let db = fx.path();

    // 只读探针：DB 主文件字节 + 行数 + workspace 快照（模拟"不执行工具/不改文件"）。
    let db_before = std::fs::read(db).expect("read db before");
    let tool_calls_before = tool_call_count(db);
    let workspace = tempfile::tempdir().expect("workspace");
    let note = workspace.path().join("note.txt");
    std::fs::write(&note, "do not touch").expect("write note");
    let workspace_before = snapshot_dir(workspace.path());
    assert!(
        !db.with_extension("db-wal").exists(),
        "fixture has no lingering wal"
    );

    // `ZGRAPH_PYTHON=/nonexistent`：若回放尝试 spawn 工具/worker 必然失败。
    let output = zgraph_env(
        &[
            "replay",
            "--run",
            RUN_ID,
            "--mode=tools",
            "--json",
            "--db",
            path_str(db),
        ],
        &[("ZGRAPH_PYTHON", "/nonexistent/python")],
    );
    assert_eq!(code(&output), 0, "stderr={}", stderr(&output));
    assert_eq!(json_lines(&output).len(), 2);

    assert_eq!(
        std::fs::read(db).expect("read db after"),
        db_before,
        "只读：DB 主文件不变"
    );
    assert_eq!(
        tool_call_count(db),
        tool_calls_before,
        "tool_calls 行数不变"
    );
    assert_eq!(
        snapshot_dir(workspace.path()),
        workspace_before,
        "workspace 不变"
    );
    assert!(!db.with_extension("db-wal").exists(), "只读回放不创建 -wal");
}

/// 目录快照（递归路径 + 文件内容；不依赖 mtime，避免只读探针假阳性）。
fn snapshot_dir(root: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    let mut entries = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let mut children: Vec<PathBuf> = std::fs::read_dir(&dir)
            .expect("read dir")
            .map(|entry| entry.expect("dir entry").path())
            .collect();
        children.sort();
        for child in children {
            if child.is_dir() {
                stack.push(child);
            } else {
                let bytes = std::fs::read(&child).expect("read file");
                entries.push((child, bytes));
            }
        }
    }
    entries.sort();
    entries
}
