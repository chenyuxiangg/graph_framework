//! 中止标定（06 §6.8.1）与 pid 判活（06 §6.8.3）——CLI 侧实现。
//!
//! trace 层只提供两段式原语（[`zgraph_trace::TraceWriter::interrupted_candidates`] /
//! [`zgraph_trace::TraceWriter::mark_interrupted`]），**判活顺序由 CLI 按 06 §6.8.1
//! 步骤 2 固定执行**：
//!
//! 1. 同机（`owner_host` 等于本机，或 DB 未记录 host）且 `pid_alive(owner_pid)` → 跳过；
//! 2. 跨机 → `heartbeat_at` 距今 > [`RECOVERY_HEARTBEAT_STALE_MS`]（60s）才判死；
//! 3. 同机但 pid 不存在（或 pid 缺失）→ 判死。
//!
//! 标定只改 `runs`（`status=Failed`/`interrupted=1`/`recovered_at`），不写
//! `RunFinished`/`Transition`、不改 `instances`、`error_code` 保持 NULL（06 §6.8.2）。
//!
//! `pid_alive` 用 `kill(pid, 0)` 语义（Unix，`nix`）；非 Unix 平台保守返回 `true`
//! （无法判活时**不**标定，避免误杀并发 run）。pid 复用是残余风险（heartbeat 二次佐证）。

use std::collections::HashMap;

use chrono::{DateTime, SecondsFormat, Utc};

use zgraph_trace::{InterruptedCandidate, RecoveredRun, TraceError, TraceWriter};

/// 跨机判死的心跳陈旧阈值（06 §6.8.1 步骤 2：60s）。
pub const RECOVERY_HEARTBEAT_STALE_MS: i64 = 60_000;

/// 被标定的 run + 判活依据的 owner pid（stderr 警告用）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Recovered {
    /// trace 标定原语返回的标定结果。
    pub run: RecoveredRun,
    /// 判死时的 owner pid（DB 可空）。
    pub owner_pid: Option<u32>,
}

/// 本机主机名：与 kernel `owner_host` 的取值逻辑一致
/// （`HOSTNAME` → `COMPUTERNAME` → `unknown`，kernel `run.rs::host_name`）。
pub fn current_host() -> String {
    std::env::var("HOSTNAME")
        .or_else(|_| std::env::var("COMPUTERNAME"))
        .unwrap_or_else(|_| "unknown".to_string())
}

/// pid 是否存活（`kill(pid, 0)` 语义；ESRCH 即不存活）。
#[cfg(unix)]
pub fn pid_alive(pid: u32) -> bool {
    use nix::sys::signal::kill;
    use nix::unistd::Pid;

    let Ok(pid) = i32::try_from(pid) else {
        return false;
    };
    kill(Pid::from_raw(pid), None).is_ok()
}

/// 非 Unix：无 `kill(pid, 0)` 语义，保守返回存活（不自动标定）。
#[cfg(not(unix))]
pub fn pid_alive(_pid: u32) -> bool {
    true
}

/// 执行启动中止标定（06 §6.8.1）：返回本次实际标定的 run（已终态/仍存活的被跳过）。
pub fn recover_interrupted(writer: &TraceWriter) -> Result<Vec<Recovered>, TraceError> {
    let candidates = writer.interrupted_candidates()?;
    let now = Utc::now();
    let host = current_host();
    let dead: Vec<&InterruptedCandidate> = candidates
        .iter()
        .filter(|candidate| is_dead(candidate, &host, now))
        .collect();
    if dead.is_empty() {
        return Ok(Vec::new());
    }
    let run_ids: Vec<String> = dead
        .iter()
        .map(|candidate| candidate.run_id.clone())
        .collect();
    let now_text = now.to_rfc3339_opts(SecondsFormat::Millis, true);
    let recovered = writer.mark_interrupted(&run_ids, &now_text)?;
    let pid_by_run: HashMap<&str, Option<u32>> = dead
        .iter()
        .map(|candidate| (candidate.run_id.as_str(), candidate.owner_pid))
        .collect();
    Ok(recovered
        .into_iter()
        .map(|run| Recovered {
            owner_pid: pid_by_run.get(run.run_id.as_str()).copied().flatten(),
            run,
        })
        .collect())
}

/// 06 §6.8.1 步骤 2 的判死顺序（同机看 pid；跨机看 heartbeat 是否陈旧）。
fn is_dead(candidate: &InterruptedCandidate, current_host: &str, now: DateTime<Utc>) -> bool {
    let same_host = candidate
        .owner_host
        .as_deref()
        .is_none_or(|host| host == current_host);
    if same_host {
        // 同机：pid 存活 → 合法并发，跳过；pid 缺失/不存在 → 判死。
        return candidate.owner_pid.is_none_or(|pid| !pid_alive(pid));
    }
    heartbeat_stale(candidate.heartbeat_at.as_deref(), now)
}

/// 跨机判死的 heartbeat 陈旧判断（缺失/不可解析视为陈旧）。
fn heartbeat_stale(heartbeat_at: Option<&str>, now: DateTime<Utc>) -> bool {
    let Some(text) = heartbeat_at else {
        return true;
    };
    match DateTime::parse_from_rfc3339(text) {
        Ok(at) => {
            now.signed_duration_since(at.with_timezone(&Utc))
                .num_milliseconds()
                > RECOVERY_HEARTBEAT_STALE_MS
        }
        Err(_) => true,
    }
}

/// stderr 警告行（06 §6.8.1 步骤 5）。
pub fn warning_line(recovered: &Recovered) -> String {
    let status = recovered.run.previous_status.as_str();
    match recovered.owner_pid {
        Some(pid) => format!(
            "warning: run {} marked interrupted (was {status}, pid {pid} gone)",
            recovered.run.run_id
        ),
        None => format!(
            "warning: run {} marked interrupted (was {status})",
            recovered.run.run_id
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        current_host, heartbeat_stale, is_dead, pid_alive, recover_interrupted, warning_line,
        Recovered, RECOVERY_HEARTBEAT_STALE_MS,
    };
    use chrono::{Duration, SecondsFormat, Utc};
    use zgraph_trace::{InterruptedCandidate, RecordMode, RunStartedInfo, RunStatus, TraceInput};

    fn candidate(
        owner_host: Option<&str>,
        owner_pid: Option<u32>,
        heartbeat_at: Option<String>,
    ) -> InterruptedCandidate {
        InterruptedCandidate {
            run_id: "run-x".to_string(),
            status: RunStatus::Running,
            owner_pid,
            owner_host: owner_host.map(str::to_string),
            heartbeat_at,
        }
    }

    /// 已退出并回收的进程 pid（`kill(pid, 0)` 必为 ESRCH）。
    #[cfg(unix)]
    fn dead_pid() -> u32 {
        let mut child = std::process::Command::new("true")
            .spawn()
            .expect("spawn true");
        let pid = child.id();
        child.wait().expect("wait true");
        pid
    }

    #[test]
    fn same_host_alive_pid_is_skipped() {
        let now = Utc::now();
        let host = current_host();
        let live = candidate(Some(&host), Some(std::process::id()), None);
        assert!(!is_dead(&live, &host, now));
    }

    #[cfg(unix)]
    #[test]
    fn same_host_dead_or_missing_pid_is_dead() {
        let now = Utc::now();
        let host = current_host();
        let dead = candidate(Some(&host), Some(dead_pid()), None);
        assert!(is_dead(&dead, &host, now));
        let missing = candidate(Some(&host), None, None);
        assert!(is_dead(&missing, &host, now));
        // DB 未记录 host（None）按同机处理。
        let no_host = candidate(None, Some(dead_pid()), None);
        assert!(is_dead(&no_host, &host, now));
    }

    #[test]
    fn cross_host_uses_heartbeat_staleness() {
        let now = Utc::now();
        let fresh = now.to_rfc3339_opts(SecondsFormat::Millis, true);
        let stale = (now - Duration::milliseconds(RECOVERY_HEARTBEAT_STALE_MS + 1_000))
            .to_rfc3339_opts(SecondsFormat::Millis, true);
        let host = current_host();
        assert!(!is_dead(
            &candidate(Some("other-host"), None, Some(fresh)),
            &host,
            now
        ));
        assert!(is_dead(
            &candidate(Some("other-host"), None, Some(stale)),
            &host,
            now
        ));
        // 心跳缺失/不可解析 → 判死（无法证明存活）。
        assert!(is_dead(
            &candidate(Some("other-host"), None, None),
            &host,
            now
        ));
        assert!(is_dead(
            &candidate(Some("other-host"), None, Some("not-a-time".to_string())),
            &host,
            now
        ));
        assert!(heartbeat_stale(None, now));
    }

    #[cfg(unix)]
    #[test]
    fn pid_alive_observes_process_lifecycle() {
        assert!(pid_alive(std::process::id()));
        assert!(!pid_alive(dead_pid()));
    }

    /// 写一条 `Running` 的 RunStarted（不写 RunFinished），返回 db 路径与 run id。
    fn seed_running(
        dir: &std::path::Path,
        run_id: &str,
        owner_host: &str,
        owner_pid: u32,
        started_at: String,
    ) -> std::path::PathBuf {
        let path = dir.join("recovery.db");
        let writer = zgraph_trace::TraceWriter::open(&path).expect("writer");
        writer.handle().send(TraceInput::RunStarted(RunStartedInfo {
            run_id: run_id.to_string(),
            graph_hash: "0123456789abcdef".to_string(),
            graph_name: "demo".to_string(),
            dsl_json: "{}".to_string(),
            user_id: "local".to_string(),
            entry_inputs: serde_json::json!({}),
            seed: 0,
            record_mode: RecordMode::Hash,
            owner_pid,
            owner_host: owner_host.to_string(),
            started_at,
        }));
        writer.flush().expect("flush");
        writer.close().expect("close");
        path
    }

    #[cfg(unix)]
    #[test]
    fn recover_interrupted_marks_dead_owner_and_skips_live_one() {
        let dir = tempfile::tempdir().expect("tempdir");
        let host = current_host();
        let path = seed_running(
            dir.path(),
            "run-dead",
            &host,
            dead_pid(),
            Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true),
        );
        let writer = zgraph_trace::TraceWriter::open(&path).expect("open writer");
        let recovered = recover_interrupted(&writer).expect("recover");
        assert_eq!(recovered.len(), 1, "{recovered:?}");
        assert_eq!(recovered[0].run.run_id, "run-dead");
        assert_eq!(recovered[0].run.previous_status, RunStatus::Running);
        assert!(recovered[0].owner_pid.is_some());
        assert!(warning_line(&recovered[0]).contains("marked interrupted"));
        // 幂等：再次标定不重复返回。
        assert!(recover_interrupted(&writer).expect("second").is_empty());
        writer.close().expect("close");

        let reader = zgraph_trace::TraceReader::open_readonly(&path).expect("reader");
        let run = reader.run("run-dead").expect("run").expect("row");
        assert_eq!(run.status, RunStatus::Failed);
        assert!(run.interrupted);
        assert!(run.recovered_at.is_some());
        assert_eq!(
            run.error_code, None,
            "interrupted is not a domain error code"
        );
    }

    #[cfg(unix)]
    #[test]
    fn recover_interrupted_skips_live_owner() {
        let dir = tempfile::tempdir().expect("tempdir");
        let host = current_host();
        let path = seed_running(
            dir.path(),
            "run-live",
            &host,
            std::process::id(),
            Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true),
        );
        let writer = zgraph_trace::TraceWriter::open(&path).expect("open writer");
        assert!(recover_interrupted(&writer).expect("recover").is_empty());
        writer.close().expect("close");
        let reader = zgraph_trace::TraceReader::open_readonly(&path).expect("reader");
        let run = reader.run("run-live").expect("run").expect("row");
        assert_eq!(run.status, RunStatus::Running);
        assert!(!run.interrupted);
    }

    #[test]
    fn warning_line_includes_pid_when_known() {
        let recovered = Recovered {
            run: zgraph_trace::RecoveredRun {
                run_id: "run-1".to_string(),
                previous_status: RunStatus::Paused,
                recovered_at: "2026-10-30T00:00:00.000Z".to_string(),
            },
            owner_pid: Some(42),
        };
        assert_eq!(
            warning_line(&recovered),
            "warning: run run-1 marked interrupted (was Paused, pid 42 gone)"
        );
    }
}
