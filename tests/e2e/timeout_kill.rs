//! AC-06（M3 真 worker 口径，D1）：超时 = **杀进程组**，无残留子进程，run 在 5s 内失败。
//!
//! 语义基准：`doc/spec-v1.2/07-验收用例.md` AC-06 + `doc/architecture-v2.md` §8 D1：
//!
//! - 实例 `TimedOut`、run `Failed`（`NODE_TIMEOUT`）、trace 有 `NodeTimedOut`、
//!   `killed_workers ≥ 1`、`NodeAbandoned` 不产生、`abandoned_tasks` 恒 0；
//! - 超时/取消对**进程组**（Unix `setsid`）发 SIGKILL：worker 与其派生孙进程的 PID
//!   都必须从进程表消失（`kill(pid, 0)` 轮询，禁止 sleep 猜测）。
//!
//! 真进程证据由 `tests/e2e/fixtures`（SDK fixture 写 pid / raw 脚本）提供；
//! 本文件不使用任何 mock。

#![cfg(unix)]

#[path = "common/mod.rs"]
mod common;

use std::time::{Duration, Instant};

use serde_json::json;

use zgraph_kernel::RunStatus;
use zgraph_trace::InstanceStatus as TraceInstanceStatus;

/// 07 AC-06 的节点级超时（毫秒）：主用例（`workers.slow`）按 07 原文取 1000ms。
const NODE_TIMEOUT_MS: u64 = 1_000;

/// 孙进程用例的节点级超时（毫秒）：放宽到 1500ms（REV-015 #5）。
///
/// 该用例的 fixture 必须先写 worker pid、完成握手、派孙进程并把 child pid 落盘，
/// 慢机冷启动可能 >1s；一旦先 kill 则 child pid 文件不存在 → 测试侧断言证据缺失。
/// 1500ms 仍远小于 AC-06 的 5s 上限。
const GRANDCHILD_TIMEOUT_MS: u64 = 1_500;

/// 真 worker 冷启动余量：超时判定用 50ms watchdog tick 轮询，避免默认 1s tick 拖慢。
const WATCHDOG_TICK_MS: u64 = 50;

/// 断言 trace 的超时语义（D1：TimedOut + NodeTimedOut 一次 + 无 NodeAbandoned）。
fn assert_timeout_trace(reader: &zgraph_trace::TraceReader, run_id: &str, node_id: &str) {
    common::assert_lifecycle_shape(reader, run_id);
    let run = common::run_row(reader, run_id);
    assert_eq!(
        run.status,
        zgraph_trace::RunStatus::Failed,
        "trace runs.status"
    );
    assert_eq!(run.error_code.as_deref(), Some("NODE_TIMEOUT"));
    assert!(
        run.killed_workers >= 1,
        "D1: killed_workers 应 >= 1，实际 {}",
        run.killed_workers
    );
    assert_eq!(run.abandoned_tasks, 0, "D1: abandoned_tasks 在 v2 恒为 0");

    let instance = common::instance(reader, run_id, node_id);
    assert_eq!(
        instance.status,
        TraceInstanceStatus::TimedOut,
        "节点 {node_id} 应迁移到 TimedOut"
    );
    assert_eq!(instance.error_code.as_deref(), Some("NODE_TIMEOUT"));

    let all = common::events(reader, run_id);
    assert_eq!(
        common::count_kind(&all, "NodeTimedOut"),
        1,
        "NodeTimedOut 恰一次"
    );
    assert_eq!(
        common::count_kind(&all, "NodeAbandoned"),
        0,
        "D1: v2 不产生 NodeAbandoned"
    );
    let finished = common::run_finished_payload(reader, run_id);
    assert_eq!(finished["status"], "Failed");
    assert_eq!(finished["error"]["code"], "NODE_TIMEOUT");
}

#[tokio::test]
async fn ac06_slow_worker_timeout_fails_run_kills_worker_within_5s() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db = dir.path().join("ac06_slow.db");

    // 真示例 worker `python/workers/slow.py`：sleep 30s，节点 timeout_ms=1000。
    let graph = common::sleep_seconds_graph("workers.slow", "run", Some(NODE_TIMEOUT_MS));
    let inputs = common::entry_inputs(&[("start", "seconds", json!(30.0))]);
    let spec = common::spec_with(graph, inputs, |options| {
        options.tick_interval_ms = WATCHDOG_TICK_MS;
    });

    let started = Instant::now();
    let bridge = tokio::time::timeout(
        Duration::from_secs(5),
        common::run_to_trace(spec, common::executor(common::RUN_TIMEOUT), &db),
    )
    .await
    .expect("AC-06: run 必须在 5s 内失败返回，不能等待 30s");
    let elapsed = started.elapsed();
    assert!(
        elapsed < Duration::from_secs(5),
        "D1: 超时 kill 后 run 必须 5s 内失败，实际 {elapsed:?}"
    );
    assert!(
        elapsed >= Duration::from_millis(900),
        "节点 timeout_ms=1000 应被遵守（过早就说明不是超时路径），实际 {elapsed:?}"
    );

    assert_eq!(bridge.outcome.status, RunStatus::Failed);
    let error = bridge.outcome.error.as_ref().expect("failed run error");
    assert_eq!(error.code, "NODE_TIMEOUT", "unexpected error: {error:?}");
    assert!(bridge.outcome.metrics.killed_workers >= 1);
    assert_eq!(bridge.outcome.metrics.abandoned_tasks, 0, "D1: 恒 0");

    let run_id = bridge.outcome.run_id.clone();
    assert_timeout_trace(bridge.reader(), &run_id, "work");
    assert_eq!(
        common::run_row(bridge.reader(), &run_id).status,
        zgraph_trace::RunStatus::Failed
    );
}

#[tokio::test]
async fn ac06_timeout_kills_process_group_including_grandchild() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db = dir.path().join("ac06_grandchild.db");
    let paths = common::PidPaths::new();

    // e2e fixture：worker 自己再派一个 sleep 孙进程，两个 pid 都落盘后长睡。
    let graph = common::linear_graph(
        "e2e_fixtures.descendants",
        "run",
        Some(GRANDCHILD_TIMEOUT_MS),
    );
    let inputs = common::entry_inputs(&[(
        "start",
        "x",
        json!({
            "worker_pid_file": paths.worker.display().to_string(),
            "child_pid_file": paths.child.display().to_string(),
        }),
    )]);
    let spec = common::spec_with(graph, inputs, |options| {
        options.tick_interval_ms = WATCHDOG_TICK_MS;
    });

    let started = Instant::now();
    let bridge = tokio::time::timeout(
        Duration::from_secs(5),
        common::run_to_trace(spec, common::executor(common::RUN_TIMEOUT), &db),
    )
    .await
    .expect("AC-06: run 必须在 5s 内失败返回");
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "D1: 超时 kill 后 run 必须 5s 内失败，实际 {:?}",
        started.elapsed()
    );

    assert_eq!(bridge.outcome.status, RunStatus::Failed);
    assert_eq!(
        bridge.outcome.error.as_ref().expect("error").code,
        "NODE_TIMEOUT"
    );
    let run_id = bridge.outcome.run_id.clone();
    assert_timeout_trace(bridge.reader(), &run_id, "work");

    // 关键断言：worker 与**孙进程**都在进程组 SIGKILL 后消失（无残留/无僵尸）。
    let worker_pid = common::read_pid(&paths.worker);
    let child_pid = common::read_pid(&paths.child);
    common::assert_pid_gone(worker_pid, common::PID_GONE_TIMEOUT).await;
    common::assert_pid_gone(child_pid, common::PID_GONE_TIMEOUT).await;
}
