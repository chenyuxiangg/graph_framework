//! v7 `tool_calls`（DR-014 §1/§5）：写入/读取 roundtrip、cap/截断/全量 sha256、
//! 幂等唯一键、排序、单 run 行数上限，以及「不产生 events 行」。
//!
//! 语义依据：DR-014 §1（DDL/字段）、§5（上限/截断）、06 §6.15、07 AC-24 c–f/i。

mod common;

use serde_json::json;
use sha2::{Digest, Sha256};
use zgraph_trace::{
    BashToolCall, EventQuery, ToolCallRecord, ToolCallSource, ToolCallStatus, ToolSideEffect,
    TraceInput, TOOL_CALLS_MAX_PER_RUN, TOOL_CALL_ARGS_MAX_BYTES, TOOL_CALL_BASH_OUTPUT_MAX_BYTES,
    TOOL_CALL_COMMAND_MAX_BYTES, TOOL_CALL_PATH_MAX_BYTES, TOOL_CALL_RESULT_MAX_BYTES,
};

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut out = String::with_capacity(64);
    for byte in digest {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

fn record(run_id: &str, instance_id: &str, call_id: &str) -> ToolCallRecord {
    ToolCallRecord {
        run_id: run_id.to_string(),
        instance_id: instance_id.to_string(),
        node_id: "agent".to_string(),
        attempt: 0,
        call_id: call_id.to_string(),
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

fn json_text(value: &serde_json::Value) -> String {
    serde_json::to_string(value).expect("json")
}

#[test]
fn tool_call_roundtrip_all_scalar_fields_without_events_rows() {
    let fx = common::Fixture::new();
    fx.handle.send(common::started("run-tools"));
    let mut call = record("run-tools", "inst-tools", "call_native_1");
    call.tool_name = "mcp__srv__search".to_string();
    call.source = ToolCallSource::Mcp;
    call.args = json!({"query": "hello", "limit": 3});
    call.result = Some(json!({"hits": [1, 2, 3]}));
    call.duration_ms = 42;
    fx.handle.send(TraceInput::ToolCall(call.clone()));
    fx.flush();

    let reader = fx.reader();
    let calls = reader.tool_calls("run-tools").expect("tool calls");
    assert_eq!(calls.len(), 1);
    let row = &calls[0];
    assert_eq!(row.run_id, "run-tools");
    assert_eq!(row.instance_id, "inst-tools");
    assert_eq!(row.node_id, "agent");
    assert_eq!(row.attempt, 0);
    assert_eq!(row.call_id, "call_native_1");
    assert_eq!(row.tool_name, "mcp__srv__search");
    assert_eq!(row.source, ToolCallSource::Mcp);
    assert_eq!(row.status, ToolCallStatus::Ok);
    assert_eq!(row.args_json, json_text(&call.args));
    assert_eq!(
        row.result_json.as_deref(),
        Some(json_text(&call.result.clone().unwrap()).as_str())
    );
    assert_eq!(row.error_json, None);
    assert_eq!(row.error_sha256, None);
    assert!(!row.truncated);
    assert_eq!(
        row.args_sha256.as_deref(),
        Some(sha256_hex(json_text(&call.args).as_bytes()).as_str())
    );
    assert_eq!(
        row.result_sha256.as_deref(),
        Some(sha256_hex(json_text(&call.result.clone().unwrap()).as_bytes()).as_str())
    );
    assert_eq!(row.started_at, "2026-11-02T10:00:00.000Z");
    assert_eq!(row.finished_at, "2026-11-02T10:00:01.000Z");
    assert_eq!(row.duration_ms, 42);
    assert_eq!(row.bash_signal, None);
    assert_eq!(row.side_effect_path, None);
    assert_eq!(row.bash_command, None);
    assert!(!row.bash_truncated);

    // 不产生 events 行（events.kind 不变；tool_calls 是审计补充）。
    let events = reader
        .events("run-tools", &EventQuery::default())
        .expect("events");
    assert_eq!(events.len(), 1, "仅 RunStarted");
    assert_eq!(
        reader
            .run("run-tools")
            .expect("run")
            .expect("row")
            .event_count,
        1,
        "tool_calls 不计入 event_count"
    );
}

#[test]
fn args_result_error_are_capped_with_full_sha256_and_truncated_flag() {
    let fx = common::Fixture::new();
    fx.handle.send(common::started("run-cap"));
    let mut call = record("run-cap", "inst-cap", "call_cap");
    call.args = json!({"blob": "a".repeat(TOOL_CALL_ARGS_MAX_BYTES)});
    call.result = Some(json!({"out": "b".repeat(TOOL_CALL_RESULT_MAX_BYTES)}));
    call.error = Some(json!({"message": "c".repeat(TOOL_CALL_RESULT_MAX_BYTES)}));
    call.status = ToolCallStatus::Error;
    fx.handle.send(TraceInput::ToolCall(call.clone()));
    fx.flush();

    let reader = fx.reader();
    let rows = reader.tool_calls("run-cap").expect("tool calls");
    assert_eq!(rows.len(), 1);
    let row = &rows[0];
    assert!(row.truncated, "超限必须 truncated=1（回放 fail-closed）");
    assert!(row.args_json.len() <= TOOL_CALL_ARGS_MAX_BYTES);
    assert!(row.result_json.as_ref().unwrap().len() <= TOOL_CALL_RESULT_MAX_BYTES);
    assert!(row.error_json.as_ref().unwrap().len() <= TOOL_CALL_RESULT_MAX_BYTES);
    // 全量 sha256 仍可证明记录不完整。
    assert_eq!(
        row.args_sha256.as_deref(),
        Some(sha256_hex(json_text(&call.args).as_bytes()).as_str())
    );
    assert_eq!(
        row.result_sha256.as_deref(),
        Some(sha256_hex(json_text(&call.result.clone().unwrap()).as_bytes()).as_str())
    );
    assert_eq!(
        row.error_sha256.as_deref(),
        Some(sha256_hex(json_text(&call.error.clone().unwrap()).as_bytes()).as_str()),
        "error 超限时 error_sha256 必须是全量 sha256（REV-109 C2）"
    );
    assert_eq!(row.status, ToolCallStatus::Error);
}

#[test]
fn error_sha256_roundtrips_for_error_results() {
    let fx = common::Fixture::new();
    fx.handle.send(common::started("run-error-hash"));
    let mut call = record("run-error-hash", "inst", "call-error");
    call.status = ToolCallStatus::Error;
    call.result = None;
    call.error = Some(json!({"code": "TOOL_ERROR", "reason": "jail_denied"}));
    fx.handle.send(TraceInput::ToolCall(call.clone()));
    fx.flush();

    let reader = fx.reader();
    let rows = reader.tool_calls("run-error-hash").expect("tool calls");
    let row = &rows[0];
    assert_eq!(
        row.error_json.as_deref(),
        Some(json_text(call.error.as_ref().unwrap()).as_str())
    );
    assert_eq!(
        row.error_sha256.as_deref(),
        Some(sha256_hex(json_text(call.error.as_ref().unwrap()).as_bytes()).as_str())
    );
    assert_eq!(row.result_sha256, None);
}

#[test]
fn bash_fields_are_capped_head_tail_and_keep_exit_signal_profile() {
    let fx = common::Fixture::new();
    fx.handle.send(common::started("run-bash"));
    let mut call = record("run-bash", "inst-bash", "call_bash");
    call.tool_name = "builtin__bash".to_string();
    call.args = json!({"command": "ls"});
    call.result = Some(json!({"exit_code": 0}));
    call.bash = Some(BashToolCall {
        command: "c".repeat(TOOL_CALL_COMMAND_MAX_BYTES + 10),
        cwd: "/tmp/workspace".to_string(),
        exit_code: Some(0),
        signal: Some(15),
        stdout: "o".repeat(TOOL_CALL_BASH_OUTPUT_MAX_BYTES + 100),
        stderr: "e".repeat(TOOL_CALL_BASH_OUTPUT_MAX_BYTES + 100),
        sandbox_profile_sha256: Some("d".repeat(64)),
    });
    fx.handle.send(TraceInput::ToolCall(call));
    fx.flush();

    let reader = fx.reader();
    let rows = reader.tool_calls("run-bash").expect("tool calls");
    assert_eq!(rows.len(), 1);
    let row = &rows[0];
    assert!(row.bash_truncated, "bash 超限必须 bash_truncated=1");
    assert!(row.bash_command.as_ref().unwrap().len() <= TOOL_CALL_COMMAND_MAX_BYTES);
    assert_eq!(row.bash_cwd.as_deref(), Some("/tmp/workspace"));
    assert_eq!(row.bash_exit_code, Some(0));
    assert_eq!(row.bash_signal, Some(15));
    assert!(row.bash_stdout.as_ref().unwrap().contains("truncated"));
    assert!(row.bash_stderr.as_ref().unwrap().contains("truncated"));
    assert!(row.bash_stdout.as_ref().unwrap().len() <= TOOL_CALL_BASH_OUTPUT_MAX_BYTES);
    assert!(row.bash_stderr.as_ref().unwrap().len() <= TOOL_CALL_BASH_OUTPUT_MAX_BYTES);
    assert_eq!(
        row.sandbox_profile_sha256.as_deref(),
        Some("d".repeat(64).as_str())
    );
}

#[test]
fn side_effect_fields_roundtrip_and_long_path_is_capped() {
    let fx = common::Fixture::new();
    fx.handle.send(common::started("run-side"));
    let mut call = record("run-side", "inst-side", "call_side");
    call.tool_name = "builtin__write".to_string();
    call.side_effect = Some(ToolSideEffect {
        path: "workspace/src/main.rs".to_string(),
        before_sha256: Some("a".repeat(64)),
        after_sha256: Some("b".repeat(64)),
        bytes_before: Some(10),
        bytes_after: Some(20),
    });
    fx.handle.send(TraceInput::ToolCall(call));
    fx.flush();
    let reader = fx.reader();
    let rows = reader.tool_calls("run-side").expect("tool calls");
    let row = &rows[0];
    assert_eq!(
        row.side_effect_path.as_deref(),
        Some("workspace/src/main.rs")
    );
    assert_eq!(
        row.side_effect_before.as_deref(),
        Some("a".repeat(64).as_str())
    );
    assert_eq!(
        row.side_effect_after.as_deref(),
        Some("b".repeat(64).as_str())
    );
    assert_eq!(row.side_effect_bytes_before, Some(10));
    assert_eq!(row.side_effect_bytes_after, Some(20));

    // 防御性 path 截断：置 truncated（回放 fail-closed）。
    let fx = common::Fixture::new();
    fx.handle.send(common::started("run-side2"));
    let mut call = record("run-side2", "inst-side2", "call_side2");
    call.side_effect = Some(ToolSideEffect {
        path: "p".repeat(TOOL_CALL_PATH_MAX_BYTES + 1),
        before_sha256: None,
        after_sha256: None,
        bytes_before: None,
        bytes_after: None,
    });
    fx.handle.send(TraceInput::ToolCall(call));
    fx.flush();
    let reader = fx.reader();
    let rows = reader.tool_calls("run-side2").expect("tool calls");
    assert!(rows[0].truncated);
    assert!(rows[0].side_effect_path.as_ref().unwrap().len() <= TOOL_CALL_PATH_MAX_BYTES);
}

#[test]
fn source_and_status_values_roundtrip_through_closed_sets() {
    let fx = common::Fixture::new();
    fx.handle.send(common::started("run-enums"));
    let sources = [
        ToolCallSource::Builtin,
        ToolCallSource::Mcp,
        ToolCallSource::Python,
    ];
    let statuses = [
        ToolCallStatus::Ok,
        ToolCallStatus::Error,
        ToolCallStatus::Cancelled,
        ToolCallStatus::Timeout,
    ];
    let mut expected = Vec::new();
    for (source_index, source) in sources.iter().enumerate() {
        for (status_index, status) in statuses.iter().enumerate() {
            let call_id = format!("call_s{source_index}_t{status_index}");
            let mut call = record("run-enums", "inst-enums", &call_id);
            call.source = *source;
            call.status = *status;
            expected.push((call_id, *source, *status));
            fx.handle.send(TraceInput::ToolCall(call));
        }
    }
    fx.flush();
    let reader = fx.reader();
    let rows = reader.tool_calls("run-enums").expect("tool calls");
    assert_eq!(rows.len(), 12);
    for row in &rows {
        let (_, source, status) = expected
            .iter()
            .find(|(call_id, _, _)| call_id == &row.call_id)
            .expect("expected record");
        assert_eq!(row.source, *source);
        assert_eq!(row.status, *status);
    }
}

#[test]
fn duplicate_call_id_is_idempotent_and_attempt_changes_locator() {
    let fx = common::Fixture::new();
    fx.handle.send(common::started("run-dup"));
    fx.handle
        .send(TraceInput::ToolCall(record("run-dup", "inst", "call-1")));
    // 完全相同的定位键：幂等丢弃（不产生第二行）。
    fx.handle
        .send(TraceInput::ToolCall(record("run-dup", "inst", "call-1")));
    // attempt 不同 → 新行。
    let mut second_attempt = record("run-dup", "inst", "call-1");
    second_attempt.attempt = 1;
    fx.handle.send(TraceInput::ToolCall(second_attempt));
    // node 不同但唯一键不含 node_id（同一实例/attempt/call_id）→ 仍幂等。
    let mut other_node = record("run-dup", "inst", "call-1");
    other_node.node_id = "other-node".to_string();
    fx.handle.send(TraceInput::ToolCall(other_node));
    // 不同 call_id → 新行。
    fx.handle
        .send(TraceInput::ToolCall(record("run-dup", "inst", "call-2")));
    fx.flush();

    let reader = fx.reader();
    let rows = reader.tool_calls("run-dup").expect("tool calls");
    assert_eq!(rows.len(), 3, "幂等丢弃重复定位键，只保留唯一键不同的行");
    assert_eq!(rows.iter().filter(|row| row.call_id == "call-1").count(), 2);
    assert_eq!(rows.iter().filter(|row| row.call_id == "call-2").count(), 1);
}

#[test]
fn tool_calls_are_ordered_by_started_at_then_call_id_and_isolated_per_run() {
    let fx = common::Fixture::new();
    fx.handle.send(common::started("run-order"));
    // 插入顺序与 started_at 顺序相反。
    let mut late = record("run-order", "inst", "call-late");
    late.started_at = "2026-11-02T10:00:09.000Z".to_string();
    let mut early = record("run-order", "inst", "call-early");
    early.started_at = "2026-11-02T10:00:01.000Z".to_string();
    // 同 started_at：插入顺序 z→a（与 call_id 升序相反），断言冻结排序 (started_at, call_id)。
    let mut same_z = record("run-order", "inst", "call-same-z");
    same_z.started_at = "2026-11-02T10:00:05.000Z".to_string();
    let mut same_a = record("run-order", "inst", "call-same-a");
    same_a.started_at = "2026-11-02T10:00:05.000Z".to_string();
    for call in [late, early, same_z, same_a] {
        fx.handle.send(TraceInput::ToolCall(call));
    }
    fx.handle.send(common::started("run-order-2"));
    fx.handle.send(TraceInput::ToolCall(record(
        "run-order-2",
        "inst",
        "call-other",
    )));
    fx.flush();

    let reader = fx.reader();
    let rows = reader.tool_calls("run-order").expect("tool calls");
    assert_eq!(
        rows.iter()
            .map(|row| row.call_id.as_str())
            .collect::<Vec<_>>(),
        vec!["call-early", "call-same-a", "call-same-z", "call-late"],
        "按 (started_at, call_id) 升序（同 started_at 用 call_id 可判定）"
    );
    assert_eq!(
        reader.tool_calls("run-order-2").expect("tool calls").len(),
        1,
        "run 隔离"
    );
    assert!(reader
        .tool_calls("run-missing")
        .expect("missing run")
        .is_empty());
}

#[test]
fn per_run_row_cap_stops_recording_at_10000_across_batches_without_failing_run() {
    let fx = common::Fixture::new();
    fx.handle.send(common::started("run-cap-rows"));
    let chunk = TOOL_CALLS_MAX_PER_RUN / 2;
    for index in 0..=TOOL_CALLS_MAX_PER_RUN {
        let mut call = record("run-cap-rows", "inst-cap-rows", &format!("call-{index}"));
        call.args = json!({"i": index});
        fx.handle.send(TraceInput::ToolCall(call));
        // 中途 flush 强制跨 batch：上限计数必须跨批延续（不是单批内存计数）。
        if index + 1 == chunk {
            fx.flush();
        }
    }
    // 另一 run 的预算独立。
    fx.handle.send(TraceInput::ToolCall(record(
        "run-cap-rows-2",
        "inst",
        "call-other",
    )));
    fx.flush();

    let reader = fx.reader();
    let rows = reader.tool_calls("run-cap-rows").expect("tool calls");
    assert_eq!(
        rows.len(),
        TOOL_CALLS_MAX_PER_RUN,
        "单 run 最多记录 {TOOL_CALLS_MAX_PER_RUN} 行（超限停止记录，不 fail run）"
    );
    assert_eq!(
        reader
            .tool_calls("run-cap-rows-2")
            .expect("second run")
            .len(),
        1,
        "行数上限按 run 隔离"
    );
    assert_eq!(
        reader
            .run("run-cap-rows")
            .expect("run")
            .expect("row")
            .status,
        zgraph_trace::RunStatus::Running,
        "超限不 fail run"
    );
}

#[test]
fn no_tool_calls_means_empty_rows_for_legacy_run() {
    let fx = common::Fixture::new();
    fx.handle.send(common::started("run-legacy"));
    fx.flush();
    let reader = fx.reader();
    assert!(reader
        .tool_calls("run-legacy")
        .expect("tool calls")
        .is_empty());
}
