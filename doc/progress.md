# 开发进度

> 更新规则：任务开始 / 完成 / 阻塞 / 交接时更新；只保留当前状态（历史看 git 与 `doc/reviews/`）；全文件 ≤ 30 行。

**当前阶段**：M5（收尾）进行中：CLI `run`/`replay`、L2 记录与重放、AC-12 多 run 隔离、P2 收敛、legacy 退役；口径见 [`decisions/DR-002-m5-cli-replay-recording.md`](decisions/DR-002-m5-cli-replay-recording.md)

| 模块 | 状态 | 当前任务 | 负责 | 分支 / worktree | 更新 |
|---|---|---|---|---|---|
| kernel | wip | ISSUE-061 gate 幻影 attempt；ISSUE-076 timeout_hint 内核侧（REV-026） | kernel-dev | `fix/kernel-m5-p2` / `.worktrees/kernel-kernel-dev` | 2026-10-30 |
| harness | wip | `LlmCallRecorder` seam；ISSUE-074 合法 JSON 截断；ISSUE-081 拆函数（REV-027） | harness-dev | `feat/harness-m5-record` | 2026-10-30 |
| cli | wip | `zgraph run` 全契约 + `--record` 落 `recorded_outputs`（REV-028）；wave2 replay/L2 | cli-dev | `feat/cli-m5-run` | 2026-10-30 |
| e2e | wip | AC-12 10 run 并发隔离真链路（REV-029）；wave2 CLI/L2 链路 | e2e-dev | `test/e2e-m5-ac12` | 2026-10-30 |
| process/docs | wip | M5 kickoff：DR-002 + 启动进度（REV-025） | lead | `docs/process-m5-kickoff` | 2026-10-30 |
| dsl / trace / worker / py-worker | done | M1–M4 已合并；trace/worker 在 wave2 补 ISSUE-048/066 | — | — | 2026-10-30 |

**下一步**
1. 合并 wave1（REV-025–029）后开 wave2：cli `replay --mode=events|outputs` + L2 重放、harness `timeout_hint`、ISSUE-066（kernel→worker→cli）、e2e CLI/AC-12 补链。
2. docs 回写 spec 05/06/07/00 + `PROVENANCE`；收尾台账/README/基线/评审索引。
3. ISSUE-075/077/079/080 按容量收敛；ISSUE-033/036/073/059 留 M6（DR-002 §7）。

**阻塞**
- 无硬阻塞；M4 基线 `cargo test --workspace` 423 passed/2 ignored、pytest 84 passed；4 个模块 worktree 共享 `CARGO_TARGET_DIR`，等锁属正常。
