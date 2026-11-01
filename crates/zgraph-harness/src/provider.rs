//! `AgentProvider` seam（v0.3/V4；DR-011 §1）。
//!
//! harness 选择面：`AgentConfig.harness` 缺省/`kind:"builtin"` → 现有
//! [`BuiltinProvider`]（包装 M4 的 `run_agent`，**行为/输出逐字节不变**）；
//! `kind:"subprocess"` → [`crate::plugin::SubprocessProvider`]（`zgraph-harness/1`
//! NDJSON over stdio）。
//!
//! 对外契约：外部 harness 拥有自己的 agent loop，不得写 trace/状态机；
//! 只经本 seam 返回四字段 [`AgentOutput`] 与既有 `StreamingEvent`。

use std::collections::{BTreeMap, HashMap};
use std::path::Path;
use std::sync::atomic::AtomicU32;
use std::sync::Arc;

use async_trait::async_trait;
use serde_json::Value;
use zgraph_dsl::InputPort;

use crate::agent_loop::{run_agent, AgentOutput, AgentRun, RunFailure};
use crate::config::{AgentConfig, LoadedAgent};
use crate::ctx::{CancelToken, HarnessCtx};
use crate::llm::LlmProvider;
use crate::record::LlmCallRecorder;
use crate::tools::ToolRunner;

/// Provider 类型（DR-011 §1；v0.3 实现值仅这两种）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentProviderKind {
    /// 内置 harness（`run_agent`；缺省）。
    Builtin,
    /// 外部子进程 adapter（`zgraph-harness/1`）。
    Subprocess,
}

impl AgentProviderKind {
    /// 稳定字面量（日志/测试用）。
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Builtin => "builtin",
            Self::Subprocess => "subprocess",
        }
    }
}

/// 一次 provider 调用的追踪身份（写入 `run` 帧 `trace` 段与外部 adapter 日志）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderTrace {
    /// 所属 run。
    pub run_id: String,
    /// 实例 ID。
    pub instance_id: String,
    /// 节点 ID。
    pub node_id: String,
    /// 尝试序号（0 起）。
    pub attempt: u32,
    /// foreach item 下标（非 region 恒 0）。
    pub item_index: u32,
}

/// 一次 provider 调用的完整输入快照。
///
/// `inputs` 为**按 AgentConfig/节点 `input_mapping` 映射后的** agent 输入字段
/// （外部 adapter 直接消费）；builtin 路径使用 `loaded_agent` 等既有依赖，
/// 两者都通过同一 request 传递，保证 seam 单一。
pub struct ProviderRequest<'a> {
    /// Agent 配置（含 `harness` 选择）。
    pub agent: &'a AgentConfig,
    /// 映射后的 agent 输入（agent 字段名 → 值；外部 adapter 直接消费）。
    pub inputs: BTreeMap<String, Value>,
    /// 原始输入端口快照（端口名 → 值；builtin 路径按 05 §5.9.3 自行映射）。
    pub port_inputs: &'a HashMap<String, Value>,
    /// Harness 上下文（事件/取消/deadline）。
    pub ctx: Arc<dyn HarnessCtx>,
    /// 追踪身份。
    pub trace: ProviderTrace,
    /// 外部 adapter 的工作目录（`assets_dir`；DR-011 §5）。
    pub cwd: &'a Path,
    /// 生效实例超时（毫秒；`declared > AgentConfig.node_timeout_ms`）。
    pub node_timeout_ms: u64,
    /// 已加载 Agent（builtin 依赖；subprocess 忽略）。
    pub loaded_agent: &'a LoadedAgent,
    /// 工具目录（builtin 依赖）。
    pub tools_dir: &'a Path,
    /// LLM transport（builtin 依赖）。
    pub llm: &'a Arc<dyn LlmProvider>,
    /// 工具执行后端（builtin 依赖）。
    pub tool_runner: &'a Arc<dyn ToolRunner>,
    /// 生效 iteration 上限（builtin 依赖）。
    pub max_iterations: u32,
    /// 节点输入端口声明（builtin 依赖）。
    pub declared_ports: &'a [InputPort],
    /// 节点级 `input_mapping`（builtin 依赖）。
    pub node_mapping: &'a BTreeMap<String, String>,
    /// LLM 调用记录器（builtin 依赖）。
    pub recorder: Option<&'a Arc<dyn LlmCallRecorder>>,
    /// run 级 `skill_view` 预算（builtin 依赖）。
    pub skill_view_budget: &'a Arc<AtomicU32>,
    /// 本 run 的 MCP 状态（builtin 依赖；subprocess 恒 `None`）。
    pub mcp: Option<&'a crate::mcp::McpRun>,
}

/// Agent 执行后端抽象（DR-011 §1）。
///
/// 错误类型取 [`RunFailure`]（而非设计稿的 `HarnessError`）：builtin 路径需要
/// 逐字节保留 `Cancelled`/`Timeout` 语义（对应 `NodeError::Cancelled`/
/// `NODE_TIMEOUT`），`RunFailure` 是 M4 既有出口类型。
#[async_trait]
pub trait AgentProvider: Send + Sync {
    /// Provider 类型。
    fn kind(&self) -> AgentProviderKind;

    /// 执行一次 agent 调用。
    async fn run(&self, request: ProviderRequest<'_>) -> Result<AgentOutput, RunFailure>;
}

/// 内置 harness（缺省）：包装 M4 `run_agent`，行为/输出不变。
#[derive(Debug, Default)]
pub struct BuiltinProvider;

impl BuiltinProvider {
    /// 无状态构造。
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl AgentProvider for BuiltinProvider {
    fn kind(&self) -> AgentProviderKind {
        AgentProviderKind::Builtin
    }

    async fn run(&self, request: ProviderRequest<'_>) -> Result<AgentOutput, RunFailure> {
        let run = AgentRun {
            agent: request.loaded_agent,
            tools_dir: request.tools_dir,
            provider: request.llm,
            tool_runner: request.tool_runner,
            inputs: request.port_inputs,
            declared_ports: request.declared_ports,
            node_mapping: request.node_mapping,
            max_iterations: request.max_iterations,
            node_timeout_ms: request.node_timeout_ms,
            ctx: request.ctx.clone(),
            cancel: CancelToken::new(request.ctx.clone()),
            recorder: request.recorder,
            mcp: request.mcp,
            skill_view_budget: request.skill_view_budget,
        };
        run_agent(run).await
    }
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, HashMap};
    use std::sync::atomic::AtomicU32;
    use std::sync::Arc;

    use serde_json::json;
    use zgraph_dsl::{InputPort, PortType};

    use super::*;
    use crate::config::{AgentConfig, LoadedAgent, ModelConfig};
    use crate::llm::{FakeLlmProvider, ProviderKind, SecretString};
    use crate::test_support::TestCtx;
    use crate::tools::FakeToolRunner;

    fn loaded_agent() -> LoadedAgent {
        LoadedAgent {
            config: AgentConfig {
                id: "agent".to_string(),
                name: "agent".to_string(),
                schema_version: 1,
                model: ModelConfig {
                    provider: ProviderKind::Deepseek,
                    name: "deepseek-chat".to_string(),
                    base_url: None,
                    api_key_env: "UNUSED".to_string(),
                    temperature: 0.0,
                    max_tokens: 256,
                    context_window_tokens: 65_536,
                },
                role_prompt_file: "prompt.md".to_string(),
                constraints_file: None,
                tools: Vec::new(),
                skills: Vec::new(),
                mcp: None,
                harness: None,
                input_mapping: BTreeMap::new(),
                max_iterations: 10,
                llm_timeout_ms: 5_000,
                llm_max_retries: 0,
                node_timeout_ms: 10_000,
            },
            role_prompt: "你是测试 agent。".to_string(),
            constraints: None,
            api_key: SecretString::new("test"),
            tools: Vec::new(),
            skills: Vec::new(),
            mcp: None,
        }
    }

    #[tokio::test]
    async fn builtin_provider_runs_the_existing_agent_loop() {
        let loaded = loaded_agent();
        let llm: Arc<dyn LlmProvider> = Arc::new(
            FakeLlmProvider::from_script(json!({
                "version": 1,
                "chunk_chars": 64,
                "calls": [{"kind": "text", "content": "builtin-output"}]
            }))
            .expect("fake llm"),
        );
        let runner: Arc<dyn ToolRunner> = Arc::new(FakeToolRunner::new());
        let budget = Arc::new(AtomicU32::new(0));
        let ports = vec![InputPort {
            name: "message".to_string(),
            port_type: PortType::Any,
            required: true,
            description: None,
        }];
        let mut port_inputs = HashMap::new();
        port_inputs.insert("message".to_string(), json!("hi"));
        let ctx = TestCtx::new();
        let provider = BuiltinProvider::new();
        assert_eq!(provider.kind(), AgentProviderKind::Builtin);
        let request = ProviderRequest {
            agent: &loaded.config,
            inputs: BTreeMap::from([("user_message".to_string(), json!("hi"))]),
            port_inputs: &port_inputs,
            ctx: ctx.clone(),
            trace: ProviderTrace {
                run_id: "run-1".to_string(),
                instance_id: "inst-1".to_string(),
                node_id: "n1".to_string(),
                attempt: 0,
                item_index: 0,
            },
            cwd: std::path::Path::new("."),
            node_timeout_ms: 10_000,
            loaded_agent: &loaded,
            tools_dir: std::path::Path::new("."),
            llm: &llm,
            tool_runner: &runner,
            max_iterations: 10,
            declared_ports: &ports,
            node_mapping: &BTreeMap::new(),
            recorder: None,
            skill_view_budget: &budget,
            mcp: None,
        };
        let output = provider.run(request).await.expect("builtin run");
        assert_eq!(output.content, "builtin-output");
        assert_eq!(output.iterations, 1);
        // builtin 路径逐字节保留既有 streaming 事件（token delta）。
        assert_eq!(ctx.token_text(), "builtin-output");
    }
}
