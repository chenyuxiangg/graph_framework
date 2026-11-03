//! F1c（v0.5 / M10）AC-25：`--replay-tools=reexec` 工具级重跑 **真 CLI 链路**。
//!
//! 语义依据：DR-017、spec 07 AC-25、06 §6.16.1、DR-014（v7 `tool_calls`）。
//! 数据通路：真 `zgraph` + 真 fake LLM + 真 Rust 内置工具 + 真 SQLite v7；
//! reexec 走真实 CLI（显式 `--allow-builtin-tools` + 显式 `--workspace-dir`），
//! 用 trace `result_sha256` 做漂移比对；不 mock 产品侧 reexec 逻辑。
//!
//! 覆盖：a 默认 recorded 不变/不执行；b 四种只读工具 reexec 成功 + additive
//! `replay_mode`/`drift` 且去 additive 后与 recorded 一致；c 文件/结果漂移
//! fail-closed；d 副作用工具不执行（write/edit/bash 记录复用、workspace/DB 不变）；
//! e truncated/缺行/缺 sha fail-closed；f 缺 flags exit2/非法 workspace exit3/
//! events-outputs 忽略 reexec/无 v8；MCP/Python source 与 failed/timeout 行复用记录。

#![cfg(unix)]

#[path = "cli_support/mod.rs"]
mod cli_support;
#[path = "v04_support/mod.rs"]
mod support;
#[path = "v03_support/mod.rs"]
mod v03;

use std::path::{Path, PathBuf};

use rusqlite::params;
use serde_json::{json, Value};

use cli_support::assert_exit;
use support::{
    agent_with_tools, assert_exit_ok, calls_with_text, completed_run_id, run_scenario, tool_call,
    tool_rows, trace_json, tree_snapshot, user_version, workspace, write_text,
};

// ---------------------------------------------------------------------------
// 构建真实混合 run（四个只读工具 + write/edit 副作用）
// ---------------------------------------------------------------------------

/// 混合 run：read/grep/glob/diff（可 reexec）+ write/edit（recorded 复用）。
struct MixedRun {
    db: PathBuf,
    run_id: String,
    ws: PathBuf,
    recorded_json: Vec<Value>,
    recorded_stdout: Vec<u8>,
    recorded_human: String,
}

impl MixedRun {
    /// 运行真实 CLI 并取 recorded 基线。
    fn build(dir: &Path) -> Self {
        let ws = workspace(dir);
        write_text(&ws.join("read.txt"), "alpha\nbeta\n");
        write_text(&ws.join("grep.txt"), "NEEDLE line\n");
        write_text(&ws.join("a.rs"), "g1\n");
        write_text(&ws.join("b.txt"), "g2\n");
        write_text(&ws.join("diff.txt"), "diff me\n");
        let calls = calls_with_text(vec![
            tool_call("c1", "builtin__read", json!({"path": "read.txt"})),
            tool_call(
                "c2",
                "builtin__grep",
                json!({"pattern": "NEEDLE", "path": "grep.txt"}),
            ),
            tool_call("c3", "builtin__glob", json!({"pattern": "*.rs"})),
            tool_call(
                "c4",
                "builtin__diff",
                json!({"path": "diff.txt", "old_string": "diff me", "new_string": "diff you"}),
            ),
            tool_call(
                "c5",
                "builtin__write",
                json!({"path": "note.txt", "content": "side effect"}),
            ),
            tool_call(
                "c6",
                "builtin__edit",
                json!({"path": "note.txt", "old_string": "side", "new_string": "SIDE"}),
            ),
        ]);
        let outcome = run_scenario(
            dir,
            &agent_with_tools(&[
                "builtin__read",
                "builtin__grep",
                "builtin__glob",
                "builtin__diff",
                "builtin__write",
                "builtin__edit",
            ]),
            calls,
            &support::builtin_flags(&ws, &[]),
        );
        let run_id = completed_run_id(&outcome, "reexec mixed run");
        let recorded = replay_tools_recorded(&outcome.db, &run_id);
        // 人类行单独取（JSON 基线不含 "no tools executed" 文案）。
        let human = support::replay_mode(&outcome.db, &run_id, "tools", &[]);
        Self {
            db: outcome.db,
            run_id,
            ws,
            recorded_json: v03::ndjson(&recorded),
            recorded_stdout: recorded.stdout.clone(),
            recorded_human: String::from_utf8_lossy(&human.stdout).into_owned(),
        }
    }

    /// reexec 参数（显式 opt-in + 显式 workspace）。
    fn reexec_args(&self) -> Vec<String> {
        replay_tools_args(&self.db, &self.run_id, Some(&self.ws))
    }
}

/// `replay --mode=tools --json`（recorded 默认）。
fn replay_tools_recorded(db: &Path, run_id: &str) -> std::process::Output {
    support::replay_mode(db, run_id, "tools", &["--json"])
}

/// reexec 回放参数；`workspace=None` 时省略 workspace（用于缺 flag 负例）。
fn replay_tools_args(db: &Path, run_id: &str, workspace: Option<&Path>) -> Vec<String> {
    let mut args = vec![
        "replay".to_string(),
        "--run".to_string(),
        run_id.to_string(),
        "--db".to_string(),
        cli_support::arg(db),
        "--mode".to_string(),
        "tools".to_string(),
        "--replay-tools".to_string(),
        "reexec".to_string(),
        "--allow-builtin-tools".to_string(),
        "--json".to_string(),
    ];
    if let Some(ws) = workspace {
        args.push("--workspace-dir".to_string());
        args.push(cli_support::arg(ws));
    }
    args
}

/// 执行 reexec。
fn run_reexec(args: &[String]) -> std::process::Output {
    cli_support::run_cli(args, &cli_support::repo_root())
}

/// 去掉 additive 字段后的 ToolCall 行。
fn strip_additive(lines: &[Value]) -> Vec<Value> {
    lines
        .iter()
        .map(|line| {
            let mut object = line.as_object().cloned().expect("ToolCall object");
            object.remove("replay_mode");
            object.remove("drift");
            Value::Object(object)
        })
        .collect()
}

/// 紧凑断言两个字符串 stdout 相等（避免整段 NDJSON panic）。
fn assert_stdout_same(left: &[u8], right: &[u8], context: &str) {
    if left == right {
        return;
    }
    let common = left
        .iter()
        .zip(right.iter())
        .take_while(|(a, b)| a == b)
        .count();
    panic!(
        "{context}: stdout differs at byte {common}; left_len={} right_len={}",
        left.len(),
        right.len()
    );
}

/// 紧凑断言 DB 文件快照相等（避免打印二进制大数组）。
fn assert_db_same(
    before: &[(String, Option<Vec<u8>>)],
    after: &[(String, Option<Vec<u8>>)],
    context: &str,
) {
    for ((left_name, left), (right_name, right)) in before.iter().zip(after.iter()) {
        assert_eq!(left_name, right_name, "{context}: snapshot names");
        match (left, right) {
            (Some(a), Some(b)) => {
                assert_stdout_same(a, b, &format!("{context}: {left_name}"));
            }
            (None, None) => {}
            _ => panic!("{context}: {left_name} presence differs"),
        }
    }
}

/// 紧凑断言之 workspace 树相等（避免打印文件字节）。
fn assert_ws_same(
    left: &std::collections::BTreeMap<String, Vec<u8>>,
    right: &std::collections::BTreeMap<String, Vec<u8>>,
    context: &str,
) {
    assert_eq!(
        left.keys().collect::<Vec<_>>(),
        right.keys().collect::<Vec<_>>(),
        "{context}: 文件集合"
    );
    for (key, value) in left {
        assert_eq!(value, right.get(key).expect("key"), "{context}: {key} 内容");
    }
}

/// 逐行 `(call_id, replay_mode, drift)`。
fn modes(lines: &[Value]) -> Vec<(String, String, bool)> {
    lines
        .iter()
        .map(|line| {
            (
                line["call_id"].as_str().unwrap_or_default().to_string(),
                line["replay_mode"].as_str().unwrap_or_default().to_string(),
                line["drift"].as_bool().unwrap_or(true),
            )
        })
        .collect()
}

// ---------------------------------------------------------------------------
// 合成 v7 行（MCP/Python/failed/timeout 复用记录）
// ---------------------------------------------------------------------------

/// 合成 v7 `tool_calls` 行描述（避免长参数列表）。
struct SyntheticRow<'a> {
    call_id: &'a str,
    tool_name: &'a str,
    source: &'a str,
    status: &'a str,
    args: &'a Value,
    result: Option<&'a Value>,
    error: Option<&'a Value>,
}

/// 向 `tool_calls` 插入一行合成记录（sha 由 `zgraph_trace::tool_result_sha256` 同口径生成）。
fn insert_synthetic(db: &Path, run_id: &str, row: SyntheticRow<'_>) {
    let SyntheticRow {
        call_id,
        tool_name,
        source,
        status,
        args,
        result,
        error,
    } = row;
    let instance_id: String = cli_support::read_only_connect(db)
        .query_row(
            "SELECT instance_id FROM tool_calls WHERE run_id = ?1 LIMIT 1",
            [run_id],
            |row| row.get(0),
        )
        .expect("instance id");
    let args_text = serde_json::to_string(args).expect("args json");
    let args_sha = zgraph_trace::tool_result_sha256(args).expect("args sha");
    let (result_text, result_sha) = match result {
        Some(value) => (
            Some(serde_json::to_string(value).expect("result json")),
            Some(zgraph_trace::tool_result_sha256(value).expect("result sha")),
        ),
        None => (None, None),
    };
    let (error_text, error_sha) = match error {
        Some(value) => (
            Some(serde_json::to_string(value).expect("error json")),
            Some(zgraph_trace::tool_result_sha256(value).expect("error sha")),
        ),
        None => (None, None),
    };
    support::writable_connect(db)
        .execute(
            "INSERT INTO tool_calls \
             (run_id, instance_id, node_id, attempt, call_id, tool_name, source, status, \
              args_json, result_json, error_json, truncated, args_sha256, result_sha256, \
              error_sha256, started_at, finished_at, duration_ms) \
             VALUES (?1, ?2, 'agent', 0, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 0, ?10, ?11, ?12, \
                     '2026-11-03T00:00:00.000Z', '2026-11-03T00:00:00.001Z', 1)",
            params![
                run_id,
                instance_id,
                call_id,
                tool_name,
                source,
                status,
                args_text,
                result_text,
                error_text,
                args_sha,
                result_sha,
                error_sha
            ],
        )
        .expect("insert synthetic tool_call");
}

/// 用 `VACUUM INTO` 复制独立 DB（构造损坏记录）。
fn copy_db(src: &Path, dst: &Path) {
    support::writable_connect(src)
        .execute("VACUUM INTO ?1", [dst.to_string_lossy()])
        .expect("vacuum into copy");
}

// ---------------------------------------------------------------------------
// AC-25 a / b：recorded 不变 + reexec 成功/additive
// ---------------------------------------------------------------------------

#[test]
fn ac25a_recorded_default_is_unchanged_and_executes_nothing() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mixed = MixedRun::build(dir.path());
    // 把副作用目标改成哨兵：recorded 回放绝不能执行 write/edit。
    write_text(&mixed.ws.join("note.txt"), "SENTINEL");
    let snapshot = tree_snapshot(&mixed.ws);
    // 预热一次 recorded 读（WAL/shm 稳定后再取 DB 快照）。
    let _ = replay_tools_recorded(&mixed.db, &mixed.run_id);
    let db_before = support::db_snapshot(&mixed.db);
    let rows_before = support::tool_call_count(&mixed.db, &mixed.run_id);
    // 默认 recorded 输出与基线逐字节一致（无 additive 字段），且不执行工具。
    let recorded = replay_tools_recorded(&mixed.db, &mixed.run_id);
    assert_exit_ok(&recorded, "recorded replay");
    assert_stdout_same(
        &recorded.stdout,
        &mixed.recorded_stdout,
        "recorded vs baseline",
    );
    assert!(
        mixed.recorded_human.contains("recorded; no tools executed"),
        "人类输出标注 recorded: {}",
        mixed.recorded_human
    );
    assert!(
        !String::from_utf8_lossy(&recorded.stdout).contains("replay_mode"),
        "recorded 不得出现 additive reexec 字段"
    );
    assert_ws_same(&tree_snapshot(&mixed.ws), &snapshot, "recorded workspace");
    assert_db_same(&support::db_snapshot(&mixed.db), &db_before, "recorded db");
    assert_eq!(
        support::tool_call_count(&mixed.db, &mixed.run_id),
        rows_before
    );
    assert_eq!(
        std::fs::read_to_string(mixed.ws.join("note.txt")).expect("note"),
        "SENTINEL"
    );
}

#[test]
fn ac25b_reexec_readonly_succeeds_and_is_additive() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mixed = MixedRun::build(dir.path());
    let recorded_before = replay_tools_recorded(&mixed.db, &mixed.run_id);
    // 预热一次 recorded 读（WAL/shm 稳定后再取 DB 快照）。
    let _ = replay_tools_recorded(&mixed.db, &mixed.run_id);
    let db_before = support::db_snapshot(&mixed.db);
    let rows_before = support::tool_call_count(&mixed.db, &mixed.run_id);
    let snapshot = tree_snapshot(&mixed.ws);

    let reexec = run_reexec(&mixed.reexec_args());
    assert_exit_ok(&reexec, "reexec replay");
    let lines = v03::ndjson(&reexec);
    assert_eq!(lines.len(), 6);
    assert_eq!(
        modes(&lines),
        vec![
            ("c1".to_string(), "reexec".to_string(), false),
            ("c2".to_string(), "reexec".to_string(), false),
            ("c3".to_string(), "reexec".to_string(), false),
            ("c4".to_string(), "reexec".to_string(), false),
            ("c5".to_string(), "recorded".to_string(), false),
            ("c6".to_string(), "recorded".to_string(), false),
        ],
        "read/grep/glob/diff reexec；write/edit recorded 复用"
    );
    // 去 additive 后与 recorded 逐行一致（结果/字段不因 reexec 改变）。
    assert_eq!(
        strip_additive(&lines),
        mixed.recorded_json,
        "reexec 去 additive 后必须与 recorded 一致"
    );
    // 输出 additive 人类行。
    let human = run_reexec(
        &mixed
            .reexec_args()
            .into_iter()
            .filter(|arg| arg != "--json")
            .collect::<Vec<_>>(),
    );
    assert_exit_ok(&human, "reexec human");
    let human_text = String::from_utf8_lossy(&human.stdout).into_owned();
    assert!(
        human_text.contains("mode=reexec") && human_text.contains("replay_mode=reexec drift=false"),
        "{human_text}"
    );
    assert!(human_text.contains("c5 builtin__write"), "{human_text}");
    // recorded 输出逐字节不变；workspace/DB/行数零副作用。
    let recorded_after = replay_tools_recorded(&mixed.db, &mixed.run_id);
    assert_eq!(recorded_after.stdout, recorded_before.stdout);
    assert_ws_same(&tree_snapshot(&mixed.ws), &snapshot, "reexec workspace");
    assert_db_same(&support::db_snapshot(&mixed.db), &db_before, "reexec db");
    assert_eq!(
        support::tool_call_count(&mixed.db, &mixed.run_id),
        rows_before
    );
}

// ---------------------------------------------------------------------------
// AC-25 c：文件/结果漂移 fail-closed
// ---------------------------------------------------------------------------

#[test]
fn ac25c_reexec_drift_fails_closed_for_all_readonly_tools() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mixed = MixedRun::build(dir.path());
    // (目标文件, 变更动作, 期望 drift 的 call_id, 恢复动作)
    type Mutation = Box<dyn Fn(&Path)>;
    let cases: Vec<(&str, Mutation, &str, Mutation)> = vec![
        (
            "read.txt",
            Box::new(|ws: &Path| write_text(&ws.join("read.txt"), "changed\n")),
            "c1",
            Box::new(|ws: &Path| write_text(&ws.join("read.txt"), "alpha\nbeta\n")),
        ),
        (
            "grep.txt",
            Box::new(|ws: &Path| write_text(&ws.join("grep.txt"), "NO MATCH\n")),
            "c2",
            Box::new(|ws: &Path| write_text(&ws.join("grep.txt"), "NEEDLE line\n")),
        ),
        (
            "*.rs",
            Box::new(|ws: &Path| write_text(&ws.join("extra.rs"), "g3\n")),
            "c3",
            Box::new(|ws: &Path| {
                std::fs::remove_file(ws.join("extra.rs")).expect("remove extra.rs");
            }),
        ),
        (
            "diff.txt",
            Box::new(|ws: &Path| write_text(&ws.join("diff.txt"), "different\n")),
            "c4",
            Box::new(|ws: &Path| write_text(&ws.join("diff.txt"), "diff me\n")),
        ),
    ];
    for (name, mutate, expected_call, restore) in cases {
        mutate(&mixed.ws);
        let output = run_reexec(&mixed.reexec_args());
        assert_exit(&output, 3, &format!("{name} drift"));
        let stderr = v03::err(&output);
        assert!(
            stderr.contains("REPLAY_TOOL_RECORD_MISMATCH"),
            "{name}: {stderr}"
        );
        assert!(
            stderr.contains(expected_call)
                && (stderr.contains("drift") || stderr.contains("reexec failed")),
            "{name}: 应报告 {expected_call} 漂移/重跑失败: {stderr}"
        );
        // 不写 runs.error_code（CLI 命名空间）。
        let conn = cli_support::read_only_connect(&mixed.db);
        let error_code: Option<String> = conn
            .query_row(
                "SELECT error_code FROM runs WHERE run_id = ?1",
                [&mixed.run_id],
                |row| row.get(0),
            )
            .expect("runs error_code");
        assert!(error_code.is_none(), "{}: 不得写 runs.error_code", name);
        drop(conn);
        restore(&mixed.ws);
        // 恢复后 reexec 重新成功（证明 drift 由真实文件变化触发）。
        let output = run_reexec(&mixed.reexec_args());
        assert_exit_ok(&output, &format!("{name} restored"));
    }
}

// ---------------------------------------------------------------------------
// AC-25 d：副作用工具不执行（含 bash 不触发沙箱）
// ---------------------------------------------------------------------------

#[test]
fn ac25d_side_effect_tools_are_never_executed() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mixed = MixedRun::build(dir.path());
    // 哨兵：若 write/edit 被执行，内容会被覆盖/改写。
    write_text(&mixed.ws.join("note.txt"), "SENTINEL");
    // 合成一条 builtin__bash 成功记录（本环境嵌套 Seatbelt：一旦尝试沙箱必 exit 3）。
    insert_synthetic(
        &mixed.db,
        &mixed.run_id,
        SyntheticRow {
            call_id: "bash1",
            tool_name: "builtin__bash",
            source: "builtin",
            status: "ok",
            args: &json!({"command": "echo hi"}),
            result: Some(
                &json!({"exit_code": 0, "stdout": "hi\n", "stderr": "", "truncated": false}),
            ),
            error: None,
        },
    );
    let snapshot = tree_snapshot(&mixed.ws);
    // 预热一次 recorded 读（WAL/shm 稳定后再取 DB 快照）。
    let _ = replay_tools_recorded(&mixed.db, &mixed.run_id);
    let db_before = support::db_snapshot(&mixed.db);
    let rows_before = support::tool_call_count(&mixed.db, &mixed.run_id);
    let output = run_reexec(&mixed.reexec_args());
    assert_exit_ok(&output, "reexec with side-effect records");
    let lines = v03::ndjson(&output);
    let bashed = lines
        .iter()
        .find(|line| line["call_id"] == json!("bash1"))
        .expect("bash synthetic row in replay");
    assert_eq!(bashed["replay_mode"], json!("recorded"), "bash 复用记录");
    assert_eq!(
        std::fs::read_to_string(mixed.ws.join("note.txt")).expect("note"),
        "SENTINEL",
        "write/edit 不得被执行"
    );
    assert_ws_same(
        &tree_snapshot(&mixed.ws),
        &snapshot,
        "side-effect workspace",
    );
    assert_db_same(
        &support::db_snapshot(&mixed.db),
        &db_before,
        "side-effect db",
    );
    assert_eq!(
        support::tool_call_count(&mixed.db, &mixed.run_id),
        rows_before
    );
    // 若 bash 被触发，嵌套 Seatbelt 会以 HARNESS_CONFIG_LOAD/sandbox_nested 失败；
    // 成功即证明未触发沙箱执行。
    assert!(
        !v03::err(&output).contains("sandbox"),
        "reexec 不得触发 bash/沙箱: {}",
        v03::err(&output)
    );
}

// ---------------------------------------------------------------------------
// AC-25 e：truncated / 缺行 / 缺 sha fail-closed
// ---------------------------------------------------------------------------

#[test]
fn ac25e_reexec_fail_closed_on_truncated_missing_sha_and_missing_row() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mixed = MixedRun::build(dir.path());
    let snapshot = tree_snapshot(&mixed.ws);

    // ① truncated=1 → REPLAY_TOOL_RECORD_TRUNCATED。
    let truncated_db = dir.path().join("truncated.db");
    copy_db(&mixed.db, &truncated_db);
    support::writable_connect(&truncated_db)
        .execute(
            "UPDATE tool_calls SET truncated = 1 WHERE run_id = ?1 AND call_id = 'c1'",
            [&mixed.run_id],
        )
        .expect("mark truncated");
    let output = run_reexec(&replay_tools_args(
        &truncated_db,
        &mixed.run_id,
        Some(&mixed.ws),
    ));
    assert_exit(&output, 3, "truncated");
    assert!(
        v03::err(&output).contains("REPLAY_TOOL_RECORD_TRUNCATED"),
        "{}",
        v03::err(&output)
    );

    // ② 只读行缺 result_sha256 → validate 先以 MISMATCH 拒绝（记录无法复核，fail-closed）。
    let no_sha_db = dir.path().join("no-sha.db");
    copy_db(&mixed.db, &no_sha_db);
    support::writable_connect(&no_sha_db)
        .execute(
            "UPDATE tool_calls SET result_sha256 = NULL WHERE run_id = ?1 AND call_id = 'c1'",
            [&mixed.run_id],
        )
        .expect("clear result sha");
    let output = run_reexec(&replay_tools_args(
        &no_sha_db,
        &mixed.run_id,
        Some(&mixed.ws),
    ));
    assert_exit(&output, 3, "missing sha");
    let stderr = v03::err(&output);
    assert!(
        stderr.contains("REPLAY_TOOL_RECORD_MISMATCH") && stderr.contains("result_sha256"),
        "{}",
        stderr
    );

    // ③ 删除一行（streaming tool_call_start 计数 > 记录行数）→ MISSING。
    let missing_db = dir.path().join("missing.db");
    copy_db(&mixed.db, &missing_db);
    support::writable_connect(&missing_db)
        .execute(
            "DELETE FROM tool_calls WHERE run_id = ?1 AND call_id = 'c1'",
            [&mixed.run_id],
        )
        .expect("delete row");
    let output = run_reexec(&replay_tools_args(
        &missing_db,
        &mixed.run_id,
        Some(&mixed.ws),
    ));
    assert_exit(&output, 3, "missing row");
    assert!(
        v03::err(&output).contains("REPLAY_TOOL_RECORD_MISSING"),
        "{}",
        v03::err(&output)
    );
    // 所有 fail-closed 分支均不得执行/改 workspace。
    assert_eq!(tree_snapshot(&mixed.ws), snapshot);
}

// ---------------------------------------------------------------------------
// AC-25 f：缺 flags / 非法 workspace / events-outputs 忽略 reexec / 无 v8
// ---------------------------------------------------------------------------

#[test]
fn ac25f_flags_workspace_and_modes_are_enforced() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mixed = MixedRun::build(dir.path());

    // 缺 --allow-builtin-tools → exit 2 用法错误。
    let mut args = mixed.reexec_args();
    let position = args
        .iter()
        .position(|arg| arg == "--allow-builtin-tools")
        .expect("flag present");
    args.remove(position);
    let output = run_reexec(&args);
    assert_exit(&output, 2, "缺 allow-builtin-tools");
    assert!(
        v03::err(&output).contains("requires --allow-builtin-tools"),
        "{}",
        v03::err(&output)
    );

    // 缺显式 --workspace-dir → exit 2（不默认 cwd）。
    let output = run_reexec(&replay_tools_args(&mixed.db, &mixed.run_id, None));
    assert_exit(&output, 2, "缺 workspace-dir");
    assert!(
        v03::err(&output).contains("explicit --workspace-dir"),
        "{}",
        v03::err(&output)
    );

    // 非法 workspace → exit 3（HARNESS_CONFIG_LOAD workspace_invalid）。
    let bad_ws = dir.path().join("no-such-ws");
    let output = run_reexec(&replay_tools_args(&mixed.db, &mixed.run_id, Some(&bad_ws)));
    assert_exit(&output, 3, "非法 workspace");
    let stderr = v03::err(&output);
    assert!(
        stderr.contains("HARNESS_CONFIG_LOAD") && stderr.contains("workspace_invalid"),
        "{stderr}"
    );

    // events/outputs 忽略 reexec（输出与不带参数逐字节一致）。
    for mode in ["events", "outputs"] {
        let base = cli_support::run_cli(
            &[
                "replay".to_string(),
                "--run".to_string(),
                mixed.run_id.clone(),
                "--db".to_string(),
                cli_support::arg(&mixed.db),
                "--mode".to_string(),
                mode.to_string(),
                "--json".to_string(),
            ],
            &cli_support::repo_root(),
        );
        let mut with_reexec = vec![
            "replay".to_string(),
            "--run".to_string(),
            mixed.run_id.clone(),
            "--db".to_string(),
            cli_support::arg(&mixed.db),
            "--mode".to_string(),
            mode.to_string(),
            "--json".to_string(),
        ];
        with_reexec.extend([
            "--replay-tools".to_string(),
            "reexec".to_string(),
            "--allow-builtin-tools".to_string(),
            "--workspace-dir".to_string(),
            cli_support::arg(&mixed.ws),
        ]);
        let with = cli_support::run_cli(&with_reexec, &cli_support::repo_root());
        assert_exit_ok(&with, mode);
        assert_eq!(with.stdout, base.stdout, "{mode}: reexec 应被忽略");
    }
    // 无 v8：schema 仍 v7，无新表/列。
    assert_eq!(user_version(&mixed.db), 7, "reexec 不得升 v8");
    let columns: Vec<String> = {
        let conn = cli_support::read_only_connect(&mixed.db);
        let mut statement = conn
            .prepare("PRAGMA table_info(tool_calls)")
            .expect("table_info");
        statement
            .query_map([], |row| row.get::<_, String>(1))
            .expect("columns")
            .map(|row| row.expect("column"))
            .collect()
    };
    assert!(
        !columns
            .iter()
            .any(|name| name.contains("pre_sha") || name.contains("reexec")),
        "不得新增 reexec/pre_sha 列: {columns:?}"
    );
    // trace --json 仍为既有字段（无 reexec additive 输出）。
    let trace = trace_json(&mixed.db, &mixed.run_id);
    assert!(
        !String::from_utf8_lossy(&trace.stdout).contains("replay_mode"),
        "trace 输出不得带 reexec additive 字段"
    );
}

// ---------------------------------------------------------------------------
// AC-25（REV-118 P3②）：MCP/Python source 与 failed/timeout 行复用记录
// ---------------------------------------------------------------------------

#[test]
fn ac25_reexec_reuses_mcp_python_and_failed_rows() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mixed = MixedRun::build(dir.path());
    // 合成 MCP / Python / failed / timeout 行（均带合法 sha）。
    insert_synthetic(
        &mixed.db,
        &mixed.run_id,
        SyntheticRow {
            call_id: "mcp1",
            tool_name: "mcp__echo__echo",
            source: "mcp",
            status: "ok",
            args: &json!({"text": "hi"}),
            result: Some(&json!({"content": [{"type": "text", "text": "echo:hi"}]})),
            error: None,
        },
    );
    insert_synthetic(
        &mixed.db,
        &mixed.run_id,
        SyntheticRow {
            call_id: "py1",
            tool_name: "add",
            source: "python",
            status: "ok",
            args: &json!({"a": 1, "b": 2}),
            result: Some(&json!({"sum": 3})),
            error: None,
        },
    );
    insert_synthetic(
        &mixed.db,
        &mixed.run_id,
        SyntheticRow {
            call_id: "fail1",
            tool_name: "builtin__read",
            source: "builtin",
            status: "error",
            args: &json!({"path": "missing.txt"}),
            result: None,
            error: Some(&json!({"error": {"reason": "not_found", "message": "missing"}})),
        },
    );
    insert_synthetic(
        &mixed.db,
        &mixed.run_id,
        SyntheticRow {
            call_id: "timeout1",
            tool_name: "mcp__echo__slow",
            source: "mcp",
            status: "timeout",
            args: &json!({}),
            result: None,
            error: None,
        },
    );
    let snapshot = tree_snapshot(&mixed.ws);
    let rows_before = support::tool_call_count(&mixed.db, &mixed.run_id);
    let output = run_reexec(&mixed.reexec_args());
    assert_exit_ok(&output, "reexec synthetic sources");
    let lines = v03::ndjson(&output);
    for call_id in ["mcp1", "py1", "fail1", "timeout1"] {
        let line = lines
            .iter()
            .find(|line| line["call_id"] == json!(call_id))
            .unwrap_or_else(|| panic!("{call_id} in replay"));
        assert_eq!(
            line["replay_mode"],
            json!("recorded"),
            "{call_id}: 非只读白名单/失败行必须复用记录"
        );
        assert_eq!(line["drift"], json!(false));
    }
    // 结果/错误按记录原样输出（不执行、不重算）。
    let mcp = lines
        .iter()
        .find(|line| line["call_id"] == json!("mcp1"))
        .expect("mcp1");
    assert_eq!(mcp["tool_name"], json!("mcp__echo__echo"));
    assert_eq!(mcp["source"], json!("mcp"));
    assert_eq!(mcp["result"]["content"][0]["text"], json!("echo:hi"));
    let fail = lines
        .iter()
        .find(|line| line["call_id"] == json!("fail1"))
        .expect("fail1");
    assert_eq!(fail["status"], json!("error"));
    assert_eq!(fail["error"]["error"]["reason"], json!("not_found"));
    assert_ws_same(&tree_snapshot(&mixed.ws), &snapshot, "synthetic workspace");
    assert_eq!(
        support::tool_call_count(&mixed.db, &mixed.run_id),
        rows_before
    );
    assert_eq!(
        tool_rows(&mixed.db, &mixed.run_id).len() as i64,
        rows_before
    );
}
