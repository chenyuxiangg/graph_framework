# DR-011 · 可插拔 harness 设计冻结（AgentProvider seam + 子进程协议）

> **状态**：V1 设计冻结；实现见 DR-008 V4（`feat/harness-plugin`）。
> **日期**：2026-11-01 · **作者**：docs-dev · **关联**：DR-008、05 §5.1.3/§5.9.4/§5.10、03（超时/取消/进程组）、06（L2）、07 AC-20、[`research-v03-capabilities.md`](../research-v03-capabilities.md)
> **用户决策**：**子进程 adapter + Rust trait seam**；Codex/DSH 只做薄适配器；外部 harness 拥有自己的 loop，框架不双重 trace/状态机。

## 0. 摘要

| 项 | 冻结 |
|---|---|
| seam | `AgentProvider` 由扩展位升级为**实际选择器**（Rust trait）：`Builtin`（默认）与 `Subprocess` |
| 配置 | `AgentConfig.harness?: {kind:"builtin"} | {kind:"subprocess", command, args?, protocol:"zgraph-harness/1", env?, timeout_ms?}`（additive） |
| 协议 | NDJSON over stdio；`hello`（协议/版本协商）→ `run` → `event`（streaming 子集）→ `final`（四字段 output）→ 退出 |
| 流式映射 | 仅 `token_delta`/`reasoning_delta`/`tool_call_start`/`tool_call_end`/`progress` 子集（05 §5.3.2 `StreamingEvent`） |
| 取消/超时 | 父进程关闭 stdin/SIGTERM→SIGKILL 进程组（03 §3.9.4）；超时按 `timeout_ms`/节点超时 |
| 输出上限 | final content ≤ 1 MiB；事件行 ≤ 1 MiB；stderr 捕获 ≤ 8 KiB（不落 trace 全文） |
| L2 | 外部 harness **不重跑**：`--mode=outputs` 用 `recorded_outputs`；`--mode=events` 重放既有事件（不启动外部进程/网络） |
| 默认 builtin | 无 `harness` 配置 → 现有 `HarnessExecutor` 路径逐字节不变 |

## 1. `AgentProvider` seam（Rust）

```rust
#[async_trait::async_trait]
pub trait AgentProvider: Send + Sync {
    fn kind(&self) -> AgentProviderKind;       // Builtin | Subprocess | CodexAppServer | Acp（v0.3 只实现前两者）
    async fn run(&self, agent: &AgentConfig, inputs: Value, ctx: &NodeContext)
        -> Result<AgentOutput, HarnessError>;
}
```

- 选择：`AgentConfig.harness.kind`（缺省 `builtin`）→ `ProviderRegistry`；未知 kind/配置非法 → `HARNESS_PLUGIN_CONFIG`。
- **Builtin**：现有 `HarnessExecutor::run_agent`（05 §5.5.4）保持；trait 化不改变其行为/输出。
- **Subprocess**：`SubprocessHarness` 启动外部进程、按协议交互；`AgentOutput` 四字段由 `final` 构造。
- Codex/DSH 不实现为 Rust crate 依赖：作为**外部命令**（或极薄 Python/Rust wrapper）实现 `zgraph-harness/1`，配置 `command` 指向（DR-011 只冻结协议；适配器骨架 V4 交付）。

## 2. 子进程协议 `zgraph-harness/1`（NDJSON over stdio）

### 2.1 握手（必须）

```json
// 父 → 子（第一行）
{"v":1,"type":"hello","protocol":"zgraph-harness/1","run_id":"...","node_id":"...","agent":{"agent_id":"reviewer","model":"...","limits":{"timeout_ms":180000,"max_content_bytes":1048576}}}
// 子 → 父
{"v":1,"type":"hello_ok","protocol":"zgraph-harness/1","provider":"codex","capabilities":{"streaming":true,"reasoning":false}}
```

- 协议版本不匹配/hello 超时（5s）→ `HARNESS_PLUGIN_PROTOCOL{reason:"handshake"}`。
- `capabilities.streaming=false` 时只允许 `final`（无 event）；`reasoning` 决定 `reasoning_delta` 是否映射。

### 2.2 run / event / final

```json
{"v":1,"type":"run","inputs":{...}}                                   // 父 → 子（hello 后）
{"v":1,"type":"event","kind":"token_delta","data":{"text":"..."}}    // 子 → 父（可选多行）
{"v":1,"type":"event","kind":"tool_call_start","data":{"call_id":"c1","tool_name":"read_file","args_summary":{...}}}
{"v":1,"type":"event","kind":"tool_call_end","data":{"call_id":"c1","ok":true,"result_summary":{...}}}
{"v":1,"type":"event","kind":"progress","data":{"percent":50,"message":"..."}}
{"v":1,"type":"final","output":{"content":"...","usage":{"input_tokens":1,"output_tokens":2}}}  // 子 → 父
{"v":1,"type":"error","code":"HARNESS_*","message":"..."}              // 子 → 父（可替代 final）
```

- `output` 必须满足 05 §5.3.2 `AgentOutput` 四字段（content/usage/...）；缺失/类型错 → `HARNESS_PLUGIN_PROTOCOL{reason:"final"}`。
- `event.kind` 仅允许上表子集；未知 kind **忽略并计数 warning**（向前兼容），不失败。
- 行大小 ≤ 1 MiB；超限 → 协议错误；JSON 非法 → 协议错误。

### 2.3 取消/退出

- 父进程取消：关闭 stdin + SIGTERM（3s）→ SIGKILL **进程组**；子进程可输出 `cancelled` 事件（可选），父进程以取消语义结束。
- 子进程非 0 退出/无 `final` → `HARNESS_PLUGIN_EXIT{exit_code}`；stderr 截断保留 8 KiB 供诊断（不写 trace 全文/密钥）。
- 超时：`timeout_ms`（配置）或节点 `timeout_ms` 先到者；超时 = 取消 + `NODE_TIMEOUT` 语义（实例 TimedOut → 冷峻 fail_run，沿用 03 §3.9）。

## 3. 配置（AgentConfig，additive）

```json
{
  "agent_id": "reviewer",
  "harness": {
    "kind": "subprocess",
    "command": "codex-adapter",
    "args": ["--profile", "review"],
    "protocol": "zgraph-harness/1",
    "env": {"CODEX_HOME": "${CODEX_HOME}"},
    "timeout_ms": 300000
  }
}
```

- `harness` 缺省 = `{kind:"builtin"}`；`kind` 闭集 `builtin|subprocess`；`subprocess` 必填 `command`/`protocol`（v0.3 只接受 `"zgraph-harness/1"`）。
- `args`/`env` 可选；`env` 值支持 `${ENV}` 展开，不落日志/trace；`timeout_ms` 可选（覆盖 provider 级，仍受节点超时上限）。
- 与 `skills`/`mcp` 正交：可同时配置；外部 harness 若自带 skills/MCP，由 adapter 内部处理（框架不做双重注入）。
- 未知字段拒绝（`deny_unknown_fields`）；配置非法 → `HARNESS_PLUGIN_CONFIG`。

## 4. 流式与 trace（不双重记录）

- 外部 harness 的 `event` 映射为**同一** `StreamingEvent` 并走既有 sink（05/06）：`token_delta`→`TokenDelta`、`reasoning_delta`→`ReasoningDelta`、`tool_call_start/end`→`ToolCallStart/ToolCallEnd`、`progress`→`Progress`。
- 框架**不**额外记录外部 harness 的内部 LLM 调用到 `llm_calls`（避免双重计数/泄漏）；`run` 级 `AgentOutput.usage` 若提供则计入既有 metrics（04/05 口径）。
- 外部 harness 不得直接写 trace DB/事件；只经协议事件由框架 sink 写。
- `events.kind` 不变；无 `harness` 配置时输出/事件/回放逐字节不变。

## 5. L2（不重跑）

- `--mode=events`：重放既有 trace 事件，不启动外部命令/网络。
- `--mode=outputs`：`recorded_outputs` 按 `(node_id,attempt[,item_index])` 返回记录输出；**不执行** `command`，不要求外部 harness 存在（可跨机器回放）。
- 记录缺失 → 既有 `REPLAY_RECORD_MISSING`；`record=hash` 行为不变。

## 6. 安全

- `command`/`args` 由 AgentConfig（受信配置）提供；不接受来自图/LLM 的动态拼接；cwd=`<assets_dir>`；env 白名单透传（父进程 env + 显式 `env`）。
- 进程组隔离/终止（03 §3.9.4）；超时/输出上限；stderr 截断；不写密钥/token 到日志/trace。
- 外部进程视为不可信：其输出经协议 schema 校验后才进入 AgentOutput；非法事件不提升为指令。

## 7. 错误码（append-only，闭集）

| 码 | 触发 |
|---|---|
| `HARNESS_PLUGIN_CONFIG` | `harness` 配置非法/未知 kind/缺 command |
| `HARNESS_PLUGIN_PROTOCOL` | 握手/版本/事件/final 非法；reason 闭集 `handshake`/`version`/`event`/`final`/`line_too_large` |
| `HARNESS_PLUGIN_EXIT` | 子进程非 0/无 final/崩溃 |
| `HARNESS_PLUGIN_TIMEOUT` | 子进程超时（或复用 `NODE_TIMEOUT` 语义，二者映射冻结：超时走 `NODE_TIMEOUT`，本码用于协议级） |

- 映射：配置 → 节点 `Crashed(Config)`；协议/退出 → `Crashed(HarnessPlugin)`（`HarnessErrorKind` 追加闭集值）；超时 → `TimedOut`/`NODE_TIMEOUT`（沿用 03）。
- 既有错误码不变；新码只追加 00 §0.4。

## 8. Codex / DSH 适配器边界

| 适配器 | v0.3 范围 | 边界 |
|---|---|---|
| Codex | **薄 adapter 骨架 + fake 协议测试**；真实 `codex exec --json` 映射延至 V5/ext | 不嵌入 Codex crate（未发布 crates.io/依赖重）；adapter 子进程把 Codex 事件映射为 `zgraph-harness/1` |
| DSH/ACP | **薄 adapter 骨架**；无 token delta 时 `capabilities.streaming=false` | 不引入 DSH 的 loop/状态机；只作为外部进程；ACP 语义事件经 adapter 映射 |
| 其他 | 任何实现 `zgraph-harness/1` 的进程可用 | 协议是唯一契约；框架不感知内部实现 |

## 9. 兼容与性能

| 场景 | 期望 |
|---|---|
| 无 `harness` 配置 | 走 builtin；AgentConfig/run/trace/replay 与 v0.2 逐字节不变 |
| `kind:"builtin"` 显式 | 等同于缺省（语义相同；配置不参与 hash） |
| 外部 harness L2 | 不启动进程；回放结果与记录一致 |
| 默认性能 | 无外部配置零额外进程/IO；builtin 路径无变化 |

## 10. AC-20（设计冻结；实测 V4）

| # | 用例 | 断言 |
|---|---|---|
| a | builtin 默认不变 | 无 harness/显式 builtin → AgentConfig/run/trace/replay 与 v0.2 golden 逐字节一致 |
| b | subprocess 握手/协议 | fake external harness（NDJSON）→ hello/hello_ok、run、final；`AgentOutput` 四字段正确 |
| c | 流式事件映射 | token/reasoning/tool_call/progress → 既有 StreamingEvent；顺序/seq 正确 |
| d | 取消/超时/进程组 | run 取消/超时 → 子进程组终止、实例 Cancelled/TimedOut、无进程泄漏 |
| e | 输出上限/协议错误 | 超大行/非法 JSON/无 final → 对应 `HARNESS_PLUGIN_*`；不写 trace 全文/密钥 |
| f | L2 不重跑 | `--mode=events/outputs` 不启动外部命令；结果与记录一致；跨无命令环境可用 |
| g | fake external 真链路 | e2e：真 CLI + fake external harness 二进制 → run/trace/replay 全链路 |
| h | Codex/DSH 骨架边界 | adapter 契约测试（fake 事件映射/无 streaming 降级）；不双重 trace/状态机 |

> **AC-20 默认零配置路径**：每条用例均在无 `harness`（或显式 builtin）对照下复跑（或由既有 v0.2 golden 覆盖）；builtin 路径输出/事件/回放逐字节一致。

## 11. Lead 裁决（2026-11-01，已确认）

| # | 事项 | 裁决 |
|---|---|---|
| 1 | 协议命名/版本 | `zgraph-harness/1`（`v:1` 双写）；版本不匹配 fail-closed |
| 2 | 外部 harness 的 `usage` | **计入**既有 metrics（无则不计） |
| 3 | `tool_call` 事件 | 允许映射既有 `StreamingEvent`；不写额外表 |
| 4 | 真实 Codex/DSH 适配器 | **不阻塞发布**：V4 必交付 seam + 协议 + fake external 真链路 e2e + Codex/DSH 薄 adapter 骨架（best-effort smoke；无环境则 release notes 标注「adapter 骨架，待真实环境验证」） |
| 5 | 取消宽限 | 关闭 stdin → SIGTERM **3s** → SIGKILL 进程组（复用 03 §3.9.4） |
| 6 | 输出/超时常量 | 见 00 §0.5：`HARNESS_PLUGIN_LINE_MAX_BYTES=1048576`、`HARNESS_PLUGIN_FINAL_MAX_BYTES=1048576`、`HARNESS_PLUGIN_STDERR_MAX_BYTES=8192`、`HARNESS_PLUGIN_DEFAULT_TIMEOUT_MS=300000`、`HARNESS_PLUGIN_HANDSHAKE_TIMEOUT_MS=5000` |

## 12. 硬约束（Lead 裁决 2026-11-01）

- 外部 harness **不得**写 trace DB/状态机；只能经协议输入/输出契约交互（事件由框架 sink 记录；`events.kind`/schema 不变）。
- 取消 = **进程组 SIGTERM（3s）→ SIGKILL**；不得只杀父进程。
- 输出/超时默认值使用 00 §0.5 常量（行/final/stderr/超时/握手），不得硬编码散落。
- L2：`--mode=outputs`/`--mode=events` **不启动外部进程/不重跑**；用 `recorded_outputs`/既有事件。
- 默认 builtin 路径逐字节不变；无 `harness` 配置零额外进程/IO。

> 本表取代原「待确认」状态；V4 实现必须覆盖 AC-20 a–h。
