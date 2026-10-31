//! 人工决策表 `human_decisions`（schema v5 / DR-006 §5）的读写 API。
//!
//! 语义依据：`doc/decisions/DR-006-w3a-human-decide-freeze.md` §5（表与写连接模型
//! C13）与 `doc/spec-v1.2/06-存储与可观测.md` §6.2.14/§6.6。
//!
//! # 写者模型（C13 冻结）
//!
//! - [`DecisionStore`]：`decide` CLI 用。独立 RW 连接、**单事务 INSERT**、
//!   `busy_timeout=5000ms`、`SQLITE_BUSY` 重试 3 次；唯一键
//!   `(run_id, instance_id)`（或 `decision_id` UNIQUE）冲突映射为
//!   [`DecisionError::Conflict`]（decide exit 5，**不是**幂等成功）。`precheck`
//!   在**同一 RO 快照**按 DR-006 §4.5 C3 顺序返回「决策行是否存在 / run 状态 /
//!   实例状态」，供 CLI 判定退出码。
//! - [`DecisionConsumer`]：run 进程用。RO 轮询连接（`busy_timeout=1000ms`）读
//!   [`DecisionConsumer::pending`]；独立 RW 连接短事务 [`DecisionConsumer::mark_consumed`]
//!   （条件 `consumed_at IS NULL`，`busy_timeout=5000ms`），已消费行不再更新（幂等）。
//! - [`TraceWriter`](crate::TraceWriter) **不暴露**本表写入口（C13；事件表唯一写连接
//!   不变式只约束事件表）。
//!
//! # 打开语义
//!
//! 两个写者都要求**已存在**的 v5 库：不创建空库、不迁移（迁移是 run 写端职责；
//! DR-006 §5.4）。`user_version < 5` → [`TraceError::Storage`]（`unsupported schema
//! version=N, expected 5`）；`> 5` → [`TraceError::SchemaTooNew`]；库不存在 → 存储错误。
//!
//! # 状态形状
//!
//! `decision`/`approver`/`decided_at` 均为字符串（与 `events.at`/`transitions.at`
//! 同口径）；`decision` 的闭集 `approved|rejected` 由表 CHECK 与 CLI 参数校验双重
//! 保证（本层不重复定义 kernel 的 `HumanDecision` 枚举，避免跨 crate 依赖）。

use std::path::{Path, PathBuf};

use rusqlite::{params, Connection, ErrorCode, TransactionBehavior};
use thiserror::Error;

use crate::error::TraceError;
use crate::model::{InstanceStatus, RunStatus};
use crate::reader::{parse_run_status, parse_status};
use crate::schema::{
    self, db_err, ensure_human_decisions_schema, DB_BUSY_TIMEOUT_MS, DB_WRITE_RETRY,
    DECISION_POLL_BUSY_TIMEOUT_MS,
};
use crate::writer::tx::{is_busy, sleep_before_retry};

/// 决策行的读取形状（DR-006 §6.3 / 06 §6.2.14）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HumanDecisionRow {
    /// 所属 run。
    pub run_id: String,
    /// 决策绑定的实例。
    pub instance_id: String,
    /// `approved` | `rejected`（表 CHECK 闭集）。
    pub decision: String,
    /// 决策人（必填非空，由 CLI 校验）。
    pub approver: String,
    /// 决策理由（可空）。
    pub reason: Option<String>,
    /// UUIDv7，仅审计（不参与唯一/幂等键）。
    pub decision_id: String,
    /// 决策写入时刻（UTC RFC3339 毫秒 Z）。
    pub decided_at: String,
    /// 被 run consumer 消费的时刻（`None` = 未消费，W3a 不清理，归 W3b）。
    pub consumed_at: Option<String>,
}

/// 待插入的决策行（`decide` 生成 `decision_id`/`decided_at`，`consumed_at` 恒 NULL）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewHumanDecision {
    /// 目标 run。
    pub run_id: String,
    /// 目标实例。
    pub instance_id: String,
    /// `approved` | `rejected`。
    pub decision: String,
    /// 决策人（非空）。
    pub approver: String,
    /// 决策理由（可空；`--reason` 未提供为 `None`）。
    pub reason: Option<String>,
    /// UUIDv7（仅审计）。
    pub decision_id: String,
    /// 决策写入时刻（UTC RFC3339 毫秒 Z）。
    pub decided_at: String,
}

/// `decide` 前置校验的同一 RO 快照结果（DR-006 §4.5 C3 顺序）。
///
/// CLI 判定顺序：`decision_exists`（exit 5）→ `run_status`（不存在/终态 exit 1）→
/// `instance_status`（不存在 exit 1）→ 非 `WaitingHuman`（exit 5）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DecisionPrecheck {
    /// `(run_id, instance_id)` 是否已有决策行（**先于**一切 run/instance 状态判断）。
    pub decision_exists: bool,
    /// run 状态；`None` = run 不存在。
    pub run_status: Option<RunStatus>,
    /// 实例状态（限定该 run）；`None` = 实例不存在或不属于该 run。
    pub instance_status: Option<InstanceStatus>,
}

/// 决策写入错误（`decide` 用）。
///
/// 与 [`TraceError`] 分开：给既有 `TraceError` 增加变体会破坏仓内/仓外对其的
/// 穷举 `match`（cli 的 `report_storage_error` 等），属 source-breaking；本类型只
/// 服务决策写者，且区分「唯一键冲突（exit 5）」与「存储错误（exit 4）」。
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum DecisionError {
    /// 唯一键冲突（`(run_id, instance_id)` 已有行或 `decision_id` 重复）→ decide exit 5。
    #[error("CONFLICT: {0}")]
    Conflict(String),

    /// 存储/版本/busy 耗尽错误 → decide exit 4。
    #[error(transparent)]
    Trace(#[from] TraceError),
}

impl DecisionError {
    /// 稳定对外错误码（decision/storage 命名空间；`HUMAN_*` 由 CLI 映射）。
    pub const fn code(&self) -> &'static str {
        match self {
            Self::Conflict(_) => "CONFLICT",
            Self::Trace(error) => error.code(),
        }
    }
}

/// `decide` 的决策写入者（独立 RW 连接；DR-006 §5.2 C13）。
pub struct DecisionStore {
    conn: Connection,
    db_path: PathBuf,
}

impl DecisionStore {
    /// 打开**已存在**的 v5 库（不创建、不迁移；DR-006 §5.4）。
    pub fn open(path: impl AsRef<Path>) -> Result<DecisionStore, TraceError> {
        let db_path = path.as_ref().to_path_buf();
        let conn = schema::open_existing_readwrite_connection(&db_path, DB_BUSY_TIMEOUT_MS)?;
        ensure_human_decisions_schema(&conn)?;
        Ok(DecisionStore { conn, db_path })
    }

    /// 数据库路径。
    pub fn db_path(&self) -> &Path {
        &self.db_path
    }

    /// 同一 RO 快照的前置校验（DR-006 §4.5 C3 顺序的数据来源）。
    pub fn precheck(
        &self,
        run_id: &str,
        instance_id: &str,
    ) -> Result<DecisionPrecheck, TraceError> {
        // 三个查询在同一个读事务快照内完成（`unchecked_transaction`：只读，
        // 结束时回滚即可），避免预检期间行/状态变化导致自相矛盾的判定。
        let tx = self.conn.unchecked_transaction().map_err(db_err)?;
        let decision_exists: bool = tx
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM human_decisions \
                 WHERE run_id = ?1 AND instance_id = ?2)",
                params![run_id, instance_id],
                |row| row.get(0),
            )
            .map_err(db_err)?;
        let run_status = match tx.query_row(
            "SELECT status FROM runs WHERE run_id = ?1",
            [run_id],
            |row| row.get::<_, String>(0),
        ) {
            Ok(text) => Some(parse_run_status(&text)?),
            Err(rusqlite::Error::QueryReturnedNoRows) => None,
            Err(err) => return Err(db_err(err)),
        };
        let instance_status = match tx.query_row(
            "SELECT status FROM instances WHERE instance_id = ?1 AND run_id = ?2",
            params![instance_id, run_id],
            |row| row.get::<_, String>(0),
        ) {
            Ok(text) => Some(parse_status(&text)?),
            Err(rusqlite::Error::QueryReturnedNoRows) => None,
            Err(err) => return Err(db_err(err)),
        };
        tx.rollback().map_err(db_err)?;
        Ok(DecisionPrecheck {
            decision_exists,
            run_status,
            instance_status,
        })
    }

    /// 单事务 INSERT 一行决策；唯一键冲突 → [`DecisionError::Conflict`]。
    ///
    /// `busy_timeout=5000ms`（连接级）之上再做 `SQLITE_BUSY` 重试
    /// [`DB_WRITE_RETRY`] 次；非 busy 的约束错误（唯一键）立即映射为冲突。
    pub fn insert(
        &mut self,
        decision: NewHumanDecision,
    ) -> Result<HumanDecisionRow, DecisionError> {
        let mut attempt = 0;
        loop {
            match self.try_insert(&decision) {
                Ok(row) => return Ok(row),
                Err(err) if is_busy(&err) && attempt + 1 < DB_WRITE_RETRY => {
                    attempt += 1;
                    sleep_before_retry();
                }
                Err(err) if is_busy(&err) => {
                    return Err(DecisionError::Trace(decision_busy_exhausted()))
                }
                Err(err) => return Err(map_insert_error(err, &decision)),
            }
        }
    }

    fn try_insert(
        &mut self,
        decision: &NewHumanDecision,
    ) -> Result<HumanDecisionRow, rusqlite::Error> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute(
            "INSERT INTO human_decisions \
             (run_id, instance_id, decision, approver, reason, decision_id, decided_at, consumed_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, NULL)",
            params![
                decision.run_id,
                decision.instance_id,
                decision.decision,
                decision.approver,
                decision.reason,
                decision.decision_id,
                decision.decided_at,
            ],
        )?;
        tx.commit()?;
        Ok(HumanDecisionRow {
            run_id: decision.run_id.clone(),
            instance_id: decision.instance_id.clone(),
            decision: decision.decision.clone(),
            approver: decision.approver.clone(),
            reason: decision.reason.clone(),
            decision_id: decision.decision_id.clone(),
            decided_at: decision.decided_at.clone(),
            consumed_at: None,
        })
    }
}

/// run 进程的决策消费者（RO 轮询 + RW 短事务标记消费；DR-006 §5.2/§5.3）。
pub struct DecisionConsumer {
    read: Connection,
    write: Connection,
    db_path: PathBuf,
}

impl DecisionConsumer {
    /// 打开**已存在**的 v5 库的轮询/标记连接（不创建、不迁移）。
    pub fn open(path: impl AsRef<Path>) -> Result<DecisionConsumer, TraceError> {
        let db_path = path.as_ref().to_path_buf();
        let read =
            schema::open_readonly_with_busy_timeout(&db_path, DECISION_POLL_BUSY_TIMEOUT_MS)?;
        let write = schema::open_existing_readwrite_connection(&db_path, DB_BUSY_TIMEOUT_MS)?;
        ensure_human_decisions_schema(&read)?;
        ensure_human_decisions_schema(&write)?;
        Ok(DecisionConsumer {
            read,
            write,
            db_path,
        })
    }

    /// 数据库路径。
    pub fn db_path(&self) -> &Path {
        &self.db_path
    }

    /// 未消费决策（`consumed_at IS NULL`；按 `(decided_at, decision_id)` 升序）。
    pub fn pending(&self, run_id: &str) -> Result<Vec<HumanDecisionRow>, TraceError> {
        query_human_decisions(&self.read, run_id, true)
    }

    /// 标记一行已消费；返回是否真的更新（已消费行不更新 → `false`，幂等）。
    ///
    /// `consumed_at` 一旦写入不再更新（审计一次；DR-006 §4.2）。busy 重试
    /// [`DB_WRITE_RETRY`] 次；失败由调用方告警（不 fail run，DR-006 §5.3 E-5）。
    pub fn mark_consumed(
        &self,
        run_id: &str,
        instance_id: &str,
        consumed_at: &str,
    ) -> Result<bool, TraceError> {
        let sql = "UPDATE human_decisions SET consumed_at = ?3 \
                   WHERE run_id = ?1 AND instance_id = ?2 AND consumed_at IS NULL";
        let mut attempt = 0;
        loop {
            match self
                .write
                .execute(sql, params![run_id, instance_id, consumed_at])
            {
                Ok(0) => return Ok(false),
                Ok(_) => return Ok(true),
                Err(err) if is_busy(&err) && attempt + 1 < DB_WRITE_RETRY => {
                    attempt += 1;
                    sleep_before_retry();
                }
                Err(err) if is_busy(&err) => return Err(decision_busy_exhausted()),
                Err(err) => return Err(db_err(err)),
            }
        }
    }
}

/// 查询 run 的决策行（`TraceReader::human_decisions` 与 `pending` 共用）。
///
/// 排序 `(decided_at, decision_id)` 升序（DR-006 §6.3 E-10：确定性顺序）。
pub(crate) fn query_human_decisions(
    conn: &Connection,
    run_id: &str,
    only_pending: bool,
) -> Result<Vec<HumanDecisionRow>, TraceError> {
    let mut sql = String::from(
        "SELECT run_id, instance_id, decision, approver, reason, decision_id, \
         decided_at, consumed_at FROM human_decisions WHERE run_id = ?1",
    );
    if only_pending {
        sql.push_str(" AND consumed_at IS NULL");
    }
    sql.push_str(" ORDER BY decided_at ASC, decision_id ASC");
    let mut stmt = conn.prepare(&sql).map_err(db_err)?;
    let mut rows = stmt.query([run_id]).map_err(db_err)?;
    let mut decisions = Vec::new();
    while let Some(row) = rows.next().map_err(db_err)? {
        decisions.push(HumanDecisionRow {
            run_id: row.get(0).map_err(db_err)?,
            instance_id: row.get(1).map_err(db_err)?,
            decision: row.get(2).map_err(db_err)?,
            approver: row.get(3).map_err(db_err)?,
            reason: row.get(4).map_err(db_err)?,
            decision_id: row.get(5).map_err(db_err)?,
            decided_at: row.get(6).map_err(db_err)?,
            consumed_at: row.get(7).map_err(db_err)?,
        });
    }
    Ok(decisions)
}

/// decision 写者 `SQLITE_BUSY` 重试耗尽的存储错误（CLI 映射 exit 4）。
fn decision_busy_exhausted() -> TraceError {
    TraceError::Storage(format!(
        "sqlite busy after {DB_WRITE_RETRY} attempts (busy_timeout={DB_BUSY_TIMEOUT_MS}ms)"
    ))
}

/// 唯一键冲突（PK `(run_id, instance_id)` / `decision_id UNIQUE`）→ `DecisionError::Conflict`。
fn map_insert_error(err: rusqlite::Error, decision: &NewHumanDecision) -> DecisionError {
    if is_unique_violation(&err) {
        return DecisionError::Conflict(format!(
            "human decision already recorded for run={} instance={} (decision_id={})",
            decision.run_id, decision.instance_id, decision.decision_id
        ));
    }
    DecisionError::Trace(db_err(err))
}

/// 仅把 UNIQUE/PRIMARY KEY 扩展码当冲突；CHECK/NOT NULL 等约束错误仍是存储错误。
fn is_unique_violation(err: &rusqlite::Error) -> bool {
    matches!(
        err,
        rusqlite::Error::SqliteFailure(inner, _)
            if inner.code == ErrorCode::ConstraintViolation
                && matches!(
                    inner.extended_code,
                    rusqlite::ffi::SQLITE_CONSTRAINT_PRIMARYKEY
                        | rusqlite::ffi::SQLITE_CONSTRAINT_UNIQUE
                )
    )
}
