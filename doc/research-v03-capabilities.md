# v0.3 能力扩展调研（V1 设计冻结输入）

> **状态**：**V1 冻结输入；arch-researcher 报告已合入（2026-11-01）**；剩余 spike 见 ISSUE-151–154 与 DR-008 V3/V4；本文件随 REV-096 评审（不引用 /tmp 路径）。
> **范围**：skills（Agent Skills）/ MCP（stdio + Streamable HTTP）/ 可插拔 harness（Codex exec、DSH/ACP）。
> **原则**：只引用公开资料与仓库内既有调研（`doc/analysis/research-harness-alternatives.md`、`research-codex-embed.md`）；不写入 token/密钥；未验证项显式标「需 spike」。

## 0. 结论速览（推荐；REV-096 裁决已合入）

| 能力 | 推荐实现 | 备选 | 需 spike 验证 |
|---|---|---|---|
| skills | 目录 + `SKILL.md`（YAML frontmatter + Markdown 正文）；system 仅注入 `name/description`；`skill_view(name)` 工具按需加载正文；**不执行 skill 脚本** | 全量注入（token 风险） | agentskills.io frontmatter 字段/大小限制的最新口径 |
| MCP client | **rmcp 3.8.0**（Apache-2.0；Rust SDK；stdio + Streamable HTTP）作为 client；适配自有 `ToolDef`；协议目标 **MCP `2026-07-28`**；feature-gated | 自写最小 stdio JSON-RPC fallback | rmcp 3.8.0 对 Streamable HTTP/`2026-07-28` 的实际支持矩阵（V3 spike） |
| 外部 harness | **子进程 adapter + Rust `AgentProvider` trait seam**；NDJSON `zgraph-harness/1`；Codex/DSH **薄适配器** | 嵌入 Codex crate（未发布 crates.io、依赖重） | `codex exec --json`/app-server 事件稳定性；DSH ACP/headless 入口（V4 spike） |

## 0.1 调研状态（arch-researcher 2026-11-01；open 项保留）

**已证实（写入 DR/设计）**

| 事实 | 结论 |
|---|---|
| rmcp | **3.8.0**（2026-10-26）、Apache-2.0、MSRV 1.88/edition 2024；`ClientLifecycleMode` 双 era；`call_tool` 自动 MRTR（默认 10）；无 per-call 超时；HTTP client 16 并发 + `control_request_timeout=5s` |
| MCP 规范 | `2026-07-28` 无状态（删除 initialize/session/GET-SSE/Last-Event-ID；每请求 `_meta`；HTTP `MCP-Protocol-Version`/`Mcp-Method`）；MRTR 取代 sampling/roots；`2025-11-25` legacy 仍占生态多数 → **双 era 必须** |
| Agent Skills | 目录 `skills/<name>/SKILL.md`；name ≤64 且=父目录、description ≤1024；license/compatibility/metadata/allowed-tools；单层不递归；三级渐进披露；catalog/正文预算；正文不压缩 |
| Codex | `codex exec --json` JSONL 事件；无 token delta；iterations 无对应；退出码仅 0/1；`--output-last-message` 最稳；cached tokens 需丢弃/另存 |
| DSH/ACP | ACP 有 chunk 通道；**DSH 只发 committed 消息**；可依赖入口 `dsh --profile acp`；headless `--json` schema 未公开、sdk wire 不可考 |
| trace | 不需要 v7：`streaming_events` + `llm_calls` + `recorded_outputs` 足够；`call_id` 命名空间可区分 |

**待 spike（不阻塞 V1；V2–V4 逐项关闭）**

| # | 项 | 影响 |
|---|---|---|
| 1 | rmcp 3.8 `mcp` feature 编译体积/依赖树/rust-version | V3 依赖决策（超预算→自写最小 stdio fallback） |
| 2 | rmcp 对 `2026-07-28` Streamable HTTP 的实际兼容/双 era 探测 | V3 传输实现 |
| 3 | macOS arm64 进程组 kill（npx→node 孙进程） | V3/V4 资源回收 |
| 4 | `codex exec --json` 的 SIGTERM 收尾/进程组/`-o` 文件 | V4 Codex adapter |
| 5 | `dsh --profile acp` 的 `agent_message_chunk` 粒度/usage/cancel 收敛 | V4 DSH adapter（best-effort） |
| 6 | `dsh --profile headless --json` 事件 schema | 是否纳入（当前不依赖） |
| 7 | Agent Skills 规范后续版本字段差异 | V2 实现前复核 |

## 1. skills（Agent Skills）

### 1.1 规范要点（待 spike 锁定版本）

- 目录形态：`skills/<name>/SKILL.md`；`name` 小写短横线、与目录名一致；`description` 为发现/路由的唯一依据。
- `SKILL.md` = YAML frontmatter + Markdown 正文；frontmatter 建议字段：`name`、`description`、`license?`、`allowed-tools?`、`metadata?`（版本/作者）。
- 渐进披露：agent 先看到「索引（name+description）」，需要时再读取正文/附属文件（`references/`、`scripts/` 等）；**框架 v0.3 不执行 skill 脚本**，只做指令注入与按需加载。

### 1.2 与本仓库现状的衔接

- 02 §2.5.7/§2.11 C-20 已冻结：skills 归 **Harness Loading**，不参与 DSL/`graph_hash`；05 §5.10 把 skills 列在保留扩展位。
- 现有 `AgentConfig` 是严格白名单（`deny_unknown_fields`）；新增 `skills` 必须 additive（缺省=无 skills，旧配置逐字节不变）。
- 资产发现沿用 `--assets-dir`/`HARNESS_CONFIG_LOAD` 体系；skills 目录独立 `--skills-dir`（env `ZGRAPH_SKILLS_DIR`），缺省 `.zgraph/skills`（与 assets 并列）。

### 1.3 风险

- 不可信内容（SKILL.md 可含 prompt 注入）：必须作为**数据**注入，标注来源、不执行、不覆盖系统指令；正文大小/条数上限。
- 路径穿越/符号链接：只读取 skills 根下 regular file（canonicalize 后前缀校验）；拒绝 `..`/绝对路径。
- token 预算：索引 + 正文字数计入 05 §5.4 预算；超限裁剪策略需冻结（索引全量、正文按 `skill_view` 单次加载上限）。

### 1.4 开放问题（spike）

1. agentskills.io 的 frontmatter 是否强制 `allowed-tools`/`metadata` 版本字段？未知字段是拒绝还是忽略（与 AgentConfig 严格白名单的差异需裁决）。
2. 官方推荐 `description` 长度上限（如 1024 字符）与 SKILL.md 大小上限（如 256KiB）具体值。
3. 是否需要 `skill_view` 支持附属文件（如 `references/*.md`）——v0.3 推荐仅正文，附属文件延后。

## 2. MCP（Model Context Protocol）

### 2.1 传输与生命周期（设计目标）

- **stdio**：client 启动 server 子进程（command/args/env），JSON-RPC over stdin/stdout；错误日志走 stderr。
- **Streamable HTTP**：POST JSON-RPC；按规范处理 SSE 流/会话 id；只允许 http/https，TLS 校验默认开启。
- 生命周期：`initialize`（协议版本/能力协商）→ `tools/list`（缓存，含 `inputSchema`）→ 按需 `tools/call` → 取消（notifications/cancelled）→ 关闭（stdio 终止进程组）。
- 工具命名空间：`mcp__<server>__<tool>`，映射为自有 `ToolDef`（name 唯一，schema 来自 `inputSchema`）；冲突/非法名 → Harness 配置错误。

### 2.2 Rust client 选型（待 spike 锁定）

- **推荐 rmcp**：官方 Rust SDK，覆盖 stdio/Streamable HTTP，避免自写 JSON-RPC/传输细节；引入为 workspace 依赖，版本以 spike 实测为准。
- 备选：自写最小 JSON-RPC client（仅 stdio）——不推荐（HTTP/SSE/会话/取消需长期维护）。
- 需 spike：rmcp 对 `2026-07-28` 协议版本、Streamable HTTP、`tools/call` 取消/超时的支持；许可证；MSRV 与现有 workspace 一致。

### 2.3 安全/资源

- 信任模型：`mcp_servers.json` 是**受信配置**（由用户/管理员提供）；server 仍视为不可信边界：工具输出按外部数据注入，长度截断、无隐式代码执行。
- 密钥：通过 env 注入（`mcp_servers.json` 只允许 `env` 名映射，不落日志）；不写入 trace。
- 超时：initialize/tools-list/call 分别超时；stdio 崩溃/协议错误 → 工具调用失败（受控错误，不拖垮 run 状态机）；进程组 kill（复用 03 §3.9.4 口径）。
- 并发：v0.3 保持**单响应多 tool_call 串行**（05 §5.5.3），不引入并行。

### 2.4 trace 决策（推荐）

- **默认不新增 schema（不加 v7）**：MCP 工具调用沿用既有 `StreamingEvent::ToolCallStart/ToolCallEnd` 记录（`streaming_events` 表 + `llm_calls` 汇总），不引入新表；`events.kind` 不变。
- 若后续需要逐次工具审计（参数/结果摘要/耗时），作为 v7 候选另立 DR（默认关闭、迁移兼容）；v0.3 不纳入。
- 工具名以 `mcp__<server>__<tool>` 进入 `ToolCallStart.tool_name`；结果截断与 `result_summary` 上限沿用 05/06 既有口径。

## 3. 外部 harness（Codex / DSH）

### 3.1 现状结论（仓库内既有调研）

- Codex CLI 是 Rust 实现但**未发布 crates.io**；git 依赖/嵌入成本高、API churn 大（`doc/analysis/research-codex-embed.md`）；官方方向是 app-server/exec 等**进程边界**。
- DSH 为 **MIT** 开源、拥有自己的 agent loop/状态机；提供 **ACP/headless/SDK wire** 入口（`research-harness-alternatives.md` §3.1–§3.4）；ACP 语义事件可映射、token delta 可能缺失（adapter 层降级）；适合作为**外部进程**而非嵌入。
- **Codex**：Rust 实现但未发布 crates.io；官方进程边界为 **app-server / `exec --json`**（`research-codex-embed.md` §3）；V4 以薄 adapter 消费其事件。

### 3.2 推荐形态

- **Rust trait seam**：`AgentProvider` 由「扩展位」升级为实际选择器：`builtin`（默认，行为不变）与 `subprocess`（外部进程协议）。
- **子进程协议**：NDJSON over stdio，`zgraph-harness/1`：hello/能力协商 → run → 事件（token/reasoning/tool_call/progress 子集）→ final（四字段 output）→ 退出；取消走 SIGTERM/关闭 stdin；超时/输出上限由父进程强制。
- Codex 适配器：优先用 `codex exec --json`（若事件形状稳定）或 app-server JSON-RPC（若更稳定），实现协议的子进程 adapter；**不嵌入 Codex crate**。
- DSH 适配器：若 DSH 提供 headless/ACP 入口，用薄适配器把 ACP 语义事件映射为本协议；无 token delta 时降级为「无 streaming」（在 adapter 层声明能力，不改变框架事件类型）。
- L2 回放：外部 harness **不重跑**——`--mode=outputs` 用 `recorded_outputs` 直接返回最终输出；`--mode=events` 重放既有事件，不依赖外部进程/网络。
- 安全：外部命令由 AgentConfig 指定，视同用户受信配置；仍限制参数/环境/工作目录、进程组 kill、输出上限；不传密钥到日志。

### 3.3 需 spike 验证

1. `codex exec --json` 的事件/退出码/取消行为是否稳定到可作为协议适配（版本锁定策略）。
2. Codex app-server JSON-RPC 与 ACP 的能力差异；是否支持 turn 循环外的「单次 run」。
3. DSH 是否提供无需交互的 headless/ACP 入口；版本/许可证/进程模型。
4. 外部 harness 的「输出四字段」是否可由适配器稳定构造（content/usage 等）。

## 4. 报告来源（公开 URL；2026-11-01 访问）

- MCP 规范：https://modelcontextprotocol.io/specification/latest ；2026-07-28 changelog：https://modelcontextprotocol.io/specification/2026-07-28/changelog.md ；stdio/Streamable HTTP/tools 子页（同上域名）。
- rmcp：https://crates.io/api/v1/crates/rmcp （3.8.0/Apache-2.0）；https://github.com/modelcontextprotocol/rust-sdk
- Agent Skills：https://agentskills.io/specification ；https://agentskills.io/client-implementation/adding-skills-support
- Codex：https://github.com/openai/codex （`codex-rs/exec/src/exec_events.rs`、`event_processor_with_jsonl_output.rs`、`cli.rs`）
- ACP：https://agentclientprotocol.com/protocol/v1/prompt-turn.md ；DSH ACP：https://github.com/deepseek-ai/DeepSeek-Harness/blob/master/packages/acp/README.md
