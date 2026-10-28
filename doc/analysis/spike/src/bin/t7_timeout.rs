//! Probe 7: node-instance timeout / cancellation semantics.
//!
//! Design claim: "when a node instance exceeds its 30s timeout we stop waiting
//! for the task to return". This probe measures what that means in practice:
//!   * `tokio::time::timeout` around a synchronous Python task only abandons
//!     the wait; the Python work keeps running (cannot be preempted);
//!   * `CancellationToken` gives an immediate "decision" but again cannot stop
//!     the Python task;
//!   * abandoned tasks keep consuming CPU/GIL and interfere with other Python
//!     calls, and `Runtime::drop` blocks until blocking tasks finish.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use pyo3::prelude::*;
use pyo3::types::PyModule;
use serde_json::{json, Value};
use tokio::runtime::Builder;
use tokio_util::sync::CancellationToken;
use zgraph_spike::{init_python, py_to_json};

fn call_python_long(func: &str, seconds: f64, done: Arc<AtomicBool>) -> PyResult<Value> {
    Python::attach(|py| {
        let module = PyModule::import(py, "node_long")?;
        let f = module.getattr(func)?;
        let result = f.call1((seconds,))?;
        done.store(true, Ordering::SeqCst);
        py_to_json(&result)
    })
}

fn call_python_quick() -> PyResult<Value> {
    Python::attach(|py| {
        let module = PyModule::import(py, "node_ok")?;
        let func = module.getattr("run")?;
        let inputs = zgraph_spike::json_to_py(
            py,
            &json!({"a": 1, "b": 2.0, "flag": true}),
        )?;
        let result = func.call1((inputs, py.None()))?;
        py_to_json(&result)
    })
}

/// Average latency of a small Python call; used to show an abandoned busy task
/// stealing GIL time from the event loop.
fn quick_call_latency_us(n: usize) -> f64 {
    let started = Instant::now();
    for _ in 0..n {
        call_python_quick().expect("quick python call");
    }
    started.elapsed().as_secs_f64() * 1_000_000.0 / n as f64
}

fn main() {
    init_python().expect("python init");
    println!("python_version={}", Python::version_str());

    let baseline_us = quick_call_latency_us(20);
    println!("RESULT quick_python_call_baseline_us={baseline_us:.1}");

    // ---- 1. tokio::time::timeout around a synchronous Python task ----------
    let rt = Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .unwrap();

    let done = Arc::new(AtomicBool::new(false));
    let (timed_out, decision_ms, done_at_decision) = rt.block_on(async {
        let done_for_task = done.clone();
        let started = Instant::now();
        let handle = tokio::task::spawn_blocking(move || {
            call_python_long("busy", 2.0, done_for_task)
        });
        let result = tokio::time::timeout(Duration::from_millis(100), handle).await;
        let decision_ms = started.elapsed().as_secs_f64() * 1000.0;
        (result.is_err(), decision_ms, done.load(Ordering::SeqCst))
    });
    println!(
        "RESULT timeout_around_spawn_blocking timed_out={timed_out} decision_ms={decision_ms:.1} python_task_done_at_decision={done_at_decision}"
    );

    // Interference: while the abandoned Python busy task is still running, small
    // Python calls from the "event loop" are slowed down by GIL contention.
    let interference_us = quick_call_latency_us(20);
    println!(
        "RESULT abandoned_task_interference baseline_us={baseline_us:.1} during_abandoned_us={interference_us:.1} slowdown_x={:.2}",
        interference_us / baseline_us.max(0.001)
    );

    // Drop the runtime while the abandoned blocking task is still running.
    let drop_started = Instant::now();
    drop(rt);
    let drop_wait_ms = drop_started.elapsed().as_secs_f64() * 1000.0;
    println!(
        "RESULT runtime_drop_waits_for_abandoned_blocking_task drop_wait_ms={drop_wait_ms:.1} python_task_done_after_drop={}",
        done.load(Ordering::SeqCst)
    );
    let after_drop_us = quick_call_latency_us(20);
    println!("RESULT quick_python_call_after_drop_us={after_drop_us:.1}");

    // ---- 2. CancellationToken: immediate decision, task still runs ----------
    let rt = Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .unwrap();
    let done2 = Arc::new(AtomicBool::new(false));
    let (decision_kind, decision_ms2) = rt.block_on(async {
        let done_for_task = done2.clone();
        let handle = tokio::task::spawn_blocking(move || call_python_long("busy", 2.0, done_for_task));
        let token = CancellationToken::new();
        let token2 = token.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(100)).await;
            token2.cancel();
        });
        let started = Instant::now();
        let mut handle = handle;
        let kind = tokio::select! {
            result = &mut handle => {
                format!("task_finished_first ok={}", result.is_ok())
            }
            _ = token.cancelled() => "cancelled_by_token".to_string(),
        };
        (kind, started.elapsed().as_secs_f64() * 1000.0)
    });
    println!(
        "RESULT cancellation_token decision={decision_kind} decision_ms={decision_ms2:.1} python_task_done_at_decision={}",
        done2.load(Ordering::SeqCst)
    );
    std::thread::sleep(Duration::from_millis(300));
    println!(
        "RESULT cancellation_token after_300ms python_task_done={}",
        done2.load(Ordering::SeqCst)
    );
    drop(rt);

    // ---- 3. Control: timeout on a genuinely async future -------------------
    let rt = Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .unwrap();
    let control = rt.block_on(async {
        let started = Instant::now();
        let result = tokio::time::timeout(
            Duration::from_millis(100),
            tokio::time::sleep(Duration::from_secs(2)),
        )
        .await;
        (result.is_err(), started.elapsed().as_secs_f64() * 1000.0)
    });
    println!(
        "RESULT async_future_timeout_control timed_out={} decision_ms={:.1}",
        control.0, control.1
    );
    drop(rt);

    println!("PROBE7_OK=1");
}
