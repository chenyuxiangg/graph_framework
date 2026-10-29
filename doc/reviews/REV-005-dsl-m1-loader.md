# REV-005 · dsl · M1 DSL 类型/loader/R1–R14/graph_hash/validate CLI

> **跨模块声明**：本分支以 `dsl` 为主，另触及 `crates/zgraph-cli`（`validate` 命令）、`tests/fixtures/graphs|assets`（e2e 归属）、`doc/progress.md` 与 `doc/issues.md`（任务台账）。属 Lead 指定的 M1 交付，非静默越界；评审记录此声明。

- **分支**：`feat/dsl-m1-loader`
- **作者**：`dsl-spec-writer`（实现 `4e80cd1`、遗留登记 `f344fd7`、进度 `f28aaa3`、REV-005 修正 `57987d0`）
- **评审人**：`docs-reviewer`（独立评审，非作者）
- **变更范围**：`202d5c4..57987d0`（全分支 57 files, +6741/−29；其中作者修正 `f942b33..57987d0` 为 21 files, +911/−164；`f28aaa3` 仅进度，`f942b33` 为上一版评审记录）
- **评审时间**：2026-10-29
- **关联**：02 §2.1–§2.8（R1–R14、错误对象、CLI、graph_hash）、00 §0.4/§0.1（UTC RFC3339）、architecture §8 D6/D7、07 AC-04；ISSUE-026/027/028/029/030/031

## 1. 结论

**结论：不通过**

`57987d0` 已实质关闭上一轮 #1–#3、#5，并大幅改善 #4/#6：

- **#1 on_error 作用域**：`shape::check_keys` 已不再全局跳过；节点级 `on_error` 仍为 `DSL_UNSUPPORTED_FIELD`（带 `node_id`），顶层/config/port/edge/gate 实测均为 `DSL_PARSE_ERROR`。验收口径已闭合，但 `actual` 仍只有字段名 **`on_error`**，而 02 §2.2.4 要求 `actual` 为字段名+值（见 #3）。
- **#2 from_output_port**：已改为 `Option<Value>`，出现即 `DSL_UNSUPPORTED_FIELD`；`123` 实测 `location={edge_index:0}, actual=123`，正确。
- **#3 子图 id**：空/缺失 → `DSL_MISSING_FIELD`，非 string → `DSL_PARSE_ERROR`，均 exit 2；无引用子图不再静默通过。
- **#4 location**：`Location.field` 已删除，节点级带 `node_id`、边级带 `edge_index`、R2/R3 文档级 `{}`；fixtures 改为 28 个坏图的强定位表（`error_count=1`、location 精确断言）。但仍有一个**有效 SubgraphId 的子图头错误** location 为空，见 #1 阻塞。
- **#5 缺边字段**：缺 `from` 只报一条 `DSL_MISSING_FIELD`，`location={edge_index:0}`，`error_count=1`，已修。
- **#6 created_at**：非法日期 `2026-99-99T99:99:99Z` 已 exit 2；但修复把合法的 UTC RFC3339 `+00:00` 也判为非法，属于过度收紧（见 #2 阻塞）。

因此仍有 2 个阻塞项和 3 个重要项，不能合并。

## 2. 四维评审

| 维度 | 结论 | 依据（文件/行/测试） |
|---|---|---|
| 功能正确性 | ❌ | #1–#3/#5 已闭合；`57987d0` 修正了大部分 location 和时间范围，但**有效 SubgraphId 的子图头错误仍无 location**（#1 阻塞），**合法 `+00:00` 被拒绝**（#2 阻塞），`on_error` actual 与资产索引错误 location 仍有契约偏差（#3/#4 重要）。 |
| 代码结构合理性 | ✅ | 分模块与 R1–R14 阶段划分清晰；`parse_edges` 去重前置扫描、`attach_node_location`/`attach_edge_location` 收敛；CLI locator 随 contract 简化；fixtures 强定位表化；新增 ISSUE-031 记录子图头张力。 |
| 逻辑性 | ⚠️ | 主路径、95 个测试、hash 均通过；但子图头错误在 `parse_subgraph` 内先于 id 定位收集，`check_subgraph_references` 又追加一条“未定义”错误；`+00:00` 与 `on_error.actual` 是覆盖/一致性缺口。 |
| 可读性 | ✅ | 模块文档、函数注释、fixtures 命名清晰；无生产 `unwrap/expect/panic`；函数 ≤60 行；建议 #9–#10 的 CLI/依赖边界继续在 issue 中收口。 |

## 3. 发现清单

| # | 级别 | 位置 | 问题 | 建议 | 状态 |
|---|---|---|---|---|---|
| 1 | 阻塞 | `crates/zgraph-dsl/src/nodes.rs:991-1017`；`crates/zgraph-dsl/src/nodes.rs:937-965` | **有效 SubgraphId 的子图头错误仍无 location**：`parse_subgraph` 在 `check_keys`/缺键/`parse_string_map` 阶段就 push 错误并提前 `return None`；这些错误没有 `node_id`，随后 `check_subgraph_references` 再追加一条 `SubgraphInvalid`（“referenced…but not defined”）。实测 `/tmp/g_sub_header_unknown.json` 与 `on_error`：`location:{}` + 第二条 `{node_id:"sub"}`，`error_count=2`。02 §2.7.3 规定子图级错误 `node_id`=SubgraphId；ISSUE-031 只覆盖 id 缺失/空/非 string 的无 SubgraphId 情形，不覆盖有效 id。 | 先解析/校验 `id`，再把本子图头错误统一 `.at_node(subgraph_id)`；有效定义不应因 header 错误再触发 `check_subgraph_references` 的“未定义”二次错误。补有效 id + 未知字段/on_error 的反例。 | open（阻塞） |
| 2 | 阻塞 | `crates/zgraph-dsl/src/shape.rs:131-189`；`crates/zgraph-dsl/tests/rules.rs:1098-1103` | **合法 UTC RFC3339 `+00:00` 被拒绝**：新实现只接受 `Z` 后缀；`2026-10-29T10:00:00+00:00`、`2026-10-29T10:00:00.123+00:00` 实测 exit 2。00 §0.1/02 R3 要求 UTC RFC3339；RFC3339 的 `+00:00` 与 `Z` 都表示 UTC，上一版也接受 `+00:00`。测试还把 `+00:00` 列为非法，固化了过度收紧。 | 接受 `Z` 与 `+00:00`（含小数秒+`+00:00`）；仅拒绝非零 offset/非法日期；修正测试，保留非法日期反例。 | open（阻塞） |
| 3 | 重要 | `crates/zgraph-dsl/src/nodes.rs:311-330`；`tests/fixtures/graphs/bad_reserved_field.json` | 节点级 `on_error` 的 `actual` 仍只是 `"on_error"`，没有字段值；02 §2.2.4 要求 `actual`=字段名+值。 | 用 `map.get("on_error")` 构造 `{"on_error": value}`（或约定的字段名+值结构）；补 actual 断言。 | open（重要） |
| 4 | 重要 | `crates/zgraph-dsl/src/assets.rs:42-75,103-129` | 资产索引错误（duplicate id / invalid asset JSON）`location:{}`；`DSL_AGENT_NOT_FOUND` 在 02 §2.4.2 的 location 为 `node_id`，但 index-level 错误没有可用节点。实测 duplicate/invalid assets 均 `location:{}`。 | 明确 index-level 错误是否属文档级；若要求 node_id，定位到首个相关 agent_call 节点或新增登记例外（类似 ISSUE-031）。 | open（重要） |
| 5 | 重要 | `crates/zgraph-dsl/src/nodes.rs:937-965`；`crates/zgraph-dsl/src/nodes.rs:991-1017` | 子图头错误导致二次错误：定义已存在但 header 非法时，`parse_subgraph` 返回 None，`check_subgraph_references` 把引用它的主图节点再报 `DSL_SUBGRAPH_INVALID`；实测 2 条错误，首条才是根因。 | 对“定义存在但非法”的子图，不再追加“未定义”错误；或先收集/标记 invalid id，避免级联。 | open（重要） |
| 6 | 重要 | `crates/zgraph-dsl/tests/fixtures.rs`；`crates/zgraph-dsl/tests/rules.rs`；`tests/fixtures/graphs/` | 回归测试仍缺：有效 id 的子图头错误、`+00:00` 合法时间、资产索引 duplicate/invalid 的 location、`on_error.actual` 的值；且 `rev005_created_at` 把 `+00:00` 断言为非法。 | 补上述反例/正例；fixtures 强定位表增加“有效 id 子图头”和资产索引用例；修正 `+00:00` 断言。 | open（重要） |
| 7 | 重要（已登记） | `doc/issues.md:33-38`；`crates/zgraph-dsl/src/assets.rs:8-9`；`crates/zgraph-dsl/src/symbols.rs:327-397`；`crates/zgraph-cli/src/validate.rs:32-66` | 已登记：ISSUE-026（D6/D7 gate params/冲突语义）、ISSUE-027（无 assets-dir 跳过 R14c）、ISSUE-028（fixtures 路径）、ISSUE-029（schema/--structure-only/--print-hash/语义校验）、ISSUE-030（blake3 边界）、ISSUE-031（无 SubgraphId 子图头 location）。 | 评审接受这些为已登记范围；其中 ISSUE-026/027/029 的 spec/CLI 口径需在 M2 前锁定，ISSUE-031 只覆盖无 id 情形。 | open（已登记） |
| 8 | 建议 | `crates/zgraph-cli/src/main.rs:3-6`；`doc/spec-v1.2/02-DSL与校验.md:710-716` | 02 §2.7.4 规定 validate 的“其他错误”退出码为 `1`，M1 按任务口径用 `3`（文件不可读/用法）；文档仍写 1。 | 回写 02/06 的 M1 口径或改实现，避免双重口径。 | open（建议） |

## 4. 验证证据

### 4.1 最终范围

```bash
cd /Users/zyzs/lczyzs/code/zgraph/.worktrees/dsl-dsl-impl
git log --oneline -3
# 57987d0 fix(dsl): 关闭 REV-005 阻塞项（on_error 作用域/from_output_port/子图 id/location/去重/RFC3339）
# f942b33 docs(review): REV-005 M1 DSL 评审（不通过）
# f28aaa3 docs(process): 同步 M0 完成后的进度
git diff --shortstat f942b33..57987d0
# 21 files changed, 911 insertions(+), 164 deletions(-)
git diff --shortstat 202d5c4..57987d0
# 57 files changed, 6741 insertions(+), 29 deletions(-)（含评审记录/进度）
```

### 4.2 必跑命令

```bash
export CARGO_HOME="$PWD/.cargo-home" HTTPS_PROXY="http://127.0.0.1:7890" HTTP_PROXY="http://127.0.0.1:7890"
cargo fmt --all -- --check                       # exit 0
cargo clippy --workspace --all-targets -- -D warnings  # exit 0
cargo test --workspace                           # exit 0，95 passed
# dsl 65：unit 1 + rules 51 + fixtures 5 + hash 7 + doctest 1；
# cli 15：unit 7 + integration 8；worker 15：unit 7 + integration 8
cargo run -q -p zgraph-cli -- validate tests/fixtures/graphs/customer_ticket.json \
  --assets-dir tests/fixtures/assets
# ✓ graph valid: 6 nodes, 6 edges, 0 subgraphs (graph_hash=ec320e4244b365ab), exit 0
cargo run -q -p zgraph-cli -- validate tests/fixtures/graphs/bad_port.json --json
# DSL_INVALID_PORT, location={node_id:n1,edge_index:0,port:zzz}, exit 2
bash scripts/dev/check.sh                        # CHECK PASSED
```

### 4.3 手工反例（逐条复核 57987d0）

| 反例 | 期望 | 实际 |
|---|---|---|
| 顶层 `on_error` | `DSL_PARSE_ERROR` / `{}` | **通过**：`PARSE_ERROR`、`location:{}`、exit 2 |
| agent `config.on_error` | ParseError / `node_id:a` | **通过**：`node_id:"a"`、exit 2 |
| 边 `on_error` | ParseError / `edge_index:0` | **通过**：`edge_index:0`、exit 2 |
| 输入端口 `on_error` | ParseError / `node_id` | **通过**：`node_id:"start"`、exit 2 |
| `from_output_port:123` | Unsupported / `edge_index:0` / actual 123 | **通过**：`UNSUPPORTED_FIELD`、actual 123、exit 2 |
| 无引用子图 `id:""` | `MISSING_FIELD` | **通过**：`MISSING_FIELD`、exit 2 |
| 无引用子图 `id:7` | `PARSE_ERROR` | **通过**：`PARSE_ERROR`、exit 2 |
| 缺 `from` 的边 | 1 条 Missing / edge 0 | **通过**：1 条、`edge_index:0`、`error_count=1` |
| 节点未知字段 `bogus` | ParseError / `node_id:"start"` | **通过**：`node_id:"start"`、exit 2 |
| `created_at:"2026-99-99T99:99:99Z"` | ParseError / `{}` | **通过**：`PARSE_ERROR`、exit 2 |
| **有效 id 子图头未知字段** | ParseError / **`node_id:"inner"`** / 1 条 | **失败**：`location:{}` + 第二条 `SubgraphInvalid`，`error_count=2` |
| **`created_at:"2026-10-29T10:00:00+00:00"`** | 合法 UTC RFC3339 | **失败**：`PARSE_ERROR`、exit 2 |
| **节点级 `on_error` actual** | 字段名+值 | **失败**：actual 只有 `"on_error"` |
| **资产索引 duplicate/invalid** | 按 spec 有明确 location | **失败**：`location:{}` |

### 4.4 其他确认

- D6/D7：`grep`/代码检查未发现 `import`/`eval`；`python_node` 仅格式校验；`valid_subgraph` 加载期通过、运行期拒绝留 M2；方向正确。
- `graph_hash`：golden `ec320e4244b365ab` 不变；格式化/键序、timeout、端口 description 测试通过；白名单排除 description/author/created_at。
- fixtures：28 坏图 + 4 合法图均被引用，强定位表断言 `code+location+error_count`；但仍缺上述有效 id 子图头、`+00:00`、资产索引反例。
- 未 push/merge；仅新增/更新本评审记录。

## 5. 遗留问题

- **阻塞 #1/#2 必须先修**：有效 SubgraphId 的子图头错误 location + 级联；合法 `+00:00` 的 RFC3339 支持。
- **重要 #3/#4/#5/#6**：`on_error.actual`、资产索引 location、子图头二次错误、对应测试。
- 已登记：ISSUE-026/027/028/029/030/031；其中 ISSUE-031 仅覆盖无 SubgraphId 的子图头，本轮的“有效 id 子图头”仍不在其范围。
- CLI 退出码 3 与 02 §2.7.4 的 1 需在 spec/实现二选一（任务 M1 口径为 0/2/3，建议回写）。
- 修复后复审重跑必跑命令，并补 `REV-005` 修订段。

## 6. 条件关闭记录（仅"有条件通过"填写）

不适用（结论为"不通过"）。

## 7. 复审记录（2026-10-29，`57987d0`）

- 复审范围 `f942b33..57987d0`：21 files, +911/−164；全分支 `202d5c4..57987d0` 57 files, +6741/−29（含评审记录/进度）。
- 已确认修复：#1 on_error 作用域、#2 from_output_port、#3 子图 id、#5 缺边字段去重；#4 的 `field` 删除/节点与边/R2/R3 location 大体落实；#6 非法日期已拒绝。
- 仍未通过：#1 有效 SubgraphId 的子图头错误 location 为空且二次错误；#2 合法 `+00:00` 被拒绝。另有 `on_error.actual`、资产索引 location、测试覆盖等重要项。
- **结论维持不通过**；修完 #1/#2 与重要项后可再次复审。
