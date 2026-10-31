# REV-066 · docs · W1 收口（DR-004 §10 + 07 AC-13）

- **分支**：`docs/docs-sop-v1-ac`
- **作者**：`lead`（提交 `393b2b6` DR-004 §10 + 07 AC-13；`957ed19` PROVENANCE 更新；均含 `Review: REV-066` / `Agent: lead`）
- **评审人**：`reviewer-sop`（**≠ 作者**；独立评审）
- **变更范围**：`263b3a8..957ed19`，3 文件 **+33/−2**（全部 `docs`）：`doc/decisions/DR-004-v02-scope-and-waves.md`、`doc/spec-v1.2/07-验收用例.md`、`doc/spec-v1.2/PROVENANCE.md`；未改产品/测试/fixture/CI
- **评审时间**：2026-10-31
- **关联**：DR-004 §10（`doc/decisions/DR-004-v02-scope-and-waves.md`）；SOP 设计稿 v1 §11；REV-064（py-worker `715f13a`）、REV-065（e2e `263b3a8`）；PROVENANCE §2 第 6 条修订规则

## 1. 结论

**结论：有条件通过**

内容事实与 W1 实际交付基本一致：DR-004 §10 的 A1–A7 摘要、REV-064/REV-065 合并提交、full/smoke/exhaust hash、7 passed/1 ignored、46 passed/2 ignored、pytest 104、check.sh 证据均可核验；07 AC-13 与 e2e 断言一致，未夸大 full 只 validate / real-LLM `#[ignore]` / 不把 gate=false 当失败；PROVENANCE §3 的 07 SHA-256 `a68bf34d…`、行数 253、00–08 合计 6050 与实测一致。

但存在 3 项**重要**文档一致性问题（均为可机械修复的索引/编号/同步问题，非产品语义问题）：

1. **PROVENANCE §2 新条目编号重复**：新增“W1 SOP v1 回写批次”编号为 **11**，与既有 `11. REV-035 评审条件修正批次` 重号；应为 **14**，且 §3 合计行“见 §2 第 11 条”会指错条目。
2. **07 AC-13 未纳入索引/DoD**：07 顶部“通过标准：AC-01..AC-12”、§7.2 用例总表、§7.3 #3 检查单均未加入 AC-13；新增 AC 对验收不可见。
3. **spec README 索引过期**：`doc/spec-v1.2/README.md` 仍写 07 行数 241、AC-01..AC-12（实际 253、含 AC-13）；与 PROVENANCE §3/07 不一致。

另有 3 条建议（DR-004 §10 自指措辞、AC-13 “32 个符号”措辞、PROVENANCE 多余空行），不阻塞但可顺手修。关闭 C1–C3 后复审改判。

## 2. 四维评审

| 维度 | 结论 | 依据（文件/行/测试） |
|---|---|---|
| 功能正确性 | ⚠️（C2/C3） | DR-004 §10 与 A1–A7、REV-064/065、hash/测试数字逐项核对一致；07 AC-13 与 e2e 实际断言一致（full validate 60/100、smoke 11/18、exhaust 变体、approved 11 实例/3 gate/artifact 透传、P 失败与 HARNESS_MAX、real-LLM ignored）；PROVENANCE §3 07 hash/行数/合计实测正确；**但 07 AC-13 未进入顶部通过标准/§7.2/§7.3，spec README 仍 241/AC-01..12** |
| 代码结构合理性 | ⚠️（C1） | 变更仅 3 个 docs 文件，DR 仅追加、07 仅追加 AC、PROVENANCE 追加修订记录；未改原决策/其他分册；**PROVENANCE 新条目编号重复（11）与 §3 引用错位** |
| 逻辑性 | ⚠️（C1–C3） | 修订记录/哈希/行数逻辑本身正确；AC-13 内容与 W1 链路自洽；**编号序列 1..13 后新增 11 破坏序号唯一性；AC-13 未进入验收索引；README/07/PROVENANCE 三处范围不同步** |
| 可读性 | ✅（S1–S3） | DR §10 结构清楚、证据紧凑；AC-13 表格/列表可读；PROVENANCE §2 第 11 条措辞清楚；S1/S2/S3 为措辞/空行建议 |

## 3. 发现清单

| # | 级别 | 位置 | 问题 | 建议 | 状态 |
|---|---|---|---|---|---|
| C1 | **重要** | `doc/spec-v1.2/PROVENANCE.md:83,105,126` | §2 既有 `11. REV-035…`（:83），新增 W1 条目也编号 **11**（:105），序号重复；§3 合计行（:126）“见 §2 第 11 条”因此指向 REV-035，而不是本批 W1 记录。违反 CODEOWNERS/§2 第 6 条“§2 追加修订记录”的唯一编号惯例 | 将新增 W1 条目改为 **14**（保留既有 11–13 编号与引用），§3 :126 改为“见 §2 第 14 条”；顺手删除 :109–111 的多余空行 | open（条件） |
| C2 | **重要** | `doc/spec-v1.2/07-验收用例.md:7,29-44,243-249` | 新增 AC-13 后未同步验收索引：顶部通过标准仍“AC-01..AC-12”，§7.2 用例总表最后一行仍 AC-12，§7.3 #3 检查单仍“AC-01..AC-07, AC-09..AC-12”；AC-13 在正式验收/DoD 中不可见 | 顶部改为 `AC-01..AC-13`（或注明 W1 AC-13 的适用方式）；§7.2 增 AC-13 行（主题/对应 DoD/关联规格/W1 实测口径）；§7.3 增 AC-13 检查项或加入 #3；修改后重测 07 SHA-256/行数并同步 PROVENANCE §3/README | open（条件） |
| C3 | **重要** | `doc/spec-v1.2/README.md:30,63` | spec 索引仍写 07 行数 **241**、范围 **AC-01..AC-12**（实际 253、含 AC-13）；与 PROVENANCE §3 和 07 正文不一致；历史批次（如 M6）同步过 README 行数表 | 07 行改为 **253**（若 C2 修改 07 后重新实测）、范围 `AC-01..AC-13`，描述补 W1 SOP v1；:63 验收范围同步 AC-13 | open（条件） |
| S1 | 建议（低） | `doc/decisions/DR-004-v02-scope-and-waves.md:244` | §10 在 W1-docs 分支内写“**后续**：W1-docs（本节 + 07 AC-13）完成后…”，把 W1-docs 自身列为后续，存在自指/时点歧义 | 改为“**本分支（W1-docs，本节 + 07 AC-13）合并后**，按 §4 顺序进入 W2a0…”；不改变波次顺序 | open（建议） |
| S2 | 建议（低） | `doc/spec-v1.2/07-验收用例.md:234` | AC-13 写“full 32 个 `python_node` 符号可 import”，实际测试是 32 个节点条目/17 个去重 `(module,function)` 符号；措辞易被读成 32 个不同符号 | 改为“32 个 `python_node` 节点（17 个去重符号）均可 import” | open（建议） |
| S3 | 建议（低） | `doc/spec-v1.2/PROVENANCE.md:109-111` | 新增条目与 `## 3` 之间多出 3 个空行，格式噪声 | 保留 1 个空行即可（与既有条目一致） | open（建议） |

> 级别口径：阻塞 = 必须修复后才能给“通过”；重要 = 必须修复或登记 ISSUE；建议 = 可后置，但需修复或进台账。

## 4. 验证证据

```bash
# --- 范围/干净度 ---
cd /Users/zyzs/lczyzs/code/zgraph/.worktrees/docs-lead
git status --porcelain --branch        # ## docs/docs-sop-v1-ac...origin/main [ahead 2]；clean
git diff 263b3a8..HEAD --stat         # 3 files changed, 33 insertions(+), 2 deletions(-)
git diff 263b3a8..HEAD --name-only    # DR-004、07-验收用例、PROVENANCE（仅 docs）
git rev-parse HEAD origin/main        # 957ed19... / 263b3a8...（base = origin/main）

# --- 自检 ---
bash scripts/dev/check.sh
# CHECK PASSED（身份/冻结文件/分支/语法/cargo fmt/git diff --check 全 OK）

# --- PROVENANCE §3 实测 ---
shasum -a 256 doc/spec-v1.2/0*.md
# 07-验收用例.md a68bf34d55f50046ce1c246e0926a33d9ee0e8845b36145ad12fbd53922a49c3
wc -l doc/spec-v1.2/0*.md
# 07 = 253；00–08 total = 6050
# §3 07 行 = a68bf34d… / 253；合计 6050；与实测一致。
# 其余 00/01/02/03/04/05/06/08 行 hash/行数与 §3 表逐项一致。

# --- 交叉引用/编号核对 ---
grep -nE "^[0-9]+\. \*\*" doc/spec-v1.2/PROVENANCE.md
# :83  11. REV-035 评审条件修正批次
# :105 11. W1 SOP v1 回写批次        ← 重复编号（C1）
grep -n "§2 第 11 条" doc/spec-v1.2/PROVENANCE.md
# :126  ...见 §2 第 11 条...          ← 指向既有 REV-035，非 W1（C1）
grep -n "AC-01..AC-12\|AC-13" doc/spec-v1.2/07-验收用例.md
# :7  通过标准仍 AC-01..AC-12（C2）
# :44 §7.2 表最后仍是 AC-12（无 AC-13，C2）
# :231 AC-13 正文存在
# :249 §7.3 #3 仍 AC-01..AC-07, AC-09..AC-12（C2）
grep -n "AC-01..AC-12\|241" doc/spec-v1.2/README.md
# :30 07 | 241 | AC-01..AC-12（C3）
# :63 验收仍 AC-01..AC-12（C3）

# --- DR §10 原决策未改 ---
git diff 263b3a8..HEAD -- doc/decisions/DR-004-v02-scope-and-waves.md
# 仅文件末尾追加 §10；无原决策行删改（符合“追加，不改原决策”）
```

- 测试结果：本分支纯 docs；`check.sh` PASSED；无代码测试变化。W1 证据（REV-064/065）已在合并前门禁通过：e2e 7 passed/1 ignored、全 e2e 46/2、pytest 104；与 DR §10/AC-13 引用一致。
- 手工验证：07 hash/行数/合计、其余分册 hash/行数均与 PROVENANCE §3 一致；DR §10 仅追加；差异仅 3 个 docs 文件；worktree clean。
- 本评审未改产品/文档内容；评审记录与索引属 docs 模块（作者 lead、评审人 reviewer-sop ≠ 作者）。

## 5. 遗留问题

- **C1–C3（重要，条件）**：PROVENANCE 编号/引用、07 AC-13 验收索引、spec README 同步；关闭后复审改判。
- **S1–S3（建议）**：DR §10 自指措辞、AC-13 “32 符号”措辞、PROVENANCE 多余空行；可随 C1–C3 一并修或登记 issues。
- 事实核对：A1–A7 摘要、REV-064 `715f13a`、REV-065 `263b3a8`、full/smoke/exhaust hash、7/1、46/2、pytest 104、check.sh 均与当前 main/REV 记录一致；07 AC-13 未夸大全为 validate/real-LLM ignored/失败语义。
- 未发现越界改动、禁用文件、冻结文件修改；分支 base = `origin/main`；worktree clean；本评审未 merge/push/切 main。

## 6. 条件关闭记录（仅"有条件通过"填写）

| 条件 | 关闭证据 | 关闭日期 |
|---|---|---|
| C1（重要）PROVENANCE §2 新条目编号 11 → 14；§3 :126 引用同步为第 14 条 | 待修 | — |
| C2（重要）07 顶部通过标准/§7.2/§7.3 纳入 AC-13；修改后重测 07 SHA-256/行数并同步 PROVENANCE §3 | 待修 | — |
| C3（重要）spec README 07 行数/AC 范围同步（253/AC-13；若 C2 改 07 后重新实测） | 待修 | — |

> C1–C3 关闭后复审改判；S1–S3 为非阻塞建议。
