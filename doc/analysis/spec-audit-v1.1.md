> **来源与用途**：本文件是《详细设计 v1.1》的独立规格审计（76 条缺陷），
> 已用于产出 `doc/spec-v1.2/` 规格冻结。保留在仓库内作为**决策依据与追溯材料**；
> 其中与 PyO3/进程内执行相关的结论，已由 DR-001 替换为"Rust 内核 + Python 子进程"。

# 规格审计报告：《图模型驱动工作流框架 · 详细设计 v1.1》

> **审计对象**：`/Users/zyzs/Downloads/详细设计-v1.0.html`（80KB，`lang=zh-CN`；正文标题写「详细设计 v1.1」，文件名仍为 v1.0，见 C-25/M-20 的版本问题）
> **复核用纯文本**：`/tmp/designdoc/doc.txt`（1451 行；本报告「」引文均可用行号 `Lxxxx` 在该文件核对）
> **审计范围**：文档第 1–6 章全部 38 个小节，以及 2.1/3.1 架构图中承诺但未展开的外围模块
> **产出用途**：为「能否完成开发」的评估提供**可直接转成待决策事项**的缺陷清单。每条都回答同一个问题：*开发时会卡在哪、需要谁做决定*
> **方法**：全文通读 + 三视角交叉比对（DSL 定义 / 运行期结构 / 时序走查）+ Rust 与 PyO3/tokio 语义推演；未做代码实验（由 spike 队友负责）
> **边界**：只判断"文档是否足以支撑开发"，不评价产品价值；文档未定义的模块按"缺失"处理，不做臆测（见附录 B）

## 0. 摘要

### 0.1 总体判断

**一句话结论：第 1 章（DSL）基本可落地；第 2–6 章只能作为"设计意图"阅读，按现文档无法直接进入编码阶段。**

三个硬事实决定这个判断：

1. **第 4 章（Scheduler）的伪代码按字面不可编译**：`&mut self.runs` 与 `&mut self` 方法互相借用在 4.2 主循环里同时出现（P-01）；`tokio::spawn` 的 `'static + Send` 要求与 `RunContext`（含 `&'static CompiledEdge`、未定义的 `clone_ref()`）冲突（P-02/P-03）；`Delivery` 连 `to_instance_id` 字段都没有却已被使用（P-04）。Scheduler 是"图内核"，内核写不出来则所有上层设计都是纸面。
2. **第 3 章（Harness）依赖的 5 个外部模块全部没有定义**：AgentConfig、LLM Provider、Skill/ToolDef、Memory Manager（MODULE-09）、KB 适配器——只有一行契约（M-01–M-05）。Harness 是"Agent 统一执行壳"，壳里依赖的东西全不在文档里。
3. **run 既无法启动也无法可靠结束**：没有定义 entry 实例创建与启动输入注入（S-01），也没有定义 Crashed / gate 拦截 / 卡死场景下 run 如何收敛（C-07/C-09/C-10/S-06/S-10），而 2.7 又承诺「run 最终必为 Completed / Failed 之一」。

同时必须公允地说：**第 1 章 DSL 与第 5 章回调接口的方向是对的**——类型定义齐全、扩展点边界（框架固定 vs 用户注册）判断准确、符号解析链路的思路清晰。问题集中在"从 schema 到可运行内核"的中间层。

### 0.2 缺陷统计

| 分类 | 条目 | 阻塞级 | 需裁定 | 可自行决策 |
|---|---|---|---|---|
| 1. 内部矛盾（C） | 26 | 13 | 12 | 1 |
| 2. 不可实现 / 不可编译的伪代码（P） | 12 | 7 | 2 | 3 |
| 3. 缺失规格（M） | 20 | 13 | 6 | 1 |
| 4. 语义边界未定义（S） | 18 | 10 | 8 | 0 |
| **合计** | **76** | **43** | **28** | **5** |

> 43 条阻塞级 = 开工前必须关闭的清单；其中约 30 条是**文档补写/评审**工作（不是编码）。最高优先级的 5 条见 **5.2**；需产品/架构拍板的 3 个问题见 **5.3**；建议裁定顺序见 **5.4**；开工门槛见 **5.5**。

### 0.3 编号与档位约定

- **C-xx** = 内部矛盾（Contradiction）：文档 A 处与 B 处对同一对象给出不同说法。每条包含 A 原文、B 原文、冲突判定、开发卡点、建议裁定、决策人。
- **P-xx** = 不可实现伪代码（Pseudo-code）：按字面写 Rust 会编译失败或语义不可实现的原因。
- **M-xx** = 缺失规格（Missing）：文档引用但未定义的对象。
- **S-xx** = 语义边界未定义（Semantics）：结构存在但行为分支未定义，不同实现会给出不同行为。
- **档位**：
  - **阻塞**＝不裁定无法完成核心实现，或裁定错误会导致核心结构返工；**开工前必须关闭**。
  - **需裁定**＝需产品/架构/文档 Owner 给结论，可在实现期分批关闭；不关闭则验收标准与测试无法固定。
  - **可自行决策**＝实现者按惯例处理，不需要跨角色决策。
- 每条都标注**决策人**（产品 / 架构 / 文档 Owner / 实现者 / API 负责人），可直接汇总成 decision log。

### 0.4 各章完备度快评（主观评估，用于排期参考）

| 章节 | 可直接实现程度 | 主要缺口 |
|---|---|---|
| 1. Graph DSL 完整 Schema | ~70% | 示例自身非法（C-15）；多源同端口规则缺失（C-05）；节点无输出端口（M-16）；子图映射配置缺失（C-17） |
| 2. 运行时数据流设计 | ~55% | 结构字段较全，但 Delivery 缺字段（P-04）、EdgeRef 不可构造（P-03）、实例寻址缺失（M-13/S-02）、buffer 双写（C-23）、trace 可靠性承诺不可实现（C-24） |
| 3. Agent Harness | ~25% | 5 个依赖契约缺失（M-01–M-05）；伪代码不可编译（P-05–P-09）；Agent 输出形状冲突（C-19）；上下文预算无总预算来源（S-15） |
| 4. Scheduler 事件循环 | ~20% | 循环借用冲突与并发模型冲突（P-01/P-02）；缺超时事件类型（M-18）；终止判定三处不一致（C-07/C-10/S-06）；暂停/取消未定义（S-05） |
| 5. 节点回调接口 | ~55% | trait 与 ctx 基本可用，但 NeedHumanInput 语义矛盾（C-12）、emit 不可编译（P-05）、NodeContext 无构造器（P-12）、错误映射未定义（M-17） |
| 6. 扩展点与符号解析 | ~50% | 解析链路清楚，但命名规范三套并存（C-22）、热更新承诺与全局 Registry 矛盾（C-21）、skills 符号来源错误（C-20）、签名校验按文档实现恒失败（P-10） |
| 外围（API/WS、前端、SQLite/Checkpoint、Python 运行时） | <10% | 仅在架构图中出现（M-06–M-10），没有可实现的契约 |

### 0.5 阅读路径建议

- **只有 10 分钟**：看 0.1、0.2、5.2。
- **要做开工/评审决策**：0.2 → 5.3（3 个拍板问题）→ 5.4（裁定顺序）→ 5.5（准入条件）。
- **要开始写代码（Loader）**：第 1 章 + C-04/C-05/C-15/C-25 + M-16。
- **要开始写 Scheduler**：第 2/4 章 + P-01/P-02/P-03/P-04 + M-12/M-13/M-18 + S-01/S-02/S-06/S-09/S-10。
- **要开始写 Harness**：第 3/5 章 + M-01–M-05 + P-05/P-06/P-09 + C-12/C-19 + M-17。

---

## 1. 内部矛盾（章节 A 说法 vs 章节 B 说法）

> 说明：每条包含「A 原文（章节+行号）」「B 原文」「冲突判定」「开发卡点」「建议裁定」「决策人」。行号 `Lxxxx` 指 `/tmp/designdoc/doc.txt`（原 HTML 的纯文本抽取副本，1:1 对应段落）。
> 档位定义见文首 0.3。**C = Contradiction**。

| 编号 | 标题 | 主要位置 | 档位 |
|---|---|---|---|
| C-01 | 节点实例 timeout 30s 与单次 LLM 调用 timeout 60s 冲突 | 4.3 vs 3.4 | 阻塞 |
| C-02 | 「实例只跑一次」与 HumanInput 注入后重新执行冲突 | 2.7 vs 4.2 / 3.4 | 阻塞 |
| C-03 | max_iterations 超限「返回总结+warning」与 Err→Crashed 冲突 | 3.4 vs 3.5 / 3.2 | 阻塞 |
| C-04 | EdgeIndex.incoming 单值索引与「多源指向同一端口」冲突 | 2.2 vs 2.7 | 阻塞 |
| C-05 | 校验规则 7「两节点间一条边」与不变式「两条边指向同一 port 被拦截」不一致 | 1.3 vs 2.7 | 阻塞 |
| C-06 | gate 的 ctx：RunContext 还是 NodeContext | 6.5/6.3 vs 6.1/3.6 | 阻塞 |
| C-07 | watchdog 只扫 Running，Pending 卡死不可见且不驱动 run 终态 | 4.3 vs 2.7/4.2 | 阻塞 |
| C-08 | trigger=any 的 required 缺失为 null，违反「buffer 先于执行」 | 4.3 vs 2.7/5.3 | 阻塞 |
| C-09 | 节点失败「由 gate 决定后续」在 gate 只在 publish 求值时不可能 | 4.2 vs 6.1/2.4.4 | 阻塞 |
| C-10 | run 结束判定「所有节点 Completed」与「遍历全部终态」不一致 | 4.2 vs 2.6/2.7 | 阻塞 |
| C-11 | Cancel 产生 Cancelled，但 RunStatus 无 Cancelled | 4.3/4.1 vs 2.4.1/2.7 | 阻塞 |
| C-12 | NeedHumanInput → WaitingHuman 与 trait「Err → Crashed」冲突 | 3.4 vs 5.1/5.3 | 阻塞 |
| C-13 | 控制通道「独立 + 优先消费」与 RunControl 是 RuntimeEvent、单 recv 冲突 | 2.5 vs 4.1/4.2 | 需裁定 |
| C-14 | 暂停时「不触发就绪检查和调度」与 4.2 无 paused 检查冲突 | 4.3 vs 4.2 | 需裁定 |
| C-15 | 1.2 示例自身违反 1.3 规则 6（end 节点 inputs 为空却接 result） | 1.2 vs 1.3 | 需裁定 |
| C-16 | StreamingEvent 在 2.4.5 与 5.2 重复定义 | 2.4.5 vs 5.2 | 可自行决策 |
| C-17 | SubgraphConfig 缺 input/output_mapping，且「单 End」与多分支子图冲突 | 1.1 vs 1.1/1.3 | 阻塞 |
| C-18 | run_agent 取 &RunContext 与「不直接碰 RunContext」冲突 | 3.2 vs 3.6/6.1 | 需裁定 |
| C-19 | Agent 输出形状：content/stop_reason/iterations vs answer/need_human | 3.2 vs 2.6 | 需裁定 |
| C-20 | resolve_graph 收集 skills 符号，但 DSL 无 skills 字段 | 6.3 vs 1.1/3.2 | 需裁定 |
| C-21 | 「清该图符号缓存、重新 import」与「幂等复用、全局 Registry」冲突 | 6.4 vs 6.3/6.4 | 需裁定 |
| C-22 | gate 符号名三种写法不一致（短名 / gates.{name} / 全限定名） | 6.3 vs 1.2/6.3 | 需裁定 |
| C-23 | 「publish 返回时投递已落 buffer」与 4.2 先 publish 后写、且写两次冲突 | 2.5/2.6 vs 4.2 | 需裁定 |
| C-24 | 「trace 不丢」与同一通道 streaming 可丢弃冲突 | 2.7 vs 2.4.1/5.2 | 需裁定 |
| C-25 | GraphMetadata.created_at 必填 vs 示例缺失、规则 2 不校验 | 1.1 vs 1.2/1.3 | 需裁定 |
| C-26 | 自环合法（4.3）与「无环检测（自环不算）」及无循环上限并存 | 4.3 vs 1.3 | 需裁定 |

### C-01 节点实例 timeout 30s 与单次 LLM 调用 timeout 60s 冲突
- **档位**：阻塞
- **A｜4.3**（L1072–1073）：「每个节点实例有 timeout（默认 30s，可配置）。超时后状态迁移到 TimedOut，不等待异步任务返回。任务结果回来时丢弃。」
- **B｜3.4**（L854–857）：「单次 LLM 调用超时（默认 60s）｜重试一次；仍超时则终止｜Err(Timeout)」。
- **冲突**：agent_call 节点的外层实例预算（30s）小于其内层单次 LLM 预算（60s），且内层还允许「重试一次」（最坏 120s）。外层必然先触发 TimedOut，3.4 的超时分支与「重试一次」永远不可达。
- **开发卡点**：超时由谁计时、谁取消、重试是否复用同一实例，全部无法编码；Scheduler 实现 30s 则 Harness 的 60s/重试成死代码，Harness 实现则 Scheduler 的实例超时成死代码。
- **建议裁定**：显式定义「预算层级」：`node.timeout`（默认 30s，DSL 可配）必须 ≥ `llm.timeout × (retries + 1)`，并在 Loader 增加校验；或废除实例级 timeout，只保留 Harness 内部预算 + watchdog 兜底。默认值需产品确认（30s 内 LLM 未返回即失败是否符合体验预期）。
- **决策人**：架构负责人（Scheduler/Harness 责任边界与默认值）；产品确认默认值。

### C-02 「实例只跑一次」与 HumanInput 注入后重新执行冲突
- **档位**：阻塞
- **A｜2.7**（L609–614）：「实例只跑一次｜每个 instance_id 的 run() 只调用一次；自环产生新 instance_id」，违反时「重复执行 → 状态机拒绝 Running→Running 迁移」；同节「状态迁移只前进不后退（Pending→Ready→Running→终态；WaitingHuman 只能回到 Running）」。
- **B｜4.2**（L1051–1057）HumanInput 分支：写入 `"human_input"` 端口 → `transition(run, instance_id, "Running", "human_input 注入")` → 注释「……异步执行（同 InputArrived）」；**3.4**（L849–852）：「节点转 WaitingHuman，等待注入后继续」。
- **冲突**：WaitingHuman→Running 之后必然再次调用同一 instance_id 的 `run()`，直接违反「每个 instance_id 的 run() 只调用一次」。状态机允许的迁移与不变式禁止的行为互相矛盾。
- **开发卡点**：无法确定人工介入后是「重跑整个 run()」还是「续跑 Harness 的 LLM 循环」。这决定：NodeHandler 是否需要第二入口（如 `resume(state, human_input, ctx)`）、buffer 中历史 inputs 是否保留、memory 是否重复 append、trace 如何表达"第二次执行"。
- **建议裁定**：把 human-in-the-loop 定义为「同一实例的多阶段执行」：NodeHandler 增加可选 `resume`；InstanceRecord 增加 `phase`；2.7 不变式改为「每个 phase 的 run() 至多一次」。若产品要求"注入即重跑"，则明确定义为新 instance_id（与自环一致），并同步改 4.2。
- **决策人**：架构（状态机/接口）；产品（注入的是"补充信息"还是"新输入"）。

### C-03 max_iterations 超限「返回总结+warning」与 Err→Crashed 冲突
- **档位**：阻塞
- **A｜3.4**（L829–833）同一行内两种互斥动作：「最大迭代｜iteration >= max_iterations（默认 10）｜**终止循环，返回当前上下文总结 + warning**｜**Err(MaxIterationsExceeded) → 图节点转 Crashed**」。
- **B｜3.5**（L873）：「max_iterations 超限｜不可恢复｜无｜Crashed（带当前上下文快照）｜error」；**3.2**（L691–695）实际伪代码在循环顶部 `return Err(HarnessError::MaxIterationsExceeded { iter: iteration });`——没有"总结"，也没有 warning。
- **冲突**：`Ok(summary)` 与 `Err(Crashed)` 是两种不兼容的终态；同一张表内同时给出，且与 3.2/3.5 不一致。
- **开发卡点**：调用方（Scheduler）无法决定下游是否收到输出、run 是否 Failed、trace 是否含部分内容；如果按 3.4 前半句实现，则 3.5 的错误矩阵与 2.7「run 终态唯一」都要改。
- **建议裁定**：写死一种。推荐 `Err` + Crashed（与 3.2/3.5 一致），并在错误结构中携带 `partial_output` / `context_snapshot`，由上层（gate/编排）决定如何使用；删除 3.4 的「返回总结 + warning」。若产品坚持"超迭代也要产出"，则定义 `Ok` + `warnings` 且 run 不 Failed——但那与"死循环必须暴露"的冷峻原则冲突。
- **决策人**：产品 + 架构。

### C-04 EdgeIndex.incoming 单值索引与「多源指向同一端口」冲突
- **档位**：阻塞
- **A｜2.2**（L359–361）：「/// (to_node_id, to_input_port) → 入边（用于校验 input 来源）  pub incoming: HashMap<(NodeId, String), EdgeRef>」——一个端口只保存一条入边。
- **B｜2.7**（L605–607）不变式只约束「一条边……至多投递一份」，并未禁止多个源指向同一端口；1.3 规则 7 也只拦「from → to_node 对重复」（L262），即 A→C.x 与 B→C.x 在 DSL 层是合法的。
- **冲突**：若 DSL 允许两源同端口（当前规则允许），索引会静默覆盖一条入边——编译期"required input 有来源"校验和运行期反查都会少一条；若不允许，则缺一条校验规则（见 C-05）。
- **开发卡点**：`incoming` 的类型直接决定 Loader 校验能力与 PubSub 的反查能力。现在无法判断这是「刻意的单源约束」还是「数据结构 bug」，因此 DSL 合法性无法定义、测试无法编写。
- **建议裁定**：二选一并写死。(a) 禁止多源：规则 7 扩展为「(to_node, to_input_port) 唯一」，错误码 `DSL_PORT_MULTI_SOURCE`；索引保持单值。(b) 允许 fan-in：索引改 `Vec<EdgeRef>`，并定义合并语义（后到覆盖 / reducer 函数 / 报错）。推荐 (a)，把 fan-in 留到 v2 并在 1.3 注明。
- **决策人**：产品（是否需要多源合并）+ 架构。

### C-05 校验规则 7 与不变式「两条边指向同一 port 被拦截」不一致
- **档位**：阻塞
- **A｜1.3 规则 7**（L262）：「两节点间一条边｜DSL_DUPLICATE_EDGE｜from → to_node 对重复」——只拦同源同目标。
- **B｜2.7**（L607）：「两条边指向同一 port → 编译期已拦截（DSL_DUPLICATE_EDGE）」——描述的是同目标同端口（不同 from 也会被拦），且复用同一错误码。
- **冲突**：规则 7 的实现拦不住「A→C.x、B→C.x」；2.7 声称已拦。并且 `DSL_DUPLICATE_EDGE` 同时被用来表示"边重复"与"端口多源"两种不同错误，前端无法给出准确提示。
- **开发卡点**：1.3 自称「Loader 执行顺序」的**完整**校验清单，与运行期不变式对不上，实现者无法确定哪边是权威；同一条边既"应通过"又"应报错"，测试用例无法固定。
- **建议裁定**：以不变式为准，规则 7 拆成两条：7a「(from, to_node) 唯一」→ `DSL_DUPLICATE_EDGE`；7b「(to_node, to_input_port) 唯一」→ `DSL_PORT_MULTI_SOURCE`。同步修订 2.7 的错误码引用。
- **决策人**：文档 Owner + 架构。

### C-06 gate 的 ctx：RunContext 还是 NodeContext
- **档位**：阻塞
- **A｜6.5**（L1400）：「/// 门控：fn(output: Value, ctx: &RunContext) -> bool」；**6.3**（L1352–1354）运行期示例 `let f = &resolver.gate_registry[gate_fn]; let passed = f.call(output, ctx);`——ctx 来源未定义。
- **B｜6.1**（L1239）：「门控纯函数 gate(output, ctx) -> bool」；**6.1**（L1245）：「受限 ctx——用户回调只拿到 NodeContext 只读视图，碰不到 RunContext 内部，物理上无法破坏框架状态」；**3.6**（L915）：「通过 NodeContext 透传，不直接碰 RunContext」。
- **冲突**：gate 是用户注册的 Python 代码。给它 `&RunContext` 直接推翻 6.1 的隔离承诺（gate 可读整个 run 的实例表、buffer、global_vars、user_id），也与「纯函数」定位冲突。此外 RunContext 在 Scheduler 内是可变借用的，gate 求值期间无法再可变借用（见 P-01）。
- **开发卡点**：这是安全/隔离模型的架构决定；两种签名对应两套 ctx 构造、生命周期与借用结构。选错会导致 4.2 的整个事件循环结构返工。
- **建议裁定**：统一为 `fn(output: Value, ctx: &NodeContext) -> bool`（发布者实例的只读视图）。若产品确实需要 gate 读全局变量，通过 `ctx.get_var()` 暴露白名单键。6.5 的 `SignatureKind::Gate` 注释同步修改。
- **决策人**：架构。

### C-07 watchdog 只扫 Running，Pending 卡死不可见且不驱动 run 终态
- **档位**：阻塞
- **A｜4.3**（L1087–1088）：「Watchdog 检测卡死｜后台定时任务扫描：Running 状态超过 timeout × 2 的实例，标记为 TimedOut，记录 warning 日志。不自动恢复，只告警。」
- **B｜2.7**（L625–627）：「run 终态唯一｜run 最终必为 Completed / Failed 之一，且 RunFinished 事件只发一次」；**4.2**（L1044–1049）NodeFailed 分支：「不自动停止 run——由图设计者通过 gate 决定后续……如果所有路径都断了，run 自然卡住，靠 watchdog 发现」；**4.3**（L1081–1082）：暂停时「事件照常写入 buffer，但不触发就绪检查和调度」。
- **冲突**：(a) gate 拦截/上游 Crashed 会让下游永久停在 Pending，watchdog 只扫 Running → 永远发现不了；(b) WaitingHuman 没有超时策略，注入永远不来就永久挂起；(c) watchdog 只"告警"不迁移 run，谁来把 run 置 Failed 未定义 → run 可以永远停在 Running，与「run 终态唯一」直接矛盾。
- **开发卡点**：框架需要"无进展检测"才能保证 run 必定终结。现在 watchdog 的输入（只扫 Running、无 started_at 的 Pending）、输出（只日志）都不足以实现该保证，验收用例「卡死场景 run 必失败」无法通过。
- **建议裁定**：升级为 run 级活性检测：维护 `last_progress_at`（任何 buffer 写入/迁移/事件即刷新），超过阈值（默认 = max(node.timeout)×N，可配）→ run Failed（reason=Stalled），活跃实例标 TimedOut/Cancelled，发 `RunFinished{Failed, reason=Stalled}` 与 trace 告警；Pending 实例也纳入（用 created_at 起算）。阈值与误杀风险由产品确认。
- **决策人**：架构 + 产品。

### C-08 trigger=any 时 required 缺失为 null，违反「buffer 先于执行」
- **档位**：阻塞
- **A｜4.3**（L1078–1079）：「trigger = "any" 时，任意一个 required input 到达即触发。未到的 input port 在节点回调中为 null，由节点自行处理。」
- **B｜2.7**（L617–619）：「buffer 先于执行｜节点执行前，所有 required input 必已在 buffer 中｜is_ready() 返回 true 但 buffer 缺数据 → 断言失败」；**5.3**（L1182）：「未到的 optional port 为 None」（把 null 语义限定给 optional，而非 required）。
- **冲突**：any 模式明确允许 required 输入缺失并以 null 执行，与「所有 required input 必已在 buffer」互斥；5.3 又只说 optional 为 None。三处对同一端口语义给出三种说法。
- **开发卡点**：`is_ready()` 的实现取决于此（required 全到 vs 任一 required 到）；inputs 构建时是否插入 null；Python 侧 `inputs["x"]` 是否会 KeyError 也无法确定（5.3 示例直接 `inputs["user_query"]`）。
- **建议裁定**：定义两套就绪谓词并改写不变式：`all` → required 全到；`any` → 至少一个 required 到，其余 required 在 inputs 中显式注入 null 并记 trace。不变式改为「就绪谓词满足后，谓词所要求的端口必在 buffer」。5.3 改为「未到的端口（含 any 模式下未到的 required）为 None」。
- **决策人**：产品（any 是否真要"缺参执行"）+ 架构。

### C-09 节点失败后「由图设计者通过 gate 决定后续」在机制上不可能
- **档位**：阻塞
- **A｜4.2**（L1044–1049）：NodeFailed → `transition(..., "Crashed", ...)`；注释「不自动停止 run——由图设计者通过 gate 决定后续」。
- **B｜6.1**（L1238）：门控「求值时机（output 发布后由发布者拉起）」；**2.4.4**（L484–488）PublishResult 只由 publish 产生；**4.2**（L1018–1031）gate 只在 NodeCompleted → publish 路径上求值。
- **冲突**：Crashed 节点没有 output，不会 publish，其出边上的 gate 永远不会被求值 →「由 gate 决定后续」在机制上不存在。错误也不是数据（没有 error 端口/Delivery 没有 error 字段），下游拿不到任何信号。
- **开发卡点**：失败传播语义缺失导致 NodeFailed 分支无法实现：下游是永久 Pending、被级联失败，还是走另一条边？这决定 DSL 是否需要 `on_error` 边，以及 run 的终止判定（C-10/C-07）。
- **建议裁定**：二选一写死。(a) 冷峻模式：任一节点 Crashed → 立即 run Failed（与 3.5「不可恢复错误直接 Crashed」一致，可验证、实现简单）；(b) 失败路由：DSL 增加 `on_error` 边或 Delivery 增加 error 载荷，gate 可读错误。推荐 (a) 作为 v1，(b) 列 v2 扩展点。
- **决策人**：产品 + 架构。

### C-10 run 结束判定「所有节点 Completed」与「遍历全部终态」不一致
- **档位**：阻塞
- **A｜4.2**（L1038）：「// 5. 检查 run 是否结束（所有节点都 Completed）」。
- **B｜2.6**（L595–597）：「run 结束判定｜end_success 实例 Completed → 遍历实例表全部终态 → RunFinished」；**2.7**（L625）终态只有 Completed / Failed。
- **冲突**：因 gate 拦截而永不执行的下游节点、失败节点（Crashed）、any 模式下未触发的分支，都不可能是 Completed → 按 4.2 判定 run 永不结束；按 2.6 判定则「Crashed 算不算终态」「Pending 算不算终态」都没有定义。两处还暗示不同的触发者（end 节点 vs 全实例）。
- **开发卡点**：run 生命周期无法闭环（RunFinished 何时发、status 填什么、end 节点是否为唯一终止条件），叠加 C-07 后 run 可能永远不结束，前端永远转圈。
- **建议裁定**：统一为实例表驱动的判定：`is_run_finished = 不存在活跃实例（Pending/Ready/Running/WaitingHuman）`；并给出「哪些 Pending 视为已终止」的规则（入边全部被 gate 拦截且无其它来源 → Skipped）。若采用 C-09(a)，Crashed 直接使 run Failed，判定更简单。
- **决策人**：架构。

### C-11 Cancel 产生 Cancelled，但 RunStatus 无 Cancelled
- **档位**：阻塞
- **A｜4.3**（L1062）：「取消：标记所有 Running 实例为 Cancelled，停止调度」；**4.1**（L983–986）`RunControlAction = Pause | Resume | Cancel`。
- **B｜2.4.1**（L422）：`pub status: RunStatus, // Running | Paused | Completed | Failed`；**2.7**（L625–627）：「run 最终必为 Completed / Failed 之一，且 RunFinished 事件只发一次」；**2.4.5**（L517）`RunFinished { status: RunStatus, ... }`。
- **冲突**：取消后 run 的状态无处可填（Cancelled 不是 RunStatus 变体）。若填 Failed，则「用户主动取消」与「框架失败」无法区分（前端/统计/告警需要区分）；实例侧的 Cancelled 同样不在任何状态枚举中（见 M-12）。
- **开发卡点**：控制通道实现到一半就会撞上枚举不完整；RunFinished 的 status 语义直接决定前端展示与 SLA 统计口径。
- **建议裁定**：RunStatus 增加 `Cancelled`，终态集合改为 `Completed / Failed / Cancelled`，RunFinished 增加 `reason`；明确「取消」是用户意图、不计入失败率。
- **决策人**：产品 + 架构。

### C-12 NeedHumanInput → WaitingHuman 与 trait「Err → Crashed」冲突
- **档位**：阻塞
- **A｜3.4**（L849–852）：「人工请求｜LLM 请求 human_input｜节点转 WaitingHuman，等待注入后继续｜Err(NeedHumanInput)（由 Scheduler 处理）」；**3.2**（L745–748）`return Err(HarnessError::NeedHumanInput { prompt, schema })`。
- **B｜5.1**（L1096–1112）trait 文档：「Err(e) - 节点执行失败，状态迁移到 Crashed」；**5.3**（L1191–1192）：「Raises: NodeError: 业务错误时抛出，节点状态变为 Crashed」；5.1 的 `NodeError` 里虽有 `NeedHumanInput` 变体（L1124），但文档未给它任何特殊处理。
- **冲突**：同为 `Err` 返回，NeedHumanInput 要转 WaitingHuman、其它要转 Crashed，但没有任何异常映射表（HarnessError → NodeError → InstanceStatus，见 M-17）。Python 侧更没有"抛出后别 Crashed"的说明。
- **开发卡点**：`execute_node` 的错误分支无法实现；Python 节点要实现"请求人工"没有可用 API；4.2 NodeFailed 分支会把人工请求当失败处理。
- **建议裁定**：把"挂起"从"错误"中拆出：`enum NodeOutcome { Done(Value), WaitHuman { prompt, schema } }`，或至少给 NodeError 单列分派表；5.1 文档改为「Err 除 NeedHumanInput 外 → Crashed」。同步定义 Python 侧 `raise HumanInputRequired(...)`。
- **决策人**：架构。

### C-13 控制通道「独立 + 优先消费」与 RunControl 是 RuntimeEvent、单 recv 无优先级冲突
- **档位**：需裁定
- **A｜2.5**（L520–547）：「三条独立通道……互不干扰」；③ 控制通道：生产者「前端 / API 层」、消费者「Scheduler」、介质「mpsc channel」、特性「优先消费；暂停时不触发新调度」。
- **B｜4.1**（L982–987）：`RuntimeEvent::RunControl { run_id, action }` 是 RuntimeEvent 的一个变体；**4.2**（L991–992）主循环 `while let Some(event) = self.event_receiver.recv().await`——只有一个接收端，没有 `select!`、没有优先级队列。
- **冲突**：若控制命令与数据事件同队列，「最高优先级」无法保证（用户取消要排在所有 InputArrived 之后）；若独立队列，4.1/4.2 的伪代码里没有接收它的地方。
- **开发卡点**：取消/暂停的实际时延不可预测（影响 S-05 验收）；「暂停时不触发新调度」的检查位置也没定（4.2 无 paused 检查，见 C-14）。
- **建议裁定**：双 mpsc + `tokio::select!` 优先处理控制分支（并明确"每轮先排空控制队列"）；把 RunControl 从 RuntimeEvent 移出，或在 4.2 明确注释"控制事件由 select 分支处理"。
- **决策人**：架构。

### C-14 暂停时「不触发就绪检查和调度」与 4.2 无 paused 检查冲突
- **档位**：需裁定
- **A｜4.3**（L1081–1082）：「Run 暂停时收到新事件｜事件照常写入 buffer，但不触发就绪检查和调度。恢复时统一扫描所有 buffer，把就绪的节点调度起来。」
- **B｜4.2**（L994–1015）InputArrived 分支无条件执行 `if run.buffers.is_ready(instance_id) { transition Ready; transition Running; tokio::spawn(...) }`；NodeCompleted 分支（L1018–1041）同样没有 paused 检查，会继续 publish 扇出。
- **冲突**：4.2 的实现会在暂停期间继续创建并执行实例，与 4.3 相反；「恢复时统一扫描」在 4.2 中没有对应分支（且扫描对象是实例还是节点也未定义）。
- **开发卡点**：暂停语义不可实现：要么暂停不生效，要么恢复逻辑缺失。另外"已在跑的继续跑完"是否允许其输出继续扇出（触发下游 Pending→Ready）必须明确，否则"暂停"的边界模糊。
- **建议裁定**：统一为：暂停只冻结「就绪检查与新调度」，不冻结 buffer 写入、不中断在飞任务、不允许其输出触发下游调度（输出照常 publish 进 buffer，但不做 ready check）；恢复 = 对所有实例重跑一次就绪检查。在 4.2 增加 paused 判断与 `resume_scan()`。
- **决策人**：产品 + 架构。

### C-15 1.2 示例自身违反 1.3 规则 6（end 节点 inputs 为空却接收 result 端口）
- **档位**：需裁定
- **A｜1.2**（L223–229）：`{ "id": "end_success", "type": "end", "inputs": [], "trigger": "all", "config": {} }`；**1.2**（L242）：`{ "from": "answer_generator", "to_node": "end_success", "to_input_port": "result" }`。
- **B｜1.3 规则 6**（L260）：「to_input_port 在目标节点声明中｜DSL_INVALID_PORT｜目标节点没有这个 input port」。
- **冲突**：文档唯一给出的完整示例无法通过自己的校验规则；end 节点是否豁免端口校验、end 节点是否需要 inputs 来接收 run 输出，都没有写。
- **开发卡点**：Loader 的第一个端到端用例（示例图）就会失败；无法判断应改示例还是给 end/start 豁免，且 end 节点的输出语义（S-17）也被这条堵住。
- **建议裁定**：修订示例（end_success 增加 `inputs: [{ "name": "result", "type": "any", "required": true }]`），并在 1.3 明确 end/start 的端口约束（end 是否必须声明 inputs）。
- **决策人**：文档 Owner（示例修订）。

### C-16 StreamingEvent 在 2.4.5 与 5.2 重复定义
- **档位**：可自行决策
- **A｜2.4.5**（L503–509）：`pub enum StreamingEvent { TokenDelta, ToolCallStart, ToolCallEnd, Progress }`。
- **B｜5.2**（L1161–1171）：再次 `pub enum StreamingEvent`，变体相同。
- **冲突**：同一 crate 内重复定义（E0428），照抄即编译失败；两处没有主从/差异说明。
- **开发卡点**：照文档抄会编译失败；更关键的是掩盖了"3.2 使用了不存在的 HumanRequest 变体"这一实质缺失（见 P-06）。
- **建议裁定**：保留 5.2 为权威定义，2.4.5 改为引用；若人工介入需要流式提示，补 `HumanRequest` 变体。
- **决策人**：实现者（文档 Owner 顺手修订）。

### C-17 SubgraphConfig 缺 input/output_mapping，且「单 End」与多分支子图冲突
- **档位**：阻塞
- **A｜1.1**（L137–139）：`interface SubgraphConfig { subgraph_id: string; }`——只有 ID，没有映射字段；**1.1**（L155–161）：`SubgraphDefinition { input_mapping: Record; output_mapping: Record }`（L159–160），`output_mapping` 注释「内部 End 节点输出 → 子图外部输出」；**2.2**（L351–352）：`subgraphs: HashMap<SubgraphId, Box<CompiledGraph>>`。
- **B｜1.3 规则 9**（L266）：「子图闭合｜DSL_SUBGRAPH_INVALID｜子图无 End 节点或有多个 End」。
- **冲突**：(a) 父图→子图的输入映射无处声明（SubgraphConfig 没有 mapping 字段），子图节点的 inputs 端口与子图内部节点如何对应未定义；(b) 子图要求"只能有一个 End"，但 output_mapping 的语义又依赖 End 输出——一旦有失败/异常收口（两个 end）就不合法；(c) 子图节点的输出（作为父图出边的 from）没有定义来源。
- **开发卡点**：2.1 承诺 Loader 做「子图展开」，但展开所需的映射配置不在 DSL 里，无法实现；`parent_instance`（2.4.2）也就没有生成规则。
- **建议裁定**：SubgraphConfig 增加 `input_mapping/output_mapping`（或允许覆盖 SubgraphDefinition 的映射）；规则 9 改为「至少一个 End，且 output_mapping 的源必须是 End 的已声明端口」；定义子图节点 output = 按 output_mapping 聚合的 map。若 v1 不做子图，从 DSL 与 2.1 中移除并标注 v2。
- **决策人**：架构 + 产品。

### C-18 run_agent 取 &RunContext 与「不直接碰 RunContext」冲突
- **档位**：需裁定
- **A｜3.2**（L673–678）：`pub async fn run_agent(&self, agent_id: &AgentId, inputs: HashMap<String, Value>, ctx: &RunContext) -> Result<Value, HarnessError>`；函数内 L685 `assemble_context(agent, inputs, ctx)`、L704 `stream_tokens(ctx, &response)`、L709 `ctx.emit(...)`、L737 `memory.append_turn(ctx, ...)`。
- **B｜3.6**（L912–915）：「Scheduler｜ctx.emit(StreamingEvent) / ctx.get_var()｜通过 NodeContext 透传，不直接碰 RunContext」；**6.1**（L1245）：「用户回调只拿到 NodeContext 只读视图，碰不到 RunContext 内部」。Harness 虽是框架内部模块，但 3.6 把它列为 Scheduler 的对端契约。
- **冲突**：Harness 的入参类型与 3.6 的隔离描述不一致；且 3.2 调用了 RunContext 上不存在的方法（P-09）。
- **开发卡点**：接口签名决定模块依赖方向（Harness 是否依赖 Scheduler 内部结构）、能否脱离 Scheduler 单测 Harness。
- **建议裁定**：`run_agent(ctx: &NodeContext)`，或新增只读 `HarnessCtx { node_ctx, agent_runtime, run_vars }`；3.6 与 3.2 同步。
- **决策人**：架构。

### C-19 Agent 输出形状：content/stop_reason/iterations vs answer/need_human
- **档位**：需裁定
- **A｜3.2**（L735–742）：`return Ok(json!({ "content": content, "stop_reason": stop_reason, "iterations": iteration }))`。
- **B｜2.6**（L591–593）：「output = {"answer": "...", "need_human": false} → 重复步骤 1-6，扇出到 human_escalation_check 与 end_success」；1.2 示例的 gate `gate_need_human`（L240）与 end 节点都依赖这个形状。
- **冲突**：第 2 章走查假设 agent 节点输出是业务字段（answer/need_human），第 3 章 Harness 固定返回 LLM 元数据（content/stop_reason/iterations）。下游 gate 与节点读不到契约字段。
- **开发卡点**：agent_call 节点的输出 schema 决定 DSL 类型检查、gate 可读字段、示例与集成测试能否跑通；两章答案不同，无法写测试。
- **建议裁定**：定义 agent 节点输出契约：固定含 `content`，并允许 AgentConfig 声明 `output_schema`（或 DSL 边上 `output_mapping`）把 LLM 文本抽取为业务字段；2.6 示例改为 `output_mapping` 后的结果。若 v1 不做映射，则明确 agent 节点输出仅 `{content, stop_reason, iterations}` 并修改第 2 章所有示例。
- **决策人**：产品 + 架构。

### C-20 resolve_graph 收集 skills 符号，但 DSL 无 skills 字段
- **档位**：需裁定
- **A｜6.3**（L1320–1332）：`resolve_graph` 注释「收集符号：所有 gate 引用 + python_node 的 module.function + skills」，并对 `SymbolKind::Skill(name)` 调 `resolve_skill`。
- **B｜1.1**（L106–139）：GraphNode / AgentCallConfig / PythonNodeConfig / SubgraphConfig 均无 skills 字段；skills 位于 Agent 资产（3.2 `agent.skills`，3.6「Skill 库 resolve(names)」），而 AgentConfig 未定义（M-01）。
- **冲突**：`collect_symbols(graph)` 的输入是 CompiledGraph，图中不存在 skill 符号 → 该分支永远为空；反之 skill 解析发生在 agent 执行期（懒加载），与 6.3「图加载时解析全部符号」冲突。
- **开发卡点**：SymbolResolver 的输入/输出边界（图级 vs Agent 级）无法确定；`DSL_SYMBOL_NOT_FOUND` 对 skill 是否生效也不清楚（影响 M-11）。
- **建议裁定**：符号解析分两级——图级（gate/python node）在 Loader 解析并校验；Agent 级（skills/tools）在 Harness Loading 阶段解析，错误归 HarnessError。6.3 删除 skills 分支或注明「运行期解析」。
- **决策人**：架构。

### C-21 「清该图符号缓存、重新 import」与「幂等复用、全局 Registry」冲突
- **档位**：需裁定
- **A｜6.4**（L1364–1366）：「改 Python 代码｜重新加载图 → 清该图符号缓存 → 重新 import｜不隐式 reload——旧 run 仍在跑时不强制替换实现」。
- **B｜6.3**（L1336–1345）：`resolve_gate` 内「幂等：已注册直接复用」；**6.4**（L1372–1374）：「两个图共用符号｜Registry 幂等复用，先注册者生效」；（L1392）「符号实体留在 Registry（全局共享，图 B 可能仍引用），真正清除在重载图时清理缓存」。
- **冲突**：(a) SymbolResolver 只有 resolve_* 方法，没有 evict/clear；(b)「该图的符号缓存」不存在——registry 按符号名全局索引，无法知道符号属于哪张图；(c) 若重载时清全局符号，「图 B 可能仍引用」的承诺就被破坏。
- **开发卡点**：热更新路径不存在（要么无法 reload，要么误伤其它图）；「旧 run 用旧实现」需要绑定版本，而 RegisteredFn 没有版本字段。
- **建议裁定**：明确 v1 范围——Python 代码热替换不支持（重载需重启），只支持换图/换 Agent 配置；若要支持，引入 per-graph registry（6.6 远期路线）+ Arc 引用计数 + 符号版本号，并把 6.4 改为设计目标而非承诺。
- **决策人**：架构 + 产品。

### C-22 gate 符号名三种写法不一致
- **档位**：需裁定
- **A｜6.3**（L1341）：`python_runtime::import_function(&format!("gates.{name}"))?`——假定 name 是短名并强加 `gates.` 前缀；**6.3**（L1288）示例符号是 `"gates.hr.need_human"`（全限定）；**1.2**（L240）DSL 里写 `"function": "gate_need_human"`（无前缀）。
- **B｜1.1**（L132–135）PythonNodeConfig 用 `module: "gates.hr"` + `function: "need_human_check"` 两字段——与 gate 的单字符串字段设计不一致。
- **冲突**：三种写法并存：短名（gate_need_human）、全限定名（gates.hr.need_human）、module+function 二元组。按 6.3 的拼接逻辑，全限定名会被拼成 `gates.gates.hr.need_human`，短名会被拼成 `gates.gate_need_human`（与示例模块路径不符）。
- **开发卡点**：无法实现 import 与 DSL 重名/冲突检测（`DSL_GATE_CONFLICT` 依赖规范名）；用户写错时无法给出准确错误。
- **建议裁定**：统一为「全限定符号名」：DSL 写 `gates.hr.need_human`，代码直接 import 不再拼前缀；python_node 也收敛为同一形式（或保留 module/function 但文档固定，不做双轨）。
- **决策人**：文档 Owner + 架构。

### C-23 「publish 返回时投递已落 buffer」与 4.2 先 publish 后写、且写两次冲突
- **档位**：需裁定
- **A｜2.5**（L524–531）：「① 数据通道｜节点 payload｜节点执行器｜Input Buffer → 下游节点｜内存 HashMap（RunContext 内）｜同步写、同步读；单数据源覆盖语义」；结尾（L547）：「数据通道是同步的（publish 返回时投递已落 buffer），保证图语义确定性」。**2.6** 步骤 5–6（L571–577）：先「写入下游 buffer」，再「触发 InputArrived 事件」。
- **B｜4.2**（L1025–1031）：`let publish = self.pubsub.publish(run, instance_id, output);` 之后才 `run.buffers.write(&d.to_instance_id, &d.to_input_port, d.data);`，然后 `self.event_sender.send(RuntimeEvent::InputArrived { ... }).await`；而 InputArrived 分支（L998）又 `run.buffers.write(instance_id, &port, data);` 一次。
- **冲突**：publish 返回时并未落 buffer（与 2.5 相反）；同一份数据写两次（4.2 直接写 + InputArrived 再写），"单写者"不成立；arrived_required 被重复累加（虽幂等，但事件数与 trace 会翻倍）。
- **开发卡点**：「谁写 buffer」是调度器实现的第一性问题：决定 publish 是否纯函数、InputArrived 事件是否必要、事件重放如何幂等。
- **建议裁定**：收敛为单一写者：publish 只计算投递（纯函数，返回 Vec<Delivery>），事件循环在 InputArrived 中统一写 buffer；或 publish 内写 buffer 且事件只做通知（不重复写）。同步修订 2.5/2.6。
- **决策人**：架构。

### C-24 「trace 不丢」与同一通道的 streaming 可丢弃冲突
- **档位**：需裁定
- **A｜2.7**（L621–623）：「trace 不丢｜每次状态迁移必有一条 TransitionRecord；streaming 事件尽力而为（channel 满可丢弃）」；违反时「迁移未落库 → watchdog 检查 transition 计数」。
- **B｜2.4.1**（L427）`event_sender: mpsc::Sender<TraceEvent>`、**5.2**（L1137）NodeContext 同样是 `mpsc::Sender<TraceEvent>`；Streaming 与 Transition 共用一条有界 channel，满时不区分类型。
- **冲突**：「迁移不丢」在有界 channel 上无法保证；「迁移未落库 → 检查 transition 计数」没有定义计数来源、阈值与判定逻辑（watchdog 只扫 Running，见 C-07）。
- **开发卡点**：可靠性承诺无法实现，Checkpoint/审计的验收标准写不出来；通道满时的降级策略（阻塞/丢弃/换通道）需架构决定。
- **建议裁定**：拆分通道或分级：trace/transition 走可靠通道（背压或写前不丢），streaming 走尽力而为通道；或在同通道对 Transition 用阻塞发送（背压），并明确"事件循环可被背压阻塞"的代价。watchdog 的计数指标给出定义（写入 vs 落库）。
- **决策人**：架构 + 产品。

### C-25 GraphMetadata.created_at 必填 vs 示例缺失、规则 2 不校验
- **档位**：需裁定
- **A｜1.1**（L96–101）：`interface GraphMetadata { name: string; description?: string; author?: string; created_at: string; }`——created_at 非可选。
- **B｜1.2 示例**（L166–171）metadata 只有 name/description/author，没有 created_at；**1.3 规则 2**（L252）：「必填字段存在｜DSL_MISSING_FIELD｜version / entry_points / nodes / edges 缺失」——清单不含 metadata 嵌套字段。
- **冲突**：示例按 serde 反序列化会因缺必填字段失败，而规则 2 的清单不覆盖它；错误最终以 `DSL_PARSE_ERROR` 还是 `DSL_MISSING_FIELD` 出现不确定。
- **开发卡点**：Loader 的错误码与必填清单不一致，测试无法断言错误码；创建时间由编辑器还是服务端填也未定（影响图资产版本管理）。
- **建议裁定**：规则 2 明确「含嵌套必填字段（metadata.name / metadata.created_at 等）」；created_at 建议改为可选、由服务端在首次保存时填充，示例补齐。
- **决策人**：文档 Owner + 架构。

### C-26 自环合法（4.3）与「无环检测（自环不算）」及无循环上限并存
- **档位**：需裁定
- **A｜4.3**（L1075–1076）：「自环边触发新实例｜节点 output 发布到自己的 input port 时，创建新的 instance_id（不是复用旧实例）。旧实例保持 Completed，新实例从 Pending 开始。」——自环是受支持的运行期行为。
- **B｜1.3 规则 10**（L268）：「无环检测（可选）｜DSL_CYCLE_DETECTED｜检测到无 gate 的纯数据环（自环边不算）」；全文没有任何运行期循环次数上限（`max_iterations` 只约束 Harness 内部 LLM 循环，见 3.4）。
- **冲突**：自环被当作合法特性，但没有任何机制限制它产生的实例数量；「纯数据环」的定义（自环为什么不算、有 gate 的环是否允许无限）也缺失；`attempt` 字段（2.4.2）的语义未定（S-03/S-08）。
- **开发卡点**：无法评估资源耗尽风险；是否需要在 DSL 增加 `max_loops`/`max_attempts`、超限后如何处理无法决定；规则 10 与 4.3 的组合使"环"的合法性判断不可实现。
- **建议裁定**：明确 v1 策略：回边/自环必须带 gate，且每个实例有 `max_attempts`（默认值由产品定，超限 → 实例 Failed，run 按 C-09 策略终结）；规则 10 改为「允许自环，但回边必须声明上限」。
- **决策人**：产品（回环次数）+ 架构。

## 2. 不可实现 / 不可编译的伪代码

> 判定标准：把文档中的伪代码**按字面**写成 Rust 会发生什么。只列"按当前文档无法实现"的，不列风格问题。
> **P = Pseudo-code**。

| 编号 | 问题 | 位置 | 档位 |
|---|---|---|---|
| P-01 | 4.2 事件循环借用冲突：&mut self.runs 与 &mut self 方法、await 期间持借用 | 4.2 | 阻塞 |
| P-02 | tokio::spawn 的 'static + Send 要求无法满足，clone_ref 未定义 | 4.2 | 阻塞 |
| P-03 | EdgeRef 的 &'static CompiledEdge 无法构造（自引用/生命周期） | 2.2 | 阻塞 |
| P-04 | Delivery 缺 to_instance_id，4.2 却直接使用 | 2.4.4 vs 4.2 | 阻塞 |
| P-05 | NodeContext::emit 缺 run_id、同步 fn 调 async send | 5.2 / 2.4.5 | 阻塞 |
| P-06 | 使用不存在的 StreamingEvent::HumanRequest；LlmResponse/HarnessError 未定义 | 3.2 | 阻塞 |
| P-07 | 3.2 的 Rust 语法非法（命名参数调用）+ 未定义变量 skills_as_tool_schemas | 3.2 | 可自行决策 |
| P-08 | 3.2 的 use-after-move（tool_name 多次被移动） | 3.2 | 可自行决策 |
| P-09 | run_agent 拿 &RunContext 却调 ctx.emit；RunContext 无此方法、无法跨 spawn | 3.2 | 阻塞 |
| P-10 | 6.5 签名校验：match 非穷尽 + return_type 与注解对象比较恒不等 | 6.5 | 需裁定 |
| P-11 | 6.3 运行时调用：PyO3 需要 GIL token，registry 借用/索引不成立 | 6.3 | 需裁定 |
| P-12 | NodeContext 字段全私有、无构造器，Scheduler 无法创建 | 5.2 | 可自行决策 |

### P-01 4.2 事件循环的借用冲突
- **档位**：阻塞
- **原文 A｜4.2**（L995–1031）：
  - `let run = self.runs.get_mut(&run_id).unwrap();`（`&mut self` 借用）
  - `self.transition(run, instance_id, "Ready", "inputs 到齐");` / `self.transition(run, instance_id, "Running", "调度执行");`
  - `let publish = self.pubsub.publish(run, instance_id, output);`
  - 循环内 `self.event_sender.send(RuntimeEvent::InputArrived { ... }).await;`，此时 `run`（来自 `self.runs`）仍然存活。
- **Rust 语义**：`run` 持有对 `self.runs` 的可变借用；`self.transition(...)`、`self.pubsub.publish(...)`、`self.event_sender.send(...)` 若通过 `&self`/`&mut self` 访问 Scheduler，则与存活的 `run` 构成 E0499/E0502 冲突。若 `run` 是 `&mut RunContext`，`.await` 跨越该借用还要求 Future 可 Send（或 Scheduler 只能跑在 current_thread runtime，与 4.2 的 `tokio::spawn` 矛盾）。
- **结论**：不能按当前伪代码实现。必须选定所有权模型：(a) `transition`/`publish` 改为 `RunContext` 的方法（只借 run），事件通过预先克隆的本地 sender 变量发送；(b) 事件循环每轮"取事件 → 取出所需数据 → drop 借用 → 处理 → 重新借用"三段式；(c) `Arc<Mutex<RunContext>>`（牺牲"单线程"承诺，且 4.3 的"天然串行"不再成立）。
- **开发卡点**：这是 Scheduler 的第一行代码，模型定了才能写其余全部逻辑；现在无法开工。
- **建议裁定**：采用 (a)+(b)：RunContext 自带 transition/publish 方法（`impl RunContext`），Scheduler 只持 `Sender`/`PubSub` 的克隆；事件循环每轮避免长期持有 `self` 借用。
- **决策人**：架构负责人。

### P-02 tokio::spawn 的 'static + Send 要求与 clone_ref 未定义
- **档位**：阻塞
- **原文｜4.2**（L1006–1014）：
  ```
  let run_clone = run.clone_ref();
  tokio::spawn(async move {
      let result = execute_node(&run_clone, instance_id).await;
      match result {
          Ok(output) => send_event(RuntimeEvent::NodeCompleted { ... }),
          Err(e) => send_event(RuntimeEvent::NodeFailed { ... }),
      }
  });
  ```
- **Rust 语义**：`tokio::spawn` 要求 `Future + Send + 'static`。`run` 是 `&mut RunContext`（来自 `self.runs`）；`clone_ref()` 未定义；即使定义为返回 `Arc<RunContext>`，`execute_node(&run_clone, ...)` 需要在 `'static` 任务中拥有或 `'static` 借用数据，而 `send_event` 捕获的 sender 也必须满足 Send + 'static。`RunContext` 内含 `HashMap<InstanceId, InstanceRecord>`、`Sender` 等；按值移动会丢失共享（Task 看不到主循环后续变化），按 `Arc` 共享则需要整体内部可变性（与单线程模型冲突）。
- **结论**：4.2「不阻塞事件循环」的并发模型与「RunContext 单线程独占」不可同时成立，伪代码不可实现。
- **建议裁定**：确定"实例执行只依赖输入快照 + NodeContext（含 event sender + 只读 global_vars 快照）"，执行结果通过 `RuntimeEvent` 回传，RunContext 不跨线程共享。这样 `execute_node` 的签名要从 `&RunContext` 改为 `(inputs, ctx)`，与 P-09 一并解决。
- **决策人**：架构。

### P-03 EdgeRef 的 `&'static CompiledEdge` 无法构造
- **档位**：阻塞
- **原文｜2.2**（L341–367）：
  ```
  pub struct CompiledGraph { pub edges: Vec<CompiledEdge>, ... }
  pub struct EdgeIndex { pub outgoing: HashMap<NodeId, Vec<EdgeRef>>,
                         pub incoming: HashMap<(NodeId, String), EdgeRef> }
  pub struct EdgeRef { pub index: usize, pub edge: &'static CompiledEdge }
  ```
- **Rust 语义**：`&'static CompiledEdge` 要求引用存活到进程结束。边存在 `CompiledGraph::edges`（普通 Vec，随图回收），无法给出 `'static` 引用；只能 `Box::leak`（内存泄漏，且与 6.4「换图/清缓存」冲突）或 `Arc<CompiledEdge>`（类型就应是 `Arc`，而非 `&'static`）。此外 `EdgeIndex` 与 `CompiledGraph` 属于同一编译产物，索引引用自身字段元素 = 自引用结构，Rust 无法安全构造（移动后引用失效）。
- **结论**：结构定义需返工；`index: usize` 本身已足够（注释也写「指向 edges 向量的索引，避免复制」）。
- **建议裁定**：`EdgeRef { index: usize }`（或仅 `u32`），运行期 `&graph.edges[r.index]`；若需为 subgraph 复用索引，`EdgeIndex` 按图独立构建并由 `Arc<CompiledGraph>` 持有。
- **决策人**：架构。

### P-04 Delivery 缺 to_instance_id，4.2 却直接使用
- **档位**：阻塞
- **原文 A｜2.4.4**（L472–482）Delivery 字段完备清单：`run_id / from_instance / from_node / to_node / to_input_port / data / gate_fn / gate_passed / delivered_at`——**没有 `to_instance_id`**。
- **原文 B｜4.2**（L1029）：`run.buffers.write(&d.to_instance_id, &d.to_input_port, d.data);`
- **结论**：编译错误 E0609（no field `to_instance_id`）。更重要的是语义缺口：投递目标是"节点"，buffer 却按"实例"索引（2.4.1 `buffers: HashMap<InstanceId, InstanceBuffer>`），投递时的实例选择没有任何规则（S-02）。
- **建议裁定**：Delivery 增加 `to_instance_id`；在 publish 前由"实例解析器"决定目标实例（创建或复用），并把解析规则写进 2.4/4.2。
- **决策人**：架构。

### P-05 NodeContext::emit 缺 run_id、同步 fn 调 async send
- **档位**：阻塞
- **原文 A｜5.2**（L1140–1148）：
  ```
  pub fn emit(&self, event: StreamingEvent) {
      self.event_sender.send(TraceEvent::Streaming {
          instance_id: self.instance_id.clone(),
          event,
      });
  }
  ```
- **原文 B｜2.4.5**（L512–518）：`TraceEvent::Streaming { run_id: RunId, instance_id: InstanceId, event: StreamingEvent }`。
- **编译问题**：(1) 构造缺 `run_id` → E0063 missing field；(2) `tokio::mpsc::Sender::send` 是 `async fn` 且返回 `Result<(), SendError>`，同步 `fn emit` 中不能 `.await`（E0728）；(3) 若改用 `try_send`，满通道的错误被丢弃 → `#[must_use]` 警告（`deny(warnings)` 即失败），且必须明确"满了之后丢事件还是记日志"。
- **语义问题**：2.7 说 streaming「channel 满可丢弃」，但 5.2 的代码没有丢弃分支，也没有"记录丢弃数"的指标。
- **建议裁定**：`emit` 改为 `fn emit(&self, event: StreamingEvent)` 内部 `let _ = self.event_sender.try_send(...)` + 计数；或 `async fn emit` 并在节点执行路径 `await`（背压）。二者对应 C-24 的可靠性决定。
- **决策人**：架构。

### P-06 使用不存在的 StreamingEvent::HumanRequest；LlmResponse/HarnessError 未定义
- **档位**：阻塞
- **原文 A｜3.2**（L745–749）：
  ```
  LlmResponse::HumanRequest { prompt, schema } => {
      ctx.emit(StreamingEvent::HumanRequest { prompt, schema });
      return Err(HarnessError::NeedHumanInput { prompt, schema });
  }
  ```
- **原文 B｜2.4.5 / 5.2** 的 `StreamingEvent` 只有 4 个变体：`TokenDelta / ToolCallStart / ToolCallEnd / Progress`——没有 `HumanRequest`。全文也没有 `LlmResponse`、`HarnessError`、`Message`、`ToolDef` 的定义（3.6 只写「统一返回 Text | ToolCall | HumanRequest 三态」）。
- **结论**：伪代码不可编译；三态枚举缺字段定义（tool_call 的 id、args 类型；Text 的 stop_reason 取值；HumanRequest 的 schema 结构），tool 消息回填协议（对比 OpenAI/Anthropic 的 tool_call_id）也无法确定。
- **建议裁定**：给 LLM 抽象层一份字段级定义（`LlmResponse`、`Message`、`ToolCall`、`StopReason`、`LlmError`），并在 StreamingEvent 补 `HumanRequest`（若 v1 支持人工介入）。
- **决策人**：架构。

### P-07 3.2 的 Rust 语法非法（命名参数调用）+ 未定义变量
- **档位**：可自行决策
- **原文｜3.2**（L697–701）：
  ```
  let response = self.llm_provider.chat(
      model: agent.llm.model,
      messages: &messages,
      tools: &skills_as_tool_schemas,
  ).await?;
  ```
- **Rust 语义**：Rust 没有命名参数（这是非法语法）；`skills_as_tool_schemas` 全文未定义（前文只有 `skills`，L682）；`agent.llm.model` 依赖未定义的 AgentConfig（M-01）。
- **结论**：不可编译，但修正方式唯一（改为位置参数或 `ChatRequest` 结构体）——属实现者自决；真正的问题是第 3 章伪代码未经编译检查，其余错误（P-06/P-08/P-09）更严重。
- **建议裁定**：改为 `chat(ChatRequest { model, messages, tools }).await`，并定义 ChatRequest。
- **决策人**：实现者。

### P-08 3.2 的 use-after-move
- **档位**：可自行决策
- **原文｜3.2**（L708–731）：`LlmResponse::ToolCall { tool_name, args }` 绑定后：
  - L709 `ctx.emit(StreamingEvent::ToolCallStart { tool_name, args });`（移动 `tool_name`）
  - L711 `let tool = skills.find(tool_name)`（再次使用 → E0382）
  - L719 `messages.push(Message::Tool { tool_name, ... });`（第三次使用）
- **结论**：至少两处 move-after-move；`args` 同理。修正方式（clone / 引用 / 先取 id）是机械的，但同时暴露：`ToolCall` 是否需要 `id`、`args` 是否为 `serde_json::Value`、`Message::Tool` 的字段集合——这些都依赖 M-04 的接口定义。
- **建议裁定**：改为 `&str`/`Arc<str>` 或显式 clone；`Message::Tool` 增加 `tool_call_id`。
- **决策人**：实现者（依赖 M-04）。

### P-09 run_agent 拿 &RunContext 却调 ctx.emit
- **档位**：阻塞
- **原文｜3.2**（L673–678）：`ctx: &RunContext`；函数内 L685 `self.assemble_context(agent, inputs, ctx)`、L704 `self.stream_tokens(ctx, &response)`、L709 `ctx.emit(...)`、L737 `self.memory.append_turn(ctx, agent_id, Assistant, &content)`。
- **Rust 语义/缺失**：2.4.1 的 `RunContext` 没有 `emit` 方法（emit 在 NodeContext 上，5.2）；`stream_tokens`、`assemble_context`、`memory`、`llm_provider`、`registry`、`skill_lib` 均未定义；`&RunContext` 来自 Scheduler 的 `&mut` 借用时不能跨 `await` 传入 `spawn`（与 P-02 联动）。
- **结论**：Harness 的接口签名必须重定；否则 3、4 章无法对接，也无法单测 Harness。
- **建议裁定**：`run_agent(&self, agent_id, inputs, ctx: &NodeContext, runtime: &AgentRuntime) -> Result<Value, HarnessError>`；`NodeContext` 提供 `emit/get_var/user_id`；`AgentRuntime` 聚合 registry/skill_lib/memory/llm（依赖注入，利于 mock）。
- **决策人**：架构。

### P-10 6.5 签名校验：match 非穷尽 + return_type 比较恒不等
- **档位**：需裁定
- **原文 A｜6.5**（L1399–1425）：
  ```
  pub enum SignatureKind { Gate, Node, Skill }
  match kind {
      SignatureKind::Gate => { if sig.param_count != 2 || sig.return_type != "bool" { ... } }
      SignatureKind::Node => { if sig.param_count != 2 || sig.return_type != "dict" { ... } }
      // ...
  }
  ```
- **问题**：(1) `SignatureKind` 有三个变体，match 只有两个分支且没有 `_` → E0004 non-exhaustive；(2) `sig.return_type != "bool"`：Python `inspect.signature` 返回的 `return_annotation` 是类型对象（`bool` / `dict`）或字符串（仅在写了字符串注解或 `from __future__ import annotations` 时），与 `&str` 比较在类型对象情形下不能通过（编译期更可能直接类型不匹配）→ **所有合法 Python 函数都会被判签名错误**；(3) `Node` 分支要求 `-> dict`，但 5.3 示例返回 dict 注解可以满足，而异步函数/多返回值/`typing.Dict` 都不支持；(4) Python 侧"必须写类型注解"这一硬约束在 5.3 未写成规则。
- **建议裁定**：改为"参数个数 + 可调用探测"（先 `inspect.signature` 校验 arity，再可选注解校验），或规范化注解字符串后比较；match 补 `Skill` 与 `_`。是否强依赖注解由架构决定（影响 5.3 示例与用户文档）。
- **决策人**：架构。

### P-11 6.3 运行时调用：PyO3 需要 GIL token，registry 借用/索引不成立
- **档位**：需裁定
- **原文｜6.3**（L1352–1354）：
  ```
  let f = &resolver.gate_registry[gate_fn];
  let passed = f.call(output, ctx);   // Rust → PyO3 → Python 函数
  ```
- **问题**：(1) PyO3 调用必须持有 `Python<'py>` token（通常 `Python::with_gil(|py| f.call1(py, args))`），`RegisteredFn::call` 的签名未定义；(2) `HashMap<String, RegisteredFn>` 用 `gate_fn` 索引，`gate_fn` 的类型未定义（`&str` 可借，但注释里是 `Option<String>`，见 2.4.4）；(3) `&resolver` 的不可变借用与 6.3 同一结构里 `resolve_gate(&mut self)` 的注册动作可能冲突（若调用与解析在同一作用域）；(4) `ctx` 类型未定义（C-06）。
- **建议裁定**：定义 `RegisteredFn { name, signature, call(&self, py: Python, args: &[Value]) -> PyResult<Value> }` 与 `GateEvaluator` 的调用点（GIL 获取位置、是否 `allow_threads`）；gate 求值在事件循环线程还是阻塞线程池上执行也要定（影响 M-19 与超时）。
- **决策人**：架构。

### P-12 NodeContext 字段全私有、无构造器
- **档位**：可自行决策
- **原文｜5.2**（L1131–1159）：`pub struct NodeContext { run_id, instance_id, node_id, user_id, global_vars, event_sender }` 字段全私有，`impl` 只有 `emit/get_var/user_id`，没有 `new`/`From`/Clone。
- **问题**：其他模块无法构造（4.2 的 `execute_node` 需要为每次执行创建 ctx）；`get_var` 返回 `Option<&Value>` 要求 `global_vars` 是快照（每次执行克隆成本/一致性未讨论）；4.2 的 `run.clone_ref()` 若用于构造 ctx 也会受影响。
- **建议裁定**：补 `pub(crate) fn new(...)`；明确 global_vars 是"执行开始时的只读快照"（避免节点看到并发修改）。
- **决策人**：实现者。

## 3. 缺失规格（文档引用但未定义）

> 判定标准：文档中**出现了名字/契约引用**，但没有给出足以实现或评审的定义。**M = Missing**。
> 重要提示：本章 20 条中 13 条为阻塞级——第 3 章（Harness）依赖的外部模块几乎全部未定义，这是"能否完成开发"的最大不确定性来源。

| 编号 | 缺失内容 | 文档引用位置 | 档位 |
|---|---|---|---|
| M-01 | AgentConfig 全字段 | 3.2 / 3.3 / 3.6 / 6.2 | 阻塞 |
| M-02 | MODULE-09 Memory Manager | 3.2 / 3.3 / 3.6 | 阻塞 |
| M-03 | Skill / ToolDef / ToolExecutor 契约 | 3.2 / 3.6 / 6.5 | 阻塞 |
| M-04 | LLM Provider 抽象与流式接口 | 3.2 / 3.7 / 3.5 / 3.6 | 阻塞 |
| M-05 | KB 适配器与上下文组装关系 | 3.3 / 3.6 | 阻塞 |
| M-06 | Checkpoint schema 与恢复语义 | 2.1 / 2.5 / 4.2 | 阻塞 |
| M-07 | SQLite 表结构与迁移 | 2.1 / 2.4.5 | 需裁定 |
| M-08 | API / WebSocket 层 | 2.1 / 2.5 / 4.1 | 需裁定 |
| M-09 | 前端编辑器与 Trace 视图 | 1.0 / 2.1 / 2.7 / 3.7 | 需裁定 |
| M-10 | Python 运行时环境管理 | 5.3 / 6.3 / 6.4 / 6.6 | 阻塞 |
| M-11 | 错误码全集与分层 | 1.3 / 6.5 / 3.5 / 5.1 | 需裁定 |
| M-12 | InstanceStatus 枚举与合法迁移表 | 2.4.2 / 2.7 / 4.3 | 阻塞 |
| M-13 | 实例生命周期与"节点→实例"寻址 | 2.4.2 / 2.6 / 4.2 / 4.3 | 阻塞 |
| M-14 | 子图运行期语义 | 2.1 / 2.2 / 1.1 | 阻塞 |
| M-15 | Gate 契约补全（params/异常/顺序） | 1.1 / 6.1 / 2.4.3 | 阻塞 |
| M-16 | 输出端口与字段级路由 | 1.1 / 5.3 / 2.6 | 阻塞 |
| M-17 | HarnessError → NodeError → InstanceStatus 映射 | 3.2 / 3.4 / 5.1 | 需裁定 |
| M-18 | 超时/取消/看门狗机制与 DSL 配置字段 | 4.3 / 4.1 | 阻塞 |
| M-19 | 并发/运行时/线程模型 | 4.0 / 4.2 / 6.6 | 需裁定 |
| M-20 | 版本兼容与 graph_hash | 1.1 / 2.4.5 | 可自行决策 |

### M-01 AgentConfig 全字段缺失
- **档位**：阻塞
- **引用点**：**3.2**（L681）`self.registry.load(agent_id)`；`agent.skills`（L682）；`agent.llm.model`（L698）；`agent.constraints.max_iterations`（L693）；**3.3**（L759-771）`role_prompt_file`、`constraints_file`、kb 检索、`max_tokens`；**3.6**（L887-890）「load(agent_id) → AgentConfig｜返回完整配置（含文件路径），不内联文件内容」；**6.2**（L1269-1271）「Agent 配置（AgentConfig JSON）｜懒加载：执行 agent_call 时才 load」。
- **缺失内容**：全文没有 AgentConfig 定义——无字段表、无示例 JSON、无版本与校验规则、无缺省值。以下字段只是"被用到"：`llm.{provider, model, temperature?, max_tokens?, timeout?}`、`skills[]`、`constraints.{max_iterations, max_tokens, ...}`、`role_prompt_file`、`constraints_file`、`kb_ids`（推测）、`memory` 配置、`output_schema`（推测）、`agent_id`/`name`/`version`。
- **开发卡点**：Harness 的 Loading（加载配置）与 Assembling（3.3 组装）两个 Phase 无法编码；Agent 资产的生产者（前端/运营）无法产出合法配置；`max_iterations` 默认 10（3.4）是「Agent 级默认」还是「全局默认」说不清；Agent 资产缺失（6.5 `DSL_AGENT_NOT_FOUND`）的检查点（懒加载前）没有定义。
- **建议裁定**：先冻结 AgentConfig v1 schema（字段 + 类型 + 默认值 + 校验），给出 1 个示例 JSON，并把"文件路径 vs 内联内容"的加载规则写清（3.6 已说只存路径，但角色 prompt 的渲染/变量替换没有定义）。
- **决策人**：产品（能力项）+ 架构（schema）。

### M-02 MODULE-09 Memory Manager 未定义
- **档位**：阻塞
- **引用点**：**3.6**（L902-905）「Memory Manager｜recall(...) / append_turn(...) / write_long_term(...)｜读 + 写｜见模块设计 MODULE-09；检索失败降级」；**3.3**（L774-779）`MemoryBlock { short_term: 最近 N 轮（默认 20）, long_term: 向量检索 top-k（默认 5） }`；**3.3** 表（L812-814）「长期记忆｜向量库检索｜system 消息｜500（top-5）」「短期记忆｜run 内历史｜消息序列中间｜3000（最近 20 轮）」；**3.2**（L737）`self.memory.append_turn(ctx, agent_id, Assistant, &content)`。
- **缺失内容**：MODULE-09 不在本文档中；`recall`/`append_turn`/`write_long_term` 的参数与返回、存储介质（向量库？SQLite？内存？）、作用域（run / user / agent / conversation）、摘要与归档触发条件、"检索失败降级为空"的降级点、写入失败降级（3.5 有"记忆写入失败｜可恢复｜降级跳过"）、隐私与清理策略——全部缺失。
- **开发卡点**：3.3 的组装逻辑（② 短期记忆）与 3.2 的记忆写入无法实现；降级点无法定位；3.2 只 append Assistant 轮、没有 append User 轮，导致"最近 20 轮对话历史"永远缺用户侧（S-14），即按文档实现出的记忆是不自洽的。
- **建议裁定**：v1 把 Memory 定义为可插拔接口 + 最小实现（run 级短期记忆，内存，不落向量库），接口签名冻结；MODULE-09 作为依赖项明确"谁在什么时间提供"，若是外部模块，则需提供契约与 stub。
- **决策人**：架构 + 产品。

### M-03 Skill / ToolDef / ToolExecutor 契约缺失
- **档位**：阻塞
- **引用点**：**3.2**（L682）`self.skill_lib.resolve(&agent.skills)?`；（L711-714）`skills.find(tool_name)`、`self.tool_executor.execute(&tool, args).await`；**3.6**（L897-900）「Skill 库｜resolve(names) → Vec<ToolDef>；execute(tool, args) → Value｜ToolDef 带 JSON Schema，供 LLM 工具定义」；**6.5**（L1404）`SignatureKind::Skill: fn(args: Value) -> Result<Value, ToolError>`；**3.4**（L839-842）工具失败"错误喂回 LLM（不作为失败终止）"。
- **缺失内容**：ToolDef 字段（name/description/parameters JSON Schema/timeout/是否幂等/是否并行安全/权限）、ToolExecutor 的错误类型 `ToolError`、注册方式（Python 装饰器？配置文件？）、tool 调用的超时与取消、多个 tool 并发执行策略、工具结果的序列化与截断（大结果如何进上下文）、工具调用的安全边界（能否读写外部系统）、`UnknownTool` 的产生时机（resolve 阶段还是 find 阶段，对应 C-20）。
- **开发卡点**：tool_call 循环（Harness 的核心）无法实现；3.5 要求 `ToolCallEnd { error }` 的错误格式未定义；`6.5 Skill 签名（args → Value）` 与 `3.6 execute(tool, args)` 的调用层次不一致（谁持有 tool 上下文）。
- **建议裁定**：给出 ToolDef JSON Schema + `trait ToolExecutor { async fn execute(&self, tool: &ToolDef, args: Value, ctx: &ToolCtx) -> Result<Value, ToolError> }` + 超时/并发/截断规则；注册 API 与 Python 侧写法示例。若 v1 只做内置工具，明确写"不支持用户自定义 Skill"。
- **决策人**：架构 + 产品。

### M-04 LLM Provider 抽象与流式接口缺失
- **档位**：阻塞
- **引用点**：**3.6**（L892-895）「LLM Provider｜chat(model, messages, tools) → LlmResponse｜统一返回 Text | ToolCall | HumanRequest 三态」；**3.7**（L917-944）「LLM stream → Harness → ctx.emit → TokenDelta；逐 token 聚合；批量场景：TokenDelta 按 100ms 窗口聚合推送」；**3.5**（L865-867）网络错误指数退避 ×3（1s/2s/4s）、超时重试 ×1；**3.4**（L844-847）上下文溢出处理。
- **缺失内容**：`LlmResponse` / `Message` / `ToolCall` / `StopReason` / `LlmError` 的字段级定义；流式 API 形态（`chat_stream` 返回 Stream？回调？channel？）——**3.2 的主循环是"先 await 完整响应、再 stream_tokens"，与 3.7 的逐 token 透传矛盾（伪代码里根本没有流式调用点）**；provider 配置（base_url / api_key / 超时 / 并发 / 配额 / 代理）；错误分类（哪些可恢复、哪些要重试）；token 用量统计与计费；上下文窗口大小（供 3.3 预算，S-15）；多 provider 路由；response 的 tool_calls 多值问题（S-16）。
- **开发卡点**：第 3 章无法落地：没有可调用的 LLM 接口，也没有可回填的消息协议；3.7 的整条链路没有入口。
- **建议裁定**：定义 `trait LlmProvider`（`chat` 全量 + `chat_stream` 流式两个方法，或只保留流式 + 聚合），给出 `Message` 枚举（System/User/Assistant/Tool{call_id}）与 `LlmResponse` 三态字段；明确 3.2 主循环改用流式接口；错误分类表与重试策略集中定义。
- **决策人**：架构。

### M-05 KB 适配器与上下文组装关系缺失
- **档位**：阻塞
- **引用点**：**3.3**（L764-765）`kb_docs: Vec<KbDocument>`「知识库文档：kb 检索 top-k」；**3.3** 表（L810）「知识库文档｜kb 检索｜system 消息｜1500（top-3）」；**3.6**（L907-910）「知识库适配器｜retrieve(kb_id, query, top_k) → Vec<Document>｜文档带 rendered 字段（纯文本）」。
- **缺失内容**：谁触发 retrieve（Harness 的哪个 Phase？3.1 状态机只有 Idle/Loading/Assembling/LLM/Tool/Returning/Failed，**没有检索状态**）；query 从哪来（user_inputs 的哪个字段？LLM 生成检索词？固定模板？）；kb_id 来自 AgentConfig 还是节点配置；多 KB 如何合并与去重；`rendered` 的格式与截断规则；检索超时与失败降级（3.5 只提到"记忆检索失败"，没有 KB 检索失败）；检索结果是否进 trace/前端引用溯源（citation）；KB 的增删改与权限边界。
- **开发卡点**：3.3 的"知识库参考"段无法实现；token 预算 1500（top-3）与"各段默认预算可配置"（L818）的关系不明；Agent 使用者无法指定知识库。
- **建议裁定**：定义 `trait KnowledgeAdapter { async fn retrieve(&self, kb_id, query, top_k) -> Result<Vec<KbDocument>> }` 与检索触发规则（默认取 `user_inputs.query`，可配置 `query_template`），在 3.1 状态机补 `Retrieving` 状态；明确失败降级（空结果 + warning）。
- **决策人**：产品 + 架构。

### M-06 Checkpoint 落库 schema 与恢复语义缺失
- **档位**：阻塞
- **引用点**：**2.1**（L325-326）「Checkpoint Store｜SQLite 落库｜transition / event → 前端 Trace 视图」；**2.5**（L533-538）② 事件通道「mpsc channel + SQLite｜异步、不阻塞主循环、可重放」；**2.4.5**（L492-501）TransitionRecord「id: i64 // SQLite 自增」；**2.7**（L623）「迁移未落库 → watchdog 检查 transition 计数」。
- **缺失内容**：4.2 主循环**没有任何 checkpoint 写入调用**；没有表结构、写入时机（每条迁移？批量？事务边界？）、幂等/去重键、run 恢复（resume from checkpoint）算法、在飞实例如何回滚/重放、保留期与清理、graph_hash 一致性与版本迁移、SQLite 并发（WAL？单写者？）。
- **开发卡点**："可重放/可恢复"无法实现；崩溃后 run 只能重跑；trace 前端数据源不确定；2.7 的"watchdog 检查 transition 计数"没有定义。
- **建议裁定**：明确 v1 是否需要 resume。若需要：定义 checkpoint 内容（run 元数据 + 实例状态 + buffers + gate_results + 事件偏移）与恢复算法（含在飞任务标失败/重放）；若不需要：删除"可重放"承诺，只做异步批量落库的审计 trace，并在 2.5 注明。
- **决策人**：产品 + 架构。

### M-07 SQLite 表结构与迁移缺失
- **档位**：需裁定
- **引用点**：2.1/2.4.5/2.5 均提到 SQLite、自增 id、TraceEvent 落库。
- **缺失内容**：没有任何 DDL（runs / instances / transitions / streaming_events / graphs / checkpoints）、索引、保留期/分区、批量写入参数、schema 版本迁移、多租户（user_id）过滤查询设计。
- **开发卡点**：持久化层无法开始编码；即便先做内存实现，后续接 SQLite 也需返工数据结构。
- **建议裁定**：随 M-06 一并给出 DDL v1；若 v1 不做持久化，明确"内存 + 可选 JSONL 导出"。
- **决策人**：架构。

### M-08 API / WebSocket 层缺失
- **档位**：需裁定
- **引用点**：**2.1**「前端 Trace 视图｜WebSocket 推送」；**2.5**（L541-545）③ 控制通道生产者「前端 / API 层」；**4.1** HumanInput / RunControl 来自外部；**2.4.1**（L426）`user_id: String // 当前用户`；**4.3**（L1046）RunFailed 等需要通过 API 暴露。
- **缺失内容**：run 启动 API（如何传 entry 节点的输入、指定 graph、返回 run_id）、状态查询、控制（pause/resume/cancel）、human_input 提交、WS 订阅协议（事件格式/重连/断点续传/多客户端广播/鉴权）、多租户与权限、幂等键、限流。
- **开发卡点**：端到端无法验收；前端无法并行开发（契约不存在）；`user_id` 的来源（会话？请求头？）与信任边界未定义。
- **建议裁定**：给出 REST + WS 契约（OpenAPI 片段 + 事件 JSON 示例），最小覆盖 start / get / control / human_input / subscribe；鉴权方案单列。
- **决策人**：产品 + 架构。

### M-09 前端编辑器与 Trace 视图缺失
- **档位**：需裁定
- **引用点**：**1.0**（L82）「Graph DSL 是前后端共享的唯一事实源。前端编辑器输出 JSON」；**2.1**「前端 Trace 视图」；**2.4.3** `gate_results`「用于 trace 展示"哪条边放行/拦截"」；**3.7**（L944）「按 100ms 窗口聚合推送」。
- **缺失内容**：编辑器（节点面板/连线/端口/子图/校验反馈/版本管理/发布流程）、Trace 视图（时间线/实例状态/边放行/流式输出/人工输入表单渲染/错误展示）、DSL 校验错误到 UI 的定位映射、前端状态管理。
- **开发卡点**：前端无法估时与并行开发；"唯一事实源"的校验闭环（Loader 错误码 → 编辑器定位）未定义。
- **建议裁定**：本迭代以"只读 Trace + JSON 文本编辑器 + 校验反馈"为最小范围，可视化编辑器另立文档；明确错误码 → 字段路径的映射格式。
- **决策人**：产品。

### M-10 Python 运行时环境管理缺失
- **档位**：阻塞
- **引用点**：**6.3**（L1297-1298）「Python 动态导入｜importlib + getattr」；**6.4**（L1365）改代码「重新 import」；**5.3**（L1173-1211）Python 节点约定；**6.6**（L1445-1447）「Python 函数以框架权限运行｜无沙箱」；**6.5**（L1408-1410）`inspect.signature`。
- **缺失内容**：解释器初始化（PyO3 版本、auto-initialize、是否允许 sub-interpreter）、`sys.path` 如何注入用户代码目录、依赖安装/隔离（venv？全局？）、Python 版本约束、`async def` 是否支持、异常 → NodeError 的映射与 traceback 保存、GIL 与并发（多 run / 多节点同时调 Python）、超时如何中断 Python 调用（PyO3 无法安全 kill 线程）、进程模型（内嵌解释器 vs 子进程池）、启动时预热与模块重载。
- **开发卡点**：Python 节点与 gate 无法可靠执行；**超时（C-01/S-09）在 Python 调用上不可实现**（无法取消）；多 run 并发会互相阻塞；`importlib` 的模块缓存/重载语义影响 C-21。
- **建议裁定**：定义「Python 运行时 v1」：单解释器 + 启动时 `sys.path` 注入 + 依赖预装（文档化）+ 节点函数必须同步 + 超时采用"软超时（结果丢弃）+ 进程级 watchdog"，或在 v1 就采用子进程池（可获得真超时/隔离，但增加序列化成本）。这是影响架构的选择，必须先定。
- **决策人**：架构。

### M-11 错误码全集与分层缺失
- **档位**：需裁定
- **引用点**：1.3 的 10 个 `DSL_*`；6.5 的 `DSL_SYMBOL_NOT_FOUND / DSL_GATE_SIGNATURE_MISMATCH / DSL_NODE_SIGNATURE_MISMATCH / DSL_GATE_CONFLICT / DSL_AGENT_NOT_FOUND`；5.1 的 `NodeError` 4 变体；3.2 的 `HarnessError`；2.7 的 `RuntimeError`；4.2 的 `.unwrap()` panic。
- **缺失内容**：统一编码规范（模块前缀/数字码/是否稳定 API/HTTP 映射/多语言文案键）、错误分层（DSL/运行时/节点/LLM/API）、错误聚合策略（一张图多个错误时报哪个、是否全量返回）、错误与 trace 的对应、错误码与 1.3 校验顺序的关系。
- **开发卡点**：前端无法做提示；测试无法断言；API 契约不稳定；`DSL_SYMBOL_NOT_FOUND` 发生在"加载期"但 1.3 的 10 条校验里没有符号解析步骤，顺序未定义。
- **建议裁定**：出一份错误码表（含可重试标志与用户文案键），规定 Loader 采用"聚合返回前 N 条 + 首错"策略；把符号解析作为校验规则 11（或独立的 resolve 阶段）写入 1.3。
- **决策人**：架构 + 产品。

### M-12 InstanceStatus 枚举与合法迁移表缺失
- **档位**：阻塞
- **引用点**：**2.4.2**（L439）`pub status: InstanceStatus`（无定义）；**2.7**（L609-615）提到 Pending / Ready / Running / 终态 / WaitingHuman，且"Running→Running 被拒绝"；**4.3**（L1073/L1088）提到 TimedOut；**4.3**（L1062）提到 Cancelled；**4.2**（L1003-1046）用字符串 `"Ready"` / `"Running"` / `"Completed"` / `"Crashed"` 迁移。
- **缺失内容**：枚举定义、终态集合（Completed/Crashed/TimedOut/Cancelled/Skipped 哪些是终态）、完整迁移矩阵（from × to → 允许/拒绝/副作用）、Ready 是否可跳过（pending→running 直达）、WaitingHuman 能否被取消/超时、TimedOut 之后是否可重试（新实例？）、run 级 RunStatus 与实例状态的关系。
- **开发卡点**：`transition()` 无法实现；2.7 的不变式无法验证；4.2 的字符串与枚举映射不一致（大小写/拼写不同就丢事件）。
- **建议裁定**：给出显式状态机（枚举 + 迁移表 + 终态集合），4.2 改用枚举；把 C-11（Cancelled）与 C-12（WaitingHuman）的裁定结果写进该表。
- **决策人**：架构。

### M-13 实例生命周期与「节点→实例」寻址缺失
- **档位**：阻塞
- **引用点**：**2.4.2**（L434-446）InstanceRecord（instance_id / node_id / parent_instance / attempt）；**2.4.1**（L423-424）`instances: HashMap<InstanceId, InstanceRecord>`、`buffers: HashMap<InstanceId, InstanceBuffer>`；**4.3**（L1075-1076）自环创建新 instance_id；**4.2**（L1029）需要 `d.to_instance_id`（字段缺失）；**2.6**（L572-573）「answer_generator 实例的 values[...]」（暗示一节点一实例）。
- **缺失内容**：每个节点最多几个活跃实例？同一节点能否并行多实例（并行分支汇入/多次触发）？entry 实例何时创建、由谁创建？投递时如何在多个候选实例中选择（最新？全部？按 attempt？）？实例与子图的父子关系如何维护？实例表/缓存的清理时机与内存上限？
- **开发卡点**：Delivery / InputArrived / buffer 的键设计无法确定；**4.2 的核心数据通路无法实现**（这是调度器的第一性问题，与 P-04 联动）。
- **建议裁定**：定义"实例 = (node_id, 触发轮次)"；默认每节点至多一个活跃实例，多实例仅由自环/并行入口产生；投递规则：目标节点无活跃实例则创建（Pending），有则复用；Delivery 携带 to_instance_id；entry 实例在 run start 时创建（配合 S-01）。
- **决策人**：架构。

### M-14 子图运行期语义缺失
- **档位**：阻塞
- **引用点**：**2.1**（L286）Loader「解析 / 校验 / 子图展开」；**2.2**（L351-352）`subgraphs: HashMap<SubgraphId, Box<CompiledGraph>>`「递归同构」；**1.1**（L155-161）SubgraphDefinition 的 input/output_mapping；**2.4.2**（L438）`parent_instance`「子图展开时的父实例」。
- **缺失内容**：子图何时实例化（编译期展开成普通节点，还是运行期创建子 run）、父子实例 id 关系、输入如何注入子图 entry 节点、子图 end 输出如何聚合到父图节点 output、子图内失败如何传播、子图的 pause/cancel 是否递归、是否允许递归子图（自引用）。
- **开发卡点**：子图节点无法实现；`parent_instance` 字段无生成规则；与 C-17 叠加后子图功能完全不可用。
- **建议裁定**：明确 v1 子图 = 编译期展开（flatten）或运行期独立子 run，二选一后给出实例/输入/输出/失败的完整规则；若都不做，从 DSL 与 2.1 中移除并标注 v2。
- **决策人**：产品 + 架构。

### M-15 Gate 契约补全
- **档位**：阻塞
- **引用点**：**1.1**（L149-152）`GateDefinition { function: string; params?: Record<string, any> }`；**6.1**（L1238）「求值时机（output 发布后由发布者拉起）、拦截语义（false=丢弃+日志）、签名、失败处理（panic=拦截）」；**6.5** 签名校验；**2.4.3** GateResultRecord；**2.4.4**（L478）Delivery.data「载荷（发布者 output **或 gate 转换后的值**）」。
- **缺失内容**：`params` 如何传给 gate（签名只有 output 和 ctx，没有 params 位置）？gate 抛 Python 异常的语义（拦截？节点 Crashed？只 warning）？多个出边的求值顺序？gate 能否修改 payload（**2.4.4 的"gate 转换后的值"与 `-> bool` 签名直接矛盾**）？gate 超时？gate 是否允许出现在 end 节点出边 / entry 节点入边？gate 失败是否写 GateResultRecord + trace？
- **开发卡点**：PubSubRouter 的 publish 无法实现（参数、异常、顺序、数据变换全未定）；2.4.4 的"gate 转换值"与 6.1/6.5 的 bool 签名冲突需要二选一。
- **建议裁定**：定义 `fn gate(output: Value, ctx: &NodeContext, params: &Value) -> Result<bool, GateError>`；明确 params 由 DSL 注入、gate 只读不改 payload（如需转换另设 transform 扩展点，列 v2）、异常按拦截处理并记 trace、求值顺序 = 出边定义顺序、gate 超时继承节点 timeout。
- **决策人**：架构 + 产品。

### M-16 输出端口与字段级路由缺失
- **档位**：阻塞
- **引用点**：**1.1**（L106-112）GraphNode 只有 `inputs: InputPort[]`，**没有 outputs**；**5.3**（L1206-1211）「返回输出（会被发布到出边）」「节点是纯函数——给定 inputs，返回 output」；**2.6**（L593）`output = {"answer": ..., "need_human": false}` 整个扇出；**3.3**（L768）`user_inputs`「节点 input port → Agent 输入字段（经 input_mapping）」；**1.1**（L127-130）`input_mapping?: Record<string, string>`。
- **缺失内容**：节点输出如何对应下游端口？整个 dict 发到所有出边、下游拿整个 dict？还是按边选择字段？两个下游需要不同字段时怎么办（1.2 示例中 input_parser 的同一 output 同时喂 `kb_search.query` 与 `answer_generator.user_query`，实际拿到的是整个 parser 输出）？`input_mapping` 的缺省规则（缺省 = 同名？未映射端口丢弃？多个端口映射到同一 Agent 字段？）；end 节点的输出如何表达 run 结果（1.2 甚至没有为该 end 声明 inputs）。
- **开发卡点**：这是"图模型"最核心的数据契约。按现文档只能"整包广播"，无法表达字段级路由；示例图已暴露该问题（下游节点必须自己从 dict 里取字段，而 5.3 的示例写的是 `inputs["user_query"]`，即假设端口拿到的是值本身，不是整包）。**同一份文档里两种假设并存**。
- **建议裁定**：二选一：(a) DSL 增加节点 `outputs: OutputPort[]` + 边增加 `from_output_port`（或 `output_mapping`），Delivery 携带端口名；(b) v1 坚持整包广播，则必须修改 5.3 示例（`inputs["user_query"]` → 从整包取字段）并在 1.1 写明"output 是节点级、与端口无关"，同时补 Agent 输出的业务字段映射（C-19）。
- **决策人**：产品 + 架构。

### M-17 HarnessError → NodeError → InstanceStatus 映射缺失
- **档位**：需裁定
- **引用点**：**3.2** 返回 `HarnessError`（`MaxIterationsExceeded` / `UnknownTool` / `NeedHumanInput` / 省略号）；**3.4**（L852）「Err(NeedHumanInput)（由 Scheduler 处理）」；**3.5** 表格用「Crashed」泛称；**5.1** `NodeError` 四个变体；**2.4.2** `error: Option<NodeError>`。
- **缺失内容**：没有映射表：`MaxIterationsExceeded` / `LlmError` / `UnknownTool` / 配置加载失败 / `NeedHumanInput` 分别对应 `NodeError` 哪个变体、对应 `InstanceStatus` 什么状态、trace 记 error 还是 warning、错误细节（3.5 说的"上下文快照"）放哪、是否携带 partial output。
- **开发卡点**：`execute_node` 的错误分支（4.2 NodeFailed）无法实现；`InstanceRecord.error` 无法填充；C-12 的 WaitingHuman 分派依赖此表。
- **建议裁定**：给出映射表（HarnessError 变体 → NodeError 变体 → InstanceStatus → trace 级别 → 是否携带数据），并把 NeedHumanInput 从错误路径拆出（C-12）。
- **决策人**：架构。

### M-18 超时/取消/看门狗机制与 DSL 配置字段缺失
- **档位**：阻塞
- **引用点**：**4.3**（L1073）「每个节点实例有 timeout（默认 30s，可配置）」；**4.1** 的 RuntimeEvent **没有超时事件**；**4.3**（L1088）watchdog「timeout × 2」；**3.4**（L854-857）LLM 超时与重试；**4.3**（L1062）取消。
- **缺失内容**：timeout 在 DSL 哪里配置（GraphNode 无 timeout 字段）；超时由谁计时（事件循环定时器？spawn 内 `tokio::time::timeout`？）；超时后如何取消在飞任务（tokio AbortHandle？Python 无法取消）；超时事件如何进入事件循环（**缺 `RuntimeEvent::NodeTimedOut`，且 4.1 事件枚举无法扩展地表达它**）；watchdog 扫描周期与阈值配置；取消如何中断 LLM/Python 调用；超时后的结果丢弃需要状态守卫（S-09）。
- **开发卡点**：超时/取消是可靠性基础，现在没有任何可实现机制；4.3 的"不等待异步任务返回，结果回来时丢弃"若没有状态守卫，会把 TimedOut 覆盖为 Completed。
- **建议裁定**：增加 `RuntimeEvent::NodeTimedOut`；每实例维护 `deadline: Instant`（事件循环内定时器轮或最小堆）；DSL 增加 `node.timeout`；维护 `instance → AbortHandle`；规定"结果回来时实例非 Running/WaitingHuman 则丢弃并记 warning"。
- **决策人**：架构。

### M-19 并发/运行时/线程模型缺失
- **档位**：需裁定
- **引用点**：**4.0**（L948）「Scheduler 是图内核的心脏。它是一个单线程事件循环」；**4.2** 用 `tokio::spawn` 执行节点；**4.3**（L1085）「事件循环是单线程的，天然串行处理」；**6.6** Python 以框架权限运行。
- **缺失内容**：Scheduler 是每 run 一个还是全局一个？多 run 如何隔离与公平（配额/优先级）？tokio runtime 配置（worker 数、blocking 池）？节点并发上限（一次 spawn 几千个怎么办）？Python GIL 争用与阻塞策略？事件通道背压？多租户资源隔离？
- **开发卡点**：性能与稳定性无法评估；"单线程"承诺与 spawn 的并发事实需要作者澄清（状态变更是单线程、执行是并发——应显式写进文档，否则验收时"是否线程安全"无从判断）。
- **建议裁定**：写明"单线程仅指状态机与事件处理；节点执行在 tokio 多线程 runtime 并发，Python 调用受 GIL 串行"；给出并发上限、背压策略、多 run 调度策略（全局 Scheduler + run 表，与 4.2 的 `self.runs` 一致）。
- **决策人**：架构。

### M-20 版本兼容与 graph_hash
- **档位**：可自行决策
- **引用点**：**1.1**（L88）`version: "1.0"`；文档标题「详细设计 v1.1」；**2.4.5**（L513）`RunStarted { graph_hash: String }`；2.1 Loader 无版本分支。
- **缺失内容**：DSL 版本升级策略（v1.0 → v1.1 的 schema 变化）、向后兼容规则、graph_hash 的计算方式（会影响 trace 去重与图资产版本管理）、旧版本图的加载行为。
- **开发卡点**：小；但 graph_hash 若不定义，RunStarted 的去重/回放无法实现。
- **建议裁定**：规定 `version` 语义为 `major.minor`、Loader 拒绝不支持的 major；graph_hash = 规范化 JSON 的 sha256（键排序、去除空白）。
- **决策人**：实现者。

## 4. 语义边界未定义（可实现但行为不确定）

> 判定标准：结构/接口大致存在，但**具体行为的分支没有被定义**，不同实现者会写出不同行为、且都能"自圆其说"。**S = Semantics**。
> 这类缺陷通常不阻塞框架骨架，但会阻塞验收标准与测试用例；其中 10 条会被上游矛盾条目放大为阻塞。

| 编号 | 未定义的语义边界 | 位置 | 档位 |
|---|---|---|---|
| S-01 | entry 实例创建时机与 run 启动输入注入 | 2.4.1 / 2.6 / 4.2 | 阻塞 |
| S-02 | 投递到哪个实例（同节点多实例） | 2.4.4 / 4.2 | 阻塞 |
| S-03 | 自环新实例的 attempt/父实例/上限 | 2.4.2 / 4.3 | 阻塞 |
| S-04 | trigger=any 的重复触发与 buffer 清理 | 4.3 / 2.4.3 | 阻塞 |
| S-05 | Pause/Resume/Cancel 的精确行为（含在飞 LLM 取消） | 4.2 / 4.3 / 2.5 | 阻塞 |
| S-06 | run 终止判定（end / 全终态 / gate 拦截 / 崩溃） | 2.6 / 4.2 | 阻塞 |
| S-07 | watchdog 覆盖范围与阈值来源 | 4.3 / 2.7 | 阻塞 |
| S-08 | attempt / 重试语义 | 2.4.2 / 3.4 | 需裁定 |
| S-09 | 节点超时与完成消息的竞态 | 4.3 / 4.2 | 阻塞 |
| S-10 | gate 拦截后下游的终态与归属 | 4.2 / 2.7 | 阻塞 |
| S-11 | 多 run 并发与热更新交互 | 4.2 / 6.4 | 需裁定 |
| S-12 | 子图实例化与 input/output_mapping 语义 | 1.1 / 2.4.2 | 阻塞 |
| S-13 | HumanInput 端口与恢复上下文 | 4.2 / 3.2 / 5.3 | 需裁定 |
| S-14 | 记忆作用域与用户轮次写入 | 3.2 / 3.3 | 需裁定 |
| S-15 | 上下文预算溢出与总预算来源 | 3.3 / 3.4 | 需裁定 |
| S-16 | 单次响应多 tool_call 与 iteration 计数 | 3.2 / 3.4 | 需裁定 |
| S-17 | end 节点输出与 run 最终输出 | 1.2 / 2.6 / 2.4.5 | 需裁定 |
| S-18 | DSL 热更新与符号解析时机 | 6.2 / 6.4 / 6.5 | 需裁定 |

### S-01 entry 实例创建时机与 run 启动输入注入
- **档位**：阻塞
- **文档现状**：**2.4.1**（L418-430）`RunContext` **没有 run inputs 字段**（只有 `global_vars`、`user_id`）；**2.6** 走查从「input_parser 发布 output」开始（L555-557），**没有说明输入如何进入 input_parser**；**1.1**（L90）`entry_points: string[]`；**4.2** 所有调度都由 `InputArrived` 驱动；**2.1** 没有"run 启动"步骤。
- **未定义**：run 启动时谁创建 entry 实例（Pending 还是直接 Ready）、entry 节点的 required inputs 从哪来（API 传入？global_vars？）、多个 entry_points 是否并行创建、规则 8 豁免了 entry 的入边要求（1.3 L264「非 entry 节点」），因此 entry 的输入**必须**外部注入，但注入机制完全没有。
- **影响/卡点**：第一条 `InputArrived` 事件没有来源 → 整个 run 无法启动，这是 E2E 的第一块多米诺。
- **建议裁定**：定义 `RunSpec { graph_id, entry_inputs: HashMap<NodeId, HashMap<Port, Value>>, user_id, global_vars, run_options }` 与启动流程：校验 → 为每个 entry_point 创建实例 → 写 buffer → 发 InputArrived → 进入事件循环 → 记录 RunStarted。
- **决策人**：架构 + API 负责人。

### S-02 投递到哪个实例
- **档位**：阻塞
- **文档现状**：**2.4.4** Delivery 只有 `to_node`；**4.2**（L1029）用 `d.to_instance_id`（P-04 缺失）；**4.3**（L1075-1076）自环创建新实例；**2.6**（L572-573）假设目标实例已存在（「answer_generator 实例的 values[...]」）。
- **未定义**：目标实例不存在时是否创建（谁创建、状态从哪开始、attempt 继承谁）；存在多个实例时投给谁（并行入口/split 场景）；gate 拦截后是否创建下游实例；如何保证同一节点被多个上游同时投递时只创建一个实例（事件循环串行可以保证，但需要"节点 → 当前实例"映射或"未建实例"状态）。
- **影响/卡点**：buffer 写入目标无法确定；重复实例与丢投递两类 bug 不可避免；与 M-13 是同一问题的两面。
- **建议裁定**：维护 `node_active_instance: HashMap<NodeId, InstanceId>`；Delivery 增加 `to_instance_id`，在 publish 前的实例解析步骤填充（不存在则创建 Pending）；明确"没有入边来源的节点不预创建实例"。
- **决策人**：架构。

### S-03 自环新实例的 attempt / 父实例 / 上限
- **档位**：阻塞
- **文档现状**：**2.4.2**（L440）`attempt: u32 // 尝试次数（自环/重试产生新实例时递增）`；**4.3**（L1075-1076）「创建新的 instance_id（不是复用旧实例）。旧实例保持 Completed，新实例从 Pending 开始。」
- **未定义**：新实例 `attempt = 旧 + 1`？上限多少、超限后发生什么（Failed / 丢弃 / run Failed）？`parent_instance` 指向谁（自环的父实例是旧实例还是根？）？旧实例的 buffer/output 是否复用？自环边的 gate 在哪个实例上求值？`attempt` 对普通重试是否也递增（3.4 的 LLM 重试不产生新实例，二者语义冲突）？trace 如何表达"第 N 圈"？
- **影响/卡点**：业务上很常见的"门禁不通过回炉重做"无法实现；资源上限无法约束（C-26）。
- **建议裁定**：定义 `attempt` 仅用于自环/回边轮次；`max_attempts` 进 DSL（默认值由产品定）；超限 → 实例 Failed 并按 C-09 策略处理 run；自环新实例的 parent_instance = 原实例，buffer 为空，旧实例保留 Completed。
- **决策人**：产品 + 架构。

### S-04 trigger=any 的重复触发与 buffer 清理
- **档位**：阻塞
- **文档现状**：**4.3**（L1079）「任意一个 required input 到达即触发。未到的 input port 在节点回调中为 null」；**2.4.3**（L450-456）`arrived_required: HashSet<String>` 是累积集合；**2.7**「实例只跑一次」。
- **未定义**：any 模式下第二个 required 稍后到达时：再次执行？丢弃？触发新实例？`arrived_required` 是否在就绪后清空（否则重复触发或永不再次触发）？any 模式与「实例只跑一次」如何共存？上次执行时注入的 null 是否影响后续？
- **影响/卡点**：is_ready 与调度逻辑需要明确的"消费"语义；否则同一实例可能被反复调度（违反不变式）或永远只跑一次（后续数据静默丢失，用户无法理解）。
- **建议裁定**：明确 any = "首次满足即执行一次，后续到达丢弃并记 trace（推荐）"或"每次满足都触发新实例（与自环一致）"；就绪检查后清空 buffer/arrived（若允许重跑）。与 C-08 一并裁定。
- **决策人**：产品 + 架构。

### S-05 Pause / Resume / Cancel 的精确行为
- **档位**：阻塞
- **文档现状**：**4.2**（L1060-1062）注释「暂停：停止调度新节点，已在跑的继续跑完；恢复：统一扫描所有 buffer，调度就绪节点；取消：标记所有 Running 实例为 Cancelled，停止调度」；**4.3**（L1081-1082）暂停期间事件照常写 buffer；**2.5**（L545）「暂停时不触发新调度」；**4.1** Pause/Resume/Cancel。
- **未定义**：暂停粒度（run 级 / 全局 Scheduler）：暂停期间**在飞任务完成后的 publish 是否继续**（4.2 NodeCompleted 无 paused 检查，会继续投递并触发下游）？恢复时是否立即执行所有就绪节点（可能一次涌入大量任务）？取消时在飞 LLM 调用是否中断（不中断则 run 已 Cancelled 但 token 仍在消耗）？取消后 Pending/Ready 实例的状态？Cancel 是否可恢复（与 Pause 的区别）？`human_input` 提交算控制命令还是数据事件（2.5 把 human_input 列在控制通道，4.1 却是独立事件）？
- **影响/卡点**：用户最关心的"停下来"能力无法实现/验收；状态机需要"暂停期间允许的迁移集合"。
- **建议裁定**：写出「操作 × 层级（run / instance）」矩阵：Pause 只冻结派发；Cancel 产生 Cancelled 终态并尽力中断（LLM 用 abort/select；Python 标 best-effort）；恢复后按 created_at/deadline 顺序调度；明确 human_input 走数据/控制哪条通道。
- **决策人**：产品 + 架构。

### S-06 run 终止判定
- **档位**：阻塞
- **文档现状**：**2.6**（L595-597）「end_success 实例 Completed → 遍历实例表全部终态 → RunFinished」；**4.2**（L1038）「所有节点都 Completed」；**2.7**「run 终态唯一」；4.2 NodeFailed 不停止 run；4.3 gate 拦截「下游自然等待」。
- **未定义**：end 节点是否唯一终止条件（多个 end？end 不可达？）；Crashed/Pending/Cancelled 是否算终态；被 gate 拦截且无其它来源的分支如何收敛；run 的最终 output 是什么（end 的 input？聚合？）；RunFinished 的 status 如何从实例状态推导；watchdog 与终止判定的关系（C-07）。
- **影响/卡点**：RunFinished 无法可靠发出；前端永远转圈；无法定义"成功跑完"的验收用例。
- **建议裁定**：定义终止优先级：① 所有实例终态 → run 终态（任一 Crashed 则 Failed，否则 Completed）；② 任一 end 实例完成且无活跃实例；③ watchdog stall → Failed；并规定 end 节点必须声明 inputs 以承载 run 输出、run.output 的聚合规则。
- **决策人**：架构 + 产品。

### S-07 watchdog 覆盖范围与阈值来源
- **档位**：阻塞
- **文档现状**：**4.3**（L1087-1088）「Running 状态超过 timeout × 2」；**2.7**（L623）「迁移未落库 → watchdog 检查 transition 计数」；**2.4.2** `started_at: Option<DateTime<Utc>>`。
- **未定义**：扫描对象是实例还是 run；Pending 实例没有 `started_at`，「超过 timeout」无法计算；WaitingHuman 没有超时；"transition 计数"的统计口径（写入数 vs 落库数）与阈值未定义；告警动作（日志？trace 事件？前端提示？）；是否会自动杀实例（"不自动恢复，只告警"与 run 终态唯一矛盾，C-07）。
- **影响/卡点**：watchdog 无法实现"发现卡死"的最小功能，更无法闭环 run。
- **建议裁定**：改为 run 级 `last_progress_at` + 实例级 `created_at/started_at` 双阈值；新增 `RuntimeEvent::WatchdogAlert` 或 trace 事件；阈值可配置并写入配置表。
- **决策人**：架构。

### S-08 attempt / 重试语义
- **档位**：需裁定
- **文档现状**：**2.4.2** `attempt`；**3.4/3.5** LLM 重试（指数退避 ×3、超时重试 ×1）；**4.3** 自环新实例。
- **未定义**：是否存在图级重试（节点失败后自动重试）？`attempt` 与 LLM 重试的关系（不递增？）；重试是否创建新实例（与自环一致？）；重试的幂等性要求（节点可能有副作用：发消息、写库）；重试耗尽后的 run 行为。
- **影响/卡点**：v1 若不做图级重试，需明确写出，否则实现者可能各自添加；`attempt` 字段语义不清会导致 trace 统计错误。
- **建议裁定**：v1 明确"图级不自动重试，失败即失败（按 C-09 策略），只有 LLM 内部重试且不改变实例状态/attempt"；`attempt` 仅用于自环轮次。
- **决策人**：产品 + 架构。

### S-09 节点超时与完成消息的竞态
- **档位**：阻塞
- **文档现状**：**4.3**（L1073）「超时后状态迁移到 TimedOut，不等待异步任务返回。任务结果回来时丢弃。」；**4.2**（L1018-1022）NodeCompleted 分支无条件 `transition(..., "Completed", "执行成功")`；**2.7**「非法迁移 → transition() 返回 RuntimeError」。
- **未定义**：NodeCompleted/NodeFailed 处理前是否检查实例当前状态（若无检查，TimedOut 后到达的结果会把实例改成 Completed）；若 transition 拒绝（返回 RuntimeError），4.2 如何处理该错误（unwrap？忽略？）；TimedOut 是否终态、能否重试；被丢弃的结果是否要写 trace（"结果迟到"）。
- **影响/卡点**：超时机制的最后一公里无法实现；若不处理会出现"超时后成功"的幽灵状态，破坏 trace 与 run 判定。
- **建议裁定**：事件处理前置状态守卫：仅当实例处于 Running/WaitingHuman 时才接受 Completed/Failed，否则丢弃 + warning + trace；TimedOut 定义为终态；at-most-once 保证写入 2.7。
- **决策人**：架构。

### S-10 gate 拦截后下游的终态与归属
- **档位**：阻塞
- **文档现状**：**4.2**（L1033-1036）「拦截的投递 → 只记日志（gate_failed），下游自然等待」；**2.7** 终态唯一。
- **未定义**：等待多久？谁判定"不会再来了"？是否有 fail-fast（所有入边都被拦截 → 节点标记 Skipped）？被拦截分支上的节点是否计入 run 完成判定？gate 返回 false 是"本轮不投递"还是"该边永久阻断"？下游 required 输入缺失是否要告警？
- **影响/卡点**：run 可能永远等不到需要的数据（与 C-07/C-10 叠加）；前端无法展示"这条分支被拦掉了"；验收"gate 拦截场景 run 正常结束"无法通过。
- **建议裁定**：引入实例状态 `Skipped`（入边全部 resolved 且无数据满足 → Skip 并记 trace），并把 Skipped 计入终态；run 完成判定使用"无活跃实例"（S-06）。
- **决策人**：架构 + 产品。

### S-11 多 run 并发与热更新交互
- **档位**：需裁定
- **文档现状**：**2.4.1** `run_id`；**4.2** `self.runs.get_mut(&run_id)`（暗示单 Scheduler 管多 run）；**6.4**（L1390-1392）「正在跑的 run 已绑定旧实现，继续跑完；新 run 用新实现」「执行中的实例不受拔插影响」。
- **未定义**：Scheduler 是全局单例还是 per-run；多 run 之间是否共享 Registry（6.4 说全局）与资源配额；图定义缓存 key（graph_hash? graph_id+version?）；旧 run 在符号缓存被清理后如何保持旧实现（6.4 承诺但 6.3 无版本机制，C-21）。
- **影响/卡点**：热更新的承诺无法验证；多 run 隔离无法测试。
- **建议裁定**：明确 Scheduler 单例 + run 表；补 graph 缓存 key 与"符号版本"设计，或删除热替换承诺。
- **决策人**：架构。

### S-12 子图实例化与 input/output_mapping 语义
- **档位**：阻塞
- **文档现状**：**1.1**（L159-160）`input_mapping: "子图外部输入 → 内部节点输入"`、`output_mapping: "内部 End 节点输出 → 子图外部输出"`；**2.4.2** `parent_instance`。
- **未定义**：mapping 的键值方向与时机（"子图外部输入"由谁产生——父图 SubgraphConfig 没有映射字段，C-17）；键是端口名还是"节点.端口"路径；多个 End 时 output_mapping 如何区分；映射失败（缺 key）是加载期还是运行期错误；子图节点 output 是否是 mapping 后的 map；嵌套子图的 parent_instance 链与实例 id 生成。
- **影响/卡点**：子图无法实现；Error 归属（DSL 错误 vs 运行错误）不确定。
- **建议裁定**：与 M-14 一并裁定；若 v1 不做子图，从 DSL/文档移除并标注 v2（否则文档承诺与实现范围不一致）。
- **决策人**：产品 + 架构。

### S-13 HumanInput 端口与恢复上下文
- **档位**：需裁定
- **文档现状**：**4.2**（L1051-1057）写 `"human_input"` 端口 + 迁移 Running + 重新执行；**3.4**（L849-852）「等待注入后继续」；**5.3**（L1182）未到端口为 None；**1.1** InputPort 需声明。
- **未定义**：`human_input` 端口是否需要在节点声明（1.3 规则 6 只校验边，不校验注入端口）；注入数据的 schema 校验（3.2 的 `HumanRequest{prompt, schema}` 前端如何渲染/校验）；恢复后重跑 run() 还是续跑 LLM 循环（C-02）；恢复时 messages 是否保留 tool 调用历史；注入数据是否进 memory；同一实例多次人工请求如何表达（request_id？）。
- **影响/卡点**：人工介入的端到端流程（前端弹出表单 → 提交 → 节点继续）无法实现/验收。
- **建议裁定**：定义 human-in-the-loop 协议：请求事件携带 `request_id/prompt/schema`；提交携带 `request_id` + payload；Harness 以 tool_result 形式续跑（messages 保留）；`human_input` 作为框架保留端口写入 inputs；前端表单由 schema 驱动。
- **决策人**：产品 + 架构。

### S-14 记忆作用域与用户轮次写入
- **档位**：需裁定
- **文档现状**：**3.3**（L774-779）`short_term: 最近 N 轮（默认 20）`；**3.3** 表（L814）「短期记忆｜run 内历史」；**3.2**（L737）只 `append_turn(..., Assistant, ...)`；**3.6** Memory Manager `recall / append_turn / write_long_term`。
- **未定义**：短期记忆是 run 级还是 agent/会话级（多个 agent_call 节点是否共享？同一 agent 在 run 内多次调用是否共享历史？）；用户输入何时写入短期记忆（按 3.2 的实现，历史只有 assistant 轮，与"对话历史"定义矛盾）；`user_id` 维度的隐私与隔离；历史截断与摘要策略。
- **影响/卡点**：3.3 组装出的 messages 结构不正确（缺 user 轮），LLM 行为不可预期；跨节点记忆共享规则缺失导致同一 Agent 多次调用上下文不一致。
- **建议裁定**：定义 `MemoryScope { Run, Agent, Conversation, User }`（v1 推荐 Run/Agent 两级），明确"每次 LLM 调用前 append user、调用后 append assistant"；`append_turn` 增加 role 参数（C-02 同源问题）。
- **决策人**：产品 + 架构。

### S-15 上下文预算溢出与总预算来源
- **档位**：需裁定
- **文档现状**：**3.3** 表（L804-816）默认预算：system 2000、约束 500、KB 1500（top-3）、长期记忆 500（top-5）、短期记忆 3000（20 轮）、用户输入"剩余预算"；**3.4**（L844-847）「上下文溢出｜token 预算超限｜丢弃最旧短期记忆轮次；仍溢出 → 归档到长期记忆后清空该段｜继续循环」。
- **未定义**：总预算（模型上下文窗口）从哪来（AgentConfig？provider 探测？）；token 计数方式（tiktoken？provider usage 回填？）；"归档到长期记忆"的触发频率与写入内容（摘要还是原文）；清空后下一次迭代是否会再次溢出（潜在无限循环）；固定段本身超预算怎么办（system+KB 已超）；预算单位（token vs 字符）。
- **影响/卡点**：3.3 的组装函数无法实现正确的截断；3.4 的溢出分支可能导致死循环（每个 iteration 都溢出→归档→清空→继续）。
- **建议裁定**：定义总预算字段来源与计算式；规定三级降级（截断短期 → 丢弃 KB 段 → 返回可恢复错误 `ContextOverflow`），禁止"归档后继续"的无界路径或为其加轮次上限。
- **决策人**：架构 + 产品。

### S-16 单次响应多 tool_call 与 iteration 计数
- **档位**：需裁定
- **文档现状**：**3.2**（L708）`LlmResponse::ToolCall { tool_name, args }` 单数；iteration 只在 tool 分支 `+1`（L720/L729）；**3.4** max_iterations 默认 10；**3.2**（L691-695）循环顶部检查 `iteration >= max_iterations`。
- **未定义**：OpenAI 风格一次返回多个 tool_calls 如何处理（并发执行？逐个？iteration +1 还是 +N？）；iteration 语义（LLM 调用次数 vs tool 轮次）与"默认 10"的含义；达到上限时最后一条 LLM 文本是否还算输出（C-03）；tool 调用失败重试是否消耗 iteration。
- **影响/卡点**：tool_call 循环的行为取决于此；"10 次" 究竟是多少 token/轮次无法解释给用户。
- **建议裁定**：定义 iteration = LLM 调用轮次；多 tool_call 串行执行并在一条 tool 消息中回填（或 v1 限制 `max_tool_calls=1`，多调用报 `UnsupportedMultiToolCall`）；上限到达时的返回语义由 C-03 决定。
- **决策人**：架构。

### S-17 end 节点输出与 run 最终输出
- **档位**：需裁定
- **文档现状**：**1.2**（L223-229）`end_success` `inputs: []`（C-15）；**2.6**（L595-597）只判定结束，没说输出；**2.4.5**（L517）`RunFinished { run_id, status, at }` 没有 output；**2.4.1** RunContext 没有 output 字段。
- **未定义**：run 的最终 output 存在哪；end 节点是否需要输入来承载结果；多个 end（正常/失败）如何区分（规则 9 只对子图限单 End，主图允许多 end？）；前端如何展示最终结果；`RunFinished` 是否要携带 output 或 output 引用。
- **影响/卡点**：run 的产出无法交付给调用方；API/前端契约无法定义（M-08/M-09）。
- **建议裁定**：`RunContext` 增加 `output: Option<Value>`；end 节点必须声明 inputs 并把聚合结果写入 run.output；多 end 时按到达的 end 决定 status/output（或规则限定 end 必须有唯一 id 并在 RunFinished 中标注）。
- **决策人**：产品 + 架构。

### S-18 DSL 热更新与符号解析时机
- **档位**：需裁定
- **文档现状**：**6.2**（L1273-1275）「DSL 图定义｜每次 run 加载编译｜图是用户编辑产物，天然按 run 变更」；**6.4**（L1360-1362）「换图｜新图加载 → 符号解析 → 未命中符号动态导入 → 执行」；**6.5**（L1429-1435）「图加载失败」。
- **未定义**：加载发生在 run 启动时还是编辑器保存时（校验接口）；加载失败时旧图是否保留、正在跑的 run 是否受影响；符号解析失败的图能否进入 run 表（部分加载）；加载/编译缓存 key 与失效（与 S-11 重叠）；编辑器如何触发校验（API 未定义，M-08）。
- **影响/卡点**：热更新与"保存即校验"的产品体验无法落地；错误归属（加载失败 = run 创建失败？）不确定。
- **建议裁定**：明确"每次 run 启动时 compile + resolve，失败则 run 创建失败并返回 DSL 错误；编辑器保存时调用同一 Loader 做 dry-run 校验（API 见 M-08）"；缓存 key = graph_hash。
- **决策人**：架构。

## 5. 结论

### 5.1 三档归类统计

| 分类 | 条目 | 阻塞级 | 需裁定 | 可自行决策 |
|---|---|---|---|---|
| 1. 内部矛盾（C） | 26 | 13 | 12 | 1 |
| 2. 不可实现伪代码（P） | 12 | 7 | 2 | 3 |
| 3. 缺失规格（M） | 20 | 13 | 6 | 1 |
| 4. 语义边界未定义（S） | 18 | 10 | 8 | 0 |
| **合计** | **76** | **43** | **28** | **5** |

**三档定义**（本报告口径）：
- **阻塞级（43）**：不裁定就无法完成核心实现（骨架、调度器、Harness 契约、数据通路），或裁定错误会导致核心结构返工。**必须在开工前关闭。**
- **需裁定（28）**：需要产品/架构/文档 Owner 给出一句话结论，但可在实现期分批关闭；不关闭则验收标准与测试用例无法固定。
- **可自行决策（5）**：实现者按惯例处理即可，不需要跨角色决策（文档笔误、构造器缺失、语法形式）。

### 5.2 最高优先级 5 条

> 按"不解决的后果严重度 × 依赖被阻塞的条目数"排序。

| # | 主题（聚合条目） | 一句话结论 | 谁拍板 | 不解决会怎样 |
|---|---|---|---|---|
| 1 | **Harness 外部依赖契约全缺**（M-01 AgentConfig、M-02 Memory、M-03 Skill/Tool、M-04 LLM Provider、M-05 KB） | 第 3 章引用了 5 个模块，无一有字段级定义；AgentConfig 甚至没有 schema | 架构（契约）+ 产品（范围） | 第 3 章 100% 无法开工；"能否完成开发"的工时估算不可信 |
| 2 | **人工介入路径自相矛盾**（C-02、C-12、S-13，联动 M-12/C-11） | Err→Crashed 与 Err→WaitingHuman 并存；WaitingHuman 回到 Running 又违反"实例只跑一次" | 产品（交互语义）+ 架构（状态机） | 状态机与 NodeHandler 接口定不下来；人工节点写不出来；后期改动会连带改 Delivery/事件循环 |
| 3 | **run 的失败传播与终止判定缺位**（C-07、C-09、C-10、S-06、S-10，联动 C-11） | Crashed 节点不会 publish 所以 gate 无法"决定后续"；Pending 卡死无人发现；RunFinished 无可靠触发条件 | 架构 + 产品（run 结果语义） | 调度器核心循环无法闭环；前端永远转圈；"卡死必失败"无法验收 |
| 4 | **投递寻址与实例生命周期缺失**（P-04、M-13、S-02，联动 C-04/C-05/S-03） | Delivery 没有 to_instance_id；同一节点能否有多个实例、投给谁全无规则 | 架构 | Scheduler 的数据通路（publish→buffer→InputArrived）写不出来；多实例/自环/并行场景必然出 bug |
| 5 | **超时与迭代终止语义冲突**（C-01、C-03、M-18、S-09，联动 C-26） | 实例 30s 必先于 LLM 60s 到期；max_iterations 同时要求"返回总结"和"Crashed"；无超时事件类型 | 架构 + 产品（默认值与体验） | 可靠性机制无法实现；超时后可能被迟到的完成消息覆盖；默认值给不出 |

补充第 6 位（不在 5 条内但同样影响评估）：**Python 运行时模型**（M-10）——内嵌解释器还是子进程池，直接决定超时、隔离、并发能力能否实现；这一点不裁决，第 5 章的 Python 节点、第 6 章的 gate/符号解析都只能写成"演示级"。

### 5.3 需产品/架构方拍板的 3 个问题

**Q1（范围）Harness 的 5 个外部依赖，哪些属于本次开发范围？**
- 背景：3.6 列出的 Agent Registry、LLM Provider、Skill 库、Memory Manager（MODULE-09）、知识库适配器，全部只有一行契约；AgentConfig 无 schema（M-01–M-05）。
- 选项：A. 全部在范围内，本次补齐契约并实现（工作量并入本次评估）；B. 只做接口 + 本地 stub（如 echo LLM、内存 Memory、空 KB），真实实现另立项目；C. Agent 相关整体 v2，v1 只交付图调度内核（agent_call 节点先用 python_node 模拟）。
- 影响：决定 13 条阻塞级 M 条目、第 3 章是否本次交付、以及整体工期的量级差异。
- 建议：B（接口冻结 + stub）+ 明确真实实现的责任方与时间点；这样图调度内核可以先跑通。

**Q2（人工介入与控制）WaitingHuman 的恢复语义与 Cancel 的终态是什么？**
- 背景：C-02（实例只跑一次 vs 注入后重跑）、C-11（RunStatus 无 Cancelled）、C-12（Err→Crashed vs WaitingHuman）、S-05（暂停/取消的在飞行为）、S-13（human_input 端口与 schema 校验）互相纠缠。
- 选项：A. "续跑"：进入 WaitingHuman 后同一实例以 resume 方式继续 Harness 的 LLM 循环（需要新增 NodeHandler 第二入口 + phase）；B. "重跑"：注入后创建新实例重新执行 run()（与自环一致，简单但语义粗糙）；C. v1 不支持人工介入，只做 pause/resume/cancel。
- 同时需回答：Cancel 是否等于"用户意图成功终止"（新终态 Cancelled，不计失败）？在飞 LLM 是否必须中断（成本 vs 响应速度）？
- 影响：状态机（M-12）、NodeHandler trait（5.1）、事件类型（4.1）、Delivery（P-04）都要按此定稿。
- 建议：A + RunStatus 增加 Cancelled + LLM 尽力中断（abort）。

**Q3（run 结果）节点失败与 gate 拦截如何影响 run 的最终结果？**
- 背景：C-09（gate 无法对 Crashed 节点求值）、C-10/C-07/S-06（终止判定）、S-10（拦截后下游永挂）、S-17（run 输出）。
- 选项：A. 冷峻模式：任一节点 Crashed → run 立即 Failed；被 gate 永久拦截的分支视为 Skipped；end 节点（声明 inputs）聚合 run.output；B. 容错模式：失败/拦截都是"数据"，通过 on_error 边或 gate 路由到补偿分支，run 只有在所有实例终态时才结束；C. A 为 v1，B 为 v2（文档需明确写出"失败不可路由"的限制）。
- 影响：决定 run 何时结束（前端体验）、调度器 NodeFailed 分支、DSL 是否需要 on_error 字段、验收用例。
- 建议：C（v1 = A，文档显式声明限制），并在 DSL 1.1 中预留 `on_error` 字段的保留字，避免 v2 破坏兼容。

### 5.4 建议裁定顺序（按依赖关系）

| 阶段 | 要关闭的条目 | 产出物 | 阻塞的下游 |
|---|---|---|---|
| P0 范围与取舍（1–2 天） | Q1/Q2/Q3、子图取舍（M-14/C-17/S-12）、Checkpoint 是否做恢复（M-06）、Python 运行时模型（M-10） | 一页《范围与取舍决定》 | 全部后续阶段 |
| P1 领域模型冻结 | M-12（状态机+迁移表）、M-13/S-02（实例与寻址）、P-03/P-04（EdgeRef/Delivery）、M-16（输出路由）、M-15（gate 契约）、C-04/C-05（多源规则） | `types.rs` 级定义 + 状态机图 | 调度器、Harness、trace 全部 |
| P2 调度内核 | P-01/P-02（所有权与并发模型）、C-13/C-14（控制通道/暂停）、M-18 + S-09（超时/取消）、C-10/S-06/S-10（终止与失败） | 4.x 章修订稿 + 事件枚举定稿 | 端到端跑通 |
| P3 Harness | M-01–M-05（依赖契约）、P-05/P-06/P-09（接口修正）、M-17（错误映射）、C-19/S-14/S-15/S-16 | 3.x 章修订稿 + trait 定义 | agent_call 节点、成本/质量评估 |
| P4 人工介入 | C-02/C-12、S-05/S-13、C-11 | 人工介入协议（请求/提交/恢复） | 前端表单、控制 API |
| P5 外围 | M-06/M-07（checkpoint/SQLite）、M-08/M-09（API/前端）、M-10（Python 运行时实现）、M-11（错误码）、C-15/C-16/C-21/C-22/C-25/C-26 等文档修订 | 各专项设计 | 交付验收 |

> 说明：P1 与 P2 可并行（不同人、不同文件），但 P1 的"状态机 + 实例寻址 + Delivery"必须先于 P2 的编码；P0 必须最先。

### 5.5 开工准入条件（建议作为评审门槛）

以下 10 项全部满足后，才建议把本设计转为编码任务：

1. AgentConfig / LLM Provider / Skill(ToolDef) / Memory / KB 五个契约有字段级定义或明确的 stub 边界（M-01–M-05）。
2. InstanceStatus 与 RunStatus 枚举 + 迁移矩阵定稿，含 Cancelled / TimedOut / WaitingHuman / Skipped（M-12、C-11）。
3. Delivery 含 `to_instance_id`，实例创建/选择规则写死（P-04、M-13、S-02）。
4. run 启动（entry 输入注入）与 run 终止（含失败、卡死、gate 拦截）的判定规则写死（S-01、C-07、C-09、C-10、S-06、S-10、S-17）。
5. 超时/取消机制写死：预算层级（node vs LLM）、超时事件类型、在飞任务取消方式、迟到结果丢弃规则（C-01、M-18、S-09）。
6. 人工介入协议写死：WaitingHuman 的恢复方式（续跑/重跑）、human_input 端口与 schema、请求/提交事件（C-02、C-12、S-13）。
7. 控制语义写死：pause 冻结范围、cancel 终态与在飞行为、控制通道优先级（C-13、C-14、S-05）。
8. 调度器所有权模型写死：RunContext 是否跨线程共享、事件循环借用策略、实例执行的输入快照（P-01、P-02、P-09）。
9. 输出路由写死：节点输出是整包广播还是端口级映射；gate 是否可改数据（M-16、M-15、C-19）。
10. 文档自身一致：示例通过 1.3 全部校验规则；重复/错误定义修正；gate 符号名规范统一（C-15、C-16、C-22、C-25）。

### 5.6 对「能否完成开发」的判断

- **按现文档直接开工：不能。** 硬性阻断点：第 3、4 章伪代码不可编译（P-01/P-02/P-03/P-04/P-05–P-09）；第 3 章 5 个外部契约缺失（M-01–M-05）；run 无法启动（S-01）也无法可靠结束（C-07/C-10/S-06）；节点状态机未定义（M-12）。
- **关闭 43 条阻塞项后：可以进入编码。** 剩余 28 条"需裁定"属于实现期可并行决策，5 条"可自行决策"由实现者处理，不会阻断骨架。
- **返工风险最高的三处**（若强行"边写边定"）：① Delivery/实例寻址（P-04/M-13/S-02）——影响数据通路全部代码；② 状态机与人工介入（C-02/C-12/M-12）——影响 NodeHandler trait 与所有节点实现；③ 事件循环所有权模型（P-01/P-02）——影响 Scheduler 全部代码。这三处都是"改动即重构"的级别。
- **工作量口径提示**：本报告不替代估算。可以确定的是：补齐文档（43 条阻塞级中约 30 条是文档补写/评审，不是编码）本身就是一条前置工作流；建议按"每条 0.5 天文档 + 一次 2 天契约冻结评审"做计划，再用团队 velocity 校准。**不建议把"文档补齐"与"编码"完全串行**——P1（领域模型）冻结后即可并行开工 Harness 的 mock 与 Loader，但 Scheduler 必须等 P2 的裁定结果。

---

## 附录 A：章节 → 抽取文本行号索引（便于复核原句）

| 章节 | doc.txt 行号 | 本报告相关条目 |
|---|---|---|
| 1.1 类型定义 | L84–161 | C-17、C-19、C-20、C-22、C-25、M-16 |
| 1.2 完整示例 | L163–244 | C-15、C-19、C-22、S-17 |
| 1.3 校验规则 | L246–270 | C-05、C-15、C-25、C-26、M-11 |
| 2.1 数据流全景 | L274–335 | M-06、M-08、M-14、S-01 |
| 2.2 编译期数据流 | L337–369 | C-04、M-14、P-03 |
| 2.3 运行期数据流 | L371–410 | P-01、P-02、S-02 |
| 2.4 核心数据结构 | L412–518 | C-11、C-19、C-23、C-24、M-12、M-13、P-04、P-06、S-17 |
| 2.5 三条数据通道 | L520–547 | C-13、C-23、C-24、M-06、S-05 |
| 2.6 数据流时序走查 | L549–597 | C-10、C-15、C-19、C-23、S-01、S-02、S-06 |
| 2.7 数据流不变式 | L599–627 | C-02、C-04、C-05、C-07、C-08、C-10、C-11、C-24 |
| 3.1 内部状态机 | L633–669 | M-04、M-05、C-12 |
| 3.2 执行循环伪代码 | L671–752 | C-02、C-03、C-12、C-18、C-19、M-01、M-03、M-04、P-06、P-07、P-08、P-09、S-14、S-16 |
| 3.3 上下文组装 | L754–818 | M-01、M-02、M-05、C-19、S-14、S-15 |
| 3.4 停止条件 | L820–857 | C-01、C-03、C-12、M-18、S-08、S-16 |
| 3.5 错误处理矩阵 | L859–881 | C-03、C-09、M-01、M-17 |
| 3.6 与周边模块接口 | L883–915 | C-06、C-18、M-01–M-05、M-17 |
| 3.7 流式输出透传 | L917–944 | M-04、S-16 |
| 4.1 事件类型 | L950–987 | C-11、C-13、M-18、S-05 |
| 4.2 主循环伪代码 | L989–1066 | C-02、C-07、C-09、C-10、C-23、M-13、P-01、P-02、P-04、S-02、S-05、S-09 |
| 4.3 边界条件处理 | L1068–1088 | C-01、C-07、C-11、C-13、C-14、C-26、M-12、M-18、S-03、S-04、S-09、S-10 |
| 5.1 Rust Trait | L1094–1125 | C-12、C-16、P-12 |
| 5.2 NodeContext | L1127–1171 | C-24、M-12、P-05、P-12 |
| 5.3 Python 侧约定 | L1173–1221 | C-08、C-12、M-10、M-16、S-13 |
| 6.1 扩展点边界 | L1227–1245 | C-06、C-09、M-15 |
| 6.2 静态 vs 动态 | L1247–1281 | M-01、S-18 |
| 6.3 SymbolResolver | L1283–1354 | C-20、C-21、C-22、M-10、P-11 |
| 6.4 热更新语义 | L1356–1392 | C-21、S-11、S-18 |
| 6.5 签名校验 | L1394–1435 | C-22、M-11、P-10 |
| 6.6 限制与远期路线 | L1437–1451 | M-10、M-14 |

## 附录 B：引用复核说明

1. 原文档：`/Users/zyzs/Downloads/详细设计-v1.0.html`（标题与正文均写「详细设计 v1.1」，文件名仍为 v1.0）。
2. 复核用纯文本：`/tmp/designdoc/doc.txt`（1451 行）。本报告所有「」引文均可在该文件中按 `Lxxxx` 行号核对；已抽查全部 38 个小节标题与约 40 处关键措辞与 HTML 一致（HTML 表格被拍平为 `|` 分隔文本，字段顺序保持）。
3. 引用以**章节号**为准；行号仅用于快速定位抽取副本。
4. 本报告不评价：产品需求的合理性、性能指标、UI 设计稿、以及文档明确声明为远期路线（6.6）的内容（如 cdylib/wasm、沙箱）；这些不属于"详细设计是否可实现"的判定范围。
5. 未做代码实验/原型验证（由 spike 队友负责）；P 章的编译结论基于 Rust/PyO3/tokio 的语言与库语义推导，若对方 spike 结论不同，应以可运行实验为准并回改本报告。

## 附录 C：按决策人的分组统计（用于排会议）

| 角色 | 参与条目数 | 说明 |
|---|---|---|
| 架构（含与产品共同决策的 35 条） | 69 | 其中 **34 条架构可单独拍板**，35 条需与产品共同确认 |
| 产品（含与架构共同决策的 35 条） | 36 | 集中在体验/语义/范围：C-01/C-02/C-03/C-04/C-08/C-09/C-11/C-17/C-19/C-21/C-26、M-01/M-02/M-03/M-05/M-06/M-08/M-09、S-03/S-04/S-05/S-06/S-07/S-10/S-12/S-13/S-14/S-15/S-17 等 |
| 文档 Owner（仅需修订文档） | 5 | C-05、C-15、C-16（与实现者协同）、C-22、C-25 |
| 实现者 | 5 | C-16、P-07、P-08、P-12、M-20 |
| API 负责人 | 1 | S-01（run 启动 API，与架构共同） |

> 统计口径：76 条明细各有一个「决策人」；一条含多个角色时在每个角色下各计一次（故列合计大于 76）。
> 建议会前分工：**架构先看 5.4 的 P1/P2（领域模型 + 调度内核，约 20 条）**，**产品先看 5.3 的 Q1–Q3 及其关联条目（约 15 条）**，文档 Owner 可独立完成 5 条修订。

