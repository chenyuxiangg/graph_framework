# REV-040 · kernel · ISSUE-086 `fail_timeout` 回填实例超时错误

- **分支**：`fix/kernel-m6-timeout-error`
- **作者**：`kernel-dev`（提交 `e9426bd`）
- **评审人**：`lead`（**≠ 作者**；独立评审）
- **变更范围**：`origin/main(a031269)..e9426bd`，2 文件 +67/−1（`crates/zgraph-kernel/src/run.rs` 3 行、`crates/zgraph-kernel/tests/timeout.rs` 65 行）；worktree clean、基于最新 origin/main
- **评审时间**：2026-10-31
- **关联**：ISSUE-086；`doc/spec-v1.2/06` §6.2.5；M6 DR-003 §1/§3；REV-032（发现方）

## 1. 结论

**结论：通过**

## 2. 四维评审

| 维度 | 结论 | 依据（文件/行/测试） |
|---|---|---|
| 功能正确性 | ✅ | `fail_timeout()` 在 `transition` 前调用 `set_instance_error(NodeError::Timeout { elapsed_ms })`（`run.rs`），与 watchdog `check_timeouts` 路径同口径 → 实例迁移快照携带 `NODE_TIMEOUT`；新增 `executor_reported_timeout_snapshot_carries_timeout_error` 用 agent_call 图 + FakeExecutor 自报 Timeout 覆盖该路径，断言 TimedOut 迁移、`error_code=NODE_TIMEOUT`、error 为 Timeout 变体且 `elapsed_ms=12` 与 NodeTimedOut 事件一致。作者提供了 red→green 证据（回退 run.rs 即失败且 error_code=None）。 |
| 代码结构合理性 | ✅ | 只改 3 行 + 1 个测试，复用既有 `set_instance_error` 与 `transition_or_fail` 顺序；未扩大公开 API；未触碰其他模块。 |
| 逻辑性 | ✅ | 回填发生在迁移前、与 watchdog 路径顺序一致（先记错误再迁移）；`transition_or_fail` 失败分支直接 return 不会误写。`on_cancelled_error` 的 Cancelled 路径不落 error 属既有语义、spec 未要求（作者已声明范围外）。 |
| 可读性 | ✅ | 注释说明「与 watchdog 同口径」并引用 ISSUE-086 / 06 §6.2.5；测试命名与注释说明为什么走该路径。 |

## 3. 发现清单

| # | 级别 | 位置 | 问题 | 建议 | 状态 |
|---|---|---|---|---|---|
| 1 | 建议（后置） | `tests/e2e/harness_ac08.rs` 约 748–751 行 | 注释「自报路径当前不落 instances.error_code」已过时；e2e 未断言实例 `NODE_TIMEOUT` | 由 M6 e2e 收口分支（REV-054）补实例级断言并更新注释 | open（转 REV-054） |
| 2 | 备注 | `run.rs` `on_cancelled_error` | Cancelled 路径不 `set_instance_error` | ISSUE-086/spec §6.2.5 未要求，保持现状；如后续要求再立任务 | 无需处理 |

无阻塞 / 重要项。

## 4. 验证证据

```bash
# 评审人在干净 target（/tmp/zgraph-merge-target，先 cargo clean 全部 workspace 成员）上执行
CARGO_HOME=<repo>/.cargo-home CARGO_NET_OFFLINE=true
cargo clean -p zgraph-dsl -p zgraph-kernel -p zgraph-trace -p zgraph-worker -p zgraph-harness -p zgraph-cli -p zgraph-e2e
cargo test -p zgraph-kernel --quiet
cargo fmt --all -- --check     # 通过（作者亦报告）
bash scripts/dev/check.sh      # CHECK PASSED（作者执行）
```

- 结果：`zgraph-kernel` 全部 test target **0 failed**（lib 5 / graph 2 / run 8 / gate 1 / timeout 4 / timeout_hint 2 / test_hooks 1 / doctest 2 等）。
- 说明：本次在**干净私有 target** 上复核，规避了 M6 期间共享 `CARGO_TARGET_DIR` 的跨 worktree 产物串味（已由 Lead 处置：共享 target 删除、改为私有/带 clean 的合并 target）。

## 5. 遗留问题

- 已登记/转后续：e2e 实例级断言 → REV-054（M6 W3）。
- 无新增 ISSUE。
