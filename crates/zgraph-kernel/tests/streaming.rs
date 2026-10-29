//! Streaming 事件（可丢，I6）：emit 计数/丢弃、trace_enabled=false 不调用 sink。

mod common;

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use serde_json::{json, Value};
use zgraph_kernel::{
    MemoryTraceSink, NodeError, NodeExecution, NodeExecutor, RunStatus, StreamingEvent,
};

/// 每次执行发出 `2 * rounds + 3` 条 streaming 事件。
struct EmitExecutor {
    rounds: usize,
}

impl NodeExecutor for EmitExecutor {
    fn execute<'a>(
        &'a self,
        execution: NodeExecution<'a>,
    ) -> Pin<Box<dyn Future<Output = Result<Value, NodeError>> + Send + 'a>> {
        let rounds = self.rounds;
        Box::pin(async move {
            for index in 0..rounds {
                execution.ctx.emit(StreamingEvent::Progress {
                    percent: u8::try_from(index * 10).unwrap_or(100),
                    message: format!("step {index}"),
                });
                execution.ctx.emit(StreamingEvent::TokenDelta {
                    text: "x".to_string(),
                });
            }
            execution.ctx.emit(StreamingEvent::ToolCallStart {
                call_id: "call-1".to_string(),
                tool_name: "now".to_string(),
                args_summary: json!({}),
            });
            execution.ctx.emit(StreamingEvent::ToolCallEnd {
                call_id: "call-1".to_string(),
                tool_name: "now".to_string(),
                ok: true,
                result_summary: None,
            });
            execution.ctx.emit(StreamingEvent::ReasoningDelta {
                text: "r".to_string(),
            });
            Ok(json!({"emitted": true}))
        })
    }
}

#[tokio::test]
async fn streaming_records_events_and_counts_drops() {
    // 9 条事件、只接受前 2 条 → 记录 2、丢弃 7。
    let sink = Arc::new(MemoryTraceSink::with_streaming_limit(2));
    let executor = Arc::new(EmitExecutor { rounds: 3 });
    let (outcome, sink) = {
        let sink_clone = sink.clone();
        let outcome = common::run(common::linear_spec(), executor, sink_clone).await;
        (outcome, sink)
    };

    assert_eq!(outcome.status, RunStatus::Completed);
    assert_eq!(outcome.metrics.streaming_dropped, 7);
    let calls = sink.streaming_calls();
    assert_eq!(calls.len(), 2, "only accepted events are recorded");
    assert_eq!(calls[0].node_id, "work");
    assert_eq!(calls[0].run_id, outcome.run_id);
    assert!(!calls[0].instance_id.is_empty());
    assert!(calls[0].at.ends_with('Z'));
    assert!(matches!(
        calls[0].event,
        StreamingEvent::Progress { percent: 0, .. }
    ));
    let finished = sink.finished().into_iter().next().expect("finished");
    assert_eq!(finished.metrics.streaming_dropped, 7);
}

#[tokio::test]
async fn streaming_without_limit_records_everything() {
    let sink = Arc::new(MemoryTraceSink::new());
    let (outcome, sink) = {
        let sink_clone = sink.clone();
        let outcome = common::run(
            common::linear_spec(),
            Arc::new(EmitExecutor { rounds: 1 }),
            sink_clone,
        )
        .await;
        (outcome, sink)
    };
    assert_eq!(outcome.status, RunStatus::Completed);
    assert_eq!(sink.streaming_calls().len(), 5);
    assert_eq!(outcome.metrics.streaming_dropped, 0);
}

#[tokio::test]
async fn trace_disabled_never_calls_sink_but_still_builds_outcome() {
    let options = common::options_with(|options| {
        options.trace_enabled = false;
    });
    let mut spec = common::linear_spec();
    spec.options = options;
    let sink = Arc::new(MemoryTraceSink::new());
    let sink_clone = sink.clone();
    let outcome = common::run(spec, Arc::new(EmitExecutor { rounds: 2 }), sink_clone).await;

    assert_eq!(outcome.status, RunStatus::Completed);
    assert_eq!(outcome.output, Some(json!({"r": {"emitted": true}})));
    assert_eq!(outcome.metrics.streaming_dropped, 0);
    assert!(
        sink.is_empty(),
        "trace_enabled=false must not call the sink"
    );
}
