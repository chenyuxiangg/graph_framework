//! `HarnessError` 形态与 `NodeError` 映射（05 §5.7；00 §0.3/§0.4）。
//!
//! 映射纪律（05 §5.7.1–§5.7.3）：
//! - `HarnessError` 只在 Harness 内部流动；节点执行器出口统一转成
//!   `NodeError::Harness { kind, detail }`。
//! - **取消与实例超时不是 Harness 错误**：它们走 `NodeError::Cancelled` /
//!   `NodeError::Timeout { elapsed_ms }`（见 `agent_loop::RunFailure`）。
//! - `TOOL_ERROR` 的载体是工具结果消息，永远不进入实例错误列（05 §5.6.6）。

use serde_json::{Map, Value};
use zgraph_kernel::{HarnessErrorKind, NodeError};

use crate::llm::{LlmError, Usage};

/// 上下文预算溢出错误码（05 §5.4.5 / §5.7.4；00 §0.4）。
pub const CODE_CONTEXT_OVERFLOW: &str = "HARNESS_CONTEXT_OVERFLOW";

/// 工具执行失败码（00 §0.4 统一为 `TOOL_ERROR`；只喂回 LLM，不终止）。
pub const CODE_TOOL_ERROR: &str = "TOOL_ERROR";

/// Harness 阶段（05 §5.7.4 `detail.stage`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    /// 配置加载。
    Load,
    /// 上下文组装/预算。
    Assemble,
    /// LLM 调用。
    Invoke,
    /// 工具执行。
    Tool,
    /// 返回输出。
    Return,
}

impl Stage {
    /// 小写字面量（`detail.stage`）。
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Load => "load",
            Self::Assemble => "assemble",
            Self::Invoke => "invoke",
            Self::Tool => "tool",
            Self::Return => "return",
        }
    }
}

/// Harness 错误（05 §5.7.1）。
#[derive(Debug, Clone, PartialEq)]
pub struct HarnessError {
    /// 错误分类（复用 00 §0.3 的 `HarnessErrorKind`）。
    pub kind: HarnessErrorKind,
    /// 对外错误码（00 §0.4）。
    pub code: &'static str,
    /// 人类可读描述（含关键定位；不含密钥）。
    pub message: String,
    /// 结构化细节（05 §5.7.4；至少含 `code`/`stage`）。
    pub detail: Value,
}

impl HarnessError {
    /// 直接以 kind/code/message/detail 构造（内部使用；detail 由调用方负责 `code`/`stage`）。
    pub fn new(
        kind: HarnessErrorKind,
        code: &'static str,
        message: impl Into<String>,
        detail: Value,
    ) -> Self {
        Self {
            kind,
            code,
            message: message.into(),
            detail,
        }
    }

    /// 与外层错误码对应的 `&'static str`（`ToolError` 按 00 §0.4 用 `TOOL_ERROR`）。
    pub fn code_for(kind: HarnessErrorKind) -> &'static str {
        match kind {
            HarnessErrorKind::ToolError => CODE_TOOL_ERROR,
            other => other.code(),
        }
    }

    /// 进入节点执行器出口：`NodeError::Harness { kind, detail }`。
    pub fn into_node_error(self) -> NodeError {
        NodeError::Harness {
            kind: self.kind,
            detail: self.detail,
        }
    }

    /// 通用 detail 基座：`{code, stage}`。
    fn detail(code: &'static str, stage: Stage) -> Map<String, Value> {
        let mut detail = Map::new();
        detail.insert("code".to_string(), Value::from(code));
        detail.insert("stage".to_string(), Value::from(stage.as_str()));
        detail
    }

    /// 配置加载错误（05 §5.2.7 `detail` 统一形态）。
    #[allow(clippy::too_many_arguments)]
    pub fn config_load(
        reason: &str,
        path: Option<&str>,
        json_path: Option<&str>,
        expected: Option<Value>,
        actual: Option<Value>,
        message: impl Into<String>,
    ) -> Self {
        let code = HarnessErrorKind::ConfigLoad.code();
        let mut detail = Self::detail(code, Stage::Load);
        detail.insert("reason".to_string(), Value::from(reason));
        if let Some(path) = path {
            detail.insert("path".to_string(), Value::from(path));
        }
        if let Some(json_path) = json_path {
            detail.insert("json_path".to_string(), Value::from(json_path));
        }
        if let Some(expected) = expected {
            detail.insert("expected".to_string(), expected);
        }
        if let Some(actual) = actual {
            detail.insert("actual".to_string(), actual);
        }
        Self::new(
            HarnessErrorKind::ConfigLoad,
            code,
            message,
            Value::Object(detail),
        )
    }

    /// LLM 调用了未注册工具（05 §5.7.2：Crashed）。
    pub fn unknown_tool(tool: &str, iteration: u32, available: &[String]) -> Self {
        let code = HarnessErrorKind::UnknownTool.code();
        let mut detail = Self::detail(code, Stage::Tool);
        detail.insert("tool".to_string(), Value::from(tool));
        detail.insert("iteration".to_string(), Value::from(iteration));
        detail.insert(
            "available_tools".to_string(),
            Value::Array(
                available
                    .iter()
                    .take(32)
                    .cloned()
                    .map(Value::from)
                    .collect(),
            ),
        );
        Self::new(
            HarnessErrorKind::UnknownTool,
            code,
            format!("LLM called unregistered tool '{tool}'"),
            Value::Object(detail),
        )
    }

    /// 超过 `max_iterations`（05 §5.7.2：Crashed，携带当前上下文摘要）。
    pub fn max_iterations(
        iterations: u32,
        max_iterations: u32,
        last_content: Option<&str>,
        pending_tool_calls: &[String],
        usage: &Usage,
    ) -> Self {
        let code = HarnessErrorKind::MaxIterations.code();
        let mut detail = Self::detail(code, Stage::Return);
        detail.insert("iterations".to_string(), Value::from(iterations));
        detail.insert("max_iterations".to_string(), Value::from(max_iterations));
        detail.insert(
            "last_content_summary".to_string(),
            Value::from(summarize_chars(last_content.unwrap_or(""), 512)),
        );
        detail.insert(
            "pending_tool_calls".to_string(),
            Value::Array(
                pending_tool_calls
                    .iter()
                    .cloned()
                    .map(Value::from)
                    .collect(),
            ),
        );
        detail.insert("usage".to_string(), usage.to_json());
        Self::new(
            HarnessErrorKind::MaxIterations,
            code,
            format!("agent exceeded max_iterations={max_iterations}"),
            Value::Object(detail),
        )
    }

    /// provider 错误重试耗尽（或不可重试错误，05 §5.7.2）。
    pub fn llm_error(
        iteration: u32,
        provider: &str,
        model: &str,
        error: &LlmError,
        attempts: u32,
    ) -> Self {
        let code = HarnessErrorKind::LlmError.code();
        let detail = invoke_detail(
            code, iteration, provider, model, error, attempts, None, None,
        );
        Self::new(
            HarnessErrorKind::LlmError,
            code,
            format!(
                "LLM provider error after {attempts} attempts ({:?}): {}",
                error.kind, error.message
            ),
            detail,
        )
    }

    /// 单次 LLM 调用超时且重试耗尽（05 §5.3.6 / §5.7.2）。
    pub fn llm_timeout(
        iteration: u32,
        provider: &str,
        model: &str,
        error: &LlmError,
        attempts: u32,
        timeout_ms: u64,
    ) -> Self {
        let code = HarnessErrorKind::LlmTimeout.code();
        let detail = invoke_detail(
            code,
            iteration,
            provider,
            model,
            error,
            attempts,
            Some(timeout_ms),
            None,
        );
        Self::new(
            HarnessErrorKind::LlmTimeout,
            code,
            format!("LLM call timed out after {timeout_ms} ms ({attempts} attempts)"),
            detail,
        )
    }

    /// Harness 侧判定的 LLM 语义错误（`empty_response` / `too_many_tool_calls`）。
    pub fn llm_reason(iteration: u32, provider: &str, model: &str, reason: &str) -> Self {
        let code = HarnessErrorKind::LlmError.code();
        let mut detail = Self::detail(code, Stage::Invoke);
        detail.insert("iteration".to_string(), Value::from(iteration));
        detail.insert("provider".to_string(), Value::from(provider));
        detail.insert("model".to_string(), Value::from(model));
        detail.insert("reason".to_string(), Value::from(reason));
        Self::new(
            HarnessErrorKind::LlmError,
            code,
            format!("LLM response invalid: {reason}"),
            Value::Object(detail),
        )
    }

    /// 上下文预算溢出（05 §5.4.5 / §5.7.4）。
    ///
    /// kernel 已追加 `HarnessErrorKind::ContextOverflow`（M4 第 8 值；Lead 裁定方案 A，
    /// 与 `NodeError::NodeWorker` 的“仅增”先例一致）。`detail.code` 同为权威码
    /// `HARNESS_CONTEXT_OVERFLOW`，因此 `instances.error_code` 与 detail 一致。
    pub fn context_overflow(
        budget: u32,
        required: u32,
        segments: &Value,
        first_over: &str,
    ) -> Self {
        let mut detail = Self::detail(CODE_CONTEXT_OVERFLOW, Stage::Assemble);
        detail.insert("budget".to_string(), Value::from(budget));
        detail.insert("required".to_string(), Value::from(required));
        detail.insert("segments".to_string(), segments.clone());
        detail.insert("first_over".to_string(), Value::from(first_over));
        Self::new(
            HarnessErrorKind::ContextOverflow,
            CODE_CONTEXT_OVERFLOW,
            format!(
                "prompt budget exceeded: required {required} tokens > budget {budget} (first over: {first_over})"
            ),
            Value::Object(detail),
        )
    }

    /// 记忆降级（v1 不产生；接口保留，05 §5.10.1）。
    pub fn memory_degraded(op: &str) -> Self {
        let code = HarnessErrorKind::MemoryDegraded.code();
        let mut detail = Self::detail(code, Stage::Assemble);
        detail.insert("op".to_string(), Value::from(op));
        Self::new(
            HarnessErrorKind::MemoryDegraded,
            code,
            format!("memory operation degraded: {op}"),
            Value::Object(detail),
        )
    }
}

/// 构造 LlmError/LlmTimeout 的 `detail`（05 §5.7.4）。
#[allow(clippy::too_many_arguments)]
fn invoke_detail(
    code: &'static str,
    iteration: u32,
    provider: &str,
    model: &str,
    error: &LlmError,
    attempts: u32,
    timeout_ms: Option<u64>,
    extra: Option<(&str, Value)>,
) -> Value {
    let mut detail = HarnessError::detail(code, Stage::Invoke);
    detail.insert("iteration".to_string(), Value::from(iteration));
    detail.insert("provider".to_string(), Value::from(provider));
    detail.insert("model".to_string(), Value::from(model));
    detail.insert("kind".to_string(), Value::from(error.kind.as_str()));
    detail.insert("phase".to_string(), Value::from(error.phase.as_str()));
    detail.insert(
        "status".to_string(),
        error.status.map_or(Value::Null, Value::from),
    );
    detail.insert("attempts".to_string(), Value::from(attempts));
    detail.insert(
        "streamed_tokens".to_string(),
        Value::from(error.streamed_tokens),
    );
    if let Some(timeout_ms) = timeout_ms {
        detail.insert("timeout_ms".to_string(), Value::from(timeout_ms));
    }
    if let Some((key, value)) = extra {
        detail.insert(key.to_string(), value);
    }
    Value::Object(detail)
}

/// 按字符边界截断并附 `…`（05 §5.7.4 / §5.8.5 的截断纪律）。
pub(crate) fn summarize_chars(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    let mut out: String = text.chars().take(max_chars).collect();
    out.push('…');
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::{LlmErrorKind, LlmPhase};
    use serde_json::json;

    fn llm_error(kind: LlmErrorKind) -> LlmError {
        LlmError::new(kind, LlmPhase::Establish, "boom", 0)
    }

    #[test]
    fn seven_kinds_map_to_spec_error_codes() {
        let cases = [
            (HarnessErrorKind::ConfigLoad, "HARNESS_CONFIG_LOAD"),
            (HarnessErrorKind::UnknownTool, "HARNESS_UNKNOWN_TOOL"),
            (HarnessErrorKind::MaxIterations, "HARNESS_MAX_ITERATIONS"),
            (HarnessErrorKind::LlmError, "HARNESS_LLM_ERROR"),
            (HarnessErrorKind::LlmTimeout, "HARNESS_LLM_TIMEOUT"),
            (HarnessErrorKind::ToolError, "TOOL_ERROR"),
            (HarnessErrorKind::MemoryDegraded, "HARNESS_MEMORY_DEGRADED"),
        ];
        for (kind, code) in cases {
            assert_eq!(HarnessError::code_for(kind), code, "kind {kind:?}");
        }
    }

    #[test]
    fn config_load_detail_carries_reason_and_json_path() {
        let error = HarnessError::config_load(
            "unsupported_value",
            Some("agents/a.json"),
            Some("$.model.provider"),
            Some(json!(["deepseek", "openai_compatible"])),
            Some(json!("openai")),
            "bad provider",
        );
        assert_eq!(error.kind, HarnessErrorKind::ConfigLoad);
        assert_eq!(error.code, "HARNESS_CONFIG_LOAD");
        assert_eq!(error.detail["stage"], "load");
        assert_eq!(error.detail["reason"], "unsupported_value");
        assert_eq!(error.detail["json_path"], "$.model.provider");
        assert_eq!(error.detail["path"], "agents/a.json");
    }

    #[test]
    fn unknown_tool_detail_limits_available_tools() {
        let available: Vec<String> = (0..40).map(|i| format!("tool_{i}")).collect();
        let error = HarnessError::unknown_tool("ghost", 3, &available);
        assert_eq!(error.kind, HarnessErrorKind::UnknownTool);
        assert_eq!(error.detail["tool"], "ghost");
        assert_eq!(error.detail["iteration"], 3);
        assert_eq!(
            error.detail["available_tools"]
                .as_array()
                .expect("array")
                .len(),
            32
        );
    }

    #[test]
    fn max_iterations_detail_has_context_summary_and_usage() {
        let usage = Usage::provider(10, 5, Some(15), None);
        let pending = vec!["now".to_string(), "add".to_string()];
        let long = "x".repeat(600);
        let error = HarnessError::max_iterations(3, 3, Some(&long), &pending, &usage);
        assert_eq!(error.kind, HarnessErrorKind::MaxIterations);
        assert_eq!(error.detail["iterations"], 3);
        assert_eq!(error.detail["max_iterations"], 3);
        assert_eq!(error.detail["pending_tool_calls"], json!(["now", "add"]));
        assert_eq!(error.detail["usage"]["total_tokens"], 15);
        let summary = error.detail["last_content_summary"]
            .as_str()
            .expect("summary string");
        assert_eq!(summary.chars().count(), 513, "512 chars + ellipsis");
        assert!(summary.ends_with('…'));
    }

    #[test]
    fn llm_error_and_timeout_have_invoke_stage_details() {
        let error = llm_error(LlmErrorKind::RateLimited).with_status(Some(429), Some(120));
        let harness = HarnessError::llm_error(2, "genai-deepseek", "deepseek-chat", &error, 2);
        assert_eq!(harness.kind, HarnessErrorKind::LlmError);
        assert_eq!(harness.detail["stage"], "invoke");
        assert_eq!(harness.detail["kind"], "rate_limited");
        assert_eq!(harness.detail["phase"], "establish");
        assert_eq!(harness.detail["status"], 429);
        assert_eq!(harness.detail["attempts"], 2);

        let timeout = HarnessError::llm_timeout(
            2,
            "genai-deepseek",
            "deepseek-chat",
            &llm_error(LlmErrorKind::Timeout),
            2,
            60_000,
        );
        assert_eq!(timeout.kind, HarnessErrorKind::LlmTimeout);
        assert_eq!(timeout.code, "HARNESS_LLM_TIMEOUT");
        assert_eq!(timeout.detail["timeout_ms"], 60_000);
    }

    #[test]
    fn context_overflow_keeps_authoritative_code_in_detail() {
        let segments = json!({
            "system": 1800, "constraints": 300, "tools_schema": 900,
            "user_input": 1200, "tool_results": 56000, "short_term": 0
        });
        let error = HarnessError::context_overflow(50380, 61200, &segments, "tool_results");
        assert_eq!(error.code, CODE_CONTEXT_OVERFLOW);
        assert_eq!(error.detail["code"], CODE_CONTEXT_OVERFLOW);
        assert_eq!(error.detail["stage"], "assemble");
        assert_eq!(error.detail["first_over"], "tool_results");
        match error.into_node_error() {
            NodeError::Harness { detail, .. } => {
                assert_eq!(detail["code"], CODE_CONTEXT_OVERFLOW);
            }
            other => panic!("expected NodeError::Harness, got {other:?}"),
        }
    }

    #[test]
    fn into_node_error_preserves_kind_and_detail() {
        let error = HarnessError::unknown_tool("ghost", 1, &[]);
        match error.into_node_error() {
            NodeError::Harness { kind, detail } => {
                assert_eq!(kind, HarnessErrorKind::UnknownTool);
                assert_eq!(detail["stage"], "tool");
            }
            other => panic!("expected NodeError::Harness, got {other:?}"),
        }
    }

    #[test]
    fn summarize_chars_respects_character_boundaries() {
        assert_eq!(summarize_chars("短文本", 10), "短文本");
        let text = "中文abc";
        let summary = summarize_chars(text, 2);
        assert_eq!(summary.chars().count(), 3);
        assert!(summary.ends_with('…'));
    }
}
