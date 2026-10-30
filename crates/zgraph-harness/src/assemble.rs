//! 上下文组装（05 §5.4.2，确定性、逐字节可复现）。
//!
//! 顺序固定：**system（角色 + 约束 + [KB] + [长期记忆]）→ 短期记忆 → user 输入**。
//! v1 的 KB/长期/短期记忆恒空（05 §5.10.1），对应段**连同标题整段省略**。

use std::collections::{BTreeMap, HashMap};

use serde_json::Value;
use zgraph_dsl::InputPort;

use crate::budget::estimate_tokens;
use crate::llm::LlmMessage;

/// 组装后的 system 文本与诊断用分段估算。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SystemPrompt {
    /// 完整 system 文本（role prompt 原样 + 可选 `## 约束` 段）。
    pub text: String,
    /// role prompt 的 token 估算（不含约束段）。
    pub system_tokens: u32,
    /// 约束段正文的 token 估算（无约束段为 0）。
    pub constraints_tokens: u32,
}

/// 组装 system 消息（05 §5.4.2 布局）：
///
/// ```text
/// <role_prompt 原文>
///
/// ## 约束
/// <constraints 原文>
/// ```
///
/// 规则：constraints 为 `None` 或全空白 → **整段（含标题）省略**；段间固定两个换行；
/// role prompt 原样注入（`{{var}}` 不做模板渲染）。
pub fn assemble_system(role_prompt: &str, constraints: Option<&str>) -> SystemPrompt {
    let mut text = role_prompt.to_string();
    let mut constraints_tokens = 0;
    if let Some(constraints) = constraints {
        if !constraints.trim().is_empty() {
            text.push_str("\n\n## 约束\n");
            text.push_str(constraints);
            constraints_tokens = estimate_tokens(constraints);
        }
    }
    SystemPrompt {
        system_tokens: estimate_tokens(role_prompt),
        constraints_tokens,
        text,
    }
}

/// 计算生效的 `input_mapping`（端口名 → Agent 输入字段名）。
///
/// 优先级（Lead M4 裁定，写入 05 回写清单）：
/// 1. 节点级 `AgentCallConfig.input_mapping`（非空）；
/// 2. `AgentConfig.input_mapping`；
/// 3. identity（端口名即字段名）。
///
/// 05 §5.2.5 V15 的降级语义：映射 key 不在该节点声明端口集合内 → **丢弃该 key**
/// （不失败）；未列出的声明端口按 identity 补齐。
pub fn effective_input_mapping(
    declared_ports: &[InputPort],
    node_mapping: &BTreeMap<String, String>,
    agent_mapping: &BTreeMap<String, String>,
) -> BTreeMap<String, String> {
    let source = if node_mapping.is_empty() {
        agent_mapping
    } else {
        node_mapping
    };
    let declared: Vec<&str> = declared_ports
        .iter()
        .map(|port| port.name.as_str())
        .collect();
    let mut effective = BTreeMap::new();
    for (port, field) in source {
        if declared.contains(&port.as_str()) && !field.is_empty() {
            effective.insert(port.clone(), field.clone());
        }
    }
    for port in declared {
        effective
            .entry(port.to_string())
            .or_insert_with(|| port.to_string());
    }
    effective
}

/// 组装 user 消息 JSON（05 §5.4.2 的确定性序列化）。
///
/// - 对象来源：生效的 `input_mapping`；未映射端口 identity；
/// - 键序：升序（`BTreeMap`）；
/// - 端口值为 `null`（`trigger=any` 未到端口）→ 该字段**省略**；
/// - 全部字段缺失 → `{}`（仍发送一条 user 消息）。
pub fn assemble_user_json(
    declared_ports: &[InputPort],
    inputs: &HashMap<String, Value>,
    mapping: &BTreeMap<String, String>,
) -> String {
    let mut fields: BTreeMap<String, Value> = BTreeMap::new();
    for port in declared_ports {
        let Some(value) = inputs.get(&port.name) else {
            continue;
        };
        if value.is_null() {
            continue;
        }
        let field = mapping
            .get(&port.name)
            .cloned()
            .unwrap_or_else(|| port.name.clone());
        fields.insert(field, value.clone());
    }
    serde_json::to_string_pretty(&fields).unwrap_or_else(|_| "{}".to_string())
}

/// 组装初始消息序列：`[System, User]`（05 §5.4.2 的 ① 与 ⑥）。
pub fn assemble_initial_messages(system: &str, user_json: &str) -> Vec<LlmMessage> {
    vec![
        LlmMessage::System {
            content: system.to_string(),
        },
        LlmMessage::User {
            content: user_json.to_string(),
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn port(name: &str, required: bool) -> InputPort {
        InputPort {
            name: name.to_string(),
            port_type: zgraph_dsl::PortType::Any,
            required,
            description: None,
        }
    }

    #[test]
    fn system_omits_absent_or_blank_constraints_section() {
        let none = assemble_system("role", None);
        assert_eq!(none.text, "role");
        assert_eq!(none.constraints_tokens, 0);

        let blank = assemble_system("role", Some("   \n "));
        assert_eq!(blank.text, "role");
        assert_eq!(blank.constraints_tokens, 0);
    }

    #[test]
    fn system_section_order_is_frozen() {
        let prompt = assemble_system("你是助手。", Some("只输出 JSON。"));
        assert_eq!(prompt.text, "你是助手。\n\n## 约束\n只输出 JSON。");
    }

    #[test]
    fn user_json_is_sorted_and_pretty() {
        let ports = vec![port("z", true), port("a", true), port("m", false)];
        let inputs = HashMap::from([
            ("z".to_string(), json!(1)),
            ("a".to_string(), json!({"k": "v"})),
            ("m".to_string(), json!([1, 2])),
        ]);
        let mapping = BTreeMap::new();
        let text = assemble_user_json(&ports, &inputs, &mapping);
        assert_eq!(
            text,
            "{\n  \"a\": {\n    \"k\": \"v\"\n  },\n  \"m\": [\n    1,\n    2\n  ],\n  \"z\": 1\n}"
        );
        // 确定性：同输入同字符串。
        assert_eq!(text, assemble_user_json(&ports, &inputs, &mapping));
    }

    #[test]
    fn user_json_omits_null_ports_and_keeps_empty_object() {
        let ports = vec![port("a", false), port("b", true)];
        let inputs = HashMap::from([
            ("a".to_string(), Value::Null),
            ("b".to_string(), Value::Null),
        ]);
        assert_eq!(assemble_user_json(&ports, &inputs, &BTreeMap::new()), "{}");
    }

    #[test]
    fn mapping_renames_fields_and_applies_identity_for_rest() {
        let ports = vec![port("message", true), port("other", false)];
        let inputs = HashMap::from([
            ("message".to_string(), json!("hi")),
            ("other".to_string(), json!(2)),
        ]);
        let node_mapping = BTreeMap::from([("message".to_string(), "user_message".to_string())]);
        let mapping = effective_input_mapping(&ports, &node_mapping, &BTreeMap::new());
        assert_eq!(mapping["message"], "user_message");
        assert_eq!(mapping["other"], "other");
        let text = assemble_user_json(&ports, &inputs, &mapping);
        assert_eq!(text, "{\n  \"other\": 2,\n  \"user_message\": \"hi\"\n}");
    }

    #[test]
    fn node_mapping_wins_over_agent_mapping() {
        let ports = vec![port("message", true)];
        let node_mapping = BTreeMap::from([("message".to_string(), "node_field".to_string())]);
        let agent_mapping = BTreeMap::from([("message".to_string(), "agent_field".to_string())]);
        let mapping = effective_input_mapping(&ports, &node_mapping, &agent_mapping);
        assert_eq!(mapping["message"], "node_field");
    }

    #[test]
    fn unknown_mapping_keys_are_dropped_with_identity_fallback() {
        let ports = vec![port("message", true)];
        let node_mapping = BTreeMap::from([("ghost".to_string(), "x".to_string())]);
        let mapping = effective_input_mapping(&ports, &node_mapping, &BTreeMap::new());
        assert_eq!(mapping["message"], "message");
        assert!(!mapping.contains_key("ghost"));
    }

    #[test]
    fn initial_messages_are_system_then_user() {
        let messages = assemble_initial_messages("sys", "{}");
        assert_eq!(
            messages,
            vec![
                LlmMessage::System {
                    content: "sys".to_string()
                },
                LlmMessage::User {
                    content: "{}".to_string()
                }
            ]
        );
    }
}
