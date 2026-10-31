# REV-075 · kernel · W2b 运行期每激活扇出上限 + `NODE_FANOUT_EXCEEDED`（含 cli bridge 机械 arm）

> **跨模块声明**：本分支主体为 `kernel`，另含 Lead 批准的 **cli 跨模块机械改动**——`crates/zgraph-cli/src/bridge.rs::node_error_json` 增加 `FanoutExceeded` arm + 1 条 bridge JSON 断言（`bridge.rs:738-744`、`873-890`）。Lead 明确该部分**由 Lead 另行追加第二评审**；本记录（reviewer-b）只做主审，不对 cli 第二评审背书。评审人只新增本记录文件，未修改任何实现/测试代码。

- **分支**：`feat/kernel-max-fanout`
- **作者**：`kernel-dev`（`abc1e15` 阶段 1 + `aa6bfd4` defer 用例 + `6f3ac2a` 阶段 2 编译接线；正文均含 `Review: REV-075` / `Agent: kernel-dev`）
- **评审人**：`reviewer-b`（独立评审，**≠ 作者**；本次为主审）
- **变更范围**：`220f53c..6f3ac2a`（7 files，+605/−6）：`crates/zgraph-kernel/src/{graph,run,types}.rs`、`crates/zgraph-kernel/tests/{compile,max_fanout}.rs`、`crates/zgraph-kernel/Cargo.toml`、`crates/zgraph-cli/src/bridge.rs`
- **评审时间**：2026-10-31
- **关联**：Lead 合同任务 task-1 §B（W2b 口径冻结）、DR-004 §3 W2b / §5 D2、spec 03 运行期语义、上游 REV-074（dsl `max_fanout`/R16 已合入基座 `220f53c`）、下游 REV-076（e2e C3 消费 `test-hooks` 钩子）、W2b docs 回写（REV-077）

## 1. 结论

**结论：通过**

按 Lead 合同 §B1–B5 逐条核对全部成立；`P0=0、P1=0、P2=0`，P3 建议 4 条（均不影响正确性，见 §3）；未发现改测试、削弱断言、跳过测试或越界改动（cli 机械 arm 属 Lead 批准并已声明）。

- **B1 编译映射**：`CompiledNode.max_fanout: Option<u32>`（`graph.rs:49-58`），由 `compile_node` 从 DSL 节点原样带出（`graph.rs:252-257`）；子图节点走同一 `compile_node`（`compile_nodes` 共用），主图 subgraph 节点不冒泡。`cargo test -p zgraph-kernel` 默认构建无 `CompiledNode` 字面量破损。
- **B2 运行期语义**：`apply_publish` **入口**、应用任何边之前调用 `reject_fanout_exceeded`（`run.rs:1080-1084`）；`actual = evaluations` 中 `passed == true` 的计数（`run.rs:1150-1151`，含 defer 放行、不限是否最终投递/挂账；`gate=false` 不计）；`limit = min(declared.unwrap_or(u32::MAX), force)`（`run.rs:1152-1160`）；`actual > limit` → 不 emit 任何 GateResult、不应用任何边、为源节点 `create_instance(attempt = 源实例 attempt + 1, Crashed, FanoutExceeded{limit,actual})` → `fail_run(CODE_NODE_FANOUT_EXCEEDED, 冻结 message)` → 返回（`run.rs:1164-1181`）。message 与冻结模板逐字一致：`node '{node_id}' fan-out {actual} exceeded max_fanout ({limit}) in a single activation`。
- **B3 错误码/类型/JSON**：`CODE_NODE_FANOUT_EXCEEDED = "NODE_FANOUT_EXCEEDED"`（`types.rs:82-86`）；`NodeError::FanoutExceeded{limit,actual}`（`types.rs:465-478`），`#[error("node fan-out {actual} exceeded max_fanout ({limit})")]` 与合同示例逐字一致，`code()` 返回新码（`types.rs:490`）；cli bridge 只增 `kind:"FanoutExceeded"` + `limit` + `actual`，未改既有 `code`/`message` 与其它变体键集合（`bridge.rs:738-744`），形状合理且不破坏既有 canonical 形状。**未改 `crates/zgraph-trace/**`**（diff 无 trace 文件）。
- **默认行为不变**：`TestHooks` 整个结构体 `#[cfg(feature = "test-hooks")]`（`run.rs:69-83`），非 feature 构建 `hook_values → (false, None, None, None)`（`run.rs:2118-2131`）；`actual <= limit`（含双侧 None → `u32::MAX`）时 `reject_fanout_exceeded` 只读不写、返回 `false`，无事件/实例/终态副作用 → 默认构建 GateResult/实例/终态与 v0.1.0 一致。
- **B4 测试**：`max_fanout.rs` 6 条（`Cargo.toml:31-36` 以 `required-features=["test-hooks"]` 注册）覆盖超限（Crashed attempt=1 + error 码 + RunFinished.error.code + 无 GateResult + 目标未实例化）、边界 `actual==limit` 两例（force=2；声明=2）、无 force 默认行为、defer+force 两例（passed 的 defer 计入 / `gate=false` defer 不计且照常写 `GateResult{deferred:true}`）；`compile.rs` 2 条映射（主图 fixture + 子图、不冒泡）；`types.rs` 码/Display/载荷单测；`bridge.rs` JSON 形状测试。

## 2. 四维评审

| 维度 | 结论 | 依据（文件/行/测试） |
|---|---|---|
| 功能正确性 | ✅ | B1–B5 逐项成立（见 §1）。计数语义精确：`actual` 只计 `GateEvaluation.passed`（`run.rs:1150-1151`；`evaluate_outgoing` 每条出边恰一条求值、无 gate 边 `passed=true`，`run.rs:2076-2096`），defer 两例实测 `FanoutExceeded{limit:1,actual:2}` 证明 `gate=false` defer 不计而 passed defer 计入；超限前置校验保证“无 GateResult/不应用边”可被 sink 断言（该激活共 3 条 defer 边亦全部不落 GateResult）。`attempt = 源 attempt+1` 用 `saturating_add` 与既有自环 max_attempts 同序（先 create 再 fail_run，`resolve_self_loop` `run.rs:1273-1298` 对照）。边界 `actual==limit` 两条路径（force / 声明）均 Completed；`valid_max_fanout.json` 经 R16 边界（out-degree=2、max_fanout=2）加载通过。评审人实跑三套测试全绿（§4）。 |
| 代码结构合理性 | ✅（P3 备注） | 运行期校验内聚为一个 `reject_fanout_exceeded`（`run.rs:1135-1181`），`apply_publish` 只加入口一行判断（`run.rs:1080-1084`），职责单一；上限解析集中在 helper，未把逻辑散入 settle/寻址分支；`apply_publish` 仍是唯一“一次激活应用出边”的入口（全文件 `apply_publish` 仅 2 个调用点：内置节点 `finish_builtin` 与执行回传 `on_completed`，`run.rs:835/954`），校验位置无旁路；错误码/变体/Display 与既有 `NodeError` 体系同构，未新增依赖；trace crate 零改动，Crashed 实例 + `RunFinished.error` 复用既有证据链；测试文件独立并沿用 `tests/common` 辅助，无重复实现。 |
| 逻辑性 | ✅ | 顺序自洽：`actual` 统计 → `limit` 求解 → 比较 → create Crashed → fail_run → 返回，匹配合同冻结顺序；`create_instance` 会写 `node_current` 并把快照提前 emit（`run.rs:1486-1543`），`fail_run` 随后 `finish(Failed)`，sink 中 Crashed 快照先于 RunFinished（测试断言恰一条 RunFinished 且 error.code 正确）；`fail_run` 的 `cancel_active_instances` 只处理非终态实例，新建 Crashed 不会被二次迁移（`run.rs:1897-1919`）；`evaluations` 每次激活由 `evaluate_outgoing(graph,node,output)` 重算，不存在跨激活缓存误计；无效 edge_index 在 `filter_map` 被丢弃，不影响计数；`saturating_add`/`u32::MAX` 兜底无溢出/不 panic；非 feature 构建 `hook_values` 显式返回 `None`，无“忘关钩子”路径；无吞异常/死循环/竞态新增点。 |
| 可读性 | ✅（P3 备注） | 命名达意（`reject_fanout_exceeded` / `force_max_fanout` / `FanoutExceeded`）；注释解释“为什么”（防御性兜底 W4 动态扇出、静态层 R16 已拒绝合法图、v0.1.0 行为不变、与自环超限同序）；message/Display 以 doc comment 冻结（`types.rs:82-86`、`465-478`、`run.rs:1135-1143`）；错误安装点注释标明“不产生 GateResult、不应用任何边”；测试命名表达意图、断言带上下文消息。唯一样式小瑕疵：`apply_publish` doc comment 中两个“超限”含义不同（W2a0 attempt 超限 vs W2b 扇出超限），见 §3 N4。 |

## 3. 发现清单

> 级别口径：P0 = 阻塞（必须修复才能给“通过”）；P1 = 重要（必须修复或登记 ISSUE）；P2 = 一般；P3 = 建议（可后置，但建议进台账）。本分支 **P0=0、P1=0、P2=0**。

| # | 级别 | 位置 | 问题 | 建议 | 状态 |
|---|---|---|---|---|---|
| 1 | P3 建议 | `crates/zgraph-kernel/tests/max_fanout.rs`（对照 `src/run.rs:1152-1160`） | `min(declared, force)` 的“declared 胜出”方向无固化用例：3 个 force 用例的图均**未声明** `max_fanout`（declared=None，force 必然生效），`declared_limit_at_boundary_completes` 未设 force；`declared.min(forced)` 的两条真实分支（declared<force / declared>force）未被测试直接命中。评审读码判定实现正确（`declared.min(forced)`），仅测试覆盖缺口 | 用 `valid_max_fanout.json`（declared=2）补 2 例：`force=Some(1)` → Failed 且 Crashed 的 `FanoutExceeded{limit:1,actual:2}`；`force=Some(3)` → Completed（有效 limit=2，actual==limit 边界） | open（建议项） |
| 2 | P3 建议 | `crates/zgraph-kernel/tests/max_fanout.rs:303-350` | `actual` 不计 `gate=false` 的样本目前只覆盖 **defer** 边（`on_gate_false=defer`，仍写 GateResult/deferred receipt）；未覆盖 **settle（默认）** `gate=false` 边与 passed 边混合的计数边界。两者实现同属 `!evaluation.passed` 不计数，风险低 | 可选补 1 例：1 条默认 settle 的 gate=false + 1 条无 gate 放行，force=1（actual==limit）→ Completed，且 settle 边产生 `GateResult{passed:false,deferred:false}`、不参与计数 | open（建议项） |
| 3 | P3 建议 | `crates/zgraph-kernel/tests/max_fanout.rs:72-127`（同理 249-300） | 超限用例只断言源节点**最后一条**快照为 `Crashed(attempt=1)`，未显式断言其前一条为 `Completed(attempt=0)` 的快照顺序；defer 用例已精确断言 `error` 载荷，普通边用例只断言 `error_code` | 可选：断言源节点快照序列 `[Completed(attempt0), Crashed(attempt1)]`，锁定“激活先完成后发布超限”的 trace 顺序 | open（建议项） |
| 4 | P3 建议 | `crates/zgraph-kernel/src/run.rs:1064-1072` | `apply_publish` 既有 doc comment 的“超限在寻址内创建 Crashed 实例…外层仍照常写 GateResult”指 W2a0 的 **max_attempts 超限**；新增段落用“超限”指 W2b **扇出超限**。同一注释块两个“超限”含义，后续维护者可能误读为扇出超限也会写 GateResult | 既有段落措辞改为“attempt 超限”，或在新段落标题写明 `max_fanout`/`NODE_FANOUT_EXCEEDED` | open（建议项） |

## 4. 验证证据

评审人**亲自执行**（worktree `/Users/zyzs/lczyzs/code/zgraph/.worktrees/kernel-kernel-dev`，`export CARGO_HOME=/Users/zyzs/lczyzs/code/zgraph/.cargo-home`，未设 `CARGO_TARGET_DIR`，HEAD `6f3ac2a`，工作区干净）：

```bash
cargo test -p zgraph-kernel                          # 110 passed, 0 failed（含 doctest 2；默认构建不编译 max_fanout.rs）
cargo test -p zgraph-kernel --features test-hooks    # 120 passed, 0 failed（max_fanout 6 + test_hooks 3 + timeout_hint 4）
cargo test -p zgraph-cli                             # 161 passed, 0 failed
cargo clippy --workspace --all-targets --locked -- -D warnings   # clean, exit 0
cargo fmt --all -- --check                           # exit 0
bash scripts/dev/check.sh                            # CHECK PASSED（身份/远程/CLAUDE.md/分支名/py 语法/progress/fmt/git diff --check）

# 稳定性：max_fanout 套件连续 3 次 6/6 passed（0.00-0.01s，无 flake）
cargo test -p zgraph-kernel --features test-hooks --test max_fanout
cargo test -p zgraph-kernel --features test-hooks --test max_fanout -- --exact no_hook_keeps_v0_1_0_behavior
#   1 passed（5 filtered out）：无 max_fanout、无 hook → 5 条 GateResult 全 passed、run Completed（v0.1.0 行为）
cargo test -p zgraph-kernel --features test-hooks --test max_fanout -- --exact declared_limit_at_boundary_completes
#   1 passed：声明上限 max_fanout=2、actual==limit → Completed
```

**v0.1.0 回归**：

- 默认构建 `cargo test -p zgraph-kernel` **110/110 passed**——即既有全部内核用例在无 `max_fanout`/无 hook 路径上保持绿；`max_fanout.rs` 因 `required-features=["test-hooks"]` 默认构建不参与，符合合同（运行期反例只能由钩子构造）。
- 代码层无副作用：`reject_fanout_exceeded` 在 `actual <= limit` 时仅读取（`run.rs:1161-1163`），不 emit / 不 create / 不 fail_run；`hook_values` 非 feature 恒 `(false, None, None, None)`（`run.rs:2118-2131`），`TestHooks` 整体 `#[cfg(feature="test-hooks")]`；因此默认构建 GateResult/实例/终态与 v0.1.0 逐字节一致。
- `no_hook_keeps_v0_1_0_behavior`（feature 构建、钩子 None）与 `declared_limit_at_boundary_completes`（有声明且未超限）实测均 Completed，与 v0.1.0 行为一致。

**分支/基座核对**：

```text
merge-base HEAD main = 220f53c（本地 main，含 REV-074 dsl 合并）
git merge-base --is-ancestor origin/main HEAD  -> OK（origin/main=7b4aba1 为祖先；HEAD 领先 5 commits）
git status --porcelain -> 空
```

- 测试结果：Rust 三套 `0 failed`（110 / 120 / 161）；与 `doc/test-baseline.txt` 比对**无新增失败**（基线为 0 名额全绿）。
- 改动范围核对：仅 kernel 5 文件 + cli bridge 1 文件 + Cargo.toml 注册；**未改** `crates/zgraph-trace/**`、`crates/zgraph-dsl/**`、`tests/fixtures/**`、既有测试断言（`tests/compile.rs`、`tests/max_fanout.rs` 为纯新增）。

## 5. 遗留问题

- **spec 03 运行期语义尚未落盘**：`NODE_FANOUT_EXCEEDED`、计数定义（含 defer passed）、超限失败矩阵、Crashed attempt+1、无 GateResult、默认无上限、可达性说明由 W2b docs 分支（REV-077 / task-1 §D）负责；本分支代码注释已按冻结口径书写，不构成代码遗留。
- **cli bridge 机械 arm 的第二评审**：由 Lead 另行追加（Lead 已声明）；本记录不含 cli 第二评审结论。
- **e2e C3（REV-076）依赖钩子稳定性**：`TestHooks.force_max_fanout: Option<u32>`（`run.rs:76-80`）为 e2e 运行期防御层用例的构造入口，字段名/语义（有效上限 = `min(declared, force)`）需在后续分支保持稳定。
- **P3 建议项（§3 N1–N4）**：建议由 Lead 决定是否登记 `doc/issues.md`；本分支合同 §B 未授权改 `doc/issues.md`，评审人仅新增本记录。
- 未发现需要回滚/返工的问题；无阻塞项。

## 6. 条件关闭记录（仅“有条件通过”填写）

不适用（结论为 `通过`，无附加条件）。

## 7. 第二评审 · cli 跨模块机械改动（Lead 追加，2026-10-31）

- **评审人**：`lead`（**≠ 作者** `kernel-dev`；该跨模块改动由 Lead 批准，按 AGENTS §2「跨模块改动由 Lead 在评审记录中写明并指定第二评审人」执行）
- **评审范围**：`crates/zgraph-cli/src/bridge.rs` 的 `node_error_json` 新增 `kernel::NodeError::FanoutExceeded { limit, actual }` arm（`bridge.rs:738-744`）+ `node_error_json_covers_fanout_exceeded` 测试（`bridge.rs:873-890`）；**除此之外 cli 无其它改动**。
- **核对结论（通过）**：
  1. **形状兼容**：base 键 `code`（`error.code()` = `NODE_FANOUT_EXCEEDED`）与 `message`（`Display` = `node fan-out {actual} exceeded max_fanout ({limit})`）保持不变；变体键只增 `kind:"FanoutExceeded"`、`limit`、`actual` 三个，与既有 `NodeWorker` 变体“base + 变体键”模式一致，不破坏 06 §6.2.5.1 canonical 形状；未混入 `business_code`/`worker_kind` 等其它变体键。
  2. **改动最小**：仅一个 match arm + 一条单测；无 cli 参数/退出码/生命周期行为改动；`crates/zgraph-trace/**` 零改动。
  3. **测试有效**：测试断言 `code`/`kind`/`limit`/`actual`/`message` 五个字段，并显式断言不含它变体键（防止复制粘贴漂移）。
- **Lead 亲自验证**：
  ```bash
  cd /Users/zyzs/lczyzs/code/zgraph/.worktrees/kernel-kernel-dev
  export CARGO_HOME=/Users/zyzs/lczyzs/code/zgraph/.cargo-home
  cargo test -p zgraph-cli --offline   # 161 passed / 0 failed / 0 ignored
  git status --porcelain               # 空（仅本评审提交前）
  ```
- **结论**：**通过（第二评审）**。cli 机械改动与主审结论合并生效；分支满足合并前置。
