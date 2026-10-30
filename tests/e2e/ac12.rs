//! AC-12（M5/M6 全栈）：10 个 run 并发隔离 + 全局/每 run worker 上限。
//!
//! 语义基准：`doc/spec-v1.2/07-验收用例.md` AC-12（并发 10 run、同图不同 run_id、
//! 独立 db 或同 db 不同 run_id；instances/gate_results/events 不串；global_vars/
//! user_id 不串；全局 `MAX_WORKERS` 与每 run `MAX_WORKERS_PER_RUN` 生效；总耗时/
//! 单 run 比记录基线）与 `doc/architecture-v2.md` §8 D4/D8（每节点一进程；全局
//! max_workers + 每 run max_workers_per_run，取消 CPU=1/IO=4 信号量）。
//!
//! # 两个用例
//!
//! - [`ac12_ten_concurrent_runs_share_one_db_and_are_isolated`]（M5 8+2 组合）：覆盖面
//!   更广（纯 python run + 含 `agent_call` 的完整栈 run 混合并发）。
//! - [`ac12_ten_same_graph_runs_are_isolated_and_bounded`]（M6，ISSUE-089 / REV-032
//!   加强）：spec 07 AC-12 原文形态——**同一张图** 10 个并发 run（同库不同 run_id），
//!   补齐 M5 未字面满足的「10×同图」口径，另断言 10 个 run 的 `graph_hash` 完全一致。
//!
//! # 数据通路（禁止 mock kernel/worker/trace）
//!
//! ```text
//! 内联 DSL 图 → CompiledGraph::compile → kernel start_run（真调度器）
//!   → zgraph_cli::bridge::TraceBridge → zgraph-trace SQLite（**10 run 同库**）
//!   → WorkerExecutor（真 python3 -m zgraph_worker.runtime 子进程）
//!   → 含 agent_call 的 2 个 run 走 RoutingExecutor: HarnessExecutor（fake LLM）
//!     + WorkerToolRunner（工具 = 真 Python worker 子进程；其并发走 harness 独立的
//!       进程级 tool semaphore，不受下面 `with_limits` 约束——ISSUE-079，M6 统一）
//!   → TraceReader 逐 run 查询
//! ```
//!
//! # 组合与理由（任务允许的组合）
//!
//! - 8 个纯 python run：同一条三并发分支图（`start → a/b/c → done`），真 worker +
//!   真 SQLite trace；用于 user_id/global_vars 隔离与全局/每 run 峰值度量。
//! - 2 个含 `agent_call` 的 run：`start → echo(python) → answer_writer(agent) → end`，
//!   fake LLM（AC-08 fixture 脚本）+ 工具 `add` 走真 worker 子进程；覆盖「完整栈」
//!   并发（kernel + Harness + worker + trace 同时多 run）。
//! - **DB 形态**：10 个 run 全部写同一个 SQLite 文件（同 db 不同 run_id，AC 允许的
//!   更强形态）；全部写端关闭后打开**最终共享 reader**，逐 run 查询验证 run 级隔离
//!   （`stats.db_runs == 10` + 逐 run 行/实例/事件归属）。
//! - **同图用例**：把上述 10 个 run 全部替换为同一条三并发分支图（`start → a/b/c →
//!   done`，10× python），其余启动/收集/隔离/上限断言复用同一套 helper。
//! - 全局/每 run 上限：`WorkerExecutor::with_limits(4, 2)`；峰值用**真实进程区间**
//!   度量（fixture 把 start/end 墙钟追加到共享日志，按 run_tag 分组求峰值）。
//!   **口径边界（ISSUE-079）**：本测试的峰值 = `WorkerExecutor` 的 python 节点子进程
//!   （8×a/b/c 分支 + 2×echo）；agent 的工具 `add` 由 Harness 的 `WorkerToolRunner`
//!   执行，使用独立进程级信号量，**不计入** `with_limits` 全局/每 run 统计（M6 统一）。
//!   因此实测全局峰值是「节点进程并发」的下界，不代表全栈所有 worker 子进程。
//!
//! # 实测基线（本机 macOS / Rust 1.99.1 / Python 3.13）
//!
//! - 8+2 组合（2026-10-30 连跑 3 次）：区间总数 26（8×3 分支 + 2×1 echo）；全局峰值
//!   **4**（= with_limits 全局上限）；每 run 峰值：8 个 python run 均 **2**（= 每 run
//!   上限），2 个 agent run 为 1；单 run 基线 **0.79–0.85s**、10 run 并发总耗时
//!   **2.46–2.54s**、比值 **2.9–3.1**（完全串行 ≈ 10x）。2026-10-31 私有 target
//!   复跑 2 次：0.80–0.86s / 2.50–2.51s / 2.90–3.14，量级一致。
//! - 同图用例（2026-10-31 连跑 2 次，私有 target 干净构建）：区间总数 30（10×3 分支）；
//!   全局峰值 **4**；10 个 run 的每 run 峰值均 **2**；单 run 基线 **0.79–0.81s**、
//!   10 run 总耗时 **3.29s**、比值 **4.04–4.15**。
//! - 断言只保留宽松上界 10x，具体数值随机器波动只记录；两个用例各自独立 tempdir/DB。
//! - **同库 10 并发写未触发 `SQLITE_BUSY`/写线程错误**（WAL + 5s `busy_timeout` +
//!   写线程 busy 重试）：10 run 全部 Completed，逐 run reader 查询均成功。若未来在
//!   更慢磁盘/更大流量下出现写线程错误，按 AC 措辞退化为「独立 db」组合并登记台账。
//!
//! 稳定性约定（AGENTS §8）：时序断言只用宽松上限（[`common::RUN_TIMEOUT`]），
//! 不用 sleep 猜时序；独立 tempdir；不修改 `crates/**`。

#![cfg(unix)]

#[path = "common/mod.rs"]
mod common;

use std::collections::hash_map::Entry;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use zgraph_cli::bridge::{start_traced_run, BridgeOutcome, TracedRun};
use zgraph_harness::{
    FakeLlmProvider, HarnessExecutor, HarnessSettings, LlmMessage, LlmProvider, WorkerToolRunner,
};
use zgraph_kernel::{
    CompiledGraph, CompiledHandler, CompiledNode, NodeError, NodeExecution, NodeExecutor,
    RunOptions, RunSpec, RunStatus,
};
use zgraph_trace::{InstanceStatus as TraceInstanceStatus, TraceReader};
use zgraph_worker::WorkerExecutor;

/// 并发 run 总数（AC-12：10）。
const RUN_COUNT: usize = 10;
/// 纯 python 图 run 数（8+2 组合的前 8 个；同图用例为全部 10 个）。
const PYTHON_RUN_COUNT: usize = 8;
/// 纯 python 图的实例节点集合。
const PYTHON_NODES: [&str; 5] = ["start", "a", "b", "c", "done"];
/// agent 图的实例节点集合。
const AGENT_NODES: [&str; 4] = ["start", "echo", "answer_writer", "end"];
/// 每个纯 python run 的进程区间数（3 个分支）。
const PYTHON_INTERVALS_PER_RUN: usize = 3;
/// 每个 agent run 的进程区间数（1 个 echo 节点）。
const AGENT_INTERVALS_PER_RUN: usize = 1;
/// 每个纯 python run 的 gate_results 行数（6 条边投递）。
const PYTHON_GATE_RESULTS_PER_RUN: usize = 6;
/// 每个 agent run 的 gate_results 行数（3 条边投递）。
const AGENT_GATE_RESULTS_PER_RUN: usize = 3;
/// 全局 worker 并发上限（with_limits 第 2 参）。
const GLOBAL_MAX_WORKERS: usize = 4;
/// 每 run worker 并发上限（with_limits 第 3 参）。
const MAX_WORKERS_PER_RUN: usize = 2;
/// 分支执行时长（秒；让 3 路并行窗口足够宽，与 concurrency.rs 同思路）。
const BRANCH_SLEEP_SECONDS: f64 = 0.35;
/// agent 图入口 prompt（fake LLM 脚本按 05 §5.3.4 固定走 add(2,3)）。
const AGENT_PROMPT: &str = "请计算 2 + 3 的结果，并用 JSON {\"answer\":\"<结果>\"} 回答。";
/// 总耗时/单 run 耗时的宽松上界（只证伪「完全串行 ≈ 10x」；不脆断言绝对值）。
const ELAPSED_RATIO_LIMIT: f64 = 10.0;

/// 一个 run 的验收参数（tag/user_id/tenant 三者在 10 个 run 间互不相同）。
#[derive(Clone, Debug)]
struct RunPlan {
    tag: String,
    user_id: String,
    tenant: String,
    kind: RunKind,
}

/// run 使用的图/执行器类别。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum RunKind {
    /// 8 个纯 python 三并发分支 run。
    Python,
    /// 2 个含 agent_call 的 run。
    Agent,
}

impl RunPlan {
    /// 按序号生成 run（前 8 个 python，后 2 个 agent）。
    fn new(index: usize) -> Self {
        let kind = if index < PYTHON_RUN_COUNT {
            RunKind::Python
        } else {
            RunKind::Agent
        };
        Self {
            tag: format!("ac12-{index:02}"),
            user_id: format!("u-ac12-{index:02}"),
            tenant: format!("tenant-{index:02}"),
            kind,
        }
    }

    /// 同图用例的第 `index` 个 run：10 个 run 全部使用同一条 python 分支图。
    fn same_graph(index: usize) -> Self {
        Self {
            tag: format!("ac12-same-{index:02}"),
            user_id: format!("u-ac12-same-{index:02}"),
            tenant: format!("tenant-same-{index:02}"),
            kind: RunKind::Python,
        }
    }

    /// 单 run 耗时基线用的独立 plan（不计入 10 run 集合）。
    fn baseline() -> Self {
        Self {
            tag: "ac12-baseline".to_string(),
            user_id: "u-ac12-baseline".to_string(),
            tenant: "tenant-baseline".to_string(),
            kind: RunKind::Python,
        }
    }
}

/// 一条已启动但未结束的并发 run。
struct LaunchedRun {
    plan: RunPlan,
    run_id: String,
    traced: TracedRun,
    fake: Option<Arc<FakeLlmProvider>>,
}

/// 一条已结束的并发 run（内核 outcome + 已关闭写端的只读 trace）。
struct FinishedRun {
    plan: RunPlan,
    run_id: String,
    fake: Option<Arc<FakeLlmProvider>>,
    outcome: BridgeOutcome,
}

/// 一条 run 的 trace 形状摘要（跨 run 一致性比较用）。
struct TraceShape {
    instance_ids: HashSet<String>,
    event_kinds: BTreeMap<String, usize>,
    gate_count: usize,
}

// ---------------------------------------------------------------------------
// 图与 spec 构造
// ---------------------------------------------------------------------------

/// `start(x) -> a/b/c(x) -> done(ra/rb/rc, trigger=all)`：8 个纯 python run 共用。
fn python_graph() -> Arc<CompiledGraph> {
    let mut nodes = vec![
        json!({"id": "start", "type": "start",
               "inputs": [{"name": "x", "type": "any", "required": true}],
               "config": {}}),
        json!({"id": "done", "type": "end",
               "inputs": [
                   {"name": "ra", "type": "any", "required": true},
                   {"name": "rb", "type": "any", "required": true},
                   {"name": "rc", "type": "any", "required": true}
               ],
               "config": {}}),
    ];
    let mut edges = Vec::new();
    for (branch, port) in [("a", "ra"), ("b", "rb"), ("c", "rc")] {
        nodes.push(json!({
            "id": branch, "type": "python_node",
            "inputs": [{"name": "x", "type": "any", "required": true}],
            "config": {"module": "e2e_fixtures.ac12", "function": branch}
        }));
        edges.push(json!({"from": "start", "to_node": branch, "to_input_port": "x"}));
        edges.push(json!({"from": branch, "to_node": "done", "to_input_port": port}));
    }
    let text = json!({
        "version": "1.0",
        "metadata": {"name": "e2e_ac12_python"},
        "entry_points": ["start"],
        "nodes": nodes,
        "edges": edges
    })
    .to_string();
    common::compile_text(&text)
}

/// `start(message) -> echo(message) -> answer_writer(agent) -> end(result)`：2 个 agent run 共用。
fn agent_graph() -> Arc<CompiledGraph> {
    let text = json!({
        "version": "1.0",
        "metadata": {"name": "e2e_ac12_agent"},
        "entry_points": ["start"],
        "nodes": [
            {"id": "start", "type": "start",
             "inputs": [{"name": "message", "type": "string", "required": true}],
             "config": {}},
            {"id": "echo", "type": "python_node",
             "inputs": [{"name": "message", "type": "any", "required": true}],
             "config": {"module": "e2e_fixtures.ac12", "function": "echo"}},
            {"id": "answer_writer", "type": "agent_call",
             "inputs": [{"name": "message", "type": "any", "required": true}],
             "trigger": "all",
             "config": {"agent_id": "answer_writer", "timeout_ms": 60000}},
            {"id": "end", "type": "end",
             "inputs": [{"name": "result", "type": "any", "required": true}],
             "config": {}}
        ],
        "edges": [
            {"from": "start", "to_node": "echo", "to_input_port": "message"},
            {"from": "echo", "to_node": "answer_writer", "to_input_port": "message"},
            {"from": "answer_writer", "to_node": "end", "to_input_port": "result"}
        ]
    })
    .to_string();
    common::compile_text(&text)
}

/// 由 plan 构造 `RunSpec`：不同 user_id + 不同 global_vars + 不同 run_tag 输入。
fn spec_for(plan: &RunPlan, log_file: &Path) -> RunSpec {
    let log = log_file.display().to_string();
    let (graph, inputs) = match plan.kind {
        RunKind::Python => (
            python_graph(),
            common::entry_inputs(&[(
                "start",
                "x",
                json!({"log_file": log, "sleep": BRANCH_SLEEP_SECONDS, "run_tag": plan.tag}),
            )]),
        ),
        RunKind::Agent => (
            agent_graph(),
            common::entry_inputs(&[(
                "start",
                "message",
                json!({"log_file": log, "run_tag": plan.tag, "text": AGENT_PROMPT}),
            )]),
        ),
    };
    RunSpec {
        graph,
        entry_inputs: inputs,
        user_id: plan.user_id.clone(),
        global_vars: HashMap::from([
            ("tenant".to_string(), json!(plan.tenant)),
            ("run_tag".to_string(), json!(plan.tag)),
        ]),
        options: RunOptions::default(),
    }
}

// ---------------------------------------------------------------------------
// 执行器构造（真 worker；agent 走 Harness + 真 worker 工具）
// ---------------------------------------------------------------------------

/// 组合执行器：`CompiledHandler::Agent` → [`HarnessExecutor`]，其余 → 共享 `WorkerExecutor`。
///
/// 与 `harness_ac08.rs` 的 RoutingExecutor 同构（该类型定义在测试二进制内，不能跨
/// 文件复用；本文件只做路由，不改变任何执行语义）。
struct RoutingExecutor {
    harness: Arc<HarnessExecutor>,
    worker: Arc<WorkerExecutor>,
}

impl NodeExecutor for RoutingExecutor {
    fn execute<'a>(
        &'a self,
        execution: NodeExecution<'a>,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Value, NodeError>> + Send + 'a>>
    {
        if matches!(&execution.node.handler, CompiledHandler::Agent { .. }) {
            self.harness.execute(execution)
        } else {
            self.worker.execute(execution)
        }
    }

    /// ISSUE-076：转发内层 `timeout_hint`，避免包装层默认 `None` 截断 harness 的
    /// `AgentConfig.node_timeout_ms` 回退（本文件 agent 节点在 DSL 声明了 timeout_ms，
    /// 转发仍是包装层的正确语义；端到端回退用例见 `harness_ac08.rs`）。
    fn timeout_hint(&self, node: &CompiledNode) -> Option<u64> {
        if matches!(&node.handler, CompiledHandler::Agent { .. }) {
            self.harness.timeout_hint(node)
        } else {
            self.worker.timeout_hint(node)
        }
    }
}

/// AC-08 fixture 根目录（agent 资产 + tools + fake LLM 脚本）。
fn harness_fixture_dir() -> std::path::PathBuf {
    common::graphs_root().join("harness_ac08")
}

/// 从 AC-08 fixture 脚本构造独立的 fake provider（每个 agent run 一个，请求日志不混）。
fn fake_provider() -> Arc<FakeLlmProvider> {
    let path = harness_fixture_dir().join("fake_llm.json");
    Arc::new(
        FakeLlmProvider::from_path(&path)
            .unwrap_or_else(|error| panic!("读取 fake-llm 脚本 {} 失败: {error}", path.display())),
    )
}

/// 构造「agent 走 Harness（fake LLM）+ 其余走共享真 worker」的执行器。
fn routing_executor(
    worker: Arc<WorkerExecutor>,
    fake: Arc<FakeLlmProvider>,
) -> Arc<dyn NodeExecutor> {
    let fixture = harness_fixture_dir();
    let assets_dir = fixture.clone();
    let settings = HarnessSettings {
        assets_dir,
        tools_dir: fixture.join("tools"),
        provider: fake as Arc<dyn LlmProvider>,
        fake_mode: true,
        // 工具每次调用 = 真 Python worker 子进程（D3）。
        tool_runner: Arc::new(WorkerToolRunner::new(common::worker_config(
            common::RUN_TIMEOUT,
        ))),
    };
    Arc::new(RoutingExecutor {
        harness: Arc::new(HarnessExecutor::new(settings)),
        worker,
    })
}

// ---------------------------------------------------------------------------
// 启动 / 收集
// ---------------------------------------------------------------------------

/// 依次启动 10 个 run（同库不同 run_id）；`start_traced_run` 立即返回，run 由内核任务驱动。
///
/// `plan_for` 决定第 `index` 个 run 的图/user_id/global_vars：8+2 组合用
/// [`RunPlan::new`]，同图用例用 [`RunPlan::same_graph`]。
fn launch_runs<F>(
    db: &Path,
    log_file: &Path,
    worker: &Arc<WorkerExecutor>,
    plan_for: F,
) -> Vec<LaunchedRun>
where
    F: Fn(usize) -> RunPlan,
{
    (0..RUN_COUNT)
        .map(|index| {
            let plan = plan_for(index);
            let (executor, fake): (Arc<dyn NodeExecutor>, Option<Arc<FakeLlmProvider>>) =
                match plan.kind {
                    RunKind::Python => (worker.clone(), None),
                    RunKind::Agent => {
                        let fake = fake_provider();
                        let executor = routing_executor(worker.clone(), fake.clone());
                        (executor, Some(fake))
                    }
                };
            let traced = start_traced_run(spec_for(&plan, log_file), executor, db)
                .unwrap_or_else(|error| panic!("start run {} failed: {error}", plan.tag));
            let run_id = traced.run_id().to_string();
            LaunchedRun {
                plan,
                run_id,
                traced,
                fake,
            }
        })
        .collect()
}

/// 等待全部 run 结束（run 已并发启动，这里顺序 await 每个句柄；内核任务独立推进）。
async fn finish_runs(launched: Vec<LaunchedRun>) -> Vec<FinishedRun> {
    let mut finished = Vec::with_capacity(launched.len());
    for run in launched {
        let outcome = run
            .traced
            .wait()
            .await
            .unwrap_or_else(|error| panic!("run {} wait failed: {error}", run.plan.tag));
        finished.push(FinishedRun {
            plan: run.plan,
            run_id: run.run_id,
            fake: run.fake,
            outcome,
        });
    }
    finished
}

/// 单 run 耗时基线：同样使用 with_limits(4,2) 的执行器与三并发分支图。
async fn run_single_baseline(dir: &Path, worker: Arc<WorkerExecutor>, log_file: &Path) -> Duration {
    let db = dir.join("ac12_baseline.db");
    let started = Instant::now();
    let bridge = tokio::time::timeout(
        common::RUN_TIMEOUT,
        common::run_to_trace(spec_for(&RunPlan::baseline(), log_file), worker, &db),
    )
    .await
    .expect("baseline run must finish within RUN_TIMEOUT");
    assert_eq!(bridge.outcome.status, RunStatus::Completed);
    started.elapsed()
}

// ---------------------------------------------------------------------------
// AC-12 断言
// ---------------------------------------------------------------------------

/// 全部 run Completed、无错误、run_id 互不相同。
fn assert_all_completed(runs: &[FinishedRun]) {
    assert_eq!(runs.len(), RUN_COUNT, "并发 run 数");
    let mut ids = HashSet::new();
    for run in runs {
        assert_eq!(
            run.outcome.outcome.status,
            RunStatus::Completed,
            "{} 应 Completed：{:?}",
            run.plan.tag,
            run.outcome.outcome.error
        );
        assert!(
            run.outcome.outcome.error.is_none(),
            "{} Completed 不应带 error",
            run.plan.tag
        );
        assert_eq!(run.outcome.outcome.run_id, run.run_id);
        assert!(
            ids.insert(run.run_id.clone()),
            "run_id 必须唯一：{}",
            run.run_id
        );
    }
}

/// trace 级隔离：同库共享 reader 上逐 run 检查归属，并跨 run 比较形状/实例 id。
fn assert_runs_isolated(runs: &[FinishedRun], shared_db: &Path) {
    let shared = TraceReader::open_readonly(shared_db).expect("open shared trace db");
    let stats = shared.stats(&runs[0].run_id).expect("shared stats");
    assert_eq!(
        stats.db_runs, RUN_COUNT as u64,
        "同库应恰有 {RUN_COUNT} 个 run（同 db 不同 run_id）"
    );
    let shapes: Vec<TraceShape> = runs
        .iter()
        .map(|run| assert_run_trace(&shared, run))
        .collect();
    assert_shapes_consistent(runs, &shapes);
    let mut seen: HashSet<String> = HashSet::new();
    for shape in &shapes {
        for instance_id in &shape.instance_ids {
            assert!(
                seen.insert(instance_id.clone()),
                "instance_id 跨 run 相交：{instance_id}"
            );
        }
    }
}

/// 同图证据：全部 run 的 `runs.graph_hash` 一致（同一张图定义，spec 07 AC-12「同图」）。
fn assert_single_graph_hash(runs: &[FinishedRun], shared_db: &Path) {
    let reader = TraceReader::open_readonly(shared_db).expect("open shared trace db");
    let hashes: HashSet<String> = runs
        .iter()
        .map(|run| {
            reader
                .run(&run.run_id)
                .expect("query run row")
                .expect("run row exists")
                .graph_hash
        })
        .collect();
    assert_eq!(
        hashes.len(),
        1,
        "{} 个 run 应共用同一张图（同 graph_hash），实际 {hashes:?}",
        runs.len()
    );
}

/// 单 run 的完整 trace 断言，返回形状摘要供跨 run 比较。
///
/// **读法**：`TraceReader` 的查询本身已按 `run_id` 过滤，因此下面各处
/// `row.run_id == run.run_id` 是**防御性一致性检查**，不承担隔离职责；真正的隔离
/// 证据是逐 run 的精确计数（实例/gate/区间）、跨 run `instance_id` 互斥，以及同
/// kind run 的 events 形状逐项一致。
fn assert_run_trace(reader: &TraceReader, run: &FinishedRun) -> TraceShape {
    assert_run_identity(reader, run);
    let instance_ids = assert_run_instances(reader, run);
    let event_kinds = assert_run_events(reader, run, &instance_ids);
    let gate_count = assert_run_gate_results(reader, run, &instance_ids);
    assert_run_transitions(reader, run, &instance_ids);
    assert_run_streaming(reader, run, &instance_ids);
    TraceShape {
        instance_ids,
        event_kinds,
        gate_count,
    }
}

/// run 行与入口输入：status/user_id/entry_inputs/graph_name 都属于本 run。
fn assert_run_identity(reader: &TraceReader, run: &FinishedRun) {
    let row = reader
        .run(&run.run_id)
        .expect("query run row")
        .expect("run row exists");
    assert_eq!(row.status, zgraph_trace::RunStatus::Completed);
    assert_eq!(
        row.user_id, run.plan.user_id,
        "{} user_id 不得串",
        run.plan.tag
    );
    let expected_graph = match run.plan.kind {
        RunKind::Python => "e2e_ac12_python",
        RunKind::Agent => "e2e_ac12_agent",
    };
    assert_eq!(row.graph_name.as_deref(), Some(expected_graph));
    let entry = row.entry_inputs.expect("entry_inputs snapshot");
    let tag = match run.plan.kind {
        RunKind::Python => &entry["start"]["x"]["run_tag"],
        RunKind::Agent => &entry["start"]["message"]["run_tag"],
    };
    assert_eq!(tag, &json!(run.plan.tag), "entry_inputs 只含本 run 输入");
}

/// 实例集合与输入/输出：节点集合、终态、run_id 归属、user_id/tenant 回显。
fn assert_run_instances(reader: &TraceReader, run: &FinishedRun) -> HashSet<String> {
    let expected: HashSet<String> = match run.plan.kind {
        RunKind::Python => PYTHON_NODES
            .iter()
            .map(|node| (*node).to_string())
            .collect(),
        RunKind::Agent => AGENT_NODES.iter().map(|node| (*node).to_string()).collect(),
    };
    let instances = reader.instances(&run.run_id).expect("query instances");
    assert_eq!(
        instances.len(),
        expected.len(),
        "{} 实例数（每节点一个实例）",
        run.plan.tag
    );
    let mut nodes = HashSet::new();
    let mut ids = HashSet::new();
    for row in &instances {
        assert_eq!(row.run_id, run.run_id, "instances.run_id 不得串");
        assert_eq!(
            row.status,
            TraceInstanceStatus::Completed,
            "{} 节点 {} 应 Completed",
            run.plan.tag,
            row.node_id
        );
        assert!(row.error_code.is_none(), "Completed 实例无 error_code");
        assert!(nodes.insert(row.node_id.clone()), "节点重复实例");
        ids.insert(row.instance_id.clone());
        assert_instance_payloads(run, row);
    }
    assert_eq!(nodes, expected, "{} 实例节点集合", run.plan.tag);
    ids
}

/// 实例输入/输出断言分派。
fn assert_instance_payloads(run: &FinishedRun, row: &zgraph_trace::InstanceRow) {
    match run.plan.kind {
        RunKind::Python => assert_python_instance(run, row),
        RunKind::Agent => assert_agent_instance(run, row),
    }
}

/// 纯 python 图实例：start 输出/分支输入都带本 run 的 run_tag；分支输出回显 user_id/tenant。
fn assert_python_instance(run: &FinishedRun, row: &zgraph_trace::InstanceRow) {
    let tag = json!(run.plan.tag);
    let user_id = json!(run.plan.user_id);
    let tenant = json!(run.plan.tenant);
    match row.node_id.as_str() {
        "start" => {
            let output = row.output_json.as_ref().expect("start output");
            assert_eq!(output["x"]["run_tag"], tag);
            assert!(output["x"]["log_file"].is_string());
        }
        node @ ("a" | "b" | "c") => {
            let output = row.output_json.as_ref().expect("branch output");
            assert_eq!(output["tag"], json!(node));
            assert_eq!(output["run_tag"], tag);
            assert_eq!(output["user_id"], user_id, "实例输出 user_id 不得串");
            assert_eq!(output["tenant"], tenant, "实例输出 global_vars 不得串");
            let input = row.input.as_ref().expect("branch input");
            assert_eq!(
                input["x"]["x"]["run_tag"], tag,
                "分支输入只含本 run payload"
            );
        }
        _ => {}
    }
}

/// agent 图实例：echo 回显本 run 的 user_id/tenant；answer_writer 输出四字段（05 §5.9.2）。
fn assert_agent_instance(run: &FinishedRun, row: &zgraph_trace::InstanceRow) {
    let tag = json!(run.plan.tag);
    match row.node_id.as_str() {
        "start" => {
            let output = row.output_json.as_ref().expect("start output");
            assert_eq!(output["message"]["run_tag"], tag);
        }
        "echo" => {
            let output = row.output_json.as_ref().expect("echo output");
            assert_eq!(output["run_tag"], tag);
            assert_eq!(output["user_id"], json!(run.plan.user_id));
            assert_eq!(output["tenant"], json!(run.plan.tenant));
            assert!(output["message"].is_string(), "echo 应透传 message 文本");
        }
        "answer_writer" => {
            let output = row.output_json.as_ref().expect("agent output");
            assert_eq!(output["content"], json!("{\"answer\":\"5\"}"));
            assert_eq!(output["stop_reason"], json!("EndTurn"));
            assert_eq!(output["iterations"], json!(2), "两轮 LLM（tool loop）");
            let total = output["usage"]["total_tokens"]
                .as_u64()
                .expect("total_tokens");
            assert!(total > 0, "usage.total_tokens 必须 > 0");
        }
        _ => {}
    }
}

/// 事件：run_id/instance_id 归属、生命周期形状、RunStarted.user_id；返回 kind 计数。
fn assert_run_events(
    reader: &TraceReader,
    run: &FinishedRun,
    instance_ids: &HashSet<String>,
) -> BTreeMap<String, usize> {
    let events = common::events(reader, &run.run_id);
    assert!(!events.is_empty(), "{} 事件非空", run.plan.tag);
    for event in &events {
        assert_eq!(event.run_id, run.run_id, "events.run_id 不得串");
        if let Some(instance_id) = &event.instance_id {
            assert!(
                instance_ids.contains(instance_id),
                "事件实例 {instance_id} 不属于 run {}",
                run.plan.tag
            );
        }
    }
    common::assert_lifecycle_shape(reader, &run.run_id);
    let started = events.first().expect("RunStarted event");
    assert_eq!(
        started.payload["user_id"],
        json!(run.plan.user_id),
        "RunStarted 事件 user_id 不得串"
    );
    let mut kinds: BTreeMap<String, usize> = BTreeMap::new();
    for event in &events {
        *kinds.entry(event.kind.clone()).or_insert(0) += 1;
    }
    if run.plan.kind == RunKind::Agent {
        // 流式段按 100ms 聚合，段数受时序影响；其余事件形状应与首个 agent run 一致。
        kinds.remove("Streaming");
    }
    kinds
}

/// gate_results：行数、run_id/instance_id 归属、全部放行且无 gate error。
fn assert_run_gate_results(
    reader: &TraceReader,
    run: &FinishedRun,
    instance_ids: &HashSet<String>,
) -> usize {
    let expected = match run.plan.kind {
        RunKind::Python => PYTHON_GATE_RESULTS_PER_RUN,
        RunKind::Agent => AGENT_GATE_RESULTS_PER_RUN,
    };
    let gates = reader
        .gate_results(&run.run_id)
        .expect("query gate_results");
    assert_eq!(gates.len(), expected, "{} gate_results 行数", run.plan.tag);
    for gate in &gates {
        assert_eq!(gate.run_id, run.run_id, "gate_results.run_id 不得串");
        assert!(gate.passed, "本验收图无拦截 gate，投递应全部放行");
        assert!(gate.error.is_none());
        assert!(
            instance_ids.contains(&gate.instance_id),
            "gate 发布实例不属于 run {}",
            run.plan.tag
        );
    }
    gates.len()
}

/// transitions：非空、run_id/instance_id 归属本 run。
fn assert_run_transitions(reader: &TraceReader, run: &FinishedRun, instance_ids: &HashSet<String>) {
    let transitions = reader.transitions(&run.run_id).expect("query transitions");
    assert!(!transitions.is_empty(), "{} transitions 非空", run.plan.tag);
    for transition in &transitions {
        assert_eq!(transition.run_id, run.run_id, "transitions.run_id 不得串");
        assert!(
            instance_ids.contains(&transition.instance_id),
            "transition 实例不属于 run {}",
            run.plan.tag
        );
    }
}

/// 流式段：python run 无流式（不得混入 agent run 的 token）；agent run 仅本 run 且 token 拼接正确。
fn assert_run_streaming(reader: &TraceReader, run: &FinishedRun, instance_ids: &HashSet<String>) {
    let segments = reader
        .streaming_events(&run.run_id)
        .expect("query streaming_events");
    if run.plan.kind == RunKind::Python {
        assert!(
            segments.is_empty(),
            "纯 python run 不应有流式段（不得串 agent run）"
        );
        return;
    }
    assert!(!segments.is_empty(), "agent run 应有流式段");
    for segment in &segments {
        assert_eq!(segment.run_id, run.run_id, "streaming.run_id 不得串");
        assert!(instance_ids.contains(&segment.instance_id));
    }
    let mut tokens: Vec<&zgraph_trace::StreamingRow> = segments
        .iter()
        .filter(|segment| segment.kind == "token")
        .collect();
    tokens.sort_by_key(|segment| segment.segment_index);
    let joined: String = tokens
        .iter()
        .filter_map(|segment| segment.text.clone())
        .collect();
    assert_eq!(
        joined, "{\"answer\":\"5\"}",
        "token 段拼接 == agent content"
    );
}

/// 同图 run 的 trace 形状必须一致（事件 kind 计数/门控数/实例数），跨 run 串写会破坏形状。
fn assert_shapes_consistent(runs: &[FinishedRun], shapes: &[TraceShape]) {
    assert_eq!(runs.len(), shapes.len());
    let mut first: HashMap<RunKind, usize> = HashMap::new();
    for (index, run) in runs.iter().enumerate() {
        match first.entry(run.plan.kind) {
            Entry::Vacant(slot) => {
                slot.insert(index);
            }
            Entry::Occupied(slot) => {
                let base = &shapes[*slot.get()];
                let shape = &shapes[index];
                assert_eq!(
                    shape.event_kinds,
                    base.event_kinds,
                    "{} 与 {} 的 events 形状应一致",
                    run.plan.tag,
                    runs[*slot.get()].plan.tag
                );
                assert_eq!(shape.gate_count, base.gate_count);
                assert_eq!(shape.instance_ids.len(), base.instance_ids.len());
            }
        }
    }
}

/// 纯 python run 输出：end 的三个端口都携带本 run 的 run_tag/user_id/tenant。
fn assert_python_outputs(runs: &[FinishedRun]) {
    for run in runs.iter().filter(|run| run.plan.kind == RunKind::Python) {
        let output = run
            .outcome
            .outcome
            .output
            .as_ref()
            .unwrap_or_else(|| panic!("{} end output", run.plan.tag));
        for port in ["ra", "rb", "rc"] {
            assert_eq!(output[port]["run_tag"], json!(run.plan.tag), "端口 {port}");
            assert_eq!(output[port]["user_id"], json!(run.plan.user_id));
            assert_eq!(output[port]["tenant"], json!(run.plan.tenant));
        }
    }
}

/// agent run 输出与 fake LLM 请求：四字段形状、两轮请求、工具真实执行（add=5）。
fn assert_agent_outputs(runs: &[FinishedRun]) {
    for run in runs.iter().filter(|run| run.plan.kind == RunKind::Agent) {
        let output = run
            .outcome
            .outcome
            .output
            .as_ref()
            .unwrap_or_else(|| panic!("{} end output", run.plan.tag));
        let result = &output["result"];
        assert_eq!(result["content"], json!("{\"answer\":\"5\"}"));
        assert_eq!(result["stop_reason"], json!("EndTurn"));
        assert_eq!(result["iterations"], json!(2));
        let fake = run.fake.as_ref().expect("agent run 应有独立 fake provider");
        let requests = fake.requests();
        assert_eq!(requests.len(), 2, "{} 两轮 LLM 请求", run.plan.tag);
        let tool_payload = requests[1]
            .messages
            .iter()
            .find_map(|message| match message {
                LlmMessage::Tool { content, .. } => Some(content.as_str()),
                _ => None,
            })
            .unwrap_or_else(|| panic!("{} 第二轮应带 Tool 消息", run.plan.tag));
        let payload: Value = serde_json::from_str(tool_payload).expect("Tool 消息为 JSON");
        assert_eq!(payload["sum"], json!(5), "工具 add 由真 worker 执行");
    }
}

// ---------------------------------------------------------------------------
// 并发上限度量 + 耗时基线
// ---------------------------------------------------------------------------

/// 真实进程区间峰值：全局 ≤ with_limits 全局上限；每 run ≤ 每 run 上限；记录耗时比。
///
/// 口径边界（ISSUE-079）：区间只覆盖 `WorkerExecutor` 的 python 节点子进程
/// （分支/echo）；agent 工具子进程走 Harness 的 `WorkerToolRunner` 独立信号量，不在
/// 此统计内（M6 统一后再扩展）。峰值是节点进程并发的下界，断言仍对配置上限有效。
fn assert_worker_limits(
    log_file: &Path,
    runs: &[FinishedRun],
    baseline: Duration,
    elapsed: Duration,
) {
    let intervals = common::read_intervals_by_run(log_file);
    assert_eq!(intervals.len(), runs.len(), "每个 run 都应记录进程区间");
    let all: Vec<(f64, f64)> = intervals.values().flatten().copied().collect();
    let expected_total: usize = runs
        .iter()
        .map(|run| match run.plan.kind {
            RunKind::Python => PYTHON_INTERVALS_PER_RUN,
            RunKind::Agent => AGENT_INTERVALS_PER_RUN,
        })
        .sum();
    assert_eq!(all.len(), expected_total, "区间总数（真进程证据）");
    let global_peak = common::peak_concurrency(&all);
    assert!(
        global_peak <= GLOBAL_MAX_WORKERS,
        "跨 run 峰值 {global_peak} 超过全局 max_workers={GLOBAL_MAX_WORKERS}"
    );
    assert!(
        global_peak >= 2,
        "全局上限 4 退化为串行：峰值 {global_peak}"
    );

    let per_run_peaks = per_run_peaks(runs, &intervals);
    let observed_max = per_run_peaks.iter().copied().max().unwrap_or(0);
    assert_eq!(
        observed_max, MAX_WORKERS_PER_RUN,
        "每 run 峰值应达到且不超过 max_workers_per_run={MAX_WORKERS_PER_RUN}（实际 {observed_max}）"
    );

    let ratio = elapsed.as_secs_f64() / baseline.as_secs_f64();
    assert!(
        ratio < ELAPSED_RATIO_LIMIT,
        "10 run 总耗时/单 run = {ratio:.2} 超过宽松上限 {ELAPSED_RATIO_LIMIT}"
    );
    // 基线记录（不参与断言）：本机实测 baseline≈0.79–0.85s、total≈2.46–2.54s、
    // 比值≈2.9–3.1（与文件头「实测基线」一致；机器/负载不同会有波动）。
    eprintln!(
        "[AC-12] 区间组={} 全局峰值={global_peak} 每run峰值={per_run_peaks:?} \
         单run基线={baseline:?} 10run总耗时={elapsed:?} 比值={ratio:.2}",
        intervals.len()
    );
}

/// 每个 run 的峰值并发（python run 应 3 区间、agent run 应 1 区间，且均 ≤ 每 run 上限）。
fn per_run_peaks(
    runs: &[FinishedRun],
    intervals: &BTreeMap<String, Vec<(f64, f64)>>,
) -> Vec<usize> {
    let mut peaks = Vec::with_capacity(runs.len());
    for run in runs {
        let expected = match run.plan.kind {
            RunKind::Python => PYTHON_INTERVALS_PER_RUN,
            RunKind::Agent => AGENT_INTERVALS_PER_RUN,
        };
        let spans = intervals
            .get(&run.plan.tag)
            .unwrap_or_else(|| panic!("{} 缺进程区间", run.plan.tag));
        assert_eq!(spans.len(), expected, "{} 区间数", run.plan.tag);
        let peak = common::peak_concurrency(spans);
        assert!(
            peak <= MAX_WORKERS_PER_RUN,
            "{} 峰值 {peak} 超过每 run max_workers_per_run={MAX_WORKERS_PER_RUN}",
            run.plan.tag
        );
        peaks.push(peak);
    }
    peaks
}

// ---------------------------------------------------------------------------
// 用例（8+2 全栈组合 / spec 07 原文「10×同图」形态）
// ---------------------------------------------------------------------------

/// AC-12：10 个 run 并发（同一 SQLite 库、不同 run_id）全栈隔离 + 并发上限。
#[tokio::test]
async fn ac12_ten_concurrent_runs_share_one_db_and_are_isolated() {
    let dir = tempfile::tempdir().expect("tempdir");
    let shared_db = dir.path().join("ac12_shared.db");
    let log_file = dir.path().join("ac12_intervals.log");
    let worker =
        common::executor_with_limits(common::RUN_TIMEOUT, GLOBAL_MAX_WORKERS, MAX_WORKERS_PER_RUN);
    let baseline = run_single_baseline(
        dir.path(),
        worker.clone(),
        &dir.path().join("ac12_baseline.log"),
    )
    .await;

    let started = Instant::now();
    let launched = launch_runs(&shared_db, &log_file, &worker, RunPlan::new);
    let runs = tokio::time::timeout(common::RUN_TIMEOUT, finish_runs(launched))
        .await
        .expect("10 个并发 run 必须在宽松上限内全部结束");
    let elapsed = started.elapsed();

    assert_all_completed(&runs);
    assert_runs_isolated(&runs, &shared_db);
    assert_python_outputs(&runs);
    assert_agent_outputs(&runs);
    assert_worker_limits(&log_file, &runs, baseline, elapsed);
}

/// AC-12（spec 07 原文形态，ISSUE-089 / REV-032 加强）：**同一张图** 10 个并发 run。
///
/// M5 的 8+2 组合虽覆盖更强核心语义，但未字面满足「10 个同图 run」——这里把 10 个
/// run 全部换成同一条三并发分支图，并额外断言 10 个 `runs.graph_hash` 完全一致；
/// run 间隔离（instances/gate_results/events/transitions/streaming 精确计数 +
/// `instance_id` 互斥 + 同形）与全局/每 run 上限沿用同一套 helper，避免两套口径。
#[tokio::test]
async fn ac12_ten_same_graph_runs_are_isolated_and_bounded() {
    let dir = tempfile::tempdir().expect("tempdir");
    let shared_db = dir.path().join("ac12_same_graph.db");
    let log_file = dir.path().join("ac12_same_graph_intervals.log");
    let worker =
        common::executor_with_limits(common::RUN_TIMEOUT, GLOBAL_MAX_WORKERS, MAX_WORKERS_PER_RUN);
    let baseline = run_single_baseline(
        dir.path(),
        worker.clone(),
        &dir.path().join("ac12_same_graph_baseline.log"),
    )
    .await;

    let started = Instant::now();
    let launched = launch_runs(&shared_db, &log_file, &worker, RunPlan::same_graph);
    let runs = tokio::time::timeout(common::RUN_TIMEOUT, finish_runs(launched))
        .await
        .expect("10 个同图并发 run 必须在宽松上限内全部结束");
    let elapsed = started.elapsed();

    assert_all_completed(&runs);
    assert_runs_isolated(&runs, &shared_db);
    assert_single_graph_hash(&runs, &shared_db);
    assert_python_outputs(&runs);
    assert_worker_limits(&log_file, &runs, baseline, elapsed);
}
