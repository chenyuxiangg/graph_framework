//! worker 子进程生命周期：M0 兼容 API + M3 supervisor 委托入口。
//!
//! M0 冻结的行为约定（`run_worker`/`sample_request`/`WorkerConfig`）保持不变；
//! 进程组、取消升级、帧空闲超时、流式上限等 M3 能力在 [`crate::supervisor`] 实现，
//! 本模块只负责把它们收敛成稳定的调用面。协议见 `schemas/worker-protocol-v1.md`。

use std::path::PathBuf;
use std::time::Duration;

use crate::protocol::{
    Frame, ProtocolError, RunRequest, WorkerError, CODE_HANDSHAKE, CODE_PROTOCOL,
};
use crate::supervisor::{no_cancel, CancelSignal, FrameSink, StreamingStats};

/// worker 启动配置（M0 冻结字段；M3 参数见 [`crate::supervisor::SupervisorConfig`]）。
#[derive(Debug, Clone)]
pub struct WorkerConfig {
    /// Python 可执行文件（如 `python3` 或 venv 路径）。
    pub program: String,
    /// 进程参数（如 `-m zgraph_worker.runtime`）。
    pub args: Vec<String>,
    /// 追加环境变量（如 `PYTHONPATH`）。
    pub env: Vec<(String, String)>,
    /// 工作目录。
    pub cwd: Option<PathBuf>,
    /// 整次执行超时（含握手）。
    pub timeout: Duration,
    /// 握手超时。
    pub hello_timeout: Duration,
    /// result 后进程退出宽限。
    pub exit_grace: Duration,
    /// stderr 保留上限（字节，取尾部）。
    pub max_stderr_bytes: usize,
}

impl Default for WorkerConfig {
    fn default() -> Self {
        Self {
            program: "python3".to_string(),
            args: vec!["-m".to_string(), "zgraph_worker.runtime".to_string()],
            env: Vec::new(),
            cwd: None,
            timeout: Duration::from_secs(180),
            hello_timeout: Duration::from_secs(5),
            exit_grace: Duration::from_secs(1),
            max_stderr_bytes: 8 * 1024,
        }
    }
}

/// worker 调用失败。
#[derive(Debug, thiserror::Error)]
pub enum WorkerFailure {
    /// 进程无法启动。
    #[error("spawn worker failed: {0}")]
    Spawn(String),
    /// 协议错误（握手/帧/退出约定）。
    #[error(transparent)]
    Protocol(#[from] ProtocolError),
    /// worker 返回结构化错误。
    #[error("worker error {0}")]
    Worker(WorkerError),
    /// 超时（进程组已被 kill 并 reap）。
    #[error("worker timed out after {0:?}")]
    Timeout(Duration),
    /// 外部取消（进程组已按协议 §4.3 升级 kill 并 reap）。
    #[error("worker cancelled")]
    Cancelled,
    /// 非预期退出（被信号杀死或非约定退出码）。
    #[error("worker exited unexpectedly: code={code:?}, stderr={stderr}")]
    Exit {
        /// 退出码。
        code: Option<i32>,
        /// stderr 尾部。
        stderr: String,
    },
}

/// 成功执行结果。
#[derive(Debug, Clone)]
pub struct WorkerRun {
    /// 节点输出。
    pub output: serde_json::Value,
    /// 执行期间接纳的 emit 帧（流式事件）。
    pub events: Vec<Frame>,
    /// stderr 尾部（供 trace/排障）。
    pub stderr: String,
    /// emit 流式统计（含超限丢弃计数，M3）。
    pub streaming: StreamingStats,
}

pub(crate) fn protocol_failure(message: impl Into<String>) -> WorkerFailure {
    WorkerFailure::Protocol(ProtocolError {
        code: CODE_PROTOCOL,
        message: message.into(),
    })
}

pub(crate) fn handshake_failure(message: impl Into<String>) -> WorkerFailure {
    WorkerFailure::Protocol(ProtocolError {
        code: CODE_HANDSHAKE,
        message: message.into(),
    })
}

/// 启动 worker 并执行一次 `run`，直到 result/error/超时（M0 兼容入口）。
///
/// M3 起进程以独立进程组启动：超时/协议错误会对**进程组** kill（D1），孙进程不残留。
pub async fn run_worker(
    config: &WorkerConfig,
    request: &RunRequest,
) -> Result<WorkerRun, WorkerFailure> {
    let mut ignore = |_frame: &Frame| {};
    run_worker_controlled(config, request, &mut ignore, no_cancel()).await
}

/// 可外部取消、逐帧回调的 worker 调用（M3；`run_worker` 是它的无取消包装）。
///
/// - `on_frame`：每接纳一帧 emit 同步回调一次（用于即时 `ctx.emit`）。
/// - `cancel`：完成即请求取消，按协议 §4.3 走 `cancel` 帧 → SIGTERM → SIGKILL 进程组。
pub async fn run_worker_controlled(
    config: &WorkerConfig,
    request: &RunRequest,
    on_frame: FrameSink<'_>,
    cancel: CancelSignal,
) -> Result<WorkerRun, WorkerFailure> {
    crate::supervisor::run_supervised(
        &crate::supervisor::SupervisorConfig::from_worker(config.clone()),
        request,
        on_frame,
        cancel,
    )
    .await
}

/// 构造默认 request 的便捷函数（测试与 M0 示例用）。
pub fn sample_request(module: &str, function: &str, inputs: serde_json::Value) -> RunRequest {
    RunRequest {
        run_id: "run-1".to_string(),
        instance_id: "inst-1".to_string(),
        node_id: "node-1".to_string(),
        attempt: 0,
        module: module.to_string(),
        function: function.to_string(),
        inputs,
        params: serde_json::json!({}),
        user_id: "local".to_string(),
        global_vars: serde_json::json!({}),
        seed: 0,
        deadline_ms: 0,
        trace_id: "trace-1".to_string(),
    }
}

/// 供测试使用的常量：stderr 上限。
pub const DEFAULT_MAX_STDERR_BYTES: usize = 8 * 1024;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::{CODE_TIMEOUT, MAX_FRAME_BYTES};

    #[test]
    fn sample_request_has_expected_identity() {
        let request = sample_request("m", "f", serde_json::json!({"a": 1}));
        assert_eq!(request.module, "m");
        assert_eq!(request.seed, 0);
        assert_eq!(request.attempt, 0);
    }

    #[test]
    fn worker_limits_match_protocol_contract() {
        assert_eq!(MAX_FRAME_BYTES, 8 * 1024 * 1024);
        assert_eq!(DEFAULT_MAX_STDERR_BYTES, 8 * 1024);
        assert_eq!(CODE_TIMEOUT, "NODE_TIMEOUT");
    }

    #[test]
    fn cancellation_is_a_distinct_failure_from_timeout() {
        let cancelled = WorkerFailure::Cancelled;
        assert_eq!(cancelled.to_string(), "worker cancelled");
        assert!(!matches!(cancelled, WorkerFailure::Timeout(_)));
    }
}
