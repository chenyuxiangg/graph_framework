//! LLM 调用记录 seam（ISSUE-082；architecture-v2 §5.2 L2 `llm_calls` 前置）。
//!
//! 05 §5.1.1 **H3：Harness 不写 SQLite**。L2（recorded outputs）需要的
//! `llm_calls(run_id, instance_id, call_index, request_json, response_json, status, recorded_at)`
//! 由本 seam 把「每次 LLM 调用的最终结果」交给**观察者**（M5 CLI/trace 层实现落库）；
//! Harness 只负责组装脱敏快照并保证「每轮恰记录一次」。
//!
//! 纪律：
//! - 记录器是**观察者**：不得改变执行语义；实现必须快速、非阻塞（不得 await/IO）；
//! - 记录器 panic **不得**传播到 Harness（[`record_safely`] 吞掉）；
//! - 请求快照**永不**包含 `api_key`/`SecretString` 明文（本模块不读取该字段）；
//! - 重试只记录**最终结果**（同一 `call_index` 只产生一条记录）。

use std::panic::{catch_unwind, AssertUnwindSafe};

use serde_json::{json, Value};

use crate::llm::{LlmError, LlmMessage, LlmRequest, LlmResponse};
use crate::tools::ToolSchema;

/// 成功调用的 `status`（写入 `llm_calls.status`）。
pub const LLM_CALL_STATUS_OK: &str = "ok";

/// 终态失败的 `status`（写入 `llm_calls.status`）。
pub const LLM_CALL_STATUS_ERROR: &str = "error";

/// LLM 调用记录器（M5 CLI/trace 层实现；05 §5.1.1 H3 的写端 seam）。
///
/// 实现方把快照落到 `llm_calls` 表或先缓冲再批量写；**不得**阻塞事件循环
/// （只做内存追加 / `try_send`，不得 await、不得做文件/DB IO）。
pub trait LlmCallRecorder: Send + Sync {
    /// 记录一次 LLM 调用的最终结果（每轮恰一次；重试只记最终结果）。
    fn record_llm_call(&self, call: &LlmCallSnapshot<'_>);
}

/// 一次 LLM 调用的只读快照。
pub struct LlmCallSnapshot<'a> {
    /// 执行实例 ID（`NodeContext.instance_id()`；`llm_calls.instance_id`）。
    pub instance_id: &'a str,
    /// 迭代轮次内序号（0 起；同一轮重试不递增）。
    pub call_index: u32,
    /// 请求快照（**已脱敏**，永不包含 `api_key`）。
    pub request: &'a Value,
    /// 响应快照：成功 = 聚合结果；终态失败 = `{"error":{kind,message,phase}}`。
    pub response: Option<&'a Value>,
    /// `ok` / `error`（[`LLM_CALL_STATUS_OK`] / [`LLM_CALL_STATUS_ERROR`]）。
    pub status: &'a str,
}

/// 安全调用记录器：记录器 panic 不传播（观察者不改变执行语义）。
pub(crate) fn record_safely(recorder: &dyn LlmCallRecorder, call: &LlmCallSnapshot<'_>) {
    // 记录器是外部注入的观察者；panic 静默吞掉（Harness 无日志设施，且不得
    // 因此改变执行语义）。`AssertUnwindSafe` 只用于不持有跨调用状态的快照。
    let _ = catch_unwind(AssertUnwindSafe(|| recorder.record_llm_call(call)));
}

/// 组装请求快照（05 §5.3.2 可观测字段；**脱敏**）。
///
/// 只含 `model{provider,name,base_url}` / `messages` / `tools` / `temperature` /
/// `max_tokens` / `stream` / `timeout_ms`；`api_key` 字段**不会**被读取。
pub(crate) fn request_snapshot(request: &LlmRequest) -> Value {
    json!({
        "model": {
            "provider": request.model.provider.as_str(),
            "name": request.model.name,
            "base_url": request.model.base_url,
        },
        "messages": request
            .messages
            .iter()
            .map(message_snapshot)
            .collect::<Vec<Value>>(),
        "tools": request
            .tools
            .iter()
            .map(tool_snapshot)
            .collect::<Vec<Value>>(),
        "temperature": request.temperature,
        "max_tokens": request.max_tokens,
        "stream": request.stream,
        "timeout_ms": request.timeout_ms,
    })
}

/// 单条消息 → JSON（05 §5.4.1 角色 + tool_call 结构）。
fn message_snapshot(message: &LlmMessage) -> Value {
    match message {
        LlmMessage::System { content } => json!({"role": "system", "content": content}),
        LlmMessage::User { content } => json!({"role": "user", "content": content}),
        LlmMessage::Assistant {
            content,
            tool_calls,
        } => json!({
            "role": "assistant",
            "content": content,
            "tool_calls": tool_calls
                .iter()
                .map(|call| json!({
                    "call_id": call.call_id,
                    "name": call.name,
                    "args": call.args,
                }))
                .collect::<Vec<Value>>(),
        }),
        LlmMessage::Tool {
            tool_call_id,
            name,
            content,
        } => json!({
            "role": "tool",
            "tool_call_id": tool_call_id,
            "name": name,
            "content": content,
        }),
    }
}

/// 工具 schema → JSON（`name`/`description`/`parameters`，05 §5.6.3）。
fn tool_snapshot(schema: &ToolSchema) -> Value {
    json!({
        "name": schema.name,
        "description": schema.description,
        "parameters": schema.parameters,
    })
}

/// 成功响应快照：05 §5.3.2 聚合结果 + 05 §5.9.2 字面量。
pub(crate) fn success_response_snapshot(response: &LlmResponse) -> Value {
    json!({
        "content": response.content,
        "reasoning": response.reasoning,
        "tool_calls": response
            .tool_calls
            .iter()
            .map(|call| json!({
                "call_id": call.call_id,
                "name": call.name,
                "args": call.args,
            }))
            .collect::<Vec<Value>>(),
        "stop_reason": response.stop_reason.as_literal(),
        "usage": {
            "prompt_tokens": response.usage.prompt_tokens,
            "completion_tokens": response.usage.completion_tokens,
            "total_tokens": response.usage.total_tokens,
            "source": response.usage.source.as_str(),
        },
        "latency_ms": response.latency_ms,
        "attempts": response.attempts,
        "provider_request_id": response.provider_request_id,
    })
}

/// 终态失败响应快照：`{"error": {"kind", "message", "phase"}}`。
///
/// `kind` 用 [`LlmErrorKind::as_str`] 的稳定小写名（与 `HarnessError.detail.kind`
/// 一致）。`message` 由 transport 构造，**不含密钥**（05 §5.3.2）。
pub(crate) fn error_response_snapshot(error: &LlmError) -> Value {
    json!({
        "error": {
            "kind": error.kind.as_str(),
            "message": error.message,
            "phase": error.phase.as_str(),
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::{
        LlmDelta, LlmPhase, LlmStreamSink, ModelRef, ProviderKind, SecretString, StopReason,
        ToolCall, Usage, UsageSource,
    };

    const TEST_KEY: &str = "sk-test-secret-key-0123456789";

    fn request() -> LlmRequest {
        LlmRequest {
            model: ModelRef {
                provider: ProviderKind::OpenAiCompatible,
                name: "deepseek-chat".to_string(),
                base_url: Some("https://llm.example/v1".to_string()),
            },
            messages: vec![
                LlmMessage::System {
                    content: "sys".to_string(),
                },
                LlmMessage::User {
                    content: "{\"message\":\"hi\"}".to_string(),
                },
                LlmMessage::Assistant {
                    content: None,
                    tool_calls: vec![ToolCall {
                        call_id: "c1".to_string(),
                        name: "now".to_string(),
                        args: json!({}),
                        args_raw: "{}".to_string(),
                    }],
                },
                LlmMessage::Tool {
                    tool_call_id: "c1".to_string(),
                    name: "now".to_string(),
                    content: "{\"now\":\"t\"}".to_string(),
                },
            ],
            tools: vec![ToolSchema {
                name: "now".to_string(),
                description: "clock".to_string(),
                parameters: json!({"type": "object", "properties": {}}),
            }],
            temperature: 0.2,
            max_tokens: 256,
            stream: true,
            api_key: SecretString::new(TEST_KEY),
            timeout_ms: 1_500,
        }
    }

    fn ok_response() -> LlmResponse {
        LlmResponse {
            content: Some("done".to_string()),
            reasoning: Some("why".to_string()),
            tool_calls: vec![ToolCall {
                call_id: "c2".to_string(),
                name: "now".to_string(),
                args: json!({"x": 1}),
                args_raw: "{\"x\":1}".to_string(),
            }],
            stop_reason: StopReason::ToolCalls,
            usage: Usage {
                prompt_tokens: 10,
                completion_tokens: 4,
                total_tokens: 14,
                reasoning_tokens: Some(2),
                source: UsageSource::Provider,
            },
            latency_ms: 42,
            provider_request_id: Some("req-1".to_string()),
            attempts: 2,
        }
    }

    #[test]
    fn request_snapshot_has_observable_fields_and_no_secret() {
        let snapshot = request_snapshot(&request());
        assert_eq!(snapshot["model"]["provider"], json!("openai_compatible"));
        assert_eq!(snapshot["model"]["name"], json!("deepseek-chat"));
        assert_eq!(
            snapshot["model"]["base_url"],
            json!("https://llm.example/v1")
        );
        // `temperature` 是 f32（0.2f32 → 0.20000000298…），按近似值断言。
        let temperature = snapshot["temperature"].as_f64().expect("temperature");
        assert!(
            (temperature - 0.2).abs() < 1e-6,
            "temperature: {temperature}"
        );
        assert_eq!(snapshot["max_tokens"], json!(256));
        assert_eq!(snapshot["stream"], json!(true));
        assert_eq!(snapshot["timeout_ms"], json!(1_500));
        assert_eq!(snapshot["messages"].as_array().expect("messages").len(), 4);
        assert_eq!(snapshot["messages"][0]["role"], json!("system"));
        assert_eq!(
            snapshot["messages"][2]["tool_calls"][0]["call_id"],
            json!("c1")
        );
        assert_eq!(snapshot["messages"][3]["role"], json!("tool"));
        assert_eq!(snapshot["tools"][0]["name"], json!("now"));

        // 脱敏：序列化文本不得出现密钥明文，也不得出现 api_key 字段名。
        let text = serde_json::to_string(&snapshot).expect("serialize snapshot");
        assert!(!text.contains(TEST_KEY), "snapshot leaked api key: {text}");
        assert!(
            !text.contains("api_key"),
            "snapshot has api_key field: {text}"
        );
        assert!(!text.contains("SecretString"));
    }

    #[test]
    fn success_snapshot_matches_contract_shape() {
        let snapshot = success_response_snapshot(&ok_response());
        assert_eq!(snapshot["content"], json!("done"));
        assert_eq!(snapshot["reasoning"], json!("why"));
        assert_eq!(snapshot["tool_calls"][0]["name"], json!("now"));
        assert_eq!(snapshot["tool_calls"][0]["args"], json!({"x": 1}));
        assert_eq!(snapshot["stop_reason"], json!("ToolCalls"));
        assert_eq!(snapshot["usage"]["prompt_tokens"], json!(10));
        assert_eq!(snapshot["usage"]["completion_tokens"], json!(4));
        assert_eq!(snapshot["usage"]["total_tokens"], json!(14));
        assert_eq!(snapshot["usage"]["source"], json!("provider"));
        assert_eq!(snapshot["latency_ms"], json!(42));
        assert_eq!(snapshot["attempts"], json!(2));
        assert_eq!(snapshot["provider_request_id"], json!("req-1"));
    }

    #[test]
    fn error_snapshot_carries_kind_message_phase() {
        let error = LlmError::new(
            crate::llm::LlmErrorKind::RateLimited,
            LlmPhase::Establish,
            "HTTP 429: slow down",
            0,
        );
        let snapshot = error_response_snapshot(&error);
        assert_eq!(snapshot["error"]["kind"], json!("rate_limited"));
        assert_eq!(snapshot["error"]["message"], json!("HTTP 429: slow down"));
        assert_eq!(snapshot["error"]["phase"], json!("establish"));
    }

    /// 最小记录器：只用于验证 [`record_safely`] 吞 panic 的语义。
    struct PanickingRecorder;

    impl LlmCallRecorder for PanickingRecorder {
        fn record_llm_call(&self, _call: &LlmCallSnapshot<'_>) {
            panic!("recorder must not propagate");
        }
    }

    #[test]
    fn record_safely_swallows_recorder_panic() {
        let request = request_snapshot(&request());
        let recorder = PanickingRecorder;
        let snapshot = LlmCallSnapshot {
            instance_id: "inst",
            call_index: 0,
            request: &request,
            response: None,
            status: LLM_CALL_STATUS_ERROR,
        };
        // 不 panic 即通过（panic 会被 hook 打到 stderr，属预期）。
        record_safely(&recorder, &snapshot);
    }

    /// 供 `LlmStreamSink` 占位（本模块不涉及流式；避免未使用导入告警）。
    struct NoopSink;

    impl LlmStreamSink for NoopSink {
        fn on_delta(&mut self, _delta: LlmDelta) {}
    }

    #[test]
    fn noop_sink_is_send() {
        fn assert_send<T: Send>() {}
        assert_send::<NoopSink>();
    }
}
