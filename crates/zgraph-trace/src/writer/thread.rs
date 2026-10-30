//! 单写线程的事件循环与批处理（06 §6.4.1/§6.4.3）。
//!
//! 语义（相对 06 伪代码的实现）：
//! - **双通道按序号合并**：每条数据消息携带发送序号，写线程比较两条通道的队头，
//!   始终先处理序号更小者——同一发送端先发出的流式事件不会被其后的无损事件越过；
//! - 无损通道用短切片 `recv_timeout`（std `mpsc` 无法同时等待两条通道），保证写线程
//!   不会长时间空等无损通道而让流式通道积压被误丢；
//! - 凑满 [`TRACE_BATCH_ROWS`] 或到达 100 ms 截止即提交；提交前 flush 聚合缓冲
//!   （100 ms 聚合窗口 = 批量窗口）；
//! - `Flush`/`Shutdown`/恢复类控制消息到达时先排空流式通道中全部已入队事件再提交：
//!   barrier 覆盖两条通道，关停不丢已入队事件；
//! - 每次提交后更新 `runs.last_seq`/`event_count`；每 5 s 刷新在飞 run 的
//!   `heartbeat_at`；每 5 s 尝试 `wal_checkpoint(PASSIVE)`；
//! - 不可恢复错误记录一次文本（[`TraceHandle::take_error`]），之后丢弃新输入直到
//!   关闭，避免阻塞生产者（fail-stop 由上层 M3/M5 接线）。

use std::collections::HashSet;
use std::sync::mpsc::{Receiver, RecvTimeoutError, SyncSender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use rusqlite::Connection;

use crate::error::TraceError;
use crate::model::TraceInput;
use crate::recovery::{InterruptedCandidate, RecoveredRun};
use crate::schema::{
    db_err, HEARTBEAT_INTERVAL_MS, TRACE_BATCH_INTERVAL_MS, TRACE_BATCH_ROWS,
    WAL_CHECKPOINT_INTERVAL_MS,
};
use crate::writer::recovery;
use crate::writer::stream::Aggregator;
use crate::writer::tx::{self, Batch, TxContext};
use crate::writer::{StreamingMsg, WriterMsg};

/// 写线程空等无损通道的最长切片（毫秒）。
///
/// std `mpsc` 无法像 `select!` 那样同时等待两条通道：用短切片轮询流式通道，避免
/// 高流式负载下写线程在无损通道上空等 100 ms 而导致 1024 容量的流式通道被误丢。
const STREAMING_POLL_INTERVAL_MS: u64 = 10;

/// 写线程入口：消费两条通道直到 `Shutdown` 或无损通道发送端全部断开。
pub(crate) fn run(
    conn: Connection,
    lossless: Receiver<WriterMsg>,
    streaming: Receiver<StreamingMsg>,
    error: Arc<Mutex<Option<String>>>,
) {
    let writer = Writer {
        conn,
        error,
        fatal: None,
        state: WriteState {
            aggregator: Aggregator::default(),
            live_runs: HashSet::new(),
            last_heartbeat: Instant::now(),
            last_checkpoint: Instant::now(),
        },
        lossless_head: None,
        stream_head: None,
    };
    writer.run_loop(&lossless, &streaming);
}

struct WriteState {
    aggregator: Aggregator,
    live_runs: HashSet<String>,
    last_heartbeat: Instant,
    last_checkpoint: Instant,
}

struct Writer {
    conn: Connection,
    error: Arc<Mutex<Option<String>>>,
    fatal: Option<TraceError>,
    state: WriteState,
    /// 已从无损通道取出、尚未处理的消息（控制消息或等待排序的 `Input`）。
    lossless_head: Option<WriterMsg>,
    /// 已从流式通道取出、等待按序号排序的流式输入。
    stream_head: Option<StreamingMsg>,
}

enum CycleOutcome {
    Continue,
    Shutdown(SyncSender<Result<(), TraceError>>),
    Disconnected,
}

impl Writer {
    fn run_loop(mut self, lossless: &Receiver<WriterMsg>, streaming: &Receiver<StreamingMsg>) {
        loop {
            match self.run_cycle(lossless, streaming) {
                CycleOutcome::Continue => {}
                CycleOutcome::Shutdown(reply) => {
                    let result = self.finalize();
                    let _ = reply.send(result);
                    return;
                }
                CycleOutcome::Disconnected => {
                    let _ = self.finalize();
                    return;
                }
            }
        }
    }

    fn run_cycle(
        &mut self,
        lossless: &Receiver<WriterMsg>,
        streaming: &Receiver<StreamingMsg>,
    ) -> CycleOutcome {
        let deadline = Instant::now() + Duration::from_millis(TRACE_BATCH_INTERVAL_MS);
        let mut batch = Batch::default();
        loop {
            self.fill_heads(lossless, streaming);
            if self.lossless_head.is_none() && self.stream_head.is_none() {
                let now = Instant::now();
                if now >= deadline {
                    break;
                }
                let wait = deadline
                    .saturating_duration_since(now)
                    .min(Duration::from_millis(STREAMING_POLL_INTERVAL_MS));
                match lossless.recv_timeout(wait) {
                    Ok(message) => self.lossless_head = Some(message),
                    // 短切片超时：回到循环顶部继续轮询流式通道（本批不结束）。
                    Err(RecvTimeoutError::Timeout) => {}
                    Err(RecvTimeoutError::Disconnected) => {
                        self.drain_streaming_all(streaming, &mut batch);
                        let _ = self.finish_batch(&mut batch);
                        return CycleOutcome::Disconnected;
                    }
                }
                continue;
            }
            // 无损队头是控制消息：先排空流式通道再提交（屏障覆盖两条通道）。
            if self.lossless_head.is_some() && !self.next_is_input() {
                let control = self.lossless_head.take().expect("control message");
                if let Some(outcome) = self.handle_control(control, streaming, &mut batch) {
                    return outcome;
                }
                continue;
            }
            self.process_next_ordered(&mut batch);
            if batch.event_rows() >= TRACE_BATCH_ROWS || Instant::now() >= deadline {
                break;
            }
        }
        let _ = self.finish_batch(&mut batch);
        CycleOutcome::Continue
    }

    /// 非阻塞补齐两条通道的队头（已有队头则不覆盖，保证序号比较的对象稳定）。
    fn fill_heads(&mut self, lossless: &Receiver<WriterMsg>, streaming: &Receiver<StreamingMsg>) {
        if self.lossless_head.is_none() {
            self.lossless_head = lossless.try_recv().ok();
        }
        if self.stream_head.is_none() {
            self.stream_head = streaming.try_recv().ok();
        }
    }

    fn next_is_input(&self) -> bool {
        matches!(self.lossless_head, Some(WriterMsg::Input { .. }))
    }

    /// 按发送序号合并两条通道：流式队头序号更小时先处理流式事件，否则处理无损事件。
    ///
    /// 这就是"先流式后无损"的精确版本：序号在发送端取号，同一发送端先发出的流式
    /// 事件序号必然更小，因此不会被其后的无损事件越过（06 §6.4.1 / §6.5.2 因果顺序）。
    fn process_next_ordered(&mut self, batch: &mut Batch) {
        let lossless_seq = match &self.lossless_head {
            Some(WriterMsg::Input { seq, .. }) => Some(*seq),
            _ => None,
        };
        let stream_first = match (&self.stream_head, lossless_seq) {
            (Some(message), Some(seq)) => message.seq < seq,
            (Some(_), None) => true,
            (None, _) => false,
        };
        if stream_first {
            let message = self.stream_head.take().expect("streaming head");
            self.route(message.input, batch);
        } else if let Some(WriterMsg::Input { input, .. }) = self.lossless_head.take() {
            self.route(*input, batch);
        }
    }

    /// 处理控制消息：先排空流式通道（屏障覆盖两条通道），提交后应答。
    fn handle_control(
        &mut self,
        control: WriterMsg,
        streaming: &Receiver<StreamingMsg>,
        batch: &mut Batch,
    ) -> Option<CycleOutcome> {
        self.drain_streaming_all(streaming, batch);
        match control {
            WriterMsg::Flush(reply) => {
                let result = self.finish_batch(batch);
                let _ = reply.send(result);
            }
            WriterMsg::InterruptedCandidates(reply) => {
                // 先提交已入队的写入，保证 SELECT 看到 FIFO 之前的全部写入。
                let result = self
                    .finish_batch(batch)
                    .and_then(|()| self.interrupted_candidates());
                let _ = reply.send(result);
            }
            WriterMsg::MarkInterrupted(request) => {
                let result = self
                    .finish_batch(batch)
                    .and_then(|()| self.mark_interrupted(&request.run_ids, &request.now));
                let _ = request.reply.send(result);
            }
            WriterMsg::UpdateRunColumns(request) => {
                let result = self
                    .finish_batch(batch)
                    .and_then(|()| self.update_run_columns(&request));
                let _ = request.reply.send(result);
            }
            WriterMsg::Shutdown(reply) => {
                let _ = self.finish_batch(batch);
                return Some(CycleOutcome::Shutdown(reply));
            }
            WriterMsg::Input { .. } => unreachable!("input routed through process_next_ordered"),
        }
        None
    }

    /// 排空流式通道中**全部**已入队事件（不受单批行数上限约束）。
    ///
    /// 用于 `Flush`/`Shutdown` 等屏障消息与通道断开：必须把调用前已入队的流式事件
    /// 全部并入本批，否则 flush 返回后仍可能有流式段未落库（屏障不完整）。
    fn drain_streaming_all(&mut self, streaming: &Receiver<StreamingMsg>, batch: &mut Batch) {
        if let Some(message) = self.stream_head.take() {
            self.route(message.input, batch);
        }
        while let Ok(message) = streaming.try_recv() {
            self.route(message.input, batch);
        }
    }

    fn route(&mut self, input: TraceInput, batch: &mut Batch) {
        if self.fatal.is_some() {
            return;
        }
        if matches!(&input, TraceInput::Streaming { .. }) {
            if let Err(err) = self
                .state
                .aggregator
                .handle_streaming(&self.conn, input, batch)
            {
                self.record_fatal(err);
            }
            return;
        }
        match input {
            TraceInput::InstanceSnapshot(snapshot) => {
                self.state
                    .aggregator
                    .note_instance(&snapshot.instance_id, &snapshot.node_id);
                batch.push_snapshot(TraceInput::InstanceSnapshot(snapshot));
            }
            TraceInput::Transition { record, snapshot } => {
                self.state
                    .aggregator
                    .note_instance(&snapshot.instance_id, &snapshot.node_id);
                if record.to.is_terminal() {
                    if let Err(err) =
                        self.state
                            .aggregator
                            .flush_instance(&self.conn, &record.instance_id, batch)
                    {
                        self.record_fatal(err);
                        return;
                    }
                }
                batch.push_event(TraceInput::Transition { record, snapshot });
            }
            TraceInput::RunFinished(_) => {
                // 终态事件之前必须落完全部流式段（seq 因果顺序）。
                if let Err(err) = self.state.aggregator.flush_all(&self.conn, batch) {
                    self.record_fatal(err);
                    return;
                }
                batch.push_event(input);
            }
            other => batch.push_event(other),
        }
    }

    fn finish_batch(&mut self, batch: &mut Batch) -> Result<(), TraceError> {
        if let Some(err) = self.fatal.clone() {
            batch.clear();
            return Err(err);
        }
        if let Err(err) = self.state.aggregator.flush_all(&self.conn, batch) {
            batch.clear();
            return self.fail(err);
        }
        let heartbeat_due = !self.state.live_runs.is_empty()
            && self.state.last_heartbeat.elapsed() >= Duration::from_millis(HEARTBEAT_INTERVAL_MS);
        if batch.is_empty() && !heartbeat_due {
            return Ok(());
        }
        let context = TxContext {
            heartbeat_due,
            live_runs: &self.state.live_runs,
        };
        let result = tx::commit_batch(&mut self.conn, batch, &context);
        batch.clear();
        match result {
            Ok(outcome) => {
                for run_id in outcome.started {
                    self.state.live_runs.insert(run_id);
                }
                for run_id in outcome.finished {
                    self.state.live_runs.remove(&run_id);
                }
                if heartbeat_due {
                    self.state.last_heartbeat = Instant::now();
                }
                self.maybe_checkpoint();
                Ok(())
            }
            Err(err) => self.fail(err),
        }
    }

    /// 中止标定候选查询（只读；经写线程连接保证与前序写入同序）。
    fn interrupted_candidates(&self) -> Result<Vec<InterruptedCandidate>, TraceError> {
        recovery::interrupted_candidates(&self.conn)
    }

    /// 中止标定（单事务 UPDATE）；SQL/IO 失败按不可恢复错误处理（06 §6.4.5）。
    fn mark_interrupted(
        &mut self,
        run_ids: &[String],
        now: &str,
    ) -> Result<Vec<RecoveredRun>, TraceError> {
        match recovery::mark_interrupted(&mut self.conn, run_ids, now) {
            Ok(recovered) => Ok(recovered),
            Err(err) => {
                self.record_fatal(err.clone());
                Err(err)
            }
        }
    }

    /// 仅更新 `runs` 的两列（ISSUE-082；不触碰 status/instances/events）。
    ///
    /// SQL/IO 失败按不可恢复错误处理（06 §6.4.5）；`0` 行 = run 不存在，不是错误。
    fn update_run_columns(
        &mut self,
        request: &crate::writer::UpdateRunColumnsRequest,
    ) -> Result<u64, TraceError> {
        let result = self
            .conn
            .execute(
                "UPDATE runs SET worker_versions_json = ?1, model_params_json = ?2 \
                 WHERE run_id = ?3",
                rusqlite::params![
                    request.worker_versions_json,
                    request.model_params_json,
                    request.run_id
                ],
            )
            .map(|updated| u64::try_from(updated).unwrap_or(u64::MAX))
            .map_err(db_err);
        match result {
            Ok(updated) => Ok(updated),
            Err(err) => {
                self.record_fatal(err.clone());
                Err(err)
            }
        }
    }

    fn finalize(&mut self) -> Result<(), TraceError> {
        if let Some(err) = self.fatal.clone() {
            return Err(err);
        }
        let mut batch = Batch::default();
        if let Err(err) = self.state.aggregator.flush_all(&self.conn, &mut batch) {
            return self.fail(err);
        }
        if !batch.is_empty() {
            let context = TxContext {
                heartbeat_due: false,
                live_runs: &self.state.live_runs,
            };
            if let Err(err) = tx::commit_batch(&mut self.conn, &batch, &context) {
                return self.fail(err);
            }
        }
        self.conn
            .query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_row| Ok(()))
            .map_err(db_err)?;
        Ok(())
    }

    fn maybe_checkpoint(&mut self) {
        if self.state.last_checkpoint.elapsed() < Duration::from_millis(WAL_CHECKPOINT_INTERVAL_MS)
        {
            return;
        }
        let _ = self
            .conn
            .query_row("PRAGMA wal_checkpoint(PASSIVE)", [], |_row| Ok(()));
        self.state.last_checkpoint = Instant::now();
    }

    fn fail(&mut self, err: TraceError) -> Result<(), TraceError> {
        self.record_fatal(err.clone());
        Err(err)
    }

    fn record_fatal(&mut self, err: TraceError) {
        if self.fatal.is_some() {
            return;
        }
        if let Ok(mut slot) = self.error.lock() {
            if slot.is_none() {
                *slot = Some(err.to_string());
            }
        }
        self.fatal = Some(err);
    }
}
