> **归档说明**：Codex 复用可行性调研（冻结期归档）。结论已并入 DR-001；链路外部链接若失效以文字结论为准。

# Codex CLI (Rust) 复用可行性调研

> 调研对象：https://github.com/openai/codex 的 `codex-rs/`（Rust 实现）
> 快照时间：**2026-10-28 09:40 (+08:00)**；仓库 `pushed_at = 2026-10-28T01:29:52Z`，`default_branch = main`，阅读均为 `main` 分支文件（GitHub API / jsDelivr 读取，未 clone）。
> 结论先行：**不建议复用其代码作为我们的 Harness 内核**；建议「**仅参考**」——借它的**契约设计**（Responses API 事件模型 / token delta 与 item 完成分离 / Op-Event 队列范式），不引入它的 crate。若一定要"跑起来一个 Codex"，唯一较现实的形态是 **app-server 子进程 + JSON-RPC**，而非 Rust 库依赖。
> 标注约定：**【事实】**= 有 URL/文件行号可查证；**【推断】**= 我的判断，未实测。

---

## 0. 一句话结论 + 关键数字

| 问题 | 结论 | 依据 |
|---|---|---|
| 能整包当一个 crate 依赖吗 | **不能直接 `cargo add`**：未发布到 crates.io；只能 `git` 依赖 | 见 §2.1 |
| `git` 依赖技术上可行吗 | 可行，但会拖入 **74 个 in-repo crate + 41 个外部 crate**（`codex-core` 一级依赖 115 个） | 见 §4.1 |
| 有官方「嵌入式」支持吗 | 有官方**示例**（`thread-manager-sample`），但**没有稳定的嵌入 API**；社区同类质疑 issue 仍 open 且无维护者回复 | 见 §2.3 / §2.4 |
| 我们要的三处拦截点存在吗 | **都存在**（in-process extension API 或 app-server 协议），但都绑定在它的 turn 循环/状态机里 | 见 §3 |
| 最大障碍 | 不是"能不能拦"，而是"**它拥有 LLM 调用和 turn 循环**"：我们要自己拥有 Harness 的 6 个阶段，复用等于把内核交给一个 **0.166-alpha、每周多次发版、无 semver** 的第三方 | 见 §5 |
| 预估集成成本 | 整包 git 依赖 **25–40 人日**（+ 每月 1–2 人日维护）；app-server 子进程 **12–20 人日**；仅参考自研 **3–5 人日** | 见 §5 |

---

## 1. 仓库与许可【事实】

| 项 | 值 | 证据 |
|---|---|---|
| License | **Apache-2.0**（spdx `Apache-2.0`） | `GET /repos/openai/codex` 的 `license` 字段；[LICENSE](https://github.com/openai/codex/blob/main/LICENSE)（10926 B，含完整 Apache 2.0 文本） |
| README 声明 | "This repository is licensed under the [Apache-2.0 License](https://github.com/openai/codex/blob/main/LICENSE)." | [README.md](https://github.com/openai/codex/blob/main/README.md) |
| Stars / Forks | 131,281 / 20,740 | GitHub API，2026-10-28 |
| 创建时间 | 2025-04-13 | 同上 |
| 最后 push | 2026-10-28T01:29:52Z（调研当天仍在提交） | 同上 |
| 仓库体积 | ~704,630 KB（API `size`，git 对象未经压缩的近似值） | 同上 |
| edition / toolchain | **edition = 2024**，`rust-toolchain.toml` 固定 **channel = "1.95.0"** | [codex-rs/Cargo.toml](https://github.com/openai/codex/blob/main/codex-rs/Cargo.toml) `[workspace.package]`；[rust-toolchain.toml](https://github.com/openai/codex/blob/main/codex-rs/rust-toolchain.toml) |
| workspace 版本 | **`version = "0.0.0"`**（所有内部 crate 继承） | codex-rs/Cargo.toml `[workspace.package]`（0.0.0 说明从未打算发布） |
| workspace 成员数 | **165** | codex-rs/Cargo.toml `members = [...]` 计数 |

`codex-rs/` 目录结构（顶层，165 个成员 crate 的一小部分）：`core/`、`core-api/`、`codex-api/`、`codex-client/`、`protocol/`、`tools/`、`config/`、`ext/*`（插件）、`app-server*/`、`exec-server*/`、`sandboxing/`、`linux-sandbox/`、`windows-sandbox-rs/`、`mxc-sandbox/`、`tui/`、`codex-mcp/`、`rmcp-client/`、`rollout/`、`state/`、`models-manager/`、`model-provider*/`、`code-mode*/`、`v8-poc/`、`otel*/`、`vendor/` 等。
（来源：[codex-rs 目录列表](https://github.com/openai/codex/tree/main/codex-rs)，GitHub contents API）

**许可兼容性【事实+推断】**：Apache-2.0 与我们（假设 Apache/MIT）兼容，可自由复制/修改/商用，**但**（a）复制其代码需保留版权与 NOTICE 类归属；（b）"Codex" 是 OpenAI 商标，不能用于我们的产品命名。→ 许可**不构成**复用障碍，真正的障碍在工程耦合与维护。

---

## 2. 能否作为 Rust 库依赖？

### 2.1 crates.io：**没有发布**【事实】

直接查 crates.io API（2026-10-28）：

| crate 名 | crates.io 结果 |
|---|---|
| `codex-core` | **不存在**（`{"errors":[{"detail":"crate codex-core does not exist"}]}`） |
| `codex-core-api` | 不存在 |
| `codex-tools` | 不存在 |
| `codex-config` | 不存在 |
| `codex-api` | 不存在 |
| `codex-cli` | 不存在 |
| `codex-protocol` | **存在但无关**：仓库 `github.com/namastexlabs/codex`，0.63.0，2315 次下载 —— 是**同名的第三方 crate**，不是 OpenAI 的 |

> 证据：`https://crates.io/api/v1/crates/{name}`（逐个查询）。另外 workspace 版本写死 `0.0.0`，也从另一面证明官方没有发布库 crate 的计划【事实】。
> 官方分发渠道是 **npm `@openai/codex` / Homebrew / GitHub Releases 二进制**（见 README），定位是"终端应用"，不是库。

**⚠️ 命名陷阱**：如果想"就近试试"，`cargo add codex-protocol` 装到的是**完全无关的第三方实现**；`codex` 这个 crate 名是 typst 的 Unicode 符号库。任何"从 crates.io 拉 codex"的做法都是错的。

### 2.2 `git` 依赖：技术可行，成本高【事实+推断】

Cargo 允许 `git = "https://github.com/openai/codex", rev = "...", package = "codex-core"`，并从该 git 仓库的 workspace 中解析所有 path 依赖（同一 rev）。因此**技术上可以编过**（未实测构建，见 §7 限制）。

代价（【事实】来自 manifests）：
- `codex-core` 的 `[dependencies]` 共 **115 条**，其中 **74 条是 in-repo `codex-*` path 依赖**；
- 需要 **Rust 1.95.0 + edition 2024**（这会强制我们内核的 toolchain 升级/对齐）；
- 无 semver 约束：workspace 版本恒为 `0.0.0`，只能钉 `rev`，每次升级都是"重新评估一遍 API"；
- 首编译把 74 个 in-repo crate 全拉进来（不是按需裁剪）。

### 2.3 公开 API 与官方嵌入示例【事实】

- **官方 facade**：`codex-core-api`（`codex_core_api`），源码仅 **7,357 B ≈ 151 LOC**，内容基本是 `pub use codex_core::...` 的一长串 re-export。
  链接：[core-api/src/lib.rs](https://github.com/openai/codex/blob/main/codex-rs/core-api/src/lib.rs)
- **官方嵌入示例**：`codex-rs/thread-manager-sample`（约 20 KB main.rs），CLI 描述即 "Run one Codex turn through ThreadManager and print mapped notifications as newline-delimited JSON."
  [thread-manager-sample/src/main.rs](https://github.com/openai/codex/blob/main/codex-rs/thread-manager-sample/src/main.rs)
  其嵌入流程（**这就是官方认可的 in-process 用法**）：
  1. `find_codex_home()` → `load_config_toml_with_layer_stack(...)` → `bootstrap_auth_config(...)`（**读磁盘上的 config.toml + 凭据**）
  2. `ExtensionRegistryBuilder::<Config>::new()` + `install_image_generation_extension(...)`
  3. `ThreadManager::new(&config, auth_manager, build_models_manager(...), CodexAppsToolsCache::default(), SessionSource::Exec, environment_manager, Arc::new(extensions.build()), user_instructions_provider, None, passthrough_image_store(), thread_store, local_agent_graph_store_from_state_db(state_db), installation_id, None, None)` — **14 个参数**
  4. `thread_manager.start_thread(StartThreadOptions::new(config)).await`
  5. 消费 `EventMsg`（示例里把事件映射成 app-server notification），`thread.shutdown_and_wait()`

  → 一个"跑一轮"的示例需要 **config 加载 + auth manager + models manager + state DB + thread store + environment manager + extension registry + user instructions provider + installation id**。这就是"嵌入面"的真实宽度。

- **公开核心类型**（可用于嵌入）：`ThreadManager`（旧名 `ConversationManager`，已 `#[deprecated(note = "use ThreadManager")]`）、`CodexThread`（旧名 `CodexConversation`）、`NewThread`（旧名 `NewConversation`）、`StartThreadOptions`、`TurnInput/TurnInputRequest`、`EventMsg`、`Op`。
  [core/src/lib.rs](https://github.com/openai/codex/blob/main/codex-rs/core/src/lib.rs)（可在其中看到三组 `#[deprecated]` 别名——**重命名式 API churn 已经在发生**）
- **官方对"嵌入"的定位**：`codex-rs/docs/protocol_v1.md` 明确写 "Codex is external to the UI, as `Codex` is intended to be operated by arbitrary UI implementations."，且 Op/EventMsg **都是 `non_exhaustive`，文档原话："It should be expected that new `EventMsg` variants will be added over time"**。
  [docs/protocol_v1.md](https://github.com/openai/codex/blob/main/codex-rs/docs/protocol_v1.md)

### 2.4 社区已有的同类结论（重要旁证）【事实】

GitHub issue **#56902**（2026-10-25 创建，**至今 open，0 条评论**）标题就是：
"`core-api` is a 151-LOC stub while `codex-core` pulls 71 in-repo crates — is a thin embedding facade planned?"

> 该 issue 在 commit `970111f`（2026-10-23）实测：`core/Cargo.toml` 声明 **110 个直接依赖，其中 71 个是 in-repo `codex-*`**；结论原话："`codex-core` has no boundary that would let a host take the engine without the rest of the stack."（core 没有边界能让宿主只拿走引擎而不带上整栈）。
> 并且指出依赖方向反转：`core/src/thread_manager.rs:44-45` 反过来 `use codex_app_server_protocol::ThreadHistoryBuilder`（引擎依赖 RPC 层）。**维护者未回复，无 roadmap 承诺**。

对照我在 **2026-10-28 main** 的实测：`codex-core` `[dependencies]` = **115 条，in-repo 74 条**。→ 5 天内一级依赖 +5、in-repo +3【事实】。这条同时是"活跃"和"churn"的证据。
链接：https://github.com/openai/codex/issues/56902

---

## 3. 契约匹配度（最关键）——逐项回答

先给总表，后附细节与代码证据。

| 我们的需求 | Codex 提供什么 | 差距 | 我们能否注入 |
|---|---|---|---|
| **(a) token 级流式** | 有两层：LLM 层 `ResponseEvent::OutputTextDelta / ToolCallInputDelta / ReasoningContentDelta`；会话层 `EventMsg::AgentMessageContentDelta / ReasoningContentDelta` | 命名/粒度是它的；无"我们的事件 ID/span"概念 | ✅ 可（`ModelResponseInterceptor` 包装流，或消费 `EventMsg`） |
| **(a) 第三方 endpoint** | 仅 **Responses API**：`wire_api = "chat"` 已被**硬删除**（反序列化直接报错） | **Chat Completions 不再可用**；DeepSeek 目前**已提供 `/responses`**（见 §3.1），勉强对得上 | ⚠️ 需 Responses 兼容端点 |
| **(b) 自定义工具（Python skill 注册表）** | in-process：`ExtensionRegistryBuilder::tool_contributor` → `Vec<Arc<dyn ToolExecutor<ToolCall>>>`；out-of-process：app-server `dynamicTools`（**experimental**）+ `item/tool/call` 请求 | 工具必须实现它的 `ToolExecutor` trait（async、`FunctionCallError`、`ToolSpec`）；MCP 是另一条路 | ✅ 可（包装 `spawn_blocking` 调 Python） |
| **(c) 会话/上下文自管** | 它自己拥有：`Config`（磁盘 config.toml 分层）、conversation / `rollout`（JSONL + SQLite state）、compaction（多个 `compact_*` 模块）、AGENTS.md、skills、plugins、内置 system prompt（`codex-prompts`） | 我们要自己控制 system prompt 组装 / token 预算 / input_mapping → 只能"部分覆盖"（ContextContributor / PromptSlot / developer_instructions），**不能拿走 turn 循环** | ⚠️ 只能部分注入，无法接管 |
| **(d) 沙箱/执行** | 强绑定：`codex-exec`、`exec-server`、`shell-command`、`sandboxing`、`linux-sandbox`、`windows-sandbox-rs`、`seatbelt`(macOS)、`landlock`+`seccompiler`(Linux, 依赖挂在 `codex-protocol` 上) | 可配置为**不沙箱**（`SandboxPolicy::DangerFullAccess`），但**依赖仍在编译单元里**，且我们的 PyO3/skill 执行路径与它的 exec 子系统重叠 | ⚠️ 可关策略，去不掉依赖 |

### 3.1 (a) LLM 调用与流式

**token delta 存在【事实】**：`ResponseEvent`（`codex-api`，被 `codex-core` re-export 为 `ResponseStream`/`ResponseEvent`）
[codex-api/src/common.rs](https://github.com/openai/codex/blob/main/codex-rs/codex-api/src/common.rs)
```rust
pub enum ResponseEvent {
    Created { response_id: Option<String> },
    OutputItemAdded(ResponseItem),
    OutputItemDone(ResponseItem),
    OutputTextDelta(String),                                  // ← 逐 token 文本
    ToolCallInputDelta { item_id: String, call_id: Option<String>, delta: String }, // ← 工具参数增量
    ReasoningSummaryDelta { delta: String, summary_index: i64 },
    ReasoningContentDelta { delta: String, content_index: i64 },
    Completed { response_id: String, token_usage: Option<TokenUsage>, end_turn: Option<bool>, .. },
    RateLimits(..), ModelsEtag(..), SafetyBuffering(..), ModelVerifications(..), ...
}
```
会话层对应 `EventMsg::AgentMessageContentDelta` / `ReasoningContentDelta` / `ExecCommandOutputDelta`（`protocol/src/protocol.rs` 第 1707 / 1709 / 1627 行），以及 `EventMsg::DynamicToolCallRequest`（第 1643 行）。
[protocol/src/protocol.rs](https://github.com/openai/codex/blob/main/codex-rs/protocol/src/protocol.rs)

**第三方 endpoint：只支持 Responses 协议，Chat Completions 已被删除【事实】**
`codex-rs/model-provider-info/src/lib.rs` 第 108 行：
```rust
const CHAT_WIRE_API_REMOVED_ERROR: &str = "`wire_api = \"chat\"` is no longer supported.\n
How to fix: set `wire_api = \"responses\"` in your provider config.\n
More info: https://github.com/openai/codex/discussions/7782";
```
`WireApi` 枚举现在只剩 `Responses`；自定义 provider 支持 `base_url` + `env_key` + `http_headers` + `query_params` + `wire_api`（结构体 `ModelProviderInfo` 第 147 行起）。
[model-provider-info/src/lib.rs](https://github.com/openai/codex/blob/main/codex-rs/model-provider-info/src/lib.rs)

**DeepSeek 侧的最新事实【事实】**：DeepSeek 已提供 Responses API 端点 `POST https://api.deepseek.com/responses`，返回**语义化 SSE**（`response.output_text.delta` / `response.reasoning_text.delta` / `response.output_item.added` / `response.completed`，**没有 `data: [DONE]`**），支持 `function` 工具与 `custom`（仅 `apply_patch`），`stream: true`，**无状态**（`store:false`，多轮需客户端回传全量 `input`）。
[DeepSeek: Responses API](https://api-docs.deepseek.com/api/create-response/) ｜ [DeepSeek Quick Start（OpenAI 兼容 base_url）](https://api-docs.deepseek.com/zh-cn/quick_start/model/)

→ 所以"Codex 只能连 ChatGPT 登录态"这个担心**不成立**；真正的问题是**它能发、我们不能完全控制它发什么**：
`ResponsesApiRequest` 会额外发送 `include`、`prompt_cache_key`、`client_metadata`、`service_tier`、`stream_options`、`access_programs`、`store` 等字段（[codex-api/src/common.rs](https://github.com/openai/codex/blob/main/codex-rs/codex-api/src/common.rs) `ResponsesApiRequest`），这些**不在 DeepSeek 文档的参数表里**。
【推断】DeepSeek 大概率忽略未知字段（其文档对 `input` 项写明"other types are ignored"），但 `include`/`prompt_cache_key` 是否会 400、`end_turn` 缺失时的 fallback 是否够用，**只能实测**。这条风险与"是否复用 Codex"无关，即使自研也要对齐。

### 3.2 (b) 工具注入

**in-process 注入点（最接近我们的需求）【事实】**：`codex-extension-api` 的 registry 提供十余种 contributor 注册钩子，其中：
[ext/extension-api/src/registry.rs](https://github.com/openai/codex/blob/main/codex-rs/ext/extension-api/src/registry.rs)
```rust
builder.tool_contributor(..)              // 注入自定义工具
builder.tool_lifecycle_contributor(..)    // on_tool_dispatch / on_tool_start / on_command_start / on_tool_finish
builder.model_request_contributor(..)     // 拦截 LLM 请求与响应流（token delta 的官方 tap 点）
builder.prompt_contributor(..)           // ContextContributor：往 prompt 里塞片段
builder.token_usage_contributor(..)
builder.turn_lifecycle_contributor / turn_item_contributor / message_lifecycle_contributor
builder.config_contributor / mcp_server_contributor / turn_input_contributor / skill_invocation_contributor / approval_review_contributor / turn_start_admission / event_sink
```
工具注入签名（[contributors.rs](https://github.com/openai/codex/blob/main/codex-rs/ext/extension-api/src/contributors.rs) 第 382 行）：
```rust
pub trait ToolContributor: Send + Sync {
    fn tools(&self, session_store: &ExtensionData, thread_store: &ExtensionData)
        -> Vec<Arc<dyn for<'call> ToolExecutor<ToolCall<'call>>>>;
}
```
`ToolExecutor`（[tools/src/tool_executor.rs](https://github.com/openai/codex/blob/main/codex-rs/tools/src/tool_executor.rs)）：
```rust
pub trait ToolExecutor<Invocation>: Send + Sync {
    fn tool_name(&self) -> ToolName;
    fn spec(&self) -> ToolSpec;
    fn exposure(&self) -> ToolExposure;           // Direct/Deferred/CodeModeOnly/Hidden
    fn handle<'a>(&'a self, invocation: Invocation) -> ToolExecutorFuture<'a>;
}
pub type ToolExecutorFuture<'a> = Pin<Box<dyn Future<Output = Result<Box<dyn ToolOutput>, FunctionCallError>> + Send + 'a>>;
```
→ **好消息**：`handle` 返回 `Send + 'a` 的 boxed future，这正是"`tokio::task::spawn_blocking` 里跑 PyO3 同步调用、再 `await` 回来"的形状，与我们已确认的 Python 阻塞约束**天然兼容**。
→ **坏消息**：工具必须被塞进它的 registry、参与它的 `ToolExposure`/tool-search/code-mode 曝光策略；我们的 `ToolDef`（Python skill 库）需要在启动期做一次全量映射，且"哪些工具对本轮模型可见"由**它的** step 策略决定，不是我们的 context 组装决定。

**out-of-process 注入点【事实】**：app-server 的 `thread/start` 支持 `dynamicTools`（需 `capabilities.experimentalApi = true`），调用时发 `item/tool/call` 请求给客户端，客户端回结果；对应 item 类型 `dynamicToolCall`。
[官方文档 Codex App Server](https://developers.openai.com/codex/app-server/) 中 "Dynamic tool calls (experimental)" 与 "Item deltas" 章节（我读的是官方中文镜像 [sources/en/codex/app-server.md](https://github.com/crasuna/openai-dev-docs-cn-mirror/blob/main/sources/en/codex/app-server.md)，共 83 KB，内容与官方页一致）。
注意：**experimental** 意味着随时可能改/移除。

**MCP 能否单独用【事实】**：`codex-mcp` / `rmcp-client` 是它自己的 MCP 封装（依赖 `rmcp`）。`rmcp` 本身是 crates.io 上的公开 crate，**不需要**为 MCP 去复用 codex；`codex-mcp` 只是薄封装 + 策略。

### 3.3 (c) 会话 / 上下文管理

**强绑定，且是它最核心的护城河【事实】**：
- 配置来自**磁盘** `config.toml` 分层（`ConfigLayerStack` / `load_config_toml_with_layer_stack` / `requirements`），嵌入示例也走这条路；
- 对话历史持久化为 `rollout`（JSONL）+ `state`（SQLite：`init_state_db`、`thread_store_from_state_db`）+ `thread-store`、`history`、`memories`；
- 压缩自成一整套模块：`compact_token_budget.rs`、`compact_remote_history.rs`、`compact_retained_tool_outputs.rs`、`compact_model_fallback.rs`（见 core/src/lib.rs）；
- system prompt 来自 `codex-prompts` + AGENTS.md（`agents_md.rs`）+ skills + plugins + collaboration-mode-templates + `context-fragments`。

我们需要的"**自己组装 system prompt / token 预算 / input_mapping**"在它这里有三个可选突破口：
1. `ContextContributor` + `PromptFragment` / `PromptSlot` —— 只能**追加片段到既有 slot**；
2. `UserInstructionsProvider` / `ThreadInstructionsProvider` —— 可提供 user instructions（AGENTS.md 那一路）；
3. app-server `turn/start.settings.developer_instructions` —— 可设 developer instructions（`null` = 用内置）。

→ 也就是说：**能注入，不能接管**。它的 base instructions、工具描述渲染、compaction 时机、`response_id` 续接语义仍在它手里。这直接违背"加载 Agent 配置 → 组装上下文"这两个阶段必须由我们拥有的设计前提。

### 3.4 (d) 沙箱 / 执行策略

- 可以配成不沙箱：`SandboxPolicy::DangerFullAccess` 存在（`protocol.rs` 第 1225–1226 行 `#[serde(rename = "danger-full-access")]`），app-server 也允许 per-turn `sandbox policy` override。【事实】
- 但**依赖去不掉**：`codex-core` → `codex-exec-server`、`codex-execpolicy`、`codex-shell-command`、`codex-sandboxing`；`codex-sandboxing` → `codex-mxc-sandbox` / `codex-windows-sandbox` / `codex-network-proxy` / `codex-utils-pty`；`codex-protocol` 在 Linux 上还挂 `landlock` + `seccompiler`，macOS 挂 seatbelt 相关路径。【事实，见各 Cargo.toml】
- 含义【推断】：我们 v1 内核不需要这些，但会**全量编译**它们；`codex-exec-server` 还带来"环境/PTY/网络代理"整套概念，与我们的 Python skill 执行路径功能重叠且语义冲突（谁负责子进程生命周期？）。

---

## 4. 依赖与维护成本

### 4.1 规模数字（全部可复算）

| 指标 | 数值 | 来源 |
|---|---|---|
| `codex-core` `[dependencies]` | **115** 条（in-repo `codex-*` **74** 条，外部 **41** 条） | codex-rs/core/Cargo.toml @main，脚本计数 |
| 同上，5 天前的 issue 实测（`970111f`） | 110 条 / in-repo 71 条 | issue #56902 |
| workspace 成员 crate | **165** | codex-rs/Cargo.toml |
| `Cargo.lock` 包数 | **1478**（其中 `codex-*` 170 个，外部 1308 个） | codex-rs/Cargo.lock 计数 |
| 典型重依赖存在性 | `reqwest`、`tokio`、`rmcp`、`tonic`+`prost`、`axum`、`hyper`、`rustls`、`tungstenite`、`opentelemetry`、`ratatui`、`image`、`aws-config`、`openssl-sys`、**`v8`** 均在 lock 中 | `Cargo.lock` 名称检索 |
| V8 出现在哪 | `codex-code-mode-runtime` 依赖 `v8`（`v8_enable_sandbox`）+ `deno_core_icudata`；另有 `v8-poc` crate。**`codex-core` 本身只依赖 `codex-code-mode`（gRPC 客户端），不直接依赖 V8**【事实】。但工作区把 V8 当常规构建项【推断：其 CI/发布二进制包含 V8】 | code-mode-runtime/Cargo.toml；core/Cargo.toml |

【推断】构建代价（**未实测**，本机 crates.io 下载不通，见 §7）：把 `codex-core` 作为 git 依赖首次 `cargo build`，预计 **15–45 分钟**（Debug，增量另算）、`target/` **3–6 GB**；若间接带入 V8，单 V8 就要 20–40 分钟。二进制体积增加 **数十 MB 量级**。

### 4.2 版本与 API churn【事实】

- **无 semver**：workspace 版本恒为 `0.0.0`；发布标签是 `rust-v0.16x.y[-alpha.N]`（**0.x 预发布语义**）。
- **发版密度**（GitHub Releases，2026-10-28 抓取最近 10 条）：

| tag | 发布时刻 |
|---|---|
| rust-v0.166.0-alpha.4 | 2026-10-28T01:22:03Z |
| rust-v0.166.0-alpha.3 | 2026-10-27T19:23:16Z |
| rust-v0.166.0-alpha.2 | 2026-10-27T11:54:48Z |
| rust-v0.166.0-alpha.1 | 2026-10-27T01:33:18Z |
| rust-v0.165.0 | 2026-10-27T20:09:38Z |
| rust-v0.164.0 | 2026-10-23T04:48:10Z |

→ **6 天内从 `0.164.0` 走到 `0.166.0-alpha.4`，跨越 3 个 minor 号；单日最多 3 个 alpha**。单个 release 附带 280–355 个构建资产（各平台二进制 + 校验/符号）。
- **已在发生的破坏性变更**（不是推测，是源码里的事实）：
  - `ConversationManager → ThreadManager`、`CodexConversation → CodexThread`、`NewConversation → NewThread`，旧名全部 `#[deprecated]`（[core/src/lib.rs](https://github.com/openai/codex/blob/main/codex-rs/core/src/lib.rs)）；
  - `wire_api = "chat"` **直接删除**（不是 deprecated，是反序列化期 `Err`），指向 [discussion #7782](https://github.com/openai/codex/discussions/7782)；
  - `Op`/`EventMsg` 均 `#[non_exhaustive]`，文档明示变体会持续新增（protocol_v1.md）。
- **没有 embed 契约的稳定性承诺**：issue #56902 open 且 0 回复（§2.4）。

### 4.3 许可与法务

Apache-2.0 ✅ 与我们兼容；复制源码需保留版权/NOTICE、标注修改；专利授权条款对我们有利；商标不可用。**不作为否决项**。

---

## 5. 方案对比与建议

### 方案 A：整包嵌入（`codex-core` git 依赖，进程内）
把 `codex-core` + `codex-core-api` 作为 git 依赖，构造 `ThreadManager`，用 `tool_contributor` 接 Python skill，用 `model_request_contributor` 或 `EventMsg` 做 trace，用 `developer_instructions`/`ContextContributor` 拼 prompt。

- 优点：能力最完整（compaction、多 agent、MCP、skills 全都有）；token delta 与工具钩子都是"一等公民"。
- 致命缺点：
  1. **它拥有 LLM 调用和 turn/循环/停止判定**——我们要"自己拥有 Harness 六阶段"的设计前提被推翻；
  2. 必须 boot 起它的 config/auth/state/thread-store/exec 子系统，还要接受它写 `~/.codex`（rollout JSONL + SQLite）作为第二事实源，与我们的 `rusqlite` 图状态冲突；
  3. 74 个 in-repo crate 全量编译 + `Rust 1.95` + edition 2024 对齐；
  4. 无 semver、周更、`#[deprecated]` 常态、issue #56902 无 roadmap → **永久维护负担**；
  5. `non_exhaustive` 事件枚举 + 我们自己的 `NodeError/状态机` 映射会长期漏分支。
- 【推断】**不建议**。

### 方案 B：app-server 子进程 + JSON-RPC（进程外）
把 `codex` 二进制作为子进程（`codex app-server --listen stdio://`），我们做 JSON-RPC 客户端：
`thread/start`（带 `dynamicTools` = 我们的 Python skill 清单、`developer_instructions`、`sandboxPolicy`）→ `turn/start` → 读 `item/agentMessage/delta`（→ `NodeContext.emit`）/`item/tool/call`（→ `spawn_blocking` 调 Python）/`turn/completed`（→ 节点 output）→ `turn/interrupt` 作停止。

- 优点：**与我们的 tokio 内核天然隔离**（进程边界，Python 阻塞不污染事件循环）；不引入编译期耦合；token delta、自定义工具、instructions 三个注入点都官方存在；许可证干净。
- 缺点：
  1. `dynamicTools` 与 `item/tool/call` 是 **experimental**（官方文档明示需 `capabilities.experimentalApi=true`）；
  2. 仍是**它跑 turn 循环**：上下文压缩、工具可见性、停止判定、`response_id` 续接都在它那边；我们只是"外围调度 + 事件转译"；
  3. 双状态：它的 thread/rollout/SQLite vs 我们的图状态，需要"只把它当无状态执行器"的纪律（每次新 thread，或用 `CODEX_HOME` 指到我们的临时目录）；
  4. 二进制分发/版本钉死/安全更新节奏由对方决定（周更！）；
  5. 延迟与可靠性：多一层进程 + 需处理 overload `-32001` 重试、断线恢复。
- 【推断】若产品必须"短期内跑通一个能用的编码 agent"，这是唯一现实的复用形态；但它是"**把 Codex 包进产品**"，不是"**实现我们的 Harness**"。

### 方案 C：**仅参考**（推荐）
不引入任何 codex crate/二进制，只借它的**契约与实现套路**，自己写（我们本来就要写）：
- **Responses API 客户端 + SSE 解析**：对 DeepSeek 而言就是 `POST /responses` + 解析 `response.output_text.delta` / `reasoning_text.delta` / `output_item.done` / `completed`；`reqwest` + `eventsource-stream`（公开 crate）即可，**不需要 codex-client**（它的 `sse_stream` 也只是包 `eventsource-stream`，见 [codex-client/src/lib.rs](https://github.com/openai/codex/blob/main/codex-rs/codex-client/src/lib.rs)）。
- **事件契约**：直接照抄它"**token delta 与 item 完成分离**"的设计（`OutputTextDelta` / `ToolCallInputDelta` 累积 → `OutputItemDone` 落地），以及 `Op`（提交队列）/`EventMsg`（事件队列）单向双队列范式；落到我们 `NodeContext.emit` 时一一对应即可。
- **tool_call delta 聚合**：`ToolCallInputDelta{item_id, call_id, delta}` 按 `item_id` 累积 JSON 参数、收到 `OutputItemDone(FunctionCall)` 才执行——这个细节很容易踩坑，直接采用它的语义。
- **错误分类**：借 `FunctionCallError` / `ApiError` / `SandboxError` 的**分类维度**做我们的 `NodeError`（不复用类型）。
- 具体可读的参考文件（每个都很小，值得读但要重写）：
  `codex-rs/codex-client/src/sse.rs`、`codex-rs/codex-api/src/sse/`、`codex-rs/codex-api/src/provider.rs`（重试策略）、`codex-rs/codex-api/src/common.rs`（请求/事件类型）、`codex-rs/tools/src/tool_call.rs`。

### 成本预估（人日，含评审与联调；【推断】）

| 方案 | 首次集成 | 一次性 | 持续（每月） | 风险 |
|---|---|---|---|---|
| A 整包 git 依赖 | toolchain/构建 2–4；boot(config/auth/state) 2–3；tool_contributor→Python 4–6；trace/delta 3–5；prompt 控制 5–8；停止/输出映射 3–5；错误映射 3–5 | **25–40 人日** | **1–2 人日** + 每次升级不确定 | **高**（无 embedding 契约、无 roadmap） |
| B app-server 子进程 | JSON-RPC 客户端 2–3；dynamicTools→Python 3–5；delta→trace 2–3；turn/输出/中断 2–3；打包/版本/配置 2–4；状态隔离 1–2 | **12–20 人日** | **0.5–1 人日** | **中**（experimental 字段、周更、双状态） |
| C 仅参考自研 | Responses 客户端+SSE 3–5（本就属于内核工作） | **3–5 人日** | 0（自己控制） | **低** |
| D vendor 小模块（`codex-client`/`codex-api`） | 依赖手术 + 适配 5–10，且 `codex-api` 会拖入 `codex-protocol`（36 个依赖，含 landlock/seccompiler/icu/ts-rs/gix-url） | 5–10 人日 | 0.5–1 | 中高，**收益低**（`eventsource-stream` 本就公开） |

> **结论**：选 **C**。理由：三处注入点它确实都有（技术上"可复用"），但**复用换不来设计目标**——我们的 Harness 必须拥有"组装上下文 / 调 LLM / 工具循环 / 停止判定"，而 Codex 的设计前提恰恰是"这些归引擎，宿主只能观察和追加"。为一份我们要重写的东西引入 74 个 crate + 周更依赖，是负收益。
> 如果**产品节奏**要求 2 周内有一个能跑的编码 agent，再考虑 **B**，并明确它是"外挂 Codex"，不进入内核架构。

---

## 6. 如果仍要复用：最小风险路径（B 的落地要点）

1. 只走 **app-server**（官方文档明示："Use it when you want a deep integration inside your own product: authentication, conversation history, approvals, and streamed agent events"——[文档](https://developers.openai.com/codex/app-server/)）。
2. 二进制**钉死版本**（例如 `rust-v0.165.0`）+ 校验和；CI 里做"升级即跑契约测试"。
3. `CODEX_HOME` 指向我们自己的临时目录，**关闭**其 rollout/state 持久化语义，避免第二事实源；每次执行用新 `thread/start`。
4. `turn/start` 带 `settings.sandboxPolicy = danger-full-access`（我们 v1 不需要沙箱）、`settings.developer_instructions` = 我们的 system prompt、`cwd` 指向工作区。
5. 工具：`thread/start.dynamicTools`（experimental）注册我们的 `ToolDef`；收到 `item/tool/call` 后 `spawn_blocking` 调 Python skill，回 `contentItems`。
6. trace：`item/agentMessage/delta` → `NodeContext.emit(kind="token")`；`item/started|completed` → 节点事件；`turn/completed` → 节点 output。
7. 错误：JSON-RPC `error.code`（如 `-32602/-32603/-32001`）+ `turn/completed.status` → 我们的 `NodeError` 映射表；`-32001` 需指数退避重试。
8. 必须有 fallback：一旦 `dynamicTools` 被移除或改协议，立刻切回自研（这正是选 B 时要把适配层做成薄层的原因）。

---

## 7. 限制与未验证项（诚实清单）

1. **未做真实构建**：本机 crates.io 直连下载超时，故没有 `cargo tree`、没有实测编译时间/体积/可行性。【A 的构建代价、二进制体积均为推断】。要证实只需在一台有网的机器上 `cargo add --git https://github.com/openai/codex codex-core --rev <sha>` 后 `cargo build`。
2. **未实测 DeepSeek `/responses` 与 codex 的兼容性**：Codex 会发 `include`/`prompt_cache_key`/`client_metadata`/`service_tier`/`stream_options`/`access_programs`，DeepSeek 文档未列这些参数 → 是否 400 未知。【推断】大概率忽略，但必须以一次真实请求验证。
3. **未读全量源码**：`core/src/` 只读了 `lib.rs`、`client_common.rs`、`codex_thread.rs`（部分）、`spawn.rs`；`protocol.rs` 24 万字节只 grep 关键符号；`core-api`、`tools`、`extension-api`、`model-provider-info` 读了要点。**"能否真正接管 prompt 组装"这一条是基于公开 trait 签名的判断，未做 PoC** —— 若要选 A/B，建议先花 1 人日做 spike：注册一个 `ContextContributor` + `ToolContributor`，打印最终发往模型的 `input` 与 `base_instructions`，确认可覆盖范围。
4. **release 数字是抓取时的快照**（最近 10 条），不是完整版本史。
5. 我**没有**读 `code-mode-host` / `plugin-host` / `vendor/` 的细节；若未来考虑 code-mode（V8 沙箱执行）路线，需另做调研。

---

## 8. 证据索引（一次性核对用）

| # | 事实 | URL |
|---|---|---|
| 1 | License / stars / pushed_at | https://api.github.com/repos/openai/codex |
| 2 | LICENSE 全文（Apache-2.0） | https://github.com/openai/codex/blob/main/LICENSE |
| 3 | README（Apache-2.0 声明、分发渠道） | https://github.com/openai/codex/blob/main/README.md |
| 4 | workspace：165 成员、version 0.0.0、edition 2024 | https://github.com/openai/codex/blob/main/codex-rs/Cargo.toml |
| 5 | toolchain 1.95.0 | https://github.com/openai/codex/blob/main/codex-rs/rust-toolchain.toml |
| 6 | `codex-core` 115 依赖（74 in-repo） | https://github.com/openai/codex/blob/main/codex-rs/core/Cargo.toml |
| 7 | `core-api` 151-LOC facade | https://github.com/openai/codex/blob/main/codex-rs/core-api/src/lib.rs |
| 8 | 官方嵌入示例（ThreadManager 14 参 + EventMsg 消费） | https://github.com/openai/codex/blob/main/codex-rs/thread-manager-sample/src/main.rs |
| 9 | Op/Event 协议说明（non_exhaustive、任意 UI 宿主） | https://github.com/openai/codex/blob/main/codex-rs/docs/protocol_v1.md |
| 10 | `ResponseEvent` token delta 定义 | https://github.com/openai/codex/blob/main/codex-rs/codex-api/src/common.rs |
| 11 | `EventMsg` delta / DynamicToolCall 定义 | https://github.com/openai/codex/blob/main/codex-rs/protocol/src/protocol.rs |
| 12 | `wire_api="chat"` 已删除 | https://github.com/openai/codex/blob/main/codex-rs/model-provider-info/src/lib.rs |
| 13 | Extension registry（十余类 contributor 注册钩子） | https://github.com/openai/codex/blob/main/codex-rs/ext/extension-api/src/registry.rs |
| 14 | `ToolContributor` / `ToolExecutor` | https://github.com/openai/codex/blob/main/codex-rs/ext/extension-api/src/contributors.rs ・ https://github.com/openai/codex/blob/main/codex-rs/tools/src/tool_executor.rs |
| 15 | `ModelResponseInterceptor`（响应流拦截） | https://github.com/openai/codex/blob/main/codex-rs/ext/extension-api/src/model_request.rs |
| 16 | app-server 官方文档（JSON-RPC / item delta / dynamicTools） | https://developers.openai.com/codex/app-server/ |
| 17 | 社区 embedding 质疑 issue（open，0 评论） | https://github.com/openai/codex/issues/56902 |
| 18 | crates.io 无 codex-core / codex-client | https://crates.io/api/v1/crates/codex-core |
| 19 | `codex-protocol` 是无关同名 crate | https://crates.io/crates/codex-protocol |
| 20 | DeepSeek Responses API（SSE 事件、function tools、无状态） | https://api-docs.deepseek.com/api/create-response/ |
| 21 | DeepSeek OpenAI 兼容 base_url | https://api-docs.deepseek.com/zh-cn/quick_start/model/ |
| 22 | Releases 节奏（0.164→0.166-alpha.4，6 天） | https://api.github.com/repos/openai/codex/releases?per_page=10 |
| 23 | chat wire_api 移除讨论 | https://github.com/openai/codex/discussions/7782 |
