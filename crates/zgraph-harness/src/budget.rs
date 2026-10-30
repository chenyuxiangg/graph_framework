//! token 预算与确定性估算（05 §5.4.3–§5.4.5）。
//!
//! v1 **不引入分词器依赖**：用确定性字符近似（误差 ±20%），由
//! `prompt_budget = floor(context_window × 0.8) − max_tokens` 的 20% 安全系数吸收。
//! 裁剪只允许丢“短期记忆”（v1 恒空）；仍溢出 → `HARNESS_CONTEXT_OVERFLOW`
//! （Crashed；绝不静默截断 user/system，05 §5.4.5）。

use serde_json::{json, Value};

use crate::constants::CONTEXT_BUDGET_SAFETY_RATIO;
use crate::error::HarnessError;
use crate::llm::LlmMessage;
use crate::tools::ToolSchema;

/// v1 字符类 token 估算（05 §5.4.4）。
///
/// `tokens = ceil(n_ascii / 4 + n_cjk × 1 + n_other × 2)`；
/// 字符类边界：ASCII（U+0000–U+007F）、CJK/全角（U+2E80–U+9FFF、
/// U+3000–U+303F、U+FF00–U+FFEF）、其它非 ASCII（emoji、其他文字）。
pub fn estimate_tokens(text: &str) -> u32 {
    let mut ascii: u32 = 0;
    let mut cjk: u32 = 0;
    let mut other: u32 = 0;
    for ch in text.chars() {
        let code_point = ch as u32;
        if code_point <= 0x7f {
            ascii = ascii.saturating_add(1);
        } else if is_cjk(code_point) {
            cjk = cjk.saturating_add(1);
        } else {
            other = other.saturating_add(1);
        }
    }
    let weighted = f64::from(ascii) / 4.0 + f64::from(cjk) + f64::from(other) * 2.0;
    weighted.ceil() as u32
}

/// CJK 统一表意文字 / 全角标点区段。
fn is_cjk(code_point: u32) -> bool {
    matches!(code_point, 0x2E80..=0x9FFF | 0xFF00..=0xFFEF)
}

/// 单条消息的 token 估算 = 内容估算 + 4 token 协议开销（05 §5.4.4）。
pub fn estimate_message_tokens(message: &LlmMessage) -> u32 {
    match message {
        LlmMessage::System { content } | LlmMessage::User { content } => {
            estimate_tokens(content).saturating_add(4)
        }
        LlmMessage::Assistant {
            content,
            tool_calls,
        } => {
            let mut tokens = content
                .as_deref()
                .map_or(0, estimate_tokens)
                .saturating_add(4);
            for call in tool_calls {
                tokens = tokens
                    .saturating_add(estimate_tokens(&call.name))
                    .saturating_add(estimate_tokens(&call.args_raw));
            }
            tokens
        }
        LlmMessage::Tool { content, name, .. } => estimate_tokens(content)
            .saturating_add(estimate_tokens(name))
            .saturating_add(4),
    }
}

/// 消息序列的 token 估算。
pub fn estimate_messages_tokens(messages: &[LlmMessage]) -> u32 {
    messages.iter().fold(0u32, |total, message| {
        total.saturating_add(estimate_message_tokens(message))
    })
}

/// 预算总量公式（05 §5.4.4）：
/// `prompt_budget_tokens = floor(context_window_tokens × 0.8) − max_tokens`。
pub fn prompt_budget(context_window_tokens: u32, max_tokens: u32) -> u32 {
    let window = f64::from(context_window_tokens) * CONTEXT_BUDGET_SAFETY_RATIO;
    (window.floor() as u64)
        .saturating_sub(u64::from(max_tokens))
        .min(u64::from(u32::MAX)) as u32
}

/// 预算分段估算（05 §5.7.4 `detail.segments` 的固定键集）。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct BudgetSegments {
    /// system 头部（role prompt）。
    pub system: u32,
    /// 约束段。
    pub constraints: u32,
    /// 工具 JSON Schema（请求 `tools` 字段）。
    pub tools_schema: u32,
    /// 短期记忆（v1 恒 0）。
    pub short_term: u32,
    /// user 输入。
    pub user_input: u32,
    /// tool 结果 + 历史 assistant 轮（当前请求上下文，不参与裁剪）。
    pub tool_results: u32,
}

impl BudgetSegments {
    /// 分段总量。
    pub fn total(&self) -> u32 {
        self.system
            .saturating_add(self.constraints)
            .saturating_add(self.tools_schema)
            .saturating_add(self.short_term)
            .saturating_add(self.user_input)
            .saturating_add(self.tool_results)
    }

    /// 05 §5.7.4 的 segments JSON。
    pub fn to_json(&self) -> Value {
        json!({
            "system": self.system,
            "constraints": self.constraints,
            "tools_schema": self.tools_schema,
            "short_term": self.short_term,
            "user_input": self.user_input,
            "tool_results": self.tool_results,
        })
    }

    /// 按 05 §5.7.4 的语义找第一个使累计量超预算的段。
    pub fn first_over(&self, budget: u32) -> &'static str {
        let ordered: [(&'static str, u32); 6] = [
            ("system", self.system),
            ("constraints", self.constraints),
            ("tools_schema", self.tools_schema),
            ("short_term", self.short_term),
            ("user_input", self.user_input),
            ("tool_results", self.tool_results),
        ];
        let mut accumulated: u32 = 0;
        for (name, tokens) in ordered {
            accumulated = accumulated.saturating_add(tokens);
            if accumulated > budget {
                return name;
            }
        }
        "tool_results"
    }
}

/// 每次 LLM 调用前的预算检查（05 §5.4.5 `check_and_trim`）。
///
/// v1 不触发裁剪（短期记忆恒空）。`constraints_tokens` 由
/// [`crate::assemble::SystemPrompt`] 提供，用于把 system 段拆成
/// `system` / `constraints` 两个诊断键。
///
/// 历史 assistant 轮（正文与 tool_call 参数）与 tool 结果同属“当前请求上下文”，
/// 计入 `tool_results` 段——它们不参与裁剪，且必须在溢出诊断中可见。
pub fn check_and_trim(
    messages: &[LlmMessage],
    tool_schemas: &[ToolSchema],
    budget: u32,
    constraints_tokens: u32,
) -> Result<(), HarnessError> {
    let mut segments = BudgetSegments {
        constraints: constraints_tokens,
        tools_schema: tool_schemas.iter().fold(0u32, |total, schema| {
            total.saturating_add(schema.estimated_tokens())
        }),
        ..BudgetSegments::default()
    };
    let mut system_seen = false;
    for message in messages {
        match message {
            LlmMessage::System { content } => {
                if !system_seen {
                    system_seen = true;
                    // system 消息 = role prompt + 约束段；拆出约束后归入 system 段。
                    let total = estimate_tokens(content).saturating_add(4);
                    segments.system = total.saturating_sub(constraints_tokens);
                }
            }
            LlmMessage::User { .. } => {
                segments.user_input = segments
                    .user_input
                    .saturating_add(estimate_message_tokens(message));
            }
            LlmMessage::Assistant { .. } | LlmMessage::Tool { .. } => {
                segments.tool_results = segments
                    .tool_results
                    .saturating_add(estimate_message_tokens(message));
            }
        }
    }

    let required = segments.total();
    if required <= budget {
        return Ok(());
    }
    let first_over = segments.first_over(budget);
    Err(HarnessError::context_overflow(
        budget,
        required,
        &segments.to_json(),
        first_over,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assemble::{assemble_initial_messages, assemble_system, SystemPrompt};
    use crate::llm::{ToolCall, Usage};

    fn schema(name: &str) -> ToolSchema {
        ToolSchema {
            name: name.to_string(),
            description: "desc".to_string(),
            parameters: json!({"type": "object", "properties": {}}),
        }
    }

    #[test]
    fn estimates_ascii_by_quarter_tokens() {
        assert_eq!(estimate_tokens(""), 0);
        assert_eq!(estimate_tokens("abcd"), 1);
        assert_eq!(estimate_tokens("abcde"), 2);
    }

    #[test]
    fn estimates_cjk_and_other_non_ascii() {
        assert_eq!(estimate_tokens("中文"), 2);
        assert_eq!(estimate_tokens("😀"), 2);
        // 4 ascii (1) + 2 cjk (2) + 1 emoji (2) = 5
        assert_eq!(estimate_tokens("abcd中文😀"), 5);
    }

    #[test]
    fn message_estimate_includes_protocol_overhead() {
        let message = LlmMessage::User {
            content: "abcd".to_string(),
        };
        assert_eq!(estimate_message_tokens(&message), 5);
        let assistant = LlmMessage::Assistant {
            content: Some("abcd".to_string()),
            tool_calls: vec![ToolCall {
                call_id: "c1".to_string(),
                name: "now".to_string(),
                args: json!({}),
                args_raw: "{}".to_string(),
            }],
        };
        // 1 (content) + 4 (overhead) + 1 (name) + 1 (args "{}") = 7
        assert_eq!(estimate_message_tokens(&assistant), 7);
    }

    #[test]
    fn prompt_budget_matches_spec_formula() {
        assert_eq!(prompt_budget(65_536, 2_048), 50_380);
        assert_eq!(prompt_budget(4_096, 2_048), 1_228);
        assert_eq!(prompt_budget(1_000, 2_000), 0);
    }

    #[test]
    fn budget_passes_when_under_limit() {
        let system = assemble_system("you are helpful", Some("be brief"));
        let messages = assemble_initial_messages(&system.text, "{}");
        check_and_trim(
            &messages,
            &[schema("now")],
            50_380,
            system.constraints_tokens,
        )
        .expect("under budget");
    }

    #[test]
    fn overflow_reports_segments_and_first_over() {
        let system = assemble_system("role", None);
        let mut messages = assemble_initial_messages(&system.text, "{}");
        let huge_tool = "x".repeat(60_000);
        messages.push(LlmMessage::Tool {
            tool_call_id: "c1".to_string(),
            name: "big".to_string(),
            content: huge_tool,
        });
        let error = check_and_trim(&messages, &[], 1_000, system.constraints_tokens)
            .expect_err("must overflow");
        assert_eq!(
            crate::error::HarnessError::code_for(error.kind),
            "HARNESS_CONTEXT_OVERFLOW"
        );
        assert_eq!(error.detail["code"], crate::error::CODE_CONTEXT_OVERFLOW);
        assert_eq!(error.detail["stage"], "assemble");
        assert_eq!(error.detail["first_over"], "tool_results");
        assert!(error.detail["segments"]["tool_results"].as_u64().unwrap() > 10_000);
    }

    #[test]
    fn constraints_segment_is_split_from_system() {
        let system = assemble_system("role", Some("constraint text"));
        let messages = assemble_initial_messages(&system.text, "{}");
        let errors = check_and_trim(&messages, &[], 0, system.constraints_tokens);
        let error = errors.expect_err("must overflow with zero budget");
        assert_eq!(
            error.detail["segments"]["constraints"],
            system.constraints_tokens
        );
        assert!(error.detail["segments"]["system"].as_u64().unwrap() >= 1);
    }

    #[test]
    fn usage_accumulation_keeps_reasoning_tokens() {
        let mut total = Usage::provider(10, 5, None, Some(2));
        total.accumulate(&Usage::provider(20, 6, None, Some(3)));
        assert_eq!(total.prompt_tokens, 30);
        assert_eq!(total.completion_tokens, 11);
        assert_eq!(total.total_tokens, 41);
        assert_eq!(total.reasoning_tokens, Some(5));
    }

    #[test]
    fn system_prompt_type_is_reachable() {
        let prompt: SystemPrompt = assemble_system("a", None);
        assert_eq!(prompt.text, "a");
    }
}
