# REV-036 · docs/process · M5 收尾（台账/进度/索引/baseline/README）

> **模块声明（跨模块，作者为 Lead）**：本分支改 `doc/**` 6 个文件 + 根 `README.md`（**process** 模块），无代码改动。作者 `lead`，评审人 `reviewer-b`（**非作者**）。评审**未修改分支任何文件**，只新增本记录；未 merge / 未 push / 未改 main。根 README 为 process 模块，按 AGENTS §2 由本记录声明为 Lead 授权的 docs+process 跨模块文档收尾。
>
> **评审口径**：独立读 7 文件全量 diff、逐条对照 main 事实与 REV-026/027/028/029/030/031/032/033/034/035/038 结论及 DR-002 §7/§9；用全新隔离 target 实跑 `cargo test --workspace`（552 passed / 0 failed / 2 ignored）与 `pytest`（84 passed）佐证 test-baseline；用脚本核对 issues 编号/列格式、spec 00–08 行数（5958）、评审索引链接、M6 清单集合；`check.sh` PASS、worktree clean、diff 仅 7 文件。

- **分支**：`docs/process-m5-wrapup`
- **作者**：`lead`
- **评审人**：`reviewer-b`
- **变更范围**：`a21c5234e755b59159d67e65ec820dd10634aded..f431367f0e59520745369bd8b05b35d5f19a16e5`（1 commit `f431367 docs(process): M5 收尾——台账/进度/评审索引/baseline/README（REV-036）`；`git diff --shortstat` = **7 files changed, 39 insertions(+), 32 deletions(-)**；base = `main` = `a21c523`）
- **评审时间**：2026-10-30
- **关联**：REV-035 §8.6（残留 1/2/3：时点语句、台账同步、Nit）；REV-030 #2–#5（崩溃标定硬化）；REV-032 #1（kernel `fail_timeout`）；DR-002 §7（M6 排除）/§9 #9/#10/#12；AGENTS §3/§5/§6/§7；ISSUE-048/066/076/081/082/084/085–089

## 1. 结论

**结论：有条件通过**

本分支把 M5 收尾文档基本落到位：`doc/progress.md` 24 行、四块结构完整；`doc/issues.md` 89 条编号连续无重号、8 列格式一致（脚本核验），048/066/076/084 转 fixed、081/082 保持部分修的说明与既有评审结论基本相符，新增 ISSUE-086–089 内容与 REV-032/REV-030 发现逐条对应；`doc/reviews/README.md` 补 REV-035/036 行与 M5 段落；`doc/index.md` 更新 reviews 范围；`doc/test-baseline.txt` **仅新增 1 行 # 注释、未扩大失败名额**（0 failed / 2 ignored，fresh workspace 实测 552 passed/0 failed/2 ignored、pytest 84）；根 `README.md` M5 完成态与主要交付清单准确；`PROVENANCE.md` §2 第 11 条复审完成态更新。`check.sh` PASS、diff 仅 7 文件、worktree clean。

但发现 **2 项需要先关闭的事实/状态一致性条件**（§3 C1/C2），当前不能判为「通过」：

- **C1（阻塞）ISSUE-084 状态跨文档矛盾**：本分支 `doc/issues.md:92` 将 ISSUE-084 置 **fixed**（残余硬化转 ISSUE-088），`doc/progress.md:13`/`README.md:24` 也按已修叙述；但 main 上 REV-035 回写的 **`doc/spec-v1.2/06-存储与可观测.md:688`、`07-验收用例.md:231`、`08-审计关闭矩阵.md:428` 与 `doc/decisions/DR-002...:142`（§9 #10）仍明确写「ISSUE-084 保持 open，留 M6 加固」。同一事实两套状态，违反交叉一致要求；且 REV-035 §8.6 #2 已明确要求 REV-036「按实现事实转 fixed/open 并统一 README/05/08 措辞」，本分支未触及 spec/DR-002。
- **C2（阻塞/一致性）M6 清单与编号表述不准确**：`doc/progress.md:5` 写「REV-025–038」但 REV-037 从未创建（已顺延 M6，reviews README 自己也写明）；`doc/progress.md:19` 与 `README.md:26` 把 ISSUE-086–089（本批新登记）、075/077/079/080（DR-002 §4）、085（REV-028 建议）统称「DR-002 §7/§9 排除/顺延项」——DR-002 §7 实际只有 033/036/073/059，§9 #10/#12 为 084/081，其余并非 §7/§9 条目；另外 `progress.md:20` 列了 ISSUE-068/078（process 依赖治理）而 `README.md:26` M6 清单未列（只写 REV-037 治理），两处 M6 清单不一致。属事实/口径错误，需与 C1 同批修订。

两者均为**文档事实/一致性**问题，不涉及代码；修订后可增量复审转「通过」。其余建议/Nit 见 §3（不阻塞）。

## 2. 四维评审

| 维度 | 结论 | 依据 |
|---|---|---|
| 功能正确性 | ⚠️ | 大部分事实正确：issues 编号/格式、048/066/076/081/082 状态与既有评审结论一致；test-baseline 仅 +注释、fresh workspace 552/0/2 与 pytest 84 实测吻合；README/index/reviews 主要交付清单与 main 合并历史一致。**扣分**：ISSUE-084 在 issues/README/progress 与 spec 06/07/08/DR-002 两套状态（C1）；M6 清单来源/REV 编号表述不准确（C2）。 |
| 代码结构合理性 | ✅ | 收尾只改文档与根 README（process），无代码/依赖/脚本改动；README 为 process 模块跨文档变更，Lead 已声明；`doc/**` 修改集中于台账/进度/索引/baseline/PROVENANCE，无无关重写；新增 ISSUE 追加在表尾，未动历史行。 |
| 逻辑性 | ⚠️ | 更新逻辑总体自洽（REV-035 残留时点语句已改为完成态；issues 状态与新增 ISSUE-086–089 对应 REV-030/032 发现；baseline 仅注释）。**扣分**：跨文档状态未同步（C1）；M6 清单在 progress/README 与 DR-002 §7/§9 之间口径不一致（C2）。 |
| 可读性 | ✅ | progress 24 行、四块结构简洁；issues 行文与既有格式一致；README 状态段清晰、M6 列表可读；reviews/index 表格格式正确。仅 1 条可选 Nit（spec 05 L8 旧数字未精确化，REV-035 §8.6 #3）。 |

## 3. 发现清单

| # | 级别 | 位置 | 问题 | 建议 | 状态 |
|---|---|---|---|---|---|
| 1 | **阻塞（C1）** | `doc/issues.md:92`（084 fixed）vs `doc/spec-v1.2/06-存储与可观测.md:688`、`doc/spec-v1.2/07-验收用例.md:231`、`doc/spec-v1.2/08-审计关闭矩阵.md:428`、`doc/decisions/DR-002-m5-cli-replay-recording.md:142`（均写 084 保持 open/M6） | ISSUE-084 状态跨文档矛盾：issues/README/progress 按 **fixed（核心实现完成，残余硬化→ISSUE-088）**，但 REV-035 回写的 spec 06/07/08 与 DR-002 §9 #10 仍按 **open/M6 加固**；REV-035 §8.6 #2 已把「统一 README/05/08 措辞」交给 REV-036。 | 二选一并全仓一致：①（推荐）保留 084 fixed：把 spec 06 §6.8.1 注、07 §7.3 #6、08 §8.7 行、DR-002 §9 #10 改为「核心实现完成 → fixed；多机/EPERM/host 硬化转 ISSUE-088（M6，open）」；或②把 issues 084 改回 open/partial 并在 M6 清单补 084（但会与 088 重复）。修订后本评审增量复审关闭。 | open |
| 2 | **阻塞（C2）** | `doc/progress.md:5`；`doc/progress.md:19`、`README.md:26`；`doc/progress.md:20` vs `README.md:26` | ① `REV-025–038` 包含从未创建的 REV-037（reviews README 已注「REV-037 顺延 M6」）；② M6 清单把 086–089（本批新登记）、075/077/079/080（DR-002 §4）、085（REV-028）统称「DR-002 §7/§9 排除/顺延项」，与 DR-002 §7/§9 实际条目不符；③ progress 列 ISSUE-068/078（process 依赖治理）而 README M6 清单未列，两处 M6 集合不一致。 | ① 改为「REV-025–036/038（REV-037 顺延 M6）」；② 改为分类表述：「M6：DR-002 §7/§9 顺延（033/036/059/073/081/084→088 等）+ DR-002 §4 顺延（075/077/079/080）+ REV-028/030/032 新登记（085/086/087/088/089）」；③ README M6 补 ISSUE-068/078（或明确「见 REV-037 依赖治理」并保持与 progress 一致）。 | open |
| 3 | 建议（Nit） | `doc/spec-v1.2/05-AgentHarness.md:1407`（L8 表「74/78/88 等」） | REV-035 §8.6 #3 指出 74 为 REV-028 期旧值（当前 `tests/run.rs:625` 为 68 行）；本分支未精确化。因有「等」且 91/build_plan 75 已正确，不阻塞 | 可与 C1/C2 同批将 74 改为 68/78/88（或保留「等」并注明当前值）。 | open |
| 4 | 建议/Nit | `doc/reviews/README.md:43`、`doc/index.md`、`README.md:25` | REV-036 索引行、`doc/index.md`「已全部合入 main」、README「本批已合入」在评审/合并前即为完成态；本评审为有条件通过时这些断言暂不成立（合并/关闭条件后成立） | C1/C2 修订并最终通过后保留即可；若需严格时点，可在合并前把 REV-036 行注为「有条件通过（待修订）」，合并后更新为通过。不阻塞（属最终态文档惯例）。 | open |
| 5 | 备注（已核对） | `doc/test-baseline.txt`；fresh target | 仅新增 1 行注释、无失败条目；fresh `/tmp/rev036-target` `cargo test --workspace` **552 passed / 0 failed / 2 ignored**（2 ignored = harness 真模型 smoke + e2e AC-08b），`pytest` **84 passed**；与 M5 行「0 failed / 2 ignored、pytest 84」一致，基线名额仍 0 | 无需动作（dry-run 结果待条件关闭后复跑记录） | 无需处理 |
| 6 | 备注（已核对） | `doc/issues.md` | 89 条 ISSUE 编号连续 1–89、无重号；每行 8 列；新增 086（kernel fail_timeout 实例错误码，REV-032 #1）、087（L2 global_vars 未记录，REV-030 边界）、088（崩溃标定 hardening，REV-030 #2–#5）、089（REV-032 观察项）与评审发现逐条对应；048/066/076/084 fixed、081/082 部分修与 REV-030/031/032/034/035 结论不冲突（除 C1 的 spec/DR 未同步） | C1 修订后 084 表述全仓一致即可 | open（随 C1） |
| 7 | 备注（边界） | `doc/reviews/README.md:42-44,53`；`doc/index.md` | REV-035/036 行、M5 段落与 main 已合并事实一致；REV-036 行在本次评审后才会生成对应文件（本记录提交后链接成立）；REV-037 未创建已在 reviews README 如实注明，符合「不虚增评审」 | 无阻塞；合并后按实际状态复核链接即可 | 无需处理 |

> 阻塞项即 C1/C2（均为文档事实/一致性）；#3–#7 为建议/Nit/已核对备注。修订 C1/C2 后本评审做增量复审并把结论改为「通过」。

## 4. 验证证据

### 4.1 范围/元数据

```bash
cd /Users/zyzs/lczyzs/code/zgraph/.worktrees/process-lead
git branch --show-current   # docs/process-m5-wrapup
git rev-parse HEAD          # f431367f0e59520745369bd8b05b35d5f19a16e5
git merge-base main HEAD    # a21c5234e755b59159d67e65ec820dd10634aded（= main）
git rev-list --left-right --count main...HEAD   # 0 1
git diff --shortstat main...HEAD               # 7 files changed, 39 insertions(+), 32 deletions(-)
git diff --name-status main...HEAD
# M README.md；M doc/{index.md,issues.md,progress.md,reviews/README.md,spec-v1.2/PROVENANCE.md,test-baseline.txt}
git log -1 --format='%an <%ae> / %cn <%ce> / %s%n%b'   # chenyuxiangg / lead；正文含 Review: REV-036 / Agent: lead
git status --short          # 空（评审时 clean）
```

### 4.2 测试/门禁（全新隔离 target `/tmp/rev036-target`）

| 命令 | 结果 |
|---|---|
| `cargo test --workspace -- --test-threads=2` | **552 passed / 0 failed / 2 ignored**（`/tmp/rev036-ws.log`；2 ignored = harness 真模型 smoke + e2e AC-08b 手动） |
| `pytest --tb=no`（venv） | **84 passed** |
| `bash scripts/dev/check.sh` | **CHECK PASSED**（身份/remote/CLAUDE.md/禁用文件/分支名/Python 语法/progress 19 非空行≤30/fmt/diff-check） |
| `git status --short` / `git diff --check` | 空 / exit 0 |
| merge dry-run | **待 C1/C2 修订后执行**（当前结论为有条件通过，merge.sh 记录门禁要求「通过」；不伪造 dry-run 结果） |

### 4.3 文档事实核对（脚本/抽样）

| 项 | 结果 |
|---|---|
| spec 00–08 行数 | `wc -l` 合计 **5958**，与 `doc/progress.md:15` 一致；PROVENANCE §3 各文件行数合计 5958（历史批次 5818/5938/5949/5956 为分批复测留痕，不覆盖） |
| ISSUE 编号/格式 | 89 条、1–89 连续、无重号、8 列全一致；086–089 存在 |
| reviews 索引链接 | 39 个链接中仅 `REV-036-docs-m5-wrapup.md` 在评审前暂缺（本记录提交后成立）；其余存在；REV 行数 38，与 main 已合并记录 + REV-036 计划一致 |
| M6 清单 | progress:19 与 README:26 的枚举集合一致；差异仅在 ISSUE-068/078（progress:20 列、README 仅写 REV-037 治理）与来源归属表述（C2） |
| 状态一致性 | 048/066/076/081/082 fixed/partial 与 REV-030/031/032/034/035 结论一致；**084 与 spec/DR 冲突（C1）** |

## 5. 遗留问题

- **C1/C2 关闭前不得合并**（见 §6）；修订由 Lead 在本分支追加 commit（可改 spec 06/07/08、DR-002 §9 #10、progress/README），本评审随后做增量复审。
- C1 修订时注意不要删除 REV-035 历史记录中的「首轮有条件通过」表述（历史留痕）；REV-035 §8.6 #2 的「统一 README/05/08」以最终态为准。
- C2 修订后建议同步 `doc/reviews/README.md` 的 M5 段落与 `doc/index.md` 的 REV 范围（REV-037 未创建/顺延 M6 的表述已在 reviews README 注明，进度/README 与之对齐）。
- merge dry-run（fresh target `/tmp/rev036-target`；`cargo clean -p` 工作区成员；fetch 失败重试）待条件关闭、结论改「通过」后执行；「dry-run 必须全门禁通过」为合并前置。
- 本评审未修改任何产品代码/文档；本记录为本分支唯一新增文件（评审对象 `f431367` + 本记录提交）。

## 6. 条件关闭记录（有条件通过）

| 条件 | 内容 | 关闭证据 | 关闭日期 |
|---|---|---|---|
| **C1** | ISSUE-084 状态 / 残余硬化归属在 issues、README、progress 与 spec 06/07/08、DR-002 §9 #10 全仓一致（推荐：084 fixed；硬化→ISSUE-088 M6） | 修订 commit + `grep ISSUE-084` 各文档结果；本评审增量复审 | 待关闭 |
| **C2** | M6 清单/REV 编号表述准确且 progress/README 一致：`REV-025–036/038`（REV-037 顺延）、M6 来源分类、ISSUE-068/078 在 README 与 progress 对齐 | 修订 commit + 本评审增量复审 | 待关闭 |
