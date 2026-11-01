//! 内核公共类型、错误码与时间戳约定。
//!
//! 语义基准：`doc/spec-v1.2/00-总纲.md` §0.2–§0.4（类型/错误码）、§0.7（术语消歧）；
//! 落地偏差：`doc/architecture-v2.md` §8 **D1**（超时 = kill，`abandoned_tasks` 恒 0）、
//! **D10**（新增 `NodeError::NodeWorker` 变体）与 §8.2（4 个 worker 错误码）。
//!
//! 所有时间戳均为 UTC RFC3339 毫秒精度且以 `Z` 结尾
//! （`chrono::DateTime::to_rfc3339_opts(SecondsFormat::Millis, true)`）。

use std::fmt;

use serde_json::Value;
use thiserror::Error;

/// 图内节点 ID（DSL 声明，图内唯一）。
pub type NodeId = String;

/// 节点输入/输出端口名（节点内唯一）。
pub type PortName = String;

/// run ID（UUIDv7，全局唯一、时间有序）。
pub type RunId = String;

/// 实例 ID（UUIDv7，实例创建时生成）。
pub type InstanceId = String;

/// 同一节点重建实例时的尝试序号（自环/重试从 0 起 +1）。
pub type Attempt = u32;

/// `foreach` 展开内元素下标（W4a；`0..N-1`，非 region 节点恒 0）。
///
/// 与 [`NodeId`] 组成 item 实例定位键 `(NodeId, ItemIndex)`（01 §1.2.5）。
pub type ItemIndex = u32;

/// 一次 `foreach` 展开事实的标识（UUIDv7；W4a / DR-007 §6.1）。
pub type ExpansionId = String;

/// 边的稳定标识：`CompiledGraph.edges` 数组下标。
///
/// **命名说明**：任务给定的冻结 API 同时列出 `pub type EdgeIndex = usize` 与
/// `pub struct EdgeIndex`（边索引结构，01 §1.1.2），二者在 Rust 中不能同名共存。
/// 这里保留**结构体** `EdgeIndex`（`CompiledGraph::edge_index()` 的返回类型，
/// 语义更重），数值别名改用 `EdgeIdx`（仍是 `usize`，字段/签名语义不变）。
pub type EdgeIdx = usize;

// ---------------------------------------------------------------------------
// 错误码常量（00 §0.4 与 architecture §8.2；对外契约，CLI/trace/测试依赖）
// ---------------------------------------------------------------------------

/// 入口配置非法：`entry_points` 为空/不存在，或 `entry_inputs` 含多余节点/端口。
pub const CODE_RUN_ENTRY_INVALID: &str = "RUN_ENTRY_INVALID";

/// 入口必填端口未被 `entry_inputs` 覆盖。
pub const CODE_RUN_ENTRY_INPUT_MISSING: &str = "RUN_ENTRY_INPUT_MISSING";

/// 状态机非法迁移（框架 bug，fail-fast）。
pub const CODE_RUN_ILLEGAL_TRANSITION: &str = "RUN_ILLEGAL_TRANSITION";

/// 投递到非自环的终态实例（框架 bug，fail-fast）。
pub const CODE_RUN_DELIVERY_TO_TERMINAL: &str = "RUN_DELIVERY_TO_TERMINAL";

/// 静默且仍有永不就绪实例（超过 `deadlock_grace_ms`）。
pub const CODE_RUN_DEADLOCK: &str = "RUN_DEADLOCK";

/// 用户取消。
pub const CODE_RUN_CANCELLED: &str = "RUN_CANCELLED";

/// `foreach.items_port` 的值不是 JSON array（W4a；展开前 run 级失败，无 item 实例）。
pub const CODE_RUN_FOREACH_ITEMS_NOT_ARRAY: &str = "RUN_FOREACH_ITEMS_NOT_ARRAY";

/// 展开元素数 `N` 超过 `foreach.max_items`（W4a；展开前 run 级失败）。
pub const CODE_RUN_FOREACH_ITEMS_EXCEEDED: &str = "RUN_FOREACH_ITEMS_EXCEEDED";

/// `key_field` 派生的非 NULL `item_key` 重复（W4a；展开前 run 级失败）。
pub const CODE_RUN_FOREACH_ITEM_KEY_DUPLICATE: &str = "RUN_FOREACH_ITEM_KEY_DUPLICATE";

/// region 某个 item 的出口 `E` 变 `Skipped`，join 无法完成（W4a；run 级冷峻失败）。
pub const CODE_RUN_FOREACH_ITEM_INCOMPLETE: &str = "RUN_FOREACH_ITEM_INCOMPLETE";

/// trace 写端不可恢复错误（fail-stop，06 §6.4.5）。
///
/// 属 **CLI/storage 命名空间**（06 §6.12.3 D3），不是 00 §0.4 领域错误码：
/// 只在**内存态** `RunOutcome.error` 中携带，`RunFinished.error` 与
/// `runs.error_code` 保持 NULL（CLI 据此映射 `error[STORAGE_ERROR]` + 退出码 4）。
pub const CODE_STORAGE_ERROR: &str = "STORAGE_ERROR";

/// 实例超时（D1：kill 语义）。
pub const CODE_NODE_TIMEOUT: &str = "NODE_TIMEOUT";

/// v1 不支持的节点类型（`subgraph`）在派发前被拒绝。
pub const CODE_NODE_TYPE_UNSUPPORTED: &str = "NODE_TYPE_UNSUPPORTED";

/// 节点主动抛出的业务错误。
pub const CODE_NODE_BUSINESS_ERROR: &str = "NODE_BUSINESS_ERROR";

/// Python 未捕获异常。
pub const CODE_NODE_PYTHON_EXCEPTION: &str = "NODE_PYTHON_EXCEPTION";

/// 框架不变量违反 / handler panic。
pub const CODE_NODE_INTERNAL: &str = "NODE_INTERNAL";

/// 单次激活放行的出边数超过节点 `max_fanout`（W2b / DR-004 §3）。
///
/// 运行期防御性校验：静态层（DSL R16）已保证合法图 `out_degree <= max_fanout`，
/// 本码只兜底程序化构图 / 动态扇出（W4）。
pub const CODE_NODE_FANOUT_EXCEEDED: &str = "NODE_FANOUT_EXCEEDED";

/// 非 Harness 资产加载失败。
pub const CODE_NODE_CONFIG_ERROR: &str = "NODE_CONFIG_ERROR";

/// worker `hello` 超时 / 协议版本不匹配 / 缺 `sdk_version`（D10/§8.2）。
pub const CODE_NODE_WORKER_HANDSHAKE: &str = "NODE_WORKER_HANDSHAKE";

/// 非法帧 / exit 0 无 result / result 后不退出 / 半行帧 / 超 8 MiB（§8.2）。
pub const CODE_NODE_WORKER_PROTOCOL: &str = "NODE_WORKER_PROTOCOL";

/// worker 启动时 `module/function` 导入失败或签名不符（D7/§8.2）。
pub const CODE_NODE_WORKER_IMPORT: &str = "NODE_WORKER_IMPORT";

/// worker 被信号杀死 / 非约定 exit code（§8.2）。
pub const CODE_NODE_WORKER_CRASH: &str = "NODE_WORKER_CRASH";

/// Harness 资产加载失败。
pub const CODE_HARNESS_CONFIG_LOAD: &str = "HARNESS_CONFIG_LOAD";

/// LLM 调用了未注册工具。
pub const CODE_HARNESS_UNKNOWN_TOOL: &str = "HARNESS_UNKNOWN_TOOL";

/// 超过 tool 轮次上限。
pub const CODE_HARNESS_MAX_ITERATIONS: &str = "HARNESS_MAX_ITERATIONS";

/// provider 错误重试耗尽。
pub const CODE_HARNESS_LLM_ERROR: &str = "HARNESS_LLM_ERROR";

/// 单次 LLM 调用超时且重试耗尽。
pub const CODE_HARNESS_LLM_TIMEOUT: &str = "HARNESS_LLM_TIMEOUT";

/// 工具执行失败（喂回 LLM，不终止；00 §0.4 统一为 `TOOL_ERROR`）。
pub const CODE_HARNESS_TOOL_ERROR: &str = "TOOL_ERROR";

/// 上下文预算裁剪后仍溢出（M4 追加；00 §0.4 已有该码，而 §0.3 七值缺该 kind，见 05 §5.13）。
pub const CODE_HARNESS_CONTEXT_OVERFLOW: &str = "HARNESS_CONTEXT_OVERFLOW";

/// MCP server/工具/开关错误（v0.3/V3 追加，DR-010 §8；00 §0.4 append-only）。
///
/// `detail.reason` 为闭集（`not_allowed`/`server_not_found`/`allow_invalid`/
/// `name_conflict`/`env_undefined`/`config_invalid`/`init_failed`/`protocol_version`/
/// `tool_not_found`/`tool_not_allowed`/`input_required_unsupported`/`timeout` 等）。
pub const CODE_HARNESS_MCP_ERROR: &str = "HARNESS_MCP_ERROR";

/// 被引用的 skill 存在但格式/路径非法（v0.3/V2 追加，DR-009 §6；00 §0.4 append-only）。
///
/// 缺失引用走 [`CODE_HARNESS_CONFIG_LOAD`]`{reason:"skill_not_found"}`，不新增
/// `HARNESS_SKILL_NOT_FOUND`。
pub const CODE_HARNESS_SKILL_INVALID: &str = "HARNESS_SKILL_INVALID";

/// 记忆检索/写入失败（降级告警，不终止）。
pub const CODE_HARNESS_MEMORY_DEGRADED: &str = "HARNESS_MEMORY_DEGRADED";

/// 外部 harness（子进程 adapter）spawn/握手/协议/崩溃/超时（v0.3/V4 追加，DR-011 §6）。
///
/// `detail.reason` 为闭集（`spawn_failed`/`handshake`/`version`/`protocol`/`result_invalid`/
/// `line_too_large`/`output_limit`/`event_limit`/`exit_nonzero`/`timeout`）；**无独立
/// `HARNESS_PLUGIN_TIMEOUT` 码**（节点超时仍 `NODE_TIMEOUT`，协议级超时归本码）。
pub const CODE_HARNESS_PLUGIN_ERROR: &str = "HARNESS_PLUGIN_ERROR";

/// 人工决策值（W3a0；`builtin.human` 节点 output 的 `decision` 字段 /
/// `human_decisions.decision` 列；DR-006 §4）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HumanDecision {
    /// 批准：实例正常 `Completed`；后续路由交给图。
    Approved,
    /// 驳回：同样是正常 `Completed`（**不隐式 Failed**）；路由交给图。
    Rejected,
}

impl HumanDecision {
    /// 全部 2 种决策（文档/测试用）。
    pub const ALL: [Self; 2] = [Self::Approved, Self::Rejected];

    /// 与 00 §0.3 / `human_decisions.decision` CHECK 一致的字符串名。
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Approved => "approved",
            Self::Rejected => "rejected",
        }
    }
}

/// 实例状态（00 §0.3；v0.2/W3a 全部可能产生的状态）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum InstanceStatus {
    /// 已创建，等待输入。
    Pending,
    /// 输入就绪，等待调度。
    Ready,
    /// 已派发执行。
    Running,
    /// 输入就绪、等待人工决策（W3a0；非终态、不执行、不占 worker；01 §1.2.4 / 03 §3.4.5）。
    WaitingHuman,
    /// 成功（终态）。
    Completed,
    /// 失败（终态）。
    Crashed,
    /// 超时（终态；D1 的 kill 语义）。
    TimedOut,
    /// 被取消（终态）。
    Cancelled,
    /// 入边全部结算且无可用数据，永不执行（终态）。
    Skipped,
}

impl InstanceStatus {
    /// 是否为终态（终态之间不存在任何合法迁移）。
    pub const fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Completed | Self::Crashed | Self::TimedOut | Self::Cancelled | Self::Skipped
        )
    }

    /// 是否为**活跃/等待**状态（非终态且仍让 run 保持不静默）。
    ///
    /// W3a0（DR-006 §3.5）：终止判定 `is_quiescent` 必须把 `WaitingHuman` 计入
    /// 活跃集合——人工等待是合法状态，不是死锁、不得提前完成 run。
    pub const fn is_active(self) -> bool {
        matches!(
            self,
            Self::Pending | Self::Ready | Self::Running | Self::WaitingHuman
        )
    }

    /// 与 00 §0.3 / trace 存储一致的字符串名。
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "Pending",
            Self::Ready => "Ready",
            Self::Running => "Running",
            Self::WaitingHuman => "WaitingHuman",
            Self::Completed => "Completed",
            Self::Crashed => "Crashed",
            Self::TimedOut => "TimedOut",
            Self::Cancelled => "Cancelled",
            Self::Skipped => "Skipped",
        }
    }

    /// 01 §1.2.3 迁移矩阵的可执行版本（I2；W3a0 增 `WaitingHuman` 行/列）。
    ///
    /// 只允许矩阵中的迁移；`Pending → Crashed` 等不在矩阵内的组合一律返回 `false`。
    pub const fn can_transition_to(self, to: Self) -> bool {
        matches!(
            (self, to),
            (Self::Pending, Self::WaitingHuman)
                | (Self::Pending, Self::Ready)
                | (Self::Pending, Self::Cancelled)
                | (Self::Pending, Self::Skipped)
                | (Self::Ready, Self::Running)
                | (Self::Ready, Self::Cancelled)
                | (Self::Running, Self::Completed)
                | (Self::Running, Self::Crashed)
                | (Self::Running, Self::TimedOut)
                | (Self::Running, Self::Cancelled)
                | (Self::WaitingHuman, Self::Completed)
                | (Self::WaitingHuman, Self::TimedOut)
                | (Self::WaitingHuman, Self::Cancelled)
        )
    }
}

/// run 状态（00 §0.3）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RunStatus {
    /// 运行中（派发/应用投递/终止判定均在推进）。
    Running,
    /// 暂停：停止派发，但在飞任务继续、投递仍应用。
    Paused,
    /// 终态：静默结束且未失败。
    Completed,
    /// 终态：冷峻失败（任一 Crashed/TimedOut/框架错误）。
    Failed,
    /// 终态：用户取消。
    Cancelled,
}

impl RunStatus {
    /// 是否为终态。
    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Failed | Self::Cancelled)
    }

    /// 与 00 §0.3 / trace 存储一致的字符串名。
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Running => "Running",
            Self::Paused => "Paused",
            Self::Completed => "Completed",
            Self::Failed => "Failed",
            Self::Cancelled => "Cancelled",
        }
    }
}

/// 记录模式（architecture §5.2/§5.3 的 `--record`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RecordMode {
    /// 不记录节点输出。
    None,
    /// 只记录输出哈希（默认）。
    #[default]
    Hash,
    /// 完整记录输出 JSON（L2 重放所需）。
    Full,
}

impl RecordMode {
    /// 落库/CLI 使用的字符串名。
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Hash => "hash",
            Self::Full => "full",
        }
    }
}

/// run 级指标（00 §0.3 + D1/§8.2）。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RunMetrics {
    /// 因 sink 拒绝而被丢弃的 streaming 事件数。
    pub streaming_dropped: u64,
    /// 被放弃的任务数；D1 后**恒为 0**（保留字段兼容）。
    pub abandoned_tasks: u64,
    /// trace 背压次数；v1 内核 sink 为同步调用，恒为 0。
    pub trace_backpressure: u64,
    /// 因超时/取消被终止的执行任务数（M3 换真进程 kill 后沿用）。
    pub killed_workers: u64,
}

/// `RunFinished.error` 的结构（00 §0.3 `TraceError` + §0.4 错误码）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunError {
    /// 00 §0.4 / §8.2 错误码。
    pub code: String,
    /// 人类可读描述（CLI stderr / trace 排障）。
    pub message: String,
}

impl RunError {
    /// 以错误码与描述构造。
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }
}

impl fmt::Display for RunError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

/// 启动失败（`start_run` 返回；**不产生 run、不写任何 trace 事件**）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunStartError {
    /// [`CODE_RUN_ENTRY_INVALID`] 或 [`CODE_RUN_ENTRY_INPUT_MISSING`]。
    pub code: String,
    /// 人类可读描述。
    pub message: String,
}

impl RunStartError {
    /// 以错误码与描述构造。
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }
}

impl fmt::Display for RunStartError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for RunStartError {}

/// 图编译失败（`LoadedGraph` 结构自洽性被破坏；正常加载路径不会出现）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompileError {
    /// 人类可读描述。
    pub message: String,
}

impl CompileError {
    /// 以描述构造。
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for CompileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for CompileError {}

/// Harness 错误分类（00 §0.3；映射见 05 §5.7）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HarnessErrorKind {
    /// AgentConfig/prompt 文件缺失或非法。
    ConfigLoad,
    /// LLM 调用了未注册工具。
    UnknownTool,
    /// 超过 `max_iterations`。
    MaxIterations,
    /// provider 错误重试耗尽。
    LlmError,
    /// 单次 LLM 调用超时且重试耗尽。
    LlmTimeout,
    /// 工具执行失败（喂回 LLM，不终止）。
    ToolError,
    /// 记忆检索/写入失败（降级告警，不终止）。
    MemoryDegraded,
    /// 上下文预算裁剪后仍溢出（M4 追加，见 05 §5.13；00 §0.4 `HARNESS_CONTEXT_OVERFLOW`）。
    ContextOverflow,
    /// 被引用的 skill 存在但格式/路径非法（v0.3/V2 追加，DR-009 §6）。
    ///
    /// `detail.reason` 为闭集（`unknown_field`/`unparseable_yaml`/…）；
    /// 缺失引用仍走 [`Self::ConfigLoad`]（`skill_not_found`）。
    SkillInvalid,
    /// MCP 配置/开关/生命周期错误（v0.3/V3 追加，DR-010 §8；单一 `HARNESS_MCP_ERROR`）。
    Mcp,
    /// 外部 harness（子进程 adapter）错误（v0.3/V4 追加，DR-011 §6；单一
    /// `HARNESS_PLUGIN_ERROR`，`detail.reason` 闭集）。
    Plugin,
}

impl HarnessErrorKind {
    /// 与 00 §0.4 / §8.2 对应的错误码。
    pub const fn code(self) -> &'static str {
        match self {
            Self::ConfigLoad => CODE_HARNESS_CONFIG_LOAD,
            Self::UnknownTool => CODE_HARNESS_UNKNOWN_TOOL,
            Self::MaxIterations => CODE_HARNESS_MAX_ITERATIONS,
            Self::LlmError => CODE_HARNESS_LLM_ERROR,
            Self::LlmTimeout => CODE_HARNESS_LLM_TIMEOUT,
            Self::ToolError => CODE_HARNESS_TOOL_ERROR,
            Self::MemoryDegraded => CODE_HARNESS_MEMORY_DEGRADED,
            Self::ContextOverflow => CODE_HARNESS_CONTEXT_OVERFLOW,
            Self::SkillInvalid => CODE_HARNESS_SKILL_INVALID,
            Self::Mcp => CODE_HARNESS_MCP_ERROR,
            Self::Plugin => CODE_HARNESS_PLUGIN_ERROR,
        }
    }
}

/// worker（子进程）错误分类（D10 / §8.2）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkerErrorKind {
    /// `hello` 超时 / 协议版本不匹配 / 缺 `sdk_version`。
    Handshake,
    /// 非法帧 / exit 0 无 result / result 后不退出等。
    Protocol,
    /// `module/function` 导入失败或签名不符（D7）。
    Import,
    /// 被信号杀死 / 非约定 exit code。
    Crash,
}

impl WorkerErrorKind {
    /// 与 §8.2 对应的错误码。
    pub const fn code(self) -> &'static str {
        match self {
            Self::Handshake => CODE_NODE_WORKER_HANDSHAKE,
            Self::Protocol => CODE_NODE_WORKER_PROTOCOL,
            Self::Import => CODE_NODE_WORKER_IMPORT,
            Self::Crash => CODE_NODE_WORKER_CRASH,
        }
    }
}

/// 节点执行错误（00 §0.3 + D10）。
///
/// `code()` 返回**对外错误码**；`BusinessError.code` 是节点载荷值
/// （如 `NODE_RETURN_NOT_DICT`、`BIZ_*`），两者语义不同。
#[derive(Debug, Clone, PartialEq, Error)]
pub enum NodeError {
    /// 节点主动抛出的业务错误。
    #[error("business error [{code}]: {message}")]
    BusinessError {
        /// 业务码（`NODE_RETURN_NOT_DICT`/`NODE_RETURN_NOT_JSON`/`BIZ_*`）。
        code: String,
        /// 人类可读描述。
        message: String,
    },
    /// Python 未捕获异常（含 traceback）。
    #[error("python exception: {message}")]
    PythonException {
        /// 异常摘要（`<TypeName>: <str>`）。
        message: String,
        /// `traceback.format_exc()` 或等价全文。
        traceback: String,
    },
    /// 实例级超时（只允许调度器或 AgentCallHandler 预检产生）。
    #[error("node timeout after {elapsed_ms} ms")]
    Timeout {
        /// 已耗时（毫秒）。
        elapsed_ms: u64,
    },
    /// 协作取消。
    #[error("node cancelled")]
    Cancelled,
    /// Agent Harness 错误。
    #[error("harness error ({kind:?}): {detail}")]
    Harness {
        /// 错误分类。
        kind: HarnessErrorKind,
        /// 结构化细节。
        detail: Value,
    },
    /// 配置/资产加载失败（非 Harness）。
    #[error("config error: {message}")]
    Config {
        /// 人类可读描述。
        message: String,
    },
    /// 框架不变量违反（fail-fast）。
    #[error("internal error: {message}")]
    Internal {
        /// 人类可读描述。
        message: String,
    },
    /// worker 子进程错误（D10）。
    ///
    /// `signal`/`exit_code`/`stderr_tail` 是 **M5 仅增**的崩溃证据（ISSUE-066；
    /// architecture §8.2 要求 `NODE_WORKER_CRASH` 记录 signal/exit）：仅崩溃路径
    /// 且确实有该证据时可能为 `Some`，其余路径（握手/协议/导入/worker 自报错误）
    /// 保持 `None`；错误码与 `Display` 均不因此改变。
    #[error("worker error ({kind:?})")]
    NodeWorker {
        /// 错误分类。
        kind: WorkerErrorKind,
        /// 终止 worker 的 Unix 信号编号（非 Unix 平台恒 `None`）。
        signal: Option<i32>,
        /// 非约定退出码（被信号杀死时为 `None`）。
        exit_code: Option<i32>,
        /// stderr 尾部（≤ 1 KiB，按字符边界截断；空则 `None`）。
        stderr_tail: Option<String>,
    },
    /// 单次激活的放行扇出超过节点 `max_fanout`（W2b / DR-004 §3）。
    ///
    /// 由事件循环在应用该激活任何出边之前产生：该激活不产生 `GateResult`、
    /// 不应用任何边；源节点按 `attempt = 源实例 attempt + 1` 创建 Crashed 实例。
    #[error("node fan-out {actual} exceeded max_fanout ({limit})")]
    FanoutExceeded {
        /// 本次激活的有效上限（`min(节点 max_fanout, test-hooks force)`）。
        limit: u32,
        /// 本次激活实际放行的出边数（`GateEvaluation.passed == true` 计数）。
        actual: u32,
    },
}

impl NodeError {
    /// 对外错误码（00 §0.4 + §8.2）。
    pub fn code(&self) -> &'static str {
        match self {
            Self::BusinessError { .. } => CODE_NODE_BUSINESS_ERROR,
            Self::PythonException { .. } => CODE_NODE_PYTHON_EXCEPTION,
            Self::Timeout { .. } => CODE_NODE_TIMEOUT,
            Self::Cancelled => CODE_RUN_CANCELLED,
            Self::Harness { kind, .. } => kind.code(),
            Self::Config { .. } => CODE_NODE_CONFIG_ERROR,
            Self::Internal { .. } => CODE_NODE_INTERNAL,
            Self::NodeWorker { kind, .. } => kind.code(),
            Self::FanoutExceeded { .. } => CODE_NODE_FANOUT_EXCEEDED,
        }
    }
}

/// 当前时刻的 UTC RFC3339 毫秒时间戳（`...Z`）。
///
/// 内核所有 `*_at`/`ts` 字段与 trace 记录统一走这里，避免精度/时区漂移。
pub fn now_rfc3339() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

/// JSON 值的类型名（W4a `RUN_FOREACH_ITEMS_NOT_ARRAY` 的 `got {actual_type}`）。
///
/// 与 DSL 的 `type_name` 同口径（`null`/`boolean`/`number`/`string`/`array`/`object`）。
pub fn json_type_name(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// ISSUE-066：崩溃证据只增字段，不改错误码与 `Display`。
    #[test]
    fn node_worker_crash_payload_keeps_code_and_display() {
        let error = NodeError::NodeWorker {
            kind: WorkerErrorKind::Crash,
            signal: Some(9),
            exit_code: None,
            stderr_tail: Some("killed".to_string()),
        };
        assert_eq!(error.code(), CODE_NODE_WORKER_CRASH);
        assert_eq!(error.to_string(), "worker error (Crash)");
        match &error {
            NodeError::NodeWorker {
                kind,
                signal,
                exit_code,
                stderr_tail,
            } => {
                assert_eq!(*kind, WorkerErrorKind::Crash);
                assert_eq!(*signal, Some(9));
                assert_eq!(*exit_code, None);
                assert_eq!(stderr_tail.as_deref(), Some("killed"));
            }
            other => panic!("unexpected variant: {other:?}"),
        }
    }

    /// 4 个 worker 码不因新增载荷而改变（§8.2）；非崩溃路径细节保持 `None`。
    #[test]
    fn worker_error_codes_are_stable_with_crash_details() {
        let cases = [
            (WorkerErrorKind::Handshake, CODE_NODE_WORKER_HANDSHAKE),
            (WorkerErrorKind::Protocol, CODE_NODE_WORKER_PROTOCOL),
            (WorkerErrorKind::Import, CODE_NODE_WORKER_IMPORT),
            (WorkerErrorKind::Crash, CODE_NODE_WORKER_CRASH),
        ];
        for (kind, expected) in cases {
            let error = NodeError::NodeWorker {
                kind,
                signal: None,
                exit_code: Some(7),
                stderr_tail: Some("tail".to_string()),
            };
            assert_eq!(error.code(), expected, "code for {error:?}");
        }
    }

    /// 新载荷可被下游（CLI 桥接）逐字段读取并序列化（内核本身保持无 serde，
    /// 见 spec 06 §6.2.5.1；序列化断言在 `zgraph-cli` 的 `node_error_json` 测试）。
    #[test]
    fn node_worker_payload_fields_are_reachable() {
        let error = NodeError::NodeWorker {
            kind: WorkerErrorKind::Crash,
            signal: None,
            exit_code: Some(7),
            stderr_tail: Some("tail".to_string()),
        };
        let NodeError::NodeWorker {
            exit_code,
            stderr_tail,
            ..
        } = &error
        else {
            panic!("expected NodeWorker");
        };
        assert_eq!(*exit_code, Some(7));
        assert_eq!(stderr_tail.as_deref(), Some("tail"));
    }

    /// W2b：扇出超限错误码与 `Display` 稳定（trace/cli 依赖），载荷可逐字段读取。
    #[test]
    fn fanout_exceeded_keeps_code_and_display() {
        let error = NodeError::FanoutExceeded {
            limit: 1,
            actual: 2,
        };
        assert_eq!(CODE_NODE_FANOUT_EXCEEDED, "NODE_FANOUT_EXCEEDED");
        assert_eq!(error.code(), CODE_NODE_FANOUT_EXCEEDED);
        assert_eq!(error.to_string(), "node fan-out 2 exceeded max_fanout (1)");
        match &error {
            NodeError::FanoutExceeded { limit, actual } => {
                assert_eq!((*limit, *actual), (1, 2));
            }
            other => panic!("unexpected variant: {other:?}"),
        }
    }

    /// W3a0：`WaitingHuman` 非终态、属活跃集合，且状态名与迁移矩阵就位。
    #[test]
    fn waiting_human_is_active_and_has_expected_transitions() {
        assert_eq!(InstanceStatus::WaitingHuman.as_str(), "WaitingHuman");
        assert!(!InstanceStatus::WaitingHuman.is_terminal());
        assert!(InstanceStatus::WaitingHuman.is_active());
        for status in [
            InstanceStatus::Pending,
            InstanceStatus::Ready,
            InstanceStatus::Running,
            InstanceStatus::WaitingHuman,
        ] {
            assert!(status.is_active(), "{} is active", status.as_str());
        }
        for status in [
            InstanceStatus::Completed,
            InstanceStatus::Crashed,
            InstanceStatus::TimedOut,
            InstanceStatus::Cancelled,
            InstanceStatus::Skipped,
        ] {
            assert!(!status.is_active(), "{} is terminal", status.as_str());
        }
        assert!(InstanceStatus::Pending.can_transition_to(InstanceStatus::WaitingHuman));
        for to in [
            InstanceStatus::Completed,
            InstanceStatus::TimedOut,
            InstanceStatus::Cancelled,
        ] {
            assert!(InstanceStatus::WaitingHuman.can_transition_to(to));
        }
        // WaitingHuman 不经 Ready/Running、不可被再次等待。
        for to in [
            InstanceStatus::Pending,
            InstanceStatus::Ready,
            InstanceStatus::Running,
            InstanceStatus::WaitingHuman,
            InstanceStatus::Crashed,
            InstanceStatus::Skipped,
        ] {
            assert!(!InstanceStatus::WaitingHuman.can_transition_to(to));
        }
    }

    /// W3a0：决策值与 `human_decisions.decision` 的 snake_case 字符串一致。
    #[test]
    fn human_decision_strings_are_snake_case() {
        assert_eq!(HumanDecision::ALL.len(), 2);
        assert_eq!(HumanDecision::Approved.as_str(), "approved");
        assert_eq!(HumanDecision::Rejected.as_str(), "rejected");
    }
    /// v0.3/V4（DR-011 §6）：外部 harness 只追加第 9 个 `HarnessErrorKind::Plugin`，
    /// 错误码 `HARNESS_PLUGIN_ERROR`；既有 kind 码不变（append-only）。
    #[test]
    fn plugin_harness_kind_uses_plugin_error_code() {
        assert_eq!(
            HarnessErrorKind::Plugin.code(),
            CODE_HARNESS_PLUGIN_ERROR,
            "HARNESS_PLUGIN_ERROR"
        );
        let error = NodeError::Harness {
            kind: HarnessErrorKind::Plugin,
            detail: serde_json::json!({"reason": "protocol"}),
        };
        assert_eq!(error.code(), "HARNESS_PLUGIN_ERROR");
        // 既有码不变（示例：MCP / SkillInvalid / ConfigLoad）。
        assert_eq!(HarnessErrorKind::Mcp.code(), "HARNESS_MCP_ERROR");
        assert_eq!(
            HarnessErrorKind::SkillInvalid.code(),
            "HARNESS_SKILL_INVALID"
        );
        assert_eq!(HarnessErrorKind::ConfigLoad.code(), "HARNESS_CONFIG_LOAD");
    }
}
