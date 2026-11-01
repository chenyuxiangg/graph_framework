//! v0.3 MCP（DR-010）：registry 解析、AgentConfig 白名单、工具映射与进程内工具桥接。
//!
//! 设计要点：
//! - **默认无 MCP**：`AgentConfig.mcp` 缺省时不读 registry、不启进程、不注册工具；
//! - **显式开关**：`--allow-mcp`（库调用方经 `HarnessExecutor::with_mcp_allow`）；未开启
//!   且配置了 MCP → `HARNESS_MCP_ERROR{reason:"not_allowed"}`；
//! - **必须显式进 `AgentConfig.tools`**：MCP 工具不会被自动暴露给 LLM；`allow` 再做交集；
//! - **客户端 seam**：[`McpClient`]/[`McpClientFactory`] 抽象传输，真实实现（rmcp、双 era）
//!   在 `feature = "mcp"` 下；测试用 [`testing::FakeMcpClient`] 覆盖 list/call/MRTR/超时/截断/重启；
//! - **错误收敛**：所有 MCP 失败 → 单一 `HARNESS_MCP_ERROR`（DR-010 §8），`detail.reason`
//!   为闭集；工具执行期错误作为 `TOOL_ERROR` 结果喂回 LLM，不 fail run。

#[cfg(feature = "mcp")]
mod real;
#[cfg(any(test, feature = "mcp"))]
pub mod testing;

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use serde_json::Value;

use crate::constants::{
    MAX_MCP_SERVER_NAME_CHARS, MAX_MCP_TOOL_DESCRIPTION_CHARS, MCP_DEFAULT_CALL_TIMEOUT_MS,
    MCP_MAX_RESTARTS_PER_RUN, MCP_MRTR_MAX_ROUNDS, MCP_REGISTRY_MAX_BYTES,
    MCP_TOOL_RESULT_MAX_CHARS,
};
use crate::error::HarnessError;
use crate::tools::{ToolDef, ToolFailure};

#[cfg(feature = "mcp")]
pub use real::RealMcpFactory;

/// 阶段（用于 `HARNESS_MCP_ERROR.detail.stage`；对齐 [`crate::error::Stage`]）。
use crate::error::Stage;

/// `AgentConfig.mcp` 段（additive；DR-010 §2.2）。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct McpConfig {
    /// 本 Agent 使用的 registry server 名（顺序无关；≤16）。
    pub servers: Vec<String>,
    /// 可选的工具 allow 列表（`mcp__<server>__<tool>`；空 = 不额外限制）。
    pub allow: Vec<String>,
}

/// registry 中单个 server 的解析结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpServerSpec {
    /// server 名（`^[a-z0-9][a-z0-9_-]{0,63}$`）。
    pub name: String,
    /// 传输方式（stdio 或 Streamable HTTP）。
    pub transport: McpTransport,
    /// 单次 `tools/call` 超时（毫秒；缺省 30s）。
    pub timeout_ms: u64,
}

/// MCP 传输方式。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum McpTransport {
    /// stdio 子进程。
    Stdio {
        /// 可执行文件。
        command: String,
        /// 参数。
        args: Vec<String>,
        /// 额外环境变量（`${ENV}` 已展开；不落日志）。
        env: BTreeMap<String, String>,
    },
    /// Streamable HTTP。
    Http {
        /// endpoint URL（默认 https）。
        url: String,
        /// 请求头（`${ENV}` 已展开；不落日志）。
        headers: BTreeMap<String, String>,
        /// 显式允许 `http://`（默认仅 https）。
        allow_insecure_http: bool,
    },
}

impl McpServerSpec {
    /// 无密钥的启动命令/URL 摘要（stderr 提示用；不展开 env/headers 值）。
    pub fn display_command(&self) -> String {
        match &self.transport {
            McpTransport::Stdio {
                command, args, env, ..
            } => {
                let mut out = command.clone();
                for arg in args {
                    out.push(' ');
                    out.push_str(arg);
                }
                if !env.is_empty() {
                    let names: Vec<&str> = env.keys().map(String::as_str).collect();
                    out.push_str(&format!(" (env: {})", names.join(",")));
                }
                out
            }
            McpTransport::Http { url, .. } => url.clone(),
        }
    }
}

/// 已解析的 MCP 计划：只含本 Agent 引用的 servers + allow 列表。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpPlan {
    /// 本 Agent 引用的 server（registry 顺序 = AgentConfig 引用顺序）。
    pub servers: Vec<McpServerSpec>,
    /// 工具 allow 列表（空 = 不额外限制）。
    pub allow: Vec<String>,
}

impl McpPlan {
    /// 按 server 名查找。
    pub fn server(&self, name: &str) -> Option<&McpServerSpec> {
        self.servers.iter().find(|server| server.name == name)
    }

    /// 工具是否在 allow 列表内（空 allow = 全部允许）。
    pub fn tool_allowed(&self, full_name: &str) -> bool {
        self.allow.is_empty() || self.allow.iter().any(|entry| entry == full_name)
    }
}

/// MCP 工具（映射前；`name` 为 server 内的原始工具名）。
#[derive(Debug, Clone, PartialEq)]
pub struct McpTool {
    /// server 名。
    pub server: String,
    /// server 内工具名。
    pub name: String,
    /// 描述（已截断到 ≤1024 字符）。
    pub description: String,
    /// inputSchema（2020-12 JSON Schema 子集）。
    pub input_schema: Value,
}

impl McpTool {
    /// LLM function name：`mcp__<server>__<tool>`（DR-010 §3）。
    pub fn full_name(&self) -> String {
        format!("mcp__{}__{}", self.server, self.name)
    }
}

/// 一次 `tools/call` 的结果（文本/结构化；`is_error` = server 标记的工具错误）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpCallOutcome {
    /// 结果文本（未截断；调用方负责 32768 截断）。
    pub text: String,
    /// server 是否标记 `isError`。
    pub is_error: bool,
}

/// MCP 客户端错误类别（映射到 `HARNESS_MCP_ERROR.detail.reason`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum McpClientErrorKind {
    /// 初始化/握手失败（含双 era 均失败）。
    Init,
    /// 协议版本不兼容。
    ProtocolVersion,
    /// 传输/连接关闭（可重启重试）。
    Closed,
    /// 超时。
    Timeout,
    /// MRTR `input_required`（v0.3 不支持）。
    InputRequiredUnsupported,
    /// 其他。
    Other,
}

/// MCP 客户端错误（无敏感信息）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpClientError {
    /// 类别。
    pub kind: McpClientErrorKind,
    /// 人类可读信息（不得含 env/headers/密钥）。
    pub message: String,
}

impl McpClientError {
    /// 构造。
    pub fn new(kind: McpClientErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }

    /// 映射为 `detail.reason`（DR-010 §8 闭集）。
    pub fn reason(&self) -> &'static str {
        match self.kind {
            McpClientErrorKind::Init => "init_failed",
            McpClientErrorKind::ProtocolVersion => "protocol_version",
            McpClientErrorKind::Closed => "transport_closed",
            McpClientErrorKind::Timeout => "timeout",
            McpClientErrorKind::InputRequiredUnsupported => "input_required_unsupported",
            McpClientErrorKind::Other => "client_error",
        }
    }
}

/// 客户端 seam：真实 rmcp 实现与测试 Fake 共享的传输抽象。
#[async_trait]
pub trait McpClient: Send + Sync {
    /// `tools/list`（分页由实现内部处理；本方法返回全量）。
    async fn list_tools(&self) -> Result<Vec<McpTool>, McpClientError>;

    /// `tools/call`（含 MRTR/超时；`args` 必须是 JSON object）。
    async fn call_tool(
        &self,
        tool: &str,
        args: Value,
        timeout: Duration,
        mrtr_max_rounds: usize,
    ) -> Result<McpCallOutcome, McpClientError>;

    /// 关闭连接/进程（best-effort；Drop 也应清理）。
    async fn shutdown(&self);
}

/// 客户端工厂 seam：按 server spec 建连（每 run 每 server 懒建一次）。
#[async_trait]
pub trait McpClientFactory: Send + Sync {
    /// 建立连接（stdio 启动子进程 / HTTP 建 transport + 握手）。
    async fn connect(&self, spec: &McpServerSpec) -> Result<Arc<dyn McpClient>, McpClientError>;
}

/// 由 `detail.reason` 生成 `HARNESS_MCP_ERROR`（无敏感值）。
pub(crate) fn mcp_config_error(
    reason: &'static str,
    server: Option<&str>,
    message: impl Into<String>,
) -> HarnessError {
    HarnessError::mcp_error(reason, server, None, Stage::Load, message)
}

/// registry 文件（`<assets-dir>/mcp_servers.json`）。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct McpRegistry {
    servers: BTreeMap<String, McpServerSpec>,
}

impl McpRegistry {
    /// 加载 registry；文件缺失 = 无 server（不报错）。
    pub fn load(assets_dir: &Path) -> Result<Self, HarnessError> {
        let path = assets_dir.join("mcp_servers.json");
        let bytes = match std::fs::read(&path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Self::default())
            }
            Err(error) => {
                return Err(mcp_config_error(
                    "config_invalid",
                    None,
                    format!("cannot read mcp_servers.json: {error}"),
                ))
            }
        };
        if bytes.len() as u64 > MCP_REGISTRY_MAX_BYTES {
            return Err(mcp_config_error(
                "config_invalid",
                None,
                format!(
                    "mcp_servers.json is {} bytes (limit {MCP_REGISTRY_MAX_BYTES})",
                    bytes.len()
                ),
            ));
        }
        let text = String::from_utf8(bytes).map_err(|_| {
            mcp_config_error(
                "config_invalid",
                None,
                "mcp_servers.json is not valid UTF-8",
            )
        })?;
        let root: Value = serde_json::from_str(&text).map_err(|error| {
            mcp_config_error(
                "config_invalid",
                None,
                format!("unparseable mcp_servers.json: {error}"),
            )
        })?;
        Self::from_value(&root, &|name| std::env::var(name).ok())
    }

    /// 从已解析 JSON 构造（`env_lookup` 供测试注入环境变量读取）。
    pub fn from_value(
        root: &Value,
        env_lookup: &dyn Fn(&str) -> Option<String>,
    ) -> Result<Self, HarnessError> {
        let Some(object) = root.as_object() else {
            return Err(mcp_config_error(
                "config_invalid",
                None,
                "mcp_servers.json root must be an object",
            ));
        };
        for key in object.keys() {
            if key != "mcpServers" && key != "$schema" {
                return Err(mcp_config_error(
                    "config_invalid",
                    None,
                    format!("unknown mcp_servers.json field '{key}'"),
                ));
            }
        }
        let Some(servers) = object.get("mcpServers") else {
            return Ok(Self::default());
        };
        let Some(servers) = servers.as_object() else {
            return Err(mcp_config_error(
                "config_invalid",
                None,
                "'mcpServers' must be an object",
            ));
        };
        let mut parsed = BTreeMap::new();
        for (name, value) in servers {
            if !is_valid_server_name(name) {
                return Err(mcp_config_error(
                    "config_invalid",
                    Some(name),
                    format!("invalid MCP server name '{name}'"),
                ));
            }
            let spec = parse_server(name, value, env_lookup)?;
            parsed.insert(name.clone(), spec);
        }
        Ok(Self { servers: parsed })
    }

    /// 已加载 server 数（测试/诊断）。
    pub fn len(&self) -> usize {
        self.servers.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.servers.is_empty()
    }

    /// 解析 AgentConfig.mcp → 计划；`allowed` = `--allow-mcp` 开关。
    ///
    /// 语义：无 `mcp` 段/空 servers → `Ok(None)`（默认路径不变）；未开开关 →
    /// `HARNESS_MCP_ERROR{not_allowed}`；server 缺失/重复/allow 非法 → fail-closed。
    pub fn resolve(
        &self,
        config: &McpConfig,
        allowed: bool,
        max_servers: usize,
    ) -> Result<Option<McpPlan>, HarnessError> {
        if config.servers.is_empty() && config.allow.is_empty() {
            return Ok(None);
        }
        if config.servers.is_empty() {
            return Err(mcp_config_error(
                "config_invalid",
                None,
                "AgentConfig.mcp.allow requires non-empty 'servers'",
            ));
        }
        if config.servers.len() > max_servers {
            return Err(mcp_config_error(
                "config_invalid",
                None,
                format!("AgentConfig.mcp.servers exceeds limit {max_servers}"),
            ));
        }
        if !allowed {
            return Err(mcp_config_error(
                "not_allowed",
                None,
                "MCP is disabled: pass --allow-mcp to enable configured MCP servers",
            ));
        }
        let mut seen: BTreeMap<&str, ()> = BTreeMap::new();
        let mut servers = Vec::new();
        for name in &config.servers {
            if seen.insert(name.as_str(), ()).is_some() {
                return Err(mcp_config_error(
                    "name_conflict",
                    Some(name),
                    format!("duplicate MCP server reference '{name}'"),
                ));
            }
            let Some(spec) = self.servers.get(name) else {
                return Err(mcp_config_error(
                    "server_not_found",
                    Some(name),
                    format!("MCP server '{name}' not found in registry"),
                ));
            };
            servers.push(spec.clone());
        }
        let mut allow_seen: BTreeMap<&str, ()> = BTreeMap::new();
        for entry in &config.allow {
            let Some((server, tool)) = parse_full_tool_name(entry) else {
                return Err(mcp_config_error(
                    "allow_invalid",
                    None,
                    format!("invalid MCP allow entry '{entry}' (expected mcp__<server>__<tool>)"),
                ));
            };
            if !servers.iter().any(|spec| spec.name == server) {
                return Err(mcp_config_error(
                    "allow_invalid",
                    Some(server),
                    format!("MCP allow entry '{entry}' references an unconfigured server"),
                ));
            }
            if tool.is_empty() {
                return Err(mcp_config_error(
                    "allow_invalid",
                    Some(server),
                    format!("invalid MCP allow entry '{entry}'"),
                ));
            }
            if allow_seen.insert(entry.as_str(), ()).is_some() {
                return Err(mcp_config_error(
                    "allow_invalid",
                    Some(server),
                    format!("duplicate MCP allow entry '{entry}'"),
                ));
            }
        }
        Ok(Some(McpPlan {
            servers,
            allow: config.allow.clone(),
        }))
    }
}

/// 解析 `mcp__<server>__<tool>`（返回 `(server, tool)`）。
pub fn parse_full_tool_name(full_name: &str) -> Option<(&str, &str)> {
    let rest = full_name.strip_prefix("mcp__")?;
    let (server, tool) = rest.split_once("__")?;
    if server.is_empty() || tool.is_empty() {
        return None;
    }
    Some((server, tool))
}

pub(crate) fn is_valid_server_name(name: &str) -> bool {
    if name.is_empty() || name.chars().count() > MAX_MCP_SERVER_NAME_CHARS {
        return false;
    }
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !(first.is_ascii_lowercase() || first.is_ascii_digit()) {
        return false;
    }
    chars.all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '_' || ch == '-')
}

fn parse_server(
    name: &str,
    value: &Value,
    env_lookup: &dyn Fn(&str) -> Option<String>,
) -> Result<McpServerSpec, HarnessError> {
    let Some(object) = value.as_object() else {
        return Err(mcp_config_error(
            "config_invalid",
            Some(name),
            "MCP server entry must be an object",
        ));
    };
    for key in object.keys() {
        if !matches!(
            key.as_str(),
            "command" | "args" | "env" | "url" | "headers" | "allow_insecure_http" | "timeout_ms"
        ) {
            return Err(mcp_config_error(
                "config_invalid",
                Some(name),
                format!("unknown MCP server field '{key}'"),
            ));
        }
    }
    let has_command = object.contains_key("command");
    let has_url = object.contains_key("url");
    if has_command == has_url {
        return Err(mcp_config_error(
            "config_invalid",
            Some(name),
            "MCP server must have exactly one of 'command' or 'url'",
        ));
    }
    let timeout_ms = match object.get("timeout_ms") {
        None => MCP_DEFAULT_CALL_TIMEOUT_MS,
        Some(value) => value
            .as_u64()
            .filter(|value| (1..=600_000).contains(value))
            .ok_or_else(|| {
                mcp_config_error(
                    "config_invalid",
                    Some(name),
                    "'timeout_ms' must be an integer in 1..=600000",
                )
            })?,
    };
    let transport = if has_command {
        let command = object
            .get("command")
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| {
                mcp_config_error(
                    "config_invalid",
                    Some(name),
                    "'command' must be a non-empty string",
                )
            })?;
        let args = match object.get("args") {
            None => Vec::new(),
            Some(value) => value
                .as_array()
                .ok_or_else(|| {
                    mcp_config_error("config_invalid", Some(name), "'args' must be an array")
                })?
                .iter()
                .map(|item| {
                    item.as_str().map(str::to_string).ok_or_else(|| {
                        mcp_config_error(
                            "config_invalid",
                            Some(name),
                            "'args' entries must be strings",
                        )
                    })
                })
                .collect::<Result<Vec<_>, _>>()?,
        };
        let env = match object.get("env") {
            None => BTreeMap::new(),
            Some(value) => expand_string_map(name, "env", value, env_lookup)?,
        };
        McpTransport::Stdio {
            command: command.to_string(),
            args,
            env,
        }
    } else {
        let url = object
            .get("url")
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| {
                mcp_config_error(
                    "config_invalid",
                    Some(name),
                    "'url' must be a non-empty string",
                )
            })?;
        let https = url.starts_with("https://");
        let http = url.starts_with("http://");
        if !https && !http {
            return Err(mcp_config_error(
                "config_invalid",
                Some(name),
                "MCP url must start with https:// (or http:// with allow_insecure_http)",
            ));
        }
        let allow_insecure_http = match object.get("allow_insecure_http") {
            None => false,
            Some(value) => value.as_bool().ok_or_else(|| {
                mcp_config_error(
                    "config_invalid",
                    Some(name),
                    "'allow_insecure_http' must be a boolean",
                )
            })?,
        };
        if http && !allow_insecure_http {
            return Err(mcp_config_error(
                "config_invalid",
                Some(name),
                "http:// MCP url requires allow_insecure_http: true",
            ));
        }
        let headers = match object.get("headers") {
            None => BTreeMap::new(),
            Some(value) => expand_string_map(name, "headers", value, env_lookup)?,
        };
        McpTransport::Http {
            url: url.to_string(),
            headers,
            allow_insecure_http,
        }
    };
    Ok(McpServerSpec {
        name: name.to_string(),
        transport,
        timeout_ms,
    })
}

fn expand_string_map(
    server: &str,
    field: &str,
    value: &Value,
    env_lookup: &dyn Fn(&str) -> Option<String>,
) -> Result<BTreeMap<String, String>, HarnessError> {
    let Some(object) = value.as_object() else {
        return Err(mcp_config_error(
            "config_invalid",
            Some(server),
            format!("'{field}' must be an object"),
        ));
    };
    let mut expanded = BTreeMap::new();
    for (key, value) in object {
        let Some(raw) = value.as_str() else {
            return Err(mcp_config_error(
                "config_invalid",
                Some(server),
                format!("'{field}.{key}' must be a string"),
            ));
        };
        let value = expand_env(server, key, raw, env_lookup)?;
        expanded.insert(key.clone(), value);
    }
    Ok(expanded)
}

/// 展开 `${ENV}`（仅整段/内嵌形式；未定义 → `env_undefined`，错误信息不含值）。
fn expand_env(
    server: &str,
    key: &str,
    raw: &str,
    env_lookup: &dyn Fn(&str) -> Option<String>,
) -> Result<String, HarnessError> {
    let mut out = String::with_capacity(raw.len());
    let mut rest = raw;
    while let Some(start) = rest.find("${") {
        out.push_str(&rest[..start]);
        let Some(end) = rest[start + 2..].find('}') else {
            return Err(mcp_config_error(
                "config_invalid",
                Some(server),
                format!("unterminated env reference in '{key}'"),
            ));
        };
        let name = &rest[start + 2..start + 2 + end];
        if name.is_empty()
            || !name
                .chars()
                .all(|ch| ch.is_ascii_uppercase() || ch.is_ascii_digit() || ch == '_')
        {
            return Err(mcp_config_error(
                "config_invalid",
                Some(server),
                format!("invalid env reference in '{key}'"),
            ));
        }
        let Some(value) = env_lookup(name) else {
            return Err(mcp_config_error(
                "env_undefined",
                Some(server),
                format!("environment variable '{name}' is not defined"),
            ));
        };
        out.push_str(&value);
        rest = &rest[start + 2 + end + 1..];
    }
    out.push_str(rest);
    if out.contains("${") {
        return Err(mcp_config_error(
            "config_invalid",
            Some(server),
            format!("unresolved env reference in '{key}'"),
        ));
    }
    Ok(out)
}

/// 把已发现的 MCP 工具映射为 `ToolDef`（只保留显式请求 ∩ allow）。
pub fn map_tools(
    plan: &McpPlan,
    requested: &[String],
    discovered: &[McpTool],
) -> Result<Vec<ToolDef>, HarnessError> {
    let mut by_name: BTreeMap<String, &McpTool> = BTreeMap::new();
    for tool in discovered {
        by_name.insert(tool.full_name(), tool);
    }
    let mut tools = Vec::new();
    for reference in requested {
        if !reference.starts_with("mcp__") {
            continue;
        }
        let Some((server_name, _tool_name)) = parse_full_tool_name(reference) else {
            return Err(mcp_config_error(
                "config_invalid",
                None,
                format!("invalid MCP tool reference '{reference}'"),
            ));
        };
        let Some(spec) = plan.server(server_name) else {
            return Err(mcp_config_error(
                "server_not_found",
                Some(server_name),
                format!("MCP tool '{reference}' references an unconfigured server"),
            ));
        };
        if !plan.tool_allowed(reference) {
            return Err(mcp_config_error(
                "tool_not_allowed",
                Some(server_name),
                format!("MCP tool '{reference}' is not allowed by AgentConfig.mcp.allow"),
            ));
        }
        let Some(tool) = by_name.get(reference) else {
            return Err(mcp_config_error(
                "tool_not_found",
                Some(server_name),
                format!("MCP tool '{reference}' not found on server '{server_name}'"),
            ));
        };
        tools.push(ToolDef {
            name: reference.clone(),
            description: truncate_chars(&tool.description, MAX_MCP_TOOL_DESCRIPTION_CHARS),
            parameters: tool.input_schema.clone(),
            timeout_ms: spec.timeout_ms,
            source: format!("mcp:{}", spec.name),
        });
    }
    Ok(tools)
}

fn truncate_chars(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value.to_string();
    }
    value.chars().take(max_chars).collect()
}

/// 截断工具结果到 `MCP_TOOL_RESULT_MAX_CHARS`（chars；追加标记）。
pub fn truncate_result(value: &str) -> String {
    if value.chars().count() <= MCP_TOOL_RESULT_MAX_CHARS {
        return value.to_string();
    }
    let head: String = value.chars().take(MCP_TOOL_RESULT_MAX_CHARS).collect();
    format!("{head}…[mcp result truncated]")
}

/// 运行期 MCP 状态：每 run 每 server 懒连接、工具列表 run 级缓存、崩溃重连≤1。
pub struct McpRun {
    plan: Arc<McpPlan>,
    factory: Arc<dyn McpClientFactory>,
    state: tokio::sync::Mutex<RunState>,
}

#[derive(Default)]
struct RunState {
    clients: BTreeMap<String, ClientSlot>,
    discovered: Option<Vec<McpTool>>,
}

struct ClientSlot {
    client: Option<Arc<dyn McpClient>>,
    restarts: u32,
}

impl McpRun {
    /// 构造（不立即连接；懒启动）。
    pub fn new(plan: Arc<McpPlan>, factory: Arc<dyn McpClientFactory>) -> Arc<Self> {
        Arc::new(Self {
            plan,
            factory,
            state: tokio::sync::Mutex::new(RunState::default()),
        })
    }

    /// 本 run 的 MCP 计划。
    pub fn plan(&self) -> &McpPlan {
        &self.plan
    }

    /// 启动提示（stderr；无密钥；调用方决定是否打印）。
    pub fn startup_notice(&self) -> Vec<String> {
        self.plan
            .servers
            .iter()
            .map(|server| {
                format!(
                    "mcp: starting server '{}' ({})",
                    server.name,
                    server.display_command()
                )
            })
            .collect()
    }

    async fn ensure_client(
        &self,
        state: &mut RunState,
        server: &str,
    ) -> Result<Arc<dyn McpClient>, HarnessError> {
        let spec = self.plan.server(server).ok_or_else(|| {
            mcp_config_error("server_not_found", Some(server), "MCP server not in plan")
        })?;
        if let Some(slot) = state.clients.get(server) {
            if let Some(client) = &slot.client {
                return Ok(client.clone());
            }
        }
        let client = self.factory.connect(spec).await.map_err(|error| {
            HarnessError::mcp_error(
                error.reason(),
                Some(server),
                None,
                Stage::Load,
                format!("MCP server '{server}' connect failed: {}", error.message),
            )
        })?;
        state
            .clients
            .entry(server.to_string())
            .or_insert(ClientSlot {
                client: None,
                restarts: 0,
            })
            .client = Some(client.clone());
        Ok(client)
    }

    /// `tools/list`（每 run 缓存；列出所有 servers 的工具并过滤非法工具名/描述）。
    pub async fn list_tools(&self) -> Result<Vec<McpTool>, HarnessError> {
        let mut state = self.state.lock().await;
        if let Some(cached) = &state.discovered {
            return Ok(cached.clone());
        }
        let mut all = Vec::new();
        for server in &self.plan.servers {
            let client = self.ensure_client(&mut state, &server.name).await?;
            let tools = client.list_tools().await.map_err(|error| {
                HarnessError::mcp_error(
                    error.reason(),
                    Some(&server.name),
                    None,
                    Stage::Load,
                    format!(
                        "MCP server '{}' tools/list failed: {}",
                        server.name, error.message
                    ),
                )
            })?;
            for mut tool in tools {
                tool.server = server.name.clone();
                if !is_valid_function_name(&tool.full_name()) {
                    eprintln!(
                        "warning: skipping MCP tool '{}' (name is not LLM-function-safe)",
                        tool.full_name()
                    );
                    continue;
                }
                if !tool.input_schema.is_object() {
                    eprintln!(
                        "warning: skipping MCP tool '{}' (inputSchema is not an object)",
                        tool.full_name()
                    );
                    continue;
                }
                tool.description =
                    truncate_chars(&tool.description, MAX_MCP_TOOL_DESCRIPTION_CHARS);
                all.push(tool);
            }
        }
        all.sort_by_key(|tool| tool.full_name());
        state.discovered = Some(all.clone());
        Ok(all)
    }

    /// 执行 MCP 工具（进程内）；失败返回 `TOOL_ERROR` JSON 内容（喂回 LLM）。
    pub async fn execute_tool(&self, full_name: &str, args: &Value) -> (bool, String) {
        let Some((server, tool)) = parse_full_tool_name(full_name) else {
            return (
                false,
                crate::tools::tool_error_content(&ToolFailure::new(
                    "unknown_mcp_tool",
                    format!("invalid MCP tool name '{full_name}'"),
                )),
            );
        };
        if self.plan.server(server).is_none() {
            return (
                false,
                crate::tools::tool_error_content(&ToolFailure::new(
                    "server_not_found",
                    format!("MCP server '{server}' is not configured for this agent"),
                )),
            );
        }
        if !args.is_object() {
            return (
                false,
                crate::tools::tool_error_content(&ToolFailure::new(
                    "invalid_args",
                    "MCP tool arguments must be a JSON object",
                )),
            );
        }
        let timeout = self
            .plan
            .server(server)
            .map(|spec| Duration::from_millis(spec.timeout_ms))
            .unwrap_or(Duration::from_millis(MCP_DEFAULT_CALL_TIMEOUT_MS));
        let mut state = self.state.lock().await;
        let mut attempt = 0u32;
        loop {
            let client = match self.ensure_client(&mut state, server).await {
                Ok(client) => client,
                Err(error) => {
                    return (
                        false,
                        crate::tools::tool_error_content(&ToolFailure::new(
                            "mcp_init_failed",
                            error.message.clone(),
                        )),
                    )
                }
            };
            let result = client
                .call_tool(tool, args.clone(), timeout, MCP_MRTR_MAX_ROUNDS)
                .await;
            match result {
                Ok(outcome) => {
                    let text = truncate_result(&outcome.text);
                    return (!outcome.is_error, text);
                }
                Err(error)
                    if matches!(error.kind, McpClientErrorKind::Closed)
                        && attempt < MCP_MAX_RESTARTS_PER_RUN =>
                {
                    // 崩溃/连接关闭：同 run 最多重启一次（仅重建该 server）。
                    attempt += 1;
                    if let Some(slot) = state.clients.get_mut(server) {
                        if let Some(client) = slot.client.take() {
                            client.shutdown().await;
                        }
                        slot.restarts += 1;
                    }
                    continue;
                }
                Err(error) => {
                    return (
                        false,
                        crate::tools::tool_error_content(&ToolFailure::new(
                            error.reason(),
                            error.message,
                        )),
                    )
                }
            }
        }
    }

    /// 关闭全部连接（run 结束/取消；best-effort）。
    pub async fn shutdown(&self) {
        let mut state = self.state.lock().await;
        for slot in state.clients.values_mut() {
            if let Some(client) = slot.client.take() {
                client.shutdown().await;
            }
        }
        state.discovered = None;
    }
}

fn is_valid_function_name(name: &str) -> bool {
    !name.is_empty()
        && name.chars().count() <= 64
        && name
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-')
}

/// 默认客户端工厂：`feature = "mcp"` 用 rmcp 实现，否则返回明确错误。
pub(crate) fn default_factory() -> Arc<dyn McpClientFactory> {
    #[cfg(feature = "mcp")]
    {
        Arc::new(RealMcpFactory)
    }
    #[cfg(not(feature = "mcp"))]
    {
        Arc::new(UnavailableFactory)
    }
}

/// 未编译 `mcp` feature 时的 fail-closed 工厂（默认构建不引入 rmcp 依赖）。
#[cfg(not(feature = "mcp"))]
struct UnavailableFactory;

#[cfg(not(feature = "mcp"))]
#[async_trait]
impl McpClientFactory for UnavailableFactory {
    async fn connect(&self, _spec: &McpServerSpec) -> Result<Arc<dyn McpClient>, McpClientError> {
        Err(McpClientError::new(
            McpClientErrorKind::Init,
            "zgraph-harness was built without the `mcp` feature",
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::testing::{FakeMcpClient, FakeMcpFactory};
    use super::*;
    use serde_json::json;

    fn spec(name: &str, timeout_ms: u64) -> McpServerSpec {
        McpServerSpec {
            name: name.to_string(),
            transport: McpTransport::Stdio {
                command: "fake".to_string(),
                args: vec!["--stdio".to_string()],
                env: BTreeMap::new(),
            },
            timeout_ms,
        }
    }

    fn tool(server: &str, name: &str, description: &str) -> McpTool {
        McpTool {
            server: server.to_string(),
            name: name.to_string(),
            description: description.to_string(),
            input_schema: json!({"type": "object", "properties": {}}),
        }
    }

    fn registry() -> McpRegistry {
        let value = json!({
            "mcpServers": {
                "alpha": {"command": "npx", "args": ["-y", "server"], "env": {"TOKEN": "${TOKEN}"}},
                "remote": {"url": "https://example.com/mcp", "timeout_ms": 5000}
            }
        });
        McpRegistry::from_value(&value, &|name| match name {
            "TOKEN" => Some("secret".to_string()),
            _ => None,
        })
        .expect("registry")
    }

    #[test]
    fn registry_parses_and_rejects_invalid_entries() {
        let registry = registry();
        assert_eq!(registry.len(), 2);
        let plan = registry
            .resolve(
                &McpConfig {
                    servers: vec!["alpha".to_string(), "remote".to_string()],
                    allow: vec![],
                },
                true,
                crate::constants::MAX_MCP_SERVERS,
            )
            .expect("plan")
            .expect("some");
        assert_eq!(plan.servers.len(), 2);
        assert_eq!(plan.servers[1].timeout_ms, 5000);
        match &plan.servers[0].transport {
            McpTransport::Stdio { env, .. } => {
                assert_eq!(env.get("TOKEN").map(String::as_str), Some("secret"));
            }
            other => panic!("unexpected: {other:?}"),
        }

        let cases = [
            (
                json!({"mcpServers": {"bad": {"command": "x", "url": "https://e"}}}),
                "config_invalid",
            ),
            (
                json!({"mcpServers": {"bad": {"args": []}}}),
                "config_invalid",
            ),
            (
                json!({"mcpServers": {"bad": {"url": "http://e/mcp"}}}),
                "config_invalid",
            ),
            (
                json!({"mcpServers": {"bad": {"url": "https://e", "allow_insecure_http": "yes"}}}),
                "config_invalid",
            ),
            (
                json!({"mcpServers": {"bad": {"command": "x", "timeout_ms": 0}}}),
                "config_invalid",
            ),
            (
                json!({"mcpServers": {"bad": {"command": "x", "unknown": 1}}}),
                "config_invalid",
            ),
            (
                json!({"mcpServers": {"bad": {"command": "x", "env": {"T": "${NOPE}"}}}}),
                "env_undefined",
            ),
            (
                json!({"mcpServers": {"bad": {"command": "x", "env": {"T": "${bad-name}"}}}}),
                "config_invalid",
            ),
            (json!({"other": {}}), "config_invalid"),
        ];
        for (value, reason) in cases {
            let error = McpRegistry::from_value(&value, &|_| None).expect_err("invalid");
            assert_eq!(error.detail["reason"], reason, "value: {value}");
            assert_eq!(error.code, "HARNESS_MCP_ERROR");
        }
    }

    #[test]
    fn registry_resolve_fail_closed() {
        let registry = registry();
        let config = McpConfig {
            servers: vec!["alpha".to_string()],
            allow: vec!["mcp__alpha__echo".to_string()],
        };
        // 未开开关 → not_allowed。
        let error = registry
            .resolve(&config, false, 16)
            .expect_err("not allowed");
        assert_eq!(error.detail["reason"], "not_allowed");
        // 缺失 server。
        let error = registry
            .resolve(
                &McpConfig {
                    servers: vec!["ghost".to_string()],
                    allow: vec![],
                },
                true,
                16,
            )
            .expect_err("missing");
        assert_eq!(error.detail["reason"], "server_not_found");
        // allow 非法 / server 未配置 / 重复。
        for allow in [
            vec!["bad".to_string()],
            vec!["mcp__remote__echo".to_string()],
            vec![
                "mcp__alpha__echo".to_string(),
                "mcp__alpha__echo".to_string(),
            ],
        ] {
            let error = registry
                .resolve(
                    &McpConfig {
                        servers: vec!["alpha".to_string()],
                        allow,
                    },
                    true,
                    16,
                )
                .expect_err("invalid allow");
            assert_eq!(error.detail["reason"], "allow_invalid");
        }
        // 重复 server 引用 → name_conflict。
        let error = registry
            .resolve(
                &McpConfig {
                    servers: vec!["alpha".to_string(), "alpha".to_string()],
                    allow: vec![],
                },
                true,
                16,
            )
            .expect_err("duplicate");
        assert_eq!(error.detail["reason"], "name_conflict");
        // 空配置 → None（默认路径不变）。
        assert!(registry
            .resolve(&McpConfig::default(), true, 16)
            .expect("none")
            .is_none());
    }

    #[test]
    fn maps_only_explicit_and_allowed_tools() {
        let plan = McpPlan {
            servers: vec![spec("alpha", 1000)],
            allow: vec!["mcp__alpha__echo".to_string()],
        };
        let discovered = vec![
            tool("alpha", "echo", &"d".repeat(2000)),
            tool("alpha", "other", "not requested"),
        ];
        let tools =
            map_tools(&plan, &["mcp__alpha__echo".to_string()], &discovered).expect("mapped");
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].name, "mcp__alpha__echo");
        assert_eq!(tools[0].source, "mcp:alpha");
        assert_eq!(
            tools[0].description.chars().count(),
            crate::constants::MAX_MCP_TOOL_DESCRIPTION_CHARS
        );
        assert_eq!(tools[0].parameters["type"], "object");

        let error = map_tools(&plan, &["mcp__alpha__echo".to_string()], &discovered[1..])
            .expect_err("tool not found");
        assert_eq!(error.detail["reason"], "tool_not_found");
        let error = map_tools(&plan, &["mcp__alpha__other".to_string()], &discovered)
            .expect_err("not allowed");
        assert_eq!(error.detail["reason"], "tool_not_allowed");
    }

    #[tokio::test]
    async fn run_lists_and_calls_tools_with_truncation() {
        let client = FakeMcpClient::new().with_tools(vec![tool("", "echo", "desc")]);
        let client = client.with_default_outcome("x".repeat(40_000), false);
        let factory = FakeMcpFactory::new().with_client("alpha", client.clone());
        let plan = McpPlan {
            servers: vec![spec("alpha", 1000)],
            allow: vec![],
        };
        let run = McpRun::new(Arc::new(plan), factory);
        let tools = run.list_tools().await.expect("tools");
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].server, "alpha");
        // run 级缓存：再次 list 不重新连接。
        assert_eq!(run.list_tools().await.expect("cached").len(), 1);
        assert_eq!(client.connects.load(std::sync::atomic::Ordering::SeqCst), 1);

        let (ok, text) = run.execute_tool("mcp__alpha__echo", &json!({"q": 1})).await;
        assert!(ok);
        assert!(text.chars().count() <= crate::constants::MCP_TOOL_RESULT_MAX_CHARS + 64);
        assert_eq!(client.call_count(), 1);
    }

    #[tokio::test]
    async fn run_maps_errors_to_tool_errors_and_restarts_once() {
        // input_required → 结构化失败。
        let client = FakeMcpClient::new().push_outcome(Err(McpClientError::new(
            McpClientErrorKind::InputRequiredUnsupported,
            "input_required",
        )));
        let factory = FakeMcpFactory::new().with_client("alpha", client.clone());
        let plan = McpPlan {
            servers: vec![spec("alpha", 1000)],
            allow: vec![],
        };
        let run = McpRun::new(Arc::new(plan), factory);
        let (ok, text) = run.execute_tool("mcp__alpha__echo", &json!({})).await;
        assert!(!ok);
        assert!(
            text.contains("input_required_unsupported"),
            "content: {text}"
        );

        // 首次 Closed → 同 run 重启 1 次后成功。
        let client = FakeMcpClient::new()
            .with_default_outcome("recovered", false)
            .fail_first_closed();
        let factory = FakeMcpFactory::new().with_client("alpha", client.clone());
        let plan = McpPlan {
            servers: vec![spec("alpha", 1000)],
            allow: vec![],
        };
        let run = McpRun::new(Arc::new(plan), factory);
        let (ok, text) = run.execute_tool("mcp__alpha__echo", &json!({})).await;
        assert!(ok);
        assert_eq!(text, "recovered");
        assert_eq!(client.call_count(), 2, "one restart then retry");
        assert_eq!(client.connects.load(std::sync::atomic::Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn run_reports_timeout_and_is_error() {
        let client = FakeMcpClient::new().with_default_outcome("boom", true);
        let factory = FakeMcpFactory::new().with_client("alpha", client.clone());
        let plan = McpPlan {
            servers: vec![spec("alpha", 5)],
            allow: vec![],
        };
        let run = McpRun::new(Arc::new(plan), factory);
        let (ok, text) = run.execute_tool("mcp__alpha__echo", &json!({})).await;
        assert!(!ok, "isError result is a tool error");
        assert_eq!(text, "boom");
        run.shutdown().await;
    }

    #[test]
    fn result_truncation_is_bounded() {
        let text = truncate_result(&"y".repeat(40_000));
        assert!(text.chars().count() <= crate::constants::MCP_TOOL_RESULT_MAX_CHARS + 64);
        assert!(text.contains("mcp result truncated"));
        assert_eq!(truncate_result("small"), "small");
    }
}
