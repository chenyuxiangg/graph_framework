# REV-090 · kernel · W4b 动态 N / foreach 展开 / item 实例 / join

> **跨模块声明（Lead 已批准，第二评审由 Lead 追加）**：本分支主体为 kernel（`crates/zgraph-kernel/**`）；阶段 2 含 **1 处 cli 跨模块机械改动**：`crates/zgraph-cli/src/bridge.rs` 的 `map_snapshot` 真实 item 字段透传 + `foreach_expansion` → trace `ForeachExpansion` 映射 + bridge 测试（`211c2be`）。仅此一处 cli 生产代码/测试，**未改 cli 其它逻辑/退出码，未改 kernel 外其它模块**；默认（无 foreach）行为零变化。**cli 第二评审由 Lead 追加**；本记录（reviewer-b）只做 kernel 主审。

- **分支**：`feat/kernel-dynamic-fanout`
- **作者**：`kernel-dev`（`53b42ca` 阶段 1 kernel 全实现 + 15 例 foreach 测试；`211c2be` 阶段 2 cli bridge 机械映射；正文均含 `Review: REV-090` / `Agent: kernel-dev`；已 rebase 到本地 main `948d677`，含 dsl foreach + trace v6）
- **评审人**：`reviewer-b`（独立评审，≠ 作者）
- **变更范围**：`948d677..211c2be`（9 files，+2194/−93）：kernel `context/graph/lib/run/trace/types` + `tests/{compile,foreach}.rs`；cli `bridge.rs`；无其它模块
- **评审时间**：2026-11-01
- **关联**：task-35（W4b 合同）/ task-38（作者任务）、DR-007 §6/§7/§8/§12、spec 01 §1.2.5、03 §3.4.6/§3.10/§3.15

## 1. 结论

**结论：有条件通过**（P0=0；**P1=1（C1）**；P2=5；P3=4）

运行期主链路（controller buffer/state、无 controller 实例、展开校验/4 个 RUN_FOREACH_*、空列表、item 定位/attempt/defer/human、join 有序数组+last source/空列表无 GateResult、取消/终止/controller skip/deadlock、展开后再投递）与 DR-007 §6 大部分一致；测试 15 例覆盖广、既有 kernel/workspace 全绿。但发现一个**Pause/Resume 契约级缺陷**：controller 在 Paused 期间变为 `Ready` 后，Resume 不会展开，随后 `check_termination` 直接 `RUN_DEADLOCK`。已在临时测试中复现（不提交；worktree 已恢复干净）。修复 C1 + 补回归测试后复审。

## 2. 四维评审

| 维度 | 结论 | 依据 |
|---|---|---|
| 功能正确性 | ⚠️ 有条件 | 展开/校验/join/失败/取消/controller skip/deadlock/展开后投递均符合 DR-007 §6；但 C1 使「Pause 期间 controller Ready → Resume」必然 RUN_DEADLOCK，违反 DR-007 §6.4 E-8 / 03 §3.8 Resume 语义 |
| 代码结构合理性 | ✅（P2 备注） | controller/item/join 集中在 `run.rs`；`CompiledForeach` 编译期生成 region/exit/join；`NodeKey=(node,item)` 贯穿寻址；空列表/join 来源/controller 状态有独立函数；cli bridge 仅映射；无其它模块改动 |
| 逻辑性 | ⚠️ 部分（C1） | 展开校验顺序、item attempt/defer 作用域、join last source、skip/死锁规则清晰；缺 `Resume` 对 `Ready` controller 的展开；另有 source_instance_id/后置 defer 等 P2/P3 边界 |
| 可读性 | ✅（P3 备注） | 注释解释 controller/join/空列表/失败顺序；测试命名清晰；P3 为测试缺口与少量文档/边界注释 |

## 3. 发现清单

### P1 · 条件（必须修复后复审）

| # | 位置 | 问题 | 修复建议 |
|---|---|---|---|
| **C1** | `crates/zgraph-kernel/src/run.rs:795`（`resume_pending` 只扫描 Pending 实例）、`:1562`（`controller_try_expand` 在 `status != Running` 时直接 return）、`:938`（`check_termination` 见到 `active_controller` 即 `RUN_DEADLOCK`）；测试 `tests/foreach.rs:762`（`pause_freezes_item_dispatch` 只在 item 已 Running 后 pause） | **Pause 期间 controller 变为 Ready 后 Resume 不展开，run 误判 RUN_DEADLOCK**：① Pause 发生在 F 上游仍在飞（如 `prep` Running）时；② `prep` 完成后 `apply_publish` 投递到 F → `controller_apply_delivery` 把 controller 置 `Ready`；③ 因 run Paused，`controller_try_expand` 的 `status != Running` 早退，controller 停在 Ready；④ Resume 只调用 `resume_pending`（仅 Pending 实例），不展开 controller；⑤ `check_termination` 的 `instances_idle` 为 true 且 `active_controller()` 返回 Ready → 立即 `RUN_DEADLOCK`（message `foreach controller of node 'F' is Ready while run is silent`）。**已用临时测试复现**（`Pause` after `prep` Running → 等 700ms → Resume → outcome Failed/RUN_DEADLOCK；临时测试已删除，worktree 干净） | 在 `handle_control(Resume)` / `resume_pending` 于 status 已恢复 Running 后，按 `node_order` 扫描 `controllers` 中 `Ready` 的 F 并调用 `controller_try_expand`（串行 F 链需循环/重扫，直至无新 Ready）；或把 controller 展开统一放到 `dispatch_ready`/drive 循环的 Running 路径。补回归测试：Pause 在上游 Running 时，prep 完成后仍 Paused、F controller Ready、Resume 后正常展开/join/Completed；串行 case 也覆盖 |

### P2 · 建议（不阻断主审，但建议同修/补测）

| # | 位置 | 问题 | 建议 |
|---|---|---|---|
| N1 | `run.rs:1416 controller_apply_delivery`（`source_instance_id` 每次 passed 都覆盖） | **expansion `source_instance_id` 语义**：DR §8.3/N18 要求「最后一条使 controller 进入 Ready 的投递来源」；当前每次 passed 投递都覆盖，若 Ready 后（如 Pause 期间）又到一条 optional passed 投递，Resume 展开时 source 变成后一条，不是真正使 Ready 的那条 | 只在 controller 从 Collecting→Ready 的那次投递记录 source（或记录 Ready 时刻的 source 快照）；补 Pause/多次投递断言 |
| N2 | `run.rs:1822 apply_publish` 目标 F 分支（先 `emit_gate_result`，`!passed && defer_edge` 直接 `record_deferred_receipt`） | **F 已 Expanded/Skipped 后的 defer gate=false 投递**：DR-007 §6.6 写「F 已 Expanded/Skipped 后任何投递（含 defer gate=true）→ `RUN_DELIVERY_TO_TERMINAL`」；当前 gate=false+defer 会在 Expanded/Skipped 后仅记 receipt，不 fail。属边界语义歧义/偏差（gate=true 已 fail） | 明确「任何投递」是否包含 defer gate=false；若包含，在 target F 分支先检查 controller state Expanded/Skipped → fail_delivery_to_terminal，再处理 defer receipt；补测试 |
| N3 | `run.rs:1680 inject_join_value` 目标 F2 分支 | **串行 join 到另一个 F2 的 controller 时未检查 F2 state**：若 F2 已 Expanded/Skipped，join 仍静默写 values/settled 并 try_ready/try_expand（状态不符直接无操作），不像 F 普通投递那样 `RUN_DELIVERY_TO_TERMINAL`；serial foreach 的晚到 join 语义未定义 | 在 `inject_join_value` 的 F2 分支检查 `Expanded/Skipped` → fail delivery（与 F 入边一致），并补 serial foreach 晚到测试 |
| N4 | `run.rs:1004 is_quiescent` / `join_buffers` | **join_buffers 非空不参与静默判定**：若 region 出口 E 在 join 已投递后才被 defer reentry 再完成（region 内允许 defer/环），join buffer 可能再次非空但无活跃实例，run 可能静默完成而不投递第二次 join（E 重入语义未定义） | 明确 E 是否允许 join 后重入；若不允许在 R18/运行期拒绝，或把非空 join_buffers 纳入静默/报 `RUN_FOREACH_ITEM_INCOMPLETE`；补测试 |
| N5 | `graph.rs:522 wire_foreach` | **编译期防御校验不完整**：只重建 region/exit/join，未校验 region 节点类型、E→J 无 gate、F 入边来自 R 外（依赖 DSL R17/R18）；程序化构造 `LoadedGraph` 可绕过 DSL 产生不一致 `CompiledForeach`（DSL 路径安全） | 在 `wire_foreach` 增加与 R18 同口径的防御性 `CompileError`（类型/join gate/F 入边），或注释明确「仅 DSL 加载路径保证」并在 `CompiledGraph::compile` 文档声明；不影响 DSL 正常路径 |

### P3 · 台账/阅读性

| # | 位置 | 问题 | 建议 |
|---|---|---|---|
| N6 | `tests/foreach.rs` | 测试缺口：无 **Pause 期间 controller Ready** 回归（C1 根因）；无 F 本身 human（only region 内 H）、entry F、item timeout、serial join 晚到、E 重入等边界 | C1 修复时补 Pause/Resume controller 测试；其余按 W4b 后续补 |
| N7 | `run.rs:3033 derive_item_key` | 数字 canonical 用 `serde_json::Number::to_string`；`1.0`/`-0.0` 等形态是否与 DR-007「canonical JSON 文本」完全一致未测（`1`/`1.5` 已测） | 补 `1.0`、`-0.0`、指数形式断言并按 canonical JSON 口径修正（若需） |
| N8 | `trace.rs:TransitionRecord` / cli bridge `map_transition` | kernel TransitionRecord 增 item 字段，但 trace crate `TransitionRecord` 无 item 列（v6 由 instances 快照提供）；bridge 忽略 item，语义正确但未注释 | W4b-docs/08 记录 source-breaking/投影口径；bridge 注释说明 item 由 snapshot 承载 |
| N9 | `run.rs:1416` / `inject_join_value` | 展开后再投递的 defer gate=false、serial join 等边界未在测试中覆盖（见 N2/N3） | 与 N2/N3 一并补测试/明确语义 |

## 4. 验证证据

worktree `/Users/zyzs/lczyzs/code/zgraph/.worktrees/kernel-kernel-dev`，HEAD `211c2be`，base `main` `948d677`，工作区干净；`CARGO_HOME=/Users/zyzs/lczyzs/code/zgraph/.cargo-home`，未设 `CARGO_TARGET_DIR`：

```bash
cargo test -p zgraph-kernel                 # 24 targets：139 passed / 0 failed / 0 ignored
cargo test -p zgraph-kernel --features test-hooks   # 26 targets：149 passed / 0 failed
cargo test -p zgraph-cli                    # 9 targets：207 passed / 0 failed
cargo test --workspace                      # 70 targets：869 passed / 0 failed / 3 ignored
cargo clippy --workspace --all-targets --locked -- -D warnings  # clean（exit 0）
cargo fmt --all -- --check                  # exit 0
bash scripts/dev/check.sh                   # PASSED

git status --porcelain                      # 空
git merge-base HEAD main                    # 948d677（本地 main = origin/main）
git diff --name-only main...HEAD            # 9 files：kernel 8 + cli bridge 1；无其它模块
git log -1 --format=%B 53b42ca/211c2be      # 均含 Review: REV-090 / Agent: kernel-dev
```

- **计数与作者自报一致**：kernel 139/0、features 149/0、cli 207/0、workspace 869/0/3；clippy/fmt/check 全绿；本次未复现 ISSUE-091 timing flake。
- **测试强度**：`tests/foreach.rs` **15 例**（展开/join 顺序、空列表、非数组、max_items、key 重复、canonical、item 失败、E skipped、取消、Pause item dispatch、human per-item、per-item defer rework、controller skip、controller Collecting deadlock、展开后投递）；`compile.rs` positive region 映射（F/W/E/owner）。
- **C1 复现**：临时增加 Pause-after-prep-Running 回归测试（**测试已删除，不提交**；`git status` 已恢复空）→ `Pause` 后 prep 完成、Resume → outcome **Failed + RUN_DEADLOCK**，message `foreach controller of node 'F' is Ready while run is silent`。根因定位：`resume_pending` 不展开 Ready controller + `controller_try_expand` Paused 早退 + `check_termination` 立即判死锁。
- **无 foreach 行为**：既有 kernel 122 用例在本分支 139 中全绿；workspace 869/0/3；无新增 ignore/放宽既有断言。

## 5. 跨模块 cli bridge 核对

| 文件 | 改动 | 核对 |
|---|---|---|
| `crates/zgraph-cli/src/bridge.rs` | `map_snapshot` item_index/item_key 真实透传；`foreach_expansion` → trace `ForeachExpansion`（`keys` None→`{"$index":i}`、item_count u32→u64）；测试 observer/map row | 与 trace v6 类型 u32/Option<String>、u64 count、`$index` 表示一致；仅映射，无其它 cli 行为/退出码改动 |
| `crates/zgraph-cli/src/bridge.rs` tests | 构造点 item 字段、foreach_expansion 落库测试 | 机械适配；未放宽既有断言 |
| kernel 外其它 | 无 | 未改 kernel 其它模块/退出码/CLI 逻辑 |

- **第二评审由 Lead 追加**；本主审确认：除上述 bridge 映射外，diff 无 cli 其它逻辑改动；默认无 foreach 行为逐字节不变。

## 6. 遗留问题

- **C1（P1）修复后**：在 `resume_pending`/Resume 路径展开 `Ready` controller（按 node_order、支持串行链），补 Pause-during-upstream 回归；关闭后复审。**修复前不合并。**
- **P2/P3**：N1 source_instance_id、N2 defer gate=false after Expanded/Skipped、N3 serial join晚到、N4 join buffer 静默、N5 编译防御、N6 测试缺口、N7 number canonical、N8 item 投影注释；建议与 C1 同批或 W4b 补测。
- **无 P0**；kernel 主链路其余部分经测试/代码审查未发现第二 P1；cli 第二评审待 Lead。

## 7. 条件关闭记录（待关闭）

| 条件 | 状态 | 关闭证据 |
|---|---|---|
| C1 Pause 期间 controller Ready → Resume 展开 | **open** | `resume_pending`/Resume 扫描 Ready controller 并展开；补 Pause-during-upstream + 串行 case 回归；重跑 kernel/features/workspace |
