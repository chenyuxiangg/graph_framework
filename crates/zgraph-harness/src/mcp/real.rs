//! 真实 MCP 客户端（rmcp 3.8.0；`feature = "mcp"`）。
//!
//! - **双 era**：`ClientLifecycleMode::Auto`：先按 `2026-07-28` 无状态语义探测，
//!   失败/legacy 响应 → 回落 `2025-11-25` initialize 握手（rmcp 原生支持，DR-010 §1）。
//! - **stdio**：`process-wrap` 进程组 + stderr 持续读取（≤8192，不写 trace）。
//! - **HTTP**：Streamable HTTP（reqwest；16 并发 / control 5s；https-only 由 registry 校验）。
//! - **超时/MRTR**：外层 `tokio::time::timeout` 包 `tools/call`；MRTR 轮次上限 10；
//!   `input_required` 无法完成 → 结构化失败 `input_required_unsupported`。

use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use rmcp::model::{
    CallToolRequestParams, CallToolResult, ClientCapabilities, ClientConfig, ContentBlock,
    Implementation, ProtocolVersion, Tool as RmcpTool,
};
use rmcp::service::RunningService;
use rmcp::transport::child_process::TokioChildProcess;
use rmcp::transport::streamable_http_client::StreamableHttpClientTransportConfig;
use rmcp::transport::StreamableHttpClientTransport;
use rmcp::{ClientHandler, ClientLifecycleMode, ClientServiceExt, RoleClient, ServiceError};
use serde_json::Value;
use tokio::io::AsyncReadExt;

use super::{
    McpCallOutcome, McpClient, McpClientError, McpClientErrorKind, McpClientFactory, McpServerSpec,
    McpTool, McpTransport,
};
use crate::constants::{
    MCP_CONTROL_REQUEST_TIMEOUT_MS, MCP_HTTP_MAX_CONCURRENCY, MCP_INIT_TIMEOUT_MS,
    MCP_STDERR_MAX_BYTES,
};

/// 真实客户端工厂（stdio / Streamable HTTP）。
#[derive(Debug, Default, Clone, Copy)]
pub struct RealMcpFactory;

/// 客户端 service 实现（只提供 clientInfo；v0.3 不声明 elicitation/sampling/roots）。
#[derive(Clone, Copy, Debug, Default)]
struct ZgraphMcpClient;

impl ClientHandler for ZgraphMcpClient {
    fn get_info(&self) -> ClientConfig {
        ClientConfig::new(
            ClientCapabilities::default(),
            Implementation::new("zgraph-harness", env!("CARGO_PKG_VERSION")),
        )
    }
}

struct RealMcpClient {
    service: tokio::sync::Mutex<Option<RunningService<RoleClient, ZgraphMcpClient>>>,
    stderr: Arc<Mutex<String>>,
}

impl RealMcpClient {
    fn new(
        service: RunningService<RoleClient, ZgraphMcpClient>,
        stderr: Arc<Mutex<String>>,
    ) -> Self {
        Self {
            service: tokio::sync::Mutex::new(Some(service)),
            stderr,
        }
    }

    fn map_service_error(error: &ServiceError) -> McpClientError {
        match error {
            ServiceError::InputRequiredRoundsExceeded { .. } => McpClientError::new(
                McpClientErrorKind::InputRequiredUnsupported,
                "MCP server requested input_required (unsupported in v0.3)",
            ),
            ServiceError::Timeout { .. } => {
                McpClientError::new(McpClientErrorKind::Timeout, "MCP request timed out")
            }
            ServiceError::TransportClosed => {
                McpClientError::new(McpClientErrorKind::Closed, "MCP transport closed")
            }
            ServiceError::TransportSend(_) => {
                McpClientError::new(McpClientErrorKind::Closed, "MCP transport send failed")
            }
            ServiceError::McpError(error) => {
                let reason = if error.code.0 == -32022 {
                    McpClientErrorKind::ProtocolVersion
                } else {
                    McpClientErrorKind::Other
                };
                McpClientError::new(
                    reason,
                    format!("MCP protocol error {}: {}", error.code.0, error.message),
                )
            }
            ServiceError::UnexpectedResponse => {
                McpClientError::new(McpClientErrorKind::Other, "unexpected MCP response")
            }
            ServiceError::Cancelled { .. } => {
                McpClientError::new(McpClientErrorKind::Closed, "MCP request cancelled")
            }
            ServiceError::SubscriptionLagged { .. } => {
                McpClientError::new(McpClientErrorKind::Other, "MCP subscription lagged")
            }
            _ => McpClientError::new(McpClientErrorKind::Other, "MCP client error"),
        }
    }
}

fn truncate_stderr(text: &str) -> String {
    if text.len() <= MCP_STDERR_MAX_BYTES {
        return text.to_string();
    }
    let mut end = MCP_STDERR_MAX_BYTES;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    text[..end].to_string()
}

async fn read_stderr(mut stderr: tokio::process::ChildStderr, sink: Arc<Mutex<String>>) {
    let mut buffer = [0u8; 1024];
    loop {
        match stderr.read(&mut buffer).await {
            Ok(0) | Err(_) => break,
            Ok(read) => {
                let chunk = String::from_utf8_lossy(&buffer[..read]);
                if let Ok(mut text) = sink.lock() {
                    if text.len() < MCP_STDERR_MAX_BYTES {
                        text.push_str(&chunk);
                        *text = truncate_stderr(&text);
                    }
                }
            }
        }
    }
}

#[async_trait]
impl McpClientFactory for RealMcpFactory {
    async fn connect(
        &self,
        spec: &McpServerSpec,
        assets_dir: &std::path::Path,
    ) -> Result<Arc<dyn McpClient>, McpClientError> {
        let lifecycle = ClientLifecycleMode::Auto {
            preferred_versions: vec![ProtocolVersion::V_2026_07_28],
            legacy_version: Some(ProtocolVersion::V_2025_11_25),
        };
        match &spec.transport {
            McpTransport::Stdio { command, args, env } => {
                let command = command.clone();
                let args = args.clone();
                let env = env.clone();
                let mut command = process_wrap::tokio::CommandWrap::with_new(command, |command| {
                    command.args(&args);
                    command.envs(&env);
                    command.current_dir(assets_dir);
                    command.kill_on_drop(true);
                });
                #[cfg(unix)]
                command.wrap(process_wrap::tokio::ProcessGroup::leader());
                let (transport, stderr) = TokioChildProcess::builder(command)
                    .stderr(Stdio::piped())
                    .spawn()
                    .map_err(|error| {
                        McpClientError::new(
                            McpClientErrorKind::Init,
                            format!("failed to spawn MCP stdio server: {error}"),
                        )
                    })?;
                let sink = Arc::new(Mutex::new(String::new()));
                if let Some(stderr) = stderr {
                    tokio::spawn(read_stderr(stderr, sink.clone()));
                }
                let service = tokio::time::timeout(
                    Duration::from_millis(MCP_INIT_TIMEOUT_MS),
                    ZgraphMcpClient.serve_with_lifecycle(transport, lifecycle),
                )
                .await
                .map_err(|_| {
                    McpClientError::new(McpClientErrorKind::Init, "MCP initialize timed out")
                })?
                .map_err(|error| {
                    McpClientError::new(
                        McpClientErrorKind::Init,
                        format!("MCP initialize failed: {error}"),
                    )
                })?;
                Ok(Arc::new(RealMcpClient::new(service, sink)))
            }
            McpTransport::Http { url, headers, .. } => {
                let mut config = StreamableHttpClientTransportConfig::with_uri(url.clone())
                    .max_concurrent_requests(MCP_HTTP_MAX_CONCURRENCY)
                    .control_request_timeout(Duration::from_millis(MCP_CONTROL_REQUEST_TIMEOUT_MS));
                for (name, value) in headers {
                    let name = http::HeaderName::from_bytes(name.as_bytes()).map_err(|_| {
                        McpClientError::new(
                            McpClientErrorKind::Init,
                            "invalid MCP HTTP header name",
                        )
                    })?;
                    let value = http::HeaderValue::from_str(value).map_err(|_| {
                        McpClientError::new(
                            McpClientErrorKind::Init,
                            "invalid MCP HTTP header value",
                        )
                    })?;
                    config.custom_headers.insert(name, value);
                }
                let transport = StreamableHttpClientTransport::from_config(config);
                let service = tokio::time::timeout(
                    Duration::from_millis(MCP_INIT_TIMEOUT_MS),
                    ZgraphMcpClient.serve_with_lifecycle(transport, lifecycle),
                )
                .await
                .map_err(|_| {
                    McpClientError::new(McpClientErrorKind::Init, "MCP initialize timed out")
                })?
                .map_err(|error| {
                    McpClientError::new(
                        McpClientErrorKind::Init,
                        format!("MCP HTTP initialize failed: {error}"),
                    )
                })?;
                Ok(Arc::new(RealMcpClient::new(
                    service,
                    Arc::new(Mutex::new(String::new())),
                )))
            }
        }
    }
}

fn map_rmcp_tool(tool: RmcpTool) -> McpTool {
    let mut description = tool
        .description
        .map(|value| value.to_string())
        .unwrap_or_default();
    if description.chars().count() > crate::constants::MAX_MCP_TOOL_DESCRIPTION_CHARS {
        description = description
            .chars()
            .take(crate::constants::MAX_MCP_TOOL_DESCRIPTION_CHARS)
            .collect();
    }
    McpTool {
        server: String::new(),
        name: tool.name.to_string(),
        description,
        input_schema: Value::Object((*tool.input_schema).clone()),
    }
}

/// 按上限**流式累积**结果文本（不先拼全量再截断；REV-098 P2⑧）。
fn call_result_text(result: &CallToolResult, limit_chars: usize) -> String {
    let mut text = String::new();
    let mut count = 0usize;
    for block in &result.content {
        let chunk = match block {
            ContentBlock::Text(content) => content.text.clone(),
            other => serde_json::to_string(other).unwrap_or_default(),
        };
        let remaining = limit_chars.saturating_sub(count);
        if remaining == 0 {
            break;
        }
        let take = chunk.chars().take(remaining).collect::<String>();
        count += take.chars().count();
        text.push_str(&take);
    }
    if text.is_empty() {
        if let Some(structured) = &result.structured_content {
            text = structured.to_string().chars().take(limit_chars).collect();
        }
    }
    text
}

#[async_trait]
impl McpClient for RealMcpClient {
    async fn list_tools(&self) -> Result<Vec<McpTool>, McpClientError> {
        let guard = self.service.lock().await;
        let service = guard.as_ref().ok_or_else(|| {
            McpClientError::new(McpClientErrorKind::Closed, "MCP client is closed")
        })?;
        let tools = tokio::time::timeout(
            Duration::from_millis(crate::constants::MCP_TOOLS_LIST_TIMEOUT_MS),
            service.peer().list_all_tools(),
        )
        .await
        .map_err(|_| McpClientError::new(McpClientErrorKind::Timeout, "MCP tools/list timed out"))?
        .map_err(|error| Self::map_service_error(&error))?;
        Ok(tools.into_iter().map(map_rmcp_tool).collect())
    }

    async fn call_tool(
        &self,
        tool: &str,
        args: Value,
        timeout: Duration,
        mrtr_max_rounds: usize,
    ) -> Result<McpCallOutcome, McpClientError> {
        let guard = self.service.lock().await;
        let service = guard.as_ref().ok_or_else(|| {
            McpClientError::new(McpClientErrorKind::Closed, "MCP client is closed")
        })?;
        let arguments = args.as_object().cloned().unwrap_or_default();
        let params = CallToolRequestParams::new(tool.to_string()).with_arguments(arguments);
        let result = tokio::time::timeout(
            timeout,
            service.call_tool_with_mrtr_max_rounds(params, mrtr_max_rounds),
        )
        .await
        .map_err(|_| McpClientError::new(McpClientErrorKind::Timeout, "MCP tools/call timed out"))?
        .map_err(|error| Self::map_service_error(&error))?;
        Ok(McpCallOutcome {
            text: call_result_text(&result, crate::constants::MCP_TOOL_RESULT_MAX_CHARS),
            is_error: result.is_error.unwrap_or(false),
        })
    }

    async fn shutdown(&self) {
        let mut guard = self.service.lock().await;
        if let Some(service) = guard.take() {
            let _ = service.cancel().await;
        }
        if let Ok(stderr) = self.stderr.lock() {
            // stderr 只保留诊断用；不写 trace（DR-010 §4/§7）。
            let _ = stderr.len();
        }
    }
}
