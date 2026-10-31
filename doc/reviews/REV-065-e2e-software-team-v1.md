# REV-065 · e2e · software_team v1（W1）端到端评审

- **分支**：`test/e2e-software-team-v1`
- **作者**：`w1-e2e`（3 个提交：`1b8bbcf` codegen+validate、`dd285d6` smoke fake-LLM+失败路径、`dd335fc` real-LLM ignored；均含 `Review: REV-065` / `Agent: w1-e2e`）
- **评审人**：`reviewer-sop`（**≠ 作者**；独立评审）
- **变更范围**：`715f13a..dd335fc`，32 文件 **+2143/−0**（全部 `e2e` 模块：`tests/e2e/**`、`tests/fixtures/graphs/**`）：`tests/e2e/software_team_v1.rs`（1109 行）、`tests/e2e/Cargo.toml`（+4）、`tests/fixtures/graphs/software_team_v1/{codegen.py,py/software_team_stub.py,agents/*,fake_llm_*.json,tools/.gitkeep}`；未改 kernel/dsl/harness/worker/cli/python/doc/CI
- **评审时间**：2026-10-31
- **关联**：DR-004 §3 W1（`doc/decisions/DR-004-v02-scope-and-waves.md:56`）；SOP 设计稿 v1 §4.1/§5.2/§5.3/§7/§11（`doc/design/software-team-sop-v1.md`）；REV-062/REV-063/REV-064；spec 04 §4.3/§4.5、05 §5.9

## 1. 结论

**结论：有条件通过**

W1 的 e2e 主体真实有效：codegen 按 SOP §4.1 生成 full（60 节点/100 边）与 smoke（11 节点/18 边）；full 经真 `zgraph validate --assets-dir`（R1–R14，含 R14c）通过且两次生成字节级一致、`graph_hash` 相同；smoke 用真 CLI + fake LLM + 真 kernel/worker + 真 trace 跑通 planer → 3 工作 → 3 评审 → 3 解析 → archer，断言 per-item 实例、`P.artifact` 原样透传（非评审摘要）、approved gate、`trigger=all`；rework/非法 JSON 失败路径断言 `NODE_PYTHON_EXCEPTION`/Crashed/run Failed/无 approved gate 放行；real-LLM smoke 严格 `#[ignore]` + 双 env；check.sh/PASS、目标 target 5 passed/1 ignored、全 e2e 44 passed/2 ignored、pytest 104 passed 均复现。

但存在 **1 项重要**：W1 的失败路径要求（DR-004:56 / SOP §11.4）包含 **`HARNESS_MAX_ITERATIONS`**，本分支 e2e 只覆盖 `NODE_PYTHON_EXCEPTION`（P 的 rework/非法 JSON），没有 `HARNESS_MAX_ITERATIONS` 的 e2e 用例（`grep HARNESS_MAX` 在 `tests/e2e/**` 无匹配；现有 kernel `tests/failure.rs:216` 只验证错误码映射，不是 SOP e2e 失败链路）。若该失败路径确属 W1 交付，应补一条 e2e（fake tool-loop 耗尽 `max_iterations` → run Failed）或由 Lead 明确从本分支 W1 验收中排除并登记 ISSUE；关闭前本记录为「有条件通过」。

另有 3 条建议（不阻塞）：full 图的 18 个 stub 函数/12 个 agent 资产仅 validate（未 import/config-load）；smoke 未断言 reviewer 的 `spec`/`result` 输入快照；smoke profile 为合成单阶段（非 §4.1 子集），文档可再注明。

## 2. 四维评审

| 维度 | 结论 | 依据（文件/行/测试） |
|---|---|---|
| 功能正确性 | ⚠️（C1） | full/smoke codegen 与 §4.1 依赖矩阵逐项一致（`codegen.py:83-104`，独立 oracle `software_team_v1.rs:74-117`）；full validate 60/100（hash `3d8d735b1d5e8a1b`）、smoke 11/18（hash `df9341c04063b80a`）；smoke approved run 断言 11 实例/artifact/gate/trace；failure P 路径断言 NODE_PYTHON_EXCEPTION。**缺 `HARNESS_MAX_ITERATIONS` e2e**（C1） |
| 代码结构合理性 | ✅ | 32 文件全部在 `e2e` 模块；`tests/e2e/Cargo.toml` 仅新增 test target；codegen/agents/prompts/py/fake_llm/tools 布局清晰；未改 kernel/dsl/harness/worker/cli/python/doc/CI；无跨模块复制/既有 fixture 冲突（新目录 `software_team_v1/` 独立） |
| 逻辑性 | ✅ | codegen 纯函数 + 确定性序列化；full/smoke 规模自检；edge 唯一性/端口/入边/end 约束由 validate 与结构断言双覆盖；fake 调用序确定性（planer 唯一先执行、3 评审内容相同、archer 最后）有注释；失败路径明确区分 P 抛错与 gate=false |
| 可读性 | ✅（S1–S3） | 文件头/函数注释解释"为什么"（数据通路、fake 限制、失败语义）；测试命名清晰；新 fixture 有 docstring；S1–S3 为测试覆盖/文档细节 |

## 3. 发现清单

| # | 级别 | 位置 | 问题 | 建议 | 状态 |
|---|---|---|---|---|---|
| C1 | **重要** | `doc/decisions/DR-004-v02-scope-and-waves.md:56`、`doc/design/software-team-sop-v1.md:269`、`tests/e2e/software_team_v1.rs`（无 HARNESS_MAX） | **W1 失败路径要求包含 `HARNESS_MAX_ITERATIONS`，本分支 e2e 未覆盖**：DR-004:56 列"失败路径（`NODE_PYTHON_EXCEPTION` / `HARNESS_MAX_ITERATIONS`）"，SOP §11.4 同；e2e 只有 `smoke_rework_verdict_crashes_parse_and_fails_run` 与 `smoke_invalid_review_json_crashes_parse_and_fails_run`（均 NODE_PYTHON_EXCEPTION），`grep HARNESS_MAX` 在 `tests/e2e/**` 无匹配。kernel `crates/zgraph-kernel/tests/failure.rs:216` 只测 NodeError→code 映射，不是 SOP fake e2e 失败链路 | 补 e2e：用 fake tool-loop/受控工具把评审 agent 的 `max_iterations` 耗尽 → 断言 `HARNESS_MAX_ITERATIONS`/实例 Crashed/run Failed（可新增 test-only agent/tool 或复用现有 tool fixture）；若 Lead 决定 W1 本分支不覆盖该路径，则从 DR-004 W1/07 AC 中明确排除并登记 ISSUE，再走复审 | open（条件） |
| S1 | 建议（低） | `tests/e2e/software_team_v1.rs:390-522`（full 仅 validate）、`codegen.py:204-268`、`py/software_team_stub.py`（18 函数） | full 图只做 validate（R14 不 import python_node）；测试结构断言只比较 `module/function` 字符串，不验证 18 个 stub 函数可导入/12 个 agent 资产可加载。full 的 13 个非 smoke stub 函数与 9 个非 smoke 设计角色资产在 W1 内从未执行/加载（已人工抽查 32 个 python_node symbol 均可导入，但测试未固化） | 在 e2e 增加轻量校验：`PYTHONPATH=python:fixtures/py` import 全部 full python_node symbol；或对 12 个 agent 资产走 HarnessConfig 轻量加载（不需 LLM）；或明确 full 仅 validate 为 W1 接受项并写进 AC | open（建议） |
| S2 | 建议（低） | `tests/e2e/software_team_v1.rs:723-780`（`assert_approved_item_chain`） | approved 链路断言了 W/R/P 输出、`P.artifact`、archer 输入，但没有断言 **reviewer 的输入快照**（`spec` 来自 D、`result` 来自 W）。fake LLM 不读输入，故 reviewer 输入若接错仍可能通过 | 从 trace `instances(...).input` 断言 `{planer_item}_review.input.spec == planer_output`、`.result == work_output`（每 item） | open（建议） |
| S3 | 建议（低） | `codegen.py:106-123`（SMOKE_ROLES）与 `software_team_v1.rs:120,525` | smoke profile 是**合成单阶段**（planer 3 items 含 `requirement_analysis`/`iter_plan`，并非 §4.1 的 planer N=1 子集），虽然文件头注释说"缩小但同构"，读者易误以为 smoke 是 §4.1 矩阵的子集；另 `codegen.py` 的 `FULL_ROLES` 与测试 `DESIGN_MATRIX` 是两份手工副本，长期有漂移风险 | 在 smoke 说明中明确"合成 profile，不承诺与 §4.1 子集一致"；给 `DESIGN_MATRIX` 加注释指向 SOP §4.1 或由 codegen 导出只读矩阵供测试比对 | open（建议） |

> 级别口径：阻塞 = 必须修复后才能给"通过"；重要 = 必须修复或登记 ISSUE；建议 = 可后置，但需修复或进台账。

## 4. 验证证据

```bash
# --- 范围/干净度 ---
cd /Users/zyzs/lczyzs/code/zgraph/.worktrees/e2e-w1-e2e
git status --porcelain --branch        # ## test/e2e-software-team-v1...origin/main [ahead 3]；clean
git diff 715f13a..HEAD --shortstat     # 32 files changed, 2143 insertions(+), 0 deletions
git diff 715f13a..HEAD --name-status   # 仅 tests/e2e/software_team_v1.rs、tests/e2e/Cargo.toml、tests/fixtures/graphs/software_team_v1/**
git rev-parse HEAD origin/main         # dd335fc... / 715f13a...（base = origin/main）

# --- 仓库自检 ---
bash scripts/dev/check.sh
# CHECK PASSED（身份/冻结文件/分支/语法/cargo fmt/git diff --check 全 OK）

# --- CLI 裸 bin（e2e 依赖）---
CARGO_HOME=/Users/zyzs/lczyzs/code/zgraph/.cargo-home CARGO_NET_OFFLINE=true \
  cargo build -p zgraph-cli --bin zgraph --locked
# Finished `dev` profile ... in 0.52s

# --- 指定 e2e target ---
CARGO_HOME=... CARGO_NET_OFFLINE=true cargo test -p zgraph-e2e --test software_team_v1
# running 6 tests
# real_llm_smoke_manual ... ignored, manual: ...
# smoke_codegen_is_isomorphic_single_stage_pipeline ... ok
# full_codegen_validates_with_60_nodes_100_edges_and_stable_hash ... ok
# smoke_invalid_review_json_crashes_parse_and_fails_run ... ok
# smoke_rework_verdict_crashes_parse_and_fails_run ... ok
# smoke_run_approved_pipeline_propagates_artifacts_and_enforces_gate ... ok
# test result: ok. 5 passed; 0 failed; 1 ignored

# --- 全 e2e 包 ---
CARGO_HOME=... CARGO_NET_OFFLINE=true cargo test -p zgraph-e2e
# 各 test target 均 ok；汇总 44 passed / 0 failed / 2 ignored（exit 0）

# --- Python 回归 ---
/Users/zyzs/lczyzs/code/zgraph/.venv/bin/python -m pytest python/tests
# 104 passed in 2.26s（exit 0；基线 0）

# --- 生成/validate 手工抽查 ---
python tests/fixtures/graphs/software_team_v1/codegen.py --profile full --out /tmp/full.json
# generated full: 60 nodes, 100 edges -> /tmp/full.json
python tests/fixtures/graphs/software_team_v1/codegen.py --profile smoke --out /tmp/smoke.json
# generated smoke: 11 nodes, 18 edges -> /tmp/smoke.json
target/debug/zgraph validate /tmp/full.json --assets-dir tests/fixtures/graphs/software_team_v1
# ✓ graph valid: 60 nodes, 100 edges, 0 subgraphs (graph_hash=3d8d735b1d5e8a1b)
target/debug/zgraph validate /tmp/smoke.json --assets-dir tests/fixtures/graphs/software_team_v1
# ✓ graph valid: 11 nodes, 18 edges, 0 subgraphs (graph_hash=df9341c04063b80a)

# --- 资产/符号抽查 ---
ls tests/fixtures/graphs/software_team_v1/agents/*.json | wc -l          # 12（11 设计角色 + reviewer）
ls tests/fixtures/graphs/software_team_v1/agents/prompt_*.md | wc -l     # 12
grep -c "^def work_" tests/fixtures/graphs/software_team_v1/py/software_team_stub.py  # 18
# 用 PYTHONPATH=python:tests/fixtures/graphs/software_team_v1/py import full 图的 32 个 python_node symbol → missing: []（人工抽查，测试未固化）
grep -rn "HARNESS_MAX_ITERATIONS" tests/e2e tests/fixtures/graphs/software_team_v1 || echo "NO HARNESS_MAX e2e case"
# → C1
```

- 测试结果：目标 target 5 passed / 1 ignored；全 e2e 44 passed / 2 ignored；pytest 104 passed；均无失败；失败基线为 0。
- 手工验证：full/smoke validate 输出与规模/资产索引一致；full 32 个 python_node symbol 可导入（但未进测试）；agent 资产/提示词 12/12；`HARNESS_MAX_ITERATIONS` 在 e2e 无覆盖（C1）。
- 本评审未改产品/fixture/测试；评审记录与索引属 docs 模块（按 AGENTS §3/§6 落在此分支，作者 w1-e2e、评审人 reviewer-sop ≠ 作者）。

## 5. 遗留问题

- **C1（重要）**：需补 `HARNESS_MAX_ITERATIONS` e2e，或由 Lead 明确本分支 W1 验收排除该路径并登记 ISSUE；关闭后复审改判。
- **S1/S2/S3（建议）**：full 符号/资产加载校验、reviewer 输入快照断言、smoke 合成 profile/矩阵副本说明；可后置或登记 `doc/issues.md`。
- 附带说明：Lead 派单写 diff `+1969`，实际 `git diff 715f13a..HEAD --shortstat` 为 **32 files +2143/−0**（以实际为准；不影响评审结论）。
- 未发现越界改动（仅 e2e 模块）、禁用文件、冻结文件修改；分支 base = `origin/main`；worktree clean；本评审未 merge/push/切 main。

## 6. 条件关闭记录（仅"有条件通过"填写）

| 条件 | 关闭证据 | 关闭日期 |
|---|---|---|
| C1（重要）HARNESS_MAX_ITERATIONS e2e | 待补 e2e（或 Lead 明确排除并登记 ISSUE/更新 DR-004 W1/AC） | — |

> 本记录结论为"有条件通过"；C1 关闭后复审改判。
