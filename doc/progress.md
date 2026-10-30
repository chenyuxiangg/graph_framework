# 开发进度

> 更新规则：任务开始 / 完成 / 阻塞 / 交接时更新；只保留当前状态（历史看 git 与 `doc/reviews/`）；全文件 ≤ 30 行。

**当前阶段**：M5（收尾）**已完成**：wave1/wave2 全部交付并合入 main（REV-025–036/038；REV-037 顺延 M6）；本分支 REV-036 收口台账/进度/索引/baseline/README。口径见 [`decisions/DR-002-m5-cli-replay-recording.md`](decisions/DR-002-m5-cli-replay-recording.md)

| 模块 | 状态 | M5 交付 | REV |
|---|---|---|---|
| kernel | done | ISSUE-061 gate 幻影 attempt；ISSUE-076 内核侧 `declared_timeout_ms`+`timeout_hint` | REV-026 |
| harness | done | `LlmCallRecorder` seam；ISSUE-074 合法 JSON 截断；ISSUE-081 主清单拆函数；ISSUE-076 harness 侧 | REV-027/033 |
| cli | done | `zgraph run` 全契约；`replay` L1/L2；`llm_calls`/`recorded_outputs`；启动中止标定/`recovered[]`；`timeout_hint` 转发 | REV-028/030 |
| e2e | done | AC-12 10 run 并发隔离；AC-01 CLI/L2/崩溃标定/ISSUE-076 真链路 | REV-029/032 |
| trace | done | ISSUE-084 中止标定原语（`interrupted_candidates`/`mark_interrupted`）；ISSUE-048 `RunRow.owner_pid` | REV-034 |
| worker | done | ISSUE-066 `NODE_WORKER_CRASH` 记录 signal/exit/stderr_tail（跨 4 模块，双评审） | REV-031 |
| docs | done | spec 00/04/05/06/07/08 M5 回写 + PROVENANCE/索引/README/legacy（00–08 合计 5958 行） | REV-035 |
| process/docs | done | DR-002 决策/波次/台账、M5 kickoff 与收尾 | REV-025/038/036 |

**下一步（M6）**
1. M6：DR-002 §7/§9 顺延（ISSUE-033/036/059/073/081 剩余/082 版本列/084→088）+ DR-002 §4 顺延（075/077/079/080）+ REV-028/030/032 新登记（085/086/087/089）。
2. process 依赖治理 + CI clippy 固化（ISSUE-068/078/073；REV-037 顺延 M6）。
3. AC-12「10×同图 AC-01」加强用例（REV-032 未新增，07 已注明）；`global_vars` 入 L2 记录（ISSUE-087）。

**阻塞**
- 无硬阻塞；最终验证 `cargo test --workspace` 0 failed / 2 ignored、`pytest -q` 84 passed，基线名额 **0（全绿）**。
