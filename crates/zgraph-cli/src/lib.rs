//! `zgraph` CLI 库：子命令解析、输出与退出码映射。
//!
//! 二进制入口 [`crate::run`] 只做 `std::env::args` 收集与退出码返回；各子命令的
//! 实现位于 [`validate`]（M1）、[`trace_cmd`]（M2，L1 只读回放）、[`run_cmd`]
//! （M5，`zgraph run` 真实链路）与 [`replay_cmd`]（M5 wave2，L1/L2 重放）。
//!
//! 退出码权威表：`doc/spec-v1.2/06-存储与可观测.md` §6.10.5。
//! 注（M6 起，ISSUE-059 收口）：`validate` 统一为目标口径 `0/1/2/70`——**用法 → 2、
//! IO/内部 → 1**（02 §2.7.4 的 M1 临时口径 3 已废弃）；`trace` 使用 `0/1/2/4`；
//! `run` 使用 `0/1/2/3/4/130`；`replay` 使用 `0/1/2/3/4`（L2 记录类错误
//! `REPLAY_*` 退出 3，不写 `runs.error_code`）；所有子命令的 panic 由顶层
//! `catch_unwind` 捕获并统一返回 **70**（不依赖 Rust 默认 101）。
//!
//! # v0.1 CLI 决策（REV-028 #6 / DR-002 §9 #11）
//!
//! - **手写参数解析，不引入 `clap`**：06 §6.10.1 的目标实现是 `clap`，但 v0.1 保持
//!   M1 起的手写解析（四个子命令、参数面已稳定），避免在收尾期引入新依赖（同一
//!   wave 的 ISSUE-068/078 正在做依赖治理）；`clap` 迁移留待后续版本。spec 06
//!   §6.10.1 的「M5 实现现状」注记需由 docs 分支同步本决策。
//! - **`-v/--verbose`**：按 06 §6.10.1「映射 `RUST_LOG`」实现为全局参数（见
//!   [`extract_verbose`]/[`apply_verbosity`]）。v0.1 未引入日志框架，CLI 自身不打印
//!   debug 日志，因此该参数只做 RUST_LOG 映射（-v → `debug`、-vv → `trace`；
//!   显式 `RUST_LOG` 优先、不覆盖），供承载日志的模块与子进程继承。
//! - **颜色 / `NO_COLOR`**：06 §6.10.1 要求「仅 TTY 启用 ANSI、`NO_COLOR` 优先」。
//!   v0.1 的**全部**输出（人类可读、`--json`、NDJSON）都是纯文本、不含 ANSI 转义，
//!   故 `NO_COLOR` 语义天然满足；不新增 `termcolor`/`nu-ansi-term` 依赖、也不做 TTY
//!   探测。该策略由集成测试 `tests/validate.rs` 的输出扫描守卫（未来引入颜色必须
//!   同时实现 TTY + `NO_COLOR` 判定）。
//! - **`--version`**：输出 `zgraph <pkg> (DSL <schema>)`，例如
//!   `zgraph 0.1.0 (DSL 1.0)`；spec 06 §6.10.1 的字面格式待 docs 回写（DR-002 §9 #11）。

#![forbid(unsafe_code)]

pub mod bridge;
pub mod recovery;
pub mod replay_cmd;
pub mod run_cmd;
pub mod trace_cmd;
pub mod validate;

use std::any::Any;
use std::process::ExitCode;

/// CLI 版本号（来自 Cargo 包版本）。
pub const CLI_VERSION: &str = env!("CARGO_PKG_VERSION");

/// 退出码 0：成功。
pub const EXIT_SUCCESS: u8 = 0;
/// 退出码 1：run 未成功 / trace 中断 / `validate` 的 IO/内部错误（06 §6.10.5）。
pub const EXIT_OTHER: u8 = 1;
/// 退出码 2：图/资产错误或用法错误（06 §6.10.5；含 `validate` 的用法错误）。
pub const EXIT_USAGE: u8 = 2;
/// 退出码 3：入口输入非法（`run`；不产生 run）。
pub const EXIT_ENTRY_INPUT: u8 = 3;
/// 退出码 4：存储/环境错误（06 §6.10.5）。
pub const EXIT_STORAGE: u8 = 4;
/// 退出码 130：SIGINT 取消（128+2；run 标 `Cancelled`）。
pub const EXIT_INTERRUPTED: u8 = 130;
/// 退出码 70：内部错误（panic 被顶层 `catch_unwind` 捕获；06 §6.10.5）。
pub const EXIT_INTERNAL: u8 = 70;

/// `-v/--verbose` 的 `RUST_LOG` 映射：单次 → `debug`（06 §6.10.1）。
pub const VERBOSE_RUST_LOG_DEBUG: &str = "debug";
/// `-v/--verbose` 的 `RUST_LOG` 映射：重复（`-vv`）→ `trace`（06 §6.10.1）。
pub const VERBOSE_RUST_LOG_TRACE: &str = "trace";

/// 用法说明（stdout/stderr 共用）。
pub const USAGE: &str = "\
usage: zgraph <command>

commands:
  run <graph.json> --inputs <json|@file> [options]       run a graph (M5)
  replay --run <run_id> [--mode=events|outputs]          replay a run (L1 events / L2 outputs)
  validate <graph.json> [--json] [--assets-dir <dir>]   validate a graph definition
  trace <run_id> [options]                               replay a run trace (L1)
  -v, --verbose                                          map verbosity to RUST_LOG (debug/trace)
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
    /// 用法错误（退出码 2，06 §6.10.5）：未知子命令、缺参数、非法选项值。
    ///
    /// M6（ISSUE-059）起 `validate` 的用法错误也走本构造器；M1 临时口径的 3 已废弃。
    pub fn usage(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            exit_code: EXIT_USAGE,
        }
    }
}

/// 进程入口：收集参数、分发子命令，错误时打印信息与用法并返回退出码。
///
/// 全局 `-v/--verbose` 在分发前抽取并映射到 `RUST_LOG`（06 §6.10.1）。
/// panic 由 `run_with` 的 `catch_unwind` 捕获并映射为退出码 70（06 §6.10.5）。
pub fn run() -> ExitCode {
    run_with(|| {
        let args: Vec<String> = std::env::args().skip(1).collect();
        let (args, verbosity) = extract_verbose(args);
        apply_verbosity(verbosity);
        dispatch(&args)
    })
}

/// 抽取全局 `-v/--verbose`，返回「剩余参数 + 出现次数」。
///
/// 该参数在 06 §6.10.1 属**全局参数**，可出现在子命令前后的任意位置；抽取后
/// 由 [`dispatch`] 按原顺序处理其余参数。重复出现按次数累计（`-vv` 等价于两次
/// `-v`，见 [`rust_log_for_verbosity`]）。
pub fn extract_verbose(args: Vec<String>) -> (Vec<String>, u8) {
    let mut rest = Vec::with_capacity(args.len());
    let mut verbosity: u8 = 0;
    for arg in args {
        if arg == "-v" || arg == "--verbose" {
            verbosity = verbosity.saturating_add(1);
        } else {
            rest.push(arg);
        }
    }
    (rest, verbosity)
}

/// `-v` 次数 → `RUST_LOG` 级别；0 次不设置（保持用户环境/默认）。
///
/// 06 §6.10.1 只规定「`-v/--verbose` 映射 `RUST_LOG`」，未规定级别与叠加语义；
/// v0.1 取最小约定：1 次 → `debug`，≥2 次 → `trace`。
pub fn rust_log_for_verbosity(verbosity: u8) -> Option<&'static str> {
    match verbosity {
        0 => None,
        1 => Some(VERBOSE_RUST_LOG_DEBUG),
        _ => Some(VERBOSE_RUST_LOG_TRACE),
    }
}

/// 把 `-v/--verbose` 映射写入 `RUST_LOG`（06 §6.10.1）。
///
/// 显式设置的非空 `RUST_LOG` 优先、不被覆盖（用户配置优先于 CLI 默认映射）。
/// v0.1 未引入日志框架（`log`/`env_logger`），CLI 自身不产生 debug 日志，本函数
/// 只完成 spec 规定的环境映射，供承载日志的模块与 worker 子进程继承。
fn apply_verbosity(verbosity: u8) {
    let Some(level) = rust_log_for_verbosity(verbosity) else {
        return;
    };
    if std::env::var_os("RUST_LOG").is_none_or(|value| value.is_empty()) {
        std::env::set_var("RUST_LOG", level);
    }
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
/// 业务失败由子命令返回退出码（不经过此 `Err`）。全部用法错误统一为退出码
/// `2`（06 §6.10.5）。全局 `-v/--verbose` 由 [`run`] 在调用本函数前通过
/// [`extract_verbose`] 抽取；直接调用本函数时应先做同样抽取。
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
            let parsed = validate::parse(&args[1..]).map_err(CliError::usage)?;
            Ok(validate::run(&parsed))
        }
        "run" => {
            let parsed = run_cmd::parse(&args[1..]).map_err(CliError::usage)?;
            match parsed {
                Some(options) => Ok(run_cmd::run(&options)),
                None => {
                    println!("{}", run_cmd::RUN_USAGE);
                    Ok(ExitCode::SUCCESS)
                }
            }
        }
        "replay" => {
            let parsed = replay_cmd::parse(&args[1..]).map_err(CliError::usage)?;
            match parsed {
                Some(options) => Ok(replay_cmd::run(&options)),
                None => {
                    println!("{}", replay_cmd::REPLAY_USAGE);
                    Ok(ExitCode::SUCCESS)
                }
            }
        }
        "trace" => {
            let parsed = trace_cmd::parse(&args[1..]).map_err(CliError::usage)?;
            match parsed {
                Some(options) => Ok(trace_cmd::run(&options)),
                None => {
                    println!("{}", trace_cmd::TRACE_USAGE);
                    Ok(ExitCode::SUCCESS)
                }
            }
        }
        other => Err(CliError::usage(format!("unknown command: {other}"))),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        dispatch, extract_verbose, panic_message, run_with, rust_log_for_verbosity, version_line,
        CliError, EXIT_INTERNAL, EXIT_USAGE, VERBOSE_RUST_LOG_DEBUG, VERBOSE_RUST_LOG_TRACE,
    };
    use std::process::ExitCode;

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_string()).collect()
    }

    #[test]
    fn version_line_contains_cli_and_dsl_versions() {
        let line = version_line();
        assert!(line.starts_with("zgraph "));
        assert!(line.contains("DSL 1.0"));
    }

    #[test]
    fn unknown_command_is_usage_error() {
        let error = dispatch(&args(&["nope"])).expect_err("usage error");
        assert!(error.message.contains("unknown command"));
        assert_eq!(error.exit_code, EXIT_USAGE);
    }

    #[test]
    fn validate_usage_errors_exit_2() {
        // ISSUE-059：M1 临时口径 3 已废弃，validate 用法错误统一为 2（06 §6.10.5）。
        let error = dispatch(&args(&["validate"])).expect_err("missing graph");
        assert_eq!(error.exit_code, EXIT_USAGE);
        let error = dispatch(&args(&["validate", "--nope"])).expect_err("unknown option");
        assert!(error.message.contains("unknown option"));
        assert_eq!(error.exit_code, EXIT_USAGE);
    }

    #[test]
    fn version_and_help_succeed() {
        assert_eq!(
            dispatch(&args(&["--version"])).expect("version"),
            ExitCode::SUCCESS
        );
        assert_eq!(
            dispatch(&args(&["--help"])).expect("help"),
            ExitCode::SUCCESS
        );
    }

    #[test]
    fn trace_usage_errors_exit_2() {
        let error = dispatch(&args(&["trace"])).expect_err("missing run id");
        assert_eq!(error.exit_code, EXIT_USAGE);

        let error =
            dispatch(&args(&["trace", "run-1", "--kinds", "Nope"])).expect_err("invalid kind");
        assert!(error.message.contains("invalid --kinds"));
        assert_eq!(error.exit_code, EXIT_USAGE);
    }

    #[test]
    fn run_usage_errors_exit_2() {
        let error = dispatch(&args(&["run"])).expect_err("missing graph/inputs");
        assert_eq!(error.exit_code, EXIT_USAGE);
        let error = dispatch(&args(&[
            "run",
            "g.json",
            "--inputs",
            "{}",
            "--real",
            "--fake-llm",
        ]))
        .expect_err("conflict");
        assert!(error.message.contains("--real conflicts"));
        assert_eq!(error.exit_code, EXIT_USAGE);
    }

    #[test]
    fn replay_usage_errors_exit_2() {
        let error = dispatch(&args(&["replay"])).expect_err("missing --run");
        assert_eq!(error.exit_code, EXIT_USAGE);
        let error =
            dispatch(&args(&["replay", "--run", "r1", "--mode", "all"])).expect_err("invalid mode");
        assert!(error.message.contains("invalid --mode"));
        assert_eq!(error.exit_code, EXIT_USAGE);
    }

    #[test]
    fn cli_error_uses_single_usage_code() {
        assert_eq!(CliError::usage("x").exit_code, EXIT_USAGE);
        assert_eq!(EXIT_USAGE, 2, "06 §6.10.5: usage/DSL/asset errors exit 2");
    }

    #[test]
    fn extract_verbose_accepts_flag_anywhere_and_counts_repeats() {
        let (rest, verbosity) = extract_verbose(args(&[]));
        assert!(rest.is_empty());
        assert_eq!(verbosity, 0);

        let (rest, verbosity) = extract_verbose(args(&["-v"]));
        assert!(rest.is_empty());
        assert_eq!(verbosity, 1);

        // 可出现在子命令前/后与参数值之间；抽取后其余参数顺序不变。
        let (rest, verbosity) = extract_verbose(args(&[
            "-v",
            "validate",
            "graph.json",
            "--verbose",
            "--json",
            "-v",
        ]));
        assert_eq!(rest, args(&["validate", "graph.json", "--json"]));
        assert_eq!(verbosity, 3);

        // `--version`/`-V` 不是 verbose，不被误抽取。
        let (rest, verbosity) = extract_verbose(args(&["--version"]));
        assert_eq!(rest, args(&["--version"]));
        assert_eq!(verbosity, 0);
    }

    #[test]
    fn verbosity_maps_to_rust_log_levels() {
        assert_eq!(rust_log_for_verbosity(0), None);
        assert_eq!(rust_log_for_verbosity(1), Some(VERBOSE_RUST_LOG_DEBUG));
        assert_eq!(rust_log_for_verbosity(2), Some(VERBOSE_RUST_LOG_TRACE));
        assert_eq!(rust_log_for_verbosity(5), Some(VERBOSE_RUST_LOG_TRACE));
    }

    #[test]
    fn apply_verbosity_sets_rust_log_only_when_absent() {
        // 进程级 env 写：本用例独占 RUST_LOG，用 guard 恢复，避免影响其他测试。
        struct RustLogGuard(Option<std::ffi::OsString>);
        impl Drop for RustLogGuard {
            fn drop(&mut self) {
                match self.0.take() {
                    Some(value) => std::env::set_var("RUST_LOG", value),
                    None => std::env::remove_var("RUST_LOG"),
                }
            }
        }
        let _guard = RustLogGuard(std::env::var_os("RUST_LOG"));

        std::env::remove_var("RUST_LOG");
        super::apply_verbosity(0);
        assert_eq!(std::env::var_os("RUST_LOG"), None, "0 次不设置");

        super::apply_verbosity(1);
        assert_eq!(
            std::env::var("RUST_LOG").ok().as_deref(),
            Some(VERBOSE_RUST_LOG_DEBUG)
        );

        std::env::set_var("RUST_LOG", "warn");
        super::apply_verbosity(2);
        assert_eq!(
            std::env::var("RUST_LOG").ok().as_deref(),
            Some("warn"),
            "显式 RUST_LOG 不被 -v 覆盖"
        );

        std::env::set_var("RUST_LOG", "");
        super::apply_verbosity(1);
        assert_eq!(
            std::env::var("RUST_LOG").ok().as_deref(),
            Some(VERBOSE_RUST_LOG_DEBUG),
            "空 RUST_LOG 视为未设置"
        );
    }

    #[test]
    fn dispatch_after_verbose_extraction_accepts_global_flag() {
        // 模拟 `run()` 的分发路径：先抽取全局 -v，再 dispatch。
        let (rest, verbosity) = extract_verbose(args(&["validate", "-v"]));
        assert_eq!(verbosity, 1);
        let error = dispatch(&rest).expect_err("missing graph");
        assert!(error.message.contains("validate requires"));
        assert_eq!(error.exit_code, EXIT_USAGE);
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
        let code = run_with(|| Err(CliError::usage("bad arguments")));
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
