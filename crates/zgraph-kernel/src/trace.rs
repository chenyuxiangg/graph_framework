//! trace sink 契约与内置实现（内核**不依赖** `zgraph-trace`）。
//!
//! 语义基准：`doc/spec-v1.2/00-总纲.md` §0.3（事件枚举/不可丢清单）、
//! `doc/spec-v1.2/06-存储与可观测.md` §6.3（事件分类/时间戳）与
//! `doc/architecture-v2.md` §5.2（v2 增量字段）。
//!
//! 分工：内核只负责**按序调用** sink（`RunStarted` 第一、`RunFinished` 最后且仅一次）；
//! SQLite/回放由 `zgraph-trace` 实现，CLI 负责把本模块记录桥接为 trace 类型。
//! `RunOptions::trace_enabled = false` 时内核**不调用** sink（仍构造 outcome）。
//!
//! `streaming` 是唯一可丢事件：返回 `false` 表示 sink 丢弃，内核计入
//! `RunMetrics::streaming_dropped`（I6）。

use std::sync::{Arc, Mutex, MutexGuard};

use serde_json::Value;

use crate::types::{
    Attempt, EdgeIdx, InstanceId, InstanceStatus, NodeError, NodeId, PortName, RunError, RunId,
    RunMetrics, RunStatus,
};

/// 流式事件（节点 `ctx.emit` 产生；只进 trace，不参与数据通道）。
#[derive(Debug, Clone, PartialEq)]
pub enum StreamingEvent {
    /// token 增量。
    TokenDelta {
        /// 文本增量。
        text: String,
    },
    /// 推理（reasoning）增量。
    ReasoningDelta {
        /// 文本增量。
        text: String,
    },
    /// 工具调用开始。
    ToolCallStart {
        /// 调用 ID。
        call_id: String,
        /// 工具名。
        tool_name: String,
        /// 参数摘要。
        args_summary: Value,
    },
    /// 工具调用结束。
    ToolCallEnd {
        /// 调用 ID。
        call_id: String,
        /// 工具名。
        tool_name: String,
        /// 是否成功。
        ok: bool,
        /// 结果摘要（可缺省）。
        result_summary: Option<Value>,
    },
    /// 进度百分比（0–100）。
    Progress {
        /// 百分比。
        percent: u8,
        /// 描述。
        message: String,
    },
}

/// 实例快照（06 §6.2.5 `instances` 的 upsert 载荷；创建与每次状态变化均调用）。
#[derive(Debug, Clone, PartialEq)]
pub struct InstanceSnapshot {
    /// 所属 run。
    pub run_id: RunId,
    /// 实例 ID。
    pub instance_id: InstanceId,
    /// 节点 ID。
    pub node_id: NodeId,
    /// 节点类型字符串：`start`/`end`/`python_node`/`agent_call`/`subgraph`。
    pub node_type: String,
    /// 尝试序号。
    pub attempt: Attempt,
    /// 当前状态。
    pub status: InstanceStatus,
    /// 派发时的输入快照（全部声明端口；未到为 null）。
    pub input: Option<Value>,
    /// 成功输出。
    pub output: Option<Value>,
    /// 失败错误。
    pub error: Option<NodeError>,
    /// 失败错误码（`error.code()` 冗余，便于 SQL 查询）。
    pub error_code: Option<String>,
    /// 创建时间（UTC RFC3339 毫秒 Z）。
    pub created_at: String,
    /// 开始执行时间。
    pub started_at: Option<String>,
    /// 结束时间。
    pub finished_at: Option<String>,
}

/// 状态迁移记录（01 §1.5/06 §6.2.7）。
#[derive(Debug, Clone, PartialEq)]
pub struct TransitionRecord {
    /// 所属 run。
    pub run_id: RunId,
    /// 实例 ID。
    pub instance_id: InstanceId,
    /// 节点 ID。
    pub node_id: NodeId,
    /// 尝试序号。
    pub attempt: Attempt,
    /// 迁移前状态。
    pub from: InstanceStatus,
    /// 迁移后状态。
    pub to: InstanceStatus,
    /// 迁移原因（`inputs ready`/`dispatch`/`execution ok`/...）。
    pub reason: Option<String>,
    /// 迁移时刻（UTC RFC3339 毫秒 Z）。
    pub ts: String,
}

/// 门控结果记录（01 §1.5 / 04 §4.6.4；每条出边每次 publish 恰好一条）。
///
/// W2a0（DR-005 §4.1）：新增 [`Self::deferred`]/[`Self::attempt`]/[`Self::verdict`]
/// 三个已冻结字段（04 §4.6.4 例外：只增这三个、仍不记录求值耗时）。
#[derive(Debug, Clone, PartialEq)]
pub struct GateResultRecord {
    /// 所属 run。
    pub run_id: RunId,
    /// 发布者实例。
    pub instance_id: InstanceId,
    /// 发布者节点。
    pub node_id: NodeId,
    /// 被求值的边。
    pub edge_index: EdgeIdx,
    /// 边目标节点。
    pub to_node: NodeId,
    /// 边目标端口。
    pub to_port: PortName,
    /// 门控函数名；`None` = 该边无 gate（恒放行）。
    pub gate_fn: Option<String>,
    /// 是否放行。
    pub passed: bool,
    /// 求值异常描述。
    pub error: Option<String>,
    /// 本次求值是否触发 defer（= `!passed && 边 on_gate_false == Defer`）。
    ///
    /// 是**本次求值结果**而非边的静态属性（边属性可从 `edge_index` 查
    /// [`crate::CompiledEdge::on_gate_false`]，不重复入库）。
    pub deferred: bool,
    /// **发布者实例**的 attempt（非自环/未重入为 0）。
    pub attempt: Attempt,
    /// 门控判定值（仅内置 `check_output`/`compare` 且 `params.path` 为字符串）；
    /// 只用于 trace/回放辅助，**不得**作为路由判据（路由只看 `passed`）。
    pub verdict: Option<Value>,
    /// 求值时刻（UTC RFC3339 毫秒 Z）。
    pub ts: String,
}

/// `RunStarted` 载荷（06 §6.3/architecture §5.2 的 run 元数据）。
#[derive(Debug, Clone, PartialEq)]
pub struct RunStartedInfo {
    /// run ID。
    pub run_id: RunId,
    /// 图指纹。
    pub graph_hash: String,
    /// 图名（`metadata.name`）。
    pub graph_name: String,
    /// 原始 DSL JSON。
    pub dsl_json: String,
    /// 用户 ID。
    pub user_id: String,
    /// 入口注入（`node_id -> {port: value}`）。
    pub entry_inputs: Value,
    /// 随机种子（可复现 L2）。
    pub seed: u64,
    /// 记录模式字符串（`none`/`hash`/`full`）。
    pub record_mode: String,
    /// 宿主进程 PID。
    pub owner_pid: u32,
    /// 宿主主机名（无法获取时 `unknown`）。
    pub owner_host: String,
    /// 启动时刻（UTC RFC3339 毫秒 Z）。
    pub started_at: String,
}

/// `RunFinished` 载荷（恰一次，I5）。
#[derive(Debug, Clone, PartialEq)]
pub struct RunFinishedInfo {
    /// run ID。
    pub run_id: RunId,
    /// 终态。
    pub status: RunStatus,
    /// 输出摘要（end 聚合结果；失败/取消为 None）。
    pub output_summary: Option<Value>,
    /// 失败错误（`Failed` 时）。
    pub error: Option<RunError>,
    /// run 指标。
    pub metrics: RunMetrics,
    /// 结束时刻（UTC RFC3339 毫秒 Z）。
    pub at: String,
}

/// 内核 → trace 的同步 sink 契约。
///
/// 所有方法在 run 生命周期内**有序**调用；实现者不得反向调用内核、不得阻塞
/// 事件循环（生产实现应把事件转发到有界通道，由独立写线程落库）。
pub trait TraceSink: Send + Sync {
    /// run 启动（生命周期内第一条事件）。
    fn run_started(&self, info: RunStartedInfo);
    /// 实例创建（Pending）与每次状态变化时调用（写线程按 `instance_id` upsert）。
    fn instance_snapshot(&self, snapshot: InstanceSnapshot);
    /// 状态迁移：同时给出迁移记录与迁移后的快照。
    fn transition(&self, record: TransitionRecord, snapshot: InstanceSnapshot);
    /// 门控结果（不可丢；每条出边每次 publish 一条）。
    fn gate_result(&self, record: GateResultRecord);
    /// 实例超时（D1：超时 = kill；`elapsed_ms` 为已执行毫秒数）。
    fn node_timed_out(&self, run_id: &RunId, instance_id: &InstanceId, elapsed_ms: u64, at: String);
    /// 实例被放弃（**v2 不产生**；D1 保留接口兼容）。
    fn node_abandoned(&self, run_id: &RunId, instance_id: &InstanceId, at: String);
    /// 流式事件（可丢）：返回 `false` 表示丢弃，内核计入 `streaming_dropped`。
    fn streaming(
        &self,
        run_id: &RunId,
        instance_id: &InstanceId,
        node_id: &NodeId,
        event: StreamingEvent,
        at: String,
    ) -> bool;
    /// run 结束（生命周期内最后一条事件，恰一次）。
    fn run_finished(&self, info: RunFinishedInfo);

    /// 取走 trace 写端的**不可恢复**错误文本（fail-stop 轮询；06 §6.4.5）。
    ///
    /// 默认 `None`（内存/no-op sink 永不失败）。生产实现（CLI 的 trace 桥）应非阻塞地
    /// 取走写线程已记录的错误；内核事件循环据此把 run 置为**内存态** `Failed` 并终止，
    /// 且不把该错误写入领域 `error_code`（`STORAGE_ERROR` 属 CLI/storage 命名空间，
    /// 06 §6.12.3 D3）。
    fn take_error(&self) -> Option<String> {
        None
    }
}

/// 全 no-op sink（`--no-trace` / 不关心 trace 的嵌入调用方）。
pub struct NullTraceSink;

impl TraceSink for NullTraceSink {
    fn run_started(&self, _info: RunStartedInfo) {}

    fn instance_snapshot(&self, _snapshot: InstanceSnapshot) {}

    fn transition(&self, _record: TransitionRecord, _snapshot: InstanceSnapshot) {}

    fn gate_result(&self, _record: GateResultRecord) {}

    fn node_timed_out(
        &self,
        _run_id: &RunId,
        _instance_id: &InstanceId,
        _elapsed_ms: u64,
        _at: String,
    ) {
    }

    fn node_abandoned(&self, _run_id: &RunId, _instance_id: &InstanceId, _at: String) {}

    fn streaming(
        &self,
        _run_id: &RunId,
        _instance_id: &InstanceId,
        _node_id: &NodeId,
        _event: StreamingEvent,
        _at: String,
    ) -> bool {
        true
    }

    fn run_finished(&self, _info: RunFinishedInfo) {}
}

/// 一次 `node_timed_out` 调用的记录（测试用）。
#[derive(Debug, Clone, PartialEq)]
pub struct TimedOutEvent {
    /// 所属 run。
    pub run_id: RunId,
    /// 实例 ID。
    pub instance_id: InstanceId,
    /// 已执行毫秒数。
    pub elapsed_ms: u64,
    /// 时刻。
    pub at: String,
}

/// 一次 `node_abandoned` 调用的记录（测试用；v2 不应出现）。
#[derive(Debug, Clone, PartialEq)]
pub struct AbandonedEvent {
    /// 所属 run。
    pub run_id: RunId,
    /// 实例 ID。
    pub instance_id: InstanceId,
    /// 时刻。
    pub at: String,
}

/// 一次 `streaming` 调用的记录（测试用）。
#[derive(Debug, Clone, PartialEq)]
pub struct StreamingCall {
    /// 所属 run。
    pub run_id: RunId,
    /// 实例 ID。
    pub instance_id: InstanceId,
    /// 节点 ID。
    pub node_id: NodeId,
    /// 事件载荷。
    pub event: StreamingEvent,
    /// 时刻。
    pub at: String,
}

/// `MemoryTraceSink` 记录的调用类别（用于断言 sink 调用顺序：I5/I6）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TraceCall {
    /// `run_started`。
    RunStarted,
    /// `instance_snapshot`。
    InstanceSnapshot,
    /// `transition`。
    Transition,
    /// `gate_result`。
    GateResult,
    /// `node_timed_out`。
    NodeTimedOut,
    /// `node_abandoned`。
    NodeAbandoned,
    /// `streaming`。
    Streaming,
    /// `run_finished`。
    RunFinished,
}

/// 测试/嵌入用内存 sink：`Mutex` 收集全部调用，提供按类访问器。
///
/// 通过 [`MemoryTraceSink::with_streaming_limit`] 可让前 N 次 `streaming` 成功、
/// 之后返回 `false`，用于验证 `streaming_dropped` 计数。
pub struct MemoryTraceSink {
    inner: Mutex<MemoryTraceData>,
    streaming_limit: Option<usize>,
}

impl Default for MemoryTraceSink {
    fn default() -> Self {
        Self::new()
    }
}

/// 内存 sink 的收集数据。
#[derive(Default)]
struct MemoryTraceData {
    started: Vec<RunStartedInfo>,
    snapshots: Vec<InstanceSnapshot>,
    transitions: Vec<TransitionRecord>,
    gate_results: Vec<GateResultRecord>,
    timed_out: Vec<TimedOutEvent>,
    abandoned: Vec<AbandonedEvent>,
    streaming: Vec<StreamingCall>,
    finished: Vec<RunFinishedInfo>,
    calls: Vec<TraceCall>,
}

impl MemoryTraceSink {
    /// 无限制内存 sink。
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(MemoryTraceData::default()),
            streaming_limit: None,
        }
    }

    /// 只接受前 `limit` 条 streaming 事件，其余返回 `false`（模拟可丢通道满）。
    pub fn with_streaming_limit(limit: usize) -> Self {
        Self {
            inner: Mutex::new(MemoryTraceData::default()),
            streaming_limit: Some(limit),
        }
    }

    /// 是否完全没有收到任何 sink 调用（启动校验失败测试用）。
    pub fn is_empty(&self) -> bool {
        let data = self.lock();
        data.started.is_empty()
            && data.snapshots.is_empty()
            && data.transitions.is_empty()
            && data.gate_results.is_empty()
            && data.timed_out.is_empty()
            && data.abandoned.is_empty()
            && data.streaming.is_empty()
            && data.finished.is_empty()
            && data.calls.is_empty()
    }

    /// `RunStarted` 调用列表。
    pub fn started_infos(&self) -> Vec<RunStartedInfo> {
        self.lock().started.clone()
    }

    /// 实例快照列表（创建 + 每次状态变化）。
    pub fn snapshots(&self) -> Vec<InstanceSnapshot> {
        self.lock().snapshots.clone()
    }

    /// 迁移记录列表。
    pub fn transitions(&self) -> Vec<TransitionRecord> {
        self.lock().transitions.clone()
    }

    /// 门控结果列表。
    pub fn gate_results(&self) -> Vec<GateResultRecord> {
        self.lock().gate_results.clone()
    }

    /// 超时事件列表。
    pub fn timed_out(&self) -> Vec<TimedOutEvent> {
        self.lock().timed_out.clone()
    }

    /// 放弃事件列表（v2 应为空）。
    pub fn abandoned(&self) -> Vec<AbandonedEvent> {
        self.lock().abandoned.clone()
    }

    /// streaming 调用列表（被丢弃的不记录）。
    pub fn streaming_calls(&self) -> Vec<StreamingCall> {
        self.lock().streaming.clone()
    }

    /// `RunFinished` 列表（应恰好一条）。
    pub fn finished(&self) -> Vec<RunFinishedInfo> {
        self.lock().finished.clone()
    }

    /// 按调用顺序记录的 sink 方法序列（断言 RunStarted 第一 / RunFinished 最后）。
    pub fn call_log(&self) -> Vec<TraceCall> {
        self.lock().calls.clone()
    }

    /// 加锁（中毒时取回内部数据：测试 sink 不因另一个测试的 panic 失效）。
    fn lock(&self) -> MutexGuard<'_, MemoryTraceData> {
        match self.inner.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        }
    }
}

impl TraceSink for MemoryTraceSink {
    fn run_started(&self, info: RunStartedInfo) {
        let mut data = self.lock();
        data.calls.push(TraceCall::RunStarted);
        data.started.push(info);
    }

    fn instance_snapshot(&self, snapshot: InstanceSnapshot) {
        let mut data = self.lock();
        data.calls.push(TraceCall::InstanceSnapshot);
        data.snapshots.push(snapshot);
    }

    fn transition(&self, record: TransitionRecord, snapshot: InstanceSnapshot) {
        let mut data = self.lock();
        data.calls.push(TraceCall::Transition);
        data.transitions.push(record);
        data.snapshots.push(snapshot);
    }

    fn gate_result(&self, record: GateResultRecord) {
        let mut data = self.lock();
        data.calls.push(TraceCall::GateResult);
        data.gate_results.push(record);
    }

    fn node_timed_out(
        &self,
        run_id: &RunId,
        instance_id: &InstanceId,
        elapsed_ms: u64,
        at: String,
    ) {
        let mut data = self.lock();
        data.calls.push(TraceCall::NodeTimedOut);
        data.timed_out.push(TimedOutEvent {
            run_id: run_id.clone(),
            instance_id: instance_id.clone(),
            elapsed_ms,
            at,
        });
    }

    fn node_abandoned(&self, run_id: &RunId, instance_id: &InstanceId, at: String) {
        let mut data = self.lock();
        data.calls.push(TraceCall::NodeAbandoned);
        data.abandoned.push(AbandonedEvent {
            run_id: run_id.clone(),
            instance_id: instance_id.clone(),
            at,
        });
    }

    fn streaming(
        &self,
        run_id: &RunId,
        instance_id: &InstanceId,
        node_id: &NodeId,
        event: StreamingEvent,
        at: String,
    ) -> bool {
        let mut data = self.lock();
        data.calls.push(TraceCall::Streaming);
        if let Some(limit) = self.streaming_limit {
            if data.streaming.len() >= limit {
                return false;
            }
        }
        data.streaming.push(StreamingCall {
            run_id: run_id.clone(),
            instance_id: instance_id.clone(),
            node_id: node_id.clone(),
            event,
            at,
        });
        true
    }

    fn run_finished(&self, info: RunFinishedInfo) {
        let mut data = self.lock();
        data.calls.push(TraceCall::RunFinished);
        data.finished.push(info);
    }
}

/// 内核内部的事件发射器：保证 `RunFinished` 之后不再有任何 sink 调用。
///
/// 为什么需要：执行任务（streaming）与事件循环并发；`fail_run`/`finish` 结束后
/// 在飞任务可能在 abort 生效前再 emit 一次。用 `closed` 互斥量把
/// "调用 sink" 与 "关闭 sink" 串行化，保证 `RunFinished` 严格最后（I5/I6）。
#[derive(Clone)]
pub(crate) struct Emitter {
    sink: Arc<dyn TraceSink>,
    enabled: bool,
    closed: Arc<Mutex<bool>>,
}

impl Emitter {
    pub(crate) fn new(sink: Arc<dyn TraceSink>, enabled: bool) -> Self {
        Self {
            sink,
            enabled,
            closed: Arc::new(Mutex::new(false)),
        }
    }

    /// 加锁（中毒不影响正确性：closed 仍是单调布尔）。
    fn lock_closed(&self) -> MutexGuard<'_, bool> {
        match self.closed.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        }
    }

    pub(crate) fn run_started(&self, info: RunStartedInfo) {
        if !self.enabled {
            return;
        }
        let closed = self.lock_closed();
        if *closed {
            return;
        }
        self.sink.run_started(info);
    }

    pub(crate) fn instance_snapshot(&self, snapshot: InstanceSnapshot) {
        if !self.enabled {
            return;
        }
        let closed = self.lock_closed();
        if *closed {
            return;
        }
        self.sink.instance_snapshot(snapshot);
    }

    pub(crate) fn transition(&self, record: TransitionRecord, snapshot: InstanceSnapshot) {
        if !self.enabled {
            return;
        }
        let closed = self.lock_closed();
        if *closed {
            return;
        }
        self.sink.transition(record, snapshot);
    }

    pub(crate) fn gate_result(&self, record: GateResultRecord) {
        if !self.enabled {
            return;
        }
        let closed = self.lock_closed();
        if *closed {
            return;
        }
        self.sink.gate_result(record);
    }

    pub(crate) fn node_timed_out(
        &self,
        run_id: &RunId,
        instance_id: &InstanceId,
        elapsed_ms: u64,
        at: String,
    ) {
        if !self.enabled {
            return;
        }
        let closed = self.lock_closed();
        if *closed {
            return;
        }
        self.sink
            .node_timed_out(run_id, instance_id, elapsed_ms, at);
    }

    pub(crate) fn streaming(
        &self,
        run_id: &RunId,
        instance_id: &InstanceId,
        node_id: &NodeId,
        event: StreamingEvent,
        at: String,
    ) -> bool {
        if !self.enabled {
            return true;
        }
        let closed = self.lock_closed();
        if *closed {
            return false;
        }
        self.sink.streaming(run_id, instance_id, node_id, event, at)
    }

    /// 取走 trace 写端不可恢复错误（fail-stop 轮询；`trace_enabled=false` 时不轮询）。
    ///
    /// 不持 `closed` 锁：与 sink 调用并发是允许的（底层实现用原子/互斥自行保证），
    /// 事件循环只会在两次 sink 调用之间调用本方法。
    pub(crate) fn take_error(&self) -> Option<String> {
        if !self.enabled {
            return None;
        }
        self.sink.take_error()
    }

    /// 关闭 sink 并发出最后一条 `RunFinished`；重复调用返回 `false`（I5 去重）。
    pub(crate) fn finish(&self, info: RunFinishedInfo) -> bool {
        if !self.enabled {
            return false;
        }
        let mut closed = self.lock_closed();
        if *closed {
            return false;
        }
        *closed = true;
        self.sink.run_finished(info);
        true
    }
}
