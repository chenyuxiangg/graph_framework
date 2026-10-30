//! 中止标定（06 §6.8）：两段式原语的数据类型与固定文案。
//!
//! trace 层只提供两段原语，**不做判活**：`kill(pid, 0)` 与 heartbeat 陈旧判断由
//! 调用方（`zgraph run` 启动流程，REV-030）按 06 §6.8.1 步骤 2 的固定顺序执行，
//! 避免在本 crate 引入 OS/进程依赖。
//!
//! 标定**只改 `runs`**：不修改 `instances`、不写 `RunFinished`/`Transition`
//! （06 §6.8.2 硬约束），`error_code` 保持 NULL（中断不属于 00 §0.4 领域错误码）。

use crate::model::RunStatus;

/// 标定写入 `runs.error_message` 的固定文案（06 §6.8.1 步骤 3）。
pub const INTERRUPTED_ERROR_MESSAGE: &str = "run interrupted: owner process exited";

/// 判活候选（06 §6.8.1 步骤 1）：`status IN ('Running','Paused')` 的 run。
///
/// 字段按判活需要展开：同一主机看 `owner_pid` 是否存活；跨机退回
/// `heartbeat_at` 是否超过 `RECOVERY_HEARTBEAT_STALE_MS`。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InterruptedCandidate {
    /// run id。
    pub run_id: String,
    /// 当前状态（`Running` 或 `Paused`）。
    pub status: RunStatus,
    /// owner 进程号（DB 可空；非法/越界值读作 `None`）。
    pub owner_pid: Option<u32>,
    /// owner 主机名（v1 恒本机；DB 可空）。
    pub owner_host: Option<String>,
    /// 写线程最近一次心跳时刻（RFC3339 毫秒 Z）。
    pub heartbeat_at: Option<String>,
}

/// 实际被中止标定的 run（06 §6.8.1 步骤 3 的更新结果）。
///
/// 只有 `UPDATE ... WHERE status IN ('Running','Paused')` 命中的行才会出现在
/// 返回值里；已终态或不存在的 `run_id` 被静默跳过。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecoveredRun {
    /// run id。
    pub run_id: String,
    /// 标定前的状态（`Running`/`Paused`）。
    pub previous_status: RunStatus,
    /// 本次标定时间（由调用方传入，RFC3339 毫秒 Z）。
    pub recovered_at: String,
}
