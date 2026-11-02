//! `HarnessExecutor`：`agent_call` 节点的 [`NodeExecutor`] 实现（M4）。
//!
//! 职责边界（05 §5.1.1 H1）：
//! - 只通过 `NodeContext` 与框架交互；不触碰 `RunContext`/调度器；
//! - **唯一**处理 `CompiledHandler::Agent`；其它 handler 属框架路由错误 →
//!   `NodeError::Internal`；
//! - 每次执行**懒加载** AgentConfig（不缓存），工具发现结果按 `tools_dir`
//!   在进程内缓存一次（D3：工具模块本身无跨调用缓存）。

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::atomic::AtomicU32;
use std::sync::{Arc, Mutex};

use serde_json::Value;
use zgraph_dsl::InputPort;
use zgraph_kernel::{
    CompiledHandler, CompiledNode, NodeContext, NodeError, NodeExecution, NodeExecutor, PortName,
    StreamingEvent,
};
use zgraph_worker::WorkerLimits;

use crate::agent_loop::RunFailure;
use crate::assemble::effective_input_mapping;
use crate::builtin::{has_builtin_prefix, BuiltinToolSet};
use crate::config::{
    load_agent_with_load_options, peek_agent_node_timeout, AgentLoadOptions, HarnessKind,
    ModelConfig,
};
use crate::constants::DEFAULT_NODE_TIMEOUT_MS;
use crate::ctx::HarnessCtx;
use crate::error::HarnessError;
use crate::llm::LlmProvider;
use crate::plugin::SubprocessProvider;
use crate::provider::{AgentProvider, BuiltinProvider, ProviderRequest, ProviderTrace};
use crate::record::LlmCallRecorder;
use crate::tools::{acquire_tool_permits, ToolRegistry, ToolRunner};

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
    /// CLI 组合根注入的共享并发上限（ISSUE-079；`None` = 进程级默认回退）。
    tool_limits: Option<Arc<WorkerLimits>>,
    /// 本 run 用到的模型参数摘要（ISSUE-082；按 run 聚合、canonical 去重）。
    model_params: Mutex<HashMap<String, BTreeSet<String>>>,
    /// v0.3：`--skills-dir` 显式覆盖（`None` → env `ZGRAPH_SKILLS` → `<assets>/skills`）。
    ///
    /// 独立字段而非 [`HarnessSettings`] 字段：保持冻结面（e2e/CLI 结构体字面量）不变。
    skills_dir: Option<PathBuf>,
    /// v0.3（DR-009 §5）：`run_id` → `skill_view` 预算计数器。
    ///
    /// 冻结口径为「**单 run** ≤ `MAX_SKILL_VIEWS` 次」：同一 run 的全部
    /// `agent_call` 实例共享同一计数器；不同 run（含复用 executor）各自重置。
    skill_view_budgets: Mutex<HashMap<String, Arc<AtomicU32>>>,
    /// v0.3/V3：`--allow-mcp` 开关（默认关；CLI/库调用方需显式开启）。
    mcp_allowed: bool,
    /// v0.3/V3：MCP 客户端工厂（默认 rmcp；测试注入 Fake）。
    mcp_factory: Arc<dyn crate::mcp::McpClientFactory>,
    /// v0.3/V3：per-run MCP 会话（共享 pool + 每 agent 独立 plan/allow 视图）。
    ///
    /// REV-098 C1：同一 run 的多个 agent 共享 client/tools 缓存，但 servers/allow
    /// 按 agent 隔离；换 run 时关闭旧 pool。
    mcp_run: tokio::sync::Mutex<Option<McpSessionRuns>>,
    /// v0.3/V4：内置 provider（缺省；包装既有 `run_agent` 路径，行为不变）。
    builtin_provider: BuiltinProvider,
    /// v0.3/V4：外部子进程 harness（每 run 每 provider 常驻 adapter）。
    ///
    /// 懒启动：无 `harness.kind="subprocess"` 配置时零进程/零 IO。
    subprocess_provider: SubprocessProvider,
    /// v0.4/D2：内置文件工具（`--allow-builtin-tools`；`None` = 未开启、零副作用）。
    builtin_tools: Option<Arc<BuiltinToolSet>>,
    /// v0.4/D2：工具调用记录 sink（D5 v7 预留；默认 `None` = no-op）。
    tool_call_sink: Option<Arc<dyn crate::tools::ToolCallRecordSink>>,
}

/// 单 run 的 MCP 会话状态（executor 内部）。
struct McpSessionRuns {
    run_id: String,
    pool: Arc<crate::mcp::McpPool>,
    by_agent: HashMap<String, Arc<crate::mcp::McpRun>>,
}

impl HarnessExecutor {
    /// 构造执行器（不记录 LLM 调用；使用进程级默认并发上限）。
    pub fn new(settings: HarnessSettings) -> Self {
        Self {
            settings,
            tools_cache: tokio::sync::Mutex::new(HashMap::new()),
            recorder: None,
            tool_limits: None,
            model_params: Mutex::new(HashMap::new()),
            skills_dir: None,
            skill_view_budgets: Mutex::new(HashMap::new()),
            mcp_allowed: false,
            mcp_factory: crate::mcp::default_factory(),
            mcp_run: tokio::sync::Mutex::new(None),
            builtin_provider: BuiltinProvider::new(),
            subprocess_provider: SubprocessProvider::new(),
            builtin_tools: None,
            tool_call_sink: None,
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
            tool_limits: None,
            model_params: Mutex::new(HashMap::new()),
            skills_dir: None,
            skill_view_budgets: Mutex::new(HashMap::new()),
            mcp_allowed: false,
            mcp_factory: crate::mcp::default_factory(),
            mcp_run: tokio::sync::Mutex::new(None),
            builtin_provider: BuiltinProvider::new(),
            subprocess_provider: SubprocessProvider::new(),
            builtin_tools: None,
            tool_call_sink: None,
        }
    }

    /// 构造执行器并注入共享并发上限（ISSUE-079 / DR-003 D7）。
    ///
    /// 传入的 [`WorkerLimits`] 应与 [`zgraph_worker::WorkerExecutor`] 使用**同一实例**
    /// （CLI 组合根创建后分别注入），使工具调用与节点执行合计不超过全局 CPU×2 /
    /// 每 run 8 上限。**只增不破**：[`HarnessSettings`] 字段集合与既有构造器不变。
    pub fn with_tool_limits(settings: HarnessSettings, limits: Arc<WorkerLimits>) -> Self {
        Self {
            settings,
            tools_cache: tokio::sync::Mutex::new(HashMap::new()),
            recorder: None,
            tool_limits: Some(limits),
            model_params: Mutex::new(HashMap::new()),
            skills_dir: None,
            skill_view_budgets: Mutex::new(HashMap::new()),
            mcp_allowed: false,
            mcp_factory: crate::mcp::default_factory(),
            mcp_run: tokio::sync::Mutex::new(None),
            builtin_provider: BuiltinProvider::new(),
            subprocess_provider: SubprocessProvider::new(),
            builtin_tools: None,
            tool_call_sink: None,
        }
    }

    /// 记录器 + 共享并发上限（CLI `--record=full` 生产路径）。
    pub fn with_llm_recorder_and_tool_limits(
        settings: HarnessSettings,
        recorder: Arc<dyn LlmCallRecorder>,
        limits: Arc<WorkerLimits>,
    ) -> Self {
        Self {
            settings,
            tools_cache: tokio::sync::Mutex::new(HashMap::new()),
            recorder: Some(recorder),
            tool_limits: Some(limits),
            model_params: Mutex::new(HashMap::new()),
            skills_dir: None,
            skill_view_budgets: Mutex::new(HashMap::new()),
            mcp_allowed: false,
            mcp_factory: crate::mcp::default_factory(),
            mcp_run: tokio::sync::Mutex::new(None),
            builtin_provider: BuiltinProvider::new(),
            subprocess_provider: SubprocessProvider::new(),
            builtin_tools: None,
            tool_call_sink: None,
        }
    }

    /// 执行器配置（只读）。
    pub fn settings(&self) -> &HarnessSettings {
        &self.settings
    }

    /// v0.3（DR-009 §1）：注入 `--skills-dir` 显式覆盖（`None` = env/资产默认）。
    ///
    /// **只增不破**：不修改 [`HarnessSettings`] 字段集合；未配置 skills 的路径行为不变。
    pub fn with_skills_dir(mut self, skills_dir: Option<PathBuf>) -> Self {
        self.skills_dir = skills_dir;
        self
    }

    /// v0.3/V3（DR-010 §2.2）：允许配置的 MCP servers 启动（`--allow-mcp`）。
    pub fn with_mcp_allow(mut self, allowed: bool) -> Self {
        self.mcp_allowed = allowed;
        self
    }

    /// v0.3/V3（REV-098 #3）：关闭本执行器所有 MCP 连接/子进程组并清空 run 缓存。
    ///
    /// **幂等**：无 MCP（或已关闭）→ no-op；可重复调用。CLI 在 run 结束
    /// （Completed/Failed/Cancelled/错误路径、`--json`/human 两条出口）**必须**调用一次，
    /// 保证 stdio 子进程组在 run 结束时立即回收，而不是等下一次 run 或进程退出。
    /// `feature = "mcp"` 关闭时同样安全（不编译 rmcp，只清理空槽）。
    pub async fn close_mcp(&self) {
        let mut guard = self.mcp_run.lock().await;
        if let Some(session) = guard.take() {
            session.pool.shutdown().await;
        }
    }

    /// v0.3/V4：关闭全部外部 harness 进程（幂等；CLI 所有 run 出口调用）。
    ///
    /// 与 [`Self::close_mcp`] 并列：无 subprocess provider 时零开销。DR-011 §2.2：
    /// 关 stdin → `SIGTERM` 2s → `SIGKILL` 进程组；不残留孤儿进程。
    pub async fn close_provider(&self) {
        self.subprocess_provider.close_provider().await;
    }

    /// v0.4/D2：注入内置文件工具集合（`--allow-builtin-tools`；additive）。
    ///
    /// 未注入 = 不注册任何 `builtin__*`（默认路径零副作用，兼容矩阵见 DR-012）。
    pub fn with_builtin_tools(mut self, tools: Arc<BuiltinToolSet>) -> Self {
        self.builtin_tools = Some(tools);
        self
    }

    /// v0.4/D2：注入工具调用记录 sink（D5 v7 预留；默认 no-op）。
    pub fn with_tool_call_record_sink(
        mut self,
        sink: Arc<dyn crate::tools::ToolCallRecordSink>,
    ) -> Self {
        self.tool_call_sink = Some(sink);
        self
    }

    /// v0.3/V3：注入 MCP 客户端工厂（测试 seam；生产用默认 rmcp 工厂）。
    pub fn with_mcp_factory(mut self, factory: Arc<dyn crate::mcp::McpClientFactory>) -> Self {
        self.mcp_factory = factory;
        self
    }

    /// v0.3（DR-009 §5）：取该 run 的 `skill_view` 预算计数器（不存在则新建）。
    ///
    /// 冻结口径「单 run ≤ `MAX_SKILL_VIEWS` 次」：同 run 的所有 agent_call 实例
    /// 共享同一 `Arc<AtomicU32>`；不同 run 独立计数（复用同一 executor 也不串账）。
    fn skill_view_budget(&self, run_id: &str) -> Arc<AtomicU32> {
        let mut budgets = match self.skill_view_budgets.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        budgets
            .entry(run_id.to_string())
            .or_insert_with(|| Arc::new(AtomicU32::new(0)))
            .clone()
    }

    /// 本 run 用到的模型参数摘要（ISSUE-082；JSON 对象数组，**不含密钥**）。
    ///
    /// 每个摘要 = AgentConfig `model` 段的可见参数（provider/name/base_url/
    /// temperature/max_tokens/context_window_tokens）；同一 run 内按 canonical JSON
    /// 去重并按 JSON 文本升序输出。`None` = 本 run 尚未成功加载过 AgentConfig。
    pub fn model_params(&self, run_id: &str) -> Option<Vec<Value>> {
        let guard = match self.model_params.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        let params = guard.get(run_id)?;
        Some(
            params
                .iter()
                .filter_map(|text| serde_json::from_str::<Value>(text).ok())
                .collect(),
        )
    }

    /// 记录一次 AgentConfig 模型参数摘要（ISSUE-082；canonical 去重）。
    fn record_model_params(&self, run_id: &str, model: &ModelConfig) {
        // 显式枚举字段：**不包含** `api_key_env` 等密钥来源信息；
        // `base_url` 再做防御性脱敏（去掉 userinfo/query/fragment，REV-048 C5）。
        let summary = serde_json::json!({
            "provider": model.provider.as_str(),
            "name": model.name,
            "base_url": model.base_url.as_deref().map(sanitize_base_url),
            "temperature": model.temperature,
            "max_tokens": model.max_tokens,
            "context_window_tokens": model.context_window_tokens,
        });
        let Ok(text) = serde_json::to_string(&summary) else {
            return;
        };
        let mut guard = match self.model_params.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        guard.entry(run_id.to_string()).or_default().insert(text);
    }

    /// 取得（并缓存）`tools_dir` 的工具注册表；同一进程内只 `discover` 一次。
    async fn registry(&self, ctx: &Arc<dyn HarnessCtx>) -> Result<Arc<ToolRegistry>, HarnessError> {
        let tools_dir = absolute(&self.settings.tools_dir);
        let mut cache = self.tools_cache.lock().await;
        if let Some(registry) = cache.get(&tools_dir) {
            return Ok(registry.clone());
        }
        // ISSUE-079：`discover` 也 spawn worker 子进程，计入同一并发上限。
        let permits = acquire_tool_permits(ctx).await.map_err(|failure| {
            HarnessError::config_load(
                &failure.reason,
                Some(&tools_dir.display().to_string()),
                Some("$.tools"),
                None,
                None,
                failure.message,
            )
        })?;
        let mut defs = self
            .settings
            .tool_runner
            .discover(&tools_dir, ctx.clone())
            .await?;
        drop(permits);
        // DR-013 §1.1（REV-106 N1）：`builtin__` 前缀保留——**无论是否 opt-in**，
        // Python 工具占用该前缀都在注册期 fail-closed。
        for tool in &defs {
            if has_builtin_prefix(&tool.name) {
                return Err(HarnessError::config_load(
                    "reserved_prefix",
                    Some(&tools_dir.display().to_string()),
                    Some("$.tools[].name"),
                    Some(serde_json::json!("non-'builtin__' prefix (reserved)")),
                    Some(serde_json::json!(&tool.name)),
                    format!(
                        "python tool '{}' uses the reserved 'builtin__' prefix",
                        tool.name
                    ),
                ));
            }
        }
        if let Some(builtin) = &self.builtin_tools {
            // 跨源重名 fail-closed（无覆盖语义）。
            for tool in builtin.definitions() {
                if let Some(existing) = defs.iter().find(|item| item.name == tool.name) {
                    return Err(HarnessError::config_load(
                        "tool_name_conflict",
                        Some(&tools_dir.display().to_string()),
                        Some("$.tools"),
                        Some(serde_json::json!("globally unique tool name")),
                        Some(
                            serde_json::json!({"tool": tool.name, "sources": [existing.source, tool.source]}),
                        ),
                        format!(
                            "tool name '{}' declared by '{}' and '{}'",
                            tool.name, existing.source, tool.source
                        ),
                    ));
                }
                defs.push(tool.clone());
            }
        }
        let registry = Arc::new(ToolRegistry::new(defs)?);
        cache.insert(tools_dir, registry.clone());
        Ok(registry)
    }

    /// 按注入配置构造执行上下文：注入共享上限时用 [`HarnessNodeCtx`] 包装内核上下文。
    fn context_for(&self, ctx: &NodeContext) -> Arc<dyn HarnessCtx> {
        match &self.tool_limits {
            Some(limits) => Arc::new(HarnessNodeCtx {
                inner: ctx.clone(),
                limits: limits.clone(),
            }),
            None => Arc::new(ctx.clone()),
        }
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
            node_id: execution.ctx.node_id().to_string(),
            attempt: execution.ctx.attempt(),
            item_index: execution.ctx.item_index(),
            ctx: self.context_for(execution.ctx),
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

/// 生产上下文适配（ISSUE-079 / DR-003 D7；D15「生产唯一实现 = `NodeContext`、
/// 无独立包装类型」的**局部修订**）。
///
/// 仅当 CLI 组合根注入共享并发上限时出现：内核 [`NodeContext`] 仍是唯一状态来源，
/// 本类型只额外携带 `Arc<WorkerLimits>`，使工具调用与节点执行共享同一限额。未注入
/// 路径继续直接使用 `impl HarnessCtx for NodeContext`（D15 原样）。
struct HarnessNodeCtx {
    inner: NodeContext,
    limits: Arc<WorkerLimits>,
}

impl HarnessCtx for HarnessNodeCtx {
    fn emit(&self, event: StreamingEvent) {
        NodeContext::emit(&self.inner, event);
    }

    fn cancel_requested(&self) -> bool {
        NodeContext::cancel_requested(&self.inner)
    }

    fn node_deadline_remaining_ms(&self) -> u64 {
        NodeContext::node_deadline_remaining_ms(&self.inner)
    }

    fn user_id(&self) -> &str {
        NodeContext::user_id(&self.inner)
    }

    fn get_var(&self, key: &str) -> Option<&Value> {
        NodeContext::get_var(&self.inner, key)
    }

    fn seed(&self) -> u64 {
        NodeContext::seed(&self.inner)
    }

    fn instance_id(&self) -> &str {
        NodeContext::instance_id(&self.inner)
    }

    fn run_id(&self) -> &str {
        NodeContext::run_id(&self.inner)
    }

    fn tool_limits(&self) -> Option<Arc<WorkerLimits>> {
        Some(self.limits.clone())
    }
}

/// `agent_call` 单次执行的输入快照（`execute` → [`HarnessExecutor::run_agent_call`] 的 seam）。
struct AgentCallJob {
    agent_id: String,
    node_mapping: BTreeMap<String, String>,
    node_max_iterations: Option<u32>,
    /// 执行节点 ID（外部 harness `run.trace.node_id`）。
    node_id: String,
    /// 尝试序号（外部 harness session reset 判定）。
    attempt: u32,
    /// foreach item 下标（非 region 恒 0）。
    item_index: u32,
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
        let mut agent = load_agent_with_load_options(
            &self.settings.assets_dir,
            &job.agent_id,
            &registry,
            self.settings.fake_mode,
            &AgentLoadOptions {
                skills_dir: self.skills_dir.clone(),
                mcp_allowed: self.mcp_allowed,
                builtin_tools_allowed: self.builtin_tools.is_some(),
            },
        )
        .map_err(|error| config_failure(error, &job.ctx, job.resolved_timeout_ms))?;
        // v0.3/V3：MCP 工具发现（async；每 run 每 server 懒连接 + run 级缓存）。
        let mcp_run = self
            .prepare_mcp_run(&job, &mut agent)
            .await
            .map_err(|error| config_failure(error, &job.ctx, job.resolved_timeout_ms))?;
        // ISSUE-082：记录本 run 的模型参数摘要（去重；失败 run 也保留已加载的摘要）。
        self.record_model_params(job.ctx.run_id(), &agent.config.model);
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
        let skill_view_budget = self.skill_view_budget(job.ctx.run_id());
        // v0.3/V4：provider 选择（缺省 builtin；显式 builtin 等义）。
        let harness_kind = agent
            .config
            .harness
            .as_ref()
            .map(|harness| harness.kind)
            .unwrap_or(HarnessKind::Builtin);
        if harness_kind == HarnessKind::Subprocess && job.node_max_iterations.is_some() {
            // 外部 harness 拥有自己的 loop：节点级 max_iterations 覆盖 → 能力冲突。
            return Err(config_failure(
                HarnessError::config_load(
                    "provider_capability_conflict",
                    None,
                    Some("$.max_iterations"),
                    Some(serde_json::json!("absent")),
                    Some(serde_json::json!(job.node_max_iterations)),
                    "subprocess harness owns its loop: node max_iterations must be absent"
                        .to_string(),
                ),
                &job.ctx,
                job.resolved_timeout_ms,
            ));
        }
        let request = ProviderRequest {
            agent: &agent.config,
            inputs: mapped_agent_inputs(&job, &agent.config),
            port_inputs: &job.inputs,
            ctx: job.ctx.clone(),
            trace: ProviderTrace {
                run_id: job.ctx.run_id().to_string(),
                instance_id: job.ctx.instance_id().to_string(),
                node_id: job.node_id.clone(),
                attempt: job.attempt,
                item_index: job.item_index,
            },
            cwd: &self.settings.assets_dir,
            node_timeout_ms,
            loaded_agent: &agent,
            tools_dir: &tools_dir,
            llm: &self.settings.provider,
            tool_runner: &self.settings.tool_runner,
            max_iterations: effective_max_iterations,
            declared_ports: &job.declared_ports,
            node_mapping: &job.node_mapping,
            recorder: self.recorder.as_ref(),
            skill_view_budget: &skill_view_budget,
            mcp: mcp_run.as_deref(),
            builtin: self.builtin_tools.as_ref(),
            tool_call_sink: self.tool_call_sink.as_ref(),
        };
        let provider: &dyn AgentProvider = match harness_kind {
            HarnessKind::Builtin => &self.builtin_provider,
            HarnessKind::Subprocess => &self.subprocess_provider,
        };
        provider
            .run(request)
            .await
            .map(|output| output.to_json())
            .map_err(RunFailure::into_node_error)
    }
}

impl HarnessExecutor {
    /// v0.3/V3：准备 MCP run 状态并解析本 Agent 显式请求的 MCP 工具。
    ///
    /// - 无 `AgentConfig.mcp` → 不读 registry/不启进程（默认路径零开销）；
    /// - 换 run → 关闭旧连接（进程组 kill）；同 run 多次 agent_call 复用连接与工具缓存；
    /// - `allow` 交集/`tools` 显式白名单在 [`crate::mcp::map_tools`] 校验（fail-closed）；
    /// - stderr 打印无密钥启动命令（DR-010 §2.2/§6）。
    async fn prepare_mcp_run(
        &self,
        job: &AgentCallJob,
        agent: &mut crate::config::LoadedAgent,
    ) -> Result<Option<Arc<crate::mcp::McpRun>>, HarnessError> {
        let Some(plan) = agent.mcp.clone() else {
            return Ok(None);
        };
        let run_id = job.ctx.run_id().to_string();
        let mut guard = self.mcp_run.lock().await;
        if let Some(session) = guard.as_ref() {
            if session.run_id != run_id {
                let old = guard.take().expect("checked");
                old.pool.shutdown().await;
            }
        }
        if guard.is_none() {
            *guard = Some(McpSessionRuns {
                run_id,
                pool: crate::mcp::McpPool::new(
                    self.mcp_factory.clone(),
                    self.settings.assets_dir.clone(),
                ),
                by_agent: HashMap::new(),
            });
        }
        let session = guard.as_mut().expect("session exists");
        let run = match session.by_agent.get(&job.agent_id) {
            Some(run) => run.clone(),
            None => {
                let run = crate::mcp::McpRun::new(Arc::new(plan), session.pool.clone());
                for notice in run.startup_notice() {
                    eprintln!("{notice}");
                }
                session.by_agent.insert(job.agent_id.clone(), run.clone());
                run
            }
        };
        drop(guard);
        let discovered = run.list_tools().await?;
        let requested: Vec<String> = agent
            .config
            .tools
            .iter()
            .filter(|reference| reference.starts_with("mcp__"))
            .cloned()
            .collect();
        let bound = crate::mcp::map_tools(run.plan(), &requested, &discovered)?;
        run.bind_tools(&bound).await;
        for item in &bound {
            if agent
                .tools
                .iter()
                .any(|existing| existing.name == item.def.name)
            {
                return Err(crate::mcp::mcp_config_error(
                    "name_conflict",
                    None,
                    format!(
                        "MCP tool '{}' conflicts with an existing tool",
                        item.def.name
                    ),
                ));
            }
        }
        agent.tools.extend(bound.into_iter().map(|item| item.def));
        Ok(Some(run))
    }
}

/// 防御性脱敏 `base_url`：去掉 userinfo（`user:pass@`）与 query/fragment，
/// 仅保留 `scheme://host[:port][/path]`；非 `scheme://` 形式原样返回（配置校验已拒绝
/// 非法值，这里不引入解析风险）。用于 `model_params_json`（不含密钥，REV-048 C5）。
fn sanitize_base_url(raw: &str) -> String {
    let Some((scheme, rest)) = raw.split_once("://") else {
        return raw.to_string();
    };
    let rest = rest.split(['?', '#']).next().unwrap_or(rest);
    let (authority, path) = match rest.split_once('/') {
        Some((authority, path)) => (authority, Some(path)),
        None => (rest, None),
    };
    let host = authority
        .rsplit_once('@')
        .map_or(authority, |(_, host)| host);
    let mut sanitized = format!("{scheme}://{host}");
    if let Some(path) = path {
        sanitized.push('/');
        sanitized.push_str(path);
    }
    sanitized
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

/// 映射后的 agent 输入（外部 harness `run.inputs`；端口名 → agent 字段）。
///
/// 与 builtin 相同的 05 §5.9.3 映射规则（节点级 `input_mapping` 优先）；
/// 未到端口为 `null`（与整包透传一致）。
fn mapped_agent_inputs(
    job: &AgentCallJob,
    agent: &crate::config::AgentConfig,
) -> BTreeMap<String, Value> {
    effective_input_mapping(&job.declared_ports, &job.node_mapping, &agent.input_mapping)
        .into_iter()
        .map(|(port, field)| {
            let value = job.inputs.get(&port).cloned().unwrap_or(Value::Null);
            (field, value)
        })
        .collect()
}

/// 绝对化工具目录（worker 侧 `discover`/`call` 需要 abs 路径）。
fn absolute(path: &Path) -> PathBuf {
    std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf())
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::atomic::Ordering;
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    use serde_json::{json, Value};
    use tempfile::TempDir;
    use zgraph_dsl::load_graph;
    use zgraph_kernel::{
        run_to_completion, CompiledGraph, InstanceStatus, MemoryTraceSink, RunOptions, RunOutcome,
        RunSpec, RunStatus, StreamingEvent,
    };

    use crate::llm::{FakeLlmProvider, LlmMessage, LlmProvider};
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

    /// 与 [`AGENT_GRAPH`] 相同，但 `agent_call` 节点显式覆盖 `max_iterations`
    /// （v0.3/V4 能力冲突测试：外部 harness 拥有自己的 loop）。
    const AGENT_GRAPH_WITH_ITERATIONS: &str = r#"{
      "version": "1.0",
      "metadata": {"name": "harness-agent-iterations"},
      "entry_points": ["start"],
      "nodes": [
        {"id": "start", "type": "start",
         "inputs": [{"name": "message", "type": "any", "required": true}], "config": {}},
        {"id": "worker", "type": "agent_call",
         "inputs": [{"name": "message", "type": "any", "required": true}],
         "config": {"agent_id": "parser", "max_iterations": 3}},
        {"id": "end", "type": "end",
         "inputs": [{"name": "result", "type": "any", "required": true}], "config": {}}
      ],
      "edges": [
        {"from": "start", "to_node": "worker", "to_input_port": "message"},
        {"from": "worker", "to_node": "end", "to_input_port": "result"}
      ]
    }"#;

    /// 两个串行 agent_call 实例（同一 run）——验证 run 级 `skill_view` 预算共享。
    const TWO_AGENT_GRAPH: &str = r#"{
      "version": "1.0",
      "metadata": {"name": "harness-two-agents"},
      "entry_points": ["start"],
      "nodes": [
        {"id": "start", "type": "start",
         "inputs": [{"name": "message", "type": "any", "required": true}], "config": {}},
        {"id": "worker1", "type": "agent_call",
         "inputs": [{"name": "message", "type": "any", "required": true}],
         "config": {"agent_id": "parser"}},
        {"id": "worker2", "type": "agent_call",
         "inputs": [{"name": "message", "type": "any", "required": true}],
         "config": {"agent_id": "parser"}},
        {"id": "end", "type": "end",
         "inputs": [{"name": "result", "type": "any", "required": true}], "config": {}}
      ],
      "edges": [
        {"from": "start", "to_node": "worker1", "to_input_port": "message"},
        {"from": "worker1", "to_node": "worker2", "to_input_port": "message"},
        {"from": "worker2", "to_node": "end", "to_input_port": "result"}
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

    /// ISSUE-082：run 结束可查询模型参数摘要（含字段、不含密钥）。
    #[tokio::test]
    async fn model_params_are_recorded_per_run_without_secrets() {
        let dir = assets(agent_config("parser", &[]), "你是解析器。");
        let fake = Arc::new(
            FakeLlmProvider::from_script(fake_script(json!([
                {"kind": "text", "content": "ok", "stop_reason": "EndTurn"}
            ])))
            .expect("script"),
        );
        let runner = Arc::new(FakeToolRunner::new());
        let executor = Arc::new(HarnessExecutor::new(settings(dir.path(), &fake, &runner)));
        let (outcome, _sink) = run_graph(AGENT_GRAPH, executor.clone(), json!("hi")).await;
        assert_eq!(outcome.status, RunStatus::Completed);
        let params = executor
            .model_params(&outcome.run_id)
            .expect("model params recorded");
        assert_eq!(params.len(), 1, "one distinct model per run");
        let summary = &params[0];
        assert_eq!(summary["provider"], json!("deepseek"));
        assert_eq!(summary["name"], json!("deepseek-chat"));
        assert_eq!(summary["base_url"], Value::Null);
        assert_eq!(summary["temperature"], json!(0.0));
        assert_eq!(summary["max_tokens"], json!(256));
        assert_eq!(summary["context_window_tokens"], json!(65536));
        let text = summary.to_string();
        assert!(
            !text.contains("api_key"),
            "model params must not leak secrets: {text}"
        );
        assert!(
            !text.contains("HARNESS_TEST_KEY"),
            "env var name excluded: {text}"
        );
        assert_eq!(executor.model_params("missing-run"), None);
    }

    /// ISSUE-082：同一 run 内相同模型参数去重、不同模型并列。
    #[test]
    fn model_params_dedup_within_run() {
        let executor = HarnessExecutor::new(HarnessSettings {
            assets_dir: PathBuf::from("/tmp/assets"),
            tools_dir: PathBuf::from("/tmp/tools"),
            provider: Arc::new(
                FakeLlmProvider::from_script(fake_script(json!([
                    {"kind": "text", "content": "unused"}
                ])))
                .expect("script"),
            ),
            fake_mode: true,
            tool_runner: Arc::new(FakeToolRunner::new()),
        });
        let model = |temperature: f32| ModelConfig {
            provider: crate::llm::ProviderKind::Deepseek,
            name: "deepseek-chat".to_string(),
            base_url: None,
            api_key_env: "SECRET_ENV".to_string(),
            temperature,
            max_tokens: 256,
            context_window_tokens: 65_536,
        };
        executor.record_model_params("run-a", &model(0.2));
        executor.record_model_params("run-a", &model(0.2));
        executor.record_model_params("run-a", &model(0.7));
        executor.record_model_params("run-b", &model(0.2));
        assert_eq!(executor.model_params("run-a").expect("run-a").len(), 2);
        assert_eq!(executor.model_params("run-b").expect("run-b").len(), 1);
        assert_eq!(executor.model_params("run-c"), None);
    }

    /// ISSUE-082 / REV-048 C5：`base_url` 摘要防御性脱敏（去 userinfo/query/fragment）。
    #[test]
    fn model_params_sanitize_base_url_userinfo_and_query() {
        let executor = HarnessExecutor::new(HarnessSettings {
            assets_dir: PathBuf::from("/tmp/assets"),
            tools_dir: PathBuf::from("/tmp/tools"),
            provider: Arc::new(
                FakeLlmProvider::from_script(fake_script(json!([
                    {"kind": "text", "content": "unused"}
                ])))
                .expect("script"),
            ),
            fake_mode: true,
            tool_runner: Arc::new(FakeToolRunner::new()),
        });
        let model = ModelConfig {
            provider: crate::llm::ProviderKind::OpenAiCompatible,
            name: "gpt-4o-mini".to_string(),
            base_url: Some(
                "https://alice:secret@example.com:8443/v1/chat?api_key=sk-live#frag".to_string(),
            ),
            api_key_env: "OPENAI_API_KEY".to_string(),
            temperature: 0.2,
            max_tokens: 512,
            context_window_tokens: 65_536,
        };
        executor.record_model_params("run-url", &model);
        let params = executor.model_params("run-url").expect("recorded");
        assert_eq!(
            params[0]["base_url"],
            json!("https://example.com:8443/v1/chat")
        );
        let text = params[0].to_string();
        for leaked in ["alice", "secret", "sk-live", "api_key"] {
            assert!(!text.contains(leaked), "base_url leaked {leaked}: {text}");
        }
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

    /// ISSUE-079：`with_tool_limits` 注入的共享上限对工具调用生效（经内核端到端）。
    #[tokio::test]
    async fn injected_limits_gate_tool_calls_through_kernel() {
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
        let limits = Arc::new(WorkerLimits::new(1, 8));
        let executor = Arc::new(HarnessExecutor::with_tool_limits(
            settings(dir.path(), &fake, &runner),
            limits.clone(),
        ));
        // 模拟同一 limiter 上正在执行的节点（占住唯一全局许可）。
        let held = limits.acquire_global().await.expect("node-like permit");
        let task = tokio::spawn(run_graph(AGENT_GRAPH, executor, json!("hi")));
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert!(
            !task.is_finished(),
            "discover/tool call must wait for the shared global limiter"
        );
        drop(held);
        let (outcome, _sink) = tokio::time::timeout(Duration::from_secs(5), task)
            .await
            .expect("run completes after permit released")
            .expect("join");
        assert_eq!(outcome.status, RunStatus::Completed);
        assert_eq!(runner.calls().len(), 1);
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

    /// v0.3（DR-009 / REV-097）：skills 目录经执行器注入 → catalog 进 system、
    /// `skill_view` 注册并可真实调用（FakeLlmProvider 真链路、无 worker）。
    #[tokio::test]
    async fn skills_dir_is_wired_through_executor() {
        let mut agent = agent_config("parser", &[]);
        agent
            .as_object_mut()
            .expect("agent object")
            .insert("skills".to_string(), json!(["code-review"]));
        let dir = assets(agent, "你是解析器。");
        let skill_dir = dir.path().join("skills").join("code-review");
        std::fs::create_dir_all(&skill_dir).expect("skill dir");
        std::fs::write(
            skill_dir.join("SKILL.md"),
            "---\nname: code-review\ndescription: Review a patch\n---\n\n# Review\nlook closely\n",
        )
        .expect("SKILL.md");
        let fake = Arc::new(
            FakeLlmProvider::from_script(fake_script(json!([
                {"kind": "tool_calls", "content": null,
                 "tool_calls": [{"call_id": "c1", "name": "skill_view",
                                 "args": {"name": "code-review"}}]},
                {"kind": "text", "content": "approved", "stop_reason": "EndTurn"}
            ])))
            .expect("script"),
        );
        let runner = Arc::new(FakeToolRunner::new());
        let executor = Arc::new(
            HarnessExecutor::new(settings(dir.path(), &fake, &runner))
                .with_skills_dir(Some(dir.path().join("skills"))),
        );
        let (outcome, _sink) = run_graph(AGENT_GRAPH, executor, json!("hi")).await;
        assert_eq!(
            outcome.status,
            RunStatus::Completed,
            "error: {:?}",
            outcome.error
        );

        let requests = fake.requests();
        assert!(requests.len() >= 2);
        let system_text = format!("{:?}", requests[0].messages[0]);
        assert!(
            system_text.contains("## 技能") && system_text.contains("<name>code-review</name>"),
            "system prompt carries catalog: {system_text}"
        );
        assert!(requests[0]
            .tools
            .iter()
            .any(|tool| tool.name == "skill_view"));
        // 第二轮请求里带 skill 正文（tool 消息），trace 只在事件里记 chars。
        let tool_content = requests[1]
            .messages
            .iter()
            .find_map(|message| match message {
                LlmMessage::Tool { content, .. } => Some(content.clone()),
                _ => None,
            })
            .expect("tool message");
        assert!(tool_content.contains("<skill_content name=\"code-review\">"));
        assert!(tool_content.contains("look closely"));
    }

    /// DR-009 §5（REV-097 C1）：预算按 **run_id** 隔离——同 run 返回同一计数器，
    /// 新 run / 新 executor 从 0 开始。
    #[test]
    fn skill_view_budget_is_per_run_and_resets() {
        let dir = assets(agent_config("parser", &[]), "role");
        let fake = Arc::new(
            FakeLlmProvider::from_script(fake_script(json!([
                {"kind": "text", "content": "x"}
            ])))
            .expect("script"),
        );
        let runner = Arc::new(FakeToolRunner::new());
        let executor = HarnessExecutor::new(settings(dir.path(), &fake, &runner));
        let first = executor.skill_view_budget("run-1");
        let same = executor.skill_view_budget("run-1");
        assert!(Arc::ptr_eq(&first, &same), "same run shares one counter");
        first.fetch_add(crate::constants::MAX_SKILL_VIEWS, Ordering::SeqCst);
        let other = executor.skill_view_budget("run-2");
        assert_eq!(other.load(Ordering::SeqCst), 0, "new run starts at 0");

        let fresh = HarnessExecutor::new(settings(dir.path(), &fake, &runner));
        assert_eq!(
            fresh.skill_view_budget("run-1").load(Ordering::SeqCst),
            0,
            "new executor starts at 0"
        );
    }

    /// DR-009 §5（REV-097 C1）：同一 run 的两个串行 agent_call 实例共享预算；
    /// 第 6 次新激活在第二个实例上 → `TOOL_ERROR{reason:"view_budget"}`（喂回 LLM）。
    #[tokio::test]
    async fn run_level_skill_view_budget_shared_across_agent_calls() {
        let mut agent = agent_config("parser", &[]);
        let object = agent.as_object_mut().expect("agent object");
        object.insert("skills".to_string(), json!(["*"]));
        object.insert("max_iterations".to_string(), json!(8));
        let dir = assets(agent, "你是解析器。");
        for index in 0..6 {
            let name = format!("skill-{index}");
            let skill_dir = dir.path().join("skills").join(&name);
            std::fs::create_dir_all(&skill_dir).expect("skill dir");
            std::fs::write(
                skill_dir.join("SKILL.md"),
                format!("---\nname: {name}\ndescription: skill {index}\n---\n\nBODY {index}\n"),
            )
            .expect("SKILL.md");
        }
        let view = |index: usize| {
            json!({"kind": "tool_calls", "content": null,
                   "tool_calls": [{"call_id": format!("c{index}"), "name": "skill_view",
                                   "args": {"name": format!("skill-{index}")}}]})
        };
        let mut calls: Vec<Value> = (0..5)
            .map(view)
            .chain([json!({"kind": "text", "content": "first"})])
            .collect();
        calls.push(view(5));
        calls.push(json!({"kind": "text", "content": "second"}));
        let fake =
            Arc::new(FakeLlmProvider::from_script(fake_script(json!(calls))).expect("script"));
        let runner = Arc::new(FakeToolRunner::new());
        let executor = Arc::new(HarnessExecutor::new(settings(dir.path(), &fake, &runner)));
        let (outcome, sink) = run_graph(TWO_AGENT_GRAPH, executor, json!("hi")).await;
        assert_eq!(
            outcome.status,
            RunStatus::Completed,
            "view_budget is a tool error (fed back), not a run failure: {:?}",
            outcome.error
        );

        // 第二个实例的第 6 次新激活 → 工具结果含 view_budget。
        let last_tool = fake
            .requests()
            .last()
            .expect("request")
            .messages
            .iter()
            .rev()
            .find_map(|message| match message {
                LlmMessage::Tool { content, .. } => Some(content.clone()),
                _ => None,
            })
            .expect("tool message");
        assert!(last_tool.contains("view_budget"), "content: {last_tool}");

        // trace 证据：恰一次 ToolCallEnd ok=false。
        let failures = sink
            .streaming_calls()
            .into_iter()
            .filter(|call| {
                matches!(
                    call.event,
                    StreamingEvent::ToolCallEnd {
                        tool_name: ref name,
                        ok: false,
                        ..
                    } if name == "skill_view"
                )
            })
            .count();
        assert_eq!(failures, 1);
    }

    // ------------------------------------------------------------------
    // v0.3 MCP（DR-010 / REV-098）：执行器级 fail-closed + FakeMcpClient 真链路
    // ------------------------------------------------------------------

    fn assets_with_mcp() -> TempDir {
        let mut agent = agent_config("parser", &[]);
        let object = agent.as_object_mut().expect("agent object");
        object.insert("mcp".to_string(), json!({"servers": ["fake"], "allow": []}));
        object.insert("tools".to_string(), json!(["mcp__fake__echo"]));
        let dir = assets(agent, "你是解析器。");
        std::fs::write(
            dir.path().join("mcp_servers.json"),
            json!({
                "mcpServers": {
                    "fake": {"command": "fake-mcp", "args": ["--stdio"], "timeout_ms": 1000}
                }
            })
            .to_string(),
        )
        .expect("mcp registry");
        dir
    }

    fn fake_mcp_factory() -> Arc<crate::mcp::testing::FakeMcpFactory> {
        let client = crate::mcp::testing::FakeMcpClient::new()
            .with_tools(vec![crate::mcp::McpTool {
                server: String::new(),
                name: "echo".to_string(),
                description: "echo args".to_string(),
                input_schema: json!({"type": "object", "properties": {}}),
            }])
            .with_default_outcome("mcp-ok", false);
        crate::mcp::testing::FakeMcpFactory::new().with_client("fake", client) as _
    }

    #[tokio::test]
    async fn mcp_tools_are_bridged_in_process_when_allowed() {
        let dir = assets_with_mcp();
        let fake = Arc::new(
            FakeLlmProvider::from_script(fake_script(json!([
                {"kind": "tool_calls", "content": null,
                 "tool_calls": [{"call_id": "c1", "name": "mcp__fake__echo",
                                 "args": {"q": 1}}]},
                {"kind": "text", "content": "done", "stop_reason": "EndTurn"}
            ])))
            .expect("script"),
        );
        let runner = Arc::new(FakeToolRunner::new());
        let executor = Arc::new(
            HarnessExecutor::new(settings(dir.path(), &fake, &runner))
                .with_mcp_allow(true)
                .with_mcp_factory(fake_mcp_factory()),
        );
        let (outcome, _sink) = run_graph(AGENT_GRAPH, executor, json!("hi")).await;
        assert_eq!(
            outcome.status,
            RunStatus::Completed,
            "error: {:?}",
            outcome.error
        );
        // 工具结果作为 tool 消息进入第二次请求（进程内桥接、不经 worker）。
        let tool_text = fake
            .requests()
            .iter()
            .flat_map(|request| request.messages.iter())
            .find_map(|message| match message {
                LlmMessage::Tool { content, .. } => Some(content.clone()),
                _ => None,
            })
            .expect("tool message");
        assert_eq!(tool_text, "mcp-ok");
        assert_eq!(
            requests_tool_names(&fake),
            vec!["mcp__fake__echo".to_string()]
        );
    }

    fn requests_tool_names(fake: &Arc<FakeLlmProvider>) -> Vec<String> {
        fake.requests()
            .first()
            .map(|request| request.tools.iter().map(|tool| tool.name.clone()).collect())
            .unwrap_or_default()
    }

    /// v0.3/V3（REV-098 #3）：`close_mcp` 幂等关闭全部 MCP 连接（Fake 记录 shutdown）。
    #[tokio::test]
    async fn close_mcp_shuts_down_once_and_is_idempotent() {
        let dir = assets_with_mcp();
        let fake = Arc::new(
            FakeLlmProvider::from_script(fake_script(json!([
                {"kind": "text", "content": "noop"}
            ])))
            .expect("script"),
        );
        let runner = Arc::new(FakeToolRunner::new());
        // 记录 shutdown 的 fake client（通过 factory 注入）。
        let client = crate::mcp::testing::FakeMcpClient::new()
            .with_tools(vec![crate::mcp::McpTool {
                server: String::new(),
                name: "echo".to_string(),
                description: "echo".to_string(),
                input_schema: json!({"type": "object", "properties": {}}),
            }])
            .with_default_outcome("ok", false);
        let factory =
            crate::mcp::testing::FakeMcpFactory::new().with_client("fake", client.clone());
        let executor = Arc::new(
            HarnessExecutor::new(settings(dir.path(), &fake, &runner))
                .with_mcp_allow(true)
                .with_mcp_factory(factory as _),
        );
        let (outcome, _sink) = run_graph(AGENT_GRAPH, executor.clone(), json!("hi")).await;
        assert_eq!(
            outcome.status,
            RunStatus::Completed,
            "error: {:?}",
            outcome.error
        );
        assert_eq!(
            client.shutdowns.load(std::sync::atomic::Ordering::SeqCst),
            0,
            "run 期间连接保持常驻"
        );
        executor.close_mcp().await;
        assert_eq!(
            client.shutdowns.load(std::sync::atomic::Ordering::SeqCst),
            1
        );
        executor.close_mcp().await;
        assert_eq!(
            client.shutdowns.load(std::sync::atomic::Ordering::SeqCst),
            1,
            "second close is a no-op"
        );
    }

    fn two_agent_graph(first: &str, second: &str) -> String {
        format!(
            r#"{{"version":"1.0","metadata":{{"name":"two-agents"}},"entry_points":["start"],
            "nodes":[
              {{"id":"start","type":"start","inputs":[{{"name":"message","type":"any","required":true}}],"config":{{}}}},
              {{"id":"w1","type":"agent_call","inputs":[{{"name":"message","type":"any","required":true}}],"config":{{"agent_id":"{first}"}}}},
              {{"id":"w2","type":"agent_call","inputs":[{{"name":"message","type":"any","required":true}}],"config":{{"agent_id":"{second}"}}}},
              {{"id":"end","type":"end","inputs":[{{"name":"result","type":"any","required":true}}],"config":{{}}}}
            ],
            "edges":[
              {{"from":"start","to_node":"w1","to_input_port":"message"}},
              {{"from":"w1","to_node":"w2","to_input_port":"message"}},
              {{"from":"w2","to_node":"end","to_input_port":"result"}}
            ]}}"#
        )
    }

    fn write_agent_file(dir: &TempDir, agent: Value) {
        let id = agent["id"].as_str().expect("id");
        std::fs::write(
            dir.path().join("agents").join(format!("{id}.json")),
            serde_json::to_string_pretty(&agent).expect("json"),
        )
        .expect("agent file");
    }

    fn write_registry(dir: &TempDir, servers: Value) {
        std::fs::write(
            dir.path().join("mcp_servers.json"),
            json!({"mcpServers": servers}).to_string(),
        )
        .expect("registry");
    }

    fn fake_server_tools() -> Vec<crate::mcp::McpTool> {
        vec![crate::mcp::McpTool {
            server: String::new(),
            name: "echo".to_string(),
            description: "echo".to_string(),
            input_schema: json!({"type": "object", "properties": {}}),
        }]
    }

    /// REV-098 C1：同 run 两个 agent 的 servers/allow 互相隔离；
    /// 第二个 agent 访问自己的 server 不误报 server_not_found。
    #[tokio::test]
    async fn mcp_plans_are_isolated_per_agent_in_same_run() {
        let mut agent1 = agent_config("parser", &[]);
        agent1.as_object_mut().expect("obj").insert(
            "mcp".to_string(),
            json!({"servers": ["alpha"], "allow": []}),
        );
        agent1
            .as_object_mut()
            .expect("obj")
            .insert("tools".to_string(), json!(["mcp__alpha__echo"]));
        let dir = assets(agent1, "你是解析器。");
        let mut agent2 = agent_config("parser2", &[]);
        agent2.as_object_mut().expect("obj").insert(
            "mcp".to_string(),
            json!({"servers": ["beta"], "allow": ["mcp__beta__echo"]}),
        );
        agent2
            .as_object_mut()
            .expect("obj")
            .insert("tools".to_string(), json!(["mcp__beta__echo"]));
        write_agent_file(&dir, agent2);
        write_registry(
            &dir,
            json!({
                "alpha": {"command": "fake-alpha", "args": []},
                "beta": {"command": "fake-beta", "args": []}
            }),
        );
        let alpha = crate::mcp::testing::FakeMcpClient::new()
            .with_tools(fake_server_tools())
            .with_default_outcome("alpha-ok", false);
        let beta = crate::mcp::testing::FakeMcpClient::new()
            .with_tools(fake_server_tools())
            .with_default_outcome("beta-ok", false);
        let factory = crate::mcp::testing::FakeMcpFactory::new()
            .with_client("alpha", alpha)
            .with_client("beta", beta);
        let fake = Arc::new(
            FakeLlmProvider::from_script(fake_script(json!([
                {"kind": "tool_calls", "content": null,
                 "tool_calls": [{"call_id": "a1", "name": "mcp__alpha__echo", "args": {}}]},
                {"kind": "text", "content": "one"},
                {"kind": "tool_calls", "content": null,
                 "tool_calls": [{"call_id": "b1", "name": "mcp__beta__echo", "args": {}}]},
                {"kind": "text", "content": "two"}
            ])))
            .expect("script"),
        );
        let runner = Arc::new(FakeToolRunner::new());
        let executor = Arc::new(
            HarnessExecutor::new(settings(dir.path(), &fake, &runner))
                .with_mcp_allow(true)
                .with_mcp_factory(factory as _),
        );
        let (outcome, _sink) =
            run_graph(&two_agent_graph("parser", "parser2"), executor, json!("hi")).await;
        assert_eq!(
            outcome.status,
            RunStatus::Completed,
            "error: {:?}",
            outcome.error
        );
        let texts: Vec<String> = fake
            .requests()
            .iter()
            .flat_map(|request| request.messages.iter())
            .filter_map(|message| match message {
                LlmMessage::Tool { content, .. } => Some(content.clone()),
                _ => None,
            })
            .collect();
        assert!(texts.iter().any(|text| text == "alpha-ok"), "{texts:?}");
        assert!(texts.iter().any(|text| text == "beta-ok"), "{texts:?}");
    }

    /// REV-098 C1：第二个 agent 的 allow 不被第一个 agent 的空 allow 绕过。
    #[tokio::test]
    async fn second_agent_allow_is_not_bypassed_by_first_agent() {
        let mut agent1 = agent_config("parser", &[]);
        agent1.as_object_mut().expect("obj").insert(
            "mcp".to_string(),
            json!({"servers": ["alpha"], "allow": []}),
        );
        agent1
            .as_object_mut()
            .expect("obj")
            .insert("tools".to_string(), json!(["mcp__alpha__echo"]));
        let dir = assets(agent1, "你是解析器。");
        let mut agent2 = agent_config("parser2", &[]);
        agent2.as_object_mut().expect("obj").insert(
            "mcp".to_string(),
            json!({"servers": ["alpha"], "allow": ["mcp__alpha__other"]}),
        );
        agent2
            .as_object_mut()
            .expect("obj")
            .insert("tools".to_string(), json!(["mcp__alpha__echo"]));
        write_agent_file(&dir, agent2);
        write_registry(
            &dir,
            json!({"alpha": {"command": "fake-alpha", "args": []}}),
        );
        let alpha = crate::mcp::testing::FakeMcpClient::new()
            .with_tools(fake_server_tools())
            .with_default_outcome("ok", false);
        let factory = crate::mcp::testing::FakeMcpFactory::new().with_client("alpha", alpha);
        let fake = Arc::new(
            FakeLlmProvider::from_script(fake_script(json!([
                {"kind": "text", "content": "one"},
                {"kind": "text", "content": "two"}
            ])))
            .expect("script"),
        );
        let runner = Arc::new(FakeToolRunner::new());
        let executor = Arc::new(
            HarnessExecutor::new(settings(dir.path(), &fake, &runner))
                .with_mcp_allow(true)
                .with_mcp_factory(factory as _),
        );
        let (outcome, _sink) =
            run_graph(&two_agent_graph("parser", "parser2"), executor, json!("hi")).await;
        let error = outcome.error.expect("must fail on second agent allow");
        assert_eq!(error.code, "HARNESS_MCP_ERROR", "{error:?}");
        assert!(
            error.message.contains("tool_not_allowed") || error.message.contains("not allowed"),
            "message: {}",
            error.message
        );
    }

    /// REV-098 #4：无 `mcp` feature 构建 + 配置 MCP（allow 开）→ Loading `init_failed`。
    #[cfg(not(feature = "mcp"))]
    #[tokio::test]
    async fn mcp_configured_without_feature_fails_init() {
        let dir = assets_with_mcp();
        let fake = Arc::new(
            FakeLlmProvider::from_script(fake_script(json!([
                {"kind": "text", "content": "unused"}
            ])))
            .expect("script"),
        );
        let runner = Arc::new(FakeToolRunner::new());
        let executor = Arc::new(
            HarnessExecutor::new(settings(dir.path(), &fake, &runner)).with_mcp_allow(true),
        );
        let (outcome, _sink) = run_graph(AGENT_GRAPH, executor, json!("hi")).await;
        let error = outcome.error.expect("must fail");
        assert_eq!(error.code, "HARNESS_MCP_ERROR");
    }

    /// 无 MCP 的默认执行器：`close_mcp` no-op（两次调用不 panic）。
    #[tokio::test]
    async fn close_mcp_is_noop_without_mcp() {
        let dir = assets(agent_config("parser", &[]), "role");
        let fake = Arc::new(
            FakeLlmProvider::from_script(fake_script(json!([
                {"kind": "text", "content": "x"}
            ])))
            .expect("script"),
        );
        let runner = Arc::new(FakeToolRunner::new());
        let executor = HarnessExecutor::new(settings(dir.path(), &fake, &runner));
        executor.close_mcp().await;
        executor.close_mcp().await;
    }

    #[tokio::test]
    async fn mcp_not_allowed_fails_at_loading() {
        let dir = assets_with_mcp();
        let fake = Arc::new(
            FakeLlmProvider::from_script(fake_script(json!([
                {"kind": "text", "content": "unused"}
            ])))
            .expect("script"),
        );
        let runner = Arc::new(FakeToolRunner::new());
        // 默认（未 --allow-mcp）→ HARNESS_MCP_ERROR（不启动 server）。
        let executor = Arc::new(
            HarnessExecutor::new(settings(dir.path(), &fake, &runner))
                .with_mcp_factory(fake_mcp_factory()),
        );
        let (outcome, _sink) = run_graph(AGENT_GRAPH, executor, json!("hi")).await;
        let error = outcome.error.expect("run must fail");
        assert_eq!(error.code, "HARNESS_MCP_ERROR");
    }
    /// v0.3/V4：显式 `harness.kind="builtin"` 与缺省路径行为一致（AC-20 a）。
    #[tokio::test]
    async fn explicit_builtin_harness_behaves_like_the_default_path() {
        let script = json!([
            {"kind": "text", "content": "ok", "stop_reason": "EndTurn"}
        ]);
        let fake =
            Arc::new(FakeLlmProvider::from_script(fake_script(script.clone())).expect("script"));
        let runner = Arc::new(FakeToolRunner::new());

        // 缺省（无 harness 字段）。
        let default_dir = assets(agent_config("parser", &[]), "你是解析器。");
        let default_executor = Arc::new(HarnessExecutor::new(settings(
            default_dir.path(),
            &fake,
            &runner,
        )));
        let (default_outcome, default_sink) =
            run_graph(AGENT_GRAPH, default_executor, json!("hi")).await;

        // 显式 builtin。
        let mut agent = agent_config("parser", &[]);
        agent["harness"] = json!({"kind": "builtin"});
        let builtin_dir = assets(agent, "你是解析器。");
        let builtin_executor = Arc::new(HarnessExecutor::new(settings(
            builtin_dir.path(),
            &fake,
            &runner,
        )));
        let (builtin_outcome, builtin_sink) =
            run_graph(AGENT_GRAPH, builtin_executor.clone(), json!("hi")).await;

        assert_eq!(default_outcome.status, zgraph_kernel::RunStatus::Completed);
        assert_eq!(builtin_outcome.status, zgraph_kernel::RunStatus::Completed);
        // AC-20 a（REV-099 N7 加强）：输出 / 事件 / 迁移 / 实例逐字段对照（忽略 UUID/时间戳）。
        assert_eq!(
            default_outcome.output, builtin_outcome.output,
            "run_output must be identical"
        );
        let project_snapshots = |sink: &MemoryTraceSink| {
            sink.snapshots()
                .into_iter()
                .map(|snapshot| {
                    (
                        snapshot.node_id,
                        snapshot.attempt,
                        snapshot.status,
                        snapshot.input,
                    )
                })
                .collect::<Vec<_>>()
        };
        assert_eq!(
            project_snapshots(&default_sink),
            project_snapshots(&builtin_sink),
            "instance lifecycle must be identical"
        );
        let project_transitions = |sink: &MemoryTraceSink| {
            sink.transitions()
                .into_iter()
                .map(|record| {
                    (
                        record.node_id,
                        record.attempt,
                        record.from,
                        record.to,
                        record.reason,
                    )
                })
                .collect::<Vec<_>>()
        };
        assert_eq!(
            project_transitions(&default_sink),
            project_transitions(&builtin_sink),
            "transition sequence must be identical"
        );
        let project_events = |sink: &MemoryTraceSink| {
            sink.streaming_calls()
                .into_iter()
                .map(|call| (call.node_id, call.event))
                .collect::<Vec<_>>()
        };
        assert_eq!(
            project_events(&default_sink),
            project_events(&builtin_sink),
            "streaming event sequence must be identical"
        );
        builtin_executor.close_provider().await;
    }

    /// v0.3/V4：subprocess harness + 节点级 `max_iterations` → 能力冲突
    /// `HARNESS_CONFIG_LOAD{reason:"provider_capability_conflict"}`（不 spawn 进程）。
    #[tokio::test]
    async fn subprocess_harness_rejects_node_max_iterations_conflict() {
        let mut agent = agent_config("parser", &[]);
        agent["harness"] = json!({
            "kind": "subprocess",
            "command": "definitely-not-a-real-harness-binary",
            "protocol": "zgraph-harness/1"
        });
        let dir = assets(agent, "你是解析器。");
        let fake = Arc::new(
            FakeLlmProvider::from_script(fake_script(json!([
                {"kind": "text", "content": "never"}
            ])))
            .expect("script"),
        );
        let runner = Arc::new(FakeToolRunner::new());
        let executor = Arc::new(HarnessExecutor::new(settings(dir.path(), &fake, &runner)));
        let (outcome, _) =
            run_graph(AGENT_GRAPH_WITH_ITERATIONS, executor.clone(), json!("hi")).await;
        assert_eq!(outcome.status, zgraph_kernel::RunStatus::Failed);
        assert_eq!(
            outcome.error.as_ref().map(|error| error.code.as_str()),
            Some("HARNESS_CONFIG_LOAD")
        );
        executor.close_provider().await;
    }
    // ------------------------------------------------------------------
    // v0.4/D2：内置文件工具 opt-in / 白名单 / 冲突 / streaming / sink
    // ------------------------------------------------------------------

    fn builtin_set(dir: &Path) -> Arc<crate::BuiltinToolSet> {
        crate::BuiltinToolSet::new(&std::fs::canonicalize(dir).expect("canonical"), &[])
            .expect("builtin set")
    }

    fn builtin_script(calls: Value) -> Value {
        fake_script(calls)
    }

    #[tokio::test]
    async fn builtin_tools_require_opt_in() {
        let dir = assets(agent_config("parser", &["builtin__read"]), "role");
        std::fs::write(dir.path().join("a.txt"), "hello").expect("file");
        let fake = Arc::new(
            FakeLlmProvider::from_script(builtin_script(json!([
                {"kind": "text", "content": "never"}
            ])))
            .expect("script"),
        );
        let runner = Arc::new(FakeToolRunner::new());
        let executor = Arc::new(HarnessExecutor::new(settings(dir.path(), &fake, &runner)));
        let (outcome, _trace) = run_graph(AGENT_GRAPH, executor, json!("hi")).await;
        assert_eq!(outcome.status, zgraph_kernel::RunStatus::Failed);
        assert_eq!(
            outcome.error.as_ref().map(|error| error.code.as_str()),
            Some("HARNESS_CONFIG_LOAD")
        );
        assert!(
            outcome
                .error
                .as_ref()
                .is_some_and(|error| error.message.contains("builtin_tools_not_allowed")),
            "{:?}",
            outcome.error
        );
    }

    #[tokio::test]
    async fn builtin_file_tool_runs_through_agent_loop_with_streaming_and_sink() {
        let dir = assets(agent_config("parser", &["builtin__read"]), "role");
        std::fs::write(dir.path().join("a.txt"), "hello builtin").expect("file");
        let fake = Arc::new(
            FakeLlmProvider::from_script(builtin_script(json!([
                {"kind": "tool_calls", "content": null, "tool_calls": [
                    {"call_id": "c1", "name": "builtin__read", "args": {"path": "a.txt"}}
                ]},
                {"kind": "text", "content": "done"}
            ])))
            .expect("script"),
        );
        let runner = Arc::new(FakeToolRunner::new());
        let sink = crate::test_support::CapturingToolCallSink::new();
        let executor = Arc::new(
            HarnessExecutor::new(settings(dir.path(), &fake, &runner))
                .with_builtin_tools(builtin_set(dir.path()))
                .with_tool_call_record_sink(sink.clone()),
        );
        let (outcome, trace) = run_graph(AGENT_GRAPH, executor.clone(), json!("hi")).await;
        assert_eq!(outcome.status, zgraph_kernel::RunStatus::Completed);
        let calls = trace.streaming_calls();
        assert!(
            calls.iter().any(|call| matches!(
                &call.event,
                StreamingEvent::ToolCallStart { tool_name, .. } if tool_name == "builtin__read"
            )),
            "{calls:?}"
        );
        assert!(
            calls.iter().any(|call| matches!(
                &call.event,
                StreamingEvent::ToolCallEnd { tool_name, ok, .. }
                    if tool_name == "builtin__read" && *ok
            )),
            "{calls:?}"
        );
        // 工具结果走既有 TOOL_ERROR/结果摘要路径（LLM 收到文件内容）。
        let records = sink.records();
        assert_eq!(records.len(), 1, "{records:?}");
        assert_eq!(records[0].0, "c1");
        assert_eq!(records[0].1, "builtin__read");
        assert_eq!(records[0].2, crate::tools::ToolSource::Builtin);
        assert!(records[0].3, "ok");
        assert!(records[0].5 > 0, "result bytes recorded");
        executor.close_provider().await;
    }

    #[tokio::test]
    async fn builtin_tool_must_be_whitelisted_in_agent_tools() {
        let dir = assets(agent_config("parser", &[]), "role");
        std::fs::write(dir.path().join("a.txt"), "hello").expect("file");
        let fake = Arc::new(
            FakeLlmProvider::from_script(builtin_script(json!([
                {"kind": "tool_calls", "content": null, "tool_calls": [
                    {"call_id": "c1", "name": "builtin__read", "args": {"path": "a.txt"}}
                ]},
                {"kind": "text", "content": "never"}
            ])))
            .expect("script"),
        );
        let runner = Arc::new(FakeToolRunner::new());
        let executor = Arc::new(
            HarnessExecutor::new(settings(dir.path(), &fake, &runner))
                .with_builtin_tools(builtin_set(dir.path())),
        );
        let (outcome, _trace) = run_graph(AGENT_GRAPH, executor, json!("hi")).await;
        assert_eq!(outcome.status, zgraph_kernel::RunStatus::Failed);
        assert_eq!(
            outcome.error.as_ref().map(|error| error.code.as_str()),
            Some("HARNESS_UNKNOWN_TOOL")
        );
    }

    #[tokio::test]
    async fn python_tool_cannot_use_reserved_builtin_prefix() {
        let dir = assets(agent_config("parser", &["builtin__read"]), "role");
        std::fs::write(dir.path().join("a.txt"), "hello").expect("file");
        let python_tool = ToolDef {
            name: "builtin__read".to_string(),
            description: "shadow".to_string(),
            parameters: json!({"type": "object", "properties": {}}),
            timeout_ms: 1_000,
            source: "tools.legacy".to_string(),
        };
        let runner = Arc::new(FakeToolRunner::new().tool(python_tool));
        let fake = Arc::new(
            FakeLlmProvider::from_script(builtin_script(json!([
                {"kind": "text", "content": "never"}
            ])))
            .expect("script"),
        );
        let executor = Arc::new(
            HarnessExecutor::new(settings(dir.path(), &fake, &runner))
                .with_builtin_tools(builtin_set(dir.path())),
        );
        let (outcome, _trace) = run_graph(AGENT_GRAPH, executor, json!("hi")).await;
        assert_eq!(outcome.status, zgraph_kernel::RunStatus::Failed);
        assert_eq!(
            outcome.error.as_ref().map(|error| error.code.as_str()),
            Some("HARNESS_CONFIG_LOAD")
        );
        assert!(
            outcome
                .error
                .as_ref()
                .is_some_and(|error| error.message.contains("reserved")),
            "{:?}",
            outcome.error
        );
    }
    /// REV-106 N1：即使**未 opt-in**，Python 工具占用保留前缀 `builtin__` 也必须在
    /// 注册期 fail-closed（DR-013 §1.1 命名空间保留）。
    #[tokio::test]
    async fn reserved_builtin_prefix_is_rejected_without_opt_in() {
        let dir = assets(agent_config("parser", &[]), "role");
        let python_tool = ToolDef {
            name: "builtin__read".to_string(),
            description: "shadow".to_string(),
            parameters: json!({"type": "object", "properties": {}}),
            timeout_ms: 1_000,
            source: "tools.legacy".to_string(),
        };
        let runner = Arc::new(FakeToolRunner::new().tool(python_tool));
        let fake = Arc::new(
            FakeLlmProvider::from_script(fake_script(json!([
                {"kind": "text", "content": "never"}
            ])))
            .expect("script"),
        );
        // 注意：不注入 builtin set（未 opt-in）。
        let executor = Arc::new(HarnessExecutor::new(settings(dir.path(), &fake, &runner)));
        let (outcome, _trace) = run_graph(AGENT_GRAPH, executor, json!("hi")).await;
        assert_eq!(outcome.status, zgraph_kernel::RunStatus::Failed);
        assert_eq!(
            outcome.error.as_ref().map(|error| error.code.as_str()),
            Some("HARNESS_CONFIG_LOAD")
        );
        assert!(
            outcome
                .error
                .as_ref()
                .is_some_and(|error| error.message.contains("reserved")),
            "{:?}",
            outcome.error
        );
    }
    // ------------------------------------------------------------------
    // v0.4/D3：grep/glob 注册（与文件工具共存）/白名单/streaming+sink
    // ------------------------------------------------------------------

    #[tokio::test]
    async fn search_tools_register_with_file_tools_and_run_through_agent_loop() {
        let dir = assets(
            agent_config(
                "parser",
                &["builtin__read", "builtin__grep", "builtin__glob"],
            ),
            "role",
        );
        std::fs::write(dir.path().join("a.txt"), "needle one\nmore needle\n").expect("file");
        let fake = Arc::new(
            FakeLlmProvider::from_script(fake_script(json!([
                {"kind": "tool_calls", "content": null, "tool_calls": [
                    {"call_id": "g1", "name": "builtin__grep",
                     "args": {"pattern": "needle", "path": "."}}
                ]},
                {"kind": "tool_calls", "content": null, "tool_calls": [
                    {"call_id": "l1", "name": "builtin__glob",
                     "args": {"pattern": "*.txt"}}
                ]},
                {"kind": "text", "content": "done"}
            ])))
            .expect("script"),
        );
        let runner = Arc::new(FakeToolRunner::new());
        let sink = crate::test_support::CapturingToolCallSink::new();
        let executor = Arc::new(
            HarnessExecutor::new(settings(dir.path(), &fake, &runner))
                .with_builtin_tools(builtin_set(dir.path()))
                .with_tool_call_record_sink(sink.clone()),
        );
        let (outcome, trace) = run_graph(AGENT_GRAPH, executor.clone(), json!("hi")).await;
        assert_eq!(outcome.status, zgraph_kernel::RunStatus::Completed);
        let calls = trace.streaming_calls();
        for expected in ["builtin__grep", "builtin__glob"] {
            assert!(
                calls.iter().any(|call| matches!(
                    &call.event,
                    StreamingEvent::ToolCallEnd { tool_name, ok, .. }
                        if tool_name == expected && *ok
                )),
                "{expected} missing: {calls:?}"
            );
        }
        let records = sink.records();
        assert_eq!(records.len(), 2, "{records:?}");
        assert!(records
            .iter()
            .all(|record| record.2 == crate::tools::ToolSource::Builtin && record.3));
        executor.close_provider().await;
    }

    #[tokio::test]
    async fn search_tools_must_be_whitelisted() {
        let dir = assets(agent_config("parser", &["builtin__read"]), "role");
        std::fs::write(dir.path().join("a.txt"), "needle\n").expect("file");
        let fake = Arc::new(
            FakeLlmProvider::from_script(fake_script(json!([
                {"kind": "tool_calls", "content": null, "tool_calls": [
                    {"call_id": "g1", "name": "builtin__grep", "args": {"pattern": "needle"}}
                ]},
                {"kind": "text", "content": "never"}
            ])))
            .expect("script"),
        );
        let runner = Arc::new(FakeToolRunner::new());
        let executor = Arc::new(
            HarnessExecutor::new(settings(dir.path(), &fake, &runner))
                .with_builtin_tools(builtin_set(dir.path())),
        );
        let (outcome, _trace) = run_graph(AGENT_GRAPH, executor, json!("hi")).await;
        assert_eq!(outcome.status, zgraph_kernel::RunStatus::Failed);
        assert_eq!(
            outcome.error.as_ref().map(|error| error.code.as_str()),
            Some("HARNESS_UNKNOWN_TOOL")
        );
    }
}
