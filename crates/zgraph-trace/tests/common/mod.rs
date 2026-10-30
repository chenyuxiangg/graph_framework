//! 集成测试共享夹具与构造函数（真实 SQLite 临时库，不共享 CI 库）。
#![allow(dead_code)]

use std::path::PathBuf;

use serde_json::json;
use zgraph_trace::{
    GateResultRecord, InstanceSnapshot, InstanceStatus, RecordMode, RunFinishedInfo, RunMetrics,
    RunStartedInfo, RunStatus, StreamingEvent, TraceFailure, TraceHandle, TraceInput, TraceReader,
    TraceWriter, TransitionRecord,
};

/// 测试基准时刻（09:12:03.104Z）。
pub const STARTED_AT: &str = "2026-10-28T09:12:03.104Z";

/// 生成相对基准时刻的 RFC3339 毫秒 Z 时间戳。
pub fn ts(offset_ms: i64) -> String {
    let base = chrono::DateTime::parse_from_rfc3339(STARTED_AT).expect("valid base timestamp");
    (base + chrono::Duration::milliseconds(offset_ms))
        .to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

/// 一次性临时库 + 写端 + 句柄。
pub struct Fixture {
    pub dir: tempfile::TempDir,
    pub path: PathBuf,
    pub writer: TraceWriter,
    pub handle: TraceHandle,
}

impl Fixture {
    /// 建临时目录并打开写端（schema 已迁移、写线程已启动）。
    pub fn new() -> Self {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("trace.db");
        let writer = TraceWriter::open(&path).expect("open trace writer");
        let handle = writer.handle();
        Self {
            dir,
            path,
            writer,
            handle,
        }
    }

    /// 屏障：等待已发送写入提交。
    pub fn flush(&self) {
        self.writer.flush().expect("flush trace writer");
    }

    /// 打开只读 reader（模拟 `zgraph trace`）。
    pub fn reader(&self) -> TraceReader {
        TraceReader::open_readonly(&self.path).expect("open readonly trace reader")
    }

    /// 关闭写端（flush + checkpoint）。
    pub fn close(self) -> Result<(), zgraph_trace::TraceError> {
        self.writer.close()
    }
}

/// `RunStarted` 输入。
pub fn started(run_id: &str) -> TraceInput {
    TraceInput::RunStarted(RunStartedInfo {
        run_id: run_id.to_string(),
        graph_hash: "0123456789abcdef".to_string(),
        graph_name: "demo_graph".to_string(),
        dsl_json: r#"{"version":"1.0"}"#.to_string(),
        user_id: "local".to_string(),
        entry_inputs: json!({"start": {"in": 1}}),
        global_vars: json!({}),
        seed: 42,
        record_mode: RecordMode::Hash,
        owner_pid: std::process::id(),
        owner_host: "test-host".to_string(),
        started_at: STARTED_AT.to_string(),
    })
}

/// 构造实例快照（默认 python_node/attempt 0）。
pub fn snapshot(
    run_id: &str,
    instance_id: &str,
    node_id: &str,
    status: InstanceStatus,
) -> InstanceSnapshot {
    InstanceSnapshot {
        run_id: run_id.to_string(),
        instance_id: instance_id.to_string(),
        node_id: node_id.to_string(),
        node_type: "python_node".to_string(),
        attempt: 0,
        status,
        input: Some(json!({"in": 1})),
        output: if status == InstanceStatus::Completed {
            Some(json!({"result": "ok"}))
        } else {
            None
        },
        error: None,
        error_code: None,
        created_at: ts(0),
        started_at: if matches!(
            status,
            InstanceStatus::Running
                | InstanceStatus::Completed
                | InstanceStatus::Crashed
                | InstanceStatus::TimedOut
                | InstanceStatus::Cancelled
        ) {
            Some(ts(5))
        } else {
            None
        },
        finished_at: if status.is_terminal() {
            Some(ts(100))
        } else {
            None
        },
    }
}

/// 构造一条状态迁移输入。
pub fn transition(
    run_id: &str,
    instance_id: &str,
    node_id: &str,
    from: InstanceStatus,
    to: InstanceStatus,
    at: String,
) -> TraceInput {
    let mut snapshot = snapshot(run_id, instance_id, node_id, to);
    if to == InstanceStatus::Crashed {
        snapshot.error = Some(json!({"kind": "internal", "message": "boom"}));
        snapshot.error_code = Some("NODE_INTERNAL".to_string());
    }
    TraceInput::Transition {
        record: TransitionRecord {
            run_id: run_id.to_string(),
            instance_id: instance_id.to_string(),
            node_id: node_id.to_string(),
            attempt: 0,
            from,
            to,
            reason: Some("test".to_string()),
            ts: at,
        },
        snapshot,
    }
}

/// 构造门控结果输入。
pub fn gate(
    run_id: &str,
    instance_id: &str,
    node_id: &str,
    edge_index: i64,
    passed: bool,
    error: Option<&str>,
) -> TraceInput {
    TraceInput::GateResult(GateResultRecord {
        run_id: run_id.to_string(),
        instance_id: instance_id.to_string(),
        node_id: node_id.to_string(),
        edge_index,
        to_node: "next".to_string(),
        to_port: "in".to_string(),
        gate_fn: Some("builtin.check_output".to_string()),
        passed,
        error: error.map(str::to_string),
        ts: ts(200),
    })
}

/// 构造 `RunFinished` 输入。
pub fn finished(run_id: &str, status: RunStatus) -> TraceInput {
    TraceInput::RunFinished(RunFinishedInfo {
        run_id: run_id.to_string(),
        status,
        output_summary: Some(json!({"result": "ok"})),
        error: if status == RunStatus::Failed {
            Some(TraceFailure {
                code: "NODE_TIMEOUT".to_string(),
                message: "node timed out".to_string(),
            })
        } else {
            None
        },
        metrics: RunMetrics {
            streaming_dropped: 3,
            abandoned_tasks: 0,
            trace_backpressure: 5,
            killed_workers: 1,
        },
        at: ts(1842),
    })
}

/// 构造 token 增量输入。
pub fn token(
    run_id: &str,
    instance_id: &str,
    node_id: Option<&str>,
    text: &str,
    at: String,
) -> TraceInput {
    TraceInput::Streaming {
        run_id: run_id.to_string(),
        instance_id: instance_id.to_string(),
        node_id: node_id.map(str::to_string),
        event: StreamingEvent::TokenDelta {
            text: text.to_string(),
        },
        at,
    }
}

/// 构造 reasoning 增量输入。
pub fn reasoning(run_id: &str, instance_id: &str, text: &str, at: String) -> TraceInput {
    TraceInput::Streaming {
        run_id: run_id.to_string(),
        instance_id: instance_id.to_string(),
        node_id: None,
        event: StreamingEvent::ReasoningDelta {
            text: text.to_string(),
        },
        at,
    }
}
