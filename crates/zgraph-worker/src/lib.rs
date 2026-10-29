//! worker 协议 v1 客户端与子进程生命周期（M0 骨架）。
//!
//! - 协议类型与帧编解码：[`protocol`]
//! - 子进程 spawn / 握手 / run / 超时：[`spawn`]
//!
//! 契约：`schemas/worker-protocol-v1.md`；设计：`doc/architecture-v2.md` §3。

#![forbid(unsafe_code)]

pub mod protocol;
pub mod spawn;

pub use protocol::{
    Frame, Hello, ProtocolError, RunRequest, WorkerError, WorkerOutcome, MAX_FRAME_BYTES,
    PROTOCOL_VERSION,
};
pub use spawn::{run_worker, sample_request, WorkerConfig, WorkerFailure, WorkerRun};
