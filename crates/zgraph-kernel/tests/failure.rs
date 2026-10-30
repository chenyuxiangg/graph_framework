//! 冷峻失败（I9）：异常/业务错误 → run Failed、在飞实例 Cancelled、错误码映射。

mod common;

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use zgraph_kernel::{
    FakeExecutor, HarnessErrorKind, InstanceStatus, NodeError, NodeExecution, NodeExecutor,
    RunStatus, WorkerErrorKind,
};

const FAIL_JSON: &str = r#"{
  "version": "1.0",
  "metadata": {"name": "fail"},
  "entry_points": ["start"],
  "nodes": [
    {"id": "start", "type": "start", "inputs": [], "config": {}},
    {"id": "boom", "type": "python_node",
     "inputs": [{"name": "i", "type": "any", "required": true}],
     "config": {"module": "workers.fail", "function": "boom"}},
    {"id": "done", "type": "end",
     "inputs": [{"name": "r", "type": "any", "required": true}], "config": {}}
  ],
  "edges": [
    {"from": "start", "to_node": "boom", "to_input_port": "i"},
    {"from": "boom", "to_node": "done", "to_input_port": "r"}
  ]
}"#;

const COLD_JSON: &str = r#"{
  "version": "1.0",
  "metadata": {"name": "cold"},
  "entry_points": ["start"],
  "nodes": [
    {"id": "start", "type": "start", "inputs": [], "config": {}},
    {"id": "fail", "type": "python_node",
     "inputs": [{"name": "i", "type": "any", "required": true}],
     "config": {"module": "workers.cold", "function": "fail"}},
    {"id": "slow", "type": "python_node",
     "inputs": [{"name": "i", "type": "any", "required": true}],
     "config": {"module": "workers.cold", "function": "slow"}}
  ],
  "edges": [
    {"from": "start", "to_node": "fail", "to_input_port": "i"},
    {"from": "start", "to_node": "slow", "to_input_port": "i"}
  ]
}"#;

#[tokio::test]
async fn business_error_crashes_and_fails_run() {
    let graph = common::compile_text(FAIL_JSON);
    let executor = Arc::new(FakeExecutor::new().fail(
        "boom",
        NodeError::BusinessError {
            code: "BIZ_ORDER_NOT_FOUND".to_string(),
            message: "order missing".to_string(),
        },
    ));
    let (outcome, sink) =
        common::run_with_sink(common::spec(graph, common::entry_inputs(&[])), executor).await;

    assert_eq!(outcome.status, RunStatus::Failed);
    let error = outcome.error.expect("run error");
    assert_eq!(error.code, "NODE_BUSINESS_ERROR");
    assert!(error.message.contains("boom"), "message: {}", error.message);
    assert_eq!(outcome.output, None);
    assert_eq!(
        common::last_status(&sink, "boom"),
        Some(InstanceStatus::Crashed)
    );
    let crashed = common::snapshots_of(&sink, "boom")
        .into_iter()
        .find(|snapshot| snapshot.status == InstanceStatus::Crashed)
        .expect("crashed snapshot");
    assert_eq!(crashed.error_code.as_deref(), Some("NODE_BUSINESS_ERROR"));
    assert!(matches!(
        crashed.error,
        Some(NodeError::BusinessError { .. })
    ));
    common::assert_lifecycle_once(&sink);
    let finished = sink.finished().into_iter().next().expect("finished");
    assert_eq!(finished.status, RunStatus::Failed);
    assert_eq!(
        finished.error.expect("finished error").code,
        "NODE_BUSINESS_ERROR"
    );
    assert_eq!(finished.metrics.abandoned_tasks, 0);
}

#[tokio::test]
async fn python_exception_crashes_with_traceback_payload() {
    let graph = common::compile_text(FAIL_JSON);
    let executor = Arc::new(FakeExecutor::new().fail(
        "boom",
        NodeError::PythonException {
            message: "RuntimeError: boom".to_string(),
            traceback: "Traceback (most recent call last):\n  boom()".to_string(),
        },
    ));
    let (outcome, sink) =
        common::run_with_sink(common::spec(graph, common::entry_inputs(&[])), executor).await;

    assert_eq!(outcome.status, RunStatus::Failed);
    assert_eq!(
        outcome.error.expect("run error").code,
        "NODE_PYTHON_EXCEPTION"
    );
    let crashed = common::snapshots_of(&sink, "boom")
        .into_iter()
        .find(|snapshot| snapshot.status == InstanceStatus::Crashed)
        .expect("crashed snapshot");
    match crashed.error {
        Some(NodeError::PythonException { message, traceback }) => {
            assert!(message.contains("RuntimeError"));
            assert!(traceback.contains("Traceback"));
        }
        other => panic!("unexpected error: {other:?}"),
    }
    // 冷峻：end 从未被创建/执行。
    assert_eq!(common::last_status(&sink, "done"), None);
}

#[tokio::test]
async fn cold_failure_cancels_in_flight_instances() {
    // 两种节点错误都走同一条冷峻路径：实例 Crashed、在飞实例 Cancelled。
    let errors = [
        (
            NodeError::PythonException {
                message: "RuntimeError: boom".to_string(),
                traceback: "Traceback".to_string(),
            },
            "NODE_PYTHON_EXCEPTION",
        ),
        (
            NodeError::BusinessError {
                code: "BIZ_X".to_string(),
                message: "x".to_string(),
            },
            "NODE_BUSINESS_ERROR",
        ),
    ];
    for (error, expected_code) in errors {
        let graph = common::compile_text(COLD_JSON);
        let executor = Arc::new(FakeExecutor::new().fail("fail", error).delay(
            "slow",
            Duration::from_secs(30),
            json!({}),
        ));
        let (outcome, sink) =
            common::run_with_sink(common::spec(graph, common::entry_inputs(&[])), executor).await;

        assert_eq!(outcome.status, RunStatus::Failed);
        assert_eq!(outcome.error.expect("run error").code, expected_code);
        assert_eq!(
            common::last_status(&sink, "slow"),
            Some(InstanceStatus::Cancelled),
            "in-flight instance must be cancelled by cold failure ({expected_code})"
        );
        assert!(
            outcome.metrics.killed_workers >= 1,
            "in-flight execution task counted as killed: {:?}",
            outcome.metrics
        );
        assert_eq!(outcome.metrics.abandoned_tasks, 0);
        // 已 Completed 的实例不被冷峻收尾改写。
        assert_eq!(
            common::last_status(&sink, "start"),
            Some(InstanceStatus::Completed)
        );
        common::assert_lifecycle_once(&sink);
    }
}

#[test]
fn node_error_codes_match_spec() {
    let cases = [
        (
            NodeError::BusinessError {
                code: "BIZ_X".to_string(),
                message: "x".to_string(),
            },
            "NODE_BUSINESS_ERROR",
        ),
        (
            NodeError::PythonException {
                message: "x".to_string(),
                traceback: String::new(),
            },
            "NODE_PYTHON_EXCEPTION",
        ),
        (NodeError::Timeout { elapsed_ms: 1 }, "NODE_TIMEOUT"),
        (NodeError::Cancelled, "RUN_CANCELLED"),
        (
            NodeError::Harness {
                kind: HarnessErrorKind::ConfigLoad,
                detail: json!({}),
            },
            "HARNESS_CONFIG_LOAD",
        ),
        (
            NodeError::Harness {
                kind: HarnessErrorKind::UnknownTool,
                detail: json!({}),
            },
            "HARNESS_UNKNOWN_TOOL",
        ),
        (
            NodeError::Harness {
                kind: HarnessErrorKind::MaxIterations,
                detail: json!({}),
            },
            "HARNESS_MAX_ITERATIONS",
        ),
        (
            NodeError::Harness {
                kind: HarnessErrorKind::LlmError,
                detail: json!({}),
            },
            "HARNESS_LLM_ERROR",
        ),
        (
            NodeError::Harness {
                kind: HarnessErrorKind::LlmTimeout,
                detail: json!({}),
            },
            "HARNESS_LLM_TIMEOUT",
        ),
        (
            NodeError::Harness {
                kind: HarnessErrorKind::ToolError,
                detail: json!({}),
            },
            "TOOL_ERROR",
        ),
        (
            NodeError::Harness {
                kind: HarnessErrorKind::MemoryDegraded,
                detail: json!({}),
            },
            "HARNESS_MEMORY_DEGRADED",
        ),
        (
            NodeError::Harness {
                kind: HarnessErrorKind::ContextOverflow,
                detail: json!({}),
            },
            "HARNESS_CONTEXT_OVERFLOW",
        ),
        (
            NodeError::Config {
                message: "cfg".to_string(),
            },
            "NODE_CONFIG_ERROR",
        ),
        (
            NodeError::Internal {
                message: "bug".to_string(),
            },
            "NODE_INTERNAL",
        ),
        (
            NodeError::NodeWorker {
                kind: WorkerErrorKind::Handshake,
                signal: None,
                exit_code: None,
                stderr_tail: None,
            },
            "NODE_WORKER_HANDSHAKE",
        ),
        (
            NodeError::NodeWorker {
                kind: WorkerErrorKind::Protocol,
                signal: None,
                exit_code: None,
                stderr_tail: None,
            },
            "NODE_WORKER_PROTOCOL",
        ),
        (
            NodeError::NodeWorker {
                kind: WorkerErrorKind::Import,
                signal: None,
                exit_code: None,
                stderr_tail: None,
            },
            "NODE_WORKER_IMPORT",
        ),
        (
            NodeError::NodeWorker {
                kind: WorkerErrorKind::Crash,
                signal: None,
                exit_code: None,
                stderr_tail: None,
            },
            "NODE_WORKER_CRASH",
        ),
    ];
    for (error, expected) in cases {
        assert_eq!(error.code(), expected, "code for {error:?}");
    }
}

/// 执行器直接 panic（模拟 handler 内部 bug）。
struct PanicExecutor;

impl NodeExecutor for PanicExecutor {
    fn execute<'a>(
        &'a self,
        _execution: NodeExecution<'a>,
    ) -> Pin<Box<dyn Future<Output = Result<Value, NodeError>> + Send + 'a>> {
        Box::pin(async move {
            panic!("executor boom");
        })
    }
}

#[tokio::test]
async fn executor_panic_normalizes_to_internal_and_fails_fast() {
    let started = Instant::now();
    let (outcome, sink) =
        common::run_with_sink(common::linear_spec(), Arc::new(PanicExecutor)).await;
    let elapsed = started.elapsed();

    // 关键点：不得等到默认 180s deadline 才判 TimedOut。
    assert!(
        elapsed < Duration::from_secs(5),
        "panic must fail the run immediately (took {elapsed:?})"
    );
    assert_eq!(outcome.status, RunStatus::Failed);
    let error = outcome.error.expect("run error");
    assert_eq!(error.code, "NODE_INTERNAL");
    assert!(
        error.message.contains("executor boom"),
        "panic payload should be surfaced: {}",
        error.message
    );
    let snapshots = common::snapshots_of(&sink, "work");
    let crashed = snapshots
        .iter()
        .find(|snapshot| snapshot.status == InstanceStatus::Crashed)
        .expect("crashed snapshot");
    assert_eq!(crashed.error_code.as_deref(), Some("NODE_INTERNAL"));
    assert!(matches!(crashed.error, Some(NodeError::Internal { .. })));
    assert!(
        sink.timed_out().is_empty(),
        "panic must not be reported as NodeTimedOut"
    );
    common::assert_lifecycle_once(&sink);
}

const AGENT_JSON: &str = r#"{
  "version": "1.0",
  "metadata": {"name": "agent-timeout"},
  "entry_points": ["start"],
  "nodes": [
    {"id": "start", "type": "start", "inputs": [], "config": {}},
    {"id": "agent", "type": "agent_call",
     "inputs": [{"name": "i", "type": "any", "required": true}],
     "config": {"agent_id": "agent-a"}},
    {"id": "done", "type": "end",
     "inputs": [{"name": "r", "type": "any", "required": true}], "config": {}}
  ],
  "edges": [
    {"from": "start", "to_node": "agent", "to_input_port": "i"},
    {"from": "agent", "to_node": "done", "to_input_port": "r"}
  ]
}"#;

#[tokio::test]
async fn agent_handler_timeout_is_timed_out() {
    // 04 §4.1.4 规则 1：仅 AgentCallHandler 允许自报 Timeout → TimedOut + NODE_TIMEOUT。
    let graph = common::compile_text(AGENT_JSON);
    let executor =
        Arc::new(FakeExecutor::new().fail("agent", NodeError::Timeout { elapsed_ms: 12 }));
    let (outcome, sink) =
        common::run_with_sink(common::spec(graph, common::entry_inputs(&[])), executor).await;

    assert_eq!(outcome.status, RunStatus::Failed);
    assert_eq!(outcome.error.expect("run error").code, "NODE_TIMEOUT");
    assert_eq!(
        common::last_status(&sink, "agent"),
        Some(InstanceStatus::TimedOut)
    );
    assert_eq!(sink.timed_out().len(), 1);
}

#[tokio::test]
async fn python_handler_timeout_is_normalized_to_internal() {
    // 04 §4.1.4 规则 1：非 Agent handler 返回 Timeout → warning + Internal → Crashed。
    let executor =
        Arc::new(FakeExecutor::new().fail("work", NodeError::Timeout { elapsed_ms: 12 }));
    let (outcome, sink) = common::run_with_sink(common::linear_spec(), executor).await;

    assert_eq!(outcome.status, RunStatus::Failed);
    let error = outcome.error.expect("run error");
    assert_eq!(error.code, "NODE_INTERNAL");
    assert!(
        error.message.contains("must not produce Timeout"),
        "normalization message: {}",
        error.message
    );
    assert_eq!(
        common::last_status(&sink, "work"),
        Some(InstanceStatus::Crashed)
    );
    assert!(
        sink.timed_out().is_empty(),
        "normalized timeout must not emit NodeTimedOut"
    );
}

#[tokio::test]
async fn cancelled_without_flag_is_normalized_to_internal() {
    // 04 §4.1.4 规则 2：Err(Cancelled) 但 cancel_requested()==false → Internal → Crashed。
    let executor = Arc::new(FakeExecutor::new().fail("work", NodeError::Cancelled));
    let (outcome, sink) = common::run_with_sink(common::linear_spec(), executor).await;

    assert_eq!(outcome.status, RunStatus::Failed);
    let error = outcome.error.expect("run error");
    assert_eq!(error.code, "NODE_INTERNAL");
    assert!(
        error.message.contains("Cancelled without cancel flag"),
        "normalization message: {}",
        error.message
    );
    assert_eq!(
        common::last_status(&sink, "work"),
        Some(InstanceStatus::Crashed)
    );
}

/// 执行器在返回 future **之前**同步 panic（REV-007 残留：原 CatchPanic 只包 future）。
struct SyncPanicExecutor;

impl NodeExecutor for SyncPanicExecutor {
    fn execute<'a>(
        &'a self,
        _execution: NodeExecution<'a>,
    ) -> Pin<Box<dyn Future<Output = Result<Value, NodeError>> + Send + 'a>> {
        panic!("sync executor boom");
    }
}

#[tokio::test]
async fn executor_sync_panic_normalizes_to_internal_and_fails_fast() {
    let started = Instant::now();
    let (outcome, sink) =
        common::run_with_sink(common::linear_spec(), Arc::new(SyncPanicExecutor)).await;
    let elapsed = started.elapsed();

    assert!(
        elapsed < Duration::from_secs(5),
        "sync panic must fail immediately, not at the 180s deadline (took {elapsed:?})"
    );
    assert_eq!(outcome.status, RunStatus::Failed);
    let error = outcome.error.expect("run error");
    assert_eq!(error.code, "NODE_INTERNAL");
    assert!(
        error.message.contains("sync executor boom"),
        "sync panic payload should be surfaced: {}",
        error.message
    );
    let crashed = common::snapshots_of(&sink, "work")
        .into_iter()
        .find(|snapshot| snapshot.status == InstanceStatus::Crashed)
        .expect("crashed snapshot");
    assert_eq!(crashed.error_code.as_deref(), Some("NODE_INTERNAL"));
    assert!(
        sink.timed_out().is_empty(),
        "sync panic must not be reported as NodeTimedOut"
    );
    common::assert_lifecycle_once(&sink);
}
