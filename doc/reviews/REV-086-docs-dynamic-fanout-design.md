# REV-086 · docs · W4a 动态 N / `foreach` 设计冻结

> **评审类型**：设计冻结独立评审（四维 + 可实现性/一致性；docs-only，不动实现）。本记录只新增评审文件，未改文档内容。
> **跨模块声明**：本分支 14 files 全部位于 `doc/**`（DR-007、spec 00/01/02/03/04/06/07、README/PROVENANCE、SOP v1.2、DR-004、index、progress），**无代码/测试/配置改动**；无跨模块越界。不涉及实现分支评审。

- **分支**：`docs/docs-dynamic-fanout-design`
- **作者**：`docs-dev`（单提交 `ab53d4e`，基于 main `559ede6`；正文含 `Review: REV-086` / `Agent: docs-dev`）
- **评审人**：`reviewer-b`（独立评审，**≠ 作者**）
- **变更范围**：`559ede6..ab53d4e`（14 files，+917/−56）：新增 `doc/decisions/DR-007-w4a-dynamic-fanout-freeze.md`（557 行）；spec 00/01/02/03/04/06/07 W4a 冻结写入；`doc/design/software-team-sop-v1.md` 升 v1.2；DR-004 §10、`doc/index.md`、`doc/progress.md`、`doc/spec-v1.2/README.md`、`PROVENANCE.md`。
- **评审时间**：2026-11-01
- **关联**：task-29（Lead 冻结口径合同）、task-30（作者任务）、DR-004 §3 W4a/§5 D4、DR-005/DR-006 格式、spec 01/02/03/06/07、SOP v1.2

## 1. 结论

**结论：有条件通过**

P0=0；**P1=5（C1–C5，均为设计冻结闭环前必须关闭的阻断项）**；P2=6；P3=10。核心方向（运行期动态 N、无 controller 实例、item 定位、join、trace v6、默认零变化）与 DR-004 D4/task-29 合同一致，文档骨架完整、PROVENANCE 数字/哈希实测准确；但**区域边界、join 投递来源、controller buffer 生命周期、AC-17 错误覆盖、4 条 Lead 裁决落盘**存在可实现性/一致性问题，若不冻结会在 W4b 产生歧义实现。C1–C4 是 DR/spec 的设计补全，C5 是 AC 覆盖补全；关闭后由 Lead 复审（条件关闭记录见 §7）。

**4 条待确认项与 Lead 2026-11-01 裁决核对**（均与 DR-007 已冻结默认值**语义一致**；但 DR §15.3 仍标「需确认」，见 C4）：

| 项 | Lead 裁决 | DR-007 现状 | 核对结果 |
|---|---|---|---|
| ① `item_key` canonical 字符串 | number canonical / bool `true\|false` / 复杂值 → NULL 按索引派生 | §2.1/§6.2/E-4 冻结 string 原值、number/bool canonical、null/object/array→NULL | 一致；但 §6.2 有自问自答/「非法」矛盾句，需清理（C4） |
| ② `max_items` 缺省无上限 | 采纳，风险登记 | §2.1/§6.2/E-10/§13 冻结「缺省无上限」+ 风险表 | 一致；台账未登记（P3 N12） |
| ③ join 目标可为 `end`，数组按普通端口值进入 end 聚合 | 采纳 | §2.1/§3.2/§6.5/E-6/01 §1.9 冻结 J 可为 end、`run_output` 对应端口为数组 | 一致 |
| ④ region 内 `trigger=any` 沿用普通 any（未到端口 null） | 采纳 | §6.1/E-3/§15.2 冻结 controller buffer 按 any 就绪、未到端口 null | 一致；但 `items_port` 是否必须先到未显式（C3/P2） |

## 2. 四维评审

| 维度 | 结论 | 依据 |
|---|---|---|
| 功能正确性/可实现性 | ⚠️ 有条件 | 方向与 D4/task-29 一致；但 C1（F 入边边界）、C2（join 投递来源）、C3（controller 未就绪）会让 W4b 无法唯一实现；C5 使 4 个 `RUN_FOREACH_*` 中 `ITEM_INCOMPLETE` 无验收口径 |
| 结构合理性 | ✅（P2 备注） | 新增 DR-007 + 分册 banner/正文/AC-17/Dr-004/index/PROVENANCE 结构完整；无代码改动；但 00/01 代码块/表格格式损坏（N6）、`join_buffer`/`ControllerBuffer` 未入 RunState（N9） |
| 逻辑性 | ⚠️ 有条件 | R17/R18 顺序、reason 闭集、hash 投影、迁移步骤清晰；C1–C3 的边界/来源/生命周期缺口破坏「唯一判定」；历史 W2b banner 被改写（N7） |
| 可读性 | ⚠️ 有条件 | DR 结构/表格可读，但 §6.2 canonical 段自问自答（C4）、README 02/index SOP 陈旧（N11）、SOP §9「四项」（N14）等降低可读性/可追溯性 |

## 3. 发现清单（P0=0 / P1=5 / P2=6 / P3=10）

### P1 · 条件（合并前必须关闭）

| # | 位置 | 问题 | 建议 |
|---|---|---|---|
| **C1** | DR-007 §3.2 R18 检查 row8（L149）、§4 区域算法（L195–202）；spec 02 R18（L437） | **R18 未检查 F 的入边来自 R 外**：row8 仅检查「R 内节点（除 F）」，§4 伪码对 F 同样豁免；但 task-29 A2 明确「F 的入边来自 R 外（正常依赖）」。于是 R 内节点（或回边）→F 可被接受，controller buffer 收到 item 实例投递、跨 item/单次展开语义未定义，region 闭合不成立 | R18 增加「F 的全部入边来源 ∉ R（reason=`external_edge`）」检查与算法伪码；补 N-13 反例 fixture/断言；02 R18 表与 DR §4 同步，reason 闭集仍 11 值 |
| **C2** | DR-007 §6.5（L254–260）、03 §3.4.6 join（L397–411）、03 §3.15 join 行（L663）、07 AC-17 c（L~438）；01 `Delivery.from_instance`（L197–208）/`GateResultRecord.instance_id`（L271–285） | **join 投递来源实例未冻结**：N>0 时应取哪个 E item 的 `from_instance`/attempt 未定义；**N=0 空列表没有 E item 实例**，但 §6.5 要求「正常 GateResult/结算」、03 §3.15 写「join GateResult」、AC-17 c 要求 join 投递正常 GateResult——而 `Delivery.from_instance` 与 `GateResultRecord.instance_id` 均为必填 `InstanceId`，无实例可用。W4b 无法实现 AC-17 c | 冻结 join 的投递来源：N>0 明确 source（如最后完成 E item/或合成 source）与 attempt；N=0 明确同样规则或显式「不产生 GateResult」并同步 DR/03/07/06；保证 trace 证据可断言。补 AC-17 c 的 source/instance 断言 |
| **C3** | DR-007 §6.1（L225–230）、§6.7（L277–285）、§15.1 C-3；03 §3.4.6（L362–419）、§3.10（L581–583）；01 `RunState`（L329–334） | **controller buffer 的「未就绪/已结算/skip/死锁」语义未定义**：`is_quiescent` 只把「ready-but-not-expanded」视为非静默；若 F 的入边部分到达（或全部 settle=false），controller 不 ready、无 item 实例、无 Pending，run 可能**静默完成且不展开**（或无法进入既有 `RUN_DEADLOCK`）；F 作为 defer 边目标时 `node_current` 无 controller 实例，W2a receipt/dormant/死锁路径也未定义；`items_port` 未到（optional/trigger=any）时展开时机未冻结。`join_buffer`/`ControllerBuffer` 类型也未在 01 RunState/类型章节定义 | 在 §3.4.6/§3.10 冻结 controller 状态机（not-ready/ready/expanded/skipped）与终止/死锁/跳过行为；定义 F 入边全部 settle 时的 region skip 或 RUN_DEADLOCK；定义 defer→F 的 receipt 语义（或 R18 禁止 defer 入 F）；明确 items_port 必须先到；01 补 `ControllerBuffer`/`join_buffer` 字段/类型；补 AC-17 用例 |
| **C4** | DR-007 §15.3（L506–512）、§6.2 canonical 段（L241）；DR-004 §10（L269–271） | **4 条 Lead 裁决未落盘**：§15.3 仍写「需用户/Lead 确认（不阻塞 W4a 冻结，W4b 实现前定稿）」；Lead 2026-11-01 已逐条裁决（①–④与 DR 冻结值一致），但 DR 未记录 adopted，W4b「唯一语义依据」仍带 open 问题。§6.2 item_key 段含「非法 → RUN…？**不**：…还是报错？**冻结**」自问自答，与最终 NULL 语义冲突；DR-004 §10 旧行「W3b durable resume 可选，需用户确认」未标注已被下条「W3b 不做」取代 | 在 DR-007 §15.3/§15.2 追加「Lead 2026-11-01 裁决：①–④采纳」并移除待确认状态（保留历史修订段）；重写 §6.2 canonical 段为单一冻结陈述；DR-004 §10 旧行加「已被 2026-11-01 决定取代」或由新行明确 |
| **C5** | 07 AC-17 a–i（L420–470）；DR-007 §6.6（L273–274）、03 §3.15（L662） | **AC-17 未覆盖 `RUN_FOREACH_ITEM_INCOMPLETE`**：4 个 `RUN_FOREACH_*` 中，AC-17 d 只覆盖 item 节点 Crashed/TimedOut，e 覆盖 key 重复，f 覆盖 max_items/非数组；**E Skipped → ITEM_INCOMPLETE 没有 AC/断言**。另 `key_field` 的 number/bool canonical、null/复合值→NULL 也无直接 AC（仅 string 重复用例） | 增加 AC-17 j（或扩 d）：某 item 的 E 因 gate false Skipped → `RUN_FOREACH_ITEM_INCOMPLETE`、run Failed、其余 item Cancelled、无 join 投递；补 key canonical（number/bool/null/complex）断言；与 03 §3.15 行、DR §6.6 对齐 |

### P2 · 建议（不阻断合并，但建议 W4b 前修）

| # | 位置 | 问题 | 建议 |
|---|---|---|---|
| N6 | 00-总纲 L46；01-领域模型 L38/L80/L333 | **代码块/表格格式损坏**：00 `Attempt` 与 `ItemIndex` 两个表行被拼成一行（`||` 连接）；01 `CompiledNode.max_fanout`/`foreach`、`InstanceRecord.attempt`/`item_index`、`RunState.deferred_receipts`/`controller_buffers` 均被拼到同一行（Rust 代码块语法无效） | 拆行/补换行；建议 markdown 渲染/`git diff --check` 之外加简单格式检查；W4b 以这些 struct 为实现依据 |
| N7 | 02-DSL与校验 L6（W2b banner） | **改写历史批次段**：W2b banner 原文（REV-077，2026-10-31）被改成含 `R18` 与「DSL 域仍 19 码（W4a 新增第19码）」；违反「历史段只追加」；W2b 当时的事实是 18 码/旧顺序 | 保留 W2b 原文，仅在 W4a banner 追加「W4a 起顺序含 R18、码数 19」；02 W2b 特性表行同理不改历史 |
| N8 | 02 R17 表（L436）、DR §2.1/§3.2 子图段 | **子图内 foreach 的规则归属矛盾**：R17 表说「子图内 → `DSL_PARSE_ERROR`（子图内按 R6 收敛）」，DR/02 R18 又说「子图内不允许，R17/R6 已拦、R18 不适用」；没有明确 R6 的独立检查点/W4b 实现位置 | 在 02 R6/§2.6.1 明确「subgraph 内任意节点带 foreach → R6 `DSL_SUBGRAPH_INVALID`（含 well-formed 形状）」；R17 表改为仅主图/递归 R4 形状，删除「子图内 → DSL_PARSE_ERROR」歧义 |
| N9 | 01 RunState L329–334；03 §3.4.6 L401 | **`ControllerBuffer`/`join_buffer` 类型未定义**：03 伪码使用 `controller_buffer[F]`、`join_buffer[F][item_index]`，01 只列 `controller_buffers: HashMap<NodeId, ControllerBuffer>` 未定义 `ControllerBuffer` 结构，且 `join_buffer` 完全未入 RunState | 01 §1.2.5/§1.7 补 `ControllerBuffer`（values/arrived/trigger/ready/expanded）与 `join_buffer: HashMap<NodeId, BTreeMap<ItemIndex, Value>>`（或等价）；03 伪码与字段一致 |
| N10 | 06 §6.2.15 keys_json（L489/500）；DR §6.2/§8.3 | **`#<index>` 表示不单射**：NULL 键表示为 `"#i"`，真实 string key `"#0"` 会与 index 0 的 NULL 键撞表示；`keys_json` 无法区分，影响 L2 完整性校验/审计 | 用带类型标签表示（如 `{"$index":0}`/`["null",0]` 或前缀 `#null:`）并在 DR/06/07 同步；仍保持 keys 数组可读 |
| N11 | `doc/spec-v1.2/README.md` 02 行；`doc/index.md` L14 | **索引陈旧**：README 02 行仍写「R1–R16 校验」、W2b 旧顺序、无 W4a；index SOP 行仍写「v1.1 … 动态 N 仍延期」，与已交付 SOP v1.2/dynamic profile 冲突；README 00 行「18 个 DSL 码」后接 W4a 19（同格矛盾） | 更新 README 02/00 行（R1–R18、R17/R18、W4a、19 码）与 index SOP 行（v1.2/dynamic profile/静态 profile 不变）；PROVENANCE 行数已同步，但索引内容需同步 |

### P3 · 台账/阅读性建议

| # | 位置 | 问题 | 建议 |
|---|---|---|---|
| N12 | `doc/issues.md`（未改） | Lead 裁决 ② `max_items` 缺省无上限要求「风险登记」；DR §13 有风险表，但**唯一问题台账未登记**（task-30 允许「如有新 open 项」；AGENTS §7 要求设计分歧/已知风险入台账） | 建议登记 1 条 P3（如 ISSUE-141）「foreach 缺省无上限 → 运行期内存/实例风险，由作者 max_items 约束」，状态 open/accepted；或明确 DR §13 为唯一风险记录并说明 |
| N13 | DR-004 §10 L269–271 | 旧行「W3b durable resume 可选，需用户确认」与新增「W3b 不做」并存；虽 append-only，但语义相反且无替代标记 | 在旧行加「已被 2026-11-01 用户决定取代（见下条）」；新行保留 |
| N14 | SOP §9（移除动态 N 后） | 文本仍写「以上四项仍属执行模型扩展」，实际剩 3 项；§7.1 仍标题「两 profile 共用」，v1.2 已有三 profile | 改「三项」/「三 profile」；§7.1 标题同步 |
| N15 | 02 R18 row4；README 00 行；07 DoD（L~470） | R18 row4 同时含 `forbidden_node_type`/`nested_foreach`，未定义同节点两者同时命中时的首因；README 00 仍以「18 个 DSL 码」起句；07 DoD 写「既有 DSL 18 码 + R1–R16 校验序列不变」缺「默认无 foreach 时」限定 | R18 row4 明确先 `forbidden_node_type` 后 `nested_foreach`（或反之）并冻结；README 00 改为 19 码起句；07 DoD 加「无 foreach 图」限定 |
| N16 | 06 §6.7 L793 | 迁移链写「v1..v4→v6 逐版本链」，与 DR §8.2/C4 的「v1..v5→v6」不一致（v5→v6 段另述，但读者易混） | 统一写「v1→…→v6（含 v5→v6）」 |
| N17 | DR §6.2 L241 | canonical key 段「null/object/array → **非法** → RUN…？**不**：…还是报错？**冻结**」为自问自答，虽最终冻结 NULL，但「非法」与问句易误读（C4 条件一并清理） | 重写为单句冻结：string 原值；number/bool canonical；null/object/array/缺键 → NULL（不报错）；仅非 NULL 重复报错 |
| N18 | DR §15.2 `source_instance_id`/§8.3 | `source_instance_id` 定义为「触发展开的投递实例（可为 NULL）」，但 F 的 trigger=all 多入边时选哪条、entry F 无实例时 null 的语义未冻结；trace 审计/回放可能需要 | 冻结选择规则（如「最后使其 ready 的投递」或「触发 items_port 的投递」）；entry/trigger any 的 null 明确；补 AC 断言 |
| N19 | 07 AC-17 整体/DR §11 | AC-17 a–i 未覆盖 **controller 未就绪/全部 settle skip**（C3）、**F 入边来自 R 的反例**（C1）、**join source**（C2）；建议作为 C1–C3 的配套断言 | C1–C3 条件关闭时补对应 AC/negative fixture；AC-17 总 DoD 注明覆盖 4 个 `RUN_FOREACH_*` + 5 类边界 |
| N20 | `doc/index.md` DR-004 行 | DR-004 行仍只描述 W3a0/W3a，未提 W3b 不做/W4a 执行记录（DR-004 已追加） | 索引行加 W4a/W3b 状态一句（与 DR-007 行一致） |

## 4. 验证证据（只读，docs-only）

评审人亲自执行（worktree `/Users/zyzs/lczyzs/code/zgraph/.worktrees/docs-docs-dev`，HEAD `ab53d4e`，base `main` `559ede6`，工作区干净）：

```bash
bash scripts/dev/check.sh                 # PASSED（身份/冻结/分支名/进度/fmt/diff --check）
wc -l doc/progress.md                     # 15（≤30 目标）
git diff --check main...HEAD              # 无输出
git diff --name-only main...HEAD | grep -v '^doc/'   # 无（纯 doc，无代码/测试）
git status --porcelain                    # 空
git log -1 --format='%B' ab53d4e          # 含 Review: REV-086 / Agent: docs-dev
git diff --numstat main...HEAD            # 14 files +917/−56

# PROVENANCE §3 实测核对（逐文件 SHA-256 + 行数）
for f in doc/spec-v1.2/0[0-8]-*.md; do shasum -a 256 "$f"; wc -l "$f"; done
# 9 个分册 SHA/行数全部与 PROVENANCE §3 表一致：
# 00 68373df...343 / 01 47a31cf...421 / 02 7941f5f...984 / 03 6704e74...675 /
# 04 b051c3c...939 / 05 52f35e7...1429 / 06 1dfb7df...1271 / 07 902324b...484 / 08 9450d7a...564

wc -l doc/spec-v1.2/0[0-8]-*.md | tail -1        # 7110（与 §3/§2 第23条一致）
wc -l doc/spec-v1.2/*.md | tail -1               # 7479（含 README/PROVENANCE/REFERENCES）
git diff --numstat main...HEAD -- doc/spec-v1.2/0[0-8]-*.md | awk ...  # +245/−21（净 +224）
git diff --numstat main...HEAD -- doc/spec-v1.2 | awk ...              # +281/−41（净 +240）
```

- **PROVENANCE 数字全部实测一致**：00–08 合计 7110、全 spec 7479、00–08 +245/−21、全目录 +281/−41；§3 九行 SHA/行数与实际逐字节一致（见上）；README 行数表 343/421/984/675/939/1429/1271/484/564 与实际一致。**N12 等 P3 为内容/台账问题，不是数字问题**。
- **锚点/grep 核验**：`R17`/`R18`/`DSL_FOREACH_INVALID`/`RUN_FOREACH_*`/`item_index`/`foreach_expansions`/`AC-17`/`W4a` 在 DR-007、00/01/02/03/06/07、README、PROVENANCE、DR-004、index、progress 均有对应锚点；11 个 R18 reason 与四组 AC 编号可检索。
- **W3b 不做**：DR-004 §10 已追加、`progress.md` 下一步写明、DR-007 §1.3/§16 记录；但旧 DR-004 行未标取代（N13）。
- **SOP v1.2**：文件头/§3.3/§7.3/§9/§11.3 均为设计/验收口径，**无 as-built 字样**；approved-only v1/rework v1.1 结构描述未被改写（静态 profile 兼容）；仅 §9「四项」/§7.1「两 profile」文字陈旧（N14）。
- **未运行 cargo**：本批 docs-only、无代码/测试/fixture 改动；按 Lead 要求只做 check.sh + PROVENANCE/锚点核验，未跑 cargo（无需验证代码）。

## 5. 作者 4 条待确认项与 Lead 裁决（逐条）

见 §1 表：①–④ **DR-007 冻结默认值均与 Lead 裁决一致**（canonical number/bool + 复杂值 NULL；max_items 缺省无上限；J 可为 end、数组入 end 聚合；trigger=any 沿用 ordinary any）。**但 DR §15.3 仍把四者标为「需确认」**，且 §6.2 canonical 段落含自问自答/「非法」矛盾，需在 C4 一次性落盘 Lead 2026-11-01 裁决（历史段只追加）。未发现与裁决相悖的冻结值。

## 6. 遗留问题

- **C1–C5 关闭后**：由 Lead 复审条件关闭记录（可复用本记录 §7 表追加关闭证据）；关闭前不合并。
- **W4b 后续**：C1–C3 的状态机/来源冻结直接影响 kernel/trace 实现；C5 进入 W4b e2e；W4b-docs 回写 as-built/AC-17 实测/08 注记。
- **P2/P3**：N6 格式（00/01）、N7 历史改写、N8 子图归属、N9 状态类型、N10 keys 表示、N11 README/index、N12 issues 台账、N13 DR-004 旧行、N14 SOP 文字、N15 R18 row4/README 00/07 DoD、N16 v1..v5 表述、N17 canonical 文案、N18 source_instance_id、N19 AC 补全、N20 index DR-004；建议 docs-dev 在关闭 C1–C5 时一并处理 N6/N7/N11（合并前），其余进台账/后续批次。
- **无 P0**；无代码/实现改动；不需要 e2e/cargo 验证。

## 7. 条件关闭记录（待关闭）

| 条件 | 状态 | 关闭证据（由作者/Lead 追加） |
|---|---|---|
| C1 R18 F 入边边界 | **open** | DR §3.2/§4 + 02 R18 + N-13 fixture |
| C2 join 来源/空列表 GateResult | **open** | DR §6.5/03 §3.4.6/07 AC-17 c + 06 trace 证据 |
| C3 controller 未就绪/终止/skip/defer | **open** | DR §6.1/§6.7/§15.1 + 03 §3.10/§3.4.6 + AC |
| C4 Lead 4 项裁决 + §6.2 清理 + DR-004 旧行 | **open** | DR §15.3/§15.2/§6.2 + DR-004 §10 |
| C5 AC-17 `ITEM_INCOMPLETE`/canonical 覆盖 | **open** | 07 AC-17 + DR §6.6/03 §3.15 |
