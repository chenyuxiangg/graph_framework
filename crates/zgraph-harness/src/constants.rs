//! 默认常量（00 §0.5 + 05 §5.11.1）。
//!
//! 00 §0.5 是本表最高权威；05 §5.11.1 的 7 个“待补录常量”已按 M4 口径回写 00。

/// 单次 LLM 调用默认超时（毫秒；`DEFAULT_LLM_TIMEOUT_MS`，05 §5.2.2）。
pub const DEFAULT_LLM_TIMEOUT_MS: u64 = 60_000;

/// LLM 默认重试次数（`DEFAULT_LLM_MAX_RETRIES`；总请求数 = 1 + 该值）。
pub const DEFAULT_LLM_MAX_RETRIES: u32 = 1;

/// tool 轮次（LLM 请求轮次）默认上限（`DEFAULT_MAX_ITERATIONS`）。
pub const DEFAULT_MAX_ITERATIONS: u32 = 10;

/// `agent_call` 实例默认超时（毫秒；`DEFAULT_NODE_TIMEOUT_MS`，仅作 DSL 缺省回退）。
pub const DEFAULT_NODE_TIMEOUT_MS: u64 = 180_000;

/// 单次工具调用默认超时（毫秒；`DEFAULT_TOOL_TIMEOUT_MS`）。
pub const DEFAULT_TOOL_TIMEOUT_MS: u64 = 30_000;

/// 单次响应默认 token 上限（`DEFAULT_LLM_MAX_TOKENS`）。
pub const DEFAULT_LLM_MAX_TOKENS: u32 = 2_048;

/// 默认采样温度（`DEFAULT_LLM_TEMPERATURE`）。
pub const DEFAULT_LLM_TEMPERATURE: f32 = 0.2;

/// 默认模型上下文窗口（`DEFAULT_CONTEXT_WINDOW_TOKENS`）。
pub const DEFAULT_CONTEXT_WINDOW_TOKENS: u32 = 65_536;

/// prompt 预算安全系数（`CONTEXT_BUDGET_SAFETY_RATIO`，05 §5.4.4）。
pub const CONTEXT_BUDGET_SAFETY_RATIO: f64 = 0.8;

/// 单条工具结果进上下文的最大字节数（`DEFAULT_TOOL_RESULT_MAX_BYTES`，05 §5.6.5）。
pub const TOOL_RESULT_MAX_BYTES: usize = 8_192;

/// 尊重 provider `retry-after` 的上限（毫秒；`RETRY_AFTER_MAX_MS`）。
pub const RETRY_AFTER_MAX_MS: u64 = 30_000;

/// 指数退避基数（毫秒；`BACKOFF_BASE_MS`）。
pub const BACKOFF_BASE_MS: u64 = 1_000;

/// 指数退避上限（毫秒；05 §5.3.5 的 8s 上限）。
pub const BACKOFF_MAX_MS: u64 = 8_000;

/// 单条响应允许的最大 tool_call 数（超过视为 provider 异常，05 §5.5.3）。
pub const MAX_TOOL_CALLS_PER_RESPONSE: usize = 32;

/// `AgentConfig.tools` 允许的最大条目数（V14，05 §5.2.2）。
pub const MAX_TOOLS_PER_AGENT: usize = 32;

/// AgentConfig 文件字节上限（256 KiB，05 §5.2.1）。
pub const MAX_AGENT_CONFIG_BYTES: u64 = 256 * 1024;

/// role_prompt / constraints 文件字节上限（1 MiB，05 §5.2.1）。
pub const MAX_PROMPT_BYTES: u64 = 1024 * 1024;

/// 工具描述字符上限（1..=1024，05 §5.6.1）。
pub const MAX_TOOL_DESCRIPTION_CHARS: usize = 1_024;

/// `args_summary`/`result_summary` 的 preview 字符上限（05 §5.8.5）。
pub const MAX_SUMMARY_PREVIEW_CHARS: usize = 256;

/// preview 结构摘要的最大嵌套深度（超出降级为类型/尺寸标记；ISSUE-075）。
pub const MAX_SUMMARY_PREVIEW_DEPTH: usize = 2;

/// preview 结构摘要中每个数组最多展开的元素数（其余折叠为计数；ISSUE-075）。
pub const MAX_SUMMARY_PREVIEW_ARRAY_ITEMS: usize = 4;

/// 敏感键在 preview 中的脱敏占位符（05 §5.8.5；ISSUE-075）。
pub const REDACTED_PREVIEW_VALUE: &str = "***";

/// 敏感键词表（键名归一化后包含任一词即脱敏；05 §5.8.5 / ISSUE-075）。
///
/// 归一化 = 小写 + 去掉非字母数字（`_`/`-`/`.`/空格等），因此
/// `api_key`、`API-KEY`、`clientSecret` 均命中。
pub const SENSITIVE_KEY_TOKENS: &[&str] = &[
    "apikey",
    "authorization",
    "cookie",
    "credential",
    "password",
    "passwd",
    "privatekey",
    "secret",
    "session",
    "token",
];

/// `HARNESS_MAX_ITERATIONS.detail.last_content_summary` 字符上限（05 §5.7.4）。
pub const MAX_LAST_CONTENT_SUMMARY_CHARS: usize = 512;

/// 取消轮询间隔（毫秒；v2 无 `CancellationToken`，见 crate 文档）。
pub const CANCEL_POLL_INTERVAL_MS: u64 = 50;

/// 支持的 AgentConfig `schema_version`（05 §5.2.2）。
pub const SUPPORTED_SCHEMA_VERSION: u64 = 1;

/// 调用器默认解释器（可被 `ZGRAPH_PYTHON` 覆盖；与 M3 e2e 约定一致）。
pub const DEFAULT_PYTHON_PROGRAM: &str = "python3";

/// worker 协议入口（`python -m <该模块>`，协议 v1 不变）。
pub const WORKER_RUNTIME_MODULE: &str = "zgraph_worker.runtime";

/// 工具注册表 Bridge 的固定 Python module（task-3 / 05 §5.6.2）。
pub const TOOL_RUNNER_MODULE: &str = "zgraph_worker.tool_runner";
