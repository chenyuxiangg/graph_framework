# REV-052 · harness/worker/cli · ISSUE-079 工具并发纳入全局 worker 上限

- **分支**：`fix/harness-m6-tool-limits`
- **作者**：`harness-dev`（提交 `0d4aab1`，rebase 后；原 `31ffbb0`）
- **评审人**：`lead`（**≠ 作者**；主评审）＋ `reviewer-a`（**跨模块第二评审**，AGENTS §2 要求）
- **变更范围**：`main(c35028a)..0d4aab1`，9 文件 +559/−71：`crates/zgraph-worker/{src/executor.rs,src/lib.rs}`（只增公共 API）、`crates/zgraph-harness/{src/{ctx,executor,agent_loop,tools,test_support,lib}.rs}`、`crates/zgraph-cli/src/run_cmd.rs`（组合根 1 处）；**worker 协议未改、e2e 未改**；worktree clean、已 rebase 到最新 main
- **评审时间**：2026-10-31
- **关联**：ISSUE-079；`doc/architecture-v2.md` §8 D4；`05` §5.13.1 D13 / §5.2.6；DR-003 §3 D7；REV-045（依赖治理）

> **跨模块声明**：harness（主）+ worker（只增公共 API）+ cli（组合根 1 处）。Lead 已批准；第二评审人 = `reviewer-a`（第二评审小节见文末，合入前追加）。

## 1. 结论

**结论：通过**（主评审；第二评审小节由 reviewer-a 追加后生效）

## 2. 四维评审

| 维度 | 结论 | 依据（文件/行/测试） |
|---|---|---|
| 功能正确性 | ✅ | worker 新增 `WorkerLimits` 共享句柄（`global` CPU×2 / `per_run` 8；`new`/`default_limits`/getters/`acquire_run`/`acquire_global`），`WorkerExecutor` 改持 `Arc<WorkerLimits>`，新增 `with_shared_limits`/`limits()`；**`new`/`with_limits`/`max_workers()`/`max_workers_per_run()` 语义与返回值不变**。harness：`HarnessCtx` 增 `run_id()`（默认 `"unknown"`）/`tool_limits()`（默认 None，不破坏既有 impl）；`HarnessExecutor` 增 `with_tool_limits`/`with_llm_recorder_and_tool_limits`，`HarnessSettings` 字段集合不变；工具调用与 discover 统一按「先每 run、后全局」取 permit、持有到工具 worker 调用结束；未注入回退私有默认限制。cli：`build_executor` 创建 `Arc<WorkerLimits>::default_limits()` 同时注入 WorkerExecutor 与 HarnessExecutor → 工具调用与节点执行共享同一实例。新增测试覆盖：工具+节点叠加峰值 ≤ 全局上限；两 run 共享 limiter 时每 run ≤8 且互不串；未注入回退行为；execute_tool 端到端等待共享许可；executor 经内核注入生效。 |
| 代码结构合理性 | ✅ | `WorkerLimits` 把原私有信号量语义提为可注入句柄，只增 API；harness 用内部 `HarnessNodeCtx` 包装（仅注入路径）避免改动 `HarnessSettings` 与既有构造器；`WorkerToolRunner` 不再自持信号量（许可上移到 Harness 调用点统一获取，避免双重限额）；cli 改动收敛到组合根 1 处。 |
| 逻辑性 | ✅ | 加锁顺序与 `WorkerExecutor::execute` 现有固定顺序一致（先 run 后 global，`executor.rs` 有注释「固定顺序」）；死锁论证：`agent_call` 不占 `WorkerExecutor` permit，工具调用不会嵌套在节点 permit 内，所有消费者同序 → 无交叉等待环；permit 持有至工具调用结束、drop 归还；取消等待时 permit 随 future abort 归还，无泄漏。 |
| 可读性 | ✅ | 新增公共类型/方法带文档；`WorkerToolRunner` 注释说明「不再自持锁」的原因；测试名表达语义（`stacked_tool_and_node_permits_stay_within_global_cap` 等）。 |

## 3. 发现清单

| # | 级别 | 位置 | 问题 | 建议 | 状态 |
|---|---|---|---|---|---|
| 1 | 偏差（已接受） | `harness/executor.rs` `HarnessNodeCtx` | D15 原为「生产唯一实现 = NodeContext、无包装类型」；注入 limiter 时出现内部包装 | 仅注入路径出现、未注入路径原样；DR-003 D7 要求 HarnessCtx 带 limiter，属局部修订，本记录声明 | 接受 |
| 2 | 偏差（已接受） | 未注入路径 | 旧实现只有全局 CPU×2；未注入回退新增每 run 8 上限（按 D7「默认 CPU×2 / 每 run 8」） | 行为变更仅在「未注入」路径，cli 生产路径已注入共享实例；e2e 不注入时走回退（AC-08/AC-12 预期不受影响） | 接受 |
| 3 | 建议（后续） | `tests/e2e/ac12.rs`（约 L25/L41/L45/L51）、`common::executor_with_limits`、`harness_ac08.rs` | e2e 仍述「工具不受全局上限」旧口径，且未注入共享 limiter | 由 e2e 后续分支（REV-060 计划）改为注入 `worker.limits()` 并更新峰值断言（同时覆盖工具+节点，无需放宽） | open（转 e2e 后续） |
| 4 | 备注 | 等待 permit 期间 | 取消感知仅在 future abort 层（与旧 `WorkerToolRunner::permit` 一致），permit 由 abort 归还 | 可接受；如需更细可另立任务 | 无需处理 |

无阻塞 / 重要项。

## 4. 验证证据

```bash
# 评审人在干净 target（/tmp/zgraph-merge-target，先 cargo clean 全部 workspace 成员）上执行；分支已 rebase 到 main c35028a
CARGO_HOME=<repo>/.cargo-home CARGO_NET_OFFLINE=true
cargo clean -p zgraph-dsl -p zgraph-kernel -p zgraph-trace -p zgraph-worker -p zgraph-harness -p zgraph-cli -p zgraph-e2e
cargo test -p zgraph-harness -p zgraph-worker -p zgraph-cli --quiet
cargo clippy --workspace --all-targets --locked -- -D warnings
```

- 结果：`zgraph-harness` **143 passed / 0 failed / 1 ignored**（+6 新测试）；`zgraph-worker` 新增 4 个共享 limiter 测试；`zgraph-cli` 125 passed；workspace clippy `-D warnings` 通过（3.77s）。
- 作者另报告：`cargo fmt --all` OK、`check.sh` PASS、rebase 无冲突。

## 5. 遗留问题

- e2e 适配（AC-12 注释/断言、注入共享 limiter）→ 后续 e2e 分支（REV-060 计划）。
- 05/06/07 的「工具不受全局上限」旧注记 → REV-057 spec 回写。
- 无阻塞项。

## 6. 跨模块第二评审（reviewer-a）

> **身份/范围**：`reviewer-a` 非作者；本分支跨 **harness（主）+ worker（只增公共 API）+ cli（组合根 1 处）**，按 AGENTS §2 由 Lead 批准并指定本人作第二评审。Lead 主评审已给「通过」；本小节独立核对 worker 公共 API、harness 包装/未注入路径、cli 同实例注入、加锁顺序与 permit 生命周期、测试有效性与 e2e 影响，未修改任何代码，仅追加本小节。**第二评审结论：通过**（主评审结论生效，无新增阻塞项）。

### 6.1 worker 公共 API 只增不改、封装边界 ✅

- `WorkerLimits`（`crates/zgraph-worker/src/executor.rs:58-121`）字段**全私有**；`Debug`（`:61-68`）只输出 `max_workers`/`max_workers_per_run` 且 `finish_non_exhaustive`，不泄露两个 `Semaphore`/per-run `HashMap`；对外只有只读 getters 与 `acquire_run`/`acquire_global`（返回 `OwnedSemaphorePermit`，不暴露 semaphore 本身）。
- `WorkerExecutor` 改为持 `Arc<WorkerLimits>`：`new`（`:132-135`）与 `with_limits`（`:137-144`）签名/语义不变（默认 CPU×2 / 每 run 8；0→1 夹取；`max_workers`/`max_workers_per_run` 委托 `:154-161`）；只新增 `with_shared_limits`（`:149`）与 `limits()`（`:164`）；`crates/zgraph-worker/src/lib.rs:17-19` 导出只加 `WorkerLimits`，无移除/改名（`git diff` 负行仅格式化的 `pub use` 行，非删除项）。
- 既有 acquire 错误语义保留：内部 `acquire_run/acquire_global`（`:179-194`）仍映射 `internal("worker run/global semaphore closed")`；worker 协议未改（`schemas/` 无 diff）。
- 结论：公共 API **只增不破**；`acquire_*` 为跨 crate 共享所需的新增 pub 方法（tokio 类型），不泄漏内部可变状态，仅让调用方持有 permit（建议见 §6.7 #4）。

### 6.2 harness 包装路径 / 未注入路径 / `HarnessSettings` ✅

- `context_for`（`crates/zgraph-harness/src/executor.rs:159-167`）：仅 `tool_limits.is_some()` 时用**私有** `HarnessNodeCtx`（`:226-268`）包装 `NodeContext`；`None` 时仍 `Arc::new(ctx.clone())`，即 `impl HarnessCtx for NodeContext`（`ctx.rs:64-85`）→ **D15「生产唯一实现 = NodeContext」在未注入路径原样保留**。
- `HarnessNodeCtx` 实现 `HarnessCtx` 全部方法并转发 `NodeContext`（emit/cancel/deadline/user_id/get_var/seed/instance_id/run_id），只额外覆写 `tool_limits`（`:264`）→ 仅注入路径携带共享 limiter，状态来源仍唯一（`NodeContext`），无状态分叉。
- `HarnessCtx`（`ctx.rs:41-51`）新增 `run_id`（默认 `"unknown"`）与 `tool_limits`（默认 `None`）**均有默认实现**；`impl HarnessCtx for NodeContext` 只覆写 `run_id`（`:83`），`tool_limits` 保持 None → 既有 impl/test 零改动（编译验证：其余 impl 未改仍通过）。
- `HarnessSettings`（`executor.rs:34-45`）字段集合未变；`HarnessExecutor::new`/`with_llm_recorder` 行为不变（`tool_limits: None`）；只增 `with_tool_limits`（`:101`）与 `with_llm_recorder_and_tool_limits`（`:111`）；CLI 普通路径/`--record=full` 分别走两者。

### 6.3 cli 同一 `Arc<WorkerLimits>` 注入两个执行器 ✅

- `run_cmd::build_executor`（`crates/zgraph-cli/src/run_cmd.rs:909-934`）：`let limits = Arc::new(WorkerLimits::default_limits())`（`:914`）→ `WorkerExecutor::with_shared_limits(..., limits.clone())`（`:915-918`）→ `HarnessExecutor::with_llm_recorder_and_tool_limits(settings, recorder, limits)` / `with_tool_limits(settings, limits)`（`:927-930`）→ **同一实例**（clone 出的两个 Arc 指向同一 `WorkerLimits`；`RecordingExecutor`/`RoutingExecutor` 仅包装不重建）。
- 全仓核对：CLI 生产路径仅此一处创建/注入 `WorkerLimits`；未注入的测试/e2e 走 harness 回退（私有静态实例），不改变生产组合根语义。

### 6.4 加锁顺序 / permit 生命周期 / 死锁 ✅

- 顺序与 `WorkerExecutor` 一致：harness `acquire_tool_permits` 先 `acquire_run(ctx.run_id())` 再 `acquire_global()`（`tools.rs:1138-1155`）；worker `WorkerExecutor::execute` 同为 run→global（`worker/executor.rs:204-205`）；旧 harness 全局-only 信号量已删除，全仓无「先 global 后 run」的逆向获取。
- 无死锁环：`agent_call` 节点执行经 `HarnessExecutor`，**不占** `WorkerExecutor` 的节点 permit；工具/discover 之间串行（discover 先于 loop）；permit 为单向 run→global，不嵌套持锁等待另一类 permit；`tools_cache` 锁只在 registry 路径（锁序 cache→limiter，无线反向路径），不构成环路。
- permit 生命周期：`execute_tool` 取得后持有到 `run_tool_call().await` 结束再 `drop`（`agent_loop.rs:644-652`）；`?`/Err 提前返回及 future 被 abort 时，owned permit 随作用域/future drop 归还（无泄漏/重复归还）；`registry` 的 discover permit 在 await 后显式 `drop`（`executor.rs:130-157`）。
- 唯一可改进的锁粒度（非缺陷）：`registry` 在 `tools_cache` async Mutex 内 await 「取 permit + discover」（discover 跨 await 为既有模式，本分支新增 permit 等待），permit 饱和时不同 run/不同 tools_dir 的首次加载会队头阻塞；因锁序单向，**不是死锁**，仅吞吐建议（§6.7 #1）。

### 6.5 新增测试有效性（叠加峰值 / 跨 run 隔离 / 回退）✅

- worker 新增 **4** 用例（`executor.rs:690-749`）：默认容量 + 0→1；共享实例 `Arc::ptr_eq` + 全局 cap 阻塞；每 run 隔离；per-run=8 上限。**独立复跑**：`cargo test -p zgraph-worker --lib limits` → **5 passed / 0 failed**（含既有协议契约测试）。
- harness 新增 **6** 用例：`tools.rs:2218` 注入 limiter 全局阻塞、`:2238` 每 run 隔离、`:2255` 未注入回退（断言 `tool_limits()==None` + 默认容量）、`:2270` **工具+节点叠加峰值**（6 task 混合 tool/node permits，`peak<=global cap` 且计数归零）、`executor.rs:654` 经内核端到端注入生效、`agent_loop.rs:1616` `execute_tool` 路径等待共享许可。**独立复跑**：`cargo test -p zgraph-harness --lib` → **143 passed / 0 failed / 1 ignored**（与主评审一致；1 ignored 为真实模型 smoke）。测试确实覆盖「叠加峰值、跨 run 隔离、未注入回退」三类关键语义。
- 覆盖缺口（可接受）：CLI `build_executor` 同实例注入无自动化断言（私有 fn）；当前由静态核对 + harness 端到端 + worker 单测覆盖，e2e 后续 REV-060 可用共享峰值用例补（§6.7 #2）。

### 6.6 e2e 影响评估（本分支未改 e2e，旧注释后续适配）✅

- `git diff --name-only` 无 `tests/**`、`schemas/**`、`Cargo.lock` 改动 → **e2e 与 worker 协议未动**，跨模块边界与作者报告一致（9 文件 +559/−71）。
- 旧口径残留（确认存在，需后续分支适配）：`tests/e2e/ac12.rs:45,856` 仍写「工具走独立进程级信号量、不计入 `with_limits` 全局/每 run 统计」；spec 05 §5.6.5（`05:969`）与 §5.13 D13（`05:1374`）仍写「工具不占每 run 许可」。与 DR-003 D7 新口径不符；Lead 已登记转 **REV-060（e2e 注入共享 limiter + 更新峰值断言）** 与 **REV-057（spec 回写）**，本分支不改 e2e 属正确边界。
- 合并门禁行为评估：e2e 未注入 → 走进程级回退 limiter（容量同旧 CPU×2，另加每 run 8 上限）；现有 e2e 并发工具 ≤1/≤2、python 节点仍用各自 `WorkerExecutor::with_limits`，不触发新上限；CLI e2e 用真 `zgraph`（已注入共享实例）但单 run/单工具，预期不变。`merge.sh` 会跑全 workspace（含 e2e）作最终确认。

### 6.7 建议（非阻塞）

1. `registry`（`executor.rs:130-157`）缩小 `tools_cache` 锁范围：先锁查/释放 → 取 permit + discover → 再锁双检插入，避免异步锁跨 await 放大串行；无死锁，仅吞吐。
2. CLI `build_executor` 的「同一实例」可加一条集成断言（或在 REV-060 e2e 共享峰值用例中覆盖）。
3. `HarnessCtx::run_id` 默认 `"unknown"`：第三方若只覆写 `tool_limits` 而不覆写 `run_id`，所有 run 会共用同一 per-run 桶；建议在 trait 文档注明「注入 limiter 时应同时实现 run_id」（当前已写默认值，可再强调）。
4. worker `WorkerLimits::acquire_*` 为跨 crate pub API（返回 tokio permit）；建议文档注明「供组合根/执行器使用，勿绕过 Helper 长期持有/forget」，不阻塞。

> 第二评审结论：**通过**。主评审「通过」+ 本小节 → REV-052 生效；§6.7 建议可随 REV-060/REV-057 处理，不阻塞合并。

