# 开发进度

> 更新规则：任务开始 / 完成 / 阻塞 / 交接时更新；只保留当前状态（历史看 git 与 `doc/reviews/`）；全文件 ≤ 30 行。

**当前阶段**：M3（worker / 示例 / spec 回写 / e2e 验收）全部合并；下一里程碑 M4 Harness

| 模块 | 状态 | 当前任务 | 负责 | 分支 / worktree | 更新 |
|---|---|---|---|---|---|
| process | done | M0 骨架；M2/M3 依赖与合并门禁（REV-006/012/015） | lead | — | 2026-10-30 |
| dsl | done | M1 DSL 类型/loader/R1–R14/graph_hash/validate（REV-005） | dsl-spec-writer | — | 2026-10-29 |
| kernel | done | M2 状态机/事件循环/Delivery/门控/超时/控制/死锁（REV-007） | kernel-dev | — | 2026-10-29 |
| trace | done | M2 SQLite v2/写线程/L1 reader/L2 记录表（REV-008） | trace-dev | — | 2026-10-29 |
| cli | done | M2 `zgraph trace` + kernel→trace 桥 + AC 验收（REV-009）；M5 补 run/replay | cli-dev | — | 2026-10-29 |
| worker | done | M3 supervisor/进程组 kill/WorkerExecutor/协议检查单闭环（REV-012） | worker-dev | — | 2026-10-30 |
| py-worker | done | M3 python/workers 5 示例 + L3 幂等/副作用契约（REV-013） | py-worker-dev | — | 2026-10-30 |
| docs | done | M3 spec v1.2 M0–M2 回写 + PROVENANCE（REV-014） | docs-writer | — | 2026-10-30 |
| e2e | done | M3 `tests/e2e/**` 真 worker AC-06/07/09/10 + 故障注入 + 并发（REV-015） | e2e-dev | — | 2026-10-30 |
| harness | next | M4：LLM 流式 + tool（经 worker）+ 上下文预算 + fake provider | 待定 | — | 2026-10-30 |

**下一步**
1. M4 Harness：LLM 流式/tool 循环/上下文预算/错误映射/fake provider（architecture §6.1）。
2. M3 遗留 P2：gated self-loop 幻影 attempt（ISSUE-061）、NODE_WORKER_CRASH 记录 signal/exit（ISSUE-066）、AC-07 字面 ticker（ISSUE-063）。
3. spec 05/08 与 04 §4.2 新增 API 回写（ISSUE-067/070）；M5 CLI run/replay 与 L2 重放。

**阻塞**
- 无硬阻塞；M0–M3 测试全绿（cargo workspace 313+ tests、pytest 44 passed）；遗留项按 ISSUE-020/047/061–072 推进。
