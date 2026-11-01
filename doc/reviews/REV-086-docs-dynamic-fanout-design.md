# REV-086 · docs · W4a 动态 N / `foreach` 设计冻结

> **评审类型**：设计冻结独立评审（四维 + 可实现性/一致性；docs-only，不动实现）。本记录只新增评审文件，未改文档内容。
> **跨模块声明**：本分支 14 files 全部位于 `doc/**`（DR-007、spec 00/01/02/03/04/06/07、README/PROVENANCE、SOP v1.2、DR-004、index、progress），**无代码/测试/配置改动**；无跨模块越界。不涉及实现分支评审。

- **分支**：`docs/docs-dynamic-fanout-design`
- **作者**：`docs-dev`（单提交 `ab53d4e`，基于 main `559ede6`；正文含 `Review: REV-086` / `Agent: docs-dev`）
- **评审人**：`reviewer-b`（独立评审，**≠ 作者**）
- **变更范围**：`559ede6..ab53d4e`（14 files，+917/−56）：新增 `doc/decisions/DR-007-w4a-dynamic-fanout-freeze.md`（557 行）；spec 00/01/02/03/04/06/07 W4a 冻结写入；`doc/design/software-team-sop-v1.md` 升 v1.2；DR-004 §10、`doc/index.md`、`doc/progress.md`、`doc/spec-v1.2/README.md`、`PROVENANCE.md`。
- **评审时间**：2026-11-01
- **复审**：2026-11-01，HEAD **`fd06908`**（条件修复提交，parent `703ea07`；14 files 纯 doc +157/−85）；C1–C5 已关闭，结论升级为 **通过**（首轮结论保留为历史，见 §7）
- **关联**：task-29（Lead 冻结口径合同）、task-30（作者任务）、DR-004 §3 W4a/§5 D4、DR-005/DR-006 格式、spec 01/02/03/06/07、SOP v1.2

## 1. 结论

**结论：通过**

**复审（2026-11-01，HEAD `fd06908`）：C1–C5 全部关闭，证据见 §7；下方首轮「有条件通过」记录保留为历史。** 复审无 P1；残余 P2=5 / P3=3（§7.3，均不阻断）。

> **首轮结论（2026-11-01，HEAD `703ea07`）：有条件通过**（已被上方复审取代；保留为历史）。首轮 P0=0；P1=5（C1–C5）；P2=6；P3=10。核心方向（运行期动态 N、无 controller 实例、item 定位、join、trace v6、默认零变化）与 DR-004 D4/task-29 合同一致，文档骨架完整、PROVENANCE 数字/哈希实测准确；首轮问题集中在**区域边界、join 投递来源、controller buffer 生命周期、AC-17 错误覆盖、4 条 Lead 裁决落盘**。首轮 C1–C4 是 DR/spec 的设计补全，C5 是 AC 覆盖补全；均已由 `fd06908` 关闭。

**4 条待确认项与 Lead 2026-11-01 裁决核对**（均与 DR-007 已冻结默认值**语义一致**；C4 已关闭——§15.3 已落盘 Lead 裁决；仅 DR-004 执行行仍为历史表述，见 §7.3 N21）：

| 项 | Lead 裁决 | DR-007 现状 | 核对结果 |
|---|---|---|---|
| ① `item_key` canonical 字符串 | number canonical / bool `true\|false` / 复杂值 → NULL 按索引派生 | §2.1/§6.2/E-4 冻结 string 原值、number/bool canonical、null/object/array→NULL | 一致；§6.2 自问自答/「非法」矛盾句已清理（C4 关闭） |
| ② `max_items` 缺省无上限 | 采纳，风险登记 | §2.1/§6.2/E-10/§13 冻结「缺省无上限」+ 风险表 | 一致；ISSUE-141 已登记（N12 关闭） |
| ③ join 目标可为 `end`，数组按普通端口值进入 end 聚合 | 采纳 | §2.1/§3.2/§6.5/E-6/01 §1.9 冻结 J 可为 end、`run_output` 对应端口为数组 | 一致 |
| ④ region 内 `trigger=any` 沿用普通 any（未到端口 null） | 采纳 | §6.1/E-3/§15.2 冻结 controller buffer 按 any 就绪、未到端口 null | 一致；§6.1 已冻结 items_port 必须先到（C3 关闭） |

## 2. 四维评审

| 维度 | 结论 | 依据 |
|---|---|---|
| 功能正确性/可实现性 | ✅（复审通过） | 方向与 D4/task-29 一致；首轮 C1–C3/C5 已关闭（§7.1）；残余仅为 P2 实现细节（N24/N25），不改变设计判定 |
| 结构合理性 | ✅（P2 备注） | DR-007 + 分册 banner/正文/AC-17 a–j/DR-004/index/PROVENANCE 结构完整；`ControllerBuffer`/`JoinBuffer`/`ControllerState` 已入 01；无代码改动；残 README 02 行与 DR-004 执行行文案（N21/N22） |
| 逻辑性 | ✅（复审通过） | R17/R18 顺序、reason 闭集、hash 投影、迁移步骤清晰；C1–C3 的边界/来源/生命周期已冻结；历史 W2b banner 已恢复（N7 关闭） |
| 可读性 | ✅（P2 备注） | §6.2 canonical 已重写、index SOP/README 00 已更新、SOP §9「四项」已改「三项」；残 README 02 行、AC-17 j 机制措辞（N22/N23）等 P2/P3 阅读性问题 |

## 3. 发现清单（首轮：P0=0 / P1=5 / P2=6 / P3=10；C1–C5 已于复审关闭，证据见 §7）

### P1 · 首轮条件（均已于复审关闭）

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

见 §1 表：①–④ **DR-007 冻结默认值均与 Lead 裁决一致**（canonical number/bool + 复杂值 NULL；max_items 缺省无上限；J 可为 end、数组入 end 聚合；trigger=any 沿用 ordinary any）。**C4 已关闭**：DR-007 §15.3 已落盘 Lead 2026-11-01 裁决表、§6.2 canonical 段已重写、DR-004 旧 W3b 行已加取代注记；历史段只追加。未发现与裁决相悖的冻结值。

## 6. 遗留问题

- **C1–C5 已全部关闭（2026-11-01 复审，HEAD `fd06908`）**；证据与残余 P2/P3 见 §7。首轮记录保留在 §1、§3（历史）。
- **W4b 后续**：C1–C3 的状态机/来源/生命周期已冻结，直接作为 kernel/trace 实现依据；C5 的 AC-17 j 进入 W4b e2e；W4b-docs 回写 as-built/AC-17 实测/08 注记。
- **残余 P2/P3（不阻断）**：N21 DR-004 执行行仍为历史「待确认」表述（DR-007 §15.3 已确认）；N22 README 02 行未逐项写 W4a R17/R18；N23 AC-17 j 的「出边 gate=false」应为「E 的入边全被门控拦截」；N24 N=0 controller 注入 `[]` 的 J 实例创建/边结算细节；N25 controller `Skipped` 时 F 无实例的出边 skip/来源细节；N26 `progress`/index 复审后「待复审」状态由 Lead/merge 更新；N27 `01` `explizit` 拼写等 P3。均不改变冻结语义，建议 W4b 前顺手处理。
- **无 P0/P1**；无代码/实现改动；不需要 e2e/cargo 验证。

## 7. 条件关闭复审（2026-11-01，HEAD `fd06908`）

**复审对象**：`703ea07..fd06908`（作者修复提交 `fd06908`，parent 评审提交 `703ea07`）；14 files **纯 doc/**，+157/−85；作者提交正文含 `Review: REV-086` / `Agent: docs-dev`；worktree 干净。首轮条件表见 §3（历史），以下为关闭证据。

### 7.1 C1–C5 逐条关闭证据

| 条件 | 状态 | 复审证据 |
|---|---|---|
| **C1 R18 F 入边边界** | **closed** | DR-007 §3.2 row8 增加「**F 的全部入边来源 ∉ R**（reason=`external_edge`）」；§4 伪码增加 `if v == F and any(in_edge.from in R): error(external_edge)`；反例 **N-13** 已加（region 内节点/回边→F）；02 R18 表同步「边界闭合含 F 入边 ∉R」并给反例；03 §3.4.6 `on_delivery` 注明 controller 只接收 R 外投递。→ 满足 task-29 A2。 |
| **C2 join 来源/GateResult** | **closed** | DR-007 §6.5 冻结：**N>0** `Delivery.from_instance`/`GateResultRecord.instance_id`/`attempt` = 最后完成使 join_buffer 达 N 的 E item 实例；**N=0** 无 GateResult/Delivery，controller 注入 `[]`（trace=expansion+J input/transition）；03 §3.4.6/§3.15、06 §6.2.15、07 AC-17 c 同步。→ 满足 Lead C2 口径。 |
| **C3 controller 生命周期/终止** | **closed** | 01 §1.6.1 新增 `ControllerState{Collecting,Ready,Expanded,Skipped}`/`ControllerBuffer`/`JoinBuffer`，RunState 增 `controller_buffers`/`join_buffer`；DR-007 §6.1/§6.6/§6.7 冻结 items_port 必须先到、全部 settle=false→Skipped、部分到达静默→RUN_DEADLOCK、defer→F、Expanded/Skipped 拒绝投递；03 §3.4.6/§3.10 `is_quiescent` 改为 live controller ∈{Collecting,Ready}，`Expanded/Skipped` 不算活跃。→ 满足 Lead C3 口径。 |
| **C4 Lead 4 项裁决落盘** | **closed** | DR-007 §15.3 改为「Lead 裁决（2026-11-01，已确认）」表：①canonical、②max_items 无上限、③join 可 end、④trigger=any；§6.2 canonical 段重写为单一冻结陈述（去掉「非法」自问自答）；DR-004 §10 旧「W3b 可选，需用户确认」行加「已被 2026-11-01 决定取代」注记（历史保留）。ISSUE-141 已登记 max_items 风险。 |
| **C5 AC-17 ITEM_INCOMPLETE/canonical** | **closed** | 07 新增 **AC-17 j**（`exit_skipped.json`：E Skipped→`RUN_FOREACH_ITEM_INCOMPLETE`/无 Crashed/其余 item Cancelled/无 join 投递；`keys_canonical.json`：number/bool/string/null·object→`{"$index":i}`/非 NULL 重复）；AC-17 总 DoD 明确覆盖 4 个 `RUN_FOREACH_*` + 5 类边界；07 banner/§7.1/§7.2/DR §11 同步 a–j。→ 覆盖缺口关闭（测试机制措辞见 N23 残余）。 |

### 7.2 复审验证（新 HEAD + 重算数字）

```bash
git diff --stat 703ea07..fd06908         # 14 files changed, 157 insertions(+), 85 deletions(-)
git -c core.quotepath=false diff --name-only 703ea07..HEAD | grep -v '^doc/'   # 无（纯 doc）
git status --porcelain                   # 空
git log -1 --format='%B' fd06908         # 含 Review: REV-086 / Agent: docs-dev
bash scripts/dev/check.sh                # PASSED
wc -l doc/progress.md                    # 15（≤30）

wc -l doc/spec-v1.2/0[0-8]-*.md | tail -1  # 7154（与 PROVENANCE §3/§2 第24条一致）
wc -l doc/spec-v1.2/*.md | tail -1         # 7532（含 README/PROVENANCE/REFERENCES）
git diff --numstat 559ede6..HEAD -- doc/spec-v1.2/0[0-8]-*.md | awk ...  # +285/−17（净 +268）
git diff --numstat 559ede6..HEAD -- doc/spec-v1.2 | awk ...              # +330/−37（净 +293）

for f in doc/spec-v1.2/0[0-8]-*.md; do shasum -a 256 "$f"; wc -l "$f"; done
# 9/9 SHA/行数与 PROVENANCE §3 当前表逐字节一致：
# 00 b966952...344 / 01 2472260...446 / 02 548d82c...984 / 03 cb2d964...684 /
# 04 b051c3c...939 / 05 52f35e7...1429 / 06 c82e4f7...1273 / 07 bc90d67...491 / 08 9450d7a...564
```

- **PROVENANCE §3/§2 第 24 条全部实测一致**：00–08 **7154**、spec 目录 **7532**、00–08 numstat **+285/−17**、全目录 **+330/−37**；9 分册 SHA/行数逐字节一致；README 行数表 344/446/984/684/939/1429/1273/491/564 一致。
- **锚点/无残留**：`AC-17 a–j` 已在 DR-007/07/README/PROVENANCE/DR-004 同步；当前 spec/DR 中无「a–i」残留、无 `#index`/`#<index>` 旧表示；00/01 代码块/表格已拆行；02 W2b 历史 banner 恢复原文（`R1→R13→R15→R16→R14`、18 码），W4a 单独 banner 记录 19 码/新增 R18；子图内 foreach 归 R6（02 R6/R17 已改）；06 迁移链改「v1→…→v6（含 v5→v6）」；R18 row4 顺序冻结；`source_instance_id` 规则冻结（N18）。
- **SOP/文档**：SOP §9 已「三项」、§7.1「静态两 profile 共用」；index SOP 行改 v1.2 三 profile；README 00 行 19 码；DR-004 行补 W3a/W3b/W4a 状态；ISSUE-141 已登记。
- **范围**：`git diff 703ea07..HEAD` 14 files +157/−85 纯 doc；无代码/测试/配置；无 cargo 运行（docs-only）。

### 7.3 残余 P2/P3（不阻断，建议 W4b 前顺手处理）

| # | 级别 | 位置 | 问题 | 建议 |
|---|---|---|---|---|
| N21 | P2 | DR-004 §10 W4a 行（L272） | 该执行记录仍写「**待确认项 4 条见 DR-007 §15.3**…由 REV-086 逐条确认」；DR-007 §15.3 已改为已确认，DR-004 行属历史交接表述但现读起来像仍待确认 | 在 DR-004 §10 追加一行「REV-086 复审（2026-11-01）确认 4 项裁决」或按只追加原则注明；不改历史行 |
| N22 | P2 | `doc/spec-v1.2/README.md` 02 行（L25） | 已改为「R1–R18」并加 W4a 顺序括注，但行内**未列 W4a `foreach`/R17/R18/`DSL_FOREACH_INVALID`** 的实质内容（仅顺序提示）；README 00 行已 19 码、07 行已 AC-17 a–j | 02 行补「W4a：`foreach` + R17/R18 + `DSL_FOREACH_INVALID`」一句，与 README 00/07 一致 |
| N23 | P2 | `07-验收用例.md` AC-17 j 第 1 条（L~470） | 文字写「某 item 的 E 因**出边** gate=false 全部 Skipped」；E→J 边由 R18 禁止 gate，E Skipped 实际由 **E 的入边**全被门控拦截导致（E 是出口节点）；W4b 实现 e2e 时可能照错措辞构造错误 fixture | 改为「E 的全部入边 gate=false → E Skipped（E 为 region 出口，无法完成 join）」；断言不变 |
| N24 | P2 | DR-007 §6.5 N=0 / 03 §3.4.6 L413 / AC-17 c | N=0 冻结「controller 直接把 `[]` 写入 J 输入 buffer，不产生 Delivery/GateResult」；但未明确 **J 实例如何创建**（created_seq/transition/是否走 entry 注入等价路径）与 E→J 边如何结算；AC c 却要求 J `instances.input`/transition 可断言 | 在 DR/03 补一句「注入等价于 entry 外部注入：必要时创建 J 实例并写 input，不产生 GateResult/Delivery；J 的 transition/instances.input 作为证据」，明确 E→J 边结算/无 GateResult |
| N25 | P2 | DR-007 §6.6 controller Skipped 行 / 03 §3.4.6 L428 | controller `Skipped`（F 无实例）时写「F 的出边按普通 skip 级联（E→J 无数据 → J 按普通规则 skip/等其它入边）」；但 F/E 均无实例，E→J 边的 settlement 来源/GateResult 语义未像 N=0 那样冻结；J 若未实例化则无法级联 | 明确 controller-origin skip 的边结算（与 N=0 同路径：不产生 GateResult/Delivery，或定义 controller source），并补 AC-17 f 的 J skip/等其它入边断言 |
| N26 | P3 | `doc/progress.md`、`doc/index.md` | 复审提交后 progress 仍写「待复审」、index reviews 行仍「首轮有条件通过 → C1–C5 已修复，待复审」；属状态行陈旧（Lead/merge 动作后可更新） | merge 后由 Lead 更新为「REV-086 通过」；不影响设计冻结 |
| N27 | P3 | `01-领域模型与状态机.md` L~80 `item_key` 注释 | 保留拼写 `explizit`（德语拼法，应为 explicit）；仅为注释拼写 | 顺手改 explicit；不影响语义 |

> 残余均不改变 C1–C5 冻结语义；C2/C3 的 N24/N25 是**实现级边界细节**，Lead 已明确 C2 的机制（N=0 不产生 GateResult/Delivery、controller 注入），故不作为合并阻断。若 Lead 认为需在 W4b 前定稿，可在 W4b-docs 或小 commit 处理。

### 7.4 复审结论

**C1–C5 全部关闭；复审无 P0/P1；最终结论：通过。** 建议 Lead 执行 merge（`fd06908` + 本评审记录提交）。
