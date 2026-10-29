//! `Streaming` 事件的写线程内聚合（06 §6.5）。
//!
//! 仅 `token`/`reasoning` 各自独立缓冲；`tool_call_*`/`progress` 立即成段，且到达前
//! 先 flush 同实例的 token/reasoning 缓冲（因果顺序）。flush 触发：100 ms 批量
//! 边界、缓冲 ≥4096 字符、同实例非 delta 事件、实例终态迁移、通道关闭。
//!
//! 段文本为无损拼接；≥16384 UTF-8 字节时截断到码点边界并 `truncated=1`。
//! `segment_index` 按 `(run_id, instance_id, kind)` 从 0 递增，首次出现时用
//! `SELECT MAX(segment_index)` 从库中初始化（进程重启/新实例）。

use std::collections::HashMap;

use rusqlite::{params, Connection, OptionalExtension};

use crate::error::TraceError;
use crate::model::{StreamingEvent, TraceInput};
use crate::schema::{db_err, STREAM_SEGMENT_MAX_BYTES, STREAM_SEGMENT_MAX_CHARS};
use crate::writer::tx::Batch;

/// 可聚合的 delta 类别。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) enum StreamKind {
    /// `token` 增量。
    Token,
    /// `reasoning` 增量。
    Reasoning,
}

impl StreamKind {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Token => "token",
            Self::Reasoning => "reasoning",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
struct BufferKey {
    run_id: String,
    instance_id: String,
    kind: StreamKind,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct IndexKey {
    run_id: String,
    instance_id: String,
    kind: &'static str,
}

#[derive(Debug)]
struct DeltaBuffer {
    node_id: Option<String>,
    text: String,
    delta_count: i64,
    first_at: String,
    last_at: String,
}

impl DeltaBuffer {
    fn new(first_at: String) -> Self {
        Self {
            node_id: None,
            text: String::new(),
            delta_count: 0,
            first_at: first_at.clone(),
            last_at: first_at,
        }
    }
}

/// 一条待聚合的 delta（`token`/`reasoning`）。
struct Delta<'a> {
    run_id: &'a str,
    instance_id: &'a str,
    node_id: Option<String>,
    at: String,
    kind: StreamKind,
    text: String,
}

/// 一个待落库的流式段（1 events 行 + 1 streaming_events 行）。
#[derive(Debug)]
pub(crate) struct Segment {
    pub(crate) run_id: String,
    pub(crate) instance_id: String,
    pub(crate) node_id: Option<String>,
    pub(crate) kind: &'static str,
    pub(crate) segment_index: i64,
    pub(crate) text: Option<String>,
    pub(crate) text_chars: Option<i64>,
    pub(crate) text_bytes: Option<i64>,
    pub(crate) truncated: bool,
    pub(crate) delta_count: Option<i64>,
    pub(crate) call_id: Option<String>,
    pub(crate) tool_name: Option<String>,
    pub(crate) ok: Option<bool>,
    pub(crate) summary: Option<serde_json::Value>,
    pub(crate) percent: Option<i64>,
    pub(crate) first_at: String,
    pub(crate) last_at: String,
}

/// 写线程持有的聚合器（非线程安全，仅写线程访问）。
#[derive(Default)]
pub(crate) struct Aggregator {
    buffers: HashMap<BufferKey, DeltaBuffer>,
    next_index: HashMap<IndexKey, i64>,
}

impl Aggregator {
    /// 当前是否有未落库的 delta 缓冲。
    pub(crate) fn is_empty(&self) -> bool {
        self.buffers.is_empty()
    }

    /// 处理一条 `Streaming` 输入：delta 入缓冲（可能提前分段），非 delta 立即成段。
    pub(crate) fn handle_streaming(
        &mut self,
        conn: &Connection,
        input: TraceInput,
        batch: &mut Batch,
    ) -> Result<(), TraceError> {
        let TraceInput::Streaming {
            run_id,
            instance_id,
            node_id,
            event,
            at,
        } = input
        else {
            return Err(TraceError::Storage(
                "non-streaming input passed to aggregator".to_string(),
            ));
        };
        match event {
            StreamingEvent::TokenDelta { text } => self.push_delta(
                conn,
                Delta {
                    run_id: &run_id,
                    instance_id: &instance_id,
                    node_id,
                    at,
                    kind: StreamKind::Token,
                    text,
                },
                batch,
            ),
            StreamingEvent::ReasoningDelta { text } => self.push_delta(
                conn,
                Delta {
                    run_id: &run_id,
                    instance_id: &instance_id,
                    node_id,
                    at,
                    kind: StreamKind::Reasoning,
                    text,
                },
                batch,
            ),
            other => {
                self.flush_instance(conn, &instance_id, batch)?;
                let segment =
                    self.non_delta_segment(conn, run_id, instance_id, node_id, other, at)?;
                batch.push_segment(segment);
                Ok(())
            }
        }
    }

    /// flush 全部缓冲（批量边界/通道关闭）。
    pub(crate) fn flush_all(
        &mut self,
        conn: &Connection,
        batch: &mut Batch,
    ) -> Result<(), TraceError> {
        let mut keys: Vec<BufferKey> = self.buffers.keys().cloned().collect();
        self.flush_keys(conn, &mut keys, batch)
    }

    /// flush 某实例的全部缓冲（实例终态迁移）。
    pub(crate) fn flush_instance(
        &mut self,
        conn: &Connection,
        instance_id: &str,
        batch: &mut Batch,
    ) -> Result<(), TraceError> {
        let mut keys: Vec<BufferKey> = self
            .buffers
            .keys()
            .filter(|key| key.instance_id == instance_id)
            .cloned()
            .collect();
        self.flush_keys(conn, &mut keys, batch)
    }

    fn push_delta(
        &mut self,
        conn: &Connection,
        delta: Delta<'_>,
        batch: &mut Batch,
    ) -> Result<(), TraceError> {
        let key = BufferKey {
            run_id: delta.run_id.to_string(),
            instance_id: delta.instance_id.to_string(),
            kind: delta.kind,
        };
        let buffer = self
            .buffers
            .entry(key.clone())
            .or_insert_with(|| DeltaBuffer::new(delta.at.clone()));
        if buffer.node_id.is_none() {
            buffer.node_id = delta.node_id;
        }
        buffer.text.push_str(&delta.text);
        buffer.delta_count += 1;
        buffer.last_at = delta.at;
        if buffer.text.chars().count() >= STREAM_SEGMENT_MAX_CHARS {
            self.flush_key(conn, &key, batch)?;
        }
        Ok(())
    }

    fn flush_keys(
        &mut self,
        conn: &Connection,
        keys: &mut Vec<BufferKey>,
        batch: &mut Batch,
    ) -> Result<(), TraceError> {
        keys.sort();
        for key in keys.drain(..) {
            self.flush_key(conn, &key, batch)?;
        }
        Ok(())
    }

    fn flush_key(
        &mut self,
        conn: &Connection,
        key: &BufferKey,
        batch: &mut Batch,
    ) -> Result<(), TraceError> {
        let Some(buffer) = self.buffers.remove(key) else {
            return Ok(());
        };
        let index_key = IndexKey {
            run_id: key.run_id.clone(),
            instance_id: key.instance_id.clone(),
            kind: key.kind.as_str(),
        };
        let segment_index = self.assign_index(conn, &index_key)?;
        let node_id = match buffer.node_id {
            Some(node_id) => Some(node_id),
            None => lookup_instance_node(conn, &key.instance_id)?,
        };
        let (text, text_chars, text_bytes, truncated) = finalize_text(buffer.text);
        batch.push_segment(Segment {
            run_id: key.run_id.clone(),
            instance_id: key.instance_id.clone(),
            node_id,
            kind: key.kind.as_str(),
            segment_index,
            text: Some(text),
            text_chars: Some(text_chars),
            text_bytes: Some(text_bytes),
            truncated,
            delta_count: Some(buffer.delta_count),
            call_id: None,
            tool_name: None,
            ok: None,
            summary: None,
            percent: None,
            first_at: buffer.first_at,
            last_at: buffer.last_at,
        });
        Ok(())
    }

    fn non_delta_segment(
        &mut self,
        conn: &Connection,
        run_id: String,
        instance_id: String,
        node_id: Option<String>,
        event: StreamingEvent,
        at: String,
    ) -> Result<Segment, TraceError> {
        let kind = event.variant();
        let index_key = IndexKey {
            run_id: run_id.clone(),
            instance_id: instance_id.clone(),
            kind,
        };
        let segment_index = self.assign_index(conn, &index_key)?;
        let node_id = match node_id {
            Some(node_id) => Some(node_id),
            None => lookup_instance_node(conn, &instance_id)?,
        };
        let mut segment = Segment {
            run_id,
            instance_id,
            node_id,
            kind,
            segment_index,
            text: None,
            text_chars: None,
            text_bytes: None,
            truncated: false,
            delta_count: None,
            call_id: None,
            tool_name: None,
            ok: None,
            summary: None,
            percent: None,
            first_at: at.clone(),
            last_at: at,
        };
        match event {
            StreamingEvent::ToolCallStart {
                call_id,
                tool_name,
                args_summary,
            } => {
                segment.call_id = Some(call_id);
                segment.tool_name = Some(tool_name);
                segment.summary = Some(args_summary);
            }
            StreamingEvent::ToolCallEnd {
                call_id,
                tool_name,
                ok,
                result_summary,
            } => {
                segment.call_id = Some(call_id);
                segment.tool_name = Some(tool_name);
                segment.ok = Some(ok);
                segment.summary = result_summary;
            }
            StreamingEvent::Progress { percent, message } => {
                segment.text_chars =
                    Some(i64::try_from(message.chars().count()).unwrap_or(i64::MAX));
                segment.text_bytes = Some(i64::try_from(message.len()).unwrap_or(i64::MAX));
                segment.percent = Some(i64::from(percent));
                segment.text = Some(message);
            }
            StreamingEvent::TokenDelta { .. } | StreamingEvent::ReasoningDelta { .. } => {
                return Err(TraceError::Storage(
                    "delta event routed to non-delta segment".to_string(),
                ));
            }
        }
        Ok(segment)
    }

    fn assign_index(&mut self, conn: &Connection, key: &IndexKey) -> Result<i64, TraceError> {
        match self.next_index.get(key).copied() {
            Some(index) => {
                self.next_index.insert(key.clone(), index + 1);
                Ok(index)
            }
            None => {
                let sql = "SELECT COALESCE(MAX(segment_index), -1) + 1 FROM streaming_events \
                           WHERE run_id = ?1 AND instance_id = ?2 AND kind = ?3";
                let index: i64 = conn
                    .query_row(sql, params![key.run_id, key.instance_id, key.kind], |row| {
                        row.get(0)
                    })
                    .map_err(db_err)?;
                self.next_index.insert(key.clone(), index + 1);
                Ok(index)
            }
        }
    }
}

fn lookup_instance_node(
    conn: &Connection,
    instance_id: &str,
) -> Result<Option<String>, TraceError> {
    conn.query_row(
        "SELECT node_id FROM instances WHERE instance_id = ?1",
        params![instance_id],
        |row| row.get::<_, String>(0),
    )
    .optional()
    .map_err(db_err)
}

/// 无损拼接结果按 UTF-8 字节上限截断到码点边界。
fn finalize_text(text: String) -> (String, i64, i64, bool) {
    if text.len() <= STREAM_SEGMENT_MAX_BYTES {
        let chars = i64::try_from(text.chars().count()).unwrap_or(i64::MAX);
        let bytes = i64::try_from(text.len()).unwrap_or(i64::MAX);
        return (text, chars, bytes, false);
    }
    let mut truncated = String::new();
    for ch in text.chars() {
        if truncated.len() + ch.len_utf8() > STREAM_SEGMENT_MAX_BYTES {
            break;
        }
        truncated.push(ch);
    }
    let chars = i64::try_from(truncated.chars().count()).unwrap_or(i64::MAX);
    let bytes = i64::try_from(truncated.len()).unwrap_or(i64::MAX);
    (truncated, chars, bytes, true)
}
