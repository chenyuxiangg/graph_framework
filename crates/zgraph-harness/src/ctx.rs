//! Harness 与框架的上下文 seam（05 §5.1.1 H1；architecture-v2 §8 D2）。
//!
//! 生产路径唯一实现是 [`zgraph_kernel::NodeContext`]；测试可注入自有实现，避免
//! 为了单测去构造内核私有状态。本模块**不**触碰 `RunContext`/调度器。

use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use serde_json::Value;
use zgraph_kernel::{NodeContext, StreamingEvent};

use crate::constants::CANCEL_POLL_INTERVAL_MS;
use crate::llm::{LlmDelta, LlmStreamSink};

/// Harness 可见的节点上下文（05 §5.1.1 H1 的**最窄**接口面）。
///
/// v2 偏差（D2）：跨进程 Python SDK 只暴露 `emit/log/get_var/user_id/
/// cancel_requested/check_cancelled/deadline_remaining_ms`；Rust 侧 Harness 也只用
/// 这一份子集——没有 `CancellationToken`（用轮询），没有 `RunContext`。
pub trait HarnessCtx: Send + Sync {
    /// 投递一个**可丢** streaming 事件（不阻塞、不 panic）。
    fn emit(&self, event: StreamingEvent);
    /// 是否已请求取消（轮询语义；`Acquire` 快照，非同步点）。
    fn cancel_requested(&self) -> bool;
    /// 实例 deadline 剩余毫秒（已过期返回 0）。
    fn node_deadline_remaining_ms(&self) -> u64;
    /// 当前用户 ID（只读快照）。
    fn user_id(&self) -> &str;
    /// 读取 run 级全局变量快照；键不存在返回 `None`。
    fn get_var(&self, key: &str) -> Option<&Value>;
    /// 确定性种子（run 级，D5）。
    fn seed(&self) -> u64;
    /// 本次执行实例 ID（只读身份）。
    fn instance_id(&self) -> &str;
}

impl HarnessCtx for NodeContext {
    fn emit(&self, event: StreamingEvent) {
        NodeContext::emit(self, event);
    }

    fn cancel_requested(&self) -> bool {
        NodeContext::cancel_requested(self)
    }

    fn node_deadline_remaining_ms(&self) -> u64 {
        NodeContext::node_deadline_remaining_ms(self)
    }

    fn user_id(&self) -> &str {
        NodeContext::user_id(self)
    }

    fn get_var(&self, key: &str) -> Option<&Value> {
        NodeContext::get_var(self, key)
    }

    fn seed(&self) -> u64 {
        NodeContext::seed(self)
    }

    fn instance_id(&self) -> &str {
        NodeContext::instance_id(self)
    }
}

/// 取消的只读句柄（v2：`NodeContext.cancel_requested()` 轮询的包装）。
///
/// 不伪造 `cancelled().await` 语义：等待方由 [`wait_cancelled`] 以
/// [`CANCEL_POLL_INTERVAL_MS`] 间隔轮询（每次 LLM chunk / 每个 delta / genai
/// `select!` 分支都会检查）。
#[derive(Clone)]
pub struct CancelToken {
    ctx: Arc<dyn HarnessCtx>,
}

impl CancelToken {
    /// 由 Harness 上下文创建（生产：`Arc<NodeContext>`）。
    pub fn new(ctx: Arc<dyn HarnessCtx>) -> Self {
        Self { ctx }
    }

    /// 是否已请求取消。
    pub fn is_cancelled(&self) -> bool {
        self.ctx.cancel_requested()
    }
}

impl fmt::Debug for CancelToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CancelToken")
            .field("cancelled", &self.is_cancelled())
            .finish()
    }
}

/// 轮询取消：完成即表示已请求取消（最多约 [`CANCEL_POLL_INTERVAL_MS`] 延迟）。
pub(crate) async fn wait_cancelled(ctx: &Arc<dyn HarnessCtx>) {
    loop {
        if ctx.cancel_requested() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(CANCEL_POLL_INTERVAL_MS)).await;
    }
}

/// LLM 增量 → `StreamingEvent` 的 1:1 转发 sink（05 §5.8；不聚合、不阻塞）。
///
/// 同时记录已转发的增量数，供 Harness 在自身超时兜底时判定 `LlmPhase`
/// （`streamed_tokens > 0` 即视为已开始输出，05 §5.3.2）。
pub(crate) struct StreamSink {
    ctx: Arc<dyn HarnessCtx>,
    streamed: u32,
}

impl StreamSink {
    /// 绑定上下文。
    pub(crate) fn new(ctx: Arc<dyn HarnessCtx>) -> Self {
        Self { ctx, streamed: 0 }
    }

    /// 已转发到 `ctx.emit` 的增量数。
    pub(crate) fn streamed(&self) -> u32 {
        self.streamed
    }

    /// 是否已经交付过正文/推理增量（决定 `LlmPhase`）。
    pub(crate) fn started(&self) -> bool {
        self.streamed > 0
    }
}

impl LlmStreamSink for StreamSink {
    fn on_delta(&mut self, delta: LlmDelta) {
        match delta {
            LlmDelta::Token { text } => {
                self.ctx.emit(StreamingEvent::TokenDelta { text });
                self.streamed = self.streamed.saturating_add(1);
            }
            LlmDelta::Reasoning { text } => {
                self.ctx.emit(StreamingEvent::ReasoningDelta { text });
                self.streamed = self.streamed.saturating_add(1);
            }
            // tool_call 增量只供 adapter 内部聚合观察，不直接外发（05 §5.8.1）。
            LlmDelta::ToolCallStarted { .. } | LlmDelta::ToolCallArgs { .. } => {}
        }
    }
}
