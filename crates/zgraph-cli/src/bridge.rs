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

/// kernel `TraceSink` 的 trace 写端适配器。
///
/// 每次 sink 调用映射为一条 [`trace::TraceInput`]；`streaming` 使用可丢发送，
/// 返回 `false` 让内核累计 `streaming_dropped`。
pub struct TraceBridge {
    handle: trace::TraceHandle,
}

impl TraceBridge {
    /// 用 trace 写句柄构造桥接器（句柄可继续克隆给其他生产者，但 M2 只有本桥）。
    pub fn new(handle: trace::TraceHandle) -> Self {
        Self { handle }
    }

    /// 底层写句柄（诊断/错误检查用）。
    pub fn handle(&self) -> &trace::TraceHandle {
        &self.handle
    }
}

impl kernel::TraceSink for TraceBridge {
    fn run_started(&self, info: kernel::RunStartedInfo) {
        self.handle
            .send(trace::TraceInput::RunStarted(map_run_started(&info)));
    }

    fn instance_snapshot(&self, snapshot: kernel::InstanceSnapshot) {
        self.handle
            .send(trace::TraceInput::InstanceSnapshot(map_snapshot(&snapshot)));
    }

    fn transition(&self, record: kernel::TransitionRecord, snapshot: kernel::InstanceSnapshot) {
        let (record, snapshot) = map_transition(record, &snapshot);
        self.handle
            .send(trace::TraceInput::Transition { record, snapshot });
    }

    fn gate_result(&self, record: kernel::GateResultRecord) {
        self.handle
            .send(trace::TraceInput::GateResult(map_gate_result(&record)));
    }

    fn node_timed_out(
        &self,
        run_id: &kernel::RunId,
        instance_id: &kernel::InstanceId,
        elapsed_ms: u64,
        at: String,
    ) {
        self.handle.send(trace::TraceInput::NodeTimedOut {
            run_id: run_id.clone(),
            instance_id: instance_id.clone(),
            elapsed_ms,
            at,
        });
    }

    fn node_abandoned(&self, run_id: &kernel::RunId, instance_id: &kernel::InstanceId, at: String) {
        self.handle.send(trace::TraceInput::NodeAbandoned {
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
        self.handle.send_streaming(trace::TraceInput::Streaming {
            run_id: run_id.clone(),
            instance_id: instance_id.clone(),
            node_id: Some(node_id.clone()),
            event: map_streaming_event(event),
            at,
        })
    }

    fn run_finished(&self, info: kernel::RunFinishedInfo) {
        self.handle
            .send(trace::TraceInput::RunFinished(map_run_finished(&info)));
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
    pub async fn wait(mut self) -> Result<BridgeOutcome, BridgeError> {
        let outcome = self.handle.wait().await;
        let writer = self
            .writer
            .take()
            .ok_or_else(|| BridgeError::Writer("trace writer already closed".to_string()))?;
        let db_path = writer.db_path().to_path_buf();
        let flush_result = writer.flush();
        let trace_error = self.trace_handle.take_error();
        let close_result = writer.close();
        flush_result?;
        close_result?;
        if let Some(message) = trace_error {
            return Err(BridgeError::Writer(message));
        }
        let reader = trace::TraceReader::open_readonly(&db_path)?;
        Ok(BridgeOutcome {
            outcome,
            reader,
            db_path,
        })
    }
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
    let trace_handle = writer.handle();
    let sink: Arc<dyn kernel::TraceSink> = Arc::new(TraceBridge::new(trace_handle.clone()));
    let handle = match kernel::start_run(None, spec, executor, sink) {
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
fn map_run_finished(info: &kernel::RunFinishedInfo) -> trace::RunFinishedInfo {
    trace::RunFinishedInfo {
        run_id: info.run_id.clone(),
        status: map_run_status(info.status),
        output_summary: info.output_summary.clone(),
        error: info.error.as_ref().map(|error| trace::TraceFailure {
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
        kernel::NodeError::NodeWorker { kind } => {
            object.insert("kind".to_string(), json!("NodeWorker"));
            object.insert("worker_kind".to_string(), json!(format!("{kind:?}")));
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
