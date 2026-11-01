# REV-088 · dsl · W4b `foreach` 字段 + R17 形状 + R18 区域校验

> **跨模块声明**：本分支主体为 `dsl`（`crates/zgraph-dsl/**`），另按 task-35/36 合同在 `tests/fixtures/graphs/**`（`e2e` 模块路径）**新增 22 个 fixture**（1 valid + 21 bad，e2e 只读消费；Lead 已批准）。已在作者提交正文声明；本评审记录再次声明。评审人只新增本评审记录，未修改产品/测试代码。

- **分支**：`feat/dsl-foreach`
- 分支: feat/dsl-foreach
- **作者**：`dsl-dev`（提交 `9efc41c`；正文 `Review: REV-088` / `Agent: dsl-dev` + 跨模块声明）
- **评审人**：`reviewer-a`（独立评审，**≠ 作者**）
- **变更范围**：`42f93d7..9efc41c`（**31 files, +4210/−27**）：`src/{error,foreach,hash,lib,nodes,types}.rs`、`tests/{fixtures,hash,rules}.rs`、新增 22 个 `tests/fixtures/graphs/{valid_foreach,bad_foreach_*}.json`
- **评审时间**：2026-11-01
- **关联**：DR-007 §2（语法）/§3（R17/R18）/§4（区域算法）/§5（hash）/§12（兼容矩阵）；spec 02 W4a 段（§2.1.3/§2.2.2/§2.2.4/§2.4.1 R17·R18/§2.4.2 模板/§2.8.3/§2.9）；task-35/36；REV-086（W4a 冻结）

## 1. 结论

**结论：通过**

按 task-35/36 与 DR-007 §2/§3/§4/§5/§12、spec 02 W4a 段逐项核对，实现与冻结语义一致；P0=0、P1=0、P2=0，P3 建议 8 条（含作者 4 项偏差判读；均不阻塞）。未放宽/跳过既有断言；v0.1.0 既有 42 fixture 经评审人独立 A/B **84/84 逐字节一致**。

### 1.1 合同覆盖对照

| 合同项 | 实现/证据 | 结论 |
|---|---|---|
| **类型/白名单/serde** | `GraphNode.foreach: Option<ForeachConfig>`（`types.rs:342`，`skip_serializing_if`）；`ForeachConfig` 字段与 DR §2.2 逐一对应（`items_port/key_field: Option/join/max_items: Option<u32>/on_item_error: Option<OnItemError>`，均 skip None，`types.rs:274-293`）；`OnItemError::{Fail}` 闭集 + `as_str/from_name`（`types.rs:247-267`）；`NODE_KEYS` 7→8 加入 `foreach`（`nodes.rs:33-42`）；`FOREACH_KEYS` 5 键（`:43-49`）；导出 `ForeachConfig`/`OnItemError`（`lib.rs:53-54`） | ✅ |
| **R17（R4 阶段、复用 `DSL_PARSE_ERROR`）** | `parse_foreach`（`nodes.rs:820-858`）顺序 = 非 object → 未知键 → 缺必填 items_port/join → items_port（已声明端口）→ key_field → join → max_items → on_item_error → 节点类型；9 类模板 message/expected/actual/location 与 DR §3.1 逐字一致（评审 CLI 实测见 §4.3）；节点类型仅 python_node/agent_call（含 builtin.human），start/end/subgraph → R17（`check_foreach_node_type`，`:994-1009`）；同节点聚合、nodes 数组序 | ✅ |
| **R18（新码 `DSL_FOREACH_INVALID` 第 19 码）** | `error.rs` 新增 `ForeachInvalid`/`DSL_FOREACH_INVALID`，`ALL` 18→19，doc 更新；执行顺序 `R15→R18→R16→R14`（`lib.rs:209-214`，R18 在 R15 后、R16 前）；载荷冻结：message `foreach region of node '{F}' is invalid: {reason}`、expected `"valid foreach region"`、actual `{"node_id":F,"reason":reason}`、location.node_id=F（`foreach.rs:262-276`）；reason 闭集 11 值按 DR §3.2 顺序 | ✅ |
| **区域算法** | `R = {F} ∪ BFS(F, 不进入 J)`（`foreach.rs:245-260`）；`GraphIndex` 按 nodes/edges 数组序建索引、`BTreeSet` region、出口按 nodes 序取 → **确定性**；row1–row10 逐条实现（`validate_region`，`:137-243`），row4 内先 forbidden 后 nested（N15）、row8 含 F 入边必须来自 R 外（C1/N13）、row9 items_port 来源、row10 region 不相交 | ✅ |
| **边界闭环** | 单出口 `E`（恰 1 个节点有出边离开 R）→ E 恰 1 条出边且目标=J、无 gate（否则 `external_edge`/`join_edge_gated`）；R 内节点（除 E）出边闭合；R 内节点（除 F）入边来源 ∈R；F 入边 ∉R；join 存在/≠F/∉R/类型 ∈{python_node,agent_call,end}；禁 start/end/subgraph 与嵌套 foreach；region 不相交 | ✅ |
| **子图归 R6** | `reject_subgraph_foreach`（`nodes.rs:1385-1400`）：子图内任意节点带 `foreach`（**无论形状是否合法**；合法形状经 parse 后在此拒绝，非法形状由 R4 错误包裹）→ `DSL_SUBGRAPH_INVALID`（message 前缀 `subgraph '<id>': …`、location 保留内部 `node_id`）；`validate_subgraph_rules` 调用（`:1312`）；fixture + rules 双覆盖 | ✅ |
| **hash** | canonical projection 节点对象仅在**显式存在** `foreach` 时原样投影（`hash.rs:119-122`）；缺省不补齐；显式 `on_item_error:"fail"`/`max_items`/`key_field` 参与（语义等价但 hash 不同，沿用不归一化口径）；`graph_hash` 前置注释加 R18（`hash.rs:18`） | ✅ |
| **fixtures** | 新增 **22**（21 bad + 1 valid）；`fixtures.rs` BAD_CASES 35→**56**、VALID 7→**8**（=64 = 根 fixture 总数，逐文件覆盖断言）；R17 10 类（含 items_port 未声明/非 string 两变体）、R18 **10 个可达 reason** 各 1 例、子图 1 例；r17/r18 规则测试 13 个 + hash 3 个 | ✅ |
| **既有行为不变** | 无 foreach 图：R17/R18 零副作用（R18 提前返回）、hash 投影不变、18 码不变；评审人独立 A/B（main `42f93d7` CLI vs 分支 CLI）42 fixture × {human,--json} = **84/84 逐字节一致（含 exit code）** | ✅ |
| **执行顺序** | `r18_runs_after_r15`（R15 先报）、`r18_runs_before_r16_and_r14`（R18 先于 R16/R14）实测通过 | ✅ |

## 2. 四维评审

| 维度 | 结论 | 依据（文件/行/测试） |
|---|---|---|
| 功能正确性 | ✅ | 合同逐项成立（§1.1）；`cargo test -p zgraph-dsl` **118 passed/0 failed**（unit 1 + fixtures 7 + hash 18 + rules 91 + doctest 1）、`cargo test --workspace` **836 passed/0 failed/3 ignored** 与作者报告一致；新 fixture CLI 实测 hash `10cd8fff9c8f85fb` 与 golden 一致，R17/R18/子图错误载荷逐字符合冻结模板；既有 42 fixture A/B 84/84 一致 |
| 代码结构合理性 | ✅ | R17 形状在 `nodes.rs`（R4 阶段同文件）、R18 区域独立 `foreach.rs` 纯函数模块（输入 `nodes/edges`、返回 `Vec<DslError>`），职责清晰；`GraphIndex` 一次构建、BFS 确定性；错误载荷集中在 `region_error`；`DslErrorCode` 单一扩展点；无重复实现/无新依赖（Cargo 未变） |
| 逻辑性 | ✅ | reason 优先级与 DR §3.2 表一致（row1→row10，row4 内 forbidden→nested）；BFS 不进入 J；出口/边界/入边来源检查闭合（含 C1/N13 回边判定）；首错按 nodes 数组序；子图收敛与 R6 既有口径一致；无 foreach 图零副作用；A/B 证明既有 18 码/R1–R16/CLI 输出未受影响 |
| 可读性 | ✅（N1/N3/N4/N6 备注） | 模块/规则注释均引用 DR-007/spec 章节并解释「为什么」（C1 单次展开、row4 顺序、N=0/hash 不归一化）；错误模板集中在函数内、expected/actual 机器可读；测试名与断言可读。N1/N3/N4/N6 为文档/命名小建议 |

## 3. 发现清单

> 级别口径：P0 = 阻塞；P1 = 重要（必须修复或登记 ISSUE）；P2 = 一般；P3 = 建议（可后置，应进台账）。本分支 P0=0、P1=0、P2=0。

| # | 级别 | 位置 | 问题 | 建议 | 状态 |
|---|---|---|---|---|---|
| 1 | P3 裁决 | `crates/zgraph-dsl/src/foreach.rs:45,84`；`crates/zgraph-dsl/tests/rules.rs:2599` | **作者偏差 ③（Lead 委托判读）**：`region_overlap` 在冻结判定顺序下**不可达**（row4 的 nested_foreach / row8 的 external_edge 必先拦截相交 region）；实现保留该分支并按 row10 优先级放置，测试 `r18_region_overlap_is_checked_after_boundary_rows` 锁定「boundary 先赢」的优先级。**可接受**：spec 02 R18 要求 11 reason 闭集，代码保留防御分支、不改变任何可达行为 | W4b-docs（REV-093）在 02/DR-007 as-built 注明「region_overlap 为防御性不可达 reason；测试锁定优先级」；无需改代码 | open（裁决完成） |
| 2 | P3 建议 | `crates/zgraph-dsl/src/nodes.rs:860-873` | **作者偏差 ④（Lead 委托判读）**：`check_foreach_keys` 按 `serde_json::Map` 迭代顺序报未知键；`preserve_order` 由工作区依赖（genai）传递开启，**单 crate 测试与 workspace 构建下多未知键的诊断顺序可能不同**。测试已用集合断言规避。`check_node_keys`/顶层未知键为既有同模式（非本分支引入） | 可接受；如需跨构建诊断序列完全确定，建议对未知键 `collect + sort` 后迭代（或 W4b-docs 注明该既有口径） | open（裁决完成） |
| 3 | P3 建议 | `crates/zgraph-dsl/tests/fixtures.rs:3` | 模块注释仍写「**18 个 DSL 码**反例」（W4a 已新增第 19 码 `DSL_FOREACH_INVALID`；本文件已覆盖 R17/R18） | 改为「19 个 DSL 码反例」或「全部 DSL 码反例」 | open（建议项） |
| 4 | P3 说明 | `crates/zgraph-cli/src/main.rs`（未改）；`doc/issues.md:37`（ISSUE-029） | task-35 合同提到「`--structure-only` 含 R17/R18」，但 CLI `--structure-only` 选项当前**未实现**（ISSUE-029 open，M1 前既有缺口，非本分支引入）；R18 位于 `load_graph_value`，任何 `validate` 路径均执行 R17/R18，语义上已覆盖 | 无需本分支动作；W4b-docs/后续 M1.x 处理 ISSUE-029；评审已实测普通 `validate` 对 R18 反例 exit 2 | open（说明项） |
| 5 | P3 裁决 | `crates/zgraph-dsl/src/types.rs:274`；spec 02:149；DR-007 §2.2 | **作者偏差 ①（Lead 委托判读）**：task 文本写 `ForeachSpec`，实现为 `ForeachConfig`。**可接受**：DR-007 §2.2 与 spec 02 §2.2.2 冻结名均为 `ForeachConfig`（`CompiledForeach` 为运行期映射），task 文本为旧表述，DR/spec 为准 | W4b-docs 无需改；task 文本无需追溯 | open（裁决完成） |
| 6 | P3 裁决 | `crates/zgraph-dsl/src/foreach.rs:137,179`；DR-007 §3.2 row1 | **作者偏差 ②（Lead 委托判读）**：`join` 存在性/`join_in_region` 归 R18 而非 R17。**可接受**：与 DR §3.2 row1/row2 冻结一致（R17 只校验 `join` 为非空 string），fixtures `bad_foreach_join_not_found`/`bad_foreach_join_in_region` 均命中 R18 | 无需动作 | open（裁决完成） |
| 7 | P3 建议 | `crates/zgraph-dsl/src/nodes.rs:1385-1400` | 子图内 foreach 的 `DSL_SUBGRAPH_INVALID` **message 文本**（`node 'pin' declares foreach, which is not supported inside subgraphs`）未在 DR/spec 中冻结（DR §3.2 仅规定收敛码/前缀/location）；rules 测试自行锁定该文案，未来若 spec 细化需同步 | W4b-docs as-built 记录该精确文案与 location 口径（与 R6 收敛一致即可）；无代码改动 | open（建议项） |
| 8 | P3 建议 | `crates/zgraph-dsl/tests/rules.rs:2599` | 测试名 `r18_region_overlap_is_checked_after_boundary_rows` 易被读成「验证到 region_overlap」；实际断言的是 `external_edge` 优先级（overlap 不可达），函数注释已解释 | 可选改名/加注（如 `r18_boundary_rows_precede_region_overlap`），不影响语义 | open（建议项） |

## 4. 验证证据

评审人**亲自执行**（worktree `/Users/zyzs/lczyzs/code/zgraph/.worktrees/dsl-dsl-dev`，HEAD `9efc41c`，基于 `main = origin/main = 42f93d7`，工作区干净；`CARGO_HOME=<repo>/.cargo-home`）：

### 4.1 测试与门禁

```bash
cargo test -p zgraph-dsl
# unit 1 + fixtures 7 + hash 18 + rules 91 + doctest 1 = 118 passed, 0 failed, 0 ignored
cargo test --workspace                 # 836 passed / 0 failed / 3 ignored（与作者报告一致）
cargo clippy --workspace --all-targets --locked -- -D warnings   # clean（exit 0）
cargo fmt --all -- --check             # exit 0
bash scripts/dev/check.sh              # CHECK PASSED
```

- 范围：`git diff --name-status main...HEAD` = 9 个 dsl src/test 文件 + 22 个新 fixture；`Cargo.toml`/`Cargo.lock` 无 diff；提交正文含 `Review: REV-088` / `Agent: dsl-dev` 与跨模块声明；worktree 干净。

### 4.2 v0.1.0 既有 42 fixture 独立 A/B（main `42f93d7` CLI vs 分支 CLI）

```bash
# baseline：main worktree 重新 cargo build -p zgraph-cli --bin zgraph（HEAD 42f93d7）
# branch  ：.worktrees/dsl-dsl-dev/target/debug/zgraph（HEAD 9efc41c，fresh）
# 对 tests/fixtures/graphs 下 42 个既有 fixture（排除 22 个新 foreach fixture），
# 每图 human 与 --json 两种模式比较 stdout+stderr+exit code：
fixtures=42 identical=84 diff=0
```

- **84/84 逐字节一致**（含退出码），与作者「42 个既有 fixture CLI 输出与 hash 逐字节不变」一致；hash golden 测试（customer_ticket/valid_cycle/valid_parallel/valid_gate_defer/valid_max_fanout 等）在 118 中继续通过。

### 4.3 新增 fixture 的 CLI 实测（与冻结模板逐字核对）

```text
valid_foreach.json
  → exit 0, {"ok":true,...,"graph_hash":"10cd8fff9c8f85fb"}      # = dsl hash golden
bad_foreach_external_edge.json
  → exit 2, DSL_FOREACH_INVALID
    message "foreach region of node 'F' is invalid: external_edge"
    expected "valid foreach region" / actual {"node_id":"F","reason":"external_edge"}
    location.node_id "F", error_count 1
bad_foreach_not_object.json（R17）
  → exit 2, DSL_PARSE_ERROR, "field 'foreach' expects object, got string",
    expected "object", actual "items", location.node_id "F"
bad_foreach_subgraph.json（R6 收敛）
  → exit 2, DSL_SUBGRAPH_INVALID,
    "subgraph 'inner': node 'pin' declares foreach, which is not supported inside subgraphs",
    location.node_id "pin", error_count 1
```

- `bad_foreach_external_edge` 构造为 `W→F` 回边（R 内→F），验证 C1/N13：R18 row8 报 `external_edge`（而非 nested/overlap）✓。
- `regional` fixtures 的 error_count/location 由 `fixtures.rs` 通用强断言（错误码/定位/`error_count==1`/expected-or-actual）+ `r18_reason_closed_set_is_reported_by_fixtures`（message+actual）覆盖；R17 9 类模板由 `r17_field_value_boundaries_are_validated`/`r17_foreach_aggregates_field_errors_in_frozen_order` 强断言 message/expected/actual/location。

### 4.4 其他关键对照

- R18 区域算法确定性：`r18_region_algorithm_is_deterministic`；首错按 nodes 序：`r18_first_invalid_foreach_in_nodes_order_is_reported`；P-1/P-2/P-3（含 J=end、串行 foreach）合法图：`r18_valid_regions_pass`；R17 类型限制（start/end/subgraph）与 `agent_call`/`builtin.human` 允许：`r17_foreach_is_allowed_only_on_executable_node_types`。
- 子图：fixture（合法形状内嵌→R6 拒绝）+ rules 构造（message/location/error_count 断言）双覆盖。
- `DSL` 域 19 码：`error.rs` `ALL`/单测断言 19；spec 00/02 已为 19 码（W4a 冻结），无代码-文档计数偏差。

## 5. 遗留问题

- **P3 建议未登记 `doc/issues.md`**（本分支合同仅允许 `crates/zgraph-dsl/**` + 新增 fixture；台账由 docs/Lead 维护）：N1/N3/N4/N6–N8 建议由 Lead 决定是否登记；N1/N5/N6 为已裁决偏差，建议在 W4b-docs（REV-093）DR-007 §14/as-built 注记。
- **跨模块**：新增 22 个 `tests/fixtures/graphs/**`（e2e 路径）已声明；W4b e2e（REV-092）将只读消费，勿改其内容。
- **后续**：本分支合并后进入 trace v6（REV-089）、kernel 动态扇出（REV-090）、cli（REV-091）、e2e（REV-092）、W4b-docs（REV-093）；R18 运行期语义（展开/join/4 个 `RUN_FOREACH_*`）不在本分支范围。
- 未发现需要返工的问题；无阻塞项。

## 6. 条件关闭记录（仅"有条件通过"填写）

不适用（结论为 `通过`，无附加条件）。
