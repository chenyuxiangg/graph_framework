//! V5a（v0.3 / M8）AC-20：可插拔 harness（`AgentProvider` + `zgraph-harness/1`
//! 子进程协议）**真 CLI 链路**。
//!
//! 语义依据：DR-011、spec 07 AC-20、05 §5.14.3、06 §6.14。
//! 数据通路（禁止 mock）：真 `zgraph` 二进制 + **真 fake adapter 子进程**
//! （`tests/e2e/fixtures/py/fake_harness_adapter.py`）+ 真 SQLite trace；Codex 侧用
//! 假 `codex` 可执行 PATH 注入跑仓库内真实 adapter（`python/zgraph_worker/adapters/codex.py`）。
//!
//! 覆盖：a builtin 默认不变；b 子进程协议握手/四字段/能力冲突/版本与帧错误；
//! c 流式映射与 `streaming=false` 降级；d 取消/超时/进程组无孤儿；e 输出/协议错误
//! fail-closed；f L2 回放不 spawn 适配器（pid/日志标记）；g fake external 真链路；
//! h Codex adapter 边界（无 token delta/iterations=1/cached 丢弃/不双重 llm_calls）与
//! DSH 结构 fail-closed。

#![cfg(unix)]

#[path = "cli_support/mod.rs"]
mod cli_support;
#[path = "v03_support/mod.rs"]
mod v03;

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Output;
use std::sync::{Mutex, MutexGuard};
use std::time::Duration;

use serde_json::{json, Value};

use cli_support::{assert_exit, json_stdout};
use v03::{
    agent, assets, fake_llm, llm_call_count, llm_requests, run, run_row, tool_events, write_agent,
};

// ---------------------------------------------------------------------------
// ISSUE-178：真子进程 harness 用例串行化（降低同二进制内进程风暴）
// ---------------------------------------------------------------------------

/// 同一 test binary 的 7 个用例默认并行；每个都会 spawn 真 CLI + Python worker +
/// fake adapter/codex。共享 8 核机器上 7 路进程风暴会把单条 codex 链路从 ~1s 拉到
/// ~30s（实测：6 路并发时 35s，触发默认 30s NODE_TIMEOUT）。用进程内全局锁让本
/// binary 的重子进程用例**串行**执行，消除同二进制自我竞争；跨 binary/跨 agent 的
/// 外部负载仍由环境预算（codex 用例 node_timeout=120s）兜底。断言语义不变。
static SUBPROCESS_SERIAL: Mutex<()> = Mutex::new(());

/// 获取本 binary 的重子进程用例串行锁（锁中毒时仍继续，避免掩盖真实失败）。
fn serial_guard() -> MutexGuard<'static, ()> {
    SUBPROCESS_SERIAL
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

// ---------------------------------------------------------------------------
// 辅助
// ---------------------------------------------------------------------------

/// fake adapter 脚本路径。
fn adapter_script() -> PathBuf {
    v03::fixture("py/fake_harness_adapter.py")
}

/// 仓库内 Codex adapter（真实实现；跨模块只读引用）。
fn codex_adapter() -> PathBuf {
    cli_support::repo_root().join("python/zgraph_worker/adapters/codex.py")
}

/// Python 解释器。
fn python_program() -> String {
    std::env::var("ZGRAPH_PYTHON").unwrap_or_else(|_| "python3".to_string())
}

/// 单条 `text` 脚本响应。
fn text_call(content: &str) -> Value {
    json!({"kind": "text", "content": content, "stop_reason": "EndTurn",
           "usage": {"prompt_tokens": 10, "completion_tokens": 2}})
}

/// 带 `harness` 的 Agent 资产。
fn agent_with_harness(tools: &[&str], harness: Value) -> Value {
    let mut value = agent("parser", tools);
    value["harness"] = harness;
    value
}

/// adapter 子进程的标记文件（pid + 帧日志）。
struct Markers {
    dir: tempfile::TempDir,
    pid: PathBuf,
    log: PathBuf,
}

impl Markers {
    fn new() -> Self {
        let dir = tempfile::tempdir().expect("markers tempdir");
        let pid = dir.path().join("adapter.pid");
        let log = dir.path().join("adapter.log");
        Self { dir, pid, log }
    }

    /// 注入 adapter 的 env。
    fn env(&self) -> Value {
        let _ = self.dir.path();
        json!({
            "ZGRAPH_FAKE_ADAPTER_PID_FILE": self.pid.display().to_string(),
            "ZGRAPH_FAKE_ADAPTER_LOG": self.log.display().to_string(),
        })
    }

    /// 帧日志行数（L2 不 spawn 断言）。
    fn log_lines(&self) -> usize {
        std::fs::read_to_string(&self.log)
            .unwrap_or_default()
            .lines()
            .count()
    }
}

/// subprocess harness 配置。
fn subprocess_harness(mode: &str, markers: &Markers, extra_env: Value) -> Value {
    let mut env = markers.env();
    if let Some(extra) = extra_env.as_object() {
        for (key, value) in extra {
            env[key] = value.clone();
        }
    }
    json!({
        "kind": "subprocess",
        "command": python_program(),
        "args": [adapter_script().display().to_string(), mode],
        "protocol": "zgraph-harness/1",
        "env": env,
    })
}

/// 建资产 + fake LLM 并跑一次（`graph_name` 可为 agent_one/agent_timeout）。
fn harness_run(
    dir: &Path,
    agent_value: &Value,
    calls: Value,
    graph_name: &str,
) -> (Output, PathBuf, PathBuf) {
    let assets_root = assets(dir);
    write_agent(&assets_root, agent_value);
    let script = fake_llm(&assets_root, calls);
    let db = dir.join("trace.db");
    let output = run(dir, &db, graph_name, &assets_root, &script, &[]);
    (output, db, assets_root)
}

/// run_id。
fn payload_run_id(payload: &Value) -> String {
    payload["run_id"].as_str().expect("run_id").to_string()
}

/// `llm_calls.request_json` 归一化（剔除调度浮动 `timeout_ms`）。
fn normalize(requests: Vec<Value>) -> Vec<Value> {
    requests
        .into_iter()
        .map(|mut request| {
            if let Some(object) = request.as_object_mut() {
                object.remove("timeout_ms");
                object.remove("session");
            }
            request
        })
        .collect()
}

/// trace 中 streaming_events 的 kind 序列。
fn streaming_kinds(db: &Path, run_id: &str) -> Vec<String> {
    let conn = cli_support::read_only_connect(db);
    let mut statement = conn
        .prepare("SELECT kind FROM streaming_events WHERE run_id = ?1 ORDER BY seq")
        .expect("streaming kinds");
    statement
        .query_map([run_id], |row| row.get::<_, String>(0))
        .expect("query streaming")
        .map(|row| row.expect("kind"))
        .collect()
}

/// adapter 帧日志。
fn adapter_frames(markers: &Markers) -> Vec<Value> {
    std::fs::read_to_string(&markers.log)
        .unwrap_or_default()
        .lines()
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect()
}

// ---------------------------------------------------------------------------
// AC-20 a：builtin 默认不变 / 显式 builtin 等义
// ---------------------------------------------------------------------------

#[test]
fn ac20a_builtin_default_and_explicit_builtin_unchanged() {
    let _serial = serial_guard();
    let dir = tempfile::tempdir().expect("tempdir");
    // 无 harness 配置（默认 builtin）。
    let baseline_dir = dir.path().join("baseline");
    std::fs::create_dir_all(&baseline_dir).expect("baseline");
    let (baseline_out, baseline_db, _) = harness_run(
        &baseline_dir,
        &agent("parser", &[]),
        json!([text_call("same")]),
        "agent_one.json",
    );
    assert_exit(&baseline_out, 0, "默认 builtin run");
    let baseline_payload = json_stdout(&baseline_out);
    assert_eq!(baseline_payload["status"], json!("Completed"));
    let baseline_run = payload_run_id(&baseline_payload);

    // 显式 `harness: {"kind":"builtin"}`。
    let explicit_dir = dir.path().join("explicit");
    std::fs::create_dir_all(&explicit_dir).expect("explicit");
    let (explicit_out, explicit_db, _) = harness_run(
        &explicit_dir,
        &agent_with_harness(&[], json!({"kind": "builtin"})),
        json!([text_call("same")]),
        "agent_one.json",
    );
    assert_exit(&explicit_out, 0, "显式 builtin run");
    let explicit_payload = json_stdout(&explicit_out);
    assert_eq!(explicit_payload["status"], json!("Completed"));
    let explicit_run = payload_run_id(&explicit_payload);

    assert_eq!(
        normalize(llm_requests(&baseline_db, &baseline_run)),
        normalize(llm_requests(&explicit_db, &explicit_run)),
        "显式 builtin 与默认逐字节等义（除剩余 timeout）"
    );
    assert_eq!(
        baseline_payload["run_output"], explicit_payload["run_output"],
        "输出一致"
    );
    let (_, _, _, baseline_hash, _, _) = run_row(&baseline_db, &baseline_run);
    let (_, _, _, explicit_hash, _, _) = run_row(&explicit_db, &explicit_run);
    assert_eq!(
        baseline_hash, explicit_hash,
        "harness 配置不参与 graph_hash"
    );
    // 无外部进程：默认/显式 builtin 不产生 adapter 标记。
    for db in [&baseline_db, &explicit_db] {
        let run_id = if std::ptr::eq(db, &baseline_db) {
            &baseline_run
        } else {
            &explicit_run
        };
        assert_eq!(llm_call_count(db, run_id), 1, "builtin 每轮 1 条 llm_calls");
    }
    // L1 replay 不变。
    let trace = v03::trace_json(&baseline_db, &baseline_run);
    let replay = v03::replay_events_json(&baseline_db, &baseline_run);
    assert_eq!(replay.stdout, trace.stdout);
}

// ---------------------------------------------------------------------------
// AC-20 b/e：子进程协议（握手/四字段/流式/错误 fail-closed/能力冲突）
// ---------------------------------------------------------------------------

#[test]
fn ac20b_subprocess_protocol_runs_and_records_four_fields() {
    let _serial = serial_guard();
    let dir = tempfile::tempdir().expect("tempdir");
    let markers = Markers::new();
    let (output, db, _) = harness_run(
        dir.path(),
        &agent_with_harness(&[], subprocess_harness("echo", &markers, json!({}))),
        json!([text_call("unused")]),
        "agent_one.json",
    );
    assert_exit(&output, 0, "subprocess adapter run");
    let payload = json_stdout(&output);
    assert_eq!(payload["status"], json!("Completed"));
    let run_id = payload_run_id(&payload);
    // 四字段（外部 adapter result）按 AgentOutput 形状返回。
    let result = &payload["run_output"]["result"];
    assert_eq!(result["content"], json!("final-text"));
    assert_eq!(result["iterations"], json!(1), "iterations 固定 1");
    assert_eq!(result["stop_reason"], json!("EndTurn"));
    let usage = &result["usage"];
    assert_eq!(usage["prompt_tokens"], json!(11));
    assert_eq!(usage["completion_tokens"], json!(7));
    assert_eq!(usage["reasoning_tokens"], json!(3));
    // 框架恰写 1 条 llm_calls（不双重记录）。
    assert_eq!(
        llm_call_count(&db, &run_id),
        1,
        "外部 harness 不双重写 llm_calls"
    );
    let calls: Vec<(String, Option<String>)> = {
        let conn = cli_support::read_only_connect(&db);
        let mut statement = conn
            .prepare("SELECT request_json, response_json FROM llm_calls WHERE run_id = ?1")
            .expect("llm_calls");
        statement
            .query_map([&run_id], |row| Ok((row.get(0)?, row.get(1)?)))
            .expect("query")
            .map(|row| row.expect("row"))
            .collect()
    };
    let request: Value = serde_json::from_str(&calls[0].0).expect("request json");
    assert!(
        request.get("provider").is_some() || request.get("agent_id").is_some(),
        "外部 harness request 快照含 provider/agent_id: {request}"
    );
    let response: Value =
        serde_json::from_str(calls[0].1.as_deref().expect("response json")).expect("response");
    assert_eq!(response["content"], json!("final-text"));
    // streaming 映射：token×2 + reasoning + tool_start/tool_end + progress。
    let kinds = streaming_kinds(&db, &run_id);
    assert!(kinds.iter().any(|kind| kind == "token"), "{kinds:?}");
    assert!(kinds.iter().any(|kind| kind == "reasoning"), "{kinds:?}");
    assert!(
        kinds.iter().any(|kind| kind == "tool_call_start"),
        "{kinds:?}"
    );
    assert!(
        kinds.iter().any(|kind| kind == "tool_call_end"),
        "{kinds:?}"
    );
    assert!(kinds.iter().any(|kind| kind == "progress"), "{kinds:?}");
    // 握手帧：协议/limits；run 帧带 agent/session。
    let frames = adapter_frames(&markers);
    let hello = frames
        .iter()
        .find(|frame| frame["dir"] == json!("in") && frame["payload"]["type"] == json!("hello"))
        .expect("hello frame");
    assert_eq!(hello["payload"]["protocol"], json!("zgraph-harness/1"));
    assert!(hello["payload"]["limits"]["max_output_bytes"]
        .as_u64()
        .is_some());
    let run_frame = frames
        .iter()
        .find(|frame| frame["dir"] == json!("in") && frame["payload"]["type"] == json!("run"))
        .expect("run frame");
    assert_eq!(run_frame["payload"]["agent"]["id"], json!("parser"));
    assert!(run_frame["payload"]["session"]["reuse_key"].is_string());
}

#[test]
fn ac20b_capability_conflict_and_frame_errors_are_fail_closed() {
    let _serial = serial_guard();
    let dir = tempfile::tempdir().expect("tempdir");
    // ① 能力冲突：subprocess + tools 非空 → HARNESS_CONFIG_LOAD{provider_capability_conflict}。
    let conflict_dir = dir.path().join("conflict");
    std::fs::create_dir_all(&conflict_dir).expect("conflict dir");
    let markers = Markers::new();
    let (output, db, _) = harness_run(
        &conflict_dir,
        &agent_with_harness(
            &["some_tool"],
            subprocess_harness("echo", &markers, json!({})),
        ),
        json!([text_call("x")]),
        "agent_one.json",
    );
    assert_exit(&output, 1, "能力冲突 fail-closed");
    let payload = json_stdout(&output);
    let run_id = payload_run_id(&payload);
    let (_, code, message, ..) = run_row(&db, &run_id);
    assert_eq!(code.as_deref(), Some("HARNESS_CONFIG_LOAD"));
    assert!(
        message
            .as_deref()
            .is_some_and(|text| text.contains("provider_capability_conflict")),
        "{message:?}"
    );

    // ② 协议/帧错误 → HARNESS_PLUGIN_ERROR{reason}。
    let cases = [
        ("version_bad", "version"),
        ("bad_json", "handshake"),
        ("big_line", "line_too_large"),
        ("bad_result", "result_invalid"),
        ("exit_nonzero", "exit_nonzero"),
        ("stderr_big", "exit_nonzero"),
    ];
    for (mode, reason) in cases {
        let case_dir = dir.path().join(mode);
        std::fs::create_dir_all(&case_dir).expect("case dir");
        let markers = Markers::new();
        let (output, db, _) = harness_run(
            &case_dir,
            &agent_with_harness(&[], subprocess_harness(mode, &markers, json!({}))),
            json!([text_call("x")]),
            "agent_one.json",
        );
        assert_exit(&output, 1, mode);
        let payload = json_stdout(&output);
        let run_id = payload_run_id(&payload);
        let (_, code, message, ..) = run_row(&db, &run_id);
        assert_eq!(code.as_deref(), Some("HARNESS_PLUGIN_ERROR"), "{mode}");
        assert!(
            message.as_deref().is_some_and(|text| text.contains(reason)),
            "{mode}: 期望 reason {reason}: {message:?}"
        );
        // stderr 截断：错误 detail 不含 12k stderr 全文。
        if mode == "stderr_big" {
            assert!(
                message.as_deref().is_some_and(|text| text.len() < 12_000),
                "{mode}: stderr 应截断≤8KiB: {message:?}"
            );
        }
        // adapter 进程（若已启动）在 run 结束后回收。
        if let Ok(pid_text) = std::fs::read_to_string(&markers.pid) {
            if let Ok(pid) = pid_text.trim().parse::<i32>() {
                assert!(
                    v03::wait_pid_gone(pid, Duration::from_secs(5)),
                    "{mode}: adapter pid {pid} 应被回收"
                );
            }
        }
    }
}

// ---------------------------------------------------------------------------
// AC-20 c：流式映射 / `streaming=false` 降级
// ---------------------------------------------------------------------------

#[test]
fn ac20c_streaming_false_ignores_delta_events() {
    let _serial = serial_guard();
    let dir = tempfile::tempdir().expect("tempdir");
    let markers = Markers::new();
    let (output, db, _) = harness_run(
        dir.path(),
        &agent_with_harness(&[], subprocess_harness("no_stream", &markers, json!({}))),
        json!([text_call("x")]),
        "agent_one.json",
    );
    assert_exit(&output, 0, "streaming=false adapter run");
    let payload = json_stdout(&output);
    assert_eq!(payload["status"], json!("Completed"));
    assert_eq!(
        payload["run_output"]["result"]["content"],
        json!("final-text")
    );
    let run_id = payload_run_id(&payload);
    let kinds = streaming_kinds(&db, &run_id);
    assert!(
        !kinds.iter().any(|kind| kind == "token"),
        "capabilities.streaming=false → 忽略 delta/不产生 TokenDelta: {kinds:?}"
    );
    assert!(
        tool_events(&db, &run_id).is_empty(),
        "无 tool 事件: {kinds:?}"
    );
}

// ---------------------------------------------------------------------------
// AC-20 d：取消 / 超时 / 进程组无孤儿
// ---------------------------------------------------------------------------

#[test]
fn ac20d_timeout_and_cancel_leave_no_adapter_process() {
    let _serial = serial_guard();
    let dir = tempfile::tempdir().expect("tempdir");
    // ① 节点超时（agent_timeout.json：timeout_ms=1200）→ NODE_TIMEOUT，adapter 被 kill。
    let timeout_dir = dir.path().join("timeout");
    std::fs::create_dir_all(&timeout_dir).expect("timeout dir");
    let markers = Markers::new();
    let (output, db, _) = harness_run(
        &timeout_dir,
        &agent_with_harness(&[], subprocess_harness("silent", &markers, json!({}))),
        json!([text_call("x")]),
        "agent_timeout.json",
    );
    assert_exit(&output, 1, "节点超时 → run Failed");
    let payload = json_stdout(&output);
    assert_eq!(payload["error"]["code"], json!("NODE_TIMEOUT"));
    let run_id = payload_run_id(&payload);
    let pid_text = v03::wait_file(&markers.pid, Duration::from_secs(10), "adapter pid");
    let pid: i32 = pid_text.trim().parse().expect("pid");
    assert!(
        v03::wait_pid_gone(pid, Duration::from_secs(5)),
        "超时后 adapter 进程组应被 kill（pid {pid}）"
    );
    let (status, code, _) = v03::instance(&db, &run_id, "agent");
    assert_eq!(status, "TimedOut");
    assert_eq!(code.as_deref(), Some("NODE_TIMEOUT"));

    // ② 取消：adapter silent 在飞时 SIGINT → Cancelled + 无孤儿。
    let cancel_dir = dir.path().join("cancel");
    std::fs::create_dir_all(&cancel_dir).expect("cancel dir");
    let markers = Markers::new();
    let assets_root = assets(&cancel_dir);
    write_agent(
        &assets_root,
        &agent_with_harness(&[], subprocess_harness("silent", &markers, json!({}))),
    );
    let script = fake_llm(&assets_root, json!([text_call("x")]));
    let db = cancel_dir.join("trace.db");
    let run_child = v03::RunChild::spawn(
        &cancel_dir,
        &db,
        "agent_one.json",
        &assets_root,
        &script,
        &[],
    );
    let pid_text = v03::wait_file(&markers.pid, Duration::from_secs(10), "adapter pid");
    let adapter_pid: i32 = pid_text.trim().parse().expect("pid");
    v03::send_sigint(run_child.pid());
    let output = run_child.wait("harness cancel run");
    assert_exit(&output, 130, "SIGINT → exit 130");
    assert_eq!(json_stdout(&output)["status"], json!("Cancelled"));
    assert!(
        v03::wait_pid_gone(adapter_pid, Duration::from_secs(5)),
        "取消后 adapter 进程应被 kill（pid {adapter_pid}）"
    );
}

// ---------------------------------------------------------------------------
// AC-20 f：L2 回放不 spawn 外部进程
// ---------------------------------------------------------------------------

#[test]
fn ac20f_l2_replay_never_spawns_adapter() {
    let _serial = serial_guard();
    let dir = tempfile::tempdir().expect("tempdir");
    let markers = Markers::new();
    let (output, db, _) = harness_run(
        dir.path(),
        &agent_with_harness(&[], subprocess_harness("echo", &markers, json!({}))),
        json!([text_call("x")]),
        "agent_one.json",
    );
    assert_exit(&output, 0, "外部 harness 原 run");
    let payload = json_stdout(&output);
    let run_id = payload_run_id(&payload);
    assert_eq!(payload["status"], json!("Completed"), "record=full 供 L2");
    let frames_before = markers.log_lines();
    assert!(
        frames_before >= 2,
        "原 run 有 hello/run 帧: {frames_before}"
    );

    // L1：不启动进程（帧日志不变）。
    let trace = v03::trace_json(&db, &run_id);
    let replay_events = v03::replay_events_json(&db, &run_id);
    assert_exit(&replay_events, 0, "replay events");
    assert_eq!(replay_events.stdout, trace.stdout, "L1 replay 逐字节一致");
    assert_eq!(
        markers.log_lines(),
        frames_before,
        "L1 回放不 spawn adapter"
    );

    // L2：outputs 回放 matched；不 spawn。
    let replay_outputs = v03::replay_outputs_json(&db, &run_id);
    assert_exit(&replay_outputs, 0, "replay outputs");
    let l2 = json_stdout(&replay_outputs);
    assert_eq!(l2["matched"], json!(true));
    assert_eq!(l2["original_output"], l2["replayed_output"]);
    assert_eq!(
        markers.log_lines(),
        frames_before,
        "L2 回放不 spawn adapter"
    );
    assert!(markers.pid.is_file(), "原 run 的 pid 标记保留");
}

// ---------------------------------------------------------------------------
// AC-20 h：Codex adapter（假 codex PATH 注入）/ DSH 结构 fail-closed
// ---------------------------------------------------------------------------

#[test]
fn ac20h_codex_adapter_via_fake_binary_and_dsh_structure() {
    let _serial = serial_guard();
    let dir = tempfile::tempdir().expect("tempdir");
    // ISSUE-178 加固：每用例独立 PATH/CODEX_HOME/TMPDIR，不继承调用者 PATH
    //（避免并行用例/宿主环境影响 fake codex 解析）；`python3` 用本测试 python 的
    // 符号链接放进 bin/，保证 fake codex 的 `#!/usr/bin/env python3` 确定性解析。
    let bin_dir = dir.path().join("bin");
    let codex_home = dir.path().join("codex-home");
    let tmp_dir = dir.path().join("tmp");
    std::fs::create_dir_all(&bin_dir).expect("bin dir");
    std::fs::create_dir_all(&codex_home).expect("codex home");
    std::fs::create_dir_all(&tmp_dir).expect("tmp dir");
    let codex = bin_dir.join("codex");
    std::fs::copy(v03::fixture("py/fake_codex.py"), &codex).expect("copy fake codex");
    std::fs::set_permissions(&codex, std::fs::Permissions::from_mode(0o755))
        .expect("chmod fake codex");
    std::os::unix::fs::symlink(python_program(), bin_dir.join("python3"))
        .expect("link python3 into isolated PATH bin");
    let pid_file = dir.path().join("codex.pid");
    // 只含隔离 bin + 系统只读目录：不泄漏宿主/其它用例 PATH（ISSUE-178 隔离）。
    let isolated_path = format!("{}:/usr/bin:/bin:/usr/sbin:/sbin", bin_dir.display());
    let harness = json!({
        "kind": "subprocess",
        "command": python_program(),
        "args": [codex_adapter().display().to_string()],
        "protocol": "zgraph-harness/1",
        "env": {
            "PATH": isolated_path,
            "CODEX_HOME": codex_home.display().to_string(),
            "TMPDIR": tmp_dir.display().to_string(),
            "ZGRAPH_FAKE_CODEX_PID_FILE": pid_file.display().to_string(),
        },
    });
    // 环境预算：codex adapter 链路会 spawn 2 个 Python + fake codex；并行负载下
    // 实测 25–35s（6 路并发 35s），默认 30s node timeout 会误报 NODE_TIMEOUT
    //（ISSUE-178）。本用例只放宽**环境预算**到 120s，仍断言必须成功（不跳过/不伪造）。
    let mut codex_agent = agent_with_harness(&[], harness);
    codex_agent["node_timeout_ms"] = json!(120_000);
    let (output, db, _) = harness_run(
        dir.path(),
        &codex_agent,
        json!([text_call("unused")]),
        "agent_one.json",
    );
    assert_exit(&output, 0, "Codex adapter 真链路（假 codex）");
    let payload = json_stdout(&output);
    assert_eq!(payload["status"], json!("Completed"));
    let result = &payload["run_output"]["result"];
    assert_eq!(result["content"], json!("codex-final"), "取 -o 最终消息");
    assert_eq!(result["iterations"], json!(1), "Codex iterations 固定 1");
    let usage = &result["usage"];
    assert_eq!(usage["prompt_tokens"], json!(10));
    assert_eq!(usage["completion_tokens"], json!(5));
    assert_eq!(usage["reasoning_tokens"], json!(2));
    assert!(
        usage.get("cached_input_tokens").is_none()
            && usage.get("cache_write_input_tokens").is_none(),
        "cached tokens v0.3 丢弃: {usage}"
    );
    let run_id = payload_run_id(&payload);
    // Codex streaming=false：只发 result、不产生 token delta。
    let kinds = streaming_kinds(&db, &run_id);
    assert!(
        !kinds
            .iter()
            .any(|kind| kind == "token" || kind == "reasoning"),
        "Codex 无 token delta: {kinds:?}"
    );
    assert_eq!(llm_call_count(&db, &run_id), 1, "不双重写 llm_calls");
    // 进程回收（有界等待 15s：并行负载下 Python/codex 退出可能排队；仍断言已回收）。
    let pid_text = v03::wait_file(&pid_file, Duration::from_secs(15), "codex pid file");
    let codex_pid: i32 = pid_text.trim().parse().expect("codex pid");
    assert!(
        v03::wait_pid_gone(codex_pid, Duration::from_secs(15)),
        "codex 子进程应被回收（pid {codex_pid}）"
    );

    // DSH ACP 结构：无真实 dsh 环境 → 命令不存在时 fail-closed spawn_failed
    // （release notes 标注「adapter 骨架，待真实环境验证」）。
    let dsh_dir = dir.path().join("dsh");
    std::fs::create_dir_all(&dsh_dir).expect("dsh dir");
    let markers = Markers::new();
    let missing = json!({
        "kind": "subprocess",
        "command": "zgraph-e2e-no-such-dsh-binary",
        "args": [],
        "protocol": "zgraph-harness/1",
        "env": markers.env(),
    });
    let (output, db, _) = harness_run(
        &dsh_dir,
        &agent_with_harness(&[], missing),
        json!([text_call("x")]),
        "agent_one.json",
    );
    assert_exit(&output, 1, "无 dsh 二进制 → fail-closed");
    let payload = json_stdout(&output);
    let run_id = payload_run_id(&payload);
    let (_, code, message, ..) = run_row(&db, &run_id);
    assert_eq!(code.as_deref(), Some("HARNESS_PLUGIN_ERROR"));
    assert!(
        message
            .as_deref()
            .is_some_and(|text| text.contains("spawn_failed")),
        "reason spawn_failed: {message:?}"
    );
}
