# REV-126 · kernel · v0.5 F3a2 预算停止语义（BUDGET_EXCEEDED / stop_reason）

> **跨模块声明**：kernel 主体 + harness 响应后检查（Lead 已批准）；CLI 3 行机械编译适配（`16f6a7d`，Lead 批准）无业务行为；评审人只新增本评审记录。

- **分支**：`feat/kernel-budget-stop`
- 分支: feat/kernel-budget-stop
- **作者**：`kernel-dev`（commits `c1744aa`（kernel+harness）、`16f6a7d`（CLI 机械适配）；正文 `Review: REV-126` / `Agent: kernel-dev`）
- **评审人**：`reviewer-a`（独立评审，≠ 作者）
- **基于**：main `518ef34`（`main = origin/main = 518ef34`）
- **变更范围**：`518ef34..16f6a7d`（**9 files, +202/−14**）：`zgraph-kernel/{run,trace,types}.rs` + `tests/budget_stop.rs`；`zgraph-harness/{agent_loop,executor,observability}.rs`；CLI `bridge/run_cmd` 机械适配
- **评审时间**：2026-11-03
- **关联**：task-118/122；DR-018 §3、AC-27 c/d；F3a1/REV-123

## 1. 结论

**结论：通过**

P0=0、P1=0、P2=0；P3 4 项（kernel 实例终态名 Crashed vs 任务措辞 Failed、stop_reason 仅 kernel 面未进 trace/CLI、`node_error_json` 机械 arm、BUDGET_EXCEEDED 文档 D6）不阻塞。预算语义/映射/不可续/无 Running/门禁成立；机械 CLI 无行为。

### 1.1 已核对成立

| 项 | 结论 |
|---|---|
| harness 响应后检查 | ✅ `RunObservability::check_after_call()` 与 `check_before_call()` 共用 `check_budget`（`>=` 上限）；`agent_loop` 在 `record_llm_call` 后立即检查，最后一次 EndTurn 越界也返回 `RunFailure::BudgetExceeded`；无预算 `is_enabled=false` 直接 Ok，不检查/不停止；测试 `token_budget_after_response_stops_even_on_final_response` 覆盖 60+40=100、请求数=1、Failed、BUDGET_EXCEEDED、stop_reason |
| kernel 映射 | ✅ `RunFailure::BudgetExceeded` → `NodeError::BudgetExceeded`；node/instance `Crashed`（kernel 终态失败语义）→ run `Failed` + `error_code=BUDGET_EXCEEDED`（append-only domain code，不复用 RUN_*/NODE_*、不新增 HarnessErrorKind）；`RunOutcome.stop_reason=Some("budget_exceeded")`，`RunFinishedInfo.stop_reason` 同；非预算错误 `None` |
| 无 Running/不可续 | ✅ kernel `budget_stop.rs` 断言 run Failed、最后实例状态 Crashed、所有最后快照无 Running；无 resume 路径（v0.5 不实现） |
| trace v7 | ✅ 无 `zgraph-trace` schema/DDL/列改动，不升 v8；`RunFinishedInfo` 新增字段仅 kernel 内存 sink；无新表/列 |
| CLI 机械适配 | ✅ `bridge::node_error_json` 新增 `BudgetExceeded` arm（kind=BudgetExceeded），2 处测试字面量补 `stop_reason: None`；无既有路径行为变化/无断言改动 |
| 测试/门禁 | ✅ kernel **153/0**、harness **383/0/1**（默认/features mcp 同）、cli **257/0**；workspace **1278/0/3 ×2**；pytest **183**；clippy `-D warnings`/fmt/check/no-default 全绿；作者 WS 一次 ac17d flake 本次未复现 |
| 范围/跨模块 | ✅ kernel+harness 小改 Lead 批准；CLI 仅机械 3 行（实际 diff 6+2 行，均为新增字段/arm）；无 docs/其它模块改动；无 token/dist |

## 2. 四维评审

| 维度 | 结论 | 依据 |
|---|---|---|
| 功能正确性 | ✅ | 请求前+响应后双检查；最后响应越界 fail；映射/码/stop_reason/终态一致；无预算路径不变 |
| 代码结构合理性 | ✅ | budget 判定集中于 `RunObservability`；kernel 错误枚举 append-only；CLI 机械适配最小；trace 无 v8 |
| 逻辑性 | ✅ | `>=` 上限、响应后立即停止、请求数不增；stop_reason 仅预算错误；非预算 None；无 Running 终态 |
| 可读性 | ✅（P3） | 注释/字段说明清晰；P3 为实例状态命名/trace 面/文档 D6 |

## 3. 发现清单

### 3.1 P0/P1/P2

无。

### 3.2 P3（建议，不阻塞）

- **P3-1 实例终态措辞**：任务/测试注释说「node/instance Failed」，kernel `InstanceStatus` 实际终态为 **Crashed**（枚举语义「失败（终态）」）；测试已断言 Crashed/无 Running。建议 D6 文档统一「Crashed（失败终态）」，避免与 `RunStatus::Failed` 混淆。
- **P3-2 stop_reason 只到 kernel 面**：`RunOutcome.stop_reason`/kernel `RunFinishedInfo.stop_reason` 已加；`zgraph-trace::RunFinishedInfo` 与 CLI bridge `map_run_finished` 未携带（trace v7 不变属预期）。CLI/F3b 需决定是否在 trace 事件/`run` 输出暴露；当前不影响 kernel 语义/AC-27 c/d。
- **P3-3 CLI `node_error_json` BudgetExceeded arm** 只输出 `kind`；机械编译适配，F3b 可补 token/cost 字段（不改变既有错误路径）。
- **P3-4 文档**：`BUDGET_EXCEEDED`/stop_reason 的 00/05/06/07/08 as-built 回写留 D6（task-118 已列）。
- **P3-5**：author workspace 一次 `ac17d` 既有 flake 非本分支；本次 2 次 workspace 绿，无新增 baseline。

## 4. 验证证据

评审人亲自执行（worktree `.worktrees/kernel-kernel-dev`，HEAD `16f6a7d`，main `518ef34`，worktree 干净；`CARGO_HOME=<repo>/.cargo-home`）：

```bash
cargo test -p zgraph-kernel                    # 153 passed / 0 failed
cargo test -p zgraph-harness                   # 383 passed / 0 failed / 1 ignored
cargo test -p zgraph-harness --features mcp    # 383 / 0 / 1
cargo test -p zgraph-cli                       # 257 passed / 0 failed
cargo test --workspace                         # 1278 / 0 / 3（连跑 2 次一致）
python -m pytest                               # 183 passed
cargo clippy --workspace --all-targets --locked -- -D warnings  # clean
cargo fmt --all -- --check                     # exit 0
cargo check -p zgraph-cli --no-default-features --locked        # 通过
bash scripts/dev/check.sh                      # CHECK PASSED
```

- **代码抽查**：`check_after_call` 调用时机（record_llm_call 后）、kernel `stop_reason_for` 仅 BUDGET_EXCEEDED、`RunFinishedInfo`/`RunOutcome` 字段、CLI bridge 机械 arm、trace 无 schema diff。
- **范围**：9 files；无 `zgraph-trace` schema；无 docs/其它实现；无 token/dist。
- **自报对比**：kernel 153、harness 383、workspace 1278×2、cli 257、pytest 183 一致。

## 5. 条件关闭方式

无（结论通过；P3 见 §3/§7）。

## 6. 条件关闭记录（"通过"）

无开放条件。

## 7. 遗留问题

- P3-1/P3-2/P3-3 由 D6/F3b 决定（实例命名、stop_reason trace/CLI 面、BudgetExceeded 错误字段）；不阻塞 F3a2。
- F3b（cli-dev）将切换 `BUDGET_EXCEEDED` 断言并接 CLI 输出；F3a2 可 merge。
- 机械 CLI 适配已由 Lead 批准；无其它跨模块行为。

## 8. 第二评审 · 跨模块（harness/CLI 机械）（Lead 追加，2026-11-03）

- **评审人**：`lead`（**≠ 作者** `kernel-dev`；Lead 已批准跨模块改动，按 AGENTS §2 追加第二评审）
- **范围**（`518ef34..5ec1464`）：
  1. `crates/zgraph-kernel/**`（types/run/trace + tests）——BUDGET_EXCEEDED 域码、NodeError 变体、RunFinishedInfo/RunOutcome additive stop_reason；
  2. `crates/zgraph-harness/{agent_loop,executor,observability}.rs`——响应后预算检查；
  3. `crates/zgraph-cli/{bridge,run_cmd}.rs`——**仅 3 行机械编译适配**（BudgetExceeded match arm + 2 测试字面量 stop_reason: None，Lead 批准，无行为）。
- **核对结论（通过）**：错误码 append-only、不复用 RUN_*/NODE_*、不新增 HarnessErrorKind；trace v7 schema/DDL 不变；无预算默认路径不检查/输出不变；CLI 机械适配无逻辑/断言改动。
- **Lead 亲自验证**：`git diff 518ef34..5ec1464 --stat` 仅 kernel 3 源 + 新测试、harness 3 源、cli 2 文件（3 行机械）；`cargo test --workspace --offline` = 1278/0/3；kernel 153/0；harness 383/0/1；`--version`/clippy/fmt/check 均绿。
- **裁决/后续**：InstanceStatus 命名（Crashed vs Failed）由 D6 统一；stop_reason 仅在 kernel 内存面（trace v7 不变），F3b 从 RunOutcome 读取并展示；`node_error_json` additive 字段由 F3b/D6 决定。
- **结论**：**通过（第二评审）**。分支满足合并前置。
