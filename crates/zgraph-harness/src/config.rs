//! AgentConfig 资产加载与校验（05 §5.2；关闭审计 M-01）。
//!
//! 资产布局：`<assets_dir>/agents/*.json`，**按文件内 `id` 索引**（单层、不递归、
//! 文件名排序保证确定性）；运行期每次 `agent_call` 执行懒加载 + 全量校验，**不缓存**。
//! 加载失败一律 `HARNESS_CONFIG_LOAD`（`detail` 见 05 §5.2.7）。
//!
//! 与 04/02 的边界：`tools` / `skills` 归 Harness Loading（`HARNESS_CONFIG_LOAD`），
//! 图加载期只判 `agent_id` 索引命中（`DSL_AGENT_NOT_FOUND`）。

use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};

use serde_json::{json, Map, Value};

use crate::constants::{
    DEFAULT_CONTEXT_WINDOW_TOKENS, DEFAULT_LLM_MAX_RETRIES, DEFAULT_LLM_MAX_TOKENS,
    DEFAULT_LLM_TEMPERATURE, DEFAULT_LLM_TIMEOUT_MS, DEFAULT_MAX_ITERATIONS,
    DEFAULT_NODE_TIMEOUT_MS, MAX_AGENT_CONFIG_BYTES, MAX_PROMPT_BYTES, MAX_TOOLS_PER_AGENT,
    SUPPORTED_SCHEMA_VERSION,
};
use crate::error::HarnessError;
use crate::llm::{ModelRef, ProviderKind, SecretString};
use crate::tools::{is_tool_name, ToolDef, ToolRegistry};

/// AgentConfig 的 `model` 段（05 §5.2.2）。
#[derive(Debug, Clone, PartialEq)]
pub struct ModelConfig {
    /// provider 类别。
    pub provider: ProviderKind,
    /// 模型名。
    pub name: String,
    /// OpenAI-compatible 端点（deepseek 恒 `None`）。
    pub base_url: Option<String>,
    /// API key 环境变量名（不内联密钥）。
    pub api_key_env: String,
    /// 采样温度（0.0 ≤ x ≤ 2.0）。
    pub temperature: f32,
    /// 单次响应 token 上限。
    pub max_tokens: u32,
    /// 预算总来源（上下文窗口）。
    pub context_window_tokens: u32,
}

impl ModelConfig {
    /// 派生 LLM 请求用的 [`ModelRef`]。
    pub fn model_ref(&self) -> ModelRef {
        ModelRef {
            provider: self.provider,
            name: self.name.clone(),
            base_url: self.base_url.clone(),
        }
    }
}

/// 校验后的 AgentConfig（05 §5.2.2 字段级；未知字段一律拒绝）。
#[derive(Debug, Clone, PartialEq)]
pub struct AgentConfig {
    /// Agent 唯一标识。
    pub id: String,
    /// 人类可读名。
    pub name: String,
    /// schema 版本（仅接受 1）。
    pub schema_version: u64,
    /// 模型段。
    pub model: ModelConfig,
    /// 角色 prompt 文件（相对 AgentConfig 文件目录）。
    pub role_prompt_file: String,
    /// 约束文件（可选）。
    pub constraints_file: Option<String>,
    /// 本 Agent 可见工具引用（顺序即发给 LLM 的 schema 顺序）。
    pub tools: Vec<String>,
    /// 节点端口 → Agent 输入字段（05 §5.2.2/§5.9.3）。
    pub input_mapping: BTreeMap<String, String>,
    /// LLM 请求轮次上限。
    pub max_iterations: u32,
    /// 单次 LLM 调用超时。
    pub llm_timeout_ms: u64,
    /// 仅 Establish 阶段生效的重试次数。
    pub llm_max_retries: u32,
    /// 实例超时建议值（DSL 未声明 `timeout_ms` 时由调度器回退使用）。
    pub node_timeout_ms: u64,
}

/// 加载完成、可直接进入 loop 的不可变 Agent 值对象。
#[derive(Clone)]
pub struct LoadedAgent {
    /// 校验后的配置。
    pub config: AgentConfig,
    /// role prompt 原文（不做模板渲染）。
    pub role_prompt: String,
    /// 约束段原文（`None` 或空 → 整段省略）。
    pub constraints: Option<String>,
    /// 已解析的密钥（不落日志）。
    pub api_key: SecretString,
    /// 已按 `config.tools` 顺序解析的工具定义。
    pub tools: Vec<ToolDef>,
}

impl std::fmt::Debug for LoadedAgent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LoadedAgent")
            .field("config", &self.config)
            .field("role_prompt_chars", &self.role_prompt.chars().count())
            .field("constraints", &self.constraints)
            .field("tools", &self.config.tools)
            .field("api_key", &"<redacted>")
            .finish()
    }
}

impl LoadedAgent {
    /// 派生 LLM 请求用的 [`ModelRef`]。
    pub fn model_ref(&self) -> ModelRef {
        self.config.model.model_ref()
    }

    /// 已解析工具按声明顺序生成 schema（05 §5.6.3）。
    pub fn tool_schemas(&self) -> Vec<crate::tools::ToolSchema> {
        self.tools.iter().map(|tool| tool.schema()).collect()
    }
}

/// 顶层允许键（05 §5.2.2 + `$schema` 白名单）。
const TOP_LEVEL_KEYS: [&str; 13] = [
    "id",
    "name",
    "schema_version",
    "model",
    "role_prompt_file",
    "constraints_file",
    "tools",
    "input_mapping",
    "max_iterations",
    "llm_timeout_ms",
    "llm_max_retries",
    "node_timeout_ms",
    "$schema",
];

/// v1 保留字段：出现即 `HARNESS_CONFIG_LOAD{reason:"reserved_field"}`（05 §5.10.2）。
const RESERVED_KEYS: [&str; 6] = [
    "output_mapping",
    "output_schema",
    "memory",
    "kb_ids",
    "tools_concurrency",
    "provider_fallback",
];

/// `model` 段允许键。
const MODEL_KEYS: [&str; 7] = [
    "provider",
    "name",
    "base_url",
    "api_key_env",
    "temperature",
    "max_tokens",
    "context_window_tokens",
];

/// 资产索引：`id` → AgentConfig 文件路径（按文件名排序读入，确定性）。
pub fn agent_index(assets_dir: &Path) -> Result<BTreeMap<String, PathBuf>, HarnessError> {
    let assets_dir = absolute(assets_dir);
    let agents_dir = assets_dir.join("agents");
    let entries = match std::fs::read_dir(&agents_dir) {
        Ok(entries) => entries,
        // 目录不存在 = 空索引（05 §5.2.4 步 1：运行期兜底 agent_not_indexed）。
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(BTreeMap::new()),
        Err(error) => {
            return Err(config_error(
                "file_unreadable",
                Some(&agents_dir.display().to_string()),
                None,
                None,
                None,
                format!("read agents dir {}: {error}", agents_dir.display()),
            ))
        }
    };
    let mut files: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.is_file() && is_json_file(path))
        .collect();
    files.sort_by(|a, b| a.file_name().cmp(&b.file_name()));
    let mut index: BTreeMap<String, PathBuf> = BTreeMap::new();
    for path in files {
        let value = read_json_file(&path, MAX_AGENT_CONFIG_BYTES)?;
        let id = value
            .as_object()
            .and_then(|object| object.get("id"))
            .and_then(Value::as_str)
            .map(str::to_string)
            .ok_or_else(|| {
                config_error(
                    "missing_field",
                    Some(&path.display().to_string()),
                    Some("$.id"),
                    Some(Value::from("string")),
                    None,
                    format!("agent asset {} missing string field 'id'", path.display()),
                )
            })?;
        // 重复 id：保留文件名排序后的第一个（DSL 加载期已保证唯一；运行期确定性优先）。
        index.entry(id).or_insert(path);
    }
    Ok(index)
}

/// `AgentConfig.node_timeout_ms` 合法范围下界（05 §5.2.2 V13）。
pub(crate) const NODE_TIMEOUT_MIN_MS: u64 = 1_000;

/// `AgentConfig.node_timeout_ms` 合法范围上界（05 §5.2.2 V13）。
pub(crate) const NODE_TIMEOUT_MAX_MS: u64 = 3_600_000;

/// 轻量同步读取 `AgentConfig.node_timeout_ms`（ISSUE-076 / DR-002 §4）。
///
/// 供 [`crate::executor::HarnessExecutor`] 的 `timeout_hint` 在**派发前**同步
/// 给出 `agent_call` 实例超时回退值：只扫描 `<assets_dir>/agents/*.json`
/// （按文件名排序，与 [`agent_index`] 一致），按文件内 `id` 命中后读取
/// `node_timeout_ms`；**不**解析 model/tools/prompt、不读密钥、不查 `ToolRegistry`。
///
/// 返回 `None` 的情形（内核回退 [`DEFAULT_NODE_TIMEOUT_MS`]；运行期 Loading 仍会
/// 以 `HARNESS_CONFIG_LOAD` 报错）：资产目录/文件缺失、JSON 非法、`id` 未命中、
/// 字段缺失/非整数、值越界（[`NODE_TIMEOUT_MIN_MS`]..=[`NODE_TIMEOUT_MAX_MS`]）。
pub(crate) fn peek_agent_node_timeout(assets_dir: &Path, agent_id: &str) -> Option<u64> {
    let agents_dir = absolute(assets_dir).join("agents");
    let entries = std::fs::read_dir(&agents_dir).ok()?;
    let mut files: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.is_file() && is_json_file(path))
        .collect();
    files.sort_by(|a, b| a.file_name().cmp(&b.file_name()));
    for path in files {
        let Ok(value) = read_json_file(&path, MAX_AGENT_CONFIG_BYTES) else {
            continue;
        };
        let Some(object) = value.as_object() else {
            continue;
        };
        if object.get("id").and_then(Value::as_str) != Some(agent_id) {
            continue;
        }
        // 命中即返回：与 `agent_index` 的「文件名排序取第一个 id」语义一致。
        return object
            .get("node_timeout_ms")
            .and_then(Value::as_u64)
            .filter(|value| (NODE_TIMEOUT_MIN_MS..=NODE_TIMEOUT_MAX_MS).contains(value));
    }
    None
}

/// 加载并校验一个 Agent（05 §5.2.4 确定性流程 1–10）。
///
/// - `registry`：已发现的工具注册表（步 8 查表）；
/// - `fake_mode`：`--fake-llm` 下跳过 `api_key_env` 检查（步 9）。
pub fn load_agent(
    assets_dir: &Path,
    agent_id: &str,
    registry: &ToolRegistry,
    fake_mode: bool,
) -> Result<LoadedAgent, HarnessError> {
    let assets_dir = absolute(assets_dir);
    let index = agent_index(&assets_dir)?;
    let path = index.get(agent_id).cloned().ok_or_else(|| {
        config_error(
            "agent_not_indexed",
            Some(&assets_dir.join("agents").display().to_string()),
            Some("$.id"),
            Some(Value::from(agent_id.to_string())),
            None,
            format!(
                "agent '{agent_id}' is not indexed under {}",
                assets_dir.display()
            ),
        )
    })?;
    let value = read_json_file(&path, MAX_AGENT_CONFIG_BYTES)?;
    let config = parse_agent_config(&value, &path, agent_id)?;
    let agent_dir = path.parent().unwrap_or(Path::new("."));
    let role_prompt = read_prompt(
        agent_dir,
        &assets_dir,
        &config.role_prompt_file,
        "$.role_prompt_file",
    )?;
    let constraints = match &config.constraints_file {
        None => None,
        Some(file) => Some(read_prompt(
            agent_dir,
            &assets_dir,
            file,
            "$.constraints_file",
        )?),
    };
    let tools = resolve_tools(&config, registry)?;
    let api_key = resolve_api_key(&config, fake_mode)?;
    Ok(LoadedAgent {
        config,
        role_prompt,
        constraints,
        api_key,
        tools,
    })
}

// ---------------------------------------------------------------------------
// 解析与 V1–V16 校验
// ---------------------------------------------------------------------------

/// 解析 AgentConfig JSON（未知字段拒绝 + V1–V16）。
fn parse_agent_config(
    value: &Value,
    path: &Path,
    requested_id: &str,
) -> Result<AgentConfig, HarnessError> {
    let object = value.as_object().ok_or_else(|| {
        config_error(
            "type_mismatch",
            Some(&path.display().to_string()),
            Some("$"),
            Some(Value::from("object")),
            Some(Value::from(json_type_name(value))),
            "AgentConfig must be a JSON object".to_string(),
        )
    })?;
    reject_unknown(object, path, "$", &TOP_LEVEL_KEYS, &RESERVED_KEYS)?;

    // V1/V2：id 与索引键一致 + 命名规则。
    let id = required_string(object, "id", path, "$.id")?;
    if id != requested_id {
        return Err(config_error(
            "unsupported_value",
            Some(&path.display().to_string()),
            Some("$.id"),
            Some(Value::from(requested_id.to_string())),
            Some(Value::from(id.clone())),
            format!("agent id '{id}' does not match requested '{requested_id}'"),
        ));
    }
    if !is_agent_id(&id) {
        return Err(config_error(
            "unsupported_value",
            Some(&path.display().to_string()),
            Some("$.id"),
            Some(Value::from("^[a-z][a-z0-9_-]{0,63}$")),
            Some(Value::from(id.clone())),
            format!("invalid agent id '{id}'"),
        ));
    }

    // V3：name。
    let name = required_string(object, "name", path, "$.name")?;
    let name_chars = name.chars().count();
    if name_chars == 0 || name_chars > 128 {
        return Err(config_error(
            "out_of_range",
            Some(&path.display().to_string()),
            Some("$.name"),
            Some(Value::from("1..=128 characters")),
            Some(Value::from(name_chars)),
            "agent name length must be 1..=128".to_string(),
        ));
    }

    // V4：schema_version。
    let schema_version = optional_u64(object, "schema_version", path, "$.schema_version")?
        .unwrap_or(SUPPORTED_SCHEMA_VERSION);
    if schema_version != SUPPORTED_SCHEMA_VERSION {
        return Err(config_error(
            "unsupported_schema_version",
            Some(&path.display().to_string()),
            Some("$.schema_version"),
            Some(json!([SUPPORTED_SCHEMA_VERSION])),
            Some(Value::from(schema_version)),
            format!("unsupported schema_version {schema_version}"),
        ));
    }

    let model_value = object.get("model").ok_or_else(|| {
        config_error(
            "missing_field",
            Some(&path.display().to_string()),
            Some("$.model"),
            Some(Value::from("object")),
            None,
            "AgentConfig missing required field 'model'".to_string(),
        )
    })?;
    let model = parse_model(model_value, path)?;

    let role_prompt_file = required_string(object, "role_prompt_file", path, "$.role_prompt_file")?;
    if role_prompt_file.trim().is_empty() {
        return Err(config_error(
            "unsupported_value",
            Some(&path.display().to_string()),
            Some("$.role_prompt_file"),
            Some(Value::from("non-empty string")),
            Some(Value::from(role_prompt_file.clone())),
            "role_prompt_file must not be empty".to_string(),
        ));
    }
    let constraints_file = optional_string(object, "constraints_file", path, "$.constraints_file")?;

    let tools = parse_tools(object, path)?;
    let input_mapping = parse_input_mapping(object, path)?;
    let max_iterations = parse_max_iterations(object, path)?;
    let (llm_timeout_ms, llm_max_retries, node_timeout_ms) = parse_timeouts(object, path)?;

    Ok(AgentConfig {
        id,
        name,
        schema_version,
        model,
        role_prompt_file,
        constraints_file,
        tools,
        input_mapping,
        max_iterations,
        llm_timeout_ms,
        llm_max_retries,
        node_timeout_ms,
    })
}

/// 解析 `model` 段（V5–V11；ISSUE-081 拆分，逐字段校验见下方子函数）。
fn parse_model(value: &Value, path: &Path) -> Result<ModelConfig, HarnessError> {
    let empty = Map::new();
    let object = value.as_object().unwrap_or(&empty);
    if !value.is_object() {
        return Err(type_error(
            path,
            "$.model",
            "object",
            json_type_name(value),
            "model must be an object",
        ));
    }
    reject_unknown(object, path, "$.model", &MODEL_KEYS, &[])?;
    let provider = parse_provider(object, path)?;
    let name = parse_model_name(object, path)?;
    // V11 先于 V10：max_tokens 上限依赖 context_window_tokens。
    let context_window_tokens = parse_context_window(object, path)?;
    let max_tokens = parse_max_tokens(object, path, context_window_tokens)?;
    let base_url = parse_base_url(object, path, provider)?;
    let api_key_env = parse_api_key_env(object, path)?;
    let temperature = parse_temperature(object, path)?;
    Ok(ModelConfig {
        provider,
        name,
        base_url,
        api_key_env,
        temperature,
        max_tokens,
        context_window_tokens,
    })
}

/// V5：`model.provider`。
fn parse_provider(object: &Map<String, Value>, path: &Path) -> Result<ProviderKind, HarnessError> {
    let provider_raw = required_string(object, "provider", path, "$.model.provider")?;
    ProviderKind::parse(&provider_raw).ok_or_else(|| {
        config_error(
            "unsupported_value",
            Some(&path.display().to_string()),
            Some("$.model.provider"),
            Some(json!(["deepseek", "openai_compatible"])),
            Some(Value::from(provider_raw.clone())),
            format!("unsupported model provider '{provider_raw}'"),
        )
    })
}

/// 模型名格式校验（`^[A-Za-z0-9._:-]{1,128}$`）。
fn parse_model_name(object: &Map<String, Value>, path: &Path) -> Result<String, HarnessError> {
    let name = required_string(object, "name", path, "$.model.name")?;
    if !is_model_name(&name) {
        return Err(config_error(
            "unsupported_value",
            Some(&path.display().to_string()),
            Some("$.model.name"),
            Some(Value::from("^[A-Za-z0-9._:-]{1,128}$")),
            Some(Value::from(name.clone())),
            format!("invalid model name '{name}'"),
        ));
    }
    Ok(name)
}

/// V11：`context_window_tokens`（4096..=2_000_000，默认 65536）。
fn parse_context_window(object: &Map<String, Value>, path: &Path) -> Result<u32, HarnessError> {
    let value = optional_u64(
        object,
        "context_window_tokens",
        path,
        "$.model.context_window_tokens",
    )?
    .unwrap_or(u64::from(DEFAULT_CONTEXT_WINDOW_TOKENS));
    if !(4_096..=2_000_000).contains(&value) {
        return Err(config_error(
            "out_of_range",
            Some(&path.display().to_string()),
            Some("$.model.context_window_tokens"),
            Some(json!([4096, 2_000_000])),
            Some(Value::from(value)),
            "context_window_tokens out of range".to_string(),
        ));
    }
    Ok(u32::try_from(value).unwrap_or(u32::MAX))
}

/// V10：`max_tokens`（1..=32768，且 ≤ 20% context window）。
fn parse_max_tokens(
    object: &Map<String, Value>,
    path: &Path,
    context_window_tokens: u32,
) -> Result<u32, HarnessError> {
    let value = optional_u64(object, "max_tokens", path, "$.model.max_tokens")?
        .unwrap_or(u64::from(DEFAULT_LLM_MAX_TOKENS));
    if !(1..=32_768).contains(&value) {
        return Err(config_error(
            "out_of_range",
            Some(&path.display().to_string()),
            Some("$.model.max_tokens"),
            Some(json!([1, 32_768])),
            Some(Value::from(value)),
            "max_tokens out of range".to_string(),
        ));
    }
    if value.saturating_mul(5) > u64::from(context_window_tokens) {
        return Err(config_error(
            "out_of_range",
            Some(&path.display().to_string()),
            Some("$.model.max_tokens"),
            Some(Value::from("max_tokens <= 0.2 * context_window_tokens")),
            Some(Value::from(value)),
            "max_tokens exceeds 20% of context window".to_string(),
        ));
    }
    Ok(u32::try_from(value).unwrap_or(u32::MAX))
}

/// V8：`api_key_env`（环境变量名，不内联密钥）。
fn parse_api_key_env(object: &Map<String, Value>, path: &Path) -> Result<String, HarnessError> {
    let api_key_env = required_string(object, "api_key_env", path, "$.model.api_key_env")?;
    if !is_env_name(&api_key_env) {
        return Err(config_error(
            "unsupported_value",
            Some(&path.display().to_string()),
            Some("$.model.api_key_env"),
            Some(Value::from("^[A-Z_][A-Z0-9_]{0,63}$")),
            Some(Value::from(api_key_env.clone())),
            format!("invalid api_key_env '{api_key_env}'"),
        ));
    }
    Ok(api_key_env)
}

/// V9：`temperature`（0.0..=2.0，默认 0.2）。
fn parse_temperature(object: &Map<String, Value>, path: &Path) -> Result<f32, HarnessError> {
    let temperature = optional_f64(object, "temperature", path, "$.model.temperature")?
        .unwrap_or(f64::from(DEFAULT_LLM_TEMPERATURE));
    if !(0.0..=2.0).contains(&temperature) {
        return Err(config_error(
            "out_of_range",
            Some(&path.display().to_string()),
            Some("$.model.temperature"),
            Some(json!([0.0, 2.0])),
            Some(Value::from(temperature)),
            "temperature out of range".to_string(),
        ));
    }
    Ok(temperature as f32)
}

/// V6/V7：`base_url` 条件必填与格式。
fn parse_base_url(
    object: &Map<String, Value>,
    path: &Path,
    provider: ProviderKind,
) -> Result<Option<String>, HarnessError> {
    let raw = match object.get("base_url") {
        None | Some(Value::Null) => None,
        Some(Value::String(text)) if text.is_empty() => None,
        Some(Value::String(text)) => Some(text.clone()),
        Some(other) => {
            return Err(type_error(
                path,
                "$.model.base_url",
                "string or null",
                json_type_name(other),
                "base_url must be a string or null",
            ))
        }
    };
    match provider {
        ProviderKind::Deepseek => {
            if raw.is_some() {
                return Err(config_error(
                    "unsupported_value",
                    Some(&path.display().to_string()),
                    Some("$.model.base_url"),
                    Some(Value::from("null for provider=deepseek")),
                    Some(Value::from(raw.unwrap_or_default())),
                    "provider=deepseek requires base_url=null".to_string(),
                ));
            }
            Ok(None)
        }
        ProviderKind::OpenAiCompatible => {
            let Some(base_url) = raw else {
                return Err(config_error(
                    "missing_field",
                    Some(&path.display().to_string()),
                    Some("$.model.base_url"),
                    Some(Value::from("http(s)://... (no trailing slash)")),
                    None,
                    "provider=openai_compatible requires base_url".to_string(),
                ));
            };
            let valid = (base_url.starts_with("http://") || base_url.starts_with("https://"))
                && !base_url.ends_with('/')
                && !base_url.ends_with("/chat/completions")
                && !base_url.contains(char::is_whitespace);
            if !valid {
                return Err(config_error(
                    "unsupported_value",
                    Some(&path.display().to_string()),
                    Some("$.model.base_url"),
                    Some(Value::from(
                        "http(s)://host[/path]，无尾斜杠，不以 /chat/completions 结尾",
                    )),
                    Some(Value::from(base_url.clone())),
                    "invalid base_url format".to_string(),
                ));
            }
            Ok(Some(base_url))
        }
    }
}

/// V14：tools 列表（数量、引用名规则；注册表命中在 `resolve_tools`）。
fn parse_tools(object: &Map<String, Value>, path: &Path) -> Result<Vec<String>, HarnessError> {
    let Some(value) = object.get("tools") else {
        return Ok(Vec::new());
    };
    let Some(array) = value.as_array() else {
        return Err(type_error(
            path,
            "$.tools",
            "array of strings",
            json_type_name(value),
            "tools must be an array",
        ));
    };
    if array.len() > MAX_TOOLS_PER_AGENT {
        return Err(config_error(
            "out_of_range",
            Some(&path.display().to_string()),
            Some("$.tools"),
            Some(Value::from(format!("<= {MAX_TOOLS_PER_AGENT} items"))),
            Some(Value::from(array.len())),
            "too many tools".to_string(),
        ));
    }
    let mut tools = Vec::with_capacity(array.len());
    for (index, item) in array.iter().enumerate() {
        let Some(reference) = item.as_str() else {
            return Err(type_error(
                path,
                &format!("$.tools[{index}]"),
                "string",
                json_type_name(item),
                "tool reference must be a string",
            ));
        };
        let name = reference.rsplit('.').next().unwrap_or(reference);
        if !is_tool_name(name) {
            return Err(config_error(
                "unsupported_value",
                Some(&path.display().to_string()),
                Some(&format!("$.tools[{index}]")),
                Some(Value::from("^[a-z][a-z0-9_]{0,63}$ 或 <module>.<tool>")),
                Some(Value::from(reference.to_string())),
                format!("invalid tool reference '{reference}'"),
            ));
        }
        tools.push(reference.to_string());
    }
    Ok(tools)
}

/// V15：input_mapping（key/value 非空）。
fn parse_input_mapping(
    object: &Map<String, Value>,
    path: &Path,
) -> Result<BTreeMap<String, String>, HarnessError> {
    let Some(value) = object.get("input_mapping") else {
        return Ok(BTreeMap::new());
    };
    let Some(map) = value.as_object() else {
        return Err(type_error(
            path,
            "$.input_mapping",
            "object<string,string>",
            json_type_name(value),
            "input_mapping must be an object",
        ));
    };
    let mut mapping = BTreeMap::new();
    for (key, value) in map {
        let Some(field) = value.as_str() else {
            return Err(type_error(
                path,
                &format!("$.input_mapping.{key}"),
                "string",
                json_type_name(value),
                "input_mapping value must be a string",
            ));
        };
        if key.trim().is_empty() || field.trim().is_empty() {
            return Err(config_error(
                "unsupported_value",
                Some(&path.display().to_string()),
                Some(&format!("$.input_mapping.{key}")),
                Some(Value::from("non-empty port -> field")),
                Some(Value::from(format!("{key} -> {field}"))),
                "input_mapping key/value must not be empty".to_string(),
            ));
        }
        mapping.insert(key.clone(), field.to_string());
    }
    Ok(mapping)
}

/// V12：max_iterations。
fn parse_max_iterations(object: &Map<String, Value>, path: &Path) -> Result<u32, HarnessError> {
    let value = optional_u64(object, "max_iterations", path, "$.max_iterations")?
        .unwrap_or(u64::from(DEFAULT_MAX_ITERATIONS));
    if !(1..=50).contains(&value) {
        return Err(config_error(
            "out_of_range",
            Some(&path.display().to_string()),
            Some("$.max_iterations"),
            Some(json!([1, 50])),
            Some(Value::from(value)),
            "max_iterations out of range".to_string(),
        ));
    }
    Ok(u32::try_from(value).unwrap_or(u32::MAX))
}

/// V13：llm_timeout_ms / llm_max_retries / node_timeout_ms。
fn parse_timeouts(
    object: &Map<String, Value>,
    path: &Path,
) -> Result<(u64, u32, u64), HarnessError> {
    let llm_timeout_ms = optional_u64(object, "llm_timeout_ms", path, "$.llm_timeout_ms")?
        .unwrap_or(DEFAULT_LLM_TIMEOUT_MS);
    if !(1_000..=600_000).contains(&llm_timeout_ms) {
        return Err(range_error(
            path,
            "$.llm_timeout_ms",
            &llm_timeout_ms,
            1_000,
            600_000,
        ));
    }
    let llm_max_retries = optional_u64(object, "llm_max_retries", path, "$.llm_max_retries")?
        .unwrap_or(u64::from(DEFAULT_LLM_MAX_RETRIES));
    if llm_max_retries > 5 {
        return Err(range_error(
            path,
            "$.llm_max_retries",
            &llm_max_retries,
            0,
            5,
        ));
    }
    let node_timeout_ms = optional_u64(object, "node_timeout_ms", path, "$.node_timeout_ms")?
        .unwrap_or(DEFAULT_NODE_TIMEOUT_MS);
    if !(NODE_TIMEOUT_MIN_MS..=NODE_TIMEOUT_MAX_MS).contains(&node_timeout_ms) {
        return Err(range_error(
            path,
            "$.node_timeout_ms",
            &node_timeout_ms,
            NODE_TIMEOUT_MIN_MS,
            NODE_TIMEOUT_MAX_MS,
        ));
    }
    Ok((
        llm_timeout_ms,
        u32::try_from(llm_max_retries).unwrap_or(u32::MAX),
        node_timeout_ms,
    ))
}

// ---------------------------------------------------------------------------
// 文件读取与路径安全（V16）
// ---------------------------------------------------------------------------

/// 解析并读取 prompt/constraints 文件（相对 AgentConfig 文件目录；V16 路径白名单）。
fn read_prompt(
    agent_dir: &Path,
    assets_root: &Path,
    raw: &str,
    json_path: &str,
) -> Result<String, HarnessError> {
    let path = resolve_asset_file(agent_dir, assets_root, raw, json_path)?;
    let bytes = read_bounded(&path, MAX_PROMPT_BYTES)?;
    decode_utf8(bytes, &path)
}

/// V16：`role_prompt_file`/`constraints_file` 路径解析与逃逸拒绝。
///
/// - 相对路径相对**AgentConfig 文件所在目录**；允许绝对路径；
/// - 先做词法归一化（拒绝 `..` 越界），保留 `path_escape` 的快速判定；
/// - 再 `canonicalize`（解析**全部符号链接**）后用真实路径重新做前缀校验：
///   必须落在规范化后的资产根或 CWD 之内，否则同样 → `path_escape`
///   （ISSUE-077，05 §5.2.1 路径安全）；
/// - 目标不存在/不可解析（`canonicalize` 失败）时退回词法路径，由
///   [`read_bounded`] 按**既有契约**报 `file_missing`/`file_unreadable`。
fn resolve_asset_file(
    agent_dir: &Path,
    assets_root: &Path,
    raw: &str,
    json_path: &str,
) -> Result<PathBuf, HarnessError> {
    let raw_path = Path::new(raw);
    let candidate = if raw_path.is_absolute() {
        raw_path.to_path_buf()
    } else {
        agent_dir.join(raw_path)
    };
    let Some(normalized) = normalize_lexical(&candidate) else {
        return Err(path_escape_error(agent_dir, raw, json_path));
    };
    let assets_root = normalize_lexical(assets_root).unwrap_or_else(|| assets_root.to_path_buf());
    let cwd = normalize_lexical(&absolute(Path::new("."))).unwrap_or_default();
    if !normalized.starts_with(&assets_root) && !normalized.starts_with(&cwd) {
        return Err(path_escape_error(agent_dir, raw, json_path));
    }
    let Ok(canonical) = std::fs::canonicalize(&normalized) else {
        // 目标缺失/不可解析：保持既有错误码（read_bounded → file_missing/file_unreadable）。
        return Ok(normalized);
    };
    if !canonical.starts_with(&canonical_or_lexical(&assets_root))
        && !canonical.starts_with(&canonical_or_lexical(&cwd))
    {
        return Err(path_escape_error(agent_dir, raw, json_path));
    }
    Ok(canonical)
}

/// 真实路径（解析符号链接）；不可解析时退回调用方给定的（已归一化）路径。
fn canonical_or_lexical(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

fn path_escape_error(agent_dir: &Path, raw: &str, json_path: &str) -> HarnessError {
    config_error(
        "path_escape",
        Some(&agent_dir.display().to_string()),
        Some(json_path),
        Some(Value::from("path inside assets dir or CWD")),
        Some(Value::from(raw.to_string())),
        format!("asset path '{raw}' escapes assets dir/CWD"),
    )
}

/// 词法归一化（不访问文件系统）；`..` 越界 → `None`。
fn normalize_lexical(path: &Path) -> Option<PathBuf> {
    let mut parts: Vec<std::ffi::OsString> = Vec::new();
    let mut absolute_path = false;
    for component in path.components() {
        match component {
            Component::Prefix(prefix) => parts.push(prefix.as_os_str().to_owned()),
            Component::RootDir => {
                absolute_path = true;
                parts.clear();
            }
            Component::CurDir => {}
            Component::ParentDir => {
                parts.pop()?;
            }
            Component::Normal(name) => parts.push(name.to_owned()),
        }
    }
    let mut normalized = PathBuf::new();
    if absolute_path {
        normalized.push(Component::RootDir.as_os_str());
    }
    for part in parts {
        normalized.push(part);
    }
    Some(normalized)
}

/// 绝对化（不要求路径存在）。
fn absolute(path: &Path) -> PathBuf {
    std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf())
}

/// 读取 JSON 文件（字节上限 + UTF-8 + 解析）。
fn read_json_file(path: &Path, max_bytes: u64) -> Result<Value, HarnessError> {
    let bytes = read_bounded(path, max_bytes)?;
    let text = decode_utf8(bytes, path)?;
    serde_json::from_str(&text).map_err(|error| {
        config_error(
            "json_parse",
            Some(&path.display().to_string()),
            None,
            Some(Value::from("valid JSON")),
            None,
            format!("parse {}: {error}", path.display()),
        )
    })
}

/// 读取文件（存在性/可读性/大小上限）。
fn read_bounded(path: &Path, max_bytes: u64) -> Result<Vec<u8>, HarnessError> {
    let metadata = std::fs::metadata(path).map_err(|error| {
        let reason = if error.kind() == std::io::ErrorKind::NotFound {
            "file_missing"
        } else {
            "file_unreadable"
        };
        config_error(
            reason,
            Some(&path.display().to_string()),
            None,
            None,
            None,
            format!("read {}: {error}", path.display()),
        )
    })?;
    if metadata.len() > max_bytes {
        return Err(config_error(
            "file_too_large",
            Some(&path.display().to_string()),
            None,
            Some(Value::from(max_bytes)),
            Some(Value::from(metadata.len())),
            format!(
                "file {} is {} bytes (limit {max_bytes})",
                path.display(),
                metadata.len()
            ),
        ));
    }
    std::fs::read(path).map_err(|error| {
        config_error(
            "file_unreadable",
            Some(&path.display().to_string()),
            None,
            None,
            None,
            format!("read {}: {error}", path.display()),
        )
    })
}

/// UTF-8 解码（非法字节 → `invalid_utf8`）。
fn decode_utf8(bytes: Vec<u8>, path: &Path) -> Result<String, HarnessError> {
    String::from_utf8(bytes).map_err(|error| {
        config_error(
            "invalid_utf8",
            Some(&path.display().to_string()),
            None,
            Some(Value::from("UTF-8")),
            None,
            format!("{} is not valid UTF-8: {error}", path.display()),
        )
    })
}

/// 步 8：`tools` 查注册表（未注册 → `tool_not_registered`）。
fn resolve_tools(
    config: &AgentConfig,
    registry: &ToolRegistry,
) -> Result<Vec<ToolDef>, HarnessError> {
    let mut resolved = Vec::with_capacity(config.tools.len());
    for (index, reference) in config.tools.iter().enumerate() {
        let tool = registry.resolve(reference).ok_or_else(|| {
            config_error(
                "tool_not_registered",
                None,
                Some(&format!("$.tools[{index}]")),
                Some(Value::from("registered tool name")),
                Some(Value::from(reference.clone())),
                format!(
                    "agent '{}' references unregistered tool '{reference}'",
                    config.id
                ),
            )
        })?;
        resolved.push(tool.clone());
    }
    Ok(resolved)
}

/// 步 9：`model.api_key_env`（fake 模式跳过）。
fn resolve_api_key(config: &AgentConfig, fake_mode: bool) -> Result<SecretString, HarnessError> {
    if fake_mode {
        return Ok(SecretString::new("fake-mode"));
    }
    let env_name = &config.model.api_key_env;
    let value = std::env::var(env_name)
        .ok()
        .filter(|value| !value.is_empty());
    match value {
        Some(value) => Ok(SecretString::new(value)),
        None => Err(config_error(
            "api_key_missing",
            None,
            Some("$.model.api_key_env"),
            Some(Value::from("env var set and non-empty")),
            Some(Value::from(env_name.clone())),
            format!("env var {env_name} is missing or empty"),
        )),
    }
}

// ---------------------------------------------------------------------------
// 小工具与字段提取
// ---------------------------------------------------------------------------

fn is_json_file(path: &Path) -> bool {
    path.extension().and_then(|extension| extension.to_str()) == Some("json")
}

fn json_type_name(value: &Value) -> &'static str {
    crate::tools::json_type_name(value)
}

/// 未知字段拒绝 + v1 保留字段（05 §5.2.2/§5.10.2）。
fn reject_unknown(
    object: &Map<String, Value>,
    path: &Path,
    prefix: &str,
    allowed: &[&str],
    reserved: &[&str],
) -> Result<(), HarnessError> {
    for key in object.keys() {
        let json_path = if prefix == "$" {
            format!("$.{key}")
        } else {
            format!("{prefix}.{key}")
        };
        if reserved.contains(&key.as_str()) {
            return Err(config_error(
                "reserved_field",
                Some(&path.display().to_string()),
                Some(&json_path),
                Some(Value::from("field absent (reserved for v1.1/v2)")),
                Some(Value::from(format!("reserved field '{key}'"))),
                format!("reserved field '{key}' is not supported by v1"),
            ));
        }
        if !allowed.contains(&key.as_str()) {
            return Err(config_error(
                "unknown_field",
                Some(&path.display().to_string()),
                Some(&json_path),
                Some(json!(allowed
                    .iter()
                    .map(|key| Value::from(*key))
                    .collect::<Vec<_>>())),
                Some(Value::from(key.clone())),
                format!("unknown field '{key}'"),
            ));
        }
    }
    Ok(())
}

fn required_string(
    object: &Map<String, Value>,
    key: &str,
    path: &Path,
    json_path: &str,
) -> Result<String, HarnessError> {
    match object.get(key) {
        Some(Value::String(text)) => Ok(text.clone()),
        Some(other) => Err(type_error(
            path,
            json_path,
            "string",
            json_type_name(other),
            format!("field '{key}' must be a string"),
        )),
        None => Err(missing_field(path, json_path, "string", key)),
    }
}

fn optional_string(
    object: &Map<String, Value>,
    key: &str,
    path: &Path,
    json_path: &str,
) -> Result<Option<String>, HarnessError> {
    match object.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(text)) => Ok(Some(text.clone())),
        Some(other) => Err(type_error(
            path,
            json_path,
            "string or null",
            json_type_name(other),
            format!("field '{key}' must be a string or null"),
        )),
    }
}

fn optional_u64(
    object: &Map<String, Value>,
    key: &str,
    path: &Path,
    json_path: &str,
) -> Result<Option<u64>, HarnessError> {
    match object.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Number(number)) => number.as_u64().map(Some).ok_or_else(|| {
            type_error(
                path,
                json_path,
                "non-negative integer",
                "number",
                format!("field '{key}' must be a non-negative integer"),
            )
        }),
        Some(other) => Err(type_error(
            path,
            json_path,
            "integer",
            json_type_name(other),
            format!("field '{key}' must be an integer"),
        )),
    }
}

fn optional_f64(
    object: &Map<String, Value>,
    key: &str,
    path: &Path,
    json_path: &str,
) -> Result<Option<f64>, HarnessError> {
    match object.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Number(number)) => number.as_f64().map(Some).ok_or_else(|| {
            type_error(
                path,
                json_path,
                "number",
                "number",
                format!("field '{key}' must be a number"),
            )
        }),
        Some(other) => Err(type_error(
            path,
            json_path,
            "number",
            json_type_name(other),
            format!("field '{key}' must be a number"),
        )),
    }
}

fn missing_field(path: &Path, json_path: &str, expected: &str, key: &str) -> HarnessError {
    config_error(
        "missing_field",
        Some(&path.display().to_string()),
        Some(json_path),
        Some(Value::from(expected)),
        None,
        format!("missing required field '{key}'"),
    )
}

fn type_error(
    path: &Path,
    json_path: &str,
    expected: &str,
    actual: &str,
    message: impl Into<String>,
) -> HarnessError {
    config_error(
        "type_mismatch",
        Some(&path.display().to_string()),
        Some(json_path),
        Some(Value::from(expected)),
        Some(Value::from(actual)),
        message,
    )
}

fn range_error(path: &Path, json_path: &str, actual: &u64, min: u64, max: u64) -> HarnessError {
    config_error(
        "out_of_range",
        Some(&path.display().to_string()),
        Some(json_path),
        Some(json!([min, max])),
        Some(Value::from(*actual)),
        format!("value {actual} out of range {min}..={max}"),
    )
}

fn config_error(
    reason: &str,
    path: Option<&str>,
    json_path: Option<&str>,
    expected: Option<Value>,
    actual: Option<Value>,
    message: impl Into<String>,
) -> HarnessError {
    HarnessError::config_load(reason, path, json_path, expected, actual, message)
}

/// `id` 规则：`^[a-z][a-z0-9_-]{0,63}$`。
fn is_agent_id(id: &str) -> bool {
    let mut chars = id.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    first.is_ascii_lowercase()
        && id.chars().count() <= 64
        && chars.all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '_' || ch == '-')
}

/// `api_key_env` 规则：`^[A-Z_][A-Z0-9_]{0,63}$`。
fn is_env_name(name: &str) -> bool {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    (first.is_ascii_uppercase() || first == '_')
        && name.chars().count() <= 64
        && chars.all(|ch| ch.is_ascii_uppercase() || ch.is_ascii_digit() || ch == '_')
}

/// `model.name` 规则：`^[A-Za-z0-9._:-]{1,128}$`。
fn is_model_name(name: &str) -> bool {
    let chars = name.chars().count();
    (1..=128).contains(&chars)
        && name
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | ':' | '-'))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::constants::DEFAULT_TOOL_TIMEOUT_MS;
    use tempfile::TempDir;

    fn tool(name: &str) -> ToolDef {
        ToolDef {
            name: name.to_string(),
            description: "d".to_string(),
            parameters: json!({"type": "object", "properties": {}}),
            timeout_ms: DEFAULT_TOOL_TIMEOUT_MS,
            source: "tools.builtin".to_string(),
        }
    }

    fn registry(names: &[&str]) -> ToolRegistry {
        ToolRegistry::new(names.iter().map(|name| tool(name)).collect()).expect("registry")
    }

    /// 在临时目录写入一个 Agent 资产 + prompt，返回 assets_dir。
    ///
    /// `constraints=None` 时同时删除 `constraints_file` 字段（避免悬空文件引用）。
    fn asset_dir(agent: &Value, prompt: &str, constraints: Option<&str>) -> TempDir {
        let dir = TempDir::new().expect("tempdir");
        let assets = dir.path();
        std::fs::create_dir_all(assets.join("agents")).expect("agents dir");
        std::fs::create_dir_all(assets.join("prompts")).expect("prompts dir");
        std::fs::write(assets.join("prompts").join("role.md"), prompt).expect("role");
        let mut agent = agent.clone();
        if let Some(constraints) = constraints {
            std::fs::write(assets.join("prompts").join("constraints.md"), constraints)
                .expect("constraints");
        } else if let Some(object) = agent.as_object_mut() {
            object.remove("constraints_file");
        }
        let id = agent["id"].as_str().expect("id");
        std::fs::write(
            assets.join("agents").join(format!("{id}.json")),
            serde_json::to_string_pretty(&agent).expect("json"),
        )
        .expect("agent file");
        dir
    }

    fn valid_agent() -> Value {
        json!({
            "id": "parser",
            "name": "解析器",
            "schema_version": 1,
            "model": {
                "provider": "deepseek",
                "name": "deepseek-chat",
                "base_url": null,
                "api_key_env": "HARNESS_TEST_API_KEY",
                "temperature": 0.2,
                "max_tokens": 2048,
                "context_window_tokens": 65536
            },
            "role_prompt_file": "../prompts/role.md",
            "constraints_file": "../prompts/constraints.md",
            "tools": ["echo", "add"],
            "input_mapping": {"message": "user_message"},
            "max_iterations": 5,
            "llm_timeout_ms": 30000,
            "llm_max_retries": 2,
            "node_timeout_ms": 120000
        })
    }

    #[test]
    fn loads_valid_agent_with_defaults_and_prompts() {
        let dir = asset_dir(&valid_agent(), "你是解析器。", Some("只输出 JSON。"));
        let agent = load_agent(dir.path(), "parser", &registry(&["echo", "add"]), true)
            .expect("load agent");
        assert_eq!(agent.config.id, "parser");
        assert_eq!(agent.role_prompt, "你是解析器。");
        assert_eq!(agent.constraints.as_deref(), Some("只输出 JSON。"));
        assert_eq!(agent.tools.len(), 2);
        assert_eq!(agent.config.model.temperature, 0.2);
        assert_eq!(agent.config.max_iterations, 5);
    }

    #[test]
    fn applies_spec_defaults_when_optional_fields_missing() {
        let mut agent = valid_agent();
        let object = agent.as_object_mut().expect("object");
        for key in [
            "schema_version",
            "tools",
            "input_mapping",
            "max_iterations",
            "llm_timeout_ms",
            "llm_max_retries",
            "node_timeout_ms",
        ] {
            object.remove(key);
        }
        object
            .get_mut("model")
            .and_then(Value::as_object_mut)
            .expect("model")
            .remove("temperature");
        let dir = asset_dir(&agent, "role", None);
        let loaded =
            load_agent(dir.path(), "parser", &ToolRegistry::default(), true).expect("load agent");
        assert_eq!(loaded.config.schema_version, 1);
        assert_eq!(loaded.config.max_iterations, DEFAULT_MAX_ITERATIONS);
        assert_eq!(loaded.config.llm_timeout_ms, DEFAULT_LLM_TIMEOUT_MS);
        assert_eq!(loaded.config.llm_max_retries, DEFAULT_LLM_MAX_RETRIES);
        assert_eq!(loaded.config.node_timeout_ms, DEFAULT_NODE_TIMEOUT_MS);
        assert_eq!(loaded.config.model.temperature, DEFAULT_LLM_TEMPERATURE);
        assert!(loaded.tools.is_empty());
        assert!(loaded.constraints.is_none());
    }

    #[test]
    fn missing_agent_is_agent_not_indexed() {
        let dir = asset_dir(&valid_agent(), "role", None);
        let error =
            load_agent(dir.path(), "ghost", &registry(&["echo"]), true).expect_err("missing");
        assert_eq!(error.detail["reason"], "agent_not_indexed");
    }

    #[test]
    fn missing_agents_dir_is_empty_index() {
        let dir = TempDir::new().expect("tempdir");
        let index = agent_index(dir.path()).expect("index");
        assert!(index.is_empty());
    }

    #[test]
    fn unknown_field_is_rejected_with_json_path() {
        let mut agent = valid_agent();
        agent
            .as_object_mut()
            .expect("object")
            .insert("surprise".to_string(), json!(1));
        let dir = asset_dir(&agent, "role", None);
        let error =
            load_agent(dir.path(), "parser", &registry(&["echo"]), true).expect_err("unknown");
        assert_eq!(error.detail["reason"], "unknown_field");
        assert_eq!(error.detail["json_path"], "$.surprise");
    }

    #[test]
    fn reserved_field_is_rejected() {
        let mut agent = valid_agent();
        agent
            .as_object_mut()
            .expect("object")
            .insert("output_mapping".to_string(), json!({}));
        let dir = asset_dir(&agent, "role", None);
        let error =
            load_agent(dir.path(), "parser", &registry(&["echo"]), true).expect_err("reserved");
        assert_eq!(error.detail["reason"], "reserved_field");
    }

    #[test]
    fn missing_required_field_is_reported() {
        let mut agent = valid_agent();
        agent.as_object_mut().expect("object").remove("model");
        let dir = asset_dir(&agent, "role", None);
        let error = load_agent(dir.path(), "parser", &registry(&[]), true).expect_err("missing");
        assert_eq!(error.detail["reason"], "missing_field");
        assert_eq!(error.detail["json_path"], "$.model");
    }

    #[test]
    fn oversized_agent_config_is_rejected() {
        let dir = TempDir::new().expect("tempdir");
        std::fs::create_dir_all(dir.path().join("agents")).expect("agents");
        let path = dir.path().join("agents").join("big.json");
        std::fs::write(&path, vec![b'x'; (MAX_AGENT_CONFIG_BYTES + 1) as usize]).expect("write");
        let error = agent_index(dir.path()).expect_err("too large");
        assert_eq!(error.detail["reason"], "file_too_large");
    }

    #[test]
    fn invalid_utf8_is_rejected() {
        let dir = TempDir::new().expect("tempdir");
        std::fs::create_dir_all(dir.path().join("agents")).expect("agents");
        std::fs::write(
            dir.path().join("agents").join("bad.json"),
            [0xff, 0xfe, 0x00],
        )
        .expect("write");
        let error = agent_index(dir.path()).expect_err("utf8");
        assert_eq!(error.detail["reason"], "invalid_utf8");
    }

    #[test]
    fn prompt_path_escape_is_rejected() {
        let mut agent = valid_agent();
        agent
            .as_object_mut()
            .expect("object")
            .insert("role_prompt_file".to_string(), json!("../../../etc/passwd"));
        let dir = asset_dir(&agent, "role", None);
        let error =
            load_agent(dir.path(), "parser", &registry(&["echo"]), true).expect_err("escape");
        assert_eq!(error.detail["reason"], "path_escape");
    }

    /// 用指定 `role_prompt_file` 写一份合法 Agent 资产，返回 assets_dir。
    fn agent_with_role_file(role_prompt_file: &str) -> TempDir {
        let mut agent = valid_agent();
        agent
            .as_object_mut()
            .expect("object")
            .insert("role_prompt_file".to_string(), json!(role_prompt_file));
        asset_dir(&agent, "role", None)
    }

    /// ISSUE-077：符号链接指向资产根**外**的真实文件 → `path_escape`。
    #[cfg(unix)]
    #[test]
    fn symlinked_prompt_escaping_assets_root_is_rejected() {
        use std::os::unix::fs::symlink;

        let outside = TempDir::new().expect("outside");
        let secret = outside.path().join("secret.md");
        std::fs::write(&secret, "TOP-SECRET").expect("secret");
        let dir = agent_with_role_file("../prompts/leak.md");
        symlink(&secret, dir.path().join("prompts").join("leak.md")).expect("symlink");

        let error = load_agent(dir.path(), "parser", &registry(&["echo", "add"]), true)
            .expect_err("symlink escape");
        assert_eq!(error.detail["reason"], "path_escape");
    }

    /// ISSUE-077：**中间目录**为符号链接时同样拦截（词法路径仍在根内）。
    #[cfg(unix)]
    #[test]
    fn symlinked_directory_escaping_assets_root_is_rejected() {
        use std::os::unix::fs::symlink;

        let outside = TempDir::new().expect("outside");
        std::fs::write(outside.path().join("leak.md"), "TOP-SECRET").expect("secret");
        let dir = agent_with_role_file("../linked/leak.md");
        symlink(outside.path(), dir.path().join("linked")).expect("symlink dir");

        let error = load_agent(dir.path(), "parser", &registry(&["echo", "add"]), true)
            .expect_err("symlink dir escape");
        assert_eq!(error.detail["reason"], "path_escape");
    }

    /// ISSUE-077 正例：链接目标仍在资产根内时不得误杀。
    #[cfg(unix)]
    #[test]
    fn symlink_inside_assets_root_is_allowed() {
        use std::os::unix::fs::symlink;

        let dir = agent_with_role_file("../prompts/alias.md");
        let real = dir.path().join("prompts").join("real.md");
        std::fs::write(&real, "inside prompt").expect("real");
        symlink(&real, dir.path().join("prompts").join("alias.md")).expect("symlink");

        let loaded = load_agent(dir.path(), "parser", &registry(&["echo", "add"]), true)
            .expect("symlink inside assets root must load");
        assert_eq!(loaded.role_prompt, "inside prompt");
    }

    /// ISSUE-077：`canonicalize` 失败（目标不存在）时保持既有 `file_missing` 契约。
    #[test]
    fn missing_prompt_file_keeps_file_missing_reason() {
        let dir = agent_with_role_file("../prompts/absent.md");
        let error = load_agent(dir.path(), "parser", &registry(&["echo", "add"]), true)
            .expect_err("missing prompt");
        assert_eq!(error.detail["reason"], "file_missing");
    }

    #[test]
    fn oversized_prompt_is_rejected() {
        let mut agent = valid_agent();
        agent
            .as_object_mut()
            .expect("object")
            .insert("role_prompt_file".to_string(), json!("../prompts/huge.md"));
        let dir = asset_dir(&agent, "role", None);
        std::fs::write(
            dir.path().join("prompts").join("huge.md"),
            vec![b'a'; (MAX_PROMPT_BYTES + 1) as usize],
        )
        .expect("write");
        let error =
            load_agent(dir.path(), "parser", &registry(&["echo"]), true).expect_err("too large");
        assert_eq!(error.detail["reason"], "file_too_large");
    }

    #[test]
    fn unregistered_tool_is_config_load() {
        let dir = asset_dir(&valid_agent(), "role", None);
        let error = load_agent(dir.path(), "parser", &registry(&["echo"]), true).expect_err("tool");
        assert_eq!(error.detail["reason"], "tool_not_registered");
        assert_eq!(error.detail["json_path"], "$.tools[1]");
    }

    #[test]
    fn api_key_is_required_unless_fake_mode() {
        let dir = asset_dir(&valid_agent(), "role", None);
        // 未设置环境变量 + 非 fake → api_key_missing。
        let error = load_agent(dir.path(), "parser", &registry(&["echo", "add"]), false)
            .expect_err("api key");
        assert_eq!(error.detail["reason"], "api_key_missing");
        // fake 模式跳过。
        let loaded = load_agent(dir.path(), "parser", &registry(&["echo", "add"]), true)
            .expect("fake mode skips api key");
        assert!(
            !loaded.api_key.is_empty(),
            "fake mode injects placeholder key"
        );
        assert_eq!(loaded.api_key.expose(), "fake-mode");
    }

    #[test]
    fn validates_provider_base_url_and_ranges() {
        let cases: Vec<(Value, &str)> = vec![
            (json!("openai"), "type_mismatch"),
            (
                json!({"provider": "openai_compatible", "name": "m", "api_key_env": "K",
                    "base_url": null}),
                "missing_field",
            ),
            (
                json!({"provider": "openai_compatible", "name": "m", "api_key_env": "K",
                    "base_url": "https://host/v1/"}),
                "unsupported_value",
            ),
            (
                json!({"provider": "deepseek", "name": "m", "api_key_env": "K",
                    "base_url": "https://host/v1"}),
                "unsupported_value",
            ),
            (
                json!({"provider": "deepseek", "name": "m", "api_key_env": "k"}),
                "unsupported_value",
            ),
        ];
        for (model, reason) in cases {
            let mut agent = valid_agent();
            agent
                .as_object_mut()
                .expect("object")
                .insert("model".to_string(), model);
            let dir = asset_dir(&agent, "role", None);
            let error = load_agent(dir.path(), "parser", &registry(&["echo"]), true)
                .expect_err("invalid model");
            assert_eq!(error.detail["reason"], reason, "model case");
        }
    }

    #[test]
    fn validates_numeric_ranges() {
        let cases: Vec<(Value, &str)> = vec![
            (json!({"temperature": 3.0}), "out_of_range"),
            (json!({"max_tokens": 32769}), "out_of_range"),
            (json!({"context_window_tokens": 1024}), "out_of_range"),
            (
                json!({"max_tokens": 20000, "context_window_tokens": 4096}),
                "out_of_range",
            ),
        ];
        for (overrides, reason) in cases {
            let mut agent = valid_agent();
            let model = agent["model"].as_object().cloned().expect("model");
            let mut model = model;
            for (key, value) in overrides.as_object().expect("object") {
                model.insert(key.clone(), value.clone());
            }
            agent
                .as_object_mut()
                .expect("object")
                .insert("model".to_string(), Value::Object(model));
            let dir = asset_dir(&agent, "role", None);
            let error = load_agent(dir.path(), "parser", &registry(&["echo"]), true)
                .expect_err("invalid model");
            assert_eq!(error.detail["reason"], reason);
        }
    }

    #[test]
    fn validates_schema_version_and_iterations() {
        let mut agent = valid_agent();
        agent
            .as_object_mut()
            .expect("object")
            .insert("schema_version".to_string(), json!(2));
        let dir = asset_dir(&agent, "role", None);
        let error =
            load_agent(dir.path(), "parser", &registry(&["echo"]), true).expect_err("schema");
        assert_eq!(error.detail["reason"], "unsupported_schema_version");

        let mut agent = valid_agent();
        agent
            .as_object_mut()
            .expect("object")
            .insert("max_iterations".to_string(), json!(51));
        let dir = asset_dir(&agent, "role", None);
        let error =
            load_agent(dir.path(), "parser", &registry(&["echo"]), true).expect_err("iters");
        assert_eq!(error.detail["reason"], "out_of_range");
    }

    #[test]
    fn rejects_too_many_tools_and_bad_tool_reference() {
        let mut agent = valid_agent();
        let tools: Vec<Value> = (0..33)
            .map(|index| Value::from(format!("t{index}")))
            .collect();
        agent
            .as_object_mut()
            .expect("object")
            .insert("tools".to_string(), Value::Array(tools));
        let dir = asset_dir(&agent, "role", None);
        let error = load_agent(dir.path(), "parser", &registry(&[]), true).expect_err("too many");
        assert_eq!(error.detail["reason"], "out_of_range");

        let mut agent = valid_agent();
        agent
            .as_object_mut()
            .expect("object")
            .insert("tools".to_string(), json!(["BadName"]));
        let dir = asset_dir(&agent, "role", None);
        let error = load_agent(dir.path(), "parser", &registry(&[]), true).expect_err("bad name");
        assert_eq!(error.detail["reason"], "unsupported_value");
    }

    #[test]
    fn input_mapping_requires_non_empty_values() {
        let mut agent = valid_agent();
        agent
            .as_object_mut()
            .expect("object")
            .insert("input_mapping".to_string(), json!({"message": ""}));
        let dir = asset_dir(&agent, "role", None);
        let error = load_agent(dir.path(), "parser", &registry(&["echo", "add"]), true)
            .expect_err("empty mapping");
        assert_eq!(error.detail["reason"], "unsupported_value");
    }

    #[test]
    fn accepts_fully_qualified_tool_reference() {
        let mut agent = valid_agent();
        agent
            .as_object_mut()
            .expect("object")
            .insert("tools".to_string(), json!(["tools.builtin.echo"]));
        let dir = asset_dir(&agent, "role", None);
        let loaded = load_agent(dir.path(), "parser", &registry(&["echo"]), true)
            .expect("qualified reference");
        assert_eq!(loaded.tools[0].name, "echo");
    }

    #[test]
    fn agent_index_is_sorted_by_file_name() {
        let dir = TempDir::new().expect("tempdir");
        std::fs::create_dir_all(dir.path().join("agents")).expect("agents");
        for (file, id) in [("b.json", "beta"), ("a.json", "alpha")] {
            std::fs::write(
                dir.path().join("agents").join(file),
                json!({"id": id}).to_string(),
            )
            .expect("write");
        }
        let index = agent_index(dir.path()).expect("index");
        let ids: Vec<&String> = index.keys().collect();
        assert_eq!(ids, vec!["alpha", "beta"]);
    }

    // ---- ISSUE-076：派发前轻量读取 `node_timeout_ms`（`timeout_hint` 数据源） ----

    #[test]
    fn peek_node_timeout_returns_validated_agent_config_value() {
        let dir = asset_dir(&valid_agent(), "role", None);
        assert_eq!(peek_agent_node_timeout(dir.path(), "parser"), Some(120_000));
        // 未命中 id → None。
        assert_eq!(peek_agent_node_timeout(dir.path(), "other"), None);
    }

    #[test]
    fn peek_node_timeout_is_none_for_missing_or_invalid_assets() {
        // 资产目录不存在 / agents 子目录不存在 → None。
        let missing = TempDir::new().expect("tempdir");
        assert_eq!(peek_agent_node_timeout(missing.path(), "parser"), None);
        assert_eq!(
            peek_agent_node_timeout(&missing.path().join("nope"), "parser"),
            None
        );

        // 字段缺失（AgentConfig 默认 180s 由内核兜底）→ None。
        let mut agent = valid_agent();
        agent
            .as_object_mut()
            .expect("object")
            .remove("node_timeout_ms");
        let dir = asset_dir(&agent, "role", None);
        assert_eq!(peek_agent_node_timeout(dir.path(), "parser"), None);

        // 越界 / 非整数 → None（运行期 Loading 仍会 HARNESS_CONFIG_LOAD）。
        for invalid in [json!(10), json!(4_000_000), json!("120000")] {
            let mut agent = valid_agent();
            agent
                .as_object_mut()
                .expect("object")
                .insert("node_timeout_ms".to_string(), invalid.clone());
            let dir = asset_dir(&agent, "role", None);
            assert_eq!(
                peek_agent_node_timeout(dir.path(), "parser"),
                None,
                "invalid node_timeout_ms {invalid}"
            );
        }

        // JSON 非法/无 id 的文件被跳过，不影响其它资产命中。
        let dir = asset_dir(&valid_agent(), "role", None);
        std::fs::write(dir.path().join("agents").join("broken.json"), "{not json")
            .expect("broken file");
        std::fs::write(
            dir.path().join("agents").join("anonymous.json"),
            json!({"name": "no-id"}).to_string(),
        )
        .expect("anonymous file");
        assert_eq!(peek_agent_node_timeout(dir.path(), "parser"), Some(120_000));
    }

    #[test]
    fn peek_node_timeout_uses_first_file_for_duplicate_ids() {
        // 与 `agent_index` 一致：文件名排序后第一个命中者生效。
        let dir = TempDir::new().expect("tempdir");
        std::fs::create_dir_all(dir.path().join("agents")).expect("agents");
        for (file, timeout) in [("a.json", 11_000), ("b.json", 22_000)] {
            std::fs::write(
                dir.path().join("agents").join(file),
                json!({"id": "dup", "node_timeout_ms": timeout}).to_string(),
            )
            .expect("write");
        }
        assert_eq!(peek_agent_node_timeout(dir.path(), "dup"), Some(11_000));
    }
}
