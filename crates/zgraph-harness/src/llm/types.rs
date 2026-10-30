//! LLM 类型层（05 §5.3.2；字段级冻结）。
//!
//! 关键规则：
//! - [`LlmPhase`] 由 adapter 维护（“是否已向 sink 交付过任何 Token/Reasoning”），
//!   不是由 Harness 猜。
//! - [`ToolCall::args`] 必须是完整可解析的 JSON；解析失败的一方（adapter/工具层）
//!   负责把它当 `TOOL_ERROR` 喂回，绝不执行半截参数。
//! - `usage` 缺失时 `source = Unavailable`、三项填 0，**不得**用 0 冒充真实计量。
//! - [`SecretString`] 不实现 `Debug`/`Display`，防止 `api_key` 泄漏到日志/trace。

use serde::ser::{Serialize, SerializeStruct, Serializer};
use serde_json::Value;

use crate::constants::DEFAULT_LLM_MAX_TOKENS;
use crate::ctx::CancelToken;
use crate::tools::ToolSchema;

/// 模型 provider 类别（与 AgentConfig `model.provider` 一一对应，05 §5.2.2）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderKind {
    /// DeepSeek（genai 原生 adapter）。
    Deepseek,
    /// 任意 OpenAI-compatible 端点（注入 `base_url` + `Authorization: Bearer`）。
    OpenAiCompatible,
}

impl ProviderKind {
    /// 与 AgentConfig JSON 一致的字符串（`deepseek` / `openai_compatible`）。
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Deepseek => "deepseek",
            Self::OpenAiCompatible => "openai_compatible",
        }
    }

    /// 从 AgentConfig 字符串解析。
    pub fn parse(raw: &str) -> Option<Self> {
        match raw {
            "deepseek" => Some(Self::Deepseek),
            "openai_compatible" => Some(Self::OpenAiCompatible),
            _ => None,
        }
    }
}

/// 一次请求的模型引用（由 `AgentConfig.model` 派生；v1 一实例一模型）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelRef {
    /// provider 类别。
    pub provider: ProviderKind,
    /// 模型名（如 `deepseek-chat`）。
    pub name: String,
    /// OpenAI-compatible 端点（provider=deepseek 时恒 `None`）。
    pub base_url: Option<String>,
}

/// 不实现 `Debug`/`Display` 的密钥包装（05 §5.3.2）。
///
/// `Drop` 时尽力减少明文驻留时间；由于 crate `forbid(unsafe_code)`，无法保证把
/// 已分配内存逐字节清零（不做 `volatile` 写），这里只清空并释放缓冲。
pub struct SecretString(String);

impl SecretString {
    /// 包装一个密钥明文。
    pub fn new(secret: impl Into<String>) -> Self {
        Self(secret.into())
    }

    /// crate 内部读取明文（仅 adapter 注入鉴权时使用；禁止落日志/trace）。
    pub(crate) fn expose(&self) -> &str {
        &self.0
    }

    /// 明文是否为空。
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl Clone for SecretString {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl Drop for SecretString {
    fn drop(&mut self) {
        // 安全子集内无法保证零化（无 `volatile`/`as_mut_vec`），尽力释放。
        self.0.clear();
        self.0.shrink_to_fit();
    }
}

/// 一次 LLM 请求（05 §5.3.2）。
pub struct LlmRequest {
    /// 模型引用。
    pub model: ModelRef,
    /// 消息序列（system 至多一条且在首位）。
    pub messages: Vec<LlmMessage>,
    /// 工具 schema；空数组表示请求体**省略** tools 字段（05 §5.6.3）。
    pub tools: Vec<ToolSchema>,
    /// 采样温度（透传 provider）。
    pub temperature: f32,
    /// 单次响应 token 上限。
    pub max_tokens: u32,
    /// 是否流式（v1 恒 true；全量接口不存在）。
    pub stream: bool,
    /// 已解析的密钥（不落日志）。
    pub api_key: SecretString,
    /// 本次调用生效超时（`min(llm_timeout_ms, 剩余 deadline)`）。
    pub timeout_ms: u64,
}

impl Clone for LlmRequest {
    fn clone(&self) -> Self {
        Self {
            model: self.model.clone(),
            messages: self.messages.clone(),
            tools: self.tools.clone(),
            temperature: self.temperature,
            max_tokens: self.max_tokens,
            stream: self.stream,
            api_key: self.api_key.clone(),
            timeout_ms: self.timeout_ms,
        }
    }
}

impl LlmRequest {
    /// 由模型配置与消息构造一个默认（流式、无工具）的请求骨架。
    pub fn new(model: ModelRef, messages: Vec<LlmMessage>, api_key: SecretString) -> Self {
        Self {
            model,
            messages,
            tools: Vec::new(),
            temperature: crate::constants::DEFAULT_LLM_TEMPERATURE,
            max_tokens: DEFAULT_LLM_MAX_TOKENS,
            stream: true,
            api_key,
            timeout_ms: crate::constants::DEFAULT_LLM_TIMEOUT_MS,
        }
    }
}

/// 消息类型（05 §5.4.1 权威定义）。
#[derive(Debug, Clone, PartialEq)]
pub enum LlmMessage {
    /// system 消息（至多一条且在序列首位）。
    System {
        /// 文本内容。
        content: String,
    },
    /// user 消息（v1 仅首轮输入）。
    User {
        /// 文本内容。
        content: String,
    },
    /// assistant 消息（`tool_calls` 为空即普通 assistant）。
    Assistant {
        /// 正文（纯 tool_call 轮为 `None`）。
        content: Option<String>,
        /// 该轮的全部 tool_call（顺序即执行顺序）。
        tool_calls: Vec<ToolCall>,
    },
    /// 工具结果消息。
    Tool {
        /// 与上一条 `Assistant.tool_calls[i].call_id` 一一对应。
        tool_call_id: String,
        /// 工具名。
        name: String,
        /// 结果 JSON 字符串（含错误 JSON）。
        content: String,
    },
}

/// LLM 增量（05 §5.3.2 `LlmDelta`）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LlmDelta {
    /// 正文增量（1:1 → `StreamingEvent::TokenDelta`）。
    Token {
        /// 文本增量。
        text: String,
    },
    /// 推理增量（1:1 → `StreamingEvent::ReasoningDelta`）。
    Reasoning {
        /// 文本增量。
        text: String,
    },
    /// tool call 出现（仅 adapter 内部聚合观察，不直接外发）。
    ToolCallStarted {
        /// 响应内序号。
        index: u32,
        /// 调用 ID。
        call_id: String,
        /// 工具名。
        name: String,
    },
    /// 参数 JSON 增量（仅聚合，不直接外发）。
    ToolCallArgs {
        /// 响应内序号。
        index: u32,
        /// 参数 JSON 增量。
        delta: String,
    },
}

/// 一个已聚合完成的 tool call（05 §5.3.2）。
#[derive(Debug, Clone, PartialEq)]
pub struct ToolCall {
    /// 调用 ID（`ToolCallStart`/`ToolCallEnd`/`Tool.tool_call_id` 一致）。
    pub call_id: String,
    /// 工具名。
    pub name: String,
    /// 已解析的参数（必须是 JSON object；否则工具层报 `args_not_object`）。
    pub args: Value,
    /// 原始参数字符串（诊断用）。
    pub args_raw: String,
}

/// 停止原因（05 §5.3.2；序列化见 05 §5.9.2）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StopReason {
    /// 模型自然结束。
    EndTurn,
    /// 模型发起工具调用。
    ToolCalls,
    /// 达到 `max_tokens`（正常返回）。
    MaxTokens,
    /// 内容被安全过滤。
    ContentFilter,
    /// provider 特有原因。
    Other {
        /// 原始字符串。
        raw: String,
    },
}

impl StopReason {
    /// 序列化字面量（`EndTurn` / `ToolCalls` / `MaxTokens` / `ContentFilter` / `Other:<raw>`）。
    pub fn as_literal(&self) -> String {
        match self {
            Self::EndTurn => "EndTurn".to_string(),
            Self::ToolCalls => "ToolCalls".to_string(),
            Self::MaxTokens => "MaxTokens".to_string(),
            Self::ContentFilter => "ContentFilter".to_string(),
            Self::Other { raw } => format!("Other:{raw}"),
        }
    }

    /// 解析脚本/配置里的字符串（Fake provider 脚本用）。
    pub fn from_literal(raw: &str) -> Self {
        match raw {
            "EndTurn" | "end_turn" | "stop" => Self::EndTurn,
            "ToolCalls" | "tool_calls" => Self::ToolCalls,
            "MaxTokens" | "max_tokens" | "length" => Self::MaxTokens,
            "ContentFilter" | "content_filter" => Self::ContentFilter,
            other => match other.strip_prefix("Other:") {
                Some(inner) => Self::Other {
                    raw: inner.to_string(),
                },
                None => Self::Other {
                    raw: other.to_string(),
                },
            },
        }
    }
}

impl Serialize for StopReason {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.as_literal())
    }
}

/// usage 来源（05 §5.3.2；小写序列化，05 §5.9.2）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum UsageSource {
    /// provider 真实计量。
    #[default]
    Provider,
    /// 字符近似估算（Fake provider 缺省）。
    Estimated,
    /// provider 未提供。
    Unavailable,
}

impl UsageSource {
    /// 序列化字面量（小写）。
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Provider => "provider",
            Self::Estimated => "estimated",
            Self::Unavailable => "unavailable",
        }
    }
}

/// 一次响应/累计的 token 计量（05 §5.3.2）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Usage {
    /// 输入 token。
    pub prompt_tokens: u32,
    /// 输出 token。
    pub completion_tokens: u32,
    /// 总量。
    pub total_tokens: u32,
    /// reasoning token（provider 提供时）。
    pub reasoning_tokens: Option<u32>,
    /// 计量来源。
    pub source: UsageSource,
}

impl Usage {
    /// 全零 usage（`source = Unavailable` 的语义由调用方覆盖）。
    pub const fn zero() -> Self {
        Self {
            prompt_tokens: 0,
            completion_tokens: 0,
            total_tokens: 0,
            reasoning_tokens: None,
            source: UsageSource::Provider,
        }
    }

    /// 构造 provider 计量。
    pub fn provider(
        prompt_tokens: u32,
        completion_tokens: u32,
        total_tokens: Option<u32>,
        reasoning_tokens: Option<u32>,
    ) -> Self {
        let total_tokens =
            total_tokens.unwrap_or_else(|| prompt_tokens.saturating_add(completion_tokens));
        Self {
            prompt_tokens,
            completion_tokens,
            total_tokens,
            reasoning_tokens,
            source: UsageSource::Provider,
        }
    }

    /// 累计一次成功响应的 usage（05 §5.3.5：只累计成功响应）。
    pub fn accumulate(&mut self, other: &Self) {
        self.prompt_tokens = self.prompt_tokens.saturating_add(other.prompt_tokens);
        self.completion_tokens = self
            .completion_tokens
            .saturating_add(other.completion_tokens);
        self.total_tokens = self.total_tokens.saturating_add(other.total_tokens);
        self.reasoning_tokens = match (self.reasoning_tokens, other.reasoning_tokens) {
            (None, None) => None,
            (Some(a), None) => Some(a),
            (None, Some(b)) => Some(b),
            (Some(a), Some(b)) => Some(a.saturating_add(b)),
        };
        // 来源降级语义：任一为 Unavailable/Estimated 时保守记录（provider 优先）。
        self.source = match (self.source, other.source) {
            (UsageSource::Provider, UsageSource::Provider) => UsageSource::Provider,
            (UsageSource::Unavailable, _) | (_, UsageSource::Unavailable) => {
                UsageSource::Unavailable
            }
            _ => UsageSource::Estimated,
        };
    }

    /// 05 §5.9.2 的 usage JSON 形状（数值不可为 null）。
    pub fn to_json(&self) -> Value {
        serde_json::json!({
            "prompt_tokens": self.prompt_tokens,
            "completion_tokens": self.completion_tokens,
            "total_tokens": self.total_tokens,
            "reasoning_tokens": self.reasoning_tokens,
            "source": self.source.as_str(),
        })
    }
}

impl Serialize for Usage {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut state = serializer.serialize_struct("Usage", 5)?;
        state.serialize_field("prompt_tokens", &self.prompt_tokens)?;
        state.serialize_field("completion_tokens", &self.completion_tokens)?;
        state.serialize_field("total_tokens", &self.total_tokens)?;
        state.serialize_field("reasoning_tokens", &self.reasoning_tokens)?;
        state.serialize_field("source", self.source.as_str())?;
        state.end()
    }
}

/// 一次流式调用的最终聚合结果（05 §5.3.2）。
#[derive(Debug, Clone, PartialEq)]
pub struct LlmResponse {
    /// 正文（无正文 → None；多 tool_call 首轮常见）。
    pub content: Option<String>,
    /// 完整 reasoning（provider 提供时）。
    pub reasoning: Option<String>,
    /// 已按 index 聚合、args 已解析的 tool_calls。
    pub tool_calls: Vec<ToolCall>,
    /// 停止原因。
    pub stop_reason: StopReason,
    /// 本次请求计量。
    pub usage: Usage,
    /// 延迟（毫秒）。
    pub latency_ms: u64,
    /// provider request id（可缺省）。
    pub provider_request_id: Option<String>,
    /// 含首次的尝试次数（Harness 重试后覆盖为累计值）。
    pub attempts: u32,
}

impl LlmResponse {
    /// 构造一个只有文本的响应（测试/脚本用）。
    pub fn text(content: impl Into<String>) -> Self {
        Self {
            content: Some(content.into()),
            reasoning: None,
            tool_calls: Vec::new(),
            stop_reason: StopReason::EndTurn,
            usage: Usage::zero(),
            latency_ms: 0,
            provider_request_id: None,
            attempts: 1,
        }
    }
}

/// LLM 错误分类（05 §5.3.2；retry 决策所需信息全在这里）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LlmErrorKind {
    /// 建连失败（DNS/TCP/TLS）。
    Connect,
    /// 超时（未吐 delta 的建连/首包超时，或流中途超时）。
    Timeout,
    /// HTTP 429。
    RateLimited,
    /// HTTP 5xx。
    ServerError,
    /// 其它 4xx（配置/鉴权问题，不可重试）。
    ClientError,
    /// 流中断。
    StreamBroken,
    /// JSON/SSE 解码失败。
    Decode,
    /// 已取消。
    Cancelled,
}

impl LlmErrorKind {
    /// 稳定的小写字符串（detail/trace 诊断用）。
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Connect => "connect",
            Self::Timeout => "timeout",
            Self::RateLimited => "rate_limited",
            Self::ServerError => "server_error",
            Self::ClientError => "client_error",
            Self::StreamBroken => "stream_broken",
            Self::Decode => "decode",
            Self::Cancelled => "cancelled",
        }
    }
}

/// 错误发生阶段（05 §5.3.2：adapter 维护）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LlmPhase {
    /// 尚未向 sink 交付任何 Token/Reasoning。
    Establish,
    /// 已交付过增量。
    Streaming,
}

impl LlmPhase {
    /// 稳定的小写字符串。
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Establish => "establish",
            Self::Streaming => "streaming",
        }
    }
}

/// LLM transport 错误（05 §5.3.2）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LlmError {
    /// 错误分类。
    pub kind: LlmErrorKind,
    /// 发生阶段。
    pub phase: LlmPhase,
    /// HTTP 状态码（有则填）。
    pub status: Option<u16>,
    /// provider 建议的重试等待（`retry-after`，毫秒）。
    pub retry_after_ms: Option<u64>,
    /// 人类可读信息（不含密钥）。
    pub message: String,
    /// 已成功 emit 的正文 token 数（>0 即视为已开始输出）。
    pub streamed_tokens: u32,
}

impl LlmError {
    /// 构造一个错误。
    pub fn new(
        kind: LlmErrorKind,
        phase: LlmPhase,
        message: impl Into<String>,
        streamed_tokens: u32,
    ) -> Self {
        Self {
            kind,
            phase,
            status: None,
            retry_after_ms: None,
            message: message.into(),
            streamed_tokens,
        }
    }

    /// 填充 HTTP 状态与 `retry-after`。
    pub fn with_status(mut self, status: Option<u16>, retry_after_ms: Option<u64>) -> Self {
        self.status = status;
        self.retry_after_ms = retry_after_ms;
        self
    }

    /// 05 §5.3.5：只有“建立阶段”的 Connect/Timeout/429/5xx 可重试。
    pub fn is_retryable(&self) -> bool {
        self.phase == LlmPhase::Establish
            && matches!(
                self.kind,
                LlmErrorKind::Connect
                    | LlmErrorKind::Timeout
                    | LlmErrorKind::RateLimited
                    | LlmErrorKind::ServerError
            )
    }

    /// 是否取消（不分阶段）。
    pub fn is_cancelled(&self) -> bool {
        self.kind == LlmErrorKind::Cancelled
    }
}

/// LLM transport 抽象：一次调用 = 一次完整请求（含流式增量回调与最终聚合结果）。
///
/// 本 trait 是 Harness 与 transport 之间唯一耦合点；不暴露任何 genai 类型。
/// **重试编排在 Harness 内部**（05 §5.3.5），adapter 只如实报告 [`LlmError`]。
#[async_trait::async_trait]
pub trait LlmProvider: Send + Sync {
    /// provider 名（`"genai-deepseek"` / `"genai-openai-compatible"` / `"fake"`）。
    fn name(&self) -> &'static str;

    /// 发起一次流式请求。增量通过 sink 同步推送（非阻塞）；返回聚合后的最终结果。
    ///
    /// 取消：cancel 被触发时必须尽快 drop 底层请求并返回
    /// `Err(LlmError { kind: Cancelled, .. })`。
    async fn chat_stream(
        &self,
        req: LlmRequest,
        sink: &mut dyn LlmStreamSink,
        cancel: &CancelToken,
    ) -> Result<LlmResponse, LlmError>;
}

/// 增量接收端：实现方只做「1:1 转发到 `ctx.emit`」，不得阻塞、不得 await、不得做 IO。
pub trait LlmStreamSink: Send {
    /// 接收一个增量（05 §5.8：Harness 不做时间窗聚合）。
    fn on_delta(&mut self, delta: LlmDelta);
}
