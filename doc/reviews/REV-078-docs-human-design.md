# REV-078 · docs · W3a0 人工评审设计冻结（human lifecycle / decide / `human_decisions` / trace v5）

> **范围声明**：本分支仅改 `doc/**`（13 files，+997/−72；其中新增 `DR-006` 597 行），**无代码/测试/`doc/reviews/**` 改动**；评审人只新增本记录，未改文档内容。本记录为**设计冻结评审**（不评审实现；W3a 实现尚未开始）。

- **分支**：`docs/docs-human-design`
- **作者**：`docs-dev`（`b40af47` 主冻结提交 + `85a076d` E-11 裁决记录提交；两次提交正文均含 `Review: REV-078` / `Agent: docs-dev`）
- **评审人**：`reviewer-b`（独立评审，**≠ 作者**）
- **变更范围**：`1ed37ab..85a076d`（13 files，+997/−72）：新增 `doc/decisions/DR-006-w3a-human-decide-freeze.md`（597 行）、spec `00/01/02/03/04/06/07`、`doc/spec-v1.2/{README,PROVENANCE}.md`、`doc/{index,progress}.md`、`doc/decisions/DR-004…md` §10
- **评审对象说明**：任务书登记 HEAD 为 `b40af47`；评审时分支已追加 **`85a076d`**（E-11 裁决记录，仅改 DR-004/DR-006/progress/PROVENANCE 4 文件，不改语义）。本记录以 **`85a076d`** 为对象，已分别核对两次提交的 diff 与 trailer。
- **评审时间**：2026-10-31
- **关联**：Lead 合同 task-11（W3a0 口径冻结）、task-12（作者任务）、DR-004 §3 W3a0 / §5 D3 / §3 W3a DoD、REV-063 C12/C13/S10、DR-005（W2a defer/attempt-per-activation）、DR-006

## 1. 结论

**结论：有条件通过**

P0=0、**P1=3**、P2=8、P3=8。设计方向正确、总体自洽：触发模型（`builtin.human` 保留 python_node 符号）与 02 §2.5/04 §4.4/§4.7 既有契约一致且不触碰 DSL schema/R1–R16/hash；`WaitingHuman` 生命周期、终止判定、decision consumer、C13 写者模型、trace v5 迁移、拒绝矩阵与 AC-16 a–g 均已冻结到可实现的详细度；E-11 裁决（instance 不存在 → exit 1、存在但非 WaitingHuman → exit 5）在 `85a076d` 中被 Lead 明确记录，且与本评审任务摘要口径一致。

但有 **3 条 P1 逻辑/契约矛盾**（副作用顺序、consumer 行为、二次 decide 冲突码）会直接影响 W3a 实现与 AC-16 可判定性；另有 P2 事实性/一致性问题（PROVENANCE 数字、跨册 AC 计数、`--db` 清单、03 表格与取消行、`reject_reason` 码、`instances.input` 时点、AC 覆盖缺口、trace 关闭/库 API 场景、预检可见性）。条件见 §6；**P3 建议不进条件**，但建议在关闭条件时一并登记台账或后续批次处理（§5）。

**task-13 检查清单逐项结论**：

| # | 检查项 | 结论 |
|---|---|---|
| 1 | `builtin.human` 保留符号 vs 02 §2.5 / 04 §4.4/§4.7、gate canonical、DSL/R1–R16/hash 不变、默认行为不变 | ✅（N19 措辞建议）：精确匹配 `module=="builtin" && function=="human"`；无新 NodeType/字段；`graph_hash`/validate 对既有图不变；与 gate op（`check_output`/`compare`/`manual`）无冲突。⚠️「R14 例外」措辞易误读（R14 本就不 import python_node），见 N19 |
| 2 | 状态机/生命周期（Pending→WaitingHuman、WaitingHuman→Completed/Cancelled/TimedOut、非静默、暂停/取消/deadline、I1） | ⚠️（N1/N7/N8）：迁移集合与非静默/终止判定（`is_quiescent` 含 WaitingHuman）自洽，I1 解释成立；但 `WaitingHuman→Completed` 副作用顺序有缺陷（N1）、input 快照时点未冻结（N7）、03 §3.8 Cancel/表格不一致（N8） |
| 3 | decide 自动接续 / 无 `resume` / 退出码 5 / `--json` / E-11 | ⚠️（N3/N6）：自动接续与「无 resume」表述清楚（DR §4.6、04 §4.1.3）；E-11 裁决一致；退出码表（0/1/2/4/5/70）与 `--json` 形状完整；但二次 decide 冲突码不可判定（N3）、`reject_reason` 码命名不一致（N6） |
| 4 | `human_decisions` 表/唯一键/decision_id/consumed_at/连接模型/C13/崩溃未消费归 W3b | ✅（N10/N11/N21）：DDL、唯一键 `(run_id,instance_id)`、`decision_id` 审计、`consumed_at` 仅 consumer 写、WAL+busy_timeout+重试、`TraceWriter` 不写该表、未消费行归 W3b 均与 C12/C13/S10 一致，且无跨表事务要求；⚠️ trace 关闭/库 API 场景（N10）、预检状态可见性（N11）、applied 结果不落库（N21）未冻结 |
| 5 | trace v5 迁移可实现性（CHECK 重建/备份/只读拒绝/v3→v5 链）/ API/CLI 输出不破坏既有契约 | ✅（N17/N18/N16）：重建 `instances`/`transitions`（SQLite CHECK 不可 ALTER）实现可行——两表各 3 索引、`instances` 无外部 FK 引用（`human_decisions` 在其后创建）、`events` 不重建；备份/单事务/失败恢复/只读拒绝/`schema_migrations` 1..5 完整；`events.kind` 不变、NDJSON 追加行有冻结口径。⚠️ 既有 e2e `expected 4` 需随 v5 更新（N17）、含决策 replay 断言缺口（N18） |
| 6 | 拒绝矩阵满足 DR-004「终止或进入 rework」/ AC-16 a–g 覆盖 DoD | ⚠️（N9）：矩阵语义满足（reject = 正常 Completed，去向完全交给图；W2a defer 返工复用）；AC-16 b 覆盖 rework、a 覆盖 approve；**缺 reject→终止分支、decide exit 4、human 图 validate、kernel applied=false 的 AC 断言**（N9）；AC-16 b 图的门条件表述需精确化（N9 附） |
| 7 | 是否误改代码/`doc/reviews/**`；progress ≤30；历史决策只追加；PROVENANCE 第 20 条 + §3 实测 | ⚠️（N4/N14）：无代码/测试/评审记录改动；progress 16 行（≤30、四块）；DR-004 §10 追加不改原决策；PROVENANCE §2 第 20 条存在、§3 9/9 SHA-256 与行数逐项实测一致；但 §3 合计/增量数字错误（N4），另有一处笔误与索引小项（N14） |

## 2. 四维评审

| 维度 | 结论 | 依据（文件/行/实测） |
|---|---|---|
| 文档正确性（语义） | ⚠️（N1–N9） | 触发模型/生命周期/decide/表/迁移主体与 task-11 合同及既有 spec 一致；缺陷集中在 6 处状态机与契约细节（N1、N3、N6、N7、N9、N10、N11）和 3 处事实性数字/引用（N4、N5、N8） |
| 设计自洽性（结构） | ⚠️（N2/N3/N8/N9） | 00/01/02/03/04/06/07 与 DR-006 的落点相互引用可达（§12.1 C-1..C-9 逐条落实）；模块边界（trace/kernel/cli/e2e/docs）与 DR-004 §4 一致；但 DR §7.3 与 03 §3.4.5 的 consumer 行为互相矛盾（N2），DR/06/07 的二次 decide 冲突码不自洽（N3），03 §3.8 表格被截断且 Cancel 行遗漏 WaitingHuman（N8） |
| 逻辑性 | ⚠️（N1/N2/N3/N9/N10/N11） | 状态机迁移/幂等/唯一键/WAL 并发/迁移失败恢复逻辑基本完整；三处会直接改变实现的逻辑缺口：副作用顺序（N1）、consumer 过滤（N2）、冲突码判定顺序（N3）；另有 trace 关闭、预检可见性两个场景未闭合（N10/N11） |
| 可读性 / 可追溯性 | ⚠️（N12–N21） | 术语与章节组织清晰（DR-006 一页摘要 H1–H9 + 非目标 + 风险 + 待确认项；AC-16 a–g 结构统一）；PROVENANCE §3 SHA-256/行数 9/9 实测一致、REV/分支/base 可追溯、DR-004 §10 追加式；瑕疵：handler 命名三套（N12）、DR §14.1 文件清单一处不符（N13）、索引/笔误/poll 注释（N14/N15）、既有测试与 AC 细节（N17/N18）、占位语义（N20/N21） |

## 3. 发现清单

> 级别口径：P0 = 阻塞；P1 = 重要（设计冻结前必须修复）；P2 = 一般（应修复或登记 ISSUE）；P3 = 建议（可后置，应进台账）。本分支 **P0=0**。

| # | 级别 | 位置 | 问题 | 建议 | 状态 |
|---|---|---|---|---|---|
| N1 | **P1 重要** | `DR-006:135`（§3.2 副作用表）、`01-领域模型与状态机.md:121`（§1.2.3）、`03-调度器与生命周期.md:336-339`（§3.4.5 伪代码） | `WaitingHuman→Completed` 的副作用顺序写成「先 `transition(Completed)`，再写 `output`/`finished_at`」；但 as-built 的 `transition_raw` 在迁移时立即构造并发送快照，`on_completed`/`finish_builtin` 都是**先写 output 再 transition**（否则 `instances.output` 落 NULL）。照此实现会导致 Transition 快照/instances.output 为空，AC-16 a 的 output 断言失败 | 冻结为「写 `output={decision,reason}` → 清 deadline → `transition(Completed, reason=human_decided)`（快照携带 output）→ publish」；同步 DR §3.2、01 §1.2.3、03 §3.4.5（`finished_at` 由 transition 统一写入，勿重复） | open（条件 C1） |
| N2 | **P1 重要** | `DR-006:423`（§7.3 第 2 步）vs `03-调度器与生命周期.md:350`（§3.4.5 consumer 段） | 两处 consumer 行为矛盾：DR 说先用 bridge 维护的 `WaitingHuman` 集合判断「仍等待」才提交 control；03 说对 `pending(run_id)` 的**每行**都提交 `RunControl::HumanDecision`。按 DR 的过滤逻辑，实例已超时/已取消/已由别的控制完成时（集合 miss）该 pending 行永远不会被提交、也不会被标 `consumed_at`，形成永久悬挂行；kernel 的 `applied=false` 幂等确认路径也被绕过 | 冻结「consumer **总是**提交每行（kernel 以 `applied=false` 幂等处理），收到确认后标 consumed」；删除 bridge 集合过滤，或明确 set-miss 行也提交/标 consumed 的口径；同步 03 §3.4.5 | open（条件 C2） |
| N3 | **P1 重要** | `DR-006:229`（§4.4 行 6）、`:243`（§4.5 预检表）、`06-存储与可观测.md:1110`（§6.10.8 预检）、`07-验收用例.md:384`（AC-16 c） | 二次 decide 的冲突码不可判定：DR §4.4 行 6 说「第二次 INSERT 冲突 → exit 5 `HUMAN_DECISION_EXISTS`」；但 §4.5 预检顺序是先判 `status=WaitingHuman`、后判决策行。若第一次决策已被 run 消费（实例 Completed），第二次 decide 会先命中 `HUMAN_INSTANCE_NOT_WAITING`，而非 AC-16 c 断言的 `HUMAN_DECISION_EXISTS`；只有「消费前」的竞争窗口才会出现 DECISION_EXISTS，测试会不稳定 | 冻结预检顺序与码：建议**先查 `human_decisions` 行是否存在 → 恒返回 `HUMAN_DECISION_EXISTS`**，再判 run/instance 状态；或明确「已消费后的二次 decide = NOT_WAITING」并让 AC-16 c 容忍两种码。同步 DR §4.4/§4.5、06 §6.10.8、07 AC-16 c | open（条件 C3） |
| N4 | P2 一般 | `doc/spec-v1.2/PROVENANCE.md:194`（§3 合计注）、同文件 §2 第 20 条 `:179` | §3 写「00–08 合计 **6791 行**」「净 +304（新增 **346**/删除 **42**）」；实测 `wc -l` 合计 **6795**（330+390+958+592+933+1429+1219+420+524），实际 diff 为 **+349/−45**（净 +304 正确）；同文件 §2 第 20 条已写 6795，§3 与 §2/实测不一致。含 `README/PROVENANCE/REFERENCES` 的 7115 正确（6795+80+218+22） | 改 §3 合计为 **6795**、增量改为 **+349/−45**；保留 7115 不变；说明与 §2 第 20 条一致 | open（条件 C4） |
| N5 | P2 一般 | `doc/spec-v1.2/README.md:63`；`06-存储与可观测.md:869`（§6.10.1 全局参数） | ① README「验证方式」仍写「验收：07 的 **AC-01..AC-15**」，07 已改 AC-01..AC-16；② 06 §6.10.1 全局参数 `--db <path>`（run/replay/trace）未把新增 `decide` 纳入，与 §6.10 五子命令/§6.10.8「--db 同口径」不一致 | ① README 改 AC-01..AC-16；② 06 §6.10.1 改 `--db <path>`（run/**decide**/replay/trace） | open（条件 C4） |
| N6 | P2 一般 | `DR-006:381`（§7.1 `reject_reason`）vs `01-领域模型与状态机.md:277`（§1.5）、`03-调度器与生命周期.md:326-328` | `HumanDecisionRecord.reject_reason` 的机器码命名不一致：DR §7.1 注释「见 §4.5」指向 `HUMAN_*`（CLI 命名空间大写码）；01/03 实现算法使用 `instance_missing`/`not_current`/`not_waiting_human`（snake_case 内核码）。W3a kernel/cli 实现与 trace/stderr 文档将各写一套 | 冻结内核 `reject_reason` 为 snake_case 三码（instance_missing/not_current/not_waiting_human），并注明与 decide `--json.code`（HUMAN_*）的映射/区别；DR §7.1 改引用 §4.1 而非 §4.5 | open（条件 C5） |
| N7 | P2 一般 | `DR-006:88`（§2.2 输入快照）vs `01-领域模型与状态机.md:126-127`（§1.2.4）、`03-调度器与生命周期.md:329-334`、`04-节点接口与扩展点.md` §4.4.5 | `instances.input` 写入时点未冻结：DR §2.2 写「**实例创建时**把触发它的 buffer 快照写入 instances.input」，但 human 实例是「首条投递到达时惰性创建 Pending」，此时 required 端口可能未到齐；01 §1.2.4/03 §3.4.5 未写快照时点，04 只说「记录等待上下文快照」。AC-16 a 断言 `instances.input` 为等待上下文，若按「创建时」实现将得到不完整快照 | 冻结为「在 `Pending→WaitingHuman`（全部 required 满足）时写入 `instances.input`，形状=全部声明端口（未到 null），与普通节点 dispatch 快照同语义」；同步 DR §2.2/01 §1.2.4/03 §3.4.5（可加一条副作用） | open（条件 C5） |
| N8 | P2 一般 | `03-调度器与生命周期.md:444-447`（§3.8 控制表）、`:439`（Cancel 行） | ① 新增的 W3a0 blockquote（`:446`）插在表格行之间，把表末行「暂停中 run 静默」挤出表格，Markdown 表格在渲染时断裂（W3a0 表行与末行不再同表）；② `Cancel` 行的迁移集合只写「Pending/Ready → Cancelled；Running → …」，未含 `WaitingHuman → Cancelled`，与 01 §1.2.3、§3.15、DR H2 的冻结不一致（取消是等待实例的主要退出路径之一） | 把 blockquote 移到表格之后；`Cancel` 行动作改「Pending/Ready/**WaitingHuman** → Cancelled；Running → …」 | open（条件 C4） |
| N9 | P2 一般 | `07-验收用例.md:381-384`（AC-16 c）、`:391-394`（e）；`DR-006:483-485`（§10 c/e）；`DR-006:218`（§4.3）/AC-16 b | AC-16 覆盖缺口：① DR-004 DoD「reject 按矩阵**终止**或进入 rework」只测了 rework（b），无 reject→终止（rejected 边 → end/失败节点）断言；② DR-006 §8 声称「CLI decide 全部退出码/--json」一致，但 AC-16 c 未覆盖 `user_version<5`/DB 错误的 **exit 4**（也未覆盖 70）；③ DR-006 §8 兼容矩阵有「`builtin.human` 图 validate 通过」，AC-16 无 validate 断言；④ DR §9 把「决策重复应用」的幂等证据挂在 AC-16 c，但 c 只测 CLI 预检，未覆盖 kernel `applied=false`（迟到/非当前实例）路径；⑤ AC-16 b 的 rejected 边未写明 gate 谓词（应为 `check_output{path:"decision",equals:"rejected"}`，defer 属性只覆盖另一侧），否则实现可能把它做成 gate 恒 false 的 defer 边 → 只挂账不返工 | 在 AC-16 a–g 内补：reject→终止子断言（b 或新增变体）、decide exit 4（v<5/DB 错误）、human 图 validate、applied=false（kernel 单测或 e2e 注入）；b 中写明 rejected 边 gate 表达式与 defer 语义分工 | open（条件 C5） |
| N10 | P2 一般 | `03-调度器与生命周期.md:65`（`trace_enabled`）、DR §7.3；库 API（`start_run`/`run_to_completion`） | human 图在**无 trace DB** 场景无决策通道：`trace_enabled=false` 时 run 不落 SQLite，`zgraph decide` 无表可读写；库 API 没有内建 consumer。设计未冻结该场景的行为（是「human 图要求 trace 开启，否则启动失败」还是「库调用方自行提交 `RunControl::HumanDecision`」），当前语义会退化为无限等待（若未声明 timeout） | 冻结：① CLI human 图在 trace 关闭时启动失败或强制开启；② 库 API 明示 human 等待必须由调用方通过 control 提交决策（无自动 consumer）；在 DR 非目标/§8 兼容矩阵与 03 补注 | open（条件 C5） |
| N11 | P2 一般 | DR §4.5/06 §6.10.8 预检（读 `instances.status`）vs 06 §6.4/§6.6 写线程批量；`doc/progress`/07 AC-16 a | decide 预检的**实例状态可见性**未冻结：预检依赖 TraceWriter 异步批量落库的 `instances.status`；run 的实时等待行来自 bridge lifecycle observer（内存），用户看到 waiting 后立即 decide 可能读到旧状态 → 假 exit 5。AC-16 a 若直接 decide 而不轮询 DB 可能 flake | 冻结可见性方案（进入 WaitingHuman 时 flush instance 行 / decide 预检短暂重试直到 trace 可见 / 或明确要求以 DB 可见为 decide 前置）；AC-16 a 明确「轮询 `instances.status=WaitingHuman` 后再 decide」 | open（条件 C5） |
| N12 | P3 建议 | `01:32`（`HandlerRef`）、`DR-006:370/577`（`CompiledHandler`）、`04:110`（`HumanHandler`） | handler 类型名三套并存：01 用 `HandlerRef::Human`、DR 用 `CompiledHandler::Human`、04 §4.1.2 用 `HumanHandler`。W3a kernel 的 as-built 类型是 `CompiledHandler`（REV-074/075 已引用），命名不统一会造成实现/文档偏差 | 统一以 as-built `CompiledHandler::Human` 为准；01 的 `HandlerRef`（历史名）加注「实现名 CompiledHandler」；04 行改 `CompiledHandler::Human` | open（建议项） |
| N13 | P3 建议 | `DR-006:577`（§14.1 文件清单） | 清单称 01 改动含「§1.2.1 状态」，实际 `1ed37ab..HEAD` 的 01 diff 只涉及 banner、§1.1.1、§1.2.3、§1.2.4、§1.5、§1.6、§1.9；§1.2.1（`InstanceRecord`）未改。文件清单与实测 diff 不一致 | 删除「§1.2.1 状态」或补实际改动；清单以 `git diff --stat` 为准 | open（建议项） |
| N14 | P3 建议 | `doc/index.md:8`；`PROVENANCE.md:218` | ① `doc/index.md` 的 DR-004 行 §10 摘要仍到「W2b 已交付」，未含本批 W3a0 执行记录（DR-004 §10 已追加）；② PROVENANCE §5 W3a0 条目有笔误：`unified` 不写 as-built（无意义英文残留） | ① index 行补 W3a0（设计冻结）；② 删除 `unified`，改为「本批不写 as-built」 | open（建议项） |
| N15 | P3 建议 | `00-总纲.md` §0.5 `HUMAN_DECISION_POLL_MS` 注 vs `DR-006:421` | 00 注称「仅在有人机等待场景产生读负载，无 human 节点的图不改变行为」；DR §7.3 允许「无 human 节点的图也启动 consumer（空轮询开销可忽略）」——两处对「是否增加读负载」表述不一致。实现若始终启动 consumer，00 的注不准确（行为层面仍不变） | 冻结按需启动（首次出现 WaitingHuman 后启动）或把 00 注改为「无 human 图不改变行为语义；空轮询开销可忽略」 | open（建议项） |
| N16 | P3 建议 | `06-存储与可观测.md` §6.10.4/§6.10.6；DR §6.4 | trace `--json` 的 HumanDecision 行在 `consumed_at` 为 NULL（pending 行）时的表示（`null` 还是省略）未冻结；`--stats` 的 human 计数写「可增/可选」，设计冻结宜定稿 | 明确 pending 行 `consumed_at:null`（与列语义一致）；`--stats` 选择「不增加字段」或冻结新增字段名 | open（建议项） |
| N17 | P3 建议 | `tests/e2e/gate_defer.rs:611`（既有）；`06:732/748`（v5 只读拒绝） | 既有 e2e 断言 v3 只读拒绝消息含 `unsupported schema version=3, expected 4`；v5 后该消息将变为 `expected 5`，W3a 必须更新此既有用例（AC-16 e 未提；06 §6.7 W3a0 段也未列） | 在 06 §6.7 W3a0 段/AC-16 e 注明「W3a 同步更新既有 `gate_defer` 只读拒绝断言（expected 4→5）」 | open（建议项） |
| N18 | P3 建议 | DR §6.4 / `06:1014`（§6.10.7 replay events）；AC-16 e | 已冻结「`replay --mode=events` 含 HumanDecision 行且与 `trace --json` 逐字节一致」，但 AC-16 e 只断言**无决策** run 的 trace --json 不变，未断言**含决策** run 的 replay--events ≡ trace--json；该等价关系的实现（replay 读 human_decisions）缺少验收证据 | 在 AC-16 a 或 e 增加断言：含决策 run 的 `replay --mode=events --json` 与 `trace --json` stdout 逐字节一致 | open（建议项） |
| N19 | P3 建议 | `02-DSL与校验.md` §2.5 冻结注（W3a0）；`DR-006:81` | 「R14 例外」措辞易误读：按既有 D7，R14 对**所有** python_node 都只做格式校验、不 import（import 在 worker 启动期）；human 的真实差异在 kernel 编译/运行期（不启动 worker、无 `NODE_WORKER_IMPORT`）。写成「R14 例外」可能让 W3a dsl 实现误加无必要分支；另外 `builtin.human` 由普通用户符号变为保留符号，属对既有（恰好使用该符号的）图的语义变更，未在兼容矩阵注明 | 改为「R14 无代码例外；human 与普通 python_node 同走格式校验；差异在 kernel 编译/运行期」；在 DR §8/02 补一句保留符号兼容性说明 | open（建议项） |
| N20 | P3 建议 | `06:1105-1111`（§6.10.8 迁移/DB） | `decide` 对**不存在**的 DB 是否创建空库未冻结；现有 `trace` 有「DB 不存在不得创建空库 → exit 4」口径，decide 若按普通打开会创建空文件并留下无 schema 库，副作用不明确 | 与 trace 同口径：不存在 → 不创建、exit 4（`HUMAN_DB_ERROR`）；在 §6.10.8 写明 | open（建议项） |
| N21 | P3 建议 | `DR-006:211`（§4.2「applied=false 也标 consumed（审计留痕）」） | 「审计留痕」被高估：`human_decisions` 只存 `consumed_at`，`applied`/`reject_reason` 仅经 `TraceSink::human_decided` 到 CLI 内存/stderr，**不落库**；进程退出后无法从 trace 审计该决策是否应用及拒绝原因 | 明示「applied 结果不落库（stderr/内存可观测）」，或后续 W3b 评估加列/事件（合同外，登记即可） | open（建议项） |

## 4. 验证证据

评审人**亲自执行**（worktree `/Users/zyzs/lczyzs/code/zgraph/.worktrees/docs-docs-dev`，`export CARGO_HOME=/Users/zyzs/lczyzs/code/zgraph/.cargo-home`，HEAD **`85a076d`**，工作区干净；本批为纯 docs 设计冻结，按 Lead 指示不跑 cargo）：

```bash
git log --oneline -3                    # 85a076d / b40af47 / 1ed37ab
git status --porcelain                  # 空
git diff --stat main...HEAD             # 13 files changed, +997/−72
git -c core.quotepath=false diff --name-only main...HEAD   # 全部 doc/**，无 crates/tests/doc/reviews
git diff -U3 b40af47..85a076d           # 4 files（DR-004/DR-006/progress/PROVENANCE），仅 E-11 裁决同步
bash scripts/dev/check.sh               # CHECK PASSED（progress 11 非空行/16 行 ≤30；fmt/diff --check）
wc -l doc/progress.md                   # 16 行（四块：当前阶段/进行中任务/下一步/阻塞）
```

**PROVENANCE §3 实测复核（9/9 SHA-256 与行数逐项一致）**：

| 文件 | 行数（实测 = §3） | SHA-256（实测 = §3） |
|---|---|---|
| 00-总纲.md | 330 | ✅ `f1cc58d3…` |
| 01-领域模型与状态机.md | 390 | ✅ `6a05aaa8…` |
| 02-DSL与校验.md | 958 | ✅ `0ebe6e3d…` |
| 03-调度器与生命周期.md | 592 | ✅ `6a366280…` |
| 04-节点接口与扩展点.md | 933 | ✅ `0a01128f…` |
| 05-AgentHarness.md | 1429 | ✅ `52f35e7b…`（未改） |
| 06-存储与可观测.md | 1219 | ✅ `6b97af50…` |
| 07-验收用例.md | 420 | ✅ `86fac170…` |
| 08-审计关闭矩阵.md | 524 | ✅ `5dc763aa…`（未改） |

- **实测 00–08 合计 6795 行**（= 上表加总）；`doc/spec-v1.2/` 含 README/PROVENANCE/REFERENCES 合计 **7115**（6795+80+218+22）与 §3 一致；**但 §3 合计注写 6791、增量写「新增 346/删除 42」（实测 +349/−45）→ N4**。
- PROVENANCE §2 第 20 条的行数迁移值（00 317→330、01 348→390、02 950→958、03 526→592、04 906→933、06 1119→1219、07 372→420）与 `git show main:… | wc -l` 及本批实测逐项一致。
- 两提交 trailer：`Review: REV-078` / `Agent: docs-dev`（b40af47、85a076d 均核对）。

**设计与既有契约交叉核对**：`builtin.human` 精确匹配/不 import 与 02 §2.5.4 D7 一致；`HandlerRef`/`CompiledHandler` 命名差异见 N12；`is_quiescent` 含 WaitingHuman 与 03 §3.10 一致；`R13/R15/R16` 与 `max_fanout` 不受影响（无 schema 变化）；`human_decisions` DDL 无其它表 FK 指向 `instances`（仅 runs/instances 被引用），v4→v5 重建两表在 FK 层面可实现（`human_decisions` 在重建后创建）；`transitions`/`instances` 各 3 索引（06:197-199/270-272）与 DR §6.2 的「重建 3 索引」一致；`TraceWriter` 不写该表的写者模型与 06 §6.6 + C13 一致。

## 5. 遗留问题

- **Lead 裁决记录**：`85a076d` 记录 Lead 2026-10-31 对 DR-006 §12.3 五项与 E-11 的裁决；本评审确认其与 DR-006 §4.5/§12.3、06 §6.10.5、07 AC-16 c 口径一致，也与本任务摘要的 E-11 口径一致。task-11 合同 §3.4 的「否则 exit 5」文本仍有歧义，建议 Lead 在合同侧加一行裁决注记（不改已冻结语义）。
- **P3 建议（N12–N21）**：不构成合并条件；建议在关闭条件时把 N12/N13/N14/N15/N17/N19/N20/N21 中不即时修复的项登记 `doc/issues.md`（本批 docs 合同允许追加台账），或由 W3a 实现/W3a-docs 批次吸收。
- **W3a 实现待定稿项**：N6/N7/N9/N10/N11 属 W3a 实现前必须定稿的设计细节（条件 C5 覆盖）；W3b 相关（durable resume、未消费行处置、applied 持久化）已按合同留在 W3b，非本批缺陷。
- **未发现**：代码/测试/评审记录误改；DSL schema/R1–R16/hash 变化；trace `events.kind` 变化；历史 W1/W2a/W2b 决策被重写（DR-004 §10 仅追加）。
- 本记录结论为「有条件通过」；条件关闭后由评审人追加复审段并将结论改为无条件「通过」，`scripts/dev/merge.sh` 才会放行。

## 6. 条件关闭记录（仅「有条件通过」填写）

| 条件 | 关闭证据（需可复核） | 关闭日期 |
|---|---|---|
| C1 修 `WaitingHuman→Completed` 副作用顺序（N1）：写 output/清 deadline → transition（快照携带 output）→ publish；同步 DR §3.2 / 01 §1.2.3 / 03 §3.4.5 | 三处修订后顺序一致，且与 as-built `on_completed`/`finish_builtin` 同序；AC-16 a 的 output 断言语义成立 | 待关闭 |
| C2 统一 decision consumer 行为（N2）：总是提交 pending 行（kernel `applied=false` 幂等），或明确定义 set-miss 行的 consumed/applied 口径；删除/修正 DR §7.3 与 03 §3.4.5 矛盾 | 两处描述一致；不再存在「集合 miss → 行永不 consumed」的悬挂场景 | 待关闭 |
| C3 冻结 decide 预检顺序与二次 decide 冲突码（N3）：建议先查决策行 → 恒 `HUMAN_DECISION_EXISTS`；同步 DR §4.4/§4.5、06 §6.10.8、07 AC-16 c | 三处码/顺序一致，AC-16 c 可确定性断言（无时序依赖） | 待关闭 |
| C4 文档事实/一致性修复（N4/N5/N8）：PROVENANCE §3 合计 6795、增量 +349/−45；spec README 验收 AC-01..AC-16；06 §6.10.1 `--db` 加 decide；03 §3.8 表格恢复 + Cancel 行补 WaitingHuman | 上述位置修订；§3 与 §2 第 20 条/实测一致；Markdown 表格渲染完整 | 待关闭 |
| C5 设计补全（N6/N7/N9/N10/N11）：`reject_reason` 码命名统一；`instances.input` 快照时点冻结；AC-16 补 reject→终止、decide exit 4、human validate、applied=false 断言（b 的门条件写明）；trace 关闭/库 API 的 human 决策通道冻结；decide 预检可见性方案冻结 | DR/01/03/04/06/07 对应章节修订；AC-16 a–g 覆盖矩阵自洽且可判定；相关 P3 项（N12–N21）中未修项已在 `doc/issues.md` 登记 | 待关闭 |
