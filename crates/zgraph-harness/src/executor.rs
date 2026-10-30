//! `HarnessExecutor`：`agent_call` 节点的 [`NodeExecutor`] 实现（M4）。
//!
//! 职责边界（05 §5.1.1 H1）：
//! - 只通过 `NodeContext` 与框架交互；不触碰 `RunContext`/调度器；
//! - **唯一**处理 `CompiledHandler::Agent`；其它 handler 属框架路由错误 →
//!   `NodeError::Internal`；
//! - 每次执行**懒加载** AgentConfig（不缓存），工具发现结果按 `tools_dir`
//!   在进程内缓存一次（D3：工具模块本身无跨调用缓存）。

use std::collections::{BTreeMap, HashMap};
use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::Arc;

use serde_json::Value;
use zgraph_dsl::InputPort;
use zgraph_kernel::{
    CompiledHandler, CompiledNode, NodeError, NodeExecution, NodeExecutor, PortName,
};

use crate::agent_loop::{run_agent, AgentRun, RunFailure};
use crate::config::{load_agent, peek_agent_node_timeout};
use crate::constants::DEFAULT_NODE_TIMEOUT_MS;
use crate::ctx::{CancelToken, HarnessCtx};
use crate::error::HarnessError;
use crate::llm::LlmProvider;
use crate::record::LlmCallRecorder;
use crate::tools::{ToolRegistry, ToolRunner};

/// Harness 执行器配置（e2e/M5 依赖的冻结面）。
pub struct HarnessSettings {
    /// Agent 资产根：`<assets_dir>/agents/*.json`（05 §5.2.1）。
    pub assets_dir: PathBuf,
    /// 工具目录（D3：worker 的 `tools_dir`；05 §5.6.2）。
    pub tools_dir: PathBuf,
    /// LLM transport。
    pub provider: Arc<dyn LlmProvider>,
    /// `--fake-llm` 模式：跳过 `api_key_env` 检查（05 §5.2.4 步 9）。
    pub fake_mode: bool,
    /// 工具执行后端（生产：`WorkerToolRunner`；测试：`FakeToolRunner`）。
    pub tool_runner: Arc<dyn ToolRunner>,
}

impl std::fmt::Debug for HarnessSettings {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HarnessSettings")
            .field("assets_dir", &self.assets_dir)
            .field("tools_dir", &self.tools_dir)
            .field("provider", &self.provider.name())
            .field("fake_mode", &self.fake_mode)
            .finish_non_exhaustive()
    }
}

/// `agent_call` 节点执行器：Loading/Assembling/Invoking/ToolExec 的进程内壳。
pub struct HarnessExecutor {
    settings: HarnessSettings,
    /// 工具发现缓存：`tools_dir` → 进程内唯一注册表（D3；工具模块无跨调用缓存）。
    tools_cache: tokio::sync::Mutex<HashMap<PathBuf, Arc<ToolRegistry>>>,
    /// LLM 调用记录器（ISSUE-082；可选观察者，H3：Harness 不写 SQLite）。
    recorder: Option<Arc<dyn LlmCallRecorder>>,
}

impl HarnessExecutor {
    /// 构造执行器（不记录 LLM 调用）。
    pub fn new(settings: HarnessSettings) -> Self {
        Self {
            settings,
            tools_cache: tokio::sync::Mutex::new(HashMap::new()),
            recorder: None,
        }
    }

    /// 构造执行器并挂接 LLM 调用记录器（ISSUE-082；M5 CLI/trace 写端 seam）。
    ///
    /// **只增不破**：保留 [`HarnessExecutor::new`] 与 [`HarnessSettings`] 的字段集合
    /// （e2e 以结构体字面量构造 settings，冻结面不变）。
    pub fn with_llm_recorder(
        settings: HarnessSettings,
        recorder: Arc<dyn LlmCallRecorder>,
    ) -> Self {
        Self {
            settings,
            tools_cache: tokio::sync::Mutex::new(HashMap::new()),
            recorder: Some(recorder),
        }
    }

    /// 执行器配置（只读）。
    pub fn settings(&self) -> &HarnessSettings {
        &self.settings
    }

    /// 取得（并缓存）`tools_dir` 的工具注册表；同一进程内只 `discover` 一次。
    async fn registry(&self, ctx: &Arc<dyn HarnessCtx>) -> Result<Arc<ToolRegistry>, HarnessError> {
        let tools_dir = absolute(&self.settings.tools_dir);
        let mut cache = self.tools_cache.lock().await;
        if let Some(registry) = cache.get(&tools_dir) {
            return Ok(registry.clone());
        }
        let defs = self
            .settings
            .tool_runner
            .discover(&tools_dir, ctx.clone())
            .await?;
        let registry = Arc::new(ToolRegistry::new(defs)?);
        cache.insert(tools_dir, registry.clone());
        Ok(registry)
    }
}

impl NodeExecutor for HarnessExecutor {
    fn execute<'a>(
        &'a self,
        execution: NodeExecution<'a>,
    ) -> Pin<Box<dyn Future<Output = Result<Value, NodeError>> + Send + 'a>> {
        // 只有 Agent handler 归本执行器；其它 handler 是内核路由错误。
        let Some((agent_id, node_mapping, node_max_iterations)) =
            agent_call_parts(&execution.node.handler)
        else {
            return Box::pin(async move {
                Err(NodeError::Internal {
                    message: "HarnessExecutor only handles CompiledHandler::Agent".to_string(),
                })
            });
        };
        let declared_timeout_ms = execution.node.declared_timeout_ms;
        // ISSUE-076：Loading 早退（取消/超时/ConfigLoad）的 elapsed 上报也用同一优先级
        // `declared > AgentConfig(node_timeout_ms 轻量读取) > DEFAULT`（与内核派发一致）。
        let job = AgentCallJob {
            resolved_timeout_ms: resolve_node_timeout_ms(
                declared_timeout_ms,
                &self.settings.assets_dir,
                &agent_id,
            ),
            agent_id,
            node_mapping,
            node_max_iterations,
            ctx: Arc::new(execution.ctx.clone()),
            inputs: execution.inputs.clone(),
            declared_ports: execution.node.inputs.clone(),
            declared_timeout_ms,
        };
        Box::pin(async move { self.run_agent_call(job).await })
    }

    /// ISSUE-076：`agent_call` 在 DSL 未声明 `timeout_ms` 时回退
    /// `AgentConfig.node_timeout_ms`（05 §5.2.6；DR-002 §4 同步轻量路径）。
    ///
    /// 只读资产 JSON 的 `node_timeout_ms` 字段（按 `id` 命中，轻量范围校验），
    /// **不**查 `ToolRegistry`/密钥/prompt；仅对 `CompiledHandler::Agent` 返回
    /// `Some`。任何失败或缺失 → `None`（内核回退 `DEFAULT_NODE_TIMEOUT_MS`，
    /// 运行期 Loading 仍会以 `HARNESS_CONFIG_LOAD` 失败）。
    fn timeout_hint(&self, node: &CompiledNode) -> Option<u64> {
        let CompiledHandler::Agent { agent_id, .. } = &node.handler else {
            return None;
        };
        peek_agent_node_timeout(&self.settings.assets_dir, agent_id)
    }
}

/// `agent_call` 单次执行的输入快照（`execute` → [`HarnessExecutor::run_agent_call`] 的 seam）。
struct AgentCallJob {
    agent_id: String,
    node_mapping: BTreeMap<String, String>,
    node_max_iterations: Option<u32>,
    ctx: Arc<dyn HarnessCtx>,
    inputs: HashMap<PortName, Value>,
    declared_ports: Vec<InputPort>,
    declared_timeout_ms: Option<u64>,
    resolved_timeout_ms: u64,
}

/// 从 handler 取出 `agent_call` 三元组；其它 handler → `None`（内核路由错误）。
fn agent_call_parts(
    handler: &CompiledHandler,
) -> Option<(String, BTreeMap<String, String>, Option<u32>)> {
    match handler {
        CompiledHandler::Agent {
            agent_id,
            input_mapping,
            max_iterations,
        } => Some((agent_id.clone(), input_mapping.clone(), *max_iterations)),
        _ => None,
    }
}

impl HarnessExecutor {
    /// `agent_call` 执行主体：Loading → Assembling → Invoking/ToolExec（05 §5.5.4）。
    async fn run_agent_call(&self, job: AgentCallJob) -> Result<Value, NodeError> {
        check_loading_guard(&job.ctx, job.resolved_timeout_ms)?;
        let registry = self
            .registry(&job.ctx)
            .await
            .map_err(|error| config_failure(error, &job.ctx, job.resolved_timeout_ms))?;
        let agent = load_agent(
            &self.settings.assets_dir,
            &job.agent_id,
            &registry,
            self.settings.fake_mode,
        )
        .map_err(|error| config_failure(error, &job.ctx, job.resolved_timeout_ms))?;
        // ISSUE-076：有效实例超时 = DSL 声明 > AgentConfig > DEFAULT（05 §5.2.6），
        // 与内核 dispatch 的解析顺序一致；仅用于 `Timeout.elapsed_ms` 上报。
        let node_timeout_ms = job
            .declared_timeout_ms
            .unwrap_or(agent.config.node_timeout_ms);
        // 节点级 max_iterations 覆盖 AgentConfig（05 §5.2.6）。
        let effective_max_iterations = job
            .node_max_iterations
            .unwrap_or(agent.config.max_iterations)
            .clamp(1, 50);
        let tools_dir = absolute(&self.settings.tools_dir);
        let cancel = CancelToken::new(job.ctx.clone());
        let run = AgentRun {
            agent: &agent,
            tools_dir: &tools_dir,
            provider: &self.settings.provider,
            tool_runner: &self.settings.tool_runner,
            inputs: &job.inputs,
            declared_ports: &job.declared_ports,
            node_mapping: &job.node_mapping,
            max_iterations: effective_max_iterations,
            node_timeout_ms,
            ctx: job.ctx.clone(),
            cancel,
            recorder: self.recorder.as_ref(),
        };
        run_agent(run)
            .await
            .map(|output| output.to_json())
            .map_err(RunFailure::into_node_error)
    }
}

/// Phase Loading 预检（05 §5.7.3）：取消优先，其次实例 deadline。
fn check_loading_guard(
    ctx: &Arc<dyn HarnessCtx>,
    resolved_timeout_ms: u64,
) -> Result<(), NodeError> {
    if ctx.cancel_requested() {
        return Err(NodeError::Cancelled);
    }
    let remaining = ctx.node_deadline_remaining_ms();
    if remaining == 0 {
        return Err(NodeError::Timeout {
            elapsed_ms: resolved_timeout_ms.saturating_sub(remaining),
        });
    }
    Ok(())
}

/// 生效实例超时（05 §5.2.6，与内核派发解析同序）：DSL 声明 > AgentConfig > 默认。
///
/// 轻量同步读取 `AgentConfig.node_timeout_ms`（[`peek_agent_node_timeout`]）；
/// 该值只用于 Loading 早退时的 `Timeout.elapsed_ms` 诊断，避免与内核 deadline 口径
/// 漂移（ISSUE-076）。运行期成功加载后以校验过的 `agent.config.node_timeout_ms` 为准。
fn resolve_node_timeout_ms(declared: Option<u64>, assets_dir: &Path, agent_id: &str) -> u64 {
    declared
        .or_else(|| peek_agent_node_timeout(assets_dir, agent_id))
        .unwrap_or(DEFAULT_NODE_TIMEOUT_MS)
}

/// ConfigLoad 错误在“已请求取消/已超时”时按取消/超时优先映射（05 §5.7.3）。
fn config_failure(
    error: HarnessError,
    ctx: &Arc<dyn HarnessCtx>,
    node_timeout_ms: u64,
) -> NodeError {
    if ctx.cancel_requested() {
        NodeError::Cancelled
    } else {
        let remaining = ctx.node_deadline_remaining_ms();
        if remaining == 0 {
            NodeError::Timeout {
                elapsed_ms: node_timeout_ms.saturating_sub(remaining),
            }
        } else {
            error.into_node_error()
        }
    }
}

/// 绝对化工具目录（worker 侧 `discover`/`call` 需要 abs 路径）。
fn absolute(path: &Path) -> PathBuf {
    std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf())
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    use serde_json::{json, Value};
    use tempfile::TempDir;
    use zgraph_dsl::load_graph;
    use zgraph_kernel::{
        run_to_completion, CompiledGraph, InstanceStatus, MemoryTraceSink, RunOptions, RunOutcome,
        RunSpec, RunStatus, StreamingEvent,
    };

    use crate::llm::{FakeLlmProvider, LlmProvider};
    use crate::record::{LlmCallRecorder, LLM_CALL_STATUS_OK};
    use crate::test_support::CapturingRecorder;
    use crate::tools::{FakeToolBehavior, FakeToolRunner, ToolDef, ToolRunner};

    use super::*;

    const AGENT_GRAPH: &str = r#"{
      "version": "1.0",
      "metadata": {"name": "harness-agent"},
      "entry_points": ["start"],
      "nodes": [
        {"id": "start", "type": "start",
         "inputs": [{"name": "message", "type": "any", "required": true}], "config": {}},
        {"id": "worker", "type": "agent_call",
         "inputs": [{"name": "message", "type": "any", "required": true}],
         "config": {"agent_id": "parser"}},
        {"id": "end", "type": "end",
         "inputs": [{"name": "result", "type": "any", "required": true}], "config": {}}
      ],
      "edges": [
        {"from": "start", "to_node": "worker", "to_input_port": "message"},
        {"from": "worker", "to_node": "end", "to_input_port": "result"}
      ]
    }"#;

    const PYTHON_GRAPH: &str = r#"{
      "version": "1.0",
      "metadata": {"name": "python-only"},
      "entry_points": ["start"],
      "nodes": [
        {"id": "start", "type": "start",
         "inputs": [{"name": "message", "type": "any", "required": true}], "config": {}},
        {"id": "work", "type": "python_node",
         "inputs": [{"name": "x", "type": "any", "required": true}],
         "config": {"module": "workers.demo", "function": "run"}},
        {"id": "end", "type": "end",
         "inputs": [{"name": "r", "type": "any", "required": true}], "config": {}}
      ],
      "edges": [
        {"from": "start", "to_node": "work", "to_input_port": "x"},
        {"from": "work", "to_node": "end", "to_input_port": "r"}
      ]
    }"#;

    fn agent_config(agent_id: &str, tools: &[&str]) -> Value {
        json!({
            "id": agent_id,
            "name": "parser",
            "schema_version": 1,
            "model": {
                "provider": "deepseek",
                "name": "deepseek-chat",
                "base_url": null,
                "api_key_env": "HARNESS_TEST_KEY",
                "temperature": 0.0,
                "max_tokens": 256,
                "context_window_tokens": 65536
            },
            "role_prompt_file": "prompt.md",
            "constraints_file": null,
            "tools": tools,
            "input_mapping": {"message": "user_message"},
            "max_iterations": 4
        })
    }

    fn assets(agent: Value, prompt: &str) -> TempDir {
        let dir = TempDir::new().expect("tempdir");
        std::fs::create_dir_all(dir.path().join("agents")).expect("agents");
        std::fs::write(dir.path().join("agents").join("prompt.md"), prompt).expect("prompt");
        let id = agent["id"].as_str().expect("id").to_string();
        std::fs::write(
            dir.path().join("agents").join(format!("{id}.json")),
            serde_json::to_string_pretty(&agent).expect("json"),
        )
        .expect("agent file");
        dir
    }

    fn tool_def(name: &str) -> ToolDef {
        ToolDef {
            name: name.to_string(),
            description: "tool".to_string(),
            parameters: json!({"type": "object", "properties": {}}),
            timeout_ms: crate::constants::DEFAULT_TOOL_TIMEOUT_MS,
            source: "tools.builtin".to_string(),
        }
    }

    fn fake_script(calls: Value) -> Value {
        json!({"version": 1, "chunk_chars": 64, "calls": calls})
    }

    /// 编译 `AGENT_GRAPH`（取 `CompiledNode` 供 `timeout_hint` 测试）。
    fn compiled_graph() -> Arc<CompiledGraph> {
        let loaded = load_graph(AGENT_GRAPH.as_bytes()).expect("valid graph");
        CompiledGraph::compile(&loaded).expect("compiled graph")
    }

    /// 构造带指定 `node_timeout_ms` 的 agent 资产。
    fn assets_with_node_timeout(timeout: Option<Value>) -> TempDir {
        let mut agent = agent_config("parser", &[]);
        if let Some(timeout) = timeout {
            agent["node_timeout_ms"] = timeout;
        }
        assets(agent, "你是解析器。")
    }

    /// 只用于 `timeout_hint` 的轻量执行器（不跑图）。
    fn hint_executor(assets_dir: &Path) -> HarnessExecutor {
        let fake = Arc::new(
            FakeLlmProvider::from_script(fake_script(json!([
                {"kind": "text", "content": "never"}
            ])))
            .expect("script"),
        );
        let runner = Arc::new(FakeToolRunner::new());
        HarnessExecutor::new(settings(assets_dir, &fake, &runner))
    }

    fn settings(
        assets_dir: &Path,
        fake: &Arc<FakeLlmProvider>,
        runner: &Arc<FakeToolRunner>,
    ) -> HarnessSettings {
        let provider: Arc<dyn LlmProvider> = fake.clone();
        let tool_runner: Arc<dyn ToolRunner> = runner.clone();
        HarnessSettings {
            assets_dir: assets_dir.to_path_buf(),
            tools_dir: assets_dir.join("tools"),
            provider,
            fake_mode: true,
            tool_runner,
        }
    }

    async fn run_graph(
        graph_text: &str,
        executor: Arc<HarnessExecutor>,
        entry: Value,
    ) -> (RunOutcome, Arc<MemoryTraceSink>) {
        let loaded = load_graph(graph_text.as_bytes()).expect("valid graph");
        let graph = CompiledGraph::compile(&loaded).expect("compiled graph");
        let mut entry_inputs: HashMap<String, HashMap<String, Value>> = HashMap::new();
        entry_inputs.insert(
            "start".to_string(),
            HashMap::from([("message".to_string(), entry)]),
        );
        let spec = RunSpec {
            graph,
            entry_inputs,
            user_id: "u1".to_string(),
            global_vars: HashMap::new(),
            options: RunOptions::default(),
        };
        let sink = Arc::new(MemoryTraceSink::new());
        let outcome = run_to_completion(None, spec, executor, sink.clone())
            .await
            .expect("run starts");
        (outcome, sink)
    }

    fn snapshot_status(sink: &MemoryTraceSink, node: &str) -> Option<InstanceStatus> {
        sink.snapshots()
            .into_iter()
            .rfind(|snapshot| snapshot.node_id == node)
            .map(|snapshot| snapshot.status)
    }

    #[tokio::test]
    async fn agent_call_completes_through_kernel_and_publishes_output() {
        let dir = assets(agent_config("parser", &[]), "你是解析器。");
        let fake = Arc::new(
            FakeLlmProvider::from_script(fake_script(json!([
                {"kind": "text", "content": "{\"intent\":\"refund\"}", "stop_reason": "EndTurn"}
            ])))
            .expect("script"),
        );
        let runner = Arc::new(FakeToolRunner::new());
        let executor = Arc::new(HarnessExecutor::new(settings(dir.path(), &fake, &runner)));
        let (outcome, sink) = run_graph(AGENT_GRAPH, executor, json!("退款")).await;
        assert_eq!(outcome.status, RunStatus::Completed, "outcome: {outcome:?}");
        let published = outcome.output.expect("end output");
        let agent_output = &published["result"];
        assert_eq!(agent_output["content"], "{\"intent\":\"refund\"}");
        assert_eq!(agent_output["stop_reason"], "EndTurn");
        assert_eq!(agent_output["iterations"], 1);
        assert!(agent_output["usage"]["total_tokens"].is_number());
        assert!(agent_output["usage"]["source"].is_string());
        assert_eq!(
            snapshot_status(&sink, "worker"),
            Some(InstanceStatus::Completed)
        );
        // 流式事件经 NodeContext 进入 trace（1:1，不聚合）。
        let tokens: String = sink
            .streaming_calls()
            .into_iter()
            .filter_map(|call| match call.event {
                StreamingEvent::TokenDelta { text } => Some(text),
                _ => None,
            })
            .collect();
        assert_eq!(tokens, "{\"intent\":\"refund\"}");
    }

    #[tokio::test]
    async fn agent_call_with_tool_round_trips_through_kernel() {
        let dir = assets(agent_config("parser", &["now"]), "你是解析器。");
        let fake = Arc::new(
            FakeLlmProvider::from_script(fake_script(json!([
                {"kind": "tool_calls", "content": null,
                 "tool_calls": [{"call_id": "c1", "name": "now", "args": {}}]},
                {"kind": "text", "content": "done", "stop_reason": "EndTurn"}
            ])))
            .expect("script"),
        );
        let runner = Arc::new(
            FakeToolRunner::with_tools(vec![tool_def("now")])
                .behavior("now", FakeToolBehavior::Return(json!({"now": "t"}))),
        );
        let executor = Arc::new(HarnessExecutor::new(settings(dir.path(), &fake, &runner)));
        let (outcome, sink) = run_graph(AGENT_GRAPH, executor, json!("hi")).await;
        assert_eq!(outcome.status, RunStatus::Completed);
        let published = outcome.output.expect("end output");
        assert_eq!(published["result"]["content"], "done");
        assert_eq!(published["result"]["iterations"], 2);
        assert_eq!(runner.calls().len(), 1);
        let tool_started = sink
            .streaming_calls()
            .iter()
            .filter(|call| matches!(call.event, StreamingEvent::ToolCallStart { .. }))
            .count();
        let tool_ended = sink
            .streaming_calls()
            .iter()
            .filter(|call| matches!(call.event, StreamingEvent::ToolCallEnd { ok: true, .. }))
            .count();
        assert_eq!(tool_started, 1);
        assert_eq!(tool_ended, 1);
    }

    #[tokio::test]
    async fn tool_discovery_is_cached_across_runs() {
        let dir = assets(agent_config("parser", &[]), "role");
        let fake = Arc::new(
            FakeLlmProvider::from_script(fake_script(json!([
                {"kind": "text", "content": "ok", "stop_reason": "EndTurn"}
            ])))
            .expect("script"),
        );
        let runner = Arc::new(FakeToolRunner::new());
        let executor = Arc::new(HarnessExecutor::new(settings(dir.path(), &fake, &runner)));
        let (first, _) = run_graph(AGENT_GRAPH, executor.clone(), json!("a")).await;
        let (second, _) = run_graph(AGENT_GRAPH, executor, json!("b")).await;
        assert_eq!(first.status, RunStatus::Completed);
        assert_eq!(second.status, RunStatus::Completed);
        assert_eq!(
            runner.discover_calls(),
            1,
            "tools_dir 发现结果进程内缓存一次"
        );
    }

    #[tokio::test]
    async fn config_load_failure_crashes_instance_with_harness_code() {
        let dir = TempDir::new().expect("tempdir");
        std::fs::create_dir_all(dir.path().join("agents")).expect("agents");
        let fake = Arc::new(
            FakeLlmProvider::from_script(fake_script(json!([
                {"kind": "text", "content": "never"}
            ])))
            .expect("script"),
        );
        let runner = Arc::new(FakeToolRunner::new());
        let executor = Arc::new(HarnessExecutor::new(settings(dir.path(), &fake, &runner)));
        let (outcome, sink) = run_graph(AGENT_GRAPH, executor, json!("hi")).await;
        assert_eq!(outcome.status, RunStatus::Failed);
        let error = outcome.error.expect("run error");
        assert_eq!(error.code, "HARNESS_CONFIG_LOAD");
        let snapshot = sink
            .snapshots()
            .into_iter()
            .rfind(|snapshot| snapshot.node_id == "worker")
            .expect("worker snapshot");
        assert_eq!(snapshot.status, InstanceStatus::Crashed);
        assert_eq!(snapshot.error_code.as_deref(), Some("HARNESS_CONFIG_LOAD"));
        assert!(fake.requests().is_empty());
    }

    #[tokio::test]
    async fn non_agent_handler_is_internal_error() {
        let dir = TempDir::new().expect("tempdir");
        std::fs::create_dir_all(dir.path().join("agents")).expect("agents");
        let fake = Arc::new(
            FakeLlmProvider::from_script(fake_script(json!([
                {"kind": "text", "content": "never"}
            ])))
            .expect("script"),
        );
        let runner = Arc::new(FakeToolRunner::new());
        let executor = Arc::new(HarnessExecutor::new(settings(dir.path(), &fake, &runner)));
        let (outcome, sink) = run_graph(PYTHON_GRAPH, executor, json!(1)).await;
        assert_eq!(outcome.status, RunStatus::Failed);
        let error = outcome.error.expect("run error");
        assert_eq!(error.code, "NODE_INTERNAL");
        assert_eq!(
            snapshot_status(&sink, "work"),
            Some(InstanceStatus::Crashed)
        );
        assert!(fake.requests().is_empty());
    }

    #[tokio::test]
    async fn with_llm_recorder_captures_calls_with_kernel_instance_id() {
        let dir = assets(agent_config("parser", &[]), "你是解析器。");
        let fake = Arc::new(
            FakeLlmProvider::from_script(fake_script(json!([
                {"kind": "text", "content": "ok", "stop_reason": "EndTurn"}
            ])))
            .expect("script"),
        );
        let runner = Arc::new(FakeToolRunner::new());
        let recorder = CapturingRecorder::new();
        let recorder_dyn: Arc<dyn LlmCallRecorder> = recorder.clone();
        let executor = Arc::new(HarnessExecutor::with_llm_recorder(
            settings(dir.path(), &fake, &runner),
            recorder_dyn,
        ));
        let (outcome, _) = run_graph(AGENT_GRAPH, executor, json!("hi")).await;
        assert_eq!(outcome.status, RunStatus::Completed);
        assert_eq!(
            outcome.output.as_ref().expect("output")["result"]["content"],
            "ok"
        );

        let calls = recorder.calls();
        assert_eq!(calls.len(), 1, "一次 LLM 调用 → 一条记录");
        assert_eq!(calls[0].call_index, 0);
        assert_eq!(calls[0].status, LLM_CALL_STATUS_OK);
        assert!(
            !calls[0].instance_id.is_empty(),
            "instance_id 应来自 NodeContext"
        );
        assert_eq!(calls[0].request["model"]["provider"], json!("deepseek"));
        assert_eq!(calls[0].request["messages"][0]["role"], json!("system"));
        assert_eq!(
            calls[0].response.as_ref().expect("response")["content"],
            json!("ok")
        );
        assert!(
            !calls[0].request_text().contains("test-key"),
            "记录不得泄漏密钥"
        );
    }

    // ---- ISSUE-076：`timeout_hint`（agent_call 的 AgentConfig 超时回退） ----

    #[test]
    fn timeout_hint_returns_agent_config_node_timeout() {
        let dir = assets_with_node_timeout(Some(json!(123_000)));
        let executor = hint_executor(dir.path());
        let graph = compiled_graph();
        let worker = graph.nodes.get("worker").expect("worker node");
        assert_eq!(executor.timeout_hint(worker), Some(123_000));
    }

    #[test]
    fn timeout_hint_is_none_for_non_agent_nodes() {
        let dir = assets_with_node_timeout(Some(json!(123_000)));
        let executor = hint_executor(dir.path());
        let graph = compiled_graph();
        for node_id in ["start", "end"] {
            let node = graph.nodes.get(node_id).expect("builtin node");
            assert_eq!(
                executor.timeout_hint(node),
                None,
                "非 agent_call 节点不得返回 hint: {node_id}"
            );
        }
    }

    #[test]
    fn timeout_hint_is_none_when_config_omits_or_has_invalid_value() {
        // 缺失 → None（内核用默认值兜底；不把 180000 钉成 hint）。
        let dir = assets_with_node_timeout(None);
        let graph = compiled_graph();
        let worker = graph.nodes.get("worker").expect("worker");
        assert_eq!(hint_executor(dir.path()).timeout_hint(worker), None);

        // 越界/非整数 → None（运行期 Loading 仍按 HARNESS_CONFIG_LOAD 失败）。
        for invalid in [json!(10), json!(4_000_000), json!("120000")] {
            let dir = assets_with_node_timeout(Some(invalid.clone()));
            assert_eq!(
                hint_executor(dir.path()).timeout_hint(worker),
                None,
                "invalid node_timeout_ms {invalid}"
            );
        }
    }

    #[test]
    fn timeout_hint_is_none_when_assets_or_agent_are_missing() {
        // 空资产目录（无 agents 子目录）。
        let empty = TempDir::new().expect("tempdir");
        let graph = compiled_graph();
        let worker = graph.nodes.get("worker").expect("worker");
        assert_eq!(hint_executor(empty.path()).timeout_hint(worker), None);

        // agents 目录存在但 agent_id 未命中。
        let dir = TempDir::new().expect("tempdir");
        std::fs::create_dir_all(dir.path().join("agents")).expect("agents");
        assert_eq!(hint_executor(dir.path()).timeout_hint(worker), None);
    }

    #[test]
    fn node_timeout_priority_is_declared_then_agent_config_then_default() {
        let dir = assets_with_node_timeout(Some(json!(123_000)));
        // DSL 声明优先于 AgentConfig hint。
        assert_eq!(
            resolve_node_timeout_ms(Some(300), dir.path(), "parser"),
            300
        );
        // 未声明 → AgentConfig.node_timeout_ms。
        assert_eq!(resolve_node_timeout_ms(None, dir.path(), "parser"), 123_000);
        // 资产缺失 → 默认值（与内核 `DEFAULT_NODE_TIMEOUT_MS` 一致）。
        assert_eq!(
            resolve_node_timeout_ms(None, &dir.path().join("missing"), "parser"),
            DEFAULT_NODE_TIMEOUT_MS
        );
    }

    #[tokio::test]
    async fn kernel_uses_agent_config_node_timeout_when_dsl_omits_it() {
        // ISSUE-076 端到端：DSL 未声明 timeout_ms 时，内核经 `timeout_hint` 采用
        // AgentConfig.node_timeout_ms=1s；若无 hint 则默认 180s（本用例会 Completed）。
        let dir = assets_with_node_timeout(Some(json!(1_000)));
        let fake = Arc::new(
            FakeLlmProvider::from_script(fake_script(json!([
                {"kind": "text", "content": "never"}
            ])))
            .expect("script"),
        );
        let runner = Arc::new(FakeToolRunner::new());
        let executor = Arc::new(HarnessExecutor::new(settings(dir.path(), &fake, &runner)));
        let started = Instant::now();
        let (outcome, sink) = run_graph(AGENT_GRAPH, executor, json!("hi")).await;
        let elapsed = started.elapsed();

        assert!(
            elapsed < Duration::from_secs(10),
            "hint 1s 应快速生效（默认 180s），实际 {elapsed:?}"
        );
        assert_eq!(outcome.status, RunStatus::Failed, "outcome: {outcome:?}");
        let error = outcome.error.expect("run error");
        assert_eq!(error.code, "NODE_TIMEOUT");
        assert_eq!(
            snapshot_status(&sink, "worker"),
            Some(InstanceStatus::TimedOut)
        );
    }
}
