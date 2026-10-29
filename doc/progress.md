# 开发进度

> 更新规则：任务开始 / 完成 / 阻塞 / 交接时更新；只保留当前状态（历史看 git 与 `doc/reviews/`）；全文件 ≤ 30 行。

**当前阶段**：M0 已合并（main=202d5c4）→ M1 DSL 待 REV-005 评审

| 模块 | 状态 | 当前任务 | 负责 | 分支 / worktree | 更新 |
|---|---|---|---|---|---|
| process | done | M0 骨架（202d5c4） | lead | — | 2026-10-29 |
| dsl | review | M1：类型/loader/R1–R14/graph_hash/validate CLI（REV-005） | dsl-spec-writer | feat/dsl-m1-loader | 2026-10-29 |
| kernel | next | M2：状态机/事件循环/Delivery/门控/冷峻失败/超时 kill | 待定 | — | 2026-10-29 |
| trace | next | M2：TraceEvent + SQLite + L1 replay + L2 记录表 | 待定 | — | 2026-10-29 |
| worker | next | M3：进程组 kill/并发上限/协议异常分支（ISSUE-025） | 待定 | — | 2026-10-29 |
| harness | next | M4：LLM 流式 + tool（经 worker）+ 预算 | 待定 | — | 2026-10-29 |
| cli | next | M5：validate/run/trace/replay | 待定 | — | 2026-10-29 |
| e2e | next | M1–M5：AC-01..AC-12 移植 | 待定 | — | 2026-10-29 |

**下一步**
1. REV-005 评审通过 → 合并 M1 → 推送。
2. M2 内核（kernel/trace）。

**阻塞**
- 无硬阻塞；ISSUE-014（门控模型）、ISSUE-015/025（协议分支补齐）、ISSUE-020（spec 回写）按里程碑推进。
