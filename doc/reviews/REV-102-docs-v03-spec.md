# REV-102 · docs · v0.3 as-built 回写 + AC 实测

- **分支**：`docs/docs-v03-spec`
- **作者**：`docs-dev`（`0dc055b` as-built 主体 + `36098c4` AC 实测收口）
- **评审人**：`reviewer-b`（独立评审，≠ 作者）
- **基于**：main `2ff92c4`（含 V5a e2e / REV-101）
- **变更范围**：`2ff92c4..36098c4`（17 files，+199/−71）：`README.md`、`doc/spec-v1.2/00/02/05/06/07/08/PROVENANCE/README`、DR-004/008/009/010/011、index/issues/progress；**全 docs，无代码/测试/既有评审记录**
- **评审时间**：2026-11-02
- **关联**：task-64/68、REV-096–101、DR-008..011、spec 00/02/05/06/07/08、PROVENANCE、`tests/e2e/*_v03.rs`

## 1. 结论

**结论：有条件通过**（C2 复审于 `adb687e`：**部分关闭**；C1 由 REV-103 `050f86f` 修复/待合并；见 §9）

P0=0；**P1=2**（C1 由 REV-103、C2 仍有残留）；P2=6；P3=3。AC 实测与 PROVENANCE §3 的核心证据**真实、可复核**：AC-18 10 / AC-19 11 / AC-20 7 / 回归 3 = **31**，28 个 AC 用例名与 07 逐名一致；e2e **137/0/2**、workspace（一次通过）**1037/0/3**、pytest **183**、harness **235/0/1**；spec 00–08 **7419 行**、全目录 **7822 行**，§3 9/9 SHA 与实测完全一致；check.sh PASSED；无代码/测试改动。

但两项需关闭后才能作为发布 as-built：

- **C1（P1）workspace 仍存在 flake**：复核 `cargo test --workspace` 一次失败——`plugin::tests::codex_in_flight_cancel_leaves_no_orphan_process_group` panic `fake Codex pid file not written within 10s`（234 passed/1 failed/1 ignored）；随后重跑 **1037/0/3** 通过，隔离单测 3 次通过。**REV-100 的“有界等待”只消除 hang，未消除 pid 等待竞态**；在 README/PROVENANCE 宣称 1037/0/3 稳定前需修测试（等待 fake Codex 写入 pid 后再 cancel，或让 fake Codex 同步到可观测点）+ workspace 连跑 2 次。
- **C2（P1）as-built 仍有多处“待 V5a”/旧计数，和 07/README 实测矛盾**：00 top banner、05 §5.14.4 表格（226/996/180）与偏差 #9（仍称 `plan` 忽略）、06 §6.14.1（workspace 996/待 V5a）、08 §8.12.1（待 V5a）、PROVENANCE §2 第 26 条（`07 待 V5a` + 末尾 `6038 行`）、DR-004/008/009/010/011 执行记录（旧 226/996/180、AC 待 V5a；DR-008 V5b 写 REV-101 而非 REV-102）均未更新到最终 **31/235/1037/137/183** 口径。07 本身已正确实测；这些残留会误导发布文档，需全量对齐并删除“待 V5a”占位。

## 2. 四维评审

| 维度 | 结论 | 依据 |
|---|---|---|
| 功能正确性（AC 实测） | ✅ | 4 target 用例名/计数与 07 一致：skills 10 / MCP 11 / harness 7 / regression 3；新语义（MCP 重启 spawn=2/transport_closed、截断 keys+后缀、http_insecure、llm_calls 1、Codex iterations=1/cached/无 delta、L2 不 spawn）已写入 05/07/08 |
| 结构合理性 | ✅ | 07 改为“用例名 + 断言”双列；PROVENANCE 第 26 条 + §3 重算；DR 只追加；README/progress/index 发布态 |
| 逻辑性 | ⚠️（C1/P2） | 核心 AC 逻辑正确；但 C3 测试竞态导致 workspace 门禁不稳定，as-built 文档部分仍停留在 V5a 前口径 |
| 可读性 | ✅（P2/P3） | 07 可读/可追溯；P2/P3 为跨文档 stale/旧计数、reviews README 缺行等 |

## 3. 发现清单

### P1 · 合并前必须关闭

| # | 位置 | 问题 | 条件 |
|---|---|---|---|
| C1 | `crates/zgraph-harness/src/plugin.rs::tests::codex_in_flight_cancel_leaves_no_orphan_process_group`（经 V4/REV-100 带入 main） | 测试固定 600ms 后 cancel，再等 fake Codex pid；并行负载下 cancel 可能早于 fake Codex 启动/写 pid → 等 10s 后 panic。复核 `cargo test --workspace` 实测 **234 passed / 1 failed / 1 ignored**（同日志）；重跑 1037/0/3、隔离 3 次通过 → **竞态 flake 仍在** | 测试先等 pid 文件出现（有界）再 cancel；或 fake Codex 写 pid 后同步信号；补 workspace 连跑 2 次证据。修复后重跑 `cargo test --workspace` 2 次全绿 |
| C2 | `00`/`05 §5.14.4`/`06 §6.14.1`/`08 §8.12.1`/`PROVENANCE §2 26`/DR-004/008/009/010/011 | 仍写“AC 待 V5a / workspace 996 / harness 226 / pytest 180 / 6038 行 / plan 忽略 / V5b REV-101”等旧口径，与 07/README/PROVENANCE §3 的最终实测（31/235/1037/137/183、7419/7822、REV-102）矛盾 | 全量更新：删除“待 V5a”占位；05 table 及偏差 #9 改为 REV-100 已修（`plan`→progress/usage_update）；PROVENANCE 第 26 条末尾 6038→7419；DR-008 V5b=REV-102；DR-004/009/010/011 计数/AC 状态同步到最终。保留“只追加”历史段，新增 V5a/V5b 结果段 |

### P2 · 建议

| # | 位置 | 问题 | 建议 |
|---|---|---|---|
| N1 | `05 §5.14.4` 表格 | skills/MCP/plugin 表写 226/996/180、未列 V5a 最终 235/1037/137/183；同节底部 V5a 补充段已是最终值 | 表格证据列统一到最终计数（或注明“实现时点”+V5a 最终段） |
| N2 | `06 §6.14.1` | 末尾 `workspace 996/0/3`、`待 V5a`；V5a 已实测 | 改 1037/0/3 并删“待 V5a”；trace v6 结论保留 |
| N3 | `08 §8.12.1` | “待 V5a：AC-18/19/20 实测用例名/计数”后已有 V5a 段；仍留矛盾 | 删前句/改为“V5a 已回写见下”；保留 source-breaking 结论 |
| N4 | `PROVENANCE §2 第 26 条` | 末尾仍 `6038 行`；同文件 §3=7419；`07 待 V5a` 与 07 实测矛盾 | 末尾改 7419；将“07 待 V5a”改为 AC 实测已回写；§3 已正确无需重算 |
| N5 | `DR-004/008/009/010/011` 执行记录 | 计数旧（226/996/180）、AC 待 V5a；DR-008 V5b 行写 `REV-101（docs）` 错（REV-101=e2e，docs=REV-102） | 新增 V5a/V5b 最终证据行（只追加），修正 V5b 评审号；不要改历史行 |
| N6 | `doc/reviews/README.md` | 原缺 **REV-099、REV-100、REV-102** 行；REV-101 有；index 已列 REV-102 评审中。**本评审提交已补三行** | merge 后 index REV-102 改「通过」；REV-101 行已有 |
| N7 | `doc/issues.md` ISSUE-156/161/162 等 | 状态仍是 REV-097/099 首轮 P2/P3；其中 161（plan/usage）、162（stderr/hello/cancel/Codex stderr）、159 部分（truncation/http）已由 REV-100 修复，ISSUE-156 wildcard 已由 REV-097 后续修 | 按 REV-100/3bbe4b5 更新为 fixed/调整剩余项，避免发布台账 stale |

### P3 · 说明

| # | 位置 | 问题 | 建议 |
|---|---|---|---|
| N8 | `README.md` `tests/e2e/` 目录表行 | 仍只列 AC-06/07/09/10/AC-08/AC-12/AC-14/AC-15，未提 V5a AC-18/19/20 四 target | 发布前补一句 V5a 四 target/31 用例 |
| N9 | `doc/index.md` REV-102 状态 | 仍「本分支评审中」；评审通过后应改「通过」 | merge/收口时更新 |
| N10 | `PROVENANCE §2 26` | 同条内有实现时点（7409/7811）与最终（7419/7822）双数字；可保留但需标明时点 | 保留历史时点，明确“最终见 §3” |

## 4. 验证证据

worktree `/Users/zyzs/lczyzs/code/zgraph/.worktrees/docs-docs-dev`，HEAD `36098c4`，base `2ff92c4`，工作区干净；`CARGO_HOME=/Users/zyzs/lczyzs/code/zgraph/.cargo-home`，未设 `CARGO_TARGET_DIR`：

```bash
bash scripts/dev/check.sh                         # PASSED
cargo test -p zgraph-e2e                          # 137 passed / 0 failed / 2 ignored
cargo test --workspace                            # 第一次：234 passed / 1 FAILED / 1 ignored（见 C1）
                                                   # 重跑：1037 passed / 0 failed / 3 ignored
pytest -q                                         # exit 0；183 passed
# PROVENANCE §3 复核（doc/spec-v1.2）：
shasum -a 256 0*.md                               # 9/9 与 PROVENANCE §3 完全一致
wc -l 0*.md                                       # 00–08 = 7419
wc -l *.md                                        # 全目录 = 7822
```

- **AC 实测**：`grep '^fn ac'` → skills 10 / MCP 11 / harness 7；v03_regression `#[test]` 3；07 中 28 个 AC 名与四 target **逐名一致**（doc↔e2e 双向无差集），V5a 合计 **31**。
- **计数**：e2e **137/0/2**；workspace 一次失败/重跑 **1037/0/3**（C1）；pytest **183**；harness **235/0/1**（默认与 mcp 均 235/0/1）。
- **PROVENANCE §3**：9 个 SHA-256/行数与表完全一致；00–08 **7419**、spec 目录 **7822**；00–08 numstat **+109/−40（净 +69）**、spec **+135/−53（净 +82）**；spec README 行数表与 `wc -l` 一致。
- **范围**：17 files 全 docs/README；无代码/测试/既有评审记录改动；无 token/密钥；`git status` 空。
- **新语义**：07/05/08 已写 spawn=2、`transport_closed`、truncated keys+后缀、`http_insecure`、`llm_calls` 1、Codex iterations=1/cached/无 delta、L2 不 spawn；**但 05 旧偏差 #9 仍写 plan 忽略（C2）**。

## 5. 遗留问题 / 条件关闭

- **C1**：修 C3 测试竞态；workspace 连跑 2 次；否则 README/PROVENANCE 的 1037/0/3 不成立。
- **C2**：更新 00/05/06/08/PROVENANCE/DR 的旧计数与“待 V5a”，使 as-built 与 07/README 一致；保留“只追加”历史段。
- P2/P3 N1–N10 随 C2 收口/发布前处理；reviews README 本评审补 REV-102 行（REV-099/100 缺行由对应评审人/本次一并补）。
- 无 P0；C1/C2 关闭后复审转「通过」，再进入 REV-103 发布。

## 9. C2 条件关闭复审（2026-11-02，`adb687e`，parent `b85d008`）

**复审结论：C2 部分关闭，仍有 4 项残留；不转「通过」。** `adb687e` 为 docs-only（12 files +29/−29），check.sh PASSED；PROVENANCE §3 当前 9/9 SHA/行数一致（00–08 **7419**、spec **7822**）、AC 31 名/计数未变、无代码/测试改动。

### 已关闭项（核对通过）
- **00** banner：改为 V5a 实测 31/235/1037/137/183。
- **05 §5.14.4**：表格 226/996/180 → 235/1037/183；偏差 #9 → `plan`→Progress/`usage_update` 折入 usage（REV-100）。
- **06 §6.14.1**：996/待 V5a → 1037/实测 31 用例。
- **08 §8.12.1**：待 V5a → 实测 31 用例（保留真实 Codex/DSH 未验证）。
- **PROVENANCE §2 26**：07 待 V5a → V5a 实测 31；§3 SHA/行数重算。
- **DR-008**：V2–V4 计数 235/1037/183；V5b 评审号改 **REV-102**。
- **DR-009/010/011**：计数与 AC 实测数已对齐；**但 DR-011 实现偏差 bullet 仍 stale（见下）**。
- **issues**：ISSUE-159/161/162 标 fixed（REV-100）；README `tests/e2e/` 补 V5a 四 target；reviews README 已补 REV-099/100/102 行。

### 仍残留（C2 未完全关闭）
| # | 位置 | 残留 | 修复建议 |
|---|---|---|---|
| R1 | `doc/spec-v1.2/PROVENANCE.md:257` | v0.3 §2 第 26 条末行仍「00–08 **7409 行** / 总 **7811**」；同文件 §3 已是 **7419/7822** | 改为 7419/7822，或明确标「实现主体时点；最终见 §3」；与 C2 原意「末尾 7419」一致 |
| R2 | `doc/decisions/DR-011-v03-harness-plugin-design.md:169` | 仍写 `plan` 当未知 event 忽略+warn（REV-099 N1 待改）、`usage_update` 忽略、stderr head 8 KiB（N2 待改）；REV-100 已修 | 改「plan→Progress、usage_update 折入 usage、stderr head+tail 各4KiB（REV-100）」；ISSUE-161/162 fixed 已佐证 |
| R3 | `doc/decisions/DR-004-v02-scope-and-waves.md:280` | 标题仍「docs REV-101」；末尾「下一步 V5a e2e → V5b 发布 v0.3.0」；实际 docs=REV-102、V5a 已完成 | 改 docs REV-102；下一步改「REV-102 复核 → REV-104 发布」 |
| R4 | `doc/index.md:29` | REV-102 仍「本分支评审中」 | 复审/合并时改「通过」；属 C2 P3 台账项 |

> 另 P3：PROVENANCE 26 的「reviews README REV-097–101」未随 REV-099/100/102 三行补全；可改「REV-097–102」。

### C1 说明
- C1 由独立 `fix/harness-codex-cancel-race`（REV-103，`050f86f`）修复，待 Lead 按「harness fix → docs rebase → docs merge」顺序合并；本 docs 分支当前未含该 commit。C1 关闭后需在 docs rebase 后复跑 workspace ×2，届时 REV-102 才能转「通过」。

### 复审门禁
```bash
bash scripts/dev/check.sh                 # PASSED
cd doc/spec-v1.2 && shasum -a 256 0*.md   # 9/9 与 §3 一致
wc -l 0*.md                               # 00–08=7419
wc -l *.md                                # spec 目录=7822
grep -rn '待 V5a|996|226/|6038' doc README.md  # 无核心残留（7409/7811 与 DR-011 见 R1/R2）
```
