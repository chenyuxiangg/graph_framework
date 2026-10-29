//! `zgraph` CLI 入口（M0：仅版本/用法；M1 起接 validate/run/trace/replay）。

use std::process::ExitCode;

/// CLI 版本号（来自 Cargo 包版本）。
pub const CLI_VERSION: &str = env!("CARGO_PKG_VERSION");

/// 返回一行版本字符串，例如 `zgraph 0.1.0 (DSL 1.0)`。
pub fn version_line() -> String {
    format!(
        "zgraph {CLI_VERSION} (DSL {})",
        zgraph_dsl::DSL_SCHEMA_VERSION
    )
}

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        Some("--version") | Some("-V") => {
            println!("{}", version_line());
            ExitCode::SUCCESS
        }
        None => {
            println!("{}", version_line());
            println!("usage: zgraph <command>");
            println!("commands: (M0 骨架) --version");
            ExitCode::SUCCESS
        }
        Some(other) => {
            eprintln!("unknown command: {other}");
            ExitCode::from(2)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::version_line;

    #[test]
    fn version_line_contains_cli_and_dsl_versions() {
        let line = version_line();
        assert!(line.starts_with("zgraph "));
        assert!(line.contains("DSL 1.0"));
    }
}
