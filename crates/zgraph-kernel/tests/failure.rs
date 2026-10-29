//! 冷峻失败（I9）：异常/业务错误 → run Failed、在飞实例 Cancelled、错误码映射。

mod common;

use std::sync::Arc;
use std::time::Duration;

use serde_json::json;
use zgraph_kernel::{
    FakeExecutor, HarnessErrorKind, InstanceStatus, NodeError, RunStatus, WorkerErrorKind,
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
            "HARNESS_TOOL_ERROR",
        ),
        (
            NodeError::Harness {
                kind: HarnessErrorKind::MemoryDegraded,
                detail: json!({}),
            },
            "HARNESS_MEMORY_DEGRADED",
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
            },
            "NODE_WORKER_HANDSHAKE",
        ),
        (
            NodeError::NodeWorker {
                kind: WorkerErrorKind::Protocol,
            },
            "NODE_WORKER_PROTOCOL",
        ),
        (
            NodeError::NodeWorker {
                kind: WorkerErrorKind::Import,
            },
            "NODE_WORKER_IMPORT",
        ),
        (
            NodeError::NodeWorker {
                kind: WorkerErrorKind::Crash,
            },
            "NODE_WORKER_CRASH",
        ),
    ];
    for (error, expected) in cases {
        assert_eq!(error.code(), expected, "code for {error:?}");
    }
}
