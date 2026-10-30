//! 单写线程的事件循环与批处理（06 §6.4.3）。
//!
//! 语义（相对 06 伪代码的 M2 实现）：
//! - 先非阻塞/限时接收，凑满 [`TRACE_BATCH_ROWS`] 或到达 100 ms 截止即提交；
//! - 提交前 flush 聚合缓冲（100 ms 聚合窗口 = 批量窗口）；
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
use crate::writer::WriterMsg;

/// 写线程入口：消费通道直到 `Shutdown` 或全部发送端断开。
pub(crate) fn run(conn: Connection, rx: Receiver<WriterMsg>, error: Arc<Mutex<Option<String>>>) {
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
    };
    writer.run_loop(&rx);
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
}

enum CycleOutcome {
    Continue,
    Shutdown(SyncSender<Result<(), TraceError>>),
    Disconnected,
}

impl Writer {
    fn run_loop(mut self, rx: &Receiver<WriterMsg>) {
        loop {
            match self.run_cycle(rx) {
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

    fn run_cycle(&mut self, rx: &Receiver<WriterMsg>) -> CycleOutcome {
        let deadline = Instant::now() + Duration::from_millis(TRACE_BATCH_INTERVAL_MS);
        let mut batch = Batch::default();
        loop {
            if batch.event_rows() >= TRACE_BATCH_ROWS {
                break;
            }
            let now = Instant::now();
            let idle = batch.is_empty() && self.state.aggregator.is_empty();
            if !idle && now >= deadline {
                break;
            }
            let timeout = deadline.saturating_duration_since(now);
            match rx.recv_timeout(timeout) {
                Ok(WriterMsg::Input(input)) => self.route(*input, &mut batch),
                Ok(WriterMsg::Flush(reply)) => {
                    let result = self.finish_batch(&mut batch);
                    let _ = reply.send(result);
                }
                Ok(WriterMsg::InterruptedCandidates(reply)) => {
                    // 先提交已入队的写入，保证 SELECT 看到 FIFO 之前的全部写入。
                    let result = self
                        .finish_batch(&mut batch)
                        .and_then(|()| self.interrupted_candidates());
                    let _ = reply.send(result);
                }
                Ok(WriterMsg::MarkInterrupted(request)) => {
                    let result = self
                        .finish_batch(&mut batch)
                        .and_then(|()| self.mark_interrupted(&request.run_ids, &request.now));
                    let _ = request.reply.send(result);
                }
                Ok(WriterMsg::Shutdown(reply)) => {
                    let _ = self.finish_batch(&mut batch);
                    return CycleOutcome::Shutdown(reply);
                }
                Err(RecvTimeoutError::Timeout) => break,
                Err(RecvTimeoutError::Disconnected) => {
                    let _ = self.finish_batch(&mut batch);
                    return CycleOutcome::Disconnected;
                }
            }
        }
        let _ = self.finish_batch(&mut batch);
        CycleOutcome::Continue
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
