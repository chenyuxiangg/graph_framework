//! `zgraph run` + `zgraph decide` 真链路集成测试（W3a / DR-006 §7.3；AC-16 a/f/g 的 CLI 层）。
//!
//! 覆盖：
//! - human 图真 run（真 kernel + 真 SQLite trace；human 节点不启 worker）在输入就绪后
//!   进入 `WaitingHuman`、run 进程保持存活、**不产生 worker**；
//! - 外部 `zgraph decide` 后 run 自动接续 Completed、实例 output 携带 decision、
//!   决策行 `consumed_at` 非空、stderr 有实时决策行、`trace` 可见决策摘要；
//! - rejected 同样正常 Completed（不隐式 Failed）；
//! - `-q/--quiet` 抑制决策行；
//! - human 图 + `--db :memory:` 启动前预检拒绝（`RUN_ENTRY_INVALID` exit 3、
//!   reason `trace_required_for_human`、不产生 run/不打印 run id）；
//! - run 等待期间同实例并发 decide：恰一 exit 0、其余 exit 5、恰一行决策。
//!
//! 稳定性约定（AGENTS §8）：每个用例独立 `TempDir`/DB；等待用「轮询 DB 直到可见」
//! 而非 sleep 猜测；子进程有超时兜底并被 kill（不残留进程）。

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use rusqlite::{Connection, OptionalExtension};
use serde_json::{json, Value};
use tempfile::TempDir;

/// 等待上限（真链路冷启动 + 决策消费轮询均为毫秒级，留足余量）。
const TIMEOUT: Duration = Duration::from_secs(20);

/// 轮询间隔（等待 DB 可见）。
const POLL: Duration = Duration::from_millis(20);

/// 单人审批图：`start -> review(builtin.human) -> end`（无 Python worker）。
///
/// 不分支 approve/reject：两种决策都落到 end（拒绝去向由图形状决定；本用例只验证
/// 「拒绝也是正常 Completed」）。
const HUMAN_GRAPH: &str = r#"{
  "version": "1.0",
  "metadata": {"name": "cli-human-decide"},
  "entry_points": ["start"],
  "nodes": [
    {"id": "start", "type": "start", "inputs": [], "config": {}},
    {"id": "review", "type": "python_node",
     "inputs": [{"name": "payload", "type": "any", "required": true}],
     "config": {"module": "builtin", "function": "human"}},
    {"id": "end", "type": "end",
     "inputs": [{"name": "r", "type": "any", "required": true}], "config": {}}
  ],
  "edges": [
    {"from": "start", "to_node": "review", "to_input_port": "payload"},
    {"from": "review", "to_node": "end", "to_input_port": "r"}
  ]
}"#;

/// 无 human 节点的普通图（用于 `:memory:` 不拒绝的对照）。
const PLAIN_GRAPH: &str = r#"{
  "version": "1.0",
  "metadata": {"name": "cli-plain"},
  "entry_points": ["start"],
  "nodes": [
    {"id": "start", "type": "start", "inputs": [], "config": {}},
    {"id": "end", "type": "end",
     "inputs": [{"name": "r", "type": "any", "required": true}], "config": {}}
  ],
  "edges": [{"from": "start", "to_node": "end", "to_input_port": "r"}]
}"#;

/// 一次测试的独立目录（图 + trace DB）。
struct Fixture {
    _dir: TempDir,
    graph: PathBuf,
    db: PathBuf,
}

impl Fixture {
    fn new(graph_text: &str) -> Self {
        let dir = tempfile::tempdir().expect("tempdir");
        let graph = dir.path().join("human_gate.json");
        std::fs::write(&graph, graph_text).expect("write graph");
        let db = dir.path().join("trace.db");
        Self {
            _dir: dir,
            graph,
            db,
        }
    }

    /// `zgraph run` 参数（默认 human 模式）。
    fn run_args(&self) -> Vec<String> {
        vec![
            "run".to_string(),
            self.graph.display().to_string(),
            "--inputs".to_string(),
            "{}".to_string(),
            "--db".to_string(),
            self.db.display().to_string(),
        ]
    }

    /// `zgraph run ... --db :memory:`。
    fn memory_run_args(&self) -> Vec<String> {
        let mut args = self.run_args();
        let at = args.iter().position(|arg| arg == "--db").expect("--db");
        args[at + 1] = ":memory:".to_string();
        args
    }

    /// `zgraph decide` 参数。
    fn decide_args(&self, run_id: &str, instance: &str, decision: &str) -> Vec<String> {
        vec![
            "decide".to_string(),
            run_id.to_string(),
            "--instance".to_string(),
            instance.to_string(),
            "--decision".to_string(),
            decision.to_string(),
            "--approver".to_string(),
            "alice".to_string(),
            "--db".to_string(),
            self.db.display().to_string(),
        ]
    }
}

/// 运行中的 CLI 子进程：stdout/stderr 由独立线程读取，`wait` 有超时兜底。
struct RunningCli {
    child: Option<Child>,
    stdout: Option<JoinHandle<Vec<u8>>>,
    stderr: Option<JoinHandle<Vec<u8>>>,
}

impl RunningCli {
    fn spawn(args: &[String]) -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_zgraph"))
            .args(args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn zgraph");
        let stdout = child.stdout.take().map(read_thread);
        let stderr = child.stderr.take().map(read_thread);
        Self {
            child: Some(child),
            stdout,
            stderr,
        }
    }

    fn is_running(&mut self) -> bool {
        self.child
            .as_mut()
            .expect("child")
            .try_wait()
            .expect("try_wait")
            .is_none()
    }

    /// 等待退出（超时则 kill + panic；输出完整收集）。
    fn wait(mut self) -> CliResult {
        let deadline = Instant::now() + TIMEOUT;
        loop {
            let status = self
                .child
                .as_mut()
                .expect("child")
                .try_wait()
                .expect("try_wait");
            if let Some(status) = status {
                self.child.take();
                return CliResult {
                    code: status.code().unwrap_or(-1),
                    stdout: join_text(self.stdout.take()),
                    stderr: join_text(self.stderr.take()),
                };
            }
            if Instant::now() > deadline {
                let _ = self.child.as_mut().expect("child").kill();
                let _ = self.child.as_mut().expect("child").wait();
                panic!("zgraph process did not exit within {TIMEOUT:?}");
            }
            std::thread::sleep(POLL);
        }
    }
}

impl Drop for RunningCli {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

/// 收集 stdin 管道直到 EOF 的读取线程。
fn read_thread(mut pipe: impl Read + Send + 'static) -> JoinHandle<Vec<u8>> {
    std::thread::spawn(move || {
        let mut buffer = Vec::new();
        let _ = pipe.read_to_end(&mut buffer);
        buffer
    })
}

fn join_text(handle: Option<JoinHandle<Vec<u8>>>) -> String {
    handle
        .map(|handle| handle.join().expect("reader thread"))
        .map(|bytes| String::from_utf8_lossy(&bytes).to_string())
        .unwrap_or_default()
}

/// 子进程结果（与 `std::process::Output` 等价的最小子集）。
struct CliResult {
    code: i32,
    stdout: String,
    stderr: String,
}

fn zgraph(args: &[String]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_zgraph"))
        .args(args)
        .output()
        .expect("zgraph binary runs")
}

fn path_str(path: &Path) -> &str {
    path.to_str().expect("utf8 path")
}

/// 轮询 DB 直到 human 实例可见 `WaitingHuman`（N11：可见后再 decide）。
fn wait_for_waiting_human(db: &Path) -> (String, String) {
    let deadline = Instant::now() + TIMEOUT;
    loop {
        if let Some(row) = waiting_human(db) {
            return row;
        }
        assert!(
            Instant::now() < deadline,
            "instance 'review' did not reach WaitingHuman; db={}",
            db.display()
        );
        std::thread::sleep(POLL);
    }
}

/// 一次查询：`(run_id, instance_id)`；不可见时返回 `None`。
fn waiting_human(db: &Path) -> Option<(String, String)> {
    let conn = Connection::open(db).ok()?;
    conn.query_row(
        "SELECT run_id, instance_id FROM instances \
         WHERE node_id = 'review' AND status = 'WaitingHuman' LIMIT 1",
        [],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )
    .optional()
    .ok()
    .flatten()
}

/// `runs.status`。
fn run_status(db: &Path, run_id: &str) -> Option<String> {
    Connection::open(db)
        .ok()?
        .query_row(
            "SELECT status FROM runs WHERE run_id = ?1",
            [run_id],
            |row| row.get(0),
        )
        .optional()
        .ok()
        .flatten()
}

/// 查询决策行：`(decision, approver, reason, consumed_at)`。
fn decision_rows(db: &Path, run_id: &str) -> Vec<(String, String, Option<String>, Option<String>)> {
    let conn = Connection::open(db).expect("open db");
    let mut stmt = conn
        .prepare(
            "SELECT decision, approver, reason, consumed_at FROM human_decisions \
             WHERE run_id = ?1 ORDER BY decided_at, decision_id",
        )
        .expect("prepare");
    stmt.query_map([run_id], |row| {
        Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
    })
    .expect("query")
    .collect::<Result<Vec<_>, _>>()
    .expect("collect")
}

/// 等待决策行被 consumer 标记 `consumed_at`（run 可能刚退出，标记在退出前进已提交）。
fn wait_for_consumed(
    db: &Path,
    run_id: &str,
) -> Vec<(String, String, Option<String>, Option<String>)> {
    let deadline = Instant::now() + TIMEOUT;
    loop {
        let rows = decision_rows(db, run_id);
        if rows.iter().any(|row| row.3.is_some()) {
            return rows;
        }
        assert!(
            Instant::now() < deadline,
            "human decision was not marked consumed; rows={rows:?}"
        );
        std::thread::sleep(POLL);
    }
}

// ---------------------------------------------------------------------------
// 真链路：等待 → decide → 自动接续
// ---------------------------------------------------------------------------

#[test]
fn run_waits_human_then_approve_applies_and_marks_consumed() {
    let fx = Fixture::new(HUMAN_GRAPH);
    let mut run = RunningCli::spawn(&fx.run_args());
    let (run_id, instance_id) = wait_for_waiting_human(&fx.db);
    assert!(
        run.is_running(),
        "run must stay alive while an instance is WaitingHuman (DR-006 §4.6)"
    );

    let decide = zgraph(&fx.decide_args(&run_id, &instance_id, "approved"));
    assert_eq!(
        decide.status.code(),
        Some(0),
        "decide stdout={} stderr={}",
        String::from_utf8_lossy(&decide.stdout),
        String::from_utf8_lossy(&decide.stderr)
    );

    let result = run.wait();
    assert_eq!(result.code, 0, "run stderr={}", result.stderr);
    assert!(
        result.stderr.contains("human decision applied: instance="),
        "stderr must carry the realtime decision line: {}",
        result.stderr
    );
    assert!(
        result.stdout.contains("approved"),
        "run output must carry the decision: {}",
        result.stdout
    );

    // DB：run Completed、实例 output 携带 decision、决策行已消费、WaitingHuman 迁移可见。
    assert_eq!(
        run_status(&fx.db, &run_id).as_deref(),
        Some("Completed"),
        "reject/approve 都是正常 Completed"
    );
    let rows = wait_for_consumed(&fx.db, &run_id);
    assert_eq!(rows.len(), 1, "exactly one decision row: {rows:?}");
    assert_eq!(rows[0].0, "approved");
    assert_eq!(rows[0].1, "alice");
    assert_eq!(rows[0].2, None, "--reason not provided → NULL");
    assert!(
        rows[0].3.is_some(),
        "consumed_at must be written by run consumer"
    );
    let conn = Connection::open(&fx.db).expect("open db");
    let waiting: String = conn
        .query_row(
            "SELECT status FROM instances WHERE instance_id = ?1",
            [&instance_id],
            |row| row.get(0),
        )
        .expect("review instance");
    assert_eq!(waiting, "Completed");
    // run 进程退出后二次 decide 恒 HUMAN_DECISION_EXISTS（预检①先于 run 状态）。
    let second = zgraph(&fx.decide_args(&run_id, &instance_id, "rejected"));
    assert_eq!(
        second.status.code(),
        Some(5),
        "{}",
        String::from_utf8_lossy(&second.stderr)
    );

    // trace 输出：决策摘要可见且无 pending。
    let trace = zgraph(&[
        "trace".to_string(),
        run_id.clone(),
        "--db".to_string(),
        path_str(&fx.db).to_string(),
    ]);
    assert_eq!(trace.status.code(), Some(0));
    let text = String::from_utf8_lossy(&trace.stdout).to_string();
    assert!(
        text.contains("human_decisions: 1 (approved 1 / rejected 0; pending 0)"),
        "{text}"
    );
}

#[test]
fn rejected_decision_is_also_completed() {
    let fx = Fixture::new(HUMAN_GRAPH);
    let mut run = RunningCli::spawn(&fx.run_args());
    let (run_id, instance_id) = wait_for_waiting_human(&fx.db);
    assert!(run.is_running());
    let mut args = fx.decide_args(&run_id, &instance_id, "rejected");
    let reason_at = args.len();
    args.push("--reason".to_string());
    args.insert(reason_at + 1, "needs rework".to_string());
    let decide = zgraph(&args);
    assert_eq!(
        decide.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&decide.stderr)
    );
    let result = run.wait();
    assert_eq!(result.code, 0, "{}", result.stderr);
    assert!(
        result.stderr.contains("decision=rejected"),
        "{}",
        result.stderr
    );
    assert_eq!(run_status(&fx.db, &run_id).as_deref(), Some("Completed"));
    let rows = wait_for_consumed(&fx.db, &run_id);
    assert_eq!(rows[0].0, "rejected");
    assert_eq!(rows[0].2.as_deref(), Some("needs rework"));
}

#[test]
fn quiet_suppresses_realtime_decision_line() {
    let fx = Fixture::new(HUMAN_GRAPH);
    let mut args = fx.run_args();
    args.push("-q".to_string());
    let mut run = RunningCli::spawn(&args);
    let (run_id, instance_id) = wait_for_waiting_human(&fx.db);
    assert!(run.is_running());
    let decide = zgraph(&fx.decide_args(&run_id, &instance_id, "approved"));
    assert_eq!(decide.status.code(), Some(0));
    let result = run.wait();
    assert_eq!(result.code, 0, "{}", result.stderr);
    assert!(
        !result.stderr.contains("human decision applied"),
        "-q must suppress decision lines: {}",
        result.stderr
    );
    wait_for_consumed(&fx.db, &run_id);
}

// ---------------------------------------------------------------------------
// applied=false：迟到/过期决策的幂等确认（DR-006 §4.4 行 3 / §5.3）
// ---------------------------------------------------------------------------

/// 直接向 DB 插一行「过期」决策（模拟迟到/重复/崩溃遗留；decide 预检不会允许）。
fn insert_stale_decision(db: &Path, run_id: &str, instance_id: &str) {
    Connection::open(db)
        .expect("open db")
        .execute(
            "INSERT INTO human_decisions \
             (run_id, instance_id, decision, approver, reason, decision_id, decided_at, consumed_at) \
             VALUES (?1, ?2, 'approved', 'stale', NULL, ?3, ?4, NULL)",
            rusqlite::params![
                run_id,
                instance_id,
                "0192f0d0-0000-7000-8000-00000000dead",
                "2026-10-31T10:00:00.000Z"
            ],
        )
        .expect("insert stale decision");
}

#[test]
fn stale_decision_is_consumed_without_changing_state() {
    let fx = Fixture::new(HUMAN_GRAPH);
    let mut run = RunningCli::spawn(&fx.run_args());
    let (run_id, review_instance) = wait_for_waiting_human(&fx.db);
    // `start` 实例早已 Completed：对它的决策是「迟到」决策（非等待态）。
    let start_instance: String = Connection::open(&fx.db)
        .expect("open db")
        .query_row(
            "SELECT instance_id FROM instances WHERE run_id = ?1 AND node_id = 'start'",
            [&run_id],
            |row| row.get(0),
        )
        .expect("start instance");
    insert_stale_decision(&fx.db, &run_id, &start_instance);
    // consumer 一律提交 → kernel 幂等校验 applied=false → 仍标 consumed（不悬挂）。
    let deadline = Instant::now() + TIMEOUT;
    loop {
        let rows = decision_rows(&fx.db, &run_id);
        if rows.iter().any(|row| row.3.is_some()) {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "stale decision was not consumed: {rows:?}"
        );
        std::thread::sleep(POLL);
    }
    // run 仍在等 review 的真实决策；迟到决策不改变 run/实例状态。
    assert!(run.is_running(), "stale decision must not finish the run");
    let decide = zgraph(&fx.decide_args(&run_id, &review_instance, "approved"));
    assert_eq!(decide.status.code(), Some(0));
    let result = run.wait();
    assert_eq!(result.code, 0, "{}", result.stderr);
    assert!(
        result
            .stderr
            .contains("warning: human decision not applied: instance="),
        "{}",
        result.stderr
    );
    assert!(
        result.stderr.contains("reject_reason=not_waiting_human"),
        "{}",
        result.stderr
    );
    let start_status: String = Connection::open(&fx.db)
        .expect("open db")
        .query_row(
            "SELECT status FROM instances WHERE instance_id = ?1",
            [&start_instance],
            |row| row.get(0),
        )
        .expect("start instance status");
    assert_eq!(
        start_status, "Completed",
        "stale decision must not change state"
    );
    assert_eq!(decision_rows(&fx.db, &run_id).len(), 2);
}

// ---------------------------------------------------------------------------
// 并发（AC-16 f 的 CLI 层）
// ---------------------------------------------------------------------------

#[test]
fn concurrent_decides_while_run_waits_exactly_one_succeeds() {
    let fx = Fixture::new(HUMAN_GRAPH);
    let mut run = RunningCli::spawn(&fx.run_args());
    let (run_id, instance_id) = wait_for_waiting_human(&fx.db);
    assert!(run.is_running());

    const PARALLEL: usize = 4;
    let children: Vec<Child> = (0..PARALLEL)
        .map(|_| {
            Command::new(env!("CARGO_BIN_EXE_zgraph"))
                .args(fx.decide_args(&run_id, &instance_id, "approved"))
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .expect("spawn decide")
        })
        .collect();
    let codes: Vec<i32> = children
        .into_iter()
        .map(|mut child| child.wait().expect("wait decide").code().unwrap_or(-1))
        .collect();
    assert_eq!(
        codes.iter().filter(|code| **code == 0).count(),
        1,
        "exactly one concurrent decide succeeds, got {codes:?}"
    );
    assert!(
        codes.iter().all(|code| matches!(code, 0 | 5)),
        "only 0/5 allowed, got {codes:?}"
    );

    let result = run.wait();
    assert_eq!(result.code, 0, "run stderr={}", result.stderr);
    let rows = wait_for_consumed(&fx.db, &run_id);
    assert_eq!(rows.len(), 1, "unique key keeps exactly one row: {rows:?}");
    assert!(rows[0].3.is_some());
    assert_eq!(run_status(&fx.db, &run_id).as_deref(), Some("Completed"));
}

// ---------------------------------------------------------------------------
// `:memory:` 预检（DR-006 §7.3 #7 / N10）
// ---------------------------------------------------------------------------

#[test]
fn human_graph_rejects_memory_db_before_starting_run() {
    let fx = Fixture::new(HUMAN_GRAPH);
    let output = zgraph(&fx.memory_run_args());
    assert_eq!(
        output.status.code(),
        Some(3),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    assert!(stderr.contains("RUN_ENTRY_INVALID"), "{stderr}");
    assert!(stderr.contains("trace_required_for_human"), "{stderr}");
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    assert!(
        stdout.is_empty(),
        "no run output for rejected entry: {stdout}"
    );
    assert!(
        !stderr.contains(" created "),
        "must not print a run id / created line: {stderr}"
    );
}

#[test]
fn plain_graph_still_accepts_memory_db() {
    // 对照：无 human 节点的图行为不变（`:memory:` 仍接受，06 §6.9.1）。
    let fx = Fixture::new(PLAIN_GRAPH);
    let output = zgraph(&fx.memory_run_args());
    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stderr).contains(":memory:"));
}

// ---------------------------------------------------------------------------
// W3a L2：human 实例 recorded_outputs 补录（DR-006 §2.2/§8 L2）
// ---------------------------------------------------------------------------

/// `recorded_outputs` 行：`(node_id, attempt, output_hash, output_json)`。
type RecordedRow = (String, u32, String, Option<Value>);

fn recorded_outputs(db: &Path, run_id: &str) -> Vec<RecordedRow> {
    let conn = Connection::open(db).expect("open db");
    let mut stmt = conn
        .prepare(
            "SELECT node_id, attempt, output_hash, output_json FROM recorded_outputs \
             WHERE run_id = ?1 ORDER BY node_id, attempt",
        )
        .expect("prepare");
    stmt.query_map([run_id], |row| {
        let output_json: Option<String> = row.get(3)?;
        Ok((
            row.get(0)?,
            row.get(1)?,
            row.get(2)?,
            output_json.map(|text| serde_json::from_str(&text).expect("valid output_json")),
        ))
    })
    .expect("query")
    .collect::<Result<Vec<_>, _>>()
    .expect("collect")
}

fn runs_count(db: &Path) -> i64 {
    Connection::open(db)
        .expect("open db")
        .query_row("SELECT count(*) FROM runs", [], |row| row.get(0))
        .expect("runs count")
}

/// `--record=full` 跑 human 图：human 实例补录（output_json/hash 与 `output_hash`
/// 口径一致），且 `replay --mode=outputs` 成功、重放输出 == 原始（不重新要求决策）。
#[test]
fn human_full_record_supports_l2_outputs_replay() {
    let fx = Fixture::new(HUMAN_GRAPH);
    let mut args = fx.run_args();
    args.push("--record=full".to_string());
    let mut run = RunningCli::spawn(&args);
    let (run_id, instance_id) = wait_for_waiting_human(&fx.db);
    assert!(run.is_running());
    let decide = zgraph(&fx.decide_args(&run_id, &instance_id, "approved"));
    assert_eq!(decide.status.code(), Some(0));
    let result = run.wait();
    assert_eq!(result.code, 0, "run stderr={}", result.stderr);

    let expected = json!({"decision": "approved", "reason": null});
    let rows = recorded_outputs(&fx.db, &run_id);
    let human = rows
        .iter()
        .find(|row| row.0 == "review")
        .unwrap_or_else(|| panic!("human node record missing: {rows:?}"));
    assert_eq!(human.1, 0, "attempt 0");
    assert_eq!(human.3.as_ref(), Some(&expected), "full 保存 human output");
    assert_eq!(
        human.2,
        zgraph_cli::run_cmd::output_hash(&expected),
        "hash 复用 CLI record_output 口径"
    );

    // L2：human 节点用记录输出重放，不重新要求人工决策。
    let replay = zgraph(&[
        "replay".to_string(),
        "--run".to_string(),
        run_id.clone(),
        "--mode".to_string(),
        "outputs".to_string(),
        "--json".to_string(),
        "--db".to_string(),
        path_str(&fx.db).to_string(),
    ]);
    assert_eq!(
        replay.status.code(),
        Some(0),
        "replay stdout={} stderr={}",
        String::from_utf8_lossy(&replay.stdout),
        String::from_utf8_lossy(&replay.stderr)
    );
    let value: Value =
        serde_json::from_str(String::from_utf8_lossy(&replay.stdout).trim()).expect("replay json");
    assert_eq!(value["ok"], json!(true));
    assert_eq!(value["matched"], json!(true));
    assert_eq!(value["original_output"], value["replayed_output"]);
    assert_eq!(runs_count(&fx.db), 1, "L2 不产生新 run");
}

/// `--record=hash`：human 实例只写 hash（`output_json` 为 NULL，与既有口径一致）；
/// L2 outputs 重放按既有语义拒绝（`REPLAY_RECORD_MISSING` exit 3）。
#[test]
fn human_hash_record_stores_only_hash() {
    let fx = Fixture::new(HUMAN_GRAPH);
    let mut args = fx.run_args();
    args.push("--record=hash".to_string());
    let run = RunningCli::spawn(&args);
    let (run_id, instance_id) = wait_for_waiting_human(&fx.db);
    assert_eq!(
        zgraph(&fx.decide_args(&run_id, &instance_id, "approved"))
            .status
            .code(),
        Some(0)
    );
    let result = run.wait();
    assert_eq!(result.code, 0, "run stderr={}", result.stderr);

    let rows = recorded_outputs(&fx.db, &run_id);
    let human = rows
        .iter()
        .find(|row| row.0 == "review")
        .unwrap_or_else(|| panic!("human node hash record missing: {rows:?}"));
    assert_eq!(human.3, None, "hash mode stores no output_json");
    assert_eq!(
        human.2,
        zgraph_cli::run_cmd::output_hash(&json!({"decision": "approved", "reason": null}))
    );

    let replay = zgraph(&[
        "replay".to_string(),
        "--run".to_string(),
        run_id,
        "--mode".to_string(),
        "outputs".to_string(),
        "--json".to_string(),
        "--db".to_string(),
        path_str(&fx.db).to_string(),
    ]);
    assert_eq!(replay.status.code(), Some(3));
    assert!(
        String::from_utf8_lossy(&replay.stderr).contains("REPLAY_RECORD_MISSING"),
        "{}",
        String::from_utf8_lossy(&replay.stderr)
    );
}

/// `--record=none`：不写 human 记录（桥配置为 `None`，`recorded_outputs` 为空）。
#[test]
fn human_record_none_writes_nothing() {
    let fx = Fixture::new(HUMAN_GRAPH);
    let mut args = fx.run_args();
    args.push("--record=none".to_string());
    let run = RunningCli::spawn(&args);
    let (run_id, instance_id) = wait_for_waiting_human(&fx.db);
    assert_eq!(
        zgraph(&fx.decide_args(&run_id, &instance_id, "approved"))
            .status
            .code(),
        Some(0)
    );
    let result = run.wait();
    assert_eq!(result.code, 0, "run stderr={}", result.stderr);
    assert!(
        recorded_outputs(&fx.db, &run_id).is_empty(),
        "record=none must not write recorded_outputs"
    );
}
