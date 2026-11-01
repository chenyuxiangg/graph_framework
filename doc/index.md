# 文档索引

| 文档 | 说明 |
|---|---|
| [`decisions/DR-001-runtime-route.md`](decisions/DR-001-runtime-route.md) | **技术路线决策**：Rust 内核 + Python 子进程 worker；v1.1 语义为核心；复用矩阵 |
| [`decisions/DR-002-m5-cli-replay-recording.md`](decisions/DR-002-m5-cli-replay-recording.md) | **M5 落地决策**：CLI `run`/`replay` 口径、L2 记录与重放、ISSUE-061/066/076 修复方案、wave 计划与 M6 排除项 |
| [`decisions/DR-003-m6-scope-and-waves.md`](decisions/DR-003-m6-scope-and-waves.md) | **M6 范围/波次决策**：17 个顺延 ISSUE 的模块、REV 编号、跨模块方案与验收 DoD（M6 **已完成**） |
| [`decisions/DR-004-v02-scope-and-waves.md`](decisions/DR-004-v02-scope-and-waves.md) | **v0.2（M7）范围/波次决策**：SOP v1 落地、图级有限返工、扇出上限、人工评审、动态 N；共享队列 v0.3 候选（评审 REV-063；§10 执行记录：W1 已交付、W2a0+W2a 已交付、**W2b 已交付**、**W3a0 设计冻结已交付（REV-078）**、W3a 已交付（REV-079–084）、**用户 2026-11-01 决定 W3b 不做**、**W4a 设计冻结（REV-086）与 W4b 实现（REV-088–093，合并 `201341b`）已交付**；下一步发布收口（REV-094 as-built 本分支）） |
| [`decisions/DR-005-w2a-defer-gate-freeze.md`](decisions/DR-005-w2a-defer-gate-freeze.md) | **W2a0 返工设计冻结 + W2a 实现**：`on_gate_false`（settle/defer）、R15 可重入校验、`DeferredReceipt`/attempt-per-activation/dormant/死锁、trace schema v4、`parse_verdict_rework`、SOP v1.1 双 profile、AC-14（设计评审 REV-067；实现 REV-069/070/068/071，回写 REV-073） |
| [`decisions/DR-006-w3a-human-decide-freeze.md`](decisions/DR-006-w3a-human-decide-freeze.md) | **W3a0 设计冻结 + W3a 实现**：`builtin.human` 保留符号、`WaitingHuman` 生命周期、decide 自动接续（**无 `resume`**）、拒绝矩阵（reject 正常 Completed / 去向交给图与 W2a 返工）、`human_decisions` + C13 写连接模型、trace schema v5（CHECK 重建/迁移/只读拒绝）、CLI decide 退出码/`--json`、AC-16 a–g（设计 REV-078 通过；实现 REV-079–083 合并 `main` `2444548`；as-built 回写 REV-084） |

| [`decisions/DR-007-w4a-dynamic-fanout-freeze.md`](decisions/DR-007-w4a-dynamic-fanout-freeze.md) | **W4a 设计冻结 + W4b 实现**：节点级 `foreach`（items_port/key_field/join/max_items/on_item_error）、R17 形状 + R18 区域（新码 `DSL_FOREACH_INVALID`，DSL 18→19）、item 实例 `(node,item)`/join 有序数组/空列表、4 个 `RUN_FOREACH_*`、trace schema v6（`instances.item_index/item_key` + `foreach_expansions`）、SOP v1.2 dynamic-N profile、AC-17 a–j 设计（设计冻结 REV-086；实现 REV-088–093 合入 main `201341b`；as-built 回写 REV-094） |
| [`architecture-v2.md`](architecture-v2.md) | **架构蓝图 v2**：进程模型、NDJSON 协议、可复现分级、代码布局、里程碑 |
| [`design/software-team-sop-v1.md`](design/software-team-sop-v1.md) | **software_team SOP 设计稿 v1.2**：设计角色依赖 DAG + 工作 agent + per-item 评审；**approved-only v1 / rework v1.1 / dynamic-N v1.2 三 profile**（`parse_verdict_rework` + defer 回边 + `max_attempts` 预算；dynamic-N 用 foreach 替代按 N 代码生成）；框架侧 W2a/W2b/W3a 已实现（REV-069/070/068/071、074–077、079–084）；**动态 N 已由 W4a 设计冻结（REV-086；W4b 实现）** |
| [`spec-v1.2/`](spec-v1.2/README.md) | **语义基准**（9 分册）：DSL/状态机/调度/Harness/存储/验收/审计关闭（落地按 DR-001 替换 PyO3 为 worker 子进程） |
| [`analysis/`](analysis/) | 冻结期归档材料：可行性评估、决策记录 01、v1.1 规格审计、spike 报告 + 源码、两份技术调研 |
| [`spec-v1.2/PROVENANCE.md`](spec-v1.2/PROVENANCE.md) | 规格来源、冻结/回填记录、SHA-256 与行数 |
| [`spec-v1.2/REFERENCES.md`](spec-v1.2/REFERENCES.md) | spec 历史引用 → 仓库内路径映射 |
| [`legacy/`](../legacy/README.md) | 旧 Python 实现：**已归档、无维护、不得新增代码**（冻结只读）：`src/`、`config/`、`tests/`、`graph_base_cli.py`、设计文档 |
| [`repo-status.md`](repo-status.md) | 2026-10-28 代码仓现状评估（历史快照；技术路线已由 DR-001 取代） |
| [`development-workflow.md`](development-workflow.md) | 开发流程细则：worktree 并行、四维评审、合并与清理、进度与台账 |
| [`progress.md`](progress.md) | 当前开发进度（保持简洁，≤30 行） |
| [`issues.md`](issues.md) | 遗留问题台账（P0–P3，唯一问题来源） |
| [`reviews/`](reviews/README.md) | 评审记录：索引、模板、REV-000–REV-036、REV-038–094（REV-000 bootstrap；M4 为 REV-017–024；M5 为 REV-025–036/038；M6 为 REV-039–061；W1/v0.2 kickoff 为 REV-062–066；W2a0 为 REV-067、W2a 实现为 REV-068–071；**REV-072 e2e 已通过（合并 main `e2cba52`）；REV-073 W2a 实现回写（已合并）；REV-074 dsl max_fanout、REV-075 kernel max_fanout（含 cli 跨模块第二评审）已合入 main；REV-076 e2e max_fanout（已通过，合并 main `9a91076`）；REV-077 W2b docs（有条件通过 → C1–C4 关闭后通过，已合并）；REV-078 W3a0 docs（通过）；REV-079 trace v5、REV-080 kernel human wait、REV-081 cli decide、REV-082 cli human L2、REV-083 e2e AC-16 均已合并 main；REV-084 W3a as-built docs（通过）；REV-085 W3a 收口进度（通过）；REV-086 W4a 动态 N 设计冻结（通过，已合并）；REV-088–093 W4b 实现（dsl/trace/kernel/fix/cli/e2e，均已合并 main `201341b`）；REV-094 W4b as-built docs（本分支待评审）**；REV-037/051/059 未创建） |
| [`test-baseline.txt`](test-baseline.txt) | 已知失败测试基线（合并门禁依据；只允许缩小） |

流程入口：仓库根 [`AGENTS.md`](../AGENTS.md)（强制约束）与 [`README.md`](../README.md)（快速开始）。
