# software_team SOP 设计稿 v1（per-item 评审形态）

> **状态**：设计稿 v1（待独立复审）· 2026-10-31
> **作者**：lead · **分支**：`docs/docs-software-team-sop-v1`
> **关联**：DR-001（技术路线）、spec v1.2 分册 00/01/02/03/04/05、`legacy/config/software_team_graph.json`（SOP 语义来源，冻结只读）、ISSUE-011/ISSUE-062
> **版本约定**：本稿定义 **software_team SOP 设计 v1**（在框架 v0.1.x 能力内）；人工评审、图级有限返工、每节点最大扇出限制、动态 N、共享评审队列延期到 **框架 v0.2**（见 §9）。
> **范围**：只定义新 DSL（schema 1.0 / v1.2 语义）下的软件团队 SOP 图设计；**不含代码改动**。

---

## 1. 目标与取舍

### 1.1 目标

把 `legacy/config/software_team_graph.json` 表达的软件开发 SOP（角色链 + 产物交接 + 评审闸门）落成一张**当前框架可执行**的 DSL 图：

- **设计类角色**是图的关键节点（每角色一个 `agent_call`），负责定义本阶段任务、派发工作 agent、并对评审结论负责；
- 每个设计角色**派发 N 个工作 agent** 产出该阶段的文档/测试产物；
- 每个工作 agent 的产物由**该角色的评审节点**（共享 `design_reviewer` 资产）评审，通过后才进入下游；
- 设计角色按**依赖 DAG** 并发/串行调度；下游节点在自己的 `inputs` 未到齐时**等待**（Pending）；
- 工作 agent 数量 N 在 v1 **编译期固定**，由 codegen 按依赖矩阵展开。

### 1.2 v1 与 legacy 的差异

| 主题 | legacy `software_team_graph.json` | v1 设计 |
|---|---|---|
| 调度 | 300s tick 轮询 + 单 token | 事件驱动；设计角色按依赖 DAG 并发（如 `mde` 与 `te` 并发、`mte` 与 `ste` 并发） |
| 自环 | 11 条 `*_loop` 等待产物落盘 | **删除自环**：产物由工作 agent 在实例内产出，评审通过即推进 |
| 并行 | 单主线 + 旁路 worker | 每个设计角色扇出 N 个工作 agent；跨角色按依赖并发 |
| 评审 | 5 条 manual 边 + 人工 `trigger-edge` | **评审 agent 节点**（共享 `design_reviewer` 资产），无人工等待 |
| 文件端口 | `iport/oport` 文件存在性 | 端口是类型化数据值；产物内容随 agent output 传递 |
| 返工 | 自环重跑 | **v1 不做重做**：评审不通过 → `P` 抛错 → run `Failed`；图级返工框架 v0.2 |
| 共享评审队列 | 无 | v1 不做（每个工作 agent 配独立评审节点）；共享队列框架 v0.2 |

> 依据：用户 2026-10-31 确认的设计方向——设计角色为主链关键节点、设计角色派发工作 agent、评审由共享 `design_reviewer` 资产的评审节点执行（设计角色定义评审标准并对结论负责，而非由设计 agent 亲自评审）；人工评审/图级有限返工/每节点最大扇出限制/动态 N 另起版本。

---

## 2. 核心设计决策

| # | 决策 | 理由 |
|---|---|---|
| D1 | 设计角色 = 依赖 DAG 上的 `agent_call` 节点 | 每角色持有阶段目标/prompt；按依赖并发或串行，不强制单主线 |
| D2 | 工作 agent = **图级节点**（`agent_call` 或 `python_node`），不是设计 agent 的 tool | 需要独立实例状态、超时、trace、gate 与失败语义；tool 调用只有摘要 |
| D3 | 每个工作 agent 配一个**独立评审节点**，所有评审节点引用**同一个 `design_reviewer` 资产** | 规避共享队列（v1 不支持同一节点多次消费）；同一 agent_id 可被多节点引用（R14c 只查存在性） |
| D4 | 节点间用**边**传数据；节点实例独立超时/独立 trace；重试仅自环（`retry.max_attempts`），v1 不使用自环 | 边 + gate 表达「评审通过才放行」；普通节点失败即 `Crashed`→run `Failed` |
| D5 | 评审结论由 `python_node`（`P`）解析成结构化字段；`P` 负责**强制失败** | `agent_call` 输出固定四字段、`content` 为字符串；gate 只能读 JSON 路径 |
| D6 | 下游节点 `trigger: "all"`，等齐依赖 item 后才执行 | 「inputs 没到就等」= Pending 语义 |
| D7 | 评审 agent 的 tool loop 只做**只读验证**（跑检查/测试、对照需求），**不修改工作 agent 产物**；v1 不做重做。`max_iterations` 是 **LLM 调用轮次预算**（默认 10，上限 50） | 工作 agent 的 output 不可变（04 §4.5），评审无法回写；图级重做留 v0.2 |
| D8 | v1 只保留 **approved 路径**；评审最终不通过 → `P` 抛异常 → `NODE_PYTHON_EXCEPTION` → `Crashed` → run `Failed` | gate=false 只会结算边 + 级联 `Skipped`，run 仍可能 `Completed`，不能作为失败机制 |
| D9 | N 编译期固定；N 变化时由 codegen 在 run 前生成图 JSON | 图是静态的；不依赖运行期动态 fan-out |
| D10 | 工作 agent 的产物以**数据值**传递（文档内容/路径清单/结构化结果） | 当前无框架级 artifact 端口/存储；gate 无文件系统访问 |
| D11 | 工作 agent 优先复用已有实现（如 `python/workers/software_team/{planer,archer}.py` 的纯函数），确需 LLM 时用 `agent_call` | 避免重复实现；`python_node` 无 LLM 成本 |

---

## 3. 模式定义：设计角色 + 工作 agent + per-item 评审

对每个设计角色 `D_i`（工作 agent 数 `N_i`）：

```text
上游依赖 item 的 P 节点 ──gate(approved)──▶ D_i          （D_i 的每个依赖 item 一个输入端口）
D_i ──fan-out──▶ W_{i,1} ──┬─▶ R_{i,1} ──▶ P_{i,1} ──gate(approved)──▶ 下游 D_k / end
   ├──fan-out──▶ W_{i,2} ──┼─▶ R_{i,2} ──▶ P_{i,2} ──gate(approved)──▶ 下游 D_k / end
   │                 ⋮      │        ⋮            ⋮
   ├──fan-out──▶ W_{i,N} ──┴─▶ R_{i,N} ──▶ P_{i,N} ──gate(approved)──▶ 下游 D_k / end
   └──(评审上下文)──▶ R_{i,1..N}（D_i 的整包输出投递给每个评审节点）
                     W_{i,j} ──▶ P_{i,j}.artifact（产物直送解析节点，独立于评审结论）
```

- `D_i`：设计角色 agent；输入 = 其依赖 item（每 item 一个端口，`trigger: "all"`）；输出 = 本阶段任务包（整包广播给所有出边）。
- `W_{i,j}`：工作 agent（`agent_call` 或 `python_node`）；接收 `D_i` 的任务包，产出该 item 的成果；**同时**把整包产物送给评审节点 `R` 和解析节点 `P`。
- `R_{i,j}`：评审 agent（`agent_call`，统一 `agent_id: "design_reviewer"`）；两个输入端口：`spec`（来自 `D_i`）、`result`（来自 `W_{i,j}`），`trigger: "all"`。
- `P_{i,j}`：解析节点（`python_node`）；两个输入端口：`review`（来自 `R_{i,j}`）、`artifact`（来自 `W_{i,j}`），`trigger: "all"`；输出 `{ "verdict": "approved", "comments": ..., "artifact": <W 的整包产物> }`。
- `gate`：挂在从 `P_{i,j}` 到下游 `D_k` / `end` 的边上，`check_output { path: "verdict", equals: "approved" }`。
- **下游 `D_k`**：某个依赖 item 的设计角色；每个依赖 item 对应一个输入端口（`trigger: "all"`）。同一 item 可被多个下游角色依赖（如 archer 三份设计既喂 `mde` 也喂 `te`），此时 `P` 有多条出边（各带 approved gate）。
- **等待语义**：`D_k` 在其所有依赖 item 的 approved 投递到齐前保持 Pending；到齐后一次性执行。

> **产物传播（关闭 C1）**：工作 agent 的原始产物通过 `W→P.artifact` 进入 `P`，`P` 在输出里原样透传 `artifact`；下游只从 `P` 的出边取数据，因此拿到的是"评审结论 + 完整产物"，不是评审摘要。评审节点 `R` 只负责给出结论，不承担数据交接。

> **没有独立 `T` 节点（关闭 C3）**：approved item 的"下一个节点"就是**依赖它的下游设计角色 `D_k`**（或 `end`）。若某 item 需要独立的下一阶段节点（如该模块单独的测试执行），把它建模为该下游角色的**工作 agent**，而不是新的节点类型；v1 不引入 `T_{i,j}` 概念。

---

## 4. v1 参考实例（依赖矩阵 + 工作 agent 清单）

### 4.1 设计角色依赖矩阵（与 legacy 的 iport 依赖对齐）

| 设计角色 `D_i` | 依赖（上游 item） | 工作 agent（N） | 产出 item |
|---|---|---|---|
| `planer`（PM） | —（run 输入 `requirement`） | 需求分析 ×1 | `specification` |
| `archer`（SE） | `planer.specification` | 架构设计 / 组织设计 / 迭代计划 ×3 | `arch` / `org` / `iter_plan` |
| `mde`（MDE） | `archer.arch, archer.org, archer.iter_plan` | 模块设计 / 模块迭代计划 ×2 | `module_design` / `module_iter_plan` |
| `te`（TE） | `archer.arch, archer.org, archer.iter_plan` | 总体测试计划 ×1 | `test_plan` |
| `mte`（MTE） | `te.test_plan` | 模块测试用例 ×1 | `module_cases` |
| `ste`（STE） | `te.test_plan` | 系统测试用例 ×1 | `sys_cases` |
| `fo`（FO） | `mde.module_design, mde.module_iter_plan` | 特性流程 ×1 | `feature_process` |
| `mto`（MTO） | `mte.module_cases, fo.feature_process` | 模块测试执行 ×1 | `module_report` |
| `sto`（STO） | `ste.sys_cases, mto.module_report` | 系统测试执行 ×1 | `sys_report` |
| `qa`（QA） | `sto.sys_report` | 验收检查 ×1 | `qa_verdict` |
| `release` | `qa.qa_verdict` | 发布说明 / 变更日志 / 用户手册 ×3 | `notes` / `changelog` / `guide` |
| `end` | `release.notes, release.changelog, release.guide` | — | （run 输出） |

- **并发**：`mde` 与 `te` 在 `archer` 完成后并发；`mte` 与 `ste` 在 `te` 完成后并发；其余为依赖链。
- **等待**：每个设计角色等它的全部依赖 item 批准；`end` 等 3 份发布产物。
- **规模**：11 个设计角色 + 16 个工作 agent + 16 个评审 + 16 个解析 + 1 个 `end` = **60 节点 / 100 边**（codegen 产物，见 §7）。
- 与 legacy 的对应：`legacy` 的 `archer→{mde,te}`、`te→{mte,ste}` 在这里是**依赖 DAG 的并发**，不再是"单 token + 旁路 worker"；`legacy` 的 `fo→sto` "返工后重拉起" 在 v1 不存在（无图级返工），`sto` 只依赖 `ste + mto`。

### 4.2 执行顺序示例

```text
planer ─approved─▶ archer ─┬─approved─▶ mde ─approved─▶ fo ─┐
                           └─approved─▶ te ─┬─approved─▶ mte ─┼─approved─▶ mto ─┐
                                            └─approved─▶ ste ────────────────────┼─approved─▶ sto
                                                                                 ┘
sto ─approved─▶ qa ─approved─▶ release ─approved─▶ end
```

---

## 5. Gate、评审与失败契约

### 5.1 评审结论（v1 不重做）

评审 agent 必须在 `content` 中输出可解析 JSON（示例）：

```json
{"verdict": "approved", "comments": "设计覆盖了需求项 R1–R5"}
```

- `verdict = "approved"`：进入解析节点，正常放行。
- 其它值（如 `"rework"`）：`P` 视为评审不通过并**抛异常** → `NODE_PYTHON_EXCEPTION` → `Crashed` → run `Failed`；v1 **不执行重做**（工作 agent 的 output 不可变，评审无法回写；图级重做见 §9）。
- 评审 agent 的 tool loop 只能调用**只读检查工具**（跑测试/lint/需求对照）；不得写文件或修改工作 agent 产物。
- 达到 `max_iterations` 仍无法给出结论 → `HARNESS_MAX_ITERATIONS` → 实例 `Crashed` → run `Failed`。
- 评审不得返回其它 `verdict` 值；**不要**把"不通过"表达成 gate=false（见 §5.3）。

### 5.2 解析节点 `P` 的契约（关闭 C1/C2）

- `type: "python_node"`；
- 输入：`review`（`R` 的整包输出）、`artifact`（`W` 的整包产物），`trigger: "all"`；
- 输出：`{ "verdict": "approved", "comments": <string>, "artifact": <W 的整包产物> }`；
- **强制失败**：若 `content` 非法 JSON、缺少 `verdict`、或 `verdict != "approved"`，`P` **抛 Python 异常**；
- 异常语义：`PythonException` → 实例 `Crashed`（`NODE_PYTHON_EXCEPTION`）→ run `Failed`（冷峻失败，不继续、不跳过）。
- `artifact` 必须原样透传，供下游设计角色消费（解析节点不得只输出摘要）。

### 5.3 gate 只做"最后一道保险"

```json
{ "function": "check_output", "params": { "path": "verdict", "equals": "approved" } }
```

- gate 挂在 `P → 下游 D / end` 的边上；`P` 只会输出 `verdict = "approved"`，因此该 gate 在 v1 是**不变量校验**（防未来改动）。
- **不能**把 gate=false 当作失败机制：内核实测（REV-062 §4）显示 gate=false 只结算边并级联下游 `Skipped`，run 仍可能 `Completed`（`run_output = null`）。v1 的失败一律由 §5.2 的抛错承担。

### 5.4 下游等待

- `D_k` / `end` 的 `inputs` 全部 `required: true`，`trigger: "all"`；未到齐则 Pending。
- 「inputs 不达标」在 v1 **没有"等一份新的合格输入"机制**：评审不通过即 `P` 抛错 → run Failed；重做/回环见 §9（框架 v0.2）。

---

## 6. Agent 资产与执行预算

| 资产 `id` | 类型 | 作用 | 关键配置 |
|---|---|---|---|
| `planer` / `archer` / `mde` / `te` / `mte` / `ste` / `fo` / `mto` / `sto` / `qa` / `release` | 设计角色 agent | 持有阶段目标、评审标准与任务包 | `role_prompt_file`、`constraints_file`、`input_mapping`、`max_iterations` |
| `design_reviewer` | 评审 agent（共享） | 评审单个 item；tool loop 只做**只读检查**，不修改产物 | 固定 JSON 输出 prompt；`tools` 仅含只读检查工具；`max_iterations` 10–20；`node_timeout_ms` 300000 |
| `*_work` | 工作 agent | 产出该 item 的文档/测试 | `agent_call`（需 LLM）或 `python_node`（确定性任务） |

- **只有 `agent_call` 节点通过 `agent_id` 引用资产**；`python_node` 用 `module` + `function`，`start`/`end` 用 `{}`。
- 资产根：CLI `--assets-dir` / env `ZGRAPH_ASSETS`；工具根：`--tools-dir` / `ZGRAPH_TOOLS` / `<assets-dir>/tools`。
- 预算建议：评审 agent `max_iterations` 10–20（上限 50）、`node_timeout_ms` 300000；设计角色/工作 agent `node_timeout_ms` 600000（并受实例超时 `timeout_ms` 覆盖）。
- **复用（关闭 S7）**：`planer` / `archer` 的工作 agent 可直接用 `python/workers/software_team/{planer,archer}.py` 的纯函数（`python_node`），与 legacy `planer_proc` / `archer_proc` 语义对应；需要 LLM 产出的 item 才用 `agent_call`。

---

## 7. N 与 codegen 规则（关闭 C3）

图是静态 JSON；按 §4.1 的**依赖矩阵**在生成图时展开：

```text
roles = [planer, archer, mde, te, mte, ste, fo, mto, sto, qa, release]
items(role) = §4.1 的产出 item 列表；deps(role) = §4.1 的依赖 item 列表

# 1) 节点
for role in roles:
    dep_ports = [每个 dep item 一个端口] 或（planer）[requirement 端口]   # entry 输入由 run 注入
    D[role] = agent_call(agent_id=role, inputs=dep_ports, trigger="all")
    for item in items(role):
        W[role][item] = agent_call 或 python_node   # 见 §6 复用
        R[role][item] = agent_call(agent_id="design_reviewer",
                                   inputs=[spec, result], trigger="all")
        P[role][item] = python_node(module="workers.software_team.parse_verdict", function="run",
                                    inputs=[review, artifact], trigger="all")
end = end(inputs=release 的 3 个 item 端口, trigger="all")
entry_points = [planer]          # planer 的 requirement 由 RunSpec.entry_inputs 注入，不生成入边

# 2) 边
for role in roles:
    for item in items(role):
        D[role] -> W[role][item].task
        D[role] -> R[role][item].spec
        W[role][item] -> R[role][item].result
        W[role][item] -> P[role][item].artifact
        R[role][item] -> P[role][item].review
        for downstream in roles 中依赖 (role,item) 的角色:
            P[role][item] -> D[downstream].<port name>，gate approved
        if (role,item) 被 end 依赖:
            P[role][item] -> end.<port name>，gate approved
```

- 端口名必须匹配 `^[A-Za-z_][A-Za-z0-9_]*$`（不能使用文件路径）；每个下游端口对应一个依赖 item。
- `(from,to_node)` 与 `(to_node,to_input_port)` 各自唯一 → 每个 item 用独立端口。
- 生成后必须 `zgraph validate` 通过（§11）。
- 按 §4.1 的 N_i 展开（工作 agent 合计 16 个）的参考图规模：**60 节点 / 100 边**。

---

## 8. 与 legacy 图对照

| legacy 元素 | v1 对应 | 差异 |
|---|---|---|
| 12 节点 / 25 边（11 自环 + 14 前进） | 11 设计角色 + 16 工作/评审/解析节点 + 1 end（60 节点 / 100 边） | 节点数随 N 展开；无自环 |
| 11 条 `*_loop` 自环（gate=false 等待） | 删除 | 工作 agent 同步产出，不再轮询 |
| 5 条 `driver:"manual"` 审批边 | 评审 agent + approved gate | 评审由 agent 完成；人工评审框架 v0.2 |
| `archer→{mde,te}`、`te→{mte,ste}` 并行 | 依赖 DAG 并发（mde/te 并发、mte/ste 并发） | 不再依赖单 token 旁路 dispatch |
| `fo→sto`（返工后重拉起） | 删除 | v1 无图级返工 |
| `iport/oport` 文件组 | 类型化输入/输出端口（数据值） | 不再依赖磁盘文件；文档内容随 output 传递 |
| gate `check_output`（查产物文件） | gate `check_output`（查 `verdict` 字段） | 从「文件齐否」变为「评审结论」（仅作不变量） |
| `cmd` / `pre_handle` / `post_handle` | 无 | 交接副作用并入工作/评审 agent |
| `parallel` 扇出 | 每角色 N 个工作 agent 扇出 | 语义一致（并发派发） |

---

## 9. 延期项（框架 v0.2 候选）

1. **人工评审**：`WaitingHuman` + 外部 decide/resume + durable run（v1 无人工等待；`manual` gate 仅静态值）。
2. **图级有限返工**：可重臂 gate / 带出口 cycle；当前带出口环会以 `RUN_DEADLOCK` 或 `RUN_DELIVERY_TO_TERMINAL` 失败（视 `trigger`/gate 配置，ISSUE-062 的两种失效模式）；引入后评审不通过可走 rejected 边回到工作节点。
3. **每节点最大扇出限制**：v1 无每节点 fan-out 上限；v0.2 增加图级/运行期上限与校验。
4. **动态 N**：运行期 map/reduce fan-out（当前 N 编译期固定，需 codegen）。
5. **共享评审队列**：一个评审节点按到达顺序排队处理多份投递，评完一个放行一个（需要节点邮箱/多次激活/按 item 路由，内核级改动）。

> 以上五项均属于执行模型扩展，不在本设计稿的 v1 落地范围内；落地前需另立 DR。

---

## 10. 已知限制与风险

- **无人工审批**：若 SOP 要求人签字（需求评审/发布审批），v1 只能由 agent 代评或拆到图外。
- **无图级返工环**：评审不通过即 `P` 抛错 → run Failed；v1 没有重做通道（图级返工见 §9）。
- **无框架级 artifact 端口/存储**：文档以数据值传递；如需落盘由工具/工作 agent 写文件，但内核与 gate 不可见（gate 不能查文件）。
- **评审 JSON 脆弱性（关闭 S4）**：评审 `content` 可能被 `max_tokens` 截断（`stop_reason=MaxTokens` 属正常返回）或带 Markdown 围栏；`P` 解析失败 → `Crashed` → run `Failed`。需在 prompt 中约束纯 JSON，并在 `P` 中给出明确错误信息。
- **评审超时预算（关闭 S4）**：评审 agent 多轮 tool loop 与单次 LLM 60s、实例默认 180s 的关系需显式配置；建议评审节点 `node_timeout_ms=300000`、`max_iterations≤20`。
- **LLM 并发无全局限流**：N 个工作 agent / 设计角色会并发发起 LLM 调用；provider 限流需外部控制（减小 N / 串行化依赖）。
- **agent 输出固定四字段**：业务字段必须经 `python_node` 解析，且 `content` 必须可解析 JSON。
- **工具串行**：单响应多 tool_call 串行执行；大量并行工具应拆成图级节点。
- **每 run worker 上限 8**（Python worker）：工作 agent 使用 tools 时与节点执行共享该上限。
- **N 静态**：N 变化必须重新生成图；同一图不能在运行期改变 fan-out 宽度。

---

## 11. v1 落地验收（关闭 S3）

> 本设计稿的 v1 验收；框架 v0.2 能力（§9）另行验收。

1. `zgraph validate` 通过 §7 codegen 生成的参考图（按 §4.1 的 N_i，含 agent 资产索引）。
2. fake LLM 脚本驱动端到端：设计角色 → N 工作 agent → N 评审 → approved gate → 下游设计角色；断言每个 item 有独立实例与 trace。
3. **产物传播**：断言下游设计角色收到的 `artifact` 是工作 agent 的完整产物（不是评审摘要）。
4. **失败路径**：评审 agent 达到 `max_iterations` → run `Failed`（`HARNESS_MAX_ITERATIONS`）；`P` 收到非 approved / 非法 JSON → `NODE_PYTHON_EXCEPTION` → run `Failed`；断言不出现"gate 拦截后 run Completed(null)"被误当成失败。
5. **等待语义**：下游 `trigger=all` 节点在某依赖 item 未批准时保持 Pending。
6. **规模/成本**：按 §4.1 的 N_i 生成的参考图（60 节点/100 边）的 LLM 调用次数、worker 子进程数、run 时长基线。

---

## 附录 A：单 item 管线骨架（已用 `zgraph validate` 验证）

> 仅演示一条 item 管线（`D → W → R → P → 下游 D`，含 `W→P.artifact` 产物传播）；完整 11 角色图由 §7 规则 codegen 展开。
> 实测：`zgraph validate` → `✓ graph valid: 5 nodes, 6 edges, 0 subgraphs (graph_hash=15f4501150237554)`（exit 0）。

```json
{
  "version": "1.0",
  "metadata": { "name": "software_team_v1_item_pipeline" },
  "entry_points": ["planer"],
  "nodes": [
    { "id": "planer", "type": "agent_call",
      "inputs": [{ "name": "requirement", "type": "string", "required": true }],
      "config": { "agent_id": "planer" } },
    { "id": "planer_req", "type": "agent_call",
      "inputs": [{ "name": "task", "type": "any", "required": true }],
      "config": { "agent_id": "planer_work" } },
    { "id": "planer_req_review", "type": "agent_call",
      "inputs": [
        { "name": "spec", "type": "any", "required": true },
        { "name": "result", "type": "any", "required": true }
      ],
      "trigger": "all",
      "config": { "agent_id": "design_reviewer" } },
    { "id": "planer_req_parse", "type": "python_node",
      "inputs": [
        { "name": "review", "type": "any", "required": true },
        { "name": "artifact", "type": "any", "required": true }
      ],
      "trigger": "all",
      "config": { "module": "workers.software_team.parse_verdict", "function": "run" } },
    { "id": "archer", "type": "agent_call",
      "inputs": [{ "name": "spec", "type": "any", "required": true }],
      "config": { "agent_id": "archer" } }
  ],
  "edges": [
    { "from": "planer", "to_node": "planer_req", "to_input_port": "task" },
    { "from": "planer", "to_node": "planer_req_review", "to_input_port": "spec" },
    { "from": "planer_req", "to_node": "planer_req_review", "to_input_port": "result" },
    { "from": "planer_req", "to_node": "planer_req_parse", "to_input_port": "artifact" },
    { "from": "planer_req_review", "to_node": "planer_req_parse", "to_input_port": "review" },
    { "from": "planer_req_parse", "to_node": "archer", "to_input_port": "spec",
      "gate": { "function": "check_output", "params": { "path": "verdict", "equals": "approved" } } }
  ]
}
```

## 附录 B：关键 DSL 约束（落地时必须满足）

- `version` 必须为 `"1.0"`；顶层键仅 `version/metadata/entry_points/nodes/edges/subgraphs`。
- 节点 id 1–64 字符 `[A-Za-z0-9_.-]`；端口名必须是 Python 标识符。
- `(from,to_node)` 与 `(to_node,to_input_port)` 各自唯一 → 每 item 用独立端口/节点。
- 非 entry 节点入边 ≥1；`trigger=all` 时每个 `required` 端口都要有入边。
- 带 gate 的环合法，但**带出口的环当前不可运行**（两种失效模式见 §9 第 2 项）→ v1 不使用任何环。
- `agent_call` 的 `agent_id` 必须在资产索引中存在；`python_node` 只校验 `module.function` 格式，import 在 worker 启动期。
- `end` 必须 ≥1 个 `required` 输入且 `trigger=all`（本设计 3 个）。
- `subgraph` 加载期通过、运行期拒绝（v1 不使用）。
