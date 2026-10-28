# REV-003 · process · merge.sh 分支删除修复

> 编号：初评按 lead 指示由 REV-002 改为 **REV-003**（既有 `REV-002-process-merge-gate-fix.md` 保留不动）。初评结论"不通过"（`3c399f9`）；lead 提交 `371ef33` 后复审通过。

- **分支**：`feat/process-fix-merge-branch-delete`
- **作者**：`lead`（提交 `fda4cc8`；复审补丁 `371ef33`，`Agent: lead`）
- **评审人**：`docs-reviewer`（独立评审，非作者）
- **变更范围**：`552466e..371ef33`（初评 `fda4cc8`：3 files, +16/−8；复审 `371ef33`：7 files, +87/−7）：`scripts/dev/merge.sh`、`scripts/dev/selftest-merge.sh`、`doc/development-workflow.md`、`doc/issues.md`、`doc/progress.md`、`doc/reviews/README.md`、`doc/index.md`
- **评审时间**：2026-10-29（初评）；2026-10-29（复审）
- **关联**：ISSUE-023、ISSUE-024；既有 `REV-002-process-merge-gate-fix.md`；AGENTS §1/§3/§4/§6/§7；`doc/development-workflow.md` §8

## 1. 结论

**结论：通过**

复核 `371ef33` 后，初评的三项阻塞（编号冲突、自举两阶段、验证方法）均已关闭：

1. **编号冲突**：本记录已为 REV-003；`371ef33` 同步 ISSUE-023、`doc/progress.md`、`doc/reviews/README.md` 与 `doc/index.md`；本复审再修正索引中 REV-003 的评审人/结论。
2. **自举两阶段**已正式落文档：`doc/development-workflow.md:114-122` 新增 §8，ISSUE-023 改为 `in-progress` 并写明"首次旧脚本停在删分支、重跑新脚本幂等完成"，`doc/progress.md:21` 同步提示。我按此前 /tmp 实验复核：旧门禁第一次确实在旧 `git branch -d` 处 exit 1（main 已合并、分支保留），第二次新门禁 exit 0 并删除分支；文档描述与实测一致。
3. **验证方法缺陷**已修正：新增 `scripts/dev/selftest-merge.sh`，在临时 clone 的 **main 上先提交待测 merge.sh**，从本地 main 建分支并 `--set-upstream-to=origin/main`（复现 upstream 落后），断言 dry-run 非破坏、真实合并清理分支/worktree。我实测通过：`SELFTEST PASSED`（`PYTHON_BIN=/tmp/gf_venv/bin/python`）。

另：push 失败已改为 `|| die ...`（实测 exit 1 + `MERGE BLOCKED`，分支保留）；worktree 删除不再吞错；ISSUE-023/进度/索引已同步 REV-003（索引中 REV-003 的评审人/结论由本记录修正）。剩余仅 #8 的建议级自测覆盖问题，不影响合并。

## 2. 四维评审

| 维度 | 结论 | 依据（文件/行/测试） |
|---|---|---|
| 功能正确性 | ✅ | 目标缺陷（upstream 未更新导致 `git branch -d` 误报）已修复：`scripts/dev/merge.sh:123-128` ancestor 校验 + `branch -D`；修正条件实测真实合并（含 upstream=origin/main 的作者 worktree）exit 0、分支/worktree 均清理；dry-run 不清理；push 跳过/成功/失败路径均符合预期；`selftest-merge.sh` 通过（#4.3）。 |
| 代码结构合理性 | ✅ | 改动集中在 `merge.sh` step 8–10 与注释；自测逻辑独立在 `scripts/dev/selftest-merge.sh`（临时 clone，不污染仓库）；台账/流程文档同步。REV-002 保留、REV-003 编号与索引已理清（#1/#6）。 |
| 逻辑性 | ✅ | 顺序 `worktree 清理 → push → ancestor 校验 → branch -D` 自洽；push 失败 `\|\| die` 保留分支、worktree 删除失败即报；自举两阶段在 workflow §8/ISSUE-023 写死，和实测一致（#2/#4/#5）。 |
| 可读性 | ✅ | 注释解释"为什么不能用 `git branch -d`"，`die`/`info` 信息清楚；workflow §8 与 ISSUE-023 可直接照做。仅 #8 建议级：selftest 未覆盖作者 worktree 路径、日志固定 `/tmp/selftest-*.log`、`MAIN_WT` 取首个 worktree、文档命令未写 `PYTHON_BIN`。 |

## 3. 发现清单

| # | 级别 | 位置 | 问题 | 建议 | 状态 |
|---|---|---|---|---|---|
| 1 | 阻塞 | `doc/reviews/REV-002-process-merge-gate-fix.md`；`doc/issues.md:30`；`doc/progress.md:21`；`doc/reviews/README.md:6-9` | 初评：REV-002 已被既有记录占用，REV-002 再建会重复；引用侧未同步 | 371ef33 已把本记录改 REV-003、同步 ISSUE-023/progress，补齐 REV-002/REV-003 索引；本复审再修正索引中 REV-003 的评审人/结论 | fixed（371ef33 + 本次） |
| 2 | 阻塞 | `doc/development-workflow.md:114-122`；`doc/issues.md:30`；`doc/progress.md:21` | 初评：自举两阶段未写文档，用户会误判首次合并失败 | §8 已写死"旧脚本首次停在删分支、main 已更新、分支保留 → 在新 main 重跑即幂等"；ISSUE-023 `in-progress`；我实测复现与文档一致 | fixed（371ef33） |
| 3 | 重要 | `scripts/dev/selftest-merge.sh`；`doc/development-workflow.md:122` | 初评：给定冒烟流程在 `checkout main` 时回退 merge.sh，且分支无 upstream，测不到修复 | 新增 selftest：clone 主工作区、把待测 merge.sh 提交到 clone main、本地 main 建分支并设 upstream=origin/main；实测 `SELFTEST PASSED` | fixed（371ef33） |
| 4 | 重要 | `scripts/dev/merge.sh:115-121` | 初评：push 失败裸退 128，无 `MERGE BLOCKED` | 已改为 `git push origin main \|\| die "推送失败（main 已合并、分支保留…）"`；实测 exit 1、`MERGE BLOCKED`、分支保留 | fixed（371ef33） |
| 5 | 建议 | `scripts/dev/merge.sh:108-113` | 初评：`git worktree remove ... \|\| true` 吞错却打印"已删除" | 已移除 `\|\| true`，失败即 `die` | fixed（371ef33） |
| 6 | 建议 | `doc/reviews/README.md:6-9`；`doc/index.md:16` | 初评：索引未收录既有 REV-002/新 REV-003 | 371ef33 已补两行；本复审修正 REV-003 行为"通过"、评审人 `docs-reviewer`，并清理多余空行 | fixed（本次） |
| 7 | 建议 | `doc/progress.md:8`；`doc/issues.md:30` | 初评：两条 process 行；ISSUE-023 评审前标 fixed | progress 已合并 process 行；ISSUE-023 改 `in-progress`，待合并后再 `fixed`；下一步注明自举 | fixed（371ef33） |
| 8 | 建议 | `scripts/dev/selftest-merge.sh:17-18,21,54,61`；`doc/development-workflow.md:122` | 复审补充：selftest 未创建"作者 worktree"场景（只测临时 detached worktree）；日志固定写 `/tmp/selftest-*.log`；`MAIN_WT` 依赖首条 worktree 是主工作区；文档命令未示例 `PYTHON_BIN` | 建议后续给 selftest 增加 `git worktree add` 的用例、日志用 `mktemp`、按 branch 识别主 worktree，并在 workflow §8 补 `PYTHON_BIN=/tmp/gf_venv/bin/python` 示例（非阻塞） | open（建议级） |

## 4. 验证证据

### 4.1 静态与仓库自检

```bash
cd .worktrees/process-agent-lead
git diff --check 3c399f9..371ef33      # exit 0
bash -n scripts/dev/merge.sh           # exit 0
bash -n scripts/dev/selftest-merge.sh  # exit 0
bash scripts/dev/check.sh              # CHECK PASSED（progress 21 非空行）
/tmp/gf_venv/bin/python -m pytest -q --tb=no  # 25 failed / 162 passed；基线 25，无新增
```

### 4.2 `371ef33` 变更内容

`git diff --stat 3c399f9..371ef33`：7 files changed, 87 insertions(+), 7 deletions(-)。核心：
- `doc/development-workflow.md` §8：门禁脚本自举两阶段；merge.sh 改动必须附 selftest。
- `doc/issues.md` ISSUE-023：改为 `in-progress`，写明首次/重跑语义；REV-003。
- `doc/progress.md`：合并 process 行、下一步提示自举。
- `doc/reviews/README.md`/`doc/index.md`：补 REV-002/REV-003。
- `scripts/dev/merge.sh`：worktree 删除失败即 `die`；push 失败 `|| die`。
- `scripts/dev/selftest-merge.sh`：新增自测。

### 4.3 新增自测（本机实测）

```bash
PYTHON_BIN=/tmp/gf_venv/bin/python bash scripts/dev/selftest-merge.sh
# == clone /Users/zyzs/lczyzs/code/zgraph to /var/folders/.../repo ==
# SELFTEST PASSED
```

脚本核对：`SRC` 为当前 worktree；`MAIN_WT` 取 `git worktree list` 首行主工作区；clone 后把 `SRC/scripts/dev/merge.sh` 提交到 **clone 的 main**（不是测完 checkout main 回退）；从本地 main 建 `test/process-merge-selftest` 并 `--set-upstream-to=origin/main`；断言 dry-run 分支/worktree 不被清理、真实合并 exit 0 且分支/worktree 删除、main 含合并提交。`/tmp` 临时目录由 trap 清理。

### 4.4 修正条件的实测（承接初评）

1. **作者 worktree + upstream=origin/main**：新脚本在临时 main，分支 `git worktree add`，dry-run exit 0 且分支/worktree 保留；真实合并 exit 0，分支/worktree 删除，输出顺序"已删除 worktree → 未推送 → 已删除分支"。
2. **无 worktree / 临时检查目录**：dry-run exit 0、分支保留、临时目录 trap 清理；真实合并 exit 0、分支删除。
3. **push 跳过**：exit 0、分支删除。
4. **push 失败（当前 371ef33 脚本）**：`remote.origin.pushurl` 指向不存在目录，exit=1，输出 `MERGE BLOCKED: 推送失败（main 已合并、分支保留…）`；main 已合并、分支保留、worktree 已移除。
5. **push 成功（bare 远端）**：exit 0，分支删除，bare 远端 main 与本地一致。
6. **自举复现**：临时 clone 用旧 main 的 merge.sh 合并含新脚本、upstream=origin/main 的分支 → 第一 run exit=1（`not yet merged to origin/main` / `MERGE BLOCKED: 分支删除失败`，main 已更新、分支保留）→ 第二 run 新脚本 exit=0、分支删除。与 workflow §8 一致。

> 以上均在 `/tmp` 副本执行；仓库工作区未被修改。

## 5. 遗留问题

- **无阻塞项**；#8 为建议级（selftest 作者 worktree 覆盖、日志/主工作区识别、PYTHON_BIN 文档），不阻塞合并，建议 M0 前顺手补齐。
- 合并本分支时按 `doc/development-workflow.md` §8 预期两阶段：第一次旧脚本停在删分支（main 已合并、分支保留）→ 重跑新脚本完成；ISSUE-023 合并后改 `fixed`。
- 本次复审仅修改 `doc/reviews/REV-003-process-merge-branch-delete.md` 与 `doc/reviews/README.md`；未 push、未 merge。

## 6. 条件关闭记录

| 原阻塞项 | 关闭证据 | 关闭日期 |
|---|---|---|
| #1 编号冲突 | 371ef33 把 REV-002→REV-003 同步到 ISSUE-023/progress/reviews 索引；本复审修正索引 REV-003 行（评审人/结论） | 2026-10-29 |
| #2 自举两阶段 | `doc/development-workflow.md` §8；ISSUE-023 `in-progress` 含首次失败/重跑语义；/tmp 实测第一次 exit=1、第二次 exit=0 | 2026-10-29 |
| #3 验证方法 | `scripts/dev/selftest-merge.sh`；`SELFTEST PASSED`；其条件（clone main + 待测脚本提交到 clone main + upstream 落后 + dry-run/真实合并断言）与初评要求一致 | 2026-10-29 |

> 初评"不通过"的 3 项阻塞已于 2026-10-29 关闭，结论转为"通过"。

## 7. 复核与转通过记录

### 7.1 初评（2026-10-29，`3c399f9`）

- 结论"不通过"：REV-002 编号冲突、自举两阶段未文档化、给定冒烟流程无法测到新脚本。
- 另建议 push 失败 `die`、worktree 删除错误传播、索引/台账同步。

### 7.2 复审（2026-10-29，`371ef33`）——转为通过

- 复核范围 `3c399f9..371ef33`（7 files, +87/−7）；初评结论保留，本段记录关闭与残留。
- 逐项确认：REV-003 引用同步；workflow §8/ISSUE-023/progress 自举说明；selftest-merge.sh 实测通过；push/worktree 建议已采纳；progress/索引已整理。
- 剩余仅 #8 建议级；四维无阻塞项，merge.sh 修复行为在修正条件下全部验证通过。
- **结论：通过**。合并时按 workflow §8 预计两阶段（首次旧脚本停删分支、重跑新脚本完成）；本记录与 reviews 索引随复审提交，不 push/merge。
