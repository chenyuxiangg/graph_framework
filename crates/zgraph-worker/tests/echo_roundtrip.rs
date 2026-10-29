//! M0 验收：Rust supervisor ↔ Python worker 的 NDJSON 往返。
//!
//! 依赖 `python/` 下的 worker SDK；解释器可用 `ZGRAPH_PYTHON` 覆盖，默认 `python3`。
//! 在 CI 中通过 `uv run` 提供环境；本地无 uv 时直接使用系统 python3（需标准库即可）。

use std::path::PathBuf;
use std::time::Duration;

use zgraph_worker::{run_worker, sample_request, WorkerConfig, WorkerFailure};

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("repo root")
}

fn worker_config(timeout: Duration) -> WorkerConfig {
    let root = repo_root();
    WorkerConfig {
        program: std::env::var("ZGRAPH_PYTHON").unwrap_or_else(|_| "python3".to_string()),
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
async fn worker_timeout_kills_process() {
    let mut config = worker_config(Duration::from_millis(600));
    config.args = vec!["-m".to_string(), "zgraph_worker.runtime".to_string()];
    let request = sample_request(
        "zgraph_worker.slow",
        "run",
        serde_json::json!({"seconds": 5}),
    );
    let err = run_worker(&config, &request)
        .await
        .expect_err("must timeout");
    assert!(
        matches!(err, WorkerFailure::Timeout(_)),
        "expected timeout, got {err:?}"
    );
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
