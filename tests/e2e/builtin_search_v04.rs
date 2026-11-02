//! D5c（v0.4 / M9）AC-22：内置搜索工具（grep/glob）**真 CLI 链路**。
//!
//! 语义依据：DR-013 §3.6/§3.7/§9、spec 07 AC-22、05 §5.15.2。
//! 数据通路：真 `zgraph` + 真 fake LLM + 真 Rust 搜索工具 + 真 SQLite trace v7。
//!
//! 覆盖：a regex/字面/大小写/glob 过滤；b `.gitignore`/隐藏/二进制/排序/结果上限
//! （1000 条、32 KiB）；c glob 排序/隐藏/仅文件/数量上限/越界；d 无 opt-in 默认不可见。

#![cfg(unix)]

#[path = "cli_support/mod.rs"]
mod cli_support;
#[path = "v04_support/mod.rs"]
mod support;
#[path = "v03_support/mod.rs"]
mod v03;

use std::path::Path;

use serde_json::{json, Value};

use cli_support::{assert_exit, json_stdout};
use support::{
    agent_with_tools, builtin_flags, calls_with_text, completed_run_id, run_scenario, tool_call,
    tool_row, write_file, write_text,
};

/// 结果 `matches` 的 `(path,line)` 序列。
fn match_positions(result: &Value) -> Vec<(String, i64)> {
    result["matches"]
        .as_array()
        .expect("matches array")
        .iter()
        .map(|item| {
            (
                item["path"].as_str().expect("path").to_string(),
                item["line"].as_i64().expect("line"),
            )
        })
        .collect()
}

/// 结果 `paths` 序列。
fn paths(result: &Value) -> Vec<String> {
    result["paths"]
        .as_array()
        .expect("paths array")
        .iter()
        .map(|item| item.as_str().expect("path").to_string())
        .collect()
}

/// LLM tool 消息（按 call_id）。
fn tool_message(db: &Path, run_id: &str, call_id: &str) -> String {
    for request in v03::llm_requests(db, run_id) {
        for message in request["messages"].as_array().cloned().unwrap_or_default() {
            if message["role"] == json!("tool") && message["tool_call_id"] == json!(call_id) {
                return message["content"].as_str().unwrap_or_default().to_string();
            }
        }
    }
    panic!("tool message {call_id} not found");
}

/// 基础搜索 workspace。
fn search_workspace(ws: &Path) {
    write_text(&ws.join("visible/a.rs"), "NEEDLE alpha\n");
    write_text(&ws.join("visible/b.rs"), "needle beta\n");
    write_text(&ws.join("visible/c.txt"), "NEEDLE gamma\n");
    write_text(&ws.join("visible/lines.txt"), "NEEDLE one\nNEEDLE two\n");
    write_text(&ws.join(".gitignore"), "ignored.txt\nignored_dir/\n");
    write_text(&ws.join("ignored.txt"), "NEEDLE ignored\n");
    write_text(&ws.join("ignored_dir/x.txt"), "NEEDLE ignored dir\n");
    write_text(&ws.join(".hidden.txt"), "NEEDLE hidden\n");
    write_file(&ws.join("bin.dat"), b"\x00NEEDLE binary\n");
}

// ---------------------------------------------------------------------------
// AC-22 a：regex / 字面 / 大小写 / glob 过滤
// ---------------------------------------------------------------------------

#[test]
fn ac22a_grep_regex_fixed_case_and_glob_filter() {
    let dir = tempfile::tempdir().expect("tempdir");
    let ws = support::workspace(dir.path());
    search_workspace(&ws);
    let calls = calls_with_text(vec![
        tool_call(
            "g1",
            "builtin__grep",
            json!({"pattern": "NEEDLE", "path": "visible"}),
        ),
        tool_call(
            "g2",
            "builtin__grep",
            json!({"pattern": "needle", "path": "visible", "case_sensitive": false}),
        ),
        tool_call(
            "g3",
            "builtin__grep",
            json!({"pattern": "NEEDLE*", "path": "visible", "fixed_strings": true}),
        ),
        tool_call(
            "g4",
            "builtin__grep",
            json!({"pattern": "NEEDLE", "path": "visible", "glob": "**/*.rs"}),
        ),
    ]);
    let flags = builtin_flags(&ws, &[]);
    let outcome = run_scenario(
        dir.path(),
        &agent_with_tools(&["builtin__grep"]),
        calls,
        &flags,
    );
    let run_id = completed_run_id(&outcome, "AC-22 a grep");
    let db = &outcome.db;

    let g1 = tool_row(db, &run_id, "g1");
    let result = g1.result.expect("g1");
    let positions = match_positions(&result);
    assert_eq!(
        positions,
        vec![
            ("visible/a.rs".to_string(), 1),
            ("visible/c.txt".to_string(), 1),
            ("visible/lines.txt".to_string(), 1),
            ("visible/lines.txt".to_string(), 2),
        ],
        "默认大小写敏感 + 排序 (path,line)"
    );
    assert_eq!(result["truncated"], json!(false));
    assert_eq!(result["total"], json!(4));
    // 大小写不敏感：b.rs 的 needle 也命中。
    let g2 = tool_row(db, &run_id, "g2");
    assert_eq!(g2.result.expect("g2")["total"], json!(5));
    // fixed_strings：`NEEDLE*` 作为字面量 → 无匹配。
    let g3 = tool_row(db, &run_id, "g3");
    assert_eq!(g3.result.expect("g3")["total"], json!(0));
    // glob 过滤：仅 *.rs。
    let g4 = tool_row(db, &run_id, "g4");
    assert_eq!(
        match_positions(&g4.result.expect("g4")),
        vec![("visible/a.rs".to_string(), 1)],
        "glob 过滤"
    );
    assert!(tool_message(db, &run_id, "g1").contains("NEEDLE"));
}

// ---------------------------------------------------------------------------
// AC-22 b：.gitignore/隐藏/二进制策略 + 1000 条/32 KiB 上限 + 排序确定
// ---------------------------------------------------------------------------

#[test]
fn ac22b_grep_gitignore_hidden_binary_and_result_caps() {
    let dir = tempfile::tempdir().expect("tempdir");
    let ws = support::workspace(dir.path());
    search_workspace(&ws);
    // 60 条长行匹配（每行 600+ 字符）→ 结果 JSON 超 32 KiB 截断。
    let long_line = "NEEDLE ".to_string() + &"p".repeat(600);
    let mut long_body = String::new();
    for _ in 0..60 {
        long_body.push_str(&long_line);
        long_body.push('\n');
    }
    write_text(&ws.join("long.txt"), &long_body);
    // 150 行匹配 → 默认 max_results=100 截断。
    let mut many = String::new();
    for index in 0..150 {
        many.push_str(&format!("NEEDLE line {index}\n"));
    }
    write_text(&ws.join("many.txt"), &many);

    let calls = calls_with_text(vec![
        tool_call("g1", "builtin__grep", json!({"pattern": "NEEDLE"})),
        tool_call(
            "g2",
            "builtin__grep",
            json!({"pattern": "NEEDLE", "path": "long.txt", "max_results": 1000}),
        ),
        tool_call(
            "g3",
            "builtin__grep",
            json!({"pattern": "NEEDLE", "path": "many.txt", "max_results": 1000}),
        ),
        tool_call(
            "g4",
            "builtin__grep",
            json!({"pattern": "NEEDLE", "path": "many.txt"}),
        ),
    ]);
    let flags = builtin_flags(&ws, &[]);
    let outcome = run_scenario(
        dir.path(),
        &agent_with_tools(&["builtin__grep"]),
        calls,
        &flags,
    );
    let run_id = completed_run_id(&outcome, "AC-22 b grep caps");
    let db = &outcome.db;

    let g1 = tool_row(db, &run_id, "g1");
    let result = g1.result.expect("g1");
    let paths: Vec<String> = match_positions(&result)
        .into_iter()
        .map(|(p, _)| p)
        .collect();
    assert!(
        !paths.iter().any(|p| p.contains("ignored")),
        "尊重 .gitignore: {paths:?}"
    );
    assert!(
        !paths.iter().any(|p| p.contains(".hidden")),
        "默认跳过隐藏文件: {paths:?}"
    );
    assert!(
        !paths.iter().any(|p| p == "bin.dat"),
        "跳过二进制: {paths:?}"
    );
    let mut sorted = paths.clone();
    sorted.sort();
    assert_eq!(paths, sorted, "结果按 (path,line) 确定排序");
    // 32 KiB 截断：60 条全命中但只回填部分（total 记命中数、truncated=true、JSON ≤32 KiB）。
    let g2 = tool_row(db, &run_id, "g2");
    let result = g2.result.expect("g2");
    assert_eq!(result["total"], json!(60), "命中条数");
    assert_eq!(result["truncated"], json!(true), "结果超 32 KiB 截断");
    assert!(
        result["matches"].as_array().expect("matches").len() < 60,
        "截断后回填条数 < total"
    );
    let raw = g2.raw_result_json.as_deref().expect("raw result");
    assert!(
        raw.len() <= 32_768,
        "结果 JSON ≤ 32 KiB，实际 {}",
        raw.len()
    );
    // 默认 100 条上限。
    let g4 = tool_row(db, &run_id, "g4");
    let result = g4.result.expect("g4");
    assert_eq!(result["total"], json!(100));
    assert_eq!(result["truncated"], json!(true));
    // max_results=1000 时 150 条全返回且按行升序。
    let g3 = tool_row(db, &run_id, "g3");
    let result = g3.result.expect("g3");
    let lines: Vec<i64> = match_positions(&result)
        .into_iter()
        .map(|(_, line)| line)
        .collect();
    assert_eq!(
        lines,
        (1..=150).collect::<Vec<i64>>(),
        "排序确定 (path,line)"
    );
    assert_eq!(result["truncated"], json!(false));
}

// ---------------------------------------------------------------------------
// AC-22 c：glob 排序/隐藏/仅文件/上限/越界
// ---------------------------------------------------------------------------

#[test]
fn ac22c_glob_sort_hidden_limits_and_jail() {
    let dir = tempfile::tempdir().expect("tempdir");
    let ws = support::workspace(dir.path());
    write_text(&ws.join("a.txt"), "a\n");
    write_text(&ws.join("ignored.txt"), "i\n");
    write_text(&ws.join(".hidden.txt"), "h\n");
    write_text(&ws.join("sub/b.txt"), "b\n");
    write_text(&ws.join("sub/c.rs"), "c\n");
    std::fs::create_dir_all(ws.join("empty-dir")).expect("empty dir");

    let calls = calls_with_text(vec![
        tool_call("b1", "builtin__glob", json!({"pattern": "**/*.txt"})),
        tool_call(
            "b2",
            "builtin__glob",
            json!({"pattern": "**/*", "include_hidden": true}),
        ),
        tool_call(
            "b3",
            "builtin__glob",
            json!({"pattern": "**/*.txt", "max_results": 2}),
        ),
        tool_call(
            "b4",
            "builtin__glob",
            json!({"pattern": "*", "path": "sub"}),
        ),
        tool_call(
            "b5",
            "builtin__glob",
            json!({"pattern": "*", "path": "../outside"}),
        ),
    ]);
    let flags = builtin_flags(&ws, &[]);
    let outcome = run_scenario(
        dir.path(),
        &agent_with_tools(&["builtin__glob"]),
        calls,
        &flags,
    );
    let run_id = completed_run_id(&outcome, "AC-22 c glob");
    let db = &outcome.db;

    // 默认跳过隐藏；glob 不受 .gitignore 影响；排序确定；目录不返回。
    let b1 = tool_row(db, &run_id, "b1");
    let result = b1.result.expect("b1");
    assert_eq!(
        paths(&result),
        vec![
            "a.txt".to_string(),
            "ignored.txt".to_string(),
            "sub/b.txt".to_string()
        ],
        "路径排序 + 默认跳过隐藏 + 仅文件"
    );
    assert_eq!(result["total"], json!(3));
    // include_hidden=true 列出隐藏文件。
    let b2 = tool_row(db, &run_id, "b2");
    let result = b2.result.expect("b2");
    assert!(paths(&result).iter().any(|path| path == ".hidden.txt"));
    assert!(
        !paths(&result).iter().any(|path| path == "empty-dir"),
        "不返回目录"
    );
    // 数量上限：total 为完整命中数，返回被截断。
    let b3 = tool_row(db, &run_id, "b3");
    let result = b3.result.expect("b3");
    assert_eq!(result["total"], json!(3));
    assert_eq!(paths(&result).len(), 2);
    assert_eq!(result["truncated"], json!(true));
    // path 限定子目录。
    let b4 = tool_row(db, &run_id, "b4");
    assert_eq!(
        paths(&b4.result.expect("b4")),
        vec!["sub/b.txt".to_string(), "sub/c.rs".to_string()]
    );
    // 越界拒绝。
    let b5 = tool_row(db, &run_id, "b5");
    assert_eq!(
        b5.error.expect("b5")["error"]["reason"],
        json!("path_escape")
    );
    let _ = tool_message(db, &run_id, "b1");
}

// ---------------------------------------------------------------------------
// AC-22 d：无 opt-in 默认不可见（grep/glob 不注册）。
// ---------------------------------------------------------------------------

#[test]
fn ac22d_search_tools_require_opt_in() {
    let dir = tempfile::tempdir().expect("tempdir");
    let outcome = run_scenario(
        dir.path(),
        &agent_with_tools(&["builtin__grep", "builtin__glob"]),
        support::text_reply("x"),
        &[],
    );
    assert_exit(&outcome.output, 1, "无 --allow-builtin-tools");
    let payload = json_stdout(&outcome.output);
    let run_id = payload["run_id"].as_str().expect("run_id").to_string();
    let (_, code, message, _, _, _) = v03::run_row(&outcome.db, &run_id);
    assert_eq!(code.as_deref(), Some("HARNESS_CONFIG_LOAD"));
    assert!(
        message
            .as_deref()
            .is_some_and(|text| text.contains("builtin_tools_not_allowed")),
        "{message:?}"
    );
    assert_eq!(support::tool_call_count(&outcome.db, &run_id), 0);
}
