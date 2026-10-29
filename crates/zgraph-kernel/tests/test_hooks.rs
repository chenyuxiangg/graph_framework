//! `test-hooks` 故障注入：disable_settle→RUN_DEADLOCK、force_node_timeout、ticker 间隔（AC-07/AC-11）。
//!
//! 需要 `feature = "test-hooks"`（见 `Cargo.toml` 的 `[[test]] required-features`）。

mod common;

use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::json;
use zgraph_kernel::{
    start_run, FakeExecutor, InstanceStatus, MemoryTraceSink, RunStatus, TestHooks,
};

const GATED_JSON: &str = r#"{
  "version": "1.0",
  "metadata": {"name": "gated-deadlock"},
  "entry_points": ["start"],
  "nodes": [
    {"id": "start", "type": "start", "inputs": [], "config": {}},
    {"id": "a", "type": "python_node",
     "inputs": [{"name": "i", "type": "any", "required": true}],
     "config": {"module": "workers.hooks", "function": "a"}},
    {"id": "b", "type": "python_node",
     "inputs": [{"name": "i", "type": "any", "required": true}],
     "config": {"module": "workers.hooks", "function": "b"}}
  ],
  "edges": [
    {"from": "start", "to_node": "a", "to_input_port": "i"},
    {"from": "a", "to_node": "b", "to_input_port": "i",
     "gate": {"function": "check_output", "params": {"path": "ok", "equals": true}}}
  ]
}"#;

#[tokio::test]
async fn disable_settle_triggers_run_deadlock() {
    let options = common::options_with(|options| {
        options.deadlock_grace_ms = 100;
        options.tick_interval_ms = 10;
        options.with_test_hooks(TestHooks {
            disable_settle: true,
            ..TestHooks::default()
        });
    });
    let mut spec = common::spec(common::compile_text(GATED_JSON), common::entry_inputs(&[]));
    spec.options = options;
    let executor = Arc::new(FakeExecutor::new().respond("a", json!({"ok": false})));

    let started = Instant::now();
    let (outcome, sink) = common::run_with_sink(spec, executor).await;
    let elapsed = started.elapsed();

    assert_eq!(outcome.status, RunStatus::Failed);
    let error = outcome.error.expect("run error");
    assert_eq!(error.code, "RUN_DEADLOCK", "message: {}", error.message);
    assert!(
        elapsed >= Duration::from_millis(90),
        "deadlock must respect deadlock_grace_ms (took {elapsed:?})"
    );
    assert!(
        elapsed < Duration::from_secs(5),
        "deadlock detection must not hang (took {elapsed:?})"
    );
    // 入边永不结算 → b 一直 Pending；failed_run 的冷峻收尾把它标 Cancelled。
    let b_statuses = common::statuses(&sink, "b");
    assert!(
        b_statuses.contains(&InstanceStatus::Pending),
        "unsettled incoming edge keeps b Pending: {b_statuses:?}"
    );
    assert_eq!(
        b_statuses.last(),
        Some(&InstanceStatus::Cancelled),
        "cold failure cancels non-terminal instances"
    );
    common::assert_lifecycle_once(&sink);
}

#[tokio::test]
async fn force_node_timeout_overrides_dsl_timeout() {
    let options = common::options_with(|options| {
        options.tick_interval_ms = 10;
        options.with_test_hooks(TestHooks {
            force_node_timeout_ms: Some(50),
            ..TestHooks::default()
        });
    });
    let mut spec = common::linear_spec();
    spec.options = options;
    let executor = Arc::new(FakeExecutor::new().delay("work", Duration::from_secs(30), json!({})));

    let started = Instant::now();
    let (outcome, sink) = common::run_with_sink(spec, executor).await;
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "forced timeout should fire quickly"
    );
    assert_eq!(outcome.status, RunStatus::Failed);
    assert_eq!(outcome.error.expect("run error").code, "NODE_TIMEOUT");
    assert_eq!(
        common::last_status(&sink, "work"),
        Some(InstanceStatus::TimedOut)
    );
    let timed_out = sink.timed_out();
    assert_eq!(timed_out.len(), 1);
    assert!(timed_out[0].elapsed_ms >= 40);
}

#[tokio::test]
async fn ticker_interval_stays_small_while_executor_runs() {
    let (probe_tx, mut probe_rx) = tokio::sync::mpsc::unbounded_channel();
    let options = common::options_with(|options| {
        options.tick_interval_ms = 5;
        options.with_test_hooks(TestHooks {
            tick_probe: Some(probe_tx),
            ..TestHooks::default()
        });
    });
    let mut spec = common::chain_spec();
    spec.options = options;
    let executor = Arc::new(
        FakeExecutor::new()
            .delay("a", Duration::from_millis(300), json!({"a": 1}))
            .respond("b", json!({"b": 1})),
    );
    let sink = Arc::new(MemoryTraceSink::new());
    let handle = start_run(None, spec, executor, sink.clone()).expect("started");
    let mut status_rx = handle.subscribe_status();

    // 执行器运行期间 Pause 必须被事件循环及时处理（AC-07）。
    common::wait_for_running(&sink, "a").await;
    let pause_started = Instant::now();
    assert!(handle.pause());
    common::wait_for_run_status(&mut status_rx, RunStatus::Paused).await;
    let pause_latency = pause_started.elapsed();
    assert!(
        pause_latency < Duration::from_millis(500),
        "pause must be handled promptly (took {pause_latency:?})"
    );
    assert!(handle.resume());
    let outcome = handle.wait().await;
    assert_eq!(outcome.status, RunStatus::Completed);

    // 收集 ticker 时刻，断言最大间隔 < 50ms（AC-07）。
    let mut ticks = Vec::new();
    while let Ok(instant) = probe_rx.try_recv() {
        ticks.push(instant);
    }
    assert!(
        ticks.len() >= 10,
        "expected watchdog ticks during a 300ms execution, got {}",
        ticks.len()
    );
    let mut max_gap = Duration::ZERO;
    for pair in ticks.windows(2) {
        let gap = pair[1].saturating_duration_since(pair[0]);
        if gap > max_gap {
            max_gap = gap;
        }
    }
    assert!(
        max_gap < Duration::from_millis(50),
        "event loop ticker stalled: max gap {max_gap:?}"
    );
}
