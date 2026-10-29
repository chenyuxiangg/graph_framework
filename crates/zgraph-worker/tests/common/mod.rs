//! worker M3 集成测试公共辅助：fixtures 路径、worker/supervisor 配置、图构造。

#![allow(dead_code)]

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use serde_json::{json, Value};
use zgraph_dsl::load_graph;
use zgraph_kernel::{
    run_to_completion, CompiledGraph, MemoryTraceSink, NodeExecutor, NodeId, PortName, RunOptions,
    RunOutcome, RunSpec,
};
use zgraph_worker::{SupervisorConfig, WorkerConfig, WorkerExecutor};

/// fixture 等待 pid 消失的上限（D1：超时 kill 后 5s 内无残留）。
pub const PID_GONE_TIMEOUT: Duration = Duration::from_secs(5);

/// 仓库根目录（`crates/zgraph-worker` 上两级）。
pub fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("repo root resolves")
}

/// worker 测试 fixture 根目录。
pub fn fixtures_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

/// Python 解释器（可用 `ZGRAPH_PYTHON` 覆盖）。
pub fn python_program() -> String {
    std::env::var("ZGRAPH_PYTHON").unwrap_or_else(|_| "python3".to_string())
}

/// 集成测试用 `PYTHONPATH`：worker SDK + 用户代码 fixture。
pub fn python_path() -> String {
    let sdk = repo_root().join("python");
    let user = fixtures_root().join("py");
    format!("{}:{}", sdk.display(), user.display())
}

/// 以 Python worker SDK 为入口的 M0 兼容配置。
pub fn worker_config(timeout: Duration) -> WorkerConfig {
    WorkerConfig {
        program: python_program(),
        args: vec!["-m".to_string(), "zgraph_worker.runtime".to_string()],
        env: vec![("PYTHONPATH".to_string(), python_path())],
        cwd: Some(repo_root()),
        timeout,
        hello_timeout: Duration::from_secs(5),
        exit_grace: Duration::from_secs(1),
        max_stderr_bytes: 8 * 1024,
    }
}

/// M3 supervisor 配置（默认宽限；测试按需覆盖）。
pub fn supervisor_config(timeout: Duration) -> SupervisorConfig {
    SupervisorConfig::from_worker(worker_config(timeout))
}

/// 直接 `python <script>` 的 raw 协议 fixture 配置（绕过 SDK）。
pub fn script_config(script: &str, env: Vec<(String, String)>, timeout: Duration) -> WorkerConfig {
    let path = fixtures_root().join("workers").join(script);
    let mut config = worker_config(timeout);
    config.args = vec![path.display().to_string()];
    config.env = vec![("PYTHONPATH".to_string(), python_path())];
    config.env.extend(env);
    config
}

/// 测试用临时 pid 文件三元组（自身 / 孙进程 / 通用日志）。
#[derive(Debug)]
pub struct PidPaths {
    /// 临时目录（保持存活到测试结束）。
    pub dir: tempfile::TempDir,
    /// worker 自身 pid 文件。
    pub worker: PathBuf,
    /// 孙进程 pid 文件。
    pub child: PathBuf,
    /// 执行区间日志（并发测试）。
    pub log: PathBuf,
}

impl PidPaths {
    /// 在独立 tempdir 下创建三个路径。
    pub fn new() -> Self {
        let dir = tempfile::tempdir().expect("tempdir");
        let worker = dir.path().join("worker.pid");
        let child = dir.path().join("child.pid");
        let log = dir.path().join("intervals.log");
        Self {
            dir,
            worker,
            child,
            log,
        }
    }

    /// 注入到 fixture 的环境变量。
    pub fn env(&self) -> Vec<(String, String)> {
        vec![
            (
                "ZGRAPH_FIXTURE_PID_FILE".to_string(),
                self.worker.display().to_string(),
            ),
            (
                "ZGRAPH_FIXTURE_CHILD_PID_FILE".to_string(),
                self.child.display().to_string(),
            ),
        ]
    }
}

impl Default for PidPaths {
    fn default() -> Self {
        Self::new()
    }
}

/// 读 pid 文件（等待 fixture 写入，最长 5s）。
pub fn read_pid(path: &std::path::Path) -> String {
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    loop {
        if let Ok(text) = std::fs::read_to_string(path) {
            let pid = text.trim().to_string();
            if !pid.is_empty() {
                return pid;
            }
        }
        assert!(
            std::time::Instant::now() < deadline,
            "pid file {} not written in time",
            path.display()
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// pid 是否已被回收（`kill -0` 失败即消失；stderr 静音避免测试噪声）。
pub fn pid_alive(pid: &str) -> bool {
    std::process::Command::new("kill")
        .args(["-0", pid])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

/// 断言 pid 在 `timeout` 内消失（僵尸/未 reap 都会失败）。
///
/// **必须 async**：tokio `kill_on_drop` 的子进程回收由 runtime 驱动；
/// 同步 `thread::sleep` 会阻塞 current-thread runtime，僵尸永远收不掉。
pub async fn assert_pid_gone(pid: &str, timeout: Duration) {
    let deadline = tokio::time::Instant::now() + timeout;
    while tokio::time::Instant::now() < deadline {
        if !pid_alive(pid) {
            return;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("pid {pid} still alive after {timeout:?} (zombie or leaked child)");
}

/// 构造线性图 `start(x) -> work(x) -> done(r)`。
pub fn linear_graph(module: &str, function: &str, timeout_ms: Option<u64>) -> Arc<CompiledGraph> {
    let mut config = json!({"module": module, "function": function});
    if let Some(timeout) = timeout_ms {
        config["timeout_ms"] = json!(timeout);
    }
    let text = json!({
        "version": "1.0",
        "metadata": {"name": "worker_linear"},
        "entry_points": ["start"],
        "nodes": [
            {"id": "start", "type": "start",
             "inputs": [{"name": "x", "type": "any", "required": true}],
             "config": {}},
            {"id": "work", "type": "python_node",
             "inputs": [{"name": "x", "type": "any", "required": true}],
             "config": config},
            {"id": "done", "type": "end",
             "inputs": [{"name": "r", "type": "any", "required": true}],
             "config": {}}
        ],
        "edges": [
            {"from": "start", "to_node": "work", "to_input_port": "x"},
            {"from": "work", "to_node": "done", "to_input_port": "r"}
        ]
    })
    .to_string();
    compile_text(&text)
}

/// 构造并行图：`start -> a / b -> done`（两个分支并行就绪）。
pub fn parallel_graph(module: &str, function_a: &str, function_b: &str) -> Arc<CompiledGraph> {
    let text = json!({
        "version": "1.0",
        "metadata": {"name": "worker_parallel"},
        "entry_points": ["start"],
        "nodes": [
            {"id": "start", "type": "start",
             "inputs": [{"name": "x", "type": "any", "required": true}],
             "config": {}},
            {"id": "a", "type": "python_node",
             "inputs": [{"name": "x", "type": "any", "required": true}],
             "config": {"module": module, "function": function_a}},
            {"id": "b", "type": "python_node",
             "inputs": [{"name": "x", "type": "any", "required": true}],
             "config": {"module": module, "function": function_b}},
            {"id": "done", "type": "end",
             "inputs": [
                 {"name": "ra", "type": "any", "required": true},
                 {"name": "rb", "type": "any", "required": true}
             ],
             "config": {}}
        ],
        "edges": [
            {"from": "start", "to_node": "a", "to_input_port": "x"},
            {"from": "start", "to_node": "b", "to_input_port": "x"},
            {"from": "a", "to_node": "done", "to_input_port": "ra"},
            {"from": "b", "to_node": "done", "to_input_port": "rb"}
        ]
    })
    .to_string();
    compile_text(&text)
}

/// 从 JSON 文本加载 DSL 并编译（校验失败视为测试失败）。
pub fn compile_text(text: &str) -> Arc<CompiledGraph> {
    let loaded = load_graph(text.as_bytes()).expect("graph should pass DSL validation");
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

/// 线性图 + 给定输入的 `RunSpec`（seed 固定，验证 D5 透传）。
pub fn linear_spec(graph: Arc<CompiledGraph>, input: Value) -> RunSpec {
    let options = RunOptions {
        seed: 7,
        ..RunOptions::default()
    };
    RunSpec {
        graph,
        entry_inputs: entry_inputs(&[("start", "x", input)]),
        user_id: "u-worker".to_string(),
        global_vars: HashMap::new(),
        options,
    }
}

/// 构造 worker 执行器（默认并发上限）。
pub fn executor(timeout: Duration) -> Arc<WorkerExecutor> {
    Arc::new(WorkerExecutor::new(supervisor_config(timeout)))
}

/// 跑完一个 run，返回 outcome 与 memory sink。
pub async fn run_graph(
    spec: RunSpec,
    executor: Arc<dyn NodeExecutor>,
) -> (RunOutcome, Arc<MemoryTraceSink>) {
    let sink = Arc::new(MemoryTraceSink::new());
    let outcome = run_to_completion(None, spec, executor, sink.clone())
        .await
        .expect("run should start");
    (outcome, sink)
}
