//! `builtin__bash`：内置 shell 工具（DR-013 §3.8/§5；v0.4/D4）。
//!
//! 安全模型（全部 fail-closed，无 fallback/无 `--unsafe-bash`）：
//! - 命令固定 `/bin/bash --noprofile --norc -c <command>`，在 macOS `sandbox-exec`
//!   profile 内执行（见 [`super::sandbox`]）；
//! - `cwd` 必须是 jail 内目录（canonicalize）；`HOME=<workspace>`、
//!   `TMPDIR=<workspace>/.tmp`；env 白名单重建（剔除 `BASH_ENV`/`LD_*`/`DYLD_*` 等）；
//! - stdin=null；新进程组 + SIGTERM 2s→SIGKILL；drain 2 MiB；结果各 16 KiB
//!   head+tail（truncated 标记）；
//! - 非 0 退出 = 正常工具结果；超时 → `TOOL_ERROR{timeout}`；运行期沙箱故障 →
//!   `TOOL_ERROR{sandbox_failure}`。
//!
//! 沙箱不可用/嵌套/profile 非法属于**配置期**（`HARNESS_CONFIG_LOAD{reason}`，CLI
//! run 预检 exit 3）；运行期异常才是 `sandbox_failure`。

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::time::Duration;

use serde_json::{json, Value};

use crate::builtin::files::{optional_u64, required_str, BuiltinToolSet, ToolCallMetadata};
use crate::builtin::jail::Access;
use crate::builtin::sandbox::{
    head_tail, preflight_with, render_profile, run_supervised, ProcessControl, SandboxExecProbe,
    SandboxProfileConfig,
};
use crate::constants::{
    BASH_COMMAND_MAX_BYTES, BASH_DEFAULT_TIMEOUT_MS, BASH_MAX_TIMEOUT_MS, BASH_STDERR_MAX_BYTES,
    BASH_STDOUT_MAX_BYTES, SANDBOX_EXEC_PATH,
};
use crate::error::HarnessError;
use crate::tools::{ToolDef, ToolFailure};

/// bash 工具名（冻结；DR-013 §3.8）。
pub(crate) const TOOL_BASH: &str = "builtin__bash";

/// 固定 PATH（系统目录 + CLT；不依赖调用方 PATH）。
pub(crate) const BASH_FIXED_PATH: &str =
    "/Library/Developer/CommandLineTools/usr/bin:/usr/bin:/bin:/usr/sbin:/sbin";

/// bazh 结果 `sandbox` 标识。
pub(crate) const BASH_SANDBOX_NAME: &str = "macos-sandbox-exec";

/// 环境白名单（从父进程继承的键；HOME/TMPDIR/PATH 由 jail 强制重写）。
const ENV_WHITELIST: &[&str] = &[
    "LANG",
    "LC_ALL",
    "LC_CTYPE",
    "SHELL",
    "USER",
    "LOGNAME",
    "DEVELOPER_DIR",
    "SDKROOT",
];

/// 显式 `env` 禁止的键（防注入；大小写不敏感）。
const ENV_DENY_KEYS: &[&str] = &[
    "BASH_ENV",
    "ENV",
    "PYTHONPATH",
    "PYTHONSTARTUP",
    "NODE_OPTIONS",
    "SSH_AUTH_SOCK",
    "HTTP_PROXY",
    "HTTPS_PROXY",
    "ALL_PROXY",
    "NO_PROXY",
    "PWD",
    "OLDPWD",
];

/// 显式 `env` 不得覆盖的受保护键（jail 语义）。
const ENV_PROTECTED_KEYS: &[&str] = &["HOME", "TMPDIR", "PATH"];

/// 一次沙箱命令请求（runner 只负责执行，不加策略）。
#[derive(Debug, Clone)]
pub(crate) struct BashRequest {
    /// 用户命令（≤8 KiB）。
    pub command: String,
    /// 已 canonicalize 且位于 jail 内的 cwd。
    pub cwd: PathBuf,
    /// 超时。
    pub timeout: Duration,
    /// 白名单重建后的完整环境变量。
    pub env: BTreeMap<String, String>,
}

/// 一次沙箱命令结果（原始 drain 字节，≤2 MiB/路；head+tail 由工具收口）。
#[derive(Debug, Clone, Default)]
pub(crate) struct BashOutcome {
    /// 退出码（信号终止时 `None`）。
    pub exit_code: Option<i32>,
    /// 终止信号（正常退出时 `None`）。
    pub signal: Option<i32>,
    /// stdout 原始字节。
    pub stdout: Vec<u8>,
    /// stderr 原始字节。
    pub stderr: Vec<u8>,
    /// drain 是否截断（>2 MiB）。
    pub drain_truncated: bool,
    /// 耗时（毫秒）。
    pub duration_ms: u64,
}

/// `--allow-bash` 的选项（CLI 侧构造；渲染 profile + preflight 在构造期完成）。
#[derive(Debug, Clone, Default)]
pub struct BashOptions {
    /// 额外可执行目录（`--allow-exec-path`；只读 allow-path ≠ 可执行）。
    pub exec_paths: Vec<PathBuf>,
    /// 是否放开网络（`--allow-network`；默认 deny）。
    pub allow_network: bool,
}

/// 沙箱执行器抽象（生产 = [`MacOsSandbox`]；测试 = Fake）。
pub(crate) trait SandboxRunner: Send + Sync {
    /// 执行一次沙箱命令；超时/取消返回 [`ToolFailure`]。
    fn run(
        &self,
        request: &BashRequest,
        control: &ProcessControl,
    ) -> Result<BashOutcome, ToolFailure>;

    /// 沙箱标识（结果 `sandbox` 字段）。
    fn sandbox_name(&self) -> &'static str {
        BASH_SANDBOX_NAME
    }

    /// profile sha256（D5 `tool_calls.sandbox_profile_sha256`）。
    #[allow(dead_code)] // D5 trace v7 接线（本波只冻结接口）
    fn profile_sha256(&self) -> &str;
}

/// macOS `sandbox-exec` 实现。
pub(crate) struct MacOsSandbox {
    profile: String,
    /// profile sha256（D5 `tool_calls` 记录；本波冻结接口）。
    #[allow(dead_code)]
    sha256: String,
}

impl MacOsSandbox {
    /// 渲染 profile + preflight；失败 → `HARNESS_CONFIG_LOAD{reason}`（不注册 bash）。
    #[allow(clippy::new_ret_no_self)] // 返回 `Arc<dyn SandboxRunner>`（对象安全注入）
    pub(crate) fn new(
        workspace: &Path,
        allow_paths: &[PathBuf],
        options: &BashOptions,
    ) -> Result<Arc<dyn SandboxRunner>, HarnessError> {
        let rendered = render_profile(&SandboxProfileConfig {
            workspace: workspace.to_path_buf(),
            allow_paths: allow_paths.to_vec(),
            exec_paths: options.exec_paths.clone(),
            allow_network: options.allow_network,
        })?;
        preflight_with(&SandboxExecProbe, &rendered.text)?;
        Ok(Arc::new(Self {
            profile: rendered.text,
            sha256: rendered.sha256,
        }))
    }
}

impl SandboxRunner for MacOsSandbox {
    fn run(
        &self,
        request: &BashRequest,
        control: &ProcessControl,
    ) -> Result<BashOutcome, ToolFailure> {
        let mut command = Command::new(SANDBOX_EXEC_PATH);
        command
            .arg("-p")
            .arg(&self.profile)
            .arg("/bin/bash")
            .arg("--noprofile")
            .arg("--norc")
            .arg("-c")
            .arg(&request.command)
            .current_dir(&request.cwd)
            .env_clear();
        for (key, value) in &request.env {
            command.env(key, value);
        }
        let output = run_supervised(command, request.timeout, control)?;
        Ok(BashOutcome {
            exit_code: output.exit_code,
            signal: output.signal,
            stdout: output.stdout,
            stderr: output.stderr,
            drain_truncated: output.drain_truncated,
            duration_ms: output.duration_ms,
        })
    }

    #[allow(dead_code)] // D5 接线
    fn profile_sha256(&self) -> &str {
        &self.sha256
    }
}

/// `builtin__bash` 工具定义。
pub(crate) fn bash_tool_def() -> ToolDef {
    ToolDef {
        name: TOOL_BASH.to_string(),
        description:
            "Run a shell command inside the OS sandbox (macOS sandbox-exec; network denied by default)."
                .to_string(),
        parameters: json!({
            "type": "object",
            "properties": {
                "command": {"type": "string", "minLength": 1,
                            "maxLength": BASH_COMMAND_MAX_BYTES},
                "cwd": {"type": "string", "default": "."},
                "timeout_ms": {"type": "integer", "minimum": 100,
                               "maximum": BASH_MAX_TIMEOUT_MS,
                               "default": BASH_DEFAULT_TIMEOUT_MS},
                "env": {"type": "object",
                        "additionalProperties": {"type": "string"}}
            },
            "required": ["command"],
            "additionalProperties": false
        }),
        timeout_ms: BASH_MAX_TIMEOUT_MS,
        source: "builtin__bash".to_string(),
    }
}

impl BuiltinToolSet {
    /// `builtin__bash` 执行入口（同步；agent_loop 在 blocking 线程池调用并支持取消）。
    ///
    /// D5b1：同时返回 [`ToolCallMetadata::bash`]（command/cwd/exit/signal/输出/
    /// profile_sha256）用于 trace `tool_calls`。
    pub(crate) fn bash(
        &self,
        args: &Value,
        control: &ProcessControl,
    ) -> Result<(Value, ToolCallMetadata), ToolFailure> {
        let request = self.bash_request(args)?;
        let runner = self
            .bash_runner
            .as_ref()
            .ok_or_else(|| ToolFailure::new("unknown_builtin_tool", "bash tool is not enabled"))?;
        let outcome = runner.run(&request, control)?;
        let (stdout, stdout_capped) = head_tail(&outcome.stdout, BASH_STDOUT_MAX_BYTES);
        let (stderr, stderr_capped) = head_tail(&outcome.stderr, BASH_STDERR_MAX_BYTES);
        let truncated = outcome.drain_truncated || stdout_capped || stderr_capped;
        let metadata = ToolCallMetadata {
            bash: Some(crate::tools::BashCallRecord {
                command: request.command.clone(),
                cwd: request.cwd.display().to_string(),
                exit_code: outcome.exit_code,
                signal: outcome.signal,
                stdout: stdout.clone(),
                stderr: stderr.clone(),
                sandbox_profile_sha256: Some(runner.profile_sha256().to_string()),
            }),
            truncated,
            ..ToolCallMetadata::default()
        };
        Ok((
            json!({
                "exit_code": outcome.exit_code,
                "signal": outcome.signal,
                "stdout": stdout,
                "stderr": stderr,
                "truncated": truncated,
                "sandbox": runner.sandbox_name(),
                "duration_ms": outcome.duration_ms,
            }),
            metadata,
        ))
    }

    /// 参数校验 + cwd jail + env 白名单（超时/取消由调度层负责）。
    fn bash_request(&self, args: &Value) -> Result<BashRequest, ToolFailure> {
        let command = required_str(args, "command")?;
        if command.is_empty() {
            return Err(ToolFailure::new(
                "invalid_args",
                "command must be non-empty",
            ));
        }
        if command.len() > BASH_COMMAND_MAX_BYTES {
            return Err(ToolFailure::new(
                "too_large",
                format!("command exceeds {BASH_COMMAND_MAX_BYTES} bytes"),
            ));
        }
        let cwd_raw = args.get("cwd").and_then(Value::as_str).unwrap_or(".");
        let cwd = self.jail.resolve_any(cwd_raw, Access::Read)?;
        if !cwd.is_dir {
            return Err(ToolFailure::new(
                "invalid_args",
                format!("cwd '{cwd_raw}' is not a directory"),
            ));
        }
        let timeout_ms = optional_u64(args, "timeout_ms", BASH_DEFAULT_TIMEOUT_MS)?;
        if !(100..=BASH_MAX_TIMEOUT_MS).contains(&timeout_ms) {
            return Err(ToolFailure::new(
                "invalid_args",
                format!("timeout_ms must be in 100..={BASH_MAX_TIMEOUT_MS}"),
            ));
        }
        let explicit = parse_explicit_env(args.get("env"))?;
        let workspace = self.jail.root().to_path_buf();
        let tmpdir = self.ensure_tmpdir()?;
        let env = build_bash_env(&workspace, &tmpdir, &explicit)?;
        Ok(BashRequest {
            command: command.to_string(),
            cwd: cwd.path,
            timeout: Duration::from_millis(timeout_ms),
            env,
        })
    }

    /// `TMPDIR=<workspace>/.tmp`（创建；best-effort 写入 `.gitignore`）。
    ///
    /// **REV-108 C1（jail 逃逸修复）**：`.tmp`/`.gitignore` 都必须经 [`WorkspaceJail`]
    /// 解析（canonicalize + 组件前缀）。恶意 workspace 把它们做成指向 jail 外的
    /// symlink 时：`.tmp` → fail-closed `path_escape`（绝不使用越界 TMPDIR）；
    /// `.gitignore` → 跳过并 warn（不 fail bash，但**绝不写 jail 外**）。
    /// 创建 `.tmp` 后再二次 canonicalize 校验，杜绝 symlink 竞态写外部。
    fn ensure_tmpdir(&self) -> Result<PathBuf, ToolFailure> {
        let root = self.jail.root().to_path_buf();
        let candidate = root.join(".tmp");
        let tmp = match candidate.symlink_metadata() {
            Ok(_) => {
                // 已存在（含 symlink）：经 jail 解析并跟随到真实路径；出根 → path_escape。
                let resolved = self
                    .jail
                    .resolve_any(".tmp", Access::Write)
                    .map_err(tmpdir_escape_failure)?;
                if !resolved.is_dir {
                    return Err(ToolFailure::new(
                        "invalid_args",
                        "workspace '.tmp' exists but is not a directory",
                    ));
                }
                resolved.path
            }
            Err(_) => {
                // 不存在：父目录是已 canonicalize 的 workspace 根，先创建再二次校验。
                std::fs::create_dir_all(&candidate).map_err(|error| {
                    ToolFailure::new(
                        "io_error",
                        format!("cannot create TMPDIR '{}': {error}", candidate.display()),
                    )
                })?;
                let resolved = self
                    .jail
                    .resolve_any(".tmp", Access::Write)
                    .map_err(tmpdir_escape_failure)?;
                if !resolved.is_dir {
                    return Err(ToolFailure::new(
                        "path_escape",
                        "workspace '.tmp' did not resolve to a directory inside the jail",
                    ));
                }
                resolved.path
            }
        };
        self.append_gitignore_if_safe();
        Ok(tmp)
    }

    /// best-effort 把 `.tmp/` 追加到 workspace `.gitignore`（不写 jail 外、不 fail bash）。
    fn append_gitignore_if_safe(&self) {
        let candidate = self.jail.root().join(".gitignore");
        if candidate.symlink_metadata().is_err() {
            return;
        }
        let resolved = match self.jail.resolve(".gitignore", Access::Write) {
            Ok(resolved) => resolved,
            Err(failure) => {
                eprintln!(
                    "warning: skipping workspace .gitignore update (jail refuses path: {})",
                    failure.message
                );
                return;
            }
        };
        if !resolved.path.is_file() {
            eprintln!("warning: skipping workspace .gitignore update (not a regular file)");
            return;
        }
        let Ok(text) = std::fs::read_to_string(&resolved.path) else {
            return;
        };
        if text
            .lines()
            .any(|line| line.trim() == ".tmp" || line.trim() == ".tmp/")
        {
            return;
        }
        if let Err(error) = std::fs::OpenOptions::new()
            .append(true)
            .open(&resolved.path)
            .and_then(|mut file| {
                use std::io::Write as _;
                writeln!(file, ".tmp/")
            })
        {
            eprintln!("warning: cannot update workspace .gitignore: {error}");
        }
    }
}

/// 解析并校验显式 `env`（白名单追加；危险/受保护键 fail-closed）。
fn parse_explicit_env(value: Option<&Value>) -> Result<BTreeMap<String, String>, ToolFailure> {
    let mut explicit = BTreeMap::new();
    let Some(Value::Object(object)) = value else {
        if value.is_some() {
            return Err(ToolFailure::new("invalid_args", "env must be an object"));
        }
        return Ok(explicit);
    };
    let mut total = 0usize;
    for (key, value) in object {
        if !valid_env_key(key) {
            return Err(ToolFailure::new(
                "invalid_args",
                format!("env key '{key}' is not a valid environment variable name"),
            ));
        }
        if is_denied_env_key(key) {
            return Err(ToolFailure::new(
                "invalid_args",
                format!("env key '{key}' is not allowed (deny-list)"),
            ));
        }
        if ENV_PROTECTED_KEYS.iter().any(|protected| protected == key) {
            return Err(ToolFailure::new(
                "invalid_args",
                format!("env key '{key}' is managed by the sandbox jail"),
            ));
        }
        let Some(text) = value.as_str() else {
            return Err(ToolFailure::new(
                "invalid_args",
                format!("env value for '{key}' must be a string"),
            ));
        };
        if text.contains('\0') {
            return Err(ToolFailure::new(
                "invalid_args",
                format!("env value for '{key}' contains NUL"),
            ));
        }
        total += key.len() + text.len();
        explicit.insert(key.clone(), text.to_string());
    }
    if total > BASH_COMMAND_MAX_BYTES {
        return Err(ToolFailure::new(
            "too_large",
            format!("env exceeds {BASH_COMMAND_MAX_BYTES} bytes"),
        ));
    }
    Ok(explicit)
}

/// 构建 bash 环境：白名单继承 + jail 强制值 + 显式追加（危险键已拒绝）。
pub(crate) fn build_bash_env(
    workspace: &Path,
    tmpdir: &Path,
    explicit: &BTreeMap<String, String>,
) -> Result<BTreeMap<String, String>, ToolFailure> {
    let parent: BTreeMap<String, String> = std::env::vars_os()
        .filter_map(|(key, value)| {
            let key = key.into_string().ok()?;
            let value = value.into_string().ok()?;
            Some((key, value))
        })
        .collect();
    build_bash_env_with_parent(workspace, tmpdir, explicit, &parent)
}

/// 可注入父进程环境的构建实现（测试用；`LC_*` 全量继承，`LD_*`/`DYLD_*` 永不继承）。
pub(crate) fn build_bash_env_with_parent(
    workspace: &Path,
    tmpdir: &Path,
    explicit: &BTreeMap<String, String>,
    parent: &BTreeMap<String, String>,
) -> Result<BTreeMap<String, String>, ToolFailure> {
    let mut env = BTreeMap::new();
    env.insert("HOME".to_string(), workspace.display().to_string());
    env.insert("TMPDIR".to_string(), tmpdir.display().to_string());
    env.insert("PATH".to_string(), BASH_FIXED_PATH.to_string());
    env.insert("SHELL".to_string(), "/bin/bash".to_string());
    for key in ENV_WHITELIST {
        if *key == "SHELL" {
            continue;
        }
        if let Some(value) = parent.get(*key) {
            env.insert((*key).to_string(), value.clone());
        }
    }
    // DR §3.8：继承全部 `LC_*`（locale 子类），但显式剔除危险键。
    for (key, value) in parent {
        if key.starts_with("LC_") && !is_denied_env_key(key) {
            env.insert(key.clone(), value.clone());
        }
    }
    for (key, value) in explicit {
        env.insert(key.clone(), value.clone());
    }
    Ok(env)
}

/// `.tmp` 解析失败的包装：jail 越界 → `path_escape`（fail-closed）。
fn tmpdir_escape_failure(failure: ToolFailure) -> ToolFailure {
    if matches!(
        failure.reason.as_str(),
        "path_escape" | "path_read_only" | "not_regular_file" | "is_directory"
    ) {
        ToolFailure::new(
            "path_escape",
            format!(
                "workspace '.tmp' escapes the jail or is not a directory: {}",
                failure.message
            ),
        )
    } else {
        failure
    }
}

fn valid_env_key(key: &str) -> bool {
    let mut chars = key.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !(first.is_ascii_alphabetic() || first == '_') {
        return false;
    }
    chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
}

fn is_denied_env_key(key: &str) -> bool {
    let upper = key.to_ascii_uppercase();
    ENV_DENY_KEYS.contains(&upper.as_str())
        || upper.starts_with("LD_")
        || upper.starts_with("DYLD_")
        || upper.starts_with("GIT_CONFIG_")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::FakeSandboxRunner;
    use serde_json::json;
    use tempfile::TempDir;

    fn set_with_runner(
        files: bool,
    ) -> (
        TempDir,
        Arc<crate::test_support::FakeSandboxRunner>,
        Arc<BuiltinToolSet>,
    ) {
        let workspace = TempDir::new().expect("workspace");
        let canonical = std::fs::canonicalize(workspace.path()).expect("canonical");
        let runner = FakeSandboxRunner::new();
        let set =
            BuiltinToolSet::new_with_runner(&canonical, &[], files, runner.clone()).expect("set");
        (workspace, runner, set)
    }

    fn call_ok(set: &BuiltinToolSet, args: Value) -> Value {
        let text = set.call(TOOL_BASH, &args).expect("bash ok");
        serde_json::from_str(&text).expect("json")
    }

    fn call_err(set: &BuiltinToolSet, args: Value) -> ToolFailure {
        set.call(TOOL_BASH, &args).expect_err("bash error")
    }

    fn bash_only_set() -> (TempDir, Arc<FakeSandboxRunner>, Arc<BuiltinToolSet>) {
        set_with_runner(false)
    }

    #[test]
    fn bash_registration_follows_opt_in() {
        let (_workspace, _runner, bash_only) = bash_only_set();
        let names: Vec<&str> = bash_only
            .definitions()
            .iter()
            .map(|tool| tool.name.as_str())
            .collect();
        assert_eq!(names, vec![TOOL_BASH], "{names:?}");

        let workspace = TempDir::new().expect("workspace");
        let files_only = BuiltinToolSet::new(workspace.path(), &[]).expect("files");
        assert_eq!(files_only.definitions().len(), 7);
        assert!(!files_only
            .definitions()
            .iter()
            .any(|tool| tool.name == TOOL_BASH));

        let runner = FakeSandboxRunner::new();
        let combined =
            BuiltinToolSet::new_with_runner(workspace.path(), &[], true, runner).expect("combined");
        assert_eq!(combined.definitions().len(), 8, "7 file/search + bash");
    }

    #[test]
    fn validates_arguments_fail_closed() {
        let (_workspace, _runner, set) = bash_only_set();
        assert_eq!(call_err(&set, json!({})).reason, "invalid_args");
        assert_eq!(
            call_err(&set, json!({"command": ""})).reason,
            "invalid_args"
        );
        let huge = "x".repeat(BASH_COMMAND_MAX_BYTES + 1);
        assert_eq!(call_err(&set, json!({"command": huge})).reason, "too_large");
        assert_eq!(
            call_err(&set, json!({"command": "x", "timeout_ms": 99})).reason,
            "invalid_args"
        );
        assert_eq!(
            call_err(
                &set,
                json!({"command": "x", "timeout_ms": BASH_MAX_TIMEOUT_MS + 1})
            )
            .reason,
            "invalid_args"
        );
        assert_eq!(
            call_err(&set, json!({"command": "x", "env": "nope"})).reason,
            "invalid_args"
        );
        assert_eq!(
            call_err(&set, json!({"command": "x", "env": {"BAD-KEY": "v"}})).reason,
            "invalid_args"
        );
        for key in [
            "BASH_ENV",
            "LD_PRELOAD",
            "DYLD_INSERT_LIBRARIES",
            "PYTHONPATH",
        ] {
            assert_eq!(
                call_err(&set, json!({"command": "x", "env": {key: "v"}})).reason,
                "invalid_args",
                "{key}"
            );
        }
        for key in ["HOME", "TMPDIR", "PATH"] {
            assert_eq!(
                call_err(&set, json!({"command": "x", "env": {key: "/evil"}})).reason,
                "invalid_args",
                "{key}"
            );
        }
        assert_eq!(
            call_err(&set, json!({"command": "x", "cwd": "../outside"})).reason,
            "path_escape"
        );
        assert_eq!(
            call_err(&set, json!({"command": "x", "cwd": "missing"})).reason,
            "not_found"
        );
    }

    #[test]
    fn runs_with_whitelisted_env_and_jail_cwd() {
        let (workspace, runner, set) = bash_only_set();
        let canonical = std::fs::canonicalize(workspace.path()).expect("canonical");
        std::fs::create_dir_all(workspace.path().join("sub")).expect("sub");
        let payload = call_ok(
            &set,
            json!({"command": "echo hi", "cwd": "sub",
                   "env": {"CUSTOM_FLAG": "on"}}),
        );
        assert_eq!(payload["exit_code"], json!(0));
        assert_eq!(payload["signal"], json!(null));
        assert_eq!(payload["sandbox"], json!("macos-sandbox-exec"));
        assert_eq!(payload["stdout"], json!("ok"));
        assert_eq!(payload["truncated"], json!(false));
        assert!(payload["duration_ms"].as_u64().is_some());
        let requests = runner.requests();
        assert_eq!(requests.len(), 1);
        let request = &requests[0];
        assert_eq!(request.command, "echo hi");
        assert_eq!(request.cwd, canonical.join("sub"));
        assert_eq!(
            request.env.get("HOME"),
            Some(&canonical.display().to_string())
        );
        assert_eq!(
            request.env.get("TMPDIR"),
            Some(&canonical.join(".tmp").display().to_string())
        );
        assert_eq!(request.env.get("PATH"), Some(&BASH_FIXED_PATH.to_string()));
        assert_eq!(request.env.get("CUSTOM_FLAG"), Some(&"on".to_string()));
        assert!(!request.env.contains_key("BASH_ENV"));
        assert!(canonical.join(".tmp").is_dir(), "TMPDIR created");
    }

    #[test]
    fn appends_tmpdir_to_existing_gitignore_once() {
        let (workspace, _runner, set) = bash_only_set();
        std::fs::write(workspace.path().join(".gitignore"), "target/\n").expect("gitignore");
        call_ok(&set, json!({"command": "echo hi"}));
        call_ok(&set, json!({"command": "echo again"}));
        let text = std::fs::read_to_string(workspace.path().join(".gitignore")).expect("read");
        assert_eq!(text.matches(".tmp/").count(), 1, "{text}");
    }

    #[test]
    fn nonzero_exit_is_a_normal_result() {
        let (_workspace, runner, set) = bash_only_set();
        runner.push_outcome(Ok(BashOutcome {
            exit_code: Some(2),
            stdout: b"out".to_vec(),
            stderr: b"bad".to_vec(),
            ..BashOutcome::default()
        }));
        let payload = call_ok(&set, json!({"command": "exit 2"}));
        assert_eq!(payload["exit_code"], json!(2));
        assert_eq!(payload["stdout"], json!("out"));
        assert_eq!(payload["stderr"], json!("bad"));
        assert_eq!(payload["truncated"], json!(false));
    }

    #[test]
    fn signal_termination_is_reported_as_null_exit_code() {
        let (_workspace, runner, set) = bash_only_set();
        runner.push_outcome(Ok(BashOutcome {
            exit_code: None,
            signal: Some(9),
            ..BashOutcome::default()
        }));
        let payload = call_ok(&set, json!({"command": "killme"}));
        assert_eq!(payload["exit_code"], json!(null));
        assert_eq!(payload["signal"], json!(9));
    }

    #[test]
    fn caps_stdout_and_stderr_at_16_kib_head_tail() {
        let (_workspace, runner, set) = bash_only_set();
        let stdout = "a".repeat(100_000);
        let stderr = "b".repeat(50_000);
        runner.push_outcome(Ok(BashOutcome {
            exit_code: Some(0),
            signal: None,
            stdout: stdout.into_bytes(),
            stderr: stderr.into_bytes(),
            drain_truncated: false,
            duration_ms: 1,
        }));
        let payload = call_ok(&set, json!({"command": "spam"}));
        let out = payload["stdout"].as_str().expect("stdout");
        let err = payload["stderr"].as_str().expect("stderr");
        assert!(out.len() <= BASH_STDOUT_MAX_BYTES, "{}", out.len());
        assert!(err.len() <= BASH_STDERR_MAX_BYTES, "{}", err.len());
        assert!(out.contains("…[truncated]…"));
        assert!(err.contains("…[truncated]…"));
        assert_eq!(payload["truncated"], json!(true));
        // bash 结果不受 32 KiB 总上限裁剪（两路 head+tail 允许略超）。
        assert!(payload["sandbox"].is_string());
    }

    #[test]
    fn timeout_and_sandbox_failures_are_tool_errors() {
        let (_workspace, runner, set) = bash_only_set();
        runner.push_outcome(Err(ToolFailure::new("timeout", "timed out")));
        let error = call_err(&set, json!({"command": "sleep 1"}));
        assert_eq!(error.reason, "timeout");

        runner.push_outcome(Err(ToolFailure::new("sandbox_failure", "nested")));
        let error = call_err(&set, json!({"command": "echo hi"}));
        assert_eq!(error.reason, "sandbox_failure");
    }

    #[test]
    fn build_env_forces_jail_values_and_inherits_whitelist() {
        let (_workspace_dir, workspace) = {
            let dir = TempDir::new().expect("tempdir");
            let canonical = std::fs::canonicalize(dir.path()).expect("canonical");
            (dir, canonical)
        };
        let mut explicit = BTreeMap::new();
        explicit.insert("LANG".to_string(), "C".to_string());
        explicit.insert("EXTRA".to_string(), "1".to_string());
        let tmpdir = workspace.join(".tmp");
        std::fs::create_dir_all(&tmpdir).expect("tmpdir");
        let env = build_bash_env(&workspace, &tmpdir, &explicit).expect("env");
        assert_eq!(env.get("HOME"), Some(&workspace.display().to_string()));
        assert_eq!(
            env.get("TMPDIR"),
            Some(&workspace.join(".tmp").display().to_string())
        );
        assert_eq!(env.get("PATH"), Some(&BASH_FIXED_PATH.to_string()));
        assert_eq!(env.get("SHELL"), Some(&"/bin/bash".to_string()));
        assert_eq!(env.get("LANG"), Some(&"C".to_string()));
        assert_eq!(env.get("EXTRA"), Some(&"1".to_string()));
    }
    #[cfg(unix)]
    #[test]
    fn tmpdir_symlink_escape_is_fail_closed() {
        let (workspace, _runner, set) = bash_only_set();
        let outside = TempDir::new().expect("outside");
        std::os::unix::fs::symlink(outside.path(), workspace.path().join(".tmp")).expect("symlink");
        let before: Vec<String> = std::fs::read_dir(outside.path())
            .expect("read")
            .map(|entry| {
                entry
                    .expect("entry")
                    .file_name()
                    .to_string_lossy()
                    .to_string()
            })
            .collect();
        let error = call_err(&set, json!({"command": "echo hi"}));
        assert_eq!(error.reason, "path_escape", "{error:?}");
        let after: Vec<String> = std::fs::read_dir(outside.path())
            .expect("read")
            .map(|entry| {
                entry
                    .expect("entry")
                    .file_name()
                    .to_string_lossy()
                    .to_string()
            })
            .collect();
        assert_eq!(before, after, "outside workspace must stay unchanged");
    }

    #[cfg(unix)]
    #[test]
    fn gitignore_symlink_escape_is_skipped_without_writing_outside() {
        let (workspace, _runner, set) = bash_only_set();
        let outside = TempDir::new().expect("outside");
        let victim = outside.path().join("gitignore.txt");
        std::fs::write(&victim, "target/\n").expect("victim");
        std::os::unix::fs::symlink(&victim, workspace.path().join(".gitignore")).expect("symlink");
        let payload = call_ok(&set, json!({"command": "echo hi"}));
        assert_eq!(payload["exit_code"], json!(0));
        assert_eq!(
            std::fs::read_to_string(&victim).expect("victim read"),
            "target/\n",
            "jail-external .gitignore must not be modified"
        );
        assert!(
            workspace.path().join(".tmp").is_dir(),
            "tmpdir created in jail"
        );
        // workspace 内真实 .gitignore 仍不受影响（不存在）。
        assert!(
            !workspace.path().join(".gitignore").is_file()
                || std::fs::symlink_metadata(workspace.path().join(".gitignore")).is_ok()
        );
    }

    #[cfg(unix)]
    #[test]
    fn tmpdir_symlink_inside_workspace_is_allowed_with_canonical_tmpdir() {
        let (workspace, runner, set) = bash_only_set();
        let real = workspace.path().join("real-tmp");
        std::fs::create_dir_all(&real).expect("real tmp");
        std::os::unix::fs::symlink(&real, workspace.path().join(".tmp")).expect("symlink");
        call_ok(&set, json!({"command": "echo hi"}));
        let requests = runner.requests();
        let canonical = std::fs::canonicalize(workspace.path()).expect("canonical");
        assert_eq!(
            requests[0].env.get("TMPDIR"),
            Some(&canonical.join("real-tmp").display().to_string())
        );
    }

    #[test]
    fn tmpdir_regular_file_is_rejected() {
        let (workspace, _runner, set) = bash_only_set();
        std::fs::write(workspace.path().join(".tmp"), "not a dir").expect("file");
        let error = call_err(&set, json!({"command": "echo hi"}));
        assert!(
            matches!(error.reason.as_str(), "invalid_args" | "path_escape"),
            "{error:?}"
        );
    }

    #[test]
    fn inherits_all_lc_vars_but_never_ld_or_dyld() {
        let workspace = PathBuf::from("/tmp/ws");
        let tmpdir = PathBuf::from("/tmp/ws/.tmp");
        let mut parent = BTreeMap::new();
        parent.insert("LANG".to_string(), "en_US.UTF-8".to_string());
        parent.insert("LC_ALL".to_string(), "C".to_string());
        parent.insert("LC_MESSAGES".to_string(), "zh_CN.UTF-8".to_string());
        parent.insert("LC_NUMERIC".to_string(), "de_DE.UTF-8".to_string());
        parent.insert("LD_PRELOAD".to_string(), "/evil.so".to_string());
        parent.insert(
            "DYLD_INSERT_LIBRARIES".to_string(),
            "/evil.dylib".to_string(),
        );
        parent.insert("BASH_ENV".to_string(), "/evil.sh".to_string());
        let env = build_bash_env_with_parent(&workspace, &tmpdir, &BTreeMap::new(), &parent)
            .expect("env");
        assert_eq!(env.get("LANG"), Some(&"en_US.UTF-8".to_string()));
        assert_eq!(env.get("LC_ALL"), Some(&"C".to_string()));
        assert_eq!(env.get("LC_MESSAGES"), Some(&"zh_CN.UTF-8".to_string()));
        assert_eq!(env.get("LC_NUMERIC"), Some(&"de_DE.UTF-8".to_string()));
        assert!(!env.contains_key("LD_PRELOAD"));
        assert!(!env.contains_key("DYLD_INSERT_LIBRARIES"));
        assert!(!env.contains_key("BASH_ENV"));
        assert_eq!(env.get("TMPDIR"), Some(&"/tmp/ws/.tmp".to_string()));
    }
}
