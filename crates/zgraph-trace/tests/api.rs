//! 冻结公开 API 的编译期约束、枚举字符串与错误码映射（cli 直接依赖）。

use zgraph_trace::{
    InstanceStatus, RecordMode, RunStatus, TraceError, TraceHandle, TraceWriter,
    STREAMING_CHANNEL_CAP, TRACE_CHANNEL_CAP, TRACE_SCHEMA_VERSION,
};

fn assert_send_sync<T: Send + Sync>() {}
fn assert_clone<T: Clone>() {}
fn assert_send<T: Send>() {}

#[test]
fn frozen_api_types_are_shareable_across_tasks() {
    assert_send_sync::<TraceHandle>();
    assert_clone::<TraceHandle>();
    assert_send::<TraceWriter>();
    assert_eq!(
        TRACE_SCHEMA_VERSION, 4,
        "v4 = W2a gate_results deferred/attempt/verdict"
    );
}

#[test]
fn channel_capacities_match_spec_6_4_1() {
    assert_eq!(TRACE_CHANNEL_CAP, 8_192, "无损通道 06 §6.4.1");
    assert_eq!(STREAMING_CHANNEL_CAP, 1_024, "流式通道 00 §0.5");
}

#[test]
fn instance_status_strings_roundtrip() {
    let all = [
        InstanceStatus::Pending,
        InstanceStatus::Ready,
        InstanceStatus::Running,
        InstanceStatus::Completed,
        InstanceStatus::Crashed,
        InstanceStatus::TimedOut,
        InstanceStatus::Cancelled,
        InstanceStatus::Skipped,
    ];
    for status in all {
        assert_eq!(InstanceStatus::parse(status.as_str()), Some(status));
    }
    assert_eq!(InstanceStatus::parse("WaitingHuman"), None);
    assert!(InstanceStatus::Completed.is_terminal());
    assert!(!InstanceStatus::Running.is_terminal());
}

#[test]
fn run_status_and_record_mode_strings_roundtrip() {
    for status in [
        RunStatus::Running,
        RunStatus::Paused,
        RunStatus::Completed,
        RunStatus::Failed,
        RunStatus::Cancelled,
    ] {
        assert_eq!(RunStatus::parse(status.as_str()), Some(status));
    }
    assert_eq!(RunStatus::parse("Interrupted"), None);
    assert!(RunStatus::Failed.is_terminal());
    assert!(!RunStatus::Paused.is_terminal());

    for mode in [RecordMode::None, RecordMode::Hash, RecordMode::Full] {
        assert_eq!(RecordMode::parse(mode.as_str()), Some(mode));
    }
    assert_eq!(RecordMode::default(), RecordMode::Hash);
    assert_eq!(RecordMode::parse("Hash"), Some(RecordMode::Hash));
    assert_eq!(RecordMode::parse("unknown"), None);
}

#[test]
fn trace_error_codes_map_to_cli_namespaces() {
    let storage = TraceError::Storage("disk full".to_string());
    assert_eq!(storage.code(), "STORAGE_ERROR");
    assert_eq!(storage.to_string(), "STORAGE_ERROR: disk full");
    let too_new = TraceError::SchemaTooNew {
        found: 3,
        supported: 2,
    };
    assert_eq!(too_new.code(), "STORE_SCHEMA_TOO_NEW");
    assert_eq!(
        too_new.to_string(),
        "STORE_SCHEMA_TOO_NEW: db version=3, supported=2"
    );
    assert_eq!(
        TraceError::NotFound("run not found: r1".to_string()).code(),
        "STORAGE_ERROR"
    );
    assert_eq!(
        TraceError::Serialize("bad json".to_string()).code(),
        "STORAGE_ERROR"
    );
}

#[test]
fn streaming_variant_names_match_storage_kinds() {
    use zgraph_trace::StreamingEvent;
    let events = [
        StreamingEvent::TokenDelta {
            text: "t".to_string(),
        },
        StreamingEvent::ReasoningDelta {
            text: "r".to_string(),
        },
        StreamingEvent::ToolCallStart {
            call_id: "c".to_string(),
            tool_name: "n".to_string(),
            args_summary: serde_json::json!({}),
        },
        StreamingEvent::ToolCallEnd {
            call_id: "c".to_string(),
            tool_name: "n".to_string(),
            ok: true,
            result_summary: None,
        },
        StreamingEvent::Progress {
            percent: 1,
            message: "m".to_string(),
        },
    ];
    let variants: Vec<&str> = events.iter().map(StreamingEvent::variant).collect();
    assert_eq!(
        variants,
        vec![
            "token",
            "reasoning",
            "tool_call_start",
            "tool_call_end",
            "progress"
        ]
    );
}
