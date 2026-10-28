# REV-003 · process · merge.sh 分支删除修复

> **前置说明**：复核发现 **REV-002 已被既有记录占用**（`doc/reviews/REV-002-process-merge-gate-fix.md`，`main` 上 50f05d2 引入且未入索引）；按 lead 指示本记录改用 **REV-003**。`doc/issues.md:30` / `doc/progress.md:21` 仍写 REV-002，需由 lead 同步；`doc/reviews/README.md` 未收录既有 REV-002 与本记录（见 #1/#6/#7）。

- **分支**：`feat/process-fix-merge-branch-delete`
- **作者**：`lead`（提交 `fda4cc8`，`Agent: lead`）
- **评审人**：`process-reviewer`（独立评审，非作者）
- **变更范围**：`552466e..fda4cc8`（3 files changed, 16 insertions(+), 8 deletions(-)）：`scripts/dev/merge.sh`、`doc/issues.md`、`doc/progress.md`
- **评审时间**：2026-10-29
- **关联**：ISSUE-023、ISSUE-024；既有 `REV-002-process-merge-gate-fix.md`；AGENTS §1/§3/§4/§6/§7

## 1. 结论

**结论：不通过**

`merge.sh` 的修复逻辑本身在修正后的实测中**行为正确**（旧 `git branch -d` 误报已消除，dry-run/真实合并/无 worktree/push 跳过/push 成功/push 失败路径均已验证）。按 lead 指示编号已改为 REV-003，解决了原 #1 的编号冲突；但本次交付仍存在 2 个必须先解决的阻塞项（#2/#3）：

1. **编号冲突已按 lead 指示处理**：本记录改用 REV-003；`doc/issues.md:30`/`doc/progress.md:21` 仍写 REV-002，需同步（#1/#6/#7）。
2. **修复不能一次自举合并**：本分支修的是 `merge.sh`，但合并它时运行的仍是 `main` 上的旧脚本；实测旧门禁会先 ff 合并、随后在旧 `git branch -d` 处失败退出（正是 ISSUE-023 的现象），需第二次运行新脚本才成功。流程未登记这个两阶段/自举步骤（#2）。
3. **任务给定的冒烟流程实际测不到新脚本**：按给定命令，`git checkout main` 会把 `merge.sh` 回退为旧版，dry-run/真实合并执行的是旧逻辑；且分支由本地 `main` 创建、没有 upstream，无法复现原缺陷（#3）。作者自检证据因此不成立，须修正验证方法。

上述 #2/#3 关闭后再评审；代码修复方向本身正确，编号冲突已由 REV-003 解决。

## 2. 四维评审

| 维度 | 结论 | 依据（文件/行/测试） |
|---|---|---|
| 功能正确性 | ⚠️ | 目标缺陷（upstream 未更新导致 `git branch -d` 误报）已修复：`scripts/dev/merge.sh:123-128` 改为 ancestor 校验 + `branch -D`；我在修正后的 /tmp 实测中验证真实合并（含 upstream=origin/main 的分支）exit 0 且分支/worktree 均清理；dry-run 不清理；push 跳过/成功/失败行为符合预期。**但**自举合并一次不成功（#2），给定冒烟流程无法验证新脚本（#3）。 |
| 代码结构合理性 | ⚠️ | 改动局限在 `merge.sh` 的 step 8–10 与注释，职责单一、无重复实现；`doc/issues.md`/`progress.md` 为流程台账更新。**但**评审记录编号与本模块既有 REV-002 冲突，`doc/reviews/README.md` 也从未收录既有 REV-002，新记录同样不会入索引（#1/#6）。 |
| 逻辑性 | ⚠️ | 新顺序 `worktree 清理 → push → ancestor 校验 → branch -D` 在正常路径自洽；ancestor 校验前置于 `-D`，避免强删未合并分支。**但** push 失败时 `set -e` 直接以 128 退出、无 `MERGE BLOCKED` 说明（#4）；worktree 删除失败被 `\|\| true` 吞掉却仍打印"已删除"（#5）；自举合并两阶段未写入流程（#2）。 |
| 可读性 | ✅ | 注释解释"为什么不能用 `git branch -d`"，`info`/`die` 信息清楚；变量命名与现有脚本一致。仅 `doc/progress.md:10-11` 出现两条 `process` 行、ISSUE-023 在评审前标 `fixed` 等台账文字问题（#7）。 |

## 3. 发现清单

| # | 级别 | 位置 | 问题 | 建议 | 状态 |
|---|---|---|---|---|---|
| 1 | 阻塞 | `doc/reviews/REV-002-process-merge-gate-fix.md`；`doc/reviews/REV-003-process-merge-branch-delete.md`；`doc/issues.md:30`；`doc/progress.md:21`；`doc/reviews/README.md:5-8` | **评审编号冲突（已改号）**：REV-002 已被 `REV-002-process-merge-gate-fix.md` 占用（main 的 50f05d2 引入），且该既有记录未进 `doc/reviews/README.md`；按 lead 指示本记录改用 REV-003，但 ISSUE-023/进度/索引仍写 REV-002。 | 已完成本记录改号 REV-003；lead 需把 ISSUE-023、`doc/progress.md:21` 同步改 REV-003，并在 `doc/reviews/README.md` 补录既有 REV-002 与新 REV-003 两行。 | fixed（本次改号，引用侧见 #6/#7） |
| 2 | 阻塞 | `scripts/dev/merge.sh:104-128`；`doc/progress.md:21`；`doc/issues.md:30` | **自举合并一次不成功**：本分支要改的正是合并门禁，而执行合并的是 main(552466e) 上的旧 merge.sh。实测旧门禁完成 ff 合并后仍在旧 `git branch -d` 处 `MERGE BLOCKED: 分支删除失败` 退出（分支保留、worktree 已删、main 已更新含新脚本）；必须再跑一次新脚本才成功。流程/ISSUE/进度未说明这个两阶段步骤，用户会看到首次合并失败。 | 在 ISSUE-023 或 REV 中写明"本分支合并需两次 merge.sh（第一次预期在旧删除步骤失败、main 更新后第二次成功）"，或经用户同意采用一次性 bootstrap；把该操作纳入合并检查单。 | open |
| 3 | 重要 | 任务指定冒烟命令（`cp merge.sh` → `checkout -b` → commit → `checkout main` → merge.sh）；`scripts/dev/worktree.sh:60-72`；本分支实际 upstream | **冒烟流程不覆盖新脚本**：`cp` 后新脚本只在测试分支提交，`git checkout main` 会把 `scripts/dev/merge.sh` 回退为旧版，随后 dry-run/真实合并执行旧逻辑；且本地 `main` 创建的分支没有 upstream（本分支实际 `@{upstream}=origin/main`），旧 `git branch -d` 反而不会报错，无法复现 ISSUE-023。 | 修正验证步骤：在临时 clone 的 **main** 上先提交新 `merge.sh`（或从含新脚本的临时 main 运行），再用 `git checkout -b <branch> origin/main` 建分支以设置 upstream；测试内容与本报告 §4 一致。 | open |
| 4 | 重要 | `scripts/dev/merge.sh:115-121` | push 失败时 `git push origin main` 在 `set -e` 下直接退出（实测 exit 128），没有 `MERGE BLOCKED` 提示；此时 worktree 已删除、分支保留，用户只看到 git 原始 fatal。 | 改为 `git push origin main \|\| die "推送失败（分支已保留，修复后可重跑 merge.sh）"`，并说明重跑语义。 | open |
| 5 | 建议 | `scripts/dev/merge.sh:108-113` | `git worktree remove --force ... \|\| true` 吞掉失败，随后无条件打印"已删除 worktree"；若实际未删，后续 `git branch -D` 会以"分支删除失败"报错，误导定位。 | 不吞错误或失败时 `die`；至少区分"尝试删除"和"已删除"。 | open |
| 6 | 建议 | `doc/reviews/README.md:5-8`；`doc/index.md:16`；`doc/issues.md:30` | 评审索引未收录既有 REV-002，本次新记录（或改号后的 REV-003）也不会入索引；`doc/index.md` 的 reviews 行只写 REV-000/001。 | 随评审记录提交更新 reviews 索引与 doc/index 行；既有 REV-002 一并补录。 | open |
| 7 | 建议 | `doc/progress.md:10-11,21`；`doc/issues.md:30-31` | `progress` 出现两条 `process` 行（review 与 next），ISSUE-023 在独立评审前已标 `fixed` 且写"REV-002 覆盖"；ISSUE-024 是 docs 问题却由 process 分支登记（台账可接受，但应注明来源 REV-001 #22）。 | ISSUE-023 评审通过前保持 `in-progress`，通过后再 `fixed`；合并 process 两行；ISSUE-024 注明来自 REV-001 #22。 | open |

## 4. 验证证据

### 4.1 静态与仓库自检

```bash
cd .worktrees/process-agent-lead
bash -n scripts/dev/merge.sh                 # exit 0
git diff --check 552466e..HEAD               # exit 0
bash scripts/dev/check.sh                    # CHECK PASSED
/tmp/gf_venv/bin/python -m pytest -q --tb=no # 25 failed / 162 passed；基线 25，无新增
```

- 测试结果：`25 failed / 162 passed`，与 `doc/test-baseline.txt` 的 25 项完全一致，无新增失败。
- 环境：默认 `python3` 无 pytest/uv，使用 `/tmp/gf_venv/bin/python`（ISSUE-003）。

### 4.2 按任务原样命令的实测（证明其无效）

在 `/tmp` clone 中按给定命令执行后，`git checkout main` 将 `merge.sh` 回退为旧版；dry-run/真实合并实际调用旧逻辑。第一次真实合并输出（节选，旧脚本顺序）：

```text
INFO: 已合并 test/process-merge-smoke -> main
INFO: 已删除分支: test/process-merge-smoke
INFO: 未推送（加 --push 推送 origin/main）
real_exit=0
```

旧脚本是"先删分支、后提示未推送"，说明执行的不是新脚本；此外该 smoke 分支由本地 `main` 创建、无 upstream，旧 `git branch -d` 不会报 ISSUE-023，故该冒烟无法证明修复。

### 4.3 修正后的实测（/tmp，仓库未改动）

1. **新脚本 + upstream=origin/main + 作者 worktree**：克隆后在临时 main 提交新 `merge.sh`，`git checkout -b test/process-merge-smoke` 后 `git branch --set-upstream-to=origin/main` 并 `git worktree add`：
   - dry-run：exit 0，分支与 worktree 均保留（`git worktree list` 仍有 smoke-wt）。
   - 真实合并（无 `--push`）：exit 0；`git branch --list` 为空；`git worktree list` 只剩 main；输出顺序为"已删除 worktree → 未推送 → 已删除分支"。
2. **无 worktree / 临时检查目录路径**：dry-run exit 0，分支保留、临时目录由 trap 移除；真实合并 exit 0，临时目录不残留、分支删除。
3. **`--push` 被跳过**：exit 0，分支删除，main 本地前进。
4. **`--push` 失败**：将 `remote.origin.pushurl` 指向不存在目录，真实合并 ff 成功后 push 失败，exit=128；main 已合并、分支保留（`test/process-merge-smoke3`）、worktree 已移除。行为可恢复但报错不友好（#4）。
5. **`--push` 成功**：把 pushurl 指向 `/tmp/zgraph-bare.git`，exit 0；bare 远端 main 与本地一致，分支删除。
6. **自举复现**：在临时 clone 用旧 main 的 merge.sh 合并一个 upstream=origin/main、且提交里含新 merge.sh 的分支：
   - 第一次：ff 合并成功 → `warning: not deleting branch ... not yet merged to 'refs/remotes/origin/main'` → `MERGE BLOCKED: 分支删除失败（检查是否已完全合并）`，exit=1；分支保留，worktree 已删，main 已是含新脚本的提交。
   - 第二次（新脚本）：exit=0，分支删除，确认两阶段可恢复（#2）。

> 以上全部在 `/tmp/zgraph-merge-test*` 副本执行；仓库与作者 worktree 未被修改。

## 5. 遗留问题

- **阻塞 #2/#3 未关闭（#1 已通过改号 REV-003 关闭），结论不通过**；请写清自举两阶段、修正验证流程，并由 lead 同步 issues/progress/index 的 REV-003 引用，再提交复审。
- 建议同时修 #4（push 失败 `die` 消息）、#5（worktree 删除错误传播）、#6（索引）、#7（台账状态/重复行）。
- 未 push、未 merge、未改作者其他文件；本次仅新增本评审记录 REV-003（既有 REV-002 保留不动；引用侧同步见 #6/#7）。

## 6. 条件关闭记录

不适用（结论为"不通过"）。
