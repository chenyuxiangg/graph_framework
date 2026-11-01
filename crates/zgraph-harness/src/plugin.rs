//! 外部 harness（子进程 adapter）协议实现（v0.3/V4；DR-011 §2/§5/§6）。
//!
//! 协议 `zgraph-harness/1`：NDJSON over stdio。
//! `hello/hello_ok`（5s 握手）→ 每节点一次 `run` → `event`（仅既有 5 种
//! `StreamingEvent`）→ `result` / `error`；`cancel` 控制帧。
//!
//! 硬约束（DR-011）：
//! - 每 run 每 provider **常驻**一个 adapter 进程（懒启动；`cwd = assets_dir`）；
//! - 外部进程**不得**写 trace/状态机/`llm_calls`（只经本模块返回四字段输出）；
//! - 输出上限：行/final ≤1 MiB、总输出 ≤8 MiB、事件 ≤20000、stderr ≤8 KiB；
//! - 取消：`cancel` 帧 → 关 stdin → `SIGTERM` 2s → `SIGKILL` 进程组（Unix）；
//! - 节点超时 → `NODE_TIMEOUT` 语义（[`RunFailure::InstanceTimeout`]）；
//!   协议级超时/握手/版本/协议错误 → `HARNESS_PLUGIN_ERROR{reason}`（无独立超时码）。

use std::collections::HashMap;
use std::path::Path;
use std::process::Stdio;
use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout};
use tokio::sync::mpsc;
use tokio::sync::Mutex;
use zgraph_kernel::StreamingEvent;

use crate::agent_loop::{AgentOutput, RunFailure};
use crate::config::HarnessConfig;
use crate::constants::{
    HARNESS_PLUGIN_CANCEL_GRACE_MS, HARNESS_PLUGIN_DEFAULT_TIMEOUT_MS,
    HARNESS_PLUGIN_FINAL_MAX_BYTES, HARNESS_PLUGIN_HANDSHAKE_TIMEOUT_MS,
    HARNESS_PLUGIN_LINE_MAX_BYTES, HARNESS_PLUGIN_MAX_EVENTS, HARNESS_PLUGIN_MAX_INVALID_LINES,
    HARNESS_PLUGIN_PROTOCOL, HARNESS_PLUGIN_PROTOCOL_VERSION, HARNESS_PLUGIN_READ_POLL_MS,
    HARNESS_PLUGIN_STDERR_MAX_BYTES, HARNESS_PLUGIN_TOTAL_OUTPUT_MAX_BYTES,
};
use crate::error::{HarnessError, Stage};
use crate::llm::{StopReason, Usage};
use crate::provider::{AgentProvider, AgentProviderKind, ProviderRequest};

/// 每 run 的 adapter 池（run_id → provider key → adapter）。
type AdapterPool = HashMap<String, HashMap<String, Arc<Mutex<Adapter>>>>;

/// 外部 harness provider（每 run 每 provider 常驻 adapter）。
///
/// 进程池键 = `run_id` → `provider key`（command/args/env/timeout 指纹）；
/// 同一键的多次 `agent_call` 复用同一 adapter 进程（session 由 `run` 帧驱动）。
/// [`SubprocessProvider::close_provider`] 幂等关闭全部进程组（CLI 所有 run 出口调用）。
pub struct SubprocessProvider {
    runs: Mutex<AdapterPool>,
}

impl std::fmt::Debug for SubprocessProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SubprocessProvider").finish_non_exhaustive()
    }
}

impl Default for SubprocessProvider {
    fn default() -> Self {
        Self {
            runs: Mutex::new(HashMap::new()),
        }
    }
}

impl SubprocessProvider {
    /// 空 provider（无进程）。
    pub fn new() -> Self {
        Self::default()
    }

    /// 关闭全部外部 harness 进程（幂等；返回实际关闭的进程数）。
    ///
    /// 依据 DR-011 §2.2：关 stdin → `SIGTERM` → 宽限 2s → `SIGKILL` 进程组；
    /// run 结束/取消/任一 CLI run 出口均调用本函数（V3 `close_mcp` 同模式）。
    pub async fn close_provider(&self) -> usize {
        let runs = {
            let mut guard = self.runs.lock().await;
            std::mem::take(&mut *guard)
        };
        let mut closed = 0;
        for sessions in runs.into_values() {
            for session in sessions.into_values() {
                let mut adapter = session.lock().await;
                if !adapter.closed {
                    adapter.shutdown().await;
                    closed += 1;
                }
            }
        }
        closed
    }

    /// 当前存活 adapter 进程数（测试/诊断用）。
    pub async fn live_adapter_count(&self) -> usize {
        let guard = self.runs.lock().await;
        guard.values().map(HashMap::len).sum()
    }

    /// 取（或懒启动）本 run 本 provider 的 adapter；换 run 时先关闭旧 run 的进程组。
    async fn adapter_for(
        &self,
        config: &HarnessConfig,
        request: &ProviderRequest<'_>,
    ) -> Result<Arc<Mutex<Adapter>>, HarnessError> {
        let run_id = request.trace.run_id.clone();
        let key = provider_key(config);
        let mut guard = self.runs.lock().await;
        // 换 run：关闭并移除旧 run 的 provider 进程（每 run 常驻、跨 run 必须回收）。
        let stale: Vec<String> = guard
            .keys()
            .filter(|existing| existing.as_str() != run_id)
            .cloned()
            .collect();
        for stale_id in stale {
            if let Some(sessions) = guard.remove(&stale_id) {
                for session in sessions.into_values() {
                    session.lock().await.shutdown().await;
                }
            }
        }
        let sessions = guard.entry(run_id.clone()).or_default();
        if let Some(existing) = sessions.get(&key) {
            if !existing.lock().await.closed {
                return Ok(Arc::clone(existing));
            }
        }
        let mut adapter = Adapter::spawn(config, request.cwd)?;
        adapter.handshake(&run_id, &request.trace.node_id).await?;
        let adapter = Arc::new(Mutex::new(adapter));
        sessions.insert(key, Arc::clone(&adapter));
        Ok(adapter)
    }
}

#[async_trait]
impl AgentProvider for SubprocessProvider {
    fn kind(&self) -> AgentProviderKind {
        AgentProviderKind::Subprocess
    }

    async fn run(&self, request: ProviderRequest<'_>) -> Result<AgentOutput, RunFailure> {
        let Some(config) = request
            .agent
            .harness
            .as_ref()
            .filter(|harness| harness.is_subprocess())
        else {
            return Err(RunFailure::Harness(HarnessError::config_load(
                "provider_not_implemented",
                None,
                Some("$.harness.kind"),
                Some(json!("subprocess")),
                Some(json!("builtin")),
                "SubprocessProvider requires AgentConfig.harness.kind = 'subprocess'".to_string(),
            )));
        };
        let adapter = self
            .adapter_for(config, &request)
            .await
            .map_err(RunFailure::Harness)?;
        // 注意锁序：adapter 锁内不获取 pool 锁（避免与 adapter_for 的 pool→adapter 死锁）。
        let (closed, outcome) = {
            let mut guard = adapter.lock().await;
            if guard.closed {
                (
                    true,
                    Err(RunFailure::Harness(plugin_error(
                        "exit_nonzero",
                        "external harness process is not available",
                        None,
                    ))),
                )
            } else {
                let outcome = guard.exchange(config, &request).await;
                (guard.closed, outcome)
            }
        };
        if closed {
            // 终结性失败已杀进程：从池中移除，下一次调用重新拉起。
            let key = provider_key(config);
            let mut runs = self.runs.lock().await;
            if let Some(sessions) = runs.get_mut(&request.trace.run_id) {
                sessions.remove(&key);
                if sessions.is_empty() {
                    runs.remove(&request.trace.run_id);
                }
            }
        }
        outcome
    }
}

/// provider 进程指纹（同 run 内复用键）。
fn provider_key(config: &HarnessConfig) -> String {
    json!({
        "command": config.command,
        "args": config.args,
        "env": config.env,
        "timeout_ms": config.timeout_ms,
    })
    .to_string()
}

/// 读取侧错误（reader task → 主循环）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReadError {
    /// 子进程 stdout EOF（无 `result`）。
    Eof,
    /// 单行超过 1 MiB。
    LineTooLarge,
    /// 总输出超过 8 MiB。
    OutputLimit,
    /// 非法 JSON 行（warn 计数，超阈值终止）。
    InvalidJson,
}

/// 单个外部 harness adapter 进程与其协议状态。
struct Adapter {
    child: Child,
    stdin: Option<ChildStdin>,
    frames: mpsc::Receiver<Result<Value, ReadError>>,
    stderr: Arc<Mutex<Vec<u8>>>,
    stderr_task: Option<tokio::task::JoinHandle<()>>,
    reader_task: Option<tokio::task::JoinHandle<()>>,
    pgid: Option<i32>,
    streaming: bool,
    session_key: Option<String>,
    closed: bool,
}

impl Drop for Adapter {
    fn drop(&mut self) {
        // 兜底：executor/CLI 异常退出时不留孤儿进程组（与 V3 MCP/worker 同模式）。
        if !self.closed {
            if let Some(pgid) = self.pgid {
                send_group_signal(pgid, GroupSignal::Kill);
            }
            let _ = self.child.start_kill();
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GroupSignal {
    Term,
    Kill,
}

/// 向进程组发信号（负 pid = 整组；失败忽略，进程可能已退出）。
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

/// 非 Unix 平台没有进程组语义（由 `Drop`/`start_kill` 兜底）。
#[cfg(not(unix))]
fn send_group_signal(_pgid: i32, _signal: GroupSignal) {}

impl Adapter {
    fn spawn(config: &HarnessConfig, cwd: &Path) -> Result<Self, HarnessError> {
        let Some(program) = config.command.as_deref() else {
            return Err(plugin_error(
                "spawn_failed",
                "subprocess harness has no command",
                None,
            ));
        };
        let mut command = tokio::process::Command::new(program);
        command
            .args(&config.args)
            .current_dir(cwd)
            .kill_on_drop(true)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        for (key, value) in &config.env {
            command.env(key, value);
        }
        #[cfg(unix)]
        {
            // tokio::process::Command 暴露 std 扩展：独立进程组（pgid = child pid）。
            command.process_group(0);
        }
        let mut child = command.spawn().map_err(|error| {
            plugin_error(
                "spawn_failed",
                &format!("failed to spawn external harness '{program}': {error}"),
                Some(json!({"provider": program})),
            )
        })?;
        let pgid = child.id().and_then(|pid| i32::try_from(pid).ok());
        let stdin = child.stdin.take();
        let Some(stdout) = child.stdout.take() else {
            return Err(plugin_error(
                "spawn_failed",
                "external harness stdout was not piped",
                None,
            ));
        };
        let stderr = Arc::new(Mutex::new(Vec::new()));
        let stderr_task = child.stderr.take().map(|pipe| {
            let buffer = Arc::clone(&stderr);
            tokio::spawn(drain_stderr(pipe, buffer))
        });
        let (tx, frames) = mpsc::channel(64);
        let reader_task = Some(tokio::spawn(read_frames(BufReader::new(stdout), tx)));
        Ok(Self {
            child,
            stdin,
            frames,
            stderr,
            stderr_task,
            reader_task,
            pgid,
            streaming: false,
            session_key: None,
            closed: false,
        })
    }

    async fn handshake(&mut self, run_id: &str, node_id: &str) -> Result<(), HarnessError> {
        let hello = json!({
            "v": HARNESS_PLUGIN_PROTOCOL_VERSION,
            "type": "hello",
            "protocol": HARNESS_PLUGIN_PROTOCOL,
            "run_id": run_id,
            "node_id": node_id,
            "provider": "external",
            "limits": {
                "node_timeout_ms": HARNESS_PLUGIN_DEFAULT_TIMEOUT_MS,
                "max_output_bytes": HARNESS_PLUGIN_LINE_MAX_BYTES,
                "max_events": HARNESS_PLUGIN_MAX_EVENTS
            }
        });
        self.write_frame(&hello, "handshake").await?;
        let frame = tokio::time::timeout(
            Duration::from_millis(HARNESS_PLUGIN_HANDSHAKE_TIMEOUT_MS),
            self.next_frame(),
        )
        .await
        .map_err(|_| plugin_error("handshake", "external harness hello_ok timed out", None))??;
        if frame.get("type").and_then(Value::as_str) != Some("hello_ok") {
            return Err(plugin_error(
                "handshake",
                "external harness did not answer hello_ok",
                None,
            ));
        }
        if frame.get("v").and_then(Value::as_u64)
            != Some(u64::from(HARNESS_PLUGIN_PROTOCOL_VERSION))
        {
            return Err(plugin_error(
                "version",
                "external harness protocol version mismatch",
                None,
            ));
        }
        if frame.get("protocol").and_then(Value::as_str) != Some(HARNESS_PLUGIN_PROTOCOL) {
            return Err(plugin_error(
                "version",
                &format!("external harness protocol mismatch: expected {HARNESS_PLUGIN_PROTOCOL}"),
                None,
            ));
        }
        self.streaming = frame
            .get("capabilities")
            .and_then(|capabilities| capabilities.get("streaming"))
            .and_then(Value::as_bool)
            .unwrap_or(false);
        Ok(())
    }

    /// 一次节点执行：发 `run` 帧并接收 `event*` → `result`/`error`。
    async fn exchange(
        &mut self,
        config: &HarnessConfig,
        request: &ProviderRequest<'_>,
    ) -> Result<AgentOutput, RunFailure> {
        // 上一轮 result 之后可能还有迟到帧：开始新一轮前丢弃（不跨调用混帧）。
        while self.frames.try_recv().is_ok() {}
        let session_key = format!("{}:{}", request.trace.run_id, request.agent.id);
        let reset =
            self.session_key.as_deref() != Some(session_key.as_str()) || request.trace.attempt > 0;
        self.session_key = Some(session_key.clone());
        let run_frame = json!({
            "v": HARNESS_PLUGIN_PROTOCOL_VERSION,
            "type": "run",
            "agent": {
                "id": request.agent.id,
                "model": request.agent.model.name,
                "temperature": request.agent.model.temperature,
                "max_tokens": request.agent.model.max_tokens,
            },
            "session": {"reuse_key": session_key, "reset": reset},
            "inputs": request.inputs,
            "trace": {
                "run_id": request.trace.run_id,
                "instance_id": request.trace.instance_id,
                "node_id": request.trace.node_id,
                "attempt": request.trace.attempt,
                "item_index": request.trace.item_index,
            },
        });
        if let Err(error) = self.write_frame(&run_frame, "protocol").await {
            self.shutdown().await;
            return Err(RunFailure::Harness(error));
        }
        let mut timeout_ms = config
            .timeout_ms
            .unwrap_or(HARNESS_PLUGIN_DEFAULT_TIMEOUT_MS)
            .min(request.node_timeout_ms.max(1));
        // 内核 deadline 可能比解析后的实例超时更紧：取两者较小值（DR-011 §2.2）。
        let kernel_remaining = request.ctx.node_deadline_remaining_ms();
        if kernel_remaining == 0 || kernel_remaining < timeout_ms {
            timeout_ms = kernel_remaining.max(1);
        }
        let deadline = Instant::now() + Duration::from_millis(timeout_ms);
        let started = Instant::now();
        let mut events = 0usize;
        let mut invalid_lines = 0u32;
        let mut tool_names: HashMap<String, String> = HashMap::new();
        loop {
            if request.ctx.cancel_requested() {
                self.shutdown().await;
                return Err(RunFailure::Cancelled);
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                let elapsed_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
                self.shutdown().await;
                return Err(RunFailure::InstanceTimeout { elapsed_ms });
            }
            let poll = remaining.min(Duration::from_millis(HARNESS_PLUGIN_READ_POLL_MS));
            let received = tokio::time::timeout(poll, self.frames.recv()).await;
            let frame = match received {
                Err(_) => continue,
                Ok(None) | Ok(Some(Err(ReadError::Eof))) => {
                    return Err(RunFailure::Harness(self.exit_failure().await));
                }
                Ok(Some(Err(ReadError::InvalidJson))) => {
                    invalid_lines = invalid_lines.saturating_add(1);
                    if invalid_lines >= HARNESS_PLUGIN_MAX_INVALID_LINES {
                        self.shutdown().await;
                        return Err(RunFailure::Harness(plugin_error(
                            "protocol",
                            "external harness emitted too many invalid JSON lines",
                            Some(json!({"invalid_lines": invalid_lines})),
                        )));
                    }
                    eprintln!(
                        "zgraph: warn: external harness emitted invalid JSON line ({invalid_lines}/{HARNESS_PLUGIN_MAX_INVALID_LINES})"
                    );
                    continue;
                }
                Ok(Some(Err(ReadError::LineTooLarge))) => {
                    self.shutdown().await;
                    return Err(RunFailure::Harness(plugin_error(
                        "line_too_large",
                        "external harness line exceeds the 1 MiB limit",
                        None,
                    )));
                }
                Ok(Some(Err(ReadError::OutputLimit))) => {
                    self.shutdown().await;
                    return Err(RunFailure::Harness(plugin_error(
                        "output_limit",
                        "external harness total output exceeds the 8 MiB limit",
                        None,
                    )));
                }
                Ok(Some(Ok(frame))) => frame,
            };
            if frame.get("v").and_then(Value::as_u64)
                != Some(u64::from(HARNESS_PLUGIN_PROTOCOL_VERSION))
            {
                self.shutdown().await;
                return Err(RunFailure::Harness(plugin_error(
                    "version",
                    "external harness frame version mismatch",
                    None,
                )));
            }
            match frame.get("type").and_then(Value::as_str) {
                Some("event") => {
                    events += 1;
                    if events > HARNESS_PLUGIN_MAX_EVENTS {
                        self.shutdown().await;
                        return Err(RunFailure::Harness(plugin_error(
                            "event_limit",
                            "external harness event count exceeds the 20000 limit",
                            None,
                        )));
                    }
                    self.handle_event(&frame, request, &mut tool_names);
                }
                Some("result") => {
                    return match parse_result(&frame) {
                        Ok(output) => Ok(output),
                        Err(error) => {
                            // result 非法：adapter 状态不可信，杀进程并让下次调用重拉。
                            self.shutdown().await;
                            Err(RunFailure::Harness(error))
                        }
                    };
                }
                Some("error") => {
                    let error = parse_error_frame(&frame);
                    self.shutdown().await;
                    return Err(RunFailure::Harness(error));
                }
                _ => {
                    self.shutdown().await;
                    return Err(RunFailure::Harness(plugin_error(
                        "protocol",
                        "external harness frame has an unknown type",
                        None,
                    )));
                }
            }
        }
    }

    fn handle_event(
        &self,
        frame: &Value,
        request: &ProviderRequest<'_>,
        tool_names: &mut HashMap<String, String>,
    ) {
        let kind = frame
            .get("kind")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let text = || {
            frame
                .get("text")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string()
        };
        match kind {
            "delta" if self.streaming => request
                .ctx
                .emit(StreamingEvent::TokenDelta { text: text() }),
            "reasoning" if self.streaming => request
                .ctx
                .emit(StreamingEvent::ReasoningDelta { text: text() }),
            "tool_start" => {
                let call_id = frame
                    .get("call_id")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string();
                let tool_name = frame
                    .get("tool")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string();
                tool_names.insert(call_id.clone(), tool_name.clone());
                request.ctx.emit(StreamingEvent::ToolCallStart {
                    call_id,
                    tool_name,
                    args_summary: frame.get("args_summary").cloned().unwrap_or(Value::Null),
                });
            }
            "tool_end" => {
                let call_id = frame
                    .get("call_id")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string();
                let tool_name = tool_names.get(&call_id).cloned().unwrap_or_default();
                request.ctx.emit(StreamingEvent::ToolCallEnd {
                    call_id,
                    tool_name,
                    ok: frame.get("ok").and_then(Value::as_bool).unwrap_or(false),
                    result_summary: frame.get("result_summary").cloned(),
                });
            }
            "progress" => {
                let percent = frame
                    .get("percent")
                    .and_then(Value::as_u64)
                    .and_then(|percent| u8::try_from(percent).ok())
                    .unwrap_or(0)
                    .min(100);
                request.ctx.emit(StreamingEvent::Progress {
                    percent,
                    message: frame
                        .get("message")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string(),
                });
            }
            // DR-011 §2.1：未知 event kind 忽略 + warn（向前兼容）。
            other => {
                if matches!(other, "delta" | "reasoning") {
                    // capabilities.streaming=false：adapter 不应发 delta；忽略并 warn。
                    eprintln!(
                        "zgraph: warn: external harness emitted '{other}' with streaming=false; ignored"
                    );
                } else if !other.is_empty() {
                    eprintln!(
                        "zgraph: warn: unknown external harness event kind '{other}'; ignored"
                    );
                }
            }
        }
    }

    async fn write_frame(&mut self, frame: &Value, stage_reason: &str) -> Result<(), HarnessError> {
        let Some(stdin) = self.stdin.as_mut() else {
            return Err(plugin_error(
                "exit_nonzero",
                "external harness stdin is closed",
                None,
            ));
        };
        let mut bytes = serde_json::to_vec(frame).map_err(|error| {
            plugin_error(
                stage_reason,
                &format!("failed to encode external harness frame: {error}"),
                None,
            )
        })?;
        bytes.push(b'\n');
        if bytes.len() > HARNESS_PLUGIN_LINE_MAX_BYTES {
            return Err(plugin_error(
                "line_too_large",
                "frame sent to external harness exceeds the 1 MiB limit",
                None,
            ));
        }
        stdin.write_all(&bytes).await.map_err(|error| {
            plugin_error(
                "exit_nonzero",
                &format!("failed to write external harness frame: {error}"),
                None,
            )
        })?;
        stdin.flush().await.map_err(|error| {
            plugin_error(
                "exit_nonzero",
                &format!("failed to flush external harness frame: {error}"),
                None,
            )
        })
    }

    /// 读取下一帧（握手等短路径用；不接受取消/deadline）。
    async fn next_frame(&mut self) -> Result<Value, HarnessError> {
        loop {
            match self.frames.recv().await {
                Some(Ok(frame)) => return Ok(frame),
                Some(Err(ReadError::InvalidJson)) => continue,
                Some(Err(ReadError::LineTooLarge)) => {
                    return Err(plugin_error(
                        "line_too_large",
                        "external harness line exceeds the 1 MiB limit",
                        None,
                    ));
                }
                Some(Err(ReadError::OutputLimit)) => {
                    return Err(plugin_error(
                        "output_limit",
                        "external harness total output exceeds the 8 MiB limit",
                        None,
                    ));
                }
                Some(Err(ReadError::Eof)) | None => {
                    return Err(plugin_error(
                        "handshake",
                        "external harness closed stdout during handshake",
                        None,
                    ));
                }
            }
        }
    }

    /// EOF/退出：非 0 或无 result → `exit_nonzero`（携带退出码与 stderr 尾）。
    async fn exit_failure(&mut self) -> HarnessError {
        let mut extra = serde_json::Map::new();
        if let Ok(Some(status)) = self.child.try_wait() {
            extra.insert("exit_code".to_string(), Value::from(status.code()));
        }
        let stderr = stderr_tail(&self.stderr).await;
        if !stderr.is_empty() {
            extra.insert("stderr_tail".to_string(), Value::from(stderr));
        }
        let error = plugin_error(
            "exit_nonzero",
            "external harness exited without a result frame",
            Some(Value::Object(extra)),
        );
        self.shutdown().await;
        error
    }

    /// 取消/终结：关 stdin → `SIGTERM` → 宽限 2s → `SIGKILL` 进程组 → reap。
    async fn shutdown(&mut self) {
        if self.closed {
            return;
        }
        self.closed = true;
        // 1) `cancel` 控制帧（best-effort）+ 关闭 stdin。
        if self.stdin.is_some() {
            let cancel = json!({"v": HARNESS_PLUGIN_PROTOCOL_VERSION, "type": "cancel"});
            let _ = self.write_frame(&cancel, "protocol").await;
        }
        self.stdin = None;
        // 2) SIGTERM 进程组 → 宽限 → SIGKILL。
        if let Some(pgid) = self.pgid.take() {
            send_group_signal(pgid, GroupSignal::Term);
            tokio::time::sleep(Duration::from_millis(HARNESS_PLUGIN_CANCEL_GRACE_MS)).await;
        }
        let _ = self.child.start_kill();
        let _ = tokio::time::timeout(Duration::from_secs(1), self.child.wait()).await;
        if let Some(task) = self.reader_task.take() {
            task.abort();
        }
        if let Some(task) = self.stderr_task.take() {
            let _ = tokio::time::timeout(Duration::from_secs(1), task).await;
        }
    }
}

/// stdout reader：按行读取、大小上限校验、JSON 解析（cancel-safe 的 channel 模型）。
async fn read_frames(
    mut stdout: BufReader<ChildStdout>,
    tx: mpsc::Sender<Result<Value, ReadError>>,
) {
    let mut line: Vec<u8> = Vec::new();
    let mut total = 0usize;
    loop {
        line.clear();
        match stdout.read_until(b'\n', &mut line).await {
            Ok(0) => {
                let _ = tx.send(Err(ReadError::Eof)).await;
                return;
            }
            Ok(read) => {
                if read > HARNESS_PLUGIN_LINE_MAX_BYTES {
                    let _ = tx.send(Err(ReadError::LineTooLarge)).await;
                    return;
                }
                total = total.saturating_add(read);
                if total > HARNESS_PLUGIN_TOTAL_OUTPUT_MAX_BYTES {
                    let _ = tx.send(Err(ReadError::OutputLimit)).await;
                    return;
                }
                match serde_json::from_slice::<Value>(&line) {
                    Ok(frame) => {
                        if tx.send(Ok(frame)).await.is_err() {
                            return;
                        }
                    }
                    Err(_) => {
                        if tx.send(Err(ReadError::InvalidJson)).await.is_err() {
                            return;
                        }
                    }
                }
            }
            Err(_) => {
                let _ = tx.send(Err(ReadError::Eof)).await;
                return;
            }
        }
    }
}

/// stderr 排空并按 8 KiB 截断（不写 trace 全文/密钥）。
async fn drain_stderr(mut pipe: tokio::process::ChildStderr, buffer: Arc<Mutex<Vec<u8>>>) {
    use tokio::io::AsyncReadExt;
    let mut chunk = [0_u8; 1024];
    loop {
        match pipe.read(&mut chunk).await {
            Ok(0) | Err(_) => return,
            Ok(read) => {
                let mut guard = buffer.lock().await;
                if guard.len() < HARNESS_PLUGIN_STDERR_MAX_BYTES {
                    let room = HARNESS_PLUGIN_STDERR_MAX_BYTES - guard.len();
                    let take = room.min(read);
                    guard.extend_from_slice(&chunk[..take]);
                }
            }
        }
    }
}

async fn stderr_tail(buffer: &Arc<Mutex<Vec<u8>>>) -> String {
    let guard = buffer.lock().await;
    String::from_utf8_lossy(&guard).to_string()
}

/// `result` 帧 → 四字段 output（content/stop_reason/iterations/usage）。
fn parse_result(frame: &Value) -> Result<AgentOutput, HarnessError> {
    let Some(content) = frame.get("content").and_then(Value::as_str) else {
        return Err(plugin_error(
            "result_invalid",
            "external harness result frame has no string 'content'",
            None,
        ));
    };
    if content.len() > HARNESS_PLUGIN_FINAL_MAX_BYTES {
        return Err(plugin_error(
            "result_invalid",
            "external harness result content exceeds the 1 MiB limit",
            None,
        ));
    }
    let Some(stop_reason) = frame.get("stop_reason").and_then(Value::as_str) else {
        return Err(plugin_error(
            "result_invalid",
            "external harness result frame has no string 'stop_reason'",
            None,
        ));
    };
    let Some(iterations) = frame
        .get("iterations")
        .and_then(Value::as_u64)
        .and_then(|iterations| u32::try_from(iterations).ok())
    else {
        return Err(plugin_error(
            "result_invalid",
            "external harness result frame has no integer 'iterations'",
            None,
        ));
    };
    Ok(AgentOutput {
        content: content.to_string(),
        stop_reason: StopReason::from_literal(stop_reason),
        iterations,
        usage: parse_usage(frame.get("usage")),
    })
}

/// usage 映射：`input→prompt`、`output→completion`、`reasoning_output→reasoning`；
/// `cached_input_tokens`/`cache_write_input_tokens` 丢弃（DR-011 §3.1）。
fn parse_usage(value: Option<&Value>) -> Usage {
    let Some(usage) = value.and_then(Value::as_object) else {
        return Usage::zero();
    };
    let field = |names: &[&str]| -> Option<u32> {
        names
            .iter()
            .find_map(|name| usage.get(*name).and_then(Value::as_u64))
            .and_then(|value| u32::try_from(value).ok())
    };
    let prompt = field(&["prompt_tokens", "input_tokens"]).unwrap_or(0);
    let completion = field(&["completion_tokens", "output_tokens"]).unwrap_or(0);
    let total = field(&["total_tokens"]);
    let reasoning = field(&["reasoning_tokens", "reasoning_output_tokens"]);
    Usage::provider(prompt, completion, total, reasoning)
}

/// `error` 帧 → `HARNESS_PLUGIN_ERROR{reason}`（reason 闭集外的值收敛为 `protocol`）。
fn parse_error_frame(frame: &Value) -> HarnessError {
    const REASONS: [&str; 10] = [
        "spawn_failed",
        "handshake",
        "version",
        "protocol",
        "result_invalid",
        "line_too_large",
        "output_limit",
        "event_limit",
        "exit_nonzero",
        "timeout",
    ];
    let raw = frame
        .get("reason")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let reason = if REASONS.contains(&raw) {
        raw
    } else {
        "protocol"
    };
    let message = frame
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or("external harness reported an error");
    let mut extra = serde_json::Map::new();
    if let Some(code) = frame.get("code") {
        extra.insert("adapter_code".to_string(), code.clone());
    }
    plugin_error(reason, message, Some(Value::Object(extra)))
}

/// 构造 `HARNESS_PLUGIN_ERROR`（`detail.reason` 闭集；stage 固定 Invoke）。
fn plugin_error(reason: &str, message: &str, extra: Option<Value>) -> HarnessError {
    HarnessError::plugin(reason, Stage::Invoke, message.to_string(), extra)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::sync::atomic::AtomicU32;
    use std::sync::Arc;

    use serde_json::{json, Value};
    use tempfile::TempDir;
    use zgraph_dsl::InputPort;
    use zgraph_kernel::StreamingEvent;

    use super::*;
    use crate::config::{AgentConfig, LoadedAgent, ModelConfig};
    use crate::ctx::HarnessCtx;
    use crate::llm::{FakeLlmProvider, LlmProvider, ProviderKind, SecretString};
    use crate::test_support::TestCtx;
    use crate::tools::{FakeToolRunner, ToolRunner};

    /// 简易 fake adapter（`zgraph-harness/1`）：由 `mode` 参数选择行为。
    const FAKE_ADAPTER: &str = r#"
import json
import sys
import time

mode = sys.argv[1] if len(sys.argv) > 1 else "echo"

def send(payload):
    sys.stdout.write(json.dumps(payload) + "\n")
    sys.stdout.flush()

def read():
    line = sys.stdin.readline()
    if not line:
        return None
    return json.loads(line)

hello = read()
if mode == "version_bad":
    send({"v": 1, "type": "hello_ok", "protocol": "zgraph-harness/2",
          "provider": "fake", "capabilities": {"streaming": False}})
    sys.exit(0)

streaming = mode not in ("no_stream",)
send({"v": 1, "type": "hello_ok", "protocol": "zgraph-harness/1",
      "provider": "fake", "capabilities": {"streaming": streaming}})

def send_result():
    send({"v": 1, "type": "result", "content": "final-text",
          "stop_reason": "EndTurn", "iterations": 1,
          "usage": {"input_tokens": 11, "output_tokens": 7,
                    "reasoning_output_tokens": 3, "cached_input_tokens": 99}})

while True:
    frame = read()
    if frame is None:
        break
    if frame.get("type") == "cancel":
        break
    if frame.get("type") != "run":
        continue
    if mode == "echo":
        send({"v": 1, "type": "event", "kind": "delta", "text": "hel"})
        send({"v": 1, "type": "event", "kind": "delta", "text": "lo"})
        send({"v": 1, "type": "event", "kind": "reasoning", "text": "think"})
        send({"v": 1, "type": "event", "kind": "tool_start", "call_id": "c1",
              "tool": "echo", "args_summary": {"a": 1}})
        send({"v": 1, "type": "event", "kind": "tool_end", "call_id": "c1",
              "ok": True, "result_summary": {"b": 2}})
        send({"v": 1, "type": "event", "kind": "progress", "percent": 40, "message": "half"})
        send_result()
    elif mode == "no_stream":
        send({"v": 1, "type": "event", "kind": "delta", "text": "ignored"})
        send_result()
    elif mode == "unknown_event":
        send({"v": 1, "type": "event", "kind": "plan", "text": "future"})
        send_result()
    elif mode == "error":
        send({"v": 1, "type": "error", "code": "HARNESS_PLUGIN_ERROR",
              "message": "adapter boom", "reason": "protocol"})
    elif mode == "bad_result":
        send({"v": 1, "type": "result", "content": "x"})
        break
    elif mode == "bad_json":
        sys.stdout.write("not-json\n")
        sys.stdout.flush()
        send_result()
    elif mode == "bad_json_many":
        for _ in range(4):
            sys.stdout.write("not-json\n")
            sys.stdout.flush()
        break
    elif mode == "big_line":
        sys.stdout.write("x" * (2 * 1024 * 1024) + "\n")
        sys.stdout.flush()
        time.sleep(0.2)
        break
    elif mode == "many_events":
        for index in range(20001):
            send({"v": 1, "type": "event", "kind": "progress",
                  "percent": index % 100, "message": "x"})
        send_result()
    elif mode == "exit_nonzero":
        sys.exit(3)
    elif mode == "silent":
        time.sleep(60)
    else:
        send_result()
"#;

    /// 假 `codex` 可执行（PATH 注入）：输出脚本化 ThreadEvent + `-o` 最终消息。
    const FAKE_CODEX: &str = r#"#!/usr/bin/env python3
import json
import sys

args = sys.argv[1:]
output = None
if "--output-last-message" in args:
    output = args[args.index("--output-last-message") + 1]
for event in [
    {"type": "thread.started", "thread_id": "t1"},
    {"type": "item.completed", "item": {"id": "i1", "type": "agent_message", "text": "codex-final"}},
    {"type": "turn.completed", "usage": {"input_tokens": 10, "output_tokens": 5,
                                        "reasoning_output_tokens": 2, "cached_input_tokens": 99}},
]:
    print(json.dumps(event), flush=True)
if output:
    with open(output, "w", encoding="utf-8") as handle:
        handle.write("codex-final")
"#;

    /// 仓库内 Codex adapter 路径（跨模块只读引用；py-worker 已获 Lead 批准）。
    fn codex_adapter_path() -> std::path::PathBuf {
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../python/zgraph_worker/adapters/codex.py")
    }

    struct Fixture {
        _dir: TempDir,
        agent: AgentConfig,
        loaded: LoadedAgent,
        llm: Arc<dyn LlmProvider>,
        runner: Arc<dyn ToolRunner>,
        budget: Arc<AtomicU32>,
        port_inputs: std::collections::HashMap<String, Value>,
        mapped_inputs: BTreeMap<String, Value>,
        declared_ports: Vec<InputPort>,
        node_mapping: BTreeMap<String, String>,
    }

    fn python_program() -> String {
        std::env::var("ZGRAPH_PYTHON").unwrap_or_else(|_| "python3".to_string())
    }

    fn fixture(mode: &str) -> Fixture {
        let dir = TempDir::new().expect("tempdir");
        let script = dir.path().join("fake_adapter.py");
        std::fs::write(&script, FAKE_ADAPTER).expect("adapter script");
        std::fs::create_dir_all(dir.path().join("agents")).expect("agents dir");
        let (args, env) = if mode == "codex" {
            let bin_dir = dir.path().join("bin");
            std::fs::create_dir_all(&bin_dir).expect("fake codex dir");
            let codex = bin_dir.join("codex");
            std::fs::write(&codex, FAKE_CODEX).expect("fake codex");
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&codex, std::fs::Permissions::from_mode(0o755))
                    .expect("chmod fake codex");
            }
            let existing = std::env::var("PATH").unwrap_or_default();
            let path = format!("{}:{existing}", bin_dir.display());
            (
                vec![codex_adapter_path().display().to_string()],
                BTreeMap::from([("PATH".to_string(), path)]),
            )
        } else {
            (
                vec![script.display().to_string(), mode.to_string()],
                BTreeMap::new(),
            )
        };
        let harness = HarnessConfig {
            kind: crate::config::HarnessKind::Subprocess,
            command: Some(python_program()),
            args,
            env,
            timeout_ms: None,
            protocol: Some(HARNESS_PLUGIN_PROTOCOL.to_string()),
        };
        let agent = AgentConfig {
            id: "agent".to_string(),
            name: "agent".to_string(),
            schema_version: 1,
            model: ModelConfig {
                provider: ProviderKind::Deepseek,
                name: "deepseek-chat".to_string(),
                base_url: None,
                api_key_env: "UNUSED".to_string(),
                temperature: 0.0,
                max_tokens: 256,
                context_window_tokens: 65_536,
            },
            role_prompt_file: "prompt.md".to_string(),
            constraints_file: None,
            tools: Vec::new(),
            skills: Vec::new(),
            mcp: None,
            harness: Some(harness),
            input_mapping: BTreeMap::new(),
            max_iterations: 10,
            llm_timeout_ms: 5_000,
            llm_max_retries: 0,
            node_timeout_ms: 10_000,
        };
        let loaded = LoadedAgent {
            config: agent.clone(),
            role_prompt: String::new(),
            constraints: None,
            api_key: SecretString::new(""),
            tools: Vec::new(),
            skills: Vec::new(),
            mcp: None,
        };
        let llm: Arc<dyn LlmProvider> = Arc::new(
            FakeLlmProvider::from_script(json!({"version": 1, "chunk_chars": 64,
                "calls": [{"kind": "text", "content": "builtin"}]}))
            .expect("fake llm"),
        );
        let runner: Arc<dyn ToolRunner> = Arc::new(FakeToolRunner::new());
        let declared_ports = vec![InputPort {
            name: "message".to_string(),
            port_type: zgraph_dsl::PortType::Any,
            required: true,
            description: None,
        }];
        Fixture {
            _dir: dir,
            agent,
            loaded,
            llm,
            runner,
            budget: Arc::new(AtomicU32::new(0)),
            port_inputs: std::collections::HashMap::from([("message".to_string(), json!("hi"))]),
            mapped_inputs: BTreeMap::from([("user_message".to_string(), json!("hi"))]),
            declared_ports,
            node_mapping: BTreeMap::new(),
        }
    }

    impl Fixture {
        fn assets(&self) -> &Path {
            self._dir.path()
        }

        fn request<'a>(
            &'a self,
            ctx: Arc<dyn HarnessCtx>,
            node_timeout_ms: u64,
        ) -> ProviderRequest<'a> {
            ProviderRequest {
                agent: &self.agent,
                inputs: self.mapped_inputs.clone(),
                port_inputs: &self.port_inputs,
                ctx,
                trace: crate::provider::ProviderTrace {
                    run_id: "run-1".to_string(),
                    instance_id: "inst-1".to_string(),
                    node_id: "n1".to_string(),
                    attempt: 0,
                    item_index: 0,
                },
                cwd: self.assets(),
                node_timeout_ms,
                loaded_agent: &self.loaded,
                tools_dir: self.assets(),
                llm: &self.llm,
                tool_runner: &self.runner,
                max_iterations: 10,
                declared_ports: &self.declared_ports,
                node_mapping: &self.node_mapping,
                recorder: None,
                skill_view_budget: &self.budget,
                mcp: None,
            }
        }
    }

    fn provider_error(result: Result<AgentOutput, RunFailure>) -> HarnessError {
        match result.expect_err("expected provider failure") {
            RunFailure::Harness(error) => error,
            other => panic!("expected Harness failure, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn streams_events_and_maps_result_fields() {
        let fixture = fixture("echo");
        let ctx = TestCtx::new();
        let provider = SubprocessProvider::new();
        let output = AgentProvider::run(&provider, fixture.request(ctx.clone(), 5_000))
            .await
            .expect("run");
        assert_eq!(output.content, "final-text");
        assert_eq!(output.stop_reason, StopReason::EndTurn);
        assert_eq!(output.iterations, 1);
        // usage 映射：input→prompt、output→completion、reasoning_output→reasoning；
        // cached_input_tokens 丢弃（总 token 由 prompt+completion 推出）。
        assert_eq!(output.usage.prompt_tokens, 11);
        assert_eq!(output.usage.completion_tokens, 7);
        assert_eq!(output.usage.reasoning_tokens, Some(3));
        assert_eq!(output.usage.total_tokens, 18);
        assert_eq!(ctx.event_count(), 6, "{:?}", ctx.events());
        let events = ctx.events();
        assert!(matches!(&events[0], StreamingEvent::TokenDelta { text } if text == "hel"));
        assert!(matches!(&events[1], StreamingEvent::TokenDelta { text } if text == "lo"));
        assert!(matches!(&events[2], StreamingEvent::ReasoningDelta { text } if text == "think"));
        assert!(matches!(
            &events[3],
            StreamingEvent::ToolCallStart { call_id, tool_name, .. }
                if call_id == "c1" && tool_name == "echo"
        ));
        assert!(matches!(
            &events[4],
            StreamingEvent::ToolCallEnd { call_id, tool_name, ok, .. }
                if call_id == "c1" && tool_name == "echo" && *ok
        ));
        assert!(matches!(
            &events[5],
            StreamingEvent::Progress { percent, .. } if *percent == 40
        ));
        assert_eq!(ctx.token_text(), "hello");
        assert_eq!(provider.live_adapter_count().await, 1);
        // 幂等关闭：一次关闭 1 个进程，二次 0。
        assert_eq!(provider.close_provider().await, 1);
        assert_eq!(provider.close_provider().await, 0);
        assert_eq!(provider.live_adapter_count().await, 0);
    }

    #[tokio::test]
    async fn streaming_false_drops_deltas_and_unknown_events_are_ignored() {
        let no_stream = fixture("no_stream");
        let ctx = TestCtx::new();
        let provider = SubprocessProvider::new();
        let output = AgentProvider::run(&provider, no_stream.request(ctx.clone(), 5_000))
            .await
            .expect("run");
        assert_eq!(output.content, "final-text");
        assert_eq!(ctx.event_count(), 0, "streaming=false must drop deltas");

        let unknown = fixture("unknown_event");
        let ctx = TestCtx::new();
        let output = AgentProvider::run(&provider, unknown.request(ctx.clone(), 5_000))
            .await
            .expect("unknown event kind is ignored");
        assert_eq!(output.content, "final-text");
        assert_eq!(ctx.event_count(), 0);
        provider.close_provider().await;
    }

    #[tokio::test]
    async fn invalid_json_lines_are_tolerated_up_to_the_threshold() {
        let fixture = fixture("bad_json");
        let ctx = TestCtx::new();
        let provider = SubprocessProvider::new();
        let output = AgentProvider::run(&provider, fixture.request(ctx, 5_000))
            .await
            .expect("single invalid line is tolerated");
        assert_eq!(output.content, "final-text");
        provider.close_provider().await;
    }

    #[tokio::test]
    async fn fatal_protocol_failures_map_to_plugin_error_reasons() {
        let cases = [
            ("bad_json_many", "protocol"),
            ("big_line", "line_too_large"),
            ("many_events", "event_limit"),
            ("bad_result", "result_invalid"),
            ("error", "protocol"),
        ];
        for (mode, reason) in cases {
            let fixture = fixture(mode);
            let ctx = TestCtx::new();
            let provider = SubprocessProvider::new();
            let error =
                provider_error(AgentProvider::run(&provider, fixture.request(ctx, 10_000)).await);
            assert_eq!(
                error.kind,
                zgraph_kernel::HarnessErrorKind::Plugin,
                "{mode}: {error:?}"
            );
            assert_eq!(error.code, "HARNESS_PLUGIN_ERROR", "{mode}");
            assert_eq!(error.detail["reason"], reason, "{mode}: {error:?}");
            assert_eq!(
                provider.live_adapter_count().await,
                0,
                "{mode}: adapter killed"
            );
        }
    }

    #[tokio::test]
    async fn handshake_version_mismatch_fails_closed() {
        let fixture = fixture("version_bad");
        let ctx = TestCtx::new();
        let provider = SubprocessProvider::new();
        let error =
            provider_error(AgentProvider::run(&provider, fixture.request(ctx, 5_000)).await);
        assert_eq!(error.detail["reason"], "version", "{error:?}");
        assert_eq!(provider.live_adapter_count().await, 0);
    }

    #[tokio::test]
    async fn nonzero_exit_without_result_is_exit_nonzero() {
        let fixture = fixture("exit_nonzero");
        let ctx = TestCtx::new();
        let provider = SubprocessProvider::new();
        let error =
            provider_error(AgentProvider::run(&provider, fixture.request(ctx, 5_000)).await);
        assert_eq!(error.detail["reason"], "exit_nonzero", "{error:?}");
        assert_eq!(error.detail["exit_code"], 3, "{error:?}");
        assert_eq!(provider.live_adapter_count().await, 0);
    }

    #[tokio::test]
    async fn silent_adapter_hits_instance_timeout() {
        let fixture = fixture("silent");
        let ctx = TestCtx::new();
        let provider = SubprocessProvider::new();
        let result = AgentProvider::run(&provider, fixture.request(ctx, 150)).await;
        match result.expect_err("timeout expected") {
            RunFailure::InstanceTimeout { elapsed_ms } => {
                assert!(elapsed_ms >= 150, "elapsed_ms = {elapsed_ms}");
            }
            other => panic!("expected InstanceTimeout, got {other:?}"),
        }
        assert_eq!(provider.live_adapter_count().await, 0, "no orphan adapter");
    }

    #[tokio::test]
    async fn cancellation_propagates_and_kills_adapter() {
        let fixture = fixture("silent");
        let ctx = TestCtx::new();
        ctx.request_cancel();
        let provider = SubprocessProvider::new();
        let result = AgentProvider::run(&provider, fixture.request(ctx, 5_000)).await;
        assert!(matches!(result, Err(RunFailure::Cancelled)), "{result:?}");
        assert_eq!(provider.live_adapter_count().await, 0, "no orphan adapter");
    }

    #[tokio::test]
    async fn adapter_is_persistent_per_run_and_provider() {
        let fixture = fixture("echo");
        let provider = SubprocessProvider::new();
        for _ in 0..2 {
            let ctx = TestCtx::new();
            AgentProvider::run(&provider, fixture.request(ctx, 5_000))
                .await
                .expect("run");
            assert_eq!(provider.live_adapter_count().await, 1, "adapter reused");
        }
        assert_eq!(provider.close_provider().await, 1);
    }

    #[tokio::test]
    async fn close_provider_is_idempotent_without_any_process() {
        let provider = SubprocessProvider::new();
        assert_eq!(provider.close_provider().await, 0);
        assert_eq!(provider.close_provider().await, 0);
    }
    /// v0.3/V4：Codex adapter（假 `codex` PATH 注入）→ 协议帧 → 四字段 output。
    ///
    /// AC-20 h：`streaming=false` 只发 result（无 event delta）；`iterations=1`；
    /// usage 映射且 cached tokens 丢弃；`-o/--output-last-message` 优先。
    #[tokio::test]
    async fn codex_adapter_maps_thread_events_through_the_protocol() {
        let codex = fixture("codex");
        let ctx = TestCtx::new();
        let provider = SubprocessProvider::new();
        let output = AgentProvider::run(&provider, codex.request(ctx.clone(), 10_000))
            .await
            .expect("codex adapter run");
        assert_eq!(output.content, "codex-final");
        assert_eq!(output.stop_reason, StopReason::EndTurn);
        assert_eq!(output.iterations, 1);
        assert_eq!(output.usage.prompt_tokens, 10);
        assert_eq!(output.usage.completion_tokens, 5);
        assert_eq!(output.usage.total_tokens, 15);
        assert_eq!(output.usage.reasoning_tokens, Some(2));
        assert_eq!(
            ctx.event_count(),
            0,
            "Codex streaming=false must not emit deltas"
        );
        assert_eq!(provider.close_provider().await, 1);

        // 无 codex 二进制（PATH 不含 fake）→ 结构化 spawn_failed。
        let no_binary = fixture("codex");
        // 覆盖 env：一个不存在的 PATH 目录。
        let unreachable =
            std::env::temp_dir().join(format!("zgraph-no-codex-{}", std::process::id()));
        std::fs::create_dir_all(&unreachable).expect("empty bin dir");
        let mut agent = no_binary.agent.clone();
        if let Some(harness) = agent.harness.as_mut() {
            harness.env = BTreeMap::from([("PATH".to_string(), unreachable.display().to_string())]);
        }
        let ctx = TestCtx::new();
        let provider = SubprocessProvider::new();
        let fixture2 = Fixture {
            agent,
            loaded: LoadedAgent {
                config: no_binary.agent.clone(),
                ..no_binary.loaded.clone()
            },
            ..no_binary
        };
        let error =
            provider_error(AgentProvider::run(&provider, fixture2.request(ctx, 10_000)).await);
        assert_eq!(error.detail["reason"], "spawn_failed", "{error:?}");
        assert_eq!(provider.live_adapter_count().await, 0);
    }
}
