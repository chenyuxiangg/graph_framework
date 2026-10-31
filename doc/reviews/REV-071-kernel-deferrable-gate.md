# REV-071 · kernel · W2a deferrable gate / attempt-per-activation / dormant（主审）

> **跨模块声明（Lead 批准，写入本记录）**：本分支主模块 = `kernel`；为 W2a0 trace v4 三字段的真实接线，机械修改 `crates/zgraph-cli/src/bridge.rs::map_gate_result`（1 文件 +5/−7，仅 `deferred`/`attempt`/`verdict` 逐字段透传，替换 trace 分支的过渡值）。cli 语义按 Lead 分工由**第二评审 reviewer-a** 追加 §7 复核；本记录为主审（kernel）记录，并已逐 hunk 确认 cli 改动仅限该函数。

- **分支**：`feat/kernel-deferrable-gate`
- **作者**：`kernel-writer`（`ee4ca27` kernel 语义 8 files +1410/−22；`6c90a16` cli bridge 1 file +5/−7；合计 9 files +1415/−29；两笔正文均含 `Review: REV-071` / `Agent: kernel-writer`）
- **评审人**：`reviewer-b`（**主审**，≠ 作者）；**第二评审：`reviewer-a`**（跨模块 cli 接线复核，按 Lead 分工在本记录后追加 §7）
- **变更范围**：`61ba5a3..6c90a16`（base = `origin/main` `61ba5a3`；9 files +1415/−29：kernel `src/{run,gate,graph,trace}.rs` + `tests/{compile,gate,defer_gate,defer_reentry}.rs`；cli `src/bridge.rs` 1 文件）
- **评审时间**：2026-10-31
- **关联**：DR-005 §3（B1–B6）/§4.1/§7 AC-14 a/b/e/f/g/§9/§11 C-1/N-9；spec 01 §1.4.1/§1.5/§1.7；spec 03 §3.4.2/§3.4.3/§3.9.3/§3.10/§3.15；REV-067（W2a0 冻结）/REV-069（dsl R15）/REV-070（trace v4）；REV-026/ISSUE-061

## 1. 结论

**结论：通过**

按 Lead 冻结契约逐项独立核对，实现与 DR-005 §3/§4.1 及 AC-14 kernel 档一致：

- **apply_publish 四类路径与顺序**：blocked 先 emit 再「defer→receipt / settle→settle+skip」；passed 先 `resolve`（defer 走 attempt-per-activation）、成功后 emit → buffer → settle → ready → skip；`RUN_DELIVERY_TO_TERMINAL`（含 defer→已 `Completed` 的 `end` 特例、defer 目标终态非 `Completed` 防御）resolve 失败返回、**不 emit**；超限在 resolve 内 create Crashed + `fail_run(NODE_INTERNAL)`，外层 emit 因 sink 已关闭为 no-op（**该代 GateResult 不落 trace**，与自环现状一致）；循环顶部终态短路。
- **attempt-per-activation 仅 defer 边**：settle 边到 `Completed` 仍 `RUN_DELIVERY_TO_TERMINAL`；自环 `attempt=源+1`（不双重递增）、非自环 `attempt=目标当前+1`；`attempt >= max_attempts`（缺省 3）→ Crashed(Internal) + `Failed/NODE_INTERNAL`，message 与 DR-005 §3.5 逐字一致（自环/非自环两种模板）；合法边界 `attempt=max-1` 放行、`attempt=max` 失败，测试锁定。
- **重入实例**：per-port 继承（入边 defer 的端口不继承，其余从上一代 buffer 深拷贝 values+arrived） + 创建时清空 `settled_by_node[target]`、不回收继承端口结算；A→B→A 测试逐步断言 4 次调用的 `(node, attempt, inputs)`，证明 defer 端口用当前代值、非 defer 端口继承，且无 `Skipped`（缺陷 A/B 均覆盖）。
- **dormant/死锁**：静默时 receipt 目标无当前实例 → 确定性 `RUN_DEADLOCK`（不等 `deadlock_grace`，测试把 grace 设 60s 仍 <2s 失败），message 含 edge 索引与 target node（`edge(s) [2]`/`[b]`）；目标 `Completed`/`Skipped` → dormant → `Completed`；receipt 放行不删；Paused 不判定；既有 watchdog 未改。
- **GateResult/verdict**：`deferred=!passed && 边为 Defer`（本次求值结果）、`attempt=发布者实例 attempt`、`verdict` 仅 `check_output`/`compare` + string path + 求值无错误时取路径原始值（`manual`/无 gate/异常/路径缺失 → `None`），路由仍只看 `passed`（I7 不变）。
- **v0.1.0 兼容**：既有测试仅 `compile.rs` 的 use 行调整 + 新增用例（`git diff` 无既有断言删改、无 `#[ignore]`）；`cargo test --workspace` **682 passed / 0 failed / 3 ignored**，e2e **46 passed / 2 ignored**，无 defer 图行为/trace 期望回退。
- **门禁**：kernel 107 passed；cli 160 passed；clippy（workspace `-D warnings`）/fmt/`check.sh` 全过；pytest 104 passed；新增 defer 套件 10 次重跑无 flaky。

发现 0 个**阻塞**、0 个**重要**；7 条**建议**（N1–N7，均为测试覆盖/轻微去重，非契约偏差）。未发现改测试、削弱断言、越界改动或与 DR-005 不符处。

## 2. 四维评审

| 维度 | 结论 | 依据（文件/行/测试） |
|---|---|---|
| 功能正确性 | ✅（N1–N5 备注） | `run.rs:1061-1116` apply_publish 四路径与 emit 位置；`run.rs:1246-1348` defer 寻址/attempt 边界/超限 message；`run.rs:1361-1419` per-port 继承 + settled 清空；`run.rs:667-710` dormant/确定性死锁；`gate.rs:110-147` verdict；`graph.rs:106,346` 编译期补齐；`trace.rs:145-150` 三字段。测试：defer_gate 6、defer_reentry 4、gate +4、compile +1，均断言具体状态/尝试号/输入/错误码/message/记录条数 |
| 代码结构合理性 | ✅ | 主模块边界清晰：`graph.rs` 只补编译字段、`trace.rs` 只增记录字段、`gate.rs` 只增 verdict 提取、`run.rs` 集中运行期语义；defer 逻辑以 `resolve_defer_target/self_loop/reentry/create_reentry_instance` 拆分（新增函数均 ≤60 行，本分支最长 `resolve_defer_target` 32 行）；超限复用既有「create_instance + fail_run」形态；未新增运行期错误码（复用 `NODE_INTERNAL`/`RUN_DELIVERY_TO_TERMINAL`/`RUN_DEADLOCK`） |
| 逻辑性 | ✅ | 决策矩阵行 1–10 逐条有对应分支；`G_all` 无关（R15 在 dsl）；determinism：evaluations 按 edge_index 排序、receipt 用 `BTreeMap`、blocked message 按边升序 + target 去重；fail_run 后靠 sink closed no-op + 循环终态短路，无重复 emit/死循环；无吞异常/竞态；Paused 早退 |
| 可读性 | ✅（N6/N7 备注） | 新增函数/字段注释解释「为什么」并引用 DR-005 §3.x/§4.1、缺陷 A/B；自环与非自环 message 模板与 spec 一致；测试命名与断言可读；N6 为 `evaluate_verdict` 重复 `resolve_path`（off-loop 内，非契约违反）、N7 为几处审计字段无读取方（`#[allow(dead_code)]` 已注明理由） |

## 3. 发现清单

> 级别口径：阻塞 = 必须修复后才能给「通过」；重要 = 必须修复或登记 ISSUE；建议 = 可后置但需修复或进台账。

| # | 级别 | 位置 | 问题 | 建议 | 状态 |
|---|---|---|---|---|---|
| N1 | 建议 | `crates/zgraph-kernel/src/run.rs:1271-1273` | 「defer 边 passed 且目标当前实例为**终态非 Completed**（如 Skipped）」的防御分支（`RUN_DELIVERY_TO_TERMINAL`）无可执行测试；当前仅靠代码审读 | 构造「目标先被 settle 级联 Skipped，随后 defer 源放行」的图补单测，锁定 code/message 且不产生 GateResult | open（建议） |
| N2 | 建议 | `crates/zgraph-kernel/tests/defer_reentry.rs`（A↔B 图 `a`/`b` 各只有 1 个 defer 端口） | DR-005 §3.2 C-1 的核心风险形状是 **SOP `P` 的两个输入端口均为 defer**（旧 `review` 不得提前就绪）；kernel 单测只覆盖「1 settle + 1 defer」，AC-14 c 的双 defer 端口由 e2e 分支覆盖。实现逻辑通用，但建议补 kernel 级两 defer 端口用例 | 加「P 型节点（review/artifact 均 defer）」单测：先投递一个 defer 端口断言仍 Pending，第二个到齐才执行，且新 attempt 两端口 inputs 均来自当前代 | open（建议） |
| N3 | 建议 | `crates/zgraph-kernel/src/run.rs:686-710` | blocked receipt 的确定性 message 只测了**单条** blocked 边（`[2]`/`[b]`）；多条 blocked 时的**边升序 + target 去重**无测试 | 加多 receipt 用例（如 2 条 blocked 边 + 1 条 dormant），断言 message 的边序号升序、target 去重且不误报 dormant 边 | open（建议） |
| N4 | 建议 | `crates/zgraph-kernel/tests/defer_gate.rs`（I7 组合未覆盖） | 「defer 边 gate 求值异常 → `passed=false` + `deferred=true` + receipt、**不 fail run**、`verdict=None`」未测（B5/I7）；现有仅测 settle 边 gate 异常（既有 gate 测试） | 加一条 defer 边 + 非法 gate params（如 `check_output{in:"x"}`）用例，断言 run 正常收尾/挂账、GateResult `error=Some`/`deferred=true`/`verdict=None` | open（建议） |
| N5 | 建议 | `crates/zgraph-kernel/src/run.rs:667-679` | 「暂停期间不判定 deferred 死锁」仅有代码早退（`status != Running`），无专门测试 | Pause + blocked receipt（不 Cancel）断言无 `RUN_DEADLOCK`，Resume 后再确定性失败（或 Cancel → Cancelled） | open（建议） |
| N6 | 建议 | `crates/zgraph-kernel/src/gate.rs:135-147` vs `:157,194` | `evaluate_verdict` 为取路径值再次调用 `resolve_path`，同一次 off-loop 求值内路径解析两次（predicate 一次 + verdict 一次）；不是契约违反（事件循环不二次求值），仅为轻微重复 | 可选：让门控求值返回已解析值供 verdict 复用；或加注释说明「重复解析可接受，保持门控纯函数简单」 | open（建议） |
| N7 | 建议 | `crates/zgraph-kernel/src/run.rs:372-389` | `DeferredReceipt.source_instance/source_attempt/updated_at` 为 spec 冻结审计字段，内核暂无读取方（`#[allow(dead_code)]`，注释已说明）；不影响判定 | 后续 trace/metrics 消费或保留注释；可登记台账避免被误删 | open（建议） |

## 4. 验证证据

### 4.1 范围、base、干净度

```bash
cd /Users/zyzs/lczyzs/code/zgraph/.worktrees/kernel-kernel-writer
git status --porcelain --branch    # ## feat/kernel-deferrable-gate...origin/main [ahead 2]；评审前 clean
git log --oneline -3               # 6c90a16 cli bridge / ee4ca27 kernel / 61ba5a3 base
git rev-parse origin/main          # 61ba5a3...（base = origin/main，分支未落后）
git show --stat HEAD~1 HEAD        # ee4ca27 8 files +1410/−22；6c90a16 1 file +5/−7
git diff 61ba5a3..6c90a16 --name-status
# M cli/src/bridge.rs；M kernel/src/{gate,graph,run,trace}.rs；
# M kernel/tests/{compile,gate}.rs；A kernel/tests/{defer_gate,defer_reentry}.rs
git diff --shortstat 61ba5a3..6c90a16   # 9 files changed, 1415 insertions(+), 29 deletions(-)
# 每笔提交均含 Review: REV-071 / Agent: kernel-writer（见 git show -s --format=%B）
```

### 4.2 门禁（worktree 私有 target）

```bash
export CARGO_HOME=/Users/zyzs/lczyzs/code/zgraph/.cargo-home
cargo test -p zgraph-kernel --offline
# 107 passed / 0 failed / 0 ignored（含新增 defer_gate 6、defer_reentry 4、gate 21、compile 9）
cargo test -p zgraph-cli --offline
# 160 passed / 0 failed（81 unit + acceptance_m2 10 + replay 13 + run 31 + trace 14 + validate 11）
cargo test -p zgraph-e2e --offline
# 46 passed / 0 failed / 2 ignored（v0.1.0/W1 e2e 不变）
cargo test --workspace --offline
# 682 passed / 0 failed / 3 ignored（与 Lead 预期计数一致；基线 0 失败）
cargo clippy --workspace --all-targets --offline -- -D warnings   # clean, exit 0
cargo fmt --all -- --check                                        # exit 0
bash scripts/dev/check.sh                                         # CHECK PASSED
/Users/zyzs/lczyzs/code/zgraph/.venv/bin/python3 -m pytest -q     # 104 passed（本分支未改 Python）
# 新增 defer 两套件连跑 10 次：每次 6+4 passed / 0 failed，无 flaky
```

### 4.3 重点逐项核对（对应 Lead 清单 1–9）

| # | 检查项 | 结论与证据 |
|---|---|---|
| 1 | apply_publish 四类路径顺序与 emit 位置 | ✅ `run.rs:1061-1116`：`defer_edge` 判定；blocked 分支先 `emit_gate_result` 再 defer→`record_deferred_receipt`/settle→settle+skip；passed 先 resolve（`resolve_defer_target`/`resolve_target`），`None` 直接 return（不 emit）；超限返回 Crashed 实例后外层 emit 为 no-op。测试：`defer_gate_false_self_loop_is_dormant_and_completes`、`settle_gate_false_still_settles_and_skips_without_deferred`、`defer_gate_true_to_completed_end_fails_delivery_to_terminal`（断言 `gate_records(edge4).is_empty()` 且无 Crashed）、两个超限用例（断言超限那次无 GateResult） |
| 2 | attempt-per-activation 仅 defer；attempt 边界；Crashed/run error/message | ✅ 仅 `defer_edge` 走 `resolve_defer_target`（settle 走 `resolve_target`，到终态仍 `RUN_DELIVERY_TO_TERMINAL`）；自环 `source_attempt+1`（`run.rs:1279-1304`）、非自环 `target_current+1`（`run.rs:1246-1276`）；`attempt >= max_attempts` → `create_crashed_reentry`（InstanceStatus::Crashed + NodeError::Internal + `fail_run(NODE_INTERNAL)`）。测试：max=2 时 loop attempts `0/1 Completed、2 Crashed`、message 含 `self-loop of node 'loop' exceeded max_attempts (2)` 与 `on edge 1`；cycle max=2 时 a `0/1 Completed、2 Crashed`、message 逐字匹配 `defer re-entry of node 'a' exceeded max_attempts (2) on edge 2 (attempt 2)` |
| 3 | per-port 继承 / settled 清空 / stale-input / 误 Skipped / A→B→A 输入断言 | ✅ `run.rs:1361-1419`：create → `settled_by_node.remove` → 仅继承「入边 `on_gate_false != Defer`」且上一代已到达的端口（values+arrived）；不回收继承端口结算。测试 `defer_cycle_rework_uses_attempt_per_activation_and_current_inputs` 逐步断言 4 次调用输入：a0 `{seed, feedback:null}`、b0 `{from_a:gen0}`、a1 `{seed(继承), feedback:gen1(当前代)}`、b1 `{from_a:gen1}`，attempts 0/1，无 Skipped；self-loop 用例断言 attempt1 `seed` 继承、`again` 重投。两 defer 输入端口形状另有 N2 建议 |
| 4 | dormant/死锁判定与确定性、message、暂停边界、watchdog | ✅ `run.rs:667-710`：`check_termination` 在 `is_quiescent` 后先 `fail_if_blocked_deferred` 再 complete；无接收方实例才 blocked；`BTreeMap` 升序 + target 去重；message `deferred receipt(s) blocked on edge(s) [2]: target node(s) [b] never instantiated`（测试断言 `[2]`、`[b]`，b 无实例，a 自环 receipt 为 dormant）；dormant 用例 run Completed；grace 60s 时确定性失败耗时 <2s（测试断言）；Paused 早退（`status != Running`）；watchdog `check_deadlock`/`check_timeouts` 未改 |
| 5 | GateResult/verdict 四类分支、attempt 来源、passed/I7 | ✅ `run.rs:1122-1141`：`deferred = !passed && edge.Defer`、`attempt = 源实例 attempt`、`verdict = evaluation.verdict`；`gate.rs:110-147`：仅 `CheckOutput`/`Compare` + `params.path: string` 且无求值错误 → `resolve_path(output,path)` 原值，`Manual`/无 gate/异常 → None；路由只看 `passed`。测试：gate +4（blocked 仍取路径值、路径缺失/中间非 object、compare、manual/非字符串 path/异常）与 run 级 `attempt/deferred/verdict` 断言；I7 的 defer 组合缺测试见 N4 |
| 6 | v0.1.0 兼容（既有期望未改/未削弱、workspace 计数、trace 顺序无漂移） | ✅ 测试侧 `git diff` 仅 compile.rs 一行 use 调整 + 新增用例；`git diff ... tests` 的删行仅 1 行（import），无既有断言删改、无新增 `#[ignore]`；workspace **682/0/3**、e2e **46/2**（与 M6/W1 基线一致）；既有 self_loop/gate_skip/delivery_to_terminal/linear/timeout 等全部通过；无 defer 图 trace 顺序不变（apply_publish 对 settle 分支结构未变，仅 helper 抽取） |
| 7 | 新测试有效性 | ✅ 新增 `defer_gate.rs` 6 用例、`defer_reentry.rs` 4 用例、`gate.rs` 4 用例、`compile.rs` 1 用例；断言均为具体状态序列/attempts/inputs/错误码/message/记录条数/`deferred` 向量，无恒真或削弱；覆盖超限、`end` 特例、blocked/dormant、自环与 A↔B 重入、按端口继承、verdict 四类；`assert_lifecycle_once` 锁定 RunStarted/RunFinished 各一次。缺口见 N1–N5 |
| 8 | 代码质量 / 范围 | ✅ 新函数均 ≤60 行（最长 `resolve_defer_target` 32 行；`check_defer_reentrancy` 不在本分支）；确定性（排序/BTreeMap/无 HashMap 顺序依赖）；中文注释解释「为什么」；范围仅 kernel + Lead 批准的 cli bridge 一处；未改 `Cargo.*`/dsl/trace/harness/doc/`CLAUDE.md`。备注：`spawn_execution` 69 行属**基线既存**（`61ba5a3` 同 69 行），非本分支引入 |
| 9 | 与 DR-005/AC-14 不符处 | ✅ 未发现。AC-14 a/b/e/f/g 的 kernel 档语义均有测试；c/h 属 e2e 分支；边界 E-2（Skipped dormant）/E-3（receipt 不删）/E-4（blocked）/N-9（统一节点级 settled）与实现一致；trace 三字段语义符合 §4.1 |

### 4.4 独立复核说明

- 未采信作者自述：所有计数/哈希/门禁命令均由评审人在 worktree 私有 target 上重跑；两条 commit 的 `--stat`、`--name-status`、`git diff` 逐 hunk 核对。
- 新增函数行数、`#[ignore]`、既有断言删改分别用 diff/grep 独立检查；`spawn_execution` 69 行与 base 版本对比确认非本分支引入。
- cli 改动为纯字段透传：`bridge.rs:587-601` 将 `record.deferred/attempt/verdict` 原样赋给 `trace::GateResultRecord`，无其它 cli 逻辑变化（第二评审 reviewer-a 追加 §7 复核）。

## 5. 遗留问题

- **N1–N7（建议，非阻塞）**：见 §3；建议由作者在后续 commit 或登记 `doc/issues.md` 收口（本次评审人只写评审记录，不改台账）。
- **AC-14 c/h（SOP rework e2e、schema v4 迁移与回放）** 属 `e2e` 分支交付，不在本分支范围；kernel 分支只保证内核语义与三字段真实来源。cli bridge 的端到端 payload 断言亦由 e2e AC-14 h 覆盖。
- **基线既存**：`run.rs::spawn_execution` 69 行 >60 属 `61ba5a3` 已有的形态（本分支未触碰），不构成本分支问题；如需收口按既有 ISSUE-081 风格处理。
- 未发现越界改动、冻结文件修改、测试削弱或未登记阻塞；分支 base = `origin/main` `61ba5a3`，worktree clean；本评审未 merge/push/切 main。
- **第二评审**：`reviewer-a` 将按 Lead 分工追加 §7（cli 跨模块接线复核）；本记录为主审结论。

## 6. 条件关闭记录（仅"有条件通过"填写）

不适用（本记录结论为「通过」，无未关闭条件；N1–N7 为非阻塞建议）。
