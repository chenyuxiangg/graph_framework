# 开发进度

> 更新规则：任务开始 / 完成 / 阻塞 / 交接时更新；只保留当前状态（历史看 git 与 `doc/reviews/`）；全文件 ≤ 30 行。

**当前阶段**：阶段 1 完成（DR-001 + 架构 v2 已合并，REV-001 通过）→ M0 骨架（先修 merge.sh 门禁）

| 模块 | 状态 | 当前任务 | 负责 | 分支 / worktree | 更新 |
|---|---|---|---|---|---|
| docs | done | DR-001/架构 v2/spec v1.2 已合并（552466e） | lead | — | 2026-10-29 |
| process | review | 修 merge.sh 分支删除（ISSUE-023，REV-003 复审中）；随后 M0 | lead | feat/process-fix-merge-branch-delete | 2026-10-29 |
| dsl | next | M1：DSL 类型 + R1–R14 + graph_hash | 待定 | — | 2026-10-29 |
| kernel | next | M2：状态机/事件循环/Delivery/门控/冷峻失败/超时 kill | 待定 | — | 2026-10-29 |
| trace | next | M2：TraceEvent + SQLite + L1 replay + L2 记录表 | 待定 | — | 2026-10-29 |
| worker | next | M3：协议实现、超时 kill、崩溃隔离、示例 worker | 待定 | — | 2026-10-29 |
| harness | next | M4：LLM 流式 + tool（经 worker）+ 预算 | 待定 | — | 2026-10-29 |
| cli | next | M5：validate/run/trace/replay | 待定 | — | 2026-10-29 |
| e2e | next | M1–M5：AC-01..AC-12 移植 | 待定 | — | 2026-10-29 |

**下一步**
1. 合并 process 修复（REV-003）→ 门禁可用（注意自举两阶段，见 ISSUE-023）。
2. M0 骨架（workspace/协议/SDK/CI/legacy 冻结）。
3. M1 起按 `doc/architecture-v2.md` §6 的 AC→里程碑映射开发。

**阻塞**
- 无硬阻塞；ISSUE-014（门控）、ISSUE-019（CLAUDE 边界解释）、ISSUE-020（偏差回写）需在 M0/M1 前确认。
