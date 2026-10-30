//! genai 0.7.2 transport adapter（05 §5.3.3）。
//!
//! **边界**：`genai` 的全部类型只出现在本模块内；对外只有
//! [`GenaiLlmProvider`] 与 crate 自有类型（[`crate::llm`]）。
//!
//! 映射（05 §5.3.3）：
//! - `exec_chat_stream` 的 SSE 流 → `ChatStreamEvent` 循环；
//! - `Chunk`/`ReasoningChunk` → `LlmDelta::Token/Reasoning`（1:1 交给 sink）；
//! - `ToolCallChunk` → 内部 `LlmDelta::ToolCallStarted/Args`（最终值取自 `End`）；
//! - `End` → `LlmResponse`（usage/stop_reason/tool_calls 以 captured_* 为准）；
//! - `Error::HttpError`/`webc::Error` → `LlmError{status, retry_after_ms, phase}`；
//! - `deepseek` 用 genai 原生 DeepSeek adapter（默认端点 `https://api.deepseek.com/v1/`）；
//!   `openai_compatible` 注入 `base_url` + `AuthData::Key`（adapter 生成
//!   `Authorization: Bearer`）。
//!
//! 超时：统一由 Harness 侧 `tokio::time::timeout` 兜底（05 §5.3.6），adapter 不自行
//! 分类超时；取消：`tokio::select!` + 最多 50ms 轮询（v2 无 `CancellationToken`）。

use std::collections::HashSet;
use std::time::Instant;

use futures_util::StreamExt;
use genai::adapter::AdapterKind;
use genai::chat::{
    ChatMessage, ChatOptions, ChatRequest, ChatStreamEvent, ContentPart, MessageContent,
    StopReason as GenaiStopReason, Tool as GenaiTool, ToolCall as GenaiToolCall, ToolResponse,
    Usage as GenaiUsage,
};
use genai::resolver::{AuthData, Endpoint};
use genai::webc::Error as GenaiWebcError;
use genai::{Client, ModelIden, ServiceTarget};

use crate::constants::CANCEL_POLL_INTERVAL_MS;
use crate::ctx::CancelToken;
use crate::llm::{
    LlmDelta, LlmError, LlmErrorKind, LlmMessage, LlmPhase, LlmProvider, LlmRequest, LlmResponse,
    LlmStreamSink, StopReason, ToolCall, Usage, UsageSource,
};
use crate::tools::ToolSchema;

/// DeepSeek 默认端点（与 genai 0.7.2 `DeepSeekAdapter` 的默认值一致）。
const DEEPSEEK_DEFAULT_ENDPOINT: &str = "https://api.deepseek.com/v1/";

/// genai 0.7.2 的 LLM transport 适配器（无状态；每次请求由 `LlmRequest` 决定端点/密钥）。
pub struct GenaiLlmProvider {
    client: Client,
}

impl GenaiLlmProvider {
    /// 构造（构建内部 reqwest client；失败 → 可读错误）。
    pub fn new() -> Result<Self, String> {
        Client::new()
            .map(|client| Self { client })
            .map_err(|error| format!("build genai client: {error}"))
    }
}

impl std::fmt::Debug for GenaiLlmProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GenaiLlmProvider").finish_non_exhaustive()
    }
}

#[async_trait::async_trait]
impl LlmProvider for GenaiLlmProvider {
    fn name(&self) -> &'static str {
        "genai"
    }

    async fn chat_stream(
        &self,
        req: LlmRequest,
        sink: &mut dyn LlmStreamSink,
        cancel: &CancelToken,
    ) -> Result<LlmResponse, LlmError> {
        let started = Instant::now();
        if cancel.is_cancelled() {
            return Err(cancelled_error(LlmPhase::Establish, 0));
        }
        let (target, chat_request, options) = build_genai_request(&req);
        let mut stream = match self
            .client
            .exec_chat_stream(target, chat_request, Some(&options))
            .await
        {
            Ok(response) => Box::pin(response.stream),
            Err(error) => return Err(map_genai_error(&error, LlmPhase::Establish, 0)),
        };

        let mut phase = LlmPhase::Establish;
        let mut streamed: u32 = 0;
        let mut tool_index: u32 = 0;
        let mut started_calls: HashSet<String> = HashSet::new();
        let mut cancel_wait = Box::pin(wait_cancelled(cancel));
        loop {
            let next = tokio::select! {
                biased;
                _ = &mut cancel_wait => {
                    return Err(cancelled_error(phase, streamed));
                }
                item = stream.next() => item,
            };
            let Some(event) = next else {
                return Err(LlmError::new(
                    LlmErrorKind::StreamBroken,
                    phase,
                    "stream ended without ChatStreamEvent::End",
                    streamed,
                ));
            };
            let event = match event {
                Ok(event) => event,
                Err(error) => return Err(map_genai_error(&error, phase, streamed)),
            };
            match event {
                ChatStreamEvent::Start | ChatStreamEvent::Heartbeat => {}
                ChatStreamEvent::Chunk(chunk) => {
                    phase = LlmPhase::Streaming;
                    streamed = streamed.saturating_add(1);
                    sink.on_delta(LlmDelta::Token {
                        text: chunk.content,
                    });
                }
                ChatStreamEvent::ReasoningChunk(chunk) => {
                    phase = LlmPhase::Streaming;
                    streamed = streamed.saturating_add(1);
                    sink.on_delta(LlmDelta::Reasoning {
                        text: chunk.content,
                    });
                }
                // 思维签名只参与 provider 续传，不进正文/推理增量。
                ChatStreamEvent::ThoughtSignatureChunk(_) => {}
                ChatStreamEvent::ToolCallChunk(chunk) => {
                    let call = chunk.tool_call;
                    if started_calls.insert(call.call_id.clone()) {
                        sink.on_delta(LlmDelta::ToolCallStarted {
                            index: tool_index,
                            call_id: call.call_id.clone(),
                            name: call.fn_name.clone(),
                        });
                        tool_index = tool_index.saturating_add(1);
                    }
                    sink.on_delta(LlmDelta::ToolCallArgs {
                        index: tool_index.saturating_sub(1),
                        delta: call.fn_arguments.to_string(),
                    });
                }
                ChatStreamEvent::End(end) => {
                    let latency_ms =
                        u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
                    return Ok(build_response(end, latency_ms));
                }
            }
        }
    }
}

/// 取消错误（phase 由已交付增量决定）。
fn cancelled_error(phase: LlmPhase, streamed: u32) -> LlmError {
    LlmError::new(
        LlmErrorKind::Cancelled,
        phase,
        "cancelled by caller",
        streamed,
    )
}

/// 轮询取消（v2：无 `CancellationToken`；≤ [`CANCEL_POLL_INTERVAL_MS`] 延迟）。
async fn wait_cancelled(cancel: &CancelToken) {
    loop {
        if cancel.is_cancelled() {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(CANCEL_POLL_INTERVAL_MS)).await;
    }
}

// ---------------------------------------------------------------------------
// 请求映射
// ---------------------------------------------------------------------------

/// Harness 请求 → genai（`ServiceTarget` + `ChatRequest` + `ChatOptions`）。
pub(crate) fn build_genai_request(
    request: &LlmRequest,
) -> (ServiceTarget, ChatRequest, ChatOptions) {
    let target = service_target(request);
    let chat_request = to_genai_chat_request(request);
    let options = ChatOptions::default()
        .with_temperature(f64::from(request.temperature))
        .with_max_tokens(request.max_tokens)
        .with_capture_usage(true)
        .with_capture_content(true)
        .with_capture_reasoning_content(true)
        .with_capture_tool_calls(true);
    (target, chat_request, options)
}

/// `ModelRef` + `api_key` → genai `ServiceTarget`（端点/鉴权注入，05 §5.3.3）。
fn service_target(request: &LlmRequest) -> ServiceTarget {
    let (adapter_kind, endpoint) = match request.model.provider {
        crate::llm::ProviderKind::Deepseek => (
            AdapterKind::DeepSeek,
            Endpoint::from_static(DEEPSEEK_DEFAULT_ENDPOINT),
        ),
        crate::llm::ProviderKind::OpenAiCompatible => {
            let base_url = request.model.base_url.clone().unwrap_or_default();
            // AgentConfig 禁止尾斜杠；genai 的 OpenAI adapter 用 `Url::join("chat/completions")`，
            // 需要一个以 `/` 结尾的 base url。
            let endpoint = Endpoint::from_owned(format!("{}/", base_url.trim_end_matches('/')));
            (AdapterKind::OpenAI, endpoint)
        }
    };
    ServiceTarget {
        endpoint,
        // AuthData 的 Debug 已脱敏；明文只在本模块内出现一次。
        auth: AuthData::Key(request.api_key.expose().to_string()),
        model: ModelIden::new(adapter_kind, request.model.name.clone()),
    }
}

/// 消息/工具映射（05 §5.3.3；System 走 `ChatRequest.system`）。
fn to_genai_chat_request(request: &LlmRequest) -> ChatRequest {
    let mut system: Option<String> = None;
    let mut messages: Vec<ChatMessage> = Vec::with_capacity(request.messages.len());
    for message in &request.messages {
        match message {
            LlmMessage::System { content } => {
                if system.is_none() {
                    system = Some(content.clone());
                } else {
                    // 05 §5.4.1 只允许首条 system；多出的是框架 bug，保留为普通角色
                    // 让 provider 报错而不是静默丢弃。
                    messages.push(ChatMessage::system(content.clone()));
                }
            }
            LlmMessage::User { content } => messages.push(ChatMessage::user(content.clone())),
            LlmMessage::Assistant {
                content,
                tool_calls,
            } => messages.push(to_genai_assistant(content.as_deref(), tool_calls)),
            LlmMessage::Tool {
                tool_call_id,
                name,
                content,
            } => {
                let response = ToolResponse::new(tool_call_id.clone(), content.clone())
                    .with_fn_name(name.clone());
                messages.push(ChatMessage::from(response));
            }
        }
    }
    ChatRequest {
        system,
        messages,
        tools: if request.tools.is_empty() {
            // 05 §5.6.3：空数组必须在请求体中省略 tools 字段。
            None
        } else {
            Some(request.tools.iter().map(to_genai_tool).collect())
        },
        previous_response_id: None,
        store: None,
    }
}

/// assistant 消息：正文 + tool_calls（同一条消息，顺序一致）。
fn to_genai_assistant(content: Option<&str>, tool_calls: &[ToolCall]) -> ChatMessage {
    let mut parts: Vec<ContentPart> = Vec::new();
    if let Some(text) = content {
        if !text.is_empty() {
            parts.push(ContentPart::Text(text.to_string()));
        }
    }
    for call in tool_calls {
        parts.push(ContentPart::ToolCall(to_genai_tool_call(call)));
    }
    ChatMessage::assistant(MessageContent::from_parts(parts))
}

fn to_genai_tool_call(call: &ToolCall) -> GenaiToolCall {
    GenaiToolCall {
        call_id: call.call_id.clone(),
        fn_name: call.name.clone(),
        fn_arguments: call.args.clone(),
        thought_signatures: None,
    }
}

fn to_genai_tool(tool: &ToolSchema) -> GenaiTool {
    GenaiTool::new(tool.name.clone())
        .with_description(tool.description.clone())
        .with_schema(tool.parameters.clone())
}

// ---------------------------------------------------------------------------
// 响应映射
// ---------------------------------------------------------------------------

/// `ChatStreamEvent::End` → `LlmResponse`（以 captured_* 为最终值）。
fn build_response(end: genai::chat::StreamEnd, latency_ms: u64) -> LlmResponse {
    let genai::chat::StreamEnd {
        captured_usage,
        captured_stop_reason,
        captured_content,
        captured_reasoning_content,
        captured_response_id,
    } = end;
    let mut text_parts: Vec<String> = Vec::new();
    let mut tool_calls: Vec<ToolCall> = Vec::new();
    if let Some(content) = captured_content {
        for part in content.into_parts() {
            match part {
                ContentPart::Text(text) => text_parts.push(text),
                ContentPart::ToolCall(call) => tool_calls.push(from_genai_tool_call(call)),
                _ => {}
            }
        }
    }
    let content = if text_parts.is_empty() {
        None
    } else {
        Some(text_parts.concat())
    };
    let stop_reason = captured_stop_reason.map_or_else(
        || {
            if tool_calls.is_empty() {
                StopReason::EndTurn
            } else {
                StopReason::ToolCalls
            }
        },
        map_stop_reason,
    );
    let usage = captured_usage.map_or_else(
        || Usage {
            source: UsageSource::Unavailable,
            ..Usage::zero()
        },
        map_usage,
    );
    LlmResponse {
        content,
        reasoning: captured_reasoning_content,
        tool_calls,
        stop_reason,
        usage,
        latency_ms,
        provider_request_id: captured_response_id,
        attempts: 1,
    }
}

fn from_genai_tool_call(call: GenaiToolCall) -> ToolCall {
    let args_raw = serde_json::to_string(&call.fn_arguments).unwrap_or_default();
    ToolCall {
        call_id: call.call_id,
        name: call.fn_name,
        args: call.fn_arguments,
        args_raw,
    }
}

/// genai `StopReason` → Harness `StopReason`（05 §5.9.2 字面量）。
fn map_stop_reason(reason: GenaiStopReason) -> StopReason {
    match reason {
        GenaiStopReason::Completed(_) => StopReason::EndTurn,
        GenaiStopReason::MaxTokens(_) => StopReason::MaxTokens,
        GenaiStopReason::ToolCall(_) => StopReason::ToolCalls,
        GenaiStopReason::ContentFilter(_) => StopReason::ContentFilter,
        GenaiStopReason::StopSequence(raw) => StopReason::Other { raw },
        GenaiStopReason::Other(raw) => StopReason::Other { raw },
    }
}

/// genai `Usage` → Harness `Usage`（i32 → u32 饱和；缺 reasoning 时为 None）。
fn map_usage(usage: GenaiUsage) -> Usage {
    let prompt_tokens = to_u32(usage.prompt_tokens);
    let completion_tokens = to_u32(usage.completion_tokens);
    let total_tokens = usage.total_tokens.map_or_else(
        || prompt_tokens.saturating_add(completion_tokens),
        to_u32_opt,
    );
    let reasoning_tokens = usage
        .completion_tokens_details
        .and_then(|details| details.reasoning_tokens)
        .map(to_u32_opt);
    Usage {
        prompt_tokens,
        completion_tokens,
        total_tokens,
        reasoning_tokens,
        source: UsageSource::Provider,
    }
}

fn to_u32(value: Option<i32>) -> u32 {
    value.map_or(0, to_u32_opt)
}

fn to_u32_opt(value: i32) -> u32 {
    u32::try_from(value.max(0)).unwrap_or(0)
}

// ---------------------------------------------------------------------------
// 错误映射
// ---------------------------------------------------------------------------

/// HTTP 状态 → `LlmErrorKind`（05 §5.3.5：429/5xx 可重试，其它 4xx 不可）。
pub(crate) fn classify_status(status: u16) -> LlmErrorKind {
    match status {
        429 => LlmErrorKind::RateLimited,
        500..=599 => LlmErrorKind::ServerError,
        // 408 请求超时按 Timeout（Establish 阶段可重试）。
        408 => LlmErrorKind::Timeout,
        _ => LlmErrorKind::ClientError,
    }
}

/// 解析 `retry-after` 头（秒）→ 毫秒。
pub(crate) fn parse_retry_after_secs(raw: &str) -> Option<u64> {
    raw.trim()
        .parse::<u64>()
        .ok()
        .map(|seconds| seconds.saturating_mul(1_000))
}

/// 解析 `retry-after-ms` 头（毫秒）。
pub(crate) fn parse_retry_after_ms(raw: &str) -> Option<u64> {
    raw.trim().parse::<u64>().ok()
}

/// 从响应头提取 `retry-after`（优先 `retry-after-ms`）。
macro_rules! retry_after_from_headers {
    ($headers:expr) => {{
        let headers = $headers;
        headers
            .get("retry-after-ms")
            .and_then(|value| value.to_str().ok())
            .and_then(parse_retry_after_ms)
            .or_else(|| {
                headers
                    .get("retry-after")
                    .and_then(|value| value.to_str().ok())
                    .and_then(parse_retry_after_secs)
            })
    }};
}

/// genai 错误 → `LlmError`（05 §5.3.3 表）。
pub(crate) fn map_genai_error(
    error: &genai::Error,
    phase: LlmPhase,
    streamed_tokens: u32,
) -> LlmError {
    match error {
        genai::Error::HttpError {
            status,
            headers,
            body,
            ..
        } => {
            let status_code = status.as_u16();
            LlmError::new(
                classify_status(status_code),
                phase,
                format!("HTTP {status_code}: {}", truncate(body, 512)),
                streamed_tokens,
            )
            .with_status(Some(status_code), retry_after_from_headers!(headers))
        }
        genai::Error::WebModelCall { webc_error, .. }
        | genai::Error::WebAdapterCall { webc_error, .. } => {
            map_webc_error(webc_error, phase, streamed_tokens)
        }
        genai::Error::WebStream { cause, .. } => LlmError::new(
            LlmErrorKind::StreamBroken,
            phase,
            format!("web stream error: {cause}"),
            streamed_tokens,
        ),
        genai::Error::StreamParse { serde_error, .. } => LlmError::new(
            LlmErrorKind::Decode,
            phase,
            format!("stream decode error: {serde_error}"),
            streamed_tokens,
        ),
        genai::Error::ChatResponse { body, .. } => LlmError::new(
            LlmErrorKind::StreamBroken,
            phase,
            format!("provider error event: {}", truncate(&body.to_string(), 512)),
            streamed_tokens,
        ),
        other => LlmError::new(
            if phase == LlmPhase::Streaming {
                LlmErrorKind::StreamBroken
            } else {
                LlmErrorKind::ClientError
            },
            phase,
            truncate(&other.to_string(), 1024),
            streamed_tokens,
        ),
    }
}

/// `webc::Error` → `LlmError`（建连/DNS/TLS/HTTP/解码；ISSUE-081 拆分）。
fn map_webc_error(error: &GenaiWebcError, phase: LlmPhase, streamed_tokens: u32) -> LlmError {
    match error {
        GenaiWebcError::Reqwest(reqwest_error) => {
            let status = reqwest_error.status().map(|status| status.as_u16());
            let kind = classify_reqwest(
                reqwest_error.is_timeout(),
                reqwest_error.is_connect(),
                status,
                phase,
            );
            LlmError::new(
                kind,
                phase,
                format!("reqwest error: {reqwest_error}"),
                streamed_tokens,
            )
            .with_status(status, None)
        }
        GenaiWebcError::ResponseFailedStatus {
            status,
            body,
            headers,
        } => {
            let status_code = status.as_u16();
            LlmError::new(
                classify_status(status_code),
                phase,
                format!("HTTP {status_code}: {}", truncate(body, 512)),
                streamed_tokens,
            )
            .with_status(Some(status_code), retry_after_from_headers!(headers))
        }
        GenaiWebcError::ResponseFailedNotJson { content_type, body } => LlmError::new(
            LlmErrorKind::Decode,
            phase,
            format!(
                "response content type {content_type} is not JSON: {}",
                truncate(body, 512)
            ),
            streamed_tokens,
        ),
        GenaiWebcError::ResponseFailedInvalidJson { cause, .. } => LlmError::new(
            LlmErrorKind::Decode,
            phase,
            format!("invalid JSON response: {cause}"),
            streamed_tokens,
        ),
        other => LlmError::new(
            if phase == LlmPhase::Streaming {
                LlmErrorKind::StreamBroken
            } else {
                LlmErrorKind::ClientError
            },
            phase,
            truncate(&other.to_string(), 1024),
            streamed_tokens,
        ),
    }
}

/// `reqwest` 传输错误的分类（超时/建连/HTTP 状态/流中断）。
///
/// 不直接接收 `reqwest::Error`（transitive 依赖不外露），只接收分类所需布尔/状态。
fn classify_reqwest(
    is_timeout: bool,
    is_connect: bool,
    status: Option<u16>,
    phase: LlmPhase,
) -> LlmErrorKind {
    if is_timeout {
        LlmErrorKind::Timeout
    } else if is_connect {
        LlmErrorKind::Connect
    } else if let Some(status) = status {
        classify_status(status)
    } else if phase == LlmPhase::Streaming {
        LlmErrorKind::StreamBroken
    } else {
        LlmErrorKind::Connect
    }
}

/// 诊断文本截断（不切 UTF-8 码点）。
fn truncate(text: &str, max_chars: usize) -> String {
    crate::error::summarize_chars(text, max_chars)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::{ModelRef, ProviderKind, SecretString};

    fn request(provider: ProviderKind, base_url: Option<&str>) -> LlmRequest {
        LlmRequest::new(
            ModelRef {
                provider,
                name: "deepseek-chat".to_string(),
                base_url: base_url.map(str::to_string),
            },
            vec![
                LlmMessage::System {
                    content: "sys".to_string(),
                },
                LlmMessage::User {
                    content: "{}".to_string(),
                },
            ],
            SecretString::new("test-key"),
        )
    }

    #[test]
    fn deepseek_uses_native_adapter_and_default_endpoint() {
        let (target, chat_request, _) = build_genai_request(&request(ProviderKind::Deepseek, None));
        assert_eq!(target.model.adapter_kind, AdapterKind::DeepSeek);
        assert_eq!(target.endpoint.base_url(), DEEPSEEK_DEFAULT_ENDPOINT);
        assert_eq!(target.model.model_name.as_str(), "deepseek-chat");
        assert_eq!(chat_request.system.as_deref(), Some("sys"));
        assert_eq!(chat_request.messages.len(), 1);
    }

    #[test]
    fn openai_compatible_injects_base_url_with_trailing_slash() {
        let (target, _, _) = build_genai_request(&request(
            ProviderKind::OpenAiCompatible,
            Some("https://llm.internal.example/v1"),
        ));
        assert_eq!(target.model.adapter_kind, AdapterKind::OpenAI);
        assert_eq!(
            target.endpoint.base_url(),
            "https://llm.internal.example/v1/"
        );
        // 鉴权走 AuthData::Key（Bearer 由 adapter 注入；Debug 已脱敏）。
        assert!(format!("{:?}", target.auth).contains("REDACTED"));
    }

    #[test]
    fn message_mapping_covers_all_roles() {
        let mut llm_request = request(ProviderKind::Deepseek, None);
        llm_request.messages.push(LlmMessage::Assistant {
            content: Some("partial".to_string()),
            tool_calls: vec![ToolCall {
                call_id: "call_1".to_string(),
                name: "now".to_string(),
                args: serde_json::json!({}),
                args_raw: "{}".to_string(),
            }],
        });
        llm_request.messages.push(LlmMessage::Tool {
            tool_call_id: "call_1".to_string(),
            name: "now".to_string(),
            content: "{\"ok\":true}".to_string(),
        });
        let chat_request = to_genai_chat_request(&llm_request);
        assert_eq!(chat_request.system.as_deref(), Some("sys"));
        assert_eq!(chat_request.messages.len(), 3);
        let assistant = &chat_request.messages[1];
        assert_eq!(assistant.role, genai::chat::ChatRole::Assistant);
        let texts = assistant.content.texts();
        assert_eq!(texts, vec!["partial"]);
        let tool_call_parts = assistant
            .content
            .parts()
            .iter()
            .filter_map(|part| match part {
                ContentPart::ToolCall(call) => Some(call),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(tool_call_parts.len(), 1);
        assert_eq!(tool_call_parts[0].call_id, "call_1");
        let tool_message = &chat_request.messages[2];
        assert_eq!(tool_message.role, genai::chat::ChatRole::Tool);
        assert_eq!(tool_message.content.parts().len(), 1);
    }

    #[test]
    fn tools_mapping_preserves_order_and_omits_empty_array() {
        let mut llm_request = request(ProviderKind::Deepseek, None);
        llm_request.tools = vec![
            ToolSchema {
                name: "now".to_string(),
                description: "当前时间".to_string(),
                parameters: serde_json::json!({"type": "object", "properties": {}}),
            },
            ToolSchema {
                name: "add".to_string(),
                description: "相加".to_string(),
                parameters: serde_json::json!({"type": "object", "properties": {}}),
            },
        ];
        let chat_request = to_genai_chat_request(&llm_request);
        let tools = chat_request.tools.expect("tools");
        assert_eq!(tools[0].name.as_str(), "now");
        assert_eq!(tools[1].name.as_str(), "add");
        assert_eq!(tools[0].description.as_deref(), Some("当前时间"));
        assert_eq!(
            tools[0].schema,
            Some(serde_json::json!({"type": "object", "properties": {}}))
        );

        let empty = to_genai_chat_request(&request(ProviderKind::Deepseek, None));
        assert!(empty.tools.is_none(), "empty tools must be omitted");
    }

    #[test]
    fn chat_options_enable_capture_and_sampling() {
        let mut llm_request = request(ProviderKind::Deepseek, None);
        llm_request.temperature = 0.3;
        llm_request.max_tokens = 128;
        let (_, _, options) = build_genai_request(&llm_request);
        let temperature = options.temperature.expect("temperature");
        assert!(
            (temperature - 0.3).abs() < 1e-6,
            "temperature ~0.3, got {temperature}"
        );
        assert_eq!(options.max_tokens, Some(128));
        assert_eq!(options.capture_usage, Some(true));
        assert_eq!(options.capture_content, Some(true));
        assert_eq!(options.capture_reasoning_content, Some(true));
        assert_eq!(options.capture_tool_calls, Some(true));
    }

    #[test]
    fn status_classification_matches_retry_rules() {
        assert_eq!(classify_status(429), LlmErrorKind::RateLimited);
        assert_eq!(classify_status(500), LlmErrorKind::ServerError);
        assert_eq!(classify_status(503), LlmErrorKind::ServerError);
        assert_eq!(classify_status(408), LlmErrorKind::Timeout);
        assert_eq!(classify_status(400), LlmErrorKind::ClientError);
        assert_eq!(classify_status(401), LlmErrorKind::ClientError);
    }

    #[test]
    fn retry_after_headers_are_parsed() {
        assert_eq!(parse_retry_after_secs("2"), Some(2_000));
        assert_eq!(parse_retry_after_secs(" 1 "), Some(1_000));
        assert_eq!(parse_retry_after_secs("later"), None);
        assert_eq!(parse_retry_after_ms("150"), Some(150));
        assert_eq!(parse_retry_after_ms("-1"), None);
    }

    #[test]
    fn stop_reason_mapping_is_spec_literal_compatible() {
        assert_eq!(
            map_stop_reason(GenaiStopReason::Completed("stop".to_string())),
            StopReason::EndTurn
        );
        assert_eq!(
            map_stop_reason(GenaiStopReason::ToolCall("tool_calls".to_string())),
            StopReason::ToolCalls
        );
        assert_eq!(
            map_stop_reason(GenaiStopReason::MaxTokens("length".to_string())),
            StopReason::MaxTokens
        );
        assert_eq!(
            map_stop_reason(GenaiStopReason::ContentFilter("filter".to_string())),
            StopReason::ContentFilter
        );
        assert_eq!(
            map_stop_reason(GenaiStopReason::Other("length".to_string())),
            StopReason::Other {
                raw: "length".to_string()
            }
        );
        assert_eq!(
            StopReason::Other {
                raw: "length".to_string()
            }
            .as_literal(),
            "Other:length"
        );
    }

    #[test]
    fn usage_mapping_converts_and_fills_total() {
        let usage = GenaiUsage {
            prompt_tokens: Some(120),
            completion_tokens: Some(8),
            total_tokens: None,
            prompt_tokens_details: None,
            completion_tokens_details: Some(genai::chat::CompletionTokensDetails {
                reasoning_tokens: Some(3),
                ..Default::default()
            }),
        };
        let mapped = map_usage(usage);
        assert_eq!(mapped.prompt_tokens, 120);
        assert_eq!(mapped.completion_tokens, 8);
        assert_eq!(mapped.total_tokens, 128);
        assert_eq!(mapped.reasoning_tokens, Some(3));
        assert_eq!(mapped.source, UsageSource::Provider);

        let missing = map_usage(GenaiUsage::default());
        assert_eq!(missing.total_tokens, 0);
        assert_eq!(missing.source, UsageSource::Provider);
    }

    #[test]
    fn stream_end_builds_text_response_without_tool_calls() {
        let end = genai::chat::StreamEnd {
            captured_usage: None,
            captured_stop_reason: Some(GenaiStopReason::Completed("stop".to_string())),
            captured_content: Some(MessageContent::from_parts(vec![
                ContentPart::Text("hel".to_string()),
                ContentPart::Text("lo".to_string()),
            ])),
            captured_reasoning_content: Some("think".to_string()),
            captured_response_id: Some("req-1".to_string()),
        };
        let response = build_response(end, 5);
        assert_eq!(response.content.as_deref(), Some("hello"));
        assert_eq!(response.reasoning.as_deref(), Some("think"));
        assert!(response.tool_calls.is_empty());
        assert_eq!(response.stop_reason, StopReason::EndTurn);
        assert_eq!(response.usage.source, UsageSource::Unavailable);
        assert_eq!(response.provider_request_id.as_deref(), Some("req-1"));
        assert_eq!(response.attempts, 1);
    }

    #[test]
    fn stream_end_builds_tool_call_response() {
        let end = genai::chat::StreamEnd {
            captured_usage: Some(GenaiUsage::default()),
            captured_stop_reason: Some(GenaiStopReason::ToolCall("tool_calls".to_string())),
            captured_content: Some(MessageContent::from_parts(vec![ContentPart::ToolCall(
                GenaiToolCall {
                    call_id: "call_1".to_string(),
                    fn_name: "add".to_string(),
                    fn_arguments: serde_json::json!({"a": 1, "b": 2}),
                    thought_signatures: None,
                },
            )])),
            captured_reasoning_content: None,
            captured_response_id: None,
        };
        let response = build_response(end, 1);
        assert_eq!(response.content, None);
        assert_eq!(response.tool_calls.len(), 1);
        assert_eq!(response.tool_calls[0].name, "add");
        assert_eq!(response.tool_calls[0].call_id, "call_1");
        assert_eq!(
            response.tool_calls[0].args,
            serde_json::json!({"a": 1, "b": 2})
        );
        assert_eq!(response.stop_reason, StopReason::ToolCalls);
    }

    /// 真实模型调用（手动；`DEEPSEEK_API_KEY` + `--ignored`）。
    #[tokio::test]
    #[ignore = "real model: requires DEEPSEEK_API_KEY and network"]
    async fn real_deepseek_smoke() {
        let Ok(api_key) = std::env::var("DEEPSEEK_API_KEY") else {
            eprintln!("skip: DEEPSEEK_API_KEY not set");
            return;
        };
        let provider = GenaiLlmProvider::new().expect("genai client");
        let request = LlmRequest {
            model: ModelRef {
                provider: ProviderKind::Deepseek,
                name: "deepseek-chat".to_string(),
                base_url: None,
            },
            messages: vec![LlmMessage::User {
                content: "只回复两个字：你好".to_string(),
            }],
            tools: Vec::new(),
            temperature: 0.0,
            max_tokens: 16,
            stream: true,
            api_key: SecretString::new(api_key),
            timeout_ms: 30_000,
        };
        let ctx = crate::test_support::TestCtx::new();
        let cancel = CancelToken::new(ctx.clone());
        let mut sink = crate::ctx::StreamSink::new(ctx);
        let response = tokio::time::timeout(
            std::time::Duration::from_secs(60),
            provider.chat_stream(request, &mut sink, &cancel),
        )
        .await
        .expect("not timed out")
        .expect("real LLM call");
        assert!(response
            .content
            .as_deref()
            .is_some_and(|text| !text.is_empty()));
    }
}
