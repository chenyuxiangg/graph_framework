//! Probe 2: tokio + Python execution model, measured against the GIL.
//!
//! Every variant runs the SAME Python work unit 4 times concurrently and
//! records wall time plus the maximum scheduling gap of a 5 ms ticker task
//! living on the same tokio runtime. The ticker gap is the direct measurement
//! of "does node execution block the event loop?".
//!
//! Work units:
//!   * `sleep_node(0.4)` -> `time.sleep`, releases the GIL
//!   * `busy_node(0.4)`  -> pure-Python busy loop, holds the GIL
//!
//! Execution models compared:
//!   * sequential baseline (single thread, no concurrency)
//!   * tokio multi-thread runtime + `tokio::spawn` + `Python::attach`
//!   * tokio multi-thread runtime + `spawn_blocking` + `Python::attach`
//!   * `std::thread` scope + `Python::attach`
//!   * tokio current_thread runtime + `spawn` (single-thread event loop!)
//!   * tokio current_thread runtime + `spawn_blocking`
//!   * pure-Rust async control (`tokio::time::sleep`)

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use pyo3::prelude::*;
use pyo3::types::PyModule;
use serde_json::{json, Value};
use tokio::runtime::Builder;
use tokio_util::sync::CancellationToken;
use zgraph_spike::{init_python, py_to_json};

const NODE_SECS: f64 = 0.4;
const N_TASKS: usize = 4;
const WORKERS: usize = 4;

#[derive(Clone, Copy, Debug)]
enum Work {
    Sleep,
    Busy,
}

impl Work {
    fn func(self) -> &'static str {
        match self {
            Work::Sleep => "sleep_node",
            Work::Busy => "busy_node",
        }
    }
    fn label(self) -> &'static str {
        match self {
            Work::Sleep => "sleep(GIL released)",
            Work::Busy => "busy(GIL held)",
        }
    }
}

fn call_node(work: Work) -> PyResult<Value> {
    Python::attach(|py| {
        let module = PyModule::import(py, "node_work")?;
        let func = module.getattr(work.func())?;
        let result = func.call1((NODE_SECS,))?;
        py_to_json(&result)
    })
}

/// Ticker task: ticks every 5 ms and records the max observed interval.
/// The max gap tells us how long the runtime was unable to poll it.
fn spawn_ticker() -> (CancellationToken, Arc<AtomicU64>, Arc<Mutex<Instant>>) {
    let token = CancellationToken::new();
    let max_gap = Arc::new(AtomicU64::new(0));
    let last_tick = Arc::new(Mutex::new(Instant::now()));
    let token2 = token.clone();
    let max_gap2 = max_gap.clone();
    let last_tick2 = last_tick.clone();
    tokio::spawn(async move {
        let mut iv = tokio::time::interval(Duration::from_millis(5));
        iv.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        let mut last = Instant::now();
        loop {
            tokio::select! {
                _ = iv.tick() => {}
                _ = token2.cancelled() => break,
            }
            let now = Instant::now();
            let gap = now.duration_since(last).as_millis() as u64;
            max_gap2.fetch_max(gap, Ordering::SeqCst);
            last = now;
            *last_tick2.lock().unwrap() = now;
        }
    });
    (token, max_gap, last_tick)
}

/// Gap seen at the ticker plus any tail stall between the last tick and `end`.
/// On a current_thread runtime a blocked event loop can starve the ticker so
/// completely that it is never polled again; the tail stall captures that case.
fn effective_gap(max_gap: &Arc<AtomicU64>, last_tick: &Arc<Mutex<Instant>>, end: Instant) -> u64 {
    let since = end
        .saturating_duration_since(*last_tick.lock().unwrap())
        .as_millis() as u64;
    max_gap.load(Ordering::SeqCst).max(since)
}

fn report(name: &str, work: Work, wall_ms: f64, ticker_gap_ms: u64, extra: Value) {
    println!(
        "RESULT variant={name} work=\"{}\" wall_ms={wall_ms:.1} ticker_max_gap_ms={ticker_gap_ms} extra={}",
        work.label(),
        serde_json::to_string(&extra).unwrap()
    );
}

/// Sequential baseline: 4 calls back-to-back on the calling thread.
fn seq_baseline(work: Work) -> f64 {
    let t = Instant::now();
    for _ in 0..N_TASKS {
        call_node(work).expect("node call");
    }
    t.elapsed().as_secs_f64() * 1000.0
}

/// `spawn_attach`: `tokio::spawn` an async block which synchronously calls into
/// Python. This is the anti-pattern the design's pseudocode implies.
fn tokio_mt_spawn_attach(work: Work) -> (f64, u64, Vec<f64>) {
    let rt = Builder::new_multi_thread()
        .worker_threads(WORKERS)
        .enable_all()
        .build()
        .unwrap();
    rt.block_on(async move {
        let (token, max_gap, last_tick) = spawn_ticker();
        tokio::time::sleep(Duration::from_millis(15)).await;
        let start = Instant::now();
        let mut handles = Vec::new();
        for _ in 0..N_TASKS {
            handles.push(tokio::spawn(async move {
                call_node(work).expect("node call");
                start.elapsed().as_secs_f64() * 1000.0
            }));
        }
        let mut finishes = Vec::new();
        for h in handles {
            finishes.push(h.await.unwrap());
        }
        let wall = start.elapsed().as_secs_f64() * 1000.0;
        let end = Instant::now();
        let gap = effective_gap(&max_gap, &last_tick, end);
        token.cancel();
        tokio::time::sleep(Duration::from_millis(20)).await;
        (wall, gap, finishes)
    })
}

/// `spawn_blocking`: Python runs on the blocking pool, runtime workers stay free.
fn tokio_mt_spawn_blocking(work: Work) -> (f64, u64, Vec<f64>) {
    let rt = Builder::new_multi_thread()
        .worker_threads(WORKERS)
        .enable_all()
        .build()
        .unwrap();
    rt.block_on(async move {
        let (token, max_gap, last_tick) = spawn_ticker();
        tokio::time::sleep(Duration::from_millis(15)).await;
        let start = Instant::now();
        let mut handles = Vec::new();
        for _ in 0..N_TASKS {
            handles.push(tokio::task::spawn_blocking(move || {
                call_node(work).expect("node call");
                start.elapsed().as_secs_f64() * 1000.0
            }));
        }
        let mut finishes = Vec::new();
        for h in handles {
            finishes.push(h.await.unwrap());
        }
        let wall = start.elapsed().as_secs_f64() * 1000.0;
        let end = Instant::now();
        let gap = effective_gap(&max_gap, &last_tick, end);
        token.cancel();
        tokio::time::sleep(Duration::from_millis(20)).await;
        (wall, gap, finishes)
    })
}

fn std_threads(work: Work) -> (f64, Vec<f64>) {
    let start = Instant::now();
    let finishes = std::thread::scope(|scope| {
        let mut handles = Vec::new();
        for _ in 0..N_TASKS {
            handles.push(scope.spawn(move || {
                call_node(work).expect("node call");
                start.elapsed().as_secs_f64() * 1000.0
            }));
        }
        handles
            .into_iter()
            .map(|h| h.join().unwrap())
            .collect::<Vec<_>>()
    });
    let wall = start.elapsed().as_secs_f64() * 1000.0;
    (wall, finishes)
}

/// Single-thread event loop (current_thread runtime) + `tokio::spawn`.
fn tokio_ct_spawn_attach(work: Work) -> (f64, u64) {
    let rt = Builder::new_current_thread().enable_all().build().unwrap();
    rt.block_on(async move {
        let (token, max_gap, last_tick) = spawn_ticker();
        tokio::time::sleep(Duration::from_millis(15)).await;
        let start = Instant::now();
        let mut handles = Vec::new();
        for _ in 0..N_TASKS {
            handles.push(tokio::spawn(async move {
                call_node(work).expect("node call");
            }));
        }
        for h in handles {
            h.await.unwrap();
        }
        let wall = start.elapsed().as_secs_f64() * 1000.0;
        let end = Instant::now();
        let gap = effective_gap(&max_gap, &last_tick, end);
        token.cancel();
        tokio::time::sleep(Duration::from_millis(20)).await;
        (wall, gap)
    })
}

/// Single-thread event loop + `spawn_blocking` (recommended model).
fn tokio_ct_spawn_blocking(work: Work) -> (f64, u64) {
    let rt = Builder::new_current_thread().enable_all().build().unwrap();
    rt.block_on(async move {
        let (token, max_gap, last_tick) = spawn_ticker();
        tokio::time::sleep(Duration::from_millis(15)).await;
        let start = Instant::now();
        let mut handles = Vec::new();
        for _ in 0..N_TASKS {
            handles.push(tokio::task::spawn_blocking(move || {
                call_node(work).expect("node call");
            }));
        }
        for h in handles {
            h.await.unwrap();
        }
        let wall = start.elapsed().as_secs_f64() * 1000.0;
        let end = Instant::now();
        let gap = effective_gap(&max_gap, &last_tick, end);
        token.cancel();
        tokio::time::sleep(Duration::from_millis(20)).await;
        (wall, gap)
    })
}

/// Control: pure async sleep, no Python at all.
fn pure_async_control() -> (f64, u64) {
    let rt = Builder::new_multi_thread()
        .worker_threads(WORKERS)
        .enable_all()
        .build()
        .unwrap();
    rt.block_on(async move {
        let (token, max_gap, last_tick) = spawn_ticker();
        tokio::time::sleep(Duration::from_millis(15)).await;
        let start = Instant::now();
        let mut handles = Vec::new();
        for _ in 0..N_TASKS {
            handles.push(tokio::spawn(async move {
                tokio::time::sleep(Duration::from_secs_f64(NODE_SECS)).await;
            }));
        }
        for h in handles {
            h.await.unwrap();
        }
        let wall = start.elapsed().as_secs_f64() * 1000.0;
        let end = Instant::now();
        let gap = effective_gap(&max_gap, &last_tick, end);
        token.cancel();
        tokio::time::sleep(Duration::from_millis(20)).await;
        (wall, gap)
    })
}

fn run_matrix(rep: usize) {
    println!("--- rep {rep} ---");
    let work_list = [
        (Work::Sleep, "rust_sequential", seq_baseline(Work::Sleep), 0u64, Vec::<f64>::new()),
        (Work::Busy, "rust_sequential", seq_baseline(Work::Busy), 0u64, Vec::<f64>::new()),
    ];
    for (work, name, wall, gap, per) in work_list {
        report(name, work, wall, gap, json!({"per_task_finish_offset_ms": per}));
    }

    let (wall, gap, per) = tokio_mt_spawn_attach(Work::Sleep);
    report("tokio_mt4_spawn_attach", Work::Sleep, wall, gap, json!({"per_task_finish_offset_ms": per}));
    let (wall, gap, per) = tokio_mt_spawn_attach(Work::Busy);
    report("tokio_mt4_spawn_attach", Work::Busy, wall, gap, json!({"per_task_finish_offset_ms": per}));
    let (wall, gap, per) = tokio_mt_spawn_blocking(Work::Sleep);
    report("tokio_mt4_spawn_blocking", Work::Sleep, wall, gap, json!({"per_task_finish_offset_ms": per}));
    let (wall, gap, per) = tokio_mt_spawn_blocking(Work::Busy);
    report("tokio_mt4_spawn_blocking", Work::Busy, wall, gap, json!({"per_task_finish_offset_ms": per}));

    let (wall, per) = std_threads(Work::Sleep);
    report("std_thread4", Work::Sleep, wall, 0, json!({"per_task_finish_offset_ms": per}));
    let (wall, per) = std_threads(Work::Busy);
    report("std_thread4", Work::Busy, wall, 0, json!({"per_task_finish_offset_ms": per}));

    let (wall, gap) = tokio_ct_spawn_attach(Work::Sleep);
    report("tokio_ct_spawn_attach", Work::Sleep, wall, gap, json!({}));
    let (wall, gap) = tokio_ct_spawn_blocking(Work::Sleep);
    report("tokio_ct_spawn_blocking", Work::Sleep, wall, gap, json!({}));
    let (wall, gap) = tokio_ct_spawn_blocking(Work::Busy);
    report("tokio_ct_spawn_blocking", Work::Busy, wall, gap, json!({}));

    let (wall, gap) = pure_async_control();
    report("tokio_mt4_pure_async_control", Work::Sleep, wall, gap, json!({"note": "no Python, tokio::time::sleep"}));
}

fn main() {
    init_python().expect("python init");
    let cpus = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(0);
    println!("host_cpus={cpus} tokio_worker_threads={WORKERS} n_tasks={N_TASKS} node_secs={NODE_SECS}");
    println!("python_version={}", Python::version_str());

    run_matrix(0);
    // Key GIL comparison repeated once to show stability.
    println!("--- key GIL comparison, repeat ---");
    let (wall, gap, per) = tokio_mt_spawn_attach(Work::Busy);
    report("tokio_mt4_spawn_attach", Work::Busy, wall, gap, json!({"per_task_finish_offset_ms": per, "repeat": true}));
    let (wall, gap, _per) = tokio_mt_spawn_blocking(Work::Sleep);
    report("tokio_mt4_spawn_blocking", Work::Sleep, wall, gap, json!({"repeat": true}));

    println!("PROBE2_OK=1");
}
