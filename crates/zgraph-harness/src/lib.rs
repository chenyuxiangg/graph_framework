//! `zgraph-harness`：`agent_call` 节点的 Agent Harness（M4）。
//!
//! 语义基准：`doc/spec-v1.2/05-AgentHarness.md`（AgentConfig、`LlmProvider`、上下文组装与
//! token 预算、Harness 状态机与 tool loop、工具注册表、错误映射、流式事件、输出契约）。
//! 落地偏差：`doc/architecture-v2.md` §8 —— 与 M4 直接相关的是 **D3**（工具每次调用一个
//! Python worker 子进程、无跨调用模块缓存）与 **D7**（图加载期不 import 用户 Python）。
//!
//! # 模块职责
//!
//! - [`config`]：AgentConfig 资产加载与 V1–V16 校验（05 §5.2）。
//! - [`assemble`]/[`budget`]：system/user 消息组装与 token 预算（05 §5.4）。
//! - [`llm`]：transport 抽象（`LlmProvider`）、类型层、`FakeLlmProvider` 与 genai 适配。
//! - [`tools`]：工具注册表、参数校验、结果截断、`WorkerToolRunner`/`FakeToolRunner`（05 §5.6）。
//! - [`agent_loop`]：六状态执行循环、重试、iteration 与 tool loop（05 §5.5）。
//! - [`executor`]：`NodeExecutor` 实现（唯一处理 `CompiledHandler::Agent`）。
//! - [`error`]：`HarnessError` → `NodeError::Harness` 映射（05 §5.7）。
//!
//! # v2 适配（写进代码注释，供 spec 回写）
//!
//! - **D3**：工具不在内核进程内执行；`WorkerToolRunner` 每次调用 spawn 一个
//!   `python -m zgraph_worker.runtime` 子进程，module 固定 `zgraph_worker.tool_runner`，
//!   function 为 `discover`/`call`。工具发现结果在 executor 内按 `tools_dir` 缓存一次；
//!   工具模块本身**无**跨调用缓存（进程即缓存边界）。
//! - **取消**：v2 `NodeContext` 只有 `cancel_requested()` 轮询，没有 `CancellationToken`；
//!   [`CancelToken`] 只是对 `HarnessCtx` 的只读包装（每 ≤50ms 轮询一次），不伪造
//!   `cancelled().await` 语义。
//! - **`HarnessCtx` seam**：生产路径只由 `NodeContext` 适配而来（H1：不触碰
//!   `RunContext`/调度器）；测试可注入自己的实现。
//! - **重试编排位置**：重试在 Harness 内部（§5.3.5 规则），不在 adapter 内；这与
//!   05 §5.5.4 伪代码“provider 内部”的措辞不同，但语义等价且更可测。
//!
//! # 公开 API 稳定性
//!
//! `HarnessSettings`/`HarnessExecutor`/`LlmProvider`/`LlmStreamSink`/`FakeLlmProvider`/
//! `CancelToken`/`HarnessCtx`/`ToolRunner`/`WorkerToolRunner`/`FakeToolRunner`/`AgentOutput`
//! 为 e2e 与 M5 依赖的冻结面（任务契约）；新增项只能追加。

#![forbid(unsafe_code)]
#![warn(missing_docs)]

mod agent_loop;
mod assemble;
mod budget;
mod config;
mod constants;
mod ctx;
mod error;
mod executor;
mod llm;
mod tools;

pub use agent_loop::{AgentOutput, RunFailure};
pub use assemble::{
    assemble_initial_messages, assemble_system, assemble_user_json, effective_input_mapping,
    SystemPrompt,
};
pub use budget::{
    check_and_trim, estimate_message_tokens, estimate_messages_tokens, estimate_tokens,
    prompt_budget, BudgetSegments,
};
pub use config::{agent_index, load_agent, AgentConfig, LoadedAgent, ModelConfig};
pub use constants::*;
pub use ctx::{CancelToken, HarnessCtx};
pub use error::{HarnessError, CODE_CONTEXT_OVERFLOW, CODE_TOOL_ERROR};
pub use executor::{HarnessExecutor, HarnessSettings};
pub use llm::{
    FakeLlmProvider, GenaiLlmProvider, LlmDelta, LlmError, LlmErrorKind, LlmMessage, LlmPhase,
    LlmProvider, LlmRequest, LlmResponse, LlmStreamSink, ModelRef, ProviderKind, SecretString,
    StopReason, ToolCall, Usage, UsageSource,
};
pub use tools::{
    FakeToolBehavior, FakeToolRunner, ToolCallRequest, ToolDef, ToolFailure, ToolRegistry,
    ToolRunner, ToolSchema, WorkerToolRunner,
};

#[cfg(test)]
mod test_support;
