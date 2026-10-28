# 开发进度

> 更新规则：任务开始 / 完成 / 阻塞 / 交接时更新；只保留当前状态（历史看 git 与 `doc/reviews/`）；全文件 ≤ 30 行。

**当前阶段**：阶段 1 — 架构定稿（DR-001）→ 待评审合并后进入 M0 骨架

| 模块 | 状态 | 当前任务 | 负责 | 分支 / worktree | 更新 |
|---|---|---|---|---|---|
| docs | review | DR-001 + 架构蓝图 + spec v1.2/审计入库 | lead | feat/docs-dr-001-runtime-route | 2026-10-29 |
| process | next | M0：Cargo workspace、协议 schema、worker SDK、CI、legacy 冻结 | 待定 | — | 2026-10-29 |
| dsl | next | M1：DSL 类型 + R1–R14 + graph_hash | 待定 | — | 2026-10-29 |
| kernel | next | M2：状态机/事件循环/Delivery/门控/冷峻失败 | 待定 | — | 2026-10-29 |
| trace | next | M2：TraceEvent + SQLite + replay | 待定 | — | 2026-10-29 |
| worker | next | M3：子进程协议、超时/取消、崩溃隔离 | 待定 | — | 2026-10-29 |
| harness | next | M4：LLM 流式 + tool 循环 + 预算 | 待定 | — | 2026-10-29 |
| cli | next | M5：validate/run/trace/replay | 待定 | — | 2026-10-29 |
| e2e | next | M1–M5：验收用例移植（AC-01..AC-12） | 待定 | — | 2026-10-29 |

**下一步**
1. 评审并合并 DR-001（REV-001）。
2. M0 骨架：workspace + 协议 + worker SDK + CI + legacy 冻结。
3. M1 起按 `doc/spec-v1.2/07-验收用例.md` 的 AC 分模块开发。

**阻塞**
- 无硬阻塞；ISSUE-014（门控模型）、ISSUE-016（复现粒度）需分别在 M1/M2 前定稿。
