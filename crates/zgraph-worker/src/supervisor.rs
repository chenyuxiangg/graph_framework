//! worker supervisor：进程组生命周期、帧流、取消升级与资源回收（M3）。
//!
//! M0 的 [`crate::spawn`] 只覆盖「spawn → 握手 → run → result」最小闭环；本模块
//! 补齐 M3 DoD（`doc/architecture-v2.md` §3.4/§6.1、`schemas/worker-protocol-v1.md` §4/§6）：
//!
//! - **进程组**：Unix `process_group(0)` 启动（pgid = child pid），超时/取消/协议错误
//!   对**进程组**发信号，孙进程不残留（D1：超时 = kill，5s 内无残留）。
//! - **取消升级**：`cancel` 帧 → 协作宽限 → `SIGTERM` → 宽限 → `SIGKILL` 进程组（§4.3）。
//! - **退出约定**：`result` 后 `exit_grace` 内必须退出，否则 `NODE_WORKER_PROTOCOL` 并杀组；
//!   信号退出 / 非约定退出码映射为崩溃（§3）。
//! - **帧空闲超时**：仅在「非 run-result 等待」生效；run-result 等待在
//!   `RunRequest.deadline_ms != 0` 时豁免（内核 deadline 判定），`deadline_ms == 0`
//!   时用 `frame_timeout` 兜底（Lead 批准口径；见 [`SupervisorConfig::run_frame_timeout`]）。
//!   超时按 `NODE_WORKER_PROTOCOL` 返回，**不**映射 `NodeError::Timeout`（04 §4.1.4）。
//! - **流式读取上限**：emit 帧累计条数/字节超限丢弃并计数，避免 OOM（§4.2）。
//! - **stderr**：独立 task 持续排空，仅保留末尾 `max_stderr_bytes`（默认 8 KiB，§4.2）。

use std::future::Future;
use std::pin::Pin;
use std::process::{ExitStatus, Stdio};
use std::time::Duration;

use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader, BufWriter};
use tokio::process::{Child, ChildStderr, ChildStdin, ChildStdout, Command};
use tokio::task::JoinHandle;

use crate::protocol::{
    decode_frame_line, Frame, Hello, WorkerError, MAX_FRAME_BYTES, PROTOCOL_VERSION,
};
use crate::spawn::{handshake_failure, protocol_failure, WorkerConfig, WorkerFailure, WorkerRun};

/// 外部取消信号：future 完成即表示调用方请求取消（supervisor 按协议 §4.3 升级）。
pub type CancelSignal = Pin<Box<dyn Future<Output = ()> + Send>>;

/// emit 帧回调：supervisor 每收到并接纳一帧就同步回调一次（用于即时转 `ctx.emit`）。
pub type FrameSink<'a> = &'a mut (dyn FnMut(&Frame) + Send);

/// 永不触发的外部取消信号（M0 兼容路径）。
pub fn no_cancel() -> CancelSignal {
    Box::pin(std::future::pending())
}

/// emit 流式读取上限：累计超过任一上限即丢弃后续 emit（可丢，协议 §2/§4.2）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StreamingLimits {
    /// 最多保留的 emit 帧数（默认 [`DEFAULT_MAX_EMIT_EVENTS`]）。
    pub max_events: usize,
    /// 最多保留的 emit 帧字节数，按帧 JSON 长度累计（默认 [`DEFAULT_MAX_EMIT_BYTES`]）。
    pub max_bytes: usize,
}

/// emit 帧数默认上限：与内核 streaming 通道容量（1024）同量级且留余量。
pub const DEFAULT_MAX_EMIT_EVENTS: usize = 4096;
/// emit 累计字节默认上限：8 MiB（约等于单帧上限，避免无界聚合）。
pub const DEFAULT_MAX_EMIT_BYTES: usize = 8 * 1024 * 1024;
/// 帧空闲超时默认值：协议 §4.4 的 `worker_frame_timeout_ms = 300s`。
pub const DEFAULT_FRAME_TIMEOUT: Duration = Duration::from_secs(300);
/// cancel 帧后的协作宽限：协议 §4.3 的 2s。
pub const DEFAULT_CANCEL_GRACE: Duration = Duration::from_secs(2);
/// SIGTERM 后的宽限：协议 §4.3 的 1s。
pub const DEFAULT_TERM_GRACE: Duration = Duration::from_secs(1);
/// 进程被杀后等待 reap 的上限（D1：≤5s 内无残留）。
const REAP_TIMEOUT: Duration = Duration::from_secs(2);
/// result 后检测残余帧/EOF 的上限（孙进程可能仍持有 stdout，不能无限等）。
const TRAILING_READ_TIMEOUT: Duration = Duration::from_millis(200);
/// 进程退出后等待 stderr 排空任务收尾的上限（管道已 EOF，正常远小于此值）。
const STDERR_DRAIN_TIMEOUT: Duration = Duration::from_secs(1);

impl Default for StreamingLimits {
    fn default() -> Self {
        Self {
            max_events: DEFAULT_MAX_EMIT_EVENTS,
            max_bytes: DEFAULT_MAX_EMIT_BYTES,
        }
    }
}

/// emit 帧统计：`delivered + dropped` = 收到的 emit 帧数。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct StreamingStats {
    /// 已接纳（保留并回调）的帧数。
    pub delivered: u64,
    /// 因条数/字节上限丢弃的帧数。
    pub dropped: u64,
    /// 已接纳帧的累计字节。
    pub bytes: u64,
}

/// supervisor 配置：M0 [`WorkerConfig`] + M3 生命周期参数。
#[derive(Debug, Clone)]
pub struct SupervisorConfig {
    /// M0 兼容的进程/超时配置。
    pub worker: WorkerConfig,
    /// 帧空闲超时（协议 §4.4；口径见 [`SupervisorConfig::run_frame_timeout`]）。
    pub frame_timeout: Duration,
    /// cancel 帧后的协作宽限（协议 §4.3：默认 2s）。
    pub cancel_grace: Duration,
    /// SIGTERM 后的宽限（协议 §4.3：默认 1s）。
    pub term_grace: Duration,
    /// emit 流式读取上限。
    pub streaming: StreamingLimits,
}

impl Default for SupervisorConfig {
    fn default() -> Self {
        Self::from_worker(WorkerConfig::default())
    }
}

impl SupervisorConfig {
    /// 以 M0 配置为基线，补齐 M3 默认参数。
    pub fn from_worker(worker: WorkerConfig) -> Self {
        Self {
            worker,
            frame_timeout: DEFAULT_FRAME_TIMEOUT,
            cancel_grace: DEFAULT_CANCEL_GRACE,
            term_grace: DEFAULT_TERM_GRACE,
            streaming: StreamingLimits::default(),
        }
    }

    /// run-result 等待的帧空闲超时口径（Lead 批准的方案 A）。
    ///
    /// 协议 §4.4「除 run 期间外」：run 期由**内核 deadline** 兜底，因此
    /// `deadline_ms != 0` 时 supervisor 不施加帧超时（豁免）；`deadline_ms == 0`
    /// 表示调用方没有提供内核 deadline（M0 直接调用），此时用 `frame_timeout`
    /// 兜底，否则 worker 停滞会无限等待。
    pub fn run_frame_timeout(&self, request: &crate::protocol::RunRequest) -> Option<Duration> {
        if request.deadline_ms == 0 {
            Some(self.frame_timeout)
        } else {
            None
        }
    }
}

/// 进程组信号（Unix 用 nix 安全封装；非 Unix 退化为直接 kill 子进程）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GroupSignal {
    /// `SIGTERM`：取消升级的中间阶段。
    Term,
    /// `SIGKILL`：最终回收，保证无残留。
    Kill,
}

/// 向进程组发信号（负 pid = 整组）；失败忽略（进程可能已退出）。
#[cfg(unix)]
fn send_group_signal(pgid: i32, signal: GroupSignal) {
    use nix::sys::signal::{kill, Signal};
    use nix::unistd::Pid;

    let signal = match signal {
        GroupSignal::Term => Signal::SIGTERM,
        GroupSignal::Kill => Signal::SIGKILL,
    };
    let _ = kill(Pid::from_raw(-pgid), signal);
}

/// 非 Unix 平台没有进程组语义，退化为 `Child::start_kill`（见 [`Supervisor::signal`]）。
#[cfg(not(unix))]
fn send_group_signal(_pgid: i32, _signal: GroupSignal) {}

/// 单个 worker 的 supervisor：持有子进程、协议管线与回收状态。
///
/// `Drop` 兜底：内核超时/取消会 **abort** 执行任务（drop 本 future），此时无法
/// `await` 回收，因此 Drop 里必须同步给进程组发 `SIGKILL`（D1：无残留）。
pub struct Supervisor {
    child: Child,
    stdin: Option<BufWriter<ChildStdin>>,
    reader: BufReader<ChildStdout>,
    stderr_task: Option<JoinHandle<String>>,
    config: SupervisorConfig,
    /// `Some(pgid)` = 进程组仍 armed（Drop 时补杀）；成功/已 reap 后置 None。
    pgid: Option<i32>,
    /// 已 reap 的退出状态（避免重复 wait）。
    exit: Option<ExitStatus>,
    /// 握手成功的 `hello`（ISSUE-082：worker/SDK/Python 版本来源；失败为 `None`）。
    hello: Option<Hello>,
    streaming: StreamingStats,
    events: Vec<Frame>,
}

impl Drop for Supervisor {
    fn drop(&mut self) {
        self.signal(GroupSignal::Kill);
    }
}

impl Supervisor {
    /// 以独立进程组启动 worker（Unix `process_group(0)`：pgid = child pid）。
    pub async fn spawn(config: SupervisorConfig) -> Result<Self, WorkerFailure> {
        let mut command = Command::new(&config.worker.program);
        command
            .args(&config.worker.args)
            .kill_on_drop(true)
            .env("PYTHONHASHSEED", "0")
            .env("PYTHONUNBUFFERED", "1")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        for (key, value) in &config.worker.env {
            command.env(key, value);
        }
        if let Some(cwd) = &config.worker.cwd {
            command.current_dir(cwd);
        }
        #[cfg(unix)]
        command.process_group(0);

        let mut child = command
            .spawn()
            .map_err(|err| WorkerFailure::Spawn(err.to_string()))?;
        let pgid = child.id().and_then(|pid| i32::try_from(pid).ok());
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| protocol_failure("no stdin"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| protocol_failure("no stdout"))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| protocol_failure("no stderr"))?;
        let stderr_task = tokio::spawn(drain_stderr(stderr, config.worker.max_stderr_bytes));
        Ok(Self {
            child,
            stdin: Some(BufWriter::new(stdin)),
            reader: BufReader::new(stdout),
            stderr_task: Some(stderr_task),
            config,
            pgid,
            exit: None,
            hello: None,
            streaming: StreamingStats::default(),
            events: Vec::new(),
        })
    }

    /// 握手成功时记录的 `hello`（ISSUE-082：版本聚合来源；未握手为 `None`）。
    pub fn hello(&self) -> Option<&Hello> {
        self.hello.as_ref()
    }

    /// 完整生命周期：握手 → 发送 run → 收集 emit/result/error → reap。
    ///
    /// `cancel` 在任一等待点优先被轮询（`biased`）：一旦完成立即走协议 §4.3 的升级链。
    pub(crate) async fn drive(
        &mut self,
        request: &crate::protocol::RunRequest,
        on_frame: FrameSink<'_>,
        cancel: &mut CancelSignal,
    ) -> Result<WorkerRun, WorkerFailure> {
        self.hello = Some(self.handshake(cancel).await?);
        self.write_run(request).await?;
        let idle = self.config.run_frame_timeout(request);
        loop {
            let frame = tokio::select! {
                biased;
                _ = &mut *cancel => return Err(WorkerFailure::Cancelled),
                result = read_frame_bounded(&mut self.reader, idle) => result?,
            };
            let Some(frame) = frame else {
                // EOF 先于 result：必须看退出状态才能区分「exit 0 无 result」（协议错误）
                // 与「被信号杀死 / 非约定退出码」（崩溃，§3 封闭表）。
                return self.finish_eof_before_result().await;
            };
            match frame.kind.as_str() {
                "emit" => self.deliver_emit(frame, on_frame),
                // `log` 帧是结构化排障信息；M3 只保证不阻塞读取（stderr 另行收集）。
                "log" => {}
                "result" => return self.finish_result(frame).await,
                "error" => return self.finish_error(frame).await,
                other => return Err(protocol_failure(format!("unexpected frame kind: {other}"))),
            }
        }
    }

    /// 握手：`hello_timeout` 内必须收到合法 `hello`，随后回 `hello_ack`（§2）。
    async fn handshake(&mut self, cancel: &mut CancelSignal) -> Result<Hello, WorkerFailure> {
        let timeout = self.config.worker.hello_timeout;
        let read = tokio::select! {
            biased;
            _ = &mut *cancel => return Err(WorkerFailure::Cancelled),
            result = tokio::time::timeout(timeout, read_frame_line(&mut self.reader)) => result,
        };
        let frame = match read {
            Err(_) => return Err(handshake_failure("hello timeout")),
            Ok(result) => result?,
        };
        let Some(frame) = frame else {
            return Err(handshake_failure("worker exited before hello"));
        };
        if frame.kind != "hello" {
            return Err(handshake_failure(format!(
                "expected hello, got {}",
                frame.kind
            )));
        }
        let hello: Hello = serde_json::from_value(frame.payload)
            .map_err(|err| handshake_failure(format!("bad hello payload: {err}")))?;
        if hello.protocol_version != PROTOCOL_VERSION {
            return Err(handshake_failure(format!(
                "worker protocol {} != {}",
                hello.protocol_version, PROTOCOL_VERSION
            )));
        }
        let ack = Frame::new("hello_ack", serde_json::json!({"accepted": true}));
        self.write(&ack).await?;
        Ok(hello)
    }

    /// 发送 `run` 帧（`inputs` 已在调用方补齐全部声明端口）。
    async fn write_run(
        &mut self,
        request: &crate::protocol::RunRequest,
    ) -> Result<(), WorkerFailure> {
        let payload = serde_json::to_value(request)
            .map_err(|err| protocol_failure(format!("encode run request: {err}")))?;
        self.write(&Frame::new("run", payload)).await
    }

    /// 写一帧到 worker stdin 并 flush。
    async fn write(&mut self, frame: &Frame) -> Result<(), WorkerFailure> {
        let Some(stdin) = self.stdin.as_mut() else {
            return Err(protocol_failure("worker stdin closed"));
        };
        let mut line = frame.to_line()?;
        line.push('\n');
        stdin
            .write_all(line.as_bytes())
            .await
            .map_err(|err| protocol_failure(format!("write frame: {err}")))?;
        stdin
            .flush()
            .await
            .map_err(|err| protocol_failure(format!("flush frame: {err}")))
    }

    /// 接纳一帧 emit：先按上限判定丢弃/保留，再回调（流式即时转发）并暂存。
    fn deliver_emit(&mut self, frame: Frame, on_frame: FrameSink<'_>) {
        let size = serde_json::to_vec(&frame).map_or(0, |bytes| bytes.len());
        let limits = self.config.streaming;
        let next_bytes = self.streaming.bytes.saturating_add(size as u64);
        if self.events.len() >= limits.max_events || next_bytes > limits.max_bytes as u64 {
            self.streaming.dropped += 1;
            return;
        }
        self.streaming.delivered += 1;
        self.streaming.bytes = next_bytes;
        on_frame(&frame);
        self.events.push(frame);
    }

    /// stdout 在 `result` 前 EOF：按退出状态区分协议错误与崩溃（§3 封闭表）。
    async fn finish_eof_before_result(&mut self) -> Result<WorkerRun, WorkerFailure> {
        match self.wait_exit(self.config.worker.exit_grace).await {
            Some(status) if status.success() => {
                Err(protocol_failure("worker closed stdout before result"))
            }
            Some(status) => Err(Self::classify_exit(status)),
            None => {
                self.kill_and_reap().await;
                Err(protocol_failure("worker closed stdout before result"))
            }
        }
    }

    /// `result` 分支：恰好一帧 + `exit_grace` 内 exit 0 才算成功（§3 封闭表）。
    async fn finish_result(&mut self, frame: Frame) -> Result<WorkerRun, WorkerFailure> {
        let output = frame
            .payload
            .get("output")
            .cloned()
            .ok_or_else(|| protocol_failure("result without output"))?;
        match self.wait_exit(self.config.worker.exit_grace).await {
            Some(status) if status.success() => {}
            Some(status) => return Err(Self::classify_exit(status)),
            None => {
                self.kill_and_reap().await;
                return Err(protocol_failure("worker did not exit after result"));
            }
        }
        self.ensure_no_trailing_frame().await?;
        let stderr = self.take_stderr().await;
        self.pgid = None;
        self.stdin = None;
        Ok(WorkerRun {
            output,
            events: std::mem::take(&mut self.events),
            stderr,
            streaming: self.streaming,
        })
    }

    /// `error` 分支：只有 `exit 2 + 结构化 error` 才是业务失败（§3 封闭表）。
    async fn finish_error(&mut self, frame: Frame) -> Result<WorkerRun, WorkerFailure> {
        let error: WorkerError = serde_json::from_value(frame.payload)
            .map_err(|err| protocol_failure(format!("bad error payload: {err}")))?;
        match self.wait_exit(self.config.worker.exit_grace).await {
            Some(status) if status.code() == Some(2) => Err(WorkerFailure::Worker(Box::new(error))),
            Some(status) if status.success() => Err(protocol_failure(
                "error frame but worker exited with code 0",
            )),
            Some(status) => Err(Self::classify_exit(status)),
            None => {
                self.kill_and_reap().await;
                Err(protocol_failure("worker did not exit after error"))
            }
        }
    }

    /// 退出码分类：exit 3 = SDK 自检失败（协议错误）；其余非 0/信号 = 崩溃（§3）。
    ///
    /// `Exit` 同时记录信号编号与退出码（ISSUE-066：architecture §8.2 要求
    /// `NODE_WORKER_CRASH` 留下 signal/exit 证据；二者互斥）。
    fn classify_exit(status: ExitStatus) -> WorkerFailure {
        match status.code() {
            Some(3) => protocol_failure("worker exited with code 3 (sdk self-check failed)"),
            code => WorkerFailure::Exit {
                code,
                signal: exit_signal(&status),
                stderr: String::new(),
            },
        }
    }

    /// `result` 后读残余帧：重复 result / 多余帧 = 协议错误，EOF = 正常。
    ///
    /// 进程已 reap，但孙进程可能继承 stdout 未关闭，故读操作必须有上限（否则挂死）。
    async fn ensure_no_trailing_frame(&mut self) -> Result<(), WorkerFailure> {
        let read =
            tokio::time::timeout(TRAILING_READ_TIMEOUT, read_frame_line(&mut self.reader)).await;
        match read {
            Ok(Ok(Some(_))) => Err(protocol_failure("duplicate/trailing frame after result")),
            Ok(Ok(None)) | Err(_) => Ok(()),
            Ok(Err(err)) => Err(err),
        }
    }

    /// 等待子进程退出（带宽限）；结果缓存到 `self.exit`，重复 wait 不会二次消费。
    async fn wait_exit(&mut self, grace: Duration) -> Option<ExitStatus> {
        if let Some(status) = self.exit {
            return Some(status);
        }
        match tokio::time::timeout(grace, self.child.wait()).await {
            Ok(Ok(status)) => {
                self.exit = Some(status);
                Some(status)
            }
            _ => None,
        }
    }

    /// 给进程组发信号；非 Unix 平台退化为直接 kill 子进程。
    fn signal(&mut self, signal: GroupSignal) {
        if let Some(pgid) = self.pgid {
            send_group_signal(pgid, signal);
        }
        #[cfg(not(unix))]
        let _ = self.child.start_kill();
    }

    /// 立即 `SIGKILL` 进程组并 reap（超时/协议错误路径；D1 无残留）。
    async fn kill_and_reap(&mut self) {
        if self.exit.is_none() {
            self.signal(GroupSignal::Kill);
            if self.wait_exit(REAP_TIMEOUT).await.is_none() {
                // 进程组信号失败（如平台不支持）时兜底直接 kill。
                let _ = self.child.start_kill();
                let _ = self.wait_exit(REAP_TIMEOUT).await;
            }
        }
        self.pgid = None;
        self.stdin = None;
    }

    /// 协议 §4.3 的取消升级链：cancel 帧 → 宽限 → SIGTERM → 宽限 → SIGKILL 进程组。
    async fn cancel_and_reap(&mut self) -> WorkerFailure {
        let cancel = Frame::new("cancel", serde_json::json!({}));
        let _ = self.write(&cancel).await;
        if self.wait_exit(self.config.cancel_grace).await.is_some() {
            self.disarm();
            return WorkerFailure::Cancelled;
        }
        self.signal(GroupSignal::Term);
        if self.wait_exit(self.config.term_grace).await.is_some() {
            self.disarm();
            return WorkerFailure::Cancelled;
        }
        self.signal(GroupSignal::Kill);
        let _ = self.wait_exit(REAP_TIMEOUT).await;
        self.disarm();
        WorkerFailure::Cancelled
    }

    /// 回收完成：关闭 stdin 并解除 Drop 的进程组补杀（避免 PID 复用误杀）。
    fn disarm(&mut self) {
        self.pgid = None;
        self.stdin = None;
    }

    /// 取 stderr 尾部（最多等 1s；进程已死时管道很快 EOF）。
    async fn take_stderr(&mut self) -> String {
        let Some(task) = self.stderr_task.take() else {
            return String::new();
        };
        match tokio::time::timeout(STDERR_DRAIN_TIMEOUT, task).await {
            Ok(Ok(text)) => text,
            _ => String::new(),
        }
    }

    /// 失败路径统一带上 stderr 尾部（仅 `Exit` 变体有 stderr 字段）。
    async fn attach_stderr(&mut self, failure: WorkerFailure) -> WorkerFailure {
        let stderr = self.take_stderr().await;
        match failure {
            WorkerFailure::Exit { code, signal, .. } => WorkerFailure::Exit {
                code,
                signal,
                stderr,
            },
            other => other,
        }
    }
}

/// 提取终止进程的 Unix 信号编号；非 Unix 平台恒 `None`（ISSUE-066）。
#[cfg(unix)]
fn exit_signal(status: &ExitStatus) -> Option<i32> {
    use std::os::unix::process::ExitStatusExt;
    status.signal()
}

#[cfg(not(unix))]
fn exit_signal(_status: &ExitStatus) -> Option<i32> {
    None
}

/// 帧读取：`timeout` 为 `Some` 时超过即 `NODE_WORKER_PROTOCOL`（帧空闲超时）。
async fn read_frame_bounded(
    reader: &mut BufReader<ChildStdout>,
    timeout: Option<Duration>,
) -> Result<Option<Frame>, WorkerFailure> {
    match timeout {
        None => read_frame_line(reader).await,
        Some(limit) => match tokio::time::timeout(limit, read_frame_line(reader)).await {
            Ok(result) => result,
            Err(_) => Err(protocol_failure(format!(
                "frame idle timeout after {limit:?}"
            ))),
        },
    }
}

/// 读取一行完整帧；**有界**（> [`MAX_FRAME_BYTES`] 立即协议错误，防不可信 worker OOM）。
///
/// 为什么不用 `read_until`：它先把整行读进内存、之后才在解码层判 8 MiB；
/// 用户 Python 写一条超大无换行行即可让 supervisor/内核进程 OOM（M3「禁止 OOM、
/// 崩溃隔离」）。这里用 `fill_buf` 分块累积，超限立刻返回 `NODE_WORKER_PROTOCOL`，
/// 由调用方 kill 进程组。
async fn read_frame_line(
    reader: &mut BufReader<ChildStdout>,
) -> Result<Option<Frame>, WorkerFailure> {
    // 上限含结尾换行（解码层按 body 长度判定，二者相差 1 字节）。
    let max_buffered = MAX_FRAME_BYTES.saturating_add(1);
    let mut buffer: Vec<u8> = Vec::new();
    loop {
        let chunk = reader
            .fill_buf()
            .await
            .map_err(|err| protocol_failure(format!("read frame: {err}")))?;
        if chunk.is_empty() {
            return if buffer.is_empty() {
                Ok(None)
            } else {
                Err(protocol_failure(
                    "partial frame at EOF (missing trailing newline)",
                ))
            };
        }
        let newline = chunk.iter().position(|&byte| byte == b'\n');
        let take = newline.map_or(chunk.len(), |index| index + 1);
        if buffer.len().saturating_add(take) > max_buffered {
            return Err(protocol_failure(format!(
                "frame too large: > {MAX_FRAME_BYTES} bytes"
            )));
        }
        buffer.extend_from_slice(&chunk[..take]);
        reader.consume(take);
        if newline.is_some() {
            return decode_frame_line(&buffer)
                .map(Some)
                .map_err(WorkerFailure::from);
        }
    }
}

/// 持续排空 stderr，环形保留末尾 `max_bytes`（§4.2：不读会阻塞 worker）。
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

/// 完整 supervisor 调用：spawn → drive（可取消）→ 失败路径统一 kill/reap。
///
/// `WorkerConfig.timeout` 是**兜底**总超时；内核场景由 `deadline_ms` + abort 的
/// Drop 路径保证更早回收（D1），这里的超时只防止 M0 直接调用无限挂起。
pub async fn run_supervised(
    config: &SupervisorConfig,
    request: &crate::protocol::RunRequest,
    on_frame: FrameSink<'_>,
    cancel: CancelSignal,
) -> Result<WorkerRun, WorkerFailure> {
    run_supervised_observed(config, request, on_frame, cancel)
        .await
        .1
}

/// 同 [`run_supervised`]，但额外返回握手 `hello`（ISSUE-082：worker/SDK/Python 版本）。
///
/// `hello` 在握手成功后返回（**即使后续 run 失败**，用于失败 run 也尽力记录版本）；
/// spawn/握手失败 → `None`。协议与执行语义不变。
pub async fn run_supervised_observed(
    config: &SupervisorConfig,
    request: &crate::protocol::RunRequest,
    on_frame: FrameSink<'_>,
    cancel: CancelSignal,
) -> (Option<Hello>, Result<WorkerRun, WorkerFailure>) {
    let timeout = config.worker.timeout;
    let mut supervisor = match Supervisor::spawn(config.clone()).await {
        Ok(supervisor) => supervisor,
        Err(failure) => return (None, Err(failure)),
    };
    let mut cancel = cancel;
    let outcome =
        tokio::time::timeout(timeout, supervisor.drive(request, on_frame, &mut cancel)).await;
    let hello = supervisor.hello().cloned();
    let result = match outcome {
        Ok(Ok(run)) => Ok(run),
        Ok(Err(WorkerFailure::Cancelled)) => {
            let _ = supervisor.cancel_and_reap().await;
            Err(WorkerFailure::Cancelled)
        }
        Ok(Err(failure)) => {
            supervisor.kill_and_reap().await;
            Err(supervisor.attach_stderr(failure).await)
        }
        Err(_elapsed) => {
            supervisor.kill_and_reap().await;
            let failure = supervisor
                .attach_stderr(WorkerFailure::Timeout(timeout))
                .await;
            Err(failure)
        }
    };
    (hello, result)
}

/// 把 emit 帧载荷解析成 `serde_json::Value`（测试/调用方便捷入口）。
pub fn emit_payload(frame: &Frame) -> Option<&Value> {
    if frame.kind == "emit" {
        Some(&frame.payload)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::{RunRequest, MAX_FRAME_BYTES};

    fn request(deadline_ms: u64) -> RunRequest {
        RunRequest {
            run_id: "run".into(),
            instance_id: "inst".into(),
            node_id: "node".into(),
            attempt: 0,
            module: "m".into(),
            function: "f".into(),
            inputs: serde_json::json!({}),
            params: serde_json::json!({}),
            user_id: "u".into(),
            global_vars: serde_json::json!({}),
            seed: 0,
            deadline_ms,
            trace_id: "t".into(),
        }
    }

    #[test]
    fn run_frame_timeout_applies_only_without_kernel_deadline() {
        let config = SupervisorConfig::default();
        assert_eq!(
            config.run_frame_timeout(&request(0)),
            Some(DEFAULT_FRAME_TIMEOUT)
        );
        assert_eq!(config.run_frame_timeout(&request(1_700_000_000_000)), None);
    }

    #[test]
    fn supervisor_defaults_follow_protocol_contract() {
        let config = SupervisorConfig::default();
        assert_eq!(config.frame_timeout, Duration::from_secs(300));
        assert_eq!(config.cancel_grace, Duration::from_secs(2));
        assert_eq!(config.term_grace, Duration::from_secs(1));
        assert_eq!(config.worker.hello_timeout, Duration::from_secs(5));
        assert_eq!(config.worker.max_stderr_bytes, 8 * 1024);
        assert_eq!(config.streaming, StreamingLimits::default());
        assert_eq!(config.streaming.max_events, DEFAULT_MAX_EMIT_EVENTS);
        assert_eq!(config.streaming.max_bytes, DEFAULT_MAX_EMIT_BYTES);
    }

    /// 协议 §3 封闭表的退出码分支：exit 3 = SDK 自检失败（协议错误），其余非 0/信号 = 崩溃。
    #[cfg(unix)]
    #[test]
    fn classify_exit_maps_exit_codes_and_signals() {
        use std::os::unix::process::ExitStatusExt;

        let exit_three = ExitStatus::from_raw(3 << 8);
        match Supervisor::classify_exit(exit_three) {
            WorkerFailure::Protocol(error) => {
                assert_eq!(error.code, crate::protocol::CODE_PROTOCOL);
                assert!(
                    error.message.contains("code 3"),
                    "unexpected message: {}",
                    error.message
                );
            }
            other => panic!("exit 3 must be a protocol error, got {other:?}"),
        }

        match Supervisor::classify_exit(ExitStatus::from_raw(2 << 8)) {
            WorkerFailure::Exit {
                code: Some(2),
                signal: None,
                ..
            } => {}
            other => panic!("exit 2 must be a crash with no signal, got {other:?}"),
        }
        match Supervisor::classify_exit(ExitStatus::from_raw(0)) {
            WorkerFailure::Exit {
                code: Some(0),
                signal: None,
                ..
            } => {}
            other => panic!("exit 0 must map to Exit(0), got {other:?}"),
        }
        // 信号杀死：raw 低 7 位为信号，code() = None → 崩溃；signal 必须有值（ISSUE-066）。
        match Supervisor::classify_exit(ExitStatus::from_raw(9)) {
            WorkerFailure::Exit {
                code: None,
                signal: Some(9),
                ..
            } => {}
            other => panic!("signal death must record SIGKILL, got {other:?}"),
        }
    }

    #[test]
    fn streaming_limits_stay_within_single_frame_bound() {
        assert_eq!(
            DEFAULT_MAX_EMIT_BYTES, MAX_FRAME_BYTES,
            "流式字节上限与单帧上限同量级，避免无界聚合"
        );
    }
}
