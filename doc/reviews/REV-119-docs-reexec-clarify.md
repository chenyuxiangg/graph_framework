# REV-119 · docs · reexec 口径文档修订（DR-017/05/06/07/AC-25）

> **评审范围**：docs-only（7 文件）；对齐 F1b 实现口径：候选/非候选、exit2 范围、recorded 不变/reexec additive、MISSING/TRUNCATED/MISMATCH（缺 `result_sha256`=MISSING）、公共 `tool_result_sha256`、无 v8。无代码/测试/既有评审记录改动；未触碰 `dist/`、无 token。评审人只新增本评审记录。

- **分支**：`docs/docs-v05-reexec-clarify`
- 分支: docs/docs-v05-reexec-clarify
- **作者**：`docs-dev`（commits `8a8c459`（主修订）→`dc35666`（Lead 裁决 MISSING）；正文 `Review: REV-119` / `Agent: docs-dev`）
- **评审人**：`reviewer-a`（独立评审，≠ 作者）
- **基于**：main `d3da745`（含 REV-118 第二评审；`main = origin/main = d3da745`）
- **变更范围**：`d3da745..dc35666`（**7 files, +81/−74**）：`DR-017`、`spec 05 §5.16.1/06 §6.16.1/07 AC-25/08 §8.14`、`index`、`progress`
- **评审时间**：2026-11-03
- **关联**：task-105/107；DR-017、AC-25、REV-118（F1b 实现）、F1a/REV-116

## 1. 结论

**结论：有条件通过**

P0=0、P1=0；**P2 条件 C1/C2**（`doc/progress.md` 仍写「REV-119 期望 MISMATCH / 待确认」，与 Lead 裁决 MISSING 相反；`DR-016` 风险/AC-25 矩阵仍写「副作用工具拒绝 exit2」，与 F1b 非候选复用口径不一致）；P3 2 项（v0.4 历史行口径/索引细节）。DR-017/05/06/07/08/index 主体已对齐实现；C1/C2 为 docs 小修，关闭后通过。

### 1.1 已核对成立

| 项 | 结论 |
|---|---|
| DR-017 重写 | ✅ 候选 `source=builtin && read/grep/glob/diff && status=ok`；非候选副作用/MCP/Python/`status!=ok` 复用记录、绝不执行、不报 exit2；exit2 仅缺 `--allow-builtin-tools`/显式 `--workspace-dir`；非法 workspace → `HARNESS_CONFIG_LOAD` exit3；recorded 逐字节不变；reexec additive `replay_mode`/`drift=false`；mismatch exit3 不输出 drift=true；MISSING/TRUNCATED/MISMATCH 语义；公共 `tool_result_sha256`；无 v8 |
| 缺 sha 裁决 | ✅ Lead 裁决 **缺 `result_sha256` = `REPLAY_TOOL_RECORD_MISSING`**（记录不完整，非漂移）；DR-017 §2/§7、05 §5.16.1、06 §6.16.1、07 AC-25 f 均已改；与 F1b 实现一致（`result_sha256` None → MISSING，代码不改） |
| 05/06/07/08 | ✅ 05 §5.16.1 候选/非候选/exit2/exit3/additive；06 §6.16.1 同 + `tool_result_sha256`；07 AC-25 b/d/f 明确 additive、非候选复用、MISSING；08 §8.14 更新 réeexec 行；AC-24h 标注 F1c 完整 e2e |
| index | ✅ DR-017 摘要改为候选/非候选/exit 口径；reviews 范围至 REV-119 |
| 与实现一致 | ✅ 对照 `crates/zgraph-cli/src/replay_cmd.rs`：候选条件/前置 parse exit2/非法 workspace exit3/非候选复用/输出 additive/`result_sha256` None → MISSING；F1b 6 测试覆盖 |
| 范围/门禁 | ✅ docs-only 7 文件；无代码/测试/既有评审改动；无 token/dist；`check.sh` **PASSED** |

## 2. 四维评审

| 维度 | 结论 | 依据 |
|---|---|---|
| 功能正确性 | ⚠️ 有条件 | 主体口径与 F1b 一致；C1 progress 仍写相反裁决、C2 DR-016 旧 exit2 语义未同步 |
| 代码结构合理性 | ✅ | docs-only；DR-017 重排序/表格化；spec 分册一致；无代码/测试改动 |
| 逻辑性 | ✅（C1/C2） | 候选/非候选/exit2/exit3/MISSING/TRUNCATED/MISMATCH 自洽；C1/C2 为跨文档残留 |
| 可读性 | ✅（P3） | 摘要/场景表清晰；P3 历史行/索引细节 |

## 3. 发现清单

### 3.1 P2 条件（本分支 docs 小修后关闭）

| # | 位置 | 问题 | 建议 |
|---|---|---|---|
| **C1** | `doc/progress.md:15` | **仍写「DR-017 缺 `result_sha256` 码：REV-119 期望 MISMATCH，F1b 实现为 MISSING（待 F1c/评审确认）」**，与 Lead 裁决「缺 sha=MISSING」及本分支 DR-017/05/06/07 已改内容相反；且列为「阻塞」待确认 | 改为「REV-119 已裁决缺 `result_sha256` = `REPLAY_TOOL_RECORD_MISSING`，F1b 实现一致；F1c 补 e2e」并移出阻塞（或标注已关闭） |
| **C2** | `doc/decisions/DR-016-v05-scope-and-waves.md:81`（风险「`--replay-tools=reexec` 但工具为副作用 → 用法错误/拒绝（exit 2）」）、`:91`（AC-25 矩阵「副作用工具拒绝」） | DR-016 仍保留 F1b 已废弃口径（副作用 exit2/拒绝），与 DR-017/05/06/07/REV-118 的「非候选复用、不执行、不报 exit2」矛盾 | 改为「非候选（副作用/MCP/Python/失败行）复用记录、不执行、不报 exit2；exit2 仅缺 flags；非法 workspace CONFIG_LOAD exit3」 |

### 3.2 P3（建议）

- **P3-1**：`doc/spec-v1.2/07-验收用例.md:602` AC-24h 仍写「`reexec` v0.4 用法错误 exit 2」；v0.5 F1b 下该 e2e 已改为「缺前置 flag exit2」，建议注明 v0.4 历史/更新为 v0.5 前置口径（AC-25 已覆盖）。
- **P3-2**：`doc/spec-v1.2/08-审计关闭矩阵.md:640`（§8.13 v0.4 历史）仍写「reexec 用法错误 exit 2」；§8.14 已更新，建议在该历史行加「v0.4 历史；v0.5 见 §8.14」避免误读。
- **P3-3**：`doc/index.md` reviews 范围已到 119，reviews README 行由本次评审补；progress 当前阶段/下一步可随 merge 更新。

## 4. 验证证据

评审人亲自执行（worktree `.worktrees/docs-docs-dev`，HEAD `dc35666`，main `d3da745`，worktree 干净）：

```bash
git status --porcelain                     # 空
git diff --stat main...HEAD                # 7 files +81/−74（docs-only）
bash scripts/dev/check.sh                  # CHECK PASSED
grep -n "候选行\|非候选\|MISSING\|exit2\|replay_mode\|drift" DR-017/spec 05/06/07/08
# 对照 crates/zgraph-cli/src/replay_cmd.rs + tests/tool_reexec.rs（F1b）：
#   source=builtin && is_reexec_readonly_tool && status=ok；缺 flag parse exit2；
#   workspace 非法 EXIT_ENTRY_INPUT=3；result_sha256 None→MISSING；非候选复用
```

- **范围/约束**：docs-only 7 文件；无 `crates/`/`tests/`/`doc/reviews/` 改动；无 token/dist；check.sh PASSED。
- **实现对照**：DR-017 候选/exit2/exit3/additive/MISSING 与 `replay_cmd.rs`/`tool_reexec` 测试一致；F1b 门禁见 REV-118（cli 255/0、tool_reexec 6/0、workspace 1234/0）。
- **历史行**：DR-014 等 v0.4 历史保留；spec 08 §8.13 旧行见 P3-2。

## 5. 条件关闭方式

- **C1/C2 由 docs-dev 在本分支 docs 小 commit 关闭**：修 `progress:15` 与 `DR-016:81/91`；重跑 check.sh。
- 关闭后评审人复核 grep（progress 无「期望 MISMATCH/待确认」、DR-016 无「副作用 exit2/拒绝」）+ check.sh，追加「条件关闭」段、结论改「通过」；随后 Lead merge。

## 6. 条件关闭记录（"有条件通过"）

| 条件 | 状态 | 关闭要求 |
|---|---|---|
| C1 progress 缺 sha 裁决改为 MISSING/已关闭 | **open** | `doc/progress.md:15`；check.sh |
| C2 DR-016 风险/AC-25 改非候选复用口径 | **open** | `DR-016:81/91`；check.sh |

## 7. 遗留问题

- C1/C2 关闭前不 merge；P3-1/P3-2 历史行可同批或后续 docs 小修。
- 实现不改代码（Lead 裁决）；F1c 补 AC-25 完整 e2e；F2 待。
- 无代码/测试影响；docs-only。
