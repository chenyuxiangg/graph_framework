# DR-007 · W4a 动态 N 设计冻结：节点级 `foreach` 展开 + 有序 join + trace v6

> **状态**：设计冻结（W4b 实现依据）；评审编号 **REV-086**（评审记录由独立评审人撰写，不在本分支）。
> **日期**：2026-11-01 · **作者**：docs-dev · **分支**：`docs/docs-dynamic-fanout-design`
> **关联**：[`DR-004-v02-scope-and-waves.md`](DR-004-v02-scope-and-waves.md) §3 W4a / §5 D4、[`DR-005-w2a-defer-gate-freeze.md`](DR-005-w2a-defer-gate-freeze.md)（attempt-per-activation/defer）、[`DR-006-w3a-human-decide-freeze.md`](DR-006-w3a-human-decide-freeze.md)（human/observer/trace v5 模式）、spec v1.2 分册 00/01/02/03/04/06/07/08、[`design/software-team-sop-v1.md`](../design/software-team-sop-v1.md)（本批升 v1.2）
> **前置事实**：W1/W2a/W2b/W3a 已合入 `main`（HEAD `559ede6`）；用户 **2026-11-01 决定 W3b（durable resume）不做**，不纳入 v0.2（记入 DR-004 §10 与 progress）；本批只写设计、不写 as-built、不改代码。
> **效力**：本 DR 是 W4b（dsl / kernel / trace / cli / e2e）与 W4b-docs 的**唯一语义依据**；与既有 spec 冲突处以本 DR + 本批 spec 冻结写入为准（见 §15）。**默认（无 `foreach` 的图）行为/hash/validate/run/trace/回放与现状逐字节不变**。

---

## 0. 摘要（一页）

| # | 冻结内容 | 落点 |
|---|---|---|
| F1 | 节点级 `foreach` 字段（仅 `python_node`/`agent_call`，含 `builtin.human`）：`items_port`/`key_field`/`join`/`max_items`/`on_item_error` 闭集 | 02 §2.2.1/§2.2.2/§2.2.4 |
| F2 | **R17（形状）**：复用 `DSL_PARSE_ERROR`（R4 阶段）；**R18（区域）**：新 DSL 码 **`DSL_FOREACH_INVALID`**（DSL 域 18→19）；reason 闭集 11 值；执行顺序 `R1→R13→R15→R18→R16→R14` | 00 §0.4；02 §2.4.1/§2.4.2/§2.4.3 |
| F3 | **区域 R** = F ∪ 从 F 出发、不经过 J 可达的节点；边界闭合/单一出口 E→J（无 gate）/禁止嵌套/禁止与其它 region 相交/region 内类型限制 | 02 §2.4.1 R18；本 DR §4 |
| F4 | **hash**：canonical 投影节点键仅作者显式写 `foreach` 时加入；缺省不补齐 → 旧图 hash 逐字节不变 | 02 §2.8.3；本 DR §5 |
| F5 | **运行期无 controller 实例**：F 的入边正常 gate/GateResult，但**不创建 F 单实例**；controller buffer 按 F 的 `trigger` 就绪 → **展开**（`expansion_id` UUIDv7） | 03 §3.4.6；本 DR §6 |
| F6 | **item 实例**：R 内节点每 item 一个实例，定位键 `(node_id, item_index)`；F 自身 per-item 执行；attempt/超时/defer/W2b 均在 `(node,item)` 作用域 | 01 §1.2.5；03 §3.4.6 |
| F7 | **join**：R 唯一出口 E 的 N 个 item 全部 Completed 后，按 `item_index` 升序构造数组投递给 J（E→J 边的 `to_input_port`）；空列表立即投递 `[]` | 03 §3.4.6；本 DR §6.5 |
| F8 | **4 个 run 级错误**（无实例 Crashed，写 `RunFinished.error`）：`RUN_FOREACH_ITEMS_NOT_ARRAY` / `RUN_FOREACH_ITEMS_EXCEEDED` / `RUN_FOREACH_ITEM_KEY_DUPLICATE` / `RUN_FOREACH_ITEM_INCOMPLETE` | 00 §0.4；03 §3.15；本 DR §7 |
| F9 | **trace schema v6**：`instances` 增 `item_index`/`item_key` + 唯一键重建；新表 `foreach_expansions`；`trace` 追加 ForeachExpansion 行；v5→v6 迁移/备份/只读拒绝；v1..v5→v6 链；`recorded_outputs` 结构不改、L2 查找键扩展 | 06 §6.2.2/§6.2.5/§6.2.15/§6.7/§6.10.4 |
| F10 | **CLI**：`run` 无新参数；`trace`/`replay` 只扩输出；退出码沿用既有 0/1/2/3/4/5/70（run 级 FOREACH 错误走既有 run 失败路径 exit 1）；`on_item_error` v0.2 闭集仅 `fail` | 06 §6.10.4/§6.10.7；本 DR §9 |
| F11 | **SOP 设计稿升 v1.2**：新增 dynamic-N profile（foreach 替代按 N codegen；per-item W/R/P 链 + join；静态 v1/v1.1 profile 兼容）；§9 移除「动态 N」延期项 | `design/software-team-sop-v1.md` v1.2；本 DR §10 |
| F12 | **AC-17 a–j 设计**（W4b 实测；W4a 只冻结断言口径） | 07 AC-17；本 DR §11 |
| F13 | W4b 实施顺序：dsl → kernel+trace → cli（如需）→ e2e；影响文件清单与非目标见 §14/§15 | 本 DR §14 |

**一句话**：把「列表 → 固定图」扩展为「列表 → 运行期展开的 per-item 子图」：作者在节点上声明 `foreach`（消费一个列表端口），框架把该节点与其下游 per-item 链路按列表元素**各执行一次**，再把每个 item 的出口结果按索引聚成**有序数组**交给 join 节点；DSL 校验保证区域边界可判定，trace 记录展开事实，回放按 item 重放。

---

## 1. 背景、目标与非目标

### 1.1 背景

- W1 SOP v1 / W2a v1.1 的工作 agent 数量 N 由 **codegen 按 N 生成固定图**（`software_team_v1` full 60/116、smoke 11/21）；N 在运行期变化（用户输入的 items、上游产出的 items）时无法复用同一张图。
- DR-004 §3 W4a/§5 D4 已冻结方向：**节点级 `foreach`**（来源=entry input 或上游输出列表；item key 确定性；join=单端口有序 item 结果列表；默认 `on_item_error=fail`；取消传播；每激活上限复用 W2b；trace 记录展开；`graph_hash` 只随图 JSON）。
- W2a 的 attempt-per-activation、W2b 的每激活扇出上限、W3a 的 human/observer 已提供可复用的运行期基础；W4a 负责把动态 N 收敛成**可实现、可校验、可回放**的契约。

### 1.2 目标

1. **运行期动态 N**：同一张图按列表长度 N 展开 per-item 子图，N 由运行期数据决定（entry input 或上游输出）。
2. **静态可校验**：区域边界、单一出口、join、类型/嵌套禁令在加载期由 R17/R18 判定；错误定位稳定。
3. **默认零变化**：无 `foreach` 的图 hash/validate/run/trace/回放逐字节不变；`events.kind` 不变（展开事实落新表，不新增事件类型）。
4. **可回放**：trace v6 记录 item 实例与展开事实；L1 events 回放不变；L2 per-item 输出重放。
5. **与既有语义正交**：region 内继续复用 W2a defer/attempt、W2b 每激活上限、W3a human per-item；取消/Pause/超时/冷峻失败按 item 传播。

### 1.3 非目标（W4a 明确）

- W5 共享评审队列；动态子图；**嵌套 foreach**；`on_item_error` 非 `fail`（v0.2 闭集仅 fail）；dynamic port routing（join 单端口有序数组，不动态加端口）；durable/checkpoint（W3b 已决定不做）；item 级独立重试策略（仅 attempt/max_attempts）；foreach 用于 `start`/`end`/`subgraph`；子图内 foreach。
- 不做「按 item 并行度上限」配置（沿用全局/每 run worker 名额；N 只由 `max_items` 约束）。
- 不改 `NodeHandler`/worker 协议（per-item 仍是同一 handler 被多次调用）。

### 1.4 术语

| 术语 | 定义 |
|---|---|
| `F` | 声明了 `foreach` 的节点（foreach 节点） |
| `J` | `foreach.join` 指向的 join 节点（F 的下游、region 外） |
| **region R** | F ∪ 从 F 出发、不经过 J 可达的节点集合（§4） |
| 出口 `E` | R 中唯一「有出边指向 R 外」的节点；E→J 恰一条无 gate 边 |
| `expansion` | 一次 F 的展开事实（`expansion_id` + item_count + keys） |
| `item_index` | 展开内元素下标 `0..N-1`（`ItemIndex = u32`） |
| `item_key` | 元素确定性键（显式 `key_field` 或按索引派生；`NULL` = 按索引派生） |
| controller buffer | F 的**非实例**输入缓存（按 F 的 `trigger` 就绪后触发展开） |
| join 数组 | 按 `item_index` 升序的 `[E.output_0..E.output_{N-1}]`，投递给 J |

---

## 2. DSL 语法冻结（F1）

### 2.1 `foreach` 字段（仅节点级、仅 `python_node`/`agent_call`，含 `builtin.human`）

```json
"foreach": {
  "items_port": "<input port name>",   // 必填；必须是本节点已声明 input 端口
  "key_field": "<field name>",         // 可选；元素为 object 时取该字段作 item_key
  "join": "<node_id>",                 // 必填；该节点是 region 唯一出口指向的 join 节点
  "max_items": 100,                    // 可选；整数 >=1；缺省无上限
  "on_item_error": "fail"              // 可选；v0.2 闭集仅 "fail"（缺省 fail）
}
```

| 键 | 必填 | 类型/取值 | 说明 |
|---|---|---|---|
| `items_port` | ✅ | string，且 ∈ F 的 `inputs[].name` | 该端口的值在运行期必须是 JSON array（§6.2） |
| `key_field` | ❌ | string，非空 | item 为 object 时取该字段；缺省/元素非 object → item_key=NULL（按索引派生） |
| `join` | ✅ | string（NodeId） | 必须存在、≠F、∉R；类型 ∈ {python_node, agent_call, end} |
| `max_items` | ❌ | 整数 ≥1 | 缺省无上限；N > max_items → `RUN_FOREACH_ITEMS_EXCEEDED` |
| `on_item_error` | ❌ | 闭集 `"fail"` | v0.2 唯一值；其它字符串/类型 → R17 `DSL_PARSE_ERROR` |

- **节点类型限制**：`start`/`end`/`subgraph` 出现 `foreach` → R17 `DSL_PARSE_ERROR`（`node '{id}' of type '{type}' does not support foreach`）。
- **子图内不允许**：`subgraphs` 内节点出现 `foreach` → R6 收敛 `DSL_SUBGRAPH_INVALID`（message 前缀 `subgraph '<id>': …`，location 保留内部 node_id）。
- **DSL schema/既有字段**：`GraphNode` 仅新增可选顶层键 `foreach`；`config` 不变；所有既有节点/边字段与规则 R1–R16 不变。

### 2.2 白名单与 serde

- `GraphNode` 节点键白名单（7→8）：`id, type, inputs, trigger, config, outputs, max_fanout, foreach`。
- `foreach` 对象键白名单：`items_port, key_field, join, max_items, on_item_error`；未知键 → R17（拼写错误桶）。
- Rust 对照：`GraphNode.foreach: Option<ForeachConfig>`；`ForeachConfig { items_port: String, key_field: Option<String>, join: NodeId, max_items: Option<u32>, on_item_error: Option<OnItemError> }`；`serde(skip_serializing_if="Option::is_none")`（None 不输出 null）；`OnItemError` 枚举闭集 `Fail`（`rename_all="snake_case"`，值 `fail`）。
- 缺省语义：`key_field=None`、`max_items=None`（无上限）、`on_item_error=None`（等价 `fail`）。**缺省不写回 DSL、不参与 hash**（§5）。

### 2.3 编译映射（W4b）

| DSL | `CompiledNode` | 说明 |
|---|---|---|
| `nodes[].foreach` | `.foreach: Option<CompiledForeach>` | 原样带出；`OnItemError` 编译为 `Fail`（v0.2 唯一值） |
| `subgraphs` | 不允许 | R6 收敛 |

---

## 3. 校验规则 R17 / R18（F2）

### 3.1 R17 形状（R4 阶段；复用 `DSL_PARSE_ERROR`）

| 场景 | `code` | message（冻结） | `expected` | `actual` | `location` |
|---|---|---|---|---|---|
| foreach 非 object | `DSL_PARSE_ERROR` | `field 'foreach' expects object, got {actual_type}` | `"object"` | 原值/类型 | `node_id`=F |
| 未知键 | `DSL_PARSE_ERROR` | `unknown field '{key}' in foreach of node '{F}'` | 白名单数组（固定 5 键） | key 原名 | `node_id`=F |
| 缺必填 `items_port`/`join` | `DSL_PARSE_ERROR` | `foreach of node '{F}' missing required field '{field}'` | `"present"` | `null`（缺省） | `node_id`=F |
| `items_port` 非 string/空/未声明 | `DSL_PARSE_ERROR` | `'foreach.items_port' of node '{F}' must be a declared input port, got {actual}` | F 的端口名数组 | 原值 | `node_id`=F |
| `key_field` 非 string/空 | `DSL_PARSE_ERROR` | `'foreach.key_field' must be a non-empty string, got {actual}` | `"non-empty string"` | 原值 | `node_id`=F |
| `join` 非 string/空 | `DSL_PARSE_ERROR` | `'foreach.join' must be a node id, got {actual}` | `"node id"` | 原值 | `node_id`=F |
| `max_items` 类型/取值非法 | `DSL_PARSE_ERROR` | `'foreach.max_items' must be an integer >= 1, got {actual}` | `">= 1"` | 原值 | `node_id`=F |
| `on_item_error` 非闭集 | `DSL_PARSE_ERROR` | `'foreach.on_item_error' must be one of ['fail'], got {actual}` | `["fail"]` | 原值 | `node_id`=F |
| 节点类型不支持 | `DSL_PARSE_ERROR` | `node '{F}' of type '{type}' does not support foreach` | `"python_node|agent_call"` | 实际 type | `node_id`=F |

- R17 是 R4 的**扩展**（同一阶段/同一错误码；不新增 DSL 码）；每节点按 `nodes` 数组序、每字段按上表顺序报告；同节点多项错误**聚合**（R4 既有口径）。
- 子图内 foreach 的形状错误：由 R6 递归执行 R4 → 收敛 `DSL_SUBGRAPH_INVALID`（保留内部 node_id/expected/actual）。

### 3.2 R18 区域（新规则；新 DSL 码 `DSL_FOREACH_INVALID`，DSL 域 18→19）

**定义**：对每个 F（按 `nodes` 数组序，取首错）：
- `J = F.foreach.join`；
- `R = {F} ∪ {v | v 从 F 沿出边可达且路径不经过 J}`（有向图、全部边含 gate/defer；不经过 J 指路径内部不出现 J）。

**判定（reason 闭集，按此顺序）**：

| # | 检查 | reason | 说明 |
|---|---|---|---|
| 1 | `J` 存在 | `join_not_found` | join 指向不存在 node_id |
| 2 | `J != F` 且 `J ∉ R` | `join_in_region` | join 不得在 region 内（等价 J==F） |
| 3 | `J.type ∈ {python_node, agent_call, end}` | `join_type_unsupported` | 禁止 start/subgraph 等 |
| 4 | R 内节点类型 ∈ {python_node, agent_call}（含 `builtin.human`）；不得含 start/end/subgraph/**其它 foreach 节点** | `forbidden_node_type` / `nested_foreach` | **row4 内顺序冻结**：先判类型（start/end/subgraph → `forbidden_node_type`），再判 `foreach`（→ `nested_foreach`）；R 内任一节点带 `foreach` 即禁止（含间接嵌套） |
| 5 | R 有且仅有 1 个出口 E（R 内节点中「存在出边指向 R 外」的节点数恰为 1） | `no_exit` / `multiple_exits` | 无 → no_exit；>1 → multiple_exits |
| 6 | E 恰有 1 条出边且目标=J；该边**不得有 gate** | `external_edge` / `join_edge_gated` | E 的其它出边/目标非 J → external_edge；E→J 有 gate → join_edge_gated |
| 7 | R 内节点（除 E）的全部出边目标 ∈ R | `external_edge` | region 不得泄漏 |
| 8 | R 内节点（除 F）的全部入边来源 ∈ R 或 = F；**F 的全部入边来源 ∉ R**（F 入边来自 R 外） | `external_edge` | 外部节点不得直接投递进 region（跨 item 语义未定义）；**controller buffer 只接收 R 外投递**，R 内（含回边）→F 会破坏单次展开/跨 item 语义 → 拒绝（C1） |
| 9 | F 的 `items_port` 由 F 的入边提供（R12 不变） | `items_port_missing` | 端口已声明但无任何入边供源 |
| 10 | R 与其它 foreach 的 region **不相交** | `region_overlap` | 一个节点只能属于一个 region（F 可为另一 foreach 的 J，见串行） |

- **错误载荷（冻结）**：`code=DSL_FOREACH_INVALID`；`message = "foreach region of node '{F}' is invalid: {reason}"`；`expected = "valid foreach region"`；`actual = {"node_id": F, "reason": reason}`；`location.node_id = F`。
- **首错**：按 `nodes` 数组序取首个非法 F（不聚合跨 F）；单个 F 的检查按上表顺序取首因。
- **子图**：子图内不允许 foreach（R17/R6 已拦）；R18 不适用于子图（若实现路径出现，按 R6 `DSL_SUBGRAPH_INVALID` 收敛）。
- **执行顺序（W4a 冻结）**：`R1→R13→R15→R18→R16→R14`；R18 在 R15（defer 可重入）之后、R16（每节点 max_fanout）之前，R14（外部引用）最后。`--structure-only` 含 R17/R18。
- **R13/R15 不变**：region 内允许带 gate 的返工环（defer 语义不变）；R13 只看无 gate 环、R15 只看 defer 边源可重入，均按**全图**执行，不因 region 收窄或放宽。
- **R16（W2b）关系**：`max_fanout` 仍是「每个 item 实例一次激活的放行出边数上限」；**不限制 N**（N 由 `max_items`/列表长度决定）；F/region 节点的 `max_fanout` 照常由 R16 校验其静态出度。

### 3.3 正反例（设计，fixtures 由 W4b dsl 交付）

| # | 示例 | 结论 |
|---|---|---|
| P-1 | `start → F(foreach items_port="items", join="J") → W1 → E → J → end`；`items` 由 start 提供 | 合法；R={F,W1,E} |
| P-2 | `start → F(foreach) → E(=F) → J(agent_call)`（单节点 region，F 自身为出口） | 合法（R={F}，E=F） |
| P-3 | F 的 join=J 同时是另一个 foreach 的 F（串行 foreach 链） | 合法（J 不属于前一 region） |
| N-1 | `join` 不存在 | `join_not_found` |
| N-2 | `join=F` | `join_in_region` |
| N-3 | region 内节点有出边指向外部 X | `external_edge` |
| N-4 | region 有两个出口 | `multiple_exits` |
| N-5 | `E→J` 边带 gate | `join_edge_gated` |
| N-6 | region 内含另一个 foreach 节点 | `nested_foreach` |
| N-7 | region 含 `start`/`end`/`subgraph` | `forbidden_node_type` |
| N-8 | `join` 指向 `start` | `join_type_unsupported` |
| N-9 | F 的 `items_port` 无入边 | `items_port_missing` |
| N-10 | 两个 foreach 的 region 共用节点 | `region_overlap` |
| N-11 | `start` 节点带 `foreach` | R17 `DSL_PARSE_ERROR` |
| N-12 | `foreach.on_item_error="skip"` | R17 `DSL_PARSE_ERROR` |
| **N-13（C1）** | region 内节点（含 region 回边）有入边指向 F | `external_edge`（F 的入边必须来自 R 外） |

---

## 4. 区域生成与边界算法（W4b 实现依据）

```text
for F in nodes (nodes order):
    if F.foreach is None: continue
    J = F.foreach.join
    if J not in nodes: error(R18, join_not_found)
    R = reachable_from(F, stop_before=J)       # DFS/BFS 确定性：按 nodes 序 + edges 序
    if J == F or J in R: error(R18, join_in_region)
    if J.type not in {python, agent, end}: error(...)
    exits = [v in R if exists edge v->u with u not in R]
    if not exits: error(no_exit); if len(exits)>1: error(multiple_exits)
    E = exits[0]
    out_e = edges from E
    if len(out_e)!=1 or out_e[0].to_node != J: error(external_edge)
    if out_e[0].gate is not None: error(join_edge_gated)
    for v in R:
        if v != E and any(out_edge.to_node not in R): error(external_edge)
        if v != F and any(in_edge.from not in R and in_edge.from != F): error(external_edge)
        if v == F and any(in_edge.from in R): error(external_edge)   # C1：F 的入边必须来自 R 外
    if F.foreach.items_port not provided by any in-edge: error(items_port_missing)
    if R intersects previous region: error(region_overlap)
```

- 遍历确定性：`reachable_from` 用 `nodes` 数组序 + 每节点出边按 `edges` 数组序展开（BFS 队列），保证同一图同一错误。
- R 可以包含 F 自身（F 有出边到 region 内）或 R={F}（F 直接 E→J）。
- **E 可以是 F 吗**？可以（R={F} 或 F 为唯一出口时）；此时 F 的 item 输出即 join 元素。
- J 可同时是另一 foreach 的 F（串行），因为 J ∉ 前一 region；但两 region 不得相交。

---

## 5. hash 冻结（F4）

- canonical projection（02 §2.8.3）的**节点对象**新增 `foreach` 键：**仅作者显式写时**投影（原样对象）；缺失不补齐、不填默认值。
- `max_items`/`on_item_error` 缺省若作者未写 → 不投影；显式写 `"on_item_error":"fail"` → 投影（语义等价但 hash 不同，沿用既有「显式 vs 缺省不归一」口径）。
- 无 foreach 的图：投影不变 → **hash 逐字节不变**（v0.1.0/W1/W2a/W2b/W3a golden 全量回归）。
- `graph_hash` 只随图 JSON 变化；运行期 N/列表内容/展开事实**不影响** hash。

---

## 6. 运行期模型（F5–F8）

### 6.1 无 controller 实例（F5）

- F 的入边投递**照常求值 gate 并记 `GateResult`**（`to_node=F`），但**不创建 F 的单实例**、不进入普通 Ready/Running；**只接收 R 外投递**（R18 row8/C1 保证）。
- 内核维护 **F controller buffer**（01 §1.2.5 `ControllerBuffer`）：按 F 的 `trigger`（all/any）与声明端口判定就绪；就绪后读取 `items_port` 的值（§6.2）→ 触发展开。
- **controller 状态机（C3 冻结）**：`Collecting`（已收到部分/全部入边但未就绪）→ `Ready`（trigger 满足且 `items_port` 已到）→ `Expanded`（item 实例已创建）或 `Skipped`。**只有 `Ready` 才展开**；`Expanded` 后任何到 F 的投递 → `RUN_DELIVERY_TO_TERMINAL`（复用；controller 单次展开）；`Skipped` 后投递同样拒绝。
- **items_port 未到**：即使其它端口满足 trigger，也**不得进入 Ready/展开**；若 F 的入边已全部结算且 items_port 始终无数据 → controller → `Skipped`（region 不展开，按 §6.6/§6.7 正常 skip 级联）；若仍有在飞/未结算入边，静默时按 §6.7 判 `RUN_DEADLOCK`。
- 展开事实：`expansion_id`（UUIDv7）+ `item_count` + `keys`（item_key 数组，NULL 键以结构化 `{"$index": i}` 表示（避免与真实 string key 撞表示））；落 trace `foreach_expansions`（§8）。
- **controller buffer 不产生实例/不派发**；`instances` 中不出现 F 的「非 item」实例（F 的 item 实例仍按 §6.3 创建，item_index 0..N-1）。
- 若 F 的 `trigger=any`：controller buffer 在至少一个 required 端口到达时可展开；展开时其它端口取当前 buffer（未到为 null）——与普通节点 `trigger=any` 的就绪口径一致（04 §4.4.2 / 01 §1.3）。

### 6.2 展开校验（F8，run 级失败，无实例 Crashed）

| 检查（按顺序） | 失败 → `RunFinished.error.code` | message（冻结） |
|---|---|---|
| `items_port` 值不是 JSON array | `RUN_FOREACH_ITEMS_NOT_ARRAY` | `foreach node '{F}' items_port '{p}' is not an array (got {actual_type})` |
| `N > max_items`（声明时） | `RUN_FOREACH_ITEMS_EXCEEDED` | `foreach node '{F}' items {N} exceed max_items {limit}` |
| `key_field` 派生键重复（N>1） | `RUN_FOREACH_ITEM_KEY_DUPLICATE` | `foreach node '{F}' item key '{key}' is duplicated at indexes {i},{j}` |
| N=0 | **合法**（不报错） | —（立即投递 `[]` 给 J） |

- **item_key 派生（冻结，单一陈述；C4/N17）**：`key_field` 缺省、元素非 object、或 `key_field` 值缺失/null/object/array → `item_key=NULL`（按索引派生，trace 以 `{"$index": i}` 表示；**不报错**）；元素为 object 且 `key_field` 为 string → 原值；number/bool → canonical JSON 字符串（`1`/`1.5`/`true`）。只有**派生出非 NULL 且重复**才 `RUN_FOREACH_ITEM_KEY_DUPLICATE`（例如两个对象都 `id=1`）。
- 展开校验失败 → 冷峻 `fail_run`（run Failed；不创建任何 item 实例；controller buffer 清空）；trace 写 `RunFinished.error`（与既有 run 级错误同路径）。
- **N 上限**：`max_items` 只约束 N；W2b `max_fanout` 不限制 N（§3.2）。`max_items` 缺省 = 无上限（由内存/运行期风险文档声明，§13）。

### 6.3 item 实例与定位（F6）

- N>0 时，为 **R 内每个节点**创建 N 个 item 实例（item_index 0..N-1）；**F 自身也执行 N 次**（per-item handler：`items_port` 值=item[i]，其它 F 输入端口值沿用 controller buffer）。
- 实例定位键：`(node_id, item_index)`；`RunState.node_current`/寻址/自环/defer 重入/attempt 全部按 `(node,item)` 作用域；非 region 节点 `item_index=0`。
- **attempt 语义**：按 `(node,item)` 独立（自环/defer 重入只影响对应 item）；`max_attempts` 仍由节点 config 决定。
- **边投递**：region 内的边（含 F 的出边）只在同一 `item_index` 内投递；跨 item 的投递**禁止**（R18 边界已保证 region 闭合）。
- **per-item gate**：每条边按 item 实例的 output 求值；`GateResult` 的 `instance_id`=item 实例、`to_node`=目标节点（item 投递）。
- **item 实例状态机**：与普通实例一致（Pending/Ready/Running/Completed/Crashed/TimedOut/Cancelled/Skipped/WaitingHuman）。
- **F 的 item 实例**：item_index=i 的 F 实例消费 item[i] 作为 `items_port` 输入；F 不产生 controller 级别的实例。

### 6.4 展开触发与竞态

- controller buffer 就绪 → 展开（创建 item 实例）是**事件循环内同步/确定性**动作；展开后 controller buffer 清空，不再接受 F 的入边投递（F 不可重入/不可二次展开；如需串行多次展开，用多个 F）。
- 展开与取消/暂停：取消优先（展开前 cancel → 不展开，run Cancelled）；Pause 时 controller buffer 可继续接收投递，但**不展开**（展开属派发语义，Pause 冻结；见 §6.7）。
- 展开失败（§6.2）→ fail_run；若同批还有其它事件，按既有终态短路丢弃。

### 6.5 join（F7；来源/GateResult 冻结 C2）

- R 唯一出口 E 的每个 item 实例 Completed 后，内核记录该 item 的 output（按 item_index）。
- **全部 N 个 item 的 E 都 Completed** 后，按 item_index 升序构造数组 `[E.output_0 .. E.output_{N-1}]`，作为**一次投递**发给 J：
  - 目标端口 = `E→J` 边的 `to_input_port`；
  - **N>0 的 source（冻结）**：`Delivery.from_instance` = **最后完成使 join_buffer 达到 N 的那个 E item 实例**；`GateResultRecord.instance_id`/`attempt` = 该 E item 实例的 id/attempt；`to_node=J`、`edge=E→J`；该实例由计数触发，顺序确定（事件循环串行处理完成事件，最后一条唯一）。
  - **N=0 的 source（冻结）**：无 E item 实例 → **不产生 `GateResult`、不产生 `Delivery`**；controller 直接把 `[]` 写入 J 的输入 buffer（`instances.input` 快照可见 `[]`），J 按普通就绪/结算继续。**J 实例创建/边结算（N24）**：注入等价于 entry 外部注入——若 J 尚不存在则创建 J 实例（`item_index=0`）并写 input；`E→J` 边不产生 Delivery/GateResult（无 E 实例）；J 的 `instances.input`/`transition` 为证据。trace 证据 = `foreach_expansions(count=0)` + J 的 input/transition（**无** GateResult 行）。
  - J 为普通节点 item_index=0；J 为 `end` 时走 end 聚合（`run_output` 对应端口为数组）。
- join 投递（N>0）是**普通 Delivery**（不是新事件类型）；`events.kind` 不变。
- J 的 `trigger=all` 时需等其它入边到齐（正常语义）；J 可使用 `check_output` 等 gate 处理数组（整包广播）。

### 6.6 失败/取消/上限（F8）

| 场景 | 行为 |
|---|---|
| `on_item_error=fail`（v0.2 唯一）且 R 内任一 item 实例 Crashed/TimedOut | 既有冷峻 `fail_run`（run Failed；其他 item 实例转 Cancelled；`killed_workers` 按既有语义） |
| R 内任一 item 的 E 变为 `Skipped`（入边全被门控拦截） | `RUN_FOREACH_ITEM_INCOMPLETE` → run Failed（message：`foreach node '{F}' item {i} exit node '{E}' was skipped`） |
| run 取消 | 全部 item 实例 Cancelled；run Cancelled（既有取消路径） |
| Pause | 冻结 item 派发（在飞照常）；controller 展开在 Pause 下冻结（§6.4） |
| 超时 | 每个 item 实例按自身 `timeout_ms` 计时（从派发起）；到期 TimedOut → 冷峻 fail_run |
| `max_items` 超限 | `RUN_FOREACH_ITEMS_EXCEEDED`（展开前，§6.2） |
| items_port 非数组 | `RUN_FOREACH_ITEMS_NOT_ARRAY` |
| item_key 重复 | `RUN_FOREACH_ITEM_KEY_DUPLICATE` |
| W2b max_fanout | 只限制每个 item 实例一次激活的放行出边数；**不限制 N** |
| W3a human | region 内 human 节点每 item 独立实例/等待/decide；`human_decisions` 按 instance_id 唯一（不需改表） |
| **controller：入边全部 settle=false（无数据）** | controller → `Skipped`（不展开、不创建 item 实例）；region 节点不创建；F 的出边按普通 skip 级联（E→J 无数据 → J 按普通规则 skip/等其它入边）。**controller-origin skip 的边结算（N25）**：与 N=0 同路径——不产生 GateResult/Delivery；J 若其它入边未到则不实例化/按普通 skip 或等待（静默后既有死锁判定），J 的 input/transition 为证据 |
| **controller：部分入边到达、required 未满足且无在飞** | 静默后 `RUN_DEADLOCK`（message 含 F；03 §3.10） |
| **controller：items_port 无数据（settle=false）** | controller → `Skipped`（不展开）；若 items_port 仍有在飞投递，静默时按 `RUN_DEADLOCK` |
| **defer 边以 F 为目标** | gate=true → 正常进入 controller buffer（展开前）；gate=false → 挂 `DeferredReceipt`，静默时 F 无实例 → 既有确定性 `RUN_DEADLOCK`；F 已 `Expanded`/`Skipped` 后任何投递（含 defer gate=true）→ `RUN_DELIVERY_TO_TERMINAL` |

### 6.7 终止判定（03 §3.10 扩展）

```text
is_quiescent(run) =
   no instance with status ∈ {Pending, Ready, Running, WaitingHuman}
   AND no live controller with state ∈ {Collecting, Ready}   # Expanded/Skipped 不算活跃（C3）
   AND ready_queue.is_empty()
   AND no pending deliveries in flight
```

- **controller 活跃性（C3 冻结）**：`Collecting`/`Ready`（未展开、未 skip）视为非静默；`Expanded`/`Skipped` 不算活跃。静默后：
  1. 若存在 controller ∈ {Collecting, Ready}（未 Pause）→ `RUN_DEADLOCK`（message 含 F 与控制器状态）；
  2. 若存在 defer receipt 目标 F 无任何实例 → 既有确定性 `RUN_DEADLOCK`（§6.6 defer 行）；
  3. 否则正常完成/聚合（部分 item 未完成按 §6.6 失败或 E skipped 检测；正常 N 完成才 join）。
- Pause 下 controller 不展开、不判死锁（等 Resume/Cancel）。

---

## 7. 错误码与失败矩阵（F2/F8）

### 7.1 DSL 域（18→19）

| 码 | 触发 | 落点 |
|---|---|---|
| `DSL_FOREACH_INVALID`（**W4a 新增，第 19 码**） | R18 区域/边界/嵌套/类型/join 非法；reason 闭集 11 值 | 02 §2.4.1 R18/§2.4.2 |

- R17 复用 `DSL_PARSE_ERROR`；子图内收敛 `DSL_SUBGRAPH_INVALID`；其余 DSL 域既有 18 码不变。

### 7.2 运行期域（新增 4 个 run 级码）

| 码 | 触发 | 影响 |
|---|---|---|
| `RUN_FOREACH_ITEMS_NOT_ARRAY` | `items_port` 值非 JSON array | run Failed（`RunFinished.error`；无 item 实例） |
| `RUN_FOREACH_ITEMS_EXCEEDED` | N > `max_items` | run Failed（展开前） |
| `RUN_FOREACH_ITEM_KEY_DUPLICATE` | `key_field` 派生非 NULL 键重复 | run Failed（展开前） |
| `RUN_FOREACH_ITEM_INCOMPLETE` | R 内任一 item 的 E 变 Skipped（未完成 join） | run Failed（冷峻；其余 item 取消） |

- 这些是 **run 级**错误（不是 `NodeError`；无实例 Crashed；`runs.error_code`/`RunFinished.error` 承载）。
- 既有 0/1/2/3/4/5/70 退出码不变；run 级 FOREACH 错误 → run Failed → CLI exit 1。

### 7.3 失败矩阵（汇总）

| 场景 | run 结果 | trace |
|---|---|---|
| 任一 item 节点 Crashed/TimedOut（on_item_error=fail） | Failed（既有冷峻；`error.code`=节点错误码） | 该 item Crashed/TimedOut + RunFinished.error |
| 回边/出口 Skipped | Failed `RUN_FOREACH_ITEM_INCOMPLETE` | Skips + RunFinished.error |
| items not array / exceeded / duplicate key | Failed（对应 RUN_FOREACH_*；无 item 实例） | expansion 不落（校验失败）；RunFinished.error |
| 空列表 | Completed（join `[]`） | expansion(count=0)+join GateResult |
| 正常 | Completed | item transitions/gate results + expansion + join |

---

## 8. trace schema v6（F9）

### 8.1 版本与迁移（06 §6.2.2/§6.7）

- `TRACE_SCHEMA_VERSION = 6`；新库单事务建 v6 DDL + `schema_migrations` 1..6 六行 + `PRAGMA user_version=6`（无备份）。
- 旧库逐版本迁移 v1→…→v6，每步 `VACUUM INTO` 备份、单版本单事务、失败恢复。
- 只读 `trace`/`replay` 对 `<6` 拒绝（`unsupported schema version=N, expected 6`）；`>6` → `STORE_SCHEMA_TOO_NEW`。

### 8.2 v5→v6 迁移步骤

```text
1) VACUUM INTO '<db>.bak.v5-{ts}'；回写备份 user_version=5 并校验
2) 单事务：
   a) 重建 instances（12 步法）：
      新列 item_index INTEGER NOT NULL DEFAULT 0、item_key TEXT；
      UNIQUE(run_id,node_id,attempt) → UNIQUE(run_id,node_id,attempt,item_index)；
      显式列清单复制（旧行 item_index=0、item_key=NULL）；重建 3 索引 + 新增 (run_id,node_id,item_index) 索引
   b) CREATE TABLE foreach_expansions（06 §6.2.15）+ INDEX(run_id)
   c) INSERT schema_migrations(6)；PRAGMA user_version=6
3) 提交；失败 → 删 -wal/-shm → 备份覆盖 → exit 4
```

- `human_decisions`/`recorded_outputs` **结构不变**（instance_id 唯一已可区分 item）；`events` 不重建、不重写。
- 旧 run（无 foreach）迁移后逐列一致、`trace`/`replay` golden 逐字节不变（item_index 全 0、无 expansion 行）。

### 8.3 `foreach_expansions` 表（06 §6.2.15）

```sql
CREATE TABLE IF NOT EXISTS foreach_expansions (
  run_id            TEXT NOT NULL REFERENCES runs(run_id) ON DELETE CASCADE,
  expansion_id      TEXT PRIMARY KEY,              -- UUIDv7
  node_id           TEXT NOT NULL,                 -- F
  source_instance_id TEXT,                          -- 触发展开的投递实例（选最后一条使 controller 进入 Ready 的投递来源；entry 注入/无实例时 NULL——N18 冻结）
  item_count        INTEGER NOT NULL CHECK (item_count >= 0),
  keys_json         TEXT,                           -- JSON array；NULL 键表示为 {"$index": <i>} 对象（真实 string key 保持字符串）
  at                TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_foreach_expansions_run ON foreach_expansions(run_id);
```

### 8.4 trace 输出（06 §6.10.4）

- 人类输出：事件时间线后追加 `foreach node='<F>' items=<N> keys=[...]` 行（无 foreach 不输出，默认输出逐字节不变）。
- `--json`：事件行后追加 `{"kind":"ForeachExpansion","run_id","expansion_id","node_id","item_count","keys","at"}` NDJSON（按 `(at, expansion_id)` 升序）；**不写入 `events.kind`**；无 foreach run 输出不变。
- `--stats` 不新增字段（后续可另立）。

### 8.5 replay（06 §6.10.7）

- L1 `--mode=events` 不变（events 逐字节）。
- L2 `--mode=outputs`：`recorded_outputs` 表结构不改；重放查找键扩展为 `(node_id, attempt, item_index)`（item_index 由 instances 表提供实例 id），按 expansion item_count/keys 校验完整性；item 输出按记录重放，不重新要求人工决策；不重跑真实 worker。

---

## 9. CLI 影响（F10）

| 命令 | 变化 |
|---|---|
| `zgraph run` | 无新参数；foreach 完全由图 JSON 驱动；run 级 FOREACH 错误 → Failed + exit 1（既有错误输出形状） |
| `zgraph validate` | R17/R18 生效；`--structure-only` 含 R17/R18；退出码不变（DSL 错误 2） |
| `zgraph trace` | 人类输出 + `--json` ForeachExpansion 行（§8.4）；其余不变 |
| `zgraph replay` | `--mode=events` 不变；`--mode=outputs` per-item 重放（§8.5）；退出码不变 |
| `zgraph decide` | 不变（region 内 human 每 item 独立 instance，decide 按 instance_id） |

- 不新增退出码/不新增 CLI 参数/不改 `--json` 既有键形状（仅追加行）。

---

## 10. SOP 设计稿 v1.2（F11）

- 新增 **dynamic-N profile**：设计角色 D 的上游列表（entry input 或上游产物）→ 工作 agent 节点声明
  `foreach: {items_port, key_field, join}` → per-item W（工作）/R（评审）/P（解析）链 → join 到下一设计角色（有序 artifact 列表）。
- **静态 v1/v1.1 profile 保留兼容**（approved-only / rework）；dynamic-N 是第三个 profile，不替换静态。
- §7 codegen：dynamic profile **不再按 N codegen**（同一张图跑任意 N）；静态 profile 的 codegen `--mode` 保留。
- §9 延期项移除「动态 N」；W5 共享评审队列仍延期。
- 只写设计，不写 as-built（W4b-docs 回写）。

---

## 11. AC-17 验收矩阵（设计冻结；W4b 实测）

| # | 场景 | 图/资产（设计） | 关键断言 |
|---|---|---|---|
| a | entry 列表 3 items 展开 | `foreach_dynamic/entry_items3.json`：`start(items=[...]) → F(foreach) → W1 → E → J → end`；真 CLI | 3 个 item 实例/节点 item_index 0..2、独立 transition/attempt/worker；join 有序数组 `[r0,r1,r2]`；run Completed；expansion 行 count=3/keys |
| b | 上游输出列表展开（非 entry） | `.../upstream_items.json`：上游 python 节点输出列表 → F(foreach) | N 由运行期数据决定（非 codegen）；join 顺序=item_index |
| c | 空列表 | `.../empty_items.json`：items=[] | 无 item 实例；expansion count=0；J 收 `[]`；run Completed |
| d | item 失败（冷峻） | `.../item_failure.json`：某 item 节点抛错 | run Failed；`error_code`=节点错误码；其他 item 取消；无 join 投递 |
| e | key_field 重复 | `.../dup_keys.json`：两个元素 `id=1` | `RUN_FOREACH_ITEM_KEY_DUPLICATE` Failed；无 item 实例 |
| f | max_items 超限 / items 非数组 | `.../exceeded.json`、`.../not_array.json` | `RUN_FOREACH_ITEMS_EXCEEDED` / `RUN_FOREACH_ITEMS_NOT_ARRAY` Failed |
| g | 取消传播 | 运行中取消 | 全部 item 实例 Cancelled；run Cancelled；`RunFinished{Cancelled}` |
| h | region 内 human（2 items） | `.../human_items2.json`：region 内 `builtin.human` | 2 个 WaitingHuman item 实例；分别 decide；join 有序结果；`human_decisions` 2 行（不同 instance_id） |
| i | trace v6/回放/golden | v5 seed + 新库；v0.1.0 图 | v5→v6 迁移（备份/只读拒绝/v3→v6 链 golden）；L1 events 回放；L2 per-item outputs 回放；graph_hash 只随图 JSON；无 foreach 图 golden 不变；N=0 expansion 行 + J input `[]`、**无 GateResult**（C2） |
| **j** | **ITEM_INCOMPLETE + key canonical（C5）** | `.../exit_skipped.json`、`.../keys_canonical.json` | 某 item 的 E 因门控 false 全部 Skipped → `RUN_FOREACH_ITEM_INCOMPLETE` Failed、无 Crashed、其余 item Cancelled、无 join 投递；key canonical：number→`"1"`、bool→`"true"`、null/object/array→`{"$index": i}`、string 原值、非 NULL 重复→`RUN_FOREACH_ITEM_KEY_DUPLICATE` |

> **AC-17 覆盖（C5/N19）**：4 个 `RUN_FOREACH_*`（c/f/j + d 节点失败）与 5 类边界（controller 未就绪/全部 settle=false/items_port 未到/defer→F/F 入边来自 R）均需至少一条负例或断言；W4b 实现时补齐测试名。

> W4b 实现后由 W4b-docs 回写实测测试名/计数；本批不写 as-built。

---

## 12. 兼容性与回归矩阵

| 类别 | 用例 | 期望 |
|---|---|---|
| hash | v0.1.0/W1/W2a/W2b/W3a 图（无 foreach） | 逐字节不变 |
| validate | 无 foreach 图 | 错误码/定位/`error_count` golden 不变 |
| run/trace | 无 foreach 图 | 行为/golden/回放不变；instances 新列默认 0/NULL 不改输出 |
| DSL | 既有 18 码 + R1–R16 | 不变（仅新增 R17/R18 与第 19 码） |
| trace | v5 库只读 | 拒绝（exit 4，不迁移） |
| trace | v5 库 run 打开 | 迁移 v6（备份/单事务/重建 instances/新表）；旧 run 回放不变 |
| run | region 内含 defer/环/human/W2b | 复用既有语义（per-item 作用域） |
| events | 所有场景 | `events.kind` 不变（展开事实落新表） |

---

## 13. 风险与缓解

| 风险 | 缓解 |
|---|---|
| N 过大导致实例/内存爆炸 | `max_items` 可选上限（缺省无上限是显式契约，文档警示）；N 只由作者声明约束；W2b 不限制 N |
| 区域判定不确定/复杂图误判 | R18 冻结确定性算法（nodes/edges 序；11 reason 闭集）；正反例 fixtures |
| item 实例寻址与既有单实例假设冲突 | 定位键统一 `(node,item)`；非 region item_index=0；迁移旧行为 0；单测覆盖自环/defer/human |
| join 不完整/遗漏 | E skipped → `RUN_FOREACH_ITEM_INCOMPLETE`；全部 E Completed 才投递 |
| 回放与 item 顺序漂移 | item_index 升序 + expansion keys 校验；L2 记录键含 item_index |
| v6 迁移破坏旧 golden | 旧行 item_index=0、无 expansion；events 不改写；v5 seed golden 回归 |
| human 在 foreach 内的决策歧义 | instance_id 唯一（每 item）；decide 不变；AC-17 h |
| 默认图回归 | 无 foreach 的代码路径零分支；全量 golden + workspace 回归 |

---

## 14. W4b 实施顺序与影响文件

| 顺序 | 模块 | 分支（DR-004 §4） | 依赖 | 关键交付 |
|---|---|---|---|---|
| 1 | dsl | `feat/dsl-foreach` | 本批 | `GraphNode.foreach`/白名单/serde/hash；R17（R4）/R18（新码 19）；顺序 R1→R13→R15→R18→R16→R14；fixtures（valid_foreach/bad_foreach_*）与 fixtures 表 |
| 2 | kernel + trace | `feat/kernel-dynamic-fanout`、`feat/trace-dynamic` | dsl | controller buffer/展开/item 实例/join/错误码/取消/Pause；trace v6（instances 列/唯一键重建、foreach_expansions、迁移、trace 输出） |
| 3 | cli | （如需，最小） | kernel/trace | trace/replay 输出扩展；run 无新参数；退出码不变 |
| 4 | e2e | `test/e2e-dynamic-fanout` | 全部 | AC-17 a–j 真链路 + v5 seed 迁移 + golden |
| 5 | docs | `docs/docs-dynamic-spec`（W4b-docs） | 实现合并 | as-built 回写/AC-17 实测/08 注记/PROVENANCE 第 2 条 |

**影响文件（W4b，非本批）**：`crates/zgraph-dsl/**`（types/nodes/edges/hash/lib/tests/fixtures）、`crates/zgraph-kernel/**`（types/graph/run/trace/tests）、`crates/zgraph-trace/**`（schema/model/reader/expansions/tests）、`crates/zgraph-cli/**`（trace/replay/run 输出）、`tests/e2e/**` + `tests/fixtures/graphs/foreach_dynamic/**` + trace seeds。

---

### 14.1 W4b 实现执行记录（as-built，2026-11-01，REV-094；只追加）

| 模块 | 分支 / 实现提交 | 评审 | 合并点（main） | 测试证据 |
|---|---|---|---|---|
| dsl | `feat/dsl-foreach` / `9efc41c` | REV-088（reviewer-a；fixture 跨模块 Lead 第二评审） | `45cd7c6` | dsl **118/0**；fixtures 56 bad/8 valid；既有 42 fixture A/B 84/84 |
| trace | `feat/trace-dynamic` / `49806c2` + 修复 `dc25340` | REV-089（reviewer-b；首轮不通过 → 修复后通过；跨模块第二评审 lead） | `948d677` | trace **94/0**（migration 18/expansions 5/item 5）；workspace 834/0/3 |
| kernel | `feat/kernel-dynamic-fanout` / `53b42ca` + `211c2be` + 修复 `f6e8ed9` | REV-090（reviewer-b；有条件通过 → 复审通过；cli 第二评审 lead） | `519066d` | kernel **149/0**、test-hooks **159/0**、cli **222/0**；`foreach.rs` 25 |
| fix | `fix/kernel-foreach-items-unwrap` / `8846240` | REV-092（reviewer-b） | `683260b` | kernel 149/159、cli 222、workspace 894/0/3 |
| cli | `feat/cli-dynamic` / `59c7443` | REV-091（reviewer-a） | `4a1ff0f` | cli **222/0**（`foreach_dynamic.rs` 9）；workspace 889/0/3 |
| e2e | `test/e2e-dynamic-fanout` / `a82943d` + `e1fdcd1` | REV-093（reviewer-b） | `201341b`（e2e 评审后 main 头） | e2e **106/0/2**（dynamic_fanout **21/0**，6 连跑）；workspace **915/0/3**；全量 e2e 3 连跑稳定 |
| docs | `docs/docs-dynamic-spec`（本批） | REV-094（reviewer-a） | 本分支待合并 | check.sh / PROVENANCE §3 实测 |

**as-built 偏差（已评审裁决/接受）**：

- **items_port 取值规则 A（REV-092，v1 整包广播）**：`v=controller.values[items_port]`；array 直用；**object 且含与 items_port 同名键 → 取该键值（必须 array）**；否则 `RUN_FOREACH_ITEMS_NOT_ARRAY`；只取一层、不点分。NOT_ARRAY→EXCEEDED→DUPLICATE 顺序作用于 unwrap 后列表。
- **region_overlap 防御性不可达**（row4/row8 先拦截；dsl 单测锁定优先级）；**子图内 foreach 归 R6**（精确 message/location 已由 rules 锁定）。
- **controller Pause/Resume**：Resume 后 `expand_ready_controllers` 按 node_order 循环重扫 Ready controller（串行 F 链）；`source_instance_id` 只在 Collecting 记录；F 已 Expanded/Skipped 后任何投递（含 defer gate=false）→ `RUN_DELIVERY_TO_TERMINAL`；`incomplete_join` 防御。
- **join/item 终态**：join source=最后完成使 join 满 N 的 E item；N=0 无 GateResult/Delivery；冷峻失败只取消失效中 item，已 Completed 保留（e2e d/j 差异即此时序）。
- **trace v6 迁移**：FK CASCADE 窗口（事务外 OFF/恢复 ON）；真实 v1→v6（v6-only 索引移出 `DDL_CURRENT`；**ISSUE-142 P1 fixed**，REV-089）。
- **cli 输出**：人类摘要+`source_instance/at` 明细；`--json` 增 `source_instance_id`（null 不省略）；行序 events→HumanDecision→ForeachExpansion；L2 预检 CORRUPT/MISSING/Skipped 合法。
- **遗留**：ISSUE-142（fixed）、ISSUE-143（e2e NOT_ARRAY 缺 object-无同名键变体）等；REV-088/090/091/093 的 P3 见台账。

## 15. 冲突、边界与待确认项

### 15.1 与既有 spec/DR 的冲突及处置

| # | 冲突点 | 处置 |
|---|---|---|
| C-1 | 01 单实例假设（`node_current: HashMap<NodeId, InstanceId>`）vs item 实例 | 定位键扩为 `(node_id, item_index)`；非 region 为 0；01 §1.2.5 冻结 |
| C-2 | 02 `GraphNode` 白名单 8 键 vs 新 `foreach` | 白名单加 `foreach`；R1–R16 不变；R17/R18 追加 |
| C-3 | 03 `is_quiescent`/watchdog 不识别 controller buffer | §3.10 扩展：未展开就绪的 controller buffer 视为非静默（Pause 冻结） |
| C-4 | 06 `instances` 唯一键/CHECK 无 item 列 | schema v6 重建（12 步法）；旧行默认 item_index=0/item_key NULL |
| C-5 | W3a `human_decisions`/recorded_outputs 按 instance_id | 不改表（instance_id 唯一）；L2 查找键经 instances.item_index 扩展 |
| C-6 | W2b `max_fanout` 语义 | 明确「不限制 N」；只限每 item 激活放行出边数 |
| C-7 | 07 通过标准 AC-01..AC-16 | 扩展为 AC-01..AC-17（AC-17 本批设计冻结） |
| C-8 | 04 `builtin.human` | region 内允许；每 item 独立实例/decide；不改 handler |
| C-9 | DSL 域 18 码 | 新增 `DSL_FOREACH_INVALID` → **19 码**；R17 复用 `DSL_PARSE_ERROR` |

### 15.2 冻结时补全的边界（无现成决策）

| # | 边界 | 冻结 |
|---|---|---|
| E-1 | F 自身是否也执行 N 次 | **是**（per-item handler；items_port=item[i]；其它 F 输入沿用 controller buffer） |
| E-2 | R={F} 单节点 region | 合法；E=F；F 的 item 输出即 join 元素 |
| E-3 | F trigger=any | controller buffer 按 any 就绪；未到端口取 null；与普通节点口径一致 |
| E-4 | item_key 派生 | `key_field` 缺省/元素非 object/缺键/null/复合值 → item_key=NULL（trace `{"$index": i}`）；仅**非 NULL 重复**报错 |
| E-5 | 空列表 | 合法：expansion(count=0) + 立即投递 `[]` 给 J；无 item 实例 |
| E-6 | join 目标为 end | 数组走 end 的 to_input_port 聚合（`run_output` 对应端口为数组） |
| E-7 | region 内 defer/环 | 沿用 R13/R15 与 W2a attempt-per-activation（按 item 作用域） |
| E-8 | Pause 期间 controller 就绪 | 不展开（冻结派发语义）；Resume 后展开 |
| E-9 | 展开后 F 再收到投递 | 不再接受（F 不可重入/单次展开）；多展开用多个 F |
| E-10 | `max_items` 缺省无上限 | 显式契约 + 风险文档警示；不隐含默认值 |

### 15.3 Lead 裁决（2026-11-01，已确认；C4）

**已确认（Lead 2026-11-01 裁决，与 §6.2/§6.5/§6.6 冻结值一致）**：

| # | 事项 | 裁决（采纳） |
|---|---|---|
| ① | item_key canonical | string 原值；number → JSON canonical 文本（`1`/`1.5`）；bool → `true`/`false`；缺键/null/object/array → `NULL`（按索引派生，trace `{"$index": i}`）；仅非 NULL 重复报错 |
| ② | `max_items` 缺省 | **无上限**（由作者显式 `max_items` 约束；风险登记 ISSUE-141，§13） |
| ③ | J 为 `end` | **允许**：join 数组按普通端口值进入 end 输入聚合（`run_output` 对应端口为数组），不再作为 end 输出 |
| ④ | region 内 `trigger=any` | 沿用普通 `any` 语义（未到端口为 null；items_port 必须已到才展开，§6.1） |

> 本表取代原「需确认」状态（历史决策不改；如后续需变更，追加修订段）。

---

## 16. 与 DR-004 / DR-005 / DR-006 / REV 的对应

| 来源 | 本 DR 落点 |
|---|---|
| DR-004 §3 W4a / §5 D4 | §2–§11 全部 |
| DR-005（W2a defer/attempt-per-activation） | §6.3/§6.6（per-item 作用域） |
| DR-006（W3a human/observer/trace v5 模式） | §6.6（human per item）、§8（v6 迁移模式） |
| W2b（max_fanout） | §3.2/§6.6（不限制 N） |
| REV-063 S10/C12/C13（写者/表格模式） | §8.3（expansion 表写者：TraceWriter，事件表同连接） |
| ISSUE-111（trace fixture 归属） | §14（trace seeds 归 e2e，按既有口径） |
| 用户 2026-11-01「W3b 不做」 | DR-004 §10/§15 记录；本 DR 非目标 §1.3 |

---

## 17. 本批改动文件与本批门禁

### 17.1 本批（W4a，docs）改动

| 文件 | 改动 |
|---|---|
| `doc/decisions/DR-007-w4a-dynamic-fanout-freeze.md` | 新增（本文件） |
| `doc/spec-v1.2/02-DSL与校验.md` | banner；`foreach` schema/白名单/serde；R17/R18；执行顺序；模板；示例/fixtures；hash；DSL 码 19 |
| `doc/spec-v1.2/01-领域模型与状态机.md` | banner；`CompiledNode.foreach`；item 实例/定位键 `(node,item)`/attempt；`InstanceRecord.item_index/item_key`；RunState controller buffers |
| `doc/spec-v1.2/03-调度器与生命周期.md` | banner；新增 §3.4.6 展开/join；dispatch/失败/取消/上限；终止判定；§3.15 矩阵 |
| `doc/spec-v1.2/06-存储与可观测.md` | banner；schema v6 DDL/迁移；`foreach_expansions`；trace/replay 输出；只读拒绝 |
| `doc/spec-v1.2/07-验收用例.md` | banner；AC-17 a–j 设计；§7.1/§7.2；通过标准 AC-01..AC-17 |
| `doc/spec-v1.2/00-总纲.md` | banner；`DSL_FOREACH_INVALID`（19 码）+ 4 个 `RUN_FOREACH_*`；`ItemIndex`/`ExpansionId` 别名 |
| `doc/spec-v1.2/04-节点接口与扩展点.md` | 最小注记：region 内 human per-item |
| `doc/design/software-team-sop-v1.md` | v1.2（dynamic-N profile；§9 移除动态 N；§7/§11 更新） |
| `doc/decisions/DR-004...md` | §10 追加 W3b 不做 + W4a 执行记录 |
| `doc/progress.md` / `doc/index.md` / `doc/spec-v1.2/PROVENANCE.md` / `README.md` | 状态/条目/§3 重算/行数表 |

### 17.2 不修改（红线）

`CLAUDE.md`；任何代码/测试/配置/`schemas/**`；`doc/reviews/**`（评审人写）；不改写已冻结 W1/W2a/W2b/W3a 历史决策（只追加 W4a 口径）；不把未冻结项写成 as-built。

### 17.3 提交与评审

- 分支 `docs/docs-dynamic-fanout-design`；提交正文含 `Review: REV-086` 与 `Agent: docs-dev`；**本批不 push、不合并**。
- `bash scripts/dev/check.sh` 必须 PASSED；PROVENANCE §3 的 SHA-256/行数以实测为准；合并前由独立评审人写 `doc/reviews/REV-086-docs-dynamic-fanout-design.md`，结论必须为「通过」（AGENTS §3/§4）。
- W4b 实现完成后由 W4b-docs 做 as-built 回写（PROVENANCE 第 2 条、AC-17 实测、08 注记）。
