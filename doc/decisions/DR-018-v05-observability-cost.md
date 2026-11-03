# DR-018 · v0.5 可观测性与成本预算

> **状态**：V1 设计冻结（REV-115；价格来源/预算停止语义/字段位置为 Lead 裁决项）。
> **日期**：2026-11-03 · **作者**：docs-dev · **关联**：05 §5.3/§5.4、06 §6.2/§6.11、07 AC-27、task-95
> **合同**：token/费用/耗时统计（run/node/agent）；预算上限默认关；trace/CLI additive、**不升 v8**；无配置默认路径不变。

## 0. 摘要

| 项 | 冻结 |
|---|---|
| 统计维度 | run 汇总 / node（instance+attempt）/ agent（agent_id+model）；token（prompt/completion/total）、耗时、调用次数 |
| 数据源 | 既有 `llm_calls.usage`（token）+ `events`/trace 时间戳；**不新增表/列**（不升 v8） |
| 费用 | 可选 **价格表文件**（`--pricing <file>`，JSON：model → input/output USD per 1M tokens）；缺文件/未知模型 → 费用不可用 + warning（不 fail） |
| 预算 | `--max-tokens <N>`（run 总 token）/`--max-cost <USD>`（run 总费用）；**默认关**；超限停止 |
| 停止语义 | 检查点：每次 LLM 调用完成后（下一轮/下一工具前）；超限 → run `Failed` + error_code **`BUDGET_EXCEEDED`**（append-only）+ stop_reason `Other("budget_exceeded")`；**v0.5 不可续**（v0.6 durable resume 另议） |
| 输出 | `RunFinished.metrics` JSON additive 键 + `trace --stats` additive + 人类摘要行；既有字段/输出不变 |
| AC-27 | a–f |

## 1. 统计字段

### 1.1 维度

- **run**：总 token（prompt/completion/total）、LLM 调用数、工具调用数、总耗时（wall/LLM/工具，若可得）、费用（可选）。
- **node**：按 `(instance_id, attempt)` 聚合：token、LLM 调用数、duration；来源 `llm_calls` 行（`instance_id`/`attempt` 已有）。
- **agent**：按 `agent_id`/`model` 聚合：token/调用数/费用（若价格表可用）。
- 既有 `RunMetrics`/`--stats` 字段保持；新键 additive。

### 1.2 数据源与口径

- token 以 `llm_calls.usage`（prompt/completion/total/reasoning）为准；缺失 usage → 该调用计入「未知 token」并在输出 warning（不臆造 0）。
- 耗时：已有 `started_at/finished_at/duration_ms`（LLM/tool）；run wall 由 trace 时间戳计算。
- 费用：`cost = prompt_tokens/1e6*input_price + completion_tokens/1e6*output_price`；缓存/推理 token 按现有字段能力处理（未知价格→不可用）。
- 不写 token/密钥；不记录 prompt/response 全文（既有口径）。

## 2. 价格表（配置 vs 内置）

- **推荐：配置文件**（`--pricing <file>` 或 env `ZGRAPH_PRICING`），JSON：

```json
{"currency":"USD","models":{"gpt-x":{"input_per_mtok":1.0,"output_per_mtok":2.0}}}
```

- 不内置价格（价格随供应商/时间变化，硬编码易过期）；缺文件 → 仅 token/耗时，费用字段缺失 + warning；未知模型 → 该调用费用未知 + warning（不 fail run）。
- 价格表变更不影响 run 语义/hash；仅输出计算。
- 裁决项：是否允许内置默认价（推荐**否**）。

## 3. 预算上限与停止语义

| 项 | 推荐 |
|---|---|
| CLI 命名 | `--max-tokens <N>`（run 总 tokens）/`--max-cost <USD>`（run 总费用）；二者可单用/并用 |
| 默认 | **关**（不配置不计算预算、不停止；输出不变） |
| 检查点 | 每次 LLM 调用完成后、下一次 LLM/工具调用前；run 结束时最终检查 |
| 超限动作 | **立即停止**：不再发起 LLM/工具；run `Failed`，`error_code = BUDGET_EXCEEDED`（新域码，append-only）；`RunFinished.stop_reason = Other("budget_exceeded")`；已产生的 trace/metrics 保留 |
| 是否可续 | **v0.5 不可续**；用户需新 run（v0.6 durable resume 再评估） |
| 精度 | tokens 整数；cost 小数（USD，保留 6 位）；超限判定用 `>=` 上限 |
| 负值/非法 | 配置错误（Crashed(Config)） |

> `BUDGET_EXCEEDED` 为新 domain error code（append-only；不新增 `HarnessErrorKind`）；不写 `runs.error_code` 之外的语义变更。裁决项：是否复用 `RUN_*`/`NODE_*` 现有码 vs 新码（推荐新码，语义明确）。

## 4. trace/CLI 输出（additive，不升 v8）

- **RunFinished.metrics**（既有 JSON）：追加键（如 `token_usage:{prompt,completion,total}`、`cost_usd`、`duration_ms`、`llm_calls`、`tool_calls`）；不删除/不改既有键。
- **`trace --stats`**：追加 token/费用/耗时汇总（仅当有数据；无配置/无调用输出不变）。
- **人类可读**：run 结束摘要可加一行 tokens/cost/duration（可 `-q` 抑制）；`--json` additive。
- **不新增表/列**：统计从 `llm_calls`+events 聚合；`tool_calls` 已有。**trace v7 不变，不升 v8**。
- 裁决项：是否新建 `run_metrics` 表（推荐**否**，v0.5 聚合即可；若查询性能不足 v0.6 评估）。

## 5. 兼容/安全

- 无 `--pricing`/预算参数：统计仍可输出（token/耗时）但**不改变既有输出**（除非显式 `--stats`/新 verbose）；默认 run/trace/replay 逐字节不变。
- 预算停止是显式 opt-in；无预算不检查/不停止。
- 价格表/费用不参与 hash/DSL；不写 token/密钥/价格表内容到 trace。
- 与 reexec/沙箱/外部 harness 正交。

## 6. AC-27（设计冻结；F3 实测）

| # | 断言 |
|---|---|
| a | run/node/agent 维度 token/耗时/调用数聚合正确（fake LLM 已知 usage） |
| b | `--pricing` 有效 → cost 计算正确；缺文件/未知模型 → 费用不可用 + warning，不 fail |
| c | `--max-tokens` 超限 → 停止、Failed、`BUDGET_EXCEEDED`、`Other("budget_exceeded")`；不再发起调用 |
| d | `--max-cost` 同 c（价格表可用时）；不可用费用 → 配置/警告口径 |
| e | 默认无预算/无 pricing：run/trace/replay 输出与 v0.4 逐字节一致（无 v8） |
| f | trace/CLI additive（metrics/--stats/人类行）；既有字段不变；不新增表/列 |

## 7. Lead 裁决项

| # | 事项 | 推荐 |
|---|---|---|
| 1 | 价格来源 | **配置文件**（`--pricing`/env），不内置默认价 |
| 2 | 预算命名/单位 | `--max-tokens`（tokens）/`--max-cost`（USD）；默认关 |
| 3 | 停止语义 | 每次 LLM 调用后检查；立即停止；Failed + `BUDGET_EXCEEDED` + `Other("budget_exceeded")`；v0.5 不可续 |
| 4 | 字段位置 | 复用 `RunFinished.metrics`/`llm_calls`/`--stats`；**不新建表**、不升 v8 |
| 5 | 未知 usage/价格 | warning + 不臆造；不 fail run |
