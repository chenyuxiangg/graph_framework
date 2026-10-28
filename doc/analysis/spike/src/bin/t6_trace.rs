//! Probe 6: trace/event channel + SQLite writer throughput.
//!
//! Measures the design's claim that "trace events go through an mpsc channel and
//! are persisted asynchronously to SQLite without blocking the main loop":
//!   * async pipeline: producer task -> bounded mpsc(1024) -> dedicated writer
//!     thread -> rusqlite (bundled) batched transactions, 10k events
//!   * ticker max gap on the event loop while that happens
//!   * synchronous baselines on the main thread: one 10k-row transaction, and
//!     10k autocommit inserts
//!   * bounded `try_send` overflow behaviour (how many events are dropped when
//!     the writer cannot keep up and the design chooses to drop)

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use rusqlite::Connection;
use serde_json::{json, Value};
use tokio::runtime::Builder;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

const N_EVENTS: usize = 10_000;
const CHANNEL_CAP: usize = 1024;
const BATCH: usize = 500;

#[derive(Debug)]
struct TraceEvent {
    seq: i64,
    node: &'static str,
    kind: &'static str,
    payload: String,
}

fn make_event(i: usize) -> TraceEvent {
    TraceEvent {
        seq: i as i64,
        node: "node-1",
        kind: "progress",
        payload: format!(
            "{{\"seq\":{i},\"message\":\"progress event with payload text\",\"labels\":[\"a\",\"b\"],\"nested\":{{\"x\":[1,null,true]}}}}"
        ),
    }
}

#[derive(Default)]
struct WriterStats {
    inserted: AtomicUsize,
    batches: AtomicUsize,
    max_batch_ms: AtomicU64,
}

fn spawn_ticker() -> (CancellationToken, Arc<AtomicU64>) {
    let token = CancellationToken::new();
    let max_gap = Arc::new(AtomicU64::new(0));
    let token2 = token.clone();
    let max_gap2 = max_gap.clone();
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
        }
    });
    (token, max_gap)
}

fn open_db(path: &PathBuf) -> rusqlite::Result<Connection> {
    let conn = Connection::open(path)?;
    conn.query_row("PRAGMA journal_mode=WAL", [], |row| row.get::<_, String>(0))?;
    conn.execute_batch("PRAGMA synchronous=NORMAL;")?;
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS trace_events (
             seq      INTEGER PRIMARY KEY,
             node     TEXT NOT NULL,
             kind     TEXT NOT NULL,
             payload  TEXT NOT NULL
         );",
    )?;
    Ok(conn)
}

fn db_writer(
    mut rx: mpsc::Receiver<TraceEvent>,
    path: PathBuf,
    stats: Arc<WriterStats>,
) -> rusqlite::Result<()> {
    let conn = open_db(&path)?;
    let mut batch: Vec<TraceEvent> = Vec::with_capacity(BATCH);
    loop {
        match rx.blocking_recv() {
            Some(ev) => batch.push(ev),
            None => break,
        }
        while batch.len() < BATCH {
            match rx.try_recv() {
                Ok(ev) => batch.push(ev),
                Err(_) => break,
            }
        }
        let batch_started = Instant::now();
        let tx = conn.unchecked_transaction()?;
        {
            let mut stmt = tx.prepare_cached(
                "INSERT INTO trace_events (seq, node, kind, payload) VALUES (?1, ?2, ?3, ?4)",
            )?;
            for ev in &batch {
                stmt.execute(rusqlite::params![ev.seq, ev.node, ev.kind, ev.payload])?;
            }
        }
        tx.commit()?;
        stats.inserted.fetch_add(batch.len(), Ordering::SeqCst);
        stats.batches.fetch_add(1, Ordering::SeqCst);
        let batch_ms = batch_started.elapsed().as_millis() as u64;
        stats.max_batch_ms.fetch_max(batch_ms, Ordering::SeqCst);
        batch.clear();
    }
    Ok(())
}

fn remove_db(path: &PathBuf) {
    for suffix in ["", "-wal", "-shm"] {
        let p = PathBuf::from(format!("{}{suffix}", path.display()));
        let _ = std::fs::remove_file(p);
    }
}

fn db_row_count(path: &PathBuf) -> i64 {
    let conn = Connection::open(path).unwrap();
    conn.query_row("SELECT COUNT(*) FROM trace_events", [], |row| row.get(0))
        .unwrap()
}

fn async_pipeline(db_path: PathBuf) -> Value {
    remove_db(&db_path);
    let stats = Arc::new(WriterStats::default());
    let stats_read = stats.clone();
    let rt = Builder::new_multi_thread()
        .worker_threads(4)
        .enable_all()
        .build()
        .unwrap();
    let writer_db_path = db_path.clone();

    let result = rt.block_on(async move {
        let (token, max_gap) = spawn_ticker();
        tokio::time::sleep(Duration::from_millis(20)).await;
        let (tx, rx) = mpsc::channel::<TraceEvent>(CHANNEL_CAP);
        let writer = std::thread::spawn(move || db_writer(rx, writer_db_path, stats.clone()));
        let started = Instant::now();
        for i in 0..N_EVENTS {
            if tx.send(make_event(i)).await.is_err() {
                break;
            }
        }
        drop(tx);
        let producer_wall_ms = started.elapsed().as_secs_f64() * 1000.0;
        let joined = tokio::task::spawn_blocking(move || writer.join()).await;
        let total_ms = started.elapsed().as_secs_f64() * 1000.0;
        token.cancel();
        tokio::time::sleep(Duration::from_millis(20)).await;
        let writer_result = match joined {
            Ok(Ok(Ok(()))) => "ok",
            Ok(Ok(Err(e))) => return json!({"writer_error": e.to_string()}),
            Ok(Err(_)) => "join_panic",
            Err(e) => return json!({"spawn_blocking_join_error": e.to_string()}),
        };
        json!({
            "writer_result": writer_result,
            "producer_wall_ms": producer_wall_ms,
            "total_ms": total_ms,
            "ticker_max_gap_ms": max_gap.load(Ordering::SeqCst),
            "inserted": stats_read.inserted.load(Ordering::SeqCst),
            "batches": stats_read.batches.load(Ordering::SeqCst),
            "max_batch_insert_ms": stats_read.max_batch_ms.load(Ordering::SeqCst),
            "per_event_us": total_ms * 1000.0 / N_EVENTS as f64,
        })
    });

    let mut result = result;
    if let Some(path) = result.get("db_path").and_then(|v| v.as_str()) {
        let _ = path;
    }
    result["db_rows_in_file"] = json!(db_row_count(&db_path));
    result["db_file_bytes"] = json!(std::fs::metadata(&db_path).map(|m| m.len()).unwrap_or(0));
    result
}

fn sync_single_transaction(db_path: PathBuf) -> Value {
    remove_db(&db_path);
    let conn = open_db(&db_path).unwrap();
    let started = Instant::now();
    let tx = conn.unchecked_transaction().unwrap();
    {
        let mut stmt = tx
            .prepare_cached("INSERT INTO trace_events (seq, node, kind, payload) VALUES (?1,?2,?3,?4)")
            .unwrap();
        for i in 0..N_EVENTS {
            let ev = make_event(i);
            stmt.execute(rusqlite::params![ev.seq, ev.node, ev.kind, ev.payload])
                .unwrap();
        }
    }
    tx.commit().unwrap();
    let elapsed_ms = started.elapsed().as_secs_f64() * 1000.0;
    json!({
        "total_ms": elapsed_ms,
        "per_event_us": elapsed_ms * 1000.0 / N_EVENTS as f64,
        "db_rows_in_file": db_row_count(&db_path),
        "db_file_bytes": std::fs::metadata(&db_path).map(|m| m.len()).unwrap_or(0),
    })
}

fn sync_autocommit(db_path: PathBuf, n: usize) -> Value {
    remove_db(&db_path);
    let conn = open_db(&db_path).unwrap();
    let started = Instant::now();
    let mut stmt = conn
        .prepare_cached("INSERT INTO trace_events (seq, node, kind, payload) VALUES (?1,?2,?3,?4)")
        .unwrap();
    for i in 0..n {
        let ev = make_event(i);
        stmt.execute(rusqlite::params![ev.seq, ev.node, ev.kind, ev.payload])
            .unwrap();
    }
    drop(stmt);
    let elapsed_ms = started.elapsed().as_secs_f64() * 1000.0;
    json!({
        "events": n,
        "total_ms": elapsed_ms,
        "per_event_us": elapsed_ms * 1000.0 / n as f64,
        "db_rows_in_file": db_row_count(&db_path),
    })
}

/// No consumer at all: how many events can be buffered, how fast is try_send,
/// and how many would the design have to drop?
fn bounded_try_send_drop() -> Value {
    let (tx, rx) = mpsc::channel::<TraceEvent>(CHANNEL_CAP);
    let started = Instant::now();
    let mut accepted = 0usize;
    let mut dropped_full = 0usize;
    for i in 0..N_EVENTS {
        match tx.try_send(make_event(i)) {
            Ok(()) => accepted += 1,
            Err(tokio::sync::mpsc::error::TrySendError::Full(_)) => dropped_full += 1,
            Err(tokio::sync::mpsc::error::TrySendError::Closed(_)) => break,
        }
    }
    let elapsed_us = started.elapsed().as_secs_f64() * 1_000_000.0;
    drop(rx);
    json!({
        "capacity": CHANNEL_CAP,
        "accepted": accepted,
        "dropped_full": dropped_full,
        "total_events": N_EVENTS,
        "elapsed_us": elapsed_us,
        "per_try_send_us": elapsed_us / N_EVENTS as f64,
    })
}

fn main() {
    let base = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    println!("n_events={N_EVENTS} channel_cap={CHANNEL_CAP} batch={BATCH}");

    let async_result = async_pipeline(base.join("trace_async.sqlite"));
    println!("RESULT variant=async_pipeline_10k detail={}", serde_json::to_string(&async_result).unwrap());

    let batch_result = sync_single_transaction(base.join("trace_batch.sqlite"));
    println!("RESULT variant=sync_single_txn_10k detail={}", serde_json::to_string(&batch_result).unwrap());

    let autocommit_result = sync_autocommit(base.join("trace_autocommit.sqlite"), N_EVENTS);
    println!("RESULT variant=sync_autocommit_10k detail={}", serde_json::to_string(&autocommit_result).unwrap());

    let drop_result = bounded_try_send_drop();
    println!("RESULT variant=bounded_try_send_10k detail={}", serde_json::to_string(&drop_result).unwrap());

    let async_total = async_result["total_ms"].as_f64().unwrap_or(0.0);
    let batch_total = batch_result["total_ms"].as_f64().unwrap_or(0.0);
    println!(
        "SUMMARY async_pipeline_total_ms={async_total:.1} sync_txn_total_ms={batch_total:.1} async_overhead_x={:.2} ticker_max_gap_ms={}",
        async_total / batch_total.max(0.001),
        async_result["ticker_max_gap_ms"]
    );
    println!("PROBE6_OK=1");
}
