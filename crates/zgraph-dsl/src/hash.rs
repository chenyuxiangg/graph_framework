//! `graph_hash`：canonical JSON 投影 + BLAKE3 前 16 hex。
//!
//! 契约来源：`doc/spec-v1.2/02-DSL与校验.md` §2.8（六步）。要点：
//! - 白名单投影：只保留 `version / metadata.name / entry_points / nodes / edges / subgraphs`
//!   及节点/端口/边/子图的语义字段；`metadata.description/author/created_at` 不参与。
//! - 对象键按 UTF-8 字节序递归排序；数组保持 DSL 顺序；不补齐默认值；无空白。
//! - `blake3(canonical_utf8)` 的十六进制摘要取**前 16 个字符**（8 字节，小写）。

use serde_json::{Map, Value};

use crate::types::{NodeType, DSL_SCHEMA_VERSION};

/// 图指纹长度（00 §0.2：BLAKE3 规范化 JSON 前 16 hex）。
pub const GRAPH_HASH_LEN: usize = 16;

/// 计算 `graph_hash`。
///
/// 调用前置：图已通过 R1–R13、R15（loader 内部保证）；本函数不会失败。
pub fn graph_hash(raw: &Value) -> String {
    let canonical = canonical_projection(raw);
    let digest = blake3::hash(canonical_json(&canonical).as_bytes())
        .to_hex()
        .to_string();
    digest[..GRAPH_HASH_LEN].to_string()
}

/// 返回 canonical JSON 文本（UTF-8，无空白，键排序）。
///
/// 公开导出用于测试与审计；正常流程走 [`graph_hash`]。
pub fn canonical_json(value: &Value) -> String {
    let mut out = String::new();
    write_canonical(value, &mut out);
    out
}

/// 构造参与 hash 的 canonical 投影（02 §2.8.3）。
pub fn canonical_projection(raw: &Value) -> Value {
    let mut top = Map::new();
    top.insert(
        "version".to_string(),
        Value::String(
            raw.get("version")
                .and_then(Value::as_str)
                .unwrap_or(DSL_SCHEMA_VERSION)
                .to_string(),
        ),
    );
    top.insert("metadata".to_string(), project_metadata(raw));
    top.insert(
        "entry_points".to_string(),
        clone_or_default(raw, "entry_points"),
    );
    top.insert("nodes".to_string(), project_nodes(raw));
    top.insert("edges".to_string(), project_edges(raw));
    if let Some(subgraphs) = raw.get("subgraphs").and_then(Value::as_array) {
        top.insert(
            "subgraphs".to_string(),
            Value::Array(subgraphs.iter().map(project_subgraph).collect()),
        );
    }
    Value::Object(top)
}

fn clone_or_default(raw: &Value, key: &str) -> Value {
    raw.get(key)
        .cloned()
        .unwrap_or_else(|| Value::Array(Vec::new()))
}

fn project_metadata(raw: &Value) -> Value {
    let mut metadata = Map::new();
    if let Some(name) = raw.get("metadata").and_then(|m| m.get("name")) {
        metadata.insert("name".to_string(), name.clone());
    }
    Value::Object(metadata)
}

fn project_nodes(raw: &Value) -> Value {
    let nodes = raw.get("nodes").and_then(Value::as_array);
    let projected = nodes
        .map(|nodes| {
            nodes
                .iter()
                .map(|node| {
                    let node_type = node
                        .get("type")
                        .and_then(Value::as_str)
                        .and_then(NodeType::from_name);
                    project_node(node, node_type)
                })
                .collect()
        })
        .unwrap_or_default();
    Value::Array(projected)
}

fn project_node(node: &Value, node_type: Option<NodeType>) -> Value {
    let mut out = Map::new();
    for key in ["id", "type"] {
        if let Some(value) = node.get(key) {
            out.insert(key.to_string(), value.clone());
        }
    }
    if let Some(inputs) = node.get("inputs").and_then(Value::as_array) {
        let ports = inputs
            .iter()
            .map(|port| project_port(port, true))
            .collect::<Vec<_>>();
        out.insert("inputs".to_string(), Value::Array(ports));
    }
    if let Some(trigger) = node.get("trigger") {
        out.insert("trigger".to_string(), trigger.clone());
    }
    out.insert("config".to_string(), project_config(node, node_type));
    if let Some(outputs) = node.get("outputs").and_then(Value::as_array) {
        let ports = outputs
            .iter()
            .map(|port| project_port(port, false))
            .collect::<Vec<_>>();
        out.insert("outputs".to_string(), Value::Array(ports));
    }
    Value::Object(out)
}

fn project_config(node: &Value, node_type: Option<NodeType>) -> Value {
    let Some(config) = node.get("config").and_then(Value::as_object) else {
        return Value::Object(Map::new());
    };
    let allowed: &[&str] = match node_type {
        Some(NodeType::AgentCall) => &[
            "agent_id",
            "input_mapping",
            "max_iterations",
            "timeout_ms",
            "retry",
        ],
        Some(NodeType::PythonNode) => &["module", "function", "exec_class", "timeout_ms", "retry"],
        Some(NodeType::Subgraph) => &["subgraph_id"],
        Some(NodeType::Start) | Some(NodeType::End) | None => &[],
    };
    let mut out = Map::new();
    for key in allowed {
        if let Some(value) = config.get(*key) {
            out.insert((*key).to_string(), value.clone());
        }
    }
    Value::Object(out)
}

fn project_port(port: &Value, with_required: bool) -> Value {
    let mut out = Map::new();
    for key in ["name", "type"] {
        if let Some(value) = port.get(key) {
            out.insert(key.to_string(), value.clone());
        }
    }
    if with_required {
        if let Some(required) = port.get("required") {
            out.insert("required".to_string(), required.clone());
        }
    }
    if let Some(description) = port.get("description") {
        out.insert("description".to_string(), description.clone());
    }
    Value::Object(out)
}

fn project_edges(raw: &Value) -> Value {
    let edges = raw.get("edges").and_then(Value::as_array);
    let projected = edges
        .map(|edges| edges.iter().map(project_edge).collect())
        .unwrap_or_default();
    Value::Array(projected)
}

fn project_edge(edge: &Value) -> Value {
    let mut out = Map::new();
    for key in ["from", "to_node", "to_input_port"] {
        if let Some(value) = edge.get(key) {
            out.insert(key.to_string(), value.clone());
        }
    }
    if let Some(gate) = edge.get("gate").and_then(Value::as_object) {
        let mut projected = Map::new();
        if let Some(function) = gate.get("function") {
            projected.insert("function".to_string(), function.clone());
        }
        if let Some(params) = gate.get("params") {
            projected.insert("params".to_string(), params.clone());
        }
        out.insert("gate".to_string(), Value::Object(projected));
    }
    // v0.2：仅当作者**显式写了** `on_gate_false` 才进投影；缺省 settle 不补齐
    // （02 §2.8.3 "不补齐默认值"，保证 v0.1.0 既有图 hash 不变）。
    if let Some(policy) = edge.get("on_gate_false") {
        out.insert("on_gate_false".to_string(), policy.clone());
    }
    if let Some(port) = edge.get("from_output_port") {
        out.insert("from_output_port".to_string(), port.clone());
    }
    Value::Object(out)
}

fn project_subgraph(subgraph: &Value) -> Value {
    let mut out = Map::new();
    if let Some(id) = subgraph.get("id") {
        out.insert("id".to_string(), id.clone());
    }
    if let Some(nodes) = subgraph.get("nodes").and_then(Value::as_array) {
        let nodes = nodes
            .iter()
            .map(|node| {
                let node_type = node
                    .get("type")
                    .and_then(Value::as_str)
                    .and_then(NodeType::from_name);
                project_node(node, node_type)
            })
            .collect::<Vec<_>>();
        out.insert("nodes".to_string(), Value::Array(nodes));
    }
    if let Some(edges) = subgraph.get("edges").and_then(Value::as_array) {
        let edges = edges.iter().map(project_edge).collect::<Vec<_>>();
        out.insert("edges".to_string(), Value::Array(edges));
    }
    for key in ["input_mapping", "output_mapping"] {
        if let Some(mapping) = subgraph.get(key) {
            out.insert(key.to_string(), mapping.clone());
        }
    }
    Value::Object(out)
}

fn write_canonical(value: &Value, out: &mut String) {
    match value {
        Value::Null => out.push_str("null"),
        Value::Bool(true) => out.push_str("true"),
        Value::Bool(false) => out.push_str("false"),
        Value::Number(number) => out.push_str(&canonical_number(number)),
        Value::String(text) => {
            let escaped = serde_json::to_string(text).expect("string serialization cannot fail");
            out.push_str(&escaped);
        }
        Value::Array(items) => {
            out.push('[');
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                write_canonical(item, out);
            }
            out.push(']');
        }
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort_unstable();
            out.push('{');
            for (index, key) in keys.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                write_canonical(&Value::String((*key).clone()), out);
                out.push(':');
                if let Some(item) = map.get(*key) {
                    write_canonical(item, out);
                }
            }
            out.push('}');
        }
    }
}

fn canonical_number(number: &serde_json::Number) -> String {
    if let Some(value) = number.as_u64() {
        return value.to_string();
    }
    if let Some(value) = number.as_i64() {
        return value.to_string();
    }
    let float = number.as_f64().unwrap_or(0.0);
    if float == 0.0 {
        return "0".to_string();
    }
    // serde_json 以最短往返（ryu）写出浮点，符合 02 §2.8.2 步 4 的 RFC 8785 口径。
    number.to_string()
}
