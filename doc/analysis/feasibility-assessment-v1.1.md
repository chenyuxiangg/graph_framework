> **归档说明**：本文件是《详细设计 v1.1》的可开发性评估（冻结期归档，路径为历史引用）。当前落地形态见 [`DR-001`](../decisions/DR-001-runtime-route.md) 与 [`architecture-v2`](../architecture-v2.md)。

# 《图模型驱动工作流框架·详细设计 v1.1》可开发性评估

> 评估对象：`/Users/zyzs/Downloads/详细设计-v1.0.html`（正文标题 v1.1，文件名 v1.0，DSL `version: "1.0"`）
> 评估方式：全文结构分析 + 规格审计（子代理，76 条缺陷清单）+ 技术 spike（子代理，8 项真实编译运行探针）
> 环境：macOS 26.6 arm64 · rustc 1.99.1 · Python 3.13.0 (GIL) · PyO3 0.30.1 · tokio 1.54.1 · SQLite 3.51
> 代码仓库：`/Users/zyzs/lczyzs/code/zgraph`（greenfield）
> 证据：`_audit/spec-audit.md`（1009 行）、`_spike/REPORT.md`（394 行 + 15 份原始日志）、`_research/codex-embed.md`

> **2026-10-28 更新**：6 项决策已拍板（范围=只做内核、Harness=自研薄壳不复用整包、失败=冷峻、人工介入暂不做、超时=停止等待、验收=§5 断言）。
> 范围边界、Harness 选型论证、修订后的工作量（**20–27 人日 / 8–10 会话**）与 DoD 见 **[决策记录-01](decision-record-01-scope.md)**；本文 §7 保留为决策前的备查记录。

---

## 0. 结论

**能完成，但不能"照着文档直接编码"。需要先做 1–2 天的规格冻结，再按里程碑开发。**

三句话概括：

1. **技术上没有死路。** 最难的 20%（PyO3 0.30 嵌入 Python 3.13、JSON 双向转换、Python↔Rust 回调、gate 求值、异常结构化、SQLite trace 异步落库、单线程事件循环 + channel 回传）我已全部真实编译跑通，8 个探针全部通过。**不是"纸面判断能行"，是"已经跑给你看了"。**
2. **文档是"语义规格"，不是"施工图"。** 第 1 章 DSL 可用度约 70%；第 2–6 章可直接实现度 20–55%；前端 / API / SQLite schema / Agent 资产平台可用度 <10%。审计共查出 **76 条缺陷（43 条阻塞级）**，其中约 30 条是文档补写，不是编码。
3. **有 3 个设计假设被实测证伪**（执行模型、并行度、超时"杀死任务"），必须改架构决策；另有 8 项领域语义必须先冻结（状态机、实例寻址、数据路由、run 启动/终止、超时、人工介入、子图、范围）。

**一句话给决策者：这份文档足以支撑一个"图调度内核 + Agent 执行壳"的 MVP，前提是先花 1–2 天把 43 条阻塞项里真正影响架构的 8 项定死；完整产品还需要补 5 份外围设计。**

### 量化结论

| 指标 | 数值 |
|---|---|
| 文档规格体积 | 6 章 / 30 节 / 1451 行；14 struct、5 enum、1 trait、15 个 DSL 错误码 |
| 规格缺陷 | 76 条 = 内部矛盾 26 + 不可编译伪代码 12 + 缺失规格 20 + 语义边界 18；**阻塞级 43** |
| 技术探针 | 8/8 通过；**0 个阻塞级技术障碍**；3 处执行模型假设被证伪 |
| 核心后端工程量 | **24–32 人日**（不含前端）；约 15–18k LOC Rust + Python |
| 含前端（若在范围） | +5–8 人日，约 4–5k LOC TS |
| 我的交付节奏 | **10–13 个可验收里程碑会话**（P0 规格冻结 → P6 端到端） |
| 按现文档直接开工 | **不可以**（第 3/4 章伪代码不可编译 + 5 个外部契约缺失 + run 无法启动/终止） |

---

## 1. 我对文档的理解（一页纸）

框架 = **编译期**（Graph DSL JSON → 10 条校验 → CompiledGraph + 双向边索引）+ **运行期**（单线程事件循环驱动 实例状态机 + Input Buffer + Pub/Sub + Gate）+ **执行侧**（Agent Harness 或 Python 节点，统一 `run(inputs, ctx) -> output`）+ **可观测侧**（Transition/Streaming/Gate 事件 → SQLite → WebSocket）。

六个核心设计：

1. **DSL 是前后端唯一事实源**：节点 5 类（`agent_call / python_node / subgraph / start / end`），边带可选 `gate`，输入端口显式声明类型 + `required`，`trigger: all|any`。
2. **编译期建双向索引**：`outgoing`（发布扇出 O(1)）+ `incoming`（来源校验 O(1)），运行期不扫全表。
3. **三条通道隔离**：数据通道（同步写 buffer，保证 publish 返回即已投递）、事件通道（异步 mpsc → SQLite → WS）、控制通道（Pause/Resume/Cancel 优先）。
4. **实例状态机 + 6 条不变式**：`Pending→Ready→Running→终态` 单调前进；每实例只 `run()` 一次（自环=新实例）；执行前 required 必在 buffer；trace 必落库；run 终态唯一。
5. **Agent Harness 统一执行壳**：Loading → Assembling → LLM ↔ Tool → Returning/Failed；上下文按 `system(角色+约束+KB+长期记忆)/短期记忆/用户输入` 固定顺序 + token 预算；错误矩阵区分"瞬时故障可重试"与"设计错误直接 Crashed"。
6. **SymbolResolver 解耦扩展**：DSL 字符串符号在图加载期解析（Python `importlib` + `inspect.signature` 签名校验），执行期 O(1) 查表；热拔插只到"图级"。

**这是"事件驱动图执行内核 + LLM Agent 执行壳"的规格书，语义写得很实，生态侧（资产/存储/前端/API）留白。**

---

## 2. 技术验证：8 个探针的实测结果

全部代码在 `_spike/`，可用 `bash _spike/scripts/run_all.sh` 一键复现；原始日志在 `_spike/logs/`。

| # | 设计假设 | 实测 | 判定 |
|---|---|---|---|
| 1 | PyO3 嵌入 Python 3.13，`run(inputs,ctx)` / gate 动态导入，JSON↔PyObject 双向 | dict/list/嵌套/None/Unicode/i64 精度全部往返稳定；异常 + traceback 完整 | ✅ 直接落地 |
| 2 | 「节点执行 `tokio::spawn` 不阻塞事件循环」 | **证伪**：4×Python `sleep(0.4)` 让事件循环 ticker 停顿 **405 ms**；4×CPU 忙等停顿 **1502 ms**；单线程 runtime + `spawn` 把 4×sleep 串行成 **1614.8 ms** | ❌ 必须改 |
| 3 | 「并行分支的 Python 节点可并行」 | **部分证伪**：4×0.4s CPU 并发 **1608–1686 ms ≈ 4×（串行）**；GIL 释放型 sleep 才 **405 ms（≈1×）**。CPU 型 Python 吞吐 ≈ 1 核 | ⚠️ 需改预期 |
| 4 | `ctx.emit` 同步回调 + 有界满时丢弃 | 安全：3.86 µs/次、10k 次 38.6 ms、无死锁；cap=2 时丢 98/100（需 dropped 指标） | ✅ 直接落地 |
| 5 | 节点 30s 超时"不等待任务返回" | 只能"停止等待"（103–106 ms 返回），**任务继续跑**：drop runtime 要等 1890.7 ms，期间其它 Python 调用变慢 8.9×～113×；`PyErr_SetInterrupt` 完全无效 | ⚠️ 语义必须重定义 |
| 6 | Python 异常 → Rust 结构化错误，解释器可继续 | 通过：自定义异常类型/消息/属性/traceback 全保留，连续 3 轮失败后解释器健康 | ✅ 直接落地 |
| 7 | trace 走 mpsc 异步落 SQLite，不阻塞主循环 | 通过：10k 条 **36.1 ms**、主循环 ticker 停顿 **6 ms**；同步 autocommit 要 **133.9 ms**（不可接受） | ✅ 直接落地 |
| 8 | 单线程事件循环 + channel 回传，避免 `&mut` 别名与 `'static` | 通过：最小 DAG 引擎 **310.7 ms**（串行 500 ms，1.61×），ticker 停顿 6 ms；反例（`spawn` 捕获 `&mut self`）编译失败 E0521/E0382 | ✅ 直接落地 |

### 2.1 由此确定的执行模型（替代文档 4.2 的 `tokio::spawn` 写法）

```
事件循环（current_thread runtime，只做调度，禁止任何同步 Python 调用）
  └─ 就绪节点 → spawn_blocking / 专用 Python 执行线程池（独立并发上限）
       └─ 线程内 Python::attach → import → run(inputs, ctx)
       └─ 完成/失败 → mpsc 回传 NodeEvent → 事件循环更新独占 HashMap → 调度后继
```

- **Python 节点并发上限必须独立于 tokio worker 数**：CPU 型建议 1，I/O 型可 >1。
- **超时改为"停止等待 + 记录 timeout 事件 + 释放调度槽"**，并新增"被放弃任务"记账（防止无限累积占满 blocking 池；`Runtime::drop` 会等它们，优雅退出需 `shutdown_timeout`/`shutdown_background`）。
- 若要"30s 必杀且资源必回收"，唯一可靠方案是**进程/子解释器隔离**——这是比现设计更重的架构决策，需单独立项。
- `gate` 返回值必须定"严格 bool"：实测 `str/int/None` 提取都会 TypeError。

---

## 3. 规格完备度盘点

| 章节 | 可直接实现度（审计口径） | 关键缺口 |
|---|---|---|
| 1. Graph DSL | ~70% | **1.2 示例自身违反 1.3 规则 6**（end 节点 `inputs: []` 却接 `result` 端口）；多源同端口规则冲突；**节点只有 inputs 没有 outputs**（字段级路由无法表达）；子图缺 input/output_mapping |
| 2. 运行期数据流 | ~55% | `Delivery` 缺 `to_instance_id` 却被伪代码使用；`EdgeRef` 持 `&'static` 自引用不可构造；实例寻址缺失；buffer 双写 |
| 3. Agent Harness | ~25% | **5 个外部契约全缺**（AgentConfig / Memory-MODULE-09 / Skill-ToolDef / LLM Provider 含流式 / KB）；伪代码不可编译；`chat()` 阻塞返回与 3.7 逐 token 流式矛盾 |
| 4. Scheduler | ~20% | 主循环 `&mut` 别名冲突与 `tokio::spawn` 的 `'static` 冲突；无超时事件类型；终止判定三处不一致；暂停/取消未定义 |
| 5. 节点回调接口 | ~55% | `NeedHumanInput` 语义矛盾；emit 不可编译；`NodeContext` 无构造器；错误映射缺失 |
| 6. 扩展点/符号解析 | ~50% | 符号命名三套并存；热更新承诺与全局 Registry 矛盾；签名校验按文档实现恒失败（`return_type` 比较） |
| 外围：API/WS、前端、SQLite/Checkpoint、Python 运行时 | **<10%** | 只在架构图出现，没有可实现契约 |

**完全缺失、但完整产品必需的 10 项**：前端编辑器、Trace 视图、API/WS 层、SQLite schema 与恢复语义、AgentConfig schema、Memory Manager（MODULE-09 未提供）、Skill/ToolDef 契约、LLM Provider 流式接口、Python 运行时管理、错误码全集。

---

## 4. 开工前必须冻结的 8 项

> 审计的 43 条阻塞项里，真正影响架构的收敛为下面 8 项。**不冻结就写代码 = 后期重构**。

| # | 冻结项 | 现状问题（文档） | 建议裁定（可直接采纳） |
|---|---|---|---|
| F1 | **范围**：Harness 5 个外部依赖 | 只有一行契约，无 schema | **接口冻结 + 本地 stub**（echo LLM / 内存 Memory / 空 KB），真实实现另立项 |
| F2 | **领域模型**：InstanceStatus / RunStatus 迁移表 | 枚举未定义；Cancel 无 Cancelled；WaitingHuman 与"只跑一次"冲突 | `Pending/Ready/Running/WaitingHuman/Completed/Crashed/TimedOut/Cancelled/Skipped` + 显式迁移矩阵 |
| F3 | **实例寻址与投递** | `Delivery` 无 `to_instance_id`；同节点多实例投给谁无规则 | 补字段；下游实例按 `(node, attempt)` 懒创建；自环/重试 attempt 上限 |
| F4 | **数据路由** | 节点只有 inputs；输出是整包广播还是端口级未定；示例中 `inputs["user_query"]` 假设端口拿到值本身 | v1：**整包广播 + 节点内取字段**（改 5.3 示例），DSL 预留 `outputs/from_output_port` 到 v2 |
| F5 | **调度器所有权 + 执行模型** | 4.2 伪代码不可编译 | 事件循环独占状态 + channel 回传 + `spawn_blocking` + Python 并发上限（§2.1，已实测） |
| F6 | **run 启动与终止** | 没有 entry 输入注入机制；Crashed 不 publish 所以 gate 无法"决定后续"；watchdog 只扫 Running；"全 Completed" 与 "全终态" 矛盾 | 定义 `RunSpec{entry_inputs,...}`；v1 **冷峻模式**（任一 Crashed → run Failed，gate 永久拦截 → Skipped）；终止判定 = 无活跃实例 |
| F7 | **超时/取消语义** | 实例 30s 必先于 LLM 60s 到期；无超时事件；取消是否中断在飞 LLM 未定 | 实例超时 ≥ LLM 超时×(重试+1)（建议 180s/60s/1 次）；超时=停止等待；协作式取消标志；在飞 LLM 尽力 abort |
| F8 | **人工介入协议** | `Err→Crashed` 与 `Err→WaitingHuman` 并存；恢复方式未定 | v1 推荐**续跑**（Harness 保留会话，新增 resume 入口，Python 节点不支持等待人工）；`human_input` 端口 + JSON schema 校验 |

**另外 2 项范围确认**：F9 子图运行期语义（单 End、mapping 方向、嵌套实例归属）；F10 前端/API/Checkpoint 是否本次交付。

---

## 5. 逐模块可开发性与工作量（修订版）

> 初版估算 19–24 人日偏乐观：未计"规格冻结"，且 spike 证明超时/取消/被放弃任务记账比文档设想复杂。整合后上调约 30–40%。

| 模块 | 章节 | 人日 | 复杂度 | 主要风险 |
|---|---|---|---|---|
| P0 规格冻结 + 工程骨架 + types | — | 1.5–2 | 中 | 决策等待是唯一阻塞 |
| M1 DSL/Loader/10 条校验/编译期图/边索引 | 1, 2.2 | 2–3 | 中 | 规则 7/10 与示例非法（C-15） |
| M2 实例状态机 + Buffer + 就绪判定 + 不变式 | 2.4–2.7 | 2–3 | 中 | any 模式与 required 协调 |
| M3 Scheduler/生命周期/暂停恢复取消/终止/看门狗 | 4 | 3.5–4.5 | **高** | 最容易出并发与卡死 bug |
| M4 PubSub 扇出 + Gate | 2.4.4, 6 | 1–1.5 | 中 | gate 严格 bool / ctx 类型 |
| M5 PyO3 运行时 + SymbolResolver + 执行器 + NodeContext | 5, 6 | 3–4 | **高** | 执行模型（已定）、并发上限、解释器生命周期 |
| M6 Agent Harness + 上下文预算 + tool 循环 + 流式 + Provider | 3 | 4–5 | **高** | 5 个外部契约缺失、真实 LLM 联调 |
| M7 trace/SQLite/WS 推送 | 2.5, 3.7 | 1.5–2 | 低 | schema 自定、关键事件不可丢 |
| M8 API 层（REST + WS） | 无 | 1.5–2 | 中 | 无规格，取决于调用方 |
| M9 子图递归执行 | 1.1, 2.2 | 1.5–2 | **高** | 语义最模糊 |
| M10 human-in-loop | 2.7, 3.4 | 1–1.5 | **高** | 与不变式冲突（需 F8 先定） |
| M11 示例图 + e2e + 压测 + 文档 | — | 1.5–2 | 低 | 验收标准 |
| **后端合计** | | **24–32** | | |
| M12 前端编辑器 + Trace 视图（可选） | 无 | 5–8 | 高 | 0 设计 |

---

## 6. 里程碑计划

| 里程碑 | 产物与验收 | 会话数 |
|---|---|---|
| P0 | 规格冻结 v1.2（§4 的 8 项 + 决策记录）+ cargo workspace + `types.rs` + DSL 类型 | 1 |
| P1 | Loader + 10 条校验全用例 + 编译期图；客服工单示例（修正后）解析通过 | 1 |
| P2 | 调度内核：`start → python_node → end` 跑通；不变式断言全绿；终态/卡死判定有测试 | 1.5 |
| P3 | PyO3 扩展层：DSL 符号解析、签名校验、异常 traceback、`ctx.emit`；坏输入报对应错误码 | 1.5 |
| P4 | Harness：真实模型一次带 tool 调用的 agent 节点跑通；流式 token 到 WS | 1.5 |
| P5 | 可观测 + API：SQLite trace + WS + REST + 暂停/恢复/取消 + watchdog | 1 |
| P6 | 子图 + 自环/重试 + e2e 客服工单流 + 20 并发 run 稳定性 + 文档 | 1.5 |
| P7（可选） | 前端编辑器 + Trace 视图（需先出交互稿） | 2–3 |

**累计 10–13 个会话；每个会话结束都有可运行、可验收的东西，不做"全部写完再联调"。**

---

## 7. 需要你拍板的 6 件事

| # | 决策 | 选项与推荐 |
|---|---|---|
| 1 | **本次范围** | A 内核（推荐先做）/ B 内核+API / C 全栈含前端 |
| 2 | **Harness 外部依赖** | 接口 + stub（**推荐**）/ 本次实现全部 / Agent 整体 v2 |
| 3 | **失败语义** | v1 冷峻模式：Crashed → run Failed，gate 永久拦截 → Skipped（**推荐**，并在 DSL 预留 `on_error` 字段） |
| 4 | **人工介入** | v1 做续跑 / v1 不做（推荐 v1 不做，只保留 pause/resume/cancel）/ 重跑语义 |
| 5 | **超时强制终止** | 停止等待 + 协作取消（**推荐**）/ 必须真杀 → 进程隔离立项（+5–8 人日） |
| 6 | **验收 demo** | 文档 1.2 客服工单流 + 哪些断言（推荐：5 节点跑通、1 个 gate 拦截分支、trace 可回放、坏图报正确错误码） |

另：缺失的 5 份输入（前端设计、API 契约、SQLite schema、AgentConfig/MODULE-09、LLM Provider 接口）**我可以按 F1–F8 的裁定起草并请你确认**，不必等外部文档。

---

## 8. 风险登记（实测后更新）

| 风险 | 概率 | 影响 | 缓解 |
|---|---|---|---|
| 执行模型选错导致重构 | ~~中~~ **已消除** | 高 | spike 实测锁定 `spawn_blocking` + channel 回传 |
| CPU 型 Python 节点被 GIL 串行，并行分支不达预期 | **高（已实测）** | 中 | 文档写明"并行只对 I/O 型有效"；CPU 型节点并发上限 1；重计算迁 Rust 或子进程 |
| 超时后任务继续占用 GIL/线程，拖慢全局 | **高（已实测）** | 高 | 被放弃任务计数 + 告警；协作式取消；必要时进程隔离 |
| 领域语义未冻结就编码 → Delivery/状态机/trait 返工 | 高 | 高 | P0 冻结 F1–F8 后才允许 P2/P4 编码 |
| Harness 外部契约（F1）被当成本次必做 | 中 | 高 | 范围决策；接口 + stub |
| 子图语义按我的理解实现，与预期不符 | 中 | 中 | 先出 1 页契约确认，再编码 |
| 前端 0 设计导致返工 | 高 | 中 | 本期不做；做了先出交互稿 |
| 真实 LLM 联调（额度/模型差异/网络） | 中 | 中 | Provider trait + fake provider；e2e 可切换 |
| 本机 crates.io 直连不通、沙箱限制 `~/.cargo` | **已发生（已解决）** | 低 | `_spike/scripts/env.sh`：代理 127.0.0.1:7890 + 私有 `CARGO_HOME`；CI 需同样配置 |

---

## 9. 交付物

| 文件 | 内容 |
|---|---|
| `docs/可行性评估-详细设计v1.1.md` | 本报告 |
| `_audit/spec-audit.md` | 76 条缺陷明细（每条的章节引用、行号、开发卡点、建议裁定、决策人），可直接转 decision log |
| `_spike/REPORT.md` + `_spike/logs/` | 8 项实测探针、原始日志、结论表、第 3/4/5 章执行模型建议 |
| `_spike/`（Cargo 工程 + py + scripts） | 可复现的技术验证骨架；`target/` 与 `.cargo-home/` 为构建缓存（约 400MB，可 `rm -rf`） |

**结论重申：按现文档直接开工不行；冻结这 8 项后，我可以按 10–13 个里程碑把它做出来。**
