//! `zgraph replay` 子命令（M5 wave2）：L1 事件回放 / L2 结果回放（DR-002 §3.3）。
//!
//! - `--mode=events`（默认，L1）：复用 [`crate::trace_cmd`] 的只读回放实现与输出格式，
//!   **不重跑、不写库**；
//! - `--mode=tools`（L3，v0.4/D5b2）：只读 `tool_calls` 还原工具调用（recorded-only，
//!   **绝不执行任何工具/副作用**；`--replay-tools=reexec` 为 v0.5 接口 → v0.4 用法错误）；
//! - `--mode=outputs`（L2）：只读打开 DB，校验 `--record=full` 记录完整性后，用
//!   [`RecordedExecutor`] 重跑内核（不启动 worker/LLM、内存 trace sink 不落库），
//!   比较重跑输出与原始 `runs.output_json`。成功 exit 0，不一致
//!   `REPLAY_OUTPUT_MISMATCH` exit 1；记录缺失/不完整/损坏 exit 3
//!   （CLI 命名空间，不写 `runs.error_code`）；存储/run 不存在 exit 4。
//!
//! **不写新 run、不修改原 trace**：所有 DB 访问走 [`TraceReader::open_readonly`]。
//!
//! 重放映射依据 `(node_id, attempt)`：重跑时内核重新生成 `instance_id`（UUIDv7，不可
//! 复用）；内核语义保证 `(node_id, attempt)` 在 run 内唯一（非自环节点 attempt=0，
//! 自环每次投递 attempt+1；ISSUE-061 修复后被 gate 拦截的投递不创建实例）。
//! `start`/`end` 为内核内置执行、不进入执行器，故不查记录。
//!
//! `global_vars`（ISSUE-087 / schema v3）：由 `runs.global_vars_json` 重建运行环境；
//! v3 前的历史 run 该列为 NULL → 按空 map 兼容并在 stderr 告警（旧行为；依赖
//! `ctx.get_var` 的图可能 `REPLAY_OUTPUT_MISMATCH`）。`replay` 只读、**不迁移** schema
//! （06 §6.6/§6.7）：v2 库需先由写端（`zgraph run`）迁移到 v3，否则按
//! `error[STORAGE_ERROR]` 退出码 4。`llm_calls` 仅作审计，不参与重放。
//!
//! # human 节点（W3a / DR-006 §2.2/§8 L2）
//!
//! human 实例由内核内置完成、不经 [`NodeExecutor`]；若按 `CompiledHandler::Human`
//! 重跑，内核会再次进入 `WaitingHuman` 等待决策（且新 `instance_id` 由内核生成，
//! CLI 无法经 `RunControl` 注入）。L2 冻结口径是「human 实例输出按
//! `(node_id, attempt)` 记录，重放**不重新要求人工决策**」——因此重跑前把 recorded
//! graph 中的 human 节点替换为可执行 handler（[`replay_recorded_human_graph`]），
//! 由 [`RecordedExecutor`] 直接返回记录输出；不启动 worker、不 solicit 人工，
//! publish/gate/end 聚合与原 run 一致。无 human 节点的图原样重跑（零变化）。
//!
//! # foreach item（W4b / DR-007 §8.5）
//!
//! L2 记录查找键扩展为 `(node_id, attempt, item_index)`（`item_index` 由 `instances`
//! 表提供；`recorded_outputs` 结构不变）。重跑前按 `foreach_expansions` 的
//! `item_count`/`keys` 校验 per-item 记录完整性（不匹配 → `REPLAY_RECORD_CORRUPT`；
//! 已展开但缺 expansion → `REPLAY_RECORD_MISSING`）。item 输出按记录重放，内核重新
//! 展开（同一 entry/上游数据 → 同一 N/keys），不启动 worker、不重新要求人工决策。

use std::collections::{BTreeMap, HashMap};
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::process::ExitCode;
use std::sync::Arc;

use serde_json::{json, Map, Value};

use zgraph_dsl::load_graph;
use zgraph_kernel::{
    run_to_completion, CompiledGraph, CompiledHandler, MemoryTraceSink, NodeError, NodeExecution,
    NodeExecutor, NodeId, PortName, RunOptions, RunOutcome, RunSpec, RunStatus,
};
use zgraph_trace::{
    ForeachExpansionRow, InstanceRow, InstanceStatus as TraceInstanceStatus, RecordedOutputRow,
    RunRow, RunStatus as TraceRunStatus, StreamingRow, ToolCallRow, TraceError, TraceReader,
};

use crate::run_cmd::output_hash;
use crate::trace_cmd::{self, resolve_db_path, TraceOptions};
use crate::{EXIT_OTHER, EXIT_STORAGE, EXIT_SUCCESS};

/// `replay --help` 的用法文本。
pub const REPLAY_USAGE: &str = "\
usage: zgraph replay --run <run_id> [options]

options:
  --run <run_id>        run to replay
  --mode <mode>         events (L1, default) | outputs (L2) | tools (recorded tool calls)
  --replay-tools <mode> recorded (default; v0.4 recorded-only) | reexec (v0.4 未实现)
  --db <path>           trace database (default: $ZGRAPH_DB or .zgraph/trace.db)
  --json                machine-readable output (events/tools: NDJSON; outputs: JSON object)
  --help, -h            print this help";

/// `REPLAY_RECORD_MISSING`：`--record=full` 记录缺失/不完整（DR-002 §3.3）。
pub const CODE_REPLAY_RECORD_MISSING: &str = "REPLAY_RECORD_MISSING";

/// `REPLAY_RUN_NOT_COMPLETED`：原始 run 非 `Completed`，不支持 L2 重放。
pub const CODE_REPLAY_RUN_NOT_COMPLETED: &str = "REPLAY_RUN_NOT_COMPLETED";

/// `REPLAY_RECORD_CORRUPT`：记录 hash 复核失败 / 记录快照不可用。
pub const CODE_REPLAY_RECORD_CORRUPT: &str = "REPLAY_RECORD_CORRUPT";

/// `REPLAY_OUTPUT_MISMATCH`：L2 重跑结果与原始输出不一致。
pub const CODE_REPLAY_OUTPUT_MISMATCH: &str = "REPLAY_OUTPUT_MISMATCH";

/// `REPLAY_TOOL_RECORD_MISSING`：`--mode=tools` 缺 `tool_calls` 记录（CLI 命名空间）。
pub const CODE_REPLAY_TOOL_RECORD_MISSING: &str = "REPLAY_TOOL_RECORD_MISSING";

/// `REPLAY_TOOL_RECORD_TRUNCATED`：`tool_calls` 记录被截断，不能安全回放（fail-closed）。
pub const CODE_REPLAY_TOOL_RECORD_TRUNCATED: &str = "REPLAY_TOOL_RECORD_TRUNCATED";

/// `REPLAY_TOOL_RECORD_MISMATCH`：`tool_calls` 记录 JSON 不合法 / 全量 sha256 不符。
pub const CODE_REPLAY_TOOL_RECORD_MISMATCH: &str = "REPLAY_TOOL_RECORD_MISMATCH";

/// replay 的 CLI 命名空间记录类错误退出码（DR-002 §3.3）。
const EXIT_RECORD: u8 = 3;

/// L2 只重放的节点类型（`start`/`end` 为内核内置执行，不查记录；DR-002 §3.3）。
const EXECUTED_NODE_TYPES: [&str; 2] = ["python_node", "agent_call"];

/// 差异摘要中 JSON 文本的最大字符数（避免刷屏）。
const DIFF_TEXT_LIMIT: usize = 512;

/// replay 模式。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReplayMode {
    /// L1：事件回放（复用 `zgraph trace`）。
    Events,
    /// L2：按记录输出重跑。
    Outputs,
    /// L3：工具级回放（`tool_calls` recorded；绝不执行任何工具）。
    Tools,
}

/// `replay` 的参数。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplayArgs {
    /// 要重放的 run id。
    pub run_id: String,
    /// 重放模式。
    pub mode: ReplayMode,
    /// `--db` 显式库路径。
    pub db: Option<PathBuf>,
    /// `--json`。
    pub json: bool,
}

/// 解析 `replay` 之后的参数；`--help`/`-h` 返回 `Ok(None)`。
pub fn parse(args: &[String]) -> Result<Option<ReplayArgs>, String> {
    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        return Ok(None);
    }
    let mut run_id = None;
    let mut mode = ReplayMode::Events;
    let mut db = None;
    let mut json = false;
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        if let Some(value) = arg.strip_prefix("--run=") {
            run_id = Some(non_empty(value, "--run")?);
            continue;
        }
        if let Some(value) = arg.strip_prefix("--mode=") {
            mode = parse_mode(value)?;
            continue;
        }
        if let Some(value) = arg.strip_prefix("--replay-tools=") {
            parse_replay_tools(value)?;
            continue;
        }
        match arg.as_str() {
            "--run" => run_id = Some(option_value(&mut iter, "--run")?),
            "--mode" => mode = parse_mode(&option_value(&mut iter, "--mode")?)?,
            "--replay-tools" => {
                let value = option_value(&mut iter, "--replay-tools")?;
                parse_replay_tools(&value)?;
            }
            "--db" => db = Some(PathBuf::from(option_value(&mut iter, "--db")?)),
            "--json" => json = true,
            flag if flag.starts_with('-') => return Err(format!("unknown option: {flag}")),
            value => return Err(format!("unexpected argument: {value}")),
        }
    }
    let run_id = run_id.ok_or_else(|| "replay requires --run <run_id>".to_string())?;
    Ok(Some(ReplayArgs {
        run_id,
        mode,
        db,
        json,
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

/// 非空字符串参数。
fn non_empty(value: &str, option: &str) -> Result<String, String> {
    if value.is_empty() {
        return Err(format!("{option} requires a non-empty value"));
    }
    Ok(value.to_string())
}

/// 解析 `--mode`。
fn parse_mode(value: &str) -> Result<ReplayMode, String> {
    match value {
        "events" => Ok(ReplayMode::Events),
        "outputs" => Ok(ReplayMode::Outputs),
        "tools" => Ok(ReplayMode::Tools),
        other => Err(format!(
            "invalid --mode '{other}' (expected events|outputs|tools)"
        )),
    }
}

/// 解析 `--replay-tools`（v0.4 只实现 `recorded`；`reexec` 为 v0.5 接口 → 用法错误）。
fn parse_replay_tools(value: &str) -> Result<(), String> {
    match value {
        "recorded" => Ok(()),
        "reexec" => Err(
            "--replay-tools=reexec is not implemented in v0.4 (recorded-only replay); \
             use --replay-tools=recorded"
                .to_string(),
        ),
        other => Err(format!(
            "invalid --replay-tools '{other}' (expected recorded|reexec)"
        )),
    }
}

/// 执行 `replay` 并返回进程退出码。
pub fn run(args: &ReplayArgs) -> ExitCode {
    match args.mode {
        ReplayMode::Events => run_events(args),
        ReplayMode::Tools => run_tools(args),
        ReplayMode::Outputs => match tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
        {
            Ok(runtime) => runtime.block_on(run_outputs(args)),
            Err(error) => {
                eprintln!("error: cannot start tokio runtime: {error}");
                ExitCode::from(EXIT_OTHER)
            }
        },
    }
}

/// L3：工具级 recorded 回放（v0.4/D5b2；DR-014 §4/§7）。
///
/// 只读 `tool_calls` 还原并按 `(started_at, call_id)` 输出；**绝不执行任何工具**
/// （无 spawn、无 workspace 写入、不新增 `tool_calls` 行）。fail-closed 校验：
/// `streaming tool_call_start` 计数 > 记录行数 → `REPLAY_TOOL_RECORD_MISSING`；
/// 任一行 `truncated=1` → `REPLAY_TOOL_RECORD_TRUNCATED`；args/result/error JSON
/// 不可解析或全量 sha256 不符 → `REPLAY_TOOL_RECORD_MISMATCH`（均 CLI 码、exit 3，
/// 不写 `runs.error_code`）。
fn run_tools(args: &ReplayArgs) -> ExitCode {
    let db_path = resolve_db_path(args.db.as_deref());
    let reader = match TraceReader::open_readonly(&db_path) {
        Ok(reader) => reader,
        Err(error) => return report(ReplayError::from(error)),
    };
    let run = match reader.run(&args.run_id) {
        Ok(Some(run)) => run,
        Ok(None) => {
            return report(ReplayError::storage(format!(
                "run not found: {}",
                args.run_id
            )))
        }
        Err(error) => return report(ReplayError::from(error)),
    };
    let calls = match reader.tool_calls(&run.run_id) {
        Ok(calls) => calls,
        Err(error) => return report(ReplayError::from(error)),
    };
    let streaming = match reader.streaming_events(&run.run_id) {
        Ok(streaming) => streaming,
        Err(error) => return report(ReplayError::from(error)),
    };
    if let Err(error) = validate_tool_calls(&calls, &streaming) {
        return report(error);
    }
    if args.json {
        for call in &calls {
            println!("{}", trace_cmd::tool_call_json_value(call));
        }
    } else {
        println!(
            "run {}  status={}  graph={}  hash={}",
            run.run_id,
            run.status.as_str(),
            run.graph_name.as_deref().unwrap_or("-"),
            run.graph_hash
        );
        println!("tool_calls: {} (recorded; no tools executed)", calls.len());
        for call in &calls {
            println!("  {}", trace_cmd::tool_call_human_line(call));
        }
    }
    ExitCode::from(EXIT_SUCCESS)
}

/// `--mode=tools` 的 fail-closed 校验（顺序：missing → truncated → mismatch）。
fn validate_tool_calls(
    calls: &[ToolCallRow],
    streaming: &[StreamingRow],
) -> Result<(), ReplayError> {
    let starts = streaming
        .iter()
        .filter(|row| row.kind == "tool_call_start")
        .count();
    if starts > calls.len() {
        return Err(ReplayError::record(
            CODE_REPLAY_TOOL_RECORD_MISSING,
            format!(
                "run has {starts} streaming tool_call_start event(s) but only {} tool_calls \
                 record(s) (recorded replay is fail-closed)",
                calls.len()
            ),
        ));
    }
    if let Some(call) = calls.iter().find(|call| call.truncated) {
        return Err(ReplayError::record(
            CODE_REPLAY_TOOL_RECORD_TRUNCATED,
            format!(
                "tool_calls row call_id='{}' node='{}' is truncated (truncated=1); \
                 recorded replay is fail-closed",
                call.call_id, call.node_id
            ),
        ));
    }
    for call in calls {
        verify_tool_json("args", &call.args_json, call.args_sha256.as_deref(), call)?;
        if let Some(raw) = &call.result_json {
            verify_tool_json("result", raw, call.result_sha256.as_deref(), call)?;
        }
        if let Some(raw) = &call.error_json {
            verify_tool_json("error", raw, call.error_sha256.as_deref(), call)?;
        }
    }
    Ok(())
}

/// 单个 JSON 字段的可解析性 + 全量 sha256 复核（fail-closed → MISMATCH）。
fn verify_tool_json(
    field: &str,
    raw: &str,
    sha: Option<&str>,
    call: &ToolCallRow,
) -> Result<(), ReplayError> {
    serde_json::from_str::<Value>(raw)
        .map_err(|error| tool_mismatch(call, field, format!("is not valid JSON: {error}")))?;
    let expected = sha.ok_or_else(|| {
        tool_mismatch(
            call,
            field,
            format!("has no {field}_sha256 (cannot verify recorded content)"),
        )
    })?;
    let actual = sha256_hex(raw.as_bytes());
    if actual != expected {
        return Err(tool_mismatch(
            call,
            field,
            format!("sha256 mismatch recorded={expected} recomputed={actual}"),
        ));
    }
    Ok(())
}

/// `REPLAY_TOOL_RECORD_MISMATCH` 错误（CLI 命名空间，exit 3）。
fn tool_mismatch(call: &ToolCallRow, field: &str, detail: String) -> ReplayError {
    ReplayError::record(
        CODE_REPLAY_TOOL_RECORD_MISMATCH,
        format!(
            "tool_calls row call_id='{}' node='{}' field {field} {detail}",
            call.call_id, call.node_id
        ),
    )
}

/// 小写 hex sha256（与 trace writer 的 `args_sha256`/`result_sha256` 同口径）。
fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(bytes);
    let mut out = String::with_capacity(64);
    for byte in digest {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

/// L1：委托 [`crate::trace_cmd`] 的默认时间线/NDJSON 回放（语义与退出码完全一致）。
fn run_events(args: &ReplayArgs) -> ExitCode {
    let options = TraceOptions {
        run_id: args.run_id.clone(),
        db: args.db.clone(),
        json: args.json,
        since_seq: 0,
        limit: trace_cmd::DEFAULT_LIMIT,
        kinds: None,
        no_streaming: false,
        stats: false,
        quiet: false,
    };
    trace_cmd::run(&options)
}

// ---------------------------------------------------------------------------
// L2：记录输出重放
// ---------------------------------------------------------------------------

/// replay 错误（CLI 命名空间码 + 退出码）。
#[derive(Debug)]
struct ReplayError {
    code: &'static str,
    message: String,
    exit_code: u8,
}

impl ReplayError {
    /// 记录类错误（exit 3）。
    fn record(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            exit_code: EXIT_RECORD,
        }
    }

    /// 存储/IO 类错误（exit 4）。
    fn storage(message: impl Into<String>) -> Self {
        Self {
            code: "STORAGE_ERROR",
            message: message.into(),
            exit_code: EXIT_STORAGE,
        }
    }
}

impl From<TraceError> for ReplayError {
    fn from(error: TraceError) -> Self {
        let code = error.code();
        let message = match error {
            TraceError::SchemaTooNew { found, supported } => {
                format!("db version={found}, supported={supported}")
            }
            TraceError::Storage(message)
            | TraceError::NotFound(message)
            | TraceError::Serialize(message) => message,
        };
        Self {
            code,
            message,
            exit_code: EXIT_STORAGE,
        }
    }
}

/// 打印 `error[CODE]: message` 并返回退出码。
fn report(error: ReplayError) -> ExitCode {
    eprintln!("error[{}]: {}", error.code, error.message);
    ExitCode::from(error.exit_code)
}

/// L2 记录查找键：`(node_id, attempt, item_index)`（W4b / DR-007 §8.5）。
///
/// `item_index` 由 `instances` 表提供（`recorded_outputs` 表结构不变）；非 foreach
/// 实例恒 0，因此无 foreach 的既有记录键与 `(node_id, attempt)` 一一对应（golden 不变）。
type RecordKey = (String, u32, u32);

/// L2 预检产物。
struct ReplayPlan {
    /// `(node_id, attempt, item_index) -> recorded output.json`。
    outputs: HashMap<RecordKey, Value>,
    /// 记录行数（摘要用）。
    recorded: usize,
    /// 原 run 的实例行（`foreach_expansions` 完整性预检用）。
    instances: Vec<InstanceRow>,
}

/// 以记录输出为唯一数据源的执行器（不启动 worker/LLM）。
struct RecordedExecutor {
    outputs: HashMap<RecordKey, Value>,
}

impl NodeExecutor for RecordedExecutor {
    fn execute<'a>(
        &'a self,
        execution: NodeExecution<'a>,
    ) -> Pin<Box<dyn Future<Output = Result<Value, NodeError>> + Send + 'a>> {
        let key = (
            execution.node.id.clone(),
            execution.ctx.attempt(),
            execution.ctx.item_index(),
        );
        let result = match self.outputs.get(&key) {
            Some(output) => Ok(output.clone()),
            None => Err(NodeError::Internal {
                message: format!(
                    "replay: no recorded output for node '{}' attempt {} item {}",
                    key.0, key.1, key.2
                ),
            }),
        };
        Box::pin(async move { result })
    }
}

/// L2 主流程（只读 DB → 预检 → 重跑 → 比较输出）。
async fn run_outputs(args: &ReplayArgs) -> ExitCode {
    let db_path = resolve_db_path(args.db.as_deref());
    let reader = match TraceReader::open_readonly(&db_path) {
        Ok(reader) => reader,
        Err(error) => return report(ReplayError::from(error)),
    };
    let run = match reader.run(&args.run_id) {
        Ok(Some(run)) => run,
        Ok(None) => {
            return report(ReplayError::storage(format!(
                "run not found: {}",
                args.run_id
            )))
        }
        Err(error) => return report(ReplayError::from(error)),
    };
    let plan = match build_plan(&reader, &run) {
        Ok(plan) => plan,
        Err(error) => return report(error),
    };
    let graph = match load_recorded_graph(&reader, &run) {
        Ok(graph) => graph,
        Err(error) => return report(error),
    };
    // W4b（DR-007 §8.5）：按 `foreach_expansions` 的 item_count/keys 校验 per-item
    // 记录完整性（缺失 → REPLAY_RECORD_MISSING；不匹配 → REPLAY_RECORD_CORRUPT）。
    if let Err(error) = validate_foreach_expansions(&reader, &run, &graph, &plan.instances) {
        return report(error);
    }
    let graph = replay_recorded_human_graph(graph);
    let spec = match build_spec(&run, graph) {
        Ok(spec) => spec,
        Err(error) => return report(error),
    };
    let executor = Arc::new(RecordedExecutor {
        outputs: plan.outputs.clone(),
    });
    let sink = Arc::new(MemoryTraceSink::new());
    let outcome = match run_to_completion(None, spec, executor, sink).await {
        Ok(outcome) => outcome,
        Err(error) => {
            return report(ReplayError::record(
                CODE_REPLAY_RECORD_CORRUPT,
                format!("replayed run could not start: {error}"),
            ))
        }
    };
    finish_outputs(args, &run, &outcome, plan.recorded)
}

/// 预检：record_mode/status/记录完整性与 hash 复核（DR-002 §3.3 步 2；W4b per-item）。
fn build_plan(reader: &TraceReader, run: &RunRow) -> Result<ReplayPlan, ReplayError> {
    ensure_replayable(run.record_mode.as_deref(), run.status)?;
    let instances = reader.instances(&run.run_id)?;
    let instance_item_index = instance_item_indexes(&instances);
    let records = reader.recorded_outputs(&run.run_id)?;
    let outputs = index_recorded_outputs(&records, &instance_item_index)?;
    ensure_executed_nodes_recorded(&instances, &outputs)?;
    Ok(ReplayPlan {
        outputs,
        recorded: records.len(),
        instances,
    })
}

/// `instance_id -> item_index`（`recorded_outputs` 不含 item_index，由 instances 提供）。
fn instance_item_indexes(instances: &[InstanceRow]) -> HashMap<String, u32> {
    instances
        .iter()
        .map(|instance| (instance.instance_id.clone(), instance.item_index))
        .collect()
}

/// 前置校验：原 run 必须 `--record=full` 且状态 `Completed`（DR-002 §3.3 步 2）。
fn ensure_replayable(record_mode: Option<&str>, status: TraceRunStatus) -> Result<(), ReplayError> {
    let record_mode = record_mode.unwrap_or("unknown");
    if record_mode != "full" {
        return Err(ReplayError::record(
            CODE_REPLAY_RECORD_MISSING,
            format!(
                "run was recorded with mode '{record_mode}'; --mode=outputs requires --record=full"
            ),
        ));
    }
    if status != TraceRunStatus::Completed {
        return Err(ReplayError::record(
            CODE_REPLAY_RUN_NOT_COMPLETED,
            format!(
                "run status is {}; only Completed runs can be replayed",
                status.as_str()
            ),
        ));
    }
    Ok(())
}

/// 把 `recorded_outputs` 行建成 `(node_id, attempt, item_index) -> output` 索引，
/// 并复核 `output_json` 存在性、hash 一致性与重复记录（DR-002 §3.3 步 2；W4b per-item）。
///
/// `recorded_outputs` 表结构不变（唯一键含 `instance_id`），`item_index` 由
/// `instances` 表按 `instance_id` 提供；记录引用未知实例 → `REPLAY_RECORD_CORRUPT`。
fn index_recorded_outputs(
    records: &[RecordedOutputRow],
    instance_item_index: &HashMap<String, u32>,
) -> Result<HashMap<RecordKey, Value>, ReplayError> {
    let mut outputs: HashMap<RecordKey, Value> = HashMap::new();
    for record in records {
        let output = record.output_json.as_ref().ok_or_else(|| {
            ReplayError::record(
                CODE_REPLAY_RECORD_CORRUPT,
                format!(
                    "recorded output for node '{}' attempt {} has no output_json (full record required)",
                    record.node_id, record.attempt
                ),
            )
        })?;
        let item_index = instance_item_index
            .get(&record.instance_id)
            .copied()
            .ok_or_else(|| {
                ReplayError::record(
                    CODE_REPLAY_RECORD_CORRUPT,
                    format!(
                        "recorded output references unknown instance '{}' (node '{}' attempt {})",
                        record.instance_id, record.node_id, record.attempt
                    ),
                )
            })?;
        let recomputed = output_hash(output);
        if recomputed != record.output_hash {
            return Err(ReplayError::record(
                CODE_REPLAY_RECORD_CORRUPT,
                format!(
                    "recorded output hash mismatch for node '{}' attempt {} item {item_index}: \
                     recorded={} recomputed={recomputed}",
                    record.node_id, record.attempt, record.output_hash
                ),
            ));
        }
        let key = (record.node_id.clone(), record.attempt, item_index);
        if outputs.insert(key.clone(), output.clone()).is_some() {
            return Err(ReplayError::record(
                CODE_REPLAY_RECORD_CORRUPT,
                format!(
                    "duplicate recorded output for node '{}' attempt {} item {}",
                    key.0, key.1, key.2
                ),
            ));
        }
    }
    Ok(outputs)
}

/// 完整性预检：已执行且 `Completed` 的 `python_node`/`agent_call` 必须有记录
/// （DR-002 §3.3 步 2；W4b 查找键含 per-item `item_index`）。
fn ensure_executed_nodes_recorded(
    instances: &[InstanceRow],
    outputs: &HashMap<RecordKey, Value>,
) -> Result<(), ReplayError> {
    for instance in instances {
        if !EXECUTED_NODE_TYPES.contains(&instance.node_type.as_str())
            || instance.status != TraceInstanceStatus::Completed
        {
            continue;
        }
        let key = (
            instance.node_id.clone(),
            instance.attempt,
            instance.item_index,
        );
        if !outputs.contains_key(&key) {
            return Err(ReplayError::record(
                CODE_REPLAY_RECORD_MISSING,
                format!(
                    "missing recorded output for executed node '{}' attempt {} item {} \
                     (status=Completed)",
                    key.0, key.1, key.2
                ),
            ));
        }
    }
    Ok(())
}

/// L2 foreach 完整性预检（W4b / DR-007 §8.5）：`foreach_expansions` 的
/// `item_count`/`keys` 必须与 item 实例一致。
///
/// - expansion 引用未知节点/非 foreach 节点、同一 F 多行、`item_count` 与
///   item_index 集合不一致、`keys` 与实例 `item_key` 不一致 → `REPLAY_RECORD_CORRUPT`；
/// - 图内 foreach 节点已有 region 实例但缺 expansion 行 → `REPLAY_RECORD_MISSING`
///   （controller 被 Skipped、region 从未展开时无实例也无 expansion，属正常）。
fn validate_foreach_expansions(
    reader: &TraceReader,
    run: &RunRow,
    graph: &CompiledGraph,
    instances: &[InstanceRow],
) -> Result<(), ReplayError> {
    let expansions = reader.foreach_expansions(&run.run_id)?;
    validate_foreach_records(&expansions, graph, instances)
}

/// [`validate_foreach_expansions`] 的纯函数核心（单测可直接构造行/图）。
fn validate_foreach_records(
    expansions: &[ForeachExpansionRow],
    graph: &CompiledGraph,
    instances: &[InstanceRow],
) -> Result<(), ReplayError> {
    let mut by_node: BTreeMap<&str, &ForeachExpansionRow> = BTreeMap::new();
    for expansion in expansions {
        if by_node
            .insert(expansion.node_id.as_str(), expansion)
            .is_some()
        {
            return Err(ReplayError::record(
                CODE_REPLAY_RECORD_CORRUPT,
                format!(
                    "duplicate foreach_expansions row for node '{}' (single expansion expected)",
                    expansion.node_id
                ),
            ));
        }
    }
    for (node_id, expansion) in &by_node {
        let node = graph.nodes.get(*node_id).ok_or_else(|| {
            ReplayError::record(
                CODE_REPLAY_RECORD_CORRUPT,
                format!("foreach expansion references unknown node '{node_id}'"),
            )
        })?;
        if node.foreach.is_none() {
            return Err(ReplayError::record(
                CODE_REPLAY_RECORD_CORRUPT,
                format!("foreach expansion references non-foreach node '{node_id}'"),
            ));
        }
        let f_instances: Vec<&InstanceRow> = instances
            .iter()
            .filter(|instance| instance.node_id == *node_id)
            .collect();
        validate_one_expansion(expansion, &f_instances)?;
    }
    // 图内 foreach 节点：region 已有实例却无 expansion 行 → 记录缺失。
    for node in graph.nodes.values().filter(|node| node.foreach.is_some()) {
        if by_node.contains_key(node.id.as_str()) {
            continue;
        }
        let Some(config) = &node.foreach else {
            continue;
        };
        let region_has_instance = instances.iter().any(|instance| {
            config
                .region
                .iter()
                .any(|region_node| region_node == &instance.node_id)
        });
        if region_has_instance {
            return Err(ReplayError::record(
                CODE_REPLAY_RECORD_MISSING,
                format!(
                    "foreach node '{}' has item instances but no foreach_expansions row",
                    node.id
                ),
            ));
        }
    }
    Ok(())
}

/// 校验单次展开：`item_count` ↔ F 的 item_index 集合、`keys` ↔ item_key。
fn validate_one_expansion(
    expansion: &ForeachExpansionRow,
    f_instances: &[&InstanceRow],
) -> Result<(), ReplayError> {
    let expected_count = usize::try_from(expansion.item_count).map_err(|_| {
        ReplayError::record(
            CODE_REPLAY_RECORD_CORRUPT,
            format!(
                "foreach expansion of node '{}' item_count {} overflows usize",
                expansion.node_id, expansion.item_count
            ),
        )
    })?;
    let keys = expansion
        .keys
        .as_ref()
        .and_then(Value::as_array)
        .ok_or_else(|| {
            ReplayError::record(
                CODE_REPLAY_RECORD_CORRUPT,
                format!(
                    "foreach expansion of node '{}' has no keys array",
                    expansion.node_id
                ),
            )
        })?;
    if keys.len() != expected_count {
        return Err(ReplayError::record(
            CODE_REPLAY_RECORD_CORRUPT,
            format!(
                "foreach expansion of node '{}' keys length {} != item_count {}",
                expansion.node_id,
                keys.len(),
                expected_count
            ),
        ));
    }
    // F 的 item 实例（可能多 attempt）：按 item_index 建索引，取每个 index 的首个实例。
    let mut by_index: BTreeMap<u32, &InstanceRow> = BTreeMap::new();
    for instance in f_instances {
        by_index.entry(instance.item_index).or_insert(instance);
    }
    let actual_indexes: Vec<u32> = by_index.keys().copied().collect();
    let expected_indexes: Vec<u32> =
        (0..u32::try_from(expected_count).unwrap_or(u32::MAX)).collect();
    if actual_indexes != expected_indexes {
        return Err(ReplayError::record(
            CODE_REPLAY_RECORD_CORRUPT,
            format!(
                "foreach expansion of node '{}' item_count {} does not match item instances \
                 {actual_indexes:?}",
                expansion.node_id, expansion.item_count
            ),
        ));
    }
    for (index, key) in keys.iter().enumerate() {
        let instance = by_index
            .get(&u32::try_from(index).unwrap_or(u32::MAX))
            .expect("index set validated above");
        let expected_key = match key {
            Value::String(key) => Some(key.as_str()),
            // NULL 键的冻结表示：单键对象 `{"$index": i}`（真实 string key 不与它撞）。
            Value::Object(map)
                if map.len() == 1
                    && map.get("$index").and_then(Value::as_u64) == Some(index as u64) =>
            {
                None
            }
            other => {
                return Err(ReplayError::record(
                    CODE_REPLAY_RECORD_CORRUPT,
                    format!(
                        "foreach expansion of node '{}' has invalid keys[{index}]={other}",
                        expansion.node_id
                    ),
                ))
            }
        };
        if instance.item_key.as_deref() != expected_key {
            return Err(ReplayError::record(
                CODE_REPLAY_RECORD_CORRUPT,
                format!(
                    "foreach expansion of node '{}' keys[{index}]={key} does not match \
                     instance item_key {:?}",
                    expansion.node_id, instance.item_key
                ),
            ));
        }
    }
    Ok(())
}

/// 读 `runs.graph_hash` 对应的 `graphs.dsl_json` 并编译（不校验 agent 资产，R14c 跳过）。
fn load_recorded_graph(
    reader: &TraceReader,
    run: &RunRow,
) -> Result<Arc<CompiledGraph>, ReplayError> {
    let graph_row = reader.graph(&run.graph_hash)?.ok_or_else(|| {
        ReplayError::storage(format!(
            "graph definition '{}' not found for run {}",
            run.graph_hash, run.run_id
        ))
    })?;
    let loaded = load_graph(graph_row.dsl_json.as_bytes()).map_err(|_| {
        ReplayError::storage(format!(
            "recorded graph definition for run {} is invalid",
            run.run_id
        ))
    })?;
    CompiledGraph::compile(&loaded)
        .map_err(|error| ReplayError::storage(format!("cannot compile recorded graph: {error}")))
}

/// 把 recorded graph 中的 human 节点替换为「按记录输出执行」的可执行 handler
/// （W3a / DR-006 §2.2/§8 L2）。
///
/// 原因：`CompiledHandler::Human` 由内核内置完成、不经 [`NodeExecutor`]，按原样重跑
/// 会再次进入 `WaitingHuman` 等决策（且新 `instance_id` 由内核生成，CLI 无法注入）。
/// L2 的冻结口径是「human 实例输出按 `(node_id, attempt)` 记录，重放不重新要求人工
/// 决策」：把 human 节点在本地产物里改为普通 Python handler，使
/// [`RecordedExecutor`] 直接返回该记录输出——不启动 worker、不 solicit 人工，
/// 后续 publish/gate/end 聚合与原始 run 一致。无 human 节点时原样返回（零变化）。
fn replay_recorded_human_graph(graph: Arc<CompiledGraph>) -> Arc<CompiledGraph> {
    let has_human = graph
        .nodes
        .values()
        .any(|node| matches!(node.handler, CompiledHandler::Human));
    if !has_human {
        return graph;
    }
    let mut replay = CompiledGraph::clone(&graph);
    for node in replay.nodes.values_mut() {
        if matches!(node.handler, CompiledHandler::Human) {
            node.handler = CompiledHandler::Python {
                module: "builtin".to_string(),
                function: "human".to_string(),
                exec_class: zgraph_dsl::ExecClass::Io,
            };
        }
    }
    Arc::new(replay)
}

/// 由 `runs` 快照重建 `RunSpec`（graph/entry_inputs/user_id/seed/global_vars）。
///
/// `global_vars` 来自 v3 的 `runs.global_vars_json`（ISSUE-087）；历史 run 无记录时
/// 按空 map 兼容并告警（见 [`global_vars_from_run`]）。
fn build_spec(run: &RunRow, graph: Arc<CompiledGraph>) -> Result<RunSpec, ReplayError> {
    let raw_inputs = run.entry_inputs.as_ref().ok_or_else(|| {
        ReplayError::record(
            CODE_REPLAY_RECORD_MISSING,
            "run has no entry_inputs snapshot",
        )
    })?;
    let entry_inputs: HashMap<NodeId, HashMap<PortName, Value>> =
        serde_json::from_value(raw_inputs.clone()).map_err(|error| {
            ReplayError::record(
                CODE_REPLAY_RECORD_CORRUPT,
                format!("entry_inputs snapshot is invalid: {error}"),
            )
        })?;
    Ok(RunSpec {
        graph,
        entry_inputs,
        user_id: run.user_id.clone(),
        global_vars: global_vars_from_run(run)?,
        options: RunOptions {
            seed: run.seed,
            record_mode: zgraph_kernel::RecordMode::None,
            ..RunOptions::default()
        },
    })
}

/// 从 `runs.global_vars_json` 重建 `RunSpec.global_vars`（ISSUE-087）。
///
/// - v3 记录（object）→ 逐键还原；
/// - `None`（v2→v3 迁移前的历史 run）→ 空 map 兼容 + stderr 告警（旧行为；若图依赖
///   `ctx.get_var` 可能出现 `REPLAY_OUTPUT_MISMATCH`，与 `--record=full` 语义一致）；
/// - 非 object → `REPLAY_RECORD_CORRUPT`（记录损坏）。
fn global_vars_from_run(run: &RunRow) -> Result<HashMap<String, Value>, ReplayError> {
    match &run.global_vars {
        Some(Value::Object(map)) => Ok(map
            .iter()
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect()),
        Some(_) => Err(ReplayError::record(
            CODE_REPLAY_RECORD_CORRUPT,
            "global_vars snapshot is not a JSON object",
        )),
        None => {
            eprintln!(
                "warning: run {} has no recorded global_vars (pre-v3 record); \
                 replaying with empty globals",
                run.run_id
            );
            Ok(HashMap::new())
        }
    }
}

/// 比较重跑结果与原始输出并输出（DR-002 §3.3 步 4）。
fn finish_outputs(
    args: &ReplayArgs,
    run: &RunRow,
    outcome: &RunOutcome,
    recorded: usize,
) -> ExitCode {
    let matched = outcome.status == RunStatus::Completed
        && outcome.error.is_none()
        && outcome.output == run.output_json;
    let exit_code = if matched { EXIT_SUCCESS } else { EXIT_OTHER };
    let message = (!matched).then(|| mismatch_message(run, outcome));
    if args.json {
        println!(
            "{}",
            outputs_payload(run, outcome, recorded, matched, message.as_deref())
        );
    } else {
        println!("{}", json_text(outcome.output.as_ref()));
        if let Some(message) = &message {
            eprintln!("error[{CODE_REPLAY_OUTPUT_MISMATCH}]: {message}");
        } else {
            eprintln!(
                "ok replay run={} mode=outputs matched=true outputs={recorded}",
                run.run_id
            );
        }
    }
    ExitCode::from(exit_code)
}

/// `--json` 输出（L2）。
fn outputs_payload(
    run: &RunRow,
    outcome: &RunOutcome,
    recorded: usize,
    matched: bool,
    message: Option<&str>,
) -> Value {
    let mut object = Map::new();
    object.insert("ok".to_string(), json!(matched));
    object.insert("command".to_string(), json!("replay"));
    object.insert("mode".to_string(), json!("outputs"));
    object.insert("run_id".to_string(), json!(run.run_id));
    object.insert("record_mode".to_string(), json!(run.record_mode));
    object.insert("recorded_outputs".to_string(), json!(recorded));
    object.insert("status".to_string(), json!(outcome.status.as_str()));
    object.insert("matched".to_string(), json!(matched));
    object.insert("original_output".to_string(), json!(run.output_json));
    object.insert("replayed_output".to_string(), json!(outcome.output));
    if let Some(error) = &outcome.error {
        object.insert(
            "replayed_error".to_string(),
            json!({"code": error.code, "message": error.message}),
        );
    }
    if let Some(message) = message {
        object.insert(
            "error".to_string(),
            json!({"code": CODE_REPLAY_OUTPUT_MISMATCH, "message": message}),
        );
    }
    Value::Object(object)
}

/// 不一致摘要（状态/error_code/输出差异）。
fn mismatch_message(run: &RunRow, outcome: &RunOutcome) -> String {
    let error = outcome
        .error
        .as_ref()
        .map(|error| format!("{}: {}", error.code, error.message))
        .unwrap_or_else(|| "none".to_string());
    format!(
        "replayed result differs from recorded output: original status={} error_code={} output={}; \
         replayed status={} error={} output={}",
        run.status.as_str(),
        run.error_code.as_deref().unwrap_or("none"),
        json_text(run.output_json.as_ref()),
        outcome.status.as_str(),
        error,
        json_text(outcome.output.as_ref()),
    )
}

/// 受限长度的 JSON 文本（差异摘要用）。
fn json_text(value: Option<&Value>) -> String {
    let text = match value {
        Some(value) => serde_json::to_string(value).unwrap_or_else(|_| "null".to_string()),
        None => "null".to_string(),
    };
    if text.chars().count() <= DIFF_TEXT_LIMIT {
        return text;
    }
    let truncated: String = text.chars().take(DIFF_TEXT_LIMIT).collect();
    format!("{truncated}…[truncated]")
}

#[cfg(test)]
mod tests {
    use super::{
        build_spec, ensure_replayable, index_recorded_outputs, parse, replay_recorded_human_graph,
        validate_foreach_records, ReplayArgs, ReplayMode, CODE_REPLAY_RECORD_CORRUPT,
        CODE_REPLAY_RECORD_MISSING, CODE_REPLAY_RUN_NOT_COMPLETED,
    };
    use crate::run_cmd::output_hash;
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::sync::Arc;

    use serde_json::{json, Value};
    use zgraph_kernel::{
        run_to_completion, CompiledGraph, CompiledHandler, MemoryTraceSink, NodeError,
        NodeExecution, NodeExecutor, RunStatus,
    };
    use zgraph_trace::{RunRow, RunStatus as TraceRunStatus};

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    const GV_GRAPH: &str = r#"{
        "version": "1.0",
        "metadata": {"name": "gv"},
        "entry_points": ["start"],
        "nodes": [
          {"id": "start", "type": "start", "inputs": [], "config": {}},
          {"id": "work", "type": "python_node",
           "inputs": [{"name": "i", "type": "any", "required": true}],
           "config": {"module": "m", "function": "f"}},
          {"id": "end", "type": "end",
           "inputs": [{"name": "r", "type": "any", "required": true}], "config": {}}
        ],
        "edges": [
          {"from": "start", "to_node": "work", "to_input_port": "i"},
          {"from": "work", "to_node": "end", "to_input_port": "r"}
        ]
    }"#;

    fn gv_graph() -> Arc<CompiledGraph> {
        let loaded = zgraph_dsl::load_graph(GV_GRAPH.as_bytes()).expect("graph loads");
        CompiledGraph::compile(&loaded).expect("graph compiles")
    }

    /// W3a L2：重放前把 human 节点替换为可执行 handler（由 `RecordedExecutor` 按
    /// `(node_id, attempt)` 返回记录输出，不重新要求人工决策）；无 human 图原样返回。
    #[test]
    fn replay_recorded_human_graph_replaces_human_handlers_only() {
        const HUMAN_GRAPH: &str = r#"{
            "version": "1.0",
            "metadata": {"name": "human"},
            "entry_points": ["start"],
            "nodes": [
              {"id": "start", "type": "start", "inputs": [], "config": {}},
              {"id": "review", "type": "python_node",
               "inputs": [{"name": "p", "type": "any", "required": true}],
               "config": {"module": "builtin", "function": "human"}},
              {"id": "end", "type": "end",
               "inputs": [{"name": "r", "type": "any", "required": true}], "config": {}}
            ],
            "edges": [
              {"from": "start", "to_node": "review", "to_input_port": "p"},
              {"from": "review", "to_node": "end", "to_input_port": "r"}
            ]
        }"#;
        let loaded = zgraph_dsl::load_graph(HUMAN_GRAPH.as_bytes()).expect("human graph loads");
        let graph = CompiledGraph::compile(&loaded).expect("human graph compiles");
        assert!(matches!(
            &graph.nodes["review"].handler,
            CompiledHandler::Human
        ));

        let replay = replay_recorded_human_graph(Arc::clone(&graph));
        assert!(
            matches!(
                &replay.nodes["review"].handler,
                CompiledHandler::Python { .. }
            ),
            "human handler must become replayable through the recorded executor"
        );
        assert!(matches!(
            &replay.nodes["start"].handler,
            CompiledHandler::Start
        ));
        assert_eq!(
            replay.graph_hash, graph.graph_hash,
            "重放替换不改 graph_hash/审计字段"
        );

        // 无 human 图：同一 `Arc` 原样返回（零变化）。
        let plain = gv_graph();
        assert!(Arc::ptr_eq(
            &plain,
            &replay_recorded_human_graph(Arc::clone(&plain))
        ));
    }

    fn sample_run(global_vars: Option<Value>) -> RunRow {
        RunRow {
            run_id: "run-gv".to_string(),
            graph_hash: "0123456789abcdef".to_string(),
            graph_name: Some("gv".to_string()),
            status: TraceRunStatus::Completed,
            user_id: "local".to_string(),
            started_at: "2026-10-30T00:00:00.000Z".to_string(),
            finished_at: Some("2026-10-30T00:00:01.000Z".to_string()),
            entry_inputs: Some(json!({})),
            output_json: Some(json!({"r": 5})),
            error_code: None,
            error_message: None,
            interrupted: false,
            recovered_at: None,
            last_seq: 1,
            event_count: 1,
            streaming_dropped: 0,
            abandoned_tasks: 0,
            trace_backpressure: 0,
            killed_workers: 0,
            seed: 7,
            record_mode: Some("full".to_string()),
            owner_pid: None,
            owner_host: None,
            heartbeat_at: None,
            global_vars,
            worker_versions: None,
            model_params: None,
        }
    }

    /// 读取 `ctx.get_var("threshold")` 作为输出的执行器（验证重放环境重建）。
    struct GetVarExecutor;

    impl NodeExecutor for GetVarExecutor {
        fn execute<'a>(
            &'a self,
            execution: NodeExecution<'a>,
        ) -> std::pin::Pin<
            Box<dyn std::future::Future<Output = Result<Value, NodeError>> + Send + 'a>,
        > {
            let value = execution.ctx.get_var("threshold").cloned();
            Box::pin(async move {
                match value {
                    // 节点输出直接作为 end 的输入端口值（内核把整个 output 投递到出边端口）。
                    Some(value) => Ok(value),
                    None => Err(NodeError::Internal {
                        message: "threshold global var missing in replay".to_string(),
                    }),
                }
            })
        }
    }

    #[test]
    fn build_spec_restores_recorded_global_vars() {
        let run = sample_run(Some(json!({"threshold": 5, "tenant": "acme"})));
        let spec = build_spec(&run, gv_graph()).expect("build spec");
        assert_eq!(spec.global_vars.get("threshold"), Some(&json!(5)));
        assert_eq!(spec.global_vars.get("tenant"), Some(&json!("acme")));
    }

    #[tokio::test]
    async fn replayed_spec_exposes_global_vars_through_ctx_get_var() {
        let run = sample_run(Some(json!({"threshold": 5})));
        let spec = build_spec(&run, gv_graph()).expect("build spec");
        let outcome = run_to_completion(
            None,
            spec,
            Arc::new(GetVarExecutor),
            Arc::new(MemoryTraceSink::new()),
        )
        .await
        .expect("replay run starts");
        assert_eq!(outcome.status, RunStatus::Completed);
        assert_eq!(
            outcome.output, run.output_json,
            "L2 重放输出与记录一致（依赖 ctx.get_var）"
        );
    }

    #[test]
    fn build_spec_legacy_run_without_global_vars_falls_back_to_empty_map() {
        let run = sample_run(None);
        let spec = build_spec(&run, gv_graph()).expect("legacy run stays replayable");
        assert!(spec.global_vars.is_empty());
    }

    #[test]
    fn build_spec_rejects_corrupt_global_vars_snapshot() {
        let run = sample_run(Some(json!(42)));
        let error = match build_spec(&run, gv_graph()) {
            Ok(_) => panic!("non-object global_vars snapshot must be rejected"),
            Err(error) => error,
        };
        assert_eq!(error.code, super::CODE_REPLAY_RECORD_CORRUPT);
    }

    #[test]
    fn parse_defaults_to_events_mode() {
        let parsed = parse(&args(&["--run", "run-1"]))
            .expect("parse")
            .expect("not help");
        assert_eq!(
            parsed,
            ReplayArgs {
                run_id: "run-1".to_string(),
                mode: ReplayMode::Events,
                db: None,
                json: false,
            }
        );
    }

    #[test]
    fn parse_accepts_equals_forms_and_outputs_mode() {
        let parsed = parse(&args(&[
            "--run=run-2",
            "--mode=outputs",
            "--db",
            "/tmp/t.db",
            "--json",
        ]))
        .expect("parse")
        .expect("not help");
        assert_eq!(parsed.run_id, "run-2");
        assert_eq!(parsed.mode, ReplayMode::Outputs);
        assert_eq!(parsed.db, Some(PathBuf::from("/tmp/t.db")));
        assert!(parsed.json);
    }

    #[test]
    fn parse_rejects_usage_errors() {
        assert!(parse(&args(&[])).is_err(), "missing --run");
        assert!(parse(&args(&["--run"])).is_err(), "missing value");
        assert!(parse(&args(&["--run", "r", "--mode", "all"])).is_err());
        assert!(parse(&args(&["--run", "r", "--nope"])).is_err());
        assert!(parse(&args(&["--run", "r", "positional"])).is_err());
    }

    #[test]
    fn parse_accepts_tools_mode_and_recorded_replay_tools() {
        let parsed = parse(&args(&["--run", "r", "--mode=tools"]))
            .expect("parse")
            .expect("not help");
        assert_eq!(parsed.mode, ReplayMode::Tools);
        let parsed = parse(&args(&[
            "--run",
            "r",
            "--mode",
            "tools",
            "--replay-tools=recorded",
        ]))
        .expect("parse")
        .expect("not help");
        assert_eq!(parsed.mode, ReplayMode::Tools);
        assert!(parse(&args(&[
            "--run",
            "r",
            "--mode=events",
            "--replay-tools",
            "recorded"
        ]))
        .expect("parse")
        .is_some());
    }

    #[test]
    fn parse_rejects_reexec_and_unknown_replay_tools() {
        // v0.4 只实现 recorded；reexec 是 v0.5 接口 → 用法错误（exit 2 由 dispatch 映射）。
        let error = parse(&args(&[
            "--run",
            "r",
            "--mode=tools",
            "--replay-tools=reexec",
        ]))
        .expect_err("reexec not implemented");
        assert!(error.contains("not implemented in v0.4"), "{error}");
        let error = parse(&args(&["--run", "r", "--replay-tools=bogus"]))
            .expect_err("invalid replay-tools");
        assert!(error.contains("invalid --replay-tools"), "{error}");
        let error = parse(&args(&["--run", "r", "--mode=bogus"])).expect_err("invalid mode");
        assert!(error.contains("events|outputs|tools"), "{error}");
    }

    #[test]
    fn parse_help_returns_none() {
        assert!(parse(&args(&["--help"])).expect("parse").is_none());
        assert!(parse(&args(&["--run", "r", "-h"]))
            .expect("parse")
            .is_none());
    }

    #[test]
    fn ensure_replayable_checks_record_mode_and_status() {
        // 非 full 记录 → REPLAY_RECORD_MISSING（含 mode 文案）。
        let error = ensure_replayable(Some("hash"), TraceRunStatus::Completed)
            .expect_err("hash mode must be rejected");
        assert_eq!(error.code, CODE_REPLAY_RECORD_MISSING);
        assert!(error.message.contains("--record=full"), "{}", error.message);
        // 未记录 record_mode 视为 unknown → 同样拒绝。
        let error = ensure_replayable(None, TraceRunStatus::Completed).expect_err("unknown mode");
        assert_eq!(error.code, CODE_REPLAY_RECORD_MISSING);
        // 非 Completed → REPLAY_RUN_NOT_COMPLETED。
        let error = ensure_replayable(Some("full"), TraceRunStatus::Failed)
            .expect_err("failed run must be rejected");
        assert_eq!(error.code, CODE_REPLAY_RUN_NOT_COMPLETED);
        assert!(ensure_replayable(Some("full"), TraceRunStatus::Completed).is_ok());
    }

    #[test]
    fn index_recorded_outputs_verifies_hash_duplicates_and_missing_json() {
        let output = json!({"r": 1});
        let hash = output_hash(&output);
        let record = |hash: String, output_json: Option<Value>| zgraph_trace::RecordedOutputRow {
            recorded_output_id: 1,
            run_id: "run-1".to_string(),
            instance_id: "inst-1".to_string(),
            node_id: "work".to_string(),
            attempt: 0,
            output_hash: hash,
            output_json,
            recorded_at: "2026-10-30T00:00:00.000Z".to_string(),
        };
        let item_indexes = HashMap::from([("inst-1".to_string(), 0_u32)]);

        // 合法记录 → 建立索引（键含 item_index）。
        let outputs =
            index_recorded_outputs(&[record(hash.clone(), Some(output.clone()))], &item_indexes)
                .expect("valid record");
        assert_eq!(
            outputs.get(&("work".to_string(), 0, 0)),
            Some(&output),
            "无 foreach 记录 item_index=0，键与旧 (node_id, attempt) 一一对应"
        );

        // hash 不符 → CORRUPT。
        let error = index_recorded_outputs(
            &[record("bad-hash".to_string(), Some(output.clone()))],
            &item_indexes,
        )
        .expect_err("hash mismatch");
        assert_eq!(error.code, CODE_REPLAY_RECORD_CORRUPT);

        // 缺 output_json（hash 模式记录）→ CORRUPT。
        let error = index_recorded_outputs(&[record(hash.clone(), None)], &item_indexes)
            .expect_err("missing json");
        assert_eq!(error.code, CODE_REPLAY_RECORD_CORRUPT);

        // 未知实例（无法从 instances 取得 item_index）→ CORRUPT。
        let error = index_recorded_outputs(
            &[record(hash.clone(), Some(output.clone()))],
            &HashMap::new(),
        )
        .expect_err("unknown instance");
        assert_eq!(error.code, CODE_REPLAY_RECORD_CORRUPT);

        // 重复 (node_id, attempt, item_index) → CORRUPT。
        let error = index_recorded_outputs(
            &[
                record(hash.clone(), Some(output.clone())),
                record(hash, Some(output)),
            ],
            &item_indexes,
        )
        .expect_err("duplicate record");
        assert_eq!(error.code, CODE_REPLAY_RECORD_CORRUPT);
    }

    // -----------------------------------------------------------------------
    // W4b：foreach per-item 记录键与 expansion 完整性预检
    // -----------------------------------------------------------------------

    /// P-1 形状的 foreach 图：`start → F(foreach, join=J) → W(出口 E) → J`。
    const FOREACH_GRAPH: &str = r#"{
        "version": "1.0",
        "metadata": {"name": "foreach"},
        "entry_points": ["start"],
        "nodes": [
          {"id": "start", "type": "start", "inputs": [], "config": {}},
          {"id": "F", "type": "python_node",
           "inputs": [{"name": "items", "type": "any", "required": true}],
           "config": {"module": "workers.foreach", "function": "fanout"},
           "foreach": {"items_port": "items", "join": "J"}},
          {"id": "W", "type": "python_node",
           "inputs": [{"name": "task", "type": "any", "required": true}],
           "config": {"module": "workers.foreach", "function": "work"}},
          {"id": "J", "type": "end",
           "inputs": [{"name": "results", "type": "any", "required": true}], "config": {}}
        ],
        "edges": [
          {"from": "start", "to_node": "F", "to_input_port": "items"},
          {"from": "F", "to_node": "W", "to_input_port": "task"},
          {"from": "W", "to_node": "J", "to_input_port": "results"}
        ]
    }"#;

    fn foreach_graph() -> Arc<CompiledGraph> {
        let loaded = zgraph_dsl::load_graph(FOREACH_GRAPH.as_bytes()).expect("foreach graph loads");
        CompiledGraph::compile(&loaded).expect("foreach graph compiles")
    }

    fn instance_row(
        instance_id: &str,
        node_id: &str,
        item_index: u32,
        item_key: Option<&str>,
        status: zgraph_trace::InstanceStatus,
    ) -> zgraph_trace::InstanceRow {
        zgraph_trace::InstanceRow {
            instance_id: instance_id.to_string(),
            run_id: "run-f".to_string(),
            node_id: node_id.to_string(),
            node_type: "python_node".to_string(),
            attempt: 0,
            item_index,
            item_key: item_key.map(str::to_string),
            status,
            input: None,
            output_json: None,
            error: None,
            error_code: None,
            created_seq: None,
            started_at: None,
            finished_at: None,
            finished_seq: None,
        }
    }

    fn expansion_row(
        node_id: &str,
        item_count: u64,
        keys: Value,
        source: Option<&str>,
    ) -> zgraph_trace::ForeachExpansionRow {
        zgraph_trace::ForeachExpansionRow {
            expansion_id: format!("exp-{node_id}"),
            run_id: "run-f".to_string(),
            node_id: node_id.to_string(),
            source_instance_id: source.map(str::to_string),
            item_count,
            keys: Some(keys),
            at: "2026-11-01T10:00:00.000Z".to_string(),
        }
    }

    fn done(node_id: &str, item_index: u32) -> zgraph_trace::InstanceRow {
        instance_row(
            &format!("inst-{node_id}-{item_index}"),
            node_id,
            item_index,
            None,
            zgraph_trace::InstanceStatus::Completed,
        )
    }

    /// 一致的 expansion（count=2 / keys 索引对象 / F item 实例 0,1）→ 通过。
    #[test]
    fn validate_foreach_records_accepts_consistent_expansion() {
        let instances = vec![done("F", 0), done("F", 1), done("W", 0), done("W", 1)];
        let expansions = vec![expansion_row(
            "F",
            2,
            json!([{"$index": 0}, {"$index": 1}]),
            Some("inst-start"),
        )];
        validate_foreach_records(&expansions, &foreach_graph(), &instances)
            .expect("consistent expansion must pass");
    }

    /// 空列表展开（count=0 / keys=[] / 无 item 实例）→ 通过。
    #[test]
    fn validate_foreach_records_accepts_empty_expansion() {
        let expansions = vec![expansion_row("F", 0, json!([]), None)];
        validate_foreach_records(&expansions, &foreach_graph(), &[])
            .expect("empty expansion must pass");
    }

    /// `item_count` 与 F 的 item_index 集合不一致 → REPLAY_RECORD_CORRUPT。
    #[test]
    fn validate_foreach_records_rejects_item_count_mismatch() {
        let instances = vec![done("F", 0), done("F", 1)];
        let expansions = vec![expansion_row(
            "F",
            3,
            json!([{"$index": 0}, {"$index": 1}, {"$index": 2}]),
            None,
        )];
        let error = validate_foreach_records(&expansions, &foreach_graph(), &instances)
            .expect_err("item_count mismatch");
        assert_eq!(error.code, CODE_REPLAY_RECORD_CORRUPT);
        assert!(error.message.contains("item_count 3"), "{}", error.message);
    }

    /// `keys` 与实例 `item_key` 不一致 → REPLAY_RECORD_CORRUPT（含显式 key 原值）。
    #[test]
    fn validate_foreach_records_rejects_keys_mismatch() {
        let instances = vec![
            instance_row(
                "inst-F-0",
                "F",
                0,
                Some("a"),
                zgraph_trace::InstanceStatus::Completed,
            ),
            instance_row(
                "inst-F-1",
                "F",
                1,
                Some("b"),
                zgraph_trace::InstanceStatus::Completed,
            ),
        ];
        // 实例 item_key=Some("a"/"b")，keys 却是索引对象 → 不一致。
        let wrong = vec![expansion_row(
            "F",
            2,
            json!([{"$index": 0}, {"$index": 1}]),
            None,
        )];
        let error = validate_foreach_records(&wrong, &foreach_graph(), &instances)
            .expect_err("keys mismatch");
        assert_eq!(error.code, CODE_REPLAY_RECORD_CORRUPT);

        // 显式 key 正确 → 通过。
        let right = vec![expansion_row("F", 2, json!(["a", "b"]), None)];
        validate_foreach_records(&right, &foreach_graph(), &instances).expect("explicit keys");

        // 非法 keys 元素（number）→ CORRUPT。
        let invalid = vec![expansion_row("F", 2, json!([1, 2]), None)];
        let error = validate_foreach_records(&invalid, &foreach_graph(), &instances)
            .expect_err("invalid keys entry");
        assert_eq!(error.code, CODE_REPLAY_RECORD_CORRUPT);
    }

    /// region 已有 item 实例但缺 expansion 行 → REPLAY_RECORD_MISSING；
    /// controller 未展开（region 无实例）→ 合法。
    #[test]
    fn validate_foreach_records_requires_expansion_for_expanded_region() {
        let expanded = vec![done("F", 0), done("W", 0)];
        let error = validate_foreach_records(&[], &foreach_graph(), &expanded)
            .expect_err("missing expansion");
        assert_eq!(error.code, CODE_REPLAY_RECORD_MISSING);

        // 未展开（region 无实例；例如 controller Skipped）→ 不要求 expansion。
        let unexpanded = vec![instance_row(
            "inst-start",
            "start",
            0,
            None,
            zgraph_trace::InstanceStatus::Completed,
        )];
        validate_foreach_records(&[], &foreach_graph(), &unexpanded)
            .expect("region never expanded is valid without expansion row");
    }

    /// expansion 引用未知节点/非 foreach 节点、同一 F 多行 → REPLAY_RECORD_CORRUPT。
    #[test]
    fn validate_foreach_records_rejects_invalid_expansion_rows() {
        // 非 foreach 节点（W）。
        let error = validate_foreach_records(
            &[expansion_row("W", 1, json!([{"$index": 0}]), None)],
            &foreach_graph(),
            &[],
        )
        .expect_err("non-foreach node expansion");
        assert_eq!(error.code, CODE_REPLAY_RECORD_CORRUPT);

        // 未知节点。
        let error = validate_foreach_records(
            &[expansion_row("nope", 0, json!([]), None)],
            &foreach_graph(),
            &[],
        )
        .expect_err("unknown node expansion");
        assert_eq!(error.code, CODE_REPLAY_RECORD_CORRUPT);

        // 同一 F 两行。
        let duplicated = vec![
            expansion_row("F", 0, json!([]), None),
            zgraph_trace::ForeachExpansionRow {
                expansion_id: "exp-F-2".to_string(),
                ..expansion_row("F", 0, json!([]), None)
            },
        ];
        let error = validate_foreach_records(&duplicated, &foreach_graph(), &[])
            .expect_err("duplicate expansion");
        assert_eq!(error.code, CODE_REPLAY_RECORD_CORRUPT);
    }
}
