//! 测试 seam：`FakeMcpClient`/`FakeMcpFactory`（DR-010/AC-19 crate 级验证）。
//!
//! 覆盖：list/call、MRTR/`input_required` 结构化失败、超时、结果截断、annotations
//! 不可信（Fake 不回传 annotations，映射只读 inputSchema）、崩溃重启（首调 Closed）。

use std::collections::{BTreeMap, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use serde_json::Value;

use super::{
    McpCallOutcome, McpClient, McpClientError, McpClientErrorKind, McpClientFactory, McpServerSpec,
    McpTool,
};

/// 可编程 fake client。
pub struct FakeMcpClient {
    /// `tools/list` 返回。
    tools: Mutex<Vec<McpTool>>,
    /// 已发生的 `tools/call`（tool, args）。
    pub calls: Mutex<Vec<(String, Value)>>,
    /// 依次返回的错误/结果（空则用 default/ok）。
    outcomes: Mutex<VecDeque<Result<McpCallOutcome, McpClientError>>>,
    /// 默认成功结果。
    default_outcome: Mutex<McpCallOutcome>,
    /// 模拟调用延迟（触发外层超时）。
    delay: Duration,
    /// 首次 call 返回 Closed（触发同 run 重启一次）。
    fail_first_closed: AtomicBool,
    /// 连接次数（重启证据）。
    pub connects: AtomicU32,
    /// shutdown 调用次数。
    pub shutdowns: AtomicU32,
}

impl Default for FakeMcpClient {
    fn default() -> Self {
        Self {
            tools: Mutex::new(Vec::new()),
            calls: Mutex::new(Vec::new()),
            outcomes: Mutex::new(VecDeque::new()),
            default_outcome: Mutex::new(McpCallOutcome {
                text: String::new(),
                is_error: false,
            }),
            delay: Duration::ZERO,
            fail_first_closed: AtomicBool::new(false),
            connects: AtomicU32::new(0),
            shutdowns: AtomicU32::new(0),
        }
    }
}

impl FakeMcpClient {
    /// 空结果 client。
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    /// 设置 `tools/list` 结果。
    pub fn with_tools(self: &Arc<Self>, tools: Vec<McpTool>) -> Arc<Self> {
        *self.tools.lock().expect("tools") = tools;
        self.clone()
    }

    /// 设置默认成功结果。
    pub fn with_default_outcome(
        self: &Arc<Self>,
        text: impl Into<String>,
        is_error: bool,
    ) -> Arc<Self> {
        *self.default_outcome.lock().expect("outcome") = McpCallOutcome {
            text: text.into(),
            is_error,
        };
        self.clone()
    }

    /// 追加一次调用结果。
    pub fn push_outcome(
        self: Arc<Self>,
        outcome: Result<McpCallOutcome, McpClientError>,
    ) -> Arc<Self> {
        if let Ok(mut outcomes) = self.outcomes.lock() {
            outcomes.push_back(outcome);
        }
        self
    }

    /// 设置调用延迟。
    pub fn with_delay(self: Arc<Self>, delay: Duration) -> Arc<Self> {
        let mut client = Arc::try_unwrap(self).unwrap_or_else(|_| panic!("unique client"));
        client.delay = delay;
        Arc::new(client)
    }

    /// 首次调用返回 `Closed`（之后再成功）。
    pub fn fail_first_closed(self: Arc<Self>) -> Arc<Self> {
        self.fail_first_closed.store(true, Ordering::SeqCst);
        self
    }

    /// 取已调用次数。
    pub fn call_count(&self) -> usize {
        self.calls.lock().map(|calls| calls.len()).unwrap_or(0)
    }
}

#[async_trait]
impl McpClient for FakeMcpClient {
    async fn list_tools(&self) -> Result<Vec<McpTool>, McpClientError> {
        Ok(self.tools.lock().expect("tools").clone())
    }

    async fn call_tool(
        &self,
        tool: &str,
        args: Value,
        timeout: Duration,
        _mrtr_max_rounds: usize,
    ) -> Result<McpCallOutcome, McpClientError> {
        if let Ok(mut calls) = self.calls.lock() {
            calls.push((tool.to_string(), args));
        }
        if !self.delay.is_zero()
            && tokio::time::timeout(timeout, tokio::time::sleep(self.delay))
                .await
                .is_err()
        {
            return Err(McpClientError::new(
                McpClientErrorKind::Timeout,
                "fake MCP call timed out",
            ));
        }
        if self.fail_first_closed.swap(false, Ordering::SeqCst) {
            return Err(McpClientError::new(
                McpClientErrorKind::Closed,
                "fake transport closed (restart test)",
            ));
        }
        let mut outcomes = self.outcomes.lock().expect("outcomes");
        if let Some(outcome) = outcomes.pop_front() {
            return outcome;
        }
        Ok(self.default_outcome.lock().expect("outcome").clone())
    }

    async fn shutdown(&self) {
        self.shutdowns.fetch_add(1, Ordering::SeqCst);
    }
}

/// fake factory：按 server 名返回预置 client（未预置 → init 失败）。
#[derive(Default)]
pub struct FakeMcpFactory {
    clients: Mutex<BTreeMap<String, Arc<FakeMcpClient>>>,
}

impl FakeMcpFactory {
    /// 空工厂。
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    /// 注册 server → client。
    pub fn with_client(self: Arc<Self>, server: &str, client: Arc<FakeMcpClient>) -> Arc<Self> {
        self.clients
            .lock()
            .expect("clients")
            .insert(server.to_string(), client);
        self
    }
}

#[async_trait]
impl McpClientFactory for FakeMcpFactory {
    async fn connect(&self, spec: &McpServerSpec) -> Result<Arc<dyn McpClient>, McpClientError> {
        let clients = self.clients.lock().expect("clients");
        let Some(client) = clients.get(&spec.name) else {
            return Err(McpClientError::new(
                McpClientErrorKind::Init,
                format!("fake server '{}' is not configured", spec.name),
            ));
        };
        client.connects.fetch_add(1, Ordering::SeqCst);
        Ok(client.clone() as Arc<dyn McpClient>)
    }
}
