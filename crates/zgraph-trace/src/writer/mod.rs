//! 单写线程、批量事务与流式聚合（M2）。
//!
//! 公开面只有 [`TraceWriter`] 与 [`TraceHandle`]；写连接只属于写线程。
//!
//! 通道为 M2 单通道简化（06 §6.4.1 双通道的等价语义）：`sync_channel(8192)` 一条，
//! 无损事件用阻塞 `send`，流式事件用 `try_send`（满则丢弃并计数）。

mod stream;
mod thread;
mod tx;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, SyncSender, TrySendError};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use crate::error::TraceError;
use crate::model::TraceInput;
use crate::schema::{self, TRACE_CHANNEL_CAP};

/// 写线程消息：数据与两类控制消息共用同一 FIFO（保证 barrier 语义）。
pub(crate) enum WriterMsg {
    Input(Box<TraceInput>),
    Flush(SyncSender<Result<(), TraceError>>),
    Shutdown(SyncSender<Result<(), TraceError>>),
}

/// trace 写端：持有写线程与写通道发送端。
///
/// `open` 同步完成建库/迁移/启动写线程；调用方通过 [`TraceWriter::handle`] 把
/// [`TraceHandle`] 克隆分发给内核/cli 适配层。关闭时调用 [`TraceWriter::close`]
/// 以 flush 剩余缓冲并 `wal_checkpoint(TRUNCATE)`。
pub struct TraceWriter {
    handle: TraceHandle,
    join: Option<JoinHandle<()>>,
    db_path: PathBuf,
}

impl TraceWriter {
    /// 打开（必要时创建）数据库、完成 schema 迁移并启动写线程。
    ///
    /// 错误：schema 版本超前 → [`TraceError::SchemaTooNew`]；IO/迁移失败 →
    /// [`TraceError::Storage`]。
    pub fn open(path: impl AsRef<Path>) -> Result<TraceWriter, TraceError> {
        let db_path = path.as_ref().to_path_buf();
        let conn = schema::open_writer_connection(&db_path)?;
        let (sender, receiver) = mpsc::sync_channel(TRACE_CHANNEL_CAP);
        let error = Arc::new(Mutex::new(None));
        let thread_error = Arc::clone(&error);
        let join = std::thread::Builder::new()
            .name("zgraph-trace-writer".to_string())
            .spawn(move || thread::run(conn, receiver, thread_error))
            .map_err(|err| {
                TraceError::Storage(format!("failed to spawn trace writer thread: {err}"))
            })?;
        Ok(TraceWriter {
            handle: TraceHandle {
                sender,
                dropped: Arc::new(AtomicU64::new(0)),
                error,
            },
            join: Some(join),
            db_path,
        })
    }

    /// 克隆一个可跨线程/任务发送事件的句柄。
    pub fn handle(&self) -> TraceHandle {
        self.handle.clone()
    }

    /// 屏障：保证此前发送的写入（含流式缓冲）已提交并对其他连接可见。
    pub fn flush(&self) -> Result<(), TraceError> {
        self.handle.flush()
    }

    /// flush + 关闭通道 + join 写线程 + `wal_checkpoint(TRUNCATE)`。
    ///
    /// 若写线程已记录不可恢复错误，返回该错误的文本视图（[`TraceError::Storage`]）。
    pub fn close(mut self) -> Result<(), TraceError> {
        let flush_result = self.handle.flush();
        let shutdown_result = self.handle.shutdown();
        if let Some(join) = self.join.take() {
            join.join()
                .map_err(|_| TraceError::Storage("trace writer thread panicked".to_string()))?;
        }
        flush_result?;
        shutdown_result
    }

    /// 数据库路径（错误信息/CLI 展示用）。
    pub fn db_path(&self) -> &Path {
        &self.db_path
    }
}

/// trace 写句柄（`Clone + Send + Sync`）：可被内核事件循环/多个任务共享。
///
/// 顺序保证：同一发送端上的 `send` 按调用顺序进入通道；多个发送端之间不保证全局
/// 顺序（M2 的单写者模型由上层保证）。
#[derive(Clone)]
pub struct TraceHandle {
    sender: SyncSender<WriterMsg>,
    dropped: Arc<AtomicU64>,
    error: Arc<Mutex<Option<String>>>,
}

impl TraceHandle {
    /// 无损发送（阻塞；通道 cap=8192）。通道已断开时记录错误并丢弃该输入。
    pub fn send(&self, input: TraceInput) {
        if self.sender.send(WriterMsg::Input(Box::new(input))).is_err() {
            self.record_error("trace writer channel disconnected (send dropped)".to_string());
        }
    }

    /// 可丢发送（`try_send`）：通道满/断开时计数并返回 `false`。
    ///
    /// 仅用于 `TraceInput::Streaming`；非流式事件必须用 [`TraceHandle::send`]。
    pub fn send_streaming(&self, input: TraceInput) -> bool {
        match self.sender.try_send(WriterMsg::Input(Box::new(input))) {
            Ok(()) => true,
            Err(TrySendError::Full(_)) => {
                self.dropped.fetch_add(1, Ordering::Relaxed);
                false
            }
            Err(TrySendError::Disconnected(_)) => {
                self.dropped.fetch_add(1, Ordering::Relaxed);
                self.record_error(
                    "trace writer channel disconnected (streaming dropped)".to_string(),
                );
                false
            }
        }
    }

    /// 累计丢弃的流式事件数（对应 `RunMetrics.streaming_dropped`）。
    pub fn streaming_dropped(&self) -> u64 {
        self.dropped.load(Ordering::Relaxed)
    }

    /// 取走写线程最近一次不可恢复错误文本（无错误返回 `None`）。
    pub fn take_error(&self) -> Option<String> {
        match self.error.lock() {
            Ok(mut slot) => slot.take(),
            Err(poisoned) => poisoned.into_inner().take(),
        }
    }

    fn flush(&self) -> Result<(), TraceError> {
        let (reply, receiver) = mpsc::sync_channel(0);
        self.sender
            .send(WriterMsg::Flush(reply))
            .map_err(|_| channel_closed())?;
        receiver.recv().map_err(|_| channel_closed())?
    }

    fn shutdown(&self) -> Result<(), TraceError> {
        let (reply, receiver) = mpsc::sync_channel(0);
        self.sender
            .send(WriterMsg::Shutdown(reply))
            .map_err(|_| channel_closed())?;
        receiver.recv().map_err(|_| channel_closed())?
    }

    fn record_error(&self, message: String) {
        if let Ok(mut slot) = self.error.lock() {
            if slot.is_none() {
                *slot = Some(message);
            }
        }
    }
}

fn channel_closed() -> TraceError {
    TraceError::Storage("trace writer thread is closed".to_string())
}
