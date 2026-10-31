# REV-066 · docs · W1 收口（DR-004 §10 + 07 AC-13）

- **分支**：`docs/docs-sop-v1-ac`
- **作者**：`lead`（初版 `393b2b6` DR-004 §10 + 07 AC-13；`957ed19` PROVENANCE；复审修复 `f606be8` C1–C3/S1–S3；均含 `Review: REV-066` / `Agent: lead`）
- **评审人**：`reviewer-sop`（**≠ 作者**；首轮 `02cea3d`，本次复审）
- **变更范围**：`263b3a8..f606be8`，作者 docs **4 文件 +37/−7**（`doc/decisions/DR-004-v02-scope-and-waves.md`、`doc/spec-v1.2/07-验收用例.md`、`doc/spec-v1.2/PROVENANCE.md`、`doc/spec-v1.2/README.md`）；评审记录 + `doc/reviews/README.md` 另 2 文件（reviewer）；未改产品/测试/fixture/CI
- **评审时间**：2026-10-31
- **关联**：DR-004 §10；SOP 设计稿 v1 §11；REV-064（`715f13a`）、REV-065（`263b3a8`）；PROVENANCE §2 第 6 条修订规则

## 1. 结论

**结论：通过**

首轮事实核对继续成立：DR-004 §10 的 A1–A7 摘要、REV-064/065 合并提交、full/smoke/exhaust hash、7 passed/1 ignored、46 passed/2 ignored、pytest 104、check.sh 均与记录一致；07 AC-13 与 e2e 实际断言一致（full 只 validate、real-LLM 仅 `#[ignore]`、P 失败/`HARNESS_MAX_ITERATIONS`、不把 gate=false 当失败），无夸大；PROVENANCE §3 已同步 07 `a02afcd5…` / 254 行 / 合计 6051。

复审关闭情况：C1（PROVENANCE 编号 11→14 + §3 引用）、C2（AC-13 进入顶部通过标准/§7.2/§7.3，07 hash/行数重测）、C3（spec README 254/AC-01..AC-13 同步）均已在 `f606be8` 关闭；S1（DR §10 自指措辞）、S2（32 节点/17 去重符号）、S3（多余空行）均已落实。仅余 S4（PROVENANCE #14 改动范围可补记 README 同步）为非阻塞建议；无新的阻塞/重要问题。

## 2. 四维评审

| 维度 | 结论 | 依据（文件/行/测试） |
|---|---|---|
| 功能正确性 | ✅ | DR §10 与 A1–A7、REV-064/065、hash/测试数字逐项一致；07 AC-13 与 e2e 实际断言一致；PROVENANCE §3 07 hash/行数/合计实测正确；README/07 的 AC 范围已同步 |
| 代码结构合理性 | ✅ | 变更仅 docs；DR 仅追加、07 只新增/同步 AC-13、PROVENANCE 修订记录、README 索引同步；未改产品/测试/fixture/CI；PROVENANCE 序号 1..14 唯一 |
| 逻辑性 | ✅ | 修订记录/哈希/行数自洽；AC-13 已进入通过标准/用例总表/DoD 检查单；README 与 PROVENANCE §3 一致；DR 波次顺序不变 |
| 可读性 | ✅（S4） | DR §10 结构清楚；AC-13 证据紧凑；“本分支（W1-docs）合并后”消除自指；PROVENANCE 条目空行收敛；S4 为变更范围记录建议 |

## 3. 发现清单

| # | 级别 | 位置 | 问题 | 建议 | 状态 |
|---|---|---|---|---|---|
| C1 | 重要 | `doc/spec-v1.2/PROVENANCE.md:105,124` | 新条目原编号 **11** 与既有 REV-035 重号；§3 原引用“§2 第 11 条”指错 | 改为 **14**，§3 改为“第 14 条”；删除多余空行 | 已关闭（`f606be8`） |
| C2 | 重要 | `doc/spec-v1.2/07-验收用例.md:7,45,250` | AC-13 未进入顶部通过标准/§7.2/§7.3 | 顶部 `AC-01..AC-13`；§7.2 增 AC-13 行；§7.3 #3 改为 `AC-01..AC-07, AC-09..AC-13`；重测 07 | 已关闭（`f606be8`：`a02afcd5…` / 254 行 / 合计 6051） |
| C3 | 重要 | `doc/spec-v1.2/README.md:30,63` | 07 行数/AC 范围仍 241/AC-01..AC-12 | 更新为 254/AC-01..AC-13，并在验证方式引用 §2 第 14 条 | 已关闭（`f606be8`） |
| S1 | 建议（低） | `doc/decisions/DR-004-v02-scope-and-waves.md:244` | §10 在 W1-docs 分支内把 W1-docs 列为“后续”，自指 | 改为“本分支（W1-docs）合并后…” | 已落实（`f606be8`） |
| S2 | 建议（低） | `doc/spec-v1.2/07-验收用例.md:235` | “32 个符号”易被读成 32 个不同符号 | 改为“32 个 `python_node` 节点（17 个去重符号）” | 已落实（`f606be8`） |
| S3 | 建议（低） | `doc/spec-v1.2/PROVENANCE.md:105-108` | 新增条目与 `## 3` 之间多余空行 | 保留 1 个空行 | 已落实（`f606be8`） |
| S4 | 建议（低） | `doc/spec-v1.2/PROVENANCE.md:105-108` | #14 的“改动范围”列了 07/DR，未列 `README.md` 07 行数/AC 范围同步（实际 diff 含 README）；不影响 C1–C3 关闭，但修订记录不够完整 | 在 #14 补一句“README 07 行数/AC 范围同步”；可后置/登记 ISSUE | open（建议，非阻塞） |

> 级别口径：阻塞 = 必须修复后才能给“通过”；重要 = 必须修复或登记 ISSUE；建议 = 可后置，但需修复或进台账。

## 4. 验证证据

```bash
# --- 范围/干净度 ---
cd /Users/zyzs/lczyzs/code/zgraph/.worktrees/docs-lead
git status --porcelain --branch        # ## docs/docs-sop-v1-ac...origin/main [ahead 4]；clean
git diff 02cea3d..f606be8 --shortstat  # 4 files changed, 12 insertions(+), 13 deletions(-)（复审修复）
git diff 263b3a8..f606be8 --shortstat -- doc/decisions doc/spec-v1.2
# 4 files changed, 37 insertions(+), 7 deletions(-)（作者 docs；另 2 文件为评审记录/索引）
git diff 263b3a8..f606be8 --name-status -- doc/decisions doc/spec-v1.2
# DR-004、07、PROVENANCE、README（仅 docs）
git rev-parse HEAD origin/main          # f606be8... / 263b3a8...（base = origin/main）

# --- 自检 ---
bash scripts/dev/check.sh
# CHECK PASSED（身份/冻结文件/分支/语法/cargo fmt/git diff --check 全 OK）

# --- PROVENANCE §3 实测 ---
shasum -a 256 doc/spec-v1.2/0*.md
# 07-验收用例.md a02afcd5b2a42a3afcbd34a2947d7e9238632c892c0f576c982af4b06273ff92
wc -l doc/spec-v1.2/0*.md
# 07 = 254；00–08 total = 6051
# §3 07 行 = a02afcd5… / 254；合计 6051；与实测一致。
# 其余 00/01/02/03/04/05/06/08 行 hash/行数与 §3 表逐项一致。

# --- 交叉引用/编号核对 ---
grep -nE "^[0-9]+\. \*\*" doc/spec-v1.2/PROVENANCE.md
# :83 11. REV-035；:90 13. M6；:105 14. W1 SOP v1（无重复）
grep -n "§2 第 14 条\|§2 第 11 条" doc/spec-v1.2/PROVENANCE.md doc/spec-v1.2/README.md
# PROVENANCE:124 见 §2 第 14 条；README:61 W1 批次见 §2 第 14 条
grep -n "AC-01..AC-13\|AC-13\|AC-09..AC-13" doc/spec-v1.2/07-验收用例.md
# :7 通过标准 AC-01..AC-13；:45 §7.2 AC-13 行；:232 AC-13 正文；:250 §7.3 #3 AC-09..AC-13
grep -n "07-验收用例\|AC-01..AC-13\|254\|241" doc/spec-v1.2/README.md
# :30 07 | 254 | AC-01..AC-13；:63 验收 AC-01..AC-13（无 241/AC-01..AC-12）
grep -n "本分支（W1-docs）合并后" doc/decisions/DR-004-v02-scope-and-waves.md
# :244 S1 已落实

# --- DR §10 原决策未改（仅追加 + S1 措辞）---
git diff 263b3a8..f606be8 -- doc/decisions/DR-004-v02-scope-and-waves.md
# 仅末尾 §10（原决策行未改）
```

- 测试结果：本分支纯 docs；`check.sh` PASSED；无代码测试变化。W1 e2e 合并前门禁 7 passed/1 ignored、全 e2e 46/2、pytest 104 与 DR §10/AC-13 引用一致。
- 手工验证：07 `a02afcd5…` / 254、合计 6051 与 PROVENANCE §3/README 一致；索引/编号无重复；AC-13 已进入通过标准/§7.2/§7.3；S1–S3 已落实。
- 本评审未改产品/文档内容；评审记录与索引属 docs 模块（作者 lead、评审人 reviewer-sop ≠ 作者）。

## 5. 遗留问题

- **S4（建议，低）**：PROVENANCE #14 可补记 README 同步；可后置或登记 `doc/issues.md`，不阻塞合并。
- C1–C3 已关闭；S1–S3 已落实；未发现新的阻塞/重要问题。
- 事实核对：A1–A7 摘要、REV-064 `715f13a`、REV-065 `263b3a8`、full/smoke/exhaust hash、7/1、46/2、pytest 104、check.sh 均与当前 main/REV 记录一致。
- 未发现越界改动、禁用文件、冻结文件修改；分支 base = `origin/main`；worktree clean；本评审未 merge/push/切 main。

## 6. 条件关闭记录（仅"有条件通过"填写）

| 条件 | 关闭证据 | 关闭日期 |
|---|---|---|
| C1（重要）PROVENANCE #2 新条目 11 → 14；§3 引用同步 | `f606be8`：:105 `14. W1 SOP…`，§3 见第 14 条；空行收敛 | ✅ 关闭 2026-10-31 |
| C2（重要）07 顶部/§7.2/§7.3 纳入 AC-13，重测 hash/行数 | `f606be8`：AC-01..AC-13、§7.2 AC-13 行、§7.3 AC-09..AC-13；07 `a02afcd5…` / 254 / 合计 6051；PROVENANCE §3 同步 | ✅ 关闭 2026-10-31 |
| C3（重要）spec README 07 行数/AC 范围同步 | `f606be8`：README 07 = 254/AC-01..AC-13；:63 AC-01..AC-13；:61 引 §2 第 14 条 | ✅ 关闭 2026-10-31 |

> C1–C3 已关闭；S1–S3 已落实；S4 为非阻塞建议。本记录结论 **通过**；合并仍须通过 `scripts/dev/merge.sh` 门禁。
