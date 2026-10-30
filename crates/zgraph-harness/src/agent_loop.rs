//! Harness 六状态执行循环（05 §5.5）。
//!
//! `Loading → Assembling → Invoking ⇄ ToolExec → Returning | Failed`：
//! - `iteration` 在**发起请求之前** +1；重试不 +1（05 §5.5.2）；
//! - 单响应多 tool_call **串行**、按数组顺序，每个 emit `ToolCallStart`→执行→`ToolCallEnd`；
//! - 单个工具失败写错误 JSON 继续（不终止）；未知工具 → `HARNESS_UNKNOWN_TOOL`；
//! - 空响应**连续 2 次** → `HARNESS_LLM_ERROR{reason:"empty_response"}`（采信 05 §5.5.3
//!   表格；§5.5.4 伪代码的“单次即错”作废，见 REV-020 讨论）。
//!
//! v2 偏差：取消 = 轮询（[`CancelToken`]），deadline 预检先于 provider 错误分类
//! （05 §5.7.3）；重试编排在 Harness 内部（不在 adapter）。

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use serde::ser::{Serialize, Serializer};
use serde_json::{json, Value};
use zgraph_dsl::InputPort;
use zgraph_kernel::{NodeError, PortName, StreamingEvent};

use crate::assemble::{
    assemble_initial_messages, assemble_system, assemble_user_json, effective_input_mapping,
};
use crate::budget::{check_and_trim, prompt_budget};
use crate::config::LoadedAgent;
use crate::constants::{
    BACKOFF_BASE_MS, BACKOFF_MAX_MS, DEFAULT_TOOL_TIMEOUT_MS, MAX_TOOL_CALLS_PER_RESPONSE,
    RETRY_AFTER_MAX_MS,
};
use crate::ctx::{wait_cancelled, CancelToken, HarnessCtx, StreamSink};
use crate::error::HarnessError;
use crate::llm::{
    LlmError, LlmErrorKind, LlmMessage, LlmPhase, LlmProvider, LlmRequest, LlmResponse, ModelRef,
    StopReason, ToolCall, Usage,
};
use crate::tools::{
    serialize_tool_result, summarize_args, summarize_result, tool_error_content,
    validate_tool_arguments, ToolCallRequest, ToolDef, ToolFailure, ToolRunner, ToolSchema,
};

/// `agent_call` 节点的输出契约（05 §5.9.1 / §5.9.2）。
#[derive(Debug, Clone, PartialEq)]
pub struct AgentOutput {
    /// 最终 assistant 文本；无内容时 `""`（不会为 null）。
    pub content: String,
    /// 停止原因。
    pub stop_reason: StopReason,
    /// LLM 请求轮次（§5.5.2）。
    pub iterations: u32,
    /// 累计 usage（只累计成功响应）。
    pub usage: Usage,
}

impl AgentOutput {
    /// 05 §5.9.2 的节点 output JSON 形状。
    pub fn to_json(&self) -> Value {
        json!({
            "content": self.content,
            "stop_reason": self.stop_reason.as_literal(),
            "iterations": self.iterations,
            "usage": self.usage.to_json(),
        })
    }
}

impl Serialize for AgentOutput {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.to_json().serialize(serializer)
    }
}

/// 循环失败：取消 / 实例超时**不是** Harness 错误（05 §5.7.3）。
#[derive(Debug, Clone, PartialEq)]
pub enum RunFailure {
    /// 协作取消 → `NodeError::Cancelled`。
    Cancelled,
    /// Harness deadline 自检失败 → `NodeError::Timeout { elapsed_ms }`。
    InstanceTimeout {
        /// 已耗时（毫秒）。
        elapsed_ms: u64,
    },
    /// Harness 错误 → `NodeError::Harness { kind, detail }`。
    Harness(HarnessError),
    /// 框架不变量违反（消息序列非法等）→ `NodeError::Internal`。
    Internal {
        /// 人类可读描述。
        message: String,
    },
}

impl RunFailure {
    /// 出口映射（05 §5.7.2 补充映射）。
    pub fn into_node_error(self) -> NodeError {
        match self {
            Self::Cancelled => NodeError::Cancelled,
            Self::InstanceTimeout { elapsed_ms } => NodeError::Timeout { elapsed_ms },
            Self::Harness(error) => error.into_node_error(),
            Self::Internal { message } => NodeError::Internal { message },
        }
    }
}

impl From<HarnessError> for RunFailure {
    fn from(error: HarnessError) -> Self {
        Self::Harness(error)
    }
}

/// 一次 Agent 执行的全部输入（不可变；由 executor 组装）。
pub(crate) struct AgentRun<'a> {
    /// 已校验的 Agent（Loading 阶段产物；`tools` 即本 Agent 的**可见工具集**）。
    pub(crate) agent: &'a LoadedAgent,
    /// 工具目录（D3：worker 的 `tools_dir`）。
    pub(crate) tools_dir: &'a Path,
    /// LLM transport。
    pub(crate) provider: &'a Arc<dyn LlmProvider>,
    /// 工具执行后端。
    pub(crate) tool_runner: &'a Arc<dyn ToolRunner>,
    /// 节点全部声明端口的输入（未到为 `Value::Null`）。
    pub(crate) inputs: &'a std::collections::HashMap<PortName, Value>,
    /// 节点输入端口声明（顺序 + 名字）。
    pub(crate) declared_ports: &'a [InputPort],
    /// 节点级 `input_mapping`（优先于 AgentConfig）。
    pub(crate) node_mapping: &'a BTreeMap<String, String>,
    /// 生效的 iteration 上限（节点级覆盖 AgentConfig）。
    pub(crate) max_iterations: u32,
    /// 实例超时（毫秒；仅用于 `Timeout.elapsed_ms` 计算）。
    pub(crate) node_timeout_ms: u64,
    /// Harness 上下文 seam。
    pub(crate) ctx: Arc<dyn HarnessCtx>,
    /// 取消句柄。
    pub(crate) cancel: CancelToken,
}

/// 执行一次 Agent（05 §5.5.4 语义；Loading 由 executor 预先完成）。
pub(crate) async fn run_agent(run: AgentRun<'_>) -> Result<AgentOutput, RunFailure> {
    let config = &run.agent.config;
    let provider_name = run.provider.name();
    let model = run.agent.model_ref();
    // Assembling
    let system = assemble_system(&run.agent.role_prompt, run.agent.constraints.as_deref());
    let mapping =
        effective_input_mapping(run.declared_ports, run.node_mapping, &config.input_mapping);
    let user_json = assemble_user_json(run.declared_ports, run.inputs, &mapping);
    let mut messages = assemble_initial_messages(&system.text, &user_json);
    // 05 §5.2.2：LLM 只能调用 `AgentConfig.tools` 列出的工具（可见工具集）。
    // 未注册**与**已注册但未授权给本 Agent 的工具一律 `HARNESS_UNKNOWN_TOOL`。
    let visible_tools: &[ToolDef] = &run.agent.tools;
    let visible_tool_names: Vec<String> =
        visible_tools.iter().map(|tool| tool.name.clone()).collect();
    let tool_schemas = run.agent.tool_schemas();
    let budget = prompt_budget(config.model.context_window_tokens, config.model.max_tokens);
    check_and_trim(&messages, &tool_schemas, budget, system.constraints_tokens)?;

    let mut iteration: u32 = 0;
    let mut usage_total = Usage::zero();
    let mut last_content: Option<String> = None;
    let mut last_pending: Vec<String> = Vec::new();
    let mut empty_streak: u32 = 0;

    loop {
        if iteration >= run.max_iterations {
            return Err(RunFailure::Harness(HarnessError::max_iterations(
                iteration,
                run.max_iterations,
                last_content.as_deref(),
                &last_pending,
                &usage_total,
            )));
        }
        // 取消/实例 deadline 预检优先于 provider 错误（05 §5.7.3）。
        if run.cancel.is_cancelled() {
            return Err(RunFailure::Cancelled);
        }
        let remaining_before = run.ctx.node_deadline_remaining_ms();
        if remaining_before == 0 {
            return Err(RunFailure::InstanceTimeout {
                elapsed_ms: run.node_timeout_ms.saturating_sub(remaining_before),
            });
        }
        iteration = iteration.saturating_add(1);
        run.ctx.emit(StreamingEvent::Progress {
            percent: progress_percent(iteration, run.max_iterations),
            message: format!("llm call {iteration}/{}", run.max_iterations),
        });
        // 消息序列合法性 fail-fast（05 §5.4.1：违反 = 框架 bug → Internal）。
        validate_message_sequence(&messages)?;
        // 每次调用前重新检查预算（工具结果只增不减，05 §5.4.6）。
        check_and_trim(&messages, &tool_schemas, budget, system.constraints_tokens)?;

        let response = invoke_with_retry(
            &run,
            &messages,
            &tool_schemas,
            iteration,
            provider_name,
            &model,
        )
        .await?;
        usage_total.accumulate(&response.usage);
        let LlmResponse {
            content,
            tool_calls,
            stop_reason,
            ..
        } = response;
        last_content = content.clone();
        last_pending = tool_calls.iter().map(|call| call.name.clone()).collect();

        if tool_calls.is_empty() {
            let final_content = content.unwrap_or_default();
            if final_content.is_empty() {
                // 05 §5.5.3：空响应追加 assistant 空消息继续；连续 2 次才失败。
                empty_streak = empty_streak.saturating_add(1);
                messages.push(LlmMessage::Assistant {
                    content: None,
                    tool_calls: Vec::new(),
                });
                if empty_streak >= 2 {
                    return Err(RunFailure::Harness(HarnessError::llm_reason(
                        iteration,
                        provider_name,
                        &model.name,
                        "empty_response",
                    )));
                }
                continue;
            }
            return Ok(AgentOutput {
                content: final_content,
                stop_reason,
                iterations: iteration,
                usage: usage_total,
            });
        }

        empty_streak = 0;
        if tool_calls.len() > MAX_TOOL_CALLS_PER_RESPONSE {
            return Err(RunFailure::Harness(HarnessError::llm_reason(
                iteration,
                provider_name,
                &model.name,
                "too_many_tool_calls",
            )));
        }
        messages.push(LlmMessage::Assistant {
            content: content.clone(),
            tool_calls: tool_calls.clone(),
        });
        for call in &tool_calls {
            // 未知/未授权工具立即失败（不是 TOOL_ERROR，05 §5.2.2/§5.6.6）。
            let Some(tool) = visible_tools.iter().find(|tool| tool.name == call.name) else {
                return Err(RunFailure::Harness(HarnessError::unknown_tool(
                    &call.name,
                    iteration,
                    &visible_tool_names,
                )));
            };
            run.ctx.emit(StreamingEvent::ToolCallStart {
                call_id: call.call_id.clone(),
                tool_name: call.name.clone(),
                args_summary: summarize_args(&call.args),
            });
            let (ok, tool_content) = execute_tool(&run, tool, call).await?;
            run.ctx.emit(StreamingEvent::ToolCallEnd {
                call_id: call.call_id.clone(),
                tool_name: call.name.clone(),
                ok,
                result_summary: Some(summarize_result(&tool_content, ok)),
            });
            messages.push(LlmMessage::Tool {
                tool_call_id: call.call_id.clone(),
                name: call.name.clone(),
                content: tool_content,
            });
        }
        // ToolExec → Invoking（下一轮 iteration）。
    }
}

/// `Progress.percent = min(99, floor(iteration × 100 / max_iterations))`（05 §5.8.2）。
fn progress_percent(iteration: u32, max_iterations: u32) -> u8 {
    let max = u64::from(max_iterations.max(1));
    let percent = u64::from(iteration).saturating_mul(100) / max;
    u8::try_from(percent.min(99)).unwrap_or(99)
}

/// 单次 LLM 调用 + Establish 阶段重试编排（05 §5.3.5；重试不消耗 iteration）。
async fn invoke_with_retry(
    run: &AgentRun<'_>,
    messages: &[LlmMessage],
    tool_schemas: &[ToolSchema],
    iteration: u32,
    provider_name: &str,
    model: &ModelRef,
) -> Result<LlmResponse, RunFailure> {
    let config = &run.agent.config;
    let max_attempts = config.llm_max_retries.saturating_add(1);
    let mut attempt: u32 = 0;
    loop {
        if run.cancel.is_cancelled() {
            return Err(RunFailure::Cancelled);
        }
        let remaining = run.ctx.node_deadline_remaining_ms();
        if remaining == 0 {
            return Err(RunFailure::InstanceTimeout {
                elapsed_ms: run.node_timeout_ms.saturating_sub(remaining),
            });
        }
        let timeout_ms = config.llm_timeout_ms.min(remaining);
        if timeout_ms < 1_000 {
            // 05 §5.3.6：剩余 < 1000ms 不再发起新调用，直接按实例超时失败。
            return Err(RunFailure::InstanceTimeout {
                elapsed_ms: run.node_timeout_ms.saturating_sub(remaining),
            });
        }
        attempt = attempt.saturating_add(1);
        let request = LlmRequest {
            model: model.clone(),
            messages: messages.to_vec(),
            tools: tool_schemas.to_vec(),
            temperature: config.model.temperature,
            max_tokens: config.model.max_tokens,
            stream: true,
            api_key: run.agent.api_key.clone(),
            timeout_ms,
        };
        let mut sink = StreamSink::new(run.ctx.clone());
        let outcome = tokio::time::timeout(
            Duration::from_millis(timeout_ms),
            run.provider.chat_stream(request, &mut sink, &run.cancel),
        )
        .await;
        let error = match outcome {
            Ok(Ok(mut response)) => {
                response.attempts = attempt;
                return Ok(response);
            }
            Ok(Err(error)) => error,
            Err(_elapsed) => {
                // Harness 侧兜底超时：phase 由已转发增量数判定（05 §5.3.2）。
                let streamed = sink.streamed();
                LlmError::new(
                    LlmErrorKind::Timeout,
                    if sink.started() {
                        LlmPhase::Streaming
                    } else {
                        LlmPhase::Establish
                    },
                    format!("LLM call timed out after {timeout_ms} ms"),
                    streamed,
                )
            }
        };
        // 取消与实例 deadline 优先于 provider 错误分类（05 §5.7.3）。
        if run.cancel.is_cancelled() || error.is_cancelled() {
            return Err(RunFailure::Cancelled);
        }
        if run.ctx.node_deadline_remaining_ms() == 0 {
            return Err(RunFailure::InstanceTimeout {
                elapsed_ms: run.node_timeout_ms,
            });
        }
        if error.is_retryable() && attempt < max_attempts {
            let wait_ms = retry_wait_ms(&error, attempt);
            if run.ctx.node_deadline_remaining_ms() > wait_ms {
                if sleep_or_cancel(run, wait_ms).await {
                    return Err(RunFailure::Cancelled);
                }
                continue;
            }
        }
        return Err(RunFailure::Harness(map_invoke_error(
            &error,
            iteration,
            provider_name,
            &model.name,
            attempt,
            timeout_ms,
        )));
    }
}

/// 第 `attempt` 次失败后的等待（05 §5.3.5）：`retry_after_ms` 优先（上限 30s），
/// 否则 `min(1000 × 2^(attempt-1), 8000)`。
fn retry_wait_ms(error: &LlmError, attempt: u32) -> u64 {
    if let Some(retry_after_ms) = error.retry_after_ms {
        return retry_after_ms.min(RETRY_AFTER_MAX_MS);
    }
    let exponent = attempt.saturating_sub(1).min(31);
    BACKOFF_BASE_MS
        .saturating_mul(1u64 << exponent.min(10))
        .min(BACKOFF_MAX_MS)
}

/// 退避等待；返回 `true` 表示期间被取消。
async fn sleep_or_cancel(run: &AgentRun<'_>, wait_ms: u64) -> bool {
    tokio::select! {
        biased;
        _ = wait_cancelled(&run.ctx) => true,
        _ = tokio::time::sleep(Duration::from_millis(wait_ms)) => false,
    }
}

/// LLM 错误 → HarnessError（05 §5.7.2）。
fn map_invoke_error(
    error: &LlmError,
    iteration: u32,
    provider: &str,
    model: &str,
    attempts: u32,
    timeout_ms: u64,
) -> HarnessError {
    if error.kind == LlmErrorKind::Timeout {
        HarnessError::llm_timeout(iteration, provider, model, error, attempts, timeout_ms)
    } else {
        HarnessError::llm_error(iteration, provider, model, error, attempts)
    }
}

/// 执行单个工具调用（参数校验 → 超时/取消 → 结果序列化；05 §5.6.4/§5.6.5）。
///
/// 返回 `(ok, tool_message_content)`：任何工具级失败都转为错误 JSON 喂回 LLM，
/// 只有取消向上传播为 [`RunFailure::Cancelled`]。
async fn execute_tool(
    run: &AgentRun<'_>,
    tool: &ToolDef,
    call: &ToolCall,
) -> Result<(bool, String), RunFailure> {
    if let Err(failure) = validate_tool_arguments(tool, &call.args) {
        return Ok((false, tool_error_content(&failure)));
    }
    let remaining = run.ctx.node_deadline_remaining_ms();
    if remaining == 0 {
        return Err(RunFailure::InstanceTimeout {
            elapsed_ms: run.node_timeout_ms,
        });
    }
    // 05 §5.6.5：min(ToolDef.timeout_ms, DEFAULT_TOOL_TIMEOUT_MS, 剩余 deadline)。
    let timeout_ms = tool
        .timeout_ms
        .min(DEFAULT_TOOL_TIMEOUT_MS)
        .min(remaining)
        .max(1);
    let request = ToolCallRequest {
        call_id: call.call_id.clone(),
        tool_name: call.name.clone(),
        module: tool.source.clone(),
        tools_dir: run.tools_dir.to_path_buf(),
        args: call.args.clone(),
        timeout_ms,
    };
    let call_future = run.tool_runner.call(request, run.ctx.clone());
    let mut cancel_check = Box::pin(wait_cancelled(&run.ctx));
    let mut timed = Box::pin(tokio::time::timeout(
        Duration::from_millis(timeout_ms),
        call_future,
    ));
    let outcome = tokio::select! {
        biased;
        _ = &mut cancel_check => return Err(RunFailure::Cancelled),
        result = &mut timed => result,
    };
    match outcome {
        // stop-waiting：超时后不再等待（D3：worker 进程组由 ToolRunner 回收）。
        Err(_elapsed) => Ok((
            false,
            tool_error_content(&ToolFailure {
                reason: "timeout".to_string(),
                message: format!("tool '{}' timed out after {timeout_ms} ms", tool.name),
                error_type: None,
                traceback: None,
                timeout_ms: Some(timeout_ms),
            }),
        )),
        Ok(Err(failure)) => {
            if failure.reason == "cancelled" {
                return Err(RunFailure::Cancelled);
            }
            Ok((false, tool_error_content(&failure)))
        }
        Ok(Ok(value)) => match serialize_tool_result(&value) {
            Ok(content) => Ok((true, content)),
            Err(failure) => Ok((false, tool_error_content(&failure))),
        },
    }
}

/// 消息序列合法性断言（05 §5.4.1；违反 = 框架 bug → `Internal`）。
///
/// v1 的组装/回填路径不会产生非法序列；本函数用于 fail-fast 定位。
pub(crate) fn validate_message_sequence(messages: &[LlmMessage]) -> Result<(), RunFailure> {
    let mut system_seen = false;
    let mut user_count = 0usize;
    let mut known_calls: Vec<String> = Vec::new();
    for (index, message) in messages.iter().enumerate() {
        match message {
            LlmMessage::System { content } => {
                if index != 0 || system_seen {
                    return Err(RunFailure::Internal {
                        message: "system message must appear at most once and first".to_string(),
                    });
                }
                if content.len() > 256 * 1024 {
                    return Err(RunFailure::Internal {
                        message: "system message content exceeds 256 KiB".to_string(),
                    });
                }
                system_seen = true;
            }
            LlmMessage::User { content } => {
                if content.len() > 256 * 1024 {
                    return Err(RunFailure::Internal {
                        message: "user message content exceeds 256 KiB".to_string(),
                    });
                }
                user_count += 1;
            }
            LlmMessage::Assistant { tool_calls, .. } => {
                for call in tool_calls {
                    known_calls.push(call.call_id.clone());
                }
            }
            LlmMessage::Tool { tool_call_id, .. } => {
                if !known_calls.contains(tool_call_id) {
                    return Err(RunFailure::Internal {
                        message: format!("orphan tool message for call '{tool_call_id}'"),
                    });
                }
            }
        }
    }
    if user_count == 0 {
        return Err(RunFailure::Internal {
            message: "message sequence must contain at least one user message".to_string(),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    use serde_json::json;
    use zgraph_dsl::{InputPort, PortType};

    use crate::config::{AgentConfig, ModelConfig};
    use crate::llm::{FakeLlmProvider, ProviderKind, SecretString};
    use crate::test_support::TestCtx;
    use crate::tools::{FakeToolBehavior, FakeToolRunner, ToolRegistry};

    fn tool_def(name: &str, parameters: Value, timeout_ms: u64) -> ToolDef {
        ToolDef {
            name: name.to_string(),
            description: "test tool".to_string(),
            parameters,
            timeout_ms,
            source: "tools.builtin".to_string(),
        }
    }

    fn no_args_tool(name: &str) -> ToolDef {
        tool_def(
            name,
            json!({"type": "object", "properties": {}}),
            DEFAULT_TOOL_TIMEOUT_MS,
        )
    }

    fn registry(tools: Vec<ToolDef>) -> ToolRegistry {
        ToolRegistry::new(tools).expect("registry")
    }

    fn loaded_agent(
        tools: Vec<ToolDef>,
        max_iterations: u32,
        llm_max_retries: u32,
        role_prompt: &str,
    ) -> LoadedAgent {
        let tool_names: Vec<String> = tools.iter().map(|tool| tool.name.clone()).collect();
        LoadedAgent {
            config: AgentConfig {
                id: "agent".to_string(),
                name: "agent".to_string(),
                schema_version: 1,
                model: ModelConfig {
                    provider: ProviderKind::Deepseek,
                    name: "deepseek-chat".to_string(),
                    base_url: None,
                    api_key_env: "HARNESS_TEST_KEY".to_string(),
                    temperature: 0.0,
                    max_tokens: 256,
                    context_window_tokens: 65_536,
                },
                role_prompt_file: "prompt.md".to_string(),
                constraints_file: None,
                tools: tool_names,
                input_mapping: BTreeMap::new(),
                max_iterations,
                llm_timeout_ms: 5_000,
                llm_max_retries,
                node_timeout_ms: 10_000,
            },
            role_prompt: role_prompt.to_string(),
            constraints: None,
            api_key: SecretString::new("test-key"),
            tools,
        }
    }

    fn declared_ports() -> Vec<InputPort> {
        vec![InputPort {
            name: "message".to_string(),
            port_type: PortType::Any,
            required: true,
            description: None,
        }]
    }

    async fn execute(
        agent: &LoadedAgent,
        _registry: &ToolRegistry,
        fake: &Arc<FakeLlmProvider>,
        runner: &Arc<FakeToolRunner>,
        ctx: &Arc<TestCtx>,
    ) -> Result<AgentOutput, RunFailure> {
        let provider: Arc<dyn LlmProvider> = fake.clone();
        let tool_runner: Arc<dyn ToolRunner> = runner.clone();
        let inputs = HashMap::from([("message".to_string(), json!("hi"))]);
        let ports = declared_ports();
        let node_mapping = BTreeMap::new();
        let cancel = CancelToken::new(ctx.clone());
        run_agent(AgentRun {
            agent,
            tools_dir: Path::new("/tmp/tools"),
            provider: &provider,
            tool_runner: &tool_runner,
            inputs: &inputs,
            declared_ports: &ports,
            node_mapping: &node_mapping,
            max_iterations: agent.config.max_iterations,
            node_timeout_ms: agent.config.node_timeout_ms,
            ctx: ctx.clone(),
            cancel,
        })
        .await
    }

    fn script(value: Value) -> Arc<FakeLlmProvider> {
        Arc::new(FakeLlmProvider::from_script(value).expect("valid script"))
    }

    fn text(content: &str) -> Value {
        json!({"kind": "text", "content": content})
    }

    fn tool_calls(calls: Value) -> Value {
        json!({"kind": "tool_calls", "content": null, "tool_calls": calls})
    }

    fn progress_events(ctx: &TestCtx) -> Vec<(u8, String)> {
        ctx.events()
            .into_iter()
            .filter_map(|event| match event {
                StreamingEvent::Progress { percent, message } => Some((percent, message)),
                _ => None,
            })
            .collect()
    }

    fn tool_events(ctx: &TestCtx) -> Vec<StreamingEvent> {
        ctx.events()
            .into_iter()
            .filter(|event| {
                matches!(
                    event,
                    StreamingEvent::ToolCallStart { .. } | StreamingEvent::ToolCallEnd { .. }
                )
            })
            .collect()
    }

    #[tokio::test]
    async fn single_round_text_returns_output_and_streams_tokens() {
        let fake = script(json!({"chunk_chars": 2, "calls": [text("hello")]}));
        let runner = Arc::new(FakeToolRunner::new());
        let agent = loaded_agent(vec![], 5, 1, "you are a parser");
        let ctx = TestCtx::new();
        let output = execute(&agent, &ToolRegistry::default(), &fake, &runner, &ctx)
            .await
            .expect("single round");
        assert_eq!(output.content, "hello");
        assert_eq!(output.iterations, 1);
        assert_eq!(output.stop_reason, StopReason::EndTurn);
        assert_eq!(
            ctx.token_text(),
            "hello",
            "A2: token deltas concat == content"
        );
        let progress = progress_events(&ctx);
        assert_eq!(progress, vec![(20, "llm call 1/5".to_string())]);
        let requests = fake.requests();
        assert_eq!(requests.len(), 1);
        assert!(matches!(requests[0].messages[0], LlmMessage::System { .. }));
        match &requests[0].messages[1] {
            LlmMessage::User { content } => {
                assert_eq!(content, "{\n  \"message\": \"hi\"\n}");
            }
            other => panic!("expected user message, got {other:?}"),
        }
        assert!(requests[0].tools.is_empty(), "no tools -> omit tools");
    }

    #[tokio::test]
    async fn single_tool_call_appends_messages_and_loops() {
        let fake = script(json!({
            "chunk_chars": 64,
            "calls": [
                tool_calls(json!([{"call_id": "c1", "name": "now", "args": {}}])),
                text("done")
            ]
        }));
        let runner = Arc::new(
            FakeToolRunner::new().behavior("now", FakeToolBehavior::Return(json!({"now": "t"}))),
        );
        let registry = registry(vec![no_args_tool("now")]);
        let agent = loaded_agent(vec![no_args_tool("now")], 5, 1, "role");
        let ctx = TestCtx::new();
        let output = execute(&agent, &registry, &fake, &runner, &ctx)
            .await
            .expect("tool loop");
        assert_eq!(output.content, "done");
        assert_eq!(output.iterations, 2);
        let calls = runner.calls();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].tool_name, "now");
        assert_eq!(calls[0].module, "tools.builtin");
        assert_eq!(calls[0].tools_dir, Path::new("/tmp/tools"));
        // A1/A3：ToolCallStart 在 ToolCallEnd 之前，call_id 一致。
        let events = tool_events(&ctx);
        assert_eq!(events.len(), 2);
        assert!(matches!(
            &events[0],
            StreamingEvent::ToolCallStart { call_id, tool_name, .. }
                if call_id == "c1" && tool_name == "now"
        ));
        assert!(matches!(
            &events[1],
            StreamingEvent::ToolCallEnd { call_id, ok: true, .. } if call_id == "c1"
        ));
        // 第二次请求序列里出现 Assistant(tool_calls) 与 Tool。
        let requests = fake.requests();
        assert_eq!(requests.len(), 2);
        let second = &requests[1].messages;
        assert!(second.iter().any(|message| matches!(
            message,
            LlmMessage::Assistant { tool_calls, .. } if tool_calls[0].call_id == "c1"
        )));
        assert!(second.iter().any(|message| matches!(
            message,
            LlmMessage::Tool { tool_call_id, content, .. }
                if tool_call_id == "c1" && content.contains("\"now\"")
        )));
        // A5：每个 Invoking 入口恰好一条 Progress。
        assert_eq!(progress_events(&ctx).len(), 2);
        // A2：最终文本 = 全部 TokenDelta 拼接。
        assert_eq!(ctx.token_text(), "done");
    }

    #[tokio::test]
    async fn multiple_tool_calls_run_serially_in_array_order() {
        let fake = script(json!({
            "chunk_chars": 64,
            "calls": [
                tool_calls(json!([
                    {"call_id": "c1", "name": "add", "args": {"a": 1, "b": 2}},
                    {"call_id": "c2", "name": "now", "args": {}}
                ])),
                text("ok")
            ]
        }));
        let runner = Arc::new(
            FakeToolRunner::new()
                .behavior("add", FakeToolBehavior::Return(json!({"sum": 3})))
                .behavior("now", FakeToolBehavior::Return(json!({"now": "t"}))),
        );
        let tools = vec![no_args_tool("add"), no_args_tool("now")];
        let registry = registry(tools.clone());
        let agent = loaded_agent(tools, 5, 1, "role");
        let ctx = TestCtx::new();
        let output = execute(&agent, &registry, &fake, &runner, &ctx)
            .await
            .expect("multi tool");
        assert_eq!(
            output.iterations, 2,
            "一个响应多个 tool_call 只 +1 iteration"
        );
        let names: Vec<String> = runner
            .calls()
            .into_iter()
            .map(|request| request.tool_name)
            .collect();
        assert_eq!(names, vec!["add".to_string(), "now".to_string()]);
        let starts: Vec<String> = tool_events(&ctx)
            .into_iter()
            .filter_map(|event| match event {
                StreamingEvent::ToolCallStart { call_id, .. } => Some(call_id),
                _ => None,
            })
            .collect();
        assert_eq!(starts, vec!["c1".to_string(), "c2".to_string()]);
    }

    #[tokio::test]
    async fn tool_exception_is_fed_back_and_loop_continues() {
        let fake = script(json!({
            "chunk_chars": 64,
            "calls": [
                tool_calls(json!([{"call_id": "c1", "name": "boom", "args": {}}])),
                text("recovered")
            ]
        }));
        let runner = Arc::new(FakeToolRunner::new().behavior(
            "boom",
            FakeToolBehavior::Exception {
                error_type: "ValueError".to_string(),
                message: "bad".to_string(),
            },
        ));
        let registry = registry(vec![no_args_tool("boom")]);
        let agent = loaded_agent(vec![no_args_tool("boom")], 5, 1, "role");
        let ctx = TestCtx::new();
        let output = execute(&agent, &registry, &fake, &runner, &ctx)
            .await
            .expect("exception continues");
        assert_eq!(output.content, "recovered");
        assert_eq!(runner.calls().len(), 1);
        let requests = fake.requests();
        let tool_message = requests[1]
            .messages
            .iter()
            .find_map(|message| match message {
                LlmMessage::Tool { content, .. } => Some(content.clone()),
                _ => None,
            })
            .expect("tool message");
        assert!(
            tool_message.contains("\"exception\"") && tool_message.contains("bad"),
            "tool message: {tool_message}"
        );
        assert!(matches!(
            &tool_events(&ctx)[1],
            StreamingEvent::ToolCallEnd { ok: false, .. }
        ));
    }

    #[tokio::test]
    async fn tool_timeout_is_fed_back_and_loop_continues() {
        let fake = script(json!({
            "chunk_chars": 64,
            "calls": [
                tool_calls(json!([{"call_id": "c1", "name": "slow", "args": {}}])),
                text("after timeout")
            ]
        }));
        let runner = Arc::new(FakeToolRunner::new().behavior("slow", FakeToolBehavior::Timeout));
        let slow = tool_def("slow", json!({"type": "object", "properties": {}}), 30);
        let registry = registry(vec![slow.clone()]);
        let agent = loaded_agent(vec![slow], 5, 1, "role");
        let ctx = TestCtx::new();
        let output = execute(&agent, &registry, &fake, &runner, &ctx)
            .await
            .expect("timeout continues");
        assert_eq!(output.content, "after timeout");
        let requests = fake.requests();
        let tool_message = requests[1]
            .messages
            .iter()
            .find_map(|message| match message {
                LlmMessage::Tool { content, .. } => Some(content.clone()),
                _ => None,
            })
            .expect("tool message");
        assert!(
            tool_message.contains("\"timeout\"") && tool_message.contains("30"),
            "tool message: {tool_message}"
        );
    }

    #[tokio::test]
    async fn unknown_tool_is_crashed_before_execution() {
        let fake = script(json!({
            "calls": [tool_calls(json!([{"call_id": "c1", "name": "ghost", "args": {}}]))]
        }));
        let runner = Arc::new(FakeToolRunner::new());
        let agent = loaded_agent(vec![], 5, 1, "role");
        let ctx = TestCtx::new();
        let failure = execute(&agent, &ToolRegistry::default(), &fake, &runner, &ctx)
            .await
            .expect_err("unknown tool");
        match failure {
            RunFailure::Harness(error) => {
                assert_eq!(error.kind, zgraph_kernel::HarnessErrorKind::UnknownTool);
                assert_eq!(error.detail["tool"], "ghost");
                assert_eq!(error.detail["iteration"], 1);
            }
            other => panic!("expected Harness(UnknownTool), got {other:?}"),
        }
        assert!(runner.calls().is_empty());
        assert!(
            tool_events(&ctx).is_empty(),
            "unknown tool must not emit ToolCallStart"
        );
    }

    #[tokio::test]
    async fn max_iterations_reports_context_summary() {
        let fake = script(json!({
            "calls": [tool_calls(json!([{"call_id": "c1", "name": "now", "args": {}}]))]
        }));
        let runner = Arc::new(FakeToolRunner::new());
        let registry = registry(vec![no_args_tool("now")]);
        let agent = loaded_agent(vec![no_args_tool("now")], 2, 1, "role");
        let ctx = TestCtx::new();
        let failure = execute(&agent, &registry, &fake, &runner, &ctx)
            .await
            .expect_err("max iterations");
        match failure {
            RunFailure::Harness(error) => {
                assert_eq!(error.kind, zgraph_kernel::HarnessErrorKind::MaxIterations);
                assert_eq!(error.detail["iterations"], 2);
                assert_eq!(error.detail["max_iterations"], 2);
                assert_eq!(error.detail["pending_tool_calls"], json!(["now"]));
                assert!(error.detail["last_content_summary"].is_string());
                assert!(error.detail["usage"].is_object());
            }
            other => panic!("expected Harness(MaxIterations), got {other:?}"),
        }
        assert_eq!(runner.calls().len(), 2);
    }

    #[tokio::test]
    async fn two_consecutive_empty_responses_fail_with_empty_response() {
        let fake = script(json!({"calls": [text("")]}));
        let runner = Arc::new(FakeToolRunner::new());
        let agent = loaded_agent(vec![], 5, 1, "role");
        let ctx = TestCtx::new();
        let failure = execute(&agent, &ToolRegistry::default(), &fake, &runner, &ctx)
            .await
            .expect_err("empty responses");
        match failure {
            RunFailure::Harness(error) => {
                assert_eq!(error.detail["reason"], "empty_response");
                assert_eq!(error.detail["iteration"], 2);
            }
            other => panic!("expected empty_response, got {other:?}"),
        }
        assert_eq!(fake.requests().len(), 2);
    }

    #[tokio::test]
    async fn a_single_empty_response_continues_to_next_round() {
        let fake = script(json!({"calls": [text(""), text("after")]}));
        let runner = Arc::new(FakeToolRunner::new());
        let agent = loaded_agent(vec![], 5, 1, "role");
        let ctx = TestCtx::new();
        let output = execute(&agent, &ToolRegistry::default(), &fake, &runner, &ctx)
            .await
            .expect("single empty then text");
        assert_eq!(output.content, "after");
        assert_eq!(output.iterations, 2);
    }

    #[tokio::test]
    async fn retry_on_establish_failure_succeeds_and_does_not_consume_iteration() {
        let fake = script(json!({
            "chunk_chars": 64,
            "calls": [text("ok")],
            "failures": [{
                "call_index": 0, "attempt": 0, "phase": "establish",
                "kind": "RateLimited", "status": 429, "retry_after_ms": 1
            }]
        }));
        let runner = Arc::new(FakeToolRunner::new());
        let agent = loaded_agent(vec![], 5, 1, "role");
        let ctx = TestCtx::new();
        let output = execute(&agent, &ToolRegistry::default(), &fake, &runner, &ctx)
            .await
            .expect("retry success");
        assert_eq!(output.iterations, 1, "重试不消耗 iteration");
        assert_eq!(fake.requests().len(), 2);
    }

    #[tokio::test]
    async fn retry_exhaustion_maps_to_llm_error_with_attempts() {
        let fake = script(json!({
            "calls": [text("ok")],
            "failures": [
                {"call_index": 0, "attempt": 0, "phase": "establish", "kind": "ServerError", "status": 500},
                {"call_index": 0, "attempt": 1, "phase": "establish", "kind": "ServerError", "status": 500}
            ]
        }));
        let runner = Arc::new(FakeToolRunner::new());
        let agent = loaded_agent(vec![], 5, 1, "role");
        let ctx = TestCtx::new();
        let failure = execute(&agent, &ToolRegistry::default(), &fake, &runner, &ctx)
            .await
            .expect_err("retry exhausted");
        match failure {
            RunFailure::Harness(error) => {
                assert_eq!(error.kind, zgraph_kernel::HarnessErrorKind::LlmError);
                assert_eq!(error.detail["attempts"], 2);
                assert_eq!(error.detail["status"], 500);
                assert_eq!(error.detail["phase"], "establish");
                assert_eq!(error.detail["stage"], "invoke");
            }
            other => panic!("expected Harness(LlmError), got {other:?}"),
        }
        assert_eq!(fake.requests().len(), 2);
    }

    #[tokio::test]
    async fn streaming_failure_is_not_retried() {
        let fake = script(json!({
            "chunk_chars": 2,
            "calls": [text("abcdef")],
            "failures": [{
                "call_index": 0, "attempt": 0, "phase": "streaming", "kind": "StreamBroken"
            }]
        }));
        let runner = Arc::new(FakeToolRunner::new());
        let agent = loaded_agent(vec![], 5, 3, "role");
        let ctx = TestCtx::new();
        let failure = execute(&agent, &ToolRegistry::default(), &fake, &runner, &ctx)
            .await
            .expect_err("streaming failure");
        match failure {
            RunFailure::Harness(error) => {
                assert_eq!(error.kind, zgraph_kernel::HarnessErrorKind::LlmError);
                assert_eq!(error.detail["phase"], "streaming");
            }
            other => panic!("expected Harness(LlmError), got {other:?}"),
        }
        assert_eq!(fake.requests().len(), 1, "streaming 阶段不重试");
        assert_eq!(ctx.token_text(), "ab", "阶段内已收到的增量不外发聚合");
    }

    #[tokio::test]
    async fn cancellation_before_call_returns_cancelled() {
        let fake = script(json!({"calls": [text("never")]}));
        let runner = Arc::new(FakeToolRunner::new());
        let agent = loaded_agent(vec![], 5, 1, "role");
        let ctx = TestCtx::new();
        ctx.request_cancel();
        let failure = execute(&agent, &ToolRegistry::default(), &fake, &runner, &ctx)
            .await
            .expect_err("cancelled");
        assert_eq!(failure, RunFailure::Cancelled);
        assert!(fake.requests().is_empty());
    }

    #[tokio::test]
    async fn exhausted_deadline_returns_instance_timeout() {
        let fake = script(json!({"calls": [text("never")]}));
        let runner = Arc::new(FakeToolRunner::new());
        let agent = loaded_agent(vec![], 5, 1, "role");
        let ctx = TestCtx::with_remaining_ms(0);
        let failure = execute(&agent, &ToolRegistry::default(), &fake, &runner, &ctx)
            .await
            .expect_err("deadline");
        assert_eq!(failure, RunFailure::InstanceTimeout { elapsed_ms: 10_000 });
        assert_eq!(
            failure.clone().into_node_error(),
            NodeError::Timeout { elapsed_ms: 10_000 }
        );
        assert!(fake.requests().is_empty());
    }

    #[tokio::test]
    async fn near_deadline_does_not_start_new_call() {
        let fake = script(json!({"calls": [text("never")]}));
        let runner = Arc::new(FakeToolRunner::new());
        let agent = loaded_agent(vec![], 5, 1, "role");
        let ctx = TestCtx::with_remaining_ms(500);
        let failure = execute(&agent, &ToolRegistry::default(), &fake, &runner, &ctx)
            .await
            .expect_err("near deadline");
        assert!(matches!(failure, RunFailure::InstanceTimeout { .. }));
        assert!(fake.requests().is_empty());
    }

    #[tokio::test]
    async fn progress_percent_and_message_follow_spec() {
        let fake = script(json!({
            "chunk_chars": 64,
            "calls": [
                tool_calls(json!([{"call_id": "c1", "name": "now", "args": {}}])),
                text("done")
            ]
        }));
        let runner = Arc::new(FakeToolRunner::new());
        let registry = registry(vec![no_args_tool("now")]);
        let agent = loaded_agent(vec![no_args_tool("now")], 4, 1, "role");
        let ctx = TestCtx::new();
        execute(&agent, &registry, &fake, &runner, &ctx)
            .await
            .expect("progress");
        assert_eq!(
            progress_events(&ctx),
            vec![
                (25, "llm call 1/4".to_string()),
                (50, "llm call 2/4".to_string())
            ]
        );
    }

    #[tokio::test]
    async fn context_overflow_maps_to_authoritative_code() {
        let fake = script(json!({"calls": [text("never")]}));
        let runner = Arc::new(FakeToolRunner::new());
        let agent = loaded_agent(vec![], 5, 1, &"x".repeat(300_000));
        let ctx = TestCtx::new();
        let failure = execute(&agent, &ToolRegistry::default(), &fake, &runner, &ctx)
            .await
            .expect_err("overflow");
        match failure {
            RunFailure::Harness(error) => {
                assert_eq!(error.kind, zgraph_kernel::HarnessErrorKind::ContextOverflow);
                assert_eq!(error.detail["code"], "HARNESS_CONTEXT_OVERFLOW");
                assert_eq!(error.detail["stage"], "assemble");
                assert_eq!(error.detail["first_over"], "system");
            }
            other => panic!("expected ContextOverflow, got {other:?}"),
        }
        assert!(fake.requests().is_empty());
    }

    #[tokio::test]
    async fn invalid_tool_arguments_are_fed_back_without_calling_python() {
        let fake = script(json!({
            "chunk_chars": 64,
            "calls": [
                tool_calls(json!([{"call_id": "c1", "name": "add", "args": {}}])),
                text("fixed")
            ]
        }));
        let runner = Arc::new(FakeToolRunner::new());
        let add = tool_def(
            "add",
            json!({
                "type": "object",
                "properties": {"a": {"type": "number"}, "b": {"type": "number"}},
                "required": ["a", "b"]
            }),
            DEFAULT_TOOL_TIMEOUT_MS,
        );
        let registry = registry(vec![add.clone()]);
        let agent = loaded_agent(vec![add], 5, 1, "role");
        let ctx = TestCtx::new();
        let output = execute(&agent, &registry, &fake, &runner, &ctx)
            .await
            .expect("args validation continues");
        assert_eq!(output.content, "fixed");
        assert!(runner.calls().is_empty(), "参数校验失败不得调用工具");
        let tool_message = fake.requests()[1]
            .messages
            .iter()
            .find_map(|message| match message {
                LlmMessage::Tool { content, .. } => Some(content.clone()),
                _ => None,
            })
            .expect("tool message");
        assert!(tool_message.contains("missing_required"));
        assert!(matches!(
            &tool_events(&ctx)[1],
            StreamingEvent::ToolCallEnd { ok: false, .. }
        ));
    }

    #[tokio::test]
    async fn non_object_tool_result_is_fed_back() {
        let fake = script(json!({
            "chunk_chars": 64,
            "calls": [
                tool_calls(json!([{"call_id": "c1", "name": "now", "args": {}}])),
                text("ok")
            ]
        }));
        let runner = Arc::new(
            FakeToolRunner::new().behavior("now", FakeToolBehavior::Return(json!([1, 2, 3]))),
        );
        let registry = registry(vec![no_args_tool("now")]);
        let agent = loaded_agent(vec![no_args_tool("now")], 5, 1, "role");
        let ctx = TestCtx::new();
        execute(&agent, &registry, &fake, &runner, &ctx)
            .await
            .expect("non-object continues");
        let tool_message = fake.requests()[1]
            .messages
            .iter()
            .find_map(|message| match message {
                LlmMessage::Tool { content, .. } => Some(content.clone()),
                _ => None,
            })
            .expect("tool message");
        assert!(tool_message.contains("result_not_object"));
    }

    #[tokio::test]
    async fn registered_but_unauthorized_tool_is_unknown_and_not_executed() {
        // REV-020 C2：registry（发现结果）含 a/b，Agent 可见集只含 a；LLM 调 b →
        // HARNESS_UNKNOWN_TOOL（Crashed），b 不执行，available_tools 只列可见集。
        let fake = script(json!({
            "calls": [tool_calls(json!([{"call_id": "c1", "name": "b", "args": {}}]))]
        }));
        let runner = Arc::new(FakeToolRunner::new());
        let registry = registry(vec![no_args_tool("a"), no_args_tool("b")]);
        let agent = loaded_agent(vec![no_args_tool("a")], 5, 1, "role");
        let ctx = TestCtx::new();
        let failure = execute(&agent, &registry, &fake, &runner, &ctx)
            .await
            .expect_err("unauthorized tool");
        match failure {
            RunFailure::Harness(error) => {
                assert_eq!(error.kind, zgraph_kernel::HarnessErrorKind::UnknownTool);
                assert_eq!(error.detail["tool"], "b");
                assert_eq!(error.detail["available_tools"], json!(["a"]));
            }
            other => panic!("expected Harness(UnknownTool), got {other:?}"),
        }
        assert!(runner.calls().is_empty(), "未授权工具不得执行");
        assert!(tool_events(&ctx).is_empty());
    }

    #[tokio::test]
    async fn oversized_tool_result_is_truncated_in_context() {
        let big = json!({"text": "x".repeat(20_000)});
        let fake = script(json!({
            "chunk_chars": 64,
            "calls": [
                tool_calls(json!([{"call_id": "c1", "name": "big", "args": {}}])),
                text("ok")
            ]
        }));
        let runner = Arc::new(FakeToolRunner::new().behavior("big", FakeToolBehavior::Return(big)));
        let registry = registry(vec![no_args_tool("big")]);
        let agent = loaded_agent(vec![no_args_tool("big")], 5, 1, "role");
        let ctx = TestCtx::new();
        execute(&agent, &registry, &fake, &runner, &ctx)
            .await
            .expect("oversized result continues");
        let tool_message = fake.requests()[1]
            .messages
            .iter()
            .find_map(|message| match message {
                LlmMessage::Tool { content, .. } => Some(content.clone()),
                _ => None,
            })
            .expect("tool message");
        assert!(
            tool_message.len() <= crate::constants::TOOL_RESULT_MAX_BYTES,
            "tool content {} bytes",
            tool_message.len()
        );
        assert!(tool_message.contains("truncated"));
    }
}
