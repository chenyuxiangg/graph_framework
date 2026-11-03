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
//! （agent 输出为整包四字段，05 §5.9.2）。LLM `llm_calls` 记录见 [`LlmCallTraceRecorder`]。
//!
//! # run 结束元数据列（ISSUE-082 / 06 §6.2.4）
//!
//! run 结束（**Completed 与 Failed 均尽力**）且关闭写端前，CLI 经
//! [`TraceWriter::update_run_columns`] 补写 `runs.worker_versions_json`（来自 worker
//! 握手 `hello` 的 python/sdk 版本，去重排序）与 `runs.model_params_json`（来自
//! AgentConfig `model` 段摘要，**不含密钥**，去重）；本 run 未使用 worker/agent 时
//! 写 `[]`。`:memory:` 库**不写**这两列（跨进程不可读，同 06 §6.9.1 口径）。
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
//!
//! # human 决策消费（W3a / DR-006 §7.3）
//!
//! 含 `CompiledHandler::Human` 节点的图、非 `:memory:` 时，run 启动成功后启动
//! decision consumer **专线程**：每 [`HUMAN_DECISION_POLL_MS`] 用只读连接
//! （`busy_timeout=1000ms`）查 `DecisionConsumer::pending`；主 run 循环把每一行
//! pending **一律**提交 `RunControl::HumanDecision`（不按 WaitingHuman 集合过滤；
//! 内核幂等校验，`applied=false` 同样标 consumed，不得悬挂）。内核确认经
//! [`bridge::HumanDecisionObserver`] 回传 consumer 线程 → 独立 RW 短事务
//! `mark_consumed`（`busy_timeout=5000ms`；失败告警 + 保留 NULL 下轮重试）→ stderr
//! 决策行（`-q/--quiet` 抑制；`applied=false` 记 warning）。run 终态后 consumer
//! 停止并在退出前 flush 已确认决策；DB 访问全在专线程，不阻塞事件循环、不执行
//! 用户 Python。无 human 节点的图不启动 consumer（行为逐字节不变）。
//!
//! human 图 + `--db :memory:`：启动前预检拒绝 `RUN_ENTRY_INVALID`（exit 3、reason
//! `trace_required_for_human`、**不产生 run**、不打印 run id；DR-006 §7.3 #7/N10）。

use std::collections::{BTreeSet, HashMap, HashSet};
use std::future::Future;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::process::ExitCode;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc;
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use serde_json::{json, Map, Value};

use zgraph_dsl::{load_graph_with_options, LoadOptions, LoadedGraph, ValidationReport};
use zgraph_harness::{
    BudgetConfig, FakeLlmProvider, GenaiLlmProvider, HarnessError, HarnessExecutor,
    HarnessSettings, LlmCallRecorder, LlmCallSnapshot, LlmProvider, PricingTable, RunUsageStats,
    UsageTotals, WorkerToolRunner,
};
use zgraph_kernel::{
    CompiledGraph, CompiledHandler, CompiledNode, HumanDecision, HumanDecisionRecord, NodeError,
    NodeExecution, NodeExecutor, RunControl, RunError, RunOptions, RunOutcome, RunSpec, RunStatus,
    StreamingEvent, CODE_HARNESS_MCP_ERROR, CODE_RUN_ENTRY_INPUT_MISSING, CODE_RUN_ENTRY_INVALID,
};
use zgraph_trace::{
    DecisionConsumer, HumanDecisionRow, LlmCallRecord, RecordMode, RecordedOutput, TraceError,
    TraceHandle, TraceInput, TraceReader, TraceWriter,
};
use zgraph_worker::{SupervisorConfig, WorkerConfig, WorkerExecutor, WorkerLimits};

use crate::bridge::{
    self, BridgeError, BridgeObservers, HumanDecisionObserver, HumanRecording, LifecycleObserver,
    StreamingObserver, TracedRun,
};
use crate::recovery;
use crate::trace_cmd::{resolve_db_path, storage_error_detail};
use crate::{
    EXIT_ENTRY_INPUT, EXIT_INTERRUPTED, EXIT_OTHER, EXIT_STORAGE, EXIT_SUCCESS, EXIT_USAGE,
    HUMAN_DECISION_POLL_MS,
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
  --skills-dir <dir>     skills directory (default: $ZGRAPH_SKILLS or <assets-dir>/skills)
  --allow-mcp            allow configured MCP servers (default: off; enabling MCP starts child
                         processes / opens network connections; registry: <assets-dir>/mcp_servers.json)
  --allow-builtin-tools  allow Rust builtin file tools (builtin__read/write/edit/apply_patch/diff;
                         default: off)
  --workspace-dir <dir>  workspace jail root for builtin file tools (default: .; canonicalized)
  --allow-path <dir>     extra read-only directory for builtin file tools; repeatable
  --allow-write-path <dir>
                         extra writable directory for builtin file tools/bash; repeatable
                         (independent of read-only --allow-path; '/' and system dirs are
                         rejected; requires --allow-builtin-tools or --allow-bash)
  --allow-bash           allow builtin__bash under the macOS OS sandbox (fail-closed when
                         the sandbox is unavailable/nested)
  --allow-exec-path <dir>
                         extra executable directory for builtin__bash; repeatable
                         (requires --allow-bash)
  --allow-network        allow network from builtin__bash (default: deny; requires --allow-bash)

build toolchains (cargo/node/uv): pass --allow-exec-path <toolchain bin dir>, expose SDKs
  read-only via --allow-path, put caches/CARGO_HOME under --allow-write-path (or the
  workspace); cc needs -isysroot '$(xcrun --show-sdk-path)'. Nothing is enabled by default.
  --python <exe>         Python interpreter (default: $PYO3_PYTHON > $ZGRAPH_PYTHON > python3)
  --python-path <dir>    prepend a module search path; repeatable (PYTHONPATH)
  --fake-llm[=<script>]  scripted LLM provider (bare flag = built-in default script)
  --real                 real LLM provider (mutually exclusive with --fake-llm)
  --record <mode>        recorded output mode: none|hash|full (default: hash)
  --pricing <file>       USD price table (JSON; default: $ZGRAPH_PRICING; off)
  --max-tokens <N>       run token budget (integer > 0; off by default)
  --max-cost <USD>       run cost budget (requires --pricing; off by default)
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

/// skills 目录环境变量（v0.3 DR-009 §1；`--skills-dir` 优先级更高）。
pub const SKILLS_ENV: &str = "ZGRAPH_SKILLS";

/// Python 解释器环境变量（spec 权威口径：04 §4.3.6 / 06 §6.10.3 / 08 N-02）。
pub const PYO3_PYTHON_ENV: &str = "PYO3_PYTHON";

/// Python 解释器环境变量（M3+ 任务口径，`PYO3_PYTHON` 之后、`python3` 之前）。
pub const PYTHON_ENV: &str = "ZGRAPH_PYTHON";

/// 价格表环境变量（v0.5/F3b；`--pricing` 优先于 `$ZGRAPH_PRICING`；DR-018 §2）。
pub const PRICING_ENV: &str = "ZGRAPH_PRICING";

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
#[derive(Debug, Clone, PartialEq)]
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
    /// `--skills-dir` 显式路径（`None` 时按 `$ZGRAPH_SKILLS`；再缺省由 harness 用
    /// `<assets-dir>/skills`，且仅当 AgentConfig.skills 非空时才扫描；DR-009 §1）。
    pub skills_dir: Option<PathBuf>,
    /// `--allow-mcp`：显式允许启动配置的 MCP servers（默认关；DR-010 §2.2）。
    pub allow_mcp: bool,
    /// `--allow-builtin-tools`：注册 Rust 内置文件工具（默认关；DR-013 §4）。
    pub allow_builtin_tools: bool,
    /// `--workspace-dir`：内置文件工具 jail 根（缺省 `.`，构造时 canonicalize）。
    pub workspace_dir: Option<PathBuf>,
    /// `--allow-path`（可重复）：额外**只读**目录（canonicalize）。
    pub allow_paths: Vec<PathBuf>,
    /// `--allow-write-path`（可重复）：额外**可写**目录（canonicalize；v0.5/F2）。
    pub allow_write_paths: Vec<PathBuf>,
    /// `--allow-bash`：注册 `builtin__bash`（需 macOS OS 沙箱可用；fail-closed）。
    pub allow_bash: bool,
    /// `--allow-exec-path`（可重复）：bash 额外可执行目录。
    pub exec_paths: Vec<PathBuf>,
    /// `--allow-network`：bash 网络（默认 deny）。
    pub allow_network: bool,
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
    /// `--pricing <file>`（v0.5/F3b；缺省读 `$ZGRAPH_PRICING`；默认关）。
    pub pricing: Option<PathBuf>,
    /// `--max-tokens <N>`（run 总 token 预算；默认关；**有符号原值**，`<=0` 由配置
    /// 预检按 `budget_invalid` exit3 拒绝，语法错才 exit2）。
    pub max_tokens: Option<i64>,
    /// `--max-cost <USD>`（run 总费用预算；默认关；需 `--pricing`）。
    pub max_cost: Option<f64>,
}

/// 解析 `run` 之后的参数；`--help`/`-h` 返回 `Ok(None)`。
pub fn parse(args: &[String]) -> Result<Option<RunArgs>, String> {
    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        return Ok(None);
    }
    let mut state = ParseState::default();
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        match state.consume(arg, &mut iter)? {
            Consumed::Handled => {}
            Consumed::Positional => state.set_graph(arg)?,
        }
    }
    state.finish()
}

/// 一个参数的消费结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Consumed {
    /// 已作为选项消费（含 `--fake-llm=`/`--record=` 前缀形式）。
    Handled,
    /// 非选项参数：由 [`parse`] 按位置参数（图路径）处理。
    Positional,
}

/// `run` 参数的扫描中间态（`Default` 即 CLI 默认值；`parse` 内部使用）。
#[derive(Debug)]
struct ParseState {
    graph: Option<PathBuf>,
    inputs: Option<String>,
    user_id: String,
    db: Option<PathBuf>,
    assets_dir: Option<PathBuf>,
    tools_dir: Option<PathBuf>,
    skills_dir: Option<PathBuf>,
    allow_mcp: bool,
    allow_builtin_tools: bool,
    workspace_dir: Option<PathBuf>,
    allow_paths: Vec<PathBuf>,
    allow_write_paths: Vec<PathBuf>,
    allow_bash: bool,
    exec_paths: Vec<PathBuf>,
    allow_network: bool,
    python: Option<String>,
    python_paths: Vec<PathBuf>,
    fake_llm: FakeLlmMode,
    real: bool,
    record: RecordMode,
    stream: bool,
    json: bool,
    quiet: bool,
    pricing: Option<PathBuf>,
    max_tokens: Option<i64>,
    max_cost: Option<f64>,
}

impl Default for ParseState {
    fn default() -> Self {
        Self {
            graph: None,
            inputs: None,
            user_id: DEFAULT_USER_ID.to_string(),
            db: None,
            assets_dir: None,
            tools_dir: None,
            skills_dir: None,
            allow_mcp: false,
            allow_builtin_tools: false,
            workspace_dir: None,
            allow_paths: Vec::new(),
            allow_write_paths: Vec::new(),
            allow_bash: false,
            exec_paths: Vec::new(),
            allow_network: false,
            python: None,
            python_paths: Vec::new(),
            fake_llm: FakeLlmMode::Off,
            real: false,
            record: RecordMode::Hash,
            stream: false,
            json: false,
            quiet: false,
            pricing: None,
            max_tokens: None,
            max_cost: None,
        }
    }
}

impl ParseState {
    /// 消费一个参数；未知选项/缺值按用法错误返回，非选项交给调用方按位置参数处理。
    fn consume<'a>(
        &mut self,
        arg: &'a str,
        iter: &mut impl Iterator<Item = &'a String>,
    ) -> Result<Consumed, String> {
        if let Some(path) = arg.strip_prefix("--fake-llm=") {
            self.fake_llm = FakeLlmMode::Script(non_empty_path(path, "--fake-llm")?);
            return Ok(Consumed::Handled);
        }
        if let Some(value) = arg.strip_prefix("--record=") {
            self.record = parse_record(value)?;
            return Ok(Consumed::Handled);
        }
        match arg {
            "--inputs" => self.inputs = Some(option_value(iter, "--inputs")?),
            "--user-id" => self.user_id = option_value(iter, "--user-id")?,
            "--db" => self.db = Some(PathBuf::from(option_value(iter, "--db")?)),
            "--assets-dir" => {
                self.assets_dir = Some(PathBuf::from(option_value(iter, "--assets-dir")?));
            }
            "--tools-dir" => {
                self.tools_dir = Some(PathBuf::from(option_value(iter, "--tools-dir")?));
            }
            "--skills-dir" => {
                self.skills_dir = Some(PathBuf::from(option_value(iter, "--skills-dir")?));
            }
            "--allow-mcp" => {
                self.allow_mcp = true;
            }
            "--allow-builtin-tools" => {
                self.allow_builtin_tools = true;
            }
            "--workspace-dir" => {
                self.workspace_dir = Some(PathBuf::from(option_value(iter, "--workspace-dir")?));
            }
            "--allow-path" => {
                self.allow_paths
                    .push(PathBuf::from(option_value(iter, "--allow-path")?));
            }
            "--allow-write-path" => {
                self.allow_write_paths
                    .push(PathBuf::from(option_value(iter, "--allow-write-path")?));
            }
            "--allow-bash" => {
                self.allow_bash = true;
            }
            "--allow-exec-path" => {
                self.exec_paths
                    .push(PathBuf::from(option_value(iter, "--allow-exec-path")?));
            }
            "--allow-network" => {
                self.allow_network = true;
            }
            "--python" => self.python = Some(option_value(iter, "--python")?),
            "--python-path" => {
                self.python_paths
                    .push(PathBuf::from(option_value(iter, "--python-path")?));
            }
            "--fake-llm" => self.fake_llm = FakeLlmMode::DefaultScript,
            "--real" => self.real = true,
            "--record" => self.record = parse_record(&option_value(iter, "--record")?)?,
            "--pricing" => {
                self.pricing = Some(PathBuf::from(option_value(iter, "--pricing")?));
            }
            "--max-tokens" => {
                let value = option_value(iter, "--max-tokens")?;
                self.max_tokens = Some(parse_i64_option(&value, "--max-tokens")?);
            }
            "--max-cost" => {
                let value = option_value(iter, "--max-cost")?;
                self.max_cost = Some(parse_f64_option(&value, "--max-cost")?);
            }
            "--stream" => self.stream = true,
            "--json" => self.json = true,
            "-q" | "--quiet" => self.quiet = true,
            flag if flag.starts_with('-') => return Err(format!("unknown option: {flag}")),
            _ => return Ok(Consumed::Positional),
        }
        Ok(Consumed::Handled)
    }

    /// 记录图路径（只允许一个位置参数）。
    fn set_graph(&mut self, value: &str) -> Result<(), String> {
        if self.graph.is_some() {
            return Err(format!("unexpected argument: {value}"));
        }
        self.graph = Some(PathBuf::from(value));
        Ok(())
    }

    /// 校验必填项/互斥项并构造 [`RunArgs`]（错误文案与顺序与拆分前一致）。
    fn finish(self) -> Result<Option<RunArgs>, String> {
        if self.real && self.fake_llm != FakeLlmMode::Off {
            return Err("--real conflicts with --fake-llm".to_string());
        }
        if (self.allow_network || !self.exec_paths.is_empty()) && !self.allow_bash {
            return Err("--allow-network/--allow-exec-path require --allow-bash".to_string());
        }
        if !self.allow_write_paths.is_empty() && !(self.allow_builtin_tools || self.allow_bash) {
            return Err(
                "--allow-write-path requires --allow-builtin-tools or --allow-bash".to_string(),
            );
        }
        let graph = self
            .graph
            .ok_or_else(|| "run requires <graph.json>".to_string())?;
        let inputs = self
            .inputs
            .ok_or_else(|| "run requires --inputs <json|@file>".to_string())?;
        Ok(Some(RunArgs {
            graph,
            inputs,
            user_id: self.user_id,
            db: self.db,
            assets_dir: self.assets_dir,
            tools_dir: self.tools_dir,
            skills_dir: self.skills_dir,
            allow_mcp: self.allow_mcp,
            allow_builtin_tools: self.allow_builtin_tools,
            workspace_dir: self.workspace_dir,
            allow_paths: self.allow_paths,
            allow_write_paths: self.allow_write_paths,
            allow_bash: self.allow_bash,
            exec_paths: self.exec_paths,
            allow_network: self.allow_network,
            python: self.python,
            python_paths: self.python_paths,
            fake_llm: self.fake_llm,
            real: self.real,
            record: self.record,
            stream: self.stream,
            json: self.json,
            quiet: self.quiet,
            pricing: self.pricing,
            max_tokens: self.max_tokens,
            max_cost: self.max_cost,
        }))
    }
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

/// 解析有符号整数值选项（语法错误 → 用法错误 exit 2）。
///
/// REV-127 C1：`--max-tokens` 必须使用**有符号**解析，负数原值交给
/// [`BudgetConfig::validate`]（`budget_invalid` exit3），不能当语法错误 exit2。
fn parse_i64_option(value: &str, option: &str) -> Result<i64, String> {
    value
        .parse::<i64>()
        .map_err(|_| format!("{option} requires an integer (got '{value}')"))
}

/// 解析浮点值选项（语法错误 → 用法错误 exit 2；`<=0`/NaN/Inf 由配置预检负责）。
fn parse_f64_option(value: &str, option: &str) -> Result<f64, String> {
    value
        .parse::<f64>()
        .map_err(|_| format!("{option} requires a number (got '{value}')"))
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
///
/// W3a：human 实例不经执行器，由 [`crate::bridge::TraceBridge`] 在
/// `transition → Completed` 时调用本函数补录（同一 hash/full 口径，`pub(crate)`）。
pub(crate) fn record_output(
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

/// skills 目录：`--skills-dir` > `$ZGRAPH_SKILLS` > `None`（harness 再回退
/// `<assets-dir>/skills`；仅当 AgentConfig.skills 非空时才扫描，DR-009 §1）。
fn resolve_skills_dir(args: &RunArgs) -> Option<PathBuf> {
    args.skills_dir.clone().or_else(|| env_path(SKILLS_ENV))
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

/// v0.4/D2：解析 `--allow-builtin-tools`/`--workspace-dir`/`--allow-path`。
///
/// 未 opt-in → `Ok(None)`（不解析/不触碰 workspace，默认路径零副作用）；配置非法 →
/// `HARNESS_CONFIG_LOAD{reason}` + exit 3（不产生 run）。
fn resolve_builtin_tools(
    args: &RunArgs,
) -> Result<Option<Arc<zgraph_harness::BuiltinToolSet>>, ExitCode> {
    if !args.allow_builtin_tools && !args.allow_bash {
        return Ok(None);
    }
    let workspace = args
        .workspace_dir
        .clone()
        .unwrap_or_else(|| PathBuf::from("."));
    // v0.4/D4：`--allow-bash` 构造期渲染 profile + preflight（嵌套/不可用/非法 →
    // HARNESS_CONFIG_LOAD{reason}，exit 3、不产生 run）；文件/搜索工具仍需
    // `--allow-builtin-tools`（bash 不隐含文件工具）。
    let bash = args.allow_bash.then(|| zgraph_harness::BashOptions {
        exec_paths: args.exec_paths.clone(),
        allow_network: args.allow_network,
    });
    match zgraph_harness::BuiltinToolSet::new_with_paths(
        &workspace,
        &args.allow_paths,
        &args.allow_write_paths,
        args.allow_builtin_tools,
        bash,
    ) {
        Ok(tools) => Ok(Some(tools)),
        Err(error) => {
            let reason = error
                .detail
                .get("reason")
                .and_then(|value| value.as_str())
                .unwrap_or("workspace_invalid");
            eprintln!(
                "error[{}]: {} (reason: {reason})",
                error.code, error.message
            );
            Err(ExitCode::from(EXIT_ENTRY_INPUT))
        }
    }
}

/// 运行时内的主流程（启动顺序见模块文档）。
async fn run_in_runtime(args: &RunArgs) -> ExitCode {
    // v0.4/D2/D4：内置工具 opt-in 预检放在打开 trace/storage **之前**——`--allow-bash`
    // 的 sandbox preflight 失败（嵌套/不可用/profile 非法）→ exit 3、不产生 run、
    // 不创建 DB；未 opt-in 时不解析 workspace（零副作用）。
    let builtin_tools = match resolve_builtin_tools(args) {
        Ok(tools) => tools,
        Err(code) => return code,
    };
    // v0.5/F3b（DR-018 §2/§3）：pricing/budget 纯 flag 预检同样放在打开 trace/storage
    // **之前**——配置错误 → exit 3、不创建 DB、不产生 run。
    let observability = match resolve_observability(args) {
        Ok(observability) => observability,
        Err(code) => return code,
    };
    let db_path = resolve_db_path(args.db.as_deref());
    let memory_db = is_memory_db(&db_path);
    if memory_db {
        warn_memory_db();
    }
    let (writer, recovery) = match open_writer_with_recovery(&db_path) {
        Ok(opened) => opened,
        Err(code) => return code,
    };
    let assets_dir = resolve_assets_dir(args);
    let tools_dir = resolve_tools_dir(args, &assets_dir);
    let skills_dir = resolve_skills_dir(args);
    let (graph, entry_inputs) = match load_graph_and_inputs(args, &assets_dir) {
        Ok(loaded) => loaded,
        Err(code) => {
            close_quietly(writer);
            return code;
        }
    };
    // v0.5/F3b（DR-018 §2 / AC-27 d）：`--max-cost` 需收集 graph 全部 agent_call 的
    // AgentConfig model 并校验价格覆盖（外部 subprocess harness 成本不可得 →
    // fail-closed）；任何不满足 → 配置错误 exit 3、不产生 run。
    let observability = match prepare_graph_observability(&graph, &assets_dir, observability) {
        Ok(observability) => observability,
        Err(code) => {
            close_quietly(writer);
            return code;
        }
    };
    // W3a（DR-006 §7.3 #7 / N10）：human 图要求可跨进程读写的 trace DB（decide 与
    // consumer 都经该库交互）；`--db :memory:` 在**启动前**拒绝——不产生 run、不打印
    // run id、不启动 consumer（exit 3，与「入口非法不产生 run」同口径）。
    if memory_db && graph_has_human(&graph) {
        close_quietly(writer);
        eprintln!(
            "error[{CODE_RUN_ENTRY_INVALID}]: human nodes require a cross-process trace \
             database; --db :memory: cannot be read by 'zgraph decide' \
             (reason: trace_required_for_human)"
        );
        return ExitCode::from(EXIT_ENTRY_INPUT);
    }
    // V3（DR-010 §2.2 / AC-19 c）：MCP 未开启 → run 启动前失败（不产生 run、不启 server、exit 3）。
    if !args.allow_mcp {
        if let Some(agent_id) = graph_agent_using_mcp(&graph, &assets_dir) {
            close_quietly(writer);
            eprintln!(
                "error[{CODE_HARNESS_MCP_ERROR}]: MCP is disabled but agent '{agent_id}'                  configures MCP servers (reason: not_allowed); pass --allow-mcp to enable"
            );
            return ExitCode::from(EXIT_ENTRY_INPUT);
        }
    }
    // W3a：human 图启动 decision consumer（非 memory；无 human 图的图不启动，行为不变）。
    let human_consumer = !memory_db && graph_has_human(&graph);
    let (decision_tx, decision_rx) = mpsc::channel::<HumanDecisionRecord>();
    let decision_observer = human_consumer.then(|| {
        let sender = decision_tx.clone();
        Arc::new(move |record: &HumanDecisionRecord| {
            // 内核事件循环线程内同步转发（unbounded mpsc 发送不阻塞）；return 值忽略。
            let _ = sender.send(record.clone());
        }) as HumanDecisionObserver
    });
    let trace_handle = writer.handle();
    // run id 由 CLI 预生成：`llm_calls` 记录器在启动前就需要 run_id（快照不含 run）。
    let run_id = uuid::Uuid::now_v7().to_string();
    let built = match prepare_executor(
        args,
        &assets_dir,
        &tools_dir,
        skills_dir,
        builtin_tools,
        &trace_handle,
        &run_id,
        &observability,
    ) {
        Ok(built) => built,
        Err(code) => {
            close_quietly(writer);
            return code;
        }
    };
    let run_metadata = run_metadata_hook(memory_db, &built, &run_id);
    let harness_handle = Arc::clone(&built.harness);
    let (traced, lifecycle) = match start_run(
        args,
        writer,
        graph,
        entry_inputs,
        built.executor,
        &run_id,
        decision_observer,
    ) {
        Ok(started) => started,
        Err(code) => {
            // v0.3/V3（REV-098 #3）：错误路径也关闭 MCP 连接/子进程组（幂等）。
            built.harness.close_mcp().await;
            // v0.3/V4（DR-011 §2.2）：错误路径同样关闭外部 harness 进程组（幂等）。
            built.harness.close_provider().await;
            return code;
        }
    };
    debug_assert_eq!(traced.run_id(), run_id);
    // consumer 与 run 同生命周期：run 已启动（可能已产生 RunStarted）后再起专线程。
    let consumer =
        human_consumer.then(|| DecisionSession::spawn(&db_path, &run_id, args.quiet, decision_rx));
    let exit = wait_and_report(
        args,
        traced,
        lifecycle,
        consumer,
        RunFinishContext {
            memory_db,
            run_metadata,
            db_path: &db_path,
            recovery: &recovery,
            harness: harness_handle,
            observability,
        },
    )
    .await;
    // v0.3/V3（REV-098 #3）：run 结束（Completed/Failed/Cancelled/错误）后**立即**关闭
    // 所有 MCP 连接与 stdio 子进程组；`--json`/human/普通出口均经过此处。幂等。
    built.harness.close_mcp().await;
    // v0.3/V4（DR-011 §2.2）：run 结束/取消后关闭外部 harness 进程组（幂等、
    // 无 subprocess provider 时零开销；AC-20 d/f 无孤儿进程）。
    built.harness.close_provider().await;
    exit
}

/// 图是否含人工等待节点（`builtin.human` → [`CompiledHandler::Human`]）。
///
/// W3a（DR-006 §2.1）：只有这类图才需要跨进程 trace DB 与 decision consumer；
/// 无 human 节点的图行为完全不变。
/// 图中第一个配置了 MCP 的 agent_call 的 agent_id（CLI 预检；不启进程）。
fn graph_agent_using_mcp(graph: &CompiledGraph, assets_dir: &Path) -> Option<String> {
    for node_id in &graph.node_order {
        let Some(node) = graph.nodes.get(node_id) else {
            continue;
        };
        if let zgraph_kernel::CompiledHandler::Agent { agent_id, .. } = &node.handler {
            if zgraph_harness::agent_uses_mcp(assets_dir, agent_id) {
                return Some(agent_id.clone());
            }
        }
    }
    None
}

fn graph_has_human(graph: &CompiledGraph) -> bool {
    graph
        .nodes
        .values()
        .any(|node| matches!(node.handler, CompiledHandler::Human))
}

/// human 实例 L2 补录配置（W3a / DR-006 §2.2、§8 L2）。
///
/// `--record=none` 或无 human 节点 → `None`（行为逐字节不变）；否则收集图内
/// `CompiledHandler::Human` 节点 id，由 [`crate::bridge::TraceBridge`] 在 human
/// 实例 `→Completed` 时补录 `recorded_outputs`（复用 [`record_output`] 口径）。
fn human_recording(mode: RecordMode, graph: &CompiledGraph) -> Option<HumanRecording> {
    if mode == RecordMode::None {
        return None;
    }
    let human_nodes: BTreeSet<String> = graph
        .nodes
        .values()
        .filter(|node| matches!(node.handler, CompiledHandler::Human))
        .map(|node| node.id.clone())
        .collect();
    if human_nodes.is_empty() {
        return None;
    }
    Some(HumanRecording { mode, human_nodes })
}

/// 打开 trace 写端并执行启动中止标定（06 §6.8.1；打开/标定失败 → exit 4）。
///
/// 标定告警与「跳过标定」告警都在此处写 stderr（06 §6.8.1 步骤 2②/步骤 5）。
fn open_writer_with_recovery(
    db_path: &Path,
) -> Result<(TraceWriter, recovery::Recovery), ExitCode> {
    let writer = match TraceWriter::open(db_path) {
        Ok(writer) => writer,
        Err(error) => return Err(report_storage_error(&error)),
    };
    let recovery = match recovery::recover_interrupted(&writer) {
        Ok(recovery) => recovery,
        Err(error) => {
            close_quietly(writer);
            return Err(report_storage_error(&error));
        }
    };
    for warning in &recovery.warnings {
        eprintln!("{warning}");
    }
    for item in &recovery.recovered {
        eprintln!("{}", recovery::warning_line(item));
    }
    Ok((writer, recovery))
}

/// 加载并编译图 + 校验入口输入（失败由调用方关闭写端并返回对应退出码）。
fn load_graph_and_inputs(
    args: &RunArgs,
    assets_dir: &Path,
) -> Result<(Arc<CompiledGraph>, EntryInputs), ExitCode> {
    let graph = load_compiled_graph(args, assets_dir)?;
    let entry_inputs = resolve_inputs(args, &graph)?;
    Ok((graph, entry_inputs))
}

/// v0.5/F3b（DR-018 §2/§3）：可观测/预算运行时配置（启动前预检结果；默认全关）。
#[derive(Clone)]
struct Observability {
    /// 价格表（`--pricing`/`$ZGRAPH_PRICING`；未配置 = `None`）。
    pricing: Option<Arc<PricingTable>>,
    /// 预算上限（默认关）。
    budget: BudgetConfig,
    /// `--max-cost` 时从 graph 收集的 Agent 模型（去重；`with_required_models` 必需）。
    models: Option<BTreeSet<String>>,
    /// graph 中是否含外部 subprocess harness（DSH/Codex：usage/成本不可统计）。
    external_harness: bool,
}

impl Observability {
    /// 是否显式启用可观测/预算输出（无配置时既有输出逐字节不变）。
    fn enabled(&self) -> bool {
        self.pricing.is_some() || self.budget.is_enabled()
    }
}

/// Harness 配置错误 → `error[HARNESS_CONFIG_LOAD] (reason: …)` + exit 3（不产生 run）。
fn report_harness_config_error(error: &HarnessError) -> ExitCode {
    let reason = error
        .detail
        .get("reason")
        .and_then(Value::as_str)
        .unwrap_or("config_invalid");
    eprintln!(
        "error[{}]: {} (reason: {reason})",
        error.code, error.message
    );
    ExitCode::from(EXIT_ENTRY_INPUT)
}

/// 构造一条 `HARNESS_CONFIG_LOAD` 配置错误并输出 + exit 3（不产生 run）。
fn config_error(reason: &str, message: impl Into<String>) -> ExitCode {
    let error = HarnessError::config_load(reason, None, None, None, None, message);
    report_harness_config_error(&error)
}

/// 启动前**纯 flag** 预检（不触碰 trace storage / 不产生 run；DR-018 §2/§3）。
///
/// - pricing 缺文件/坏 JSON/非 USD → `pricing_file_not_found`/`pricing_invalid` exit3；
/// - `--max-cost` 无 pricing → `pricing_required_for_max_cost` exit3；
/// - budget 非法（`<=0`/NaN/Inf）→ `budget_invalid` exit3。
fn resolve_observability(args: &RunArgs) -> Result<Observability, ExitCode> {
    let pricing_path = args.pricing.clone().or_else(|| env_path(PRICING_ENV));
    let pricing = match pricing_path {
        Some(path) => match PricingTable::load_file(&path) {
            Ok(pricing) => Some(Arc::new(pricing)),
            Err(error) => return Err(report_harness_config_error(&error)),
        },
        None => None,
    };
    // REV-127 C1：`--max-tokens` 有符号原值；`<=0` → 与 `BudgetConfig.validate`
    // 同口径的 `budget_invalid` exit3（保留真实负值 detail），语法错已在 parse 阶段
    // exit2。`--max-cost` 的 `<=0`/NaN/Inf 同样交给 validate。
    let max_tokens = match args.max_tokens {
        Some(value) if value <= 0 => {
            return Err(config_error(
                "budget_invalid",
                format!("--max-tokens must be > 0 (got {value})"),
            ))
        }
        Some(value) => Some(value as u64),
        None => None,
    };
    let budget = BudgetConfig {
        max_tokens,
        max_cost: args.max_cost,
    };
    if let Err(error) = budget.validate() {
        return Err(report_harness_config_error(&error));
    }
    if budget.max_cost.is_some() && pricing.is_none() {
        return Err(config_error(
            "pricing_required_for_max_cost",
            "--max-cost requires --pricing (no built-in prices)",
        ));
    }
    Ok(Observability {
        pricing,
        budget,
        models: None,
        external_harness: false,
    })
}

/// graph 级预检（`--max-cost`）：收集全部 agent_call 的 AgentConfig model 去重并验证
/// 价格表覆盖；外部 subprocess harness（DSH/Codex）成本不可得 → fail-closed。
///
/// 只在 `--max-cost` 时做 IO/解析（无该 flag 时默认路径零额外读取）。
fn prepare_graph_observability(
    graph: &CompiledGraph,
    assets_dir: &Path,
    mut observability: Observability,
) -> Result<Observability, ExitCode> {
    if !observability.enabled() {
        return Ok(observability);
    }
    // 仅 pricing（无 max-cost）：best-effort 检测外部 harness 以标注 usage unavailable
    // （不因 agent 配置读取失败改变默认错误路径）；`--max-cost` 下则 fail-closed。
    let max_cost = observability.budget.max_cost.is_some();
    match collect_agent_models(graph, assets_dir) {
        Ok((models, external)) => {
            observability.external_harness = external;
            if max_cost {
                if external {
                    return Err(config_error(
                        "cost_unavailable_external_harness",
                        "--max-cost is unavailable with a subprocess (DSH/Codex) harness: external usage/cost is not provided",
                    ));
                }
                let pricing = observability
                    .pricing
                    .as_ref()
                    .expect("max_cost requires pricing (checked before graph load)");
                for model in &models {
                    if !pricing.has_model(model) {
                        return Err(config_error(
                            "pricing_missing_model",
                            format!(
                                "--max-cost requires pricing for agent model '{model}'; add it to --pricing"
                            ),
                        ));
                    }
                }
                observability.models = Some(models);
            }
        }
        Err(code) => {
            if max_cost {
                return Err(code);
            }
        }
    }
    Ok(observability)
}

/// 收集 graph 内全部 `agent_call` 的 `AgentConfig.model.name`（去重）与外部 harness 标记。
///
/// 直接读取 `agents/*.json`（`agent_index` + model/harness 字段），**不加载** Agent/tools、
/// 不 spawn worker（启动前 fail-closed 预检；DR-018 §2）。
fn collect_agent_models(
    graph: &CompiledGraph,
    assets_dir: &Path,
) -> Result<(BTreeSet<String>, bool), ExitCode> {
    let index = match zgraph_harness::agent_index(assets_dir) {
        Ok(index) => index,
        Err(error) => return Err(report_harness_config_error(&error)),
    };
    let mut models = BTreeSet::new();
    let mut external = false;
    for node in graph.nodes.values() {
        let CompiledHandler::Agent { agent_id, .. } = &node.handler else {
            continue;
        };
        let Some(path) = index.get(agent_id) else {
            return Err(config_error(
                "agent_not_indexed",
                format!(
                    "agent '{agent_id}' is not indexed under {}",
                    assets_dir.display()
                ),
            ));
        };
        let text = match std::fs::read_to_string(path) {
            Ok(text) => text,
            Err(error) => {
                return Err(config_error(
                    "agent_config_invalid",
                    format!("cannot read agent config '{}': {error}", path.display()),
                ))
            }
        };
        let value: Value = match serde_json::from_str(&text) {
            Ok(value) => value,
            Err(error) => {
                return Err(config_error(
                    "agent_config_invalid",
                    format!("agent config '{}' is invalid JSON: {error}", path.display()),
                ))
            }
        };
        let Some(model) = value
            .get("model")
            .and_then(|model| model.get("name"))
            .and_then(Value::as_str)
        else {
            return Err(config_error(
                "agent_config_invalid",
                format!("agent config '{}' has no model.name", path.display()),
            ));
        };
        models.insert(model.to_string());
        if value
            .get("harness")
            .and_then(|harness| harness.get("kind"))
            .and_then(Value::as_str)
            == Some("subprocess")
        {
            external = true;
        }
    }
    Ok((models, external))
}

/// 组合执行器（provider 错误 → 2/4；记录器与并发上限按 CLI 口径接线）。
#[allow(clippy::too_many_arguments)]
fn prepare_executor(
    args: &RunArgs,
    assets_dir: &Path,
    tools_dir: &Path,
    skills_dir: Option<PathBuf>,
    builtin_tools: Option<Arc<zgraph_harness::BuiltinToolSet>>,
    trace_handle: &TraceHandle,
    run_id: &str,
    observability: &Observability,
) -> Result<BuiltExecutors, ExitCode> {
    let (provider, fake_mode) = match build_provider(args) {
        Ok(built) => built,
        Err(error) => return Err(report_provider_error(error)),
    };
    let recorder = llm_call_recorder(args, trace_handle, run_id);
    Ok(build_executor(
        args,
        assets_dir,
        tools_dir,
        skills_dir,
        builtin_tools,
        provider,
        fake_mode,
        trace_handle,
        recorder,
        observability,
    ))
}

/// run 结束后补写版本/模型参数列的 hook（ISSUE-082；`:memory:` 不写）。
fn run_metadata_hook(
    memory_db: bool,
    built: &BuiltExecutors,
    run_id: &str,
) -> Option<BeforeCloseHook> {
    (!memory_db).then(|| {
        let worker = Arc::clone(&built.worker);
        let harness = Arc::clone(&built.harness);
        let run_id = run_id.to_string();
        Box::new(move |writer: &TraceWriter| {
            write_run_metadata(writer, &run_id, &worker, &harness);
        }) as BeforeCloseHook
    })
}

/// human 实时生命周期输出器（ISSUE-085；`--json`/`-q` 不订阅旁路）。
fn lifecycle_output(args: &RunArgs) -> (Option<LifecyclePrinter>, Option<LifecycleObserver>) {
    if args.json || args.quiet {
        return (None, None);
    }
    let (printer, observer) = LifecyclePrinter::new();
    (Some(printer), Some(observer))
}

/// 启动 run 并输出 `created` 行；human 模式在 `created` 之后启动实时打印线程。
///
/// `decision_observer`（W3a）仅 human 图非 `:memory:` 时传入：内核每次应用决策后经
/// 该观察者把确认转发给 decision consumer 线程。
fn start_run(
    args: &RunArgs,
    writer: TraceWriter,
    graph: Arc<CompiledGraph>,
    entry_inputs: EntryInputs,
    executor: Arc<dyn NodeExecutor>,
    run_id: &str,
    decision_observer: Option<HumanDecisionObserver>,
) -> Result<(TracedRun, Option<LifecyclePrinter>), ExitCode> {
    let (mut lifecycle, lifecycle_observer) = lifecycle_output(args);
    let observers = BridgeObservers {
        streaming: args.stream.then(stream_observer),
        lifecycle: lifecycle_observer,
        human_decided: decision_observer,
        human_recording: human_recording(args.record, &graph),
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
        run_id.to_string(),
    ) {
        Ok(traced) => traced,
        Err(error) => return Err(report_bridge_error(&error)),
    };
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
    Ok((traced, lifecycle))
}

/// run 收尾上下文（等待期与结束输出的存储/元数据参数）。
struct RunFinishContext<'a> {
    /// `--db :memory:`（无跨进程可读库；`runs` 两列不写）。
    memory_db: bool,
    /// 关闭写端前的元数据列补写回调（ISSUE-082）。
    run_metadata: Option<BeforeCloseHook>,
    /// trace 库路径（`--json.db` 与输出）。
    db_path: &'a Path,
    /// 启动中止标定结果（recovered/warnings 进输出）。
    recovery: &'a recovery::Recovery,
    /// Harness 句柄（v0.5/F3b：run 结束时读取 usage 统计）。
    harness: Arc<HarnessExecutor>,
    /// v0.5/F3b 可观测/预算运行时配置（默认关 → 输出不变）。
    observability: Observability,
}

/// 等待 run 结束并输出结果（先收尾实时生命周期行，再输出 run_output/摘要）。
///
/// `consumer`（W3a）在 run 终态后停止并 flush 已确认决策的 `consumed_at`。
async fn wait_and_report(
    args: &RunArgs,
    traced: TracedRun,
    mut lifecycle: Option<LifecyclePrinter>,
    consumer: Option<DecisionSession>,
    context: RunFinishContext<'_>,
) -> ExitCode {
    let started = Instant::now();
    let outcome = wait_for_run(traced, consumer, context.memory_db, context.run_metadata).await;
    let duration_ms = duration_millis(started);
    // 先收尾实时行，再输出 run 结果/摘要（stderr 行序稳定、且结果行必然最后出现）。
    if let Some(printer) = lifecycle.take() {
        printer.finish();
    }
    let outcome = match outcome {
        Ok(outcome) => outcome,
        Err(code) => return code,
    };
    let run_id = match &outcome {
        RunCompletion::WithReader(outcome, _) | RunCompletion::OutcomeOnly(outcome) => {
            outcome.run_id.clone()
        }
    };
    let usage = context.harness.run_usage_stats(&run_id);
    finish(
        args,
        outcome,
        duration_ms,
        context.db_path,
        &context.recovery.recovered,
        &context.recovery.warnings,
        &context.observability,
        usage.as_ref(),
    )
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

/// 关闭写端前的回调：补写 run 元数据列（ISSUE-082；`:memory:` 不调用）。
type BeforeCloseHook = Box<dyn FnOnce(&TraceWriter)>;

/// 组合执行器 + run 结束元数据写入所需的只读句柄（ISSUE-082）。
struct BuiltExecutors {
    /// 对外的组合执行器（Recording → Routing）。
    executor: Arc<dyn NodeExecutor>,
    /// Harness 句柄（查询 `model_params`）。
    harness: Arc<HarnessExecutor>,
    /// Worker 句柄（查询 `worker_versions`）。
    worker: Arc<WorkerExecutor>,
}

/// 组合执行器：Recording → Routing（Harness / Worker）。
// CLI 组合根的参数聚合（与既有 `prepare_executor` 同风格）；拆结构体反而降低可读性。
#[allow(clippy::too_many_arguments)]
fn build_executor(
    args: &RunArgs,
    assets_dir: &Path,
    tools_dir: &Path,
    skills_dir: Option<PathBuf>,
    builtin_tools: Option<Arc<zgraph_harness::BuiltinToolSet>>,
    provider: Arc<dyn LlmProvider>,
    fake_mode: bool,
    trace_handle: &TraceHandle,
    recorder: Option<Arc<dyn LlmCallRecorder>>,
    observability: &Observability,
) -> BuiltExecutors {
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
        Some(recorder) => {
            HarnessExecutor::with_llm_recorder_and_tool_limits(settings, recorder, limits)
        }
        None => HarnessExecutor::with_tool_limits(settings, limits),
    };
    // v0.4/D5b2（DR-014 §1）：工具调用审计落 trace v7 `tool_calls`（独立于
    // `--record` 与 events；默认无工具调用时零行、既有输出不变）。
    let harness = harness.with_tool_call_record_sink(Arc::new(
        crate::bridge::ToolCallTraceSink::new(trace_handle.clone()),
    ));
    // v0.5/F3b（DR-018 §2/§3）：价格表/预算/必需模型注入（默认全关 → 零行为变化）。
    // `with_required_models` 在 `--max-cost` 时由 CLI 从 graph 收集（空集合也显式传入，
    // 防止 harness 侧 `pricing_models_required` fail-closed 被绕过）。
    let harness = match observability.pricing.clone() {
        Some(pricing) => harness.with_pricing(pricing),
        None => harness,
    };
    let harness = if observability.budget.is_enabled() {
        harness.with_budget(observability.budget)
    } else {
        harness
    };
    let harness = match &observability.models {
        Some(models) => harness.with_required_models(models.iter().cloned()),
        None => harness,
    };
    // v0.3（DR-009 §1）：`--skills-dir`/env 显式覆盖；None 时 harness 回退资产默认。
    // v0.3（DR-010 §2.2）：`--allow-mcp` 显式开关（默认关；未开启且配置 MCP → 预检 exit 3）。
    // v0.4/D2：`--allow-builtin-tools` 注册 5 个 Rust 内置文件工具（additive）。
    let harness = {
        let harness = harness
            .with_skills_dir(skills_dir)
            .with_mcp_allow(args.allow_mcp);
        let harness = match builtin_tools {
            Some(tools) => harness.with_builtin_tools(tools),
            None => harness,
        };
        Arc::new(harness)
    };
    let routing: Arc<dyn NodeExecutor> = Arc::new(RoutingExecutor {
        harness: Arc::clone(&harness),
        worker: Arc::clone(&worker),
    });
    let executor: Arc<dyn NodeExecutor> = Arc::new(RecordingExecutor::new(
        routing,
        trace_handle.clone(),
        args.record,
    ));
    BuiltExecutors {
        executor,
        harness,
        worker,
    }
}

/// run 结束（Completed/Failed 尽力）补写 `runs` 两列（ISSUE-082；06 §6.2.4）。
///
/// **尽力写（REV-048 C1 方案 B）**：`update_run_columns` 的 `Err` 在此**显式忽略**——
/// trace writer 对元数据列失败不置 fatal（写线程继续服务），因此本函数失败不改变
/// run 退出码/结果输出、也不影响后续 flush/close；若存储真实损坏，flush/close 仍按
/// fail-stop 06 §6.4.5 → exit 4（由 `close_writer` 统一映射）。
///
/// 无对应观测时写 `[]`（表示本 run 未使用 worker/agent）；`:memory:` 库不调用本函数
/// （跨进程不可读，文档注明）。
fn write_run_metadata(
    writer: &TraceWriter,
    run_id: &str,
    worker: &WorkerExecutor,
    harness: &HarnessExecutor,
) {
    let worker_versions = worker.worker_versions(run_id).unwrap_or_default();
    let model_params = harness.model_params(run_id).unwrap_or_default();
    let worker_json = serde_json::to_string(&worker_versions).unwrap_or_else(|_| "[]".to_string());
    let model_json = serde_json::to_string(&model_params).unwrap_or_else(|_| "[]".to_string());
    let _ = writer.update_run_columns(run_id, &worker_json, &model_json);
}

/// run 结束产物：常规库带只读 reader；`:memory:` 只有内核 outcome（06 §6.9.1）。
enum RunCompletion {
    /// 落盘库：可回读 events/instances（生命周期行与 event_count 来源）。
    WithReader(RunOutcome, TraceReader),
    /// 进程内库：reader 不可用，输出只用 outcome（event_count 记 0）。
    OutcomeOnly(RunOutcome),
}

/// 等待 run 结束（SIGINT → cancel → 退出码 130）。
///
/// `before_close`：关闭写端前的回调（ISSUE-082 补写 run 元数据列）；`:memory:`
/// 库不写，回调被丢弃。
///
/// `consumer`（W3a / DR-006 §7.3）：主循环每轮把 consumer 线程送来的 pending 决策
/// **一律**提交 `RunControl::HumanDecision`（不按 WaitingHuman 集合过滤）；run
/// 终态后先停止 consumer（flush 已确认决策的 `consumed_at`）再等待 run 结果，
/// 保证退出前已消费决策已提交。
async fn wait_for_run(
    traced: TracedRun,
    consumer: Option<DecisionSession>,
    memory_db: bool,
    before_close: Option<BeforeCloseHook>,
) -> Result<RunCompletion, ExitCode> {
    let interrupted = Arc::new(AtomicBool::new(false));
    let signal_task = tokio::spawn(watch_interrupt(Arc::clone(&interrupted)));
    let mut submitted: HashSet<String> = HashSet::new();
    while !traced.status().is_terminal() {
        if interrupted.load(Ordering::SeqCst) {
            traced.cancel();
        }
        if let Some(session) = consumer.as_ref() {
            while let Ok(rows) = session.pending.try_recv() {
                submit_human_decisions(&traced, rows, &mut submitted);
            }
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    // run 已终态：停止 consumer 并 flush 已确认决策（未确认行保持 consumed_at=NULL，
    // 留待 W3b；DR-006 E-6/E-5）。
    if let Some(session) = consumer {
        session.finish();
    }
    let completion = if memory_db {
        // ISSUE-082：`:memory:` 无跨进程可读库，版本/模型参数列不写（文档注明）。
        drop(before_close);
        traced
            .wait_without_reader()
            .await
            .map(RunCompletion::OutcomeOnly)
    } else {
        let waited = match before_close {
            Some(before_close) => traced.wait_with_before_close(before_close).await,
            None => traced.wait().await,
        };
        waited.map(|outcome| {
            let (outcome, reader) = outcome.into_parts();
            RunCompletion::WithReader(outcome, reader)
        })
    };
    signal_task.abort();
    completion.map_err(|error| report_bridge_error(&error))
}

/// 把一批 pending 决策提交给内核（W3a / DR-006 §7.3 #2）。
///
/// **总是提交**：包括 bridge 视角下 instance 已非 `WaitingHuman` 的行（由内核幂等
/// 校验回报 `applied=false`，consumer 仍标 consumed，不得悬挂）。`submitted` 防止
/// 同一行在 `mark_consumed` 生效前被重复提交（重复控制只会得到多余拒绝确认）。
fn submit_human_decisions(
    traced: &TracedRun,
    rows: Vec<HumanDecisionRow>,
    submitted: &mut HashSet<String>,
) {
    for row in rows {
        let instance_key = row.instance_id.clone();
        if !submitted.insert(instance_key.clone()) {
            continue;
        }
        let decision = match row.decision.as_str() {
            "approved" => HumanDecision::Approved,
            "rejected" => HumanDecision::Rejected,
            other => {
                eprintln!(
                    "warning: ignoring human decision with unknown value '{other}' \
                     for instance {instance_key}"
                );
                continue;
            }
        };
        let command = RunControl::HumanDecision {
            instance_id: row.instance_id,
            decision,
            approver: row.approver,
            reason: row.reason,
            decision_id: row.decision_id,
            decided_at: row.decided_at,
        };
        if !traced.control(command) {
            // 控制通道已关闭（run 正在收尾）：该行保持未消费，留待 W3b。
            submitted.remove(&instance_key);
        }
    }
}

// ---------------------------------------------------------------------------
// run 内 decision consumer（W3a / DR-006 §7.3）
// ---------------------------------------------------------------------------

/// 待标记 `consumed_at` 的确认（`warned` 限制存储告警只打一次）。
struct PendingMark {
    record: HumanDecisionRecord,
    warned: bool,
}

/// decision consumer 会话（与 run 同生命周期）。
///
/// 独立**专线程**做全部 SQLite 访问（RO `pending` 轮询 + RW `mark_consumed` 短事务），
/// 不阻塞内核事件循环、不执行用户 Python（DR-006 §7.3 #4）。主循环只经
/// [`TracedRun::control`] 提交命令；确认经 [`HumanDecisionObserver`] 回到本线程。
struct DecisionSession {
    /// consumer 线程 → 主循环：待提交的 pending 决策。
    pending: mpsc::Receiver<Vec<HumanDecisionRow>>,
    /// 主循环 → consumer 线程的停止信号（drop = 停止）。
    stop: Option<mpsc::Sender<()>>,
    /// consumer 线程（`finish` join，保证退出前 flush）。
    thread: Option<JoinHandle<()>>,
}

impl DecisionSession {
    /// 启动 consumer 线程；DB 打开失败只告警（不 fail run），run 侧决策行留待 W3b。
    fn spawn(
        db_path: &Path,
        run_id: &str,
        quiet: bool,
        confirmed: mpsc::Receiver<HumanDecisionRecord>,
    ) -> Self {
        let (pending_tx, pending) = mpsc::channel::<Vec<HumanDecisionRow>>();
        let (stop, stop_rx) = mpsc::channel::<()>();
        let db_path = db_path.to_path_buf();
        let run_id = run_id.to_string();
        let thread = std::thread::spawn(move || {
            decision_consumer_loop(db_path, run_id, quiet, pending_tx, confirmed, stop_rx);
        });
        Self {
            pending,
            stop: Some(stop),
            thread: Some(thread),
        }
    }

    /// 停止 poller 并 join（consumer 线程在退出前 flush 已确认决策）。
    fn finish(mut self) {
        drop(self.stop.take());
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// consumer 线程主体：轮询 pending → 主循环提交 → 等确认 → `mark_consumed` + stderr 行。
///
/// 轮询节奏：每轮先 drain 确认并尝试标记，再查一次 `pending`（RO），最后最多等待
/// [`HUMAN_DECISION_POLL_MS`] 到下一个确认（收到即处理，延迟远小于轮询间隔）。
fn decision_consumer_loop(
    db_path: PathBuf,
    run_id: String,
    quiet: bool,
    pending: mpsc::Sender<Vec<HumanDecisionRow>>,
    confirmed: mpsc::Receiver<HumanDecisionRecord>,
    stop: mpsc::Receiver<()>,
) {
    let consumer = match DecisionConsumer::open(&db_path) {
        Ok(consumer) => consumer,
        Err(error) => {
            eprintln!(
                "warning: decision consumer disabled for run {run_id}: {} ({})",
                error.code(),
                storage_error_detail(&error)
            );
            return;
        }
    };
    let mut retry: Vec<PendingMark> = Vec::new();
    loop {
        if stop_disconnected(&stop) {
            break;
        }
        drain_confirmed(&confirmed, &mut retry);
        flush_confirmed(&consumer, &mut retry, quiet);
        match consumer.pending(&run_id) {
            Ok(rows) if !rows.is_empty() => {
                if pending.send(rows).is_err() {
                    break;
                }
            }
            Ok(_) => {}
            Err(error) => {
                eprintln!(
                    "warning: cannot poll human decisions for run {run_id}: {} ({})",
                    error.code(),
                    storage_error_detail(&error)
                );
            }
        }
        match confirmed.recv_timeout(Duration::from_millis(HUMAN_DECISION_POLL_MS)) {
            Ok(record) => retry.push(PendingMark {
                record,
                warned: false,
            }),
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
    // 退出前 flush：已确认决策必须尽量落 consumed_at（DR-006 §7.3 #3）。
    drain_confirmed(&confirmed, &mut retry);
    flush_confirmed(&consumer, &mut retry, quiet);
}

/// 停止信号是否已到（sender 被 drop → `Disconnected`）。
fn stop_disconnected(stop: &mpsc::Receiver<()>) -> bool {
    matches!(
        stop.try_recv(),
        Ok(()) | Err(mpsc::TryRecvError::Disconnected)
    )
}

/// 非阻塞 drain 确认通道（run 收尾前可能已排队多条确认）。
fn drain_confirmed(confirmed: &mpsc::Receiver<HumanDecisionRecord>, retry: &mut Vec<PendingMark>) {
    while let Ok(record) = confirmed.try_recv() {
        retry.push(PendingMark {
            record,
            warned: false,
        });
    }
}

/// 尝试为所有待标记确认写 `consumed_at`；成功者移出队列并按 applied 输出 stderr 行。
///
/// 失败**不 fail run**（DR-006 §5.3 E-5）：告警一次（首次失败），保留在 `retry` 中等
/// 下一轮重试，`consumed_at` 保持 NULL。
fn flush_confirmed(consumer: &DecisionConsumer, retry: &mut Vec<PendingMark>, quiet: bool) {
    retry.retain_mut(|item| {
        let consumed_at = crate::decide_cmd::now_rfc3339();
        match consumer.mark_consumed(&item.record.run_id, &item.record.instance_id, &consumed_at) {
            Ok(_) => {
                if !quiet {
                    print_decision_line(&item.record);
                }
                false
            }
            Err(error) => {
                if !item.warned {
                    item.warned = true;
                    eprintln!(
                        "warning: cannot mark human decision consumed for instance {}: {} \
                         (will retry; consumed_at stays NULL)",
                        item.record.instance_id,
                        storage_error_detail(&error)
                    );
                }
                true
            }
        }
    });
}

/// stderr 实时决策行（`-q/--quiet` 抑制；`applied=false` 记 warning）。
fn print_decision_line(record: &HumanDecisionRecord) {
    let decision = record.decision.as_str();
    if record.applied {
        eprintln!(
            "human decision applied: instance={} decision={decision} approver={} decision_id={}",
            record.instance_id, record.approver, record.decision_id
        );
    } else {
        eprintln!(
            "warning: human decision not applied: instance={} decision={decision} \
             reject_reason={}",
            record.instance_id,
            record.reject_reason.as_deref().unwrap_or("unknown")
        );
    }
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
#[allow(clippy::too_many_arguments)]
fn finish(
    args: &RunArgs,
    completion: RunCompletion,
    duration_ms: u64,
    db_path: &Path,
    recovered: &[recovery::Recovered],
    warnings: &[String],
    observability: &Observability,
    usage: Option<&RunUsageStats>,
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
            json_payload(
                &outcome,
                event_count,
                duration_ms,
                db_path,
                recovered,
                warnings,
                observability,
                usage,
            )
        );
    } else {
        // 生命周期行已由实时旁路在 run 期间写到 stderr（ISSUE-085）；这里只输出结果。
        println!("{}", run_output_text(&outcome));
        if !args.quiet {
            eprintln!(
                "{}",
                summary_line(&outcome, duration_ms, event_count, exit_code)
            );
            // v0.5/F3b（DR-018 §4）：显式配置可观测/预算时追加 usage 行（无配置输出不变）。
            if observability.enabled() {
                eprintln!("{}", usage_human_line(usage, observability));
            }
        }
        if let Some(error) = &outcome.error {
            eprintln!("error[{}]: {}", error.code, error.message);
        }
        // 未知 usage/价格 warning（不 fail run；`-q` 不抑制诊断告警）。
        if observability.enabled() {
            if let Some(usage) = usage {
                for warning in &usage.warnings {
                    eprintln!("warning: {warning}");
                }
            }
        }
    }
    ExitCode::from(exit_code)
}

/// v0.5/F3b：单维度 usage 合计 → JSON（`UsageTotals`）。
fn usage_totals_json(totals: &UsageTotals) -> Value {
    json!({
        "prompt_tokens": totals.prompt_tokens,
        "completion_tokens": totals.completion_tokens,
        "total_tokens": totals.total_tokens,
        "reasoning_tokens": totals.reasoning_tokens,
        "llm_calls": totals.llm_calls,
        "unknown_usage_calls": totals.unknown_usage_calls,
        "unpriced_calls": totals.unpriced_calls,
        "duration_ms": totals.duration_ms,
        "cost_usd": totals.cost_usd,
    })
}

/// v0.5/F3b：run 结束 usage → `--json.usage`（additive；无配置不插入）。
///
/// `run`/`by_node`/`by_agent` 三维（DR-018 §1）；外部 subprocess harness 的
/// usage/成本不可得 → `unavailable` 标注（不伪造 0）。
fn usage_json(usage: Option<&RunUsageStats>, observability: &Observability) -> Value {
    let Some(stats) = usage else {
        return json!({
            "available": false,
            "reason": if observability.external_harness {
                "external_harness_usage_unavailable"
            } else {
                "no_agent_calls"
            },
        });
    };
    let by_node: Vec<Value> = stats
        .by_node
        .iter()
        .map(|(key, totals)| {
            json!({
                "instance_id": key.instance_id,
                "attempt": key.attempt,
                "usage": usage_totals_json(totals),
            })
        })
        .collect();
    let by_agent: Vec<Value> = stats
        .by_agent
        .iter()
        .map(|(key, totals)| {
            json!({
                "agent_id": key.agent_id,
                "model": key.model,
                "usage": usage_totals_json(totals),
            })
        })
        .collect();
    let mut value = json!({
        "run": usage_totals_json(&stats.run),
        "by_node": by_node,
        "by_agent": by_agent,
        "warnings": stats.warnings,
        "budget_stop": stats
            .budget_stop
            .as_ref()
            .map(zgraph_harness::BudgetStop::to_detail)
            .unwrap_or(Value::Null),
    });
    if observability.external_harness {
        value["unavailable"] = json!("external_harness_usage_unavailable");
    }
    value
}

/// v0.5/F3b：人类可读 usage 行（`-q` 抑制；仅在显式配置时输出）。
fn usage_human_line(usage: Option<&RunUsageStats>, observability: &Observability) -> String {
    let Some(stats) = usage else {
        return if observability.external_harness {
            "usage unavailable (external harness usage is not provided; not counted)".to_string()
        } else {
            "usage unavailable (no agent LLM calls)".to_string()
        };
    };
    let totals = &stats.run;
    let cost = totals
        .cost_usd
        .map(|cost| format!("{cost:.6}"))
        .unwrap_or_else(|| "unavailable".to_string());
    let mut line = format!(
        "usage tokens prompt={} completion={} total={} llm_calls={} duration={}ms cost={cost}",
        totals.prompt_tokens,
        totals.completion_tokens,
        totals.total_tokens,
        totals.llm_calls,
        totals.duration_ms,
    );
    if totals.unknown_usage_calls > 0 {
        line.push_str(&format!(" unknown_usage={}", totals.unknown_usage_calls));
    }
    if observability.external_harness {
        line.push_str(" (external harness usage unavailable; not counted)");
    }
    line
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
#[allow(clippy::too_many_arguments)]
fn json_payload(
    outcome: &RunOutcome,
    event_count: i64,
    duration_ms: u64,
    db_path: &Path,
    recovered: &[recovery::Recovered],
    warnings: &[String],
    observability: &Observability,
    usage: Option<&RunUsageStats>,
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
    object.insert("warnings".to_string(), json!(warnings));
    // v0.5/F3b（DR-018 §4）：显式配置可观测/预算时追加 additive `usage`（默认不输出）。
    if observability.enabled() {
        object.insert("usage".to_string(), usage_json(usage, observability));
    }
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
        // 快照 / L2 记录 / llm_calls / foreach 展开 / 工具审计都不是 `events` 行，
        // 不属于生命周期输出。
        TraceInput::InstanceSnapshot(_)
        | TraceInput::ForeachExpansion(_)
        | TraceInput::ToolCall(_)
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
    eprintln!("error[{}]: {}", error.code(), storage_error_detail(error));
    ExitCode::from(EXIT_STORAGE)
}

/// 关闭写端（失败路径上尽力 flush；错误已由主错误码表达）。
fn close_quietly(writer: TraceWriter) {
    let _ = writer.close();
}

#[cfg(test)]
mod tests {
    use super::{
        exit_code_for, graph_agent_using_mcp, graph_has_human, human_recording, is_memory_db,
        lifecycle_detail, llm_call_recorder, output_hash, parse, resolve_builtin_tools,
        resolve_entry_inputs, run_in_runtime, usage_human_line, usage_json, BudgetConfig, Consumed,
        FakeLlmMode, Observability, ParseState, RecordingExecutor, RunArgs, RunUsageStats,
        UsageTotals, DEFAULT_USER_ID,
    };
    use crate::EXIT_ENTRY_INPUT;
    use serde_json::json;
    use std::future::Future;
    use std::path::{Path, PathBuf};
    use std::pin::Pin;
    use std::process::ExitCode;
    use std::sync::Arc;
    use zgraph_harness::{
        FakeLlmProvider, FakeToolRunner, HarnessExecutor, HarnessSettings, LlmProvider,
    };
    use zgraph_kernel::{
        CompiledGraph, CompiledNode, NodeError, NodeExecution, NodeExecutor, RunError, RunMetrics,
        RunOutcome, RunStatus, CODE_STORAGE_ERROR,
    };
    use zgraph_trace::{
        GateResultRecord, RecordMode, RunFinishedInfo, RunStartedInfo, StreamingEvent, TraceInput,
        TraceWriter,
    };
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
            // kernel additive API 编译适配（Lead 批准；F3a2 stop_reason）。
            stop_reason: None,
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

    /// 单人审批 human 图（`start → review(builtin.human) → end`）。
    fn human_graph_text() -> String {
        json!({
            "version": "1.0",
            "metadata": {"name": "human_gate"},
            "entry_points": ["start"],
            "nodes": [
                {"id": "start", "type": "start", "inputs": [], "config": {}},
                {"id": "review", "type": "python_node",
                 "inputs": [{"name": "payload", "type": "any", "required": true}],
                 "config": {"module": "builtin", "function": "human"}},
                {"id": "end", "type": "end",
                 "inputs": [{"name": "r", "type": "any", "required": true}], "config": {}}
            ],
            "edges": [
                {"from": "start", "to_node": "review", "to_input_port": "payload"},
                {"from": "review", "to_node": "end", "to_input_port": "r"}
            ]
        })
        .to_string()
    }

    fn compiled_graph(text: &str) -> std::sync::Arc<CompiledGraph> {
        let loaded = zgraph_dsl::load_graph(text.as_bytes()).expect("valid dsl");
        CompiledGraph::compile(&loaded).expect("compiled")
    }

    /// W3a：只有含 `builtin.human` 节点的图才需要 decision consumer / 跨进程 trace DB；
    /// 普通 python_node / 无 human 图行为不变。
    #[test]
    fn graph_has_human_detects_builtin_human_nodes_only() {
        assert!(graph_has_human(&compiled_graph(&human_graph_text())));
        assert!(!graph_has_human(&compiled_graph(&graph_text())));
    }

    /// W3a L2：human 实例补录配置仅在「含 human 节点且 `--record≠none`」时注入；
    /// human 节点集合 = 编译产物中的 `CompiledHandler::Human`，mode 取 run options。
    #[test]
    fn human_recording_is_configured_only_for_human_graphs_with_record_mode() {
        let human = compiled_graph(&human_graph_text());
        let full = human_recording(RecordMode::Full, &human).expect("human graph + full");
        assert_eq!(full.mode, RecordMode::Full);
        assert_eq!(
            full.human_nodes.iter().collect::<Vec<_>>(),
            vec![&"review".to_string()]
        );
        let hash = human_recording(RecordMode::Hash, &human).expect("human graph + hash");
        assert_eq!(hash.mode, RecordMode::Hash);

        // 无 human 图 / --record=none → 不注入（行为逐字节不变）。
        assert_eq!(
            human_recording(RecordMode::Full, &compiled_graph(&graph_text())),
            None
        );
        assert_eq!(human_recording(RecordMode::None, &human), None);
    }

    #[test]
    fn parse_accepts_observability_flags() {
        let parsed = parse(&args(&[
            "g.json",
            "--inputs",
            "{}",
            "--pricing",
            "/tmp/pricing.json",
            "--max-tokens",
            "1000",
            "--max-cost",
            "2.5",
        ]))
        .expect("parse")
        .expect("not help");
        assert_eq!(parsed.pricing, Some(PathBuf::from("/tmp/pricing.json")));
        assert_eq!(parsed.max_tokens, Some(1000));
        assert_eq!(parsed.max_cost, Some(2.5));
        // 默认关。
        let parsed = parse(&args(&["g.json", "--inputs", "{}"]))
            .expect("parse")
            .expect("not help");
        assert_eq!(parsed.pricing, None);
        assert_eq!(parsed.max_tokens, None);
        assert_eq!(parsed.max_cost, None);
    }

    #[test]
    fn parse_rejects_invalid_budget_syntax() {
        // 真正语法错 → 用法错误 exit2（负值不是语法错：REV-127 C1）。
        let error = parse(&args(&["g.json", "--inputs", "{}", "--max-tokens", "abc"]))
            .expect_err("invalid budget integer");
        assert!(error.contains("--max-tokens"), "{error}");
        for (flag, value) in [("--max-cost", "x"), ("--max-cost", "--json")] {
            let error = parse(&args(&["g.json", "--inputs", "{}", flag, value]))
                .expect_err("invalid budget float");
            assert!(error.contains(flag) || error.contains("number"), "{error}");
        }
        // 有符号解析：负值进入配置预检（budget_invalid exit3），不在 parse 阶段拒绝。
        let parsed = parse(&args(&["g.json", "--inputs", "{}", "--max-tokens", "-1"]))
            .expect("parse")
            .expect("not help");
        assert_eq!(parsed.max_tokens, Some(-1));
    }

    #[test]
    fn usage_output_marks_external_harness_unavailable() {
        let external = Observability {
            pricing: None,
            budget: BudgetConfig {
                max_tokens: Some(10),
                max_cost: None,
            },
            models: None,
            external_harness: true,
        };
        let line = usage_human_line(None, &external);
        assert!(line.contains("unavailable"), "{line}");
        assert!(line.contains("external harness"), "{line}");
        let json = usage_json(None, &external);
        assert_eq!(
            json["reason"],
            json!("external_harness_usage_unavailable"),
            "{json}"
        );

        let stats = RunUsageStats {
            run: UsageTotals {
                prompt_tokens: 30,
                completion_tokens: 3,
                total_tokens: 33,
                llm_calls: 2,
                unknown_usage_calls: 1,
                duration_ms: 12,
                cost_usd: Some(0.5),
                ..UsageTotals::default()
            },
            ..RunUsageStats::default()
        };
        let line = usage_human_line(Some(&stats), &external);
        assert!(line.contains("prompt=30"), "{line}");
        assert!(line.contains("cost=0.500000"), "{line}");
        assert!(line.contains("unknown_usage=1"), "{line}");
        assert!(
            line.contains("external harness usage unavailable"),
            "{line}"
        );
        let json = usage_json(Some(&stats), &external);
        assert_eq!(json["run"]["llm_calls"], json!(2));
        assert_eq!(
            json["unavailable"],
            json!("external_harness_usage_unavailable")
        );
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
        assert_eq!(parsed.skills_dir, None);
        assert!(!parsed.allow_mcp);
        assert!(!parsed.allow_builtin_tools);
        assert_eq!(parsed.workspace_dir, None);
        assert!(parsed.allow_paths.is_empty());
        assert!(parsed.allow_write_paths.is_empty());
        assert!(!parsed.allow_bash);
        assert!(parsed.exec_paths.is_empty());
        assert!(!parsed.allow_network);
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
            "--skills-dir",
            "skills",
            "--allow-mcp",
            "--allow-builtin-tools",
            "--workspace-dir",
            "ws",
            "--allow-path",
            "ro1",
            "--allow-path",
            "ro2",
            "--allow-write-path",
            "wp1",
            "--allow-write-path",
            "wp2",
            "--allow-bash",
            "--allow-exec-path",
            "exec1",
            "--allow-exec-path",
            "exec2",
            "--allow-network",
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
        assert_eq!(parsed.skills_dir, Some(PathBuf::from("skills")));
        assert!(parsed.allow_mcp);
        assert!(parsed.allow_builtin_tools);
        assert_eq!(parsed.workspace_dir, Some(PathBuf::from("ws")));
        assert_eq!(
            parsed.allow_paths,
            vec![PathBuf::from("ro1"), PathBuf::from("ro2")]
        );
        assert_eq!(
            parsed.allow_write_paths,
            vec![PathBuf::from("wp1"), PathBuf::from("wp2")]
        );
        assert!(parsed.allow_bash);
        assert_eq!(
            parsed.exec_paths,
            vec![PathBuf::from("exec1"), PathBuf::from("exec2")]
        );
        assert!(parsed.allow_network);
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

    /// V3（DR-010 / AC-19 c）：`--allow-mcp` 预检识别图中配置 MCP 的 agent（不启进程）。
    #[test]
    fn preflight_detects_agent_with_mcp() {
        let dir = tempfile::TempDir::new().expect("tempdir");
        std::fs::create_dir_all(dir.path().join("agents")).expect("agents");
        let agent_path = dir.path().join("agents/parser.json");
        std::fs::write(
            &agent_path,
            json!({"id": "parser", "mcp": {"servers": ["fake"]}}).to_string(),
        )
        .expect("agent");
        let graph_text = json!({
            "version": "1.0",
            "metadata": {"name": "mcp-agent"},
            "entry_points": ["start"],
            "nodes": [
                {"id": "start", "type": "start",
                 "inputs": [{"name": "x", "type": "any", "required": true}], "config": {}},
                {"id": "worker", "type": "agent_call",
                 "inputs": [{"name": "x", "type": "any", "required": true}],
                 "config": {"agent_id": "parser"}},
                {"id": "end", "type": "end",
                 "inputs": [{"name": "r", "type": "any", "required": true}], "config": {}}
            ],
            "edges": [
                {"from": "start", "to_node": "worker", "to_input_port": "x"},
                {"from": "worker", "to_node": "end", "to_input_port": "r"}
            ]
        })
        .to_string();
        let graph = compiled_graph(&graph_text);
        assert_eq!(
            graph_agent_using_mcp(&graph, dir.path()),
            Some("parser".to_string())
        );
        // 无 mcp 配置 → None（默认路径不触发预检失败）。
        std::fs::write(&agent_path, json!({"id": "parser"}).to_string()).expect("agent");
        assert!(graph_agent_using_mcp(&graph, dir.path()).is_none());
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
            skills_dir: None,
            allow_mcp: false,
            allow_builtin_tools: false,
            workspace_dir: None,
            allow_paths: Vec::new(),
            allow_write_paths: Vec::new(),
            allow_bash: false,
            exec_paths: Vec::new(),
            allow_network: false,
            python: None,
            python_paths: Vec::new(),
            fake_llm: FakeLlmMode::Off,
            real: false,
            record: RecordMode::Hash,
            stream: false,
            json: false,
            quiet: false,
            pricing: None,
            max_tokens: None,
            max_cost: None,
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

    fn trace_input_run_started() -> TraceInput {
        TraceInput::RunStarted(RunStartedInfo {
            run_id: "r1".to_string(),
            graph_hash: "0123456789abcdef".to_string(),
            graph_name: "demo".to_string(),
            dsl_json: "{}".to_string(),
            user_id: "local".to_string(),
            entry_inputs: json!({}),
            global_vars: json!({}),
            seed: 7,
            record_mode: RecordMode::Hash,
            owner_pid: 1,
            owner_host: "test-host".to_string(),
            started_at: "2026-10-28T09:12:03.104Z".to_string(),
        })
    }

    fn trace_input_gate() -> TraceInput {
        TraceInput::GateResult(GateResultRecord {
            run_id: "r1".to_string(),
            instance_id: "i1".to_string(),
            node_id: "work".to_string(),
            edge_index: 2,
            to_node: "end".to_string(),
            to_port: "r".to_string(),
            gate_fn: Some("gates.ok".to_string()),
            passed: false,
            error: None,
            deferred: false,
            attempt: 0,
            verdict: None,
            ts: "2026-10-28T09:12:03.104Z".to_string(),
        })
    }

    fn trace_input_timed_out() -> TraceInput {
        TraceInput::NodeTimedOut {
            run_id: "r1".to_string(),
            instance_id: "i1".to_string(),
            elapsed_ms: 1500,
            at: "2026-10-28T09:12:03.104Z".to_string(),
        }
    }

    fn trace_input_run_finished() -> TraceInput {
        TraceInput::RunFinished(RunFinishedInfo {
            run_id: "r1".to_string(),
            status: zgraph_trace::RunStatus::Cancelled,
            output_summary: None,
            error: None,
            metrics: zgraph_trace::RunMetrics::default(),
            at: "2026-10-28T09:12:04.104Z".to_string(),
        })
    }

    fn trace_input_streaming(event: StreamingEvent) -> TraceInput {
        TraceInput::Streaming {
            run_id: "r1".to_string(),
            instance_id: "i1".to_string(),
            node_id: Some("work".to_string()),
            event,
            at: "2026-10-28T09:12:03.104Z".to_string(),
        }
    }

    #[test]
    fn lifecycle_detail_formats_control_events() {
        // ISSUE-085：旁路事件 → 实时生命周期行（控制面事件）。
        assert_eq!(
            lifecycle_detail(&trace_input_run_started()).as_deref(),
            Some("RunStarted status=Running user=local")
        );
        assert_eq!(
            lifecycle_detail(&trace_input_gate()).as_deref(),
            Some("GateResult work edge#2 gate=gates.ok passed=false")
        );
        assert_eq!(
            lifecycle_detail(&trace_input_timed_out()).as_deref(),
            Some("NodeTimedOut - elapsed_ms=1500")
        );
        assert_eq!(
            lifecycle_detail(&trace_input_run_finished()).as_deref(),
            Some("RunFinished status=Cancelled")
        );
    }

    #[test]
    fn lifecycle_detail_formats_streaming_and_skips_deltas() {
        // Streaming：progress/tool_call 实时成行；token/reasoning 增量由 `--stream` 负责。
        let progress = trace_input_streaming(StreamingEvent::Progress {
            percent: 42,
            message: "calling".to_string(),
        });
        assert_eq!(
            lifecycle_detail(&progress).as_deref(),
            Some("Streaming work progress 42% (calling)")
        );
        let tool_start = trace_input_streaming(StreamingEvent::ToolCallStart {
            call_id: "c1".to_string(),
            tool_name: "now".to_string(),
            args_summary: json!({}),
        });
        assert_eq!(
            lifecycle_detail(&tool_start).as_deref(),
            Some("Streaming work tool_call_start now")
        );
        let token = trace_input_streaming(StreamingEvent::TokenDelta {
            text: "he".to_string(),
        });
        assert_eq!(lifecycle_detail(&token), None, "token 增量不逐块成行");
    }

    #[test]
    fn parse_state_consumes_options_and_reports_positional() {
        let values = args(&["--json", "graph.json", "-x"]);
        let mut state = ParseState::default();
        let mut iter = values.iter();
        let flag = iter.next().expect("flag arg");
        assert_eq!(
            state.consume(flag, &mut iter).expect("flag consumed"),
            Consumed::Handled
        );
        assert!(state.json);

        let graph = iter.next().expect("graph arg");
        assert_eq!(
            state.consume(graph, &mut iter).expect("positional"),
            Consumed::Positional
        );
        state.set_graph(graph).expect("graph recorded");

        let unknown = iter.next().expect("unknown arg");
        let error = state
            .consume(unknown, &mut iter)
            .expect_err("unknown option");
        assert!(error.contains("unknown option"), "{error}");
    }

    #[test]
    fn parse_state_finish_validates_required_fields_and_conflicts() {
        let error = ParseState::default().finish().expect_err("missing graph");
        assert!(error.contains("run requires <graph.json>"), "{error}");

        let mut state = ParseState::default();
        state.set_graph("graph.json").expect("graph");
        let error = state.finish().expect_err("missing inputs");
        assert!(error.contains("run requires --inputs"), "{error}");

        let mut state = ParseState::default();
        state.set_graph("graph.json").expect("graph");
        state.inputs = Some("{}".to_string());
        state.real = true;
        state.fake_llm = FakeLlmMode::DefaultScript;
        let error = state.finish().expect_err("conflict");
        assert!(error.contains("--real conflicts"), "{error}");
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
    /// v0.4/D2（DR-013 §4）：未 opt-in → 不解析 workspace（`None`）；非法 workspace →
    /// `HARNESS_CONFIG_LOAD` + exit 3（CLI 预检、不产生 run）。
    #[test]
    fn resolve_builtin_tools_respects_opt_in_and_validates_workspace() {
        let mut args = RunArgs {
            graph: PathBuf::from("g.json"),
            inputs: "{}".to_string(),
            user_id: DEFAULT_USER_ID.to_string(),
            db: None,
            assets_dir: None,
            tools_dir: None,
            skills_dir: None,
            allow_mcp: false,
            allow_builtin_tools: false,
            workspace_dir: None,
            allow_paths: Vec::new(),
            allow_write_paths: Vec::new(),
            allow_bash: false,
            exec_paths: Vec::new(),
            allow_network: false,
            python: None,
            python_paths: Vec::new(),
            fake_llm: FakeLlmMode::Off,
            real: false,
            record: RecordMode::Hash,
            stream: false,
            json: false,
            quiet: false,
            pricing: None,
            max_tokens: None,
            max_cost: None,
        };
        assert!(resolve_builtin_tools(&args)
            .expect("not opted in")
            .is_none());

        args.allow_builtin_tools = true;
        let workspace = tempfile::TempDir::new().expect("workspace");
        args.workspace_dir = Some(workspace.path().to_path_buf());
        let tools = resolve_builtin_tools(&args)
            .expect("valid workspace")
            .expect("builtin set");
        assert_eq!(
            tools.workspace_root(),
            std::fs::canonicalize(workspace.path()).expect("canonical")
        );

        args.workspace_dir = Some(workspace.path().join("missing"));
        let error = resolve_builtin_tools(&args).expect_err("missing workspace");
        assert_eq!(error, ExitCode::from(EXIT_ENTRY_INPUT));
    }
    /// v0.4/D4：`--allow-network`/`--allow-exec-path` 必须与 `--allow-bash` 同时出现。
    #[test]
    fn bash_related_flags_require_allow_bash() {
        let error = parse(&args(&["g.json", "--inputs", "{}", "--allow-network"]))
            .expect_err("network without bash");
        assert!(error.contains("--allow-bash"), "{error}");
        let error = parse(&args(&[
            "g.json",
            "--inputs",
            "{}",
            "--allow-exec-path",
            "bin",
        ]))
        .expect_err("exec path without bash");
        assert!(error.contains("--allow-bash"), "{error}");
        let parsed = parse(&args(&[
            "g.json",
            "--inputs",
            "{}",
            "--allow-bash",
            "--allow-exec-path",
            "bin",
            "--allow-network",
        ]))
        .expect("parse")
        .expect("not help");
        assert!(parsed.allow_bash && parsed.allow_network);
        assert_eq!(parsed.exec_paths, vec![PathBuf::from("bin")]);
    }
    /// REV-108 N6：`--allow-bash` 预检失败（profile 渲染失败）→ exit 3、**不创建 DB**、
    /// 不产生 run（预检前移到打开 trace storage 之前）。
    #[tokio::test]
    async fn allow_bash_preflight_failure_exits_3_without_creating_db() {
        let dir = tempfile::TempDir::new().expect("tempdir");
        let db = dir.path().join("trace.db");
        let args = RunArgs {
            graph: dir.path().join("missing-graph.json"),
            inputs: "{}".to_string(),
            user_id: DEFAULT_USER_ID.to_string(),
            db: Some(db.clone()),
            assets_dir: Some(dir.path().to_path_buf()),
            tools_dir: None,
            skills_dir: None,
            allow_mcp: false,
            allow_builtin_tools: false,
            workspace_dir: Some(dir.path().to_path_buf()),
            allow_paths: Vec::new(),
            allow_write_paths: Vec::new(),
            allow_bash: true,
            // 不存在的 exec-path → 渲染期 sandbox_profile_invalid（确定性预检失败）。
            exec_paths: vec![dir.path().join("missing-exec")],
            allow_network: false,
            python: None,
            python_paths: Vec::new(),
            fake_llm: FakeLlmMode::Off,
            real: false,
            record: RecordMode::Hash,
            stream: false,
            json: false,
            quiet: true,
            pricing: None,
            max_tokens: None,
            max_cost: None,
        };
        let code = run_in_runtime(&args).await;
        assert_eq!(code, ExitCode::from(EXIT_ENTRY_INPUT));
        assert!(
            !db.exists(),
            "preflight failure must not create the trace DB"
        );
    }
    /// v0.5/F2：`--allow-write-path` 需要内置工具或 bash opt-in。
    #[test]
    fn allow_write_path_requires_builtin_opt_in() {
        let error = parse(&args(&[
            "g.json",
            "--inputs",
            "{}",
            "--allow-write-path",
            "/tmp/w",
        ]))
        .expect_err("write path without opt-in");
        assert!(error.contains("--allow-write-path"), "{error}");
        let parsed = parse(&args(&[
            "g.json",
            "--inputs",
            "{}",
            "--allow-builtin-tools",
            "--allow-write-path",
            "/tmp/w",
        ]))
        .expect("parse")
        .expect("not help");
        assert_eq!(parsed.allow_write_paths, vec![PathBuf::from("/tmp/w")]);
    }

    /// v0.5/F2：非法可写白名单（`/`、不存在、系统目录）→ 预检 exit 3，且**不创建 DB**。
    #[tokio::test]
    async fn allow_write_path_invalid_exits_3_without_creating_db() {
        let dir = tempfile::TempDir::new().expect("tempdir");
        for (index, bad) in ["/", "/usr", "/System"].iter().enumerate() {
            let db = dir.path().join(format!("trace-{index}.db"));
            let args = RunArgs {
                graph: dir.path().join("missing-graph.json"),
                inputs: "{}".to_string(),
                user_id: DEFAULT_USER_ID.to_string(),
                db: Some(db.clone()),
                assets_dir: Some(dir.path().to_path_buf()),
                tools_dir: None,
                skills_dir: None,
                allow_mcp: false,
                allow_builtin_tools: true,
                workspace_dir: Some(dir.path().to_path_buf()),
                allow_paths: Vec::new(),
                allow_write_paths: vec![PathBuf::from(bad)],
                allow_bash: false,
                exec_paths: Vec::new(),
                allow_network: false,
                python: None,
                python_paths: Vec::new(),
                fake_llm: FakeLlmMode::Off,
                real: false,
                record: RecordMode::Hash,
                stream: false,
                json: false,
                quiet: true,
                pricing: None,
                max_tokens: None,
                max_cost: None,
            };
            let code = run_in_runtime(&args).await;
            assert_eq!(
                code,
                ExitCode::from(EXIT_ENTRY_INPUT),
                "bad write path {bad}"
            );
            assert!(!db.exists(), "bad write path {bad} must not create DB");
        }
        // 不存在的目录同样拒绝。
        let db = dir.path().join("trace-missing.db");
        let args = RunArgs {
            graph: dir.path().join("missing-graph.json"),
            inputs: "{}".to_string(),
            user_id: DEFAULT_USER_ID.to_string(),
            db: Some(db.clone()),
            assets_dir: Some(dir.path().to_path_buf()),
            tools_dir: None,
            skills_dir: None,
            allow_mcp: false,
            allow_builtin_tools: true,
            workspace_dir: Some(dir.path().to_path_buf()),
            allow_paths: Vec::new(),
            allow_write_paths: vec![dir.path().join("missing-dir")],
            allow_bash: false,
            exec_paths: Vec::new(),
            allow_network: false,
            python: None,
            python_paths: Vec::new(),
            fake_llm: FakeLlmMode::Off,
            real: false,
            record: RecordMode::Hash,
            stream: false,
            json: false,
            quiet: true,
            pricing: None,
            max_tokens: None,
            max_cost: None,
        };
        let code = run_in_runtime(&args).await;
        assert_eq!(code, ExitCode::from(EXIT_ENTRY_INPUT));
        assert!(!db.exists());
    }
}
