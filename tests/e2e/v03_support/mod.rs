//! v0.3（M8）e2e 公共辅助：**真 CLI + 真进程 + 真 SQLite** 的 skills/MCP/harness 插件链路。
//!
//! 提供：v0.3 图/Agent 资产临时构造、`--fake-llm` 脚本、`llm_calls`/`streaming_events`
//! 直接只读查询、`zgraph` 子进程（含 pid/kill）与 trace/replay 命令封装。
//!
//! 语义依据：DR-008/009/010/011、spec 07 AC-18/19/20、05 §5.14、06 §6.14。
//! 仅测试断言路径使用 SQL（与既有 e2e 同口径，不进入产品代码）。

#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::process::{Child, Output, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use crate::cli_support::{
    arg, assert_exit, json_stdout, python_program, read_only_connect, repo_root, run_cli,
    stderr_text,
};

/// v0.3 e2e 图入口输入（`start` 声明 `message`）。
pub const INPUTS: &str = r#"{"start":{"message":"hi"}}"#;

/// 轮询/子进程等待上限。
pub const WAIT_TIMEOUT: Duration = Duration::from_secs(30);

/// 轮询间隔。
pub const POLL_INTERVAL: Duration = Duration::from_millis(10);

// ---------------------------------------------------------------------------
// 路径
// ---------------------------------------------------------------------------

/// `tests/fixtures/graphs/v03/<name>`。
pub fn graph(name: &str) -> PathBuf {
    repo_root().join("tests/fixtures/graphs/v03").join(name)
}

/// `tests/e2e/fixtures/<rel>`。
pub fn fixture(rel: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures")
        .join(rel)
}

/// e2e fixture python 模块目录（`--python-path`）。
pub fn fixture_python_path() -> PathBuf {
    fixture("py")
}

// ---------------------------------------------------------------------------
// Agent 资产
// ---------------------------------------------------------------------------

/// 创建临时资产根（`agents/`、`tools/`），返回根路径。
pub fn assets(dir: &Path) -> PathBuf {
    let root = dir.join("assets");
    std::fs::create_dir_all(root.join("agents")).expect("agents dir");
    std::fs::create_dir_all(root.join("tools")).expect("tools dir");
    root
}

/// 基础 AgentConfig（与既有 e2e 资产同口径；`fake_mode` 不校验 api_key_env）。
pub fn agent(id: &str, tools: &[&str]) -> Value {
    json!({
        "id": id,
        "name": format!("v0.3 e2e {id}"),
        "schema_version": 1,
        "model": {
            "provider": "deepseek",
            "name": "deepseek-chat",
            "base_url": null,
            "api_key_env": "DEEPSEEK_API_KEY",
            "temperature": 0.0,
            "max_tokens": 512,
            "context_window_tokens": 65536
        },
        "role_prompt_file": "prompt.md",
        "constraints_file": null,
        "tools": tools,
        "input_mapping": {"message": "message"},
        "max_iterations": 8,
        "llm_timeout_ms": 60000,
        "llm_max_retries": 0,
        "node_timeout_ms": 30000
    })
}

/// 写入 Agent 资产（`agents/<id>.json` + `prompt.md`）。
pub fn write_agent(root: &Path, value: &Value) -> PathBuf {
    let id = value["id"].as_str().expect("agent id");
    let path = root.join("agents").join(format!("{id}.json"));
    std::fs::write(
        &path,
        serde_json::to_string_pretty(value).expect("agent json"),
    )
    .expect("write agent");
    // `role_prompt_file` 相对 **AgentConfig 文件目录**（`agents/`）解析。
    std::fs::write(root.join("agents/prompt.md"), "你是 v0.3 e2e agent。\n").expect("prompt.md");
    path
}

/// 写入 `<assets-dir>/mcp_servers.json`。
pub fn write_mcp_registry(root: &Path, value: &Value) -> PathBuf {
    let path = root.join("mcp_servers.json");
    std::fs::write(
        &path,
        serde_json::to_string_pretty(value).expect("registry json"),
    )
    .expect("write mcp registry");
    path
}

/// 写入 `--fake-llm` 脚本（`assets/fake_llm.json`），返回路径。
pub fn fake_llm(root: &Path, calls: Value) -> PathBuf {
    let script = json!({
        "version": 1,
        "chunk_chars": 3,
        "chunk_delay_ms": 0,
        "calls": calls,
        "failures": []
    });
    let path = root.join("fake_llm.json");
    std::fs::write(
        &path,
        serde_json::to_string_pretty(&script).expect("script json"),
    )
    .expect("write fake llm");
    path
}

// ---------------------------------------------------------------------------
// CLI
// ---------------------------------------------------------------------------

/// `zgraph run` 参数：真 fake LLM + 真 Python worker 环境 + 资产。
pub fn run_args(
    db: &Path,
    graph_path: &Path,
    assets_root: &Path,
    script: &Path,
    extra: &[String],
) -> Vec<String> {
    let mut args = vec![
        "run".to_string(),
        arg(graph_path),
        "--inputs".to_string(),
        INPUTS.to_string(),
        "--db".to_string(),
        arg(db),
        "--assets-dir".to_string(),
        arg(assets_root),
        "--tools-dir".to_string(),
        arg(&assets_root.join("tools")),
        format!("--fake-llm={}", script.display()),
        "--python".to_string(),
        python_program(),
        "--python-path".to_string(),
        arg(&repo_root().join("python")),
        "--python-path".to_string(),
        arg(&fixture_python_path()),
        // `llm_calls` 仅在 `--record=full` 落库（v0.3 e2e 需要请求/响应审计断言）。
        "--record=full".to_string(),
        "--json".to_string(),
    ];
    args.extend(extra.iter().cloned());
    args
}

/// 同步执行一次 v0.3 `run`。
pub fn run(
    cwd: &Path,
    db: &Path,
    graph_name: &str,
    assets_root: &Path,
    script: &Path,
    extra: &[String],
) -> Output {
    run_cli(
        &run_args(db, &graph(graph_name), assets_root, script, extra),
        cwd,
    )
}

/// 启动 `zgraph run` 子进程（piped），供取消/进程组用例。
pub struct RunChild {
    child: Option<Child>,
}

impl RunChild {
    /// 以 `run_args` 启动。
    pub fn spawn(
        cwd: &Path,
        db: &Path,
        graph_name: &str,
        assets_root: &Path,
        script: &Path,
        extra: &[String],
    ) -> Self {
        let child = crate::cli_support::zgraph_command()
            .args(run_args(db, &graph(graph_name), assets_root, script, extra))
            .current_dir(cwd)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn zgraph run");
        Self { child: Some(child) }
    }

    /// pid。
    pub fn pid(&self) -> u32 {
        self.child.as_ref().expect("child alive").id()
    }

    /// 等待退出（超时 SIGKILL → panic）。
    pub fn wait(mut self, context: &str) -> Output {
        let child = self.child.take().expect("child");
        wait_child(child, context, WAIT_TIMEOUT)
    }
}

impl Drop for RunChild {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

/// 等子进程结束（线程 + 超时 SIGKILL）。
pub fn wait_child(child: Child, context: &str, timeout: Duration) -> Output {
    let pid = child.id() as i32;
    let (sender, receiver) = mpsc::channel();
    let thread = std::thread::spawn(move || {
        let _ = sender.send(child.wait_with_output());
    });
    match receiver.recv_timeout(timeout) {
        Ok(Ok(output)) => {
            let _ = thread.join();
            output
        }
        Ok(Err(error)) => panic!("{context}: 等待子进程失败: {error}"),
        Err(_) => {
            let _ = nix::sys::signal::kill(
                nix::unistd::Pid::from_raw(pid),
                nix::sys::signal::Signal::SIGKILL,
            );
            let _ = thread.join();
            panic!("{context}: 子进程未在 {timeout:?} 内退出（已 SIGKILL）");
        }
    }
}

/// 发送 SIGINT（CLI 取消路径 → exit 130）。
pub fn send_sigint(pid: u32) {
    nix::sys::signal::kill(
        nix::unistd::Pid::from_raw(pid as i32),
        nix::sys::signal::Signal::SIGINT,
    )
    .expect("send SIGINT");
}

/// 进程是否存活（`kill(pid, 0)`；僵尸视为存活）。
pub fn pid_alive(pid: i32) -> bool {
    nix::sys::signal::kill(nix::unistd::Pid::from_raw(pid), None).is_ok()
}

/// 轮询等待进程消失（→ `true`；超时 `false`）。
pub fn wait_pid_gone(pid: i32, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if !pid_alive(pid) {
            return true;
        }
        std::thread::sleep(POLL_INTERVAL);
    }
    !pid_alive(pid)
}

/// 轮询等待文件出现（返回其文本；超时 panic）。
pub fn wait_file(path: &Path, timeout: Duration, context: &str) -> String {
    let deadline = Instant::now() + timeout;
    loop {
        if let Ok(text) = std::fs::read_to_string(path) {
            if !text.trim().is_empty() {
                return text;
            }
        }
        assert!(
            Instant::now() < deadline,
            "{context}: 文件 {:?} 未按时出现",
            path
        );
        std::thread::sleep(POLL_INTERVAL);
    }
}

/// 轮询等待文件内容满足条件（超时 panic）。
pub fn wait_file_contains(path: &Path, needle: &str, timeout: Duration, context: &str) -> String {
    let deadline = Instant::now() + timeout;
    loop {
        if let Ok(text) = std::fs::read_to_string(path) {
            if text.contains(needle) {
                return text;
            }
        }
        assert!(
            Instant::now() < deadline,
            "{context}: 文件 {:?} 未在 {timeout:?} 内含 {needle:?}",
            path
        );
        std::thread::sleep(POLL_INTERVAL);
    }
}

// ---------------------------------------------------------------------------
// trace 命令 / SQL 查询
// ---------------------------------------------------------------------------

/// 真 `zgraph trace <run> --json`。
pub fn trace_json(db: &Path, run_id: &str) -> Output {
    run_cli(
        &[
            "trace".to_string(),
            run_id.to_string(),
            "--db".to_string(),
            arg(db),
            "--json".to_string(),
        ],
        &repo_root(),
    )
}

/// 真 `zgraph replay --mode=events --json`。
pub fn replay_events_json(db: &Path, run_id: &str) -> Output {
    run_cli(
        &[
            "replay".to_string(),
            "--run".to_string(),
            run_id.to_string(),
            "--db".to_string(),
            arg(db),
            "--mode".to_string(),
            "events".to_string(),
            "--json".to_string(),
        ],
        &repo_root(),
    )
}

/// 真 `zgraph replay --mode=outputs --json`。
pub fn replay_outputs_json(db: &Path, run_id: &str) -> Output {
    run_cli(
        &[
            "replay".to_string(),
            "--run".to_string(),
            run_id.to_string(),
            "--db".to_string(),
            arg(db),
            "--mode".to_string(),
            "outputs".to_string(),
            "--json".to_string(),
        ],
        &repo_root(),
    )
}

/// NDJSON stdout → JSON 行。
pub fn ndjson(output: &Output) -> Vec<Value> {
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str(line).expect("NDJSON line is JSON"))
        .collect()
}

/// run 行的关键列：`(status, error_code, error_message, graph_hash, record_mode, killed_workers)`。
#[allow(clippy::type_complexity)]
pub fn run_row(
    db: &Path,
    run_id: &str,
) -> (
    String,
    Option<String>,
    Option<String>,
    String,
    Option<String>,
    i64,
) {
    read_only_connect(db)
        .query_row(
            "SELECT status, error_code, error_message, graph_hash, record_mode, killed_workers \
             FROM runs WHERE run_id = ?1",
            [run_id],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                ))
            },
        )
        .expect("run row")
}

/// `llm_calls` 的 `request_json`（按 instance/call_index 升序）。
pub fn llm_requests(db: &Path, run_id: &str) -> Vec<Value> {
    let conn = read_only_connect(db);
    let mut statement = conn
        .prepare(
            "SELECT request_json FROM llm_calls WHERE run_id = ?1 \
             ORDER BY instance_id, call_index",
        )
        .expect("prepare llm_calls");
    statement
        .query_map([run_id], |row| row.get::<_, String>(0))
        .expect("query llm_calls")
        .map(|row| serde_json::from_str(&row.expect("row")).expect("request json"))
        .collect()
}

/// `llm_calls` 行数（与调用序号）。
pub fn llm_call_count(db: &Path, run_id: &str) -> i64 {
    read_only_connect(db)
        .query_row(
            "SELECT COUNT(*) FROM llm_calls WHERE run_id = ?1",
            [run_id],
            |row| row.get(0),
        )
        .expect("count llm_calls")
}

/// `streaming_events` 的 tool 调用行：`(kind, call_id, tool_name, ok, summary, text)`。
#[allow(clippy::type_complexity)]
pub fn tool_events(
    db: &Path,
    run_id: &str,
) -> Vec<(
    String,
    Option<String>,
    Option<String>,
    Option<i64>,
    Option<String>,
    Option<String>,
)> {
    let conn = read_only_connect(db);
    let mut statement = conn
        .prepare(
            "SELECT kind, call_id, tool_name, ok, summary, text FROM streaming_events \
             WHERE run_id = ?1 AND kind IN ('tool_call_start','tool_call_end') ORDER BY seq",
        )
        .expect("prepare streaming_events");
    statement
        .query_map([run_id], |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
                row.get(5)?,
            ))
        })
        .expect("query streaming_events")
        .map(|row| row.expect("streaming row"))
        .collect()
}

/// 实例行：`(status, error_code, error_json)`。
pub fn instance(db: &Path, run_id: &str, node_id: &str) -> (String, Option<String>, Option<Value>) {
    read_only_connect(db)
        .query_row(
            "SELECT status, error_code, error FROM instances WHERE run_id = ?1 AND node_id = ?2 \
             ORDER BY attempt DESC LIMIT 1",
            [run_id, node_id],
            |row| {
                let error: Option<String> = row.get(2)?;
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    error.map(|text| serde_json::from_str(&text).expect("instance error json")),
                ))
            },
        )
        .expect("instance row")
}

/// 运行中 run 的 run_id（`Running|Paused`，最新）。
pub fn running_run_id(db: &Path) -> Option<String> {
    crate::cli_support::running_run(db).map(|(run_id, _pid)| run_id)
}

/// 断言退出码（转发 cli_support，便于统一导入）。
pub fn exit(output: &Output, expected: i32, context: &str) {
    assert_exit(output, expected, context);
}

/// stdout JSON（失败带 stderr）。
pub fn payload(output: &Output) -> Value {
    json_stdout(output)
}

/// stderr 文本。
pub fn err(output: &Output) -> String {
    stderr_text(output)
}
