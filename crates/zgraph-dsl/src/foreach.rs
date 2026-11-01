//! R18：`foreach` 区域校验（W4a / DR-007 §3.2/§4；新 DSL 码 `DSL_FOREACH_INVALID`）。
//!
//! 语义依据：`doc/decisions/DR-007-w4a-dynamic-fanout-freeze.md` §3.2（reason 闭集 11 值）
//! 与 §4（区域生成算法）。对每个声明了 `foreach` 的节点 `F`（按 `nodes` 数组序取首错）：
//!
//! - `J = F.foreach.join`；`R = {F} ∪ {v | v 从 F 沿出边可达且路径不经过 J}`；
//! - 依次检查 join 存在/不在 region/类型、region 内节点类型与嵌套、唯一出口 `E`
//!   （`E→J` 恰一条且无 gate）、region 边界闭合（含 `F` 的入边必须来自 `R` 外）、
//!   `items_port` 有入边来源、region 与已接受 region 不相交；
//! - 首因即错误；错误载荷冻结：`message`、`expected="valid foreach region"`、
//!   `actual={"node_id":F,"reason":reason}`、`location.node_id=F`。
//!
//! 执行顺序（W4a 冻结）：R15（defer 可重入）之后、R16（静态扇出上限）之前；R18 只对
//! 主图执行（子图内 foreach 由 R17/R6 拦截为 `DSL_SUBGRAPH_INVALID`）。

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use crate::error::{DslError, DslErrorCode};
use crate::types::{GraphEdge, GraphNode, NodeType};

/// R18 reason 闭集（W4a 冻结 11 值，按 DR-007 §3.2 判定顺序）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RegionReason {
    /// `foreach.join` 指向不存在的 node_id。
    JoinNotFound,
    /// `join` 在 region 内（等价 `join == F`）。
    JoinInRegion,
    /// `join` 节点类型不是 `python_node`/`agent_call`/`end`。
    JoinTypeUnsupported,
    /// region 内含 `start`/`end`/`subgraph` 节点。
    ForbiddenNodeType,
    /// region 内含其它带 `foreach` 的节点（禁止嵌套）。
    NestedForeach,
    /// region 没有出口（没有节点有指向 region 外的出边）。
    NoExit,
    /// region 有多个出口。
    MultipleExits,
    /// region 边界泄漏（出口边不是 `E→J`、region 节点外部边、F 的入边来自 region 内）。
    ExternalEdge,
    /// 唯一出口 `E→J` 的边带 gate。
    JoinEdgeGated,
    /// `F.foreach.items_port` 没有入边提供来源。
    ItemsPortMissing,
    /// region 与先前已接受（更早 `nodes` 序）的其它 `foreach` region 相交。
    RegionOverlap,
}

impl RegionReason {
    /// 冻结 reason 字符串（`actual.reason` / message 用）。
    const fn as_str(self) -> &'static str {
        match self {
            Self::JoinNotFound => "join_not_found",
            Self::JoinInRegion => "join_in_region",
            Self::JoinTypeUnsupported => "join_type_unsupported",
            Self::ForbiddenNodeType => "forbidden_node_type",
            Self::NestedForeach => "nested_foreach",
            Self::NoExit => "no_exit",
            Self::MultipleExits => "multiple_exits",
            Self::ExternalEdge => "external_edge",
            Self::JoinEdgeGated => "join_edge_gated",
            Self::ItemsPortMissing => "items_port_missing",
            Self::RegionOverlap => "region_overlap",
        }
    }
}

/// R18：主图 `foreach` 区域校验（L2 纯函数，取首个非法 `F` 的首因）。
pub(crate) fn check_foreach_regions(nodes: &[GraphNode], edges: &[GraphEdge]) -> Vec<DslError> {
    if !nodes.iter().any(|node| node.foreach.is_some()) {
        return Vec::new();
    }
    let graph = GraphIndex::new(nodes, edges);
    let mut accepted: Vec<BTreeSet<usize>> = Vec::new();
    for (f_index, node) in nodes.iter().enumerate() {
        let Some(spec) = &node.foreach else {
            continue;
        };
        match validate_region(&graph, nodes, edges, f_index, &spec.join, &spec.items_port) {
            Ok(region) => {
                if accepted
                    .iter()
                    .any(|previous| !previous.is_disjoint(&region))
                {
                    return vec![region_error(&node.id, RegionReason::RegionOverlap)];
                }
                accepted.push(region);
            }
            Err(reason) => return vec![region_error(&node.id, reason)],
        }
    }
    Vec::new()
}

/// 节点/边索引（按 `nodes` 数组序建下标；出/入边各自按 `edges` 数组序保存）。
struct GraphIndex {
    index_of: BTreeMap<String, usize>,
    out_edges: Vec<Vec<usize>>,
    in_edges: Vec<Vec<usize>>,
    source: Vec<usize>,
    target: Vec<usize>,
}

impl GraphIndex {
    fn new(nodes: &[GraphNode], edges: &[GraphEdge]) -> Self {
        let index_of: BTreeMap<String, usize> = nodes
            .iter()
            .enumerate()
            .map(|(index, node)| (node.id.clone(), index))
            .collect();
        let mut out_edges = vec![Vec::new(); nodes.len()];
        let mut in_edges = vec![Vec::new(); nodes.len()];
        let mut source = vec![usize::MAX; edges.len()];
        let mut target = vec![usize::MAX; edges.len()];
        for (edge_index, edge) in edges.iter().enumerate() {
            let (Some(&from), Some(&to)) = (
                index_of.get(edge.from.as_str()),
                index_of.get(edge.to_node.as_str()),
            ) else {
                continue;
            };
            source[edge_index] = from;
            target[edge_index] = to;
            out_edges[from].push(edge_index);
            in_edges[to].push(edge_index);
        }
        Self {
            index_of,
            out_edges,
            in_edges,
            source,
            target,
        }
    }
}

/// 校验单个 `F` 的 region；返回 region 节点下标集（升序 = `nodes` 数组序）。
fn validate_region(
    graph: &GraphIndex,
    nodes: &[GraphNode],
    edges: &[GraphEdge],
    f: usize,
    join: &str,
    items_port: &str,
) -> Result<BTreeSet<usize>, RegionReason> {
    // row1/row2：join 存在、且不在 region 内（等价 join != F）。
    let Some(&j) = graph.index_of.get(join) else {
        return Err(RegionReason::JoinNotFound);
    };
    if j == f {
        return Err(RegionReason::JoinInRegion);
    }
    let region = reachable(graph, f, j);
    if region.contains(&j) {
        return Err(RegionReason::JoinInRegion);
    }
    // row3：join 类型。
    if !matches!(
        nodes[j].node_type,
        NodeType::PythonNode | NodeType::AgentCall | NodeType::End
    ) {
        return Err(RegionReason::JoinTypeUnsupported);
    }
    // row4：region 内节点类型（先类型、后嵌套；F 自身除外——它是 foreach 节点本身）。
    for &v in &region {
        if v != f
            && matches!(
                nodes[v].node_type,
                NodeType::Start | NodeType::End | NodeType::Subgraph
            )
        {
            return Err(RegionReason::ForbiddenNodeType);
        }
    }
    for &v in &region {
        if v != f && nodes[v].foreach.is_some() {
            return Err(RegionReason::NestedForeach);
        }
    }
    // row5：唯一出口 E（region 内「存在出边指向 region 外」的节点数恰为 1）。
    let exits: Vec<usize> = region
        .iter()
        .copied()
        .filter(|&v| {
            graph.out_edges[v]
                .iter()
                .any(|&edge_index| !region.contains(&graph.target[edge_index]))
        })
        .collect();
    if exits.is_empty() {
        return Err(RegionReason::NoExit);
    }
    if exits.len() > 1 {
        return Err(RegionReason::MultipleExits);
    }
    let exit = exits[0];
    // row6：E 恰有 1 条出边且目标 = J；该边不得有 gate。
    let exit_out = &graph.out_edges[exit];
    if exit_out.len() != 1 || graph.target[exit_out[0]] != j {
        return Err(RegionReason::ExternalEdge);
    }
    if edges[exit_out[0]].gate.is_some() {
        return Err(RegionReason::JoinEdgeGated);
    }
    // row7：region 内节点（除 E）的全部出边目标 ∈ region。
    for &v in &region {
        if v == exit {
            continue;
        }
        if graph.out_edges[v]
            .iter()
            .any(|&edge_index| !region.contains(&graph.target[edge_index]))
        {
            return Err(RegionReason::ExternalEdge);
        }
    }
    // row8：region 内节点（除 F）的入边来源 ∈ region；F 的全部入边来源 ∉ region（C1/N13）。
    for &v in &region {
        if v == f {
            continue;
        }
        if graph.in_edges[v]
            .iter()
            .any(|&edge_index| !region.contains(&graph.source[edge_index]))
        {
            return Err(RegionReason::ExternalEdge);
        }
    }
    if graph.in_edges[f]
        .iter()
        .any(|&edge_index| region.contains(&graph.source[edge_index]))
    {
        return Err(RegionReason::ExternalEdge);
    }
    // row9：items_port 由 F 的入边提供（R12 不变；这里覆盖非 required 端口情形）。
    let sourced = graph.in_edges[f]
        .iter()
        .any(|&edge_index| edges[edge_index].to_input_port == items_port);
    if !sourced {
        return Err(RegionReason::ItemsPortMissing);
    }
    Ok(region)
}

/// `R = {F} ∪ {v | v 从 F 沿出边可达且路径不经过 J}`（BFS；nodes/edges 序确定）。
fn reachable(graph: &GraphIndex, f: usize, j: usize) -> BTreeSet<usize> {
    let mut region = BTreeSet::new();
    region.insert(f);
    let mut queue = VecDeque::new();
    queue.push_back(f);
    while let Some(node) = queue.pop_front() {
        for &edge_index in &graph.out_edges[node] {
            let next = graph.target[edge_index];
            if next == j || region.contains(&next) {
                continue;
            }
            region.insert(next);
            queue.push_back(next);
        }
    }
    region
}

/// R18 错误载荷（冻结）：message/expected/actual/location 全按 DR-007 §3.2。
fn region_error(f_node_id: &str, reason: RegionReason) -> DslError {
    let reason = reason.as_str();
    DslError::new(
        DslErrorCode::ForeachInvalid,
        format!("foreach region of node '{f_node_id}' is invalid: {reason}"),
    )
    .at_node(f_node_id.to_string())
    .expected("valid foreach region")
    .actual(serde_json::json!({"node_id": f_node_id, "reason": reason}))
}
