# REV-065 · e2e · software_team v1（W1）端到端评审

- **分支**：`test/e2e-software-team-v1`
- **作者**：`w1-e2e`（初版 3 提交 `1b8bbcf`/`dd285d6`/`dd335fc`；修复提交 `8c1fd59`；均含 `Review: REV-065` / `Agent: w1-e2e`）
- **评审人**：`reviewer-sop`（**≠ 作者**；独立评审；首轮 `02252c9`）
- **变更范围**：`715f13a..8c1fd59`，37 文件 **+2658/−0**（全部 `e2e` 模块：`tests/e2e/**`、`tests/fixtures/graphs/**`）；其中首轮 32 文件 +2143，`8c1fd59` 仅 5 个 e2e 文件（分支内 +415/−15）：`tests/e2e/software_team_v1.rs`、`codegen.py`、`agents/design_reviewer_exhaust.json`、`fake_llm_max_iterations.json`、`tools/echo.py`；未改 kernel/dsl/harness/worker/cli/python/doc/CI
- **评审时间**：2026-10-31
- **关联**：DR-004 §3 W1（`doc/decisions/DR-004-v02-scope-and-waves.md:56`）；SOP 设计稿 v1 §4.1/§5.2/§5.3/§7/§11（`doc/design/software-team-sop-v1.md`）；REV-062/REV-063/REV-064；spec 04 §4.3/§4.5、05 §5.9

## 1. 结论

**结论：通过**

初版 e2e 主体真实有效：codegen 按 §4.1 生成 full（60 节点/100 边）与 smoke（11 节点/18 边），full 经真 `zgraph validate --assets-dir`（R1–R14，含 R14c）通过且两次生成字节级一致/`graph_hash` 相同；smoke 用真 CLI + fake LLM + 真 kernel/worker + 真 trace 跑通 planer → 3 工作 → 3 评审 → 3 解析 → archer，断言 per-item 实例、`P.artifact` 原样透传（非评审摘要）、approved gate、`trigger=all`；rework/非法 JSON 失败路径断言 `NODE_PYTHON_EXCEPTION`/Crashed/run Failed/无 approved gate 放行；real-LLM smoke 严格 `#[ignore]` + 双 env。

首轮 C1（重要）已由 `8c1fd59` 关闭：新增 `smoke_reviewer_max_iterations_crashes_review_and_fails_run`（`design_reviewer_exhaust`：`max_iterations=1` + `tools=["echo"]`；fake 剧本 `call0=planer`、`call1..3=tool_calls`；1 次 LLM + 1 次工具后预算检查命中 `HARNESS_MAX_ITERATIONS`），断言 CLI `--json`/`runs`/评审实例/trace 四面一致（Failed、`HARNESS_MAX_ITERATIONS`、Crashed、`harness_kind=MaxIterations`、`detail.max_iterations=1`、`pending_tool_calls=["echo"]`）、解析节点未执行、无 approved gate 放行、archer 未执行。S1（full 符号/资产加载校验）与 S3（矩阵权威/合成 smoke 说明）也已落实。

**复审未发现新的阻塞/重要问题**；仅 S2（reviewer 输入快照断言）仍为建议（不阻塞）。本记录结论为 **通过**。

## 2. 四维评审

| 维度 | 结论 | 依据（文件/行/测试） |
|---|---|---|
| 功能正确性 | ✅（S2 备注） | full/smoke codegen 与 §4.1 逐项一致（`codegen.py:83-104`，独立 oracle `software_team_v1.rs:74-117`）；full validate 60/100、smoke 11/18；smoke approved 链路断言 per-item 实例/artifact/gate/trace；P 失败路径（rework/非法 JSON）与 `HARNESS_MAX_ITERATIONS` 均断言 run Failed；未把 gate=false Completed(null) 当失败；real-LLM ignored |
| 代码结构合理性 | ✅ | 37 文件全部 e2e 模块；`tests/e2e/Cargo.toml` 仅新增 target；codegen/agents/prompts/py/tools/fake_llm 布局清晰；`--review-agent` 仅换评审资产、默认图 hash 不变；未改其他模块/CI |
| 逻辑性 | ✅ | codegen 纯函数/确定性；full/smoke 规模自检；fake 调用序（planer 唯一先执行、3 评审内容相同、archer 最后）稳定；`max_iterations` 机制（1 次 LLM + 1 次工具 → 预算检查）与 05 §5.5.2 一致；失败路径四面证据一致；full/smoke/exhaust hash 复现 |
| 可读性 | ✅（S2 备注） | 文件头/函数注释解释"为什么"（数据通路、fake 限制、失败语义、S1/S3）；新资产/工具/fake 剧本有 docstring；测试命名清晰；S2 为追加断言建议 |

## 3. 发现清单

| # | 级别 | 位置 | 问题 | 建议 | 状态 |
|---|---|---|---|---|---|
| C1 | 重要 | `doc/decisions/DR-004-v02-scope-and-waves.md:56`、SOP §11.4、`tests/e2e/software_team_v1.rs` | W1 失败路径要求包含 `HARNESS_MAX_ITERATIONS`，首轮 e2e 只覆盖 `NODE_PYTHON_EXCEPTION` | 新增 e2e：`design_reviewer_exhaust`（`max_iterations=1` + `echo` 工具）+ `fake_llm_max_iterations.json`，断言 CLI/run/实例/trace 一致、无 approved gate、archer 未执行 | 已关闭（`8c1fd59`） |
| S1 | 建议 | full 图 32 个 `python_node` / 13 agent 资产 | full 仅 validate，未 import 符号/加载资产 | 新增 `full_graph_python_symbols_and_agent_assets_resolve`：32 个 symbol 真 import/可调用（17 去重符号）、13 资产可解析/文件名=id/prompt 存在/12 引用全命中 | 已落实（`8c1fd59`） |
| S2 | 建议（低） | `software_team_v1.rs:869-925`（`assert_approved_item_chain`） | approved 链路断言 W/R/P 输出与 archer 输入，但未断言 reviewer 输入快照（`spec`/`result`）；fake LLM 不读输入，接错仍可能通过 | 从 trace `instances(...).input` 断言每 item 的 `{item}_review.input.spec == planer_output`、`.result == work_output`；可后置/登记 ISSUE | open（建议，非阻塞） |
| S3 | 建议 | `codegen.py:106-123`、`software_team_v1.rs:74-123` | codegen `FULL_ROLES` 与测试 `DESIGN_MATRIX` 是两份手工副本；smoke 是合成单阶段（非 §4.1 子集），易误解 | 注释明确 `FULL_ROLES` 权威/`DESIGN_MATRIX` 独立 oracle；smoke 注明合成单阶段（已落实） | 已落实（`8c1fd59`） |

> 级别口径：阻塞 = 必须修复后才能给"通过"；重要 = 必须修复或登记 ISSUE；建议 = 可后置，但需修复或进台账。

## 4. 验证证据

```bash
# --- 范围/干净度 ---
cd /Users/zyzs/lczyzs/code/zgraph/.worktrees/e2e-w1-e2e
git status --porcelain --branch        # ## test/e2e-software-team-v1...origin/main [ahead 5]；clean
git diff 02252c9..8c1fd59 --shortstat  # 5 files changed, 415 insertions(+), 15 deletions(-)（修复仅 e2e 5 文件）
git diff 715f13a..8c1fd59 --shortstat  # 37 files changed, 2658 insertions(+)
git diff 715f13a..8c1fd59 --name-status # 全部 tests/e2e/**、tests/fixtures/graphs/**
git rev-parse HEAD origin/main          # 8c1fd59... / 715f13a...（base = origin/main）

# --- 仓库自检 ---
bash scripts/dev/check.sh
# CHECK PASSED（身份/冻结文件/分支/语法/cargo fmt/git diff --check 全 OK）

# --- CLI 裸 bin ---
CARGO_HOME=/Users/zyzs/lczyzs/code/zgraph/.cargo-home CARGO_NET_OFFLINE=true \
  cargo build -p zgraph-cli --bin zgraph --locked
# Finished `dev` profile ... in 0.52s

# --- 指定 e2e target（复审）---
CARGO_HOME=... CARGO_NET_OFFLINE=true cargo test -p zgraph-e2e --test software_team_v1
# running 8 tests
# real_llm_smoke_manual ... ignored, manual: ...
# smoke_codegen_is_isomorphic_single_stage_pipeline ... ok
# full_graph_python_symbols_and_agent_assets_resolve ... ok
# full_codegen_validates_with_60_nodes_100_edges_and_stable_hash ... ok
# smoke_invalid_review_json_crashes_parse_and_fails_run ... ok
# smoke_rework_verdict_crashes_parse_and_fails_run ... ok
# smoke_reviewer_max_iterations_crashes_review_and_fails_run ... ok
# smoke_run_approved_pipeline_propagates_artifacts_and_enforces_gate ... ok
# test result: ok. 7 passed; 0 failed; 1 ignored

# --- 全 e2e 包 ---
CARGO_HOME=... CARGO_NET_OFFLINE=true cargo test -p zgraph-e2e
# 汇总：46 passed / 0 failed / 2 ignored（exit 0）

# --- Python 回归 ---
/Users/zyzs/lczyzs/code/zgraph/.venv/bin/python -m pytest python/tests
# 104 passed in 1.75s（exit 0；基线 0）

# --- codegen/validate + hash 回归 ---
python tests/fixtures/graphs/software_team_v1/codegen.py --profile full --out /tmp/full.json
python tests/fixtures/graphs/software_team_v1/codegen.py --profile smoke --out /tmp/smoke.json
python tests/fixtures/graphs/software_team_v1/codegen.py --profile smoke \
  --review-agent design_reviewer_exhaust --out /tmp/exhaust.json
target/debug/zgraph validate /tmp/full.json --assets-dir tests/fixtures/graphs/software_team_v1
# ✓ graph valid: 60 nodes, 100 edges, 0 subgraphs (graph_hash=3d8d735b1d5e8a1b)
target/debug/zgraph validate /tmp/smoke.json --assets-dir tests/fixtures/graphs/software_team_v1
# ✓ graph valid: 11 nodes, 18 edges, 0 subgraphs (graph_hash=df9341c04063b80a)
target/debug/zgraph validate /tmp/exhaust.json --assets-dir tests/fixtures/graphs/software_team_v1
# ✓ graph valid: 11 nodes, 18 edges, 0 subgraphs (graph_hash=a8e508deb9c6cf44)
# 默认 full/smoke hash 与首轮一致；--review-agent 仅变体图 hash 变化（S1/C1 不破坏既有图）
```

- 测试结果：指定 target **7 passed / 1 ignored**；全 e2e **46 passed / 2 ignored**；pytest **104 passed**；均无失败（失败基线 0）。
- 手工验证：full/smoke/exhaust validate 规模与 hash 符合；默认 full/smoke hash 未变；`HARNESS_MAX_ITERATIONS` e2e 断言覆盖 CLI/run/实例/trace；S1 的 32 symbol/13 资产解析通过；S3 注释已落实。
- 本评审未改产品/fixture/测试；评审记录与索引属 docs 模块（按 AGENTS §3/§6 落在此分支，作者 w1-e2e、评审人 reviewer-sop ≠ 作者）。

## 5. 遗留问题

- **C1 已关闭；S1/S3 已落实；S2 仍为建议（低）**：reviewer 输入快照断言未加，可后置或登记 `doc/issues.md`，不阻塞。
- 附带说明：派单写首轮 diff `+1969`，实际首轮 `715f13a..dd335fc` 为 32 files +2143；含修复后 `715f13a..8c1fd59` 为 37 files +2658（以实际为准）。
- 未发现越界改动（仅 e2e 模块）、禁用文件、冻结文件修改；分支 base = `origin/main`；worktree clean；本评审未 merge/push/切 main。

## 6. 条件关闭记录（仅"有条件通过"填写）

| 条件 | 关闭证据 | 关闭日期 |
|---|---|---|
| C1（重要）`HARNESS_MAX_ITERATIONS` e2e | `8c1fd59` 新增 `smoke_reviewer_max_iterations_crashes_review_and_fails_run`；`design_reviewer_exhaust`（`max_iterations=1` + `echo`）+ `fake_llm_max_iterations.json`；CLI `--json`/runs/实例/trace 断言 Failed、`HARNESS_MAX_ITERATIONS`、Crashed、`harness_kind=MaxIterations`、`detail.max_iterations=1`、`pending_tool_calls=["echo"]`；解析未执行、无 approved gate、archer 未 Completed；target 7 passed/1 ignored | ✅ 关闭 2026-10-31 |

> C1 已关闭，S2 为非阻塞建议；本记录结论 **通过**。合并仍须通过 `scripts/dev/merge.sh` 的 build/test/clippy/pytest 门禁。
