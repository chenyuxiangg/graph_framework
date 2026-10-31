# REV-074 · dsl · W2b 节点级 `max_fanout` + R16 静态扇出上限

> **跨模块声明**：本分支主体为 `dsl`，另在 `tests/fixtures/graphs/`（`e2e` 模块路径）**新增** 3 个 fixture（`valid_max_fanout.json` / `bad_max_fanout_exceeded.json` / `bad_max_fanout_value.json`）。属 Lead W2b 合同 task-1 §A5/§A6 明确交付范围（e2e 只读消费），非静默越界；本评审记录只新增 `doc/reviews/` 文件，评审人未修改产品/测试代码。

- **分支**：`feat/dsl-max-fanout`
- 分支: feat/dsl-max-fanout
- **作者**：`dsl-dev`（提交 `1fa4cc0`；`Review: REV-074` / `Agent: dsl-dev`）
- **评审人**：`reviewer-a`（独立评审，**≠ 作者**）
- **变更范围**：`7b4aba1..1fa4cc0`（11 files, +687/−19）：`crates/zgraph-dsl/src/{types,nodes,edges,hash,lib}.rs`、`crates/zgraph-dsl/tests/{fixtures,hash,rules}.rs`、新增 3 个 `tests/fixtures/graphs/*.json`
- **评审时间**：2026-10-31
- **关联**：Lead 合同 task-1 §A（W2b 口径冻结）、DR-004 §3 W2b / §5 D2、02 §2.2.2/§2.2.4/§2.4.1/§2.8、W2a 参考 REV-069（spec 正文更新由 W2b docs 分支 REV-077 负责）

## 1. 结论

**结论：通过**

按 Lead 冻结合同 §A1–A6 逐项核对，全部成立；未发现阻塞/重要问题，未发现改测试、削弱断言、跳过测试或越界改动。P3 建议 4 条（评审人已手工 CLI 验证其行为正确，仅缺固化用例）与 1 条跨模块声明，见 §3。

- **A1 字段与白名单**：`GraphNode.max_fanout: Option<u32>`（`types.rs:279-287`，`skip_serializing_if = "Option::is_none"`，None 不输出 null）；`NODE_KEYS` 7 键含 `max_fanout`（`nodes.rs:33-41`）；所有节点类型允许（测试覆盖 start/end/python_node/agent_call/subgraph）。
- **A2 取值校验**：整数 ≥1 → `Some`；0/负数/浮点/字符串/null/bool/超 `u32` → `DSL_PARSE_ERROR`，message 与冻结模板逐字一致（`'max_fanout' must be >= 1, got {actual}`），location `node_id`，`expected=">= 1"`、`actual=<原值>`（`nodes.rs:785-806`）。
- **A3 静态层 R16**：L2 纯函数 `edges::check_max_fanout(nodes, edges)`（`edges.rs:687-715`），对每个节点按 `nodes` 数组序取**首错**；`out_degree` = 以该节点为 `from` 的**全部边数（含 gate/defer 边）**；`DSL_PARSE_ERROR`（未新增 DSL 错误码/DslErrorCode 变体）；message/expected/actual/location 与冻结模板逐字一致；执行顺序 `R13→R15→R16→R14`（`lib.rs:208-220` 注释与调用位置一致）；子图在 `validate_subgraph_rules` 内以同函数执行、按 R6 收敛 `DSL_SUBGRAPH_INVALID` 且保留内部 `node_id`（`nodes.rs:1113-1118`）。
- **A4 hash**：canonical 投影仅在原文**显式存在** `max_fanout` 时加入（`hash.rs:114-118`），缺失不补齐；子图节点经同一 `project_node` 亦参与；既有图 hash 逐字节不变（golden 断言 + 实测）。
- **A5 fixtures**：3 个 fixture 内容与合同一致（valid：`max_fanout=2`、out-degree=2；exceeded：`1` vs out-degree=2；value：`0`）；`tests/fixtures.rs` BAD_CASES 增 2、VALID_CASES 增 1，沿用统一强断言（code/error_count=1/location/expected-or-actual）。
- **A6 测试与门禁**：解析/白名单/类型边界、主图与子图反例、hash 有/无字段、v0.1.0 golden 不变均有覆盖；`cargo test -p zgraph-dsl` 101 passed、workspace 709/0/3、fmt/clippy/check.sh/pytest 全绿（见 §4）。

## 2. 四维评审

| 维度 | 结论 | 依据（文件/行/测试） |
|---|---|---|
| 功能正确性 | ✅ | 需求/边界/异常逐项成立：A1–A6 全部落实（见 §1）；`cargo test -p zgraph-dsl` 101 passed 与作者报告一致；v0.1.0 回归用 main 既有二进制与分支 CLI 逐字节 A/B（36 个 pre-W2a fixture × human/--json = 72/72 完全一致，3 个 W2a `on_gate_false` fixture 的差异只来自基线二进制早于 W2a，非本分支影响），5 个既有 valid 图 hash 与 REV-069 记录逐字一致；新增 `valid_max_fanout` CLI hash `e05d882cc55aa275`，两个 bad fixture 均 exit 2 且 code/location/expected/actual 正确 |
| 代码结构合理性 | ✅（N4 备注） | R16 与 R13/R15 同置 `edges.rs`（均以边集为输入）、职责单一；`nodes.rs` 只做 R4 解析与子图侧调用（复用同一函数，无重复实现）；hash 仅 `project_node` 一处增量，未改投影其它键；未新增错误码/类型，`GraphNode` 仅加 1 个公开字段，无跨 crate 结构体字面量受影响；模块边界合规：仅 `crates/zgraph-dsl/**` + 合同允许的 3 个 fixture。唯一样式小重复见 N4（不阻塞） |
| 逻辑性 | ✅ | `out_degree` 统计全部边（含 gate/defer），实测 defer 边计入（`b`：1 普通 + 1 defer、`max_fanout=1` → out-degree 2 报错）；首错按 `nodes` 数组序确定（交换节点顺序报错对象随之变化，`error_count=1`）；R15 先于 R16（defer 不可重入 + 超限 → `DSL_DEFER_NOT_REENTRANT`）、R16 先于 R14（未知 gate op + 超限 → `DSL_PARSE_ERROR`）实测成立；子图包装保留 expected/actual/location；`usize`→`u64` 比较避免截断；无 `max_fanout` 时循环全 `continue`、零副作用，v0.1.0 行为不变；R10 禁止同一 `(from,to_node)` 平行边（实测 `DSL_DUPLICATE_EDGE`），故"全部边数"≡"不同目标节点数"，不存在重复边计数歧义；无吞异常/死循环/新增 panic 点 |
| 可读性 | ✅（N1–N3 备注） | 命名与中文注释风格一致；`check_max_fanout`/`fanout_exceeded`/`parse_max_fanout` 均 ≤60 行；注释解释"为什么"（L2 纯函数、取首错原因、不补齐默认值保护 v0.1.0 hash、子图 R6 收敛）；message 模板以 doc comment 冻结；新增测试命名表达意图且断言强（message/location/expected/actual/error_count）；spec 正文尚为 W2a 口径属 D 分支职责，非本分支遗留 |

## 3. 发现清单

> 级别口径：P0 = 阻塞（必须修复才能给"通过"）；P1 = 重要（必须修复或登记 ISSUE）；P2 = 一般；P3 = 建议（可后置，但应进台账）。本分支 P0=0、P1=0、P2=0。

| # | 级别 | 位置 | 问题 | 建议 | 状态 |
|---|---|---|---|---|---|
| 1 | P3 建议 | `crates/zgraph-dsl/tests/rules.rs:1850` | R16 对 **defer 边**计入 out-degree 无显式单测（`r16_gated_edges_count_toward_out_degree` 只覆盖 gate 边）。实现按"全部边"计数，评审人手工 CLI 已实测正确（node `b`：1 普通 + 1 defer、limit 1 → `out-degree 2`）；缺固化用例 | 补 1 个 defer 边（合法可重入环内）反例，断言 out-degree 含 defer 边 | open（建议项） |
| 2 | P3 建议 | `crates/zgraph-dsl/tests/rules.rs:1950`、`crates/zgraph-dsl/src/nodes.rs:1113-1118` | 子图侧两处未固化：① R15→R16 执行顺序只测了主图；② 子图内 `max_fanout` **取值非法**（R4 路径）的收敛未测。评审人手工验证：子图内 `max_fanout=0` → `DSL_SUBGRAPH_INVALID`，message `subgraph 'inner': 'max_fanout' must be >= 1, got 0`，`location.node_id=pin`，expected/actual 保留 | 各补 1 例子图用例（顺序 + 取值非法收敛） | open（建议项） |
| 3 | P3 建议 | `crates/zgraph-dsl/tests/rules.rs:1803`、`crates/zgraph-dsl/tests/hash.rs:220` | 两个正向边界未固化：① `u32::MAX` 接受值（仅测了 `2^32` 溢出拒绝）；② 子图内部节点 `max_fanout` 参与 hash 投影（hash 测试只覆盖主图节点）。评审人实测：`max_fanout=4294967295` → valid（hash `d68e8b8d99af9fdf`）；`valid_subgraph` 内层节点加 `max_fanout=5` → hash 由 `e903b92cf900587c` 变为 `6bc7fca298280232` | 补 `u32::MAX` 接受用例与子图节点 hash 投影断言 | open（建议项） |
| 4 | P3 建议 | `crates/zgraph-dsl/src/nodes.rs:785-806` | `parse_max_fanout` 与既有 `parse_optional_u32` 的数值解析逻辑小范围重复。因 A2 冻结了独立 message 模板（与 `invalid_value` 模板不同）而独立成函数是**合理**的，不构成缺陷 | 可选：后续把"读 `as_u64` + `u32::try_from` + `>=1`"抽成共享 helper，两处各传错误构造器 | open（建议项） |
| 5 | P3 说明 | `tests/fixtures/graphs/{valid_max_fanout,bad_max_fanout_exceeded,bad_max_fanout_value}.json` | 跨模块（`e2e` 模块路径）新增 3 个 fixture。合同 §A5/§A6 明确要求放在该目录、允许新增，本记录已在顶部声明 | 合并时 Lead 知悉；W2b e2e 分支（REV-076）只读消费，勿改其内容 | open（声明） |

## 4. 验证证据

评审人**亲自执行**（worktree `/Users/zyzs/lczyzs/code/zgraph/.worktrees/dsl-dsl-dev`，`export CARGO_HOME=/Users/zyzs/lczyzs/code/zgraph/.cargo-home`，HEAD `1fa4cc0`，工作区干净）：

```bash
cargo test -p zgraph-dsl
# unit 1 + fixtures 6 + hash 15 + rules 78 + doctest 1 = 101 passed, 0 failed
cargo test -p zgraph-dsl --test fixtures --test hash
# fixtures 6 passed / hash 15 passed
cargo fmt --all -- --check                                   # exit 0
bash scripts/dev/check.sh                                    # CHECK PASSED（身份/远程/CLAUDE.md/禁用文件/分支名/py 语法/progress ≤30/fmt/git diff --check）
cargo test --workspace                                       # 709 passed, 0 failed, 3 ignored（66 个 test binary）
cargo clippy -p zgraph-dsl --all-targets -- -D warnings      # clean（exit 0）
cargo clippy --workspace --all-targets --locked -- -D warnings  # clean（exit 0，与 merge.sh 门禁同口径）
/Users/zyzs/lczyzs/code/zgraph/.venv/bin/python -m pytest -q  # 170 passed（rc=0）
```

**v0.1.0 逐字节回归（独立 A/B）**：

```bash
# baseline：main worktree /Users/zyzs/lczyzs/code/zgraph/target/debug/zgraph
#   main HEAD=7b4aba1（= origin/main，clean），二进制 Oct 31 12:56 构建（早于 W2a 合并，dsl/cli 源码其后无改动）
# branch  ：.worktrees/dsl-dsl-dev/target/debug/zgraph（HEAD 1fa4cc0，cargo build 后 fresh）
# 对 tests/fixtures/graphs 下 39 个既有 fixture，human 与 --json 两种模式比较 stdout+stderr+exit code
# 结果：identical=72 diff=6；差异 6 项 = 3 个 W2a fixture（bad_defer_not_cycle / bad_defer_value /
#       valid_gate_defer）各 2 模式 —— 原因：baseline 二进制早于 W2a 的 on_gate_false 合入，非本分支行为变化
# 36 个 pre-W2a fixture（5 valid + 31 bad）全部逐字节一致（含退出码）
```

**既有 hash / CLI 不变量**（分支 CLI 实测，与 REV-069 记录的 W2a 基线逐字一致）：

| fixture | 期望（REV-069 基线） | 实测 |
|---|---|---|
| `customer_ticket.json` | `ec320e4244b365ab` | 同 |
| `valid_cycle.json` | `0e8911b6b4003c7e` | 同 |
| `valid_parallel.json` | `b15f162f7f87fd58` | 同 |
| `valid_subgraph.json` | `e903b92cf900587c` | 同 |
| `valid_gate_defer.json` | `2ff1339bd3d6d928`（hash.rs golden） | 同 |

**新增行为 CLI 实测**：

```text
valid_max_fanout.json      → exit 0, graph_hash=e05d882cc55aa275
bad_max_fanout_exceeded    → exit 2, DSL_PARSE_ERROR,
  "node 'start' out-degree 2 exceeds max_fanout 1", location.node_id=start,
  expected="out-degree <= 1", actual="out-degree 2", error_count=1
bad_max_fanout_value       → exit 2, DSL_PARSE_ERROR,
  "'max_fanout' must be >= 1, got 0", location.node_id=start,
  expected=">= 1", actual=0, error_count=1
```

**评审人手工边界（/tmp 构造，未改仓库）**：

- `max_fanout=4294967295`（u32::MAX）→ valid（hash `d68e8b8d99af9fdf`）；`4294967296` → `DSL_PARSE_ERROR`（node_id=start）。
- defer 边计入 out-degree：`b`（1 普通 + 1 defer，`max_fanout=1`）→ `out-degree 2` 报错（同图 R15 通过，证明 R15→R16）。
- 首错按 `nodes` 序：`[start,p]` → 报 `start`；交换为 `[p,start]` → 报 `p`；均 `error_count=1`。
- 顺序：defer 不可重入 + 超限 → `DSL_DEFER_NOT_REENTRANT`（R15 先）；未知 gate op + 超限 → `DSL_PARSE_ERROR`（R16 先于 R14）。
- 子图：内层 `max_fanout=0` → `DSL_SUBGRAPH_INVALID` 且 `location.node_id=pin`、expected/actual 保留；内层节点加 `max_fanout=5` → hash 变化（参与投影）。
- 重复边口径：同源同目标不同端口被 R10 拒绝（`DSL_DUPLICATE_EDGE`），故"全部边数"计数确定、无歧义。

- 测试结果：Rust `0 failed`（`cargo test --workspace` 709 passed / 3 ignored）、Python `170 passed`；与 `doc/test-baseline.txt` 比对**无新增失败**。
- 分支基于最新 `origin/main`（`git merge-base --is-ancestor origin/main HEAD` 成立，`origin/main..HEAD` 1 个提交），工作区干净；提交信息含 `Review: REV-074` / `Agent: dsl-dev`，`check.sh` 身份/冻结文件校验通过。

## 5. 遗留问题

- **未在本分支登记 `doc/issues.md`**（本分支合同 §A6 限定只改 dsl 代码/tests + 新增 fixture，docs 模块由 W2b docs 分支负责）：§3 的 N1–N4（P3 建议）建议由 Lead 决定是否在 `doc/issues.md` 登记；N5 为跨模块声明，无台账动作。
- **spec 落盘待 D 分支**：当前 `doc/spec-v1.2/02-DSL与校验.md` 仍是 W2a 口径（R1–R15、执行顺序 `R1→R13→R15→R14`），`max_fanout`/R16 的 schema、白名单、规则行、执行顺序、错误模板、hash 参与需由 W2b docs 分支（REV-077 / task-1 §D）补齐；合并顺序建议实现分支先合、docs 后合，以保持 spec 与实现一致。
- **新增 3 个 fixture 的消费方**：由 W2b e2e 分支（REV-076 / task-1 §C）只读消费；本分支未修改既有 39 个 fixture。
- 未发现需要回滚/返工的问题；无阻塞项。

## 6. 条件关闭记录（仅"有条件通过"填写）

不适用（结论为 `通过`，无附加条件）。
