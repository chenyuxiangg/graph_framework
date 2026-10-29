//! 冻结公开 API 的领域类型（cli 与内核桥接层直接依赖）。
//!
//! 枚举字符串与 06 §6.2.11 的存储值一致（状态为 CamelCase）；字段语义见
//! `doc/spec-v1.2/01-领域模型与状态机.md` §1.5 与 00 §0.3。存储层附加列不进入
//! 这些类型（06 §6.12.2）。

use serde_json::Value;

use crate::error::TraceError;

/// 实例状态（00 §0.3；存储值 = CamelCase 字面量）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum InstanceStatus {
    /// 已创建，等待输入。
    Pending,
    /// 输入就绪，等待调度。
    Ready,
    /// 已派发执行。
    Running,
    /// 成功（终态）。
    Completed,
    /// 失败（终态）。
    Crashed,
    /// 超时（终态；v2 = kill 进程组，见 architecture §8 D1）。
    TimedOut,
    /// 被取消（终态）。
    Cancelled,
    /// 入边全部被门控拦截，永不执行（终态）。
    Skipped,
}

impl InstanceStatus {
    /// 存储/展示用稳定字符串（06 §6.2.11）。
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "Pending",
            Self::Ready => "Ready",
            Self::Running => "Running",
            Self::Completed => "Completed",
            Self::Crashed => "Crashed",
            Self::TimedOut => "TimedOut",
            Self::Cancelled => "Cancelled",
            Self::Skipped => "Skipped",
        }
    }

    /// 把存储值解析回枚举；未知值返回 `None`。
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "Pending" => Some(Self::Pending),
            "Ready" => Some(Self::Ready),
            "Running" => Some(Self::Running),
            "Completed" => Some(Self::Completed),
            "Crashed" => Some(Self::Crashed),
            "TimedOut" => Some(Self::TimedOut),
            "Cancelled" => Some(Self::Cancelled),
            "Skipped" => Some(Self::Skipped),
            _ => None,
        }
    }

    /// 是否为终态（Completed/Crashed/TimedOut/Cancelled/Skipped）。
    pub const fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Completed | Self::Crashed | Self::TimedOut | Self::Cancelled | Self::Skipped
        )
    }
}

/// run 状态（00 §0.3；存储值 = CamelCase 字面量）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RunStatus {
    /// 运行中。
    Running,
    /// 已暂停。
    Paused,
    /// 静默成功结束（终态）。
    Completed,
    /// 冷峻失败（终态）。
    Failed,
    /// 用户取消（终态）。
    Cancelled,
}

impl RunStatus {
    /// 存储/展示用稳定字符串（06 §6.2.11）。
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Running => "Running",
            Self::Paused => "Paused",
            Self::Completed => "Completed",
            Self::Failed => "Failed",
            Self::Cancelled => "Cancelled",
        }
    }

    /// 把存储值解析回枚举；未知值返回 `None`。
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "Running" => Some(Self::Running),
            "Paused" => Some(Self::Paused),
            "Completed" => Some(Self::Completed),
            "Failed" => Some(Self::Failed),
            "Cancelled" => Some(Self::Cancelled),
            _ => None,
        }
    }

    /// 是否为终态（Completed/Failed/Cancelled）。
    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Failed | Self::Cancelled)
    }
}

/// L2 记录模式（architecture §5.3：`--record=none|hash|full`，默认 `hash`）。
///
/// 存储值取 CLI 字面量小写（`none`/`hash`/`full`）；与 06 §6.2.11 的
/// `RunStatus`/`InstanceStatus` CamelCase 不同，这是 v2 新增列的约定。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum RecordMode {
    /// 不记录输出（仅 trace 骨架）。
    None,
    /// 记录输出哈希（默认）。
    #[default]
    Hash,
    /// 记录输出全文（L2 重放所需）。
    Full,
}

impl RecordMode {
    /// 存储/CLI 用稳定字符串（小写字面量）。
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Hash => "hash",
            Self::Full => "full",
        }
    }

    /// 解析 CLI 字面量；同时兼容枚举名（`None`/`Hash`/`Full`），未知返回 `None`。
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "none" | "None" => Some(Self::None),
            "hash" | "Hash" => Some(Self::Hash),
            "full" | "Full" => Some(Self::Full),
            _ => None,
        }
    }
}

/// run 级指标（00 §0.3 `RunMetrics` + architecture §8.2 `killed_workers`）。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RunMetrics {
    /// 因流式通道背压丢弃的 `Streaming` 事件数。
    pub streaming_dropped: u64,
    /// 被放弃任务数；v2 恒 0（D1，保留字段兼容）。
    pub abandoned_tasks: u64,
    /// trace 无损发送累计等待（毫秒）。
    pub trace_backpressure: u64,
    /// 因超时/取消被杀掉的 worker 数（architecture §8.2）。
    pub killed_workers: u64,
}

/// `RunFinished.error`：领域错误码 + 人类可读消息（00 §0.3 `TraceError`）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TraceFailure {
    /// 00 §0.4 领域错误码（如 `NODE_TIMEOUT`）。
    pub code: String,
    /// 人类可读消息。
    pub message: String,
}

/// 流式事件（00 §0.3 `StreamingEvent`；05 §5.8 的 emit 载荷）。
#[derive(Debug, Clone, PartialEq)]
pub enum StreamingEvent {
    /// token 增量（可丢；写线程 100ms/4096 字符聚合）。
    TokenDelta {
        /// 增量文本。
        text: String,
    },
    /// reasoning 增量（与 token 各自独立缓冲）。
    ReasoningDelta {
        /// 增量文本。
        text: String,
    },
    /// 工具调用开始（立即成段）。
    ToolCallStart {
        /// 调用 id。
        call_id: String,
        /// 工具名。
        tool_name: String,
        /// 参数摘要（05 §5.8.5）。
        args_summary: Value,
    },
    /// 工具调用结束（立即成段；`ok=false` 时 severity=warn）。
    ToolCallEnd {
        /// 调用 id。
        call_id: String,
        /// 工具名。
        tool_name: String,
        /// 是否成功。
        ok: bool,
        /// 结果摘要。
        result_summary: Option<Value>,
    },
    /// 进度（立即成段）。
    Progress {
        /// 0..99 的整数百分比。
        percent: u8,
        /// 进度消息。
        message: String,
    },
}

impl StreamingEvent {
    /// 存储用 variant 字符串（06 §6.2.9 `kind`）。
    pub const fn variant(&self) -> &'static str {
        match self {
            Self::TokenDelta { .. } => "token",
            Self::ReasoningDelta { .. } => "reasoning",
            Self::ToolCallStart { .. } => "tool_call_start",
            Self::ToolCallEnd { .. } => "tool_call_end",
            Self::Progress { .. } => "progress",
        }
    }
}

/// 实例快照（内核 `InstanceRecord` 的存储投影）。
///
/// `created_at` 仅在本类型内保留：06 §6.2.5 的 `instances` 表**没有** created_at
/// 列（只有 `created_seq`，M2 允许 NULL），时间线以 `events` 为准。
#[derive(Debug, Clone, PartialEq)]
pub struct InstanceSnapshot {
    /// 所属 run。
    pub run_id: String,
    /// 实例 id（UUIDv7）。
    pub instance_id: String,
    /// 节点 id。
    pub node_id: String,
    /// 节点类型字面量（`start`/`end`/`python_node`/`agent_call`/`subgraph`）。
    pub node_type: String,
    /// 尝试序号（自环/重试 +1）。
    pub attempt: u32,
    /// 当前状态。
    pub status: InstanceStatus,
    /// 触发时 buffer 快照。
    pub input: Option<Value>,
    /// 节点输出。
    pub output: Option<Value>,
    /// `NodeError`（JSON）。
    pub error: Option<Value>,
    /// 冗余错误码（00 §0.4）。
    pub error_code: Option<String>,
    /// 实例创建时刻（RFC3339 毫秒 Z；本表不落库，见类型文档）。
    pub created_at: String,
    /// 开始执行时刻。
    pub started_at: Option<String>,
    /// 进入终态时刻。
    pub finished_at: Option<String>,
}

/// 状态迁移记录（01 §1.5 `TransitionRecord`，不含 `seq`：权威序 = events.seq）。
#[derive(Debug, Clone, PartialEq)]
pub struct TransitionRecord {
    /// 所属 run。
    pub run_id: String,
    /// 实例 id。
    pub instance_id: String,
    /// 节点 id。
    pub node_id: String,
    /// 尝试序号。
    pub attempt: u32,
    /// 迁移前状态。
    pub from: InstanceStatus,
    /// 迁移后状态。
    pub to: InstanceStatus,
    /// 迁移原因（"inputs ready"/"dispatch"/"gate blocked"/…）。
    pub reason: Option<String>,
    /// 迁移时刻（RFC3339 毫秒 Z）。
    pub ts: String,
}

/// 门控结果记录（01 §1.5 `GateResultRecord`）。
#[derive(Debug, Clone, PartialEq)]
pub struct GateResultRecord {
    /// 所属 run。
    pub run_id: String,
    /// 发布者实例 id。
    pub instance_id: String,
    /// 发布者节点 id。
    pub node_id: String,
    /// 边索引（编译期 `edges` 数组下标）。
    pub edge_index: i64,
    /// 目标节点 id。
    pub to_node: String,
    /// 目标端口名。
    pub to_port: String,
    /// 门控符号；`None` = 恒真（无门控边）。
    pub gate_fn: Option<String>,
    /// 是否放行。
    pub passed: bool,
    /// gate 异常/非 bool 的人类可读信息。
    pub error: Option<String>,
    /// 记录时刻（RFC3339 毫秒 Z）。
    pub ts: String,
}

/// `RunStarted` 输入（cli 适配层由内核 run 启动事件映射）。
#[derive(Debug, Clone, PartialEq)]
pub struct RunStartedInfo {
    /// run id（UUIDv7）。
    pub run_id: String,
    /// 图内容哈希（blake3 前 16 hex）。
    pub graph_hash: String,
    /// 图名（`metadata.name`）。
    pub graph_name: String,
    /// 规范化 DSL JSON 原文（06 §6.2.3）。
    pub dsl_json: String,
    /// 用户 id。
    pub user_id: String,
    /// entry 输入快照（`RunSpec.entry_inputs`）。
    pub entry_inputs: Value,
    /// 可复现随机种子（architecture §5.1）。
    pub seed: u64,
    /// L2 记录模式。
    pub record_mode: RecordMode,
    /// 拥有者进程号（崩溃标定用）。
    pub owner_pid: u32,
    /// 拥有者主机名。
    pub owner_host: String,
    /// run 启动时刻（RFC3339 毫秒 Z）。
    pub started_at: String,
}

/// `RunFinished` 输入（终态与指标，同事务写 events + runs）。
#[derive(Debug, Clone, PartialEq)]
pub struct RunFinishedInfo {
    /// run id。
    pub run_id: String,
    /// 终态（Completed/Failed/Cancelled；I5 恰好一次）。
    pub status: RunStatus,
    /// run_output 摘要（06 §6.2.12；失败/中断为 `None`）。
    pub output_summary: Option<Value>,
    /// 失败领域错误（Completed/Cancelled 为 `None`）。
    pub error: Option<TraceFailure>,
    /// run 指标。
    pub metrics: RunMetrics,
    /// 终态时刻（RFC3339 毫秒 Z）。
    pub at: String,
}

/// 运行期记录的输出（L2 重放基础，architecture §5.2）。
#[derive(Debug, Clone, PartialEq)]
pub struct RecordedOutput {
    /// 所属 run。
    pub run_id: String,
    /// 实例 id。
    pub instance_id: String,
    /// 节点 id。
    pub node_id: String,
    /// 尝试序号。
    pub attempt: u32,
    /// 输出内容哈希（blake3 前 16 hex，`--record=hash|full`）。
    pub output_hash: String,
    /// 输出全文（仅 `--record=full`；`hash` 模式为 `None`）。
    pub output_json: Option<Value>,
    /// 记录时刻（RFC3339 毫秒 Z）。
    pub recorded_at: String,
}

/// LLM 调用记录（L2 重放基础，architecture §5.2）。
#[derive(Debug, Clone, PartialEq)]
pub struct LlmCallRecord {
    /// 所属 run。
    pub run_id: String,
    /// 实例 id。
    pub instance_id: String,
    /// 实例内 LLM 调用序号（0 起）。
    pub call_index: u32,
    /// 请求全文。
    pub request_json: Value,
    /// 响应全文（成功时）。
    pub response_json: Option<Value>,
    /// 调用状态（如 `ok`/`error`/`timeout`，由 Harness 约定）。
    pub status: String,
    /// 记录时刻（RFC3339 毫秒 Z）。
    pub recorded_at: String,
}

/// 写线程输入（cli 适配层把内核事件映射成这些值；签名冻结）。
#[derive(Debug, Clone, PartialEq)]
pub enum TraceInput {
    /// run 启动（同事务写 graphs + runs + events）。
    RunStarted(RunStartedInfo),
    /// 仅 upsert 实例快照（不产生 events 行）。
    InstanceSnapshot(InstanceSnapshot),
    /// 状态迁移（同事务写 events + transitions + instances upsert）。
    Transition {
        /// 迁移记录。
        record: TransitionRecord,
        /// 迁移后的实例快照。
        snapshot: InstanceSnapshot,
    },
    /// 门控结果（同事务写 events + gate_results）。
    GateResult(GateResultRecord),
    /// 实例超时（仅 events 信封，payload 含 `elapsed_ms`）。
    NodeTimedOut {
        /// 所属 run。
        run_id: String,
        /// 实例 id。
        instance_id: String,
        /// 实际耗时（毫秒）。
        elapsed_ms: u64,
        /// 事件时刻（RFC3339 毫秒 Z）。
        at: String,
    },
    /// 任务被放弃（v2 不产生，D1；仅保留兼容；severity=warn）。
    NodeAbandoned {
        /// 所属 run。
        run_id: String,
        /// 实例 id。
        instance_id: String,
        /// 事件时刻（RFC3339 毫秒 Z）。
        at: String,
    },
    /// 流式事件（可丢；写线程聚合后落段）。
    Streaming {
        /// 所属 run。
        run_id: String,
        /// 实例 id。
        instance_id: String,
        /// 节点 id；`None` 时写线程按实例快照回填。
        node_id: Option<String>,
        /// 流式变体。
        event: StreamingEvent,
        /// 事件时刻（RFC3339 毫秒 Z）。
        at: String,
    },
    /// run 终态（同事务写 events + update runs）。
    RunFinished(RunFinishedInfo),
    /// 记录输出（仅 recorded_outputs 行）。
    RecordedOutput(RecordedOutput),
    /// LLM 调用记录（仅 llm_calls 行）。
    LlmCall(LlmCallRecord),
}

/// 把 `Option<&Value>` 序列化为可空 TEXT（写线程内部使用）。
pub(crate) fn json_opt(value: Option<&Value>) -> Result<Option<String>, TraceError> {
    value
        .map(|value| {
            serde_json::to_string(value).map_err(|err| TraceError::Serialize(err.to_string()))
        })
        .transpose()
}

/// 把 `Value` 序列化为 TEXT。
pub(crate) fn json_req(value: &Value) -> Result<String, TraceError> {
    serde_json::to_string(value).map_err(|err| TraceError::Serialize(err.to_string()))
}

/// 解析可空 JSON TEXT。
pub(crate) fn parse_json_opt(text: Option<String>) -> Result<Option<Value>, TraceError> {
    text.map(|text| parse_json(&text)).transpose()
}

/// 解析 JSON TEXT。
pub(crate) fn parse_json(text: &str) -> Result<Value, TraceError> {
    serde_json::from_str(text).map_err(|err| TraceError::Serialize(err.to_string()))
}
