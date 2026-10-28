//! Probe 7b: CAN a runaway Python node actually be interrupted?
//!
//! `tokio::time::timeout` only abandons the wait, so hard cancellation needs a
//! mechanism that reaches into CPython. Three candidates are measured in
//! separate process invocations (`main`, `worker`, `async_exc`):
//!
//!   * `main`      - `PyErr_SetInterrupt()` while the Python code runs on the
//!                   MAIN thread: documented to raise KeyboardInterrupt.
//!   * `worker`    - `PyErr_SetInterrupt()` while the Python code runs on a
//!                   worker OS thread (what `spawn_blocking` really is).
//!   * `async_exc` - `PyThreadState_SetAsyncExc(thread_id, KeyboardInterrupt)`
//!                   against the worker thread: the low-level escape hatch.
//!
//! Run with: `./target/debug/t7b_interrupt <mode>`

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use pyo3::prelude::*;
use pyo3::types::PyModule;
use serde_json::{json, Value};
use zgraph_spike::{init_python, json_to_py, py_to_json};

const BUSY_SECONDS: f64 = 5.0;

fn quick() -> PyResult<Value> {
    Python::attach(|py| {
        let module = PyModule::import(py, "node_ok")?;
        let func = module.getattr("run")?;
        let inputs = json_to_py(py, &json!({"a": 1, "b": 2.0, "flag": true}))?;
        py_to_json(&func.call1((inputs, py.None()))?)
    })
}

fn sys_exc_info_cleared() -> bool {
    Python::attach(|py| -> PyResult<bool> {
        let sys = py.import("sys")?;
        Ok(sys.getattr("exc_info")?.call0()?.get_item(0)?.is_none())
    })
    .unwrap_or(false)
}

fn run_busy_on_current_thread() -> PyResult<Value> {
    Python::attach(|py| -> PyResult<Value> {
        let module = PyModule::import(py, "node_long")?;
        let func = module.getattr("busy")?;
        py_to_json(&func.call1((BUSY_SECONDS,))?)
    })
}

fn mode_main_interrupt() {
    println!("MODE=main_interrupt target=main_thread");
    let helper = std::thread::spawn(|| {
        std::thread::sleep(Duration::from_millis(300));
        unsafe { pyo3::ffi::PyErr_SetInterrupt() };
    });
    let started = Instant::now();
    let result = run_busy_on_current_thread();
    let elapsed_ms = started.elapsed().as_millis();
    helper.join().unwrap();
    let interrupted = result
        .as_ref()
        .err()
        .map(|e| e.to_string().contains("KeyboardInterrupt"))
        .unwrap_or(false);
    println!(
        "RESULT mode=main_interrupt elapsed_ms={elapsed_ms} interrupted={interrupted} result={:?}",
        result.map_err(|e| e.to_string())
    );
    report_after("main_interrupt");
}

fn mode_worker_interrupt() {
    println!("MODE=worker_interrupt target=worker_thread");
    let started_flag = Arc::new(AtomicBool::new(false));
    let flag = started_flag.clone();
    let handle = std::thread::spawn(move || {
        let started = Instant::now();
        let result = Python::attach(|py| -> PyResult<Value> {
            let module = PyModule::import(py, "node_long")?;
            let func = module.getattr("busy")?;
            flag.store(true, Ordering::SeqCst);
            py_to_json(&func.call1((BUSY_SECONDS,))?)
        });
        (started.elapsed().as_millis(), result.map_err(|e| e.to_string()))
    });
    for _ in 0..200 {
        if started_flag.load(Ordering::SeqCst) {
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    std::thread::sleep(Duration::from_millis(300));
    let interrupt_at = Instant::now();
    unsafe { pyo3::ffi::PyErr_SetInterrupt() };
    let (loop_elapsed_ms, result) = handle.join().unwrap();
    let interrupted = result
        .as_ref()
        .err()
        .map(|e| e.contains("KeyboardInterrupt"))
        .unwrap_or(false);
    println!(
        "RESULT mode=worker_interrupt loop_elapsed_ms={loop_elapsed_ms} stop_latency_ms={} interrupted={interrupted} result={result:?}",
        interrupt_at.elapsed().as_millis()
    );
    report_after("worker_interrupt");
}

fn mode_async_exc(target: &'static str) {
    println!("MODE=async_exc target=worker_thread via PyThreadState_SetAsyncExc work={target}");
    let thread_id = Arc::new(AtomicU64::new(0));
    let thread_id_out = thread_id.clone();
    let handle = std::thread::spawn(move || {
        let started = Instant::now();
        let result = Python::attach(|py| -> PyResult<Value> {
            let id = py
                .import("threading")?
                .getattr("get_ident")?
                .call0()?
                .extract::<u64>()?;
            thread_id_out.store(id, Ordering::SeqCst);
            let module = PyModule::import(py, "node_long")?;
            let func = module.getattr(target)?;
            py_to_json(&func.call1((BUSY_SECONDS,))?)
        });
        (started.elapsed().as_millis(), result.map_err(|e| e.to_string()))
    });
    for _ in 0..200 {
        if thread_id.load(Ordering::SeqCst) != 0 {
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    let id = thread_id.load(Ordering::SeqCst);
    std::thread::sleep(Duration::from_millis(300));
    let interrupt_at = Instant::now();
    let set_result = Python::attach(|_py| unsafe {
        pyo3::ffi::PyThreadState_SetAsyncExc(
            id as std::os::raw::c_long,
            pyo3::ffi::PyExc_KeyboardInterrupt,
        )
    });
    let (loop_elapsed_ms, result) = handle.join().unwrap();
    let interrupted = result
        .as_ref()
        .err()
        .map(|e| e.contains("KeyboardInterrupt"))
        .unwrap_or(false);
    println!(
        "RESULT mode=async_exc thread_id={id} set_async_exc_return={set_result} loop_elapsed_ms={loop_elapsed_ms} stop_latency_ms={} interrupted={interrupted} result={result:?}",
        interrupt_at.elapsed().as_millis()
    );
    report_after("async_exc");
}

fn report_after(mode: &str) {
    std::thread::sleep(Duration::from_millis(20));
    let quick_ok = quick().is_ok();
    println!(
        "RESULT mode={mode} interpreter_usable_after={} sys_exc_info_cleared={}",
        quick_ok,
        sys_exc_info_cleared()
    );
}

fn main() {
    init_python().expect("python init");
    println!("python_version={}", Python::version_str());
    println!("RESULT baseline_quick_call_ok={}", quick().is_ok());
    let mode = std::env::args().nth(1).unwrap_or_else(|| "main".to_string());
    match mode.as_str() {
        "main" => mode_main_interrupt(),
        "worker" => mode_worker_interrupt(),
        "async_exc" => mode_async_exc("busy"),
        "async_exc_sleep" => mode_async_exc("sleep"),
        other => {
            eprintln!("unknown mode {other}; expected main|worker|async_exc");
            std::process::exit(2);
        }
    }
}
