//! ISSUE-076 内核侧：实例超时解析优先级（spec 05 §5.2.6）与 `NodeExecutor::timeout_hint`。
//!
//! 优先级链（高 → 低）：`test-hooks.force_node_timeout_ms` > DSL `timeout_ms` >
//! 执行器 `timeout_hint`（`agent_call` 的 `AgentConfig.node_timeout_ms` 回退）>
//! `DEFAULT_NODE_TIMEOUT_MS`。
//!
//! 观测方式：`NodeContext::node_deadline_remaining_ms()` 在 `execute` 入口读取
//! 派发时解析出的 deadline 余量，避免为"默认 180s"用例真等 180 秒；另用
//! "executor 睡眠 500ms" 验证小超时确实生效（D1：TimedOut → run Failed）。

mod common;

use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use zgraph_dsl::DEFAULT_NODE_TIMEOUT_MS;
use zgraph_kernel::{InstanceStatus, NodeError, NodeExecution, NodeExecutor, RunStatus};

const HINT_MS: u64 = 80;
const DECLARED_MS: u64 = 300;

/// 记录 `execute` 入口 deadline 余量、按配置睡眠或立即返回的执行器。
struct HintExecutor {
    hint: Option<u64>,
    sleep: Duration,
    observed_remaining_ms: Mutex<Vec<u64>>,
}

impl HintExecutor {
    fn new(hint: Option<u64>, sleep: Duration) -> Self {
        Self {
            hint,
            sleep,
            observed_remaining_ms: Mutex::new(Vec::new()),
        }
    }

    /// 各次 `execute` 入口观测到的 deadline 余量（毫秒）。
    fn observed(&self) -> Vec<u64> {
        self.observed_remaining_ms
            .lock()
            .expect("observed lock")
            .clone()
    }
}

impl NodeExecutor for HintExecutor {
    fn timeout_hint(&self, _node: &zgraph_kernel::CompiledNode) -> Option<u64> {
        self.hint
    }

    fn execute<'a>(
        &'a self,
        execution: NodeExecution<'a>,
    ) -> Pin<Box<dyn Future<Output = Result<Value, NodeError>> + Send + 'a>> {
        self.observed_remaining_ms
            .lock()
            .expect("observed lock")
            .push(execution.ctx.node_deadline_remaining_ms());
        let sleep = self.sleep;
        Box::pin(async move {
            tokio::time::sleep(sleep).await;
            Ok(json!({"ok": true}))
        })
    }
}

/// 短 tick，让超时在宽松上限内被发现（D1 语义不变）。
fn quick_tick_options() -> zgraph_kernel::RunOptions {
    common::options_with(|options| options.tick_interval_ms = 10)
}

fn linear_spec_with_quick_tick(timeout_ms: Option<u64>) -> zgraph_kernel::RunSpec {
    let mut spec = match timeout_ms {
        Some(declared) => common::linear_spec_with_work_timeout(declared),
        None => common::linear_spec(),
    };
    spec.options = quick_tick_options();
    spec
}

#[tokio::test]
async fn hint_applies_when_dsl_declares_no_timeout() {
    let executor = Arc::new(HintExecutor::new(Some(HINT_MS), Duration::from_millis(500)));
    let spec = linear_spec_with_quick_tick(None);
    let started = Instant::now();
    let (outcome, sink) = common::run_with_sink(spec, executor.clone()).await;
    let elapsed = started.elapsed();

    assert!(
        elapsed < Duration::from_secs(5),
        "hint timeout must fire quickly (took {elapsed:?}, default is 180s)"
    );
    assert_eq!(outcome.status, RunStatus::Failed);
    assert_eq!(outcome.error.expect("run error").code, "NODE_TIMEOUT");
    assert_eq!(
        common::last_status(&sink, "work"),
        Some(InstanceStatus::TimedOut)
    );
    let timed_out = sink.timed_out();
    assert_eq!(timed_out.len(), 1, "NodeTimedOut event exactly once");
    assert!(
        timed_out[0].elapsed_ms >= HINT_MS / 2,
        "elapsed: {}",
        timed_out[0].elapsed_ms
    );
    // deadline = 派发时刻 + hint（不是 180s 默认值）。
    let observed = executor.observed();
    assert_eq!(observed.len(), 1, "only `work` goes through execute");
    assert!(
        observed[0] <= HINT_MS && observed[0] >= HINT_MS / 2,
        "dispatch deadline should come from the hint: {observed:?}"
    );
}

#[tokio::test]
async fn declared_timeout_beats_executor_hint() {
    let executor = Arc::new(HintExecutor::new(Some(10), Duration::from_millis(500)));
    let spec = linear_spec_with_quick_tick(Some(DECLARED_MS));
    let (outcome, sink) = common::run_with_sink(spec, executor.clone()).await;

    assert_eq!(outcome.status, RunStatus::Failed);
    assert_eq!(outcome.error.expect("run error").code, "NODE_TIMEOUT");
    let observed = executor.observed();
    assert_eq!(observed.len(), 1);
    assert!(
        observed[0] >= DECLARED_MS / 2,
        "DSL declared timeout must win over the 10ms hint: {observed:?}"
    );
    assert!(observed[0] <= DECLARED_MS, "observed: {observed:?}");
    let timed_out = sink.timed_out();
    assert_eq!(timed_out.len(), 1);
    assert!(
        timed_out[0].elapsed_ms >= DECLARED_MS / 2,
        "if the 10ms hint won, timeout would fire at ~10ms: {}",
        timed_out[0].elapsed_ms
    );
}

#[tokio::test]
async fn no_hint_and_no_declared_timeout_uses_default() {
    let executor = Arc::new(HintExecutor::new(None, Duration::ZERO));
    let spec = linear_spec_with_quick_tick(None);
    let (outcome, _sink) = common::run_with_sink(spec, executor.clone()).await;

    assert_eq!(outcome.status, RunStatus::Completed);
    let observed = executor.observed();
    assert_eq!(observed.len(), 1);
    assert!(
        observed[0] >= DEFAULT_NODE_TIMEOUT_MS - 1_000 && observed[0] <= DEFAULT_NODE_TIMEOUT_MS,
        "no hint / no DSL declaration → default {DEFAULT_NODE_TIMEOUT_MS}ms: {observed:?}"
    );
    // 既有执行器无需改动：`timeout_hint` 默认实现返回 None（对象安全 + 增量）。
    let graph = common::compile_text(common::LINEAR_JSON);
    let node = graph.nodes.get("work").expect("work node");
    assert_eq!(
        zgraph_kernel::FakeExecutor::new().timeout_hint(node),
        None,
        "default trait implementation must not provide a hint"
    );
}

/// `test-hooks.force_node_timeout_ms` 仍然最高优先级（高于 DSL 声明与 hint）。
#[cfg(feature = "test-hooks")]
#[tokio::test]
async fn force_timeout_beats_declared_and_hint() {
    use zgraph_kernel::TestHooks;

    let executor = Arc::new(HintExecutor::new(Some(10), Duration::from_millis(500)));
    let mut spec = common::linear_spec_with_work_timeout(DECLARED_MS);
    spec.options = common::options_with(|options| {
        options.tick_interval_ms = 10;
        options.with_test_hooks(TestHooks {
            force_node_timeout_ms: Some(50),
            ..TestHooks::default()
        });
    });
    let (outcome, sink) = common::run_with_sink(spec, executor.clone()).await;

    assert_eq!(outcome.status, RunStatus::Failed);
    assert_eq!(outcome.error.expect("run error").code, "NODE_TIMEOUT");
    let observed = executor.observed();
    assert_eq!(observed.len(), 1);
    assert!(observed[0] <= 50, "forced timeout must win: {observed:?}");
    let timed_out = sink.timed_out();
    assert_eq!(timed_out.len(), 1);
    assert!(
        timed_out[0].elapsed_ms < DECLARED_MS / 2,
        "forced 50ms must fire before declared {DECLARED_MS}ms: {}",
        timed_out[0].elapsed_ms
    );
}
