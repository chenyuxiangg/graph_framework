//! `FakeLlmProvider`：测试与 `--fake-llm` 的脚本化 provider（05 §5.3.4）。
//!
//! **MUST**（05 §5.3.4）：① 通过真实 [`LlmProvider`] trait 实现（不绕过 Harness 编排）；
//! ② 失败注入走同一 [`LlmError`] 路径（不 panic）；③ usage 缺省时
//! `source = Estimated`，`total_tokens = prompt + completion`。
//!
//! 脚本 schema（05 §5.3.4，`version = 1`）：
//!
//! ```json
//! {
//!   "version": 1,
//!   "chunk_chars": 3,
//!   "chunk_delay_ms": 0,
//!   "calls": [
//!     {"kind": "text", "content": "hi", "stop_reason": "EndTurn",
//!      "usage": {"prompt_tokens": 10, "completion_tokens": 2}},
//!     {"kind": "tool_calls", "content": null,
//!      "tool_calls": [{"call_id": "call_1", "name": "now", "args": {}}]}
//!   ],
//!   "failures": [
//!     {"call_index": 0, "attempt": 0, "phase": "establish",
//!      "kind": "RateLimited", "status": 429, "retry_after_ms": 100}
//!   ]
//! }
//! ```
//!
//! 扩展（只增不改）：`calls[].reasoning` 用于验证 `ReasoningDelta` 映射。

use std::path::Path;
use std::sync::Mutex;
use std::time::Instant;

use serde_json::{Map, Value};
use tokio::time::{sleep, Duration};

use crate::budget::{estimate_messages_tokens, estimate_tokens};
use crate::ctx::CancelToken;
use crate::llm::{
    LlmDelta, LlmError, LlmErrorKind, LlmPhase, LlmProvider, LlmRequest, LlmResponse,
    LlmStreamSink, StopReason, ToolCall, Usage, UsageSource,
};

/// 默认正文按 3 字符切分（05 §5.3.4）。
const DEFAULT_CHUNK_CHARS: usize = 3;

/// 解析后的单次脚本响应。
#[derive(Debug, Clone)]
struct FakeCall {
    content: Option<String>,
    reasoning: Option<String>,
    tool_calls: Vec<ToolCall>,
    stop_reason: StopReason,
    usage: Option<ScriptUsage>,
}

/// 脚本里的 usage 覆盖值。
#[derive(Debug, Clone, Copy, Default)]
struct ScriptUsage {
    prompt_tokens: Option<u32>,
    completion_tokens: Option<u32>,
    reasoning_tokens: Option<u32>,
}

/// 失败注入项。
#[derive(Debug, Clone)]
struct FakeFailure {
    call_index: usize,
    attempt: u32,
    phase: LlmPhase,
    kind: LlmErrorKind,
    status: Option<u16>,
    retry_after_ms: Option<u64>,
    message: String,
}

/// 解析后的脚本。
#[derive(Debug, Clone)]
struct FakeScript {
    chunk_chars: usize,
    chunk_delay_ms: u64,
    calls: Vec<FakeCall>,
    failures: Vec<FakeFailure>,
}

/// 消费进度：当前 call 与当前 call 内的 attempt（0 起）。
#[derive(Debug, Default)]
struct FakeState {
    call_index: usize,
    attempt: u32,
}

/// 脚本化 provider（测试/AC-08；无网络）。
pub struct FakeLlmProvider {
    script: FakeScript,
    state: Mutex<FakeState>,
    requests: Mutex<Vec<LlmRequest>>,
}

impl FakeLlmProvider {
    /// 由已解析的 JSON 脚本构造。
    pub fn from_script(script: Value) -> Result<Self, String> {
        let script = parse_script(&script)?;
        Ok(Self {
            script,
            state: Mutex::new(FakeState::default()),
            requests: Mutex::new(Vec::new()),
        })
    }

    /// 由脚本文件路径构造（UTF-8 JSON）。
    pub fn from_path(path: &Path) -> Result<Self, String> {
        let bytes = std::fs::read(path)
            .map_err(|error| format!("read fake-llm script {}: {error}", path.display()))?;
        let text = String::from_utf8(bytes)
            .map_err(|error| format!("fake-llm script is not UTF-8: {error}"))?;
        let value: Value = serde_json::from_str(&text)
            .map_err(|error| format!("parse fake-llm script {}: {error}", path.display()))?;
        Self::from_script(value)
    }

    /// 已消费到的 call 序号（从 0 起；用尽后停在最后一个）。
    pub fn call_index(&self) -> usize {
        self.lock_state().call_index
    }

    /// 收到的请求副本（按调用顺序；测试断言消息/工具映射用）。
    pub fn requests(&self) -> Vec<LlmRequest> {
        self.lock_requests().clone()
    }

    fn lock_state(&self) -> std::sync::MutexGuard<'_, FakeState> {
        match self.state.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        }
    }

    fn lock_requests(&self) -> std::sync::MutexGuard<'_, Vec<LlmRequest>> {
        match self.requests.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        }
    }

    /// 选择本次调用的脚本项与失败注入项，并推进消费进度。
    fn plan(&self, request: &LlmRequest) -> (FakeCall, Option<FakeFailure>, u32) {
        self.lock_requests().push(request.clone());
        let mut state = self.lock_state();
        let call_index = state.call_index;
        let attempt = state.attempt;
        let call = self.script.calls[call_index.min(self.script.calls.len() - 1)].clone();
        let failure = self
            .script
            .failures
            .iter()
            .find(|failure| failure.call_index == call_index && failure.attempt == attempt)
            .cloned();
        if failure.is_some() {
            state.attempt = state.attempt.saturating_add(1);
        } else {
            state.call_index = state.call_index.saturating_add(1);
            state.attempt = 0;
        }
        (call, failure, attempt)
    }
}

#[async_trait::async_trait]
impl LlmProvider for FakeLlmProvider {
    fn name(&self) -> &'static str {
        "fake"
    }

    async fn chat_stream(
        &self,
        req: LlmRequest,
        sink: &mut dyn LlmStreamSink,
        cancel: &CancelToken,
    ) -> Result<LlmResponse, LlmError> {
        let started = Instant::now();
        if cancel.is_cancelled() {
            return Err(cancelled_error(0));
        }
        let (call, failure, attempt) = self.plan(&req);
        if let Some(failure) = failure {
            if failure.phase == LlmPhase::Establish {
                return Err(
                    LlmError::new(failure.kind, LlmPhase::Establish, failure.message, 0)
                        .with_status(failure.status, failure.retry_after_ms),
                );
            }
            // streaming 失败：先交付一个正文增量，确保 `streamed_tokens > 0`。
            let mut streamed = 0u32;
            if let Some(text) = call.content.as_deref() {
                if let Some(chunk) = chunks(text, self.chunk_chars()).into_iter().next() {
                    sink.on_delta(LlmDelta::Token { text: chunk });
                    streamed = 1;
                }
            }
            return Err(LlmError::new(
                failure.kind,
                LlmPhase::Streaming,
                failure.message,
                streamed,
            )
            .with_status(failure.status, failure.retry_after_ms));
        }

        let mut streamed = 0u32;
        if let Some(reasoning) = call.reasoning.as_deref() {
            for chunk in chunks(reasoning, self.chunk_chars()) {
                self.pause(cancel, &mut streamed).await?;
                sink.on_delta(LlmDelta::Reasoning { text: chunk });
                streamed = streamed.saturating_add(1);
            }
        }
        if let Some(content) = call.content.as_deref() {
            for chunk in chunks(content, self.chunk_chars()) {
                self.pause(cancel, &mut streamed).await?;
                sink.on_delta(LlmDelta::Token { text: chunk });
                streamed = streamed.saturating_add(1);
            }
        }
        for (index, tool_call) in call.tool_calls.iter().enumerate() {
            let index = u32::try_from(index).unwrap_or(u32::MAX);
            self.pause(cancel, &mut streamed).await?;
            sink.on_delta(LlmDelta::ToolCallStarted {
                index,
                call_id: tool_call.call_id.clone(),
                name: tool_call.name.clone(),
            });
            for delta in chunks(&tool_call.args_raw, self.chunk_chars()) {
                sink.on_delta(LlmDelta::ToolCallArgs { index, delta });
            }
        }

        let usage = usage_for(&req, &call);
        Ok(LlmResponse {
            content: call.content.clone(),
            reasoning: call.reasoning.clone(),
            tool_calls: call.tool_calls.clone(),
            stop_reason: call.stop_reason.clone(),
            usage,
            latency_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
            provider_request_id: Some(format!("fake-{attempt}")),
            attempts: attempt.saturating_add(1),
        })
    }
}

impl FakeLlmProvider {
    fn chunk_chars(&self) -> usize {
        self.script.chunk_chars
    }

    /// 每个 delta 之间的可选延迟；取消在 delta 边界检查（v2 轮询语义）。
    async fn pause(&self, cancel: &CancelToken, streamed: &mut u32) -> Result<(), LlmError> {
        if cancel.is_cancelled() {
            return Err(cancelled_error(*streamed));
        }
        if self.script.chunk_delay_ms > 0 {
            sleep(Duration::from_millis(self.script.chunk_delay_ms)).await;
            if cancel.is_cancelled() {
                return Err(cancelled_error(*streamed));
            }
        }
        Ok(())
    }
}

fn cancelled_error(streamed: u32) -> LlmError {
    LlmError::new(
        LlmErrorKind::Cancelled,
        if streamed > 0 {
            LlmPhase::Streaming
        } else {
            LlmPhase::Establish
        },
        "cancelled by caller",
        streamed,
    )
}

/// 按字符（非字节）切分文本。
fn chunks(text: &str, chunk_chars: usize) -> Vec<String> {
    let size = chunk_chars.max(1);
    let chars: Vec<char> = text.chars().collect();
    chars
        .chunks(size)
        .map(|chunk| chunk.iter().collect())
        .collect()
}

/// usage 归集：脚本显式值 → Provider；缺省 → Estimated（05 §5.3.4 MUST ③）。
fn usage_for(request: &LlmRequest, call: &FakeCall) -> Usage {
    match call.usage {
        Some(script) => Usage::provider(
            script.prompt_tokens.unwrap_or(0),
            script.completion_tokens.unwrap_or(0),
            None,
            script.reasoning_tokens,
        ),
        None => {
            let prompt = estimate_messages_tokens(&request.messages);
            let completion = call
                .content
                .as_deref()
                .map_or(0, estimate_tokens)
                .saturating_add(call.tool_calls.iter().fold(0u32, |total, call| {
                    total.saturating_add(estimate_tokens(&call.args_raw))
                }));
            Usage {
                prompt_tokens: prompt,
                completion_tokens: completion,
                total_tokens: prompt.saturating_add(completion),
                reasoning_tokens: call.reasoning.as_deref().map(estimate_tokens),
                source: UsageSource::Estimated,
            }
        }
    }
}

// ---------------------------------------------------------------------------
// 脚本解析
// ---------------------------------------------------------------------------

fn parse_script(script: &Value) -> Result<FakeScript, String> {
    let object = script
        .as_object()
        .ok_or_else(|| "fake-llm script must be a JSON object".to_string())?;
    if let Some(version) = object.get("version") {
        let version = version
            .as_u64()
            .ok_or_else(|| "fake-llm script.version must be an integer".to_string())?;
        if version != 1 {
            return Err(format!("unsupported fake-llm script version {version}"));
        }
    }
    let chunk_chars = optional_u64(object.get("chunk_chars"), "chunk_chars")?
        .map_or(DEFAULT_CHUNK_CHARS, |value| value as usize)
        .max(1);
    let chunk_delay_ms = optional_u64(object.get("chunk_delay_ms"), "chunk_delay_ms")?.unwrap_or(0);
    let calls_value = object
        .get("calls")
        .ok_or_else(|| "fake-llm script.calls is required".to_string())?;
    let calls_array = calls_value
        .as_array()
        .ok_or_else(|| "fake-llm script.calls must be an array".to_string())?;
    if calls_array.is_empty() {
        return Err("fake-llm script.calls must not be empty".to_string());
    }
    let mut calls = Vec::with_capacity(calls_array.len());
    for (index, value) in calls_array.iter().enumerate() {
        calls.push(parse_call(value, index)?);
    }
    let mut failures = Vec::new();
    if let Some(value) = object.get("failures") {
        let array = value
            .as_array()
            .ok_or_else(|| "fake-llm script.failures must be an array".to_string())?;
        for (index, value) in array.iter().enumerate() {
            failures.push(parse_failure(value, index)?);
        }
    }
    Ok(FakeScript {
        chunk_chars,
        chunk_delay_ms,
        calls,
        failures,
    })
}

fn parse_call(value: &Value, index: usize) -> Result<FakeCall, String> {
    let object = value
        .as_object()
        .ok_or_else(|| format!("calls[{index}] must be an object"))?;
    let kind = object
        .get("kind")
        .and_then(Value::as_str)
        .ok_or_else(|| format!("calls[{index}].kind is required"))?;
    let content = parse_call_content(object, index, kind)?;
    let stop_reason = parse_call_stop_reason(object, index, kind)?;
    let reasoning = parse_call_reasoning(object, index)?;
    let mut tool_calls = Vec::new();
    if let Some(value) = object.get("tool_calls") {
        let array = value
            .as_array()
            .ok_or_else(|| format!("calls[{index}].tool_calls must be an array"))?;
        for (position, value) in array.iter().enumerate() {
            tool_calls.push(parse_tool_call(value, index, position)?);
        }
    }
    let usage = parse_call_usage(object, index)?;
    Ok(FakeCall {
        content,
        reasoning,
        tool_calls,
        stop_reason,
        usage,
    })
}

/// 解析并校验 `calls[i].content`（`kind=text` 必填）。
fn parse_call_content(
    object: &Map<String, Value>,
    index: usize,
    kind: &str,
) -> Result<Option<String>, String> {
    let content = match object.get("content") {
        None | Some(Value::Null) => None,
        Some(Value::String(text)) => Some(text.clone()),
        Some(other) => {
            return Err(format!("calls[{index}].content must be a string: {other}"));
        }
    };
    if kind == "text" && content.is_none() {
        return Err(format!("calls[{index}] kind=text requires content"));
    }
    if kind != "text" && kind != "tool_calls" {
        return Err(format!("calls[{index}].kind must be text|tool_calls"));
    }
    Ok(content)
}

/// 解析 `calls[i].stop_reason`（缺省按 kind 推导）。
fn parse_call_stop_reason(
    object: &Map<String, Value>,
    index: usize,
    kind: &str,
) -> Result<StopReason, String> {
    match object.get("stop_reason") {
        Some(Value::String(raw)) => Ok(StopReason::from_literal(raw.as_str())),
        None => Ok(if kind == "tool_calls" {
            StopReason::ToolCalls
        } else {
            StopReason::EndTurn
        }),
        Some(other) => Err(format!(
            "calls[{index}].stop_reason must be a string: {other}"
        )),
    }
}

/// 解析可选的 `calls[i].reasoning`（M4 扩展字段）。
fn parse_call_reasoning(
    object: &Map<String, Value>,
    index: usize,
) -> Result<Option<String>, String> {
    match object.get("reasoning") {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(text)) => Ok(Some(text.clone())),
        Some(other) => Err(format!(
            "calls[{index}].reasoning must be a string: {other}"
        )),
    }
}

/// 解析可选的 `calls[i].usage`。
fn parse_call_usage(
    object: &Map<String, Value>,
    index: usize,
) -> Result<Option<ScriptUsage>, String> {
    match object.get("usage") {
        None | Some(Value::Null) => Ok(None),
        Some(value) => Ok(Some(parse_usage(value, index)?)),
    }
}

fn parse_tool_call(value: &Value, call_index: usize, position: usize) -> Result<ToolCall, String> {
    let object = value
        .as_object()
        .ok_or_else(|| format!("calls[{call_index}].tool_calls[{position}] must be an object"))?;
    let call_id = object
        .get("call_id")
        .and_then(Value::as_str)
        .ok_or_else(|| format!("calls[{call_index}].tool_calls[{position}].call_id is required"))?;
    let name = object
        .get("name")
        .and_then(Value::as_str)
        .ok_or_else(|| format!("calls[{call_index}].tool_calls[{position}].name is required"))?;
    let args = object
        .get("args")
        .cloned()
        .ok_or_else(|| format!("calls[{call_index}].tool_calls[{position}].args is required"))?;
    let args_raw = serde_json::to_string(&args).unwrap_or_else(|_| "null".to_string());
    Ok(ToolCall {
        call_id: call_id.to_string(),
        name: name.to_string(),
        args,
        args_raw,
    })
}

fn parse_usage(value: &Value, index: usize) -> Result<ScriptUsage, String> {
    let object = value
        .as_object()
        .ok_or_else(|| format!("calls[{index}].usage must be an object"))?;
    Ok(ScriptUsage {
        prompt_tokens: optional_u32(object.get("prompt_tokens"), "prompt_tokens")?,
        completion_tokens: optional_u32(object.get("completion_tokens"), "completion_tokens")?,
        reasoning_tokens: optional_u32(object.get("reasoning_tokens"), "reasoning_tokens")?,
    })
}

fn parse_failure(value: &Value, index: usize) -> Result<FakeFailure, String> {
    let object = value
        .as_object()
        .ok_or_else(|| format!("failures[{index}] must be an object"))?;
    let call_index = optional_u64(object.get("call_index"), "call_index")?
        .ok_or_else(|| format!("failures[{index}].call_index is required"))?;
    let attempt = optional_u64(object.get("attempt"), "attempt")?.unwrap_or(0);
    let phase = match object.get("phase").and_then(Value::as_str) {
        Some("establish") => LlmPhase::Establish,
        Some("streaming") => LlmPhase::Streaming,
        _ => {
            return Err(format!(
                "failures[{index}].phase must be establish|streaming"
            ))
        }
    };
    let kind_raw = object
        .get("kind")
        .and_then(Value::as_str)
        .ok_or_else(|| format!("failures[{index}].kind is required"))?;
    let kind = parse_error_kind(kind_raw)
        .ok_or_else(|| format!("failures[{index}].kind '{kind_raw}' is not a LlmErrorKind"))?;
    let status = optional_u64(object.get("status"), "status")?
        .map(|value| u16::try_from(value).unwrap_or(u16::MAX));
    let retry_after_ms = optional_u64(object.get("retry_after_ms"), "retry_after_ms")?;
    let message = object
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or("injected fake failure")
        .to_string();
    Ok(FakeFailure {
        call_index: usize::try_from(call_index).unwrap_or(usize::MAX),
        attempt: u32::try_from(attempt).unwrap_or(u32::MAX),
        phase,
        kind,
        status,
        retry_after_ms,
        message,
    })
}

fn parse_error_kind(raw: &str) -> Option<LlmErrorKind> {
    let normalized = raw.to_ascii_lowercase().replace(['-', '_'], "");
    Some(match normalized.as_str() {
        "connect" => LlmErrorKind::Connect,
        "timeout" => LlmErrorKind::Timeout,
        "ratelimited" => LlmErrorKind::RateLimited,
        "servererror" => LlmErrorKind::ServerError,
        "clienterror" => LlmErrorKind::ClientError,
        "streambroken" => LlmErrorKind::StreamBroken,
        "decode" => LlmErrorKind::Decode,
        "cancelled" => LlmErrorKind::Cancelled,
        _ => return None,
    })
}

fn optional_u64(value: Option<&Value>, name: &str) -> Result<Option<u64>, String> {
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(value) => value
            .as_u64()
            .map(Some)
            .ok_or_else(|| format!("{name} must be a non-negative integer")),
    }
}

fn optional_u32(value: Option<&Value>, name: &str) -> Result<Option<u32>, String> {
    Ok(optional_u64(value, name)?.map(|value| u32::try_from(value).unwrap_or(u32::MAX)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ctx::CancelToken;
    use crate::llm::LlmMessage;
    use crate::test_support::TestCtx;
    use serde_json::json;
    use std::sync::Arc;

    #[derive(Default)]
    struct CollectSink {
        deltas: Vec<LlmDelta>,
    }

    impl LlmStreamSink for CollectSink {
        fn on_delta(&mut self, delta: LlmDelta) {
            self.deltas.push(delta);
        }
    }

    fn provider(script: Value) -> Arc<FakeLlmProvider> {
        Arc::new(FakeLlmProvider::from_script(script).expect("valid script"))
    }

    fn request() -> LlmRequest {
        crate::llm::LlmRequest::new(
            crate::llm::ModelRef {
                provider: crate::llm::ProviderKind::Deepseek,
                name: "deepseek-chat".to_string(),
                base_url: None,
            },
            vec![
                LlmMessage::System {
                    content: "sys".to_string(),
                },
                LlmMessage::User {
                    content: "{}".to_string(),
                },
            ],
            crate::llm::SecretString::new("key"),
        )
    }

    fn text_script(content: &str) -> Value {
        json!({
            "version": 1,
            "chunk_chars": 2,
            "calls": [{"kind": "text", "content": content}]
        })
    }

    #[tokio::test]
    async fn streams_text_in_chunks_and_reports_estimated_usage() {
        let provider = provider(text_script("abcdef"));
        let ctx = TestCtx::new();
        let cancel = CancelToken::new(ctx.clone());
        let mut sink = CollectSink::default();
        let response = provider
            .chat_stream(request(), &mut sink, &cancel)
            .await
            .expect("stream");
        assert_eq!(
            sink.deltas,
            vec![
                LlmDelta::Token {
                    text: "ab".to_string()
                },
                LlmDelta::Token {
                    text: "cd".to_string()
                },
                LlmDelta::Token {
                    text: "ef".to_string()
                },
            ]
        );
        assert_eq!(response.content.as_deref(), Some("abcdef"));
        assert_eq!(response.stop_reason, StopReason::EndTurn);
        assert_eq!(response.usage.source, UsageSource::Estimated);
        assert_eq!(
            response.usage.total_tokens,
            response.usage.prompt_tokens + response.usage.completion_tokens
        );
        assert_eq!(provider.requests().len(), 1);
    }

    #[tokio::test]
    async fn supports_single_and_multiple_tool_calls_in_order() {
        let provider = provider(json!({
            "calls": [{
                "kind": "tool_calls",
                "content": null,
                "tool_calls": [
                    {"call_id": "c1", "name": "add", "args": {"a": 1}},
                    {"call_id": "c2", "name": "now", "args": {}}
                ]
            }]
        }));
        let ctx = TestCtx::new();
        let cancel = CancelToken::new(ctx.clone());
        let mut sink = CollectSink::default();
        let response = provider
            .chat_stream(request(), &mut sink, &cancel)
            .await
            .expect("stream");
        assert_eq!(response.tool_calls.len(), 2);
        assert_eq!(response.tool_calls[0].call_id, "c1");
        assert_eq!(response.tool_calls[1].call_id, "c2");
        assert_eq!(response.stop_reason, StopReason::ToolCalls);
        assert_eq!(response.content, None);
        // tool_call 增量只供内部聚合观察，不产生正文增量。
        assert!(sink
            .deltas
            .iter()
            .all(|delta| !matches!(delta, LlmDelta::Token { .. })));
    }

    #[tokio::test]
    async fn injects_establish_failure_then_succeeds_on_retry() {
        let provider = provider(json!({
            "chunk_chars": 64,
            "calls": [{"kind": "text", "content": "ok"}],
            "failures": [{
                "call_index": 0, "attempt": 0, "phase": "establish",
                "kind": "RateLimited", "status": 429, "retry_after_ms": 100
            }]
        }));
        let ctx = TestCtx::new();
        let cancel = CancelToken::new(ctx.clone());
        let mut sink = CollectSink::default();
        let error = provider
            .chat_stream(request(), &mut sink, &cancel)
            .await
            .expect_err("injected");
        assert_eq!(error.kind, LlmErrorKind::RateLimited);
        assert_eq!(error.phase, LlmPhase::Establish);
        assert_eq!(error.status, Some(429));
        assert_eq!(error.retry_after_ms, Some(100));
        assert_eq!(error.streamed_tokens, 0);
        assert!(sink.deltas.is_empty());
        assert!(error.is_retryable());

        let response = provider
            .chat_stream(request(), &mut sink, &cancel)
            .await
            .expect("retry succeeds");
        assert_eq!(response.content.as_deref(), Some("ok"));
        assert_eq!(response.attempts, 2);
        assert_eq!(provider.requests().len(), 2);
    }

    #[tokio::test]
    async fn injects_streaming_failure_with_partial_delta() {
        let provider = provider(json!({
            "chunk_chars": 64,
            "calls": [{"kind": "text", "content": "partial"}],
            "failures": [{
                "call_index": 0, "attempt": 0, "phase": "streaming",
                "kind": "StreamBroken", "message": "mid-stream"
            }]
        }));
        let ctx = TestCtx::new();
        let cancel = CancelToken::new(ctx.clone());
        let mut sink = CollectSink::default();
        let error = provider
            .chat_stream(request(), &mut sink, &cancel)
            .await
            .expect_err("injected");
        assert_eq!(error.phase, LlmPhase::Streaming);
        assert_eq!(error.kind, LlmErrorKind::StreamBroken);
        assert!(error.streamed_tokens > 0);
        assert!(!error.is_retryable());
        assert_eq!(sink.deltas.len(), 1);
    }

    #[tokio::test]
    async fn cancellation_is_reported_as_cancelled() {
        let provider = provider(text_script("hello"));
        let ctx = TestCtx::new();
        ctx.request_cancel();
        let cancel = CancelToken::new(ctx);
        let mut sink = CollectSink::default();
        let error = provider
            .chat_stream(request(), &mut sink, &cancel)
            .await
            .expect_err("cancelled");
        assert_eq!(error.kind, LlmErrorKind::Cancelled);
        assert!(error.is_cancelled());
        assert!(sink.deltas.is_empty());
    }

    #[tokio::test]
    async fn script_is_repeated_after_exhaustion_and_usage_can_be_provider() {
        let provider = provider(json!({
            "chunk_chars": 64,
            "calls": [{"kind": "text", "content": "x",
                       "usage": {"prompt_tokens": 10, "completion_tokens": 2}}]
        }));
        let ctx = TestCtx::new();
        let cancel = CancelToken::new(ctx.clone());
        let mut sink = CollectSink::default();
        let first = provider
            .chat_stream(request(), &mut sink, &cancel)
            .await
            .expect("first");
        let second = provider
            .chat_stream(request(), &mut sink, &cancel)
            .await
            .expect("second (repeat last)");
        assert_eq!(first.usage.source, UsageSource::Provider);
        assert_eq!(first.usage.prompt_tokens, 10);
        assert_eq!(first.usage.total_tokens, 12);
        assert_eq!(second.content.as_deref(), Some("x"));
    }

    #[test]
    fn rejects_invalid_scripts() {
        assert!(FakeLlmProvider::from_script(json!({"version": 2, "calls": []})).is_err());
        assert!(FakeLlmProvider::from_script(json!({"calls": []})).is_err());
        assert!(FakeLlmProvider::from_script(json!({"calls": [{"kind": "text"}]})).is_err());
        assert!(
            FakeLlmProvider::from_script(json!({"calls": [{"kind": "nope", "content": "x"}]}))
                .is_err()
        );
        assert!(FakeLlmProvider::from_script(json!({
            "calls": [{"kind": "text", "content": "x"}],
            "failures": [{"call_index": 0, "phase": "establish", "kind": "nope"}]
        }))
        .is_err());
    }

    #[test]
    fn loads_script_from_path() {
        let dir = tempfile::TempDir::new().expect("tempdir");
        let path = dir.path().join("fake.json");
        std::fs::write(
            &path,
            json!({"calls": [{"kind": "text", "content": "hi"}]}).to_string(),
        )
        .expect("write");
        let provider = FakeLlmProvider::from_path(&path).expect("load");
        assert_eq!(provider.call_index(), 0);
        let missing = FakeLlmProvider::from_path(&dir.path().join("missing.json"));
        assert!(missing.is_err());
    }
}
