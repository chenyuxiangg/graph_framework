//! crate 内部测试辅助（仅 `cfg(test)` 编译）：可注入的 [`HarnessCtx`] 实现与
//! LLM 调用记录器捕获实现（ISSUE-082）。
//!
//! 生产路径唯一实现是 `zgraph_kernel::NodeContext`；单测用本模块避免构造内核私有状态。

#![allow(dead_code)]

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use serde_json::Value;
use zgraph_kernel::StreamingEvent;
use zgraph_worker::WorkerLimits;

use crate::builtin::bash::{BashOutcome, BashRequest, SandboxRunner};
use crate::builtin::sandbox::ProcessControl;
use crate::ctx::HarnessCtx;
use crate::record::{LlmCallRecorder, LlmCallSnapshot};
use crate::tools::ToolFailure;

/// 捕获到的一次 LLM 调用记录（拥有所有权的快照副本）。
#[derive(Debug, Clone, PartialEq)]
pub struct RecordedCall {
    /// 执行实例 ID。
    pub instance_id: String,
    /// 轮次内序号（0 起）。
    pub call_index: u32,
    /// 脱敏请求快照。
    pub request: Value,
    /// 响应快照。
    pub response: Option<Value>,
    /// `ok` / `error`。
    pub status: String,
}

impl RecordedCall {
    /// 请求快照的序列化文本（脱敏断言用）。
    pub fn request_text(&self) -> String {
        serde_json::to_string(&self.request).unwrap_or_default()
    }

    /// 响应快照的序列化文本。
    pub fn response_text(&self) -> String {
        self.response
            .as_ref()
            .and_then(|value| serde_json::to_string(value).ok())
            .unwrap_or_default()
    }
}

/// 捕获全部 `LlmCallRecorder` 回调的测试记录器。
#[derive(Debug, Default)]
pub struct CapturingRecorder {
    calls: Mutex<Vec<RecordedCall>>,
    panic_on_record: AtomicBool,
}

impl CapturingRecorder {
    /// 空捕获器。
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    /// 让每次记录 panic（验证 Harness 吞掉记录器 panic）。
    pub fn panicking() -> Arc<Self> {
        let recorder = Self::new();
        recorder.panic_on_record.store(true, Ordering::SeqCst);
        recorder
    }

    /// 已捕获的记录（按记录顺序）。
    pub fn calls(&self) -> Vec<RecordedCall> {
        match self.calls.lock() {
            Ok(guard) => guard.clone(),
            Err(poisoned) => poisoned.into_inner().clone(),
        }
    }

    /// 已捕获记录数。
    pub fn len(&self) -> usize {
        self.calls().len()
    }
}

impl LlmCallRecorder for CapturingRecorder {
    fn record_llm_call(&self, call: &LlmCallSnapshot<'_>) {
        if self.panic_on_record.load(Ordering::SeqCst) {
            panic!("capturing recorder panic (expected by test)");
        }
        let recorded = RecordedCall {
            instance_id: call.instance_id.to_string(),
            call_index: call.call_index,
            request: call.request.clone(),
            response: call.response.cloned(),
            status: call.status.to_string(),
        };
        match self.calls.lock() {
            Ok(mut guard) => guard.push(recorded),
            Err(poisoned) => poisoned.into_inner().push(recorded),
        }
    }
}

/// v0.4/D5b1：捕获富化工具调用记录的测试 sink（每类工具断言用）。
///
/// D5b2：同时捕获执行上下文（run/instance/node/attempt），供 harness 单测断言
/// `agent_loop` 发射时已填入。
#[derive(Debug, Default)]
pub struct RecordingToolCallSink {
    records: Mutex<Vec<crate::tools::ToolCallRecord>>,
    contexts: Mutex<Vec<crate::tools::ToolCallExecContext>>,
}

impl RecordingToolCallSink {
    /// 空记录器。
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    /// 已捕获记录（按调用顺序）。
    pub fn records(&self) -> Vec<crate::tools::ToolCallRecord> {
        match self.records.lock() {
            Ok(guard) => guard.clone(),
            Err(poisoned) => poisoned.into_inner().clone(),
        }
    }

    /// 已捕获执行上下文（与 [`Self::records`] 同序）。
    pub fn contexts(&self) -> Vec<crate::tools::ToolCallExecContext> {
        match self.contexts.lock() {
            Ok(guard) => guard.clone(),
            Err(poisoned) => poisoned.into_inner().clone(),
        }
    }
}

impl crate::tools::ToolCallRecordSink for RecordingToolCallSink {
    fn record_tool_call(
        &self,
        context: &crate::tools::ToolCallExecContext,
        record: &crate::tools::ToolCallRecord,
    ) {
        match (self.contexts.lock(), self.records.lock()) {
            (Ok(mut contexts), Ok(mut records)) => {
                contexts.push(context.clone());
                records.push(record.clone());
            }
            (Err(poisoned), Ok(mut records)) => {
                poisoned.into_inner().push(context.clone());
                records.push(record.clone());
            }
            (Ok(mut contexts), Err(poisoned)) => {
                contexts.push(context.clone());
                poisoned.into_inner().push(record.clone());
            }
            (Err(poisoned_ctx), Err(poisoned_rec)) => {
                poisoned_ctx.into_inner().push(context.clone());
                poisoned_rec.into_inner().push(record.clone());
            }
        }
    }
}

/// REV-110 C2：`TestCtx` 动态 deadline provider（`Mutex` 内含非 Debug 闭包）。
#[derive(Default)]
pub struct RemainingProvider(pub Mutex<Option<Arc<dyn Fn() -> u64 + Send + Sync>>>);

impl std::fmt::Debug for RemainingProvider {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RemainingProvider(..)")
    }
}

/// 可注入上下文：记录全部 streaming 事件、可设取消/剩余 deadline。
#[derive(Debug)]
pub struct TestCtx {
    events: Mutex<Vec<StreamingEvent>>,
    cancel: AtomicBool,
    remaining_ms: AtomicU64,
    /// REV-110 C2 测试：可注入的剩余 deadline provider（优先于 `remaining_ms`）。
    remaining_provider: RemainingProvider,
    user_id: String,
    instance_id: String,
    seed: u64,
    vars: HashMap<String, Value>,
    run_id: String,
    node_id: String,
    attempt: u32,
    tool_limits: Option<Arc<WorkerLimits>>,
}

impl Default for TestCtx {
    fn default() -> Self {
        Self {
            events: Mutex::new(Vec::new()),
            cancel: AtomicBool::new(false),
            remaining_ms: AtomicU64::new(180_000),
            remaining_provider: RemainingProvider::default(),
            user_id: "test-user".to_string(),
            instance_id: "test-instance".to_string(),
            seed: 42,
            vars: HashMap::new(),
            run_id: "test-run".to_string(),
            node_id: "test-node".to_string(),
            attempt: 0,
            tool_limits: None,
        }
    }
}

impl TestCtx {
    /// 默认上下文（剩余 deadline 180s，未取消）。
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    /// ISSUE-079：指定 run ID（每 run 并发上限的键）。
    pub fn with_run_id(run_id: &str) -> Arc<Self> {
        Arc::new(Self {
            run_id: run_id.to_string(),
            ..Self::default()
        })
    }

    /// ISSUE-079：注入共享并发上限（应与 `WorkerExecutor` 使用同一实例）。
    pub fn with_limits(run_id: &str, limits: Arc<WorkerLimits>) -> Arc<Self> {
        Arc::new(Self {
            run_id: run_id.to_string(),
            tool_limits: Some(limits),
            ..Self::default()
        })
    }

    /// D5b2：指定执行身份（instance/node/attempt），供工具调用记录上下文断言。
    pub fn with_node_identity(instance_id: &str, node_id: &str, attempt: u32) -> Arc<Self> {
        Arc::new(Self {
            instance_id: instance_id.to_string(),
            node_id: node_id.to_string(),
            attempt,
            ..Self::default()
        })
    }

    /// 指定剩余 deadline（毫秒）。
    pub fn with_remaining_ms(remaining_ms: u64) -> Arc<Self> {
        let ctx = Self::new();
        ctx.remaining_ms.store(remaining_ms, Ordering::SeqCst);
        ctx
    }

    /// REV-110 C2：注入动态剩余 deadline provider（工具执行中可变；返回毫秒）。
    pub fn with_remaining_provider(provider: Arc<dyn Fn() -> u64 + Send + Sync>) -> Arc<Self> {
        let ctx = Arc::new(Self::default());
        if let Ok(mut guard) = ctx.remaining_provider.0.lock() {
            *guard = Some(provider);
        }
        ctx
    }

    /// 置位取消标志。
    pub fn request_cancel(&self) {
        self.cancel.store(true, Ordering::SeqCst);
    }

    /// 修改剩余 deadline。
    pub fn set_remaining_ms(&self, remaining_ms: u64) {
        self.remaining_ms.store(remaining_ms, Ordering::SeqCst);
    }

    /// 已记录的全部 streaming 事件。
    pub fn events(&self) -> Vec<StreamingEvent> {
        match self.events.lock() {
            Ok(guard) => guard.clone(),
            Err(poisoned) => poisoned.into_inner().clone(),
        }
    }

    /// 已记录事件数。
    pub fn event_count(&self) -> usize {
        match self.events.lock() {
            Ok(guard) => guard.len(),
            Err(poisoned) => poisoned.into_inner().len(),
        }
    }

    /// 拼接全部 `TokenDelta` 文本（05 §5.8 不变量 A2 断言用）。
    pub fn token_text(&self) -> String {
        self.events()
            .into_iter()
            .filter_map(|event| match event {
                StreamingEvent::TokenDelta { text } => Some(text),
                _ => None,
            })
            .collect()
    }

    /// 供 `Arc<TestCtx>` 转 `Arc<dyn HarnessCtx>` 的便捷方法。
    pub fn into_dyn(self: Arc<Self>) -> Arc<dyn HarnessCtx> {
        self
    }
}

impl HarnessCtx for TestCtx {
    fn emit(&self, event: StreamingEvent) {
        match self.events.lock() {
            Ok(mut guard) => guard.push(event),
            Err(poisoned) => poisoned.into_inner().push(event),
        }
    }

    fn cancel_requested(&self) -> bool {
        self.cancel.load(Ordering::SeqCst)
    }

    fn node_deadline_remaining_ms(&self) -> u64 {
        if let Ok(guard) = self.remaining_provider.0.lock() {
            if let Some(provider) = guard.as_ref() {
                return provider();
            }
        }
        self.remaining_ms.load(Ordering::SeqCst)
    }

    fn user_id(&self) -> &str {
        &self.user_id
    }

    fn get_var(&self, key: &str) -> Option<&Value> {
        self.vars.get(key)
    }

    fn seed(&self) -> u64 {
        self.seed
    }

    fn instance_id(&self) -> &str {
        &self.instance_id
    }

    fn run_id(&self) -> &str {
        &self.run_id
    }

    fn node_id(&self) -> &str {
        &self.node_id
    }

    fn attempt(&self) -> u32 {
        self.attempt
    }

    fn tool_limits(&self) -> Option<Arc<WorkerLimits>> {
        self.tool_limits.clone()
    }
}

/// v0.4/D4：Fake 沙箱执行器（记录请求、返回脚本化结果；不启动真实 sandbox-exec）。
#[derive(Debug, Default)]
pub struct FakeSandboxRunner {
    outcomes: Mutex<std::collections::VecDeque<Result<BashOutcome, ToolFailure>>>,
    requests: Mutex<Vec<BashRequest>>,
    profile_sha256: String,
}

impl FakeSandboxRunner {
    /// 空 Fake（默认返回 exit 0、空输出）。
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            profile_sha256: "f".repeat(64),
            ..Self::default()
        })
    }

    /// 追加一个脚本化结果（按调用顺序消费）。
    pub fn push_outcome(&self, outcome: Result<BashOutcome, ToolFailure>) -> &Self {
        if let Ok(mut guard) = self.outcomes.lock() {
            guard.push_back(outcome);
        }
        self
    }

    /// 已记录的请求（按调用顺序）。
    pub fn requests(&self) -> Vec<BashRequest> {
        match self.requests.lock() {
            Ok(guard) => guard.clone(),
            Err(poisoned) => poisoned.into_inner().clone(),
        }
    }
}

impl SandboxRunner for FakeSandboxRunner {
    fn run(
        &self,
        request: &BashRequest,
        _control: &ProcessControl,
    ) -> Result<BashOutcome, ToolFailure> {
        if let Ok(mut guard) = self.requests.lock() {
            guard.push(request.clone());
        }
        match self.outcomes.lock() {
            Ok(mut guard) => guard.pop_front().unwrap_or_else(|| {
                Ok(BashOutcome {
                    exit_code: Some(0),
                    stdout: b"ok".to_vec(),
                    ..BashOutcome::default()
                })
            }),
            Err(poisoned) => {
                let mut guard = poisoned.into_inner();
                guard.pop_front().unwrap_or_else(|| {
                    Ok(BashOutcome {
                        exit_code: Some(0),
                        stdout: b"ok".to_vec(),
                        ..BashOutcome::default()
                    })
                })
            }
        }
    }

    fn profile_sha256(&self) -> &str {
        &self.profile_sha256
    }
}

/// v0.4/D4：会阻塞到被取消的 Fake 沙箱执行器（端到端取消测试用）。
#[derive(Debug, Default)]
pub struct BlockingSandboxRunner {
    started: std::sync::atomic::AtomicBool,
    profile_sha256: String,
}

impl BlockingSandboxRunner {
    /// 新建（记录是否已开始执行）。
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            profile_sha256: "b".repeat(64),
            ..Self::default()
        })
    }

    /// 是否已进入 run（说明 agent_loop 真的派发了 bash）。
    pub fn started(&self) -> bool {
        self.started.load(std::sync::atomic::Ordering::SeqCst)
    }
}

impl SandboxRunner for BlockingSandboxRunner {
    fn run(
        &self,
        _request: &BashRequest,
        control: &ProcessControl,
    ) -> Result<BashOutcome, ToolFailure> {
        self.started
            .store(true, std::sync::atomic::Ordering::SeqCst);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while !control.is_killed() {
            if std::time::Instant::now() >= deadline {
                return Err(ToolFailure::new(
                    "timeout",
                    "blocking fake runner timed out",
                ));
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        Err(ToolFailure::new(
            "cancelled",
            "blocking fake runner cancelled",
        ))
    }

    fn profile_sha256(&self) -> &str {
        &self.profile_sha256
    }
}

/// D5b1 测试辅助：大输出 bash 结果（触发 16 KiB head+tail 截断）。
pub fn bash_outcome_with_large_output() -> BashOutcome {
    BashOutcome {
        exit_code: Some(0),
        signal: None,
        stdout: "a".repeat(100_000).into_bytes(),
        stderr: "b".repeat(50_000).into_bytes(),
        drain_truncated: false,
        duration_ms: 1,
    }
}

/// REV-110 N2：record 时 panic 的 sink（断言 `record_tool_call_safely` 吞 panic）。
#[derive(Debug, Default)]
pub struct PanickingToolCallSink;

impl PanickingToolCallSink {
    /// 新建。
    pub fn new() -> Arc<Self> {
        Arc::new(Self)
    }
}

impl crate::tools::ToolCallRecordSink for PanickingToolCallSink {
    fn record_tool_call(
        &self,
        _context: &crate::tools::ToolCallExecContext,
        _record: &crate::tools::ToolCallRecord,
    ) {
        panic!("sink panic must not affect the run");
    }
}
