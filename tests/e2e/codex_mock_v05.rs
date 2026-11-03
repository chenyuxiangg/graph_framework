//! F5d（v0.5）AC-28 g–j：**Codex 零密钥 mock provider 真链路**（真实 codex-cli）。
//!
//! 语义依据：DR-020 §4/§5（Codex FINAL：tool 事件勘误）、spec 07 AC-28 g–j。
//! 数据通路：真 `zgraph` CLI + 真 `python/zgraph_worker/adapters/codex.py` +
//! **真实 codex-cli 0.166.0**（npm 预装，`ZGRAPH_CODEX_BIN`/`CODEX_BIN` 定位）+
//! 本地 mock OpenAI Responses SSE 服务（`fixtures/py/mock_codex_responses.py`）；
//! **零密钥**（mock provider env_key 只需有值）。
//!
//! 门控：全部 `#[ignore]` + `ZGRAPH_REAL_CODEX=1`；二进制缺失 → 显式 skip 并打印
//! 原因（不伪造）。工具执行（F5f）需非嵌套环境 + `-s danger-full-access`；本机嵌套
//! Seatbelt 下显式 skip（不是假绿：零密钥 mock 文本场景不执行工具）。
//!
//! 运行（本机/CI 预装 codex 后）：
//! `ZGRAPH_REAL_CODEX=1 ZGRAPH_CODEX_BIN=<codex> cargo test -p zgraph-e2e \
//!  --test codex_mock_v05 -- --ignored --nocapture`

#![cfg(unix)]

#[path = "cli_support/mod.rs"]
mod cli_support;
#[path = "v03_support/mod.rs"]
mod v03;

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use cli_support::{assert_exit, json_stdout};
use v03::{agent, assets, fake_llm, llm_call_count, llm_requests, run, run_row, write_agent};

/// F5d pin（DR-020 #6）：真实二进制版本；不匹配则 skip（不伪造）。
const PINNED_CODEX_VERSION: &str = "0.166.0";

/// 节点超时（DR-020 #9）：codex 冷启动/重试分钟级；e2e 用 120s。
const CODEX_NODE_TIMEOUT_MS: u64 = 120_000;

/// 输出/等待上限。
const WAIT_TIMEOUT: Duration = Duration::from_secs(30);
const POLL_INTERVAL: Duration = Duration::from_millis(20);

/// 仓库内真实 Codex adapter。
fn codex_adapter() -> PathBuf {
    cli_support::repo_root()
        .join("python")
        .join("zgraph_worker")
        .join("adapters")
        .join("codex.py")
}

/// Python 解释器（`ZGRAPH_PYTHON` 优先）。
fn python_program() -> String {
    std::env::var("ZGRAPH_PYTHON").unwrap_or_else(|_| "python3".to_string())
}

/// 定位真实 codex 二进制；缺 env 时尝试本机 `/tmp` 预装（只读路径，不提交）。
fn locate_codex() -> Option<PathBuf> {
    for key in ["ZGRAPH_CODEX_BIN", "CODEX_BIN"] {
        if let Ok(value) = std::env::var(key) {
            if !value.trim().is_empty() {
                let path = PathBuf::from(value);
                if path.is_file() {
                    return Some(path);
                }
            }
        }
    }
    let fallback = PathBuf::from(
        "/tmp/codex-npm/node_modules/@openai/codex-darwin-arm64/vendor/\
         aarch64-apple-darwin/bin/codex",
    );
    fallback.is_file().then_some(fallback)
}

/// 本地 mock server 句柄。
struct MockServer {
    child: Child,
    port: u16,
    log: PathBuf,
}

impl MockServer {
    /// 启动 mock Responses SSE（`mode`: text/tool/slow）。
    fn start(dir: &Path, mode: &str) -> Self {
        let port_file = dir.join("mock.port");
        let log = dir.join("mock.log");
        let child = Command::new(python_program())
            .arg(v03::fixture("py/mock_codex_responses.py"))
            .arg(&port_file)
            .env("MOCK_CODEX_MODE", mode)
            .env("MOCK_CODEX_LOG", &log)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn mock Responses server");
        let port_text = v03::wait_file(&port_file, Duration::from_secs(15), "mock port file");
        let port: u16 = port_text.trim().parse().expect("mock port");
        Self { child, port, log }
    }

    /// 配置目录（临时 CODEX_HOME/config.toml，mock provider）。
    fn codex_home(&self, dir: &Path) -> PathBuf {
        let home = dir.join("codex-home");
        std::fs::create_dir_all(&home).expect("codex home");
        let config = format!(
            "model = \"mock-model\"\nmodel_provider = \"mock\"\n\n\
             [model_providers.mock]\nname = \"Mock\"\n\
             base_url = \"http://127.0.0.1:{}/v1\"\nwire_api = \"responses\"\n\
             env_key = \"MOCK_API_KEY\"\n",
            self.port
        );
        std::fs::write(home.join("config.toml"), config).expect("write config.toml");
        home
    }

    /// 日志行（请求/断开事件）。
    fn log_entries(&self) -> Vec<Value> {
        std::fs::read_to_string(&self.log)
            .unwrap_or_default()
            .lines()
            .filter_map(|line| serde_json::from_str(line).ok())
            .collect()
    }

    /// 等待日志出现某 `event`（有界）。
    fn wait_event(&self, event: &str, timeout: Duration) -> Vec<Value> {
        let deadline = Instant::now() + timeout;
        loop {
            let entries = self.log_entries();
            if entries.iter().any(|entry| entry["event"] == json!(event)) {
                return entries;
            }
            assert!(
                Instant::now() < deadline,
                "mock log 未在 {timeout:?} 内出现 {event}: {entries:?}"
            );
            std::thread::sleep(POLL_INTERVAL);
        }
    }
}

impl Drop for MockServer {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// 环境门控：`ZGRAPH_REAL_CODEX=1` + 可用且 pin 版本的 codex；否则打印 skip 返回 None。
fn gate(test: &str) -> Option<PathBuf> {
    if std::env::var("ZGRAPH_REAL_CODEX").ok().as_deref() != Some("1") {
        eprintln!("skip {test}: set ZGRAPH_REAL_CODEX=1 to run real codex mock e2e");
        return None;
    }
    let Some(binary) = locate_codex() else {
        eprintln!("skip {test}: codex binary not found (set ZGRAPH_CODEX_BIN; npm @openai/codex@{PINNED_CODEX_VERSION})");
        return None;
    };
    let version = Command::new(&binary)
        .arg("--version")
        .output()
        .ok()
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string())
        .unwrap_or_default();
    if !version.contains(PINNED_CODEX_VERSION) {
        eprintln!(
            "skip {test}: codex version '{version}' != pinned '{PINNED_CODEX_VERSION}' \
             (F5d pin; re-run/rebuild with pinned version)"
        );
        return None;
    }
    Some(binary)
}

/// 真实 codex adapter 的 harness 配置（零密钥 mock provider）。
fn codex_harness(binary: &Path, codex_home: &Path, dir: &Path) -> Value {
    let home = dir.join("home");
    std::fs::create_dir_all(&home).expect("home dir");
    json!({
        "kind": "subprocess",
        "command": python_program(),
        "args": [codex_adapter().display().to_string()],
        "protocol": "zgraph-harness/1",
        "env": {
            "CODEX_BIN": binary.display().to_string(),
            "CODEX_HOME": codex_home.display().to_string(),
            "MOCK_API_KEY": "dummy-zero-key",
            "HOME": home.display().to_string(),
            "PATH": std::env::var("PATH").unwrap_or_default(),
        },
    })
}

/// 带 codex harness + 120s 节点超时的 AgentConfig。
fn codex_agent(binary: &Path, codex_home: &Path, dir: &Path) -> Value {
    let mut value = agent("parser", &[]);
    value["harness"] = codex_harness(binary, codex_home, dir);
    value["node_timeout_ms"] = json!(CODEX_NODE_TIMEOUT_MS);
    value
}

/// 一次真 CLI codex mock run。
fn run_codex_scenario(dir: &Path, binary: &Path, server: &MockServer) -> (Output, PathBuf) {
    let assets_root = assets(dir);
    let codex_home = server.codex_home(dir);
    write_agent(&assets_root, &codex_agent(binary, &codex_home, dir));
    let script = fake_llm(
        &assets_root,
        json!([{"kind": "text", "content": "unused", "stop_reason": "EndTurn",
                "usage": {"prompt_tokens": 1, "completion_tokens": 1}}]),
    );
    let db = dir.join("trace.db");
    let output = run(dir, &db, "agent_one.json", &assets_root, &script, &[]);
    (output, db)
}

/// 直接探测 adapter `hello_ok`（不执行 run；真实 adapter + 真实进程）。
fn probe_adapter_hello(binary: &Path, codex_home: &Path, dir: &Path) -> Value {
    let home = dir.join("home");
    let mut command = Command::new(python_program());
    command
        .arg(codex_adapter())
        .env("CODEX_BIN", binary)
        .env("CODEX_HOME", codex_home)
        .env("MOCK_API_KEY", "dummy-zero-key")
        .env("HOME", &home)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn().expect("spawn codex adapter");
    {
        use std::io::Write as _;
        let stdin = child.stdin.as_mut().expect("adapter stdin");
        stdin
            .write_all(
                (json!({"v": 1, "type": "hello", "protocol": "zgraph-harness/1"}).to_string()
                    + "\n")
                    .as_bytes(),
            )
            .expect("write hello");
    }
    drop(child.stdin.take());
    let output = v03::wait_child(child, "adapter hello probe", Duration::from_secs(15));
    let stdout = String::from_utf8_lossy(&output.stdout);
    let first = stdout
        .lines()
        .find(|line| !line.trim().is_empty())
        .expect("hello_ok frame");
    serde_json::from_str(first).expect("hello_ok json")
}

/// 运行真实 codex binary（mock provider），供 AC-28 g 的 `-o` 文件一致性直接验证。
fn run_codex_binary(
    dir: &Path,
    binary: &Path,
    server: &MockServer,
    prompt: &str,
    sandbox: Option<&str>,
) -> PathBuf {
    let codex_home = server.codex_home(dir);
    let output_file = dir.join("codex-out.txt");
    let mut command = Command::new(binary);
    command
        .arg("exec")
        .arg("--json")
        .arg("--skip-git-repo-check")
        .arg("--ephemeral")
        .arg("--output-last-message")
        .arg(&output_file);
    if let Some(mode) = sandbox {
        command.arg("-s").arg(mode);
    }
    command
        .arg(prompt)
        .env("CODEX_HOME", &codex_home)
        .env("MOCK_API_KEY", "dummy-zero-key")
        .env("HOME", dir.join("home"))
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    let output = command.output().expect("run codex binary");
    assert!(
        output.status.success(),
        "codex binary 失败: status={:?} stderr={}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr)
    );
    output_file
}

// ---------------------------------------------------------------------------
// AC-28 g：零密钥 mock 文本全链路（适配器 + usage/cached/llm_calls）
// ---------------------------------------------------------------------------

#[test]
#[ignore = "requires real codex-cli 0.166.0 + local mock provider; set ZGRAPH_REAL_CODEX=1"]
fn f5d_a_codex_mock_zero_key_text_chain() {
    let Some(binary) = gate("f5d_a_codex_mock_zero_key_text_chain") else {
        return;
    };
    let dir = tempfile::tempdir().expect("tempdir");
    let server = MockServer::start(dir.path(), "text");
    let (output, db) = run_codex_scenario(dir.path(), &binary, &server);
    assert_exit(&output, 0, "codex mock text run");
    let payload = json_stdout(&output);
    assert_eq!(
        payload["status"],
        json!("Completed"),
        "{}",
        v03::err(&output)
    );
    let result = &payload["run_output"]["result"];
    assert_eq!(result["content"], json!("hello from mock"), "mock 文本");
    assert_eq!(result["stop_reason"], json!("EndTurn"));
    assert_eq!(
        result["iterations"],
        json!(1),
        "F5d #5：iterations 固定 1（文档口径）"
    );
    let usage = &result["usage"];
    assert_eq!(usage["prompt_tokens"], json!(11));
    assert_eq!(usage["completion_tokens"], json!(7));
    assert_eq!(usage["reasoning_tokens"], json!(2));
    assert_eq!(usage["total_tokens"], json!(18));
    assert!(
        usage.get("cached_input_tokens").is_none()
            && usage.get("cache_write_input_tokens").is_none(),
        "F5d：cached tokens 丢弃（协议无缓存维度）: {usage}"
    );
    let run_id = payload["run_id"].as_str().expect("run_id");
    assert_eq!(
        llm_call_count(&db, run_id),
        1,
        "外部 harness 不双重写 llm_calls"
    );
    // streaming=false：无 token/reasoning delta；mock 文本场景无工具执行。
    let conn = cli_support::read_only_connect(&db);
    let kinds: Vec<String> = {
        let mut statement = conn
            .prepare("SELECT kind FROM streaming_events WHERE run_id = ?1")
            .expect("streaming");
        statement
            .query_map([run_id], |row| row.get(0))
            .expect("query")
            .map(|row| row.expect("kind"))
            .collect()
    };
    assert!(
        !kinds
            .iter()
            .any(|kind| kind == "token" || kind == "reasoning"),
        "codex streaming=false 不得产生 token/reasoning: {kinds:?}"
    );
    assert_eq!(support_tool_calls(&conn, run_id), 0, "mock 文本无工具执行");
    drop(conn);
    // AC-28 g：hello_ok 帧直接验证（真实 adapter 进程）。
    let codex_home = server.codex_home(dir.path());
    let hello = probe_adapter_hello(&binary, &codex_home, dir.path());
    assert_eq!(hello["type"], json!("hello_ok"));
    assert_eq!(hello["provider"], json!("codex_exec"));
    assert_eq!(hello["capabilities"]["streaming"], json!(false));
    // AC-28 g：content == `-o` 文件（直接跑真实 codex binary 验证 mock/文件口径）。
    let out_file = run_codex_binary(
        dir.path(),
        &binary,
        &server,
        "reply with exactly: hello",
        None,
    );
    assert_eq!(
        std::fs::read_to_string(&out_file)
            .expect("codex -o file")
            .trim(),
        "hello from mock",
        "content 必须等于 -o 最终消息文件"
    );
    // 请求快照含外部 provider 标识（hello_ok 后由框架记录）。
    let requests = llm_requests(&db, run_id);
    assert!(
        requests[0].get("provider").is_some() || requests[0].get("agent_id").is_some(),
        "外部 harness 请求快照: {}",
        requests[0]
    );
}

/// `tool_calls` 行数（v7；mock 文本场景应为 0）。
fn support_tool_calls(conn: &rusqlite::Connection, run_id: &str) -> i64 {
    conn.query_row(
        "SELECT COUNT(*) FROM tool_calls WHERE run_id = ?1",
        [run_id],
        |row| row.get(0),
    )
    .expect("count tool_calls")
}

// ---------------------------------------------------------------------------
// AC-28 i：取消（SIGTERM/取消 3s 内进程消失、无孤儿；mock SSE 断开探针）
// ---------------------------------------------------------------------------

#[test]
#[ignore = "requires real codex-cli 0.166.0 + local mock provider; set ZGRAPH_REAL_CODEX=1"]
fn f5d_c_codex_mock_cancel_within_three_seconds() {
    let Some(binary) = gate("f5d_c_codex_mock_cancel_within_three_seconds") else {
        return;
    };
    let dir = tempfile::tempdir().expect("tempdir");
    let server = MockServer::start(dir.path(), "slow");
    let assets_root = assets(dir.path());
    let codex_home = server.codex_home(dir.path());
    write_agent(&assets_root, &codex_agent(&binary, &codex_home, dir.path()));
    let script = fake_llm(
        &assets_root,
        json!([{"kind": "text", "content": "unused", "stop_reason": "EndTurn",
                "usage": {"prompt_tokens": 1, "completion_tokens": 1}}]),
    );
    let db = dir.path().join("trace.db");
    let child = v03::RunChild::spawn(
        dir.path(),
        &db,
        "agent_one.json",
        &assets_root,
        &script,
        &[],
    );
    // 等 codex 真实请求到达 mock（说明 adapter+codex 链路已建立）。
    let _ = server.wait_event("request", WAIT_TIMEOUT);
    let started = Instant::now();
    v03::send_sigint(child.pid());
    let output = child.wait("codex mock cancel");
    assert_exit(&output, 130, "SIGINT → exit 130");
    assert_eq!(json_stdout(&output)["status"], json!("Cancelled"));
    // 探针：codex 连接被关闭（进程组被回收）→ mock SSE 写失败记录 disconnect。
    let entries = server.wait_event("client_disconnected", Duration::from_secs(3));
    let elapsed = started.elapsed();
    assert!(
        elapsed < Duration::from_secs(3),
        "取消后 codex 连接/进程必须在 3s 内消失，实际 {elapsed:?}；日志 {entries:?}"
    );
    assert!(
        entries
            .iter()
            .any(|entry| entry["event"] == json!("client_disconnected")),
        "mock 应记录 client_disconnected: {entries:?}"
    );
}

// ---------------------------------------------------------------------------
// AC-28 j / F5f（可选）：非嵌套环境 `-s danger-full-access` 工具事件
// ---------------------------------------------------------------------------

#[test]
#[ignore = "requires real codex-cli 0.166.0 + non-nested host + local mock provider; set ZGRAPH_REAL_CODEX=1"]
fn f5f_codex_tool_events_require_non_nested_danger_full_access() {
    let Some(binary) = gate("f5f_codex_tool_events_require_non_nested_danger_full_access") else {
        return;
    };
    let probe = Command::new("/usr/bin/sandbox-exec")
        .args(["-p", "(version 1)(deny default)", "/usr/bin/true"])
        .output()
        .ok()
        .and_then(|output| output.status.code());
    if probe == Some(71) {
        eprintln!(
            "skip F5f: nested Seatbelt (sandbox-exec exit 71) → codex tool 执行在嵌套环境 \
             静默不可用；F5f 需非嵌套用户终端/容器（DR-020 §4.5），不伪造 tool 事件"
        );
        return;
    }
    let dir = tempfile::tempdir().expect("tempdir");
    let server = MockServer::start(dir.path(), "tool");
    let codex_home = server.codex_home(dir.path());
    let output_file = dir.path().join("tool-out.txt");
    let output = Command::new(&binary)
        .args([
            "exec",
            "--json",
            "--skip-git-repo-check",
            "--ephemeral",
            "-s",
            "danger-full-access",
            "--output-last-message",
        ])
        .arg(&output_file)
        .arg("reply with exactly: hello")
        .env("CODEX_HOME", &codex_home)
        .env("MOCK_API_KEY", "dummy-zero-key")
        .env("HOME", dir.path().join("home"))
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .expect("run codex tool scenario");
    assert!(
        output.status.success(),
        "F5f codex tool 执行失败: status={:?} stderr={}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let events: Vec<Value> = stdout
        .lines()
        .filter(|line| !line.trim().is_empty())
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect();
    let command_items: Vec<&Value> = events
        .iter()
        .filter(|event| {
            event["item"]["type"] == json!("command_execution")
                && event["type"] == json!("item.completed")
        })
        .collect();
    assert!(
        !command_items.is_empty(),
        "F5f：非嵌套 danger-full-access 下必须出现 command_execution item: {events:?}"
    );
    assert!(
        command_items
            .iter()
            .any(|event| event["item"]["exit_code"] == json!(0)),
        "F5f：command_execution exit_code 必须为 0（ok==exit_code==0）: {command_items:?}"
    );
}

// 避免未使用告警（v03::run_row 在断言失败诊断时可能用到）。
#[allow(dead_code)]
fn _diagnostic_run_row(
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
    run_row(db, run_id)
}

// 保持与既有 e2e 一致的显式 Output 诊断引用（失败时 CLI stderr 已由 assert_exit 打印）。
#[allow(dead_code)]
fn _output_stderr(output: &Output) -> String {
    v03::err(output)
}
