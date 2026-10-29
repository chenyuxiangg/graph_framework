//! M0 验收：Rust supervisor ↔ Python worker 的协议与生命周期。
//!
//! 依赖 `python/` 下的 worker SDK 与 `tests/fixtures/workers/` 的协议 fixture；
//! 解释器可用 `ZGRAPH_PYTHON` 覆盖，默认 `python3`（只用标准库，CI 无需 uv）。

use std::path::PathBuf;
use std::time::Duration;

use zgraph_worker::{run_worker, sample_request, WorkerConfig, WorkerFailure};

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("repo root")
}

fn python_program() -> String {
    std::env::var("ZGRAPH_PYTHON").unwrap_or_else(|_| "python3".to_string())
}

fn worker_config(timeout: Duration) -> WorkerConfig {
    let root = repo_root();
    WorkerConfig {
        program: python_program(),
        args: vec!["-m".to_string(), "zgraph_worker.runtime".to_string()],
        env: vec![(
            "PYTHONPATH".to_string(),
            root.join("python").display().to_string(),
        )],
        cwd: Some(root),
        timeout,
        hello_timeout: Duration::from_secs(5),
        exit_grace: Duration::from_secs(2),
        max_stderr_bytes: 8 * 1024,
    }
}

/// 协议 fixture：直接以 `python <script>` 启动，绕过 SDK runtime。
fn fixture_config(script: &str, hello_timeout: Duration) -> WorkerConfig {
    let root = repo_root();
    let mut config = worker_config(Duration::from_secs(10));
    config.args = vec![root
        .join("tests/fixtures/workers")
        .join(script)
        .display()
        .to_string()];
    config.hello_timeout = hello_timeout;
    config
}

#[tokio::test]
async fn echo_worker_roundtrip() {
    let config = worker_config(Duration::from_secs(30));
    let request = sample_request(
        "zgraph_worker.echo",
        "run",
        serde_json::json!({"message": "hello-m0"}),
    );
    let run = run_worker(&config, &request).await.expect("worker run");
    assert_eq!(run.output["echo"], "hello-m0");
    assert!(
        run.events.iter().any(|frame| frame.kind == "emit"),
        "expected at least one emit frame, got {:?}",
        run.events
    );
}

#[tokio::test]
async fn worker_timeout_kills_and_reaps_process() {
    let temp = tempfile::tempdir().expect("tempdir");
    let pid_file = temp.path().join("worker.pid");
    let config = worker_config(Duration::from_millis(600));
    let request = sample_request(
        "zgraph_worker.slow",
        "run",
        serde_json::json!({"seconds": 5, "pid_file": pid_file.display().to_string()}),
    );
    let err = run_worker(&config, &request)
        .await
        .expect_err("must timeout");
    assert!(
        matches!(err, WorkerFailure::Timeout(_)),
        "expected timeout, got {err:?}"
    );

    let pid: String = std::fs::read_to_string(&pid_file)
        .expect("worker must write pid before sleeping")
        .trim()
        .to_string();
    assert_pid_reaped(&pid);
}

#[tokio::test]
async fn import_failure_is_reported() {
    let config = worker_config(Duration::from_secs(30));
    let request = sample_request("zgraph_worker.not_exists", "run", serde_json::json!({}));
    let err = run_worker(&config, &request)
        .await
        .expect_err("must fail import");
    match err {
        WorkerFailure::Worker(w) => assert_eq!(w.code, "NODE_WORKER_IMPORT"),
        other => panic!("expected worker import error, got {other:?}"),
    }
}

#[tokio::test]
async fn hello_timeout_kills_and_reaps_process() {
    let temp = tempfile::tempdir().expect("tempdir");
    let pid_file = temp.path().join("hello.pid");
    let script = "import os,sys,time; open(sys.argv[1],'w').write(str(os.getpid())); time.sleep(5)";
    let mut config = worker_config(Duration::from_secs(5));
    config.args = vec![
        "-c".to_string(),
        script.to_string(),
        pid_file.display().to_string(),
    ];
    config.hello_timeout = Duration::from_millis(300);
    let request = sample_request("unused", "run", serde_json::json!({}));
    let err = run_worker(&config, &request)
        .await
        .expect_err("must fail handshake");
    match err {
        WorkerFailure::Protocol(protocol) => assert_eq!(protocol.code, "NODE_WORKER_HANDSHAKE"),
        other => panic!("expected handshake error, got {other:?}"),
    }
    let pid: String = std::fs::read_to_string(&pid_file)
        .expect("probe must write pid")
        .trim()
        .to_string();
    assert_pid_reaped(&pid);
}

fn assert_pid_reaped(pid: &str) {
    for _ in 0..40 {
        let alive = std::process::Command::new("kill")
            .args(["-0", pid])
            .status()
            .map(|status| status.success())
            .unwrap_or(false);
        if !alive {
            return;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    panic!("pid {pid} still alive (zombie/not reaped)");
}

#[tokio::test]
async fn duplicate_result_is_rejected() {
    let config = fixture_config("dup_result_worker.py", Duration::from_secs(5));
    let request = sample_request("unused", "run", serde_json::json!({}));
    let err = run_worker(&config, &request)
        .await
        .expect_err("must reject duplicate result");
    match err {
        WorkerFailure::Protocol(protocol) => {
            assert!(
                protocol.message.contains("duplicate") || protocol.message.contains("trailing"),
                "unexpected message: {}",
                protocol.message
            );
        }
        other => panic!("expected protocol error, got {other:?}"),
    }
}

#[tokio::test]
async fn exit_without_result_is_rejected() {
    let config = fixture_config("no_result_worker.py", Duration::from_secs(5));
    let request = sample_request("unused", "run", serde_json::json!({}));
    let err = run_worker(&config, &request)
        .await
        .expect_err("must reject missing result");
    match err {
        WorkerFailure::Protocol(protocol) => {
            assert!(protocol.message.contains("closed stdout before result"));
        }
        other => panic!("expected protocol error, got {other:?}"),
    }
}

#[tokio::test]
async fn invalid_utf8_frame_is_rejected() {
    let config = fixture_config("invalid_utf8_worker.py", Duration::from_secs(5));
    let request = sample_request("unused", "run", serde_json::json!({}));
    let err = run_worker(&config, &request)
        .await
        .expect_err("must reject invalid utf-8");
    match err {
        WorkerFailure::Protocol(protocol) => {
            assert!(
                protocol.message.contains("invalid utf-8"),
                "unexpected message: {}",
                protocol.message
            );
        }
        other => panic!("expected protocol error, got {other:?}"),
    }
}

#[tokio::test]
async fn noisy_worker_stderr_is_captured() {
    let config = worker_config(Duration::from_secs(30));
    let request = sample_request("zgraph_worker.noisy", "run", serde_json::json!({}));
    let run = run_worker(&config, &request).await.expect("worker run");
    assert_eq!(run.output["ok"], true);
    assert!(
        run.stderr.contains("user print must not pollute protocol"),
        "stderr tail should contain user print, got {} bytes",
        run.stderr.len()
    );
    assert!(run.stderr.len() <= 8 * 1024);
}
