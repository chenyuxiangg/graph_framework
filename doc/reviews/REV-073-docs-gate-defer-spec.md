# REV-073 · docs · W2a 实现回写（AC-14 校准 / as-built / 08 §8.9.1 / 台账 / PROVENANCE）

> 独立评审记录：评审人 `reviewer-docs`，**≠ 作者**（AGENTS §3）。本记录只写评审结论与证据，不含对 spec/设计稿/代码的修改。
> 评审基准：已合并实现（`main` `e7578f1` 时点：dsl/trace/py-worker/kernel+cli）+ e2e（产品终证 `d31e5d2`、评审 `2b2efb2`、N1 修复 `301b91f`；增量复审已含单 item 加固 `c6d172f` + REV-072 §8 `e2cba52`，且 e2e 已合入 `main`）+ DR-005 冻结语义 + REV-067…072 记录。**不信作者自述，逐项对照真实代码/测试/断言**。

- **分支**：`docs/docs-gate-defer-spec`
- **作者**：`docs-writer`（7 提交：`20a9b89` 07/01/02/03/04/06 回写、`46c8b3d` DR-005 R1–R5 + 08 §8.9.1 + PROVENANCE 第 17 条、`6b8eb9a` 索引/架构/进度/台账 ISSUE-104..109、`d6561ac` e2e 终证/台账/PROVENANCE 终测、`0eef2e3` 登记 ISSUE-110/111、`a35a54d` 关闭 REV-073 S-1..S-4/N1–N8、`706fcdf` 单 item rework profile 校准；正文均含 `Review: REV-073` / `Agent: docs-writer`）
- **评审人**：`reviewer-docs`（**≠ 作者**；主审，独立评审）
- **变更范围**：**最终 `e7578f1..706fcdf`**，**18 files +345/−101**（设计/规格/索引 16 files +177/−101；评审记录 2 files +168）：根 `README.md`（Lead 已授权）与 `doc/**`（DR-004 §10/W2a DoD、DR-005、architecture-v2 注记、index、issues、progress、spec 01/02/03/04/06/07/08、PROVENANCE、spec README）。首轮 `e7578f1..0eef2e3` 16 files +181/−97；增量复核 `d4f0550..706fcdf` 2 commits（`a35a54d`/`706fcdf`）11 files +59/−67。无代码/测试/配置/`CLAUDE.md` 改动
- **评审时间**：2026-10-31
- **关联**：DR-004 §10（执行记录）；DR-005（W2a0 冻结 + R1–R5）；REV-072（e2e W2a 验收，`2b2efb2` 通过、`301b91f` N1 修复）；REV-067/068/069/070/071；07 AC-14；PROVENANCE §2 第 17 条；ISSUE-104..111、ISSUE-062

## 1. 结论

**结论：通过**（2026-10-31 增量复审修订：首轮结论为「有条件通过」；S-1/S-2/S-3/S-4 均已于 `a35a54d`/`706fcdf` 关闭，N-1…N-8 已落实，证据见 §7；首轮条件清单保留于下供追溯）

方向判定：本批把 **W2a 实现（已合并 `main` `e7578f1`）与 e2e Phase 2B 的真实结果**回写进 07 AC-14、01/02/03/04/06 as-built、08 §8.9.1、DR-005 R1–R5、DR-004 §10、PROVENANCE §2 第 17 条与 §3、台账与 progress/README；**W2a0 冻结语义未被重写**，DR-004 原决策未改；PROVENANCE 9 个分册 SHA-256 与行数实测一致（00–08 **6385**、含索引 **6668**、08=**524**），`check.sh` PASSED，范围仅 `doc/**`+根 README。as-built 主体（`updated_at: String`、per-port 继承+`settled_by_node` 清空、`InstanceBuffer` 无实例级 settled、`apply_publish` 四路径、超限 sink 关闭、`fail_if_blocked_deferred` 顺序/message、`GateEvaluation.verdict` 规则、DDL 表尾列序、`verdict_json` 严格报错、R15 首错/载荷/子图聚合、迁移 v3→v4）逐项与 `main` 代码 / REV-068…071/072 复核一致。

首轮独立核对曾发现 **4 条重要**问题（文档校准错误，无阻塞实现项），当时判「有条件通过」。条件如下，**均已于 2026-10-31 在 `a35a54d`/`706fcdf` 关闭（逐条证据见 §7.2）**：

1. **S-1（重要）AC-14 g 与本批 as-built 资产/断言不符**：`defer_blocked.json` 的两条 defer 边实际均为 `check_output{path:"go",equals:true}`（`a` 的 `stop` 输出 `{"go": false}`，故运行期 false），文档却写 `manual default false`/「恒 false」；且文档断言 `gate_results`「恰 2 行且均 passed=false/deferred=true」，实测为 **3 行**（`start→a` 无 gate 1 行 passed + 自环/a→b 2 行 deferred）。落点 `07:299/:301`、`DR-005:592`。**✅ 已关闭（§7.2）**
2. **S-2（重要）「连续 5 轮全绿」证据口径不实**：REV-072 §4 记录的是 `gate_defer + software_team_v1` **连跑 3 次**、`smoke_rework` 过滤**连跑 5 次**；无「e2e 58/0/2、workspace 694/0/3 全量 5 轮」记录。该表述见 `07:257/:316`、`08:512`、`DR-004:251`、`DR-005:595`、`PROVENANCE:143`。**✅ 已关闭（§7.2；引用口径微调见 §7.6 N-9）**
3. **S-3（重要）超限 GateResult 表述自相矛盾**：「resolve 失败的那条边完全不产生 GateResult」与同段「victim `P→W.feedback` 只有 attempt 0/1 两行」「该边更早轮次 GateResult（如有）」冲突；正确口径 = **超限那次求值不产生**，该边更早轮次行保留（e 为 2 行），仅当首轮即超限（h 的 `max_attempts=1` 自环）时该边 0 行。落点 `07:288`、`03:186/:261`、`DR-005:341/:382`、`08:520`、`DR-004:251`、`PROVENANCE:143`。**✅ 已关闭（§7.2）**
4. **S-4（重要）Phase 2B「进行中/待」残留与同批「已通过」矛盾**：`PROVENANCE:136`「Phase 2B 进行中，最终证据另补」、`:137`「a/b/f/g … `#[ignore]` 待 Phase 2B」、`07:19`「运行期待 Phase 2B」、`spec README:65`「e2e Phase 2B 最终证据另补」——而同一提交后文 `07:257` 已写「Phase 2B 已通过」，且 REV-072 记录（e2e 分支 `2b2efb2`）已通过。应统一为「Phase 2B 已通过」并同步 `08:512`/`DR-004:251`/`progress:17` 的「待 REV-072」为「REV-072 已通过、待合并」。**✅ 已关闭（§7.2）**

另有 8 条**建议**（N-1…N-8，见 §3）：PROVENANCE「增量 95 行」实测不符、dsl 首轮计数 85/664（末态 86/665）、issues 表格空行断裂、progress 非四块结构、08 §8.9.1「不改 8.0–8.9」措辞、full 60/116 运行期覆盖口径、README 历史摘要、e2e HEAD/REV-072 引用时点。均不阻塞，但需修复或登记台账。

## 2. 四维评审

| 维度 | 结论 | 依据（文件/行/实测） |
|---|---|---|
| 功能正确性 | ⚠️（S-1…S-4 备注） | AC-14 a/b/c/e/f/h 与 e2e 实测逐项复核**一致**：a（`gate_defer.rs:656-712`：attempt0/1、同边两行 `passed/deferred/verdict=true/false`、attempt1 继承 `seed`、`again=attempt0 output`）；b（`:714-750`：A/B 0/1、A→B 两行放行、B→A 第二行 false+deferred）；f（`:752-779`：dormant、`verdict=true` 原始路径值）；e（`software_team_v1.rs:1794-1907`：恰一 Crashed W attempt=3、victim 不写死、超限 feedback 边 attempt 0/1 两行、无残留非终态）；c（`:1752-1790` + `assert_rework_chain`：W/R/P 0/1、前进/回退边真实值、P1 消费 round2 review、archer 最终 artifact）；h（`gate_defer.rs:286-630`：迁移/备份/旧行默认/旧 payload v3/只读 exit4/L1 golden/新 run 三键/超限自环 0 行）。缺口：S-1 g 的图与行数断言错误；S-2 稳定性证据口径不实；S-3/S-4 表述矛盾（非实现偏差）。as-built 注（01:204、02:67/612、03:264、04:708、06:302/698）与 `main` 代码逐项吻合（见 §4.4） |
| 代码结构合理性（文档/计划结构） | ⚠️（N-4/N-5 备注） | 16 文件全部在 Lead 授权范围（`doc/**` + 根 README）；按「07→01/02/03/04/06→DR/08/PROVENANCE→索引/台账→收口」分层提交，commit 粒度合理；08 采用「§8.9 W2a0 注记 + §8.9.1 W2a 实现证据」分册，未重写 8.0–8.8 历史；architecture-v2 仅加 banner 注记；DR-004 只追加 §10、不改 D1–D7。缺口：progress 新增「进行中」「遗留 ISSUE」两块（非四块结构，N-4）；issues 表格被空行断开（N-3） |
| 逻辑性 | ⚠️（S-3/S-4 备注） | PROVENANCE §2 第 17 条 / §3 与实测一致（9/9 SHA-256、行数 6385/6668、08=524）；「W2a0 设计冻结 ≠ W2a 实现」的分层表述清晰；08 §8.9.1 模块-commit-REV-计数-台账四列与 REV-068…072 记录一致（除 dsl 首轮计数、e2e 待 REV-072 两处时点问题）；DR-005 R1–R5 修正落实（§6.4 超限证据、§7 h L1 拆分、§3.4 行 9、§3.3 伪代码 3/4/8、§12.3 commit 数）；ISSUE-104 fixed 有 e2e 产品提交+REV-072 支撑；ISSUE-105…109 内容与 REV-070/071/069/068 建议一一对应。缺口：S-3/S-4 内部矛盾；N-1/N-2 数字与时点 |
| 可读性 | ✅（S-1/S-4、N-6/N-7 备注） | 中文术语一致（as-built/attempt-per-activation/deferred/dormant）；表格/代码块规范；交叉引用（DR-005 §7 a–h、REV-072、ISSUE-104..111、PROVENANCE 第 17 条）实测可解析；无未替换模板占位；新增 §8.9.1/§2 第 17 条编号连续。瑕疵：S-1 描述与文件不符、S-4 残留「待 Phase 2B」、README 历史压缩为 1 段摘要（N-6） |

## 3. 发现清单

> 级别口径：**阻塞** = 必须修复后才能给「通过」；**重要** = 必须修复或登记 ISSUE；**建议** = 可后置，但需修复或进台账。

| # | 级别 | 位置 | 问题 | 建议 | 状态 |
|---|---|---|---|---|---|
| S1 | 重要 | `doc/spec-v1.2/07-验收用例.md:299`、`:301`；`doc/decisions/DR-005-w2a-defer-gate-freeze.md:592` | AC-14 g 的 as-built 图描述与 fixture 不符：实际 `tests/fixtures/graphs/defer_gate/defer_blocked.json` 两条 defer 边均为 `check_output{path:"go",equals:true}`（`py/defer_stub.py::stop` 返回 `{"go": False}`），文档写 `manual default false` /「恒 false」；且断言 `gate_results` 恰 2 行且均 `passed=false/deferred=true`，而 `gate_defer.rs::blocked_deferred_receipt_fails_with_run_deadlock:809-818` 断言 **3 行**（`start→a` 无 gate 1 行 passed + 自环 + a→b 2 行 deferred） | 改为 as-built：两条 defer 边 gate = `check_output{path:"go",equals:true}`（`a` 输出 `go=false`）；`gate_results` 3 行（1 无 gate passed + 2 deferred），并对照该测试复跑 | open |
| S2 | 重要 | `07:257`、`07:316`、`08:512`、`DR-004:251`、`DR-005:595`、`PROVENANCE:143`、`doc/progress.md:5` | 「e2e 58/0/2、workspace 694/0/3，连续 5 轮全绿」与 REV-072 §4 证据不符：REV-072 记录 `gate_defer + software_team_v1` 连跑 **3 次**、`smoke_rework` 过滤连跑 **5 次**；无全量套件 5 轮记录 | 改为 REV-072 实际口径（3 次两 target + 5 次 smoke_rework 过滤），或补全量 5 轮的运行日志/命令证据后保留「5 轮」 | open |
| S3 | 重要 | `07:288`、`03-调度器与生命周期.md:186`、`:261`、`DR-005:341`、`:382`、`08:520`、`DR-004:251`、`PROVENANCE:143` | 超限 GateResult 表述自相矛盾：既写「resolve 失败的那条边完全不产生 GateResult / 失败边 0 行」，又写「该边更早轮次 GateResult（如有）」；`07:288` 同句内又写「victim 的 `P→W.feedback` 边只有 attempt 0/1 两行」 | 统一为：「**超限那次求值**不产生 GateResult（sink 已关）；该边更早轮次的行保留（e 为 attempt 0/1 两行）；仅当首轮即超限（h 的 `max_attempts=1` 自环）时该边 0 行」 | open |
| S4 | 重要 | `PROVENANCE:136`、`:137`、`07:19`、`doc/spec-v1.2/README.md:65`；并同步 `08:512`、`DR-004:251`、`progress:17` | Phase 2B 残留陈述与同批「已通过」矛盾：PROVENANCE 来源写「Phase 2B 进行中，最终证据另补」、07 写「a/b/f/g `#[ignore]` 待 Phase 2B」、07 资产表写「运行期待 Phase 2B」、spec README 写「最终证据另补」；而 `07:257` 已写「Phase 2B 已通过」，REV-072 已通过（e2e 分支 `2b2efb2`，无新增 `#[ignore]`） | 统一为「Phase 2B 已通过（e2e 分支 `d31e5d2`、REV-072 `2b2efb2`；最终 HEAD `301b91f` 仅 N1 断言收紧）」；`待 REV-072` 改为「REV-072 已通过、待合并」 | open |
| N1 | 建议 | `PROVENANCE:161` | 「00/01/…/08 增量 95 行」与实测不符：00–08 由 6334→**6385**（净 +51）；本批 00–08 diff 为 +87/−36 | 改「增量 51 行（新增 87/删除 36）」或删去增幅数字 | open |
| N2 | 建议 | `08:508`、`DR-004:247` | dsl 测试计数取 REV-069 首轮 85/664；N1–N4 修复后末态为 86/665（REV-069 §7） | 用末态 86/665，或写明「首轮 85/664 → 复审 86/665」 | open |
| N3 | 建议 | `doc/issues.md:118` | ISSUE-109 与 ISSUE-110 之间多一个空行，把 ISSUE-110/111 与主表断开（Markdown 渲染断表，8 列表头丢失） | 删除空行，保持 104–111 连续同一表格 | open |
| N4 | 建议 | `doc/progress.md:7-25` | 结构非「四块」：现有「当前阶段 / 里程碑表 / 进行中 / 下一步 / 阻塞 / 遗留 ISSUE」六块，且「遗留 ISSUE」与 `doc/issues.md` 台账重复（AGENTS §5 要求只保留四块） | 合并为「当前阶段 / 进行中任务 / 下一步 / 阻塞」四块；里程碑表与遗留 ISSUE 列表移到 DR/issues 引用（保持 ≤30 行） | open |
| N5 | 建议 | `08:501` vs `08:496` | §8.9.1 头注称「不改 8.0–8.9 历史」，但同批把 §8.9 的 ISSUE-104 行改为 fixed（状态更新） | 头注补「§8.9 状态行同步更新除外」，或把状态变更只放 §8.9.1/台账 | open |
| N6 | 建议 | `07:256-257`、`DR-004 §3 W2a DoD` | e2e c/e 运行期用 smoke 11/21（full 60/116 仅 validate/结构/hash）；DR-004 W2a DoD 写「真实 SOP v1 图 rework e2e」，两者口径需确认（W1 有 smoke 先例） | 在 07/08 明示「full 60/116 运行期未覆盖（沿用 W1 smoke 先例）」并由 Lead 确认，或登记 ISSUE | open |
| N7 | 建议 | `README.md:13-22`（历史摘要 `:21`） | README 状态段把原 M5/M6 详细段压缩为「历史里程碑（摘要）」1 段；若 Lead 的「M0–M6 历史保留」指保留原段落，则属内容缩减 | 保留摘要 + 指向 DR-003/REV 索引的链接即可（当前已可追溯）；如要求原样保留则恢复简表 | open |
| N8 | 建议 | `08:512`、`DR-004:251`、`progress:17`、`doc/index.md:21` | 语料时点：e2e 证据固定 `d31e5d2`，而 e2e 分支 HEAD 已 `301b91f`（REV-072 N1 修复，仅收紧 g message 断言）；REV-072 记录已存在（`2b2efb2`，通过）但文档仍写「待 REV-072」 | e2e 合并/rebase 时同步为最终 HEAD +「REV-072 已通过」；`doc/index.md` 的 REV-073 行在记录合并后更新 | open |

## 4. 验证命令与结果

### 4.1 范围、提交约定、增量 commit

```bash
cd /Users/zyzs/lczyzs/code/zgraph/.worktrees/docs-docs-w2a
git status --porcelain --branch          # ## docs/docs-gate-defer-spec...origin/main [ahead 5]；评审前 clean
git rev-parse HEAD origin/main           # 0eef2e37… / e7578f15…
git log --oneline origin/main..HEAD
# 0eef2e3 docs(process): 登记 ISSUE-110（REV-072 N2/N3）
# d6561ac docs(docs): W2a 收口——e2e 证据/台账/README/PROVENANCE 终测
# 6b8eb9a docs(process): W2a 收口——索引/架构注记/进度/台账 ISSUE-104..109
# 46c8b3d docs(docs): DR-005 R1–R5 修正 + 08 §8.9.1 实现证据 + PROVENANCE 第 17 条
# 20a9b89 docs(docs): W2a 实现回写 07 AC-14 校准 + 01/02/03/04/06 as-built
git log origin/main..HEAD --format=%B | grep -c "Review: REV-073"   # 5
git log origin/main..HEAD --format=%B | grep -c "Agent: docs-writer" # 5
git diff --name-status origin/main...HEAD | wc -l                    # 16
git diff --shortstat origin/main...HEAD                              # 16 files changed, 181 insertions(+), 97 deletions(-)
git diff --name-status 0eef2e3^..0eef2e3                             # 6 files（issues/progress/08/PROVENANCE/spec README/DR-004）
git diff --shortstat 0eef2e3^..0eef2e3                               # 6 files changed, 12 insertions(+), 7 deletions(-)
git diff --name-only origin/main...HEAD | grep -E 'CLAUDE|\.rs$|\.py$|Cargo|scripts/|schemas/|doc/reviews/' || echo "(none)"
# (none) → 无代码/测试/配置/CLAUDE/评审记录改动；16 文件均为 doc/** + 根 README（Lead 授权）
```

### 4.2 PROVENANCE 实测（SHA-256 / 行数）

```bash
cd doc/spec-v1.2
shasum -a 256 0*.md
# 00 ce92145f…d25c5（309）
# 01 710f6c8f…bf85d（346）
# 02 04820a7d…30928（935）
# 03 85cc0408…e155b（489）
# 04 46247060…ad513（906）
# 05 52f35e7b…bf42（1429，未改）
# 06 8ab4b5e2…c9ee（1117）
# 07 6c13370a…f5bf7（330）
# 08 7afb8c72…a793f（524）
wc -l 0*.md | tail -1                                   # 6385
wc -l 0*.md README.md PROVENANCE.md REFERENCES.md | tail -1  # 6668
```

- 9 个 SHA-256 与 `PROVENANCE.md:151-159` §3 表**逐字一致**；行数表（PROVENANCE §3 与 `spec-v1.2/README.md:24-32`）与本实测一致（含 08=524、07=330）。
- `PROVENANCE` §2 第 17 条已记录本批（2026-10-31、REV-073、分支、范围、e2e 终证、台账、00–08 6385）✓；§2 第 15 条已恢复首轮 6308（REV-067 R7 关闭）✓。
- 唯一数字瑕疵：§3 脚注「增量 95 行」与实测净 +51 不符（N1）。

### 4.3 一致性 grep 与文件级核对

```bash
grep -n "Phase 2B\|待 REV-072\|连续 5 轮" doc/**/*.md doc/*.md README.md   # S-2/S-4/N8 残留点（见 §3）
grep -n "manual default false\|恰 2 行" doc/spec-v1.2/07-验收用例.md doc/decisions/DR-005-w2a-defer-gate-freeze.md  # S-1
grep -n "^| ISSUE-10[4-9]\|^| ISSUE-11[01]" doc/issues.md   # 104–111 连续；104 fixed/105–111 open
grep -n "REV-072" doc/issues.md doc/spec-v1.2/08-审计关闭矩阵.md doc/decisions/DR-004-v02-scope-and-waves.md doc/index.md doc/progress.md  # 引用存在；时点待同步（N8）
grep -n "R1–R15\|18 个 DSL 码" README.md doc/architecture-v2.md doc/spec-v1.2/02-DSL与校验.md  # README 目录 R1–R15 ✓；architecture 仅 banner 注记 ✓
```

- **AC-14 a/b/c/e/f/h 与 e2e 断言逐条对照一致**（见 §2 依据行号；`gate_defer.rs:656-824`、`software_team_v1.rs:1565-1907`）；g 见 S-1。
- **as-built 注与 `main` 代码对照**（§4.4）全部吻合；06 DDL 三列确在表尾（`schema.rs:206-222`），`migrate_step(3)` 用 `GATE_RESULTS_V4_COLUMNS`（`:393-396`），`map_gate_row`/`parse_json_opt` 严格报错（`reader.rs:759-780`、`model.rs:479-486`）。
- **08 §8.9.1 / DR-004 §10 证据表**：docs/dsl/trace/py-worker/kernel+cli 的 commit/REV/计数与 REV-067…071 记录一致；e2e 行与 REV-072 一致（8/0、11/1、58/0/2、694/0/3）——除 dsl 首轮 85/664（N2）、e2e「待 REV-072」（N8）、5 轮口径（S-2）。
- **台账**：ISSUE-100/101 已 fixed 且引用 REV-067/069、REV-068 正确；ISSUE-104 fixed（e2e 提交+REV-072 支撑）✓；105–109 open 内容与 REV-070/071/069/068 建议对应 ✓；110（REV-072 N2/N3）、111（e2e trace fixture 路径归属）内容合理 ✓；ISSUE-062 保持 open ✓；编号 104–111 连续、无重复；仅 N3 的空行断表。
- **DR-004 §10**：只追加执行记录，原 D1–D7/§3 决策未改 ✓。
- **architecture-v2**：仅顶部 banner 注记（指向 02 R15 / 00 §0.4 18 码），D1–D12 未改 ✓。
- **根 README**：状态段 `README.md:13-22` 实测 10 行（10–15 合规）；含 M0–M6 历史摘要、W2a0/W2a/下一步 W2b；目录 `crates/zgraph-dsl` 已改 **R1–R15** ✓。
- **progress**：25 行（≤30）✓；「当前阶段 / 下一步 W2b / 阻塞（无硬阻塞）」准确；结构见 N4。

### 4.4 e2e 证据交叉核验（只读 `test/e2e-gate-defer`，不修改）

```bash
cd /Users/zyzs/lczyzs/code/zgraph/.worktrees/e2e-e2e-writer
git log --oneline -6
# 301b91f test(e2e): 收紧 AC-14 g deadlock message 断言（REV-072 N1）
# 2b2efb2 docs(review): REV-072 e2e W2a 返工验收（主审 reviewer-a，通过）
# d31e5d2 … / 4e79562 / b80ea74 / 8ec09f1 / e7578f1
# 说明：本评审时 e2e HEAD 已 301b91f（docs 证据固定 d31e5d2；N1 仅收紧 message 断言）
grep -n "fn blocked_deferred_receipt_fails_with_run_deadlock" -A 45 tests/e2e/gate_defer.rs
# 3 行 GateResult（start→a 无 gate 1 + 自环/a→b deferred 2）；message 断言 edge [2]/target [b]；
grep -n "fn smoke_rework_" -A 120 tests/e2e/software_team_v1.rs
# c/e 断言与 07 一致；超限 feedback 边 attempts 0/1 两行
# 对照结果：07 a/b/c/e/f/h、DR-005 §7 a/b/c/e/f/h、DR-004 §10 计数与测试一致；
#           07/DR §7 g 的 manual/「恰 2 行」与 fixture/测试不一致（S-1）
# 稳定性证据：REV-072 §4「gate_defer + software_team_v1 连跑 3 次、smoke_rework 过滤 5 次」→ S-2
# 读 REV-072：结论 通过；N1（g message 断言弱化）已由 301b91f 修复；N2/N3 由 ISSUE-110 承接；N4 由 07 落点更新关闭
```

### 4.5 门禁

```bash
cd /Users/zyzs/lczyzs/code/zgraph/.worktrees/docs-docs-w2a
bash scripts/dev/check.sh     # CHECK PASSED（exit 0；含身份/冻结文件/禁用文件/语法/fmt/diff-check）
git diff --check              # 通过（无空白错误）
```

- 本分支为纯文档改动（16 files 均 `doc/**` + 根 README），未跑 `cargo test`/`pytest`（合并门禁会跑）；as-built 复核使用 `main` 已合并代码与 e2e 只读分支，不修改二者。

## 5. 遗留问题

- **条件未关闭**：S-1…S-4 关闭前结论保持「有条件通过」，不得合并；关闭后增量复审转「通过」。
- **建议项**：N-1…N-8 建议在本分支顺手修（N-1/N-2/N-3/N-4 为小改；N-5/N-6/N-7/N-8 为措辞/时点/口径），或登记 ISSUE 后置。
- **跨批（非本分支可独立完成）**：
  - e2e 分支 **REV-072 已通过**（`2b2efb2`），产品终证 `d31e5d2`，N1 修复 `301b91f`；本分支后续在 e2e 合并后 rebase 时需按最终 HEAD 复核 `d31e5d2` 引用与 `待 REV-072` 措辞（N8 / S-4）。
  - ISSUE-110（REV-072 N2/N3：rework L2 outputs 无直接 e2e、4 个测试函数 >60 行）与 ISSUE-111（AGENTS §2 e2e 路径清单缺 `tests/fixtures/trace/**`）已登记；ISSUE-111 需 process/docs 后续同步 AGENTS §2 路径清单。
  - full 60/116 SOP rework 图运行期覆盖率（N-6）需 Lead 确认沿用 W1 smoke 先例或登记 ISSUE。
- 未发现：越界/禁用/冻结文件改动、未评审代码、历史决策被改写（DR-004 原决策、architecture D1–D12、08 §8.0–8.8 均未改）、模板占位、不可执行的悬空引用。

## 6. 条件关闭记录（仅「有条件通过」填写）

| 条件 | 关闭证据（需可复核） | 关闭日期 |
|---|---|---|
| S-1 AC-14 g as-built 图/行数修正（check_output{path:"go"}、3 行 gate_results） | 07:299/301 + DR-005:592 修订；`gate_defer.rs::blocked_deferred_receipt_fails_with_run_deadlock` 复跑一致 | **已关闭 2026-10-31（§7.2）** |
| S-2 「连续 5 轮全绿」改为 REV-072 实测口径或补日志 | 07:257/316、08:512、DR-004:251、DR-005:595、PROVENANCE:143、progress:5 修订后与 REV-072 §4 一致 | **已关闭 2026-10-31（§7.2）** |
| S-3 超限 GateResult 表述统一 | 07:288、03:186/261、DR-005:341/382、08:520、DR-004:251、PROVENANCE:143 统一为「该次求值不产生；更早轮次行保留；首轮即超限时 0 行」 | **已关闭 2026-10-31（§7.2）** |
| S-4 Phase 2B 残留陈述清理（含「待 REV-072」） | PROVENANCE:136/137、07:19、spec README:65、08:512、DR-004:251、progress:17 改为「Phase 2B 已通过 / REV-072 已通过、待合并」 | **已关闭 2026-10-31（§7.2）** |


---

## 7. 条件关闭与复审（2026-10-31）

> 复审对象：`d4f0550..706fcdf`（2 commits：`a35a54d` 关闭 S-1..S-4/N1–N8、`706fcdf` 单 item rework profile 校准）。最终范围 `e7578f1..706fcdf`（18 files +345/−101，见 §7.1）。本段不改写 §3/§6 的首轮发现快照；§3 中 `open` 状态为首次评审时点，实际关闭以本段为准。

### 7.1 复审范围、范围门禁与实测

```bash
cd /Users/zyzs/lczyzs/code/zgraph/.worktrees/docs-docs-w2a
git status --porcelain --branch        # ## docs/docs-gate-defer-spec...origin/main [ahead 8]；增量复审前 clean
git rev-parse HEAD                     # 706fcdf50fca5370d0d5ebb8442a5357c2f529ef
git log --oneline d4f0550..706fcdf
# 706fcdf docs(docs): 关闭 REV-073 S-1..S-4 并校准 e2e 单 item profile
# a35a54d docs(docs): 关闭 REV-073 S-1..S-4/N1–N8（AC-14 c/e 待 e2e final 校准）
git diff --name-status d4f0550..706fcdf   # 11 files（README/DR-004/DR-005/index/issues/progress/03/07/08/PROVENANCE/spec README）
git diff --shortstat d4f0550..706fcdf     # 11 files changed, 59 insertions(+), 67 deletions(-)
git diff --shortstat origin/main...HEAD   # 18 files changed, 345 insertions(+), 101 deletions(-)
# 设计/规格/索引 16 files +177/−101；评审记录 2 files +168
git log origin/main..HEAD --format=%B | grep -c "Review: REV-073"     # 8（7 作者 + 1 评审）
git log origin/main..HEAD --format=%B | grep -c "Agent: docs-writer" # 7（作者 7 提交全部含）
git diff --name-only origin/main...HEAD | grep -E 'CLAUDE|\.rs$|\.py$|Cargo|scripts/|schemas/' || echo "(none)"
# (none) → 范围仍仅 doc/** + 根 README；无代码/测试/配置/冻结文件改动
bash scripts/dev/check.sh                # CHECK PASSED
git diff --check                         # 通过
```

```bash
cd doc/spec-v1.2 && shasum -a 256 0*.md && wc -l 0*.md README.md PROVENANCE.md REFERENCES.md
# 9/9 SHA-256 与 PROVENANCE §3 表逐字一致：00 ce92145f…d25c5 / 01 710f6c8f…bf85d / 02 04820a7d…30928 /
# 03 a7553b41…a640 / 04 46247060…ad513 / 05 52f35e7b…bf42 / 06 8ab4b5e2…c9ee / 07 21e0ea21…906c / 08 e7483be1…0f19
# 00–08 = 6385 行（309/346/935/489/906/1429/1117/330/524）；含 README/PROVENANCE/REFERENCES = 6668 行
# 00–08 净 +51 行（新增 87 / 删除 36）——与 PROVENANCE §3 脚注、§2 第 17 条、spec README 行数表**全部一致**
```

- 一致性 grep（实测）：无 `Phase 2B`/`待 REV-072`/`d31e5d2`/`最终证据另补`/`待 e2e final` 残留；无「全量 5 轮」「连续 5 轮全绿」残留；无「那条边完全不产生 GateResult」「失败边 0 行」残留；`07:299/301` 已为 `check_output{path:"go",equals:true}` + 3 行断言。
- PROVENANCE §2 第 17 条已记录本批（2026-10-31、REV-073、最终范围/行数），§3 当前值与实测一致；spec README 行数表（08=524、07=330…）同步。
- 说明：e2e 已合入 `main`（HEAD `e2cba52`，REV-072 §7/§8 记录与 README 行已在 main）；本 docs 分支仍基于 `e7578f1`，Lead 计划在 e2e 合并后 rebase（仅哈希变化）——docs 内容仍需在 rebase 时同步「e2e 已合并」措辞（§7.6 N-11，非阻塞）。

### 7.2 条件关闭证据（逐条）

| 条件 | 关闭证据（文件/行/命令） | 结论 |
|---|---|---|
| **S-1** AC-14 g as-built 图/行数 | `07:299`：两条 defer 边均为 `check_output{path:"go",equals:true}`，`a` 为 `defer_stub.stop` 运行期输出 `{"go":false}` → 两条 gate=false；`07:301`：`gate_results` **3 行**（`start→a` 无 gate 1 行 passed + `a→a`/`a→b` 2 行 false/deferred）；`DR-005:592` 同口径。对照 `tests/fixtures/graphs/defer_gate/defer_blocked.json` 与 `tests/e2e/gate_defer.rs:809-818`（断言 3 行、1 ungated passed + 2 deferred）**一致** | ✅ 关闭 |
| **S-2** 稳定性口径 | 文档不再出现「全量 5 轮/连续 5 轮全绿」；改为「e2e 58/0/2、空载 workspace 694/0/3；稳定性 = 4 CPU busy-loop + 全 e2e 并行下 e2e 连续 5 轮 58/0/2」，并记录「首版 3-item 并发 fake LLM 调用序交错 → merge 门禁 flake → 单 item profile」教训（`07:257/:316`、`08:512`、`DR-004:251`、`DR-005:595`、`PROVENANCE:143`）。与 REV-072 §8（e2e 连跑 3 轮 + busy-loop 下 `software_team_v1` 连跑 3 轮 + 空载 workspace）在轮次/作用对象上仍有引用差异 → **非阻塞建议 N-9** | ✅ 关闭（主体；引用口径见 N-9） |
| **S-3** 超限 GateResult 表述 | 统一为「**超限那次求值不产生 GateResult**；该边更早轮次行保留（e 为 attempt 0/1 两行）；仅首轮即超限（h 的 `max_attempts=1` 自环）时该边 0 行」：`03:186/:261`、`DR-005:341/:342/:382`、`07:287`、`08:520`、`DR-004:251`、`PROVENANCE:143`；grep 无旧「完全不产生/0 行」残留；与 REV-071（sink 关闭/外层 emit no-op）和 REV-072 §8（e feedback 0/1 两行、h 自环 0 行）一致 | ✅ 关闭 |
| **S-4** Phase 2B/待 REV-072 残留 | grep 无 `Phase 2B`/`待 REV-072`/`d31e5d2`/`最终证据另补`/`待 e2e final`；统一为「REV-072 已通过（N1 后 §7 + 单 item 加固 §8 增量复核）；e2e 分支待合并，最终 HEAD 以合并时为准」：`PROVENANCE:136/:137`、`07:19/:257`、`spec README:65`、`08:512`、`DR-004:251`、`progress:17` | ✅ 关闭 |
| **Lead 补充** 单 item 校准 | 见 §7.4；与 `main`（e2e 已合并）代码/测试/review 记录一致 | ✅ 关闭 |

### 7.3 建议项 N-1…N-8 落实核验

| # | 首轮建议 | 落实证据 | 状态 |
|---|---|---|---|
| N-1 | PROVENANCE「增量 95 行」 vs 实测 | `PROVENANCE:161` 已改「净 +51（新增 87/删除 36）」；`git diff origin/main...HEAD --numstat -- doc/spec-v1.2/0*.md` 实测 add=87/del=36/net=51 | ✅ |
| N-2 | dsl 首轮 85/664 | `08:508`、`DR-004:247` 改为「86（末态；首轮 85）、665/0/3（首轮 664）」，与 REV-069 §7 一致 | ✅ |
| N-3 | issues 表格空行断裂 | ISSUE-104…112 连续同一表格（`issues:112-120`），无空行；ISSUE-112（P2，full rework 运行期未覆盖）已登记 | ✅（引用瑕疵见 N-10） |
| N-4 | progress 非四块结构 | `progress.md` 17 行、结构收敛为「当前阶段 / 进行中任务 / 下一步 / 阻塞」四块（AGENTS §5） | ✅ |
| N-5 | 08 §8.9.1「不改 8.0–8.9 历史」措辞 | `08:503` 注（N-5）：§8.9 为 W2a 追加段、非 8.0–8.8 历史审计矩阵；ISSUE-104 状态行更新属追加段维护，8.0–8.8 未改 | ✅ |
| N-6 | full 60/116 运行期覆盖口径 | `ISSUE-112` 登记（P2）；`DR-004:75` W2a DoD 标注「部分达成（2026-10-31，REV-072）」+ ISSUE-112；07/08 c/e 明确改用 `rework-item` 5/7、full/smoke 仅 validate/结构/hash | ✅ |
| N-7 | README 历史摘要 | 根 `README.md:13-22` 状态段 10 行（10–15 合规），保留「历史里程碑（摘要）」并链到 DR/REV 索引；W2a 行已更新 REV-072 已通过/待合并口径 | ✅ |
| N-8 | e2e 证据时点/哈希 | 全部改为「e2e 分支 + REV-072 已通过；最终 HEAD 以合并时为准」，不写死 commit 哈希；rework-item `graph_hash` 标「合并后以实测为准」 | ✅（e2e 已合入 main，rebase 同步见 N-11） |

### 7.4 e2e 单 item 校准核验（对照 `main` 已合并 e2e + REV-072 §7/§8）

- `07` AC-14 c/e 改为 `--profile rework-item`：**5 节点/7 边**，graph_hash as-built `79dd51c684ffc771`（合并后以实测为准）；`codegen.py` 的 `PROFILE_REWORK_ITEM` 仅接受 `--mode rework`（非 rework 显式 `ValueError`）——与 REV-072 §8 一致（评审人独立复跑 5/7/hash）。
- **c**（roundtrip，fake 4 calls）：run `Completed`、`run_output=null`；W/R/P attempts `0/1` 且 `Completed`、`planer` 仅 attempt0；forward（P→archer）attempt0 `false/deferred/"rework"` → attempt1 `true/false/"approved"`；feedback（P→W）attempt0 `true/false/"rework"` → attempt1 `false/true/"approved"`；P1 `review.content` 为 round2 approved（含 round2 comments，无 stale-input）；archer 拿到 W attempt1 最终 artifact（`feedback.verdict="rework"`）——与 `tests/e2e/software_team_v1.rs:1774-1808` + `assert_rework_chain` 一致。
- **e**（exhaust，单 item 恒定 rework）：exit 1 / `Failed NODE_INTERNAL`；message 含 `exceeded max_attempts (3)` 且点名 victim `planer_specification`；W attempts `[0,1,2,3]`（3=Crashed）、R/P `[0,1,2]`；forward 3 行均为 `(attempt, false, true, "rework")`；feedback **仅 attempt 0/1 两行**（超限 attempt2 不入库）；archer **0 实例**；无残留非终态——与 `:1815-1907` 及 REV-072 §8 一致。
- **覆盖 tradeoff**：full 60/116、smoke 11/21 仅 validate/结构/hash；full 运行期未覆盖 → **ISSUE-112（P2，open）**；DR-004 W2a DoD 标「部分达成」；「3-item 并发 fake LLM 调用序交错 → merge 门禁 flake → 单 item profile」教训已记录（07/08/DR-004/DR-005/PROVENANCE）。
- 08/DR-004/DR-005/ISSUE-104 口径：ISSUE-104 fixed（W2a e2e 交付 + REV-072 已通过）✓；REV-072 已通过（`2b2efb2`，§7/§8 增量复核）✓。

### 7.5 四维复审（增量）

| 维度 | 复审结论 | 依据 |
|---|---|---|
| 功能正确性 | ✅ | S-1/S-3/S-4 关闭且与 main 代码/e2e/REV-072 §8 一致；as-built 注未回退；AC-14 a/b/f/g/h 仍与既有 e2e 断言一致；c/e 单 item 断言逐条核对（§7.4） |
| 代码结构合理性（文档结构） | ✅ | 范围仍仅 `doc/**`+根 README；两笔修复提交按「S 条件/N 建议」与「e2e 单 item 校准」拆分；issues/progress 格式修复；08 §8.9.1/DR-004 §10 证据表与 REV 记录对应 |
| 逻辑性 | ✅（N-9 引用口径备注） | PROVENANCE 9/9 + 6385/6668、净 +51 自洽；N1–N8 数字/时点一致；DR-005 R1–R5 未被回退；无新的「Phase 2B/待 REV-072」矛盾；仅稳定性引用来源与 REV-072 §8 轮次/对象有差异（N-9，建议） |
| 可读性 | ✅ | 术语/表格规范；无占位；「e2e 分支待合并、HEAD 以合并时为准」统一；issues 表格连续；progress 四块简洁 |

### 7.6 残留建议（非阻塞）

| # | 级别 | 位置 | 问题 | 建议 |
|---|---|---|---|---|
| N-9 | 建议 | `07:257/:316`、`08:512`、`DR-005:595`、`DR-004:251`、`PROVENANCE:143` | 稳定性表述引用「REV-072 §8」但内容为「4 CPU busy-loop + 全 e2e 并行下 e2e **连续 5 轮** 58/0/2」；REV-072 §8 实际记录为 **e2e 连跑 3 轮**（每轮 58/0/2）+ **4 busy-loop 下 `software_team_v1` 连跑 3 轮**（每轮 11/0/1）+ 空载 workspace 694/0/3 | 若「5 轮 e2e + busy-loop」是作者自跑，补命令/日志并改为「作者自跑」；否则按 REV-072 §8 改为「3 轮 e2e + busy-loop 下 software_team_v1 3 轮」；轮次/作用对象对齐后即可 |
| N-10 | 建议 | `issues:120`（ISSUE-112） | 引用「REV-072 N6/§8」，但 REV-072 仅有 N1–N4（§8 未编号 N5/N6），`N6` 为悬空编号 | 改「REV-072 §8（单 item 加固）」或「REV-072 N2–N3/§8」 |
| N-11 | 建议（跨批） | `PROVENANCE:143`、`07:257`、`08:512`、`DR-004:251`、`DR-005:595`、`progress:5`、`doc/index.md:21` | e2e 已合入 `main`（HEAD `e2cba52`，REV-072 §7/§8 记录与 README 索引行已在 main），docs 分支仍基于 `e7578f1` 且写「e2e 分支待合并」 | docs rebase 时改为「e2e 已合并（main `e2cba52`）」；同步 `doc/index.md`/reviews README 的 REV-072/073 索引并解决冲突 |
| N-12 | 建议 | `progress.md:5` | 「S-1..S-4 + N1–N8 已修」在 N-9 引用微调后需同步（当前为作者视角） | 随 N-9/合并收口更新 |

### 7.7 结论修订

- 首轮 §1 的「结论：有条件通过」经本条**修订为「结论：通过」**：S-1/S-2/S-3/S-4 全部关闭（§7.2；S-2 引用口径微调为 N-9 建议）、N-1…N-8 全部落实（§7.3）、e2e 单 item 校准核实（§7.4）、四维复审 ✅（§7.5）；残留 N-9…N-12 均为**建议/跨批时点**，不阻塞合并。
- 合并前置已满足：评审记录含 `分支: docs/docs-gate-defer-spec` + `结论：通过` + 真实评审人；评审提交后 worktree clean；`check.sh` PASSED；范围仅 `doc/**`+根 README。合并后由 Lead 按 AGENTS §4 清理 worktree/分支并更新 `progress.md`/`issues.md`；docs 分支需在 e2e 合并后 rebase（N-11）。
- 签字：`reviewer-docs` · 2026-10-31（增量复审）。
