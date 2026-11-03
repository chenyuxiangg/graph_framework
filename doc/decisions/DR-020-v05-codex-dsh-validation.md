# DR-020 · v0.5 真实 Codex/DSH 验证方案

> **状态**：**部分冻结**——DSH 源码仓 spike 已出（interim，full chain 只缺 API key）；**Codex 段待第二个 spike**。
> **日期**：2026-11-03 · **作者**：docs-dev · **关联**：DR-011（外部 harness）、DR-016、07 AC-20/28、v0.5 F5
> **原则**：真实构建/运行、证据可复现、不可用如实标注、**不伪造**；不写密钥/不使用用户 `~/.dsh` 凭据。

## 0. 摘要

| 项 | 结论（DSH） | Codex |
|---|---|---|
| 源码 | https://github.com/deepseek-ai/deepseek-harness （公开 MIT；TypeScript/pnpm monorepo，master ~350MB） | https://github.com/openai/codex（待 spike） |
| 本环境构建 | clone 带宽不可行（~25KB/s）；改用**本机已安装 dsh CLI + ACP 包 0.2.1-rc.3**；源码构建需 **Node ≥24.9 + pnpm 11.28.5** | 待 spike |
| 真实入口 | **`dsh --profile acp`**（ACP v1，JSON-RPC 2.0，newline-delimited stdio；SDK `@agentclientprotocol/sdk` 1.4.0）；另有 `--profile headless "job"`（one-shot 无流式）、`--profile sdk`（JSON-RPC 非 ACP） | 待 spike |
| 实测（无凭据、隔离 DSH_HOME） | initialize/session/new/session/close/EOF 均通过；`session/prompt` → **-32603 no API key**（无模型请求）；unknown session → -32602 | 待 spike |
| 验证分层 | (a) CI fake ACP server 单测 transport/映射；(b) 真实 DSH 协议握手（无 key，本机可跑）；(c) 全链路 prompt 需 `DEEPSEEK_API_KEY`（用户提供/授权） | 同左 |
| AC-28 | a–f（见 §5，含 DSH 握手/降级/映射） | 待补 |

## 1. DSH 事实（spike 2026-11-03，本机实测）

- **仓库**：公开 MIT、TypeScript/pnpm monorepo（master，~350MB）；本环境 clone 带宽 ~25KB/s 不可行；**仓库不要求 CI clone**（F5 可用本地安装/npx）。
- **本机可用版本**：已安装 dsh CLI + ACP 包 **0.2.1-rc.3**；npm 路径 `npx @deepseek-ai/dsh@0.2.1-rc.5 --profile acp`；源码构建需 Node ≥24.9 + pnpm 11.28.5。
- **入口**：`dsh --profile acp`（ACP v1；JSON-RPC 2.0；newline-delimited stdio；SDK `@agentclientprotocol/sdk` 1.4.0）；`--profile headless "job"` 为 one-shot 无流式；`--profile sdk` 为 JSON-RPC 非 ACP。
- **实测（隔离 `DSH_HOME=/tmp/dsh-home`，无凭据）**：
  1. `initialize` → `{"protocolVersion":1,"agentInfo":{"name":"deepseek-harness-acp",...},"authMethods":[]}`（**1.44s cold**）；
  2. `session/new`（cwd=`/tmp/dsh-workspace`，mcpServers=[]）→ `{sessionId, configOptions:[model=deepseek-official/deepseek-v4-flash, reasoning_effort=high]}`；
  3. `session/prompt` → **-32603** `no API key for provider route deepseek-official; export DEEPSEEK_API_KEY`（**无模型请求**）；
  4. unknown session → **-32602**；`session/close(ok)` → `{}`；stdin EOF → **exit 0**。
- **安全**：测试隔离 `DSH_HOME=/tmp/dsh-home`；**不使用**用户 `~/.dsh` 凭据；不发起无授权模型调用；不写密钥。

## 2. 关键 gap 与 F5 实现范围

`python/zgraph_worker/adapters/dsh_acp.py` 当前**没有 ACP transport**（只发结构化协议错误）。F5 必须实现 **ACP JSON-RPC NDJSON client**：

- 方法：`initialize` → `session/new` → `session/prompt`；`session/cancel`（notification）、`session/close`；处理 `session/update` notifications；**应答 server→client `session/request_permission`**（否则工具调用挂起）。
- 映射（`zgraph-harness/1`）：
  | ACP | 映射 |
  |---|---|
  | `agent_message_chunk` | `delta`（DSH 只发 committed messages → coarse，非逐 token） |
  | `agent_thought_chunk` | `reasoning` |
  | `tool_call` | `tool_start` |
  | `tool_call_update` | `tool_end` |
  | `progress` | **无对应**（不映射/忽略） |
  | `usage_update`(used/size) | **不能**填 input/output tokens（仅上下文用量；不伪造） |
  | `stopReason` end_turn/max_tokens/cancelled | `EndTurn`/`MaxTokens`/`Other("cancelled")` |
- 协议错误/取消/EOF：exit 0/关闭语义按 ACP；超时/取消 kill 进程组（沿用 DR-011）。
- 不双重 trace/状态机；外部 harness 不写 trace/DB（v0.3 边界）。

## 3. 验证分层（写入 AC-28）

| 层 | 内容 | 门控 | 本机可跑 |
|---|---|---|---|
| (a) CI fake ACP | fake ACP server 单测 transport/initialize/session/update 映射/permission 应答 | 默认 CI | 是 |
| (b) 真实 DSH 握手 | 真 `dsh --profile acp` initialize/session/new/close/EOF/unknown session/无 key prompt 降级 | `#[ignore]` + **`ZGRAPH_REAL_DSH=1`** | 是（无 key） |
| (c) 全链路 prompt | 真 prompt → 模型 → run/trace/replay | `#[ignore]` + `ZGRAPH_REAL_DSH=1` + **`DEEPSEEK_API_KEY`**（用户提供/授权） | 需 key |

- **CI 不 clone 仓库**；F5 用本机安装/npx；发布材料记录构建/运行/限制；不可用如实标注。
- 命令（本机实测，可复现）：

```bash
# 隔离 DSH_HOME，不带用户凭据
export DSH_HOME=/tmp/dsh-home
# 真实 ACP 入口（本机已安装 0.2.1-rc.3；或 npx @deepseek-ai/dsh@0.2.1-rc.5）
dsh --profile acp
# 全链路（需用户授权 key；不写入仓库）
export DEEPSEEK_API_KEY=...   # 仅本地 env
```

## 4. Codex（待第二个 spike）

- 待补：源码仓 clone/构建（Rust workspace）、`codex exec --json`/`--output-last-message` 入口、认证/env、事件样例、取消/退出码、adapter 映射差异、可复现命令。
- 未到前 Codex 段保持「待 spike」；不阻塞 DSH 验证/F5 其他项。

## 5. AC-28（设计冻结；F5 实测；Codex 待补）

| # | 断言（DSH 部分已实测/可复现；Codex 待 spike） |
|---|---|
| a | 源码/安装事实记录（DSH：MIT/monorepo/Node≥24.9+pnpm11.28.5；本环境 clone 不可行 → 本机 dsh/npx；**不伪造 clone/构建**） |
| b | 真实入口握手（DSH `--profile acp`：initialize/session/new/close/EOF；记录 1.44s cold/模型 configOptions） |
| c | 无 key 降级：`session/prompt` → -32603 no API key，**无模型请求**、不伪造成功；unknown session -32602 |
| d | adapter transport 映射：fake ACP CI 单测（chunk/tool/permission/usage/stopReason）；DSH committed/coarse 文档化 |
| e | 全链路（需 `DEEPSEEK_API_KEY`，用户授权）：真 prompt → run/trace/replay；不可用如实标注 |
| f | CI 默认跳过（`#[ignore]` + `ZGRAPH_REAL_DSH=1`/`ZGRAPH_REAL_CODEX=1`）；不 clone 仓库；发布材料证据齐全 |

## 6. 风险

| 风险 | 说明 | 缓解 |
|---|---|---|
| DSH 构建/clone 不可行 | 带宽/Node/pnpm 版本 | 用本机安装/npx；发布材料如实标注；不要求 CI clone |
| 无 API key | 全链路无法验证 | 分层验证；无 key 只到握手/降级；不伪造 |
| adapter transport 缺失 | 当前 dsh_acp.py 只发协议错误 | F5 实现 ACP client（§2）；fake ACP CI 覆盖 |
| permission 挂起 | 不应答 session/request_permission 会挂起 | F5 必须应答；fake/真实测试 |
| usage 语义差异 | used/size 非 input/output tokens | 不伪造；只记录上下文用量/文档化 |
| 凭据隔离 | 用户 ~/.dsh 凭据 | 强制 DSH_HOME=/tmp/dsh-home；不写密钥 |
