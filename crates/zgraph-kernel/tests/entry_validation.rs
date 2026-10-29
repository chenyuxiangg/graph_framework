//! 启动校验（03 §3.2 步骤 1–2）：失败返回 RUN_ENTRY_INVALID/RUN_ENTRY_INPUT_MISSING，
//! **不产生任何 sink 事件**。

mod common;

use std::collections::HashMap;
use std::sync::Arc;

use serde_json::json;
use zgraph_kernel::{start_run, FakeExecutor, MemoryTraceSink, RunError, RunSpec};

fn start_expect_err(spec: RunSpec, sink: &Arc<MemoryTraceSink>) -> RunError {
    let result = start_run(
        None,
        spec,
        Arc::new(FakeExecutor::new()),
        sink.clone() as Arc<dyn zgraph_kernel::TraceSink>,
    );
    match result {
        Ok(_) => panic!("start should be rejected"),
        Err(error) => RunError::new(error.code, error.message),
    }
}

#[tokio::test]
async fn missing_required_entry_input_is_rejected_without_trace() {
    let graph = common::compile_fixture("customer_ticket.json");
    let sink = Arc::new(MemoryTraceSink::new());
    let error = start_expect_err(common::spec(graph, HashMap::new()), &sink);
    assert_eq!(error.code, "RUN_ENTRY_INPUT_MISSING");
    assert!(
        error.message.contains("user_message"),
        "message: {}",
        error.message
    );
    assert!(
        sink.is_empty(),
        "no run/events may be produced on start failure"
    );
}

#[tokio::test]
async fn entry_inputs_with_non_entry_node_is_rejected() {
    let graph = common::compile_fixture("customer_ticket.json");
    let sink = Arc::new(MemoryTraceSink::new());
    let spec = common::spec(
        graph,
        common::entry_inputs(&[("kb_search", "query", json!("hi"))]),
    );
    let error = start_expect_err(spec, &sink);
    assert_eq!(error.code, "RUN_ENTRY_INVALID");
    assert!(
        error.message.contains("non-entry"),
        "message: {}",
        error.message
    );
    assert!(sink.is_empty());
}

#[tokio::test]
async fn entry_inputs_with_unknown_node_is_rejected() {
    let graph = common::compile_fixture("customer_ticket.json");
    let sink = Arc::new(MemoryTraceSink::new());
    let spec = common::spec(graph, common::entry_inputs(&[("ghost", "x", json!(1))]));
    let error = start_expect_err(spec, &sink);
    assert_eq!(error.code, "RUN_ENTRY_INVALID");
    assert!(
        error.message.contains("unknown node"),
        "message: {}",
        error.message
    );
    assert!(sink.is_empty());
}

#[tokio::test]
async fn entry_inputs_with_undeclared_port_is_rejected() {
    let graph = common::compile_fixture("customer_ticket.json");
    let sink = Arc::new(MemoryTraceSink::new());
    let spec = common::spec(
        graph,
        common::entry_inputs(&[
            ("input_parser", "user_message", json!("hi")),
            ("input_parser", "bogus", json!(1)),
        ]),
    );
    let error = start_expect_err(spec, &sink);
    assert_eq!(error.code, "RUN_ENTRY_INVALID");
    assert!(
        error.message.contains("undeclared port"),
        "message: {}",
        error.message
    );
    assert!(sink.is_empty());
}

#[tokio::test]
async fn linear_entry_inputs_missing_required_port_is_rejected() {
    let graph = common::compile_text(common::LINEAR_JSON);
    let sink = Arc::new(MemoryTraceSink::new());
    let error = start_expect_err(common::spec(graph, HashMap::new()), &sink);
    assert_eq!(error.code, "RUN_ENTRY_INPUT_MISSING");
    assert!(
        error.message.contains("start"),
        "message: {}",
        error.message
    );
    assert!(sink.is_empty());
}

#[tokio::test]
async fn valid_start_produces_events_and_outcome() {
    // 反证：通过校验后才会有 RunStarted / RunFinished。
    let graph = common::compile_fixture("customer_ticket.json");
    let sink = Arc::new(MemoryTraceSink::new());
    let spec = common::spec(
        graph,
        common::entry_inputs(&[("input_parser", "user_message", json!("退款"))]),
    );
    let executor = Arc::new(FakeExecutor::new().respond("input_parser", json!({"ok": true})));
    let outcome = common::run(spec, executor, sink.clone()).await;
    // 该图有 agent/python 节点，FakeExecutor 全部回 echo，run 应 Completed。
    assert_eq!(outcome.status, zgraph_kernel::RunStatus::Completed);
    common::assert_lifecycle_once(&sink);
}
