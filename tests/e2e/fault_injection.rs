//! M3 故障注入（真 worker 子进程）：内核错误码映射 + trace 落库。
//!
//! 语义基准：`doc/spec-v1.2/07-验收用例.md` AC-04（附加：`NODE_WORKER_IMPORT`）/
//! AC-05（异常）与 `doc/architecture-v2.md` §8.2 的 4 个 worker 错误码 + D10：
//!
//! | 故障 | 注入方式 | 期望 |
//! |---|---|---|
//! | 未捕获异常 | SDK worker（`e2e_fixtures.faults.value_error`） | `NODE_PYTHON_EXCEPTION` |
//! | 导入失败 | 图指向不存在的 module | `NODE_WORKER_IMPORT`（D7：运行期而非加载期） |
//! | exit 0 无 result | raw NDJSON `no_result.py` | `NODE_WORKER_PROTOCOL` |
//! | 重复 result | raw NDJSON `duplicate_result.py` | `NODE_WORKER_PROTOCOL` |
//! | result 后不退出 | raw NDJSON `result_no_exit.py` | `NODE_WORKER_PROTOCOL` |
//! | 被信号杀死 | SDK worker（`os.kill(SIGKILL)`） | `NODE_WORKER_CRASH` |
//! | hello 超时 | raw NDJSON `hello_timeout.py` + 短 hello_timeout | `NODE_WORKER_HANDSHAKE` |
//!
//! 每个用例都断言 **内核 outcome** 与 **SQLite trace**（runs.error_code、
//! instances.status/error_code、RunFinished 恰一次）两侧一致，避免只看返回值。
//! raw NDJSON fixture 在 `tests/e2e/fixtures/workers/` 自建（不改 `crates/**`）。

#![cfg(unix)]

#[path = "common/mod.rs"]
mod common;

use std::sync::Arc;
use std::time::Duration;

use serde_json::json;

use zgraph_cli::bridge::BridgeOutcome;
use zgraph_kernel::{CompiledGraph, RunStatus};
use zgraph_trace::InstanceStatus as TraceInstanceStatus;
use zgraph_worker::WorkerExecutor;

/// hello 超时用例的 supervisor 握手上限（默认 5s 太慢，故障注入要快）。
const HELLO_TIMEOUT: Duration = Duration::from_millis(300);

/// 用给定执行器跑「线性真 worker 图」，返回 bridge outcome + reader。
///
/// raw NDJSON 脚本自己实现协议循环，**忽略** run 载荷里的 module/function；
/// 图只提供 `start -> work -> done` 形状与实例 id（错误码由 supervisor/executor 决定）。
async fn run_fault(
    dir: &tempfile::TempDir,
    name: &str,
    graph: Arc<CompiledGraph>,
    executor: Arc<WorkerExecutor>,
) -> BridgeOutcome {
    let db = dir.path().join(name);
    let inputs = common::entry_inputs(&[("start", "x", json!({"probe": true}))]);
    let spec = common::spec(graph, inputs);
    tokio::time::timeout(
        common::RUN_TIMEOUT,
        common::run_to_trace(spec, executor, &db),
    )
    .await
    .expect("fault run must finish")
}

/// 断言故障 run 的 outcome 与 trace 双侧一致：run Failed + 错误码 + work 实例 Crashed。
fn assert_fault(outcome: &BridgeOutcome, expected_code: &str) {
    assert_eq!(
        outcome.outcome.status,
        RunStatus::Failed,
        "fault should fail run: {:?}",
        outcome.outcome.error
    );
    let error = outcome.outcome.error.as_ref().expect("failed run error");
    assert_eq!(error.code, expected_code, "unexpected run error: {error:?}");

    let reader = outcome.reader();
    let run_id = outcome.outcome.run_id.as_str();
    common::assert_lifecycle_shape(reader, run_id);
    let run = common::run_row(reader, run_id);
    assert_eq!(run.status, zgraph_trace::RunStatus::Failed);
    assert_eq!(
        run.error_code.as_deref(),
        Some(expected_code),
        "trace runs.error_code"
    );

    let work = common::instance(reader, run_id, "work");
    assert_eq!(
        work.status,
        TraceInstanceStatus::Crashed,
        "worker 故障应使实例 Crashed"
    );
    assert_eq!(
        work.error_code.as_deref(),
        Some(expected_code),
        "trace instances.error_code"
    );

    let all = common::events(reader, run_id);
    assert_eq!(common::count_kind(&all, "RunFinished"), 1);
    assert_eq!(common::count_kind(&all, "NodeTimedOut"), 0);
    let finished = common::run_finished_payload(reader, run_id);
    assert_eq!(finished["status"], "Failed");
    assert_eq!(finished["error"]["code"], expected_code);
}

/// `work` 实例的 `NodeError` JSON（bridge 逐变体展开）。
fn work_error(outcome: &BridgeOutcome) -> serde_json::Value {
    common::instance(outcome.reader(), outcome.outcome.run_id.as_str(), "work")
        .error
        .expect("instance error json")
}

#[tokio::test]
async fn python_exception_maps_to_node_python_exception() {
    let dir = tempfile::tempdir().expect("tempdir");
    let outcome = run_fault(
        &dir,
        "python_exception.db",
        common::linear_graph("e2e_fixtures.faults", "value_error", None),
        common::executor(common::RUN_TIMEOUT),
    )
    .await;
    assert_fault(&outcome, "NODE_PYTHON_EXCEPTION");

    let error = work_error(&outcome);
    assert_eq!(error["kind"], "PythonException");
    let traceback = error["traceback"].as_str().expect("traceback string");
    assert!(
        traceback.contains("ValueError"),
        "traceback 应保留异常类型: {traceback}"
    );
    assert!(
        error["exception"]
            .as_str()
            .is_some_and(|message| message.contains("boom-e2e")),
        "exception 摘要: {error}"
    );
}

#[tokio::test]
async fn import_failure_maps_to_node_worker_import() {
    let dir = tempfile::tempdir().expect("tempdir");
    // D7：图加载期不 import 用户 Python，导入失败在 worker 启动时映射 NODE_WORKER_IMPORT。
    let outcome = run_fault(
        &dir,
        "import_failure.db",
        common::linear_graph("no_such_module_e2e", "run", None),
        common::executor(common::RUN_TIMEOUT),
    )
    .await;
    assert_fault(&outcome, "NODE_WORKER_IMPORT");
    assert_eq!(work_error(&outcome)["kind"], "NodeWorker");
}

#[tokio::test]
async fn exit_zero_without_result_maps_to_node_worker_protocol() {
    let dir = tempfile::tempdir().expect("tempdir");
    let outcome = run_fault(
        &dir,
        "no_result.db",
        common::linear_graph("e2e_fixtures.faults", "value_error", None),
        common::script_executor("no_result.py", common::RUN_TIMEOUT, Duration::from_secs(5)),
    )
    .await;
    assert_fault(&outcome, "NODE_WORKER_PROTOCOL");
}

#[tokio::test]
async fn duplicate_result_maps_to_node_worker_protocol() {
    let dir = tempfile::tempdir().expect("tempdir");
    let outcome = run_fault(
        &dir,
        "duplicate_result.db",
        common::linear_graph("e2e_fixtures.faults", "value_error", None),
        common::script_executor(
            "duplicate_result.py",
            common::RUN_TIMEOUT,
            Duration::from_secs(5),
        ),
    )
    .await;
    assert_fault(&outcome, "NODE_WORKER_PROTOCOL");
}

#[tokio::test]
async fn result_without_exit_maps_to_node_worker_protocol() {
    let dir = tempfile::tempdir().expect("tempdir");
    let outcome = run_fault(
        &dir,
        "result_no_exit.db",
        common::linear_graph("e2e_fixtures.faults", "value_error", None),
        common::script_executor(
            "result_no_exit.py",
            common::RUN_TIMEOUT,
            Duration::from_secs(5),
        ),
    )
    .await;
    assert_fault(&outcome, "NODE_WORKER_PROTOCOL");
}

#[tokio::test]
async fn signal_death_maps_to_node_worker_crash() {
    let dir = tempfile::tempdir().expect("tempdir");
    let outcome = run_fault(
        &dir,
        "signal_death.db",
        common::linear_graph("e2e_fixtures.faults", "kill_self", None),
        common::executor(common::RUN_TIMEOUT),
    )
    .await;
    assert_fault(&outcome, "NODE_WORKER_CRASH");
    assert_eq!(work_error(&outcome)["kind"], "NodeWorker");
}

#[tokio::test]
async fn hello_timeout_maps_to_node_worker_handshake() {
    let dir = tempfile::tempdir().expect("tempdir");
    let outcome = run_fault(
        &dir,
        "hello_timeout.db",
        common::linear_graph("e2e_fixtures.faults", "value_error", None),
        common::script_executor("hello_timeout.py", common::RUN_TIMEOUT, HELLO_TIMEOUT),
    )
    .await;
    assert_fault(&outcome, "NODE_WORKER_HANDSHAKE");
}
