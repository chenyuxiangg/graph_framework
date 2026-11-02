//! macOS Seatbelt OS 沙箱（`sandbox-exec`；DR-013 §5 + spike 附录 A 冻结）。
//!
//! 组成：
//! - **profile 渲染**：79 行模板（附录 A）+ workspace 读写、allow-path 只读、
//!   `process-exec` 目录收窄（exec_paths）、系统只读集合、mach 服务、网络 deny；
//!   所有路径 `realpath` + `/` 开头 + 拒绝 `"`/换行/控制字符（防 SBPL 注入）；
//! - **preflight**：`sandbox-exec -p <profile> /usr/bin/true`，exit 0 可用；
//!   **71 → `sandbox_nested`**；**65 → `sandbox_profile_invalid`**；其它/不可执行 →
//!   `sandbox_unavailable`（均 `HARNESS_CONFIG_LOAD`，零新码）；
//! - **进程监督**：setsid/新进程组、drain 上限、SIGTERM 2s→SIGKILL、超时/取消
//!   回收进程组；运行期沙箱故障 → `TOOL_ERROR{reason:"sandbox_failure"}`。
//!
//! **平台**：仅 macOS 实现真实沙箱；Linux/Windows 上 `sandbox-exec` 不存在 →
//! preflight `sandbox_unavailable`（fail-closed，文档化）；**无 fallback**。

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use nix::sys::signal::{killpg, Signal};
use nix::unistd::Pid;
use sha2::{Digest, Sha256};

use crate::constants::{
    BASH_DRAIN_MAX_BYTES, BASH_PREFLIGHT_TIMEOUT_MS, BASH_TERMINATE_GRACE_MS, SANDBOX_EXEC_PATH,
};
use crate::error::HarnessError;
use crate::tools::ToolFailure;

/// 冻结的 SBPL 模板（spike 附录 A；`@WORKSPACE@`/`@ALLOW_PATHS@`/`@EXEC_PATHS@`/
/// `@NETWORK@` 由 Rust 侧渲染替换）。
const PROFILE_TEMPLATE: &str = r#"(version 1)
(deny default)
(debug deny)

;;; ---------- 进程 ----------
(allow process-fork)
(allow signal (target self))
(allow process-info* (target self))
(allow sysctl-read)
(allow sysctl-write (sysctl-name "kern.grade_cputype"))
(allow process-exec
  (subpath "/bin") (subpath "/sbin")
  (subpath "/usr/bin") (subpath "/usr/sbin") (subpath "/usr/lib")
  (subpath "/System")
  (subpath "/Library/Apple")
  (subpath "/Library/Developer/CommandLineTools")
  (subpath "@WORKSPACE@"@EXEC_PATHS@))

;;; ---------- 系统只读路径 ----------
(allow file-read* file-test-existence
  (literal "/")
  (subpath "/System")
  (subpath "/usr/lib")
  (subpath "/usr/share")
  (subpath "/usr/bin")
  (subpath "/usr/sbin")
  (subpath "/bin")
  (subpath "/sbin")
  (subpath "/Library/Apple")
  (subpath "/Library/Developer/CommandLineTools")
  (subpath "/private/var/db/timezone")
  (subpath "/private/var/db/dyld")
  (subpath "/private/var/db/DarwinDirectory")
  (literal "/private/etc/localtime")
  (literal "/private/var/select/sh"))
(allow file-read-metadata
  (subpath "/usr") (subpath "/Library") (subpath "/System")
  (subpath "/bin") (subpath "/sbin") (subpath "/private"))
(allow file-read-metadata file-test-existence
  (literal "/etc") (literal "/tmp") (literal "/var"))

;;; ---------- 设备 ----------
(allow file-read* file-test-existence file-write-data
  (literal "/dev/null") (literal "/dev/zero"))
(allow file-read* file-test-existence
  (literal "/dev/random") (literal "/dev/urandom"))
(allow file-read-data file-test-existence file-write-data
  (subpath "/dev/fd"))

;;; ---------- 标准 mach 服务 ----------
(allow mach-lookup
  (global-name "com.apple.cfprefsd.agent")
  (global-name "com.apple.cfprefsd.daemon")
  (global-name "com.apple.system.logger")
  (global-name "com.apple.system.notification_center")
  (global-name "com.apple.system.opendirectoryd.libinfo")
  (global-name "com.apple.system.opendirectoryd.membership")
  (global-name "com.apple.trustd")
  (global-name "com.apple.trustd.agent")
  (global-name "com.apple.logd")
  (global-name "com.apple.logd.events")
  (global-name "com.apple.runningboard")
  (global-name "com.apple.secinitd")
  (global-name "com.apple.bsd.dirhelper")
  (global-name "com.apple.analyticsd")
  (local-name "com.apple.cfprefsd.agent"))
(allow ipc-posix-shm-read*
  (ipc-posix-name "apple.shm.notification_center")
  (ipc-posix-name-prefix "apple.cfprefs."))

;;; ---------- workspace: 读写 ----------
(allow file-read* file-write* file-test-existence (subpath "@WORKSPACE@"))

;;; ---------- 只读白名单（--allow-path） ----------
@ALLOW_PATHS@

;;; ---------- exec 目录读取（--allow-exec-path 同步授予该目录只读） ----------
@EXEC_READS@

;;; ---------- 网络（默认全拒；--allow-network 时放开） ----------
@NETWORK@
"#;

/// SBPL 模板占位符（渲染前路径中禁止出现；防二次替换注入，REV-108 C2）。
const TEMPLATE_TOKENS: [&str; 5] = [
    "@WORKSPACE@",
    "@EXEC_PATHS@",
    "@ALLOW_PATHS@",
    "@EXEC_READS@",
    "@NETWORK@",
];

/// 网络放开时的追加 block（spike verify.py network allow 分支）。
const NETWORK_ALLOW_BLOCK: &str = r#"(allow network*)
(allow file-read* file-test-existence
  (subpath "/private/etc/ssl")
  (literal "/private/etc/hosts")
  (literal "/private/etc/resolv.conf"))
(allow mach-lookup
  (global-name "com.apple.networkd")
  (global-name "com.apple.networkscored"))"#;

/// profile 渲染输入（路径必须已 canonicalize；渲染期再 realpath + 校验）。
#[derive(Debug, Clone)]
pub(crate) struct SandboxProfileConfig {
    /// workspace 根（可读写）。
    pub workspace: PathBuf,
    /// 额外只读白名单目录。
    pub allow_paths: Vec<PathBuf>,
    /// 额外可执行目录（`--allow-exec-path`，opt-in）。
    pub exec_paths: Vec<PathBuf>,
    /// 是否放开网络（`--allow-network`；默认 deny）。
    pub allow_network: bool,
}

/// 渲染后的 profile。
#[derive(Debug, Clone)]
pub(crate) struct RenderedProfile {
    /// SBPL 文本（交给 `sandbox-exec -p`）。
    pub text: String,
    /// profile sha256（D5 `tool_calls.sandbox_profile_sha256`）。
    pub sha256: String,
}

/// 渲染 profile（路径 realpath + 注入防护 + **单次**安全模板替换；失败 →
/// `HARNESS_CONFIG_LOAD{reason:"sandbox_profile_invalid"}`）。
///
/// REV-108 C2：路径 realpath 后拒绝包含任一模板 token（`@WORKSPACE@` 等）或
/// `"`/反斜杠/换行/控制字符；渲染采用一次扫描替换，插入的路径值不会被再次扫描，
/// 因而不存在「路径把 SBPL 片段注入到已渲染引号内」的二次替换。
pub(crate) fn render_profile(
    config: &SandboxProfileConfig,
) -> Result<RenderedProfile, HarnessError> {
    let workspace = render_path(&config.workspace, "workspace")?;
    let mut allow_lines = Vec::new();
    for path in &config.allow_paths {
        allow_lines.push(format!(
            "(allow file-read* file-test-existence (subpath \"{}\"))",
            render_path(path, "allow_path")?
        ));
    }
    let mut exec_lines = String::new();
    let mut exec_reads = Vec::new();
    for path in &config.exec_paths {
        let rendered = render_path(path, "exec_path")?;
        exec_lines.push_str(&format!("\n  (subpath \"{rendered}\")"));
        // REV-108 N2：exec-path 还需可读（二进制/动态库），否则动态加载器读不到。
        exec_reads.push(format!(
            "(allow file-read* file-test-existence (subpath \"{rendered}\"))"
        ));
    }
    let network = if config.allow_network {
        NETWORK_ALLOW_BLOCK
    } else {
        "(deny network*)"
    };
    let text = fill_template(&[
        ("@WORKSPACE@", workspace.as_str()),
        ("@EXEC_PATHS@", exec_lines.as_str()),
        ("@ALLOW_PATHS@", allow_lines.join("\n").as_str()),
        ("@EXEC_READS@", exec_reads.join("\n").as_str()),
        ("@NETWORK@", network),
    ]);
    Ok(RenderedProfile {
        sha256: sha256_hex(text.as_bytes()),
        text,
    })
}

/// 单次扫描替换模板 token；插入值不再参与后续匹配（防注入）。
fn fill_template(pairs: &[(&str, &str)]) -> String {
    let mut output = String::with_capacity(PROFILE_TEMPLATE.len() + 512);
    let mut rest = PROFILE_TEMPLATE;
    loop {
        let mut earliest: Option<(usize, &str, &str)> = None;
        for (token, value) in pairs {
            if let Some(index) = rest.find(*token) {
                if earliest.is_none_or(|(best, _, _)| index < best) {
                    earliest = Some((index, *token, *value));
                }
            }
        }
        let Some((index, token, value)) = earliest else {
            output.push_str(rest);
            return output;
        };
        output.push_str(&rest[..index]);
        output.push_str(value);
        rest = &rest[index + token.len()..];
    }
}

/// 渲染单个沙箱路径：realpath（`/private/...`）+ 绝对 + 拒绝注入字符。
fn render_path(path: &Path, label: &str) -> Result<String, HarnessError> {
    let canonical = std::fs::canonicalize(path).map_err(|error| {
        profile_error(format!(
            "sandbox {label} path '{}' cannot be resolved: {error}",
            path.display()
        ))
    })?;
    let text = canonical.to_string_lossy().to_string();
    if !text.starts_with('/') {
        return Err(profile_error(format!(
            "sandbox {label} path '{}' is not absolute after realpath",
            path.display()
        )));
    }
    if text.contains('"')
        || text.contains('\\')
        || text.contains('\n')
        || text.contains('\r')
        || text.chars().any(char::is_control)
    {
        return Err(profile_error(format!(
            "sandbox {label} path '{}' contains characters not allowed in SBPL",
            path.display()
        )));
    }
    if let Some(token) = TEMPLATE_TOKENS.iter().find(|token| text.contains(**token)) {
        return Err(profile_error(format!(
            "sandbox {label} path '{}' contains reserved template token {token}",
            path.display()
        )));
    }
    Ok(text)
}

fn profile_error(message: String) -> HarnessError {
    HarnessError::config_load(
        "sandbox_profile_invalid",
        Some(SANDBOX_EXEC_PATH),
        Some("$.bash.sandbox_profile"),
        Some(serde_json::json!("valid renderable SBPL profile")),
        None,
        message,
    )
}

/// `sandbox-exec` 探测结果（可注入 Fake 以便单测分类）。
#[derive(Debug, Clone, Default)]
pub(crate) struct ProbeOutcome {
    /// 退出码（spawn 失败时 `None`）。
    pub exit_code: Option<i32>,
    /// stderr（分类/诊断用；诊断消息只带前 4 KiB）。
    pub stderr: String,
    /// spawn 错误（二进制不存在/不可执行）。
    pub spawn_error: Option<String>,
}

/// preflight 探测抽象（生产用 [`SandboxExecProbe`]；测试用 Fake）。
pub(crate) trait SandboxProbe: Send + Sync {
    /// 用给定 profile 探测沙箱可用性（不执行用户命令）。
    fn probe(&self, profile: &str) -> ProbeOutcome;
}

/// 真实 `sandbox-exec` 探测（固定路径；本进程上下文）。
#[derive(Debug, Default)]
pub(crate) struct SandboxExecProbe;

impl SandboxProbe for SandboxExecProbe {
    fn probe(&self, profile: &str) -> ProbeOutcome {
        probe_sandbox_exec(profile)
    }
}

/// 真实探测实现：`sandbox-exec -p <profile> /usr/bin/true`（10s 超时）。
///
/// REV-108 N7：stderr 由独立线程**并发 drain**（防 pipe 满阻塞）；超时按进程组
/// kill（`process_group(0)` + killpg SIGKILL），避免探针子进程残留。
fn probe_sandbox_exec(profile: &str) -> ProbeOutcome {
    let path = Path::new(SANDBOX_EXEC_PATH);
    if !path.is_file() {
        return ProbeOutcome {
            spawn_error: Some(format!("{SANDBOX_EXEC_PATH} not found")),
            ..ProbeOutcome::default()
        };
    }
    let mut command = Command::new(path);
    command
        .arg("-p")
        .arg(profile)
        .arg("/usr/bin/true")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(error) => {
            return ProbeOutcome {
                spawn_error: Some(format!("cannot execute {SANDBOX_EXEC_PATH}: {error}")),
                ..ProbeOutcome::default()
            };
        }
    };
    let pid = child.id() as i32;
    let (sender, receiver) = mpsc::channel();
    if let Some(pipe) = child.stderr.take() {
        spawn_probe_stderr_drain(pipe, sender);
    }
    let deadline = Instant::now() + Duration::from_millis(BASH_PREFLIGHT_TIMEOUT_MS);
    let outcome = loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                break ProbeOutcome {
                    exit_code: status.code(),
                    stderr: String::new(),
                    spawn_error: None,
                }
            }
            Ok(None) => {
                if Instant::now() >= deadline {
                    #[cfg(unix)]
                    let _ = killpg(Pid::from_raw(pid), Signal::SIGKILL);
                    let _ = child.kill();
                    let _ = child.wait();
                    break ProbeOutcome {
                        spawn_error: Some(format!(
                            "sandbox preflight timed out after {BASH_PREFLIGHT_TIMEOUT_MS} ms"
                        )),
                        ..ProbeOutcome::default()
                    };
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(error) => {
                #[cfg(unix)]
                let _ = killpg(Pid::from_raw(pid), Signal::SIGKILL);
                let _ = child.kill();
                break ProbeOutcome {
                    spawn_error: Some(format!("sandbox preflight wait failed: {error}")),
                    ..ProbeOutcome::default()
                };
            }
        }
    };
    // 子进程退出后回收 stderr（线程已在 drain；最多再等 1s）。
    let stderr = match receiver.recv_timeout(Duration::from_secs(1)) {
        Ok(text) => text,
        Err(_) => {
            #[cfg(unix)]
            let _ = killpg(Pid::from_raw(pid), Signal::SIGKILL);
            receiver
                .recv_timeout(Duration::from_millis(200))
                .unwrap_or_default()
        }
    };
    ProbeOutcome { stderr, ..outcome }
}

/// stderr 并发 drain（最多保留 64 KiB，其余读空丢弃；线程退出即关闭）。
fn spawn_probe_stderr_drain(mut pipe: impl Read + Send + 'static, sender: mpsc::Sender<String>) {
    std::thread::spawn(move || {
        const KEEP: usize = 64 * 1024;
        let mut kept: Vec<u8> = Vec::new();
        let mut buffer = [0u8; 8_192];
        loop {
            match pipe.read(&mut buffer) {
                Ok(0) => break,
                Ok(read) => {
                    if kept.len() < KEEP {
                        let room = KEEP - kept.len();
                        let take = read.min(room);
                        kept.extend_from_slice(&buffer[..take]);
                    }
                }
                Err(_) => break,
            }
        }
        let _ = sender.send(String::from_utf8_lossy(&kept).to_string());
    });
}

/// 用给定 probe 做 preflight；失败按 exit 71/65/其它分类为三种 `HARNESS_CONFIG_LOAD`。
pub(crate) fn preflight_with(
    probe: &dyn SandboxProbe,
    profile: &str,
) -> Result<ProbeOutcome, HarnessError> {
    let outcome = probe.probe(profile);
    match classify_probe(&outcome) {
        Ok(()) => Ok(outcome),
        Err(reason) => Err(sandbox_config_error(reason, &outcome)),
    }
}

/// 分类：返回 `Err(reason)` 或 `Ok(())`。
pub(crate) fn classify_probe(outcome: &ProbeOutcome) -> Result<(), &'static str> {
    if outcome.spawn_error.is_some() {
        return Err("sandbox_unavailable");
    }
    match outcome.exit_code {
        Some(0) => Ok(()),
        Some(71) => Err("sandbox_nested"),
        Some(65) => Err("sandbox_profile_invalid"),
        _ => Err("sandbox_unavailable"),
    }
}

fn sandbox_config_error(reason: &'static str, outcome: &ProbeOutcome) -> HarnessError {
    let detail = if let Some(error) = &outcome.spawn_error {
        error.clone()
    } else {
        format!(
            "exit={:?}; stderr: {}",
            outcome.exit_code,
            truncate_diagnostic(&outcome.stderr)
        )
    };
    let message = match reason {
        "sandbox_nested" => format!(
            "macOS Seatbelt sandbox nesting detected (sandbox-exec exit 71): zgraph is running inside another sandbox; --allow-bash requires unsandboxed host. {detail}"
        ),
        "sandbox_profile_invalid" => {
            format!("sandbox profile rejected by sandbox-exec (exit 65): {detail}")
        }
        _ => format!("macOS sandbox-exec unavailable: {detail}"),
    };
    HarnessError::config_load(
        reason,
        Some(SANDBOX_EXEC_PATH),
        Some("$.bash.sandbox"),
        Some(serde_json::json!("sandbox-exec preflight exit 0")),
        Some(serde_json::json!(outcome.exit_code)),
        message,
    )
}

fn truncate_diagnostic(stderr: &str) -> String {
    let limit = 4_096;
    if stderr.len() <= limit {
        return stderr.trim().to_string();
    }
    let mut end = limit;
    while end > 0 && !stderr.is_char_boundary(end) {
        end -= 1;
    }
    stderr[..end].trim().to_string()
}

/// 进程控制句柄（超时/取消时由监督方请求 kill 进程组）。
#[derive(Debug, Default)]
pub(crate) struct ProcessControl {
    killed: AtomicBool,
    pgid: Mutex<Option<i32>>,
}

impl ProcessControl {
    /// 新建控制句柄。
    pub(crate) fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    /// 请求终止（SIGTERM→2s→SIGKILL 由执行线程完成）。
    pub(crate) fn request_kill(&self) {
        self.killed.store(true, Ordering::SeqCst);
    }

    /// 是否已请求终止。
    pub(crate) fn is_killed(&self) -> bool {
        self.killed.load(Ordering::SeqCst)
    }

    fn set_pgid(&self, pid: i32) {
        if let Ok(mut guard) = self.pgid.lock() {
            *guard = Some(pid);
        }
    }

    #[cfg(test)]
    pub(crate) fn pgid(&self) -> Option<i32> {
        self.pgid.lock().ok().and_then(|guard| *guard)
    }

    fn signal_group(&self, signal: Signal) -> bool {
        let Ok(guard) = self.pgid.lock() else {
            return false;
        };
        let Some(pgid) = *guard else {
            return false;
        };
        killpg(Pid::from_raw(pgid), signal).is_ok()
    }
}

/// 一次受限命令的输出（drain 上限内原始字节；head+tail 由 bash 工具收口）。
#[derive(Debug, Clone, Default)]
pub(crate) struct SupervisedOutput {
    /// stdout 原始字节（≤ [`BASH_DRAIN_MAX_BYTES`]）。
    pub stdout: Vec<u8>,
    /// stderr 原始字节（≤ [`BASH_DRAIN_MAX_BYTES`]）。
    pub stderr: Vec<u8>,
    /// 退出码（信号终止时 `None`）。
    pub exit_code: Option<i32>,
    /// 终止信号（正常退出时 `None`）。
    pub signal: Option<i32>,
    /// 是否发生 drain 截断（超过 2 MiB）。
    pub drain_truncated: bool,
    /// 耗时（毫秒）。
    pub duration_ms: u64,
}

/// 监督执行：drain、超时/取消、进程组回收；输出 raw 字节。
///
/// 失败 → [`ToolFailure`]：超时 `timeout`、取消 `cancelled`、沙箱故障
/// `sandbox_failure`；**正常非 0 退出不算失败**（由 bash 工具作为结果返回）。
pub(crate) fn run_supervised(
    mut command: Command,
    timeout: Duration,
    control: &ProcessControl,
) -> Result<SupervisedOutput, ToolFailure> {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let started = Instant::now();
    let mut child = command.spawn().map_err(|error| {
        ToolFailure::new(
            "sandbox_failure",
            format!("failed to start sandboxed command: {error}"),
        )
    })?;
    let pid = child.id() as i32;
    control.set_pgid(pid);
    let stdout_pipe = child.stdout.take();
    let stderr_pipe = child.stderr.take();
    let (sender, receiver) = mpsc::channel();
    let mut drains = 0usize;
    if let Some(pipe) = stdout_pipe {
        spawn_drain(pipe, sender.clone(), true);
        drains += 1;
    }
    if let Some(pipe) = stderr_pipe {
        spawn_drain(pipe, sender, false);
        drains += 1;
    }
    let drain_timeout = Duration::from_millis(BASH_TERMINATE_GRACE_MS);
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let mut drain_truncated = false;
    let mut drained = 0usize;
    let mut timed_out = false;
    let mut cancelled = false;
    let deadline = started + timeout;
    let mut terminated = false;
    loop {
        if control.is_killed() {
            cancelled = true;
            terminate_group(control, &mut child);
            terminated = true;
            break;
        }
        if Instant::now() >= deadline {
            timed_out = true;
            terminate_group(control, &mut child);
            terminated = true;
            break;
        }
        match child.try_wait() {
            Ok(Some(_status)) => break,
            Ok(None) => std::thread::sleep(Duration::from_millis(10)),
            Err(error) => {
                cleanup_group(control);
                return Err(ToolFailure::new(
                    "sandbox_failure",
                    format!("failed to poll sandboxed command: {error}"),
                ));
            }
        }
    }
    let status = if terminated {
        let _ = child.wait();
        None
    } else {
        child.try_wait().ok().flatten()
    };
    // 子进程退出后清理残留进程组（孙进程），并回收 drain 线程。
    cleanup_group(control);
    let wait = if terminated {
        Duration::ZERO
    } else {
        drain_timeout
    };
    let wait_deadline = Instant::now() + wait;
    while drained < drains {
        let remaining = wait_deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            break;
        }
        match receiver.recv_timeout(remaining.min(Duration::from_millis(200))) {
            Ok(message) => {
                drained += 1;
                if message.is_stdout {
                    stdout = message.bytes;
                } else {
                    stderr = message.bytes;
                }
                drain_truncated |= message.truncated;
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                if !terminated {
                    cleanup_group(control);
                }
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
    let (exit_code, signal) = match &status {
        Some(status) => (status.code(), {
            #[cfg(unix)]
            {
                use std::os::unix::process::ExitStatusExt;
                status.signal()
            }
            #[cfg(not(unix))]
            {
                None
            }
        }),
        None => (
            None,
            if cancelled || timed_out {
                Some(15)
            } else {
                None
            },
        ),
    };
    // 运行期沙箱故障（探测后异常/嵌套）→ fail-closed。
    let stderr_text = String::from_utf8_lossy(&stderr).to_string();
    if let Some(code) = exit_code {
        if code == 71 && stderr_text.contains("sandbox_apply") {
            return Err(ToolFailure::new(
                "sandbox_failure",
                "sandbox-exec failed at runtime (nested Seatbelt?)",
            ));
        }
        if code == 65 && stderr_text.contains("sandbox-exec") {
            return Err(ToolFailure::new(
                "sandbox_failure",
                "sandbox-exec rejected the profile at runtime",
            ));
        }
    }
    if timed_out {
        return Err(ToolFailure::new(
            "timeout",
            format!("bash command timed out after {} ms", timeout.as_millis()),
        ));
    }
    if cancelled {
        return Err(ToolFailure::new("cancelled", "bash command was cancelled"));
    }
    Ok(SupervisedOutput {
        stdout,
        stderr,
        exit_code,
        signal,
        drain_truncated,
        duration_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
    })
}

#[derive(Debug)]
struct DrainMessage {
    is_stdout: bool,
    bytes: Vec<u8>,
    truncated: bool,
}

fn spawn_drain(
    mut pipe: impl Read + Send + 'static,
    sender: mpsc::Sender<DrainMessage>,
    is_stdout: bool,
) {
    std::thread::spawn(move || {
        let mut kept = Vec::new();
        let mut truncated = false;
        let mut buffer = [0u8; 8_192];
        loop {
            match pipe.read(&mut buffer) {
                Ok(0) => break,
                Ok(read) => {
                    if kept.len() < BASH_DRAIN_MAX_BYTES {
                        let room = BASH_DRAIN_MAX_BYTES - kept.len();
                        let take = read.min(room);
                        kept.extend_from_slice(&buffer[..take]);
                        if take < read {
                            truncated = true;
                        }
                    } else {
                        truncated = true;
                    }
                }
                Err(_) => break,
            }
        }
        let _ = sender.send(DrainMessage {
            is_stdout,
            bytes: kept,
            truncated,
        });
    });
}

fn terminate_group(control: &ProcessControl, child: &mut std::process::Child) {
    control.signal_group(Signal::SIGTERM);
    let deadline = Instant::now() + Duration::from_millis(BASH_TERMINATE_GRACE_MS);
    while Instant::now() < deadline {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) => std::thread::sleep(Duration::from_millis(20)),
            Err(_) => break,
        }
    }
    control.signal_group(Signal::SIGKILL);
}

fn cleanup_group(control: &ProcessControl) {
    control.signal_group(Signal::SIGKILL);
}

/// 输出 head+tail 收口（≤16 KiB；含 `…[truncated]…` 标记）。
pub(crate) fn head_tail(bytes: &[u8], max_bytes: usize) -> (String, bool) {
    if bytes.len() <= max_bytes {
        return (String::from_utf8_lossy(bytes).to_string(), false);
    }
    let marker = "\n…[truncated]…\n";
    let budget = max_bytes.saturating_sub(marker.len());
    let head_budget = budget / 2;
    let tail_budget = budget - head_budget;
    let mut head_end = head_budget.min(bytes.len());
    while head_end > 0 && !is_utf8_boundary(bytes, head_end) {
        head_end -= 1;
    }
    let mut tail_start = bytes.len().saturating_sub(tail_budget);
    while tail_start < bytes.len() && !is_utf8_boundary(bytes, tail_start) {
        tail_start += 1;
    }
    let mut text = String::from_utf8_lossy(&bytes[..head_end]).to_string();
    text.push_str(marker);
    text.push_str(&String::from_utf8_lossy(&bytes[tail_start..]));
    (text, true)
}

fn is_utf8_boundary(bytes: &[u8], index: usize) -> bool {
    std::str::from_utf8(&bytes[..index]).is_ok()
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut out = String::with_capacity(64);
    for byte in digest {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    use tempfile::TempDir;

    fn temp_canonical() -> (TempDir, PathBuf) {
        let dir = TempDir::new().expect("tempdir");
        let canonical = std::fs::canonicalize(dir.path()).expect("canonical");
        (dir, canonical)
    }

    #[test]
    fn renders_profile_with_workspace_allow_paths_and_deny_network() {
        let (_workspace_dir, workspace) = temp_canonical();
        let (_allow_dir, allow) = temp_canonical();
        let (_exec_dir, exec) = temp_canonical();
        let rendered = render_profile(&SandboxProfileConfig {
            workspace: workspace.clone(),
            allow_paths: vec![allow.clone()],
            exec_paths: vec![exec.clone()],
            allow_network: false,
        })
        .expect("render");
        assert!(rendered.text.starts_with("(version 1)\n(deny default)"));
        let workspace_text = workspace.to_string_lossy().to_string();
        assert!(
            rendered.text.contains(&format!(
                "(allow file-read* file-write* file-test-existence (subpath \"{workspace_text}\"))"
            )),
            "{}",
            rendered.text
        );
        let allow_text = allow.to_string_lossy().to_string();
        assert!(rendered
            .text
            .contains(&format!("(subpath \"{allow_text}\")")));
        // 只读 allow-path：只出现在 file-read* 行，不出现 file-write*。
        assert!(!rendered.text.contains(&format!(
            "file-write* file-test-existence (subpath \"{allow_text}\")"
        )));
        // exec path 位于 process-exec allow 块内。
        let exec_section = rendered
            .text
            .split("(allow process-exec")
            .nth(1)
            .and_then(|rest| rest.split(";;;").next())
            .expect("process-exec block");
        assert!(
            exec_section.contains(&exec.to_string_lossy().to_string()),
            "{exec_section}"
        );
        // REV-108 N2：exec path 同时授予 file-read*（二进制/动态库可读）。
        let exec_read_text = exec.to_string_lossy().to_string();
        let exec_read_line =
            format!("(allow file-read* file-test-existence (subpath \"{exec_read_text}\"))");
        assert!(rendered.text.contains(&exec_read_line), "{}", rendered.text);
        assert!(rendered.text.contains("(deny network*)"));
        assert!(!rendered.text.contains("@WORKSPACE@"));
        assert!(!rendered.text.contains("@ALLOW_PATHS@"));
        assert_eq!(rendered.sha256.len(), 64);
        assert!(
            rendered.text.contains("/private/"),
            "macOS realpath /private form"
        );
        assert!(rendered
            .text
            .contains("(literal \"/private/var/select/sh\")"));
    }

    #[test]
    fn renders_network_allow_block_when_requested() {
        let (_dir, workspace) = temp_canonical();
        let rendered = render_profile(&SandboxProfileConfig {
            workspace,
            allow_paths: Vec::new(),
            exec_paths: Vec::new(),
            allow_network: true,
        })
        .expect("render");
        assert!(rendered.text.contains("(allow network*)"));
        assert!(!rendered.text.contains("(deny network*)"));
        assert!(rendered.text.contains("/private/etc/resolv.conf"));
        assert!(rendered.text.contains("com.apple.networkd"));
    }

    #[test]
    fn rejects_injection_characters_in_paths() {
        let (dir, _canonical) = temp_canonical();
        let bad = dir.path().join("bad\"name");
        std::fs::create_dir_all(&bad).expect("mkdir");
        let error = render_profile(&SandboxProfileConfig {
            workspace: bad,
            allow_paths: Vec::new(),
            exec_paths: Vec::new(),
            allow_network: false,
        })
        .expect_err("injection rejected");
        assert_eq!(
            error.detail["reason"],
            serde_json::json!("sandbox_profile_invalid")
        );
        assert!(error.message.contains("not allowed in SBPL"));
    }

    #[test]
    fn rejects_missing_paths() {
        let (dir, _canonical) = temp_canonical();
        let error = render_profile(&SandboxProfileConfig {
            workspace: dir.path().join("missing"),
            allow_paths: Vec::new(),
            exec_paths: Vec::new(),
            allow_network: false,
        })
        .expect_err("missing workspace");
        assert_eq!(
            error.detail["reason"],
            serde_json::json!("sandbox_profile_invalid")
        );
    }

    struct FakeProbe {
        outcome: ProbeOutcome,
        seen: Mutex<Vec<String>>,
    }

    impl FakeProbe {
        fn new(outcome: ProbeOutcome) -> Self {
            Self {
                outcome,
                seen: Mutex::new(Vec::new()),
            }
        }
    }

    impl SandboxProbe for FakeProbe {
        fn probe(&self, profile: &str) -> ProbeOutcome {
            if let Ok(mut guard) = self.seen.lock() {
                guard.push(profile.to_string());
            }
            self.outcome.clone()
        }
    }

    fn outcome(exit_code: Option<i32>) -> ProbeOutcome {
        ProbeOutcome {
            exit_code,
            stderr: String::new(),
            spawn_error: None,
        }
    }

    #[test]
    fn classifies_probe_outcomes_fail_closed() {
        assert!(classify_probe(&outcome(Some(0))).is_ok());
        assert_eq!(
            classify_probe(&outcome(Some(71))).unwrap_err(),
            "sandbox_nested"
        );
        assert_eq!(
            classify_probe(&outcome(Some(65))).unwrap_err(),
            "sandbox_profile_invalid"
        );
        assert_eq!(
            classify_probe(&outcome(Some(1))).unwrap_err(),
            "sandbox_unavailable"
        );
        let spawn = ProbeOutcome {
            spawn_error: Some("missing".to_string()),
            ..ProbeOutcome::default()
        };
        assert_eq!(classify_probe(&spawn).unwrap_err(), "sandbox_unavailable");
    }

    #[test]
    fn preflight_maps_classification_to_config_load_reason() {
        let nested =
            preflight_with(&FakeProbe::new(outcome(Some(71))), "(version 1)").expect_err("nested");
        assert_eq!(nested.kind, zgraph_kernel::HarnessErrorKind::ConfigLoad);
        assert_eq!(nested.detail["reason"], serde_json::json!("sandbox_nested"));
        assert!(nested.message.contains("nesting detected"));

        let invalid = preflight_with(&FakeProbe::new(outcome(Some(65))), "(version 1)")
            .expect_err("invalid profile");
        assert_eq!(
            invalid.detail["reason"],
            serde_json::json!("sandbox_profile_invalid")
        );

        let unavailable = preflight_with(&FakeProbe::new(outcome(Some(2))), "(version 1)")
            .expect_err("unavailable");
        assert_eq!(
            unavailable.detail["reason"],
            serde_json::json!("sandbox_unavailable")
        );
        preflight_with(&FakeProbe::new(outcome(Some(0))), "(version 1)").expect("available");
    }

    #[test]
    fn head_tail_caps_output_with_marker_and_keeps_utf8() {
        let small = b"hello world";
        let (text, truncated) = head_tail(small, 16);
        assert_eq!(text, "hello world");
        assert!(!truncated);
        let repeated = "é".repeat(100_000);
        let (text, truncated) = head_tail(repeated.as_bytes(), 16_384);
        assert!(truncated);
        assert!(text.len() <= 16_384 + 64, "{}", text.len());
        assert!(text.contains("…[truncated]…"));
        assert!(std::str::from_utf8(text.as_bytes()).is_ok());
    }

    #[test]
    fn supervisor_returns_stdout_and_nonzero_exit_as_result() {
        let mut command = Command::new("/bin/sh");
        command.args(["-c", "printf hello"]);
        let output =
            run_supervised(command, Duration::from_secs(5), &ProcessControl::new()).expect("run");
        assert_eq!(output.exit_code, Some(0));
        assert_eq!(output.stdout, b"hello");
        assert!(output.duration_ms < 5_000);

        let mut command = Command::new("/bin/sh");
        command.args(["-c", "exit 3"]);
        let output =
            run_supervised(command, Duration::from_secs(5), &ProcessControl::new()).expect("run");
        assert_eq!(output.exit_code, Some(3));
        assert!(output.stderr.is_empty());
    }

    #[test]
    fn supervisor_times_out_and_kills_process_group() {
        let control = ProcessControl::new();
        let mut command = Command::new("/bin/sh");
        command.args(["-c", "sleep 30 & sleep 30"]);
        let started = Instant::now();
        let error =
            run_supervised(command, Duration::from_millis(200), &control).expect_err("timeout");
        assert_eq!(error.reason, "timeout");
        assert!(
            started.elapsed() < Duration::from_secs(8),
            "timeout kill took {:?}",
            started.elapsed()
        );
        let pgid = control.pgid().expect("pgid");
        let mut gone = false;
        for _ in 0..100 {
            if killpg(Pid::from_raw(pgid), None).is_err() {
                gone = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(gone, "process group {pgid} still alive after timeout");
    }

    #[test]
    fn supervisor_cancels_via_process_control() {
        let control = ProcessControl::new();
        let mut command = Command::new("/bin/sh");
        command.args(["-c", "sleep 30"]);
        let killer = control.clone();
        let handle = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(150));
            killer.request_kill();
        });
        let started = Instant::now();
        let error =
            run_supervised(command, Duration::from_secs(30), &control).expect_err("cancelled");
        assert_eq!(error.reason, "cancelled");
        assert!(started.elapsed() < Duration::from_secs(8));
        let _ = handle.join();
    }

    #[test]
    fn supervisor_caps_drain_but_keeps_reading() {
        let mut command = Command::new("/bin/sh");
        command.args(["-c", "head -c 2500000 /dev/zero"]);
        let output =
            run_supervised(command, Duration::from_secs(10), &ProcessControl::new()).expect("run");
        assert_eq!(output.exit_code, Some(0));
        assert!(output.drain_truncated);
        assert_eq!(output.stdout.len(), BASH_DRAIN_MAX_BYTES);
    }

    #[test]
    fn probe_test_helper_never_spawns_for_fake() {
        let probe = FakeProbe::new(outcome(Some(0)));
        preflight_with(&probe, "profile").expect("ok");
        assert_eq!(probe.seen.lock().expect("lock").len(), 1);
    }
    /// 真沙箱验证（环境感知、fail-closed）：
    ///
    /// - 本 agent/CI 会话若运行在**嵌套 Seatbelt** 中，`sandbox-exec` 必然 exit 71 →
    ///   预期 `HARNESS_CONFIG_LOAD{reason:"sandbox_nested"}`（硬失败，不降级）；测试
    ///   通过并显式打印环境限制（**不伪造沙箱成功**）。
    /// - 在普通终端（未被沙箱化）中运行时，则真实执行隔离断言：workspace 写成功、
    ///   `/etc/passwd` 读被拒、`/tmp` 写被拒、网络默认 deny。
    ///
    /// 普通终端验证入口：`crates/zgraph-harness/scripts/verify-sandbox.sh`。
    #[test]
    fn real_sandbox_preflight_and_isolation_environment_aware() {
        use crate::builtin::bash::{build_bash_env, BashOptions, BashRequest, MacOsSandbox};
        use std::collections::BTreeMap;

        let (_dir, workspace) = temp_canonical();
        std::fs::write(workspace.join("seed.txt"), "seed").expect("seed");
        let profile = render_profile(&SandboxProfileConfig {
            workspace: workspace.clone(),
            allow_paths: Vec::new(),
            exec_paths: Vec::new(),
            allow_network: false,
        })
        .expect("render");
        match preflight_with(&SandboxExecProbe, &profile.text) {
            Err(error) => {
                let reason = error
                    .detail
                    .get("reason")
                    .and_then(|value| value.as_str())
                    .unwrap_or("unknown");
                assert!(
                    matches!(reason, "sandbox_nested" | "sandbox_unavailable"),
                    "unexpected real-sandbox classification: {reason}: {}",
                    error.message
                );
                assert!(
                    !error.message.is_empty(),
                    "preflight failure must carry diagnostics (stderr drain)"
                );
                eprintln!(
                    "[D4 real-sandbox] OS sandbox unavailable in this environment: \
                     reason={reason} (fail-closed verified; diagnostics: {}). Run \
                     crates/zgraph-harness/scripts/verify-sandbox.sh in a normal terminal \
                     to exercise the isolation assertions.",
                    error.message
                );
            }
            Ok(_) => {
                let runner = MacOsSandbox::new(&workspace, &[], &BashOptions::default())
                    .expect("preflight already succeeded");
                let tmpdir = workspace.join(".tmp");
                std::fs::create_dir_all(&tmpdir).expect("tmpdir");
                let env = build_bash_env(&workspace, &tmpdir, &BTreeMap::new()).expect("env");
                let run = |command: &str| {
                    runner
                        .run(
                            &BashRequest {
                                command: command.to_string(),
                                cwd: workspace.clone(),
                                timeout: Duration::from_secs(30),
                                env: env.clone(),
                            },
                            &ProcessControl::new(),
                        )
                        .expect("sandboxed run")
                };

                let write = run("echo inside > inside.txt");
                assert_eq!(write.exit_code, Some(0), "{:?}", write.stderr);
                assert_eq!(
                    std::fs::read_to_string(workspace.join("inside.txt")).expect("read"),
                    "inside\n"
                );

                let read_etc = run("cat /etc/passwd");
                assert_ne!(
                    read_etc.exit_code,
                    Some(0),
                    "reading /etc/passwd must be denied"
                );

                let write_tmp = run("echo x > /tmp/zgraph_d4_probe");
                assert_ne!(write_tmp.exit_code, Some(0), "writing /tmp must be denied");

                let network = run("python3 -c 'import socket; socket.create_connection((\"1.1.1.1\", 80), timeout=3)'");
                assert_ne!(
                    network.exit_code,
                    Some(0),
                    "network must be denied by default"
                );
            }
        }
    }
    /// REV-108 C2：路径含 SBPL 模板 token → 渲染前拒绝（防二次替换注入）。
    #[test]
    fn rejects_template_tokens_in_rendered_paths() {
        for token in [
            "@WORKSPACE@",
            "@EXEC_PATHS@",
            "@ALLOW_PATHS@",
            "@EXEC_READS@",
            "@NETWORK@",
        ] {
            let parent = TempDir::new().expect("parent");
            let workspace = parent.path().join(token);
            std::fs::create_dir_all(&workspace).expect("workspace dir");
            let error = render_profile(&SandboxProfileConfig {
                workspace,
                allow_paths: Vec::new(),
                exec_paths: Vec::new(),
                allow_network: false,
            })
            .expect_err(token);
            assert_eq!(
                error.detail["reason"],
                serde_json::json!("sandbox_profile_invalid"),
                "{token}"
            );
            assert!(
                error.message.contains("reserved template token"),
                "{token}: {}",
                error.message
            );
        }
        // allow-path / exec-path 同样拒绝。
        let (_dir, workspace) = temp_canonical();
        let parent = TempDir::new().expect("parent");
        let bad_allow = parent.path().join("@ALLOW_PATHS@");
        std::fs::create_dir_all(&bad_allow).expect("allow dir");
        let error = render_profile(&SandboxProfileConfig {
            workspace: workspace.clone(),
            allow_paths: vec![bad_allow],
            exec_paths: Vec::new(),
            allow_network: false,
        })
        .expect_err("allow token");
        assert_eq!(
            error.detail["reason"],
            serde_json::json!("sandbox_profile_invalid")
        );
        let bad_exec = parent.path().join("@NETWORK@");
        std::fs::create_dir_all(&bad_exec).expect("exec dir");
        let error = render_profile(&SandboxProfileConfig {
            workspace,
            allow_paths: Vec::new(),
            exec_paths: vec![bad_exec],
            allow_network: false,
        })
        .expect_err("exec token");
        assert_eq!(
            error.detail["reason"],
            serde_json::json!("sandbox_profile_invalid")
        );
    }

    #[test]
    fn rejects_backslash_and_control_chars_in_paths() {
        let parent = TempDir::new().expect("parent");
        let backslash = parent.path().join("bad\\name");
        std::fs::create_dir_all(&backslash).expect("backslash dir");
        let error = render_profile(&SandboxProfileConfig {
            workspace: backslash,
            allow_paths: Vec::new(),
            exec_paths: Vec::new(),
            allow_network: false,
        })
        .expect_err("backslash");
        assert_eq!(
            error.detail["reason"],
            serde_json::json!("sandbox_profile_invalid")
        );

        let newline = parent.path().join("bad\nname");
        std::fs::create_dir_all(&newline).expect("newline dir");
        let error = render_profile(&SandboxProfileConfig {
            workspace: newline,
            allow_paths: Vec::new(),
            exec_paths: Vec::new(),
            allow_network: false,
        })
        .expect_err("newline");
        assert_eq!(
            error.detail["reason"],
            serde_json::json!("sandbox_profile_invalid")
        );
    }

    /// REV-108 N7：preflight stderr drain 线程在有界队列外继续读空，不阻塞、不无限增长。
    #[test]
    fn probe_stderr_drain_is_bounded_and_completes() {
        let payload = vec![b'x'; 200_000];
        let (sender, receiver) = mpsc::channel();
        spawn_probe_stderr_drain(std::io::Cursor::new(payload), sender);
        let text = receiver
            .recv_timeout(Duration::from_secs(5))
            .expect("drain thread finished");
        assert_eq!(text.len(), 64 * 1024, "only first 64 KiB retained");
        assert!(text.bytes().all(|byte| byte == b'x'));
    }
}
