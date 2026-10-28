# 文档索引

| 文档 | 说明 |
|---|---|
| [`decisions/DR-001-runtime-route.md`](decisions/DR-001-runtime-route.md) | **技术路线决策**：Rust 内核 + Python 子进程 worker；v1.1 语义为核心；复用矩阵 |
| [`architecture-v2.md`](architecture-v2.md) | **架构蓝图 v2**：进程模型、NDJSON 协议、可复现分级、代码布局、里程碑 |
| [`spec-v1.2/`](spec-v1.2/README.md) | **语义基准**（9 分册）：DSL/状态机/调度/Harness/存储/验收/审计关闭（落地按 DR-001 替换 PyO3 为 worker 子进程） |
| [`analysis/spec-audit-v1.1.md`](analysis/spec-audit-v1.1.md) | 《详细设计 v1.1》的 76 条规格审计（决策追溯材料） |
| [`graph_base_design.md`](graph_base_design.md) | legacy Python 实现的设计文档（历史参考，已被 DR-001 取代） |
| [`repo-status.md`](repo-status.md) | 2026-10-28 代码仓现状评估：结构、测试结果、缺陷根因、推进顺序 |
| [`development-workflow.md`](development-workflow.md) | 开发流程细则：worktree 并行、四维评审、合并与清理、进度与台账 |
| [`progress.md`](progress.md) | 当前开发进度（保持简洁，≤30 行） |
| [`issues.md`](issues.md) | 遗留问题台账（P0–P3，唯一问题来源） |
| [`reviews/`](reviews/README.md) | 评审记录：索引、模板、REV-000（bootstrap） |
| [`test-baseline.txt`](test-baseline.txt) | 已知失败测试基线（合并门禁依据；只允许缩小） |

流程入口：仓库根 [`AGENTS.md`](../AGENTS.md)（强制约束）与 [`README.md`](../README.md)（快速开始）。
