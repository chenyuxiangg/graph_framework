//! `zgraph` CLI 入口。
//!
//! M1 交付 `zgraph validate <graph.json> [--json] [--assets-dir <dir>]`；
//! 错误码/输出格式见 `doc/spec-v1.2/02-DSL与校验.md` §2.7.4。
//! 退出码：`0` 校验通过；`2` DSL 错误；`3` 用法/IO 错误。
//! 注：02 §2.7.4 用 `1` 表示"其他错误"，M1 合并进 `3`（任务口径 0/2/3）。

mod validate;

use std::process::ExitCode;

/// CLI 版本号（来自 Cargo 包版本）。
pub const CLI_VERSION: &str = env!("CARGO_PKG_VERSION");

/// 用法说明（stdout/stderr 共用）。
pub const USAGE: &str = "\
usage: zgraph <command>

commands:
  validate <graph.json> [--json] [--assets-dir <dir>]   validate a graph definition
  --version, -V                                          print version
  --help, -h                                             print this help";

/// 返回一行版本字符串，例如 `zgraph 0.1.0 (DSL 1.0)`。
pub fn version_line() -> String {
    format!(
        "zgraph {CLI_VERSION} (DSL {})",
        zgraph_dsl::DSL_SCHEMA_VERSION
    )
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match dispatch(&args) {
        Ok(code) => code,
        Err(usage_error) => {
            eprintln!("{usage_error}");
            eprintln!("{USAGE}");
            ExitCode::from(3)
        }
    }
}

fn dispatch(args: &[String]) -> Result<ExitCode, String> {
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
            let args = validate::parse(&args[1..])?;
            Ok(validate::run(&args))
        }
        other => Err(format!("unknown command: {other}")),
    }
}

#[cfg(test)]
mod tests {
    use super::{dispatch, version_line};
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
        assert!(error.contains("unknown command"));
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
}
