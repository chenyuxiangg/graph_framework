# REV-127 · CLI · v0.5 F3b 可观测/预算接线

- **分支**：`feat/cli-observability`
- **作者**：`cli-dev`（单提交 `c0a8ea4`，基于 main `518ef34` 含 F3a1）
- **评审人**：`reviewer-b`（独立评审，≠ 作者）
- **变更范围**：`518ef34..c0a8ea4`（3 files，+1318/−12）：`run_cmd.rs`、`trace_cmd.rs`、`tests/observability.rs`；仅 cli，无 Cargo
- **评审时间**：2026-11-03
- **关联**：task-119/121、DR-018 §2/§4、AC-27、F3a1 API、REV-123

## 1. 结论

**结论：通过**（首轮有条件通过 → C1 由 `436a2de` 关闭；P2 残余及 F3a2 rebase 义务见 §7，均不阻断）

P0=0；**P1=0**；P2 残余；P3 后续。flag/预检、多模型收集、外部 subprocess fail-closed、usage/trace stats additive、默认输出不变、F3a1 接口透传均通过；最终 cli **268/0**、workspace **1285/0/3**、pytest **183**、clippy/fmt/check/no-default 全绿。

> **首轮历史（保留）**：`c0a8ea4` 首轮为有条件通过——C1 `--max-tokens -1` 被 parser 当 exit2；`436a2de` 已关闭（见 §7）。

首轮条件：

- **C1 `--max-tokens` 负值走用法错误 exit2，而非 `budget_invalid` exit3**：`parse_u64_option` 把 `-1` 当语法错误；任务-121/DR-018 要求 budget ≤0/NaN/Inf → `budget_invalid` 配置错误 exit3（`--max-cost -1` 已正确 exit3，`--max-tokens 0` 也 exit3）。现有 `parse_rejects_invalid_budget_syntax` 把 `--max-tokens -1` 固化为 parser 错误，与要求冲突。

## 2. 四维评审

| 维度 | 结论 | 依据 |
|---|---|---|
| 功能正确性 | ✅ | 预检/多模型/usage/stats 成立；C1 负 token budget 已由 `436a2de` 关闭 |
| 结构合理性 | ✅ | flags→Observability→preflight→executor 透传清晰；仅 cli；trace additive 分离 |
| 逻辑性 | ✅（P2） | graph 全模型去重、外部 fail-closed、默认不变、additive 输出均正确 |
| 可读性 | ✅（P3） | usage/reason/注释清楚；P3 测试/文档 |

## 3. 发现清单

### P1 · 合并前必须关闭

| # | 位置 | 问题 | 条件 |
|---|---|---|---|
| C1 | `run_cmd.rs::parse_u64_option`/`--max-tokens` | `-1` → `Err` 用法错误 exit2；任务要求 `<=0` → `budget_invalid` exit3。`parse_rejects_invalid_budget_syntax` 还断言该行为 | 用有符号解析（i64/字符串→i128）后由 `BudgetConfig.validate` 统一拒绝 ≤0，或对负值显式映射 `budget_invalid` exit3；把 `parse_rejects_invalid_budget_syntax` 中 `--max-tokens -1` 移到 exit3/不变 DB 用例；补 `-1`/`-N` 测试。`0` 已 exit3 保持 |

### P2 · 建议

| # | 位置 | 问题 | 建议 |
|---|---|---|---|
| N1 | `run_in_runtime` 顺序 | flag 级 `resolve_observability` 在 DB 前；但 graph 级 `prepare_graph_observability`（多模型缺价/外部 harness/max-cost）在 `open_writer_with_recovery` **之后**，失败时 DB 已创建（测试注释“DB 可能已建”）。任务-119 仅 flags 前预检，任务-121 `pricing_missing_model` 要求零请求零 run（未明确 DB） | 明确口径；若要求图级也“不建 DB”，把 graph 加载/价格覆盖检查移到 open_writer 前（需 graph 可先加载）；否则 D6 文档化 |
| N2 | 外部 subprocess `--json.usage` | `usage=None` 时返回 `{available:false,reason}`，不含 `run/by_node/by_agent/warnings/budget_stop`；任务措辞要求 `--json.usage` 三维结构 + unavailable 标注 | 保持 `available:false` 是合理简化，但需 D6 明确；或输出空 `run` + `unavailable` |
| N3 | `--max-cost` NaN/Inf CLI | F3a1 validate 有单测；CLI 级只测 0/负值，NaN/Inf 无 e2e | 补 CLI 用例 exit3 不建 DB |
| N4 | `trace --stats` cost | `finished_cost_usd` 依赖 F3a2 `RunFinished.metrics`；当前无数据不输出（符合“无数据不输出”），但 F3a2 合并后需验证字段路径 `cost_usd/metrics.cost_usd/record.metrics.cost_usd` | F3a2 rebase 后补 cost/预算 stop 断言 |
| N5 | 预算超限占位 | `budget_stop` usage 已有，但 run error/exit 仍 `NodeError::Internal`（F3a2 未合并）；测试标注占位 | F3a2 合并后 rebase 到 `BUDGET_EXCEEDED`/`Other("budget_exceeded")`；作为合并条件/后续 |
| N6 | `$ZGRAPH_PRICING` | 代码支持 env 兜底，CLI e2e 未测 | 补 env 优先级/缺省测试 |

### P3 · 测试/文档

| # | 位置 | 问题 | 建议 |
|---|---|---|---|
| N7 | `usage_json` | by_node 无 node_id、by_agent 无 distinct model 之外字段；F3b 消费口径 | D6 文档 |
| N8 | `trace --stats` | token/llm_calls 依赖 `--record=full`；默认 `hash` 无数据（测试覆盖） | D6 明确 |
| N9 | 新 reason | `cost_unavailable_external_harness`/`budget_invalid`/`pricing_models_required` 等 D6 回写 | D6 |
| N10 | 稳定性 | 本评审 cli/workspace 全绿；F3a2 并行合并后需 rebase | 合并条件 |

## 4. 验证证据

worktree `/Users/zyzs/lczyzs/code/zgraph/.worktrees/cli-cli-dev`，HEAD `c0a8ea4`，base `518ef34`，工作区干净；`CARGO_HOME=/Users/zyzs/lczyzs/code/zgraph/.cargo-home`，未设 `CARGO_TARGET_DIR`：

```bash
cargo test -p zgraph-cli            # 267 passed / 0 failed / 0 ignored
cargo test --workspace              # 92 targets：1284 passed / 0 failed / 3 ignored
pytest -q                           # exit 0；183 passed
cargo clippy --workspace --all-targets --locked -- -D warnings  # clean
cargo fmt --all -- --check          # clean
cargo check -p zgraph-harness --no-default-features  # clean
bash scripts/dev/check.sh           # PASSED
```

- **与自报一致**：cli 267、workspace 1284；仅 cli 3 files，无 Cargo；`git status` 空。
- **已核**：pricing 缺/坏/非 USD、max-cost 无 pricing、budget 0/负 max-cost → exit3 不建 DB；语法错 exit2；多模型 graph 收集（双模型缺价 `pricing_missing_model`、零 run；全价正常 by_agent 两模型）；外部 subprocess+max_cost `cost_unavailable_external_harness` 零 run；有效 pricing usage 精确 280/20/300 + cost 0.00032；unknown model cost null+warning；max-tokens 停止且 budget_stop；trace stats additive 280/20/300/llm_calls/llm_models/duration；默认无 flags 无 usage/llm 键、trace stats 无 llm 键。
- **未覆盖/风险**：C1 负 token budget；P2 N1–N6。

## 5. F3a2 合并义务（条件，不阻塞本评审）

- F3a2 先合并时，F3b 必须 rebase 并把预算超限断言从 `NodeError::Internal`/占位切换到 `BUDGET_EXCEEDED` + `stop_reason Other("budget_exceeded")`（run Failed/exit 语义）。
- `trace --stats` cost 从 F3a2 `RunFinished.metrics` 读取；补 cost + `budget_stop` 断言。
- dsl-dev/Lead 裁决的 F3a2 映射（kernel）与本 CLI 的 usage 输出必须一致；CLI 不写 metrics。

## 6. 遗留问题

- **C1**：负 `--max-tokens` → budget_invalid exit3；修后复审转「通过」。
- **P2 N1–N6**：graph 级 DB 口径、外部 usage 形状、NaN/Inf CLI、F3a2 cost、占位预算 stop、env pricing。
- **P3 N7–N10**：D6 文档/新 reason/rebase。
- 无 P0；C1 未关闭前不可 merge。

## 7. 条件关闭复核（2026-11-03，`436a2de`，parent `f19ab3c`）

**复核结论：通过。** `436a2de` 仅 `run_cmd.rs`（+35/−14）与 `tests/observability.rs`（+90）；无 Cargo；工作区干净。

### C1（负 max-tokens）
- `RunArgs.max_tokens` 改为 `Option<i64>`，`parse_i64_option`：负值进入配置预检，不再 parser exit2；真正语法错仍 exit2。
- `resolve_observability` 对 `Some(value<=0)` 返回 `HARNESS_CONFIG_LOAD{reason:"budget_invalid"}` exit3（含 -1/-987654）；在 DB 打开前。
- 测试：`max-tokens-negative`、`max-tokens-large-negative` exit3 不建 DB；`parse_rejects_invalid_budget_syntax` 只保留 `abc` 语法错，负值 parse 成功；`--max-cost 0/-1/NaN/inf/-inf` exit3 不建 DB；`$ZGRAPH_PRICING` env 兜底/flag 优先/缺失 exit3 测试通过。
- **C1 关闭**。

### P2 残余（不阻断）
- **graph 级预检 DB 口径**：多模型/外部 harness/`pricing_missing_model` 在 `open_writer` 之后（flags 级 pricing/budget 才保证不建 DB）；任务仅明确 flags 级不建 DB，graph 级测试注释“DB 可能已建”。D6 明确。
- **外部 `--json.usage` 形状**：`usage=None` 时 `{available:false,reason}`，不含 run/by_node/by_agent/warnings/budget_stop；代码注释/测试留 D6。
- `trace --stats` cost 依赖 F3a2 `RunFinished.metrics`；F3a2 合并后补 cost/budget_stop 断言。
- 预算超限当前仍 `NodeError::Internal` 占位；F3a2 合并后 rebase 到 `BUDGET_EXCEEDED` + `Other("budget_exceeded")`。
- `$ZGRAPH_PRICING` 已补；`record=full` 才有 trace llm stats（文档口径）。
- by_node node_id、新 reason 回写等 D6/P3。

### 最终门禁（`436a2de`）
```bash
cargo test -p zgraph-cli            # 268 passed / 0 failed
cargo test --workspace              # 92 targets：1285 passed / 0 failed / 3 ignored
pytest -q                           # exit 0；183 passed
cargo clippy/fmt/check/no-default   # clean
bash scripts/dev/check.sh           # PASSED
```
- 仅 cli 2 files；无 Cargo；`git status` 空；默认无 flags 输出不变。

### F3a2 合并义务（后续条件，不阻塞本次通过）
- F3a2 先合并时，F3b 必须 rebase：预算超限断言由占位切到 `BUDGET_EXCEEDED` + `stop_reason Other("budget_exceeded")`；`trace --stats` cost/`budget_stop` 断言补齐；CLI 不写 kernel metrics。由 cli-dev 在合并前/后执行。
