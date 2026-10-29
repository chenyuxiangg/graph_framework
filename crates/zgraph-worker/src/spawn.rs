//! worker 子进程生命周期：spawn、握手、发送 run、收集 emit/result/error、超时与杀进程。
//!
//! M0 只实现客户端最小闭环；完整 supervisor（并发上限、进程组 kill、stderr 入库）在 M3 扩展。

use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;

use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader, BufWriter};
use tokio::process::{Child, ChildStderr, ChildStdin, ChildStdout, Command};

use crate::protocol::{
    decode_frame_line, Frame, Hello, ProtocolError, RunRequest, WorkerError, CODE_HANDSHAKE,
    CODE_PROTOCOL, PROTOCOL_VERSION,
};

/// worker 启动配置。
#[derive(Debug, Clone)]
pub struct WorkerConfig {
    /// Python 可执行文件（如 `python3` 或 venv 路径）。
    pub program: String,
    /// 进程参数（如 `-m zgraph_worker.runtime`）。
    pub args: Vec<String>,
    /// 追加环境变量（如 `PYTHONPATH`）。
    pub env: Vec<(String, String)>,
    /// 工作目录。
    pub cwd: Option<PathBuf>,
    /// 整次执行超时（含握手）。
    pub timeout: Duration,
    /// 握手超时。
    pub hello_timeout: Duration,
    /// result 后进程退出宽限。
    pub exit_grace: Duration,
    /// stderr 保留上限（字节，取尾部）。
    pub max_stderr_bytes: usize,
}

impl Default for WorkerConfig {
    fn default() -> Self {
        Self {
            program: "python3".to_string(),
            args: vec!["-m".to_string(), "zgraph_worker.runtime".to_string()],
            env: Vec::new(),
            cwd: None,
            timeout: Duration::from_secs(180),
            hello_timeout: Duration::from_secs(5),
            exit_grace: Duration::from_secs(1),
            max_stderr_bytes: 8 * 1024,
        }
    }
}

/// worker 调用失败。
#[derive(Debug, thiserror::Error)]
pub enum WorkerFailure {
    /// 进程无法启动。
    #[error("spawn worker failed: {0}")]
    Spawn(String),
    /// 协议错误（握手/帧/退出约定）。
    #[error(transparent)]
    Protocol(#[from] ProtocolError),
    /// worker 返回结构化错误。
    #[error("worker error {0}")]
    Worker(WorkerError),
    /// 超时（进程已被 kill_on_drop 终止）。
    #[error("worker timed out after {0:?}")]
    Timeout(Duration),
    /// 非预期退出。
    #[error("worker exited unexpectedly: code={code:?}, stderr={stderr}")]
    Exit {
        /// 退出码。
        code: Option<i32>,
        /// stderr 尾部。
        stderr: String,
    },
}

/// 成功执行结果。
#[derive(Debug, Clone)]
pub struct WorkerRun {
    /// 节点输出。
    pub output: serde_json::Value,
    /// 执行期间收到的 emit 帧（流式事件）。
    pub events: Vec<Frame>,
    /// stderr 尾部（供 trace/排障）。
    pub stderr: String,
}

fn protocol_error(message: impl Into<String>) -> WorkerFailure {
    WorkerFailure::Protocol(ProtocolError {
        code: CODE_PROTOCOL,
        message: message.into(),
    })
}

fn handshake_error(message: impl Into<String>) -> WorkerFailure {
    WorkerFailure::Protocol(ProtocolError {
        code: CODE_HANDSHAKE,
        message: message.into(),
    })
}

/// 启动 worker 并执行一次 `run`，直到 result/error/超时。
pub async fn run_worker(
    config: &WorkerConfig,
    request: &RunRequest,
) -> Result<WorkerRun, WorkerFailure> {
    let mut command = Command::new(&config.program);
    command
        .args(&config.args)
        .kill_on_drop(true)
        .env("PYTHONHASHSEED", "0")
        .env("PYTHONUNBUFFERED", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for (key, value) in &config.env {
        command.env(key, value);
    }
    if let Some(cwd) = &config.cwd {
        command.current_dir(cwd);
    }

    let mut child = command
        .spawn()
        .map_err(|err| WorkerFailure::Spawn(err.to_string()))?;
    let stdin = child
        .stdin
        .take()
        .ok_or_else(|| protocol_error("no stdin"))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| protocol_error("no stdout"))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| protocol_error("no stderr"))?;

    let stderr_task = tokio::spawn(drain_stderr(stderr, config.max_stderr_bytes));
    let inner = drive(&mut child, stdin, stdout, request, config);
    let result = tokio::time::timeout(config.timeout, inner).await;

    // 任何失败路径（含握手超时/协议错误）都必须显式 kill + wait/reap，
    // 不能只依赖 kill_on_drop（否则僵尸进程窗口不可控）。
    if !matches!(result, Ok(Ok(_))) {
        let _ = child.kill().await;
        let _ = child.wait().await;
    }

    let stderr_tail = match tokio::time::timeout(Duration::from_secs(1), stderr_task).await {
        Ok(Ok(text)) => text,
        _ => String::new(),
    };

    match result {
        Err(_) => Err(WorkerFailure::Timeout(config.timeout)),
        Ok(Ok(mut run)) => {
            run.stderr = stderr_tail;
            Ok(run)
        }
        Ok(Err(err)) => Err(attach_stderr(err, stderr_tail)),
    }
}

fn attach_stderr(err: WorkerFailure, stderr: String) -> WorkerFailure {
    match err {
        WorkerFailure::Exit { code, .. } => WorkerFailure::Exit { code, stderr },
        other => other,
    }
}

async fn drive(
    child: &mut Child,
    stdin: ChildStdin,
    stdout: ChildStdout,
    request: &RunRequest,
    config: &WorkerConfig,
) -> Result<WorkerRun, WorkerFailure> {
    let mut writer = BufWriter::new(stdin);
    let mut reader = BufReader::new(stdout);

    let hello_frame = tokio::time::timeout(config.hello_timeout, read_frame_line(&mut reader))
        .await
        .map_err(|_| handshake_error("hello timeout"))??
        .ok_or_else(|| handshake_error("worker exited before hello"))?;
    if hello_frame.kind != "hello" {
        return Err(handshake_error(format!(
            "expected hello, got {}",
            hello_frame.kind
        )));
    }
    let hello: Hello = serde_json::from_value(hello_frame.payload)
        .map_err(|err| handshake_error(format!("bad hello payload: {err}")))?;
    if hello.protocol_version != PROTOCOL_VERSION {
        return Err(handshake_error(format!(
            "worker protocol {} != {}",
            hello.protocol_version, PROTOCOL_VERSION
        )));
    }

    write_frame(
        &mut writer,
        &Frame::new("hello_ack", serde_json::json!({"accepted": true})),
    )
    .await?;
    let run_payload = serde_json::to_value(request)
        .map_err(|err| protocol_error(format!("encode run request: {err}")))?;
    write_frame(&mut writer, &Frame::new("run", run_payload)).await?;

    let mut events = Vec::new();
    loop {
        let frame = read_frame_line(&mut reader)
            .await?
            .ok_or_else(|| protocol_error("worker closed stdout before result"))?;
        match frame.kind.as_str() {
            "emit" => events.push(frame),
            "log" => {}
            "result" => {
                let output = frame
                    .payload
                    .get("output")
                    .cloned()
                    .ok_or_else(|| protocol_error("result without output"))?;
                ensure_exit(child, config.exit_grace).await?;
                ensure_no_trailing_frame(&mut reader).await?;
                return Ok(WorkerRun {
                    output,
                    events,
                    stderr: String::new(),
                });
            }
            "error" => {
                let err: WorkerError = serde_json::from_value(frame.payload)
                    .map_err(|e| protocol_error(format!("bad error payload: {e}")))?;
                return Err(WorkerFailure::Worker(err));
            }
            other => {
                return Err(protocol_error(format!("unexpected frame kind: {other}")));
            }
        }
    }
}

async fn read_frame_line(
    reader: &mut BufReader<ChildStdout>,
) -> Result<Option<Frame>, WorkerFailure> {
    let mut buffer: Vec<u8> = Vec::new();
    let read = reader
        .read_until(b'\n', &mut buffer)
        .await
        .map_err(|err| protocol_error(format!("read frame: {err}")))?;
    if read == 0 {
        return Ok(None);
    }
    decode_frame_line(&buffer)
        .map(Some)
        .map_err(WorkerFailure::from)
}

async fn ensure_no_trailing_frame(
    reader: &mut BufReader<ChildStdout>,
) -> Result<(), WorkerFailure> {
    // 进程已退出（ensure_exit 已 wait），管道已关闭：一次 read_until 读到 EOF 或残余数据。
    match read_frame_line(reader).await? {
        Some(_) => Err(protocol_error("duplicate/trailing frame after result")),
        None => Ok(()),
    }
}

async fn write_frame(
    writer: &mut BufWriter<ChildStdin>,
    frame: &Frame,
) -> Result<(), WorkerFailure> {
    let mut line = frame.to_line()?;
    line.push('\n');
    writer
        .write_all(line.as_bytes())
        .await
        .map_err(|err| protocol_error(format!("write frame: {err}")))?;
    writer
        .flush()
        .await
        .map_err(|err| protocol_error(format!("flush frame: {err}")))
}

async fn ensure_exit(child: &mut Child, grace: Duration) -> Result<(), WorkerFailure> {
    match tokio::time::timeout(grace, child.wait()).await {
        Ok(Ok(status)) if status.success() => Ok(()),
        Ok(Ok(status)) => Err(WorkerFailure::Exit {
            code: status.code(),
            stderr: String::new(),
        }),
        Ok(Err(err)) => Err(protocol_error(format!("wait worker: {err}"))),
        Err(_) => {
            let _ = child.kill().await;
            Err(protocol_error("worker did not exit after result"))
        }
    }
}

async fn drain_stderr(mut stderr: ChildStderr, max_bytes: usize) -> String {
    let mut collected: Vec<u8> = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        match stderr.read(&mut chunk).await {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                collected.extend_from_slice(&chunk[..n]);
                if collected.len() > max_bytes {
                    let keep_from = collected.len() - max_bytes;
                    collected.drain(..keep_from);
                }
            }
        }
    }
    String::from_utf8_lossy(&collected).into_owned()
}

/// 构造默认 request 的便捷函数（测试与 M0 示例用）。
pub fn sample_request(module: &str, function: &str, inputs: serde_json::Value) -> RunRequest {
    RunRequest {
        run_id: "run-1".to_string(),
        instance_id: "inst-1".to_string(),
        node_id: "node-1".to_string(),
        attempt: 0,
        module: module.to_string(),
        function: function.to_string(),
        inputs,
        params: serde_json::json!({}),
        user_id: "local".to_string(),
        global_vars: serde_json::json!({}),
        seed: 0,
        deadline_ms: 0,
        trace_id: "trace-1".to_string(),
    }
}

/// 供测试使用的常量：stderr 上限。
pub const DEFAULT_MAX_STDERR_BYTES: usize = 8 * 1024;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::{CODE_TIMEOUT, MAX_FRAME_BYTES};

    #[test]
    fn sample_request_has_expected_identity() {
        let request = sample_request("m", "f", serde_json::json!({"a": 1}));
        assert_eq!(request.module, "m");
        assert_eq!(request.seed, 0);
        assert_eq!(request.attempt, 0);
    }

    #[test]
    fn worker_limits_match_protocol_contract() {
        assert_eq!(MAX_FRAME_BYTES, 8 * 1024 * 1024);
        assert_eq!(DEFAULT_MAX_STDERR_BYTES, 8 * 1024);
        assert_eq!(CODE_TIMEOUT, "NODE_TIMEOUT");
    }
}
