//! R4–R7：节点声明、节点 ID 唯一、子图定义与引用、入口点校验。
//!
//! 契约来源：`doc/spec-v1.2/02-DSL与校验.md` §2.4.1 R4–R7 与 §2.2.4 键白名单。

use std::collections::{BTreeMap, BTreeSet, HashSet};

use serde_json::{Map, Value};

use crate::edges;
use crate::error::{msg, DslError, DslErrorCode};
use crate::shape::{
    check_keys, invalid_port, invalid_value, is_node_id, is_rfc3339_utc, optional_str,
    required_array, required_nonempty_str, required_str, subgraph_invalid, wrap_subgraph_errors,
};
use crate::types::{
    AgentCallConfig, ExecClass, GraphEdge, GraphMetadata, GraphNode, InputPort, NodeConfig,
    NodeType, OutputPort, PortType, PythonNodeConfig, RetryConfig, SubgraphConfig,
    SubgraphDefinition, Trigger, DSL_SCHEMA_VERSION,
};

const TOP_LEVEL_KEYS: [&str; 6] = [
    "version",
    "metadata",
    "entry_points",
    "nodes",
    "edges",
    "subgraphs",
];
const TOP_LEVEL_REQUIRED: [&str; 5] = ["version", "metadata", "entry_points", "nodes", "edges"];
const METADATA_KEYS: [&str; 4] = ["name", "description", "author", "created_at"];
const NODE_KEYS: [&str; 6] = ["id", "type", "inputs", "trigger", "config", "outputs"];
const INPUT_PORT_KEYS: [&str; 4] = ["name", "type", "required", "description"];
const OUTPUT_PORT_KEYS: [&str; 3] = ["name", "type", "description"];
const AGENT_CONFIG_KEYS: [&str; 5] = [
    "agent_id",
    "input_mapping",
    "max_iterations",
    "timeout_ms",
    "retry",
];
const PYTHON_CONFIG_KEYS: [&str; 5] = ["module", "function", "exec_class", "timeout_ms", "retry"];
const SUBGRAPH_CONFIG_KEYS: [&str; 1] = ["subgraph_id"];
const RETRY_KEYS: [&str; 1] = ["max_attempts"];
const SUBGRAPH_KEYS: [&str; 5] = ["id", "nodes", "edges", "input_mapping", "output_mapping"];

/// R2：顶层必填/类型/未知键。
pub(crate) fn check_top_level(raw: &Value) -> Vec<DslError> {
    let mut errors = Vec::new();
    let Some(map) = raw.as_object() else {
        errors.push(msg::wrong_type("top level", "object", "graph", raw));
        return errors;
    };
    check_keys(map, &TOP_LEVEL_KEYS, "top level", &mut errors);
    for key in TOP_LEVEL_REQUIRED {
        if !map.contains_key(key) {
            errors.push(msg::missing_field(key, "top level"));
        }
    }
    check_top_level_types(map, &mut errors);
    errors
}

fn check_top_level_types(map: &Map<String, Value>, errors: &mut Vec<DslError>) {
    if let Some(version) = map.get("version") {
        if !version.is_string() {
            errors.push(msg::wrong_type("version", "string", "top level", version));
        }
    }
    if let Some(metadata) = map.get("metadata") {
        if !metadata.is_object() {
            errors.push(msg::wrong_type("metadata", "object", "top level", metadata));
        }
    }
    check_string_array(map, "entry_points", true, errors);
    check_array_nonempty(map, "nodes", errors);
    check_array(map, "edges", errors);
    if let Some(subgraphs) = map.get("subgraphs") {
        if !subgraphs.is_array() {
            errors.push(msg::wrong_type(
                "subgraphs",
                "array",
                "top level",
                subgraphs,
            ));
        }
    }
}

fn check_string_array(
    map: &Map<String, Value>,
    key: &str,
    non_empty: bool,
    errors: &mut Vec<DslError>,
) {
    let Some(value) = map.get(key) else {
        return;
    };
    let Some(items) = value.as_array() else {
        errors.push(msg::wrong_type(key, "array", "top level", value));
        return;
    };
    if non_empty && items.is_empty() {
        errors.push(msg::empty_array(key, "top level"));
    }
    for (index, item) in items.iter().enumerate() {
        if !item.is_string() {
            let field = format!("{key}[{index}]");
            errors.push(msg::wrong_type(&field, "string", "top level", item));
        }
    }
}

fn check_array(map: &Map<String, Value>, key: &str, errors: &mut Vec<DslError>) {
    let Some(value) = map.get(key) else {
        return;
    };
    if !value.is_array() {
        errors.push(msg::wrong_type(key, "array", "top level", value));
    }
}

fn check_array_nonempty(map: &Map<String, Value>, key: &str, errors: &mut Vec<DslError>) {
    let Some(value) = map.get(key) else {
        return;
    };
    if !value.is_array() {
        errors.push(msg::wrong_type(key, "array", "top level", value));
        return;
    }
    if value.as_array().is_some_and(Vec::is_empty) {
        errors.push(msg::empty_array(key, "top level"));
    }
}

/// R3：`version` 兼容性与 `metadata` 内容。
pub(crate) fn check_version_and_metadata(raw: &Value) -> Result<GraphMetadata, Vec<DslError>> {
    let mut errors = Vec::new();
    let Some(map) = raw.as_object() else {
        return Err(vec![msg::wrong_type("top level", "object", "graph", raw)]);
    };
    let version = map
        .get("version")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if version != DSL_SCHEMA_VERSION {
        errors.push(
            DslError::new(
                DslErrorCode::ParseError,
                format!("unsupported DSL version '{version}': expected '1.0'"),
            )
            .expected(DSL_SCHEMA_VERSION)
            .actual(version),
        );
    }
    let metadata = parse_metadata(map.get("metadata"), &mut errors);
    if errors.is_empty() {
        metadata.ok_or_else(|| vec![msg::missing_field("metadata", "top level")])
    } else {
        Err(errors)
    }
}

fn parse_metadata(value: Option<&Value>, errors: &mut Vec<DslError>) -> Option<GraphMetadata> {
    let value = value?;
    let Some(map) = value.as_object() else {
        errors.push(msg::wrong_type("metadata", "object", "top level", value));
        return None;
    };
    let start = errors.len();
    for key in map.keys() {
        if !METADATA_KEYS.contains(&key.as_str()) {
            errors.push(msg::unknown_field(key, "metadata"));
        }
    }
    let name = parse_metadata_name(map, errors);
    let description = optional_str(map, "description", "metadata", errors).map(str::to_string);
    let author = optional_str(map, "author", "metadata", errors).map(str::to_string);
    let created_at = parse_created_at(map, errors);
    if errors.len() > start {
        return None;
    }
    Some(GraphMetadata {
        name: name?,
        description,
        author,
        created_at,
    })
}

fn parse_metadata_name(map: &Map<String, Value>, errors: &mut Vec<DslError>) -> Option<String> {
    match map.get("name") {
        None => {
            errors.push(msg::missing_field("name", "metadata"));
            None
        }
        Some(Value::String(name)) if !name.is_empty() => Some(name.clone()),
        Some(Value::String(_)) => {
            errors.push(msg::missing_field("name", "metadata"));
            None
        }
        Some(other) => {
            errors.push(msg::wrong_type("name", "string", "metadata", other));
            None
        }
    }
}

fn parse_created_at(map: &Map<String, Value>, errors: &mut Vec<DslError>) -> Option<String> {
    let text = optional_str(map, "created_at", "metadata", errors)?;
    if !is_rfc3339_utc(text) {
        errors.push(invalid_value(
            "created_at",
            "a UTC RFC3339 timestamp",
            "metadata",
            &Value::String(text.to_string()),
        ));
        return None;
    }
    Some(text.to_string())
}

/// R4：解析并校验节点声明（失败时返回本规则聚合到的全部错误）。
pub(crate) fn parse_nodes(value: &Value) -> Result<Vec<GraphNode>, Vec<DslError>> {
    let mut errors = Vec::new();
    let Some(items) = value.as_array() else {
        return Err(vec![msg::wrong_type("nodes", "array", "top level", value)]);
    };
    let mut nodes = Vec::new();
    for (index, item) in items.iter().enumerate() {
        if let Some(node) = parse_node(item, index, &mut errors) {
            nodes.push(node);
        }
    }
    if errors.is_empty() {
        Ok(nodes)
    } else {
        Err(errors)
    }
}

fn parse_node(value: &Value, index: usize, errors: &mut Vec<DslError>) -> Option<GraphNode> {
    let start = errors.len();
    let fallback_scope = format!("nodes[{index}]");
    let Some(map) = value.as_object() else {
        errors.push(msg::wrong_type(&fallback_scope, "object", "nodes", value));
        return None;
    };
    let node_id = parse_node_id(map, &fallback_scope, errors);
    let scope = node_id
        .as_ref()
        .map_or(fallback_scope, |id| format!("node '{id}'"));
    check_node_keys(map, node_id.as_deref(), &scope, errors);
    let Some(node_type) = parse_node_type(map, node_id.as_deref(), &scope, errors) else {
        attach_node_location(errors, start, node_id.as_deref());
        return None;
    };
    let inputs = parse_inputs(map, node_id.as_deref(), &scope, errors);
    let trigger = parse_trigger(map, node_type, &scope, errors);
    check_end_constraints(
        node_type,
        inputs.as_deref(),
        node_id.as_deref(),
        &scope,
        errors,
    );
    let outputs = parse_outputs(map, node_id.as_deref(), &scope, errors);
    let config = parse_node_config(map, node_type, &scope, errors);
    attach_node_location(errors, start, node_id.as_deref());
    if errors.len() > start {
        return None;
    }
    Some(GraphNode {
        id: node_id?,
        node_type,
        inputs: inputs?,
        trigger,
        config: config?,
        outputs: outputs.unwrap_or_default(),
    })
}

fn parse_node_id(
    map: &Map<String, Value>,
    fallback_scope: &str,
    errors: &mut Vec<DslError>,
) -> Option<String> {
    match map.get("id") {
        None => {
            errors.push(msg::missing_field("id", fallback_scope));
            None
        }
        Some(Value::String(id)) if is_node_id(id) => Some(id.clone()),
        Some(Value::String(id)) => {
            errors.push(invalid_value(
                "id",
                "1-64 chars of [A-Za-z0-9_.-]",
                fallback_scope,
                &Value::String(id.clone()),
            ));
            None
        }
        Some(other) => {
            errors.push(msg::wrong_type("id", "string", fallback_scope, other));
            None
        }
    }
}

/// 节点级键白名单：`on_error` 是唯一保留键（→ `DSL_UNSUPPORTED_FIELD`），
/// 其余未知键 → `DSL_PARSE_ERROR`（02 §2.2.4，REV-005 #1）。
fn check_node_keys(
    map: &Map<String, Value>,
    node_id: Option<&str>,
    scope: &str,
    errors: &mut Vec<DslError>,
) {
    for key in map.keys() {
        let mut error = if key == "on_error" {
            let value = map.get("on_error").cloned().unwrap_or(Value::Null);
            msg::unsupported_field("on_error", &value)
        } else if NODE_KEYS.contains(&key.as_str()) {
            continue;
        } else {
            msg::unknown_field(key, scope)
        };
        if let Some(node_id) = node_id {
            error = error.at_node(node_id.to_string());
        }
        errors.push(error);
    }
}

fn parse_node_type(
    map: &Map<String, Value>,
    node_id: Option<&str>,
    scope: &str,
    errors: &mut Vec<DslError>,
) -> Option<NodeType> {
    let raw = required_str(map, "type", scope, errors)?;
    match NodeType::from_name(raw) {
        Some(node_type) => Some(node_type),
        None => {
            let mut error = DslError::new(
                DslErrorCode::UnsupportedNodeType,
                format!(
                    "node type '{raw}' is not a known node type; allowed: [start, end, agent_call, python_node, subgraph]"
                ),
            )
            .expected(json_allowed_types())
            .actual(raw);
            if let Some(node_id) = node_id {
                error = error.at_node(node_id.to_string());
            }
            errors.push(error);
            None
        }
    }
}

fn json_allowed_types() -> Value {
    Value::Array(
        NodeType::ALL
            .iter()
            .map(|kind| Value::String(kind.as_str().to_string()))
            .collect(),
    )
}

fn parse_trigger(
    map: &Map<String, Value>,
    node_type: NodeType,
    scope: &str,
    errors: &mut Vec<DslError>,
) -> Trigger {
    let Some(raw) = optional_str(map, "trigger", scope, errors) else {
        return Trigger::All;
    };
    let Some(trigger) = Trigger::from_name(raw) else {
        errors.push(invalid_value(
            "trigger",
            "\"all\" or \"any\"",
            scope,
            &Value::String(raw.to_string()),
        ));
        return Trigger::All;
    };
    let needs_all = matches!(node_type, NodeType::Start | NodeType::End);
    if needs_all && trigger != Trigger::All {
        errors.push(invalid_value(
            "trigger",
            "the literal \"all\" for start/end nodes",
            scope,
            &Value::String(raw.to_string()),
        ));
        return Trigger::All;
    }
    trigger
}

fn check_end_constraints(
    node_type: NodeType,
    inputs: Option<&[InputPort]>,
    node_id: Option<&str>,
    scope: &str,
    errors: &mut Vec<DslError>,
) {
    if node_type != NodeType::End {
        return;
    }
    let ports = inputs.unwrap_or_default();
    let mut push = |message: String, expected: &str| {
        let mut error = invalid_port(message)
            .expected(expected)
            .actual(Value::Array(Vec::new()));
        if let Some(node_id) = node_id {
            error = error.at_node(node_id.to_string());
        }
        errors.push(error);
    };
    if ports.is_empty() {
        push(
            format!("end node {scope} must declare at least one input port"),
            "non-empty inputs",
        );
        return;
    }
    if ports.iter().all(|port| !port.required) {
        push(
            format!("end node {scope} must declare at least one required input port"),
            "at least one required:true port",
        );
    }
}

fn parse_inputs(
    map: &Map<String, Value>,
    node_id: Option<&str>,
    scope: &str,
    errors: &mut Vec<DslError>,
) -> Option<Vec<InputPort>> {
    let value = required_array(map, "inputs", scope, errors)?;
    let start = errors.len();
    let mut ports = Vec::new();
    let mut names = HashSet::new();
    for (index, item) in value.iter().enumerate() {
        if let Some(port) = parse_input_port(item, node_id, index, scope, &mut names, errors) {
            ports.push(port);
        }
    }
    if errors.len() > start {
        None
    } else {
        Some(ports)
    }
}

fn parse_input_port(
    value: &Value,
    node_id: Option<&str>,
    index: usize,
    scope: &str,
    names: &mut HashSet<String>,
    errors: &mut Vec<DslError>,
) -> Option<InputPort> {
    let start = errors.len();
    let fallback = format!("{scope}.inputs[{index}]");
    let Some(map) = value.as_object() else {
        errors.push(msg::wrong_type(&fallback, "object", scope, value));
        return None;
    };
    check_keys(map, &INPUT_PORT_KEYS, &fallback, errors);
    let name = parse_port_name(map, &fallback, node_id, true, names, errors);
    let port_type = parse_port_type(map, &fallback, errors);
    let required = parse_required(map, &fallback, errors);
    let description = optional_str(map, "description", &fallback, errors).map(str::to_string);
    if errors.len() > start {
        return None;
    }
    Some(InputPort {
        name: name?,
        port_type: port_type?,
        required: required?,
        description,
    })
}

fn parse_outputs(
    map: &Map<String, Value>,
    node_id: Option<&str>,
    scope: &str,
    errors: &mut Vec<DslError>,
) -> Option<Vec<OutputPort>> {
    let Some(value) = map.get("outputs") else {
        return Some(Vec::new());
    };
    let Some(items) = value.as_array() else {
        errors.push(msg::wrong_type("outputs", "array", scope, value));
        return None;
    };
    let start = errors.len();
    let mut ports = Vec::new();
    let mut names = HashSet::new();
    for (index, item) in items.iter().enumerate() {
        let fallback = format!("{scope}.outputs[{index}]");
        let Some(raw) = item.as_object() else {
            errors.push(msg::wrong_type(&fallback, "object", scope, item));
            continue;
        };
        check_keys(raw, &OUTPUT_PORT_KEYS, &fallback, errors);
        let name = parse_port_name(raw, &fallback, node_id, false, &mut names, errors);
        let port_type = parse_port_type(raw, &fallback, errors);
        let description = optional_str(raw, "description", &fallback, errors).map(str::to_string);
        if let (Some(name), Some(port_type)) = (name, port_type) {
            ports.push(OutputPort {
                name,
                port_type,
                description,
            });
        }
    }
    if errors.len() > start {
        None
    } else {
        Some(ports)
    }
}

fn parse_port_name(
    map: &Map<String, Value>,
    fallback: &str,
    node_id: Option<&str>,
    record_port: bool,
    names: &mut HashSet<String>,
    errors: &mut Vec<DslError>,
) -> Option<String> {
    let name = required_nonempty_str(map, "name", fallback, errors)?;
    if !crate::shape::is_identifier(name) {
        errors.push(
            invalid_port(format!(
                "port name '{name}' must be a Python identifier (1-64 chars)"
            ))
            .expected("a Python identifier of 1-64 chars")
            .actual(name),
        );
        return None;
    }
    if !names.insert(name.to_string()) {
        let mut error = invalid_port(format!("duplicate port name '{name}' in node"))
            .expected("a unique port name within the node")
            .actual(name);
        if let Some(node_id) = node_id {
            error = error.at_node(node_id.to_string());
        }
        if record_port {
            error = error.at_port(name.to_string());
        }
        errors.push(error);
        return None;
    }
    Some(name.to_string())
}

fn parse_port_type(
    map: &Map<String, Value>,
    fallback: &str,
    errors: &mut Vec<DslError>,
) -> Option<PortType> {
    let raw = required_str(map, "type", fallback, errors)?;
    match PortType::from_name(raw) {
        Some(port_type) => Some(port_type),
        None => {
            errors.push(invalid_value(
                "type",
                "\"string\"|\"number\"|\"boolean\"|\"object\"|\"any\"",
                fallback,
                &Value::String(raw.to_string()),
            ));
            None
        }
    }
}

fn parse_required(
    map: &Map<String, Value>,
    fallback: &str,
    errors: &mut Vec<DslError>,
) -> Option<bool> {
    match map.get("required") {
        None => {
            errors.push(msg::missing_field("required", fallback));
            None
        }
        Some(Value::Bool(required)) => Some(*required),
        Some(other) => {
            errors.push(msg::wrong_type("required", "boolean", fallback, other));
            None
        }
    }
}

fn parse_node_config(
    map: &Map<String, Value>,
    node_type: NodeType,
    scope: &str,
    errors: &mut Vec<DslError>,
) -> Option<NodeConfig> {
    let Some(value) = map.get("config") else {
        errors.push(msg::missing_field("config", scope));
        return None;
    };
    let Some(config) = value.as_object() else {
        errors.push(msg::wrong_type("config", "object", scope, value));
        return None;
    };
    match node_type {
        NodeType::AgentCall => parse_agent_call_config(config, scope, errors),
        NodeType::PythonNode => parse_python_config(config, scope, errors),
        NodeType::Subgraph => parse_subgraph_config(config, scope, errors),
        NodeType::Start | NodeType::End => parse_start_end_config(config, scope, errors),
    }
}

fn parse_start_end_config(
    config: &Map<String, Value>,
    scope: &str,
    errors: &mut Vec<DslError>,
) -> Option<NodeConfig> {
    let start = errors.len();
    for key in config.keys() {
        errors.push(msg::unknown_field(key, scope));
    }
    if errors.len() > start {
        None
    } else {
        Some(NodeConfig::StartEnd)
    }
}

fn parse_agent_call_config(
    config: &Map<String, Value>,
    scope: &str,
    errors: &mut Vec<DslError>,
) -> Option<NodeConfig> {
    let start = errors.len();
    check_keys(config, &AGENT_CONFIG_KEYS, scope, errors);
    let agent_id = required_nonempty_str(config, "agent_id", scope, errors).map(str::to_string);
    let input_mapping =
        parse_string_map(config.get("input_mapping"), "input_mapping", scope, errors);
    let max_iterations = parse_optional_u32(config, "max_iterations", scope, errors, Some((1, 50)));
    let timeout_ms = parse_optional_u64(config, "timeout_ms", scope, errors);
    let retry = parse_retry(config.get("retry"), scope, errors);
    if errors.len() > start {
        return None;
    }
    Some(NodeConfig::AgentCall(AgentCallConfig {
        agent_id: agent_id?,
        input_mapping: input_mapping.unwrap_or_default(),
        max_iterations,
        timeout_ms,
        retry,
    }))
}

fn parse_python_config(
    config: &Map<String, Value>,
    scope: &str,
    errors: &mut Vec<DslError>,
) -> Option<NodeConfig> {
    let start = errors.len();
    check_keys(config, &PYTHON_CONFIG_KEYS, scope, errors);
    let module = required_nonempty_str(config, "module", scope, errors).map(str::to_string);
    let function = required_nonempty_str(config, "function", scope, errors).map(str::to_string);
    let exec_class = parse_exec_class(config.get("exec_class"), scope, errors);
    let timeout_ms = parse_optional_u64(config, "timeout_ms", scope, errors);
    let retry = parse_retry(config.get("retry"), scope, errors);
    if errors.len() > start {
        return None;
    }
    Some(NodeConfig::PythonNode(PythonNodeConfig {
        module: module?,
        function: function?,
        exec_class,
        timeout_ms,
        retry,
    }))
}

fn parse_subgraph_config(
    config: &Map<String, Value>,
    scope: &str,
    errors: &mut Vec<DslError>,
) -> Option<NodeConfig> {
    let start = errors.len();
    check_keys(config, &SUBGRAPH_CONFIG_KEYS, scope, errors);
    let subgraph_id =
        required_nonempty_str(config, "subgraph_id", scope, errors).map(str::to_string);
    if errors.len() > start {
        return None;
    }
    Some(NodeConfig::Subgraph(SubgraphConfig {
        subgraph_id: subgraph_id?,
    }))
}

fn parse_exec_class(
    value: Option<&Value>,
    scope: &str,
    errors: &mut Vec<DslError>,
) -> Option<ExecClass> {
    let value = value?;
    let Some(raw) = value.as_str() else {
        errors.push(msg::wrong_type("exec_class", "string", scope, value));
        return None;
    };
    match ExecClass::from_name(raw) {
        Some(exec_class) => Some(exec_class),
        None => {
            errors.push(invalid_value(
                "exec_class",
                "\"cpu\" or \"io\"",
                scope,
                value,
            ));
            None
        }
    }
}

fn parse_optional_u64(
    map: &Map<String, Value>,
    key: &str,
    scope: &str,
    errors: &mut Vec<DslError>,
) -> Option<u64> {
    let value = map.get(key)?;
    match value.as_u64() {
        Some(number) if number >= 1 => Some(number),
        _ => {
            errors.push(invalid_value(key, "an integer >= 1", scope, value));
            None
        }
    }
}

fn parse_optional_u32(
    map: &Map<String, Value>,
    key: &str,
    scope: &str,
    errors: &mut Vec<DslError>,
    range: Option<(u32, u32)>,
) -> Option<u32> {
    let value = map.get(key)?;
    let Some(number) = value.as_u64().and_then(|number| u32::try_from(number).ok()) else {
        errors.push(invalid_value(key, "an integer >= 1", scope, value));
        return None;
    };
    let out_of_range = range.is_some_and(|(low, high)| number < low || number > high);
    if number == 0 || out_of_range {
        let constraint = range.map_or("an integer >= 1".to_string(), |(low, high)| {
            format!("an integer in {low}..={high}")
        });
        errors.push(invalid_value(key, &constraint, scope, value));
        return None;
    }
    Some(number)
}

fn parse_retry(
    value: Option<&Value>,
    scope: &str,
    errors: &mut Vec<DslError>,
) -> Option<RetryConfig> {
    let value = value?;
    let Some(map) = value.as_object() else {
        errors.push(msg::wrong_type("retry", "object", scope, value));
        return None;
    };
    let start = errors.len();
    check_keys(map, &RETRY_KEYS, scope, errors);
    let max_attempts = parse_optional_u32(map, "max_attempts", scope, errors, None);
    if errors.len() > start {
        return None;
    }
    Some(RetryConfig { max_attempts })
}

fn parse_string_map(
    value: Option<&Value>,
    key: &str,
    scope: &str,
    errors: &mut Vec<DslError>,
) -> Option<BTreeMap<String, String>> {
    let value = value?;
    let Some(map) = value.as_object() else {
        errors.push(msg::wrong_type(key, "object", scope, value));
        return None;
    };
    let start = errors.len();
    let mut mapping = BTreeMap::new();
    for (name, target) in map {
        if name.is_empty() {
            errors.push(invalid_value(key, "non-empty keys", scope, value));
            continue;
        }
        let Some(target) = target.as_str() else {
            errors.push(msg::wrong_type(key, "string values", scope, target));
            continue;
        };
        if target.is_empty() {
            errors.push(invalid_value(
                key,
                "non-empty string values",
                scope,
                &Value::String(String::new()),
            ));
            continue;
        }
        mapping.insert(name.clone(), target.to_string());
    }
    if errors.len() > start {
        None
    } else {
        Some(mapping)
    }
}

/// 把节点作用域内收集的错误补上 `node_id`（02 §2.7.3：节点级错误至少一个定位）。
///
/// 已带 `node_id`/`edge_index` 的错误保持不变；未知节点 id 时保持文档级。
fn attach_node_location(errors: &mut [DslError], start: usize, node_id: Option<&str>) {
    let Some(node_id) = node_id else {
        return;
    };
    for error in &mut errors[start..] {
        if error.location.node_id.is_none() && error.location.edge_index.is_none() {
            error.location.node_id = Some(node_id.to_string());
        }
    }
}

/// R5：节点 ID 唯一（命名空间内）。
pub(crate) fn check_node_id_unique(nodes: &[GraphNode]) -> Vec<DslError> {
    let mut errors = Vec::new();
    let mut seen: BTreeMap<&str, usize> = BTreeMap::new();
    for (index, node) in nodes.iter().enumerate() {
        match seen.get(node.id.as_str()) {
            None => {
                seen.insert(node.id.as_str(), index);
            }
            Some(first) => errors.push(
                DslError::new(
                    DslErrorCode::DuplicateNodeId,
                    format!(
                        "duplicate node id '{}': nodes[{first}] and nodes[{index}]",
                        node.id
                    ),
                )
                .at_node(node.id.clone())
                .expected(*first as u64)
                .actual(index as u64),
            ),
        }
    }
    errors
}

/// R7：入口点有效性。
pub(crate) fn check_entry_points(entries: &[String], nodes: &[GraphNode]) -> Vec<DslError> {
    let mut errors = Vec::new();
    let by_id: BTreeMap<&str, &GraphNode> =
        nodes.iter().map(|node| (node.id.as_str(), node)).collect();
    let mut seen: HashSet<&str> = HashSet::new();
    for entry in entries {
        if !seen.insert(entry.as_str()) {
            errors.push(invalid_entry(entry, "duplicate"));
            continue;
        }
        match by_id.get(entry.as_str()) {
            None => errors.push(invalid_entry(entry, "unknown_node")),
            Some(node) if node.node_type == NodeType::End => {
                errors.push(invalid_entry(entry, "end_node"));
            }
            Some(_) => {}
        }
    }
    for node in nodes {
        if node.node_type == NodeType::Start && !seen.contains(node.id.as_str()) {
            errors.push(invalid_entry(&node.id, "start_not_listed"));
        }
    }
    errors
}

fn invalid_entry(value: &str, reason: &str) -> DslError {
    DslError::new(
        DslErrorCode::InvalidEntry,
        format!("entry point '{value}' is not valid: {reason}"),
    )
    .at_node(value)
    .expected("an existing node id that is not an end node")
    .actual(value)
}

/// R6：子图定义与引用。
///
/// 子图内部的 R4/R5/R8–R13 违规统一收敛为 `DSL_SUBGRAPH_INVALID`（message 前缀
/// `subgraph '<id>'`，定位保留内部节点/边信息）；定义非法同时使子图引用校验失败。
pub(crate) fn check_subgraphs(
    raw: &Value,
    main_nodes: &[GraphNode],
) -> Result<Vec<SubgraphDefinition>, Vec<DslError>> {
    let mut errors = Vec::new();
    let mut definitions = Vec::new();
    let mut ids: BTreeSet<String> = BTreeSet::new();
    if let Some(value) = raw.get("subgraphs") {
        let Some(items) = value.as_array() else {
            return Err(vec![msg::wrong_type(
                "subgraphs",
                "array",
                "top level",
                value,
            )]);
        };
        for (index, item) in items.iter().enumerate() {
            if let Some(definition) = parse_subgraph(item, index, &mut ids, &mut errors) {
                definitions.push(definition);
            }
        }
    }
    check_subgraph_references(main_nodes, &ids, &mut errors);
    if errors.is_empty() {
        Ok(definitions)
    } else {
        Err(errors)
    }
}

fn check_subgraph_references(
    main_nodes: &[GraphNode],
    ids: &BTreeSet<String>,
    errors: &mut Vec<DslError>,
) {
    for node in main_nodes {
        if node.node_type != NodeType::Subgraph {
            continue;
        }
        let NodeConfig::Subgraph(config) = &node.config else {
            continue;
        };
        if !ids.contains(&config.subgraph_id) {
            errors.push(
                subgraph_invalid(
                    &config.subgraph_id,
                    &format!(
                        "referenced by node '{}' but not defined in subgraphs[]",
                        node.id
                    ),
                )
                .at_node(node.id.clone()),
            );
        }
    }
}

/// R6：子图 `id` 必须是非空 `string`（REV-005 #3）。
///
/// 缺失/空 → `DSL_MISSING_FIELD`，非 string → `DSL_PARSE_ERROR`；错误为文档级
/// （location = {}），message 携带 `subgraphs[i]` 路径（02 §2.7.3）。
fn parse_subgraph_id(
    map: &Map<String, Value>,
    index: usize,
    errors: &mut Vec<DslError>,
) -> Option<String> {
    let scope = format!("subgraphs[{index}]");
    match map.get("id") {
        None => {
            errors.push(msg::missing_field("id", &scope));
            None
        }
        Some(Value::String(id)) if !id.is_empty() => Some(id.clone()),
        Some(Value::String(_)) => {
            errors.push(msg::missing_field("id", &scope));
            None
        }
        Some(other) => {
            errors.push(msg::wrong_type("id", "string", &scope, other));
            None
        }
    }
}

fn parse_subgraph(
    value: &Value,
    index: usize,
    ids: &mut BTreeSet<String>,
    errors: &mut Vec<DslError>,
) -> Option<SubgraphDefinition> {
    let start = errors.len();
    let fallback = format!("subgraphs[{index}]");
    let Some(map) = value.as_object() else {
        errors.push(msg::wrong_type(&fallback, "object", "subgraphs", value));
        return None;
    };
    // 先解析 id：无有效 SubgraphId 时只能文档级（ISSUE-031）；有效 id 的头错误
    // 统一 location.node_id = SubgraphId（REV-005 复审 #1）。
    let subgraph_id = parse_subgraph_id(map, index, errors);
    if errors.len() > start {
        return None;
    }
    let subgraph_id = subgraph_id?;
    if !ids.insert(subgraph_id.clone()) {
        errors.push(subgraph_invalid(&subgraph_id, "duplicate subgraph id"));
        return None;
    }
    let scope = format!("subgraph '{subgraph_id}'");
    check_keys(map, &SUBGRAPH_KEYS, &scope, errors);
    for key in ["nodes", "edges", "input_mapping", "output_mapping"] {
        if !map.contains_key(key) {
            errors.push(msg::missing_field(key, &scope));
        }
    }
    let input_mapping = parse_string_map(map.get("input_mapping"), "input_mapping", &scope, errors);
    let output_mapping =
        parse_string_map(map.get("output_mapping"), "output_mapping", &scope, errors);
    attach_node_location(errors, start, Some(&subgraph_id));
    if errors.len() > start {
        return None;
    }
    let body = build_subgraph_body(&subgraph_id, map.get("nodes"), map.get("edges"), errors)?;
    let input_mapping = input_mapping.unwrap_or_default();
    let output_mapping = output_mapping.unwrap_or_default();
    if !validate_subgraph_rules(&subgraph_id, &body, &input_mapping, &output_mapping, errors) {
        return None;
    }
    if errors.len() > start {
        return None;
    }
    Some(SubgraphDefinition {
        id: subgraph_id,
        nodes: body.nodes,
        edges: body.edges,
        input_mapping,
        output_mapping,
    })
}

/// 子图语义校验：形状、节点 ID 唯一、mapping 引用、R8–R13（全部收敛为
/// `DSL_SUBGRAPH_INVALID`）。返回 `false` 表示本子图不可用。
fn validate_subgraph_rules(
    subgraph_id: &str,
    body: &SubgraphBody,
    input_mapping: &BTreeMap<String, String>,
    output_mapping: &BTreeMap<String, String>,
    errors: &mut Vec<DslError>,
) -> bool {
    let start = errors.len();
    check_subgraph_shape(subgraph_id, &body.nodes, errors);
    errors.extend(wrap_subgraph_errors(
        subgraph_id,
        check_node_id_unique(&body.nodes),
    ));
    let entries = validate_subgraph_mappings(
        subgraph_id,
        &body.nodes,
        input_mapping,
        output_mapping,
        errors,
    );
    let entry_refs: HashSet<&str> = entries.iter().map(String::as_str).collect();
    let mut inner = edges::check_edge_endpoints(&body.edges, &body.nodes, &entry_refs);
    inner.extend(edges::check_edge_ports(&body.edges, &body.nodes));
    inner.extend(edges::check_duplicate_edges(&body.edges));
    inner.extend(edges::check_duplicate_port_sources(&body.edges));
    inner.extend(edges::check_input_sources(
        &body.nodes,
        &body.edges,
        &entry_refs,
    ));
    inner.extend(edges::check_cycles(&body.nodes, &body.edges));
    errors.extend(wrap_subgraph_errors(subgraph_id, inner));
    errors.len() == start
}

struct SubgraphBody {
    nodes: Vec<GraphNode>,
    edges: Vec<GraphEdge>,
}

fn build_subgraph_body(
    subgraph_id: &str,
    nodes_value: Option<&Value>,
    edges_value: Option<&Value>,
    errors: &mut Vec<DslError>,
) -> Option<SubgraphBody> {
    let mut inner_errors = Vec::new();
    let nodes = nodes_value.map(parse_nodes);
    let edges = edges_value.map(edges::parse_edges);
    let mut parsed_nodes = Vec::new();
    let mut parsed_edges = Vec::new();
    match nodes {
        Some(Ok(nodes)) => parsed_nodes = nodes,
        Some(Err(errors)) => inner_errors.extend(errors),
        None => inner_errors.push(msg::missing_field("nodes", subgraph_id)),
    }
    match edges {
        Some(Ok(edges)) => parsed_edges = edges,
        Some(Err(errors)) => inner_errors.extend(errors),
        None => inner_errors.push(msg::missing_field("edges", subgraph_id)),
    }
    if !inner_errors.is_empty() {
        errors.extend(wrap_subgraph_errors(subgraph_id, inner_errors));
        return None;
    }
    if parsed_nodes.is_empty() {
        errors.push(subgraph_invalid(subgraph_id, "nodes must not be empty"));
        return None;
    }
    Some(SubgraphBody {
        nodes: parsed_nodes,
        edges: parsed_edges,
    })
}

fn check_subgraph_shape(subgraph_id: &str, nodes: &[GraphNode], errors: &mut Vec<DslError>) {
    if nodes.iter().any(|node| node.node_type == NodeType::Start) {
        errors.push(subgraph_invalid(
            subgraph_id,
            "subgraph must not contain start nodes",
        ));
    }
    let ends = nodes
        .iter()
        .filter(|node| node.node_type == NodeType::End)
        .count();
    if ends == 0 {
        errors.push(subgraph_invalid(
            subgraph_id,
            "subgraph must contain at least one end node",
        ));
    }
}

fn validate_subgraph_mappings(
    subgraph_id: &str,
    nodes: &[GraphNode],
    input_mapping: &BTreeMap<String, String>,
    output_mapping: &BTreeMap<String, String>,
    errors: &mut Vec<DslError>,
) -> BTreeSet<String> {
    let mut entries = BTreeSet::new();
    for (external, reference) in input_mapping {
        match resolve_node_port(nodes, reference) {
            Some((node, port)) if node.inputs.iter().any(|input| input.name == port) => {
                entries.insert(node.id.clone());
            }
            _ => errors.push(subgraph_invalid(
                subgraph_id,
                &format!("input_mapping '{external}' references unknown node/port '{reference}'"),
            )),
        }
    }
    for (external, reference) in output_mapping {
        let valid = resolve_node_port(nodes, reference).is_some_and(|(node, port)| {
            node.node_type == NodeType::End && node.inputs.iter().any(|input| input.name == port)
        });
        if !valid {
            errors.push(subgraph_invalid(
                subgraph_id,
                &format!(
                    "output_mapping '{external}' must reference an end node port, got '{reference}'"
                ),
            ));
        }
    }
    entries
}

/// 解析 `"<node_id>.<port>"` 引用；node id 允许含点，因此从右向左尝试切分。
fn resolve_node_port<'a>(
    nodes: &'a [GraphNode],
    reference: &'a str,
) -> Option<(&'a GraphNode, &'a str)> {
    let mut split_index = reference.len();
    while let Some(dot) = reference[..split_index].rfind('.') {
        let (node_id, port) = (&reference[..dot], &reference[dot + 1..]);
        if let Some(node) = nodes.iter().find(|node| node.id == node_id) {
            return Some((node, port));
        }
        split_index = dot;
    }
    None
}
