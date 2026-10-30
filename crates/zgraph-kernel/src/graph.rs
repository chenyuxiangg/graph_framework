//! 编译期产物：`CompiledGraph`/`CompiledNode`/`CompiledEdge` 与边索引。
//!
//! 语义基准：`doc/spec-v1.2/01-领域模型与状态机.md` §1.1（编译产物、边索引、唯一性）；
//! 偏差：`doc/architecture-v2.md` §8 **D6**（门控编译为 Rust 内置 op）与
//! **D7**（加载期不 import 用户 Python，`python_node` 只编译符号）。
//!
//! 编译在 run 启动前一次性完成：`CompiledGraph` 构造后不可变（`Arc` 只读共享），
//! 运行期不再增删节点/边，因此事件循环可以放心把 `&CompiledGraph` 交给执行任务。

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use zgraph_dsl::{
    AgentCallConfig, ExecClass, GraphEdge, GraphMetadata, GraphNode, InputPort, LoadedGraph,
    NodeConfig, NodeType, OutputPort, PythonNodeConfig, SubgraphDefinition, Trigger,
    DEFAULT_MAX_ATTEMPTS, DEFAULT_NODE_TIMEOUT_MS,
};

use crate::gate::CompiledGate;
use crate::types::{CompileError, EdgeIdx, NodeId, PortName};

/// 编译后的节点。
#[derive(Debug, Clone)]
pub struct CompiledNode {
    /// 节点 ID（图内唯一）。
    pub id: NodeId,
    /// 节点类型。
    pub node_type: NodeType,
    /// 输入端口声明（保持 DSL 顺序；全部声明端口会出现在派发 inputs 中）。
    pub inputs: Vec<InputPort>,
    /// 输出端口声明（v1 仅保留，不参与路由）。
    pub outputs: Vec<OutputPort>,
    /// 输入就绪策略。
    pub trigger: Trigger,
    /// 编译后的处理入口。
    pub handler: CompiledHandler,
    /// DSL 节点显式声明的实例超时（`config.timeout_ms`；未声明为 `None`）。
    ///
    /// 调度阶段按 spec 05 §5.2.6 的优先级链解析最终超时：
    /// 本值 > [`crate::NodeExecutor::timeout_hint`]（`agent_call` 的
    /// `AgentConfig.node_timeout_ms` 回退，ISSUE-076）> [`DEFAULT_NODE_TIMEOUT_MS`]。
    /// 保留 `None` 才能区分「未声明」与「显式声明为默认值（180000）」。
    pub declared_timeout_ms: Option<u64>,
    /// 实例级超时（历史语义：`declared_timeout_ms.unwrap_or(DEFAULT_NODE_TIMEOUT_MS)`）。
    ///
    /// 保留该字段供既有调用方读取；调度阶段以 [`Self::declared_timeout_ms`] 为准，
    /// 否则执行器 hint 会被这里的默认值掩盖。
    pub timeout_ms: u64,
    /// 自环/重试允许的最大尝试次数（`retry.max_attempts`，否则
    /// [`DEFAULT_MAX_ATTEMPTS`]）；`attempt` 从 0 起。
    pub max_attempts: u32,
}

/// 编译后的节点处理入口。
///
/// `start`/`end`/`subgraph` 由内核内置处理，不交给 [`crate::executor::NodeExecutor`]。
#[derive(Debug, Clone)]
pub enum CompiledHandler {
    /// 入口标记节点：输出 = 入口注入整包 object。
    Start,
    /// 终点节点：输出 = inputs object，且不 publish。
    End,
    /// Python 节点（v2 在 worker 子进程执行；M2 由 FakeExecutor 覆盖）。
    Python {
        /// 点分模块路径。
        module: String,
        /// 函数名。
        function: String,
        /// 执行类别（v2 保留但忽略；D4）。
        exec_class: ExecClass,
    },
    /// Agent 调用节点（M4 Harness 实现）。
    Agent {
        /// Agent 资产 ID。
        agent_id: String,
        /// 端口名 → Agent 输入字段。
        input_mapping: BTreeMap<String, String>,
        /// 覆盖 AgentConfig 的 tool 轮次上限。
        max_iterations: Option<u32>,
    },
    /// 子图节点（v1 运行期拒绝：`NODE_TYPE_UNSUPPORTED`）。
    Subgraph {
        /// 被引用的子图 ID。
        subgraph_id: String,
    },
}

/// 编译后的边（数组下标即 `EdgeIndex`）。
#[derive(Debug, Clone)]
pub struct CompiledEdge {
    /// 边的稳定标识（等于 `CompiledGraph.edges` 下标）。
    pub index: EdgeIdx,
    /// 源节点。
    pub from: NodeId,
    /// 目标节点。
    pub to_node: NodeId,
    /// 目标节点输入端口。
    pub to_port: PortName,
    /// 可选门控（None = 恒放行）。
    pub gate: Option<CompiledGate>,
}

/// 边索引（01 §1.1.2；取代 v1.1 的自引用 `EdgeRef`）。
///
/// 唯一性由 DSL 保证：`(to_node, to_port)` 至多一条入边、`(from, to_node)` 至多一条边。
/// 所有 `Vec` 均按 `EdgeIndex` 升序，用于确定性的发布/结算顺序。
#[derive(Debug, Clone, Default)]
pub struct EdgeIndex {
    /// 节点 → 出边（发布扇出）。
    pub outgoing: HashMap<NodeId, Vec<EdgeIdx>>,
    /// `(目标节点, 目标端口)` → 入边（至多一条）。
    pub incoming: HashMap<(NodeId, PortName), EdgeIdx>,
    /// 节点 → 全部入边（就绪/Skipped 判定）。
    pub incoming_by_node: HashMap<NodeId, Vec<EdgeIdx>>,
}

impl EdgeIndex {
    /// 节点的出边（升序；未声明出边返回空切片）。
    pub fn outgoing_edges(&self, node_id: &str) -> &[EdgeIdx] {
        self.outgoing.get(node_id).map(Vec::as_slice).unwrap_or(&[])
    }

    /// 节点的全部入边（升序；入口节点返回空切片）。
    pub fn incoming_edges(&self, node_id: &str) -> &[EdgeIdx] {
        self.incoming_by_node
            .get(node_id)
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }

    /// 指定目标端口上的入边（至多一条）。
    pub fn incoming_edge(&self, node_id: &str, port: &str) -> Option<EdgeIdx> {
        let key = (node_id.to_string(), port.to_string());
        self.incoming.get(&key).copied()
    }
}

/// 编译后的图（不可变；`Arc` 只读共享给事件循环与执行任务）。
#[derive(Debug, Clone)]
pub struct CompiledGraph {
    /// 图指纹（BLAKE3 canonical JSON 前 16 hex）。
    pub graph_hash: String,
    /// 图元数据。
    pub metadata: GraphMetadata,
    /// 原始 DSL JSON（trace/回放用）。
    pub dsl_json: String,
    /// 节点表。
    pub nodes: HashMap<NodeId, CompiledNode>,
    /// DSL 声明顺序（end 聚合与确定性遍历）。
    pub node_order: Vec<NodeId>,
    /// 边表，下标即 `EdgeIndex`。
    pub edges: Vec<CompiledEdge>,
    /// 入口节点（保持 DSL 顺序）。
    pub entry_points: Vec<NodeId>,
    /// 子图 id → 编译产物（v1 只校验，不执行）。
    pub subgraphs: BTreeMap<String, Arc<CompiledGraph>>,
    /// 由 `edges` 构建的边索引（编译期一次性构建）。
    edge_index: EdgeIndex,
}

impl CompiledGraph {
    /// 从加载成功的 DSL 图编译为不可变产物。
    ///
    /// 失败仅表示 [`LoadedGraph`] 结构自洽性被破坏（正常 `load_graph` 路径不会出现），
    /// 例如配置变体与节点类型不一致、门的 `function` 不是内置 op。
    pub fn compile(loaded: &LoadedGraph) -> Result<Arc<CompiledGraph>, CompileError> {
        let definition = &loaded.definition;
        let dsl_json = serde_json::to_string(loaded.raw())
            .map_err(|err| CompileError::new(format!("serialize dsl json: {err}")))?;
        let (nodes, node_order) = compile_nodes(&definition.nodes)?;
        let edges = compile_edges(&definition.edges)?;
        let mut subgraphs = BTreeMap::new();
        for subgraph in &definition.subgraphs {
            let compiled = compile_subgraph(
                subgraph,
                &loaded.graph_hash,
                &dsl_json,
                &definition.metadata,
            )?;
            subgraphs.insert(subgraph.id.clone(), Arc::new(compiled));
        }
        let edge_index = build_edge_index(&edges);
        Ok(Arc::new(CompiledGraph {
            graph_hash: loaded.graph_hash.clone(),
            metadata: definition.metadata.clone(),
            dsl_json,
            nodes,
            node_order,
            edges,
            entry_points: definition.entry_points.clone(),
            subgraphs,
            edge_index,
        }))
    }

    /// 边索引。
    pub fn edge_index(&self) -> &EdgeIndex {
        &self.edge_index
    }

    /// 节点数的便捷访问。
    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    /// 边数的便捷访问。
    pub fn edge_count(&self) -> usize {
        self.edges.len()
    }
}

/// 编译节点表与 `node_order`。
fn compile_nodes(
    nodes: &[GraphNode],
) -> Result<(HashMap<NodeId, CompiledNode>, Vec<NodeId>), CompileError> {
    let mut table = HashMap::with_capacity(nodes.len());
    let mut order = Vec::with_capacity(nodes.len());
    for node in nodes {
        let compiled = compile_node(node)?;
        table.insert(compiled.id.clone(), compiled);
        order.push(node.id.clone());
    }
    Ok((table, order))
}

/// `compile_handler` 的产物：handler、DSL 声明的超时、有效超时、最大尝试次数。
type CompiledHandlerSpec = (CompiledHandler, Option<u64>, u64, u32);

/// 编译单个节点。
fn compile_node(node: &GraphNode) -> Result<CompiledNode, CompileError> {
    let (handler, declared_timeout_ms, timeout_ms, max_attempts) = compile_handler(node)?;
    Ok(CompiledNode {
        id: node.id.clone(),
        node_type: node.node_type,
        inputs: node.inputs.clone(),
        outputs: node.outputs.clone(),
        trigger: node.trigger,
        handler,
        declared_timeout_ms,
        timeout_ms,
        max_attempts,
    })
}

/// 由节点类型 + 配置得到 handler、DSL 声明的超时、有效超时与最大尝试次数。
///
/// `declared` 保留 DSL 原值（`None` = 未声明），供调度阶段按 spec 05 §5.2.6
/// 解析（DSL > AgentConfig 回退 > 默认）；`timeout_ms` 维持既有语义
/// `declared.unwrap_or(DEFAULT_NODE_TIMEOUT_MS)`，兼容既有调用方。
fn compile_handler(node: &GraphNode) -> Result<CompiledHandlerSpec, CompileError> {
    match (node.node_type, &node.config) {
        (NodeType::Start, NodeConfig::StartEnd) => Ok((
            CompiledHandler::Start,
            None,
            DEFAULT_NODE_TIMEOUT_MS,
            DEFAULT_MAX_ATTEMPTS,
        )),
        (NodeType::End, NodeConfig::StartEnd) => Ok((
            CompiledHandler::End,
            None,
            DEFAULT_NODE_TIMEOUT_MS,
            DEFAULT_MAX_ATTEMPTS,
        )),
        (NodeType::PythonNode, NodeConfig::PythonNode(config)) => {
            let handler = compile_python_handler(config);
            Ok((
                handler,
                config.timeout_ms,
                config.timeout_ms.unwrap_or(DEFAULT_NODE_TIMEOUT_MS),
                attempts(config.retry.as_ref()),
            ))
        }
        (NodeType::AgentCall, NodeConfig::AgentCall(config)) => {
            let handler = compile_agent_handler(config);
            Ok((
                handler,
                config.timeout_ms,
                config.timeout_ms.unwrap_or(DEFAULT_NODE_TIMEOUT_MS),
                attempts(config.retry.as_ref()),
            ))
        }
        (NodeType::Subgraph, NodeConfig::Subgraph(config)) => Ok((
            CompiledHandler::Subgraph {
                subgraph_id: config.subgraph_id.clone(),
            },
            None,
            DEFAULT_NODE_TIMEOUT_MS,
            DEFAULT_MAX_ATTEMPTS,
        )),
        _ => Err(CompileError::new(format!(
            "node '{}' type {:?} does not match its config variant",
            node.id, node.node_type
        ))),
    }
}

/// 编译 Python handler。
fn compile_python_handler(config: &PythonNodeConfig) -> CompiledHandler {
    CompiledHandler::Python {
        module: config.module.clone(),
        function: config.function.clone(),
        exec_class: config.exec_class.unwrap_or_default(),
    }
}

/// 编译 Agent handler。
fn compile_agent_handler(config: &AgentCallConfig) -> CompiledHandler {
    CompiledHandler::Agent {
        agent_id: config.agent_id.clone(),
        input_mapping: config.input_mapping.clone(),
        max_iterations: config.max_iterations,
    }
}

/// `retry.max_attempts`（≥1 由 DSL 校验）否则默认值。
fn attempts(retry: Option<&zgraph_dsl::RetryConfig>) -> u32 {
    retry
        .and_then(|config| config.max_attempts)
        .unwrap_or(DEFAULT_MAX_ATTEMPTS)
}

/// 编译边表；门的 `function` 必须是内置 op（D6）。
fn compile_edges(edges: &[GraphEdge]) -> Result<Vec<CompiledEdge>, CompileError> {
    let mut compiled = Vec::with_capacity(edges.len());
    for (index, edge) in edges.iter().enumerate() {
        let gate = match &edge.gate {
            None => None,
            Some(gate) => Some(CompiledGate::compile(
                &gate.function,
                gate.params.clone().unwrap_or_else(|| serde_json::json!({})),
            )?),
        };
        compiled.push(CompiledEdge {
            index,
            from: edge.from.clone(),
            to_node: edge.to_node.clone(),
            to_port: edge.to_input_port.clone(),
            gate,
        });
    }
    Ok(compiled)
}

/// 子图编译（v1 不执行：继承父图 metadata/graph_hash/dsl_json，entry_points 为空）。
///
/// 为什么 `entry_points` 为空：v1 子图节点在派发前被拒绝（04 §4.4.4），
/// 编译产物只需覆盖类型/校验信息；真实的入口映射属于 v1.1 的子图执行语义。
fn compile_subgraph(
    subgraph: &SubgraphDefinition,
    graph_hash: &str,
    dsl_json: &str,
    metadata: &GraphMetadata,
) -> Result<CompiledGraph, CompileError> {
    let (nodes, node_order) = compile_nodes(&subgraph.nodes)?;
    let edges = compile_edges(&subgraph.edges)?;
    let edge_index = build_edge_index(&edges);
    Ok(CompiledGraph {
        graph_hash: graph_hash.to_string(),
        metadata: metadata.clone(),
        dsl_json: dsl_json.to_string(),
        nodes,
        node_order,
        edges,
        entry_points: Vec::new(),
        subgraphs: BTreeMap::new(),
        edge_index,
    })
}

/// 由边表构建索引；插入顺序即升序，保证确定性。
fn build_edge_index(edges: &[CompiledEdge]) -> EdgeIndex {
    let mut index = EdgeIndex::default();
    for edge in edges {
        index
            .outgoing
            .entry(edge.from.clone())
            .or_default()
            .push(edge.index);
        index
            .incoming_by_node
            .entry(edge.to_node.clone())
            .or_default()
            .push(edge.index);
        index
            .incoming
            .insert((edge.to_node.clone(), edge.to_port.clone()), edge.index);
    }
    index
}
