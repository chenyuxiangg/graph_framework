# REV-123 · harness · v0.5 F3a1 计费/用量/预算核心

- **分支**：`feat/harness-observability`
- **作者**：`dsl-dev`（单提交 `5feed3f`，基于 main `d38dae4`）
- **评审人**：`reviewer-b`（独立评审，≠ 作者）
- **变更范围**：`d38dae4..5feed3f`（7 files，+1191/−0）：`observability.rs` 新增；`executor.rs`/`agent_loop.rs`/`provider.rs`/`plugin.rs`/`constants.rs`/`lib.rs`；仅 harness，无 Cargo
- **评审时间**：2026-11-03
- **关联**：task-112/117、DR-018 §1–§3、AC-27、05 §5.16.2、06 §6.16.2、REV-110/112

## 1. 结论

**结论：通过**（首轮有条件通过 → C1/P2 由 `e9ac29f`、C2 残项由 `4a93bc3` 关闭；见 §7）

P0=0；**P1=0**；P2 残余（不阻断，见 §7）；P3 后续。PricingTable 解析/费用公式、预算累计/请求前检查/不可续、run/node/agent 聚合、Unavailable/Estimated 口径、默认无预算无输出回归均通过；最终 harness **378/0/1**（默认/features）、workspace **1269/0/3**、cli **257/0**、pytest **183**、clippy/fmt/check/no-default 全绿。

> **首轮历史（保留）**：`5feed3f` 首轮 P1=2（C1 非 USD、C2 max_cost 缺模型）；`e9ac29f` 已关闭 C1 + P2（预算 >0），但 C2 的多 Agent 模型加载期校验仍缺（见 §7）。

首轮两项条件：

- **C1 非 USD pricing 仍产出 `cost_usd`**：`PricingTable` 只要求 `currency` 非空（`is_usd()` 供 `max_cost` 检查），但 `RunObservability::record_llm_call` 对任何 currency 都调用 `cost_usd()` 并写入 `UsageTotals.cost_usd`；无 `max_cost` 时不报错、无 warning，统计字段名义为 USD 却按其它币种数值。DR-018 §2/任务-117「currency 非 USD 限制」未落实。
- **C2 `max_cost` 下 pricing 缺配置模型未启动/加载期 fail-closed**：`observability_for_job` 只校验「有 pricing + USD」，未校验 Agent 模型是否有价；实际先发出 LLM 请求，下一次迭代 `check_before_call` 才返回 `CostUnavailable`。DR-018 §2 要求「pricing 缺图中配置模型 / 外部 DSH 成本不可得 → 启动/加载期配置错误（fail-closed）」，当前会先产生一次 LLM 调用。

## 2. 四维评审

| 维度 | 结论 | 依据 |
|---|---|---|
| 功能正确性 | ⚠️（C1/C2） | 费用/累计/边界/请求数/聚合主路径通过；非 USD 与 max_cost 缺模型口径未落实 |
| 结构合理性 | ✅ | pricing/budget/observability 独立模块；executor 按 run 持有；无 kernel/cli/Cargo 改动 |
| 逻辑性 | ✅（P2） | >= 边界、跨 agent_call、不可续、Unavailable 不计、Estimated 计入均符合预裁决 |
| 可读性 | ✅（P3） | 注释/错误 reason/测试名清晰；P3 测试/文档补齐 |

## 3. 发现清单

### P1 · 合并前必须关闭

| # | 位置 | 问题 | 条件 |
|---|---|---|---|
| C1 | `observability.rs::record_llm_call`（`pricing.cost_usd`）/`PricingTable` | 非 USD pricing 仍计算并写入 `UsageTotals.cost_usd`（字段名义 USD）；仅 `max_cost` 时有 `pricing_currency_mismatch`，无预算时静默错误币种。任务/DR 要求 currency 非 USD 限制 | 非 USD 价格表在 `PricingTable` 解析期拒绝（`pricing_invalid`）或在 `record_llm_call` 不计算 cost + warning；补非 USD 负例（无 max_cost 与有 max_cost 两路），断言 cost 不产出 |
| C2 | `executor.rs::observability_for_job` + `agent_loop` 预算检查 | `max_cost` 只校验 pricing 存在/USD，未校验 Agent 模型是否有价格；先发起 LLM 请求，下一次 `check_before_call` 才 `CostUnavailable` 停止。DR-018 §2 要求启动/加载期 fail-closed（不发起任何 LLM 请求） | 在拿到 LoadedAgent 后、请求前校验 `pricing.has_model(model)`（max_cost 时）；缺失 → `HARNESS_CONFIG_LOAD{pricing_invalid/pricing_model_missing}`，断言 `fake.requests()` 为空；补配置错误测试 |

### P2 · 建议

| # | 位置 | 问题 | 建议 |
|---|---|---|---|
| N1 | `BudgetConfig.validate` | `max_tokens=0` 被显式视为合法（立即超限；测试也这么断言）；任务-117「非法配置（0/NaN）」未明确 0 的语义。`max_cost=0` 同样合法。 | 与 DR-018/Lead 确认 0 是否合法；若非法，validate 拒绝并测；若合法，文档/测试明确「立即停止」 |
| N2 | `agent_loop` 预算检查位置 | 每次 iteration 开始检查，但 `invoke_with_retry` 的 establish 重试不逐请求检查；失败重试无 usage，当前无实际超限漂移，但字面「每次 LLM 请求前」未覆盖重试 | 在 invoke 重试循环内也调用 `check_before_call`（或文档说明失败重试不消耗预算） |
| N3 | 外部 subprocess harness | `plugin.rs` 传 `observability: None`，stats 只留全 0、无 unavailable warning；Lead 预裁决 F3b 标注，当前记录一致 | F3b 显式标注 unavailable/不伪造 |
| N4 | `RunUsageStats` 维度 | `by_node` 键为 `(instance_id, attempt)`，无 node_id；`by_agent` 为 `(agent_id, model)`；DR-018 §1.1 口径需 F3b 消费方确认 | F3b trace/stats 映射时校验；必要时 additive 加 node_id |
| N5 | 预算检查/失败 | BudgetStop 已写入 stats，但 run 终态/error_code/stop_reason 映射属 F3a2；本波 `RunFailure::BudgetExceeded` → `NodeError::Internal` 为占位（DR 接受） | F3a2 映射 `BUDGET_EXCEEDED`/`Other("budget_exceeded")`；默认无预算路径确认无影响 |

### P3 · 测试/文档

| # | 位置 | 问题 | 建议 |
|---|---|---|---|
| N6 | `pricing_invalid` | 缺文件/坏 JSON/负价/缺字段测试齐；非 USD 未测（C1 一并） | 补 |
| N7 | `run_usage_stats` 默认 | 无预算默认输出/行为测试通过；`run_observability` map 创建但无 trace 输出 | F3b 清理 map（预裁决） |
| N8 | 统计 unknown | `UsageSource::Unavailable` 不计 token、不伪造；`Estimated` 计入并标注由预裁决；测试齐 | 输出层 F3b 标注 estimated |
| N9 | 稳定性 | 首次 workspace 命中既有 codex adapter 并行 flake（2 个 plugin 测试），隔离 2/2 通过、workspace 重跑 1267/0；本分支只 harness observability，无 plugin 改动 | D6 登记既有 flake |

## 4. 验证证据

worktree `/Users/zyzs/lczyzs/code/zgraph/.worktrees/harness-dsl-dev`，HEAD `5feed3f`，base `d38dae4`，工作区干净；`CARGO_HOME=/Users/zyzs/lczyzs/code/zgraph/.cargo-home`，未设 `CARGO_TARGET_DIR`：

```bash
cargo test -p zgraph-harness                 # 377 tests：376 passed / 0 failed / 1 ignored
cargo test -p zgraph-harness --features mcp  # 377 tests：376 passed / 0 failed / 1 ignored
cargo test --workspace                       # 重跑：91 targets / 1267 passed / 0 failed / 3 ignored
cargo test -p zgraph-cli                     # 257 passed / 0 failed
pytest -q                                    # exit 0；183 passed
cargo clippy --workspace --all-targets --locked -- -D warnings  # clean
cargo fmt --all -- --check                   # clean
cargo check -p zgraph-harness --no-default-features  # clean
bash scripts/dev/check.sh                    # PASSED
```

- **与自报基本一致**：harness 376/0/1（默认/features）、workspace 重跑 1267/0/3、cli 257/0、pytest 183；clippy/fmt/check/no-default 全绿；仅 7 files（harness），无 Cargo/其它模块。
- **已核**：PricingTable 解析/缺文件/坏 JSON/负价/缺字段；cost 公式/6 位；max_tokens 100 边界、max_cost 0.5 边界；超限后 `fake.requests()` 只 1 次；跨 agent_call 累计；run/node/agent 聚合；Unknown usage/Unknown model warning；无预算默认不停止、无输出变化；max_cost 无 pricing=配置错误且无请求；BudgetExceeded → Internal 占位。
- **未覆盖/风险**：C1 非 USD、C2 max_cost 缺模型；P2 N1–N5。

## 5. Lead 预裁决确认

- Estimated usage 计入统计/预算并标注估算；Unavailable→unknown 不计：**实现一致**（测试）。
- 无 pricing 不产生 cost/unpriced 噪音：**一致**（cost None/无 warning）。
- 外部 subprocess 统计 unavailable（F3b 标注）：**一致（当前 None/全 0，待 F3b 输出）**。
- per-run map 清理 F3b、run 结束汇总检查 F3a2：**接受**。
- 新 reason D6 回写：**接受**（`pricing_*`/`budget_invalid`/`BudgetExceeded`）。
- 无预算路径逐字节不变：**通过**（默认 run 输出/行为无新字段；stats 仅显式 API）。

## 6. 遗留问题

- **C1/C2**：非 USD pricing 限制 + max_cost 缺模型启动前 fail-closed；补测试后复审转「通过」。
- **P2 N1–N5**：0 语义、重试检查、subprocess unavailable、维度口径、F3a2/F3b 映射。
- **P3 N6–N9**：非 USD 测试、map 清理、F3b 输出、既有 flake。
- 无 P0；C1/C2 未关闭前不可 merge。

## 7. 条件关闭复核（2026-11-03，`e9ac29f` + `4a93bc3`）

**复核结论：通过。** C1/P2 由 `e9ac29f` 关闭；C2 多模型残项由 `4a93bc3`（仅 executor +253）关闭；worktree 干净。

### C1（非 USD）
- `PricingTable` 解析期仅接受 `currency=USD`（非 USD → `pricing_invalid`），删除 `is_usd`/`pricing_currency_mismatch` 分支；EUR 负例通过；无 `max_cost` 不再产出非 USD `cost_usd`。**关闭**。

### P2（预算 >0）
- `max_tokens/max_cost` 必须 >0；0/负/NaN→`budget_invalid`；`invalid_budget_values_are_config_errors_before_requests` 覆盖零请求。**关闭**。

### C2（多 Agent 模型）
- 新增 additive `HarnessExecutor::with_required_models`（及 alias），`max_cost` 下必须提供。
- `NodeExecutor::execute` 入口、任何 Agent 加载/LLM 请求前调用 `validate_run_pricing()`，顺序：`budget_invalid` → `pricing_required_for_max_cost` → `pricing_models_required`（未提供 required） → `pricing_missing_model`（任一缺价）；全部 `HARNESS_CONFIG_LOAD`、零请求。
- 测试：`multi_model_requires_all_models_priced_before_any_request`（`TWO_MODEL_GRAPH` first deepseek-chat priced / second-model missing → `pricing_missing_model` + `fake.requests().is_empty()`）；`max_cost_without_required_models_is_config_error_before_requests`（`pricing_models_required` 零请求）；`max_cost_all_models_priced_runs_normally`（全有价完成、2 请求）；`required_models_not_enforced_without_max_cost`（无 max_cost 不校验、默认完成）。**C2 关闭**。

### 最终门禁（`4a93bc3`）
```bash
cargo test -p zgraph-harness                 # 383 tests：382 passed / 0 failed / 1 ignored
cargo test -p zgraph-harness --features mcp  # 383 tests：382 passed / 0 failed / 1 ignored
cargo test --workspace                       # 91 targets：1273 passed / 0 failed / 3 ignored
cargo test -p zgraph-cli                     # 257 passed / 0 failed
pytest -q                                    # exit 0；183 passed
cargo clippy/fmt/check/no-default            # clean
```
- 范围：`4a93bc3` 仅 `executor.rs` +253；无 Cargo；`git status` 空。
- **F3b 义务**：CLI 必须从 graph 的全部 AgentConfig 收集模型并调用 `with_required_models`；空集合不得作为“已提供”绕过（F3b 测试补）。其余 P2（retry 逐请求、subprocess unavailable、by_node node_id、F3a2 stop 映射）留后续波次。
