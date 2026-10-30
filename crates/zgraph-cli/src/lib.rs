//! `zgraph` CLI 库：子命令解析、输出与退出码映射。
//!
//! 二进制入口 [`crate::run`] 只做 `std::env::args` 收集与退出码返回；各子命令的
//! 实现位于 [`validate`]（M1）、[`trace_cmd`]（M2，L1 只读回放）与 [`run_cmd`]
//! （M5，`zgraph run` 真实链路）。
//!
//! 退出码权威表：`doc/spec-v1.2/06-存储与可观测.md` §6.10.5。
//! 注：M1 `validate` 沿用任务口径 `0/2/3`（02 §2.7.4 的「其他错误 1」并入 3），
//! M2 `trace` 使用 `0/1/2/4`，M5 `run` 使用 `0/1/2/3/4/130`；所有子命令的 panic
//! 由顶层 `catch_unwind` 捕获并统一返回 **70**（不依赖 Rust 默认 101）。

#![forbid(unsafe_code)]

pub mod bridge;
pub mod run_cmd;
pub mod trace_cmd;
pub mod validate;

use std::any::Any;
use std::process::ExitCode;

/// CLI 版本号（来自 Cargo 包版本）。
pub const CLI_VERSION: &str = env!("CARGO_PKG_VERSION");

/// 退出码 0：成功。
pub const EXIT_SUCCESS: u8 = 0;
/// 退出码 1：run 未成功 / trace 中断。
pub const EXIT_OTHER: u8 = 1;
/// 退出码 2：图/资产错误或用法错误（06 §6.10.5；`run`/`trace`）。
pub const EXIT_USAGE: u8 = 2;
/// 退出码 2：用法错误（trace；06 §6.10.5）。与 [`EXIT_USAGE`] 同值，保留旧名。
pub const EXIT_USAGE_TRACE: u8 = 2;
/// 退出码 3：入口输入非法（`run`；不产生 run）。
pub const EXIT_ENTRY_INPUT: u8 = 3;
/// 退出码 3：M1 `validate` 用法/IO 错误（保持 M1 行为）。
pub const EXIT_USAGE_VALIDATE: u8 = 3;
/// 退出码 4：存储/环境错误（06 §6.10.5）。
pub const EXIT_STORAGE: u8 = 4;
/// 退出码 130：SIGINT 取消（128+2；run 标 `Cancelled`）。
pub const EXIT_INTERRUPTED: u8 = 130;
/// 退出码 70：内部错误（panic 被顶层 `catch_unwind` 捕获；06 §6.10.5）。
pub const EXIT_INTERNAL: u8 = 70;

/// 用法说明（stdout/stderr 共用）。
pub const USAGE: &str = "\
usage: zgraph <command>

commands:
  run <graph.json> --inputs <json|@file> [options]       run a graph (M5)
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

    /// `run` 用法错误（退出码 2，06 §6.10.5）。
    pub fn run_usage(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            exit_code: EXIT_USAGE,
        }
    }
}

/// 进程入口：收集参数、分发子命令，错误时打印信息与用法并返回退出码。
///
/// panic 由 `run_with` 的 `catch_unwind` 捕获并映射为退出码 70（06 §6.10.5）。
pub fn run() -> ExitCode {
    run_with(|| {
        let args: Vec<String> = std::env::args().skip(1).collect();
        dispatch(&args)
    })
}

/// 顶层 panic 守卫：`catch_unwind` 捕获内部 panic → 退出码 70，panic 信息走 stderr。
///
/// 提取为独立函数是为了可重复单测覆盖（二进制层无法在不伪造失败路径的前提下安全
/// 触发 panic；单测直接覆盖包装逻辑本身）。panic 的原始信息同时由默认 panic hook
/// 打印到 stderr（不额外安装自定义 hook）。
fn run_with<F>(dispatch: F) -> ExitCode
where
    F: FnOnce() -> Result<ExitCode, CliError> + std::panic::UnwindSafe,
{
    match std::panic::catch_unwind(dispatch) {
        Ok(Ok(code)) => code,
        Ok(Err(error)) => {
            eprintln!("{}", error.message);
            eprintln!("{USAGE}");
            ExitCode::from(error.exit_code)
        }
        Err(payload) => {
            eprintln!("error[INTERNAL]: {}", panic_message(payload.as_ref()));
            ExitCode::from(EXIT_INTERNAL)
        }
    }
}

/// 从 panic payload 提取可读信息（`panic!("...")` 的 `&str` / `String`）。
fn panic_message(payload: &(dyn Any + Send)) -> String {
    if let Some(text) = payload.downcast_ref::<&str>() {
        (*text).to_string()
    } else if let Some(text) = payload.downcast_ref::<String>() {
        text.clone()
    } else {
        "panic with non-string payload".to_string()
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
        "run" => {
            let parsed = run_cmd::parse(&args[1..]).map_err(CliError::run_usage)?;
            match parsed {
                Some(options) => Ok(run_cmd::run(&options)),
                None => {
                    println!("{}", run_cmd::RUN_USAGE);
                    Ok(ExitCode::SUCCESS)
                }
            }
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
    use super::{
        dispatch, panic_message, run_with, version_line, CliError, EXIT_INTERNAL, EXIT_USAGE,
        EXIT_USAGE_TRACE, EXIT_USAGE_VALIDATE,
    };
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
    fn run_usage_errors_exit_2() {
        let error = dispatch(&["run".to_string()]).expect_err("missing graph/inputs");
        assert_eq!(error.exit_code, EXIT_USAGE);
        let error = dispatch(&[
            "run".to_string(),
            "g.json".to_string(),
            "--inputs".to_string(),
            "{}".to_string(),
            "--real".to_string(),
            "--fake-llm".to_string(),
        ])
        .expect_err("conflict");
        assert!(error.message.contains("--real conflicts"));
        assert_eq!(error.exit_code, EXIT_USAGE);
    }

    #[test]
    fn cli_error_keeps_both_codes() {
        assert_eq!(CliError::validate_usage("x").exit_code, EXIT_USAGE_VALIDATE);
        assert_eq!(CliError::trace_usage("x").exit_code, EXIT_USAGE_TRACE);
        assert_eq!(CliError::run_usage("x").exit_code, EXIT_USAGE);
    }

    // 二进制层无法安全触发 panic（不能为测试加生产后门），因此覆盖 catch 包装逻辑：
    // 捕获后必须返回 70 而不是 Rust 默认的 101（06 §6.10.5）。
    #[test]
    fn panic_is_caught_and_mapped_to_exit_70() {
        let code = run_with(|| panic!("boom from run_with unit test"));
        assert_eq!(code, ExitCode::from(EXIT_INTERNAL));
        assert_ne!(
            code,
            ExitCode::from(101),
            "must not rely on Rust default 101"
        );
    }

    #[test]
    fn run_with_passes_through_success_and_usage_errors() {
        assert_eq!(run_with(|| Ok(ExitCode::SUCCESS)), ExitCode::SUCCESS);
        let code = run_with(|| Err(CliError::run_usage("bad arguments")));
        assert_eq!(code, ExitCode::from(EXIT_USAGE));
    }

    #[test]
    fn panic_message_reads_str_and_string_payloads() {
        let text: Box<dyn std::any::Any + Send> = Box::new("borrowed panic");
        assert_eq!(panic_message(text.as_ref()), "borrowed panic");
        let owned: Box<dyn std::any::Any + Send> = Box::new("owned panic".to_string());
        assert_eq!(panic_message(owned.as_ref()), "owned panic");
        let other: Box<dyn std::any::Any + Send> = Box::new(42_u32);
        assert!(panic_message(other.as_ref()).contains("non-string"));
    }
}
