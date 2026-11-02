//! D5c（v0.4 / M9）AC-21：内置文件工具 + workspace jail **真 CLI 链路**。
//!
//! 语义依据：DR-013 §2/§3/§9、spec 07 AC-21、05 §5.15.1。
//! 数据通路（禁止 mock）：真 `zgraph` 二进制 + 真 fake LLM provider（`--fake-llm`）+
//! 真 Rust 内置工具执行 + 真 workspace 文件系统 + 真 SQLite trace v7。
//!
//! 覆盖：a read（offset/limit/UTF-8/二进制/不存在/目录/超限/绝对路径）；
//! b write 原子/覆盖/create_parents/大小上限/allow-path 只读；c edit 唯一/0/多匹配/
//! replace_all；d apply_patch 真实 `git diff` 单/多文件严格原子 + unsupported；
//! e diff 预览（不写文件/空 diff）；f jail 负例（`..`/绝对/symlink 出根/兄弟前缀）+
//! allow-path 只读；g 无 opt-in 默认不可见/零副作用；预检配置错误 exit 3 且不建 DB。

#![cfg(unix)]

#[path = "cli_support/mod.rs"]
mod cli_support;
#[path = "v04_support/mod.rs"]
mod support;
#[path = "v03_support/mod.rs"]
mod v03;

use std::path::Path;
use std::process::Command;

use serde_json::json;

use cli_support::{assert_exit, json_stdout};
use support::{
    agent_with_tools, allow_dir, builtin_flags, calls_with_text, completed_run_id, read_text,
    run_scenario, tool_call, tool_row, tool_rows, tree_snapshot, workspace, write_file, write_text,
};

// ---------------------------------------------------------------------------
// 辅助
// ---------------------------------------------------------------------------

/// LLM 第二请求起的 tool 消息内容（按原生 call_id）。
fn tool_message(agent_dir: &Path, db: &Path, run_id: &str, call_id: &str) -> String {
    let requests = v03::llm_requests(db, run_id);
    for request in &requests {
        for message in request["messages"].as_array().cloned().unwrap_or_default() {
            if message["role"] == json!("tool") && message["tool_call_id"] == json!(call_id) {
                return message["content"].as_str().unwrap_or_default().to_string();
            }
        }
    }
    let _ = agent_dir;
    panic!("tool message {call_id} not found");
}

/// 生成真实 `git diff --no-index` 补丁（old 目录 → new 目录；strip=2 后目标为 ws 内相对路径）。
fn git_diff_patch(dir: &Path, old: &Path, new: &Path) -> String {
    let output = Command::new("git")
        .arg("diff")
        .arg("--no-index")
        .arg(old)
        .arg(new)
        .current_dir(dir)
        .output()
        .expect("run git diff --no-index");
    assert!(!output.stdout.is_empty(), "git diff produced a patch");
    String::from_utf8(output.stdout).expect("git diff output is UTF-8")
}

// ---------------------------------------------------------------------------
// AC-21 a：read offset/limit/UTF-8/二进制/不存在/目录/超限/绝对路径
// ---------------------------------------------------------------------------

#[test]
fn ac21a_read_offset_limit_and_text_policies() {
    let dir = tempfile::tempdir().expect("tempdir");
    let ws = workspace(dir.path());
    let allow = allow_dir(dir.path());
    write_text(&ws.join("lines.txt"), "l1\nl2\nl3\nl4\n");
    write_file(&ws.join("bin.dat"), b"\x00binary bytes");
    write_file(&ws.join("bad.txt"), b"bad\xffutf8\n");
    std::fs::create_dir_all(ws.join("sub")).expect("sub dir");
    write_file(&ws.join("huge.txt"), &vec![b'A'; 1_100_000]);
    write_text(&allow.join("ok.txt"), "allowed read\n");
    let abs_inside = ws
        .join("lines.txt")
        .canonicalize()
        .expect("canonical ws file");
    let abs_allow = allow
        .join("ok.txt")
        .canonicalize()
        .expect("canonical allow file");

    let calls = calls_with_text(vec![
        tool_call("r1", "builtin__read", json!({"path": "lines.txt"})),
        tool_call(
            "r2",
            "builtin__read",
            json!({"path": "lines.txt", "offset": 1, "limit": 2}),
        ),
        tool_call(
            "r3",
            "builtin__read",
            json!({"path": "lines.txt", "offset": 10, "limit": 2}),
        ),
        tool_call("r4", "builtin__read", json!({"path": "missing.txt"})),
        tool_call("r5", "builtin__read", json!({"path": "sub"})),
        tool_call("r6", "builtin__read", json!({"path": "bin.dat"})),
        tool_call("r7", "builtin__read", json!({"path": "bad.txt"})),
        tool_call("r8", "builtin__read", json!({"path": "huge.txt"})),
        tool_call(
            "r9",
            "builtin__read",
            json!({"path": abs_inside.to_string_lossy()}),
        ),
        tool_call(
            "r10",
            "builtin__read",
            json!({"path": abs_allow.to_string_lossy()}),
        ),
    ]);
    let flags = builtin_flags(&ws, std::slice::from_ref(&allow));
    let outcome = run_scenario(
        dir.path(),
        &agent_with_tools(&["builtin__read"]),
        calls,
        &flags,
    );
    let run_id = completed_run_id(&outcome, "AC-21 a read");
    let db = &outcome.db;

    let rows = tool_rows(db, &run_id);
    assert_eq!(rows.len(), 10, "10 次 read 各 1 行 tool_calls");
    let by_id = |id: &str| tool_row(db, &run_id, id);
    let r1 = by_id("r1");
    assert_eq!(
        r1.result.expect("r1 result")["content"],
        json!("l1\nl2\nl3\nl4")
    );
    assert_eq!(by_id("r2").result.expect("r2")["content"], json!("l2\nl3"));
    assert_eq!(by_id("r2").result.clone().unwrap()["lines"], json!(2));
    assert_eq!(by_id("r2").result.clone().unwrap()["total_lines"], json!(4));
    assert_eq!(by_id("r3").result.expect("r3")["content"], json!(""));
    assert_eq!(by_id("r3").result.clone().unwrap()["lines"], json!(0));
    assert_eq!(by_id("r2").status, "ok");
    for (id, reason) in [
        ("r4", "not_found"),
        ("r5", "is_directory"),
        ("r6", "binary_file"),
        ("r7", "invalid_utf8"),
        ("r8", "too_large"),
    ] {
        let row = by_id(id);
        assert_eq!(row.status, "error", "{id} 应失败");
        assert_eq!(
            row.error.expect("error json")["error"]["reason"],
            json!(reason),
            "{id} reason"
        );
        assert!(row.result.is_none(), "{id} 失败无 result");
        assert!(
            tool_message(dir.path(), db, &run_id, id).contains(reason),
            "{id} 工具错误喂回 LLM 应含 reason"
        );
    }
    assert_eq!(
        by_id("r9").result.expect("r9")["content"],
        json!("l1\nl2\nl3\nl4")
    );
    assert_eq!(
        by_id("r10").result.expect("r10")["content"],
        json!("allowed read")
    );
    // read 的 1 MiB 上限与 UTF-8 策略由工具内落实；trace 行保持 builtin 源/agent 节点。
    assert!(rows.iter().all(|row| row.source == "builtin"));
    assert!(rows.iter().all(|row| row.node_id == "agent"));
}

// ---------------------------------------------------------------------------
// AC-21 b：write 原子/覆盖/create_parents/allow-path 只读
// ---------------------------------------------------------------------------

#[test]
fn ac21b_write_atomic_overwrite_and_create_parents() {
    let dir = tempfile::tempdir().expect("tempdir");
    let ws = workspace(dir.path());
    let allow = allow_dir(dir.path());
    write_text(&ws.join("a.txt"), "orig\n");
    write_text(&allow.join("ok.txt"), "keep\n");
    let abs_allow = allow.join("ok.txt").canonicalize().expect("canonical");
    let before = tree_snapshot(&ws);

    let calls = calls_with_text(vec![
        // 已存在且未 overwrite → file_exists，且原文件不变。
        tool_call(
            "w1",
            "builtin__write",
            json!({"path": "a.txt", "content": "new"}),
        ),
        tool_call("r1", "builtin__read", json!({"path": "a.txt"})),
        // overwrite=true → 覆盖成功。
        tool_call(
            "w2",
            "builtin__write",
            json!({"path": "a.txt", "content": "new", "overwrite": true}),
        ),
        // 父目录缺失 → not_found（create_parents=false 不建目录）。
        tool_call(
            "w3",
            "builtin__write",
            json!({"path": "nested/dir/f.txt", "content": "x"}),
        ),
        tool_call("r2", "builtin__read", json!({"path": "nested/dir/f.txt"})),
        // create_parents=true → 建目录并写。
        tool_call(
            "w4",
            "builtin__write",
            json!({"path": "nested/dir/f.txt", "content": "x", "create_parents": true}),
        ),
        // 写 allow-path 目标 → path_read_only。
        tool_call(
            "w5",
            "builtin__write",
            json!({"path": abs_allow.to_string_lossy(), "content": "hack", "overwrite": true}),
        ),
    ]);
    let flags = builtin_flags(&ws, std::slice::from_ref(&allow));
    let outcome = run_scenario(
        dir.path(),
        &agent_with_tools(&["builtin__write", "builtin__read"]),
        calls,
        &flags,
    );
    let run_id = completed_run_id(&outcome, "AC-21 b write");
    let db = &outcome.db;

    assert_eq!(
        tool_row(db, &run_id, "w1").error.expect("w1")["error"]["reason"],
        json!("file_exists")
    );
    // w1 失败后立即 read（r1）仍是原内容 → 失败写不落盘、不截断原文件。
    assert_eq!(
        tool_row(db, &run_id, "r1").result.expect("r1")["content"],
        json!("orig")
    );
    assert_eq!(tool_row(db, &run_id, "w2").status, "ok");
    assert_eq!(read_text(&ws.join("a.txt")), "new", "overwrite=true 覆盖");
    assert_eq!(
        tool_row(db, &run_id, "w3").error.expect("w3")["error"]["reason"],
        json!("not_found")
    );
    assert_eq!(
        tool_row(db, &run_id, "r2").error.expect("r2")["error"]["reason"],
        json!("not_found"),
        "create_parents=false 不得建目录（立即 read 佐证）"
    );
    assert_eq!(tool_row(db, &run_id, "w4").status, "ok");
    assert_eq!(read_text(&ws.join("nested/dir/f.txt")), "x");
    let w5 = tool_row(db, &run_id, "w5");
    assert_eq!(
        w5.error.expect("w5")["error"]["reason"],
        json!("path_read_only"),
        "写 allow-path → path_read_only"
    );
    assert_eq!(
        read_text(&allow.join("ok.txt")),
        "keep\n",
        "allow-path 不得被写"
    );
    // 副作用审计：成功 write 记录 before/after sha256 与 bytes（AC-24 e 前置）。
    let w2_row = tool_row(db, &run_id, "w2");
    assert_eq!(w2_row.side_effect_path.as_deref(), Some("a.txt"));
    assert_eq!(w2_row.side_effect_bytes_before, Some(5));
    assert_eq!(w2_row.side_effect_bytes_after, Some(3));
    assert!(w2_row
        .side_effect_before
        .as_deref()
        .is_some_and(|sha| sha.len() == 64));
    assert!(w2_row
        .side_effect_after
        .as_deref()
        .is_some_and(|sha| sha.len() == 64));
    // 无 opt-in 之外的机器副作用：只多了 a.txt 内容与 nested/（文件快照用于 AC-24 g）。
    assert_ne!(before.get("a.txt"), tree_snapshot(&ws).get("a.txt"));
}

// ---------------------------------------------------------------------------
// AC-21 c：edit 唯一/0/多匹配/replace_all/allow-path 只读
// ---------------------------------------------------------------------------

#[test]
fn ac21c_edit_unique_and_replace_all() {
    let dir = tempfile::tempdir().expect("tempdir");
    let ws = workspace(dir.path());
    let allow = allow_dir(dir.path());
    write_text(&ws.join("e.txt"), "alpha beta alpha\n");
    write_text(&allow.join("ok.txt"), "keep\n");
    let abs_allow = allow.join("ok.txt").canonicalize().expect("canonical");

    let calls = calls_with_text(vec![
        tool_call(
            "e1",
            "builtin__edit",
            json!({"path": "e.txt", "old_string": "beta", "new_string": "BETA"}),
        ),
        tool_call("r1", "builtin__read", json!({"path": "e.txt"})),
        tool_call(
            "e2",
            "builtin__edit",
            json!({"path": "e.txt", "old_string": "zzz", "new_string": "y"}),
        ),
        tool_call(
            "e3",
            "builtin__edit",
            json!({"path": "e.txt", "old_string": "alpha", "new_string": "ALPHA"}),
        ),
        tool_call(
            "e4",
            "builtin__edit",
            json!({"path": "e.txt", "old_string": "alpha", "new_string": "ALPHA", "replace_all": true}),
        ),
        tool_call("r2", "builtin__read", json!({"path": "e.txt"})),
        tool_call(
            "e5",
            "builtin__edit",
            json!({"path": abs_allow.to_string_lossy(), "old_string": "keep", "new_string": "hack"}),
        ),
    ]);
    let flags = builtin_flags(&ws, std::slice::from_ref(&allow));
    let outcome = run_scenario(
        dir.path(),
        &agent_with_tools(&["builtin__edit", "builtin__read"]),
        calls,
        &flags,
    );
    let run_id = completed_run_id(&outcome, "AC-21 c edit");
    let db = &outcome.db;

    let e1 = tool_row(db, &run_id, "e1");
    assert_eq!(e1.status, "ok");
    assert_eq!(e1.result.expect("e1")["replacements"], json!(1));
    assert_eq!(
        tool_row(db, &run_id, "r1").result.expect("r1")["content"],
        json!("alpha BETA alpha")
    );
    assert_eq!(
        tool_row(db, &run_id, "e2").error.expect("e2")["error"]["reason"],
        json!("no_match")
    );
    let e3 = tool_row(db, &run_id, "e3");
    assert_eq!(
        e3.error.expect("e3")["error"]["reason"],
        json!("multiple_matches")
    );
    assert_eq!(
        tool_row(db, &run_id, "r1").result.expect("r1")["content"],
        json!("alpha BETA alpha"),
        "多匹配失败不得改文件"
    );
    let e4 = tool_row(db, &run_id, "e4");
    assert_eq!(e4.result.expect("e4")["replacements"], json!(2));
    assert_eq!(
        tool_row(db, &run_id, "r2").result.expect("r2")["content"],
        json!("ALPHA BETA ALPHA")
    );
    assert_eq!(
        tool_row(db, &run_id, "e5").error.expect("e5")["error"]["reason"],
        json!("path_read_only")
    );
    assert_eq!(read_text(&allow.join("ok.txt")), "keep\n");
    // edit 副作用：before/after sha 与文件字节数。
    let e1_row = tool_row(db, &run_id, "e1");
    assert_eq!(e1_row.side_effect_path.as_deref(), Some("e.txt"));
    assert_eq!(e1_row.side_effect_bytes_before, Some(17));
    assert_eq!(e1_row.side_effect_bytes_after, Some(17));
}

// ---------------------------------------------------------------------------
// AC-21 d：apply_patch（真实 git diff）单/多文件严格原子 + unsupported
// ---------------------------------------------------------------------------

#[test]
fn ac21d_apply_patch_git_diff_single_multi_and_atomic() {
    let dir = tempfile::tempdir().expect("tempdir");
    let ws = workspace(dir.path());
    write_text(&ws.join("a.txt"), "one\ntwo\n");
    write_text(&ws.join("b.txt"), "b1\n");

    // 真实 git diff：old(当前) → new(目标)，strip=2 后落到 ws 相对路径。
    let patch_dir = dir.path().join("patch");
    let single = {
        let old = patch_dir.join("single-old");
        let new = patch_dir.join("single-new");
        write_text(&old.join("a.txt"), "one\ntwo\n");
        write_text(&new.join("a.txt"), "one\nTWO\n");
        git_diff_patch(&patch_dir, Path::new("single-old"), Path::new("single-new"))
    };
    let multi = {
        let old = patch_dir.join("multi-old");
        let new = patch_dir.join("multi-new");
        write_text(&old.join("a.txt"), "one\nTWO\n");
        write_text(&old.join("b.txt"), "b1\n");
        write_text(&new.join("a.txt"), "one\nTWO\nthree\n");
        write_text(&new.join("b.txt"), "b1\nb2\n");
        git_diff_patch(&patch_dir, Path::new("multi-old"), Path::new("multi-new"))
    };
    // 原子负例：a 的 hunk 可应用、b 的上下文与真实文件不符 → 两个文件都不得改。
    let broken = {
        let old = patch_dir.join("broken-old");
        let new = patch_dir.join("broken-new");
        write_text(&old.join("a.txt"), "one\nTWO\nthree\n");
        write_text(&old.join("b.txt"), "stale-b1\nstale-b2\n");
        write_text(&new.join("a.txt"), "one\nTWO\nthree\nfour\n");
        write_text(&new.join("b.txt"), "WRONG\n");
        git_diff_patch(&patch_dir, Path::new("broken-old"), Path::new("broken-new"))
    };

    let calls = calls_with_text(vec![
        tool_call(
            "p1",
            "builtin__apply_patch",
            json!({"patch": single, "strip": 2}),
        ),
        tool_call(
            "p2",
            "builtin__apply_patch",
            json!({"patch": multi, "strip": 2}),
        ),
        tool_call(
            "p3",
            "builtin__apply_patch",
            json!({"patch": broken, "strip": 2}),
        ),
        tool_call(
            "p4",
            "builtin__apply_patch",
            json!({"patch": "garbage not a patch\n"}),
        ),
        tool_call("r1", "builtin__read", json!({"path": "a.txt"})),
        tool_call("r2", "builtin__read", json!({"path": "b.txt"})),
    ]);
    let flags = builtin_flags(&ws, &[]);
    let outcome = run_scenario(
        dir.path(),
        &agent_with_tools(&["builtin__apply_patch", "builtin__read"]),
        calls,
        &flags,
    );
    let run_id = completed_run_id(&outcome, "AC-21 d apply_patch");
    let db = &outcome.db;

    let p1 = tool_row(db, &run_id, "p1");
    assert_eq!(p1.status, "ok", "p1: {:?}", p1.error);
    assert_eq!(p1.result.expect("p1")["files"][0]["path"], json!("a.txt"));
    let p2 = tool_row(db, &run_id, "p2");
    assert_eq!(p2.status, "ok", "p2: {:?}", p2.error);
    assert_eq!(
        p2.result.clone().unwrap()["total_added"],
        json!(2),
        "多文件 added 合计"
    );
    assert_eq!(
        p2.result.clone().unwrap()["total_removed"],
        json!(0),
        "多文件 removed 合计"
    );
    let p3 = tool_row(db, &run_id, "p3");
    assert_eq!(
        p3.error.expect("p3")["error"]["reason"],
        json!("patch_mismatch")
    );
    // 终态 = p2 后的状态：p3 失败必须零改动（a 不追加 four、b 保持 b1/b2）。
    assert_eq!(
        read_text(&ws.join("a.txt")),
        "one\nTWO\nthree\n",
        "原子：a 不得部分应用"
    );
    assert_eq!(read_text(&ws.join("b.txt")), "b1\nb2\n", "原子：b 不得改");
    let p4 = tool_row(db, &run_id, "p4");
    assert_eq!(
        p4.error.expect("p4")["error"]["reason"],
        json!("unsupported_patch")
    );
    // 多文件 apply_patch 是**一行** tool_calls（结果含 files[] 摘要，不拆多行）。
    let patch_rows = tool_rows(db, &run_id)
        .into_iter()
        .filter(|row| row.tool_name == "builtin__apply_patch")
        .count();
    assert_eq!(patch_rows, 4, "4 次 apply_patch 各 1 行 trace");
}

// ---------------------------------------------------------------------------
// AC-21 e：diff 预览（不写文件/空 diff/越界）
// ---------------------------------------------------------------------------

#[test]
fn ac21e_diff_preview_does_not_write() {
    let dir = tempfile::tempdir().expect("tempdir");
    let ws = workspace(dir.path());
    write_text(&ws.join("d.txt"), "L1\nL2\nL3\nL4\n");
    let before = std::fs::read(ws.join("d.txt")).expect("read d");

    let calls = calls_with_text(vec![
        tool_call(
            "d1",
            "builtin__diff",
            json!({"path": "d.txt", "old_string": "L2", "new_string": "L2!"}),
        ),
        tool_call(
            "d2",
            "builtin__diff",
            json!({"path": "d.txt", "old_string": "L1", "new_string": "L1"}),
        ),
        tool_call(
            "d3",
            "builtin__diff",
            json!({"path": "../outside.txt", "old_string": "a", "new_string": "b"}),
        ),
    ]);
    let flags = builtin_flags(&ws, &[]);
    let outcome = run_scenario(
        dir.path(),
        &agent_with_tools(&["builtin__diff"]),
        calls,
        &flags,
    );
    let run_id = completed_run_id(&outcome, "AC-21 e diff");
    let db = &outcome.db;

    let d1 = tool_row(db, &run_id, "d1");
    assert_eq!(d1.result.clone().unwrap()["changed"], json!(true));
    let diff = d1.result.unwrap()["diff"]
        .as_str()
        .expect("diff text")
        .to_string();
    assert!(diff.contains("--- a/d.txt"), "{diff}");
    assert!(diff.contains("+++ b/d.txt"), "{diff}");
    assert!(diff.contains("-L2"), "{diff}");
    assert!(diff.contains("+L2!"), "{diff}");
    assert_eq!(
        tool_row(db, &run_id, "d2").result.expect("d2")["changed"],
        json!(false)
    );
    assert_eq!(
        tool_row(db, &run_id, "d2").result.expect("d2")["diff"],
        json!("")
    );
    assert_eq!(
        tool_row(db, &run_id, "d3").error.expect("d3")["error"]["reason"],
        json!("path_escape")
    );
    assert_eq!(
        std::fs::read(ws.join("d.txt")).expect("d after"),
        before,
        "diff 不写文件"
    );
}

// ---------------------------------------------------------------------------
// AC-21 f：jail 负例 + allow-path 只读（写 → path_read_only）
// ---------------------------------------------------------------------------

#[test]
fn ac21f_jail_negative_escapes() {
    let dir = tempfile::tempdir().expect("tempdir");
    let ws = workspace(dir.path());
    let allow = allow_dir(dir.path());
    let outside = dir.path().join("outside");
    let sibling = dir.path().join("ws-sibling");
    std::fs::create_dir_all(&outside).expect("outside");
    std::fs::create_dir_all(&sibling).expect("sibling");
    write_text(&outside.join("secret.txt"), "outside secret\n");
    write_text(&sibling.join("secret.txt"), "sibling secret\n");
    write_text(&ws.join("in.txt"), "inside\n");
    std::os::unix::fs::symlink(&outside, ws.join("link-out")).expect("symlink out");
    std::os::unix::fs::symlink("in.txt", ws.join("link-in")).expect("symlink in");
    // 兄弟目录前缀（字符串前缀匹配陷阱：ws 与 ws-sibling 共享前缀）。
    let sibling_prefix = "../ws-sibling/secret.txt";

    let calls = calls_with_text(vec![
        tool_call(
            "j1",
            "builtin__read",
            json!({"path": "../outside/secret.txt"}),
        ),
        tool_call("j2", "builtin__read", json!({"path": "/etc/hosts"})),
        tool_call(
            "j3",
            "builtin__read",
            json!({"path": "link-out/secret.txt"}),
        ),
        tool_call("j4", "builtin__read", json!({"path": sibling_prefix})),
        tool_call(
            "j5",
            "builtin__read",
            json!({"path": "link-sib/secret.txt"}),
        ),
        tool_call("j6", "builtin__read", json!({"path": "link-in"})),
        tool_call(
            "j7",
            "builtin__write",
            json!({"path": "../outside/w.txt", "content": "x"}),
        ),
        tool_call(
            "j8",
            "builtin__edit",
            json!({"path": "link-out/secret.txt", "old_string": "outside", "new_string": "x"}),
        ),
    ]);
    std::os::unix::fs::symlink(&sibling, ws.join("link-sib")).expect("symlink sibling");
    let flags = builtin_flags(&ws, &[allow]);
    let outcome = run_scenario(
        dir.path(),
        &agent_with_tools(&["builtin__read", "builtin__write", "builtin__edit"]),
        calls,
        &flags,
    );
    let run_id = completed_run_id(&outcome, "AC-21 f jail");
    let db = &outcome.db;
    for (id, expected) in [
        ("j1", "path_escape"),
        ("j2", "path_escape"),
        ("j3", "path_escape"),
        ("j4", "path_escape"),
        ("j5", "path_escape"),
        ("j7", "path_escape"),
        ("j8", "path_escape"),
    ] {
        let row = tool_row(db, &run_id, id);
        assert_eq!(
            row.error.expect("jail error")["error"]["reason"],
            json!(expected),
            "{id} 应 fail-closed path_escape"
        );
        assert_eq!(row.status, "error");
    }
    assert_eq!(
        tool_row(db, &run_id, "j6").result.expect("j6")["content"],
        json!("inside"),
        "jail 内 symlink 允许"
    );
    assert!(!outside.join("w.txt").exists(), "越界写不得落盘");
    assert_eq!(read_text(&outside.join("secret.txt")), "outside secret\n");
}

#[test]
fn ac21f_allow_path_is_read_only() {
    let dir = tempfile::tempdir().expect("tempdir");
    let ws = workspace(dir.path());
    let allow = allow_dir(dir.path());
    write_text(&allow.join("ok.txt"), "allowed read\n");
    let abs_allow = allow.join("ok.txt").canonicalize().expect("canonical");

    let calls = calls_with_text(vec![
        tool_call(
            "a1",
            "builtin__read",
            json!({"path": abs_allow.to_string_lossy()}),
        ),
        tool_call(
            "a2",
            "builtin__write",
            json!({"path": abs_allow.to_string_lossy(), "content": "x", "overwrite": true}),
        ),
        tool_call(
            "a3",
            "builtin__edit",
            json!({"path": abs_allow.to_string_lossy(), "old_string": "allowed", "new_string": "x"}),
        ),
        tool_call(
            "a4",
            "builtin__apply_patch",
            json!({
                "patch": format!(
                    "--- {abs}\n+++ {abs}\n@@ -1 +1 @@\n-allowed read\n+hacked\n",
                    abs = abs_allow.to_string_lossy()
                ),
                "strip": 0
            }),
        ),
    ]);
    let flags = builtin_flags(&ws, std::slice::from_ref(&allow));
    let outcome = run_scenario(
        dir.path(),
        &agent_with_tools(&[
            "builtin__read",
            "builtin__write",
            "builtin__edit",
            "builtin__apply_patch",
        ]),
        calls,
        &flags,
    );
    let run_id = completed_run_id(&outcome, "AC-21 f allow-path");
    let db = &outcome.db;
    assert_eq!(
        tool_row(db, &run_id, "a1").result.expect("a1")["content"],
        json!("allowed read")
    );
    for id in ["a2", "a3", "a4"] {
        assert_eq!(
            tool_row(db, &run_id, id).error.expect("allow write error")["error"]["reason"],
            json!("path_read_only"),
            "{id} 写 allow-path 必须 path_read_only"
        );
    }
    assert_eq!(read_text(&allow.join("ok.txt")), "allowed read\n");
}

// ---------------------------------------------------------------------------
// AC-21 g / 预检：无 opt-in 默认不可见零副作用；配置非法 exit 3 不建 DB
// ---------------------------------------------------------------------------

#[test]
fn ac21g_no_optin_is_invisible_and_side_effect_free() {
    let dir = tempfile::tempdir().expect("tempdir");
    let ws = workspace(dir.path());
    write_text(&ws.join("a.txt"), "orig\n");
    let snapshot = tree_snapshot(&ws);
    let outcome = run_scenario(
        dir.path(),
        &agent_with_tools(&[]),
        support::text_reply("no tools"),
        &[],
    );
    let payload = json_stdout(&outcome.output);
    assert_eq!(payload["status"], json!("Completed"));
    let run_id = payload["run_id"].as_str().expect("run_id").to_string();
    assert_eq!(
        support::tool_call_count(&outcome.db, &run_id),
        0,
        "无 opt-in 不落 tool_calls"
    );
    assert!(support::trace_tool_call_lines(&outcome.db, &run_id).is_empty());
    assert_eq!(
        tree_snapshot(&ws),
        snapshot,
        "无 opt-in 零 workspace 副作用"
    );
    // 未 opt-in 但 AgentConfig.tools 引用 builtin__ → 加载期 fail-closed。
    let blocked_dir = dir.path().join("blocked");
    std::fs::create_dir_all(&blocked_dir).expect("blocked dir");
    let outcome = run_scenario(
        &blocked_dir,
        &agent_with_tools(&["builtin__read"]),
        support::text_reply("x"),
        &[],
    );
    assert_exit(&outcome.output, 1, "builtin__read 无 opt-in");
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
}

#[test]
fn ac21g_invalid_workspace_and_allow_path_preflight_exit3() {
    let dir = tempfile::tempdir().expect("tempdir");
    let ws = workspace(dir.path());
    // workspace 不存在 → exit 3、不建 DB、reason workspace_invalid。
    let db = dir.path().join("ws-invalid.db");
    let flags = vec![
        "--allow-builtin-tools".to_string(),
        "--workspace-dir".to_string(),
        cli_support::arg(&dir.path().join("no-such-ws")),
    ];
    let outcome = run_scenario(
        dir.path(),
        &agent_with_tools(&["builtin__read"]),
        support::text_reply("x"),
        &flags,
    );
    assert_exit(&outcome.output, 3, "workspace 非法 → exit 3");
    assert!(!db.exists(), "预检失败不得建 DB");
    let stderr = v03::err(&outcome.output);
    assert!(
        stderr.contains("HARNESS_CONFIG_LOAD") && stderr.contains("workspace_invalid"),
        "{stderr}"
    );
    // allow-path 不存在 → exit 3、不建 DB、reason allow_path_invalid。
    let db = dir.path().join("allow-invalid.db");
    let mut flags = builtin_flags(&ws, &[]);
    flags.push("--allow-path".to_string());
    flags.push(cli_support::arg(&dir.path().join("no-such-allow")));
    let outcome = run_scenario(
        dir.path(),
        &agent_with_tools(&["builtin__read"]),
        support::text_reply("x"),
        &flags,
    );
    assert_exit(&outcome.output, 3, "allow-path 非法 → exit 3");
    assert!(!db.exists(), "预检失败不得建 DB");
    let stderr = v03::err(&outcome.output);
    assert!(
        stderr.contains("HARNESS_CONFIG_LOAD") && stderr.contains("allow_path_invalid"),
        "{stderr}"
    );
}
