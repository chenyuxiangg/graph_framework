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
use crate::observability::{BudgetConfig, PricingTable, RunObservability, RunUsageStats};
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
    /// v0.5/F3a1：可选价格表（`--pricing`；默认 `None` = 不计算费用）。
    pricing: Option<Arc<PricingTable>>,
    /// v0.5/F3a1：预算上限（默认关；`max_cost` 需要 `pricing`）。
    budget: BudgetConfig,
    /// v0.5/F3a1：`run_id` → run 级计费/用量/预算状态（跨 agent_call 累计）。
    run_observability: Mutex<HashMap<String, Arc<RunObservability>>>,
    /// v0.5/F3a1（REV-123 C2）：`max_cost` 下 run 全部 Agent 模型（F3b CLI 从 graph
    /// AgentConfig 收集后传入）；未提供 → 加载期 fail-closed（零请求）。
    required_models: Option<BTreeSet<String>>,
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
            pricing: None,
            budget: BudgetConfig::default(),
            run_observability: Mutex::new(HashMap::new()),
            required_models: None,
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
            pricing: None,
            budget: BudgetConfig::default(),
            run_observability: Mutex::new(HashMap::new()),
            required_models: None,
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
            pricing: None,
            budget: BudgetConfig::default(),
            run_observability: Mutex::new(HashMap::new()),
            required_models: None,
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
            pricing: None,
            budget: BudgetConfig::default(),
            run_observability: Mutex::new(HashMap::new()),
            required_models: None,
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

    /// v0.5/F3a1（DR-018 §2）：注入价格表（`--pricing`；不内置价格）。
    ///
    /// 未知模型 → 该调用费用不可用 + warning（不 fail run）；`max_cost` 预算需要
    /// 价格表（缺 → 执行期配置错误，F3b CLI 启动前预检）。
    pub fn with_pricing(mut self, pricing: Arc<PricingTable>) -> Self {
        self.pricing = Some(pricing);
        self
    }

    /// v0.5/F3a1（DR-018 §3）：注入预算上限（默认关；`max_tokens`/`max_cost`）。
    ///
    /// 超限检查发生在**每次 LLM 请求前**（`>=`），停止而不发起该请求；结构化结果
    /// 经 `RunFailure::BudgetExceeded` 返回（F3a2 映射 kernel `BUDGET_EXCEEDED`）。
    pub fn with_budget(mut self, budget: BudgetConfig) -> Self {
        self.budget = budget;
        self
    }

    /// v0.5/F3a1（REV-123 C2）：声明本 run 全部 Agent 模型（`max_cost` 必需）。
    ///
    /// F3b CLI 从 graph 的 AgentConfig 收集后传入；`max_cost` 下本执行器在**首次
    /// LLM 请求前**整体校验这些模型都有价格（缺 → `pricing_missing_model`）；未提供
    /// → `pricing_models_required`（fail-closed，零请求）。无 `max_cost` 时忽略。
    pub fn with_required_models(mut self, models: impl IntoIterator<Item = String>) -> Self {
        self.required_models = Some(models.into_iter().collect());
        self
    }

    /// [`Self::with_required_models`] 的别名（语义同）。
    pub fn with_agent_models(self, models: impl IntoIterator<Item = String>) -> Self {
        self.with_required_models(models)
    }

    /// v0.5/F3a1：本 run 的统计快照（run/node/agent token/耗时/调用数/费用）。
    ///
    /// addditive API（F3b trace/`--stats` 消费）；未知 run = `None`。
    pub fn run_usage_stats(&self, run_id: &str) -> Option<RunUsageStats> {
        let guard = match self.run_observability.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        guard
            .get(run_id)
            .map(|observability| observability.snapshot())
    }

    /// v0.5/F3a1：本 run 的可观测/预算状态（F3b 可读取 budget stop/warnings）。
    pub fn run_observability(&self, run_id: &str) -> Option<Arc<RunObservability>> {
        let guard = match self.run_observability.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        guard.get(run_id).cloned()
    }

    /// v0.5/F3a1：取/建 run 级可观测状态（同一 run 的所有 agent_call 共享）。
    fn observability_for(&self, run_id: &str) -> Arc<RunObservability> {
        let mut runs = match self.run_observability.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        runs.entry(run_id.to_string())
            .or_insert_with(|| RunObservability::new(self.pricing.clone(), self.budget))
            .clone()
    }

    /// v0.5/F3a1（REV-123 C2）：run 级预算/价格整体预检（首次 LLM 请求前）。
    ///
    /// `max_cost` 下必须：预算合法 → 有 USD 价格表 → 已提供 required models →
    /// required models 全部有价；任一不满足 → `HARNESS_CONFIG_LOAD`（fail-closed，
    /// 零 LLM 请求）。无 `max_cost` 时不做任何额外要求（默认路径不变）。
    fn validate_run_pricing(&self) -> Result<(), HarnessError> {
        self.budget.validate()?;
        if self.budget.max_cost.is_none() {
            return Ok(());
        }
        let Some(pricing) = self.pricing.as_ref() else {
            return Err(HarnessError::config_load(
                "pricing_required_for_max_cost",
                Some("<pricing>"),
                Some("$.max_cost"),
                Some(serde_json::json!("--pricing <file> (USD)")),
                None,
                "--max-cost requires --pricing (no built-in prices)",
            ));
        };
        let Some(required) = self.required_models.as_ref() else {
            return Err(HarnessError::config_load(
                "pricing_models_required",
                Some("<pricing>"),
                Some("$.required_models"),
                Some(serde_json::json!("all AgentConfig models in this run")),
                None,
                "--max-cost requires the run's Agent models (with_required_models) for pricing validation",
            ));
        };
        for model in required {
            if !pricing.has_model(model) {
                return Err(HarnessError::config_load(
                    "pricing_missing_model",
                    Some("<pricing>"),
                    Some("$.models"),
                    Some(serde_json::json!(model)),
                    Some(serde_json::json!(pricing.currency())),
                    format!(
                        "--max-cost requires pricing for agent model '{model}'; add it to --pricing"
                    ),
                ));
            }
        }
        Ok(())
    }

    /// v0.5/F3a1：预算配置前置校验（`max_cost` 需要 USD 价格表；F3b 也会启动前预检）。
    fn observability_for_job(
        &self,
        job: &AgentCallJob,
        model: &str,
    ) -> Result<Arc<RunObservability>, NodeError> {
        self.budget
            .validate()
            .map_err(|error| error.into_node_error())?;
        if self.budget.max_cost.is_some() {
            let Some(pricing) = self.pricing.as_ref() else {
                return Err(HarnessError::config_load(
                    "pricing_required_for_max_cost",
                    Some("<pricing>"),
                    Some("$.max_cost"),
                    Some(serde_json::json!("--pricing <file> (USD)")),
                    None,
                    "--max-cost requires --pricing (no built-in prices)",
                )
                .into_node_error());
            };
            // REV-123 C2：`max_cost` 下 Agent 模型必须在价格表中（加载后、首次 LLM
            // 请求前 fail-closed；pricing 缺模型不静默按 0/未知继续）。
            if !pricing.has_model(model) {
                return Err(HarnessError::config_load(
                    "pricing_missing_model",
                    Some("<pricing>"),
                    Some("$.models"),
                    Some(serde_json::json!(model)),
                    Some(serde_json::json!(pricing.currency())),
                    format!(
                        "--max-cost requires pricing for agent model '{model}'; add it to --pricing"
                    ),
                )
                .into_node_error());
            }
        }
        Ok(self.observability_for(job.ctx.run_id()))
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
        // REV-123 C2：run 级价格/预算整体预检在**任何 Agent 加载/LLM 请求之前**。
        if let Err(error) = self.validate_run_pricing() {
            let node_error = config_failure(error, &job.ctx, job.resolved_timeout_ms);
            return Box::pin(async move { Err(node_error) });
        }
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

    fn node_id(&self) -> &str {
        NodeContext::node_id(&self.inner)
    }

    fn attempt(&self) -> u32 {
        NodeContext::attempt(&self.inner)
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
        let observability = self.observability_for_job(&job, &agent.model_ref().name)?;
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
            observability: Some(observability),
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

    /// 两个串行 agent_call、不同 Agent（不同 model）——REV-123 C2 多模型预检。
    const TWO_MODEL_GRAPH: &str = r#"{
      "version": "1.0",
      "metadata": {"name": "harness-two-models"},
      "entry_points": ["start"],
      "nodes": [
        {"id": "start", "type": "start",
         "inputs": [{"name": "message", "type": "any", "required": true}], "config": {}},
        {"id": "worker1", "type": "agent_call",
         "inputs": [{"name": "message", "type": "any", "required": true}],
         "config": {"agent_id": "parser"}},
        {"id": "worker2", "type": "agent_call",
         "inputs": [{"name": "message", "type": "any", "required": true}],
         "config": {"agent_id": "parser2"}},
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
        let sink = crate::test_support::RecordingToolCallSink::new();
        let executor = Arc::new(
            HarnessExecutor::new(settings(dir.path(), &fake, &runner))
                .with_builtin_tools(builtin_set(dir.path()))
                .with_tool_call_record_sink(sink.clone()),
        );
        let (outcome, trace) = run_graph(AGENT_GRAPH, executor.clone(), json!("hi")).await;
        assert_eq!(
            outcome.status,
            zgraph_kernel::RunStatus::Completed,
            "{:?}",
            outcome.error
        );
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
        assert_eq!(records[0].call_id, "c1");
        assert_eq!(records[0].tool_name, "builtin__read");
        assert_eq!(records[0].source, crate::tools::ToolCallSource::Builtin);
        assert_eq!(records[0].status, crate::tools::ToolCallStatus::Ok);
        assert_eq!(
            records[0]
                .result
                .as_ref()
                .and_then(|value| value["content"].as_str()),
            Some("hello builtin")
        );
        assert!(records[0].duration_ms < 60_000);
        assert!(!records[0].truncated);
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
        let sink = crate::test_support::RecordingToolCallSink::new();
        let executor = Arc::new(
            HarnessExecutor::new(settings(dir.path(), &fake, &runner))
                .with_builtin_tools(builtin_set(dir.path()))
                .with_tool_call_record_sink(sink.clone()),
        );
        let (outcome, trace) = run_graph(AGENT_GRAPH, executor.clone(), json!("hi")).await;
        assert_eq!(
            outcome.status,
            zgraph_kernel::RunStatus::Completed,
            "{:?}",
            outcome.error
        );
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
        assert!(records.iter().all(|record| {
            record.source == crate::tools::ToolCallSource::Builtin
                && record.status == crate::tools::ToolCallStatus::Ok
        }));
        assert!(records.iter().all(|record| record.side_effect.is_none()));
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
    // ------------------------------------------------------------------
    // v0.4/D4：builtin__bash opt-in / whitelist / streaming+sink（Fake runner）
    // ------------------------------------------------------------------

    #[tokio::test]
    async fn bash_tool_runs_through_agent_loop_with_fake_sandbox_runner() {
        let dir = assets(agent_config("parser", &["builtin__bash"]), "role");
        let fake = Arc::new(
            FakeLlmProvider::from_script(fake_script(json!([
                {"kind": "tool_calls", "content": null, "tool_calls": [
                    {"call_id": "b1", "name": "builtin__bash",
                     "args": {"command": "echo hi"}}
                ]},
                {"kind": "text", "content": "done"}
            ])))
            .expect("script"),
        );
        let runner = Arc::new(FakeToolRunner::new());
        let sandbox = crate::test_support::FakeSandboxRunner::new();
        let set = crate::BuiltinToolSet::new_with_runner(
            &std::fs::canonicalize(dir.path()).expect("canonical"),
            &[],
            false,
            sandbox.clone(),
        )
        .expect("bash-only set");
        let sink = crate::test_support::RecordingToolCallSink::new();
        let executor = Arc::new(
            HarnessExecutor::new(settings(dir.path(), &fake, &runner))
                .with_builtin_tools(set)
                .with_tool_call_record_sink(sink.clone()),
        );
        let (outcome, trace) = run_graph(AGENT_GRAPH, executor.clone(), json!("hi")).await;
        assert_eq!(
            outcome.status,
            zgraph_kernel::RunStatus::Completed,
            "{:?}",
            outcome.error
        );
        let calls = trace.streaming_calls();
        assert!(
            calls.iter().any(|call| matches!(
                &call.event,
                StreamingEvent::ToolCallEnd { tool_name, ok, .. }
                    if tool_name == "builtin__bash" && *ok
            )),
            "{calls:?}"
        );
        let records = sink.records();
        assert_eq!(records.len(), 1, "{records:?}");
        assert_eq!(records[0].tool_name, "builtin__bash");
        assert_eq!(records[0].status, crate::tools::ToolCallStatus::Ok);
        let bash = records[0].bash.as_ref().expect("bash metadata");
        assert_eq!(bash.command, "echo hi");
        assert_eq!(bash.exit_code, Some(0));
        assert_eq!(bash.stdout, "ok");
        assert_eq!(
            bash.sandbox_profile_sha256.as_deref().map(str::len),
            Some(64)
        );
        let requests = sandbox.requests();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].command, "echo hi");
        executor.close_provider().await;
    }

    #[tokio::test]
    async fn bash_reference_requires_allow_bash_registration() {
        // 文件/搜索工具已开启，但 bash 未开启 → registry 解析失败（tool_not_registered）。
        let dir = assets(agent_config("parser", &["builtin__bash"]), "role");
        let fake = Arc::new(
            FakeLlmProvider::from_script(fake_script(json!([
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
            Some("HARNESS_CONFIG_LOAD")
        );
        assert!(
            outcome
                .error
                .as_ref()
                .is_some_and(|error| error.message.contains("tool_not_registered")),
            "{:?}",
            outcome.error
        );
    }

    #[tokio::test]
    async fn bash_reference_without_any_opt_in_is_builtin_tools_not_allowed() {
        let dir = assets(agent_config("parser", &["builtin__bash"]), "role");
        let fake = Arc::new(
            FakeLlmProvider::from_script(fake_script(json!([
                {"kind": "text", "content": "never"}
            ])))
            .expect("script"),
        );
        let runner = Arc::new(FakeToolRunner::new());
        let executor = Arc::new(HarnessExecutor::new(settings(dir.path(), &fake, &runner)));
        let (outcome, _trace) = run_graph(AGENT_GRAPH, executor, json!("hi")).await;
        assert_eq!(outcome.status, zgraph_kernel::RunStatus::Failed);
        assert!(
            outcome
                .error
                .as_ref()
                .is_some_and(|error| error.message.contains("builtin_tools_not_allowed")),
            "{:?}",
            outcome.error
        );
    }
    // ------------------------------------------------------------------
    // v0.4/D5b1：工具调用记录富化（trace tool_calls 映射）
    // ------------------------------------------------------------------

    fn builtin_set_with_fake_bash(
        dir: &Path,
    ) -> (
        Arc<crate::test_support::FakeSandboxRunner>,
        Arc<crate::BuiltinToolSet>,
    ) {
        let sandbox = crate::test_support::FakeSandboxRunner::new();
        let set = crate::BuiltinToolSet::new_with_runner(
            &std::fs::canonicalize(dir).expect("canonical"),
            &[],
            true,
            sandbox.clone(),
        )
        .expect("builtin set");
        (sandbox, set)
    }

    fn assert_rfc3339_millis(value: &str) {
        // 无 chrono 依赖：校验固定 24 字符 `YYYY-MM-DDTHH:MM:SS.mmmZ` 形状与字段范围。
        assert_eq!(value.len(), 24, "RFC3339 millis length: {value}");
        assert!(value.ends_with('Z'), "UTC Z suffix: {value}");
        assert_eq!(&value[4..5], "-");
        assert_eq!(&value[7..8], "-");
        assert_eq!(&value[10..11], "T");
        assert_eq!(&value[13..14], ":");
        assert_eq!(&value[16..17], ":");
        assert_eq!(&value[19..20], ".");
        let parse = |range: std::ops::Range<usize>| -> u32 {
            value[range]
                .parse()
                .unwrap_or_else(|_| panic!("non-numeric: {value}"))
        };
        let month = parse(5..7);
        let day = parse(8..10);
        let hour = parse(11..13);
        let minute = parse(14..16);
        let second = parse(17..19);
        let _millis = parse(20..23);
        assert!((1..=12).contains(&month), "{value}");
        assert!((1..=31).contains(&day), "{value}");
        assert!(hour < 24 && minute < 60 && second < 60, "{value}");
    }

    #[tokio::test]
    async fn tool_call_records_cover_all_builtin_tools_with_effects_and_bash() {
        let mut agent = agent_config(
            "parser",
            &[
                "builtin__read",
                "builtin__write",
                "builtin__edit",
                "builtin__diff",
                "builtin__apply_patch",
                "builtin__grep",
                "builtin__glob",
                "builtin__bash",
            ],
        );
        agent["max_iterations"] = json!(16);
        let dir = assets(agent, "role");
        let root = dir.path();
        std::fs::write(root.join("a.txt"), "hello builtin").expect("a");
        std::fs::write(root.join("c.txt"), "a needle\n").expect("c");
        std::fs::write(root.join("a2.txt"), "x\n").expect("a2");
        let single_patch = "--- a/w.txt\n+++ b/w.txt\n@@ -1 +1 @@\n-world\n+WORLD\n";
        let multi_patch = "--- a/w.txt\n+++ b/w.txt\n@@ -1 +1 @@\n-WORLD\n+W\n--- a/a2.txt\n+++ b/a2.txt\n@@ -1 +1 @@\n-x\n+X\n";
        let fake = Arc::new(
            FakeLlmProvider::from_script(fake_script(json!([
                {"kind": "tool_calls", "content": null, "tool_calls": [
                    {"call_id": "r1", "name": "builtin__read", "args": {"path": "a.txt"}}
                ]},
                {"kind": "tool_calls", "content": null, "tool_calls": [
                    {"call_id": "w1", "name": "builtin__write",
                     "args": {"path": "w.txt", "content": "hello"}}
                ]},
                {"kind": "tool_calls", "content": null, "tool_calls": [
                    {"call_id": "e1", "name": "builtin__edit",
                     "args": {"path": "w.txt", "old_string": "hello", "new_string": "world"}}
                ]},
                {"kind": "tool_calls", "content": null, "tool_calls": [
                    {"call_id": "d1", "name": "builtin__diff",
                     "args": {"path": "c.txt", "old_string": "needle", "new_string": "NEEDLE"}}
                ]},
                {"kind": "tool_calls", "content": null, "tool_calls": [
                    {"call_id": "p1", "name": "builtin__apply_patch",
                     "args": {"patch": single_patch}}
                ]},
                {"kind": "tool_calls", "content": null, "tool_calls": [
                    {"call_id": "p2", "name": "builtin__apply_patch",
                     "args": {"patch": multi_patch}}
                ]},
                {"kind": "tool_calls", "content": null, "tool_calls": [
                    {"call_id": "g1", "name": "builtin__grep", "args": {"pattern": "needle"}}
                ]},
                {"kind": "tool_calls", "content": null, "tool_calls": [
                    {"call_id": "l1", "name": "builtin__glob", "args": {"pattern": "*.txt"}}
                ]},
                {"kind": "tool_calls", "content": null, "tool_calls": [
                    {"call_id": "b1", "name": "builtin__bash",
                     "args": {"command": "echo hi",
                              "env": {"API_KEY": "sk-secret", "CUSTOM_FLAG": "on"}}}
                ]},
                {"kind": "text", "content": "done"}
            ])))
            .expect("script"),
        );
        let runner = Arc::new(FakeToolRunner::new());
        let (_sandbox, set) = builtin_set_with_fake_bash(root);
        let sink = crate::test_support::RecordingToolCallSink::new();
        let executor = Arc::new(
            HarnessExecutor::new(settings(root, &fake, &runner))
                .with_builtin_tools(set)
                .with_tool_call_record_sink(sink.clone()),
        );
        let (outcome, _trace) = run_graph(AGENT_GRAPH, executor.clone(), json!("hi")).await;
        assert_eq!(
            outcome.status,
            zgraph_kernel::RunStatus::Completed,
            "{:?}",
            outcome.error
        );
        let records = sink.records();
        let ids: Vec<&str> = records
            .iter()
            .map(|record| record.call_id.as_str())
            .collect();
        assert_eq!(
            ids,
            vec!["r1", "w1", "e1", "d1", "p1", "p2", "g1", "l1", "b1"]
        );
        for record in &records {
            assert_eq!(record.source, crate::tools::ToolCallSource::Builtin);
            assert_eq!(record.status, crate::tools::ToolCallStatus::Ok);
            assert_rfc3339_millis(&record.started_at);
            assert_rfc3339_millis(&record.finished_at);
            assert!(record.duration_ms < 60_000);
            assert!(record.result.is_some(), "{record:?}");
            assert!(record.error.is_none(), "{record:?}");
        }
        // 文件类 side_effect。
        assert!(records[0].side_effect.is_none(), "read has no side effect");
        let write = records[1].side_effect.as_ref().expect("write side effect");
        assert_eq!(write.path, "w.txt");
        assert_eq!(write.before_sha256, None);
        assert_eq!(write.bytes_before, None);
        assert_eq!(write.after_sha256.as_deref().map(str::len), Some(64));
        assert_eq!(write.bytes_after, Some(5));
        let edit = records[2].side_effect.as_ref().expect("edit side effect");
        assert_eq!(edit.path, "w.txt");
        assert_eq!(edit.bytes_before, Some(5));
        assert_eq!(edit.bytes_after, Some(5));
        assert!(edit.before_sha256.is_some() && edit.after_sha256.is_some());
        assert_eq!(edit.before_sha256, write.after_sha256);
        // diff 无 side effect。
        assert!(records[3].side_effect.is_none());
        // 单文件 apply_patch 有 side effect；多文件为 None（files[] 在 result）。
        let patch_single = records[4].side_effect.as_ref().expect("single patch");
        assert_eq!(patch_single.path, "w.txt");
        assert_eq!(patch_single.bytes_before, Some(5));
        // 手写 patch 无 `\ No newline` 标记：apply_file 在 EOF 处补换行（"WORLD\n"=6）。
        assert_eq!(patch_single.bytes_after, Some(6));
        assert!(records[5].side_effect.is_none(), "multi-file patch = None");
        assert_eq!(
            records[5].result.as_ref().unwrap()["files"]
                .as_array()
                .expect("files")
                .len(),
            2
        );
        // 搜索无 side effect。
        assert!(records[6].side_effect.is_none() && records[7].side_effect.is_none());
        // bash 全量元数据。
        let bash = records[8].bash.as_ref().expect("bash record");
        assert_eq!(bash.command, "echo hi");
        assert_eq!(bash.exit_code, Some(0));
        assert_eq!(bash.stdout, "ok");
        assert_eq!(
            bash.sandbox_profile_sha256.as_deref().map(str::len),
            Some(64)
        );
        assert!(!records[8].truncated);
        executor.close_provider().await;
    }

    #[tokio::test]
    async fn tool_call_records_mark_error_and_timeout_statuses() {
        let dir = assets(
            agent_config("parser", &["builtin__read", "builtin__bash"]),
            "role",
        );
        let fake = Arc::new(
            FakeLlmProvider::from_script(fake_script(json!([
                {"kind": "tool_calls", "content": null, "tool_calls": [
                    {"call_id": "r_err", "name": "builtin__read", "args": {"path": "missing.txt"}}
                ]},
                {"kind": "tool_calls", "content": null, "tool_calls": [
                    {"call_id": "b_timeout", "name": "builtin__bash", "args": {"command": "sleep 9"}}
                ]},
                {"kind": "text", "content": "done"}
            ])))
            .expect("script"),
        );
        let runner = Arc::new(FakeToolRunner::new());
        let (sandbox, set) = builtin_set_with_fake_bash(dir.path());
        sandbox.push_outcome(Err(crate::ToolFailure::new("timeout", "timed out")));
        let sink = crate::test_support::RecordingToolCallSink::new();
        let executor = Arc::new(
            HarnessExecutor::new(settings(dir.path(), &fake, &runner))
                .with_builtin_tools(set)
                .with_tool_call_record_sink(sink.clone()),
        );
        let (outcome, _trace) = run_graph(AGENT_GRAPH, executor.clone(), json!("hi")).await;
        assert_eq!(
            outcome.status,
            zgraph_kernel::RunStatus::Completed,
            "{:?}",
            outcome.error
        );
        let records = sink.records();
        assert_eq!(records.len(), 2, "{records:?}");
        assert_eq!(records[0].status, crate::tools::ToolCallStatus::Error);
        assert!(records[0].result.is_none());
        assert_eq!(
            records[0].error.as_ref().unwrap()["error"]["reason"],
            json!("not_found")
        );
        assert_eq!(records[1].status, crate::tools::ToolCallStatus::Timeout);
        assert_eq!(
            records[1].error.as_ref().unwrap()["error"]["reason"],
            json!("timeout")
        );
        // REV-110 N1：bash 超时记录保留 command/cwd/profile_sha（输出/exit 为空）。
        let bash = records[1].bash.as_ref().expect("bash metadata on timeout");
        assert_eq!(bash.command, "sleep 9");
        assert_eq!(bash.exit_code, None);
        assert!(bash.stdout.is_empty() && bash.stderr.is_empty());
        assert_eq!(
            bash.sandbox_profile_sha256.as_deref().map(str::len),
            Some(64)
        );
        executor.close_provider().await;
    }

    #[tokio::test]
    async fn mcp_and_python_sources_are_recorded() {
        // MCP：source=mcp。
        let dir = assets_with_mcp();
        let fake = Arc::new(
            FakeLlmProvider::from_script(fake_script(json!([
                {"kind": "tool_calls", "content": null, "tool_calls": [
                    {"call_id": "m1", "name": "mcp__fake__echo",
                     "args": {"q": 1, "token": "sk-mcp-secret"}}
                ]},
                {"kind": "text", "content": "done"}
            ])))
            .expect("script"),
        );
        let runner = Arc::new(FakeToolRunner::new());
        let sink = crate::test_support::RecordingToolCallSink::new();
        let executor = Arc::new(
            HarnessExecutor::new(settings(dir.path(), &fake, &runner))
                .with_mcp_allow(true)
                .with_mcp_factory(fake_mcp_factory())
                .with_tool_call_record_sink(sink.clone()),
        );
        let (outcome, _trace) = run_graph(AGENT_GRAPH, executor.clone(), json!("hi")).await;
        assert_eq!(
            outcome.status,
            zgraph_kernel::RunStatus::Completed,
            "{:?}",
            outcome.error
        );
        let records = sink.records();
        assert_eq!(records.len(), 1, "{records:?}");
        assert_eq!(records[0].source, crate::tools::ToolCallSource::Mcp);
        assert_eq!(records[0].tool_name, "mcp__fake__echo");
        assert_eq!(records[0].result, Some(json!("mcp-ok")));
        assert_eq!(records[0].args["token"], json!("***"));
        assert!(records[0].side_effect.is_none() && records[0].bash.is_none());
        assert!(!serde_json::to_string(&records[0].args)
            .expect("args")
            .contains("sk-mcp-secret"));
        executor.close_mcp().await;
        executor.close_provider().await;

        // Python：source=python。
        let dir = assets(agent_config("parser", &["echo"]), "role");
        let fake = Arc::new(
            FakeLlmProvider::from_script(fake_script(json!([
                {"kind": "tool_calls", "content": null, "tool_calls": [
                    {"call_id": "p1", "name": "echo",
                     "args": {"q": 2, "password": "pw-secret"}}
                ]},
                {"kind": "text", "content": "done"}
            ])))
            .expect("script"),
        );
        let python_tool = ToolDef {
            name: "echo".to_string(),
            description: "echo".to_string(),
            parameters: json!({"type": "object", "properties": {"q": {"type": "integer"}}}),
            timeout_ms: 1_000,
            source: "tools.echo".to_string(),
        };
        let runner = Arc::new(FakeToolRunner::new().tool(python_tool));
        let sink = crate::test_support::RecordingToolCallSink::new();
        let executor = Arc::new(
            HarnessExecutor::new(settings(dir.path(), &fake, &runner))
                .with_tool_call_record_sink(sink.clone()),
        );
        let (outcome, _trace) = run_graph(AGENT_GRAPH, executor.clone(), json!("hi")).await;
        assert_eq!(
            outcome.status,
            zgraph_kernel::RunStatus::Completed,
            "{:?}",
            outcome.error
        );
        let records = sink.records();
        assert_eq!(records.len(), 1, "{records:?}");
        assert_eq!(records[0].source, crate::tools::ToolCallSource::Python);
        assert_eq!(records[0].status, crate::tools::ToolCallStatus::Ok);
        assert_eq!(records[0].result.as_ref().unwrap()["echo"]["q"], json!(2));
        executor.close_provider().await;
    }

    #[tokio::test]
    async fn bash_record_reports_truncation() {
        let dir = assets(agent_config("parser", &["builtin__bash"]), "role");
        let fake = Arc::new(
            FakeLlmProvider::from_script(fake_script(json!([
                {"kind": "tool_calls", "content": null, "tool_calls": [
                    {"call_id": "b1", "name": "builtin__bash", "args": {"command": "spam"}}
                ]},
                {"kind": "text", "content": "done"}
            ])))
            .expect("script"),
        );
        let runner = Arc::new(FakeToolRunner::new());
        let (sandbox, set) = builtin_set_with_fake_bash(dir.path());
        sandbox.push_outcome(Ok(crate::test_support::bash_outcome_with_large_output()));
        let sink = crate::test_support::RecordingToolCallSink::new();
        let executor = Arc::new(
            HarnessExecutor::new(settings(dir.path(), &fake, &runner))
                .with_builtin_tools(set)
                .with_tool_call_record_sink(sink.clone()),
        );
        let (outcome, _trace) = run_graph(AGENT_GRAPH, executor.clone(), json!("hi")).await;
        assert_eq!(
            outcome.status,
            zgraph_kernel::RunStatus::Completed,
            "{:?}",
            outcome.error
        );
        let records = sink.records();
        assert_eq!(records.len(), 1);
        assert!(records[0].truncated, "bash truncation hint recorded");
        let bash = records[0].bash.as_ref().expect("bash metadata");
        assert!(bash.stdout.len() <= crate::constants::BASH_STDOUT_MAX_BYTES);
        assert!(bash.stdout.contains("…[truncated]…"));
        executor.close_provider().await;
    }
    // ------------------------------------------------------------------
    // v0.5/F3a1：计费/用量/预算（DR-018 §1–§3）
    // ------------------------------------------------------------------

    fn known_usage(prompt: u32, completion: u32) -> Value {
        json!({
            "usage": {
                "prompt_tokens": prompt,
                "completion_tokens": completion,
                "total_tokens": prompt + completion,
                "reasoning_tokens": 0
            }
        })
    }

    fn tool_call_with_usage(call_id: &str, usage: Value) -> Value {
        let mut call = json!({
            "kind": "tool_calls",
            "content": null,
            "tool_calls": [{"call_id": call_id, "name": "now", "args": {}}]
        });
        merge_usage(&mut call, usage);
        call
    }

    fn text_with_usage(content: &str, usage: Value) -> Value {
        let mut call = json!({"kind": "text", "content": content});
        merge_usage(&mut call, usage);
        call
    }

    fn merge_usage(call: &mut Value, usage: Value) {
        if let Some(usage) = usage.get("usage") {
            call["usage"] = usage.clone();
        }
    }

    fn pricing_table() -> Arc<crate::PricingTable> {
        Arc::new(
            crate::PricingTable::parse_json(
                r#"{"currency":"USD","models":{"deepseek-chat":{"input_per_mtok":1.0,"output_per_mtok":2.0}}}"#,
            )
            .expect("pricing"),
        )
    }

    #[tokio::test]
    async fn token_budget_stops_before_next_request() {
        let dir = assets(agent_config("parser", &["now"]), "role");
        let fake = Arc::new(
            FakeLlmProvider::from_script(fake_script(json!([
                tool_call_with_usage("c1", known_usage(60, 40)),
                json!({"kind": "tool_calls", "content": null,
                       "tool_calls": [{"call_id": "c2", "name": "now", "args": {}}]}),
                json!({"kind": "text", "content": "never"})
            ])))
            .expect("script"),
        );
        let runner = Arc::new(
            FakeToolRunner::new()
                .tool(tool_def("now"))
                .behavior("now", FakeToolBehavior::Return(json!({"now": "t"}))),
        );
        let executor = Arc::new(
            HarnessExecutor::new(settings(dir.path(), &fake, &runner)).with_budget(
                crate::BudgetConfig {
                    max_tokens: Some(100),
                    max_cost: None,
                },
            ),
        );
        let (outcome, _trace) = run_graph(AGENT_GRAPH, executor.clone(), json!("hi")).await;
        assert_eq!(outcome.status, RunStatus::Failed, "{:?}", outcome.error);
        assert!(
            outcome
                .error
                .as_ref()
                .is_some_and(|error| error.message.contains("budget_exceeded")),
            "{:?}",
            outcome.error
        );
        // 超限后不再发起请求：只发出第 1 次 LLM 请求。
        assert_eq!(fake.requests().len(), 1);
        let stats = executor.run_usage_stats(&outcome.run_id).expect("stats");
        assert_eq!(stats.run.llm_calls, 1);
        assert_eq!(stats.run.total_tokens, 100);
        let stop = stats.budget_stop.as_ref().expect("budget stop");
        assert_eq!(stop.kind, crate::BudgetStopKind::Tokens);
        assert_eq!(stop.used_tokens, 100);
        executor.close_provider().await;
    }

    #[tokio::test]
    async fn cost_budget_stops_with_pricing() {
        let dir = assets(agent_config("parser", &["now"]), "role");
        let fake = Arc::new(
            FakeLlmProvider::from_script(fake_script(json!([
                tool_call_with_usage("c1", known_usage(500_000, 0)),
                json!({"kind": "tool_calls", "content": null,
                       "tool_calls": [{"call_id": "c2", "name": "now", "args": {}}]}),
                json!({"kind": "text", "content": "never"})
            ])))
            .expect("script"),
        );
        let runner = Arc::new(
            FakeToolRunner::new()
                .tool(tool_def("now"))
                .behavior("now", FakeToolBehavior::Return(json!({"now": "t"}))),
        );
        let executor = Arc::new(
            HarnessExecutor::new(settings(dir.path(), &fake, &runner))
                .with_pricing(pricing_table())
                .with_required_models(vec!["deepseek-chat".to_string()])
                .with_budget(crate::BudgetConfig {
                    max_tokens: None,
                    max_cost: Some(0.5),
                }),
        );
        let (outcome, _trace) = run_graph(AGENT_GRAPH, executor.clone(), json!("hi")).await;
        assert_eq!(outcome.status, RunStatus::Failed, "{:?}", outcome.error);
        assert_eq!(fake.requests().len(), 1);
        let stats = executor.run_usage_stats(&outcome.run_id).expect("stats");
        assert_eq!(stats.run.cost_usd, Some(0.5));
        let stop = stats.budget_stop.as_ref().expect("budget stop");
        assert_eq!(stop.kind, crate::BudgetStopKind::Cost);
        assert_eq!(stop.used_cost_usd, Some(0.5));
        executor.close_provider().await;
    }

    #[tokio::test]
    async fn max_cost_without_pricing_is_config_error() {
        let dir = assets(agent_config("parser", &[]), "role");
        let fake = Arc::new(
            FakeLlmProvider::from_script(fake_script(
                json!([{"kind": "text", "content": "never"}]),
            ))
            .expect("script"),
        );
        let runner = Arc::new(FakeToolRunner::new());
        let executor = Arc::new(
            HarnessExecutor::new(settings(dir.path(), &fake, &runner)).with_budget(
                crate::BudgetConfig {
                    max_tokens: None,
                    max_cost: Some(1.0),
                },
            ),
        );
        let (outcome, _trace) = run_graph(AGENT_GRAPH, executor, json!("hi")).await;
        assert_eq!(outcome.status, RunStatus::Failed);
        let error = outcome.error.expect("error");
        assert_eq!(error.code, "HARNESS_CONFIG_LOAD");
        assert!(error.message.contains("pricing_required_for_max_cost"));
        assert!(
            fake.requests().is_empty(),
            "config error before any LLM call"
        );
    }

    #[tokio::test]
    async fn invalid_budget_values_are_config_errors_before_requests() {
        // REV-123 P2：max_tokens/max_cost 必须 > 0；0/NaN/负值 → budget_invalid。
        for budget in [
            crate::BudgetConfig {
                max_tokens: Some(0),
                max_cost: None,
            },
            crate::BudgetConfig {
                max_tokens: None,
                max_cost: Some(0.0),
            },
            crate::BudgetConfig {
                max_tokens: None,
                max_cost: Some(f64::NAN),
            },
        ] {
            let dir = assets(agent_config("parser", &[]), "role");
            let fake = Arc::new(
                FakeLlmProvider::from_script(fake_script(
                    json!([{"kind": "text", "content": "never"}]),
                ))
                .expect("script"),
            );
            let runner = Arc::new(FakeToolRunner::new());
            let executor = Arc::new(
                HarnessExecutor::new(settings(dir.path(), &fake, &runner)).with_budget(budget),
            );
            let (outcome, _trace) = run_graph(AGENT_GRAPH, executor, json!("hi")).await;
            assert_eq!(outcome.status, RunStatus::Failed, "{budget:?}");
            let error = outcome.error.expect("error");
            assert_eq!(error.code, "HARNESS_CONFIG_LOAD", "{budget:?}");
            assert!(
                error.message.contains("budget_invalid"),
                "{}",
                error.message
            );
            assert!(fake.requests().is_empty(), "{budget:?}");
        }
    }

    #[tokio::test]
    async fn max_cost_missing_agent_model_is_config_error_before_requests() {
        // REV-123 C2：pricing 缺 Agent 模型 → 加载后、首次 LLM 请求前 fail-closed。
        let dir = assets(agent_config("parser", &[]), "role");
        let fake = Arc::new(
            FakeLlmProvider::from_script(fake_script(
                json!([{"kind": "text", "content": "never"}]),
            ))
            .expect("script"),
        );
        let runner = Arc::new(FakeToolRunner::new());
        let other_model_pricing = Arc::new(
            crate::PricingTable::parse_json(
                r#"{"currency":"USD","models":{"other-model":{"input_per_mtok":1.0,"output_per_mtok":1.0}}}"#,
            )
            .expect("pricing"),
        );
        let executor = Arc::new(
            HarnessExecutor::new(settings(dir.path(), &fake, &runner))
                .with_pricing(other_model_pricing)
                .with_required_models(vec!["deepseek-chat".to_string()])
                .with_budget(crate::BudgetConfig {
                    max_tokens: None,
                    max_cost: Some(1.0),
                }),
        );
        let (outcome, _trace) = run_graph(AGENT_GRAPH, executor, json!("hi")).await;
        assert_eq!(outcome.status, RunStatus::Failed);
        let error = outcome.error.expect("error");
        assert_eq!(error.code, "HARNESS_CONFIG_LOAD");
        assert!(
            error.message.contains("pricing_missing_model"),
            "{}",
            error.message
        );
        assert!(error.message.contains("deepseek-chat"), "{}", error.message);
        assert!(
            fake.requests().is_empty(),
            "no LLM request before pricing check"
        );
    }

    fn assets_two_agent_models(second_model: &str) -> TempDir {
        let dir = assets(agent_config("parser", &[]), "role");
        let mut agent2 = agent_config("parser2", &[]);
        agent2["model"]["name"] = json!(second_model);
        write_agent_file(&dir, agent2);
        dir
    }

    #[tokio::test]
    async fn multi_model_requires_all_models_priced_before_any_request() {
        // REV-123 C2：max_cost 下 required models 中任一缺价 → 首次请求前 fail-closed。
        let dir = assets_two_agent_models("second-model");
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
                .with_pricing(pricing_table()) // 仅 deepseek-chat
                .with_required_models(vec![
                    "deepseek-chat".to_string(),
                    "second-model".to_string(),
                ])
                .with_budget(crate::BudgetConfig {
                    max_tokens: None,
                    max_cost: Some(1.0),
                }),
        );
        let (outcome, _trace) = run_graph(TWO_MODEL_GRAPH, executor, json!("hi")).await;
        assert_eq!(outcome.status, RunStatus::Failed, "{:?}", outcome.error);
        let error = outcome.error.expect("error");
        assert_eq!(error.code, "HARNESS_CONFIG_LOAD");
        assert!(
            error.message.contains("pricing_missing_model"),
            "{}",
            error.message
        );
        assert!(error.message.contains("second-model"), "{}", error.message);
        assert!(
            fake.requests().is_empty(),
            "zero LLM requests before pricing check"
        );
    }

    #[tokio::test]
    async fn max_cost_without_required_models_is_config_error_before_requests() {
        // REV-123 C2：max_cost 但未提供 required models → pricing_models_required（零请求）。
        let dir = assets(agent_config("parser", &[]), "role");
        let fake = Arc::new(
            FakeLlmProvider::from_script(fake_script(
                json!([{"kind": "text", "content": "never"}]),
            ))
            .expect("script"),
        );
        let runner = Arc::new(FakeToolRunner::new());
        let executor = Arc::new(
            HarnessExecutor::new(settings(dir.path(), &fake, &runner))
                .with_pricing(pricing_table())
                .with_budget(crate::BudgetConfig {
                    max_tokens: None,
                    max_cost: Some(1.0),
                }),
        );
        let (outcome, _trace) = run_graph(AGENT_GRAPH, executor, json!("hi")).await;
        assert_eq!(outcome.status, RunStatus::Failed);
        let error = outcome.error.expect("error");
        assert_eq!(error.code, "HARNESS_CONFIG_LOAD");
        assert!(
            error.message.contains("pricing_models_required"),
            "{}",
            error.message
        );
        assert!(fake.requests().is_empty());
    }

    #[tokio::test]
    async fn max_cost_all_models_priced_runs_normally() {
        let dir = assets_two_agent_models("second-model");
        let fake = Arc::new(
            FakeLlmProvider::from_script(fake_script(json!([
                {"kind": "text", "content": "one"},
                {"kind": "text", "content": "two"}
            ])))
            .expect("script"),
        );
        let runner = Arc::new(FakeToolRunner::new());
        let pricing = Arc::new(
            crate::PricingTable::parse_json(
                r#"{"currency":"USD","models":{"deepseek-chat":{"input_per_mtok":1.0,"output_per_mtok":2.0},"second-model":{"input_per_mtok":1.0,"output_per_mtok":2.0}}}"#,
            )
            .expect("pricing"),
        );
        let executor = Arc::new(
            HarnessExecutor::new(settings(dir.path(), &fake, &runner))
                .with_pricing(pricing)
                .with_required_models(vec![
                    "deepseek-chat".to_string(),
                    "second-model".to_string(),
                ])
                .with_budget(crate::BudgetConfig {
                    max_tokens: None,
                    max_cost: Some(1.0),
                }),
        );
        let (outcome, _trace) = run_graph(TWO_MODEL_GRAPH, executor, json!("hi")).await;
        assert_eq!(outcome.status, RunStatus::Completed, "{:?}", outcome.error);
        assert_eq!(fake.requests().len(), 2);
    }

    #[tokio::test]
    async fn required_models_not_enforced_without_max_cost() {
        // 无 max_cost：不要求 required models（默认路径不变）；提供也被忽略。
        let dir = assets(agent_config("parser", &[]), "role");
        let fake = Arc::new(
            FakeLlmProvider::from_script(fake_script(json!([{"kind": "text", "content": "ok"}])))
                .expect("script"),
        );
        let runner = Arc::new(FakeToolRunner::new());
        let executor = Arc::new(
            HarnessExecutor::new(settings(dir.path(), &fake, &runner)).with_budget(
                crate::BudgetConfig {
                    max_tokens: Some(100),
                    max_cost: None,
                },
            ),
        );
        let (outcome, _trace) = run_graph(AGENT_GRAPH, executor, json!("hi")).await;
        assert_eq!(outcome.status, RunStatus::Completed, "{:?}", outcome.error);
        assert_eq!(fake.requests().len(), 1);

        // 提供 required models 但无 max_cost：不校验/不影响默认路径。
        let dir = assets_two_agent_models("second-model");
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
                .with_required_models(vec!["missing-model".to_string()])
                .with_pricing(pricing_table()),
        );
        let (outcome, _trace) = run_graph(TWO_MODEL_GRAPH, executor, json!("hi")).await;
        assert_eq!(outcome.status, RunStatus::Completed, "{:?}", outcome.error);
    }

    #[tokio::test]
    async fn budget_accumulates_across_agent_calls_in_same_run() {
        let dir = assets(agent_config("parser", &["now"]), "role");
        let fake = Arc::new(
            FakeLlmProvider::from_script(fake_script(json!([
                // worker1：一次文本响应（usage 60）。
                text_with_usage("first", known_usage(60, 0)),
                // worker2：先 tool_call（usage 60）→ 下一轮前预算 120 >= 100 停止。
                tool_call_with_usage("c2", known_usage(60, 0)),
                json!({"kind": "text", "content": "never"})
            ])))
            .expect("script"),
        );
        let runner = Arc::new(
            FakeToolRunner::new()
                .tool(tool_def("now"))
                .behavior("now", FakeToolBehavior::Return(json!({"now": "t"}))),
        );
        let executor = Arc::new(
            HarnessExecutor::new(settings(dir.path(), &fake, &runner)).with_budget(
                crate::BudgetConfig {
                    max_tokens: Some(100),
                    max_cost: None,
                },
            ),
        );
        let (outcome, _trace) = run_graph(TWO_AGENT_GRAPH, executor.clone(), json!("hi")).await;
        assert_eq!(outcome.status, RunStatus::Failed, "{:?}", outcome.error);
        // worker1 1 次 + worker2 第 1 次 = 2；worker2 第二请求前超限不发起。
        assert_eq!(fake.requests().len(), 2);
        let stats = executor.run_usage_stats(&outcome.run_id).expect("stats");
        assert_eq!(stats.run.llm_calls, 2);
        assert_eq!(stats.run.total_tokens, 120);
        assert!(stats.by_node.len() >= 2, "two agent_call nodes aggregated");
        executor.close_provider().await;
    }

    #[tokio::test]
    async fn stats_accumulate_without_budget_and_unknown_model_warns() {
        let dir = assets(agent_config("parser", &["now"]), "role");
        let fake = Arc::new(
            FakeLlmProvider::from_script(fake_script(json!([
                tool_call_with_usage("c1", known_usage(10, 5)),
                text_with_usage("done", known_usage(20, 10))
            ])))
            .expect("script"),
        );
        let runner = Arc::new(
            FakeToolRunner::new()
                .tool(tool_def("now"))
                .behavior("now", FakeToolBehavior::Return(json!({"now": "t"}))),
        );
        // 无预算；价格表不含模型 → 费用不可用 + warning，run 仍成功。
        let unknown_pricing = Arc::new(
            crate::PricingTable::parse_json(
                r#"{"currency":"USD","models":{"other-model":{"input_per_mtok":1.0,"output_per_mtok":1.0}}}"#,
            )
            .expect("pricing"),
        );
        let executor = Arc::new(
            HarnessExecutor::new(settings(dir.path(), &fake, &runner))
                .with_pricing(unknown_pricing),
        );
        let (outcome, _trace) = run_graph(AGENT_GRAPH, executor.clone(), json!("hi")).await;
        assert_eq!(outcome.status, RunStatus::Completed, "{:?}", outcome.error);
        let stats = executor.run_usage_stats(&outcome.run_id).expect("stats");
        assert_eq!(stats.run.llm_calls, 2);
        assert_eq!(stats.run.prompt_tokens, 30);
        assert_eq!(stats.run.completion_tokens, 15);
        assert_eq!(stats.run.total_tokens, 45);
        assert_eq!(stats.run.unpriced_calls, 2);
        assert!(stats.run.cost_usd.is_none());
        assert_eq!(stats.by_node.len(), 1);
        assert_eq!(stats.by_agent.len(), 1);
        assert!(stats.budget_stop.is_none());
        assert!(
            stats
                .warnings
                .iter()
                .any(|warning| warning.contains("no pricing")),
            "{:?}",
            stats.warnings
        );
        executor.close_provider().await;
    }

    #[tokio::test]
    async fn no_budget_default_path_has_no_budget_stop() {
        let dir = assets(agent_config("parser", &[]), "role");
        let fake = Arc::new(
            FakeLlmProvider::from_script(fake_script(json!([text_with_usage(
                "hello",
                known_usage(3, 2)
            )])))
            .expect("script"),
        );
        let runner = Arc::new(FakeToolRunner::new());
        let executor = Arc::new(HarnessExecutor::new(settings(dir.path(), &fake, &runner)));
        let (outcome, _trace) = run_graph(AGENT_GRAPH, executor.clone(), json!("hi")).await;
        assert_eq!(outcome.status, RunStatus::Completed);
        let stats = executor.run_usage_stats(&outcome.run_id).expect("stats");
        assert_eq!(stats.run.total_tokens, 5);
        assert!(stats.budget_stop.is_none());
        executor.close_provider().await;
    }
}
