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

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use serde_json::Value;

use crate::constants::{
    MAX_MCP_SERVER_NAME_CHARS, MAX_MCP_TOOL_DESCRIPTION_CHARS, MCP_DEFAULT_TIMEOUT_MS,
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
    /// 无密钥的启动命令/URL 摘要（stderr 提示用；**脱敏**）。
    ///
    /// - URL 去 userinfo（`user:pass@`）与 query/fragment；
    /// - args 中 `--token/--key/--secret/--password` 形态的值打码（`=` 形式与下一个参数）；
    /// - env 只列变量名，不展开值；只输出 server 名/传输/命令骨架。
    pub fn display_command(&self) -> String {
        match &self.transport {
            McpTransport::Stdio {
                command, args, env, ..
            } => {
                let mut out = format!("stdio: {command}");
                let mut mask_next = false;
                for arg in args {
                    let masked = if mask_next {
                        mask_next = false;
                        "<redacted>".to_string()
                    } else if is_secret_arg(arg) {
                        if arg.contains('=') {
                            format!("{}<redacted>", arg.split('=').next().unwrap_or(arg))
                        } else {
                            mask_next = true;
                            "<redacted>".to_string()
                        }
                    } else {
                        arg.clone()
                    };
                    out.push(' ');
                    out.push_str(&masked);
                }
                if !env.is_empty() {
                    let names: Vec<&str> = env.keys().map(String::as_str).collect();
                    out.push_str(&format!(" (env: {})", names.join(",")));
                }
                out
            }
            McpTransport::Http { url, .. } => format!("http: {}", sanitize_url(url)),
        }
    }
}

fn is_secret_arg(arg: &str) -> bool {
    let lower = arg.to_ascii_lowercase();
    let key = lower.split('=').next().unwrap_or(&lower);
    matches!(
        key.trim_start_matches('-'),
        "token" | "key" | "secret" | "password" | "api-key" | "apikey" | "authorization"
    )
}

fn sanitize_url(raw: &str) -> String {
    let Some((scheme, rest)) = raw.split_once("://") else {
        return raw.to_string();
    };
    let rest = rest.split(['?', '#']).next().unwrap_or(rest);
    let (authority, path) = match rest.split_once('/') {
        Some((authority, path)) => (authority, Some(path)),
        None => (rest, None),
    };
    let host = authority
        .rsplit_once('@')
        .map_or(authority, |(_, host)| host);
    let mut sanitized = format!("{scheme}://{host}");
    if let Some(path) = path {
        sanitized.push('/');
        sanitized.push_str(path);
    }
    sanitized
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
    async fn connect(
        &self,
        spec: &McpServerSpec,
        assets_dir: &Path,
    ) -> Result<Arc<dyn McpClient>, McpClientError>;
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
        let server_names: Vec<&str> = servers.iter().map(|spec| spec.name.as_str()).collect();
        let mut allow_seen: BTreeMap<&str, ()> = BTreeMap::new();
        for entry in &config.allow {
            let Some((server, tool)) =
                parse_tool_name_for_servers(entry, server_names.iter().copied())
            else {
                return Err(mcp_config_error(
                    "allow_invalid",
                    None,
                    format!(
                        "invalid MCP allow entry '{entry}' (expected mcp__<configured-server>__<tool>)"
                    ),
                ));
            };
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

/// 解析 `mcp__<server>__<tool>`（`server` 名可能含 `__`）。
///
/// **必须**用已配置 server 名做最长前缀匹配（DR-010 §3 / REV-098 C1-P2④）：
/// `mcp__my__server__echo` → (`my__server`, `echo`)；未配置 → `None`。
pub fn parse_tool_name_for_servers<'a>(
    full_name: &'a str,
    servers: impl IntoIterator<Item = &'a str>,
) -> Option<(&'a str, &'a str)> {
    let rest = full_name.strip_prefix("mcp__")?;
    let mut matched: Option<&'a str> = None;
    for server in servers {
        if rest.starts_with(server)
            && rest[server.len()..].starts_with("__")
            && rest.len() > server.len() + 2
            && matched.is_none_or(|current| server.len() > current.len())
        {
            matched = Some(server);
        }
    }
    let server = matched?;
    let tool = &rest[server.len() + 2..];
    (!tool.is_empty()).then_some((server, tool))
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
        None => MCP_DEFAULT_TIMEOUT_MS,
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
            // DR-010 §8 / 00 §0.4：reason 闭集使用 `http_insecure`（非 config_invalid）。
            return Err(mcp_config_error(
                "http_insecure",
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
///
/// LLM 可见函数名按 `^[a-z][a-z0-9_]{0,63}$` 归一化（非法字符→`_`，超长/冲突→短 hash），
/// 反向映射由 [`McpBoundTool`] 携带；原始 MCP 工具名不变。
pub fn map_tools(
    plan: &McpPlan,
    requested: &[String],
    discovered: &[McpTool],
) -> Result<Vec<McpBoundTool>, HarnessError> {
    let mut by_name: BTreeMap<String, &McpTool> = BTreeMap::new();
    for tool in discovered {
        by_name.insert(tool.full_name(), tool);
    }
    let server_names: Vec<&str> = plan.servers.iter().map(|spec| spec.name.as_str()).collect();
    let mut seen: BTreeSet<String> = BTreeSet::new();
    let mut taken: BTreeSet<String> = BTreeSet::new();
    let mut tools = Vec::new();
    for reference in requested {
        if !reference.starts_with("mcp__") || !seen.insert(reference.clone()) {
            continue;
        }
        let Some((server_name, _tool_name)) =
            parse_tool_name_for_servers(reference, server_names.iter().copied())
        else {
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
        let function_name =
            normalized_function_name(server_name, &tool.name, reference, &mut taken);
        tools.push(McpBoundTool {
            def: ToolDef {
                name: function_name,
                description: truncate_chars(&tool.description, MAX_MCP_TOOL_DESCRIPTION_CHARS),
                parameters: tool.input_schema.clone(),
                timeout_ms: spec.timeout_ms,
                source: format!("mcp:{}", spec.name),
            },
            original: (*tool).clone(),
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

/// 每 run 共享的 MCP 连接池：`server → client` + `server → tools` 缓存。
///
/// **plan/allow 不在这里**（按 agent 隔离，REV-098 C1）：同一 run 的多个 agent 共享
/// 连接与 `tools/list` 结果，但各自的 servers/allow 由 [`McpRun`] 视图约束。
pub struct McpPool {
    factory: Arc<dyn McpClientFactory>,
    assets_dir: PathBuf,
    state: tokio::sync::Mutex<PoolState>,
}

#[derive(Default)]
struct PoolState {
    clients: BTreeMap<String, ClientSlot>,
    server_tools: BTreeMap<String, Vec<McpTool>>,
}

struct ClientSlot {
    client: Option<Arc<dyn McpClient>>,
    restarts: u32,
}

impl McpPool {
    /// 构造（不连接；懒启动）。
    pub fn new(factory: Arc<dyn McpClientFactory>, assets_dir: PathBuf) -> Arc<Self> {
        Arc::new(Self {
            factory,
            assets_dir,
            state: tokio::sync::Mutex::new(PoolState::default()),
        })
    }

    async fn ensure_client(
        &self,
        state: &mut PoolState,
        spec: &McpServerSpec,
    ) -> Result<Arc<dyn McpClient>, HarnessError> {
        if let Some(client) = state
            .clients
            .get(&spec.name)
            .and_then(|slot| slot.client.clone())
        {
            return Ok(client);
        }
        let client = self
            .factory
            .connect(spec, &self.assets_dir)
            .await
            .map_err(|error| {
                HarnessError::mcp_error(
                    error.reason(),
                    Some(&spec.name),
                    None,
                    Stage::Load,
                    format!(
                        "MCP server '{}' connect failed: {}",
                        spec.name, error.message
                    ),
                )
            })?;
        state
            .clients
            .entry(spec.name.clone())
            .or_insert(ClientSlot {
                client: None,
                restarts: 0,
            })
            .client = Some(client.clone());
        Ok(client)
    }

    /// `tools/list`（按 server 缓存；同一 run 内跨 agent 共享）。
    async fn tools_for(&self, spec: &McpServerSpec) -> Result<Vec<McpTool>, HarnessError> {
        let mut state = self.state.lock().await;
        if let Some(cached) = state.server_tools.get(&spec.name) {
            return Ok(cached.clone());
        }
        let client = self.ensure_client(&mut state, spec).await?;
        let tools = client.list_tools().await.map_err(|error| {
            HarnessError::mcp_error(
                error.reason(),
                Some(&spec.name),
                None,
                Stage::Load,
                format!(
                    "MCP server '{}' tools/list failed: {}",
                    spec.name, error.message
                ),
            )
        })?;
        state.server_tools.insert(spec.name.clone(), tools.clone());
        Ok(tools)
    }

    async fn call(
        &self,
        spec: &McpServerSpec,
        tool: &str,
        args: Value,
        timeout: Duration,
    ) -> Result<McpCallOutcome, McpClientError> {
        let mut state = self.state.lock().await;
        loop {
            let client = self
                .ensure_client(&mut state, spec)
                .await
                .map_err(|error| McpClientError::new(McpClientErrorKind::Init, error.message))?;
            match client
                .call_tool(tool, args.clone(), timeout, MCP_MRTR_MAX_ROUNDS)
                .await
            {
                Ok(outcome) => return Ok(outcome),
                Err(error) if matches!(error.kind, McpClientErrorKind::Closed) => {
                    // DR-010 §4：**同 run 每 server 最多重启 1 次**（run 级全局计数，
                    // 不是 per-call）。额度用满后再 Closed → 不再 spawn，直接由调用方
                    // 映射 `TOOL_ERROR{reason:"transport_closed"}`。
                    let restarts = state
                        .clients
                        .get(&spec.name)
                        .map_or(0, |slot| slot.restarts);
                    if restarts >= MCP_MAX_RESTARTS_PER_RUN {
                        return Err(error);
                    }
                    if let Some(slot) = state.clients.get_mut(&spec.name) {
                        if let Some(client) = slot.client.take() {
                            client.shutdown().await;
                        }
                        slot.restarts += 1;
                    }
                    continue;
                }
                Err(error) => return Err(error),
            }
        }
    }

    /// 关闭全部连接（best-effort；幂等）。
    pub async fn shutdown(&self) {
        let mut state = self.state.lock().await;
        for slot in state.clients.values_mut() {
            if let Some(client) = slot.client.take() {
                client.shutdown().await;
            }
        }
        state.server_tools.clear();
    }
}

/// 单个 agent 的 MCP 视图（plan/allow 隔离；连接与 tools 缓存共享自 [`McpPool`]）。
pub struct McpRun {
    plan: Arc<McpPlan>,
    pool: Arc<McpPool>,
    bindings: tokio::sync::Mutex<BTreeMap<String, McpTool>>,
}

impl McpRun {
    /// 构造（不立即连接；懒启动）。
    pub fn new(plan: Arc<McpPlan>, pool: Arc<McpPool>) -> Arc<Self> {
        Arc::new(Self {
            plan,
            pool,
            bindings: tokio::sync::Mutex::new(BTreeMap::new()),
        })
    }

    /// 本 agent 的 MCP 计划。
    pub fn plan(&self) -> &McpPlan {
        &self.plan
    }

    /// 启动提示（stderr；脱敏；调用方决定是否打印）。
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

    /// `tools/list`（每 server 缓存共享；总 schema 大小受限）。
    pub async fn list_tools(&self) -> Result<Vec<McpTool>, HarnessError> {
        let mut all = Vec::new();
        let mut schema_bytes = 0usize;
        for server in &self.plan.servers {
            for mut tool in self.pool.tools_for(server).await? {
                tool.server = server.name.clone();
                schema_bytes = schema_bytes.saturating_add(
                    serde_json::to_string(&tool.input_schema)
                        .map(|value| value.len())
                        .unwrap_or(0),
                );
                if schema_bytes > crate::constants::MCP_TOOLS_LIST_MAX_BYTES {
                    return Err(HarnessError::mcp_error(
                        "init_failed",
                        Some(&server.name),
                        None,
                        Stage::Load,
                        "MCP tools/list schema exceeds the configured size limit",
                    ));
                }
                if !tool.input_schema.is_object() {
                    eprintln!(
                        "warning: skipping MCP tool '{}' (inputSchema is not an object)",
                        tool.name
                    );
                    continue;
                }
                tool.description =
                    truncate_chars(&tool.description, MAX_MCP_TOOL_DESCRIPTION_CHARS);
                all.push(tool);
            }
        }
        all.sort_by_key(|tool| tool.full_name());
        Ok(all)
    }

    /// 绑定 LLM 可见函数名 → 原始 MCP 工具（归一化/截断后的反向映射）。
    pub async fn bind_tools(&self, bound: &[McpBoundTool]) {
        let mut bindings = self.bindings.lock().await;
        for item in bound {
            bindings.insert(item.def.name.clone(), item.original.clone());
        }
    }

    /// 执行 MCP 工具（进程内）；失败返回 `TOOL_ERROR` JSON 内容（喂回 LLM）。
    pub async fn execute_tool(&self, function_name: &str, args: &Value) -> (bool, String) {
        let bound = {
            let bindings = self.bindings.lock().await;
            bindings.get(function_name).cloned()
        };
        let (server, tool) = match bound {
            Some(tool) => (tool.server.clone(), tool.name.clone()),
            None => {
                let server_names: Vec<&str> = self
                    .plan
                    .servers
                    .iter()
                    .map(|spec| spec.name.as_str())
                    .collect();
                match parse_tool_name_for_servers(function_name, server_names.iter().copied()) {
                    Some((server, tool)) => (server.to_string(), tool.to_string()),
                    None => {
                        return (
                            false,
                            crate::tools::tool_error_content(&ToolFailure::new(
                                "unknown_mcp_tool",
                                format!("invalid MCP tool name '{function_name}'"),
                            )),
                        )
                    }
                }
            }
        };
        let Some(spec) = self.plan.server(&server) else {
            return (
                false,
                crate::tools::tool_error_content(&ToolFailure::new(
                    "server_not_found",
                    format!("MCP server '{server}' is not configured for this agent"),
                )),
            );
        };
        if !args.is_object() {
            return (
                false,
                crate::tools::tool_error_content(&ToolFailure::new(
                    "invalid_args",
                    "MCP tool arguments must be a JSON object",
                )),
            );
        }
        if !self.plan.tool_allowed(&format!("mcp__{server}__{tool}"))
            && !self.plan.allow.is_empty()
            && !self.plan.allow.iter().any(|entry| {
                entry.ends_with(&format!("__{tool}"))
                    && entry.starts_with(&format!("mcp__{server}__"))
            })
        {
            return (
                false,
                crate::tools::tool_error_content(&ToolFailure::new(
                    "tool_not_allowed",
                    format!("MCP tool 'mcp__{server}__{tool}' is not allowed by this agent"),
                )),
            );
        }
        let timeout = Duration::from_millis(spec.timeout_ms);
        match self.pool.call(spec, &tool, args.clone(), timeout).await {
            Ok(outcome) => {
                let (text, truncated) = truncate_result(&outcome.text);
                if truncated {
                    let payload = serde_json::json!({
                        "truncated": true,
                        "is_error": outcome.is_error,
                        "text": text,
                    });
                    return (!outcome.is_error, payload.to_string());
                }
                (!outcome.is_error, text)
            }
            Err(error) => (
                false,
                crate::tools::tool_error_content(&ToolFailure::new(error.reason(), error.message)),
            ),
        }
    }

    /// 关闭本 run 的连接（转给共享 pool；幂等）。
    pub async fn shutdown(&self) {
        self.pool.shutdown().await;
    }
}

/// 映射后的绑定：LLM 可见 `ToolDef` + 原始 MCP 工具（反向映射）。
#[derive(Debug, Clone)]
pub struct McpBoundTool {
    /// 发给 LLM 的工具定义（函数名已归一化到 `^[a-z][a-z0-9_]{0,63}$`）。
    pub def: ToolDef,
    /// 原始 MCP 工具。
    pub original: McpTool,
}

/// 归一化 LLM 函数名：`^[a-z][a-z0-9_]{0,63}$`；非法字符→`_`，超长/冲突截断+短 hash。
fn normalized_function_name(
    server: &str,
    tool: &str,
    original_full: &str,
    taken: &mut BTreeSet<String>,
) -> String {
    // REV-098 P2⑥：server 段同样归一化（server name 允许 `-`，不能直接进函数名）。
    let norm_server = normalize_component(server, "server");
    let mut norm_tool = normalize_component(tool, "tool");
    let budget = 64usize.saturating_sub("mcp__".len() + "__".len());
    let prefix = |server: &str| format!("mcp__{server}__");
    let mut candidate = format!("{}{}", prefix(&norm_server), norm_tool);
    if candidate.chars().count() > 64 || taken.contains(&candidate) {
        let hash = short_hash(original_full);
        let server_keep = norm_server.chars().count().min(budget / 2);
        let server_part: String = norm_server.chars().take(server_keep).collect();
        let remaining = budget.saturating_sub(server_keep + 9).max(1);
        norm_tool = norm_tool.chars().take(remaining).collect();
        candidate = format!("{}{}_{}", prefix(&server_part), norm_tool, hash);
        while candidate.chars().count() > 64 && !norm_tool.is_empty() {
            norm_tool.pop();
            candidate = format!("{}{}_{}", prefix(&server_part), norm_tool, hash);
        }
    }
    taken.insert(candidate.clone());
    candidate
}

/// 归一化单个函数名段：小写/保留 `_`，其余→`_`，空→fallback。
fn normalize_component(value: &str, fallback: &str) -> String {
    let normalized: String = value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '_' {
                ch.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect();
    if normalized.is_empty() {
        fallback.to_string()
    } else {
        normalized
    }
}

/// FNV-1a 短 hash（函数名冲突/截断后缀；无需密码学强度）。
fn short_hash(value: &str) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in value.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x1000_0000_01b3);
    }
    format!("{:08x}", (hash >> 32) as u32)
}

/// 截断工具结果到 `MCP_TOOL_RESULT_MAX_CHARS`（chars；返回是否截断）。
pub fn truncate_result(value: &str) -> (String, bool) {
    if value.chars().count() <= MCP_TOOL_RESULT_MAX_CHARS {
        return (value.to_string(), false);
    }
    let head: String = value.chars().take(MCP_TOOL_RESULT_MAX_CHARS).collect();
    (format!("{head}…[mcp result truncated]"), true)
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
    async fn connect(
        &self,
        _spec: &McpServerSpec,
        _assets_dir: &Path,
    ) -> Result<Arc<dyn McpClient>, McpClientError> {
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

    fn pool(factory: Arc<FakeMcpFactory>) -> Arc<McpPool> {
        McpPool::new(factory, std::path::PathBuf::from("."))
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
                "http_insecure",
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
        assert_eq!(tools[0].def.name, "mcp__alpha__echo");
        assert_eq!(tools[0].def.source, "mcp:alpha");
        assert_eq!(
            tools[0].def.description.chars().count(),
            crate::constants::MAX_MCP_TOOL_DESCRIPTION_CHARS
        );
        assert_eq!(tools[0].def.parameters["type"], "object");

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
        let run = McpRun::new(Arc::new(plan), pool(factory));
        let tools = run.list_tools().await.expect("tools");
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].server, "alpha");
        // run 级缓存：再次 list 不重新连接。
        assert_eq!(run.list_tools().await.expect("cached").len(), 1);
        assert_eq!(client.connects.load(std::sync::atomic::Ordering::SeqCst), 1);

        let (ok, text) = run.execute_tool("mcp__alpha__echo", &json!({"q": 1})).await;
        assert!(ok);
        // 超限结果 → 机器可读 JSON（truncated:true）；不把 4 万字符全文塞回上下文。
        // REV-100 #2：截断标记统一在**最终 ToolResult** 上（Fake/Real 同一路径）。
        let payload: Value = serde_json::from_str(&text).expect("json");
        assert_eq!(payload["truncated"], true);
        let keys: BTreeSet<&str> = payload
            .as_object()
            .expect("object")
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(
            keys,
            BTreeSet::from(["truncated", "is_error", "text"]),
            "final ToolResult truncated shape"
        );
        assert_eq!(payload["is_error"], false);
        let truncated_text = payload["text"].as_str().expect("text");
        assert!(
            truncated_text.chars().count() <= MCP_TOOL_RESULT_MAX_CHARS + 64,
            "bounded head"
        );
        assert!(truncated_text.ends_with("…[mcp result truncated]"));
        assert!(text.len() < 40_000);
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
        let run = McpRun::new(Arc::new(plan), pool(factory));
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
        let run = McpRun::new(Arc::new(plan), pool(factory));
        let (ok, text) = run.execute_tool("mcp__alpha__echo", &json!({})).await;
        assert!(ok);
        assert_eq!(text, "recovered");
        assert_eq!(client.call_count(), 2, "one restart then retry");
        assert_eq!(client.connects.load(std::sync::atomic::Ordering::SeqCst), 2);
    }

    /// REV-100 #1：DR-010 §4「同 run 最多重启 1 次」是**每 run 每 server 全局**口径。
    ///
    /// 连续 crash 场景：首个 call 内 1 次重启（connect=2），后续 call 再 Closed 不再
    /// spawn，直接返回 `TOOL_ERROR{reason:"transport_closed"}`。
    #[tokio::test]
    async fn same_run_restarts_at_most_once_across_calls() {
        let client = FakeMcpClient::new()
            .with_default_outcome("recovered", false)
            .fail_closed_times(3);
        let factory = FakeMcpFactory::new().with_client("alpha", client.clone());
        let plan = McpPlan {
            servers: vec![spec("alpha", 1000)],
            allow: vec![],
        };
        let run = McpRun::new(Arc::new(plan), pool(factory));

        // call#1：Closed → 重启 1 次（connect=2）→ 仍 Closed（额度用满）→ TOOL_ERROR。
        let (ok, text) = run.execute_tool("mcp__alpha__echo", &json!({})).await;
        assert!(!ok);
        let payload: Value = serde_json::from_str(&text).expect("json");
        assert_eq!(payload["error"]["reason"], json!("transport_closed"));
        assert_eq!(client.connects.load(std::sync::atomic::Ordering::SeqCst), 2);

        // call#2：再 Closed → 不再 spawn（connect 仍 2），TOOL_ERROR transport_closed。
        let (ok, text) = run.execute_tool("mcp__alpha__echo", &json!({})).await;
        assert!(!ok);
        let payload: Value = serde_json::from_str(&text).expect("json");
        assert_eq!(payload["error"]["reason"], json!("transport_closed"));
        assert_eq!(client.connects.load(std::sync::atomic::Ordering::SeqCst), 2);

        // call#3：transport 恢复（无 Closed）→ 正常成功。
        let (ok, text) = run.execute_tool("mcp__alpha__echo", &json!({})).await;
        assert!(ok);
        assert_eq!(text, "recovered");
        assert_eq!(client.connects.load(std::sync::atomic::Ordering::SeqCst), 2);
        assert_eq!(client.call_count(), 4, "2+1+1 call attempts");
    }

    /// REV-100 #3：`http://` 未显式 allow → `HARNESS_MCP_ERROR{reason:"http_insecure"}`。
    #[test]
    fn insecure_http_reason_is_http_insecure() {
        let value = json!({
            "mcpServers": {"bad": {"url": "http://localhost:8080/mcp"}}
        });
        let error = McpRegistry::from_value(&value, &|_| None)
            .expect_err("insecure http must fail closed at registry parse");
        assert_eq!(error.kind, zgraph_kernel::HarnessErrorKind::Mcp);
        assert_eq!(error.code, "HARNESS_MCP_ERROR");
        assert_eq!(error.detail["reason"], json!("http_insecure"));
        assert_eq!(error.detail["server"], json!("bad"));
    }

    #[tokio::test]
    async fn run_reports_timeout_and_is_error() {
        let client = FakeMcpClient::new().with_default_outcome("boom", true);
        let factory = FakeMcpFactory::new().with_client("alpha", client.clone());
        let plan = McpPlan {
            servers: vec![spec("alpha", 5)],
            allow: vec![],
        };
        let run = McpRun::new(Arc::new(plan), pool(factory));
        let (ok, text) = run.execute_tool("mcp__alpha__echo", &json!({})).await;
        assert!(!ok, "isError result is a tool error");
        assert_eq!(text, "boom");
        run.shutdown().await;
    }

    #[test]
    fn parses_server_names_containing_double_underscore() {
        let servers = ["my__server", "alpha"];
        assert_eq!(
            parse_tool_name_for_servers("mcp__my__server__echo", servers.iter().copied()),
            Some(("my__server", "echo"))
        );
        assert_eq!(
            parse_tool_name_for_servers("mcp__my__server__a__b", servers.iter().copied()),
            Some(("my__server", "a__b"))
        );
        assert_eq!(
            parse_tool_name_for_servers("mcp__ghost__echo", servers.iter().copied()),
            None
        );
    }

    #[test]
    fn function_names_are_normalized_and_reversible() {
        let plan = McpPlan {
            servers: vec![spec("alpha", 1000)],
            allow: vec![],
        };
        let discovered = vec![
            tool("alpha", "My.Tool-Name", "d"),
            tool("alpha", &"x".repeat(100), "d"),
        ];
        let requested: Vec<String> = discovered.iter().map(McpTool::full_name).collect();
        let bound = map_tools(&plan, &requested, &discovered).expect("mapped");
        assert_eq!(bound.len(), 2);
        for item in &bound {
            assert!(
                item.def.name.len() <= 64
                    && item
                        .def
                        .name
                        .chars()
                        .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '_'),
                "function name contract: {}",
                item.def.name
            );
            assert!(item.def.name.starts_with("mcp__alpha__"));
        }
        assert_eq!(bound[0].def.name, "mcp__alpha__my_tool_name");
        assert!(bound[1].def.name.len() <= 64 && bound[1].def.name.contains('_'));
    }

    #[test]
    fn normalized_function_name_normalizes_server_segment() {
        let mut taken = BTreeSet::new();
        assert_eq!(
            normalized_function_name("a-b", "echo", "mcp__a-b__echo", &mut taken),
            "mcp__a_b__echo"
        );
        let long_server = "s".repeat(80);
        let long_tool = "t".repeat(80);
        let name = normalized_function_name(
            &long_server,
            &long_tool,
            &format!("mcp__{long_server}__{long_tool}"),
            &mut taken,
        );
        assert!(name.chars().count() <= 64, "name too long: {name}");
        assert!(name.starts_with("mcp__"));
        assert!(
            name.chars()
                .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '_'),
            "contract violation: {name}"
        );
    }

    #[tokio::test]
    async fn truncated_result_is_machine_readable_json() {
        let client = FakeMcpClient::new().with_default_outcome("z".repeat(40_000), false);
        let factory = FakeMcpFactory::new().with_client("alpha", client);
        let plan = McpPlan {
            servers: vec![spec("alpha", 1000)],
            allow: vec![],
        };
        let run = McpRun::new(
            Arc::new(plan),
            McpPool::new(factory, std::path::PathBuf::from(".")),
        );
        let (ok, text) = run.execute_tool("mcp__alpha__echo", &json!({})).await;
        assert!(ok);
        let payload: Value = serde_json::from_str(&text).expect("json");
        assert_eq!(payload["truncated"], true);
    }

    #[test]
    fn result_truncation_is_bounded() {
        let (text, truncated) = truncate_result(&"y".repeat(40_000));
        assert!(truncated);
        assert!(text.chars().count() <= crate::constants::MCP_TOOL_RESULT_MAX_CHARS + 64);
        assert!(text.contains("mcp result truncated"));
        assert_eq!(truncate_result("small"), ("small".to_string(), false));
    }
}
