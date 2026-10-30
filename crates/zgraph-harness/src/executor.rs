//! `HarnessExecutor`：`agent_call` 节点的 [`NodeExecutor`] 实现（M4）。
//!
//! 职责边界（05 §5.1.1 H1）：
//! - 只通过 `NodeContext` 与框架交互；不触碰 `RunContext`/调度器；
//! - **唯一**处理 `CompiledHandler::Agent`；其它 handler 属框架路由错误 →
//!   `NodeError::Internal`；
//! - 每次执行**懒加载** AgentConfig（不缓存），工具发现结果按 `tools_dir`
//!   在进程内缓存一次（D3：工具模块本身无跨调用缓存）。

use std::collections::HashMap;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::Arc;

use serde_json::Value;
use zgraph_kernel::{CompiledHandler, NodeError, NodeExecution, NodeExecutor};

use crate::agent_loop::{run_agent, AgentRun, RunFailure};
use crate::config::load_agent;
use crate::ctx::{CancelToken, HarnessCtx};
use crate::error::HarnessError;
use crate::llm::LlmProvider;
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
}

impl HarnessExecutor {
    /// 构造执行器。
    pub fn new(settings: HarnessSettings) -> Self {
        Self {
            settings,
            tools_cache: tokio::sync::Mutex::new(HashMap::new()),
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
        let CompiledHandler::Agent {
            agent_id,
            input_mapping,
            max_iterations,
        } = execution.node.handler.clone()
        else {
            return Box::pin(async move {
                Err(NodeError::Internal {
                    message: "HarnessExecutor only handles CompiledHandler::Agent".to_string(),
                })
            });
        };
        let node_mapping = input_mapping;
        let node_max_iterations = max_iterations;
        let ctx: Arc<dyn HarnessCtx> = Arc::new(execution.ctx.clone());
        let inputs = execution.inputs.clone();
        let declared_ports = execution.node.inputs.clone();
        let node_timeout_ms = execution.node.timeout_ms;
        Box::pin(async move {
            // ---- Phase Loading（取消/实例 deadline 预检优先）
            if ctx.cancel_requested() {
                return Err(NodeError::Cancelled);
            }
            let remaining = ctx.node_deadline_remaining_ms();
            if remaining == 0 {
                return Err(NodeError::Timeout {
                    elapsed_ms: node_timeout_ms.saturating_sub(remaining),
                });
            }
            let registry = self
                .registry(&ctx)
                .await
                .map_err(|error| config_failure(error, &ctx, node_timeout_ms))?;
            let agent = load_agent(
                &self.settings.assets_dir,
                &agent_id,
                &registry,
                self.settings.fake_mode,
            )
            .map_err(|error| config_failure(error, &ctx, node_timeout_ms))?;
            // 节点级 max_iterations 覆盖 AgentConfig（05 §5.2.6）。
            let effective_max_iterations = node_max_iterations
                .unwrap_or(agent.config.max_iterations)
                .clamp(1, 50);
            let tools_dir = absolute(&self.settings.tools_dir);
            let cancel = CancelToken::new(ctx.clone());
            let run = AgentRun {
                agent: &agent,
                registry: &registry,
                tools_dir: &tools_dir,
                provider: &self.settings.provider,
                tool_runner: &self.settings.tool_runner,
                inputs: &inputs,
                declared_ports: &declared_ports,
                node_mapping: &node_mapping,
                max_iterations: effective_max_iterations,
                node_timeout_ms,
                ctx: ctx.clone(),
                cancel,
            };
            run_agent(run)
                .await
                .map(|output| output.to_json())
                .map_err(RunFailure::into_node_error)
        })
    }
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

    use serde_json::{json, Value};
    use tempfile::TempDir;
    use zgraph_dsl::load_graph;
    use zgraph_kernel::{
        run_to_completion, CompiledGraph, InstanceStatus, MemoryTraceSink, RunOptions, RunOutcome,
        RunSpec, RunStatus, StreamingEvent,
    };

    use crate::llm::{FakeLlmProvider, LlmProvider};
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
}
