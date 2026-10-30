//! `zgraph-trace`：trace 持久层（M2 起；M6 拆回 06 §6.4.1 双通道）。
//!
//! 语义基准：`doc/spec-v1.2/06-存储与可观测.md`（DDL/seq/写路径/聚合/CLI 只读
//! 约定/fail-stop/恢复）与 `doc/architecture-v2.md` §5.2/§8（D1/D9 的 v2 增量）。
//!
//! # 架构
//!
//! ```text
//! 无损 send（cap 8192 / 阻塞）──────────┐
//! 流式 send_streaming（cap 1024 / 可丢）┴──> TraceHandle ──> 单写线程 ──> SQLite(WAL)
//!
//! `zgraph trace` ──> TraceReader（?:mode=ro，只读，不迁移/不标定）
//! ```
//!
//! - [`TraceWriter::open`] 同步完成打开/迁移/启动写线程；写线程独占唯一写连接。
//! - 写线程按 `BEGIN IMMEDIATE` 批量提交（≤256 行 events 或 100 ms）；
//!   `events.seq` 由 `AUTOINCREMENT` 在写线程内分配，明细表 `seq` 1:1 关联同事务信封。
//! - `Streaming` 在写线程按 06 §6.5 聚合：token/reasoning 各自缓冲，100 ms 窗口或
//!   ≥4096 字符提前分段；tool_call/progress 立即成段并先 flush 同实例缓冲。
//! - 崩溃标定（06 §6.8）是两段式原语：`TraceWriter::interrupted_candidates` 查
//!   `Running`/`Paused` 候选，`TraceWriter::mark_interrupted` 单事务标定为
//!   `Failed + interrupted=1`；判活（`kill(pid, 0)`/heartbeat 陈旧）由调用方负责。
//! - 写失败（busy 重试 3×50 ms 后 / IO）通过 [`TraceHandle::take_error`] 与
//!   [`TraceWriter::flush`]/[`TraceWriter::close`] 的错误暴露；fail-stop 的 run
//!   生命周期接线由 M3/M5 负责（本里程碑记录 ISSUE）。
//!
//! # 通道拓扑（06 §6.4.1）
//!
//! 双通道有界队列，均由同一写线程消费：
//! - **无损通道** `TRACE_CHANNEL_CAP = 8192`：[`TraceHandle::send`] 阻塞发送，满载时
//!   生产者等待（背压）；不可丢事件走这里。
//! - **可丢流式通道** `STREAMING_CHANNEL_CAP = 1024`：[`TraceHandle::send_streaming`]
//!   用 `try_send`，满/断开即丢弃并累计 [`TraceHandle::streaming_dropped`]
//!   （`RunMetrics.streaming_dropped`）。
//!
//! 写线程按**发送序号**合并两条通道（先入队的先处理），因此同一发送端先发出的流式
//! 事件不会被其后的无损事件越过；`TraceWriter::flush`/`close` 的屏障同时覆盖两条
//! 通道（关停不丢已入队的流式事件）。
//!
//! # 未覆盖（留待后续里程碑）
//!
//! - 06 §6.8.1 的**判活**（`kill(pid, 0)`/`RECOVERY_HEARTBEAT_STALE_MS`）与
//!   `zgraph run` 启动接线不在本 crate：trace 只提供候选查询与标定两段原语，
//!   避免引入 OS/进程依赖（CLI 消费见 REV-030）。本 crate 不实现断点续跑（§6.8.4）。
//! - `worker_versions_json`/`model_params_json` 由 M4 Harness 写入；v2 列已建好但
//!   [`RunStartedInfo`] 暂不携带（保持冻结签名）。
//!
//! # 快速开始
//!
//! ```
//! use zgraph_trace::{RunMetrics, RunStartedInfo, RunStatus, TraceWriter};
//!
//! let dir = tempfile::tempdir().expect("tempdir");
//! let writer = TraceWriter::open(dir.path().join("trace.db")).expect("open trace db");
//! let handle = writer.handle();
//! handle.send(zgraph_trace::TraceInput::RunStarted(RunStartedInfo {
//!     run_id: "run-1".to_string(),
//!     graph_hash: "0123456789abcdef".to_string(),
//!     graph_name: "demo".to_string(),
//!     dsl_json: "{}".to_string(),
//!     user_id: "local".to_string(),
//!     entry_inputs: serde_json::json!({}),
//!     seed: 0,
//!     record_mode: zgraph_trace::RecordMode::Hash,
//!     owner_pid: std::process::id(),
//!     owner_host: "localhost".to_string(),
//!     started_at: "2026-10-28T09:12:03.104Z".to_string(),
//! }));
//! handle.send(zgraph_trace::TraceInput::RunFinished(zgraph_trace::RunFinishedInfo {
//!     run_id: "run-1".to_string(),
//!     status: RunStatus::Completed,
//!     output_summary: None,
//!     error: None,
//!     metrics: RunMetrics::default(),
//!     at: "2026-10-28T09:12:04.946Z".to_string(),
//! }));
//! writer.flush().expect("flush");
//! let reader = zgraph_trace::TraceReader::open_readonly(dir.path().join("trace.db")).expect("reader");
//! assert_eq!(reader.run("run-1").expect("query").map(|run| run.status), Some(RunStatus::Completed));
//! writer.close().expect("close");
//! ```

#![forbid(unsafe_code)]

mod error;
mod model;
mod reader;
mod recovery;
mod schema;
mod writer;

pub use error::TraceError;
pub use model::{
    GateResultRecord, InstanceSnapshot, InstanceStatus, LlmCallRecord, RecordMode, RecordedOutput,
    RunFinishedInfo, RunMetrics, RunStartedInfo, RunStatus, StreamingEvent, TraceFailure,
    TraceInput, TransitionRecord,
};
pub use reader::{
    EventQuery, EventRow, GateResultRow, GraphRow, InstanceRow, LlmCallRow, RecordedOutputRow,
    RunRow, RunStats, StreamingRow, TraceReader, TransitionRow,
};
pub use recovery::{InterruptedCandidate, RecoveredRun, INTERRUPTED_ERROR_MESSAGE};
pub use schema::{
    DB_BUSY_TIMEOUT_MS, DB_WRITE_RETRY, DB_WRITE_RETRY_INTERVAL_MS, HEARTBEAT_INTERVAL_MS,
    STREAMING_CHANNEL_CAP, STREAM_SEGMENT_MAX_BYTES, STREAM_SEGMENT_MAX_CHARS,
    STREAM_TOKEN_AGGREGATE_MS, TRACE_BATCH_INTERVAL_MS, TRACE_BATCH_ROWS, TRACE_CHANNEL_CAP,
    TRACE_PAYLOAD_MAX_BYTES, TRACE_SCHEMA_VERSION, WAL_CHECKPOINT_INTERVAL_MS,
};
pub use writer::{TraceHandle, TraceWriter};
