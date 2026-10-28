//! Probe 8: minimal single-threaded event loop + node completion events.
//!
//! This is the recommended execution model skeleton:
//!
//!   * a `current_thread` tokio runtime plays the role of the event loop;
//!   * the loop owns `HashMap<node_id, NodeState>` and mutates it only between
//!     `await`s;
//!   * node execution is a `tokio::spawn`ed task that does the blocking Python
//!     call inside `spawn_blocking` and sends a completion *event* over an mpsc
//!     channel. It never captures `&mut` engine state, so there is no aliasing
//!     problem and no `'static` violation.
//!
//! DAG:  a(0.2s) + b(0.2s) in parallel -> c(0.05s) -> d(0.05s)
//! Expected wall ~0.3s, ticker max gap should stay tiny (event loop not blocked).

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use pyo3::prelude::*;
use pyo3::types::PyModule;
use serde_json::{json, Value};
use tokio::runtime::Builder;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use zgraph_spike::{init_python, py_to_json};

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

#[derive(Clone)]
struct NodeSpec {
    name: &'static str,
    deps: Vec<&'static str>,
    seconds: f64,
}

#[derive(Clone, Copy, PartialEq)]
enum Status {
    Pending,
    Running,
    Done,
}

struct NodeState {
    spec: NodeSpec,
    status: Status,
    started_at: Option<Instant>,
    duration_ms: Option<f64>,
    output: Option<Value>,
}

/// Blocking Python execution. Only owned/'static data is captured, so this can
/// safely be moved into a task.
fn run_python_node(name: &'static str, seconds: f64) -> PyResult<Value> {
    Python::attach(|py| {
        let module = PyModule::import(py, "node_work")?;
        let func = module.getattr("sleep_node")?;
        let result = func.call1((seconds,))?;
        let value = py_to_json(&result)?;
        Ok(json!({"node": name, "result": value}))
    })
}

fn spawn_node(spec: NodeSpec, tx: mpsc::Sender<Value>) {
    tokio::spawn(async move {
        let name = spec.name;
        let seconds = spec.seconds;
        let joined = tokio::task::spawn_blocking(move || run_python_node(name, seconds)).await;
        let event = match joined {
            Ok(Ok(output)) => json!({"event": "done", "node": name, "output": output}),
            Ok(Err(err)) => json!({"event": "failed", "node": name, "error": err.to_string()}),
            Err(err) => json!({"event": "failed", "node": name, "error": format!("join error: {err}")}),
        };
        let _ = tx.send(event).await;
    });
}

async fn run_dag(specs: Vec<NodeSpec>) -> (Value, f64) {
    let mut nodes: HashMap<&'static str, NodeState> = specs
        .into_iter()
        .map(|spec| {
            (
                spec.name,
                NodeState {
                    spec,
                    status: Status::Pending,
                    started_at: None,
                    duration_ms: None,
                    output: None,
                },
            )
        })
        .collect();
    let total = nodes.len();
    let (tx, mut rx) = mpsc::channel::<Value>(64);
    let mut completed = 0usize;
    let mut start_order: Vec<String> = Vec::new();
    let mut finish_order: Vec<String> = Vec::new();
    let loop_started = Instant::now();

    loop {
        // 1. Launch every node whose dependencies are all Done. All mutation of
        //    `nodes` happens here, on the event-loop task, never in a spawned task.
        let ready: Vec<NodeSpec> = nodes
            .values()
            .filter(|state| state.status == Status::Pending)
            .filter(|state| {
                state
                    .spec
                    .deps
                    .iter()
                    .all(|dep| nodes.get(dep).map(|n| n.status == Status::Done).unwrap_or(false))
            })
            .map(|state| state.spec.clone())
            .collect();
        for spec in ready {
            let state = nodes.get_mut(spec.name).unwrap();
            state.status = Status::Running;
            state.started_at = Some(Instant::now());
            start_order.push(spec.name.to_string());
            println!(
                "start node={} at_ms={:.1}",
                spec.name,
                loop_started.elapsed().as_secs_f64() * 1000.0
            );
            spawn_node(spec, tx.clone());
        }
        if completed == total {
            break;
        }
        // 2. Wait for the next completion event; only owned state is touched.
        let event = match rx.recv().await {
            Some(event) => event,
            None => break,
        };
        let node = event["node"].as_str().unwrap_or("<unknown>").to_string();
        if let Some(state) = nodes.get_mut(node.as_str()) {
            state.status = Status::Done;
            state.duration_ms = state
                .started_at
                .map(|started| started.elapsed().as_secs_f64() * 1000.0);
            state.output = event.get("output").cloned();
        }
        completed += 1;
        finish_order.push(node.clone());
        println!(
            "finish node={} at_ms={:.1}",
            node,
            loop_started.elapsed().as_secs_f64() * 1000.0
        );
    }

    let wall_ms = loop_started.elapsed().as_secs_f64() * 1000.0;
    let node_report: Vec<Value> = nodes
        .values()
        .map(|state| {
            json!({
                "node": state.spec.name,
                "status": match state.status { Status::Done => "done", Status::Running => "running", Status::Pending => "pending" },
                "duration_ms": state.duration_ms,
                "output": state.output,
            })
        })
        .collect();
    let done = nodes.values().filter(|s| s.status == Status::Done).count();
    (
        json!({
            "nodes_done": done,
            "nodes_total": total,
            "start_order": start_order,
            "finish_order": finish_order,
            "nodes": node_report,
        }),
        wall_ms,
    )
}

fn main() {
    init_python().expect("python init");
    println!("python_version={}", Python::version_str());
    println!("MODEL: single-thread event loop owns state; node tasks only send events back over mpsc");

    let specs = vec![
        NodeSpec { name: "a", deps: vec![], seconds: 0.2 },
        NodeSpec { name: "b", deps: vec![], seconds: 0.2 },
        NodeSpec { name: "c", deps: vec!["a", "b"], seconds: 0.05 },
        NodeSpec { name: "d", deps: vec!["c"], seconds: 0.05 },
    ];
    let sequential_expected_ms: f64 = specs.iter().map(|s| s.seconds * 1000.0).sum();

    let rt = Builder::new_current_thread().enable_all().build().unwrap();
    let (result, wall_ms, ticker_gap_ms) = rt.block_on(async move {
        let (token, max_gap, last_tick) = spawn_ticker();
        tokio::time::sleep(Duration::from_millis(20)).await;
        let (result, wall_ms) = run_dag(specs).await;
        let end = Instant::now();
        let gap = effective_gap(&max_gap, &last_tick, end);
        token.cancel();
        tokio::time::sleep(Duration::from_millis(20)).await;
        (result, wall_ms, gap)
    });

    println!(
        "RESULT event_loop_model wall_ms={wall_ms:.1} ticker_max_gap_ms={ticker_gap_ms} sequential_if_serial_ms={sequential_expected_ms:.1} parallel_speedup_x={:.2} detail={}",
        sequential_expected_ms / wall_ms.max(0.001),
        serde_json::to_string(&result).unwrap()
    );
    println!(
        "EVENT_LOOP_DAG_ALL_NODES_DONE={}",
        result["nodes_done"] == result["nodes_total"]
    );
    println!("PROBE8_OK=1");
}
