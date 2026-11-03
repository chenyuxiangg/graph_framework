# REV-122 · docs · reexec 缺 sha 语义修正（MISMATCH/MISSING 区分）

> **评审范围**：docs-only（7 文件）；对齐 F1c 实测：**行存在但字段缺失/损坏 → `REPLAY_TOOL_RECORD_MISMATCH`；`REPLAY_TOOL_RECORD_MISSING` 仅行缺失/streaming 多于记录**。无代码/测试/既有评审改动；未触碰 `dist/`、无 token。评审人只新增本评审记录。

- **分支**：`docs/docs-v05-reexec-sha`
- 分支: docs/docs-v05-reexec-sha
- **作者**：`docs-dev`（commit `f14a2fc`；正文 `Review: REV-122` / `Agent: docs-dev`）
- **评审人**：`reviewer-a`（独立评审，≠ 作者）
- **基于**：`21846d8`（REV-119 收口）；**main 已到 `ec7da51`（F1c/REV-121 已合并）**
- **变更范围**：`21846d8..f14a2fc`（**7 files, +15/−14**）：`DR-017`、`spec 05 §5.16.1/06 §6.16.1/07 AC-25/08 §8.13`、`index`、`progress`；`git diff main...HEAD` 同 7 文件（docs-only）
- **评审时间**：2026-11-03
- **关联**：task-110/111；F1c/REV-121；DR-017、AC-25；REV-119

## 1. 结论

**结论：通过**

> **结论历史**：首轮（基准 `f14a2fc`/基于 `21846d8`）为「**有条件通过**」——P1 C1 分支未 rebase 到含 F1c 的 main `ec7da51`；P2 C2 progress/AC-25 仍写 F1c 待。作者 rebase 并 `989a062` 补 C2/P3 后，评审人复核 ancestry/diff/progress/AC-25/check.sh（§8），C1/C2 closed，结论改**通过**。
>
> 首轮意见（历史保留）：文档语义修正正确（缺 sha=MISMATCH、行缺失=MISSING，与 F1c `ac25e`/validate 一致）；C1/C2 流程/状态同步。

### 1.1 已核对成立

| 项 | 结论 |
|---|---|
| 语义修正 | ✅ 行存在但字段缺失/损坏（缺 `result_sha256`/JSON 非法/sha 不符）→ `REPLAY_TOOL_RECORD_MISMATCH`；行缺失/streaming `tool_call_start` 多于记录 → `REPLAY_TOOL_RECORD_MISSING`；`truncated=1` → `REPLAY_TOOL_RECORD_TRUNCATED`；三码区分清晰 |
| 与实现/F1c 一致 | ✅ `validate_tool_calls` 先 `verify_tool_json`：`result_json` 存在但 `result_sha256=None` → MISMATCH（不可复核）；F1c `ac25e` 实测 `SET result_sha256=NULL` → MISMATCH、删除行 → MISSING（REV-121）；F1b 候选循环内 MISSING 路径在 validate 之后不可达，不改代码符合 Lead 裁决 |
| DR-017/05/06/07/08 | ✅ DR-017 §2 表/§6 f/§7 #1、05 §5.16.1、06 §6.16.1、07 AC-25 e/f、08 §8.13 均同步；旧 REV-119「缺 sha=MISSING」残留已清（以「行缺失」限定 MISSING） |
| index | ✅ DR-017 摘要仍为候选/非候选/exit 口径；reviews 范围到 122 |
| 范围/门禁 | ✅ docs-only 7 文件；无代码/测试/既有评审改动；无 token/dist；`check.sh` **PASSED** |
| 历史 | ✅ 保留 REV-119 结论历史语境（改为行缺失），不删除既有评审 |

## 2. 四维评审

| 维度 | 结论 | 依据 |
|---|---|---|
| 功能正确性 | ⚠️ 有条件 | 语义修正内容正确；C1 分支未 rebase 到含 F1c 的 main、C2 F1c 状态仍待 |
| 代码结构合理性 | ✅ | docs-only；DR-017 表/错误码区分集中；spec 分册同步 |
| 逻辑性 | ✅ | 行缺失 vs 行存在字段损坏、truncated、MISMATCH 边界自洽；与 validate 顺序一致 |
| 可读性 | ✅（P3） | 错误码区分段清晰；P3 为 index/历史注记 |

## 3. 发现清单

### 3.1 P1 条件（merge 前必须关闭）

| # | 位置 | 问题 | 建议 |
|---|---|---|---|
| **C1** | 分支 base `21846d8` vs main `ec7da51` | `git merge-base HEAD main = 21846d8`；`main` **不是** HEAD 祖先（HEAD 缺 F1c `d49a705`/REV-121 `ec7da51`）。merge.sh 要求分支基于最新 main；当前会因「based on latest origin/main」失败 | `git rebase main`（docs-only 应无冲突）到 `ec7da51`；复核 diff 仅 7 docs、check.sh；报告新 HEAD |

### 3.2 P2 条件（rebase 后同步）

| # | 位置 | 问题 | 建议 |
|---|---|---|---|
| **C2** | `doc/progress.md:5/11` | 仍写「F1c e2e/REV-121 评审中」「REV-122 通过 → F1c merge」；实际 F1c 已合并 main `ec7da51`、REV-121 通过 | 改「F1c/REV-121 已合并；REV-122 文档修正」；下一步 F2/F3 |
| | `doc/spec-v1.2/07-验收用例.md:73` AC-25 行 | 仍写「F1b 实现；F1c e2e 待」「完整 e2e F1c」；主体 §AC-25 也写「F1c 完整 e2e」 | 改为「F1c e2e 已实测（REV-121）」，保留 AC-25 a–f |

### 3.3 P3（建议）

- **P3-1**：F1b `crates/zgraph-cli/tests/tool_reexec.rs` 顶部注释仍写「缺 record sha → MISSING」，实际 validate 先报 MISMATCH；主代码测试断言允许两者，merge 后建议小注释修正（不改断言）。
- **P3-2**：`doc/index.md` DR-017 摘要仍写「REV-119 对齐 F1b」，可补 REV-122/F1c 最终口径；reviews 范围 122 与 reviews README 新行由本次评审补。
- **P3-3**：`DR-017` 的 F1c e2e 覆盖句可写「F1c 已实测（REV-121）」，避免与 progress 残留。

## 4. 验证证据

评审人亲自执行（worktree `.worktrees/docs-docs-dev`，HEAD `f14a2fc`，main `ec7da51`，worktree 干净）：

```bash
git status --porcelain                       # 空
git diff --stat main...HEAD                  # 7 files +15/−14（docs-only）
git merge-base HEAD main                     # 21846d8；main 非 HEAD 祖先（C1）
bash scripts/dev/check.sh                    # CHECK PASSED
# 对照 main F1c（git show main:tests/e2e/reexec_v05.rs）：
#   ac25e: SET result_sha256=NULL → 断言 REPLAY_TOOL_RECORD_MISMATCH；
#          DELETE 行 → REPLAY_TOOL_RECORD_MISSING；
# 对照 crates/zgraph-cli/src/replay_cmd.rs：
#   validate_tool_calls → verify_tool_json 先于 reexec，缺 result_sha → MISMATCH
```

- **范围/约束**：docs-only 7 文件；无 `crates/`/`tests/`/`doc/reviews/` 改动；无 token/dist；check.sh PASSED。
- **语义对照**：DR-017/05/06/07/08 与 F1c `ac25e`、`validate_tool_calls` 顺序一致；F1b MISSING 路径在候选循环内不可达（Lead 裁决不改代码）。
- **分支状态**：HEAD 未含 main 的 F1c/REV-121；C1 为流程门禁问题，不影响文档内容。

## 5. 条件关闭方式

- **C1 由 docs-dev rebase 到 main `ec7da51`**（或 Lead 认可的等价更新）；**C2 在 rebase 后更新 progress/spec07 AC-25 F1c 状态**；重跑 check.sh。
- 关闭后评审人复核 `git merge-base --is-ancestor main HEAD`、diff 仍 docs-only、progress/AC-25 无「F1c 待/评审中」+ check.sh，追加「条件关闭」段、结论改「通过」；随后 Lead merge。

## 6. 条件关闭记录（"有条件通过"）

| 条件 | 状态 | 关闭要求 |
|---|---|---|
| C1 分支 rebase 到 main ec7da51 | **closed（989a062）** | `git merge-base HEAD main=ec7da51`，main 为 HEAD 祖先；diff 含 7 docs + 评审记录，无代码/测试 |
| C2 progress/spec07 AC-25 更新 F1c 已合并/REV-121 | **closed（989a062）** | progress 改「F1c 已实测/合并（REV-121）」；AC-25 矩阵「F1c e2e 已实测，REV-121」；无「F1c 待/评审中」 |
| P3-1–P3-3 | **部分 closed（989a062）** | index DR-017 补 REV-122/F1c 最终口径；F1b 注释留 F6 文档注明（路径笔误留 P3） |

## 7. 遗留问题

- C1/C2 closed；P3-1（DR-017 F6 注记把 `tests/tool_reexec.rs` 写成 `src/tool_reexec.rs`）与 AC-25 详情标题仍「F1b 实现/F1c 完整 e2e」措辞留 D6 小修，不阻塞。
- 文档语义修正正确（缺 sha=MISMATCH、行缺失=MISSING），无代码修改；F1c 已由 REV-121 通过。
- 分支已 rebase 到 main，可 merge；之后 F2/F3。

## 8. 条件关闭复核（2026-11-03）

复核提交：`989a062`（父 `3673af2`；分支已 rebase 到 main `ec7da51`；diff 7 docs + 评审记录）。评审人执行：

```bash
git merge-base HEAD main                     # ec7da51
git merge-base --is-ancestor main HEAD       # yes（main 为祖先）
git diff --stat main...HEAD                  # 9 files（7 docs + REV-122 记录/README），无代码/测试
bash scripts/dev/check.sh                    # CHECK PASSED
grep -n "F1c.*待\|F1c.*评审中" doc/progress.md doc/spec-v1.2/07-验收用例.md  # 当前状态清零
```

| 项 | 复核结果 |
|---|---|
| C1 ancestry | ✅ `merge-base=ec7da51`；main 是 HEAD 祖先；分支含 F1c/REV-121；diff 无代码/测试 |
| C2 F1c 状态 | ✅ progress「F1c 已实测/合并（REV-121）」；AC-25 矩阵「F1c e2e 已实测，REV-121」；下一步 F2 |
| P3 | ✅ index DR-017 补 REV-122/F1c 最终口径；F6 注记路径笔误（`src/` vs `tests/tool_reexec.rs`）留 D6 小修；不阻塞 |
| 范围/门禁 | ✅ docs + 评审记录；check.sh PASSED；worktree 干净 |

**最终状态**：C1/C2 closed；建议 Lead merge → F2/F3。
