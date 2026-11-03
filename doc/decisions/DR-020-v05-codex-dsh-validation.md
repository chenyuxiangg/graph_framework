# DR-020 · v0.5 真实 Codex/DSH 验证方案

> **状态**：**DSH FINAL 已冻结**（spike 报告 `/tmp/dsh-repo-spike.md`，327 行）；**Codex 中期已落盘（`74bba4f`）；F5d FINAL/真 key 待**。
> **日期**：2026-11-03 · **作者**：docs-dev · **关联**：DR-011（外部 harness）、DR-016、DR-018、DR-019、07 AC-20/28、v0.5 F5a–d
> **原则**：真实构建/运行、证据可复现、不可用如实标注、**不伪造**；不写密钥/不使用用户 `~/.dsh` 凭据。

## 0. 摘要

| 项 | 结论（DSH FINAL） | Codex |
|---|---|---|
| 源码 | https://github.com/deepseek-ai/deepseek-harness （public/MIT；TypeScript/pnpm monorepo；~349MB developer preview） | https://github.com/openai/codex（中期已落盘（`74bba4f`）；F5d FINAL/真 key 待） |
| 本环境构建 | 本机带宽无法 clone/build，**不阻塞**：本机 Desktop **0.2.1-rc.3** 自带同一套 `dsh`+`dsh-acp`+ACP SDK 1.4.0；rc.5 npm 代码形状一致；`npx @deepseek-ai/dsh@0.2.1-rc.5 --profile acp`；源码构建 **Node ≥24.9 + pnpm 11.28.5** | Codex 中期已落盘（`74bba4f`）；F5d FINAL/真 key 待 |
| 入口 | **`dsh --profile acp`**（ACP v1 stdio 首选）；`--profile headless "job"`（one-shot 无流式）；`--profile sdk`（非 ACP） | Codex 中期已落盘（`74bba4f`）；F5d FINAL/真 key 待 |
| 传输 | **JSON-RPC 2.0 NDJSON**（每行一条；**无 Content-Length**） | 中期已确认（codex exec --json） |
| 实测（无 key，隔离 DSH_HOME） | initialize/session/new/close/EOF/unknown session 均通过；prompt → **-32603 no API key**（无模型请求） | Codex 中期已落盘（`74bba4f`）；F5d FINAL/真 key 待 |
| 验证 | **分层 F5a CI fake ACP / F5b 真实握手无 key / F5c 全链路需 key；F5d Codex 三层（CI mock 零密钥/真 key/取消）+ F5f 可选工具执行** | **中期已通（零密钥 e2e）** |
| AC-28 | a–f（§5） | 待补 |

## 1. DSH FINAL 事实（spike 2026-11-03）

- **仓库**：public/MIT、TypeScript/pnpm monorepo（~349MB **developer preview**）；本机带宽无法 clone/build（不阻塞）。
- **本机同一套**：Desktop 版 **0.2.1-rc.3** 自带 `dsh` + `dsh-acp` + ACP SDK **1.4.0**；npm rc.5 代码形状一致；`npx @deepseek-ai/dsh@0.2.1-rc.5 --profile acp`；源码构建需 Node ≥24.9 + pnpm 11.28.5。
- **入口确认**：`dsh --profile acp`（ACP v1 stdio 首选）、`--profile headless "job"`、`--profile sdk`（非 ACP）。
- **传输**：JSON-RPC 2.0 **NDJSON**（每行一条；**无 Content-Length**）。
- **真机实测（隔离 `DSH_HOME`，无凭据）**：
  1. `initialize` → `{"protocolVersion":1,"agentInfo":{"name":"deepseek-harness-acp",...},"authMethods":[]}`（**1.44s** cold）；
  2. `session/new`（**cwd 绝对路径**，`mcpServers=[]`）→ `{sessionId, configOptions:[model=deepseek-official/deepseek-v4-flash, reasoning_effort=high]}`（**0.06s**）；
  3. `session/prompt` → **-32603** `no API key for provider route deepseek-official`（**无模型请求**）；
  4. unknown session → **-32602**；`session/close(ok)` → `{}`；stdin EOF → **exit 0**。
- **安全**：隔离 `DSH_HOME`（首次启动需可写）；**不使用**用户 `~/.dsh`；不发起无授权模型调用；不写密钥。

## 2. Adapter 缺口与 F5a 实现范围

`python/zgraph_worker/adapters/dsh_acp.py` 当前只有 hello/error 骨架 → **F5a 需 ~200–300 行 + 单测**：

- 方法：`initialize` → `session/new` → `session/prompt`；读 `session/update` notifications；`session/cancel` notification；`session/close`；**应答 server→client `session/request_permission`**（否则工具调用挂起）。
- 传输：JSON-RPC 2.0 NDJSON（无 Content-Length）；stderr 日志；stdin EOF 退出。
- 映射（`zgraph-harness/1`）：
  | ACP | 映射 |
  |---|---|
  | `agent_message_chunk` | `delta`（DSH 只发 committed messages → coarse） |
  | `agent_thought_chunk` | `reasoning` |
  | `tool_call` | `tool_start` |
  | `tool_call_update` | `tool_end` |
  | `progress` | **无对应**（忽略/不映射） |
  | `usage_update`(used/size) | **不能**填 input/output tokens（仅上下文用量；不伪造；成本统计不可得） |
  | `stopReason` end_turn/max_tokens/cancelled | `EndTurn`/`MaxTokens`/`Other("cancelled")` |
- **取消语义**：zgraph cancel → ACP `session/cancel` notification，等 prompt 返回 `cancelled`，**不发 result**；**2s 宽限内退出**（否则 SIGKILL 进程组）；hello_ok 立即回、ACP init 懒执行。
- **cwd**：使用**一次性 scratch 目录**（DSH 工具会写文件/执行命令），并受 zgraph sandbox/jail 约束；`DSH_HOME` 首次启动需可写。
- 不双重 trace/状态机；外部 harness 不写 trace/DB（v0.3 边界）。

## 3. 验证分层（F5a–d；写入 AC-28）

| 层 | 内容 | 门控 | 本机可跑 |
|---|---|---|---|
| **F5a** adapter transport + fake ACP server | `dsh_acp.py` initialize/session/update/permission/cancel/close 映射单测 | 默认 CI | 是 |
| **F5b 真实 DSH 握手** | 真 `dsh --profile acp` initialize/session/new/close/EOF/unknown session/无 key prompt 降级 | `#[ignore]` + **`ZGRAPH_REAL_DSH=1`** | 是（无 key） |
| **F5c 全链路** | 真 prompt "Reply with exactly: ok" → ≥1 agent_message_chunk、stopReason=end_turn、`result.content="ok"`、run/trace/replay | `#[ignore]` + `ZGRAPH_REAL_DSH=1` + **`DEEPSEEK_API_KEY`**（用户提供/授权） | 需 key |
| **F5d Codex 三层** | CI mock 零密钥 e2e / 真实 key 全链路 / 取消；**F5f** 可选工具执行 | 中期已落盘（`74bba4f`）；F5d FINAL/真 key 待 | 零密钥可跑 |

- **CI 不 clone 仓库**；F5b 用本机安装/npx；F5c 需 key（用户授权，不写密钥）；发布材料记录构建/运行/限制，不可用如实标注。
- 阶段 B 命令（可复现）：

```bash
export DSH_HOME=/tmp/dsh-home          # 首次可写；隔离用户 ~/.dsh
export DEEPSEEK_API_KEY=...            # 仅 F5c 需要；用户授权，不写仓库
dsh --profile acp                      # 本机 0.2.1-rc.3；或 npx @deepseek-ai/dsh@0.2.1-rc.5
```

## 4. Codex（中期结论：零密钥 e2e 已通）

### 4.1 构建/安装

- 本机源码构建不可行（网络 clone/依赖带宽问题，非代码）；改用 **npm 预编译 `npm i @openai/codex@0.166.0`**（约 4min）；源码等价命令 `codex-rs && cargo build --release -p codex-cli --bin codex`。
- 版本锁定 **0.166.0**；二进制/行为以该版本为准。

### 4.2 零密钥端到端（CI 可跑，F5d-a）

- `$CODEX_HOME/config.toml` 配自定义 provider：

  ```toml
  model_provider = "mock"
  [model_providers.mock]
  base_url = "http://127.0.0.1:<PORT>/v1"
  wire_api = "responses"
  env_key = "MOCK_API_KEY"   # 只要求有值，不校验真实密钥
  ```

- 本地 **mock Responses SSE 服务**；`codex exec --json` 正常 `turn.completed`。
- **adapter 不传 `-c`**：必须走 `CODEX_HOME/config.toml`（唯一干净路径）。
- 实测：`hello_ok` + `result{content:"hello from mock", stop_reason:EndTurn, usage:{11,7,18,2}}`；**cached_input_tokens=4 丢弃**。
- 零密钥 CI 不需要工具执行（mock provider 无 command execution），规避嵌套 sandbox 风险。

### 4.3 取消/生命周期

- SIGTERM → killpg → pid 消失、无孤儿；stdin 关闭后 exit0（符合 `plugin.rs` 契约）。
- 取消宽限沿用 2s→SIGKILL（DR-011）。

### 4.4 adapter 缺口（F5d 修）

| # | 级别 | 问题 | 修法 |
|---|---|---|---|
| 1 | 中 | 任一顶层 `{"type":"error"}` 置 failed 且 `turn.completed` 不重置 → 重连后成功也误报（真机见 5 次 Reconnecting） | **`turn.completed` 清 failed** |
| 2 | 中 | 未传 `--ephemeral` → CODEX_HOME 写 sessions/rollout + 4 sqlite（6 run 5.6MB） | **加 `--ephemeral`** |
| 3 | 低 | prompt 走 argv（ARG_MAX 1MB；900KB 可过，>1MB E2BIG） | 文档标注（或临时文件，若 codex 支持） |
| 4 | 低 | `iterations=1` 与 codex 内部多轮不符；usage 累计（22/14） | 文档口径（iterations 降级；usage 累计按 v0.3 字段） |
| 5 | 低 | `exec --json` 无 `command_execution` item → tool_start/tool_end 不可得（codex 限制，非 adapter bug） | 文档标注；不伪造 tool 事件 |
| 6 | 低 | reasoning 可得但 adapter 丢弃，与 `capabilities.reasoning=false` 一致 | 文档/保持 |

### 4.5 嵌套 sandbox 风险

- codex 自带 `sandbox-exec` 在嵌套环境 **exit 71**；验证「codex 改代码」需 `-s danger-full-access` 或容器外跑。
- **CI mock provider 不需要工具执行**；真实工具执行验证列为 **F5f 可选**（非嵌套用户终端/容器，文档/手动，不伪造）。

### 4.6 F5d/F5f 分层

| 层 | 内容 | 门控 | CI |
|---|---|---|---|
| **F5d-a** | CI mock provider 零密钥确定性 e2e（CODEX_HOME config + 本地 mock SSE；断言 hello_ok/result/usage/cached 丢弃） | 默认 CI（无需 key） | 是 |
| **F5d-b** | 真实 key 全链路（`#[ignore]` + `OPENAI_API_KEY` 或 `codex login`；可选，不阻塞发布） | `#[ignore]` + env | 需 key |
| **F5d-c** | 取消 pid/无孤儿（CI 可跑） | 默认 CI | 是 |
| **F5f**（可选） | 非嵌套环境 `-s danger-full-access` 验证 codex 工具执行 | 手动/用户终端 | 否 |

## 5. AC-28（设计冻结；DSH FINAL + Codex 中期已落盘；F5 实测）

| # | 断言 |
|---|---|
| a | 源码/安装事实记录（DSH：public/MIT/TS pnpm ~349MB developer preview；本机 Desktop 0.2.1-rc.3 同套 dsh+dsh-acp+SDK1.4.0；rc.5 形状一致；源码构建 Node≥24.9+pnpm11.28.5；**不伪造 clone/构建**） |
| b | F5a fake ACP：transport/initialize/session/update/permission/cancel/close 映射单测（CI 默认） |
| c | F5b 真实握手（无 key）：initialize v1/agentInfo/authMethods[]（1.44s）、session/new（cwd 绝对/mcpServers=[]→sessionId/configOptions，0.06s）、close `{}`、EOF exit0、unknown session -32602 |
| d | F5b 无 key 降级：prompt → -32603 no API key（**无模型请求**）；不伪造成功；adapter 缺口/映射文档化 |
| e | F5c 全链路（需 `DEEPSEEK_API_KEY`，用户授权）：prompt "Reply with exactly: ok" → ≥1 agent_message_chunk、stopReason=end_turn、`result.content="ok"`、run/trace/replay；不可用如实标注 |
| f | 门控：`#[ignore]` + `ZGRAPH_REAL_DSH=1`/`ZGRAPH_REAL_CODEX=1`；CI 不 clone；发布材料证据齐全；**usage_update 不伪造 token/成本** |
| g | **F5d-a Codex 零密钥 CI**：`CODEX_HOME/config.toml` mock provider + 本地 mock Responses SSE；执行 **`codex exec --json --skip-git-repo-check -o <file> "reply with exactly: hello"`**；断言 hello_ok/result `hello from mock`/stop_reason EndTurn/usage(11,7,18,2)/cached=4 丢弃；**无需 key** |
| h | **F5d-b 真实 key 全链路**（`#[ignore]`+`OPENAI_API_KEY`/`codex login`，可选不阻塞发布）；不可用如实标注 |
| i | **F5d-c 取消**：SIGTERM→killpg→pid 消失/无孤儿；stdin close exit0 |
| j | **F5f 可选**：非嵌套环境 `-s danger-full-access` 验证 codex 工具执行（文档/手动，不伪造；CI mock 不需要） |

**可复现命令（Codex 零密钥 CI；F5d-a）**：

```bash
npm i @openai/codex@0.166.0            # 预编译；源码等价 codex-rs && cargo build --release -p codex-cli --bin codex
export CODEX_HOME=/tmp/codex-home     # config.toml 配 mock provider（base_url=http://127.0.0.1:PORT/v1, wire_api=responses, env_key=MOCK_API_KEY）
export MOCK_API_KEY=dummy              # 只要求有值
codex exec --json --skip-git-repo-check -o /tmp/codex-out.json "reply with exactly: hello"  # 前置本地 mock Responses SSE；断言 hello_ok/result/usage
```

## 6. 风险（写入 DR/台账）

| 风险 | 说明 | 缓解 |
|---|---|---|
| adapter 工作量低估 | 当前仅 hello/error 骨架 → ~200–300 行 + 单测 | F5a 独立波次/工作量评估；fake ACP 先测 |
| 工具副作用 | DSH 工具会写文件/执行命令 | 一次性 scratch cwd + zgraph sandbox/jail；最小权限 |
| 2s 取消宽限 | 可能被 SIGKILL | 记录/文档；测试确认；必要时放宽（Lead 裁决） |
| developer preview 破坏兼容 | 版本/协议可能变 | pin 版本 + 断言 `protocolVersion==1`；F5b 每次校验 |
| usage 成本不可得 | `usage_update` 仅 used/size，非 input/output tokens | **DR-018/019 注明：外部 DSH 成本不可统计/不伪造**；仅 token 若 provider 另报 |
| 无 API key | F5c 无法验证 | 分层；key 门控；如实标注 |

## 7. 波次（F5 拆分；F5d Codex 中期已落盘，F5d FINAL/真 key 待）

- **F5a** `dsh_acp.py` ACP transport + fake ACP 单测（CI）→ **F5b** 真实 DSH 握手（无 key）→ **F5c** 全链路（key 门控）→ **F5d** Codex 三层（CI mock 零密钥/真 key/取消；中期已落盘）→ **F5f** 可选工具执行。
- 每层独立提交/评审；不阻塞 F1–F4；F5c 缺 key 时如实标注不伪造。
