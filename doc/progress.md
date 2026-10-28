# 开发进度

> 更新规则：任务开始 / 完成 / 阻塞 / 交接时更新；只保留当前状态（历史看 git 与 `doc/reviews/`）；全文件 ≤ 30 行。

**当前阶段**：阶段 0 — 开发流程与工具链就绪（bootstrap）

| 模块 | 状态 | 当前任务 | 负责 | 分支 / worktree | 更新 |
|---|---|---|---|---|---|
| process | done | worktree / 评审 / 合并脚本、流程文档、测试基线与台账 | lead | main（bootstrap 例外 REV-000） | 2026-10-28 |
| kernel | blocked | 等待 ISSUE-002 修复后开始模块任务 | 待定 | — | 2026-10-28 |
| monitor | blocked | 等待 ISSUE-001（zlog 资产）恢复 | 待定 | — | 2026-10-28 |
| scheduler | blocked | 依赖 ISSUE-001 | 待定 | — | 2026-10-28 |
| e2e | blocked | 依赖 ISSUE-001 / ISSUE-002 | 待定 | — | 2026-10-28 |

**下一步**
1. 安装 uv 并 `uv sync --extra dev`（ISSUE-003）。
2. 恢复或替换 `scripts/zlog/` 资产（ISSUE-001）。
3. 修正 `config/software_team_graph.json` 的 gate 引用（ISSUE-002）。
4. 按模块建 worktree，优先 kernel / monitor。

**阻塞**
- ISSUE-001（P0）：`scripts/zlog` 缺失 → monitor / scheduler / e2e 无法运行。
- ISSUE-002（P0）：`software_team_graph.json` gate 名与 `gates` 定义不一致。
