//! 单写线程、批量事务与流式聚合（M2 起；M6 拆回双通道）。
//!
//! 公开面只有 [`TraceWriter`] 与 [`TraceHandle`]；写连接只属于写线程。
//!
//! 通道拓扑与 06 §6.4.1 一致（**双通道**）：
//! - 无损通道 `sync_channel(TRACE_CHANNEL_CAP = 8192)`：[`TraceHandle::send`] 阻塞发送，
//!   满载时生产者等待（背压）；
//! - 流式通道 `sync_channel(STREAMING_CHANNEL_CAP = 1024)`：[`TraceHandle::send_streaming`]
//!   用 `try_send`，满/断开即丢弃并累计 `streaming_dropped`（I6 可丢语义）。
//!
//! 两条通道由同一写线程消费：每条数据消息携带**发送序号**，写线程按序号合并两条
//! 通道（先入队的先处理），因此同一发送端先发出的流式事件不会被其后的无损事件越过
//! （见 `thread` 模块）。

mod recovery;
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
use crate::recovery::{InterruptedCandidate, RecoveredRun};
use crate::schema::{self, STREAMING_CHANNEL_CAP, TRACE_CHANNEL_CAP};

/// 无损通道消息：数据与控制消息共用同一 FIFO（保证 barrier 语义）。
///
/// `Input` 携带发送序号，供写线程与流式通道按到达顺序合并消费。
pub(crate) enum WriterMsg {
    Input {
        seq: u64,
        input: Box<TraceInput>,
    },
    Flush(SyncSender<Result<(), TraceError>>),
    Shutdown(SyncSender<Result<(), TraceError>>),
    /// 中止标定候选查询（SELECT，走写线程自己的连接）。
    InterruptedCandidates(SyncSender<Result<Vec<InterruptedCandidate>, TraceError>>),
    /// 中止标定（单事务 UPDATE）；装箱以保持 `WriterMsg` 尺寸与通道开销不变。
    MarkInterrupted(Box<MarkInterruptedRequest>),
}

/// 可丢流式通道消息：携带发送序号，写线程据此与无损通道合并排序。
pub(crate) struct StreamingMsg {
    pub(crate) seq: u64,
    pub(crate) input: TraceInput,
}

/// `WriterMsg::MarkInterrupted` 的载荷（多个 run id 共用同一标定时间）。
pub(crate) struct MarkInterruptedRequest {
    pub(crate) run_ids: Vec<String>,
    pub(crate) now: String,
    pub(crate) reply: SyncSender<Result<Vec<RecoveredRun>, TraceError>>,
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
        let (streaming_sender, streaming_receiver) = mpsc::sync_channel(STREAMING_CHANNEL_CAP);
        let next_seq = Arc::new(AtomicU64::new(0));
        let error = Arc::new(Mutex::new(None));
        let thread_error = Arc::clone(&error);
        let join = std::thread::Builder::new()
            .name("zgraph-trace-writer".to_string())
            .spawn(move || thread::run(conn, receiver, streaming_receiver, thread_error))
            .map_err(|err| {
                TraceError::Storage(format!("failed to spawn trace writer thread: {err}"))
            })?;
        Ok(TraceWriter {
            handle: TraceHandle {
                sender,
                streaming: streaming_sender,
                next_seq,
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

    /// 查询中止标定候选（06 §6.8.1 步骤 1）：`status IN ('Running','Paused')` 的 run。
    ///
    /// 经写线程执行（同一连接、同一 FIFO）：调用前已入队的写入会先提交，因此结果
    /// 与 [`TraceWriter::flush`] 后的只读查询一致。**判活不在本 crate 内做**——
    /// `kill(pid, 0)`/heartbeat 陈旧判断由调用方按 06 §6.8.1 步骤 2 执行。
    pub fn interrupted_candidates(&self) -> Result<Vec<InterruptedCandidate>, TraceError> {
        self.handle.control(WriterMsg::InterruptedCandidates)
    }

    /// 把判死的 run 标定为 `Failed + interrupted=1`（06 §6.8.1 步骤 3）。
    ///
    /// 单事务逐 run 执行
    /// `UPDATE runs SET status='Failed', interrupted=1, recovered_at=?, error_message=?`
    /// `WHERE run_id=? AND status IN ('Running','Paused')`，**仅返回实际更新的行**；
    /// 已终态/不存在的 run 被跳过。**`error_code` 保持 NULL**，不修改 `instances`，
    /// 不写 `RunFinished`/`Transition`（06 §6.8.2）。`now` 由调用方提供
    /// （RFC3339 毫秒 Z）；空 `run_ids` 直接返回空且不排队。
    pub fn mark_interrupted(
        &self,
        run_ids: &[String],
        now: &str,
    ) -> Result<Vec<RecoveredRun>, TraceError> {
        if run_ids.is_empty() {
            return Ok(Vec::new());
        }
        self.handle.control(|reply| {
            WriterMsg::MarkInterrupted(Box::new(MarkInterruptedRequest {
                run_ids: run_ids.to_vec(),
                now: now.to_string(),
                reply,
            }))
        })
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
/// 顺序保证：同一发送端上的 `send`/`send_streaming` 按调用顺序取号，写线程按发送序号
/// 合并两条通道，因此同一发送端先发出的流式事件不会被其后的无损事件越过（跨发送端
/// 不保证全局顺序，由上层单写者模型保证）。
#[derive(Clone)]
pub struct TraceHandle {
    /// 无损通道发送端（[`TraceHandle::send`]，阻塞）。
    sender: SyncSender<WriterMsg>,
    /// 可丢流式通道发送端（[`TraceHandle::send_streaming`]，`try_send`）。
    streaming: SyncSender<StreamingMsg>,
    /// 跨两条通道共享的发送序号（写线程据此合并排序）。
    next_seq: Arc<AtomicU64>,
    dropped: Arc<AtomicU64>,
    error: Arc<Mutex<Option<String>>>,
}

impl TraceHandle {
    /// 无损发送（阻塞；通道 cap=`TRACE_CHANNEL_CAP`）。通道已断开时记录错误并丢弃该输入。
    ///
    /// 也可用于发送 `TraceInput::Streaming`：走无损通道则不可丢，且与其余事件同序
    /// （等价于 M2 单通道行为，供需要严格因果顺序的调用方使用）。
    pub fn send(&self, input: TraceInput) {
        let seq = self.take_seq();
        let message = WriterMsg::Input {
            seq,
            input: Box::new(input),
        };
        if self.sender.send(message).is_err() {
            self.record_error("trace writer channel disconnected (send dropped)".to_string());
        }
    }

    /// 可丢发送（`try_send` 到 cap=`STREAMING_CHANNEL_CAP` 的流式通道）：通道满/断开时
    /// 计数并返回 `false`。
    ///
    /// 仅用于 `TraceInput::Streaming`；非流式事件必须用 [`TraceHandle::send`]。
    /// 流式事件**可能**晚于其后的无损事件落库（可丢通道语义，06 §6.4.1）；同一发送端
    /// 先流式后无损的顺序由发送序号保证。调用方 MUST 保证同一 run 的 `RunStarted`
    /// 先于其流式事件发送（内核 `Emitter` 已保证），否则 `events.run_id` 外键会拒绝。
    pub fn send_streaming(&self, input: TraceInput) -> bool {
        let message = StreamingMsg {
            seq: self.take_seq(),
            input,
        };
        match self.streaming.try_send(message) {
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

    /// 控制消息的通用请求/应答：与数据消息共用同一 FIFO（barrier 语义）。
    fn control<T>(
        &self,
        make: impl FnOnce(SyncSender<Result<T, TraceError>>) -> WriterMsg,
    ) -> Result<T, TraceError> {
        let (reply, receiver) = mpsc::sync_channel(0);
        self.sender
            .send(make(reply))
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

    /// 取下一个发送序号（两条通道共享；`Relaxed` 足够：排序只需单发送端单调）。
    fn take_seq(&self) -> u64 {
        self.next_seq.fetch_add(1, Ordering::Relaxed)
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

#[cfg(test)]
mod tests {
    //! 双通道容量/丢弃/背压语义（不启动写线程与数据库，直接检查通道）。

    use std::sync::mpsc::{self, Receiver};
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use super::*;
    use crate::model::StreamingEvent;

    fn streaming_input(run_id: &str) -> TraceInput {
        TraceInput::Streaming {
            run_id: run_id.to_string(),
            instance_id: "inst".to_string(),
            node_id: None,
            event: StreamingEvent::TokenDelta {
                text: "x".to_string(),
            },
            at: "2026-10-30T00:00:00.000Z".to_string(),
        }
    }

    /// 构造不与写线程连接的句柄：保留两条通道的接收端以便观察容量。
    fn detached_handle() -> (TraceHandle, Receiver<WriterMsg>, Receiver<StreamingMsg>) {
        let (sender, lossless_rx) = mpsc::sync_channel(TRACE_CHANNEL_CAP);
        let (streaming, streaming_rx) = mpsc::sync_channel(STREAMING_CHANNEL_CAP);
        let handle = TraceHandle {
            sender,
            streaming,
            next_seq: Arc::new(AtomicU64::new(0)),
            dropped: Arc::new(AtomicU64::new(0)),
            error: Arc::new(Mutex::new(None)),
        };
        (handle, lossless_rx, streaming_rx)
    }

    #[test]
    fn streaming_channel_capacity_is_1024_and_drops_are_counted() {
        let (handle, _lossless_rx, _streaming_rx) = detached_handle();
        for _ in 0..STREAMING_CHANNEL_CAP {
            assert!(
                handle.send_streaming(streaming_input("run-cap")),
                "容量内必须全部接收"
            );
        }
        assert_eq!(handle.streaming_dropped(), 0);
        for expected in 1..=3_u64 {
            assert!(!handle.send_streaming(streaming_input("run-cap")));
            assert_eq!(handle.streaming_dropped(), expected);
        }
    }

    #[test]
    fn lossless_send_blocks_when_channel_full() {
        let (handle, lossless_rx, _streaming_rx) = detached_handle();
        for _ in 0..TRACE_CHANNEL_CAP {
            handle.send(streaming_input("run-bp"));
        }
        let (done, done_rx) = mpsc::channel();
        let blocking = handle.clone();
        let sender_thread = std::thread::spawn(move || {
            blocking.send(streaming_input("run-bp"));
            let _ = done.send(());
        });
        assert!(
            done_rx.recv_timeout(Duration::from_millis(200)).is_err(),
            "无损通道满时 send 必须阻塞（背压）"
        );
        lossless_rx.recv().expect("drain one to make room");
        done_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("腾出容量后 send 应解除阻塞");
        sender_thread.join().expect("sender thread");
    }

    #[test]
    fn disconnected_channels_record_error_and_count_streaming_drops() {
        let (handle, lossless_rx, streaming_rx) = detached_handle();
        drop(lossless_rx);
        drop(streaming_rx);

        handle.send(streaming_input("run-closed"));
        assert!(handle.take_error().is_some(), "无损通道断开应记录错误");
        assert!(!handle.send_streaming(streaming_input("run-closed")));
        assert_eq!(handle.streaming_dropped(), 1);
    }
}
