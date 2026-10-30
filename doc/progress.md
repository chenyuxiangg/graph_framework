# 开发进度

> 更新规则：任务开始 / 完成 / 阻塞 / 交接时更新；只保留当前状态（历史看 git 与 `doc/reviews/`）；全文件 ≤ 30 行。

**当前阶段**：M4（Harness）已完成并合并；下一里程碑 M5（CLI / replay / 多 run / legacy 退役）

| 模块 | 状态 | 当前任务 | 负责 | 分支 / worktree | 更新 |
|---|---|---|---|---|---|
| process | done | M0 骨架；M2/M3 门禁；M4 依赖 genai/async-trait/tokio-util（REV-017） | lead | — | 2026-10-30 |
| dsl | done | M1 DSL 类型/loader/R1–R14/graph_hash/validate（REV-005） | dsl-spec-writer | — | 2026-10-29 |
| kernel | done | M2 状态机/事件循环/门控/超时/死锁；M4 `TOOL_ERROR` + `HarnessErrorKind::ContextOverflow`（REV-018） | lead | — | 2026-10-30 |
| trace | done | M2 SQLite v2/写线程/L1 reader/L2 记录表（REV-008） | trace-dev | — | 2026-10-29 |
| cli | done | M2 `zgraph trace` + kernel→trace 桥（REV-009）；M5 补 run/replay | cli-dev | — | 2026-10-29 |
| worker | done | M3 supervisor/WorkerExecutor（REV-012）；M4 `WorkerFailure::Worker` 装箱消除 clippy 回归（REV-021） | lead | — | 2026-10-30 |
| py-worker | done | M4 `zgraph_worker.tool_runner` + 工具 fixtures + 可选 timeout_ms（REV-019） | py-worker-dev | — | 2026-10-30 |
| harness | done | M4 AgentHarness：config/llm(genai 0.7.2+fake)/budget/loop/tools/executor（REV-020） | harness-dev | — | 2026-10-30 |
| e2e | done | M4 `tests/e2e/harness_ac08.rs`：fake LLM/token 入 trace/tool 超时/异常/未知工具（REV-022） | e2e-dev | — | 2026-10-30 |
| docs | done | M4 spec 05/00/04/07 v2 回写 + PROVENANCE（REV-023，docs-writer）；收尾台账/索引/README（REV-024，lead） | docs-writer / lead | — | 2026-10-30 |

**下一步**
1. M5：CLI `run`/`replay`（`--fake-llm`/`--real`）、L2 重放、AC-12 多 run 隔离、legacy 退役决策。
2. M5 前修 P2 遗留：L2 记录接线（ISSUE-082）、工具结果截断合法 JSON（ISSUE-074）、`node_timeout_ms` 回退（ISSUE-076）、gate 幻影 attempt（ISSUE-061）、`NODE_WORKER_CRASH` 记录 signal/exit（ISSUE-066）。
3. 其余 M4 建议项按 ISSUE-073/075/077–081 在 M5 收敛；08 审计矩阵注记随 M5 docs。

**阻塞**
- 无硬阻塞；M0–M4 本地测试全绿（`cargo test --workspace` 423 passed/2 ignored、pytest 84 passed）。
