# 规格冻结 v1.2 · 索引

> **版本与来源**：v1.2（规格冻结版），冻结日期 2026-10-28（v2 落地修订 2026-10-29；M0–M5 回写 2026-10-30；**M6 回写 2026-10-31**；**W2a0 回写 2026-10-31，REV-067**；**W2b 回写 2026-10-31，REV-077**；**W3a0 设计冻结 2026-10-31，REV-078**）；源稿为外部《详细设计 v1.0/1.1》（HTML，80,551 B ≈ 79 KB），
> 经 76 条独立审计后收敛，并在冻结期做了 9 高 + 4 缺口 + 28 中低处置（27 回填 / 1 保留，见 [08 §8.6.3](08-审计关闭矩阵.md)）。
> **本仓库内文件是唯一可核验版本**；来源、文件 hash 与回填说明见 [`PROVENANCE.md`](PROVENANCE.md)；
> 文中出现的 `_spike/*`、`_research/*`、`_audit/*`、`决策记录-01` 等历史引用，仓库内替代位置见 [`REFERENCES.md`](REFERENCES.md)。
>
> **落地说明（DR-001）**：本规格是**语义基准**；实现形态为 **Rust 内核 + Python 子进程 worker**。
> 与进程内执行相关的假设（PyO3、stop-waiting、门控 Python 函数、单解释器并发、CLI 无 replay 等）
> 已按 [`doc/architecture-v2.md`](../architecture-v2.md) §8 的 **D1–D12 偏差表**与 §8.2 错误码**回写为正文**（M0–M2 批次见 [`PROVENANCE.md`](PROVENANCE.md) §2 第 4 条；**M4 Harness 批次见 §2 第 7 条**，REV-023；**M5 批次见 §2 第 8–10 条**，REV-035；**M6 批次见 §2 第 13 条**，REV-057）；
> 02–07 各分册顶部 banner 标注“已回写”。正文与偏差表冲突时以正文为准，§8 保留为决策记录。**W2a0（2026-10-31，REV-067）** 与 **W2a 实现回写（2026-10-31，REV-073）** 回写 00/01/02/03/04/06/07/08：`on_gate_false`/R15/`DSL_DEFER_NOT_REENTRANT`、defer 运行期语义（`DeferredReceipt`/attempt-per-activation/dormant/死锁）、trace schema v4、AC-14（见 PROVENANCE §2 第 15 条）。**W2b（2026-10-31，REV-077）**：节点级 `max_fanout`/R16 静态扇出上限/运行期 `NODE_FANOUT_EXCEEDED`/AC-15 回写 00/01/02/03/06/07（见 §2 第 18 条）。**W3a0（2026-10-31，REV-078，设计冻结）**：`builtin.human` 保留符号/`WaitingHuman` 生命周期/decide 自动接续/`human_decisions`+C13/trace schema v5/AC-16 设计回写 00/01/02/03/04/06/07（见 §2 第 20 条；不写 as-built）。 **W3a 实现回写（2026-11-01，REV-084）**：AC-16 a–g 已从设计冻结转实测（`tests/e2e/human_decide.rs` 21 用例；e2e 85/0/2、workspace 819/0/3）；spec 00/01/03/04/06/07/08 as-built 校准（REV-079–083 已合并 main `2444548`；见 PROVENANCE §2 第 22 条）。 **W4a（2026-11-01，REV-086，设计冻结）**：节点级 `foreach`/R17/R18（`DSL_FOREACH_INVALID`，DSL 18→19）/item 实例/join/trace schema v6/AC-17 a–i 设计 + SOP 设计稿 v1.2（dynamic-N profile）回写 00/01/02/03/04/06/07（见 §2 第 23 条；不写 as-built）。

> **状态**：规格冻结（纯文档）。本规格是内核实现的语义基准。
> **上游（仓库内）**：[可行性评估](../analysis/feasibility-assessment-v1.1.md)、[决策记录 01](../analysis/decision-record-01-scope.md)、[规格审计](../analysis/spec-audit-v1.1.md)、[技术 spike](../analysis/spike/REPORT.md)、[两份调研](../analysis/research-harness-alternatives.md)
> **偏差**：[architecture-v2 §8](../architecture-v2.md)（D1–D12）；**来源与 hash**：[PROVENANCE.md](PROVENANCE.md)；**引用映射**：[REFERENCES.md](REFERENCES.md)

---

## 分册

| # | 文件 | 行数 | 内容 | 关键裁定 |
|---|---|---|---|---|
| 00 | [00-总纲](00-总纲.md) | 343 | 命名/类型/枚举/**错误码**/**默认值**/**不变式**/术语消歧 | 权威锚点；I1–I9；**18 个 DSL 码**（W2a0 +1；W2b 不新增）+ 5 个 v2 码 + 3 个 M5 `REPLAY_*` 码 + **W2b `NODE_FANOUT_EXCEEDED`**；20 个新默认常量；`OnGateFalse`（W2a0）；`NodeError::FanoutExceeded`（W2b）；**W3a0**：`InstanceStatus::WaitingHuman`、`HumanDecision`、`HUMAN_DECISION_POLL_MS`、`HUMAN_*` CLI 命名空间；**W3a**：`WaitingHuman` 已产生、decide 命名空间不变、`RunControl` 新 variant（08 §8.10）；**W4a**：`DSL_FOREACH_INVALID`（19 码）+ 4 个 `RUN_FOREACH_*`、`ItemIndex`/`ExpansionId`；TraceEvent/RunMetrics |
| 01 | [01-领域模型与状态机](01-领域模型与状态机.md) | 421 | CompiledGraph/EdgeIndex、实例模型、**迁移矩阵**、**实例寻址**、Delivery/Publish、RunState | `EdgeRef(EdgeIdx)` 取代自引用（EdgeIndex=结构体，ISSUE-043）；`Delivery.to_instance` 必填；单实例语义；两阶段 publish；**W2a0**：`CompiledEdge.on_gate_false`、`GateResultRecord.deferred/attempt/verdict`、`DeferredReceipt`（§1.4.1）；**W2b**：`CompiledNode.max_fanout`；**W3a0**：`HandlerRef::Human`、§1.2.3 WaitingHuman 迁移、§1.2.4 人工等待/output、`HumanDecisionRecord`（§1.5）；**W3a**：9 状态/13 迁移实测、input 快照时点；**W4a**：item 实例 `(node,item)`/attempt 模型、controller buffer、join（01 §1.2.5） |
| 02 | [02-DSL与校验](02-DSL与校验.md) | 984 | DSL schema 最终版、修正示例、**R1–R16 校验**、错误对象、符号命名、`graph_hash` | end 必须有输入；两级唯一性；子图加载通过/运行拒绝；`created_at` 可选；自环不豁免环检测；**W2a0**：`on_gate_false` + R15（`G_all` 可重入判定）；**W2b**：`GraphNode.max_fanout` + R16（静态扇出上限，执行顺序 `R1→R13→R15→R16→R14`；复用 `DSL_PARSE_ERROR`）；**W3a0**：`builtin.human` 保留 python_node 符号（§2.5；DSL schema/R1–R16/hash 不变，不 import/无 `NODE_WORKER_IMPORT`） |
| 03 | [03-调度器与生命周期](03-调度器与生命周期.md) | 675 | 执行拓扑、**所有权模型**、主循环、投递应用、**冷峻失败**、暂停/恢复/取消、超时/看门狗、终止判定 | RunState 单任务独占 + channel 回传；gate off-loop（含 `verdict`）；Skipped 级联；双通道 trace；**W2a0**：defer 应用（§3.4.3）、按端口继承 + settled 重置、dormant/确定性死锁；**W2b**：每激活扇出上限（§3.4.4，应用任何边前失败、无 GateResult）；**W3a0**：人工等待与决策应用（§3.4.5）、控制通道 `HumanDecision`、决策期限（缺省无期限）、`is_quiescent` 含 WaitingHuman；**W3a**：consumer/期限/终止 as-built（E-6 边界）；**W4a**：§3.4.6 展开/join/失败/取消/上限 |
| 04 | [04-节点接口与扩展点](04-节点接口与扩展点.md) | 939 | NodeHandler、NodeContext、Python 契约、内置节点、GateContext、SymbolResolver | W3a0 起产生 `WaitingHuman`（**仍无 `resume`**：decide 自动接续）；签名只硬校验 arity；严格 bool；整包广播；worker 子进程 + 全局并发上限（v2，见 §8）；**W2a0**：`GateResultRecord` 三字段例外（§4.6.4）；**W3a0**：`builtin.human` 内置等待 handler（§4.4.5）；**W3a**：精确映射实测、L2 replay handler 替换；**W4a**：foreach region 内 human per-item 注记 |
| 05 | [05-AgentHarness](05-AgentHarness.md) | 1429 | AgentConfig、LlmProvider(genai)、六状态机、上下文预算、ToolDef、错误映射、流式、AgentProvider | 流式唯一接口；预算冷峻；重试仅建立阶段；iteration=LLM 轮次；output 固定四字段 |
| 06 | [06-存储与可观测](06-存储与可观测.md) | 1271 | SQLite DDL v1（8 表 + v2 L2 记录表/列）、seq 语义、双通道写线程、Streaming 聚合、CLI 契约、崩溃标定 | 落聚合分段而非逐 token；`events.seq` 权威；崩溃标 `Failed+interrupted`；`NodeError` JSON canonical（§6.2.5.1）；不做断点续跑；**W2a0**：trace schema v4（`gate_results.deferred/attempt/verdict_json`，§6.7 迁移）；**W2b**：canonical JSON 增 `kind` 值 `FanoutExceeded`（DDL 不变）；**W3a0**：trace schema **v5**（`WaitingHuman` CHECK 重建、`human_decisions` 表、§6.7 迁移/只读拒绝）、C13 写者、`zgraph decide`（§6.10.8）与 `trace` 决策行；**W3a**：迁移/decide/`:memory:` 实测；**W4a**：schema v6（item 列/`foreach_expansions`/迁移） |
| 07 | [07-验收用例](07-验收用例.md) | 484 | AC-01..AC-17 + DoD 检查单 | 示例图/gate/Skipped/trace/坏图/冷峻/超时/不阻塞/真 LLM/并行/暂停取消/死锁/多 run/software_team SOP v1/**deferrable gate + 有限返工（W2a0）**/**每节点扇出上限（W2b，AC-15 a–d）**/**人工评审 human lifecycle + decide（W3a0 设计；**W3a 实测 AC-16 a–g：human_decide 21/0、e2e 85/0/2、workspace 819/0/3**）**/**动态 N + foreach（W4a 设计冻结，AC-17 a–i）** |
| 08 | [08-审计关闭矩阵](08-审计关闭矩阵.md) | 564 | 76 条逐条关闭状态 + 37 条一致性复核 + 可开工判定 + **§8.6 回填记录** + §8.9 W2a0 注记 + §8.9.1 W2a 实现证据 | 60 已关闭 / 6 部分 / 7 延后 / 3 不适用；回填后 **10/10 可开工**；§8.9/§8.9.1 只追加增量（不改历史快照）；**W3a**：§8.10 实现证据 + source-breaking 注记 |

## 建议阅读顺序

1. **决策者**：`../analysis/decision-record-01-scope.md` §1/§2 → 本索引 → 08（关闭矩阵与可开工结论）
2. **写 Loader**：00 §0.2–0.4 → 01 §1.1 → 02 全篇
3. **写调度器**：00 §0.6 → 01 §1.2–1.7 → 03 全篇
4. **写 Python 扩展层**：04 全篇 + `../analysis/spike/REPORT.md` §3
5. **写 Harness**：05 全篇 + `../analysis/research-harness-alternatives.md` §5
6. **写存储/CLI**：06 全篇
7. **验收**：07 全篇

## 本版冻结的关键结论（与 v1.1 的差异）

| 主题 | v1.1 | v1.2 |
|---|---|---|
| 执行模型 | `tokio::spawn` 直接跑节点 | 事件循环仅调度；Python 一律 **worker 子进程**（v2；M2 用 in-process FakeExecutor 打通切片），全局/每 run 并发上限 |
| 门控求值 | 事件循环内 | 执行任务内 off-loop，结果随 `NodeCompleted` 回传；v2 gate = 内置 op（D6） |
| 实例寻址 | `Delivery` 无目标实例字段 | `to_instance` 必填；单实例 + 自环新 attempt；**W4a**：foreach region 内按 `(node_id,item_index)` 定位 |
| 数据路由 | 未定义 | 整包广播；`outputs/from_output_port` 保留到 v2 |
| 失败语义 | 不自动停止，靠 gate 决定 | **冷峻**：Crashed/TimedOut → run Failed；gate 永久拦截 → Skipped；**defer 边** gate=false → 挂账（W2a0） |
| run 终止 | "全 Completed" 与"全终态"矛盾 | 无活跃实例即静默；冷峻失败优先；死锁宽限判定 |
| 超时 | 30s 实例 vs 60s LLM | 180s/60s/30s 三层 + **kill 进程组**（v2 D1；`killed_workers`，不再 stop-waiting） |
| 人工介入 | 与"只跑一次"冲突 | v1 不做（枚举保留），v1.1 用子 trait 扩展；**W3a 已实现**（decide 自动接续） |
| 动态 N | 延后（codegen 按 N） | **W4a 设计冻结**：节点级 `foreach` 运行期展开 + 有序 join（DR-007；W4b 实现） |
| DSL 校验 | 10 条、示例自身非法 | **18 条 R1–R18**（R15 = W2a0 defer 可重入、R16 = W2b 静态扇出上限、R17/R18 = W4a foreach 形状/区域；新码 `DSL_FOREACH_INVALID`，DSL 域 19）、示例修正、错误定位结构化 |
| 存储 | 只有 TransitionRecord | 8 表 DDL + **双通道（M6 已拆回 8192+1024；REV-041）+ 发送序号有序合并** + 聚合分段 + CLI 契约 |
| 前端/API | 无设计 | 明确 v1.1+（本次范围外） |

## 验证方式

- 规格本身：08 分册做逐条关闭核对 + 跨册一致性复核；**§8.6 回填记录**记录 lead 对 9 高 + 4 缺口 + 28 中低（27 回填 / 1 保留）的处置，回填后判定 **可开工 10/10**；**M0–M2 回写批次**见 PROVENANCE §2 第 4 条，**M4 Harness 回写批次（2026-10-30，REV-023）**见 §2 第 7 条，**M5 回写批次（2026-10-30，REV-035）**见 §2 第 8–10 条（wave1 / wave2 / ISSUE-066）；**M6 回写批次（2026-10-31，REV-057）**见 §2 第 13 条；**W1 SOP v1 回写批次（2026-10-31，REV-066）**见 §2 第 14 条；**W2a 实现回写批次（2026-10-31，REV-073）**见 §2 第 17 条；**W2b spec 回写批次（2026-10-31，REV-077）**见 §2 第 18–19 条；**W3a0 人工评审设计冻结批次（2026-10-31，REV-078）**见 §2 第 20 条（设计冻结、不写 as-built）。
- 技术前提：`../analysis/spike/REPORT.md` 的 8 项实测（PyO3/GIL/超时/事件循环/SQLite）已全部跑通。
- 验收：07 的 AC-01..AC-17，P6 阶段以 `cargo test` + CLI + SQLite 断言执行（AC-16 已由 REV-083 实测：human_decide 21/0；AC-17 设计冻结，W4b 实测）。
- **W2a0 回写批次（2026-10-31，REV-067）**：见 PROVENANCE §2 第 15 条（新增 AC-14、R15/`DSL_DEFER_NOT_REENTRANT`、trace schema v4、SOP v1.1）。
- **W2a 实现回写批次（2026-10-31，REV-073）**：见 PROVENANCE §2 第 17 条（按已合并 dsl/trace/py-worker/kernel+cli 实现校准 AC-14/as-built 注记/08 §8.9.1；REV-072 已通过、e2e 已合并 `main` `e2cba52`；c/e 运行期以 `--profile rework-item` 5/7 单 item 图校准；REV-073 复审通过）。
- **W2b spec 回写批次（2026-10-31，REV-077）**：见 PROVENANCE §2 第 18–19 条（按已合并 dsl `220f53c`/kernel `a4cb1b1`/e2e `9a91076` 实现回写 R16 静态扇出上限、运行期 `NODE_FANOUT_EXCEEDED`、`CompiledNode.max_fanout`、AC-15；默认无 `max_fanout` 行为/hash 与 v0.1.0 逐字节一致）。
- **W3a0 人工评审设计冻结批次（2026-10-31，REV-078）**：见 PROVENANCE §2 第 20 条与 [`DR-006`](../decisions/DR-006-w3a-human-decide-freeze.md)（human lifecycle/decide 自动接续/`human_decisions`/trace v5/AC-16 设计；**不写 as-built**，W3a 实现后由 W3a-docs 回写）。
- **W3a 实现回写批次（2026-11-01，REV-084）**：见 PROVENANCE §2 第 22 条与 DR-006 §15（按已合并 trace/kernel/cli/e2e 实现校准 as-built、AC-16 实测、08 §8.10 source-breaking）。
- **W4a 动态 N 设计冻结批次（2026-11-01，REV-086）**：见 PROVENANCE §2 第 23 条与 [`DR-007`](../decisions/DR-007-w4a-dynamic-fanout-freeze.md)（foreach 语法/R17/R18/trace v6/AC-17 设计/SOP v1.2；**不写 as-built**，W4b 实现后由 W4b-docs 回写）。

## 仍未覆盖 / 延后项（诚实清单）

| 项 | 处置 |
|---|---|
| API 服务（REST/WS） | v1.1+，无设计 |
| 前端编辑器 / Trace 视图 | v1.1+，无设计 |
| 人工介入全链路（WaitingHuman/续跑） | **W3a 已实现**（`builtin.human`/`WaitingHuman`/decide 自动接续，REV-079–083；as-built REV-084）；durable resume（run 退出后恢复）仍延后（W3b 可选，需用户确认） |
| 子图执行 | v1.1+（定义/校验保留，运行期 `NODE_TYPE_UNSUPPORTED`） |
| 断点恢复（进程重启续跑） | W3b（可选，需用户确认）；W3a 只做 run 存活期间的人工停等/decide（崩溃后未消费决策留 W3b） |
| Memory / KB 真实实现 | v1 返回空（接口保留） |
| MCP 工具 | v1.1+（`rmcp` 可选） |
| 进程隔离 / 强制终止 Python | **v2 已实现**：worker 子进程 + 进程组 kill（architecture §3/§4；偏差 D1/D4） |
