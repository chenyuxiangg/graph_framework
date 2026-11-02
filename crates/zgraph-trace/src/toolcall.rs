//! v7 `tool_calls`（DR-014 §1/§5）写入前的字段归一化：JSON 序列化、上限截断与
//! 全量 sha256。
//!
//! 冻结口径（DR-014 §5 / 00 §0.5）：
//! - `args_json`/`result_json`/`error_json`：≤32 KiB；超限存 **prefix** + `truncated=1`，
//!   `args_sha256`/`result_sha256` 始终记录**完整**内容哈希（回放遇截断 fail-closed）；
//! - `bash_stdout`/`bash_stderr`：各 ≤16 KiB，**head+tail** + 中间截断标记，
//!   `bash_truncated=1`；
//! - `bash_command` ≤8 KiB、`side_effect_path`/`bash_cwd` ≤4 KiB：工具层应已拒绝
//!   超限（DR-014 §5），trace 侧防御性截断并置对应 truncated 标记（审计可见）。
//!
//! 本模块只做字节级归一化（无 blobs/外部文件）；行数上限在 writer 侧按 run 计数。

use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::error::TraceError;
use crate::model::ToolCallRecord;
use crate::schema::{
    TOOL_CALL_ARGS_MAX_BYTES, TOOL_CALL_BASH_OUTPUT_MAX_BYTES, TOOL_CALL_COMMAND_MAX_BYTES,
    TOOL_CALL_PATH_MAX_BYTES, TOOL_CALL_RESULT_MAX_BYTES,
};

/// head+tail 截断时插入的中间标记（UTF-8，13 字节）。
const TRUNCATION_MARKER: &str = "…[truncated]…";

/// bash 记录归一化后的文本字段。
pub(crate) struct NormalizedBash {
    pub(crate) command: String,
    pub(crate) cwd: String,
    pub(crate) stdout: String,
    pub(crate) stderr: String,
}

/// 一条 `tool_calls` 行的归一化结果（writer 直接绑定 INSERT 参数）。
pub(crate) struct NormalizedToolCall {
    pub(crate) args_json: String,
    pub(crate) args_sha256: String,
    pub(crate) result_json: Option<String>,
    pub(crate) result_sha256: Option<String>,
    pub(crate) error_json: Option<String>,
    pub(crate) truncated: bool,
    pub(crate) side_effect_path: Option<String>,
    pub(crate) bash: Option<NormalizedBash>,
    pub(crate) bash_truncated: bool,
}

/// 归一化一次工具调用记录（不修改入参；序列化失败 → [`TraceError::Serialize`]）。
pub(crate) fn normalize(record: &ToolCallRecord) -> Result<NormalizedToolCall, TraceError> {
    let args_text = json_text(&record.args)?;
    let args_sha256 = sha256_hex(args_text.as_bytes());
    let (args_json, args_cut) = truncate_prefix(&args_text, TOOL_CALL_ARGS_MAX_BYTES);
    let mut truncated = args_cut;

    let (result_json, result_sha256) = match &record.result {
        Some(value) => {
            let text = json_text(value)?;
            let sha = sha256_hex(text.as_bytes());
            let (stored, cut) = truncate_prefix(&text, TOOL_CALL_RESULT_MAX_BYTES);
            truncated |= cut;
            (Some(stored), Some(sha))
        }
        None => (None, None),
    };

    let error_json = match &record.error {
        Some(value) => {
            let text = json_text(value)?;
            let (stored, cut) = truncate_prefix(&text, TOOL_CALL_RESULT_MAX_BYTES);
            truncated |= cut;
            Some(stored)
        }
        None => None,
    };

    let side_effect_path = match &record.side_effect {
        Some(side_effect) => {
            let (stored, cut) = truncate_prefix(&side_effect.path, TOOL_CALL_PATH_MAX_BYTES);
            truncated |= cut;
            Some(stored)
        }
        None => None,
    };

    let mut bash_truncated = false;
    let bash = match &record.bash {
        Some(bash) => {
            let (command, command_cut) =
                truncate_head_tail(&bash.command, TOOL_CALL_COMMAND_MAX_BYTES);
            bash_truncated |= command_cut;
            let (cwd, cwd_cut) = truncate_prefix(&bash.cwd, TOOL_CALL_PATH_MAX_BYTES);
            bash_truncated |= cwd_cut;
            let (stdout, stdout_cut) =
                truncate_head_tail(&bash.stdout, TOOL_CALL_BASH_OUTPUT_MAX_BYTES);
            bash_truncated |= stdout_cut;
            let (stderr, stderr_cut) =
                truncate_head_tail(&bash.stderr, TOOL_CALL_BASH_OUTPUT_MAX_BYTES);
            bash_truncated |= stderr_cut;
            Some(NormalizedBash {
                command,
                cwd,
                stdout,
                stderr,
            })
        }
        None => None,
    };

    Ok(NormalizedToolCall {
        args_json,
        args_sha256,
        result_json,
        result_sha256,
        error_json,
        truncated,
        side_effect_path,
        bash,
        bash_truncated,
    })
}

/// 序列化为 JSON 文本（与既有 trace TEXT 列同口径：非 canonical，仅审计）。
fn json_text(value: &Value) -> Result<String, TraceError> {
    serde_json::to_string(value).map_err(|err| TraceError::Serialize(err.to_string()))
}

/// 全量内容 sha256（小写 hex，64 字符）。
pub(crate) fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut out = String::with_capacity(64);
    for byte in digest {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

/// 保留前缀（≤ `max_bytes`，按 UTF-8 字符边界收窄）；返回是否发生截断。
///
/// 用于 args/result/error JSON 与 path：DR-014 冻结「prefix + truncated 标记」，
/// 不额外插入文本标记（JSON 已不可解析，消费方必须看 `truncated`）。
fn truncate_prefix(text: &str, max_bytes: usize) -> (String, bool) {
    if text.len() <= max_bytes {
        return (text.to_string(), false);
    }
    let mut end = max_bytes;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    (text[..end].to_string(), true)
}

/// 保留 head+tail（总长 ≤ `max_bytes`，中间插入 [`TRUNCATION_MARKER`]）；返回是否截断。
///
/// 用于 bash stdout/stderr/command：DR-014 冻结「16 KiB head+tail + 中间截断标记」。
fn truncate_head_tail(text: &str, max_bytes: usize) -> (String, bool) {
    if text.len() <= max_bytes {
        return (text.to_string(), false);
    }
    if max_bytes <= TRUNCATION_MARKER.len() {
        return truncate_prefix(text, max_bytes);
    }
    let budget = max_bytes - TRUNCATION_MARKER.len();
    let head_budget = budget / 2;
    let tail_budget = budget - head_budget;

    let mut head_end = head_budget;
    while head_end > 0 && !text.is_char_boundary(head_end) {
        head_end -= 1;
    }
    let mut tail_start = text.len() - tail_budget;
    while tail_start < text.len() && !text.is_char_boundary(tail_start) {
        tail_start += 1;
    }
    let mut out = String::with_capacity(max_bytes);
    out.push_str(&text[..head_end]);
    out.push_str(TRUNCATION_MARKER);
    out.push_str(&text[tail_start..]);
    (out, true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{
        BashToolCall, ToolCallRecord, ToolCallSource, ToolCallStatus, ToolSideEffect,
    };
    use serde_json::json;

    fn record() -> ToolCallRecord {
        ToolCallRecord {
            run_id: "run-1".to_string(),
            instance_id: "inst-1".to_string(),
            node_id: "agent".to_string(),
            attempt: 0,
            call_id: "call_abc".to_string(),
            tool_name: "builtin__read".to_string(),
            source: ToolCallSource::Builtin,
            status: ToolCallStatus::Ok,
            args: json!({"path": "a.txt"}),
            result: Some(json!({"content": "hi"})),
            error: None,
            started_at: "2026-11-02T10:00:00.000Z".to_string(),
            finished_at: "2026-11-02T10:00:01.000Z".to_string(),
            duration_ms: 1000,
            side_effect: None,
            bash: None,
        }
    }

    #[test]
    fn sha256_hex_matches_known_vectors() {
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn truncate_prefix_respects_byte_cap_and_char_boundaries() {
        assert_eq!(truncate_prefix("short", 16), ("short".to_string(), false));
        let (text, cut) = truncate_prefix("abcdef", 4);
        assert_eq!((text.as_str(), cut), ("abcd", true));
        // 多字节字符不得被切断（"é" = 2 bytes）。
        let (text, cut) = truncate_prefix("ééé", 3);
        assert_eq!((text.as_str(), cut), ("é", true));
        assert!(text.len() <= 3);
    }

    #[test]
    fn truncate_head_tail_keeps_both_ends_within_cap() {
        let input = format!("HEAD{}TAIL", "x".repeat(200));
        let (text, cut) = truncate_head_tail(&input, 64);
        assert!(cut);
        assert!(text.len() <= 64, "len={}", text.len());
        assert!(text.starts_with("HEAD"));
        assert!(text.ends_with("TAIL"));
        assert!(text.contains(TRUNCATION_MARKER));
        // 多字节边界安全。
        let long = "é".repeat(100);
        let (text, cut) = truncate_head_tail(&long, 31);
        assert!(cut);
        assert!(text.len() <= 31);
        assert!(text.contains(TRUNCATION_MARKER));
    }

    #[test]
    fn short_record_is_not_truncated_and_hashes_full_json() {
        let normalized = normalize(&record()).expect("normalize");
        assert!(!normalized.truncated);
        assert!(!normalized.bash_truncated);
        let args_text = serde_json::to_string(&record().args).expect("args json");
        assert_eq!(normalized.args_json, args_text);
        assert_eq!(normalized.args_sha256, sha256_hex(args_text.as_bytes()));
        let result_text = serde_json::to_string(&record().result.clone().unwrap()).expect("result");
        assert_eq!(
            normalized.result_json.as_deref(),
            Some(result_text.as_str())
        );
        assert_eq!(
            normalized.result_sha256.as_deref(),
            Some(sha256_hex(result_text.as_bytes()).as_str())
        );
        assert_eq!(normalized.error_json, None);
        assert_eq!(normalized.side_effect_path, None);
        assert!(normalized.bash.is_none());
    }

    #[test]
    fn oversized_args_result_error_are_truncated_but_full_hash_is_kept() {
        let mut record = record();
        record.args = json!({"blob": "a".repeat(TOOL_CALL_ARGS_MAX_BYTES)});
        record.result = Some(json!({"out": "b".repeat(TOOL_CALL_RESULT_MAX_BYTES)}));
        record.error = Some(json!({"message": "c".repeat(TOOL_CALL_RESULT_MAX_BYTES)}));

        let normalized = normalize(&record).expect("normalize");
        assert!(normalized.truncated, "超限必须置 truncated=1");
        assert!(normalized.args_json.len() <= TOOL_CALL_ARGS_MAX_BYTES);
        assert!(normalized.result_json.as_ref().unwrap().len() <= TOOL_CALL_RESULT_MAX_BYTES);
        assert!(normalized.error_json.as_ref().unwrap().len() <= TOOL_CALL_RESULT_MAX_BYTES);
        let full_args = serde_json::to_string(&record.args).expect("args");
        assert_eq!(normalized.args_sha256, sha256_hex(full_args.as_bytes()));
        let full_result = serde_json::to_string(record.result.as_ref().unwrap()).expect("result");
        assert_eq!(
            normalized.result_sha256.as_deref(),
            Some(sha256_hex(full_result.as_bytes()).as_str())
        );
    }

    #[test]
    fn side_effect_path_and_bash_fields_are_capped_and_flagged() {
        let mut record = record();
        record.tool_name = "builtin__bash".to_string();
        record.side_effect = Some(ToolSideEffect {
            path: "p".repeat(TOOL_CALL_PATH_MAX_BYTES + 10),
            before_sha256: Some("a".repeat(64)),
            after_sha256: Some("b".repeat(64)),
            bytes_before: Some(1),
            bytes_after: Some(2),
        });
        record.bash = Some(BashToolCall {
            command: "c".repeat(TOOL_CALL_COMMAND_MAX_BYTES + 10),
            cwd: "w".repeat(TOOL_CALL_PATH_MAX_BYTES + 10),
            exit_code: Some(0),
            signal: None,
            stdout: "o".repeat(TOOL_CALL_BASH_OUTPUT_MAX_BYTES + 100),
            stderr: "e".repeat(TOOL_CALL_BASH_OUTPUT_MAX_BYTES + 100),
            sandbox_profile_sha256: Some("d".repeat(64)),
        });

        let normalized = normalize(&record).expect("normalize");
        assert!(normalized.truncated, "path 防御性截断置 truncated");
        assert!(normalized.bash_truncated, "bash 超限置 bash_truncated");
        assert!(normalized.side_effect_path.as_ref().unwrap().len() <= TOOL_CALL_PATH_MAX_BYTES);
        let bash = normalized.bash.as_ref().expect("bash");
        assert!(bash.command.len() <= TOOL_CALL_COMMAND_MAX_BYTES);
        assert!(bash.cwd.len() <= TOOL_CALL_PATH_MAX_BYTES);
        assert!(bash.stdout.len() <= TOOL_CALL_BASH_OUTPUT_MAX_BYTES);
        assert!(bash.stderr.len() <= TOOL_CALL_BASH_OUTPUT_MAX_BYTES);
        assert!(bash.stdout.contains(TRUNCATION_MARKER));
        assert!(bash.stderr.contains(TRUNCATION_MARKER));
    }
}
