//! R8–R13：边端点、端口引用、两级唯一性、输入来源、无门控环检测。
//!
//! 契约来源：`doc/spec-v1.2/02-DSL与校验.md` §2.4.1 R8–R13。
//! 说明：边的**结构**检查（键白名单/必填/类型、gate 对象形状）在执行 R8 时完成，
//! 错误码仍按 02 §2.4.1 的归属分配（结构错误 → `DSL_PARSE_ERROR`/`DSL_MISSING_FIELD`）。

use std::collections::{BTreeMap, HashSet};

use serde_json::Value;

use crate::error::{msg, DslError, DslErrorCode};
use crate::shape::{check_keys, required_nonempty_str};
use crate::types::{GateDefinition, GraphEdge, GraphNode, NodeType};

const EDGE_KEYS: [&str; 5] = [
    "from",
    "to_node",
    "to_input_port",
    "gate",
    "from_output_port",
];
const GATE_KEYS: [&str; 2] = ["function", "params"];

/// R8（结构部分）：把 `edges` 数组解析成类型化边。
pub(crate) fn parse_edges(value: &Value) -> Result<Vec<GraphEdge>, Vec<DslError>> {
    let mut errors = Vec::new();
    let Some(items) = value.as_array() else {
        return Err(vec![msg::wrong_type("edges", "array", "top level", value)]);
    };
    let mut edges = Vec::new();
    for (index, item) in items.iter().enumerate() {
        let start = errors.len();
        let scope = format!("edges[{index}]");
        let Some(map) = item.as_object() else {
            errors.push(msg::wrong_type(&scope, "object", "edges", item).at_edge(index));
            continue;
        };
        check_keys(map, &EDGE_KEYS, &scope, &mut errors);
        let from = required_nonempty_str(map, "from", &scope, &mut errors).map(str::to_string);
        let to_node =
            required_nonempty_str(map, "to_node", &scope, &mut errors).map(str::to_string);
        let to_input_port =
            required_nonempty_str(map, "to_input_port", &scope, &mut errors).map(str::to_string);
        let gate = parse_gate(map.get("gate"), &scope, &mut errors);
        // v1 保留字段：出现即报错，不看取值（02 §2.2.4 / REV-005 #2）。
        let from_output_port = map.get("from_output_port").cloned();
        attach_edge_location(&mut errors, start, index);
        if errors.len() == start {
            edges.push(GraphEdge {
                from: from.unwrap_or_default(),
                to_node: to_node.unwrap_or_default(),
                to_input_port: to_input_port.unwrap_or_default(),
                gate,
                from_output_port,
            });
        }
    }
    if errors.is_empty() {
        Ok(edges)
    } else {
        Err(errors)
    }
}

fn parse_gate(
    value: Option<&Value>,
    scope: &str,
    errors: &mut Vec<DslError>,
) -> Option<GateDefinition> {
    let value = value?;
    let Some(map) = value.as_object() else {
        errors.push(msg::wrong_type("gate", "object", scope, value));
        return None;
    };
    check_keys(map, &GATE_KEYS, scope, errors);
    let function = match map.get("function") {
        None => {
            errors.push(msg::missing_field("function", scope));
            return None;
        }
        Some(Value::String(function)) if !function.is_empty() => function.clone(),
        Some(Value::String(_)) => {
            errors.push(msg::missing_field("function", scope));
            return None;
        }
        Some(other) => {
            errors.push(msg::wrong_type("function", "string", scope, other));
            return None;
        }
    };
    let params = match map.get("params") {
        None => None,
        Some(value) if value.is_object() => Some(value.clone()),
        Some(other) => {
            errors.push(msg::wrong_type("params", "object", scope, other));
            None
        }
    };
    Some(GateDefinition { function, params })
}

/// 把边作用域内收集的错误补上 `edge_index`（02 §2.7.3：边级错误至少一个定位）。
fn attach_edge_location(errors: &mut [DslError], start: usize, edge_index: usize) {
    for error in &mut errors[start..] {
        if error.location.edge_index.is_none() && error.location.node_id.is_none() {
            error.location.edge_index = Some(edge_index);
        }
    }
}

/// R8（语义部分）：端点存在 + 方向合法。
pub(crate) fn check_edge_endpoints(
    edges: &[GraphEdge],
    nodes: &[GraphNode],
    entries: &HashSet<&str>,
) -> Vec<DslError> {
    let by_id: BTreeMap<&str, &GraphNode> =
        nodes.iter().map(|node| (node.id.as_str(), node)).collect();
    let mut errors = Vec::new();
    for (index, edge) in edges.iter().enumerate() {
        let from_node = by_id.get(edge.from.as_str()).copied();
        let to_node = by_id.get(edge.to_node.as_str()).copied();
        if from_node.is_none() {
            errors.push(invalid_endpoint(index, "unknown_from", edge));
        }
        if to_node.is_none() {
            errors.push(invalid_endpoint(index, "unknown_to", edge));
        }
        if from_node.is_some_and(|node| node.node_type == NodeType::End) {
            errors.push(invalid_endpoint(index, "end_as_source", edge));
        }
        if to_node.is_some_and(|node| node.node_type == NodeType::Start) {
            errors.push(invalid_endpoint(index, "start_as_target", edge));
        }
        if entries.contains(edge.to_node.as_str()) {
            errors.push(invalid_endpoint(index, "entry_as_target", edge));
        }
    }
    errors
}

fn endpoint_node(edge: &GraphEdge, reason: &str) -> String {
    if reason == "unknown_from" {
        edge.from.clone()
    } else {
        edge.to_node.clone()
    }
}

fn invalid_endpoint(edge_index: usize, reason: &str, edge: &GraphEdge) -> DslError {
    DslError::new(
        DslErrorCode::InvalidEdgeRef,
        format!("edge[{edge_index}] invalid endpoint: {reason}"),
    )
    .at_edge(edge_index)
    .at_node(endpoint_node(edge, reason))
    .expected("existing node ids with a legal edge direction")
    .actual(serde_json::json!({"from": edge.from, "to_node": edge.to_node}))
}

/// R9：`to_input_port` 已声明、`from_output_port` 保留字段、gate 形状。
pub(crate) fn check_edge_ports(edges: &[GraphEdge], nodes: &[GraphNode]) -> Vec<DslError> {
    let by_id: BTreeMap<&str, &GraphNode> =
        nodes.iter().map(|node| (node.id.as_str(), node)).collect();
    let mut errors = Vec::new();
    for (index, edge) in edges.iter().enumerate() {
        if let Some(target) = by_id.get(edge.to_node.as_str()) {
            let declared: Vec<&str> = target
                .inputs
                .iter()
                .map(|port| port.name.as_str())
                .collect();
            if !declared.contains(&edge.to_input_port.as_str()) {
                errors.push(
                    DslError::new(
                        DslErrorCode::InvalidPort,
                        format!(
                            "edge[{index}].to_input_port '{}' is not declared on node '{}'",
                            edge.to_input_port, edge.to_node
                        ),
                    )
                    .at_edge(index)
                    .at_node(edge.to_node.clone())
                    .at_port(edge.to_input_port.clone())
                    .expected(declared)
                    .actual(edge.to_input_port.clone()),
                );
            }
        }
        if let Some(actual) = &edge.from_output_port {
            errors.push(msg::unsupported_field("from_output_port", actual).at_edge(index));
        }
    }
    errors
}

/// R10：`(from, to_node)` 唯一。
pub(crate) fn check_duplicate_edges(edges: &[GraphEdge]) -> Vec<DslError> {
    let mut errors = Vec::new();
    let mut seen: BTreeMap<(&str, &str), usize> = BTreeMap::new();
    for (index, edge) in edges.iter().enumerate() {
        let key = (edge.from.as_str(), edge.to_node.as_str());
        match seen.get(&key) {
            None => {
                seen.insert(key, index);
            }
            Some(first) => errors.push(
                DslError::new(
                    DslErrorCode::DuplicateEdge,
                    format!(
                        "duplicate edge {} -> {}: edges[{first}] and edges[{index}]",
                        edge.from, edge.to_node
                    ),
                )
                .at_edge(index)
                .at_node(edge.from.clone())
                .expected(*first as u64)
                .actual(index as u64),
            ),
        }
    }
    errors
}

/// R11：`(to_node, to_input_port)` 唯一（v1 禁止 fan-in）。
pub(crate) fn check_duplicate_port_sources(edges: &[GraphEdge]) -> Vec<DslError> {
    let mut errors = Vec::new();
    let mut seen: BTreeMap<(&str, &str), usize> = BTreeMap::new();
    for (index, edge) in edges.iter().enumerate() {
        let key = (edge.to_node.as_str(), edge.to_input_port.as_str());
        match seen.get(&key) {
            None => {
                seen.insert(key, index);
            }
            Some(first) => errors.push(
                DslError::new(
                    DslErrorCode::DuplicatePortSource,
                    format!(
                        "duplicate input source for '{}.{}': edges[{first}] and edges[{index}]",
                        edge.to_node, edge.to_input_port
                    ),
                )
                .at_edge(index)
                .at_node(edge.to_node.clone())
                .at_port(edge.to_input_port.clone())
                .expected(*first as u64)
                .actual(index as u64),
            ),
        }
    }
    errors
}

/// R12：非 entry 节点的输入来源完整性。
pub(crate) fn check_input_sources(
    nodes: &[GraphNode],
    edges: &[GraphEdge],
    exempt: &HashSet<&str>,
) -> Vec<DslError> {
    let mut incoming_count: BTreeMap<&str, usize> = BTreeMap::new();
    let mut sourced: HashSet<(&str, &str)> = HashSet::new();
    for edge in edges {
        *incoming_count.entry(edge.to_node.as_str()).or_insert(0) += 1;
        sourced.insert((edge.to_node.as_str(), edge.to_input_port.as_str()));
    }
    let mut errors = Vec::new();
    for node in nodes {
        if exempt.contains(node.id.as_str()) {
            continue;
        }
        if incoming_count.get(node.id.as_str()).copied().unwrap_or(0) == 0 {
            errors.push(
                DslError::new(
                    DslErrorCode::UnconnectedInput,
                    format!("node '{}' has no incoming edge", node.id),
                )
                .at_node(node.id.clone())
                .expected("at least one incoming edge")
                .actual(0_u64),
            );
            continue;
        }
        collect_missing_sources(node, &sourced, &mut errors);
    }
    errors
}

fn collect_missing_sources(
    node: &GraphNode,
    sourced: &HashSet<(&str, &str)>,
    errors: &mut Vec<DslError>,
) {
    let required: Vec<&str> = node
        .inputs
        .iter()
        .filter(|port| port.required)
        .map(|port| port.name.as_str())
        .collect();
    match node.trigger {
        crate::types::Trigger::All => {
            for port in required {
                if !sourced.contains(&(node.id.as_str(), port)) {
                    errors.push(unconnected_port(node, port));
                }
            }
        }
        crate::types::Trigger::Any => {
            if required.is_empty() {
                return;
            }
            if !required
                .iter()
                .any(|port| sourced.contains(&(node.id.as_str(), *port)))
            {
                errors.push(
                    DslError::new(
                        DslErrorCode::UnconnectedInput,
                        format!(
                            "node '{}' has no required input with a source (trigger=any)",
                            node.id
                        ),
                    )
                    .at_node(node.id.clone())
                    .expected("at least one required input with an incoming edge")
                    .actual(required.join(",")),
                );
            }
        }
    }
}

fn unconnected_port(node: &GraphNode, port: &str) -> DslError {
    DslError::new(
        DslErrorCode::UnconnectedInput,
        format!("input '{}.{port}' has no incoming edge", node.id),
    )
    .at_node(node.id.clone())
    .at_port(port.to_string())
    .expected("an incoming edge for the required port")
    .actual("no incoming edge")
}

/// R13：无门控环检测（自环计入）。
pub(crate) fn check_cycles(nodes: &[GraphNode], edges: &[GraphEdge]) -> Vec<DslError> {
    let adjacency = build_gate_less_adjacency(edges);
    let mut colors: BTreeMap<&str, u8> = BTreeMap::new();
    let mut path: Vec<&str> = Vec::new();
    let mut path_edges: Vec<usize> = Vec::new();
    for node in nodes {
        if colors.contains_key(node.id.as_str()) {
            continue;
        }
        if let Some(cycle) = find_cycle_from(
            node.id.as_str(),
            &adjacency,
            &mut colors,
            &mut path,
            &mut path_edges,
        ) {
            return vec![cycle_error(&cycle)];
        }
    }
    Vec::new()
}

fn build_gate_less_adjacency<'a>(
    edges: &'a [GraphEdge],
) -> BTreeMap<&'a str, Vec<(&'a str, usize)>> {
    let mut adjacency: BTreeMap<&'a str, Vec<(&'a str, usize)>> = BTreeMap::new();
    for (index, edge) in edges.iter().enumerate() {
        if edge.gate.is_none() {
            adjacency
                .entry(edge.from.as_str())
                .or_default()
                .push((edge.to_node.as_str(), index));
        }
    }
    adjacency
}

struct Cycle<'a> {
    nodes: Vec<&'a str>,
    edges: Vec<usize>,
}

fn find_cycle_from<'a>(
    start: &'a str,
    adjacency: &BTreeMap<&'a str, Vec<(&'a str, usize)>>,
    colors: &mut BTreeMap<&'a str, u8>,
    path: &mut Vec<&'a str>,
    path_edges: &mut Vec<usize>,
) -> Option<Cycle<'a>> {
    colors.insert(start, 1);
    path.push(start);
    if let Some(nexts) = adjacency.get(start) {
        for (next, edge_index) in nexts {
            match colors.get(next).copied().unwrap_or(0) {
                0 => {
                    path_edges.push(*edge_index);
                    let found = find_cycle_from(next, adjacency, colors, path, path_edges);
                    path_edges.pop();
                    if found.is_some() {
                        path.pop();
                        colors.insert(start, 2);
                        return found;
                    }
                }
                1 => {
                    let cycle = build_cycle(path, path_edges, next, *edge_index);
                    path.pop();
                    colors.insert(start, 2);
                    return Some(cycle);
                }
                _ => {}
            }
        }
    }
    path.pop();
    colors.insert(start, 2);
    None
}

fn build_cycle<'a>(
    path: &[&'a str],
    path_edges: &[usize],
    repeated: &'a str,
    closing_edge: usize,
) -> Cycle<'a> {
    let start = path.iter().position(|node| *node == repeated).unwrap_or(0);
    let mut nodes: Vec<&'a str> = path[start..].to_vec();
    let mut edges: Vec<usize> = path_edges.get(start..).unwrap_or_default().to_vec();
    edges.push(closing_edge);
    nodes.push(repeated);
    Cycle { nodes, edges }
}

fn cycle_error(cycle: &Cycle<'_>) -> DslError {
    let edge_index = cycle.edges.iter().copied().min().unwrap_or_default();
    let node_id = cycle.nodes.first().copied().unwrap_or_default().to_string();
    let path_text = cycle.nodes.join(" -> ");
    DslError::new(
        DslErrorCode::CycleDetected,
        format!("ungated data cycle: {path_text} (edges: {:?})", cycle.edges),
    )
    .at_edge(edge_index)
    .at_node(node_id)
    .expected("acyclic")
    .actual(serde_json::json!(cycle.nodes))
}
