//! 图级符号解析（R14b）：内置 gate op 与 python_node 符号。
//!
//! 契约来源：`doc/spec-v1.2/02-DSL与校验.md` §2.5（符号解析）与
//! `doc/architecture-v2.md` §8 偏差 D6/D7：
//! - **D6**：v1 门控 = Rust 内置 op（`check_output` / `compare` / `manual`），
//!   DSL `gate.function` 只解析内置 op；自定义 Python 门控延后（ISSUE-014）。
//! - **D7**：图加载期**不 import 用户 Python**；`python_node` 只校验
//!   `module/function` 格式；`DSL_SYMBOL_*` 只用于内置 gate op 名称与
//!   python_node 符号格式/冲突。
//!
//! 因此本模块的"签名"是内置 op 的 **params 契约**（而非 Python 函数签名）：
//! 参数契约不符 → `DSL_SIGNATURE_MISMATCH`；符号名无法解析 → `DSL_SYMBOL_NOT_FOUND`；
//! 同名符号被两类注册 → `DSL_SYMBOL_CONFLICT`。

use std::collections::HashMap;

use serde_json::{json, Value};

use crate::error::{DslError, DslErrorCode, Location};
use crate::types::{GraphEdge, GraphNode, NodeConfig, NodeType, PythonNodeConfig};

/// 内置 gate op 名单（D6，`doc/architecture-v2.md` §4/§8）。
pub const BUILTIN_GATE_OPS: [&str; 3] = ["check_output", "compare", "manual"];

/// 内置 gate op 的 canonical 符号前缀（与 python_node 符号共享命名空间，
/// 用于跨类冲突检测；python_node 写 `module="builtin", function="compare"`
/// 即与 gate `compare` 同名）。
pub const BUILTIN_SYMBOL_PREFIX: &str = "builtin.";

/// `compare` op 允许的比较运算符。
const COMPARE_OPS: [&str; 8] = ["eq", "ne", "gt", "gte", "lt", "lte", "in", "not_in"];

/// 符号类别。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SymbolKind {
    /// `python_node` 的 `module.function`。
    PythonNode,
    /// 内置 gate op。
    GateOp,
}

impl SymbolKind {
    /// 文档/错误 message 用的类别名。
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::PythonNode => "python_node",
            Self::GateOp => "gate",
        }
    }
}

/// 内置 gate op。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GateOp {
    /// 读取输出路径并按谓词判定（含 legacy `gate_value` 语义）。
    CheckOutput,
    /// 读取输出路径并按比较运算符判定。
    Compare,
    /// 人工确认占位（M2 kernel 语义；M1 只校验 params）。
    Manual,
}

impl GateOp {
    /// 从 DSL 名称解析（仅接受 3 个内置 op 名）。
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "check_output" => Some(Self::CheckOutput),
            "compare" => Some(Self::Compare),
            "manual" => Some(Self::Manual),
            _ => None,
        }
    }

    /// DSL 名称。
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::CheckOutput => "check_output",
            Self::Compare => "compare",
            Self::Manual => "manual",
        }
    }

    /// `params` 契约的人类可读签名（`DSL_SIGNATURE_MISMATCH` 的 expected）。
    pub const fn signature(self) -> &'static str {
        match self {
            Self::CheckOutput => "params {path, exactly-one-of equals|in|exists}",
            Self::Compare => "params {path, op, value}",
            Self::Manual => "params {prompt?, default?}",
        }
    }
}

/// 是否为内置 gate op 名（供 CLI/测试使用）。
pub fn is_builtin_gate_op(name: &str) -> bool {
    GateOp::from_name(name).is_some()
}

/// 内置 gate op 的 canonical 符号名（如 `builtin.compare`）。
pub fn canonical_gate_symbol(name: &str) -> Option<String> {
    GateOp::from_name(name).map(|op| format!("{BUILTIN_SYMBOL_PREFIX}{}", op.as_str()))
}

/// 校验 python_node 的 `module`/`function` 格式并返回全限定符号。
///
/// 格式（02 §2.5.1）：`module` 为点分 Python 标识符，`function` 为标识符。
pub fn python_node_symbol(module: &str, function: &str) -> Result<String, String> {
    if !is_dotted_identifier(module) {
        return Err(format!(
            "module '{module}' is not a dotted Python identifier"
        ));
    }
    if !is_identifier(function) {
        return Err(format!("function '{function}' is not a Python identifier"));
    }
    Ok(format!("{module}.{function}"))
}

fn is_identifier(text: &str) -> bool {
    let mut chars = text.chars();
    match chars.next() {
        Some(first) if first == '_' || first.is_ascii_alphabetic() => {}
        _ => return false,
    }
    chars.all(|ch| ch == '_' || ch.is_ascii_alphanumeric())
}

fn is_dotted_identifier(text: &str) -> bool {
    !text.is_empty() && text.split('.').all(is_identifier)
}

/// 单次注册的符号条目（用于冲突定位）。
#[derive(Debug, Clone)]
struct Registered {
    kind: SymbolKind,
    signature: &'static str,
    location: Location,
}

/// R14b：解析并注册图级符号，返回错误列表（空 = 通过）。
pub(crate) fn resolve_symbols(nodes: &[GraphNode], edges: &[GraphEdge]) -> Vec<DslError> {
    let mut errors = Vec::new();
    let mut table: HashMap<String, Registered> = HashMap::new();
    register_python_symbols(nodes, &mut table, &mut errors);
    register_gate_symbols(edges, &mut table, &mut errors);
    errors
}

fn register_python_symbols(
    nodes: &[GraphNode],
    table: &mut HashMap<String, Registered>,
    errors: &mut Vec<DslError>,
) {
    for node in nodes {
        if node.node_type != NodeType::PythonNode {
            continue;
        }
        let NodeConfig::PythonNode(config) = &node.config else {
            continue;
        };
        match python_node_symbol(&config.module, &config.function) {
            Ok(symbol) => register(
                table,
                &symbol,
                SymbolKind::PythonNode,
                "fn(inputs, ctx) -> dict",
                Location {
                    node_id: Some(node.id.clone()),
                    ..Location::default()
                },
                errors,
            ),
            Err(detail) => errors.push(symbol_not_found_python(node, config, &detail)),
        }
    }
}

fn register_gate_symbols(
    edges: &[GraphEdge],
    table: &mut HashMap<String, Registered>,
    errors: &mut Vec<DslError>,
) {
    for (index, edge) in edges.iter().enumerate() {
        let Some(gate) = &edge.gate else {
            continue;
        };
        let Some(op) = GateOp::from_name(&gate.function) else {
            errors.push(symbol_not_found_gate(index, &gate.function));
            continue;
        };
        let params = gate.params.clone().unwrap_or_else(|| json!({}));
        match validate_gate_params(op, &params) {
            Ok(()) => {
                let symbol = format!("{BUILTIN_SYMBOL_PREFIX}{}", op.as_str());
                register(
                    table,
                    &symbol,
                    SymbolKind::GateOp,
                    op.signature(),
                    Location {
                        edge_index: Some(index),
                        ..Location::default()
                    },
                    errors,
                );
            }
            Err(()) => errors.push(signature_mismatch(index, op, &params)),
        }
    }
}

fn register(
    table: &mut HashMap<String, Registered>,
    symbol: &str,
    kind: SymbolKind,
    signature: &'static str,
    location: Location,
    errors: &mut Vec<DslError>,
) {
    match table.get(symbol) {
        None => {
            table.insert(
                symbol.to_string(),
                Registered {
                    kind,
                    signature,
                    location,
                },
            );
        }
        Some(existing) if existing.kind == kind => {
            // 同一符号同类别重复使用：幂等（02 §2.5.2）。
        }
        Some(existing) => errors.push(symbol_conflict(
            symbol, existing, kind, signature, &location,
        )),
    }
}

fn symbol_not_found_python(node: &GraphNode, config: &PythonNodeConfig, detail: &str) -> DslError {
    let symbol = format!("{}.{}", config.module, config.function);
    DslError::new(
        DslErrorCode::SymbolNotFound,
        format!("symbol '{symbol}' not found: bad_format ({detail})"),
    )
    .at_node(node.id.clone())
    .expected(json!({"symbol": symbol, "kind": "python_node", "format": "module.function"}))
    .actual(json!({"symbol": symbol, "module": config.module, "function": config.function}))
}

fn symbol_not_found_gate(edge_index: usize, name: &str) -> DslError {
    DslError::new(
        DslErrorCode::SymbolNotFound,
        format!(
            "symbol '{name}' not found: unknown built-in gate op (allowed: {:?})",
            BUILTIN_GATE_OPS
        ),
    )
    .at_edge(edge_index)
    .expected(json!({
        "symbol": name,
        "kind": "gate",
        "allowed": BUILTIN_GATE_OPS,
    }))
    .actual(json!({"symbol": name, "kind": "gate"}))
}

fn signature_mismatch(edge_index: usize, op: GateOp, params: &Value) -> DslError {
    DslError::new(
        DslErrorCode::SignatureMismatch,
        format!(
            "signature mismatch for '{}': expected {}, actual params {}",
            op.as_str(),
            op.signature(),
            params
        ),
    )
    .at_edge(edge_index)
    .expected(json!({
        "symbol": op.as_str(),
        "kind": "gate",
        "expected": op.signature(),
    }))
    .actual(json!({"symbol": op.as_str(), "kind": "gate", "actual": params}))
}

fn symbol_conflict(
    symbol: &str,
    existing: &Registered,
    requested_kind: SymbolKind,
    requested_signature: &'static str,
    requested_location: &Location,
) -> DslError {
    let mut error = DslError::new(
        DslErrorCode::SymbolConflict,
        format!(
            "symbol conflict for '{symbol}': {} requires {}, {} requires {}",
            existing.kind.as_str(),
            existing.signature,
            requested_kind.as_str(),
            requested_signature
        ),
    )
    .expected(json!({
        "symbol": symbol,
        "existing_kind": existing.kind.as_str(),
        "existing_signature": existing.signature,
        "existing_location": existing.location,
    }))
    .actual(json!({
        "symbol": symbol,
        "requested_kind": requested_kind.as_str(),
        "requested_signature": requested_signature,
        "requested_location": requested_location,
    }));
    if let Some(node_id) = &requested_location.node_id {
        error = error.at_node(node_id.clone());
    }
    if let Some(edge_index) = requested_location.edge_index {
        error = error.at_edge(edge_index);
    }
    error
}

/// 内置 gate op 的 `params` 契约校验（M1 的"签名"）。
///
/// 失败时统一由调用方报 `DSL_SIGNATURE_MISMATCH`（expected = op 签名，actual = params）。
fn validate_gate_params(op: GateOp, params: &Value) -> Result<(), ()> {
    let Some(params) = params.as_object() else {
        return Err(());
    };
    match op {
        GateOp::CheckOutput => validate_check_output(params),
        GateOp::Compare => validate_compare(params),
        GateOp::Manual => validate_manual(params),
    }
}

fn validate_check_output(params: &serde_json::Map<String, Value>) -> Result<(), ()> {
    reject_unknown_params(params, &["path", "equals", "in", "exists"])?;
    validate_path(params.get("path"))?;
    let predicates = ["equals", "in", "exists"]
        .iter()
        .filter(|key| params.contains_key(**key))
        .count();
    if predicates != 1 {
        return Err(());
    }
    if let Some(exists) = params.get("exists") {
        if !exists.is_boolean() {
            return Err(());
        }
    }
    if let Some(candidates) = params.get("in") {
        if !candidates.is_array() {
            return Err(());
        }
    }
    Ok(())
}

fn validate_compare(params: &serde_json::Map<String, Value>) -> Result<(), ()> {
    reject_unknown_params(params, &["path", "op", "value"])?;
    validate_path(params.get("path"))?;
    let op = params.get("op").and_then(Value::as_str).ok_or(())?;
    if !COMPARE_OPS.contains(&op) {
        return Err(());
    }
    if !params.contains_key("value") {
        return Err(());
    }
    Ok(())
}

fn validate_manual(params: &serde_json::Map<String, Value>) -> Result<(), ()> {
    reject_unknown_params(params, &["prompt", "default"])?;
    if let Some(prompt) = params.get("prompt") {
        match prompt.as_str() {
            Some(text) if !text.is_empty() => {}
            _ => return Err(()),
        }
    }
    if let Some(default) = params.get("default") {
        if !default.is_boolean() {
            return Err(());
        }
    }
    Ok(())
}

fn reject_unknown_params(
    params: &serde_json::Map<String, Value>,
    allowed: &[&str],
) -> Result<(), ()> {
    if params.keys().any(|key| !allowed.contains(&key.as_str())) {
        return Err(());
    }
    Ok(())
}

fn validate_path(path: Option<&Value>) -> Result<(), ()> {
    let Some(path) = path.and_then(Value::as_str) else {
        return Err(());
    };
    if !is_dotted_identifier(path) {
        return Err(());
    }
    Ok(())
}
