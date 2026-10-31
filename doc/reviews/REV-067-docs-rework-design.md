# REV-067 · docs · W2a0 返工设计冻结（DR-005 + spec 00/01/02/03/04/06/07/08 + SOP v1.1）

> 独立评审记录：评审人 `reviewer-docs`，**≠ 作者**（AGENTS §3）。本记录只写评审结论与证据，不含对 spec/设计稿/代码的修改。
> 评审基准：用户/Lead 2026-10-31 冻结语义（defer 边、R15、attempt-per-activation、per-port 继承、`settled_by_node` 重置、`end` 特例、receipt 不消费、dormant/确定性死锁、trace v4、P v2、SOP 双 profile、AC-14、v0.1.0 兼容硬约束）；这些语义不再作为「设计争议」，本记录只核对文档是否准确、自洽、可实施。

- **分支**：`docs/docs-rework-design`
- **作者**：`docs-writer`（`27188f2`（DR-005 + SOP v1.1）、`09027dc`（spec 00/01/02/03）、`2f0bb22`（spec 04/06/07/08 + 索引/溯源）、`749e9ee`（REV-067 条件修复：C1/C2/S1–S3 + N-1…N-11，11 files +192/−118）；正文均含 `Review: REV-067` / `Agent: docs-writer`）
- **评审人**：`reviewer-docs`（**≠ 作者**；独立评审；首轮 2026-10-31 于 `2f0bb22`，增量复审同日于 `749e9ee`，见 §7）
- **变更范围**：`7f88614..749e9ee`：**设计/规格/索引 13 files +1311/−160**（首轮 `2f0bb22`：13 files +1228/−151；增量修复 `749e9ee`：11 files +192/−118，见 §7）；另评审记录 2 files（+182）。首轮明细：新增 `doc/decisions/DR-005-w2a-defer-gate-freeze.md`（+700）、`doc/design/software-team-sop-v1.md`（+151/−49，升 v1.1）、spec `00`（+18/−3）、`01`（+47/−6）、`02`（+52/−35）、`03`（+89/−12）、`04`（+9/−1）、`06`（+28/−16）、`07`（+73/−5）、`08`（+19/−0，只追加 §8.9）、`doc/spec-v1.2/README.md`（+14/−13）、`PROVENANCE.md`（+25/−9）、`doc/index.md`（+3/−2）；无代码/测试/配置/`CLAUDE.md`/`doc/progress.md`/`doc/issues.md` 改动
- **评审时间**：2026-10-31
- **关联**：DR-004 §3 W2a0 / §5 D1 / §3 W2a DoD / §6.3；DR-005（本批冻结稿）；REV-063 **C9/C11/S12**；ISSUE-100/101/062；REV-064/065/066（W1 先例）；spec v1.2 分册 00/01/02/03/04/06/07/08；`doc/design/software-team-sop-v1.md` v1.1；`v0.1.0` tag `5d43c58`

## 1. 结论

**结论：通过**（2026-10-31 增量复审修订：首轮结论为「有条件通过」；C-1/C-2/S-1/S-2/S-3 已按 §7.2 证据全部关闭，N-1…N-11 已落实/登记，残留仅 §7.5 的非阻塞建议与 Lead 台账待办）

方向判定：本批把 DR-004 §3 W2a0 的 7 项冻结内容（P v2、返工图模式、dormant/quiescence、语法、上限、trace/回放、兼容）与 Lead 2026-10-31 裁决（per-port 继承、重入清空 `settled_by_node`、`end` 特例、receipt 不消费、E-1 闭迹判定）**逐项落到 DR-005 与 00/01/02/03/04/06/07/08、SOP v1.1 与 AC-14**；R15/`DSL_DEFER_NOT_REENTRANT`（17→18 码）、执行顺序 `R1→R13→R15→R14`、attempt-per-activation 决策矩阵、trace schema v4、AC-14 a–h、v0.1.0 兼容矩阵（B6/d 条）均已成文；13 个文件全部在允许范围、无越界，PROVENANCE 9 个分册 SHA-256 与行数实测一致（首轮 6308/6568；修订后 6334/6604，见 §7.1），`check.sh` PASSED。作为 W2a 实现依据**主体可实施**。

首轮独立核对发现 **2 条阻塞 + 3 条重要**问题（不影响 DR-004 方向），当时判「有条件通过」。条件如下，**均已于 2026-10-31 在 `749e9ee` 关闭（证据见 §7.2）**：

1. **C-1（阻塞）**：03 §3.4.2/§3.4.3 的应用伪代码与 DR-005 §3.3、现行 kernel 实现三处不一致（passed 边 emit 时机、END_TERMINAL 是否 emit、超限与自环超限「同序」），且循环体缺 `if status.is_terminal() return`；需按 DR-005 §3.3 对齐并分别冻结 v0.1.0 自环超限顺序（见 #C1）。**✅ 已关闭（§7.2 C-1）**
2. **C-2（阻塞）**：03 §3.4.3 `resolve_defer_reentry` 第一分支守卫 `edge.on_gate_false != Defer or …` 把 **settle 边**也纳入 attempt-per-activation，与 01 §1.2.2:89 / B6 冲突；需加 defer 前置条件并补矩阵行（见 #C2）。**✅ 已关闭（§7.2 C-2）**
3. **S-1（重要）**：DR-005 §7 a / 07 AC-14 a 的 `gate_results` 断言把 attempt=1 行写成 `deferred=0`；该行实为 defer 命中（`deferred=true`），按现文实现的 e2e 会失败（见 #S1）。**✅ 已关闭（§7.2 S-1）**
4. **S-2（重要）**：迁移历史事件的 payload 口径未限定——v3→v4 只给 `gate_results` 表补列，旧 `events.payload` 仍是 v3 形态；06 §6.10.4 / 07 AC-14 h「v4 库 `trace --json`/L1 含三键」对迁移后的历史行不成立（见 #S2）。**✅ 已关闭（§7.2 S-2）**
5. **S-3（重要）**：W2a 波及 **cli** 模块（`bridge.rs::map_gate_result` 等）未进入 DR-005 §8 实施计划/写作用域；trace 分支已由 Lead 特批临时填 `deferred=false/attempt=0/verdict=None`（REV-070），kernel 分支必须把真实值接回，否则 trace/e2e 三字段恒为默认值（见 #S3）。**✅ 已关闭（§7.2 S-3）**

其余 11 条为**建议**（N-1…N-11），首轮不阻塞合并；**均已在 `749e9ee` 落实或登记编号（见 §7.3）**，其中台账登记由 Lead 收口（§7.5 R8）。

## 2. 四维评审

| 维度 | 结论 | 依据（文件/行/实测） |
|---|---|---|
| 功能正确性 | ⚠️（C-1/C-2/S-1/S-2 备注） | 冻结语义 A–G 逐项覆盖：DSL 边字段/白名单/类型桶/hash 投影（DR-005 §2、02 §2.2/§2.8/§2.9）、R15 与 `DSL_DEFER_NOT_REENTRANT`（00 §0.4 实测 18 行、02 §2.4.1 R15）、执行顺序 `R1→R13→R15→R14`（02 §2.1.2/§2.1.3/§2.4.1、06 §6.10.2、DR-005 §2.5）、运行期决策矩阵/defer 挂账/attempt-per-activation/end/自环/超限/dormant/确定性死锁（03 §3.4.3/§3.9.3/§3.10、01 §1.4.1）、trace v4 DDL/迁移/回放（06 §6.2.2/§6.2.8/§6.7/§6.10）、P v2（DR-005 §5、SOP §5.2）、SOP 双 profile/边表/端口/预算/codegen（SOP §3.1/§3.2/§6/§7.1/§7.2）、AC-14 a–h（DR-005 §7、07 §AC-14）、v0.1.0 兼容（B6/§9/AC-14 d）。缺口：C-1/C-2 属 03 应用算法与冻结/现行语义冲突；S-1 AC-14 a 断言自相矛盾；S-2 迁移历史事件的 payload 断言不可执行。其余实现细节与并行分支（dsl `01b3d28`/REV-069、trace `96ab28e`/REV-070、py-worker `137d68d`/REV-068）一致 |
| 代码结构合理性（文档/计划结构） | ⚠️（S-3 备注） | 13 文件范围精确（`git diff --name-status origin/main...HEAD` 逐项核对）；DR-005 §0/§1/§2–§6/§7/§8/§9/§10/§11/§12/§13 分层清晰、可执行；spec 回写分工明确（00 枚举/错误码/默认值、01 领域模型、02 DSL/R15/hash、03 调度、04 门接口例外、06 存储、07 验收、08 只追加）；SOP v1.1 以 D12–D15 增量 + §3.1/§3.2 双 profile + §7 双 codegen 保持 approved-only v1 字节级不变（§7.2：`--mode approved` 不写键、hash 三基线不变）；无重复定义（`on_gate_false` 语义只在 DR/02 冻结，01/03/06 引用）。缺口：S-3 cli 适配不在执行计划内，与 trace 分支已发生的 Lead 特批跨模块改动不一致 |
| 逻辑性 | ⚠️（C-1/C-2/N-1 备注） | 主体自洽：R13（`G'` 无 gate 环）与 R15（`G_all` 含 gate 边/自环 SCC）独立且顺序冻结；E-1 闭迹判定与等价实现（`|SCC|≥2`+SCC 内 defer 边，或 defer 自环）一致；dormant/blocked 判定表（目标无实例→确定性 `RUN_DEADLOCK`、Completed/Skipped→dormant、非终态→不静默）与 `is_quiescent` 定义匹配；per-port 继承 + 清空 `settled_by_node` 解决 stale-input 与 node 级 settled 误 Skipped 两条缺陷，且 v0.1.0 无重入实例→不变；receipt 不消费（E-3）/incident 覆盖不删除与 dormant 只看当前实例自洽；attempt 只依赖目标当前实例（自环依赖源）保证确定性；AC-14 a–h 的图骨架（自环、A→B→A、SOP、dormant、blocked、超限、迁移）可实现。缺口：C-1（emit/resolve/fail 顺序）与 C-2（settle 边误入重入）为内部矛盾；N-1「充分但非必要」与 E-1 反例方向相反 |
| 可读性 | ✅（N-8 备注） | 中英文术语一致（defer/settle/attempt/G_all/SCC/DeferredReceipt/dormant）；表格列对齐、代码块语言标注；新增 §号（01 §1.4.1、03 §3.4.3、08 §8.9）均存在且被正确引用（实测 grep 全部命中，无悬空 §号）；无未替换模板占位（唯一例外 DR-005:183 N-3 反例行含「且……」半句，见 N-8）；08 只追加 §8.9、未改写 8.0–8.8 历史快照（diff 19/0）；DR-004 未改（红线） |

## 3. 发现清单

> 级别口径：**阻塞** = 必须修复后才能给「通过」；**重要** = 必须修复或登记 ISSUE；**建议** = 可后置，但需修复或进台账。

| # | 级别 | 位置 | 问题 | 建议 | 状态 |
|---|---|---|---|---|---|
| C1 | 阻塞 | `doc/spec-v1.2/03-调度器与生命周期.md:149-176`（§3.4.2）、`:212-240`（§3.4.3） vs `doc/decisions/DR-005-w2a-defer-gate-freeze.md:290-313`；`crates/zgraph-kernel/src/run.rs:987-1050`、`:1086-1112` | 03 §3.4.2 把 `GateResult` 的 `send/emit` 放在 `resolve_target_instance` **之前**（:154-156 vs :168），并删除了现行循环开头的 `if status.is_terminal() { return; }`；DR-005 §3.3 与现行实现是「通过分支先 resolve 后 emit，resolve 失败（`RUN_DELIVERY_TO_TERMINAL`）不产生 GateResult」（REV-026 明确），END_TERMINAL 在 DR-005 是先 `fail_run` 不 emit；另 DR-005:313 冻结「超限路径与自环超限**同序**（创建 Crashed → GateResult → fail_run）」，但现行自环超限顺序是 `resolve_self_loop` 内 `fail_run` 先于 `emit`（`run.rs:1102` + `apply_publish:1020`）。若按 03/DR 字面统一，会改变无 defer 图（自环超限）的 trace 事件顺序，违反 B6/AC-14 d | 03 §3.4.2 与 DR-005 §3.3 对齐（先 resolve/reentry 再 emit；保留 terminal 短路；END_TERMINAL 是否 emit 二选一写死）；DR-005:313 改为分别冻结「自环超限保持 v0.1.0 现状（create→fail→emit）」与「defer 非自环超限（create→emit→fail）」，各自以测试锁定；复核 `grep -n "emit(record)\|is_terminal" doc/spec-v1.2/03-*.md` 与 `AC-14 d` trace golden | open |
| C2 | 阻塞 | `doc/spec-v1.2/03-调度器与生命周期.md:217`（§3.4.3 `resolve_defer_reentry`） vs `doc/spec-v1.2/01-领域模型与状态机.md:89`、`DR-005 §3.2 表行 1/8` | 守卫 `if edge.on_gate_false != Defer or edge.from != edge.to_node:` 的首个析取项为真时进入 attempt-per-activation 分支；按 03 §3.4.2:168 的调用注释（`resolve_target_instance` 对**所有**放行边调用），settle 边 gate=true 投递到已 `Completed` 目标也会新建 attempt 实例，而 01 §1.2.2:89 / DR-005 决策矩阵要求 `RUN_DELIVERY_TO_TERMINAL`；v0.1.0 带 gate 环/自环回归（B6）会被破坏 | 函数前置 `if edge.on_gate_false != Defer { return None; }`，守卫简化为 `if edge.from != edge.to_node`；决策矩阵补「Settle + passed=true + 目标 Completed → 现行 `RUN_DELIVERY_TO_TERMINAL`」行；kernel 单测覆盖 gated 环 settle 边投递到 Completed 目标 | open |
| S1 | 重要 | `doc/decisions/DR-005-w2a-defer-gate-freeze.md:555`（AC-14 a）、`doc/spec-v1.2/07-验收用例.md:259` vs `01 §1.5`（deferred 定义） | AC-14 a 断言「`gate_results` 两行（`deferred=0`、`attempt` 0/1）」/「attempt 0 与 1 的 `deferred=0` 记录」，但自环第 2 次输出 `continue:false` 使 defer 边 gate=false → `deferred=true`；attempt=1 行应为 `deferred=1`（`deferred = !passed && edge.on_gate_false == Defer`）。e2e 按此断言会失败或写错 golden | 写死两行预期：attempt0 `passed=true/deferred=false`、attempt1 `passed=false/deferred=true`（`edge_index` 同一自环边）；DR-005:555 与 07:259 同步；该用例同时覆盖 attempt-per-activation 与 deferred 记录 | open |
| S2 | 重要 | `doc/spec-v1.2/06-存储与可观测.md:914`、`:983`、`:989-1000`；`doc/spec-v1.2/07-验收用例.md:302-305`；`doc/decisions/DR-005-w2a-defer-gate-freeze.md:433-434` | 迁移历史事件的 payload 口径未限定。`events.payload` 是写入时快照（`crates/zgraph-trace/src/reader.rs:414-465`、`crates/zgraph-cli/src/trace_cmd.rs:319` 直读并输出 payload），v3→v4 只给 `gate_results` 表加列；旧 `GateResult` 事件 payload 仍是 v3 形态（无三键）。因此 06 §6.10.4「（v4 库）」与 AC-14 h「v4 库 `trace --json` 的 payload 含三键」「L1 events 逐字节含新字段」对迁移后的历史行不成立；06:983 的示例则暗示旧 run 也带三键（另见 N-7 的 `verdict:false` 问题） | 明确：「v4 后新写入事件 payload 含三键；v3→v4 迁移仅补 `gate_results` 列默认值，历史事件 payload 保持 v3 形态（`trace --json` 不合成默认键）」。AC-14 h 拆为：旧行用 SQL 断言 `deferred=0/attempt=0/verdict_json=NULL`；新 defer run 用 `trace --json`/L1 断言三键；06 §6.10.4 行加「新写入事件」限定 | open |
| S3 | 重要 | `doc/decisions/DR-005-w2a-defer-gate-freeze.md:584-592`（§8 实施顺序）、`:594-597`（写作用域）、`:12`/`:677-679`（不修改清单） vs `crates/zgraph-cli/src/bridge.rs:589-605`（trace 分支 `96ab28e` 已临时填 `deferred:false/attempt:0/verdict:None`，REV-070 跨模块声明） | `trace::GateResultRecord` 扩展后，cli 必须改 `bridge.rs::map_gate_result`（kernel→trace 映射）、`run_cmd.rs` 测试字面量、`tests/trace.rs` 断言才能编译/联动；但 DR-005 §8 模块拆分与写作用域未列 `cli`，W2a 波次表（DR-004 §4）同样无 cli。trace 分支已由 Lead 特批临时 dummy；kernel 分支必须把**真实值**接回 bridge，否则 trace/e2e 三字段恒为 `0/false/None` | DR-005 §8 增「cli 适配」项（bridge 真实接线由 kernel 分支交付，注明「Lead 指定第二评审/跨模块声明」；或由 trace 分支在 kernel 合并后回填）；§12.1 波及文件清单同步；W2a 合并前以「defer 图 `trace --json` 的 `deferred=true`/`attempt`/`verdict` 非默认值」为验收 | open |
| N1 | 建议 | `doc/decisions/DR-005-w2a-defer-gate-freeze.md:159`（§2.5.2 环语义口径）与 §11.2 E-1（`:649`，「简单环语义下是保守充分条件」） | 方向错误：E-1 反例满足「`|SCC|≥2` 且 SCC 内含 defer 边」但**不存在**过 u 且含该 defer 边的简单环；即 SCC 判定对「简单环」性质是**必要但非充分**，且相对简单环口径是**放宽**而非「保守」。现文写「充分但非必要」「保守判定/保守充分条件」与自身 E-1 反例矛盾（E-1 末句「按冻结判定**通过**」即放宽的证明） | 两处统一改为「按简单环口径会更严；本规则以闭迹为准（E-1 为放行例）」，删除「充分/必要/保守」或精确写明蕴含方向；实现仍以 §2.5.2 等价判定为准不变 | open |
| N2 | 建议 | `doc/decisions/DR-005-w2a-defer-gate-freeze.md:434`、`doc/spec-v1.2/PROVENANCE.md:122` | 「v3 seed W1 遗留缺口，**Lead 已登记 ISSUE**」未给编号；base `7f88614` 的 `doc/issues.md` 最新为 ISSUE-101..103，`grep -n "schema_v3_seed" doc/issues.md` 无命中，该断言当前不可复核（AGENTS §7） | Lead 补登记并引用编号（如 ISSUE-104），或把措辞改为「待 Lead 登记」；PROVENANCE 同改 | open |
| N3 | 建议 | `doc/spec-v1.2/README.md:30` | 07 行数写 **320**，实测 `wc -l` = **322**（PROVENANCE:136 亦为 322）；合计 6308/6568 两处均与实测一致 | 改为 322（其余 8 个分册行数与实测一致：309/343/930/467/903/1429/1108/497） | open |
| N4 | 建议 | `doc/spec-v1.2/02-DSL与校验.md:405`（R6）、`:432`（`DSL_SUBGRAPH_INVALID` detail 列表）、`DR-005 §2.5.2`（子图段） | 子图内 R15 违规按 R6 现有包装收敛为 `DSL_SUBGRAPH_INVALID`（实现：`crates/zgraph-dsl/src/nodes.rs:1058-1081` + `wrap_subgraph_errors`；并行 dsl 分支 REV-069 实测），但 02 §2.4.2 的 detail 清单无 `defer_not_reentrant`，DR-005 只写「递归执行…R15」未写错误码收敛 | 02 R6 行或 §2.4.2 明写「子图内 R15 → `DSL_SUBGRAPH_INVALID`（`subgraph '<id>': …`）」；补一个子图 R15 夹具（主图夹具 P-1/P-2/N-1/N-2 已在 DR-005 §2.5.4 冻结） | open |
| N5 | 建议 | `doc/spec-v1.2/02-DSL与校验.md:60`（同规则内聚合） vs `:414`（R15 逐条判定）、`DR-005 §2.5`（:159-187）；并行实现 REV-069 | R15 的「同规则聚合 vs 首错」未冻结：02 §2.1.3 通则是同规则聚合全部违反项，但 dsl 实现取 edges 数组序首个违规（REV-069 结论）；多 defer 违规时 `errors` 条数/首错会与 spec 字面不同 | 02 §2.4.1 R15 或 §2.1.3 明示（建议首错，与 R13 风格一致）并补多 defer 违规夹具，W2a 按此断言 | open |
| N6 | 建议 | `doc/spec-v1.2/04-节点接口与扩展点.md:700` | 04 §4.6.4 回传载体仍写 `GateEvaluation { edge_index, passed, error }`，漏 `verdict`；01 §1.7 已新增 `verdict`，DR-005 §4.1 要求随 `GateEvaluation` 回传、事件循环不二次求值 | 改为 `GateEvaluation { edge_index, passed, error, verdict }` 并注明 off-loop 取出 | open |
| N7 | 建议 | `doc/spec-v1.2/06-存储与可观测.md:983` | `trace --json` 示例 `"verdict":false` 与 C1 规则冲突：`gate_fn="gates.hr.need_human"` 非内置 `check_output`/`compare`（v2 D6 只允许内置 op；该示例本身为 M1/M2 历史形态），按 DR-005 §4.1 `verdict` 应为 `null`；`deferred:false/attempt:0` 对旧 run 合理 | 示例改 `"verdict":null`，或把示例 gate 换成内置 op 且路径值恰为 false（并注明来源） | open |
| N8 | 建议 | `doc/decisions/DR-005-w2a-defer-gate-freeze.md:183` | 反例表 N-3 行「…但 `a→b` 是**唯一** defer 边且……（合法，见 P-4）」含未完成省略占位，且该例已在 P-4 列为合法，N-3 行无信息量 | 删 N-3 或写完整（「SCC 内含至少一条 defer 边即可通过，不要求每条 defer 边都在简单环上」） | open |
| N9 | 建议 | `doc/spec-v1.2/01-领域模型与状态机.md:199-200`、`doc/spec-v1.2/03-调度器与生命周期.md:235-239` vs `crates/zgraph-kernel/src/run.rs:353-357`、`:1310-1320` | 「把该 `EdgeIdx` 计入新实例 `settled_sources`」依赖 01 §1.3 的**实例级** `settled_sources`，但现行 kernel `InstanceBuffer` 只有 `values/arrived/required`，skip 判定用**节点级** `settled_by_node`；W2a 若新增该字段，需明确继承的 settle 边记入哪套集合、是否参与 `all_incoming_settled`/`maybe_skip_after_settle`，否则实现口径不一 | DR-005 §3.2/01 §1.4.1 明确：节点级 `settled_by_node` 只由本次投递/结算写入；实例级 `settled_sources` 记录继承来源（是否仅审计/排障、不参与 skip）；kernel 按此实现并加单测 | open |
| N10 | 建议 | `doc/spec-v1.2/00-总纲.md:296` | §0.7 消歧写「defer 边（非 `end`、非自环）→ attempt-per-activation 新实例（**继承前次输入 buffer 快照**）」，未写 per-port；01 §1.4.1/03 §3.4.3 已冻结「只继承入边非 defer 的端口」，00 的「快照」易被误读为全量继承（stale-input 缺陷） | 加限定：「按端口继承（入边 `on_gate_false != defer` 的 `values`/`arrived`；defer 端口不继承）」 | open |
| N11 | 建议 | `doc/spec-v1.2/02-DSL与校验.md:403`（R4 行） vs `crates/zgraph-dsl/src/edges.rs:1-6`（并行 dsl 分支实测） | R4 新增「逐边：键白名单；`on_gate_false?` 类型」归入 R4（L1），而现行实现把边结构解析放在 R8 阶段（`load_graph_value` 中 `parse_nodes`→R5/R6/R7→`parse_edges`），错误码仍按 02 归属；照 R4 字面移动阶段会改变多错图的 `errors[0]`/`error_count` 顺序（v0.1.0 golden 风险） | 统一措辞：错误码按 R4 类型桶归属、结构解析可在 R8 阶段（与 `edges.rs` 头注释一致）；或明确 R4 仅指错误码归属 | open |

## 4. 验证命令与结果

### 4.1 范围、分支、干净度、提交约定

```bash
cd /Users/zyzs/lczyzs/code/zgraph/.worktrees/docs-docs-writer
git status --porcelain --branch        # ## docs/docs-rework-design...origin/main [ahead 3]
git rev-parse HEAD origin/main         # 2f0bb223e6ee4b117255aab54bc78c0975e9315f / 7f886141890a7d6577ec28d0b79b1aaec4a8bb2d
git merge-base --is-ancestor origin/main HEAD && echo base-ok   # base-ok
git log --oneline origin/main..HEAD
# 2f0bb22 docs(docs): W2a0 spec 回写 04/06/07/08 + 索引/溯源
# 09027dc docs(docs): W2a0 spec 回写 00/01/02/03（DSL + kernel 语义）
# 27188f2 docs(docs): W2a0 设计冻结 DR-005 + SOP 设计稿 v1.1
git log origin/main..HEAD --format=%B | grep -c "Review: REV-067"   # 3
git log origin/main..HEAD --format=%B | grep -c "Agent: docs-writer" # 3
git diff --name-status origin/main...HEAD | wc -l                     # 13
git diff --shortstat origin/main...HEAD                               # 13 files changed, 1228 insertions(+), 151 deletions(-)
git diff --name-only origin/main...HEAD | grep -E 'CLAUDE\.md|doc/progress\.md|doc/issues\.md|doc/reviews/|\.rs$|Cargo|scripts/|schemas/' || echo "(none)"
# (none) → 无代码/测试/配置/冻结文件/评审记录改动；13 文件全部为允许范围（DR-005 新增 + SOP + 8 分册 + README/PROVENANCE/index）
git diff origin/main...HEAD --numstat -- 'doc/spec-v1.2/08-审计关闭矩阵.md'   # 19  0  → 08 只追加 §8.9
```

### 4.2 PROVENANCE 实测（SHA-256 / 行数）

```bash
cd doc/spec-v1.2
shasum -a 256 0*.md
# 1877e0c9…53366  00-总纲.md
# 6aacb0e4…c48b   01-领域模型与状态机.md
# ea707c54…21c6   02-DSL与校验.md
# d27e7a3b…2257   03-调度器与生命周期.md
# 2255ae77…f7f1c  04-节点接口与扩展点.md
# 52f35e7b…bf42   05-AgentHarness.md
# 6e7fa224…b9c6   06-存储与可观测.md
# 73c5c426…0ad1   07-验收用例.md
# 020b0203…e917   08-审计关闭矩阵.md
# → 9 个 SHA-256 与 PROVENANCE §3 表逐字一致
wc -l 0*.md | tail -1                                   # 6308
wc -l 0*.md README.md PROVENANCE.md REFERENCES.md | tail -1  # 6568
wc -l 07-验收用例.md                                    # 322
grep -n "W2a0" PROVENANCE.md | head
# §2 第 15 条：2026-10-31 / REV-067 / 分支 docs/docs-rework-design / 各分册改动 / 冲突 C-1…C-7 / E-1–E-5 / v3 seed 由 W2a e2e 交付 / 实测 6308 行 → 记录完整
```

- §3 的 SHA-256 与行数**全部实测一致**（00–08 合计 6308、含 README/PROVENANCE/REFERENCES 6568）；仅 README 行数表 07 的 320 与实测 322 不一致（N-3）。
- §2 第 15 条已记录本批（日期 2026-10-31、范围 W2a0、REV-067、实测行数），满足「后续修订必须记录」规则。

### 4.3 一致性 grep（交叉引用/计数/错误码/§号）

```bash
# 18 个 DSL 码（00 表格行数；02 含 3 个 v1.1 已合并名仅历史注）
grep -c '^| `DSL_' doc/spec-v1.2/00-总纲.md               # 18
grep -o '`DSL_[A-Z_]*`' doc/spec-v1.2/00-总纲.md | sort -u | wc -l   # 21（含 v1.1 合并名 3 个）
grep -n "17 → \*\*18\*\*" doc/spec-v1.2/00-总纲.md         # 命中
grep -n "18 个 DSL 码" doc/spec-v1.2/07-验收用例.md        # AC-04 标题/表行；02 §2.7.1 同为 18
# 执行顺序（02 三处 + 06 + DR-005 + README）
grep -rn "R1→R13→R15→R14" doc/ | wc -l                    # 6 处一致；无 "R1–R14" 残留于 spec 正文（仅 architecture-v2/analysis/历史 08/reviews，属 C-7 残留）
grep -rn "R1–R14\|R1-R14\|17 码\|17 个 DSL" doc/*.md doc/spec-v1.2 doc/design | grep -v "architecture-v2\|analysis/\|reviews/\|08-审计关闭矩阵"   # 空
# AC 范围
grep -rn "AC-01..AC-14" doc/spec-v1.2/00-总纲.md doc/spec-v1.2/07-验收用例.md doc/spec-v1.2/README.md   # 一致
grep -n "AC-14" doc/spec-v1.2/07-验收用例.md | head -3     # 标题/表行/DoD 均有
# trace 版本
grep -rn "TRACE_SCHEMA_VERSION" doc/spec-v1.2/06-存储与可观测.md doc/decisions/DR-005-w2a-defer-gate-freeze.md doc/spec-v1.2/08-审计关闭矩阵.md  # 均为 4；06 §6.7 新库 1/2/3/4 + 只读拒绝 v3
# 新 §号存在性
grep -n "### 1.4.1\|### 3.4.3\|## 8.9" doc/spec-v1.2/01-*.md doc/spec-v1.2/03-*.md doc/spec-v1.2/08-*.md   # 全部存在，无悬空引用
```

- 00/01/02/03/04/06/07/08、spec README、PROVENANCE、doc/index 之间的交叉引用（R15 落点、§2.4.2 模板、§2.7 码数、§2.8.3 hash、§1.4.1、§3.4.3、§6.2.2/§6.2.8/§6.2.12/§6.7、§4.6.4、§8.9、AC-14）**均存在且计数一致**（除 N-3 的行数与 §3 注记细节）。
- `doc/index.md` 已增 DR-005 行、SOP 描述改 v1.1、reviews 范围补 REV-062–066 并注明 REV-067 待评审；`doc/spec-v1.2/README.md` 状态/banner/行数/AC 范围同步（除 07=320）。
- 红线核对：`git diff --name-only` 无 `CLAUDE.md`/`progress`/`issues`/`reviews`/代码/配置；DR-004 未改；08 只追加（19/0）。

### 4.4 仓库自检

```bash
bash scripts/dev/check.sh
# OK user.name/email/remote / OK CLAUDE.md 未改 / OK 暂存区无禁用文件 / OK 分支命名 docs/docs-rework-design
# OK Python 语法 / OK progress.md / OK issues.md / OK progress 15 行 / OK cargo fmt --check / OK git diff --check
# CHECK PASSED   [exit 0]
```

- 本分支为纯文档改动（无 `*.rs`/`*.py`/`Cargo*`/测试/配置 diff），未跑 `cargo test`/`pytest`（合并门禁会执行）；`check.sh` 已覆盖分支名/冻结文件/禁用文件/语法/格式/diff 检查。

### 4.5 实现现状对照（只读，跨 worktree）

```bash
# main（7f88614）无 on_gate_false / schema v4（W2a 未合入）
grep -n "on_gate_false" crates/zgraph-dsl/src/*.rs        # 无命中（main）
grep -n "TRACE_SCHEMA_VERSION" crates/zgraph-trace/src/schema.rs  # = 3（main）
sed -n '987,1050p;1086,1112p' crates/zgraph-kernel/src/run.rs
# apply_publish：passed 分支先 resolve_target（:1016）再 emit（:1020）；循环开头 :997 附近有 status.is_terminal() 短路；
# resolve_self_loop 超限在 :1102 先 fail_run、返回后 :1020 emit → 与 DR-005:313「create→GateResult→fail_run」不同序（C-1 证据）
sed -n '353,357p' crates/zgraph-kernel/src/run.rs         # InstanceBuffer 只有 values/arrived/required（无 settled_sources；N-9 证据）
sed -n '589,605p' crates/zgraph-cli/src/bridge.rs         # map_gate_result 逐字段映射（trace 分支已加 deferred/attempt/verdict 过渡值，C-1/S-3 证据）
# 并行 W2a 分支（未合入；用于确认本批冻结“可实现”，不替代本批评审）
# - dsl `feat/dsl-gate-defer` 01b3d28（REV-069 通过）：OnGateFalse/白名单/hash 投影/R15（G_all+Kosaraju，子图递归收敛 DSL_SUBGRAPH_INVALID）与 DR-005 §2 一致
# - trace `feat/trace-gate-defer` 96ab28e（REV-070 通过）：TRACE_SCHEMA_VERSION=4、v3→v4 三条 ALTER、payload 三键、reader 三字段，与 DR-005 §4/06 §6.2.8 一致
# - py-worker `feat/py-worker-parse-rework` 137d68d（REV-068 通过）：parse_verdict_rework 闭集/comments 缺省 ""/artifact identity，与 DR-005 §5 一致
# - 未发现 kernel/e2e W2a 分支（下一步实现依据即本批 03 §3.4.3 → C-1/C-2 必须先修）
python/workers/software_team/parse_verdict.py            # v1 行为/docstring 异常清单与 ISSUE-101 描述一致；DR-005 §5.1 的冻结口径可实现
```

## 5. 遗留问题

- **2026-10-31 增量复审更新**：首轮 5 条条件（C-1/C-2/S-1/S-2/S-3）已全部关闭（§7.2）；首轮 N-1…N-11 已落实/登记（§7.3）；本轮新增残留建议 R1–R7 与 Lead 台账待办 R8 见 **§7.5**（均非阻塞）。以下为**首轮快照**，供追溯：
- ~~条件关闭后需增量复审（见 §6）；条件全部关闭前结论保持「有条件通过」，不得合并。~~ **（2026-10-31 复审：5 条件已全部关闭、增量复审通过，最终结论见 §1/§7.6）**
- **跨分支/跨批（不在本分支改动，但 W2a 合并前需闭环，建议 Lead 协调）**：
  - trace 分支 **REV-070 S1**：实现 `verdict_json` 读取失败严格报错 vs DR-005 §4.2「视为 NULL 并告警、不让 `trace` 崩」——两边择一（建议 W2a-docs 在 06 §6.2.8/DR-005 改为与其它 JSON 列一致的严格报错，或 reader 补兜底；本批不阻塞，但需在 W2a-docs 收口）。**（复审后：已由 **ISSUE-105** 承接，见 §7.3 / §7.5 R8）**
  - dsl 分支 **REV-069 N1**：R15 `expected`/`actual` 实现与 DR-005 §2.5.2 冻结模板不一致（实现 `actual={from,to_node,on_gate_false}`；冻结要求 `{node_id,scc_size,defer_edges}`）；W2a 合并前需实现对齐模板或本批追加修订并回写 02 §2.4.2。
  - dsl 分支 **REV-069 N7**：子图内 R13/R15 同时命中聚合为 2 条 `DSL_SUBGRAPH_INVALID`，与 02 §2.1.3「跨规则 fail-fast」字面冲突（既有子图聚合口径的延续）；建议 W2a-docs 在 02 §2.6.1/R6 明确「子图内跨规则聚合」例外（与 N-4/N-5 一并处理）。
  - **ISSUE 台账**：v3 seed 缺口（N-2）建议 Lead 开号并回填 DR-005 §4.3/PROVENANCE §2 第 15 条；ISSUE-100 设计层关闭的正式收口、ISSUE-101 承接状态由 Lead 在台账维护（本批不改 `doc/issues.md`，符合范围约定）。**（复审后：已引用 **ISSUE-104**，见 §7.3 / §7.5 R8）**
- **跨册残留（已由 DR-005 §11 C-7 明示，本批不改）**：`doc/architecture-v2.md` 仍写 `R1–R14`/17 码；`doc/analysis/decision-record-01-scope.md` 亦为历史口径（R1–R14/17 码），建议 W2a-docs/process 收口时统一或标注历史。二者不阻塞本批。
- 未发现：越界改动、禁用文件/冻结文件修改、未评审代码、历史记录改写（08 只追加 §8.9、DR-004 未改）、空占位（除 N-8 半句）、悬空 §号/文件引用。

## 6. 条件关闭记录（首轮「有条件通过」的 5 条条件；2026-10-31 全部关闭，证据见 §7.2）

| 条件 | 关闭证据（需可复核） | 关闭日期 |
|---|---|---|
| C-1 03 §3.4.2/§3.4.3 应用算法与 DR-005 §3.3/现行 v0.1.0 顺序对齐（emit 时机、terminal 短路、END_TERMINAL、超限/自环顺序分别冻结） | 03:148-185 与 DR-005:273-342 逐行同构；「超限不产生该次 GateResult」经 `kernel/trace.rs` Emitter 关闭语义 + `run.rs` 现行顺序核实（§7.2） | **已关闭** 2026-10-31（§7.2） |
| C-2 `resolve_defer_reentry` 仅对 defer 边生效；决策矩阵补「Settle + passed=true + 目标 Completed → RUN_DELIVERY_TO_TERMINAL」 | 03:224 defer 前置；03:214 矩阵行；DR-005:251 行 8 + :255 attempt 计算冻结；§7.2 | **已关闭** 2026-10-31（§7.2） |
| S-1 AC-14 a 的 `deferred` 断言修正（attempt0 `deferred=false`、attempt1 `deferred=true`），DR-005 §7 a 同步 | 07:259 / DR-005:586 修订后与 01 §1.5 `deferred` 定义一致；§7.2 | **已关闭** 2026-10-31（§7.2） |
| S-2 迁移历史事件的 payload 口径限定；AC-14 h 断言拆分（旧行 SQL 默认值 / 新 run `--json`+L1 三键） | 06:297-299/06:916/06:985、07:300-307、DR-005:449-462 修订；措辞与 `reader.events` 直读 payload 的实现一致；§7.2 | **已关闭** 2026-10-31（§7.2） |
| S-3 DR-005 §8/§12.1 增 cli 适配项（bridge 真实值接线归属 + 第二评审/跨模块声明） | DR-005:619-631（§8 第 7 行/写作用域/跨模块评审）、:711/:717（§12.1/§12.2）修订；§7.2 | **已关闭** 2026-10-31（§7.2） |

---

## 7. 条件关闭与复审（2026-10-31）

> 复审对象：作者条件修复提交 **`749e9ee`**（`749e9ee^..749e9ee`：11 files +192/−118；`2f0bb22..749e9ee` 含评审提交 `b5363c4`）。本段为增量复审记录，**不改写 §3/§6 的首轮发现快照**；§3 表中 `open` 状态为首次评审时点，实际关闭/落实以本段为准。

### 7.1 复审范围、范围门禁与实测

```bash
cd /Users/zyzs/lczyzs/code/zgraph/.worktrees/docs-docs-writer
git status --porcelain --branch        # ## docs/docs-rework-design...origin/main [ahead 5]（复审提交前 clean）
git show --stat 749e9ee                # 11 files changed, 192 insertions(+), 118 deletions(-)
git diff --name-status 749e9ee^..749e9ee
# M DR-005；M 00/01/02/03/04/06/07/08；M spec README/PROVENANCE（无代码/测试/配置/CLAUDE/progress/issues）
git diff --shortstat 7f88614..749e9ee -- doc/decisions doc/design doc/spec-v1.2 doc/index.md
# 13 files changed, 1311 insertions(+), 160 deletions(-)（首轮 1228/151 + 修复 192/118）
git diff --name-only 7f88614..749e9ee | grep -E 'CLAUDE\.md|doc/progress\.md|doc/issues\.md|\.rs$|\.py$|Cargo|scripts/|schemas/' || echo "(none)"
# (none)；DR-004 未改（`git diff --name-only 7f88614..749e9ee | grep -c DR-004` → 0）
bash scripts/dev/check.sh              # CHECK PASSED（exit 0；提交评审前复跑）

cd doc/spec-v1.2
shasum -a 256 0*.md                    # 9/9 SHA-256 与 PROVENANCE §3 表逐字一致：
# 00 ce92145f…d25c5 / 01 1d9634c7…8c2f / 02 f3f397e2…43986 / 03 ffa5c0f0…dbd3dbc /
# 04 d551e857…238f4 / 05 52f35e7b…bf42 / 06 86492d86…e48c9 / 07 3736fb24…27289e / 08 cdbfd322…e86e
wc -l 0*.md README.md PROVENANCE.md REFERENCES.md
# 00–08 = 6334 行（309+343+930+486+903+1429+1112+323+499）；含 README/PROVENANCE/REFERENCES = 6604 行
# 与 README:24-32 行数表、PROVENANCE:136-146 §3 表、PROVENANCE:123/133 注记**全部一致**
```

- 一致性 grep（实测）：`R1→R13→R15→R14` 6 处一致；00 表 18 个 DSL 码；`AC-01..AC-14` 三处一致；`TRACE_SCHEMA_VERSION=4` 三处一致；新 §1.4.1/§3.4.3/§8.9 存在且被引用；`R1–R14`/17 码残留仅 08 历史审计行（§8.0–8.8 历史快照，允许）+ `architecture-v2`/analysis（C-7 已声明）；`6308/6568` 已不在当前 spec 正文（见 §7.5 R7 的史料口径提示）。
- 08 仍是**只追加**：`git diff 7f88614..749e9ee --numstat -- 'doc/spec-v1.2/08-*'` → `21 0`（§8.9 内补 ISSUE-104/105 两行；8.0–8.8 未动）。

### 7.2 条件关闭证据（逐条）

| 条件 | 关闭证据（文件:行 / 命令 / 实现对照） | 结论 |
|---|---|---|
| **C-1** 应用顺序 | 03 §3.4.2 已改为「终态短路 → blocked（先 emit 再结算/挂账）→ passed（先 resolve 后 emit）」：`03:151-153`（顺序注释 + `if run.status.is_terminal() { return; }`）、`:161`（blocked 先 emit）、`:173-176`（resolve 失败 `return`、成功后才 emit）；§3.4.3 `:220`/`:260` 冻结超限「fail_run（sink 关闭）→ 外层 emit no-op（不入库）」。DR-005 §3.3 `:273-342` 与 03 逐行同构（`:297-302` resolve 前不 emit、`:341-342` 超限顺序 + 「不得改成 create→emit→fail」）。实现核实：`crates/zgraph-kernel/src/run.rs:987-1050`（现行 `apply_publish`：passed 分支 `:1016` 先 resolve、`:1020` 后 emit；循环开头 `:997` 附近终态短路；`resolve` 失败 `None => return`）与 `:1086-1112`（`resolve_self_loop` 超限先 `fail_run` 再返回）；`crates/zgraph-kernel/src/trace.rs:584-591`（`gate_result` 在 `closed` 后直接 return）、`:643-655`（`Emitter::finish` 先置 `closed=true` 再发 `RunFinished`）、`run.rs:1444-1464`（`fail_run` → `emitter.finish`）⇒ 「超限该次 GateResult 不入库」属实，v0.1.0 自环超限 trace 顺序不变（B6/AC-14 d） | ✅ 关闭 |
| **C-2** defer 前置 | 03 `:224` 首行 `if edge.on_gate_false != Defer: return NotApplicable`；matrix 新增 `:214`「settle 直投终态 → RUN_DELIVERY_TO_TERMINAL（不 attempt+1）」；DR-005 `:251` 行 8 + `:255`「attempt 计算（C2 明确）」；01 `:89`「普通边（settle）→ RUN_DELIVERY_TO_TERMINAL」一致；attempt 计算无歧义（自环 `src.attempt+1`、非自环 `target_current.attempt+1`） | ✅ 关闭 |
| **S-1** AC-14 a | 07 `:259`：attempt0 `passed=true/deferred=false`、attempt1 `passed=false/deferred=true`；attempt1 只写 receipt（不结算/不 Skipped）、target Completed → dormant → run `Completed`；DR-005 `:586` 同步；与 01 §1.5 `deferred = !passed && edge.on_gate_false == Defer` 一致 | ✅ 关闭 |
| **S-2** 迁移 payload | 06 `:297-299`（§6.2.8：迁移只补 SQL 列默认值、历史 `events.payload` 保持 v3、`trace --json` 不合成）、`:916`（§6.10.4 行「v4 后新写入事件」）、`:985/:989`（示例改新写入口径 + S2 注记）；07 `:300-307`（AC-14 h 拆「新库/迁移结构」「迁移历史行 SQL 默认值 + 历史 payload v3」「v4 后新 run 三键 + L1」三组）；DR-005 `:453`（§4.2）、`:462`（§4.3 L1）；与 `trace --json` 直读 `events.payload` 的实现（`crates/zgraph-trace/src/reader.rs:414-465`、`crates/zgraph-cli/src/trace_cmd.rs:319`）一致 | ✅ 关闭 |
| **S-3** cli 接入计划 | DR-005 `:619-631`（§8 增第 7 行 cli 适配、写作用域含 `crates/zgraph-cli/**`、依赖钉死「真实值接线等 kernel 合并」、Lead 跨模块声明 + 第二评审 REV-070/REV-071）；`:711`/`:717`（§12.1 W2a 波及文件、§12.2 注「W2a 实现批包含 cli」）；与 trace 分支 `96ab28e` 已落过渡值（REV-070）的现状一致 | ✅ 关闭 |
| **Lead 补充 ①** AC-14 g R15-valid | 07 `:291-296`（新增 `a` defer 自环作可重入来源，`a→b` 仍 blocked）；DR-005 `:592`（AC-14 g）+ `:686`（E-4）；R15 判定：defer 自环 `a→a` + defer 边 `a→b` 源 `a` 均满足「defer 自环」→ 通过；R13 无无 gate 环 → 通过；静默后 `a→b` target 从未实例化 → 确定性 `RUN_DEADLOCK`，骨架成立 | ✅ 关闭 |
| **Lead 补充 ②** 超限 GateResult | 与 C-1 同证据：`trace.rs:584-591`/`:643-655` + `run.rs:1444-1464` 证明 `fail_run` 关闭 sink、外层 emit no-op；DR-005 `:341-342/:382`、03 `:220/:260`、07 `:283`（AC-14 e「不断言超限那次 GateResult」）一致 | ✅ 关闭 |
| **Lead 补充 ③** `updated_at` 统一 | DR-005 `:230`、01 `:190` 均改为 `String`（RFC3339 毫秒 Z）；`DeferredReceipt` 结构、写入/刷新口径（`01:196`）一致 | ✅ 关闭 |

### 7.3 建议项 N-1…N-11 落实核验

| # | 首轮建议 | 落实证据 | 状态 |
|---|---|---|---|
| N-1 | 环语义「充分/必要/保守」方向错 | DR-005 `:159` 改为「闭迹为权威、简单环更严」；E-1 `:683` 改为「放宽例（不是反例）」 | ✅ 落实 |
| N-2 | v3 seed 缺口无 ISSUE 编号 | DR-005 `:465`、PROVENANCE `:122`、07 `:19`、08 `:496` 均引用 **ISSUE-104**（编号由 Lead 分配；台账登记见 §7.5 R8） | ✅ 落实（台账待 Lead） |
| N-3 | README 07 行数 320≠实测 322 | README `:30` 改 **323**；`wc -l 07-验收用例.md` = 323；PROVENANCE `:146` = 323 | ✅ 落实（注：§2 #16 摘要仍写 322，见 R6） |
| N-4 | 子图 R15 收敛未明写 | 02 `:405`（R6 行明写收敛为 `DSL_SUBGRAPH_INVALID`）、`:432` detail 列表增 `defer_not_reentrant`；DR-005 `:166` 同口径 | ✅ 落实 |
| N-5 | R15 同规则聚合 vs 首错未冻结 | 02 `:60`（例外：R15 取 edges 序首错）、`:414`（R15 行同口径） | ✅ 落实 |
| N-6 | 04 §4.6.4 回传载体漏 `verdict` | 04 `:703` 改为 `GateEvaluation { edge_index, passed, error, verdict }` | ✅ 落实 |
| N-7 | 06 示例 `verdict:false` 与 C1 规则冲突 | 06 `:985` 示例改 `"gate_fn":"check_output"` + `"verdict":null` | ✅ 落实 |
| N-8 | DR-005 N-3 反例行占位「且……」 | 该行已删除（diff 显示 N-3 整行删除；P-4 保留为合法例） | ✅ 落实 |
| N-9 | 实例级/节点级 settled 口径未冻结 | 01 `:125/:141/:199-200`、03 `:258`、DR-005 `:267` 统一「实现按节点级 `settled_by_node`；实例级 `settled_sources` 仅审计/排障、不参与 skip/就绪判定」 | ✅ 落实 |
| N-10 | 00 §0.7 未写 per-port | 00 `:296` 已加「按端口继承（仅入边 `on_gate_false != defer` 的 values/arrived；defer 端口必须新一代重投）」 | ✅ 落实 |
| N-11 | R4 边结构校验落点 vs 现行 R8 阶段 | 02 `:403` 增注：只冻结错误码归属（R4 类型桶）、结构解析可在 R8 阶段、不得改变多错图 `errors[0]`/`error_count` 顺序 | ✅ 落实 |
| （关联） | REV-070 S1 待定 | 06 `:297`、DR-005 `:456`、08 `:497` 引用 **ISSUE-105** 承接口径定案 | ✅ 登记（定案归 W2a-docs/trace） |

### 7.4 四维复审（增量）

| 维度 | 复审结论 | 依据 |
|---|---|---|
| 功能正确性 | ✅ | C-1/C-2 关闭；03 §3.4.2/§3.4.3 与 DR-005 §3.3 同构且与现行 kernel `apply_publish`/`resolve_self_loop`/`Emitter` 顺序逐点吻合；AC-14 a/e/g/h 修正后可直接执行；超限/迁移口径分别有 Emitter 关闭语义与 events payload 直读实现支撑；B6（v0.1.0 hash/validate/run/trace）在文档层无新增破坏点 |
| 代码结构合理性（文档/计划结构） | ✅ | 增量仅 11 个 doc 文件；S-3 把 cli 适配、跨模块评审、依赖钉死写入 §8/§12；与 trace 分支既有过渡值（REV-070）衔接；DR-004 未改、08 只追加、无越界 |
| 逻辑性 | ✅（附 §7.5 R1–R4 建议） | 决策矩阵重编号后主体自洽；E-1 方向修正后与反例一致；dormant/deadlock、per-port/settled 口径、attempt 计算自洽；仅剩 DR-005 少量行号/摘要引用未同步（不改变错误码/行为，均落到同一 `RUN_DELIVERY_TO_TERMINAL`/证据口径） |
| 可读性 | ✅ | 术语一致、表格/代码块规范；新增 S2 注记、ISSUE 引用、C1/C2 注释清晰；无未替换占位（N-8 已删）；新增 §号/交叉引用实测全部命中 |

### 7.5 残留项（均非阻塞；建议 W2a-docs 或合并前微提交收口）

| # | 级别 | 位置 | 问题 | 建议 |
|---|---|---|---|---|
| R1 | 建议 | `doc/decisions/DR-005-w2a-defer-gate-freeze.md:570`（§6.4 超限证据） | 仍写「trace 有 … `GateResult`（P→W 边）」；未说明该 GateResult 来自**之前轮次**、超限该次不入库（与 §3.5/§7 e/07 AC-14 e 的裁决字面不完全同步） | 改为「Crashed 实例（attempt=3）+ **之前轮次**已产生的 P→W `GateResult` 行 + `RunFinished{Failed}`；超限该次不入库」 |
| R2 | 建议 | `doc/decisions/DR-005-w2a-defer-gate-freeze.md:593`（§7 h） | 仍写「L1 events 回放含新字段」，未按 S-2 拆「历史 run 逐字节复用旧 payload / 新 run 含三键」 | 与 07 AC-14 h `:300-307` 对齐改写 |
| R3 | 建议 | `doc/decisions/DR-005-w2a-defer-gate-freeze.md:369`（§3.4 dormant 判定表） | 决策矩阵重编号后仍写「按行 8 `RUN_DELIVERY_TO_TERMINAL`」；行 8 现为 settle 直投 Completed，此处应为行 9（defer 终态非 Completed） | 行 8 → 行 9（与 E-2 `:684` 一致） |
| R4 | 建议 | `doc/decisions/DR-005-w2a-defer-gate-freeze.md:298`（§3.3 伪代码注释） | 「NotApplicable → 现行 resolve（3/4/8/9）」中行 9 实为 `resolve_defer_reentry` 的 `Failed` 分支（结果同为 `RUN_DELIVERY_TO_TERMINAL`、不 emit），不在「现行 resolve」 | 注释改「3/4/8；行 9 走 Failed」或删去 9 |
| R5 | 建议 | `doc/decisions/DR-005-w2a-defer-gate-freeze.md:721`（§12.3） | 「逻辑拆 3 个 commit」未计入条件修复提交 `749e9ee` | 改「3 个逻辑 commit + 1 个 REV-067 条件修复 commit（`749e9ee`）」 |
| R6 | 建议 | `doc/spec-v1.2/PROVENANCE.md:132`（§2 第 16 条） | 「N-3 README 07=322」与最终实测 323 不符（首轮发现值 322、修复后 323） | 改 323，或写明「首轮 322 → 修复后 323」 |
| R7 | 建议 | `doc/spec-v1.2/PROVENANCE.md:123`（§2 第 15 条末行） | 首轮批次的实测合计被从 **6308** 改为 **6334**，覆盖了历史批次快照（第 16 条已记 6334、§3 当前 6334）；与 PROVENANCE 既有「历史批次留痕不覆盖」惯例不一致（M4 5818、M5 5958 均保留） | 第 15 条恢复 **6308**（首轮快照），第 16 条保留 6334；§3 当前值不变 |
| R8 | 台账待办（Lead） | `doc/issues.md`（base `7f88614`） | 文档已引用 **ISSUE-104**（v3 seed 缺口）与 **ISSUE-105**（`verdict_json` 损坏口径待定），但 base 台账最新为 ISSUE-101..103，尚无 104/105 行（编号由 Lead 分配；本批按范围不改 `issues.md`） | Lead 在 process 收口/合并前补登 104/105 并回填状态；与 DR-005 §4.3/06 §6.2.8 引用一致 |

> 另：§5 的跨分支项 REV-069 N1（R15 `expected`/`actual` 模板）与 N7（子图聚合口径）仍待 W2a 实现/W2a-docs 收口，不属本批 docs 分支范围。

### 7.6 结论修订

- 首轮 §1 的「结论：有条件通过」经本条**修订为「结论：通过」**：C-1/C-2/S-1/S-2/S-3 全部关闭（§7.2），N-1…N-11 全部落实或登记（§7.3），四维复审均 ✅（§7.4）；残留 R1–R8 均为**建议/台账待办，不阻塞合并**（§7.5）。
- 合并前置已满足：评审记录含 `分支: docs/docs-rework-design` + `结论：通过` + 真实评审人；worktree 在评审提交前 clean；`check.sh` PASSED；范围仅 `doc/**`。合并后由 Lead 按 AGENTS §4 清理 worktree/分支并更新 `doc/progress.md`；`doc/issues.md` 补登 ISSUE-104/105（R8）。
- 签字：`reviewer-docs` · 2026-10-31（增量复审）。
