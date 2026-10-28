//! Probe 3: Python -> Rust callback (`ctx.emit(dict)`).
//!
//! Verifies:
//!  * a `#[pyclass]` ctx holding a bounded tokio mpsc sender can be called from
//!    Python synchronously without a runtime present;
//!  * `try_send` semantics when the channel is full (streaming events dropped
//!    per design) - counts, no panic, no deadlock;
//!  * re-entering the Python interpreter from inside the callback
//!    (`Python::attach`, `json.dumps`) is safe and does not deadlock;
//!  * callback overhead for 10k emits (is a sync callback viable on the hot
//!    path?).

use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Instant;

use pyo3::prelude::*;
use pyo3::types::PyModule;
use serde_json::{json, Value};
use zgraph_spike::{init_python, json_to_py, py_to_json, EmitCtx, EmitStats};

struct EmitCase {
    label: &'static str,
    count: usize,
    capacity: usize,
    reenter_python: bool,
}

fn run_emit_case(case: &EmitCase) -> Value {
    let (tx, mut rx) = tokio::sync::mpsc::channel::<Value>(case.capacity);
    let stats = Arc::new(EmitStats::default());
    let inputs = json!({ "count": case.count });

    let started = Instant::now();
    let python_result = Python::attach(|py| -> PyResult<Value> {
        let module = PyModule::import(py, "node_emit")?;
        let func = module.getattr("run")?;
        let ctx = Py::new(py, EmitCtx::new(tx, stats.clone(), case.reenter_python))?;
        let result = func.call1((json_to_py(py, &inputs)?, ctx))?;
        py_to_json(&result)
    });
    let wall_ms = started.elapsed().as_secs_f64() * 1000.0;

    let mut received: Vec<Value> = Vec::new();
    while let Ok(event) = rx.try_recv() {
        received.push(event);
    }

    json!({
        "case": case.label,
        "count": case.count,
        "capacity": case.capacity,
        "reenter_python": case.reenter_python,
        "call_ok": python_result.is_ok(),
        "call_error": python_result.as_ref().err().map(|e| e.to_string()),
        "call_wall_ms": wall_ms,
        "sent": stats.sent.load(Ordering::SeqCst),
        "dropped_full": stats.dropped_full.load(Ordering::SeqCst),
        "closed": stats.closed.load(Ordering::SeqCst),
        "reentries": stats.reentries.load(Ordering::SeqCst),
        "drained": received.len(),
        "first_event": received.first(),
        "last_event": received.last(),
        "python_return": python_result.ok(),
    })
}

fn print_case(result: &Value) {
    let summary = json!({
        "case": result["case"],
        "count": result["count"],
        "capacity": result["capacity"],
        "reenter_python": result["reenter_python"],
        "call_ok": result["call_ok"],
        "call_wall_ms": result["call_wall_ms"],
        "sent": result["sent"],
        "dropped_full": result["dropped_full"],
        "drained": result["drained"],
        "reentries": result["reentries"],
        "call_error": result["call_error"],
    });
    println!("RESULT case={}", serde_json::to_string(&summary).unwrap());
    if result["python_return"]["stats_seen_by_python"].is_object() {
        println!(
            "RESULT python_view_of_stats case={} stats={}",
            result["case"],
            result["python_return"]["stats_seen_by_python"]
        );
    }
    if let Some(first) = result["first_event"].as_object() {
        println!("RESULT first_event case={} event={first:?}", result["case"]);
        let _ = first;
    }
}

fn main() {
    init_python().expect("python init");
    println!("python_version={}", Python::version_str());

    // 1. Happy path: 5 events, ample capacity, re-entrant Python calls.
    let case = EmitCase {
        label: "5_events_cap64_reentrant",
        count: 5,
        capacity: 64,
        reenter_python: true,
    };
    let result = run_emit_case(&case);
    print_case(&result);
    let happy_ok = result["call_ok"].as_bool() == Some(true)
        && result["sent"].as_u64() == Some(5)
        && result["drained"].as_u64() == Some(5)
        && result["reentries"].as_u64() == Some(5);
    println!("CALLBACK_HAPPY_PATH_OK={happy_ok}");
    if let Some(first) = result["first_event"].as_object() {
        println!("FIRST_EVENT={}", serde_json::to_string(first).unwrap());
    }

    // 2. Full channel: capacity 2, 100 emits. Design says drop streaming events.
    let case = EmitCase {
        label: "100_events_cap2_no_consumer",
        count: 100,
        capacity: 2,
        reenter_python: false,
    };
    let result = run_emit_case(&case);
    print_case(&result);
    let drop_ok = result["call_ok"].as_bool() == Some(true)
        && result["sent"].as_u64() == Some(2)
        && result["dropped_full"].as_u64() == Some(98)
        && result["drained"].as_u64() == Some(2);
    println!("CALLBACK_DROP_ON_FULL_OK={drop_ok}");

    // 3. Full channel while re-entering Python: would detect a deadlock by hanging.
    let case = EmitCase {
        label: "100_events_cap1_reentrant",
        count: 100,
        capacity: 1,
        reenter_python: true,
    };
    let started = Instant::now();
    let result = run_emit_case(&case);
    read_line_marker("CALLBACK_REENTRANT_FULL_CHANNEL_NO_DEADLOCK", &result, started);

    // 4. Overhead: 10k sync callbacks with enough capacity to not drop.
    let case = EmitCase {
        label: "10000_events_cap20000_overhead",
        count: 10_000,
        capacity: 20_000,
        reenter_python: false,
    };
    let result = run_emit_case(&case);
    print_case(&result);
    let per_emit_us = result["call_wall_ms"].as_f64().unwrap_or(0.0) * 1000.0 / 10_000.0;
    println!(
        "EMIT_CALLBACK_PER_CALL_US={per_emit_us:.3} sent={} drained={} dropped={}",
        result["sent"], result["drained"], result["dropped_full"]
    );
    println!("PROBE3_OK=1");
}

fn read_line_marker(name: &str, result: &Value, started: Instant) {
    print_case(result);
    println!(
        "{name}=true wall_ms={:.1} sent={} dropped={} reentries={}",
        started.elapsed().as_secs_f64() * 1000.0,
        result["sent"],
        result["dropped_full"],
        result["reentries"]
    );
}
