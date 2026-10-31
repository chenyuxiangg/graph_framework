//! `zgraph decide` 子命令：写入一行人工决策（W3a / DR-006 §4.5/§7.2）。
//!
//! 语义依据：`doc/decisions/DR-006-w3a-human-decide-freeze.md` §4.5（预检顺序）、
//! §4.6（decide 自动接续）、§5（`human_decisions` 与 C13 写者模型）与
//! `doc/spec-v1.2/06-存储与可观测.md` §6.10.8。
//!
//! # 边界（冻结）
//!
//! - **只写决策表**：不写事件表、不改 `runs.status`、不改实例状态；run 进程的
//!   decision consumer 轮询到该行后经控制通道自动接续（DR-006 §4.1/§4.6）。
//! - **不创建空库、不迁移**：经 trace `DecisionStore::open` 打开**已存在**的 v5 库；
//!   库不存在 / `user_version ≠ 5` / busy 重试耗尽 → exit 4 `HUMAN_DB_ERROR`
//!   （DR-006 §5.4 / E-3 / N20）。
//! - **预检顺序（C3 冻结，先于一切状态判断）**：① 该 `(run_id,instance_id)` 已有
//!   决策行 → exit 5 `HUMAN_DECISION_EXISTS`；② run 不存在/非 `Running|Paused` →
//!   exit 1 `HUMAN_RUN_NOT_ACTIVE`；③ instance 不存在（或不属于该 run）→ exit 1
//!   `HUMAN_INSTANCE_NOT_FOUND`；④ instance `status≠WaitingHuman` → exit 5
//!   `HUMAN_INSTANCE_NOT_WAITING`；唯一键冲突（并发窗口）→ exit 5。
//! - `HUMAN_*` 是 **CLI 命名空间**（`--json.code`），不写入 `runs.error_code`、
//!   不新增领域错误码（DR-006 §4.5）。
//! - `decision_id` 由 CLI 生成 UUIDv7（仅审计）；`decided_at` 为 UTC RFC3339
//!   毫秒 `Z` 时刻（与 trace 时间戳同口径）。

use std::path::PathBuf;
use std::process::ExitCode;

use serde_json::json;
use zgraph_trace::{DecisionError, DecisionStore, NewHumanDecision, TraceError};

use crate::trace_cmd::resolve_db_path;
use crate::{EXIT_CONFLICT, EXIT_OTHER, EXIT_STORAGE, EXIT_SUCCESS};

/// `decide --help` 的用法文本。
pub const DECIDE_USAGE: &str = "\
usage: zgraph decide <run_id> --instance <instance_id> --decision approved|rejected
                      --approver <id> [--reason <text>] [--json] [--db <path>]

options:
  --instance <id>       target instance (must be WaitingHuman)
  --decision <value>    approved | rejected
  --approver <id>       decision maker (non-empty)
  --reason <text>       optional decision reason (stored as-is)
  --db <path>           trace database (default: $ZGRAPH_DB or .zgraph/trace.db)
  --json                machine-readable output
  --help, -h            print this help";

/// `--json.code`：已有该实例决策行（唯一键冲突；exit 5）。
pub const CODE_HUMAN_DECISION_EXISTS: &str = "HUMAN_DECISION_EXISTS";
/// `--json.code`：run 不存在或非 `Running|Paused`（exit 1）。
pub const CODE_HUMAN_RUN_NOT_ACTIVE: &str = "HUMAN_RUN_NOT_ACTIVE";
/// `--json.code`：instance 不存在或不属于该 run（exit 1）。
pub const CODE_HUMAN_INSTANCE_NOT_FOUND: &str = "HUMAN_INSTANCE_NOT_FOUND";
/// `--json.code`：instance 存在但非 `WaitingHuman`（exit 5）。
pub const CODE_HUMAN_INSTANCE_NOT_WAITING: &str = "HUMAN_INSTANCE_NOT_WAITING";
/// `--json.code`：存储错误（打不开库/版本不符/busy 耗尽；exit 4）。
pub const CODE_HUMAN_DB_ERROR: &str = "HUMAN_DB_ERROR";

/// `--decision` 的闭集（与 `human_decisions.decision` CHECK 一致）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecisionValue {
    /// 批准。
    Approved,
    /// 驳回（仍是正常 Completed；去向交给图，DR-006 §4.4）。
    Rejected,
}

impl DecisionValue {
    /// 存储/输出用稳定字符串（`approved` / `rejected`）。
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Approved => "approved",
            Self::Rejected => "rejected",
        }
    }
}

/// `decide` 的参数（解析后）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecideArgs {
    /// 目标 run。
    pub run_id: String,
    /// 目标实例（必须属于该 run 且为 `WaitingHuman`）。
    pub instance_id: String,
    /// 决策值（闭集）。
    pub decision: DecisionValue,
    /// 决策人（非空、非全空白）。
    pub approver: String,
    /// 决策理由（`None` = 未提供 → NULL；提供则原样保存，不 trim）。
    pub reason: Option<String>,
    /// `--json`：stdout 只输出 JSON。
    pub json: bool,
    /// `--db` 显式库路径（缺省 `$ZGRAPH_DB` / `.zgraph/trace.db`）。
    pub db: Option<PathBuf>,
}

/// 解析 `decide` 之后的参数；`--help`/`-h` 返回 `Ok(None)`。
///
/// 值选项同时接受 `--flag value` 与 `--flag=value` 形式（与 `replay` 一致）。
/// 用法错误（缺参、`--decision` 非闭集、`--approver` 空/全空白）返回 `Err`，
/// 由 [`crate::dispatch`] 映射为退出码 2。
pub fn parse(args: &[String]) -> Result<Option<DecideArgs>, String> {
    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        return Ok(None);
    }
    let mut run_id: Option<String> = None;
    let mut instance_id: Option<String> = None;
    let mut decision: Option<DecisionValue> = None;
    let mut approver: Option<String> = None;
    let mut reason: Option<String> = None;
    let mut json_output = false;
    let mut db: Option<PathBuf> = None;
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        if let Some(value) = arg.strip_prefix("--instance=") {
            instance_id = Some(value.to_string());
            continue;
        }
        if let Some(value) = arg.strip_prefix("--decision=") {
            decision = Some(parse_decision(value)?);
            continue;
        }
        if let Some(value) = arg.strip_prefix("--approver=") {
            approver = Some(value.to_string());
            continue;
        }
        if let Some(value) = arg.strip_prefix("--reason=") {
            reason = Some(value.to_string());
            continue;
        }
        if let Some(value) = arg.strip_prefix("--db=") {
            db = Some(PathBuf::from(value));
            continue;
        }
        match arg.as_str() {
            "--instance" => {
                instance_id = Some(option_value(&mut iter, "--instance")?.to_string());
            }
            "--decision" => {
                decision = Some(parse_decision(option_value(&mut iter, "--decision")?)?);
            }
            "--approver" => approver = Some(option_value(&mut iter, "--approver")?.to_string()),
            "--reason" => reason = Some(option_value(&mut iter, "--reason")?.to_string()),
            "--db" => db = Some(PathBuf::from(option_value(&mut iter, "--db")?)),
            "--json" => json_output = true,
            flag if flag.starts_with('-') => return Err(format!("unknown option: {flag}")),
            value => {
                if run_id.is_some() {
                    return Err(format!("unexpected argument: {value}"));
                }
                run_id = Some(value.to_string());
            }
        }
    }
    let run_id = run_id.ok_or_else(|| "decide requires <run_id>".to_string())?;
    let instance_id = instance_id.ok_or_else(|| "decide requires --instance <id>".to_string())?;
    if instance_id.is_empty() {
        return Err("--instance requires a non-empty value".to_string());
    }
    let decision =
        decision.ok_or_else(|| "decide requires --decision approved|rejected".to_string())?;
    let approver = approver.ok_or_else(|| "decide requires --approver <id>".to_string())?;
    if approver.trim().is_empty() {
        return Err("--approver must not be empty".to_string());
    }
    Ok(Some(DecideArgs {
        run_id,
        instance_id,
        decision,
        approver,
        reason,
        json: json_output,
        db,
    }))
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

/// 解析 `--decision` 闭集（大小写敏感；非法值 → 用法错误）。
fn parse_decision(value: &str) -> Result<DecisionValue, String> {
    match value {
        "approved" => Ok(DecisionValue::Approved),
        "rejected" => Ok(DecisionValue::Rejected),
        other => Err(format!(
            "invalid --decision '{other}' (expected approved|rejected)"
        )),
    }
}

/// 执行 `decide` 并返回进程退出码（DR-006 §4.5 预检顺序）。
pub fn run(args: &DecideArgs) -> ExitCode {
    let db_path = resolve_db_path(args.db.as_deref());
    let mut store = match DecisionStore::open(&db_path) {
        Ok(store) => store,
        Err(error) => return fail_db(args.json, &error),
    };
    let precheck = match store.precheck(&args.run_id, &args.instance_id) {
        Ok(precheck) => precheck,
        Err(error) => return fail_db(args.json, &error),
    };
    // ① 决策行存在性先于一切 run/instance 状态判断（二次 decide 恒 DECISION_EXISTS）。
    if precheck.decision_exists {
        return fail(
            args.json,
            CODE_HUMAN_DECISION_EXISTS,
            &format!(
                "human decision already recorded for run={} instance={}",
                args.run_id, args.instance_id
            ),
            EXIT_CONFLICT,
        );
    }
    // ② run 存在且 status ∈ {Running, Paused}（终态拒绝 exit 1）。
    match precheck.run_status {
        Some(zgraph_trace::RunStatus::Running | zgraph_trace::RunStatus::Paused) => {}
        Some(status) => {
            return fail(
                args.json,
                CODE_HUMAN_RUN_NOT_ACTIVE,
                &format!(
                    "run {} is not active (status={})",
                    args.run_id,
                    status.as_str()
                ),
                EXIT_OTHER,
            )
        }
        None => {
            return fail(
                args.json,
                CODE_HUMAN_RUN_NOT_ACTIVE,
                &format!("run not found: {}", args.run_id),
                EXIT_OTHER,
            )
        }
    }
    // ③ instance 存在（且属于该 run）；不存在 → exit 1。
    let Some(status) = precheck.instance_status else {
        return fail(
            args.json,
            CODE_HUMAN_INSTANCE_NOT_FOUND,
            &format!(
                "instance not found: {} (run {})",
                args.instance_id, args.run_id
            ),
            EXIT_OTHER,
        );
    };
    // ④ instance status 必须为 WaitingHuman；否则 exit 5。
    if status != zgraph_trace::InstanceStatus::WaitingHuman {
        return fail(
            args.json,
            CODE_HUMAN_INSTANCE_NOT_WAITING,
            &format!(
                "instance {} is not WaitingHuman (status={})",
                args.instance_id,
                status.as_str()
            ),
            EXIT_CONFLICT,
        );
    }
    let decision_id = uuid::Uuid::now_v7().to_string();
    let decided_at = now_rfc3339();
    let new = NewHumanDecision {
        run_id: args.run_id.clone(),
        instance_id: args.instance_id.clone(),
        decision: args.decision.as_str().to_string(),
        approver: args.approver.clone(),
        reason: args.reason.clone(),
        decision_id: decision_id.clone(),
        decided_at: decided_at.clone(),
    };
    let inserted = match store.insert(new) {
        Ok(row) => row,
        // 并发窗口：另一进程已插入同一 `(run_id, instance_id)` → 唯一键冲突。
        Err(DecisionError::Conflict(message)) => {
            return fail(
                args.json,
                CODE_HUMAN_DECISION_EXISTS,
                &message,
                EXIT_CONFLICT,
            )
        }
        Err(DecisionError::Trace(error)) => return fail_db(args.json, &error),
    };
    if args.json {
        println!(
            "{}",
            json!({
                "ok": true,
                "run_id": inserted.run_id,
                "instance_id": inserted.instance_id,
                "decision": inserted.decision,
                "decision_id": inserted.decision_id,
                "decided_at": inserted.decided_at,
            })
        );
    } else {
        println!(
            "decision recorded: run={} instance={} decision={} decision_id={}",
            inserted.run_id, inserted.instance_id, inserted.decision, inserted.decision_id
        );
    }
    ExitCode::from(EXIT_SUCCESS)
}

/// 当前 UTC RFC3339 毫秒字符串（`Z` 结尾；与 trace 时间戳同口径）。
pub fn now_rfc3339() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

/// 业务失败输出：`--json` 只写 stdout JSON；人类模式写 stderr `error[CODE]: ...`。
fn fail(json_output: bool, code: &str, message: &str, exit_code: u8) -> ExitCode {
    if json_output {
        println!("{}", json!({"ok": false, "code": code, "message": message}));
    } else {
        eprintln!("error[{code}]: {message}");
    }
    ExitCode::from(exit_code)
}

/// 存储错误 → exit 4 `HUMAN_DB_ERROR`（v<5 附「先由 `zgraph run` 迁移」提示）。
fn fail_db(json_output: bool, error: &TraceError) -> ExitCode {
    let mut message = crate::trace_cmd::storage_error_detail(error);
    if message.starts_with("unsupported schema version") {
        message.push_str("; run 'zgraph run' to migrate the database first");
    }
    fail(json_output, CODE_HUMAN_DB_ERROR, &message, EXIT_STORAGE)
}

#[cfg(test)]
mod tests {
    use super::{now_rfc3339, parse, parse_decision, DecideArgs, DecisionValue};
    use std::path::PathBuf;

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    #[test]
    fn parse_applies_required_fields_and_defaults() {
        let parsed = parse(&args(&[
            "run-1",
            "--instance",
            "inst-1",
            "--decision",
            "approved",
            "--approver",
            "alice",
        ]))
        .expect("parse")
        .expect("not help");
        assert_eq!(
            parsed,
            DecideArgs {
                run_id: "run-1".to_string(),
                instance_id: "inst-1".to_string(),
                decision: DecisionValue::Approved,
                approver: "alice".to_string(),
                reason: None,
                json: false,
                db: None,
            }
        );
    }

    #[test]
    fn parse_accepts_equals_forms_and_all_options() {
        let parsed = parse(&args(&[
            "--json",
            "run-2",
            "--instance=inst-2",
            "--decision=rejected",
            "--approver=bob",
            "--reason=needs rework",
            "--db=/tmp/t.db",
        ]))
        .expect("parse")
        .expect("not help");
        assert_eq!(parsed.run_id, "run-2");
        assert_eq!(parsed.instance_id, "inst-2");
        assert_eq!(parsed.decision, DecisionValue::Rejected);
        assert_eq!(parsed.approver, "bob");
        assert_eq!(parsed.reason.as_deref(), Some("needs rework"));
        assert!(parsed.json);
        assert_eq!(parsed.db, Some(PathBuf::from("/tmp/t.db")));
    }

    #[test]
    fn parse_accepts_reason_whitespace_as_is() {
        // E-8：`--reason` 提供即原样保存（不 trim）；未提供为 None。
        let parsed = parse(&args(&[
            "r",
            "--instance",
            "i",
            "--decision",
            "approved",
            "--approver",
            "a",
            "--reason",
            "   ",
        ]))
        .expect("parse")
        .expect("not help");
        assert_eq!(parsed.reason.as_deref(), Some("   "));
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
            vec!["run-1"],
            vec!["run-1", "--decision", "approved", "--approver", "a"],
            vec!["run-1", "--instance", "i", "--decision", "approved"],
            vec![
                "run-1",
                "--instance",
                "i",
                "--decision",
                "maybe",
                "--approver",
                "a",
            ],
            vec![
                "run-1",
                "--instance",
                "i",
                "--decision",
                "approved",
                "--approver",
                "",
            ],
            vec![
                "run-1",
                "--instance",
                "i",
                "--decision",
                "approved",
                "--approver",
                "   ",
            ],
            vec![
                "run-1",
                "--instance",
                "",
                "--decision",
                "approved",
                "--approver",
                "a",
            ],
            vec![
                "run-1",
                "--instance",
                "i",
                "--decision",
                "approved",
                "--approver",
                "a",
                "--nope",
            ],
            vec![
                "run-1",
                "run-2",
                "--instance",
                "i",
                "--decision",
                "approved",
                "--approver",
                "a",
            ],
            vec!["run-1", "--instance"],
            vec!["run-1", "--decision"],
            vec!["run-1", "--approver"],
            vec!["run-1", "--db"],
        ] {
            assert!(parse(&args(&values)).is_err(), "values={values:?}");
        }
    }

    #[test]
    fn parse_decision_is_case_sensitive_closed_set() {
        assert_eq!(
            parse_decision("approved").expect("approved"),
            DecisionValue::Approved
        );
        assert_eq!(
            parse_decision("rejected").expect("rejected"),
            DecisionValue::Rejected
        );
        assert!(parse_decision("APPROVED").is_err());
        assert!(parse_decision("").is_err());
    }

    #[test]
    fn decision_values_use_storage_literals() {
        assert_eq!(DecisionValue::Approved.as_str(), "approved");
        assert_eq!(DecisionValue::Rejected.as_str(), "rejected");
    }

    #[test]
    fn now_rfc3339_is_utc_millis_with_z_suffix() {
        let now = now_rfc3339();
        assert!(now.ends_with('Z'), "{now}");
        assert_eq!(now.len(), "2026-10-31T10:00:00.000Z".len(), "{now}");
        assert_eq!(&now[4..5], "-", "{now}");
        assert_eq!(&now[19..20], ".", "{now}");
    }
}
