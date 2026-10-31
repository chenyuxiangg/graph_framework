# software_team SOP 设计稿 v1（per-item 评审形态）

> **状态**：设计稿（v1，待独立评审）· 2026-10-31
> **作者**：lead · **分支**：`docs/docs-software-team-sop-v1`
> **关联**：DR-001（技术路线）、spec v1.2 分册 00/01/02/03/04/05/07、`legacy/config/software_team_graph.json`（SOP 语义来源，冻结只读）、ISSUE-011/ISSUE-062
> **范围**：只定义新 DSL（schema 1.0 / v1.2 语义）下的软件团队 SOP 图设计；**不含代码改动**。人工评审、图级有限返工、每节点最大扇出限制、动态 N 与共享评审队列**延期到下一版本**（见 §9）。

---

## 1. 目标与取舍

### 1.1 目标

把 `legacy/config/software_team_graph.json` 表达的软件开发 SOP（角色链 + 产物交接 + 评审闸门）落成一张**当前框架可执行**的 DSL 图：

- 以**设计类角色**为主链关键节点；
- 每个设计角色**派发工作 agent** 产出该阶段的文档/代码/测试产物；
- 每个工作 agent 的产物由**评审 agent 评审**后才允许进入下一阶段；
- 下游节点在自己的 `inputs` 未到齐时**等待**；
- 工作 agent 数量 N 在当前版本**编译期固定**（可用 graph codegen 展开）。

### 1.2 v1 的取舍（与 legacy 语义的差异）

| 主题 | legacy `software_team_graph.json` | v1 设计 |
|---|---|---|
| 调度 | 300s tick 轮询 | 事件驱动；设计角色/工作 agent 均为一次性实例 |
| 自环 | 11 条自环等待产物落盘 | **删除自环**：产物由工作 agent 在实例内同步产出，评审通过即推进 |
| 并行 | 单主线 + 旁路 worker（单 token） | 每个设计角色**扇出 N 个工作 agent**（并发临界区由 worker 上限约束） |
| 评审 | 5 条 manual 边 + 人工 `trigger-edge` | **评审 agent 节点**（LLM），无人工等待 |
| 文件端口 | `iport/oport` 文件存在性 | 端口是类型化数据值；产物内容随 agent output 传递 |
| 返工 | 自环重跑 | 评审 agent 内部 tool loop 有限重做；**图级返工 v0.2** |
| 共享评审队列 | 无 | v1 不做（每个工作 agent 配独立评审节点）；共享队列 v0.2 |

> 依据：用户 2026-10-31 确认的设计方向——设计角色为主链、派发工作 agent、设计角色负责评审；人工评审/图级有限返工/每节点最大扇出限制/动态 N 另起版本。

---

## 2. 核心设计决策

| # | 决策 | 理由 |
|---|---|---|
| D1 | 设计角色 = 主链上的 `agent_call` 节点 | 每阶段一个角色 agent，持有该阶段的目标/约束/prompt；主链即阶段序列 |
| D2 | 工作 agent = **图级节点**（`agent_call`），不是设计 agent 的 tool | 工作 agent 需要独立实例状态、超时、trace、gate 与失败语义；tool 调用只有摘要，无法独立放行 |
| D3 | 每个工作 agent 配一个**独立评审节点**，所有评审节点引用**同一个评审 agent 资产** | 规避共享队列（v1 不支持同一节点多次消费）；同一角色可复用同一 agent_id（R14 只查存在性） |
| D4 | 评审 agent 与工作 agent 之间是**边**（数据流），不是函数调用 | 边的 gate 负责「评审通过才放行」；节点实例可独立重试/超时 |
| D5 | 评审结论经 `python_node` 解析成结构化字段 | `agent_call` 输出固定四字段、`content` 为字符串；gate 只能读 JSON 路径，不能解析字符串 |
| D6 | 下游节点 `trigger: "all"`，等齐本阶段所有已批准产物 | 「inputs 没到就等」= Pending 就绪语义；质量不合格走评审 agent 内部重做 |
| D7 | 评审 agent 内部负责有限重做（≤ `max_iterations`，上限 50） | v1 不做图级回环；评审 agent 可自行调用修复工具重做后再判 |
| D8 | v1 只保留 **approved 路径** | 最终仍不通过 → 评审 agent 报错（`HARNESS_*` / `NODE_*`）→ 实例 Crashed → run Failed；不做 rejected 分支（图级返工 v0.2） |
| D9 | N 编译期固定；N 变化时由 codegen 在 run 前生成图 JSON | 图是静态的；不依赖运行期动态 fan-out |
| D10 | 工作 agent 的产物以**数据值**传递（文档内容/路径清单/结构化结果） | 当前无 artifact 端口；gate 无文件系统访问 |

---

## 3. 模式定义：设计角色 + 工作 agent + per-item 评审

对每个阶段 *i*（设计角色 `D_i`，工作 agent 数 `N_i`）：

```text
D_i ──fan-out──▶ W_{i,1} ──▶ R_{i,1} ──▶ P_{i,1} ──gate(approved)──▶ T_{i,1}
   ├──fan-out──▶ W_{i,2} ──▶ R_{i,2} ──▶ P_{i,2} ──gate(approved)──▶ T_{i,2}
   │                 ⋮            ⋮            ⋮
   ├──fan-out──▶ W_{i,N} ──▶ R_{i,N} ──▶ P_{i,N} ──gate(approved)──▶ T_{i,N}
   └──(上下文)──▶ R_{i,1..N}（每条边一个评审节点，作为 spec 输入）
```

- `D_i`：设计角色 agent；持有本阶段目标，输出下发任务包（整包广播到所有出边）。
- `W_{i,j}`：工作 agent（`agent_call`，或确定性任务用 `python_node`）；接收 `D_i` 的任务包，产出该 item 的成果。
- `R_{i,j}`：评审 agent（`agent_call`，统一 `agent_id: "design_reviewer"`）；两个输入端口：`spec`（来自 `D_i`）、`result`（来自 `W_{i,j}`），`trigger: "all"`。
- `P_{i,j}`：解析节点（`python_node`）；解析 `R_{i,j}` 的 `content` 字符串，输出 `{ "verdict": "approved" | "rework", "comments": ..., "artifact_summary": ... }`。
- `gate`：`check_output { path: "verdict", equals: "approved" }`；只放行已批准的 item。
- `T_{i,j}`：该 item 的下一阶段节点（例如该模块的编码/测试）；`trigger: "all"`，等自己的 `inputs` 到齐。
- 下一设计角色 `D_{i+1}`：等待本阶段所有 `T_{i,j}`（或所有 `P_{i,j}` 批准）后启动；用 `trigger: "all"` + 每 item 一个输入端口表达。

> **评审输入为什么要两条边**：评审既要看「原始设计要求」（`D_i` 输出），也要看「工作产物」（`W` 输出）。两条边到 `R` 的不同端口，`trigger=all` 等齐。

---

## 4. v1 参考实例（角色链与工作 agent 清单）

主链设计角色（线性化，替代 legacy 的两级并行设计分支；并行设计阶段延期）：

```text
planer → archer → mde → te → fo → mto → sto → qa → release → end
```

> 说明：legacy 的 `archer → {mde, te}`、`te → {mte, ste}` 是单 token 模型下的两级分支；v1 将测试设计收敛进 `te` 阶段的工作 agent，主链线性推进。若后续需要并行设计阶段，按 §9 的 v0.2 项处理。

| 阶段 | 设计角色 `D_i` | 工作 agent（N） | 每 item 产物 | 下一阶段 `T` |
|---|---|---|---|---|
| 1 | `planer`（PM） | 需求分析 ×1 | `doc/specification.md` 内容 | 进入 `archer` |
| 2 | `archer`（SE） | 架构设计 ×1、组织设计 ×1、迭代计划 ×1（N=3） | 三份设计文档 | 各自进入下一角色输入 / 评审通过后汇入 `mde` |
| 3 | `mde`（MDE） | 模块设计 ×1、模块迭代计划 ×1（N=2） | 两份模块文档 | 汇入 `te` 与 `fo` 的输入 |
| 4 | `te`（TE） | 总体测试计划 ×1、模块测试用例 ×1、系统测试用例 ×1（N=3） | 测试计划/用例 | 汇入 `fo`/`mto`/`sto` 的输入 |
| 5 | `fo`（FO） | 特性流程 ×1 | `doc/feature_process.md` 内容 | 汇入 `mto` |
| 6 | `mto`（MTO） | 模块测试执行 ×1 | 模块测试报告 | 进入 `sto` |
| 7 | `sto`（STO） | 系统测试执行 ×1 | 系统测试报告 | 进入 `qa` |
| 8 | `qa`（QA） | 验收检查 ×1 | `qa/verdict.md` 内容 | 进入 `release` |
| 9 | `release` | 发布说明/变更日志/用户手册 ×3（N=3） | 三份发布文档 | 进入 `end` |

- 每个工作 agent 都有独立评审节点（同一 `design_reviewer` 资产），评审通过后才把产物交给下游。
- 下游设计角色 `D_{i+1}` 的输入端口按 item 声明（`trigger: "all"`），任一 item 未批准则 `D_{i+1}` 不就绪（Pending；若其输入边全部结算仍缺数据则 `Skipped`，最终 run 结果按内核语义）。
- `end` 节点声明 ≥1 个 `required` 输入（例如 `release` 的三份文档端口），`trigger: "all"`。

> 该表是 codegen 的**输入表**：每个阶段展开 `N_i` 条 item 管线；`D_i` 的 fan-out 边数 = `N_i`（工作）+ `N_i`（评审上下文）+ 下游边。

---

## 5. Gate、评审与失败契约

### 5.1 评审结论

评审 agent 必须在 `content` 中输出可解析 JSON（示例）：

```json
{"verdict": "approved", "comments": "设计覆盖了需求项 R1–R5", "artifact_summary": "..."}
```

- `verdict`：`"approved"`（放行）或 `"rework"`（内部重做）。
- 评审 agent 的 tool loop 内可以调用工作侧的**修复工具**重做（每个 tool 调用 = 一个 Python worker 子进程）；重做次数受 `max_iterations`（默认 10，上限 50）约束。
- 达到上限仍不通过 → 评审 agent 返回 `HARNESS_MAX_ITERATIONS` → 实例 `Crashed` → run `Failed`（冷峻失败；D8）。
- v1 **不定义 rejected 边**：不存在「评审不通过但 run 继续」的路径；该能力随图级返工在 v0.2 引入。

### 5.2 解析节点 `P`

- `type: "python_node"`，输入 = 评审 agent 的整包 output；
- 解析 `content`（JSON 字符串）→ 输出 `{ "verdict": ..., "comments": ..., "artifact_summary": ... }`；
- 解析失败视为节点失败（`PythonException` → `Crashed` → run `Failed`），不做容错猜测。

### 5.3 gate

```json
{
  "function": "check_output",
  "params": { "path": "verdict", "equals": "approved" }
}
```

- 只挂在该 item 的 `P → T` 边；`T` 的 `trigger: "all"` 保证等多 item 输入。
- gate 在发布者（`P`）实例内、边投递前求值；不通过 → 边不投递、下游按 Skipped 级联（v1 的最终不通过路径见 5.1，不会走到这里）。

### 5.4 下游等待

- `T` / `D_{i+1}` 的 `inputs` 全部 `required: true`，`trigger: "all"`；未到齐则 Pending。
- 「inputs 不达标」在 v1 由评审 agent 内部重做消化；**不存在「等一份新的合格输入」的图级机制**（gate=false 会结算并级联 Skipped，不会等待）。

---

## 6. Agent 资产清单（v1 参考）

| 资产 `id` | 类型 | 作用 | 关键配置 |
|---|---|---|---|
| `planer` / `archer` / `mde` / `te` / `fo` / `mto` / `sto` / `qa` / `release` | 设计角色 agent | 持有阶段目标与评审语境 | `role_prompt_file`、`tools`（可选）、`max_iterations` |
| `design_reviewer` | 评审 agent（共享） | 评审单个 item；必要时内部重做 | `role_prompt_file` 要求输出固定 JSON；`tools` 含修复/检查工具；`max_iterations` 覆盖重做预算 |
| `*_work`（每阶段一组） | 工作 agent | 产出该 item 的文档/代码/测试 | `input_mapping` 接收设计任务包；输出结构化结果 |

- 所有节点通过 `agent_id` 引用资产；资产根由 CLI `--assets-dir` / `ZGRAPH_ASSETS` 提供。
- 设计角色与评审 agent 的 prompt 需显式声明**输出契约**（JSON 字段）与**禁止事项**（不得写盘副作用、不得引用不可见输入）。

---

## 7. N 与 codegen 规则

- v1 的图是静态 JSON；`N_i` 在生成图时确定。
- N 变化时用脚本按 §4 表展开节点/端口/边：

```text
for 阶段 i:
  生成 D_i（若 i>1：输入端口 = 上一阶段每个 T_{i-1,j} 的输出）
  for j in 1..N_i:
    生成 W_{i,j}、R_{i,j}、P_{i,j}、T_{i,j}
    边：D_i→W_{i,j}、D_i→R_{i,j}.spec、W_{i,j}→R_{i,j}.result、R_{i,j}→P_{i,j}、P_{i,j}→T_{i,j}（带 approved gate）
  边：T_{i,j}→D_{i+1}（每 item 一个端口）
```

- 节点 id 建议 `<role>_<item>`（如 `archer_arch`、`archer_org`、`archer_plan`），保证 `(from,to_node)` 与 `(to_node,port)` 唯一。
- 端口名必须匹配 Python 标识符（`^[A-Za-z_][A-Za-z0-9_]*$`），不能使用文件路径。

---

## 8. 与 legacy 图对照

| legacy 元素 | v1 对应 | 差异 |
|---|---|---|
| 12 节点 / 25 边 | 9 个设计角色 + 每阶段 N 个工作/评审/解析/下游节点（codegen 后规模为 Σ(4N_i) 量级） | 节点数随 N 展开 |
| 11 条 `*_loop` 自环（gate=false） | 删除 | 工作 agent 同步产出，不再轮询等待 |
| 5 条 `driver:"manual"` 审批边 | 评审 agent + approved gate | 评审由 agent 完成；人工评审 v0.2 |
| `iport/oport` 文件组 | 类型化输入/输出端口（数据值） | 不再依赖磁盘文件；文档内容随 output 传递 |
| gate `check_output`（查产物文件） | gate `check_output`（查 `verdict` 字段） | 从「文件齐否」变为「评审结论」 |
| `cmd` / `pre_handle` / `post_handle` | 无 | 交接副作用并入工作/评审 agent |
| `parallel` 扇出 | 每阶段 N 个工作 agent 扇出 | 语义一致（并发派发） |

---

## 9. 延期项（下一版本 v0.2 候选）

1. **人工评审**：`WaitingHuman` + 外部 decide/resume + durable run（v1 无人工等待；`manual` gate 仅静态值）。
2. **图级有限返工**：可重臂 gate / 带出口 cycle（当前带出口自环会 `RUN_DELIVERY_TO_TERMINAL`，ISSUE-062 关联）；引入后评审不通过可走 rejected 边回到工作节点。
3. **每节点最大扇出限制**：v1 无每节点 fan-out 上限；v0.2 增加图级/运行期上限与校验。
4. **动态 N**：运行期 map/reduce fan-out（当前 N 编译期固定，需 codegen）。
5. **共享评审队列**：一个评审节点按到达顺序排队处理多份投递，评完一个放行一个（需要节点邮箱/多次激活/按 item 路由，内核级改动）。

> 以上五项均属于执行模型扩展，不在本设计稿的 v1 落地范围内；落地前需另立 DR。

---

## 10. 已知限制与风险

- **无人工审批**：若 SOP 要求人签字（需求评审/发布审批），v1 只能由 agent 代评或拆到图外。
- **无图级返工环**：评审不通过只能内部重做；超出预算则 run Failed。
- **无 artifact 端口**：文档以数据值传递；如需落盘由工具写文件，但内核/gate 不可见（gate 不能查文件）。
- **LLM 并发无全局限流**：N 个工作 agent 会并发发起 LLM 调用；provider 限流需外部控制（减小 N / 串行化）。
- **agent 输出固定四字段**：业务字段必须经 `python_node` 解析，且 `content` 必须是可解析 JSON。
- **工具串行**：单响应多 tool_call 串行执行；工作 agent 若需要大量并行工具，应拆成图级节点。
- **每 run worker 上限 8**（Python worker）；工作 agent 若用 tools，注意与节点执行共享该上限。
- **N 静态**：N 变化必须重新生成图；同一图不能在运行期改变 fan-out 宽度。

---

## 11. 后续验收计划（v0.2 落地时使用）

1. `zgraph validate` 通过 codegen 生成的 N=3 示例图（含 agent 资产索引）。
2. fake LLM 脚本驱动端到端：设计角色 → 3 工作 agent → 3 评审 → approved gate → 下一阶段；断言每 item 独立实例与 trace。
3. 失败路径：评审 agent 达到 `max_iterations` → run Failed（`HARNESS_MAX_ITERATIONS`）；解析节点非法 JSON → Crashed。
4. 等待语义：下游 `trigger=all` 节点在某 item 未批准时保持 Pending（或按内核 Skipped 语义断言）。
5. 规模/成本：N=3 时 LLM 调用次数、worker 子进程数、run 时长基线。

---

## 附录 A：图骨架（节选，非可运行 fixture）

```json
{
  "version": "1.0",
  "metadata": { "name": "software_team_v1" },
  "entry_points": ["planer"],
  "nodes": [
    { "id": "planer", "type": "agent_call",
      "inputs": [{ "name": "requirement", "type": "string", "required": true }],
      "config": { "agent_id": "planer" } },
    { "id": "planer_req", "type": "agent_call",
      "inputs": [{ "name": "task", "type": "any", "required": true }],
      "config": { "agent_id": "planer_work", "input_mapping": { "task": "task" } } },
    { "id": "planer_req_review", "type": "agent_call",
      "inputs": [
        { "name": "spec", "type": "any", "required": true },
        { "name": "result", "type": "any", "required": true }
      ],
      "trigger": "all",
      "config": { "agent_id": "design_reviewer" } },
    { "id": "planer_req_parse", "type": "python_node",
      "inputs": [{ "name": "review", "type": "any", "required": true }],
      "config": { "module": "workers.software_team.parse_verdict", "function": "run" } },
    { "id": "archer", "type": "agent_call",
      "inputs": [{ "name": "spec", "type": "any", "required": true }],
      "trigger": "all",
      "config": { "agent_id": "archer" } },
    { "id": "end", "type": "end",
      "inputs": [{ "name": "result", "type": "any", "required": true }],
      "config": {} }
  ],
  "edges": [
    { "from": "planer", "to_node": "planer_req", "to_input_port": "task" },
    { "from": "planer", "to_node": "planer_req_review", "to_input_port": "spec" },
    { "from": "planer_req", "to_node": "planer_req_review", "to_input_port": "result" },
    { "from": "planer_req_review", "to_node": "planer_req_parse", "to_input_port": "review" },
    { "from": "planer_req_parse", "to_node": "archer", "to_input_port": "spec",
      "gate": { "function": "check_output", "params": { "path": "verdict", "equals": "approved" } } }
  ]
}
```

> 该骨架只演示「设计角色 → 工作 agent → 评审 → 解析 → approved gate → 下一设计角色」一条 item 管线；完整图由 §7 规则 codegen 展开。

## 附录 B：关键 DSL 约束（落地时必须满足）

- `version` 必须为 `"1.0"`；顶层键仅 `version/metadata/entry_points/nodes/edges/subgraphs`。
- 节点 id 1–64 字符 `[A-Za-z0-9_.-]`；端口名必须是 Python 标识符。
- `(from,to_node)` 与 `(to_node,to_input_port)` 各自唯一 → 每 item 用独立端口/节点。
- 非 entry 节点入边 ≥1；`trigger=all` 时每个 `required` 端口都要有入边。
- 带 gate 的环合法，但**带出口的环当前不可运行** → v1 不使用任何环。
- `agent_call` 的 `agent_id` 必须在资产索引中存在；`python_node` 只校验 `module.function` 格式，import 在 worker 启动期。
- `end` 必须 ≥1 个 `required` 输入且 `trigger=all`。
- `subgraph` 加载期通过、运行期拒绝（v1 不使用）。
