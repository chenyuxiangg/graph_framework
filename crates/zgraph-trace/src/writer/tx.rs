//! 单批事务的 SQL 写入实现（06 §6.2.12 payload→列映射 + §6.3.3 severity 推导）。
//!
//! 一个 [`Batch`] 在一个 `BEGIN IMMEDIATE` 事务内按到达顺序执行：每条事件先
//! `INSERT events` 拿 `seq`，再写明细表并以同一 `seq` 1:1 关联，最后 upsert
//! `instances`/`runs` 进度列。`SQLITE_BUSY` 由 [`commit_batch`] 重试
//! `DB_WRITE_RETRY` 次（间隔 50 ms）。

use std::collections::{HashMap, HashSet};
use std::time::Duration;

use rusqlite::{params, Connection, Transaction, TransactionBehavior};
use serde_json::{json, Value};

use crate::error::TraceError;
use crate::model::{
    json_opt, json_req, GateResultRecord, InstanceSnapshot, InstanceStatus, LlmCallRecord,
    RecordedOutput, RunFinishedInfo, RunStartedInfo, RunStatus, TraceInput, TransitionRecord,
};
use crate::schema::{
    db_err, now_rfc3339_millis, DB_WRITE_RETRY, DB_WRITE_RETRY_INTERVAL_MS, TRACE_PAYLOAD_MAX_BYTES,
};
use crate::writer::stream::Segment;

/// 一批待提交的写操作。
#[derive(Default)]
pub(crate) struct Batch {
    ops: Vec<WriteOp>,
    event_rows: usize,
}

impl Batch {
    /// 追加一个会产生 events 行的输入（含 Transition/GateResult/RunFinished 等）。
    pub(crate) fn push_event(&mut self, input: TraceInput) {
        self.ops.push(WriteOp::Input(input));
        self.event_rows += 1;
    }

    /// 追加一个只 upsert `instances` 的快照（不产生 events 行）。
    pub(crate) fn push_snapshot(&mut self, input: TraceInput) {
        self.ops.push(WriteOp::Input(input));
    }

    /// 追加一个聚合后的流式段（1 events 行 + 1 streaming_events 行）。
    pub(crate) fn push_segment(&mut self, segment: Segment) {
        self.ops.push(WriteOp::Segment(segment));
        self.event_rows += 1;
    }

    /// 当前批 events 行数（用于 256 行上限）。
    pub(crate) fn event_rows(&self) -> usize {
        self.event_rows
    }

    /// 批是否为空。
    pub(crate) fn is_empty(&self) -> bool {
        self.ops.is_empty()
    }

    /// 清空（提交成功或写失败放弃后调用）。
    pub(crate) fn clear(&mut self) {
        self.ops.clear();
        self.event_rows = 0;
    }
}

enum WriteOp {
    Input(TraceInput),
    Segment(Segment),
}

/// 事务级上下文：是否到心跳时间、当前在飞 run 集合。
pub(crate) struct TxContext<'a> {
    pub(crate) heartbeat_due: bool,
    pub(crate) live_runs: &'a HashSet<String>,
}

/// 提交后需要回写到写线程内存态的信息。
#[derive(Default)]
pub(crate) struct TxOutcome {
    pub(crate) started: Vec<String>,
    pub(crate) finished: Vec<String>,
}

/// 内部失败分类：busy 可重试，其余不可恢复。
enum TxError {
    Busy,
    Fatal(TraceError),
}

impl From<TraceError> for TxError {
    fn from(err: TraceError) -> Self {
        Self::Fatal(err)
    }
}

#[derive(Default)]
struct Tracker {
    run_last: HashMap<String, i64>,
    run_counts: HashMap<String, u64>,
    started: Vec<String>,
    finished: Vec<String>,
}

impl Tracker {
    fn track_event(&mut self, run_id: &str, seq: i64) {
        self.run_last.insert(run_id.to_string(), seq);
        *self.run_counts.entry(run_id.to_string()).or_insert(0) += 1;
    }
}

/// 在 `BEGIN IMMEDIATE` 事务中提交一批写操作；busy 重试后仍失败则返回
/// [`TraceError::Storage`]（不可恢复，由上层 fail-stop）。
pub(crate) fn commit_batch(
    conn: &mut Connection,
    batch: &Batch,
    ctx: &TxContext<'_>,
) -> Result<TxOutcome, TraceError> {
    let mut attempt: u32 = 0;
    loop {
        attempt += 1;
        let tx = match conn.transaction_with_behavior(TransactionBehavior::Immediate) {
            Ok(tx) => tx,
            Err(err) if is_busy(&err) && attempt < DB_WRITE_RETRY => {
                sleep_before_retry();
                continue;
            }
            Err(err) if is_busy(&err) => return Err(busy_exhausted()),
            Err(err) => return Err(db_err(err)),
        };
        match execute_and_commit(tx, batch, ctx) {
            Ok(outcome) => return Ok(outcome),
            Err(TxError::Busy) if attempt < DB_WRITE_RETRY => sleep_before_retry(),
            Err(TxError::Busy) => return Err(busy_exhausted()),
            Err(TxError::Fatal(err)) => return Err(err),
        }
    }
}

fn execute_and_commit(
    tx: Transaction<'_>,
    batch: &Batch,
    ctx: &TxContext<'_>,
) -> Result<TxOutcome, TxError> {
    let mut tracker = Tracker::default();
    for op in &batch.ops {
        write_op(&tx, op, &mut tracker)?;
    }
    update_progress(&tx, &tracker)?;
    if ctx.heartbeat_due {
        update_heartbeats(&tx, ctx.live_runs, &tracker.finished)?;
    }
    tx.commit().map_err(|err| {
        if is_busy(&err) {
            TxError::Busy
        } else {
            TxError::Fatal(db_err(err))
        }
    })?;
    Ok(TxOutcome {
        started: tracker.started,
        finished: tracker.finished,
    })
}

fn write_op(tx: &Transaction<'_>, op: &WriteOp, tracker: &mut Tracker) -> Result<(), TxError> {
    match op {
        WriteOp::Input(TraceInput::RunStarted(info)) => write_run_started(tx, info, tracker),
        WriteOp::Input(TraceInput::RunFinished(info)) => write_run_finished(tx, info, tracker),
        WriteOp::Input(TraceInput::Transition { record, snapshot }) => {
            write_transition(tx, record, snapshot, tracker)
        }
        WriteOp::Input(TraceInput::GateResult(record)) => write_gate_result(tx, record, tracker),
        WriteOp::Input(TraceInput::NodeTimedOut {
            run_id,
            instance_id,
            elapsed_ms,
            at,
        }) => write_node_timed_out(tx, run_id, instance_id, *elapsed_ms, at, tracker),
        WriteOp::Input(TraceInput::NodeAbandoned {
            run_id,
            instance_id,
            at,
        }) => write_node_abandoned(tx, run_id, instance_id, at, tracker),
        WriteOp::Input(TraceInput::InstanceSnapshot(snapshot)) => {
            upsert_instance(tx, snapshot, None)?;
            Ok(())
        }
        WriteOp::Input(TraceInput::RecordedOutput(record)) => write_recorded_output(tx, record),
        WriteOp::Input(TraceInput::LlmCall(record)) => write_llm_call(tx, record),
        WriteOp::Input(TraceInput::Streaming { .. }) => Err(TxError::Fatal(TraceError::Storage(
            "streaming input must be aggregated before writing".to_string(),
        ))),
        WriteOp::Segment(segment) => write_segment(tx, segment, tracker),
    }
}

struct EventSpec<'a> {
    run_id: &'a str,
    kind: &'static str,
    severity: &'static str,
    at: &'a str,
    instance_id: Option<&'a str>,
    node_id: Option<&'a str>,
    payload: &'a Value,
}

fn insert_event(tx: &Transaction<'_>, spec: &EventSpec<'_>) -> Result<i64, TxError> {
    let payload = payload_text(spec.kind, spec.payload)?;
    tx.execute(
        "INSERT INTO events (run_id, kind, severity, at, instance_id, node_id, payload) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            spec.run_id,
            spec.kind,
            spec.severity,
            spec.at,
            spec.instance_id,
            spec.node_id,
            payload
        ],
    )
    .map_err(sql_err)?;
    Ok(tx.last_insert_rowid())
}

fn payload_text(kind: &str, payload: &Value) -> Result<String, TxError> {
    let text = serde_json::to_string(payload)
        .map_err(|err| TxError::Fatal(TraceError::Serialize(err.to_string())))?;
    if text.len() <= TRACE_PAYLOAD_MAX_BYTES {
        return Ok(text);
    }
    // 06 §6.4.2：Transition.error 不截断（超限视为内部错误）；其余 payload 截断。
    if kind == "Transition" {
        return Err(TxError::Fatal(TraceError::Serialize(format!(
            "Transition payload exceeds TRACE_PAYLOAD_MAX_BYTES ({})",
            text.len()
        ))));
    }
    let truncated = json!({"_truncated": true, "bytes": text.len()});
    serde_json::to_string(&truncated)
        .map_err(|err| TxError::Fatal(TraceError::Serialize(err.to_string())))
}

/// 规范化 JSON：object 键**递归排序**后重建（不依赖 `serde_json` 的
/// `preserve_order` 特性；工作区因 genai 已打开该特性）。
///
/// `runs.global_vars_json` 用它落库：同一 `global_vars` 快照的存储字节稳定，
/// 便于 L2 记录比对与审计（ISSUE-087）。
fn canonical_json(value: &Value) -> Value {
    match value {
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            let mut sorted = serde_json::Map::new();
            for key in keys {
                sorted.insert(key.clone(), canonical_json(&map[key]));
            }
            Value::Object(sorted)
        }
        Value::Array(items) => Value::Array(items.iter().map(canonical_json).collect()),
        other => other.clone(),
    }
}

fn write_run_started(
    tx: &Transaction<'_>,
    info: &RunStartedInfo,
    tracker: &mut Tracker,
) -> Result<(), TxError> {
    tx.execute(
        "INSERT OR IGNORE INTO graphs (graph_hash, graph_name, dsl_version, dsl_json, created_at) \
         VALUES (?1, ?2, '1.0', ?3, ?4)",
        params![
            info.graph_hash,
            info.graph_name,
            info.dsl_json,
            info.started_at
        ],
    )
    .map_err(sql_err)?;
    let entry_inputs = json_req(&info.entry_inputs)?;
    // v3（ISSUE-087）：`global_vars` 以 canonical JSON 落库（object 键递归排序），
    // 保证同一快照的落库字节稳定、便于 L2 记录比对/审计。
    if !info.global_vars.is_object() {
        return Err(TxError::Fatal(TraceError::Serialize(
            "RunStarted.global_vars must be a JSON object".to_string(),
        )));
    }
    let global_vars_json = json_req(&canonical_json(&info.global_vars))?;
    let seed = i64::try_from(info.seed).unwrap_or(i64::MAX);
    tx.execute(
        "INSERT OR IGNORE INTO runs \
         (run_id, graph_hash, status, user_id, started_at, entry_inputs, owner_pid, owner_host, \
          heartbeat_at, seed, record_mode, global_vars_json) \
         VALUES (?1, ?2, 'Running', ?3, ?4, ?5, ?6, ?7, ?4, ?8, ?9, ?10)",
        params![
            info.run_id,
            info.graph_hash,
            info.user_id,
            info.started_at,
            entry_inputs,
            i64::from(info.owner_pid),
            info.owner_host,
            seed,
            info.record_mode.as_str(),
            global_vars_json
        ],
    )
    .map_err(sql_err)?;
    let payload = json!({
        "status": RunStatus::Running.as_str(),
        "graph_hash": &info.graph_hash,
        "user_id": &info.user_id,
    });
    let seq = insert_event(
        tx,
        &EventSpec {
            run_id: &info.run_id,
            kind: "RunStarted",
            severity: "info",
            at: &info.started_at,
            instance_id: None,
            node_id: None,
            payload: &payload,
        },
    )?;
    tracker.track_event(&info.run_id, seq);
    tracker.started.push(info.run_id.clone());
    Ok(())
}

fn write_run_finished(
    tx: &Transaction<'_>,
    info: &RunFinishedInfo,
    tracker: &mut Tracker,
) -> Result<(), TxError> {
    let output_json = json_opt(info.output_summary.as_ref())?;
    let (error_code, error_message) = match &info.error {
        Some(error) => (Some(error.code.clone()), Some(error.message.clone())),
        None => (None, None),
    };
    let severity = severity_run_finished(info.status);
    let payload = match &info.error {
        Some(error) => json!({
            "status": info.status.as_str(),
            "error": {"code": &error.code, "message": &error.message},
        }),
        None => json!({"status": info.status.as_str()}),
    };
    let seq = insert_event(
        tx,
        &EventSpec {
            run_id: &info.run_id,
            kind: "RunFinished",
            severity,
            at: &info.at,
            instance_id: None,
            node_id: None,
            payload: &payload,
        },
    )?;
    tracker.track_event(&info.run_id, seq);
    tx.execute(
        "UPDATE runs SET status = ?2, finished_at = ?3, output_json = ?4, error_code = ?5, \
         error_message = ?6, streaming_dropped = ?7, abandoned_tasks = ?8, \
         trace_backpressure = ?9, killed_workers = ?10, heartbeat_at = ?3 WHERE run_id = ?1",
        params![
            info.run_id,
            info.status.as_str(),
            info.at,
            output_json,
            error_code,
            error_message,
            i64::try_from(info.metrics.streaming_dropped).unwrap_or(i64::MAX),
            i64::try_from(info.metrics.abandoned_tasks).unwrap_or(i64::MAX),
            i64::try_from(info.metrics.trace_backpressure).unwrap_or(i64::MAX),
            i64::try_from(info.metrics.killed_workers).unwrap_or(i64::MAX),
        ],
    )
    .map_err(sql_err)?;
    tracker.finished.push(info.run_id.clone());
    Ok(())
}

fn write_transition(
    tx: &Transaction<'_>,
    record: &TransitionRecord,
    snapshot: &InstanceSnapshot,
    tracker: &mut Tracker,
) -> Result<(), TxError> {
    let payload = json!({"record": {
        "run_id": &record.run_id,
        "instance_id": &record.instance_id,
        "node_id": &record.node_id,
        "attempt": record.attempt,
        "from": record.from.as_str(),
        "to": record.to.as_str(),
        "reason": record.reason.as_deref(),
        "ts": &record.ts,
    }});
    let seq = insert_event(
        tx,
        &EventSpec {
            run_id: &record.run_id,
            kind: "Transition",
            severity: severity_transition(record.to),
            at: &record.ts,
            instance_id: Some(&record.instance_id),
            node_id: Some(&record.node_id),
            payload: &payload,
        },
    )?;
    let error = json_opt(snapshot.error.as_ref())?;
    tx.execute(
        "INSERT INTO transitions \
         (seq, run_id, instance_id, node_id, attempt, from_status, to_status, reason, \
          error_code, error, at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
        params![
            seq,
            record.run_id,
            record.instance_id,
            record.node_id,
            i64::from(record.attempt),
            record.from.as_str(),
            record.to.as_str(),
            record.reason,
            snapshot.error_code,
            error,
            record.ts
        ],
    )
    .map_err(sql_err)?;
    let finished_seq = if record.to.is_terminal() {
        Some(seq)
    } else {
        None
    };
    upsert_instance(tx, snapshot, finished_seq)?;
    tracker.track_event(&record.run_id, seq);
    Ok(())
}

fn write_gate_result(
    tx: &Transaction<'_>,
    record: &GateResultRecord,
    tracker: &mut Tracker,
) -> Result<(), TxError> {
    let severity = severity_gate(record.error.as_deref(), record.passed);
    let payload = json!({"record": {
        "run_id": &record.run_id,
        "instance_id": &record.instance_id,
        "node_id": &record.node_id,
        "edge_index": record.edge_index,
        "to_node": &record.to_node,
        "to_port": &record.to_port,
        "gate_fn": record.gate_fn.as_deref(),
        "passed": record.passed,
        "error": record.error.as_deref(),
        "deferred": record.deferred,
        "attempt": record.attempt,
        "verdict": record.verdict.as_ref(),
        "ts": &record.ts,
    }});
    let seq = insert_event(
        tx,
        &EventSpec {
            run_id: &record.run_id,
            kind: "GateResult",
            severity,
            at: &record.ts,
            instance_id: Some(&record.instance_id),
            node_id: Some(&record.node_id),
            payload: &payload,
        },
    )?;
    let passed: i64 = if record.passed { 1 } else { 0 };
    let deferred: i64 = if record.deferred { 1 } else { 0 };
    let verdict_json = json_opt(record.verdict.as_ref())?;
    tx.execute(
        "INSERT INTO gate_results \
         (seq, run_id, instance_id, node_id, edge_index, to_node, to_port, gate_fn, passed, \
          error, at, deferred, attempt, verdict_json) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
        params![
            seq,
            record.run_id,
            record.instance_id,
            record.node_id,
            record.edge_index,
            record.to_node,
            record.to_port,
            record.gate_fn,
            passed,
            record.error,
            record.ts,
            deferred,
            i64::from(record.attempt),
            verdict_json
        ],
    )
    .map_err(sql_err)?;
    tracker.track_event(&record.run_id, seq);
    Ok(())
}

fn write_node_timed_out(
    tx: &Transaction<'_>,
    run_id: &str,
    instance_id: &str,
    elapsed_ms: u64,
    at: &str,
    tracker: &mut Tracker,
) -> Result<(), TxError> {
    let payload = json!({"elapsed_ms": elapsed_ms});
    let seq = insert_event(
        tx,
        &EventSpec {
            run_id,
            kind: "NodeTimedOut",
            severity: "error",
            at,
            instance_id: Some(instance_id),
            node_id: None,
            payload: &payload,
        },
    )?;
    tracker.track_event(run_id, seq);
    Ok(())
}

fn write_node_abandoned(
    tx: &Transaction<'_>,
    run_id: &str,
    instance_id: &str,
    at: &str,
    tracker: &mut Tracker,
) -> Result<(), TxError> {
    let payload = json!({"at": at});
    let seq = insert_event(
        tx,
        &EventSpec {
            run_id,
            kind: "NodeAbandoned",
            severity: "warn",
            at,
            instance_id: Some(instance_id),
            node_id: None,
            payload: &payload,
        },
    )?;
    tracker.track_event(run_id, seq);
    Ok(())
}

fn write_recorded_output(tx: &Transaction<'_>, record: &RecordedOutput) -> Result<(), TxError> {
    let output_json = json_opt(record.output_json.as_ref())?;
    tx.execute(
        "INSERT OR REPLACE INTO recorded_outputs \
         (run_id, instance_id, node_id, attempt, output_hash, output_json, recorded_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            record.run_id,
            record.instance_id,
            record.node_id,
            i64::from(record.attempt),
            record.output_hash,
            output_json,
            record.recorded_at
        ],
    )
    .map_err(sql_err)?;
    Ok(())
}

fn write_llm_call(tx: &Transaction<'_>, record: &LlmCallRecord) -> Result<(), TxError> {
    let request_json = json_req(&record.request_json)?;
    let response_json = json_opt(record.response_json.as_ref())?;
    tx.execute(
        "INSERT OR REPLACE INTO llm_calls \
         (run_id, instance_id, call_index, request_json, response_json, status, recorded_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            record.run_id,
            record.instance_id,
            i64::from(record.call_index),
            request_json,
            response_json,
            record.status,
            record.recorded_at
        ],
    )
    .map_err(sql_err)?;
    Ok(())
}

fn write_segment(
    tx: &Transaction<'_>,
    segment: &Segment,
    tracker: &mut Tracker,
) -> Result<(), TxError> {
    let severity = segment_severity(segment);
    let payload = segment_payload(segment);
    let seq = insert_event(
        tx,
        &EventSpec {
            run_id: &segment.run_id,
            kind: "Streaming",
            severity,
            at: &segment.last_at,
            instance_id: Some(&segment.instance_id),
            node_id: segment.node_id.as_deref(),
            payload: &payload,
        },
    )?;
    let summary = json_opt(segment.summary.as_ref())?;
    let ok = segment.ok.map(|ok| if ok { 1_i64 } else { 0_i64 });
    tx.execute(
        "INSERT INTO streaming_events \
         (seq, run_id, instance_id, kind, segment_index, text, text_chars, text_bytes, \
          truncated, delta_count, call_id, tool_name, ok, summary, percent, first_at, last_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17)",
        params![
            seq,
            segment.run_id,
            segment.instance_id,
            segment.kind,
            segment.segment_index,
            segment.text,
            segment.text_chars,
            segment.text_bytes,
            if segment.truncated { 1_i64 } else { 0_i64 },
            segment.delta_count,
            segment.call_id,
            segment.tool_name,
            ok,
            summary,
            segment.percent,
            segment.first_at,
            segment.last_at
        ],
    )
    .map_err(sql_err)?;
    tracker.track_event(&segment.run_id, seq);
    Ok(())
}

fn segment_severity(segment: &Segment) -> &'static str {
    if segment.kind == "tool_call_end" && segment.ok == Some(false) {
        "warn"
    } else {
        "info"
    }
}

fn segment_payload(segment: &Segment) -> Value {
    match segment.kind {
        "token" | "reasoning" => json!({
            "variant": segment.kind,
            "segment_index": segment.segment_index,
            "chars": segment.text_chars,
            "delta_count": segment.delta_count,
        }),
        "tool_call_start" => json!({
            "variant": "tool_call_start",
            "segment_index": segment.segment_index,
            "call_id": segment.call_id.as_deref(),
            "tool_name": segment.tool_name.as_deref(),
        }),
        "tool_call_end" => json!({
            "variant": "tool_call_end",
            "segment_index": segment.segment_index,
            "call_id": segment.call_id.as_deref(),
            "tool_name": segment.tool_name.as_deref(),
            "ok": segment.ok,
        }),
        _ => json!({
            "variant": "progress",
            "segment_index": segment.segment_index,
            "percent": segment.percent,
            "message": segment.text.as_deref(),
        }),
    }
}

fn upsert_instance(
    tx: &Transaction<'_>,
    snapshot: &InstanceSnapshot,
    finished_seq: Option<i64>,
) -> Result<(), TxError> {
    let input = json_opt(snapshot.input.as_ref())?;
    let output_json = json_opt(snapshot.output.as_ref())?;
    let error = json_opt(snapshot.error.as_ref())?;
    tx.execute(
        "INSERT INTO instances \
         (instance_id, run_id, node_id, node_type, attempt, status, input, output_json, error, \
          error_code, created_seq, started_at, finished_at, finished_seq) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, NULL, ?11, ?12, ?13) \
         ON CONFLICT(instance_id) DO UPDATE SET \
           node_id = excluded.node_id, node_type = excluded.node_type, \
           attempt = excluded.attempt, status = excluded.status, \
           input = COALESCE(excluded.input, instances.input), \
           output_json = COALESCE(excluded.output_json, instances.output_json), \
           error = COALESCE(excluded.error, instances.error), \
           error_code = COALESCE(excluded.error_code, instances.error_code), \
           started_at = COALESCE(excluded.started_at, instances.started_at), \
           finished_at = COALESCE(excluded.finished_at, instances.finished_at), \
           finished_seq = COALESCE(excluded.finished_seq, instances.finished_seq)",
        params![
            snapshot.instance_id,
            snapshot.run_id,
            snapshot.node_id,
            snapshot.node_type,
            i64::from(snapshot.attempt),
            snapshot.status.as_str(),
            input,
            output_json,
            error,
            snapshot.error_code,
            snapshot.started_at,
            snapshot.finished_at,
            finished_seq
        ],
    )
    .map_err(sql_err)?;
    Ok(())
}

fn update_progress(tx: &Transaction<'_>, tracker: &Tracker) -> Result<(), TxError> {
    for (run_id, last_seq) in &tracker.run_last {
        let count = tracker.run_counts.get(run_id).copied().unwrap_or(0);
        tx.execute(
            "UPDATE runs SET last_seq = MAX(last_seq, ?2), event_count = event_count + ?3 \
             WHERE run_id = ?1",
            params![run_id, last_seq, i64::try_from(count).unwrap_or(i64::MAX)],
        )
        .map_err(sql_err)?;
    }
    Ok(())
}

fn update_heartbeats(
    tx: &Transaction<'_>,
    live_runs: &HashSet<String>,
    finished: &[String],
) -> Result<(), TxError> {
    let finished: HashSet<&str> = finished.iter().map(String::as_str).collect();
    let now = now_rfc3339_millis();
    for run_id in live_runs {
        if finished.contains(run_id.as_str()) {
            continue;
        }
        tx.execute(
            "UPDATE runs SET heartbeat_at = ?2 \
             WHERE run_id = ?1 AND status IN ('Running','Paused')",
            params![run_id, now],
        )
        .map_err(sql_err)?;
    }
    Ok(())
}

fn severity_run_finished(status: RunStatus) -> &'static str {
    if status == RunStatus::Failed {
        "error"
    } else {
        "info"
    }
}

fn severity_transition(to: InstanceStatus) -> &'static str {
    match to {
        InstanceStatus::Crashed | InstanceStatus::TimedOut => "error",
        _ => "info",
    }
}

fn severity_gate(error: Option<&str>, passed: bool) -> &'static str {
    if error.is_some() {
        "error"
    } else if !passed {
        "warn"
    } else {
        "info"
    }
}

fn sql_err(err: rusqlite::Error) -> TxError {
    if is_busy(&err) {
        TxError::Busy
    } else {
        TxError::Fatal(db_err(err))
    }
}

pub(crate) fn is_busy(err: &rusqlite::Error) -> bool {
    matches!(
        err,
        rusqlite::Error::SqliteFailure(inner, _)
            if matches!(
                inner.code,
                rusqlite::ErrorCode::DatabaseBusy | rusqlite::ErrorCode::DatabaseLocked
            )
    )
}

pub(crate) fn sleep_before_retry() {
    std::thread::sleep(Duration::from_millis(DB_WRITE_RETRY_INTERVAL_MS));
}

pub(crate) fn busy_exhausted() -> TraceError {
    TraceError::Storage(format!(
        "sqlite busy after {DB_WRITE_RETRY} attempts ({}ms each)",
        DB_WRITE_RETRY_INTERVAL_MS
    ))
}
