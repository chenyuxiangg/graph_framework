//! 中止标定的 SQL 原语（06 §6.8.1）：全部在写线程自己的连接上执行。
//!
//! 两段式设计刻意把「判活」留给调用方：本模块只做
//! ① 查候选（`SELECT ... WHERE status IN ('Running','Paused')`）；
//! ② 标定（单事务逐 run `UPDATE`，命中才返回）。
//! 标定**不**修改 `instances`、**不**写 `RunFinished`/`Transition`，`error_code`
//! 保持 NULL（06 §6.8.2）。

use rusqlite::{params, Connection, OptionalExtension, Transaction, TransactionBehavior};

use crate::error::TraceError;
use crate::model::RunStatus;
use crate::recovery::{InterruptedCandidate, RecoveredRun, INTERRUPTED_ERROR_MESSAGE};
use crate::schema::{db_err, DB_WRITE_RETRY};
use crate::writer::tx::{busy_exhausted, is_busy, sleep_before_retry};

/// 查询判活候选（06 §6.8.1 步骤 1）。
///
/// 顺序稳定为 `started_at, run_id`，便于调用方按确定顺序判活/告警。
pub(crate) fn interrupted_candidates(
    conn: &Connection,
) -> Result<Vec<InterruptedCandidate>, TraceError> {
    let mut stmt = conn
        .prepare(
            "SELECT run_id, status, owner_pid, owner_host, heartbeat_at FROM runs \
             WHERE status IN ('Running','Paused') ORDER BY started_at ASC, run_id ASC",
        )
        .map_err(db_err)?;
    let rows = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<i64>>(2)?,
                row.get::<_, Option<String>>(3)?,
                row.get::<_, Option<String>>(4)?,
            ))
        })
        .map_err(db_err)?;
    let mut candidates = Vec::new();
    for row in rows {
        let (run_id, status, owner_pid, owner_host, heartbeat_at) = row.map_err(db_err)?;
        candidates.push(InterruptedCandidate {
            run_id,
            status: parse_running_status(&status)?,
            owner_pid: owner_pid.and_then(|pid| u32::try_from(pid).ok()),
            owner_host,
            heartbeat_at,
        });
    }
    Ok(candidates)
}

/// 单事务把判死的 run 标定为 `Failed + interrupted=1`（06 §6.8.1 步骤 3）。
///
/// 逐 run 先 `SELECT status` 再条件 `UPDATE`，两者同事务（`BEGIN IMMEDIATE`），
/// 保证"只更新标定瞬间仍为 Running/Paused 的行"。任一 SQL 失败 → 整体回滚并返错。
pub(crate) fn mark_interrupted(
    conn: &mut Connection,
    run_ids: &[String],
    now: &str,
) -> Result<Vec<RecoveredRun>, TraceError> {
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
        // 事务随 `tx` drop 自动回滚：失败时不留半标定状态。
        let recovered = mark_in_transaction(&tx, run_ids, now)?;
        match tx.commit() {
            Ok(()) => return Ok(recovered),
            Err(err) if is_busy(&err) && attempt < DB_WRITE_RETRY => sleep_before_retry(),
            Err(err) if is_busy(&err) => return Err(busy_exhausted()),
            Err(err) => return Err(db_err(err)),
        }
    }
}

fn mark_in_transaction(
    tx: &Transaction<'_>,
    run_ids: &[String],
    now: &str,
) -> Result<Vec<RecoveredRun>, TraceError> {
    let mut recovered = Vec::new();
    for run_id in run_ids {
        let previous: Option<String> = tx
            .query_row(
                "SELECT status FROM runs WHERE run_id = ?1 AND status IN ('Running','Paused')",
                params![run_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(db_err)?;
        let Some(previous) = previous else {
            continue;
        };
        let previous_status = parse_running_status(&previous)?;
        tx.execute(
            "UPDATE runs SET status = 'Failed', interrupted = 1, recovered_at = ?2, \
             error_message = ?3 WHERE run_id = ?1 AND status IN ('Running','Paused')",
            params![run_id, now, INTERRUPTED_ERROR_MESSAGE],
        )
        .map_err(db_err)?;
        recovered.push(RecoveredRun {
            run_id: run_id.clone(),
            previous_status,
            recovered_at: now.to_string(),
        });
    }
    Ok(recovered)
}

fn parse_running_status(text: &str) -> Result<RunStatus, TraceError> {
    match RunStatus::parse(text) {
        Some(status @ (RunStatus::Running | RunStatus::Paused)) => Ok(status),
        _ => Err(TraceError::Storage(format!(
            "unexpected run status in recovery query: {text}"
        ))),
    }
}
