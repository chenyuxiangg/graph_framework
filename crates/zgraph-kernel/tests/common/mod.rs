//! 内核集成测试公共辅助：fixtures 只读加载、图/run 构造、sink 断言。
//!
//! fixtures 路径规则（AGENTS §2）：`CARGO_MANIFEST_DIR/../../tests/fixtures/graphs/…`。

#![allow(dead_code)]

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use serde_json::{json, Value};
use zgraph_dsl::load_graph;
use zgraph_kernel::{
    run_to_completion, CompiledGraph, InstanceSnapshot, InstanceStatus, MemoryTraceSink,
    NodeExecutor, NodeId, PortName, RunOptions, RunOutcome, RunSpec,
};

/// 仓库根目录（`crates/zgraph-kernel` 上两级）。
pub fn repo_root() -> PathBuf {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    path.canonicalize().expect("repo root resolves")
}

/// 读取 `tests/fixtures/graphs/<name>` 原始字节。
pub fn fixture_bytes(name: &str) -> Vec<u8> {
    let path = repo_root().join("tests/fixtures/graphs").join(name);
    std::fs::read(&path).unwrap_or_else(|error| panic!("fixture {name} readable: {error}"))
}

/// 编译 fixture 图。
pub fn compile_fixture(name: &str) -> Arc<CompiledGraph> {
    compile_bytes(&fixture_bytes(name))
}

/// 从 JSON 文本编译图。
pub fn compile_text(text: &str) -> Arc<CompiledGraph> {
    compile_bytes(text.as_bytes())
}

/// 从 JSON 字节编译图（加载 + `CompiledGraph::compile`）。
pub fn compile_bytes(bytes: &[u8]) -> Arc<CompiledGraph> {
    let loaded = load_graph(bytes).expect("graph should pass DSL validation");
    CompiledGraph::compile(&loaded).expect("graph should compile")
}

/// 构造入口注入（`&[(node, port, value)]`）。
pub fn entry_inputs(pairs: &[(&str, &str, Value)]) -> HashMap<NodeId, HashMap<PortName, Value>> {
    let mut map: HashMap<NodeId, HashMap<PortName, Value>> = HashMap::new();
    for (node, port, value) in pairs {
        map.entry((*node).to_string())
            .or_default()
            .insert((*port).to_string(), value.clone());
    }
    map
}

/// 默认选项 + 给定入口注入的 `RunSpec`。
pub fn spec(
    graph: Arc<CompiledGraph>,
    entry_inputs: HashMap<NodeId, HashMap<PortName, Value>>,
) -> RunSpec {
    RunSpec {
        graph,
        entry_inputs,
        user_id: "u1".to_string(),
        global_vars: HashMap::new(),
        options: RunOptions::default(),
    }
}

/// 以 `Default` 为基线构造选项，再交给 `configure` 修改。
///
/// 用闭包而非"先 `default()` 再逐字段赋值"，既避免测试里的
/// `clippy::field_reassign_with_default`，也兼容 `test-hooks` 下的私有字段。
pub fn options_with(configure: impl FnOnce(&mut RunOptions)) -> RunOptions {
    let mut options = RunOptions::default();
    configure(&mut options);
    options
}

/// 带选项的 `RunSpec`。
pub fn spec_with(
    graph: Arc<CompiledGraph>,
    entry_inputs: HashMap<NodeId, HashMap<PortName, Value>>,
    options: RunOptions,
) -> RunSpec {
    RunSpec {
        graph,
        entry_inputs,
        user_id: "u1".to_string(),
        global_vars: HashMap::new(),
        options,
    }
}

/// 运行到结束（启动失败视为测试失败）。
pub async fn run(
    spec: RunSpec,
    executor: Arc<dyn NodeExecutor>,
    sink: Arc<MemoryTraceSink>,
) -> RunOutcome {
    run_to_completion(None, spec, executor, sink)
        .await
        .expect("run should start")
}

/// 运行并返回（outcome, sink）。
pub async fn run_with_sink(
    spec: RunSpec,
    executor: Arc<dyn NodeExecutor>,
) -> (RunOutcome, Arc<MemoryTraceSink>) {
    let sink = Arc::new(MemoryTraceSink::new());
    let outcome = run(spec, executor, sink.clone()).await;
    (outcome, sink)
}

/// 线性图：`start(x required) -> work(x required) -> done(r required)`。
pub const LINEAR_JSON: &str = r#"{
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

/// 线性图 + 默认入口注入 `x = {"v": 1}`。
pub fn linear_spec() -> RunSpec {
    spec(
        compile_text(LINEAR_JSON),
        entry_inputs(&[("start", "x", json!({"v": 1}))]),
    )
}

/// 串行链图：`start -> a -> b -> done`（a/b 均为 python_node）。
pub const CHAIN_JSON: &str = r#"{
  "version": "1.0",
  "metadata": {"name": "chain"},
  "entry_points": ["start"],
  "nodes": [
    {"id": "start", "type": "start", "inputs": [], "config": {}},
    {"id": "a", "type": "python_node",
     "inputs": [{"name": "i", "type": "any", "required": true}],
     "config": {"module": "workers.chain", "function": "a"}},
    {"id": "b", "type": "python_node",
     "inputs": [{"name": "i", "type": "any", "required": true}],
     "config": {"module": "workers.chain", "function": "b"}},
    {"id": "done", "type": "end",
     "inputs": [{"name": "r", "type": "any", "required": true}], "config": {}}
  ],
  "edges": [
    {"from": "start", "to_node": "a", "to_input_port": "i"},
    {"from": "a", "to_node": "b", "to_input_port": "i"},
    {"from": "b", "to_node": "done", "to_input_port": "r"}
  ]
}"#;

/// 串行链图 + 空入口注入的 `RunSpec`。
pub fn chain_spec() -> RunSpec {
    spec(compile_text(CHAIN_JSON), entry_inputs(&[]))
}

/// 线性图，但把节点 `work` 的 DSL 超时覆盖为 `timeout_ms`。
pub fn linear_spec_with_work_timeout(timeout_ms: u64) -> RunSpec {
    let text = LINEAR_JSON.replace(
        r#""config": {"module": "workers.demo", "function": "run"}"#,
        &format!(
            r#""config": {{"module": "workers.demo", "function": "run", "timeout_ms": {timeout_ms}}}"#
        ),
    );
    spec(
        compile_text(&text),
        entry_inputs(&[("start", "x", json!({"v": 1}))]),
    )
}

/// 某节点最后一条快照的状态。
pub fn last_status(sink: &MemoryTraceSink, node_id: &str) -> Option<InstanceStatus> {
    sink.snapshots()
        .into_iter()
        .rev()
        .find(|snapshot| snapshot.node_id == node_id)
        .map(|snapshot| snapshot.status)
}

/// 某节点的全部快照状态序列。
pub fn statuses(sink: &MemoryTraceSink, node_id: &str) -> Vec<InstanceStatus> {
    sink.snapshots()
        .into_iter()
        .filter(|snapshot| snapshot.node_id == node_id)
        .map(|snapshot| snapshot.status)
        .collect()
}

/// 某节点第一次进入 Running 时的 input 快照。
pub fn running_input(sink: &MemoryTraceSink, node_id: &str) -> Option<Value> {
    sink.snapshots()
        .into_iter()
        .find(|snapshot| snapshot.node_id == node_id && snapshot.status == InstanceStatus::Running)
        .and_then(|snapshot| snapshot.input)
}

/// 某节点的全部快照。
pub fn snapshots_of(sink: &MemoryTraceSink, node_id: &str) -> Vec<InstanceSnapshot> {
    sink.snapshots()
        .into_iter()
        .filter(|snapshot| snapshot.node_id == node_id)
        .collect()
}

/// 轮询等待某节点进入 Running（超时 panic）。
pub async fn wait_for_running(sink: &MemoryTraceSink, node_id: &str) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    loop {
        if sink.snapshots().iter().any(|snapshot| {
            snapshot.node_id == node_id && snapshot.status == InstanceStatus::Running
        }) {
            return;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "node {node_id} did not reach Running in time"
        );
        tokio::time::sleep(Duration::from_millis(2)).await;
    }
}

/// 轮询等待 run 状态变为 `status`。
pub async fn wait_for_run_status(
    rx: &mut tokio::sync::watch::Receiver<zgraph_kernel::RunStatus>,
    status: zgraph_kernel::RunStatus,
) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    loop {
        if *rx.borrow() == status {
            return;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "run status did not become {status:?} in time (current: {:?})",
            *rx.borrow()
        );
        rx.changed().await.expect("status channel alive");
    }
}

/// 断言 `RunFinished` 恰好一条且 `RunStarted` 恰好一条（I5）。
pub fn assert_lifecycle_once(sink: &MemoryTraceSink) {
    assert_eq!(sink.started_infos().len(), 1, "RunStarted exactly once");
    assert_eq!(sink.finished().len(), 1, "RunFinished exactly once");
}
