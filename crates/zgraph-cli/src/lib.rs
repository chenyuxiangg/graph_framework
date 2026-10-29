//! `zgraph` CLI 库：子命令解析、输出与退出码映射。
//!
//! 二进制入口 [`crate::run`] 只做 `std::env::args` 收集与退出码返回；各子命令的
//! 实现位于 [`validate`]（M1）与 [`trace_cmd`]（M2，L1 只读回放）。
//!
//! 退出码权威表：`doc/spec-v1.2/06-存储与可观测.md` §6.10.5。
//! 注：M1 `validate` 沿用任务口径 `0/2/3`（02 §2.7.4 的「其他错误 1」并入 3），
//! M2 `trace` 使用 `0/1/2/4`。

#![forbid(unsafe_code)]

pub mod trace_cmd;
pub mod validate;

use std::process::ExitCode;

/// CLI 版本号（来自 Cargo 包版本）。
pub const CLI_VERSION: &str = env!("CARGO_PKG_VERSION");

/// 退出码 0：成功。
pub const EXIT_SUCCESS: u8 = 0;
/// 退出码 1：run 未成功 / trace 中断。
pub const EXIT_OTHER: u8 = 1;
/// 退出码 2：用法错误（trace；06 §6.10.5）。
pub const EXIT_USAGE_TRACE: u8 = 2;
/// 退出码 3：M1 `validate` 用法/IO 错误（保持 M1 行为）。
pub const EXIT_USAGE_VALIDATE: u8 = 3;
/// 退出码 4：存储/环境错误（trace；06 §6.10.5）。
pub const EXIT_STORAGE: u8 = 4;

/// 用法说明（stdout/stderr 共用）。
pub const USAGE: &str = "\
usage: zgraph <command>

commands:
  validate <graph.json> [--json] [--assets-dir <dir>]   validate a graph definition
  trace <run_id> [options]                               replay a run trace (L1)
  --version, -V                                          print version
  --help, -h                                             print this help";

/// 返回一行版本字符串，例如 `zgraph 0.1.0 (DSL 1.0)`。
pub fn version_line() -> String {
    format!(
        "zgraph {CLI_VERSION} (DSL {})",
        zgraph_dsl::DSL_SCHEMA_VERSION
    )
}

/// 顶层分发错误：错误信息 + 退出码（`main` 统一打印到 stderr）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CliError {
    /// 人类可读错误信息。
    pub message: String,
    /// 进程退出码。
    pub exit_code: u8,
}

impl CliError {
    /// M1 兼容的用法/IO 错误（退出码 3）。
    pub fn validate_usage(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            exit_code: EXIT_USAGE_VALIDATE,
        }
    }

    /// `trace` 用法错误（退出码 2，06 §6.10.5）。
    pub fn trace_usage(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            exit_code: EXIT_USAGE_TRACE,
        }
    }
}

/// 进程入口：收集参数、分发子命令，错误时打印信息与用法并返回退出码。
pub fn run() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match dispatch(&args) {
        Ok(code) => code,
        Err(error) => {
            eprintln!("{}", error.message);
            eprintln!("{USAGE}");
            ExitCode::from(error.exit_code)
        }
    }
}

/// 按首个参数分发子命令。
///
/// 返回 `Err` 表示参数解析失败（由 [`run`] 打印信息与用法）；子命令自身的
/// 业务失败由子命令返回退出码（不经过此 `Err`）。
pub fn dispatch(args: &[String]) -> Result<ExitCode, CliError> {
    let Some(command) = args.first() else {
        println!("{USAGE}");
        return Ok(ExitCode::SUCCESS);
    };
    match command.as_str() {
        "--version" | "-V" => {
            println!("{}", version_line());
            Ok(ExitCode::SUCCESS)
        }
        "--help" | "-h" | "help" => {
            println!("{USAGE}");
            Ok(ExitCode::SUCCESS)
        }
        "validate" => {
            let parsed = validate::parse(&args[1..]).map_err(CliError::validate_usage)?;
            Ok(validate::run(&parsed))
        }
        "trace" => {
            let parsed = trace_cmd::parse(&args[1..]).map_err(CliError::trace_usage)?;
            match parsed {
                Some(options) => Ok(trace_cmd::run(&options)),
                None => {
                    println!("{}", trace_cmd::TRACE_USAGE);
                    Ok(ExitCode::SUCCESS)
                }
            }
        }
        other => Err(CliError::validate_usage(format!(
            "unknown command: {other}"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::{dispatch, version_line, CliError, EXIT_USAGE_TRACE, EXIT_USAGE_VALIDATE};
    use std::process::ExitCode;

    #[test]
    fn version_line_contains_cli_and_dsl_versions() {
        let line = version_line();
        assert!(line.starts_with("zgraph "));
        assert!(line.contains("DSL 1.0"));
    }

    #[test]
    fn unknown_command_is_usage_error() {
        let error = dispatch(&["nope".to_string()]).expect_err("usage error");
        assert!(error.message.contains("unknown command"));
        assert_eq!(error.exit_code, EXIT_USAGE_VALIDATE);
    }

    #[test]
    fn version_and_help_succeed() {
        assert_eq!(
            dispatch(&["--version".to_string()]).expect("version"),
            ExitCode::SUCCESS
        );
        assert_eq!(
            dispatch(&["--help".to_string()]).expect("help"),
            ExitCode::SUCCESS
        );
    }

    #[test]
    fn trace_usage_errors_exit_2() {
        let error = dispatch(&["trace".to_string()]).expect_err("missing run id");
        assert_eq!(error.exit_code, EXIT_USAGE_TRACE);

        let error = dispatch(&[
            "trace".to_string(),
            "run-1".to_string(),
            "--kinds".to_string(),
            "Nope".to_string(),
        ])
        .expect_err("invalid kind");
        assert!(error.message.contains("invalid --kinds"));
        assert_eq!(error.exit_code, EXIT_USAGE_TRACE);
    }

    #[test]
    fn cli_error_keeps_both_codes() {
        assert_eq!(CliError::validate_usage("x").exit_code, EXIT_USAGE_VALIDATE);
        assert_eq!(CliError::trace_usage("x").exit_code, EXIT_USAGE_TRACE);
    }
}
