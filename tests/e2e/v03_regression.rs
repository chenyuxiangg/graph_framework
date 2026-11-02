//! V5a（v0.3）回归：默认（无 skills/MCP/外部 harness）路径与 v0.2 golden 不变。
//!
//! 语义依据：DR-008 §0 兼容红线、spec 07 AC-18 f / AC-19 g / AC-20 a 的「默认零配置
//! 路径逐字节不变」；trace 不升 v7（仍 v6）。
//!
//! 数据通路：真 `zgraph` 二进制 + 真 fake LLM provider + 真 SQLite trace。

#![cfg(unix)]

#[path = "cli_support/mod.rs"]
mod cli_support;
#[path = "v03_support/mod.rs"]
mod v03;

use std::path::Path;

use serde_json::{json, Value};

use cli_support::{assert_exit, json_stdout};
use v03::{agent, assets, fake_llm, llm_call_count, llm_requests, run, run_row, write_agent};

/// 默认无 v0.3 配置的 run。
fn default_run(dir: &Path) -> (std::process::Output, std::path::PathBuf) {
    let assets_root = assets(dir);
    write_agent(&assets_root, &agent("parser", &[]));
    let script = fake_llm(
        &assets_root,
        json!([{"kind": "text", "content": "default", "stop_reason": "EndTurn",
                "usage": {"prompt_tokens": 10, "completion_tokens": 2}}]),
    );
    let db = dir.join("trace.db");
    let output = run(dir, &db, "agent_one.json", &assets_root, &script, &[]);
    (output, db)
}

/// v0.2 默认路径：run/trace/replay 与 v0.2 golden 语义一致（schema 仍 v6、无 v0.3 表）。
#[test]
fn v02_default_agent_path_golden_unchanged() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (output, db) = default_run(dir.path());
    assert_exit(&output, 0, "默认 agent run");
    let payload = json_stdout(&output);
    assert_eq!(payload["status"], json!("Completed"));
    assert_eq!(payload["run_output"]["result"]["content"], json!("default"));
    let run_id = payload["run_id"].as_str().expect("run_id").to_string();

    // v0.2 观测量不变：每轮 1 条 llm_calls、请求快照结构（model/messages/tools/...）。
    assert_eq!(llm_call_count(&db, &run_id), 1);
    let requests = llm_requests(&db, &run_id);
    let request = &requests[0];
    for key in [
        "model",
        "messages",
        "tools",
        "temperature",
        "max_tokens",
        "stream",
    ] {
        assert!(request.get(key).is_some(), "请求快照字段 {key}: {request}");
    }
    assert!(
        request["tools"].as_array().expect("tools").is_empty(),
        "默认无 v0.3 工具"
    );

    // v0.4/D5 起 trace schema 全局为 v7（tool_calls）；v0.3 无专属表/列。
    let conn = cli_support::read_only_connect(&db);
    let version: i64 = conn
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .expect("user_version");
    assert_eq!(
        version, 7,
        "当前 trace schema 为 v7（v0.4/D5），v0.3 不新增表"
    );
    let tables: Vec<String> = {
        let mut statement = conn
            .prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
            .expect("tables");
        statement
            .query_map([], |row| row.get(0))
            .expect("query")
            .map(|row| row.expect("table"))
            .collect()
    };
    for forbidden in ["skills", "mcp", "harness", "adapters"] {
        assert!(
            !tables.iter().any(|name| name.contains(forbidden)),
            "不得新增 {forbidden} 表: {tables:?}"
        );
    }
    let streaming_columns: Vec<String> = {
        let mut statement = conn
            .prepare("PRAGMA table_info(streaming_events)")
            .expect("table_info");
        statement
            .query_map([], |row| row.get::<_, String>(1))
            .expect("query")
            .map(|row| row.expect("column"))
            .collect()
    };
    for forbidden in ["skill", "mcp", "harness", "adapter", "server"] {
        assert!(
            !streaming_columns
                .iter()
                .any(|name| name.contains(forbidden)),
            "不得新增 {forbidden} 列: {streaming_columns:?}"
        );
    }
    drop(conn);

    // L1 replay == trace；L2 outputs matched（--record=full）。
    let trace = v03::trace_json(&db, &run_id);
    let replay_events = v03::replay_events_json(&db, &run_id);
    assert_eq!(replay_events.stdout, trace.stdout);
    let replay_outputs = v03::replay_outputs_json(&db, &run_id);
    assert_exit(&replay_outputs, 0, "L2 outputs replay");
    let l2 = json_stdout(&replay_outputs);
    assert_eq!(l2["matched"], json!(true));
    assert_eq!(l2["recorded_outputs"], json!(1));
    // graph_hash 不含 v0.3 配置（仍为图 JSON hash）。
    let (_, _, _, hash, _, _) = run_row(&db, &run_id);
    assert_eq!(hash, "48a8e8f73433f967");
}

/// 无 skills/MCP/harness 配置：stderr 无 v0.3 启动行、目录无额外标记文件（零额外 IO）。
#[test]
fn no_v03_config_produces_no_extra_processes_or_files() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (output, _db) = default_run(dir.path());
    assert_exit(&output, 0, "默认 run");
    let stderr = v03::err(&output);
    for forbidden in ["mcp:", "starting server", "adapter", "skill"] {
        assert!(
            !stderr.to_lowercase().contains(forbidden),
            "默认路径不得出现 v0.3 进程/加载日志（{forbidden}）: {stderr}"
        );
    }
    // 资产目录只应含本测试写入的 assets/（agents/prompt/tools/fake_llm）+ trace.db。
    let mut names: Vec<String> = std::fs::read_dir(dir.path())
        .expect("read dir")
        .map(|entry| {
            entry
                .expect("entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        // trace DB 的 WAL 侧车文件属存储自身（既有行为），不属 v0.3 额外产物。
        .filter(|name| !name.starts_with("trace.db"))
        .collect();
    names.sort();
    assert_eq!(
        names,
        vec!["assets".to_string()],
        "默认路径不得创建额外文件/进程标记: {names:?}"
    );
}

/// 代表图 `graph_hash`/validate 不变（v0.3 不改 DSL；skills/MCP/harness 不进 hash）。
#[test]
fn representative_graph_hashes_unchanged() {
    let dir = tempfile::tempdir().expect("tempdir");
    let cases: [(&str, &str); 4] = [
        ("customer_ticket.json", "ec320e4244b365ab"),
        ("valid_parallel.json", "b15f162f7f87fd58"),
        ("valid_gate_defer.json", "2ff1339bd3d6d928"),
        ("valid_foreach.json", "10cd8fff9c8f85fb"),
    ];
    let assets_root = assets(dir.path());
    write_agent(&assets_root, &agent("parser", &[]));
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
        let payload: Value = json_stdout(&output);
        assert_eq!(payload["ok"], json!(true), "{name}");
        assert_eq!(payload["graph_hash"], json!(expected_hash), "{name}");
    }
}
