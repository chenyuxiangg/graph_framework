//! `WorkerExecutor`：把内核 [`NodeExecutor`] 契约映射到 Python worker 子进程（M3）。
//!
//! 设计依据：`doc/architecture-v2.md` §1/§4（D1 超时 kill 进程组、D2 跨进程 ctx、
//! D4 全局/每 run 并发上限、D10 `NodeError::NodeWorker`）。
//!
//! 语义要点：
//! - **超时由内核判定**：内核 deadline 到点会 abort 执行任务；future 被 drop 时
//!   [`crate::supervisor::Supervisor`] 的 Drop 同步对**进程组**发 SIGKILL，
//!   保证 5s 内无残留（含孙进程）。执行器自身只给 supervisor 留兜底超时。
//! - **emit 流式**：supervisor 每接纳一帧 emit 就回调，本执行器立即 `ctx.emit`，
//!   不攒到 result 后批量投递。
//! - **并发上限**：每 run 信号量（默认 8）先于全局信号量（默认 CPU×2）获取，
//!   固定加锁顺序避免死锁；崩溃只影响本节点（子进程隔离）。

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde_json::{Map, Value};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use zgraph_kernel::{
    CompiledHandler, CompiledNode, NodeContext, NodeError, NodeExecution, NodeExecutor, PortName,
    StreamingEvent, WorkerErrorKind,
};

use crate::protocol::{
    Frame, RunRequest, WorkerError, CODE_CANCELLED, CODE_CRASH, CODE_HANDSHAKE, CODE_IMPORT,
    CODE_PROTOCOL, CODE_PYTHON_EXCEPTION,
};
use crate::spawn::WorkerFailure;
use crate::supervisor::{no_cancel, SupervisorConfig};

/// 每 run 默认并发上限（架构 §4 / D4）。
pub const DEFAULT_MAX_WORKERS_PER_RUN: usize = 8;

/// supervisor 兜底超时相对内核 deadline 的余量。
///
/// 必须 > 0：内核 deadline 是权威判据（04 §4.1.4），执行器抢跑只会把 TimedOut
/// 变成 Internal。Drop 路径负责真正的进程组回收。
const DEADLINE_SLACK: Duration = Duration::from_secs(3);

/// 全局 worker 并发上限默认值：CPU 核数 × 2（架构 §4；D4）。
pub fn default_max_workers() -> usize {
    std::thread::available_parallelism()
        .map_or(2, |parallelism| parallelism.get())
        .saturating_mul(2)
}

/// 基于 Python worker 子进程的节点执行器。
pub struct WorkerExecutor {
    config: SupervisorConfig,
    global: Arc<Semaphore>,
    per_run: Mutex<HashMap<String, Arc<Semaphore>>>,
    max_workers: usize,
    max_workers_per_run: usize,
}

impl WorkerExecutor {
    /// 用默认并发上限（全局 CPU×2、每 run 8）构造执行器。
    pub fn new(config: SupervisorConfig) -> Self {
        Self::with_limits(config, default_max_workers(), DEFAULT_MAX_WORKERS_PER_RUN)
    }

    /// 显式指定全局/每 run 并发上限（测试与调优用；0 会被提升为 1）。
    pub fn with_limits(
        config: SupervisorConfig,
        max_workers: usize,
        max_workers_per_run: usize,
    ) -> Self {
        let max_workers = max_workers.max(1);
        let max_workers_per_run = max_workers_per_run.max(1);
        Self {
            config,
            global: Arc::new(Semaphore::new(max_workers)),
            per_run: Mutex::new(HashMap::new()),
            max_workers,
            max_workers_per_run,
        }
    }

    /// 全局并发上限（配置值，不是当前剩余许可）。
    pub fn max_workers(&self) -> usize {
        self.max_workers
    }

    /// 每 run 并发上限。
    pub fn max_workers_per_run(&self) -> usize {
        self.max_workers_per_run
    }

    /// 取当前 run 的信号量（懒创建；run 结束不主动清理，见模块文档与评审风险）。
    fn run_semaphore(&self, run_id: &str) -> Arc<Semaphore> {
        let mut map = match self.per_run.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        map.entry(run_id.to_string())
            .or_insert_with(|| Arc::new(Semaphore::new(self.max_workers_per_run)))
            .clone()
    }

    /// 为本节点派生 supervisor 配置：总超时 = 内核剩余 deadline + 兜底余量。
    fn config_for(&self, ctx: &NodeContext) -> SupervisorConfig {
        let mut config = self.config.clone();
        let remaining = ctx.node_deadline_remaining_ms();
        config.worker.timeout = Duration::from_millis(
            remaining.saturating_add(u64::try_from(DEADLINE_SLACK.as_millis()).unwrap_or(u64::MAX)),
        );
        config
    }

    /// 获取每 run 信号量许可（先于全局，固定顺序）。
    async fn acquire_run(&self, run_id: &str) -> Result<OwnedSemaphorePermit, NodeError> {
        self.run_semaphore(run_id)
            .acquire_owned()
            .await
            .map_err(|_| internal("worker run semaphore closed"))
    }

    /// 获取全局信号量许可。
    async fn acquire_global(&self) -> Result<OwnedSemaphorePermit, NodeError> {
        self.global
            .clone()
            .acquire_owned()
            .await
            .map_err(|_| internal("worker global semaphore closed"))
    }
}

impl NodeExecutor for WorkerExecutor {
    fn execute<'a>(
        &'a self,
        execution: NodeExecution<'a>,
    ) -> Pin<Box<dyn Future<Output = Result<Value, NodeError>> + Send + 'a>> {
        Box::pin(async move {
            let (module, function) = python_handler(execution.node)?;
            let request = build_run_request(execution.ctx, &module, &function, execution.inputs);
            // 先每 run、后全局：所有任务同序加锁，不存在交叉等待（无死锁）。
            let _run_permit = self.acquire_run(execution.ctx.run_id()).await?;
            let _global_permit = self.acquire_global().await?;
            let config = self.config_for(execution.ctx);
            let ctx = execution.ctx;
            // emit 即时转发：supervisor 逐帧回调 → 内核可丢 streaming 通道（I6）。
            let mut on_frame = |frame: &Frame| {
                if let Some(event) = streaming_event(frame) {
                    ctx.emit(event);
                }
            };
            let result =
                crate::supervisor::run_supervised(&config, &request, &mut on_frame, no_cancel())
                    .await;
            result.map(|run| run.output).map_err(map_failure)
        })
    }
}

/// 从编译节点取 Python 入口；内核不应把其它类型交给 worker 执行器。
fn python_handler(node: &CompiledNode) -> Result<(String, String), NodeError> {
    match &node.handler {
        CompiledHandler::Python {
            module, function, ..
        } => Ok((module.clone(), function.clone())),
        other => Err(NodeError::Internal {
            message: format!(
                "worker executor only supports python_node (node '{}' has {other:?})",
                node.id
            ),
        }),
    }
}

/// 由 `NodeContext` + 派发 inputs 构造 `run` 载荷（协议 §2 `run` 行）。
fn build_run_request(
    ctx: &NodeContext,
    module: &str,
    function: &str,
    inputs: &HashMap<PortName, Value>,
) -> RunRequest {
    RunRequest {
        run_id: ctx.run_id().to_string(),
        instance_id: ctx.instance_id().to_string(),
        node_id: ctx.node_id().to_string(),
        attempt: ctx.attempt(),
        module: module.to_string(),
        function: function.to_string(),
        inputs: serde_json::to_value(inputs).unwrap_or(Value::Null),
        params: Value::Object(Map::new()),
        user_id: ctx.user_id().to_string(),
        global_vars: serde_json::to_value(ctx.global_vars()).unwrap_or(Value::Null),
        seed: ctx.seed(),
        deadline_ms: epoch_millis().saturating_add(ctx.node_deadline_remaining_ms()),
        trace_id: ctx.run_id().to_string(),
    }
}

/// 当前 Unix 毫秒时间戳（`deadline_ms` 的基准；与 Python `time.time()*1000` 同源）。
fn epoch_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| {
            u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX)
        })
}

/// emit 帧 → 内核 `StreamingEvent`（协议 §2 封闭表；未知 type 返回 `None` 丢弃）。
pub fn streaming_event(frame: &Frame) -> Option<StreamingEvent> {
    if frame.kind != "emit" {
        return None;
    }
    let payload = &frame.payload;
    let event_type = payload.get("type")?.as_str()?;
    let text = || {
        payload
            .get("text")
            .and_then(Value::as_str)
            .map(str::to_string)
    };
    Some(match event_type {
        "token" => StreamingEvent::TokenDelta { text: text()? },
        "reasoning" => StreamingEvent::ReasoningDelta { text: text()? },
        "tool_call_start" => StreamingEvent::ToolCallStart {
            call_id: payload.get("call_id")?.as_str()?.to_string(),
            tool_name: payload.get("tool_name")?.as_str()?.to_string(),
            args_summary: payload.get("args_summary").cloned().unwrap_or(Value::Null),
        },
        "tool_call_end" => StreamingEvent::ToolCallEnd {
            call_id: payload.get("call_id")?.as_str()?.to_string(),
            tool_name: payload.get("tool_name")?.as_str()?.to_string(),
            ok: payload.get("ok").and_then(Value::as_bool).unwrap_or(false),
            result_summary: payload.get("result_summary").cloned(),
        },
        "progress" => StreamingEvent::Progress {
            percent: u8::try_from(payload.get("percent")?.as_u64()?).ok()?,
            message: payload.get("message")?.as_str()?.to_string(),
        },
        _ => return None,
    })
}

/// supervisor 失败 → `NodeError`（D10 / 04 §4.1.4 的跨进程扩展）。
pub fn map_failure(failure: WorkerFailure) -> NodeError {
    match failure {
        WorkerFailure::Worker(error) => map_worker_error(&error),
        WorkerFailure::Protocol(error) => NodeError::NodeWorker {
            kind: protocol_kind(error.code),
        },
        // 04 §4.1.4 规则 1：只有 AgentCallHandler 允许自报 Timeout；worker 帧/调度
        // 停滞都按协议错误处理（Lead 批准口径），避免被内核归一化成 Internal。
        WorkerFailure::Timeout(_) => NodeError::NodeWorker {
            kind: WorkerErrorKind::Protocol,
        },
        WorkerFailure::Cancelled => NodeError::Cancelled,
        WorkerFailure::Spawn(message) => NodeError::Config {
            message: format!("spawn worker: {message}"),
        },
        WorkerFailure::Exit { .. } => NodeError::NodeWorker {
            kind: WorkerErrorKind::Crash,
        },
    }
}

/// worker `error` 帧码 → `NodeError`（§8.2/D10；业务码保留原 code 进入 `BusinessError`）。
pub fn map_worker_error(error: &WorkerError) -> NodeError {
    match error.code.as_str() {
        CODE_HANDSHAKE => NodeError::NodeWorker {
            kind: WorkerErrorKind::Handshake,
        },
        CODE_PROTOCOL => NodeError::NodeWorker {
            kind: WorkerErrorKind::Protocol,
        },
        CODE_IMPORT => NodeError::NodeWorker {
            kind: WorkerErrorKind::Import,
        },
        CODE_CRASH => NodeError::NodeWorker {
            kind: WorkerErrorKind::Crash,
        },
        CODE_PYTHON_EXCEPTION => NodeError::PythonException {
            message: error.message.clone(),
            traceback: error.traceback.clone().unwrap_or_default(),
        },
        CODE_CANCELLED => NodeError::Cancelled,
        // `NODE_BUSINESS_ERROR` / `NODE_RETURN_NOT_DICT` / `BIZ_*` 及自定义业务码：
        // Python `BusinessError.code` 是载荷值，必须原样保留（04 §4.3.4）。
        _ => NodeError::BusinessError {
            code: error.code.clone(),
            message: error.message.clone(),
        },
    }
}

/// 协议错误码 → `WorkerErrorKind`（未知码按协议错误处理）。
fn protocol_kind(code: &str) -> WorkerErrorKind {
    match code {
        CODE_HANDSHAKE => WorkerErrorKind::Handshake,
        CODE_IMPORT => WorkerErrorKind::Import,
        _ => WorkerErrorKind::Protocol,
    }
}

/// 构造 `Internal` 错误（并发信号量意外关闭属于框架不变量违反）。
fn internal(message: impl Into<String>) -> NodeError {
    NodeError::Internal {
        message: message.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::CODE_BUSINESS_ERROR;
    use serde_json::json;

    fn worker_error(code: &str) -> WorkerError {
        WorkerError {
            code: code.to_string(),
            message: "boom".to_string(),
            traceback: Some("trace".to_string()),
            details: None,
        }
    }

    #[test]
    fn protocol_error_codes_map_to_worker_kinds() {
        let cases = [
            (CODE_HANDSHAKE, WorkerErrorKind::Handshake),
            (CODE_PROTOCOL, WorkerErrorKind::Protocol),
            (CODE_IMPORT, WorkerErrorKind::Import),
            (CODE_CRASH, WorkerErrorKind::Crash),
        ];
        for (code, kind) in cases {
            assert_eq!(
                map_worker_error(&worker_error(code)),
                NodeError::NodeWorker { kind },
                "code {code}"
            );
        }
    }

    #[test]
    fn python_exception_keeps_message_and_traceback() {
        match map_worker_error(&worker_error(CODE_PYTHON_EXCEPTION)) {
            NodeError::PythonException { message, traceback } => {
                assert_eq!(message, "boom");
                assert_eq!(traceback, "trace");
            }
            other => panic!("unexpected mapping: {other:?}"),
        }
    }

    #[test]
    fn business_codes_keep_original_code() {
        assert_eq!(
            map_worker_error(&worker_error(CODE_BUSINESS_ERROR)),
            NodeError::BusinessError {
                code: CODE_BUSINESS_ERROR.to_string(),
                message: "boom".to_string(),
            }
        );
        assert_eq!(
            map_worker_error(&worker_error("BIZ_ORDER_NOT_FOUND")),
            NodeError::BusinessError {
                code: "BIZ_ORDER_NOT_FOUND".to_string(),
                message: "boom".to_string(),
            }
        );
    }

    #[test]
    fn cancelled_code_maps_to_cancelled() {
        assert_eq!(
            map_worker_error(&worker_error(CODE_CANCELLED)),
            NodeError::Cancelled
        );
    }

    #[test]
    fn supervisor_timeout_is_not_mapped_to_timeout_error() {
        // 04 §4.1.4：非 AgentCallHandler 不得自报 Timeout（否则内核归一化 Internal）。
        assert_eq!(
            map_failure(WorkerFailure::Timeout(Duration::from_secs(1))),
            NodeError::NodeWorker {
                kind: WorkerErrorKind::Protocol
            }
        );
    }

    #[test]
    fn crash_exit_maps_to_crash_kind() {
        assert_eq!(
            map_failure(WorkerFailure::Exit {
                code: None,
                stderr: String::new()
            }),
            NodeError::NodeWorker {
                kind: WorkerErrorKind::Crash
            }
        );
    }

    #[test]
    fn spawn_failure_maps_to_config_error() {
        match map_failure(WorkerFailure::Spawn("no python".to_string())) {
            NodeError::Config { message } => assert!(message.contains("no python")),
            other => panic!("unexpected mapping: {other:?}"),
        }
    }

    fn emit(payload: Value) -> Frame {
        Frame::new("emit", payload)
    }

    #[test]
    fn emit_frames_map_to_all_streaming_variants() {
        assert_eq!(
            streaming_event(&emit(json!({"type": "token", "text": "hi"}))),
            Some(StreamingEvent::TokenDelta {
                text: "hi".to_string()
            })
        );
        assert_eq!(
            streaming_event(&emit(json!({"type": "reasoning", "text": "r"}))),
            Some(StreamingEvent::ReasoningDelta {
                text: "r".to_string()
            })
        );
        assert_eq!(
            streaming_event(&emit(json!({
                "type": "tool_call_start",
                "call_id": "c1",
                "tool_name": "search",
                "args_summary": {"q": "x"}
            }))),
            Some(StreamingEvent::ToolCallStart {
                call_id: "c1".to_string(),
                tool_name: "search".to_string(),
                args_summary: json!({"q": "x"}),
            })
        );
        assert_eq!(
            streaming_event(&emit(json!({
                "type": "tool_call_end",
                "call_id": "c1",
                "tool_name": "search",
                "ok": true
            }))),
            Some(StreamingEvent::ToolCallEnd {
                call_id: "c1".to_string(),
                tool_name: "search".to_string(),
                ok: true,
                result_summary: None,
            })
        );
        assert_eq!(
            streaming_event(&emit(json!({
                "type": "progress",
                "percent": 42,
                "message": "half"
            }))),
            Some(StreamingEvent::Progress {
                percent: 42,
                message: "half".to_string(),
            })
        );
    }

    #[test]
    fn unknown_emit_types_are_dropped_not_fatal() {
        assert_eq!(
            streaming_event(&emit(json!({"type": "unknown", "text": "x"}))),
            None
        );
        assert_eq!(streaming_event(&emit(json!({"text": "no-type"}))), None);
        assert_eq!(streaming_event(&Frame::new("log", json!({}))), None);
    }
}
