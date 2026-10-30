# 开发进度

> 更新规则：任务开始 / 完成 / 阻塞 / 交接时更新；只保留当前状态（历史看 git 与 `doc/reviews/`）；全文件 ≤ 30 行。

**当前阶段**：M5（收尾）：wave1 已合并（REV-025/026/027/029），cli `run` 待 REV-028 C1 关闭；wave2 进行中（replay/L2、trace 崩溃标定、ISSUE-066、docs 回写）；口径见 [`decisions/DR-002-m5-cli-replay-recording.md`](decisions/DR-002-m5-cli-replay-recording.md)

| 模块 | 状态 | 当前任务 | 负责 | 分支 / worktree | 更新 |
|---|---|---|---|---|---|
| kernel | done | ISSUE-061 修复 + ISSUE-076 内核侧 timeout_hint（REV-026，main `f89ddaa`） | kernel-dev | — | 2026-10-30 |
| harness | wip | wave1 已合（REV-027）；wave2 ISSUE-076 harness 侧 `timeout_hint`（REV-033） | harness-dev | `fix/harness-m5-timeout` | 2026-10-30 |
| cli | wip | `zgraph run` 已实现（REV-028 条件通过，C1 转 ISSUE-084 中）；wave2 replay/L2 + llm_calls + 标定调用 | cli-dev | `feat/cli-m5-run` | 2026-10-30 |
| e2e | done | AC-12 10 run 并发隔离（REV-029，main `2b255de`）；wave2 CLI/L2 链路待启动 | e2e-dev | — | 2026-10-30 |
| trace | wip | wave2 ISSUE-084 中止标定 API + ISSUE-048 owner_pid（REV-034） | trace-dev | 待建 | 2026-10-30 |
| worker | todo | wave2 ISSUE-066 signal/exit 载荷（跨模块，REV-031） | worker-dev | 待建 | 2026-10-30 |
| process/docs | wip | DR-002 + kickoff（REV-025 已合）；本分支登记 ISSUE-084/085 并修订 DR-002/进度（REV-038） | lead | `docs/process-m5-issue-reg` | 2026-10-30 |
| docs | todo | wave2 spec 回写 00/04/05/06/07/08 + PROVENANCE/索引（REV-035） | docs-writer | 待建 | 2026-10-30 |

**下一步**
1. 关闭 REV-028 C1（本分支 ISSUE-084 登记 + DR-002 修订）→ cli rebase → 复审 → 合并 wave1。
2. wave2：cli `replay --mode=events|outputs` + L2 重放 + `llm_calls`/标定接线；harness timeout_hint；trace 标定；ISSUE-066；e2e CLI/L2 链路。
3. docs 回写（REV-035）+ 收尾（REV-036）+ process 依赖治理/CI（REV-037）；ISSUE-033/036/073/059/075/077/079/080/081/085 留 M6（DR-002 §7/§9）。

**阻塞**
- 无硬阻塞；基线 `cargo test --workspace` 461 passed/2 ignored（wave1 合并后）、pytest 84 passed；共享 `CARGO_TARGET_DIR` 等锁/隔离 merge target 属已知环境操作（见 DR-002/评审记录）。
