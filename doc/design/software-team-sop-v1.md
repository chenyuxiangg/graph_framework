# software_team SOP 设计稿 v1.1（per-item 评审 + 有限返工形态）

> **状态**：设计稿 **v1.1**（W2a0 冻结；REV-067）· 2026-10-31
> **作者**：lead（v1）/ docs-writer（v1.1 升级，W2a0）· **分支**：`docs/docs-rework-design`
> **关联**：DR-001（技术路线）、[`DR-005-w2a-defer-gate-freeze.md`](../decisions/DR-005-w2a-defer-gate-freeze.md)（返工语义冻结）、spec v1.2 分册 00/01/02/03/04/05/06/07、`legacy/config/software_team_graph.json`（SOP 语义来源，冻结只读）、ISSUE-011/ISSUE-062/ISSUE-100
> **版本约定（v1.1）**：本稿同时定义两个 profile——**approved-only v1**（框架 v0.1.x 能力内、语义不变）与 **rework v1.1**（框架 v0.2 / W2a 的 opt-in deferrable gate + attempt-per-activation）。人工评审、每节点最大扇出限制、动态 N、共享评审队列仍在延期项（§9，对应框架 W3a/W2b/W4b/W5）。
> **v1.1 增量（相对 v1）**：两个 profile 与环上 defer 边统一规则（§3.2）、`parse_verdict_rework` 契约（§5.2）、`max_attempts` 预算（§6/§7.3）、codegen `--mode`（§7）、有限返工验收（§11）。approved-only v1 的**图结构/语义/`graph_hash` 不变**（DR-005 §6.1）。
> **范围**：只定义新 DSL（schema 1.0 / v1.2 语义）下的软件团队 SOP 图设计；**不含代码改动**。

---

## 1. 目标与取舍

### 1.1 目标

把 `legacy/config/software_team_graph.json` 表达的软件开发 SOP（角色链 + 产物交接 + 评审闸门）落成一张**当前框架可执行**的 DSL 图：

- **设计类角色**是图的关键节点（每角色一个 `agent_call`），负责定义本阶段任务、派发工作 agent、并对评审结论负责；
- 每个设计角色**派发 N 个工作 agent** 产出该阶段的文档/测试产物；
- 每个工作 agent 的产物由**该角色的评审节点**（共享 `design_reviewer` 资产）评审，通过后才进入下游；
- 设计角色按**依赖 DAG** 并发/串行调度；下游节点在自己的 `inputs` 未到齐时**等待**（Pending）；
- 工作 agent 数量 N 在 **编译期固定**，由 codegen 按依赖矩阵展开；
- **（v1.1）rework profile**：评审返回 `rework` 时沿回边 `P→W` 重入工作节点，**有限次**返工后要么 `approved` 推进、要么超 `max_attempts` 冷峻失败（§3.2/§6.4）；`approved-only` v1 仍保持「评审不通过 → `P` 抛错 → run `Failed`」。

### 1.2 v1/v1.1 与 legacy 的差异

| 主题 | legacy `software_team_graph.json` | v1 / v1.1 设计 |
|---|---|---|
| 调度 | 300s tick 轮询 + 单 token | 事件驱动；设计角色按依赖 DAG 并发（如 `mde` 与 `te` 并发、`mte` 与 `ste` 并发） |
| 自环 | 11 条 `*_loop` 等待产物落盘 | **删除自环**：产物由工作 agent 在实例内产出，评审通过即推进（v1.1 的返工**不是**自环，是 `P→W` 回边 + attempt 重入） |
| 并行 | 单主线 + 旁路 worker | 每个设计角色扇出 N 个工作 agent；跨角色按依赖并发 |
| 评审 | 5 条 manual 边 + 人工 `trigger-edge` | **评审 agent 节点**（共享 `design_reviewer` 资产），无人工等待 |
| 文件端口 | `iport/oport` 文件存在性 | 端口是类型化数据值；产物内容随 agent output 传递 |
| 返工 | 自环重跑 | **approved-only v1**：评审不通过 → `P` 抛错 → run `Failed`；**rework v1.1**：`parse_verdict_rework` 的 `rework` 是正常输出 → 回边重入 → 有限返工（`max_attempts` 兜底） |
| 共享评审队列 | 无 | v1/v1.1 不做（每个工作 agent 配独立评审节点）；共享队列框架 v0.3（W5） |

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
| D12（v1.1） | **双 profile**：`approved-only`（v1，`parse_verdict`、无 defer、图/hash 不变）与 `rework`（v1.1，`parse_verdict_rework`、defer 回边）由 codegen `--mode` 生成；同一份设计稿、两套边标注 | 兼容基线（v0.1.0/W1）与返工闭环互不污染；e2e 可分别断言 |
| D13（v1.1） | **环上可重入边全标 `defer`**：`P→W`（gate=rework）、`W→R`、`W→P.artifact`、`R→P`、`P→下游/end`（gate=approved）；`D→W.task` / `D→R.spec` 在环外，保持 settle | R15 要求 defer 源可重入；统一规则避免「漏标一条边 → 运行期 `RUN_DELIVERY_TO_TERMINAL`」 |
| D14（v1.1） | **attempt-per-activation**：defer 边放行且目标已 `Completed` → 新建 `attempt+1` 实例（继承前次输入 buffer 快照）；`W/R/P` 节点 `config.retry.max_attempts`（缺省 3）兜底，超限 → `NODE_INTERNAL` run Failed | 有限返工、无死循环；不新增 DSL 字段（复用已有 `max_attempts`） |
| D15（v1.1） | `W` 增**可选** `feedback` 输入端口承接 `P→W` 回边（`task` 端口已被 `D→W` 占用，R11 禁止双源） | 首次 `feedback=null`、重入靠 buffer 继承；不改变 approved-only 的 `W` 结构 |

---

## 3. 模式定义：设计角色 + 工作 agent + per-item 评审

### 3.1 approved-only（v1，语义不变）

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

### 3.2 rework（v1.1，W2a；DR-005 §6）

> **opt-in**：只在 `--mode rework` 的 codegen 产物中生效；approved-only v1 的节点/边/`graph_hash` 完全不变（§7.1）。

单 item 的 rework 拓扑（`D_i` 的一个 item）：

```text
                      ┌──────────────────── rework 回边（gate=rework, defer）───────────────────────┐
                      │                                                                              ▼
  D_i ──task──▶ W_{i,j} ──result──▶ R_{i,j} ──review──▶ P_{i,j} ──gate(approved, defer)──▶ 下游 D_k / end
     │            │                    ▲                    ▲
     └──spec─────┼────────────────────┘                    │
                  └──artifact───────────────────────────────┘
```

| 边 | gate | `on_gate_false` | 说明 |
|---|---|---|---|
| `D_i → W.task` | 无 | 缺省（settle） | 首次派发；**不在环上**，不得标 defer |
| `D_i → R.spec` | 无 | 缺省（settle） | 首次评审上下文 |
| `W → R.result` | 无 | **defer** | 目标 `R` 已 `Completed` → attempt-per-activation |
| `W → P.artifact` | 无 | **defer** | 目标 `P` 已 `Completed` → attempt-per-activation |
| `R → P.review` | 无 | **defer** | 目标 `P` 已 `Completed` → attempt-per-activation |
| `P → W.feedback` | `check_output{path:"verdict",equals:"rework"}` | **defer** | 回边：`rework` 放行 → `W` 重入；`approved` 时 false → 挂账（dormant） |
| `P → D_k / end` | `check_output{path:"verdict",equals:"approved"}` | **defer** | 前进边：`approved` 放行；`rework` 时 false → 挂账（dormant） |

- **`feedback` 端口**：`W_{i,j}` 在 rework profile 增 `feedback`（`type:"any"`、`required:false`）承接 `P→W` 回边；首次激活 `feedback=null`，`trigger:"all"` 的 required 只有 `task`。`P→W` **不能**复用 `task` 端口（R11：`(to_node,to_input_port)` 唯一）。
- **统一规则（DR-005 §6.2）**：**环上所有可能重入已 `Completed` 目标实例的边都必须标 `defer`**。DR-004 §3 W2a0 的「两条 defer 边（approved 前进 / rework 回边）」是**最小子集**；v1.1 codegen 展开为环上全部可重入边（`W→R`、`W→P.artifact`、`R→P`），同时满足 REV-063 C11 与 spec 02 **R15**。
- **R13/R15 自检**：环上 gate-less 边（`W→R`、`R→P`、`W→P.artifact`）不构成 R13 的无 gate 环（`P→W` 带 gate 打断）→ R13 通过；defer 源 `W/R/P` 均落在 `SCC={P,W,R}` 且 SCC 内含 defer 边 → R15 通过。
- **重入数据完整性（DR-005 §3.2 行 6/7，Lead 2026-10-31 裁决补全）**：defer 边放行到已 `Completed` 目标时，新 attempt **按端口继承**——只继承「该端口入边 `on_gate_false != Defer`」的 `values`/`arrived`（值 + 已到 + 结算），**入边为 defer 的端口不继承、必须由当前这一代重新投递**；同时**创建重入实例时清空该节点的 `settled_by_node`**。于是多输入的 `W`（`task` 继承 + `feedback` 重投）与 `R`（`spec` 继承 + `result` 重投）能重新满足 `trigger=all`；`P` 的 `review`/`artifact` 都 defer → **都等新一代投递**，不会用旧评审提前执行。
- **冷峻语义不变**：`R` 的 `HARNESS_*`、`P` 的非法 JSON/未知 verdict 仍是 `Crashed` → run `Failed`；返工只由「合法 `verdict=rework`」驱动。

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
- 与 legacy 的对应：`legacy` 的 `archer→{mde,te}`、`te→{mte,ste}` 在这里是**依赖 DAG 的并发**，不再是"单 token + 旁路 worker"；`legacy` 的 `fo→sto` "返工后重拉起" 在 **approved-only v1** 不存在，在 **rework v1.1** 由 per-item `P→W` 回边 + attempt 重入承接（§3.2），`sto` 仍只依赖 `ste + mto`。
- **rework profile 规模**：节点数不变（60），边数 100 → **116**（每 item 一条 `P→W.feedback` 回边，16 个 item）。


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

### 5.1 评审结论（双 profile）

评审 agent 必须在 `content` 中输出可解析 JSON（示例）：

```json
{"verdict": "approved", "comments": "设计覆盖了需求项 R1–R5"}
```

- **闭集**：`verdict ∈ {"approved", "rework"}`（评审批次 ISSUE-098 顺带修：不再用「其它值」这种冗余表述）。
- **approved-only（v1）**：`verdict = "approved"` → 正常放行；`"rework"`/其它值 → `P` 视为评审不通过并**抛异常** → `NODE_PYTHON_EXCEPTION` → `Crashed` → run `Failed`；v1 **不执行重做**。
- **rework（v1.1）**：`"approved"` 与 `"rework"` 都是**正常输出**；`rework` 经 `P→W.feedback` 回边驱动 attempt 重入；非法 JSON/未知 verdict/形状错误仍抛异常 → `Failed`（§5.2）。
- 评审 agent 的 tool loop 只能调用**只读检查工具**（跑测试/lint/需求对照）；不得写文件或修改工作 agent 产物/共享产物。
- 达到 `max_iterations` 仍无法给出结论 → `HARNESS_MAX_ITERATIONS` → 实例 `Crashed` → run `Failed`。
- 评审不得返回其它 `verdict` 值；**不要**把"不通过"表达成 gate=false（见 §5.3）。

### 5.2 解析节点 `P` 的契约（关闭 C1/C2；v1.1 双函数）

**approved-only（v1，不变）**：

- `type: "python_node"`；`config: { module: "workers.software_team.parse_verdict", function: "run" }`；
- 输入：`review`（`R` 的整包输出）、`artifact`（`W` 的整包产物），`trigger: "all"`；
- 输出：`{ "verdict": "approved", "comments": <string>, "artifact": <W 的整包产物> }`；
- **强制失败**：若 `content` 非法 JSON、缺少 `verdict`、或 `verdict != "approved"`，`P` **抛 Python 异常**；
- 异常语义：`PythonException` → 实例 `Crashed`（`NODE_PYTHON_EXCEPTION`）→ run `Failed`（冷峻失败，不继续、不跳过）。
- `artifact` 必须原样透传，供下游设计角色消费（解析节点不得只输出摘要）。

**rework（v1.1）**：

- `config: { module: "workers.software_team.parse_verdict", function: "parse_verdict_rework" }`；
- 输入/输出端口与 above 相同；输出 `{ "verdict": "approved" | "rework", "comments": <string>, "artifact": <原样> }`；
- 契约（DR-005 §5.1）：`review`/`artifact` 必填；`content` 必须解析为 **object**；`verdict` 闭集 `{"approved","rework"}`；`comments` 缺省 `""`、给了必须 string；非法 JSON/未知 verdict/形状错误 → `ValueError` → `NODE_PYTHON_EXCEPTION`；
- 纯函数/幂等/无外部副作用（docstring 按 04 §4.3.7 L3 四项声明）；
- `artifact` 原样透传（`W` 的整包产物；返工时下游拿到的是**最新一次** `W` 的产物）。

### 5.3 gate 只做"最后一道保险"

```json
{ "function": "check_output", "params": { "path": "verdict", "equals": "approved" } }
```

- gate 挂在 `P → 下游 D / end` 的边上；`P` 只会输出 `verdict = "approved"` 时放行，因此该 gate 是**不变量校验**（防未来改动）。
- **不能**把 gate=false 当作失败机制：v0.1.0 内核里 gate=false 只结算边并级联下游 `Skipped`，run 仍可能 `Completed`（`run_output = null`）。v1 的失败一律由 §5.2 的抛错承担。
- **v1.1 的 gate=false 有两种角色**：① approved 前进边 false（`verdict=rework`）→ **defer 挂账**（不结算/不 Skipped），等下次 approved 覆盖；② rework 回边 false（`verdict=approved`）→ defer 挂账 → dormant。二者都**不是**失败机制（DR-005 §3.2）。

### 5.4 下游等待

- `D_k` / `end` 的 `inputs` 全部 `required: true`，`trigger: "all"`；未到齐则 Pending。
- approved-only v1：「inputs 不达标」没有"等一份新的合格输入"机制：评审不通过即 `P` 抛错 → run Failed。
- rework v1.1：defer 挂账的 approved 前进边**不结算**下游入边；但只要后续 attempt 产出 `approved`，同一 `P` 的同一前进边会放行（目标已 `Completed` 时按 attempt 重入），下游最终拿到合格输入。若永无 `approved`，由 `max_attempts` 兜底失败（不会静默 `Completed(null)`）。


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
- **返工预算（v1.1，DR-005 §6.4）**：rework profile 的 `W`/`R`/`P` 节点写 `config.retry.max_attempts`（缺省 3）。语义 = 节点实例 attempt 上限（合法 attempt `0..max-1`），自环与 defer 重入统一生效；默认 3 → 每节点最多 3 次实例（0/1/2）→ 最多 **2 次 rework**；第 3 次 rework 在 `P→W` 放行时 `W` 新实例 `attempt=3` 超限 → `Crashed(Internal)` → run `Failed(NODE_INTERNAL)`（不新增运行期错误码）。approved-only 不写该字段（缺省常量生效，与 W1 一致）。

---

## 7. N 与 codegen 规则（关闭 C3；v1.1 双 profile）

图是静态 JSON；按 §4.1 的**依赖矩阵**在生成图时展开。codegen 增 `--mode approved|rework`（缺省 `approved`，W1 产物字节级不变）。

### 7.1 通用规则（两 profile 共用）

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
        P[role][item] = python_node(module="workers.software_team.parse_verdict",
                                    function=(mode=="rework" ? "parse_verdict_rework" : "run"),
                                    inputs=[review, artifact], trigger="all")
end = end(inputs=release 的 3 个 item 端口, trigger="all")
entry_points = [planer]          # planer 的 requirement 由 RunSpec.entry_inputs 注入，不生成入边

# 2) 边（approved profile）
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

### 7.2 rework profile 增量（v1.1）

```text
# 在 §7.1 基础上：
# a) W 节点增可选 feedback 端口（approved profile 不加）
W[role][item].inputs += { name: "feedback", type: "any", required: false }

# b) 已有边标 defer（无 gate 的三条）
W -> R.result         : on_gate_false = "defer"
W -> P.artifact       : on_gate_false = "defer"
R -> P.review         : on_gate_false = "defer"

# c) P -> 下游/end 的 approved gate 边：追加 defer
P -> D[downstream].<port> : gate approved, on_gate_false = "defer"
P -> end.<port>           : gate approved, on_gate_false = "defer"

# d) 新增回边 P -> W.feedback（gate rework, defer）
P -> W.feedback : gate { check_output{path:"verdict", equals:"rework"} }, on_gate_false = "defer"

# e) 重跑预算（W/R/P）
W/R/P[role][item].config.retry.max_attempts = 3   # 可参数化；e2e 固定 3
```

- **规模**：approved profile = **60 节点 / 100 边**（W1 基线，hash 不变）；rework profile = **60 节点 / 116 边**（+16 条 `P→W` 回边；`W` 的 `feedback` 端口不改节点数）。
- **统一规则自检**：codegen 生成后遍历 `SCC={P,W,R}` 上的全部边，**每条必须** `on_gate_false="defer"`（`D→W.task`/`D→R.spec` 在环外，必须 settle）；漏标即 codegen 自身报错（W2a e2e 断言）。
- **R15 自检**：环上 defer 源 `W/R/P` 的 SCC 含 defer 边 → 通过；`P→D/end` 的源 `P` 同 SCC → 通过。
- **approved profile 的 hash 不变**：`--mode approved` 不写 `on_gate_false` 键、不写 `feedback`/`max_attempts` → 与 W1 逐字节一致（`3d8d735b1d5e8a1b` / `df9341c04063b80a` / `a8e508deb9c6cf44`）。
- 端口名必须匹配 `^[A-Za-z_][A-Za-z0-9_]*$`（不能使用文件路径）；每个下游端口对应一个依赖 item；`(from,to_node)` 与 `(to_node,to_input_port)` 各自唯一（`feedback` 是 `W` 的独立端口，避开 `task` 双源）。
- 生成后必须 `zgraph validate` 通过（含 R15；§11）。


---

## 8. 与 legacy 图对照

| legacy 元素 | v1 / v1.1 对应 | 差异 |
|---|---|---|
| 12 节点 / 25 边（11 自环 + 14 前进） | 11 设计角色 + 16 工作/评审/解析节点 + 1 end（approved 60 节点 / 100 边；rework 60/116） | 节点数随 N 展开；无自环 |
| 11 条 `*_loop` 自环（gate=false 等待） | 删除 | 工作 agent 同步产出，不再轮询；v1.1 返工用 `P→W` 回边 + attempt 重入（非自环轮询） |
| 5 条 `driver:"manual"` 审批边 | 评审 agent + approved gate | 评审由 agent 完成；人工评审框架 v0.3（W3a） |
| `archer→{mde,te}`、`te→{mte,ste}` 并行 | 依赖 DAG 并发（mde/te 并发、mte/ste 并发） | 不再依赖单 token 旁路 dispatch |
| `fo→sto`（返工后重拉起） | **rework v1.1**：`P→W` 回边 + defer 边 + attempt-per-activation | approved-only 仍无图级返工；返工由内核 opt-in deferrable gate 承载（DR-005） |
| `iport/oport` 文件组 | 类型化输入/输出端口（数据值） | 不再依赖磁盘文件；文档内容随 output 传递 |
| gate `check_output`（查产物文件） | gate `check_output`（查 `verdict` 字段） | approved 不变；rework 的 false 走 defer 挂账（不 Skipped） |
| `cmd` / `pre_handle` / `post_handle` | 无 | 交接副作用并入工作/评审 agent |
| `parallel` 扇出 | 每角色 N 个工作 agent 扇出 | 语义一致（并发派发） |

---

## 9. 延期项（框架后续版本）

1. **人工评审**：`WaitingHuman` + 外部 decide/resume + durable run（W3a/W3b；无人工等待；`manual` gate 仅静态值）。
2. **每节点最大扇出限制**：无每节点 fan-out 上限；框架 W2b 增加加载期静态 + 运行期每激活上限。
3. **动态 N**：运行期 map/reduce fan-out（当前 N 编译期固定，需 codegen；框架 W4b）。
4. **共享评审队列**：一个评审节点按到达顺序排队处理多份投递，评完一个放行一个（需要节点邮箱/多次激活/按 item 路由，内核级改动；框架 W5/v0.3 候选）。

> **已移出延期项**：图级有限返工 —— v1.1 rework profile 落地（框架 W2a；语义冻结见 DR-005，验收见 §11.2 / spec 07 AC-14）。
>
> 以上四项仍属执行模型扩展，不在本设计稿的 approved-only v1 落地范围内；落地前需另立 DR。

---

## 10. 已知限制与风险

- **无人工审批**：若 SOP 要求人签字（需求评审/发布审批），只能由 agent 代评或拆到图外。
- **返工只覆盖 approved/rework 两态**：`rework` 是正常输出；评审 agent 崩溃、`max_iterations`、`P` 解析异常仍是冷峻 `Failed`，不进入返工。**不重试** `R` 的 LLM 失败、不做跨 run 返工。
- **返工预算硬上限**：`W/R/P` 各 `max_attempts`（缺省 3）→ 最多 2 次 rework；超限 run `Failed(NODE_INTERNAL)`，不静默截断、不无限循环。
- **无框架级 artifact 端口/存储**：文档以数据值传递；如需落盘由工具/工作 agent 写文件，但内核与 gate 不可见（gate 不能查文件）。
- **评审 JSON 脆弱性（关闭 S4）**：评审 `content` 可能被 `max_tokens` 截断（`stop_reason=MaxTokens` 属正常返回）或带 Markdown 围栏；`P` 解析失败 → `Crashed` → run `Failed`。需在 prompt 中约束纯 JSON，并在 `P` 中给出明确错误信息。
- **评审超时预算（关闭 S4）**：评审 agent 多轮 tool loop 与单次 LLM 60s、实例默认 180s 的关系需显式配置；建议评审节点 `node_timeout_ms=300000`、`max_iterations≤20`。
- **LLM 并发无全局限流**：N 个工作 agent / 设计角色会并发发起 LLM 调用；provider 限流需外部控制（减小 N / 串行化依赖）。
- **agent 输出固定四字段**：业务字段必须经 `python_node` 解析，且 `content` 必须可解析 JSON。
- **工具串行**：单响应多 tool_call 串行执行；大量并行工具应拆成图级节点。
- **每 run worker 上限 8**（Python worker）：工作 agent 使用 tools 时与节点执行共享该上限。
- **N 静态**：N 变化必须重新生成图；同一图不能在运行期改变 fan-out 宽度。
- **返工重入的输入继承**：defer 边 attempt 重入的新实例继承上一实例输入 buffer 快照（DR-005 §3.2 行 6/7）；`W/R` 的 `task`/`spec` 由继承保证，`feedback`/`result` 由本次投递覆盖。

---

## 11. v1.1 落地验收（关闭 S3；DR-005 §7）

> §11.1 = approved-only v1（W1，REV-062/065 已交付）；§11.2 = rework v1.1（框架 W2a，spec 07 AC-14）。

### 11.1 approved-only v1（语义不变）

1. `zgraph validate` 通过 §7 codegen 生成的参考图（按 §4.1 的 N_i，含 agent 资产索引）。
2. fake LLM 脚本驱动端到端：设计角色 → N 工作 agent → N 评审 → approved gate → 下游设计角色；断言每个 item 有独立实例与 trace。
3. **产物传播**：断言下游设计角色收到的 `artifact` 是工作 agent 的完整产物（不是评审摘要）。
4. **失败路径**：评审 agent 达到 `max_iterations` → run `Failed`（`HARNESS_MAX_ITERATIONS`）；`P` 收到非 approved / 非法 JSON → `NODE_PYTHON_EXCEPTION` → run `Failed`；断言不出现"gate 拦截后 run Completed(null)"被误当成失败。
5. **等待语义**：下游 `trigger=all` 节点在某依赖 item 未批准时保持 Pending。
6. **规模/成本**：按 §4.1 的 N_i 生成的参考图（60 节点/100 边）的 LLM 调用次数、worker 子进程数、run 时长基线。

### 11.2 rework v1.1（W2a）

1. `zgraph validate` 通过 `--mode rework` 参考图（60 节点/116 边；R15/R13 通过）。
2. **rework→approved e2e**（fake LLM）：run `Completed`；`W/R/P` attempts `0/1`；trace `gate_results` 含 `deferred`/`attempt`/`verdict`（approved 前进边 false 时 `deferred=true`、`verdict="rework"`）；下游拿到**最终** `artifact`（最新一次 `W` 的整包产物）。
3. **超限失败**：评审恒 `rework` → 第 3 次 rework 时 `W` 新实例 attempt=3 超限 → run `Failed(NODE_INTERNAL)`；Crashed 实例 attempt=max；非终态实例 `Cancelled`。
4. **兼容**：approved-only 参考图与 W1 完全一致（`graph_hash` 三基线不变）；v0.1.0 fixture 回归不变。


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
- `(from,to_node)` 与 `(to_node,to_input_port)` 各自唯一 → 每 item 用独立端口/节点（rework 的 `P→W` 用独立 `feedback` 端口，不能复用 `task`）。
- 非 entry 节点入边 ≥1；`trigger=all` 时每个 `required` 端口都要有入边。
- **approved-only v1**：带 gate 的环合法，但**带出口的环当前不可运行**（`RUN_DELIVERY_TO_TERMINAL` / `RUN_DEADLOCK`，ISSUE-062）→ v1 不使用任何环。
- **rework v1.1**：允许「带出口环」，但环上**所有可能重入已 `Completed` 目标实例的边**必须标 `on_gate_false:"defer"`（spec 02 **R15**：defer 边源节点必须位于含 defer 边的环上；自环计入；R13 仍只查无 gate 环）。
- `on_gate_false` 只接受 `"settle"`/`"defer"` 两个字符串；缺省 `"settle"` 且不写回/不参与 `graph_hash`（显式写才参与）。
- `max_attempts` 复用节点 `config.retry.max_attempts`（缺省 3），是自环与 defer 重入的统一上限；超限 `NODE_INTERNAL`。
- `agent_call` 的 `agent_id` 必须在资产索引中存在；`python_node` 只校验 `module.function` 格式，import 在 worker 启动期。
- `end` 必须 ≥1 个 `required` 输入且 `trigger=all`（本设计 3 个）。
- `subgraph` 加载期通过、运行期拒绝（v1.1 不使用）。
