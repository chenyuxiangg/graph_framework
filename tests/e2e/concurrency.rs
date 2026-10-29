//! AC-12 前置（M3 真 worker 口径，D4/D8）：全局 `max_workers` 与每 run
//! `max_workers_per_run` 生效 + run 间隔离基础。
//!
//! 语义基准：`doc/spec-v1.2/07-验收用例.md` AC-12 + `doc/architecture-v2.md` §8
//! D4（每节点一进程；全局 CPU×2 + 每 run 8，取消 CPU=1/IO=4 信号量）与 D8
//! （AC-12 改为「全局/每 run worker 上限生效、run 间隔离」）。
//!
//! 并发上限用**真实进程区间**度量：fixture 把每次执行的 `start`/`end` 墙钟追加到
//! 共享日志，测试扫描区间求峰值并发——峰值为 1 = 串行、2/3 = 真并行。
//! run 间隔离：两个 run 并发启动（独立 trace DB / 日志），断言 outcome、实例集合、
//! 事件 run_id、输出 tag 互不串。

#![cfg(unix)]

#[path = "common/mod.rs"]
mod common;

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::Arc;

use serde_json::{json, Value};

use zgraph_cli::bridge::{start_traced_run, BridgeOutcome};
use zgraph_kernel::{CompiledGraph, RunSpec, RunStatus};
use zgraph_worker::WorkerExecutor;

/// 分支执行时长（REV-015 #6：取 0.6s，让「三路同时 Running」的窗口对 2 核 CI 也足够宽）。
const BRANCH_SLEEP_SECONDS: f64 = 0.6;

/// 三并发分支图的节点集合。
const BRANCH_NODES: [&str; 5] = ["start", "a", "b", "c", "done"];

/// 三个并行分支各写一行 start/end；`done` 等三端口（`trigger=all`）。
fn three_branch_graph() -> Arc<CompiledGraph> {
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
            "config": {"module": "e2e_fixtures.concurrency", "function": branch}
        }));
        edges.push(json!({"from": "start", "to_node": branch, "to_input_port": "x"}));
        edges.push(json!({"from": branch, "to_node": "done", "to_input_port": port}));
    }
    let text = json!({
        "version": "1.0",
        "metadata": {"name": "e2e_three_branch"},
        "entry_points": ["start"],
        "nodes": nodes,
        "edges": edges
    })
    .to_string();
    common::compile_text(&text)
}

/// 分支 payload（同一日志文件 → 三个进程的区间可比较；`run_tag` 供跨 run 配对）。
fn branch_payload(log_file: &Path, run_tag: &str) -> Value {
    json!({
        "log_file": log_file.display().to_string(),
        "sleep": BRANCH_SLEEP_SECONDS,
        "run_tag": run_tag,
    })
}

/// 一个待并发启动的 run 描述（独立 trace DB + 日志 + run_tag）。
struct BranchRun<'a> {
    /// trace DB 文件名。
    db_name: &'a str,
    /// 区间日志路径（global 用例两个 run 共享同一路径）。
    log_file: &'a Path,
    /// run 级 tag（写入日志配对键与输出）。
    run_tag: &'a str,
}

/// 三并发分支图的 `RunSpec`。
fn branch_spec(log_file: &Path, run_tag: &str) -> RunSpec {
    common::spec(
        three_branch_graph(),
        common::entry_inputs(&[("start", "x", branch_payload(log_file, run_tag))]),
    )
}

/// 并发启动两个三并发分支 run 并等待两者结束。
async fn run_two_concurrent(
    dir: &tempfile::TempDir,
    first: BranchRun<'_>,
    second: BranchRun<'_>,
    executor: Arc<WorkerExecutor>,
) -> (BridgeOutcome, BridgeOutcome) {
    let traced_one = start_traced_run(
        branch_spec(first.log_file, first.run_tag),
        executor.clone(),
        dir.path().join(first.db_name),
    )
    .expect("start run one");
    let traced_two = start_traced_run(
        branch_spec(second.log_file, second.run_tag),
        executor.clone(),
        dir.path().join(second.db_name),
    )
    .expect("start run two");
    let (one, two) = tokio::time::timeout(common::RUN_TIMEOUT, async {
        tokio::join!(traced_one.wait(), traced_two.wait())
    })
    .await
    .expect("both runs must finish");
    (
        one.expect("run one traced outcome"),
        two.expect("run two traced outcome"),
    )
}

/// 从共享日志读出执行区间 `(start, end)`（秒，同主机墙钟）。
///
/// 每行 `<run_tag> <tag> <pid> <wall-clock> start|end`；配对键
/// `(run_tag, tag, pid)`——一个节点实例一个进程、只写一次 start/end；带 `run_tag`
/// 后即使跨 run PID 复用也不会错配（REV-015 #10）。
fn read_intervals(path: &Path) -> Vec<(f64, f64)> {
    let text = std::fs::read_to_string(path)
        .unwrap_or_else(|error| panic!("interval log {}: {error}", path.display()));
    let mut starts: HashMap<String, f64> = HashMap::new();
    let mut intervals = Vec::new();
    for line in text.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() != 5 {
            continue;
        }
        let key = format!("{}/{}/{}", parts[0], parts[1], parts[2]);
        let timestamp: f64 = parts[3].parse().expect("interval timestamp");
        match parts[4] {
            "start" => {
                starts.insert(key, timestamp);
            }
            "end" => {
                if let Some(started) = starts.remove(&key) {
                    intervals.push((started, timestamp));
                }
            }
            other => panic!("unexpected interval log field {other}"),
        }
    }
    intervals
}

/// 区间列表的峰值并发数（同一时刻先结算 end 再算 start）。
fn peak_concurrency(intervals: &[(f64, f64)]) -> usize {
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

/// 跑一次三并发分支图（真实 worker + 指定并发上限），返回 outcome 与峰值并发。
async fn run_branches(
    dir: &tempfile::TempDir,
    db_name: &str,
    log_file: &Path,
    executor: Arc<WorkerExecutor>,
) -> (BridgeOutcome, usize) {
    let db = dir.path().join(db_name);
    let spec = branch_spec(log_file, db_name);
    let bridge = tokio::time::timeout(
        common::RUN_TIMEOUT,
        common::run_to_trace(spec, executor, &db),
    )
    .await
    .expect("branch run must finish");
    let intervals = read_intervals(log_file);
    assert_eq!(
        intervals.len(),
        3,
        "三个分支都应记录执行区间（真实进程证据）"
    );
    (bridge, peak_concurrency(&intervals))
}

/// outcome 级隔离：两端各自 Completed，end 输出带自己的 `run_tag`。
fn assert_outcomes_isolated(one: &BridgeOutcome, two: &BridgeOutcome) {
    assert_eq!(one.outcome.status, RunStatus::Completed);
    assert_eq!(two.outcome.status, RunStatus::Completed);
    let one_output = one.outcome.output.as_ref().expect("run one output");
    let two_output = two.outcome.output.as_ref().expect("run two output");
    assert_eq!(one_output["ra"]["run_tag"], "run-one", "{one_output}");
    assert_eq!(two_output["ra"]["run_tag"], "run-two", "{two_output}");
}

/// trace 级隔离：实例集合只属于自己、事件 run_id 不串、实例 id 不相交。
fn assert_traces_isolated(one: &BridgeOutcome, two: &BridgeOutcome) {
    let one_id = one.outcome.run_id.as_str();
    let two_id = two.outcome.run_id.as_str();
    let one_instances = one.reader().instances(one_id).expect("one instances");
    let two_instances = two.reader().instances(two_id).expect("two instances");
    let expected: HashSet<String> = BRANCH_NODES
        .iter()
        .map(|node| (*node).to_string())
        .collect();
    let node_set = |instances: &[zgraph_trace::InstanceRow]| -> HashSet<String> {
        instances.iter().map(|row| row.node_id.clone()).collect()
    };
    assert_eq!(node_set(&one_instances), expected, "run one 实例集合");
    assert_eq!(node_set(&two_instances), expected, "run two 实例集合");
    let one_ids: HashSet<&str> = one_instances
        .iter()
        .map(|row| row.instance_id.as_str())
        .collect();
    let two_ids: HashSet<&str> = two_instances
        .iter()
        .map(|row| row.instance_id.as_str())
        .collect();
    assert!(
        one_ids.is_disjoint(&two_ids),
        "两个 run 的实例 id 不得相交（run 级状态隔离）"
    );
    for event in common::events(one.reader(), one_id) {
        assert_eq!(event.run_id, one_id, "run one 事件不得混入其它 run");
    }
    for event in common::events(two.reader(), two_id) {
        assert_eq!(event.run_id, two_id, "run two 事件不得混入其它 run");
    }
    common::assert_lifecycle_shape(one.reader(), one_id);
    common::assert_lifecycle_shape(two.reader(), two_id);
}

// ---------------------------------------------------------------------------
// 并发上限
// ---------------------------------------------------------------------------

#[tokio::test]
async fn per_run_limit_caps_concurrency_within_one_run() {
    let dir = tempfile::tempdir().expect("tempdir");
    let log = dir.path().join("per_run.log");
    // 每 run 上限 2：3 个分支里最多 2 个同时在飞，且确实出现 2 路并行。
    let executor = common::executor_with_limits(common::RUN_TIMEOUT, 8, 2);
    let (bridge, peak) = run_branches(&dir, "per_run.db", &log, executor).await;
    assert_eq!(
        bridge.outcome.status,
        RunStatus::Completed,
        "{:?}",
        bridge.outcome.error
    );
    assert_eq!(
        peak, 2,
        "max_workers_per_run=2 时峰值并发应为 2（既受限又有并行），实际 {peak}"
    );
}

#[tokio::test]
async fn global_limit_caps_concurrency_across_two_concurrent_runs() {
    let dir = tempfile::tempdir().expect("tempdir");
    // 两个 run 共享同一日志：全局上限 2 → 跨 run 峰值并发也不得超过 2。
    let log = dir.path().join("global.log");
    let executor = common::executor_with_limits(common::RUN_TIMEOUT, 2, 8);
    let (one, two) = run_two_concurrent(
        &dir,
        BranchRun {
            db_name: "global_run_one.db",
            log_file: &log,
            run_tag: "run-one",
        },
        BranchRun {
            db_name: "global_run_two.db",
            log_file: &log,
            run_tag: "run-two",
        },
        executor,
    )
    .await;
    assert_eq!(one.outcome.status, RunStatus::Completed);
    assert_eq!(two.outcome.status, RunStatus::Completed);

    let intervals = read_intervals(&log);
    assert_eq!(intervals.len(), 6, "两个 run 各 3 个分支区间");
    let peak = peak_concurrency(&intervals);
    assert!(
        peak <= 2,
        "全局 max_workers=2 时跨 run 峰值并发应 <= 2，实际 {peak}"
    );
    assert!(
        peak >= 2,
        "上限 2 不应退化为串行（应观测到 2 路并行），实际 {peak}"
    );
}

#[tokio::test]
async fn default_limits_let_all_three_branches_overlap() {
    let dir = tempfile::tempdir().expect("tempdir");
    let log = dir.path().join("default.log");
    let executor = common::executor_with_limits(common::RUN_TIMEOUT, 8, 8);
    let (bridge, peak) = run_branches(&dir, "default.db", &log, executor).await;
    assert_eq!(bridge.outcome.status, RunStatus::Completed);
    assert_eq!(
        peak, 3,
        "上限充足时三个分支应同时执行（真并行），实际峰值 {peak}"
    );
}

// ---------------------------------------------------------------------------
// run 间隔离
// ---------------------------------------------------------------------------

#[tokio::test]
async fn concurrent_runs_are_isolated_in_outcome_instances_and_trace() {
    let dir = tempfile::tempdir().expect("tempdir");
    let log_one = dir.path().join("isolated_one.log");
    let log_two = dir.path().join("isolated_two.log");
    let executor = common::executor_with_limits(common::RUN_TIMEOUT, 8, 8);

    let (one, two) = run_two_concurrent(
        &dir,
        BranchRun {
            db_name: "isolated_one.db",
            log_file: &log_one,
            run_tag: "run-one",
        },
        BranchRun {
            db_name: "isolated_two.db",
            log_file: &log_two,
            run_tag: "run-two",
        },
        executor,
    )
    .await;
    assert_ne!(
        one.outcome.run_id, two.outcome.run_id,
        "两个 run 的 run_id 必须不同"
    );
    assert_outcomes_isolated(&one, &two);
    assert_traces_isolated(&one, &two);

    // 每个 run 只有自己的 3 个分支区间（日志文件不串）。
    assert_eq!(read_intervals(&log_one).len(), 3);
    assert_eq!(read_intervals(&log_two).len(), 3);
}
