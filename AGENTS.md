# AGENTS.md · agent 强制约束（graph_base）

> **适用对象**：所有在本仓库工作的 agent（含 Lead、子代理、外部接入的编码代理）。
> **效力**：本文件是**强制性流程约束**。凡标注 **MUST / MUST NOT** 的条目，违反即视为交付不通过。
> **优先级**：用户直接指令 > `CLAUDE.md`（编码规范，**禁止修改**）> 本文件（开发流程）。
> **冲突处理**：流程与 `CLAUDE.md` 冲突时，编码规范以 `CLAUDE.md` 为准，流程以本文件为准；两者与用户指令冲突时以用户指令为准，并在 `doc/issues.md` 登记。
> **首次生效**：bootstrap 提交（见 §9 例外）。

---

## 1. 仓库、身份与提交（对应要求 4）

| 项 | 强制规定 |
|---|---|
| 远程仓库 | `git@github.com:chenyuxiangg/graph_framework.git`（**只用 SSH**，禁止 HTTPS 地址） |
| 提交作者 | `user.name=chenyuxiangg`、`user.email=1007423165@qq.com`（仓库级配置；`scripts/dev/check.sh` 会校验） |
| 分支模型 | `main` 为唯一长期分支；开发一律在 `feat|fix|test|docs|chore/<module>-<slug>` 上进行 |
| 直接提交 main | **MUST NOT**（唯一例外：本仓库初始化提交，见 §9） |
| force push | **MUST NOT** 对 `main` 强推；任何分支强推需用户在评审记录中书面同意 |
| 提交信息 | `MUST` 采用 `<type>(<module>): <摘要>`；正文必须含 `Review: REV-<NNN>` 与 `Agent: <agent 名>` |
| 提交粒度 | 一个提交只做一件事；禁止把无关模块改动混入 |

`type` 取值：`feat` / `fix` / `refactor` / `test` / `docs` / `chore`。

## 2. 模块分工与 worktree（对应要求 1）

**MUST**：并行开发的每个 agent 使用**独立 worktree**；一个 worktree 同时只服务一个模块、一个任务。

| 模块 id | 负责路径（互不重叠） |
|---|---|
| `kernel` | `src/kernel/**` |
| `scheduler` | `src/components/scheduler/**` |
| `state-manager` | `src/components/state_manager/**` |
| `dispatcher` | `src/components/dispatcher/**` |
| `monitor` | `src/components/monitor/**`（`scripts/zlog/**` 缺失，见 ISSUE-001） |
| `orchestrator` | `src/components/orchestrator.py` |
| `workers` | `src/worker/**` |
| `cli` | `scripts/graph_base_cli.py` |
| `config` | `config/**` |
| `e2e` | `tests/**` |
| `docs` | `doc/**`（不含 `doc/progress.md`、`doc/issues.md` 的并发写） |
| `process` | `AGENTS.md`、`README.md`、`pyproject.toml`、`scripts/dev/**` |

- worktree 路径 **MUST** 为 `.worktrees/<module>-<agent>`（已在 `.gitignore` 中忽略）。
- 分支名 **MUST** 为 `<type>/<module>-<slug>`（slug 为小写短横线）。
- 一个 agent **MUST NOT** 同时修改两个模块；确需跨模块改动时，由 Lead 在评审记录中写明并指定第二评审人。
- 同一模块同一时间只允许一个写入者；第二个 agent 要改同模块必须等前一个合并。
- 冻结文件：`CLAUDE.md` 任何人 **MUST NOT** 修改；`doc/progress.md` 与 `doc/issues.md` 只允许追加/更新本任务相关行（避免并发冲突）。

标准操作：

```bash
bash scripts/dev/worktree.sh new kernel agent-a fix-loader-edge   # 建 worktree + 分支
bash scripts/dev/worktree.sh list                                # 查看
bash scripts/dev/worktree.sh remove kernel-agent-a               # 清理
```

## 3. 开发后必须评审（对应要求 2）

**MUST**：任何代码改动合并前必须经过**至少一次独立评审**；评审人 **MUST NOT** 是作者本人。

- 评审维度（四维，缺一不可）：**功能正确性、代码结构合理性、逻辑性、可读性**。
- 评审记录 **MUST** 写入 `doc/reviews/REV-<NNN>-<module>-<slug>.md`（模板见 `doc/reviews/TEMPLATE.md`）。
- 结论 **MUST** 明确为 `通过` / `有条件通过` / `不通过`；`有条件通过` 必须先关闭全部条件才能合并。
- 评审记录 **MUST** 包含：变更范围、四维逐项结论、发现清单（级别 + 位置 + 问题 + 建议）、验证命令与结果、遗留问题。
- 只有 `结论：通过` 的分支才允许执行合并脚本（脚本会强制校验）。
- 单 agent 会话中若无法安排独立评审人，**MUST** 停下来向用户举手，**MUST NOT** 自评自过。

## 4. 合回 main 与 worktree 清理（对应要求 3）

**MUST** 通过 `scripts/dev/merge.sh <branch> [--push]` 合并；**MUST NOT** 手工 `git merge` 绕过校验。

合并脚本按顺序强制检查：

1. 评审记录存在且 `结论：通过`；
2. worktree 干净、分支基于最新 `origin/main`；
3. 全量测试**不得新增失败**（与 `doc/test-baseline.txt` 比对；基线只允许缩小）；
4. `--ff-only` 合入 `main`；
5. 删除对应 worktree 与分支（**合并后必须清理**）；
6. 仅在显式 `--push` 时推送 `main`。

合并后 **MUST** 更新 `doc/progress.md`（若脚本未代做），并把新发现的问题登记 `doc/issues.md`。

## 5. 进度跟踪（对应要求 5）

- **唯一进度文件**：`doc/progress.md`。
- **MUST 保持简洁**：只保留「当前阶段 / 进行中任务 / 下一步 / 阻塞」四块，全文件 **≤ 30 行**；不写历史（历史看 git 与 `doc/reviews/`）。
- 开始任务、完成任务、遇到阻塞、交接工作时 **MUST** 更新对应行（含日期与分支）。

## 6. 评审留痕（对应要求 6）

- 所有评审记录 **MUST** 永久保存在 `doc/reviews/`，不得删除（撤销结论用追加修订段）。
- `merge.sh` 以评审记录为合并前置条件，因此「无评审 = 无法合并」是硬约束。

## 7. 遗留问题记录（对应要求 7）

- **唯一问题台账**：`doc/issues.md`（表格：`ISSUE-NNN / 级别 / 模块 / 问题 / 影响 / 建议 / 状态 / 登记日期`）。
- 评审中无法当场修复的问题、已知缺陷、环境阻塞、设计分歧，**MUST** 登记；**MUST NOT** 用注释/TODO 代替台账。
- 修复后 **MUST** 更新状态为 `fixed` 并注明修复提交或评审编号。

## 8. 红线（违反即返工）

1. **MUST NOT** 修改 `CLAUDE.md`。
2. **MUST NOT** 为通过测试而改测试、跳过测试、伪造结果（`CLAUDE.md` 明令禁止）。
3. **MUST NOT** 猜测被测函数意图；不确定时向用户举手。
4. **MUST NOT** 提交 `.venv/`、`__pycache__/`、`.worktrees/`、`logs/`、`*.db`、密钥/凭据。
5. **MUST NOT** 在未评审、未跑测试的情况下合并。
6. **MUST NOT** 直接向 `main` 提交代码或强推。
7. 所有 Python 代码 **MUST** 遵守 `CLAUDE.md`：类型注解、双引号、函数 ≤ 60 行、snake_case、常量全大写、每个函数有单测。

## 9. 初始化例外（仅一次）

本仓库第一笔流程提交（新增 `AGENTS.md`、`doc/development-workflow.md`、`doc/repo-status.md`、`doc/progress.md`、`doc/issues.md`、`pyproject.toml`、`README.md`、`scripts/dev/**`、`doc/reviews/REV-000-*`）在 `main` 上直接提交，理由：此时尚无评审流程可用。
例外条件：只含流程/文档/工具链，**不含产品代码**；评审记录 `REV-000` 需注明 bootstrap 性质。此后一律走 §1–§4。

## 10. 交付前自检（每次任务结束前逐项勾选）

- [ ] 只改了本模块文件（`git status` 确认）
- [ ] `bash scripts/dev/check.sh` 通过（身份/冻结文件/禁用文件/语法）
- [ ] 相关单测通过；全量测试无**新增**失败（对照 `doc/test-baseline.txt`）
- [ ] 评审记录已写且结论为 `通过`，评审人 ≠ 作者
- [ ] 提交信息含 `Review: REV-<NNN>` 与 `Agent: <name>`
- [ ] `doc/progress.md` 已更新（≤ 30 行）
- [ ] 新问题已登记 `doc/issues.md`
- [ ] 合并后 worktree 与分支已清理
