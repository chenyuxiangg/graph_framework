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

/// 记忆检索/写入失败（降级告警，不终止）。
pub const CODE_HARNESS_MEMORY_DEGRADED: &str = "HARNESS_MEMORY_DEGRADED";

/// 实例状态（00 §0.3；v1 全部可能产生的状态）。
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

    /// 与 00 §0.3 / trace 存储一致的字符串名。
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

    /// 01 §1.2.3 迁移矩阵的可执行版本（I2）。
    ///
    /// 只允许矩阵中的迁移；`Pending → Crashed` 等不在矩阵内的组合一律返回 `false`。
    pub const fn can_transition_to(self, to: Self) -> bool {
        matches!(
            (self, to),
            (Self::Pending, Self::Ready)
                | (Self::Pending, Self::Cancelled)
                | (Self::Pending, Self::Skipped)
                | (Self::Ready, Self::Running)
                | (Self::Ready, Self::Cancelled)
                | (Self::Running, Self::Completed)
                | (Self::Running, Self::Crashed)
                | (Self::Running, Self::TimedOut)
                | (Self::Running, Self::Cancelled)
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
    #[error("worker error ({kind:?})")]
    NodeWorker {
        /// 错误分类。
        kind: WorkerErrorKind,
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
            Self::NodeWorker { kind } => kind.code(),
        }
    }
}

/// 当前时刻的 UTC RFC3339 毫秒时间戳（`...Z`）。
///
/// 内核所有 `*_at`/`ts` 字段与 trace 记录统一走这里，避免精度/时区漂移。
pub fn now_rfc3339() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}
