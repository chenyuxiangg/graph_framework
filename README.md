# 任务流图模型框架（graph_base）

用有向（有环 / 无环）图描述一个任务的带状态 SOP，通过调度、状态管理与监控实现任务自动闭环的框架。

- 语言：Python 3.10+；测试：pytest；依赖管理：`uv`（见 `CLAUDE.md`）
- 设计文档：[`doc/graph_base_design.md`](doc/graph_base_design.md)
- 文档索引：[`doc/index.md`](doc/index.md)

## 快速开始

```bash
# 1) 安装 uv（一次性；禁止用 pip 直接安装项目依赖）
brew install uv            # 或见 https://docs.astral.sh/uv/

# 2) 创建 venv 并安装开发依赖
uv sync --extra dev

# 3) 跑测试
uv run pytest -q
```

> 当前测试并非全绿：已知失败 25 项（4 单测 + 21 e2e），根因是 `scripts/zlog/` 资产缺失与
> `config/software_team_graph.json` 的 gate 引用不一致。基线见
> [`doc/test-baseline.txt`](doc/test-baseline.txt)，问题见 [`doc/issues.md`](doc/issues.md)，
> 现状评估见 [`doc/repo-status.md`](doc/repo-status.md)。

## 目录结构

| 路径 | 说明 |
|---|---|
| `src/kernel/` | 图模型内核：图/节点/边/门/端口/注册表/状态/装载/校验/遍历 |
| `src/components/scheduler/` | 调度器：tick、就绪判定、派发 |
| `src/components/state_manager/` | 状态管理：schema、端口、终态判定、post-handle |
| `src/components/dispatcher/` | 派发器：进程派发与子进程管理 |
| `src/components/monitor/` | 监控：事件、日志（依赖缺失的 `scripts/zlog/`）、进程回收 |
| `src/components/orchestrator.py` | 顶层组装：create / tick / run |
| `src/worker/` | 各图配置对应的 worker（按 `config/{name}_graph.json` 约定） |
| `config/` | 图配置与索引（文件名 / `name` 字段 / worker 包三者同名） |
| `scripts/graph_base_cli.py` | CLI：tick / create / list / show / trigger-edge / rollback / validate |
| `tests/` | 端到端测试 |

## 开发流程（强制）

本仓库按模块分工、worktree 并行开发、评审后合并。**开始开发前必须先读 [`AGENTS.md`](AGENTS.md)**
（agent 强制约束）与 [`doc/development-workflow.md`](doc/development-workflow.md)（流程细则）。

- 并行开发：每个 agent 一个独立 worktree（`.worktrees/<module>-<agent>`），禁止多人共用工作区
- 评审：四维度（功能正确性 / 结构合理性 / 逻辑性 / 可读性），评审结论留痕 `doc/reviews/`
- 合并：仅允许通过 `scripts/dev/merge.sh` 合回 `main`，合并后自动清理 worktree
- 进度：`doc/progress.md`（保持简洁）；遗留问题：`doc/issues.md`
