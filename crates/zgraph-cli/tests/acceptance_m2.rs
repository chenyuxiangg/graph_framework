//! M2 端到端验收测试（AC-01/02/03/05/06/07/10/11）。
//!
//! 数据通路：DSL fixture/内联图 → `CompiledGraph::compile` → kernel `start_run`
//! （经 `zgraph_cli::bridge` 的 `TraceSink` 适配器）→ `zgraph-trace` SQLite →
//! `TraceReader` / 真实 `zgraph trace` 二进制回放。
//!
//! 约定：每个测试独立 tempdir DB；时序断言留足余量（CI 慢机）；
//! **禁止**为变绿弱化断言（AGENTS §8）。

use std::collections::{BTreeMap, HashMap};
use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::Arc;
use std::time::{Duration, Instant as StdInstant};

use serde_json::{json, Value};
use tempfile::TempDir;
use tokio::sync::mpsc;
use tokio::time::timeout;

use zgraph_cli::bridge::{run_in_process, start_traced_run, BridgeOutcome};
use zgraph_dsl::load_graph;
use zgraph_kernel::{
    CompiledGraph, FakeExecutor, NodeError, NodeExecution, NodeExecutor, RunOptions, RunSpec,
    RunStatus, StreamingEvent,
};
use zgraph_trace::{
    EventQuery, EventRow, InstanceRow, InstanceStatus as TraceInstanceStatus,
    RunStatus as TraceRunStatus, TraceReader, TransitionRow,
};

// ---------------------------------------------------------------------------
// 图与 run 构造
// ---------------------------------------------------------------------------

const CUSTOMER_TICKET: &str = "customer_ticket.json";

/// 线性图：`start(x) -> work(x) -> done(r)`。
const LINEAR_JSON: &str = r#"{
  "version": "1.0",
  "metadata": {"name": "linear"},
  "entry_points": ["start"],
  "nodes": [
    {"id": "start", "type": "start",
     "inputs": [{"name": "x", "type": "any", "required": true}], "config": {}},
    {"id": "work", "type": "python_node",
     "inputs": [{"name": "x", "type": "any", "required": true}],
     "config": {"module": "workers.demo", "function": "run"}},
    {"id": "done", "type": "end",
     "inputs": [{"name": "r", "type": "any", "required": true}], "config": {}}
  ],
  "edges": [
    {"from": "start", "to_node": "work", "to_input_port": "x"},
    {"from": "work", "to_node": "done", "to_input_port": "r"}
  ]
}"#;

/// 串行慢图：`start -> slow -> done`。
const SLOW_JSON: &str = r#"{
  "version": "1.0",
  "metadata": {"name": "slow_chain"},
  "entry_points": ["start"],
  "nodes": [
    {"id": "start", "type": "start", "inputs": [], "config": {}},
    {"id": "slow", "type": "python_node",
     "inputs": [{"name": "i", "type": "any", "required": true}],
     "config": {"module": "workers.slow", "function": "run"}},
    {"id": "done", "type": "end",
     "inputs": [{"name": "r", "type": "any", "required": true}], "config": {}}
  ],
  "edges": [
    {"from": "start", "to_node": "slow", "to_input_port": "i"},
    {"from": "slow", "to_node": "done", "to_input_port": "r"}
  ]
}"#;

/// 两节点串行图：`start -> a -> b -> done`（AC-07 ticker/pause）。
const TWO_STEP_JSON: &str = r#"{
  "version": "1.0",
  "metadata": {"name": "two_step"},
  "entry_points": ["start"],
  "nodes": [
    {"id": "start", "type": "start", "inputs": [], "config": {}},
    {"id": "a", "type": "python_node",
     "inputs": [{"name": "i", "type": "any", "required": true}],
     "config": {"module": "workers.a", "function": "run"}},
    {"id": "b", "type": "python_node",
     "inputs": [{"name": "i", "type": "any", "required": true}],
     "config": {"module": "workers.b", "function": "run"}},
    {"id": "done", "type": "end",
     "inputs": [{"name": "r", "type": "any", "required": true}], "config": {}}
  ],
  "edges": [
    {"from": "start", "to_node": "a", "to_input_port": "i"},
    {"from": "a", "to_node": "b", "to_input_port": "i"},
    {"from": "b", "to_node": "done", "to_input_port": "r"}
  ]
}"#;

/// 并行崩溃图：`start -> boom/slow -> done`（AC-05：在飞并行实例）。
const CRASH_JSON: &str = r#"{
  "version": "1.0",
  "metadata": {"name": "crash_parallel"},
  "entry_points": ["start"],
  "nodes": [
    {"id": "start", "type": "start", "inputs": [], "config": {}},
    {"id": "boom", "type": "python_node",
     "inputs": [{"name": "i", "type": "any", "required": true}],
     "config": {"module": "workers.boom", "function": "run"}},
    {"id": "slow", "type": "python_node",
     "inputs": [{"name": "i", "type": "any", "required": true}],
     "config": {"module": "workers.slow", "function": "run"}},
    {"id": "done", "type": "end",
     "inputs": [{"name": "b", "type": "any", "required": true},
                {"name": "s", "type": "any", "required": true}],
     "config": {}}
  ],
  "edges": [
    {"from": "start", "to_node": "boom", "to_input_port": "i"},
    {"from": "start", "to_node": "slow", "to_input_port": "i"},
    {"from": "boom", "to_node": "done", "to_input_port": "b"},
    {"from": "slow", "to_node": "done", "to_input_port": "s"}
  ]
}"#;

/// 门控拦截图：`start -> a -[gate go==true]-> b -> done`（AC-11）。
const GATE_SKIP_JSON: &str = r#"{
  "version": "1.0",
  "metadata": {"name": "gate_skip"},
  "entry_points": ["start"],
  "nodes": [
    {"id": "start", "type": "start", "inputs": [], "config": {}},
    {"id": "a", "type": "python_node",
     "inputs": [{"name": "i", "type": "any", "required": true}],
     "config": {"module": "workers.a", "function": "run"}},
    {"id": "b", "type": "python_node",
     "inputs": [{"name": "i", "type": "any", "required": true}],
     "config": {"module": "workers.b", "function": "run"}},
    {"id": "done", "type": "end",
     "inputs": [{"name": "r", "type": "any", "required": true}], "config": {}}
  ],
  "edges": [
    {"from": "start", "to_node": "a", "to_input_port": "i"},
    {"from": "a", "to_node": "b", "to_input_port": "i",
     "gate": {"function": "check_output", "params": {"path": "go", "equals": true}}},
    {"from": "b", "to_node": "done", "to_input_port": "r"}
  ]
}"#;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("repo root resolves")
}

fn compile_bytes(bytes: &[u8]) -> Arc<CompiledGraph> {
    let loaded = load_graph(bytes).expect("graph passes DSL validation");
    CompiledGraph::compile(&loaded).expect("graph compiles")
}

fn compile_fixture(name: &str) -> Arc<CompiledGraph> {
    let path = repo_root().join("tests/fixtures/graphs").join(name);
    let bytes = std::fs::read(&path).unwrap_or_else(|error| panic!("fixture {name}: {error}"));
    compile_bytes(&bytes)
}

fn compile_text(text: &str) -> Arc<CompiledGraph> {
    compile_bytes(text.as_bytes())
}

fn entry_inputs(pairs: &[(&str, &str, Value)]) -> HashMap<String, HashMap<String, Value>> {
    let mut map: HashMap<String, HashMap<String, Value>> = HashMap::new();
    for (node, port, value) in pairs {
        map.entry((*node).to_string())
            .or_default()
            .insert((*port).to_string(), value.clone());
    }
    map
}

/// 默认选项的 `RunSpec`。
fn spec(graph: Arc<CompiledGraph>, inputs: HashMap<String, HashMap<String, Value>>) -> RunSpec {
    RunSpec {
        graph,
        entry_inputs: inputs,
        user_id: "u1".to_string(),
        global_vars: HashMap::new(),
        options: RunOptions::default(),
    }
}

/// 从 `RunOptions::default()` 基线修改选项（兼容 test-hooks 的额外字段）。
fn spec_with(
    graph: Arc<CompiledGraph>,
    inputs: HashMap<String, HashMap<String, Value>>,
    configure: impl FnOnce(&mut RunOptions),
) -> RunSpec {
    let mut options = RunOptions::default();
    configure(&mut options);
    RunSpec {
        graph,
        entry_inputs: inputs,
        user_id: "u1".to_string(),
        global_vars: HashMap::new(),
        options,
    }
}

fn db_path(dir: &TempDir, name: &str) -> PathBuf {
    dir.path().join(name)
}

// ---------------------------------------------------------------------------
// trace 查询辅助
// ---------------------------------------------------------------------------

fn events(reader: &TraceReader, run_id: &str) -> Vec<EventRow> {
    reader
        .events(run_id, &EventQuery::default())
        .expect("query events")
}

fn count_kind(events: &[EventRow], kind: &str) -> usize {
    events.iter().filter(|event| event.kind == kind).count()
}

fn instance(reader: &TraceReader, run_id: &str, node_id: &str) -> InstanceRow {
    reader
        .instances(run_id)
        .expect("query instances")
        .into_iter()
        .find(|row| row.node_id == node_id)
        .unwrap_or_else(|| panic!("instance for node {node_id}"))
}

fn instance_status(reader: &TraceReader, run_id: &str, node_id: &str) -> TraceInstanceStatus {
    instance(reader, run_id, node_id).status
}

fn run_row(reader: &TraceReader, run_id: &str) -> zgraph_trace::RunRow {
    reader
        .run(run_id)
        .expect("query run")
        .expect("run row exists")
}

fn allowed_transition(from: TraceInstanceStatus, to: TraceInstanceStatus) -> bool {
    use TraceInstanceStatus::{
        Cancelled, Completed, Crashed, Pending, Ready, Running, Skipped, TimedOut,
    };
    matches!(
        (from, to),
        (Pending, Ready)
            | (Pending, Cancelled)
            | (Pending, Skipped)
            | (Ready, Running)
            | (Ready, Cancelled)
            | (Running, Completed)
            | (Running, Crashed)
            | (Running, TimedOut)
            | (Running, Cancelled)
    )
}

/// 用真实 `zgraph trace --json` 回放，返回 NDJSON 行。
fn trace_json(db: &Path, run_id: &str) -> Vec<Value> {
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_zgraph"))
        .args([
            "trace",
            run_id,
            "--db",
            db.to_str().expect("utf8 db"),
            "--json",
        ])
        .output()
        .expect("zgraph binary runs");
    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(|line| serde_json::from_str(line).expect("NDJSON line"))
        .collect()
}

/// 用真实 `zgraph trace` 人类输出回放。
fn trace_text(db: &Path, run_id: &str) -> String {
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_zgraph"))
        .args(["trace", run_id, "--db", db.to_str().expect("utf8 db")])
        .output()
        .expect("zgraph binary runs");
    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).to_string()
}

/// 轮询等待条件成立（上限 `deadline`）；返回最终是否成立。
async fn wait_until(deadline: Duration, mut condition: impl FnMut() -> bool) -> bool {
    let started = StdInstant::now();
    while started.elapsed() <= deadline {
        if condition() {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(1)).await;
    }
    condition()
}

// ---------------------------------------------------------------------------
// customer_ticket 场景（AC-01 / AC-02 / AC-03）
// ---------------------------------------------------------------------------

fn customer_ticket_executor(need_human: bool) -> Arc<dyn NodeExecutor> {
    let executor = FakeExecutor::new()
        .respond(
            "input_parser",
            json!({"parsed_query": "退款政策", "user_id": "u1"}),
        )
        .respond("kb_search", json!({"kb_result": {"hits": 1}}))
        .respond(
            "answer_generator",
            json!({"answer": "可以退款", "need_human": need_human}),
        )
        .respond("human_escalation_check", json!({"need_human": need_human}))
        .respond("human_handoff", json!({"handoff": true}));
    Arc::new(executor)
}

fn customer_ticket_spec(need_human: bool) -> RunSpec {
    let graph = compile_fixture(CUSTOMER_TICKET);
    let inputs = entry_inputs(&[("input_parser", "user_message", json!("我要退款"))]);
    let _ = need_human;
    spec(graph, inputs)
}

async fn run_customer_ticket(need_human: bool) -> (TempDir, BridgeOutcome) {
    let dir = tempfile::tempdir().expect("tempdir");
    let db = db_path(&dir, "trace.db");
    let bridge = run_in_process(
        customer_ticket_spec(need_human),
        customer_ticket_executor(need_human),
        &db,
    )
    .await
    .expect("bridge run");
    (dir, bridge)
}

#[tokio::test]
async fn ac01_customer_ticket_completes_with_expected_instances() {
    let (_dir, bridge) = run_customer_ticket(false).await;
    let expected_output = json!({"result": {"answer": "可以退款", "need_human": false}});

    assert_eq!(bridge.outcome.status, RunStatus::Completed);
    assert_eq!(bridge.outcome.output, Some(expected_output.clone()));

    let run_id = bridge.outcome.run_id.clone();
    let reader = bridge.reader();
    let run = run_row(reader, &run_id);
    assert_eq!(run.status, TraceRunStatus::Completed);
    assert_eq!(run.output_json, Some(expected_output));

    let expected = [
        ("input_parser", TraceInstanceStatus::Completed),
        ("kb_search", TraceInstanceStatus::Completed),
        ("answer_generator", TraceInstanceStatus::Completed),
        ("human_escalation_check", TraceInstanceStatus::Completed),
        ("human_handoff", TraceInstanceStatus::Skipped),
        ("end_success", TraceInstanceStatus::Completed),
    ];
    for (node_id, status) in expected {
        assert_eq!(
            instance_status(reader, &run_id, node_id),
            status,
            "node {node_id}"
        );
    }
    assert_eq!(reader.instances(&run_id).expect("instances").len(), 6);
}

#[tokio::test]
async fn ac02_gate_blocks_handoff_and_reverse_path_runs_it() {
    // 正向：need_human=false → gate 拦截 → human_handoff Skipped
    let (_dir, bridge) = run_customer_ticket(false).await;
    assert_eq!(bridge.outcome.status, RunStatus::Completed);
    let run_id = bridge.outcome.run_id.clone();
    let reader = bridge.reader();
    let gates = reader.gate_results(&run_id).expect("gate results");
    let handoff: Vec<_> = gates
        .iter()
        .filter(|gate| gate.to_node == "human_handoff")
        .collect();
    assert_eq!(handoff.len(), 1, "exactly one gate result to human_handoff");
    assert!(!handoff[0].passed);
    assert_eq!(handoff[0].edge_index, 4);
    assert_eq!(handoff[0].to_port, "context");
    assert_eq!(handoff[0].gate_fn.as_deref(), Some("check_output"));
    assert!(handoff[0].error.is_none());
    assert_eq!(
        instance_status(reader, &run_id, "human_handoff"),
        TraceInstanceStatus::Skipped
    );
    assert_eq!(
        instance_status(reader, &run_id, "end_success"),
        TraceInstanceStatus::Completed
    );

    let text = trace_text(bridge.db_path(), &run_id);
    assert!(text.contains("passed=false"), "{text}");
    assert!(text.contains("all_sources_blocked"), "{text}");
    assert!(text.contains("Skipped"), "{text}");

    // 反向：need_human=true → gate 放行 → human_handoff 实际执行且 Completed
    let (_dir2, bridge) = run_customer_ticket(true).await;
    assert_eq!(bridge.outcome.status, RunStatus::Completed);
    let run_id = bridge.outcome.run_id.clone();
    let reader = bridge.reader();
    let gates = reader.gate_results(&run_id).expect("gate results");
    let handoff: Vec<_> = gates
        .iter()
        .filter(|gate| gate.to_node == "human_handoff")
        .collect();
    assert_eq!(handoff.len(), 1);
    assert!(handoff[0].passed);
    assert_eq!(
        instance_status(reader, &run_id, "human_handoff"),
        TraceInstanceStatus::Completed
    );
    assert_eq!(
        instance_status(reader, &run_id, "end_success"),
        TraceInstanceStatus::Completed
    );
    assert!(trace_text(bridge.db_path(), &run_id).contains("passed=true"));
}

/// 生命周期形状：RunStarted 首、RunFinished 尾各一次；seq 严格递增。
fn assert_lifecycle_shape(reader: &TraceReader, run_id: &str) {
    let all = events(reader, run_id);
    assert_eq!(
        all.first().map(|event| event.kind.as_str()),
        Some("RunStarted")
    );
    assert_eq!(
        all.last().map(|event| event.kind.as_str()),
        Some("RunFinished")
    );
    assert_eq!(count_kind(&all, "RunStarted"), 1);
    assert_eq!(count_kind(&all, "RunFinished"), 1);
    let seqs: Vec<i64> = all.iter().map(|event| event.seq).collect();
    assert!(
        seqs.windows(2).all(|pair| pair[0] < pair[1]),
        "seq strictly increasing: {seqs:?}"
    );
}

/// 每个实例的迁移链：首 Pending→(Ready|Skipped)、连续、01 §1.2.3 矩阵合法、末态终态。
fn assert_transition_chains(reader: &TraceReader, run_id: &str) {
    let transitions: Vec<TransitionRow> = reader.transitions(run_id).expect("transitions");
    let mut chains: BTreeMap<String, Vec<&TransitionRow>> = BTreeMap::new();
    for row in &transitions {
        chains.entry(row.instance_id.clone()).or_default().push(row);
    }
    assert_eq!(chains.len(), 6, "one chain per instance");
    for (instance_id, chain) in &chains {
        assert_transition_chain(instance_id, chain);
    }
}

/// 单个实例迁移链校验（01 §1.2.3 矩阵）。
fn assert_transition_chain(instance_id: &str, chain: &[&TransitionRow]) {
    let first = chain.first().expect("chain non-empty");
    assert_eq!(
        first.from_status,
        TraceInstanceStatus::Pending,
        "{instance_id}: first transition starts from Pending"
    );
    assert!(
        matches!(
            first.to_status,
            TraceInstanceStatus::Ready | TraceInstanceStatus::Skipped
        ),
        "{instance_id}: first transition to {:?}",
        first.to_status
    );
    for pair in chain.windows(2) {
        assert_eq!(
            pair[1].from_status, pair[0].to_status,
            "{instance_id}: chain must be continuous"
        );
    }
    for row in chain {
        assert!(
            allowed_transition(row.from_status, row.to_status),
            "{instance_id}: illegal transition {:?} -> {:?}",
            row.from_status,
            row.to_status
        );
    }
    assert!(
        chain.last().expect("last").to_status.is_terminal(),
        "{instance_id}: chain must end in a terminal status"
    );
}

/// L1：`zgraph trace --json` 与 `TraceReader.events` 逐条一致。
fn assert_l1_replay(reader: &TraceReader, db: &Path, run_id: &str) {
    let all = events(reader, run_id);
    let lines = trace_json(db, run_id);
    assert_eq!(lines.len(), all.len(), "L1 replay event count");
    for (line, event) in lines.iter().zip(all.iter()) {
        assert_eq!(line["seq"], json!(event.seq));
        assert_eq!(line["kind"], event.kind.as_str());
        assert_eq!(line["at"], event.at.as_str());
        assert_eq!(line["severity"], event.severity.as_str());
        assert_eq!(line["run_id"], event.run_id.as_str());
        assert_eq!(line["payload"], event.payload);
        match &event.instance_id {
            Some(instance_id) => assert_eq!(line["instance_id"], instance_id.as_str()),
            None => assert!(line.get("instance_id").is_none()),
        }
        match &event.node_id {
            Some(node_id) => assert_eq!(line["node_id"], node_id.as_str()),
            None => assert!(line.get("node_id").is_none()),
        }
    }
}

#[tokio::test]
async fn ac03_event_order_transition_matrix_and_l1_replay() {
    let (_dir, bridge) = run_customer_ticket(false).await;
    let run_id = bridge.outcome.run_id.clone();
    let reader = bridge.reader();
    assert_lifecycle_shape(reader, &run_id);
    assert_transition_chains(reader, &run_id);
    assert_l1_replay(reader, bridge.db_path(), &run_id);
}

/// 按顺序发 3 个 token delta + 1 个 progress 的自写执行器（AC-03 streaming）。
struct EmitTokenExecutor;

impl NodeExecutor for EmitTokenExecutor {
    fn execute<'a>(
        &'a self,
        execution: NodeExecution<'a>,
    ) -> Pin<Box<dyn Future<Output = Result<Value, NodeError>> + Send + 'a>> {
        Box::pin(async move {
            let ctx = execution.ctx;
            for piece in ["可以", "退款", "政策"] {
                ctx.emit(StreamingEvent::TokenDelta {
                    text: piece.to_string(),
                });
            }
            ctx.emit(StreamingEvent::Progress {
                percent: 50,
                message: "half".to_string(),
            });
            Ok(json!({"answer": "可以退款政策"}))
        })
    }
}

#[tokio::test]
async fn ac03_streaming_aggregates_tokens_without_dropping() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db = db_path(&dir, "trace.db");
    let graph = compile_text(LINEAR_JSON);
    let inputs = entry_inputs(&[("start", "x", json!({"v": 1}))]);
    let bridge = run_in_process(spec(graph, inputs), Arc::new(EmitTokenExecutor), &db)
        .await
        .expect("bridge run");
    assert_eq!(bridge.outcome.status, RunStatus::Completed);
    assert_eq!(bridge.outcome.metrics.streaming_dropped, 0);

    let run_id = bridge.outcome.run_id.clone();
    let reader = bridge.reader();
    assert_eq!(run_row(reader, &run_id).streaming_dropped, 0);

    let segments = reader.streaming_events(&run_id).expect("streaming rows");
    let tokens: Vec<_> = segments.iter().filter(|row| row.kind == "token").collect();
    assert!(!tokens.is_empty(), "token segments expected");
    let text: String = tokens
        .iter()
        .filter_map(|row| row.text.as_deref())
        .collect();
    assert_eq!(text, "可以退款政策", "token 段拼接 = 完整回复");
    let deltas: i64 = tokens.iter().filter_map(|row| row.delta_count).sum();
    assert_eq!(deltas, 3, "delta_count 汇总 = 原始 delta 数");
    for token in &tokens {
        assert_eq!(token.node_id.as_deref(), Some("work"));
    }

    let progress: Vec<_> = segments
        .iter()
        .filter(|row| row.kind == "progress")
        .collect();
    assert_eq!(progress.len(), 1);
    assert_eq!(progress[0].text.as_deref(), Some("half"));
    assert_eq!(progress[0].percent, Some(50));

    let streaming_events = count_kind(&events(reader, &run_id), "Streaming");
    assert_eq!(streaming_events, segments.len(), "events 与聚合段 1:1");
}

// ---------------------------------------------------------------------------
// AC-05：PythonException 冷峻失败 + 崩溃隔离
// ---------------------------------------------------------------------------

const TRACEBACK: &str = "Traceback (most recent call last):\n  File \"w.py\", line 3, in run\n    raise RuntimeError(\"boom\")\nRuntimeError: boom";

/// `boom` 返回 PythonException；`slow` 长时间在飞（验证被取消）。
struct CrashExecutor;

impl NodeExecutor for CrashExecutor {
    fn execute<'a>(
        &'a self,
        execution: NodeExecution<'a>,
    ) -> Pin<Box<dyn Future<Output = Result<Value, NodeError>> + Send + 'a>> {
        let node_id = execution.node.id.clone();
        Box::pin(async move {
            if node_id == "boom" {
                return Err(NodeError::PythonException {
                    message: "RuntimeError: boom".to_string(),
                    traceback: TRACEBACK.to_string(),
                });
            }
            tokio::time::sleep(Duration::from_secs(30)).await;
            Ok(json!({"slow": true}))
        })
    }
}

/// 断言崩溃 run 的 trace：run Failed + boom Crashed（保留 traceback）+ slow Cancelled。
fn assert_crash_trace(reader: &TraceReader, run_id: &str) {
    let run = run_row(reader, run_id);
    assert_eq!(run.status, TraceRunStatus::Failed);
    assert_eq!(run.error_code.as_deref(), Some("NODE_PYTHON_EXCEPTION"));

    let boom = instance(reader, run_id, "boom");
    assert_eq!(boom.status, TraceInstanceStatus::Crashed);
    assert_eq!(boom.error_code.as_deref(), Some("NODE_PYTHON_EXCEPTION"));
    let boom_error_text = boom.error.expect("boom error json").to_string();
    assert!(
        boom_error_text.contains("Traceback (most recent call last)"),
        "instances.error must keep traceback: {boom_error_text}"
    );
    assert!(
        boom_error_text.contains("RuntimeError: boom"),
        "{boom_error_text}"
    );
    assert_eq!(
        instance_status(reader, run_id, "slow"),
        TraceInstanceStatus::Cancelled,
        "in-flight parallel instance is cancelled"
    );

    let all = events(reader, run_id);
    assert_eq!(count_kind(&all, "RunStarted"), 1);
    assert_eq!(count_kind(&all, "RunFinished"), 1);
    assert_eq!(all.last().expect("last").payload["status"], "Failed");
    assert_eq!(
        all.last().expect("last").payload["error"]["code"],
        "NODE_PYTHON_EXCEPTION"
    );
}

/// 断言崩溃隔离：同一图正常 executor 重跑，全部实例 Completed。
fn assert_healthy_rerun(reader: &TraceReader, run_id: &str) {
    for node_id in ["boom", "slow", "done"] {
        assert_eq!(
            instance_status(reader, run_id, node_id),
            TraceInstanceStatus::Completed,
            "node {node_id}"
        );
    }
}

#[tokio::test]
async fn ac05_python_exception_fails_run_then_rerun_completes() {
    let dir = tempfile::tempdir().expect("tempdir");
    let graph = compile_text(CRASH_JSON);

    // 第一次：boom 抛 PythonException → Crashed / run Failed；在飞 slow → Cancelled
    let db = db_path(&dir, "crash.db");
    let bridge = timeout(
        Duration::from_secs(5),
        run_in_process(
            spec(graph.clone(), HashMap::new()),
            Arc::new(CrashExecutor),
            &db,
        ),
    )
    .await
    .expect("failed run must return quickly")
    .expect("bridge run");
    assert_eq!(bridge.outcome.status, RunStatus::Failed);
    let error = bridge.outcome.error.as_ref().expect("run error");
    assert_eq!(error.code, "NODE_PYTHON_EXCEPTION");
    assert!(error.message.contains("boom"), "{}", error.message);
    let run_id = bridge.outcome.run_id.clone();
    assert_crash_trace(bridge.reader(), &run_id);

    // 第二次：同图正常 executor → Completed（崩溃隔离）
    let db2 = db_path(&dir, "recover.db");
    let healthy = FakeExecutor::new()
        .respond("boom", json!({"boom": true}))
        .respond("slow", json!({"slow": true}));
    let bridge = run_in_process(spec(graph, HashMap::new()), Arc::new(healthy), &db2)
        .await
        .expect("healthy rerun");
    assert_eq!(bridge.outcome.status, RunStatus::Completed);
    let run_id = bridge.outcome.run_id.clone();
    assert_healthy_rerun(bridge.reader(), &run_id);
}

// ---------------------------------------------------------------------------
// AC-06（D1 改写）：超时 stop-waiting
// ---------------------------------------------------------------------------

/// 一直睡 30s 的执行器（真 worker 进程组 kill 属 M3；此处由内核 abort 任务）。
struct SleepForeverExecutor;

impl NodeExecutor for SleepForeverExecutor {
    fn execute<'a>(
        &'a self,
        _execution: NodeExecution<'a>,
    ) -> Pin<Box<dyn Future<Output = Result<Value, NodeError>> + Send + 'a>> {
        Box::pin(async move {
            tokio::time::sleep(Duration::from_secs(30)).await;
            Ok(json!({"never": true}))
        })
    }
}

/// 断言超时 run 的 trace：TimedOut/Failed/NODE_TIMEOUT、NodeTimedOut 一次、无 NodeAbandoned。
fn assert_timeout_trace(reader: &TraceReader, run_id: &str) {
    let run = run_row(reader, run_id);
    assert_eq!(run.status, TraceRunStatus::Failed);
    assert_eq!(run.error_code.as_deref(), Some("NODE_TIMEOUT"));
    assert_eq!(run.abandoned_tasks, 0, "D1: abandoned_tasks 恒 0");
    assert!(run.killed_workers >= 1);

    // kernel REV-011：TimedOut 迁移快照回填 NodeError::Timeout{elapsed_ms} 与
    // NODE_TIMEOUT，经 bridge 映射后 trace 的 instances.error/error_code 与
    // transitions.error_code/error 均非 NULL。
    let slow = instance(reader, run_id, "slow");
    assert_eq!(slow.status, TraceInstanceStatus::TimedOut);
    assert_eq!(slow.error_code.as_deref(), Some("NODE_TIMEOUT"));
    let instance_error = slow.error.clone().expect("instances.error 非 NULL");
    let instance_elapsed = timeout_elapsed_ms(&instance_error);

    let transitions = reader.transitions(run_id).expect("transitions");
    let timed_out = transitions
        .iter()
        .find(|row| {
            row.instance_id == slow.instance_id && row.to_status == TraceInstanceStatus::TimedOut
        })
        .expect("slow 的 TimedOut 迁移");
    assert_eq!(timed_out.error_code.as_deref(), Some("NODE_TIMEOUT"));
    let transition_error = timed_out.error.clone().expect("transitions.error 非 NULL");
    assert_eq!(
        transition_error, instance_error,
        "transitions.error 与 instances.error 同形"
    );
    assert_eq!(timeout_elapsed_ms(&transition_error), instance_elapsed);

    let all = events(reader, run_id);
    assert_eq!(count_kind(&all, "NodeTimedOut"), 1);
    assert_eq!(
        count_kind(&all, "NodeAbandoned"),
        0,
        "D1: v2 不产生 NodeAbandoned"
    );
    let timed_out_event = all
        .iter()
        .find(|event| event.kind == "NodeTimedOut")
        .expect("NodeTimedOut event");
    let event_elapsed = timed_out_event.payload["elapsed_ms"]
        .as_u64()
        .expect("elapsed_ms payload");
    assert_eq!(event_elapsed, instance_elapsed, "同一 elapsed_ms 测量值");
    assert!(event_elapsed >= 90, "elapsed_ms={event_elapsed}");
    assert_eq!(all.last().expect("last").payload["status"], "Failed");
    assert_eq!(
        all.last().expect("last").payload["error"]["code"],
        "NODE_TIMEOUT"
    );
}

/// 断言 bridge 的 `NodeError::Timeout` JSON 形状并返回 `elapsed_ms`。
fn timeout_elapsed_ms(error: &Value) -> u64 {
    assert_eq!(error["code"], "NODE_TIMEOUT");
    assert_eq!(error["kind"], "Timeout", "NodeError::Timeout 变体：{error}");
    assert!(
        error["message"]
            .as_str()
            .is_some_and(|message| message.contains("timeout")),
        "message 应描述超时：{error}"
    );
    error["elapsed_ms"]
        .as_u64()
        .unwrap_or_else(|| panic!("error.elapsed_ms missing: {error}"))
}

#[tokio::test]
async fn ac06_timeout_fails_run_without_abandoned_tasks() {
    // AC-06 D1 改写：test-hooks 强制 100ms 超时（等价 scale 版 timeout_ms=1000）。
    // 「真进程组 kill / 无残留子进程」属 M3；M2 断言内核 abort + 指标语义。
    let dir = tempfile::tempdir().expect("tempdir");
    let db = db_path(&dir, "timeout.db");
    let graph = compile_text(SLOW_JSON);
    let run_spec = spec_with(graph, HashMap::new(), |options| {
        options.tick_interval_ms = 50;
        options.deadlock_grace_ms = 5_000;
        options.test_hooks.force_node_timeout_ms = Some(100);
    });

    let started = StdInstant::now();
    let result = timeout(
        Duration::from_secs(5),
        run_in_process(run_spec, Arc::new(SleepForeverExecutor), &db),
    )
    .await
    .expect("timeout run must return within 5s")
    .expect("bridge run");
    let elapsed = started.elapsed();
    assert!(elapsed < Duration::from_secs(5), "elapsed {elapsed:?}");
    assert!(elapsed >= Duration::from_millis(50), "elapsed {elapsed:?}");

    assert_eq!(result.outcome.status, RunStatus::Failed);
    let error = result.outcome.error.as_ref().expect("run error");
    assert_eq!(error.code, "NODE_TIMEOUT");
    assert_eq!(result.outcome.metrics.abandoned_tasks, 0, "D1: 恒 0");
    assert!(
        result.outcome.metrics.killed_workers >= 1,
        "killed_workers={}",
        result.outcome.metrics.killed_workers
    );

    let run_id = result.outcome.run_id.clone();
    assert_timeout_trace(result.reader(), &run_id);
}

// ---------------------------------------------------------------------------
// AC-07：事件循环不阻塞（ticker 间隔）+ Pause/Resume
// ---------------------------------------------------------------------------

/// 节点开始时上报 node_id，`slow_ms > 0` 的节点先睡再返回。
struct SignallingExecutor {
    started: tokio::sync::mpsc::UnboundedSender<String>,
    sleep_ms: u64,
}

impl NodeExecutor for SignallingExecutor {
    fn execute<'a>(
        &'a self,
        execution: NodeExecution<'a>,
    ) -> Pin<Box<dyn Future<Output = Result<Value, NodeError>> + Send + 'a>> {
        let node_id = execution.node.id.clone();
        let started = self.started.clone();
        let sleep_ms = self.sleep_ms;
        Box::pin(async move {
            let _ = started.send(node_id.clone());
            if sleep_ms > 0 {
                tokio::time::sleep(Duration::from_millis(sleep_ms)).await;
            }
            Ok(json!({"node": node_id}))
        })
    }
}

/// 断言 watchdog ticker 间隔 <50ms（节点 sleep 期间事件循环不被阻塞）。
fn assert_tick_gaps(mut probe_rx: mpsc::UnboundedReceiver<StdInstant>) {
    let mut ticks: Vec<StdInstant> = Vec::new();
    while let Ok(instant) = probe_rx.try_recv() {
        ticks.push(instant);
    }
    assert!(
        ticks.len() > 10,
        "expected watchdog ticks while nodes run, got {}",
        ticks.len()
    );
    let max_gap = ticks
        .windows(2)
        .map(|pair| pair[1].duration_since(pair[0]))
        .max()
        .expect("tick gap");
    assert!(
        max_gap < Duration::from_millis(50),
        "max tick gap {max_gap:?} (tick=5ms)"
    );
}

#[tokio::test]
async fn ac07_watchdog_ticks_and_pause_resume() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db = db_path(&dir, "ticker.db");
    let (probe_tx, probe_rx) = mpsc::unbounded_channel();
    let (started_tx, mut started_rx) = tokio::sync::mpsc::unbounded_channel();
    let graph = compile_text(TWO_STEP_JSON);
    let run_spec = spec_with(graph, HashMap::new(), |options| {
        options.tick_interval_ms = 5;
        options.test_hooks.tick_probe = Some(probe_tx);
    });
    let executor = Arc::new(SignallingExecutor {
        started: started_tx,
        sleep_ms: 300,
    });
    let traced = start_traced_run(run_spec, executor, &db).expect("start traced run");

    // a 开始执行（ticker 同时推进）
    let first = timeout(Duration::from_secs(5), started_rx.recv())
        .await
        .expect("node a starts")
        .expect("start signal");
    assert_eq!(first, "a");
    assert_paused_within_100ms(&traced).await;

    // 暂停期间 a 完成 → b 变 Ready 但不得被派发
    tokio::time::sleep(Duration::from_millis(400)).await;
    assert_eq!(traced.status(), RunStatus::Paused);
    assert!(
        started_rx.try_recv().is_err(),
        "node b must not be dispatched while paused"
    );

    // 恢复后 b 执行、run 完成
    assert!(traced.resume());
    let result = timeout(Duration::from_secs(5), traced.wait())
        .await
        .expect("run completes after resume")
        .expect("bridge run");
    assert_eq!(result.outcome.status, RunStatus::Completed);
    assert_eq!(
        started_rx.try_recv().expect("b dispatched after resume"),
        "b"
    );
    assert_tick_gaps(probe_rx);
}

/// 节点执行中暂停：`Paused` 状态必须在 <100ms 内可见。
async fn assert_paused_within_100ms(traced: &zgraph_cli::bridge::TracedRun) {
    let paused_at = StdInstant::now();
    assert!(traced.pause());
    assert!(
        wait_until(Duration::from_millis(100), || traced.status()
            == RunStatus::Paused)
        .await,
        "Paused must be visible within 100ms"
    );
    let visible = paused_at.elapsed();
    assert!(
        visible < Duration::from_millis(100),
        "paused after {visible:?}"
    );
}

// ---------------------------------------------------------------------------
// AC-10：Pause/Resume/Cancel 幂等
// ---------------------------------------------------------------------------

/// 重复 Pause/Resume 幂等：状态按预期切换，且不产生额外生命周期事件。
async fn pause_resume_twice(traced: &zgraph_cli::bridge::TracedRun) {
    assert!(traced.pause());
    assert!(
        wait_until(Duration::from_secs(2), || traced.status()
            == RunStatus::Paused)
        .await,
        "Paused"
    );
    assert!(traced.pause(), "repeated pause accepted (no-op)");
    assert_eq!(traced.status(), RunStatus::Paused);
    assert!(traced.resume());
    assert!(
        wait_until(Duration::from_secs(2), || traced.status()
            == RunStatus::Running)
        .await,
        "Running"
    );
    assert!(traced.resume(), "repeated resume accepted (no-op)");
    assert_eq!(traced.status(), RunStatus::Running);
}

/// 断言取消 run 的 trace：Cancelled + 在飞实例 Cancelled + RunFinished 恰一次。
fn assert_cancelled_trace(reader: &TraceReader, run_id: &str) {
    assert_eq!(run_row(reader, run_id).status, TraceRunStatus::Cancelled);
    assert_eq!(
        instance_status(reader, run_id, "slow"),
        TraceInstanceStatus::Cancelled,
        "in-flight instance cancelled"
    );
    let all = events(reader, run_id);
    assert_eq!(count_kind(&all, "RunStarted"), 1);
    assert_eq!(
        count_kind(&all, "RunFinished"),
        1,
        "RunFinished exactly once"
    );
    assert_eq!(all.last().expect("last").payload["status"], "Cancelled");
}

#[tokio::test]
async fn ac10_pause_resume_cancel_are_idempotent() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db = db_path(&dir, "control.db");
    let (started_tx, mut started_rx) = tokio::sync::mpsc::unbounded_channel();
    let graph = compile_text(SLOW_JSON);
    let executor = Arc::new(SignallingExecutor {
        started: started_tx,
        sleep_ms: 300,
    });
    let traced = start_traced_run(spec(graph, HashMap::new()), executor, &db).expect("start run");

    let first = timeout(Duration::from_secs(5), started_rx.recv())
        .await
        .expect("slow starts")
        .expect("start signal");
    assert_eq!(first, "slow");
    pause_resume_twice(&traced).await;

    // 在飞时取消；随后的 control 全部 no-op
    assert!(traced.cancel());
    let _ = traced.cancel();
    let _ = traced.pause();
    let _ = traced.resume();
    let result = timeout(Duration::from_secs(5), traced.wait())
        .await
        .expect("cancelled run returns")
        .expect("bridge run");
    assert_eq!(result.outcome.status, RunStatus::Cancelled);
    let run_id = result.outcome.run_id.clone();
    assert_cancelled_trace(result.reader(), &run_id);
}

// ---------------------------------------------------------------------------
// AC-11：gate 全拦截（Skipped 级联）与死锁检测
// ---------------------------------------------------------------------------

fn gate_skip_executor() -> Arc<dyn NodeExecutor> {
    let executor = FakeExecutor::new()
        .respond("a", json!({"go": false}))
        .respond("b", json!({"ran": true}));
    Arc::new(executor)
}

#[tokio::test]
async fn ac11_gate_skip_cascade_completes_without_false_deadlock() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db = db_path(&dir, "skip.db");
    let graph = compile_text(GATE_SKIP_JSON);
    let result = run_in_process(spec(graph, HashMap::new()), gate_skip_executor(), &db)
        .await
        .expect("bridge run");

    assert_eq!(result.outcome.status, RunStatus::Completed);
    assert!(result.outcome.error.is_none(), "no false RUN_DEADLOCK");
    assert_eq!(
        result.outcome.output, None,
        "no end output when all skipped"
    );

    let run_id = result.outcome.run_id.clone();
    let reader = result.reader();
    assert_eq!(run_row(reader, &run_id).status, TraceRunStatus::Completed);
    assert_eq!(
        instance_status(reader, &run_id, "b"),
        TraceInstanceStatus::Skipped
    );
    assert_eq!(
        instance_status(reader, &run_id, "done"),
        TraceInstanceStatus::Skipped
    );
    let gates = reader.gate_results(&run_id).expect("gate results");
    let blocked: Vec<_> = gates.iter().filter(|gate| gate.to_node == "b").collect();
    assert_eq!(blocked.len(), 1);
    assert!(!blocked[0].passed);
}

#[tokio::test]
async fn ac11_disable_settle_reports_run_deadlock() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db = db_path(&dir, "deadlock.db");
    let graph = compile_text(GATE_SKIP_JSON);
    let run_spec = spec_with(graph, HashMap::new(), |options| {
        options.tick_interval_ms = 10;
        options.deadlock_grace_ms = 100;
        options.test_hooks.disable_settle = true;
    });

    let started = StdInstant::now();
    let result = timeout(
        Duration::from_secs(5),
        run_in_process(run_spec, gate_skip_executor(), &db),
    )
    .await
    .expect("deadlock run must return within 5s")
    .expect("bridge run");
    let elapsed = started.elapsed();
    assert!(elapsed < Duration::from_secs(5), "elapsed {elapsed:?}");

    assert_eq!(result.outcome.status, RunStatus::Failed);
    let error = result.outcome.error.as_ref().expect("run error");
    assert_eq!(error.code, "RUN_DEADLOCK");
    assert!(
        error.message.contains("no runnable instance"),
        "{}",
        error.message
    );

    let run_id = result.outcome.run_id.clone();
    let reader = result.reader();
    assert_eq!(
        run_row(reader, &run_id).error_code.as_deref(),
        Some("RUN_DEADLOCK")
    );
    let events = events(reader, &run_id);
    assert_eq!(count_kind(&events, "RunFinished"), 1);
    assert_eq!(count_kind(&events, "NodeAbandoned"), 0);
    assert_eq!(events.last().expect("last").payload["status"], "Failed");
    assert_eq!(
        events.last().expect("last").payload["error"]["code"],
        "RUN_DEADLOCK"
    );
}
