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

> 待 reviewer-a 追加：确认 worker 公共 API 只增不改、harness 包装上下文不外泄、cli 组合根注入同一实例、加锁顺序与 e2e 影响评估。
