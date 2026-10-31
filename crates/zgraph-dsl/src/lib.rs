//! 图 DSL 的类型、加载、R1–R15 校验与 `graph_hash`（M1 实现；R15 为 W2a 新增）。
//!
//! 语义基准：`doc/spec-v1.2/02-DSL与校验.md`（schema/R1–R15/错误对象/graph_hash）
//! 与 `doc/decisions/DR-004-v02-scope-and-waves.md` D1、`DR-005`（R15 defer 可重入）；
//! 落地偏差：`doc/architecture-v2.md` §8 的 **D6**（门控 = Rust 内置 op）与
//! **D7**（图加载期不 import 用户 Python；`python_node` 只校验格式）。
//!
//! # 快速开始
//!
//! ```
//! use zgraph_dsl::load_graph;
//!
//! let graph = br#"{
//!   "version": "1.0",
//!   "metadata": {"name": "minimal"},
//!   "entry_points": ["start"],
//!   "nodes": [{"id": "start", "type": "start", "inputs": [], "config": {}}],
//!   "edges": []
//! }"#;
//! let loaded = load_graph(graph).expect("valid graph");
//! assert_eq!(loaded.graph_hash.len(), 16);
//! ```
//!
//! # 失败策略
//!
//! 跨规则 fail-fast、同规则内聚合、最多 [`MAX_DSL_ERRORS`] 条；错误码全部来自
//! 00 §0.4 的 18 个 `DSL_*` 码（`DSL_DEFER_NOT_REENTRANT` 为 W2a 新增，W2a0 回写）。

mod assets;
mod edges;
mod error;
mod hash;
mod nodes;
mod shape;
mod symbols;
mod types;

use std::collections::HashSet;
use std::path::PathBuf;

use serde::Serialize;
use serde_json::Value;

pub use error::{DslError, DslErrorCode, DslErrors, Location, MAX_DSL_ERRORS};
pub use hash::{canonical_json, graph_hash, GRAPH_HASH_LEN};
pub use symbols::{
    canonical_gate_symbol, is_builtin_gate_op, python_node_symbol, GateOp, SymbolKind,
    BUILTIN_GATE_OPS,
};
pub use types::{
    AgentCallConfig, ExecClass, GateDefinition, GraphDefinition, GraphEdge, GraphMetadata,
    GraphNode, InputPort, NodeConfig, NodeType, OnGateFalse, OutputPort, PortType,
    PythonNodeConfig, RetryConfig, SubgraphConfig, SubgraphDefinition, Trigger, DSL_SCHEMA_VERSION,
};

/// 实例级节点超时缺省值（00 §0.5 `DEFAULT_NODE_TIMEOUT_MS`）。
pub const DEFAULT_NODE_TIMEOUT_MS: u64 = 180_000;

/// 自环/显式重试最大尝试次数缺省值（00 §0.5 `DEFAULT_MAX_ATTEMPTS`）。
pub const DEFAULT_MAX_ATTEMPTS: u32 = 3;

/// 加载选项。
#[derive(Debug, Clone, Default)]
pub struct LoadOptions {
    /// Agent 资产根目录；`Some` 时执行 R14c（`<dir>/agents/*.json`），`None` 时跳过。
    pub assets_dir: Option<PathBuf>,
}

impl LoadOptions {
    /// 空选项（不校验 agent 资产）。
    pub fn new() -> Self {
        Self::default()
    }

    /// 指定 Agent 资产根目录。
    pub fn with_assets_dir(mut self, dir: impl Into<PathBuf>) -> Self {
        self.assets_dir = Some(dir.into());
        self
    }
}

/// 加载成功的产物：编译前结构 + 图指纹。
#[derive(Debug, Clone)]
pub struct LoadedGraph {
    /// 通过 R1–R15 的图定义。
    pub definition: GraphDefinition,
    /// 图指纹（BLAKE3 canonical JSON 前 16 hex）。
    pub graph_hash: String,
    raw: Value,
}

impl LoadedGraph {
    /// 原始 DSL JSON（用于 graph_hash 复核/调试）。
    pub fn raw(&self) -> &Value {
        &self.raw
    }

    /// canonical JSON 文本（02 §2.8.2）。
    pub fn canonical_json(&self) -> String {
        canonical_json(&hash::canonical_projection(&self.raw))
    }
}

/// `zgraph validate` 的输出契约（02 §2.7.2）。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ValidationReport {
    /// 是否通过全部校验。
    pub ok: bool,
    /// 错误列表（`ok=true` 时为空）。
    pub errors: Vec<DslError>,
    /// 采样总数（可能大于 `errors.len()`）。
    pub error_count: usize,
    /// 是否截断。
    pub truncated: bool,
    /// 成功时的图指纹（失败时缺省，不输出该键）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub graph_hash: Option<String>,
}

impl ValidationReport {
    /// 由加载结果构造报告。
    pub fn from_result(result: &Result<LoadedGraph, DslErrors>) -> Self {
        match result {
            Ok(loaded) => Self {
                ok: true,
                errors: Vec::new(),
                error_count: 0,
                truncated: false,
                graph_hash: Some(loaded.graph_hash.clone()),
            },
            Err(errors) => Self {
                ok: false,
                errors: errors.errors.clone(),
                error_count: errors.error_count,
                truncated: errors.truncated,
                graph_hash: None,
            },
        }
    }
}

/// 从 JSON 字节加载图（不校验 agent 资产）。
pub fn load_graph(bytes: &[u8]) -> Result<LoadedGraph, DslErrors> {
    load_graph_with_options(bytes, &LoadOptions::default())
}

/// 从 JSON 字节加载图，带加载选项。
pub fn load_graph_with_options(
    bytes: &[u8],
    options: &LoadOptions,
) -> Result<LoadedGraph, DslErrors> {
    let raw: Value = match serde_json::from_slice(bytes) {
        Ok(raw) => raw,
        Err(error) => {
            return Err(DslErrors::single(DslError::new(
                DslErrorCode::ParseError,
                format!(
                    "invalid JSON at line {}, column {}: {error}",
                    error.line(),
                    error.column()
                ),
            )))
        }
    };
    load_graph_value(raw, options)
}

/// 从 JSON 文本加载图。
pub fn load_graph_text(text: &str) -> Result<LoadedGraph, DslErrors> {
    load_graph(text.as_bytes())
}

/// 从已解析的 JSON 值加载图（跳过 R1 的 JSON 解析，仍执行 R2–R15）。
pub fn load_graph_value(raw: Value, options: &LoadOptions) -> Result<LoadedGraph, DslErrors> {
    // R2：顶层必填/类型/未知键。
    ensure_empty(nodes::check_top_level(&raw))?;
    // R3：version + metadata。
    let metadata = nodes::check_version_and_metadata(&raw).map_err(DslErrors::new)?;
    // R4：节点声明。
    let nodes_value = raw.get("nodes").cloned().unwrap_or(Value::Null);
    let node_list = nodes::parse_nodes(&nodes_value).map_err(DslErrors::new)?;
    // R5：节点 ID 唯一。
    ensure_empty(nodes::check_node_id_unique(&node_list))?;
    // R6：子图定义与引用。
    let subgraphs = nodes::check_subgraphs(&raw, &node_list).map_err(DslErrors::new)?;
    // R7：entry_points。
    let entry_points = string_list(&raw, "entry_points");
    ensure_empty(nodes::check_entry_points(&entry_points, &node_list))?;
    let entry_set: HashSet<&str> = entry_points.iter().map(String::as_str).collect();
    // R8：边端点。
    let edges_value = raw.get("edges").cloned().unwrap_or(Value::Null);
    let edge_list = edges::parse_edges(&edges_value).map_err(DslErrors::new)?;
    ensure_empty(edges::check_edge_endpoints(
        &edge_list, &node_list, &entry_set,
    ))?;
    // R9：端口引用 / 保留字段。
    ensure_empty(edges::check_edge_ports(&edge_list, &node_list))?;
    // R10 / R11：两级唯一性。
    ensure_empty(edges::check_duplicate_edges(&edge_list))?;
    ensure_empty(edges::check_duplicate_port_sources(&edge_list))?;
    // R12：输入来源完整。
    ensure_empty(edges::check_input_sources(
        &node_list, &edge_list, &entry_set,
    ))?;
    // R13：无门控环（只查无 gate 边，保持 v0.1.0 语义）。
    ensure_empty(edges::check_cycles(&node_list, &edge_list))?;
    // R15：defer 边源节点必须可重入（W2a / DR-004 D1；覆盖全部边）。
    ensure_empty(edges::check_defer_reentrancy(&node_list, &edge_list))?;
    let definition = GraphDefinition {
        version: DSL_SCHEMA_VERSION.to_string(),
        metadata,
        entry_points,
        nodes: node_list,
        edges: edge_list,
        subgraphs,
    };
    // R14：符号（内置 gate op / python_node 格式）与 agent 资产。
    let mut errors = symbols::resolve_symbols(&definition.nodes, &definition.edges);
    if let Some(assets_dir) = &options.assets_dir {
        errors.extend(assets::check_agents(&definition.nodes, assets_dir));
    }
    if !errors.is_empty() {
        return Err(DslErrors::new(errors));
    }
    // L4：graph_hash。
    let graph_hash = hash::graph_hash(&raw);
    Ok(LoadedGraph {
        definition,
        graph_hash,
        raw,
    })
}

/// 校验字节并输出报告（CLI `--json` 直接序列化本类型）。
pub fn validate_bytes(bytes: &[u8], options: &LoadOptions) -> ValidationReport {
    ValidationReport::from_result(&load_graph_with_options(bytes, options))
}

/// 校验字节（不校验 agent 资产）。
pub fn validate_graph(bytes: &[u8]) -> ValidationReport {
    validate_bytes(bytes, &LoadOptions::default())
}

fn ensure_empty(errors: Vec<DslError>) -> Result<(), DslErrors> {
    if errors.is_empty() {
        Ok(())
    } else {
        Err(DslErrors::new(errors))
    }
}

fn string_list(raw: &Value, key: &str) -> Vec<String> {
    raw.get(key)
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}
