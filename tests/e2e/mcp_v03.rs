//! V5a（v0.3 / M8）AC-19：MCP 工具（stdio 双 era + Streamable HTTP）**真 CLI 链路**。
//!
//! 语义依据：DR-010、spec 07 AC-19、06 §6.14、00 §0.4/§0.5。
//! 数据通路（禁止 mock）：真 `zgraph` 二进制 + **真 fake MCP server 子进程**
//! （Python stdio JSON-RPC；`tests/e2e/fixtures/py/fake_mcp_server.py`）+ 真 fake LLM
//! provider + 真 SQLite trace。
//!
//! 覆盖：a 双 era（`2026-07-28` stateless + `2025-11-25` legacy 回落）list/call；
//! b fake HTTP（Streamable HTTP）头/URL/`allow_insecure_http` 门控；c `--allow-mcp`
//! 预检（exit 3/不产生 run/不启 server）、命名/白名单/allow 交集/冲突；d 超时/取消/
//! 崩溃重启≤1/进程组无孤儿；e 结果截断/annotations 不可信/MRTR `input_required`
//! 结构化失败；f trace 不 v7（`tool_name` 命名空间/call_id 原生/无新表列）；g 默认无
//! MCP golden 不变。

#![cfg(unix)]

#[path = "cli_support/mod.rs"]
mod cli_support;
#[path = "v03_support/mod.rs"]
mod v03;

use std::path::{Path, PathBuf};
use std::process::Output;
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use cli_support::{assert_exit, json_stdout};
use v03::{agent, assets, fake_llm, llm_requests, run, run_row, tool_events, write_agent};

// ---------------------------------------------------------------------------
// 辅助
// ---------------------------------------------------------------------------

/// fake MCP server 脚本路径。
fn mcp_server_script() -> PathBuf {
    v03::fixture("py/fake_mcp_server.py")
}

/// Python 解释器（`ZGRAPH_PYTHON` 优先，缺省 `python3`）。
fn python_program() -> String {
    std::env::var("ZGRAPH_PYTHON").unwrap_or_else(|_| "python3".to_string())
}

/// 单条 `text` 脚本响应。
fn text_call(content: &str) -> Value {
    json!({"kind": "text", "content": content, "stop_reason": "EndTurn",
           "usage": {"prompt_tokens": 10, "completion_tokens": 2}})
}

/// 一条 MCP 工具调用脚本响应。
fn mcp_call(call_id: &str, tool: &str, args: Value) -> Value {
    json!({"kind": "tool_calls", "content": null,
           "tool_calls": [{"call_id": call_id, "name": tool, "args": args}]})
}

/// 带 MCP 配置的 Agent 资产。
fn mcp_agent(tools: &[&str], servers: &[&str], allow: &[&str]) -> Value {
    let mut value = agent("parser", tools);
    let mut mcp = json!({"servers": servers});
    if !allow.is_empty() {
        mcp["allow"] = json!(allow);
    }
    value["mcp"] = mcp;
    value
}

/// 写 stdio registry（一个 server），返回 registry 值。
fn stdio_registry(
    _assets_root: &Path,
    server_name: &str,
    era: &str,
    log: &Path,
    spawn: &Path,
    timeout_ms: Option<u64>,
    extra_env: &[(&str, &str)],
) -> Value {
    let mut env = json!({
        "ZGRAPH_FAKE_MCP_LOG": log.display().to_string(),
        "ZGRAPH_FAKE_MCP_SPAWN_LOG": spawn.display().to_string(),
    });
    for (key, value) in extra_env {
        env[*key] = json!(value);
    }
    let mut spec = json!({
        "command": python_program(),
        "args": [mcp_server_script().display().to_string(), era],
        "env": env,
    });
    if let Some(timeout_ms) = timeout_ms {
        spec["timeout_ms"] = json!(timeout_ms);
    }
    json!({"mcpServers": {server_name: spec}})
}

/// 读 registry 日志（每行 `{"dir","payload"}`）。
fn server_log(path: &Path) -> Vec<Value> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    text.lines()
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect()
}

/// 日志中的请求 method 序列（仅 in 方向）。
fn methods_in(log: &[Value]) -> Vec<String> {
    log.iter()
        .filter(|line| line["dir"] == json!("in"))
        .filter_map(|line| line["payload"]["method"].as_str().map(str::to_string))
        .collect()
}

/// spawn 日志中的 pid。
fn spawn_pids(path: &Path) -> Vec<i32> {
    std::fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .filter_map(|line| line.trim().parse().ok())
        .collect()
}

/// 建资产 + registry + fake LLM 并跑一次 agent_one（`--allow-mcp` 由调用方给）。
fn mcp_run(
    dir: &Path,
    agent_value: &Value,
    registry: &Value,
    calls: Value,
    extra: &[String],
) -> (Output, PathBuf, PathBuf) {
    let assets_root = assets(dir);
    write_agent(&assets_root, agent_value);
    v03::write_mcp_registry(&assets_root, registry);
    let script = fake_llm(&assets_root, calls);
    let db = dir.join("trace.db");
    let output = run(dir, &db, "agent_one.json", &assets_root, &script, extra);
    (output, db, assets_root)
}

/// run_id。
fn payload_run_id(payload: &Value) -> String {
    payload["run_id"].as_str().expect("run_id").to_string()
}

// ---------------------------------------------------------------------------
// AC-19 a：双 era stdio（modern stateless / legacy initialize 回落）
// ---------------------------------------------------------------------------

#[test]
fn ac19a_dual_era_stdio_list_and_call() {
    let dir = tempfile::tempdir().expect("tempdir");
    // modern（2026-07-28，无 initialize）：discover → tools/list → tools/call。
    let modern_dir = dir.path().join("modern");
    std::fs::create_dir_all(&modern_dir).expect("modern dir");
    let log = modern_dir.join("server.log");
    let spawn = modern_dir.join("spawn.log");
    let registry = stdio_registry(&modern_dir, "echo", "modern", &log, &spawn, None, &[]);
    let (output, db, _) = mcp_run(
        &modern_dir,
        &mcp_agent(&["mcp__echo__echo"], &["echo"], &[]),
        &registry,
        json!([
            mcp_call("call_native_1", "mcp__echo__echo", json!({"text": "hi"})),
            text_call("after echo")
        ]),
        &["--allow-mcp".to_string()],
    );
    assert_exit(&output, 0, "modern MCP run");
    let payload = json_stdout(&output);
    assert_eq!(payload["status"], json!("Completed"));
    let run_id = payload_run_id(&payload);
    let requests = llm_requests(&db, &run_id);
    assert_eq!(requests.len(), 2);
    let tools: Vec<String> = requests[0]["tools"]
        .as_array()
        .expect("tools")
        .iter()
        .filter_map(|tool| tool["name"].as_str().map(str::to_string))
        .collect();
    assert_eq!(tools, vec!["mcp__echo__echo".to_string()], "命名空间映射");
    assert!(
        requests[0]["tools"][0]["description"]
            .as_str()
            .is_some_and(|text| text.contains("Echo back")),
        "description 来自 server tools/list"
    );
    let tool_message = requests[1]["messages"]
        .as_array()
        .expect("messages")
        .iter()
        .find(|message| message["role"] == json!("tool"))
        .expect("tool message");
    assert_eq!(tool_message["content"], json!("echo:hi"));
    assert_eq!(
        tool_message["tool_call_id"],
        json!("call_native_1"),
        "原生 call_id"
    );
    let modern_methods = methods_in(&server_log(&log));
    assert!(
        modern_methods.contains(&"server/discover".to_string())
            && !modern_methods.iter().any(|method| method == "initialize"),
        "modern era 无 initialize: {modern_methods:?}"
    );
    assert!(modern_methods.contains(&"tools/list".to_string()));
    assert!(modern_methods.contains(&"tools/call".to_string()));

    // legacy（2025-11-25）：discover → -32601 → initialize/initialized 回落。
    let legacy_dir = dir.path().join("legacy");
    std::fs::create_dir_all(&legacy_dir).expect("legacy dir");
    let log = legacy_dir.join("server.log");
    let spawn = legacy_dir.join("spawn.log");
    let registry = stdio_registry(&legacy_dir, "legacy", "legacy", &log, &spawn, None, &[]);
    let (output, _db, _) = mcp_run(
        &legacy_dir,
        &mcp_agent(&["mcp__legacy__echo"], &["legacy"], &[]),
        &registry,
        json!([
            mcp_call("call_native_2", "mcp__legacy__echo", json!({"text": "old"})),
            text_call("after legacy")
        ]),
        &["--allow-mcp".to_string()],
    );
    assert_exit(&output, 0, "legacy MCP run（回落成功）");
    let payload = json_stdout(&output);
    assert_eq!(payload["status"], json!("Completed"));
    let legacy_methods = methods_in(&server_log(&log));
    assert_eq!(
        legacy_methods.first().map(String::as_str),
        Some("server/discover"),
        "先探测 modern: {legacy_methods:?}"
    );
    assert!(
        legacy_methods.contains(&"initialize".to_string())
            && legacy_methods.contains(&"notifications/initialized".to_string()),
        "legacy 回落握手: {legacy_methods:?}"
    );
    assert!(legacy_methods.contains(&"tools/list".to_string()));
    assert!(legacy_methods.contains(&"tools/call".to_string()));
}

// ---------------------------------------------------------------------------
// AC-19 b：Streamable HTTP（fake JSON-RPC server；头/URL/insecure 门控）
// ---------------------------------------------------------------------------

/// 启动 fake HTTP MCP server，返回 `(child, port, log)`；Drop 由调用方 kill。
fn spawn_http_server(dir: &Path) -> (std::process::Child, u16, PathBuf) {
    let port_file = dir.join("http.port");
    let log = dir.join("http.log");
    let child = std::process::Command::new(python_program())
        .arg(v03::fixture("py/fake_mcp_http.py"))
        .arg(&port_file)
        .env("ZGRAPH_FAKE_MCP_HTTP_LOG", &log)
        .env(cli_support::PYTHONDONTWRITEBYTECODE, "1")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("spawn fake MCP HTTP server");
    let port_text = v03::wait_file(&port_file, Duration::from_secs(10), "http port file");
    let port: u16 = port_text.trim().parse().expect("http port");
    (child, port, log)
}

/// 读 HTTP 请求日志。
fn http_log(path: &Path) -> Vec<Value> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    text.lines()
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect()
}

/// HTTP 请求头的小写键查找。
fn header<'a>(entry: &'a Value, name: &str) -> Option<&'a str> {
    entry["headers"].as_object().and_then(|headers| {
        headers
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .and_then(|(_, value)| value.as_str())
    })
}

#[test]
fn ac19b_streamable_http_headers_and_insecure_gate() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (mut child, port, log) = spawn_http_server(dir.path());
    let assets_root = assets(dir.path());
    write_agent(&assets_root, &mcp_agent(&["mcp__web__echo"], &["web"], &[]));
    v03::write_mcp_registry(
        &assets_root,
        &json!({"mcpServers": {"web": {
            "url": format!("http://127.0.0.1:{port}/mcp"),
            "allow_insecure_http": true,
            "timeout_ms": 10000
        }}}),
    );
    let script = fake_llm(
        &assets_root,
        json!([
            mcp_call("http-native-1", "mcp__web__echo", json!({"text": "http"})),
            text_call("after http")
        ]),
    );
    let db = dir.path().join("http.db");
    let output = run(
        dir.path(),
        &db,
        "agent_one.json",
        &assets_root,
        &script,
        &["--allow-mcp".to_string()],
    );
    let _ = child.kill();
    let _ = child.wait();
    assert_exit(&output, 0, "HTTP MCP run");
    let payload = json_stdout(&output);
    assert_eq!(payload["status"], json!("Completed"));
    let run_id = payload_run_id(&payload);
    let requests = llm_requests(&db, &run_id);
    let tool_message = requests[1]["messages"]
        .as_array()
        .expect("messages")
        .iter()
        .find(|message| message["role"] == json!("tool"))
        .expect("tool message");
    assert_eq!(tool_message["content"], json!("http-echo:http"));
    // 头校验：MCP-Protocol-Version / Mcp-Method / Mcp-Name（现代规范）。
    let entries = http_log(&log);
    assert_eq!(entries.len(), 3, "discover + list + call: {entries:?}");
    for entry in &entries {
        assert_eq!(
            header(entry, "MCP-Protocol-Version"),
            Some("2026-07-28"),
            "每请求版本协商头: {entry}"
        );
        assert_eq!(
            header(entry, "Mcp-Method"),
            entry["method"].as_str(),
            "Mcp-Method 与实际方法一致: {entry}"
        );
    }
    let call_entry = entries
        .iter()
        .find(|entry| entry["method"] == json!("tools/call"))
        .expect("tools/call entry");
    assert_eq!(header(call_entry, "Mcp-Name"), Some("echo"), "Mcp-Name 头");

    // insecure 门控：`http://` 未显式 allow_insecure_http → fail-closed `http_insecure`。
    let gate_dir = tempfile::tempdir().expect("gate tempdir");
    let gate_assets = assets(gate_dir.path());
    write_agent(&gate_assets, &mcp_agent(&["mcp__web__echo"], &["web"], &[]));
    v03::write_mcp_registry(
        &gate_assets,
        &json!({"mcpServers": {"web": {
            "url": format!("http://127.0.0.1:{port}/mcp")
        }}}),
    );
    let gate_script = fake_llm(&gate_assets, json!([text_call("x")]));
    let gate_db = gate_dir.path().join("gate.db");
    let output = run(
        gate_dir.path(),
        &gate_db,
        "agent_one.json",
        &gate_assets,
        &gate_script,
        &["--allow-mcp".to_string()],
    );
    assert_exit(&output, 1, "http:// 未显式允许 → run Failed");
    let payload = json_stdout(&output);
    let run_id = payload_run_id(&payload);
    let (_, code, message, ..) = run_row(&gate_db, &run_id);
    assert_eq!(code.as_deref(), Some("HARNESS_MCP_ERROR"));
    // REV-100 #3：reason 闭集 `http_insecure` + detail.server。
    assert!(
        message
            .as_deref()
            .is_some_and(|text| text.contains("http_insecure")),
        "detail.reason http_insecure: {message:?}"
    );
    let (_, _, error) = v03::instance(&gate_db, &run_id, "agent");
    let error = error.expect("instance error json");
    assert_eq!(error["detail"]["reason"], json!("http_insecure"));
    assert_eq!(error["detail"]["server"], json!("web"), "detail.server");
}

// ---------------------------------------------------------------------------
// AC-19 c：`--allow-mcp` 预检 / 命名 / 白名单 / allow 交集 / 冲突
// ---------------------------------------------------------------------------

#[test]
fn ac19c_not_allowed_fails_before_run_and_never_starts_server() {
    let dir = tempfile::tempdir().expect("tempdir");
    let log = dir.path().join("server.log");
    let spawn = dir.path().join("spawn.log");
    let registry = stdio_registry(dir.path(), "echo", "modern", &log, &spawn, None, &[]);
    // 不传 --allow-mcp。
    let (output, db, _) = mcp_run(
        dir.path(),
        &mcp_agent(&["mcp__echo__echo"], &["echo"], &[]),
        &registry,
        json!([text_call("x")]),
        &[],
    );
    assert_exit(&output, 3, "未开启 MCP → 启动前 exit 3");
    let stderr = v03::err(&output);
    assert!(stderr.contains("HARNESS_MCP_ERROR"), "{stderr}");
    assert!(stderr.contains("not_allowed"), "{stderr}");
    assert!(stderr.contains("--allow-mcp"), "提示开关: {stderr}");
    assert!(
        String::from_utf8_lossy(&output.stdout).trim().is_empty(),
        "启动前失败不打印 JSON"
    );
    // 不产生 run、不启动 server。
    assert!(
        !db.is_file() || {
            let conn = cli_support::read_only_connect(&db);
            conn.query_row("SELECT COUNT(*) FROM runs", [], |row| row.get::<_, i64>(0))
                .unwrap_or(0)
                == 0
        },
        "不产生 run"
    );
    assert!(!spawn.is_file(), "不得启动 MCP server 进程");

    // 开启 --allow-mcp：成功 + stderr 打印无密钥命令。
    let (output, _db, _) = mcp_run(
        dir.path(),
        &mcp_agent(&["mcp__echo__echo"], &["echo"], &[]),
        &registry,
        json!([
            mcp_call("c1", "mcp__echo__echo", json!({"text": "ok"})),
            text_call("done")
        ]),
        &["--allow-mcp".to_string()],
    );
    assert_exit(&output, 0, "开启 --allow-mcp");
    let stderr = v03::err(&output);
    assert!(
        stderr.contains("starting server 'echo'") && stderr.contains("stdio:"),
        "stderr 打印 server 启动命令（无密钥）: {stderr}"
    );
    assert!(stderr.contains(&mcp_server_script().display().to_string()));
}

#[test]
fn ac19c_naming_whitelist_allow_and_conflict_are_fail_closed() {
    let dir = tempfile::tempdir().expect("tempdir");
    let log = dir.path().join("server.log");
    let spawn = dir.path().join("spawn.log");
    let registry = stdio_registry(dir.path(), "echo", "modern", &log, &spawn, None, &[]);

    // ① 白名单：MCP 工具未在 AgentConfig.tools 显式列出 → 不可见；LLM 若仍调用该工具
    //    → `HARNESS_UNKNOWN_TOOL`（与 DR-010 §2.2「已注册但未列出 → unknown tool」一致）。
    let (output, db, _) = mcp_run(
        dir.path(),
        &mcp_agent(&[], &["echo"], &[]),
        &registry,
        json!([
            mcp_call("c1", "mcp__echo__echo", json!({"text": "hidden"})),
            text_call("after hidden")
        ]),
        &["--allow-mcp".to_string()],
    );
    assert_exit(&output, 1, "未列出的 MCP 工具不可见 → unknown tool");
    let payload = json_stdout(&output);
    let run_id = payload_run_id(&payload);
    let (_, code, message, ..) = run_row(&db, &run_id);
    assert_eq!(code.as_deref(), Some("HARNESS_UNKNOWN_TOOL"));
    assert!(
        message
            .as_deref()
            .is_some_and(|text| text.contains("mcp__echo__echo")),
        "{message:?}"
    );
    let requests = llm_requests(&db, &run_id);
    assert!(
        requests[0]["tools"].as_array().expect("tools").is_empty(),
        "未列出的 MCP 工具不得注册: {:?}",
        requests[0]["tools"]
    );

    // ② allow 交集：tools 列出但 allow 未含 → Loading fail-closed（tool_not_allowed）。
    let case = dir.path().join("allow-case");
    std::fs::create_dir_all(&case).expect("case dir");
    let (output, db, _) = mcp_run(
        &case,
        &mcp_agent(&["mcp__echo__echo"], &["echo"], &["mcp__echo__annotated"]),
        &registry,
        json!([text_call("x")]),
        &["--allow-mcp".to_string()],
    );
    assert_exit(&output, 1, "allow 未覆盖 → run Failed");
    let payload = json_stdout(&output);
    let run_id = payload_run_id(&payload);
    let (_, code, message, ..) = run_row(&db, &run_id);
    assert_eq!(code.as_deref(), Some("HARNESS_MCP_ERROR"));
    assert!(
        message
            .as_deref()
            .is_some_and(|text| text.contains("tool_not_allowed")),
        "reason tool_not_allowed: {message:?}"
    );

    // ③ 名称冲突（同一 server 重复引用）→ fail-closed name_conflict。
    let case = dir.path().join("dup-server");
    std::fs::create_dir_all(&case).expect("case dir");
    let (output, db, _) = mcp_run(
        &case,
        &mcp_agent(&["mcp__echo__echo"], &["echo", "echo"], &[]),
        &registry,
        json!([text_call("x")]),
        &["--allow-mcp".to_string()],
    );
    assert_exit(&output, 1, "重复 server 引用 → run Failed");
    let payload = json_stdout(&output);
    let run_id = payload_run_id(&payload);
    let (_, code, message, ..) = run_row(&db, &run_id);
    assert_eq!(code.as_deref(), Some("HARNESS_MCP_ERROR"));
    assert!(
        message
            .as_deref()
            .is_some_and(|text| text.contains("name_conflict")),
        "reason name_conflict: {message:?}"
    );
}

// ---------------------------------------------------------------------------
// AC-19 d：超时 / 取消 / 崩溃重启≤1 / 进程组无孤儿
// ---------------------------------------------------------------------------

#[test]
fn ac19d_call_timeout_is_tool_error_without_hanging() {
    let dir = tempfile::tempdir().expect("tempdir");
    let log = dir.path().join("server.log");
    let spawn = dir.path().join("spawn.log");
    let registry = stdio_registry(dir.path(), "echo", "modern", &log, &spawn, Some(400), &[]);
    let started = Instant::now();
    let (output, db, _) = mcp_run(
        dir.path(),
        &mcp_agent(&["mcp__echo__slow"], &["echo"], &[]),
        &registry,
        json!([
            mcp_call("c1", "mcp__echo__slow", json!({"seconds": 5})),
            text_call("after timeout")
        ]),
        &["--allow-mcp".to_string()],
    );
    let elapsed = started.elapsed();
    assert_exit(&output, 0, "call 超时是工具错误，不 fail run");
    assert!(
        elapsed < Duration::from_secs(5),
        "自包 timeout 应生效（server 端 sleep 5s），实际 {elapsed:?}"
    );
    let payload = json_stdout(&output);
    assert_eq!(payload["status"], json!("Completed"));
    let run_id = payload_run_id(&payload);
    let events = tool_events(&db, &run_id);
    assert!(
        events
            .iter()
            .any(|(kind, _, tool, ok, _, _)| kind == "tool_call_end"
                && tool.as_deref() == Some("mcp__echo__slow")
                && *ok == Some(0)),
        "tool_call_end ok=false: {events:?}"
    );
    // run 结束回收 server 进程组：无孤儿。
    let pids = spawn_pids(&spawn);
    assert_eq!(pids.len(), 1, "一个 server 进程: {pids:?}");
    assert!(
        v03::wait_pid_gone(pids[0], Duration::from_secs(5)),
        "run 结束后 MCP server 进程应被回收（无孤儿）"
    );
}

#[test]
fn ac19d_cancel_kills_server_process_group() {
    let dir = tempfile::tempdir().expect("tempdir");
    let log = dir.path().join("server.log");
    let spawn = dir.path().join("spawn.log");
    let registry = stdio_registry(dir.path(), "echo", "modern", &log, &spawn, None, &[]);
    // 构造资产并启动 run（slow 工具在飞时 SIGINT）。
    let assets_root = assets(dir.path());
    write_agent(
        &assets_root,
        &mcp_agent(&["mcp__echo__slow"], &["echo"], &[]),
    );
    v03::write_mcp_registry(&assets_root, &registry);
    let script = fake_llm(
        &assets_root,
        json!([
            mcp_call("c1", "mcp__echo__slow", json!({"seconds": 30})),
            text_call("done")
        ]),
    );
    let db = dir.path().join("trace.db");
    let run_child = v03::RunChild::spawn(
        dir.path(),
        &db,
        "agent_one.json",
        &assets_root,
        &script,
        &["--allow-mcp".to_string()],
    );
    let pid = run_child.pid();
    // 等 server 启动（spawn 日志出现 pid）后再取消。
    let spawned = v03::wait_file(&spawn, Duration::from_secs(10), "server spawn");
    let server_pid: i32 = spawned
        .lines()
        .next()
        .expect("pid line")
        .trim()
        .parse()
        .expect("pid");
    v03::send_sigint(pid);
    let output = run_child.wait("MCP cancel run");
    assert_exit(&output, 130, "SIGINT → exit 130");
    let payload = json_stdout(&output);
    assert_eq!(payload["status"], json!("Cancelled"));
    assert!(
        v03::wait_pid_gone(server_pid, Duration::from_secs(5)),
        "取消后 MCP server 进程组应被 kill（pid {server_pid} 无孤儿）"
    );
}

#[test]
fn ac19d_crash_restarts_at_most_once() {
    let dir = tempfile::tempdir().expect("tempdir");
    let log = dir.path().join("server.log");
    let spawn = dir.path().join("spawn.log");
    let crash_marker = dir.path().join("crash-once.marker");
    let mut registry = stdio_registry(dir.path(), "echo", "modern", &log, &spawn, None, &[]);
    registry["mcpServers"]["echo"]["env"]["ZGRAPH_FAKE_CRASH_ONCE_FILE"] =
        json!(crash_marker.display().to_string());
    let (output, db, _) = mcp_run(
        dir.path(),
        &mcp_agent(
            &["mcp__echo__crash_once", "mcp__echo__crash"],
            &["echo"],
            &[],
        ),
        &registry,
        json!([
            // call#1：crash_once 首次调用崩溃 → 同 run 仅 1 次重启 → 重试成功（传输恢复）。
            mcp_call("c1", "mcp__echo__crash_once", json!({})),
            // call#2：重启额度已用满 → crash 后 Closed 不再 spawn → transport_closed。
            mcp_call("c2", "mcp__echo__crash", json!({})),
            text_call("after crashes")
        ]),
        &["--allow-mcp".to_string()],
    );
    assert_exit(&output, 0, "server 崩溃/重启是工具错误（不 fail run）");
    let payload = json_stdout(&output);
    assert_eq!(payload["status"], json!("Completed"));
    let run_id = payload_run_id(&payload);
    // 每 run 每 server 恰 1 次重启：spawn/connect 总数 = 2（首启 + 唯一重启）。
    let pids = spawn_pids(&spawn);
    assert_eq!(
        pids.len(),
        2,
        "同 run 每 server 最多重启 1 次 → spawn 总数 = 2: {pids:?}"
    );
    // call#1 经重启重试成功（传输恢复）；call#2 因额度耗尽返回 transport_closed。
    let requests = llm_requests(&db, &run_id);
    let tool_messages: Vec<(String, String)> = requests
        .iter()
        .flat_map(|request| {
            request["messages"]
                .as_array()
                .cloned()
                .unwrap_or_default()
                .into_iter()
                .filter(|message| message["role"] == json!("tool"))
                .map(|message| {
                    (
                        message["tool_call_id"]
                            .as_str()
                            .unwrap_or_default()
                            .to_string(),
                        message["content"].as_str().unwrap_or_default().to_string(),
                    )
                })
                .collect::<Vec<_>>()
        })
        .collect();
    let first = tool_messages
        .iter()
        .find(|(call_id, _)| call_id == "c1")
        .expect("c1 tool message");
    assert!(
        first.1.contains("recovered"),
        "首次 crash 后重试成功（传输恢复）: {:?}",
        first.1
    );
    let second = tool_messages
        .iter()
        .find(|(call_id, _)| call_id == "c2")
        .expect("c2 tool message");
    let second_payload: Value = serde_json::from_str(&second.1).expect("c2 tool error json");
    assert_eq!(
        second_payload["error"]["reason"],
        json!("transport_closed"),
        "额度耗尽后的 Closed → TOOL_ERROR transport_closed: {second_payload}"
    );
    let events = tool_events(&db, &run_id);
    assert!(
        events.iter().any(|(kind, call_id, tool, ok, _, _)| {
            kind == "tool_call_end"
                && call_id.as_deref() == Some("c1")
                && tool.as_deref() == Some("mcp__echo__crash_once")
                && *ok == Some(1)
        }),
        "c1 重试成功 tool_call_end ok=true: {events:?}"
    );
    assert!(
        events.iter().any(|(kind, call_id, tool, ok, _, _)| {
            kind == "tool_call_end"
                && call_id.as_deref() == Some("c2")
                && tool.as_deref() == Some("mcp__echo__crash")
                && *ok == Some(0)
        }),
        "c2 transport_closed tool_call_end ok=false: {events:?}"
    );
    // 全部 server 进程在 run 结束后回收；额度用满后不得新增 spawn。
    assert_eq!(spawn_pids(&spawn).len(), 2, "后续调用不得再 spawn");
    for pid in pids {
        assert!(
            v03::wait_pid_gone(pid, Duration::from_secs(5)),
            "server pid {pid} 应被回收"
        );
    }
}

// ---------------------------------------------------------------------------
// AC-19 e：结果截断 / annotations 不可信 / MRTR input_required
// ---------------------------------------------------------------------------

#[test]
fn ac19e_result_truncation_annotations_and_input_required() {
    let dir = tempfile::tempdir().expect("tempdir");
    let log = dir.path().join("server.log");
    let spawn = dir.path().join("spawn.log");
    let registry = stdio_registry(dir.path(), "echo", "modern", &log, &spawn, None, &[]);
    let (output, db, _) = mcp_run(
        dir.path(),
        &mcp_agent(
            &[
                "mcp__echo__big",
                "mcp__echo__annotated",
                "mcp__echo__needs_input",
            ],
            &["echo"],
            &[],
        ),
        &registry,
        json!([
            mcp_call("c1", "mcp__echo__big", json!({})),
            mcp_call("c2", "mcp__echo__annotated", json!({})),
            mcp_call("c3", "mcp__echo__needs_input", json!({})),
            text_call("after")
        ]),
        &["--allow-mcp".to_string()],
    );
    assert_exit(&output, 0, "AC-19 e run");
    let payload = json_stdout(&output);
    assert_eq!(payload["status"], json!("Completed"));
    let run_id = payload_run_id(&payload);
    let requests = llm_requests(&db, &run_id);
    // 截断：40k 文本 → ≤32768 chars + 截断标记。
    let big_tool = requests
        .iter()
        .flat_map(|request| request["messages"].as_array().cloned().unwrap_or_default())
        .find(|message| {
            message["role"] == json!("tool")
                && message["content"]
                    .as_str()
                    .is_some_and(|text| text.contains("…[mcp result truncated]"))
        })
        .expect("big tool message");
    let big_text = big_tool["content"].as_str().expect("content");
    // REV-100 #2：超 32_768 chars 的最终 ToolResult 是 JSON
    // `{truncated,is_error,text}`；text 为 32768 chars 头部 + `…[mcp result truncated]`。
    let truncated_payload: Value =
        serde_json::from_str(big_text).expect("truncated ToolResult JSON");
    let mut keys: Vec<&str> = truncated_payload
        .as_object()
        .expect("tool result object")
        .keys()
        .map(String::as_str)
        .collect();
    keys.sort_unstable();
    assert_eq!(
        keys,
        vec!["is_error", "text", "truncated"],
        "truncated ToolResult keys 恰为 {{truncated,is_error,text}}: {truncated_payload}"
    );
    assert_eq!(truncated_payload["truncated"], json!(true));
    assert_eq!(truncated_payload["is_error"], json!(false));
    let truncated_text = truncated_payload["text"].as_str().expect("text");
    assert!(
        truncated_text.ends_with("…[mcp result truncated]"),
        "text 以截断标记结尾: {}",
        &truncated_text[truncated_text.len().saturating_sub(60)..]
    );
    assert!(
        truncated_text.chars().count() >= 32_768,
        "text 长度下界为 32768 chars：{}",
        truncated_text.chars().count()
    );
    assert!(truncated_text.starts_with('B'), "text 保留 server 结果前缀");
    // annotations 不可信：带 annotations 的工具仍正常返回（不改变权限/行为）。
    let annotated = requests
        .iter()
        .flat_map(|request| request["messages"].as_array().cloned().unwrap_or_default())
        .filter(|message| message["role"] == json!("tool"))
        .map(|message| message["content"].as_str().unwrap_or_default().to_string())
        .find(|content| content == "annotated-ok")
        .expect("annotated 工具结果正常喂回");
    assert_eq!(
        annotated, "annotated-ok",
        "≤32768 时仍是原始文本（非 JSON）"
    );
    // MRTR input_required：结构化失败（喂回 LLM，不静默/不 AI 代答）。
    let needs_input_text = requests
        .iter()
        .flat_map(|request| request["messages"].as_array().cloned().unwrap_or_default())
        .find(|message| {
            message["role"] == json!("tool")
                && message["content"]
                    .as_str()
                    .is_some_and(|text| text.contains("input_required"))
        })
        .map(|message| message["content"].as_str().unwrap_or_default().to_string());
    assert!(
        needs_input_text
            .as_deref()
            .is_some_and(|text| text.contains("input_required_unsupported")
                || text.contains("input_required")),
        "input_required → 结构化工具失败: {needs_input_text:?}"
    );
}

/// AC-19 e：registry `env` 的 `${ENV}` 展开传入 server，但密钥不落 stderr/trace/DB。
#[test]
fn ac19e_secret_env_is_expanded_but_not_logged() {
    let dir = tempfile::tempdir().expect("tempdir");
    let log = dir.path().join("server.log");
    let spawn = dir.path().join("spawn.log");
    let probe = dir.path().join("probe.txt");
    let secret = "s3cr3t-e2e-token-value";
    let mut registry = stdio_registry(dir.path(), "echo", "modern", &log, &spawn, None, &[]);
    registry["mcpServers"]["echo"]["env"]["ZGRAPH_FAKE_PROBE"] = json!("${ZGRAPH_E2E_MCP_SECRET}");
    registry["mcpServers"]["echo"]["env"]["ZGRAPH_FAKE_MCP_PROBE_FILE"] =
        json!(probe.display().to_string());
    let assets_root = assets(dir.path());
    write_agent(
        &assets_root,
        &mcp_agent(&["mcp__echo__echo"], &["echo"], &[]),
    );
    v03::write_mcp_registry(&assets_root, &registry);
    let script = fake_llm(
        &assets_root,
        json!([
            mcp_call("c1", "mcp__echo__echo", json!({"text": "secret-case"})),
            text_call("done")
        ]),
    );
    let db = dir.path().join("trace.db");
    let output = cli_support::run_cli_env(
        &v03::run_args(
            &db,
            &v03::graph("agent_one.json"),
            &assets_root,
            &script,
            &["--allow-mcp".to_string()],
        ),
        dir.path(),
        &[("ZGRAPH_E2E_MCP_SECRET", secret)],
    );
    assert_exit(&output, 0, "secret env case run");
    // 展开证据：server 子进程环境收到展开值（测试侧 probe 文件）。
    let expanded = v03::wait_file(&probe, Duration::from_secs(10), "probe file");
    assert_eq!(expanded.trim(), secret, "${{ENV}} 展开传入子进程 env");
    // 不泄漏：stderr 只列 env 名，不含值；trace/llm_calls 不含密钥。
    let stderr = v03::err(&output);
    assert!(!stderr.contains(secret), "stderr 不得含密钥: {stderr}");
    assert!(
        stderr.contains("ZGRAPH_FAKE_PROBE") || stderr.contains("env:"),
        "stderr 可列 env 名: {stderr}"
    );
    let payload = json_stdout(&output);
    let run_id = payload_run_id(&payload);
    let requests = llm_requests(&db, &run_id);
    assert!(
        !serde_json::to_string(&requests)
            .unwrap_or_default()
            .contains(secret),
        "llm_calls 不得含密钥"
    );
    let db_bytes = std::fs::read(&db).expect("read db");
    assert!(
        !String::from_utf8_lossy(&db_bytes).contains(secret),
        "trace DB 不得含密钥"
    );
}

// ---------------------------------------------------------------------------
// AC-19 f：trace 无 MCP 专属表/列（v0.4/D5 后全局 v7；tool_name 命名空间 / call_id 原生）
// ---------------------------------------------------------------------------

#[test]
fn ac19f_trace_schema_and_tool_identity_unchanged() {
    let dir = tempfile::tempdir().expect("tempdir");
    let log = dir.path().join("server.log");
    let spawn = dir.path().join("spawn.log");
    let registry = stdio_registry(dir.path(), "echo", "modern", &log, &spawn, None, &[]);
    let (output, db, _) = mcp_run(
        dir.path(),
        &mcp_agent(&["mcp__echo__echo"], &["echo"], &[]),
        &registry,
        json!([
            mcp_call("native-id-42", "mcp__echo__echo", json!({"text": "trace"})),
            text_call("done")
        ]),
        &["--allow-mcp".to_string()],
    );
    assert_exit(&output, 0, "AC-19 f run");
    let payload = json_stdout(&output);
    let run_id = payload_run_id(&payload);
    let conn = cli_support::read_only_connect(&db);
    // v0.4/D5 起 schema 全局为 v7；MCP 不新增专属表/列。
    let version: i64 = conn
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .expect("user_version");
    assert_eq!(
        version, 7,
        "v0.4/D5 全局 v7（tool_calls），但 MCP 不得新增专属表/列"
    );
    let tables: Vec<String> = {
        let mut statement = conn
            .prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
            .expect("tables");
        statement
            .query_map([], |row| row.get(0))
            .expect("query")
            .map(|row| row.expect("table"))
            .collect()
    };
    assert!(
        !tables.iter().any(|name| name.contains("mcp")),
        "无 MCP 新表: {tables:?}"
    );
    let streaming_columns: Vec<String> = {
        let mut statement = conn
            .prepare("PRAGMA table_info(streaming_events)")
            .expect("table_info");
        statement
            .query_map([], |row| row.get::<_, String>(1))
            .expect("query")
            .map(|row| row.expect("column"))
            .collect()
    };
    assert!(
        !streaming_columns.iter().any(|name| name.contains("server")),
        "无 MCP server 列: {streaming_columns:?}"
    );
    drop(conn);
    // tool_name 命名空间 + 原生 call_id 保留；events.kind 仍是既有 7 类。
    let events = tool_events(&db, &run_id);
    assert!(events.iter().any(|(_, call_id, tool_name, _, _, _)| {
        call_id.as_deref() == Some("native-id-42")
            && tool_name.as_deref() == Some("mcp__echo__echo")
    }));
    let trace = v03::trace_json(&db, &run_id);
    let kinds: Vec<String> = v03::ndjson(&trace)
        .iter()
        .map(|line| line["kind"].as_str().unwrap_or_default().to_string())
        .collect();
    let allowed = [
        "RunStarted",
        "Transition",
        "GateResult",
        "NodeTimedOut",
        "NodeAbandoned",
        "Streaming",
        "RunFinished",
    ];
    assert!(
        kinds.iter().all(|kind| allowed.contains(&kind.as_str())),
        "events.kind 不变: {kinds:?}"
    );
    // L1 replay 与 trace 逐字节一致。
    let replay = v03::replay_events_json(&db, &run_id);
    assert_eq!(replay.stdout, trace.stdout);
}

// ---------------------------------------------------------------------------
// AC-19 g：默认无 MCP 不变（不读 registry/不启进程/不注册工具）
// ---------------------------------------------------------------------------

#[test]
fn ac19g_default_without_mcp_is_unchanged() {
    let dir = tempfile::tempdir().expect("tempdir");
    let log = dir.path().join("server.log");
    let spawn = dir.path().join("spawn.log");
    let assets_root = assets(dir.path());
    write_agent(&assets_root, &agent("parser", &[]));
    // 即使 registry 存在且可启动，无 AgentConfig.mcp 时也不得读/启。
    v03::write_mcp_registry(
        &assets_root,
        &stdio_registry(dir.path(), "echo", "modern", &log, &spawn, None, &[]),
    );
    let script = fake_llm(&assets_root, json!([text_call("default")]));
    let db = dir.path().join("trace.db");
    let output = run(
        dir.path(),
        &db,
        "agent_one.json",
        &assets_root,
        &script,
        &[],
    );
    assert_exit(&output, 0, "默认无 MCP run");
    let payload = json_stdout(&output);
    assert_eq!(payload["status"], json!("Completed"));
    let run_id = payload_run_id(&payload);
    let requests = llm_requests(&db, &run_id);
    assert!(
        requests[0]["tools"].as_array().expect("tools").is_empty(),
        "无 MCP 配置不注册工具"
    );
    assert!(!spawn.is_file(), "无 MCP 配置不启动 server");
    assert!(!log.is_file(), "无 MCP 配置不读/连 server");
    // trace schema 与当前 v0.4 全局版本一致（v7）；replay 与 v0.2 口径一致。
    let conn = cli_support::read_only_connect(&db);
    let version: i64 = conn
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .expect("user_version");
    assert_eq!(version, 7);
    drop(conn);
    let trace = v03::trace_json(&db, &run_id);
    let replay = v03::replay_events_json(&db, &run_id);
    assert_eq!(replay.stdout, trace.stdout);
}
