//! run 生命周期、实例状态机与事件循环。
//!
//! 语义基准：`doc/spec-v1.2/03-调度器与生命周期.md`（§3.1 所有权模型、§3.2 启动、
//! §3.3 主循环、§3.4 投递、§3.5 派发、§3.7 冷峻失败、§3.8 控制、§3.9 超时/看门狗、
//! §3.10 终止）与 `doc/spec-v1.2/01-领域模型与状态机.md`（§1.2.3 迁移矩阵、§1.4
//! 投递契约、§1.9 run 输出）。
//!
//! # 落地偏差（`doc/architecture-v2.md` §8）
//!
//! - **D1**：超时 = kill 执行任务；实例 `TimedOut` → run `Failed`；
//!   `NodeAbandoned` **不产生**、`abandoned_tasks` 恒 0，新增 `killed_workers`。
//! - **D6**：门控是 Rust 内置 op，在发布者执行任务内 off-loop 求值（纯函数，
//!   因此内置 `start/end` 的求值在事件循环内也安全）。
//! - **I7/I8**：gate 异常只拦截边，不使 run Failed；事件循环不执行用户 Python。
//!
//! # 关键不变式
//!
//! - 状态迁移只经 [`transition`]（唯一入口）：非法迁移 → `RUN_ILLEGAL_TRANSITION`
//!   → 冷峻 `fail_run`。
//! - `RunFinished` 恰一次（[`Emitter::finish`] 去重）；`RunStarted` 第一。
//! - 迟到执行结果（实例已终态）直接丢弃（03 §3.6）。
//! - `start`/`end`/`subgraph` 不交给执行器：`start` 输出 = 入口注入原样 object；
//!   `end` 输出 = inputs object 且不 publish；`subgraph` 在派发前被拒绝。
//!
//! # 线程/运行时
//!
//! [`start_run`] 会 `tokio::spawn` 事件循环任务，**必须在 Tokio 运行时上下文中调用**。

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::Value;
use tokio::sync::{mpsc, watch};
use tokio::task::JoinHandle;
use zgraph_dsl::{NodeType, Trigger};

use crate::context::{NodeContext, NodeContextInit};
use crate::executor::NodeExecutor;
use crate::gate::{evaluate_gate_for, GateEvaluation};
use crate::graph::{CompiledGraph, CompiledNode};
use crate::trace::{
    Emitter, GateResultRecord, InstanceSnapshot, RunFinishedInfo, RunStartedInfo, TraceSink,
    TransitionRecord,
};
use crate::types::{
    now_rfc3339, Attempt, EdgeIdx, InstanceId, InstanceStatus, NodeError, NodeId, PortName,
    RecordMode, RunError, RunId, RunMetrics, RunStartError, RunStatus, CODE_NODE_INTERNAL,
    CODE_NODE_TIMEOUT, CODE_NODE_TYPE_UNSUPPORTED, CODE_RUN_DEADLOCK,
    CODE_RUN_DELIVERY_TO_TERMINAL, CODE_RUN_ENTRY_INPUT_MISSING, CODE_RUN_ENTRY_INVALID,
    CODE_RUN_ILLEGAL_TRANSITION,
};

// ---------------------------------------------------------------------------
// 公开配置与句柄
// ---------------------------------------------------------------------------

/// 仅测试构建可用的故障注入入口（spec 03 §3.2；`feature = "test-hooks"`）。
#[cfg(feature = "test-hooks")]
#[derive(Debug, Clone, Default)]
pub struct TestHooks {
    /// 不结算入边（`settle_edge` 不记账）→ 制造"永不就绪"以触发死锁检测（AC-11）。
    pub disable_settle: bool,
    /// 覆盖所有节点的实例超时（毫秒），用于快速超时用例。
    pub force_node_timeout_ms: Option<u64>,
    /// 每次 watchdog tick 发送一个 `Instant`（AC-07 ticker 间隔断言）。
    pub tick_probe: Option<mpsc::UnboundedSender<Instant>>,
}

/// run 选项（03 §3.2）。
///
/// **构造建议**：用 `RunOptions::default()` 再逐字段赋值（或先构造再调整）；
/// 不要写"穷举 5 个字段"的 struct literal —— `feature = "test-hooks"` 构建下本结构
/// 多一个 [`TestHooks`] 字段（spec 03 §3.2），而 `cargo test --workspace` 会因
/// dev-dependency 的特性合并把该 feature 打开。
#[derive(Debug, Clone)]
pub struct RunOptions {
    /// 静默判定宽限（毫秒）：超过后仍有 Pending → `RUN_DEADLOCK`。
    pub deadlock_grace_ms: u64,
    /// watchdog tick 间隔（毫秒）。
    pub tick_interval_ms: u64,
    /// 是否调用 trace sink（false 时内核仍构造 outcome）。
    pub trace_enabled: bool,
    /// 随机种子（可复现 L2）。
    pub seed: u64,
    /// 记录模式。
    pub record_mode: RecordMode,
    /// 仅 `feature = "test-hooks"` 存在的测试钩子（spec 03 §3.2）。
    ///
    /// 通常用 [`RunOptions::with_test_hooks`] 设置；字段公开是为了允许
    /// 需要时直接按 spec 结构构造。
    #[cfg(feature = "test-hooks")]
    pub test_hooks: TestHooks,
}

impl Default for RunOptions {
    fn default() -> Self {
        Self {
            deadlock_grace_ms: 5_000,
            tick_interval_ms: 1_000,
            trace_enabled: true,
            seed: 0,
            record_mode: RecordMode::Hash,
            #[cfg(feature = "test-hooks")]
            test_hooks: TestHooks::default(),
        }
    }
}

#[cfg(feature = "test-hooks")]
impl RunOptions {
    /// 安装测试钩子（仅测试构建可用）。
    pub fn with_test_hooks(&mut self, hooks: TestHooks) {
        self.test_hooks = hooks;
    }
}

/// run 启动输入（03 §3.2）。
#[derive(Clone)]
pub struct RunSpec {
    /// 编译后的图（不可变共享）。
    pub graph: Arc<CompiledGraph>,
    /// 入口注入：`node_id -> {port: value}`；必须覆盖 entry 节点 required 端口。
    pub entry_inputs: HashMap<NodeId, HashMap<PortName, Value>>,
    /// 用户 ID（只读进 NodeContext）。
    pub user_id: String,
    /// run 级全局变量（只读快照）。
    pub global_vars: HashMap<String, Value>,
    /// run 选项。
    pub options: RunOptions,
}

/// 控制命令（03 §3.8；独立 unbounded 通道 + `select! biased` 优先）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunControl {
    /// 暂停派发（在飞继续，投递仍应用）。
    Pause,
    /// 恢复派发（重新扫描 Pending 就绪）。
    Resume,
    /// 取消 run（幂等，只结束一次）。
    Cancel,
}

/// run 结束结果（与 `RunFinished` 一致）。
#[derive(Debug, Clone, PartialEq)]
pub struct RunOutcome {
    /// run ID。
    pub run_id: RunId,
    /// 终态。
    pub status: RunStatus,
    /// end 聚合输出（失败/取消/无 end 为 None）。
    pub output: Option<Value>,
    /// 失败错误（`Failed` 时）。
    pub error: Option<RunError>,
    /// run 指标。
    pub metrics: RunMetrics,
    /// 启动时刻（UTC RFC3339 毫秒 Z）。
    pub started_at: String,
    /// 结束时刻（UTC RFC3339 毫秒 Z）。
    pub finished_at: String,
}

/// run 句柄：控制、状态订阅与等待结果。
pub struct RunHandle {
    run_id: RunId,
    status_rx: watch::Receiver<RunStatus>,
    ctrl_tx: mpsc::UnboundedSender<RunControl>,
    join: JoinHandle<RunOutcome>,
}

impl RunHandle {
    /// run ID。
    pub fn run_id(&self) -> &str {
        &self.run_id
    }

    /// 发送控制命令；run 事件循环已结束时返回 `false`。
    pub fn control(&self, command: RunControl) -> bool {
        self.ctrl_tx.send(command).is_ok()
    }

    /// 暂停（等价于 `control(Pause)`）。
    pub fn pause(&self) -> bool {
        self.control(RunControl::Pause)
    }

    /// 恢复（等价于 `control(Resume)`）。
    pub fn resume(&self) -> bool {
        self.control(RunControl::Resume)
    }

    /// 取消（等价于 `control(Cancel)`）。
    pub fn cancel(&self) -> bool {
        self.control(RunControl::Cancel)
    }

    /// 订阅 run 状态变化（当前状态可立即读取）。
    pub fn subscribe_status(&self) -> watch::Receiver<RunStatus> {
        self.status_rx.clone()
    }

    /// 当前 run 状态快照。
    pub fn status(&self) -> RunStatus {
        *self.status_rx.borrow()
    }

    /// 等待 run 结束并返回 outcome。
    pub async fn wait(self) -> RunOutcome {
        let RunHandle { run_id, join, .. } = self;
        match join.await {
            Ok(outcome) => outcome,
            Err(error) => RunOutcome {
                run_id,
                status: RunStatus::Failed,
                output: None,
                error: Some(RunError::new(
                    CODE_NODE_INTERNAL,
                    format!("run task terminated unexpectedly: {error}"),
                )),
                metrics: RunMetrics::default(),
                started_at: now_rfc3339(),
                finished_at: now_rfc3339(),
            },
        }
    }
}

// ---------------------------------------------------------------------------
// 启动入口
// ---------------------------------------------------------------------------

/// 启动 run（异步事件循环在后台任务中运行），返回控制句柄。
///
/// 启动校验（03 §3.2 步骤 1–2）在**写任何 trace 之前**完成；失败返回
/// [`RunStartError`]，不产生 run / 实例 / 任何 sink 事件。
///
/// **必须在 Tokio 运行时内调用**（内部 `tokio::spawn` 事件循环）。
pub fn start_run(
    run_id: Option<RunId>,
    spec: RunSpec,
    executor: Arc<dyn NodeExecutor>,
    sink: Arc<dyn TraceSink>,
) -> Result<RunHandle, RunStartError> {
    validate_start(&spec)?;
    let run_id = run_id.unwrap_or_else(|| uuid::Uuid::now_v7().to_string());
    let (ctrl_tx, ctrl_rx) = mpsc::unbounded_channel();
    let (exec_tx, exec_rx) = mpsc::unbounded_channel();
    let (status_tx, status_rx) = watch::channel(RunStatus::Running);
    let engine = RunLoop::new(run_id.clone(), spec, executor, sink, exec_tx, status_tx);
    let join = tokio::spawn(engine.drive(exec_rx, ctrl_rx));
    Ok(RunHandle {
        run_id,
        status_rx,
        ctrl_tx,
        join,
    })
}

/// 启动并等待 run 结束的便捷入口。
pub async fn run_to_completion(
    run_id: Option<RunId>,
    spec: RunSpec,
    executor: Arc<dyn NodeExecutor>,
    sink: Arc<dyn TraceSink>,
) -> Result<RunOutcome, RunStartError> {
    let handle = start_run(run_id, spec, executor, sink)?;
    Ok(handle.wait().await)
}

/// 启动校验：entry 集合、`entry_inputs` 键/端口合法性、required 覆盖。
fn validate_start(spec: &RunSpec) -> Result<(), RunStartError> {
    let graph = &spec.graph;
    if graph.entry_points.is_empty() {
        return Err(RunStartError::new(
            CODE_RUN_ENTRY_INVALID,
            "entry_points must not be empty",
        ));
    }
    for entry in &graph.entry_points {
        if !graph.nodes.contains_key(entry) {
            return Err(RunStartError::new(
                CODE_RUN_ENTRY_INVALID,
                format!("entry point '{entry}' is not a declared node"),
            ));
        }
    }
    let entry_set: HashSet<&str> = graph.entry_points.iter().map(String::as_str).collect();
    for (node_id, ports) in &spec.entry_inputs {
        let Some(node) = graph.nodes.get(node_id) else {
            return Err(RunStartError::new(
                CODE_RUN_ENTRY_INVALID,
                format!("entry_inputs contains unknown node '{node_id}'"),
            ));
        };
        if !entry_set.contains(node_id.as_str()) {
            return Err(RunStartError::new(
                CODE_RUN_ENTRY_INVALID,
                format!("entry_inputs contains non-entry node '{node_id}'"),
            ));
        }
        for port in ports.keys() {
            if !node.inputs.iter().any(|declared| &declared.name == port) {
                return Err(RunStartError::new(
                    CODE_RUN_ENTRY_INVALID,
                    format!("entry_inputs for '{node_id}' contains undeclared port '{port}'"),
                ));
            }
        }
    }
    for entry in &graph.entry_points {
        let Some(node) = graph.nodes.get(entry) else {
            continue;
        };
        let provided = spec.entry_inputs.get(entry);
        for port in node.inputs.iter().filter(|port| port.required) {
            if !provided.is_some_and(|map| map.contains_key(&port.name)) {
                return Err(RunStartError::new(
                    CODE_RUN_ENTRY_INPUT_MISSING,
                    format!(
                        "entry node '{entry}' required port '{}' is not provided",
                        port.name
                    ),
                ));
            }
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 事件循环内部结构
// ---------------------------------------------------------------------------

/// 实例记录（01 §1.2.1 + 调度器内部字段）。
struct InstanceRecord {
    instance_id: InstanceId,
    node_id: NodeId,
    attempt: Attempt,
    status: InstanceStatus,
    created_at: String,
    started_at: Option<String>,
    finished_at: Option<String>,
    input: Option<Value>,
    output: Option<Value>,
    error: Option<NodeError>,
    error_code: Option<String>,
    started_instant: Option<Instant>,
    deadline: Option<Instant>,
}

/// 实例输入缓存（01 §1.3）。
struct InstanceBuffer {
    values: HashMap<PortName, Value>,
    arrived: HashSet<PortName>,
    required: HashSet<PortName>,
}

/// 执行任务 → 事件循环的回传事件（01 §1.7）。
enum RuntimeEvent {
    /// 节点成功（含 off-loop 已求值的全部出边 gate 结果）。
    Completed {
        instance_id: InstanceId,
        output: Value,
        evaluations: Vec<GateEvaluation>,
    },
    /// 节点失败。
    Failed {
        instance_id: InstanceId,
        error: NodeError,
    },
}

/// 单 run 事件循环（独占全部可变状态；03 §3.1）。
struct RunLoop {
    run_id: RunId,
    graph: Arc<CompiledGraph>,
    entry_inputs: HashMap<NodeId, HashMap<PortName, Value>>,
    user_id: String,
    global_vars: Arc<HashMap<String, Value>>,
    status: RunStatus,
    instances: HashMap<InstanceId, InstanceRecord>,
    node_current: HashMap<NodeId, InstanceId>,
    buffers: HashMap<InstanceId, InstanceBuffer>,
    settled_by_node: HashMap<NodeId, HashSet<EdgeIdx>>,
    ready_queue: VecDeque<InstanceId>,
    tasks: HashMap<InstanceId, JoinHandle<()>>,
    last_event_at: Instant,
    cancel: Arc<AtomicBool>,
    emit_dropped: Arc<AtomicU64>,
    killed_workers: u64,
    run_output: Option<Value>,
    started_at: String,
    finished_at: Option<String>,
    emitter: Emitter,
    executor: Arc<dyn NodeExecutor>,
    exec_tx: mpsc::UnboundedSender<RuntimeEvent>,
    status_tx: watch::Sender<RunStatus>,
    options: RunOptions,
    disable_settle: bool,
    force_timeout_ms: Option<u64>,
    tick_probe: Option<mpsc::UnboundedSender<Instant>>,
    final_outcome: Option<RunOutcome>,
}

impl RunLoop {
    /// 由启动输入构造事件循环状态（不产生任何 sink 调用）。
    fn new(
        run_id: RunId,
        spec: RunSpec,
        executor: Arc<dyn NodeExecutor>,
        sink: Arc<dyn TraceSink>,
        exec_tx: mpsc::UnboundedSender<RuntimeEvent>,
        status_tx: watch::Sender<RunStatus>,
    ) -> Self {
        let RunSpec {
            graph,
            entry_inputs,
            user_id,
            global_vars,
            options,
        } = spec;
        let emitter = Emitter::new(sink, options.trace_enabled);
        let (disable_settle, force_timeout_ms, tick_probe) = hook_values(&options);
        Self {
            run_id,
            graph,
            entry_inputs,
            user_id,
            global_vars: Arc::new(global_vars),
            status: RunStatus::Running,
            instances: HashMap::new(),
            node_current: HashMap::new(),
            buffers: HashMap::new(),
            settled_by_node: HashMap::new(),
            ready_queue: VecDeque::new(),
            tasks: HashMap::new(),
            last_event_at: Instant::now(),
            cancel: Arc::new(AtomicBool::new(false)),
            emit_dropped: Arc::new(AtomicU64::new(0)),
            killed_workers: 0,
            run_output: None,
            started_at: now_rfc3339(),
            finished_at: None,
            emitter,
            executor,
            exec_tx,
            status_tx,
            options,
            disable_settle,
            force_timeout_ms,
            tick_probe,
            final_outcome: None,
        }
    }

    /// 主循环（03 §3.3）：控制优先 → 派发/终止判定 → 等待事件。
    async fn drive(
        mut self,
        mut exec_rx: mpsc::UnboundedReceiver<RuntimeEvent>,
        mut ctrl_rx: mpsc::UnboundedReceiver<RunControl>,
    ) -> RunOutcome {
        self.emit_run_started();
        if self.has_subgraph() {
            self.fail_run(RunError::new(
                CODE_NODE_TYPE_UNSUPPORTED,
                "subgraph execution is not supported in v1",
            ));
        } else {
            self.bootstrap_entries();
        }
        let tick_ms = self.options.tick_interval_ms.max(1);
        let mut ticker = tokio::time::interval(Duration::from_millis(tick_ms));
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            while let Ok(command) = ctrl_rx.try_recv() {
                self.handle_control(command);
            }
            if let Some(outcome) = self.final_outcome.take() {
                return outcome;
            }
            self.dispatch_ready();
            if let Some(outcome) = self.final_outcome.take() {
                return outcome;
            }
            if let Some(outcome) = self.check_termination() {
                return outcome;
            }
            tokio::select! {
                biased;
                Some(command) = ctrl_rx.recv() => self.handle_control(command),
                _ = ticker.tick() => {
                    self.watchdog();
                    if let Some(outcome) = self.final_outcome.take() {
                        return outcome;
                    }
                }
                Some(event) = exec_rx.recv() => self.handle_exec_event(event),
            }
        }
    }

    /// 发出 `RunStarted`（生命周期第一条 sink 事件）。
    fn emit_run_started(&self) {
        let entry_inputs = serde_json::to_value(&self.entry_inputs).unwrap_or(Value::Null);
        let info = RunStartedInfo {
            run_id: self.run_id.clone(),
            graph_hash: self.graph.graph_hash.clone(),
            graph_name: self.graph.metadata.name.clone(),
            dsl_json: self.graph.dsl_json.clone(),
            user_id: self.user_id.clone(),
            entry_inputs,
            seed: self.options.seed,
            record_mode: self.options.record_mode.as_str().to_string(),
            owner_pid: std::process::id(),
            owner_host: host_name(),
            started_at: self.started_at.clone(),
        };
        self.emitter.run_started(info);
    }

    /// 是否存在 `subgraph` 节点（v1 运行期拒绝，04 §4.4.4）。
    fn has_subgraph(&self) -> bool {
        self.graph
            .nodes
            .values()
            .any(|node| node.node_type == NodeType::Subgraph)
    }

    /// 创建入口实例并按 `node_order` 就绪入队（03 §3.2 步骤 4–5）。
    fn bootstrap_entries(&mut self) {
        let entry_set: HashSet<&str> = self.graph.entry_points.iter().map(String::as_str).collect();
        let entry_order: Vec<NodeId> = self
            .graph
            .node_order
            .iter()
            .filter(|node_id| entry_set.contains(node_id.as_str()))
            .cloned()
            .collect();
        for node_id in entry_order {
            let Some(instance_id) = self.create_instance(&node_id, 0, None) else {
                return;
            };
            if let Some(values) = self.entry_inputs.get(&node_id).cloned() {
                for (port, value) in values {
                    self.buffer_write(&instance_id, &port, value);
                }
            }
            let is_start = self
                .graph
                .nodes
                .get(&node_id)
                .is_some_and(|node| node.node_type == NodeType::Start);
            if is_start {
                self.mark_ready(&instance_id);
            } else {
                self.maybe_make_ready(&instance_id);
            }
        }
    }

    /// 处理控制命令（Pause/Resume/Cancel；终态后 no-op）。
    fn handle_control(&mut self, command: RunControl) {
        if self.status.is_terminal() {
            return;
        }
        match command {
            RunControl::Pause => {
                if self.status == RunStatus::Running {
                    self.set_status(RunStatus::Paused);
                    self.last_event_at = Instant::now();
                }
            }
            RunControl::Resume => {
                if self.status == RunStatus::Paused {
                    self.set_status(RunStatus::Running);
                    self.last_event_at = Instant::now();
                    self.resume_pending();
                }
            }
            RunControl::Cancel => self.cancel_run(),
        }
    }

    /// Resume：重新扫描 Pending 实例做就绪判定（03 §3.8）。
    fn resume_pending(&mut self) {
        let pending: Vec<InstanceId> = self
            .instances
            .values()
            .filter(|record| record.status == InstanceStatus::Pending)
            .map(|record| record.instance_id.clone())
            .collect();
        for instance_id in pending {
            self.maybe_make_ready(&instance_id);
        }
    }

    /// 设置 run 状态并通知订阅者。
    fn set_status(&mut self, status: RunStatus) {
        self.status = status;
        let _ = self.status_tx.send(status);
    }

    /// 终止判定（03 §3.10）：Running 且静默 → 聚合 end 输出并 `Completed`。
    fn check_termination(&mut self) -> Option<RunOutcome> {
        if self.status != RunStatus::Running {
            return None;
        }
        if !self.is_quiescent() {
            return None;
        }
        self.complete_run();
        self.final_outcome.take()
    }

    /// 静默判定：无 Pending/Ready/Running 实例且就绪队列为空。
    fn is_quiescent(&self) -> bool {
        if !self.ready_queue.is_empty() {
            return false;
        }
        !self.instances.values().any(|record| {
            matches!(
                record.status,
                InstanceStatus::Pending | InstanceStatus::Ready | InstanceStatus::Running
            )
        })
    }

    /// 派发就绪实例（03 §3.5；Paused 时冻结）。
    fn dispatch_ready(&mut self) {
        while self.status == RunStatus::Running {
            let Some(instance_id) = self.ready_queue.pop_front() else {
                return;
            };
            let Some(record) = self.instances.get(&instance_id) else {
                continue;
            };
            if record.status != InstanceStatus::Ready {
                continue;
            }
            let node_id = record.node_id.clone();
            let Some(node) = self.graph.nodes.get(&node_id).cloned() else {
                self.fail_run(RunError::new(
                    CODE_NODE_INTERNAL,
                    format!("dispatched node '{node_id}' is missing from compiled graph"),
                ));
                return;
            };
            let inputs = self.build_inputs(&instance_id, &node);
            self.start_dispatch(&instance_id, &node, &inputs);
        }
    }

    /// 从 Ready 迁到 Running 并按节点类型派发。
    fn start_dispatch(
        &mut self,
        instance_id: &str,
        node: &CompiledNode,
        inputs: &HashMap<PortName, Value>,
    ) {
        let timeout_ms = self.force_timeout_ms.unwrap_or(node.timeout_ms);
        let deadline = Instant::now() + Duration::from_millis(timeout_ms);
        let input_value = serde_json::to_value(inputs).unwrap_or(Value::Null);
        if let Some(record) = self.instances.get_mut(instance_id) {
            record.input = Some(input_value);
            record.deadline = Some(deadline);
        }
        if !self.transition_or_fail(instance_id, InstanceStatus::Running, "dispatch") {
            return;
        }
        match node.node_type {
            NodeType::Start => {
                // 入口注入原样 object（04 §4.4.2：start 是源，不补 null）。
                let output = serde_json::to_value(inputs).unwrap_or(Value::Null);
                self.finish_builtin(instance_id, &node.id, output, true);
            }
            NodeType::End => {
                let output = serde_json::to_value(inputs).unwrap_or(Value::Null);
                self.finish_builtin(instance_id, &node.id, output, false);
            }
            NodeType::Subgraph => self.fail_run(RunError::new(
                CODE_NODE_TYPE_UNSUPPORTED,
                "subgraph execution is not supported in v1",
            )),
            NodeType::PythonNode | NodeType::AgentCall => {
                self.spawn_execution(instance_id, node, inputs.clone(), deadline);
            }
        }
    }

    /// 构造派发 inputs：全部声明端口，未到为 `null`；start 直接用入口注入。
    fn build_inputs(&self, instance_id: &str, node: &CompiledNode) -> HashMap<PortName, Value> {
        if node.node_type == NodeType::Start {
            return self.entry_inputs.get(&node.id).cloned().unwrap_or_default();
        }
        let buffer = self.buffers.get(instance_id);
        node.inputs
            .iter()
            .map(|declared| {
                let value = buffer
                    .and_then(|buffer| buffer.values.get(&declared.name))
                    .cloned()
                    .unwrap_or(Value::Null);
                (declared.name.clone(), value)
            })
            .collect()
    }

    /// 内置节点完成：写 output → Completed；`publish=true` 时发布出边。
    fn finish_builtin(&mut self, instance_id: &str, node_id: &str, output: Value, publish: bool) {
        if let Some(record) = self.instances.get_mut(instance_id) {
            record.output = Some(output.clone());
        }
        if !self.transition_or_fail(instance_id, InstanceStatus::Completed, "execution ok") {
            return;
        }
        if !publish {
            return;
        }
        let graph = self.graph.clone();
        let evaluations = evaluate_outgoing(&graph, node_id, &output);
        self.apply_publish(instance_id, node_id, &output, &evaluations);
    }

    /// 派生执行任务（off-loop；gate 求值在任务内完成后随事件回传）。
    fn spawn_execution(
        &mut self,
        instance_id: &str,
        node: &CompiledNode,
        inputs: HashMap<PortName, Value>,
        deadline: Instant,
    ) {
        let ctx = NodeContext::new(NodeContextInit {
            run_id: self.run_id.clone(),
            instance_id: instance_id.to_string(),
            node_id: node.id.clone(),
            user_id: self.user_id.clone(),
            global_vars: self.global_vars.clone(),
            cancel: self.cancel.clone(),
            emit_dropped: self.emit_dropped.clone(),
            emitter: self.emitter.clone(),
            deadline,
        });
        let executor = self.executor.clone();
        let graph = self.graph.clone();
        let tx = self.exec_tx.clone();
        let node = node.clone();
        let node_id = node.id.clone();
        let instance_id_owned = instance_id.to_string();
        let task = tokio::spawn(async move {
            let execution = crate::executor::NodeExecution {
                node: &node,
                inputs: &inputs,
                ctx: &ctx,
            };
            match executor.execute(execution).await {
                Ok(output) => {
                    let evaluations = evaluate_outgoing(&graph, &node_id, &output);
                    let _ = tx.send(RuntimeEvent::Completed {
                        instance_id: instance_id_owned,
                        output,
                        evaluations,
                    });
                }
                Err(error) => {
                    let _ = tx.send(RuntimeEvent::Failed {
                        instance_id: instance_id_owned,
                        error,
                    });
                }
            }
        });
        self.tasks.insert(instance_id.to_string(), task);
    }

    // -- 执行回传处理 -------------------------------------------------------

    /// 处理执行任务回传（迟到结果丢弃；03 §3.6）。
    fn handle_exec_event(&mut self, event: RuntimeEvent) {
        if self.status.is_terminal() {
            return;
        }
        match event {
            RuntimeEvent::Completed {
                instance_id,
                output,
                evaluations,
            } => self.on_completed(instance_id, output, evaluations),
            RuntimeEvent::Failed { instance_id, error } => self.on_failed(instance_id, error),
        }
    }

    /// 节点成功：Running→Completed → publish（end 不 publish）。
    fn on_completed(
        &mut self,
        instance_id: InstanceId,
        output: Value,
        evaluations: Vec<GateEvaluation>,
    ) {
        let Some(record) = self.instances.get(&instance_id) else {
            return;
        };
        if record.status != InstanceStatus::Running {
            // 迟到结果：实例已终态（取消/超时）→ 丢弃（03 §3.6）。
            return;
        }
        let node_id = record.node_id.clone();
        self.tasks.remove(&instance_id);
        if let Some(record) = self.instances.get_mut(&instance_id) {
            record.output = Some(output.clone());
        }
        if !self.transition_or_fail(&instance_id, InstanceStatus::Completed, "execution ok") {
            return;
        }
        let is_end = self
            .graph
            .nodes
            .get(&node_id)
            .is_some_and(|node| node.node_type == NodeType::End);
        if !is_end {
            self.apply_publish(&instance_id, &node_id, &output, &evaluations);
        }
    }

    /// 节点失败：按 `NodeError` 归一化后冷峻失败（04 §4.1.4）。
    fn on_failed(&mut self, instance_id: InstanceId, error: NodeError) {
        let Some(record) = self.instances.get(&instance_id) else {
            return;
        };
        if record.status != InstanceStatus::Running {
            return;
        }
        let node_id = record.node_id.clone();
        self.tasks.remove(&instance_id);
        match error {
            NodeError::Timeout { elapsed_ms } => {
                self.fail_timeout(&instance_id, &node_id, elapsed_ms)
            }
            NodeError::Cancelled => self.on_cancelled_error(&instance_id, &node_id),
            other => self.fail_crashed(&instance_id, &node_id, other),
        }
    }

    /// 执行器自报超时（仅 AgentCallHandler 预检合法）→ TimedOut → run Failed。
    fn fail_timeout(&mut self, instance_id: &str, node_id: &str, elapsed_ms: u64) {
        let owner = instance_id.to_string();
        self.emitter
            .node_timed_out(&self.run_id, &owner, elapsed_ms, now_rfc3339());
        if !self.transition_or_fail(instance_id, InstanceStatus::TimedOut, "timeout") {
            return;
        }
        self.fail_run(RunError::new(
            CODE_NODE_TIMEOUT,
            format!("node '{node_id}' timed out after {elapsed_ms} ms"),
        ));
    }

    /// 执行器返回 `Cancelled`：有取消标志 → 收尾；否则归一化为 Internal。
    fn on_cancelled_error(&mut self, instance_id: &str, node_id: &str) {
        if self.cancel.load(Ordering::SeqCst) {
            let _ = self.transition_raw(instance_id, InstanceStatus::Cancelled, "run cancelled");
            self.cancel_run();
            return;
        }
        self.fail_crashed(
            instance_id,
            node_id,
            NodeError::Internal {
                message: "Cancelled without cancel flag".to_string(),
            },
        );
    }

    /// 普通执行失败：Running→Crashed → 冷峻 `fail_run`。
    fn fail_crashed(&mut self, instance_id: &str, node_id: &str, error: NodeError) {
        if let Some(record) = self.instances.get_mut(instance_id) {
            record.error_code = Some(error.code().to_string());
            record.error = Some(error.clone());
        }
        if !self.transition_or_fail(instance_id, InstanceStatus::Crashed, "execution failed") {
            return;
        }
        self.fail_run(RunError::new(
            error.code(),
            format!("node '{node_id}' failed: {error}"),
        ));
    }

    // -- 投递应用（两阶段的后半段；01 §1.4 / 03 §3.4.2）--------------------

    /// 应用已求值的出边 gate 结果：寻址 → 门控记录 → 写 buffer/结算 → 就绪或 Skipped。
    fn apply_publish(
        &mut self,
        from_instance: &str,
        from_node: &str,
        output: &Value,
        evaluations: &[GateEvaluation],
    ) {
        let mut ordered: Vec<&GateEvaluation> = evaluations.iter().collect();
        ordered.sort_by_key(|evaluation| evaluation.edge_index);
        for evaluation in ordered {
            if self.status.is_terminal() {
                return;
            }
            let Some(edge) = self.graph.edges.get(evaluation.edge_index).cloned() else {
                self.fail_run(RunError::new(
                    CODE_NODE_INTERNAL,
                    format!(
                        "gate evaluation references unknown edge {}",
                        evaluation.edge_index
                    ),
                ));
                return;
            };
            let target = match self.resolve_target(from_instance, &edge.to_node, edge.index) {
                Some(target) => target,
                None => return,
            };
            self.emitter.gate_result(GateResultRecord {
                run_id: self.run_id.clone(),
                instance_id: from_instance.to_string(),
                node_id: from_node.to_string(),
                edge_index: edge.index,
                to_node: edge.to_node.clone(),
                to_port: edge.to_port.clone(),
                gate_fn: edge.gate.as_ref().map(|gate| gate.function.clone()),
                passed: evaluation.passed,
                error: evaluation.error.clone(),
                ts: now_rfc3339(),
            });
            self.last_event_at = Instant::now();
            if evaluation.passed {
                self.buffer_write(&target, &edge.to_port, output.clone());
            }
            self.settle_edge(&edge.to_node, edge.index);
            if evaluation.passed {
                self.maybe_make_ready(&target);
            }
            // 结算后统一复核：即便本条边是"放行"边，目标节点也可能因其它入边被
            // 拦截而永远无法就绪；若此时全部入边已结算 → 必须转 Skipped（01 §1.2.3）。
            self.maybe_skip_after_settle(&edge.to_node);
        }
    }

    /// 解析投递目标实例（自环新建 attempt+1；其它惰性创建/校验终态）。
    fn resolve_target(
        &mut self,
        from_instance: &str,
        to_node: &str,
        edge_index: EdgeIdx,
    ) -> Option<InstanceId> {
        let from_node = self.instances.get(from_instance).map(|r| r.node_id.clone());
        if from_node.as_deref() == Some(to_node) {
            return self.resolve_self_loop(from_instance, to_node, edge_index);
        }
        match self.node_current.get(to_node).cloned() {
            Some(instance_id) => {
                let terminal = self
                    .instances
                    .get(&instance_id)
                    .is_some_and(|record| record.status.is_terminal());
                if terminal {
                    self.fail_run(RunError::new(
                        CODE_RUN_DELIVERY_TO_TERMINAL,
                        format!(
                            "delivery on edge {edge_index} targets terminal instance of node '{to_node}'"
                        ),
                    ));
                    return None;
                }
                Some(instance_id)
            }
            None => self.create_instance(to_node, 0, None),
        }
    }

    /// 自环：每次投递新建实例 `attempt+1`；超过 `max_attempts` → 新实例 Crashed。
    fn resolve_self_loop(
        &mut self,
        from_instance: &str,
        to_node: &str,
        edge_index: EdgeIdx,
    ) -> Option<InstanceId> {
        let from_attempt = self.instances.get(from_instance).map_or(0, |r| r.attempt);
        let attempt = from_attempt.saturating_add(1);
        let max_attempts = self.graph.nodes.get(to_node).map_or(1, |n| n.max_attempts);
        if attempt >= max_attempts {
            let error = NodeError::Internal {
                message: format!(
                    "self-loop of node '{to_node}' exceeded max_attempts ({max_attempts})"
                ),
            };
            let created = self.create_instance(to_node, attempt, Some(error));
            self.fail_run(RunError::new(
                CODE_NODE_INTERNAL,
                format!(
                    "self-loop of node '{to_node}' exceeded max_attempts ({max_attempts}) on edge {edge_index}"
                ),
            ));
            return created;
        }
        self.create_instance(to_node, attempt, None)
    }

    /// 创建实例（惰性/自环/入口共用）；`crash` 为 Some 时直接以 Crashed 创建。
    fn create_instance(
        &mut self,
        node_id: &str,
        attempt: Attempt,
        crash: Option<NodeError>,
    ) -> Option<InstanceId> {
        let Some(node) = self.graph.nodes.get(node_id) else {
            self.fail_run(RunError::new(
                CODE_NODE_INTERNAL,
                format!("cannot create instance for unknown node '{node_id}'"),
            ));
            return None;
        };
        let instance_id = uuid::Uuid::now_v7().to_string();
        let created_at = now_rfc3339();
        let crashed = crash.is_some();
        let record = InstanceRecord {
            instance_id: instance_id.clone(),
            node_id: node_id.to_string(),
            attempt,
            status: if crashed {
                InstanceStatus::Crashed
            } else {
                InstanceStatus::Pending
            },
            created_at: created_at.clone(),
            started_at: None,
            finished_at: if crashed { Some(created_at) } else { None },
            input: None,
            output: None,
            error_code: crash.as_ref().map(|error| error.code().to_string()),
            error: crash,
            started_instant: None,
            deadline: None,
        };
        let required = node
            .inputs
            .iter()
            .filter(|port| port.required)
            .map(|port| port.name.clone())
            .collect();
        self.buffers.insert(
            instance_id.clone(),
            InstanceBuffer {
                values: HashMap::new(),
                arrived: HashSet::new(),
                required,
            },
        );
        self.instances.insert(instance_id.clone(), record);
        self.node_current
            .insert(node_id.to_string(), instance_id.clone());
        if let Some(snapshot) = self.snapshot(&instance_id) {
            self.emitter.instance_snapshot(snapshot);
        }
        self.last_event_at = Instant::now();
        Some(instance_id)
    }

    /// 写 buffer（单数据源：后到覆盖先到）并标记端口到达。
    fn buffer_write(&mut self, instance_id: &str, port: &str, value: Value) {
        if let Some(buffer) = self.buffers.get_mut(instance_id) {
            buffer.values.insert(port.to_string(), value);
            buffer.arrived.insert(port.to_string());
        }
        self.last_event_at = Instant::now();
    }

    /// 结算一条入边（`test-hooks.disable_settle` 时不记账，用于制造死锁）。
    fn settle_edge(&mut self, node_id: &str, edge_index: EdgeIdx) {
        if self.disable_settle {
            return;
        }
        self.settled_by_node
            .entry(node_id.to_string())
            .or_default()
            .insert(edge_index);
    }

    /// `settle_edge` 之后的统一复核：目标节点全部入边已结算时，
    /// 未就绪的 Pending 实例必须转 `Skipped` 并迭代级联（01 §1.2.3 / 03 §3.4.2）。
    ///
    /// 为什么放行分支也必须调用：最后结算的入边可能是"被拦截的那条"，
    /// 也可能是"放行边"但节点 `trigger=all` 仍缺其它（被拦截的）端口；
    /// 若只从 blocked 分支触发级联，后一种情况会留下 Pending，
    /// 静默超过 `deadlock_grace_ms` 后被误判 `RUN_DEADLOCK`。
    fn maybe_skip_after_settle(&mut self, node_id: &str) {
        self.cascade_skip(node_id);
    }

    /// 就绪判定（01 §1.3）：`start` 恒就绪；All 需 required 全覆盖；Any 需至少一个。
    fn is_ready(&self, instance_id: &str) -> bool {
        let Some(record) = self.instances.get(instance_id) else {
            return false;
        };
        let Some(node) = self.graph.nodes.get(&record.node_id) else {
            return false;
        };
        if node.node_type == NodeType::Start {
            return true;
        }
        let Some(buffer) = self.buffers.get(instance_id) else {
            return false;
        };
        match node.trigger {
            Trigger::All => buffer
                .required
                .iter()
                .all(|port| buffer.arrived.contains(port)),
            Trigger::Any => buffer
                .required
                .iter()
                .any(|port| buffer.arrived.contains(port)),
        }
    }

    /// Pending → Ready（若就绪）并入队。
    fn maybe_make_ready(&mut self, instance_id: &str) {
        let Some(record) = self.instances.get(instance_id) else {
            return;
        };
        if record.status != InstanceStatus::Pending || !self.is_ready(instance_id) {
            return;
        }
        self.mark_ready(instance_id);
    }

    /// 无条件 Pending → Ready 并入队（start 入口/就绪判定通过后）。
    fn mark_ready(&mut self, instance_id: &str) {
        if !self.transition_or_fail(instance_id, InstanceStatus::Ready, "inputs ready") {
            return;
        }
        if let Some(record) = self.instances.get(instance_id) {
            if record.status == InstanceStatus::Ready {
                self.ready_queue.push_back(instance_id.to_string());
            }
        }
    }

    /// Skipped 级联（03 §3.4.2）：全部入边结算且不能就绪 → Skipped → 结算出边并迭代级联。
    ///
    /// 用显式栈迭代实现，避免门控环上深递归爆栈。
    fn cascade_skip(&mut self, start_node: &str) {
        let mut stack = vec![start_node.to_string()];
        while let Some(node_id) = stack.pop() {
            if self.status.is_terminal() {
                return;
            }
            let Some(instance_id) = self.ensure_instance(&node_id) else {
                return;
            };
            let Some(record) = self.instances.get(&instance_id) else {
                continue;
            };
            if record.status != InstanceStatus::Pending {
                continue;
            }
            if !self.all_incoming_settled(&node_id) {
                continue;
            }
            if self.is_ready(&instance_id) {
                self.maybe_make_ready(&instance_id);
                continue;
            }
            if !self.transition_or_fail(
                &instance_id,
                InstanceStatus::Skipped,
                "all_sources_blocked",
            ) {
                return;
            }
            let outgoing: Vec<EdgeIdx> = self.graph.edge_index().outgoing_edges(&node_id).to_vec();
            for edge_index in outgoing {
                let Some(edge) = self.graph.edges.get(edge_index) else {
                    continue;
                };
                let target = edge.to_node.clone();
                self.settle_edge(&target, edge_index);
                if target == node_id {
                    continue;
                }
                if self.ensure_instance(&target).is_none() {
                    return;
                }
                stack.push(target);
            }
        }
    }

    /// 确保节点有当前实例（惰性创建 Pending）。
    fn ensure_instance(&mut self, node_id: &str) -> Option<InstanceId> {
        if let Some(instance_id) = self.node_current.get(node_id).cloned() {
            return Some(instance_id);
        }
        self.create_instance(node_id, 0, None)
    }

    /// 节点的全部入边是否已结算。
    fn all_incoming_settled(&self, node_id: &str) -> bool {
        let incoming = self.graph.edge_index().incoming_edges(node_id);
        let settled = self.settled_by_node.get(node_id);
        incoming
            .iter()
            .all(|edge_index| settled.is_some_and(|set| set.contains(edge_index)))
    }

    // -- 看门狗：超时与死锁（03 §3.9）--------------------------------------

    /// watchdog tick：tick 探针 → 超时扫描 → 死锁判定。
    fn watchdog(&mut self) {
        if let Some(probe) = &self.tick_probe {
            let _ = probe.send(Instant::now());
        }
        if self.status.is_terminal() {
            return;
        }
        self.check_timeouts();
        if self.status == RunStatus::Running {
            self.check_deadlock();
        }
    }

    /// 扫描 Running 实例 deadline（到期 → NodeTimedOut → TimedOut → 冷峻失败）。
    fn check_timeouts(&mut self) {
        let now = Instant::now();
        let expired: Vec<InstanceId> = self
            .instances
            .values()
            .filter(|record| {
                record.status == InstanceStatus::Running
                    && record.deadline.is_some_and(|deadline| now >= deadline)
            })
            .map(|record| record.instance_id.clone())
            .collect();
        let mut first: Option<(InstanceId, u64)> = None;
        for instance_id in expired {
            if self.status.is_terminal() {
                return;
            }
            let elapsed_ms = self
                .instances
                .get(&instance_id)
                .and_then(|record| record.started_instant)
                .map(|started| {
                    u64::try_from(now.saturating_duration_since(started).as_millis())
                        .unwrap_or(u64::MAX)
                })
                .unwrap_or(0);
            self.killed_workers += 1;
            if let Some(task) = self.tasks.remove(&instance_id) {
                task.abort();
            }
            self.emitter
                .node_timed_out(&self.run_id, &instance_id, elapsed_ms, now_rfc3339());
            if !self.transition_or_fail(&instance_id, InstanceStatus::TimedOut, "timeout") {
                return;
            }
            if first.is_none() {
                first = Some((instance_id, elapsed_ms));
            }
        }
        if let Some((instance_id, elapsed_ms)) = first {
            let node_id = self
                .instances
                .get(&instance_id)
                .map(|record| record.node_id.clone())
                .unwrap_or_default();
            self.fail_run(RunError::new(
                CODE_NODE_TIMEOUT,
                format!("node '{node_id}' timed out after {elapsed_ms} ms"),
            ));
        }
    }

    /// 死锁判定（AC-11）：Running 且无 Running/Ready、队列空、仍有 Pending，
    /// 且静默超过 `deadlock_grace_ms` → `RUN_DEADLOCK`。
    fn check_deadlock(&mut self) {
        let has_running = self
            .instances
            .values()
            .any(|record| record.status == InstanceStatus::Running);
        let has_ready = !self.ready_queue.is_empty()
            || self
                .instances
                .values()
                .any(|record| record.status == InstanceStatus::Ready);
        let has_pending = self
            .instances
            .values()
            .any(|record| record.status == InstanceStatus::Pending);
        if has_running || has_ready || !has_pending {
            return;
        }
        let silent_ms = u64::try_from(self.last_event_at.elapsed().as_millis()).unwrap_or(u64::MAX);
        if silent_ms > self.options.deadlock_grace_ms {
            self.fail_run(RunError::new(
                CODE_RUN_DEADLOCK,
                format!("no runnable instance for {silent_ms} ms while Pending instances remain"),
            ));
        }
    }

    // -- 终态：完成 / 冷峻失败 / 取消（03 §3.7/§3.8/§3.10）------------------

    /// 静默完成：聚合 end 输出 → `RunFinished{Completed}`（恰一次）。
    fn complete_run(&mut self) {
        if self.status.is_terminal() {
            return;
        }
        self.abort_all_tasks();
        self.run_output = self.aggregate_output();
        self.set_status(RunStatus::Completed);
        let finished_at = now_rfc3339();
        self.finished_at = Some(finished_at.clone());
        let info = RunFinishedInfo {
            run_id: self.run_id.clone(),
            status: RunStatus::Completed,
            output_summary: self.run_output.clone(),
            error: None,
            metrics: self.metrics(),
            at: finished_at,
        };
        let _ = self.emitter.finish(info);
        self.final_outcome = Some(self.build_outcome(RunStatus::Completed, None));
    }

    /// 冷峻失败：cancel 标志 → 非终态实例 Cancelled → 清队列 → `RunFinished{Failed}`。
    fn fail_run(&mut self, error: RunError) {
        if self.status.is_terminal() {
            return;
        }
        self.cancel.store(true, Ordering::SeqCst);
        self.set_status(RunStatus::Failed);
        self.cancel_active_instances("run failed");
        self.ready_queue.clear();
        self.abort_all_tasks();
        self.run_output = None;
        let finished_at = now_rfc3339();
        self.finished_at = Some(finished_at.clone());
        let info = RunFinishedInfo {
            run_id: self.run_id.clone(),
            status: RunStatus::Failed,
            output_summary: None,
            error: Some(error.clone()),
            metrics: self.metrics(),
            at: finished_at,
        };
        let _ = self.emitter.finish(info);
        self.final_outcome = Some(self.build_outcome(RunStatus::Failed, Some(error)));
    }

    /// 用户取消：Pending/Ready/Running → Cancelled → `RunFinished{Cancelled}`（幂等）。
    fn cancel_run(&mut self) {
        if self.status.is_terminal() {
            return;
        }
        self.cancel.store(true, Ordering::SeqCst);
        self.set_status(RunStatus::Cancelled);
        self.cancel_active_instances("run cancelled");
        self.ready_queue.clear();
        self.abort_all_tasks();
        self.run_output = None;
        let finished_at = now_rfc3339();
        self.finished_at = Some(finished_at.clone());
        let info = RunFinishedInfo {
            run_id: self.run_id.clone(),
            status: RunStatus::Cancelled,
            output_summary: None,
            error: None,
            metrics: self.metrics(),
            at: finished_at,
        };
        let _ = self.emitter.finish(info);
        self.final_outcome = Some(self.build_outcome(RunStatus::Cancelled, None));
    }

    /// 把所有非终态实例迁到 Cancelled（Running 同时 abort 并计入 `killed_workers`）。
    fn cancel_active_instances(&mut self, reason: &str) {
        let active: Vec<InstanceId> = self
            .instances
            .values()
            .filter(|record| !record.status.is_terminal())
            .map(|record| record.instance_id.clone())
            .collect();
        for instance_id in active {
            let running = self
                .instances
                .get(&instance_id)
                .is_some_and(|record| record.status == InstanceStatus::Running);
            if running {
                self.killed_workers += 1;
                if let Some(task) = self.tasks.remove(&instance_id) {
                    task.abort();
                }
            }
            let _ = self.transition_raw(&instance_id, InstanceStatus::Cancelled, reason);
        }
    }

    /// abort 全部在飞执行任务（结果丢弃；D1 kill 语义的 in-process 等价物）。
    fn abort_all_tasks(&mut self) {
        for (_, task) in self.tasks.drain() {
            task.abort();
        }
    }

    /// end 聚合（01 §1.9）：无 end/全部 Skipped → None；多 end 按 node_order 浅合并。
    fn aggregate_output(&self) -> Option<Value> {
        let mut merged: Option<serde_json::Map<String, Value>> = None;
        let mut completed_end = false;
        for node_id in &self.graph.node_order {
            let Some(node) = self.graph.nodes.get(node_id) else {
                continue;
            };
            if node.node_type != NodeType::End {
                continue;
            }
            let Some(instance_id) = self.node_current.get(node_id) else {
                continue;
            };
            let Some(record) = self.instances.get(instance_id) else {
                continue;
            };
            if record.status != InstanceStatus::Completed {
                continue;
            }
            let Some(Value::Object(values)) = &record.output else {
                continue;
            };
            completed_end = true;
            let target = merged.get_or_insert_with(serde_json::Map::new);
            for (key, value) in values {
                target.insert(key.clone(), value.clone());
            }
        }
        if completed_end {
            merged.map(Value::Object)
        } else {
            None
        }
    }

    /// 当前指标（`abandoned_tasks` 恒 0，D1）。
    fn metrics(&self) -> RunMetrics {
        RunMetrics {
            streaming_dropped: self.emit_dropped.load(Ordering::Relaxed),
            abandoned_tasks: 0,
            trace_backpressure: 0,
            killed_workers: self.killed_workers,
        }
    }

    /// 构造与 `RunFinished` 一致的 outcome。
    fn build_outcome(&self, status: RunStatus, error: Option<RunError>) -> RunOutcome {
        RunOutcome {
            run_id: self.run_id.clone(),
            status,
            output: if status == RunStatus::Completed {
                self.run_output.clone()
            } else {
                None
            },
            error,
            metrics: self.metrics(),
            started_at: self.started_at.clone(),
            finished_at: self.finished_at.clone().unwrap_or_else(now_rfc3339),
        }
    }

    // -- 状态迁移（唯一入口；01 §1.2.3 / I2）--------------------------------

    /// 唯一迁移入口：非法迁移返回 `Err`，由调用方转 `RUN_ILLEGAL_TRANSITION`。
    fn transition_raw(
        &mut self,
        instance_id: &str,
        to: InstanceStatus,
        reason: &str,
    ) -> Result<(), String> {
        let Some(record) = self.instances.get(instance_id) else {
            return Err(format!("instance '{instance_id}' not found"));
        };
        let from = record.status;
        if !from.can_transition_to(to) {
            return Err(format!(
                "illegal transition {} -> {} for instance '{instance_id}'",
                from.as_str(),
                to.as_str()
            ));
        }
        let ts = now_rfc3339();
        if let Some(record) = self.instances.get_mut(instance_id) {
            record.status = to;
            if to == InstanceStatus::Running {
                record.started_at = Some(ts.clone());
                record.started_instant = Some(Instant::now());
            }
            if to.is_terminal() {
                record.finished_at = Some(ts.clone());
            }
        }
        if let Some(snapshot) = self.snapshot(instance_id) {
            let record = TransitionRecord {
                run_id: self.run_id.clone(),
                instance_id: instance_id.to_string(),
                node_id: snapshot.node_id.clone(),
                attempt: snapshot.attempt,
                from,
                to,
                reason: Some(reason.to_string()),
                ts,
            };
            self.emitter.transition(record, snapshot);
        }
        self.last_event_at = Instant::now();
        Ok(())
    }

    /// 迁移失败 → 冷峻 `RUN_ILLEGAL_TRANSITION`。
    fn transition_or_fail(&mut self, instance_id: &str, to: InstanceStatus, reason: &str) -> bool {
        match self.transition_raw(instance_id, to, reason) {
            Ok(()) => true,
            Err(message) => {
                self.fail_run(RunError::new(CODE_RUN_ILLEGAL_TRANSITION, message));
                false
            }
        }
    }

    /// 由当前记录构造实例快照。
    fn snapshot(&self, instance_id: &str) -> Option<InstanceSnapshot> {
        let record = self.instances.get(instance_id)?;
        let node = self.graph.nodes.get(&record.node_id)?;
        Some(InstanceSnapshot {
            run_id: self.run_id.clone(),
            instance_id: record.instance_id.clone(),
            node_id: record.node_id.clone(),
            node_type: node.node_type.as_str().to_string(),
            attempt: record.attempt,
            status: record.status,
            input: record.input.clone(),
            output: record.output.clone(),
            error: record.error.clone(),
            error_code: record.error_code.clone(),
            created_at: record.created_at.clone(),
            started_at: record.started_at.clone(),
            finished_at: record.finished_at.clone(),
        })
    }
}

// ---------------------------------------------------------------------------
// 辅助函数
// ---------------------------------------------------------------------------

/// 计算节点的全部出边 gate 求值（执行任务 off-loop / 内置节点 in-loop 共用）。
fn evaluate_outgoing(graph: &CompiledGraph, node_id: &str, output: &Value) -> Vec<GateEvaluation> {
    graph
        .edge_index()
        .outgoing_edges(node_id)
        .iter()
        .filter_map(|edge_index| {
            let edge = graph.edges.get(*edge_index)?;
            let evaluation = match &edge.gate {
                None => GateEvaluation {
                    edge_index: *edge_index,
                    passed: true,
                    error: None,
                },
                Some(gate) => evaluate_gate_for(*edge_index, gate, output),
            };
            Some(evaluation)
        })
        .collect()
}

/// 取值测试钩子（非 `test-hooks` 构建恒为默认值）。
#[cfg(feature = "test-hooks")]
fn hook_values(
    options: &RunOptions,
) -> (bool, Option<u64>, Option<mpsc::UnboundedSender<Instant>>) {
    (
        options.test_hooks.disable_settle,
        options.test_hooks.force_node_timeout_ms,
        options.test_hooks.tick_probe.clone(),
    )
}

/// 取值测试钩子（非 `test-hooks` 构建恒为默认值）。
#[cfg(not(feature = "test-hooks"))]
fn hook_values(
    _options: &RunOptions,
) -> (bool, Option<u64>, Option<mpsc::UnboundedSender<Instant>>) {
    (false, None, None)
}

/// 宿主主机名（trace 的 `owner_host`；取不到时 `unknown`）。
fn host_name() -> String {
    std::env::var("HOSTNAME")
        .or_else(|_| std::env::var("COMPUTERNAME"))
        .unwrap_or_else(|_| "unknown".to_string())
}
