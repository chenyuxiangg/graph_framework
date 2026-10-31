# REV-080 · kernel · W3a0 人工等待生命周期 + `builtin.human` + `HumanDecision` 控制通道

> **跨模块声明**：本分支主体为 `kernel`，阶段 2 含 **1 处 cli 跨模块机械改动**：`crates/zgraph-cli/src/bridge.rs::map_instance_status` 增 `WaitingHuman` arm + `map_instance_status_covers_all_kernel_statuses` 防遗漏测试（`72036b6`，55 行）。该改动为 Lead 批准的机械映射（task-14），**cli arm 第二评审由 Lead 追加**；本记录（reviewer-b）只做主审，不对 cli arm 第二评审背书。评审人只新增本记录，未改实现/测试。

- **分支**：`feat/kernel-human-wait`
- **作者**：`kernel-dev`（`ab4b805` 阶段 1 kernel 实现+测试；`72036b6` 阶段 2 cli bridge 机械 arm+映射测试；正文均含 `Review: REV-080` / `Agent: kernel-dev`）
- **评审人**：`reviewer-b`（独立评审，**≠ 作者**；本次为主审）
- **变更范围**：`adf54a1..72036b6`（9 files，+1286/−38）：`crates/zgraph-kernel/src/{graph,lib,run,trace,types}.rs`、`crates/zgraph-kernel/tests/{compile,human_wait,state_machine}.rs`、`crates/zgraph-cli/src/bridge.rs`
- **评审时间**：2026-10-31
- **关联**：task-14（W3a 合同）/ task-16（作者任务）、DR-006 §2/§3/§4/§7.1/§8（唯一语义依据）、spec 00/01/03/04 的 W3a0 冻结段、REV-078/079（前置设计冻结与 trace v5）

## 1. 结论

**结论：通过**

P0=0、**P1=0**、P2=0、P3=8（均为建议/后续记录，无阻塞项）。实现与 DR-006 冻结语义逐条一致，测试强度足以覆盖本任务 DoD；未发现改测试、削弱断言、伪造结果或越界改动（cli arm 属 Lead 批准并已声明）。P3 建议见 §3，均不构成合并条件。

**合同/DR-006 重点逐项核对**：

- **① `WaitingHuman` 语义/迁移**：`InstanceStatus::WaitingHuman` 非终态（`is_terminal=false`）、`is_active()` 覆盖 `Pending/Ready/Running/WaitingHuman`（`types.rs:177-193`，单测）；迁移矩阵 9 状态 13 合法边（新增 `Pending→WaitingHuman`、`WaitingHuman→Completed/TimedOut/Cancelled`），非法边（WaitingHuman→Ready/Running/Crashed/Skipped 等）显式禁止（`types.rs:180-230`、`tests/state_machine.rs` 9×9 全组合）；`transition_raw` 唯一入口未破坏（迁移失败仍 `RUN_ILLEGAL_TRANSITION`）。
- **② `builtin.human` 精确编译映射**：`compile_python_handler` 仅在 `module=="builtin" && function=="human"` 精确匹配 → `CompiledHandler::Human`；其它 `builtin.*` 仍是 `Python` handler，不 import/不启 worker（`graph.rs:313-340`，`tests/compile.rs:compiles_builtin_human_handler` 双反向断言）；`node_type` 仍 `python_node`，无新 NodeType、DSL/R1–R16/hash 不变。
- **③ `Pending→WaitingHuman` 旁路**：`maybe_make_ready` 命中 Human handler 即走 `enter_waiting_human`（不 Ready/Running、不入队、不派发）；迁移前写 `instances.input` = 全部声明端口（未到 null，`build_inputs` 同普通 dispatch 快照语义）、仅 `declared_timeout_ms` Some 才登记 deadline（缺省**无期限**）、`transition_raw` 记 `started_at`/`started_instant`、reason=`human_waiting`（`run.rs:1726-1790`；`human_wait.rs:human_node_waits...` 断言单条 Pending→WaitingHuman 迁移、input 快照、执行器只见过 prep、无 Running）。
- **④ 决策应用顺序冻结**：校验 `instance_missing → not_current → not_waiting_human` → ① 写 `output={"decision","reason"}` ② 清 deadline ③ `transition(Completed, reason="human_decided")`（快照携带 output）④ `evaluate_outgoing` + `apply_publish`（与 `finish_builtin` 同构，W2a/W2b 语义复用）⑤ `human_decided{applied:true}`（`run.rs:713-790`）；不通过 → 状态不变、只发 `human_decided{applied:false, reject_reason}`、**不 fail run**（`run.rs:792-810`）。`human_wait.rs` 覆盖 approved（output/transition/GateResult/end/record 字段/run_output）与 rejected（正常 Completed、端侧 Skipped、理由 null）；三值 reject_reason 各有独立用例（含 `not_current` 优先级：旧 attempt 已完成+新实例当前 → not_current）。
- **⑤ 期限超时/取消/冷峻/Pause**：`check_timeouts` 覆盖 `Running|WaitingHuman`（用 `started_instant` 从等待起点计 elapsed），到期 `NodeTimedOut` + `NodeError::Timeout` + `TimedOut` + `fail_run(NODE_TIMEOUT)`，human 无 worker 可 kill → `killed_workers` 不增；`cancel_active_instances` 按 `!is_terminal` 覆盖 `WaitingHuman→Cancelled`（用户 Cancel/冷峻级联）；`watchdog` 在 Paused 也扫描 deadline（实测 60ms 期限 Paused 下仍 TimedOut）；`is_quiescent` 用 `is_active` 含 WaitingHuman、`check_deadlock` 显式排除 `has_waiting`（`run.rs:1855-1990`；测试：纯等待 120ms>grace 30 仍 Running、Pause 决策不派发、Pause 期限照计、冷峻级联 Cancelled）。
- **⑥ 控制通道/API/默认兼容**：`RunControl::HumanDecision{instance_id,decision,approver,reason,decision_id,decided_at}`（`Copy→Clone`，`run.rs:160-186`）；`HumanDecision` 枚举 `approved/rejected`；`HumanDecisionRecord` 10 字段；`TraceSink::human_decided` 默认 no-op、`Emitter` 经同一 `closed` 串行化（`human_decided` 在 `RunFinished` 前，终态后丢弃），`MemoryTraceSink` 记录 + `is_empty`/`call_log`/访问器；`human_decided` 在事件循环内、不写 `events.kind`、不落库（DR E-14）。`TraceCall`/`CompiledHandler`/`InstanceStatus` 的公共 enum 变更属 source-breaking，按合同由 W3a-docs 在 08 §8.7 回写（P3 N6 记录）。
- **默认/v0.1.0 兼容**：无 human 节点的图路径未变（`enter_waiting_human` 仅在 `is_human_node` 分支调用；`is_quiescent` 对非 human 状态等价；`RunControl` 仅去 `Copy` 不改变行为）；既有 kernel 110 用例全绿于 122，`state_machine` 仅按矩阵扩展、未削弱既有断言；workspace 754/0/3（3 ignored 为既有 manual）。

## 2. 四维评审

| 维度 | 结论 | 依据（文件/行/测试） |
|---|---|---|
| 功能正确性 | ✅ | ①–⑥ 全部落实；`human_wait.rs` 9 用例 + `compile.rs` 映射 + `state_machine` 9×9 矩阵；真实计时用例（期限/Pause/冷峻）；v0.1.0 既有 110 用例全绿，无新增 ignore |
| 代码结构合理性 | ✅ | 编译期 Human handler 单点（`graph.rs`）/ `is_human_node` 查询（`run.rs`）；等待/决策逻辑集中在 `enter_waiting_human`/`apply_human_decision`/`human_decision_reject`，未散入 gate/寻址；`TraceSink` 新方法默认 no-op、Emitter 串行化与既有 sink 模式一致；cli 仅 1 行映射 + 防遗漏测试，无 cli 其它行为改动 |
| 逻辑性 | ✅ | 顺序冻结（output→清 deadline→transition 快照→publish→确认）与 as-built `transition_raw` 快照时机自洽；校验顺序与 DR §4.1/§4.4 一致；`is_quiescent`/`check_deadlock`/`check_timeouts`/`cancel_active_instances` 对 9 状态闭合；Paused/Deadline/取消交叉用例通过；无吞异常/新增 panic/死循环；`applied=false` 不 fail run |
| 可读性 | ✅（P3 备注） | 命名与既有风格一致（`human_waiting`/`human_decided`/`reject_reason` 三值明确）；注释解释「为什么」（顺序冻结、无期限例外、无 worker、E-6）；测试命名表达意图、断言强（含记录字段逐字段、end 状态、GateResult、killed_workers=0）；P3 仅测试补充与公共 API 文档后续项 |

## 3. 发现清单

> 级别口径：P0 = 阻塞；P1 = 重要；P2 = 一般；P3 = 建议（可后置，建议进台账）。本分支 **P0=0、P1=0、P2=0**。

| # | 级别 | 位置 | 问题 | 建议 | 状态 |
|---|---|---|---|---|---|
| N1 | P3 建议 | `crates/zgraph-kernel/tests/human_wait.rs`（对照 DR-006 §7.3 #7/E-12） | **库 API `trace_enabled=false` 的 human 路径无测试**：DR 冻结「库 API 无 DB 依赖、`TraceSink` no-op 合法」，实现上 `Emitter.enabled=false` 只影响 sink 回调、不影响决策/发布；当前无用例锁住该场景 | 可选补 1 例：`options.trace_enabled=false` + human 图 + control 决策 → Completed/GateResult/publish 正常、sink 无 human_decided 回调（applied 不可观测属冻结边界） | open（建议项） |
| N2 | P3 建议 | `crates/zgraph-kernel/tests/human_wait.rs:289-343` | **input 快照/期限起点测试偏薄**：当前 human 节点只有单输入端口，`instances.input` 只断言 1 键；DR §2.2「全部声明端口、未到 null」与「deadline 自等待起点」仅由代码保证（timeout 用例 elapsed≥50 间接覆盖） | 可补 1 个 `trigger=any` 多端口 human 图：断言 input 含全部声明端口且未到为 `null`、`started_at`/deadline 起点与进入 WaitingHuman 一致 | open（建议项） |
| N3 | P3 建议 | `crates/zgraph-kernel/tests/human_wait.rs:429-469`（rejected 用例） | rejected 用例通过两个 end 的 Completed/Skipped 间接证明出边路由，但未像 approved 用例那样逐边断言 `GateResult`（approved_gate.passed / rejected_gate.passed）；断言强度略不对称 | 可选在 rejected 用例补 2 条 GateResult：`end_rejected` 边 passed=true、`end_approved` 边 passed=false | open（建议项） |
| N4 | P3 建议 | `crates/zgraph-kernel/src/run.rs:759-790`（applied 确认时机） | 决策 applied=true 的 `human_decided` 在 `apply_publish` **之后**发送；若该次 publish 自身触发 `fail_run`（如 `RUN_DELIVERY_TO_TERMINAL`/`NODE_FANOUT_EXCEEDED`）或 run 恰在此前终态，Emitter 已 closed → 确认被丢弃、决策行留 PENDING。与 DR E-6（行留 W3b、W3a 不重试）一致，属已冻结审计边界，但无测试/文档显式覆盖 | W3a-docs 在 06/DR-006 执行记录中注记该边界；可选补 kernel 用例（human 输出导致 delivery-to-terminal → 决策行确认丢失、run Failed） | open（建议项） |
| N5 | P3 建议 | `crates/zgraph-kernel/src/run.rs:71-83`（TestHooks 注释） | `force_node_timeout_ms` 注释为「覆盖所有节点的实例超时」，但对 human **不生效**（human 仅用 `declared_timeout_ms` 决定决策期限，未声明=无期限）；测试/e2e 若用该钩子构造 human 超时会失效 | 在 TestHooks 注释（或 W3a-docs 的 03 §3.2）注明「human 等待期限只由节点 `timeout_ms` 声明，不受 force_node_timeout_ms 影响」；e2e 用显式 DSL timeout | open（建议项） |
| N6 | P3 建议 | `crates/zgraph-kernel/src/{types,run,trace,graph}.rs` 公共 enum | 新增 `InstanceStatus::WaitingHuman`、`CompiledHandler::Human`、`TraceCall::HumanDecided`、`RunControl` 去 `Copy` 均为 **source-breaking**；按 task-14 由 W3a-docs 在 08 §8.7 回写。当前 kernel/cli/harness/worker 全 workspace 编译通过 | W3a-docs 勿漏 08 §8.7 备注（含 `RunControl` Copy→Clone 与 `TraceCall` 新值）；本分支不新增文档，属后续合同项 | open（建议项/合同项） |
| N7 | P3 建议 | `crates/zgraph-kernel/src/run.rs:1866-1915`（`check_timeouts`） | 多个实例同时到期时 `expired` 来自 `HashMap::values()` 迭代顺序，`first` 决定 fail_run 的 node/message；既有行为即如此，但 W3a 引入多个 human 等待者后更易触发（不影响 run Failed/各实例 TimedOut 结果） | 可选按 `(deadline, instance_id)` 排序取首个到期实例，保证 message/首错确定性（非本任务必须） | open（建议项） |
| N8 | P3 说明 | `crates/zgraph-cli/src/bridge.rs:620-648`、`:796-848` | 跨模块 cli arm 为 1 行 `WaitingHuman` 映射 + 49 行「9 状态 1:1 + 防遗漏」测试；属最小机械改动、无其它 cli 行为改动；kernel 映射 match 本身已由编译器穷举保证 | 无需动作；由 Lead 追加 cli arm 第二评审（按 task-14） | open（声明） |

## 4. 验证证据

评审人**亲自执行**（worktree `/Users/zyzs/lczyzs/code/zgraph/.worktrees/kernel-kernel-dev`，`export CARGO_HOME=/Users/zyzs/lczyzs/code/zgraph/.cargo-home`，未设 `CARGO_TARGET_DIR`，HEAD `72036b6`，工作区干净）：

```bash
cargo test -p zgraph-kernel                            # 122 passed / 0 failed（23 targets；含 human_wait 9 + compile 映射 1 + types 2，新增 12）
cargo test -p zgraph-kernel --features test-hooks      # 132 passed / 0 failed
cargo test -p zgraph-cli                               # 162 passed / 0 failed（+1 状态映射防遗漏）
cargo test --workspace                                 # 754 passed / 0 failed / 3 ignored（既有 manual，无新增 ignore）
cargo clippy --workspace --all-targets --locked -- -D warnings   # clean（exit 0）
cargo fmt --all -- --check                             # exit 0
bash scripts/dev/check.sh                              # CHECK PASSED（身份/冻结文件/分支名/progress/fmt/diff --check）

git status --porcelain                                 # 空
git merge-base HEAD main                               # adf54a1（= 本地 main；origin/main 3d0e933 为祖先）
git -c core.quotepath=false diff --name-only adf54a1..HEAD
# 9 files：kernel 5 src/3 tests + cli bridge.rs（阶段 2 机械 arm + test）；无 e2e/trace/docs/reviews 改动
git log -1 --format=%B ab4b805 / 72036b6              # 均含 Review: REV-080 / Agent: kernel-dev
```

- 测试结果：三套 + workspace 全绿，与作者自报计数一致；`doc/test-baseline.txt` 无新增失败（基线仍 0 名额）。
- 默认兼容：`cargo test -p zgraph-kernel` 的既有 110 用例全部通过（122 中新增 12）；`state_machine` 仅将矩阵从 8 状态扩为 9 状态/13 合法边，既有合法/非法断言保留；`--features test-hooks` 132（+max_fanout 6/test_hooks 3/timeout_hint 1）。
- 覆盖抽查（`tests/human_wait.rs` 9 用例）：`human_node_waits_without_dispatch_or_deadlock`（input 快照/单条 Pending→WaitingHuman/无 worker/120ms 超 grace 不静默/无死锁/Cancel→Cancelled）、`approved_decision_completes_and_publishes`（output 快照带 output、transition reason、2 条 GateResult、end/run_output、human_decided 字段逐项）、`rejected_decision_is_normal_completion`（正常 Completed、end_rejected Completed/end_approved Skipped、reason null）、`late_decisions_report_not_waiting_and_instance_missing`（applied=false 两值、run 继续、记录计数）、`stale_decision_after_rework_reports_not_current`（defer attempt-per-activation 重入、not_current 优先级、新实例再审）、`decision_deadline_times_out_without_killing_workers`（NODE_TIMEOUT/TimedOut/NodeError::Timeout/elapsed≥50/killed_workers=0）、`paused_run_applies_decision_without_dispatching`、`decision_deadline_keeps_running_while_paused`、`cold_failure_cancels_waiting_human`。
- 编译映射：`tests/compile.rs:compiles_builtin_human_handler` 断言 `builtin.human→Human`、`builtin.other→Python{module:"builtin"}`、普通 worker 仍 Python、`declared_timeout_ms=Some(250)`；`graph.rs:313-340` 精确匹配无前缀。

## 5. 遗留问题

- **cli arm 第二评审**：`72036b6` 的 `map_instance_status` 机械 arm 按 Lead 安排在合并前由 Lead 追加第二评审（本记录不为其背书）；已核对改动为 1 行映射 + 防遗漏测试，无其它 cli 行为改动（N8）。
- **W3a-docs 后续**：source-breaking 公共 enum 变更须在 08 §8.7 回写（N6）；human 期限/TestHooks 例外（N5）、E-6 审计边界（N4）建议写入 06/03/DR-006 执行记录；`human_decided` 确认由 cli-decide 分支的 bridge 实现（本分支默认 no-op，跨分支依赖已由 contract 明确，不阻塞）。
- **P3 建议（N1–N7）**：均不影响功能正确性；建议由 Lead 决定是否在 kernel 后续测试批次或 `doc/issues.md` 登记（本分支合同限定 kernel + cli arm，不新增文档）。
- 未发现需要回滚/返工问题；无阻塞项，无附加条件。

## 6. 条件关闭记录（仅「有条件通过」填写）

不适用（结论为 `通过`，无附加条件）。

## 7. 第二评审 · cli 跨模块机械改动（Lead 追加，2026-10-31）

- **评审人**：`lead`（**≠ 作者** `kernel-dev`；该跨模块改动由 Lead 批准，按 AGENTS §2「跨模块改动由 Lead 在评审记录中写明并指定第二评审人」执行）
- **评审范围**：`crates/zgraph-cli/src/bridge.rs` —— `map_instance_status` 新增 `kernel::InstanceStatus::WaitingHuman => trace::InstanceStatus::WaitingHuman` arm + 函数 doc（W3a0/DR-006 §7.1）+ `map_instance_status_covers_all_kernel_statuses` 全量 9 状态 1:1 映射防遗漏测试（`72036b6`，+54/−1）；**无其它 cli 行为/命令行/输出改动**。
- **核对结论（通过）**：
  1. 改动为最小机械透传：新 arm 与既有 8 个 arm 同构；不改变任何既有映射/输出/退出码；`crates/zgraph-cli` 其余文件零改动。
  2. 测试有效：9 个 kernel 状态逐一断言映射到 trace 同名状态（含 WaitingHuman），可防后续枚举增删漏映射；使用 `as_str` 失败消息便于定位。
  3. 与 trace crate 的 `WaitingHuman`（schema v5 CHECK 已放开）一致；`instance_snapshot`/`transition` 落库不丢等待态。
- **Lead 亲自验证**（worktree `.worktrees/kernel-kernel-dev`，`CARGO_HOME=/Users/zyzs/lczyzs/code/zgraph/.cargo-home`）：
  ```bash
  git diff ab4b805..72036b6 -- crates/zgraph-cli/src/bridge.rs   # 仅 arm + doc + 映射测试
  cargo test -p zgraph-cli --offline                             # 162 passed / 0 failed
  ```
- **结论**：**通过（第二评审）**。cli 机械改动与主审结论合并生效；分支满足合并前置。
