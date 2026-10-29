//! AC-07 + AC-10（M3 真 worker 口径）：事件循环不阻塞、暂停/恢复、取消 = 杀进程组。
//!
//! 语义基准：`doc/spec-v1.2/07-验收用例.md` AC-07/AC-10 + `doc/architecture-v2.md`
//! §8 D1（取消 = kill 进程组）：
//!
//! - **AC-07 事件循环不阻塞**：真 worker 正在执行（fixture 已写 pid 文件、进入 sleep）
//!   时发 Pause，`Paused` 必须在 **100ms** 内可见——事件循环若同步等待子进程则做不到。
//!   （字面 ticker 断言在 M2 已用 in-process 节点 + `test-hooks.tick_probe` 覆盖；
//!   e2e 按 REV-015 #2 不引入 test-hooks，改用同规格的「控制命令延迟」口径，真 worker
//!   ticker 由 Lead 登记 ISSUE 后续补。）
//! - **AC-10 暂停**：第 1 个执行中暂停 → 第 2 个不派发（等待 1s 无新 pid/新 Running），
//!   Resume 后完成；
//! - **AC-10 取消**：真 worker 在飞时 Cancel → run `Cancelled`、无 `Failed`、在飞实例
//!   `Cancelled`、`RunFinished` 恰一次、`killed_workers > 0`、worker PID 消失
//!   （进程组 kill）；重复 Cancel（以及取消后的 Pause/Resume）幂等。

#![cfg(unix)]

#[path = "common/mod.rs"]
mod common;

use std::path::Path;
use std::time::{Duration, Instant};

use chrono::{DateTime, FixedOffset, Utc};
use serde_json::json;

use zgraph_cli::bridge::{start_traced_run, TracedRun};
use zgraph_kernel::RunStatus;
use zgraph_trace::InstanceStatus as TraceInstanceStatus;

/// AC-07：真 worker 执行期间控制命令（Pause）的生效上限。
const PAUSE_VISIBLE_LIMIT: Duration = Duration::from_millis(100);

/// 暂停窗口：覆盖第 1 个节点的剩余执行时间（sleep 0.5s）后再等 1s（07 AC-10 原文口径）。
const PAUSE_HOLD: Duration = Duration::from_secs(1);

/// 解析 trace 墙钟（RFC3339 毫秒 Z）。
fn parse_at(at: &str) -> DateTime<FixedOffset> {
    DateTime::parse_from_rfc3339(at)
        .unwrap_or_else(|error| panic!("bad trace timestamp {at}: {error}"))
}

/// 断言取消 run 的 trace 形状（AC-10）：Cancelled、无 Crashed/TimedOut、RunFinished 恰一次。
fn assert_cancelled_trace(reader: &zgraph_trace::TraceReader, run_id: &str) {
    common::assert_lifecycle_shape(reader, run_id);
    let run = common::run_row(reader, run_id);
    assert_eq!(
        run.status,
        zgraph_trace::RunStatus::Cancelled,
        "trace runs.status"
    );
    assert!(
        run.error_code.is_none(),
        "取消不是失败：error_code={:?}",
        run.error_code
    );
    assert!(
        run.killed_workers >= 1,
        "D1: 取消在飞 worker 应计入 killed_workers，实际 {}",
        run.killed_workers
    );
    assert_eq!(run.abandoned_tasks, 0, "D1: abandoned_tasks 恒 0");

    let instances = reader.instances(run_id).expect("instances");
    let work = instances
        .iter()
        .find(|row| row.node_id == "work")
        .expect("work instance");
    assert_eq!(
        work.status,
        TraceInstanceStatus::Cancelled,
        "在飞实例应迁到 Cancelled"
    );
    for row in &instances {
        assert!(
            !matches!(
                row.status,
                TraceInstanceStatus::Crashed | TraceInstanceStatus::TimedOut
            ),
            "取消 run 不应出现 Crashed/TimedOut：{row:?}"
        );
    }

    let all = common::events(reader, run_id);
    assert_eq!(common::count_kind(&all, "NodeTimedOut"), 0);
    assert_eq!(common::count_kind(&all, "RunFinished"), 1);
    let finished = common::run_finished_payload(reader, run_id);
    assert_eq!(finished["status"], "Cancelled");
    assert!(finished.get("error").is_none_or(|value| value.is_null()));
}

// ---------------------------------------------------------------------------
// AC-07 + AC-10：暂停/恢复
// ---------------------------------------------------------------------------

/// 等真 worker 开始执行后发 Pause，断言 `Paused` 在 [`PAUSE_VISIBLE_LIMIT`] 内可见。
///
/// 这是 AC-07「事件循环不阻塞」的真 worker 口径：worker 子进程在 `sleep` 期间，
/// 若事件循环同步等待子进程，Pause 只能等 worker 结束才被处理。
async fn assert_pause_visible_while_worker_running(traced: &TracedRun, first_pid: &Path) {
    common::wait_for_file(first_pid).await;
    assert_eq!(traced.status(), RunStatus::Running);

    let started = Instant::now();
    assert!(traced.pause(), "pause command accepted");
    let visible =
        common::wait_until(PAUSE_VISIBLE_LIMIT, || traced.status() == RunStatus::Paused).await;
    let latency = started.elapsed();
    assert!(
        visible,
        "AC-07: 真 worker 执行中 Pause 应在 {PAUSE_VISIBLE_LIMIT:?} 内可见（事件循环不得阻塞），\
         实际 {latency:?}"
    );
    assert_eq!(traced.status(), RunStatus::Paused);
}

/// 暂停期间第 1 个在飞完成，但第 2 个不得被派发（等待 [`PAUSE_HOLD`] 无新 pid 文件）。
async fn assert_pause_blocks_second_dispatch(traced: &TracedRun, second_pid: &Path) {
    tokio::time::sleep(PAUSE_HOLD).await;
    assert_eq!(traced.status(), RunStatus::Paused, "暂停期间不得自行恢复");
    assert!(
        !second_pid.exists(),
        "AC-10: 暂停期间第 2 个节点不得被派发（{} 不应出现）",
        second_pid.display()
    );
}

/// trace 证据：second 的 Ready→Running 发生在 Resume **之后**（暂停窗口内确未派发）。
fn assert_second_dispatched_after_resume(
    reader: &zgraph_trace::TraceReader,
    run_id: &str,
    resumed_at: DateTime<FixedOffset>,
) {
    let transitions = reader.transitions(run_id).expect("transitions");
    let second_running = transitions
        .iter()
        .find(|row| row.node_id == "second" && row.to_status == TraceInstanceStatus::Running)
        .expect("second 的 Running 迁移");
    let at = parse_at(&second_running.at);
    assert!(
        at >= resumed_at - chrono::Duration::milliseconds(50),
        "AC-10: second 应在 Resume 之后派发：running_at={at} resumed_at={resumed_at}"
    );
}

#[tokio::test]
async fn ac07_ac10_pause_is_visible_within_100ms_and_blocks_second_dispatch() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db = dir.path().join("ac10_pause.db");
    let first_pid = dir.path().join("first.pid");
    let second_pid = dir.path().join("second.pid");

    // start(x) -> first -> second -> done；每节点真实 worker 执行 0.5s 并写自己的 pid 文件。
    // 节点睡眠取 0.5s（07 AC-10 用例为 0.3s）：只放大「检测到 worker 已启动 → 发 Pause」
    // 的调度窗口，避免测试线程偶发让出 CPU 时 Pause 晚于 first 完成，语义不变。
    let graph = common::two_step_graph("e2e_fixtures.steps", "first", "second");
    let payload = json!({
        "sleep": 0.5,
        "first_pid_file": first_pid.display().to_string(),
        "second_pid_file": second_pid.display().to_string(),
    });
    let spec = common::spec(graph, common::entry_inputs(&[("start", "x", payload)]));
    let traced = start_traced_run(spec, common::executor(common::RUN_TIMEOUT), &db)
        .expect("start traced run");

    assert_pause_visible_while_worker_running(&traced, &first_pid).await;
    assert_pause_blocks_second_dispatch(&traced, &second_pid).await;

    let resumed_at = Utc::now().fixed_offset();
    assert!(traced.resume(), "resume command accepted");
    let bridge = tokio::time::timeout(Duration::from_secs(10), traced.wait())
        .await
        .expect("run completes after resume")
        .expect("traced run");
    assert_eq!(
        bridge.outcome.status,
        RunStatus::Completed,
        "resume 后应完成: {:?}",
        bridge.outcome.error
    );
    let run_id = bridge.outcome.run_id.clone();
    common::assert_lifecycle_shape(bridge.reader(), &run_id);
    for node in ["first", "second", "done"] {
        common::assert_instance_status(
            bridge.reader(),
            &run_id,
            node,
            TraceInstanceStatus::Completed,
        );
    }
    assert_second_dispatched_after_resume(bridge.reader(), &run_id, resumed_at);

    // 正常完成后 worker 不残留（无僵尸）。
    let second_pid_value = common::read_pid(&second_pid);
    common::assert_pid_gone(second_pid_value, common::PID_GONE_TIMEOUT).await;
}

// ---------------------------------------------------------------------------
// AC-10：取消（真 worker 在飞）+ 幂等
// ---------------------------------------------------------------------------

#[tokio::test]
async fn ac10_cancel_kills_in_flight_worker_and_is_idempotent() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db = dir.path().join("ac10_cancel.db");
    let worker_pid_file = dir.path().join("worker.pid");

    // 单个真实 worker，sleep 30s；测试在它执行中取消。
    let graph = common::linear_graph("e2e_fixtures.steps", "first", None);
    let payload = json!({
        "sleep": 30.0,
        "first_pid_file": worker_pid_file.display().to_string(),
    });
    let spec = common::spec(graph, common::entry_inputs(&[("start", "x", payload)]));
    let traced = start_traced_run(spec, common::executor(common::RUN_TIMEOUT), &db)
        .expect("start traced run");

    common::wait_for_file(&worker_pid_file).await;
    let worker_pid = common::read_pid(&worker_pid_file);

    // 取消（在飞真 worker 会被进程组 kill）+ 重复 Cancel 幂等（后续控制命令 no-op）。
    let cancel_started = Instant::now();
    assert!(traced.cancel(), "cancel command accepted");
    let _ = traced.cancel();
    let _ = traced.pause();
    let _ = traced.resume();
    let bridge = tokio::time::timeout(Duration::from_secs(5), traced.wait())
        .await
        .expect("AC-10: 取消必须 5s 内结束（kill 进程组，不等待 sleep 30s）")
        .expect("traced run");
    let elapsed = cancel_started.elapsed();
    assert!(elapsed < Duration::from_secs(5), "cancel took {elapsed:?}");

    assert_eq!(bridge.outcome.status, RunStatus::Cancelled);
    assert!(
        bridge.outcome.error.is_none(),
        "取消不是失败: {:?}",
        bridge.outcome.error
    );
    assert!(
        bridge.outcome.metrics.killed_workers >= 1,
        "D1: 取消在飞 worker 应计入 killed_workers，实际 {}",
        bridge.outcome.metrics.killed_workers
    );
    assert_eq!(bridge.outcome.metrics.abandoned_tasks, 0);
    assert!(bridge.outcome.output.is_none(), "取消 run 无 end 输出");

    let run_id = bridge.outcome.run_id.clone();
    assert_cancelled_trace(bridge.reader(), &run_id);

    // 进程组 kill：真 worker PID 必须消失（无残留/无僵尸）。
    common::assert_pid_gone(worker_pid, common::PID_GONE_TIMEOUT).await;
}
