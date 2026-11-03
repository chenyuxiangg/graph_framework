# REV-129 · harness · v0.5 F5e 外部 harness usage 不可得标注

- **分支**：`feat/harness-usage-unavailable`
- **作者**：`dsl-dev`（单提交 `4533d44`，基于 main `568c0f3` 含 F5a）
- **评审人**：`reviewer-b`（独立评审，≠ 作者；跨模块 CLI 小改 Lead 已批准，第二评审 Lead 追加）
- **变更范围**：`568c0f3..4533d44`（5 files，+391/−9）：`harness/plugin.rs`、`harness/executor.rs`、`harness/llm/types.rs`、`cli/run_cmd.rs`、`cli/tests/observability.rs`；无 Cargo
- **评审时间**：2026-11-04
- **关联**：task-124/126、DR-018 §1.2、DR-020 FINAL、AC-27、F3a1/F3b、REV-128

## 1. 结论

**结论：通过**

P0=0；P1=0；P2=5；P3=3。外部 subprocess 缺 usage → `UsageSource::Unavailable`（不再 zero/Provider）；executor 记入 run/node/agent 统计（unknown++/warning、token/cost 不计预算）；CLI `--json.usage` `available=false`/`unavailable`、人类行 `usage unavailable (external…)`、trace stats 无伪造；混合场景显示已知+unknown；内置/默认输出无回归。harness **386/0/1**（默认/features）、workspace **1294/0/3（2 次）**、cli **270/0**、pytest **195/2 skipped**、clippy/fmt/check/no-default 全绿。

无 P1；Lead 预裁决开放项见 §5。

## 2. 四维评审

| 维度 | 结论 | 依据 |
|---|---|---|
| 功能正确性 | ✅ | Unavailable 映射/统计/CLI 标注/trace 不伪造；混合/外部/内置路径 |
| 结构合理性 | ✅ | plugin usage 解析、executor 统一记录、CLI 展示分层；无 Cargo |
| 逻辑性 | ✅（P2） | unknown_usage_calls/available 口径一致；不计预算；混合保留已知值 |
| 可读性 | ✅（P3） | 注释/字段/测试名清楚；P3 文档 |

## 3. 发现清单

### P2（不阻断；Lead 预裁决内）

| # | 位置 | 问题 | 建议 |
|---|---|---|---|
| N1 | `run_cmd.rs usage_json/usage_human_line` | 外部 unavailable 人类行后缀统一 `external harness usage unavailable`（不区分 Codex/DSH source）；JSON `unavailable` 同为保守字符串 | Lead 已接受 D6 可按 `usage.source` 精化；当前不阻塞 |
| N2 | `usage_totals_json` | 新增 `available` 字段，但 token 字段仍保留 0；全不可得时 `run.available=false` + 顶层 `unavailable`，不把 0 当真实值（Lead 接受 schema 兼容） | D6 文档；消费方以 available/unavailable 判断 |
| N3 | 预算 | 外部 Unavailable 不计 token → `--max-tokens` 对纯外部 run **无约束力**；`--max-cost` 外部仍在 CLI 启动前 `cost_unavailable_external_harness` fail-closed | Lead 预裁决接受；D6 文档明确「max-tokens 仅对可统计内置调用」 |
| N4 | `usage_json` 0 调用 | `llm_calls=0` 时 `available=false` 但无顶层 `unavailable`（human 无 stats 时显示 no agent LLM calls；stats Some 0 时显示 0 token） | 可补 0 调用 unavailable；D6 |
| N5 | `plugin.parse_usage` | 空对象/无可识别字段=Unavailable；显式 `input_tokens:0/output_tokens:0` 仍为 Provider 零（真实零）；口径合理但需文档 | D6 明确「显式零 vs 缺失」 |

### P3（测试/文档）

| # | 位置 | 问题 | 建议 |
|---|---|---|---|
| N6 | `trace --stats` | 外部不写 `llm_calls`，stats 无 token/llm 键（不伪造）；现有测试通过 | D6 明确 external 无 trace 统计 |
| N7 | 混用 | external+内置混合 agent 时 `external_harness` 后缀对全 run 生效（保守）；已知值+unknown 仍正确 | D6 可按 source 精化 |
| N8 | 范围 | 5 files 含 CLI 跨模块（Lead 批准）；默认/内置输出无回归测试通过 | 合并时保留第二评审 |

## 4. 验证证据

worktree `/Users/zyzs/lczyzs/code/zgraph/.worktrees/harness-dsl-dev`，HEAD `4533d44`，base `568c0f3`，工作区干净；`CARGO_HOME=/Users/zyzs/lczyzs/code/zgraph/.cargo-home`，未设 `CARGO_TARGET_DIR`：

```bash
cargo test -p zgraph-harness                 # 387 tests：386 passed / 0 failed / 1 ignored
cargo test -p zgraph-harness --features mcp  # 387 tests：386 passed / 0 failed / 1 ignored
cargo test --workspace                       # 93 targets：1294 passed / 0 failed / 3 ignored（2 次一致）
cargo test -p zgraph-cli                     # 270 passed / 0 failed
pytest -q                                    # exit 0；195 passed / 2 skipped（REAL_DSH 默认 skip）
cargo clippy/fmt/check/no-default            # clean
bash scripts/dev/check.sh                    # PASSED
```

- **与自报一致**：harness 386/0/1、workspace 1294/0/3 ×2、cli/pytest/clippy/fmt/check；5 files，无 Cargo；`git status` 空。
- **已核**：`parse_usage(None)`/空对象/无可识别字段→Unavailable；`result_without_usage`→Unavailable（prompt/completion/total 0、reasoning None、source Unavailable）；executor 外部成功 run 记 `llm_calls=1/unknown_usage_calls=1`、by_node/by_agent unknown、warnings、`budget_stop=None`（max_tokens=1 不触发）；内置/默认路径不变。
- **CLI 端到端**：外部 fake subprocess 无 usage → status Completed、usage `available=false`、`unknown_usage_calls=1`、顶层 `external_harness_usage_unavailable`；人类 `usage unavailable` + external 后缀、无 `prompt=0`；`trace --stats` 无 `llm_calls`/`token_usage` 键；混合单测已知+unknown 输出正确、不整体 unavailable。
- **默认/内置**：`default_outputs_are_unchanged_without_observability_flags` 通过；内置有 usage 不变。
- **范围**：harness 3 + cli 2（Lead 批准跨模块），无 Rust 其它模块/Cargo。

## 5. Lead 预裁决确认

- external unavailable 后缀保守（接受，D6 可精确按 source）：**一致**。
- JSON 保留 0 值 + `available=false`/`unavailable` 标明（schema 兼容，接受）：**一致**。
- 外部 + max-tokens 无预算约束力（文档记录，接受）：**一致**（Unavailable 不计 token；`max_cost` 外部 fail-closed 不变）。
- `max-cost` 外部仍 fail-closed（CLI `cost_unavailable_external_harness`）：**一致**。
- 外部无 `llm_calls`/trace token 不伪造：**一致**。
- 新 reason D6 回写：**接受**。

## 6. 遗留问题

- **P2 N1–N5**：source 精确后缀、available schema、max-tokens 外部无约束、0 调用 unavailable、显式零文档。
- **P3 N6–N8**：trace 外部无统计、混合 source、跨模块第二评审。
- F5d（Codex mock）在 e2e-dev 进行中，另行评审。
- 无 P0/P1；F5e 可 merge。

## 7. 第二评审 · 跨模块（CLI）（Lead 追加，2026-11-04）

- **评审人**：`lead`（**≠ 作者** `dsl-dev`；Lead 已批准跨模块 CLI 输出小改，按 AGENTS §2 追加第二评审）
- **范围**（`568c0f3..1711c64`）：`crates/zgraph-harness/src/{plugin,executor,llm/types}.rs`（外部 subprocess 无 usage → `UsageSource::Unavailable`，不再 zero/Provider；统计 unknown/warnings、不计预算）；`crates/zgraph-cli/src/run_cmd.rs` + `tests/observability.rs`（`available`/`unavailable` 标注、人类行不显示伪 0、默认/内置逐字节不变）。
- **核对结论（通过）**：不伪造 token/cost（Unavailable 不计预算、trace 不出现伪造 token 键）；默认与内置 provider 输出不变；CLI 标注 additive；测试覆盖 parse_usage(None/空)/executor/CLI 端到端。
- **Lead 亲自验证**：`git diff 568c0f3..1711c64 --stat` 仅 harness 3 源 + cli 2 文件；`cargo test --workspace --offline` = 1294/0/3；harness 386/0/1；pytest 195/2 skipped；check.sh PASSED。
- **裁决/后续**：external 后缀保守（D6 可按 source 精化）；JSON 0 字段+available=false 为 schema 兼容接受；外部+max-tokens 无预算约束力文档记录；max-cost 外部 fail-closed 不变；外部无 llm_calls → trace 无 token 属预期。
- **结论**：**通过（第二评审）**。分支满足合并前置。
