//! M3 `WorkerExecutor` 集成测试：经真实内核 run 循环验证错误映射、超时/取消的进程组
//! kill、emit 流式入库、ctx 载荷透传与全局/每 run 并发上限。
//!
//! 数据通路：内联 DSL 图 → `CompiledGraph::compile` → kernel `run_*` → `WorkerExecutor`
//! → `python -m zgraph_worker.runtime` 子进程。断言都基于真实 PID 与真实时序（禁止 mock）。

mod common;

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::json;
use zgraph_kernel::{
    start_run, InstanceStatus, MemoryTraceSink, NodeError, RunControl, RunOutcome, RunStatus,
    StreamingEvent,
};
use zgraph_worker::WorkerExecutor;

const RUN_TIMEOUT: Duration = Duration::from_secs(30);

fn assert_failed_with(outcome: &RunOutcome, code: &str) {
    assert_eq!(outcome.status, RunStatus::Failed, "run should fail");
    let error = outcome.error.as_ref().expect("failed run carries error");
    assert_eq!(error.code, code, "unexpected run error: {error:?}");
}

fn last_node_error(sink: &MemoryTraceSink) -> Option<NodeError> {
    sink.snapshots()
        .into_iter()
        .rev()
        .find_map(|snapshot| snapshot.error)
}

async fn run_fault(function: &str) -> (RunOutcome, Arc<MemoryTraceSink>) {
    let graph = common::linear_graph("wfaults", function, None);
    let spec = common::linear_spec(graph, json!({}));
    common::run_graph(spec, common::executor(RUN_TIMEOUT)).await
}

#[tokio::test]
async fn python_exception_maps_to_python_exception() {
    let (outcome, sink) = run_fault("value_error").await;
    assert_failed_with(&outcome, "NODE_PYTHON_EXCEPTION");
    match last_node_error(&sink) {
        Some(NodeError::PythonException { message, traceback }) => {
            assert!(message.contains("boom"), "message: {message}");
            assert!(traceback.contains("ValueError"), "traceback: {traceback}");
        }
        other => panic!("expected PythonException, got {other:?}"),
    }
}

#[tokio::test]
async fn sdk_business_error_maps_to_business_error() {
    let (outcome, sink) = run_fault("business_error").await;
    assert_failed_with(&outcome, "NODE_BUSINESS_ERROR");
    match last_node_error(&sink) {
        Some(NodeError::BusinessError { code, message }) => {
            assert_eq!(code, "NODE_BUSINESS_ERROR");
            assert!(message.contains("order not found"), "message: {message}");
        }
        other => panic!("expected BusinessError, got {other:?}"),
    }
}

#[tokio::test]
async fn custom_business_code_is_preserved_in_node_error() {
    let (outcome, sink) = run_fault("biz_code").await;
    // 对外错误码是 NODE_BUSINESS_ERROR，业务码保留在 NodeError 载荷里（04 §4.3.4）。
    assert_failed_with(&outcome, "NODE_BUSINESS_ERROR");
    match last_node_error(&sink) {
        Some(NodeError::BusinessError { code, message }) => {
            assert_eq!(code, "BIZ_ORDER_NOT_FOUND");
            assert!(
                message.contains("custom business code"),
                "message: {message}"
            );
        }
        other => panic!("expected BusinessError with custom code, got {other:?}"),
    }
}

#[tokio::test]
async fn import_failure_maps_to_worker_import() {
    let graph = common::linear_graph("no_such_module_xyz", "run", None);
    let spec = common::linear_spec(graph, json!({}));
    let (outcome, _sink) = common::run_graph(spec, common::executor(RUN_TIMEOUT)).await;
    assert_failed_with(&outcome, "NODE_WORKER_IMPORT");
}

#[tokio::test]
async fn exit_without_result_maps_to_worker_protocol() {
    let (outcome, _sink) = run_fault("exit_zero").await;
    assert_failed_with(&outcome, "NODE_WORKER_PROTOCOL");
}

#[tokio::test]
async fn signal_exit_maps_to_worker_crash() {
    let (outcome, _sink) = run_fault("kill_self").await;
    assert_failed_with(&outcome, "NODE_WORKER_CRASH");
}

#[tokio::test]
async fn emit_frames_are_forwarded_to_kernel_streaming_sink() {
    let graph = common::linear_graph("wemit", "run", None);
    let spec = common::linear_spec(graph, json!({}));
    let (outcome, sink) = common::run_graph(spec, common::executor(RUN_TIMEOUT)).await;
    assert_eq!(outcome.status, RunStatus::Completed);
    let output = outcome.output.as_ref().expect("run output");
    assert_eq!(output["r"]["emit_count"], 5);

    let events: Vec<StreamingEvent> = sink
        .streaming_calls()
        .into_iter()
        .map(|call| call.event)
        .collect();
    assert_eq!(events.len(), 5, "all emit frames should reach the sink");
    assert!(matches!(events[0], StreamingEvent::TokenDelta { .. }));
    assert!(matches!(events[1], StreamingEvent::ReasoningDelta { .. }));
    assert!(matches!(events[2], StreamingEvent::ToolCallStart { .. }));
    assert!(matches!(events[3], StreamingEvent::ToolCallEnd { .. }));
    assert!(matches!(events[4], StreamingEvent::Progress { .. }));
}

#[tokio::test]
async fn run_payload_carries_seed_user_global_vars_and_deadline() {
    let graph = common::linear_graph("wfaults", "context_probe", None);
    let mut spec = common::linear_spec(graph, json!({}));
    spec.global_vars
        .insert("greeting".to_string(), json!("hello-seed"));
    let (outcome, _sink) = common::run_graph(spec, common::executor(RUN_TIMEOUT)).await;
    assert_eq!(outcome.status, RunStatus::Completed, "{:?}", outcome.error);
    let probe = &outcome.output.as_ref().expect("run output")["r"];
    assert_eq!(
        probe["seed"], 7,
        "RunOptions.seed must reach the worker (D5)"
    );
    assert_eq!(probe["user_id"], "u-worker");
    assert_eq!(probe["greeting"], "hello-seed");
    assert_eq!(probe["has_deadline"], true);
    assert_eq!(probe["node_id"], "work");
}

#[tokio::test]
async fn kernel_timeout_kills_worker_process_group_including_grandchild() {
    let paths = common::PidPaths::new();
    let graph = common::linear_graph("wfaults", "sleep_forever", Some(500));
    let input = json!({
        "pid_file": paths.worker.display().to_string(),
        "child_pid_file": paths.child.display().to_string()
    });
    let spec = common::linear_spec(graph, input);
    let started = Instant::now();
    let (outcome, sink) = common::run_graph(spec, common::executor(RUN_TIMEOUT)).await;
    let elapsed = started.elapsed();

    assert_failed_with(&outcome, "NODE_TIMEOUT");
    assert!(
        elapsed < Duration::from_secs(5),
        "D1：超时必须在 5s 内失败并 kill，实际 {elapsed:?}"
    );
    assert_eq!(outcome.metrics.killed_workers, 1);
    let statuses: Vec<InstanceStatus> = sink
        .snapshots()
        .into_iter()
        .filter(|snapshot| snapshot.node_id == "work")
        .map(|snapshot| snapshot.status)
        .collect();
    assert!(
        statuses.contains(&InstanceStatus::TimedOut),
        "work instance should reach TimedOut, got {statuses:?}"
    );

    let worker_pid = common::read_pid(&paths.worker);
    let child_pid = common::read_pid(&paths.child);
    common::assert_pid_gone(&worker_pid, common::PID_GONE_TIMEOUT).await;
    common::assert_pid_gone(&child_pid, common::PID_GONE_TIMEOUT).await;
}

#[tokio::test]
async fn kernel_cancel_kills_worker_process_group() {
    let paths = common::PidPaths::new();
    let graph = common::linear_graph("wfaults", "sleep_forever", None);
    let input = json!({
        "pid_file": paths.worker.display().to_string(),
        "child_pid_file": paths.child.display().to_string()
    });
    let spec = common::linear_spec(graph, input);
    let sink = Arc::new(MemoryTraceSink::new());
    let handle = start_run(None, spec, common::executor(RUN_TIMEOUT), sink.clone())
        .expect("run should start");
    wait_for_node_running(&sink, "work").await;
    // 等 fixture 真正开始执行（自身与孙进程的 pid 都落盘）再取消，
    // 否则可能在写 pid 前就被 abort，后续「孙进程无残留」断言失去意义。
    wait_for_file(&paths.worker).await;
    wait_for_file(&paths.child).await;
    assert!(
        handle.control(RunControl::Cancel),
        "cancel command accepted"
    );
    let outcome = handle.wait().await;
    assert_eq!(outcome.status, RunStatus::Cancelled);
    assert!(outcome.metrics.killed_workers >= 1);

    let worker_pid = common::read_pid(&paths.worker);
    let child_pid = common::read_pid(&paths.child);
    common::assert_pid_gone(&worker_pid, common::PID_GONE_TIMEOUT).await;
    common::assert_pid_gone(&child_pid, common::PID_GONE_TIMEOUT).await;
}

#[tokio::test]
async fn crashed_worker_only_affects_its_own_instance() {
    let graph = common::parallel_graph("wfaults", "business_error", "ok");
    let spec = common::linear_spec(graph, json!({}));
    let (outcome, sink) = common::run_graph(spec, common::executor(RUN_TIMEOUT)).await;
    // 冷峻语义：run 失败；但 b 分支的子进程独立完成（进程隔离，崩溃不扩散）。
    assert_failed_with(&outcome, "NODE_BUSINESS_ERROR");
    let status = |node: &str| {
        sink.snapshots()
            .into_iter()
            .rev()
            .find(|snapshot| snapshot.node_id == node)
            .map(|snapshot| snapshot.status)
    };
    assert_eq!(status("a"), Some(InstanceStatus::Crashed));
    assert_eq!(status("b"), Some(InstanceStatus::Completed));
}

#[tokio::test]
async fn per_run_limit_serializes_parallel_branches() {
    let intervals = run_parallel(Duration::from_secs(30), 4, 1).await;
    assert_eq!(intervals.len(), 2, "both branches should record intervals");
    assert!(
        !overlaps(&intervals[0], &intervals[1]),
        "max_workers_per_run=1 must serialize branches: {intervals:?}"
    );
}

#[tokio::test]
async fn global_limit_serializes_parallel_branches() {
    let intervals = run_parallel(Duration::from_secs(30), 1, 8).await;
    assert_eq!(intervals.len(), 2);
    assert!(
        !overlaps(&intervals[0], &intervals[1]),
        "max_workers=1 must serialize branches: {intervals:?}"
    );
}

#[tokio::test]
async fn branches_run_concurrently_within_limits() {
    let intervals = run_parallel(Duration::from_secs(30), 2, 4).await;
    assert_eq!(intervals.len(), 2);
    assert!(
        overlaps(&intervals[0], &intervals[1]),
        "with limits >= 2 the branches should overlap: {intervals:?}"
    );
}

async fn run_parallel(
    timeout: Duration,
    max_workers: usize,
    max_workers_per_run: usize,
) -> Vec<(f64, f64)> {
    let paths = common::PidPaths::new();
    let graph = common::parallel_graph("wparallel", "a", "b");
    let input = json!({"log_file": paths.log.display().to_string(), "sleep": 0.5});
    let spec = common::linear_spec(graph, input);
    let executor = Arc::new(WorkerExecutor::with_limits(
        common::supervisor_config(timeout),
        max_workers,
        max_workers_per_run,
    ));
    let (outcome, _sink) = common::run_graph(spec, executor).await;
    assert_eq!(outcome.status, RunStatus::Completed, "{:?}", outcome.error);
    read_intervals(&paths.log)
}

/// 轮询等待 fixture 写出 pid 文件（最多 5s）。
async fn wait_for_file(path: &Path) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if path.exists() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("{} not created in time", path.display());
}

async fn wait_for_node_running(sink: &MemoryTraceSink, node_id: &str) {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let running = sink.snapshots().iter().any(|snapshot| {
            snapshot.node_id == node_id && snapshot.status == InstanceStatus::Running
        });
        if running {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "node {node_id} never reached Running"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

/// 读并发日志为 `(start, end)` 区间（秒，wall clock 同主机可比）。
fn read_intervals(path: &Path) -> Vec<(f64, f64)> {
    let text = std::fs::read_to_string(path).expect("interval log readable");
    let mut starts: HashMap<String, f64> = HashMap::new();
    let mut intervals = Vec::new();
    for line in text.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() != 4 {
            continue;
        }
        let timestamp: f64 = parts[2].parse().expect("interval timestamp");
        match parts[3] {
            "start" => {
                starts.insert(parts[0].to_string(), timestamp);
            }
            "end" => {
                if let Some(started) = starts.remove(parts[0]) {
                    intervals.push((started, timestamp));
                }
            }
            _ => {}
        }
    }
    intervals
}

fn overlaps(first: &(f64, f64), second: &(f64, f64)) -> bool {
    first.0 < second.1 && second.0 < first.1
}
