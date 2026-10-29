//! 超时（D1：kill 语义）：TimedOut → run Failed、NodeTimedOut 事件、
//! `abandoned_tasks` 恒 0、`killed_workers` 计数、`NodeAbandoned` 不产生。

mod common;

use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::json;
use zgraph_kernel::{FakeExecutor, InstanceStatus, RunStatus};

#[tokio::test]
async fn node_timeout_kills_and_fails_run_quickly() {
    let options = common::options_with(|options| {
        options.tick_interval_ms = 10;
        options.deadlock_grace_ms = 5_000;
    });
    let spec = {
        let mut spec = common::linear_spec_with_work_timeout(100);
        spec.options = options;
        spec
    };
    let executor = Arc::new(FakeExecutor::new().delay(
        "work",
        Duration::from_secs(30),
        json!({"never": true}),
    ));

    let started = Instant::now();
    let (outcome, sink) = common::run_with_sink(spec, executor).await;
    let elapsed = started.elapsed();

    assert!(
        elapsed < Duration::from_secs(5),
        "run must not wait for the 30s executor (took {elapsed:?})"
    );
    assert_eq!(outcome.status, RunStatus::Failed);
    assert_eq!(outcome.error.expect("run error").code, "NODE_TIMEOUT");
    assert_eq!(
        common::last_status(&sink, "work"),
        Some(InstanceStatus::TimedOut)
    );
    let timed_out = sink.timed_out();
    assert_eq!(timed_out.len(), 1, "NodeTimedOut event exactly once");
    assert!(
        timed_out[0].elapsed_ms >= 90,
        "elapsed: {}",
        timed_out[0].elapsed_ms
    );
    assert!(timed_out[0].at.ends_with('Z'));
    assert_eq!(timed_out[0].run_id, outcome.run_id);

    // D1：不产生 NodeAbandoned、abandoned_tasks 恒 0、killed_workers 计数。
    assert!(
        sink.abandoned().is_empty(),
        "NodeAbandoned must not be produced"
    );
    assert_eq!(outcome.metrics.abandoned_tasks, 0);
    assert!(
        outcome.metrics.killed_workers >= 1,
        "timeout execution task counted as killed: {:?}",
        outcome.metrics
    );
    common::assert_lifecycle_once(&sink);
    let finished = sink.finished().into_iter().next().expect("finished");
    assert_eq!(finished.status, RunStatus::Failed);
    assert_eq!(
        finished.metrics.killed_workers,
        outcome.metrics.killed_workers
    );
    // end 未执行。
    assert_eq!(common::last_status(&sink, "done"), None);
}
