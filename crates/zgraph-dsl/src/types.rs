//! DSL 的编译前类型（schema 1.0 · spec v1.2 最终版）。
//!
//! 权威定义：`doc/spec-v1.2/02-DSL与校验.md` §2.2（TypeScript 权威类型 + Rust serde 对照）。
//! 类型只在通过 R1–R4 结构校验后由 loader 构造；本文件不承担校验职责。

use std::collections::BTreeMap;

use serde::Serialize;
use serde_json::Value;

/// DSL schema 版本（`version` 字段唯一合法值）。
pub const DSL_SCHEMA_VERSION: &str = "1.0";

/// 节点类型（00 §0.3 `NodeType`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeType {
    /// 入口标记节点。
    Start,
    /// 终点节点。
    End,
    /// Python 节点（v2 在 worker 子进程执行，加载期只校验格式）。
    PythonNode,
    /// Agent 调用节点。
    AgentCall,
    /// 子图节点（v1 只校验、运行期派发前拒绝）。
    Subgraph,
}

impl NodeType {
    /// 全部 5 种节点类型（错误 message 与测试用）。
    pub const ALL: [Self; 5] = [
        Self::Start,
        Self::End,
        Self::PythonNode,
        Self::AgentCall,
        Self::Subgraph,
    ];

    /// JSON 中的 snake_case 名称。
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Start => "start",
            Self::End => "end",
            Self::PythonNode => "python_node",
            Self::AgentCall => "agent_call",
            Self::Subgraph => "subgraph",
        }
    }

    /// 从 JSON 字符串解析（未知返回 `None`）。
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.as_str() == name)
    }
}

/// 端口数据类型（02 §2.2.2 `PortType`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PortType {
    /// 字符串。
    String,
    /// 数字。
    Number,
    /// 布尔。
    Boolean,
    /// 对象。
    Object,
    /// 任意类型。
    Any,
}

impl PortType {
    /// 全部 5 种端口类型。
    pub const ALL: [Self; 5] = [
        Self::String,
        Self::Number,
        Self::Boolean,
        Self::Object,
        Self::Any,
    ];

    /// JSON 中的 snake_case 名称。
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::String => "string",
            Self::Number => "number",
            Self::Boolean => "boolean",
            Self::Object => "object",
            Self::Any => "any",
        }
    }

    /// 从 JSON 字符串解析（未知返回 `None`）。
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.as_str() == name)
    }
}

/// 输入就绪策略（02 §2.2.2 `trigger`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Trigger {
    /// 全部 required 端口到齐（缺省）。
    #[default]
    All,
    /// 任一 required 端口到齐。
    Any,
}

impl Trigger {
    /// JSON 中的 snake_case 名称。
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::All => "all",
            Self::Any => "any",
        }
    }

    /// 从 JSON 字符串解析（未知返回 `None`）。
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "all" => Some(Self::All),
            "any" => Some(Self::Any),
            _ => None,
        }
    }
}

/// Python 节点执行类别（02 §2.2.1 `exec_class`，缺省 `io`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecClass {
    /// CPU 型。
    Cpu,
    /// I/O 型（缺省）。
    #[default]
    Io,
}

impl ExecClass {
    /// JSON 中的 snake_case 名称。
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Cpu => "cpu",
            Self::Io => "io",
        }
    }

    /// 从 JSON 字符串解析（未知返回 `None`）。
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "cpu" => Some(Self::Cpu),
            "io" => Some(Self::Io),
            _ => None,
        }
    }
}

/// 图元数据（02 §2.2.2 `GraphMetadata`）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct GraphMetadata {
    /// 图名（必填、非空）。
    pub name: String,
    /// 描述（可选，不参与 graph_hash）。
    pub description: Option<String>,
    /// 作者（可选，不参与 graph_hash）。
    pub author: Option<String>,
    /// 创建时间（可选，UTC RFC3339，不参与 graph_hash）。
    pub created_at: Option<String>,
}

/// 输入端口声明（02 §2.2.2 `InputPort`）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct InputPort {
    /// 端口名（节点内唯一、Python 标识符）。
    pub name: String,
    /// 端口类型。
    pub port_type: PortType,
    /// 是否 required。
    pub required: bool,
    /// 描述（可选）。
    pub description: Option<String>,
}

/// 输出端口声明（02 §2.2.2 `OutputPort`，v1 仅保留声明）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OutputPort {
    /// 端口名（节点内唯一）。
    pub name: String,
    /// 端口类型。
    pub port_type: PortType,
    /// 描述（可选）。
    pub description: Option<String>,
}

/// 重试配置（02 §2.2.2 `RetryConfig`）。
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize)]
pub struct RetryConfig {
    /// 最大尝试次数（≥1；缺省 `DEFAULT_MAX_ATTEMPTS`）。
    pub max_attempts: Option<u32>,
}

/// `agent_call` 节点配置（02 §2.2.2 `AgentCallConfig`）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AgentCallConfig {
    /// Agent 资产 ID。
    pub agent_id: String,
    /// 端口名 → Agent 输入字段（缺省 identity 映射）。
    pub input_mapping: BTreeMap<String, String>,
    /// 覆盖 AgentConfig 的 tool 轮次上限（1..=50）。
    pub max_iterations: Option<u32>,
    /// 实例级超时（毫秒，≥1）。
    pub timeout_ms: Option<u64>,
    /// 重试配置。
    pub retry: Option<RetryConfig>,
}

/// `python_node` 节点配置（02 §2.2.2 `PythonNodeConfig`）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PythonNodeConfig {
    /// 点分 Python 模块路径。
    pub module: String,
    /// 模块内函数名。
    pub function: String,
    /// 执行类别（缺省 `io`）。
    pub exec_class: Option<ExecClass>,
    /// 实例级超时（毫秒，≥1）。
    pub timeout_ms: Option<u64>,
    /// 重试配置。
    pub retry: Option<RetryConfig>,
}

/// `subgraph` 节点配置（02 §2.2.2 `SubgraphConfig`）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SubgraphConfig {
    /// 被引用的子图 ID。
    pub subgraph_id: String,
}

/// 节点配置联合体（按 `type` 区分）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeConfig {
    /// `agent_call`。
    AgentCall(AgentCallConfig),
    /// `python_node`。
    PythonNode(PythonNodeConfig),
    /// `subgraph`。
    Subgraph(SubgraphConfig),
    /// `start` / `end`（`config` 必须为 `{}`）。
    StartEnd,
}

impl NodeConfig {
    /// 该节点类型对应的 Handler 类别名（文档/测试用）。
    pub const fn kind_name(&self) -> &'static str {
        match self {
            Self::AgentCall(_) => "agent_call",
            Self::PythonNode(_) => "python_node",
            Self::Subgraph(_) => "subgraph",
            Self::StartEnd => "start_end",
        }
    }
}

/// 节点声明（02 §2.2.2 `GraphNode`）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct GraphNode {
    /// 图内唯一 ID。
    pub id: String,
    /// 节点类型。
    pub node_type: NodeType,
    /// 输入端口声明（保持 DSL 顺序）。
    pub inputs: Vec<InputPort>,
    /// 就绪策略（缺省 `all`）。
    pub trigger: Trigger,
    /// 节点配置。
    pub config: NodeConfig,
    /// 输出端口声明（v1 保留，不参与路由）。
    pub outputs: Vec<OutputPort>,
}

/// 门控定义（02 §2.2.2 `GateDefinition`）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct GateDefinition {
    /// v1：内置 gate op 名（`check_output` / `compare` / `manual`）。
    pub function: String,
    /// gate 参数（缺省视为 `{}`；保留"作者是否写了"的差异给 graph_hash）。
    pub params: Option<Value>,
}

/// 边级 `gate=false` 策略（DR-004 D1 / W2a0；缺省 `Settle` 保持 v0.1.0 语义）。
///
/// v0.1.0 缺省语义为 `settle`：gate 不通过时目标实例结算为 `Skipped`。W2a 新增
/// opt-in 的 `defer`：gate 不通过时不结算目标（deferred），供返工环使用。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OnGateFalse {
    /// gate=false 时结算目标为 `Skipped`（缺省，v0.1.0 语义）。
    #[default]
    Settle,
    /// gate=false 时目标保持未决（deferred，W2a 返工语义）。
    Defer,
}

impl OnGateFalse {
    /// 全部 2 种策略（文档/测试用）。
    pub const ALL: [Self; 2] = [Self::Settle, Self::Defer];

    /// JSON 中的 snake_case 名称。
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Settle => "settle",
            Self::Defer => "defer",
        }
    }

    /// 从 JSON 字符串解析（未知返回 `None`）。
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|policy| policy.as_str() == name)
    }
}

/// 边声明（02 §2.2.2 `GraphEdge`，v0.2 增 `on_gate_false`）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct GraphEdge {
    /// 源节点 ID。
    pub from: String,
    /// 目标节点 ID。
    pub to_node: String,
    /// 目标节点输入端口名。
    pub to_input_port: String,
    /// 可选门控。
    pub gate: Option<GateDefinition>,
    /// 边级 `gate=false` 策略；`None` = 作者未写 = 缺省 [`OnGateFalse::Settle`]。
    ///
    /// 保留"作者是否写了"的语义：缺省值不回写 DSL、不参与 `graph_hash`
    /// （02 §2.8.3：不补齐默认值）；语义默认值在编译期补到产物。
    pub on_gate_false: Option<OnGateFalse>,
    /// v1 保留字段：必须缺省；出现即 `DSL_UNSUPPORTED_FIELD`（不看取值，保留原始值）。
    pub from_output_port: Option<Value>,
}

/// 子图定义（02 §2.2.2 `SubgraphDefinition`；v1 只校验不执行）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SubgraphDefinition {
    /// 子图 ID（图内唯一）。
    pub id: String,
    /// 子图内节点。
    pub nodes: Vec<GraphNode>,
    /// 子图内边。
    pub edges: Vec<GraphEdge>,
    /// 外部输入端口名 → `"<node_id>.<port>"`。
    pub input_mapping: BTreeMap<String, String>,
    /// 外部输出端口名 → `"<end_node_id>.<port>"`。
    pub output_mapping: BTreeMap<String, String>,
}

/// 通过 R1–R4 结构校验的图定义（编译前结构）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct GraphDefinition {
    /// schema 版本（恒为 [`DSL_SCHEMA_VERSION`]）。
    pub version: String,
    /// 元数据。
    pub metadata: GraphMetadata,
    /// 入口节点 ID 列表（保持 DSL 顺序）。
    pub entry_points: Vec<String>,
    /// 节点列表（保持 DSL 顺序）。
    pub nodes: Vec<GraphNode>,
    /// 边列表（顺序即 `EdgeIndex`）。
    pub edges: Vec<GraphEdge>,
    /// 子图定义列表。
    pub subgraphs: Vec<SubgraphDefinition>,
}

impl GraphDefinition {
    /// 节点 ID → 节点引用。
    pub fn node_map(&self) -> BTreeMap<&str, &GraphNode> {
        self.nodes
            .iter()
            .map(|node| (node.id.as_str(), node))
            .collect()
    }

    /// 节点数。
    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    /// 边数。
    pub fn edge_count(&self) -> usize {
        self.edges.len()
    }

    /// 子图数。
    pub fn subgraph_count(&self) -> usize {
        self.subgraphs.len()
    }
}
