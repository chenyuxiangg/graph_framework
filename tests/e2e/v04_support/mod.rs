//! v0.4（M9）e2e 公共辅助：内置工具（AC-21/22/23）/ trace v7 工具审计与回放（AC-24）。
//!
//! 语义依据：DR-012/013/014、spec 07 AC-21–24、05 §5.15、06 §6.15。
//! 数据通路（禁止 mock）：真 `zgraph` 二进制 + 真 fake LLM provider（`--fake-llm`）+
//! 真 Rust 内置工具执行 + 真 SQLite trace v7；bash 的真实隔离由用户普通终端
//! `crates/zgraph-harness/scripts/verify-sandbox.sh` 作证据（嵌套环境 fail-closed）。
//!
//! 复用 [`crate::v03_support`] 的资产/脚本/CLI/trace 基础能力，本模块只补 v0.4 专用
//! 辅助（workspace 树快照、tool_calls 查询、v7 trace/replay 命令、v6 降级构造等）。

#![allow(dead_code)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Output;

use rusqlite::Connection;
use serde_json::{json, Value};

use crate::cli_support;
use crate::v03;

// ---------------------------------------------------------------------------
// 图 / 资产 / 运行
// ---------------------------------------------------------------------------

/// v0.4 单 agent 图（`start → agent_call → end`，AC-21–24 共用）。
pub fn graph() -> PathBuf {
    v03::graph("agent_one.json")
}

/// 创建临时 workspace 目录（`<dir>/ws`），返回路径。
pub fn workspace(dir: &Path) -> PathBuf {
    let root = dir.join("ws");
    std::fs::create_dir_all(&root).expect("create workspace");
    root
}

/// 创建临时 allow-path 只读白名单目录（`<dir>/allow`），返回路径。
pub fn allow_dir(dir: &Path) -> PathBuf {
    let root = dir.join("allow");
    std::fs::create_dir_all(&root).expect("create allow-path dir");
    root
}

/// 写文件（自动创建父目录）。
pub fn write_file(path: &Path, bytes: &[u8]) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("create parent");
    }
    std::fs::write(path, bytes).expect("write fixture file");
}

/// 写 UTF-8 文件。
pub fn write_text(path: &Path, text: &str) {
    write_file(path, text.as_bytes());
}

/// 读文件文本。
pub fn read_text(path: &Path) -> String {
    std::fs::read_to_string(path).expect("read file")
}

/// 内置工具 AgentConfig（`tools` 顺序 = LLM schema 顺序）。
pub fn agent_with_tools(tools: &[&str]) -> Value {
    let mut value = v03::agent("parser", tools);
    // v0.4 e2e 场景单 run 常含 >8 次工具调用；迭代上限放宽（不影响工具语义）。
    value["max_iterations"] = json!(32);
    value
}

/// 文件/搜索类 opt-in CLI flags（`--allow-builtin-tools` + workspace + 可选 allow-path）。
pub fn builtin_flags(workspace: &Path, allow_paths: &[PathBuf]) -> Vec<String> {
    let mut flags = vec![
        "--allow-builtin-tools".to_string(),
        "--workspace-dir".to_string(),
        cli_support::arg(workspace),
    ];
    for path in allow_paths {
        flags.push("--allow-path".to_string());
        flags.push(cli_support::arg(path));
    }
    flags
}

/// 在 `base` 之后追加字符串 flags。
pub fn with_flags(mut base: Vec<String>, extra: &[&str]) -> Vec<String> {
    base.extend(extra.iter().map(|flag| flag.to_string()));
    base
}

/// 一次 v0.4 run 的产物。
pub struct RunOutcome {
    /// CLI 输出。
    pub output: Output,
    /// trace DB。
    pub db: PathBuf,
    /// 资产根。
    pub assets: PathBuf,
    /// fake LLM 脚本。
    pub script: PathBuf,
}

/// 构造资产 + fake LLM 并执行 `agent_one.json`（`extra` 为 CLI flags）。
pub fn run_scenario(dir: &Path, agent_value: &Value, calls: Value, extra: &[String]) -> RunOutcome {
    let assets = v03::assets(dir);
    v03::write_agent(&assets, agent_value);
    let script = v03::fake_llm(&assets, calls);
    let db = dir.join("trace.db");
    let output = v03::run(dir, &db, "agent_one.json", &assets, &script, extra);
    RunOutcome {
        output,
        db,
        assets,
        script,
    }
}

/// 便捷：单 agent 工具场景。
pub fn run_tools_scenario(
    dir: &Path,
    tools: &[&str],
    calls: Value,
    extra: &[String],
) -> RunOutcome {
    run_scenario(dir, &agent_with_tools(tools), calls, extra)
}

/// 标准成功 run 的 `run_id`（失败时带 stdout/stderr panic）。
pub fn completed_run_id(outcome: &RunOutcome, context: &str) -> String {
    let payload = v03::payload(&outcome.output);
    assert_eq!(
        payload["status"],
        json!("Completed"),
        "{context}: payload={payload} stderr={}",
        v03::err(&outcome.output)
    );
    payload["run_id"]
        .as_str()
        .expect("completed run has run_id")
        .to_string()
}

/// 单一文本回复的 fake LLM calls。
pub fn text_reply(content: &str) -> Value {
    json!([{"kind": "text", "content": content, "stop_reason": "EndTurn",
           "usage": {"prompt_tokens": 10, "completion_tokens": 2}}])
}

/// 构造一次工具调用（`call_id` 原生；args 任意 JSON）。
pub fn tool_call(call_id: &str, tool: &str, args: Value) -> Value {
    json!({"kind": "tool_calls", "content": null,
           "tool_calls": [{"call_id": call_id, "name": tool, "args": args}]})
}

/// 多工具调用 + 结尾文本脚本。
pub fn calls_with_text(tool_calls: Vec<Value>) -> Value {
    let mut calls = tool_calls;
    calls.push(
        json!({"kind": "text", "content": "done", "stop_reason": "EndTurn",
                      "usage": {"prompt_tokens": 1, "completion_tokens": 1}}),
    );
    Value::Array(calls)
}

// ---------------------------------------------------------------------------
// trace v7 `tool_calls` 查询
// ---------------------------------------------------------------------------

/// `tool_calls` 一行（v7；AC-24 字段断言用）。
#[derive(Debug, Clone)]
pub struct ToolRow {
    pub tool_call_id: i64,
    pub run_id: String,
    pub instance_id: String,
    pub node_id: String,
    pub attempt: i64,
    pub call_id: String,
    pub tool_name: String,
    pub source: String,
    pub status: String,
    pub args: Value,
    pub result: Option<Value>,
    pub error: Option<Value>,
    pub truncated: bool,
    pub args_sha256: Option<String>,
    pub result_sha256: Option<String>,
    pub error_sha256: Option<String>,
    pub duration_ms: i64,
    pub side_effect_path: Option<String>,
    pub side_effect_before: Option<String>,
    pub side_effect_after: Option<String>,
    pub side_effect_bytes_before: Option<i64>,
    pub side_effect_bytes_after: Option<i64>,
    pub bash_command: Option<String>,
    pub bash_cwd: Option<String>,
    pub bash_exit_code: Option<i64>,
    pub bash_stdout: Option<String>,
    pub bash_stderr: Option<String>,
    pub bash_truncated: bool,
    pub sandbox_profile_sha256: Option<String>,
    pub raw_args_json: String,
    pub raw_result_json: Option<String>,
}

/// 解析 JSON 列。
fn parse_json_column(raw: &Option<String>) -> Option<Value> {
    raw.as_deref().map(|text| {
        serde_json::from_str(text).unwrap_or_else(|error| panic!("tool_calls JSON column: {error}"))
    })
}

/// 读取指定 run 的 `tool_calls`（按 `tool_call_id` 升序）。
pub fn tool_rows(db: &Path, run_id: &str) -> Vec<ToolRow> {
    let conn = cli_support::read_only_connect(db);
    let mut statement = conn
        .prepare(
            "SELECT tool_call_id, run_id, instance_id, node_id, attempt, call_id, tool_name, \
                    source, status, args_json, result_json, error_json, truncated, args_sha256, \
                    result_sha256, error_sha256, duration_ms, bash_signal, side_effect_path, \
                    side_effect_before, side_effect_after, side_effect_bytes_before, \
                    side_effect_bytes_after, bash_command, bash_cwd, bash_exit_code, \
                    bash_stdout, bash_stderr, bash_truncated, sandbox_profile_sha256 \
             FROM tool_calls WHERE run_id = ?1 ORDER BY tool_call_id",
        )
        .expect("prepare tool_calls");
    statement
        .query_map([run_id], |row| {
            let raw_args_json: String = row.get(9)?;
            let raw_result_json: Option<String> = row.get(10)?;
            let raw_error_json: Option<String> = row.get(11)?;
            Ok(ToolRow {
                tool_call_id: row.get(0)?,
                run_id: row.get(1)?,
                instance_id: row.get(2)?,
                node_id: row.get(3)?,
                attempt: row.get(4)?,
                call_id: row.get(5)?,
                tool_name: row.get(6)?,
                source: row.get(7)?,
                status: row.get(8)?,
                args: serde_json::from_str(&raw_args_json).unwrap_or(Value::Null),
                result: parse_json_column(&raw_result_json),
                error: parse_json_column(&raw_error_json),
                truncated: row.get::<_, i64>(12)? != 0,
                args_sha256: row.get(13)?,
                result_sha256: row.get(14)?,
                error_sha256: row.get(15)?,
                duration_ms: row.get(16)?,
                side_effect_path: row.get(18)?,
                side_effect_before: row.get(19)?,
                side_effect_after: row.get(20)?,
                side_effect_bytes_before: row.get(21)?,
                side_effect_bytes_after: row.get(22)?,
                bash_command: row.get(23)?,
                bash_cwd: row.get(24)?,
                bash_exit_code: row.get(25)?,
                bash_stdout: row.get(26)?,
                bash_stderr: row.get(27)?,
                bash_truncated: row.get::<_, i64>(28)? != 0,
                sandbox_profile_sha256: row.get(29)?,
                raw_args_json,
                raw_result_json,
            })
        })
        .expect("query tool_calls")
        .map(|row| row.expect("tool_calls row"))
        .collect()
}

/// `tool_calls` 行数。
pub fn tool_call_count(db: &Path, run_id: &str) -> i64 {
    cli_support::read_only_connect(db)
        .query_row(
            "SELECT COUNT(*) FROM tool_calls WHERE run_id = ?1",
            [run_id],
            |row| row.get(0),
        )
        .expect("count tool_calls")
}

/// 按 `call_id` 取一行（不存在 → panic）。
pub fn tool_row(db: &Path, run_id: &str, call_id: &str) -> ToolRow {
    tool_rows(db, run_id)
        .into_iter()
        .find(|row| row.call_id == call_id)
        .unwrap_or_else(|| panic!("tool_calls row {call_id} not found"))
}

/// trace 的 schema 版本。
pub fn user_version(db: &Path) -> i64 {
    cli_support::read_only_connect(db)
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .expect("user_version")
}

/// 某表是否存在。
pub fn table_exists(db: &Path, table: &str) -> bool {
    cli_support::read_only_connect(db)
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name = ?1",
            [table],
            |row| row.get::<_, i64>(0),
        )
        .expect("sqlite_master")
        > 0
}

/// 可写连接（构造 v6/损坏记录等测试前置）。
pub fn writable_connect(db: &Path) -> Connection {
    let conn = Connection::open(db).expect("open writable trace db");
    conn.busy_timeout(std::time::Duration::from_secs(5))
        .expect("busy_timeout");
    conn
}

/// 把当前 v7 库降级为**真实 v6 形状**（删 `tool_calls`、删 v7 审计行、`user_version=6`）。
pub fn downgrade_v7_to_v6(db: &Path) {
    let conn = writable_connect(db);
    conn.execute_batch(
        "DROP TABLE IF EXISTS tool_calls;
         DELETE FROM schema_migrations WHERE version = 7;
         PRAGMA user_version = 6;",
    )
    .expect("downgrade v7 → v6");
}

/// 目录下的 `*.bak.v6-*` 备份文件。
pub fn v6_backups(dir: &Path) -> Vec<PathBuf> {
    let mut backups: Vec<PathBuf> = std::fs::read_dir(dir)
        .expect("read backup dir")
        .filter_map(|entry| {
            let name = entry.expect("backup entry").file_name();
            let text = name.to_string_lossy();
            text.contains(".bak.v6-").then(|| dir.join(name))
        })
        .collect();
    backups.sort();
    backups
}

// ---------------------------------------------------------------------------
// trace / replay 命令（v0.4 additive）
// ---------------------------------------------------------------------------

/// `zgraph trace <run> --db <db> [extra]`。
pub fn trace_extra(db: &Path, run_id: &str, extra: &[&str]) -> Output {
    let mut args = vec![
        "trace".to_string(),
        run_id.to_string(),
        "--db".to_string(),
        cli_support::arg(db),
    ];
    args.extend(extra.iter().map(|flag| flag.to_string()));
    cli_support::run_cli(&args, &cli_support::repo_root())
}

/// `trace --json`。
pub fn trace_json(db: &Path, run_id: &str) -> Output {
    trace_extra(db, run_id, &["--json"])
}

/// `trace --stats`（人类可读）。
pub fn trace_stats(db: &Path, run_id: &str) -> Output {
    trace_extra(db, run_id, &["--stats"])
}

/// `trace --stats --json`。
pub fn trace_stats_json(db: &Path, run_id: &str) -> Output {
    trace_extra(db, run_id, &["--stats", "--json"])
}

/// 断言退出码 0（带 stderr 摘要）。
pub fn assert_exit_ok(output: &Output, context: &str) {
    assert!(
        output.status.success(),
        "{context}: exit={:?}, stderr={}",
        output.status.code(),
        v03::err(output)
    );
}

/// `trace --json` 中 `kind == "ToolCall"` 的行（v7 additive）。
pub fn trace_tool_call_lines(db: &Path, run_id: &str) -> Vec<Value> {
    let output = trace_json(db, run_id);
    assert_exit_ok(&output, "trace --json");
    v03::ndjson(&output)
        .into_iter()
        .filter(|line| line["kind"] == json!("ToolCall"))
        .collect()
}

/// `zgraph replay --run <run> --db <db> --mode <mode> [extra]`。
pub fn replay_mode(db: &Path, run_id: &str, mode: &str, extra: &[&str]) -> Output {
    let mut args = vec![
        "replay".to_string(),
        "--run".to_string(),
        run_id.to_string(),
        "--db".to_string(),
        cli_support::arg(db),
        "--mode".to_string(),
        mode.to_string(),
    ];
    args.extend(extra.iter().map(|flag| flag.to_string()));
    cli_support::run_cli(&args, &cli_support::repo_root())
}

/// `replay --mode=tools --json`。
pub fn replay_tools_json(db: &Path, run_id: &str) -> Output {
    replay_mode(db, run_id, "tools", &["--json"])
}

/// `replay --mode=tools`（人类可读）。
pub fn replay_tools_human(db: &Path, run_id: &str) -> Output {
    replay_mode(db, run_id, "tools", &[])
}

/// `replay --mode=tools --json` 的 NDJSON 行。
pub fn replay_tool_call_lines(db: &Path, run_id: &str) -> Vec<Value> {
    let output = replay_tools_json(db, run_id);
    assert_exit_ok(&output, "replay --mode=tools --json");
    v03::ndjson(&output)
}

/// `replay --mode=events --json`（L1；应与 `trace --json` 逐字节一致）。
pub fn replay_events_json(db: &Path, run_id: &str) -> Output {
    replay_mode(db, run_id, "events", &["--json"])
}

/// `replay --mode=outputs --json`（L2）。
pub fn replay_outputs_json(db: &Path, run_id: &str) -> Output {
    replay_mode(db, run_id, "outputs", &["--json"])
}

// ---------------------------------------------------------------------------
// workspace / DB 快照（AC-24 g 无副作用证明）
// ---------------------------------------------------------------------------

/// workspace 树快照：相对路径 → 文件字节（含隐藏文件；不含目录项）。
pub fn tree_snapshot(root: &Path) -> BTreeMap<String, Vec<u8>> {
    let mut snapshot = BTreeMap::new();
    collect_tree(root, root, &mut snapshot);
    snapshot
}

/// 递归收集（确定性顺序）。
fn collect_tree(root: &Path, dir: &Path, out: &mut BTreeMap<String, Vec<u8>>) {
    let mut entries: Vec<_> = std::fs::read_dir(dir)
        .expect("read tree dir")
        .map(|entry| entry.expect("tree entry").path())
        .collect();
    entries.sort();
    for path in entries {
        if path.is_dir() {
            collect_tree(root, &path, out);
        } else {
            let relative = path
                .strip_prefix(root)
                .expect("relative path")
                .to_string_lossy()
                .into_owned();
            out.insert(relative, std::fs::read(&path).expect("read tree file"));
        }
    }
}

/// DB 文件快照（db/-wal/-shm 字节；无副作用断言用）。
pub fn db_snapshot(db: &Path) -> Vec<(String, Option<Vec<u8>>)> {
    cli_support::db_files_snapshot(db)
}

// ---------------------------------------------------------------------------
// sandbox 环境探测（AC-23 环境感知）
// ---------------------------------------------------------------------------

/// `sandbox-exec` 最小 profile 探测的真实退出码；二进制不存在/无法执行 → `None`。
///
/// 嵌套 Seatbelt（本 CI/agent 环境）返回 **71**；真实普通终端返回 **0**。
pub fn sandbox_probe_exit() -> Option<i32> {
    let output = std::process::Command::new("/usr/bin/sandbox-exec")
        .args(["-p", "(version 1)(deny default)", "/usr/bin/true"])
        .output()
        .ok()?;
    output.status.code()
}

/// 当前环境是否嵌套 Seatbelt（`sandbox-exec` exit 71）。
pub fn sandbox_is_nested() -> bool {
    sandbox_probe_exit() == Some(71)
}
