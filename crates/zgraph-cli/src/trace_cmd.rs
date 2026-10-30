//! `zgraph trace` 子命令：L1 只读回放（06 §6.10.4 / §6.11.2 / §6.8.3）。
//!
//! 只读约束：通过 [`TraceReader::open_readonly`] 打开库，**不创建库、不迁移、
//! 不标定**。事件严格按 `seq ASC` 输出（禁止按 `at` 排序），`--json` 输出 NDJSON
//! （一行一个事件），诊断信息一律走 stderr。

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use serde_json::Value;
use zgraph_trace::{EventQuery, EventRow, RunRow, RunStats, RunStatus, TraceError, TraceReader};

use crate::{EXIT_OTHER, EXIT_STORAGE, EXIT_SUCCESS};

/// 默认最多显示条数（06 §6.10.4；`0` = 不限）。
pub const DEFAULT_LIMIT: usize = 1000;

/// 默认库路径（相对当前工作目录；06 §6.9.1）。
pub const DEFAULT_DB_RELATIVE: &str = ".zgraph/trace.db";

/// 环境变量形式的库路径（优先级低于 `--db`、高于默认值）。
pub const DB_ENV: &str = "ZGRAPH_DB";

/// 7 类事件 kind（06 §6.2.6）。
pub const EVENT_KINDS: [&str; 7] = [
    "RunStarted",
    "Transition",
    "GateResult",
    "NodeTimedOut",
    "NodeAbandoned",
    "Streaming",
    "RunFinished",
];

/// `trace --help` 的用法文本。
pub const TRACE_USAGE: &str = "\
usage: zgraph trace <run_id> [options]

options:
  --db <path>          trace database (default: $ZGRAPH_DB or .zgraph/trace.db)
  --json               emit NDJSON (one JSON event per line, ordered by seq)
  --since-seq <n>      only show events with seq > n
  --limit <n>          max events to show (default 1000; 0 = unlimited)
  --kinds <k1,k2,...>  only show these event kinds
  --no-streaming       exclude Streaming events
  --stats              print a stats summary instead of the timeline
  -q, --quiet          suppress progress output
  --help, -h           print this help";

/// `trace` 的参数（解析后）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TraceOptions {
    /// 必填的 run id。
    pub run_id: String,
    /// `--db` 显式库路径（优先级最高）。
    pub db: Option<PathBuf>,
    /// `--json`：stdout 输出 NDJSON。
    pub json: bool,
    /// `--since-seq`：只显示 `seq > n`。
    pub since_seq: u64,
    /// `--limit`：最多条数；`0` = 不限。
    pub limit: usize,
    /// `--kinds`：kind 白名单（已与 `--no-streaming` 归一化）。
    pub kinds: Option<Vec<String>>,
    /// `--no-streaming`：排除 Streaming 骨架。
    pub no_streaming: bool,
    /// `--stats`：只输出统计摘要。
    pub stats: bool,
    /// `-q/--quiet`：抑制 stderr 进度（trace 无进度，接受即可）。
    pub quiet: bool,
}

/// 解析 `trace` 之后的参数；`--help`/`-h` 返回 `Ok(None)`。
pub fn parse(args: &[String]) -> Result<Option<TraceOptions>, String> {
    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        return Ok(None);
    }
    let mut options = TraceOptions {
        run_id: String::new(),
        db: None,
        json: false,
        since_seq: 0,
        limit: DEFAULT_LIMIT,
        kinds: None,
        no_streaming: false,
        stats: false,
        quiet: false,
    };
    let mut run_id: Option<String> = None;
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--db" => options.db = Some(PathBuf::from(option_value(&mut iter, "--db")?)),
            "--json" => options.json = true,
            "--since-seq" => {
                let value = option_value(&mut iter, "--since-seq")?;
                options.since_seq = parse_number(value, "--since-seq")?;
            }
            "--limit" => {
                let value = option_value(&mut iter, "--limit")?;
                options.limit = parse_number(value, "--limit")?;
            }
            "--kinds" => {
                let value = option_value(&mut iter, "--kinds")?;
                options.kinds = Some(parse_kinds(value)?);
            }
            "--no-streaming" => options.no_streaming = true,
            "--stats" => options.stats = true,
            "-q" | "--quiet" => options.quiet = true,
            flag if flag.starts_with('-') => return Err(format!("unknown option: {flag}")),
            value => {
                if run_id.is_some() {
                    return Err(format!("unexpected argument: {value}"));
                }
                run_id = Some(value.to_string());
            }
        }
    }
    options.run_id = run_id.ok_or_else(|| "trace requires <run_id>".to_string())?;
    normalize_kinds(&mut options);
    Ok(Some(options))
}

/// 取一个需要值的 option 的参数。
fn option_value<'a>(
    iter: &mut std::slice::Iter<'a, String>,
    flag: &str,
) -> Result<&'a str, String> {
    iter.next()
        .map(String::as_str)
        .ok_or_else(|| format!("{flag} requires a value"))
}

/// 解析数字参数；负数/非数字均为用法错误。
fn parse_number<T: std::str::FromStr>(value: &str, flag: &str) -> Result<T, String> {
    value
        .parse()
        .map_err(|_| format!("invalid value for {flag}: {value}"))
}

/// 解析逗号分隔的 kind 白名单，校验 7 类枚举并去重。
fn parse_kinds(value: &str) -> Result<Vec<String>, String> {
    let mut kinds: Vec<String> = Vec::new();
    for kind in value.split(',') {
        let kind = kind.trim();
        if !EVENT_KINDS.contains(&kind) {
            return Err(format!(
                "invalid --kinds value: '{kind}' (allowed: {})",
                EVENT_KINDS.join(",")
            ));
        }
        if !kinds.iter().any(|existing| existing.as_str() == kind) {
            kinds.push(kind.to_string());
        }
    }
    Ok(kinds)
}

/// `--no-streaming` 等价于 kind 白名单去掉 `Streaming`。
fn normalize_kinds(options: &mut TraceOptions) {
    if !options.no_streaming {
        return;
    }
    if let Some(kinds) = options.kinds.as_mut() {
        kinds.retain(|kind| kind != "Streaming");
    }
}

/// 解析库路径：`--db` > env `ZGRAPH_DB` > `<cwd>/.zgraph/trace.db`（06 §6.9.1）。
pub fn resolve_db_path(explicit: Option<&Path>) -> PathBuf {
    if let Some(path) = explicit {
        return path.to_path_buf();
    }
    if let Some(value) = std::env::var_os(DB_ENV) {
        if !value.is_empty() {
            return PathBuf::from(value);
        }
    }
    match std::env::current_dir() {
        Ok(cwd) => cwd.join(DEFAULT_DB_RELATIVE),
        Err(_) => PathBuf::from(DEFAULT_DB_RELATIVE),
    }
}

/// 执行 `trace` 并返回进程退出码。
pub fn run(options: &TraceOptions) -> ExitCode {
    let db_path = resolve_db_path(options.db.as_deref());
    let reader = match TraceReader::open_readonly(&db_path) {
        Ok(reader) => reader,
        Err(error) => return report_storage_error(&error),
    };
    let run = match reader.run(&options.run_id) {
        Ok(Some(run)) => run,
        Ok(None) => {
            eprintln!("error[STORAGE_ERROR]: run not found: {}", options.run_id);
            return ExitCode::from(EXIT_STORAGE);
        }
        Err(error) => return report_storage_error(&error),
    };
    let truncated = match is_truncated(&reader, &run) {
        Ok(truncated) => truncated,
        Err(error) => return report_storage_error(&error),
    };
    print_in_progress_note(&run);
    if options.stats {
        run_stats(&reader, options, &run, truncated)
    } else {
        run_timeline(&reader, options, &run, truncated)
    }
}

/// 06 §6.8.3：`Running`/`Paused` 的进行中提示（stderr；stdout 保持纯数据）。
///
/// `owner_host` 校验（ISSUE-088 / REV-030 #5）：只有**同机**才用本机 `kill(pid, 0)`
/// 判活并打印 `run still in progress (pid N)`；跨机时本机 pid 与 owner_pid 无关，
/// 改为提示「可能在其他主机」；`owner_host` 未知时不猜同机，同样只提示可能在其他
/// 主机。非 Unix 无 `kill(pid, 0)`，不打印 pid 存活提示。诊断走 stderr
/// （与 `--json` NDJSON 的 stdout 契约一致）；退出码按原语义 0。
fn print_in_progress_note(run: &RunRow) {
    if !matches!(run.status, RunStatus::Running | RunStatus::Paused) {
        return;
    }
    let Some(pid) = run.owner_pid else {
        return;
    };
    if run
        .owner_host
        .as_deref()
        .is_some_and(crate::recovery::is_local_host)
    {
        if crate::recovery::PID_PROBE_SUPPORTED && crate::recovery::pid_alive(pid) {
            eprintln!("run still in progress (pid {pid})");
        }
        return;
    }
    let owner = run
        .owner_host
        .as_deref()
        .filter(|host| crate::recovery::host_is_known(host))
        .unwrap_or("unknown");
    eprintln!("run may be running on another host (owner host {owner}); local pid check skipped");
}

/// `interrupted=1` 且库内无 `RunFinished` → 截断（06 §6.8.3）。
fn is_truncated(reader: &TraceReader, run: &RunRow) -> Result<bool, TraceError> {
    if !run.interrupted {
        return Ok(false);
    }
    let query = EventQuery {
        kinds: Some(vec!["RunFinished".to_string()]),
        limit: 1,
        ..EventQuery::default()
    };
    Ok(reader.events(&run.run_id, &query)?.is_empty())
}

/// 回放时间线：human 表格或 `--json` NDJSON。
fn run_timeline(
    reader: &TraceReader,
    options: &TraceOptions,
    run: &RunRow,
    truncated: bool,
) -> ExitCode {
    let events = match query_events(reader, options) {
        Ok(events) => events,
        Err(error) => return report_storage_error(&error),
    };
    if options.json {
        print_ndjson(&events);
        if truncated {
            eprintln!("{}", truncation_note(run));
        }
    } else {
        let duration = duration_text(reader, run);
        print_human(run, &events, &duration, truncated);
    }
    exit_code(run.status, truncated)
}

/// 按过滤条件查询事件（严格 `ORDER BY seq ASC` 由 reader 保证）。
fn query_events(reader: &TraceReader, options: &TraceOptions) -> Result<Vec<EventRow>, TraceError> {
    if matches!(&options.kinds, Some(kinds) if kinds.is_empty()) {
        return Ok(Vec::new());
    }
    let query = EventQuery {
        since_seq: options.since_seq,
        limit: options.limit,
        kinds: options.kinds.clone(),
        include_streaming: !options.no_streaming,
    };
    reader.events(&options.run_id, &query)
}

/// 输出 NDJSON：一行一个事件，字段见 06 §6.10.6。
fn print_ndjson(events: &[EventRow]) {
    for event in events {
        let mut object = serde_json::Map::new();
        object.insert("seq".to_string(), serde_json::json!(event.seq));
        object.insert("at".to_string(), serde_json::json!(event.at.as_str()));
        object.insert("kind".to_string(), serde_json::json!(event.kind.as_str()));
        object.insert(
            "severity".to_string(),
            serde_json::json!(event.severity.as_str()),
        );
        object.insert(
            "run_id".to_string(),
            serde_json::json!(event.run_id.as_str()),
        );
        if let Some(instance_id) = &event.instance_id {
            object.insert(
                "instance_id".to_string(),
                serde_json::json!(instance_id.as_str()),
            );
        }
        if let Some(node_id) = &event.node_id {
            object.insert("node_id".to_string(), serde_json::json!(node_id.as_str()));
        }
        object.insert("payload".to_string(), event.payload.clone());
        println!("{}", Value::Object(object));
    }
}

/// 人类可读回放：头部两行 + `seq at kind node instance detail` 表格（06 §6.10.4）。
fn print_human(run: &RunRow, events: &[EventRow], duration: &str, truncated: bool) {
    println!(
        "run {}  status={}  graph={}  hash={}",
        run.run_id,
        run.status.as_str(),
        run.graph_name.as_deref().unwrap_or("-"),
        run.graph_hash
    );
    println!(
        "started {}  finished {}  duration {}  events {}  dropped {}  abandoned {}",
        run.started_at,
        run.finished_at.as_deref().unwrap_or("-"),
        duration,
        run.event_count,
        run.streaming_dropped,
        run.abandoned_tasks
    );
    println!();
    println!(
        "{:>6}  {:<22}  {:<13}  {:<16}  {:<14}  detail",
        "seq", "at", "kind", "node", "instance"
    );
    for event in events {
        println!(
            "{:>6}  {:<22}  {:<13}  {:<16}  {:<14}  {}",
            event.seq,
            clock_time(&event.at),
            event.kind,
            event.node_id.as_deref().unwrap_or("-"),
            short_id(event.instance_id.as_deref()),
            event_detail(event)
        );
    }
    if truncated {
        println!();
        println!("{}", truncation_note(run));
    }
}

/// run 时长文本；`finished_at` 缺失或解析失败时为 `-`。
fn duration_text(reader: &TraceReader, run: &RunRow) -> String {
    if run.finished_at.is_none() {
        return "-".to_string();
    }
    reader
        .stats(&run.run_id)
        .ok()
        .and_then(|stats| stats.duration_ms)
        .map(|millis| format!("{millis}ms"))
        .unwrap_or_else(|| "-".to_string())
}

/// 截断提示（06 §6.8.3）；优先带 `recovered_at`。
fn truncation_note(run: &RunRow) -> String {
    match &run.recovered_at {
        Some(at) => format!("!! trace truncated: run interrupted at {at} (no RunFinished)"),
        None => "!! trace truncated: run interrupted (no RunFinished)".to_string(),
    }
}

/// 退出码：完整回放且 Completed/Cancelled=0；Failed/interrupted=1（06 §6.10.4）。
fn exit_code(status: RunStatus, truncated: bool) -> ExitCode {
    if truncated || status == RunStatus::Failed {
        return ExitCode::from(EXIT_OTHER);
    }
    ExitCode::from(EXIT_SUCCESS)
}

/// `trace --stats`：06 §6.11.2 的人类文本或 JSON 对象。
fn run_stats(
    reader: &TraceReader,
    options: &TraceOptions,
    run: &RunRow,
    truncated: bool,
) -> ExitCode {
    let stats = match reader.stats(&run.run_id) {
        Ok(stats) => stats,
        Err(error) => return report_storage_error(&error),
    };
    if options.json {
        print_stats_json(&stats);
    } else {
        print_stats_human(&stats);
        if truncated {
            println!();
            println!("{}", truncation_note(run));
        }
    }
    exit_code(run.status, truncated)
}

/// `--stats` 人类可读输出（06 §6.11.2）。
fn print_stats_human(stats: &RunStats) {
    println!(
        "run {} status={} duration={}",
        stats.run_id,
        stats.status.as_str(),
        format_duration_ms(stats.duration_ms)
    );
    println!(
        "events {}{}",
        stats.events_total,
        format_kind_counts(&stats.kinds)
    );
    println!("{}", format_instance_counts(&stats.instances));
    println!(
        "streaming segments {}  dropped {}",
        stats.streaming_segments, stats.streaming_dropped
    );
    println!(
        "timeouts {}  abandoned {}  trace_backpressure {}ms",
        stats.timeouts, stats.abandoned, stats.trace_backpressure
    );
    println!(
        "db {}  size {}  runs {}",
        stats.db_path,
        format_size(stats.db_size_bytes),
        stats.db_runs
    );
}

/// `--stats --json` 输出对象（06 §6.11.2）。
fn print_stats_json(stats: &RunStats) {
    let kinds: serde_json::Map<String, Value> = stats
        .kinds
        .iter()
        .map(|(kind, count)| (kind.clone(), serde_json::json!(count)))
        .collect();
    let instances: serde_json::Map<String, Value> = stats
        .instances
        .iter()
        .map(|(status, count)| (status.clone(), serde_json::json!(count)))
        .collect();
    let value = serde_json::json!({
        "run_id": stats.run_id.as_str(),
        "status": stats.status.as_str(),
        "duration_ms": stats.duration_ms,
        "events": {"kinds": kinds, "total": stats.events_total},
        "instances": {"status": instances},
        "streaming": {"segments": stats.streaming_segments, "dropped": stats.streaming_dropped},
        "timeouts": stats.timeouts,
        "abandoned": stats.abandoned,
        "trace_backpressure": stats.trace_backpressure,
        "db": {
            "path": stats.db_path.as_str(),
            "size_bytes": stats.db_size_bytes,
            "runs": stats.db_runs,
        },
    });
    println!("{value}");
}

/// `RunStats.events.kinds` 的括号摘要（BTreeMap 已按 kind 排序）。
fn format_kind_counts(kinds: &BTreeMap<String, u64>) -> String {
    if kinds.is_empty() {
        return String::new();
    }
    let items: Vec<String> = kinds
        .iter()
        .map(|(kind, count)| format!("{kind} {count}"))
        .collect();
    format!("  ({})", items.join(", "))
}

/// 实例状态计数行：`instances 6 (Completed 5, Skipped 1)`。
fn format_instance_counts(instances: &BTreeMap<String, u64>) -> String {
    let total: u64 = instances.values().sum();
    if instances.is_empty() {
        return format!("instances {total}");
    }
    let items: Vec<String> = instances
        .iter()
        .map(|(status, count)| format!("{status} {count}"))
        .collect();
    format!("instances {total} ({})", items.join(", "))
}

/// 时长显示：`1842ms`；未知为 `-`。
fn format_duration_ms(duration_ms: Option<u64>) -> String {
    duration_ms
        .map(|millis| format!("{millis}ms"))
        .unwrap_or_else(|| "-".to_string())
}

/// 字节数显示（06 §6.11.2 示例 `1.4 MB`）。
fn format_size(bytes: u64) -> String {
    /// 1 KiB。
    const KB: f64 = 1024.0;
    /// 1 MiB。
    const MB: f64 = 1024.0 * 1024.0;
    /// 1 GiB。
    const GB: f64 = 1024.0 * 1024.0 * 1024.0;
    let value = bytes as f64;
    if value >= GB {
        format!("{:.1} GB", value / GB)
    } else if value >= MB {
        format!("{:.1} MB", value / MB)
    } else if value >= KB {
        format!("{:.1} KB", value / KB)
    } else {
        format!("{bytes} B")
    }
}

/// 只取 RFC3339 的时间部分（示例表格列 `09:12:03.104Z`）。
fn clock_time(at: &str) -> &str {
    match at.split_once('T') {
        Some((_, clock)) => clock,
        None => at,
    }
}

/// 实例 id 缩短为 `前8...末2`（06 §6.10.4 示例）；`None` 显示 `-`。
fn short_id(instance_id: Option<&str>) -> String {
    let Some(id) = instance_id else {
        return "-".to_string();
    };
    let count = id.chars().count();
    if count <= 14 {
        return id.to_string();
    }
    let head: String = id.chars().take(8).collect();
    let tail: String = id.chars().skip(count - 2).collect();
    format!("{head}...{tail}")
}

/// 事件行 detail 列：按 kind 渲染摘要（06 §6.10.4）。
fn event_detail(event: &EventRow) -> String {
    match event.kind.as_str() {
        "RunStarted" => run_started_detail(&event.payload),
        "Transition" => transition_detail(&event.payload),
        "GateResult" => gate_detail(&event.payload),
        "NodeTimedOut" => timed_out_detail(&event.payload),
        "NodeAbandoned" => abandoned_detail(&event.payload),
        "Streaming" => streaming_detail(&event.payload),
        "RunFinished" => run_finished_detail(&event.payload),
        _ => event.payload.to_string(),
    }
}

/// 取 payload 内的 `record`（Transition/GateResult 的事件信封形状）。
fn record(payload: &Value) -> &Value {
    payload.get("record").unwrap_or(payload)
}

fn field_str<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value.get(key).and_then(Value::as_str)
}

fn field_i64(value: &Value, key: &str) -> Option<i64> {
    value.get(key).and_then(Value::as_i64)
}

fn field_u64(value: &Value, key: &str) -> Option<u64> {
    value.get(key).and_then(Value::as_u64)
}

fn field_bool(value: &Value, key: &str) -> Option<bool> {
    value.get(key).and_then(Value::as_bool)
}

/// `RunStarted` detail：`status=Running user=local`。
fn run_started_detail(payload: &Value) -> String {
    format!(
        "status={} user={}",
        field_str(payload, "status").unwrap_or("-"),
        field_str(payload, "user_id").unwrap_or("-")
    )
}

/// `Transition` detail：`Pending -> Ready (inputs ready)`。
fn transition_detail(payload: &Value) -> String {
    let record = record(payload);
    let from = field_str(record, "from").unwrap_or("-");
    let to = field_str(record, "to").unwrap_or("-");
    match field_str(record, "reason") {
        Some(reason) => format!("{from} -> {to} ({reason})"),
        None => format!("{from} -> {to}"),
    }
}

/// `GateResult` detail：`edge#2 gate=gates.x passed=false`。
fn gate_detail(payload: &Value) -> String {
    let record = record(payload);
    let edge = field_i64(record, "edge_index")
        .map(|index| index.to_string())
        .unwrap_or_else(|| "-".to_string());
    let gate = field_str(record, "gate_fn").unwrap_or("-");
    let passed = field_bool(record, "passed").unwrap_or(false);
    let mut detail = format!("edge#{edge} gate={gate} passed={passed}");
    if let Some(error) = field_str(record, "error") {
        detail.push_str(" error=");
        detail.push_str(error);
    }
    detail
}

/// `NodeTimedOut` detail：`elapsed=1000ms`。
fn timed_out_detail(payload: &Value) -> String {
    match field_u64(payload, "elapsed_ms") {
        Some(elapsed) => format!("elapsed={elapsed}ms"),
        None => "elapsed=-".to_string(),
    }
}

/// `NodeAbandoned` detail：`at=<时间>`。
fn abandoned_detail(payload: &Value) -> String {
    format!("at={}", field_str(payload, "at").unwrap_or("-"))
}

/// `Streaming` detail：变体摘要（06 §6.10.4）。
fn streaming_detail(payload: &Value) -> String {
    let variant = field_str(payload, "variant").unwrap_or("-");
    let segment = field_i64(payload, "segment_index").unwrap_or(0);
    match variant {
        "token" | "reasoning" => {
            let chars = field_i64(payload, "chars").unwrap_or(0);
            let deltas = field_i64(payload, "delta_count").unwrap_or(0);
            format!("{variant}[{segment}] {chars} chars ({deltas} deltas)")
        }
        "progress" => {
            let percent = field_i64(payload, "percent").unwrap_or(0);
            let message = field_str(payload, "message").unwrap_or("");
            format!("progress {percent}% \"{message}\"")
        }
        "tool_call_start" => format!(
            "tool_call_start {} call_id={}",
            field_str(payload, "tool_name").unwrap_or("-"),
            field_str(payload, "call_id").unwrap_or("-")
        ),
        "tool_call_end" => format!(
            "tool_call_end {} ok={}",
            field_str(payload, "tool_name").unwrap_or("-"),
            field_bool(payload, "ok").unwrap_or(false)
        ),
        _ => format!("{variant}[{segment}]"),
    }
}

/// `RunFinished` detail：`status=Completed`（失败时附 error code）。
fn run_finished_detail(payload: &Value) -> String {
    let status = field_str(payload, "status").unwrap_or("-");
    match payload.get("error") {
        Some(error) => format!(
            "status={status} error={}",
            field_str(error, "code").unwrap_or("-")
        ),
        None => format!("status={status}"),
    }
}

/// 把 trace 存储错误映射为 `error[<CLI 码>]: <detail>` 并返回退出码 4。
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

#[cfg(test)]
mod tests {
    use super::{parse, resolve_db_path, short_id, EVENT_KINDS};
    use std::path::PathBuf;

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    #[test]
    fn parse_applies_defaults() {
        let options = parse(&args(&["run-1"])).expect("parse").expect("not help");
        assert_eq!(options.run_id, "run-1");
        assert_eq!(options.db, None);
        assert!(!options.json);
        assert_eq!(options.since_seq, 0);
        assert_eq!(options.limit, 1000);
        assert_eq!(options.kinds, None);
        assert!(!options.stats);
    }

    #[test]
    fn parse_accepts_all_options_in_any_order() {
        let options = parse(&args(&[
            "--json",
            "run-1",
            "--since-seq",
            "7",
            "--limit",
            "0",
            "--kinds",
            "Transition,RunFinished,Transition",
            "--db",
            "/tmp/t.db",
            "--stats",
            "-q",
        ]))
        .expect("parse")
        .expect("not help");
        assert_eq!(options.run_id, "run-1");
        assert_eq!(options.since_seq, 7);
        assert_eq!(options.limit, 0);
        assert!(options.json && options.stats && options.quiet);
        assert_eq!(options.db, Some(PathBuf::from("/tmp/t.db")));
        assert_eq!(
            options.kinds,
            Some(vec!["Transition".to_string(), "RunFinished".to_string()])
        );
    }

    #[test]
    fn parse_help_returns_none() {
        assert_eq!(parse(&args(&["run-1", "--help"])).expect("help"), None);
        assert_eq!(parse(&args(&["-h"])).expect("help"), None);
    }

    #[test]
    fn parse_rejects_bad_usage() {
        for values in [
            vec![],
            vec!["run-1", "--nope"],
            vec!["run-1", "--db"],
            vec!["run-1", "--limit", "x"],
            vec!["run-1", "--since-seq", "-1"],
            vec!["run-1", "--kinds", ""],
            vec!["run-1", "--kinds", "Nope"],
            vec!["a", "b"],
        ] {
            assert!(parse(&args(&values)).is_err(), "values={values:?}");
        }
    }

    #[test]
    fn no_streaming_removes_streaming_kind() {
        let options = parse(&args(&[
            "run-1",
            "--kinds",
            "Streaming,Transition",
            "--no-streaming",
        ]))
        .expect("parse")
        .expect("not help");
        assert_eq!(options.kinds, Some(vec!["Transition".to_string()]));
    }

    #[test]
    fn resolve_db_path_prefers_explicit() {
        assert_eq!(
            resolve_db_path(Some(std::path::Path::new("/tmp/x.db"))),
            PathBuf::from("/tmp/x.db")
        );
    }

    #[test]
    fn short_id_keeps_short_and_truncates_long() {
        assert_eq!(short_id(None), "-");
        assert_eq!(short_id(Some("abc")), "abc");
        assert_eq!(
            short_id(Some("0192f0c5-7a2e-7c31-9f0d-5b1c2a3d4e5f")),
            "0192f0c5...5f"
        );
    }

    #[test]
    fn event_kinds_are_the_seven_contract_kinds() {
        assert_eq!(EVENT_KINDS.len(), 7);
        assert!(EVENT_KINDS.contains(&"RunStarted"));
        assert!(EVENT_KINDS.contains(&"RunFinished"));
    }
}
