# DR-011 · 可插拔 harness 设计冻结（AgentProvider seam + 子进程协议）

> **状态**：V1 设计冻结（REV-096；arch-researcher 结论已合入）；实现见 DR-008 V4（`feat/harness-plugin`）。
> **日期**：2026-11-01 · **作者**：docs-dev · **关联**：DR-008、05 §5.1.3/§5.9.4/§5.10、03（超时/取消/进程组）、06（L2）、07 AC-20、[`research-v03-capabilities.md`](../research-v03-capabilities.md)
> **用户/Lead 决策**：**子进程 adapter + Rust trait seam**；Codex 先、DSH ACP best-effort；外部 harness 不写 trace/状态机；真实适配器不阻塞发布。

## 0. 摘要

| 项 | 冻结 |
|---|---|
| seam | `AgentProvider`（Rust trait）实际选择：`builtin`（默认，行为不变）| `subprocess`；Codex/DSH 为**外部命令 + 薄 adapter** |
| 配置 | `AgentConfig.harness?: {kind:"builtin"} | {kind:"subprocess", command, args?, protocol:"zgraph-harness/1", env?, timeout_ms?}`（additive） |
| 进程模型 | **每 run 每 provider 一个常驻 adapter 进程**；每节点一次 prompt；attempt 重试=新 session；`agent_id` 变化=新 session；run 结束/取消 kill 进程组 |
| 协议 | NDJSON over stdio：`hello/hello_ok` → `run` → `event`（token/reasoning/tool_start/tool_end/progress 子集）→ `result` / `error` / `cancel` |
| 适配器 | **Codex 先**（`codex exec --json` / `--output-last-message`）；**DSH ACP best-effort**（`dsh --profile acp`；headless `--json`/sdk 不可依赖） |
| 硬约束 | 外部 harness **不得**写 trace DB/状态机；只能经 I/O 契约；不双重写 `llm_calls`；不传 tools/skills/mcp/max_iterations（冲突 → `provider_capability_conflict`） |
| 取消/超时 | 关闭 stdin → SIGTERM 2s → SIGKILL 进程组；超时走 `NODE_TIMEOUT` 语义；适配器自身 +5s 兜底 |
| 输出上限 | 单行/final ≤1 MiB；总输出 ≤8 MiB；事件数 ≤20000；stderr ≤8 KiB |
| StopReason | **不新增枚举**：Codex/DSH 映射用既有 `Other{raw}`；`iterations` 固定 **1**（文档降级） |
| usage | `cached_input_tokens`/`cache_write_input_tokens` **v0.3 丢弃**（文档注明）；`reasoning_tokens` 可映射 |
| 二进制 | **用户自备**（zgraph 不下载/不内嵌）；无环境则 release notes 标注「adapter 骨架，待真实环境验证」 |
| L2 | **不重跑**：`--mode=outputs` 用 `recorded_outputs`；events 重放不启动进程/网络 |
| 默认 builtin | 无 `harness` 配置 → 现有 `HarnessExecutor` 路径逐字节不变 |

## 1. `AgentProvider` seam

```rust
#[async_trait::async_trait]
pub trait AgentProvider: Send + Sync {
    fn kind(&self) -> AgentProviderKind;  // Builtin | Subprocess（v0.3 实现值）
    async fn run(&self, agent: &AgentConfig, inputs: Value, ctx: &NodeContext)
        -> Result<AgentOutput, HarnessError>;
}
```

- 选择：`AgentConfig.harness.kind`（缺省 `builtin`）；未知 kind/配置非法 → `HARNESS_CONFIG_LOAD`。
- `Builtin`：现有 `HarnessExecutor::run_agent`（05 §5.5.4）；trait 化不改变行为/输出。
- `Subprocess`：`SubprocessHarness` 启动/复用 adapter，按协议交互；`final/result` 构造 `AgentOutput` 四字段。
- **外部模式能力约束**：不传 `tools`/`skills`/`mcp`/`max_iterations`（外部 harness 拥有自己的 loop）；若 AgentConfig 同时配置这些 → `HARNESS_CONFIG_LOAD{reason:"provider_capability_conflict"}`（fail-closed，不静默忽略）。

## 2. 子进程协议 `zgraph-harness/1`（NDJSON over stdio）

### 2.1 消息

```jsonc
// 父→子
{"v":1,"type":"hello","protocol":"zgraph-harness/1","run_id":"...","node_id":"...",
 "provider":"codex_exec|acp","limits":{"node_timeout_ms":180000,"max_output_bytes":1048576,"max_events":20000}}
{"v":1,"type":"run","agent":{"id":"reviewer","model":"...","temperature":0.2,"max_tokens":2048},
 "session":{"reuse_key":"<run_id>:<agent_id>","reset":false},"inputs":{...},"trace":{"run_id":"...","instance_id":"...","attempt":1}}
{"v":1,"type":"cancel"}
// 子→父
{"v":1,"type":"hello_ok","protocol":"zgraph-harness/1","provider":"codex_exec","capabilities":{"streaming":true,"reasoning":false}}
{"v":1,"type":"event","kind":"delta","text":"..."}                        // → TokenDelta（Codex 仅整消息一次；DSH committed 消息）
{"v":1,"type":"event","kind":"reasoning","text":"..."}                    // → ReasoningDelta（无则不发）
{"v":1,"type":"event","kind":"tool_start","call_id":"c1","tool":"...","args_summary":{}}
{"v":1,"type":"event","kind":"tool_end","call_id":"c1","ok":true,"result_summary":{}}
{"v":1,"type":"event","kind":"progress","percent":40,"message":"..."}
{"v":1,"type":"result","content":"...","iterations":1,"stop_reason":"EndTurn","usage":{...}}
{"v":1,"type":"error","code":"HARNESS_...","message":"...","retryable":false}
```

- 只映射 **现有 5 个 `StreamingEvent` kind**（token/reasoning/tool_start/tool_end/progress）；`plan`/`usage_update` 不引入新 kind（`plan`→progress，`usage_update`→最终 usage）。
- `result.content` 为最终消息（Codex 用 `--output-last-message` 文件优先，stdout 解析兜底）；`iterations` 固定 1（降级，DR 写明）；`stop_reason` 映射见 §3。
- 行 ≤1 MiB；JSON 非法/未知 `type` → `HARNESS_PLUGIN_ERROR{reason}`；未知 `event.kind` 忽略+warn（向前兼容）。

### 2.2 握手/取消/退出

- `hello/hello_ok` 超时 5s、协议版本不匹配 → `HARNESS_PLUGIN_ERROR{reason:"handshake"|"version"}`。
- 取消：父进程发 `cancel` 控制帧；adapter 对 Codex 发 SIGTERM（2s→SIGKILL 进程组），对 ACP 发 `session/cancel` 并等 `cancelled`（≤5s）。
- 子进程非 0/无 result → `HARNESS_PLUGIN_ERROR{reason:"exit_nonzero"}`；stderr 截断 8 KiB（不写 trace 全文/密钥）。
- 超时：节点 `timeout_ms`（zgraph 硬 deadline）先到 → 取消 + `NODE_TIMEOUT` 语义；adapter 自身 +5s 兜底防孤儿。

## 3. Codex / DSH 适配边界

### 3.1 Codex（先做）

- 依赖面：`codex exec --json`（JSONL `ThreadEvent`：thread.started/turn.completed/turn.failed/item.*）+ `-o/--output-last-message` 读最终消息；`--skip-git-repo-check`/`-C`/`-s`/`--ephemeral` 按需。
- **无 token 级 delta**：`agent_message` 只在 item started/updated/completed 给整段文本 → `capabilities.streaming=false`（无 token 级）；adapter 仍可发**一条整消息 `delta`**（语义为「一条消息一次」，非 token 级），文档写死。
- `iterations` 无对应物 → **固定 1**（降级）；trace summary 记 `event_count`/`item_count`。
- 退出码只有 0/1（无区分度）→ 成功需结合 `turn.completed`/`turn.failed`，不能只看 code。
- usage：`input_tokens→prompt_tokens`、`output_tokens→completion_tokens`、`reasoning_output_tokens→reasoning_tokens`；`cached_input_tokens`/`cache_write_input_tokens` **丢弃**（文档注明）。
- `stop_reason`：`turn.completed`→`EndTurn`；`turn.failed`→`Other{"turn_failed"}`；kill→`Other{"timeout"|"cancelled"}`；schema 解析失败→`Other{"schema_violation"}`；**不新增 StopReason 枚举**。

### 3.2 DSH / ACP（best-effort）

- **可依赖入口仅 `dsh --profile acp`**（ACP v1 stdio：initialize/session/new/session/prompt/session/cancel/session/update）。
- **ACP 协议有 chunk 通道**（`agent_message_chunk`/`agent_thought_chunk`），**但 DSH 只发已提交（committed）消息**（每条消息一次）→ `delta` 粒度粗；`capabilities.streaming` 可为 true 但注明粒度。
- headless `--json` 事件 schema 未公开、`--profile sdk` wire 不可考 → **不写入硬依赖**；仅作为后续 spike。
- `StopReason`：ACP `end_turn`/`max_tokens`/`max_turn_requests`/`refusal`/`cancelled` 映射到既有枚举；`max_turn_requests`/`refusal`→`Other{raw}`。
- 权限请求（`session/request_permission`）v0.3 不实现自动应答 → 若无交互能力，adapter 返回结构化错误。

### 3.3 05 §5.9.4 修正（ACP）

> 旧结论「ACP 只上线语义事件、拿不到 token delta」**修正**：ACP 协议层有 `agent_message_chunk`/`agent_thought_chunk`；DSH 服务端按设计只发 **committed 消息**（raw provider deltas 不上线）。因此「无逐 token delta」是 DSH 实现取舍，不是 ACP 协议缺失；v0.3 按「一条消息一次 delta」降级。

## 4. trace / L2（不双重记录）

- 外部 harness 只能经协议事件；事件映射到既有 `StreamingEvent` 由框架 sink 记录。**禁止** adapter/harness 直接写 trace DB/状态机/`llm_calls`（硬约束）。
- 每次节点执行由框架写 1 条 `llm_calls`：`request_json={provider,agent_id,session_key,inputs_hash}`、`response_json=四字段+event_count`；不把原始 JSONL 全量塞进 trace（需日志另落 `logs/`，v0.3 不做/或仅 stderr 截断）。
- `events.kind` 不变；无 harness 配置输出/回放逐字节不变。
- L2：`--mode=events` 重放既有事件、不启动外部进程；`--mode=outputs` 用 `recorded_outputs`（UNIQUE(run_id,instance_id,attempt) 已有）还原四字段；**不重跑外部命令**、跨无命令环境可用；缺失 → 既有 `REPLAY_RECORD_MISSING`。

## 5. 安全

- `command`/`args` 由 AgentConfig（受信配置）提供；只 spawn 用户预装二进制；不接受图/LLM 动态拼接；cwd=`<assets_dir>`；env 白名单透传。
- 进程组隔离/终止；超时/输出/事件数上限；stderr 截断；不写密钥/token 到日志/trace。
- 外部输出视为不可信：只接受协议帧（JSONL），非法行 warn 计数、超阈值终止；`content` 做 UTF-8/大小校验后进 messages。
- 不读外部 harness 自身 session 文件（Codex `~/.codex`/DSH `$DSH_HOME`）；全部可观测经框架事件。

## 6. 错误码（REV-096 Lead 裁决 A：单一 `HARNESS_PLUGIN_ERROR`）

| 码/载体 | 触发 | detail.reason |
|---|---|---|
| `HARNESS_PLUGIN_ERROR`（`HarnessErrorKind::Plugin`） | 外部 harness spawn/握手/协议/结果/崩溃/**超时** | `spawn_failed`/`handshake`/`version`/`protocol`/`result_invalid`/`line_too_large`/`output_limit`/`event_limit`/`exit_nonzero`/`timeout` |
| `HARNESS_CONFIG_LOAD{reason}` | `harness` 配置非法/未知 kind/缺 command/能力冲突（`provider_capability_conflict`） | —（不新增 PLUGIN_CONFIG 码） |
| `NODE_TIMEOUT` | 节点硬 deadline 到期（zgraph 侧） | 取消仍 `NodeError::Cancelled` |

- **无 `HARNESS_PLUGIN_TIMEOUT`**：协议级/子进程超时 → `HARNESS_PLUGIN_ERROR{reason:"timeout"}`；节点超时仍 `NODE_TIMEOUT`。
- 映射：`HARNESS_PLUGIN_ERROR` → 节点 `Crashed(HarnessErrorKind::Plugin)`；配置 → `Crashed(Config)`。
- 既有码不变；只新增 `HARNESS_PLUGIN_ERROR` 一个码位（00 §0.4）。

## 7. 兼容与性能

| 场景 | 期望 |
|---|---|
| 无 `harness` | builtin；AgentConfig/run/trace/replay 与 v0.2 逐字节不变 |
| 显式 `kind:"builtin"` | 等义（配置不参与 hash） |
| 外部 harness L2 | 不启动进程；回放结果一致 |
| 默认性能 | 无配置零额外进程/IO；builtin 路径无变化 |

## 8. AC-20（设计冻结；实测 V4）

| # | 用例 | 断言 |
|---|---|---|
| a | builtin 默认不变 | 无 harness/显式 builtin：AgentConfig/run/trace/replay 与 v0.2 golden 逐字节一致 |
| b | subprocess 协议 | fake external：hello/hello_ok → run → result；四字段正确；版本/行超限 fail-closed |
| c | 流式映射与降级 | delta/reasoning/tool_start/tool_end/progress → 既有 StreamingEvent；Codex 整消息一次/DSH committed；未知 event 忽略+warn |
| d | 取消/超时/进程组 | cancel 帧 → SIGTERM 2s→SIGKILL 进程组；超时 NODE_TIMEOUT；无孤儿 |
| e | 输出/协议错误/安全 | 行/总输出/事件超限、非法 JSON/无 result、非 0 退出 → `HARNESS_PLUGIN_ERROR`；stderr 截断、无密钥 |
| f | L2 不重跑 | `--mode=events/outputs` 不启动外部进程；无命令环境可回放；结果一致 |
| g | fake external 真链路 | e2e：真 CLI + fake external harness 可执行 → run/trace/replay 全链路 |
| h | Codex/DSH 边界 | Codex 先（`--output-last-message`/iterations=1/cached 丢弃）；DSH ACP best-effort（committed 消息/权限请求不支持）；不双重 trace/状态机/llm_calls |

> **AC-20 默认零配置路径**：每条用例均在无 `harness`/显式 `builtin` 对照下复跑（或由既有 v0.2 golden 覆盖）；builtin 路径逐字节一致。

## 9. Lead 裁决（REV-096 A–D 范围；2026-11-01 已确认）

> 来源：本表仅列 REV-096 A–D 裁决范围；超出 A–D 的设计项为 V1 冻结推荐（非 Lead 裁决），实现前可由评审再议。

| # | 事项 | 裁决 |
|---|---|---|
| 1 | 协议 | `zgraph-harness/1`（`v:1` 双写）；版本不匹配 fail-closed |
| 2 | 进程模型 | **每 run 常驻 adapter**；attempt/agent 变化 → 新 session |
| 3 | 外部能力约束 | 不传 tools/skills/mcp/max_iterations；冲突 → `HARNESS_CONFIG_LOAD{reason:"provider_capability_conflict"}` |
| 4 | 真实适配器 | **不阻塞发布**：V4 交付 seam+协议+fake external e2e+Codex/DSH 骨架（best-effort smoke；无环境 release notes 标注）；**Codex 先，DSH ACP 后** |
| 5 | 取消/超时 | stdin→SIGTERM 2s→SIGKILL 进程组；节点超时走 `NODE_TIMEOUT`；adapter +5s 兜底 |
| 6 | 输出常量 | 00 §0.5：行/final 1 MiB、总输出 8 MiB、事件 20000、stderr 8 KiB、默认超时 300s、握手 5s |
| 7 | StopReason | **不新增枚举**；Codex/DSH 映射 `Other{raw}`；`iterations=1` 文档降级 |
| 8 | cached tokens | `cached_input_tokens`/`cache_write_input_tokens` **v0.3 丢弃**（文档注明） |
| 9 | 二进制来源 | **用户自备**（不下载/不内嵌）；文档写清 |
| 10 | trace | 只由框架写；**禁止双写 `llm_calls`**；events.kind 不变 |
| 11 | L2 | 不重跑外部进程；`recorded_outputs` + 既有事件 |
