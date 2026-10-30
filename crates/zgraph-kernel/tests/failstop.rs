//! trace 写端 fail-stop（ISSUE-036 / 06 §6.4.5）：
//! 事件循环轮询 `TraceSink::take_error` → 内存态 `Failed` → 尽力写一次终态。
//!
//! 故障注入在 sink 层完成（不依赖真实磁盘故障）：`FailingSink` 在 N 次调用后报告
//! 一次不可恢复错误，模拟 CLI trace 桥对 `TraceHandle::take_error` 的暴露。

mod common;

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use serde_json::json;
use zgraph_kernel::{
    FakeExecutor, GateResultRecord, InstanceId, InstanceSnapshot, NodeId, RunFinishedInfo, RunId,
    RunOptions, RunStartedInfo, RunStatus, StreamingEvent, TraceSink, TransitionRecord,
    CODE_STORAGE_ERROR,
};

/// 记录调用并在第 `fail_after` 次调用后报告一次写端错误的 sink。
struct FailingSink {
    calls: AtomicUsize,
    /// `RunFinished` 之后的 sink 调用数（必须为 0，I5/I6）。
    calls_after_finish: AtomicUsize,
    finished: Mutex<Option<RunFinishedInfo>>,
    error: Mutex<Option<String>>,
    fail_after: usize,
}

impl FailingSink {
    fn new(message: &str, fail_after: usize) -> Arc<Self> {
        Arc::new(Self {
            calls: AtomicUsize::new(0),
            calls_after_finish: AtomicUsize::new(0),
            finished: Mutex::new(None),
            error: Mutex::new(Some(message.to_string())),
            fail_after,
        })
    }

    fn lock<T>(slot: &Mutex<T>) -> MutexGuard<'_, T> {
        slot.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn note_call(&self) {
        self.calls.fetch_add(1, Ordering::SeqCst);
        if Self::lock(&self.finished).is_some() {
            self.calls_after_finish.fetch_add(1, Ordering::SeqCst);
        }
    }

    fn finished(&self) -> Option<RunFinishedInfo> {
        Self::lock(&self.finished).clone()
    }

    fn calls_after_finish(&self) -> usize {
        self.calls_after_finish.load(Ordering::SeqCst)
    }
}

impl TraceSink for FailingSink {
    fn run_started(&self, _info: RunStartedInfo) {
        self.note_call();
    }

    fn instance_snapshot(&self, _snapshot: InstanceSnapshot) {
        self.note_call();
    }

    fn transition(&self, _record: TransitionRecord, _snapshot: InstanceSnapshot) {
        self.note_call();
    }

    fn gate_result(&self, _record: GateResultRecord) {
        self.note_call();
    }

    fn node_timed_out(
        &self,
        _run_id: &RunId,
        _instance_id: &InstanceId,
        _elapsed_ms: u64,
        _at: String,
    ) {
        self.note_call();
    }

    fn node_abandoned(&self, _run_id: &RunId, _instance_id: &InstanceId, _at: String) {
        self.note_call();
    }

    fn streaming(
        &self,
        _run_id: &RunId,
        _instance_id: &InstanceId,
        _node_id: &NodeId,
        _event: StreamingEvent,
        _at: String,
    ) -> bool {
        self.note_call();
        true
    }

    fn run_finished(&self, info: RunFinishedInfo) {
        self.note_call();
        *Self::lock(&self.finished) = Some(info);
    }

    fn take_error(&self) -> Option<String> {
        if self.calls.load(Ordering::SeqCst) < self.fail_after {
            return None;
        }
        Self::lock(&self.error).take()
    }
}

fn storage_options() -> RunOptions {
    common::options_with(|options| {
        options.tick_interval_ms = 20;
    })
}

#[tokio::test]
async fn write_error_fails_run_and_writes_best_effort_terminal_state() {
    let sink = FailingSink::new("injected trace write failure", 6);
    let executor =
        Arc::new(FakeExecutor::new().delay("work", Duration::from_secs(3), json!({"r": "late"})));
    let spec = common::spec_with(
        common::compile_text(common::LINEAR_JSON),
        common::entry_inputs(&[("start", "x", json!({"v": 1}))]),
        storage_options(),
    );

    let started = Instant::now();
    let outcome = zgraph_kernel::run_to_completion(None, spec, executor, sink.clone())
        .await
        .expect("run starts");
    let elapsed = started.elapsed();

    assert_eq!(outcome.status, RunStatus::Failed, "{outcome:?}");
    let error = outcome.error.as_ref().expect("in-memory storage error");
    assert_eq!(error.code, CODE_STORAGE_ERROR);
    assert!(
        error.message.contains("injected trace write failure"),
        "{error:?}"
    );
    assert_eq!(outcome.output, None, "failed run has no output");
    assert!(
        elapsed < Duration::from_secs(2),
        "fail-stop must abort before the 3s node finishes (elapsed {elapsed:?})"
    );

    let finished = sink.finished().expect("best-effort RunFinished");
    assert_eq!(finished.status, RunStatus::Failed);
    assert!(
        finished.error.is_none(),
        "storage 错误不写入 domain error（runs.error_code 保持 NULL，06 §6.12.3 D3）"
    );
    assert_eq!(
        sink.calls_after_finish(),
        0,
        "RunFinished 之后不得再有 sink 调用（I5/I6）"
    );
}

#[tokio::test]
async fn trace_disabled_does_not_poll_or_fail() {
    let sink = FailingSink::new("injected trace write failure", 1);
    let executor =
        Arc::new(FakeExecutor::new().delay("work", Duration::from_millis(50), json!({"r": "ok"})));
    let spec = common::spec_with(
        common::compile_text(common::LINEAR_JSON),
        common::entry_inputs(&[("start", "x", json!({"v": 1}))]),
        common::options_with(|options| {
            options.tick_interval_ms = 20;
            options.trace_enabled = false;
        }),
    );

    let outcome = zgraph_kernel::run_to_completion(None, spec, executor, sink.clone())
        .await
        .expect("run starts");
    assert_eq!(outcome.status, RunStatus::Completed, "{outcome:?}");
    assert!(outcome.error.is_none());
    assert!(
        sink.finished().is_none(),
        "trace_enabled=false 时内核不得调用 sink"
    );
}
