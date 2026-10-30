//! 双通道拓扑（06 §6.4.1）：顺序、flush 屏障、关停排空、断开丢弃与容量内不丢。
//!
//! 通道容量与背压的精确行为（8192 无损阻塞 / 1024 流式丢弃）由 `writer` 模块的
//! 单元测试直接检查通道，不依赖写线程时序。

mod common;

use zgraph_trace::{
    EventQuery, InstanceStatus, StreamingEvent, TraceInput, TraceReader, TraceWriter,
};

fn progress(run_id: &str, instance_id: &str, percent: u8, at: String) -> TraceInput {
    TraceInput::Streaming {
        run_id: run_id.to_string(),
        instance_id: instance_id.to_string(),
        node_id: Some("gen".to_string()),
        event: StreamingEvent::Progress {
            percent,
            message: "p".to_string(),
        },
        at,
    }
}

fn streaming_seq(reader: &TraceReader, run_id: &str) -> i64 {
    reader
        .events(run_id, &EventQuery::default())
        .expect("events")
        .into_iter()
        .find(|event| event.kind == "Streaming")
        .expect("streaming event")
        .seq
}

fn completed_transition_seq(reader: &TraceReader, run_id: &str) -> i64 {
    reader
        .transitions(run_id)
        .expect("transitions")
        .into_iter()
        .find(|row| row.to_status == InstanceStatus::Completed)
        .expect("completed transition")
        .seq
}

#[test]
fn streaming_channel_is_drained_before_lossless_event() {
    let fx = common::Fixture::new();
    let handle = &fx.handle;
    handle.send(common::started("run-order"));
    assert!(handle.send_streaming(common::token(
        "run-order",
        "inst-order",
        Some("gen"),
        "abc",
        common::ts(10)
    )));
    handle.send(common::transition(
        "run-order",
        "inst-order",
        "gen",
        InstanceStatus::Running,
        InstanceStatus::Completed,
        common::ts(20),
    ));
    fx.flush();

    assert_eq!(handle.streaming_dropped(), 0);
    let reader = fx.reader();
    assert!(
        streaming_seq(&reader, "run-order") < completed_transition_seq(&reader, "run-order"),
        "流式通道中先入队的事件必须先于其后的无损事件落库"
    );
    let segments = reader.streaming_events("run-order").expect("segments");
    assert_eq!(segments.len(), 1);
    assert_eq!(segments[0].text.as_deref(), Some("abc"));
    assert_eq!(
        segments[0].node_id.as_deref(),
        Some("gen"),
        "node_id 由实例快照回填"
    );
    fx.close().expect("close");
}

#[test]
fn run_finished_waits_for_streaming_channel() {
    let fx = common::Fixture::new();
    let handle = &fx.handle;
    handle.send(common::started("run-finish"));
    assert!(handle.send_streaming(common::token(
        "run-finish",
        "inst-finish",
        None,
        "tail",
        common::ts(10)
    )));
    handle.send(common::finished(
        "run-finish",
        zgraph_trace::RunStatus::Completed,
    ));
    fx.flush();

    let reader = fx.reader();
    let events = reader
        .events("run-finish", &EventQuery::default())
        .expect("events");
    let streaming = events
        .iter()
        .find(|event| event.kind == "Streaming")
        .expect("streaming event");
    let finished = events
        .iter()
        .find(|event| event.kind == "RunFinished")
        .expect("run finished");
    assert!(
        streaming.seq < finished.seq,
        "RunFinished 之前必须 flush 流式缓冲"
    );
    let segments = reader.streaming_events("run-finish").expect("segments");
    assert_eq!(segments.len(), 1);
    assert_eq!(segments[0].text.as_deref(), Some("tail"));
    fx.close().expect("close");
}

#[test]
fn flush_is_a_barrier_for_both_channels() {
    let fx = common::Fixture::new();
    fx.handle.send(common::started("run-barrier"));
    for index in 0..5_i64 {
        assert!(fx.handle.send_streaming(common::token(
            "run-barrier",
            "inst-b",
            Some("gen"),
            "x",
            common::ts(index)
        )));
    }
    // 不 sleep：flush 屏障必须同时排空流式通道并提交聚合段。
    fx.flush();

    assert_eq!(fx.handle.streaming_dropped(), 0);
    let reader = fx.reader();
    let segments = reader.streaming_events("run-barrier").expect("segments");
    assert_eq!(segments.len(), 1, "100ms 窗口内合并为一段");
    assert_eq!(segments[0].text.as_deref(), Some("xxxxx"));
    assert_eq!(segments[0].delta_count, Some(5));
    fx.close().expect("close");
}

#[test]
fn streaming_channel_accepts_full_capacity_without_drops() {
    let fx = common::Fixture::new();
    fx.handle.send(common::started("run-many"));
    for index in 0..1_024_i64 {
        assert!(
            fx.handle.send_streaming(common::token(
                "run-many",
                "inst-many",
                Some("gen"),
                "a",
                common::ts(index)
            )),
            "容量 1024 内不得丢弃（第 {index} 条）"
        );
    }
    fx.flush();

    assert_eq!(fx.handle.streaming_dropped(), 0);
    let reader = fx.reader();
    let segments = reader.streaming_events("run-many").expect("segments");
    let text: String = segments
        .iter()
        .filter_map(|row| row.text.as_deref())
        .collect();
    let deltas: i64 = segments.iter().filter_map(|row| row.delta_count).sum();
    assert_eq!(text, "a".repeat(1_024));
    assert_eq!(deltas, 1_024);
    fx.close().expect("close");
}

#[test]
fn flush_barrier_covers_streaming_events_beyond_one_batch() {
    let fx = common::Fixture::new();
    fx.handle.send(common::started("run-bulk"));
    for index in 0..1_024_i64 {
        assert!(fx.handle.send_streaming(progress(
            "run-bulk",
            "inst-bulk",
            u8::try_from(index % 100).expect("percent fits in u8"),
            common::ts(index),
        )));
    }
    fx.flush();

    let reader = fx.reader();
    let segments = reader.streaming_events("run-bulk").expect("segments");
    assert_eq!(
        segments.len(),
        1_024,
        "flush 返回时通道中已入队的流式事件必须全部落库（超出单批 256 行上限也要排空）"
    );
    fx.close().expect("close");
}

#[test]
fn close_drains_streaming_channel_and_flushes_buffers() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("trace.db");
    let writer = TraceWriter::open(&path).expect("open writer");
    let handle = writer.handle();
    handle.send(common::started("run-shutdown"));
    for index in 0..3_i64 {
        assert!(handle.send_streaming(common::token(
            "run-shutdown",
            "inst-s",
            Some("gen"),
            "z",
            common::ts(index)
        )));
    }
    // 不显式 flush，直接 close：关停路径必须排空流式通道并 flush 聚合缓冲。
    writer.close().expect("close writer");

    let reader = TraceReader::open_readonly(&path).expect("reader");
    let segments = reader.streaming_events("run-shutdown").expect("segments");
    assert_eq!(segments.len(), 1);
    assert_eq!(segments[0].text.as_deref(), Some("zzz"));
    assert_eq!(segments[0].delta_count, Some(3));
}

#[test]
fn send_streaming_after_close_is_dropped_and_counted() {
    let dir = tempfile::tempdir().expect("tempdir");
    let writer = TraceWriter::open(dir.path().join("trace.db")).expect("open writer");
    let handle = writer.handle();
    writer.close().expect("close writer");

    assert!(
        !handle.send_streaming(common::token(
            "run-closed",
            "inst-closed",
            None,
            "x",
            common::ts(0)
        )),
        "写端关闭后流式发送必须返回 false（I6）"
    );
    assert_eq!(handle.streaming_dropped(), 1);
}
