//! 中止标定（06 §6.8.1）与 pid 判活（06 §6.8.3）——CLI 侧实现。
//!
//! trace 层只提供两段式原语（[`zgraph_trace::TraceWriter::interrupted_candidates`] /
//! [`zgraph_trace::TraceWriter::mark_interrupted`]），**判活顺序由 CLI 按 06 §6.8.1
//! 步骤 2 固定执行**：
//!
//! 1. **同机**（`owner_host` 已知且命中本机主机名之一）且 `pid_alive(owner_pid)` → 跳过；
//! 2. **跨机或 `owner_host` 未知** → 不能本地判 pid：`heartbeat_at` 距今 ≤
//!    [`RECOVERY_HEARTBEAT_STALE_MS`]（60s）→ 跳过**并告警**；陈旧/缺失 → 判死；
//! 3. 同机但 pid 不存在（或 pid 缺失）→ 判死。
//!
//! 主机名用 **OS hostname**（Unix `gethostname`；非 Unix 回落 `HOSTNAME`/`COMPUTERNAME`
//! 环境变量），不再把「环境变量缺失 → `unknown`」当成同机；`owner_host` 为
//! `None`/空/字面 `unknown` 时一律走 heartbeat 路径（ISSUE-088，REV-030 #2/#3）。
//! kernel M5 仍用环境变量记录 `owner_host`，因此本机判定同时接受环境变量别名。
//!
//! 标定只改 `runs`（`status=Failed`/`interrupted=1`/`recovered_at`），不写
//! `RunFinished`/`Transition`、不改 `instances`、`error_code` 保持 NULL（06 §6.8.2）。
//!
//! `pid_alive` 用 `kill(pid, 0)` 语义（Unix，`nix`）：`EPERM`（进程存在但无权限，
//! 多用户共享机）视为**存活**、`ESRCH` 视为不存在、其它错误保守视为存活；
//! 非 Unix 平台保守返回 `true`（无法判活时**不**标定，避免误杀并发 run）。
//! pid 复用是残余风险（heartbeat 二次佐证）。

use std::collections::HashMap;
use std::ffi::OsStr;

use chrono::{DateTime, SecondsFormat, Utc};

use zgraph_trace::{InterruptedCandidate, RecoveredRun, TraceError, TraceWriter};

/// 跨机判死的心跳陈旧阈值（06 §6.8.1 步骤 2：60s）。
pub const RECOVERY_HEARTBEAT_STALE_MS: i64 = 60_000;

/// 本机是否支持 `kill(pid, 0)` 判活（非 Unix 为 `false`，`trace` 据此不打印存活提示）。
pub const PID_PROBE_SUPPORTED: bool = cfg!(unix);

/// 被标定的 run + 判活依据的 owner pid（stderr 警告用）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Recovered {
    /// trace 标定原语返回的标定结果。
    pub run: RecoveredRun,
    /// 判死时的 owner pid（DB 可空）。
    pub owner_pid: Option<u32>,
}

/// 启动中止标定的结果（06 §6.8.1 步骤 2/3/5）。
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Recovery {
    /// 本次实际标定的 run（已终态/仍存活/跨机心跳未陈旧者被跳过）。
    pub recovered: Vec<Recovered>,
    /// 跳过告警（跨机或 host 未知且 heartbeat 未陈旧）：human stderr 一行 +
    /// `--json.warnings[]`（06 §6.8.1 步骤 2②「跳过并告警」，ISSUE-088）。
    pub warnings: Vec<String>,
}

/// 本机主机名：Unix 用 OS hostname（`gethostname`），失败/非 Unix 回落环境变量。
pub fn current_host() -> String {
    #[cfg(unix)]
    {
        if let Ok(name) = nix::unistd::gethostname() {
            if let Some(host) = normalize_host(name) {
                return host;
            }
        }
    }
    host_from_env()
}

/// 环境变量兜底主机名（非 Unix 的唯一来源；Unix `gethostname` 失败时兜底）。
fn host_from_env() -> String {
    for key in HOST_ENV_KEYS {
        if let Some(host) = std::env::var_os(key).and_then(normalize_host) {
            return host;
        }
    }
    UNKNOWN_HOST.to_string()
}

/// 环境变量形式的主机名（kernel M5 `host_name()` 使用同一组变量）。
const HOST_ENV_KEYS: [&str; 2] = ["HOSTNAME", "COMPUTERNAME"];

/// 未知主机名的字面值（kernel M5 缺省值；兼容旧库）。
const UNKNOWN_HOST: &str = "unknown";

/// 规范化主机名：去首尾空白；空串视为「未知」（`None`）。
fn normalize_host(raw: impl AsRef<OsStr>) -> Option<String> {
    let text = raw.as_ref().to_string_lossy();
    let trimmed = text.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

/// 主机名是否「已知」（非空且不是字面 `unknown`；兼容 M5 旧库）。
pub fn host_is_known(host: &str) -> bool {
    let host = host.trim();
    !host.is_empty() && !host.eq_ignore_ascii_case(UNKNOWN_HOST)
}

/// 本机主机名集合：OS hostname + 环境变量别名（kernel M5 用环境变量写 `owner_host`，
/// 两者可能不同；任一命中即视为同机，避免把本机 run 误判为跨机）。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct LocalHosts {
    names: Vec<String>,
}

impl LocalHosts {
    /// 探测本机全部主机名候选（去重、忽略大小写）。
    fn detect() -> Self {
        let mut names: Vec<String> = Vec::new();
        push_host(&mut names, current_host());
        for key in HOST_ENV_KEYS {
            if let Some(host) = std::env::var_os(key) {
                if let Some(host) = normalize_host(host) {
                    push_host(&mut names, host);
                }
            }
        }
        Self { names }
    }

    /// 记录值是否本机（`unknown`/空不算；忽略大小写）。
    fn contains(&self, recorded: &str) -> bool {
        let recorded = recorded.trim();
        host_is_known(recorded)
            && self
                .names
                .iter()
                .any(|name| name.eq_ignore_ascii_case(recorded))
    }
}

/// `recorded` 是否本机主机名（OS hostname 或环境变量别名；未知值恒 `false`）。
///
/// `trace` 的进行中提示用它校验 `owner_host`（ISSUE-088 / 06 §6.8.3）。
pub fn is_local_host(recorded: &str) -> bool {
    LocalHosts::detect().contains(recorded)
}

/// 追加一个本机主机名候选（去空白、跳过未知值、忽略大小写去重）。
fn push_host(names: &mut Vec<String>, host: String) {
    let host = host.trim().to_string();
    if !host_is_known(&host) {
        return;
    }
    if !names
        .iter()
        .any(|existing| existing.eq_ignore_ascii_case(&host))
    {
        names.push(host);
    }
}

/// pid 判活（`kill(pid, 0)` 语义；EPERM = 存在、ESRCH = 不存在、其它保守存在）。
#[cfg(unix)]
pub fn pid_alive(pid: u32) -> bool {
    use nix::sys::signal::kill;
    use nix::unistd::Pid;

    let Ok(pid) = i32::try_from(pid) else {
        return false;
    };
    pid_alive_from_kill(kill(Pid::from_raw(pid), None))
}

/// 非 Unix：无 `kill(pid, 0)` 语义，保守返回存活（不自动标定）。
#[cfg(not(unix))]
pub fn pid_alive(_pid: u32) -> bool {
    true
}

/// `kill(pid, 0)` 结果 → 存活判定（纯函数，单测可注入任意 errno）。
///
/// POSIX：`EPERM` 表示进程存在但无权限发信号（多用户共享机常见），必须视为**存活**；
/// `ESRCH` 表示进程不存在；其它错误（如参数/权限异常）保守按存活，避免误标定。
#[cfg(unix)]
fn pid_alive_from_kill(result: Result<(), nix::errno::Errno>) -> bool {
    use nix::errno::Errno;

    match result {
        Ok(()) => true,
        Err(Errno::EPERM) => true,
        Err(Errno::ESRCH) => false,
        Err(_) => true,
    }
}

/// 判活探针（生产为 [`pid_alive`]；单测注入闭包，覆盖 EPERM/ESRCH 之外的组合）。
type PidProbe<'a> = &'a dyn Fn(u32) -> bool;

/// 单候选判活结论（06 §6.8.1 步骤 2）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Verdict {
    /// 判死，参与标定。
    Dead,
    /// 同机 pid 存活：合法并发，静默跳过。
    AliveSameHost,
    /// 跨机或 host 未知且 heartbeat 未陈旧：跳过并告警。
    AliveRemote,
}

/// 执行启动中止标定（06 §6.8.1）：返回标定结果与跳过告警。
pub fn recover_interrupted(writer: &TraceWriter) -> Result<Recovery, TraceError> {
    let candidates = writer.interrupted_candidates()?;
    let now = Utc::now();
    let local = LocalHosts::detect();
    let mut dead: Vec<&InterruptedCandidate> = Vec::new();
    let mut warnings = Vec::new();
    for candidate in &candidates {
        match verdict_with(candidate, &local, now, &pid_alive) {
            Verdict::Dead => dead.push(candidate),
            Verdict::AliveSameHost => {}
            Verdict::AliveRemote => warnings.push(skipped_warning(candidate)),
        }
    }
    if dead.is_empty() {
        return Ok(Recovery {
            recovered: Vec::new(),
            warnings,
        });
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
    Ok(Recovery {
        recovered: recovered
            .into_iter()
            .map(|run| Recovered {
                owner_pid: pid_by_run.get(run.run_id.as_str()).copied().flatten(),
                run,
            })
            .collect(),
        warnings,
    })
}

/// 06 §6.8.1 步骤 2 的判活决策（可注入本机主机名集合与 pid 探针）。
///
/// 顺序固定：同机 → 本地 pid 判活；跨机或 host 未知 → heartbeat 陈旧性判活。
fn verdict_with(
    candidate: &InterruptedCandidate,
    local: &LocalHosts,
    now: DateTime<Utc>,
    pid_alive: PidProbe<'_>,
) -> Verdict {
    let same_host = candidate
        .owner_host
        .as_deref()
        .is_some_and(|host| local.contains(host));
    if same_host {
        // 同机：pid 存活 → 合法并发，跳过；pid 缺失/不存在 → 判死。
        return match candidate.owner_pid {
            Some(pid) if pid_alive(pid) => Verdict::AliveSameHost,
            _ => Verdict::Dead,
        };
    }
    // 跨机或 owner_host 未知（None/空/`unknown`）：不能本地判 pid，只看 heartbeat。
    if heartbeat_stale(candidate.heartbeat_at.as_deref(), now) {
        Verdict::Dead
    } else {
        Verdict::AliveRemote
    }
}

/// 跨机/host 未知但 heartbeat 未陈旧的跳过告警（06 §6.8.1 步骤 2②）。
fn skipped_warning(candidate: &InterruptedCandidate) -> String {
    let host = candidate
        .owner_host
        .as_deref()
        .filter(|host| host_is_known(host))
        .unwrap_or(UNKNOWN_HOST);
    format!(
        "warning: run {} skipped recovery (owner on {host}, heartbeat fresh)",
        candidate.run_id
    )
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
        current_host, heartbeat_stale, host_is_known, normalize_host, recover_interrupted,
        warning_line, LocalHosts, Recovered, Verdict, RECOVERY_HEARTBEAT_STALE_MS,
    };
    use chrono::{Duration, SecondsFormat, Utc};
    use zgraph_trace::{InterruptedCandidate, RecordMode, RunStartedInfo, RunStatus, TraceInput};

    fn local(names: &[&str]) -> LocalHosts {
        LocalHosts {
            names: names.iter().map(|name| (*name).to_string()).collect(),
        }
    }

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

    fn now_text() -> String {
        Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)
    }

    fn verdict(
        candidate: &InterruptedCandidate,
        local_hosts: &LocalHosts,
        pid_alive: bool,
    ) -> Verdict {
        super::verdict_with(candidate, local_hosts, Utc::now(), &|_pid| pid_alive)
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
    fn current_host_is_known_and_normalized() {
        let host = current_host();
        assert!(!host.is_empty(), "current_host must not be empty");
        assert!(host_is_known(&host), "current_host must be a known name");
        assert_eq!(host, host.trim(), "current_host must be trimmed");
    }

    #[test]
    fn normalize_host_rejects_blank_and_trims() {
        assert_eq!(normalize_host("  host-a \n"), Some("host-a".to_string()));
        assert_eq!(normalize_host("   "), None);
        assert_eq!(normalize_host(""), None);
    }

    #[test]
    fn is_local_host_matches_os_hostname_only() {
        assert!(super::is_local_host(&current_host()));
        assert!(!super::is_local_host("definitely-not-this-host.invalid"));
        assert!(!super::is_local_host("unknown"));
        assert!(!super::is_local_host(""));
    }

    #[test]
    fn host_is_known_filters_unknown_placeholder() {
        assert!(host_is_known("host-a"));
        assert!(!host_is_known(""));
        assert!(!host_is_known("   "));
        assert!(!host_is_known("unknown"));
        assert!(!host_is_known("UNKNOWN"));
    }

    #[cfg(unix)]
    #[test]
    fn pid_alive_from_kill_treats_eperm_as_alive() {
        use nix::errno::Errno;

        let classify = super::pid_alive_from_kill;
        assert!(classify(Ok(())), "kill 成功 = 存活");
        assert!(
            classify(Err(Errno::EPERM)),
            "EPERM = 进程存在但无权限 → 存活"
        );
        assert!(!classify(Err(Errno::ESRCH)), "ESRCH = 进程不存在");
        // 其它错误保守按存活，避免把活 run 误标为 interrupted。
        assert!(classify(Err(Errno::EINVAL)));
        assert!(classify(Err(Errno::EACCES)));
    }

    #[test]
    fn same_host_uses_local_pid_probe() {
        let hosts = local(&["host-a"]);
        // 注入探针：pid “存活” → 跳过（不依赖真实进程）。
        assert_eq!(
            verdict(&candidate(Some("host-a"), Some(42), None), &hosts, true),
            Verdict::AliveSameHost
        );
        assert_eq!(
            verdict(&candidate(Some("HOST-A"), Some(42), None), &hosts, true),
            Verdict::AliveSameHost,
            "主机名比较忽略大小写"
        );
        // pid 不存在 / 缺失 → 判死。
        assert_eq!(
            verdict(&candidate(Some("host-a"), Some(42), None), &hosts, false),
            Verdict::Dead
        );
        assert_eq!(
            verdict(&candidate(Some("host-a"), None, None), &hosts, true),
            Verdict::Dead
        );
    }

    #[test]
    fn cross_host_uses_heartbeat_staleness() {
        let hosts = local(&["host-a"]);
        let now = Utc::now();
        let fresh = now.to_rfc3339_opts(SecondsFormat::Millis, true);
        let stale = (now - Duration::milliseconds(RECOVERY_HEARTBEAT_STALE_MS + 1_000))
            .to_rfc3339_opts(SecondsFormat::Millis, true);
        // 跨机 + 心跳新 → 跳过（不因本地同号 pid 判死）。
        assert_eq!(
            verdict(
                &candidate(Some("other-host"), Some(1), Some(fresh)),
                &hosts,
                true
            ),
            Verdict::AliveRemote
        );
        // 跨机 + 心跳陈旧/缺失/不可解析 → 判死。
        assert_eq!(
            verdict(
                &candidate(Some("other-host"), Some(1), Some(stale)),
                &hosts,
                true
            ),
            Verdict::Dead
        );
        assert_eq!(
            verdict(&candidate(Some("other-host"), Some(1), None), &hosts, true),
            Verdict::Dead
        );
        assert_eq!(
            verdict(
                &candidate(Some("other-host"), None, Some("not-a-time".to_string())),
                &hosts,
                true
            ),
            Verdict::Dead
        );
        assert!(heartbeat_stale(None, now));
    }

    #[test]
    fn unknown_host_uses_heartbeat_not_local_pid() {
        // ISSUE-088 #2：owner_host 为 None/空/`unknown` 时不能按同机 pid 判死，
        // 必须走 heartbeat 路径；本地同号 pid 存活与否不影响结论。
        let hosts = local(&["host-a"]);
        let fresh = now_text();
        for host in [None, Some(""), Some("unknown"), Some("  ")] {
            assert_eq!(
                verdict(
                    &candidate(host, Some(42), Some(fresh.clone())),
                    &hosts,
                    true
                ),
                Verdict::AliveRemote,
                "host={host:?}: 心跳新 → 跳过并告警"
            );
            assert_eq!(
                verdict(
                    &candidate(host, Some(42), Some(fresh.clone())),
                    &hosts,
                    false
                ),
                Verdict::AliveRemote,
                "host={host:?}: 本地 pid 探针结论不影响未知 host 判定"
            );
            assert_eq!(
                verdict(&candidate(host, Some(42), None), &hosts, true),
                Verdict::Dead,
                "host={host:?}: 心跳缺失 → 判死"
            );
        }
    }

    #[test]
    fn skipped_warning_names_the_owner_host() {
        let warning = super::skipped_warning(&candidate(Some("other-host"), None, None));
        assert!(warning.contains("skipped recovery"), "{warning}");
        assert!(warning.contains("other-host"), "{warning}");
        let unknown = super::skipped_warning(&candidate(None, None, None));
        assert!(unknown.contains("owner on unknown"), "{unknown}");
    }

    #[cfg(unix)]
    #[test]
    fn pid_alive_observes_process_lifecycle() {
        assert!(super::pid_alive(std::process::id()));
        assert!(!super::pid_alive(dead_pid()));
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
            global_vars: serde_json::json!({}),
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
        let path = seed_running(dir.path(), "run-dead", &host, dead_pid(), now_text());
        let writer = zgraph_trace::TraceWriter::open(&path).expect("open writer");
        let recovery = recover_interrupted(&writer).expect("recover");
        assert_eq!(recovery.recovered.len(), 1, "{recovery:?}");
        assert_eq!(recovery.recovered[0].run.run_id, "run-dead");
        assert_eq!(
            recovery.recovered[0].run.previous_status,
            RunStatus::Running
        );
        assert!(recovery.recovered[0].owner_pid.is_some());
        assert!(warning_line(&recovery.recovered[0]).contains("marked interrupted"));
        assert!(recovery.warnings.is_empty(), "同机存活/判死不产生跳过告警");
        // 幂等：再次标定不重复返回。
        assert!(recover_interrupted(&writer)
            .expect("second")
            .recovered
            .is_empty());
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
            now_text(),
        );
        let writer = zgraph_trace::TraceWriter::open(&path).expect("open writer");
        let recovery = recover_interrupted(&writer).expect("recover");
        assert!(recovery.recovered.is_empty());
        assert!(recovery.warnings.is_empty());
        writer.close().expect("close");
        let reader = zgraph_trace::TraceReader::open_readonly(&path).expect("reader");
        let run = reader.run("run-live").expect("run").expect("row");
        assert_eq!(run.status, RunStatus::Running);
        assert!(!run.interrupted);
    }

    #[cfg(unix)]
    #[test]
    fn recover_interrupted_warns_on_fresh_cross_host_run() {
        // 06 §6.8.1 步骤 2②：跨机且 heartbeat 未陈旧 → 跳过并告警，不改 DB。
        let dir = tempfile::tempdir().expect("tempdir");
        let path = seed_running(
            dir.path(),
            "run-remote",
            "other-host",
            dead_pid(),
            now_text(),
        );
        let writer = zgraph_trace::TraceWriter::open(&path).expect("open writer");
        let recovery = recover_interrupted(&writer).expect("recover");
        assert!(recovery.recovered.is_empty(), "{recovery:?}");
        assert_eq!(recovery.warnings.len(), 1, "{recovery:?}");
        assert!(
            recovery.warnings[0].contains("run-remote")
                && recovery.warnings[0].contains("other-host")
                && recovery.warnings[0].contains("skipped recovery"),
            "{}",
            recovery.warnings[0]
        );
        writer.close().expect("close");
        let reader = zgraph_trace::TraceReader::open_readonly(&path).expect("reader");
        let run = reader.run("run-remote").expect("run").expect("row");
        assert_eq!(run.status, RunStatus::Running, "跳过 → 不改 DB");
        assert!(!run.interrupted);
    }

    #[cfg(unix)]
    #[test]
    fn recover_interrupted_warns_on_fresh_unknown_host_run() {
        // owner_host 未知（旧库/缺环境变量）→ heartbeat 路径；新心跳跳过并告警。
        let dir = tempfile::tempdir().expect("tempdir");
        let path = seed_running(dir.path(), "run-unknown", "unknown", dead_pid(), now_text());
        let writer = zgraph_trace::TraceWriter::open(&path).expect("open writer");
        let recovery = recover_interrupted(&writer).expect("recover");
        assert!(recovery.recovered.is_empty(), "{recovery:?}");
        assert_eq!(recovery.warnings.len(), 1, "{recovery:?}");
        assert!(
            recovery.warnings[0].contains("run-unknown")
                && recovery.warnings[0].contains("owner on unknown"),
            "{}",
            recovery.warnings[0]
        );
        writer.close().expect("close");
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
