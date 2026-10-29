//! 内置 gate op 的求值语义（M2 定义；D6：**不执行 Python**）。
//!
//! 语义基准：`doc/architecture-v2.md` §4/§8 **D6**（v1 门控 = Rust 内置 op）与
//! `doc/spec-v1.2/00-总纲.md` §0.6 **I7**（门控纯函数：异常/类型错误一律"不通过"，
//! 只拦截边，**不使 run Failed**）。M1 只校验 `params` 契约，M2 在此定义求值语义。
//!
//! # 通用规则
//!
//! - `path` 是点分标识符（M1 已校验）；求值时**只在 JSON object 上逐级取键**。
//!   缺失、或中间节点不是 object，统一视为"**路径不存在**"。
//! - 路径不存在时：`equals`/`in` → `passed=false`、`error=None`
//!   （这是正常的图分支，不是错误）；`exists` 按 `exists` 参数判定
//!   （`exists:true` → false；`exists:false` → true）。
//! - 任何求值异常（参数形状不对、比较类型不匹配）→ `passed=false` + `error` 描述，
//!   **绝不**向上抛错或使 run Failed（I7）。
//! - `GateEvaluation.error` 会进入 `GateResultRecord.error` 供 trace/severity 使用。
//!
//! # 各 op 语义
//!
//! ## `check_output`
//!
//! `params` 恰有 `path` + `equals|in|exists` 之一（M1 签名）：
//!
//! - `equals`：路径值与 `params.equals` **deep-equal** → 通过。
//! - `in`：路径值 deep-equal `params.in` 数组中**任一元素** → 通过。
//! - `exists`：`exists:true` 时路径存在（**含值为 `null`**）→ 通过；
//!   `exists:false` 时路径不存在 → 通过。
//!
//! ## `compare`
//!
//! `params = {path, op, value}`：
//!
//! - `eq`/`ne`：deep-equal（`ne` 为取反）。
//! - `gt`/`gte`/`lt`/`lte`：**仅当双方都是 JSON number（按 f64 比较）**；
//!   否则 `passed=false` + `error="compare: <类型不匹配描述>"`。
//! - `in`/`not_in`：路径值是否 deep-equal `value` 数组的某个元素；
//!   `value` 非数组 → `error`。
//! - 路径不存在 → `passed=false`、`error=None`（参数形状问题仍报 `error`）。
//!
//! ## `manual`
//!
//! v1 无人工介入（00 §0.3 不产生 `WaitingHuman`）：返回 `params.default`
//! （bool，缺省 `false`），`passed` 即该值。这样"能加载、默认拦截/放行"可被
//! 显式配置，而不是静默失败。

use serde_json::Value;
use zgraph_dsl::GateOp;

use crate::types::{CompileError, EdgeIdx};

/// 编译后的门控：内置 op + 原始 `params`（`{}` 若 DSL 未写）。
#[derive(Debug, Clone)]
pub struct CompiledGate {
    /// DSL 中写的 op 名（`check_output` / `compare` / `manual`）。
    pub function: String,
    /// 解析后的内置 op。
    pub op: GateOp,
    /// DSL 原始 `params`（缺省 `{}`）；求值只读。
    pub params: Value,
}

impl CompiledGate {
    /// 编译一条边上的门控；`function` 必须是内置 op（DSL R14b 已保证）。
    pub fn compile(function: &str, params: Value) -> Result<Self, CompileError> {
        let Some(op) = GateOp::from_name(function) else {
            return Err(CompileError::new(format!(
                "unknown built-in gate op '{function}'"
            )));
        };
        Ok(Self {
            function: function.to_string(),
            op,
            params,
        })
    }
}

/// 单条出边的 gate 求值结果（01 §1.7；随 `NodeCompleted` 回传，事件循环只应用）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GateEvaluation {
    /// 被求值的边（`CompiledGraph.edges` 下标）。
    pub edge_index: EdgeIdx,
    /// 是否放行（唯一路由判据）。
    pub passed: bool,
    /// 求值异常描述（类型错误/参数错误时 Some；合法 False 返回为 None）。
    pub error: Option<String>,
}

/// 求值门控（无 `EdgeIndex` 上下文的便捷入口）。
///
/// **注意**：返回值的 [`GateEvaluation::edge_index`] 固定为 `0`，
/// 因为门控本身不携带边信息。发布路径请使用 [`evaluate_gate_for`] 传入真实边号。
pub fn evaluate_gate(gate: &CompiledGate, output: &Value) -> GateEvaluation {
    evaluate_gate_for(0, gate, output)
}

/// 求值指定边上的门控（发布路径使用；`edge_index` 原样写入结果）。
///
/// 纯函数：只读 `gate.params` 与 `output`，不发事件、不改数据（I7）。
pub fn evaluate_gate_for(
    edge_index: EdgeIdx,
    gate: &CompiledGate,
    output: &Value,
) -> GateEvaluation {
    let (passed, error) = match gate.op {
        GateOp::CheckOutput => evaluate_check_output(gate, output),
        GateOp::Compare => evaluate_compare(gate, output),
        GateOp::Manual => evaluate_manual(gate),
    };
    GateEvaluation {
        edge_index,
        passed,
        error,
    }
}

/// `check_output`：路径存在性/等值/枚举谓词。
fn evaluate_check_output(gate: &CompiledGate, output: &Value) -> (bool, Option<String>) {
    let Some(params) = gate.params.as_object() else {
        return invalid("check_output", "params must be a JSON object");
    };
    let Some(path) = params.get("path").and_then(Value::as_str) else {
        return invalid("check_output", "params.path must be a string");
    };
    let resolved = resolve_path(output, path);
    if let Some(equals) = params.get("equals") {
        return (resolved.is_some_and(|actual| actual == equals), None);
    }
    if let Some(candidates) = params.get("in") {
        let Some(array) = candidates.as_array() else {
            return invalid("check_output", "params.in must be an array");
        };
        let passed = resolved.is_some_and(|actual| array.iter().any(|item| item == actual));
        return (passed, None);
    }
    if let Some(exists) = params.get("exists") {
        let Some(expected) = exists.as_bool() else {
            return invalid("check_output", "params.exists must be a bool");
        };
        return (resolved.is_some() == expected, None);
    }
    invalid(
        "check_output",
        "params must contain exactly one of equals|in|exists",
    )
}

/// `compare`：路径值与 `params.value` 按运算符比较。
fn evaluate_compare(gate: &CompiledGate, output: &Value) -> (bool, Option<String>) {
    let Some(params) = gate.params.as_object() else {
        return invalid("compare", "params must be a JSON object");
    };
    let Some(path) = params.get("path").and_then(Value::as_str) else {
        return invalid("compare", "params.path must be a string");
    };
    let Some(op) = params.get("op").and_then(Value::as_str) else {
        return invalid("compare", "params.op must be a string");
    };
    let Some(expected) = params.get("value") else {
        return invalid("compare", "params.value is required");
    };
    let resolved = resolve_path(output, path);
    match op {
        "eq" => (resolved.is_some_and(|actual| actual == expected), None),
        "ne" => (resolved.is_some_and(|actual| actual != expected), None),
        "in" | "not_in" => compare_membership(resolved, expected, op),
        "gt" | "gte" | "lt" | "lte" => compare_order(resolved, expected, op),
        _ => invalid("compare", format!("unknown op '{op}'")),
    }
}

/// `in`/`not_in`：路径值是否属于 `value` 数组。
fn compare_membership(
    resolved: Option<&Value>,
    expected: &Value,
    op: &str,
) -> (bool, Option<String>) {
    let Some(array) = expected.as_array() else {
        return invalid(
            "compare",
            format!("op '{op}' requires params.value to be an array"),
        );
    };
    let Some(actual) = resolved else {
        // 路径不存在：正常的"不匹配"，不是求值异常（规格：error=None）。
        return (false, None);
    };
    let contains = array.iter().any(|item| item == actual);
    if op == "not_in" {
        return (!contains, None);
    }
    (contains, None)
}

/// `gt`/`gte`/`lt`/`lte`：仅接受 JSON number（f64 比较）。
fn compare_order(resolved: Option<&Value>, expected: &Value, op: &str) -> (bool, Option<String>) {
    let Some(expected_number) = expected.as_f64() else {
        return invalid(
            "compare",
            format!(
                "op '{op}' requires numeric operands (params.value got {})",
                value_kind(expected)
            ),
        );
    };
    let Some(actual) = resolved else {
        // 路径不存在：正常的"不匹配"，不是求值异常（规格：error=None）。
        return (false, None);
    };
    let Some(actual_number) = actual.as_f64() else {
        return invalid(
            "compare",
            format!(
                "op '{op}' requires numeric operands (path got {})",
                value_kind(actual)
            ),
        );
    };
    let passed = match op {
        "gt" => actual_number > expected_number,
        "gte" => actual_number >= expected_number,
        "lt" => actual_number < expected_number,
        _ => actual_number <= expected_number,
    };
    (passed, None)
}

/// `manual`：v1 无人工介入，返回 `params.default`（缺省 false）。
fn evaluate_manual(gate: &CompiledGate) -> (bool, Option<String>) {
    let Some(params) = gate.params.as_object() else {
        return invalid("manual", "params must be a JSON object");
    };
    match params.get("default") {
        None => (false, None),
        Some(value) => match value.as_bool() {
            Some(default) => (default, None),
            None => invalid("manual", "params.default must be a bool"),
        },
    }
}

/// 按点分路径在 JSON object 上逐级取键；任一中间节点非 object 视为不存在。
fn resolve_path<'a>(output: &'a Value, path: &str) -> Option<&'a Value> {
    let mut current = output;
    for segment in path.split('.') {
        let object = current.as_object()?;
        current = object.get(segment)?;
    }
    Some(current)
}

/// JSON 值的类型名（错误描述用）。
fn value_kind(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

/// 统一构造"求值异常"结果：`passed=false` + 带 op 前缀的 error。
fn invalid(op: &str, detail: impl AsRef<str>) -> (bool, Option<String>) {
    (false, Some(format!("{op}: {}", detail.as_ref())))
}
