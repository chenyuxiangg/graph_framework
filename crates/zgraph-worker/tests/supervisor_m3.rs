//! M3 supervisor 集成测试：进程组 kill、取消升级、退出约定、帧空闲超时、流式上限。
//!
//! 语义依据：`doc/architecture-v2.md` §3.3/§3.4（D1）、`schemas/worker-protocol-v1.md` §4/§6。
//! 每个用例都断言 **pid 消失**（无僵尸/无残留孙进程），禁止只断言错误码。

mod common;

use std::time::{Duration, Instant};

use serde_json::json;

use zgraph_worker::{
    no_cancel, run_supervised, run_worker, sample_request, CancelSignal, Frame, RunRequest,
    StreamingLimits, SupervisorConfig, WorkerFailure, WorkerRun, CODE_PROTOCOL,
};

fn noop_sink() -> impl FnMut(&Frame) {
    |_frame: &Frame| {}
}

/// 用 `SupervisorConfig` 跑一次（无外部取消），用于注入自定义帧超时/流式上限。
async fn run_with_supervisor(
    config: &SupervisorConfig,
    request: &RunRequest,
) -> Result<WorkerRun, WorkerFailure> {
    run_supervised(config, request, &mut noop_sink(), no_cancel()).await
}

#[tokio::test]
async fn timeout_kills_process_group_including_grandchild() {
    let paths = common::PidPaths::new();
    let config = common::script_config(
        "grandchild_worker.py",
        paths.env(),
        Duration::from_millis(800),
    );
    let request = sample_request("unused", "run", json!({}));

    let err = run_worker(&config, &request)
        .await
        .expect_err("worker must time out");
    assert!(
        matches!(err, WorkerFailure::Timeout(_)),
        "expected timeout, got {err:?}"
    );

    let worker_pid = common::read_pid(&paths.worker);
    let child_pid = common::read_pid(&paths.child);
    common::assert_pid_gone(&worker_pid, common::PID_GONE_TIMEOUT).await;
    common::assert_pid_gone(&child_pid, common::PID_GONE_TIMEOUT).await;
}

#[tokio::test]
async fn cancel_escalates_through_sigterm_to_sigkill_group() {
    let paths = common::PidPaths::new();
    let mut config = common::supervisor_config(Duration::from_secs(10));
    config.worker = common::script_config(
        "cancel_ignore_worker.py",
        paths.env(),
        Duration::from_secs(10),
    );
    // fixture 忽略 SIGTERM：只有走到 SIGKILL 阶段才会退出。
    config.cancel_grace = Duration::from_millis(150);
    config.term_grace = Duration::from_millis(150);
    let request = sample_request("unused", "run", json!({}));
    let cancel: CancelSignal = Box::pin(async {
        tokio::time::sleep(Duration::from_millis(300)).await;
    });

    let started = Instant::now();
    let err = run_supervised(&config, &request, &mut noop_sink(), cancel)
        .await
        .expect_err("must be cancelled");
    let elapsed = started.elapsed();
    assert!(
        matches!(err, WorkerFailure::Cancelled),
        "expected cancelled, got {err:?}"
    );
    // 必须真的走到 SIGKILL：忽略 SIGTERM 的进程只能在最后阶段死。
    assert!(
        elapsed >= config.cancel_grace + config.term_grace,
        "escalation too fast: {elapsed:?}"
    );
    assert!(
        elapsed < Duration::from_secs(5),
        "cancellation must finish quickly, took {elapsed:?}"
    );

    let worker_pid = common::read_pid(&paths.worker);
    common::assert_pid_gone(&worker_pid, common::PID_GONE_TIMEOUT).await;
}

#[tokio::test]
async fn result_without_exit_is_protocol_error_and_process_group_killed() {
    let paths = common::PidPaths::new();
    let mut config = common::supervisor_config(Duration::from_secs(10));
    config.worker = common::script_config(
        "result_no_exit_worker.py",
        paths.env(),
        Duration::from_secs(10),
    );
    config.worker.exit_grace = Duration::from_millis(200);
    let request = sample_request("unused", "run", json!({}));

    let err = run_with_supervisor(&config, &request)
        .await
        .expect_err("result without exit must be rejected");
    match err {
        WorkerFailure::Protocol(protocol) => {
            assert_eq!(protocol.code, CODE_PROTOCOL);
            assert!(
                protocol.message.contains("did not exit after result"),
                "unexpected message: {}",
                protocol.message
            );
        }
        other => panic!("expected protocol error, got {other:?}"),
    }

    let worker_pid = common::read_pid(&paths.worker);
    common::assert_pid_gone(&worker_pid, common::PID_GONE_TIMEOUT).await;
}

#[tokio::test]
async fn frame_idle_timeout_applies_when_no_kernel_deadline() {
    let paths = common::PidPaths::new();
    let mut config = common::supervisor_config(Duration::from_secs(10));
    config.worker = common::script_config("stall_worker.py", paths.env(), Duration::from_secs(10));
    config.frame_timeout = Duration::from_millis(200);
    // sample_request 的 deadline_ms = 0：无内核 deadline，用 frame_timeout 兜底。
    let request = sample_request("unused", "run", json!({}));

    let started = Instant::now();
    let err = run_with_supervisor(&config, &request)
        .await
        .expect_err("stalled worker must be rejected");
    match err {
        WorkerFailure::Protocol(protocol) => {
            assert_eq!(protocol.code, CODE_PROTOCOL);
            assert!(
                protocol.message.contains("frame idle timeout"),
                "unexpected message: {}",
                protocol.message
            );
        }
        other => panic!("expected protocol error, got {other:?}"),
    }
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "frame timeout must not block the caller"
    );

    let worker_pid = common::read_pid(&paths.worker);
    common::assert_pid_gone(&worker_pid, common::PID_GONE_TIMEOUT).await;
}

#[tokio::test]
async fn frame_timeout_is_suspended_when_kernel_deadline_present() {
    let paths = common::PidPaths::new();
    let mut config = common::supervisor_config(Duration::from_millis(600));
    config.worker =
        common::script_config("stall_worker.py", paths.env(), Duration::from_millis(600));
    // 帧超时远小于 supervisor 总超时：若豁免口径错误，会先报协议错误。
    config.frame_timeout = Duration::from_millis(100);
    let mut request = sample_request("unused", "run", json!({}));
    request.deadline_ms = u64::MAX / 2;

    let err = run_with_supervisor(&config, &request)
        .await
        .expect_err("supervisor fallback timeout must fire");
    assert!(
        matches!(err, WorkerFailure::Timeout(_)),
        "frame timeout must be suspended for a run with kernel deadline, got {err:?}"
    );

    let worker_pid = common::read_pid(&paths.worker);
    common::assert_pid_gone(&worker_pid, common::PID_GONE_TIMEOUT).await;
}

#[tokio::test]
async fn emit_event_limit_drops_frames_and_keeps_counting() {
    let env = vec![
        ("ZGRAPH_FIXTURE_COUNT".to_string(), "50".to_string()),
        ("ZGRAPH_FIXTURE_TEXT_BYTES".to_string(), "200".to_string()),
    ];
    let mut config = common::supervisor_config(Duration::from_secs(10));
    config.worker = common::script_config("stream_flood_worker.py", env, Duration::from_secs(10));
    config.streaming = StreamingLimits {
        max_events: 8,
        max_bytes: 8 * 1024 * 1024,
    };
    let request = sample_request("unused", "run", json!({}));

    let run = run_with_supervisor(&config, &request)
        .await
        .expect("flood worker run");
    assert_eq!(run.output["emitted"], 50);
    assert_eq!(run.events.len(), 8, "only accepted frames are retained");
    assert_eq!(run.streaming.delivered, 8);
    assert_eq!(run.streaming.dropped, 42);
    assert!(
        run.streaming.bytes <= config.streaming.max_bytes as u64,
        "retained bytes must stay bounded"
    );
}

#[tokio::test]
async fn emit_byte_limit_drops_frames_and_keeps_counting() {
    let env = vec![
        ("ZGRAPH_FIXTURE_COUNT".to_string(), "20".to_string()),
        ("ZGRAPH_FIXTURE_TEXT_BYTES".to_string(), "200".to_string()),
    ];
    let mut config = common::supervisor_config(Duration::from_secs(10));
    config.worker = common::script_config("stream_flood_worker.py", env, Duration::from_secs(10));
    config.streaming = StreamingLimits {
        max_events: 10_000,
        max_bytes: 1200,
    };
    let request = sample_request("unused", "run", json!({}));

    let run = run_with_supervisor(&config, &request)
        .await
        .expect("flood worker run");
    assert!(run.streaming.delivered >= 1, "at least one frame fits");
    assert!(run.streaming.dropped >= 1, "byte cap must drop frames");
    assert!(run.streaming.bytes <= 1200);
    assert_eq!(run.events.len() as u64, run.streaming.delivered);
    assert_eq!(run.streaming.delivered + run.streaming.dropped, 20);
}
