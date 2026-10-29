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
use zgraph_kernel::{CompiledGraph, RunStatus};
use zgraph_worker::WorkerExecutor;

/// 分支执行时长（并发窗口要足够长，慢机也不易误判）。
const BRANCH_SLEEP_SECONDS: f64 = 0.3;

/// 三个并行分支各写一行 start/end；`done` 等三端口（`trigger=all`）。
fn three_branch_graph() -> Arc<CompiledGraph> {
    let text = json!({
        "version": "1.0",
        "metadata": {"name": "e2e_three_branch"},
        "entry_points": ["start"],
        "nodes": [
            {"id": "start", "type": "start",
             "inputs": [{"name": "x", "type": "any", "required": true}],
             "config": {}},
            {"id": "a", "type": "python_node",
             "inputs": [{"name": "x", "type": "any", "required": true}],
             "config": {"module": "e2e_fixtures.concurrency", "function": "a"}},
            {"id": "b", "type": "python_node",
             "inputs": [{"name": "x", "type": "any", "required": true}],
             "config": {"module": "e2e_fixtures.concurrency", "function": "b"}},
            {"id": "c", "type": "python_node",
             "inputs": [{"name": "x", "type": "any", "required": true}],
             "config": {"module": "e2e_fixtures.concurrency", "function": "c"}},
            {"id": "done", "type": "end",
             "inputs": [
                 {"name": "ra", "type": "any", "required": true},
                 {"name": "rb", "type": "any", "required": true},
                 {"name": "rc", "type": "any", "required": true}
             ],
             "config": {}}
        ],
        "edges": [
            {"from": "start", "to_node": "a", "to_input_port": "x"},
            {"from": "start", "to_node": "b", "to_input_port": "x"},
            {"from": "start", "to_node": "c", "to_input_port": "x"},
            {"from": "a", "to_node": "done", "to_input_port": "ra"},
            {"from": "b", "to_node": "done", "to_input_port": "rb"},
            {"from": "c", "to_node": "done", "to_input_port": "rc"}
        ]
    })
    .to_string();
    common::compile_text(&text)
}

/// 分支 payload（同一日志文件 → 三个进程的区间可比较）。
fn branch_payload(log_file: &Path, run_tag: &str) -> Value {
    json!({
        "log_file": log_file.display().to_string(),
        "sleep": BRANCH_SLEEP_SECONDS,
        "run_tag": run_tag,
    })
}

/// 从共享日志读出执行区间 `(start, end)`（秒，同主机墙钟）。
///
/// 每行 `<tag> <pid> <wall-clock> start|end`；按 `(tag, pid)` 配对——一个节点实例
/// 一个进程、只写一次 start/end，故配对比「按时间就近」更稳（并行分支 start 可能仅差毫秒）。
fn read_intervals(path: &Path) -> Vec<(f64, f64)> {
    let text = std::fs::read_to_string(path)
        .unwrap_or_else(|error| panic!("interval log {}: {error}", path.display()));
    let mut starts: HashMap<String, f64> = HashMap::new();
    let mut intervals = Vec::new();
    for line in text.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() != 4 {
            continue;
        }
        let key = format!("{}-{}", parts[0], parts[1]);
        let timestamp: f64 = parts[2].parse().expect("interval timestamp");
        match parts[3] {
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
    let inputs = common::entry_inputs(&[("start", "x", branch_payload(log_file, db_name))]);
    let spec = common::spec(three_branch_graph(), inputs);
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

    let db_one = dir.path().join("run_one.db");
    let db_two = dir.path().join("run_two.db");
    let spec_one = common::spec(
        three_branch_graph(),
        common::entry_inputs(&[("start", "x", branch_payload(&log, "run-one"))]),
    );
    let spec_two = common::spec(
        three_branch_graph(),
        common::entry_inputs(&[("start", "x", branch_payload(&log, "run-two"))]),
    );
    let traced_one = start_traced_run(spec_one, executor.clone(), &db_one).expect("start run one");
    let traced_two = start_traced_run(spec_two, executor.clone(), &db_two).expect("start run two");
    let (one, two) = tokio::time::timeout(common::RUN_TIMEOUT, async {
        tokio::join!(traced_one.wait(), traced_two.wait())
    })
    .await
    .expect("both runs must finish");
    let one = one.expect("run one traced outcome");
    let two = two.expect("run two traced outcome");
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

#[tokio::test]
async fn concurrent_runs_are_isolated_in_outcome_instances_and_trace() {
    let dir = tempfile::tempdir().expect("tempdir");
    let log_one = dir.path().join("isolated_one.log");
    let log_two = dir.path().join("isolated_two.log");
    let executor = common::executor_with_limits(common::RUN_TIMEOUT, 8, 8);

    let db_one = dir.path().join("isolated_one.db");
    let db_two = dir.path().join("isolated_two.db");
    let spec_one = common::spec(
        three_branch_graph(),
        common::entry_inputs(&[("start", "x", branch_payload(&log_one, "run-one"))]),
    );
    let spec_two = common::spec(
        three_branch_graph(),
        common::entry_inputs(&[("start", "x", branch_payload(&log_two, "run-two"))]),
    );
    let traced_one = start_traced_run(spec_one, executor.clone(), &db_one).expect("start one");
    let traced_two = start_traced_run(spec_two, executor.clone(), &db_two).expect("start two");
    let run_one_id = traced_one.run_id().to_string();
    let run_two_id = traced_two.run_id().to_string();
    let (one, two) = tokio::time::timeout(common::RUN_TIMEOUT, async {
        tokio::join!(traced_one.wait(), traced_two.wait())
    })
    .await
    .expect("both runs must finish");
    let one = one.expect("run one");
    let two = two.expect("run two");

    assert_ne!(run_one_id, run_two_id, "两个 run 的 run_id 必须不同");
    assert_eq!(one.outcome.status, RunStatus::Completed);
    assert_eq!(two.outcome.status, RunStatus::Completed);

    // outcome 级隔离：各自 end 输出带自己的 run_tag。
    let one_output = one.outcome.output.as_ref().expect("run one output");
    let two_output = two.outcome.output.as_ref().expect("run two output");
    assert_eq!(one_output["ra"]["run_tag"], "run-one", "{one_output}");
    assert_eq!(two_output["ra"]["run_tag"], "run-two", "{two_output}");

    // trace 级隔离：实例集合只属于自己、事件 run_id 不串、实例 id 不相交。
    let one_instances = one.reader().instances(&run_one_id).expect("one instances");
    let two_instances = two.reader().instances(&run_two_id).expect("two instances");
    let node_set = |instances: &[zgraph_trace::InstanceRow]| -> HashSet<String> {
        instances.iter().map(|row| row.node_id.clone()).collect()
    };
    let expected: HashSet<String> = ["start", "a", "b", "c", "done"]
        .into_iter()
        .map(str::to_string)
        .collect();
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

    for event in common::events(one.reader(), &run_one_id) {
        assert_eq!(event.run_id, run_one_id, "run one 事件不得混入其它 run");
    }
    for event in common::events(two.reader(), &run_two_id) {
        assert_eq!(event.run_id, run_two_id, "run two 事件不得混入其它 run");
    }
    common::assert_lifecycle_shape(one.reader(), &run_one_id);
    common::assert_lifecycle_shape(two.reader(), &run_two_id);

    // 每个 run 只有自己的 3 个分支区间（日志文件不串）。
    assert_eq!(read_intervals(&log_one).len(), 3);
    assert_eq!(read_intervals(&log_two).len(), 3);
}
