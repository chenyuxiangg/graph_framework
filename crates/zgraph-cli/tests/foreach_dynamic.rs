//! W4b：`foreach` 展开的 `trace`/`replay` CLI 契约测试（DR-007 §8.4/§8.5）。
//!
//! 覆盖：
//! - `trace` 人类输出在事件（与 human 决策行）之后追加
//!   `foreach node='<F>' items=<N> keys=[...]` + `source_instance` 明细，按
//!   `(at, expansion_id)` 确定性顺序；`--json` 追加 `{"kind":"ForeachExpansion",...}`
//!   NDJSON 行（事件行形状不变）；无 foreach run 输出不变；`--stats` 不新增字段；
//!   `replay --mode=events --json` 与 `trace --json` 逐字节一致。
//! - `replay --mode=outputs` per-item 重放：L2 查找键
//!   `(node_id, attempt, item_index)`；用 `foreach_expansions` 的
//!   `item_count`/`keys` 校验（不匹配 → `REPLAY_RECORD_CORRUPT`；缺失 →
//!   `REPLAY_RECORD_MISSING`）；item 输出按记录重放、不启动 worker。
//!
//! 稳定性：每个用例独立 `TempDir`/DB；无网络；不使用 sleep 猜测（L2 全内存重跑）。

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::{json, Value};
use tempfile::TempDir;
use zgraph_trace::{
    DecisionStore, ForeachExpansionRecord, InstanceSnapshot, InstanceStatus, NewHumanDecision,
    RecordMode, RecordedOutput, RunFinishedInfo, RunMetrics, RunStartedInfo, RunStatus,
    TraceHandle, TraceInput, TraceWriter, TransitionRecord,
};

use zgraph_cli::run_cmd::output_hash;

/// 事件 + human 决策 + 2 条 foreach 展开的只读完成态库（trace 输出用）。
struct Fixture {
    _dir: TempDir,
    path: PathBuf,
}

impl Fixture {
    fn path(&self) -> &Path {
        &self.path
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

/// P-1 形状的 foreach 流水线图（region R={F,W,E}，E=E，J=end）。
///
/// `start → prep(list) → F(foreach) → W → E → end`；`prep` 输出数组作为 F 的
/// `items` 输入；join 把 E 的 per-item 输出数组交给 end。
const PIPELINE_GRAPH: &str = r#"{
  "version": "1.0",
  "metadata": {"name": "foreach-pipeline"},
  "entry_points": ["start"],
  "nodes": [
    {"id": "start", "type": "start", "inputs": [], "config": {}},
    {"id": "prep", "type": "python_node",
     "inputs": [{"name": "seed", "type": "any", "required": true}],
     "config": {"module": "workers.f", "function": "prep"}},
    {"id": "F", "type": "python_node",
     "inputs": [{"name": "items", "type": "any", "required": true}],
     "config": {"module": "workers.f", "function": "fanout"},
     "foreach": {"items_port": "items", "join": "end"}},
    {"id": "W", "type": "python_node",
     "inputs": [{"name": "task", "type": "any", "required": true}],
     "config": {"module": "workers.f", "function": "work"}},
    {"id": "E", "type": "python_node",
     "inputs": [{"name": "work", "type": "any", "required": true}],
     "config": {"module": "workers.f", "function": "exit"}},
    {"id": "end", "type": "end",
     "inputs": [{"name": "results", "type": "any", "required": true}],
     "config": {}}
  ],
  "edges": [
    {"from": "start", "to_node": "prep", "to_input_port": "seed"},
    {"from": "prep", "to_node": "F", "to_input_port": "items"},
    {"from": "F", "to_node": "W", "to_input_port": "task"},
    {"from": "W", "to_node": "E", "to_input_port": "work"},
    {"from": "E", "to_node": "end", "to_input_port": "results"}
  ]
}"#;

const TRACE_RUN: &str = "run-dyn-trace";
const GRAPH_HASH: &str = "0123456789abcdef";

fn run_started(
    run_id: &str,
    dsl_json: &str,
    entry_inputs: Value,
    record: RecordMode,
) -> TraceInput {
    TraceInput::RunStarted(RunStartedInfo {
        run_id: run_id.to_string(),
        graph_hash: GRAPH_HASH.to_string(),
        graph_name: "foreach-demo".to_string(),
        dsl_json: dsl_json.to_string(),
        user_id: "local".to_string(),
        entry_inputs,
        global_vars: json!({}),
        seed: 7,
        record_mode: record,
        owner_pid: std::process::id(),
        owner_host: "test-host".to_string(),
        started_at: "2026-11-01T10:00:00.000Z".to_string(),
    })
}

fn run_finished(run_id: &str, output: Value) -> TraceInput {
    TraceInput::RunFinished(RunFinishedInfo {
        run_id: run_id.to_string(),
        status: RunStatus::Completed,
        output_summary: Some(output),
        error: None,
        metrics: RunMetrics {
            streaming_dropped: 0,
            abandoned_tasks: 0,
            trace_backpressure: 0,
            killed_workers: 0,
        },
        at: "2026-11-01T10:00:09.000Z".to_string(),
    })
}

fn expansion(
    run_id: &str,
    expansion_id: &str,
    node: &str,
    count: u64,
    keys: Value,
    source: Option<&str>,
    at: &str,
) -> TraceInput {
    TraceInput::ForeachExpansion(ForeachExpansionRecord {
        run_id: run_id.to_string(),
        expansion_id: expansion_id.to_string(),
        node_id: node.to_string(),
        source_instance_id: source.map(str::to_string),
        item_count: count,
        keys: Some(keys),
        at: at.to_string(),
    })
}

/// 事件（RunStarted/Transition/RunFinished）+ human 决策 + 2 条展开的完成态库。
fn trace_fixture() -> Fixture {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("trace.db");
    let writer = TraceWriter::open(&path).expect("open trace writer");
    let handle = writer.handle();
    handle.send(run_started(
        TRACE_RUN,
        "{}",
        json!({"start": {}}),
        RecordMode::Hash,
    ));
    // 一条事件行（Transition），保证「事件行在前」的顺序可断言。
    let ts = "2026-11-01T10:00:01.000Z".to_string();
    handle.send(TraceInput::Transition {
        record: TransitionRecord {
            run_id: TRACE_RUN.to_string(),
            instance_id: "inst-start".to_string(),
            node_id: "start".to_string(),
            attempt: 0,
            from: InstanceStatus::Pending,
            to: InstanceStatus::Completed,
            reason: Some("completed".to_string()),
            ts: ts.clone(),
        },
        snapshot: InstanceSnapshot {
            run_id: TRACE_RUN.to_string(),
            instance_id: "inst-start".to_string(),
            node_id: "start".to_string(),
            node_type: "start".to_string(),
            attempt: 0,
            item_index: 0,
            item_key: None,
            status: InstanceStatus::Completed,
            input: Some(json!({})),
            output: Some(json!({})),
            error: None,
            error_code: None,
            created_at: ts.clone(),
            started_at: Some(ts.clone()),
            finished_at: Some(ts),
        },
    });
    // 故意先写较晚的展开，验证按 (at, expansion_id) 排序。
    handle.send(expansion(
        TRACE_RUN,
        "0192f0d0-0000-7000-8000-0000000000b2",
        "F",
        2,
        json!([{"$index": 0}, {"$index": 1}]),
        Some("inst-prep"),
        "2026-11-01T10:00:03.000Z",
    ));
    handle.send(expansion(
        TRACE_RUN,
        "0192f0d0-0000-7000-8000-0000000000a1",
        "G",
        1,
        json!(["k1"]),
        None,
        "2026-11-01T10:00:02.000Z",
    ));
    handle.send(run_finished(TRACE_RUN, json!({"result": "ok"})));
    writer.flush().expect("flush");
    writer.close().expect("close");

    // human 决策行（实例已存在）：用于断言 NDJSON 顺序 events → HumanDecision →
    // ForeachExpansion（三种追加行互不交错）。
    let mut store = DecisionStore::open(&path).expect("decision store");
    store
        .insert(NewHumanDecision {
            run_id: TRACE_RUN.to_string(),
            instance_id: "inst-start".to_string(),
            decision: "approved".to_string(),
            approver: "alice".to_string(),
            reason: None,
            decision_id: "0192f0d0-0000-7000-8000-0000000000c3".to_string(),
            decided_at: "2026-11-01T10:00:04.000Z".to_string(),
        })
        .expect("insert decision");
    Fixture { _dir: dir, path }
}

// ---------------------------------------------------------------------------
// trace 输出（DR-007 §8.4）
// ---------------------------------------------------------------------------

#[test]
fn human_trace_appends_foreach_summary_after_events_and_decisions() {
    let fx = trace_fixture();
    let output = zgraph(&["trace", TRACE_RUN, "--db", path_str(fx.path())]);
    assert_eq!(code(&output), 0, "stderr={}", stderr(&output));
    let text = stdout(&output);

    assert!(
        text.contains("foreach node='G' items=1 keys=[\"k1\"]"),
        "{text}"
    );
    assert!(
        text.contains("foreach node='F' items=2 keys=[{\"$index\":0},{\"$index\":1}]"),
        "{text}"
    );
    assert!(text.contains("source_instance=inst-prep"), "{text}");
    assert!(
        text.contains("source_instance=-"),
        "entry/无来源时显示 '-': {text}"
    );
    // 顺序：事件表格 → human 决策摘要 → foreach 摘要（按 at 升序：G 先于 F）。
    let events_end = text.find("status=Completed").expect("RunFinished row");
    let decisions = text.find("human_decisions: 1").expect("decision summary");
    let first_foreach = text.find("foreach node='G'").expect("G expansion");
    let second_foreach = text.find("foreach node='F'").expect("F expansion");
    assert!(events_end < decisions, "{text}");
    assert!(decisions < first_foreach, "{text}");
    assert!(
        first_foreach < second_foreach,
        "ordered by (at, expansion_id)"
    );
}

#[test]
fn json_trace_appends_foreach_expansion_lines_after_events_and_decisions() {
    let fx = trace_fixture();
    let output = zgraph(&["trace", TRACE_RUN, "--db", path_str(fx.path()), "--json"]);
    assert_eq!(code(&output), 0, "stderr={}", stderr(&output));
    let lines = json_lines(&output);
    let kinds: Vec<&str> = lines
        .iter()
        .map(|line| line["kind"].as_str().expect("kind"))
        .collect();
    assert_eq!(
        kinds,
        vec![
            "RunStarted",
            "Transition",
            "RunFinished",
            "HumanDecision",
            "ForeachExpansion",
            "ForeachExpansion",
        ],
        "既有事件行形状不变，追加行顺序 events→decision→expansion"
    );

    let g = &lines[4];
    assert_eq!(g["run_id"], json!(TRACE_RUN));
    assert_eq!(
        g["expansion_id"],
        json!("0192f0d0-0000-7000-8000-0000000000a1")
    );
    assert_eq!(g["node_id"], json!("G"));
    assert_eq!(g["item_count"], json!(1));
    assert_eq!(g["keys"], json!(["k1"]));
    assert_eq!(g["source_instance_id"], json!(null));
    assert_eq!(g["at"], json!("2026-11-01T10:00:02.000Z"));

    let f = &lines[5];
    assert_eq!(f["node_id"], json!("F"));
    assert_eq!(f["item_count"], json!(2));
    assert_eq!(f["keys"], json!([{"$index": 0}, {"$index": 1}]));
    assert_eq!(f["source_instance_id"], json!("inst-prep"));
}

#[test]
fn stats_and_events_mode_replay_are_unchanged_by_foreach_rows() {
    let fx = trace_fixture();
    let db = path_str(fx.path());
    // --stats 不新增 foreach 字段（DR-007 §8.4）。
    let stats = zgraph(&["trace", TRACE_RUN, "--db", db, "--stats"]);
    assert_eq!(code(&stats), 0, "{}", stderr(&stats));
    assert!(!stdout(&stats).contains("foreach"), "{}", stdout(&stats));

    // L1：replay --mode=events --json 与 trace --json 逐字节一致（含 expansion 行）。
    let trace = zgraph(&["trace", TRACE_RUN, "--db", db, "--json"]);
    let replay = zgraph(&[
        "replay",
        "--run",
        TRACE_RUN,
        "--mode=events",
        "--json",
        "--db",
        db,
    ]);
    assert_eq!(code(&replay), 0, "{}", stderr(&replay));
    assert_eq!(stdout(&trace), stdout(&replay));
    assert!(stdout(&trace).contains("\"kind\":\"ForeachExpansion\""));
}

// ---------------------------------------------------------------------------
// L2 per-item 重放（DR-007 §8.5）
// ---------------------------------------------------------------------------

/// 一次 L2 重放 fixture：`--record=full` 的 2-item foreach 完成态 run。
struct L2Fixture {
    _dir: TempDir,
    path: PathBuf,
    run_id: String,
    expected_output: Value,
}

impl L2Fixture {
    fn db(&self) -> &Path {
        &self.path
    }
}

/// 记录一个 `(instance, node, attempt, item_index?, key?)` 输出 + instance 快照。
struct RecordSpec {
    instance: &'static str,
    node: &'static str,
    item_index: u32,
    item_key: Option<&'static str>,
    output: Value,
}

fn l2_fixture() -> L2Fixture {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("trace.db");
    let writer = TraceWriter::open(&path).expect("writer");
    let handle: TraceHandle = writer.handle();
    let run_id = "run-foreach-l2".to_string();
    handle.send(run_started(
        &run_id,
        PIPELINE_GRAPH,
        json!({"start": {}}),
        RecordMode::Full,
    ));

    let specs = vec![
        RecordSpec {
            instance: "inst-prep",
            node: "prep",
            item_index: 0,
            item_key: None,
            output: json!([{"id": "a"}, {"id": "b"}]),
        },
        RecordSpec {
            instance: "inst-F-0",
            node: "F",
            item_index: 0,
            item_key: None,
            output: json!({"i": 0}),
        },
        RecordSpec {
            instance: "inst-F-1",
            node: "F",
            item_index: 1,
            item_key: None,
            output: json!({"i": 1}),
        },
        RecordSpec {
            instance: "inst-W-0",
            node: "W",
            item_index: 0,
            item_key: None,
            output: json!({"w": 0}),
        },
        RecordSpec {
            instance: "inst-W-1",
            node: "W",
            item_index: 1,
            item_key: None,
            output: json!({"w": 1}),
        },
        RecordSpec {
            instance: "inst-E-0",
            node: "E",
            item_index: 0,
            item_key: None,
            output: json!({"e": 0}),
        },
        RecordSpec {
            instance: "inst-E-1",
            node: "E",
            item_index: 1,
            item_key: None,
            output: json!({"e": 1}),
        },
    ];
    for spec in &specs {
        let ts = "2026-11-01T10:00:02.000Z".to_string();
        handle.send(TraceInput::InstanceSnapshot(InstanceSnapshot {
            run_id: run_id.clone(),
            instance_id: spec.instance.to_string(),
            node_id: spec.node.to_string(),
            node_type: "python_node".to_string(),
            attempt: 0,
            item_index: spec.item_index,
            item_key: spec.item_key.map(str::to_string),
            status: InstanceStatus::Completed,
            input: None,
            output: Some(spec.output.clone()),
            error: None,
            error_code: None,
            created_at: ts.clone(),
            started_at: Some(ts.clone()),
            finished_at: Some(ts),
        }));
        handle.send(TraceInput::RecordedOutput(RecordedOutput {
            run_id: run_id.clone(),
            instance_id: spec.instance.to_string(),
            node_id: spec.node.to_string(),
            attempt: 0,
            output_hash: output_hash(&spec.output),
            output_json: Some(spec.output.clone()),
            recorded_at: "2026-11-01T10:00:02.500Z".to_string(),
        }));
    }
    handle.send(expansion(
        &run_id,
        "0192f0d0-0000-7000-8000-0000000000f1",
        "F",
        2,
        json!([{"$index": 0}, {"$index": 1}]),
        Some("inst-prep"),
        "2026-11-01T10:00:03.000Z",
    ));
    let expected_output = json!({"results": [{"e": 0}, {"e": 1}]});
    handle.send(run_finished(&run_id, expected_output.clone()));
    writer.flush().expect("flush");
    writer.close().expect("close");
    L2Fixture {
        _dir: dir,
        path,
        run_id,
        expected_output,
    }
}

fn replay_outputs(fx: &L2Fixture) -> Output {
    zgraph(&[
        "replay",
        "--run",
        &fx.run_id,
        "--mode=outputs",
        "--json",
        "--db",
        path_str(&fx.path),
    ])
}

/// `--record=full` 的 foreach run：per-item 记录 + expansion 一致 → L2 成功且
/// 重放输出 == 原始（内核重新展开、按 item_index 取记录，不启动 worker）。
#[test]
fn l2_replay_foreach_matches_per_item_recorded_outputs() {
    let fx = l2_fixture();
    let output = replay_outputs(&fx);
    assert_eq!(
        code(&output),
        0,
        "stdout={} stderr={}",
        stdout(&output),
        stderr(&output)
    );
    let value: Value = serde_json::from_str(stdout(&output).trim()).expect("replay json");
    assert_eq!(value["ok"], json!(true));
    assert_eq!(value["matched"], json!(true));
    assert_eq!(value["original_output"], fx.expected_output);
    assert_eq!(value["replayed_output"], fx.expected_output);
}

/// expansion `item_count` 与 item 实例不一致 → `REPLAY_RECORD_CORRUPT` exit 3。
#[test]
fn l2_replay_foreach_item_count_mismatch_is_corrupt() {
    let fx = l2_fixture();
    rusqlite_connection(fx.db())
        .execute(
            "UPDATE foreach_expansions SET item_count = 1 WHERE run_id = ?1",
            [&fx.run_id],
        )
        .expect("tamper item_count");
    let output = replay_outputs(&fx);
    assert_eq!(code(&output), 3, "stderr={}", stderr(&output));
    assert!(
        stderr(&output).contains("REPLAY_RECORD_CORRUPT"),
        "{}",
        stderr(&output)
    );
}

/// expansion `keys` 与实例 `item_key` 不一致 → `REPLAY_RECORD_CORRUPT` exit 3。
#[test]
fn l2_replay_foreach_keys_mismatch_is_corrupt() {
    let fx = l2_fixture();
    rusqlite_connection(fx.db())
        .execute(
            "UPDATE foreach_expansions SET keys_json = ?2 WHERE run_id = ?1",
            rusqlite::params![fx.run_id, "[\"x\",\"y\"]"],
        )
        .expect("tamper keys");
    let output = replay_outputs(&fx);
    assert_eq!(code(&output), 3, "stderr={}", stderr(&output));
    assert!(
        stderr(&output).contains("REPLAY_RECORD_CORRUPT"),
        "{}",
        stderr(&output)
    );
}

/// region 已有 item 实例但 expansion 行缺失 → `REPLAY_RECORD_MISSING` exit 3。
#[test]
fn l2_replay_foreach_missing_expansion_is_missing_record() {
    let fx = l2_fixture();
    rusqlite_connection(fx.db())
        .execute(
            "DELETE FROM foreach_expansions WHERE run_id = ?1",
            [&fx.run_id],
        )
        .expect("delete expansion");
    let output = replay_outputs(&fx);
    assert_eq!(code(&output), 3, "stderr={}", stderr(&output));
    assert!(
        stderr(&output).contains("REPLAY_RECORD_MISSING"),
        "{}",
        stderr(&output)
    );
}

/// 删除某 item 的 recorded_outputs 行 → `REPLAY_RECORD_MISSING` exit 3。
#[test]
fn l2_replay_foreach_missing_item_record_is_missing_record() {
    let fx = l2_fixture();
    rusqlite_connection(fx.db())
        .execute(
            "DELETE FROM recorded_outputs WHERE run_id = ?1 AND instance_id = 'inst-F-1'",
            [&fx.run_id],
        )
        .expect("delete per-item record");
    let output = replay_outputs(&fx);
    assert_eq!(code(&output), 3, "stderr={}", stderr(&output));
    assert!(
        stderr(&output).contains("REPLAY_RECORD_MISSING"),
        "{}",
        stderr(&output)
    );
}

/// 篡改某 item 记录的输出 hash → `REPLAY_RECORD_CORRUPT` exit 3（per-item 复核）。
#[test]
fn l2_replay_foreach_item_record_hash_mismatch_is_corrupt() {
    let fx = l2_fixture();
    rusqlite_connection(fx.db())
        .execute(
            "UPDATE recorded_outputs SET output_hash = 'bad' \
             WHERE run_id = ?1 AND instance_id = 'inst-E-1'",
            [&fx.run_id],
        )
        .expect("tamper per-item hash");
    let output = replay_outputs(&fx);
    assert_eq!(code(&output), 3, "stderr={}", stderr(&output));
    assert!(
        stderr(&output).contains("REPLAY_RECORD_CORRUPT"),
        "{}",
        stderr(&output)
    );
}

fn rusqlite_connection(path: &Path) -> rusqlite::Connection {
    rusqlite::Connection::open(path).expect("open db")
}
