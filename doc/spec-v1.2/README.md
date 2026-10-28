> **落地说明（DR-001）**：本规格是**语义基准**；宿主语言与技术形态已由
> [`doc/decisions/DR-001-runtime-route.md`](../decisions/DR-001-runtime-route.md) 冻结为
> **Rust 内核 + Python 子进程 worker**。阅读时请做如下替换：
> - PyO3/进程内 Python 调用 → **worker 子进程 + NDJSON 协议**（见 `doc/architecture-v2.md` §3）
> - `NodeContext`（Rust 对象）→ Python worker SDK 的 `ctx`（emit/log/get_var/user_id）
> - 门控 Python 函数 → v1 先用 **Rust 内置 op**（自定义 Python 门控见 ISSUE-014）
> 其余语义（DSL、状态机、Delivery、冷峻失败、trace、验收）不变。

# 规格冻结 v1.2 · 索引

> **状态**：规格冻结（**纯文档阶段，未写任何内核代码**——按用户 2026-10-28 指示）。
> **目的**：把《详细设计 v1.1》的 43 条阻塞级问题收敛成可直接编码的契约，用作文档评审与后续开发的唯一依据。
> **上游**：`../可行性评估-详细设计v1.1.md`（评估）、`../决策记录-01-范围与Harness.md`（6 项拍板 + 范围边界）
> **依据**：`_audit/spec-audit.md`（76 条缺陷）、`_spike/REPORT.md`（8 项实测）、`_research/*`（Codex / Rust 生态 / DSH）

---

## 分册

| # | 文件 | 行数 | 内容 | 关键裁定 |
|---|---|---|---|---|
| 00 | [00-总纲](00-总纲.md) | ~240 | 命名/类型/枚举/**错误码**/**默认值**/**不变式**/术语消歧 | 权威锚点；I1–I9；17 个 DSL 码；18 个新默认常量；TraceEvent/RunMetrics |
| 01 | [01-领域模型与状态机](01-领域模型与状态机.md) | ~300 | CompiledGraph/EdgeIndex、实例模型、**迁移矩阵**、**实例寻址**、Delivery/Publish、RunState | `EdgeRef(EdgeIndex)` 取代自引用；`Delivery.to_instance` 必填；单实例语义；两阶段 publish |
| 02 | [02-DSL与校验](02-DSL与校验.md) | ~900 | DSL schema 最终版、修正示例、**R1–R14 校验**、错误对象、符号命名、`graph_hash` | end 必须有输入；两级唯一性；子图加载通过/运行拒绝；`created_at` 可选；自环不豁免环检测 |
| 03 | [03-调度器与生命周期](03-调度器与生命周期.md) | ~340 | 执行拓扑、**所有权模型**、主循环、投递应用、**冷峻失败**、暂停/恢复/取消、超时/看门狗、终止判定 | RunState 单任务独占 + channel 回传；gate off-loop；Skipped 级联；双通道 trace |
| 04 | [04-节点接口与扩展点](04-节点接口与扩展点.md) | ~850 | NodeHandler、NodeContext、Python 契约、内置节点、GateContext、SymbolResolver | v1 无 resume；签名只硬校验 arity；严格 bool；整包广播；spawn_blocking + 信号量 |
| 05 | [05-AgentHarness](05-AgentHarness.md) | ~1170 | AgentConfig、LlmProvider(genai)、六状态机、上下文预算、ToolDef、错误映射、流式、AgentProvider | 流式唯一接口；预算冷峻；重试仅建立阶段；iteration=LLM 轮次；output 固定四字段 |
| 06 | [06-存储与可观测](06-存储与可观测.md) | ~940 | SQLite DDL v1（8 表）、seq 语义、双通道写线程、Streaming 聚合、CLI 契约、崩溃标定 | 落聚合分段而非逐 token；`events.seq` 权威；崩溃标 `Failed+interrupted`；不做断点续跑 |
| 07 | [07-验收用例](07-验收用例.md) | ~185 | AC-01..AC-12 + DoD 检查单 | 示例图/gate/Skipped/trace/坏图/冷峻/超时/不阻塞/真 LLM/并行/暂停取消/死锁/多 run |
| 08 | [08-审计关闭矩阵](08-审计关闭矩阵.md) | ~410 | 76 条逐条关闭状态 + 37 条一致性复核 + 可开工判定 + **§8.6 回填记录** | 60 已关闭 / 6 部分 / 7 延后 / 3 不适用；回填后 **10/10 可开工** |

## 建议阅读顺序

1. **决策者**：`../决策记录-01-范围与Harness.md` §1/§2 → 本索引 → 08（关闭矩阵与可开工结论）
2. **写 Loader**：00 §0.2–0.4 → 01 §1.1 → 02 全篇
3. **写调度器**：00 §0.6 → 01 §1.2–1.7 → 03 全篇
4. **写 Python 扩展层**：04 全篇 + `_spike/REPORT.md` §3
5. **写 Harness**：05 全篇 + `_research/harness-alternatives.md` §5
6. **写存储/CLI**：06 全篇
7. **验收**：07 全篇

## 本版冻结的关键结论（与 v1.1 的差异）

| 主题 | v1.1 | v1.2 |
|---|---|---|
| 执行模型 | `tokio::spawn` 直接跑节点 | 事件循环仅调度；Python 一律 `spawn_blocking` + 独立并发上限（实测依据） |
| 门控求值 | 事件循环内 | 执行任务内 off-loop，结果随 `NodeCompleted` 回传 |
| 实例寻址 | `Delivery` 无目标实例字段 | `to_instance` 必填；单实例 + 自环新 attempt |
| 数据路由 | 未定义 | 整包广播；`outputs/from_output_port` 保留到 v2 |
| 失败语义 | 不自动停止，靠 gate 决定 | **冷峻**：Crashed/TimedOut → run Failed；gate 永久拦截 → Skipped |
| run 终止 | "全 Completed" 与"全终态"矛盾 | 无活跃实例即静默；冷峻失败优先；死锁宽限判定 |
| 超时 | 30s 实例 vs 60s LLM | 180s/60s/30s 三层 + stop-waiting + 被放弃任务记账 |
| 人工介入 | 与"只跑一次"冲突 | v1 不做（枚举保留），v1.1 用子 trait 扩展 |
| DSL 校验 | 10 条、示例自身非法 | 14 条 R1–R14、示例修正、错误定位结构化 |
| 存储 | 只有 TransitionRecord | 8 表 DDL + 双通道 + 聚合分段 + CLI 契约 |
| 前端/API | 无设计 | 明确 v1.1+（本次范围外） |

## 验证方式

- 规格本身：08 分册做逐条关闭核对 + 跨册一致性复核；**§8.6 回填记录**记录 lead 对 9 高 + 4 缺口 + 28 中低的处置，回填后判定 **可开工 10/10**。
- 技术前提：`_spike/REPORT.md` 的 8 项实测（PyO3/GIL/超时/事件循环/SQLite）已全部跑通。
- 验收：07 的 AC-01..AC-12，P6 阶段以 `cargo test` + CLI + SQLite 断言执行。

## 仍未覆盖 / 延后项（诚实清单）

| 项 | 处置 |
|---|---|
| API 服务（REST/WS） | v1.1+，无设计 |
| 前端编辑器 / Trace 视图 | v1.1+，无设计 |
| 人工介入全链路（WaitingHuman/续跑） | v1.1+（枚举与错误变体保留，不产生） |
| 子图执行 | v1.1+（定义/校验保留，运行期 `NODE_TYPE_UNSUPPORTED`） |
| 断点恢复（进程重启续跑） | v1.1+（v1 只做崩溃标定，不续跑） |
| Memory / KB 真实实现 | v1 返回空（接口保留） |
| MCP 工具 | v1.1+（`rmcp` 可选） |
| 进程隔离 / 强制终止 Python | 未实现（v1 超时=停止等待 + 协作取消；强制终止需单独立项） |
