//! crate 内部测试辅助（仅 `cfg(test)` 编译）：可注入的 [`HarnessCtx`] 实现。
//!
//! 生产路径唯一实现是 `zgraph_kernel::NodeContext`；单测用本模块避免构造内核私有状态。

#![allow(dead_code)]

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use serde_json::Value;
use zgraph_kernel::StreamingEvent;

use crate::ctx::HarnessCtx;

/// 可注入上下文：记录全部 streaming 事件、可设取消/剩余 deadline。
#[derive(Debug)]
pub struct TestCtx {
    events: Mutex<Vec<StreamingEvent>>,
    cancel: AtomicBool,
    remaining_ms: AtomicU64,
    user_id: String,
    instance_id: String,
    seed: u64,
    vars: HashMap<String, Value>,
}

impl Default for TestCtx {
    fn default() -> Self {
        Self {
            events: Mutex::new(Vec::new()),
            cancel: AtomicBool::new(false),
            remaining_ms: AtomicU64::new(180_000),
            user_id: "test-user".to_string(),
            instance_id: "test-instance".to_string(),
            seed: 42,
            vars: HashMap::new(),
        }
    }
}

impl TestCtx {
    /// 默认上下文（剩余 deadline 180s，未取消）。
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    /// 指定剩余 deadline（毫秒）。
    pub fn with_remaining_ms(remaining_ms: u64) -> Arc<Self> {
        let ctx = Self::new();
        ctx.remaining_ms.store(remaining_ms, Ordering::SeqCst);
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
}
