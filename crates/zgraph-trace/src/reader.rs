//! 只读查询（`zgraph trace` / L1 回放数据源）。
//!
//! 连接以 `SQLITE_OPEN_READ_ONLY` 打开：**不创建库、不迁移、不做崩溃标定**
//! （06 §6.6/§6.8）。`events` 严格 `ORDER BY seq ASC`——`at` 是墙钟，可能逆序，
//! 禁止用于排序（06 §6.3.2）。

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use rusqlite::types::Value as SqlValue;
use rusqlite::{params_from_iter, Connection};
use serde_json::Value;

use crate::error::TraceError;
use crate::model::{parse_json, parse_json_opt, InstanceStatus, RunStatus};
use crate::schema::{self, db_err};

/// `zgraph trace` 的查询参数。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EventQuery {
    /// 只返回 `seq > since_seq` 的事件（增量跟随）。
    pub since_seq: u64,
    /// 最多返回条数；`0` = 不限。
    pub limit: usize,
    /// 只返回这些 `kind`；`None` = 全部（空 `Vec` 等价于不过滤）。
    pub kinds: Option<Vec<String>>,
    /// 是否包含 `Streaming` 事件（`--no-streaming` 时置 `false`）。
    pub include_streaming: bool,
}

impl Default for EventQuery {
    fn default() -> Self {
        Self {
            since_seq: 0,
            limit: 0,
            kinds: None,
            include_streaming: true,
        }
    }
}

impl EventQuery {
    /// 全量查询参数。
    pub fn new() -> Self {
        Self::default()
    }
}

/// 事件信封行（06 §6.2.6）。
#[derive(Debug, Clone, PartialEq)]
pub struct EventRow {
    /// 全局单调序号（per DB 文件；允许空洞）。
    pub seq: i64,
    /// 所属 run。
    pub run_id: String,
    /// 事件类别（7 类之一）。
    pub kind: String,
    /// 派生严重级别（`info`/`warn`/`error`）。
    pub severity: String,
    /// 事件时刻（RFC3339 毫秒 Z）。
    pub at: String,
    /// 实例 id（可空）。
    pub instance_id: Option<String>,
    /// 节点 id（可空）。
    pub node_id: Option<String>,
    /// payload JSON（超 64 KiB 时为 `{"_truncated":true,"bytes":N}`）。
    pub payload: serde_json::Value,
}

/// 图定义行（06 §6.2.3；内容寻址，供 trace 头部展示与 L1 回放校验）。
#[derive(Debug, Clone, PartialEq)]
pub struct GraphRow {
    /// 图内容哈希（blake3 前 16 hex）。
    pub graph_hash: String,
    /// 图名（`metadata.name`；DB 允许 NULL）。
    pub graph_name: Option<String>,
    /// DSL 版本（当前恒 `1.0`）。
    pub dsl_version: String,
    /// 规范化 DSL JSON 原文。
    pub dsl_json: String,
    /// 首次写入时刻（RFC3339 毫秒 Z）。
    pub created_at: String,
}

/// run 行（含 v2 可复现列；06 §6.2.4 + architecture §5.2）。
#[derive(Debug, Clone, PartialEq)]
pub struct RunRow {
    /// run id。
    pub run_id: String,
    /// 图内容哈希。
    pub graph_hash: String,
    /// 图名（LEFT JOIN `graphs`；graph 行缺失时为 `None`，不报错）。
    pub graph_name: Option<String>,
    /// run 状态。
    pub status: RunStatus,
    /// 用户 id（DB NULL 读作空串）。
    pub user_id: String,
    /// 启动时刻。
    pub started_at: String,
    /// 终态时刻（中断标定后为 NULL）。
    pub finished_at: Option<String>,
    /// entry 输入快照。
    pub entry_inputs: Option<serde_json::Value>,
    /// run 输出。
    pub output_json: Option<serde_json::Value>,
    /// 领域错误码（00 §0.4；非存储命名空间）。
    pub error_code: Option<String>,
    /// 领域错误消息。
    pub error_message: Option<String>,
    /// 是否被中止标定（06 §6.8；由 [`crate::TraceWriter::mark_interrupted`] 写入）。
    pub interrupted: bool,
    /// 最近一次标定时间。
    pub recovered_at: Option<String>,
    /// 本 run 最后一条 events.seq。
    pub last_seq: i64,
    /// 本 run 已写 events 行数。
    pub event_count: i64,
    /// 丢弃的流式事件数。
    pub streaming_dropped: u64,
    /// 放弃任务数（v2 恒 0）。
    pub abandoned_tasks: u64,
    /// trace 背压累计等待毫秒。
    pub trace_backpressure: u64,
    /// 被 kill 的 worker 数。
    pub killed_workers: u64,
    /// 可复现随机种子。
    pub seed: u64,
    /// 记录模式原始存储值（`none`/`hash`/`full`）。
    pub record_mode: Option<String>,
    /// owner 进程号（06 §6.8.1 判活；ISSUE-048）。
    pub owner_pid: Option<u32>,
    /// owner 主机名（v1 恒本机；06 §6.2.4）。
    pub owner_host: Option<String>,
    /// 写线程最近心跳时刻（≤5 s 刷新；ISSUE-048 判活辅助）。
    pub heartbeat_at: Option<String>,
    /// run 级 `global_vars` 快照（v3 / ISSUE-087，canonical JSON object）。
    ///
    /// `None` = 未记录：v2→v3 迁移前的历史 run 或非 CLI/库驱动的旧记录；
    /// `replay --mode=outputs` 对 `None` 按空 map 兼容并在 stderr 告警。
    pub global_vars: Option<serde_json::Value>,
    /// worker/SDK/Python 版本集合（ISSUE-082；JSON 数组，run 结束时由 CLI 写入）。
    ///
    /// `None` = 未写入（`:memory:` 库、旧记录或 run 未走到结束写入点）。
    pub worker_versions: Option<serde_json::Value>,
    /// 模型参数快照集合（ISSUE-082；JSON 数组，**不含密钥**，run 结束时由 CLI 写入）。
    ///
    /// `None` = 未写入（同 [`RunRow::worker_versions`]）。
    pub model_params: Option<serde_json::Value>,
}

/// 实例快照行（06 §6.2.5）。
#[derive(Debug, Clone, PartialEq)]
pub struct InstanceRow {
    /// 实例 id。
    pub instance_id: String,
    /// 所属 run。
    pub run_id: String,
    /// 节点 id。
    pub node_id: String,
    /// 节点类型字面量。
    pub node_type: String,
    /// 尝试序号。
    pub attempt: u32,
    /// 当前状态。
    pub status: InstanceStatus,
    /// 触发输入快照。
    pub input: Option<serde_json::Value>,
    /// 节点输出。
    pub output_json: Option<serde_json::Value>,
    /// `NodeError` JSON。
    pub error: Option<serde_json::Value>,
    /// 错误码。
    pub error_code: Option<String>,
    /// 创建对应 events.seq（M2 允许 NULL）。
    pub created_seq: Option<i64>,
    /// 开始执行时刻。
    pub started_at: Option<String>,
    /// 终态时刻。
    pub finished_at: Option<String>,
    /// 进入终态对应 events.seq。
    pub finished_seq: Option<i64>,
}

/// 状态迁移明细行（06 §6.2.7；`seq` 1:1 关联 events）。
#[derive(Debug, Clone, PartialEq)]
pub struct TransitionRow {
    /// 明细主键。
    pub transition_id: i64,
    /// 同事务 events.seq（权威序）。
    pub seq: i64,
    /// 所属 run。
    pub run_id: String,
    /// 实例 id。
    pub instance_id: String,
    /// 节点 id。
    pub node_id: String,
    /// 尝试序号。
    pub attempt: u32,
    /// 迁移前状态。
    pub from_status: InstanceStatus,
    /// 迁移后状态。
    pub to_status: InstanceStatus,
    /// 迁移原因。
    pub reason: Option<String>,
    /// 错误码（00 §0.4）。
    pub error_code: Option<String>,
    /// `NodeError` JSON。
    pub error: Option<serde_json::Value>,
    /// 迁移时刻。
    pub at: String,
}

/// 门控结果行（06 §6.2.8；v4 = W2a / DR-005 增 `deferred`/`attempt`/`verdict`）。
#[derive(Debug, Clone, PartialEq)]
pub struct GateResultRow {
    /// 明细主键。
    pub gate_result_id: i64,
    /// 同事务 events.seq。
    pub seq: i64,
    /// 所属 run。
    pub run_id: String,
    /// 发布者实例 id。
    pub instance_id: String,
    /// 发布者节点 id。
    pub node_id: String,
    /// 边索引。
    pub edge_index: i64,
    /// 目标节点。
    pub to_node: String,
    /// 目标端口。
    pub to_port: String,
    /// 门控符号（`None` = 恒真）。
    pub gate_fn: Option<String>,
    /// 是否放行。
    pub passed: bool,
    /// gate 异常信息。
    pub error: Option<String>,
    /// 记录时刻。
    pub at: String,
    /// 是否延迟结算（W2a / DR-005）。
    pub deferred: bool,
    /// 发布者实例 attempt（W2a / DR-005）。
    pub attempt: u32,
    /// 门控判定值（W2a0 冻结；仅内置 `check_output`/`compare` 且 `params.path`
    /// 为字符串时有值）。
    pub verdict: Option<Value>,
}

/// 流式聚合段行（06 §6.2.9；`node_id` 来自 events 回填）。
#[derive(Debug, Clone, PartialEq)]
pub struct StreamingRow {
    /// 同事务 events.seq。
    pub seq: i64,
    /// 所属 run。
    pub run_id: String,
    /// 实例 id。
    pub instance_id: String,
    /// 节点 id（events 回填）。
    pub node_id: Option<String>,
    /// 变体（`token`/`reasoning`/`tool_call_start`/`tool_call_end`/`progress`）。
    pub kind: String,
    /// 同 `(instance_id, kind)` 的段序号，从 0 递增。
    pub segment_index: i64,
    /// 聚合文本（progress 为 message）。
    pub text: Option<String>,
    /// 字符数。
    pub text_chars: Option<i64>,
    /// UTF-8 字节数。
    pub text_bytes: Option<i64>,
    /// 是否因 16 KiB 上限被截断。
    pub truncated: bool,
    /// token/reasoning 合并的原始 delta 数。
    pub delta_count: Option<i64>,
    /// tool_call id。
    pub call_id: Option<String>,
    /// 工具名。
    pub tool_name: Option<String>,
    /// tool_call_end 成功标志。
    pub ok: Option<bool>,
    /// 参数/结果摘要 JSON。
    pub summary: Option<serde_json::Value>,
    /// progress 百分比。
    pub percent: Option<i64>,
    /// 窗口首个 delta 时刻。
    pub first_at: String,
    /// 窗口最后 delta 时刻（也是 events.at）。
    pub last_at: String,
}

/// 记录输出行（architecture §5.2）。
#[derive(Debug, Clone, PartialEq)]
pub struct RecordedOutputRow {
    /// 明细主键。
    pub recorded_output_id: i64,
    /// 所属 run。
    pub run_id: String,
    /// 实例 id。
    pub instance_id: String,
    /// 节点 id。
    pub node_id: String,
    /// 尝试序号。
    pub attempt: u32,
    /// 输出哈希。
    pub output_hash: String,
    /// 输出全文（`hash` 模式为 `None`）。
    pub output_json: Option<serde_json::Value>,
    /// 记录时刻。
    pub recorded_at: String,
}

/// LLM 调用记录行（architecture §5.2）。
#[derive(Debug, Clone, PartialEq)]
pub struct LlmCallRow {
    /// 明细主键。
    pub llm_call_id: i64,
    /// 所属 run。
    pub run_id: String,
    /// 实例 id。
    pub instance_id: String,
    /// 实例内调用序号。
    pub call_index: u32,
    /// 请求全文。
    pub request_json: serde_json::Value,
    /// 响应全文。
    pub response_json: Option<serde_json::Value>,
    /// 调用状态。
    pub status: String,
    /// 记录时刻。
    pub recorded_at: String,
}

/// `trace --stats` 统计摘要（06 §6.11.2）。
#[derive(Debug, Clone, PartialEq)]
pub struct RunStats {
    /// run id。
    pub run_id: String,
    /// run 状态。
    pub status: RunStatus,
    /// 时长（毫秒；无 finished_at 时为 `None`）。
    pub duration_ms: Option<u64>,
    /// 各 `events.kind` 计数。
    pub kinds: BTreeMap<String, u64>,
    /// events 总数。
    pub events_total: u64,
    /// 各实例状态计数。
    pub instances: BTreeMap<String, u64>,
    /// 流式段数。
    pub streaming_segments: u64,
    /// 丢弃的流式事件数。
    pub streaming_dropped: u64,
    /// `NodeTimedOut` 事件数。
    pub timeouts: u64,
    /// `NodeAbandoned` 事件数。
    pub abandoned: u64,
    /// trace 背压累计等待毫秒。
    pub trace_backpressure: u64,
    /// DB 路径。
    pub db_path: String,
    /// DB 主文件字节数。
    pub db_size_bytes: u64,
    /// 同一 DB 内的 runs 总数（`trace --stats` 的 `db ... runs N`）。
    pub db_runs: u64,
}

/// 只读 trace 连接。
pub struct TraceReader {
    conn: Connection,
    db_path: PathBuf,
}

impl TraceReader {
    /// 以只读方式打开数据库；库不存在或没有 trace schema 时报
    /// [`TraceError::Storage`]，且**不会创建文件**。
    pub fn open_readonly(path: impl AsRef<Path>) -> Result<TraceReader, TraceError> {
        let db_path = path.as_ref().to_path_buf();
        let conn = schema::open_readonly_connection(&db_path)?;
        Ok(TraceReader { conn, db_path })
    }

    /// 数据库路径。
    pub fn db_path(&self) -> &Path {
        &self.db_path
    }

    /// 读取 `PRAGMA user_version`。
    pub fn schema_version(&self) -> Result<i64, TraceError> {
        schema::user_version(&self.conn)
    }

    /// 查询单 run；不存在返回 `None`。
    pub fn run(&self, run_id: &str) -> Result<Option<RunRow>, TraceError> {
        let sql = "SELECT r.run_id, r.graph_hash, g.graph_name, r.status, r.user_id, \
                   r.started_at, r.finished_at, r.entry_inputs, r.output_json, r.error_code, \
                   r.error_message, r.interrupted, r.recovered_at, r.last_seq, r.event_count, \
                   r.streaming_dropped, r.abandoned_tasks, r.trace_backpressure, \
                   r.killed_workers, r.seed, r.record_mode, r.owner_pid, r.owner_host, \
                   r.heartbeat_at, r.global_vars_json, r.worker_versions_json, \
                   r.model_params_json \
                   FROM runs r LEFT JOIN graphs g ON g.graph_hash = r.graph_hash \
                   WHERE r.run_id = ?1";
        let mut stmt = self.conn.prepare(sql).map_err(db_err)?;
        let mut rows = stmt.query([run_id]).map_err(db_err)?;
        match rows.next().map_err(db_err)? {
            Some(row) => Ok(Some(map_run_row(row)?)),
            None => Ok(None),
        }
    }

    /// 按 `graph_hash` 查询图定义；不存在返回 `None`（不报错）。
    pub fn graph(&self, graph_hash: &str) -> Result<Option<GraphRow>, TraceError> {
        let sql = "SELECT graph_hash, graph_name, dsl_version, dsl_json, created_at \
                   FROM graphs WHERE graph_hash = ?1";
        let mut stmt = self.conn.prepare(sql).map_err(db_err)?;
        let mut rows = stmt.query([graph_hash]).map_err(db_err)?;
        match rows.next().map_err(db_err)? {
            Some(row) => Ok(Some(map_graph_row(row)?)),
            None => Ok(None),
        }
    }

    /// 按 `seq` 升序查询事件（支持 since/limit/kinds/是否含 Streaming）。
    pub fn events(&self, run_id: &str, query: &EventQuery) -> Result<Vec<EventRow>, TraceError> {
        let (sql, args) = build_events_sql(run_id, query);
        let mut stmt = self.conn.prepare(&sql).map_err(db_err)?;
        let mut rows = stmt.query(params_from_iter(args)).map_err(db_err)?;
        let mut events = Vec::new();
        while let Some(row) = rows.next().map_err(db_err)? {
            events.push(map_event_row(row)?);
        }
        Ok(events)
    }

    /// 查询 run 的全部实例（按插入/rowid 顺序；M2 `created_seq` 允许 NULL）。
    pub fn instances(&self, run_id: &str) -> Result<Vec<InstanceRow>, TraceError> {
        let sql = "SELECT instance_id, run_id, node_id, node_type, attempt, status, input, \
                   output_json, error, error_code, created_seq, started_at, finished_at, \
                   finished_seq FROM instances WHERE run_id = ?1 ORDER BY rowid ASC";
        let mut stmt = self.conn.prepare(sql).map_err(db_err)?;
        let mut rows = stmt.query([run_id]).map_err(db_err)?;
        let mut instances = Vec::new();
        while let Some(row) = rows.next().map_err(db_err)? {
            instances.push(map_instance_row(row)?);
        }
        Ok(instances)
    }

    /// 查询状态迁移（按权威序 `seq` 升序）。
    pub fn transitions(&self, run_id: &str) -> Result<Vec<TransitionRow>, TraceError> {
        let sql = "SELECT transition_id, seq, run_id, instance_id, node_id, attempt, \
                   from_status, to_status, reason, error_code, error, at \
                   FROM transitions WHERE run_id = ?1 ORDER BY seq ASC";
        let mut stmt = self.conn.prepare(sql).map_err(db_err)?;
        let mut rows = stmt.query([run_id]).map_err(db_err)?;
        let mut records = Vec::new();
        while let Some(row) = rows.next().map_err(db_err)? {
            records.push(map_transition_row(row)?);
        }
        Ok(records)
    }

    /// 查询门控结果（按 `seq` 升序）。
    pub fn gate_results(&self, run_id: &str) -> Result<Vec<GateResultRow>, TraceError> {
        let sql = "SELECT gate_result_id, seq, run_id, instance_id, node_id, edge_index, \
                   to_node, to_port, gate_fn, passed, error, at, deferred, attempt, verdict_json \
                   FROM gate_results WHERE run_id = ?1 ORDER BY seq ASC";
        let mut stmt = self.conn.prepare(sql).map_err(db_err)?;
        let mut rows = stmt.query([run_id]).map_err(db_err)?;
        let mut results = Vec::new();
        while let Some(row) = rows.next().map_err(db_err)? {
            results.push(map_gate_row(row)?);
        }
        Ok(results)
    }

    /// 查询流式段（`ORDER BY instance_id, kind, segment_index`）。
    pub fn streaming_events(&self, run_id: &str) -> Result<Vec<StreamingRow>, TraceError> {
        let sql = "SELECT s.seq, s.run_id, s.instance_id, e.node_id, \
                   s.kind, s.segment_index, s.text, s.text_chars, s.text_bytes, s.truncated, \
                   s.delta_count, s.call_id, s.tool_name, s.ok, s.summary, s.percent, \
                   s.first_at, s.last_at \
                   FROM streaming_events s JOIN events e ON e.seq = s.seq \
                   WHERE s.run_id = ?1 \
                   ORDER BY s.instance_id ASC, s.kind ASC, s.segment_index ASC";
        let mut stmt = self.conn.prepare(sql).map_err(db_err)?;
        let mut rows = stmt.query([run_id]).map_err(db_err)?;
        let mut segments = Vec::new();
        while let Some(row) = rows.next().map_err(db_err)? {
            segments.push(map_streaming_row(row)?);
        }
        Ok(segments)
    }

    /// 查询记录输出（按 instance/attempt 升序）。
    pub fn recorded_outputs(&self, run_id: &str) -> Result<Vec<RecordedOutputRow>, TraceError> {
        let sql = "SELECT recorded_output_id, run_id, instance_id, node_id, attempt, \
                   output_hash, output_json, recorded_at FROM recorded_outputs \
                   WHERE run_id = ?1 ORDER BY instance_id ASC, attempt ASC, recorded_output_id ASC";
        let mut stmt = self.conn.prepare(sql).map_err(db_err)?;
        let mut rows = stmt.query([run_id]).map_err(db_err)?;
        let mut outputs = Vec::new();
        while let Some(row) = rows.next().map_err(db_err)? {
            outputs.push(map_recorded_output_row(row)?);
        }
        Ok(outputs)
    }

    /// 查询 LLM 调用记录（按 instance/call_index 升序）。
    pub fn llm_calls(&self, run_id: &str) -> Result<Vec<LlmCallRow>, TraceError> {
        let sql = "SELECT llm_call_id, run_id, instance_id, call_index, request_json, \
                   response_json, status, recorded_at FROM llm_calls \
                   WHERE run_id = ?1 ORDER BY instance_id ASC, call_index ASC";
        let mut stmt = self.conn.prepare(sql).map_err(db_err)?;
        let mut rows = stmt.query([run_id]).map_err(db_err)?;
        let mut calls = Vec::new();
        while let Some(row) = rows.next().map_err(db_err)? {
            calls.push(map_llm_call_row(row)?);
        }
        Ok(calls)
    }

    /// `trace --stats` 摘要（06 §6.11.2）；run 不存在返回
    /// [`TraceError::NotFound`]。
    pub fn stats(&self, run_id: &str) -> Result<RunStats, TraceError> {
        let run = self
            .run(run_id)?
            .ok_or_else(|| TraceError::NotFound(format!("run not found: {run_id}")))?;
        let kinds = self.kind_counts(run_id)?;
        let events_total = kinds.values().sum();
        let instances = self.instance_status_counts(run_id)?;
        let streaming_segments = self.count(sql_count_streaming(), run_id)?;
        let db_size_bytes = std::fs::metadata(&self.db_path)
            .map(|metadata| metadata.len())
            .unwrap_or(0);
        let db_runs = self.all_runs_count()?;
        Ok(RunStats {
            run_id: run.run_id,
            status: run.status,
            duration_ms: duration_ms(&run.started_at, run.finished_at.as_deref()),
            timeouts: kinds.get("NodeTimedOut").copied().unwrap_or(0),
            abandoned: kinds.get("NodeAbandoned").copied().unwrap_or(0),
            kinds,
            events_total,
            instances,
            streaming_segments,
            streaming_dropped: run.streaming_dropped,
            trace_backpressure: run.trace_backpressure,
            db_path: self.db_path.display().to_string(),
            db_size_bytes,
            db_runs,
        })
    }

    fn kind_counts(&self, run_id: &str) -> Result<BTreeMap<String, u64>, TraceError> {
        let sql = "SELECT kind, count(*) FROM events WHERE run_id = ?1 GROUP BY kind ORDER BY kind";
        let mut stmt = self.conn.prepare(sql).map_err(db_err)?;
        let mut rows = stmt.query([run_id]).map_err(db_err)?;
        let mut counts = BTreeMap::new();
        while let Some(row) = rows.next().map_err(db_err)? {
            let kind: String = row.get(0).map_err(db_err)?;
            let count: u64 = to_u64(row.get(1).map_err(db_err)?);
            counts.insert(kind, count);
        }
        Ok(counts)
    }

    fn instance_status_counts(&self, run_id: &str) -> Result<BTreeMap<String, u64>, TraceError> {
        let sql = "SELECT status, count(*) FROM instances WHERE run_id = ?1 \
                   GROUP BY status ORDER BY status";
        let mut stmt = self.conn.prepare(sql).map_err(db_err)?;
        let mut rows = stmt.query([run_id]).map_err(db_err)?;
        let mut counts = BTreeMap::new();
        while let Some(row) = rows.next().map_err(db_err)? {
            let status: String = row.get(0).map_err(db_err)?;
            let count: u64 = to_u64(row.get(1).map_err(db_err)?);
            counts.insert(status, count);
        }
        Ok(counts)
    }

    fn count(&self, sql: &str, run_id: &str) -> Result<u64, TraceError> {
        let value: i64 = self
            .conn
            .query_row(sql, [run_id], |row| row.get(0))
            .map_err(db_err)?;
        Ok(to_u64(value))
    }

    fn all_runs_count(&self) -> Result<u64, TraceError> {
        let value: i64 = self
            .conn
            .query_row("SELECT count(*) FROM runs", [], |row| row.get(0))
            .map_err(db_err)?;
        Ok(to_u64(value))
    }
}

fn sql_count_streaming() -> &'static str {
    "SELECT count(*) FROM streaming_events WHERE run_id = ?1"
}

fn build_events_sql(run_id: &str, query: &EventQuery) -> (String, Vec<SqlValue>) {
    let mut sql = String::from(
        "SELECT seq, run_id, kind, severity, at, instance_id, node_id, payload \
         FROM events WHERE run_id = ?1 AND seq > ?2",
    );
    let mut args = vec![
        SqlValue::Text(run_id.to_string()),
        SqlValue::Integer(i64::try_from(query.since_seq).unwrap_or(i64::MAX)),
    ];
    if let Some(kinds) = &query.kinds {
        if !kinds.is_empty() {
            let placeholders = vec!["?"; kinds.len()].join(", ");
            sql.push_str(&format!(" AND kind IN ({placeholders})"));
            args.extend(kinds.iter().map(|kind| SqlValue::Text(kind.clone())));
        }
    }
    if !query.include_streaming {
        sql.push_str(" AND kind <> 'Streaming'");
    }
    sql.push_str(" ORDER BY seq ASC");
    if query.limit > 0 {
        sql.push_str(" LIMIT ?");
        args.push(SqlValue::Integer(
            i64::try_from(query.limit).unwrap_or(i64::MAX),
        ));
    }
    (sql, args)
}

fn duration_ms(started_at: &str, finished_at: Option<&str>) -> Option<u64> {
    let finished_at = finished_at?;
    let start = chrono::DateTime::parse_from_rfc3339(started_at).ok()?;
    let end = chrono::DateTime::parse_from_rfc3339(finished_at).ok()?;
    let millis = end.signed_duration_since(start).num_milliseconds();
    u64::try_from(millis).ok()
}

fn to_u64(value: i64) -> u64 {
    u64::try_from(value).unwrap_or(0)
}

fn to_u32(value: i64) -> u32 {
    u32::try_from(value).unwrap_or(0)
}

/// DB 可空 `owner_pid` → `Option<u32>`（越界/负值读作 `None`，不报错）。
fn to_opt_u32(value: Option<i64>) -> Option<u32> {
    value.and_then(|raw| u32::try_from(raw).ok())
}

fn parse_status(text: &str) -> Result<InstanceStatus, TraceError> {
    InstanceStatus::parse(text)
        .ok_or_else(|| TraceError::Storage(format!("invalid instance status in db: {text}")))
}

fn parse_run_status(text: &str) -> Result<RunStatus, TraceError> {
    RunStatus::parse(text)
        .ok_or_else(|| TraceError::Storage(format!("invalid run status in db: {text}")))
}

fn map_run_row(row: &rusqlite::Row<'_>) -> Result<RunRow, TraceError> {
    let status: String = row.get(3).map_err(db_err)?;
    let user_id: Option<String> = row.get(4).map_err(db_err)?;
    let interrupted: i64 = row.get(11).map_err(db_err)?;
    Ok(RunRow {
        run_id: row.get(0).map_err(db_err)?,
        graph_hash: row.get(1).map_err(db_err)?,
        graph_name: row.get(2).map_err(db_err)?,
        status: parse_run_status(&status)?,
        user_id: user_id.unwrap_or_default(),
        started_at: row.get(5).map_err(db_err)?,
        finished_at: row.get(6).map_err(db_err)?,
        entry_inputs: parse_json_opt(row.get(7).map_err(db_err)?)?,
        output_json: parse_json_opt(row.get(8).map_err(db_err)?)?,
        error_code: row.get(9).map_err(db_err)?,
        error_message: row.get(10).map_err(db_err)?,
        interrupted: interrupted != 0,
        recovered_at: row.get(12).map_err(db_err)?,
        last_seq: row.get(13).map_err(db_err)?,
        event_count: row.get(14).map_err(db_err)?,
        streaming_dropped: to_u64(row.get(15).map_err(db_err)?),
        abandoned_tasks: to_u64(row.get(16).map_err(db_err)?),
        trace_backpressure: to_u64(row.get(17).map_err(db_err)?),
        killed_workers: to_u64(row.get(18).map_err(db_err)?),
        seed: to_u64(row.get(19).map_err(db_err)?),
        record_mode: row.get(20).map_err(db_err)?,
        owner_pid: to_opt_u32(row.get(21).map_err(db_err)?),
        owner_host: row.get(22).map_err(db_err)?,
        heartbeat_at: row.get(23).map_err(db_err)?,
        global_vars: parse_json_opt(row.get(24).map_err(db_err)?)?,
        worker_versions: parse_json_opt(row.get(25).map_err(db_err)?)?,
        model_params: parse_json_opt(row.get(26).map_err(db_err)?)?,
    })
}

fn map_graph_row(row: &rusqlite::Row<'_>) -> Result<GraphRow, TraceError> {
    Ok(GraphRow {
        graph_hash: row.get(0).map_err(db_err)?,
        graph_name: row.get(1).map_err(db_err)?,
        dsl_version: row.get(2).map_err(db_err)?,
        dsl_json: row.get(3).map_err(db_err)?,
        created_at: row.get(4).map_err(db_err)?,
    })
}

fn map_event_row(row: &rusqlite::Row<'_>) -> Result<EventRow, TraceError> {
    let payload: String = row.get(7).map_err(db_err)?;
    Ok(EventRow {
        seq: row.get(0).map_err(db_err)?,
        run_id: row.get(1).map_err(db_err)?,
        kind: row.get(2).map_err(db_err)?,
        severity: row.get(3).map_err(db_err)?,
        at: row.get(4).map_err(db_err)?,
        instance_id: row.get(5).map_err(db_err)?,
        node_id: row.get(6).map_err(db_err)?,
        payload: parse_json(&payload)?,
    })
}

fn map_instance_row(row: &rusqlite::Row<'_>) -> Result<InstanceRow, TraceError> {
    let status: String = row.get(5).map_err(db_err)?;
    Ok(InstanceRow {
        instance_id: row.get(0).map_err(db_err)?,
        run_id: row.get(1).map_err(db_err)?,
        node_id: row.get(2).map_err(db_err)?,
        node_type: row.get(3).map_err(db_err)?,
        attempt: to_u32(row.get(4).map_err(db_err)?),
        status: parse_status(&status)?,
        input: parse_json_opt(row.get(6).map_err(db_err)?)?,
        output_json: parse_json_opt(row.get(7).map_err(db_err)?)?,
        error: parse_json_opt(row.get(8).map_err(db_err)?)?,
        error_code: row.get(9).map_err(db_err)?,
        created_seq: row.get(10).map_err(db_err)?,
        started_at: row.get(11).map_err(db_err)?,
        finished_at: row.get(12).map_err(db_err)?,
        finished_seq: row.get(13).map_err(db_err)?,
    })
}

fn map_transition_row(row: &rusqlite::Row<'_>) -> Result<TransitionRow, TraceError> {
    let from_status: String = row.get(6).map_err(db_err)?;
    let to_status: String = row.get(7).map_err(db_err)?;
    Ok(TransitionRow {
        transition_id: row.get(0).map_err(db_err)?,
        seq: row.get(1).map_err(db_err)?,
        run_id: row.get(2).map_err(db_err)?,
        instance_id: row.get(3).map_err(db_err)?,
        node_id: row.get(4).map_err(db_err)?,
        attempt: to_u32(row.get(5).map_err(db_err)?),
        from_status: parse_status(&from_status)?,
        to_status: parse_status(&to_status)?,
        reason: row.get(8).map_err(db_err)?,
        error_code: row.get(9).map_err(db_err)?,
        error: parse_json_opt(row.get(10).map_err(db_err)?)?,
        at: row.get(11).map_err(db_err)?,
    })
}

fn map_gate_row(row: &rusqlite::Row<'_>) -> Result<GateResultRow, TraceError> {
    let passed: i64 = row.get(9).map_err(db_err)?;
    let deferred: i64 = row.get(12).map_err(db_err)?;
    Ok(GateResultRow {
        gate_result_id: row.get(0).map_err(db_err)?,
        seq: row.get(1).map_err(db_err)?,
        run_id: row.get(2).map_err(db_err)?,
        instance_id: row.get(3).map_err(db_err)?,
        node_id: row.get(4).map_err(db_err)?,
        edge_index: row.get(5).map_err(db_err)?,
        to_node: row.get(6).map_err(db_err)?,
        to_port: row.get(7).map_err(db_err)?,
        gate_fn: row.get(8).map_err(db_err)?,
        passed: passed != 0,
        error: row.get(10).map_err(db_err)?,
        at: row.get(11).map_err(db_err)?,
        deferred: deferred != 0,
        attempt: to_u32(row.get(13).map_err(db_err)?),
        verdict: parse_json_opt(row.get(14).map_err(db_err)?)?,
    })
}

fn map_streaming_row(row: &rusqlite::Row<'_>) -> Result<StreamingRow, TraceError> {
    let truncated: i64 = row.get(9).map_err(db_err)?;
    let ok: Option<i64> = row.get(13).map_err(db_err)?;
    Ok(StreamingRow {
        seq: row.get(0).map_err(db_err)?,
        run_id: row.get(1).map_err(db_err)?,
        instance_id: row.get(2).map_err(db_err)?,
        node_id: row.get(3).map_err(db_err)?,
        kind: row.get(4).map_err(db_err)?,
        segment_index: row.get(5).map_err(db_err)?,
        text: row.get(6).map_err(db_err)?,
        text_chars: row.get(7).map_err(db_err)?,
        text_bytes: row.get(8).map_err(db_err)?,
        truncated: truncated != 0,
        delta_count: row.get(10).map_err(db_err)?,
        call_id: row.get(11).map_err(db_err)?,
        tool_name: row.get(12).map_err(db_err)?,
        ok: ok.map(|value| value != 0),
        summary: parse_json_opt(row.get(14).map_err(db_err)?)?,
        percent: row.get(15).map_err(db_err)?,
        first_at: row.get(16).map_err(db_err)?,
        last_at: row.get(17).map_err(db_err)?,
    })
}

fn map_recorded_output_row(row: &rusqlite::Row<'_>) -> Result<RecordedOutputRow, TraceError> {
    Ok(RecordedOutputRow {
        recorded_output_id: row.get(0).map_err(db_err)?,
        run_id: row.get(1).map_err(db_err)?,
        instance_id: row.get(2).map_err(db_err)?,
        node_id: row.get(3).map_err(db_err)?,
        attempt: to_u32(row.get(4).map_err(db_err)?),
        output_hash: row.get(5).map_err(db_err)?,
        output_json: parse_json_opt(row.get(6).map_err(db_err)?)?,
        recorded_at: row.get(7).map_err(db_err)?,
    })
}

fn map_llm_call_row(row: &rusqlite::Row<'_>) -> Result<LlmCallRow, TraceError> {
    let request_json: String = row.get(4).map_err(db_err)?;
    Ok(LlmCallRow {
        llm_call_id: row.get(0).map_err(db_err)?,
        run_id: row.get(1).map_err(db_err)?,
        instance_id: row.get(2).map_err(db_err)?,
        call_index: to_u32(row.get(3).map_err(db_err)?),
        request_json: parse_json(&request_json)?,
        response_json: parse_json_opt(row.get(5).map_err(db_err)?)?,
        status: row.get(6).map_err(db_err)?,
        recorded_at: row.get(7).map_err(db_err)?,
    })
}
