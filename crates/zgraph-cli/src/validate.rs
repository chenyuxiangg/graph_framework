//! `zgraph validate` 子命令：参数解析、人类可读输出与 `--json` 输出。
//!
//! 人类可读格式对齐 02 §2.7.4；错误对象来自 `zgraph_dsl::ValidationReport`。

use std::path::PathBuf;
use std::process::ExitCode;

use serde_json::Value;
use zgraph_dsl::{load_graph_with_options, LoadOptions, Location, ValidationReport};

use crate::{EXIT_OTHER, EXIT_USAGE};

/// `validate` 的参数。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidateArgs {
    /// 图定义文件路径。
    pub graph: PathBuf,
    /// 是否输出 JSON 容器（02 §2.7.2）。
    pub json: bool,
    /// Agent 资产目录（缺省读 `ZGRAPH_ASSETS`；两者都没有则跳过 R14c）。
    pub assets_dir: Option<PathBuf>,
}

/// 解析 `validate` 之后的参数。
pub fn parse(args: &[String]) -> Result<ValidateArgs, String> {
    let mut graph = None;
    let mut json = false;
    let mut assets_dir = None;
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--json" => json = true,
            "--assets-dir" => {
                let value = iter
                    .next()
                    .ok_or_else(|| "--assets-dir requires a directory".to_string())?;
                assets_dir = Some(PathBuf::from(value));
            }
            flag if flag.starts_with('-') => return Err(format!("unknown option: {flag}")),
            path => {
                if graph.is_some() {
                    return Err(format!("unexpected argument: {path}"));
                }
                graph = Some(PathBuf::from(path));
            }
        }
    }
    let graph = graph.ok_or_else(|| "validate requires <graph.json>".to_string())?;
    Ok(ValidateArgs {
        graph,
        json,
        assets_dir,
    })
}

/// 执行 `validate` 并返回进程退出码（06 §6.10.5 权威表）。
///
/// `validate` 退出码（M6 起统一，ISSUE-059 废弃 M1 临时口径 3）：
/// `0` 合法；`2` DSL/资产错误；`1` IO 错误（文件不可读）；panic 由顶层
/// `catch_unwind` 统一映射为 `70`。用法错误（选项/参数）在 [`parse`] 阶段返回，
/// 由 [`crate::dispatch`] 映射为 `2`。
pub fn run(args: &ValidateArgs) -> ExitCode {
    let display = args.graph.display().to_string();
    let bytes = match std::fs::read(&args.graph) {
        Ok(bytes) => bytes,
        Err(error) => {
            // 06 §6.10.5 / 02 §2.7.4：文件不可读属「其他（IO/内部）」→ 退出码 1。
            eprintln!("error: cannot read '{display}': {error}");
            return ExitCode::from(EXIT_OTHER);
        }
    };
    let options = build_options(args);
    let result = load_graph_with_options(&bytes, &options);
    let report = ValidationReport::from_result(&result);
    if report.ok {
        if args.json {
            print_json(&report);
        } else if let Ok(loaded) = &result {
            let definition = &loaded.definition;
            println!(
                "✓ graph valid: {} nodes, {} edges, {} subgraphs (graph_hash={})",
                definition.node_count(),
                definition.edge_count(),
                definition.subgraph_count(),
                loaded.graph_hash
            );
        }
        ExitCode::SUCCESS
    } else {
        if args.json {
            print_json(&report);
        } else {
            print_errors(&display, &report);
        }
        ExitCode::from(EXIT_USAGE)
    }
}

fn build_options(args: &ValidateArgs) -> LoadOptions {
    let assets_dir = args
        .assets_dir
        .clone()
        .or_else(|| std::env::var_os("ZGRAPH_ASSETS").map(PathBuf::from));
    LoadOptions { assets_dir }
}

fn print_json(report: &ValidationReport) {
    match serde_json::to_string(report) {
        Ok(text) => println!("{text}"),
        Err(error) => eprintln!("error: cannot serialize validation report: {error}"),
    }
}

/// 人类可读错误输出（`run` 的图加载失败复用同一格式）。
pub(crate) fn print_errors(file: &str, report: &ValidationReport) {
    for error in &report.errors {
        let locator = locator(&error.location);
        if locator.is_empty() {
            eprintln!("error[{}] {file}", error.code);
        } else {
            eprintln!("error[{}] {}: {}", error.code, file, locator);
        }
        eprintln!("  {}", error.message);
        if let Some(expected) = &error.expected {
            eprintln!("  expected: {}", format_value(expected));
        }
        if let Some(actual) = &error.actual {
            eprintln!("  actual:   {}", format_value(actual));
        }
    }
    if report.truncated {
        eprintln!(
            "{} error(s) (showing first {})",
            report.error_count,
            report.errors.len()
        );
    } else {
        eprintln!("{} error(s)", report.error_count);
    }
}

/// 定位串（02 §2.7.4）：文档级/节点级/边级/端口级。
///
/// `location` 仅含 `node_id`/`edge_index`/`port`（02 §2.7.3）；文档级返回空串。
fn locator(location: &Location) -> String {
    if let Some(edge_index) = location.edge_index {
        return match &location.port {
            Some(_) => format!("edges[{edge_index}].to_input_port"),
            None => format!("edges[{edge_index}]"),
        };
    }
    if let Some(node_id) = &location.node_id {
        return match &location.port {
            Some(port) => format!("node '{node_id}'.inputs['{port}']"),
            None => format!("node '{node_id}'"),
        };
    }
    String::new()
}

fn format_value(value: &Value) -> String {
    match value {
        Value::Array(items) if !items.is_empty() && items.iter().all(Value::is_string) => {
            let values: Vec<&str> = items.iter().filter_map(Value::as_str).collect();
            format!("one of [{}]", values.join(", "))
        }
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::{format_value, locator, parse, ValidateArgs};
    use serde_json::json;
    use std::path::PathBuf;
    use zgraph_dsl::Location;

    #[test]
    fn parse_accepts_flags_in_any_order() {
        let args = parse(&[
            "--json".to_string(),
            "graph.json".to_string(),
            "--assets-dir".to_string(),
            "assets".to_string(),
        ])
        .expect("valid args");
        assert_eq!(
            args,
            ValidateArgs {
                graph: PathBuf::from("graph.json"),
                json: true,
                assets_dir: Some(PathBuf::from("assets")),
            }
        );
    }

    #[test]
    fn parse_rejects_missing_graph_and_unknown_flags() {
        assert!(parse(&["--json".to_string()]).is_err());
        assert!(parse(&["--nope".to_string(), "g.json".to_string()]).is_err());
        assert!(parse(&["--assets-dir".to_string()]).is_err());
        assert!(parse(&["a.json".to_string(), "b.json".to_string()]).is_err());
    }

    #[test]
    fn locator_covers_all_levels() {
        assert_eq!(locator(&Location::default()), "");
        assert_eq!(
            locator(&Location {
                node_id: Some("n1".to_string()),
                ..Location::default()
            }),
            "node 'n1'"
        );
        assert_eq!(
            locator(&Location {
                edge_index: Some(3),
                port: Some("query".to_string()),
                ..Location::default()
            }),
            "edges[3].to_input_port"
        );
        assert_eq!(
            locator(&Location {
                edge_index: Some(1),
                ..Location::default()
            }),
            "edges[1]"
        );
        assert_eq!(
            locator(&Location {
                node_id: Some("n1".to_string()),
                port: Some("x".to_string()),
                ..Location::default()
            }),
            "node 'n1'.inputs['x']"
        );
    }

    #[test]
    fn format_value_renders_allowed_arrays() {
        assert_eq!(format_value(&json!(["a", "b"])), "one of [a, b]");
        assert_eq!(format_value(&json!(3)), "3");
        assert_eq!(format_value(&json!({"a": 1})), "{\"a\":1}");
    }
}
