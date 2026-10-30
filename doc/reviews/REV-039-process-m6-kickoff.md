# REV-039 · process/docs · M6 kickoff：DR-003 范围/波次 + 进度/索引/README

> **模块声明（跨模块）**：本分支改 `doc/decisions/DR-003-m6-scope-and-waves.md`、`doc/progress.md`、`doc/index.md`（`docs` 模块）与根 `README.md`（**`process` 模块**，作者 `lead` 亦为 process 负责人）；按 AGENTS §2 属**跨模块**改动，已在 DR-003 文首声明。本记录由**非作者**评审人 `reviewer-a` 作独立第二评审并在本段声明，符合 AGENTS §2/§3。评审人**未修改任何被评审内容**（DR-003/README/progress/index 均未改），只新增本记录；未 merge / 未 push / 未改 main。
>
> **评审对象**：`docs/process-m6-kickoff` @ `55f0218`（作者 `lead`，1 commit）。本记录提交后分支变为 2 commit（+1 评审记录），评审对象仍是 `55f0218` 的内容。
>
> **增量复审**：`39916d0..e03ed14`（作者 `lead`，1 commit：DR-003 + progress 修订；提交正文含 `Review: REV-039` / `Agent: lead`）。首轮条件 **C1–C4 全部关闭**、建议 #5–#12 已落实或转后续维护，另记 2 条非阻塞 Nit（#14/#15）；结论更新为「通过」，关闭证据见 §4.6/§6。

- **分支**：`docs/process-m6-kickoff`
- **作者**：`lead`
- **评审人**：`reviewer-a`（**非作者**）
- **变更范围**：`origin/main`（`a031269`）..`55f0218`，**1 commit / 4 files changed, 122 insertions(+), 19 deletions(-)**：`A doc/decisions/DR-003-m6-scope-and-waves.md`、`M doc/progress.md`、`M doc/index.md`、`M README.md`；无代码/测试/CI/`schemas/`/`doc/spec-v1.2/` 正文/`CLAUDE.md` 改动，分支基于最新 `origin/main`（`git rev-list --count origin/main..HEAD` = 1）。
- **评审时间**：2026-10-30
- **关联**：AGENTS §2/§3/§4/§5/§7；`doc/decisions/DR-002-m5-cli-replay-recording.md` §7/§9；`doc/issues.md` ISSUE-033/036/059/068/073/075/077/078/079/080/081/082/085/086/087/088/089；spec 02 §2.7.4、05 §5.2.1/§5.6.2/§5.8.5/§5.13.1、06 §6.4.5/§6.7/§6.8/§6.10.2/§6.10.5、07 AC-12/§7.2/§7.3、08 §8.7；`schemas/worker-protocol-v1.md` §hello；`doc/architecture-v2.md` §2.2/§5.2/§6.1/§8 D4

## 1. 结论

**结论：通过**（增量复审：首轮为「有条件通过」，C1–C4 已由 `e03ed14` 关闭、证据见 §4.6/§6；首轮条件原文保留，不删除）

范围（§1 的 17 个 ISSUE 全集）、REV 编号（REV-037 从未创建、REV-038 已用、M6 自 REV-039 连续）、波次/分支命名、progress/index/README 自洽性与链接有效性均**核对通过**（见 §4）；`bash scripts/dev/check.sh` PASS、worktree 干净（`e03ed14` 后复查仍 PASS/干净）。**首轮**发现 4 项重要问题（其中 2 项为与 spec/验收口径的矛盾，2 项为本文件内部自相矛盾），现已**全部关闭**；增量复审另记 2 条非阻塞 Nit（#14/#15）。首轮条件原文（历史保留）：

- **C1（重要，spec 矛盾）**：`DR-003:63` D1 把 `validate` 的「图/校验失败」定为退出码 **1**，与 06 §6.10.5 权威表（`06:904-905`：`1` = IO/内部、`2` = `DSL_*` 图/资产/用法）、06 §6.10.2（`06:786-793`）及 02 §2.7.4（`02:722`：`2` = DSL 错误、`1` = 其他）**相反**；D1 自称「以 06 §6.10.5 权威表为准」却写错该行，REV-043 若照此实现会把 AC-04 的坏图（`DSL_*`）判成 1。
- **C2（重要，内部不一致）**：§1 与 §2 对同一分支的波次标注不一致——ISSUE-088 `DR-003:30` 标 **W2 / REV-055**，而 §2 `DR-003:55` 把 REV-055 排在**波次 3**；REV-045（068/078/073）§1 标 **W1**（`DR-003:18,19,22`）、§2 标 **1b**（`DR-003:45`）。
- **C3（重要，验收口径低于 spec）**：`DR-003:73` D11 与 `DR-003:80`（M6 DoD #2）把 AC-12 的防 flaky 门槛定为「至少/连续 **2 次**」，而 07 §7.2 通过标准（`07:6`）与 §7.3 #3（`07:228`）要求「**10 次**重跑无 flaky」；DR-003 文首（`DR-003:9`）又声明「spec 正文优先、不改变 v1.0 契约」，属未记录的口径下调。
- **C4（重要，DoD 自相矛盾）**：`DR-003:79`（DoD #1）要求「§1 表内 17 个 ISSUE 全部 `fixed`（ISSUE-073 按 D2 收口）」，但 `DR-003:64` D2 明确 ISSUE-073 「**wontfix**，理由入台账」——`fixed` 与 `wontfix` 不能同时作为同一 ISSUE 的验收状态。

其余为**建议项**（§3 #5–#12），不阻塞：D13 跨模块清单口径、§1「全集」缺 DR-002 §9 #11、D12 排除清单含已 fixed 的 067 且未列其他 open 项、D10 与 05 §5.6.2.1 现状重复、D16 引用错误、§6 来源归属、progress 波次状态、D4 与 039 排除口径。

> **增量复审结论（2026-10-30，对象 `e03ed14`）**：C1–C4 **全部关闭**（逐项证据见 §6）；#5–#12 已落实（#9 的台账文本同步、#11 的波次实时状态为后续维护项）。复查未发现新阻塞问题，新增 2 条非阻塞 Nit：**#14** §1 新增行括注「`--version` 已符合」与 §6（`--version` 偏差由 REV-043 承接）、DR-002 §9 #11 及 spec 06 §6.10.1 目标格式不符；**#15** ISSUE-068/073/078 行波次仍标 `W1` 而 §2 为 `1b`（按“1b = 波次 1 子波”解释可接受，属粒度标注）。两条均不影响范围/验收/编号，随 REV-043/REV-056/REV-057 或台账刷新即可（见 §3/#14/#15、§5）。

## 2. 四维评审

| 维度 | 结论 | 依据（文件/行/命令） |
|---|---|---|
| 功能正确性 | ✅ | ① 范围核对通过：`DR-003` §1 的 17 个 ISSUE 与 `doc/issues.md` 逐行存在、级别一致，且与 main `a031269` 的 `doc/progress.md` M6 清单**集合完全相同**（17 个去重后无增无漏，脚本核对见 §4.2）；无捏造 ISSUE。② REV 编号核对通过：`doc/reviews/` 无 REV-037、最新为 REV-038，REV-039 起连续分配且无重号（§4.2）。③ 首轮扣分项已消除（`e03ed14`）：C1 退出码改为 0/2/1、C3 收口 10 次、C4 收口口径统一；残余 #14 为 `--version` 括注表述 Nit（不影响 REV-043 依 DR-002 §9 #11 执行）。 |
| 代码结构合理性 | ✅ | 纯文档分支：新增决策只落 `doc/decisions/DR-003-*.md`（与 DR-001/DR-002 同级），索引/进度/README 各改其位；未触碰产品代码、测试、CI、schema、spec 正文与 `CLAUDE.md`（`git diff --name-status` 4 文件；增量 `39916d0..e03ed14` 仍仅 2 个 doc 文件，禁用文件 grep 为空）。跨模块（`doc/**` + 根 `README.md`）已在 DR-003 文首与本记录声明，Lead 为 process 负责人、reviewer-a 为非作者第二评审，符合 AGENTS §2。 |
| 逻辑性 | ✅ | ① 波次表与 §1 逐条可映射（REV-039–057 共 19 个编号，无重复/断号），与 `doc/progress.md` 波次 0/1b 行一致；首轮 C2 已修正（088→W3；REV-037 行→W1b），残余 #15 为 `1b` 子波粒度的标注差异（不矛盾）。② DoD #1 与 D2 状态口径已统一（C4 关闭）。③ D12 已删已 fixed 的 067 并补「只列 M6 相关排除项」说明（#7 关闭）。④ §1 ISSUE-079 已改为 architecture §8 **D4** / 05 §5.13.1 D13（#9 关闭）。⑤ D13 已定义 `+` / `(+x)` 并并入双评审（#5 关闭）。 |
| 可读性 | ✅ | DR-003 分节清晰（范围→波次→关键决策→DoD→环境风险→来源），每行给 ISSUE/级别/模块/内容/波次-REV，关键值（8192/1024、v2→v3、10 次收口/2 次开发门禁、17 个）可扫读；progress 四块齐全、23 行（非空 18）≤30；`doc/index.md:7,18` 与 README 状态段表述一致、链接全部可达（§4.4）。首轮 Nit（#8 §6 来源归属）已随 `e03ed14` 修正；增量后仅余 #14/#15 两条非阻塞 Nit，不影响可读性。 |

## 3. 发现清单

| # | 级别 | 位置 | 问题 | 建议 | 状态 |
|---|---|---|---|---|---|
| 1 | **重要（C1）** | `doc/decisions/DR-003-m6-scope-and-waves.md:63`（对照 `doc/spec-v1.2/06-存储与可观测.md:904-905`、`06:786-793`、`doc/spec-v1.2/02-DSL与校验.md:722`） | D1 写「`validate` 用法错误 = 2、**图/校验失败 = 1**、`--help` = 0」，与权威表相反：`DSL_*` 图/资产错误 = **2**、IO/内部 = **1**；D1 自称以此表为准，REV-043 照做会与 AC-04（坏图断言错误码、退出码非 0 且 M1 既有 `2` 口径）冲突 | 改为「`validate`：0 = 合法；**2 = DSL/资产错误与用法错误**；1 = IO/内部；`--help` = 0」；「02 §2.7.4 的 3 为 M1 临时口径」表述保留 | fixed（`e03ed14` 关闭，见 §6） |
| 2 | **重要（C2）** | `DR-003:30` vs `DR-003:55`；`DR-003:18,19,22` vs `DR-003:45` | §1 与 §2 的波次标注不一致：ISSUE-088 §1 为 W2、§2 的 REV-055 在波次 3；REV-045 在 §1 为 W1、§2 为 1b | 统一：ISSUE-088 改 W3（或在 §1 注「波次 3」），REV-045 的 068/078/073 改「W1b」；保证 §1 与 §2 可机检一致 | fixed（`e03ed14` 关闭，见 §6） |
| 3 | **重要（C3）** | `DR-003:73`、`DR-003:80`（对照 `doc/spec-v1.2/07-验收用例.md:6,228`） | D11/DoD #2 的防 flaky 门槛「至少/连续 2 次」低于 07 §7.2/§7.3 #3 的「10 次重跑无 flaky」，且未作为偏差记录；与 DR-003:9「spec 正文优先」矛盾 | 二选一：① 按 07 执行 10 次；② 保留 2 次但在 DR-003 §4/§5 明示「M6 实测口径，对 07 §7.2/§7.3 #3 的偏差，由 REV-056 回写 07」并说明耗时理由 | fixed（`e03ed14` 关闭，见 §6） |
| 4 | **重要（C4）** | `DR-003:79` vs `DR-003:64`（对照 `doc/issues.md:81`） | DoD #1 要求 17 个 ISSUE 全部 `fixed`，D2 又规定 ISSUE-073 以 `wontfix` 收口（台账状态域 `fixed/wontfix` 互斥） | DoD #1 改为「17 个 ISSUE 全部**收口**（`fixed`；ISSUE-073 按 D2 记 `wontfix` 并写明理由）」或在 D2 处统一为 fixed+guard 方案并同步台账状态域 | fixed（`e03ed14` 关闭，见 §6） |
| 5 | 建议 | `DR-003:23,30,75` | D13 只列「跨模块分支（036/047/048/082）」，而 §1 对 079 标 `harness(+cli)`、对 088 标 `cli(+trace)`；`(+)` 与 `+` 的语义差异未定义，也未说明是否需第二评审（AGENTS §2 MUST） | 在 D13/§1 注「`+` = 同分支多模块改动须双评审；`(+x)` = 仅在必需时触及 x，若触及同样声明第二评审」，或把 051/055 纳入 D13 清单 | fixed（`e03ed14` 已落实） |
| 6 | 建议 | `DR-003:11`（对照 `doc/decisions/DR-002-m5-cli-replay-recording.md:143`、`doc/progress.md:13`） | §1 标题称「= M5 顺延项全集」，但 DR-002 §9 #11（REV-028 #6：clap/`-v`/`--version`/`NO_COLOR`「随 M6」）未入 §1/DoD，只在 §2 的 REV-043 行与 progress 行出现 | §1 增一行（REV-043 承接）或在 §4 DoD 注明 REV-043 的附加范围，避免「全集」口径漏项 | fixed（`e03ed14` 已落实） |
| 7 | 建议 | `DR-003:74`（对照 `doc/issues.md:75` 及 003/005/006/009/011/020） | D12「M6 排除」：(a) 列了已 `fixed` 的 ISSUE-067（spec 04 剩余回写，REV-023 已修）；(b) 未列台账仍 open/in-progress 的 003/005/006/009/011/020 等，易被读成“M6 只排除这些” | 删 067 或改注「已 fixed」；补一句「本表只列 M6 相关排除项，其余 state=open/in-progress 见台账」，避免全集误读 | fixed（`e03ed14` 已落实） |
| 8 | 建议 | `DR-003:24,72`（对照 `doc/issues.md:88`、`doc/spec-v1.2/05-AgentHarness.md:935`） | ISSUE-080 台账模块为 `harness/docs`，DR-003 标 `docs`；D10「把实际闭集文档化到 05 §5.6.2」在 05 §5.6.2.1（`05:935`，REV-023）已完成，剩余实为「复核是否收敛到 05/00 闭集」 | 模块列改 `harness/docs`（或注明 docs 主责）；D10 改为「复核并冻结 05 §5.2.7 / 00 §0.4 的 ConfigLoad reason 闭集，05 §5.6.2.1 现状为准」 | fixed（`e03ed14` 已落实） |
| 9 | 建议 | `DR-003:23`（对照 `doc/spec-v1.2/05-AgentHarness.md:264,448,1374`、`doc/issues.md:87`） | §1 ISSUE-079 的「（D16…）」引用错误：D16 = 重试编排在 Harness；工具/worker 并发上限是 architecture §8 **D4** / 05 §5.13.1 D13 | 改为「（D4；`HarnessCtx` 无 run_id）」；台账 ISSUE-079 同错建议一并修正 | fixed（`e03ed14` 已落实） |
| 10 | 建议 | `DR-003:99`（对照 `doc/decisions/DR-002-m5-cli-replay-recording.md:4,16,113-121`） | §6 把 ISSUE-082 的来源归入「DR-002 §4/§9」，但 DR-002 §4/§9 正文无 082（仅头部关联与 §1 表#3）；082 的“版本列仍 NULL”结论来自台账与 REV-028/030 | 改为「ISSUE-082 部分修（台账 `issues.md:90`；DR-002 §1#3/§3.2）」 | fixed（`e03ed14` 已落实） |
| 11 | 建议 | `doc/progress.md:10-15` | 波次 1 五行均标 `wip`，但评审时点对应分支尚无提交（`git rev-list --count origin/main..<branch>` = 0）；REV-045 的 `chore/process-m6-deps` 标 `next` 却已有 1 提交（`4096853`） | 按实际开工状态更新（`next`/`wip`）——属实时状态维护，合并前后由 Lead 刷新即可 | fixed（`e03ed14` 已落实） |
| 12 | 建议 | `DR-003:66` vs `DR-003:74`（ISSUE-039 `issues.md:47`） | D4 引入 schema v3 迁移（“旧库迁移必须可回滚/报错不损坏数据”），而 D12 把 ISSUE-039「迁移前无 `VACUUM INTO` 备份/DB 权限」列为 M6 排除；v2→v3 迁移实际会触碰备份缺口，排除与实际改动口径不一致 | 在 D4/REV-048 明确「v3 迁移是否顺带补 `VACUUM INTO` 备份」；若补则把 039 的备份子项移入 M6 或注明“REV-048 顺带关闭” | fixed（`e03ed14` 已落实） |
| 13 | 备注（流程） | `doc/reviews/README.md` 索引 | 本记录（REV-039）合并后索引尚缺行（现只到 REV-038）；DR-003 §4 #5 已把「评审索引」列入 REV-057 wrapup | 无需本分支处理；REV-057 补索引行与 M6 段落即可（与 M5 的 REV-025 同样由后续 docs 批次补录） | 无需处理 |
| 14 | Nit（增量复审新增） | `DR-003:32` vs `DR-003:102`（对照 `doc/spec-v1.2/06-存储与可观测.md:780`、`crates/zgraph-cli/src/lib.rs:59,224`） | §1 新增行的括注「（`--version` 已符合）」与 §6「`--version` 偏差（…）随 M6，由 REV-043 承接」、DR-002 §9 #11 及 spec 06 §6.10.1 目标格式（`zgraph <pkg> (spec v1.2, schema 1)`）不一致；当前实现输出 `zgraph 0.1.0 (DSL 1.0)` | 删除括注或改为「`--version` 格式待统一（REV-043）」；REV-028 #6 已由 DoD #1/§6 明确由 REV-043 关闭，故不阻塞 REV-039 | open（非阻塞 Nit，随 REV-043/REV-056 或台账刷新落实） |
| 15 | Nit（增量复审新增） | `DR-003:18,19,22` vs `DR-003:33,45` | ISSUE-068/073/078 行波次仍为 `W1 / REV-045`，而 §2 与 REV-037 行为 `1b`；按「1b = 波次 1 子波」解释不构成矛盾，但机检会读到两个标签 | 后续修订/REV-057 统一为 `W1b`，或在 §2 波次列注明「1b ⊆ W1」 | open（非阻塞 Nit） |

> **条件/状态（增量复审后）**：C1–C4 已由 `e03ed14` **全部关闭**（逐项证据见 §6），首轮 #1–#4 状态改为 fixed；建议项 #5–#12 已落实（残余维护项见 §5）。#14/#15 为新增非阻塞 Nit，不改变「通过」结论。

## 4. 验证证据

### 4.1 范围、分支与门禁（worktree `.worktrees/process-lead`，实际执行）

```bash
$ git rev-parse --abbrev-ref HEAD          # docs/process-m6-kickoff
$ git status --porcelain                   # 空（评审时 worktree clean）
$ git rev-list --count origin/main..HEAD   # 1（基于最新 origin/main a031269）
$ git merge-base --is-ancestor origin/main HEAD && echo YES   # YES
$ git log -1 --format='%H %an <%ae> | %s'
# 55f0218 chenyuxiangg <1007423165@qq.com> | docs(process): M6 kickoff——DR-003 范围/波次/REV 与进度/索引/README 更新
$ git log -1 --format='%B'
# 正文：Review: REV-039 / Agent: lead（符合 AGENTS §1）
$ git diff origin/main...HEAD --name-status
# M README.md
# A doc/decisions/DR-003-m6-scope-and-waves.md
# M doc/index.md
# M doc/progress.md
$ git diff origin/main...HEAD --stat
# 4 files changed, 122 insertions(+), 19 deletions(-)
$ git diff origin/main...HEAD --name-only | grep -E 'CLAUDE\.md|crates/|tests/|scripts/|schemas/|\.venv|__pycache__|\.db' || echo none
# none（无代码/测试/CI/schema/禁用文件；无 doc/spec-v1.2 正文改动）
$ bash scripts/dev/check.sh
# OK user.name/email/remote；OK CLAUDE.md 未修改；OK 暂存区无禁用文件
# OK 分支命名: docs/process-m6-kickoff；OK Python 语法；OK progress 非空行 18（目标 ≤30）
# OK cargo fmt --check；OK git diff --check；CHECK PASSED（exit 0）
```

文档分支无代码/单测改动，未跑全量 `cargo test`/`pytest`（由 `merge.sh` 合并门禁执行）；本轮验证覆盖 `check.sh`、链接扫描、台账/编号机检与 spec 对照。

### 4.2 必须核查项 1–2：范围与编号（脚本核对）

```text
§1 ISSUE 提取（按出现顺序）：033 036 059 068 073 075 077 078 079 080 081 082 085 086 087 088 089
去重数 = 17；行数 = 17；与 issues.md 逐行核对：全部存在、P 级别一致、内容与台账“问题/建议”一致，无捏造。
与 main a031269 的 progress.md M6 清单去重集合：{033,036,059,068,073,075,077,078,079,080,081,082,085,086,087,088,089}
→ 与 §1 完全相同（无遗漏/无新增）。M5 progress 的 REV-037 顺延、AC-12 加强亦在 DR-003 §1/§2 有对应行。

REV 编号：doc/reviews/ 现有 REV-000…REV-036、REV-038（无 REV-037，符合“从未创建”）；无 REV-039+ 文件（评审前）。
DR-003 §2 预留 REV-039…REV-057 共 19 个：039(kickoff) 040–044(W1) 045(1b) 046–053(W2) 054–057(W3)，无重复/断号；
与 progress.md 波次 0/1 行一一对应（039/040/041/042/043/044/045）；REV-037 未被启用（DR-003:59,101）。
```

REV 编号结论：**无冲突**；`REV-039` 由本记录占用，后续按 DR-003 预留号执行。

### 4.3 必须核查项 3：关键决策 ↔ spec/协议（抽查）

| 决策 | 依据 | 结论 |
|---|---|---|
| D1 ISSUE-059 退出码 | 06 §6.10.5（`06:904-905`）、06 §6.10.2（`06:786-793`）、02 §2.7.4（`02:722`） | ❌ 矛盾（C1）：图/校验失败应为 2，D1 写 1 |
| D2 genai 0.7.2 接受 + 守卫 | 台账 ISSUE-073（`issues.md:81`）、REV-021 装箱 | ✅ 与台账建议一致（但 DoD 状态口径见 C4） |
| D3 hello 版本列 / model 参数列 | worker 协议 v1 `hello` 字段（`schemas/worker-protocol-v1.md:17`：`python_version`/`sdk_version`）；06 §6.2.4 `worker_versions_json`/`model_params_json`（“worker/SDK/Python 版本”“模型参数快照”） | ✅ 字段可得、口径一致（不含密钥） |
| D4 schema v3 `global_vars` | 实现 `TRACE_SCHEMA_VERSION = 2`（`crates/zgraph-trace/src/schema.rs:18`），故 2→3 成立；06 §6.7 forward-only + 备份/单事务 | ✅ `2→3` 与代码一致；迁移备份与 ISSUE-039 的交叉见 #12 |
| D5 fail-stop | 06 §6.4.5（`06:542-550`）：目标语义 ①stderr `STORAGE_ERROR` ②取消 run ③尽力写终态 ④exit 4；`STORAGE_ERROR` 不写 `error_code` | ✅ 与 spec 目标语义一致；`take_error` 轮询属实现方式，不违反 §6.4.2 |
| D6 human 实时生命周期 | 06 §6.10.3（`06:826-828`“human 模式 stderr 每行一条生命周期事件”） | ✅ 与 spec 字面一致；内存旁路不改变 stdout/`--json` 契约 |
| D8 崩溃标定多机加固 | 06 §6.8.1（`06:670-690`）：跨机 heartbeat 陈旧才判死、否则跳过并告警；`--json.recovered[]`/warnings | ✅ 与 spec 及 ISSUE-088 建议逐条对应 |
| D9 函数拆分清单 | 台账 ISSUE-081（`issues.md:89`）剩余清单 | ✅ 生产函数两侧清单逐项对应（测试 65/66/68/78/88 由“测试函数均 ≤60”覆盖） |
| D10 discover reason 闭集 | 05 §5.6.2.1（`05:935`）已列实际闭集；ISSUE-080 剩余为 05 §5.2.7/00 闭集复核 | ⚠️ 与现状重复，建议改口径（#8） |
| D11 AC-12 加强 | 07 AC-12（`07:202-215`：10 并发/隔离/上限）；07 §7.2/§7.3 #3（`07:6,228`：10 次无 flaky） | ⚠️ 用例形态一致，flaky 门槛低（C3） |
| D12 M6 排除 | 台账各 ISSUE 状态 | ⚠️ 含已 fixed 的 067、未列其他 open 项（#7） |
| D13 流程 | AGENTS §2/§3/§4；merge.sh 评审门禁 | ⚠️ 主流程与 AGENTS 一致；`(+)` 跨模块口径未定义（#5） |
| §1 ISSUE-079 | architecture §8 D4 / 05 §5.13.1 D13（`05:1374`） | ⚠️ 引用 D16 错误（#9） |

> 增量复审（`e03ed14`）后：D1 / D10 / D11 / D12 / D13 / §1-079 的 ⚠️ 已消除（见 §4.6）；上表保留首轮抽查记录。

### 4.4 必须核查项 4：跨模块、progress、索引/README

- **跨模块与派工（AGENTS §2）**：DR-003 文首声明 `doc/**`（docs）+ 根 `README.md`（process，Lead 为负责人），并由非作者 reviewer-a 在本记录第二评审；分支名 `docs/process-m6-kickoff` 符合 `<type>/<module>-<slug>`；波次表把 036/047/048/082 标跨模块并写明“双评审”，与 D13 一致。✅
- **progress 四块与行数**：当前阶段（`progress:5`）/ 进行中（波次表 `7-15`）/ 下一步（`17-20`）/ 阻塞（`22-23`）四块齐全；总行数 23、非空行 18（≤30，`check.sh` 亦校验）。✅
- **链接有效性**（脚本遍历 4 个改动文件的相对链接）：README 15 条、`doc/index.md` 17 条、`doc/progress.md` 1 条，**全部存在**（含 `doc/decisions/DR-003-m6-scope-and-waves.md`）。✅
- **索引/README 自洽**：`doc/index.md:7` 新增 DR-003 行（概述与正文一致）；`doc/index.md:18` “M6 自 REV-039 起”与 reviews 实况（无 037、最新 038）一致；README 状态段（M6 进行中、17 ISSUE + REV-037 + AC-12 加强）与 DR-003 §1、main progress 的 M6 清单一致。✅（REV-039 索引行缺失由 REV-057 补，见 #13）

### 4.5 必须核查项 5：门禁与工作区

- `bash scripts/dev/check.sh` → `CHECK PASSED`（exit 0，见 §4.1）；`git status --porcelain` 空。✅
- 提交信息 `docs(process): …`，正文含 `Review: REV-039` / `Agent: lead`，作者/提交者均为仓库身份 `chenyuxiangg <1007423165@qq.com>`。✅

### 4.6 增量复审（`e03ed14`，2026-10-30）

```bash
$ git log --oneline -3
# e03ed14 docs(process): 关闭 REV-039 C1–C4——DR-003 退出码/波次/flaky/收口口径修订（含建议 #5–#12）
# 39916d0 docs(review): REV-039 M6 kickoff 评审
# 55f0218 docs(process): M6 kickoff——DR-003 范围/波次/REV 与进度/索引/README 更新
$ git diff 39916d0..e03ed14 --stat
# doc/decisions/DR-003-m6-scope-and-waves.md | 31 +++++++++++++++------------
# doc/progress.md                            |  4 ++--
# 2 files changed, 19 insertions(+), 16 deletions(-)（纯文档；无代码/测试/CI/spec 正文改动）
$ git status --porcelain        # 空
$ bash scripts/dev/check.sh     # CHECK PASSED（progress 非空行 18 ≤30；cargo fmt / git diff --check 通过）
$ git log -1 --format='%B' e03ed14
# 正文：Review: REV-039 / Agent: lead（符合 AGENTS §1）
```

| 条件 | 复查点（`e03ed14` 后行号） | 结果 |
|---|---|---|
| C1 | `DR-003:64` D1：validate **0** = 合法、**2** = DSL/资产错误与用法错误、**1** = IO/内部、`--help` = 0；保留「02 §2.7.4 的 3 为 M1 临时口径，REV-056 回写」 | ✅ 与 06 §6.10.5（`06:904-905`）、06 §6.10.2（`06:786-793`）、02 §2.7.4（`02:722`）一致 |
| C2 | `DR-003:30` ISSUE-088 → **W3**/REV-055（与 `DR-003:55` 波次 3 一致）；`DR-003:33` REV-037 → **W1b**/REV-045（与 `DR-003:45` 一致） | ✅ 主项关闭；残余 #15（068/073/078 行 `W1`）为子波粒度 Nit |
| C3 | `DR-003:81` DoD #2：**收口时连续 10 次重跑无 flaky**（07 §7.2/§7.3 #3），分支开发期 2 次仅作快速门禁、不替代收口；D11（`:74`）保留「2 次」为开发期口径 | ✅ 与 07 通过标准一致，不再有未声明下调 |
| C4 | `DR-003:80` DoD #1：17 个 ISSUE 全部**收口**（16 `fixed`；ISSUE-073 按 D2 记 `wontfix` 并写明理由；REV-037 与 REV-028 #6 一并关闭）；`DR-003:65` D2 仍为 wontfix | ✅ `fixed`/`wontfix` 不再互斥，16+1=17 |
| #5–#12 | D13（`:76`）定义 `+`/`(+x)` 并纳入双评审；§1 标题+REV-028 #6 行（`:11,32`）与 §6 来源（`:100-104`）；D12（`:75`）删 067/补「只列 M6 相关排除项」/039 备份子项随 REV-048；ISSUE-080 模块（`:24`）+ D10（`:73`）；D4（`:67`）`VACUUM INTO` 备份/失败恢复；progress（`:9,15`）kickoff=review、process=wip | ✅ 已落实 |

- 另核对：§1 仍为 **17 个 ISSUE**（新增 `REV-028 #6` / `REV-037` 行不计入 ISSUE 数）；REV-039–057 编号表未变；progress 四块、23 行（非空 18）不变；wave1 各 worktree 实有未提交改动（`git -C .worktrees/<module>-* status --porcelain` 非空），故 `wip` 与实际一致。
- 新增 Nit 复核：#14 `--version` 括注（§3）；#15 `W1`/`1b` 粒度（§3）。均为非阻塞，不影响 `merge.sh` 门禁。

## 5. 遗留问题

- **条件已全部关闭**：C1–C4 已由 `e03ed14` 关闭（证据见 §4.6/§6），本记录结论已更新为「通过」；无剩余阻塞项。
- **非阻塞 Nit（合并后跟进）**：#14 `DR-003:32` 的「`--version` 已符合」括注应删除/改为「格式待统一」（REV-028 #6 本身已由 DoD #1 + §6 明确由 REV-043 关闭，不影响执行）；#15 `W1`/`1b` 粒度可在 REV-057 或下次 DR-003 修订统一。建议随 REV-043/REV-056/REV-057 落地，无需新开 ISSUE。
- **台账同步提醒**：#9 的 D16 错误仍写在 `doc/issues.md:87`（ISSUE-079）——DR-003 已正确改为 architecture §8 D4，REV-051 关闭 079 时请同步台账文本；#12 的 039「迁移备份」子项已在 D4/D12 注明随 REV-048 关闭，REV-048 评审时核对台账状态（权限 0600/0700 子项仍留 v0.2）。
- **流程**：本记录只改评审文件（`39916d0` + 本次增量复审提交）；未修改 DR-003/README/progress/index/spec/issues，未 merge/push。合并按 `scripts/dev/merge.sh docs/process-m6-kickoff --push` 执行（门禁会复跑全量测试）。

## 6. 条件关闭记录（已关闭；追加修订，首轮条件原文保留未删）

| 条件 | 关闭要求（可执行） | 关闭证据（commit/行） | 关闭日期 |
|---|---|---|---|
| C1 | `DR-003` D1 退出码与 06 §6.10.5、02 §2.7.4 对齐 | `e03ed14`：`DR-003:64` —— validate **0** 合法 / **2** DSL·资产·用法 / **1** IO·内部 / `--help`=0，并保留 02 §2.7.4「3 为 M1 临时口径，REV-056 回写」 | 2026-10-30 |
| C2 | 统一 ISSUE-088 波次与 REV-045 的 W1/1b 标注 | `e03ed14`：`DR-003:30` ISSUE-088 → **W3**/REV-055（与 `DR-003:55` 一致）；`DR-003:33` REV-037 → **W1b**/REV-045（与 `DR-003:45` 一致）；残余 #15 为子波粒度 Nit | 2026-10-30 |
| C3 | AC-12 防 flaky 门槛与 07 §7.2/§7.3 #3 对齐或显式记录偏差 | `e03ed14`：`DR-003:81` DoD #2 —— 收口时**连续 10 次**重跑无 flaky；分支开发期 2 次仅快速门禁、不替代收口 | 2026-10-30 |
| C4 | DoD #1 的 `fixed` 与 D2 的 `wontfix` 统一 | `e03ed14`：`DR-003:80` DoD #1 —— 17 个 ISSUE 全部**收口**（16 `fixed` + ISSUE-073 按 `DR-003:65` D2 记 `wontfix` 并写明理由） | 2026-10-30 |

> 增量复审由 reviewer-a 于 2026-10-30 执行；本段为追加修订，首轮条件原文与发现清单保留未删。
