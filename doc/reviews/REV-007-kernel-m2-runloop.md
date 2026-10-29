# REV-007 · kernel · M2 run 生命周期 / 事件循环 / 状态机 / 两阶段 publish

> **模块声明**：本分支只改 `crates/zgraph-kernel/**`（9 个 `src` 文件 + 17 个测试文件 + Cargo.toml 7 行 test 注册），未触及 dsl/trace/cli/文档；符合 AGENTS §2 的 kernel 模块边界。作者 `kernel-dev`，评审人 `reviewer-a`（非作者）。

- **分支**：`feat/kernel-m2-runloop`
- **作者**：`kernel-dev`
- **评审人**：`reviewer-a`
- **变更范围**：`bfec987..9dc2a27`（5 commits：`24997db` trace 契约/context/executor → `4208951` run 生命周期/事件循环 → `1069a78` 集成测试 → `95d1356` test-hooks 公开字段 → `9dc2a27` 补测；`git diff --stat` = 27 files, +6553/−2；`Cargo.lock` 未变，无 `src`/`tests` 之外改动）
- **评审时间**：2026-10-29
- **关联**：architecture-v2 §1（拓扑/所有权）/§4（执行模型）/§6.1（M2 DoD）/§8 D1/D4/D6/D7/D8/D10；spec 00 §0.2–§0.7；spec 01 全（§1.2.3 迁移矩阵、§1.3 就绪、§1.4 投递、§1.9 输出）；spec 03 §3.1–§3.15；spec 04 §4.1–§4.6；spec 07 AC-01/02/03/05/06/07/10/11

## 1. 结论

**结论：不通过**

**1 个阻塞项 + 2 个重要项**，其中阻塞项是状态机语义错误，会使合法图错误地以 `RUN_DEADLOCK` 失败，必须修复后复审：

1. **阻塞：Skipped 级联只挂在“被拦截边”上，漏掉“放行边最后结算”的顺序**（`src/run.rs:891-941` 的 passed 分支、`:1132-1180` 的 `cascade_skip`）。当多入边节点（`trigger=all`）的一条入边被 gate 永久拦截且**先**结算、另一条放行入边**后**结算时，后到的放行投递只做 `buffer_write → settle → maybe_make_ready`，不再做 Skipped 复核；节点全部入边已结算、不能就绪，却保持 `Pending`，静默超过 `deadlock_grace_ms` 后 `RUN_DEADLOCK` → run Failed。独立探针复现：join=[Pending, Cancelled]、error=`RUN_DEADLOCK`（elapsed 191ms）；按 01 §1.2.3（Pending→Skipped：入边全部结算且无数据）与 03 §3.4.2 的 `maybe_skip` 语义，应为 join/downstream `Skipped`、run `Completed`（output=None）。该缺陷也影响 Pause/Resume 场景（paused 期间同样漏复核）。
2. **重要：执行任务 panic 未被归一化**（`src/run.rs:387` tasks、`:726-774` spawn_execution）。`JoinHandle` 只被 `abort()`/`remove()`，从不 `join`；executor panic 被 Tokio 吞掉，实例保持 Running 直到节点 deadline → 探针：panic executor 在 204ms 后得到 `NODE_TIMEOUT`/`TimedOut`（默认 180s 节点超时下会挂 180s）。00 §0.7、04 §4.1.1/§4.1.4、03 §3.15 明确要求 catch_unwind → `NodeError::Internal` → `Crashed` → `fail_run`。
3. **重要：`NodeError::Timeout` 归一化缺失**（`src/run.rs:835-856` on_failed/fail_timeout）。任何 executor 返回 `Timeout` 都被当作调度器超时处理（NodeTimedOut + TimedOut + `NODE_TIMEOUT`）；04 §4.1.4 归一化规则 1 规定仅 `AgentCallHandler` 合法，其它 handler 应 warning + `Internal{handler must not produce Timeout}`。探针：python_node 返回 Timeout → 实例 TimedOut（应为 Crashed/Internal）。

其余 12 项为建议级（含作者自报风险逐项裁定，见 §4.4）；主路径实测是扎实的：AC-01/02/05/06/07/10/11 的内核语义、trace sink 顺序（RunStarted 第一/RunFinished 恰一次最后）、两阶段 publish（gate 在 executor task 内 off-loop 求值、每条出边一条 GateResult 且按 EdgeIndex 升序）、超时 kill 语义（aborted、`abandoned_tasks=0`、`killed_workers` 计数）、Pause/Resume/Cancel 幂等、disable_settle 死锁注入均通过。

> 本轮阻塞/重要项均未在作者测试中覆盖（`cargo test` 全绿但探针可复现）；按 AGENTS §3/§6，修复后由作者追加提交，本记录追加修订段复审。

## 2. 四维评审

| 维度 | 结论 | 依据（文件/行/测试） |
|---|---|---|
| 功能正确性 | ⚠️ | AC-01/02/05/06/07/10/11 主路径通过；两阶段 publish/gate 记录/超时/cancel/死锁注入/streaming 计数均有真实断言。**阻塞**：多入边节点“拦截边先结算、放行边后结算”时 Skipped 漏判 → 误报 RUN_DEADLOCK（探针 A）。**重要**：executor panic 未归一化（探针 B）、Timeout 归一化缺失（探针 C）。 |
| 代码结构合理性 | ✅ | `types`（枚举/错误码）→ `graph`（编译产物/边索引）→ `gate`（纯函数）→ `trace`（sink 契约）→ `context`（只读上下文）→ `executor`（抽象+Fake）→ `run`（状态机/事件循环）层次清晰、依赖单向；`RunState` 由事件循环独占，执行任务只持快照按 `RuntimeEvent` 回传（符合 03 §3.1）；`#![forbid(unsafe_code)]`、`#![warn(missing_docs)]`。建议见 #4（trace 记录类型与 trace crate 重复）。 |
| 逻辑性 | ❌ | Skipped 级联存在**顺序依赖**（#1）：`settle_edge` 后仅在 blocked 分支调用 `cascade_skip`，passed 分支漏复核，导致 Pending 卡死并交由死锁检测误杀；panic/Timeout 归一化与 04 §4.1.4 不一致（#2/#3）。其余：唯一迁移入口（`transition_raw`）、冷峻失败清队列/在飞 Cancelled/迟到结果丢弃、Pause 冻结派发、Cancel 幂等、死锁判定（禁用 settle 注入）均自洽。 |
| 可读性 | ✅ | 模块/字段/常量均有文档并引用 spec 章节；错误码集中 `types.rs:42-109`；注释解释“为什么”（单通道/延迟归一化约定/RunOptions FRU 构造）；时间戳统一 `now_rfc3339`。建议见 #12/#13（测试断言与长函数）。 |

## 3. 发现清单

| # | 级别 | 位置 | 问题 | 建议 | 状态 |
|---|---|---|---|---|---|
| 1 | **阻塞** | `src/run.rs:891-941`（`apply_publish` passed 分支 `:932-935`）；`src/run.rs:1132-1180`（`cascade_skip`） | Skipped 复核只发生在 gate 拦截分支。多入边节点（`trigger=all`）若“拦截边先结算、放行边后结算”，放行投递后 `all_incoming_settled && !is_ready` 仍 Pending → 静默 → `RUN_DEADLOCK`（探针 A：join=[Pending,Cancelled]、`RUN_DEADLOCK`，191ms；期望 join/downstream Skipped + run Completed、output=None）。违反 01 §1.2.3（`:93,:108`）与 03 §3.4.2 `maybe_skip` 语义（`:168-175`）。 | 在 `apply_publish` 的 passed 分支（以及任何 `settle_edge` 之后）统一做“若目标实例 Pending 且全部入边已结算且不能就绪 → `cascade_skip(to_node)`”；补两种结算顺序的用例（拦截边先/后）。 | open |
| 2 | **重要** | `src/run.rs:387`（`tasks: HashMap<InstanceId, JoinHandle<()>>`）；`:726-774`（`spawn_execution`） | 执行任务 abort/remove 之外从不观察 JoinHandle；executor panic 被 Tokio 吞掉，不转 `NodeError::Internal`，实例保持 Running 到 deadline。探针 B：panic executor → 204ms 后 `NODE_TIMEOUT`/`TimedOut`（应为 Internal/Crashed、快速失败）。违反 00 §0.7、04 §4.1.1/§4.1.4（`:92,:147`）、03 §3.15（`:358`）。 | 增加 panic 捕获：在任务内用 `catch_unwind` 包裹执行（或为每个执行任务 spawn 一个 watcher await JoinHandle，把 `JoinError::panic` 映射为 `RuntimeEvent::Failed{Internal}`）；补 panic 用例（自定义 NodeExecutor）。 | open |
| 3 | **重要** | `src/run.rs:835-856`（`on_failed`/`fail_timeout`） | `NodeError::Timeout` 未按产生者归一化：python/内置执行器返回 Timeout 也被当作调度器超时（NodeTimedOut + TimedOut + `NODE_TIMEOUT`）。04 §4.1.4 归一化规则 1（`:149-151`）只允许 AgentCallHandler；其它应 warning + `Internal{handler must not produce Timeout}`。探针 C 复现。 | 归一化时按 `CompiledNode.node_type` 判定：仅 `AgentCall` 接受 Timeout，其余转 `Internal`；补 normalize 用例。 | open |
| 4 | 建议 | `src/trace.rs`（全文件）↔ `crates/zgraph-trace/src/model.rs` | 内核自定义整套 TraceEvent 记录类型（`RunStartedInfo`/`TransitionRecord`/`GateResultRecord`/`InstanceSnapshot`/`StreamingEvent`/`RunFinishedInfo`/`RunMetrics`）与 trace crate 同名类型重复，且**无 serde**；CLI 桥接必须手写映射（尤其 `NodeError→JSON` 需 match 全部变体），两套类型有漂移风险。architecture §2.1（`doc/architecture-v2.md:48`）把 TraceEvent 归 trace 所有。 | Lead 决策并登记 ISSUE：M2 可接受“内核不依赖 trace + CLI 手写桥接”，但建议 (a) 给 kernel trace 类型与 `NodeError` 加 serde Serialize（additive，便于桥接），或 (b) M3 收敛到 trace crate（kernel → trace 依赖）；禁止 CLI 私自复制映射而不登记。 | open |
| 5 | 建议 | `src/run.rs:522-528`（`has_subgraph`）、`:462-469`（drive 早失败）、`:681-684`（`start_dispatch` Subgraph 分支，当前不可达） | 含 subgraph 节点的图在 run 启动即整体失败（不 bootstrap、零实例）；04 §4.4.4 的语义是在“就绪→派发”处检测、不创建该节点实例、其余非终态实例转 Cancelled。AC-04 附加 2 的“派发前 Failed/不创建该节点实例”满足，但上游分支也不会被执行。 | 与 04 §4.4.4 对齐（移到 `start_dispatch` 检测）或回写 spec 为“启动即拒绝”；登记 ISSUE 并补带上游节点的用例。 | open |
| 6 | 建议 | `src/run.rs:586-596`（`resume_pending`） | Resume 只扫描 Pending，未按 03 §3.8 对 ready_queue“去重排序”；且迭代 `instances.values()`（HashMap）决定入队顺序，理论上有确定性/回放风险。 | 按实例创建序/`node_order` 排序后重扫；或回写 spec 明确“resume 仅补漏、队列顺序不变”；补确定性用例。 | open |
| 7 | 建议 | `src/run.rs:1399-1433`（`aggregate_output`） | 多 end 同名 key 按 node_order 后者覆盖（符合 01 §1.9），但未按 01 §1.9（`:287`）“冲突记 warning”。 | 增加 warning 通道（或 `RunOutcome.warnings` 后续扩展）；登记 ISSUE。 | open |
| 8 | 建议 | `src/run.rs:165-171`（`RunHandle`） | 直接 drop `RunHandle` 不取消 run，事件循环继续运行、outcome 丢弃且不可再取；库 API 未文档化该行为（作者自报风险⑤）。 | 文档明示“drop ≠ cancel”；或实现 Drop 发 Cancel/记录告警；登记 ISSUE。 | open |
| 9 | 建议 | `src/run.rs:233-258`（`start_run`） | 必须在 Tokio 运行时内调用（内部 `tokio::spawn`），否则 panic；无 `Result` 兜底（作者自报风险⑤）。 | 调用 `tokio::runtime::Handle::try_current()` 校验并返回 `RunStartError`（可复用/新增错误码）；文档保留说明。 | open |
| 10 | 建议 | `src/run.rs:1435-1443`（`metrics`） | `trace_backpressure` 恒 0：内核 sink 为同步调用，无法观测无损通道背压（06 §6.4.4 要求来自发送方累计等待）。 | 接受 M2 简化并登记 ISSUE；M3/CLI 桥接改用有界通道后由生产侧计数（或扩展 sink 返回背压毫秒）。 | open |
| 11 | 建议 | `src/context.rs:36-113` | `NodeContext` 缺 04 §4.2.2 的身份 getter（`run_id`/`instance_id`/`node_id`/`attempt`）与 `attempt` 字段、`cancel_token()`；M2 in-process 视图只实现子集，与 04 §4.2 冻结面不一致。 | 明确 M2 冻结面并登记 ISSUE（M3 Python SDK 另实现）；至少补 attempt/身份 getter 或文档说明。 | open |
| 12 | 建议 | `src/run.rs:709-723`（`finish_builtin`） | 内置 start/end 的出边 gate 在事件循环内求值（`evaluate_outgoing`），而 04 §4.6.1 规定求值者在执行任务；因 D6 门控是纯 Rust 函数（微秒级）可接受，但需明示。 | 文档/ISSUE 注明“builtin 节点 gate in-loop（纯函数例外）”；M3 自定义 gate 若引入用户代码须移出 loop。 | open |
| 13 | 建议 | `tests/test_hooks.rs:134-137` | AC-07 原文（07 `:132`）要求 Pause **100ms** 内生效，测试断言 `<500ms` 偏松（作者自报风险⑥）；独立探针实测 pause 延迟 **86µs**（5ms ticker），实际远达标。 | 收紧断言到 `<100ms`；保留 5ms ticker 场景即可。 | open |
| 14 | 建议 | `tests/failure.rs:176`（108 行）、`tests/gate_skip.rs:41`（71）、`tests/timeout.rs:13`（61） | `crates/**` 的“函数 ≤60 行”等价约束（architecture §2.2）在 3 个测试函数上超限（`src/**` 全部达标）。 | 拆分或提取 helper；不阻塞。 | open |
| 15 | 建议 | `tests/` 整体 | 测试质量总体真实（状态序列、sink 调用顺序、EdgeIndex 升序/每边一次、错误码全表、文本/时序断言；`delivery_to_terminal` 还显式说明 GateResult 缺失的原因，未“为测试而测试”）。缺口：Skipped 两种结算顺序（#1）、executor panic（#2）、Timeout 归一化（#3）、resume 确定性、多 end 冲突 warning。 | 修 #1–#3 时补对应用例。 | open |

> 结论判定：**#1 阻塞未关闭 → 不通过**；#2/#3 重要项建议本轮一并修复或登记 ISSUE；#4–#15 为建议级（#4/#5/#6/#7/#10/#11 需 Lead 确认登记 `doc/issues.md`）。

## 4. 验证证据

### 4.1 范围与分支

```bash
cd /Users/zyzs/lczyzs/code/zgraph/.worktrees/kernel-kernel-dev
git log --oneline bfec987..HEAD
# 9dc2a27 test(kernel): 补充 NullTraceSink 与 FakeExecutor 默认/延迟错误用例
# 95d1356 refactor(kernel): test-hooks 字段按 spec 03 §3.2 公开（cfg 门控）
# 1069a78 test(kernel): 覆盖 M2 垂直切片（门控/状态机/失败/超时/控制/死锁/流式）
# 4208951 feat(kernel): 实现 M2 run 生命周期与事件循环（FakeExecutor 垂直切片）
# 24997db feat(kernel): 实现 M2 trace sink 契约、NodeContext 与执行器抽象
git merge-base origin/main HEAD     # bfec987（按 Lead 指示只评 bfec987..9dc2a27）
git diff --stat bfec987..HEAD       # 27 files changed, 6553 insertions(+), 2 deletions(-)
git diff --name-only bfec987..HEAD | grep -v '^crates/zgraph-kernel/' || echo "(none)"
# (none) → 无跨模块/文档改动；Cargo.lock 未变；Cargo.toml +7 行（test_hooks required-features 注册）
git status --short                  # 评审提交前空（评审探针文件已删除）
```

提交信息均含 `Review: REV-007` 与 `Agent: kernel-dev`；作者身份 `chenyuxiangg <1007423165@qq.com>`。

### 4.2 必跑命令（worktree 内，`CARGO_HOME=ROOT/.cargo-home`）

```bash
cargo fmt --all -- --check                                  # exit 0
cargo clippy --workspace --all-targets -- -D warnings       # exit 0
cargo clippy -p zgraph-kernel --all-targets --features test-hooks -- -D warnings  # exit 0
cargo test -p zgraph-kernel                                 # 70 passed / 0 failed（68 集成 + 2 doctest）
cargo test -p zgraph-kernel --features test-hooks           # 73 passed / 0 failed（+3 test_hooks）
cargo test --workspace                                      # 172 passed / 0 failed
bash scripts/dev/check.sh                                   # 13 项 OK → CHECK PASSED
/Users/zyzs/lczyzs/code/zgraph/.venv/bin/python -m pytest   # 20 passed
# 连跑 3 次 `cargo test -p zgraph-kernel` + 3 次 `--features test-hooks`：全部 18/18 个 test binary ok，无 FAILED/panicked → 未见 flaky
```

### 4.3 独立探针（临时 `tests/zz_reviewer_probe.rs`，跑完已删除，未入库）

| 探针 | 构造 | 实际结果（与规格对比） |
|---|---|---|
| **A** 阻塞 | join（trigger=all，x/y required）两入边：`d→join.y` gate 恒 false（d 10ms 先完成），`a→join.x` 放行（a 80ms 后完成），deadlock_grace_ms=100 / tick=10ms | `join=[Pending, Cancelled]`、error=`RUN_DEADLOCK`、elapsed=191ms、output=None。**规格期望**：join Skipped、done 级联 Skipped、run Completed（output=None）→ **不符（阻塞 #1）** |
| **B** 重要 | 自定义 NodeExecutor 直接 panic；节点 timeout_ms=200、tick 10ms | 204ms 后 `Failed`、error=`NODE_TIMEOUT`、work=TimedOut。**规格期望**：`NODE_INTERNAL`/Crashed 快速失败（00 §0.7/04 §4.1.4/03 §3.15）→ **不符（重要 #2）** |
| **C** 重要 | python_node 返回 `NodeError::Timeout{5}` | `Failed`、error=`NODE_TIMEOUT`、work=TimedOut、NodeTimedOut 1 条。**规格期望**：warning + Internal/Crashed（04 §4.1.4 规则 1）→ **不符（重要 #3）** |
| D 达标 | chain 图 a 延迟 500ms，5ms ticker，Pause 5 次测延迟 | pause 最大延迟 **86.083µs**（规格 100ms）→ AC-07 Pause 实际达标；测试断言 500ms 偏松（#13） |
| E 正例 | paused 期间在飞 a 完成 → b Ready；b 完成且出边 gate 拦截 | Paused 时 b=Ready、finished=0；Resume 后 b=Completed、done=Skipped、run Completed、output=None → paused 投递/不派发/Skipped 级联（拦截边为最后结算）正确 |

### 4.4 作者自报风险逐项裁定

| 风险 | 评审裁定 |
|---|---|
| ① `EdgeIndex` 结构体 + `EdgeIdx=usize` 改名（spec 00 §0.2 `:36` 写 `usize`，01 §1.1.2 `:45` 定义结构体，自相冲突） | **可接受**：Rust 不允许同名 type alias + struct；结构体保留、数值别名 `EdgeIdx` 语义不变。需登记 ISSUE 并在 00 §0.2/01 §1.1.2 回写消歧。 |
| ② `RunOptions` cfg 字段使 cli 不能 5 字段 literal | **可接受**：`cargo test --workspace` 经 dev-dependency 特性合并会打开 `test-hooks`（本次实测 workspace 构建包含 kernel `test_hooks` 3 用例），cli 必须用 `RunOptions::default()` + 字段赋值或 `..Default::default()`；需转告 cli 分支（REV-009）。 |
| ③ `evaluate_gate` 返回 edge_index=0 的辅助函数 | **可接受**：文档明确“发布路径用 `evaluate_gate_for`”，测试锁定 edge_index=0 语义；建议 CLI 只用 `evaluate_gate_for`。 |
| ④ 记录类型无 serde（桥接手动映射） | **部分可接受**：M2 垂直切片合理，但重复类型 + 无 serde 会迫使 CLI 手写全部映射（含 `NodeError→JSON`），漂移风险高；建议加 serde 或收敛类型（#4，需 Lead 决策）。 |
| ⑤ 多 end 冲突无 warning / trace_backpressure 恒 0 / start_run 需 runtime / 自环溢出直接 Crashed / 丢弃 RunHandle 不自动取消 | 自环溢出直接 `Crashed{Internal}` **符合 01 §1.2.2**（attempt > max_attempts-1）；其余 4 项为已知 M2 简化：多 end warning(#7)、backpressure(#10)、runtime 校验(#9)、Drop 语义(#8) → 建议 ISSUE。 |
| ⑥ AC-07 自测 Pause <500ms（07 原文 100ms） | 探针实测 86µs 达标；测试断言偏松，建议收紧到 <100ms（#13）；不需要 spec 偏差。 |

### 4.5 手工核对（对照 spec 01/03/04）

| 核对项 | 规格 | 实际 |
|---|---|---|
| 迁移矩阵 | 01 §1.2.3 9 条合法迁移、终态不可再迁移 | ✅ `types.rs:158-171`；测试 `state_machine` 全 8×8 组合 |
| 唯一迁移入口 | 非法迁移 → RUN_ILLEGAL_TRANSITION → fail_run | ✅ `transition_raw`/`transition_or_fail`；测试覆盖 |
| 投递到终态 | 非自环 → RUN_DELIVERY_TO_TERMINAL fail-fast | ✅ `resolve_target` + 测试（并记录 GateResult 缺失原因） |
| 自环 attempt | 每次投递新实例 attempt+1；attempt>max-1 → Crashed{Internal} | ✅ `resolve_self_loop`；测试 attempt 0/1/2、第三次 Crashed |
| 两阶段 publish | gate 在 executor task off-loop；每条出边一条 GateResult（含无 gate）；EdgeIndex 升序；data 整包 | ✅ `spawn_execution` 内 `evaluate_outgoing`；`apply_publish` 排序；测试断言 a 的出边 [1,2] 升序、无 gate 边 3 条记录、整包广播 |
| Skipped 级联 | 迭代实现、全入边结算且不能就绪 → Skipped、出边结算并级联 | ⚠️ 迭代实现正确，但**只在拦截边触发**，漏“放行边最后结算”（阻塞 #1，探针 A） |
| 终止/end 聚合 | 无活跃实例 → Completed；单/多 end 按 node_order 浅合并；全 Skipped → output=None | ✅ `linear` 测试（含多 end、无 end、全 Skipped） |
| 冷峻失败 | 清队列、非终态→Cancelled、RunFinished 恰一次、迟到结果丢弃 | ✅ `failure` 测试（业务错误/Python 异常/在飞 Cancelled/已完成不改写） |
| 超时 D1 | abort、TimedOut、abandoned_tasks=0、killed_workers 计数、NodeTimedOut | ✅ `timeout` 测试（30s executor 100ms timeout，<5s 返回；`abandoned_tasks=0`、`killed_workers≥1`、无 NodeAbandoned） |
| Pause/Resume/Cancel | 冻结派发/投递仍应用/暂停不结束；Cancel 幂等；终态 no-op | ✅ `control` 测试；探针 D/E 补充实测 |
| 死锁检测 | 无 Running/Ready、Pending 存在、静默 > grace → RUN_DEADLOCK；正常 Skipped 不误报 | ✅/⚠️ `disable_settle` 注入触发正确、单入边门控不误报；但多入边混合场景误报（阻塞 #1） |
| trace sink 顺序 | RunStarted 第一、RunFinished 恰一次且最后、Emitter closed 去重 | ✅ `linear` 测试 call_log 首尾/计数；Emitter `:622-633` |
| streaming 计数 | sink 返回 false 计数进 metrics | ✅ `streaming` 测试（limit 2 → dropped 7） |
| test-hooks 隔离 | 仅 feature 构建存在 test_hooks；disable_settle/force_timeout/tick_probe | ✅ cfg 门控 + `[[test]] required-features`；非 feature 构建无该测试文件 |
| 错误码 | 00 §0.4 + §8.2 全表 | ✅ `node_error_codes_match_spec` 17 cases |
| I8 不跑 Python | 事件循环不执行用户代码；gate 为纯 Rust（D6） | ✅ 代码层无 Python；gate 纯函数（内置 start/end in-loop 见 #12） |

### 4.6 测试质量

- 真实断言：状态序列、sink 调用顺序、EdgeIndex 升序、每边一次 GateResult、错误码全表、payload/时序、超时 <5s、deadlock grace 时间窗（`elapsed >= 90ms && <5s`）；无跳过测试、无改测试迎合实现。
- 缺口（与 #1/#2/#3/#6/#7 对应）：Skipped 两种结算顺序、executor panic、Timeout 归一化、resume 确定性、多 end 冲突 warning；3 个测试函数 >60 行（#14）。

## 5. 遗留问题

- **阻塞项（必须修复后复审）**：#1 Skipped 级联顺序缺陷；#2 executor panic 归一化；#3 Timeout 归一化（后两项为重要项，建议同轮修复或至少登记 ISSUE）。
- **待 Lead 确认登记 ISSUE（编号由 Lead 分配）**：#4 trace 记录类型重复/无 serde 的收敛方案；#5 subgraph 检测点与 04 §4.4.4 对齐；#6 resume 去重排序；#7 多 end 冲突 warning；#10 trace_backpressure M2 恒 0；#11 NodeContext 冻结面缺口；①`EdgeIndex`/`EdgeIdx` 命名偏差需回写 00 §0.2/01 §1.1.2。
- **转告 cli（REV-009）**：`RunOptions` 必须用 `default()`+字段赋值/FRU 构造（`cargo test --workspace` 会经 dev-dep 特性合并打开 `test-hooks`，5 字段穷举 literal 编译失败）；`evaluate_gate` 仅便捷入口，发布路径用 `evaluate_gate_for`；trace 桥接需手写 kernel→trace 类型映射（含 `NodeError→JSON`）。
- **进程项**：本分支未更新 `doc/progress.md`/`doc/issues.md`（worktree 干净）；合并时由 Lead 按 AGENTS §5/§7 更新。未 push/merge，`main` 未被本分支改动。
- 本记录结论为 **不通过** → `merge.sh` 不应执行；作者修复后在本文件追加「修订复审」段（不删除本记录，AGENTS §6）。

## 6. 条件关闭记录

不适用（结论为 **不通过**，非“有条件通过”）。修复提交后按 AGENTS §6 在本记录追加修订复审段；阻塞项清零前不得合并。
