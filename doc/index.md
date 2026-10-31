# 文档索引

| 文档 | 说明 |
|---|---|
| [`decisions/DR-001-runtime-route.md`](decisions/DR-001-runtime-route.md) | **技术路线决策**：Rust 内核 + Python 子进程 worker；v1.1 语义为核心；复用矩阵 |
| [`decisions/DR-002-m5-cli-replay-recording.md`](decisions/DR-002-m5-cli-replay-recording.md) | **M5 落地决策**：CLI `run`/`replay` 口径、L2 记录与重放、ISSUE-061/066/076 修复方案、wave 计划与 M6 排除项 |
| [`decisions/DR-003-m6-scope-and-waves.md`](decisions/DR-003-m6-scope-and-waves.md) | **M6 范围/波次决策**：17 个顺延 ISSUE 的模块、REV 编号、跨模块方案与验收 DoD（M6 **已完成**） |
| [`decisions/DR-004-v02-scope-and-waves.md`](decisions/DR-004-v02-scope-and-waves.md) | **v0.2（M7）范围/波次决策**：SOP v1 落地、图级有限返工、扇出上限、人工评审、动态 N；共享队列 v0.3 候选（评审 REV-063） |
| [`decisions/DR-005-w2a-defer-gate-freeze.md`](decisions/DR-005-w2a-defer-gate-freeze.md) | **W2a0 返工设计冻结**：`on_gate_false`（settle/defer）、R15 可重入校验、`DeferredReceipt`/attempt-per-activation/dormant/死锁、trace schema v4、`parse_verdict_rework`、SOP v1.1 双 profile、AC-14（评审 REV-067） |
| [`architecture-v2.md`](architecture-v2.md) | **架构蓝图 v2**：进程模型、NDJSON 协议、可复现分级、代码布局、里程碑 |
| [`design/software-team-sop-v1.md`](design/software-team-sop-v1.md) | **software_team SOP 设计稿 v1.1**：设计角色依赖 DAG + 工作 agent + per-item 评审；**approved-only v1 / rework v1.1 双 profile**（`parse_verdict_rework` + defer 回边 + `max_attempts` 预算，W2a0）；人工评审/扇出/动态 N 仍延期 |
| [`spec-v1.2/`](spec-v1.2/README.md) | **语义基准**（9 分册）：DSL/状态机/调度/Harness/存储/验收/审计关闭（落地按 DR-001 替换 PyO3 为 worker 子进程） |
| [`analysis/`](analysis/) | 冻结期归档材料：可行性评估、决策记录 01、v1.1 规格审计、spike 报告 + 源码、两份技术调研 |
| [`spec-v1.2/PROVENANCE.md`](spec-v1.2/PROVENANCE.md) | 规格来源、冻结/回填记录、SHA-256 与行数 |
| [`spec-v1.2/REFERENCES.md`](spec-v1.2/REFERENCES.md) | spec 历史引用 → 仓库内路径映射 |
| [`legacy/`](../legacy/README.md) | 旧 Python 实现：**已归档、无维护、不得新增代码**（冻结只读）：`src/`、`config/`、`tests/`、`graph_base_cli.py`、设计文档 |
| [`repo-status.md`](repo-status.md) | 2026-10-28 代码仓现状评估（历史快照；技术路线已由 DR-001 取代） |
| [`development-workflow.md`](development-workflow.md) | 开发流程细则：worktree 并行、四维评审、合并与清理、进度与台账 |
| [`progress.md`](progress.md) | 当前开发进度（保持简洁，≤30 行） |
| [`issues.md`](issues.md) | 遗留问题台账（P0–P3，唯一问题来源） |
| [`reviews/`](reviews/README.md) | 评审记录：索引、模板、REV-000–REV-036、REV-038–066（REV-000 bootstrap；M4 为 REV-017–024；M5 为 REV-025–036/038；M6 为 REV-039–061；W1/v0.2 kickoff 为 REV-062–066，均已合入 main；**REV-067 为 W2a0 评审编号，记录由评审人撰写**；REV-037/051/059 未创建） |
| [`test-baseline.txt`](test-baseline.txt) | 已知失败测试基线（合并门禁依据；只允许缩小） |

流程入口：仓库根 [`AGENTS.md`](../AGENTS.md)（强制约束）与 [`README.md`](../README.md)（快速开始）。
