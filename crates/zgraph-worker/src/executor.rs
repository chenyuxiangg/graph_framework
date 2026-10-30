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
//!   固定加锁顺序避免死锁；崩溃只影响本节点（子进程隔离）。上限句柄
//!   [`WorkerLimits`] 可被 CLI 注入给 Harness 共享（ISSUE-079：工具调用与节点执行
//!   共用同一实例，合计不超上限）。

use std::collections::{BTreeSet, HashMap};
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
    Frame, Hello, RunRequest, WorkerError, CODE_CANCELLED, CODE_CRASH, CODE_HANDSHAKE, CODE_IMPORT,
    CODE_PROTOCOL, CODE_PYTHON_EXCEPTION, CODE_TIMEOUT, EMIT_TYPES,
};
use crate::spawn::WorkerFailure;
use crate::supervisor::{no_cancel, run_supervised_observed, SupervisorConfig};

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

/// 全局 / 每 run 并发上限的**共享句柄**（D4；ISSUE-079）。
///
/// 两个信号量都在内部持有，只暴露只读上限与获取许可的方法。当 [`WorkerExecutor`]
/// 与工具调用（Harness）使用**同一个实例**时，节点执行与工具调用合计不超过
/// 全局 / 每 run 上限（CLI 组合根负责创建并注入）。
pub struct WorkerLimits {
    global: Arc<Semaphore>,
    per_run: Mutex<HashMap<String, Arc<Semaphore>>>,
    max_workers: usize,
    max_workers_per_run: usize,
}

impl std::fmt::Debug for WorkerLimits {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WorkerLimits")
            .field("max_workers", &self.max_workers)
            .field("max_workers_per_run", &self.max_workers_per_run)
            .finish_non_exhaustive()
    }
}

impl WorkerLimits {
    /// 显式指定全局 / 每 run 容量（0 会被提升为 1）。
    pub fn new(max_workers: usize, max_workers_per_run: usize) -> Self {
        let max_workers = max_workers.max(1);
        let max_workers_per_run = max_workers_per_run.max(1);
        Self {
            global: Arc::new(Semaphore::new(max_workers)),
            per_run: Mutex::new(HashMap::new()),
            max_workers,
            max_workers_per_run,
        }
    }

    /// 默认容量：全局 CPU×2、每 run 8（架构 §4 / D4）。
    pub fn default_limits() -> Self {
        Self::new(default_max_workers(), DEFAULT_MAX_WORKERS_PER_RUN)
    }

    /// 全局并发上限（配置值，不是当前剩余许可）。
    pub fn max_workers(&self) -> usize {
        self.max_workers
    }

    /// 每 run 并发上限。
    pub fn max_workers_per_run(&self) -> usize {
        self.max_workers_per_run
    }

    /// 取指定 run 的信号量（懒创建；run 结束不主动清理，见 ISSUE-065 ②）。
    fn run_semaphore(&self, run_id: &str) -> Arc<Semaphore> {
        let mut map = match self.per_run.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        map.entry(run_id.to_string())
            .or_insert_with(|| Arc::new(Semaphore::new(self.max_workers_per_run)))
            .clone()
    }

    /// 获取每 run 许可；信号量意外关闭（框架不变量违反）→ `None`。
    pub async fn acquire_run(&self, run_id: &str) -> Option<OwnedSemaphorePermit> {
        self.run_semaphore(run_id).acquire_owned().await.ok()
    }

    /// 获取全局许可；信号量意外关闭（框架不变量违反）→ `None`。
    pub async fn acquire_global(&self) -> Option<OwnedSemaphorePermit> {
        self.global.clone().acquire_owned().await.ok()
    }
}

/// 基于 Python worker 子进程的节点执行器。
pub struct WorkerExecutor {
    config: SupervisorConfig,
    limits: Arc<WorkerLimits>,
    /// 每 run 的 worker/SDK/Python 版本集合（ISSUE-082；握手 hello 聚合、去重排序）。
    worker_versions: Mutex<HashMap<String, BTreeSet<(String, String)>>>,
}

impl WorkerExecutor {
    /// 用默认并发上限（全局 CPU×2、每 run 8）构造执行器。
    pub fn new(config: SupervisorConfig) -> Self {
        Self::with_shared_limits(config, Arc::new(WorkerLimits::default_limits()))
    }

    /// 显式指定全局/每 run 并发上限（测试与调优用；0 会被提升为 1）。
    pub fn with_limits(
        config: SupervisorConfig,
        max_workers: usize,
        max_workers_per_run: usize,
    ) -> Self {
        Self::with_shared_limits(
            config,
            Arc::new(WorkerLimits::new(max_workers, max_workers_per_run)),
        )
    }

    /// 使用调用方提供的共享上限句柄（ISSUE-079：可与 Harness 工具调用共享同一实例）。
    pub fn with_shared_limits(config: SupervisorConfig, limits: Arc<WorkerLimits>) -> Self {
        Self {
            config,
            limits,
            worker_versions: Mutex::new(HashMap::new()),
        }
    }

    /// 全局并发上限（配置值，不是当前剩余许可）。
    pub fn max_workers(&self) -> usize {
        self.limits.max_workers()
    }

    /// 每 run 并发上限。
    pub fn max_workers_per_run(&self) -> usize {
        self.limits.max_workers_per_run()
    }

    /// 共享上限句柄（只读克隆；CLI 组合根可注入给 Harness，见 ISSUE-079）。
    pub fn limits(&self) -> Arc<WorkerLimits> {
        self.limits.clone()
    }

    /// 本 run 观察到的 worker 版本集合（ISSUE-082；去重排序后的 JSON 对象数组）。
    ///
    /// 数据源 = worker 握手 `hello` 的 `python_version`/`sdk_version`（协议 v1 不变）；
    /// 同一 run 内多个 worker 进程按 `(python, sdk)` 去重升序。`None` = 本 run
    /// 尚未成功握手任何 worker（CLI 按 `[]` 写入）。
    ///
    /// **范围（REV-048 C2）**：仅聚合**节点 worker**（经 [`NodeExecutor::execute`] →
    /// [`run_supervised_observed`] 的 hello）；Harness 工具 worker 走
    /// `run_worker_controlled`（无 hello 观测），因此「仅 agent + tool」的 run 会写
    /// `[]`。该口径已随 REV-057 回写 06 §6.2.4。
    pub fn worker_versions(&self, run_id: &str) -> Option<Vec<Value>> {
        let guard = match self.worker_versions.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        let versions = guard.get(run_id)?;
        Some(
            versions
                .iter()
                .map(|(python_version, sdk_version)| {
                    serde_json::json!({
                        "python_version": python_version,
                        "sdk_version": sdk_version,
                    })
                })
                .collect(),
        )
    }

    /// 记录一次握手 `hello`（ISSUE-082；去重由 `BTreeSet` 保证）。
    fn record_hello(&self, run_id: &str, hello: &Hello) {
        let mut guard = match self.worker_versions.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        guard
            .entry(run_id.to_string())
            .or_default()
            .insert((hello.python_version.clone(), hello.sdk_version.clone()));
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
        self.limits
            .acquire_run(run_id)
            .await
            .ok_or_else(|| internal("worker run semaphore closed"))
    }

    /// 获取全局信号量许可。
    async fn acquire_global(&self) -> Result<OwnedSemaphorePermit, NodeError> {
        self.limits
            .acquire_global()
            .await
            .ok_or_else(|| internal("worker global semaphore closed"))
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
            let (hello, result) =
                run_supervised_observed(&config, &request, &mut on_frame, no_cancel()).await;
            if let Some(hello) = hello {
                // 失败 run 也尽量记录（只要握手成功）。
                self.record_hello(ctx.run_id(), &hello);
            }
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
    // 协议 §2 的 emit type 封闭表；白名单外一律丢弃（不把未知事件映射进内核）。
    if !EMIT_TYPES.contains(&event_type) {
        return None;
    }
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
        WorkerFailure::Protocol(error) => node_worker_error(protocol_kind(error.code)),
        // 04 §4.1.4 规则 1：只有 AgentCallHandler 允许自报 Timeout；worker 帧/调度
        // 停滞都按协议错误处理（Lead 批准口径），避免被内核归一化成 Internal。
        WorkerFailure::Timeout(_) => node_worker_error(WorkerErrorKind::Protocol),
        WorkerFailure::Cancelled => NodeError::Cancelled,
        // spec 03 §3.9.4 步 1：worker spawn 失败（解释器不存在/exec 失败等）→
        // `NODE_WORKER_CRASH`；`NODE_CONFIG_ERROR` 只覆盖「非 Harness 资产加载失败」
        // （spec 00 §0.4），不覆盖进程启动失败。spawn 失败没有子进程退出证据，
        // 故 `signal`/`exit_code`/`stderr_tail` 均为 `None`（OS 错误细节仍无承载字段）。
        WorkerFailure::Spawn(_) => node_worker_error(WorkerErrorKind::Crash),
        // ISSUE-066：崩溃必须留下 signal/exit/stderr 证据（architecture §8.2）。
        WorkerFailure::Exit {
            code,
            signal,
            stderr,
        } => NodeError::NodeWorker {
            kind: WorkerErrorKind::Crash,
            signal,
            exit_code: code,
            stderr_tail: stderr_tail(&stderr),
        },
    }
}

/// 构造不带崩溃证据的 worker 错误（握手/协议/导入/worker 自报错误）。
fn node_worker_error(kind: WorkerErrorKind) -> NodeError {
    NodeError::NodeWorker {
        kind,
        signal: None,
        exit_code: None,
        stderr_tail: None,
    }
}

/// stderr 尾部上限（字节；ISSUE-066：崩溃载荷只保留 1 KiB，避免撑大 error_json）。
pub const STDERR_TAIL_BYTES: usize = 1024;

/// 取 stderr 尾部（≤ [`STDERR_TAIL_BYTES`]），按 UTF-8 字符边界截断；
/// 空白内容返回 `None`（CLI 仅在 `Some` 时输出该字段）。
fn stderr_tail(stderr: &str) -> Option<String> {
    if stderr.trim().is_empty() {
        return None;
    }
    if stderr.len() <= STDERR_TAIL_BYTES {
        return Some(stderr.to_string());
    }
    let mut start = stderr.len() - STDERR_TAIL_BYTES;
    while !stderr.is_char_boundary(start) {
        start += 1;
    }
    Some(stderr[start..].to_string())
}

/// worker `error` 帧码 → `NodeError`（§8.2/D10；业务码保留原 code 进入 `BusinessError`）。
pub fn map_worker_error(error: &WorkerError) -> NodeError {
    match error.code.as_str() {
        CODE_HANDSHAKE => node_worker_error(WorkerErrorKind::Handshake),
        CODE_PROTOCOL => node_worker_error(WorkerErrorKind::Protocol),
        CODE_IMPORT => node_worker_error(WorkerErrorKind::Import),
        CODE_CRASH => node_worker_error(WorkerErrorKind::Crash),
        CODE_PYTHON_EXCEPTION => NodeError::PythonException {
            message: error.message.clone(),
            traceback: error.traceback.clone().unwrap_or_default(),
        },
        CODE_CANCELLED => NodeError::Cancelled,
        // worker 自报 `NODE_TIMEOUT` 属协议违例：04 §4.1.4 规定 Timeout 只由内核
        // deadline 或 AgentCallHandler 预检产生，因此按协议错误处理（不映射 Timeout，
        // 否则会被内核归一化成 Internal，掩盖真实问题）。
        CODE_TIMEOUT => node_worker_error(WorkerErrorKind::Protocol),
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
    use crate::protocol::{CODE_BUSINESS_ERROR, PROTOCOL_VERSION};
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
                node_worker_error(kind),
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
            node_worker_error(WorkerErrorKind::Protocol)
        );
    }

    /// ISSUE-066：被信号杀死时 NodeError 必须保留 signal，且不伪造退出码。
    #[test]
    fn crash_exit_records_signal_and_stderr_tail() {
        let mapped = map_failure(WorkerFailure::Exit {
            code: None,
            signal: Some(9),
            stderr: "fatal: killed by SIGKILL\n".to_string(),
        });
        assert_eq!(mapped.code(), "NODE_WORKER_CRASH");
        match mapped {
            NodeError::NodeWorker {
                kind: WorkerErrorKind::Crash,
                signal,
                exit_code,
                stderr_tail,
            } => {
                assert_eq!(signal, Some(9));
                assert_eq!(exit_code, None);
                assert_eq!(stderr_tail.as_deref(), Some("fatal: killed by SIGKILL\n"));
            }
            other => panic!("unexpected mapping: {other:?}"),
        }
    }

    /// ISSUE-066：非约定退出码保留 exit_code；空 stderr 不输出载荷（None）。
    #[test]
    fn crash_exit_records_exit_code_and_omits_empty_stderr() {
        let mapped = map_failure(WorkerFailure::Exit {
            code: Some(7),
            signal: None,
            stderr: "  \n".to_string(),
        });
        assert_eq!(mapped.code(), "NODE_WORKER_CRASH");
        match mapped {
            NodeError::NodeWorker {
                kind: WorkerErrorKind::Crash,
                signal,
                exit_code,
                stderr_tail,
            } => {
                assert_eq!(signal, None);
                assert_eq!(exit_code, Some(7));
                assert_eq!(stderr_tail, None);
            }
            other => panic!("unexpected mapping: {other:?}"),
        }
    }

    /// stderr 尾部上限 1 KiB，且不能把多字节 UTF-8 字符截断成非法字节序列。
    #[test]
    fn stderr_tail_is_truncated_on_char_boundary() {
        let stderr = format!("a{}", "é".repeat(STDERR_TAIL_BYTES));
        assert!(stderr.len() > STDERR_TAIL_BYTES);
        let tail = stderr_tail(&stderr).expect("non-empty stderr");
        assert!(
            tail.len() <= STDERR_TAIL_BYTES,
            "tail must fit the byte cap, got {}",
            tail.len()
        );
        assert!(tail.chars().all(|ch| ch == 'é'), "tail: {tail:?}");
        assert!(stderr.ends_with(&tail));
    }

    #[test]
    fn crash_exit_without_signal_maps_to_crash_kind() {
        assert_eq!(
            map_failure(WorkerFailure::Exit {
                code: None,
                signal: None,
                stderr: String::new()
            }),
            node_worker_error(WorkerErrorKind::Crash)
        );
    }

    #[test]
    fn spawn_failure_maps_to_worker_crash() {
        // spec 03 §3.9.4：spawn 失败 → NODE_WORKER_CRASH（不是 NODE_CONFIG_ERROR）。
        let mapped = map_failure(WorkerFailure::Spawn("no python".to_string()));
        assert_eq!(mapped, node_worker_error(WorkerErrorKind::Crash));
        assert_eq!(mapped.code(), "NODE_WORKER_CRASH");
    }

    #[test]
    fn worker_self_reported_timeout_is_a_protocol_violation() {
        // 04 §4.1.4：Timeout 只允许内核/AgentCallHandler 产生；worker 自报按协议错误处理。
        assert_eq!(
            map_worker_error(&worker_error(CODE_TIMEOUT)),
            node_worker_error(WorkerErrorKind::Protocol)
        );
    }

    #[test]
    fn return_not_dict_maps_to_business_error_with_original_code() {
        assert_eq!(
            map_worker_error(&worker_error(crate::protocol::CODE_RETURN_NOT_DICT)),
            NodeError::BusinessError {
                code: crate::protocol::CODE_RETURN_NOT_DICT.to_string(),
                message: "boom".to_string(),
            }
        );
    }

    /// 内核只把 `python_node` 交给执行器；其它 handler 类型属框架不变量违反。
    #[test]
    fn python_handler_rejects_non_python_node() {
        let text = serde_json::json!({
            "version": "1.0",
            "metadata": {"name": "t"},
            "entry_points": ["start"],
            "nodes": [
                {"id": "start", "type": "start", "inputs": [], "config": {}},
                {"id": "work", "type": "python_node",
                 "inputs": [{"name": "x", "type": "any", "required": true}],
                 "config": {"module": "m", "function": "f"}},
                {"id": "done", "type": "end",
                 "inputs": [{"name": "r", "type": "any", "required": true}], "config": {}}
            ],
            "edges": [
                {"from": "start", "to_node": "work", "to_input_port": "x"},
                {"from": "work", "to_node": "done", "to_input_port": "r"}
            ]
        })
        .to_string();
        let loaded = zgraph_dsl::load_graph(text.as_bytes()).expect("valid graph");
        let graph = zgraph_kernel::CompiledGraph::compile(&loaded).expect("compile");
        let start = graph.nodes.get("start").expect("start node");
        match python_handler(start) {
            Err(NodeError::Internal { message }) => {
                assert!(message.contains("python_node"), "message: {message}");
            }
            other => panic!("non-python node must be rejected, got {other:?}"),
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

    /// ISSUE-079：共享上限句柄的默认容量 = CPU×2 / 每 run 8；0 提升为 1。
    #[test]
    fn worker_limits_defaults_match_architecture() {
        let limits = WorkerLimits::default_limits();
        assert_eq!(limits.max_workers(), default_max_workers());
        assert_eq!(limits.max_workers_per_run(), DEFAULT_MAX_WORKERS_PER_RUN);
        let zero = WorkerLimits::new(0, 0);
        assert_eq!(zero.max_workers(), 1);
        assert_eq!(zero.max_workers_per_run(), 1);
    }

    /// ISSUE-079：CLI 注入的同一实例既被执行器持有、也可被外部消费者获取许可。
    #[tokio::test]
    async fn shared_limits_are_one_instance_and_enforce_global_cap() {
        let limits = Arc::new(WorkerLimits::new(1, 1));
        let executor = WorkerExecutor::with_shared_limits(
            SupervisorConfig::from_worker(crate::spawn::WorkerConfig::default()),
            limits.clone(),
        );
        assert!(Arc::ptr_eq(&executor.limits(), &limits));
        assert_eq!(executor.max_workers(), 1);
        assert_eq!(executor.max_workers_per_run(), 1);

        let held = limits.acquire_global().await.expect("global permit");
        let blocked =
            tokio::time::timeout(Duration::from_millis(50), limits.acquire_global()).await;
        assert!(
            blocked.is_err(),
            "global cap must block the second consumer"
        );
        drop(held);
        assert!(limits.acquire_global().await.is_some());
    }

    /// ISSUE-079：每 run 上限按 run 隔离；同一 run 超限阻塞，不同 run 不受影响。
    #[tokio::test]
    async fn shared_limits_isolate_per_run_caps() {
        let limits = WorkerLimits::new(8, 1);
        let run_a = limits.acquire_run("run-a").await.expect("run-a permit");
        let blocked =
            tokio::time::timeout(Duration::from_millis(50), limits.acquire_run("run-a")).await;
        assert!(blocked.is_err(), "same-run cap must block");
        let run_b = limits.acquire_run("run-b").await.expect("run-b permit");
        drop(run_a);
        drop(run_b);
        assert!(limits.acquire_run("run-a").await.is_some());
    }

    /// ISSUE-082：握手 hello 按 run 聚合、去重并按 `(python, sdk)` 升序输出。
    #[test]
    fn worker_versions_aggregate_dedup_and_sort_per_run() {
        let executor = WorkerExecutor::new(SupervisorConfig::default());
        let hello = |python: &str, sdk: &str| Hello {
            protocol_version: PROTOCOL_VERSION,
            python_version: python.to_string(),
            sdk_version: sdk.to_string(),
            worker_id: "w1".to_string(),
        };
        executor.record_hello("run-a", &hello("3.11.9", "0.1.0"));
        executor.record_hello("run-a", &hello("3.11.9", "0.1.0"));
        executor.record_hello("run-a", &hello("3.10.13", "0.1.0"));
        executor.record_hello("run-b", &hello("3.12.1", "0.2.0"));
        assert_eq!(
            executor.worker_versions("run-a"),
            Some(vec![
                serde_json::json!({"python_version": "3.10.13", "sdk_version": "0.1.0"}),
                serde_json::json!({"python_version": "3.11.9", "sdk_version": "0.1.0"}),
            ])
        );
        assert_eq!(
            executor.worker_versions("run-b"),
            Some(vec![
                serde_json::json!({"python_version": "3.12.1", "sdk_version": "0.2.0"})
            ])
        );
        assert_eq!(executor.worker_versions("run-c"), None);
    }

    /// ISSUE-079：每 run 峰值上限 = 8（默认）且 run 间隔离。
    #[tokio::test]
    async fn per_run_cap_is_eight_and_isolated() {
        let limits = WorkerLimits::new(16, DEFAULT_MAX_WORKERS_PER_RUN);
        let mut held = Vec::new();
        for _ in 0..DEFAULT_MAX_WORKERS_PER_RUN {
            held.push(limits.acquire_run("run-a").await.expect("run-a permit"));
        }
        let blocked =
            tokio::time::timeout(Duration::from_millis(50), limits.acquire_run("run-a")).await;
        assert!(blocked.is_err(), "run-a must be capped at 8");
        let other = limits.acquire_run("run-b").await.expect("run-b unaffected");
        drop(other);
        drop(held);
    }
}
