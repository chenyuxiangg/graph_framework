//! kernel → trace 桥接层（M2）：把内核 `TraceSink` 事件逐字段落为 trace 写端输入。
//!
//! 语义基准：`doc/architecture-v2.md` §5.2/§5.3 与 `doc/spec-v1.2/06-存储与可观测.md`
//! §6.2.12（payload → 列映射）。分工：
//!
//! - `zgraph-kernel` 只定义 sink 契约（记录类型**没有 serde**）；
//! - `zgraph-trace` 只接收 [`zgraph_trace::TraceInput`]；
//! - 本模块负责**手动逐字段映射**，并把 streaming 走
//!   [`zgraph_trace::TraceHandle::send_streaming`]（返回 `false` 时内核计入
//!   `streaming_dropped`，I6）。
//!
//! 除落库外，桥接器还支持两类**只读旁路**（[`BridgeObservers`]）：`streaming`
//! （`--stream` 的流式文本）与 `lifecycle`（human 模式实时生命周期行，ISSUE-085）。
//! 旁路在 `send` 之前回调，顺序即内核 sink 调用顺序；它们不写库、不影响返回值，
//! 因此 trace 落库语义不随观察者变化。
//!
//! # 便捷入口
//!
//! [`run_in_process`]：打开 `TraceWriter` → `start_run` → 等待 → flush/close →
//! 校验写线程错误 → 返回 [`BridgeOutcome`]（含可直接查询的 `TraceReader`）。
//! 需要运行期控制（Pause/Resume/Cancel）时用 [`start_traced_run`] 拿到
//! [`TracedRun`]，控制后再 `wait`。
//!
//! # 构造约束
//!
//! `RunOptions` 在 `test-hooks` 特性合并时多一个 cfg 字段，调用方必须用
//! `RunOptions::default()` 再赋值（或 `..Default::default()`），禁止穷举字段。
//! 本模块的测试也遵循该约定。

use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde_json::{json, Map, Value};

use zgraph_kernel as kernel;
use zgraph_trace as trace;

/// 运行期流式观察者：每条内核流式事件在落库**之外**再回调一次。
///
/// `zgraph run --stream` 用它把 `TokenDelta`/`ReasoningDelta` 文本实时写到 stderr
/// （06 §6.10.3）；回调**不改变** trace 语义（观察者不得影响 run，故签名无返回值）。
pub type StreamingObserver = Arc<dyn Fn(&kernel::StreamingEvent) + Send + Sync>;

/// 生命周期事件观察者：每条会落 `events` 行的映射事件在写入 trace **之外**再回调一次。
///
/// `zgraph run`（human 模式）用它实时输出生命周期行（ISSUE-085 / 06 §6.10.3），
/// 不必等 run 结束再从 trace 回读。回调只读、不写库、无返回值；事件顺序与内核
/// sink 调用顺序一致（即 trace 落库顺序），但这里拿不到写线程分配的 `seq`。
pub type LifecycleObserver = Arc<dyn Fn(&trace::TraceInput) + Send + Sync>;

/// 桥接层观察者组合（`BridgeObservers` 只承载旁路，不参与落库）。
#[derive(Default, Clone)]
pub struct BridgeObservers {
    /// `--stream`：流式文本实时回调。
    pub streaming: Option<StreamingObserver>,
    /// human 模式：实时生命周期行（ISSUE-085）。
    pub lifecycle: Option<LifecycleObserver>,
}

/// kernel `TraceSink` 的 trace 写端适配器。
///
/// 每次 sink 调用映射为一条 [`trace::TraceInput`]；`streaming` 使用可丢发送，
/// 返回 `false` 让内核累计 `streaming_dropped`。
///
/// 观察者（[`BridgeObservers`]）在落库**之外**再拿到同一份映射结果：`streaming`
/// 用于 `--stream` 文本，`lifecycle` 用于 human 模式实时生命周期行（ISSUE-085）；
/// 两者都不写库、不影响落库语义。
pub struct TraceBridge {
    handle: trace::TraceHandle,
    observers: BridgeObservers,
}

impl TraceBridge {
    /// 用 trace 写句柄构造桥接器（句柄可继续克隆给其他生产者，但 M2 只有本桥）。
    pub fn new(handle: trace::TraceHandle) -> Self {
        Self {
            handle,
            observers: BridgeObservers::default(),
        }
    }

    /// 用观察者组合构造桥接器（`--stream` / human 实时生命周期；与落库互不影响）。
    pub fn with_observers(handle: trace::TraceHandle, observers: BridgeObservers) -> Self {
        Self { handle, observers }
    }

    /// 底层写句柄（诊断/错误检查用）。
    pub fn handle(&self) -> &trace::TraceHandle {
        &self.handle
    }

    /// 生命周期旁路：把映射后的事件再回调一次（只读，不改变落库）。
    fn notify_lifecycle(&self, input: &trace::TraceInput) {
        if let Some(observer) = &self.observers.lifecycle {
            observer(input);
        }
    }

    /// 落一条**事件类**输入：先通知生命周期旁路，再入写端（回调顺序 = 落库顺序）。
    fn send_event(&self, input: trace::TraceInput) {
        self.notify_lifecycle(&input);
        self.handle.send(input);
    }
}

impl kernel::TraceSink for TraceBridge {
    fn run_started(&self, info: kernel::RunStartedInfo) {
        self.send_event(trace::TraceInput::RunStarted(map_run_started(&info)));
    }

    fn instance_snapshot(&self, snapshot: kernel::InstanceSnapshot) {
        // 快照不产生 `events` 行，也不在实时生命周期行内（只 upsert instances）。
        self.handle
            .send(trace::TraceInput::InstanceSnapshot(map_snapshot(&snapshot)));
    }

    fn transition(&self, record: kernel::TransitionRecord, snapshot: kernel::InstanceSnapshot) {
        let (record, snapshot) = map_transition(record, &snapshot);
        self.send_event(trace::TraceInput::Transition { record, snapshot });
    }

    fn gate_result(&self, record: kernel::GateResultRecord) {
        self.send_event(trace::TraceInput::GateResult(map_gate_result(&record)));
    }

    fn node_timed_out(
        &self,
        run_id: &kernel::RunId,
        instance_id: &kernel::InstanceId,
        elapsed_ms: u64,
        at: String,
    ) {
        self.send_event(trace::TraceInput::NodeTimedOut {
            run_id: run_id.clone(),
            instance_id: instance_id.clone(),
            elapsed_ms,
            at,
        });
    }

    fn node_abandoned(&self, run_id: &kernel::RunId, instance_id: &kernel::InstanceId, at: String) {
        self.send_event(trace::TraceInput::NodeAbandoned {
            run_id: run_id.clone(),
            instance_id: instance_id.clone(),
            at,
        });
    }

    fn streaming(
        &self,
        run_id: &kernel::RunId,
        instance_id: &kernel::InstanceId,
        node_id: &kernel::NodeId,
        event: kernel::StreamingEvent,
        at: String,
    ) -> bool {
        if let Some(observer) = &self.observers.streaming {
            observer(&event);
        }
        let input = trace::TraceInput::Streaming {
            run_id: run_id.clone(),
            instance_id: instance_id.clone(),
            node_id: Some(node_id.clone()),
            event: map_streaming_event(event),
            at,
        };
        self.notify_lifecycle(&input);
        self.handle.send_streaming(input)
    }

    fn run_finished(&self, info: kernel::RunFinishedInfo) {
        self.send_event(trace::TraceInput::RunFinished(map_run_finished(&info)));
    }

    /// fail-stop 轮询（06 §6.4.5）：非阻塞取走 trace 写线程的不可恢复错误。
    fn take_error(&self) -> Option<String> {
        self.handle.take_error()
    }
}

/// 桥接层错误：trace 存储 / run 启动校验 / 写线程 fail-stop。
#[derive(Debug)]
pub enum BridgeError {
    /// trace 存储错误（打开/迁移/查询）。
    Trace(trace::TraceError),
    /// run 启动校验失败（不产生 run、不写任何 trace 事件）。
    Start(kernel::RunStartError),
    /// trace 写线程报告不可恢复错误（fail-stop）。
    Writer(String),
}

impl fmt::Display for BridgeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Trace(error) => write!(formatter, "trace: {error}"),
            Self::Start(error) => write!(formatter, "run start: {error}"),
            Self::Writer(message) => write!(formatter, "trace writer: {message}"),
        }
    }
}

impl std::error::Error for BridgeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Trace(error) => Some(error),
            Self::Start(error) => Some(error),
            Self::Writer(_) => None,
        }
    }
}

impl From<trace::TraceError> for BridgeError {
    fn from(error: trace::TraceError) -> Self {
        Self::Trace(error)
    }
}

impl From<kernel::RunStartError> for BridgeError {
    fn from(error: kernel::RunStartError) -> Self {
        Self::Start(error)
    }
}

/// 已接好 trace 的 run：可发控制命令，`wait` 后得到 outcome + 只读 reader。
pub struct TracedRun {
    handle: kernel::RunHandle,
    trace_handle: trace::TraceHandle,
    writer: Option<trace::TraceWriter>,
}

impl TracedRun {
    /// run ID（uuidv7）。
    pub fn run_id(&self) -> &str {
        self.handle.run_id()
    }

    /// 发送控制命令；run 已结束时返回 `false`。
    pub fn control(&self, command: kernel::RunControl) -> bool {
        self.handle.control(command)
    }

    /// 暂停派发（等价于 `control(Pause)`）。
    pub fn pause(&self) -> bool {
        self.handle.pause()
    }

    /// 恢复派发（等价于 `control(Resume)`）。
    pub fn resume(&self) -> bool {
        self.handle.resume()
    }

    /// 取消 run（等价于 `control(Cancel)`，幂等）。
    pub fn cancel(&self) -> bool {
        self.handle.cancel()
    }

    /// 当前 run 状态快照。
    pub fn status(&self) -> kernel::RunStatus {
        self.handle.status()
    }

    /// 等待 run 结束：flush/close 写端 → 返回 outcome 与 reader。
    pub async fn wait(self) -> Result<BridgeOutcome, BridgeError> {
        let TracedRun {
            handle,
            trace_handle,
            writer,
        } = self;
        let outcome = handle.wait().await;
        let db_path = close_writer(writer, &trace_handle)?;
        let reader = trace::TraceReader::open_readonly(&db_path)?;
        Ok(BridgeOutcome {
            outcome,
            reader,
            db_path,
        })
    }

    /// 等待 run 结束并关闭写端，但**不**打开只读 reader。
    ///
    /// 用于跨进程不可读的库（`:memory:`，06 §6.9.1）：run 结果由内核 outcome 直接
    /// 提供，调用方不得再依赖 trace 查询。
    pub async fn wait_without_reader(self) -> Result<kernel::RunOutcome, BridgeError> {
        let TracedRun {
            handle,
            trace_handle,
            writer,
        } = self;
        let outcome = handle.wait().await;
        close_writer(writer, &trace_handle)?;
        Ok(outcome)
    }
}

/// flush + close 写端并校验写线程 fail-stop 错误；返回库路径。
fn close_writer(
    writer: Option<trace::TraceWriter>,
    trace_handle: &trace::TraceHandle,
) -> Result<PathBuf, BridgeError> {
    let writer =
        writer.ok_or_else(|| BridgeError::Writer("trace writer already closed".to_string()))?;
    let db_path = writer.db_path().to_path_buf();
    let flush_result = writer.flush();
    let trace_error = trace_handle.take_error();
    let close_result = writer.close();
    flush_result?;
    close_result?;
    if let Some(message) = trace_error {
        return Err(BridgeError::Writer(message));
    }
    Ok(db_path)
}

/// run 结束产物：内核 outcome + 已关闭写端的只读 trace 连接。
pub struct BridgeOutcome {
    /// 内核 outcome（与 `RunFinished` 一致）。
    pub outcome: kernel::RunOutcome,
    reader: trace::TraceReader,
    db_path: PathBuf,
}

impl BridgeOutcome {
    /// 只读 trace 连接（写端已关闭，可安全查询）。
    pub fn reader(&self) -> &trace::TraceReader {
        &self.reader
    }

    /// trace 数据库路径。
    pub fn db_path(&self) -> &Path {
        &self.db_path
    }

    /// 拆出 `(outcome, TraceReader)`。
    pub fn into_parts(self) -> (kernel::RunOutcome, trace::TraceReader) {
        (self.outcome, self.reader)
    }
}

/// 打开 trace 写端并启动 run，返回可控制的 [`TracedRun`]。
pub fn start_traced_run(
    spec: kernel::RunSpec,
    executor: Arc<dyn kernel::NodeExecutor>,
    db_path: impl AsRef<Path>,
) -> Result<TracedRun, BridgeError> {
    let writer = trace::TraceWriter::open(db_path.as_ref())?;
    start_traced_run_with_writer(writer, spec, executor, BridgeObservers::default())
}

/// 用**已打开**的 trace 写端启动 run（`zgraph run` 的启动顺序：先开库/迁移，
/// 再做图加载与入口校验，最后才建 run；06 §6.10.3）。
///
/// `observers` 承载 `--stream`（[`StreamingObserver`]）与 human 实时生命周期
/// （[`LifecycleObserver`]，ISSUE-085）；语义与 [`start_traced_run`] 完全相同。
pub fn start_traced_run_with_writer(
    writer: trace::TraceWriter,
    spec: kernel::RunSpec,
    executor: Arc<dyn kernel::NodeExecutor>,
    observers: BridgeObservers,
) -> Result<TracedRun, BridgeError> {
    start_traced_run_inner(writer, spec, executor, observers, None)
}

/// 同 [`start_traced_run_with_writer`]，但由调用方**预先生成** run id。
///
/// `zgraph run` 在启动前需要 run id 构造 `llm_calls` 记录器（`LlmCallRecorder` 快照
/// 不含 run id），因此把 id 生成前移到 CLI；内核仍按同一 id 写 `RunStarted`。
pub fn start_traced_run_with_writer_and_run_id(
    writer: trace::TraceWriter,
    spec: kernel::RunSpec,
    executor: Arc<dyn kernel::NodeExecutor>,
    observers: BridgeObservers,
    run_id: String,
) -> Result<TracedRun, BridgeError> {
    start_traced_run_inner(writer, spec, executor, observers, Some(run_id))
}

/// 启动 run 的公共实现（`run_id = None` 时由内核生成 uuidv7）。
fn start_traced_run_inner(
    writer: trace::TraceWriter,
    spec: kernel::RunSpec,
    executor: Arc<dyn kernel::NodeExecutor>,
    observers: BridgeObservers,
    run_id: Option<String>,
) -> Result<TracedRun, BridgeError> {
    let trace_handle = writer.handle();
    let bridge = TraceBridge::with_observers(trace_handle.clone(), observers);
    let sink: Arc<dyn kernel::TraceSink> = Arc::new(bridge);
    let handle = match kernel::start_run(run_id, spec, executor, sink) {
        Ok(handle) => handle,
        Err(error) => {
            // 启动校验失败不产生任何事件；仍关闭写端以释放写线程。
            let _ = writer.close();
            return Err(BridgeError::Start(error));
        }
    };
    Ok(TracedRun {
        handle,
        trace_handle,
        writer: Some(writer),
    })
}

/// 便捷入口：启动 → 等待 → flush/close → 返回 outcome 与可查询的 reader。
pub async fn run_in_process(
    spec: kernel::RunSpec,
    executor: Arc<dyn kernel::NodeExecutor>,
    db_path: impl AsRef<Path>,
) -> Result<BridgeOutcome, BridgeError> {
    start_traced_run(spec, executor, db_path)?.wait().await
}

// ---------------------------------------------------------------------------
// 逐字段映射（kernel 记录 → trace 输入）
// ---------------------------------------------------------------------------

/// 映射 `RunStarted`。
fn map_run_started(info: &kernel::RunStartedInfo) -> trace::RunStartedInfo {
    trace::RunStartedInfo {
        run_id: info.run_id.clone(),
        graph_hash: info.graph_hash.clone(),
        graph_name: info.graph_name.clone(),
        dsl_json: info.dsl_json.clone(),
        user_id: info.user_id.clone(),
        entry_inputs: info.entry_inputs.clone(),
        seed: info.seed,
        record_mode: map_record_mode(&info.record_mode),
        owner_pid: info.owner_pid,
        owner_host: info.owner_host.clone(),
        started_at: info.started_at.clone(),
    }
}

/// 映射 `RunFinished`（I5 恰一次由内核 Emitter 保证）。
///
/// `STORAGE_ERROR` 属 CLI/storage 命名空间（06 §6.12.3 D3），**不写入** `error_code` 列：
/// 内核 fail-stop 的终态 `RunFinished` 本身已不带 `error`（见 kernel `fail_run_storage`），
/// 这里再做一次防御性过滤，避免未来调用方把 storage 码混进领域错误列。
fn map_run_finished(info: &kernel::RunFinishedInfo) -> trace::RunFinishedInfo {
    trace::RunFinishedInfo {
        run_id: info.run_id.clone(),
        status: map_run_status(info.status),
        output_summary: info.output_summary.clone(),
        error: info
            .error
            .as_ref()
            .filter(|error| error.code != kernel::CODE_STORAGE_ERROR)
            .map(|error| trace::TraceFailure {
                code: error.code.clone(),
                message: error.message.clone(),
            }),
        metrics: map_metrics(&info.metrics),
        at: info.at.clone(),
    }
}

/// 映射实例快照（`instances` upsert 载荷）。
fn map_snapshot(snapshot: &kernel::InstanceSnapshot) -> trace::InstanceSnapshot {
    trace::InstanceSnapshot {
        run_id: snapshot.run_id.clone(),
        instance_id: snapshot.instance_id.clone(),
        node_id: snapshot.node_id.clone(),
        node_type: snapshot.node_type.clone(),
        attempt: snapshot.attempt,
        status: map_instance_status(snapshot.status),
        input: snapshot.input.clone(),
        output: snapshot.output.clone(),
        error: snapshot.error.as_ref().map(node_error_json),
        error_code: snapshot.error_code.clone(),
        created_at: snapshot.created_at.clone(),
        started_at: snapshot.started_at.clone(),
        finished_at: snapshot.finished_at.clone(),
    }
}

/// 映射状态迁移（record + 迁移后快照）。
fn map_transition(
    record: kernel::TransitionRecord,
    snapshot: &kernel::InstanceSnapshot,
) -> (trace::TransitionRecord, trace::InstanceSnapshot) {
    let mapped = trace::TransitionRecord {
        run_id: record.run_id,
        instance_id: record.instance_id,
        node_id: record.node_id,
        attempt: record.attempt,
        from: map_instance_status(record.from),
        to: map_instance_status(record.to),
        reason: record.reason,
        ts: record.ts,
    };
    (mapped, map_snapshot(snapshot))
}

/// 映射门控结果；`edge_index` 由 `usize` 收窄为 `i64`（溢出取 `i64::MAX`）。
fn map_gate_result(record: &kernel::GateResultRecord) -> trace::GateResultRecord {
    trace::GateResultRecord {
        run_id: record.run_id.clone(),
        instance_id: record.instance_id.clone(),
        node_id: record.node_id.clone(),
        edge_index: edge_index_i64(record.edge_index),
        to_node: record.to_node.clone(),
        to_port: record.to_port.clone(),
        gate_fn: record.gate_fn.clone(),
        passed: record.passed,
        error: record.error.clone(),
        ts: record.ts.clone(),
    }
}

/// 把 `usize` 边号收窄为 `i64`（真实图规模不会溢出）。
fn edge_index_i64(edge_index: kernel::EdgeIdx) -> i64 {
    i64::try_from(edge_index).unwrap_or(i64::MAX)
}

/// 映射 run 状态（两套枚举字面量一致，逐项映射保持类型安全）。
fn map_run_status(status: kernel::RunStatus) -> trace::RunStatus {
    match status {
        kernel::RunStatus::Running => trace::RunStatus::Running,
        kernel::RunStatus::Paused => trace::RunStatus::Paused,
        kernel::RunStatus::Completed => trace::RunStatus::Completed,
        kernel::RunStatus::Failed => trace::RunStatus::Failed,
        kernel::RunStatus::Cancelled => trace::RunStatus::Cancelled,
    }
}

/// 映射实例状态。
fn map_instance_status(status: kernel::InstanceStatus) -> trace::InstanceStatus {
    match status {
        kernel::InstanceStatus::Pending => trace::InstanceStatus::Pending,
        kernel::InstanceStatus::Ready => trace::InstanceStatus::Ready,
        kernel::InstanceStatus::Running => trace::InstanceStatus::Running,
        kernel::InstanceStatus::Completed => trace::InstanceStatus::Completed,
        kernel::InstanceStatus::Crashed => trace::InstanceStatus::Crashed,
        kernel::InstanceStatus::TimedOut => trace::InstanceStatus::TimedOut,
        kernel::InstanceStatus::Cancelled => trace::InstanceStatus::Cancelled,
        kernel::InstanceStatus::Skipped => trace::InstanceStatus::Skipped,
    }
}

/// 映射 run 指标（字段一一对应）。
fn map_metrics(metrics: &kernel::RunMetrics) -> trace::RunMetrics {
    trace::RunMetrics {
        streaming_dropped: metrics.streaming_dropped,
        abandoned_tasks: metrics.abandoned_tasks,
        trace_backpressure: metrics.trace_backpressure,
        killed_workers: metrics.killed_workers,
    }
}

/// 映射流式事件（五变体一一对应）。
fn map_streaming_event(event: kernel::StreamingEvent) -> trace::StreamingEvent {
    match event {
        kernel::StreamingEvent::TokenDelta { text } => trace::StreamingEvent::TokenDelta { text },
        kernel::StreamingEvent::ReasoningDelta { text } => {
            trace::StreamingEvent::ReasoningDelta { text }
        }
        kernel::StreamingEvent::ToolCallStart {
            call_id,
            tool_name,
            args_summary,
        } => trace::StreamingEvent::ToolCallStart {
            call_id,
            tool_name,
            args_summary,
        },
        kernel::StreamingEvent::ToolCallEnd {
            call_id,
            tool_name,
            ok,
            result_summary,
        } => trace::StreamingEvent::ToolCallEnd {
            call_id,
            tool_name,
            ok,
            result_summary,
        },
        kernel::StreamingEvent::Progress { percent, message } => {
            trace::StreamingEvent::Progress { percent, message }
        }
    }
}

/// 内核记录模式字符串（`none`/`hash`/`full`）→ trace 枚举；未知值回退 `Hash`。
fn map_record_mode(record_mode: &str) -> trace::RecordMode {
    trace::RecordMode::parse(record_mode).unwrap_or(trace::RecordMode::Hash)
}

/// 把内核 `NodeError` 手动序列化为 trace `instances.error` / `transitions.error` 的 JSON。
///
/// 内核类型没有 serde 派生，这里逐变体展开；`code` 与 `TraceError.code` 对齐，
/// `PythonException` 额外保留 `traceback`（AC-05 断言）。
pub fn node_error_json(error: &kernel::NodeError) -> Value {
    let mut object = Map::new();
    object.insert("code".to_string(), json!(error.code()));
    object.insert("message".to_string(), json!(error.to_string()));
    match error {
        kernel::NodeError::BusinessError { code, .. } => {
            object.insert("kind".to_string(), json!("BusinessError"));
            object.insert("business_code".to_string(), json!(code));
        }
        kernel::NodeError::PythonException { message, traceback } => {
            object.insert("kind".to_string(), json!("PythonException"));
            object.insert("exception".to_string(), json!(message));
            object.insert("traceback".to_string(), json!(traceback));
        }
        kernel::NodeError::Timeout { elapsed_ms } => {
            object.insert("kind".to_string(), json!("Timeout"));
            object.insert("elapsed_ms".to_string(), json!(elapsed_ms));
        }
        kernel::NodeError::Cancelled => {
            object.insert("kind".to_string(), json!("Cancelled"));
        }
        kernel::NodeError::Harness { kind, detail } => {
            object.insert("kind".to_string(), json!("Harness"));
            object.insert("harness_kind".to_string(), json!(format!("{kind:?}")));
            object.insert("detail".to_string(), detail.clone());
        }
        kernel::NodeError::Config { .. } => {
            object.insert("kind".to_string(), json!("Config"));
        }
        kernel::NodeError::Internal { .. } => {
            object.insert("kind".to_string(), json!("Internal"));
        }
        kernel::NodeError::NodeWorker {
            kind,
            signal,
            exit_code,
            stderr_tail,
        } => {
            object.insert("kind".to_string(), json!("NodeWorker"));
            object.insert("worker_kind".to_string(), json!(format!("{kind:?}")));
            // ISSUE-066：崩溃证据仅在有值时输出（保持既有键集合兼容）。
            if let Some(signal) = signal {
                object.insert("signal".to_string(), json!(signal));
            }
            if let Some(code) = exit_code {
                object.insert("exit_code".to_string(), json!(code));
            }
            if let Some(tail) = stderr_tail {
                object.insert("stderr_tail".to_string(), json!(tail));
            }
        }
    }
    Value::Object(object)
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::{node_error_json, run_in_process, BridgeError, TraceBridge};
    use zgraph_kernel::{
        FakeExecutor, InstanceSnapshot, InstanceStatus, NodeError, RunOptions, RunSpec, RunStatus,
        StreamingEvent, TraceSink,
    };

    const LINEAR_JSON: &str = r#"{
      "version": "1.0",
      "metadata": {"name": "linear"},
      "entry_points": ["start"],
      "nodes": [
        {"id": "start", "type": "start",
         "inputs": [{"name": "x", "type": "any", "required": true}], "config": {}},
        {"id": "work", "type": "python_node",
         "inputs": [{"name": "x", "type": "any", "required": true}],
         "config": {"module": "workers.demo", "function": "run"}},
        {"id": "done", "type": "end",
         "inputs": [{"name": "r", "type": "any", "required": true}], "config": {}}
      ],
      "edges": [
        {"from": "start", "to_node": "work", "to_input_port": "x"},
        {"from": "work", "to_node": "done", "to_input_port": "r"}
      ]
    }"#;

    fn linear_spec() -> RunSpec {
        let loaded = zgraph_dsl::load_graph(LINEAR_JSON.as_bytes()).expect("valid dsl");
        let graph = zgraph_kernel::CompiledGraph::compile(&loaded).expect("compiled");
        let mut inputs: HashMap<String, HashMap<String, serde_json::Value>> = HashMap::new();
        inputs.insert(
            "start".to_string(),
            HashMap::from([("x".to_string(), serde_json::json!({"v": 1}))]),
        );
        RunSpec {
            graph,
            entry_inputs: inputs,
            user_id: "u1".to_string(),
            global_vars: HashMap::new(),
            // 必须从 default 出发：test-hooks 构建多一个字段（禁止 5 字段穷举）。
            options: RunOptions::default(),
        }
    }

    #[test]
    fn node_error_json_keeps_code_message_and_traceback() {
        let error = NodeError::PythonException {
            message: "RuntimeError: boom".to_string(),
            traceback: "Traceback (most recent call last):\n  boom".to_string(),
        };
        let value = node_error_json(&error);
        assert_eq!(value["code"], "NODE_PYTHON_EXCEPTION");
        assert_eq!(value["kind"], "PythonException");
        assert_eq!(value["exception"], "RuntimeError: boom");
        assert!(value["traceback"]
            .as_str()
            .expect("traceback")
            .contains("Traceback (most recent call last)"));
        assert!(value["message"]
            .as_str()
            .expect("message")
            .contains("RuntimeError: boom"));
    }

    #[test]
    fn node_error_json_covers_internal_and_timeout() {
        let internal = node_error_json(&NodeError::Internal {
            message: "bad".to_string(),
        });
        assert_eq!(internal["code"], "NODE_INTERNAL");
        assert_eq!(internal["kind"], "Internal");

        let timeout = node_error_json(&NodeError::Timeout { elapsed_ms: 100 });
        assert_eq!(timeout["code"], "NODE_TIMEOUT");
        assert_eq!(timeout["elapsed_ms"], 100);
    }

    /// ISSUE-066：NodeWorker 崩溃证据仅在 Some 时输出，既有 `kind`/`code`/
    /// `worker_kind` 保持不变（spec 06 §6.2.5.1 的 NodeWorker 形状仅增字段）。
    #[test]
    fn node_error_json_expands_node_worker_crash_details() {
        use zgraph_kernel::WorkerErrorKind;

        let signal = node_error_json(&NodeError::NodeWorker {
            kind: WorkerErrorKind::Crash,
            signal: Some(9),
            exit_code: None,
            stderr_tail: Some("killed by SIGKILL".to_string()),
        });
        assert_eq!(signal["code"], "NODE_WORKER_CRASH");
        assert_eq!(signal["kind"], "NodeWorker");
        assert_eq!(signal["worker_kind"], "Crash");
        assert_eq!(signal["signal"], 9);
        assert!(signal.get("exit_code").is_none());
        assert_eq!(signal["stderr_tail"], "killed by SIGKILL");

        let exit = node_error_json(&NodeError::NodeWorker {
            kind: WorkerErrorKind::Crash,
            signal: None,
            exit_code: Some(7),
            stderr_tail: None,
        });
        assert_eq!(exit["code"], "NODE_WORKER_CRASH");
        assert_eq!(exit["exit_code"], 7);
        assert!(exit.get("signal").is_none());
        assert!(exit.get("stderr_tail").is_none());

        // 非崩溃路径不携带崩溃证据（字段省略而不是 null）。
        let handshake = node_error_json(&NodeError::NodeWorker {
            kind: WorkerErrorKind::Handshake,
            signal: None,
            exit_code: None,
            stderr_tail: None,
        });
        assert_eq!(handshake["code"], "NODE_WORKER_HANDSHAKE");
        assert_eq!(handshake["worker_kind"], "Handshake");
        assert!(handshake.get("signal").is_none());
        assert!(handshake.get("exit_code").is_none());
        assert!(handshake.get("stderr_tail").is_none());
    }

    #[test]
    fn bridge_maps_sink_calls_into_trace_rows() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("trace.db");
        let writer = zgraph_trace::TraceWriter::open(&path).expect("writer");
        let bridge = TraceBridge::new(writer.handle());
        bridge.run_started(zgraph_kernel::RunStartedInfo {
            run_id: "run-1".to_string(),
            graph_hash: "0123456789abcdef".to_string(),
            graph_name: "demo".to_string(),
            dsl_json: "{}".to_string(),
            user_id: "local".to_string(),
            entry_inputs: serde_json::json!({}),
            seed: 7,
            record_mode: "hash".to_string(),
            owner_pid: std::process::id(),
            owner_host: "test-host".to_string(),
            started_at: "2026-10-28T09:12:03.104Z".to_string(),
        });
        bridge.instance_snapshot(InstanceSnapshot {
            run_id: "run-1".to_string(),
            instance_id: "inst-1".to_string(),
            node_id: "n1".to_string(),
            node_type: "python_node".to_string(),
            attempt: 0,
            status: InstanceStatus::Running,
            input: Some(serde_json::json!({"x": 1})),
            output: None,
            error: None,
            error_code: None,
            created_at: "2026-10-28T09:12:03.104Z".to_string(),
            started_at: None,
            finished_at: None,
        });
        assert!(bridge.streaming(
            &"run-1".to_string(),
            &"inst-1".to_string(),
            &"n1".to_string(),
            StreamingEvent::TokenDelta {
                text: "hello".to_string()
            },
            "2026-10-28T09:12:03.104Z".to_string(),
        ));
        writer.flush().expect("flush writer");

        let reader = zgraph_trace::TraceReader::open_readonly(&path).expect("reader");
        let run = reader.run("run-1").expect("run query").expect("run row");
        assert_eq!(run.status, zgraph_trace::RunStatus::Running);
        assert_eq!(run.graph_name.as_deref(), Some("demo"));
        let instances = reader.instances("run-1").expect("instances");
        assert_eq!(instances.len(), 1);
        assert_eq!(instances[0].node_id, "n1");
        assert_eq!(instances[0].status, zgraph_trace::InstanceStatus::Running);
        drop(reader);
        writer.close().expect("close writer");
    }

    #[test]
    fn lifecycle_observer_receives_events_before_writer_close() {
        // ISSUE-085：旁路回调与落库解耦——事件一产生就回调（写端未 flush/close 也能收到），
        // 且不改变 trace 行数/内容；不产生 `events` 行的快照不触发实时行。
        use std::sync::{Arc, Mutex};

        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("trace.db");
        let writer = zgraph_trace::TraceWriter::open(&path).expect("writer");
        let seen = Arc::new(Mutex::new(Vec::<&'static str>::new()));
        let observer_seen = Arc::clone(&seen);
        let observer: super::LifecycleObserver = Arc::new(move |input| {
            let kind = match input {
                zgraph_trace::TraceInput::RunStarted(_) => "RunStarted",
                zgraph_trace::TraceInput::InstanceSnapshot(_) => "InstanceSnapshot",
                zgraph_trace::TraceInput::Transition { .. } => "Transition",
                zgraph_trace::TraceInput::GateResult(_) => "GateResult",
                zgraph_trace::TraceInput::NodeTimedOut { .. } => "NodeTimedOut",
                zgraph_trace::TraceInput::NodeAbandoned { .. } => "NodeAbandoned",
                zgraph_trace::TraceInput::Streaming { .. } => "Streaming",
                zgraph_trace::TraceInput::RunFinished(_) => "RunFinished",
                zgraph_trace::TraceInput::RecordedOutput(_) => "RecordedOutput",
                zgraph_trace::TraceInput::LlmCall(_) => "LlmCall",
            };
            observer_seen.lock().expect("lock").push(kind);
        });
        let bridge = TraceBridge::with_observers(
            writer.handle(),
            super::BridgeObservers {
                streaming: None,
                lifecycle: Some(observer),
            },
        );
        bridge.run_started(zgraph_kernel::RunStartedInfo {
            run_id: "run-1".to_string(),
            graph_hash: "0123456789abcdef".to_string(),
            graph_name: "demo".to_string(),
            dsl_json: "{}".to_string(),
            user_id: "local".to_string(),
            entry_inputs: serde_json::json!({}),
            seed: 7,
            record_mode: "hash".to_string(),
            owner_pid: std::process::id(),
            owner_host: "test-host".to_string(),
            started_at: "2026-10-28T09:12:03.104Z".to_string(),
        });
        bridge.instance_snapshot(InstanceSnapshot {
            run_id: "run-1".to_string(),
            instance_id: "inst-1".to_string(),
            node_id: "n1".to_string(),
            node_type: "python_node".to_string(),
            attempt: 0,
            status: InstanceStatus::Running,
            input: Some(serde_json::json!({"x": 1})),
            output: None,
            error: None,
            error_code: None,
            created_at: "2026-10-28T09:12:03.104Z".to_string(),
            started_at: None,
            finished_at: None,
        });
        bridge.run_finished(zgraph_kernel::RunFinishedInfo {
            run_id: "run-1".to_string(),
            status: RunStatus::Completed,
            output_summary: Some(serde_json::json!({"r": 1})),
            error: None,
            metrics: zgraph_kernel::RunMetrics::default(),
            at: "2026-10-28T09:12:04.104Z".to_string(),
        });

        // 实时性：写端尚未 flush/close，回调已按事件顺序收到（快照不产生 events 行）。
        assert_eq!(
            seen.lock().expect("lock").as_slice(),
            ["RunStarted", "RunFinished"]
        );

        // 落库语义不变：仍只有两条 events 行（快照只 upsert instances）。
        writer.flush().expect("flush writer");
        let reader = zgraph_trace::TraceReader::open_readonly(&path).expect("reader");
        let events = reader
            .events("run-1", &zgraph_trace::EventQuery::default())
            .expect("events");
        assert_eq!(
            events
                .iter()
                .map(|event| event.kind.as_str())
                .collect::<Vec<_>>(),
            ["RunStarted", "RunFinished"]
        );
        drop(reader);
        writer.close().expect("close writer");
    }

    #[test]
    fn streaming_returns_false_after_writer_closed() {
        // I6：写端关闭后 send_streaming 返回 false，内核据此累计 streaming_dropped。
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("trace.db");
        let writer = zgraph_trace::TraceWriter::open(&path).expect("writer");
        let bridge = TraceBridge::new(writer.handle());
        writer.close().expect("close writer");
        let accepted = bridge.streaming(
            &"run-1".to_string(),
            &"inst-1".to_string(),
            &"n1".to_string(),
            StreamingEvent::TokenDelta {
                text: "x".to_string(),
            },
            "2026-10-28T09:12:03.104Z".to_string(),
        );
        assert!(!accepted, "closed writer must report the event as dropped");
    }

    #[tokio::test]
    async fn run_in_process_persists_lifecycle_and_output() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("trace.db");
        let executor = FakeExecutor::new().respond("work", serde_json::json!({"ok": true}));
        let result = run_in_process(linear_spec(), std::sync::Arc::new(executor), &path).await;
        let result = result.expect("bridge run");

        assert_eq!(result.outcome.status, RunStatus::Completed);
        assert_eq!(
            result.outcome.output,
            Some(serde_json::json!({"r": {"ok": true}}))
        );

        let run_id = result.outcome.run_id.clone();
        let events = result
            .reader()
            .events(&run_id, &zgraph_trace::EventQuery::default())
            .expect("events");
        assert_eq!(
            events.first().map(|event| event.kind.as_str()),
            Some("RunStarted")
        );
        assert_eq!(
            events.last().map(|event| event.kind.as_str()),
            Some("RunFinished")
        );
        assert_eq!(
            events
                .iter()
                .filter(|event| event.kind == "RunStarted")
                .count(),
            1
        );
        assert_eq!(
            events
                .iter()
                .filter(|event| event.kind == "RunFinished")
                .count(),
            1
        );
        let seqs: Vec<i64> = events.iter().map(|event| event.seq).collect();
        assert!(seqs.windows(2).all(|pair| pair[0] < pair[1]), "{seqs:?}");

        let run = result
            .reader()
            .run(&run_id)
            .expect("run query")
            .expect("row");
        assert_eq!(run.status, zgraph_trace::RunStatus::Completed);
        assert_eq!(
            run.output_json,
            Some(serde_json::json!({"r": {"ok": true}}))
        );
    }

    #[tokio::test]
    async fn start_error_does_not_leave_open_writer() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("trace.db");
        let mut spec = linear_spec();
        spec.entry_inputs.clear(); // 缺 required 端口 → RunStartError
        let executor = FakeExecutor::new();
        let result = run_in_process(spec, std::sync::Arc::new(executor), &path).await;
        assert!(matches!(result, Err(BridgeError::Start(_))));
    }
}
