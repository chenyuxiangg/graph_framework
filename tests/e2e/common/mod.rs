//! `tests/e2e` 公共辅助：**真** kernel run + **真** Python worker 子进程 + **真** SQLite trace。
//!
//! 数据通路（禁止 mock）：
//!
//! ```text
//! 内联 DSL 图 / tests/fixtures/graphs → CompiledGraph::compile → kernel start_run
//!   → zgraph_cli::bridge::TraceBridge → zgraph-trace SQLite → TraceReader
//!   → WorkerExecutor → python3 -m zgraph_worker.runtime（真实子进程）
//! ```
//!
//! 稳定性约定（AGENTS §8，禁止为变绿弱化断言）：
//!
//! - 时序断言只用「轮询 + 宽松上限」，不用 `sleep` 猜时序；
//! - 每个用例一个独立 `tempfile::TempDir`（trace DB 与 pid/日志文件互不干扰）；
//! - Python 解释器优先取环境变量 `ZGRAPH_PYTHON`，缺省 `python3`（CI 同为 `python3`）；
//! - `PYTHONPATH` 同时包含仓库 `python/`（SDK + 示例 worker）与本包 `fixtures/py`
//!   （e2e 专用 fixture worker），保证 import 路径与真实用户一致。
//!
//! 进程断言：超时/取消的判定必须落到**进程组 kill 后 pid 消失**（D1），
//! 因此提供 [`read_pid`] / [`pid_alive`] / [`assert_pid_gone`]；`kill(pid, 0)` 用
//! `nix` 直接调用（不 spawn `kill` 进程，轮询更省）。仅 Unix：v2 的进程组语义
//! 依赖 `setsid`/`kill(-pgid)`，测试文件统一 `#![cfg(unix)]`。

#![allow(dead_code)]

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use serde_json::{json, Value};

use zgraph_cli::bridge::{run_in_process, BridgeOutcome};
use zgraph_dsl::load_graph;
use zgraph_kernel::{CompiledGraph, NodeId, PortName, RunOptions, RunSpec};
use zgraph_trace::{
    EventQuery, EventRow, InstanceRow, InstanceStatus as TraceInstanceStatus, TraceReader,
};
use zgraph_worker::{SupervisorConfig, WorkerConfig, WorkerExecutor};

// ---------------------------------------------------------------------------
// 常量
// ---------------------------------------------------------------------------

/// 单次 run 的等待上限（真 worker 冷启动 + CI 慢机余量）。
pub const RUN_TIMEOUT: Duration = Duration::from_secs(30);

/// 超时/取消后断言 pid 消失的上限（D1：kill 后 5s 内无残留）。
pub const PID_GONE_TIMEOUT: Duration = Duration::from_secs(5);

/// 等待 pid 文件/日志落盘的上限（fixture 同步用）。
pub const FILE_TIMEOUT: Duration = Duration::from_secs(5);

// ---------------------------------------------------------------------------
// 路径与环境
// ---------------------------------------------------------------------------

/// 仓库根（本包位于 `<root>/tests/e2e`）。
pub fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("repo root resolves")
}

/// e2e fixture 根目录（`tests/e2e/fixtures`）。
pub fn fixtures_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures")
}

/// 仓库级验收图目录（`tests/fixtures/graphs`）。
pub fn graphs_root() -> PathBuf {
    repo_root().join("tests/fixtures/graphs")
}

/// Python 解释器：`ZGRAPH_PYTHON` 优先，缺省 `python3`。
pub fn python_program() -> String {
    std::env::var("ZGRAPH_PYTHON").unwrap_or_else(|_| "python3".to_string())
}

/// 集成测试 `PYTHONPATH`：仓库 `python/`（SDK + `workers/*` 示例）+ e2e fixture 包。
pub fn python_path() -> String {
    format!(
        "{}:{}",
        repo_root().join("python").display(),
        fixtures_root().join("py").display()
    )
}

// ---------------------------------------------------------------------------
// worker / executor 构造
// ---------------------------------------------------------------------------

/// Python 子进程环境变量名：禁止写 `.pyc`/`__pycache__`（ISSUE-089）。
///
/// e2e 运行会 import `tests/fixtures/**` 下的 Python 资产；不设该变量时 Python 会在
/// fixtures 目录写 `__pycache__/`（`.gitignore` 已忽略、git 语义不脏，但本地工作区
/// 会多出忽略文件）。所有 e2e 派生的 Python 子进程（worker / tool runner / CLI 的
/// 后代）都通过此变量统一关闭字节码缓存。
pub const PYTHONDONTWRITEBYTECODE: &str = "PYTHONDONTWRITEBYTECODE";

/// 以 Python worker SDK（`python -m zgraph_worker.runtime`）为入口的基础配置。
pub fn worker_config(timeout: Duration) -> WorkerConfig {
    WorkerConfig {
        program: python_program(),
        args: vec!["-m".to_string(), "zgraph_worker.runtime".to_string()],
        env: vec![
            ("PYTHONPATH".to_string(), python_path()),
            (PYTHONDONTWRITEBYTECODE.to_string(), "1".to_string()),
        ],
        cwd: Some(repo_root()),
        timeout,
        hello_timeout: Duration::from_secs(5),
        exit_grace: Duration::from_secs(1),
        max_stderr_bytes: 8 * 1024,
    }
}

/// supervisor 配置（默认宽限；需要注入故障时按需覆盖字段）。
pub fn supervisor_config(timeout: Duration) -> SupervisorConfig {
    SupervisorConfig::from_worker(worker_config(timeout))
}

/// 默认并发上限的真 worker 执行器。
pub fn executor(timeout: Duration) -> Arc<WorkerExecutor> {
    Arc::new(WorkerExecutor::new(supervisor_config(timeout)))
}

/// 指定全局/每 run 并发上限的真 worker 执行器（D4）。
pub fn executor_with_limits(
    timeout: Duration,
    max_workers: usize,
    max_workers_per_run: usize,
) -> Arc<WorkerExecutor> {
    Arc::new(WorkerExecutor::with_limits(
        supervisor_config(timeout),
        max_workers,
        max_workers_per_run,
    ))
}

/// 「直接 `python <raw 脚本>`」的 supervisor 配置（绕过 SDK，注入协议故障）。
///
/// 脚本在 `tests/e2e/fixtures/workers/`；`hello_timeout` 用于握手超时反例。
pub fn script_supervisor_config(
    script: &str,
    timeout: Duration,
    hello_timeout: Duration,
) -> SupervisorConfig {
    let path = fixtures_root().join("workers").join(script);
    let mut config = worker_config(timeout);
    config.args = vec![path.display().to_string()];
    config.hello_timeout = hello_timeout;
    SupervisorConfig::from_worker(config)
}

/// raw NDJSON fixture 脚本的执行器（协议故障用例）。
pub fn script_executor(
    script: &str,
    timeout: Duration,
    hello_timeout: Duration,
) -> Arc<WorkerExecutor> {
    Arc::new(WorkerExecutor::new(script_supervisor_config(
        script,
        timeout,
        hello_timeout,
    )))
}

// ---------------------------------------------------------------------------
// 图与 run 构造
// ---------------------------------------------------------------------------

/// 从 JSON 文本加载 DSL 并编译（任何校验/编译失败都视为测试失败）。
pub fn compile_text(text: &str) -> Arc<CompiledGraph> {
    let loaded = load_graph(text.as_bytes()).expect("graph should pass DSL validation");
    CompiledGraph::compile(&loaded).expect("graph should compile")
}

/// 从 `tests/fixtures/graphs/<name>` 加载并编译。
pub fn compile_fixture(name: &str) -> Arc<CompiledGraph> {
    let path = graphs_root().join(name);
    let bytes =
        std::fs::read(&path).unwrap_or_else(|error| panic!("fixture {}: {error}", path.display()));
    let loaded = load_graph(&bytes).expect("fixture graph should pass DSL validation");
    CompiledGraph::compile(&loaded).expect("fixture graph should compile")
}

/// 入口注入（`&[(node, port, value)]`）。
pub fn entry_inputs(pairs: &[(&str, &str, Value)]) -> HashMap<NodeId, HashMap<PortName, Value>> {
    let mut map: HashMap<NodeId, HashMap<PortName, Value>> = HashMap::new();
    for (node, port, value) in pairs {
        map.entry((*node).to_string())
            .or_default()
            .insert((*port).to_string(), value.clone());
    }
    map
}

/// 默认选项的 `RunSpec`。
pub fn spec(
    graph: Arc<CompiledGraph>,
    inputs: HashMap<NodeId, HashMap<PortName, Value>>,
) -> RunSpec {
    RunSpec {
        graph,
        entry_inputs: inputs,
        user_id: "u-e2e".to_string(),
        global_vars: HashMap::new(),
        options: RunOptions::default(),
    }
}

/// 从 `RunOptions::default()` 基线修改选项（兼容 `test-hooks` 特性合并带来的额外字段）。
pub fn spec_with(
    graph: Arc<CompiledGraph>,
    inputs: HashMap<NodeId, HashMap<PortName, Value>>,
    configure: impl FnOnce(&mut RunOptions),
) -> RunSpec {
    let mut options = RunOptions::default();
    configure(&mut options);
    RunSpec {
        graph,
        entry_inputs: inputs,
        user_id: "u-e2e".to_string(),
        global_vars: HashMap::new(),
        options,
    }
}

/// `start(x) -> work(x) -> done(r)`：单 `python_node` 线性图。
///
/// `work` 的输入端口 `x` 收到 start 的**整包 output**（v1 广播，04 §4.5；端口名与
/// 字段名一致时 fixture 可直接取值）。`timeout_ms` 写入 python_node config（覆盖默认
/// 180s 节点超时）。
pub fn linear_graph(module: &str, function: &str, timeout_ms: Option<u64>) -> Arc<CompiledGraph> {
    let mut config = json!({"module": module, "function": function});
    if let Some(timeout) = timeout_ms {
        config["timeout_ms"] = json!(timeout);
    }
    let text = json!({
        "version": "1.0",
        "metadata": {"name": "e2e_linear"},
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

/// `start(seconds) -> work(seconds) -> done(r)`：慢节点线性图（`workers.slow` 等）。
///
/// 端口名与 `workers.slow` 的字段名对齐，使示例 worker 的 `unwrap_port` 能从
/// start 的整包 output 里取到 `seconds`（AC-06 超时）。
///
/// 注意：DSL R6 禁止同一 `(from, to)` 的重复边，因此这里只接一个端口；
/// 「worker/孙进程 pid 消失」的进程组断言由 fixtures 用例（`e2e_fixtures.descendants`）覆盖。
pub fn sleep_seconds_graph(
    module: &str,
    function: &str,
    timeout_ms: Option<u64>,
) -> Arc<CompiledGraph> {
    let mut config = json!({"module": module, "function": function});
    if let Some(timeout) = timeout_ms {
        config["timeout_ms"] = json!(timeout);
    }
    let text = json!({
        "version": "1.0",
        "metadata": {"name": "e2e_sleep_seconds"},
        "entry_points": ["start"],
        "nodes": [
            {"id": "start", "type": "start",
             "inputs": [{"name": "seconds", "type": "number", "required": true}],
             "config": {}},
            {"id": "work", "type": "python_node",
             "inputs": [{"name": "seconds", "type": "number", "required": true}],
             "config": config},
            {"id": "done", "type": "end",
             "inputs": [{"name": "r", "type": "any", "required": true}],
             "config": {}}
        ],
        "edges": [
            {"from": "start", "to_node": "work", "to_input_port": "seconds"},
            {"from": "work", "to_node": "done", "to_input_port": "r"}
        ]
    })
    .to_string();
    compile_text(&text)
}

/// `start(x) -> first(x) -> second(x) -> done(r)`：两节点串行图（AC-07/AC-10 暂停）。
///
/// fixture `e2e_fixtures.steps.first/second` 收到 start 整包 output 后原样透传，
/// 因此两个节点都能从 payload 读到 `sleep`/`*_pid_file`。
pub fn two_step_graph(
    module: &str,
    first_function: &str,
    second_function: &str,
) -> Arc<CompiledGraph> {
    let text = json!({
        "version": "1.0",
        "metadata": {"name": "e2e_two_step"},
        "entry_points": ["start"],
        "nodes": [
            {"id": "start", "type": "start",
             "inputs": [{"name": "x", "type": "any", "required": true}],
             "config": {}},
            {"id": "first", "type": "python_node",
             "inputs": [{"name": "x", "type": "any", "required": true}],
             "config": {"module": module, "function": first_function}},
            {"id": "second", "type": "python_node",
             "inputs": [{"name": "x", "type": "any", "required": true}],
             "config": {"module": module, "function": second_function}},
            {"id": "done", "type": "end",
             "inputs": [{"name": "r", "type": "any", "required": true}],
             "config": {}}
        ],
        "edges": [
            {"from": "start", "to_node": "first", "to_input_port": "x"},
            {"from": "first", "to_node": "second", "to_input_port": "x"},
            {"from": "second", "to_node": "done", "to_input_port": "r"}
        ]
    })
    .to_string();
    compile_text(&text)
}

// ---------------------------------------------------------------------------
// run 入口
// ---------------------------------------------------------------------------

/// 打开 trace DB → 启动 run → 等待结束 → flush/close → 返回 outcome + 可查询 reader。
pub async fn run_to_trace(
    spec: RunSpec,
    executor: Arc<dyn zgraph_kernel::NodeExecutor>,
    db: &Path,
) -> BridgeOutcome {
    run_in_process(spec, executor, db)
        .await
        .expect("traced run should start and finish")
}

// ---------------------------------------------------------------------------
// trace 查询辅助
// ---------------------------------------------------------------------------

/// 某 run 的全部事件（按 seq 升序，含 Streaming）。
pub fn events(reader: &TraceReader, run_id: &str) -> Vec<EventRow> {
    reader
        .events(run_id, &EventQuery::default())
        .expect("query events")
}

/// 统计某类事件条数。
pub fn count_kind(events: &[EventRow], kind: &str) -> usize {
    events.iter().filter(|event| event.kind == kind).count()
}

/// 按节点 id 取实例行（取第一条；自环多 attempt 的用例直接用 `TraceReader::instances`）。
pub fn instance(reader: &TraceReader, run_id: &str, node_id: &str) -> InstanceRow {
    reader
        .instances(run_id)
        .expect("query instances")
        .into_iter()
        .find(|row| row.node_id == node_id)
        .unwrap_or_else(|| panic!("instance for node {node_id}"))
}

/// 按节点 id 取实例终态（取该节点最后一次快照）。
pub fn instance_status(reader: &TraceReader, run_id: &str, node_id: &str) -> TraceInstanceStatus {
    reader
        .instances(run_id)
        .expect("query instances")
        .into_iter()
        .rfind(|row| row.node_id == node_id)
        .unwrap_or_else(|| panic!("instance for node {node_id}"))
        .status
}

/// run 行（含 error_code / metrics / event_count）。
pub fn run_row(reader: &TraceReader, run_id: &str) -> zgraph_trace::RunRow {
    reader
        .run(run_id)
        .expect("query run")
        .expect("run row exists")
}

/// 生命周期形状：`RunStarted` 首、`RunFinished` 尾，各恰一次，seq 严格递增。
pub fn assert_lifecycle_shape(reader: &TraceReader, run_id: &str) {
    let all = events(reader, run_id);
    assert_eq!(
        all.first().map(|event| event.kind.as_str()),
        Some("RunStarted"),
        "first event must be RunStarted"
    );
    assert_eq!(
        all.last().map(|event| event.kind.as_str()),
        Some("RunFinished"),
        "last event must be RunFinished"
    );
    assert_eq!(count_kind(&all, "RunStarted"), 1, "RunStarted exactly once");
    assert_eq!(
        count_kind(&all, "RunFinished"),
        1,
        "RunFinished exactly once"
    );
    let seqs: Vec<i64> = all.iter().map(|event| event.seq).collect();
    assert!(
        seqs.windows(2).all(|pair| pair[0] < pair[1]),
        "seq strictly increasing: {seqs:?}"
    );
}

/// `RunFinished` 事件的 payload（断言状态/错误码/指标）。
pub fn run_finished_payload(reader: &TraceReader, run_id: &str) -> Value {
    events(reader, run_id)
        .into_iter()
        .rev()
        .find(|event| event.kind == "RunFinished")
        .map(|event| event.payload)
        .expect("RunFinished event")
}

/// 轮询等待条件成立（异步，避免阻塞 current-thread runtime）。
pub async fn wait_until(deadline: Duration, mut condition: impl FnMut() -> bool) -> bool {
    let started = std::time::Instant::now();
    while started.elapsed() <= deadline {
        if condition() {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    condition()
}

// ---------------------------------------------------------------------------
// PID 断言（D1：进程组 kill 后无残留，含孙进程）
// ---------------------------------------------------------------------------

/// worker 自身 + 孙进程的 pid 文件对（目录保持存活到测试结束）。
#[derive(Debug)]
pub struct PidPaths {
    /// 临时目录（drop 时清理，保证 pid 文件不跨用例串）。
    pub dir: tempfile::TempDir,
    /// worker 自身 pid 文件。
    pub worker: PathBuf,
    /// 孙进程 pid 文件。
    pub child: PathBuf,
}

impl PidPaths {
    /// 在独立 tempdir 下创建两个路径。
    pub fn new() -> Self {
        let dir = tempfile::tempdir().expect("tempdir");
        let worker = dir.path().join("worker.pid");
        let child = dir.path().join("child.pid");
        Self { dir, worker, child }
    }
}

/// 读 pid 文件（同步轮询等待 fixture 写入，最长 [`FILE_TIMEOUT`]）。
///
/// 同步实现会阻塞 current-thread runtime：只用于「文件已由 fixture 写出」或 run 已结束
/// 的场景；等待文件出现请用异步 [`wait_for_file`]。
pub fn read_pid(path: &Path) -> i32 {
    let started = std::time::Instant::now();
    loop {
        if let Ok(text) = std::fs::read_to_string(path) {
            let trimmed = text.trim();
            if !trimmed.is_empty() {
                return trimmed
                    .parse::<i32>()
                    .unwrap_or_else(|error| panic!("pid file {}: {error}", path.display()));
            }
        }
        assert!(
            started.elapsed() < FILE_TIMEOUT,
            "pid file {} not written in time",
            path.display()
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// pid 是否仍存活（`kill(pid, 0)`：ESRCH 即消失；僵尸/未 reap 仍视为存活）。
pub fn pid_alive(pid: i32) -> bool {
    nix::sys::signal::kill(nix::unistd::Pid::from_raw(pid), None).is_ok()
}

/// 断言 pid 在 `timeout` 内消失（轮询；禁止 sleep 猜测）。
///
/// **必须 async**：tokio `kill_on_drop` 的子进程回收由 runtime 驱动，同步
/// `thread::sleep` 会阻塞 current-thread runtime 导致僵尸永远收不掉。
pub async fn assert_pid_gone(pid: i32, timeout: Duration) {
    let deadline = tokio::time::Instant::now() + timeout;
    while tokio::time::Instant::now() < deadline {
        if !pid_alive(pid) {
            return;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("pid {pid} still alive after {timeout:?} (zombie or leaked child)");
}

/// 轮询等待文件出现（fixture 同步用，最长 [`FILE_TIMEOUT`]）。
pub async fn wait_for_file(path: &Path) {
    let deadline = tokio::time::Instant::now() + FILE_TIMEOUT;
    while tokio::time::Instant::now() < deadline {
        if path.exists() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("{} not created in time", path.display());
}

// ---------------------------------------------------------------------------
// 进程区间并发度量（AC-09/AC-12；fixture 追加 `<run_tag> <tag> <pid> <wall> start|end`）
// ---------------------------------------------------------------------------

/// 解析区间日志为 `(run_tag, start, end)`（秒，同主机墙钟）。
///
/// 每行 `<run_tag> <tag> <pid> <wall-clock> start|end`；配对键
/// `(run_tag, tag, pid)`——一个节点实例一个进程、只写一次 start/end；带 `run_tag`
/// 后即使跨 run PID 复用也不会错配（REV-015 #10）。非 5 字段行忽略。
fn parse_interval_lines(path: &Path) -> Vec<(String, f64, f64)> {
    let text = std::fs::read_to_string(path)
        .unwrap_or_else(|error| panic!("interval log {}: {error}", path.display()));
    let mut starts: HashMap<String, f64> = HashMap::new();
    let mut intervals = Vec::new();
    for line in text.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() != 5 {
            continue;
        }
        let run_tag = parts[0].to_string();
        let key = format!("{}/{}/{}", parts[0], parts[1], parts[2]);
        let timestamp: f64 = parts[3].parse().expect("interval timestamp");
        match parts[4] {
            "start" => {
                starts.insert(key, timestamp);
            }
            "end" => {
                if let Some(started) = starts.remove(&key) {
                    intervals.push((run_tag, started, timestamp));
                }
            }
            other => panic!("unexpected interval log field {other}"),
        }
    }
    intervals
}

/// 从共享区间日志读出全部执行区间 `(start, end)`。
pub fn read_intervals(path: &Path) -> Vec<(f64, f64)> {
    parse_interval_lines(path)
        .into_iter()
        .map(|(_, start, end)| (start, end))
        .collect()
}

/// 按 `run_tag` 分组的执行区间（多 run 隔离/每 run 上限度量）。
pub fn read_intervals_by_run(path: &Path) -> BTreeMap<String, Vec<(f64, f64)>> {
    let mut grouped: BTreeMap<String, Vec<(f64, f64)>> = BTreeMap::new();
    for (run_tag, start, end) in parse_interval_lines(path) {
        grouped.entry(run_tag).or_default().push((start, end));
    }
    grouped
}

/// 区间列表的峰值并发数（同一时刻先结算 end 再算 start）。
pub fn peak_concurrency(intervals: &[(f64, f64)]) -> usize {
    let mut events: Vec<(f64, i32)> = Vec::with_capacity(intervals.len() * 2);
    for (start, end) in intervals {
        events.push((*start, 1));
        events.push((*end, -1));
    }
    events.sort_by(|left, right| {
        left.0
            .partial_cmp(&right.0)
            .expect("finite timestamps")
            .then(left.1.cmp(&right.1))
    });
    let mut current = 0i32;
    let mut peak = 0i32;
    for (_, delta) in events {
        current += delta;
        peak = peak.max(current);
    }
    usize::try_from(peak).expect("non-negative concurrency")
}

// ---------------------------------------------------------------------------
// 状态断言辅助
// ---------------------------------------------------------------------------

/// 断言实例处于给定终态（按 trace 最后一条实例快照）。
pub fn assert_instance_status(
    reader: &TraceReader,
    run_id: &str,
    node_id: &str,
    expected: TraceInstanceStatus,
) {
    let actual = instance_status(reader, run_id, node_id);
    assert_eq!(actual, expected, "node {node_id} status in trace");
}
