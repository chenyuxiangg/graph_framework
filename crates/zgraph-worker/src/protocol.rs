//! worker 协议 v1：帧格式、握手、run 请求、错误与结果（M0 冻结）。
//!
//! 契约文档：`schemas/worker-protocol-v1.md`；设计依据：`doc/architecture-v2.md` §3。
//! 变更规则：不兼容修改必须升 [`PROTOCOL_VERSION`] 并提供兼容窗口。

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// 协议版本号，写入每个帧的 `v` 字段。
pub const PROTOCOL_VERSION: u8 = 1;

/// 单帧上限（字节）：超过视为协议错误。
pub const MAX_FRAME_BYTES: usize = 8 * 1024 * 1024;

/// 协议层错误码（与 `doc/architecture-v2.md` §8.2 一致）。
pub const CODE_HANDSHAKE: &str = "NODE_WORKER_HANDSHAKE";
/// 协议层错误码：帧格式/退出约定/超限。
pub const CODE_PROTOCOL: &str = "NODE_WORKER_PROTOCOL";
/// 协议层错误码：worker 导入失败。
pub const CODE_IMPORT: &str = "NODE_WORKER_IMPORT";
/// 协议层错误码：worker 被信号杀死 / 非约定退出码（§8.2）。
pub const CODE_CRASH: &str = "NODE_WORKER_CRASH";
/// 协议层错误码：超时（只由内核 deadline 产生；worker 不应自报）。
pub const CODE_TIMEOUT: &str = "NODE_TIMEOUT";
/// worker 错误帧码：Python 未捕获异常（§8.2）。
pub const CODE_PYTHON_EXCEPTION: &str = "NODE_PYTHON_EXCEPTION";
/// worker 错误帧码：业务错误（也用于 `BIZ_*` 与 `NODE_RETURN_*` 自定义码）。
pub const CODE_BUSINESS_ERROR: &str = "NODE_BUSINESS_ERROR";
/// worker 错误帧码：协作取消（§8.2）。
pub const CODE_CANCELLED: &str = "NODE_CANCELLED";
/// worker 错误帧码：返回值不是 JSON 对象（04 §4.3.2）。
pub const CODE_RETURN_NOT_DICT: &str = "NODE_RETURN_NOT_DICT";

/// emit 单帧内 `payload.type` 的封闭取值（协议 §2）。
pub const EMIT_TYPES: [&str; 5] = [
    "token",
    "reasoning",
    "tool_call_start",
    "tool_call_end",
    "progress",
];

/// 一帧 NDJSON 消息。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Frame {
    /// 协议版本。
    pub v: u8,
    /// 消息类型（`hello` / `hello_ack` / `run` / `emit` / `log` / `result` / `error` / `cancel`）。
    pub kind: String,
    /// 可选关联 id。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// 消息载荷。
    pub payload: Value,
}

impl Frame {
    /// 构造当前协议版本的帧。
    pub fn new(kind: impl Into<String>, payload: Value) -> Self {
        Self {
            v: PROTOCOL_VERSION,
            kind: kind.into(),
            id: None,
            payload,
        }
    }

    /// 序列化为单行 NDJSON（不含换行符）。
    pub fn to_line(&self) -> Result<String, ProtocolError> {
        let line = serde_json::to_string(self).map_err(|err| ProtocolError {
            code: CODE_PROTOCOL,
            message: format!("serialize frame: {err}"),
        })?;
        if line.len() > MAX_FRAME_BYTES {
            return Err(ProtocolError {
                code: CODE_PROTOCOL,
                message: format!("frame too large: {} bytes", line.len()),
            });
        }
        Ok(line)
    }

    /// 从单行 NDJSON 解析并校验帧。
    pub fn from_line(line: &str) -> Result<Self, ProtocolError> {
        if line.len() > MAX_FRAME_BYTES {
            return Err(ProtocolError {
                code: CODE_PROTOCOL,
                message: format!("frame too large: {} bytes", line.len()),
            });
        }
        let frame: Self = serde_json::from_str(line).map_err(|err| ProtocolError {
            code: CODE_PROTOCOL,
            message: format!("parse frame: {err}"),
        })?;
        if frame.v != PROTOCOL_VERSION {
            return Err(ProtocolError {
                code: CODE_HANDSHAKE,
                message: format!("unsupported protocol version: {}", frame.v),
            });
        }
        Ok(frame)
    }
}

/// 解析一行完整帧字节（**必须**以 `\n` 结尾；否则视为半行帧/协议错误）。
pub fn decode_frame_line(bytes: &[u8]) -> Result<Frame, ProtocolError> {
    let Some((&b'\n', body)) = bytes.split_last() else {
        return Err(ProtocolError {
            code: CODE_PROTOCOL,
            message: "partial frame at EOF (missing trailing newline)".to_string(),
        });
    };
    let line = std::str::from_utf8(body).map_err(|err| ProtocolError {
        code: CODE_PROTOCOL,
        message: format!("invalid utf-8 frame: {err}"),
    })?;
    Frame::from_line(line)
}

/// 协议错误（携带机器可读 `code`，映射到 NodeError）。
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{code}: {message}")]
pub struct ProtocolError {
    /// 错误码（`NODE_WORKER_*`）。
    pub code: &'static str,
    /// 人类可读信息。
    pub message: String,
}

/// worker 启动握手帧（worker → supervisor）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Hello {
    /// 协议版本。
    pub protocol_version: u8,
    /// Python 解释器版本。
    pub python_version: String,
    /// worker SDK 版本。
    pub sdk_version: String,
    /// worker 自报 id。
    pub worker_id: String,
}

/// 一次节点执行请求（supervisor → worker）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunRequest {
    /// run 标识。
    pub run_id: String,
    /// 实例标识。
    pub instance_id: String,
    /// 节点标识。
    pub node_id: String,
    /// 尝试次数（0 起）。
    pub attempt: u32,
    /// Python 模块路径。
    pub module: String,
    /// 模块内函数名。
    pub function: String,
    /// 全部声明端口的输入（未到为 null）。
    pub inputs: Value,
    /// 节点参数。
    pub params: Value,
    /// 当前用户。
    pub user_id: String,
    /// run 级全局变量快照。
    pub global_vars: Value,
    /// 确定性种子。
    pub seed: u64,
    /// 实例截止时间（毫秒时间戳）。
    pub deadline_ms: u64,
    /// trace 关联 id。
    pub trace_id: String,
}

/// 结构化 worker 错误（worker → supervisor 的 `error` 帧载荷）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkerError {
    /// 机器可读错误码。
    pub code: String,
    /// 人类可读信息。
    pub message: String,
    /// Python traceback（可选）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub traceback: Option<String>,
    /// 额外细节（可选）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub details: Option<Value>,
}

impl std::fmt::Display for WorkerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

/// worker 执行结局。
#[derive(Debug, Clone, PartialEq)]
pub enum WorkerOutcome {
    /// 成功输出。
    Completed(Value),
    /// 结构化失败。
    Failed(WorkerError),
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn frame_roundtrip_preserves_kind_and_payload() {
        let frame = Frame::new("run", json!({"a": 1}));
        let line = frame.to_line().expect("serialize");
        let parsed = Frame::from_line(&line).expect("parse");
        assert_eq!(parsed.kind, "run");
        assert_eq!(parsed.payload, json!({"a": 1}));
    }

    #[test]
    fn unsupported_version_is_rejected() {
        let line = r#"{"v":9,"kind":"hello","payload":{}}"#;
        let err = Frame::from_line(line).expect_err("must reject");
        assert_eq!(err.code, CODE_HANDSHAKE);
    }

    #[test]
    fn partial_frame_is_rejected() {
        let err = decode_frame_line(br#"{"v":1,"kind":"run","payload":{}}"#).expect_err("partial");
        assert_eq!(err.code, CODE_PROTOCOL);
        assert!(err.message.contains("partial frame"));
    }

    #[test]
    fn oversized_frame_is_rejected() {
        let line = "x".repeat(MAX_FRAME_BYTES + 1);
        let err = Frame::from_line(&line).expect_err("must reject");
        assert_eq!(err.code, CODE_PROTOCOL);
    }

    #[test]
    fn hello_requires_all_fields() {
        let payload = json!({"protocol_version": 1});
        let err = serde_json::from_value::<Hello>(payload).expect_err("must reject");
        assert!(err.to_string().contains("missing field"));
    }
}
