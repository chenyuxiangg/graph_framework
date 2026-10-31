# REV-069 · dsl · W2a 边级 `on_gate_false` + R15 defer 可重入校验

> **跨模块声明**：本分支以 `dsl` 为主，另新增 `tests/fixtures/graphs/` 下 3 个 fixture（`e2e` 模块归属）。属 Lead 指定的 W2a dsl 交付范围（REV-069 brief，DR-004 §4 W2a 波次），非静默越界；已按 AGENTS §2 由 Lead 指定独立评审，本记录不含产品/测试改动。

- **分支**：`feat/dsl-gate-defer`
- **作者**：`dsl-writer`（提交 `01b3d28`，`Review: REV-069` / `Agent: dsl-writer`）
- **评审人**：`reviewer-b`（独立评审，**≠ 作者**）
- **变更范围**：`7f88614..01b3d28`（12 files, +829/−28；`crates/zgraph-dsl/src/{types,edges,error,hash,lib,nodes}.rs`、`crates/zgraph-dsl/tests/{fixtures,hash,rules}.rs` 共 9 文件；新增 `tests/fixtures/graphs/{valid_gate_defer,bad_defer_not_cycle,bad_defer_value}.json`）
- **评审时间**：2026-10-31
- **关联**：DR-004 §3 W2a0 / §5 D1；DR-005（`docs/docs-rework-design`，REV-067 待合并，本评审以 brief 契约为准并参考其 §2.5/§2.8）；REV-063 C11/S12；02 §2.2.2/§2.2.4/§2.4.2/§2.7/§2.8/§2.9；ISSUE-100

## 1. 结论

**结论：通过**

按 Lead 冻结契约逐项核对，全部成立：

- **字段与解析**：`GraphEdge.on_gate_false: Option<OnGateFalse>`，`OnGateFalse::{Settle(Default), Defer}`；DSL 仅接受 `"settle"`/`"defer"`，缺失 = `None`（不写回、不进 hash）；其它字符串/非字符串 → `DSL_PARSE_ERROR`（边级 `edge_index`）。手工实测 null/number/bool/object/array 五种非字符串与未知字符串均被正确拒绝。
- **hash 兼容（硬回归）**：canonical 投影仅在 DSL 原文显式写 `on_gate_false` 时加入，未补齐默认值。以 **main 现有 CLI** 与 **本分支重新构建的 CLI** 逐字节对比：5 个 `valid_*.json` 输出（含 `graph_hash`）完全一致、31 个 `bad_*.json --json` 输出与退出码完全一致；`software_team_v1` codegen full/smoke 仍为 W1 基线 `3d8d735b1d5e8a1b` / `df9341c04063b80a`。**v0.1.0 hash 与 validate 行为零变化**。
- **R15**：`G_all`（全部边，含 gate 边/自环）上 Kosaraju SCC；判定 = defer 自环，或 `|SCC(u)|≥2` 且 SCC 内存在两端同 SCC 的 defer 边；定位 `edge_index` + `node_id=u`；同规则取 edges 数组序首个违规；message 与冻结模板逐字一致；子图递归并收敛为 `DSL_SUBGRAPH_INVALID`（沿用 02 §2.4.2 的 `subgraph '<id>': …`）。
- **执行顺序**：`R1→R13→R15→R14`。手工构造「R15 违规 + 非法 gate symbol」图，返回 R15 而非 R14；R13 代码零改动（`check_cycles` 仅注释变化），无 gate defer 自环先报 `DSL_CYCLE_DETECTED`。
- **门禁**：`cargo test -p zgraph-dsl` 85 passed；`cargo test --workspace` 664 passed / 0 failed / 3 ignored；`Cargo clippy`（dsl 与 workspace `-D warnings`）、`cargo fmt --check`、`check.sh`、`pytest` 104 passed 全绿。

发现 0 个阻塞、0 个重要问题；8 条建议中 **N1（R15 `expected`/`actual` 与待合并的 W2a0 冻结模板不一致）** 需在 W2a0 docs/REV-067 合并前由 Lead 裁决对齐（brief 未冻结该明细，故不阻塞本分支），其余为非阻塞文档/测试覆盖/一致性建议。未发现改测试、削弱断言或越界改动。

## 2. 四维评审

| 维度 | 结论 | 依据（文件/行/测试） |
|---|---|---|
| 功能正确性 | ✅（N1/N7 备注） | 字段/枚举/类型桶/hash 投影/R15/执行顺序/子图递归逐项符合 brief；v0.1.0「5 valid + 31 bad」CLI 输出与 main 二进制逐字节一致，W1 full/smoke hash 不变；R15 正反例（含跨 SCC、自环、非 defer 零误报、子图）实测与单测通过；N1 为与待合并 W2a0 冻结模板的 `expected`/`actual` 细节差异 |
| 代码结构合理性 | ✅ | `edges.rs` 新增 `check_defer_reentrancy` + 独立迭代式 Kosaraju（47/38/27 行），规则职责单一；结构解析仍在 `parse_edges`（R4 桶）、规则在 R15，与 R13 分离；hash 仅 `project_edge` 一处增量；子图复用同一 R15 函数，无重复实现；仅新增 `OnGateFalse` 公开类型（W2a0 A2 要求） |
| 逻辑性 | ✅ | SCC 方向正确（正/反向 DFS + 逆完成序）；`G_all` 含 gate 边与自环；defer 边判「两端同 SCC」；同规则首错、确定性（节点/边数组序）；缺端点边跳过避免与 R8 级联；无 defer 图早退零开销；无死循环/吞异常/生产 panic |
| 可读性 | ✅（N4/N5/N6 备注） | 命名与中文注释风格一致；新函数均 ≤60 行（最长 47）；注释解释「为什么」（G_all vs R13 的 G'、不补齐默认值、缺端点跳过）；测试命名/断言可读；N4 为 3 处 R1–R14/R1–R13 残留表述、N5 为允许值双源、N6 为 `GraphEdge` Serialize 的 None→null 语义 |

## 3. 发现清单

> 级别口径：阻塞 = 必须修复后才能给「通过」；重要 = 必须修复或登记 ISSUE；建议 = 可后置但需修复或进台账。

| # | 级别 | 位置 | 问题 | 建议 | 状态 |
|---|---|---|---|---|---|
| N1 | 建议 | `crates/zgraph-dsl/src/edges.rs:548-564` | R15 `expected`/`actual` 与 W2a0 参考冻结（DR-005 §2.5.2；docs 分支 02 §2.4.2 `expected`/`actual` 表）不一致：实现 `expected="source node on a cycle containing a defer edge"`、`actual={"from","to_node","on_gate_false"}`；冻结模板要求完整 expected 串、actual 含 `node_id`/`scc_size`/`defer_edges`（升序）。REV-069 brief 未冻结该明细（以 brief 为准 → 非阻塞），但 W2a0 docs 合并后 main 上会出现 code/spec 偏差 | 合并前由 Lead 裁决：实现对齐 W2a0 模板，或在 W2a0 docs 追加修订并回写 02 §2.4.2；同时通知 e2e 分支按最终口径断言 | open（建议，跨分支协调） |
| N2 | 建议 | `crates/zgraph-dsl/tests/rules.rs:1448-1471` | R15 反例测试对 `expected`/`actual` 仅断言 `is_some()`，未锁定错误对象内容；N1 的模板漂移未被测试捕获 | 按 N1 最终口径补精确断言（至少 `expected` 模板串与 `actual.node_id`/`edge_index`，如含 `scc_size`/`defer_edges` 一并锁定） | open（建议） |
| N3 | 建议 | `crates/zgraph-dsl/tests/rules.rs:1374-1400`、`tests/fixtures/graphs/bad_defer_value.json` | `on_gate_false` 非字符串类型单测仅覆盖 `bool`（+ 未知字符串 fixture）；`null`/`number`/`object`/`array` 由本次评审手工 CLI 验证均正确（`expected:"string"`、`actual`=原值、`edge_index` 正确），但无自动化用例 | 表驱动补 4 例，锁定 R4 类型桶行为 | open（建议） |
| N4 | 建议 | `crates/zgraph-dsl/src/hash.rs:18`、`crates/zgraph-dsl/src/lib.rs:3`、`crates/zgraph-dsl/tests/rules.rs:1` | 文档/注释残留「调用前置：图已通过 R1–R13」「语义基准 … R1–R14」「R1–R14 的规则级测试」，与已实现 R1–R15 不一致 | 同步为 R1–R15（含 R15） | open（建议） |
| N5 | 建议 | `crates/zgraph-dsl/src/edges.rs:26` | `ON_GATE_FALSE_VALUES: [&str; 2]` 与 `OnGateFalse::ALL`/`as_str` 是同一允许值的两份来源；枚举扩展时可能漂移 | `expected` 由 `OnGateFalse::ALL` 派生（或反之），保留单一事实源 | open（建议） |
| N6 | 建议 | `crates/zgraph-dsl/src/types.rs:333-335` | `GraphEdge` derive `Serialize`，`on_gate_false: Option<OnGateFalse>` 未加 `skip_serializing_if`：作者未写时序列化为 `"on_gate_false":null`，而 DSL 中显式 null 是非法值；「None=作者未写」语义在序列化形态上有歧义（当前无消费者，hash 用 raw JSON，不影响契约） | 加 `#[serde(skip_serializing_if = "Option::is_none")]`，或在注释声明「结构体序列化形态不用于回灌 DSL」 | open（建议） |
| N7 | 建议 | `crates/zgraph-dsl/src/nodes.rs:1078-1081`（子图递归） | 子图内 R13 与 R15 同时命中时聚合为 **2 条** `DSL_SUBGRAPH_INVALID`；主图因 R1→R13→R15→R14 跨规则 fail-fast 只报 R13。实测：子图 `a↔c` 无 gate 环 + `a→e2` 非法 defer → 2 条（cycle + not re-entrant）。属既有「子图内 R4/R5/R8–R13 统一聚合」口径的延续（非本分支引入），但 R15 新加入该聚合路径 | 建议 W2a-docs 在 02 §2.6.1/R6 明确「子图内跨规则聚合」为例外（或改为 fail-fast），避免与 §2.1.3 字面冲突 | open（建议） |
| N8 | 建议 | `crates/zgraph-dsl/src/edges.rs:500-564`；brief §2 R15 | brief 写「子图递归 … 收敛为 `DSL_SUBGRAPH_INVALID`（message 前缀 `subgraphs[i]`）」；实现沿用 02 §2.4.2 R6 既有口径 `subgraph '<id>': {detail}`（CLI 定位串才是 `graph.json: subgraphs[i]`），与既有 spec/REV-005 一致。二者措辞不同但实现无误 | 建议 Lead 知悉并在 W2a0/W2a-docs 统一措辞；实现无需改动 | open（说明，非缺陷） |

## 4. 验证命令与结果

### 4.1 范围、基线、干净度

```bash
cd /Users/zyzs/lczyzs/code/zgraph/.worktrees/dsl-dsl-writer
git status --porcelain --branch   # ## feat/dsl-gate-defer...origin/main [ahead 1]；评审前 clean
git log --oneline -2              # 01b3d28 feat(dsl): 边级 on_gate_false + R15 defer 可重入校验
                                  # 7f88614 docs(process): W1 完成收口——进度、台账（ISSUE-101..103）
git merge-base HEAD origin/main   # 7f88614（base = origin/main）
git show --numstat --format='' 01b3d28 | awk '{a+=$1;d+=$2} END{print a,d}'   # 829 28
git show --stat 01b3d28           # 12 files：dsl src/tests 9 + fixtures 3；无 doc/CLAUDE/CLI/kernel/trace/e2e 产品代码
git diff --name-status 7f88614..01b3d28
# M crates/zgraph-dsl/src/{edges,error,hash,lib,nodes,types}.rs
# M crates/zgraph-dsl/tests/{fixtures,hash,rules}.rs
# A tests/fixtures/graphs/{valid_gate_defer,bad_defer_not_cycle,bad_defer_value}.json
```

### 4.2 门禁（在本 worktree 私有 target 上执行）

```bash
export CARGO_HOME=/Users/zyzs/lczyzs/code/zgraph/.cargo-home
cargo test -p zgraph-dsl --offline
# unit 1 + fixtures 5 + hash 11 + rules 67 + doctest 1 = 85 passed, 0 failed
cargo test --workspace --offline
# 664 passed, 0 failed, 3 ignored（exit 0；基线名额 0，无新增失败）
cargo clippy -p zgraph-dsl --all-targets --offline -- -D warnings   # clean, exit 0
cargo clippy --workspace --all-targets --offline -- -D warnings     # clean, exit 0（merge 门禁口径）
cargo fmt --all -- --check                                          # exit 0
bash scripts/dev/check.sh                                           # CHECK PASSED
/Users/zyzs/lczyzs/code/zgraph/.venv/bin/python3 -m pytest -q       # 104 passed, rc=0
```

### 4.3 hash 兼容强验证（main CLI vs 分支 CLI，逐字节）

```bash
# main：/Users/zyzs/lczyzs/code/zgraph/target/debug/zgraph（Oct 31 12:56 构建；main dsl 源码最后改动
#      c48e878 2026-10-31 00:24、cli 570c1bd 02:12，均早于该构建；main worktree clean，dsl/cli 无未提交改动）
# 分支：cargo build -p zgraph-cli --offline 后 ./target/debug/zgraph（01b3d28）
```

| fixture | main 输出 / hash | 分支输出 / hash | 结果 |
|---|---|---|---|
| `customer_ticket.json` | `6 nodes, 6 edges (graph_hash=ec320e4244b365ab)` | 同 | **逐字节一致** |
| `valid_cycle.json` | `3 nodes, 3 edges (graph_hash=0e8911b6b4003c7e)` | 同 | **逐字节一致** |
| `valid_parallel.json` | `4 nodes, 4 edges (graph_hash=b15f162f7f87fd58)` | 同 | **逐字节一致** |
| `valid_subgraph.json` | `3 nodes, 2 edges, 1 subgraphs (graph_hash=e903b92cf900587c)` | 同 | **逐字节一致** |
| `valid_created_at_offset.json` | `1 nodes, 0 edges (graph_hash=c6cad4f83161b50c)` | 同 | **逐字节一致** |

```bash
# 31 个 bad_*.json 的 validate --json：stdout/stderr + exit code 全部与 main 逐字节一致（identical=31 diff=0）
# 证明 parse/R8–R13/hash 前置行为零变化。
python3 tests/fixtures/graphs/software_team_v1/codegen.py --profile full|smoke --out /tmp/rev069/st_*.json
# full  : main=3d8d735b1d5e8a1b / 分支=3d8d735b1d5e8a1b（= W1 基线）  逐字节一致
# smoke : main=df9341c04063b80a / 分支=df9341c04063b80a（= W1 基线）  逐字节一致
# 显式值参与投影、缺省不补齐（同一 valid_cycle，改 edges[1]）：
# omitted=0e8911b6b4003c7e；显式 "settle"=68371a8eac478266；显式 "defer"=f940dfc259c5fd83
# 新 fixture valid_gate_defer.json = 2ff1339bd3d6d928（hash.rs golden 断言同值）
```

### 4.4 R15 边界（手工构造图 + 分支 CLI 实测）

```bash
./target/debug/zgraph validate <case>.json --json
```

| 场景 | 结果 |
|---|---|
| defer 环通过（SOP 骨架 `P --rework(defer)--> W -> R -> P` + `P --approved(defer)--> end`） | ok（`valid_gate_defer.json` 通过，hash 见上） |
| 源不在环上（叶边 defer） | `DSL_DEFER_NOT_REENTRANT`，`location={edge_index:1,node_id:"a"}`，exit 2 |
| 跨 SCC 拒绝（defer 源 `a` 所在 SCC 外另有 defer 环） | 同码，`edge_index=1`、`node_id="a"`（另一 SCC 的 defer 环不授权） |
| SCC 内含任一 defer 边即授权（`a→b` gate + `b→a` gate，`a→b` defer） | 通过（仅需 SCC 内一条同 SCC defer 边） |
| defer 自环（带 gate） | 通过；自环 `G_all` 计入 SCC |
| 无 gate defer 自环/环 | **先**报 `DSL_CYCLE_DETECTED`（R13 优先；实测 `loop -> loop (edges:[1])`） |
| 同一图多条违规 | 只报 edges 数组序首个（`rules.rs` 用例 + fail-fast） |
| 非 defer 图零误报 | 31 bad / 5 valid / W1 full+smoke 输出与 main 全同；`valid_subgraph.json` 无 R15 误报 |
| 子图收敛 | 子图内非法 defer → `DSL_SUBGRAPH_INVALID`、message `subgraph 'inner': …`、`location.node_id="pin"`/`edge_index=0`；子图内合法 defer 自环通过（hash `18661d19e21bbe89`） |
| 执行顺序 R15 先于 R14 | 「非法 defer + 非法 gate symbol」图 → 返回 `DSL_DEFER_NOT_REENTRANT`（R15），非 `DSL_SYMBOL_NOT_FOUND` |
| R15 `expected`/`actual` 实测 | `expected:"source node on a cycle containing a defer edge"`、`actual:{from,to_node,on_gate_false}` —— 见 N1 与 N2 |

### 4.5 差异审读（未改测试结论）

```bash
git diff 7f88614..01b3d28 -- crates/zgraph-dsl/src/edges.rs | grep -E '^-[^-]'
# 仅模块头注释、use、EDGE_KEYS 常量行 → R13 check_cycles 等既有规则代码零改动
git diff 7f88614..01b3d28 -- crates/zgraph-dsl/tests | grep -E '^-[^-]'
# 仅 BAD_CASES 31→33、VALID_CASES 5→6、模块头计数注释、import 调整 → 测试只增不减、无削弱
```

- `fixtures.rs`：31→33 bad（新增 `bad_defer_not_cycle`/`bad_defer_value`）、5→6 valid（`valid_gate_defer`），既有 golden 表的错误码/定位全部保留。
- `hash.rs`：新增 4 个 hash 投影用例（含 v0.1.0 `valid_cycle` 逐字节不变、显式 settle 不补默认、defer 变 hash、新 fixture golden），未改既有 golden。
- `rules.rs`：新增 12 个 R15/on_gate_false 用例；R13 优先、首错、跨 SCC、自环、子图、非 defer 零误报均有断言。

## 5. 遗留问题

- **N1（跨分支，需 Lead 裁决）**：R15 `expected`/`actual` 与 W2a0 待合并冻结模板（DR-005 §2.5.2 / docs 分支 02 §2.4.2）不一致。本分支按 brief 不阻塞合并；建议在 W2a0 docs（REV-067）合并前对齐（改实现或追加 spec 修订），并由 e2e 分支按最终口径断言。本次评审**未**修改 `doc/issues.md`（评审人只写评审记录，台账由 Lead/流程维护）。
- **ISSUE-027（既有，cli 模块，不在本分支范围）**：`zgraph validate --structure-only` 尚未实现；R15 属 L2 已在本 crate 的 `load_graph_value` 路径执行（库级已验证），CLI 该 flag 的 R15 覆盖需按 ISSUE-027 在 cli/process 波次收口。
- **ISSUE-100（R13/R15 规则）**：本分支 + W2a0 已实质关闭（R13 保持只查无 gate 环；R15 独立用 G_all；S12 环检测细则落地）；待 W2a0 docs 合并后由 Lead 更新台账状态。
- **合并前置**：分支 base = `origin/main` `7f88614`；按 DR-004 §4（W2a 依赖 W2a0）与 merge.sh 的「基于最新 origin/main」检查，合并前需 origin/main 已含 W2a0 docs（否则 rebase 后重跑门禁）。
- 未发现必须关闭的阻塞条件；未发现越界改动、冻结文件修改或测试削弱。

## 6. 条件关闭记录（仅"有条件通过"填写）

不适用：首轮结论即为 **通过**，无「有条件通过」待关闭条件；N1–N4 的追加确认见 §7。

## 7. 追加复审（N1–N4 关闭，2026-10-31）

- **复审范围**：`6b73071..a084f2e`（`fix(dsl): R15 错误载荷对齐冻结模板并补类型反例（REV-069 N1–N4）`，作者 `dsl-writer`；4 files, +111/−32：`crates/zgraph-dsl/src/{edges,hash,lib}.rs`、`crates/zgraph-dsl/tests/rules.rs`，无其它模块/fixture 改动）。复审前 worktree clean，分支 ahead 3（`01b3d28` + 评审 `6b73071` + 修复 `a084f2e`）。
- **N1（建议，已关闭）**：R15 载荷对齐 DR-005 §2.5.2 / 02 §2.4.2 W2a0 冻结模板。脚本逐字比对 DR-005 原文 `expected` 与 CLI 输出 **MATCH=True**；`actual` 形状为 `{"node_id","scc_size","defer_edges"}`，`defer_edges` 按 `edges` 数组序收集（索引递增 = 升序）；`message`/`location` 未变。CLI 实测（`--json`）：
  - `bad_defer_not_cycle.json`（源不在任何环）→ `edge_index:1`、`node_id:"a"`、`scc_size:1`、`defer_edges:[]`、exit 2；
  - 等价 `r15_gated_cycle_without_defer_edge_does_not_authorize_defer` 图（SCC `{a,b}` 大小 2、SCC 内无 defer 边）→ `scc_size:2`、`defer_edges:[]`；
  - `expected = "re-entrant defer source (cycle through source containing a defer edge, or defer self-loop)"`（与冻结模板逐字一致）。
- **N2（建议，已关闭）**：`tests/rules.rs` 4 处反例已锁定 code/expected/actual 精确值（不再只 `is_some()`）：`r15_defer_edge_source_without_cycle_is_rejected`（message 逐字 + `scc_size:1`/`[]`）、`r15_gated_cycle_without_defer_edge_does_not_authorize_defer`（`scc_size:2`）、`r15_first_violating_defer_edge_in_array_order_is_reported`（首错载荷）、`r15_violation_inside_subgraph_maps_to_subgraph_invalid`（子图收敛后载荷原样保留）；`expected` 以 `R15_EXPECTED` 常量锁定。
- **N3（建议，已关闭）**：新增 `on_gate_false_non_string_types_are_parse_errors` 表驱动，覆盖 `null`/`1`/`1.5`/`true`/object/array 六类；逐类断言 `DSL_PARSE_ERROR`、`error_count=1`、`edge_index=1`、`node_id=None`、message 含 `expects string`、`expected="string"`、`actual=原值`。原 bool 分支从旧用例移入该表，断言强于原实现。
- **N4（建议，已关闭）**：`grep -rn "R1–R14\|R1-R14\|R1–R13\|R1-R13" crates/zgraph-dsl/{src,tests}` 仅剩 `hash.rs:18` 的正确表述「R1–R13、R15」；`lib.rs` 模块头改 R1–R15 并补 DR-005 引用、`rules.rs` 模块头改 R1–R15；无 `17 个`/`17 码` 残留。
- **N5–N8**：按 Lead 裁决**不修**，原记录保留（N5 允许值双源、N6 `GraphEdge` Serialize `None→null`、N7 子图 R13+R15 聚合 2 条、N8 `subgraphs[i]` 措辞），均非阻塞。
- **回归复核（载荷/注释变更未引起回退）**：

  ```bash
  cargo test -p zgraph-dsl --offline   # unit 1 + fixtures 5 + hash 11 + rules 68 + doctest 1 = 86 passed, 0 failed
  cargo test --workspace --offline     # 665 passed, 0 failed, 3 ignored（较首轮 +1，来自新增类型用例）
  cargo clippy -p zgraph-dsl --all-targets --offline -- -D warnings   # clean（exit 0）
  cargo fmt --all -- --check           # exit 0
  bash scripts/dev/check.sh            # CHECK PASSED
  # v0.1.0 兼容复测（main CLI vs 修复后分支 CLI，逐字节）：
  # 5 valid：c6cad4f83161b50c / 0e8911b6b4003c7e / b15f162f7f87fd58 / e903b92cf900587c / ec320e4244b365ab —— IDENTICAL
  # 31 bad --json：stdout/stderr + exit code identical=31 diff=0
  ```

- **结论：通过（维持）**。N1–N4 已关闭；无新的阻塞/重要问题；本追加段仅改评审记录（评审人未改产品代码/测试）。合并仍须通过 `scripts/dev/merge.sh` 的全量门禁。
