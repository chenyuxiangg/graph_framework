//! `zgraph trace` CLI 集成测试（06 §6.10.4 / §6.10.5 / §6.11.2 / §6.8.3）。
//!
//! 用 `zgraph_trace::TraceWriter` 在独立 tempdir 造库，再通过
//! `CARGO_BIN_EXE_zgraph` 运行真实二进制（非 TTY 环境，断言无 ANSI）。
//! 需要改库的场景（schema 超前、interrupted 标定）直接用 rusqlite 操作临时库。

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::{json, Value};
use tempfile::TempDir;
use zgraph_trace::{
    EventQuery, GateResultRecord, InstanceSnapshot, InstanceStatus, RecordMode, RunFinishedInfo,
    RunMetrics, RunStartedInfo, RunStatus, StreamingEvent, TraceHandle, TraceInput, TraceReader,
    TraceWriter, TransitionRecord,
};

const RUN_ID: &str = "run-trace-1";
const GRAPH_HASH: &str = "0123456789abcdef";
const GRAPH_NAME: &str = "demo_graph";
/// 事件总数（见 `send_events` 的发送顺序）。
const EVENT_COUNT: usize = 16;

/// 基准时刻 `2026-10-28T09:12:03.104Z` 加上偏移毫秒（秒数保持在 09:12 内）。
fn at(offset_ms: i64) -> String {
    let total = 3_104 + offset_ms;
    format!("2026-10-28T09:12:{:02}.{:03}Z", total / 1000, total % 1000)
}

/// 一次测试使用的只读完成态库。
struct Fixture {
    dir: TempDir,
    path: PathBuf,
    /// 按 seq 升序的全部事件 seq（供 `--since-seq` 用例）。
    seqs: Vec<i64>,
}

impl Fixture {
    fn dir(&self) -> &Path {
        self.dir.path()
    }
}

fn path_str(path: &Path) -> &str {
    path.to_str().expect("utf8 path")
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

fn json_lines(output: &Output) -> Vec<Value> {
    stdout(output)
        .lines()
        .map(|line| serde_json::from_str(line).expect("each stdout line is JSON"))
        .collect()
}

fn transition(
    node: &str,
    instance: &str,
    from: InstanceStatus,
    to: InstanceStatus,
    reason: &str,
    ts: String,
) -> TraceInput {
    let node_type = if node == "start" {
        "start"
    } else {
        "python_node"
    };
    let snapshot = InstanceSnapshot {
        run_id: RUN_ID.to_string(),
        instance_id: instance.to_string(),
        node_id: node.to_string(),
        node_type: node_type.to_string(),
        attempt: 0,
        status: to,
        input: Some(json!({"in": 1})),
        output: if to == InstanceStatus::Completed {
            Some(json!({"result": "ok"}))
        } else {
            None
        },
        error: None,
        error_code: None,
        created_at: ts.clone(),
        started_at: None,
        finished_at: None,
    };
    TraceInput::Transition {
        record: TransitionRecord {
            run_id: RUN_ID.to_string(),
            instance_id: instance.to_string(),
            node_id: node.to_string(),
            attempt: 0,
            from,
            to,
            reason: Some(reason.to_string()),
            ts,
        },
        snapshot,
    }
}

fn streaming(instance: &str, node: &str, event: StreamingEvent, ts: String) -> TraceInput {
    TraceInput::Streaming {
        run_id: RUN_ID.to_string(),
        instance_id: instance.to_string(),
        node_id: Some(node.to_string()),
        event,
        at: ts,
    }
}

/// 发送固定 16 条事件（7 Transition / 4 Streaming / 其余各 1）。
fn send_events(handle: &TraceHandle) {
    handle.send(TraceInput::RunStarted(RunStartedInfo {
        run_id: RUN_ID.to_string(),
        graph_hash: GRAPH_HASH.to_string(),
        graph_name: GRAPH_NAME.to_string(),
        dsl_json: "{}".to_string(),
        user_id: "local".to_string(),
        entry_inputs: json!({"start": {"in": 1}}),
        seed: 7,
        record_mode: RecordMode::Hash,
        owner_pid: std::process::id(),
        owner_host: "test-host".to_string(),
        started_at: at(0),
    }));
    handle.send(transition(
        "start",
        "inst-start",
        InstanceStatus::Pending,
        InstanceStatus::Ready,
        "inputs ready",
        at(10),
    ));
    handle.send(transition(
        "start",
        "inst-start",
        InstanceStatus::Ready,
        InstanceStatus::Running,
        "dispatch",
        at(20),
    ));
    handle.send(transition(
        "start",
        "inst-start",
        InstanceStatus::Running,
        InstanceStatus::Completed,
        "completed",
        at(30),
    ));
    handle.send(transition(
        "answer_writer",
        "inst-answer",
        InstanceStatus::Pending,
        InstanceStatus::Ready,
        "inputs ready",
        at(40),
    ));
    handle.send(transition(
        "answer_writer",
        "inst-answer",
        InstanceStatus::Ready,
        InstanceStatus::Running,
        "dispatch",
        at(50),
    ));
    handle.send(transition(
        "answer_writer",
        "inst-answer",
        InstanceStatus::Running,
        InstanceStatus::Completed,
        "completed",
        at(60),
    ));
    handle.send(transition(
        "human_handoff",
        "inst-human",
        InstanceStatus::Pending,
        InstanceStatus::Skipped,
        "all_sources_blocked",
        at(70),
    ));
    handle.send(TraceInput::GateResult(GateResultRecord {
        run_id: RUN_ID.to_string(),
        instance_id: "inst-answer".to_string(),
        node_id: "answer_writer".to_string(),
        edge_index: 2,
        to_node: "human_handoff".to_string(),
        to_port: "in".to_string(),
        gate_fn: Some("gates.hr.need_human".to_string()),
        passed: false,
        error: None,
        ts: at(80),
    }));
    handle.send(streaming(
        "inst-answer",
        "answer_writer",
        StreamingEvent::TokenDelta {
            text: "hello".to_string(),
        },
        at(100),
    ));
    handle.send(streaming(
        "inst-answer",
        "answer_writer",
        StreamingEvent::Progress {
            percent: 42,
            message: "half".to_string(),
        },
        at(110),
    ));
    handle.send(streaming(
        "inst-answer",
        "answer_writer",
        StreamingEvent::ToolCallStart {
            call_id: "call-1".to_string(),
            tool_name: "search".to_string(),
            args_summary: json!({"q": "refund"}),
        },
        at(120),
    ));
    handle.send(streaming(
        "inst-answer",
        "answer_writer",
        StreamingEvent::ToolCallEnd {
            call_id: "call-1".to_string(),
            tool_name: "search".to_string(),
            ok: false,
            result_summary: None,
        },
        at(130),
    ));
    handle.send(TraceInput::NodeTimedOut {
        run_id: RUN_ID.to_string(),
        instance_id: "inst-timeout".to_string(),
        elapsed_ms: 1000,
        at: at(140),
    });
    handle.send(TraceInput::NodeAbandoned {
        run_id: RUN_ID.to_string(),
        instance_id: "inst-timeout".to_string(),
        at: at(150),
    });
    handle.send(TraceInput::RunFinished(RunFinishedInfo {
        run_id: RUN_ID.to_string(),
        status: RunStatus::Completed,
        output_summary: Some(json!({"result": "ok"})),
        error: None,
        metrics: RunMetrics {
            streaming_dropped: 2,
            abandoned_tasks: 0,
            trace_backpressure: 5,
            killed_workers: 1,
        },
        at: at(1842),
    }));
}

/// 造一个写入并关闭好的临时库，返回路径与事件 seq 列表。
fn fixture() -> Fixture {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("trace.db");
    let writer = TraceWriter::open(&path).expect("open trace writer");
    let handle = writer.handle();
    send_events(&handle);
    writer.flush().expect("flush trace writer");
    let reader = TraceReader::open_readonly(&path).expect("open readonly reader");
    let seqs: Vec<i64> = reader
        .events(RUN_ID, &EventQuery::default())
        .expect("query events")
        .iter()
        .map(|event| event.seq)
        .collect();
    drop(reader);
    writer.close().expect("close trace writer");
    Fixture { dir, path, seqs }
}

#[test]
fn human_replay_renders_header_timeline_and_details() {
    let fx = fixture();
    let output = zgraph(&["trace", RUN_ID, "--db", path_str(&fx.path)]);
    assert_eq!(code(&output), 0, "{}", stderr(&output));
    let text = stdout(&output);

    assert!(
        text.contains("run run-trace-1  status=Completed  graph=demo_graph  hash=0123456789abcdef"),
        "{text}"
    );
    assert!(
        text.contains(
            "started 2026-10-28T09:12:03.104Z  finished 2026-10-28T09:12:04.946Z  \
             duration 1842ms  events 16  dropped 2  abandoned 0"
        ),
        "{text}"
    );
    // kind 明细
    assert!(text.contains("Pending -> Ready (inputs ready)"), "{text}");
    assert!(text.contains("Running -> Completed (completed)"), "{text}");
    assert!(
        text.contains("Pending -> Skipped (all_sources_blocked)"),
        "{text}"
    );
    assert!(
        text.contains("edge#2 gate=gates.hr.need_human passed=false"),
        "{text}"
    );
    assert!(text.contains("token[0] 5 chars (1 deltas)"), "{text}");
    assert!(text.contains("progress 42% \"half\""), "{text}");
    assert!(
        text.contains("tool_call_start search call_id=call-1"),
        "{text}"
    );
    assert!(text.contains("tool_call_end search ok=false"), "{text}");
    assert!(text.contains("elapsed=1000ms"), "{text}");
    assert!(text.contains("status=Completed"), "{text}");
    // 事件行按 seq：RunStarted 在最前、RunFinished 在最后；表格总行数 = 4 + 16
    let started = text.find("RunStarted").expect("RunStarted row");
    let finished = text.find("RunFinished").expect("RunFinished row");
    assert!(started < finished, "RunStarted before RunFinished");
    assert_eq!(text.lines().count(), 4 + EVENT_COUNT, "{text}");
    // 非 TTY：无 ANSI；无 stderr 诊断
    assert!(!text.contains('\u{1b}'), "no ANSI in human output");
    assert!(stderr(&output).is_empty(), "{}", stderr(&output));
}

#[test]
fn json_replay_is_ndjson_ordered_by_seq() {
    let fx = fixture();
    let output = zgraph(&["trace", RUN_ID, "--db", path_str(&fx.path), "--json"]);
    assert_eq!(code(&output), 0, "{}", stderr(&output));
    assert!(stderr(&output).is_empty(), "{}", stderr(&output));
    assert!(!stdout(&output).contains('\u{1b}'));

    let events = json_lines(&output);
    assert_eq!(events.len(), EVENT_COUNT);
    for pair in events.windows(2) {
        assert!(
            pair[1]["seq"].as_i64().expect("seq") > pair[0]["seq"].as_i64().expect("seq"),
            "seq strictly increasing: {:?} vs {:?}",
            pair[0]["seq"],
            pair[1]["seq"]
        );
    }

    let kinds: Vec<&str> = events
        .iter()
        .map(|event| event["kind"].as_str().expect("kind"))
        .collect();
    assert_eq!(
        kinds,
        vec![
            "RunStarted",
            "Transition",
            "Transition",
            "Transition",
            "Transition",
            "Transition",
            "Transition",
            "Transition",
            "GateResult",
            "Streaming",
            "Streaming",
            "Streaming",
            "Streaming",
            "NodeTimedOut",
            "NodeAbandoned",
            "RunFinished",
        ]
    );

    let first = &events[0];
    assert_eq!(first["kind"], "RunStarted");
    assert_eq!(first["run_id"], RUN_ID);
    assert_eq!(first["at"], "2026-10-28T09:12:03.104Z");
    assert_eq!(first["severity"], "info");
    assert!(first.get("instance_id").is_none());
    assert!(first.get("node_id").is_none());
    assert_eq!(first["payload"]["status"], "Running");
    assert_eq!(first["payload"]["graph_hash"], GRAPH_HASH);

    // 迁移 / gate / streaming / timeout / abandoned / finished 字段
    let ok = |kind: &str, severity: &str| {
        let event = events
            .iter()
            .find(|event| event["kind"] == kind && event["severity"] == severity)
            .unwrap_or_else(|| panic!("missing {kind}/{severity}"));
        event
    };
    let gate = ok("GateResult", "warn");
    assert_eq!(gate["payload"]["record"]["passed"], json!(false));
    assert_eq!(gate["payload"]["record"]["edge_index"], json!(2));
    assert_eq!(gate["payload"]["record"]["gate_fn"], "gates.hr.need_human");
    let timed_out = ok("NodeTimedOut", "error");
    assert_eq!(timed_out["payload"]["elapsed_ms"], json!(1000));
    let abandoned = ok("NodeAbandoned", "warn");
    assert_eq!(abandoned["payload"]["at"], at(150));
    let token = events
        .iter()
        .find(|event| event["kind"] == "Streaming" && event["payload"]["variant"] == "token")
        .expect("token segment");
    assert_eq!(token["node_id"], "answer_writer");
    assert_eq!(token["payload"]["chars"], json!(5));
    assert_eq!(token["payload"]["delta_count"], json!(1));
    let finished = events.last().expect("last event");
    assert_eq!(finished["kind"], "RunFinished");
    assert_eq!(finished["payload"]["status"], "Completed");
    assert_eq!(finished["at"], "2026-10-28T09:12:04.946Z");
}

#[test]
fn filters_limit_kinds_and_streaming_toggle() {
    let fx = fixture();
    let db = path_str(&fx.path);
    let since = fx.seqs[9].to_string();

    let output = zgraph(&["trace", RUN_ID, "--db", db, "--json", "--since-seq", &since]);
    assert_eq!(code(&output), 0, "{}", stderr(&output));
    let events = json_lines(&output);
    assert_eq!(events.len(), EVENT_COUNT - 10, "{}", stdout(&output));
    assert!(events
        .iter()
        .all(|event| event["seq"].as_i64().expect("seq") > fx.seqs[9]));

    let output = zgraph(&["trace", RUN_ID, "--db", db, "--json", "--limit", "2"]);
    let events = json_lines(&output);
    assert_eq!(events.len(), 2);
    assert_eq!(events[0]["seq"], json!(fx.seqs[0]));
    assert_eq!(events[1]["seq"], json!(fx.seqs[1]));

    let output = zgraph(&["trace", RUN_ID, "--db", db, "--json", "--limit", "0"]);
    assert_eq!(json_lines(&output).len(), EVENT_COUNT);

    let output = zgraph(&[
        "trace",
        RUN_ID,
        "--db",
        db,
        "--json",
        "--kinds",
        "Transition",
    ]);
    let events = json_lines(&output);
    assert_eq!(events.len(), 7);
    assert!(events.iter().all(|event| event["kind"] == "Transition"));

    let output = zgraph(&["trace", RUN_ID, "--db", db, "--json", "--no-streaming"]);
    let events = json_lines(&output);
    assert_eq!(events.len(), EVENT_COUNT - 4);
    assert!(events.iter().all(|event| event["kind"] != "Streaming"));

    let output = zgraph(&[
        "trace",
        RUN_ID,
        "--db",
        db,
        "--json",
        "--kinds",
        "Streaming,RunFinished",
        "--no-streaming",
    ]);
    let events = json_lines(&output);
    assert_eq!(events.len(), 1);
    assert_eq!(events[0]["kind"], "RunFinished");

    // 只剩 Streaming 又被 --no-streaming 归一化为空：无事件但仍成功
    let output = zgraph(&[
        "trace",
        RUN_ID,
        "--db",
        db,
        "--json",
        "--kinds",
        "Streaming",
        "--no-streaming",
    ]);
    assert_eq!(code(&output), 0, "{}", stderr(&output));
    assert!(stdout(&output).is_empty(), "{}", stdout(&output));
}

#[test]
fn stats_human_and_json_report_numbers() {
    let fx = fixture();
    let db = path_str(&fx.path);

    let output = zgraph(&["trace", RUN_ID, "--db", db, "--stats"]);
    assert_eq!(code(&output), 0, "{}", stderr(&output));
    assert!(stderr(&output).is_empty(), "{}", stderr(&output));
    let text = stdout(&output);
    assert!(
        text.contains("run run-trace-1 status=Completed duration=1842ms"),
        "{text}"
    );
    assert!(
        text.contains(
            "events 16  (GateResult 1, NodeAbandoned 1, NodeTimedOut 1, RunFinished 1, \
             RunStarted 1, Streaming 4, Transition 7)"
        ),
        "{text}"
    );
    assert!(
        text.contains("instances 3 (Completed 2, Skipped 1)"),
        "{text}"
    );
    assert!(text.contains("streaming segments 4  dropped 2"), "{text}");
    assert!(
        text.contains("timeouts 1  abandoned 1  trace_backpressure 5ms"),
        "{text}"
    );
    let db_line = format!("db {}", fx.path.display());
    assert!(text.contains(&db_line), "{text}");
    assert!(text.contains("  runs 1"), "{text}");
    assert!(!text.contains('\u{1b}'));

    let output = zgraph(&["trace", RUN_ID, "--db", db, "--stats", "--json"]);
    assert_eq!(code(&output), 0, "{}", stderr(&output));
    let value: Value = serde_json::from_str(&stdout(&output)).expect("stats json object");
    assert_eq!(value["run_id"], RUN_ID);
    assert_eq!(value["status"], "Completed");
    assert_eq!(value["duration_ms"], json!(1842));
    assert_eq!(value["events"]["total"], json!(16));
    assert_eq!(value["events"]["kinds"]["Transition"], json!(7));
    assert_eq!(value["events"]["kinds"]["Streaming"], json!(4));
    assert_eq!(value["events"]["kinds"]["GateResult"], json!(1));
    assert_eq!(value["instances"]["status"]["Completed"], json!(2));
    assert_eq!(value["instances"]["status"]["Skipped"], json!(1));
    assert_eq!(value["streaming"]["segments"], json!(4));
    assert_eq!(value["streaming"]["dropped"], json!(2));
    assert_eq!(value["timeouts"], json!(1));
    assert_eq!(value["abandoned"], json!(1));
    assert_eq!(value["trace_backpressure"], json!(5));
    assert_eq!(value["db"]["path"], fx.path.display().to_string());
    assert!(value["db"]["size_bytes"].as_u64().expect("size") > 0);
    assert_eq!(value["db"]["runs"], json!(1));
    assert_eq!(
        stdout(&output).lines().count(),
        1,
        "single JSON object line"
    );
}

#[test]
fn missing_database_and_run_exit_4() {
    let fx = fixture();
    let missing_db = fx.dir().join("no-such.db");

    let output = zgraph(&["trace", RUN_ID, "--db", path_str(&missing_db)]);
    assert_eq!(code(&output), 4, "{}", stderr(&output));
    assert!(stdout(&output).is_empty());
    assert!(
        stderr(&output).starts_with("error[STORAGE_ERROR]: "),
        "{}",
        stderr(&output)
    );
    assert!(!missing_db.exists(), "trace must not create the db");

    let output = zgraph(&["trace", "missing-run", "--db", path_str(&fx.path)]);
    assert_eq!(code(&output), 4, "{}", stderr(&output));
    assert!(
        stderr(&output).contains("error[STORAGE_ERROR]: run not found: missing-run"),
        "{}",
        stderr(&output)
    );
}

#[test]
fn usage_errors_exit_2() {
    let fx = fixture();
    let db = path_str(&fx.path);
    for args in [
        vec!["trace"],
        vec!["trace", RUN_ID, "--nope"],
        vec!["trace", RUN_ID, "--limit", "abc"],
        vec!["trace", RUN_ID, "--since-seq", "-1"],
        vec!["trace", RUN_ID, "--kinds", "Nope"],
        vec!["trace", RUN_ID, "--kinds", ""],
        vec!["trace", RUN_ID, "--db"],
        vec!["trace", "a", "b"],
    ] {
        let output = zgraph(&args);
        assert_eq!(code(&output), 2, "args={args:?}: {}", stderr(&output));
        assert!(stdout(&output).is_empty(), "args={args:?}");
    }
    assert!(stderr(&zgraph(&["trace", RUN_ID, "--kinds", "Nope"]))
        .contains("invalid --kinds value: 'Nope'"));
    // 合法 db 上 --help 成功且不访问库
    let output = zgraph(&["trace", "--help", "--db", db]);
    assert_eq!(code(&output), 0);
    assert!(stdout(&output).contains("usage: zgraph trace <run_id>"));
    assert!(stderr(&output).is_empty());
    // -q/--quiet 被接受
    let output = zgraph(&["trace", RUN_ID, "--db", db, "-q"]);
    assert_eq!(code(&output), 0, "{}", stderr(&output));
    let output = zgraph(&["trace", RUN_ID, "--db", db, "--quiet"]);
    assert_eq!(code(&output), 0, "{}", stderr(&output));
}

#[test]
fn schema_too_new_exits_4() {
    let fx = fixture();
    let conn = rusqlite::Connection::open(&fx.path).expect("open db for version bump");
    conn.pragma_update(None, "user_version", 99_i64)
        .expect("set user_version=99");
    drop(conn);

    let output = zgraph(&["trace", RUN_ID, "--db", path_str(&fx.path)]);
    assert_eq!(code(&output), 4, "{}", stderr(&output));
    let text = stderr(&output);
    assert!(text.contains("error[STORE_SCHEMA_TOO_NEW]: "), "{text}");
    assert!(text.contains("db version=99"), "{text}");
    assert!(stdout(&output).is_empty());
}

#[test]
fn interrupted_run_prints_truncation_note_and_exits_1() {
    let fx = fixture();
    let conn = rusqlite::Connection::open(&fx.path).expect("open db for interrupted update");
    conn.execute(
        "UPDATE runs SET status = 'Failed', interrupted = 1, finished_at = NULL, recovered_at = ?1 \
         WHERE run_id = ?2",
        rusqlite::params!["2026-10-28T09:12:05.000Z", RUN_ID],
    )
    .expect("mark interrupted");
    conn.execute(
        "DELETE FROM events WHERE run_id = ?1 AND kind = 'RunFinished'",
        [RUN_ID],
    )
    .expect("drop RunFinished");
    drop(conn);

    let output = zgraph(&["trace", RUN_ID, "--db", path_str(&fx.path)]);
    assert_eq!(code(&output), 1, "{}", stderr(&output));
    let text = stdout(&output);
    assert!(
        text.contains(
            "!! trace truncated: run interrupted at 2026-10-28T09:12:05.000Z (no RunFinished)"
        ),
        "{text}"
    );
    assert!(text.contains("status=Failed"), "{text}");
    assert!(!text.contains('\u{1b}'));

    // --json：stdout 仍是纯 NDJSON，截断诊断走 stderr，退出码 1
    let output = zgraph(&["trace", RUN_ID, "--db", path_str(&fx.path), "--json"]);
    assert_eq!(code(&output), 1, "{}", stderr(&output));
    let events = json_lines(&output);
    assert_eq!(events.len(), EVENT_COUNT - 1, "{}", stdout(&output));
    assert!(
        stderr(&output).contains("!! trace truncated:"),
        "{}",
        stderr(&output)
    );

    // --stats 同样因 interrupted 退出 1 并提示
    let output = zgraph(&["trace", RUN_ID, "--db", path_str(&fx.path), "--stats"]);
    assert_eq!(code(&output), 1, "{}", stderr(&output));
    assert!(
        stdout(&output).contains("!! trace truncated:"),
        "{}",
        stdout(&output)
    );
}

#[test]
fn db_path_priority_env_and_default() {
    let fx = fixture();
    let bin = env!("CARGO_BIN_EXE_zgraph");
    let missing = fx.dir().join("missing.db");

    // 只有 env ZGRAPH_DB
    let output = Command::new(bin)
        .args(["trace", RUN_ID])
        .env("ZGRAPH_DB", &fx.path)
        .output()
        .expect("runs");
    assert_eq!(code(&output), 0, "{}", stderr(&output));

    // --db 覆盖 env
    let output = Command::new(bin)
        .args(["trace", RUN_ID, "--db", path_str(&fx.path)])
        .env("ZGRAPH_DB", &missing)
        .output()
        .expect("runs");
    assert_eq!(code(&output), 0, "{}", stderr(&output));

    // 默认 <cwd>/.zgraph/trace.db
    let cwd = fx.dir().join("workspace");
    std::fs::create_dir_all(cwd.join(".zgraph")).expect("create .zgraph");
    std::fs::copy(&fx.path, cwd.join(".zgraph").join("trace.db")).expect("copy db");
    let output = Command::new(bin)
        .args(["trace", RUN_ID])
        .current_dir(&cwd)
        .env_remove("ZGRAPH_DB")
        .output()
        .expect("runs");
    assert_eq!(code(&output), 0, "{}", stderr(&output));
    assert!(stdout(&output).contains("status=Completed"));
}

// ---------------------------------------------------------------------------
// 06 §6.8.3：run still in progress (pid N)
// ---------------------------------------------------------------------------

/// 只发 `RunStarted`（无 `RunFinished`）的 in-progress 库。
fn in_progress_fixture(owner_pid: u32, owner_host: &str) -> (TempDir, PathBuf) {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("trace.db");
    let writer = TraceWriter::open(&path).expect("writer");
    writer.handle().send(TraceInput::RunStarted(RunStartedInfo {
        run_id: RUN_ID.to_string(),
        graph_hash: GRAPH_HASH.to_string(),
        graph_name: GRAPH_NAME.to_string(),
        dsl_json: "{}".to_string(),
        user_id: "local".to_string(),
        entry_inputs: json!({}),
        seed: 0,
        record_mode: RecordMode::Hash,
        owner_pid,
        owner_host: owner_host.to_string(),
        started_at: at(0),
    }));
    writer.flush().expect("flush");
    writer.close().expect("close");
    (dir, path)
}

#[test]
fn trace_reports_run_still_in_progress_for_live_owner() {
    let (_dir, path) = in_progress_fixture(std::process::id(), "test-host");
    let output = zgraph(&["trace", RUN_ID, "--db", path_str(&path)]);
    assert_eq!(code(&output), 0, "{}", stderr(&output));
    assert!(
        stdout(&output).contains("status=Running"),
        "{}",
        stdout(&output)
    );
    assert!(
        stderr(&output).contains(&format!(
            "run still in progress (pid {})",
            std::process::id()
        )),
        "{}",
        stderr(&output)
    );
    // --json：stdout 仍是纯 NDJSON，提示走 stderr。
    let output = zgraph(&["trace", RUN_ID, "--db", path_str(&path), "--json"]);
    assert_eq!(code(&output), 0, "{}", stderr(&output));
    assert!(json_lines(&output).len() == 1, "{}", stdout(&output));
    assert!(
        stderr(&output).contains("run still in progress (pid "),
        "{}",
        stderr(&output)
    );
}

#[cfg(unix)]
#[test]
fn trace_omits_progress_note_for_dead_owner_pid() {
    let mut child = std::process::Command::new("true")
        .spawn()
        .expect("spawn true");
    let dead_pid = child.id();
    child.wait().expect("wait true");
    let (_dir, path) = in_progress_fixture(dead_pid, "test-host");
    let output = zgraph(&["trace", RUN_ID, "--db", path_str(&path)]);
    assert_eq!(code(&output), 0, "{}", stderr(&output));
    assert!(
        !stderr(&output).contains("run still in progress"),
        "{}",
        stderr(&output)
    );
}
