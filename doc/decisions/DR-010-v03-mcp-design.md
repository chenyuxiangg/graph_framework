# DR-010 · MCP 设计冻结（tools：stdio + Streamable HTTP，双 era）

> **状态**：V1 设计冻结（REV-096；arch-researcher 结论已合入）；实现见 DR-008 V3（`feat/harness-mcp`）。
> **日期**：2026-11-01 · **作者**：docs-dev · **关联**：DR-008、05 §5.6/§5.10、06 §6.14、07 AC-19、[`research-v03-capabilities.md`](../research-v03-capabilities.md)
> **规范目标**：**双 era 必须**——`2026-07-28`（latest，无状态）与 `2025-11-25`（legacy initialize）；生态仍以旧版为主，只做新版会大面积连不上。

## 0. 摘要

| 项 | 冻结 |
|---|---|
| client | Rust **rmcp 3.8.0**（Apache-2.0；MSRV 1.88/edition 2024），feature-gated；`ClientLifecycleMode` 双版本（preferred `2026-07-28` + legacy `2025-11-25`）；支持不足允许自写最小 stdio fallback |
| 传输 | stdio + Streamable HTTP（rmcp `transport-child-process`/`transport-streamable-http-client-reqwest`；旧 HTTP+SSE 不支持） |
| registry | `<assets-dir>/mcp_servers.json`（受信配置；不允许来自 DSL/graph）；需 `--allow-mcp` 显式开启 |
| AgentConfig | `mcp: {servers: string[], allow?: string[]}`（additive；缺省=不启用）；MCP 工具**必须显式进 `AgentConfig.tools`** |
| 工具命名 | **`mcp__<server>__<tool>`**（`serverInfo.name` 跨 server 不唯一/LLM function name 安全）；`call_id` 保留 provider/LLM 原生 id（**不伪造 `#n`**）；MCP/skill 可识别性只用 `tool_name` |
| 生命周期 | `2026-07-28`：无 session/initialize，每请求 `params._meta` 版本协商；HTTP 头 `MCP-Protocol-Version`+`Mcp-Method`/`Mcp-Name`；`2025-11-25`：initialize + `notifications/initialized` + `Mcp-Session-Id`（由 rmcp 兼容） |
| 调用 | `tools/list`（分页/`ttlMs` 提示）；`tools/call`；rmcp 自动 MRTR（默认 10 轮）；**无 per-call 超时**→ 自包 `tokio::time::timeout` |
| `input_required` | v0.3 不支持：`clientCapabilities={}`；收到 `resultType:"input_required"` → 结构化工具失败 `input_required_unsupported`（不静默/AI 代答） |
| 超时/并发 | initialize/tools-list 10s；call=server `timeout_ms`（缺省 30s）；HTTP client 默认 16 并发、`control_request_timeout=5s`；框架层工具执行仍**串行** |
| 鉴权 | **仅静态 env Bearer**（headers 从 env 展开；不落盘/日志/trace）；**不做 OAuth/DCR** |
| feature | `mcp` optional feature；**CLI release 构建默认开启**（`--no-default-features` 可关；CI 双构建） |
| trace | **不新增 schema（不 v7）**：沿用 `llm_calls` + `streaming_events` 的 `tool_call_*`；无 MCP 默认路径逐字节不变 |
| 默认无 MCP/未开启 | 不读 registry/不启动进程/不注册工具 → 既有行为不变 |

## 1. 双 era 生命周期（冻结）

### 1.1 `2026-07-28`（新，无状态）

- **无协议级 session**：删除 `Mcp-Session-Id`、`initialize`/`notifications/initialized`；`tools/list` 不再按连接变化。
- **每请求版本协商**：`params._meta` 带 `io.modelcontextprotocol/protocolVersion`、`clientCapabilities`（SHOULD `clientInfo`）；结果 `serverInfo`。版本不匹配 → `UnsupportedProtocolVersionError`（`-32022`）。
- HTTP：单一 endpoint POST；`Accept: application/json, text/event-stream`；头 `MCP-Protocol-Version`（必须与 body `_meta` 一致，否则 400 `-32020 HeaderMismatch`）、`Mcp-Method`、必要时 `Mcp-Name`；取消=关闭 SSE 流（HTTP 不发 cancelled）；**`Last-Event-ID` 不支持**。
- 可选 `server/discover`（MUST 实现）；v0.3 用「现代请求失败→回落 legacy」探测，不强制前置。
- **MRTR（Multi Round-Trip Requests）**：server 返回 `InputRequiredResult`（`resultType:"input_required"`，`inputRequests`）→ 客户端带 `inputResponses` 重试（新 id）。v0.3 只支持「工具不产生 input_required」：不声明 elicitation/sampling/roots，收到即结构化失败。
- **Deprecated**：Sampling/Roots/Logging 整体弃用；旧 HTTP+SSE 传输弃用；`resources/subscribe` → `subscriptions/listen`（v0.3 不订阅）；Tasks 移出核心扩展（v0.3 不支持）。

### 1.2 `2025-11-25`（legacy，兼容）

- client MUST `initialize` → server 返回 → client `notifications/initialized`；HTTP 后续请求带 `MCP-Protocol-Version` 头；`Mcp-Session-Id` 由 rmcp session recovery 处理。
- Python 侧 `initialize` 与 capability 协商由 rmcp legacy 模式负责；v0.3 不手写。

> **双 era 强制**：同一 server 先按 `2026-07-28` 语义探测；失败/错误体显示 legacy → 回落 `2025-11-25` 握手。rmcp 3.8.0 `ClientLifecycleMode` 原生支持；e2e 必须各放一个新/旧桩 server（AC-19 a）。

## 2. 配置与开关

### 2.1 `mcp_servers.json`（registry）

```json
{
  "mcpServers": {
    "github": {"command":"npx","args":["-y","@modelcontextprotocol/server-github"],
               "env":{"GITHUB_TOKEN":"${GITHUB_TOKEN}"},"timeout_ms":30000},
    "remote": {"url":"https://example.com/mcp","allow_insecure_http":false,
               "headers":{"Authorization":"Bearer ${MCP_TOKEN}"},"timeout_ms":30000}
  }
}
```

- 位置：`<assets-dir>/mcp_servers.json`；缺失=无 server（无引用不报错）。
- server 名 `^[a-z0-9][a-z0-9_-]{0,63}$`；value 恰好 `command`（stdio）或 `url`（HTTP）之一；env/headers 支持 `${ENV}`（未定义 → 配置错误，不落日志）。
- **仅静态 env Bearer**：headers 从 env 展开；`mcp_servers.json` 不写明文密钥；不实现 OAuth/DCR。
- HTTP 默认只允许 `https`；`http://` 需显式 `allow_insecure_http: true` + 文档警示且仅建议 loopback。

### 2.2 AgentConfig（additive）

```json
{"agent_id":"triager",
 "mcp":{"servers":["github"],"allow":["mcp__github__list_issues","mcp__github__get_issue"]},
 "tools":["mcp__github__list_issues","mcp__github__get_issue","read_file"]}
```

- `mcp?: {servers: string[], allow?: string[]}`；缺省无=不启用（不读 registry/不启进程/不注册工具）。
- server 不存在 → `HARNESS_MCP_ERROR{reason:"server_not_found"}`；allow 非法/未匹配 → 配置错误（fail-closed）。
- **白名单**：MCP 工具必须在 `AgentConfig.tools` 显式列出（与「已注册但未列出 → `HARNESS_UNKNOWN_TOOL`」一致）；`allow` 进一步收窄（交集）；否则不可见/不可调用。
- **`--allow-mcp` 显式开关（Lead 裁决）**：默认关闭；`AgentConfig.mcp` 非空但未传 `--allow-mcp` → 受控错误 **`HARNESS_MCP_ERROR{reason:"not_allowed"}`**（不新增 MCP_NOT_ALLOWED 码）（**冻结：run 启动前失败、不产生 run、不启动 server；CLI exit 3，沿用 `RUN_ENTRY_INVALID` 入口校验口径**；AC-19 c 断言）。开启时 stderr 打印所用 server 完整命令（不含密钥/env 值）。
- 名称冲突（与内置/Python 工具同名）→ `HARNESS_MCP_ERROR{reason:"name_conflict"}`。

## 3. 工具映射与调用

| MCP | ToolDef |
|---|---|
| name | `mcp__<server>__<tool>`（示例 `mcp__github__list_issues`） |
| description/title | MCP description/title（截断 1024） |
| parameters | `inputSchema`（JSON Schema 2020-12；非法/超大 → 跳过该工具 + warn） |
| output | `content` 文本块拼接 + `structuredContent` 摘要；`isError:true` → 工具结果（喂回 LLM） |
| annotations | **不可信**（不据此跳过权限/校验） |
| 超时 | `timeout_ms`（server 级） |
| 结果截断 | 32768 字符（附 `truncated:true`） |
| MRTR | `call_tool()` 自动重试（默认 10 轮）；`input_required` → 结构化失败 `input_required_unsupported` |

- `tools/list` 分页（`nextCursor`）；run 级缓存（`ttlMs` 为提示；不实现 `list_changed` 订阅，下一 run 重拉）。
- 参数校验（object/required/type）不通过 → 工具错误，不调用 server。
- **协议错误 vs 工具错误分开映射**：JSON-RPC error（-32602/-32601/-32020/-32021/-32022）→ 工具错误文本（可给模型但难自愈）；`isError` → 工具结果喂回。
- 调用必须包 **`tokio::time::timeout`**（rmcp 无 per-call 超时）；取消发 `notify_cancelled`（stdio）/关闭 SSE（HTTP）。

## 4. 进程/连接生命周期与并发

- **stdio**：每 run 每 server 一个常驻子进程（首次使用懒启动；`npx`/`uvx` 冷启动秒级）；spawn 设进程组（`setpgid`/`process-wrap`），run 结束/取消 kill **进程组**（避免 `npx→node` 孙进程残留；macOS 需 spike 验证）。
- **stderr 必须持续读**（有上限 8192 字符）避免 pipe 满阻塞；不写 trace 全文。
- **崩溃**：检测 exit → server `unavailable`；同 run 最多重启 **1 次**，再失败 → 工具调用失败（不无限重启）。
- **HTTP**：`2026-07-28` 无 session；legacy `Mcp-Session-Id` 由 rmcp recovery 处理；默认 16 并发 POST（`max_concurrent_requests`），`control_request_timeout=5s`。
- **框架层执行**：单响应多 tool_call 仍**串行**（05 §5.5.3）；MCP client 内部并发不改变工具执行顺序；后续 DR 再开并行。
- **失败隔离**：MCP 调用失败 → 工具级错误（不 fail run）；`tool_call_end{ok:false}` 遥测保留；仅配置/未授权/名称冲突在 Loading 失败。

## 5. 超时/取消/资源（冻结）

| 项 | 规则 |
|---|---|
| initialize（legacy）/discover | 10_000ms |
| tools/list | 10_000ms |
| tools/call | server `timeout_ms`（缺省 30_000）；**无 AgentConfig per-call 覆盖** |
| MRTR 轮数 | rmcp 默认 10；v0.3 不调高 |
| 取消 | stdio：`notifications/cancelled` + 关闭 stdin；HTTP：关闭 SSE；超时/run 取消 → kill 进程组 |
| 输出 | 结果 32768 字符/次；stderr 8192；事件/日志不写全文 |
| 进程回收 | 每 run 结束关闭本 run 启动的 server；异常路径同样 kill 进程组 |

## 6. 安全信任模型

- registry 为受信配置，但 server 进程仍视为不可信边界：工具输出作为外部数据、annotations 不可信、不提升为 system/指令。
- **命令即 RCE**：`command`/`args` 来自资产配置（非 DSL/LLM）；`--allow-mcp` 显式开启；stderr 打印完整命令（不含密钥）供审计；不实现一键配置/下载。
- 鉴权仅静态 env Bearer；不传 token 到日志/trace/错误 message（只写变量名）；OAuth/DCR 不做。
- HTTP 默认 https；`http://` 仅显式 `allow_insecure_http:true` 且建议 loopback；校验 `Content-Type`/Origin 由 server 侧负责（客户端只连配置 URL）。
- 输出截断/大小限制防 DoS；路径/参数不走文件系统。

## 7. trace 决策（不新增 schema）

- **不升 v7**：MCP 工具调用沿用 `streaming_events` 的 `tool_call_start/tool_call_end` + `llm_calls` 汇总；`tool_name`=`mcp__<server>__<tool>`；`call_id` 保留 provider/LLM 原生 id（不伪造；MCP 可识别性只用 tool_name）。
- 不写新表/列；`events.kind` 不变；`trace`/`replay` 输出沿用既有路径。
- 安全/膨胀：只记 args_summary/result_summary（既有上限），不写参数/结果全文与密钥。
- **v7 候选（不在 v0.3）**：按 server 维度健康/延迟/失败率聚合（`mcp_servers` 表或 `streaming_events.server` 列）、builtin vs external provider 列；需另立 DR。

## 8. 错误码（REV-096 Lead 裁决 A：单一 `HARNESS_MCP_ERROR`）

| 码/载体 | 触发 | detail.reason |
|---|---|---|
| `HARNESS_MCP_ERROR`（`HarnessErrorKind::Mcp`） | MCP 配置/开关/lifecycle/崩溃（不用于工具调用失败） | `server_not_found`/`not_allowed`（未传 `--allow-mcp`）/`config_invalid`（registry/AgentConfig/双传输/名称冲突/env 未定义）/`init_failed`/`protocol_version`/`tool_not_found`（未注册或不在 tools/allow）/`name_conflict`/`http_insecure`/`input_required_unsupported` |
| `TOOL_ERROR` | **MCP 工具调用失败**（超时/崩溃/isError/参数校验/协议 -326xx） | 喂回 LLM；不新增 MCP 调用码 |

- 映射：MCP_ERROR → 节点 `Crashed(HarnessErrorKind::Mcp)`（配置/开关/初始化阶段）；`TOOL_ERROR` → 工具结果、run 继续。
- 取消仍走 `NodeError::Cancelled`；节点超时 `NODE_TIMEOUT`。
- 既有码不变；只新增 `HARNESS_MCP_ERROR` 一个码位（00 §0.4）。

## 9. 兼容/性能

| 场景 | 期望 |
|---|---|
| 无 `mcp` 配置 | 不读 registry/不启进程/不注册工具；AgentConfig/run/trace/replay 逐字节不变 |
| 有 `mcp` 但无 `--allow-mcp` | `HARNESS_MCP_ERROR{reason:"not_allowed"}`；**run 启动前失败、不产生 run、CLI exit 3** |
| 仅 legacy server | 双 era 回落成功（AC-19 a） |
| 默认路径性能 | 无 MCP 不实例化 client、无 IO；`mcp` feature 默认开但运行时零开销 |
| graph_hash | 不变（MCP 配置不参与 hash） |

## 10. AC-19（设计冻结；实测 V3）

| # | 用例 | 断言 |
|---|---|---|
| a | 双 era stdio | 新（`2026-07-28` 无状态）/旧（`2025-11-25` initialize）两个 fake server 均能 list/call；现代失败→legacy 回落 |
| b | Streamable HTTP | fake HTTP（JSON/SSE）：头 `MCP-Protocol-Version`/`Mcp-Method` 校验；https；`allow_insecure_http` 门控 |
| c | 映射/白名单/allow-mcp | `mcp__<server>__<tool>` 命名；必须在 `tools`+`allow`；未在 tools 不可见；无 `--allow-mcp` → `HARNESS_MCP_ERROR{reason:"not_allowed"}`（run 启动前失败、不产生 run、exit 3） |
| d | 超时/取消/崩溃/进程组 | call 超时（自包 timeout）/取消/崩溃 → 工具错误；进程组 kill 无泄漏；同 run 最多重启 1 次 |
| e | 结果/安全/MRTR | >32KiB 截断；annotations 不可信；`input_required` → `input_required_unsupported`；密钥不落日志/trace |
| f | trace 不变 | 不新增表/列/`events.kind`；`tool_call_*` + `llm_calls`；`call_id` 保留原生 id（只用 `tool_name=mcp__<server>__<tool>` 识别） |
| g | 默认无 MCP 不变 | 无 mcp 配置/未开启：不读 registry/不启进程/不注册工具；run/trace/replay 与 v0.2 golden 逐字节一致 |

> **AC-19 默认零配置路径**：每条用例均在无 `mcp` 配置对照下复跑（或由既有 v0.2 golden 覆盖）；无 MCP 零进程/零 IO。

## 11. Lead 裁决（REV-096 A–D 范围；2026-11-01 已确认）

> 来源：本表仅列 REV-096 A–D 裁决范围；超出 A–D 的设计项为 V1 冻结推荐（非 Lead 裁决），实现前可由评审再议。

| # | 事项 | 裁决 |
|---|---|---|
| 1 | 双 era | **必须**（`2026-07-28` + `2025-11-25`；rmcp `ClientLifecycleMode`） |
| 2 | client | **rmcp 3.8.0** feature-gated；支持不足允许最小 stdio fallback（V3 spike） |
| 3 | 命名 | **`mcp__<server>__<tool>`**；`call_id` 保留原生 id（不伪造）；MCP/skill 可识别性只用 `tool_name` |
| 4 | 白名单 | MCP 工具**必须显式进 `AgentConfig.tools`**；`allow` 交集 |
| 5 | HTTP | 默认 **https**；`http://` 需 `allow_insecure_http:true` + 警示 |
| 6 | 鉴权 | **仅静态 env Bearer**；不做 OAuth/DCR |
| 7 | feature | `mcp` optional；**CLI release 默认开启**；`--no-default-features` 可关；CI 双构建 |
| 8 | `--allow-mcp` | **v0.3 纳入**：默认关闭；未开启时 `HARNESS_MCP_ERROR{reason:"not_allowed"}`；开启 stderr 打印无密钥命令 |
| 9 | `input_required` | **不支持**，结构化失败 `input_required_unsupported` |
| 10 | trace | **不 v7**；沿用 `llm_calls`+`tool_call_*`；审计增强另立 DR |
| 11 | 并发/超时 | 框架工具串行；HTTP client 16/`control_request_timeout` 5s；call 无 per-call 覆盖、自包 timeout |

## 12. 实现执行记录（as-built，2026-11-02，REV-101；只追加）

- **分支/提交**：`feat/harness-mcp`：`18bf405`（client/桥接）+ `815abea`（run 末关闭）+ `15f3d18`（C1/C2：per-agent plan/allow、错误码收敛、脱敏、函数名）+ `23fa568`（server 段归一化）；评审 REV-098（含跨模块 Lead 第二评审）；合并 `23fa568`。
- **实测**：harness **235/0/1**、`--features mcp` **235/0/1**；e2e 137/0/2；workspace **1037/0/3**；`--no-default-features` 构建通过；rmcp **3.8.0 feature-gated**（默认构建不引入，CLI release 默认开）；默认无 MCP 零进程/零 IO。
- **实现口径/偏差**：`HARNESS_MCP_ERROR{reason}` 实现 `config_invalid`/`not_allowed`/`server_not_found`/`init_failed`/`protocol_version`/`tool_not_found`/`name_conflict`/`http_insecure`/`input_required_unsupported`（工具调用失败仍 `TOOL_ERROR`）；`--allow-mcp` 未开启 → run 启动前失败/不产生 run/exit 3；stdio server 每 run 常驻、进程组 kill、同 run 最多重启 1 次；`mcp__<server>__<tool>`、必须显式进 `tools`；`call_id` 保留原生；trace 不 v7；常量为 as-built 名单（00 §0.5）。
- **已知限制**：真实 stdio/HTTP 传输/进程组/超时由 V5a 覆盖；cwd/assets-dir、allow 长度、`__` server 名、长函数名跳过、HTTP URL 脱敏等 P2/P3 入台账（ISSUE-156+）；AC-19 V5a 实测 11 用例已回写 07。
