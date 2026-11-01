# DR-010 · MCP 设计冻结（tools：stdio + Streamable HTTP）

> **状态**：V1 设计冻结；实现见 DR-008 V3（`feat/harness-mcp`）。
> **日期**：2026-11-01 · **作者**：docs-dev · **关联**：DR-008、05 §5.6/§5.10、06 §6.2/§6.5、07 AC-19、[`research-v03-capabilities.md`](../research-v03-capabilities.md)
> **协议目标**：MCP 规范 `2026-07-28`（互操作目标；client 以 rmcp 实现为主，版本/能力以 spike 实测锁定）。

## 0. 摘要

| 项 | 冻结 |
|---|---|
| client | Rust；**rmcp**（首选）适配进 `ToolDef` 注册表；仅 stdio + Streamable HTTP 两种传输 |
| registry | `<assets_dir>/mcp_servers.json`（受信配置）：`mcpServers` map；server 声明 `command/args/env`（stdio）或 `url/headers`（HTTP） |
| AgentConfig | `mcp: {servers: string[], allow: string[]?}`（additive；缺省=不启用） |
| 工具命名 | `mcp:<server>:<tool>`（全局唯一；冲突/非法名 → 配置错误） |
| 生命周期 | initialize（协议/能力）→ tools/list（缓存）→ tools/call（按需）→ cancelled/close |
| 超时/取消 | initialize/tools-list/call 分别超时；取消发 notifications/cancelled；stdio 终止进程组 |
| 结果 | 截断（字符上限），只取文本/结构化摘要；不落全文到 trace |
| trace | **默认不新增 schema（不加 v7）**：沿用 `StreamingEvent::ToolCallStart/ToolCallEnd` + `llm_calls`；`events.kind` 不变 |
| 默认无 MCP | 不启动 server/不注册工具 → 既有行为/输出逐字节不变 |

## 1. 配置

### 1.1 `mcp_servers.json`（registry）

```json
{
  "$schema": "https://zgraph.dev/schemas/mcp-servers.schema.json",
  "mcpServers": {
    "github": {
      "command": "npx",
      "args": ["-y", "@modelcontextprotocol/server-github"],
      "env": {"GITHUB_TOKEN": "${GITHUB_TOKEN}"},
      "timeout_ms": 30000
    },
    "remote": {
      "allow_insecure_http": false,
      "url": "https://example.com/mcp",
      "headers": {"Authorization": "Bearer ${MCP_TOKEN}"},
      "timeout_ms": 30000
    }
  }
}
```

- 位置：`<assets_dir>/mcp_servers.json`（与 agents/tools 并列；可由 `--assets-dir` 覆盖）；文件缺失 = 无 server（不报错，除非 AgentConfig 引用）。
- `mcpServers` map：key = server 名（`^[a-z0-9][a-z0-9_-]{0,63}$`）；value 必须恰好有 `command`（stdio）或 `url`（HTTP）之一；两者同时/都无 → 配置错误。
- `env`/`headers` 值支持 `${ENV_VAR}` 展开（只从进程 env 读取；未定义 → 配置错误，不落日志）。**配置/日志/trace 不写密钥**。
- `timeout_ms?`：单 server 默认 30_000；调用级覆盖见 §5。
- `allow`（AgentConfig）：工具名或 `mcp:<server>:*` 白名单；缺省 = 该 server 的全部工具（仍受 AgentConfig `tools` 过滤）。

### 1.2 AgentConfig（additive）

```json
{
  "agent_id": "issue_triager",
  "mcp": {"servers": ["github"], "allow": ["mcp:github:list_issues", "mcp:github:get_issue"]},
  "tools": ["read_file"]
}
```

- `mcp?: {servers: string[], allow?: string[]}`；缺省无 = 不启用（不读取 registry/不启动进程/不注册工具）。
- `servers` 引用不存在的 server → `HARNESS_CONFIG_LOAD{reason:"mcp_server_not_found"}`；`allow` 条目非法/未匹配 → 配置错误（fail-closed）。
- 与 `tools`：MCP 工具与 Python 工具合并为同一 `ToolDef` 集合；**同名冲突 → 配置错误**（`NameConflict`），不允许覆盖内置/Python 工具。

## 2. 工具映射（`ToolDef`）

| MCP | ToolDef |
|---|---|
| name | `mcp:<server>:<tool>`（例 `mcp:github:list_issues`） |
| description | MCP `description`（截断 1024 字符） |
| parameters | `inputSchema`（JSON Schema 1.0 子集：object/type/properties/required；非法/超大 → 跳过该工具并记 warning，不崩） |
| 执行 | `tools/call`（args=调用参数；结果取 `content` 文本块拼接 + `isError`） |
| 超时 | `timeout_ms`（调用级，缺省 server 值） |
| 结果上限 | 32768 字符（严格截断，附 `truncated:true`） |

- `tools/list` 在 server 首次被 AgentConfig 引用时拉取并缓存到 run 级（不跨 run 复用，避免陈旧）；`notifications/tools/list_changed` v0.3 不处理（下一 run 重拉）。
- `isError:true` → 作为工具结果返回给 LLM（含错误文本），**不**直接 fail run（与 Python 工具错误口径对齐；05 §5.5）。
- 参数校验：调用前按 `inputSchema` 的最小校验（object/required/type）；不满足 → 工具错误（不调用 server）。

## 3. 传输（stdio / Streamable HTTP）

### 3.1 stdio

- 启动：`command` + `args`，env = 进程 env + server `env`（展开后）；cwd = `<assets_dir>`；stdout=JSON-RPC，stderr=日志（截断，不写入 trace）。
- 关闭：完成/取消/超时 → 关闭 stdin → 等待 → SIGTERM → SIGKILL **进程组**（复用 03 §3.9.4 口径）；v0.3 每个 server 一个进程（run 内共享）。
- 崩溃：进程非预期退出 → 后续 call 返回工具错误（`mcp:server crashed`）；不拖垮 run 状态机；下一 run 重启。

### 3.2 Streamable HTTP

- `POST {url}` JSON-RPC；`Accept: application/json, text/event-stream`；会话 id 按规范 header 维护；SSE 事件流解析到 response。
- **默认只允许 `https`**；`http://` 必须在该 server 配置显式 `"allow_insecure_http": true`（默认 false）并文档警示；TLS 校验默认开启（不支持跳过）。`headers` 原样附加（含 `${ENV}` 展开）。
- 超时/取消：请求级 timeout；取消发 `notifications/cancelled` 并关闭流。

### 3.3 依赖策略（Lead 裁决 2026-11-01）

- `rmcp` 以 **feature-gated** 依赖引入（如 `mcp` feature；**默认构建不引入**，不影响无 MCP 路径的编译/二进制/行为）。
- 若实现期 rmcp 版本对 **Streamable HTTP / 协议 `2026-07-28`** 支持不足：允许自写**最小 stdio fallback**（JSON-RPC over stdin/stdout，仅覆盖 initialize/tools-list/tools-call/cancel）；由 V3 spike 决定最终路线，**不阻塞 V1/V2**。
- 任何 client 选择都不得改变 `ToolDef`/AgentConfig/错误码/默认路径契约。

## 4. 生命周期与并发（冻结）

```text
on_agent_run(agent):
  if agent.mcp is None: return（零副作用）
  for server in agent.mcp.servers (声明序):
      ensure_initialized(server)          # initialize + tools/list（缓存；失败→配置/工具错误）
  register tools matching allow/tools     # 名称冲突 fail-closed
  ...
on_tool_call(tool):
  schema precheck → tools/call (timeout, cancel token) → truncate content → return
```

- 并发：沿用 05 §5.5.3 **单响应多 tool_call 串行**；同一 server 的多个调用串行；不同 server 不并行（v0.3 明确）。
- 重试：MCP 调用**不重试**（与 Python 工具口径一致，05 §5.3.5）；超时/崩溃作为工具错误返回 LLM。
- 预算：MCP 工具调用计入 AgentConfig `max_iterations`/token 预算；结果截断计入上下文。

## 5. 超时/取消/资源（冻结）

| 项 | 规则 |
|---|---|
| initialize | 10_000ms（可被 server `timeout_ms` 上限约束） |
| tools/list | 10_000ms |
| tools/call | server `timeout_ms`（缺省 30_000）；AgentConfig 可覆盖？v0.3 **不可**（保持简单，后续 DR） |
| 取消 | run 取消/节点超时 → 发 cancelled + 关闭连接/终止进程组；已返回结果丢弃 |
| 进程泄漏 | 每个 run 结束关闭本 run 启动的全部 server；异常路径同样 kill 进程组 |
| 输出上限 | 32768 字符/次；stderr 日志捕获 ≤ 8192 字符（不写 trace 全文） |

## 6. 安全信任模型

- `mcp_servers.json` 为**受信配置**（用户/管理员提供）；server 进程仍按不可信边界处理：
  - 输出作为 LLM 工具结果（外部数据），不提升为 system/指令；
  - 禁止把 env 密钥写入日志/trace/错误 message（仅写变量名）；
  - command/args/url 由配置提供，不接受来自图/LLM 的动态拼接；
  - HTTP 仅 https（推荐），http 需显式允许（v0.3 red line：只允许 https？推荐允许 http 并文档警示——Lead 裁决项）。
- 配置校验 fail-closed：未知 server/非法 allow/名称冲突 → 启动期（Harness Loading）错误。

## 7. trace 决策（默认不改 schema）

- **默认**：MCP 工具调用复用 `StreamingEvent::ToolCallStart/ToolCallEnd`（原有 `streaming_events`）与 `llm_calls` 汇总；`ToolCallStart.tool_name` = `mcp:<server>:<tool>`；参数/结果只记既有摘要字段（不新增列/表）。
- `events.kind` 不变；`trace`/`replay` 输出不变（既有工具事件路径）。
- **v7 候选（不在 v0.3）**：逐次 MCP 调用审计（server/tool/参数摘要/耗时/错误/截断标记）→ 新表 `mcp_tool_calls` 或扩展 `llm_calls`；需迁移/兼容设计，另立 DR。
- 安全：trace/日志不写 args/result 全文与密钥（与既有 tool 记录口径一致）。

## 8. 错误码（append-only，闭集）

| 码 | 触发 |
|---|---|
| `HARNESS_MCP_CONFIG` | registry/AgentConfig.mcp 非法（server 缺失/allow 非法/双传输/名称冲突/env 未定义） |
| `HARNESS_MCP_INIT` | initialize 失败/协议版本不兼容/超时 |
| `HARNESS_MCP_TOOL_NOT_FOUND` | 调用未注册/allowlist 外工具 |
| `HARNESS_MCP_CALL` | tools/call 失败/超时/崩溃/结果非法 |

- 映射为 `HarnessErrorKind`（05 §5.2.7）：配置类 → 节点 `Crashed`（`Config`）；调用类 → 工具错误返回 LLM（不 fail run）；进程崩溃由调用级错误承载。
- 既有码不变；新码只追加到 00 §0.4 Harness 域。

## 9. 兼容与性能

| 场景 | 期望 |
|---|---|
| 无 `mcp` 配置 | 不读 registry/不启动子进程/不注册工具；AgentConfig/run/trace 逐字节不变 |
| 有 MCP、LLM 未调用工具 | server 启动/initialize/tools-list 可观测（stderr），run 其余不变 |
| 默认路径性能 | 无 MCP 零开销（不 import client？Rust 编译期内含，但运行期无实例化/无 IO） |
| graph_hash | 不变（MCP 配置不参与 hash） |

## 10. AC-19（设计冻结；实测 V3）

| # | 用例 | 断言 |
|---|---|---|
| a | stdio initialize/tools-list/call | fake MCP server（NDJSON）→ 工具出现在请求 tools、调用返回结果、`mcp:<server>:<tool>` 命名 |
| b | Streamable HTTP | fake HTTP server（JSON/SSE）→ initialize/call 成功；TLS/URL 校验 |
| c | 工具映射/allowlist | 允许工具可见、未允许不可见/不可调用；名称冲突 fail-closed |
| d | 超时/取消/崩溃 | call 超时/run 取消/进程崩溃 → 受控工具错误、无进程泄漏（进程组 kill） |
| e | 结果截断/安全 | >32768 字符截断标记；env 密钥不落日志/trace；参数校验失败不调用 server |
| f | trace 默认不变 | MCP run 的 `events.kind`/既有 streaming/tool 记录不变；无 v7 表/列 |
| g | 默认无 MCP 不变 | 无 mcp 配置的图/AgentConfig/run/trace/replay 与 v0.2 golden 逐字节一致 |

> **AC-19 默认零配置路径**：每条用例均在无 `mcp` 配置对照下复跑（或由既有 v0.2 golden 覆盖）；无 MCP 路径零进程/零 IO/输出逐字节一致。

## 11. Lead 裁决（2026-11-01，已确认）

| # | 事项 | 裁决 |
|---|---|---|
| 1 | MCP client 库 | **rmcp**（feature-gated；默认构建不引入）；版本以 V3 spike 锁定；支持不足时允许自写最小 stdio fallback |
| 2 | HTTP 明文 | **默认只允许 https**；`http://` 需显式 `allow_insecure_http: true` + 文档警示 |
| 3 | trace schema | **默认不新增（不 v7）**：沿用 `llm_calls` + `streaming_events.tool_call_*`；server 身份经 `mcp:<server>:<tool>` 表达；审计增强另立 DR |
| 4 | tools/call 超时覆盖 | v0.3 **不可**由 AgentConfig 覆盖（server 级 `timeout_ms`） |
| 5 | 多 server 并发 | v0.3 **不并发**（沿用工具串行） |
| 6 | 默认零配置 | 不读 registry/不启动进程/不注册工具；run/trace/replay 与 v0.2 逐字节一致（AC-19 g + 既有 golden） |

> 本表取代原「待确认」状态；V3 实现必须覆盖 AC-19 a–g。
