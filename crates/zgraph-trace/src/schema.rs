//! SQLite schema、连接级 PRAGMA 与 forward-only 迁移（06 §6.2/§6.7）。
//!
//! v2 = spec 06 的 6+2 张表（`runs`/`instances`/`events`/`transitions`/
//! `gate_results`/`streaming_events` + `schema_migrations`/`graphs`）
//! + architecture §5.2 的增量：`runs` 新增 5 列与两张记录表。
//!
//! v3 = ISSUE-087（06 §6.7 / DR-003 D4）：`runs` 新增 `global_vars_json TEXT`
//! （canonical JSON：object 键递归排序，落库字节稳定），供 `replay --mode=outputs`
//! 重建 run 级 `global_vars` 快照。
//!
//! v4 = W2a（DR-004 D1/D5 + DR-005 + 06 §6.2.8）：`gate_results` 新增
//! `deferred INTEGER NOT NULL DEFAULT 0`（0/1）、`attempt INTEGER NOT NULL DEFAULT 0`、
//! `verdict_json TEXT`（可空 JSON），供 deferrable gate / 有限返工回放与审计。
//!
//! v5 = W3a（DR-006 §5/§6 + 06 §6.2.5/§6.2.7/§6.2.14）：`instances.status` 与
//! `transitions.from_status`/`to_status` 的 CHECK 增 `WaitingHuman`（SQLite 不能
//! `ALTER` CHECK，故两表按 12 步法重建并逐列复制）；新增 `human_decisions` 表与
//! `idx_human_decisions_pending` 部分索引（C13 分表写者：decide INSERT / run consumer
//! 只更新 `consumed_at`，[`crate::TraceWriter`] 不写该表）。
//!
//! v6 = W4a（DR-007 §8 + 06 §6.2.5/§6.2.15）：`instances` 增 `item_index`
//! （非 foreach 实例恒 0）与 `item_key`（`NULL` = 按索引派生），唯一键改为
//! `(run_id, node_id, attempt, item_index)`（12 步法重建、显式列复制、旧行取
//! `0/NULL`）；新增 `foreach_expansions` 展开事实表 + `run_id` 索引（写入者为
//! [`crate::TraceWriter`]：内核执行事实，不是 C13 决策表）。
//!
//! 版本策略：`PRAGMA user_version` 为权威；`0` = 新库（单事务建当前版本），
//! `1` = spec 06 v1 库、`2` = v2 库、`3` = v3 库、`4` = v4 库、`5` = v5 库
//! （逐版本迁移到 v6），`6` = 就绪，`>6` = 拒绝。
//! 迁移前 `VACUUM INTO '<db>.bak.v{old}-{ts}'` 备份；每版本一个事务；失败则恢复
//! 备份并返回 [`TraceError::Storage`]（06 §6.7）。

use std::path::{Path, PathBuf};
use std::time::Duration;

use rusqlite::{params, Connection, OpenFlags, Transaction, TransactionBehavior};

use crate::error::TraceError;

/// 本 crate 实现的 trace schema 版本（06 §6.7 + architecture §5.2；
/// v4 = W2a / DR-005，v5 = W3a / DR-006（`WaitingHuman` + `human_decisions`），
/// v6 = W4a / DR-007（item 实例 + `foreach_expansions`））。
pub const TRACE_SCHEMA_VERSION: i64 = 6;

/// SQLite `busy_timeout`（毫秒，06 §6.2.1）。
pub const DB_BUSY_TIMEOUT_MS: u32 = 5_000;

/// run decision consumer 的 RO 轮询连接 `busy_timeout`（毫秒，DR-006 §5.2 / 06 §6.6 C13）。
///
/// 轮询是短查询：用更小的 1 s 超时避免轮询连接在写者持锁时长时间阻塞
/// （C13：decide INSERT / consumer UPDATE 仍用 [`DB_BUSY_TIMEOUT_MS`] = 5000 ms）。
pub const DECISION_POLL_BUSY_TIMEOUT_MS: u32 = 1_000;

/// 写线程遇 `SQLITE_BUSY` 的总尝试次数（06 §6.2.1 `DB_WRITE_RETRY`）。
pub const DB_WRITE_RETRY: u32 = 3;

/// 每次 busy 重试的等待间隔（毫秒，06 §6.2.1）。
pub const DB_WRITE_RETRY_INTERVAL_MS: u64 = 50;

/// 无损事件通道容量（06 §6.4.1 / 03 §3.12 `TRACE_CHANNEL_CAP`）。
pub const TRACE_CHANNEL_CAP: usize = 8_192;

/// 可丢流式事件通道容量（06 §6.4.1 / 00 §0.5 `STREAMING_CHANNEL_CAP`）。
pub const STREAMING_CHANNEL_CAP: usize = 1_024;

/// 单批最大 events 行数（00 §0.5 `TRACE_BATCH_ROWS`）。
pub const TRACE_BATCH_ROWS: usize = 256;

/// 批量提交最大延迟，同时是流式聚合窗口（00 §0.5 / 06 §6.5.2）。
pub const TRACE_BATCH_INTERVAL_MS: u64 = 100;

/// 写线程刷新 `runs.heartbeat_at` 的间隔（06 §6.4.3）。
pub const HEARTBEAT_INTERVAL_MS: u64 = 5_000;

/// 写线程周期 `wal_checkpoint(PASSIVE)` 间隔（06 §6.4.3）。
pub const WAL_CHECKPOINT_INTERVAL_MS: u64 = 5_000;

/// 流式聚合窗口（毫秒，06 §6.5.2 `STREAM_TOKEN_AGGREGATE_MS`）。
pub const STREAM_TOKEN_AGGREGATE_MS: u64 = 100;

/// token/reasoning 单段最大字符数（06 §6.5.2 `STREAM_SEGMENT_MAX_CHARS`）。
pub const STREAM_SEGMENT_MAX_CHARS: usize = 4_096;

/// token/reasoning 单段最大 UTF-8 字节数（06 §6.5.2 `STREAM_SEGMENT_MAX_BYTES`）。
pub const STREAM_SEGMENT_MAX_BYTES: usize = 16_384;

/// 单条 `events.payload` 最大字节数（06 §6.4.2 `TRACE_PAYLOAD_MAX_BYTES`）。
pub const TRACE_PAYLOAD_MAX_BYTES: usize = 65_536;

/// v1 → v2 需要补到 `runs` 的列（architecture §5.2）。
const RUNS_V2_COLUMNS: &[(&str, &str)] = &[
    ("seed", "seed INTEGER NOT NULL DEFAULT 0"),
    ("worker_versions_json", "worker_versions_json TEXT"),
    ("model_params_json", "model_params_json TEXT"),
    ("record_mode", "record_mode TEXT"),
    (
        "killed_workers",
        "killed_workers INTEGER NOT NULL DEFAULT 0",
    ),
];

/// v2 → v3 需要补到 `runs` 的列（ISSUE-087：run 级 `global_vars` 快照，canonical JSON）。
const RUNS_V3_COLUMNS: &[(&str, &str)] = &[("global_vars_json", "global_vars_json TEXT")];

/// v3 → v4 需要补到 `gate_results` 的列（W2a / DR-005：deferred/attempt/verdict）。
const GATE_RESULTS_V4_COLUMNS: &[(&str, &str)] = &[
    (
        "deferred",
        "deferred INTEGER NOT NULL DEFAULT 0 CHECK (deferred IN (0,1))",
    ),
    ("attempt", "attempt INTEGER NOT NULL DEFAULT 0"),
    ("verdict_json", "verdict_json TEXT"),
];

/// 当前版本**基表** DDL（全部 `IF NOT EXISTS`，单事务执行；06 §6.2 + v2/v3/v4/v5 增量）。
///
/// v5/v6 新增的 `human_decisions`/`idx_human_decisions_pending` 与
/// `foreach_expansions` 单独放在 [`DDL_HUMAN_DECISIONS`]/[`DDL_FOREACH_EXPANSIONS`]：
/// 新库建库与对应版本迁移共用同一份 DDL，避免两处字面量漂移；也保证 v1→v2 的
/// 中间态不提前创建后续版本的表（按各迁移步骤的版本语义创建）。
///
/// **v6-only 的 `idx_instances_run_node_item` 不在本 DDL 内**（[`DDL_INSTANCES_V6_INDEX`]）：
/// v1→v2 步骤会对真实 v1 库执行本 DDL，而 v1 `instances` 无 `item_index` 列
/// （REV-089 C1）。
const DDL_CURRENT: &str = r#"
CREATE TABLE IF NOT EXISTS schema_migrations (
  version     INTEGER PRIMARY KEY,
  description TEXT    NOT NULL,
  applied_at  TEXT    NOT NULL
);

CREATE TABLE IF NOT EXISTS graphs (
  graph_hash  TEXT PRIMARY KEY,
  graph_name  TEXT,
  dsl_version TEXT NOT NULL,
  dsl_json    TEXT NOT NULL,
  created_at  TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS runs (
  run_id           TEXT PRIMARY KEY,
  graph_hash       TEXT NOT NULL REFERENCES graphs(graph_hash),
  status           TEXT NOT NULL
                   CHECK (status IN ('Running','Paused','Completed','Failed','Cancelled')),
  user_id          TEXT,
  started_at       TEXT NOT NULL,
  finished_at      TEXT,
  entry_inputs     TEXT,
  output_json      TEXT,
  error_code       TEXT,
  error_message    TEXT,
  owner_pid        INTEGER,
  owner_host       TEXT,
  heartbeat_at     TEXT,
  interrupted      INTEGER NOT NULL DEFAULT 0 CHECK (interrupted IN (0,1)),
  recovered_at     TEXT,
  last_seq         INTEGER NOT NULL DEFAULT 0,
  event_count      INTEGER NOT NULL DEFAULT 0,
  streaming_dropped INTEGER NOT NULL DEFAULT 0,
  abandoned_tasks  INTEGER NOT NULL DEFAULT 0,
  trace_backpressure INTEGER NOT NULL DEFAULT 0,
  seed             INTEGER NOT NULL DEFAULT 0,
  worker_versions_json TEXT,
  model_params_json TEXT,
  record_mode      TEXT,
  killed_workers   INTEGER NOT NULL DEFAULT 0,
  global_vars_json TEXT
);

CREATE INDEX IF NOT EXISTS idx_runs_status_started ON runs(status, started_at);
CREATE INDEX IF NOT EXISTS idx_runs_unfinished ON runs(status) WHERE status IN ('Running','Paused');

CREATE TABLE IF NOT EXISTS instances (
  instance_id  TEXT PRIMARY KEY,
  run_id       TEXT NOT NULL REFERENCES runs(run_id) ON DELETE CASCADE,
  node_id      TEXT NOT NULL,
  node_type    TEXT NOT NULL CHECK (node_type IN
               ('start','end','python_node','agent_call','subgraph')),
  attempt      INTEGER NOT NULL DEFAULT 0,
  item_index   INTEGER NOT NULL DEFAULT 0,
  item_key     TEXT,
  status       TEXT NOT NULL CHECK (status IN
               ('Pending','Ready','Running','WaitingHuman','Completed','Crashed','TimedOut','Cancelled','Skipped')),
  input        TEXT,
  output_json  TEXT,
  error        TEXT,
  error_code   TEXT,
  created_seq  INTEGER,
  started_at   TEXT,
  finished_at  TEXT,
  finished_seq INTEGER,
  UNIQUE (run_id, node_id, attempt, item_index)
);

CREATE INDEX IF NOT EXISTS idx_instances_run_node   ON instances(run_id, node_id);
CREATE INDEX IF NOT EXISTS idx_instances_run_status ON instances(run_id, status);
CREATE INDEX IF NOT EXISTS idx_instances_error_code ON instances(error_code) WHERE error_code IS NOT NULL;

CREATE TABLE IF NOT EXISTS events (
  seq         INTEGER PRIMARY KEY AUTOINCREMENT,
  run_id      TEXT NOT NULL REFERENCES runs(run_id) ON DELETE CASCADE,
  kind        TEXT NOT NULL CHECK (kind IN
              ('RunStarted','Transition','GateResult','NodeTimedOut',
               'NodeAbandoned','Streaming','RunFinished')),
  severity    TEXT NOT NULL DEFAULT 'info' CHECK (severity IN ('info','warn','error')),
  at          TEXT NOT NULL,
  instance_id TEXT,
  node_id     TEXT,
  payload     TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_events_run_seq  ON events(run_id, seq);
CREATE INDEX IF NOT EXISTS idx_events_kind     ON events(kind, seq);
CREATE INDEX IF NOT EXISTS idx_events_instance ON events(instance_id, seq) WHERE instance_id IS NOT NULL;

CREATE TABLE IF NOT EXISTS transitions (
  transition_id INTEGER PRIMARY KEY AUTOINCREMENT,
  seq           INTEGER NOT NULL UNIQUE REFERENCES events(seq) ON DELETE CASCADE,
  run_id        TEXT NOT NULL,
  instance_id   TEXT NOT NULL,
  node_id       TEXT NOT NULL,
  attempt       INTEGER NOT NULL,
  from_status   TEXT NOT NULL CHECK (from_status IN
                ('Pending','Ready','Running','WaitingHuman','Completed','Crashed','TimedOut','Cancelled','Skipped')),
  to_status     TEXT NOT NULL CHECK (to_status IN
                ('Pending','Ready','Running','WaitingHuman','Completed','Crashed','TimedOut','Cancelled','Skipped')),
  reason        TEXT,
  error_code    TEXT,
  error         TEXT,
  at            TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_transitions_run_seq    ON transitions(run_id, seq);
CREATE INDEX IF NOT EXISTS idx_transitions_instance   ON transitions(instance_id, seq);
CREATE INDEX IF NOT EXISTS idx_transitions_run_status ON transitions(run_id, to_status);

CREATE TABLE IF NOT EXISTS gate_results (
  gate_result_id INTEGER PRIMARY KEY AUTOINCREMENT,
  seq            INTEGER NOT NULL UNIQUE REFERENCES events(seq) ON DELETE CASCADE,
  run_id         TEXT NOT NULL,
  instance_id    TEXT NOT NULL,
  node_id        TEXT NOT NULL,
  edge_index     INTEGER NOT NULL,
  to_node        TEXT NOT NULL,
  to_port        TEXT NOT NULL,
  gate_fn        TEXT,
  passed         INTEGER NOT NULL CHECK (passed IN (0,1)),
  error          TEXT,
  at             TEXT NOT NULL,
  deferred       INTEGER NOT NULL DEFAULT 0 CHECK (deferred IN (0,1)),
  attempt        INTEGER NOT NULL DEFAULT 0,
  verdict_json   TEXT
);

CREATE INDEX IF NOT EXISTS idx_gate_run_seq  ON gate_results(run_id, seq);
CREATE INDEX IF NOT EXISTS idx_gate_run_edge ON gate_results(run_id, edge_index);
CREATE INDEX IF NOT EXISTS idx_gate_passed   ON gate_results(run_id, passed);

CREATE TABLE IF NOT EXISTS streaming_events (
  streaming_event_id INTEGER PRIMARY KEY AUTOINCREMENT,
  seq            INTEGER NOT NULL UNIQUE REFERENCES events(seq) ON DELETE CASCADE,
  run_id         TEXT NOT NULL,
  instance_id    TEXT NOT NULL,
  kind           TEXT NOT NULL CHECK (kind IN
                 ('token','reasoning','tool_call_start','tool_call_end','progress')),
  segment_index  INTEGER NOT NULL,
  text           TEXT,
  text_chars     INTEGER,
  text_bytes     INTEGER,
  truncated      INTEGER NOT NULL DEFAULT 0 CHECK (truncated IN (0,1)),
  delta_count    INTEGER,
  call_id        TEXT,
  tool_name      TEXT,
  ok             INTEGER,
  summary        TEXT,
  percent        INTEGER,
  first_at       TEXT NOT NULL,
  last_at        TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_stream_run_seq   ON streaming_events(run_id, seq);
CREATE INDEX IF NOT EXISTS idx_stream_instance  ON streaming_events(run_id, instance_id, segment_index);
CREATE INDEX IF NOT EXISTS idx_stream_tool      ON streaming_events(run_id, call_id) WHERE call_id IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_stream_kind      ON streaming_events(run_id, kind, segment_index);

CREATE TABLE IF NOT EXISTS recorded_outputs (
  recorded_output_id INTEGER PRIMARY KEY AUTOINCREMENT,
  run_id        TEXT NOT NULL REFERENCES runs(run_id) ON DELETE CASCADE,
  instance_id   TEXT NOT NULL,
  node_id       TEXT NOT NULL,
  attempt       INTEGER NOT NULL,
  output_hash   TEXT NOT NULL,
  output_json   TEXT,
  recorded_at   TEXT NOT NULL,
  UNIQUE (run_id, instance_id, attempt)
);

CREATE INDEX IF NOT EXISTS idx_recorded_outputs_run ON recorded_outputs(run_id);

CREATE TABLE IF NOT EXISTS llm_calls (
  llm_call_id   INTEGER PRIMARY KEY AUTOINCREMENT,
  run_id        TEXT NOT NULL REFERENCES runs(run_id) ON DELETE CASCADE,
  instance_id   TEXT NOT NULL,
  call_index    INTEGER NOT NULL,
  request_json  TEXT NOT NULL,
  response_json TEXT,
  status        TEXT NOT NULL,
  recorded_at   TEXT NOT NULL,
  UNIQUE (run_id, instance_id, call_index)
);

CREATE INDEX IF NOT EXISTS idx_llm_calls_run ON llm_calls(run_id);
"#;

/// v5 `human_decisions` 表 + pending 部分索引（06 §6.2.14；DR-006 §5.1）。
///
/// 新库建库与 v4→v5 迁移共用本 DDL（`IF NOT EXISTS` 使迁移可安全重入）。
/// 写者模型（C13）：`decide` 独立 RW 连接单事务 INSERT、run consumer 独立 RW
/// 短事务只更新 `consumed_at`；[`crate::TraceWriter`] **不写**本表。
const DDL_HUMAN_DECISIONS: &str = r#"
CREATE TABLE IF NOT EXISTS human_decisions (
  run_id       TEXT NOT NULL REFERENCES runs(run_id) ON DELETE CASCADE,
  instance_id  TEXT NOT NULL REFERENCES instances(instance_id) ON DELETE CASCADE,
  decision     TEXT NOT NULL CHECK (decision IN ('approved','rejected')),
  approver     TEXT NOT NULL,
  reason       TEXT,
  decision_id  TEXT NOT NULL UNIQUE,
  decided_at   TEXT NOT NULL,
  consumed_at  TEXT,
  PRIMARY KEY (run_id, instance_id)
);

CREATE INDEX IF NOT EXISTS idx_human_decisions_pending
  ON human_decisions(run_id, consumed_at) WHERE consumed_at IS NULL;
"#;

/// v4→v5 重建 `instances`（SQLite 12 步法；status CHECK 增 `WaitingHuman`）。
///
/// 列清单**显式**复制（禁止 `SELECT *`，DR-006 §6.2/06 §6.2.5）；`DROP TABLE`
/// 后按原定义重建 3 个索引。重建在 `human_decisions` 之前完成（FK 指向新表）。
const REBUILD_INSTANCES_V5: &str = r#"
CREATE TABLE instances_v5 (
  instance_id  TEXT PRIMARY KEY,
  run_id       TEXT NOT NULL REFERENCES runs(run_id) ON DELETE CASCADE,
  node_id      TEXT NOT NULL,
  node_type    TEXT NOT NULL CHECK (node_type IN
               ('start','end','python_node','agent_call','subgraph')),
  attempt      INTEGER NOT NULL DEFAULT 0,
  status       TEXT NOT NULL CHECK (status IN
               ('Pending','Ready','Running','WaitingHuman','Completed','Crashed','TimedOut','Cancelled','Skipped')),
  input        TEXT,
  output_json  TEXT,
  error        TEXT,
  error_code   TEXT,
  created_seq  INTEGER,
  started_at   TEXT,
  finished_at  TEXT,
  finished_seq INTEGER,
  UNIQUE (run_id, node_id, attempt)
);

INSERT INTO instances_v5 (
  instance_id, run_id, node_id, node_type, attempt, status, input, output_json,
  error, error_code, created_seq, started_at, finished_at, finished_seq
)
SELECT
  instance_id, run_id, node_id, node_type, attempt, status, input, output_json,
  error, error_code, created_seq, started_at, finished_at, finished_seq
FROM instances;

DROP TABLE instances;
ALTER TABLE instances_v5 RENAME TO instances;

CREATE INDEX IF NOT EXISTS idx_instances_run_node   ON instances(run_id, node_id);
CREATE INDEX IF NOT EXISTS idx_instances_run_status ON instances(run_id, status);
CREATE INDEX IF NOT EXISTS idx_instances_error_code ON instances(error_code) WHERE error_code IS NOT NULL;
"#;

/// v4→v5 重建 `transitions`（SQLite 12 步法；`from_status`/`to_status` CHECK 各增
/// `WaitingHuman`）。列清单显式复制，重建 3 个索引（06 §6.2.7）。
const REBUILD_TRANSITIONS_V5: &str = r#"
CREATE TABLE transitions_v5 (
  transition_id INTEGER PRIMARY KEY AUTOINCREMENT,
  seq           INTEGER NOT NULL UNIQUE REFERENCES events(seq) ON DELETE CASCADE,
  run_id        TEXT NOT NULL,
  instance_id   TEXT NOT NULL,
  node_id       TEXT NOT NULL,
  attempt       INTEGER NOT NULL,
  from_status   TEXT NOT NULL CHECK (from_status IN
                ('Pending','Ready','Running','WaitingHuman','Completed','Crashed','TimedOut','Cancelled','Skipped')),
  to_status     TEXT NOT NULL CHECK (to_status IN
                ('Pending','Ready','Running','WaitingHuman','Completed','Crashed','TimedOut','Cancelled','Skipped')),
  reason        TEXT,
  error_code    TEXT,
  error         TEXT,
  at            TEXT NOT NULL
);

INSERT INTO transitions_v5 (
  transition_id, seq, run_id, instance_id, node_id, attempt,
  from_status, to_status, reason, error_code, error, at
)
SELECT
  transition_id, seq, run_id, instance_id, node_id, attempt,
  from_status, to_status, reason, error_code, error, at
FROM transitions;

DROP TABLE transitions;
ALTER TABLE transitions_v5 RENAME TO transitions;

CREATE INDEX IF NOT EXISTS idx_transitions_run_seq    ON transitions(run_id, seq);
CREATE INDEX IF NOT EXISTS idx_transitions_instance   ON transitions(instance_id, seq);
CREATE INDEX IF NOT EXISTS idx_transitions_run_status ON transitions(run_id, to_status);
"#;

/// v6-only `instances` item 定位索引（DR-007 §8.2）。
///
/// **不得放进 [`DDL_CURRENT`]**：`migrate_step` 的 v1→v2 步骤会对**真实 v1 库**
/// 执行 `DDL_CURRENT`，而 v1 `instances` 没有 `item_index` 列 → 建索引会
/// `no such column`（REV-089 C1）。新库在 [`create_current`] 建完全部列后单独执行；
/// v5→v6 迁移由 [`REBUILD_INSTANCES_V6`] 重建同一索引，保证 fresh/migrated 一致。
const DDL_INSTANCES_V6_INDEX: &str = "CREATE INDEX IF NOT EXISTS idx_instances_run_node_item \
     ON instances(run_id, node_id, item_index);";

/// v6 `foreach_expansions` 展开事实表 + `run_id` 索引（DR-007 §8.3 / 06 §6.2.15）。
///
/// 写入者是 [`crate::TraceWriter`]（内核执行事实，与 events/instances 同一写线程；
/// C13 不适用——这不是决策表）。每次展开一行（`item_count=0` 也写）；展开校验
/// 失败（非数组/超限/key 重复）**不写** expansion。新库与 v5→v6 迁移共用本 DDL。
///
/// `keys_json`：`item_key` 数组；`NULL` 键以 `{"$index": i}` 结构化对象表示
/// （真实 string key 保持原字符串，不与 `{"$index"}` 撞表示）。
const DDL_FOREACH_EXPANSIONS: &str = r#"
CREATE TABLE IF NOT EXISTS foreach_expansions (
  run_id             TEXT NOT NULL REFERENCES runs(run_id) ON DELETE CASCADE,
  expansion_id       TEXT PRIMARY KEY,
  node_id            TEXT NOT NULL,
  source_instance_id TEXT,
  item_count         INTEGER NOT NULL CHECK (item_count >= 0),
  keys_json          TEXT,
  at                 TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_foreach_expansions_run ON foreach_expansions(run_id);
"#;

/// v5→v6 重建 `instances`（SQLite 12 步法；DR-007 §8.2 / 06 §6.2.5）。
///
/// 新列 `item_index INTEGER NOT NULL DEFAULT 0`、`item_key TEXT`；唯一键由
/// `(run_id,node_id,attempt)` 改为 `(run_id,node_id,attempt,item_index)`。
/// 显式列清单复制：旧行一律 `item_index=0`、`item_key=NULL`；重建 3 索引 +
/// 新增 `idx_instances_run_node_item`（与新库 DDL 保持同一索引集合）。
const REBUILD_INSTANCES_V6: &str = r#"
CREATE TABLE instances_v6 (
  instance_id  TEXT PRIMARY KEY,
  run_id       TEXT NOT NULL REFERENCES runs(run_id) ON DELETE CASCADE,
  node_id      TEXT NOT NULL,
  node_type    TEXT NOT NULL CHECK (node_type IN
               ('start','end','python_node','agent_call','subgraph')),
  attempt      INTEGER NOT NULL DEFAULT 0,
  item_index   INTEGER NOT NULL DEFAULT 0,
  item_key     TEXT,
  status       TEXT NOT NULL CHECK (status IN
               ('Pending','Ready','Running','WaitingHuman','Completed','Crashed','TimedOut','Cancelled','Skipped')),
  input        TEXT,
  output_json  TEXT,
  error        TEXT,
  error_code   TEXT,
  created_seq  INTEGER,
  started_at   TEXT,
  finished_at  TEXT,
  finished_seq INTEGER,
  UNIQUE (run_id, node_id, attempt, item_index)
);

INSERT INTO instances_v6 (
  instance_id, run_id, node_id, node_type, attempt, item_index, item_key, status,
  input, output_json, error, error_code, created_seq, started_at, finished_at,
  finished_seq
)
SELECT
  instance_id, run_id, node_id, node_type, attempt, 0, NULL, status,
  input, output_json, error, error_code, created_seq, started_at, finished_at,
  finished_seq
FROM instances;

DROP TABLE instances;
ALTER TABLE instances_v6 RENAME TO instances;

CREATE INDEX IF NOT EXISTS idx_instances_run_node   ON instances(run_id, node_id);
CREATE INDEX IF NOT EXISTS idx_instances_run_status ON instances(run_id, status);
CREATE INDEX IF NOT EXISTS idx_instances_error_code ON instances(error_code) WHERE error_code IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_instances_run_node_item ON instances(run_id, node_id, item_index);
"#;

/// 打开（必要时创建）写库，完成版本检查与迁移并应用写连接 PRAGMA。
pub(crate) fn open_writer_connection(path: &Path) -> Result<Connection, TraceError> {
    ensure_parent_dir(path)?;
    let mut conn = Connection::open(path).map_err(db_err)?;
    let version = user_version(&conn)?;
    if version > TRACE_SCHEMA_VERSION {
        return Err(TraceError::SchemaTooNew {
            found: version,
            supported: TRACE_SCHEMA_VERSION,
        });
    }
    if version < 0 {
        return Err(TraceError::Storage(format!(
            "invalid schema version {version} in {}",
            path.display()
        )));
    }
    apply_writer_pragmas(&conn)?;
    if version == 0 {
        create_current(&mut conn)?;
    } else {
        // 逐版本迁移（06 §6.7）：每次迁移前 `VACUUM INTO` 备份；单版本单事务；
        // 失败则恢复备份并把存储错误交给调用方（CLI 映射 exit 4）。
        let mut current = version;
        while current < TRACE_SCHEMA_VERSION {
            let backup = backup_database(&conn, path, current)?;
            if let Err(error) = migrate_step(&mut conn, current) {
                drop(conn);
                if let Err(restore_error) = restore_backup(&backup, path) {
                    return Err(TraceError::Storage(format!(
                        "migration from v{current} failed ({error}); \
                         backup restore also failed ({restore_error})"
                    )));
                }
                return Err(error);
            }
            current += 1;
        }
    }
    Ok(conn)
}

/// 打开只读连接（`SQLITE_OPEN_READ_ONLY`；不创建、不迁移、不标定，06 §6.6）。
pub(crate) fn open_readonly_connection(path: &Path) -> Result<Connection, TraceError> {
    open_readonly_with_busy_timeout(path, DB_BUSY_TIMEOUT_MS)
}

/// 以指定 `busy_timeout` 打开只读连接（decision consumer 的 RO 轮询用
/// [`DECISION_POLL_BUSY_TIMEOUT_MS`]；其余只读方用 [`DB_BUSY_TIMEOUT_MS`]）。
pub(crate) fn open_readonly_with_busy_timeout(
    path: &Path,
    busy_timeout_ms: u32,
) -> Result<Connection, TraceError> {
    ensure_existing_file(path)?;
    let conn =
        Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY).map_err(|err| {
            TraceError::Storage(format!("database not found: {} ({err})", path.display()))
        })?;
    conn.busy_timeout(Duration::from_millis(u64::from(busy_timeout_ms)))
        .map_err(db_err)?;
    conn.execute_batch("PRAGMA query_only = ON; PRAGMA temp_store = MEMORY;")
        .map_err(db_err)?;
    ensure_current_version(&conn)?;
    ensure_trace_schema(&conn, path)?;
    Ok(conn)
}

/// 打开**已存在**库的读写连接，**不创建、不迁移**（decision 写者用；DR-006 §5.4）。
///
/// 语义：`decide`/run consumer 只作为 trace 写者之外的显式第二/第三写者（C13），
/// 迁移是 run 写端职责——版本 `<5` 明确报错（提示先 `zgraph run` 迁移），`>5`
/// 报 [`TraceError::SchemaTooNew`]；库不存在时报错且不创建文件。
pub(crate) fn open_existing_readwrite_connection(
    path: &Path,
    busy_timeout_ms: u32,
) -> Result<Connection, TraceError> {
    ensure_existing_file(path)?;
    let conn =
        Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_WRITE).map_err(|err| {
            TraceError::Storage(format!("database not found: {} ({err})", path.display()))
        })?;
    conn.busy_timeout(Duration::from_millis(u64::from(busy_timeout_ms)))
        .map_err(db_err)?;
    conn.execute_batch("PRAGMA foreign_keys = ON; PRAGMA temp_store = MEMORY;")
        .map_err(db_err)?;
    ensure_current_version(&conn)?;
    ensure_trace_schema(&conn, path)?;
    Ok(conn)
}

/// 写者/decision 连接共用的「库文件必须已存在」前置检查（不创建空库）。
fn ensure_existing_file(path: &Path) -> Result<(), TraceError> {
    let metadata = std::fs::metadata(path).map_err(|err| match err.kind() {
        std::io::ErrorKind::NotFound => {
            TraceError::Storage(format!("database not found: {}", path.display()))
        }
        _ => TraceError::Storage(format!("cannot stat {}: {err}", path.display())),
    })?;
    if metadata.is_dir() {
        return Err(TraceError::Storage(format!(
            "database path is a directory: {}",
            path.display()
        )));
    }
    Ok(())
}

/// 读取 `PRAGMA user_version`。
pub(crate) fn user_version(conn: &Connection) -> Result<i64, TraceError> {
    conn.query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))
        .map_err(db_err)
}

/// 当前 UTC RFC3339 毫秒字符串（`Z` 结尾；写线程仅用于 heartbeat 与迁移记录）。
pub(crate) fn now_rfc3339_millis() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

/// 把 rusqlite 错误收敛为 [`TraceError::Storage`]。
pub(crate) fn db_err(err: rusqlite::Error) -> TraceError {
    TraceError::Storage(err.to_string())
}

/// 新库：单事务建当前版本 DDL，并把基线版本链写入 `schema_migrations`。
fn create_current(conn: &mut Connection) -> Result<(), TraceError> {
    let tx = begin_immediate(conn)?;
    tx.execute_batch(DDL_CURRENT).map_err(db_err)?;
    // v6-only 索引：DDL_CURRENT 不含它（见 `DDL_INSTANCES_V6_INDEX` 文档）。
    tx.execute_batch(DDL_INSTANCES_V6_INDEX).map_err(db_err)?;
    tx.execute_batch(DDL_HUMAN_DECISIONS).map_err(db_err)?;
    tx.execute_batch(DDL_FOREACH_EXPANSIONS).map_err(db_err)?;
    for version in 1..=TRACE_SCHEMA_VERSION {
        record_migration(&tx, version)?;
    }
    tx.commit().map_err(db_err)?;
    Ok(())
}

/// 单版本迁移（`from` → `from+1`；单事务；06 §6.7）。
///
/// `from == 5`（v5→v6 重建 `instances`）期间临时关闭 FK 校验：`human_decisions`
/// 以 `ON DELETE CASCADE` 引用 `instances`，SQLite 12 步法的 `DROP TABLE` 会级联
/// 删除全部决策行（DR-007 §8.2 要求 human_decisions 结构/数据不变）。`PRAGMA
/// foreign_keys` 在事务内无效，故在事务前后切换；失败路径连接随后被丢弃/回滚。
fn migrate_step(conn: &mut Connection, from: i64) -> Result<(), TraceError> {
    if from == 5 {
        with_foreign_keys_disabled(conn, |conn| migrate_step_inner(conn, from))
    } else {
        migrate_step_inner(conn, from)
    }
}

/// 在 FK 校验关闭的窗口内执行 `run`，**无论成功/失败**随后恢复
/// `PRAGMA foreign_keys = ON`（`PRAGMA` 在事务内设置无效，故必须事务外切换；
/// DR-007 §8.2 的重建 `instances` 需要它，否则 `human_decisions` 会被级联删除）。
fn with_foreign_keys_disabled<T>(
    conn: &mut Connection,
    run: impl FnOnce(&mut Connection) -> Result<T, TraceError>,
) -> Result<T, TraceError> {
    conn.execute_batch("PRAGMA foreign_keys = OFF;")
        .map_err(db_err)?;
    let result = run(conn);
    let restore = conn
        .execute_batch("PRAGMA foreign_keys = ON;")
        .map_err(db_err);
    match (result, restore) {
        (Ok(value), Ok(())) => Ok(value),
        // 迁移错误优先返回（恢复失败时连接将被丢弃/由调用方恢复备份）。
        (Err(migration_error), _) => Err(migration_error),
        (Ok(_), Err(restore_error)) => Err(restore_error),
    }
}

/// `migrate_step` 的单事务主体（版本分支）。
fn migrate_step_inner(conn: &mut Connection, from: i64) -> Result<(), TraceError> {
    let tx = begin_immediate(conn)?;
    match from {
        1 => {
            // `DDL_CURRENT` 建当前版本的基表（v1 库缺表全量补齐）；v5 的
            // `human_decisions` 不在这里提前创建，而由 4→5 步骤按 FK 顺序建。
            tx.execute_batch(DDL_CURRENT).map_err(db_err)?;
            add_missing_columns(&tx, "runs", RUNS_V2_COLUMNS)?;
            record_migration(&tx, 2)?;
        }
        2 => {
            add_missing_columns(&tx, "runs", RUNS_V3_COLUMNS)?;
            record_migration(&tx, 3)?;
        }
        3 => {
            add_missing_columns(&tx, "gate_results", GATE_RESULTS_V4_COLUMNS)?;
            record_migration(&tx, 4)?;
        }
        4 => {
            // v4→v5（DR-006 §6.2 / 06 §6.2.7）：单版本单事务内重建两表。
            // FK 顺序：先删可能残留的 human_decisions（无 → 被引用的 instances 才可
            // 安全 DROP），再重建 instances / transitions，最后建 human_decisions
            // （其 FK 指向新 instances）。步骤 1→2 不再提前创建该表，这里的
            // `DROP TABLE IF EXISTS` 仅为重入健壮性。
            tx.execute_batch("DROP TABLE IF EXISTS human_decisions;")
                .map_err(db_err)?;
            tx.execute_batch(REBUILD_INSTANCES_V5).map_err(db_err)?;
            tx.execute_batch(REBUILD_TRANSITIONS_V5).map_err(db_err)?;
            tx.execute_batch(DDL_HUMAN_DECISIONS).map_err(db_err)?;
            record_migration(&tx, 5)?;
        }
        5 => {
            // v5→v6（DR-007 §8.2 / 06 §6.2.7）：单版本单事务。
            // FK 顺序：先删可能残留的 foreach_expansions（其 FK 指向 runs，
            // 不约束 instances；重建前删除仅为重入健壮性），再重建 instances，
            // 最后建 expansion 表。旧行 item_index=0 / item_key=NULL。
            tx.execute_batch("DROP TABLE IF EXISTS foreach_expansions;")
                .map_err(db_err)?;
            tx.execute_batch(REBUILD_INSTANCES_V6).map_err(db_err)?;
            tx.execute_batch(DDL_FOREACH_EXPANSIONS).map_err(db_err)?;
            record_migration(&tx, 6)?;
        }
        other => {
            return Err(TraceError::Storage(format!(
                "no migration step from schema version {other}"
            )))
        }
    }
    tx.commit().map_err(db_err)?;
    Ok(())
}

/// 补齐 `table` 上缺失的列（`ALTER TABLE ... ADD COLUMN`，可重复执行）。
fn add_missing_columns(
    tx: &Transaction<'_>,
    table: &str,
    columns: &[(&str, &str)],
) -> Result<(), TraceError> {
    let existing = table_columns(tx, table)?;
    for (name, definition) in columns {
        if !existing.iter().any(|column| column == name) {
            let sql = format!("ALTER TABLE {table} ADD COLUMN {definition};");
            tx.execute_batch(&sql).map_err(db_err)?;
        }
    }
    Ok(())
}

fn begin_immediate(conn: &mut Connection) -> Result<Transaction<'_>, TraceError> {
    conn.transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(db_err)
}

/// 迁移审计行 + `user_version`（同一事务；`user_version` 随回滚撤销）。
fn record_migration(tx: &Transaction<'_>, version: i64) -> Result<(), TraceError> {
    tx.execute(
        "INSERT OR REPLACE INTO schema_migrations (version, description, applied_at) \
         VALUES (?1, ?2, ?3)",
        params![
            version,
            migration_description(version),
            now_rfc3339_millis()
        ],
    )
    .map_err(db_err)?;
    tx.execute_batch(&format!("PRAGMA user_version = {version};"))
        .map_err(db_err)
}

/// 版本 → `schema_migrations.description`（审计可读）。
fn migration_description(version: i64) -> &'static str {
    match version {
        1 => "v1: base trace schema",
        2 => "v2: reproducibility columns/tables",
        3 => "v3: runs.global_vars_json (ISSUE-087)",
        4 => "v4: gate_results deferred/attempt/verdict (W2a)",
        5 => "v5: WaitingHuman statuses + human_decisions (W3a)",
        6 => "v6: instances item_index/item_key + foreach_expansions (W4a)",
        _ => "unknown migration",
    }
}

/// 迁移前备份：`VACUUM INTO '<db>.bak.v{old}-{ts}'`（06 §6.7）。
///
/// 备份保留策略：不自动删除（最近 3 份之外也不清理，避免误删用户数据的唯一快照；
/// 06 §6.7「备份保留」的清理属运维动作）。
fn backup_database(conn: &Connection, db: &Path, old_version: i64) -> Result<PathBuf, TraceError> {
    let timestamp = chrono::Utc::now().format("%Y%m%dT%H%M%S%.3fZ");
    let backup = PathBuf::from(format!(
        "{}.bak.v{}-{}",
        db.display(),
        old_version,
        timestamp
    ));
    if backup.exists() {
        return Err(TraceError::Storage(format!(
            "migration backup already exists: {}",
            backup.display()
        )));
    }
    let literal = backup.to_string_lossy().replace('\'', "''");
    conn.execute_batch(&format!("VACUUM INTO '{literal}';"))
        .map_err(db_err)?;
    // `VACUUM INTO` 不保证保留 DB 头里的 `user_version`；显式写回旧版本，
    // 使备份可直接用于「恢复回旧版本」（06 §6.7 步骤 6）。
    let backup_conn = Connection::open(&backup).map_err(db_err)?;
    backup_conn
        .execute_batch(&format!("PRAGMA user_version = {old_version};"))
        .map_err(db_err)?;
    let recorded: i64 = backup_conn
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .map_err(db_err)?;
    if recorded != old_version {
        return Err(TraceError::Storage(format!(
            "migration backup {} has user_version={recorded}, expected {old_version}",
            backup.display()
        )));
    }
    Ok(backup)
}

/// 迁移失败：清掉半迁移库与残留 WAL/SHM，再用备份覆盖恢复（06 §6.7 步骤 6）。
fn restore_backup(backup: &Path, db: &Path) -> Result<(), TraceError> {
    for suffix in ["-wal", "-shm"] {
        let sidecar = PathBuf::from(format!("{}{suffix}", db.display()));
        match std::fs::remove_file(&sidecar) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(TraceError::Storage(format!(
                    "cannot remove {} during migration restore: {error}",
                    sidecar.display()
                )))
            }
        }
    }
    std::fs::copy(backup, db).map_err(|error| {
        TraceError::Storage(format!(
            "cannot restore migration backup {} to {}: {error}",
            backup.display(),
            db.display()
        ))
    })?;
    Ok(())
}

fn apply_writer_pragmas(conn: &Connection) -> Result<(), TraceError> {
    conn.busy_timeout(Duration::from_millis(u64::from(DB_BUSY_TIMEOUT_MS)))
        .map_err(db_err)?;
    // journal_mode 返回一行；用 query_row 显式消费结果。
    conn.query_row("PRAGMA journal_mode = WAL", [], |_row| Ok(()))
        .map_err(db_err)?;
    conn.execute_batch(
        "PRAGMA synchronous = NORMAL;\n\
         PRAGMA foreign_keys = ON;\n\
         PRAGMA wal_autocheckpoint = 1000;\n\
         PRAGMA temp_store = MEMORY;",
    )
    .map_err(db_err)?;
    Ok(())
}

/// 只读/decision 连接只接受**恰好**当前版本：超前拒绝（06 §6.7），落后明确报错且
/// **不迁移**（迁移是 run 写端职责；DR-006 §5.4）。
fn ensure_current_version(conn: &Connection) -> Result<(), TraceError> {
    let found = user_version(conn)?;
    if found > TRACE_SCHEMA_VERSION {
        return Err(TraceError::SchemaTooNew {
            found,
            supported: TRACE_SCHEMA_VERSION,
        });
    }
    if found < TRACE_SCHEMA_VERSION {
        return Err(TraceError::Storage(format!(
            "unsupported schema version={found}, expected {TRACE_SCHEMA_VERSION}"
        )));
    }
    Ok(())
}

/// decision 写者要求 v5 的 `human_decisions` 表存在（正常 v5 库由迁移保证；
/// 损坏库给出明确存储错误而非运行时 "no such table"）。
pub(crate) fn ensure_human_decisions_schema(conn: &Connection) -> Result<(), TraceError> {
    let count: i64 = conn
        .query_row(
            "SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name = 'human_decisions'",
            [],
            |row| row.get(0),
        )
        .map_err(db_err)?;
    if count == 0 {
        return Err(TraceError::Storage(
            "missing human_decisions table in schema v5 database".to_string(),
        ));
    }
    Ok(())
}

fn ensure_trace_schema(conn: &Connection, path: &Path) -> Result<(), TraceError> {
    let count: i64 = conn
        .query_row(
            "SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name = 'runs'",
            [],
            |row| row.get(0),
        )
        .map_err(db_err)?;
    if count == 0 {
        return Err(TraceError::Storage(format!(
            "database not found: no trace schema in {}",
            path.display()
        )));
    }
    Ok(())
}

fn ensure_parent_dir(path: &Path) -> Result<(), TraceError> {
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent).map_err(|err| {
            TraceError::Storage(format!(
                "cannot create database directory {}: {err}",
                parent.display()
            ))
        })?;
    }
    Ok(())
}

fn table_columns(conn: &Connection, table: &str) -> Result<Vec<String>, TraceError> {
    let sql = format!("PRAGMA table_info({table})");
    let mut stmt = conn.prepare(&sql).map_err(db_err)?;
    let rows = stmt
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(db_err)?;
    let mut columns = Vec::new();
    for row in rows {
        columns.push(row.map_err(db_err)?);
    }
    Ok(columns)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn foreign_keys_enabled(conn: &Connection) -> bool {
        conn.query_row("PRAGMA foreign_keys", [], |row| row.get::<_, i64>(0))
            .expect("read foreign_keys")
            == 1
    }

    #[test]
    fn foreign_keys_are_disabled_inside_and_restored_after_the_window() {
        let mut conn = Connection::open_in_memory().expect("open memory db");
        conn.execute_batch("PRAGMA foreign_keys = ON;")
            .expect("enable foreign keys");
        assert!(foreign_keys_enabled(&conn), "前置：FK 已开启");

        let outcome = with_foreign_keys_disabled(&mut conn, |conn| {
            assert!(
                !foreign_keys_enabled(conn),
                "重建窗口内 FK 必须关闭（否则 DROP TABLE 会级联删除 human_decisions）"
            );
            Ok(7)
        })
        .expect("helper succeeds");
        assert_eq!(outcome, 7);
        assert!(
            foreign_keys_enabled(&conn),
            "迁移窗口结束后 PRAGMA foreign_keys 必须恢复 ON"
        );
    }

    #[test]
    fn foreign_keys_are_restored_even_when_the_window_fails() {
        let mut conn = Connection::open_in_memory().expect("open memory db");
        conn.execute_batch("PRAGMA foreign_keys = ON;")
            .expect("enable foreign keys");

        let error = with_foreign_keys_disabled(&mut conn, |_conn| -> Result<(), TraceError> {
            Err(TraceError::Storage("injected window failure".to_string()))
        })
        .expect_err("injected failure must surface");
        assert!(
            matches!(&error, TraceError::Storage(message) if message.contains("injected window failure")),
            "unexpected error: {error:?}"
        );
        assert!(
            foreign_keys_enabled(&conn),
            "失败路径同样必须恢复 PRAGMA foreign_keys = ON"
        );
    }
}
