//! D5c（v0.4）回归：默认零配置路径与 v0.3 兼容矩阵不变。
//!
//! 语义依据：DR-012 §5 兼容矩阵、spec 07 AC-21 g/AC-22 d/AC-23 e/AC-24 j、DR-013 §4。
//! 数据通路：真 `zgraph` + 真 fake LLM + 真 SQLite trace v7。
//!
//! 覆盖：默认（无 `--allow-builtin-tools`/`--allow-bash`/workspace）不注册任何 builtin
//! 工具、无 workspace 副作用、trace v7 仅 additive（无 ToolCall 行/`tool_calls` 空）、
//! events/outputs 回放不变；代表图 `graph_hash`/validate 不变；默认路径零额外文件。

#![cfg(unix)]

#[path = "cli_support/mod.rs"]
mod cli_support;
#[path = "v04_support/mod.rs"]
mod support;
#[path = "v03_support/mod.rs"]
mod v03;

use std::path::Path;

use serde_json::json;

use cli_support::{assert_exit, json_stdout};
use support::{
    agent_with_tools, completed_run_id, replay_events_json, replay_outputs_json, run_scenario,
    trace_stats, trace_tool_call_lines, tree_snapshot, user_version, workspace, write_text,
};

/// 默认零配置 run。
fn default_run(dir: &Path) -> (support::RunOutcome, String) {
    let outcome = run_scenario(
        dir,
        &agent_with_tools(&[]),
        support::text_reply("default"),
        &[],
    );
    let run_id = completed_run_id(&outcome, "默认零配置 run");
    (outcome, run_id)
}

// ---------------------------------------------------------------------------
// 默认零配置：无 builtin 工具/零副作用/trace v7 additive/回放不变
// ---------------------------------------------------------------------------

#[test]
fn v04_default_zero_config_path_is_additive_only() {
    let dir = tempfile::tempdir().expect("tempdir");
    let ws = workspace(dir.path());
    write_text(&ws.join("keep.txt"), "keep\n");
    let snapshot = tree_snapshot(&ws);
    let (outcome, run_id) = default_run(dir.path());
    let db = &outcome.db;
    // 请求 schema 不含任何 builtin 工具。
    let requests = v03::llm_requests(db, &run_id);
    assert!(
        requests[0]["tools"].as_array().expect("tools").is_empty(),
        "默认不注册 builtin 工具"
    );
    // trace v7 仅增表：无 ToolCall 行、无 tool_calls 记录、stats 不追加计数。
    assert_eq!(user_version(db), 7);
    assert_eq!(support::tool_call_count(db, &run_id), 0);
    assert!(trace_tool_call_lines(db, &run_id).is_empty());
    let stats = trace_stats(db, &run_id);
    assert!(!String::from_utf8_lossy(&stats.stdout).contains("tool_calls"));
    // 零 workspace 副作用 / 无 v0.4 进程日志。
    assert_eq!(tree_snapshot(&ws), snapshot, "默认路径不得触碰 workspace");
    let stderr = v03::err(&outcome.output);
    for forbidden in ["builtin", "sandbox", "workspace"] {
        assert!(
            !stderr.to_lowercase().contains(forbidden),
            "默认 stderr 不得出现 v0.4 配置日志（{forbidden}）: {stderr}"
        );
    }
    // events/outputs 回放与 v0.3 语义一致。
    let trace = support::trace_json(db, &run_id);
    let events = replay_events_json(db, &run_id);
    assert_eq!(events.stdout, trace.stdout);
    let outputs = replay_outputs_json(db, &run_id);
    assert_exit(&outputs, 0, "L2 outputs replay");
    assert_eq!(json_stdout(&outputs)["matched"], json!(true));
    // graph_hash 不因 CLI opt-in 改变。
    let (_, _, _, hash, _, _) = v03::run_row(db, &run_id);
    assert_eq!(hash, "48a8e8f73433f967");
}

// ---------------------------------------------------------------------------
// 代表图 hash / validate 不变（v0.4 不改 DSL/图语义）
// ---------------------------------------------------------------------------

#[test]
fn v04_representative_graph_hashes_unchanged() {
    let dir = tempfile::tempdir().expect("tempdir");
    let cases: [(&str, &str); 4] = [
        ("customer_ticket.json", "ec320e4244b365ab"),
        ("valid_parallel.json", "b15f162f7f87fd58"),
        ("valid_gate_defer.json", "2ff1339bd3d6d928"),
        ("valid_foreach.json", "10cd8fff9c8f85fb"),
    ];
    for (name, expected_hash) in cases {
        let path = cli_support::repo_root()
            .join("tests/fixtures/graphs")
            .join(name);
        let output = cli_support::run_cli(
            &[
                "validate".to_string(),
                cli_support::arg(&path),
                "--json".to_string(),
            ],
            dir.path(),
        );
        assert_exit(&output, 0, name);
        let payload = json_stdout(&output);
        assert_eq!(payload["ok"], json!(true), "{name}");
        assert_eq!(payload["graph_hash"], json!(expected_hash), "{name}");
    }
}

// ---------------------------------------------------------------------------
// 默认路径零额外文件（除 trace DB/WAL 外）
// ---------------------------------------------------------------------------

#[test]
fn v04_default_path_creates_no_extra_files() {
    let dir = tempfile::tempdir().expect("tempdir");
    let ws = workspace(dir.path());
    write_text(&ws.join("a.txt"), "a\n");
    let (outcome, _run_id) = default_run(dir.path());
    let stderr = v03::err(&outcome.output);
    for forbidden in ["mcp:", "adapter", "sandbox", "allow-builtin"] {
        assert!(
            !stderr.to_lowercase().contains(forbidden),
            "默认路径不得出现额外进程/配置日志（{forbidden}）: {stderr}"
        );
    }
    let mut names: Vec<String> = std::fs::read_dir(dir.path())
        .expect("read dir")
        .map(|entry| {
            entry
                .expect("entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .filter(|name| !name.starts_with("trace.db"))
        .collect();
    names.sort();
    assert_eq!(
        names,
        vec!["assets".to_string(), "ws".to_string()],
        "默认路径不得创建额外文件: {names:?}"
    );
    assert_eq!(std::fs::read_to_string(ws.join("a.txt")).expect("a"), "a\n");
}
