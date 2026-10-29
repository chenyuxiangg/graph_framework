# REV-005 · dsl · M1 DSL 类型/loader/R1–R14/graph_hash/validate CLI

> **跨模块声明**：本分支以 `dsl` 为主，另触及 `crates/zgraph-cli`（`validate` 命令）、`tests/fixtures/graphs|assets`（e2e 归属）、`doc/progress.md` 与 `doc/issues.md`（任务台账）。属 Lead 指定的 M1 交付，非静默越界；评审记录此声明。

- **分支**：`feat/dsl-m1-loader`
- **作者**：`dsl-spec-writer`（实现 `4e80cd1`、遗留 `f344fd7`、进度 `f28aaa3`、两轮修正 `57987d0`/`34e6cc3`）
- **评审人**：`docs-reviewer`（独立评审，非作者）
- **变更范围**：`202d5c4..34e6cc3`（全分支 61 files, +7048/−29；最终代码修正 `5244981..34e6cc3` 为 12 files, +350/−48；`202d5c4..f344fd7` 为 45 files/+5866/−26 的初始实现）
- **评审时间**：2026-10-29
- **关联**：02 §2.1–§2.8（R1–R14、错误对象、CLI、graph_hash）、00 §0.4/§0.1（UTC RFC3339）、architecture §8 D6/D7、07 AC-04；ISSUE-026/027/028/029/030/031/032

## 1. 结论

**结论：通过**

`34e6cc3` 已关闭上一轮 REV-005 的 2 个阻塞和 4 个重要项，复核证据如下：

1. **子图头 location + 二次错误**：`parse_subgraph` 先解析 `id`，有效 id 的头错误统一 `node_id`；实测 `bad_subgraph_header_unknown` / `bad_subgraph_header_on_error` → `DSL_PARSE_ERROR`、`location={"node_id":"inner"}`、`error_count=1`，不再出现 “not defined” 二次错误。
2. **UTC RFC3339 `+00:00`**：`valid_created_at_offset`（`.123+00:00`）exit 0；`bad_created_at_offset`（`+08:00`）exit 2；`Z` 仍通过。
3. **保留字段 actual**：节点级 `on_error` 实测 `actual={"field":"on_error","value":{"route":"fallback"}}`；边 `from_output_port` 同形状；`expected` 仍为 `<absent>`。
4. **资产定位**：缺失 agent 定位到首个引用节点（`location={"node_id":"agent_node"}`，重复引用只报 1 条）；索引级 duplicate/invalid 按 ISSUE-032 走文档级 `{}`、message 带 `assets.agents[i]`。
5. **回归测试**：`cargo test --workspace` = **99 passed**；`cargo fmt/clippy`、`check.sh` 全过；合法图 hash 仍 `ec320e4244b365ab`；无新依赖/Cargo.lock 变化。

M1 DoD 的 R1–R14、错误对象、graph_hash、CLI、D6/D7、fixtures 主体验收通过；ISSUE-026/027/028/029/030/031/032 已登记为后续/边界事项，无未登记阻塞。

## 2. 四维评审

| 维度 | 结论 | 依据（文件/行/测试） |
|---|---|---|
| 功能正确性 | ✅ | R1–R14 主路径、错误对象、graph_hash、CLI 均通过；上一轮 #1–#6 有实测/测试；99 个测试全绿；合法图 hash 不变；D6/D7 无 Python import；子图加载期通过、运行期拒绝留 M2。 |
| 代码结构合理性 | ✅ | `types/nodes/edges/symbols/assets/hash/error/shape` 职责清晰；R1–R14 分阶段；`run_worker` 之外的 DSL/CLI 边界无越界（CLI/fixtures/docs 已声明）；`parse_edges`/子图头/资产定位收敛。 |
| 逻辑性 | ✅ | fail-fast/聚合、R13 DFS、hash 六步、子图 id 先解析后挂 location、RFC3339 日历/offset、资产首个引用节点去重均自洽；无竞态/死循环/生产 panic。 |
| 可读性 | ✅ | 模块与函数文档清晰；无生产 `unwrap/expect/panic`；函数 ≤60 行；fixtures 强定位表；错误码/CLI 输出一致；遗留项均有 ISSUE 记录。 |

## 3. 发现清单

| # | 级别 | 位置 | 问题 | 建议 | 状态 |
|---|---|---|---|---|---|
| 1 | 已关闭（前次阻塞） | `crates/zgraph-dsl/src/shape.rs:16-27`；`crates/zgraph-dsl/src/nodes.rs:311-330`；`crates/zgraph-dsl/src/edges.rs:188-191` | 前次 `on_error` 全局跳过/只报字段名；现已按作用域区分，节点级 Unsupported、其余 ParseError，actual 为字段名+值。 | 保持现有测试：`bad_on_error_*` + `rev005_2_node_on_error_actual...`。 | fixed（34e6cc3） |
| 2 | 已关闭（前次阻塞） | `crates/zgraph-dsl/src/edges.rs:188-191`；`crates/zgraph-dsl/src/error.rs:296-304` | 前次 `from_output_port:123` 报 ParseError；现出现即 Unsupported，actual 为 `{"field":...,"value":...}`。 | 保持 `bad_from_output_port_nonstring`。 | fixed（34e6cc3） |
| 3 | 已关闭（前次阻塞） | `crates/zgraph-dsl/src/nodes.rs:991-1045` | 前次有效 SubgraphId 的子图头错误 location `{}` 且二次报错；现先解析 id、头错误挂 `node_id`、不再级联。 | 保持 `bad_subgraph_header_unknown/on_error` + `rev005_2_valid_subgraph_header...`。 | fixed（34e6cc3） |
| 4 | 已关闭（前次重要） | `crates/zgraph-dsl/src/assets.rs:21-75,103-139` | 前次资产索引 duplicate/invalid 无 location、缺失 agent 可能重复报；现缺失 agent 定位首个引用节点且按 agent_id 去重，索引级按 ISSUE-032 文档级+`assets.agents[i]`。 | ISSUE-032 已登记 spec 例外；后续按 02 §2.6.3 回写即可。 | fixed/已登记（34e6cc3） |
| 5 | 已关闭（前次重要） | `crates/zgraph-dsl/src/edges.rs:25-60` | 前次缺边字段重复报错；现 `from/to_node/to_input_port` 各只报 1 条 Missing，`bad_missing_edge_field` count=1。 | 保持 `bad_missing_edge_field`。 | fixed（34e6cc3） |
| 6 | 已关闭（前次重要） | `crates/zgraph-dsl/src/shape.rs:125-190` | 前次 `created_at` 只接受 Z；现接受 `Z`/`+00:00`（含小数秒），拒绝非零 offset；非法日期仍拒绝。 | 保持 `valid_created_at_offset` + `bad_created_at_offset`。 | fixed（34e6cc3） |
| 7 | 重要（已登记） | `doc/issues.md:38-39`；`crates/zgraph-dsl/src/assets.rs:8-9`；`crates/zgraph-dsl/src/symbols.rs:327-397`；`crates/zgraph-cli/src/validate.rs:32-66` | 已登记 ISSUE-026/027/028/029/030/031/032：D6/D7 gate params 语义、无 assets-dir 跳过 R14c、07 路径、CLI `--structure-only`/`--print-hash`、blake3 边界、无 SubgraphId 子图头、资产 index-level location。 | 评审接受为已登记范围；ISSUE-026/027/029 的 spec/CLI 口径在 M2 前锁定。 | open（已登记） |
| 8 | 建议 | `doc/issues.md:38` | 用户称 ISSUE-031 已补“有效 id 子图头必须带 node_id”，但当前文本仍只描述 id 缺失/空/非 string 的无 SubgraphId 情形；代码/测试已覆盖有效 id，仅台账文案未体现。 | 在 ISSUE-031 增补一句“有效 SubgraphId 时头错误必须 node_id=SubgraphId（34e6cc3 已实现）”。 | open（建议） |
| 9 | 建议 | `crates/zgraph-cli/src/main.rs:3-6`；`doc/spec-v1.2/02-DSL与校验.md:710-716` | 02 §2.7.4 规定 validate 的“其他错误”退出码为 `1`，M1 按任务口径用 `3`；文档仍写 1。 | 回写 02/06 的 M1 口径或改实现，避免双重口径（ISSUE-029 一并收口）。 | open（建议） |

## 4. 验证证据

### 4.1 最终范围

```bash
cd /Users/zyzs/lczyzs/code/zgraph/.worktrees/dsl-dsl-impl
git log --oneline -3
# 34e6cc3 fix(dsl): 关闭 REV-005 复审阻塞项（子图头 location/RFC3339 +00:00/资产定位/actual）
# 5244981 docs(review): REV-005 复审（57987d0，仍不通过）
# 57987d0 fix(dsl): 关闭 REV-005 阻塞项...
git diff --shortstat 5244981..34e6cc3
# 12 files changed, 350 insertions(+), 48 deletions(-)
git diff --shortstat 202d5c4..34e6cc3
# 61 files changed, 7048 insertions(+), 29 deletions(-)（含评审记录/进度）
```

### 4.2 必跑命令

```bash
export CARGO_HOME="$PWD/.cargo-home" HTTPS_PROXY="http://127.0.0.1:7890" HTTP_PROXY="http://127.0.0.1:7890"
cargo fmt --all -- --check                       # exit 0
cargo clippy --workspace --all-targets -- -D warnings  # exit 0
cargo test --workspace                           # exit 0，99 passed
# dsl 69：unit 1 + fixtures 5 + hash 7 + rules 55 + doctest 1
# cli 15：unit 7 + integration 8
# worker 15：unit 7 + integration 8
cargo run -q -p zgraph-cli -- validate tests/fixtures/graphs/customer_ticket.json \
  --assets-dir tests/fixtures/assets
# ✓ graph valid: 6 nodes, 6 edges, 0 subgraphs (graph_hash=ec320e4244b365ab), exit 0
cargo run -q -p zgraph-cli -- validate tests/fixtures/graphs/bad_port.json --json
# DSL_INVALID_PORT, location={node_id:n1,edge_index:0,port:zzz}, exit 2
bash scripts/dev/check.sh                        # CHECK PASSED
```

### 4.3 手工抽查（34e6cc3）

| 反例/正例 | 期望 | 实际 |
|---|---|---|
| 有效 id 子图头未知字段 | ParseError / `node_id:"inner"` / 1 条 | **通过**：`location={"node_id":"inner"}`、`error_count=1`、exit 2 |
| 有效 id 子图头 `on_error` | ParseError / `node_id:"inner"` / 1 条 | **通过**：同上，无级联 |
| `created_at:"2026-10-29T10:00:00+00:00"` | 合法，exit 0 | **通过**：exit 0，hash 不变 |
| `created_at:"... .123+00:00"` | 合法，exit 0 | **通过**：exit 0 |
| `created_at:"... +08:00"` | 拒绝，exit 2 | **通过**：ParseError，exit 2 |
| 节点级 `on_error` | Unsupported，actual 字段+值 | **通过**：`actual={"field":"on_error","value":{"route":"fallback"}}` |
| 边 `from_output_port:123` | Unsupported，actual 字段+值 | **通过**：`actual={"field":"from_output_port","value":123}` |
| 缺失 agent | node_id 首个引用节点，只 1 条 | **通过**：`location={"node_id":"agent_node"}` |
| 索引级 duplicate/invalid | `{}` + `assets.agents[i]` | **通过**：`location={}`、message 含 `assets.agents[1]/[0]`，ISSUE-032 |

### 4.4 其他确认

- `graph_hash`：customer fixture golden `ec320e4244b365ab` 不变；格式化/键序、timeout、端口 description 测试通过；白名单排除 description/author/created_at。
- fixtures：36 个图文件（31 坏 + 5 合法）均被引用；强定位表 `code+location+error_count` 通过。
- D6/D7：代码检查无 Python `import`/`eval`；`python_node` 仅格式校验；`valid_subgraph` 加载期通过、运行期拒绝留 M2。
- 无新依赖/Cargo.lock 变化；未 push/merge；仅改本评审记录与 `doc/reviews/README.md`。

## 5. 遗留问题

- **无阻塞项**；M1 DoD 的 R1–R14、错误对象、graph_hash、CLI、D6/D7、fixtures 验收通过。
- 已登记：ISSUE-026（D6/D7 gate params/冲突语义）、ISSUE-027（无 assets-dir 跳过 R14c）、ISSUE-028（07 fixtures 路径）、ISSUE-029（schema/--structure-only/--print-hash/语义校验）、ISSUE-030（blake3 边界）、ISSUE-031（无 SubgraphId 子图头文档级）、ISSUE-032（资产 index-level location 例外）。评审接受为已登记范围。
- **台账建议**：ISSUE-031 当前文本仍只描述无 SubgraphId 情形，建议补“有效 id 子图头必须 node_id”（代码/测试已在 34e6cc3 实现）；不影响本次通过。
- **CLI 建议**：02 §2.7.4 的 validate “其他错误”退出码 `1` 与 M1 的 0/2/3 口径需在 02/06 回写或实现二选一（并入 ISSUE-029）。
- 合并后由 Lead 更新 `doc/progress.md` 与 ISSUE 状态。

## 6. 关闭证据（2026-10-29）

| 项 | 关闭证据 | 日期 |
|---|---|---|
| 前次 #1 on_error 作用域/actual | `bad_on_error_*` 5 个 fixture + `rev005_2_node_on_error_actual...`；实测顶层/config/port/edge/gate 均 exit 2，节点级 actual 字段+值 | 2026-10-29 |
| 前次 #2 from_output_port | `bad_from_output_port_nonstring` + `error.rs::unsupported_field`；实测 `actual={"field":...,"value":...}` | 2026-10-29 |
| 前次 #3 子图 id/头 location | `bad_subgraph_id_empty/type`、`bad_subgraph_header_unknown/on_error`；有效 id header `location={"node_id":"inner"}`、count=1、无级联 | 2026-10-29 |
| 前次 #4 资产/定位 | `bad_agent_not_found` node_id、`rev005_2_missing_agent...`；index-level 按 ISSUE-032 文档级+`assets.agents[i]` | 2026-10-29 |
| 前次 #5 缺边字段 | `bad_missing_edge_field`、`rev005_missing_edge...`；1 条 Missing、edge_index=0 | 2026-10-29 |
| 前次 #6 created_at | `valid_created_at_offset`（Z/+00:00/小数秒）exit 0；`bad_created_at_invalid`/`bad_created_at_offset` exit 2 | 2026-10-29 |
| 全量验证 | `cargo test --workspace` 99 passed、`check.sh` PASS、合法 hash `ec320e4244b365ab` | 2026-10-29 |

## 7. 复审记录（2026-10-29，`34e6cc3`）

- 复审范围 `5244981..34e6cc3`：12 files, +350/−48；全分支 `202d5c4..34e6cc3` 61 files, +7048/−29（含评审记录/进度）。
- 已关闭：#1 子图头有效 id location/级联、#2 `+00:00` RFC3339、#3 `on_error`/`from_output_port` actual、#4 资产缺失首个引用节点/index-level（ISSUE-032）、#5 缺边去重、#6 非法日期。
- 验证：`cargo fmt/clippy/test`（99 passed）、`check.sh`、合法图 hash、必跑 CLI 全通过；无新依赖/Cargo.lock。
- 剩余仅 ISSUE-026/027/028/029/030/031/032 已登记项与两条建议（ISSUE-031 文案、CLI 退出码口径），不阻塞合并。
- **结论：通过**。本分支可执行 M1 合并；合并后由 Lead 更新 progress/issues 与 worktree 清理。
