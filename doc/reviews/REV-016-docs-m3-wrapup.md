# REV-016 · docs · M3 收尾（进度/台账/评审索引/README 同步）

> **模块声明**：本分支改 5 个文件——`doc/progress.md`、`doc/issues.md`、`doc/reviews/README.md`、`doc/test-baseline.txt`、`README.md`（README 属 **process** 跨模块边界，Lead 已在提交正文显式声明“跨模块授权…由 reviewer-b 复核”）。变更范围 `23123cf..5a1036b`（1 commit，5 files, +91/−58）；作者 `lead`，评审人 `reviewer-b`（**非作者**）。评审按四维 + 可核验性独立核对，未采信自述；**未修改任何正文/台账**（发现项由 Lead 修正）。

- **分支**：`docs/docs-m3-wrapup`
- **作者**：`lead`
- **评审人**：`reviewer-b`
- **变更范围**：`23123cf..5a1036b`（单提交 `docs(docs): M3 收尾——进度/台账/评审索引与 README 同步（REV-016）`；`git diff --stat` = 5 files, +91/−58；base = merge-base = `origin/main` = `23123cf`）
- **评审时间**：2026-10-30
- **关联**：REV-012（worker）、REV-013（py-worker）、REV-014（docs spec 回写）、REV-015（e2e，**待合并**）；ISSUE-020/022/047/061–072；architecture §6.1 里程碑 DoD；AGENTS §5（progress ≤30 行）/§7（台账）；task-6

## 1. 结论

**结论：有条件通过**

内容主体质量高：`doc/progress.md` 26 行（≤30）且四块结构齐全；`doc/issues.md` 72 行台账 ID 1–72 无重号/无缺号、每行 8 列结构一致，`fixed/in-progress/open/wontfix` 状态绝大多数能从 REV-012/013/014/015 与 git 找到依据；`doc/reviews/README.md` 补齐 006–016 索引且与现有记录（000–014）的分支/评审人/日期/结论逐项一致；`README.md` 未夸大 M4/M5（明确 AC-12 未完成、Harness/run/replay 未实现）；`check.sh` 通过、范围仅 5 文件、提交元数据合规。新增 ISSUE-061..072 对 REV-012/013/014/015 的建议覆盖率高（编号/日期/级别总体合理）。

但独立核对发现 **5 项需要修正/确认**（均为文档/台账文本，不涉及代码），其中 4 项为台账/README 事实性或覆盖性错误，1 项为合并顺序前置；因此给「有条件通过」，条件见 §6：

- C1：`doc/progress.md:22` 与 `README.md:21` 把 `ISSUE-063` 列为 **P2**，而台账 `doc/issues.md:71` 为 **P3**（同一 issue 级别自相矛盾）。
- C2：`README.md:51` 声称 `schemas/ | DSL schema（M1）`，但 `schemas/graph.schema.json` **不存在**且 ISSUE-029 仍 open（M1 未实现项）——README 与台账矛盾。
- C3：新增台账**漏收 REV-013 评审的一条重要建议**（#3 整包广播示例口径不一致 / 无 kernel Delivery 级验证）；其余 REV-012/013/014/015 建议均已覆盖。
- C4：ISSUE-014 标 `fixed`，但 spec 02/04 与 architecture 共 10 处仍以「Python gate 延后（ISSUE-014）」作为未决追踪项——台账状态与 spec 引用矛盾。
- C5：**合并顺序前置**（非内容缺陷）：本分支基于 `23123cf`（e2e 尚未合并），当前分支内 `tests/e2e/` 不存在，`progress` 的「M3 全部合并 / cargo 313+ tests」与 `README` 的 `tests/e2e` 行、reviews 索引 REV-015/016 链接均依赖「e2e 先合并 → 本分支 rebase」；需在 rebase 后复核这些行（含索引 REV-016 结论行）。

## 2. 四维评审

| 维度 | 结论 | 依据（文件/行/测试/脚本） |
|---|---|---|
| 功能正确性 | ⚠️ | 台账整体正确（72 行、8 列、状态分布 `fixed 23 / open 39 / in-progress 6 / wontfix 4`），fixed 项大多可回溯到 REV-012/013/014（M3 已合并）与 git；新增 ISSUE-061..072 编号连续、日期 `2026-10-30`、级别总体合理；ISSUE-070 的两条代码/spec 断言经实核真实（`HARNESS_TOOL_ERROR` 见 `crates/zgraph-kernel/src/types.rs:106,347` vs spec 00 §0.4 `TOOL_ERROR`；`result_summary` spec 00:113 为 `Value`，实现 `Option<Value>` 见 `kernel/src/trace.rs:54`）。扣分：C1 级别矛盾、C2 README 资产声明与 ISSUE-029 矛盾、C3 覆盖遗漏。 |
| 代码结构合理性 | ✅ | 仅 5 个预期文件，未改代码/测试/CI/脚本；`doc/progress.md` 26 行 ≤30 且保留四块；`doc/issues.md` 每行 8 列、模块列允许复合标签（e2e/kernel、docs/process 等）；`doc/reviews/README.md` 仅追加 006–016 与跨模块说明；`doc/test-baseline.txt` 仅追加注释行——`merge.sh:100` 用 `grep -v '^#'` 解析基线，注释不会破坏门禁；README 属 process 边界但 Lead 已在提交正文声明，本评审独立复核。 |
| 逻辑性 | ⚠️ | 状态与证据链总体自洽：ISSUE-020 in-progress（M0–M2 已回写，05/08 随 M4）、ISSUE-047 in-progress（M3 e2e 覆盖 AC-06/07/09/10，AC-08/12 待 M4/M5）、ISSUE-022 open（示例层已落地、spec/AC 转 ISSUE-067）等保留状态**合理**；ISSUE-023/025/026/027/034/043/046/050/053/054 的 fixed 均有 REV 证据。扣分：ISSUE-014 fixed 与 spec 引用矛盾（C4）、ISSUE-063 级别不一致（C1）、REV-013 #3 遗漏（C3）。 |
| 可读性 | ✅ | progress 分块清晰、下一步 3 条与阻塞单行；台表单行信息密度可读、建议列均含落点/REV；README「当前状态/未实现/目录」结构清楚，未出现夸大 M4/M5 的表述（AC-08/12 明确未完成）。小项：README「CI 全绿」缺仓库内可核验证据（建议）、ISSUE-017 数字与 REV-015 复审记录不一致（建议）。 |

## 3. 发现清单

| # | 级别 | 位置 | 问题 | 建议 | 状态 |
|---|---|---|---|---|---|
| 1 | 重要（C1） | `doc/progress.md:22`、`README.md:21` vs `doc/issues.md:71` | 同一 `ISSUE-063`（AC-07 字面 ticker）在 progress/README 被归入「M3 遗留 **P2**」，台账却是 **P3**；进度/门面文档与唯一台账矛盾。 | 统一口径：建议按台账 **P3** 修改 progress/README 文案（如「M3 遗留项」不再标 P2），或若判定确为 P2 则把 `ISSUE-063` 级别改 P2 并同步依据；二选一后两处一致。 | open |
| 2 | 重要（C2） | `README.md:51` vs `doc/issues.md:37`（ISSUE-029） | README 目录表写 `schemas/ \| DSL schema（M1）、worker 协议 v1…`，但 `schemas/graph.schema.json` **不存在**（`ls schemas/` 仅 `worker-protocol-v1.md`），ISSUE-029 仍 open 且明确列为未实现项——README 夸大了 M1 资产完成度。 | 改为「`schemas/worker-protocol-v1.md`（协议 v1，M0 冻结/M3 检查单闭环）；DSL JSON Schema 未实现（ISSUE-029）」或等价措辞；不影响 M0–M3 完成声明。 | open |
| 3 | 重要（C3） | `doc/issues.md`（新 ISSUE-061..072 覆盖矩阵） | **REV-013 #3 未被任何新 ISSUE 覆盖**：REV-013 记录 §3 #3 指出「整包广播口径不一致（`echo`/`parallel` 透传 vs `slow`/`planer`/`archer` 解包）+ 4 条子进程 e2e 喂人工已解包 inputs、未走 kernel Delivery」，属重要级建议（REV-013 §5 建议登记 ISSUE）。抽查其余 REV-012/013/014/015 建议：065（协议 §7）、066（signal/exit）、067（04 API + ISSUE-022 子项）、068（nix）、069（WorkerRun）、071（REV-013 其余 6 条）、072（REV-015 建议）均已覆盖——仅缺该条。 | 在 `ISSUE-071` 追加该条（或新增 `ISSUE-073`）；可顺带把 REV-013 #1 的「cycle.py docstring 前置条件」并入 `ISSUE-062` 描述。 | open |
| 4 | 重要（C4） | `doc/issues.md:22`（ISSUE-014 fixed） vs spec/architecture 10 处引用 | ISSUE-014 已标 `fixed`（v1 决策为内置 op，REV-014 已回写），但 `doc/spec-v1.2/02-DSL与校验.md:95,489,495,875`、`04-节点接口与扩展点.md:406,558,570,641,692`、`doc/architecture-v2.md:20` 仍写「Python gate 延后（ISSUE-014）/ 见 ISSUE-014」——读者会把已 fixed 的 issue 当作未决 deferral，状态与正文矛盾。 | 二选一：① 保持 ISSUE-014 `in-progress` 或新增「Python gate 未来实现」ISSUE，并把 spec/arch 的「ISSUE-014」引用改指新 ISSUE；② 保留 fixed 但把 spec/arch 引用改为「v1 已决策 D6；Python gate 为后续扩展位（决策见 ISSUE-014 fixed 记录）」并在台账备注指向。 | open |
| 5 | 重要（C5，合并顺序前置） | 分支 base `23123cf`；`doc/progress.md:5,26`、`README.md:18,52`、`doc/reviews/README.md:22-23` | 本分支基于 23123cf（e2e **尚未合并**）：当前分支内 `tests/e2e/` 不存在（已验证），但 progress 写「M3 全部合并 / cargo 313+ tests」、README 目录列 `tests/e2e/`、reviews 索引链接 REV-015/016——若 wrapup 先于 e2e 合并会事实不符/死链。Lead 已声明稍后由 merge.sh rebase，本项为**合并前置**而非内容缺陷。 | 确保合并顺序 **e2e → wrapup rebase**；rebase 后复核：`tests/e2e/` 存在、`cargo test --workspace` 含 e2e（reviewer-a 实测 313 passed）、reviews 索引 REV-015 文件存在、`REV-016` 行结论与最终记录一致（条件关闭后改「通过」）。 | open |
| 6 | 建议 | `doc/issues.md:76`（ISSUE-068）与 `:80`（ISSUE-072 第 2 条） | nix 提升 `[workspace.dependencies]` 同时在 ISSUE-068 与 ISSUE-072 的建议里重复登记（非矛盾但重复）。 | ISSUE-072 该条交叉引用「见 ISSUE-068」，或合并到 068（保留 072 其余 e2e 项）。 | open |
| 7 | 建议 | `doc/issues.md:25`（ISSUE-017） vs REV-015 §7.2 | 台账写「并行 248–295ms、串行 504–558ms（REV-015）」，REV-015 复审记录实测为「并行 268–290ms、串行 490–506ms」；两处数字来自不同轮次/commit 正文，引用同一 REV-015 易被误读。 | 改为「REV-015 复现实测 268–290ms / 490–506ms（多轮波动见记录）」或注明多轮范围。 | open |
| 8 | 建议 | `doc/issues.md:30`（ISSUE-022） | ISSUE-022 原问题「声明尚未写入节点契约与验收」仅部分完成（示例层已落地，spec 04/07 转 ISSUE-067），状态仍 `open`；`in-progress` 更准确表达“部分完成且已拆分跟踪”。 | 视 Lead 口径改为 `in-progress`（保留 ISSUE-067 为剩余项）。 | open |
| 9 | 建议 | `README.md:18` | 「双栈测试与 CI 全绿」在仓库内无 CI 运行记录/徽章可核验（仅 `.github/workflows/ci.yml` 与本地测试）；作为门面文档宜表述可核验事实。 | 改为「本地双栈测试全绿；CI 已配置（GitHub Actions：fmt/clippy/test + pytest）」，或附 CI 运行链接/记录。 | open |
| 10 | 建议 | `doc/reviews/README.md:23` | 索引已预写 `REV-016 … 通过 / reviewer-b`，而本记录当前为「有条件通过」；条件关闭前该行与记录不一致。 | 条件期间可暂标「有条件通过（条件见记录）」；C1–C4 关闭、记录追加复审通过段后再改回「通过」。 | open |

> 无阻塞项；C1–C4 为“有条件通过”条件，C5 为合并顺序前置；#6–#10 建议级（可随本轮一并修）。台账/正文修正由 Lead 执行（本评审不改）。

## 4. 验证证据

### 4.1 范围、提交与洁净度

```bash
cd /Users/zyzs/lczyzs/code/zgraph/.worktrees/docs-lead
git branch --show-current           # docs/docs-m3-wrapup
git merge-base origin/main HEAD     # 23123cfa730163b63adaccdc03c0313006156812 == origin/main
git log --oneline -2                # 5a1036b → 23123cf
git diff --stat 23123cf..HEAD       # 5 files changed, 91 insertions(+), 58 deletions(-)
git diff --name-only 23123cf..HEAD  # README.md doc/issues.md doc/progress.md doc/reviews/README.md doc/test-baseline.txt
git status --porcelain              # 空
```

- `5a1036b` author/committer = `chenyuxiangg <1007423165@qq.com>`；subject `docs(docs): ...`；正文含 `Review: REV-016` / `Agent: lead`，并含 `跨模块授权：…README.md（process 边界）…reviewer-b 复核（REV-016）` 声明。
- 未改 `crates/**`/`python/**`/`tests/**`/`.github/**`/`scripts/**`；未改 `doc/spec-v1.2/**`/`doc/architecture-v2.md`。

### 4.2 必跑命令与结果

```bash
bash scripts/dev/check.sh           # CHECK PASSED，exit 0（12 项 OK）
PYTHONPATH=python .venv/bin/python -m pytest   # 44 passed（base=23123cf 已含 REV-013 py-worker）
wc -l doc/progress.md               # 26（≤30，四块结构齐全）
```

### 4.3 台账完整性/一致性（脚本核对）

| 项 | 结果 |
|---|---|
| 行数与 ID | **72 行**，ID `ISSUE-001..072` 连续，**无重号、无缺号** |
| 列结构 | 每行 **10 个 `|` 分段（8 列）**，与表头一致；模块列复合标签（e2e/kernel、docs/process、kernel/e2e 等）与表头说明（ISSUE-011 先例）一致 |
| 状态分布 | `fixed 23 / open 39 / in-progress 6 / wontfix 4`（日期：2026-10-28 ×12、10-29 ×48、10-30 ×12） |
| 新增 061–072 | 12 行、日期均 `2026-10-30`、级别 P2×2（061/066）/P3×10；编号与新覆盖矩阵见 §4.4 |
| fixed 证据抽查 | ISSUE-023→`merge.sh:137-138` 用 `merge-base --is-ancestor` + `branch -D`（已修）；ISSUE-015/025→REV-012 §4.4/§7；ISSUE-026/027/034/043/046/050/053/054→REV-014 回写（PROVENANCE hash 一致）；ISSUE-004→`doc/test-baseline.txt` 归零（`merge.sh:100` 忽略 `#` 注释，注释安全）；ISSUE-047/052→REV-015（待合并） |
| 保留状态 | ISSUE-020 in-progress（M0–M2 已回写，05/08→067/070）、ISSUE-047 in-progress（AC-08/12 待 M4/M5）、ISSUE-022 open（示例层已落地→067）——**合理**（ISSUE-063 级别矛盾见 §3 #1） |

### 4.4 新增 ISSUE 覆盖矩阵（对照 REV-012/013/014/015 要求登记的条目）

| 来源 | 覆盖情况 |
|---|---|
| REV-012（reviewer-a #13/#14 + reviewer-b §5.1.1） | 065（schema §7 六项限制）、066（NODE_WORKER_CRASH signal/exit）、067（spec 04 API + ISSUE-022 子项）、068（nix workspace.dependencies）、069（WorkerRun pub struct）；#15 已由 `67a60a2` 修复（非台账项） |
| REV-013 | 062（valid_cycle 可执行回环）、067（L3 契约/AC 回写）、071（SDK 重复/software_team 流程/`again is not False`/恒真断言/AGENTS `python/tests` 等 6 条）；**#3 整包广播示例口径缺失（C3）**；#1 docstring 前置条件可并入 062 |
| REV-014 | 条件已全部关闭（最终复审通过），无遗留要求；本批另由 REV-012 建议落 067/068 |
| REV-015 | 061（gated self-loop 幻影 attempt）、062（valid_cycle）、063（AC-07 字面 ticker）、064（AC-09 覆盖语义）、072（raw fixture/nix/PID/CLI 退出码等）；并在 047/052/054 备注更新；ISSUE-029 等状态未变 |

### 4.5 评审索引与 README 事实核对

- `doc/reviews/README.md`：**17 行数据（000–016）**；000–014 行与 `doc/reviews/` 实际文件、各记录「评审人/评审时间/分支/结论」逐项一致（006–011 均为 reviewer-a / 2026-10-29；012 reviewer-a；013/014 reviewer-b）；REV-015/016 文件当前不在本分支（e2e 待合并 / 本记录待提交），属 C5 合并顺序前置。
- `README.md` 事实核对：M0–M3 已完成、M4–M5 明确未实现（Harness、`run`/`replay`、AC-12）——与代码事实一致；`schemas/` 行夸大 DSL schema（C2）；`tests/e2e/` 依赖 e2e 合并（C5）；`crates/zgraph-worker` 描述 supervisor/WorkerExecutor（M3）与 REV-012 实码一致；`python/workers` 示例与 REV-013 一致；`tests/fixtures/` 存在。
- ISSUE-070 两条断言独立实核为真：`HARNESS_TOOL_ERROR`（kernel types.rs:106,347）vs spec 00 §0.4 `TOOL_ERROR`；`result_summary` spec 00:113 `Value` vs 实现 `Option<Value>`（kernel trace.rs:54 / trace model.rs:205）。

### 4.6 合并顺序前置说明

当前分支 base=`23123cf`，`tests/e2e/` 不存在（`ls tests/e2e` → absent）；`progress` 的「M3 全部合并 / 313+ tests」与 README/e2e 行、索引 REV-015/016 均以 e2e 已合并为前提。Lead 计划「e2e 先合并，wrapup 由 merge.sh rebase」——本评审按该前提给出 C5 复核清单；内容本身不构成缺陷。

## 5. 遗留问题

- **条件（合并前关闭，由 Lead 修正正文/台账；本评审不改）**：C1 ISSUE-063 级别统一；C2 README `schemas/` 措辞；C3 补登 REV-013 #3（并入 ISSUE-071 或新增）；C4 ISSUE-014 状态与 spec/arch 引用统一；C5 e2e→wrapup 合并顺序 + rebase 后复核（含索引 REV-016 行）。
- **建议（可随本轮修）**：#6 ISSUE-068/072 nix 去重；#7 ISSUE-017 数字对齐 REV-015；#8 ISSUE-022 → in-progress；#9 README「CI 全绿」措辞；#10 索引 REV-016 行条件期标注。
- **未在本分支可核验**：CI 全绿（无运行记录）、313+ tests（e2e 未合并前本分支为 294；合并后成立）——已作为 C5/#9 处理。
- 本评审未修改任何正文/台账/README/代码；未 merge/push/rebase。

## 6. 条件关闭记录（仅“有条件通过”填写）

| 条件 | 内容 | 关闭证据 | 关闭日期 |
|---|---|---|---|
| C1 | 统一 ISSUE-063 级别（progress/README 的 P2 vs 台账 P3） | 待 Lead 修正并给出最终口径 | — |
| C2 | README `schemas/` 行移除/修正“DSL schema（M1）”未实现声明 | 待 Lead 修正 README:51 | — |
| C3 | 补登 REV-013 #3（整包广播示例口径/无 kernel Delivery 级验证）到 ISSUE-071 或新 ISSUE | 待 Lead 追加台账行 | — |
| C4 | ISSUE-014 fixed 与 spec 02/04 + architecture 引用统一（保留 in-progress 或改引用到新 ISSUE） | 待 Lead 决策并同步台账/正文 | — |
| C5 | 合并顺序：e2e 先合并 → wrapup rebase；复核 progress/README/索引的 e2e/313/REV-015/016 引用与最终结论行 | 待 e2e 合并 + rebase 后由 Lead/复审确认 | — |

### 6.1 复审（2026-10-30，reviewer-b，HEAD `4de4ae8`；rebase 后 base=`542a911`）

**复审范围**：作者修正提交 `4de4ae8`（`bdca974..HEAD` = 3 files, +9/−9：`README.md`/`doc/issues.md`/`doc/progress.md`；`doc/reviews/README.md` 与 `doc/test-baseline.txt` 本次未改）+ rebase 后全量复核。**独立看文件 + 亲跑测试，不采信自述**。

| 条件 | 结果 | 关闭证据 |
|---|---|---|
| C1 | ⚠️ **未完全关闭**（progress ✅ / README ❌） | `doc/progress.md:22` 已改为逐项级别「ISSUE-061，P2 / ISSUE-066，P2 / ISSUE-063，**P3**」，与台账一致；但 **`README.md:21` 仍为「M3 遗留 P2 见 doc/issues.md（ISSUE-061/063/066 等）」**，仍把 P3 的 ISSUE-063 归入 P2——修正提交未改该行。 |
| C2 | ✅ 已关闭 | `README.md:51` 已改为「worker 协议 v1（M0 冻结，M3 检查单闭环）；DSL JSON Schema 未实现（ISSUE-029）」，与 `schemas/` 实际内容及 ISSUE-029 一致。 |
| C3 | ✅ 已关闭 | `doc/issues.md:79`（ISSUE-071）已补 REV-013 #3（整包广播示例口径不一致 / 4 条子进程用例未经 kernel Delivery 级验证）；`doc/issues.md:70`（ISSUE-062）已补 `workers.cycle` docstring 前置条件（REV-013 #1）。 |
| C4 | ✅ 已关闭 | `doc/issues.md:22` ISSUE-014 状态改回 `in-progress`，建议列注明「v1 内置 op 已定稿并回写；自定义 Python 门控延后，spec 02/04 与 architecture 共 10 处仍引用 ISSUE-014」——与 spec/arch 引用一致，不再自相矛盾。 |
| C5 | ✅ 已关闭 | rebase 后 `base=origin/main=`**`542a911`**；`tests/e2e/` 已存在（5 个 test 文件）、`doc/reviews/REV-015-*.md` 已随合并到位；reviews 索引 REV-015/016 链接指向实际文件；**独立 `cargo test --workspace` = 313 passed / 0 failed**、`pytest` = 44 passed → progress「313+」口径成立。 |
| 建议 #6–#10 | ✅ #6–#9 已落实；#10 待最终关闭 | #6 ISSUE-068 已交叉引用 ISSUE-072；#7 ISSUE-017 数字已对齐 268–290ms / 490–506ms（REV-015 §7.2）；#8 ISSUE-022 已改 in-progress；#9 README「CI 全绿」已改为「本地双栈测试全绿…CI 已配置」；#10 索引 REV-016 行仍预写「通过」，待 C1 关闭后成立。 |

**复审验证命令与结果**

```bash
cd /Users/zyzs/lczyzs/code/zgraph/.worktrees/docs-lead
git rev-parse HEAD                      # 4de4ae89f24f65d16a38113b08741160ebbdc079
git merge-base origin/main HEAD         # 542a911ef4ea7ca09ec09828fb4266feeed2d828 == origin/main（已 rebase）
git diff --stat bdca974..HEAD           # 3 files changed, 9 insertions(+), 9 deletions(-)
cargo test --workspace                  # 313 passed / 0 failed
PYTHONPATH=python .venv/bin/python -m pytest   # 44 passed
wc -l doc/progress.md                   # 26（≤30；当前阶段/进行中/下一步/阻塞 四块齐全）
bash scripts/dev/check.sh               # CHECK PASSED
git status --porcelain                  # 空
```

- 台账结构：脚本核对 **72 行、ID `ISSUE-001..072` 无重号/缺号、每行 8 列**；状态分布 `fixed 22 / open 38 / in-progress 8 / wontfix 4`（ISSUE-014/022 改 in-progress 后一致）。
- 索引一致性：`doc/reviews/README.md` **17 行（000–016）** 与 `doc/reviews/` 实际文件（REV-000..016 均存在）一致；REV-015/016 链接有效。
- 范围/元数据：`542a911..HEAD` 共 6 files = 5 个收尾文件 + 本评审记录；`4de4ae8` author/committer=`chenyuxiangg <1007423165@qq.com>`，正文含 `Review: REV-016` / `Agent: lead`；README 属 process 跨模块改动仍在 Lead 提交正文的授权声明范围内。

**复审结论：仍有条件（C1 README:21 未同步；C2–C5 已全部关闭）**

剩余唯一条件（一行文本修正，无需其它改动）：把 `README.md:21` 的「M3 遗留 **P2** 见…（ISSUE-061/063/066 等）」改为逐项/正确级别，例如：

> legacy 退役决策；M3 遗留项见 [`doc/issues.md`](doc/issues.md)（ISSUE-061 P2 / ISSUE-063 P3 / ISSUE-066 P2 等）。

修正后（索引 REV-016 行的「通过」即成为最终状态）请回我复审；我追加 §6.2 并将本记录结论转「通过」。

### 6.2 最终复审（2026-10-30，reviewer-b，HEAD `35df83d`；base=`542a911`）

**复审范围**：`a79c6e3..HEAD` = `35df83d`（`docs(docs): 修正 README M3 遗留级别标注（REV-016 C1 收尾）`，**1 file, +1/−1，仅 README**）；独立看文件 + 亲跑测试，不采信自述。

**C1 关闭证据（最终）**

| 位置 | 当前内容 | 与台账一致 |
|---|---|---|
| `README.md:21` | 「legacy 退役决策；M3 遗留项见 [`doc/issues.md`](doc/issues.md)（ISSUE-061 P2 / ISSUE-063 **P3** / ISSUE-066 P2 等）。」 | ✅ |
| `doc/progress.md:22` | 「M3 遗留：gated self-loop 幻影 attempt（ISSUE-061，P2）、NODE_WORKER_CRASH 记录 signal/exit（ISSUE-066，P2）、AC-07 字面 ticker（ISSUE-063，**P3**）」 | ✅ |
| `doc/issues.md:71`（ISSUE-063） | 级别 **P3** | ✅（基准） |

- 全仓 grep「M3 遗留 P2」→ **0 命中**（仅本记录 §6.1 的历史文字保留旧表述，非线上文档）；不再有把 ISSUE-063 归入 P2 的标注。
- **C1 关闭。**

**C2–C5 维持关闭（复核确认，本提交仅 README 一行，未回退）**

| 条件 | 复核结果 |
|---|---|
| C2 | `README.md:51` 仍为「worker 协议 v1…；DSL JSON Schema 未实现（ISSUE-029）」✅ |
| C3 | `doc/issues.md:79` ISSUE-071 含 REV-013 #3；`:70` ISSUE-062 含 workers.cycle docstring 前置条件 ✅ |
| C4 | `doc/issues.md:22` ISSUE-014 = `in-progress`（与 spec 02/04 + architecture 10 处引用一致）✅ |
| C5 | base=`origin/main`=`542a911`（已 rebase，可 ff）；`tests/e2e/` 5 个 test 文件到位；`doc/reviews/README.md` **17 行（000–016）**与 `doc/reviews/` 实际文件逐一对应（REV-000..016 均存在）；`doc/progress.md` **26 行**（≤30，四块齐全）；`doc/issues.md` **72 行、ID 1–72 无重号/缺号、每行 8 列** ✅ |

**最终复审验证命令与结果**

```bash
cd /Users/zyzs/lczyzs/code/zgraph/.worktrees/docs-lead
git rev-parse HEAD                      # 35df83d8a32ec90ff2907bf6445fc9f095d3fc7f
git diff --stat a79c6e3..HEAD           # 1 file changed, 1 insertion(+), 1 deletion(-)（仅 README.md）
cargo test --workspace                  # 313 passed / 0 failed
PYTHONPATH=python .venv/bin/python -m pytest   # 44 passed
wc -l doc/progress.md                   # 26
bash scripts/dev/check.sh               # CHECK PASSED
git status --porcelain                  # 空
```

- 提交元数据：`35df83d` author/committer=`chenyuxiangg <1007423165@qq.com>`，正文含 `Review: REV-016` / `Agent: lead`；README 属 process 跨模块改动仍在 Lead 提交正文的授权声明范围内。
- 范围：`542a911..HEAD` = 6 files（5 个收尾文件 + 本评审记录），无其它改动。

**复审结论：通过**

- 原 §1「有条件通过」及 §6/§6.1 历史保留；C1–C5 **全部关闭**，索引 REV-016 行「通过」成为最终状态。
- 本记录满足 `merge.sh` 门禁：`分支: docs/docs-m3-wrapup` + 「结论：通过」，分支已基于最新 `origin/main`（`542a911`）可 ff；`merge.sh` 会重跑 `cargo test --workspace`（本轮独立 313 passed / 0 failed）。
- 本评审未修改任何正文/台账/README/代码；未 merge/push。
