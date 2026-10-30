//! `zgraph run` 子命令（M5）：加载图 → 组合真实执行器 → 跑完 run → trace 落库与输出。
//!
//! 语义基准：`doc/spec-v1.2/06-存储与可观测.md` §6.10.3（参数/启动顺序/输出）、
//! §6.10.5（退出码）与 §6.10.6（`--json` 结构）；`doc/architecture-v2.md` §5.2/§5.3
//! （`--record` 与 `recorded_outputs`）；`doc/spec-v1.2/07-验收用例.md` AC-01/AC-08。
//!
//! # 启动顺序（06 §6.10.3）
//!
//! 打开/迁移 DB（失败 → 4）→ 加载图（DSL 错误 → 2，不产生 run）→ 校验入口输入
//! （→ 3，不产生 run）→ 组合执行器（`agent_call` → [`HarnessExecutor`]；其余 → 真
//! Python 子进程 [`WorkerExecutor`]）→ [`bridge::start_traced_run_with_writer`] →
//! 等待（SIGINT → cancel）→ 输出。
//!
//! # 执行器组合
//!
//! ```text
//! kernel ──> RecordingExecutor（成功输出 → recorded_outputs）
//!              └─> RoutingExecutor（按 CompiledHandler 路由）
//!                    ├─ Agent        -> HarnessExecutor（fake/real LLM + WorkerToolRunner）
//!                    └─ Python       -> WorkerExecutor（python -m zgraph_worker.runtime）
//! ```
//!
//! `RecordedOutput` 只对**成功**执行发送：`full` 带 `output_json`，`hash` 只带
//! `output_hash`，`none` 不发（architecture §5.2）。agent 节点与 python 节点同规则
//! （agent 输出为整包四字段，05 §5.9.2）。LLM `llm_calls` 记录不在本轮（ISSUE-082）。
//!
//! # 解释器与特殊库
//!
//! - Python 解释器顺序：`--python` > `$PYO3_PYTHON` > `$ZGRAPH_PYTHON` > `python3`
//!   （04 §4.3.6 / 06 §6.10.3 / 08 N-02）。
//! - `--db :memory:`：接受并打 stderr 警告（跨进程不可读，06 §6.9.1）；run 结果直接
//!   来自内核 outcome（`event_count` 记 0），不打开只读 reader。
//! - 所有 panic 由 CLI 顶层 `catch_unwind` 捕获并返回退出码 70（见 `crate::run_with`）。
//!
//! # human 生命周期（ISSUE-085 / 06 §6.10.3）
//!
//! 组合根订阅 [`bridge::LifecycleObserver`] 的内存事件旁路：run **运行中**逐行把
//! `[seq N] Kind ...` 写到 stderr，不再等 run 结束回读 trace。`--json` 不订阅
//! （stdout 契约不变）、`-q/--quiet` 不订阅（stderr 保持静默）、`:memory:` 同样走
//! 旁路；`--stream` 仍负责 `TokenDelta`/`ReasoningDelta` 的原始文本。

use std::collections::HashMap;
use std::future::Future;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::process::ExitCode;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc;
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Instant;

use serde_json::{json, Map, Value};

use zgraph_dsl::{load_graph_with_options, LoadOptions, LoadedGraph, ValidationReport};
use zgraph_harness::{
    FakeLlmProvider, GenaiLlmProvider, HarnessExecutor, HarnessSettings, LlmCallRecorder,
    LlmCallSnapshot, LlmProvider, WorkerToolRunner,
};
use zgraph_kernel::{
    CompiledGraph, CompiledHandler, CompiledNode, NodeError, NodeExecution, NodeExecutor, RunError,
    RunOptions, RunOutcome, RunSpec, RunStatus, StreamingEvent, CODE_RUN_ENTRY_INPUT_MISSING,
    CODE_RUN_ENTRY_INVALID,
};
use zgraph_trace::{
    LlmCallRecord, RecordMode, RecordedOutput, TraceError, TraceHandle, TraceInput, TraceReader,
    TraceWriter,
};
use zgraph_worker::{SupervisorConfig, WorkerConfig, WorkerExecutor, WorkerLimits};

use crate::bridge::{
    self, BridgeError, BridgeObservers, LifecycleObserver, StreamingObserver, TracedRun,
};
use crate::recovery;
use crate::trace_cmd::resolve_db_path;
use crate::{
    EXIT_ENTRY_INPUT, EXIT_INTERRUPTED, EXIT_OTHER, EXIT_STORAGE, EXIT_SUCCESS, EXIT_USAGE,
};

/// `run --help` 的用法文本。
pub const RUN_USAGE: &str = "\
usage: zgraph run <graph.json> --inputs <json|@file> [options]

options:
  --inputs <json|@file>  entry inputs: {node_id:{port:value}} (always) or
                         {port:value} when the graph has exactly one entry point
  --user-id <id>         run user id (default: local)
  --db <path>            trace database (default: $ZGRAPH_DB or .zgraph/trace.db)
  --assets-dir <dir>     agent asset root (default: $ZGRAPH_ASSETS or .)
  --tools-dir <dir>      tool directory (default: $ZGRAPH_TOOLS or <assets-dir>/tools)
  --python <exe>         Python interpreter (default: $PYO3_PYTHON > $ZGRAPH_PYTHON > python3)
  --python-path <dir>    prepend a module search path; repeatable (PYTHONPATH)
  --fake-llm[=<script>]  scripted LLM provider (bare flag = built-in default script)
  --real                 real LLM provider (mutually exclusive with --fake-llm)
  --record <mode>        recorded output mode: none|hash|full (default: hash)
  --stream               stream token text to stderr while the run is executing
  --json                 emit machine-readable JSON on stdout (06 §6.10.6)
  -q, --quiet            suppress human-readable progress on stderr
  --help, -h             print this help";

/// 默认 user id（06 §6.10.3）。
pub const DEFAULT_USER_ID: &str = "local";

/// 资产根缺省值（06 §6.10.3）。
pub const DEFAULT_ASSETS_DIR: &str = ".";

/// Python 解释器缺省值（`--python`/`PYO3_PYTHON`/`ZGRAPH_PYTHON` 都缺省时）。
pub const DEFAULT_PYTHON: &str = "python3";

/// 资产根环境变量（06 §6.10.1）。
pub const ASSETS_ENV: &str = "ZGRAPH_ASSETS";

/// 工具目录环境变量（06 §6.10.1）。
pub const TOOLS_ENV: &str = "ZGRAPH_TOOLS";

/// Python 解释器环境变量（spec 权威口径：04 §4.3.6 / 06 §6.10.3 / 08 N-02）。
pub const PYO3_PYTHON_ENV: &str = "PYO3_PYTHON";

/// Python 解释器环境变量（M3+ 任务口径，`PYO3_PYTHON` 之后、`python3` 之前）。
pub const PYTHON_ENV: &str = "ZGRAPH_PYTHON";

/// 进程内 SQLite 库（06 §6.9.1：接受但跨进程不可读，`run` 用 outcome 直接输出）。
pub const MEMORY_DB: &str = ":memory:";

/// 裸 `--fake-llm` 的内置默认脚本（05 §5.3.4：一段文本 + 一次工具调用）。
///
/// 多 agent 图会按脚本顺序继续消费；`calls` 用尽后重复最后一项（05 §5.3.4）。
pub const DEFAULT_FAKE_LLM_SCRIPT: &str = r#"{
  "version": 1,
  "chunk_chars": 3,
  "chunk_delay_ms": 0,
  "calls": [
    {"kind": "text", "content": "{\"intent\":\"refund\"}", "stop_reason": "EndTurn",
     "usage": {"prompt_tokens": 120, "completion_tokens": 12}},
    {"kind": "tool_calls", "content": null, "stop_reason": "ToolCalls",
     "tool_calls": [{"call_id": "call_1", "name": "now", "args": {}}],
     "usage": {"prompt_tokens": 300, "completion_tokens": 20}}
  ],
  "failures": []
}"#;

/// 入口输入：`node_id -> {port: value}`（与 kernel `RunSpec.entry_inputs` 同形）。
pub type EntryInputs = HashMap<String, HashMap<String, Value>>;

/// `--fake-llm` 的三种形态。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FakeLlmMode {
    /// 未启用。
    Off,
    /// 裸 `--fake-llm`：内置默认脚本。
    DefaultScript,
    /// `--fake-llm=<path>`：读取脚本文件。
    Script(PathBuf),
}

/// `run` 的参数（解析后；路径/env 在运行时解析，便于单测）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunArgs {
    /// 图定义文件。
    pub graph: PathBuf,
    /// `--inputs` 原文（JSON 字符串或 `@file`）。
    pub inputs: String,
    /// `runs.user_id` / `GateContext.user_id`。
    pub user_id: String,
    /// `--db` 显式路径（`None` 时按 `$ZGRAPH_DB` → 默认值解析）。
    pub db: Option<PathBuf>,
    /// `--assets-dir` 显式路径（`None` 时按 `$ZGRAPH_ASSETS` → `.` 解析）。
    pub assets_dir: Option<PathBuf>,
    /// `--tools-dir` 显式路径（`None` 时按 `$ZGRAPH_TOOLS` → `<assets-dir>/tools`）。
    pub tools_dir: Option<PathBuf>,
    /// `--python` 显式解释器（`None` 时按 `$ZGRAPH_PYTHON` → `python3`）。
    pub python: Option<String>,
    /// `--python-path`（可重复；按序叠加到 `PYTHONPATH` 之前）。
    pub python_paths: Vec<PathBuf>,
    /// `--fake-llm[=<script>]`。
    pub fake_llm: FakeLlmMode,
    /// `--real`。
    pub real: bool,
    /// `--record`（默认 `hash`）。
    pub record: RecordMode,
    /// `--stream`：`TokenDelta`/`ReasoningDelta` 文本实时写 stderr。
    pub stream: bool,
    /// `--json`：stdout 只输出 06 §6.10.6 结构。
    pub json: bool,
    /// `-q/--quiet`：抑制 stderr 人类可读进度（不抑制错误/流式文本）。
    pub quiet: bool,
}

/// 解析 `run` 之后的参数；`--help`/`-h` 返回 `Ok(None)`。
pub fn parse(args: &[String]) -> Result<Option<RunArgs>, String> {
    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        return Ok(None);
    }
    let mut graph = None;
    let mut inputs = None;
    let mut user_id = DEFAULT_USER_ID.to_string();
    let mut db = None;
    let mut assets_dir = None;
    let mut tools_dir = None;
    let mut python = None;
    let mut python_paths = Vec::new();
    let mut fake_llm = FakeLlmMode::Off;
    let mut real = false;
    let mut record = RecordMode::Hash;
    let mut stream = false;
    let mut json = false;
    let mut quiet = false;
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        if let Some(path) = arg.strip_prefix("--fake-llm=") {
            fake_llm = FakeLlmMode::Script(non_empty_path(path, "--fake-llm")?);
            continue;
        }
        if let Some(value) = arg.strip_prefix("--record=") {
            record = parse_record(value)?;
            continue;
        }
        match arg.as_str() {
            "--inputs" => inputs = Some(option_value(&mut iter, "--inputs")?),
            "--user-id" => user_id = option_value(&mut iter, "--user-id")?,
            "--db" => db = Some(PathBuf::from(option_value(&mut iter, "--db")?)),
            "--assets-dir" => {
                assets_dir = Some(PathBuf::from(option_value(&mut iter, "--assets-dir")?));
            }
            "--tools-dir" => {
                tools_dir = Some(PathBuf::from(option_value(&mut iter, "--tools-dir")?));
            }
            "--python" => python = Some(option_value(&mut iter, "--python")?),
            "--python-path" => {
                python_paths.push(PathBuf::from(option_value(&mut iter, "--python-path")?));
            }
            "--fake-llm" => fake_llm = FakeLlmMode::DefaultScript,
            "--real" => real = true,
            "--record" => record = parse_record(&option_value(&mut iter, "--record")?)?,
            "--stream" => stream = true,
            "--json" => json = true,
            "-q" | "--quiet" => quiet = true,
            flag if flag.starts_with('-') => return Err(format!("unknown option: {flag}")),
            value => {
                if graph.is_some() {
                    return Err(format!("unexpected argument: {value}"));
                }
                graph = Some(PathBuf::from(value));
            }
        }
    }
    if real && fake_llm != FakeLlmMode::Off {
        return Err("--real conflicts with --fake-llm".to_string());
    }
    let graph = graph.ok_or_else(|| "run requires <graph.json>".to_string())?;
    let inputs = inputs.ok_or_else(|| "run requires --inputs <json|@file>".to_string())?;
    Ok(Some(RunArgs {
        graph,
        inputs,
        user_id,
        db,
        assets_dir,
        tools_dir,
        python,
        python_paths,
        fake_llm,
        real,
        record,
        stream,
        json,
        quiet,
    }))
}

/// 取一个必填选项值。
fn option_value<'a>(
    iter: &mut impl Iterator<Item = &'a String>,
    option: &str,
) -> Result<String, String> {
    iter.next()
        .cloned()
        .ok_or_else(|| format!("{option} requires a value"))
}

/// `--fake-llm=<path>` 的路径不能为空。
fn non_empty_path(value: &str, option: &str) -> Result<PathBuf, String> {
    if value.is_empty() {
        return Err(format!("{option}=<path> requires a non-empty path"));
    }
    Ok(PathBuf::from(value))
}

/// 解析 `--record` 取值（`none|hash|full`）。
fn parse_record(value: &str) -> Result<RecordMode, String> {
    RecordMode::parse(value)
        .ok_or_else(|| format!("invalid --record '{value}' (expected none|hash|full)"))
}

/// `--inputs` 的非法形态（06 §6.10.3：`RUN_ENTRY_*`，退出码 3，不产生 run）。
#[derive(Debug, Clone, PartialEq, Eq)]
struct EntryInputError {
    code: &'static str,
    message: String,
}

impl EntryInputError {
    fn invalid(message: impl Into<String>) -> Self {
        Self {
            code: CODE_RUN_ENTRY_INVALID,
            message: message.into(),
        }
    }

    fn missing(message: impl Into<String>) -> Self {
        Self {
            code: CODE_RUN_ENTRY_INPUT_MISSING,
            message: message.into(),
        }
    }
}

/// 读取 `--inputs` 原文：`@file` 读文件，否则直接当 JSON 文本。
fn read_inputs_value(raw: &str) -> Result<Value, EntryInputError> {
    let text = if let Some(path) = raw.strip_prefix('@') {
        std::fs::read_to_string(path).map_err(|error| {
            EntryInputError::invalid(format!("cannot read --inputs file '{path}': {error}"))
        })?
    } else {
        raw.to_string()
    };
    serde_json::from_str(&text)
        .map_err(|error| EntryInputError::invalid(format!("--inputs is not valid JSON: {error}")))
}

/// 把 `--inputs` 归一到 `node_id -> {port: value}` 并做入口校验。
///
/// 形态规则（06 §6.10.3）：嵌套**总是允许**；恰好 1 个 entry 时允许 `{port: value}`
/// 简写（自动归属该 entry）。非 entry 节点 / 未声明端口 → `RUN_ENTRY_INVALID`；
/// required 端口缺失 → `RUN_ENTRY_INPUT_MISSING`。
fn resolve_entry_inputs(
    graph: &CompiledGraph,
    raw: &Value,
) -> Result<EntryInputs, EntryInputError> {
    let object = raw.as_object().ok_or_else(|| {
        EntryInputError::invalid(
            "--inputs must be a JSON object ({node_id: {port: value}} or single-entry shorthand)",
        )
    })?;
    let entries = &graph.entry_points;
    let shorthand = entries.len() == 1 && !object.keys().any(|key| entries.contains(key));
    let mut inputs: EntryInputs = HashMap::new();
    if shorthand {
        let entry = entries
            .first()
            .expect("shorthand requires exactly one entry point");
        inputs.insert(entry.clone(), validate_ports(graph, entry, object)?);
    } else {
        for (node_id, value) in object {
            if !entries.contains(node_id) {
                return Err(EntryInputError::invalid(format!(
                    "--inputs node '{node_id}' is not an entry point"
                )));
            }
            let ports = value.as_object().ok_or_else(|| {
                EntryInputError::invalid(format!(
                    "--inputs['{node_id}'] must be an object {{port: value}}"
                ))
            })?;
            inputs.insert(node_id.clone(), validate_ports(graph, node_id, ports)?);
        }
    }
    require_entry_ports(graph, &inputs)?;
    Ok(inputs)
}

/// 校验某入口节点的端口集合（未声明端口 → `RUN_ENTRY_INVALID`）。
fn validate_ports(
    graph: &CompiledGraph,
    node_id: &str,
    ports: &Map<String, Value>,
) -> Result<HashMap<String, Value>, EntryInputError> {
    let node = graph
        .nodes
        .get(node_id)
        .ok_or_else(|| EntryInputError::invalid(format!("unknown node '{node_id}'")))?;
    for port in ports.keys() {
        if !node.inputs.iter().any(|declared| &declared.name == port) {
            return Err(EntryInputError::invalid(format!(
                "entry node '{node_id}' has no input port '{port}'"
            )));
        }
    }
    Ok(ports
        .iter()
        .map(|(port, value)| (port.clone(), value.clone()))
        .collect())
}

/// 每个入口节点的 required 端口都必须被覆盖（`RUN_ENTRY_INPUT_MISSING`）。
fn require_entry_ports(graph: &CompiledGraph, inputs: &EntryInputs) -> Result<(), EntryInputError> {
    for entry in &graph.entry_points {
        let Some(node) = graph.nodes.get(entry) else {
            return Err(EntryInputError::invalid(format!(
                "entry point '{entry}' is not a declared node"
            )));
        };
        for port in node.inputs.iter().filter(|port| port.required) {
            let provided = inputs
                .get(entry)
                .is_some_and(|ports| ports.contains_key(&port.name));
            if !provided {
                return Err(EntryInputError::missing(format!(
                    "entry node '{entry}' required port '{}' is not provided",
                    port.name
                )));
            }
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 执行器组合
// ---------------------------------------------------------------------------

/// 按 `CompiledHandler` 路由：`Agent` → Harness，其余 → 真 Python worker 子进程。
struct RoutingExecutor {
    harness: Arc<HarnessExecutor>,
    worker: Arc<WorkerExecutor>,
}

impl NodeExecutor for RoutingExecutor {
    fn execute<'a>(
        &'a self,
        execution: NodeExecution<'a>,
    ) -> Pin<Box<dyn Future<Output = Result<Value, NodeError>> + Send + 'a>> {
        if matches!(&execution.node.handler, CompiledHandler::Agent { .. }) {
            self.harness.execute(execution)
        } else {
            self.worker.execute(execution)
        }
    }

    /// 转发 Harness 的 `timeout_hint`（ISSUE-076；否则包装层默认 `None` 会截断）。
    fn timeout_hint(&self, node: &CompiledNode) -> Option<u64> {
        if matches!(&node.handler, CompiledHandler::Agent { .. }) {
            self.harness.timeout_hint(node)
        } else {
            None
        }
    }
}

/// 在真实执行器外再包一层：每次**成功**执行发送一条 `RecordedOutput`。
struct RecordingExecutor {
    inner: Arc<dyn NodeExecutor>,
    handle: TraceHandle,
    mode: RecordMode,
}

impl RecordingExecutor {
    fn new(inner: Arc<dyn NodeExecutor>, handle: TraceHandle, mode: RecordMode) -> Self {
        Self {
            inner,
            handle,
            mode,
        }
    }
}

impl NodeExecutor for RecordingExecutor {
    fn execute<'a>(
        &'a self,
        execution: NodeExecution<'a>,
    ) -> Pin<Box<dyn Future<Output = Result<Value, NodeError>> + Send + 'a>> {
        let run_id = execution.ctx.run_id().to_string();
        let instance_id = execution.ctx.instance_id().to_string();
        let node_id = execution.ctx.node_id().to_string();
        let attempt = execution.ctx.attempt();
        let inner = self.inner.execute(execution);
        let handle = self.handle.clone();
        let mode = self.mode;
        Box::pin(async move {
            let output = inner.await?;
            record_output(
                &handle,
                mode,
                &run_id,
                &instance_id,
                &node_id,
                attempt,
                &output,
            );
            Ok(output)
        })
    }

    /// 透明转发内层执行器的 `timeout_hint`（ISSUE-076；包装层不得截断）。
    fn timeout_hint(&self, node: &CompiledNode) -> Option<u64> {
        self.inner.timeout_hint(node)
    }
}

/// `llm_calls` 写端：把 harness `LlmCallRecorder` 快照转成 `TraceInput::LlmCall`。
///
/// 仅 `--record=full` 时挂接（DR-002 §3.2）；request 已由 harness 脱敏（无 `api_key`）。
/// 记录器只做内存 `send`（不阻塞、不做 DB IO），符合 harness seam 的纪律。
struct LlmCallTraceRecorder {
    handle: TraceHandle,
    run_id: String,
}

impl LlmCallRecorder for LlmCallTraceRecorder {
    fn record_llm_call(&self, call: &LlmCallSnapshot<'_>) {
        self.handle.send(TraceInput::LlmCall(LlmCallRecord {
            run_id: self.run_id.clone(),
            instance_id: call.instance_id.to_string(),
            call_index: call.call_index,
            request_json: call.request.clone(),
            response_json: call.response.cloned(),
            status: call.status.to_string(),
            recorded_at: now_rfc3339(),
        }));
    }
}

/// 成功输出 → `RecordedOutput`（`full` 带全文，`hash` 只带哈希，`none` 不发）。
fn record_output(
    handle: &TraceHandle,
    mode: RecordMode,
    run_id: &str,
    instance_id: &str,
    node_id: &str,
    attempt: u32,
    output: &Value,
) {
    if mode == RecordMode::None {
        return;
    }
    let output_json = (mode == RecordMode::Full).then(|| output.clone());
    handle.send(TraceInput::RecordedOutput(RecordedOutput {
        run_id: run_id.to_string(),
        instance_id: instance_id.to_string(),
        node_id: node_id.to_string(),
        attempt,
        output_hash: output_hash(output),
        output_json,
        recorded_at: now_rfc3339(),
    }));
}

/// L2 输出哈希：`blake3(canonical_json(output))` 的 64 位小写 hex（architecture §5.2）。
pub fn output_hash(output: &Value) -> String {
    blake3::hash(zgraph_dsl::canonical_json(output).as_bytes())
        .to_hex()
        .to_string()
}

/// 当前 UTC 时间（RFC3339 毫秒 Z，与 trace/kernel 的 `recorded_at` 口径一致）。
fn now_rfc3339() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

// ---------------------------------------------------------------------------
// 运行环境解析与构造
// ---------------------------------------------------------------------------

/// 资产根：`--assets-dir` > `$ZGRAPH_ASSETS` > `.`（06 §6.10.3）。
fn resolve_assets_dir(args: &RunArgs) -> PathBuf {
    args.assets_dir
        .clone()
        .or_else(|| env_path(ASSETS_ENV))
        .unwrap_or_else(|| PathBuf::from(DEFAULT_ASSETS_DIR))
}

/// 工具目录：`--tools-dir` > `$ZGRAPH_TOOLS` > `<assets-dir>/tools`。
fn resolve_tools_dir(args: &RunArgs, assets_dir: &Path) -> PathBuf {
    args.tools_dir
        .clone()
        .or_else(|| env_path(TOOLS_ENV))
        .unwrap_or_else(|| assets_dir.join("tools"))
}

/// 读取非空环境变量为路径。
fn env_path(name: &str) -> Option<PathBuf> {
    let value = std::env::var_os(name)?;
    if value.is_empty() {
        None
    } else {
        Some(PathBuf::from(value))
    }
}

/// 读取非空环境变量为字符串。
fn env_string(name: &str) -> Option<String> {
    let value = std::env::var_os(name)?;
    if value.is_empty() {
        None
    } else {
        Some(value.to_string_lossy().into_owned())
    }
}

/// 是否为进程内 SQLite 库（`--db :memory:`，06 §6.9.1）。
fn is_memory_db(path: &Path) -> bool {
    path == Path::new(MEMORY_DB)
}

/// `:memory:` 的 stderr 警告（06 §6.9.1：接受但跨进程不可读）。
fn warn_memory_db() {
    eprintln!(
        "warning: --db {MEMORY_DB} is process-local; the trace cannot be read after this run (06 §6.9.1)"
    );
}

/// Python 解释器解析顺序（spec 权威）：`--python` > `$PYO3_PYTHON` > `$ZGRAPH_PYTHON`
/// > `python3`（04 §4.3.6 / 06 §6.10.3 / 08 N-02；`ZGRAPH_PYTHON` 保留给 e2e/测试覆盖）。
fn resolve_python(args: &RunArgs) -> String {
    args.python
        .clone()
        .or_else(|| env_string(PYO3_PYTHON_ENV))
        .or_else(|| env_string(PYTHON_ENV))
        .unwrap_or_else(|| DEFAULT_PYTHON.to_string())
}

/// 构造 Python worker 子进程配置（worker 与 tool runner 共用）。
fn worker_config(args: &RunArgs) -> WorkerConfig {
    let program = resolve_python(args);
    let env = python_path_env(&args.python_paths)
        .map(|value| vec![("PYTHONPATH".to_string(), value)])
        .unwrap_or_default();
    WorkerConfig {
        program,
        args: vec!["-m".to_string(), "zgraph_worker.runtime".to_string()],
        env,
        cwd: None,
        timeout: std::time::Duration::from_secs(180),
        hello_timeout: std::time::Duration::from_secs(5),
        exit_grace: std::time::Duration::from_secs(1),
        max_stderr_bytes: 8 * 1024,
    }
}

/// 组合 `PYTHONPATH`：`--python-path` 按序在前，继承的 `PYTHONPATH` 在后（叠加）。
fn python_path_env(python_paths: &[PathBuf]) -> Option<String> {
    let mut ordered: Vec<PathBuf> = python_paths.to_vec();
    if let Some(existing) = std::env::var_os("PYTHONPATH") {
        ordered.extend(std::env::split_paths(&existing));
    }
    if ordered.is_empty() {
        return None;
    }
    std::env::join_paths(ordered)
        .ok()
        .map(|joined| joined.to_string_lossy().into_owned())
}

/// provider 构造失败：用法错误（fake 脚本非法）→ 2；环境错误（真实 provider）→ 4。
enum ProviderError {
    Usage(String),
    Environment(String),
}

/// 构造 LLM provider 与 `fake_mode`（05 §5.2.4 步 9）。
fn build_provider(args: &RunArgs) -> Result<(Arc<dyn LlmProvider>, bool), ProviderError> {
    match &args.fake_llm {
        FakeLlmMode::DefaultScript => {
            let script: Value = serde_json::from_str(DEFAULT_FAKE_LLM_SCRIPT).map_err(|error| {
                ProviderError::Usage(format!("built-in fake-llm script is broken: {error}"))
            })?;
            let provider = FakeLlmProvider::from_script(script).map_err(|error| {
                ProviderError::Usage(format!("built-in fake-llm script is invalid: {error}"))
            })?;
            Ok((Arc::new(provider), true))
        }
        FakeLlmMode::Script(path) => {
            let provider = FakeLlmProvider::from_path(path).map_err(|error| {
                ProviderError::Usage(format!("invalid --fake-llm script: {error}"))
            })?;
            Ok((Arc::new(provider), true))
        }
        FakeLlmMode::Off => {
            let provider = GenaiLlmProvider::new().map_err(ProviderError::Environment)?;
            Ok((Arc::new(provider), args.real))
        }
    }
}

/// `--record` 字符串（trace）→ 内核 `RunOptions.record_mode`。
fn kernel_record_mode(mode: RecordMode) -> zgraph_kernel::RecordMode {
    match mode {
        RecordMode::None => zgraph_kernel::RecordMode::None,
        RecordMode::Hash => zgraph_kernel::RecordMode::Hash,
        RecordMode::Full => zgraph_kernel::RecordMode::Full,
    }
}

/// run 选项。
///
/// 用 `..Default::default()` 而不是穷举字段：`test-hooks` 构建会多一个 `test_hooks`
/// 字段（kernel `RunOptions` 文档约定）。
fn run_options(mode: RecordMode) -> RunOptions {
    RunOptions {
        record_mode: kernel_record_mode(mode),
        ..RunOptions::default()
    }
}

/// `--stream` 的观察者：只把 token/reasoning 文本实时写到 stderr。
fn stream_observer() -> StreamingObserver {
    Arc::new(|event: &StreamingEvent| match event {
        StreamingEvent::TokenDelta { text } | StreamingEvent::ReasoningDelta { text } => {
            eprint!("{text}");
        }
        _ => {}
    })
}

// ---------------------------------------------------------------------------
// 子命令入口
// ---------------------------------------------------------------------------

/// 执行 `run` 并返回进程退出码。
pub fn run(args: &RunArgs) -> ExitCode {
    match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime.block_on(run_in_runtime(args)),
        Err(error) => {
            eprintln!("error: cannot start tokio runtime: {error}");
            ExitCode::from(EXIT_OTHER)
        }
    }
}

/// 运行时内的主流程（启动顺序见模块文档）。
async fn run_in_runtime(args: &RunArgs) -> ExitCode {
    let db_path = resolve_db_path(args.db.as_deref());
    let memory_db = is_memory_db(&db_path);
    if memory_db {
        warn_memory_db();
    }
    let writer = match TraceWriter::open(&db_path) {
        Ok(writer) => writer,
        Err(error) => return report_storage_error(&error),
    };
    // 启动中止标定（06 §6.8.1：打开/迁移 DB 后、编译/启动 run 前）。
    let recovered = match recovery::recover_interrupted(&writer) {
        Ok(recovered) => recovered,
        Err(error) => {
            close_quietly(writer);
            return report_storage_error(&error);
        }
    };
    for item in &recovered {
        eprintln!("{}", recovery::warning_line(item));
    }
    let assets_dir = resolve_assets_dir(args);
    let tools_dir = resolve_tools_dir(args, &assets_dir);
    let graph = match load_compiled_graph(args, &assets_dir) {
        Ok(graph) => graph,
        Err(code) => {
            close_quietly(writer);
            return code;
        }
    };
    let entry_inputs = match resolve_inputs(args, &graph) {
        Ok(inputs) => inputs,
        Err(code) => {
            close_quietly(writer);
            return code;
        }
    };
    let (provider, fake_mode) = match build_provider(args) {
        Ok(built) => built,
        Err(error) => {
            close_quietly(writer);
            return report_provider_error(error);
        }
    };
    let trace_handle = writer.handle();
    // run id 由 CLI 预生成：`llm_calls` 记录器在启动前就需要 run_id（快照不含 run）。
    let run_id = uuid::Uuid::now_v7().to_string();
    let recorder = llm_call_recorder(args, &trace_handle, &run_id);
    let executor = build_executor(
        args,
        &assets_dir,
        &tools_dir,
        provider,
        fake_mode,
        &trace_handle,
        recorder,
    );
    // human 模式（非 --json、非 -q）订阅 bridge 内存事件旁路，实时输出生命周期行
    // （ISSUE-085 / 06 §6.10.3）；`--json` 的 stdout 契约与 `-q` 抑制行为不变。
    let (mut lifecycle, lifecycle_observer) = if args.json || args.quiet {
        (None, None)
    } else {
        let (printer, observer) = LifecyclePrinter::new();
        (Some(printer), Some(observer))
    };
    let observers = BridgeObservers {
        streaming: args.stream.then(stream_observer),
        lifecycle: lifecycle_observer,
    };
    let spec = RunSpec {
        graph: Arc::clone(&graph),
        entry_inputs,
        user_id: args.user_id.clone(),
        global_vars: HashMap::new(),
        options: run_options(args.record),
    };
    let traced = match bridge::start_traced_run_with_writer_and_run_id(
        writer,
        spec,
        executor,
        observers,
        run_id.clone(),
    ) {
        Ok(traced) => traced,
        Err(error) => return report_bridge_error(&error),
    };
    debug_assert_eq!(traced.run_id(), run_id);
    if !args.json && !args.quiet {
        eprintln!(
            "run {run_id} created (graph={} hash={})",
            graph.metadata.name, graph.graph_hash
        );
        // 启动期产生的事件（RunStarted 等）已排队；`created` 之后再起打印线程，
        // 行顺序与 06 §6.10.3 示例一致。
        if let Some(printer) = lifecycle.as_mut() {
            printer.start_printing();
        }
    }
    let started = Instant::now();
    let outcome = wait_for_run(traced, memory_db).await;
    let duration_ms = duration_millis(started);
    // 先收尾实时行，再输出 run 结果/摘要（stderr 行序稳定、且结果行必然最后出现）。
    if let Some(printer) = lifecycle.take() {
        printer.finish();
    }
    let outcome = match outcome {
        Ok(outcome) => outcome,
        Err(code) => return code,
    };
    finish(args, outcome, duration_ms, &db_path, &recovered)
}

/// 加载并编译图；失败时打印 stderr 并返回退出码 2（不产生 run）。
fn load_compiled_graph(args: &RunArgs, assets_dir: &Path) -> Result<Arc<CompiledGraph>, ExitCode> {
    let display = args.graph.display().to_string();
    let bytes = match std::fs::read(&args.graph) {
        Ok(bytes) => bytes,
        Err(error) => {
            eprintln!("error: cannot read graph '{display}': {error}");
            return Err(ExitCode::from(EXIT_USAGE));
        }
    };
    let options = LoadOptions {
        assets_dir: Some(assets_dir.to_path_buf()),
    };
    let result = load_graph_with_options(&bytes, &options);
    let loaded: LoadedGraph = match result {
        Ok(loaded) => loaded,
        Err(_) => {
            let report = ValidationReport::from_result(&result);
            crate::validate::print_errors(&display, &report);
            return Err(ExitCode::from(EXIT_USAGE));
        }
    };
    CompiledGraph::compile(&loaded).map_err(|error| {
        eprintln!("error: graph compile failed: {error}");
        ExitCode::from(EXIT_USAGE)
    })
}

/// 读取并校验入口输入；失败时打印 `error[RUN_ENTRY_*]` 并返回退出码 3。
fn resolve_inputs(args: &RunArgs, graph: &CompiledGraph) -> Result<EntryInputs, ExitCode> {
    let raw = read_inputs_value(&args.inputs).map_err(|error| entry_input_exit(&error))?;
    resolve_entry_inputs(graph, &raw).map_err(|error| entry_input_exit(&error))
}

/// 入口输入错误 → stderr + 退出码 3。
fn entry_input_exit(error: &EntryInputError) -> ExitCode {
    eprintln!("error[{}]: {}", error.code, error.message);
    ExitCode::from(EXIT_ENTRY_INPUT)
}

/// `--record=full` 时挂接 `llm_calls` 记录器（DR-002 §3.2：hash/none 不写）。
fn llm_call_recorder(
    args: &RunArgs,
    trace_handle: &TraceHandle,
    run_id: &str,
) -> Option<Arc<dyn LlmCallRecorder>> {
    if args.record != RecordMode::Full {
        return None;
    }
    Some(Arc::new(LlmCallTraceRecorder {
        handle: trace_handle.clone(),
        run_id: run_id.to_string(),
    }))
}

/// 组合执行器：Recording → Routing（Harness / Worker）。
fn build_executor(
    args: &RunArgs,
    assets_dir: &Path,
    tools_dir: &Path,
    provider: Arc<dyn LlmProvider>,
    fake_mode: bool,
    trace_handle: &TraceHandle,
    recorder: Option<Arc<dyn LlmCallRecorder>>,
) -> Arc<dyn NodeExecutor> {
    let config = worker_config(args);
    // ISSUE-079（DR-003 D7）：工具调用与节点执行共享**同一**并发上限实例
    // （全局 CPU×2 / 每 run 8），避免两者叠加超过 D4 上限。
    let limits = Arc::new(WorkerLimits::default_limits());
    let worker = Arc::new(WorkerExecutor::with_shared_limits(
        SupervisorConfig::from_worker(config.clone()),
        limits.clone(),
    ));
    let settings = HarnessSettings {
        assets_dir: assets_dir.to_path_buf(),
        tools_dir: tools_dir.to_path_buf(),
        provider,
        fake_mode,
        tool_runner: Arc::new(WorkerToolRunner::new(config)),
    };
    let harness = match recorder {
        Some(recorder) => Arc::new(HarnessExecutor::with_llm_recorder_and_tool_limits(
            settings, recorder, limits,
        )),
        None => Arc::new(HarnessExecutor::with_tool_limits(settings, limits)),
    };
    let routing: Arc<dyn NodeExecutor> = Arc::new(RoutingExecutor { harness, worker });
    Arc::new(RecordingExecutor::new(
        routing,
        trace_handle.clone(),
        args.record,
    ))
}

/// run 结束产物：常规库带只读 reader；`:memory:` 只有内核 outcome（06 §6.9.1）。
enum RunCompletion {
    /// 落盘库：可回读 events/instances（生命周期行与 event_count 来源）。
    WithReader(RunOutcome, TraceReader),
    /// 进程内库：reader 不可用，输出只用 outcome（event_count 记 0）。
    OutcomeOnly(RunOutcome),
}

/// 等待 run 结束（SIGINT → cancel → 退出码 130）。
async fn wait_for_run(traced: TracedRun, memory_db: bool) -> Result<RunCompletion, ExitCode> {
    let interrupted = Arc::new(AtomicBool::new(false));
    let signal_task = tokio::spawn(watch_interrupt(Arc::clone(&interrupted)));
    while !traced.status().is_terminal() {
        if interrupted.load(Ordering::SeqCst) {
            traced.cancel();
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    let completion = if memory_db {
        traced
            .wait_without_reader()
            .await
            .map(RunCompletion::OutcomeOnly)
    } else {
        traced.wait().await.map(|outcome| {
            let (outcome, reader) = outcome.into_parts();
            RunCompletion::WithReader(outcome, reader)
        })
    };
    signal_task.abort();
    completion.map_err(|error| report_bridge_error(&error))
}

/// 监听 SIGINT：只置位标志，取消在 run 循环里执行（避免早到信号丢语义）。
async fn watch_interrupt(interrupted: Arc<AtomicBool>) {
    if tokio::signal::ctrl_c().await.is_ok() {
        interrupted.store(true, Ordering::SeqCst);
    }
}

/// 从 `Instant` 取毫秒时长（饱和转换）。
fn duration_millis(started: Instant) -> u64 {
    u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX)
}

// ---------------------------------------------------------------------------
// 结束输出（06 §6.10.3 / §6.10.6）
// ---------------------------------------------------------------------------

/// 输出 run 结果并返回退出码。
///
/// `:memory:` 没有可回读的 reader：`event_count` 记 0（trace 跨进程不可读，
/// 06 §6.9.1），其余字段全部来自内核 outcome。
fn finish(
    args: &RunArgs,
    completion: RunCompletion,
    duration_ms: u64,
    db_path: &Path,
    recovered: &[recovery::Recovered],
) -> ExitCode {
    let (outcome, reader) = match completion {
        RunCompletion::WithReader(outcome, reader) => (outcome, Some(reader)),
        RunCompletion::OutcomeOnly(outcome) => (outcome, None),
    };
    let run_id = outcome.run_id.clone();
    let event_count = reader
        .as_ref()
        .and_then(|reader| reader.run(&run_id).ok().flatten())
        .map_or(0, |row| row.event_count);
    let exit_code = exit_code_for(&outcome);
    if args.json {
        println!(
            "{}",
            json_payload(&outcome, event_count, duration_ms, db_path, recovered)
        );
    } else {
        // 生命周期行已由实时旁路在 run 期间写到 stderr（ISSUE-085）；这里只输出结果。
        println!("{}", run_output_text(&outcome));
        if !args.quiet {
            eprintln!(
                "{}",
                summary_line(&outcome, duration_ms, event_count, exit_code)
            );
        }
        if let Some(error) = &outcome.error {
            eprintln!("error[{}]: {}", error.code, error.message);
        }
    }
    ExitCode::from(exit_code)
}

/// run 终态 → 退出码（06 §6.10.5）。
///
/// CLI 只通过 SIGINT 取消 run，因此 `Cancelled` 一律取 130（程序化取消属库 API，
/// 不在 CLI 范围内）。**存储 fail-stop**（内核内存态携带 `STORAGE_ERROR`）取 4
/// （06 §6.4.5 / §6.10.5「存储故障取消 → Failed（若可写）+ 4」）。
fn exit_code_for(outcome: &RunOutcome) -> u8 {
    if outcome.status == RunStatus::Failed
        && outcome
            .error
            .as_ref()
            .is_some_and(|error| error.code == zgraph_kernel::CODE_STORAGE_ERROR)
    {
        return EXIT_STORAGE;
    }
    match outcome.status {
        RunStatus::Completed => EXIT_SUCCESS,
        RunStatus::Failed => EXIT_OTHER,
        RunStatus::Cancelled => EXIT_INTERRUPTED,
        RunStatus::Running | RunStatus::Paused => EXIT_OTHER,
    }
}

/// `--json` 结构（06 §6.10.6）。
fn json_payload(
    outcome: &RunOutcome,
    event_count: i64,
    duration_ms: u64,
    db_path: &Path,
    recovered: &[recovery::Recovered],
) -> Value {
    let mut object = Map::new();
    object.insert(
        "ok".to_string(),
        json!(outcome.status == RunStatus::Completed),
    );
    object.insert("command".to_string(), json!("run"));
    object.insert("run_id".to_string(), json!(outcome.run_id));
    object.insert("status".to_string(), json!(outcome.status.as_str()));
    object.insert("duration_ms".to_string(), json!(duration_ms));
    object.insert("event_count".to_string(), json!(event_count));
    object.insert(
        "streaming_dropped".to_string(),
        json!(outcome.metrics.streaming_dropped),
    );
    object.insert(
        "abandoned_tasks".to_string(),
        json!(outcome.metrics.abandoned_tasks),
    );
    object.insert(
        "trace_backpressure".to_string(),
        json!(outcome.metrics.trace_backpressure),
    );
    object.insert(
        "killed_workers".to_string(),
        json!(outcome.metrics.killed_workers),
    );
    object.insert("db".to_string(), json!(db_path.display().to_string()));
    object.insert(
        "run_output".to_string(),
        outcome.output.clone().unwrap_or(Value::Null),
    );
    object.insert(
        "recovered".to_string(),
        Value::Array(recovered.iter().map(recovered_json).collect()),
    );
    object.insert("warnings".to_string(), json!([]));
    if let Some(error) = &outcome.error {
        object.insert("error".to_string(), run_error_json(error));
    }
    Value::Object(object)
}

/// 中止标定结果 → `--json.recovered[]` 元素（06 §6.8.1 / DR-002 §9 #10）。
fn recovered_json(item: &recovery::Recovered) -> Value {
    json!({
        "run_id": item.run.run_id,
        "previous_status": item.run.previous_status.as_str(),
        "recovered_at": item.run.recovered_at,
    })
}

/// `RunError` → `{"code","message"}`。
fn run_error_json(error: &RunError) -> Value {
    json!({"code": error.code, "message": error.message})
}

/// human 模式 stdout 的 `run_output` JSON（失败/取消为 `null`）。
fn run_output_text(outcome: &RunOutcome) -> String {
    serde_json::to_string(&outcome.output).unwrap_or_else(|_| "null".to_string())
}

/// 结束摘要行（06 §6.10.3 末行）。
fn summary_line(outcome: &RunOutcome, duration_ms: u64, event_count: i64, exit_code: u8) -> String {
    let prefix = if exit_code == EXIT_SUCCESS {
        "ok"
    } else {
        "error"
    };
    format!(
        "{prefix} run={} status={} duration={duration_ms}ms events={event_count} \
         dropped={} abandoned={} killed={}",
        outcome.run_id,
        outcome.status.as_str(),
        outcome.metrics.streaming_dropped,
        outcome.metrics.abandoned_tasks,
        outcome.metrics.killed_workers,
    )
}

// ---------------------------------------------------------------------------
// 实时生命周期输出（ISSUE-085 / 06 §6.10.3）
// ---------------------------------------------------------------------------

/// human 模式实时生命周期输出器（ISSUE-085）：bridge 内存事件旁路 → mpsc → 打印线程。
///
/// 生命周期行原本在 run 结束、写端 close 后从 trace 回读（非实时）；现在改为组合根
/// 订阅 bridge 旁路，运行中逐行输出。时序约定：
///
/// 1. [`LifecyclePrinter::new`] 在 run **启动前**建通道（`RunStarted` 可能在
///    `start_run` 内同步产生）；
/// 2. 启动成功、打印 `run ... created` 行之后调用
///    [`LifecyclePrinter::start_printing`]——启动期事件先排队，行顺序仍是
///    「created → 生命周期事件」；
/// 3. run 结束调用 [`LifecyclePrinter::finish`]：停止哨兵 + join，保证结果行与摘要行
///    之前生命周期行已全部写到 stderr。
///
/// `[seq N]` 是 **CLI 侧**生命周期序号（1 起，只对产生 `events` 行的事件递增）；
/// 单 run 新库时与 trace `events.seq` 一致，含 `Streaming` 增量的 run 因不逐块成行
/// 而会小于 trace 的聚合段编号（`--stream` 负责原始文本）。
struct LifecyclePrinter {
    sender: mpsc::Sender<Option<String>>,
    receiver: Option<mpsc::Receiver<Option<String>>>,
    thread: Option<JoinHandle<()>>,
}

impl LifecyclePrinter {
    /// 建通道并返回（输出器, 旁路观察者）；观察者只格式化 + 入队，不阻塞内核。
    fn new() -> (Self, LifecycleObserver) {
        let (sender, receiver) = mpsc::channel::<Option<String>>();
        let observer_sender = sender.clone();
        let seq = Arc::new(AtomicU64::new(1));
        let observer: LifecycleObserver = Arc::new(move |input| {
            let Some(detail) = lifecycle_detail(input) else {
                return;
            };
            let seq = seq.fetch_add(1, Ordering::Relaxed);
            // 打印线程未启动/已结束（或 run 启动失败）时丢弃行，不影响 run 语义。
            let _ = observer_sender.send(Some(format!("[seq {seq}] {detail}")));
        });
        (
            Self {
                sender,
                receiver: Some(receiver),
                thread: None,
            },
            observer,
        )
    }

    /// `created` 行之后启动打印线程：先补打已入队的启动期事件，再实时跟随。
    fn start_printing(&mut self) {
        let Some(receiver) = self.receiver.take() else {
            return;
        };
        self.thread = Some(std::thread::spawn(move || {
            // `None` 是结束哨兵；通道断开同样退出。
            let mut stderr = std::io::stderr();
            while let Ok(Some(line)) = receiver.recv() {
                // 写失败（例如 stderr 管道被对端关闭）时静默结束，不影响 run 结果。
                if writeln!(stderr, "{line}").is_err() {
                    break;
                }
            }
        }));
    }

    /// 收尾：发送停止哨兵并 join 打印线程（保证结果/摘要行之前生命周期行已写出）。
    fn finish(mut self) {
        let _ = self.sender.send(None);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// 旁路事件 → 生命周期行正文（不含 `[seq N]`）；不产生 `events` 行的输入返回 `None`。
///
/// 覆盖 06 §6.10.3 示例的控制面事件。`Streaming` 的 token/reasoning **增量**不逐块
/// 成行（原始文本由 `--stream` 实时写 stderr；trace 侧聚合段依赖写线程 100ms/
/// 4096 字符聚合，旁路无法在段边界前还原），其余 Streaming 变体（progress/
/// tool_call_start/tool_call_end）照常实时成行。
fn lifecycle_detail(input: &TraceInput) -> Option<String> {
    match input {
        TraceInput::RunStarted(info) => {
            Some(format!("RunStarted status=Running user={}", info.user_id))
        }
        TraceInput::Transition { record, .. } => Some(format!(
            "Transition {} {} -> {} ({})",
            record.node_id,
            record.from.as_str(),
            record.to.as_str(),
            record.reason.as_deref().unwrap_or("-")
        )),
        TraceInput::GateResult(record) => Some(format!(
            "GateResult {} edge#{} gate={} passed={}",
            record.node_id,
            record.edge_index,
            record.gate_fn.as_deref().unwrap_or("-"),
            record.passed
        )),
        TraceInput::NodeTimedOut { elapsed_ms, .. } => {
            Some(format!("NodeTimedOut - elapsed_ms={elapsed_ms}"))
        }
        TraceInput::NodeAbandoned { .. } => Some("NodeAbandoned -".to_string()),
        TraceInput::Streaming { node_id, event, .. } => {
            streaming_lifecycle_detail(node_id.as_deref().unwrap_or("-"), event)
        }
        TraceInput::RunFinished(info) => {
            Some(format!("RunFinished status={}", info.status.as_str()))
        }
        // 快照 / L2 记录 / llm_calls 都不是 `events` 行，不属于生命周期输出。
        TraceInput::InstanceSnapshot(_)
        | TraceInput::RecordedOutput(_)
        | TraceInput::LlmCall(_) => None,
    }
}

/// `Streaming` 旁路 → 生命周期行（token/reasoning 增量不逐块打印，见 [`lifecycle_detail`]）。
fn streaming_lifecycle_detail(node: &str, event: &zgraph_trace::StreamingEvent) -> Option<String> {
    match event {
        zgraph_trace::StreamingEvent::TokenDelta { .. }
        | zgraph_trace::StreamingEvent::ReasoningDelta { .. } => None,
        zgraph_trace::StreamingEvent::ToolCallStart { tool_name, .. } => {
            Some(format!("Streaming {node} tool_call_start {tool_name}"))
        }
        zgraph_trace::StreamingEvent::ToolCallEnd { tool_name, ok, .. } => Some(format!(
            "Streaming {node} tool_call_end {tool_name} ok={ok}"
        )),
        zgraph_trace::StreamingEvent::Progress { percent, message } => {
            Some(format!("Streaming {node} progress {percent}% ({message})"))
        }
    }
}

// ---------------------------------------------------------------------------
// 错误映射
// ---------------------------------------------------------------------------

/// provider 构造错误 → stderr + 退出码（fake 脚本 2；真实 provider 环境 4）。
fn report_provider_error(error: ProviderError) -> ExitCode {
    match error {
        ProviderError::Usage(message) => {
            eprintln!("error: {message}");
            ExitCode::from(EXIT_USAGE)
        }
        ProviderError::Environment(message) => {
            eprintln!("error: cannot create real LLM provider: {message}");
            ExitCode::from(EXIT_STORAGE)
        }
    }
}

/// `BridgeError` → 退出码：启动校验（入口）3；存储/写端 4。
fn report_bridge_error(error: &BridgeError) -> ExitCode {
    match error {
        BridgeError::Start(start) => {
            eprintln!("error[{}]: {}", start.code, start.message);
            ExitCode::from(EXIT_ENTRY_INPUT)
        }
        BridgeError::Trace(error) => report_storage_error(error),
        BridgeError::Writer(message) => {
            eprintln!("error[STORAGE_ERROR]: {message}");
            ExitCode::from(EXIT_STORAGE)
        }
    }
}

/// `TraceError` → `error[<CLI 码>]` + 退出码 4。
fn report_storage_error(error: &TraceError) -> ExitCode {
    let detail = match error {
        TraceError::SchemaTooNew { found, supported } => {
            format!("db version={found}, supported={supported}")
        }
        TraceError::Storage(message)
        | TraceError::NotFound(message)
        | TraceError::Serialize(message) => message.clone(),
    };
    eprintln!("error[{}]: {detail}", error.code());
    ExitCode::from(EXIT_STORAGE)
}

/// 关闭写端（失败路径上尽力 flush；错误已由主错误码表达）。
fn close_quietly(writer: TraceWriter) {
    let _ = writer.close();
}

#[cfg(test)]
mod tests {
    use super::{
        exit_code_for, is_memory_db, llm_call_recorder, output_hash, parse, resolve_entry_inputs,
        FakeLlmMode, RecordingExecutor, RunArgs, DEFAULT_USER_ID,
    };
    use serde_json::json;
    use std::future::Future;
    use std::path::{Path, PathBuf};
    use std::pin::Pin;
    use std::sync::Arc;
    use zgraph_harness::{
        FakeLlmProvider, FakeToolRunner, HarnessExecutor, HarnessSettings, LlmProvider,
    };
    use zgraph_kernel::{
        CompiledGraph, CompiledNode, NodeError, NodeExecution, NodeExecutor, RunError, RunMetrics,
        RunOutcome, RunStatus, CODE_STORAGE_ERROR,
    };
    use zgraph_trace::{RecordMode, TraceWriter};
    use zgraph_worker::{SupervisorConfig, WorkerConfig, WorkerExecutor};

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    /// `RunOutcome` 构造器（退出码映射测试用；metric 全 0）。
    fn outcome(status: RunStatus, error: Option<RunError>) -> RunOutcome {
        RunOutcome {
            run_id: "run-1".to_string(),
            status,
            output: None,
            error,
            metrics: RunMetrics {
                streaming_dropped: 0,
                abandoned_tasks: 0,
                trace_backpressure: 0,
                killed_workers: 0,
            },
            started_at: "2026-10-30T00:00:00.000Z".to_string(),
            finished_at: "2026-10-30T00:00:01.000Z".to_string(),
        }
    }

    fn graph_text() -> String {
        json!({
            "version": "1.0",
            "metadata": {"name": "io"},
            "entry_points": ["start"],
            "nodes": [
                {"id": "start", "type": "start",
                 "inputs": [{"name": "x", "type": "any", "required": true}], "config": {}},
                {"id": "end", "type": "end",
                 "inputs": [{"name": "r", "type": "any", "required": true}], "config": {}}
            ],
            "edges": [{"from": "start", "to_node": "end", "to_input_port": "r"}]
        })
        .to_string()
    }

    fn compiled_graph(text: &str) -> std::sync::Arc<CompiledGraph> {
        let loaded = zgraph_dsl::load_graph(text.as_bytes()).expect("valid dsl");
        CompiledGraph::compile(&loaded).expect("compiled")
    }

    #[test]
    fn parse_applies_defaults() {
        let parsed = parse(&args(&["g.json", "--inputs", "{}"]))
            .expect("parse")
            .expect("not help");
        assert_eq!(parsed.graph, PathBuf::from("g.json"));
        assert_eq!(parsed.inputs, "{}");
        assert_eq!(parsed.user_id, DEFAULT_USER_ID);
        assert_eq!(parsed.db, None);
        assert_eq!(parsed.assets_dir, None);
        assert_eq!(parsed.tools_dir, None);
        assert_eq!(parsed.python, None);
        assert!(parsed.python_paths.is_empty());
        assert_eq!(parsed.fake_llm, FakeLlmMode::Off);
        assert!(!parsed.real);
        assert_eq!(parsed.record, RecordMode::Hash);
        assert!(!parsed.stream);
        assert!(!parsed.json);
        assert!(!parsed.quiet);
    }

    #[test]
    fn parse_accepts_all_options() {
        let parsed = parse(&args(&[
            "g.json",
            "--inputs",
            "@/tmp/in.json",
            "--user-id",
            "u1",
            "--db",
            "/tmp/t.db",
            "--assets-dir",
            "assets",
            "--tools-dir",
            "tools",
            "--python",
            "/venv/bin/python",
            "--python-path",
            "a",
            "--python-path",
            "b",
            "--fake-llm=/tmp/script.json",
            "--record=full",
            "--stream",
            "--json",
            "-q",
        ]))
        .expect("parse")
        .expect("not help");
        assert_eq!(parsed.inputs, "@/tmp/in.json");
        assert_eq!(parsed.user_id, "u1");
        assert_eq!(parsed.db, Some(PathBuf::from("/tmp/t.db")));
        assert_eq!(parsed.assets_dir, Some(PathBuf::from("assets")));
        assert_eq!(parsed.tools_dir, Some(PathBuf::from("tools")));
        assert_eq!(parsed.python.as_deref(), Some("/venv/bin/python"));
        assert_eq!(
            parsed.python_paths,
            vec![PathBuf::from("a"), PathBuf::from("b")]
        );
        assert_eq!(
            parsed.fake_llm,
            FakeLlmMode::Script(PathBuf::from("/tmp/script.json"))
        );
        assert_eq!(parsed.record, RecordMode::Full);
        assert!(parsed.stream && parsed.json && parsed.quiet);
    }

    #[test]
    fn parse_bare_fake_llm_and_record_space_form() {
        let parsed = parse(&args(&[
            "g.json",
            "--inputs",
            "{}",
            "--fake-llm",
            "--record",
            "none",
        ]))
        .expect("parse")
        .expect("not help");
        assert_eq!(parsed.fake_llm, FakeLlmMode::DefaultScript);
        assert_eq!(parsed.record, RecordMode::None);
    }

    #[test]
    fn parse_rejects_usage_errors() {
        assert!(parse(&args(&[])).is_err());
        assert!(parse(&args(&["g.json"])).is_err(), "missing --inputs");
        assert!(parse(&args(&["--inputs", "{}"])).is_err(), "missing graph");
        assert!(parse(&args(&["g.json", "--inputs", "{}", "--real", "--fake-llm"])).is_err());
        assert!(parse(&args(&["g.json", "--inputs", "{}", "--record", "all"])).is_err());
        assert!(parse(&args(&["g.json", "--inputs", "{}", "--nope"])).is_err());
        assert!(parse(&args(&["g.json", "--inputs"])).is_err());
    }

    #[test]
    fn parse_help_returns_none() {
        assert!(parse(&args(&["--help"])).expect("parse").is_none());
        assert!(parse(&args(&["g.json", "-h"])).expect("parse").is_none());
    }

    #[test]
    fn inputs_accept_nested_object() {
        let graph = compiled_graph(&graph_text());
        let inputs = resolve_entry_inputs(&graph, &json!({"start": {"x": 1}})).expect("valid");
        assert_eq!(inputs["start"]["x"], json!(1));
    }

    #[test]
    fn inputs_accept_single_entry_shorthand() {
        let graph = compiled_graph(&graph_text());
        let inputs = resolve_entry_inputs(&graph, &json!({"x": 2})).expect("valid");
        assert_eq!(inputs["start"]["x"], json!(2));
    }

    #[test]
    fn inputs_reject_non_entry_undeclared_and_missing() {
        let graph = compiled_graph(&graph_text());
        let non_entry = resolve_entry_inputs(&graph, &json!({"end": {"r": 1}})).expect_err("x");
        assert_eq!(non_entry.code, "RUN_ENTRY_INVALID");
        let undeclared =
            resolve_entry_inputs(&graph, &json!({"start": {"nope": 1}})).expect_err("x");
        assert_eq!(undeclared.code, "RUN_ENTRY_INVALID");
        let missing = resolve_entry_inputs(&graph, &json!({"start": {}})).expect_err("x");
        assert_eq!(missing.code, "RUN_ENTRY_INPUT_MISSING");
        let empty = resolve_entry_inputs(&graph, &json!({})).expect_err("x");
        assert_eq!(empty.code, "RUN_ENTRY_INPUT_MISSING");
        let array = resolve_entry_inputs(&graph, &json!([])).expect_err("x");
        assert_eq!(array.code, "RUN_ENTRY_INVALID");
    }

    #[test]
    fn output_hash_is_stable_full_width_blake3() {
        let output = json!({"content": "ok", "iterations": 1});
        let first = output_hash(&output);
        let second = output_hash(&json!({"iterations": 1, "content": "ok"}));
        assert_eq!(
            first, second,
            "canonical JSON makes the hash key-order stable"
        );
        assert_eq!(first.len(), 64, "blake3 hash is 64 lowercase hex chars");
        assert!(first.chars().all(|ch| ch.is_ascii_hexdigit()));
        assert_eq!(
            first,
            blake3::hash(zgraph_dsl::canonical_json(&output).as_bytes())
                .to_hex()
                .to_string()
        );
    }

    #[test]
    fn run_args_is_equatable() {
        let args = RunArgs {
            graph: PathBuf::from("g.json"),
            inputs: "{}".to_string(),
            user_id: DEFAULT_USER_ID.to_string(),
            db: None,
            assets_dir: None,
            tools_dir: None,
            python: None,
            python_paths: Vec::new(),
            fake_llm: FakeLlmMode::Off,
            real: false,
            record: RecordMode::Hash,
            stream: false,
            json: false,
            quiet: false,
        };
        assert_eq!(args.clone(), args);
    }

    #[test]
    fn memory_db_path_is_recognised() {
        assert!(is_memory_db(Path::new(":memory:")));
        assert!(!is_memory_db(Path::new("./:memory:")));
        assert!(!is_memory_db(Path::new("trace.db")));
        // CLI 解析层把 :memory: 当作普通路径值接受（不做非法路径拒绝）。
        let parsed = parse(&args(&["g.json", "--inputs", "{}", "--db", ":memory:"]))
            .expect("parse")
            .expect("not help");
        assert_eq!(parsed.db, Some(PathBuf::from(":memory:")));
    }

    /// 只用于 `timeout_hint` 转发的内层执行器（不真正执行）。
    struct HintExecutor {
        hint: Option<u64>,
    }

    impl NodeExecutor for HintExecutor {
        fn execute<'a>(
            &'a self,
            _execution: NodeExecution<'a>,
        ) -> Pin<Box<dyn Future<Output = Result<serde_json::Value, NodeError>> + Send + 'a>>
        {
            Box::pin(async {
                Err(NodeError::Internal {
                    message: "unused".to_string(),
                })
            })
        }

        fn timeout_hint(&self, _node: &CompiledNode) -> Option<u64> {
            self.hint
        }
    }

    fn temp_writer() -> (tempfile::TempDir, TraceWriter) {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("trace.db");
        let writer = TraceWriter::open(&path).expect("writer");
        (dir, writer)
    }

    #[test]
    fn recording_executor_forwards_timeout_hint_to_inner() {
        let (dir, writer) = temp_writer();
        let handle = writer.handle();
        let graph = compiled_graph(&graph_text());
        let node = graph.nodes.get("start").expect("start node");
        let recording = RecordingExecutor::new(
            Arc::new(HintExecutor { hint: Some(42) }),
            handle.clone(),
            RecordMode::None,
        );
        assert_eq!(
            recording.timeout_hint(node),
            Some(42),
            "ISSUE-076: wrapper must not truncate the inner timeout hint"
        );
        let no_hint = RecordingExecutor::new(
            Arc::new(HintExecutor { hint: None }),
            handle,
            RecordMode::None,
        );
        assert_eq!(no_hint.timeout_hint(node), None);
        writer.close().expect("close writer");
        drop(dir);
    }

    #[test]
    fn llm_call_recorder_is_attached_only_in_full_mode() {
        let (dir, writer) = temp_writer();
        let handle = writer.handle();
        let full = parse(&args(&["g.json", "--inputs", "{}", "--record", "full"]))
            .expect("parse")
            .expect("not help");
        assert!(llm_call_recorder(&full, &handle, "run-1").is_some());
        for mode in ["hash", "none"] {
            let parsed = parse(&args(&["g.json", "--inputs", "{}", "--record", mode]))
                .expect("parse")
                .expect("not help");
            assert!(
                llm_call_recorder(&parsed, &handle, "run-1").is_none(),
                "llm_calls must only be written with --record=full (mode={mode})"
            );
        }
        writer.close().expect("close writer");
        drop(dir);
    }

    /// `start -> agent_call(agent_id=a) -> end`（无 DSL timeout_ms）。
    fn agent_graph_text() -> String {
        json!({
            "version": "1.0",
            "metadata": {"name": "agent"},
            "entry_points": ["start"],
            "nodes": [
                {"id": "start", "type": "start",
                 "inputs": [{"name": "message", "type": "any", "required": true}], "config": {}},
                {"id": "work", "type": "agent_call",
                 "inputs": [{"name": "message", "type": "any", "required": true}],
                 "config": {"agent_id": "a"}},
                {"id": "end", "type": "end",
                 "inputs": [{"name": "result", "type": "any", "required": true}], "config": {}}
            ],
            "edges": [
                {"from": "start", "to_node": "work", "to_input_port": "message"},
                {"from": "work", "to_node": "end", "to_input_port": "result"}
            ]
        })
        .to_string()
    }

    /// `start -> python_node -> end` 图（用于断言 worker 分支无 hint）。
    fn python_graph_text() -> String {
        json!({
            "version": "1.0",
            "metadata": {"name": "py"},
            "entry_points": ["start"],
            "nodes": [
                {"id": "start", "type": "start",
                 "inputs": [{"name": "x", "type": "any", "required": true}], "config": {}},
                {"id": "work", "type": "python_node",
                 "inputs": [{"name": "x", "type": "any", "required": true}],
                 "config": {"module": "workers.echo", "function": "run"}},
                {"id": "end", "type": "end",
                 "inputs": [{"name": "r", "type": "any", "required": true}], "config": {}}
            ],
            "edges": [
                {"from": "start", "to_node": "work", "to_input_port": "x"},
                {"from": "work", "to_node": "end", "to_input_port": "r"}
            ]
        })
        .to_string()
    }

    #[test]
    fn routing_executor_forwards_agent_timeout_hint_to_harness() {
        let dir = tempfile::tempdir().expect("tempdir");
        let assets_dir = dir.path().join("assets");
        std::fs::create_dir_all(assets_dir.join("agents")).expect("agents dir");
        std::fs::write(
            assets_dir.join("agents/agent_a.json"),
            r#"{"id":"a","node_timeout_ms":1234}"#,
        )
        .expect("agent asset");
        std::fs::create_dir_all(assets_dir.join("tools")).expect("tools dir");
        let provider: Arc<dyn LlmProvider> = Arc::new(
            FakeLlmProvider::from_script(json!({
                "version": 1,
                "calls": [{"kind": "text", "content": "x"}]
            }))
            .expect("fake provider"),
        );
        let harness = Arc::new(HarnessExecutor::new(HarnessSettings {
            assets_dir: assets_dir.clone(),
            tools_dir: assets_dir.join("tools"),
            provider,
            fake_mode: true,
            tool_runner: Arc::new(FakeToolRunner::new()),
        }));
        let worker = Arc::new(WorkerExecutor::new(SupervisorConfig::from_worker(
            WorkerConfig::default(),
        )));
        let routing = super::RoutingExecutor { harness, worker };

        let agent_graph = compiled_graph(&agent_graph_text());
        let agent_node = agent_graph.nodes.get("work").expect("agent node");
        assert_eq!(
            routing.timeout_hint(agent_node),
            Some(1234),
            "ISSUE-076: routing must forward the harness hint for agent_call"
        );

        let python_graph = compiled_graph(&python_graph_text());
        let python_node = python_graph.nodes.get("work").expect("python node");
        assert_eq!(routing.timeout_hint(python_node), None);
    }

    #[test]
    fn lifecycle_detail_covers_realtime_events() {
        // ISSUE-085：旁路事件 → 实时生命周期行；非 events 输入与流式增量不产生行。
        use super::lifecycle_detail;
        use zgraph_trace::{
            GateResultRecord, RunFinishedInfo, RunStartedInfo, StreamingEvent, TraceInput,
        };

        let started = TraceInput::RunStarted(RunStartedInfo {
            run_id: "r1".to_string(),
            graph_hash: "0123456789abcdef".to_string(),
            graph_name: "demo".to_string(),
            dsl_json: "{}".to_string(),
            user_id: "local".to_string(),
            entry_inputs: json!({}),
            seed: 7,
            record_mode: RecordMode::Hash,
            owner_pid: 1,
            owner_host: "test-host".to_string(),
            started_at: "2026-10-28T09:12:03.104Z".to_string(),
        });
        assert_eq!(
            lifecycle_detail(&started).as_deref(),
            Some("RunStarted status=Running user=local")
        );

        let gate = TraceInput::GateResult(GateResultRecord {
            run_id: "r1".to_string(),
            instance_id: "i1".to_string(),
            node_id: "work".to_string(),
            edge_index: 2,
            to_node: "end".to_string(),
            to_port: "r".to_string(),
            gate_fn: Some("gates.ok".to_string()),
            passed: false,
            error: None,
            ts: "2026-10-28T09:12:03.104Z".to_string(),
        });
        assert_eq!(
            lifecycle_detail(&gate).as_deref(),
            Some("GateResult work edge#2 gate=gates.ok passed=false")
        );

        let timed_out = TraceInput::NodeTimedOut {
            run_id: "r1".to_string(),
            instance_id: "i1".to_string(),
            elapsed_ms: 1500,
            at: "2026-10-28T09:12:03.104Z".to_string(),
        };
        assert_eq!(
            lifecycle_detail(&timed_out).as_deref(),
            Some("NodeTimedOut - elapsed_ms=1500")
        );

        // Streaming：progress/tool_call 实时成行；token/reasoning 增量由 `--stream` 负责。
        let progress = TraceInput::Streaming {
            run_id: "r1".to_string(),
            instance_id: "i1".to_string(),
            node_id: Some("work".to_string()),
            event: StreamingEvent::Progress {
                percent: 42,
                message: "calling".to_string(),
            },
            at: "2026-10-28T09:12:03.104Z".to_string(),
        };
        assert_eq!(
            lifecycle_detail(&progress).as_deref(),
            Some("Streaming work progress 42% (calling)")
        );

        let tool_start = TraceInput::Streaming {
            run_id: "r1".to_string(),
            instance_id: "i1".to_string(),
            node_id: Some("work".to_string()),
            event: StreamingEvent::ToolCallStart {
                call_id: "c1".to_string(),
                tool_name: "now".to_string(),
                args_summary: json!({}),
            },
            at: "2026-10-28T09:12:03.104Z".to_string(),
        };
        assert_eq!(
            lifecycle_detail(&tool_start).as_deref(),
            Some("Streaming work tool_call_start now")
        );

        let token = TraceInput::Streaming {
            run_id: "r1".to_string(),
            instance_id: "i1".to_string(),
            node_id: Some("work".to_string()),
            event: StreamingEvent::TokenDelta {
                text: "he".to_string(),
            },
            at: "2026-10-28T09:12:03.104Z".to_string(),
        };
        assert_eq!(lifecycle_detail(&token), None, "token 增量不逐块成行");

        let finished = TraceInput::RunFinished(RunFinishedInfo {
            run_id: "r1".to_string(),
            status: zgraph_trace::RunStatus::Cancelled,
            output_summary: None,
            error: None,
            metrics: zgraph_trace::RunMetrics::default(),
            at: "2026-10-28T09:12:04.104Z".to_string(),
        });
        assert_eq!(
            lifecycle_detail(&finished).as_deref(),
            Some("RunFinished status=Cancelled")
        );
    }

    #[test]
    fn exit_code_mapping_matches_spec() {
        assert_eq!(exit_code_for(&outcome(RunStatus::Completed, None)), 0);
        assert_eq!(
            exit_code_for(&outcome(
                RunStatus::Failed,
                Some(RunError::new("NODE_INTERNAL", "boom"))
            )),
            1
        );
        assert_eq!(exit_code_for(&outcome(RunStatus::Cancelled, None)), 130);
        // ISSUE-036：存储 fail-stop 的**内存态** STORAGE_ERROR → 退出码 4（06 §6.4.5）。
        assert_eq!(
            exit_code_for(&outcome(
                RunStatus::Failed,
                Some(RunError::new(CODE_STORAGE_ERROR, "injected"))
            )),
            4
        );
    }
}
