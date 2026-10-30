//! `zgraph replay` 子命令（M5 wave2）：L1 事件回放 / L2 结果回放（DR-002 §3.3）。
//!
//! - `--mode=events`（默认，L1）：复用 [`crate::trace_cmd`] 的只读回放实现与输出格式，
//!   **不重跑、不写库**；
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

use std::collections::HashMap;
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::process::ExitCode;
use std::sync::Arc;

use serde_json::{json, Map, Value};

use zgraph_dsl::load_graph;
use zgraph_kernel::{
    run_to_completion, CompiledGraph, MemoryTraceSink, NodeError, NodeExecution, NodeExecutor,
    NodeId, PortName, RunOptions, RunOutcome, RunSpec, RunStatus,
};
use zgraph_trace::{
    InstanceStatus as TraceInstanceStatus, RunRow, RunStatus as TraceRunStatus, TraceError,
    TraceReader,
};

use crate::run_cmd::output_hash;
use crate::trace_cmd::{self, resolve_db_path, TraceOptions};
use crate::{EXIT_OTHER, EXIT_STORAGE, EXIT_SUCCESS};

/// `replay --help` 的用法文本。
pub const REPLAY_USAGE: &str = "\
usage: zgraph replay --run <run_id> [options]

options:
  --run <run_id>        run to replay
  --mode <mode>         events (L1, default) | outputs (L2)
  --db <path>           trace database (default: $ZGRAPH_DB or .zgraph/trace.db)
  --json                machine-readable output (events: NDJSON; outputs: JSON object)
  --help, -h            print this help";

/// `REPLAY_RECORD_MISSING`：`--record=full` 记录缺失/不完整（DR-002 §3.3）。
pub const CODE_REPLAY_RECORD_MISSING: &str = "REPLAY_RECORD_MISSING";

/// `REPLAY_RUN_NOT_COMPLETED`：原始 run 非 `Completed`，不支持 L2 重放。
pub const CODE_REPLAY_RUN_NOT_COMPLETED: &str = "REPLAY_RUN_NOT_COMPLETED";

/// `REPLAY_RECORD_CORRUPT`：记录 hash 复核失败 / 记录快照不可用。
pub const CODE_REPLAY_RECORD_CORRUPT: &str = "REPLAY_RECORD_CORRUPT";

/// `REPLAY_OUTPUT_MISMATCH`：L2 重跑结果与原始输出不一致。
pub const CODE_REPLAY_OUTPUT_MISMATCH: &str = "REPLAY_OUTPUT_MISMATCH";

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
        match arg.as_str() {
            "--run" => run_id = Some(option_value(&mut iter, "--run")?),
            "--mode" => mode = parse_mode(&option_value(&mut iter, "--mode")?)?,
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
        other => Err(format!(
            "invalid --mode '{other}' (expected events|outputs)"
        )),
    }
}

/// 执行 `replay` 并返回进程退出码。
pub fn run(args: &ReplayArgs) -> ExitCode {
    match args.mode {
        ReplayMode::Events => run_events(args),
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

/// L2 预检产物。
struct ReplayPlan {
    /// `(node_id, attempt) -> recorded output.json`。
    outputs: HashMap<(String, u32), Value>,
    /// 记录行数（摘要用）。
    recorded: usize,
}

/// 以记录输出为唯一数据源的执行器（不启动 worker/LLM）。
struct RecordedExecutor {
    outputs: HashMap<(String, u32), Value>,
}

impl NodeExecutor for RecordedExecutor {
    fn execute<'a>(
        &'a self,
        execution: NodeExecution<'a>,
    ) -> Pin<Box<dyn Future<Output = Result<Value, NodeError>> + Send + 'a>> {
        let key = (execution.node.id.clone(), execution.ctx.attempt());
        let result = match self.outputs.get(&key) {
            Some(output) => Ok(output.clone()),
            None => Err(NodeError::Internal {
                message: format!(
                    "replay: no recorded output for node '{}' attempt {}",
                    key.0, key.1
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

/// 预检：record_mode/status/记录完整性与 hash 复核（DR-002 §3.3 步 2）。
fn build_plan(reader: &TraceReader, run: &RunRow) -> Result<ReplayPlan, ReplayError> {
    let record_mode = run.record_mode.as_deref().unwrap_or("unknown");
    if record_mode != "full" {
        return Err(ReplayError::record(
            CODE_REPLAY_RECORD_MISSING,
            format!(
                "run was recorded with mode '{record_mode}'; --mode=outputs requires --record=full"
            ),
        ));
    }
    if run.status != TraceRunStatus::Completed {
        return Err(ReplayError::record(
            CODE_REPLAY_RUN_NOT_COMPLETED,
            format!(
                "run status is {}; only Completed runs can be replayed",
                run.status.as_str()
            ),
        ));
    }
    let instances = reader.instances(&run.run_id)?;
    let records = reader.recorded_outputs(&run.run_id)?;
    let mut outputs: HashMap<(String, u32), Value> = HashMap::new();
    for record in &records {
        let output = record.output_json.as_ref().ok_or_else(|| {
            ReplayError::record(
                CODE_REPLAY_RECORD_CORRUPT,
                format!(
                    "recorded output for node '{}' attempt {} has no output_json (full record required)",
                    record.node_id, record.attempt
                ),
            )
        })?;
        let recomputed = output_hash(output);
        if recomputed != record.output_hash {
            return Err(ReplayError::record(
                CODE_REPLAY_RECORD_CORRUPT,
                format!(
                    "recorded output hash mismatch for node '{}' attempt {}: recorded={} recomputed={recomputed}",
                    record.node_id, record.attempt, record.output_hash
                ),
            ));
        }
        let key = (record.node_id.clone(), record.attempt);
        if outputs.insert(key.clone(), output.clone()).is_some() {
            return Err(ReplayError::record(
                CODE_REPLAY_RECORD_CORRUPT,
                format!(
                    "duplicate recorded output for node '{}' attempt {}",
                    key.0, key.1
                ),
            ));
        }
    }
    for instance in &instances {
        if !EXECUTED_NODE_TYPES.contains(&instance.node_type.as_str())
            || instance.status != TraceInstanceStatus::Completed
        {
            continue;
        }
        let key = (instance.node_id.clone(), instance.attempt);
        if !outputs.contains_key(&key) {
            return Err(ReplayError::record(
                CODE_REPLAY_RECORD_MISSING,
                format!(
                    "missing recorded output for executed node '{}' attempt {} (status=Completed)",
                    key.0, key.1
                ),
            ));
        }
    }
    Ok(ReplayPlan {
        outputs,
        recorded: records.len(),
    })
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
    use super::{build_spec, parse, ReplayArgs, ReplayMode};
    use std::path::PathBuf;
    use std::sync::Arc;

    use serde_json::{json, Value};
    use zgraph_kernel::{
        run_to_completion, CompiledGraph, MemoryTraceSink, NodeError, NodeExecution, NodeExecutor,
        RunStatus,
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
    fn parse_help_returns_none() {
        assert!(parse(&args(&["--help"])).expect("parse").is_none());
        assert!(parse(&args(&["--run", "r", "-h"]))
            .expect("parse")
            .is_none());
    }
}
