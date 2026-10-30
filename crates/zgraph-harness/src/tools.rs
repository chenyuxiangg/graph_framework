//! 工具注册表、参数校验与执行（05 §5.6；D3：工具经 worker 子进程执行）。
//!
//! 语义要点：
//! - 工具是 **Python 侧声明 + Rust 侧注册表**；LLM 只能看到 `AgentConfig.tools` 列出的工具。
//! - 工具执行失败一律 `TOOL_ERROR` 喂回 LLM，绝不终止；调用未注册工具才是
//!   `HARNESS_UNKNOWN_TOOL`（终止）。
//! - 参数校验（object / required / 已声明 key 类型）在 Rust 侧完成，失败**不调用
//!   Python**，把错误 JSON 作为 `Tool` 消息喂回（05 §5.6.4）。
//! - 结果必须是 object；紧凑序列化 ≤ 8 KiB，超出走**结构感知截断**并附
//!   `…[truncated N bytes]`，截断后仍是**合法 JSON**（05 §5.6.5 / ISSUE-074）。
//!
//! D3 落地：`WorkerToolRunner` 每次 `call` 都 spawn 一个全新 worker 子进程，module 固定
//! [`TOOL_RUNNER_MODULE`]、function 为 `discover`/`call`；工具模块无跨调用缓存。工具发现
//! 结果由 executor 按 `tools_dir` 缓存一次。并发上限用全局
//! `tokio::sync::Semaphore`（CPU×2，与 `WorkerExecutor::default_max_workers` 一致）；
//! v2 不共享 `WorkerExecutor` 的每 run 信号量（工具调用不携带 run 级执行上下文）。

use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use serde_json::{json, Map, Value};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use zgraph_worker::{
    default_max_workers, run_worker_controlled, RunRequest, WorkerConfig, WorkerFailure,
};

use crate::budget::estimate_tokens;
use crate::constants::{
    DEFAULT_TOOL_TIMEOUT_MS, MAX_SUMMARY_PREVIEW_ARRAY_ITEMS, MAX_SUMMARY_PREVIEW_CHARS,
    MAX_SUMMARY_PREVIEW_DEPTH, MAX_TOOL_DESCRIPTION_CHARS, REDACTED_PREVIEW_VALUE,
    SENSITIVE_KEY_TOKENS, TOOL_RESULT_MAX_BYTES, TOOL_RUNNER_MODULE,
};
use crate::ctx::HarnessCtx;
use crate::error::HarnessError;

/// 工具定义（Rust 侧内存模型，05 §5.6.1 冻结）。
#[derive(Debug, Clone, PartialEq)]
pub struct ToolDef {
    /// 工具名（`^[a-z][a-z0-9_]{0,63}$`；全局唯一）。
    pub name: String,
    /// 描述（1..=1024 字符）。
    pub description: String,
    /// JSON Schema（05 §5.6.1 子集）。
    pub parameters: Value,
    /// 单次调用超时（毫秒）。
    pub timeout_ms: u64,
    /// Python 模块全名（如 `tools.builtin`；仅诊断/分发用）。
    pub source: String,
}

impl ToolDef {
    /// 发给 LLM 的最小 schema（05 §5.6.1 `ToolSchema`）。
    pub fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: self.name.clone(),
            description: self.description.clone(),
            parameters: self.parameters.clone(),
        }
    }
}

/// 发给 LLM 的工具 schema（与 `ToolDef` 分离）。
#[derive(Debug, Clone, PartialEq)]
pub struct ToolSchema {
    /// 工具名。
    pub name: String,
    /// 工具描述。
    pub description: String,
    /// JSON Schema。
    pub parameters: Value,
}

impl ToolSchema {
    /// 预算估算（请求 `tools` 字段的近似 token 数）。
    pub fn estimated_tokens(&self) -> u32 {
        let parameters = serde_json::to_string(&self.parameters).unwrap_or_default();
        estimate_tokens(&self.name)
            .saturating_add(estimate_tokens(&self.description))
            .saturating_add(estimate_tokens(&parameters))
    }
}

/// 工具注册表：`name` 全局唯一，按全限定名/短名解析。
#[derive(Debug, Clone, Default)]
pub struct ToolRegistry {
    tools: Vec<ToolDef>,
    by_name: HashMap<String, usize>,
    by_qualified: HashMap<String, usize>,
}

impl ToolRegistry {
    /// 由发现结果构造；校验 schema 子集与重名（05 §5.6.1/§5.6.2）。
    pub fn new(tools: Vec<ToolDef>) -> Result<Self, HarnessError> {
        let mut registry = Self {
            tools: Vec::with_capacity(tools.len()),
            by_name: HashMap::new(),
            by_qualified: HashMap::new(),
        };
        for tool in tools {
            validate_tool_def(&tool)?;
            if let Some(previous) = registry.by_name.get(&tool.name) {
                let previous = &registry.tools[*previous];
                return Err(HarnessError::config_load(
                    "duplicate_tool",
                    None,
                    Some("$.tools"),
                    Some(Value::from("globally unique tool name")),
                    Some(json!({"name": tool.name, "sources": [previous.source, tool.source]})),
                    format!(
                        "duplicate tool name '{}' declared by '{}' and '{}'",
                        tool.name, previous.source, tool.source
                    ),
                ));
            }
            let index = registry.tools.len();
            registry.by_name.insert(tool.name.clone(), index);
            registry
                .by_qualified
                .insert(format!("{}.{}", tool.source, tool.name), index);
            registry.tools.push(tool);
        }
        Ok(registry)
    }

    /// 按 `AgentConfig.tools` 的引用名解析（含 `.` 时先查全限定名，05 §5.6.2）。
    pub fn resolve(&self, reference: &str) -> Option<&ToolDef> {
        if reference.contains('.') {
            if let Some(index) = self.by_qualified.get(reference) {
                return self.tools.get(*index);
            }
        }
        self.by_name
            .get(reference)
            .and_then(|index| self.tools.get(*index))
            .or_else(|| {
                // 全限定名但 source 前缀不匹配时，允许按短名兜底（name 全局唯一，无歧义）。
                reference
                    .rsplit('.')
                    .next()
                    .and_then(|name| self.by_name.get(name))
                    .and_then(|index| self.tools.get(*index))
            })
    }

    /// 按 LLM 给的短名查（v1 要求 name 全局唯一）。
    pub fn get(&self, name: &str) -> Option<&ToolDef> {
        self.by_name
            .get(name)
            .and_then(|index| self.tools.get(*index))
    }

    /// 全部已注册工具名（`HARNESS_UNKNOWN_TOOL.detail.available_tools`）。
    pub fn names(&self) -> Vec<String> {
        self.tools.iter().map(|tool| tool.name.clone()).collect()
    }

    /// 按 `AgentConfig.tools` 顺序生成 schema；引用未注册 → `HARNESS_CONFIG_LOAD`。
    pub fn schemas(&self, references: &[String]) -> Result<Vec<ToolSchema>, HarnessError> {
        let mut schemas = Vec::with_capacity(references.len());
        for (index, reference) in references.iter().enumerate() {
            let tool = self.resolve(reference).ok_or_else(|| {
                HarnessError::config_load(
                    "tool_not_registered",
                    None,
                    Some(&format!("$.tools[{index}]")),
                    Some(Value::from("registered tool name")),
                    Some(Value::from(reference.clone())),
                    format!("agent references unregistered tool '{reference}'"),
                )
            })?;
            schemas.push(tool.schema());
        }
        Ok(schemas)
    }

    /// 已注册工具数。
    pub fn len(&self) -> usize {
        self.tools.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.tools.is_empty()
    }
}

/// 工具调用请求（Harness → ToolRunner；字段为 D3 落地的必要上下文）。
#[derive(Debug, Clone, PartialEq)]
pub struct ToolCallRequest {
    /// 调用 ID（与 `ToolCallStart/End`、`Tool.tool_call_id` 一致）。
    pub call_id: String,
    /// 工具名。
    pub tool_name: String,
    /// 工具声明的 Python 模块全名。
    pub module: String,
    /// 工具目录（worker 侧 `discover`/`call` 的 `tools_dir`）。
    pub tools_dir: std::path::PathBuf,
    /// 已校验的参数对象。
    pub args: Value,
    /// 生效超时（`min(ToolDef.timeout_ms, 30_000, 剩余 deadline)`）。
    pub timeout_ms: u64,
}

/// 工具执行失败（TOOL_ERROR 载荷；05 §5.6.5）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolFailure {
    /// 失败原因（`exception` / `timeout` / `args_not_object` / ...）。
    pub reason: String,
    /// 人类可读信息。
    pub message: String,
    /// Python 异常类型（`reason="exception"` 时）。
    pub error_type: Option<String>,
    /// Python traceback（≤8 KiB）。
    pub traceback: Option<String>,
    /// 超时值（`reason="timeout"` 时）。
    pub timeout_ms: Option<u64>,
}

impl ToolFailure {
    /// 构造一个只含 reason/message 的失败。
    pub fn new(reason: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            reason: reason.into(),
            message: message.into(),
            error_type: None,
            traceback: None,
            timeout_ms: None,
        }
    }

    /// 05 §5.6.5 的错误 JSON 载荷。
    pub fn to_json(&self) -> Value {
        let mut object = Map::new();
        object.insert("reason".to_string(), Value::from(self.reason.clone()));
        object.insert("message".to_string(), Value::from(self.message.clone()));
        if let Some(error_type) = &self.error_type {
            object.insert("type".to_string(), Value::from(error_type.clone()));
        }
        if let Some(traceback) = &self.traceback {
            // 05 §5.6.5 / ISSUE-074：traceback 上限统一 8 KiB（此前误用 1 MiB）。
            object.insert(
                "traceback".to_string(),
                Value::from(truncate_text_bytes(traceback, TOOL_RESULT_MAX_BYTES)),
            );
        }
        if let Some(timeout_ms) = self.timeout_ms {
            object.insert("timeout_ms".to_string(), Value::from(timeout_ms));
        }
        Value::Object(object)
    }
}

/// 工具执行后端（生产：worker 子进程；测试：`FakeToolRunner`）。
#[async_trait]
pub trait ToolRunner: Send + Sync {
    /// 发现工具注册表（05 §5.6.2）。失败 → `HARNESS_CONFIG_LOAD`。
    async fn discover(
        &self,
        tools_dir: &Path,
        ctx: Arc<dyn HarnessCtx>,
    ) -> Result<Vec<ToolDef>, HarnessError>;

    /// 执行一次工具调用。失败 → [`ToolFailure`]（喂回 LLM，不终止）。
    // `ToolFailure` 约 100 字节；契约要求 `Result<Value, ToolFailure>`（05 §5.6.5），
    // 不装箱以免破坏公开 API。
    #[allow(clippy::result_large_err)]
    async fn call(
        &self,
        request: ToolCallRequest,
        ctx: Arc<dyn HarnessCtx>,
    ) -> Result<Value, ToolFailure>;
}

// ---------------------------------------------------------------------------
// 参数校验、结果序列化与摘要（05 §5.6.4/§5.6.5、§5.8.5）
// ---------------------------------------------------------------------------

/// 调用前参数校验（05 §5.6.4）：失败**不调用 Python**，直接作为 TOOL_ERROR 喂回。
pub fn validate_tool_arguments(tool: &ToolDef, args: &Value) -> Result<(), ToolFailure> {
    let Some(object) = args.as_object() else {
        return Err(ToolFailure {
            reason: "args_not_object".to_string(),
            message: format!("tool '{}' args must be a JSON object", tool.name),
            error_type: None,
            traceback: None,
            timeout_ms: None,
        });
    };
    let parameters = tool.parameters.as_object();
    if let Some(parameters) = parameters {
        if let Some(required) = parameters.get("required").and_then(Value::as_array) {
            let missing: Vec<Value> = required
                .iter()
                .filter_map(Value::as_str)
                .filter(|key| !object.contains_key(*key))
                .map(|key| Value::from(key.to_string()))
                .collect();
            if !missing.is_empty() {
                return Err(ToolFailure {
                    reason: "missing_required".to_string(),
                    message: format!("tool '{}' missing required args: {missing:?}", tool.name),
                    error_type: None,
                    traceback: None,
                    timeout_ms: None,
                });
            }
        }
        if let Some(properties) = parameters.get("properties").and_then(Value::as_object) {
            for (key, value) in object {
                let Some(schema) = properties.get(key) else {
                    // v1 宽松：未声明 key 允许（05 §5.6.4）。
                    continue;
                };
                let Some(expected) = schema.get("type").and_then(Value::as_str) else {
                    continue;
                };
                if !matches_json_type(value, expected) {
                    return Err(ToolFailure {
                        reason: "type_mismatch".to_string(),
                        message: format!(
                            "tool '{}' arg '{key}' expected {expected}, got {}",
                            tool.name,
                            json_type_name(value)
                        ),
                        error_type: None,
                        traceback: None,
                        timeout_ms: None,
                    });
                }
            }
        }
    }
    Ok(())
}

/// JSON 值是否符合 JSON Schema 顶层类型（05 §5.6.4 的 v1 深度）。
fn matches_json_type(value: &Value, expected: &str) -> bool {
    match expected {
        "string" => value.is_string(),
        "number" => value.is_number(),
        "integer" => value.as_i64().is_some() || value.as_u64().is_some(),
        "boolean" => value.is_boolean(),
        "object" => value.is_object(),
        "array" => value.is_array(),
        "null" => value.is_null(),
        // 未知类型不阻断（v1 宽松；深层约束交给 Python 工具）。
        _ => true,
    }
}

/// JSON 值类型名（错误诊断用）。
pub fn json_type_name(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(number) if number.is_i64() || number.is_u64() => "integer",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

/// 序列化工具结果：必须是 object；紧凑 JSON ≤ 8 KiB（05 §5.6.5 / ISSUE-074）。
///
/// 超限时走**结构感知截断**（[`serialize_json_limited`]），保证喂回 LLM 的内容
/// 始终是**可 `serde_json::from_str` 的合法 JSON**。
pub fn serialize_tool_result(value: &Value) -> Result<String, ToolFailure> {
    if !value.is_object() {
        return Err(ToolFailure {
            reason: "result_not_object".to_string(),
            message: format!("tool must return a dict, got {}", json_type_name(value)),
            error_type: None,
            traceback: None,
            timeout_ms: None,
        });
    }
    Ok(serialize_json_limited(value))
}

/// 截断标记预留字节（05 §5.6.5：`…[truncated N bytes]` 预留 64 B 标记空间）。
pub(crate) const TRUNCATION_MARKER_RESERVE: usize = 64;

/// 结构降级时追加到对象上的保留键（业务结果不应使用同名键）。
const TRUNCATED_OBJECT_KEY: &str = "_truncated";

/// 单次截断的降级步数上限（防御性；正常数十步内收敛）。
const MAX_SHRINK_STEPS: usize = 4_096;

/// 把 JSON 值序列化为**合法 JSON**，且紧凑文本 ≤ [`TOOL_RESULT_MAX_BYTES`]。
///
/// 结构感知截断策略（05 §5.6.5 + ISSUE-074 合法 JSON 契约）：
/// 1. 未超限：原样紧凑序列化；
/// 2. 超限：为截断标记预留 [`TRUNCATION_MARKER_RESERVE`] 字节后，反复「单步降级」：
///    - **字符串值**：在 JSON 字符串内部按**字符边界**截断，末尾附
///      `…[truncated N bytes]`（N = 该字符串丢掉的 UTF-8 字节数）；
///    - 无法靠字符串吸收时**结构降级**：对象从尾部丢属性、数组从尾部丢元素
///      （保留前缀），最后追加 `"_truncated": "…[truncated N bytes]"` 标记
///      （N = 原始紧凑 JSON 字节数 − 当前文本字节数）；
/// 3. 极端情况（如键名本身超长）退化为固定「截断信封」对象（仍合法 JSON）。
///
/// 不变量：返回值必定可 `serde_json::from_str`，且字节数 ≤ [`TOOL_RESULT_MAX_BYTES`]。
fn serialize_json_limited(value: &Value) -> String {
    let compact = serde_json::to_string(value).unwrap_or_else(|_| "{}".to_string());
    if compact.len() <= TOOL_RESULT_MAX_BYTES {
        return compact;
    }
    let budget = TOOL_RESULT_MAX_BYTES.saturating_sub(TRUNCATION_MARKER_RESERVE);
    let mut current = value.clone();
    let mut structural = false;
    for _ in 0..MAX_SHRINK_STEPS {
        if serialized_len(&current) <= budget {
            break;
        }
        if truncate_largest_string(&mut current, budget) {
            continue;
        }
        if !drop_tail_item(&mut current) {
            return fallback_envelope(&compact);
        }
        structural = true;
    }
    if serialized_len(&current) > budget {
        return fallback_envelope(&compact);
    }
    if structural {
        append_structural_marker(&mut current, &compact);
    }
    let output = serde_json::to_string(&current).unwrap_or_else(|_| fallback_envelope(&compact));
    if output.len() <= TOOL_RESULT_MAX_BYTES {
        output
    } else {
        fallback_envelope(&compact)
    }
}

/// 构造喂回 LLM 的工具错误内容（`{"error": {...}}`，同样受 8 KiB 限制）。
pub(crate) fn tool_error_content(failure: &ToolFailure) -> String {
    let payload = json!({ "error": failure.to_json() });
    serialize_json_limited(&payload)
}

/// 紧凑序列化长度（`Value` 恒可序列化；失败按最大值处理以触发降级）。
fn serialized_len(value: &Value) -> usize {
    serde_json::to_string(value).map_or(usize::MAX, |text| text.len())
}

/// 字符串值在「结构降级标记」中的累计丢弃字节数（内部辅助）。
///
/// 返回 `(payload, already_dropped)`：若字符串已带 `…[truncated N bytes]` 后缀，
/// 剥掉后缀并返回此前已丢弃的字节数，避免多次截断叠加多个标记。
fn strip_truncation_marker(text: &str) -> (&str, usize) {
    let Some(start) = text.rfind("…[truncated ") else {
        return (text, 0);
    };
    let tail = &text[start..];
    let Some(digits) = tail
        .strip_prefix("…[truncated ")
        .and_then(|rest| rest.strip_suffix(" bytes]"))
    else {
        return (text, 0);
    };
    let already = digits.parse::<usize>().unwrap_or(0);
    (&text[..start], already)
}

/// 按「保留前 `keep_chars` 个字符 + 标记」构造字符串内容。
fn marked_string(payload: &str, keep_chars: usize, already_dropped: usize) -> String {
    let kept: String = payload.chars().take(keep_chars).collect();
    let removed = payload
        .len()
        .saturating_sub(kept.len())
        .saturating_add(already_dropped);
    format!("{kept}…[truncated {removed} bytes]")
}

/// 找到序列化后最大的字符串叶子（路径 = 逐层 Object 值序 / Array 下标）。
fn largest_string_path(value: &Value) -> Option<Vec<usize>> {
    fn walk(value: &Value, path: &mut Vec<usize>, best: &mut Option<(usize, Vec<usize>)>) {
        match value {
            Value::String(text) => {
                let len = serde_json::to_string(text).map_or(0, |text| text.len());
                let better = best.as_ref().is_none_or(|(best_len, _)| len > *best_len);
                if better {
                    *best = Some((len, path.clone()));
                }
            }
            Value::Array(items) => {
                for (index, item) in items.iter().enumerate() {
                    path.push(index);
                    walk(item, path, best);
                    path.pop();
                }
            }
            Value::Object(map) => {
                for (index, (_, item)) in map.iter().enumerate() {
                    path.push(index);
                    walk(item, path, best);
                    path.pop();
                }
            }
            _ => {}
        }
    }
    let mut best: Option<(usize, Vec<usize>)> = None;
    walk(value, &mut Vec::new(), &mut best);
    best.map(|(_, path)| path)
}

/// 按路径取可变值（路径由 [`largest_string_path`] 生成，顺序一致）。
fn value_at_path_mut<'a>(value: &'a mut Value, path: &[usize]) -> Option<&'a mut Value> {
    let mut current = value;
    for &index in path {
        current = match current {
            Value::Array(items) => items.get_mut(index)?,
            Value::Object(map) => map.values_mut().nth(index)?,
            _ => return None,
        };
    }
    Some(current)
}

/// 单步降级：把最大字符串叶子截断到「整体 ≤ budget」并附标记。
///
/// 返回 `false` 表示无法靠字符串吸收超限部分（没有字符串，或最大的字符串也
/// 填不满差值），调用方转入结构降级。字符串内截断后仍是合法 JSON 字符串。
fn truncate_largest_string(value: &mut Value, budget: usize) -> bool {
    let total = serialized_len(value);
    if total <= budget {
        return false;
    }
    let Some(path) = largest_string_path(value) else {
        return false;
    };
    let excess = total - budget;
    let Some(Value::String(text)) = value_at_path_mut(value, &path) else {
        return false;
    };
    let (payload, already_dropped) = strip_truncation_marker(text);
    let current_len = serde_json::to_string(text).map_or(0, |text| text.len());
    // 该字符串需让出 excess 字节且仍预留标记空间；否则由结构降级处理。
    if current_len <= excess + TRUNCATION_MARKER_RESERVE {
        return false;
    }
    let target = current_len - excess;
    let mut low = 0usize;
    let mut high = payload.chars().count();
    while low < high {
        let mid = (low + high).div_ceil(2);
        let candidate = marked_string(payload, mid, already_dropped);
        let fits = serde_json::to_string(&candidate).is_ok_and(|text| text.len() <= target);
        if fits {
            low = mid;
        } else {
            high = mid - 1;
        }
    }
    let candidate = marked_string(payload, low, already_dropped);
    if candidate == *text {
        return false;
    }
    *text = candidate;
    true
}

/// 单步结构降级：优先下沉到「最右容器」丢其尾部元素/属性，否则丢当前容器尾部项。
///
/// 保留前缀语义：`{"items":[...]}` 不会整体丢 `items`，而是先缩短数组前缀。
/// 一次至少丢掉尾部 `len / 8` 项（≥1），保证超长数组在有限步内收敛。
fn drop_tail_item(value: &mut Value) -> bool {
    match value {
        Value::Array(items) => {
            if let Some(last) = items.last_mut() {
                if is_container(last) && drop_tail_item(last) {
                    return true;
                }
            }
            drop_tail_chunk(items)
        }
        Value::Object(map) => {
            let Some(key) = map.keys().next_back().cloned() else {
                return false;
            };
            if let Some(last) = map.get_mut(&key) {
                if is_container(last) && drop_tail_item(last) {
                    return true;
                }
            }
            let drop = map.len().div_ceil(8).max(1);
            if map.len() <= drop {
                map.clear();
            } else {
                let keys: Vec<String> = map.keys().rev().take(drop).cloned().collect();
                for key in keys {
                    map.remove(&key);
                }
            }
            true
        }
        _ => false,
    }
}

/// 数组尾部按 `len / 8`（≥1）成块丢弃，返回是否仍有内容（`false` = 已空）。
fn drop_tail_chunk(items: &mut Vec<Value>) -> bool {
    if items.is_empty() {
        return false;
    }
    let drop = items.len().div_ceil(8).max(1);
    let keep = items.len().saturating_sub(drop);
    items.truncate(keep);
    true
}

/// 数组/对象是「可继续结构降级」的容器。
fn is_container(value: &Value) -> bool {
    matches!(value, Value::Array(_) | Value::Object(_))
}

/// 为结构降级追加唯一标记（对象插 `_truncated` 键；数组推标记字符串）。
fn append_structural_marker(value: &mut Value, original_compact: &str) {
    let current_len = serialized_len(value);
    let dropped = original_compact.len().saturating_sub(current_len);
    let marker = Value::String(format!("…[truncated {dropped} bytes]"));
    match value {
        Value::Object(map) => {
            map.insert(TRUNCATED_OBJECT_KEY.to_string(), marker);
        }
        Value::Array(items) => items.push(marker),
        _ => {}
    }
}

/// 极端兜底：无法结构降级时返回固定「截断信封」对象（合法 JSON）。
fn fallback_envelope(original_compact: &str) -> String {
    let mut map = Map::new();
    map.insert(
        TRUNCATED_OBJECT_KEY.to_string(),
        Value::String(format!("…[truncated {} bytes]", original_compact.len())),
    );
    serde_json::to_string(&Value::Object(map))
        .unwrap_or_else(|_| "{\"_truncated\":\"…[truncated]\"}".to_string())
}

/// 以字符边界截断文本并附 `…`，返回**严格 ≤ `max_chars`** 个字符（05 §5.8.5）。
pub(crate) fn truncate_chars(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    if max_chars == 0 {
        return String::new();
    }
    let mut out: String = text.chars().take(max_chars - 1).collect();
    out.push('…');
    out
}

/// 按 UTF-8 字符边界把文本截到 ≤ `max_bytes` 字节（不切码点、不追加标记）。
fn truncate_text_bytes(text: &str, max_bytes: usize) -> String {
    if text.len() <= max_bytes {
        return text.to_string();
    }
    let boundary = floor_char_boundary(text, max_bytes);
    text[..boundary].to_string()
}

/// 向下取字符边界（不切断 UTF-8 码点）。
fn floor_char_boundary(text: &str, max_bytes: usize) -> usize {
    if max_bytes >= text.len() {
        return text.len();
    }
    let mut boundary = max_bytes.min(text.len());
    while boundary > 0 && !text.is_char_boundary(boundary) {
        boundary -= 1;
    }
    boundary
}

/// `ToolCallStart.args_summary`（05 §5.8.5）：**不得**放完整参数。
///
/// ISSUE-075：`preview` 由「参数紧凑 JSON 前缀」收紧为**结构摘要**——标量只保留
/// 类型/尺寸，敏感键（[`SENSITIVE_KEY_TOKENS`]）值一律 `***`，最后按字符边界
/// 截断到 [`MAX_SUMMARY_PREVIEW_CHARS`]。因此短参数不会完整出现在 trace 中。
pub fn summarize_args(args: &Value) -> Value {
    let compact = serde_json::to_string(args).unwrap_or_default();
    let keys: Vec<Value> = args
        .as_object()
        .map(|object| object.keys().cloned().map(Value::from).collect())
        .unwrap_or_default();
    json!({
        "keys": keys,
        "bytes": compact.len(),
        "preview": structural_preview(args),
    })
}

/// `ToolCallEnd.result_summary`（05 §5.8.5）：成功 `{ok,keys,bytes,preview}`；
/// 失败 `{ok:false,error:{...≤1KiB}}`。
///
/// ISSUE-075：成功路径的 `preview` 与 [`summarize_args`] 同口径（结构摘要 +
/// 截断 + 敏感键脱敏）；非 JSON 结果视为字符串标量，只记录字符数。
pub fn summarize_result(content: &str, ok: bool) -> Value {
    if ok {
        let parsed = serde_json::from_str::<Value>(content).ok();
        let keys: Vec<Value> = parsed
            .as_ref()
            .and_then(Value::as_object)
            .map(|object| object.keys().cloned().map(Value::from).collect())
            .unwrap_or_default();
        let preview = match &parsed {
            Some(value) => structural_preview(value),
            None => structural_preview(&Value::String(content.to_string())),
        };
        json!({
            "ok": true,
            "keys": keys,
            "bytes": content.len(),
            "preview": preview,
        })
    } else {
        let parsed: Value = serde_json::from_str(content).unwrap_or(Value::Null);
        let error = parsed.get("error").cloned().unwrap_or(parsed);
        json!({
            "ok": false,
            "error": truncate_json_value(&error, 1024),
        })
    }
}

/// 敏感键判定：键名归一化（小写 + 去非字母数字）后包含敏感词即命中。
fn is_sensitive_key(key: &str) -> bool {
    let normalized: String = key
        .chars()
        .filter(|ch| ch.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect();
    SENSITIVE_KEY_TOKENS
        .iter()
        .any(|token| normalized.contains(token))
}

/// preview 结构摘要的紧凑 JSON 文本（≤ [`MAX_SUMMARY_PREVIEW_CHARS`] 字符）。
fn structural_preview(value: &Value) -> String {
    let compact = serde_json::to_string(&preview_value(value, 0)).unwrap_or_default();
    truncate_chars(&compact, MAX_SUMMARY_PREVIEW_CHARS)
}

/// 递归渲染结构摘要：对象保留键、值降级为类型/尺寸；敏感键值 → `***`。
fn preview_value(value: &Value, depth: usize) -> Value {
    match value {
        Value::Object(map) => {
            if depth >= MAX_SUMMARY_PREVIEW_DEPTH {
                return Value::from(format!("object({} keys)", map.len()));
            }
            let mut rendered = Map::new();
            for (key, child) in map {
                let child = if is_sensitive_key(key) {
                    Value::from(REDACTED_PREVIEW_VALUE)
                } else {
                    preview_value(child, depth + 1)
                };
                rendered.insert(key.clone(), child);
            }
            Value::Object(rendered)
        }
        Value::Array(items) => {
            if depth >= MAX_SUMMARY_PREVIEW_DEPTH {
                return Value::from(format!("array({} items)", items.len()));
            }
            let shown = items.len().min(MAX_SUMMARY_PREVIEW_ARRAY_ITEMS);
            let mut rendered: Vec<Value> = items
                .iter()
                .take(shown)
                .map(|item| preview_value(item, depth + 1))
                .collect();
            if items.len() > shown {
                rendered.push(Value::from(format!("…(+{} more)", items.len() - shown)));
            }
            Value::Array(rendered)
        }
        Value::String(text) => Value::from(format!("string({} chars)", text.chars().count())),
        Value::Number(_) => Value::from("number"),
        Value::Bool(_) => Value::from("boolean"),
        Value::Null => Value::from("null"),
    }
}

/// 把 JSON 值的紧凑序列化截断到 ≤ max_bytes（字符边界；失败时退化为字符串标记）。
fn truncate_json_value(value: &Value, max_bytes: usize) -> Value {
    let compact = serde_json::to_string(value).unwrap_or_default();
    if compact.len() <= max_bytes {
        return value.clone();
    }
    Value::from(format!(
        "{}{}",
        &compact[..floor_char_boundary(&compact, max_bytes.saturating_sub(16))],
        "…[truncated]"
    ))
}

/// 校验工具定义（05 §5.6.1 的字段约束与 schema 子集）。
fn validate_tool_def(tool: &ToolDef) -> Result<(), HarnessError> {
    if !is_tool_name(&tool.name) {
        return Err(HarnessError::config_load(
            "invalid_declaration",
            None,
            Some("$.name"),
            Some(Value::from("^[a-z][a-z0-9_]{0,63}$")),
            Some(Value::from(tool.name.clone())),
            format!("invalid tool name '{}'", tool.name),
        ));
    }
    let description_chars = tool.description.chars().count();
    if description_chars == 0 || description_chars > MAX_TOOL_DESCRIPTION_CHARS {
        return Err(HarnessError::config_load(
            "invalid_declaration",
            None,
            Some("$.description"),
            Some(Value::from("1..=1024 characters")),
            Some(Value::from(description_chars)),
            format!(
                "tool '{}' description length {description_chars}",
                tool.name
            ),
        ));
    }
    if !(1..=600_000).contains(&tool.timeout_ms) {
        return Err(HarnessError::config_load(
            "invalid_tool_timeout",
            None,
            Some("$.tools[].timeout_ms"),
            Some(Value::from("integer in 1..=600000")),
            Some(Value::from(tool.timeout_ms)),
            format!("tool '{}' timeout_ms out of range", tool.name),
        ));
    }
    validate_parameter_schema(&tool.name, &tool.parameters)
}

/// 工具名规则（`^[a-z][a-z0-9_]{0,63}$`）。
pub fn is_tool_name(name: &str) -> bool {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !first.is_ascii_lowercase() {
        return false;
    }
    let rest = chars.as_str();
    rest.len() <= 63 && rest.chars().all(is_tool_name_rest)
}

/// 工具名后续字符（小写字母/数字/下划线/短横线？05 §5.6.1 只允许 `[a-z0-9_]`）。
fn is_tool_name_rest(ch: char) -> bool {
    ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '_'
}

/// `parameters` 允许的 schema 关键字（05 §5.6.1 子集）。
const ALLOWED_SCHEMA_KEYWORDS: [&str; 16] = [
    "type",
    "properties",
    "required",
    "items",
    "enum",
    "description",
    "default",
    "minimum",
    "maximum",
    "minLength",
    "maxLength",
    "additionalProperties",
    "title",
    "format",
    "examples",
    "const",
];

/// 明确不允许的 schema 关键字（05 §5.6.1：出现即 `unsupported_schema_keyword`）。
const FORBIDDEN_SCHEMA_KEYWORDS: [&str; 8] = [
    "$ref",
    "oneOf",
    "anyOf",
    "allOf",
    "patternProperties",
    "if",
    "then",
    "else",
];

/// schema 递归校验的最大深度（防御性上限）。
const MAX_SCHEMA_DEPTH: usize = 32;

/// `parameters` 子集约束（05 §5.6.1 表；递归校验 properties/items）。
fn validate_parameter_schema(tool_name: &str, parameters: &Value) -> Result<(), HarnessError> {
    let Some(object) = parameters.as_object() else {
        return Err(config_schema_error(
            tool_name,
            "$.parameters",
            "parameters must be a JSON object",
        ));
    };
    if object.get("type").and_then(Value::as_str) != Some("object") {
        return Err(config_schema_error(
            tool_name,
            "$.parameters.type",
            "parameters.type must be \"object\"",
        ));
    }
    match object.get("properties") {
        Some(Value::Object(_)) => {}
        Some(_) => {
            return Err(config_schema_error(
                tool_name,
                "$.parameters.properties",
                "parameters.properties must be an object",
            ))
        }
        None => {
            return Err(config_schema_error(
                tool_name,
                "$.parameters.properties",
                "parameters.properties is required",
            ))
        }
    }
    validate_schema_node(tool_name, parameters, "$.parameters", 0)
}

/// 递归校验一个 schema 节点：关键字白名单 + properties/items/additionalProperties 结构。
fn validate_schema_node(
    tool_name: &str,
    value: &Value,
    json_path: &str,
    depth: usize,
) -> Result<(), HarnessError> {
    if depth > MAX_SCHEMA_DEPTH {
        return Err(config_schema_error(
            tool_name,
            json_path,
            "schema nesting too deep",
        ));
    }
    let Some(object) = value.as_object() else {
        return Err(config_schema_error(
            tool_name,
            json_path,
            "schema node must be an object",
        ));
    };
    for (keyword, keyword_value) in object {
        let keyword_path = format!("{json_path}.{keyword}");
        validate_schema_keyword(tool_name, keyword, keyword_value, &keyword_path, depth)?;
    }
    Ok(())
}

/// 单个 schema 关键字：白名单判定 + 按关键字校验值结构（子节点递归回 [`validate_schema_node`]）。
fn validate_schema_keyword(
    tool_name: &str,
    keyword: &str,
    value: &Value,
    keyword_path: &str,
    depth: usize,
) -> Result<(), HarnessError> {
    if FORBIDDEN_SCHEMA_KEYWORDS.contains(&keyword) || !ALLOWED_SCHEMA_KEYWORDS.contains(&keyword) {
        return Err(unsupported_schema_keyword(tool_name, keyword_path, keyword));
    }
    match keyword {
        "properties" => validate_properties_keyword(tool_name, value, keyword_path, depth),
        "items" => validate_items_keyword(tool_name, value, keyword_path, depth),
        "additionalProperties" => {
            validate_additional_properties_keyword(tool_name, value, keyword_path)
        }
        "type" => validate_type_keyword(tool_name, value, keyword_path),
        "required" | "enum" => validate_array_keyword(tool_name, value, keyword_path),
        _ => Ok(()),
    }
}

/// `properties`：对象，逐个属性递归校验（路径追加属性名）。
fn validate_properties_keyword(
    tool_name: &str,
    value: &Value,
    keyword_path: &str,
    depth: usize,
) -> Result<(), HarnessError> {
    let Some(properties) = value.as_object() else {
        return Err(config_schema_error(
            tool_name,
            keyword_path,
            "properties must be an object",
        ));
    };
    for (name, schema) in properties {
        validate_schema_node(
            tool_name,
            schema,
            &format!("{keyword_path}.{name}"),
            depth + 1,
        )?;
    }
    Ok(())
}

/// `items`：单个 schema 对象，递归校验。
fn validate_items_keyword(
    tool_name: &str,
    value: &Value,
    keyword_path: &str,
    depth: usize,
) -> Result<(), HarnessError> {
    if !value.is_object() {
        return Err(config_schema_error(
            tool_name,
            keyword_path,
            "items must be a schema object",
        ));
    }
    validate_schema_node(tool_name, value, keyword_path, depth + 1)
}

/// `additionalProperties`：仅允许 bool。
fn validate_additional_properties_keyword(
    tool_name: &str,
    value: &Value,
    keyword_path: &str,
) -> Result<(), HarnessError> {
    if !value.is_boolean() {
        return Err(unsupported_schema_keyword(
            tool_name,
            keyword_path,
            "additionalProperties (only bool allowed)",
        ));
    }
    Ok(())
}

/// `type`：字符串且属于冻结的 JSON 类型子集。
fn validate_type_keyword(
    tool_name: &str,
    value: &Value,
    keyword_path: &str,
) -> Result<(), HarnessError> {
    let Some(type_name) = value.as_str() else {
        return Err(config_schema_error(
            tool_name,
            keyword_path,
            "type must be a string",
        ));
    };
    if !matches!(
        type_name,
        "object" | "string" | "number" | "integer" | "boolean" | "array" | "null"
    ) {
        return Err(unsupported_schema_keyword(
            tool_name,
            keyword_path,
            type_name,
        ));
    }
    Ok(())
}

/// `required` / `enum`：必须是数组（元素语义由下游工具实现校验）。
fn validate_array_keyword(
    tool_name: &str,
    value: &Value,
    keyword_path: &str,
) -> Result<(), HarnessError> {
    if !value.is_array() {
        return Err(config_schema_error(
            tool_name,
            keyword_path,
            "required/enum must be arrays",
        ));
    }
    Ok(())
}

/// 构造 `unsupported_schema_keyword` 错误。
fn unsupported_schema_keyword(tool_name: &str, json_path: &str, keyword: &str) -> HarnessError {
    HarnessError::config_load(
        "unsupported_schema_keyword",
        None,
        Some(json_path),
        Some(Value::from("supported JSON Schema subset")),
        Some(Value::from(keyword.to_string())),
        format!("tool '{tool_name}' uses unsupported schema keyword '{keyword}'"),
    )
}

/// schema 校验错误（统一 reason）。
fn config_schema_error(tool_name: &str, json_path: &str, message: &str) -> HarnessError {
    HarnessError::config_load(
        "invalid_declaration",
        None,
        Some(json_path),
        Some(Value::from("valid tool parameters schema")),
        Some(Value::from(tool_name.to_string())),
        message,
    )
}

// ---------------------------------------------------------------------------
// WorkerToolRunner（D3：每次工具调用一个 worker 子进程）
// ---------------------------------------------------------------------------

/// 全进程工具并发上限（CPU×2，与架构 §4/D4 的全局 worker 上限一致）。
fn tool_semaphore() -> &'static Arc<Semaphore> {
    static SEMAPHORE: OnceLock<Arc<Semaphore>> = OnceLock::new();
    SEMAPHORE.get_or_init(|| Arc::new(Semaphore::new(default_max_workers())))
}

/// 生产 ToolRunner：每次 `discover`/`call` 都 spawn 一个新 worker（D3）。
///
/// - 无跨调用模块缓存（进程即缓存边界）；
/// - 超时对**进程组** kill → `ToolFailure { reason: "timeout" }`；
/// - 取消 → `ToolFailure { reason: "cancelled" }`（loop 再映射为 `NodeError::Cancelled`）；
/// - 并发受全局信号量约束（[`default_max_workers`] = CPU×2）。
///
/// 构造必须注入 [`WorkerConfig`]（e2e/CLI 需要设置 `PYTHONPATH`、cwd 等）；每次工具
/// 调用在基线上覆盖 `timeout` 为生效值（[`DEFAULT_TOOL_TIMEOUT_MS`] 上界由 loop 截断），
/// 其余字段（program/args/env/cwd/hello_timeout/exit_grace/max_stderr_bytes）原样透传。
#[derive(Debug, Clone)]
pub struct WorkerToolRunner {
    config: WorkerConfig,
}

impl Default for WorkerToolRunner {
    fn default() -> Self {
        Self::new(WorkerConfig::default())
    }
}

impl WorkerToolRunner {
    /// 以基线 worker 配置构造（每次调用克隆并覆盖超时）。
    pub fn new(config: WorkerConfig) -> Self {
        Self { config }
    }

    /// 基线 worker 配置（只读；测试断言注入被采用）。
    pub fn config(&self) -> &WorkerConfig {
        &self.config
    }

    /// 按本次调用生效超时派生 worker 配置。
    fn effective_config(&self, timeout_ms: u64) -> WorkerConfig {
        let mut config = self.config.clone();
        config.timeout = Duration::from_millis(timeout_ms.saturating_add(1_000));
        config
    }

    /// 获取全局并发许可。
    async fn permit() -> Result<OwnedSemaphorePermit, ToolFailure> {
        tool_semaphore()
            .clone()
            .acquire_owned()
            .await
            .map_err(|_| ToolFailure::new("runner_unavailable", "tool semaphore closed"))
    }

    /// 调用一次 worker 的 `zgraph_worker.tool_runner.<function>`。
    ///
    /// `WorkerFailure` 来自 `zgraph-worker`（外部类型，约 144 字节）；这里原样返回
    /// 便于按失败类别映射，不装箱。
    #[allow(clippy::result_large_err)]
    async fn run_worker(
        &self,
        function: &str,
        inputs: Value,
        ctx: &Arc<dyn HarnessCtx>,
        timeout_ms: u64,
    ) -> Result<Value, WorkerFailure> {
        let config = self.effective_config(timeout_ms);
        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|elapsed| u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX))
            .unwrap_or(0);
        let instance_id = ctx.instance_id().to_string();
        let request = RunRequest {
            run_id: instance_id.clone(),
            instance_id: instance_id.clone(),
            node_id: "agent_tool".to_string(),
            attempt: 0,
            module: TOOL_RUNNER_MODULE.to_string(),
            function: function.to_string(),
            inputs,
            params: json!({}),
            user_id: ctx.user_id().to_string(),
            // HarnessCtx 只有 `get_var`（无枚举）；工具契约不需要 global_vars。
            global_vars: json!({}),
            seed: ctx.seed(),
            deadline_ms: now_ms.saturating_add(timeout_ms),
            trace_id: instance_id,
        };
        let mut ignore = |_frame: &zgraph_worker::Frame| {};
        let cancel_ctx = ctx.clone();
        let cancel: zgraph_worker::CancelSignal =
            Box::pin(async move { crate::ctx::wait_cancelled(&cancel_ctx).await });
        run_worker_controlled(&config, &request, &mut ignore, cancel)
            .await
            .map(|run| run.output)
    }
}

#[async_trait]
impl ToolRunner for WorkerToolRunner {
    async fn discover(
        &self,
        tools_dir: &Path,
        ctx: Arc<dyn HarnessCtx>,
    ) -> Result<Vec<ToolDef>, HarnessError> {
        let _permit = WorkerToolRunner::permit().await.map_err(|failure| {
            HarnessError::config_load(
                &failure.reason,
                Some(&tools_dir.display().to_string()),
                Some("$.tools"),
                None,
                None,
                failure.message,
            )
        })?;
        let inputs = json!({ "tools_dir": tools_dir.display().to_string() });
        let timeout_ms = DEFAULT_TOOL_TIMEOUT_MS;
        match self.run_worker("discover", inputs, &ctx, timeout_ms).await {
            Ok(output) => parse_discover_output(&output, tools_dir),
            Err(failure) => Err(discover_worker_error(failure, tools_dir)),
        }
    }

    async fn call(
        &self,
        request: ToolCallRequest,
        ctx: Arc<dyn HarnessCtx>,
    ) -> Result<Value, ToolFailure> {
        let _permit = WorkerToolRunner::permit().await?;
        let inputs = json!({
            "tools_dir": request.tools_dir.display().to_string(),
            "module": request.module,
            "name": request.tool_name,
            "args": request.args,
        });
        match self
            .run_worker("call", inputs, &ctx, request.timeout_ms)
            .await
        {
            Ok(output) => parse_call_output(&output),
            Err(failure) => Err(call_worker_failure(failure, request.timeout_ms)),
        }
    }
}

/// 解析 `discover` 的 worker 输出（ISSUE-081 拆分：失败/声明解析见子函数）。
fn parse_discover_output(output: &Value, tools_dir: &Path) -> Result<Vec<ToolDef>, HarnessError> {
    let Some(object) = output.as_object() else {
        return Err(HarnessError::config_load(
            "tool_module_import",
            Some(&tools_dir.display().to_string()),
            None,
            Some(Value::from("object")),
            Some(Value::from(json_type_name(output))),
            "tool discover output is not an object".to_string(),
        ));
    };
    if object.get("ok").and_then(Value::as_bool) != Some(true) {
        return Err(decode_discover_error(object, tools_dir));
    }
    let Some(tools) = object.get("tools").and_then(Value::as_array) else {
        return Err(HarnessError::config_load(
            "invalid_declaration",
            Some(&tools_dir.display().to_string()),
            Some("$.tools"),
            Some(Value::from("array")),
            None,
            "tool discover output missing tools array".to_string(),
        ));
    };
    let mut defs = Vec::with_capacity(tools.len());
    for (index, tool) in tools.iter().enumerate() {
        defs.push(parse_tool_declaration(tool, index, tools_dir)?);
    }
    Ok(defs)
}

/// `discover` 的 `ok=false` 载荷 → `HARNESS_CONFIG_LOAD`（reason/message 透传）。
fn decode_discover_error(object: &Map<String, Value>, tools_dir: &Path) -> HarnessError {
    let error = object.get("error").cloned().unwrap_or(Value::Null);
    let reason = error
        .get("reason")
        .and_then(Value::as_str)
        .unwrap_or("tool_module_import")
        .to_string();
    let message = error
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or("tool discover failed")
        .to_string();
    let mut actual = error;
    if let Some(actual) = actual.as_object_mut() {
        actual.remove("traceback");
    }
    HarnessError::config_load(
        &reason,
        Some(&tools_dir.display().to_string()),
        None,
        None,
        Some(actual),
        message,
    )
}

/// 解析单条工具声明（`name`/`description`/`source`/`parameters`/`timeout_ms`）。
fn parse_tool_declaration(
    tool: &Value,
    index: usize,
    tools_dir: &Path,
) -> Result<ToolDef, HarnessError> {
    let Some(tool_object) = tool.as_object() else {
        return Err(HarnessError::config_load(
            "invalid_declaration",
            Some(&tools_dir.display().to_string()),
            Some(&format!("$.tools[{index}]")),
            Some(Value::from("object")),
            Some(Value::from(json_type_name(tool))),
            "tool declaration must be an object".to_string(),
        ));
    };
    let name = require_str(tool_object, "name", index, tools_dir)?;
    let description = require_str(tool_object, "description", index, tools_dir)?;
    let source = require_str(tool_object, "source", index, tools_dir)?;
    let parameters = tool_object
        .get("parameters")
        .cloned()
        .ok_or_else(|| missing_tool_field("parameters", index, tools_dir))?;
    let timeout_ms = parse_tool_timeout(tool_object, index, tools_dir)?;
    Ok(ToolDef {
        name,
        description,
        parameters,
        timeout_ms,
        source,
    })
}

/// 解析工具声明的可选 `timeout_ms`（1..=600000；缺省 30000）。
///
/// 05 §5.6.5：生效超时 = `min(ToolDef.timeout_ms, DEFAULT_TOOL_TIMEOUT_MS)`，
/// 因此声明 300 时实际 300ms、声明 60000 时裁剪到 30000ms。
fn parse_tool_timeout(
    object: &Map<String, Value>,
    index: usize,
    tools_dir: &Path,
) -> Result<u64, HarnessError> {
    match object.get("timeout_ms") {
        None | Some(Value::Null) => Ok(DEFAULT_TOOL_TIMEOUT_MS),
        Some(value) => {
            let timeout_ms = value.as_u64().ok_or_else(|| {
                HarnessError::config_load(
                    "invalid_tool_timeout",
                    Some(&tools_dir.display().to_string()),
                    Some(&format!("$.tools[{index}].timeout_ms")),
                    Some(Value::from("integer in 1..=600000")),
                    Some(value.clone()),
                    "tool timeout_ms must be a positive integer".to_string(),
                )
            })?;
            if !(1..=600_000).contains(&timeout_ms) {
                return Err(HarnessError::config_load(
                    "invalid_tool_timeout",
                    Some(&tools_dir.display().to_string()),
                    Some(&format!("$.tools[{index}].timeout_ms")),
                    Some(Value::from("integer in 1..=600000")),
                    Some(Value::from(timeout_ms)),
                    "tool timeout_ms out of range".to_string(),
                ));
            }
            Ok(timeout_ms)
        }
    }
}

fn require_str(
    object: &Map<String, Value>,
    field: &str,
    index: usize,
    tools_dir: &Path,
) -> Result<String, HarnessError> {
    object
        .get(field)
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| missing_tool_field(field, index, tools_dir))
}

fn missing_tool_field(field: &str, index: usize, tools_dir: &Path) -> HarnessError {
    HarnessError::config_load(
        "invalid_declaration",
        Some(&tools_dir.display().to_string()),
        Some(&format!("$.tools[{index}].{field}")),
        Some(Value::from("string")),
        None,
        format!("tool declaration missing field '{field}'"),
    )
}

/// 解析 `call` 的 worker 输出。
fn parse_call_output(output: &Value) -> Result<Value, ToolFailure> {
    let Some(object) = output.as_object() else {
        return Err(ToolFailure::new(
            "worker_protocol",
            "tool call output is not an object",
        ));
    };
    if object.get("ok").and_then(Value::as_bool) == Some(true) {
        return Ok(object.get("result").cloned().unwrap_or(Value::Null));
    }
    let error = object.get("error").cloned().unwrap_or(Value::Null);
    let reason = error
        .get("reason")
        .and_then(Value::as_str)
        .unwrap_or("exception")
        .to_string();
    let message = error
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or("tool call failed")
        .to_string();
    Err(ToolFailure {
        reason,
        message,
        error_type: error
            .get("type")
            .and_then(Value::as_str)
            .map(str::to_string),
        traceback: error
            .get("traceback")
            .and_then(Value::as_str)
            .map(str::to_string),
        timeout_ms: None,
    })
}

/// `discover` 的 worker 层失败 → `HARNESS_CONFIG_LOAD`。
fn discover_worker_error(failure: WorkerFailure, tools_dir: &Path) -> HarnessError {
    let (reason, message) = match &failure {
        WorkerFailure::Timeout(_) => ("tool_module_timeout", failure.to_string()),
        WorkerFailure::Cancelled => ("tool_discover_cancelled", failure.to_string()),
        WorkerFailure::Spawn(_) => ("tool_runner_spawn", failure.to_string()),
        WorkerFailure::Protocol(_) => ("tool_module_import", failure.to_string()),
        WorkerFailure::Worker(_) => ("tool_module_import", failure.to_string()),
        WorkerFailure::Exit { .. } => ("tool_worker_crash", failure.to_string()),
    };
    HarnessError::config_load(
        reason,
        Some(&tools_dir.display().to_string()),
        None,
        None,
        None,
        message,
    )
}

/// `call` 的 worker 层失败 → `ToolFailure`（喂回 LLM）。
fn call_worker_failure(failure: WorkerFailure, timeout_ms: u64) -> ToolFailure {
    match failure {
        WorkerFailure::Timeout(_) => ToolFailure {
            reason: "timeout".to_string(),
            message: format!("tool call timed out after {timeout_ms} ms"),
            error_type: None,
            traceback: None,
            timeout_ms: Some(timeout_ms),
        },
        WorkerFailure::Cancelled => ToolFailure::new("cancelled", "tool call cancelled"),
        WorkerFailure::Worker(error) => ToolFailure {
            reason: "exception".to_string(),
            message: error.message.clone(),
            error_type: Some(error.code.clone()),
            traceback: error.traceback.clone(),
            timeout_ms: None,
        },
        WorkerFailure::Spawn(error) => {
            ToolFailure::new("runner_spawn", format!("spawn tool worker failed: {error}"))
        }
        WorkerFailure::Protocol(error) => {
            ToolFailure::new("worker_protocol", format!("tool worker protocol: {error}"))
        }
        WorkerFailure::Exit {
            code,
            signal,
            stderr,
        } => ToolFailure::new(
            "worker_crash",
            format!("tool worker exited unexpectedly code={code:?} signal={signal:?}: {stderr}"),
        ),
    }
}

// ---------------------------------------------------------------------------
// FakeToolRunner（单测注入：name → 行为）
// ---------------------------------------------------------------------------

/// 假 ToolRunner 的单工具行为（05 §5.6 测试注入）。
#[derive(Debug, Clone, PartialEq)]
pub enum FakeToolBehavior {
    /// 返回固定结果。
    Return(Value),
    /// 返回 Python 风格异常（TOOL_ERROR）。
    Exception {
        /// 异常类型名。
        error_type: String,
        /// 异常信息。
        message: String,
    },
    /// 延迟后返回结果。
    Delay {
        /// 延迟时长。
        duration: Duration,
        /// 延迟结束后的结果。
        result: Value,
    },
    /// 模拟超时（loop 的超时兜底会先行截断）。
    Timeout,
    /// 模拟取消。
    Cancelled,
}

/// 单测用 ToolRunner：`discover` 返回配置的工具，`call` 按工具名返回注入行为。
pub struct FakeToolRunner {
    tools: Vec<ToolDef>,
    behaviors: HashMap<String, FakeToolBehavior>,
    discover_error: Option<HarnessError>,
    calls: Mutex<Vec<ToolCallRequest>>,
    discover_calls: AtomicUsize,
}

impl Default for FakeToolRunner {
    fn default() -> Self {
        Self::new()
    }
}

impl FakeToolRunner {
    /// 空工具集。
    pub fn new() -> Self {
        Self {
            tools: Vec::new(),
            behaviors: HashMap::new(),
            discover_error: None,
            calls: Mutex::new(Vec::new()),
            discover_calls: AtomicUsize::new(0),
        }
    }

    /// 带工具集构造。
    pub fn with_tools(tools: Vec<ToolDef>) -> Self {
        let mut runner = Self::new();
        runner.tools = tools;
        runner
    }

    /// 追加一个工具声明。
    pub fn tool(mut self, tool: ToolDef) -> Self {
        self.tools.push(tool);
        self
    }

    /// 注入工具行为。
    pub fn behavior(mut self, name: impl Into<String>, behavior: FakeToolBehavior) -> Self {
        self.behaviors.insert(name.into(), behavior);
        self
    }

    /// 注入 discover 失败。
    pub fn with_discover_error(mut self, error: HarnessError) -> Self {
        self.discover_error = Some(error);
        self
    }

    /// 已收到的调用（顺序）。
    pub fn calls(&self) -> Vec<ToolCallRequest> {
        match self.calls.lock() {
            Ok(guard) => guard.clone(),
            Err(poisoned) => poisoned.into_inner().clone(),
        }
    }

    /// discover 调用次数。
    pub fn discover_calls(&self) -> usize {
        self.discover_calls.load(Ordering::SeqCst)
    }
}

#[async_trait]
impl ToolRunner for FakeToolRunner {
    async fn discover(
        &self,
        _tools_dir: &Path,
        _ctx: Arc<dyn HarnessCtx>,
    ) -> Result<Vec<ToolDef>, HarnessError> {
        self.discover_calls.fetch_add(1, Ordering::SeqCst);
        if let Some(error) = &self.discover_error {
            return Err(error.clone());
        }
        Ok(self.tools.clone())
    }

    async fn call(
        &self,
        request: ToolCallRequest,
        _ctx: Arc<dyn HarnessCtx>,
    ) -> Result<Value, ToolFailure> {
        {
            let mut calls = match self.calls.lock() {
                Ok(guard) => guard,
                Err(poisoned) => poisoned.into_inner(),
            };
            calls.push(request.clone());
        }
        let behavior = self
            .behaviors
            .get(&request.tool_name)
            .cloned()
            .unwrap_or_else(|| FakeToolBehavior::Return(json!({ "echo": request.args })));
        match behavior {
            FakeToolBehavior::Return(value) => Ok(value),
            FakeToolBehavior::Exception {
                error_type,
                message,
            } => Err(ToolFailure {
                reason: "exception".to_string(),
                message,
                error_type: Some(error_type),
                traceback: Some("Traceback (fake)".to_string()),
                timeout_ms: None,
            }),
            FakeToolBehavior::Delay { duration, result } => {
                tokio::time::sleep(duration).await;
                Ok(result)
            }
            FakeToolBehavior::Timeout => {
                // 让外层 `tokio::time::timeout` 先行截断；无外层时也在有限时间返回。
                tokio::time::sleep(Duration::from_millis(request.timeout_ms.max(50))).await;
                Err(ToolFailure {
                    reason: "timeout".to_string(),
                    message: "fake timeout".to_string(),
                    error_type: None,
                    traceback: None,
                    timeout_ms: Some(request.timeout_ms),
                })
            }
            FakeToolBehavior::Cancelled => Err(ToolFailure::new("cancelled", "fake cancelled")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TestCtx;
    use std::sync::Arc;

    fn tool(name: &str, parameters: Value) -> ToolDef {
        ToolDef {
            name: name.to_string(),
            description: "test tool".to_string(),
            parameters,
            timeout_ms: DEFAULT_TOOL_TIMEOUT_MS,
            source: "tools.builtin".to_string(),
        }
    }

    fn ctx() -> Arc<dyn HarnessCtx> {
        TestCtx::new()
    }

    #[test]
    fn registry_resolves_short_and_qualified_names() {
        let registry = ToolRegistry::new(vec![
            tool("now", json!({"type": "object", "properties": {}})),
            tool("add", json!({"type": "object", "properties": {}})),
        ])
        .expect("registry");
        assert_eq!(registry.resolve("now").expect("short").name, "now");
        assert_eq!(
            registry
                .resolve("tools.builtin.add")
                .expect("qualified")
                .name,
            "add"
        );
        assert!(registry.get("ghost").is_none());
        assert_eq!(registry.names(), vec!["now".to_string(), "add".to_string()]);
    }

    #[test]
    fn registry_rejects_duplicate_names_with_sources() {
        let error = ToolRegistry::new(vec![
            tool("now", json!({"type": "object", "properties": {}})),
            tool("now", json!({"type": "object", "properties": {}})),
        ])
        .expect_err("duplicate");
        assert_eq!(error.kind, zgraph_kernel::HarnessErrorKind::ConfigLoad);
        assert_eq!(error.detail["reason"], "duplicate_tool");
    }

    #[test]
    fn registry_rejects_unsupported_schema_keyword() {
        let error = ToolRegistry::new(vec![tool(
            "now",
            json!({"type": "object", "properties": {}, "$ref": "#/x"}),
        )])
        .expect_err("unsupported");
        assert_eq!(error.detail["reason"], "unsupported_schema_keyword");
    }

    #[test]
    fn schemas_are_ordered_like_agent_tools() {
        let registry = ToolRegistry::new(vec![
            tool("a", json!({"type": "object", "properties": {}})),
            tool("b", json!({"type": "object", "properties": {}})),
        ])
        .expect("registry");
        let schemas = registry
            .schemas(&["b".to_string(), "a".to_string()])
            .expect("schemas");
        assert_eq!(schemas[0].name, "b");
        assert_eq!(schemas[1].name, "a");
        let error = registry
            .schemas(&["ghost".to_string()])
            .expect_err("unregistered");
        assert_eq!(error.detail["reason"], "tool_not_registered");
    }

    #[test]
    fn argument_validation_checks_object_required_and_types() {
        let def = tool(
            "add",
            json!({
                "type": "object",
                "properties": {"a": {"type": "number"}, "b": {"type": "number"}},
                "required": ["a", "b"]
            }),
        );
        assert_eq!(
            validate_tool_arguments(&def, &json!({"a": 1, "b": 2})),
            Ok(())
        );

        let not_object = validate_tool_arguments(&def, &json!([1, 2])).expect_err("not object");
        assert_eq!(not_object.reason, "args_not_object");

        let missing = validate_tool_arguments(&def, &json!({"a": 1})).expect_err("missing");
        assert_eq!(missing.reason, "missing_required");

        let mismatch =
            validate_tool_arguments(&def, &json!({"a": "x", "b": 2})).expect_err("mismatch");
        assert_eq!(mismatch.reason, "type_mismatch");
        assert!(mismatch.message.contains("'a'"));
    }

    #[test]
    fn integer_and_unknown_args_validation() {
        let def = tool(
            "f",
            json!({
                "type": "object",
                "properties": {"n": {"type": "integer"}},
                "required": []
            }),
        );
        assert!(validate_tool_arguments(&def, &json!({"n": 3})).is_ok());
        assert!(validate_tool_arguments(&def, &json!({"n": 3.5})).is_err());
        // 未声明 key 宽松允许。
        assert!(validate_tool_arguments(&def, &json!({"n": 3, "extra": true})).is_ok());
    }

    #[test]
    fn result_serialization_requires_object_and_truncates_on_char_boundary() {
        let value = json!({"ok": true});
        assert_eq!(
            serialize_tool_result(&value).expect("object"),
            "{\"ok\":true}"
        );
        let error = serialize_tool_result(&json!([1, 2, 3])).expect_err("array");
        assert_eq!(error.reason, "result_not_object");

        let big = json!({"text": "中".repeat(5_000)});
        let content = serialize_tool_result(&big).expect("object big");
        assert!(content.len() <= TOOL_RESULT_MAX_BYTES);
        // ISSUE-074：截断后仍是**合法 JSON**（可在对象内部继续取字段）。
        let parsed: Value = serde_json::from_str(&content).expect("truncated tool result is JSON");
        let text = parsed["text"].as_str().expect("text remains a string");
        assert!(text.contains("…[truncated "), "missing marker: {text}");
        assert!(text.ends_with(" bytes]"), "marker tail: {text}");
        assert!(
            text.starts_with("中"),
            "string value keeps original prefix（字符边界）"
        );
        // 字符边界合法（不存在半截码点）。
        assert!(std::str::from_utf8(content.as_bytes()).is_ok());
    }

    #[test]
    fn oversized_ascii_string_is_truncated_inside_json_to_valid_json() {
        let big = json!({"text": "x".repeat(20_000)});
        let content = serialize_tool_result(&big).expect("object");
        assert!(
            content.len() <= TOOL_RESULT_MAX_BYTES,
            "len={}",
            content.len()
        );
        let parsed: Value = serde_json::from_str(&content).expect("valid JSON");
        let text = parsed["text"].as_str().expect("text");
        assert!(text.starts_with("xxxx"), "kept prefix");
        assert!(text.ends_with(" bytes]"));
        assert!(text.contains("…[truncated "));
        // 标记前是纯前缀，未混入对象语法残片。
        let marker_at = text.find("…[truncated ").expect("marker");
        assert!(text[..marker_at].chars().all(|ch| ch == 'x'));
    }

    #[test]
    fn oversized_utf8_string_truncation_respects_char_boundary() {
        // 每个汉字 3 字节；保证截断点落在码点边界。
        let big = json!({"text": "汉".repeat(8_000)});
        let content = serialize_tool_result(&big).expect("object");
        assert!(content.len() <= TOOL_RESULT_MAX_BYTES);
        let parsed: Value = serde_json::from_str(&content).expect("valid JSON");
        let text = parsed["text"].as_str().expect("text");
        let marker_at = text.find("…[truncated ").expect("marker");
        assert!(
            text[..marker_at].chars().all(|ch| ch == '汉'),
            "prefix must be whole code points"
        );
    }

    #[test]
    fn oversized_object_degrades_to_valid_json_with_marker() {
        // 大量小属性：无法靠字符串吸收 → 结构降级（保留前缀 + `_truncated` 标记）。
        let mut map = serde_json::Map::new();
        for index in 0..2_000 {
            map.insert(format!("k{index:04}"), json!(index));
        }
        let big = Value::Object(map);
        let content = serialize_tool_result(&big).expect("object");
        assert!(
            content.len() <= TOOL_RESULT_MAX_BYTES,
            "len={}",
            content.len()
        );
        let parsed: Value = serde_json::from_str(&content).expect("valid JSON");
        let object = parsed.as_object().expect("object");
        assert!(
            object.contains_key("_truncated"),
            "structural degrade must mark truncation"
        );
        assert!(
            object
                .get("_truncated")
                .and_then(Value::as_str)
                .is_some_and(|marker| marker.ends_with(" bytes]")),
            "marker shape"
        );
        // 前缀属性保留（第一个 key 仍在）。
        assert!(object.contains_key("k0000"));
    }

    #[test]
    fn oversized_nested_array_degrades_to_valid_json() {
        let items: Vec<Value> = (0..5_000).map(Value::from).collect();
        let big = json!({"items": items, "tail": {"kept": true}});
        let content = serialize_tool_result(&big).expect("object");
        assert!(
            content.len() <= TOOL_RESULT_MAX_BYTES,
            "len={}",
            content.len()
        );
        let parsed: Value = serde_json::from_str(&content).expect("valid JSON");
        assert!(parsed.is_object(), "top-level stays an object");
        assert!(parsed.get("_truncated").is_some() || parsed.get("items").is_some());
    }

    #[test]
    fn args_summary_has_shape_and_limited_preview() {
        let args = json!({"secret_token": "abcdefghij"});
        let summary = summarize_args(&args);
        assert_eq!(summary["keys"], json!(["secret_token"]));
        assert_eq!(summary["bytes"], args.to_string().len());
        // ISSUE-074/075：preview 严格 ≤ MAX_SUMMARY_PREVIEW_CHARS 个字符（含 `…`），
        // 且不再包含完整参数值。
        let preview = summary["preview"].as_str().expect("preview");
        assert!(preview.chars().count() <= MAX_SUMMARY_PREVIEW_CHARS);
        assert!(
            !preview.contains("abcdefghij"),
            "sensitive value leaked: {preview}"
        );
        let long = summarize_result(&format!("{{\"a\":\"{}\"}}", "x".repeat(2_000)), true);
        assert!(
            long["preview"].as_str().expect("preview").chars().count() <= MAX_SUMMARY_PREVIEW_CHARS
        );

        let ok_summary = summarize_result("{\"a\":1,\"b\":2}", true);
        assert_eq!(ok_summary["ok"], true);
        assert_eq!(ok_summary["keys"], json!(["a", "b"]));

        let error = ToolFailure::new("exception", "boom");
        let content = tool_error_content(&error);
        let failed = summarize_result(&content, false);
        assert_eq!(failed["ok"], false);
        assert_eq!(failed["error"]["reason"], "exception");
    }

    /// ISSUE-075：敏感键的值**不得**出现在 preview（结构摘要 + 脱敏）。
    #[test]
    fn args_summary_redacts_sensitive_keys_and_values() {
        let args = json!({
            "api_key": "sk-live-abc123",
            "Authorization": "Bearer tok-999",
            "client_secret": "s3cr3t-value",
            "password": "hunter2",
            "query": "full user prompt text",
            "nested": {"accessToken": "nested-token-value", "count": 3},
            "items": [{"session_id": "sess-xyz"}, "plain item"]
        });
        let summary = summarize_args(&args);
        let rendered = summary.to_string();
        for leaked in [
            "sk-live-abc123",
            "tok-999",
            "s3cr3t-value",
            "hunter2",
            "full user prompt text",
            "nested-token-value",
            "sess-xyz",
            "plain item",
        ] {
            assert!(
                !rendered.contains(leaked),
                "preview leaked {leaked}: {rendered}"
            );
        }
        // 结构摘要仍保留：键名在 keys 中，敏感值一律 `***`。
        assert_eq!(
            summary["keys"],
            json!([
                "api_key",
                "Authorization",
                "client_secret",
                "password",
                "query",
                "nested",
                "items"
            ])
        );
        let preview = summary["preview"].as_str().expect("preview");
        assert_eq!(
            preview.matches(REDACTED_PREVIEW_VALUE).count(),
            5,
            "preview={preview}"
        );
        assert!(preview.chars().count() <= MAX_SUMMARY_PREVIEW_CHARS);
    }

    /// ISSUE-075：超大参数的结构摘要也必须严格 ≤256 字符（字符边界截断）。
    #[test]
    fn args_summary_preview_is_bounded_for_large_payloads() {
        let fields: Map<String, Value> = (0..200)
            .map(|index| (format!("key{index}"), json!("v".repeat(50))))
            .collect();
        let args = Value::Object(fields);
        let summary = summarize_args(&args);
        let preview = summary["preview"].as_str().expect("preview");
        assert!(preview.chars().count() <= MAX_SUMMARY_PREVIEW_CHARS);
        assert!(preview.ends_with('…'), "truncation marker expected");
        assert!(!preview.contains(&"v".repeat(20)), "values must not leak");
    }

    /// ISSUE-075：`result_summary.preview` 同口径脱敏；非 JSON 结果只记尺寸。
    #[test]
    fn result_summary_preview_redacts_sensitive_values() {
        let content = json!({
            "api_key": "res-key-123",
            "value": "RESULT-VALUE",
            "tokens": 42
        })
        .to_string();
        let summary = summarize_result(&content, true);
        let rendered = summary.to_string();
        assert!(!rendered.contains("res-key-123"), "{rendered}");
        assert!(!rendered.contains("RESULT-VALUE"), "{rendered}");
        assert_eq!(summary["keys"], json!(["api_key", "value", "tokens"]));
        assert!(
            summary["preview"]
                .as_str()
                .expect("preview")
                .contains(REDACTED_PREVIEW_VALUE),
            "preview={}",
            summary["preview"]
        );
        // 非 JSON 结果：只记录字符数，正文不进入 preview。
        let raw = "raw tool text SECRET-BODY";
        let text_result = summarize_result(raw, true);
        assert!(!text_result.to_string().contains("SECRET-BODY"));
        assert_eq!(text_result["keys"], json!([]));
        assert_eq!(text_result["bytes"], raw.len());
    }

    #[test]
    fn tool_error_content_respects_byte_limit_and_stays_valid_json() {
        let failure = ToolFailure {
            reason: "exception".to_string(),
            message: "m".repeat(20_000),
            error_type: Some("ValueError".to_string()),
            traceback: Some("t".repeat(20_000)),
            timeout_ms: None,
        };
        let content = tool_error_content(&failure);
        assert!(
            content.len() <= TOOL_RESULT_MAX_BYTES,
            "len={}",
            content.len()
        );
        // ISSUE-074：喂回 LLM 的错误 JSON 必须可解析。
        let parsed: Value = serde_json::from_str(&content).expect("error content is JSON");
        assert_eq!(parsed["error"]["reason"], json!("exception"));
        assert_eq!(parsed["error"]["type"], json!("ValueError"));
        // 超长 message/traceback 在字符串内截断（带标记）。
        let message = parsed["error"]["message"].as_str().expect("message");
        assert!(message.starts_with('m'));
        assert!(
            message.contains("…[truncated "),
            "message marker: {}",
            &message[..40]
        );
    }

    #[test]
    fn long_traceback_is_capped_at_eight_kib_in_tool_failure_json() {
        let failure = ToolFailure {
            reason: "exception".to_string(),
            message: "boom".to_string(),
            error_type: Some("RuntimeError".to_string()),
            traceback: Some("汉".repeat(20_000)),
            timeout_ms: None,
        };
        let json_value = failure.to_json();
        let traceback = json_value["traceback"].as_str().expect("traceback");
        assert!(
            traceback.len() <= TOOL_RESULT_MAX_BYTES,
            "traceback {} bytes > 8 KiB",
            traceback.len()
        );
        // 字符边界：不切断 UTF-8 码点。
        assert!(traceback.chars().all(|ch| ch == '汉'));
    }

    #[test]
    fn truncate_chars_is_strict_upper_bound() {
        let text = "中".repeat(100);
        let summary = truncate_chars(&text, 10);
        assert_eq!(summary.chars().count(), 10);
        assert!(summary.ends_with('…'));
        assert_eq!(truncate_chars(&text, 0), String::new());
        assert_eq!(truncate_chars("短", 10), "短");
    }

    #[tokio::test]
    async fn fake_runner_records_calls_and_returns_behaviors() {
        let runner = FakeToolRunner::with_tools(vec![tool(
            "now",
            json!({"type": "object", "properties": {}}),
        )])
        .behavior("now", FakeToolBehavior::Return(json!({"now": "t"})));
        let discovered = runner.discover(Path::new("/tmp/tools"), ctx()).await;
        assert_eq!(discovered.expect("discover").len(), 1);
        assert_eq!(runner.discover_calls(), 1);
        let request = ToolCallRequest {
            call_id: "c1".to_string(),
            tool_name: "now".to_string(),
            module: "tools.builtin".to_string(),
            tools_dir: std::path::PathBuf::from("/tmp/tools"),
            args: json!({}),
            timeout_ms: 30_000,
        };
        let result = runner.call(request, ctx()).await;
        assert_eq!(result.expect("call"), json!({"now": "t"}));
        assert_eq!(runner.calls().len(), 1);
    }

    #[tokio::test]
    async fn fake_runner_injects_exception_and_timeout() {
        let runner = FakeToolRunner::new()
            .behavior(
                "boom",
                FakeToolBehavior::Exception {
                    error_type: "ValueError".to_string(),
                    message: "bad".to_string(),
                },
            )
            .behavior("slow", FakeToolBehavior::Timeout);
        let request = |name: &str| ToolCallRequest {
            call_id: "c1".to_string(),
            tool_name: name.to_string(),
            module: "tools.builtin".to_string(),
            tools_dir: std::path::PathBuf::from("/tmp/tools"),
            args: json!({}),
            timeout_ms: 10,
        };
        let error = runner
            .call(request("boom"), ctx())
            .await
            .expect_err("exception");
        assert_eq!(error.reason, "exception");
        let error = runner
            .call(request("slow"), ctx())
            .await
            .expect_err("timeout");
        assert_eq!(error.reason, "timeout");
    }

    #[test]
    fn discover_timeout_defaults_and_validates_range() {
        let tools_dir = Path::new("/tmp/tools");
        let output = json!({
            "ok": true,
            "tools": [
                {"name": "now", "description": "d", "source": "tools.builtin",
                 "parameters": {"type": "object", "properties": {}}},
                {"name": "slow", "description": "d", "source": "tools.builtin",
                 "parameters": {"type": "object", "properties": {}}, "timeout_ms": 300}
            ]
        });
        let defs = parse_discover_output(&output, tools_dir).expect("discover");
        assert_eq!(defs[0].timeout_ms, DEFAULT_TOOL_TIMEOUT_MS);
        assert_eq!(defs[1].timeout_ms, 300);

        let invalid = json!({
            "ok": true,
            "tools": [
                {"name": "bad", "description": "d", "source": "tools.builtin",
                 "parameters": {"type": "object", "properties": {}}, "timeout_ms": 0}
            ]
        });
        let error = parse_discover_output(&invalid, tools_dir).expect_err("invalid timeout");
        assert_eq!(error.detail["reason"], "invalid_tool_timeout");
    }

    #[test]
    fn nested_forbidden_schema_keyword_is_rejected() {
        let error = ToolRegistry::new(vec![tool(
            "now",
            json!({
                "type": "object",
                "properties": {"x": {"oneOf": [{"type": "string"}]}}
            }),
        )])
        .expect_err("nested forbidden");
        assert_eq!(error.detail["reason"], "unsupported_schema_keyword");
        assert_eq!(error.detail["json_path"], "$.parameters.properties.x.oneOf");
    }

    #[test]
    fn worker_tool_runner_adopts_injected_config() {
        let config = zgraph_worker::WorkerConfig {
            program: "python-custom".to_string(),
            args: vec!["-m".to_string(), "zgraph_worker.runtime".to_string()],
            env: vec![("PYTHONPATH".to_string(), "/repo/python".to_string())],
            cwd: Some(std::path::PathBuf::from("/repo")),
            timeout: Duration::from_secs(5),
            hello_timeout: Duration::from_secs(5),
            exit_grace: Duration::from_secs(1),
            max_stderr_bytes: 4096,
        };
        let runner = WorkerToolRunner::new(config);
        assert_eq!(runner.config().program, "python-custom");
        assert_eq!(
            runner.config().env,
            vec![("PYTHONPATH".to_string(), "/repo/python".to_string())]
        );
        assert_eq!(runner.config().cwd.as_deref(), Some(Path::new("/repo")));
        // 每次调用覆盖 timeout = 生效值 + 兜底余量；其余字段透传。
        let effective = runner.effective_config(300);
        assert_eq!(effective.timeout, Duration::from_millis(1_300));
        assert_eq!(effective.program, "python-custom");
        assert_eq!(effective.hello_timeout, Duration::from_secs(5));
    }

    #[tokio::test]
    async fn worker_tool_runner_roundtrip_with_real_python() {
        // D3 真实子进程契约（py-worker fixtures，已合入 main）：
        // discover → ToolDef（含可选 timeout_ms）+ call 成功/异常/超时。
        let Some((root, tools_dir)) = real_tool_fixture() else {
            eprintln!("skip: py-worker tool fixtures not present");
            return;
        };
        let runner = WorkerToolRunner::new(real_worker_config(&root));
        let ctx: Arc<dyn HarnessCtx> = TestCtx::new();
        let defs = runner
            .discover(&tools_dir, ctx.clone())
            .await
            .expect("discover real worker");
        assert_discovered_tools(&defs);
        assert_real_tool_calls(&runner, &defs, &tools_dir, ctx).await;
    }

    /// 真实子进程 fixture：返回 `(repo 根, tools_dir)`；fixtures 缺失 → `None`（调用方跳过）。
    fn real_tool_fixture() -> Option<(std::path::PathBuf, std::path::PathBuf)> {
        let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .canonicalize()
            .expect("repo root");
        let tools_dir = root.join("tests/fixtures/workers/tools/tools_ok/tools");
        tools_dir.is_dir().then_some((root, tools_dir))
    }

    /// 真实 `python -m zgraph_worker.runtime` 的工具执行后端配置。
    fn real_worker_config(root: &std::path::Path) -> zgraph_worker::WorkerConfig {
        let program = std::env::var("ZGRAPH_PYTHON").unwrap_or_else(|_| "python3".to_string());
        zgraph_worker::WorkerConfig {
            program,
            args: vec![
                "-m".to_string(),
                crate::constants::WORKER_RUNTIME_MODULE.to_string(),
            ],
            env: vec![(
                "PYTHONPATH".to_string(),
                root.join("python").display().to_string(),
            )],
            cwd: Some(root.to_path_buf()),
            timeout: Duration::from_secs(30),
            hello_timeout: Duration::from_secs(5),
            exit_grace: Duration::from_secs(1),
            max_stderr_bytes: 8 * 1024,
        }
    }

    /// discover 结果的工具名集合与 `timeout_ms` 采纳语义。
    fn assert_discovered_tools(defs: &[ToolDef]) {
        let names: Vec<&str> = defs.iter().map(|tool| tool.name.as_str()).collect();
        for expected in ["add", "echo", "explode", "sleepy", "now"] {
            assert!(
                names.contains(&expected),
                "missing tool {expected}: {names:?}"
            );
        }
        let add = defs.iter().find(|tool| tool.name == "add").expect("add");
        assert_eq!(add.timeout_ms, DEFAULT_TOOL_TIMEOUT_MS);
        assert!(add.source.starts_with("tools."), "source: {}", add.source);
        let sleepy = defs
            .iter()
            .find(|tool| tool.name == "sleepy")
            .expect("sleepy");
        assert_eq!(sleepy.timeout_ms, 300, "declared timeout_ms is adopted");
    }

    /// 真实子进程 call 成功/异常/超时三条路径。
    async fn assert_real_tool_calls(
        runner: &WorkerToolRunner,
        defs: &[ToolDef],
        tools_dir: &std::path::Path,
        ctx: Arc<dyn HarnessCtx>,
    ) {
        let request = |name: &str, args: Value, timeout_ms: u64| ToolCallRequest {
            call_id: "c1".to_string(),
            tool_name: name.to_string(),
            module: defs
                .iter()
                .find(|tool| tool.name == name)
                .map(|tool| tool.source.clone())
                .expect("tool discovered"),
            tools_dir: tools_dir.to_path_buf(),
            args,
            timeout_ms,
        };
        let result = runner
            .call(request("add", json!({"a": 2, "b": 3}), 30_000), ctx.clone())
            .await
            .expect("call add");
        assert_eq!(result["sum"], 5);

        let explode = runner
            .call(request("explode", json!({}), 30_000), ctx.clone())
            .await
            .expect_err("explode raises");
        assert_eq!(explode.reason, "exception");
        assert_eq!(explode.error_type.as_deref(), Some("ValueError"));
        assert!(explode.message.contains("boom"));

        let timeout = runner
            .call(request("sleepy", json!({"seconds": 5}), 300), ctx)
            .await
            .expect_err("sleepy times out");
        assert_eq!(timeout.reason, "timeout");
    }

    #[test]
    fn tool_name_rules_match_spec() {
        assert!(is_tool_name("now"));
        assert!(is_tool_name("http_get"));
        assert!(is_tool_name("a1"));
        assert!(!is_tool_name("Now"));
        assert!(!is_tool_name("1now"));
        assert!(!is_tool_name("now.get"));
        assert!(!is_tool_name(&"a".repeat(65)));
    }
}
