//! AC-08（M4 真链路口径）：fake LLM + tool loop + token 流式入 trace + 工具异常/超时语义。
//!
//! 语义基准：`doc/spec-v1.2/07-验收用例.md` AC-08、`doc/spec-v1.2/05-AgentHarness.md`
//! §5.3.4（fake provider 脚本）、§5.6.5/§5.6.6（工具执行与错误语义）、§5.8（流式事件
//! 1:1 emit + 06 写线程聚合）、§5.9（`agent_call` 输出四字段）。
//!
//! 数据通路（**禁止 mock** kernel / worker / trace）：
//!
//! ```text
//! tests/fixtures/graphs/harness_ac08/graph.json
//!   → CompiledGraph::compile → kernel start_run（真调度器）
//!   → RoutingExecutor: CompiledHandler::Agent → HarnessExecutor（真 Harness 循环）
//!                     其余（本图无 python_node）→ WorkerExecutor
//!   → Harness 每个 tool 调用 = 真 Python worker 子进程
//!     （`python -m zgraph_worker.runtime` + `zgraph_worker.tool_runner.call`）
//!   → zgraph_cli::bridge::TraceBridge → 真 SQLite trace → TraceReader
//! ```
//!
//! 用例：
//! - [`ac08a_fake_llm_tool_loop_streams_tokens_into_trace`]：两轮 tool loop，断言输出四字段
//!   形状、token 段拼接 == content、`tool_call_start/end`、`progress` 每轮一条、无 `TOOL_ERROR`；
//! - [`tool_exception_is_fed_back_and_run_continues`]：工具抛异常被 JSON 化喂回 LLM，run 继续；
//! - [`tool_timeout_is_fed_back_and_run_continues`]：工具声明 `timeout_ms=300` 超时后杀进程组，
//!   `TOOL_ERROR{reason:"timeout"}` 喂回，run 继续且整个用例 < 15s；
//! - [`unknown_tool_crashes_instance`]：LLM 调用未注册工具 → `HARNESS_UNKNOWN_TOOL` → Crashed；
//! - [`agent_config_node_timeout_hint_backs_off_when_dsl_omits_timeout`]：ISSUE-076 回退链 +
//!   ISSUE-086 实例级超时错误（TimedOut、`instances.error_code=NODE_TIMEOUT`、error JSON 非 NULL）；
//! - [`ac08b_real_llm_manual`]：`#[ignore]`，真实 DeepSeek 手动验收（AC-08 真实模型部分）。
//!
//! 手动执行真实模型用例：
//!
//! ```bash
//! CARGO_HOME=<repo>/.cargo-home ZGRAPH_PYTHON=<repo>/.venv/bin/python \
//! ZGRAPH_REAL_LLM=1 DEEPSEEK_API_KEY=... \
//!   cargo test -p e2e --test harness_ac08 -- --ignored --nocapture
//! ```
//!
//! 稳定性约定（AGENTS §8）：时序只用轮询/宽松上限；每个用例独立 `tempfile::TempDir`；
//! fixture 的 `fake_llm.json` 按 05 §5.3.4 脚本 schema 驱动两轮请求。

#![cfg(unix)]

#[path = "common/mod.rs"]
mod common;

use std::future::Future;
use std::path::Path;
use std::pin::Pin;
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use zgraph_cli::bridge::BridgeOutcome;
use zgraph_harness::{
    FakeLlmProvider, GenaiLlmProvider, HarnessExecutor, HarnessSettings, LlmMessage, LlmProvider,
    LlmRequest, WorkerToolRunner,
};
use zgraph_kernel::{
    CompiledGraph, CompiledHandler, CompiledNode, NodeError, NodeExecution, NodeExecutor, RunStatus,
};
use zgraph_trace::{InstanceStatus as TraceInstanceStatus, StreamingRow, TraceReader};

/// 单个用例的宽松上限（真 worker 冷启动 + CI 慢机余量；时序断言不猜 sleep）。
const CASE_TIMEOUT: Duration = Duration::from_secs(30);

/// AC-08c 的硬上限：工具超时 300ms 生效后 run 必须很快继续，整例 < 15s。
const TOOL_TIMEOUT_CASE_LIMIT: Duration = Duration::from_secs(15);

/// `slow` 工具 fixture 声明的工具级超时（毫秒，05 §5.6.1 `ToolDef.timeout_ms`）。
const SLOW_TOOL_TIMEOUT_MS: u64 = 300;

/// AC-08 验收图在 `tests/fixtures/graphs/` 下的目录名。
const FIXTURE_GRAPH: &str = "harness_ac08";

/// agent 节点 id（与 `graph.json` 一致）与 AgentConfig `id`。
const AGENT_NODE_ID: &str = "answer_writer";

// ---------------------------------------------------------------------------
// 执行器：agent_call 走 Harness，其余走真 worker
// ---------------------------------------------------------------------------

/// 组合执行器：`CompiledHandler::Agent{..}` → [`HarnessExecutor`]，其余 → `WorkerExecutor`。
///
/// `NodeExecution.node.handler` 是 `pub`（kernel 编译产物），这里只做路由，不改变
/// 任何执行语义；`start`/`end` 由内核内置处理，不会进入本执行器。
struct RoutingExecutor {
    harness: Arc<HarnessExecutor>,
    worker: Arc<zgraph_worker::WorkerExecutor>,
}

impl NodeExecutor for RoutingExecutor {
    fn execute<'a>(
        &'a self,
        execution: NodeExecution<'a>,
    ) -> Pin<Box<dyn Future<Output = Result<Value, NodeError>> + Send + 'a>> {
        let is_agent = matches!(&execution.node.handler, CompiledHandler::Agent { .. });
        if is_agent {
            self.harness.execute(execution)
        } else {
            self.worker.execute(execution)
        }
    }

    /// ISSUE-076：必须转发内层执行器的 `timeout_hint`；否则包装层默认 `None` 会
    /// 截断 harness 的 `AgentConfig.node_timeout_ms` 回退，端到端不生效。
    fn timeout_hint(&self, node: &CompiledNode) -> Option<u64> {
        if matches!(&node.handler, CompiledHandler::Agent { .. }) {
            self.harness.timeout_hint(node)
        } else {
            self.worker.timeout_hint(node)
        }
    }
}

// ---------------------------------------------------------------------------
// fixture 路径与执行器构造
// ---------------------------------------------------------------------------

/// `tests/fixtures/graphs/harness_ac08`（agent 资产 + tools + fake 脚本）。
fn fixture_dir() -> std::path::PathBuf {
    common::graphs_root().join(FIXTURE_GRAPH)
}

/// 验收图路径（`compile_fixture` 的相对名）。
fn fixture_graph_name() -> String {
    format!("{FIXTURE_GRAPH}/graph.json")
}

/// agent 资产根（`<assets_dir>/agents/*.json`）。
fn assets_dir() -> std::path::PathBuf {
    fixture_dir()
}

/// 工具目录（真 worker `zgraph_worker.tool_runner.discover/call` 的 `tools_dir`）。
fn tools_dir() -> std::path::PathBuf {
    fixture_dir().join("tools")
}

/// 按 05 §5.3.4 脚本 JSON 构造 fake provider。
fn fake_provider(script: Value) -> Arc<FakeLlmProvider> {
    Arc::new(FakeLlmProvider::from_script(script).expect("fake-llm script 应合法"))
}

/// 从 `tests/fixtures/graphs/harness_ac08/fake_llm.json` 构造 fake provider。
fn fake_provider_from_fixture() -> Arc<FakeLlmProvider> {
    let path = fixture_dir().join("fake_llm.json");
    Arc::new(
        FakeLlmProvider::from_path(&path)
            .unwrap_or_else(|error| panic!("读取 fake-llm 脚本 {} 失败: {error}", path.display())),
    )
}

/// 构造「Harness 处理 agent_call + 真 worker 处理其余 handler」的组合执行器。
fn routing_executor(provider: Arc<dyn LlmProvider>, fake_mode: bool) -> Arc<RoutingExecutor> {
    let settings = HarnessSettings {
        assets_dir: assets_dir(),
        tools_dir: tools_dir(),
        provider,
        fake_mode,
        // 工具执行 = 真 Python worker 子进程（每次调用一进程，D3）。
        tool_runner: Arc::new(WorkerToolRunner::new(common::worker_config(
            common::RUN_TIMEOUT,
        ))),
    };
    Arc::new(RoutingExecutor {
        harness: Arc::new(HarnessExecutor::new(settings)),
        worker: common::executor(common::RUN_TIMEOUT),
    })
}

/// 用 AC-08 图跑一次 run 并等待 trace 落盘（轮询/超时由 bridge 与内核保证）。
async fn run_ac08(executor: Arc<RoutingExecutor>, db: &std::path::Path) -> BridgeOutcome {
    let graph = common::compile_fixture(&fixture_graph_name());
    let inputs = common::entry_inputs(&[(
        "start",
        "message",
        json!("请计算 2 + 3 的结果，并用 JSON {\"answer\":\"<结果>\"} 回答。"),
    )]);
    let spec = common::spec(graph, inputs);
    tokio::time::timeout(CASE_TIMEOUT, common::run_to_trace(spec, executor, db))
        .await
        .expect("AC-08 run 必须在宽松上限内结束")
}

// ---------------------------------------------------------------------------
// trace / 请求断言辅助
// ---------------------------------------------------------------------------

/// 某 run 的全部流式段（06 §6.2.9；按 instance/kind/segment_index 升序）。
fn streaming_segments(reader: &TraceReader, run_id: &str) -> Vec<StreamingRow> {
    reader
        .streaming_events(run_id)
        .expect("query streaming_events")
}

/// 过滤指定 `kind` 的流式段。
fn segments_of_kind<'a>(segments: &'a [StreamingRow], kind: &str) -> Vec<&'a StreamingRow> {
    segments
        .iter()
        .filter(|segment| segment.kind == kind)
        .collect()
}

/// A2：`token` 段按 `segment_index` 拼接（05 §5.8.1 不变量 / §5.8.3 聚合语义）。
fn token_text(segments: &[StreamingRow]) -> String {
    let mut tokens: Vec<&StreamingRow> = segments_of_kind(segments, "token");
    tokens.sort_by_key(|segment| segment.segment_index);
    tokens
        .iter()
        .filter_map(|segment| segment.text.clone())
        .collect::<Vec<_>>()
        .join("")
}

/// 取 `agent_call` 实例的整包 output（05 §5.9.2 四字段形状）。
fn agent_output(reader: &TraceReader, run_id: &str) -> Value {
    let instance = common::instance(reader, run_id, AGENT_NODE_ID);
    assert_eq!(
        instance.status,
        TraceInstanceStatus::Completed,
        "{AGENT_NODE_ID} 实例应 Completed"
    );
    assert_eq!(
        instance.error_code, None,
        "{AGENT_NODE_ID} 实例不应有错误码"
    );
    instance
        .output_json
        .expect("agent_call 实例应有 output_json")
}

/// 一条工具结果消息（`LlmMessage::Tool`）的解析结果。
struct ToolMessage {
    call_id: String,
    name: String,
    payload: Value,
}

/// 从请求里取全部 `Tool` 消息（05 §5.4.1：工具结果 JSON 字符串）。
fn tool_messages(request: &LlmRequest) -> Vec<ToolMessage> {
    request
        .messages
        .iter()
        .filter_map(|message| {
            let LlmMessage::Tool {
                tool_call_id,
                name,
                content,
            } = message
            else {
                return None;
            };
            let payload: Value = serde_json::from_str(content).unwrap_or_else(|error| {
                panic!("Tool 消息 content 必须是 JSON（{error}）：{content}")
            });
            Some(ToolMessage {
                call_id: tool_call_id.clone(),
                name: name.clone(),
                payload,
            })
        })
        .collect()
}

/// 从请求里取全部 assistant 轮次声明的 tool_call 名（顺序即执行顺序）。
fn assistant_tool_names(request: &LlmRequest) -> Vec<String> {
    request
        .messages
        .iter()
        .flat_map(|message| match message {
            LlmMessage::Assistant { tool_calls, .. } => tool_calls
                .iter()
                .map(|call| call.name.clone())
                .collect::<Vec<_>>(),
            _ => Vec::new(),
        })
        .collect()
}

// ---------------------------------------------------------------------------
// AC-08a：fake LLM tool loop + token 流式入 trace
// ---------------------------------------------------------------------------

/// 断言 AC-08a 的 trace 语义：token 拼接、tool_call_start/end、progress、无 TOOL_ERROR。
fn assert_ac08a_trace(reader: &TraceReader, run_id: &str, content: &str) {
    let segments = streaming_segments(reader, run_id);

    // A2：token 段按 segment_index 拼接 == 最终 content（06 写线程 100ms 聚合）。
    let joined = token_text(&segments);
    assert_eq!(joined, content, "token 段拼接应等于 content");
    assert!(!joined.is_empty(), "token 段非空");

    // tool_call_start / tool_call_end：ok=true、tool_name=add、call_id 一致（A3）。
    let starts = segments_of_kind(&segments, "tool_call_start");
    let ends = segments_of_kind(&segments, "tool_call_end");
    let add_starts: Vec<&StreamingRow> = starts
        .iter()
        .copied()
        .filter(|segment| segment.tool_name.as_deref() == Some("add"))
        .collect();
    let add_ends: Vec<&StreamingRow> = ends
        .iter()
        .copied()
        .filter(|segment| segment.tool_name.as_deref() == Some("add"))
        .collect();
    assert!(!add_starts.is_empty(), "tool_call_start(add) 至少一条");
    assert!(!add_ends.is_empty(), "tool_call_end(add) 至少一条");
    assert!(
        add_ends.iter().all(|segment| segment.ok == Some(true)),
        "add 工具应成功（ok=true）"
    );
    assert_eq!(
        add_starts[0].call_id.as_deref(),
        add_ends[0].call_id.as_deref(),
        "A3：ToolCallStart 与 ToolCallEnd 的 call_id 必须一致"
    );
    // 无 TOOL_ERROR：所有工具结束事件都成功，run/实例错误码为空（上面已断言 run 行）。
    assert!(
        ends.iter().all(|segment| segment.ok == Some(true)),
        "不应出现失败工具调用（TOOL_ERROR）"
    );

    // A5：每个 Invoking 入口恰一条 Progress，message/percent 按 05 §5.8.2。
    let progress = segments_of_kind(&segments, "progress");
    assert_eq!(progress.len(), 2, "两轮 LLM 请求 → 两条 progress");
    let messages: Vec<&str> = progress
        .iter()
        .filter_map(|segment| segment.text.as_deref())
        .collect();
    assert_eq!(messages, vec!["llm call 1/4", "llm call 2/4"]);
    let percents: Vec<i64> = progress
        .iter()
        .filter_map(|segment| segment.percent)
        .collect();
    assert_eq!(percents, vec![25, 50], "percent = floor(iter*100/max)");
}

#[tokio::test]
async fn ac08a_fake_llm_tool_loop_streams_tokens_into_trace() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db = dir.path().join("ac08a.db");
    let fake = fake_provider_from_fixture();
    let bridge = run_ac08(routing_executor(fake.clone(), true), &db).await;

    // run：Completed，无错误。
    assert_eq!(bridge.outcome.status, RunStatus::Completed);
    assert!(bridge.outcome.error.is_none(), "Completed run 无 error");
    let run_id = bridge.outcome.run_id.clone();
    let reader = bridge.reader();
    common::assert_lifecycle_shape(reader, &run_id);
    let run = common::run_row(reader, &run_id);
    assert_eq!(run.status, zgraph_trace::RunStatus::Completed);
    assert_eq!(run.error_code, None);

    // 实例 output：四字段形状（05 §5.9.2）。
    let output = agent_output(reader, &run_id);
    assert_eq!(output["content"], json!("{\"answer\":\"5\"}"));
    assert_eq!(output["stop_reason"], json!("EndTurn"));
    assert_eq!(output["iterations"], json!(2), "两轮请求（tool loop）");
    let usage = output.get("usage").expect("usage 字段");
    let prompt = usage["prompt_tokens"].as_u64().expect("prompt_tokens");
    let completion = usage["completion_tokens"]
        .as_u64()
        .expect("completion_tokens");
    let total = usage["total_tokens"].as_u64().expect("total_tokens");
    assert!(total > 0, "usage.total_tokens 必须 > 0");
    assert_eq!(total, prompt + completion, "total = prompt + completion");
    assert_eq!(usage["source"], json!("provider"), "脚本显式 usage");

    // 流式事件语义（token/tool_call/progress）。
    assert_ac08a_trace(reader, &run_id, "{\"answer\":\"5\"}");

    // LLM 请求：两轮，tools schema 顺序与 AgentConfig.tools 一致（05 §5.6.3）。
    let requests = fake.requests();
    assert_eq!(requests.len(), 2, "两轮 LLM 请求");
    let tool_names: Vec<&str> = requests[0]
        .tools
        .iter()
        .map(|schema| schema.name.as_str())
        .collect();
    assert_eq!(tool_names, vec!["add", "explode", "slow"]);
    // 第二次请求含第一轮 tool 结果消息（add 真执行过：2+3=5）。
    let tool_results = tool_messages(&requests[1]);
    assert_eq!(tool_results.len(), 1, "第二次请求带一条 Tool 消息");
    assert_eq!(tool_results[0].name, "add");
    assert_eq!(tool_results[0].call_id, "call_add_1");
    assert_eq!(tool_results[0].payload["sum"], json!(5));
}

// ---------------------------------------------------------------------------
// 工具异常/超时语义（任务用例 b/c）：错误喂回 LLM，run 继续
// ---------------------------------------------------------------------------

/// 两轮脚本：第 1 轮调用指定工具（args），第 2 轮返回文本 `{"answer":"ok"}`。
fn two_call_script(tool_name: &str, args: Value) -> Value {
    json!({
        "version": 1,
        "chunk_chars": 3,
        "chunk_delay_ms": 0,
        "calls": [
            {
                "kind": "tool_calls",
                "content": null,
                "stop_reason": "ToolCalls",
                "tool_calls": [
                    {"call_id": format!("call_{tool_name}_1"), "name": tool_name, "args": args}
                ],
                "usage": {"prompt_tokens": 90, "completion_tokens": 6}
            },
            {
                "kind": "text",
                "content": "{\"answer\":\"ok\"}",
                "stop_reason": "EndTurn",
                "usage": {"prompt_tokens": 140, "completion_tokens": 10}
            }
        ],
        "failures": []
    })
}

/// 断言工具失败后 run 仍 Completed 且实例 Completed（05 §5.6.6：不终止）。
fn assert_tool_failure_continues(reader: &TraceReader, run_id: &str) -> StreamingRow {
    let run = common::run_row(reader, run_id);
    assert_eq!(run.status, zgraph_trace::RunStatus::Completed);
    assert_eq!(run.error_code, None, "工具失败不得进入 run 错误列");
    let output = agent_output(reader, run_id);
    assert_eq!(output["content"], json!("{\"answer\":\"ok\"}"));
    let ends = streaming_segments(reader, run_id);
    let failed: Vec<StreamingRow> = ends
        .into_iter()
        .filter(|segment| segment.kind == "tool_call_end" && segment.ok == Some(false))
        .collect();
    assert_eq!(failed.len(), 1, "恰一条失败 tool_call_end");
    failed.into_iter().next().expect("failed tool_call_end")
}

#[tokio::test]
async fn tool_exception_is_fed_back_and_run_continues() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db = dir.path().join("ac08_exception.db");
    let fake = fake_provider(two_call_script("explode", json!({})));
    let bridge = run_ac08(routing_executor(fake.clone(), true), &db).await;

    assert_eq!(bridge.outcome.status, RunStatus::Completed);
    let run_id = bridge.outcome.run_id.clone();
    let reader = bridge.reader();
    common::assert_lifecycle_shape(reader, &run_id);
    let failed = assert_tool_failure_continues(reader, &run_id);
    assert_eq!(failed.tool_name.as_deref(), Some("explode"));
    let summary = failed.summary.expect("失败 tool_call_end 应有 summary");
    assert_eq!(summary["ok"], json!(false));
    assert_eq!(summary["error"]["reason"], json!("exception"));
    assert_eq!(summary["error"]["type"], json!("ValueError"));
    assert_eq!(summary["error"]["message"], json!("boom"));

    // 第二次请求：assistant tool_call + Tool 消息（异常 JSON reason=exception）。
    let requests = fake.requests();
    assert_eq!(requests.len(), 2, "工具异常后继续第二轮请求");
    assert_eq!(assistant_tool_names(&requests[1]), vec!["explode"]);
    let tool_results = tool_messages(&requests[1]);
    assert_eq!(tool_results.len(), 1);
    assert_eq!(tool_results[0].name, "explode");
    assert_eq!(
        tool_results[0].payload["error"]["reason"],
        json!("exception")
    );
    assert_eq!(
        tool_results[0].payload["error"]["type"],
        json!("ValueError")
    );
    assert_eq!(tool_results[0].payload["error"]["message"], json!("boom"));
    assert!(
        tool_results[0].payload["error"]["traceback"]
            .as_str()
            .unwrap_or_default()
            .contains("ValueError"),
        "异常 JSON 应带 Python traceback"
    );
}

#[tokio::test]
async fn tool_timeout_is_fed_back_and_run_continues() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db = dir.path().join("ac08_timeout.db");
    let fake = fake_provider(two_call_script("slow", json!({"seconds": 2})));
    let started = Instant::now();
    let bridge = run_ac08(routing_executor(fake.clone(), true), &db).await;
    let elapsed = started.elapsed();

    assert_eq!(bridge.outcome.status, RunStatus::Completed);
    assert!(
        elapsed < TOOL_TIMEOUT_CASE_LIMIT,
        "工具 timeout_ms=300 生效后整例应 < 15s，实际 {elapsed:?}"
    );
    assert!(
        elapsed >= Duration::from_millis(SLOW_TOOL_TIMEOUT_MS - 50),
        "slow 工具应等到声明的 300ms 超时（而不是立即失败），实际 {elapsed:?}"
    );

    let run_id = bridge.outcome.run_id.clone();
    let reader = bridge.reader();
    common::assert_lifecycle_shape(reader, &run_id);
    let failed = assert_tool_failure_continues(reader, &run_id);
    assert_eq!(failed.tool_name.as_deref(), Some("slow"));
    let summary = failed.summary.expect("超时 tool_call_end 应有 summary");
    assert_eq!(summary["ok"], json!(false));
    assert_eq!(summary["error"]["reason"], json!("timeout"));
    assert_eq!(
        summary["error"]["timeout_ms"],
        json!(SLOW_TOOL_TIMEOUT_MS),
        "生效超时应为工具声明的 timeout_ms"
    );

    // 第二次请求：Tool 消息含 timeout 错误 JSON（TOOL_ERROR 喂回后 run 继续）。
    let requests = fake.requests();
    assert_eq!(requests.len(), 2, "工具超时后继续第二轮请求");
    let tool_results = tool_messages(&requests[1]);
    assert_eq!(tool_results.len(), 1);
    assert_eq!(tool_results[0].name, "slow");
    assert_eq!(tool_results[0].payload["error"]["reason"], json!("timeout"));
    assert_eq!(
        tool_results[0].payload["error"]["timeout_ms"],
        json!(SLOW_TOOL_TIMEOUT_MS)
    );
}

// ---------------------------------------------------------------------------
// 工具名未注册（任务用例 d）：HARNESS_UNKNOWN_TOOL → 实例 Crashed
// ---------------------------------------------------------------------------

/// 单轮脚本：LLM 调用未注册工具 `nowhere`。
fn unknown_tool_script() -> Value {
    json!({
        "version": 1,
        "chunk_chars": 3,
        "chunk_delay_ms": 0,
        "calls": [
            {
                "kind": "tool_calls",
                "content": null,
                "stop_reason": "ToolCalls",
                "tool_calls": [
                    {"call_id": "call_nowhere_1", "name": "nowhere", "args": {}}
                ],
                "usage": {"prompt_tokens": 80, "completion_tokens": 5}
            }
        ],
        "failures": []
    })
}

#[tokio::test]
async fn unknown_tool_crashes_instance() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db = dir.path().join("ac08_unknown_tool.db");
    let fake = fake_provider(unknown_tool_script());
    let bridge = run_ac08(routing_executor(fake.clone(), true), &db).await;

    // run Failed，错误码 HARNESS_UNKNOWN_TOOL（05 §5.6.6 / §5.7.2）。
    assert_eq!(bridge.outcome.status, RunStatus::Failed);
    let error = bridge
        .outcome
        .error
        .as_ref()
        .expect("Failed run 应有 error");
    assert_eq!(error.code, "HARNESS_UNKNOWN_TOOL");
    let run_id = bridge.outcome.run_id.clone();
    let reader = bridge.reader();
    common::assert_lifecycle_shape(reader, &run_id);

    let run = common::run_row(reader, &run_id);
    assert_eq!(run.status, zgraph_trace::RunStatus::Failed);
    assert_eq!(run.error_code.as_deref(), Some("HARNESS_UNKNOWN_TOOL"));

    let instance = common::instance(reader, &run_id, AGENT_NODE_ID);
    assert_eq!(
        instance.status,
        TraceInstanceStatus::Crashed,
        "未注册工具 → 实例 Crashed（不喂回 LLM）"
    );
    assert_eq!(
        instance.error_code.as_deref(),
        Some("HARNESS_UNKNOWN_TOOL"),
        "instances.error_code"
    );

    // 只发了一轮请求：未知工具立即终止，不再发第二次请求。
    assert_eq!(fake.requests().len(), 1);
    let finished = common::run_finished_payload(reader, &run_id);
    assert_eq!(finished["status"], json!("Failed"));
    assert_eq!(finished["error"]["code"], json!("HARNESS_UNKNOWN_TOOL"));
}

// ---------------------------------------------------------------------------
// ISSUE-076：DSL 未声明 timeout_ms 时回退 AgentConfig.node_timeout_ms（端到端）
// ---------------------------------------------------------------------------

/// ISSUE-076 用例的 AgentConfig 回退超时（毫秒）：DSL 不声明，内核经
/// `NodeExecutor::timeout_hint` 采用该值；默认 180s 不可能在宽松上限内结束。
const HINT_NODE_TIMEOUT_MS: u64 = 1_800;

/// 慢 fake LLM 的 chunk 延迟与总字符数：`ceil(300/3) * 100ms = 10s` > hint。
const HINT_CHUNK_DELAY_MS: u64 = 100;

/// 慢 fake LLM 的回应字符数（300 / chunk_chars=3 = 100 个 chunk）。
const HINT_RESPONSE_CHARS: usize = 300;

/// 本用例的宽松上限（远小于默认 180s，远大于 hint）。
const HINT_CASE_LIMIT: Duration = Duration::from_secs(15);

/// 生成临时 agent 资产：`agents/slow_agent.json` 声明 `node_timeout_ms=1800`。
fn timeout_hint_assets(dir: &std::path::Path) -> std::path::PathBuf {
    let assets = dir.join("issue076_assets");
    let agents = assets.join("agents");
    std::fs::create_dir_all(&agents).expect("create agents dir");
    std::fs::create_dir_all(assets.join("tools")).expect("create tools dir");
    let config = json!({
        "id": "slow_agent",
        "name": "ISSUE-076 慢响应 agent",
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
        "tools": [],
        "input_mapping": {"message": "message"},
        "max_iterations": 4,
        "llm_timeout_ms": 60000,
        "llm_max_retries": 1,
        "node_timeout_ms": HINT_NODE_TIMEOUT_MS
    });
    std::fs::write(agents.join("slow_agent.json"), config.to_string()).expect("write agent config");
    std::fs::write(
        agents.join("prompt.md"),
        "你是 ISSUE-076 慢响应测试 agent。",
    )
    .expect("write prompt");
    assets
}

/// 慢响应 fake LLM 脚本（token 间隔 100ms，总时长约 10s）。
fn slow_text_script() -> Value {
    json!({
        "version": 1,
        "chunk_chars": 3,
        "chunk_delay_ms": HINT_CHUNK_DELAY_MS,
        "calls": [{
            "kind": "text",
            "content": "x".repeat(HINT_RESPONSE_CHARS),
            "stop_reason": "EndTurn",
            "usage": {"prompt_tokens": 10, "completion_tokens": 2}
        }],
        "failures": []
    })
}

/// `start -> agent_call(不声明 timeout_ms) -> end`。
fn timeout_hint_graph() -> Arc<CompiledGraph> {
    let text = json!({
        "version": "1.0",
        "metadata": {"name": "e2e_issue076_timeout_hint"},
        "entry_points": ["start"],
        "nodes": [
            {"id": "start", "type": "start",
             "inputs": [{"name": "message", "type": "string", "required": true}],
             "config": {}},
            {"id": "worker", "type": "agent_call",
             "inputs": [{"name": "message", "type": "any", "required": true}],
             "trigger": "all",
             "config": {"agent_id": "slow_agent"}},
            {"id": "end", "type": "end",
             "inputs": [{"name": "result", "type": "any", "required": true}],
             "config": {}}
        ],
        "edges": [
            {"from": "start", "to_node": "worker", "to_input_port": "message"},
            {"from": "worker", "to_node": "end", "to_input_port": "result"}
        ]
    })
    .to_string();
    common::compile_text(&text)
}

/// ISSUE-076 用例的组合执行器（agent → Harness + 慢 fake；其余 → 真 worker）。
fn timeout_hint_executor(dir: &Path) -> Arc<dyn NodeExecutor> {
    let assets = timeout_hint_assets(dir);
    let fake = Arc::new(FakeLlmProvider::from_script(slow_text_script()).expect("slow script"));
    let settings = HarnessSettings {
        assets_dir: assets.clone(),
        tools_dir: assets.join("tools"),
        provider: fake,
        fake_mode: true,
        tool_runner: Arc::new(WorkerToolRunner::new(common::worker_config(
            common::RUN_TIMEOUT,
        ))),
    };
    Arc::new(RoutingExecutor {
        harness: Arc::new(HarnessExecutor::new(settings)),
        worker: common::executor(common::RUN_TIMEOUT),
    })
}

/// 跑 ISSUE-076 用例（DSL 不声明 timeout_ms），返回 outcome 与耗时。
async fn run_timeout_hint_case(dir: &Path) -> (BridgeOutcome, Duration) {
    let db = dir.join("issue076.db");
    let inputs = common::entry_inputs(&[("start", "message", json!("hi"))]);
    let started = Instant::now();
    let bridge = tokio::time::timeout(
        HINT_CASE_LIMIT,
        common::run_to_trace(
            common::spec(timeout_hint_graph(), inputs),
            timeout_hint_executor(dir),
            &db,
        ),
    )
    .await
    .expect("hint=1800ms 必须远早于默认 180s（否则宽松上限内不会结束）");
    (bridge, started.elapsed())
}

/// ISSUE-076 时序断言：耗时与 hint 同量级（证明回退链生效，而非默认 180s）。
fn assert_timeout_hint_elapsed(elapsed: Duration) {
    assert!(
        elapsed >= Duration::from_millis(HINT_NODE_TIMEOUT_MS - 300),
        "超时不应早于 hint：elapsed={elapsed:?}"
    );
    assert!(
        elapsed < Duration::from_secs(8),
        "hint 未生效（默认 180s 会超上限）：elapsed={elapsed:?}"
    );
}

/// 实例级超时错误（ISSUE-086 / 06 §6.2.5 + §6.2.5.1）：TimedOut + `NODE_TIMEOUT` + NodeError JSON。
///
/// ISSUE-086（REV-040）后 Harness 自报 `NodeError::Timeout` 经内核 `fail_timeout()`
/// 也会回填实例错误，与 watchdog 路径同口径；e2e 以**实例级** `error_code`/`error`
/// 为权威证据（只看 run 级会漏掉两条路径落库不一致）。
fn assert_timeout_instance_error(reader: &TraceReader, run_id: &str) -> Value {
    let instance = common::instance(reader, run_id, "worker");
    assert_eq!(
        instance.status,
        TraceInstanceStatus::TimedOut,
        "agent_call 实例应 TimedOut"
    );
    assert_eq!(
        instance.error_code.as_deref(),
        Some("NODE_TIMEOUT"),
        "ISSUE-086：instances.error_code 必须与 watchdog 路径同口径落库"
    );
    let error = instance
        .error
        .as_ref()
        .expect("ISSUE-086：instances.error 不得为 NULL");
    assert_eq!(error["code"], json!("NODE_TIMEOUT"));
    assert_eq!(error["kind"], json!("Timeout"));
    assert!(
        error["message"]
            .as_str()
            .is_some_and(|text| !text.is_empty()),
        "instances.error.message 非空：{error}"
    );
    error.clone()
}

/// ISSUE-076 trace 断言：实例 TimedOut + 实例级 `NODE_TIMEOUT` 落库 + `NodeTimedOut` 恰一次。
fn assert_timeout_hint_trace(reader: &TraceReader, run_id: &str) {
    common::assert_lifecycle_shape(reader, run_id);
    let run = common::run_row(reader, run_id);
    assert_eq!(run.status, zgraph_trace::RunStatus::Failed);
    assert_eq!(run.error_code.as_deref(), Some("NODE_TIMEOUT"));
    let instance_error = assert_timeout_instance_error(reader, run_id);
    // agent_call 没有常驻 worker 子进程：超时由内核 abort harness 任务（工具 discover
    // 的短命 worker 已退出），因此 `killed_workers` 对 agent 节点不适用（D1 口径只覆盖
    // `WorkerExecutor` 的 python 节点）；节点超时以实例级错误 + NodeTimedOut 事件为准。
    let events = common::events(reader, run_id);
    assert_eq!(
        common::count_kind(&events, "NodeTimedOut"),
        1,
        "NodeTimedOut 恰一次"
    );
    let timed_out = events
        .into_iter()
        .find(|event| event.kind == "NodeTimedOut")
        .expect("NodeTimedOut event");
    assert_eq!(
        instance_error["elapsed_ms"], timed_out.payload["elapsed_ms"],
        "实例错误与 NodeTimedOut 事件的 elapsed_ms 必须同源（同一条超时路径）"
    );
}

/// ISSUE-076 端到端：DSL 未声明 timeout_ms → 内核经 RoutingExecutor 转发 harness
/// `timeout_hint` 采用 AgentConfig.node_timeout_ms，实例 TimedOut / NODE_TIMEOUT，
/// 且耗时与 hint 同量级（证明回退链生效，而非默认 180s）。
#[tokio::test]
async fn agent_config_node_timeout_hint_backs_off_when_dsl_omits_timeout() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (bridge, elapsed) = run_timeout_hint_case(dir.path()).await;
    assert_timeout_hint_elapsed(elapsed);
    assert_eq!(bridge.outcome.status, RunStatus::Failed);
    let error = bridge
        .outcome
        .error
        .as_ref()
        .expect("Failed run 应有 error");
    assert_eq!(error.code, "NODE_TIMEOUT");
    let run_id = bridge.outcome.run_id.clone();
    assert_timeout_hint_trace(bridge.reader(), &run_id);
}

// ---------------------------------------------------------------------------
// 真实模型（任务用例 e，AC-08 真实模型部分）：#[ignore] 手动执行
// ---------------------------------------------------------------------------

/// AC-08 真实模型手动验收（07 AC-08 后半）：真实 DeepSeek + 同一 fixture。
///
/// 默认 `#[ignore]`（CI 无网络/无 key）。启用条件：`ZGRAPH_REAL_LLM=1` 且
/// `DEEPSEEK_API_KEY` 非空；命令见文件顶部文档注释。
#[tokio::test]
#[ignore = "manual: 需 ZGRAPH_REAL_LLM=1 与 DEEPSEEK_API_KEY（真实 DeepSeek 网络调用）"]
async fn ac08b_real_llm_manual() {
    if std::env::var("ZGRAPH_REAL_LLM").as_deref() != Ok("1") {
        eprintln!("[skip] 未设置 ZGRAPH_REAL_LLM=1，跳过真实模型验收");
        return;
    }
    let api_key = std::env::var("DEEPSEEK_API_KEY").unwrap_or_default();
    if api_key.trim().is_empty() {
        eprintln!("[skip] 缺少 DEEPSEEK_API_KEY，跳过真实模型验收");
        return;
    }

    let provider: Arc<dyn LlmProvider> =
        Arc::new(GenaiLlmProvider::new().expect("构建 genai client 失败（真实模型用例）"));
    let dir = tempfile::tempdir().expect("tempdir");
    let db = dir.path().join("ac08b_real.db");
    let bridge = run_ac08(routing_executor(provider, false), &db).await;

    assert_eq!(bridge.outcome.status, RunStatus::Completed);
    let run_id = bridge.outcome.run_id.clone();
    let reader = bridge.reader();
    common::assert_lifecycle_shape(reader, &run_id);

    let output = agent_output(reader, &run_id);
    assert!(
        output["content"]
            .as_str()
            .is_some_and(|text| !text.is_empty()),
        "真实模型应返回非空 content"
    );
    assert!(
        output["stop_reason"]
            .as_str()
            .is_some_and(|text| !text.is_empty()),
        "真实模型 stop_reason 非空"
    );
    assert!(output["iterations"].as_u64().unwrap_or(0) >= 1);
    assert!(
        output["usage"]["total_tokens"].as_u64().unwrap_or(0) > 0,
        "AC-08：真实模型 usage.total_tokens > 0"
    );
    // AC-08：TokenDelta 聚合后非空（06 写线程聚合）。
    let segments = streaming_segments(reader, &run_id);
    assert!(
        !token_text(&segments).is_empty(),
        "AC-08：真实模型 token 流式聚合后非空"
    );
}
