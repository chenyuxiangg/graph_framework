# 开发进度

> 更新规则：任务开始 / 完成 / 阻塞 / 交接时更新；只保留当前状态（历史看 git 与 `doc/reviews/`）；全文件 ≤ 30 行。

**当前阶段**：M2（内核 + trace + cli trace/验收）全部合并（main=4a15271）；下一步 M3 worker

| 模块 | 状态 | 当前任务 | 负责 | 分支 / worktree | 更新 |
|---|---|---|---|---|---|
| process | done | M0 骨架；M2 依赖预热（REV-006，002071d） | lead | — | 2026-10-29 |
| dsl | done | M1 DSL 类型/loader/R1–R14/graph_hash/validate（REV-005，9aa0b9e） | dsl-spec-writer | — | 2026-10-29 |
| kernel | done | M2 状态机/事件循环/Delivery/门控/Skipped/冷峻失败/超时/控制/死锁（REV-007，4570a53） | kernel-dev | — | 2026-10-29 |
| trace | done | M2 SQLite v2/写线程/L1 reader/L2 记录表（REV-008，8815370） | trace-dev | — | 2026-10-29 |
| cli | done | M2 `zgraph trace` L1 + kernel→trace 桥 + AC-01/02/03/05/06/07/10/11 验收（REV-009，4a15271） | cli-dev | — | 2026-10-29 |
| worker | next | M3：真 worker/进程组 kill/并发上限/协议异常分支（ISSUE-025/047） | 待定 | — | 2026-10-29 |
| harness | next | M4：LLM 流式 + tool（经 worker）+ 预算 | 待定 | — | 2026-10-29 |
| e2e | next | M3–M5：AC-01..AC-12 真 worker/LLM/多 run 场景（REV-009 后迁移 `tests/e2e`） | 待定 | — | 2026-10-29 |

**下一步**
1. M3 worker：真子进程 + 进程组 kill + 并发上限 + 协议异常分支；回归 AC-06/07 真进程断言（ISSUE-047）。
2. M4 Harness / M5 CLI run·replay + e2e 迁移（ISSUE-052）。

**阻塞**
- 无硬阻塞；ISSUE-014/015/020/033–059 按里程碑推进（spec 回写 = ISSUE-050）。
