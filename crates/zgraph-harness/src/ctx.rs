//! Harness 与框架的上下文 seam（05 §5.1.1 H1；architecture-v2 §8 D2）。
//!
//! 生产路径唯一实现是 [`zgraph_kernel::NodeContext`]；测试可注入自有实现，避免
//! 为了单测去构造内核私有状态。本模块**不**触碰 `RunContext`/调度器。

use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use serde_json::Value;
use zgraph_kernel::{NodeContext, StreamingEvent};
use zgraph_worker::WorkerLimits;

use crate::constants::CANCEL_POLL_INTERVAL_MS;
use crate::llm::{LlmDelta, LlmStreamSink};

/// `HarnessCtx::run_id()` 的默认值（未覆写实现的只读身份；仅作每 run 上限的兜底桶）。
const UNKNOWN_RUN_ID: &str = "unknown";

/// Harness 可见的节点上下文（05 §5.1.1 H1 的**最窄**接口面）。
///
/// v2 偏差（D2）：跨进程 Python SDK 只暴露 `emit/log/get_var/user_id/
/// cancel_requested/check_cancelled/deadline_remaining_ms`；Rust 侧 Harness 也只用
/// 这一份子集——没有 `CancellationToken`（用轮询），没有 `RunContext`。
///
/// ISSUE-079（DR-003 D7）：新增 `run_id` 与可选共享并发上限 `tool_limits`——
/// 工具调用与 `WorkerExecutor` 节点执行使用同一 [`WorkerLimits`] 实例，合计不超
/// 全局 / 每 run 上限。两者均有默认实现，既有 impl 不受影响。
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
    /// 所属 run ID（ISSUE-079：每 run 并发上限的键；默认 `"unknown"`）。
    fn run_id(&self) -> &str {
        UNKNOWN_RUN_ID
    }
    /// CLI 组合根注入的共享并发上限；`None` = 使用 Harness 进程级默认上限。
    fn tool_limits(&self) -> Option<Arc<WorkerLimits>> {
        None
    }

    /// D5b1：当前墙钟时刻（RFC3339 毫秒 Z；工具调用记录 `started_at/finished_at`）。
    ///
    /// 上下文未提供可注入时钟（DR-014 §3 的 `ctx` 时钟）；默认实现取系统 UTC。
    /// **无新增依赖**：RFC3339 由 std `SystemTime` + 公历换算生成（测试覆盖格式）。
    fn now_rfc3339_millis(&self) -> String {
        rfc3339_millis_now()
    }
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

    fn run_id(&self) -> &str {
        NodeContext::run_id(self)
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

/// 当前 UTC 时刻的 RFC3339 毫秒表示（`YYYY-MM-DDTHH:MM:SS.mmmZ`）。
pub(crate) fn rfc3339_millis_now() -> String {
    let duration = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    rfc3339_millis_from_epoch(
        i64::try_from(duration.as_secs()).unwrap_or(i64::MAX),
        duration.subsec_millis(),
    )
}

/// 由 Unix 秒 + 毫秒格式化 RFC3339（合入前系统时间早于 epoch 时按 0 处理）。
pub(crate) fn rfc3339_millis_from_epoch(seconds: i64, millis: u32) -> String {
    let seconds = seconds.max(0);
    let millis = millis.min(999);
    let days = seconds.div_euclid(86_400);
    let seconds_of_day = seconds.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    let hour = seconds_of_day / 3_600;
    let minute = (seconds_of_day % 3_600) / 60;
    let second = seconds_of_day % 60;
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}.{millis:03}Z")
}

/// Howard Hinnant `civil_from_days`：Unix 天数 → (year, month, day)。
fn civil_from_days(days_since_epoch: i64) -> (i64, u32, u32) {
    let z = days_since_epoch + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let day_of_era = (z - era * 146_097) as u64; // [0, 146096]
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era as i64 + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = (day_of_year - (153 * month_prime + 2) / 5 + 1) as u32;
    let month = if month_prime < 10 {
        month_prime + 3
    } else {
        month_prime - 9
    } as u32;
    let year = if month <= 2 { year + 1 } else { year };
    (year, month, day)
}

#[cfg(test)]
mod rfc3339_tests {
    use super::*;

    #[test]
    fn formats_epoch_and_known_dates() {
        assert_eq!(rfc3339_millis_from_epoch(0, 0), "1970-01-01T00:00:00.000Z");
        assert_eq!(
            rfc3339_millis_from_epoch(1_709_164_800, 42), // 2024-02-29T00:00:00Z
            "2024-02-29T00:00:00.042Z"
        );
        assert_eq!(
            rfc3339_millis_from_epoch(1_700_000_000, 999),
            "2023-11-14T22:13:20.999Z"
        );
    }

    #[test]
    fn now_is_rfc3339_millis_utc() {
        let text = rfc3339_millis_now();
        assert_eq!(text.len(), 24, "{text}");
        assert!(text.ends_with('Z'));
        assert_eq!(&text[10..11], "T");
        assert_eq!(&text[19..20], ".");
    }
}
