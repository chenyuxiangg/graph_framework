# REV-094 · docs · W4b 动态 N（foreach）as-built 回写

> **范围声明**：本分支为 **17 个纯文档文件**（+160/−57），全部 `doc/**`；无代码/测试/配置改动，未改既有评审记录（仅 `doc/reviews/README.md` 索引由本评审补 REV-094 行）。评审人只新增本评审记录并补全 reviews 索引行，未改其它文档内容。

- **分支**：`docs/docs-dynamic-spec`
- 分支: docs/docs-dynamic-spec
- **作者**：`docs-dev`（提交 `1980810`；正文 `Review: REV-094` / `Agent: docs-dev`）
- **评审人**：`reviewer-a`（独立评审，**≠ 作者**）
- **变更范围**：`201341b..1980810`（**17 files, +160/−57**）：DR-004 §10 / DR-007 §14.1、SOP v1.2 as-built、spec 00/01/02/03/04/06/07/08、PROVENANCE、spec README、`doc/{index,issues,progress,reviews/README}.md`
- **评审时间**：2026-11-01
- **关联**：DR-007（§14.1 执行记录）；REV-088..093（W4b 实现/修复/e2e）；task-35/41；spec 06/07 W4a 设计段 → W4b as-built

## 1. 结论

**结论：通过**

按 task-41/51 评审口径逐条核对，as-built 与已合并实现/测试**一致**；AC-17 **21 用例名逐一真实存在**且计数经评审人实测复现（e2e 106/0/2、dynamic_fanout 21/0×3、workspace 915/0/3、kernel 149/159、cli 222、trace 94、dsl 118）；PROVENANCE 第 25 条 + §3 数字/SHA 实测一致；台账/进度/索引格式合规；无代码改动、未改既有评审记录、DR-004/DR-007 纯追加。P0=0、P1=0、**P2=2**（均非阻塞：trace 实现提交 SHA 为 rebase 前旧值、08 §8.11.2 source-breaking 清单有遗漏）、P3=4。P2 建议由 docs-dev 合并前小 commit 修正（或 Lead 登记 ISSUE）——不改变本分支结论。

### 1.1 as-built ↔ 实现/测试核对

| 文档主张 | 实测证据 | 结论 |
|---|---|---|
| **foreach `items_port` 取值规则 A**（array 直用；object 同名键 unwrap；其余 NOT_ARRAY；只取一层） | `crates/zgraph-kernel/src/run.rs:1665-1681` 实现（`object.contains_key(&items_port)` → 取键值，其余 NOT_ARRAY）；`ac17a`/`ac17f` e2e 真链路覆盖；kernel `foreach.rs` 25 例 | ✅ |
| **trace schema v6**（item 列/四列唯一键/`foreach_expansions`/writer/reader） | `sort` 实测 trace **94/0**（migration **18**、expansions **5**、item_instances **5**）；`ForeachExpansionRecord`/`TraceInput::ForeachExpansion`/`TraceReader::foreach_expansions`（`ORDER BY at, expansion_id`）存在 | ✅ |
| **真实 v1→v6 + FK CASCADE 窗口 + ISSUE-142 fixed** | `schema.rs:651-659` `with_foreign_keys_disabled`（事务外 OFF→恢复 ON）；`DDL_INSTANCES_V6_INDEX` 独立（`:422`，fresh/v5→v6 才建，**:623-624**）；`tests/migration.rs:167+` 真实 v0.1.0 v1 形状（6+2 表）v1→v6；ISSUE-142 台账 `fixed` | ✅（SHA 引用见 N1） |
| **kernel controller/Pause/终止/join** | `ControllerState`/`ControllerBuffer`/`JoinBuffer`；Resume 后重扫 `expand_ready_controllers`（`run.rs:799,830`）；`source_instance_id` 仅 Collecting 记录（`:1519`）；F Expanded/Skipped 后投递 → `RUN_DELIVERY_TO_TERMINAL`（`:1492-1505`）；serial F2 state 检查（`:1785`）；`incomplete_join` 防御（`:1080`）；N=0 无 GateResult/Delivery（`:1731,1773`）；冷峻失败 `cancel_active_instances` 仅取消非终态（`:2828-2835`，Completed 保留） | ✅ |
| **cli trace/--json 行序、--stats 冻结、L2 per-item 预检** | REV-091 已独立核实（本分支仅文档，代码未变）：行序 events→HumanDecision→ForeachExpansion、`(at,expansion_id)`、`source_instance_id` null 不省略、`--stats` 不变、`RecordKey=(node_id,attempt,item_index)`、CORRUPT/MISSING/Skipped 合法；e2e `ac17h`/`ac17i` 真链路 | ✅ |
| **R17/R18/19 码、`region_overlap` 不可达** | REV-088 已核实；02 banner as-built 与 `dsl 118/0`、fixtures 56 bad/8 valid 一致；dsl 单测锁定优先级 | ✅ |
| **08 §8.11 source-breaking** | 新增 §8.11（实现证据表 + 8 行兼容备注 + 偏差/遗留）；与 W3a §8.10 体例一致；**清单完整性有遗漏（N2）** | ⚠️ 见 N2 |
| **07 AC-17 21 用例名真实** | `tests/e2e/dynamic_fanout.rs` 实际 **21** 个 `#[test]`/`#[tokio::test]`；与 07 §AC-17 列出的 21 个用例名**集合完全相等**（差集为空）；分组 8 静态/迁移/默认 + a1/b1/c1/d1/e1/f2/g1/h1/i2/j2 = 21 ✓ | ✅ |
| **计数与 main 实测一致** | 评审实跑：`dynamic_fanout` **21/0**（3 次：14.55s/1.26s/0.98s）、e2e **106/0/2**、workspace **915/0/3**、kernel **149/0** / test-hooks **159/0**、cli **222/0**、trace **94/0**、dsl **118/0** —— 与 DR-007 §14.1/08 §8.11.1/07/03/02/PROVENANCE 所述逐项一致 | ✅ |
| **PROVENANCE 第 25 条 + §3** | 00–08=`wc -l` **7198**、含 README/PROVENANCE/REFERENCES **7588**；**9/9 SHA-256+行数 ALL_MATCH**；00–08 numstat **+76/−32（净+44）**、全 spec 目录 **+106/−50（净+56）** —— 与 §2 第 25 条/§3 注实测一致；spec README 行数表 **9/9 行数实测一致** | ✅ |
| **台账/索引/进度** | ISSUE-142（P1，fixed）/ISSUE-143–149（P3，open）共 8 行、各 8 列、编号连续、模块/建议/映射齐全；`progress.md` **15 行四块**（W4b 完成 → 发布收口：性能基线/兼容矩阵/tag v0.2.0）；index/SOP as-built 已同步；reviews README 补 REV-088–094（REV-094 行由本评审补全） | ✅（索引缺口见 N3） |
| **只追加/无副作用** | `git diff --name-only main...HEAD` 全为 `doc/**`；`doc/reviews/` 仅 `README.md`；DR-004 **+1/−0**、DR-007 **+22/−0**（纯追加）；无 Cargo/代码改动 | ✅ |

## 2. 四维评审

| 维度 | 结论 | 依据（文件/行/测试） |
|---|---|---|
| 功能正确性 | ✅ | as-built 与实现/测试逐条一致（§1.1）；AC-17 21 用例名与源码集合完全相等；全部计数评审实跑复现；PROVENANCE §3 9/9 SHA/行数实测一致；仅 N1（SHA 溯源）与 N2（08 清单遗漏）为文档准确性问题 |
| 代码结构合理性 | ✅（N2/N3 备注） | 采用既有 docs 结构：banner as-built + 章节 as-built 注 + DR-007 §14.1 执行记录 + 08 §8.11 增量节 + PROVENANCE §2/§3 + 台账/索引/进度；00–04 banner 追加、02/03/06 详注、07 设计段重写为实测（任务允许）、08 只新增 §8.11（不重写 8.0–8.10）；DR-004/DR-007 纯追加 |
| 逻辑性 | ✅ | 可追溯链模块→分支/提交→REV→合并点→测试；同一数字在 DR-007 §14.1/08 §8.11.1/PROVENANCE §2/07/03/02 一致（除 N1 的 trace SHA 与 N4 合并顺序表述）；AC-17 a–j 分组与 e2e 实际用例一一对应；ISSUE 142 fixed/143–149 open 与 REV 映射闭合 |
| 可读性 | ✅（N3/N4/N5 备注） | as-built 注均带日期/REV/DR 锚点；偏差「为什么」解释清楚（unwrap 规则 A、FK 窗口、Pause 修复、终态语义）；N5 资产图数笔误、N4 执行顺序表述、索引范围 N3 为小瑕疵 |

## 3. 发现清单

> 级别口径：P0=阻塞；P1=重要（必须修复或登记 ISSUE）；P2=一般；P3=建议。本分支 **P0=0、P1=0**。

| # | 级别 | 位置 | 问题 | 建议 | 状态 |
|---|---|---|---|---|---|
| 1 | **P2 一般** | `doc/decisions/DR-007-...md:494`；`doc/decisions/DR-004-...md:274`；`doc/spec-v1.2/08-审计关闭矩阵.md:577`；`doc/spec-v1.2/PROVENANCE.md:235`；`doc/spec-v1.2/06-存储与可观测.md:798`；`doc/issues.md:150` | **trace 实现/修复提交 SHA 为 rebase 前旧值**：多处引用 trace `49806c2`（实现）与 `dc25340`（修复），二者对象存在但与同主题 main 实际提交 **`93a5f23`**（`feat(trace): schema v6 item 实例/foreach_expansions`）/**`e057cef`**（`fix(trace): 修复真实 v1→v6 迁移 + FK/备份断言`）不同；`git merge-base --is-ancestor 49806c2 HEAD` = 否（非 main 祖先；trace 分支合并前 rebase，DSL 分支同理未受影响因为其提交是 main 祖先）。审计/发布说明引用 main 不可达提交 | 改为 main 实际提交（`93a5f23`/`e057cef`），或注明「分支提交（rebase 前）/ main 提交」；涉及 06/08/PROVENANCE 的文本改动需重算 §3 中 06/08 的 SHA-256（行数不变）；issue 142 的 fixed 提交同步改 | open（建议修正，非阻塞） |
| 2 | **P2 一般** | `doc/spec-v1.2/08-审计关闭矩阵.md:584-596`（§8.11.2） | **source-breaking 清单不完整**（v0.2 发布说明「必须列出」）：与 W3a §8.10 体例对比，缺 ① kernel `TransitionRecord` **新增 `item_index`/`item_key` 公开字段**（`crates/zgraph-kernel/src/trace.rs:102-123`；公共结构体字面量构造会 break；现 item 8 只写 trace `TransitionRecord` 无 item 字段，未列 kernel 结构变更）；② kernel `TraceSink::foreach_expansion` **默认 no-op**（W3a 先例将 `human_decided` 默认 no-op 列入）；③ dsl 新公开类型 `ForeachConfig`/`OnItemError`（item 5 仅列字段未列类型）；④ `NodeContext::item_index()/item_key()`（`context.rs:97,102`）、`TraceCall::ForeachExpansion`/`MemoryTraceSink::foreach_expansions()`、`ItemIndex`/`ExpansionId` 别名等 additive API | 补 1–2 行 + 扩展现有行（尤其 kernel `TransitionRecord` 字段与 `TraceSink::foreach_expansion`）；改后重算 08 的 SHA-256 到 PROVENANCE §3 | open（建议修正，非阻塞） |
| 3 | P3 建议 | `doc/index.md:24`；`doc/reviews/README.md:89-95` | 索引不连续：`index.md` reviews 行范围仍写「REV-038–**086**」（本批已在该行写入 REV-088–094，却未更新范围），且**缺 REV-087**（W4a wrapup 记录存在）；`reviews/README.md` 表缺 **REV-085/086/087 三行**（main 既有缺口，本批补 088–094 时未顺手补齐） | 本批或紧随 commit 补 3 行索引、index 范围改 `REV-038–094`（纯索引，不改记录） | open（建议项） |
| 4 | P3 建议 | `doc/decisions/DR-004-...md:274`；`doc/decisions/DR-007-...md:489-496`；`doc/spec-v1.2/PROVENANCE.md:235` | **执行记录顺序与真实 main 历史不一致**：三处均写「kernel → **fix** → cli → e2e」，实际 main 顺序为 kernel `519066d` → cli `4a1ff0f` → fix `683260b` → e2e `201341b`（`git log --first-parent --reverse 42f93d7..201341b` 实证）；reviews README 底部行顺序是正确的（`519066d→4a1ff0f→683260b→201341b`） | 统一为实际合并顺序（fix 行移到 cli 之后），或注明「表按模块分组、非时间序」 | open（建议项） |
| 5 | P3 建议 | `doc/spec-v1.2/07-验收用例.md:423` | AC-17 as-built 写资产「`tests/fixtures/graphs/foreach_dynamic/**`（**11 图**）」，实际目录 **12** 个图（entry_items3/upstream_items/empty_items/item_failure/dup_keys/exceeded/not_array/cancel/human_items2/exit_skipped/keys_canonical/controller_skip，均被 dynamic_fanout.rs 引用） | 改为「12 图」 | open（建议项） |
| 6 | P3 建议 | `doc/spec-v1.2/07-验收用例.md:14` | 通过标准行仍写「AC-17 在 W4b 实现后**由设计转实测**」（本批已实测），且「连续 10 次运行无 flaky」与 W4b 实测口径（dynamic_fanout 6 连跑、全量 e2e 3 连跑）不一致 | 改为「AC-17 已实测」并把连跑口径与 AC-17/DoD 统一（或注明全量 10 连跑为发布前目标） | open（建议项） |

## 4. 验证证据

评审人**亲自执行**（worktree `/Users/zyzs/lczyzs/code/zgraph/.worktrees/docs-docs-dev`，HEAD `1980810`，基于 `main = origin/main = 201341b`，工作区干净；`CARGO_HOME=<repo>/.cargo-home`）：

```bash
# 1) 实现/测试计数（本分支 docs-only，代码 = main）
cargo test -p zgraph-e2e --test dynamic_fanout   # 21 passed / 0 failed（3 次：14.55s / 1.26s / 0.98s）
cargo test --workspace                           # 915 passed / 0 failed / 3 ignored
cargo test -p zgraph-dsl                         # 118 / 0
cargo test -p zgraph-trace                       # 94 / 0（migration 18 + expansions 5 + item_instances 5）
cargo test -p zgraph-kernel                      # 149 / 0；--features test-hooks = 159 / 0
cargo test -p zgraph-cli                         # 222 / 0
cargo test -p zgraph-e2e                         # 106 passed / 0 failed / 2 ignored（2 ignored 为既有 real-LLM manual）
bash scripts/dev/check.sh                        # CHECK PASSED（本次未复现 ISSUE-091 flake）

# 2) AC-17 用例名真实性（脚本比对）
# 07 §AC-17 列出的 21 个用例名 vs tests/e2e/dynamic_fanout.rs 实际函数名：集合完全相等（差集为空）
# 资产存在：tests/fixtures/graphs/foreach_dynamic/**（12 图）、tests/fixtures/trace/schema_v5_seed.sql、
#          tests/fixtures/trace/v5_expected_trace.json、tests/e2e/fixtures/py/foreach_dynamic.py（GENERATION.md §W4b）

# 3) PROVENANCE §3 实测
cd doc/spec-v1.2 && wc -l 0*.md                       # 7198
wc -l 0*.md README.md PROVENANCE.md REFERENCES.md     # 7588
shasum -a 256 0*.md                                   # 9/9 与 §3 表一致（脚本复核 ALL_MATCH）
cd <repo> && git diff --numstat main...HEAD -- 'doc/spec-v1.2/0*.md'   # +76/−32（净+44）
git diff --numstat main...HEAD -- doc/spec-v1.2                        # +106/−50（净+56）
# spec README 行数表 9/9 行数实测一致（00=345/01=447/02=985/03=685/04=940/05=1429/06=1278/07=489/08=600）

# 4) 台账/进度/范围
grep -c '^| ISSUE-14[2-9]' doc/issues.md                 # 8（142 fixed + 143–149 open，8 列）
wc -l doc/progress.md                                    # 15（四块：当前阶段/进行中/下一步/阻塞）
git -c core.quotepath=false diff --name-only main...HEAD # 17 files，全部 doc/**
git diff --numstat main...HEAD -- doc/decisions/DR-004-v02-scope-and-waves.md  # 1/0（纯追加）
git diff --numstat main...HEAD -- doc/decisions/DR-007-w4a-dynamic-fanout-freeze.md  # 22/0（纯追加）
git diff --name-only main...HEAD -- doc/reviews/         # 仅 README.md（既有评审记录未改）
```

- **代码对照**（只读 grep，验证 as-built 主张）：`kernel/run.rs:1665-1681`（unwrap A）、`:799/:830`（Resume 重扫）、`:1519`（source 仅 Collecting）、`:1492-1505`（terminal delivery）、`:1731/:1773`（N=0）、`:1080`（incomplete_join）、`:2828-2835`（冷峻取消保留 Completed）；`trace/schema.rs:422,623-624,651-659`（v6-only 索引/FK 窗口）；`trace/tests/migration.rs:167+`（真实 v1 形状）；`kernel/context.rs:97,102`（item 访问器）。
- **计数/名称/数字全绿**，唯一问题为 N1（trace SHA 溯源）与 N2（08 清单完整性）等文档准确性项。

## 5. 遗留问题

- **N1/N2（P2）**：建议 docs-dev 在合并前加小 commit 修正（替换 trace SHA 为 `93a5f23`/`e057cef`；补 08 §8.11.2 遗漏行），并重算 PROVENANCE §3 中 **06/08** 的 SHA-256（行数不变）；若 Lead 选择先行合并，可登记 ISSUE-150/151 由发布收口批次修正——**不改变本分支结论**。
- **N3–N6（P3）**：建议本批或紧随 commit 处理：reviews 索引补 REV-085/086/087 三行 + index 范围改 `REV-038–094`；DR-004/DR-007/PROVENANCE 执行顺序统一；07 资产图数 11→12；07 通过标准行同步实测口径。
- **发布收口**：合并本分支后 W4b 完整收口；下一步**发布收口**（性能基线、兼容矩阵复核、`tag v0.2.0`；release notes 须列 08 §8.10/§8.11 source-breaking）——需**用户确认** tag/Release。
- 未发现需要返工的问题；无阻塞项（P2 为文档准确性问题，建议随发布说明修正）。

## 6. 条件关闭记录（仅"有条件通过"填写）

不适用（结论为 `通过`，无附加条件）。
