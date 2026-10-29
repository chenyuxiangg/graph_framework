//! DSL 错误码、错误对象与错误容器。
//!
//! 契约来源：`doc/spec-v1.2/02-DSL与校验.md` §2.4/§2.7；错误码全集见
//! `doc/spec-v1.2/00-总纲.md` §0.4（DSL 域 17 个码，本 crate 不新增）。
//!
//! 失败策略（02 §2.1.3）：跨规则 fail-fast，同规则内聚合，最多返回
//! [`MAX_DSL_ERRORS`] 条；`truncated` 标记是否发生截断。

use std::fmt;

use serde::Serialize;
use serde_json::Value;

/// 单次校验最多返回的错误条数（02 §2.1.3）。
pub const MAX_DSL_ERRORS: usize = 20;

/// DSL 域错误码（00 §0.4 的 17 个 `DSL_*` 码，一一对应）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DslErrorCode {
    /// 非法 JSON / 类型不符。
    ParseError,
    /// 必填字段缺失。
    MissingField,
    /// 节点 ID 重复。
    DuplicateNodeId,
    /// `entry_points` 非法（不存在 / 是 end / 重复 / start 未被列入）。
    InvalidEntry,
    /// 边端点非法（不存在 / end 作源 / start 作目标 / entry 作目标）。
    InvalidEdgeRef,
    /// 端口引用或端口声明非法。
    InvalidPort,
    /// 同一 `(from, to_node)` 重复。
    DuplicateEdge,
    /// 同一 `(to_node, to_input_port)` 有多条入边（fan-in 禁止）。
    DuplicatePortSource,
    /// 非 entry 节点的 required 输入无来源。
    UnconnectedInput,
    /// 子图定义或引用非法。
    SubgraphInvalid,
    /// 无门控的数据环（自环计入）。
    CycleDetected,
    /// 符号无法解析（内置 gate op 未知 / python_node 符号格式非法）。
    SymbolNotFound,
    /// 符号签名/参数契约不符（内置 gate op 的 params 校验）。
    SignatureMismatch,
    /// 同名符号被两类注册（gate op 与 python_node 冲突）。
    SymbolConflict,
    /// agent 资产缺失或索引非法。
    AgentNotFound,
    /// 节点 `type` 字符串未知/保留。
    UnsupportedNodeType,
    /// 出现 v1 不支持/保留的字段（`on_error`、`from_output_port`）。
    UnsupportedField,
}

impl DslErrorCode {
    /// 稳定对外错误码字符串（00 §0.4）。
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ParseError => "DSL_PARSE_ERROR",
            Self::MissingField => "DSL_MISSING_FIELD",
            Self::DuplicateNodeId => "DSL_DUPLICATE_NODE_ID",
            Self::InvalidEntry => "DSL_INVALID_ENTRY",
            Self::InvalidEdgeRef => "DSL_INVALID_EDGE_REF",
            Self::InvalidPort => "DSL_INVALID_PORT",
            Self::DuplicateEdge => "DSL_DUPLICATE_EDGE",
            Self::DuplicatePortSource => "DSL_DUPLICATE_PORT_SOURCE",
            Self::UnconnectedInput => "DSL_UNCONNECTED_INPUT",
            Self::SubgraphInvalid => "DSL_SUBGRAPH_INVALID",
            Self::CycleDetected => "DSL_CYCLE_DETECTED",
            Self::SymbolNotFound => "DSL_SYMBOL_NOT_FOUND",
            Self::SignatureMismatch => "DSL_SIGNATURE_MISMATCH",
            Self::SymbolConflict => "DSL_SYMBOL_CONFLICT",
            Self::AgentNotFound => "DSL_AGENT_NOT_FOUND",
            Self::UnsupportedNodeType => "DSL_UNSUPPORTED_NODE_TYPE",
            Self::UnsupportedField => "DSL_UNSUPPORTED_FIELD",
        }
    }

    /// 全部 17 个 DSL 错误码（文档与测试用）。
    pub const ALL: [Self; 17] = [
        Self::ParseError,
        Self::MissingField,
        Self::DuplicateNodeId,
        Self::InvalidEntry,
        Self::InvalidEdgeRef,
        Self::InvalidPort,
        Self::DuplicateEdge,
        Self::DuplicatePortSource,
        Self::UnconnectedInput,
        Self::SubgraphInvalid,
        Self::CycleDetected,
        Self::SymbolNotFound,
        Self::SignatureMismatch,
        Self::SymbolConflict,
        Self::AgentNotFound,
        Self::UnsupportedNodeType,
        Self::UnsupportedField,
    ];
}

impl fmt::Display for DslErrorCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl Serialize for DslErrorCode {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

/// 错误定位（02 §2.7.3 + 任务扩展的 `field`）。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Location {
    /// 出错节点 ID（子图级错误填子图 ID）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub node_id: Option<String>,
    /// `edges` 数组下标（0-based）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub edge_index: Option<usize>,
    /// 端口名。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub port: Option<String>,
    /// 出错的 JSON 字段名（字段级错误）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub field: Option<String>,
}

impl Location {
    /// 是否没有定位信息（文档级错误）。
    pub fn is_empty(&self) -> bool {
        self.node_id.is_none()
            && self.edge_index.is_none()
            && self.port.is_none()
            && self.field.is_none()
    }
}

/// 单个 DSL 错误（对外稳定形状，02 §2.7.1）。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DslError {
    /// 错误码。
    pub code: DslErrorCode,
    /// 英文稳定模板 message（不得用于逻辑判断）。
    pub message: String,
    /// 结构化定位。
    pub location: Location,
    /// 期望值（允许值数组 / 类型 / 签名等）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expected: Option<Value>,
    /// 实际值。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub actual: Option<Value>,
}

impl DslError {
    /// 新建错误（无定位、无期望/实际）。
    pub fn new(code: DslErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            location: Location::default(),
            expected: None,
            actual: None,
        }
    }

    /// 设置 `node_id`。
    pub fn at_node(mut self, node_id: impl Into<String>) -> Self {
        self.location.node_id = Some(node_id.into());
        self
    }

    /// 设置 `edge_index`。
    pub fn at_edge(mut self, edge_index: usize) -> Self {
        self.location.edge_index = Some(edge_index);
        self
    }

    /// 设置 `port`。
    pub fn at_port(mut self, port: impl Into<String>) -> Self {
        self.location.port = Some(port.into());
        self
    }

    /// 设置 `field`。
    pub fn at_field(mut self, field: impl Into<String>) -> Self {
        self.location.field = Some(field.into());
        self
    }

    /// 设置 `expected`。
    pub fn expected(mut self, value: impl Into<Value>) -> Self {
        self.expected = Some(value.into());
        self
    }

    /// 设置 `actual`。
    pub fn actual(mut self, value: impl Into<Value>) -> Self {
        self.actual = Some(value.into());
        self
    }
}

impl fmt::Display for DslError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "error[{}] {}", self.code, self.message)
    }
}

/// 一次加载/校验的全部错误（02 §2.1.3 的聚合容器）。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DslErrors {
    /// 错误列表（按规则序 → 数组序；最多 [`MAX_DSL_ERRORS`] 条）。
    pub errors: Vec<DslError>,
    /// 采样总数（可能大于 `errors.len()`）。
    pub error_count: usize,
    /// 是否因超过上限而截断。
    pub truncated: bool,
}

impl DslErrors {
    /// 由完整错误列表构造（内部按上限截断）。
    pub fn new(mut errors: Vec<DslError>) -> Self {
        let error_count = errors.len();
        let truncated = error_count > MAX_DSL_ERRORS;
        errors.truncate(MAX_DSL_ERRORS);
        Self {
            errors,
            error_count,
            truncated,
        }
    }

    /// 只有一条错误。
    pub fn single(error: DslError) -> Self {
        Self::new(vec![error])
    }

    /// 首错（02 §2.7.2：`errors[0]` 即首错）。
    pub fn first(&self) -> &DslError {
        &self.errors[0]
    }

    /// 全部错误码（测试断言用）。
    pub fn codes(&self) -> Vec<DslErrorCode> {
        self.errors.iter().map(|error| error.code).collect()
    }

    /// 是否包含指定错误码。
    pub fn contains(&self, code: DslErrorCode) -> bool {
        self.errors.iter().any(|error| error.code == code)
    }
}

impl fmt::Display for DslErrors {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (index, error) in self.errors.iter().enumerate() {
            if index > 0 {
                f.write_str("; ")?;
            }
            write!(f, "{error}")?;
        }
        Ok(())
    }
}

impl std::error::Error for DslErrors {}

/// JSON 值类型名（错误 message 用）。
pub(crate) fn type_name(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

/// 消息模板（02 §2.4.2，英文稳定模板）。
pub(crate) mod msg {
    use super::{type_name, DslError, DslErrorCode};
    use serde_json::{json, Value};

    /// `scope` 说明错误位置，例如 `node 'x'`、`edges[3]`、`top level`。
    pub(crate) fn missing_field(field: &str, scope: &str) -> DslError {
        DslError::new(
            DslErrorCode::MissingField,
            format!("missing required field '{field}' in {scope}"),
        )
        .at_field(field)
        .expected(serde_json::json!(field))
        .actual(serde_json::json!("<absent>"))
    }

    pub(crate) fn unknown_field(key: &str, scope: &str) -> DslError {
        DslError::new(
            DslErrorCode::ParseError,
            format!("unknown field '{key}' in {scope}"),
        )
        .at_field(key)
        .expected(serde_json::json!("a field allowed by schema 1.0"))
        .actual(serde_json::json!(key))
    }

    pub(crate) fn unsupported_field(field: &str) -> DslError {
        DslError::new(
            DslErrorCode::UnsupportedField,
            format!("reserved field '{field}' is not supported by schema 1.0 (planned for v2)"),
        )
        .at_field(field)
        .expected(json!("<absent>"))
        .actual(json!(field))
    }

    pub(crate) fn wrong_type(field: &str, expected: &str, scope: &str, actual: &Value) -> DslError {
        DslError::new(
            DslErrorCode::ParseError,
            format!(
                "field '{field}' in {scope} expects {expected}, got {}",
                type_name(actual)
            ),
        )
        .at_field(field)
        .expected(json!(expected))
        .actual(actual.clone())
    }

    pub(crate) fn invalid_value(
        field: &str,
        constraint: &str,
        scope: &str,
        actual: &Value,
    ) -> DslError {
        DslError::new(
            DslErrorCode::ParseError,
            format!(
                "field '{field}' in {scope} must be {constraint}, got {}",
                actual
            ),
        )
        .at_field(field)
        .expected(json!(constraint))
        .actual(actual.clone())
    }

    pub(crate) fn empty_array(field: &str, scope: &str) -> DslError {
        DslError::new(
            DslErrorCode::ParseError,
            format!("field '{field}' in {scope} must not be empty"),
        )
        .at_field(field)
        .expected(json!("at least 1 item"))
        .actual(json!([]))
    }
}

#[cfg(test)]
mod tests {
    use super::DslErrorCode;

    #[test]
    fn all_error_codes_are_unique_and_match_spec() {
        let mut names: Vec<&str> = DslErrorCode::ALL.iter().map(|code| code.as_str()).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), 17, "00 §0.4 有 17 个 DSL 错误码");
        assert!(names.iter().all(|name| name.starts_with("DSL_")));
    }
}
