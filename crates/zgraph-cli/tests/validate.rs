//! `zgraph validate` CLI 集成测试：退出码、人类可读输出、`--json` 容器。
//!
//! 直接调用 `target/debug/zgraph`（`CARGO_BIN_EXE_zgraph`），不引入网络依赖。

use std::path::PathBuf;
use std::process::{Command, Output};

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("repo root")
}

fn fixtures() -> PathBuf {
    repo_root().join("tests/fixtures/graphs")
}

fn assets() -> PathBuf {
    repo_root().join("tests/fixtures/assets")
}

fn zgraph(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_zgraph"))
        .args(args)
        .output()
        .expect("zgraph binary runs")
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).to_string()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).to_string()
}

fn code(output: &Output) -> i32 {
    output.status.code().expect("exit code")
}

#[test]
fn version_flag_prints_versions() {
    let output = zgraph(&["--version"]);
    assert_eq!(code(&output), 0);
    assert!(stdout(&output).contains("zgraph 0.1.0"));
    assert!(stdout(&output).contains("DSL 1.0"));
}

// ISSUE-059（REV-043）：validate 退出码统一为 06 §6.10.5 权威表——
// 用法错误 → 2，IO/内部 → 1；M1 临时口径 3 已废弃。
#[test]
fn usage_errors_exit_2() {
    for args in [
        vec!["validate"],
        vec!["validate", "--nope"],
        vec!["validate", "--assets-dir"],
        vec!["unknown-command"],
    ] {
        let output = zgraph(&args);
        assert_eq!(code(&output), 2, "args={args:?}: {}", stderr(&output));
    }
}

#[test]
fn unreadable_graph_exits_1() {
    let output = zgraph(&["validate", "/definitely/not/a/file.json"]);
    assert_eq!(code(&output), 1, "{}", stderr(&output));
    assert!(
        stderr(&output).contains("cannot read"),
        "{}",
        stderr(&output)
    );
}

// 06 §6.10.1：`-v/--verbose` 是全局参数，可出现在子命令前后，且不改变输出/退出码。
#[test]
fn verbose_flag_is_accepted_globally() {
    let graph = fixtures().join("customer_ticket.json");
    let path = graph.to_str().expect("utf8 path");
    let assets_dir = assets();
    let assets_path = assets_dir.to_str().expect("utf8 path");
    for args in [
        vec!["-v", "validate", path, "--assets-dir", assets_path],
        vec!["validate", path, "--assets-dir", assets_path, "--verbose"],
        vec!["--version", "-v"],
    ] {
        let output = zgraph(&args);
        assert_eq!(code(&output), 0, "args={args:?}: {}", stderr(&output));
    }
}

// v0.1 颜色决策（06 §6.10.1，REV-028 #6）：全部输出为纯文本，不含 ANSI 转义，
// 因此 `NO_COLOR` 语义天然满足；未来引入颜色时必须同时实现 TTY + `NO_COLOR` 判定。
#[test]
fn output_has_no_ansi_escapes_and_honours_no_color() {
    let graph = fixtures().join("bad_port.json");
    let path = graph.to_str().expect("utf8 path");
    for no_color in [None, Some("1")] {
        for args in [
            vec!["--help"],
            vec!["validate", path],
            vec!["validate", path, "--json"],
        ] {
            let mut command = Command::new(env!("CARGO_BIN_EXE_zgraph"));
            command.args(&args);
            if let Some(value) = no_color {
                command.env("NO_COLOR", value);
            }
            let output = command.output().expect("zgraph binary runs");
            for text in [stdout(&output), stderr(&output)] {
                assert!(!text.contains('\u{1b}'), "ANSI escape found: {text}");
            }
        }
    }
}

#[test]
fn valid_graph_exits_0_with_success_line() {
    let graph = fixtures().join("customer_ticket.json");
    let assets_dir = assets();
    let output = zgraph(&[
        "validate",
        graph.to_str().expect("utf8 path"),
        "--assets-dir",
        assets_dir.to_str().expect("utf8 path"),
    ]);
    assert_eq!(code(&output), 0, "{}", stderr(&output));
    let text = stdout(&output);
    assert!(
        text.contains("✓ graph valid: 6 nodes, 6 edges, 0 subgraphs (graph_hash="),
        "{text}"
    );
    assert!(stderr(&output).is_empty());
}

#[test]
fn valid_graph_json_report_has_hash() {
    let graph = fixtures().join("customer_ticket.json");
    let output = zgraph(&[
        "validate",
        graph.to_str().expect("utf8 path"),
        "--json",
        "--assets-dir",
        assets().to_str().expect("utf8 path"),
    ]);
    assert_eq!(code(&output), 0, "{}", stderr(&output));
    let report: serde_json::Value = serde_json::from_str(&stdout(&output)).expect("json report");
    assert_eq!(report["ok"], serde_json::json!(true));
    assert_eq!(report["error_count"], serde_json::json!(0));
    assert_eq!(report["errors"], serde_json::json!([]));
    let hash = report["graph_hash"].as_str().expect("hash string");
    assert_eq!(hash.len(), 16);
}

#[test]
fn invalid_graph_exits_2_with_human_readable_error() {
    let graph = fixtures().join("bad_port.json");
    let output = zgraph(&["validate", graph.to_str().expect("utf8 path")]);
    assert_eq!(code(&output), 2);
    let text = stderr(&output);
    assert!(text.contains("error[DSL_INVALID_PORT]"), "{text}");
    assert!(text.contains("edges[0].to_input_port"), "{text}");
    assert!(text.contains("expected:"), "{text}");
    assert!(text.contains("actual:"), "{text}");
    assert!(stdout(&output).is_empty());
}

#[test]
fn invalid_graph_json_report_is_stdout_only() {
    let graph = fixtures().join("bad_port.json");
    let output = zgraph(&["validate", graph.to_str().expect("utf8 path"), "--json"]);
    assert_eq!(code(&output), 2);
    assert!(stderr(&output).is_empty(), "{}", stderr(&output));
    let report: serde_json::Value = serde_json::from_str(&stdout(&output)).expect("json report");
    assert_eq!(report["ok"], serde_json::json!(false));
    assert_eq!(report["error_count"], serde_json::json!(1));
    assert_eq!(report["truncated"], serde_json::json!(false));
    let first = &report["errors"][0];
    assert_eq!(first["code"], serde_json::json!("DSL_INVALID_PORT"));
    assert_eq!(first["location"]["edge_index"], serde_json::json!(0));
    assert_eq!(first["location"]["port"], serde_json::json!("zzz"));
}

#[test]
fn agent_assets_are_only_checked_when_assets_dir_is_given() {
    let graph = fixtures().join("bad_agent_not_found.json");
    let path = graph.to_str().expect("utf8 path");
    let without = zgraph(&["validate", path]);
    assert_eq!(code(&without), 0, "R14c skipped: {}", stderr(&without));

    let with = zgraph(&[
        "validate",
        path,
        "--assets-dir",
        assets().to_str().expect("utf8 path"),
    ]);
    assert_eq!(code(&with), 2);
    assert!(stderr(&with).contains("error[DSL_AGENT_NOT_FOUND]"));
}

#[test]
fn zgraph_assets_env_is_honoured() {
    let graph = fixtures().join("bad_agent_not_found.json");
    let output = Command::new(env!("CARGO_BIN_EXE_zgraph"))
        .args(["validate", graph.to_str().expect("utf8 path")])
        .env("ZGRAPH_ASSETS", assets())
        .output()
        .expect("runs");
    assert_eq!(code(&output), 2, "{}", stderr(&output));
    assert!(stderr(&output).contains("DSL_AGENT_NOT_FOUND"));
}
