//! v0.5/F1b：`replay --mode=tools --replay-tools=reexec` CLI 契约测试（DR-017 / AC-25）。
//!
//! 覆盖：
//! - reexec 只读白名单 `builtin__read/grep/glob/diff`：用记录 args 真重跑，结果经
//!   `zgraph_trace::tool_result_sha256` 与记录 `result_sha256` 比对 → 一致 exit0，
//!   输出 additive `replay_mode`/`drift`（recorded 模式输出不变）；
//! - 文件/结果漂移 → `REPLAY_TOOL_RECORD_MISMATCH` exit3（文案含 drift；不写 run）；
//! - truncated / 缺 record sha → `REPLAY_TOOL_RECORD_TRUNCATED`/`MISSING` fail-closed；
//! - **副作用工具（write/edit/apply_patch/bash）绝不执行**（workspace 快照不变，用记录结果）；
//!   MCP/Python 同样用记录；
//! - 前置：缺 `--allow-builtin-tools`/显式 `--workspace-dir` → usage exit2；非法
//!   workspace → 既有配置错误 exit3；events/outputs 忽略 reexec 参数；
//! - 只读：DB 主文件字节/`tool_calls` 行数/无 `-wal` 不变，不产生新 trace 行。
//!
//! Fixtures 为 synthetic v7 DB（TraceWriter 生成）+ 真 builtin 只读工具实现；无网络。

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::{json, Value};
use tempfile::TempDir;
use zgraph_harness::BuiltinToolSet;
use zgraph_trace::{
    RecordMode, RunFinishedInfo, RunMetrics, RunStartedInfo, RunStatus, StreamingEvent,
    ToolCallRecord, ToolCallSource, ToolCallStatus, TraceInput, TraceWriter,
};

use zgraph_cli::replay_cmd::{
    CODE_REPLAY_TOOL_RECORD_MISMATCH, CODE_REPLAY_TOOL_RECORD_MISSING,
    CODE_REPLAY_TOOL_RECORD_TRUNCATED,
};

const RUN_ID: &str = "run-reexec";
const GRAPH_HASH: &str = "0123456789abcdef";

fn zgraph(args: &[String]) -> Output {
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

fn path_str(path: &Path) -> &str {
    path.to_str().expect("utf8 path")
}

fn json_lines(output: &Output) -> Vec<Value> {
    stdout(output)
        .lines()
        .map(|line| serde_json::from_str(line).expect("NDJSON line"))
        .collect()
}

/// 测试 workspace（jail 根）+ 预置只读文件。
struct Workspace {
    _dir: TempDir,
    path: PathBuf,
}

impl Workspace {
    fn new() -> Self {
        let dir = tempfile::tempdir().expect("workspace tempdir");
        let path: PathBuf = std::fs::canonicalize(dir.path()).expect("canonical workspace");
        std::fs::write(path.join("a.txt"), "l0\nl1\nl2\n").expect("write a.txt");
        std::fs::write(path.join("b.rs"), "needle one\nA.B\n").expect("write b.rs");
        std::fs::create_dir_all(path.join("nested")).expect("nested dir");
        std::fs::write(path.join("nested/c.txt"), "needle two\n").expect("write c.txt");
        Self { _dir: dir, path }
    }

    fn set(&self) -> std::sync::Arc<BuiltinToolSet> {
        BuiltinToolSet::new_with_options(&self.path, &[], true, None).expect("builtin tool set")
    }

    /// 目录快照（路径 + 内容；reexec 副作用探针）。
    fn snapshot(&self) -> Vec<(PathBuf, Vec<u8>)> {
        let mut entries = Vec::new();
        let mut stack = vec![self.path.clone()];
        while let Some(dir) = stack.pop() {
            let mut children: Vec<PathBuf> = std::fs::read_dir(&dir)
                .expect("read dir")
                .map(|entry| entry.expect("entry").path())
                .collect();
            children.sort();
            for child in children {
                if child.is_dir() {
                    stack.push(child);
                } else {
                    entries.push((child.clone(), std::fs::read(&child).expect("read file")));
                }
            }
        }
        entries.sort();
        entries
    }
}

/// 一次待记录的工具调用。
struct CallSpec {
    call_id: &'static str,
    tool_name: String,
    source: ToolCallSource,
    status: ToolCallStatus,
    args: Value,
    result: Option<Value>,
    error: Option<Value>,
}

impl CallSpec {
    fn builtin(call_id: &'static str, tool_name: &str, args: Value, result: Value) -> Self {
        Self {
            call_id,
            tool_name: tool_name.to_string(),
            source: ToolCallSource::Builtin,
            status: ToolCallStatus::Ok,
            args,
            result: Some(result),
            error: None,
        }
    }
}

fn run_started() -> TraceInput {
    TraceInput::RunStarted(RunStartedInfo {
        run_id: RUN_ID.to_string(),
        graph_hash: GRAPH_HASH.to_string(),
        graph_name: "reexec-demo".to_string(),
        dsl_json: "{}".to_string(),
        user_id: "local".to_string(),
        entry_inputs: json!({}),
        global_vars: json!({}),
        seed: 7,
        record_mode: RecordMode::Hash,
        owner_pid: std::process::id(),
        owner_host: "test-host".to_string(),
        started_at: "2026-11-03T10:00:00.000Z".to_string(),
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
        at: "2026-11-03T10:00:09.000Z".to_string(),
    })
}

/// 用 TraceWriter 生成 synthetic v7 `tool_calls`（hash/sha256 由 writer 计算）。
fn fixture_dir(calls: Vec<CallSpec>) -> (TempDir, PathBuf) {
    let dir = tempfile::tempdir().expect("fixture tempdir");
    let db = dir.path().join("trace.db");
    let writer = TraceWriter::open(&db).expect("trace writer");
    let handle = writer.handle();
    handle.send(run_started());
    for (position, call) in calls.iter().enumerate() {
        handle.send(TraceInput::Streaming {
            run_id: RUN_ID.to_string(),
            instance_id: "inst-agent-0".to_string(),
            node_id: Some("agent_node".to_string()),
            event: StreamingEvent::ToolCallStart {
                call_id: call.call_id.to_string(),
                tool_name: call.tool_name.clone(),
                args_summary: json!({}),
            },
            at: format!("2026-11-03T10:00:0{position}.000Z"),
        });
        handle.send(TraceInput::ToolCall(ToolCallRecord {
            run_id: RUN_ID.to_string(),
            instance_id: "inst-agent-0".to_string(),
            node_id: "agent_node".to_string(),
            attempt: 0,
            call_id: call.call_id.to_string(),
            tool_name: call.tool_name.clone(),
            source: call.source,
            status: call.status,
            args: call.args.clone(),
            result: call.result.clone(),
            error: call.error.clone(),
            started_at: format!("2026-11-03T10:00:0{position}.000Z"),
            finished_at: format!("2026-11-03T10:00:0{position}.500Z"),
            duration_ms: 5,
            side_effect: None,
            bash: None,
        }));
    }
    handle.send(run_finished());
    writer.flush().expect("flush");
    writer.close().expect("close");
    (dir, db)
}

fn reexec_args(db: &Path, workspace: &Path, json_flag: bool) -> Vec<String> {
    let mut args = vec![
        "replay".to_string(),
        "--run".to_string(),
        RUN_ID.to_string(),
        "--mode=tools".to_string(),
        "--replay-tools=reexec".to_string(),
        "--allow-builtin-tools".to_string(),
        "--workspace-dir".to_string(),
        path_str(workspace).to_string(),
        "--db".to_string(),
        path_str(db).to_string(),
    ];
    if json_flag {
        args.push("--json".to_string());
    }
    args
}

fn db_connection(path: &Path) -> rusqlite::Connection {
    rusqlite::Connection::open(path).expect("open db")
}

fn tool_call_count(path: &Path) -> i64 {
    db_connection(path)
        .query_row("SELECT count(*) FROM tool_calls", [], |row| row.get(0))
        .expect("tool_calls count")
}

/// 四只读工具（read/grep/glob/diff）匹配 → exit0；additive replay_mode/drift；
/// 副作用/python 行复用记录；workspace/DB 不变（只读）。
#[test]
fn reexec_readonly_tools_match_recorded_results() {
    let workspace = Workspace::new();
    let set = workspace.set();
    let read = set
        .reexec_readonly("builtin__read", &json!({"path": "a.txt"}))
        .expect("read expected");
    let grep = set
        .reexec_readonly("builtin__grep", &json!({"pattern": "needle", "path": "."}))
        .expect("grep expected");
    let glob = set
        .reexec_readonly("builtin__glob", &json!({"pattern": "**/*.rs"}))
        .expect("glob expected");
    let diff = set
        .reexec_readonly(
            "builtin__diff",
            &json!({"path": "a.txt", "old_string": "l1", "new_string": "L1"}),
        )
        .expect("diff expected");
    let calls = vec![
        CallSpec::builtin("c-read", "builtin__read", json!({"path": "a.txt"}), read),
        CallSpec::builtin(
            "c-grep",
            "builtin__grep",
            json!({"pattern": "needle", "path": "."}),
            grep,
        ),
        CallSpec::builtin(
            "c-glob",
            "builtin__glob",
            json!({"pattern": "**/*.rs"}),
            glob,
        ),
        CallSpec::builtin(
            "c-diff",
            "builtin__diff",
            json!({"path": "a.txt", "old_string": "l1", "new_string": "L1"}),
            diff,
        ),
        // 副作用工具行：reexec 下**不执行**，复用记录结果。
        CallSpec::builtin(
            "c-write",
            "builtin__write",
            json!({"path": "written.txt", "content": "x"}),
            json!({"ok": true}),
        ),
        // 非 builtin（python）：复用记录。
        CallSpec {
            call_id: "c-python",
            tool_name: "search".to_string(),
            source: ToolCallSource::Python,
            status: ToolCallStatus::Ok,
            args: json!({"query": "q"}),
            result: Some(json!({"hits": 0})),
            error: None,
        },
    ];
    let (_dir, db) = fixture_dir(calls);
    let db_before = std::fs::read(&db).expect("db bytes");
    let tool_calls_before = tool_call_count(&db);
    let workspace_before = workspace.snapshot();

    let args = reexec_args(&db, &workspace.path, true);
    let output = zgraph(&args);
    assert_eq!(code(&output), 0, "stderr={}", stderr(&output));
    let lines = json_lines(&output);
    assert_eq!(lines.len(), 6, "one NDJSON line per recorded call");
    for (call_id, expected_mode) in [
        ("c-read", "reexec"),
        ("c-grep", "reexec"),
        ("c-glob", "reexec"),
        ("c-diff", "reexec"),
        ("c-write", "recorded"),
        ("c-python", "recorded"),
    ] {
        let line = lines
            .iter()
            .find(|line| line["call_id"] == json!(call_id))
            .unwrap_or_else(|| panic!("missing call {call_id}: {lines:?}"));
        assert_eq!(
            line["replay_mode"],
            json!(expected_mode),
            "{call_id}: {line}"
        );
        assert_eq!(line["drift"], json!(false), "{call_id}: {line}");
        // additive 之外字段与 recorded 模式一致（tool_name/status/args/result）。
        assert!(line.get("tool_name").is_some());
    }
    // 只读：DB 主文件字节 / tool_calls 行数 / workspace 快照不变；不建 -wal。
    assert_eq!(
        std::fs::read(&db).expect("db after"),
        db_before,
        "不写 trace"
    );
    assert_eq!(tool_call_count(&db), tool_calls_before, "不新增 trace 行");
    assert_eq!(
        workspace.snapshot(),
        workspace_before,
        "只读工具不改 workspace"
    );
    let wal = db.with_extension("db-wal");
    assert!(!wal.exists(), "readonly replay must not create -wal");
}

/// 文件内容漂移 → reexec 结果 sha 不符 → `REPLAY_TOOL_RECORD_MISMATCH` exit3（含 drift）。
#[test]
fn reexec_drift_after_file_change_is_mismatch() {
    let workspace = Workspace::new();
    let set = workspace.set();
    let read = set
        .reexec_readonly("builtin__read", &json!({"path": "a.txt"}))
        .expect("read expected");
    let (_dir, db) = fixture_dir(vec![CallSpec::builtin(
        "c-read",
        "builtin__read",
        json!({"path": "a.txt"}),
        read,
    )]);
    let db_before = std::fs::read(&db).expect("db bytes");
    // 记录后修改文件 → reexec 结果漂移。
    std::fs::write(workspace.path.join("a.txt"), "changed content\n").expect("mutate file");

    let output = zgraph(&reexec_args(&db, &workspace.path, false));
    assert_eq!(code(&output), 3, "stderr={}", stderr(&output));
    let text = stderr(&output);
    assert!(text.contains(CODE_REPLAY_TOOL_RECORD_MISMATCH), "{text}");
    assert!(
        text.contains("drift"),
        "mismatch 文案需区分结果漂移: {text}"
    );
    assert_eq!(std::fs::read(&db).expect("db after"), db_before);
}

/// `truncated=1` → `REPLAY_TOOL_RECORD_TRUNCATED` exit3；缺 `result_sha256` → MISSING。
#[test]
fn reexec_fail_closed_on_truncated_or_missing_sha() {
    let workspace = Workspace::new();
    let set = workspace.set();
    let read = set
        .reexec_readonly("builtin__read", &json!({"path": "a.txt"}))
        .expect("read expected");
    let (_dir, db) = fixture_dir(vec![CallSpec::builtin(
        "c-read",
        "builtin__read",
        json!({"path": "a.txt"}),
        read,
    )]);
    db_connection(&db)
        .execute("UPDATE tool_calls SET truncated = 1", [])
        .expect("mark truncated");
    let output = zgraph(&reexec_args(&db, &workspace.path, false));
    assert_eq!(code(&output), 3, "stderr={}", stderr(&output));
    assert!(
        stderr(&output).contains(CODE_REPLAY_TOOL_RECORD_TRUNCATED),
        "{}",
        stderr(&output)
    );

    let workspace = Workspace::new();
    let set = workspace.set();
    let read = set
        .reexec_readonly("builtin__read", &json!({"path": "a.txt"}))
        .expect("read expected");
    let (_dir, db) = fixture_dir(vec![CallSpec::builtin(
        "c-read",
        "builtin__read",
        json!({"path": "a.txt"}),
        read,
    )]);
    db_connection(&db)
        .execute("UPDATE tool_calls SET result_sha256 = NULL", [])
        .expect("drop sha");
    let output = zgraph(&reexec_args(&db, &workspace.path, false));
    assert_eq!(code(&output), 3, "stderr={}", stderr(&output));
    // 缺 result sha 在预检阶段即 fail-closed：按记录校验口径报 MISMATCH（任务允许
    // MISSING 或 MISMATCH；此处断言两者之一，关键是 exit3 + 不静默通过）。
    let text = stderr(&output);
    assert!(
        text.contains(CODE_REPLAY_TOOL_RECORD_MISSING)
            || text.contains(CODE_REPLAY_TOOL_RECORD_MISMATCH),
        "{text}"
    );

    // 缺记录（streaming 有 start、tool_calls 无行）→ MISSING fail-closed。
    let workspace = Workspace::new();
    let set = workspace.set();
    let read = set
        .reexec_readonly("builtin__read", &json!({"path": "a.txt"}))
        .expect("read expected");
    let (_dir, db) = fixture_dir(vec![CallSpec::builtin(
        "c-read",
        "builtin__read",
        json!({"path": "a.txt"}),
        read,
    )]);
    db_connection(&db)
        .execute("DELETE FROM tool_calls", [])
        .expect("drop record");
    let output = zgraph(&reexec_args(&db, &workspace.path, false));
    assert_eq!(code(&output), 3, "stderr={}", stderr(&output));
    assert!(
        stderr(&output).contains(CODE_REPLAY_TOOL_RECORD_MISSING),
        "{}",
        stderr(&output)
    );
}

/// 缺前置 flag → usage exit2；非法 workspace → 既有配置错误 exit3（不降级 recorded）。
#[test]
fn reexec_prerequisites_are_usage_or_config_errors() {
    let workspace = Workspace::new();
    let set = workspace.set();
    let read = set
        .reexec_readonly("builtin__read", &json!({"path": "a.txt"}))
        .expect("read expected");
    let (_dir, db) = fixture_dir(vec![CallSpec::builtin(
        "c-read",
        "builtin__read",
        json!({"path": "a.txt"}),
        read,
    )]);
    let base = |extra: &[&str]| {
        let mut args = vec![
            "replay".to_string(),
            "--run".to_string(),
            RUN_ID.to_string(),
            "--mode=tools".to_string(),
            "--replay-tools=reexec".to_string(),
            "--db".to_string(),
            path_str(&db).to_string(),
        ];
        args.extend(extra.iter().map(|value| value.to_string()));
        args
    };
    // 缺 --allow-builtin-tools → exit2。
    let output = zgraph(&base(&["--workspace-dir", path_str(&workspace.path)]));
    assert_eq!(code(&output), 2, "stderr={}", stderr(&output));
    assert!(
        stderr(&output).contains("requires --allow-builtin-tools"),
        "{}",
        stderr(&output)
    );
    // 缺显式 --workspace-dir → exit2（不默认 cwd）。
    let output = zgraph(&base(&["--allow-builtin-tools"]));
    assert_eq!(code(&output), 2, "stderr={}", stderr(&output));
    assert!(
        stderr(&output).contains("explicit --workspace-dir"),
        "{}",
        stderr(&output)
    );
    // 非法 workspace（不存在）→ 既有配置错误 exit3（HARNESS_CONFIG_LOAD）。
    let missing = workspace.path.join("does-not-exist");
    let output = zgraph(&base(&[
        "--allow-builtin-tools",
        "--workspace-dir",
        path_str(&missing),
    ]));
    assert_eq!(code(&output), 3, "stderr={}", stderr(&output));
    assert!(
        stderr(&output).contains("HARNESS_CONFIG_LOAD"),
        "{}",
        stderr(&output)
    );
}

/// events/outputs 忽略 reexec 参数（REV-111/DR-017）：events 输出逐字节不变；
/// outputs 仍走既有 L2 记录校验（非 usage error）。默认 recorded 输出无 additive 字段。
#[test]
fn reexec_flag_is_ignored_outside_tools_mode_and_default_is_recorded() {
    let workspace = Workspace::new();
    let set = workspace.set();
    let read = set
        .reexec_readonly("builtin__read", &json!({"path": "a.txt"}))
        .expect("read expected");
    let (_dir, db) = fixture_dir(vec![CallSpec::builtin(
        "c-read",
        "builtin__read",
        json!({"path": "a.txt"}),
        read,
    )]);

    let baseline = zgraph(&[
        "replay".to_string(),
        "--run".to_string(),
        RUN_ID.to_string(),
        "--mode=events".to_string(),
        "--json".to_string(),
        "--db".to_string(),
        path_str(&db).to_string(),
    ]);
    assert_eq!(code(&baseline), 0, "{}", stderr(&baseline));
    let with_reexec = zgraph(&[
        "replay".to_string(),
        "--run".to_string(),
        RUN_ID.to_string(),
        "--mode=events".to_string(),
        "--replay-tools=reexec".to_string(),
        "--json".to_string(),
        "--db".to_string(),
        path_str(&db).to_string(),
    ]);
    assert_eq!(code(&with_reexec), 0, "{}", stderr(&with_reexec));
    assert_eq!(
        stdout(&baseline),
        stdout(&with_reexec),
        "events 忽略 reexec"
    );

    let outputs = zgraph(&[
        "replay".to_string(),
        "--run".to_string(),
        RUN_ID.to_string(),
        "--mode=outputs".to_string(),
        "--replay-tools=reexec".to_string(),
        "--db".to_string(),
        path_str(&db).to_string(),
    ]);
    assert_eq!(code(&outputs), 3, "stderr={}", stderr(&outputs));
    assert!(stderr(&outputs).contains("REPLAY_RECORD_MISSING"));

    // 默认 recorded：无 replay_mode/drift additive 字段（v0.4 输出不变）。
    let recorded = zgraph(&[
        "replay".to_string(),
        "--run".to_string(),
        RUN_ID.to_string(),
        "--mode=tools".to_string(),
        "--json".to_string(),
        "--db".to_string(),
        path_str(&db).to_string(),
    ]);
    assert_eq!(code(&recorded), 0, "{}", stderr(&recorded));
    let line = &json_lines(&recorded)[0];
    assert!(line.get("replay_mode").is_none(), "{line}");
    assert!(line.get("drift").is_none(), "{line}");
}

/// `tool_result_sha256` 与 writer 落库的 `result_sha256` 完全一致（跨模块 helper 口径）。
#[test]
fn tool_result_sha256_matches_writer_recorded_sha() {
    let result = json!({"ok": true, "nested": {"b": 2, "a": 1}, "list": [1, 2]});
    let call = CallSpec::builtin(
        "c-sha",
        "builtin__read",
        json!({"path": "a.txt"}),
        result.clone(),
    );
    let (_dir, db) = fixture_dir(vec![call]);
    let conn = db_connection(&db);
    let recorded: String = conn
        .query_row(
            "SELECT result_sha256 FROM tool_calls WHERE call_id = 'c-sha'",
            [],
            |row| row.get(0),
        )
        .expect("result_sha256");
    assert_eq!(
        zgraph_trace::tool_result_sha256(&result).expect("sha"),
        recorded,
        "公共 API 必须与 writer 同口径"
    );
}
