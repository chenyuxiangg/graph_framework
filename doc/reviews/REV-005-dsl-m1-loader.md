# REV-005 · dsl · M1 DSL 类型/loader/R1–R14/graph_hash/validate CLI

> **跨模块声明**：本分支以 `dsl` 为主，另触及 `crates/zgraph-cli`（`validate` 命令）、`tests/fixtures/graphs|assets`（e2e 归属）、`doc/progress.md` 与 `doc/issues.md`（任务台账）。属 Lead 指定的 M1 交付，非静默越界；评审记录此声明。

- **分支**：`feat/dsl-m1-loader`
- **作者**：`dsl-spec-writer`（实现 `4e80cd1`、遗留登记 `f344fd7`；最终 tip `f28aaa3` 仅补 `doc/progress.md`）
- **评审人**：`docs-reviewer`（独立评审，非作者）
- **变更范围**：`202d5c4..f28aaa3`（最终 46 files, +5874/−29；用户给的 45 files/+5866/−26 为不含进度修正的 `202d5c4..f344fd7` 口径。`f28aaa3` 仅 4 行进度同步，不影响代码）
- **评审时间**：2026-10-29
- **关联**：02 §2.1–§2.8（R1–R14、错误对象、CLI、graph_hash）、00 §0.4、architecture §8 D6/D7、07 AC-04；ISSUE-026/027/028/029/030

## 1. 结论

**结论：不通过**

主体工程量和测试基础较好：6 个模块文件拆分清晰，`cargo fmt/clippy/test`、`check.sh`、必跑 CLI 全部通过；R1/R2/R3/R5/R7/R10/R11/R12/R13 及 R14 的 D6/D7 主路径有实现或测试；`graph_hash` 的 canonical 投影、golden hash、格式/内容敏感性测试通过；fixtures 17 码 + R3 版本反例 + 4 个合法图均有引用；`ISSUE-026/027/028/029/030` 已登记。

但仍存在**可复现的语义错误**，不是"测试没写全"这么简单：

1. **未知/保留字段 `on_error` 被静默吞掉**：`shape::check_keys` 对所有调用方跳过 `on_error`，只有节点级 `check_node_keys` 显式报 `DSL_UNSUPPORTED_FIELD`；顶层、节点 `config`、输入/输出端口、边、`gate`、子图定义中的 `on_error` 都被忽略，图仍返回 `ok:true`。02 §2.2.4 规定只有节点级 `on_error` 是保留字段，其余未知键必须 `DSL_PARSE_ERROR`。
2. **`from_output_port` 非字符串值返回错码**：`parse_from_output_port` 对 `123`/`null` 报 `DSL_PARSE_ERROR`；02 §2.2.4 明确"边级 `from_output_port` 出现即报错，不看取值"，应为 `DSL_UNSUPPORTED_FIELD`。
3. **子图 `id` 缺失类型/空值校验**：`parse_subgraph` 只取 `as_str().filter(!empty)`，不报错；当没有主图 `subgraph` 节点引用该定义时，`subgraphs[0].id=""` 或 `id:7` 会让整图 `ok:true`（无效子图被静默丢弃）。R6/`SubgraphDefinition` 要求 `id` 为必填 string。
4. **错误对象的 `location` 契约不符**：`Location` 多出 `field` 键，且大量节点/边级错误只填 `field`、没有 `node_id`/`edge_index`。例如节点未知字段返回 `location:{"field":"bogus"}`；R2/R3 文档级错误也带 `field`。02 §2.7.3 只允许 `node_id`/`edge_index`/`port`，且节点/边错误至少要有一个定位；R2/R3 文档级为 `{}`。这影响 CLI `--json` 的稳定对外形状。
5. **同一缺失边字段报两次**：`parse_edges` 先对 `from/to_node/to_input_port` 做 `contains_key` 检查，再由 `required_nonempty_str` 检查一次；缺 `from` 会生成两条完全相同的 `DSL_MISSING_FIELD`，`error_count=2`，违反"同规则内按违规项聚合"和稳定错误序列。
6. **`created_at` 的 RFC3339 校验过宽**：`is_rfc3339_utc` 只检查长度/分隔符/数字位，不校验月/日/时/分/秒范围；`2026-99-99T99:99:99Z` 被接受。R3 要求 UTC RFC3339。
7. **测试/ fixtures 未覆盖上述分支**：`bad_reserved_field` 只覆盖节点级 `on_error`；`r9_from_output_port_is_reserved` 只覆盖字符串；无空/数字子图 id、无顶层/config/端口/边/gate 的 `on_error`、无缺 `from` 的重复计数、无非法 `created_at`；`bad_fixtures_carry_structured_location` 仅检查 location 非空，`location.field` 也能通过，无法发现缺 `node_id`/`edge_index`。

因此当前不能合并；修复 #1–#6 并补对应测试后可复审。

## 2. 四维评审

| 维度 | 结论 | 依据（文件/行/测试） |
|---|---|---|
| 功能正确性 | ❌ | R1/R2/R3 主路径、R5/R7/R10–R13、R14 D6/D7、hash、CLI 主路径通过；但 R4/R6/R9 的未知/保留字段与子图 id 有无效图放行（#1–#3），错误对象与 CLI `--json` 不符 02 §2.7.3（#4），边字段缺键重复计数（#5），R3 时间戳过宽（#6）。 |
| 代码结构合理性 | ✅ | `types/nodes/edges/symbols/assets/hash/error/shape` 职责清楚；R1–R14 分阶段调用；CLI `validate` 独立；fixtures 与 tests 分离；无跨模块代码越界（CLI/fixtures/docs 已声明）。 |
| 逻辑性 | ⚠️ | 主流程 fail-fast/聚合、hash 六步、R13 DFS、R14 注册顺序总体自洽；但 `shape::check_keys` 的全局 `on_error` 跳过与调用方语义冲突，`parse_subgraph` 的 `id` 空值路径静默返回 None，`parse_edges` 双检缺键，均是可复现的逻辑漏洞。 |
| 可读性 | ✅ | 模块/函数文档齐全，命名一致；无生产代码 `unwrap/expect/panic`；函数均 ≤60 行；建议 #8–#10 的文档/工具链边界可读性可继续加强。 |

## 3. 发现清单

| # | 级别 | 位置 | 问题 | 建议 | 状态 |
|---|---|---|---|---|---|
| 1 | 阻塞 | `crates/zgraph-dsl/src/shape.rs:13-25`；`crates/zgraph-dsl/src/nodes.rs:306-321,589-610,939-996`；`crates/zgraph-dsl/src/edges.rs:41-49,69-103` | `check_keys` 对所有对象跳过 `on_error`；仅节点级显式报 Unsupported。顶层/config/端口/边/gate/子图的 `on_error` 被静默接受，`zgraph validate` 返回 0。 | 仅节点级直接键保留 Unsupported 并显式报；其余作用域不得跳过 `on_error`，按未列入白名单报 `DSL_PARSE_ERROR`。为各作用域加反例。 | open（阻塞） |
| 2 | 阻塞 | `crates/zgraph-dsl/src/edges.rs:106-119`；`crates/zgraph-dsl/src/edges.rs:164-207` | `from_output_port` 为 `123`/`null`/object 时走 `wrong_type` 报 `DSL_PARSE_ERROR`，且不进入 `check_edge_ports` 的 Unsupported 分支。02 §2.2.4 要求出现即 `DSL_UNSUPPORTED_FIELD`，不看取值。 | `parse_from_output_port` 不按类型解析，只要键存在就构造 `DSL_UNSUPPORTED_FIELD`（保留原始值到 actual）；删除/绕过类型错路径。 | open（阻塞） |
| 3 | 阻塞 | `crates/zgraph-dsl/src/nodes.rs:939-996`；`crates/zgraph-dsl/src/nodes.rs:882-910` | 子图 `id=""` / `id:7` 且无主图引用时，`parse_subgraph` 返回 `None` 不 push 错误，`check_subgraphs` 返回空定义；整图 `ok:true`。 | 用 `required_nonempty_str(map,"id",...)` 校验 string/non-empty；非 string 报 `DSL_SUBGRAPH_INVALID` 或 ParseError，且不得静默丢弃子图定义。 | open（阻塞） |
| 4 | 阻塞 | `crates/zgraph-dsl/src/error.rs:116-132`；`crates/zgraph-dsl/src/shape.rs:293-359`；`crates/zgraph-dsl/src/nodes.rs:240-330,424-590`；`crates/zgraph-dsl/src/edges.rs:25-120` | `Location` 增加 `field` 键；大量错误只有 `field`，无 `node_id`/`edge_index`。实测节点未知字段 `location:{"field":"bogus"}`；R2/R3 缺失/未知也带 `field`。02 §2.7.3 的 location 仅 `node_id/edge_index/port`，节点/边级至少一个，R2/R3 文档级 `{}`。 | 去掉 `Location.field`（或降为 message/expected，不进 location）；`check_keys`/`parse_input_port`/`parse_edges` 等按调用方补 `at_node`/`at_edge`；R2/R3 文档级留空。更新 CLI/human locator 与错误对象测试。 | open（阻塞） |
| 5 | 重要 | `crates/zgraph-dsl/src/edges.rs:41-49` | 缺 `from` 时预扫描 `contains_key` 和 `required_nonempty_str` 各报一次，CLI 实测 2 条完全相同的 `DSL_MISSING_FIELD`、`error_count=2`。 | 删除预扫描 for 循环，统一由 `required_nonempty_str` 报一次；或预扫描只报一次并让 required 不再重复。补 `error_count==1` 测试。 | open（重要） |
| 6 | 重要 | `crates/zgraph-dsl/src/shape.rs:113-159`；`crates/zgraph-dsl/src/nodes.rs:209-223` | `is_rfc3339_utc` 只按位置/数字校验；`metadata.created_at="2026-99-99T99:99:99Z"` 通过。R3 要求 UTC RFC3339。 | 校验月 1–12、日按时/月/闰年、时 0–23、分/秒 0–59（RFC3339 允许秒 60）、小数位；或引入成熟时间解析（仍保持无重依赖）。补非法日期反例。 | open（重要） |
| 7 | 重要 | `crates/zgraph-dsl/tests/rules.rs:143,223-241,495-512,852-867`；`crates/zgraph-dsl/tests/fixtures.rs:75-110`；`tests/fixtures/graphs/bad_reserved_field.json` | 测试未覆盖 #1–#6；`r2_missing_top_level_field` 主动断言 `location.field`，把 spec 偏差固化为测试；`bad_fixtures_carry_structured_location` 只检查 `!location.is_empty()`，`field` 即可通过。 | 补顶层/config/端口/边/gate/子图 `on_error`、非字符串 `from_output_port`、空/数字子图 id、缺 from 计数、非法 created_at、节点未知字段 location 的测试；修正断言到 02 §2.7.3。 | open（重要） |
| 8 | 重要（已登记） | `crates/zgraph-dsl/src/assets.rs:8-9,21-40`；`crates/zgraph-dsl/src/symbols.rs:327-397`；`doc/issues.md:33-37`；`crates/zgraph-cli/src/validate.rs:32-66` | 已知偏差：无 `--assets-dir`/`ZGRAPH_ASSETS` 时跳过 R14c（ISSUE-027）；gate params 契约是 M1 对 D7 的解读（ISSUE-026）；`--structure-only`/`--print-hash` 未实现（ISSUE-029）；`blake3` 属 process 边界（ISSUE-030）；fixtures 路径与 07 不一致（ISSUE-028）。 | 评审确认这些为**已登记**的 M1 范围/后续项；合并前至少把 ISSUE-026/027/029 的 spec 回写/CLI 口径锁定，避免 validate/run 行为分歧。 | open（已登记） |
| 9 | 建议 | `crates/zgraph-cli/src/main.rs:3-6`；`doc/spec-v1.2/02-DSL与校验.md:710-716` | 02 §2.7.4 规定 validate 的"其他错误"退出码为 `1`，M1 实现按任务口径合并为 `3`（文件不可读/用法）。任务要求 0/2/3，但 spec 仍写 1。 | 在 02/06 回写"M1 口径 0/2/3"或改实现；不要留双重口径。 | open（建议） |
| 10 | 建议 | `crates/zgraph-dsl/Cargo.toml:10`；`Cargo.lock` | `blake3` 直接加在 dsl crate，不在 workspace.dependencies；ISSUE-030 已登记 process 边界。 | 由 process 收口到 workspace 依赖；M1 可接受现状，但需在 ISSUE-030 关闭前明确。 | open（已登记） |

## 4. 验证证据

### 4.1 范围与环境

```bash
cd /Users/zyzs/lczyzs/code/zgraph/.worktrees/dsl-dsl-impl
git log --oneline -3
# f28aaa3 docs(process): 同步 M0 完成后的进度
# f344fd7 docs(process): 登记 M1 遗留项
# 4e80cd1 feat(dsl): M1 DSL 类型/loader/R1–R14/graph_hash/validate CLI
git diff --shortstat 202d5c4..f28aaa3
# 46 files changed, 5874 insertions(+), 29 deletions(-)
```

`f28aaa3` 只改 `doc/progress.md`；代码以 `4e80cd1`+`f344fd7` 为准。

### 4.2 必跑命令

```bash
export CARGO_HOME="$PWD/.cargo-home" HTTPS_PROXY="http://127.0.0.1:7890" HTTP_PROXY="http://127.0.0.1:7890"
cargo fmt --all -- --check                       # exit 0
cargo clippy --workspace --all-targets -- -D warnings  # exit 0
cargo test --workspace                           # exit 0
# DSL: unit 1 + rules 45 + fixtures 4 + hash 7；
# CLI: unit 7 + integration 8；worker 既有 7 unit + 8 integration 全绿
cargo run -q -p zgraph-cli -- validate tests/fixtures/graphs/customer_ticket.json \
  --assets-dir tests/fixtures/assets
# ✓ graph valid: 6 nodes, 6 edges, 0 subgraphs (graph_hash=ec320e4244b365ab)
cargo run -q -p zgraph-cli -- validate tests/fixtures/graphs/bad_port.json --json
# {"ok":false,...,"code":"DSL_INVALID_PORT","location":{"node_id":"n1","edge_index":0,"port":"zzz"}}
# exit=2
bash scripts/dev/check.sh                        # CHECK PASSED
```

### 4.3 手工构造反例（不只信测试）

| 反例 | 命令/期望 | 实际 |
|---|---|---|
| 顶层 `on_error` 出现在合法图 | 应 `DSL_PARSE_ERROR` | **exit 0**，`ok:true` |
| python_node `config.on_error` | 应拒绝（未知 config 键） | **exit 0**，`ok:true` |
| 边对象 `on_error` | 应拒绝（未知边键） | **exit 0**，`ok:true` |
| 输入端口 `on_error` | 应拒绝（未知端口键） | **exit 0**，`ok:true` |
| `from_output_port:123` | 应 `DSL_UNSUPPORTED_FIELD` | exit 2，`DSL_PARSE_ERROR` |
| 无引用的子图 `id:""` | 应 `DSL_SUBGRAPH_INVALID` | **exit 0**，`ok:true` |
| 无引用的子图 `id:7` | 应拒绝 | **exit 0**，`ok:true` |
| 缺 `from` 的边 | 应 1 条 `DSL_MISSING_FIELD` | 2 条相同错误，`error_count=2` |
| 节点未知字段 `bogus` | 应带 `node_id:"start"` | `location:{"field":"bogus"}`，无 node_id |
| `created_at:"2026-99-99T99:99:99Z"` | 应拒绝 | **exit 0**，`ok:true` |
| 合法节点级 `on_error` | 应 `DSL_UNSUPPORTED_FIELD` | exit 2，location node_id（正确） |

### 4.4 其他确认

- `symbols.rs` 无 Python `import`/`eval`；`python_node` 只做 `module/function` 格式校验；`valid_subgraph.json` 加载期通过（运行期拒绝留 M2），D6/D7 主方向落实。
- `graph_hash`：customer fixture golden `ec320e4244b365ab`；格式化/键序稳定、节点 timeout 变化、端口 description 变化均有测试；投影排除 description/author/created_at（代码检查）。
- fixtures：`every_fixture_is_referenced_and_matches_expected_code` 通过，17 码 + `bad_version` + 4 合法图无孤儿；但上述反例未进入 fixtures。
- 工作区 `git status --short` 为空；未 push/merge；仅新增本评审记录。

## 5. 遗留问题

- **阻塞 #1–#4 必须先修**；#5/#6 重要，建议本轮一起修；#7 测试需同步覆盖，不能让 `location.field` 继续掩盖缺少 `node_id/edge_index`。
- 已登记：ISSUE-026（D6/D7 gate params/冲突语义）、ISSUE-027（无 assets-dir 跳过 R14c）、ISSUE-028（fixtures 路径）、ISSUE-029（schema/--structure-only/--print-hash/语义校验）、ISSUE-030（blake3 边界）。评审认可这些作为已知范围，但 ISSUE-026/027/029 的 spec/CLI 口径应在 M2 前锁定。
- 建议在 `doc/issues.md` 增补 #1–#6（若不在本轮直接修）；本轮评审不改 issues/progress，仅记录。
- 修复后复审重跑必跑命令，并补 `doc/reviews/REV-005` 修订段。

## 6. 条件关闭记录（仅"有条件通过"填写）

不适用（结论为"不通过"）。
