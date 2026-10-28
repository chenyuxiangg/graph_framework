# DR-001 · 技术路线与复用策略（Rust 内核 + Python 子进程 worker）

- **状态**：已采纳（用户 2026-10-28 决策）
- **决策人**：用户；记录人：lead（agent）
- **关联**：ISSUE-007（技术路线）、`doc/architecture-v2.md`、`doc/spec-v1.2/`、`doc/analysis/spec-audit-v1.1.md`

## 1. 背景

产品定位（用户确认）：**通用图模型平台**——用户自定义图流程，生成期望结果，过程可复现；流程中可以跑 Agent，也可以不跑。"可复现"是产品承诺，不是附加项。

当前有两套资产：

| 资产 | 内容 | 问题 |
|---|---|---|
| 现有 Python 仓（graph_base） | 10,172 行；187 个测试（162 通过）；tick 轮询 + 子进程 worker + 门控迁移 + JSON 文件状态；无 LLM/Agent | 语义与 v1.1 不同；2 个 P0 缺陷；状态/调度模型不足以支撑"可复现 + Agent" |
| 《详细设计 v1.1》+ 规格冻结 v1.2 | 6 章语义；76 条审计缺陷已收敛为 v1.2（00–08 共 5,407 行、10 分册含索引/溯源）；技术 spike 8 项实测 | 0 行实现；原方案 Rust 进程内 PyO3 有 3 个执行模型假设被 spike 证伪 |

## 2. 决策

| # | 决策 | 说明 |
|---|---|---|
| D1 | **语义核心 = 《详细设计 v1.1》** | DSL（类型化端口/trigger/门控/子图）、实例状态机、Delivery、冷峻失败、trace 事件模型、Agent Harness、验收断言，均以 `doc/spec-v1.2/` 为准 |
| D2 | **宿主语言 = Rust** | 单一可分发二进制；内核类型安全；长生命周期、高可靠调度；为将来 wasm/cdylib 插件留余地 |
| D3 | **用户代码 = Python 子进程 worker**（非进程内 PyO3） | 进程隔离天然解决 GIL、崩溃污染、超时无法终止三个问题；worker 可独立升级/复用现有 Python 资产 |
| D4 | **现有 Python 仓 = 只读参考资产** | 复用其**语义与经验**（门控/返工/人工边/并行扇出/进程派发/测试场景），不直接作为产品代码；冻结为 `legacy/`，M5 后退役 |
| D5 | **事件驱动取代 tick 轮询** | tick 仅作为 CLI 内部驱动保留兼容；run 状态以事件循环 + SQLite trace 为权威，不再依赖文件轮询 |
| D6 | **可复现分级承诺** | L1 调度可重放；**L2 结果可重放（recorded outputs，不重算）**；L3 外部副作用不承诺、不提供 dry-run（见 architecture §5） |

## 3. 为什么不选另外两条路

| 方案 | 否决理由 |
|---|---|
| 全部继续 Python | 现有模型（tick + 文件状态 + 单 src/sink + 文件名端口）无法承载 v1.1 语义；单二进制分发与长期可靠性不足；"可复现"需要强类型事件账本，Python 宿主需要重建同等工程约束 |
| Rust + 进程内 PyO3（v1.1 原方案） | spike 证伪：`tokio::spawn` 直调 Python 使事件循环停顿 405–1502ms；CPU 型 Python 节点仍被 GIL 串行（4×0.4s ≈ 1.6s）；超时无法终止任务（放弃后占 GIL、拖慢全局 8.9×–113×）；进程内无沙箱，用户代码崩溃会污染内核 |
| Rust + Python 子进程（本决策） | 保留 Rust 的类型/性能/单二进制优势，同时把 Python 关进进程边界：GIL 隔离、崩溃隔离、超时可 `kill`、可并行、可版本化协议 |

## 4. 复用矩阵（去粗取精）

| Python 仓资产 | 处置 | 去向 |
|---|---|---|
| 3 张图配置（software_team / test_for_e2e / test_for_complex） | **保留为验收 fixture**，改写为 DSL v1.2 | `tests/fixtures/graphs/`（M1） |
| 门控语义（`check_output`、gate_value、返工自环、manual 边、parallel 扇出） | **语义移植**为 Rust 内置 gate op + 配置项 | `zgraph-kernel`（M2） |
| 状态/端口/终态判定 | 概念保留（类型化端口），实现重写 | `zgraph-kernel`（M2） |
| `ProcessDispatcher`（setsid、超时、回收、日志） | **经验移植**：进程组、超时 kill、僵尸回收、stderr 截断 | `zgraph-worker`（M3） |
| monitor 事件/告警 | 映射为 trace 事件与告警 | `zgraph-trace`（M2） |
| CLI（validate/run/tick/trigger-edge/rollback） | 保留 validate/run/trace，新增 `replay`/`--record`；tick 移除；manual 边用 `trigger` 子命令；**`rollback` 不迁移**（v1 无回滚语义，需人工重跑；如需要另立需求） | `zgraph-cli`（M5） |
| 187 个测试 | **作为行为参考**，挑 15–20 个转写为 e2e | `tests/`（M1–M5） |
| 17 个示例 worker | 挑选 3–5 个迁移为 worker SDK 示例 | `python/workers/`（M3） |
| zlog 本地库 | **不迁移**（隐式路径依赖）；worker SDK 用标准 `logging` + 结构化 stderr | — |
| tick 轮询 / JSON 文件状态 / 单 src-sink / 文件名端口 | **不迁移**（"粗"的部分） | — |

## 5. 后果与开放项

**后果（必须接受）**

- 双语言：Rust 内核 + Python worker；跨语言契约（协议）成为新的核心资产，需版本化与兼容测试。
- 每个 worker 一次进程冷启动（~50–100ms）；LLM/IO 场景可接受，高频小节点需 worker 池（后置）。
- 现有 10k 行 Python 不产生直接代码复用，只保留语义与测试价值（已在 M0 冻结为 `legacy/`）。
- 迁移期双份代码：`legacy/` 只读，新代码在 `crates/` 与 `python/`。
- **编码规范适用边界**：`CLAUDE.md` 不可修改且写明 Python 技术栈，本决策将其解释为“**Python 代码规范**”；Rust 代码遵循 `rustfmt` + `clippy -D warnings` + 等价约束（类型标注、snake_case、常量、函数 ≤ 60 行）。边界登记 ISSUE-019。

**开放项（M0–M1 内定稿）**

| # | 问题 | 结论期限 |
|---|---|---|
| O-1 | 自定义 Python 门控：v1 只支持 Rust 内置 op，还是加常驻 gate worker？ | M1 前（默认：内置 op） |
| O-2 | worker 是否复用池化（按 module 常驻）？ | M3 前（默认：先一节点一进程） |
| O-3 | 可复现 L2 的记录粒度 | **已定**：`--record` 默认 `hash`；L2 重放要求运行期 `--record=full`（architecture §5.2/§5.3；ISSUE-016 已关闭） |
| O-4 | legacy 是整体删除还是永久保留 | M5 决策 |
| O-5 | `doc/graph_base_design.md` 的定位（历史设计 vs 实现说明） | M0 随 README 调整 |

## 6. 验收锚点

本决策落地后，以 `doc/spec-v1.2/07-验收用例.md` 的 AC-01..AC-12 为验收基线；**机制替换清单**如下（详见 `doc/architecture-v2.md` §8 的 D1–D12）：

| # | v1.2 语义 | v2 落地 |
|---|---|---|
| 1 | 进程内 PyO3 调用 Python | worker 子进程 + NDJSON 协议 v1 |
| 2 | `NodeContext`（Rust 对象） | Python worker SDK `ctx`（emit/log/get_var/user_id/cancel/deadline；无 cancel_token） |
| 3 | 超时 = stop-waiting + `NodeAbandoned` | 超时 = **杀进程组**；`NodeAbandoned` 不产生；AC-06 改写 |
| 4 | 工具在 Harness 进程内执行、模块缓存、共享信号量 | 工具每次调用一个 worker 子进程；无缓存；全局并发上限 |
| 5 | 门控是用户 Python 函数 | v1 用 **Rust 内置 op**（`check_output`/`compare`/`manual`）；自定义门控 M1 定稿 |
| 6 | 图加载期 import Python 符号 + 签名校验 | 加载期只校验格式；存在性/签名在 worker 启动时校验 |
| 7 | 单解释器 + CPU/IO 信号量（1/4） | 每节点一进程；全局 `max_workers` + 每 run 上限；`exec_class` 忽略 |
| 8 | CLI 只有 validate/run/trace，v1 不做重放 | 增 `replay` 与 `--record`；L1/L2 重放；`--dry-run` 不实现 |
| 9 | 无 seed/时间注入 | `PYTHONHASHSEED=0`（spawn 设置）+ `run.seed` + `ctx.now()/ctx.random()` |

每个里程碑必须给出：可运行产物 + 测试 + 评审记录 + 进度/台账更新。
