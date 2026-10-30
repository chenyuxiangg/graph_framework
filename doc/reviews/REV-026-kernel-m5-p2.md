# REV-026 · kernel · M5 P2：ISSUE-061 gate 幻影 attempt + ISSUE-076 timeout_hint

> **模块声明**：本分支只改 `crates/zgraph-kernel/**`（6 个文件：`src/executor.rs`、`src/graph.rs`、`src/run.rs`、`tests/compile.rs`、`tests/self_loop.rs`、新增 `tests/timeout_hint.rs`），符合 AGENTS §2 `kernel` 模块边界；**无** `Cargo.toml`/`Cargo.lock`/跨模块/文档改动。作者 `kernel-dev`，评审人 `reviewer-a`（**非作者**）。评审**未修改分支任何文件**：为独立复现 ISSUE-061，仅在评审期间把 `crates/zgraph-kernel/src/run.rs` 临时回退到 `a04c29d` 版本跑反例，随后 `git checkout HEAD --` 恢复；评审结束时 HEAD=`a99e726`、worktree clean，与评审前一致。
>
> **语义收敛判定（Lead 指定）**：作者方案「**放行分支保持先寻址再 GateResult，仅拦截分支不寻址**」**判定为可接受**（附 1 条 M5 docs 回写项，见 §3 #1）：① 放行分支顺序与既有实现一致，`RUN_DELIVERY_TO_TERMINAL` 语义与 `tests/delivery_to_terminal.rs` 期望不变；② 拦截分支改为 GateResult → `settle_edge` → `maybe_skip_after_settle`，与 spec 01 §1.4（`BlockedDelivery.to_instance: Option`、blocked → 结算入边 + 级联 Skipped）及 DR-002 §4 一致；③ `InstanceSnapshot` 不产生 `events` 行（`trace::TraceInput` 契约），故 **trace 可见事件顺序无变化**，只改变「被拦截投递是否寻址/是否因终态 fail」；④ 代价是 spec 03 §3.4.2 伪代码（所有边先 `resolve_target_instance` 再发 GateResult）与代码漂移，属 M5 docs 回写范围（§3 #1）。

- **分支**：`fix/kernel-m5-p2`
- **作者**：`kernel-dev`
- **评审人**：`reviewer-a`（**非作者**）
- **变更范围**：`2b255de..a99e726`（2 commit：`a04c29d fix(kernel): 实例超时解析接入 NodeExecutor::timeout_hint（ISSUE-076）`、`a99e726 fix(kernel): gate 拦截的自环投递不再创建幻影 attempt（ISSUE-061）`；`git diff --shortstat` = **6 files changed, 471 insertions(+), 30 deletions(-)**；base = merge-base = `2b255de`；评审时本地 `main` = `5d2e53d`（REV-025 文档合并，晚于 rebase base，**合入前需 rebase**，见 §5）
- **评审时间**：2026-10-30
- **关联**：ISSUE-061/076；DR-002 §4（ISSUE-061/076 口径）、DR-002 §9 #6（M5 回写）；spec 05 §5.2.6（实例超时优先级）、spec 03 §3.2/§3.4.1/§3.4.2、spec 01 §1.2.2/§1.4；REV-015（ISSUE-061 原始反例）、REV-020（ISSUE-076 原始登记）、REV-025（DR-002 评审）

## 1. 结论

**结论：通过**

两个 P2 修复均**正确、可独立复现、无回归**，且全量测试不缩水：

- **ISSUE-061（gate 幻影 attempt）已关闭**：`apply_publish` 拦截分支改为「GateResult → settle → 级联 Skipped」，不再 `resolve_target`。**独立复现**：仅把 `run.rs` 回退到 `a04c29d`（未含本修复）后 `cargo test -p zgraph-kernel --test self_loop` → **2 FAILED**（`left: [(0,Completed),(1,Skipped)]` 幻影实例；`max_attempts=2` 误报 `NODE_INTERNAL`），恢复修复后 3 passed；新增/既有自环测试全部通过，e2e 反例 `tests/e2e/parallel.rs` **未被改动/弱化**（按 DR-002 归 e2e-dev wave2 补断言）。
- **ISSUE-076（内核侧）已打通**：`CompiledNode.declared_timeout_ms: Option<u64>` 保留「未声明」语义（`timeout_ms` 历史语义不变）；`NodeExecutor::timeout_hint(&CompiledNode) -> Option<u64>` 为**对象安全的默认方法**（默认 `None`，对既有执行器纯增量，`Arc<dyn NodeExecutor>` 编译通过）；`start_dispatch` 解析顺序 **test-hooks force > DSL declared > executor hint > `DEFAULT_NODE_TIMEOUT_MS`**，与 spec 05 §5.2.6 及 DR-002 §4 完全一致，并有 4 条定向单测（含 test-hooks 最高优先级）。
- **全量不缩水**：`cargo test --workspace` = **431 passed / 0 failed / 2 ignored**（2 ignored 为既有 harness 真模型 smoke + e2e AC-08b 手动，与基线一致；相对 main 净增 7 条测试、无删除/弱化）；`cargo test -p zgraph-kernel` = **86 passed**；`--features test-hooks` = **90 passed**；`cargo clippy --workspace --all-targets -- -D warnings`、`cargo fmt --all -- --check`、`check.sh` 全 PASS。
- 未发现阻塞/重要项；3 条建议 + 1 条 Nit + 2 条备注见 §3（文档回写/覆盖率/合并前置），均不阻塞本分支验收。

## 2. 四维评审

| 维度 | 结论 | 依据（文件/行/测试） |
|---|---|---|
| 功能正确性 | ✅ | ① ISSUE-061 反例独立复现（§4.2）：未修版 2 FAILED、修复后转绿，且 `max_attempts=2/3` 两种边界均覆盖；拦截投递仍记录 1 条 GateResult（`tests/self_loop.rs:151-232` 断言 `[false]` / `[true,false]`）——门控可观测性未被修复吞掉。② ISSUE-076 优先级链实现与 spec 05 §5.2.6 逐项一致（`run.rs:668-682`），4 条单测覆盖 hint 生效/DSL 优先/无 hint 默认/force 最高（`tests/timeout_hint.rs:86-200`），`compile.rs:133-155` 覆盖 declared=None/Some 两种编译形态。③ 既有 `self_loop_beyond_max_attempts_fails_run`、`delivery_to_terminal_instance_fails_run` 断言未改且通过。 |
| 代码结构合理性 | ✅ | 改动全部收敛在 kernel crate：编译产物（`graph.rs`）、执行器契约（`executor.rs`）、派发/投递（`run.rs`）+ 对应测试；无跨模块/依赖/CI 改动。`NodeExecutor` 只加**默认方法**，不破坏既有实现（`FakeExecutor`、`WorkerExecutor`、`HarnessExecutor`、测试执行器）与对象安全；`emit_gate_result` 从 `apply_publish` 抽出（只依赖 edge/from/求值结果），消除重复；`DEFAULT_NODE_TIMEOUT_MS` 统一来自 `zgraph_dsl` 单源；`CompiledHandlerSpec` 类型别名让 4 元组返回可读。 |
| 逻辑性 | ✅ | ① 优先级链短路语义正确：`force.or(declared).or_else(hint).unwrap_or(DEFAULT)`——force 时不调用 hint，declared 时不调用 hint；`timeout_ms` 保留 `declared.unwrap_or(DEFAULT)` 兼容既有读取方，调度侧以 `declared_timeout_ms` 为准避免默认值掩盖 hint（`graph.rs:37-48`）。② 拦截分支顺序 GateResult → settle → `maybe_skip_after_settle` 与 01 §1.4 / 03 §3.4.2 blocked 语义一致；级联仍经 `cascade_skip → ensure_instance` 为**非自环**目标惰性创建并转 Skipped（AC-02 语义保持），自环目标因当前实例已终态而不产生幻影（`run.rs:997-1005`、`1213+`）。③ 放行分支寻址失败仍 `return`（不产生 GateResult、fail run），与既有 `RUN_DELIVERY_TO_TERMINAL` 语义/测试一致。④ 无吞异常/死循环/竞态：只改事件循环内同步逻辑，trace 事件数不变。 |
| 可读性 | ✅ | 注释解释「为什么」而非「是什么」（如为什么不能读 `node.timeout_ms`、为什么拦截分支不寻址、为什么 blocked 的 `to_instance` 可为空）；测试用 `ScriptedExecutor`（按调用顺序出 output）与 `HintExecutor`（记录 execute 入口 deadline 余量）表达意图清晰，无需真等 180s；timeout 测试用 500ms 睡眠/80ms hint 的宽松余量，避免脆断言。Nit：commit body 对「先寻址再 GateResult」的规范出处写成 01 §1.4（实际在 03 §3.4.2 伪码），见 #4。 |

## 3. 发现清单

| # | 级别 | 位置 | 问题 | 建议 | 状态 |
|---|---|---|---|---|---|
| 1 | 建议 | `crates/zgraph-kernel/src/run.rs:997-1005` vs `doc/spec-v1.2/03-调度器与生命周期.md:152`（§3.4.2 伪码「先 resolve_target_instance 再 GateResult」）、`doc/spec-v1.2/01-领域模型与状态机.md:87`（§1.2.2「投递到已终态 → RUN_DELIVERY_TO_TERMINAL」未区分 passed/blocked） | 本分支对**拦截边**改为不寻址、且不再因「目标终态」fail；方向由 ISSUE-061 与 DR-002 §4 授权、与 01 §1.4 BlockedDelivery 语义一致，但 spec 03 §3.4.2 伪码与 01 §1.2.2 表述尚未同步；DR-002 §9 #6 的 M5 回写清单未列 03/01 | M5 docs 回写时更新 03 §3.4.2 伪码（blocked 分支不寻址）与 01 §1.2.2/§1.4（注明 blocked 投递不寻址、不因目标终态 fail）；同时把 03/01 补进 DR-002 §9 #6 回写清单 | open（M5 docs） |
| 2 | 建议 | `crates/zgraph-kernel/src/run.rs:997`（拦截分支）；`crates/zgraph-kernel/tests/delivery_to_terminal.rs` | 新增语义「blocked 投递到终态实例不再 fail run」**无定向单测**：现有 `delivery_to_terminal` 只覆盖 passed 边；该分支行为已改变，未来回归无护栏 | 补 1 条 kernel 单测：`a → b -[gate false]-> a`（目标 a 已 Completed）→ run Completed、GateResult passed=false、a 终态不改；或明确由 e2e wave2 覆盖并登记 | open |
| 3 | 建议（跟进） | `crates/zgraph-kernel/src/executor.rs:63`；cli 分支 `crates/zgraph-cli/src/run_cmd.rs:410,429`（`RoutingExecutor`/`RecordingExecutor` 未 override `timeout_hint`）；e2e `RoutingExecutor` 同理 | ISSUE-076 **端到端未闭环**：kernel 已支持 hint，但 CLI/e2e 的组合执行器若沿用默认 `timeout_hint() -> None`，kernel 永远收不到 Harness 的 `AgentConfig.node_timeout_ms`，ISSUE-076 实际仍不生效；DR-002 §4 未写「组合执行器须转发 hint」 | REV-028（cli）/REV-033（harness）在 `RoutingExecutor`/`RecordingExecutor` 转发 `timeout_hint` 到具体 agent 执行器，并补端到端用例（DSL 未声明 timeout + AgentConfig.node_timeout_ms 生效）；DR-002 §4 补一句转发要求 | open（REV-028/033） |
| 4 | Nit | `a99e726` commit body | 正文写「passed 分支保持 01 §1.4「先寻址再写 GateResult」顺序」——该顺序实际由 spec 03 §3.4.2 伪码规定，01 §1.4 未规定顺序 | docs 回写时统一引用 03 §3.4.2；提交正文不必回改 | open（M5 docs） |
| 5 | Nit | `crates/zgraph-kernel/src/executor.rs:51-66`（trait 文档） | kernel 对**每个派发节点**（start/end/python_node/agent_call/subgraph 中除 subgraph 外的 builtin 与执行器节点）都会调用 `timeout_hint`；trait 文档只写「仅 Harness 实现返回 Some：agent_call 节点」，未明确实现必须对所有节点类型安全返回 `None`（防止 REV-033 的 Harness 实现对非 Agent handler `unwrap`/panic） | trait 文档补一句「实现须对所有节点类型安全返回 None；仅 agent_call 可返回 Some」，并在 REV-033 测试覆盖 start/end 经过 hint 的路径 | open |
| 6 | 备注（合并前置） | 分支 base `2b255de` vs 本地 `main` `5d2e53d` | 评审时本地 main 已含 REV-025 文档合并（kernel 分支未含该 4 个 docs commit）→ **`git merge --ff-only` 会非快进**；`merge.sh` 的 rebase 条件基于 `origin/main`（仍为 `1cc9123`，是分支祖先）不会自动 rebase | 合入前由 Lead/kernel-dev 将分支 rebase 到当前本地 main（`5d2e53d`，仅 docs 无冲突），再跑 `merge.sh`；dry-run 不校验 ff（§4.5 已通过） | open（合并前置） |

> 无阻塞项、无重要项；#1–#5 为文档/测试/端到端跟进，#6 为合并流程前置。

## 4. 验证证据

### 4.1 范围、分支与提交元数据（worktree `.worktrees/kernel-kernel-dev`）

```bash
cd /Users/zyzs/lczyzs/code/zgraph/.worktrees/kernel-kernel-dev
git status --short                         # 空（评审前/后均 clean）
git branch --show-current                  # fix/kernel-m5-p2
git merge-base main HEAD                   # 2b255de2e20af2ffa98b6e85579589c11c764022
git rev-parse HEAD main                    # a99e726... / 5d2e53d...（main 含 REV-025 文档合并）
git log --oneline main..HEAD               # a99e726 / a04c29d
git diff --shortstat main...HEAD           # 6 files changed, 471 insertions(+), 30 deletions(-)
git diff --name-status main...HEAD
# M crates/zgraph-kernel/src/{executor,graph,run}.rs
# M crates/zgraph-kernel/tests/{compile,self_loop}.rs
# A crates/zgraph-kernel/tests/timeout_hint.rs
git log -2 --format='%H|%an <%ae>|%s'      # 均为 chenyuxiangg <1007423165@qq.com>
# 两 commit 正文均含 Review: REV-026 / Agent: kernel-dev；粒度 = 一 commit 一个 issue
```

- 无 `Cargo.toml`/`Cargo.lock`/其他模块/文档改动；提交作者/committer 合规。

### 4.2 ISSUE-061 独立复现（不采信作者日志）

```bash
# 1) 只把 run.rs 回退到"仅 ISSUE-076"的 a04c29d（保留 a99e726 的新测试）
git checkout a04c29d -- crates/zgraph-kernel/src/run.rs
CARGO_HOME=/Users/zyzs/lczyzs/code/zgraph/.cargo-home CARGO_NET_OFFLINE=true \
CARGO_TARGET_DIR=/tmp/zgraph-merge-target CARGO_INCREMENTAL=0 \
cargo test -p zgraph-kernel --test self_loop -- --test-threads=2
# running 3 tests
# test blocked_self_loop_delivery_exactly_at_max_attempts_still_completes ... FAILED
# test blocked_self_loop_delivery_creates_no_phantom_instance ... FAILED
# test self_loop_beyond_max_attempts_fails_run ... ok
# left: [(0, Completed), (1, Skipped)]  right: [(0, Completed)]
# left: Failed  right: Completed（NODE_INTERNAL: self-loop ... exceeded max_attempts (2)）
# → 未修版 2 FAILED（/tmp/rev026-prefix.log）
# 2) 恢复
git checkout HEAD -- crates/zgraph-kernel/src/run.rs   # HEAD 仍为 a99e726，git status 空
```

修复后同测试：

```bash
cargo test -p zgraph-kernel --test self_loop -- --test-threads=2
# test result: ok. 3 passed; 0 failed; 0 ignored
# - blocked_self_loop_delivery_creates_no_phantom_instance: 恒 false gate、max_attempts=2/3 → run Completed、仅 attempt0、GateResult=[false]
# - blocked_self_loop_delivery_exactly_at_max_attempts_still_completes: 先放行后拦截、max_attempts=2/3 → Completed、attempt0/1、GateResult=[true,false]
# - 既有 self_loop_beyond_max_attempts_fails_run（passed 自环超限 → Crashed/NODE_INTERNAL）保持 ok
```

- **不弱化证据**：`tests/e2e/parallel.rs`（REV-015 固化的反例）不在 diff；`delivery_to_terminal.rs` 不在 diff 且通过；`self_loop.rs` 原测试函数逐字节未改（diff 仅新增 import/辅助/新测试）。

### 4.3 ISSUE-076 优先级与对象安全

| 核对项 | 实现/测试 | 结果 |
|---|---|---|
| `declared_timeout_ms` 区分「未声明 vs 显式默认值」 | `graph.rs:37-48,231-291`；`tests/compile.rs:122-155` | ✅ 未声明=None；显式 1234/60000 保留原值；`timeout_ms` 历史语义 `declared.unwrap_or(DEFAULT)` 不变 |
| 对象安全默认方法 | `executor.rs:51-66`；`Arc<dyn NodeExecutor>` 全仓使用（clippy/build 通过） | ✅ 无泛型参数、默认 `None`；既有执行器零改动 |
| 优先级链 | `run.rs:668-682`：`force.or(declared).or_else(hint).unwrap_or(DEFAULT)` | ✅ 与 spec 05 §5.2.6 / DR-002 §4 逐字一致；force 短路 |
| hint 生效 | `tests/timeout_hint.rs:86-118`：hint=80ms + executor sleep 500ms → run Failed/NODE_TIMEOUT、实例 TimedOut、NodeTimedOut 恰一次、execute 入口余量 ≤80ms | ✅ 真超时（非仅读字段） |
| DSL declared 优先 | `:120-142`：declared 300ms + hint 10ms → 观察余量 150–300ms | ✅ 10ms hint 未生效 |
| 无 hint 用默认 | `:144-167`：余量 ≈180000ms（不真等）；`FakeExecutor::timeout_hint == None` | ✅ 既有执行器路径不变 |
| test-hooks force 最高 | `:169-200`（`cfg(feature="test-hooks")`）：force 50ms + declared 300ms + hint 10ms → 50ms 生效 | ✅ |

### 4.4 全量与门禁（隔离 target `/tmp/zgraph-merge-target`，`CARGO_NET_OFFLINE=true`，先 `cargo clean -p` 全部 workspace 成员避免陈旧 `CARGO_MANIFEST_DIR` 二进制）

| 命令 | 结果 |
|---|---|
| `cargo test -p zgraph-kernel -- --test-threads=2` | **86 passed / 0 failed / 0 ignored** |
| `cargo test -p zgraph-kernel --features test-hooks -- --test-threads=2` | **90 passed / 0 failed**（含 `force_timeout_beats_declared_and_hint`） |
| `cargo test --workspace -- --test-threads=2` | **431 passed / 0 failed / 2 ignored**（忽略项 = harness 真模型 smoke + e2e AC-08b 手动，与基线一致；相对 main 净增 7 条、无删减；`/tmp/rev026-workspace.log`） |
| `cargo clippy --workspace --all-targets -- -D warnings` | exit 0（与 CI 同口径） |
| `cargo fmt --all -- --check` | PASS |
| `bash scripts/dev/check.sh` | CHECK PASSED（身份/冻结文件/禁用文件/progress ≤30/fmt/diff-check） |

> 环境说明（如实记录）：本机内存紧张（vm_stat free 页一度 <4k），首次 `cargo test -p zgraph-kernel --lib` 被 OS SIGKILL；以 `--test-threads=2` 重跑通过，最终结果不受影响。未改测试/未跳过任何测试。

### 4.5 merge.sh dry-run（隔离 target，最终结论前）

```bash
cd /Users/zyzs/lczyzs/code/zgraph
CARGO_HOME=/Users/zyzs/lczyzs/code/zgraph/.cargo-home CARGO_NET_OFFLINE=true \
CARGO_TARGET_DIR=/tmp/zgraph-merge-target CARGO_INCREMENTAL=0 \
PYTHON_BIN=/Users/zyzs/lczyzs/code/zgraph/.venv/bin/python \
bash scripts/dev/merge.sh fix/kernel-m5-p2 --dry-run
# 结果见下方（评审时实跑）
```

```text
（已由评审人补记实跑结果，见文末「dry-run 实际结果补记」）
```

> 注意：dry-run 只到测试门禁即返回 0，不校验 `--ff-only`；本分支 base 落后本地 main（§3 #6），**合入前必须先 rebase**。

### 4.6 语义收敛判定（Lead 指定）——结论：可接受

- **放行分支**：`resolve_target` → GateResult → buffer/settle/ready/skip，与 main 顺序一致；寻址失败仍不产生 GateResult 并 fail run（`delivery_to_terminal` 测试保持绿）。
- **拦截分支**：GateResult → settle → `maybe_skip_after_settle`；不寻址 → 不自环 attempt+1、不因目标终态 fail。级联 Skipped 仍由 `cascade_skip → ensure_instance` 保证（非自环目标实例照常创建并转 Skipped），AC-02 语义未变。
- **事件顺序**：实例快照走 `TraceInput::InstanceSnapshot`（只 upsert、不产生 `events` 行），因此 trace 的 `events.seq` 顺序不受影响；`GateResult` 仍在边界处、`settle_edge` 之前记录。
- **结论**：可接受；唯一代价是 spec 03 §3.4.2 伪码/01 §1.2.2 表述待 M5 回写（§3 #1），由 docs 分支随 ISSUE-061 口径统一处理，不阻塞本分支。

## 5. 遗留问题

- **合并前置（必须）**：分支 base = `2b255de`，本地 `main` = `5d2e53d`（REV-025 文档合并）→ 合入前 rebase 到 `5d2e53d`（仅 docs，预计无冲突）；`merge.sh` 的 rebase 依据 `origin/main`（`1cc9123`）不会自动触发，需人工 rebase 后跑 `merge.sh fix/kernel-m5-p2`。
- **M5 docs 回写（REV-035）**：#1 更新 03 §3.4.2/01 §1.2.2/§1.4 并补进 DR-002 §9 #6；#4 统一规范出处引用。
- **端到端跟进（REV-028/REV-033）**：#3 CLI/e2e 组合执行器转发 `timeout_hint`；#5 trait 文档补「对所有节点类型安全返回 None」，REV-033 覆盖 start/end 路径。
- **测试补强**：#2 blocked→终态目标不 fail 的定向单测（kernel 或 e2e wave2）。
- **合并后**：Lead 更新 `doc/reviews/README.md`/`doc/index.md` 的 REV-026 索引与 `doc/progress.md`。
- 本评审未修改被评审分支任何文件；REV-026 记录为本分支唯一新增文件（由评审人提交）。

## 6. 评审签署

- 两 commit 粒度清晰、模块边界正确；ISSUE-061 反例经**独立回退复现**（2 FAILED→修复后绿），ISSUE-076 优先级链与对象安全经实现+定向单测+全量验证；全量 431 passed/0 failed/2 ignored（无新增失败、无测试删减），clippy/fmt/check.sh PASS。
- 未发现阻塞/重要项；建议/跟进项已列 §3/§5，不阻塞本分支。
- **结论：通过**（合入前需按 §5 先 rebase 到当前 main 并复跑 `merge.sh` 全门禁）。
- 评审人：`reviewer-a`（非作者）；评审时间：2026-10-30。

## 8. dry-run 实际结果补记（评审人 `reviewer-a`，2026-10-30）

> 本记录 §4.5 的 dry-run 输出块未及回填（分支随后已合并至 main `ad7fbec`）；评审人补记实测证据如下（原记录不删除）。

- 结论：`通过`（REV-026 两 commit `9ddc539`/`f89ddaa` 已合并）。
- `cargo test --workspace`：**431 passed / 0 failed / 2 ignored**（含 e2e AC-12 合并后基线；2 ignored 为既有手动项）；`cargo test -p zgraph-kernel` **86 passed**；`cargo test -p zgraph-kernel --features test-hooks` **90 passed**。
- ISSUE-061 独立复现：仅把 `run.rs` 回退到 `a04c29d` 后 `cargo test -p zgraph-kernel --test self_loop` → **2 FAILED**（`left: [(0,Completed),(1,Skipped)]` 幻影实例；`max_attempts=2` 误报 `NODE_INTERNAL`）；恢复修复后 **3 passed**。
- `cargo clippy --workspace --all-targets -- -D warnings`、`cargo fmt --all -- --check`、`scripts/dev/check.sh` 均 PASS。
- 合并后遗留（原记录 §5 已列）：DR-002 §9 #6 增加 03 §3.4.2 / 01 §1.2.2 回写项；CLI/e2e 组合执行器转发 `timeout_hint` 留 wave2。
