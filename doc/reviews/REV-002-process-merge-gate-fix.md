# REV-002 · process · 合并门禁修复（bootstrap 后续）

> 背景：REV-000 引入的 `scripts/dev/merge.sh` 在自验证时发现 **`--dry-run` 会删除作者的 worktree**（REV-000 未覆盖该路径）。
> 本记录为 bootstrap 阶段的流程脚本修正，仍适用 `AGENTS.md` §9 的例外（仅文档/工具链，不含产品代码）。

- **分支**：`main`（bootstrap 例外，修复门禁脚本自身缺陷）
- **作者**：lead（agent）
- **评审人**：lead（自审；仅工具链，bootstrap 例外）
- **变更范围**：`140f870..HEAD`
- **评审时间**：2026-10-28
- **关联**：ISSUE-012

## 1. 结论

**结论：通过**

## 2. 四维评审

| 维度 | 结论 | 依据 |
|---|---|---|
| 功能正确性 | ✅ | dry-run 改为只检查、不清理作者 worktree；临时检查目录仍由 `trap` 清理，不会残留 |
| 代码结构合理性 | ✅ | 只改 `merge.sh` 的检查顺序与控制流；评审记录查找扩为 `main + 分支` 两处，符合"评审人在分支上写记录"的自然流程 |
| 逻辑性 | ✅ | 检查顺序：main 干净 → 分支存在 → 定位 worktree → 评审 → 干净 → rebase → 测试基线 → dry-run 退出 / ff-only 合并 → 清理 → 可选 push；每个失败点都有明确 `MERGE BLOCKED` |
| 可读性 | ✅ | 注释与错误信息明确；新增 `PYTHON_BIN` 说明（无 uv 环境） |

## 3. 发现清单

| # | 级别 | 位置 | 问题 | 建议 | 状态 |
|---|---|---|---|---|---|
| 1 | 阻塞 | `merge.sh` dry-run 分支 | dry-run 删除作者 worktree（破坏性） | 移除该清理；仅保留 trap 对临时目录的清理 | fixed（本次） |
| 2 | 重要 | `merge.sh` 评审查找 | 只在 main 的 `doc/reviews` 查找，评审记录在功能分支时会被误判缺失 | 同时在 main 与分支 worktree 查找 | fixed（本次） |
| 3 | 建议 | 流程文档 | 未说明评审记录提交位置 | 在 `doc/development-workflow.md` §3 补充 | fixed（本次） |

## 4. 验证证据

```bash
bash -n scripts/dev/merge.sh
# 冒烟：分支 + 分支内 REV-099（通过）→ dry-run
PYTHON_BIN=/tmp/gf_venv/bin/python bash scripts/dev/merge.sh feat/docs-gate-check --dry-run
# → 评审通过 / 测试 25 失败 = 基线 25 / dry-run 结束；作者 worktree 仍存在
git worktree list   # 冒烟 worktree 未被误删
```

- 测试基线：`25 failed / 162 passed`，与 `doc/test-baseline.txt` 一致，无新增失败。

## 5. 遗留问题

- ISSUE-012 已修复并登记。
- 无新增遗留问题。

## 6. 条件关闭记录

不适用（结论为"通过"）。
