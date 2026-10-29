//! 节点运行时上下文 `NodeContext`（04 §4.2 / 偏差 D2）。
//!
//! 设计约束（**安全边界**）：`NodeContext` 只含"不可变快照 + 原子标志 + 可丢
//! streaming 通道"，在类型上**不可能**触达 `RunState`/buffer/状态机，
//! 因此节点无法破坏调度器不变量（I1–I9）。事件循环独占 `RunState`（03 §3.1）。
//!
//! 跨进程偏差（D2）：v2 的 Python SDK 代理只暴露
//! `emit/log/get_var/user_id/cancel_requested/check_cancelled/deadline_remaining_ms`；
//! Rust 侧的 `NodeContext` 是同一语义的进程内视图（M2 供 FakeExecutor 使用）。

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;

use serde_json::Value;

use crate::trace::{Emitter, StreamingEvent};
use crate::types::{now_rfc3339, NodeError, NodeId, RunId};

/// 构造 `NodeContext` 的内部参数包（避免公开构造面；`new` 仅内核派发路径可调）。
pub(crate) struct NodeContextInit {
    pub(crate) run_id: RunId,
    pub(crate) instance_id: String,
    pub(crate) node_id: NodeId,
    pub(crate) user_id: String,
    pub(crate) global_vars: Arc<HashMap<String, Value>>,
    pub(crate) cancel: Arc<AtomicBool>,
    pub(crate) emit_dropped: Arc<AtomicU64>,
    pub(crate) emitter: Emitter,
    pub(crate) deadline: Instant,
}

/// 节点运行时上下文（`Clone` 廉价：执行任务/代理可持有副本）。
#[derive(Clone)]
pub struct NodeContext {
    run_id: RunId,
    instance_id: String,
    node_id: NodeId,
    user_id: String,
    global_vars: Arc<HashMap<String, Value>>,
    cancel: Arc<AtomicBool>,
    emit_dropped: Arc<AtomicU64>,
    emitter: Emitter,
    deadline: Instant,
}

impl NodeContext {
    /// 内核派发路径专用构造器（用户代码无法构造）。
    pub(crate) fn new(init: NodeContextInit) -> Self {
        Self {
            run_id: init.run_id,
            instance_id: init.instance_id,
            node_id: init.node_id,
            user_id: init.user_id,
            global_vars: init.global_vars,
            cancel: init.cancel,
            emit_dropped: init.emit_dropped,
            emitter: init.emitter,
            deadline: init.deadline,
        }
    }

    /// 投递一个**可丢** streaming 事件（不阻塞、不 panic、无返回值）。
    ///
    /// sink 返回 `false`（丢弃）时累加 run 级 `streaming_dropped` 计数；
    /// 该计数最终进入 `RunFinished.metrics`（I6）。
    pub fn emit(&self, event: StreamingEvent) {
        let accepted = self.emitter.streaming(
            &self.run_id,
            &self.instance_id,
            &self.node_id,
            event,
            now_rfc3339(),
        );
        if !accepted {
            self.emit_dropped.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// 当前用户 ID（run 级只读快照）。
    pub fn user_id(&self) -> &str {
        &self.user_id
    }

    /// 读取 run 级全局变量快照；键不存在返回 `None`。
    pub fn get_var(&self, key: &str) -> Option<&Value> {
        self.global_vars.get(key)
    }

    /// run 级取消标志快照（`Acquire`，非同步点）。
    pub fn cancel_requested(&self) -> bool {
        self.cancel.load(Ordering::Acquire)
    }

    /// 已请求取消 → `Err(NodeError::Cancelled)`；否则 `Ok(())`。
    ///
    /// 供长循环/分片处主动自检（04 §4.2.2）。
    pub fn check_cancelled(&self) -> Result<(), NodeError> {
        if self.cancel_requested() {
            return Err(NodeError::Cancelled);
        }
        Ok(())
    }

    /// 实例 deadline 剩余毫秒（已过期返回 0）。
    ///
    /// 供 Harness 每轮循环前预检（05 §5.3.6）；**不得**代替调度器超时判定。
    pub fn node_deadline_remaining_ms(&self) -> u64 {
        let remaining = self.deadline.saturating_duration_since(Instant::now());
        u64::try_from(remaining.as_millis()).unwrap_or(u64::MAX)
    }
}
