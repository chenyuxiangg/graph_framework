# DR-005 · W2a0 返工设计冻结：deferrable gate + attempt-per-activation（opt-in）

> **状态**：设计冻结（W2a 实现依据）；评审编号 **REV-067**（评审记录由独立评审人撰写，不在本分支）。
> **日期**：2026-10-31 · **作者**：docs-writer · **分支**：`docs/docs-rework-design`
> **关联**：[`DR-004-v02-scope-and-waves.md`](DR-004-v02-scope-and-waves.md) §3 W2a0 / §5 D1 / §3 W2a DoD / §6、[`REV-063-docs-v02-kickoff.md`](../reviews/REV-063-docs-v02-kickoff.md) C9/C11/S12、[`design/software-team-sop-v1.md`](../design/software-team-sop-v1.md)（本批升级 v1.1）、[`issues.md`](../issues.md) ISSUE-062/100/101、spec v1.2 分册 00/01/02/03/06/07/08
> **前置事实**：`v0.1.0` 已发布（tag `v0.1.0` @ `5d43c58`）；W1 SOP v1 已合并（REV-064/065/066）；DR-004 用户 A1–A7 已确认（`parse_verdict_rework` + defer 边 + 源可重入 + `max_attempts` 缺省 3）。
> **效力**：本 DR 是 W2a（kernel / dsl / trace / py-worker / e2e）与 W2a-docs 的**唯一语义依据**；与既有 spec 冲突处以本 DR + 本批 spec 回写为准，冲突点见 §11。默认行为（无 `on_gate_false` 的图）**不变**。

---

## 0. 摘要（一页）

| # | 冻结内容 | 落点 |
|---|---|---|
| A1 | 边级 `on_gate_false?: "settle" \| "defer"`，缺省 `"settle"`；只接受两字符串，其它值/类型 → `DSL_PARSE_ERROR`（R4） | 02 §2.2.1/§2.2.2/§2.2.4/S-2/S-6 |
| A2 | `CompiledEdge.on_gate_false: OnGateFalse`（`Settle`/`Defer`，`Default=Settle`） | 01 §1.1.1 |
| A3 | 新校验规则 **R15** + 新错误码 **`DSL_DEFER_NOT_REENTRANT`**（DSL 域 17→18 码）；defer 边源节点必须在 `G_all` 的 SCC 中「位于含 defer 边的环上」 | 00 §0.4；02 §2.1.2/§2.4.1/§2.4.2/§2.4.3/§2.7 |
| A4 | `max_attempts` 复用节点 `config.retry.max_attempts`（缺省 3），合法 attempt `0..max-1`，自环与 defer 重入统一生效 | 00 §0.5；02 §2.2.1 |
| B1 | defer 边运行期：gate=false → 记 `DeferredReceipt`（不结算/不 Skipped）；gate=true 且目标已 `Completed` → attempt-per-activation 新实例（**按端口继承 + 清空节点级 settled 集**，见 §3.2 补全）；`attempt >= max` → 新实例 `Crashed(Internal)` → run `Failed(NODE_INTERNAL)` | 01 §1.2.2/§1.4.1；03 §3.4.3 |
| B2 | defer 边目标为 `end` 且当前实例 `Completed` → **不做** attempt+1，`RUN_DELIVERY_TO_TERMINAL` | 03 §3.4.3 |
| B3 | 自环 attempt 语义不变（`source.attempt + 1`），**不得**双重递增；仅 `defer` 自环叠加「按端口继承 + settled 重置」 | 03 §3.4.3 |
| B4 | 静默时 receipt 目标节点当前实例不存在 → 确定性 `RUN_DEADLOCK`（不等 `deadlock_grace`）；目标实例存在且 `Completed` → dormant，不阻塞 | 03 §3.9.3/§3.10 |
| B5 | gate 求值异常仍 `passed=false`（I7），defer 边因此走 deferred 分支（不 fail run） | 00 §0.6 I7；03 §3.4.3 |
| B6 | 无 defer 字段的图（v0.1.0 全部 fixture、W1 SOP v1 图）行为/hash/trace 不变 | §9 |
| C1 | kernel/trace `GateResultRecord` 增 `deferred: bool`、`attempt: u32`（发布者实例）、`verdict: Option<Value>`（仅内置 `check_output`/`compare` 且 `params.path` 为字符串） | 00 §0.3；01 §1.5；03 §3.4.1；04 §4.6.4 |
| C2 | trace schema **v4**：`gate_results` 增 `deferred`/`attempt`/`verdict_json`；`TRACE_SCHEMA_VERSION=4`；新库单事务建 v4；旧库逐版本迁移 v3→v4；payload 同步；`GateResultRow` 同步 | 06 §6.2.2/§6.2.8/§6.2.12/§6.7 |
| C3 | 回放：L1 events 含新字段；L2 outputs 语义不变；golden 覆盖 defer/rework 路径 | 06 §6.10.4/§6.10.7；07 AC-14 |
| D1 | py-worker 新增纯函数 `parse_verdict_rework(inputs, ctx)`；`approved`/`rework` 闭集；非法输入 → `ValueError` → `NODE_PYTHON_EXCEPTION`；v1 不变 | 07 AC-14；本 DR §5 |
| E1–E5 | SOP 设计稿升 **v1.1**：approved-only v1 / rework v1.1 双 profile；环上全部可重入边标 defer；`max_attempts` 预算；codegen `--mode`；延期项同步 | `design/software-team-sop-v1.md` |
| F | 07 新增 **AC-14**（a–h 八条），入 §7.1/§7.2 表与 DoD | 07 |

**一句话**：把「gate=false 一律 settle 并级联 Skipped」扩展为**边级 opt-in**：`defer` 边在 false 时**挂账不结算**，在目标已完成后**按 attempt 重入**（新实例沿用旧输入），并由**静态可重入校验（R15）+ 运行期 dormant/死锁判定**共同保证有限、确定、可回放。

---

## 1. 背景、目标与非目标

### 1.1 背景

- v0.1.0 的 gate 语义：`passed=false` → 结算入边（无数据）+ `maybe_skip` 级联；因此**门控不能表达"稍后重试"**，只表达"永久拦截"。
- SOP v1（approved-only）把"评审不通过"表达成 `parse_verdict` 抛错 → `NODE_PYTHON_EXCEPTION` → run `Failed`；评审"返工"无法在框架内闭环。
- 带出口环（回边）在 v0.1.0 有两种失效模式：`RUN_DELIVERY_TO_TERMINAL`（目标实例已 Completed）与 `RUN_DEADLOCK`（`valid_cycle.json`，ISSUE-062）。
- DR-004 §5 D1 已定：**opt-in deferrable gate + P v2 + attempt-per-activation**；W1 已落地 approved-only 的 py-worker/e2e，W2a0 负责把语义冻结成可实现、可验收的规格。

### 1.2 目标

1. **SOP v1 rework 端到端可执行**：rework → approved（run `Completed`、`W/R/P` attempts 0/1、下游拿最终 artifact）。
2. **默认语义零变化**：无 `on_gate_false` 的图行为、`graph_hash`、trace 与 v0.1.0 完全一致（B6）。
3. **有限且可判定**：重入次数由 `max_attempts` 兜底；dormant/死锁判定是**确定性**的，不依赖 `deadlock_grace` 的宽限碰运气。
4. **可观测**：trace 记录 `deferred`/`attempt`/`verdict`；schema v4 forward-only 迁移；L1/L2 回放可用。
5. **静态可校验**：非法 defer（源不在含 defer 的环上）在 `zgraph validate` 阶段即拒绝（R15），不做运行期猜测。

### 1.3 非目标

- 不做通用「任意节点图级重试 / retry 编排 DSL」；重试语义**只**由 defer 边 + `max_attempts` 表达。
- 不做人工评审（W3a）、扇出上限（W2b）、动态 N（W4b）、共享评审队列（W5）。
- 不新增运行期错误码；超限复用 `NODE_INTERNAL`（与自环超限同风格）。
- 不做历史 trace 重写（forward-only 迁移，D5）。
- 不改 `python/workers/software_team/parse_verdict.py` 的 v1 `run`/`parse_verdict` 行为（D1 opt-in）。

### 1.4 术语

| 术语 | 定义 |
|---|---|
| settle 边 | `on_gate_false` 缺省值；gate=false 时按 v0.1.0 语义结算入边 + `maybe_skip` 级联 |
| defer 边 | `on_gate_false="defer"`；gate=false 时不结算、不 Skipped，只更新 `DeferredReceipt` |
| DeferredReceipt | 运行期「某条 defer 边最近一次 deferred 决策」的挂账记录（`EdgeIdx` 为键），见 §3.1 |
| attempt-per-activation | defer 边放行且目标当前实例已 `Completed` 时，新建 `attempt+1` 实例（不报 `RUN_DELIVERY_TO_TERMINAL`） |
| dormant | 静默时 receipt 目标实例存在且已 `Completed`（终态不可能再变），不阻塞 quiescence |
| 可重入环 | `G_all`（全部边，含 gate 边/自环）中经过 defer 边源节点、且含至少一条 defer 边的环；R15 判定 |
| 发布者实例 | 产生 `GateResult` 的源节点实例；`GateResultRecord.attempt` 取该实例的 attempt |

---

## 2. DSL 冻结（A1–A4）

### 2.1 新边字段（A1）

| 项 | 冻结值 |
|---|---|
| 字段 | `GraphEdge.on_gate_false`（边级，`GraphEdge` 顶层键） |
| 类型 | `"settle" \| "defer"`（**只**这两个字符串字面量） |
| 缺省 | `"settle"`（v0.1.0 语义） |
| 非法值 | 其它字符串 / 非字符串（数字、bool、null、对象、数组）→ R4 `DSL_PARSE_ERROR`（类型桶；`expected: ["settle","defer"]`、`actual`=原值） |
| 缺省是否写回 DSL | **否**（不补齐、不参与 `graph_hash`，见 §2.3） |
| 出现位置 | 主图与子图 `edges` 的同名键（子图递归同规则） |
| 保留字/未知键 | 不存在 `on_gate_false` 之外的保留字；未列键仍按 §2.2.4 白名单 → `DSL_PARSE_ERROR` |

- **只看取值是否合法，不改变 R13**：R13 仍只查「无 gate 的环」；defer 不豁免环检测（见 §2.5）。
- 示例（合法）：
  ```json
  { "from": "parse", "to_node": "work", "to_input_port": "feedback",
    "gate": { "function": "check_output", "params": { "path": "verdict", "equals": "rework" } },
    "on_gate_false": "defer" }
  ```

### 2.2 键白名单与 serde 对照（02 §2.2.4 / S-2 / S-6）

| 位置 | 变更 |
|---|---|
| §2.2.4 `GraphEdge` 允许键 | 由 `from, to_node, to_input_port, gate`（+ 保留字段 `from_output_port`）→ **新增 `on_gate_false`** |
| `GraphEdge` 必填键 | 不变：`from, to_node, to_input_port` |
| S-2 未知字段预扫描 | `on_gate_false` 进入白名单；**不**是保留字段（出现即生效，不是报 `DSL_UNSUPPORTED_FIELD`） |
| S-6 缺省值记录 | DSL 侧用 `Option<OnGateFalse>`（或等价「作者是否写了」标记）区分「缺省」与「显式 settle」；编译期语义默认值补到 `CompiledEdge`，**不写回 DSL** |
| 类型校验时机 | 预扫描之后的 R4 结构校验（与 `trigger`/`gate` 同桶）；typed 反序列化失败一律 `DSL_PARSE_ERROR` |

### 2.3 `graph_hash` 兼容（02 §2.8.2/§2.8.3）

- **缺省不投影**：作者未写 `on_gate_false` → canonical 投影中边对象**无该键** → v0.1.0 全部图的 hash 逐字节不变。
- **显式写则投影**：作者写了（含显式 `"settle"`）→ 边对象含 `on_gate_false` → 参与 `graph_hash`（与 `trigger`/`timeout_ms`/`retry` 的「不补齐默认值」规则一致，§2.8.3 已知限制同理：显式 `"settle"` 与缺省语义等价但 hash 不同）。
- 计算前置：`graph_hash` 仍在 `R1–R13` 与 **R15** 全部通过后计算（`R14` 失败不产出 hash，不变）。
- 旧 hash golden（v0.1.0 fixture、W1 SOP v1 full/smoke/exhaust）**必须逐字节不变**（AC-14 d）。

### 2.4 编译产物（A2）

01 §1.1.1 增字段；00 §0.3 增权威枚举：

```rust
/// 边级「门控失败策略」（DSL `on_gate_false`；缺省 Settle = v0.1.0 语义）。
pub enum OnGateFalse { Settle, Defer }   // Default = Settle

pub struct CompiledEdge {
    pub index: EdgeIdx,
    pub from: NodeId,
    pub to_node: NodeId,
    pub to_port: PortName,
    pub gate: Option<GateRef>,
    pub on_gate_false: OnGateFalse,   // W2a0 新增；编译期把缺省补齐为 Settle
}
```

- DSL 侧 `GraphEdge.on_gate_false: Option<OnGateFalse>`（记录作者是否写）；`CompiledEdge` 恒有值。
- 子图递归编译时同样映射；子图执行仍在 v1.1（本次不启用）。

### 2.5 新校验规则 R15（A3）

#### 2.5.1 定位与执行顺序

- R15 是**纯函数**（不触碰外部：无 Python、无资产目录），因此属于 **L2（graph 阶段）**，与 R13 同阶段；`zgraph validate --structure-only` **必须执行** R15。
- 编号沿用「规则编号即契约」的既有冻结：R15 追加在 R14 之后，但**执行顺序冻结为 `R1 → R13 → R15 → R14`**（L2 内 R15 紧随 R13；R14 是 L3 外部引用解析，必须在所有纯规则之后）。此顺序与 02 §2.1.3「规则编号即执行顺序」的字面表述存在例外，已在 02 §2.1.2/§2.1.3 写明（见 §11 冲突 C-4）。

#### 2.5.2 判定算法

对**每一条** `on_gate_false == Defer` 的边 `e = (u → v)`（主图与每个子图各自命名空间）：

```text
G_all = (V_all, E_all)      # 顶点 = 命名空间内全部节点；边 = 全部边（含 gate 边、含自环）
S = SCC(u)                  # G_all 上的强连通分量
pass(e) =
    存在 defer 自环 (u → u)                       # 自环记入 G_all
    或 (|S| >= 2 且 存在 defer 边 (a → b)，a ∈ S 且 b ∈ S)
fail(e) → DSL_DEFER_NOT_REENTRANT，location = {edge_index: e.index, node_id: u}
```

- **等价实现**（与上面显式判定等价，允许两者任一）：
  - `|SCC(u)| >= 2` 且 `SCC(u)` 内存在 `defer` 边（两端点同属该 SCC）；或
  - 存在 `defer` 自环 `u → u`。
- 复杂度上界：每个命名空间一次 **Tarjan/Kosaraju**（`O(V+E)`）；defer 边逐条查 SCC + 一次「SCC 内是否有 defer 边」集合查询（`O(E)`）。
- **环的语义口径**：本规则以**闭迹（closed walk，允许重复顶点）**为权威口径；按「简单环」口径**更严**——E-1 的构造满足本判定、但不存在「过 u 且含该 defer 边」的简单环，本规则按冻结判定**通过**（属放宽例，不是反例；见 §11 E-1）。实现以 §2.5.2 的等价判定为准，**不得**自行收紧或放宽。
- 定位与 message（英文稳定模板，02 §2.4.2）：
  - message：`defer edge source '{u}' is not re-entrant: must lie on a cycle containing a defer edge`
  - location：`{ "edge_index": <e.index>, "node_id": "<u>" }`
  - expected：`"re-entrant defer source (cycle through source containing a defer edge, or defer self-loop)"`
  - actual：`{ "node_id": "<u>", "scc_size": <n>, "defer_edges": [<indices>] }`（`defer_edges` = 该 SCC 内 defer 边索引，升序）
- **与 R13 独立**：R13 保持原样（`G'` 只含无 gate 边，自环计入）；R15 用 `G_all`。二者可同时报错时按执行顺序（R13 先）只报 R13（fail-fast）。
- **子图**：R6 的递归规则集由「R4/R5/R8–R13」扩为「R4/R5/R8–R13、R15」（子图命名空间内独立判定；`R7` 仍不适用）。**错误码收敛（N-4）**：子图内 R15 违规由 R6 包装为 `DSL_SUBGRAPH_INVALID`（message `subgraph '<id>': ...`，`location` 保留内部 `node_id`/`edge_index`），不把 `DSL_DEFER_NOT_REENTRANT` 直接抛出；与现有子图 R13 收敛一致。

#### 2.5.3 正例（通过）

| # | 图骨架 | 说明 |
|---|---|---|
| P-1 | `a --gate--> b`, `b --gate--> a`，两条边均 `defer` | `SCC = {a,b}`，含 defer 边 → 通过 |
| P-2 | 带 gate 的 `defer` 自环 `a --gate(defer)--> a` | 自环计入 `G_all`；R13 因有 gate 放行；R15 命中 defer 自环 |
| P-3 | SOP v1.1：`P --gate(approved,defer)--> D`、`P --gate(rework,defer)--> W`、`W--defer-->R`、`W--defer-->P.artifact`、`R--defer-->P` | `SCC = {P,W,R}` 且 SCC 内含 defer 边；每个 defer 源（P/W/R）都在该 SCC 上 → 全部通过 |
| P-4 | `a --defer(gate)--> b`，`b --gate--> a`（回边非 defer） | `SCC={a,b}` 内含 defer 边 `a→b` → 通过（环内只需**至少一条** defer 边） |

#### 2.5.4 反例（报错）

| # | 图骨架 | 期望 |
|---|---|---|
| N-1 | `a --defer(gate)--> b`，b 无出边 | `SCC(a)={a}`、无 defer 自环 → `DSL_DEFER_NOT_REENTRANT`（`edge_index`=该边，`node_id=a`） |
| N-2 | `a --defer(gate)--> b`；`b --gate--> c`；`c --gate--> b` | `SCC(a)={a}`（b/c 自环但与 a 不连通）→ 报错 |
| N-4 | 无 gate 的 `defer` 自环 `a → a` | **先**被 R13 `DSL_CYCLE_DETECTED` 拦截（R13 先于 R15）；R15 不报 |
| N-5 | 无 gate 的 `a→b→a` 环 + 某条 `defer` 前进边指向环外 | R13 先报 `DSL_CYCLE_DETECTED`（defer 不豁免 R13） |

> W2a 必须在 `crates/zgraph-dsl/tests/fixtures.rs` 或等价夹具表补 P-1/P-2/N-1/N-2 四条（`validate` 正反例），并在 07 AC-14 引用。

### 2.6 `max_attempts`（A4，复用而非新字段）

| 项 | 冻结值 |
|---|---|
| 来源 | 节点 `config.retry.max_attempts`（**不新增** DSL 字段） |
| 缺省 | `DEFAULT_MAX_ATTEMPTS = 3`（00 §0.5） |
| 编译产物 | `CompiledNode.max_attempts`（既有字段，语义扩为「自环与 defer 重入统一上限」） |
| 合法 attempt | `0 ..= max_attempts - 1`（`max_attempts=3` → 0/1/2 三次实例；第 4 次即 `attempt=3 >= 3` 触发超限） |
| 超限 | 新实例 `Crashed`（`NodeError::Internal`）+ run `Failed(NODE_INTERNAL)`；message 与自环超限同风格（见 §3.5） |
| 自环 | 现行为不变：`attempt = source.attempt + 1` |
| defer 非自环重入 | `attempt = 目标当前实例.attempt + 1` |

> SOP rework 预算：`max_attempts=3` → `W/R/P` 各最多 3 次实例（0/1/2）→ 最多 **2 次 rework**；第 3 次 rework 触发超限失败（AC-14 e）。

### 2.7 L2/L3 流水线更新（02 §2.1.2）

| 阶段 | 规则（W2a0 后） |
|---|---|
| L1 parse | R1–R4 |
| L2 graph | R5–R13、**R15** |
| L3 resolve | R14 |
| L4 compile | 无规则；`graph_hash`（前置：R1–R13、R15 通过） |

- `--structure-only` = L1+L2（R1–R13、R15），输出 `(external references not verified)`。
- 默认 `validate`/`run` = L1–L3（R1–R13、R15、R14）。

---

## 3. 运行期冻结（B1–B6，kernel）

### 3.1 运行期结构：`DeferredReceipt`

01 §1.4.1 新增（01 §1.6 `RunState` 持有）：

```rust
/// 一条 defer 边最近一次「deferred 决策」的挂账记录（W2a0）。
/// key = EdgeIdx；不因后续放行删除（dormant 判定只看目标节点当前实例状态）。
pub struct DeferredReceipt {
    pub edge_index: EdgeIdx,
    pub source_instance: InstanceId,  // 做出 deferred 决策的发布者实例
    pub source_attempt: Attempt,      // 该实例 attempt（审计/排障）
    pub target_node: NodeId,          // = CompiledEdge.to_node
    pub updated_at: String,          // 实现口径：RFC3339 毫秒 Z（与既有 ts/at 同口径）
}
```

- `RunState.deferred_receipts: BTreeMap<EdgeIdx, DeferredReceipt>`（**BTreeMap**：quiescence 判定与错误 message 需要确定性的边序；同一 `EdgeIdx` 重复 deferred → 覆盖旧值、刷新 `updated_at`）。**`updated_at` 实现口径 = `String`（RFC3339 毫秒 `Z`）**，与 kernel 既有 `ts`/`at` 记录同口径；spec 01 同步。
- 仅 **defer 边** 且 **gate=false** 时写入；settle 边的 gate=false 不写 receipt（保持 v0.1.0）。
- receipt 不参与 `graph_hash`、不进入 `Delivery`；只进运行期状态（trace 经 `GateResultRecord.deferred=true` 可观测）。

### 3.2 决策矩阵（B1/B2/B3）

对一条边 `e = (u→v)`、发布者实例 `src`（`src.node_id = u`）、本次 gate 求值结果 `passed`：

| # | `e.on_gate_false` | `passed` | 目标 `v` 现状 | 动作 |
|---|---|---|---|---|
| 1 | `Settle` | false | 任意 | **现行**：结算 `v` 入边 + `maybe_skip` 级联（不产生/不更新 receipt） |
| 2 | `Defer` | false | 任意 | `GateResult{passed:false, deferred:true}` → 更新 `deferred_receipts[e.index]`；**不结算 `v` 入边、不 `maybe_skip`** |
| 3 | `Defer` | true | `v` 无实例 | **现行**：惰性创建 `v` attempt0（Pending）→ 投递 |
| 4 | `Defer` | true | `v` 非终态（Pending/Ready/Running） | **现行**：投递给 `v` 当前实例（不新建 attempt） |
| 5 | `Defer` | true | `v` 当前实例 `Completed`，`v.type == end` | **B2 特例**：不 attempt+1；`fail_run(RUN_DELIVERY_TO_TERMINAL)`（end 是 run 级聚合，重入未定义） |
| 6 | `Defer` | true | `v` 当前实例 `Completed`，`u == v`（自环） | **B3**：按自环语义 `attempt = src.attempt + 1`；`>= max` → 新实例 Crashed(Internal) + `NODE_INTERNAL`；否则新建 Pending 实例 + **按端口继承 + `settled_by_node[v]` 重置**（§3.2 行 6/7 补全） |
| 7 | `Defer` | true | `v` 当前实例 `Completed`，`u != v` | **B1**：`attempt = target_current.attempt + 1`；`>= max` → 新实例 Crashed(Internal) + `NODE_INTERNAL`；否则新建 Pending 实例 + **按端口继承 + `settled_by_node[v]` 重置**，再应用本次投递 |
| 8 | **`Settle`** | true | 目标当前实例 `Completed` | **现行失败路径**：`RUN_DELIVERY_TO_TERMINAL`（**不** attempt+1；attempt-per-activation **只对 `Defer` 边**）；resolve 失败 → 不产生 GateResult |
| 9 | `Defer` | true | `v` 当前实例终态非 `Completed` | **现行**失败路径：`RUN_DELIVERY_TO_TERMINAL`（冷峻失败已发生，防御性） |
| 10 | 无 gate（`gate: None`） | true | 同 3–9 | 同 3–9（`deferred` 只在 false 时可能为 true；无 gate 恒 true，故走 attempt-per-activation 判断） |

> **attempt 计算（C2 明确）**：`Defer` **自环** → `attempt = src.attempt + 1`（保持 v0.1.0 自环语义，不双重递增）；`Defer` **非自环** → `attempt = target_current.attempt + 1`；**`Settle` 边永不进入 attempt-per-activation**（行 8 = 现行 `RUN_DELIVERY_TO_TERMINAL`）。
>
> 行 10 是 SOP v1.1 的关键：`W→R`、`W→P.artifact`、`R→P` 无 gate，但标 `defer` 的作用就是**允许目标已 Completed 时按 attempt 重入**。
>
> **行 6/7 的「按端口继承 + settled 重置」是 B1 的必要补全（Lead 2026-10-31 裁决；见 §11 C-1）**。原因有二：
> - **stale-input 提前就绪（缺陷 A）**：SOP 的 `P` 有两个 defer 输入（`artifact` 来自 W、`review` 来自 R）。若新 attempt 继承**全部** arrived，P 的新实例在 `W→P.artifact` 投递后就会满足 `trigger=all`（旧 `review` 仍 arrived）→ 用**上一代评审结论**提前执行，rework 语义随机化。
> - **node 级 settled 集误 Skipped（缺陷 B）**：`settled_by_node` 是节点级集合，上一代入边已全部结算；新 attempt 创建后若走 `maybe_skip_after_settle`，`all_incoming_settled` 为真且新实例未 ready → 立即 `Skipped` 级联，rework 直接断掉。
>
> **冻结规则（两条，必须同时实现）**：
> 1. **按端口继承（per-port inheritance）**：创建 defer 重入实例时，对目标节点 `v` 的每个声明输入端口 `p`，查 `EdgeIndex.incoming[(v,p)]`：
>    - 该端口**有入边且 `edge.on_gate_false != Defer`** → 继承 `values[p]` 与 `arrived` 中的 `p`（深拷贝）；结算状态**统一按节点级 `settled_by_node`** 处理（不由实例级集合参与）;
>    - 该端口**入边为 defer**（或没有入边，如可选端口未接）→ **不继承**（值和 arrived 都不继承），必须由当前这一代重新投递才能满足 required。
>    - `required` 集合由编译期节点重建；`gate_results` 不继承。若实现保留实例级 `settled_sources`，仅作审计/排障、不参与 skip/就绪判定（N-9 口径）。
> 2. **重入实例创建时清空 `settled_by_node[target_node]`**：让新一代重新结算自己的入边，避免上一代节点级 settled 集把新 Pending 实例误判 `Skipped`。某些 settle 入边（来自已完成的外部节点）不会重投 → 该节点可能永远「非全结算」，这不影响就绪（就绪只看 buffer arrived），而 `maybe_skip` 需要「全结算」才触发，反而正确避免误级联。**v0.1.0 无重入实例 → 行为不变**（B6）。
>
> **SOP 落地校验**：`W` 的 `task`（`D→W`，settle）继承、`feedback`（`P→W`，defer）重投；`R` 的 `spec`（`D→R`，settle）继承、`result`（`W→R`，defer）重投；`P` 的 `review`/`artifact` 都 defer → **都等新一代投递**（`W→P.artifact` 与 `R→P.review` 两条都到位才 ready，杜绝旧评审提前执行）。
> **自环（行 6）同样适用**：自环端口若是 defer 也重投；源实例即目标实例，`settled_by_node[v]` 同样重置。

### 3.3 投递应用算法（03 §3.4.3 的权威伪代码）

```text
apply_completed(src, output, evaluations):                 # 执行任务回传后、事件循环内；顺序与 03 §3.4.2 逐行同构
  transition(src, Completed); set output
  for eval in evaluations.sorted_by_edge_index():
      if run.status.is_terminal(): return                  # 前置短路：fail_run 后不再处理后续边
      edge = graph.edges[eval.edge_index]
      deferred = (!eval.passed && edge.on_gate_false == Defer)
      record  = GateResultRecord { ..., passed: eval.passed, error: eval.error,
                                   deferred, attempt: src.attempt, verdict: eval.verdict }

      # ---- 分支 1：gate 不通过（先 emit，再结算/挂账）----
      if !eval.passed:
          emit(record)                                     # 不可丢（先于结算）
          if edge.on_gate_false == Defer:
              deferred_receipts[edge.index] = DeferredReceipt { edge.index, src.id, src.attempt,
                                                               edge.to_node, now() }
              continue                                     # 不结算、不 maybe_skip
          settle_edge(edge.to_node, edge.index)
          maybe_skip_after_settle(edge.to_node)
          continue

      # ---- 分支 2：gate 通过（先 resolve，成功后才 emit）----
      target = resolve_target(src, edge)                   # 内部先 resolve_defer_reentry(edge)：
                                                           #   NotApplicable → 现行 resolve（3/4/8；行 9 走 resolve_defer_reentry 的 Failed）
                                                           #   Created(inst) → 该实例（成功 reentry 或超限 Crashed）
                                                           #   Failed        → None（RUN_DELIVERY_TO_TERMINAL，含 end 特例）
      if target is None: return                            # resolve 失败：**不产生 GateResult**
      emit(record)                                         # 仅 resolve 成功（含超限返回的 Crashed 实例）后 emit；超限时 sink 已关闭 → no-op（不入库）
      buffer_write(target, edge.to_port, output)
      settle_edge(edge.to_node, edge.index)
      maybe_make_ready(target)
      maybe_skip_after_settle(edge.to_node)                # 现行「结算后统一复核」

resolve_defer_reentry(edge, src) -> NotApplicable | Created(instance) | Failed:
  if edge.on_gate_false != Defer: return NotApplicable     # C2：只对 defer 边
  if edge.from != edge.to_node:                            # 非自环
      current = node_current.get(edge.to_node)
      if current is None: return NotApplicable             # 首次投递（现行）
      if !instances[current].status.is_terminal(): return NotApplicable   # 现实例（现行）
      if instances[current].status != Completed: return Failed           # 防御性：不 emit
      if node_type(edge.to_node) == End: return Failed     # end 特例：不 emit
      attempt = instances[current].attempt + 1
      if attempt >= node(edge.to_node).max_attempts:
          created = create_instance(edge.to_node, attempt, crash = Internal{...})
          fail_run(NODE_INTERNAL)                          # 先 fail_run（与现行自环一致；emitter.finish → sink 关闭）
          return Created(created)                          # 外层照常 emit（no-op、不入库）→ buffer/结算 → 下一轮终态早退
      return Created(create_reentry_instance(edge.to_node, attempt, current))
  # 自环 defer：attempt 语义保持现行为（source.attempt + 1）
  attempt = instances[src].attempt + 1
  if attempt >= node(edge.to_node).max_attempts:
      created = create_instance(edge.to_node, attempt, crash = Internal{...})
      fail_run(NODE_INTERNAL)                              # emitter.finish → sink 关闭
      return Created(created)                              # 外层 emit 为 no-op（不入库）
  return Created(create_reentry_instance(edge.to_node, attempt, src))

create_reentry_instance(target, attempt, prev):
  clear_settled_by_node(target)                            # 缺陷 B：节点级结算集重置
  for p in declared_inputs(target):                        # 缺陷 A：per-port 继承
      e = incoming[(target, p)]
      if e.exists and e.on_gate_false != Defer: inherit values[p] + arrived[p]   # 结算按 settled_by_node
      else: skip                                            # defer 端口等待新一代投递
```

- **确定性**：evaluations 按 `edge_index` 升序；receipt 用 `BTreeMap`；新实例 attempt 只依赖目标当前实例 attempt（自环依赖源实例 attempt）。
- **失败顺序（C1 冻结，与现行 kernel/03 §3.4.2 逐行同构）**：
  - `RUN_DELIVERY_TO_TERMINAL`（含 `end` 特例与防御性终态）：`resolve` 失败返回 `None` → **不产生 GateResult**（与现行 settle 路径一致，REV-026/ISSUE-061）。
  - **超限（自环与 defer 重入）**：`resolve` 内先 **create Crashed 实例 + `fail_run(NODE_INTERNAL)`**（`fail_run` 先 `emitter.finish` 关闭 sink），返回该实例 → 外层照常 **emit（no-op，不入库）** → 写 buffer/结算 → 循环下一轮因 `is_terminal` 早退。**该条边的 GateResult 不可观测、不要求入库**；trace 证据 = Crashed 实例（attempt=max）+ 之前已产生的 GateResult 行 + `RunFinished{Failed}`/`NODE_INTERNAL`。**不得**改成 create→emit→fail（会改变 v0.1.0 自环超限的事件顺序，违反 B6/AC-14 d）。
- **trace 顺序**：一条边至多一条 `GateResult`；resolve 失败不产生投递/记录；**超限路径在 `fail_run`（sink 关闭）之后才 emit，因此该次 GateResult 不入库**（同现行自环）；e2e 只断言 Crashed 实例 + 之前 GateResult + `RunFinished{Failed}`。
- **I7 不变**：gate 异常/panic 仍是 `passed=false`（B5），defer 边因此走 deferred 分支；**不** fail run、不改 `error` 语义。

### 3.4 receipt 生命周期与 dormant/死锁（B4）

- receipt **只在** defer 边 gate=false 时写入/覆盖；**不因**后续 gate=true 放行而删除（冻结为「最近一次 deferred 决策」）。dormant 判定只看目标节点**当前实例**状态，因此历史 receipt 不会造成误判。
- 收尾判定（03 §3.10 追加在 `is_quiescent` 为真之后、`complete_run` 之前）：

```text
if is_quiescent(run):
    blocked = [r for r in deferred_receipts.values()
               if node_current.get(r.target_node) is None]        # 目标节点从未创建实例
    if !blocked.is_empty():
        fail_run(RUN_DEADLOCK,
                 message = "deferred receipt(s) blocked on edge(s) [..]: target node(s) never instantiated")
        # 确定性：静默后不可能再产生新事件，无需 deadlock_grace
    else:
        complete_run()                                                 # dormant 或普通静默
```

- **判定表**：

| receipt 目标节点当前实例 | 判定 |
|---|---|
| 不存在（从未实例化） | **blocking** → `RUN_DEADLOCK`（不依赖 `deadlock_grace`；理由：静默后不可能再有新事件） |
| 存在且 `Completed` | **dormant**：不阻塞 → 正常 `Completed` |
| 存在且非终态 | 不可能处于静默（有 Pending/Ready/Running 实例）；若因 pause 等边界不静默，走现行机制 |
| 存在且终态非 `Completed`（`Crashed`/`TimedOut`/`Skipped`） | `Crashed`/`TimedOut` ⇒ run 已冷峻 Failed（不可达）；`Skipped` ⇒ 视为 **dormant**（终态不可能再投递；若后续放行则按决策矩阵**行 9** `RUN_DELIVERY_TO_TERMINAL`）（§11 边界 E-2） |

- **误判防护**：rework 进行中有活跃实例 → 不静默；approved 路径回边 false 的 receipt 目标已 `Completed` → dormant；SOP approved-only 图（无 defer）无 receipt → 完全不变。
- 看门狗仍保留既有「无活跃实例 + Pending 存在 + 超过 `deadlock_grace`」的检测（03 §3.9.3）；B4 的确定性死锁是**额外**、更早的判据。
- 暂停期间不判定（03 §3.10：静默判定只在 Running 生效）。

### 3.5 超限失败的 message（B1）

- 非自环 defer 重入超限：
  `"defer re-entry of node '{v}' exceeded max_attempts ({n}) on edge {i} (attempt {attempt})"`
- 自环（现行为，保持同风格）：
  `"self-loop of node '{v}' exceeded max_attempts ({n})"` / `"… on edge {i}"`
- 都是 `NodeError::Internal` → `NODE_INTERNAL`，**不新增运行期错误码**；Crashed 实例的 `attempt = max`（如 `max=3` → attempt 3）。
- **不产生该条边的 `GateResult` 入库（Lead 2026-10-31 裁决）**：`fail_run` 先 `emitter.finish` 关闭 sink，外层 emit 是 no-op；W2a e2e **不得**断言「超限那次的 GateResult」。trace 证据 = Crashed 实例（attempt=max）+ 之前已产生的 GateResult 行 + `RunFinished{Failed}`/`NODE_INTERNAL`。

### 3.6 兼容性（B6）

| 场景 | 结果 |
|---|---|
| v0.1.0 全部 `tests/fixtures/graphs/*.json`（无 `on_gate_false` 键） | `graph_hash`、validate 结果、run 行为不变 |
| W1 SOP v1 图（`parse_verdict` + approved gate，无 defer） | 不变（`REWORK` 仍抛错 Failed） |
| v0.1.0 trace v3 库 | 只读 `trace`/`replay` 拒绝（schema < 4，D5）；`run` 打开时迁移 v3→v4（§4） |
| 旧 trace 回放 | GateResult 行取列默认 `deferred=0`/`attempt=0`/`verdict=NULL` |

---

## 4. trace 冻结（C1–C3）

### 4.1 `GateResultRecord`（C1，kernel + trace 同一形状）

```rust
pub struct GateResultRecord {
    pub run_id: RunId,
    pub instance_id: InstanceId,   // 发布者实例
    pub node_id: NodeId,           // 发布者节点
    pub edge_index: EdgeIdx,
    pub to_node: NodeId,
    pub to_port: PortName,
    pub gate_fn: Option<String>,
    pub passed: bool,
    pub error: Option<String>,
    pub deferred: bool,            // W2a0：本次求值是否触发 defer（= !passed && 边为 Defer）
    pub attempt: u32,              // W2a0：发布者实例 attempt
    pub verdict: Option<Value>,    // W2a0：见下
    pub ts: DateTime<Utc>,
}
```

- `deferred` 语义：**本次求值的结果**，不是边的静态属性（边的静态属性可从 `edge_index` 查 `CompiledEdge.on_gate_false`，不重复入库）。
- `attempt`：发布者实例 attempt（u32；trace 存 `INTEGER`）。
- `verdict` 取值规则（仅以下两类，其余一律 `None`）：
  1. 内置 `check_output`：`params.path` 为字符串 → 取 `output` 在该点分路径上的值（**路径存在时** `Some(value)`，不存在/中间非 object → `None`）；
  2. 内置 `compare`：同上（`params.path` 为字符串，取路径值）；
  3. `manual`、无 gate 边、`params.path` 非字符串、gate 异常 → `None`。
- `verdict` 是**原始路径值**（不保证等于 `passed` 判据；例如 `in`/`exists`/`compare` 的比较目标不在其中），只用于排障/回放辅助，**不得**作为路由判据（路由仍只看 `passed`）。
- 求值位置不变：执行任务内 off-loop；`GateEvaluation` 同步携带 `verdict: Option<Value>`（01 §1.7），事件循环不二次求值。
- 04 §4.6.4 原「不记录求值耗时、不得改 `GateResultRecord`」措辞改为「本批仅新增三个已冻结字段；仍不记录耗时；后续新增须升 spec 版本并登记」。

### 4.2 trace schema v4（C2，06 §6.2.8/§6.2.2）

```sql
CREATE TABLE IF NOT EXISTS gate_results (
  gate_result_id INTEGER PRIMARY KEY AUTOINCREMENT,
  seq            INTEGER NOT NULL UNIQUE REFERENCES events(seq) ON DELETE CASCADE,
  run_id         TEXT NOT NULL,
  instance_id    TEXT NOT NULL,             -- 发布者实例
  node_id        TEXT NOT NULL,             -- 发布者节点
  edge_index     INTEGER NOT NULL,
  to_node        TEXT NOT NULL,
  to_port        TEXT NOT NULL,
  gate_fn        TEXT,
  passed         INTEGER NOT NULL CHECK (passed IN (0,1)),
  error          TEXT,
  deferred       INTEGER NOT NULL DEFAULT 0 CHECK (deferred IN (0,1)),  -- v4 新增
  attempt        INTEGER NOT NULL DEFAULT 0,                             -- v4 新增
  verdict_json   TEXT,                                                    -- v4 新增
  at             TEXT NOT NULL
);
```

- `TRACE_SCHEMA_VERSION = 4`；新库**单事务**建 v4 DDL + 写 `schema_migrations` 1/2/3/4 四行 + `PRAGMA user_version = 4`（无备份）。
- 旧库迁移：v1→v2→v3→v4 逐版本循环；v3→v4 单版本单事务：`VACUUM INTO '<db>.bak.v3-{ts}'`（回写 `user_version=3`、校验）→ 三条 `ALTER TABLE gate_results ADD COLUMN …`（默认值 `0`/`0`/`NULL`）→ 插 `schema_migrations(4)` → `PRAGMA user_version = 4`；失败 → 删 `-wal`/`-shm` → 备份覆盖恢复 → exit 4。
- 只读 `trace`/`replay`：`user_version < 4` → 拒绝且**不迁移**（`unsupported schema version=N, expected 4`，exit 4）；`> 4` → `STORE_SCHEMA_TOO_NEW`。
- `events.kind='GateResult'` 的 `payload.record` 增 `deferred`/`attempt`/`verdict`（**仅 v4 后新写入事件**）；`deferred` 为 bool、`attempt` 为整数、`verdict` 为 JSON 值或 null。payload 总大小上限（64 KiB）不变。
- **迁移历史行（S2）**：v3→v4 只补 `gate_results` SQL 列默认值（`deferred=0`/`attempt=0`/`verdict_json=NULL`），**不改写历史 `events.payload`**——旧 `GateResult` 事件 payload 仍为 v3 形态（无三键），`trace --json` **不合成**默认键；历史行以表列默认值为准。
- `GateResultRow` 增 `deferred: bool`、`attempt: u32`、`verdict: Option<Value>`（从 `verdict_json` 解析；旧行为 NULL → `None`）；`reader.gate_results` 列清单同步。
- `zgraph trace --json` 自然带出（NDJSON 直接输出 payload；历史行按旧 payload）；**人类可读文本列格式不变**（不加列、不改对齐）。`trace --stats` 不变。
- `verdict_json` 的写入口径：`serde_json::to_string(&verdict)`（`None` → NULL）。**读取失败口径待定**：本 DR 暂冻结「视为 `NULL` + 告警、不让 `trace` 崩」，REV-070 S1 倾向严格报错；见 **ISSUE-105**（W2a-docs/trace 后续定案：reader 兜底 NULL+告警，或改 06 §6.2.8 措辞）。

### 4.3 回放与 golden（C3）

| 层 | 要求 |
|---|---|
| L1 events | `--mode=events` 回放 v4 库：**v4 后新写入事件**的 `GateResult` payload 含三个新键；**v3→v4 迁移的历史事件 payload 保持 v3 形态**（不合成默认键，历史行的三字段以 `gate_results` 表列为准）；`trace --json` 与 replay events 逐字节一致的口径不变 |
| L2 outputs | `--mode=outputs` 语义**不变**（GateResult 不参与 L2 判据；rework 图的 L2 重放记录的是各实例输出，attempt-per-activation 会产生新的 `(node_id, attempt)` 记录） |
| golden | 新增 defer/rework 路径 golden（AC-14 a/b/c）；v0.1.0 v3 seed → 迁移 v4 → `trace`/`replay` 期望输出（AC-14 h） |
| 校验 | **v3 seed 迁移样例由 W2a e2e 交付**（`tests/fixtures/trace/schema_v3_seed.sql` + `v0.1.0_expected_trace.json` 在 W1 未交付，属 W1 遗留缺口，Lead 已登记 **ISSUE-104**）：W2a e2e 生成 v3 seed → 迁移 v4 → 回放断言；DR-004 §6.3 的 seed 路径是**目标路径**，不是既有资产。本批**不得**写成「沿用 W1 交付」 |

---

## 5. py-worker 冻结（D1）

### 5.1 `parse_verdict_rework` 契约

```python
def parse_verdict_rework(inputs: dict[str, Any], ctx: WorkerContext) -> dict[str, Any]:
    """解析评审输出（rework 模式），返回 verdict / comments / artifact。

    输入：
        review（object，必填）：评审 agent 整包输出；content 必须是 JSON 字符串；
        artifact（any，必填）：工作 agent 整包产物，原样透传。
    输出：
        {"verdict": "approved" | "rework", "comments": <str>, "artifact": <原样>}。
    幂等性：
        纯函数，相同 inputs 输出相同。
    外部副作用：
        无（不读写文件、不发网络、不写全局状态）。
    异常：
        review/artifact 缺失、review 非 object、content 非字符串/非法 JSON/非 object、
        verdict 缺失或不在 {"approved","rework"}、comments 非 string 时抛 ValueError
        （worker 侧映射 NODE_PYTHON_EXCEPTION → Crashed → run Failed）。
    """
```

| 项 | 冻结 |
|---|---|
| 签名 | `(inputs: dict[str, Any], ctx: WorkerContext)`；模块 `workers.software_team.parse_verdict`，函数 `parse_verdict_rework` |
| 输入必填 | `review`、`artifact`（缺任一无 → `ValueError`） |
| `review` | 必须 object；`content` 必须 string 且非空白；`content` 必须 `json.loads` 成 **object** |
| verdict | 闭集 `{"approved","rework"}`；缺失/其它值/非字符串 → `ValueError` |
| comments | 缺省 `""`；给了必须 string（含空串合法）；否则 `ValueError` |
| artifact | **原样透传**（identity；测试用 `==` 等价断言，不用 `is`） |
| 错误出口 | 全部 `ValueError`（`NODE_PYTHON_EXCEPTION`）；**不新增**错误码 |
| 副作用 | 纯函数/幂等/无外部副作用（04 §4.3.7 L3 四项声明齐备） |
| v1 不变 | `parse_verdict.run` 行为与异常契约**不改**（approved-only 继续用 `run`） |
| ISSUE-101 | 顺带补 `parse_verdict` docstring 的异常分支清单（缺 artifact、content 非 string/空白、comments 非 string）并加 content 缺失/纯空白边界用例；**代码/测试在 W2a py-worker 分支**，W2a0 只冻结清单 |

### 5.2 错误/映射对照

| 场景 | `parse_verdict`（v1） | `parse_verdict_rework`（v1.1） |
|---|---|---|
| `verdict == "approved"` | 正常输出 | 正常输出 |
| `verdict == "rework"` | `ValueError` → Failed | **正常输出**（SOP rework 由此走回边） |
| 其它 verdict | `ValueError` | `ValueError` |
| 非法 JSON / 形状错误 | `ValueError` | `ValueError` |

> 不许把 rework 表达成 gate=false（SOP §5.3 的 v1 约束在 rework 模式仍成立：gate=false 只用于 approved 前进边的 deferred 挂账，不承担"失败"）。

---

## 6. SOP 设计稿 v1.1 冻结（E1–E5）

> 完整设计见升级后的 [`design/software-team-sop-v1.md`](../design/software-team-sop-v1.md) v1.1 §3.2/§7；本节只冻结**不可变契约**。

### 6.1 双 profile（E1/E4）

| profile | 解析节点 | defer 边 | 适用 |
|---|---|---|---|
| `approved-only`（v1，不变） | `parse_verdict.run` | 无 | W1 现状、CI 回归基线 |
| `rework`（v1.1） | `parse_verdict_rework` | 有（下表） | 返工闭环；W2a e2e |

codegen：`--mode approved|rework`（缺省 `approved`，保持 W1 字节级可复现）。`approved` profile 输出与 W1 完全一致（`graph_hash` 不变）；`rework` profile 只做**增量**：`W` 增可选 `feedback` 输入端口、`P` 增回边、相关边标 `on_gate_false:"defer"`、`W/R/P` 写 `config.retry.max_attempts`。

### 6.2 rework 边表（E2）

| 边 | gate | `on_gate_false` | 作用 |
|---|---|---|---|
| `D_role → W.task` | 无 | （缺省 settle） | 首次派发设计任务（**不在环上**，不得 defer） |
| `D_role → R.spec` | 无 | （缺省 settle） | 评审上下文（首次） |
| `W → R.result` | 无 | **defer** | W 产物送评审；目标 R 已 Completed → attempt-per-activation |
| `W → P.artifact` | 无 | **defer** | 产物直送解析；目标 P 已 Completed → attempt-per-activation |
| `R → P.review` | 无 | **defer** | 评审结论送解析；目标 P 已 Completed → attempt-per-activation |
| `P → W.feedback` | `check_output{path:"verdict",equals:"rework"}` | **defer** | 回边：rework 放行 → W 重入；approved 为 false → 挂账（dormant） |
| `P → D_downstream / end` | `check_output{path:"verdict",equals:"approved"}` | **defer** | 前进边：approved 放行；rework 为 false → 挂账（dormant） |

- `W` 新增 `feedback` 输入端口（`type: "any"`, `required: false`）；`trigger` 仍 `all`（required 仅 `task`）；首次激活 `feedback=null`。这是 R11（`(to_node,to_input_port)` 唯一）+ R12（非 entry 有入边、required 端口有源）下的**唯一**接线方式（`P→W` 不能复用 `task` 端口）——见 §11 冲突 C-2。
- **统一规则**：「**环上所有可能重入已 `Completed` 目标实例的边都必须标 `defer`**」。DR-004 §3 W2a0 的「两条 defer 边（approved 前进 / rework 回边）」是**最小子集**；v1.1 codegen 展开为环上全部可重入边，同时满足 REV-063 C11 与 R15。
- **R15 自检**：defer 源 = `W`/`R`/`P`/（前进边源 P）；`SCC = {W,R,P}` 且 SCC 内存在 defer 边（`W→R` 等）→ 全部通过。环上 gate-less 边 `W→R`、`R→P`、`W→P.artifact` 不构成 R13 的无 gate 环（`P→W` 带 gate 打断）→ R13 通过。

### 6.3 ASCII 拓扑（单 item，rework profile）

```text
                      ┌──────────────────────────── rework 回边（gate=rework, defer）──────────────────────────┐
                      │                                                                                        │
  D_role ──task──▶ W_work ──result──▶ R_review ──review──▶ P_parse ──gate(approved, defer)──▶ 下游 D_k / end ──▶ …
     │              │                   ▲                    ▲
     └──spec────────┼───────────────────┘                    │
                    └──artifact─────────────────────────────┘
      （W→R、W→P.artifact、R→P 三条边：无 gate，标 defer，用于目标已 Completed 时 attempt 重入）
```

- 一次 rework 循环：`P(rework) → W(attempt+1) → R(attempt+1) → P(attempt+1) → P(approved) → 下游/end(首次或 attempt+1)`。
- 首轮 approved：`P(approved) → 下游`；回边 `P→W` gate=false → receipt（目标 W 已 Completed）→ dormant，不阻塞。

### 6.4 `max_attempts` 预算（E3）

| 项 | 冻结 |
|---|---|
| 写入口 | rework profile codegen 对 `W/R/P` 节点写 `config.retry.max_attempts`（缺省 3；可参数化但 e2e 固定 3） |
| 语义 | 每节点实例 attempt 上限；`W/R/P` 分别计数（不共享预算） |
| 默认预算 | `3` → 每节点最多 3 次实例（0/1/2）→ 最多 **2 次 rework**；第 3 次 rework 在 `P→W` 放行时 `W` 新实例 attempt=3 ≥ 3 → `Crashed(Internal)` → run `Failed(NODE_INTERNAL)` |
| 超限证据 | `W` 的 Crashed 实例（attempt=3，`kind=Internal`）+ **之前轮次**已产生的 `P→W` `GateResult` 行 + run `Failed` + `NODE_INTERNAL`；**超限该次 GateResult 不入库**（`fail_run` 关 sink，外层 emit no-op，见 §3.5/§7 e）；其他非终态实例 `Cancelled` |
| approved-only | 不写该字段也不标 defer（缺省常量生效）→ 与 W1 一致 |

### 6.5 延期/限制同步（E5）

- 延期项：**图级有限返工移出**（v1.1 落地；框架 W2a）；仍延期：人工评审（W3a）、每节点扇出上限（W2b）、动态 N（W4b）、共享评审队列（W5/v0.3）。
- 限制：rework 模式**不重试** `R` 的 LLM 失败/`P` 的解析异常（仍冷峻 Failed）；不做跨 run 返工；不改 working agent 产物（返工靠新 attempt 重跑，输入继承 + `feedback`）。

---

## 7. AC-14 验收矩阵（F）

> 07 §§7.1/7.2 与 AC-14 正文同步；资产在 W2a 分支交付（`e2e`/`py-worker`），W2a0 只冻结断言。

| # | 场景 | 图/资产 | 关键断言 |
|---|---|---|---|
| a | **自环 defer 返工** | `tests/fixtures/graphs/defer_gate/defer_self_loop.json`：`start → loop`，`loop --gate(check_output{path:"continue",equals:true}, defer)--> loop.again`；`loop` 输入 `seed`(required，start→loop 注入) + `again`(**required:false**，只能由自环供源；若 required 则首轮 `trigger=all` 永不就绪——ISSUE-062 同型坑)；Python stub 第 1 次输出 `{continue:true}`、第 2 次 `{continue:false}` | run `Completed`；`loop` 实例 attempt `0/1`；attempt1 因**按端口继承**拿到 `seed`（`again` 由自环重投）；`gate_results` 两行（同一边）：**attempt0 `passed=true/deferred=false`**、**attempt1 `passed=false/deferred=true`**；随后目标节点当前实例已 `Completed` → `deferred_receipt` dormant → run 正常 `Completed`；无 `RUN_DEADLOCK` |
| b | **A→B→A 回边返工** | `tests/fixtures/graphs/defer_gate/defer_ab_cycle.json`：`start→A`，`A --gate(defer)--> B`，`B --gate(defer)--> A`；**两条环边均带 gate**（打断 R13 的无门控环；as-built 为 `check_output` 判 A/B 输出标志），`A`/`B` 的循环端口 `required:false`（入口/终止方式由 e2e 实现定，契约：R13/R15 均须通过）；`start→A` 用 settle 边供首次输入 | A attempt `0/1`、B attempt `0/1`；run `Completed`；R15 通过（`SCC={A,B}` 含 defer 边）；R13 通过（无无 gate 环） |
| c | **SOP v1 rework e2e** | `software_team_v1` codegen `--mode rework`（**full 60/116、smoke 11/21**；approved 60/100、11/18 不变）+ `parse_verdict_rework` + fake LLM（rework→approved 脚本；**调用序可判定**）。**as-built（REV-073）**：codegen 静态断言已通过（`tests/e2e/software_team_v1.rs::rework_codegen_validates_with_expected_scale_and_full_defer_policy`）；运行期证据待 e2e Phase 2B | run `Completed`；`W/R/P` attempts `0/1`；trace `gate_results` 含 `deferred=true`（approved 前进边 false）、`attempt`、`verdict="rework"/"approved"`；下游拿到最终 artifact（`P.artifact` 透传，非评审摘要）；`parse_verdict_rework` 输出 `verdict=rework` 不抛错；**P 的新一轮执行必须消费新一轮 R 的 `review`**（fake LLM 调用序锁定：若 P 用旧 review 提前执行，会消耗错误脚本条目而失败——直接验证 per-port 不继承） |
| d | **回归不变** | v0.1.0 全部 fixture（`crates/zgraph-dsl/tests/fixtures.rs` golden）+ W1 approved-only 图 | `graph_hash` 逐字节不变；validate/run 结果、trace golden 不变；无 defer 字段的图 `deferred=0`（迁移后旧库亦然） |
| e | **超 max_attempts** | rework profile + fake LLM 恒定 `rework`（codegen 已写 `W/R/P` `config.retry.max_attempts=3`，静态已断言；运行期证据待 Phase 2B） | run `Failed` + `NODE_INTERNAL`；`Crashed` 实例 attempt = `max_attempts`（=3）；`instances.error.kind=Internal`；非终态实例 `Cancelled`；**不断言超限那次的 GateResult**（sink 已关闭、不入库，见 §3.5）；证据 = Crashed 实例 + 之前 GateResult + `RunFinished{Failed}` |
| f | **dormant 正常完成** | `defer_gate/defer_self_loop.json` 的内存变体（e2e 改 gate `equals=false` 后写临时图）：gate 恒 false（defer）→ 只挂账 | run `Completed`（不误报 `RUN_DEADLOCK`）：`loop` 仅 attempt0、自环边 1 行 `passed=false/deferred=true`；receipt 目标实例存在且 `Completed` → dormant |
| g | **blocked deferred → 死锁（R15-valid 变体）** | `tests/fixtures/graphs/defer_gate/defer_blocked.json`（**必须通过 R15**）：`start→a`；`a --gate(manual default false, on_gate_false:"defer")--> a.again`（`again` required:false，**defer 自环**给 `a` 一个可重入来源）；`a --gate(恒 false, on_gate_false:"defer")--> b.x`（`b` 非 entry、`x` required 仅由该边供源）。运行后 a 有两条 receipt：`a→a` 目标 a 已 `Completed` → dormant；`a→b` 目标 b 从未实例化 → blocked | 静默后**立即** `Failed` + `RUN_DEADLOCK`（不等 `deadlock_grace`）；`error.message` 含 edge 索引与 target node；`b` 无实例。**注**：若删掉 defer 自环，`a` 不在含 defer 边的环上 → `zgraph validate` 先报 `DSL_DEFER_NOT_REENTRANT`（R15 设计意图，不可用该骨架做 blocked 用例） |
| h | **schema v4 迁移与回放** | v3 seed（`tests/fixtures/trace/{schema_v3_seed.sql,v0.1.0_expected_trace.json,seed_graph.json,GENERATION.md}`，**W2a e2e 交付**；W1 遗留缺口 ISSUE-104）→ 迁移 v4；defer 图新库 | 迁移后 `user_version=4` + `schema_migrations` 1..4 + 备份 `.bak.v3-*`；**旧行**取默认 `deferred=0/attempt=0/verdict=NULL` 且历史 `events.payload` 保持 v3 形态；**新 run** payload 含三键；只读 `trace` 对 v3 拒绝（exit 4）；**L1 events：历史 run 逐字节复用旧 payload、新 run 含三键**；L2 outputs 语义不变 |

> **as-built（W2a 实现，REV-073）**：AC-14 资产已落 `tests/fixtures/graphs/defer_gate/` 与 `tests/fixtures/trace/`；`tests/e2e/gate_defer.rs` 覆盖 a/b/f/g（断言已写、`#[ignore]` 待 e2e Phase 2B 打开）与 h（已运行通过：迁移/旧 payload/只读拒绝/L1 golden）；`tests/e2e/software_team_v1.rs` 覆盖 c/e 的 codegen 静态断言（full 60/116、smoke 11/21、defer 统一规则、`max_attempts=3`），c/e 运行期证据待 Phase 2B。实现证据汇总见 spec 08 §8.9.1。

### 7.1 测试落点（W2a）

| 模块 | 落点 |
|---|---|
| dsl | `crates/zgraph-dsl/tests/fixtures.rs`（R15 正反例、`on_gate_false` 类型/白名单、hash 兼容） |
| kernel | `crates/zgraph-kernel/tests/**`（defer 决策矩阵、attempt-per-activation、buffer 继承、end 特例、自环不双增、dormant/deadlock、超限） |
| trace | `crates/zgraph-trace/tests/**`（v4 DDL/迁移/备份/只读拒绝、GateResultRow 新字段） |
| py-worker | `python/tests/test_workers.py`（`parse_verdict_rework` 正常/异常边界 + ISSUE-101 边界） |
| e2e | `tests/e2e/gate_defer.rs`（a/b/f/g）、`tests/e2e/software_team_v1.rs` rework 用例（c/e）、`tests/e2e/cli_replay.rs`（h） |
| docs | 本批 spec 回写 + SOP v1.1 + 07 AC-14（W2a0） |

### 7.2 门禁

- `cargo test --workspace`、`cargo clippy --workspace --all-targets --locked -- -D warnings`、`pytest`、`bash scripts/dev/check.sh` 全绿；v0.1.0 兼容矩阵（DR-004 §6.3）无新增失败。
- W2a 合并后由 W2a-docs 再做一次 spec 回写（若实现与冻结有偏差，另立 DR/修订段，不改本 DR 的决策）。

---

## 8. 实施顺序与模块拆分（W2a）

| 顺序 | 模块 | 分支（DR-004 §4） | 依赖 | 关键交付 |
|---|---|---|---|---|
| 1 | docs | `docs/docs-rework-design`（本批） | 无 | 本 DR + spec 00/01/02/03/06/07/08 + SOP v1.1 + PROVENANCE |
| 2 | dsl | `feat/dsl-gate-defer` | 本批 | `on_gate_false` 字段/白名单/类型桶/hash 投影/编译映射/R15 + 夹具 |
| 3 | trace | `feat/trace-gate-defer` | 本批（可与 dsl 并行，接口按 §4） | `GateResultRecord`/`GateResultRow` 三字段、schema v4、迁移、payload |
| 4 | kernel | `feat/kernel-deferrable-gate` | dsl 合并（CompiledEdge 字段） | 决策矩阵、DeferredReceipt、attempt-per-activation + buffer 继承、end/自环、超限、dormant/deadlock |
| 5 | py-worker | `feat/py-worker-parse-rework` | W1 py-worker 已合并（DR-004 S11） | `parse_verdict_rework` + ISSUE-101 边界 |
| 6 | e2e | `test/e2e-gate-defer` | kernel/dsl/trace/py-worker/cli 合并 | AC-14 a/b/c/e/h 的图与脚本（c/e 走 SOP rework codegen）；断言 `trace --json` 三字段非默认值 |
| 7 | **cli**（S3） | `feat/cli-gate-defer-bridge`（名称由 Lead 定） | trace 分支已合并（字段已存在）；kernel 分支合并后接真实值 | **`bridge.rs::map_gate_result`**：trace 分支先落过渡值 `deferred=false/attempt=0/verdict=None`（REV-070 特批）；kernel 分支接线**真实** `deferred`/`attempt`/`verdict`；`run_cmd.rs` 测试字面量与 `tests/trace.rs` 断言同步 |

- **写作用域互斥**：docs 只写 `doc/**`；kernel 只写 `crates/zgraph-kernel/**`；dsl/trace/py-worker/e2e 按 AGENTS §2；**cli 只写 `crates/zgraph-cli/**`**；`e2e` 单写者，W1 e2e 已合并后才开始。
- **依赖钉死**：kernel 等 dsl 的 `CompiledEdge` 字段；e2e 等全部；py-worker 与 dsl 可并行；**cli 的真实值接线等 kernel 合并**（过渡值 → 真实值，不留 dummy）。
- **跨模块评审（S3）**：cli 改动由 **Lead 声明为跨模块**并指定第二评审人——REV-070（trace 分支）由 Lead 追加第二评审段；REV-071（kernel/cli 接线）第二评审人由 Lead 另定。
- **docs 串行**：本批合并后才允许 W2a-docs（`docs/docs-gate-defer-spec`）回写实现偏差。
- **注意**：DR-004 §4 的 W2a 波次表**未列 cli**，本 DR §8 的 cli 项是 Lead 2026-10-31 对 S3 的裁决增补，以本 DR 为准。

---

## 9. 兼容性与回归矩阵

| 类别 | 用例 | 期望 |
|---|---|---|
| hash | v0.1.0 fixtures、`software_team_v1` full/smoke/exhaust、approved profile | `graph_hash` 与 W1 基线逐字节一致 |
| validate | 无 defer 图 | 错误码/定位/`error_count` golden 不变 |
| validate | defer 图正/反例 | R15 正例通过；反例 `DSL_DEFER_NOT_REENTRANT`（定位 `edge_index`+`node_id`） |
| run | 无 defer 图（含 `valid_cycle.json` 既有失败断言） | 行为不变（`valid_cycle.json` 仍 FAILED/`RUN_DEADLOCK`，ISSUE-062 不在本批修） |
| trace v3 | 只读 `trace` | 拒绝（exit 4，不迁移） |
| trace v3 | `run` 打开 | 迁移 v4（备份 + 单事务）+ 旧行默认值；`trace` 可读 |
| trace v4 | `zgraph trace --json` | 新字段自然出现；人类文本列格式不变 |
| L1 | v4 库 events 回放 | 逐字节一致（含新字段） |
| L2 | rework 图 `--record=full` + `replay --mode=outputs` | 语义不变（按新 `(node_id, attempt)` 记录重放） |

---

## 10. 风险与缓解

| 风险 | 应对 |
|---|---|
| 重入实例的输入继承被误实现 | **必须同时实现**「per-port 不继承 defer 端口」+「创建时清空 `settled_by_node[target]`」，否则出现 **stale-input 提前就绪**（P 用旧 review）或 **误 Skipped**（node 级 settled 集级联）；§3.2 补全规则 + kernel 单测（P/W/R 多输入重入）+ AC-14 c 的 fake-LLM 调用序锁定 |
| deferred receipt 误判 dormant/blocked | §3.4 判定表 + AC-14 f/g 正反例；receipt 用 BTreeMap 保证 message 确定性 |
| R15 实现两套（`|SCC|≥2` vs 简单环） | §2.5.2 冻结等价判定 + 边界 E-1；dsl 夹具 P-1/P-2/N-1/N-2 |
| R13/R15 顺序出错导致错误码漂移 | §2.5.1 冻结 `R1→R13→R15→R14`；N-4/N-5 例 |
| SOP rework 边漏标 defer → 运行期 `RUN_DELIVERY_TO_TERMINAL` | §6.2 统一规则 + codegen 生成后自检（遍历 `SCC={P,W,R}` 边全集必须全部 defer）；AC-14 c |
| schema v4 迁移失败 | 逐版本 `VACUUM INTO` 备份 + 单事务 + 失败恢复 + 只读拒绝（06 §6.7）；AC-14 h |
| v0.1.0 行为漂移 | B6 + AC-14 d golden（hash/validate/run/trace） |
| end 被 defer 重入 | B2 显式拒绝（`RUN_DELIVERY_TO_TERMINAL`），避免未定义的 run 级聚合重入 |
| `max_attempts` 预算误设（过小/过大） | E3 预算表 + AC-14 e；默认 3 冻结 |

---

## 11. 冲突、边界与需 Lead/评审确认项

### 11.1 与既有 spec / DR 的冲突及处置

| # | 冲突点 | 处置 |
|---|---|---|
| C-1 | **B1 未写 buffer 继承**：SOP 的 `W`/`R` 是多输入节点，attempt-per-activation 新建实例若只带本次投递数据，`trigger=all` 永不满足 → 误死锁；AC-14 a/c 也无法通过；而「继承全部 arrived」又会 **stale-input 提前就绪** / **node 级 settled 集误 Skipped** | **Lead 2026-10-31 裁决确认**：补全为「**按端口继承**（只继承入边 `on_gate_false != Defer` 的端口）+ **重入实例创建时清空 `settled_by_node[target]`**」（§3.2 行 6/7 补全规则、§3.3）；**不改变** attempt 编号与超限语义；v0.1.0 无重入 → 不变 |
| C-2 | **SOP `P→W` 回边的端口**：`D_role→W.task` 已占用 `W.task`，R11 禁止同一 `(to_node,to_input_port)` 双源；若 `P→W` 另开 `feedback` 端口且 required，则首次激活永不就绪 | rework profile 给 `W` 增**可选** `feedback` 端口（`required:false`），`P→W.feedback`；首次 `feedback=null`，重入靠 buffer 继承 + 新 feedback。approved-only profile 不变（E1） |
| C-3 | **04 §4.6.4**：「不得改 `GateResultRecord`」 vs C1 新增三字段 | 本批同步修订 04 §4.6.4（冻结例外：只增三字段、仍不记耗时、后续新增须升 spec 版本）；`GateResultRecord` 只增字段，不改既有字段语义 |
| C-4 | **02 §2.1.3**：「规则编号即执行顺序」 vs R15 属 L2 但编号在 R14 之后 | 冻结执行顺序 `R1→R13→R15→R14`；02 §2.1.2/§2.1.3 写明「R15 为 L2 纯函数规则，必须在 L3 的 R14 之前；编号保留仅为契约兼容」 |
| C-5 | **06 §6.2.8** 引 04「不得改 `GateResultRecord`」 | 随 C-3 同步改引文；v4 DDL 与 payload 以本 DR §4 为准 |
| C-6 | **DR-004 §3 W2a0** 只写「两条 defer 边」 | 明确其为**最小子集**；v1.1 codegen 展开环上全部可重入边（REV-063 C11 统一规则 + R15），不违背原决策 |
| C-7 | **architecture-v2.md** 仍写 `R1–R14` / 17 码（非本批允许写文件） | 本批**不修改** `doc/architecture-v2.md`；记为跨册残留，建议 Lead 在 W2a-docs 或 process 收口时同步（本 DR 与 spec 00/02 已是权威口径） |

### 11.2 冻结时补全的边界（无现成决策）

| # | 边界 | 冻结 |
|---|---|---|
| E-1 | R15 的「环」是否要求简单环 | 按**闭迹**语义冻结，`|SCC|≥2 且 SCC 内含 defer 边` 为**权威判定**；按「简单环」口径**更严**。构造例（SCC=`{u,x,a,b}`，边 `u→x, x→u, x→a, a→b, b→x`，defer 边 `a→b`）：闭迹 `u→x→a→b→x→u` 过 u 且含 `a→b`；但不存在同时过 u 与 `a→b` 的**简单环**——本规则按冻结判定**通过**，这是**放宽例**（不是反例，也不加反例夹具，避免自相矛盾） |
| E-2 | receipt 目标实例为终态非 `Completed`（`Skipped`） | 视为 **dormant**（终态不可能再投递）；`Crashed`/`TimedOut` 时 run 已冷峻 Failed，不可达。若后续对该终态实例放行，按 §3.2 决策矩阵行 9 `RUN_DELIVERY_TO_TERMINAL` |
| E-3 | gate=false 的 defer 边是否清空/消费 receipt | **不消费**：receipt 表示「最近一次 deferred 决策」，放行不删除；dormant 只看目标当前实例状态（确定性、少分支） |
| E-4 | defer 边目标为非 entry 但从未实例化的「合法 dormant」场景 | 不存在：receipt 目标从未实例化 ⇒ 该 defer 边没有再交付路径 ⇒ 冻结为 **blocked → RUN_DEADLOCK**（AC-14 g 的 R15-valid 变体：必须另有 defer 自环/可重入来源，否则 R15 先拒绝）；若设计者需要「可选延迟边」，应在图上保证目标可从其它路径实例化 |
| E-5 | `verdict` 的路径取值对 `compare` 的 `path` 缺失 | `None`（不报错；gate 求值本身按 compare 契约处理） |

> 上述 C-1/C-2/C-4/C-7 与 E-1–E-5 请在 REV-067 评审中逐条确认；若有不同意见，以 Lead 裁决为准并回写本 DR（追加修订段，不改历史决策）。

---

## 12. 影响文件清单与本批门禁

### 12.1 本批（W2a0，docs）改动

| 文件 | 改动 |
|---|---|
| `doc/decisions/DR-005-w2a-defer-gate-freeze.md` | 新增（本文件） |
| `doc/spec-v1.2/00-总纲.md` | banner；`OnGateFalse` 枚举；`DSL_DEFER_NOT_REENTRANT`（17→18）；`DEFAULT_MAX_ATTEMPTS` 语义；运行期错误注记；术语 |
| `doc/spec-v1.2/01-领域模型与状态机.md` | banner；`CompiledEdge.on_gate_false`；`DeferredReceipt`/`RunState`；寻址表；`GateResultRecord` 三字段；`GateEvaluation.verdict` |
| `doc/spec-v1.2/02-DSL与校验.md` | banner；字段/白名单/serde；R15；错误模板；§2.7（18 码/顺序）；hash 投影；编译映射；R6 递归；结构校验 |
| `doc/spec-v1.2/03-调度器与生命周期.md` | banner；§3.4 求值/应用；新增 §3.4.3；§3.9.3/§3.10；§3.15 矩阵 |
| `doc/spec-v1.2/04-节点接口与扩展点.md` | banner；§4.6.4 三字段例外 |
| `doc/spec-v1.2/06-存储与可观测.md` | banner；schema v4；DDL/payload/reader；§6.7 迁移；§6.10 注记 |
| `doc/spec-v1.2/07-验收用例.md` | banner；§7.1/§7.2/DoD；新增 AC-14 |
| `doc/spec-v1.2/08-审计关闭矩阵.md` | 只追加 §8.9 W2a0 注记（不改 8.0–8.8） |
| `doc/design/software-team-sop-v1.md` | 升级 v1.1（E1–E5） |
| `doc/spec-v1.2/README.md` / `PROVENANCE.md` / `doc/index.md` | 行数/状态/§2 第 15 条/hash/索引 |

> **W2a 实现波及（非本批 docs 改动，S3）**：`crates/zgraph-cli/src/bridge.rs`（`map_gate_result`：过渡值 → 真实 `deferred/attempt/verdict`）、`crates/zgraph-cli/src/run_cmd.rs`（测试字面量）、`tests/trace.rs`（断言）——由 W2a 的 cli 适配分支交付，见 §8 第 7 行；**不得**把 cli 写成「不改」。本批（W2a0）仍只改 `doc/**`。

### 12.2 不修改（红线）

`CLAUDE.md`、`doc/progress.md`、`doc/issues.md`、`doc/reviews/**`（评审记录由评审人写）、任何代码/测试/配置/`schemas/**`、`doc/architecture-v2.md`（非本批允许范围，见 C-7）。

> 注：本表约束的是 **W2a0 docs 批**；W2a 实现批**包含 cli**（§8 第 7 行），不在此列。

### 12.3 提交与评审

- 分支 `docs/docs-rework-design`；提交正文含 `Review: REV-067` 与 `Agent: docs-writer`；**3 个逻辑 commit**（设计 DR+SOP / spec 00–03 / spec 06–08+索引）+ **1 个 REV-067 条件修复 commit**（`749e9ee`）；W2a 实现回写（REV-073）在 `docs/docs-gate-defer-spec` 分支另计。
- `bash scripts/dev/check.sh` 必须 PASSED；PROVENANCE §3 的 SHA-256/行数以实测为准。
- 合并前由独立评审人写 `doc/reviews/REV-067-docs-rework-design.md`，结论必须为「通过」（AGENTS §3/§4）。

---

## 13. 与 DR-004 / REV-063 / ISSUE 的对应

| 来源 | 本 DR 落点 |
|---|---|
| DR-004 §3 W2a0（P v2 / 返工图模式 / dormant / 语法 / 上限 / trace / 兼容） | §5 / §6 / §3.4 / §2 / §3.5 / §4 / §9 |
| DR-004 §5 D1（opt-in deferrable gate + attempt-per-activation） | §0 / §3.2 |
| DR-004 §3 W2a DoD（自环/A→B→A/SOP rework e2e/双路径/超限/golden） | §7 AC-14 a–e、§9 |
| REV-063 **C11**（defer 边源节点在含 defer 的环上） | §2.5 R15（G_all 判定） |
| REV-063 **C9**（attempt-per-activation/dormant/max_attempts） | §3.2/§3.4/§2.6 |
| REV-063 **S12** / ISSUE-100（R13 只查无 gate 环，需新增规则） | §2.5（R15 与 R13 独立；N-4/N-5 说明） |
| ISSUE-101（`parse_verdict` docstring/边界） | §5.1 |
| ISSUE-062（`valid_cycle.json` 自环重入死锁） | §9：本批**不修**（defer 自环是 opt-in 新语义；无 defer 自环行为不变） |
