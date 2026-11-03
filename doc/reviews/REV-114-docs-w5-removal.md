# REV-114 · docs · 取消 W5「共享评审队列」（DR-015 + 引用状态注记）

> **评审范围**：docs-only（11 文件）；无代码/测试/既有评审记录/spec 冻结正文改动；未触碰 `dist/`、无 token/密钥。评审人只新增本评审记录，未改实现/文档。

- **分支**：`docs/docs-w5-removal`
- 分支: docs/docs-w5-removal
- **作者**：`docs-dev`（提交 `5d68cc0`；正文 `Review: REV-114` / `Agent: docs-dev`）
- **评审人**：`reviewer-a`（独立评审，≠ 作者）
- **基于**：main `be87ec2`（v0.4.0 已发布；`main = origin/main = be87ec2`）
- **变更范围**：`be87ec2..5d68cc0`（**11 files, +113/−25**）：新增 `doc/decisions/DR-015-v05-roadmap-and-w5-removal.md`；`README.md`、`doc/progress.md`、`doc/spec-v1.2/00-总纲.md`（仅 v0.4 banner 状态注记）、`doc/decisions/DR-004/007/008/012`、`doc/design/software-team-sop-v1.md`、`doc/index.md`、`doc/issues.md`
- **评审时间**：2026-11-03
- **关联**：task-93/94；用户 2026-11-03 裁决；DR-004/007/008/012；DR-015；ISSUE-155

## 1. 结论

**结论：通过**

> **结论历史**：首轮（基准 `5d68cc0`）为「**有条件通过**」——P2 C1：`doc/index.md` DR-015 行与 architecture 行粘连、DR-008/DR-012 入口行未标取消。作者 `a632806` 关闭 C1 + P3 后，评审人复核 index 三处/check.sh 绿（§8），C1 closed，结论改**通过**。
>
> 首轮意见（历史保留）：P0=0/P1=0；DR-015 内容/引用状态注记/历史保留主体成立；C1 index 入口；P3 历史行/状态措辞。

### 1.1 已核对成立

| 项 | 结论 |
|---|---|
| DR-015 内容 | ✅ 用户 2026-11-03 裁决「取消，不是延期」；W5 定义（节点邮箱/多激活/按 item 路由，内核级）；历史延期链 v0.2→v0.3→v0.4→v0.5；不做理由（per-item+foreach+human_decisions 能力覆盖、内核/回放/兼容成本、需求未验证、机会成本）；替代方案（per-item/foreach/human_decisions/未来轻量节点并发上限，不做邮箱/多激活）；W5 状态 **removed from roadmap**；v0.5 候选（reexec/真实 Codex·DSH/ISSUE 清理/sandbox Linux·构建工具/可观测性）**不承诺**；历史保留说明 |
| 引用状态注记 | ✅ README 已知限制/下一步去掉 W5、指向 DR-015；progress v0.5 候选不含 W5；spec 00 v0.4 banner「W5 已取消」；DR-007 §1.3/§9、DR-008 §0/§1.2/§7/§8 #1、DR-012 §0/§1.2/§8 #8、DR-004 §2/§3/W5 标题/§10 v0.4 行、SOP 版本约定/能力表/§9 均追加取消注记；ISSUE-155 仅追加「W5 后于 2026-11-03 用户取消（DR-015）」，未新增缺陷 issue |
| 历史保留 | ✅ 原延期表述不改写，只追加状态注记；评审记录/历史结论不删除；DR-015 历史表保留 |
| 范围/约束 | ✅ docs-only 11 文件；无 `crates/`/`tests/`/`doc/reviews/` 改动；spec 只改 00 v0.4 banner（允许的状态注记）；无 token/密钥；未触碰 `dist/`；`check.sh` **PASSED** |
| 表述一致性 | ⚠️ C1：`doc/index.md` 入口行仍有「W5 延至 v0.4 / W5→v0.5」未标取消，且 DR-015 行与 architecture 行粘连；其余正文/入口（README/progress/spec00/DR-* /SOP）已标注取消 |

## 2. 四维评审

| 维度 | 结论 | 依据 |
|---|---|---|
| 功能正确性 | ⚠️ 有条件 | DR-015 与主要引用正确；C1 index 入口与 DR-015 矛盾/表格损坏；P3 历史行可选注记 |
| 代码结构合理性 | ✅ | docs-only；DR-015 独立、引用分层；未改代码/测试/spec 正文；index/issues 同步（除 C1） |
| 逻辑性 | ✅ | 取消 vs 延期口径清楚；替代方案/历史保留/v0.5 不承诺自洽；无新缺陷 issue；W5 均指向 DR-015（正文），入口 index 待修 |
| 可读性 | ✅（C1/P3） | DR-015 摘要/历史表/理由/替代方案清晰；C1 为 index 表格格式/入口口径，P3 为历史行注记措辞 |

## 3. 发现清单

### 3.1 P2 条件（D6/本分支内关闭）

| # | 位置 | 问题 | 建议 |
|---|---|---|---|
| **C1** | `doc/index.md:21`（DR-015 行与 `architecture-v2.md` 行粘连：`...（REV-114） || [architecture...`）；`doc/index.md:13`（DR-008 行「W5 延至 v0.4（REV-096）」）；`doc/index.md:17`（DR-012 行「W5→v0.5（REV-105）」） | **入口索引与 DR-015 矛盾/格式损坏**：DR-015 行未拆行，Markdown 表格断裂；DR-008/DR-012 入口摘要仍把 W5 描述为未来计划（未标已取消），与「取消，不是延期」及任务「入口处更新」冲突 | 拆开 DR-015 与 architecture 行；DR-008/DR-012 行追加「**W5 已取消（用户 2026-11-03，DR-015）**」（或改指向 DR-015）；保持历史正文不改写 |

### 3.2 P3（建议，不阻塞）

- **P3-1 DR-004 历史行可选注记**：`DR-004:221`（§5 待确认「W5 放 v0.3」）、`DR-004:283`（v0.3 发布记录「下一步 v0.4 W5 + ISSUE 清理」）仍为历史计划表述；正文已在 §2/§3/§10 v0.4 行注记取消，任务允许历史保留，建议在这两处追加「后于 2026-11-03 取消（DR-015）」以免读者只看历史行误解（不改写结论）。
- **P3-2 DR-015 状态措辞**：`DR-015` 首行状态「已裁决（用户 2026-11-03；REV-114 评审）」；评审尚未结束，发布后可改为「通过（REV-114）」或保留「已裁决」；不阻塞。
- **P3-3 DR-015 历史表未列 DR-007 §9**：历史延期链表列 DR-004/DR-008/DR-012；DR-007 §9 已单独追加注记，可在表内补一行或不影响。
- **P3-4 `doc/index.md` reviews 至 114**：range 已更新；reviews README REV-114 行由评审人本次补；index 中 reviews 细节范围可随 D6 同步。
- **P3-5 ISSUE-155**：仅更新状态注记、无新增 issue 正确；建议保持 open/fixed 历史字段不变。

## 4. 验证证据

评审人亲自执行（worktree `.worktrees/docs-docs-dev`，HEAD `5d68cc0`，main `be87ec2`，worktree 干净）：

```bash
git status --porcelain                     # 空
git diff --stat main...HEAD                # 11 files +113/−25（docs-only）
git diff --name-only main...HEAD           # 仅 README/doc/**；无 crates/tests/doc/reviews
bash scripts/dev/check.sh                  # CHECK PASSED
grep -rn "W5" ...                          # 正文/README/progress/spec00/DR-007/008/012/SOP 均有取消注记；
                                           # index DR-008/DR-012 入口行未标取消（C1）
```

- **DR-015 逐节核对**：用户裁决/日期、W5 定义、历史延期链、5 条不做理由、替代方案、removed 状态、v0.5 候选不承诺、历史保留均成立；与 DR-004/007/008/012/SOP 正文一致。
- **范围/红线**：无代码/测试/既有评审/spec 冻结正文改动；spec 仅 00 v0.4 banner 状态注记；无 token/密钥；`dist/` 未触碰；`check.sh` PASSED。
- **ISSUE-155**：只追加取消注记，未新增 W5 缺陷 issue；符合「需求取消不是缺陷」口径。

## 5. 条件关闭方式

- **C1 由 docs-dev 在本分支 docs 小 commit 关闭**：修 `doc/index.md` DR-015 行拆分 + DR-008/DR-012 行追加「W5 已取消（DR-015）」；重跑 `check.sh`。
- 关闭后评审人复核 `doc/index.md` 表格渲染/`grep W5` 入口注记 + check.sh，追加「条件关闭」段、结论改「通过」；随后 Lead merge；GitHub Release body 的 W5 句由 Lead 可选同步修正。

## 6. 条件关闭记录（"有条件通过"）

| 条件 | 状态 | 关闭要求 |
|---|---|---|
| C1 index 入口 DR-015 行拆分 + DR-008/DR-012 取消注记 | **closed（a632806）** | `doc/index.md:13/:17` 追加「W5 已取消（DR-015）」；`:20–21` DR-015 与 architecture 行拆开、表格恢复；check.sh PASSED |
| P3 ①–③ | **closed（a632806）** | DR-004 §5:221/§10 v0.3 行追加取消注记；DR-015 状态改「已裁决…REV-114 通过」；历史表补 DR-007 §9 行 |

## 7. 遗留问题

- C1 与 P3 ①–③ 已关闭；P3-4 reviews README REV-114 已由本次评审补；P3-5 ISSUE-155 只追加注记正确。
- 无代码/测试影响；W5 从未实现，DR-015 仅为路线图/文档；无需 workspace 测试（docs-only）。
- 历史保留：DR-004/007/008/012/SOP 既有延期表述未改写；取消注记只追加。

## 8. 条件关闭复核（2026-11-03）

复核提交：`a632806`（父 `53bf940`；docs-only 3 文件 `doc/index.md`/`DR-004`/`DR-015`）。评审人执行：

```bash
git status --porcelain                       # 空
git diff --stat 53bf940..a632806             # 3 files +8/−7（docs-only）
bash scripts/dev/check.sh                    # CHECK PASSED
grep -n "W5" doc/index.md                    # :13/:17 均有「W5 已取消（DR-015）」；:20 DR-015 行独立
git diff 53bf940..a632806 -- doc/index.md    # DR-015 与 architecture 拆行/表格恢复
```

| 项 | 复核结果 |
|---|---|
| C1 index 三处 | ✅ DR-008/DR-012 入口行追加取消注记；DR-015 行与 architecture 行拆开、原粘连 `\|\|` 消失、空行移除；表格渲染恢复 |
| P3 ①–③ | ✅ DR-004 §5:221 与 §10 v0.3 行追加取消注记；DR-015 状态「REV-114 通过」；历史表补 DR-007 §9 |
| 范围/门禁 | ✅ 仅 3 个 docs 文件；无代码/测试/既有评审/spec 冻结正文；无 token/dist；check.sh PASSED |

**最终状态**：C1/P3 closed；建议 Lead merge（可选同步 GitHub Release body/notes 的 W5 句）。
