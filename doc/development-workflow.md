# 开发流程细则（worktree / 评审 / 合并）

> 本文是 [`AGENTS.md`](../AGENTS.md) 的操作手册：把 7 条强制约束展开成可执行步骤。
> 所有命令在仓库根目录执行；脚本位于 `scripts/dev/`。

## 0. 前置条件

```bash
# 身份（仓库级；check.sh 会校验）
git config user.name  chenyuxiangg
git config user.email 1007423165@qq.com

# 依赖与测试（uv + venv，见 CLAUDE.md）
uv sync --extra dev
uv run pytest -q          # 当前有 25 项已知失败，基线见 doc/test-baseline.txt
```

## 1. 任务开始：建 worktree（一 agent 一 worktree）

```bash
bash scripts/dev/worktree.sh new <module> <agent> <slug> [--type feat|fix|test|docs|chore|refactor]
# 等价产物：
#   路径   .worktrees/<module>-<agent>
#   分支   <type>/<module>-<slug>（基于 origin/main）
```

规则（强制）：

- 一个 worktree 只服务一个模块、一个任务；**禁止**两个 agent 共用一个工作目录。
- `module` 取值见 `AGENTS.md` §2 的模块表；`agent` 用简短唯一名（如 `agent-a`、`claude-1`）。
- 分支只基于 `origin/main`；**禁止**从其他特性分支分叉。

## 2. 开发

```bash
cd .worktrees/<module>-<agent>
# 编码 → 写/改单测 → 本地验证
bash scripts/dev/check.sh                 # 身份、冻结文件、禁用文件、py 语法
uv run pytest -q                          # 全量；对照 doc/test-baseline.txt 不得新增失败
git add -A && git commit -m "<type>(<module>): <摘要>"
# 正文追加：
#   Review: REV-<NNN>
#   Agent: <agent>
```

约定：

- 提交信息格式 `<type>(<module>): <摘要>`；一个提交只做一件事。
- 只改本模块文件；跨模块改动需 Lead 指派第二评审人并在评审记录注明。
- 不确定被测函数意图时停下向用户举手，**禁止**猜（CLAUDE.md）。
- **clippy 门禁**：CI 与 `merge.sh` 均执行 `cargo clippy --workspace --all-targets --locked -- -D warnings`；本地提交前应自跑。
- **构建缓存**：多个 worktree **不得**共用同一个 `CARGO_TARGET_DIR` 并发构建（cargo 对 workspace 成员单元不区分 worktree，会串产物导致测试假绿/假红，ISSUE-093）；每个 worktree 用私有 target，或合并前 `cargo clean -p` 全部 workspace 成员。

## 3. 评审（四维度，评审人 ≠ 作者）

1. 作者在 worktree 内提交后，把分支与变更范围交给**独立评审人**（另一个 agent 或 Lead）。
2. 评审人按模板写 `doc/reviews/REV-<NNN>-<module>-<slug>.md`，逐项给出四维结论：

| 维度 | 检查要点 |
|---|---|
| **功能正确性** | 需求/设计是否完整实现；边界与异常路径；是否有真实可复现的验证；是否偷偷改测试 |
| **代码结构合理性** | 模块边界、职责单一、依赖方向、是否重复造轮子、是否侵入他模块、公共接口是否收敛 |
| **逻辑性** | 状态机/分支/循环条件是否自洽；并发与副作用；错误传播；不存在死循环/竞态/吞异常 |
| **可读性** | 命名、类型注解、双引号、函数 ≤ 60 行、注释解释"为什么"、测试可读、无魔术数 |

3. 评审发现按级别记录：`阻塞`（必须改）/ `重要`（应改）/ `建议`（可后置）。
4. 结论三选一：`通过` / `有条件通过`（列出关闭条件）/ `不通过`。
5. **只有 `通过` 能合并**；`有条件通过` 关闭全部条件后追加一段"条件关闭记录"，再合并。
6. 评审记录可提交在**功能分支内**（推荐，随分支一起评审）或 `main` 的 `doc/reviews/`（作为 docs 提交）；`merge.sh` 会两处查找。
7. 评审记录不删除；结论变更用追加修订段。

## 4. 合并与清理（仅允许脚本）

```bash
bash scripts/dev/merge.sh <branch>            # 合并到本地 main
bash scripts/dev/merge.sh <branch> --push     # 合并并推送 origin/main
bash scripts/dev/merge.sh <branch> --dry-run  # 只做检查，不合并
# 本机无 uv/pytest 时：PYTHON_BIN=/path/to/python bash scripts/dev/merge.sh <branch>
# 沙箱/受限环境：export CARGO_HOME=$PWD/.cargo-home（需可写）并配置代理
```

脚本强制顺序：评审记录（结论=通过）→ worktree 干净 → 与 `origin/main` 同步（必要时 rebase）→ 测试无新增失败 → `--ff-only` 合并 → 删除 worktree → 删除分支 →（可选 push）。

合并后：

```bash
# 更新 doc/progress.md（≤30 行）与 doc/issues.md（新发现的问题）
git add doc/progress.md doc/issues.md
git commit -m "docs(process): 更新进度与问题台账"
```

> 进度/台账这类流程文件的提交属于 `docs(process)`，不走模块评审；但不得借此夹带产品代码。

## 5. 进度与问题台账

- **进度**：`doc/progress.md`，四块（当前阶段 / 进行中 / 下一步 / 阻塞），全文件 ≤ 30 行。
- **问题**：`doc/issues.md`，每条 `ISSUE-NNN`，级别 `P0`（阻塞）/`P1`（高）/`P2`（中）/`P3`（低），状态 `open/in-progress/fixed/wontfix`。
- 评审无法当场解决的问题 **必须** 进台账，不允许只写在评审记录或代码 TODO 里。

## 6. 冲突与例外

| 场景 | 处理 |
|---|---|
| 两个 agent 要改同一模块 | 后到者等待前一个合并；或由 Lead 拆分文件级任务并在评审记录注明 |
| 同模块两个 worktree 冲突 | 先合并者优先；后者 `git rebase origin/main` 后重跑测试与评审 |
| 紧急修复（hotfix） | 仍走 worktree + 评审；评审可由 Lead 承担，但**不得**跳过测试与留痕 |
| 环境阻塞（如 ISSUE-001 zlog 缺失） | 登记台账；merge 脚本只要求"不新增失败"，不要求全绿 |
| 用户要求例外 | 在 `doc/issues.md` 记录例外内容与批准人，再执行 |

## 7. 模块与责任人对照（初始化时为空）

| 模块 | 当前负责人 | 进行中任务 | 分支/worktree |
|---|---|---|---|
| — | — | — | — |

> 负责人随任务变化；以 `doc/progress.md` 为准。

## 8. 门禁脚本自身的合并（自举两阶段）

若某分支修改了 `scripts/dev/merge.sh`，合并它时运行的是 **main 上的旧脚本**，可能出现：

1. 旧脚本完成评审检查、测试、ff 合并后，在"删除分支"步骤失败（exit 1），但此时 main 已更新、worktree 已清理、分支保留；
2. 在新 main 上重跑 `merge.sh <branch>` 即完成（幂等：已合并 → 直接走清理/删分支）。

这不是故障，是自举顺序的固有限制；第一次失败需在评审记录与台账中注明。`merge.sh` 的改动必须附
`bash scripts/dev/selftest-merge.sh` 的自测结果（在临时 clone 中验证 dry-run/真实合并/清理）。

本机运行示例：`PYTHON_BIN=/path/to/python bash scripts/dev/selftest-merge.sh`（需 pytest；沙箱需可写 `CARGO_HOME`）。
