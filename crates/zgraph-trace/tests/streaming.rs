//! Streaming 聚合语义（06 §6.5）：无损拼接、分段窗口、tool_call flush、截断、序号。

mod common;

use std::time::Duration;

use serde_json::json;
use zgraph_trace::{
    EventQuery, InstanceStatus, StreamingEvent, TraceInput, TraceReader, TraceWriter,
    STREAM_SEGMENT_MAX_BYTES, STREAM_SEGMENT_MAX_CHARS,
};

fn streaming_rows(reader: &TraceReader, run_id: &str) -> Vec<zgraph_trace::StreamingRow> {
    reader
        .streaming_events(run_id)
        .expect("query streaming events")
}

fn wait_for_segments(reader: &TraceReader, run_id: &str, expected: usize) {
    for _ in 0..100 {
        if streaming_rows(reader, run_id).len() >= expected {
            return;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    panic!("expected at least {expected} streaming segment(s) within 2s");
}

fn progress(run_id: &str, instance_id: &str, percent: u8, message: &str, at: String) -> TraceInput {
    TraceInput::Streaming {
        run_id: run_id.to_string(),
        instance_id: instance_id.to_string(),
        node_id: None,
        event: StreamingEvent::Progress {
            percent,
            message: message.to_string(),
        },
        at,
    }
}

fn tool_call_start(run_id: &str, instance_id: &str, at: String) -> TraceInput {
    TraceInput::Streaming {
        run_id: run_id.to_string(),
        instance_id: instance_id.to_string(),
        node_id: None,
        event: StreamingEvent::ToolCallStart {
            call_id: "call-1".to_string(),
            tool_name: "now".to_string(),
            args_summary: json!({"tz": "UTC"}),
        },
        at,
    }
}

fn tool_call_end(run_id: &str, instance_id: &str, ok: bool, at: String) -> TraceInput {
    TraceInput::Streaming {
        run_id: run_id.to_string(),
        instance_id: instance_id.to_string(),
        node_id: None,
        event: StreamingEvent::ToolCallEnd {
            call_id: "call-1".to_string(),
            tool_name: "now".to_string(),
            ok,
            result_summary: Some(json!({"value": "2026-10-28"})),
        },
        at,
    }
}

#[test]
fn token_deltas_are_aggregated_losslessly_with_delta_count() {
    let fx = common::Fixture::new();
    let handle = &fx.handle;
    handle.send(common::started("run-token"));
    handle.send(TraceInput::InstanceSnapshot(common::snapshot(
        "run-token",
        "inst-gen",
        "gen_node",
        InstanceStatus::Running,
    )));
    for (text, at) in [
        ("hello ", common::ts(100)),
        ("世界", common::ts(150)),
        ("!", common::ts(190)),
    ] {
        assert!(handle.send_streaming(common::token("run-token", "inst-gen", None, text, at)));
    }
    assert_eq!(handle.streaming_dropped(), 0);
    fx.flush();

    let reader = fx.reader();
    let segments = streaming_rows(&reader, "run-token");
    assert_eq!(segments.len(), 1, "all deltas inside one 100ms window");
    let segment = &segments[0];
    assert_eq!(segment.kind, "token");
    assert_eq!(segment.text.as_deref(), Some("hello 世界!"));
    assert_eq!(segment.delta_count, Some(3));
    assert_eq!(
        segment.text_chars,
        Some(i64::try_from("hello 世界!".chars().count()).expect("chars"))
    );
    assert_eq!(
        segment.text_bytes,
        Some(i64::try_from("hello 世界!".len()).expect("bytes"))
    );
    assert!(!segment.truncated);
    assert_eq!(segment.segment_index, 0);
    assert_eq!(
        segment.node_id.as_deref(),
        Some("gen_node"),
        "node_id backfilled from instances snapshot"
    );
    assert_eq!(segment.first_at, common::ts(100));
    assert_eq!(segment.last_at, common::ts(190));

    let events = reader
        .events("run-token", &EventQuery::default())
        .expect("events");
    let streaming: Vec<_> = events
        .iter()
        .filter(|event| event.kind == "Streaming")
        .collect();
    assert_eq!(streaming.len(), 1);
    assert_eq!(streaming[0].seq, segment.seq, "events/streaming_events 1:1");
    assert_eq!(streaming[0].at, common::ts(190), "events.at = last delta");
    assert_eq!(streaming[0].severity, "info");
    assert_eq!(streaming[0].node_id.as_deref(), Some("gen_node"));
    let payload = &streaming[0].payload;
    assert_eq!(payload["variant"], json!("token"));
    assert_eq!(payload["segment_index"], json!(0));
    assert_eq!(payload["delta_count"], json!(3));
    assert!(
        payload.get("text").is_none(),
        "payload must not repeat body"
    );
    fx.close().expect("close");
}

#[test]
fn aggregate_window_flushes_after_100ms_without_explicit_flush() {
    let fx = common::Fixture::new();
    fx.handle.send(common::started("run-window"));
    fx.handle.send(common::token(
        "run-window",
        "inst-w",
        Some("gen"),
        "auto",
        common::ts(50),
    ));
    let reader = fx.reader();
    wait_for_segments(&reader, "run-window", 1);
    let segments = streaming_rows(&reader, "run-window");
    assert_eq!(segments.len(), 1);
    assert_eq!(segments[0].text.as_deref(), Some("auto"));
    assert_eq!(segments[0].delta_count, Some(1));
    fx.close().expect("close");
}

#[test]
fn buffer_splits_at_4096_chars_and_stays_lossless() {
    let fx = common::Fixture::new();
    let handle = &fx.handle;
    handle.send(common::started("run-split"));
    for index in 0..10 {
        handle.send(common::token(
            "run-split",
            "inst-split",
            Some("gen"),
            &"a".repeat(1_000),
            common::ts(index * 10),
        ));
    }
    fx.flush();
    let reader = fx.reader();
    let segments = streaming_rows(&reader, "run-split");
    assert_eq!(
        segments.len(),
        2,
        "5000 chars per segment, split after 5 deltas"
    );
    assert_eq!(segments[0].segment_index, 0);
    assert_eq!(segments[1].segment_index, 1);
    assert_eq!(segments[0].delta_count, Some(5));
    assert_eq!(segments[1].delta_count, Some(5));
    let first_len = segments[0].text.as_deref().expect("text").len();
    let second_len = segments[1].text.as_deref().expect("text").len();
    assert!(first_len >= STREAM_SEGMENT_MAX_CHARS);
    assert!(second_len >= STREAM_SEGMENT_MAX_CHARS);
    let joined: String = segments
        .iter()
        .map(|segment| segment.text.as_deref().expect("text"))
        .collect();
    assert_eq!(joined, "a".repeat(10_000), "lossless concatenation");
    assert_eq!(segments[0].first_at, common::ts(0));
    assert_eq!(segments[0].last_at, common::ts(40));
    assert_eq!(segments[1].first_at, common::ts(50));
    assert_eq!(segments[1].last_at, common::ts(90));
    fx.close().expect("close");
}

#[test]
fn segment_byte_cap_truncates_on_char_boundary() {
    let fx = common::Fixture::new();
    let handle = &fx.handle;
    handle.send(common::started("run-bytes"));
    handle.send(common::token(
        "run-bytes",
        "inst-ascii",
        Some("gen"),
        &"x".repeat(20_000),
        common::ts(10),
    ));
    handle.send(common::token(
        "run-bytes",
        "inst-utf8",
        Some("gen"),
        &"é".repeat(20_000),
        common::ts(20),
    ));
    fx.flush();
    let reader = fx.reader();
    let segments = streaming_rows(&reader, "run-bytes");
    assert_eq!(segments.len(), 2);
    let ascii = segments
        .iter()
        .find(|row| row.instance_id == "inst-ascii")
        .expect("ascii segment");
    assert!(
        ascii.truncated,
        "segment over 16 KiB must be marked truncated"
    );
    assert_eq!(
        ascii.text_bytes,
        Some(i64::try_from(STREAM_SEGMENT_MAX_BYTES).expect("max bytes"))
    );
    assert_eq!(
        ascii.text_chars,
        Some(i64::try_from(STREAM_SEGMENT_MAX_BYTES).expect("chars"))
    );
    assert_eq!(
        ascii.text.as_deref().map(str::len),
        Some(STREAM_SEGMENT_MAX_BYTES)
    );
    let utf8 = segments
        .iter()
        .find(|row| row.instance_id == "inst-utf8")
        .expect("utf8 segment");
    assert!(utf8.truncated);
    assert_eq!(
        utf8.text_bytes,
        Some(i64::try_from(STREAM_SEGMENT_MAX_BYTES).expect("max bytes"))
    );
    assert_eq!(
        utf8.text_chars,
        Some(i64::try_from(STREAM_SEGMENT_MAX_BYTES / 2).expect("chars")),
        "multi-byte chars truncated on boundary"
    );
    assert_eq!(utf8.text.as_deref(), Some("é".repeat(8_192).as_str()));
    fx.close().expect("close");
}

#[test]
fn non_delta_streaming_events_flush_pending_token_and_become_segments() {
    let fx = common::Fixture::new();
    let handle = &fx.handle;
    handle.send(common::started("run-tool"));
    handle.send(TraceInput::InstanceSnapshot(common::snapshot(
        "run-tool",
        "inst-agent",
        "agent_node",
        InstanceStatus::Running,
    )));
    handle.send(common::token(
        "run-tool",
        "inst-agent",
        None,
        "abc",
        common::ts(100),
    ));
    handle.send(tool_call_start("run-tool", "inst-agent", common::ts(200)));
    handle.send(progress(
        "run-tool",
        "inst-agent",
        42,
        "half",
        common::ts(210),
    ));
    handle.send(tool_call_end(
        "run-tool",
        "inst-agent",
        false,
        common::ts(220),
    ));
    fx.flush();

    let reader = fx.reader();
    let events = reader
        .events("run-tool", &EventQuery::default())
        .expect("events");
    let streaming: Vec<_> = events
        .iter()
        .filter(|event| event.kind == "Streaming")
        .collect();
    let variants: Vec<&serde_json::Value> = streaming
        .iter()
        .map(|event| &event.payload["variant"])
        .collect();
    assert_eq!(
        variants,
        vec![
            &json!("token"),
            &json!("tool_call_start"),
            &json!("progress"),
            &json!("tool_call_end"),
        ],
        "token buffer flushed before tool_call_start; tool events keep arrival order"
    );
    assert_eq!(streaming[0].payload["delta_count"], json!(1));
    assert_eq!(
        streaming[3].severity, "warn",
        "tool_call_end ok=false is warn"
    );

    let segments = streaming_rows(&reader, "run-tool");
    let token = segments
        .iter()
        .find(|row| row.kind == "token")
        .expect("token");
    assert_eq!(token.text.as_deref(), Some("abc"));
    assert_eq!(token.node_id.as_deref(), Some("agent_node"));
    let start = segments
        .iter()
        .find(|row| row.kind == "tool_call_start")
        .expect("tool_call_start");
    assert_eq!(start.call_id.as_deref(), Some("call-1"));
    assert_eq!(start.tool_name.as_deref(), Some("now"));
    assert_eq!(start.summary, Some(json!({"tz": "UTC"})));
    assert_eq!(start.segment_index, 0);
    let progress_row = segments
        .iter()
        .find(|row| row.kind == "progress")
        .expect("progress");
    assert_eq!(progress_row.text.as_deref(), Some("half"));
    assert_eq!(progress_row.percent, Some(42));
    let end = segments
        .iter()
        .find(|row| row.kind == "tool_call_end")
        .expect("tool_call_end");
    assert_eq!(end.ok, Some(false));
    assert_eq!(end.summary, Some(json!({"value": "2026-10-28"})));
    fx.close().expect("close");
}

#[test]
fn terminal_transition_flushes_instance_buffers_before_transition() {
    let fx = common::Fixture::new();
    let handle = &fx.handle;
    handle.send(common::started("run-term"));
    handle.send(common::transition(
        "run-term",
        "inst-term",
        "gen",
        InstanceStatus::Pending,
        InstanceStatus::Ready,
        common::ts(10),
    ));
    handle.send(common::transition(
        "run-term",
        "inst-term",
        "gen",
        InstanceStatus::Ready,
        InstanceStatus::Running,
        common::ts(20),
    ));
    handle.send(common::token(
        "run-term",
        "inst-term",
        None,
        "final text",
        common::ts(30),
    ));
    handle.send(common::transition(
        "run-term",
        "inst-term",
        "gen",
        InstanceStatus::Running,
        InstanceStatus::Completed,
        common::ts(40),
    ));
    fx.flush();

    let reader = fx.reader();
    let events = reader
        .events("run-term", &EventQuery::default())
        .expect("events");
    let streaming_seq = events
        .iter()
        .find(|event| event.kind == "Streaming")
        .expect("streaming event")
        .seq;
    let transitions = reader.transitions("run-term").expect("transitions");
    let completed = transitions
        .iter()
        .find(|row| row.to_status == InstanceStatus::Completed)
        .expect("completed transition");
    assert!(
        streaming_seq < completed.seq,
        "buffered token must be flushed before the terminal transition"
    );
    let segments = streaming_rows(&reader, "run-term");
    assert_eq!(segments.len(), 1);
    assert_eq!(segments[0].text.as_deref(), Some("final text"));
    assert_eq!(
        segments[0].node_id.as_deref(),
        Some("gen"),
        "node_id from instances snapshot"
    );
    let instances = reader.instances("run-term").expect("instances");
    assert_eq!(instances[0].finished_seq, Some(completed.seq));
    fx.close().expect("close");
}

#[test]
fn segment_index_is_continuous_per_instance_and_kind() {
    let fx = common::Fixture::new();
    let handle = &fx.handle;
    handle.send(common::started("run-index"));
    handle.send(common::token(
        "run-index",
        "inst-idx",
        Some("gen"),
        "one",
        common::ts(10),
    ));
    fx.flush();
    handle.send(common::token(
        "run-index",
        "inst-idx",
        Some("gen"),
        "two",
        common::ts(200),
    ));
    fx.flush();
    handle.send(common::token(
        "run-index",
        "inst-idx",
        Some("gen"),
        "three",
        common::ts(400),
    ));
    fx.flush();
    let reader = fx.reader();
    let segments = streaming_rows(&reader, "run-index");
    assert_eq!(segments.len(), 3);
    assert_eq!(
        segments
            .iter()
            .map(|row| (row.segment_index, row.text.clone()))
            .collect::<Vec<_>>(),
        vec![
            (0, Some("one".to_string())),
            (1, Some("two".to_string())),
            (2, Some("three".to_string())),
        ]
    );
    fx.close().expect("close");
}

#[test]
fn token_and_reasoning_buffers_are_independent() {
    let fx = common::Fixture::new();
    let handle = &fx.handle;
    handle.send(common::started("run-kind"));
    handle.send(common::token(
        "run-kind",
        "inst-kind",
        Some("gen"),
        "tok",
        common::ts(10),
    ));
    handle.send(common::reasoning(
        "run-kind",
        "inst-kind",
        "why",
        common::ts(20),
    ));
    fx.flush();
    let reader = fx.reader();
    let segments = streaming_rows(&reader, "run-kind");
    assert_eq!(segments.len(), 2);
    let token = segments
        .iter()
        .find(|row| row.kind == "token")
        .expect("token");
    let reasoning = segments
        .iter()
        .find(|row| row.kind == "reasoning")
        .expect("reasoning");
    assert_eq!(token.text.as_deref(), Some("tok"));
    assert_eq!(reasoning.text.as_deref(), Some("why"));
    assert_eq!(token.segment_index, 0);
    assert_eq!(reasoning.segment_index, 0, "index is per (instance, kind)");
    assert_eq!(token.delta_count, Some(1));
    assert_eq!(reasoning.delta_count, Some(1));
    fx.close().expect("close");
}

#[test]
fn close_flushes_remaining_buffers_and_checkpoints() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("trace.db");
    let writer = TraceWriter::open(&path).expect("open writer");
    let handle = writer.handle();
    handle.send(common::started("run-close"));
    handle.send(common::token(
        "run-close",
        "inst-close",
        Some("gen"),
        "pending text",
        common::ts(10),
    ));
    writer.close().expect("close writer");
    let wal_path = format!("{}-wal", path.display());
    assert!(
        !std::path::Path::new(&wal_path).exists(),
        "close must truncate the WAL"
    );
    let reader = TraceReader::open_readonly(&path).expect("reader");
    let segments = reader.streaming_events("run-close").expect("segments");
    assert_eq!(segments.len(), 1);
    assert_eq!(segments[0].text.as_deref(), Some("pending text"));
}
