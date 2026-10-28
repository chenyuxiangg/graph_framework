# REV-000 · process · 开发流程与工具链初始化（bootstrap）

> 本记录对应 `AGENTS.md` §9 的**初始化例外**：首次在 `main` 直接提交，仅含文档/工具链，不含产品代码。
> 此后所有合并必须由独立评审人对代码变更出具记录。

- **分支**：`main`（bootstrap 例外）
- **作者**：lead（agent）
- **评审人**：lead（自审；**仅因本笔不含产品代码**，bootstrap 例外；后续禁止自评自过）
- **变更范围**：`bcf8621..HEAD`（本提交）
- **评审时间**：2026-10-28
- **关联**：AGENTS.md、doc/development-workflow.md、doc/repo-status.md、doc/issues.md、doc/test-baseline.txt、pyproject.toml、README.md、scripts/dev/**

## 1. 结论

**结论：通过**

## 2. 四维评审

| 维度 | 结论 | 依据 |
|---|---|---|
| 功能正确性 | ✅ | 流程约束与用户 7 条要求逐条对应（AGENTS.md §1–§7）；脚本 `worktree.sh` / `merge.sh` / `check.sh` 已实测（新建/清理 worktree、dry-run 合并、身份与禁用文件校验） |
| 代码结构合理性 | ✅ | 约束、流程文档、脚本、台账职责分离；脚本位于 `scripts/dev/`，与产品代码隔离；模块表与仓库现有目录一一对应 |
| 逻辑性 | ✅ | 合并门禁顺序闭合：评审记录 → 干净工作区 → 同步 main → 测试无新增失败 → ff-only → 清理；基线允许缩小、禁止放大；路径均做存在性/分支名校验 |
| 可读性 | ✅ | 全中文、表格化；每条约束可执行；脚本 `set -euo pipefail` + 明确报错信息 |

## 3. 发现清单

| # | 级别 | 位置 | 问题 | 建议 | 状态 |
|---|---|---|---|---|---|
| 1 | 建议 | `doc/progress.md` | 模块负责人暂为"待定" | 首个模块任务启动时补负责人 | open |
| 2 | 建议 | `scripts/dev/merge.sh` | 测试需本地 pytest/uv 可用；环境缺失时报错退出 | 环境就绪后补 CI（ISSUE-005） | open |

> 无阻塞项、无重要项。

## 4. 验证证据

```bash
# 脚本语法与行为
bash -n scripts/dev/worktree.sh && bash -n scripts/dev/merge.sh && bash -n scripts/dev/check.sh
bash scripts/dev/worktree.sh new kernel agent-test smoke --dry-run   # 仅打印计划
bash scripts/dev/check.sh                                            # 身份/禁用文件/语法
# 测试基线（评估环境：/tmp 临时 venv）
PYTHONPATH=. python -m pytest -q --tb=no    # 25 failed, 162 passed → 写入 doc/test-baseline.txt
```

- 工作区清理后无新增文件被误纳入（`.worktrees/`、`docs/`、`_audit/`、`_research/`、`_spike/` 已 ignore）。
- 身份：`user.name=chenyuxiangg`、`user.email=1007423165@qq.com`（`check.sh` 校验通过）。

## 5. 遗留问题

- ISSUE-001（P0）`scripts/zlog/` 缺失 → monitor/scheduler/e2e 阻塞
- ISSUE-002（P0）`software_team_graph.json` gate 引用不一致
- ISSUE-003~011 见 `doc/issues.md`

## 6. 条件关闭记录

不适用（结论为"通过"）。
