> **归档说明**：Rust 生态/DSH 调研（冻结期归档）。结论已并入 DR-001（transport=genai 等）。

# Rust Agent Harness 开源替代方案调研

> **采样时间**：2026-10-28（Asia/Shanghai）。版本号、下载量、更新时间均为当日 crates.io / GitHub / npm 快照。
> **范围**：为 Rust 内核（tokio + PyO3 + rusqlite，LLM 走 OpenAI-compatible，DeepSeek 官方 API）寻找「LLM transport + SSE 流式 + tool_call delta 聚合」的可复用开源库，并评估 DeepSeek Harness（DSH）的可复用性。
> **方法边界**：只做元数据 / 文档 / 源码只读查询。未添加依赖、未构建、未 clone、未 pip/npm 安装；未启动任何 DSH 服务。`cargo info` 在本机实测会卡在 .crate 下载（>90s 未完成，已中止），因此 crates.io 信息改用 API 查询（带自定义 UA）。**依赖重量 = Cargo.toml 声明的 normal 依赖个数，不是实际解析后的树**。

---

## 速览（给 lead 的一页结论）

**Q1：transport 用 async-openai 还是 reqwest + eventsource-stream 自写？**
→ **优先 `genai` 0.7.x；若只接受 OpenAI-only 则 `async-openai` 0.42.x；最后才是自写。**
- genai：SSE token delta + tool_call delta 已按 `index` 聚合 + `reasoning_content` 事件 + 超时/错误/`retry-after` header 都现成，纯 transport 无 loop（依据：源码 `exec_chat_stream`、`OpenAIStreamer::capture_tool_call`、`Error::HttpError`）。
- async-openai：SSE + 类型齐全 + 自定义 `base_url`，但 tool delta 要自己聚合、无 `reasoning_content`、125 个版本 churn（依据：crates.io 元数据 + 源码 delta 类型）。
- 自写 reqwest + eventsource-stream：可行（约 400–600 LOC），但要自己处理 SSE 分帧/UTF-8 跨块/多 choice/tool index 合并/`[DONE]`/Retry-After，边际收益低（推断，3–5 人日）。

**Q2：tool loop 该复用 rig 吗？**
→ **不该。** rig 的 `AgentRun` 明确接管 turn budget / tool-call validation / history / usage / final response，`Handlers` 接管工具 dispatch；我们只有 ~200–300 LOC 的 loop 需求。且 0.46.0-rc.1 + MSRV 1.97.1 + 32 个声明依赖 + 整个 Bevy coding-agent 仓库快速演进（依据：rig-agent README、rig-core Cargo.toml、crates.io 元数据）。

**Q3：哪些绝对不要复用？**
→ **rig 的 Agent runtime（会吃掉我们的 loop/工具执行）、DSH 运行时嵌入（Node 双进程 + ACP 不给 token 级 delta + 工具在 DSH 内执行）、swiftide（RAG/indexing 平台、采纳低）、llm-chain（2023-11 停更）、kalosm（本地模型方向不符）。** MCP 只在未来真要接工具协议时用官方 `rmcp`（可选 feature），它不能替代 LLM transport。

**DSH 许可（与「未找到公开许可」的结论不同，请复核）：** 我查到**公开仓库** `github.com/deepseek-ai/deepseek-harness`（MIT，TypeScript，263,737★，push 2026-10-23）与**公开 npm 包** `@deepseek-ai/dsh`（MIT，39 个版本，repository 指向同一仓库；`dsh-acp` 等运行时包也在 npm 200 可查）。本机 `dsh/package.json` 的 `private: true` 只说明该 umbrella 包装不从 workspace 发布，**不等于运行时私有**；各 `@deepseek-ai/*` 包内均带标准 MIT LICENSE（`Copyright (c) 2026 DeepSeek`）。复核命令见 §3.1。→ 结论：**MIT 可复用（保留声明）**，但**不适合嵌入**（架构错位），建议作为参考实现，或未来作为进程外 subagent（ACP/JSON-RPC stdio + 工具经 MCP 暴露）。

---

## 0. TL;DR（推荐结论）

| 层 | 结论 |
|---|---|
| LLM transport + SSE + tool_call delta 聚合 | **首选 `genai` 0.7.x**（MIT OR Apache-2.0）。它是纯 transport 库：每个 SSE delta 一个 `Chunk`，tool_call delta 已按 `index` 聚合，`reasoning_content` 单独成事件，超时 / 错误分类 / Retry-After header 都可控；**没有 agent loop、没有工具执行**，正好把上下文组装、工具注册表、trace、错误映射留在我们手里。 |
| 备选 transport | `async-openai` 0.42.x（MIT）。OpenAI 官方 schema 最全、SSE + tool delta 类型齐全，但 **tool_call delta 要自己聚合**、**不建模 `reasoning_content`**、0.x 版本 churn 更严重。 |
| 上下文组装 / tool loop / 停止条件 / trace / 错误映射 | **自己写**（约 500–700 LOC）。不要为这几项引入 rig 的 Agent runtime 或 DSH。 |
| 不要复用 | `rig`（全栈 agent 框架，接管 loop，0.46-rc + MSRV 1.97.1）、`swiftide`（RAG/indexing 平台，方向不符、采纳低）、`llm-chain`（2023-11 起停更）、`kalosm`（本地模型/多模态，方向不符）。 |
| MCP（未来） | 用官方 `rmcp`（Apache-2.0，活跃）。MCP 是工具协议，不能替代 LLM transport。 |
| DSH | **已开源**：`github.com/deepseek-ai/deepseek-harness`，**MIT**，TypeScript/Node + Electron，developer preview。**MIT 允许读码/移植/商用**（保留版权声明）。但它只能通过 ACP / JSON-RPC stdio / headless 被 Rust **当作进程外服务**驱动；loop、工具执行、上下文、流式语义都在 Node 侧，且 ACP 明确不传 raw provider delta。**不建议嵌入 Rust 内核**，建议作为参考实现。 |

**一句话**：transport 用 `genai`，harness 自己写；DSH 是「参考实现 / 外部 subagent 服务」，不是「可嵌入的薄库」。这与任务里「自己写 ~1.2k LOC vs 复用」的前提一致——可复用的只有 transport，约省 3–5 人日（推断）。

---

## 1. 需求 → 能力映射（谁该负责什么）

| Harness 职责 | genai | async-openai | rig | DSH | 我们的选择 |
|---|---|---|---|---|---|
| AgentConfig 加载（role_prompt/constraints 文件） | ✗ | ✗ | AgentBuilder（代码内配置，无文件加载约定） | 插件化 YAML 配置 | 自己写（薄） |
| 上下文组装（system + 约束 + KB + 记忆 + 用户输入） | ✗ | ✗ | 有 memory / prompt hooks（框架内） | 有 system-prompt / compaction 插件 | **自己写**（核心逻辑） |
| LLM 调用 | ✓ | ✓ | ✓ | ✓（内部） | 复用 transport |
| Token 级 SSE 流式 | ✓ 每 delta 一个 `Chunk`（另有 `ReasoningChunk`/`Heartbeat`） | ✓ SSE parsed stream | ✓ | 内部有，但 ACP 不暴露 raw delta | **复用 genai** |
| tool_call delta 聚合 | ✓ 内置按 `index` 合并 arguments | △ 只给类型，需自己聚合 | ✓ 框架内完成 | ✓ 内部 | **复用 genai** |
| tool_call 循环 / 工具执行 | ✗（设计上不提供） | ✗ | ✓ AgentRun + Handlers | ✓ 自己的 tool registry/sandbox | **自己写**（工具走自有注册表） |
| 停止条件（max_iterations / 超时 / 人工） | △ 只有流式 header/read 超时 | △ 通用 HTTP 超时/中间件 | △ turn budget 在框架内 | 有 timeout / compaction / goal 等插件 | **自己写** |
| 重试 / 退避 | ✗ 无 retry 模块（但暴露 `retry-after` header） | △ 可选 tower middleware（`SimpleRetryPolicy`，feature `middleware`） | 框架内有 | ✓ `dsh-llm-retry` | **自己写**（~80 LOC）或 tower |
| 错误 → 领域错误映射 | ✓ 分类很细的 `Error` enum | ✓ `OpenAIError` | ✓ | ✓ | 自己映射（薄） |
| trace / 观测 | △ `tracing` + `raw_frame_sink` 原始帧旁路 | △ `tracing` + tower 中间件 | ✓ hooks | ✓ session log/OTel | 自己写（trace 是我们的） |

**结论**：genai / async-openai 只覆盖第 3–5 行，正好是「薄」的部分；rig / DSH 覆盖太多，会把 loop 和工具执行从我们手里拿走。

---

## 2. 候选详评

### 2.1 `genai`（首选 transport）

**事实（crates.io / GitHub / 源码，2026-10-28）**
- 许可：`MIT OR Apache-2.0`（crates.io 字段）；GitHub license 检测为 Apache-2.0（双许可，商用/闭源分发 OK）。
- 版本：`0.7.2`，发布于 **2026-10-28**（当天）；共 154 个版本；总下载 535,498，近 90 天 250,510；GitHub 920★，仓库最后 push 2026-10-28。
- 依赖：21 个 normal 声明；`aws-*` 仅在 `bedrock-sigv4` feature 下引入；默认 features = `rustls-tls` + `system-proxy`；HTTP 栈是 **reqwest 0.13**；`edition = "2024"`（无 `rust-version` 声明，⇒ 至少 Rust 1.85）。
- 流式 API：`Client::exec_chat_stream(model, ChatRequest, Option<&ChatOptions>)` → `ChatStream`；事件枚举 `ChatStreamEvent::{Start, Chunk, ReasoningChunk, ToolCallChunk, ThoughtSignatureChunk, Heartbeat, End}`。
- tool_call delta 聚合：`OpenAIStreamer::capture_tool_call(index, call_id, fn_name, arguments)` 按 `index` 合并 arguments 字符串，并用 `capture_tool_calls` 在 `StreamEnd.captured_tool_calls` 里交付完整调用；`ToolCallChunk` 事件里给的是「已累积到当前时刻」的调用。
- 选项（`ChatOptions`）：`capture_content/usage/tool_calls/reasoning_content`、`stream_header_timeout`、`stream_read_timeout`、`extra_headers`、`extra_body`、`tool_choice`、`reasoning_effort`、`raw_frame_sink`（原始帧旁路，便于自建 trace/审计）。
- 错误：`Error` enum 分类细（`WebAdapterCall` / `WebModelCall` / `ChatResponse` / `StreamParse` / `WebStream` / `HttpError` / `Resolver` / `RequiresApiKey` / `ModelMapperFailed`…）；`HttpError { status, canonical_reason, body, headers }`，注释明确 header 里带 `retry-after` / `retry-after-ms` / `x-should-retry`，**供下游重试层使用**。
- 端点适配：DeepSeek 是内置 adapter（README 把 DeepSeek 列为 OpenAI-compatible 适配）；任意 OpenAI-compatible 端点可用 `genai_{n}::model` + `GENAI_{n}_ENDPOINT`/`GENAI_{n}_API_KEY`，或用 `ServiceTargetResolver` 自定义 endpoint/auth/model（README 明确支持）。`deepseek-*` 模型名默认路由到 DeepSeek adapter。
- 定位：纯 chat/stream 库；**没有 agent loop、没有工具执行、没有记忆/RAG**（这正是我们要的）。

**适配度**：★★★★★（transport 层）。`exec_chat_stream` + `Chunk`/`ToolCallChunk`/`End` 正好对应「token 回调 + tool_call 聚合 + 收尾用量」。

**风险 / 注意**
- 0.x，且 `0.7.x` 刚发生 API 变化（CHANGELOG 记录 `ChatOptions` 新增字段会让 struct literal 编译失败）——建议用 `..Default::default()` 并锁 minor 版本。
- **无内建重试**：需要我们在 `HttpError`（429/5xx/Retry-After）与 `webc::Error`（连接/超时）之上自研。
- `reqwest 0.13` 与内核现有依赖是否冲突未知（未构建）。
- GitHub license 显示 Apache-2.0、crates.io 显示双许可；两处都允许商用，但归档/合规材料建议以 crates.io + 仓库 LICENSE 文件为准。

**集成草图形状（示意，非可编译代码）**

```rust
use futures::StreamExt;
use genai::chat::{ChatOptions, ChatRequest, ChatStreamEvent};
use genai::Client;

let client = Client::builder().build(); // 端点/鉴权可用 ServiceTargetResolver 或 genai_1:: 命名空间注入
let req = ChatRequest::new(history).with_tools(tool_schemas);
let opts = ChatOptions::default()
    .with_capture_tool_calls(true)
    .with_capture_usage(true)
    .with_stream_header_timeout(Duration::from_secs(15))
    .with_stream_read_timeout(Duration::from_secs(60));

let mut stream = client.exec_chat_stream("deepseek-chat", req, Some(&opts)).await?;
while let Some(ev) = stream.stream.next().await {
    match ev? {
        ChatStreamEvent::Chunk(c) => on_token(&c.content),          // token 级回调
        ChatStreamEvent::ReasoningChunk(c) => on_reasoning(&c.content),
        ChatStreamEvent::ToolCallChunk(tc) => { /* tc.tool_call 已聚合 */ }
        ChatStreamEvent::End(end) => { /* end.captured_usage / captured_tool_calls / stop_reason */ }
        _ => {}
    }
}
```

---

### 2.2 `async-openai`（备选 transport）

**事实**
- 许可：`MIT`；仓库 2,022★，最后 push 2026-10-21。
- 版本：`0.42.2`（2026-10-10）；共 **125 个版本 / 43 个 0.x.y 系列**；总下载 9,909,198，近 90 天 3,110,483。
- 依赖：25 个 normal 声明（大量 optional）；默认 features 只有 `rustls`（不启用任何 API），要显式开 `chat-completion`；HTTP 栈 reqwest 0.13；MSRV 1.75。
- 流式：`Chat::create_stream(request)` 返回 `ChatCompletionResponseStream`（内部解析 SSE，自动置 `stream: true`）。
- 类型：`ChatCompletionStreamResponseDelta { content, tool_calls: Option<Vec<ChatCompletionMessageToolCallChunk>>, role, refusal }`；chunk 含 `index / id / type / function{name, arguments}` —— **只给增量，聚合要自己按 `index` 做**。
- 端点：`OpenAIConfig::new().with_api_base("...")`、环境变量 `OPENAI_BASE_URL`，任意 OpenAI-compatible base_url 可用。
- 重试：源码里 retry 能力在 **`middleware` feature 的 tower 中间件**（`SimpleRetryPolicy`）中，测试覆盖 429 重试；默认 client 不启用（README 宣称「rate limited 时指数退避」，与源码默认路径不一致，**上线前要实测**）。
- **`reasoning_content` 未建模**：delta 结构里没有该字段（DeepSeek reasoner 的思考流会被忽略）。crates.io 上存在 `async-openai-thinking`（0.41.5）这类 fork，说明这是已知缺口。
- `byot` feature 可自带 Request/Response 类型，自定义非标字段。

**适配度**：★★★★（transport 层）。若团队只做 OpenAI 官方 schema、想要更「官方」的类型，可选；但要自己写聚合与 reasoning 处理。
**风险**：0.x 125 个版本，API churn 比 genai 更明显；tool delta 聚合、reasoning 缺失、默认无重试都要自担。

---

### 2.3 `rig` / `rig-core`（全栈 agent 框架，不适合「薄」）

**事实**
- 许可 MIT；`0.46.0-rc.1`（2026-10-23）；MSRV **1.97.1**；总下载 3,864,412，近 90 天 1,959,366；GitHub 8,997★，最后 push 2026-10-27；32 个 normal 依赖（`rig-http`/`rig-reqwest`/`rig-tungstenite`/`schemars`/`async-stream`，`epub`/`lopdf`/`quick-xml` 可选）。
- 仓库已演进为 workspace：`rig-core`（provider-neutral 模型/补全/工具契约）、`rig-agent`（AgentBuilder、prompting/streaming、typed hooks、tool registry、sans-IO `AgentRun`）、`rig-tools`、`rig-ecs`/`rig-harness`、以及基于 Bevy 的 coding agent。
- `rig-agent` 的 `AgentRun` 明确拥有 loop 的**所有决策**：turn budget、tool-call validation/recovery、history threading、structured-output policy、usage accounting、final response；`Handlers` 表负责工具 dispatch，`hooks` 可拦截。
- custom base_url：OpenAI provider 以 OpenAI-shaped `Route`/dialect 配置自定义端点；**确切 builder API 未在本环境核实（GitHub API 限流）**，以 docs.rs 0.46 为准。

**适配度**：★★（如果目标就是「用一套 Rust agent 框架」）；对「薄 harness」只有 ★。用了它 = 把 loop、工具执行、history 交给 rig；仅拿 `rig-core` 做 transport 又要绕开它的 agent 抽象，收益不划算。
**风险**：0.46-rc（API 不稳）、MSRV 1.97.1（高于常见 CI 基线）、依赖重、上游快速演进（crates + Bevy coding agent 双线）。集成成本 3–5 人日且存在「试完放弃」的沉没成本（推断）。

---

### 2.4 `swiftide`（RAG/indexing 平台，不复用）

- MIT；`0.33.0`（2026-10-14），但上一版 `0.32.1` 是 2025-11-15（**约 11 个月空档**）；85,536 总下载、近 90 天仅 2,382；796★。
- 14 个 normal 依赖全是自家子 crate（`swiftide-agents`/`core`/`indexing`/`integrations`/`query`/`tasks`/`langfuse`），实际是「可组合 LLM agents + typed task graphs + streaming RAG pipelines」平台。
- 不贴合：我们不需要它的索引/向量库/管道/集成；KB 与记忆是我们自己的上下文组装。低采纳也放大了维护风险。

### 2.5 `llm-chain`（停更，不复用）

- MIT；最新 `0.13.0` 发布于 **2023-11-15**，近 3 年无新版本；101,003 总下载、近 90 天 8,312（历史存量）。
- chain/prompt 模板时代的产物，没有现代 OpenAI-compatible SSE tool-call 流式抽象。供应链与兼容性风险高。

### 2.6 `kalosm`（本地模型，方向不符）

- `MIT/Apache-2.0`；`0.4.0` 发布于 2025-02-09（约 1.7 年无更新）；25,428 总下载、近 90 天 2,166。
- 定位「pretrained AI models 的简单接口」，偏本地模型/多模态（llama、whisper、OCR 等），不是 OpenAI-compatible transport。方向不符。

### 2.7 `rmcp`（MCP 工具协议，按需引入）

- `Apache-2.0`；`3.8.0`（2026-10-26）；71 个版本；总下载 41,172,383，近 90 天 23,148,923；GitHub 4,061★；MSRV 1.88；仓库 `modelcontextprotocol/rust-sdk`，README 自称 **official Rust MCP SDK**，tokio async。
- 能力：client（`ServiceExt::serve` + `TokioChildProcess` stdio + streamable HTTP 等 transport）+ server + `rmcp-macros`；实现 MCP `2026-07-28` 规范并兼容旧版。
- 37 个 normal 依赖（hyper/reqwest/schemars/oauth2…），较重 → 建议只在我们真要接 MCP 工具时开可选 feature（client + child-process transport）。
- **注意**：MCP 是「工具调用协议」，不是 LLM transport，不能替代 genai/async-openai；两者是互补关系。

---

## 3. DeepSeek Harness（DSH）现状：是否可合法复用

### 3.1 是否开源 / 许可（结论：**是，MIT**）

- **公开仓库**：`https://github.com/deepseek-ai/deepseek-harness`（GitHub API：`license.spdx_id = MIT`，README 明写 `## License [MIT]`）。263,722★ / 31,898 forks；创建 2026-08-13，最后 push 2026-10-23；主语言 TypeScript；**developer preview**，README 明确「THERE WILL BE COMPATIBILITY-BREAKING CHANGES」，并指向 `THIRD_PARTY_NOTICES.md`。
- **npm 发布**：`@deepseek-ai/dsh`（MIT，39 个版本；`latest=0.2.1-rc.2`、`next=0.2.1-rc.4`；repository 指向同一 GitHub repo）。`npx @deepseek-ai/dsh web` 可跑。
- **本机安装包（只读核查）**：`/Applications/DeepSeek Harness.app`，`Info.plist` 版本 `0.2.1-rc.3`；Electron；`app.asar` **174 MB / 18,191 个文件**；内嵌 Node **24.21.0**；`@deepseek-ai/*` 每个包内 `LICENSE` 均为标准 MIT 文本（`Copyright (c) 2026 DeepSeek`）；顶层 `dsh/package.json` 为 `@deepseek-ai/dsh-desktop-runtime`（`private: true`，版本 0.2.1-rc.3）。
- **因此：不存在「未找到公开许可」**。DSH 可合法复用：MIT 允许使用、修改、分发、商用、闭源衍生，只需保留版权与许可声明；第三方依赖按其各自许可（仓库有 THIRD_PARTY_NOTICES）。**把 TS 代码移植成 Rust 也属于 MIT 允许的衍生作品**（保留 notice 即可）。但注意：本报告仅是技术调研，不构成法律意见。

> **证据复核（约 30 秒，可自行重跑）**
> ```bash
> curl -s -H "User-Agent: zgraph-eval/1.0" https://api.github.com/repos/deepseek-ai/deepseek-harness \
>   | python3 -c "import json,sys;d=json.load(sys.stdin);print(d['full_name'],d['license']['spdx_id'],d['language'],d['stargazers_count'],d['pushed_at'])"
> # → deepseek-ai/deepseek-harness MIT TypeScript 263737 2026-10-23T15:25:40Z
>
> curl -s -H "User-Agent: zgraph-eval/1.0" https://registry.npmjs.org/@deepseek-ai%2Fdsh \
>   | python3 -c "import json,sys;d=json.load(sys.stdin);print(d['name'],d['license'],len(d['versions']),d['repository']['url'])"
> # → @deepseek-ai/dsh MIT 39 git+https://github.com/deepseek-ai/deepseek-harness.git
>
> curl -s -o /dev/null -w "%{http_code}\n" -H "User-Agent: zgraph-eval/1.0" https://registry.npmjs.org/@deepseek-ai%2Fdsh-acp   # → 200
> ```
> 关于「运行时是否私有」：本机 `dsh/package.json` 是桌面 runtime 的 umbrella 包（`private: true`），仅表示「不从该 workspace 发布」；npm registry 上确实存在 `@deepseek-ai/dsh` 及其运行时子包（如 `dsh-acp`），license 字段为 MIT。若团队合规要求以正式扫描为准，建议核对仓库根 `LICENSE` 与 `THIRD_PARTY_NOTICES.md`，而不是只看 `private` 标志。

### 3.2 技术栈与架构

- TypeScript + Node 24 + Electron + **Cordis 插件架构**（「Everything is a Plugin」）。
- 关键包/职责（来自 `dsh/package.json` 依赖列表与 package README）：`dsh-agent-loop`（驱动每个 turn：model request → streamed response → tool execution → durable session history；`maxParallelToolCalls` 并发/顺序策略）、`dsh-llm` + `dsh-llm-deepseek` + `dsh-llm-retry`（LLM 适配与重试）、`dsh-timeout`、`dsh-tool-call-timeout-policy`、`dsh-compaction`、`dsh-token-meter`、`dsh-session*`（持久化/JSONL/SQLite）、`dsh-tools`/`dsh-tool-*`、`dsh-sandbox`、`dsh-mcp-client`、`dsh-acp`、`dsh-sdk-jsonrpc-server`、`dsh-api-gateway`、`dsh-web*`。
- 这正是一个「完整 harness」：比我们需要的多得多（记忆、RAG、UI、沙箱、工作流、subagent、goal、插件市场……）。

### 3.3 能否作为外部服务被 Rust 调用（可以，但代价/限制明确）

**可行的进程外接口（文档事实）：**
1. **ACP v1 over JSON-RPC stdio**：`dsh --profile acp` 启动，客户端可 `initialize` / `session/new|list|resume|close` / `session/prompt` / `session/cancel`，接收 `session/update`、`session/request_permission`；无鉴权；支持挂 MCP server（stdio/HTTP）。
2. **SDK wire protocol over stdio**：`@deepseek-ai/dsh-sdk-jsonrpc-server` ——「serves the SDK wire protocol over stdio so out-of-process clients can drive harness agents」，`initialize` 握手返回 `deepseek-harness-sdk-runtime`，之后 `session/prompt`、`session.event`、`session.status`、`session/wait`、`shutdown`。
3. **one-shot headless**：`dsh --profile headless "task"`（`--json` 输出 NDJSON 事件；退出码 0/1；reasoning delta 打到 stderr，最终答案 stdout）。
4. **Web/HTTP**：`npx @deepseek-ai/dsh web` 默认 `http://127.0.0.1:3080`（本会话 GUI 是 `127.0.0.1:62847`，端口可配）；这是 UI 后端，不是稳定公开 API。

**关键限制：**
- ACP 设计为「automation-only」：**只上线标准语义事件（committed assistant messages / thoughts / tool lifecycle / config / context usage），raw provider deltas、retry attempts、DSH 展示数据都不上线**（README 原话）。**拿不到 token 级 provider delta**——这与我们「token 级 SSE 流式」的需求直接冲突。
- 工具执行在 DSH 进程内（它自己的 tool registry + sandbox）；Rust 侧的工具只能以 **MCP server** 形式挂进去，**不能把它接到我们自有的 `ToolDef` 注册表并让 Rust 执行**（除非再桥一层）。
- 双进程（Rust + Node 24）+ 174MB app + preview 期破坏性变更；引入的是「第二个 harness」，不是「一个 transport 库」。

### 3.4 DSH 复用结论

| 用法 | 可行性 | 结论 |
|---|---|---|
| 作为 Rust 内核的 transport / loop 库 | ✗ 架构错位、无 Rust crate、token 级 delta 不暴露 | **不要** |
| 作为参考实现（读码/移植设计） | ✓ MIT；agent-loop 的 turn/step 状态机、tool 并发/顺序、超时策略、错误分类、compaction/token meter、session 持久化都值得借鉴 | **推荐**（保留版权声明） |
| 作为进程外 subagent 服务（ACP/JSON-RPC stdio），工具经 MCP 暴露 | ✓ 法律与协议上都可行；但要求我们的工具先做成 MCP server，且流式为语义事件 | **可选/后置**，仅当未来需要 DSH 的 UI/多 agent/沙箱/插件生态时 |

---

## 4. 对比表（2026-10-28 快照）

| 候选 | 语言 / Rust 可依赖 | 许可 | 流式（token 级） | tool_call delta 聚合 | tool loop | 自定义工具 | 声明依赖数 | 维护活跃 | 集成成本（人日，推断） | 主要风险 |
|---|---|---|---|---|---|---|---|---|---|---|
| **genai 0.7.2** | Rust crate ✓ | MIT OR Apache-2.0 | ✓ 每 delta 一个 `Chunk`；reasoning 独立 | ✓ 内置按 index 聚合 | ✗（有意不提供） | ✓ `Tool` schema + `ToolResponse` 手工 | 21（AWS 可选） | 0.7.2 2026-10-28；250k/90d；920★；push 10-28 | **2–3** | 0.x 大改（0.7 刚变）；无 retry；reqwest 0.13 冲突未知 |
| **async-openai 0.42.2** | Rust crate ✓ | MIT | ✓ SSE chunk stream | △ 只给类型，需自聚合 | ✗ | ✓ 类型 + `byot` | 25（多可选） | 0.42.2 2026-10-10；3.1M/90d；2,022★ | 2–3 | 125 版本 churn；无 `reasoning_content`；默认无重试 |
| **rig / rig-core 0.46.0-rc.1** | Rust crate ✓ | MIT | ✓ | ✓（框架内） | ✓ AgentRun + Handlers | ✓ Tool trait / Handlers | 32（+workspace 多 crate） | 近 90 天 1.96M 下载；8,997★；push 10-27 | 3–5（不建议） | 0.46-rc API 不稳；MSRV 1.97.1；接管 loop；依赖重 |
| **swiftide 0.33.0** | Rust crate ✓ | MIT | △ 平台内 | 框架内 | ✓（agents） | ✓ | 14（自家子 crate 全家桶） | 0.33.0 2026-10-14，但距上版 11 个月；2.4k/90d；796★ | 5+（不建议） | RAG/indexing 平台方向不符；采纳低 |
| **llm-chain 0.13.0** | Rust crate ✓ | MIT | ✗ 旧式 chain | ✗ | 部分 | ✗ | — | **2023-11-15 后再无发布**；8.3k/90d | 不建议 | 停更 ~3 年 |
| **kalosm 0.4.0** | Rust crate ✓ | MIT/Apache-2.0 | ✗ 本地推理为主 | n/a | 部分 | n/a | — | 2025-02-09；2.2k/90d | 不建议 | 方向不符；1.7 年无更新 |
| **rmcp 3.8.0** | Rust crate ✓ | Apache-2.0 | n/a（工具协议） | n/a | n/a | ✓ MCP 工具 | 37 | 3.8.0 2026-10-26；23M/90d；4,061★ | 1–2（未来接 MCP 时） | 不是 LLM transport；依赖较重 |
| **DSH（进程外/参考）** | TypeScript/Node（Rust 只能进程外） | **MIT** | ✗ ACP 只传语义事件/committed messages；headless 只 reasoning→stderr | DSH 内部完成，不暴露 | ✓ 内置 | 经 MCP 挂载；不能用 Rust 注册表 | Node 24 + 174MB app（18,191 文件） | push 2026-10-23；263,722★；developer preview | 5–10（不建议嵌入） | 双进程；preview 破坏性变更；架构错位；工具执行不在我们手里 |

> 「近 90 天下载」= crates.io `recent_downloads`；「人日」是基于 API 面积和已有 spike 经验的推断，非实测。

---

## 5. 推荐组合

### 方案 A（推荐）：`genai` transport + 自研 harness

```
Rust 内核
├── harness/
│   ├── config.rs      AgentConfig 加载（role_prompt/constraints 文件）
│   ├── context.rs     system + 约束 + KB + 记忆 + 用户输入 组装
│   ├── loop.rs        max_iterations / 超时 / 人工停止；tool_call 循环
│   ├── trace.rs       step 级 trace（token、tool_call、usage、耗时）
│   └── error.rs       HarnessError 映射
├── llm/
│   ├── transport.rs   genai Client（endpoint/auth 注入）
│   ├── stream.rs      Chunk → on_token；ToolCallChunk → 聚合；End → usage/stop_reason
│   └── retry.rs       429/5xx/连接超时 + Retry-After（用 genai Error::HttpError.headers）
└── tools/
    └── registry.rs    自有 ToolDef 注册表（执行留在 Rust）
```

- **端点**：优先用 genai 的 DeepSeek adapter（`deepseek-chat` 等）；若走任意 OpenAI-compatible base_url，用 `ServiceTargetResolver` 或 `genai_1::` 命名空间（`GENAI_1_ENDPOINT` / `GENAI_1_API_KEY`）。
- **流式回调**：`ChatStreamEvent::Chunk` 直接喂 token 回调；`ReasoningChunk` 单独走 thinking 通道；`End.captured_usage` 记 token 计量。
- **tool_call 聚合**：开 `capture_tool_calls(true)`；流中 `ToolCallChunk` 可用于「工具名早显示」，最终以 `StreamEnd.captured_tool_calls` 为准做参数 JSON 解析（先校验再执行，避免半截 JSON）。
- **停止/重试**：loop 自己判断 `max_iterations` / wall-clock 超时 / 人工取消（`tokio::select!` + `CancellationToken`）；重试用 `Error::HttpError { status, headers }` 分类，尊重 `retry-after`，连接/流中断重试前要判断「是否已向用户吐 token」（通常不重试已开始输出的流，或只重试建立阶段）。
- **MCP**：暂不引入；未来需要时把 `rmcp` 作为可选 feature（`client` + `transport-child-process`），把 MCP 工具适配进自有 `ToolDef` 注册表。

**预期成本（推断）**：transport + 流 + 聚合适配 ~2–3 人日（含错误映射/回归）；harness（配置/上下文/loop/trace/停止）~3–4 人日；合计 ~6 人日，比纯自研 transport（~3–5 人日）省一半左右，且保留全部控制权。

### 方案 B（备选）：`async-openai` + 自研聚合/loop

适用条件：只做 OpenAI 官方 schema、需要 `byot` 自定义类型、或团队更信任该 crate 的成熟度（9.9M 下载）。
代价：按 `index` 自写 tool delta 聚合（~150 LOC + 测试）；`reasoning_content` / `reasoning_effort` 需自行处理（DeepSeek reasoner 场景要额外字段或 fork）；版本 churn 更大（125 版本）。成本与 A 接近或略高。

### 方案 C（不推荐）

- **rig 作为整套 runtime**：会把 turn budget / tool dispatch / history / hooks 收走，与「tool loop 在自己手里」冲突；0.46-rc + MSRV 1.97.1 增加升级成本。
- **DSH 嵌入**：Node 双进程 + ACP 语义事件粒度 + 工具执行在 DSH，等于把内核的 harness 搬到 Node；只有在「我们愿意放弃自研 loop、接受 DSH 全家桶」时才考虑。

---

## 6. 待验证清单（本次环境无法验证）

| # | 事项 | 原因 | 建议验证方式 |
|---|---|---|---|
| 1 | genai 对 DeepSeek 官方 endpoint 的流式 tool_call / reasoning 实际行为 | 未构建、未发真实请求 | 在内核 spike 里用测试 key 跑 3 个 case：纯文本流、单 tool call、多 tool call 并行 |
| 2 | genai `ServiceTargetResolver` 0.7.2 的确切签名 | 读的是 main（0.7.3-dev），GitHub API 限流 | `docs.rs/genai/0.7.2` + 源码 examples/c06 |
| 3 | reqwest 0.13 与内核现有依赖是否冲突 | 未构建 | `cargo tree -d`（在内核里） |
| 4 | async-openai 默认是否重试（README 与源码不一致） | 源码检索只看到 middleware feature 测试 | 读 0.42.2 源码 `client.rs` 的 `post_stream` + 实测 429 |
| 5 | rig 自定义 OpenAI base_url 的 builder API | GitHub API 限流，未取到 client 源码 | docs.rs/rig-core 0.46 + 仓库 `crates/rig-core/src/client/openai*` |
| 6 | DSH ACP `session/update` 的实际粒度（是否 per-block） | 未启动 DSH 服务（避免改动环境） | 另起端口跑 `dsh --profile acp` 用测试脚本观测；文档已明确 raw provider delta 不上线 |
| 7 | 依赖树实际大小 | 未生成 Cargo.lock | 内核 spike 中 `cargo tree` 统计 |

---

## 7. 参考来源

**crates.io（元数据 API，2026-10-28）**
- async-openai：https://crates.io/crates/async-openai ｜ API：https://crates.io/api/v1/crates/async-openai
- genai：https://crates.io/crates/genai
- rig-core：https://crates.io/crates/rig-core ｜ rig：https://crates.io/crates/rig
- swiftide：https://crates.io/crates/swiftide
- llm-chain：https://crates.io/crates/llm-chain
- kalosm：https://crates.io/crates/kalosm
- rmcp：https://crates.io/crates/rmcp
- `async-openai-thinking`（fork，佐证 reasoning_content 缺口）：https://crates.io/crates/async-openai-thinking

**GitHub（仓库/README/源码，2026-10-28）**
- async-openai：https://github.com/64bit/async-openai
- genai：https://github.com/jeremychone/rust-genai
- rig：https://github.com/0xPlaygrounds/rig
- swiftide：https://github.com/bosun-ai/swiftide
- llm-chain：https://github.com/sobelio/llm-chain
- kalosm：https://github.com/floneum/floneum
- rmcp：https://github.com/modelcontextprotocol/rust-sdk
- **DSH**：https://github.com/deepseek-ai/deepseek-harness（MIT；README/SAFETY.md/THIRD_PARTY_NOTICES.md）

**npm（DSH 发布物）**
- https://www.npmjs.com/package/@deepseek-ai/dsh（MIT，39 个版本，repository 指向 GitHub）

**docs.rs**
- https://docs.rs/genai ｜ https://docs.rs/async-openai ｜ https://docs.rs/rig-core ｜ https://docs.rs/rmcp

**本机只读证据**
- `/Applications/DeepSeek Harness.app/Contents/Info.plist`（版本 0.2.1-rc.3）
- `app.asar` 内 `dsh/package.json`、`@deepseek-ai/dsh-agent-loop/README.md`、`dsh-acp/README.md`、`dsh-sdk-jsonrpc-server/README.md`、`dsh-headless/README.md`、各包 `LICENSE`
- 解析方式：Python 按 asar 头读取 JSON 目录并只读抽取指定文件到 `/tmp`，未修改 app 包

**本次使用的查询命令（摘要）**
```bash
cargo search async-openai                 # 可用
cargo info async-openai                   # 卡在下载，>90s 后中止（未用）
curl -s -A "zgraph-research/0.1" https://crates.io/api/v1/crates/<name>           # 元数据
curl -s -A "zgraph-research/0.1" https://crates.io/api/v1/crates/<name>/<ver>/dependencies
curl -s -A "zgraph-research" https://api.github.com/repos/<owner>/<repo>          # 仓库活跃/许可
```

---

## 附录：事实 / 推断边界

- **事实**：版本号、发布时间、许可字符串、下载量、依赖声明数、README/源码片段、DSH 本机文件结构与 LICENSE、DSH 仓库与 npm 发布状态、ACP/headless/SDK 的文档描述。
- **推断**：「集成成本（人日）」「是否值得复用」「rig 拿 transport 会与框架打架」「DSH 双进程成本」等，均基于 API 面积与团队上下文的判断，已在文中标注。
- **未验证**：所有需要实际构建/发请求/启动 DSH 服务的行为（见第 6 节）。本机 crates.io 直连下载不可靠，未生成依赖树。
