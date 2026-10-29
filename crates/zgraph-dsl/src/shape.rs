//! JSON 形状检查与标识符/时间格式工具（R2–R4 的公共底座）。
//!
//! 设计说明（对应 02 §2.2.3 S-2/S-3）：Loader 在 typed 构造**之前**用
//! `serde_json::Value` 做键白名单与必填检查；因此本模块只处理 `Value`，
//! 不依赖 serde derive。

use serde_json::{Map, Value};

use crate::error::{msg, DslError, DslErrorCode};

/// 键白名单检查：未列出的键 → `DSL_PARSE_ERROR`（`on_error` 例外由调用方处理）。
pub(crate) fn check_keys(
    map: &Map<String, Value>,
    allowed: &[&str],
    scope: &str,
    errors: &mut Vec<DslError>,
) {
    for key in map.keys() {
        if key == "on_error" {
            continue;
        }
        if !allowed.contains(&key.as_str()) {
            errors.push(msg::unknown_field(key, scope));
        }
    }
}

/// 必填字符串；缺失/非字符串/空串均记录错误。
pub(crate) fn required_str<'a>(
    map: &'a Map<String, Value>,
    key: &str,
    scope: &str,
    errors: &mut Vec<DslError>,
) -> Option<&'a str> {
    match map.get(key) {
        None => {
            errors.push(msg::missing_field(key, scope));
            None
        }
        Some(Value::String(text)) => Some(text.as_str()),
        Some(other) => {
            errors.push(msg::wrong_type(key, "string", scope, other));
            None
        }
    }
}

/// 必填非空字符串。
pub(crate) fn required_nonempty_str<'a>(
    map: &'a Map<String, Value>,
    key: &str,
    scope: &str,
    errors: &mut Vec<DslError>,
) -> Option<&'a str> {
    match required_str(map, key, scope, errors) {
        Some("") => {
            errors.push(msg::invalid_value(
                key,
                "a non-empty string",
                scope,
                &Value::String(String::new()),
            ));
            None
        }
        other => other,
    }
}

/// 可选字符串（存在但非字符串 → 错误）。
pub(crate) fn optional_str<'a>(
    map: &'a Map<String, Value>,
    key: &str,
    scope: &str,
    errors: &mut Vec<DslError>,
) -> Option<&'a str> {
    match map.get(key) {
        None => None,
        Some(Value::String(text)) => Some(text.as_str()),
        Some(other) => {
            errors.push(msg::wrong_type(key, "string", scope, other));
            None
        }
    }
}

/// 必填数组。
pub(crate) fn required_array<'a>(
    map: &'a Map<String, Value>,
    key: &str,
    scope: &str,
    errors: &mut Vec<DslError>,
) -> Option<&'a Vec<Value>> {
    match map.get(key) {
        None => {
            errors.push(msg::missing_field(key, scope));
            None
        }
        Some(Value::Array(items)) => Some(items),
        Some(other) => {
            errors.push(msg::wrong_type(key, "array", scope, other));
            None
        }
    }
}

/// 节点 ID 合法字符集（02 §2.2.1：1–64 字符 `[A-Za-z0-9_.-]`）。
pub(crate) fn is_node_id(text: &str) -> bool {
    !text.is_empty()
        && text.len() <= 64
        && text
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '.' | '-'))
}

/// Python 标识符（端口名规则：`^[A-Za-z_][A-Za-z0-9_]*$`，≤64）。
pub(crate) fn is_identifier(text: &str) -> bool {
    !text.is_empty() && text.len() <= 64 && {
        let mut chars = text.chars();
        let first_ok = chars
            .next()
            .is_some_and(|ch| ch == '_' || ch.is_ascii_alphabetic());
        first_ok && chars.all(|ch| ch == '_' || ch.is_ascii_alphanumeric())
    }
}

/// UTC RFC3339 时间戳（`YYYY-MM-DDTHH:MM:SS[.fff]Z` 或 `+00:00` 后缀）。
pub(crate) fn is_rfc3339_utc(text: &str) -> bool {
    let bytes = text.as_bytes();
    if bytes.len() < 20 || bytes[4] != b'-' || bytes[7] != b'-' {
        return false;
    }
    if bytes[10] != b'T' || bytes[13] != b':' || bytes[16] != b':' {
        return false;
    }
    let digits_ok =
        |mut range: std::ops::Range<usize>| range.all(|index| bytes[index].is_ascii_digit());
    if !digits_ok(0..4)
        || !digits_ok(5..7)
        || !digits_ok(8..10)
        || !digits_ok(11..13)
        || !digits_ok(14..16)
        || !digits_ok(17..19)
    {
        return false;
    }
    let rest = &text[19..];
    if rest == "Z" || rest == "+00:00" {
        return true;
    }
    let Some(fraction) = rest.strip_prefix('.') else {
        return false;
    };
    let Some(zone) = fraction
        .strip_suffix('Z')
        .or_else(|| fraction.strip_suffix("+00:00"))
    else {
        return false;
    };
    !zone.is_empty() && zone.chars().all(|ch| ch.is_ascii_digit())
}

/// 构造 `DSL_PARSE_ERROR` 便捷方法（字段取值非法）。
pub(crate) fn invalid_value(
    field: &str,
    constraint: &str,
    scope: &str,
    actual: &Value,
) -> DslError {
    msg::invalid_value(field, constraint, scope, actual)
}

/// 构造 `DSL_INVALID_PORT` 错误（节点/端口声明级）。
pub(crate) fn invalid_port(message: impl Into<String>) -> DslError {
    DslError::new(DslErrorCode::InvalidPort, message.into())
}

/// 把子图内部错误包装为 `DSL_SUBGRAPH_INVALID`（R6：子图定义非法单一错误码）。
pub(crate) fn wrap_subgraph_errors(subgraph_id: &str, errors: Vec<DslError>) -> Vec<DslError> {
    errors
        .into_iter()
        .map(|error| {
            let mut wrapped = DslError::new(
                DslErrorCode::SubgraphInvalid,
                format!("subgraph '{subgraph_id}': {}", error.message),
            )
            .expected(
                error
                    .expected
                    .clone()
                    .unwrap_or_else(|| Value::String("valid subgraph definition".to_string())),
            )
            .actual(error.actual.clone().unwrap_or(Value::Null));
            wrapped.location = error.location;
            if wrapped.location.node_id.is_none() {
                wrapped.location.node_id = Some(subgraph_id.to_string());
            }
            wrapped
        })
        .collect()
}

/// 子图级错误（无内部包装）。
pub(crate) fn subgraph_invalid(subgraph_id: &str, detail: &str) -> DslError {
    DslError::new(
        DslErrorCode::SubgraphInvalid,
        format!("subgraph '{subgraph_id}': {detail}"),
    )
    .at_node(subgraph_id.to_string())
    .expected("a valid subgraph definition")
    .actual(detail)
}
