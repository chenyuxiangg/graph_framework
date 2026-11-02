//! D5c（v0.4 / M9）AC-24：trace v7 `tool_calls` + 工具级回放 **真 CLI 链路**。
//!
//! 语义依据：DR-014、spec 07 AC-24、06 §6.15、DR-013 §8。
//! 数据通路：真 `zgraph` + 真 fake LLM + 真 Rust 内置工具 + 真 SQLite v7。
//!
//! 覆盖：a v6→v7 迁移/备份/旧数据保留；b 只读 v6 库拒绝迁移；c 无工具调用时 trace/
//! replay additive 且与 v6 语义一致；d 每次调用恰 1 行/字段/call_id 原生；e 副作用
//! sha/bytes；i trace 人类/`--json`/`--stats` 与 32 KiB 上限；g recorded 回放无副作用 +
//! 缺记录/截断/不符 fail-closed；h `reexec` v0.4 用法错误；events/outputs 不变。

#![cfg(unix)]

#[path = "cli_support/mod.rs"]
mod cli_support;
#[path = "v04_support/mod.rs"]
mod support;
#[path = "v03_support/mod.rs"]
mod v03;

use std::path::{Path, PathBuf};

use serde_json::json;

use cli_support::{assert_exit, json_stdout};
use support::{
    agent_with_tools, assert_exit_ok, builtin_flags, calls_with_text, completed_run_id,
    downgrade_v7_to_v6, replay_events_json, replay_mode, replay_outputs_json,
    replay_tool_call_lines, replay_tools_json, run_scenario, tool_call, tool_row, tool_rows,
    trace_extra, trace_json, trace_stats, trace_stats_json, trace_tool_call_lines, tree_snapshot,
    user_version, v6_backups, workspace, write_text,
};

// ---------------------------------------------------------------------------
// 辅助
// ---------------------------------------------------------------------------

/// 按字节比较 stdout 并给出**紧凑**差异（避免 assert_eq 打印整段 NDJSON）。
fn assert_stdout_equal(left: &[u8], right: &[u8], context: &str) {
    if left == right {
        return;
    }
    let common = left
        .iter()
        .zip(right.iter())
        .take_while(|(a, b)| a == b)
        .count();
    let start = common.saturating_sub(80);
    let end = (common + 160).min(left.len().max(right.len()));
    panic!(
        "{context}: stdout differs at byte {common}\n left[{}..{}]={:?}\nright[{}..{}]={:?}",
        start,
        end.min(left.len()),
        String::from_utf8_lossy(&left[start..end.min(left.len())]),
        start,
        end.min(right.len()),
        String::from_utf8_lossy(&right[start..end.min(right.len())]),
    );
}

/// 用 SQLite `VACUUM INTO` 复制出独立 DB（供构造损坏记录；避免 -wal 丢失）。
fn copy_db(src: &Path, dst: &Path) {
    let conn = support::writable_connect(src);
    conn.execute("VACUUM INTO ?1", [dst.to_string_lossy()])
        .expect("vacuum into copy");
}

/// 用 `` 运行一段 SQL（构造损坏 v7 记录）。
fn exec_sql(db: &Path, sql: &str) {
    support::writable_connect(db)
        .execute_batch(sql)
        .expect("mutate tool_calls db");
}

/// 一个含 write/read/edit/diff/grep/glob 的工具 run（AC-24 通用夹具）。
fn tool_run(dir: &Path) -> (support::RunOutcome, String, PathBuf) {
    let ws = workspace(dir);
    write_text(&ws.join("seed.txt"), "alpha\n");
    let calls = calls_with_text(vec![
        tool_call(
            "c1",
            "builtin__write",
            json!({"path": "note.txt", "content": "hello v04"}),
        ),
        tool_call("c2", "builtin__read", json!({"path": "note.txt"})),
        tool_call(
            "c3",
            "builtin__edit",
            json!({"path": "note.txt", "old_string": "hello", "new_string": "HELLO"}),
        ),
        tool_call(
            "c4",
            "builtin__diff",
            json!({"path": "note.txt", "old_string": "HELLO v04", "new_string": "HELLO v04!"}),
        ),
        tool_call("c5", "builtin__grep", json!({"pattern": "HELLO"})),
        tool_call("c6", "builtin__glob", json!({"pattern": "*.txt"})),
    ]);
    let flags = builtin_flags(&ws, &[]);
    let outcome = run_scenario(
        dir,
        &agent_with_tools(&[
            "builtin__write",
            "builtin__read",
            "builtin__edit",
            "builtin__diff",
            "builtin__grep",
            "builtin__glob",
        ]),
        calls,
        &flags,
    );
    let run_id = completed_run_id(&outcome, "AC-24 tool run");
    (outcome, run_id, ws)
}

// ---------------------------------------------------------------------------
// AC-24 a/b：v6→v7 迁移 + 备份 + 只读拒绝
// ---------------------------------------------------------------------------

#[test]
fn ac24a_v6_to_v7_migration_with_backup_and_old_data() {
    let dir = tempfile::tempdir().expect("tempdir");
    // 1) 先产生一个真实 run（v7），再降级为真实 v6 形状。
    let first = run_scenario(
        dir.path(),
        &agent_with_tools(&[]),
        support::text_reply("first"),
        &[],
    );
    let first_run = completed_run_id(&first, "migration seed run");
    let db = first.db.clone();
    assert_eq!(user_version(&db), 7);
    downgrade_v7_to_v6(&db);
    assert_eq!(user_version(&db), 6, "降级为 v6");
    assert!(
        !support::table_exists(&db, "tool_calls"),
        "v6 无 tool_calls"
    );

    // 2) 新二进制打开 v6 库：单事务迁移 v7 + 备份 + 旧数据保留。
    let second = run_scenario(
        dir.path(),
        &agent_with_tools(&["builtin__write"]),
        calls_with_text(vec![tool_call(
            "m1",
            "builtin__write",
            json!({"path": "migrated.txt", "content": "after migration"}),
        )]),
        &builtin_flags(&workspace(dir.path()), &[]),
    );
    let second_run = completed_run_id(&second, "migration second run");
    assert_eq!(user_version(&db), 7, "迁移后 v7");
    assert!(
        support::table_exists(&db, "tool_calls"),
        "迁移新增 tool_calls"
    );
    // 备份恰一份 v6 快照（无 tool_calls、旧 run 保留）。
    let backups = v6_backups(dir.path());
    assert_eq!(backups.len(), 1, "恰一份 .bak.v6- 备份: {backups:?}");
    let backup = &backups[0];
    assert_eq!(user_version(backup), 6, "备份保持 v6");
    assert!(
        !support::table_exists(backup, "tool_calls"),
        "备份无 tool_calls"
    );
    let backup_conn = cli_support::read_only_connect(backup);
    let backup_runs: i64 = backup_conn
        .query_row("SELECT COUNT(*) FROM runs", [], |row| row.get(0))
        .expect("backup runs");
    assert_eq!(backup_runs, 1, "备份含迁移前 1 个 run");
    assert!(
        backup_conn
            .query_row(
                "SELECT COUNT(*) FROM events WHERE run_id = ?1",
                [&first_run],
                |row| row.get::<_, i64>(0)
            )
            .expect("backup events")
            > 0
    );
    drop(backup_conn);
    // 旧 run 在新库中仍可 trace/replay；新 run 落 tool_calls 并记录副作用。
    let trace = trace_json(&db, &first_run);
    assert_exit_ok(&trace, "旧 run trace");
    let events = replay_events_json(&db, &first_run);
    assert_exit_ok(&events, "旧 run replay events");
    assert_eq!(events.stdout, trace.stdout);
    let m1 = tool_row(&db, &second_run, "m1");
    assert_eq!(m1.status, "ok");
    assert_eq!(m1.side_effect_path.as_deref(), Some("migrated.txt"));
    assert_eq!(support::tool_call_count(&db, &second_run), 1);
    // 新 run 的 run_id 与旧不同，且 runs 表两条。
    assert_ne!(first_run, second_run);
    let runs: i64 = cli_support::read_only_connect(&db)
        .query_row("SELECT COUNT(*) FROM runs", [], |row| row.get(0))
        .expect("runs count");
    assert_eq!(runs, 2);
}

#[test]
fn ac24b_readonly_v6_database_is_rejected() {
    let dir = tempfile::tempdir().expect("tempdir");
    let first = run_scenario(
        dir.path(),
        &agent_with_tools(&[]),
        support::text_reply("seed"),
        &[],
    );
    let _ = first;
    let db = dir.path().join("trace.db");
    downgrade_v7_to_v6(&db);
    use std::os::unix::fs::PermissionsExt as _;
    let mut perms = std::fs::metadata(&db).expect("metadata").permissions();
    let readonly = std::fs::Permissions::from_mode(0o444);
    std::fs::set_permissions(&db, readonly).expect("chmod readonly");
    let outcome = run_scenario(
        dir.path(),
        &agent_with_tools(&[]),
        support::text_reply("blocked"),
        &[],
    );
    perms.set_mode(0o644);
    std::fs::set_permissions(&db, perms).expect("restore perms");
    assert_exit(&outcome.output, 4, "只读 v6 库 → STORAGE_ERROR/exit 4");
    let stderr = v03::err(&outcome.output);
    assert!(stderr.contains("STORAGE_ERROR"), "{stderr}");
    assert_eq!(user_version(&db), 6, "只读库不得迁移");
    assert!(!support::table_exists(&db, "tool_calls"));
}

// ---------------------------------------------------------------------------
// AC-24 c：无工具调用时 v7 additive 与 v6 语义一致
// ---------------------------------------------------------------------------

#[test]
fn ac24c_no_tool_run_trace_and_replay_unchanged() {
    let dir = tempfile::tempdir().expect("tempdir");
    let outcome = run_scenario(
        dir.path(),
        &agent_with_tools(&[]),
        support::text_reply("no-tool"),
        &[],
    );
    let run_id = completed_run_id(&outcome, "无工具 run");
    let db = &outcome.db;
    assert_eq!(user_version(db), 7, "schema 已 v7");
    assert_eq!(support::tool_call_count(db, &run_id), 0);
    // human trace 不出现工具段；--json 不出现 ToolCall 行；--stats 不出现计数。
    let human = trace_extra(db, &run_id, &[]);
    assert_exit_ok(&human, "trace human");
    assert!(!String::from_utf8_lossy(&human.stdout).contains("tool_calls:"));
    assert!(trace_tool_call_lines(db, &run_id).is_empty());
    let stats = trace_stats(db, &run_id);
    assert!(!String::from_utf8_lossy(&stats.stdout).contains("tool_calls"));
    let stats_json = trace_stats_json(db, &run_id);
    let stats_value = json_stdout(&stats_json);
    assert!(
        stats_value.get("tool_calls").is_none(),
        "无工具调用不追加 tool_calls 计数: {stats_value}"
    );
    // events 回放与 trace 逐字节一致；L2 outputs matched。
    let trace = trace_json(db, &run_id);
    let replay = replay_events_json(db, &run_id);
    assert_eq!(replay.stdout, trace.stdout);
    let outputs = replay_outputs_json(db, &run_id);
    assert_exit_ok(&outputs, "replay outputs");
    assert_eq!(json_stdout(&outputs)["matched"], json!(true));
}

// ---------------------------------------------------------------------------
// AC-24 d：每次工具调用恰 1 行 + 字段 + call_id 原生
// ---------------------------------------------------------------------------

#[test]
fn ac24d_one_row_per_call_with_required_fields() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (outcome, run_id, _ws) = tool_run(dir.path());
    let db = &outcome.db;
    let rows = tool_rows(db, &run_id);
    assert_eq!(rows.len(), 6, "6 次调用恰 6 行 trace");
    let mut call_ids: Vec<&str> = rows.iter().map(|row| row.call_id.as_str()).collect();
    call_ids.sort_unstable();
    assert_eq!(
        call_ids,
        vec!["c1", "c2", "c3", "c4", "c5", "c6"],
        "原生 call_id"
    );
    assert_eq!(support::tool_call_count(db, &run_id), 6);
    // 全部行同一 agent 实例（单节点）。
    let instance_ids: std::collections::BTreeSet<&str> =
        rows.iter().map(|row| row.instance_id.as_str()).collect();
    assert_eq!(instance_ids.len(), 1, "同一实例");
    for row in &rows {
        assert_eq!(row.node_id, "agent");
        assert_eq!(row.attempt, 0);
        assert_eq!(row.source, "builtin");
        assert_eq!(row.status, "ok", "{}: {:?}", row.call_id, row.error);
        assert!(row.duration_ms >= 0);
        assert!(!row.truncated);
        assert_eq!(
            row.args_sha256.as_deref().map(str::len),
            Some(64),
            "args_sha256"
        );
        assert_eq!(
            row.result_sha256.as_deref().map(str::len),
            Some(64),
            "result_sha256"
        );
        assert!(row.result.is_some(), "{} 成功结果", row.call_id);
        assert!(row.error.is_none());
    }
    // args 与 LLM 原生入参一致（不伪造/不改写）。
    let ctx = &rows[0];
    assert_eq!(ctx.args["path"], json!("note.txt"));
    assert_eq!(ctx.args["content"], json!("hello v04"));
}

// ---------------------------------------------------------------------------
// AC-24 e：副作用记录（write/edit/apply_patch → path + before/after sha + bytes）
// ---------------------------------------------------------------------------

#[test]
fn ac24e_side_effect_metadata_is_recorded() {
    let dir = tempfile::tempdir().expect("tempdir");
    let ws = workspace(dir.path());
    write_text(&ws.join("a.txt"), "one\ntwo\n");
    write_text(&ws.join("b.txt"), "b1\n");
    let patch = "--- a/a.txt\n+++ b/a.txt\n@@ -1,2 +1,2 @@\n one\n-two\n+TWO\n";
    let calls = calls_with_text(vec![
        tool_call(
            "s1",
            "builtin__write",
            json!({"path": "new.txt", "content": "brand new"}),
        ),
        tool_call("r1", "builtin__read", json!({"path": "new.txt"})),
        tool_call(
            "s2",
            "builtin__write",
            json!({"path": "new.txt", "content": "brand new!", "overwrite": true}),
        ),
        tool_call(
            "s3",
            "builtin__edit",
            json!({"path": "new.txt", "old_string": "brand", "new_string": "BRAND"}),
        ),
        tool_call(
            "s4",
            "builtin__apply_patch",
            json!({"patch": patch, "strip": 1}),
        ),
        // 多文件 apply_patch：仍**一行** trace；side_effect_* 为 NULL（files[] 在 result）。
        tool_call(
            "s5",
            "builtin__apply_patch",
            json!({
                "patch": "--- a/a.txt\n+++ b/a.txt\n@@ -1,2 +1,2 @@\n one\n-TWO\n+TWO!\n--- a/b.txt\n+++ b/b.txt\n@@ -1 +1 @@\n-b1\n+B1\n",
                "strip": 1
            }),
        ),
    ]);
    let flags = builtin_flags(&ws, &[]);
    let outcome = run_scenario(
        dir.path(),
        &agent_with_tools(&[
            "builtin__write",
            "builtin__read",
            "builtin__edit",
            "builtin__apply_patch",
        ]),
        calls,
        &flags,
    );
    let run_id = completed_run_id(&outcome, "AC-24 e side effect");
    let db = &outcome.db;
    // write 新建：before 空、after sha 与 read 一致。
    let s1 = tool_row(db, &run_id, "s1");
    assert_eq!(s1.side_effect_path.as_deref(), Some("new.txt"));
    assert!(s1.side_effect_before.is_none());
    assert!(s1
        .side_effect_after
        .as_deref()
        .is_some_and(|sha| sha.len() == 64));
    assert_eq!(s1.side_effect_bytes_before, None);
    assert_eq!(s1.side_effect_bytes_after, Some(9));
    let read_sha = tool_row(db, &run_id, "r1").result.expect("r1")["sha256"].clone();
    assert_eq!(s1.side_effect_after.as_deref(), read_sha.as_str());
    // write 覆盖：before = 旧 sha、after = 新 sha、bytes 正确。
    let s2 = tool_row(db, &run_id, "s2");
    assert_eq!(s2.side_effect_before.as_deref(), read_sha.as_str());
    assert_ne!(s2.side_effect_after.as_deref(), read_sha.as_str());
    assert_eq!(s2.side_effect_bytes_before, Some(9));
    assert_eq!(s2.side_effect_bytes_after, Some(10));
    // edit：before/after sha + bytes（new.txt "BRAND new!" 10 bytes）。
    let s3 = tool_row(db, &run_id, "s3");
    assert_eq!(s3.side_effect_path.as_deref(), Some("new.txt"));
    assert_eq!(s3.side_effect_bytes_before, Some(10));
    assert_eq!(s3.side_effect_bytes_after, Some(10));
    assert!(s3.side_effect_before.is_some() && s3.side_effect_after.is_some());
    // apply_patch 单文件：before/after sha + bytes；多文件时 side_effect 仅 result.files[]。
    let s4 = tool_row(db, &run_id, "s4");
    assert_eq!(s4.side_effect_path.as_deref(), Some("a.txt"));
    assert_eq!(s4.side_effect_bytes_before, Some(8));
    assert_eq!(s4.side_effect_bytes_after, Some(8));
    assert_eq!(s4.result.expect("s4")["files"][0]["path"], json!("a.txt"));
    // REV-112 P2：多文件 apply_patch 的 trace 副作用列必须为 NULL（避免误导单文件
    // 回滚语义）；多文件结果仍在 result.files[] 中逐文件记录 sha。
    let s5 = tool_row(db, &run_id, "s5");
    assert_eq!(
        s5.side_effect_path, None,
        "多文件 side_effect_path 必须 NULL"
    );
    assert_eq!(
        s5.side_effect_before, None,
        "多文件 side_effect_before NULL"
    );
    assert_eq!(s5.side_effect_after, None, "多文件 side_effect_after NULL");
    assert_eq!(s5.side_effect_bytes_before, None);
    assert_eq!(s5.side_effect_bytes_after, None);
    let files = s5.result.expect("s5 result")["files"]
        .as_array()
        .expect("files[]")
        .clone();
    assert_eq!(files.len(), 2, "多文件 files[] 两条");
    assert_eq!(files[0]["path"], json!("a.txt"));
    assert_eq!(files[1]["path"], json!("b.txt"));
    for file in &files {
        assert!(file["sha256_before"]
            .as_str()
            .is_some_and(|sha| sha.len() == 64));
        assert!(file["sha256_after"]
            .as_str()
            .is_some_and(|sha| sha.len() == 64));
    }
    assert_eq!(
        std::fs::read_to_string(ws.join("a.txt")).expect("a after multi"),
        "one\nTWO!\n"
    );
    assert_eq!(
        std::fs::read_to_string(ws.join("b.txt")).expect("b after multi"),
        "B1\n"
    );
}

// ---------------------------------------------------------------------------
// AC-24 i：trace 人类/`--json`/`--stats` additive + 32 KiB 上限
// ---------------------------------------------------------------------------

#[test]
fn ac24i_trace_outputs_additive_and_caps() {
    let dir = tempfile::tempdir().expect("tempdir");
    let ws = workspace(dir.path());
    write_text(&ws.join("seed.txt"), "seed\n");
    let big_content = "C".repeat(40_000);
    let calls = calls_with_text(vec![
        tool_call(
            "big1",
            "builtin__write",
            json!({"path": "big.txt", "content": big_content}),
        ),
        tool_call(
            "big2",
            "builtin__read",
            json!({"path": "big.txt", "limit": 10000}),
        ),
    ]);
    let flags = builtin_flags(&ws, &[]);
    let outcome = run_scenario(
        dir.path(),
        &agent_with_tools(&["builtin__write", "builtin__read"]),
        calls,
        &flags,
    );
    let run_id = completed_run_id(&outcome, "AC-24 i trace caps");
    let db = &outcome.db;

    // 人类行：一行/调用，含 status/duration/truncated/args 摘要（不打印完整 args）。
    let human = trace_extra(db, &run_id, &[]);
    assert_exit_ok(&human, "trace human");
    let human_text = String::from_utf8_lossy(&human.stdout).into_owned();
    assert!(
        human_text.contains("tool_calls: 2"),
        "tool_calls 段: {}",
        human_text
            .lines()
            .filter(|line| line.contains("tool_calls") || line.contains("big1"))
            .collect::<Vec<_>>()
            .join("\n")
    );
    assert!(
        human_text.contains("big1 builtin__write source=builtin status=ok duration="),
        "big1 人类行: {human_text}"
    );
    assert!(
        human_text.contains("truncated=true args=truncated path=big.txt"),
        "被截断行只给标记（不展示 prefix/完整 args）: {human_text}"
    );
    assert!(
        human_text.contains("big2 builtin__read source=builtin status=ok")
            && human_text.contains("args=keys=[path,limit] bytes="),
        "非截断行给 keys 摘要: {human_text}"
    );
    assert!(
        !human_text.contains(&"C".repeat(64)),
        "人类输出不打印完整 args"
    );
    // --json：ToolCall 行 additive，字段与表一致。
    let lines = trace_tool_call_lines(db, &run_id);
    assert_eq!(lines.len(), 2, "ToolCall NDJSON");
    let big1 = lines
        .iter()
        .find(|line| line["call_id"] == json!("big1"))
        .expect("big1 line");
    assert_eq!(big1["kind"], json!("ToolCall"));
    for key in [
        "tool_call_id",
        "run_id",
        "instance_id",
        "node_id",
        "attempt",
        "source",
        "status",
        "args",
        "result",
        "error",
        "truncated",
        "args_sha256",
        "result_sha256",
        "duration_ms",
        "side_effect_path",
        "bash_command",
        "sandbox_profile_sha256",
    ] {
        assert!(big1.get(key).is_some(), "ToolCall 缺字段 {key}: {big1}");
    }
    assert_eq!(big1["truncated"], json!(true), "args 超 32 KiB");
    assert!(
        big1["args"].is_string(),
        "截断 args 保留 prefix 原文（非完整 JSON）: {}",
        big1["args"]
    );
    // trace 表：args_json cap 32768 + 全量 sha256；read result ≤32 KiB 且工具层 truncated。
    let row = tool_row(db, &run_id, "big1");
    assert!(row.truncated);
    assert_eq!(row.raw_args_json.len(), 32_768, "args cap");
    assert_eq!(row.args_sha256.as_deref().map(str::len), Some(64));
    let read_row = tool_row(db, &run_id, "big2");
    let result = read_row.result.expect("big2 result");
    assert_eq!(result["truncated"], json!(true), "read 结果工具层截断");
    let raw = read_row.raw_result_json.as_deref().expect("raw result");
    assert!(raw.len() <= 32_768, "result cap: {}", raw.len());
    // --stats additive 计数。
    let stats = trace_stats(db, &run_id);
    assert!(String::from_utf8_lossy(&stats.stdout).contains("tool_calls 2"));
    let stats_json = trace_stats_json(db, &run_id);
    assert_eq!(json_stdout(&stats_json)["tool_calls"], json!(2));
    // 既有 events 输出不变：L1 replay 与 trace --json 一致。
    let replay = replay_events_json(db, &run_id);
    let trace = trace_json(db, &run_id);
    assert_eq!(replay.stdout, trace.stdout);
}

// ---------------------------------------------------------------------------
// AC-24 g：recorded 回放还原 + 零副作用；缺记录/截断/不符 fail-closed
// ---------------------------------------------------------------------------

#[test]
fn ac24g_recorded_replay_restores_without_side_effects() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (outcome, run_id, ws) = tool_run(dir.path());
    let db = &outcome.db;
    let trace_lines = trace_tool_call_lines(db, &run_id);
    assert_eq!(trace_lines.len(), 6);
    // 预热一次（WAL/shm 稳定），再取快照。
    let warm = replay_tools_json(db, &run_id);
    assert_exit_ok(&warm, "replay tools warmup");
    let workspace_before = tree_snapshot(&ws);
    let db_before = support::db_snapshot(db);
    let rows_before = support::tool_call_count(db, &run_id);
    let runs_before: i64 = cli_support::read_only_connect(db)
        .query_row("SELECT COUNT(*) FROM runs", [], |row| row.get(0))
        .expect("runs before");
    // 无 Python / 无工具执行环境：recorded 回放仍成功。
    let args = {
        let mut args = vec![
            "replay".to_string(),
            "--run".to_string(),
            run_id.clone(),
            "--db".to_string(),
            cli_support::arg(db),
            "--mode".to_string(),
            "tools".to_string(),
            "--json".to_string(),
        ];
        args.extend(Vec::<String>::new());
        args
    };
    let output = cli_support::run_cli_env(
        &args,
        &cli_support::repo_root(),
        &[("ZGRAPH_PYTHON", "/nonexistent-python-for-replay")],
    );
    assert_exit_ok(&output, "recorded replay with ZGRAPH_PYTHON=/nonexistent");
    let lines = v03::ndjson(&output);
    assert_eq!(lines, trace_lines, "recorded 还原 == trace ToolCall 行");
    // 零副作用：workspace/DB/-wal/-shm 字节、tool_calls 行数、runs 数均不变。
    assert_eq!(tree_snapshot(&ws), workspace_before, "回放不得改 workspace");
    assert_eq!(
        support::db_snapshot(db),
        db_before,
        "回放不得改 DB/-wal/-shm"
    );
    assert_eq!(support::tool_call_count(db, &run_id), rows_before);
    let runs_after: i64 = cli_support::read_only_connect(db)
        .query_row("SELECT COUNT(*) FROM runs", [], |row| row.get(0))
        .expect("runs after");
    assert_eq!(runs_after, runs_before, "回放不写新 run");
}

#[test]
fn ac24g_recorded_replay_fail_closed_on_corruption() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (outcome, run_id, ws) = tool_run(dir.path());
    let source = outcome.db.clone();
    let snapshot = tree_snapshot(&ws);

    let cases: [(&str, String, &str); 4] = [
        (
            "truncated",
            "UPDATE tool_calls SET truncated = 1 WHERE rowid = (SELECT MIN(rowid) FROM tool_calls);"
                .to_string(),
            "REPLAY_TOOL_RECORD_TRUNCATED",
        ),
        (
            "missing",
            "DELETE FROM tool_calls WHERE rowid = (SELECT MAX(rowid) FROM tool_calls);".to_string(),
            "REPLAY_TOOL_RECORD_MISSING",
        ),
        (
            "invalid-json",
            "UPDATE tool_calls SET result_json = '{not json' WHERE result_json IS NOT NULL AND rowid = (SELECT MIN(rowid) FROM tool_calls WHERE result_json IS NOT NULL);".to_string(),
            "REPLAY_TOOL_RECORD_MISMATCH",
        ),
        (
            "sha-mismatch",
            "UPDATE tool_calls SET result_sha256 = '0000000000000000000000000000000000000000000000000000000000000000' WHERE result_json IS NOT NULL AND rowid = (SELECT MIN(rowid) FROM tool_calls WHERE result_json IS NOT NULL);".to_string(),
            "REPLAY_TOOL_RECORD_MISMATCH",
        ),
    ];
    for (name, sql, expected_code) in cases {
        let copy = dir.path().join(format!("{name}.db"));
        copy_db(&source, &copy);
        exec_sql(&copy, &sql);
        let output = replay_tools_json(&copy, &run_id);
        assert_exit(&output, 3, &format!("{name} fail-closed"));
        let stderr = v03::err(&output);
        assert!(
            stderr.contains(expected_code),
            "{name}: 期望 {expected_code}: {stderr}"
        );
        assert!(stderr.contains("tool_calls"), "{name}: 错误定位: {stderr}");
    }
    // 全部 fail-closed 分支都不得执行工具/改 workspace。
    assert_eq!(tree_snapshot(&ws), snapshot);
}

// ---------------------------------------------------------------------------
// AC-24 h：reexec（v0.4 未实现 → 用法错误）；events/outputs 不变
// ---------------------------------------------------------------------------

#[test]
fn ac24h_reexec_is_usage_error_and_events_outputs_unchanged() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (outcome, run_id, _ws) = tool_run(dir.path());
    let db = &outcome.db;
    // `--mode=tools --replay-tools=reexec` → exit 2（v0.4 recorded-only）。
    let output = replay_mode(db, &run_id, "tools", &["--replay-tools=reexec"]);
    assert_exit(&output, 2, "reexec 未实现");
    let stderr = v03::err(&output);
    assert!(stderr.contains("reexec"), "{stderr}");
    assert!(
        stderr.contains("not implemented") && stderr.contains("recorded"),
        "{stderr}"
    );
    // 非法 --replay-tools 值 → exit 2。
    let output = replay_mode(db, &run_id, "tools", &["--replay-tools=bogus"]);
    assert_exit(&output, 2, "非法 replay-tools");
    assert!(
        v03::err(&output).contains("--replay-tools"),
        "{}",
        v03::err(&output)
    );
    // events/outputs 路径不变。
    let trace = trace_json(db, &run_id);
    let events = replay_events_json(db, &run_id);
    assert_stdout_equal(&events.stdout, &trace.stdout, "replay events vs trace");
    let outputs = replay_outputs_json(db, &run_id);
    assert_exit_ok(&outputs, "outputs replay");
    assert_eq!(json_stdout(&outputs)["matched"], json!(true));
    // `--replay-tools=recorded` 在 events 模式被忽略且不报错（Lead REV-111 裁决）。
    let recorded = replay_mode(
        db,
        &run_id,
        "events",
        &["--replay-tools=recorded", "--json"],
    );
    assert_exit_ok(&recorded, "events + recorded");
    assert_stdout_equal(
        &recorded.stdout,
        &trace.stdout,
        "events + recorded vs trace",
    );
    let _ = replay_tool_call_lines;
}
