//! worker 协议 v1 客户端、子进程生命周期与内核执行器（M3）。
//!
//! - 协议类型与帧编解码：[`protocol`]
//! - M0 兼容调用面（`run_worker`/`WorkerConfig`）：[`spawn`]
//! - supervisor（进程组、取消升级、帧超时、流式上限）：[`supervisor`]
//! - `NodeExecutor` 实现（错误映射、并发上限、emit 流式）：[`executor`]
//!
//! 契约：`schemas/worker-protocol-v1.md`；设计：`doc/architecture-v2.md` §3/§4（D1/D2/D4/D10）。

#![forbid(unsafe_code)]

pub mod executor;
pub mod protocol;
pub mod spawn;
pub mod supervisor;

pub use executor::{
    default_max_workers, map_failure, map_worker_error, streaming_event, WorkerExecutor,
    WorkerLimits, DEFAULT_MAX_WORKERS_PER_RUN, STDERR_TAIL_BYTES,
};
pub use protocol::{
    Frame, Hello, ProtocolError, RunRequest, WorkerError, WorkerOutcome, CODE_BUSINESS_ERROR,
    CODE_CANCELLED, CODE_CRASH, CODE_HANDSHAKE, CODE_IMPORT, CODE_PROTOCOL, CODE_PYTHON_EXCEPTION,
    CODE_RETURN_NOT_DICT, CODE_TIMEOUT, MAX_FRAME_BYTES, PROTOCOL_VERSION,
};
pub use spawn::{
    run_worker, run_worker_controlled, sample_request, WorkerConfig, WorkerFailure, WorkerRun,
};
pub use supervisor::{
    no_cancel, run_supervised, run_supervised_observed, CancelSignal, FrameSink, StreamingLimits,
    StreamingStats, SupervisorConfig, DEFAULT_CANCEL_GRACE, DEFAULT_FRAME_TIMEOUT,
    DEFAULT_MAX_EMIT_BYTES, DEFAULT_MAX_EMIT_EVENTS, DEFAULT_TERM_GRACE,
};
