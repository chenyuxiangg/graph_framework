# `zgraph-harness/1` 子进程协议（v0.3/V4）

> 状态：实现已交付（`feat/harness-plugin`，REV-099）；设计冻结见
> [`doc/decisions/DR-011-v03-harness-plugin-design.md`](../../../doc/decisions/DR-011-v03-harness-plugin-design.md)。
> 本文件面向**外部 adapter 作者**，只描述 wire 协议与契约边界。

## 1. 进程模型

- **每 run 每 provider 一个常驻 adapter 进程**（懒启动；`cwd = <assets_dir>`）。
- 一次 `agent_call`（含重试 attempt）发一帧 `run`；`agent_id` 变化或 `attempt > 0`
  时 `session.reset=true`（adapter 应新建后端 session）。
- 进程模型由 `AgentConfig.harness` 配置：

```json
{
  "kind": "subprocess",
  "command": "python3",
  "args": ["-m", "zgraph_worker.adapters.codex"],
  "env": {"CODEX_BIN": "/usr/local/bin/codex"},
  "timeout_ms": 300000,
  "protocol": "zgraph-harness/1"
}
```

- 二进制由**用户自备**（zgraph 不下载/不内嵌）；配置非法 → `HARNESS_CONFIG_LOAD{reason}`。
- 外部 harness 拥有自己的 agent loop：`AgentConfig.tools/skills/mcp` 与节点级
  `max_iterations` 必须为空，否则 `HARNESS_CONFIG_LOAD{reason:"provider_capability_conflict"}`。

## 2. 帧格式（NDJSON over stdio，`v:1` 与 `protocol` 双写）

父 → 子：

```jsonc
{"v":1,"type":"hello","protocol":"zgraph-harness/1","run_id":"...","node_id":"...",
 "provider":"external","limits":{...}}
{"v":1,"type":"run","agent":{"id":"reviewer","model":"...","temperature":0.2,"max_tokens":2048},
 "session":{"reuse_key":"<run_id>:<agent_id>","reset":false},
 "inputs":{...},                       // 已按 AgentConfig/节点 input_mapping 映射
 "trace":{"run_id":"...","instance_id":"...","node_id":"...","attempt":0,"item_index":0}}
{"v":1,"type":"cancel"}
```

子 → 父：

```jsonc
{"v":1,"type":"hello_ok","protocol":"zgraph-harness/1","provider":"codex_exec",
 "capabilities":{"streaming":false,"reasoning":false}}
{"v":1,"type":"event","kind":"delta","text":"..."}          // 仅 capabilities.streaming=true
{"v":1,"type":"event","kind":"reasoning","text":"..."}
{"v":1,"type":"event","kind":"tool_start","call_id":"c1","tool":"...","args_summary":{}}
{"v":1,"type":"event","kind":"tool_end","call_id":"c1","ok":true,"result_summary":{}}
{"v":1,"type":"event","kind":"progress","percent":40,"message":"..."}
{"v":1,"type":"result","content":"...","iterations":1,"stop_reason":"EndTurn",
 "usage":{"input_tokens":1,"output_tokens":2,"reasoning_output_tokens":0}}
{"v":1,"type":"error","code":"HARNESS_PLUGIN_ERROR","reason":"protocol","message":"..."}
```

- 只允许映射既有 5 种 `StreamingEvent`（`delta`/`reasoning`/`tool_start`/`tool_end`/
  `progress`）；未知 `event.kind` 由框架**忽略 + warn**（向前兼容）。`streaming=false`
  时 `delta`/`reasoning` 被丢弃。
- `result.content` 上限 1 MiB；`iterations` 必须为整数（Codex/DSH 降级固定 **1**）；
  `stop_reason` 用既有字面量（`EndTurn`/`MaxTokens`/`Other:<raw>` 等，**不新增枚举**）。
- `usage`：`input_tokens`→prompt、`output_tokens`→completion、`reasoning_output_tokens`
  →reasoning、`total_tokens` 可省（两者相加）；`cached_input_tokens` 等缓存字段丢弃。

## 3. 限制与失败（`HARNESS_PLUGIN_ERROR{reason}`）

| 限制 | 值 | 失败 reason |
|---|---|---|
| 握手 `hello_ok` | 5 s | `handshake` |
| 单行 NDJSON | 1 MiB | `line_too_large` |
| `result.content` | 1 MiB | `result_invalid` |
| 总输出 | 8 MiB | `output_limit` |
| 事件数 | 20000 / run | `event_limit` |
| stderr 捕获 | 8 KiB（截断，不写 trace 全文） | — |
| 非法 JSON 行 | 容忍 2 行（第 3 行终止） | `protocol` |
| 非 0 退出 / 无 `result` | — | `exit_nonzero` |
| spawn 失败 | — | `spawn_failed` |
| 协议版本不符（`hello_ok`/帧 `v`） | — | `version` |

- 未知 `type`/非法帧 → `protocol` 并杀进程组；`error` 帧终止本次调用。
- 取消：父进程发 `cancel` 帧后关 stdin → `SIGTERM` 进程组 → 宽限 2 s → `SIGKILL`
  （Unix；非 Unix 仅 `kill` 子进程）。节点硬 deadline 到期 → zgraph 侧 `NODE_TIMEOUT`
  （`HARNESS_PLUGIN_ERROR{reason:"timeout"}` 仅用于协议级超时）。
- 外部进程**禁止**写 trace/状态机/`llm_calls`；可观测性只经上述事件帧（框架记录）。

## 4. 内置适配器

| adapter | 入口 | 状态 |
|---|---|---|
| Codex | `python/zgraph_worker/adapters/codex.py`（包装 `codex exec --json` + `-o`） | **先做**；`streaming=false`、`iterations=1`、cached tokens 丢弃；无 `codex` 二进制 → 结构化 `spawn_failed` |
| DSH ACP | `python/zgraph_worker/adapters/dsh_acp.py`（`dsh --profile acp`） | **best-effort 骨架**：握手 + 结构化 `protocol` 错误（真实 ACP transport 待真实环境验证） |
