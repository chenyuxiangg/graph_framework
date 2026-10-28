# 代码仓现状评估（2026-10-28）

> 快照：`bcf8621`（`main`，共 2 个提交）；评估人：Lead（agent）；评估方式：静态阅读 + 实跑测试 + 缺陷定位。
> 详细问题见 [`issues.md`](issues.md)；基线见 [`test-baseline.txt`](test-baseline.txt)。

## 1. 结论速览

| 维度 | 现状 | 判定 |
|---|---|---|
| 架构分层 | kernel / components / worker / config 分层清晰，依赖方向基本单向 | ✅ 良好 |
| 代码规模 | 10,172 行 Python（src+scripts+tests），187 个 test function | ✅ 有实质实现 |
| 单元测试 | `src/**`：166 个，**162 通过 / 4 失败** | ⚠️ 可运行，少数阻塞 |
| E2E 测试 | `tests/**`：21 个，**0 通过 / 21 失败** | ❌ 阻塞（根因明确） |
| 可复现性 | 无 `pyproject.toml`/锁文件；`zlog` 本地资产缺失；README 为空 | ❌ 新人无法一键跑通 |
| 流程与质量门禁 | 无 CI、无分支保护、无评审留痕 | ❌ 本次已补规则与脚本 |
| 设计一致性 | 仓库实现依据 `doc/graph_base_design.md`；另存在 Rust 方案《详细设计 v1.1》，两者 DSL/执行模型不同 | ⚠️ 需确认技术路线（ISSUE-007） |

**一句话**：内核骨架与测试体系已具规模，但当前**不可全绿运行**；两个 P0 缺陷（`zlog` 资产缺失、图配置 gate 引用错）导致 monitor/scheduler/e2e 全部受阻。修掉这两项后即可恢复到一个可信基线。

## 2. 仓库结构（快照）

| 路径 | 文件数 | 行数 | 职责 |
|---|---:|---:|---|
| `src/kernel/` | 23 | 3,607 | 图、节点、边、门、端口、注册表、状态、装载、校验、遍历 |
| `src/components/scheduler/` | 7 | 1,915 | tick、就绪判定、派发 |
| `src/components/state_manager/` | 8 | 780 | 状态 schema、端口、终态判定、post_handle |
| `src/components/dispatcher/` | 4 | 297 | 进程派发 |
| `src/components/monitor/` | 6 | 370 | 事件、日志、进程回收 |
| `src/components/orchestrator.py` | 1 | 90 | Graph + Scheduler + StateManager + Monitor + Dispatcher 组装 |
| `src/worker/` | 17 | 1,189 | 3 组示例 worker（software_team / test_for_complex / test_for_e2e） |
| `scripts/graph_base_cli.py` | 1 | 211 | CLI（tick/create/list/show/record-completion/trigger-edge/rollback/validate） |
| `tests/` | 4 | 1,651 | 端到端测试 |
| `config/` | 4 | — | 3 张示例图 + 配置索引 |
| `doc/` | 2 | 412+ | 设计文档与索引 |

依赖方向（静态阅读结论）：`kernel` 不依赖 components；`components/*` 依赖 `kernel`；`worker` 依赖 `src/worker/_common`；`tests` 依赖 CLI/Orchestrator。未发现 kernel 反向依赖 components 的迹象。

## 3. 运行环境与可复现性

| 项 | 现状 |
|---|---|
| Python | 解释器 3.13.0（Homebrew）；`CLAUDE.md` 要求 3.10+ |
| 依赖管理 | **无 `pyproject.toml`、无锁文件、无 requirements**（本次已补 pyproject，uv 仍待安装） |
| `uv` | **未安装**（`CLAUDE.md` 要求用 uv；安装见 ISSUE-003） |
| pytest | 环境里未安装；本次评估在 `/tmp` 临时 venv 安装 pytest 9.2.0 后执行（未写入仓库） |
| 测试入口 | 需要 `PYTHONPATH=.`；本次已通过 `pyproject.toml` 的 `pythonpath=["."]` 固定 |
| CLI | `python scripts/graph_base_cli.py --help` 可用（纯标准库，无 zlog 依赖） |

评估实际执行：

```bash
PYTHONPATH=. python -m pytest -q --tb=no     # 187 collected
# 结果：25 failed, 162 passed（失败清单见 doc/test-baseline.txt）
```

## 4. 缺陷根因（已定位）

### 4.1 P0｜`scripts/zlog/` 缺失（ISSUE-001）

- 现象：`ModuleNotFoundError: No module named 'zlog'`，集中在 monitor/scheduler/e2e。
- 根因：`src/components/monitor/logger.py` 把 `scripts/` 注入 `sys.path` 后 `from zlog import init_logging/get_logger`，但仓库中 **`scripts/zlog/` 从未提交**（`git log --all -- scripts/zlog` 为空）。
- 影响面：3 个 scheduler 单测 + 14 个 e2e（叠加 4.2 后共 21 个 e2e 全挂）。
- 说明：PyPI 上的 `zlog` 是无关的 C 日志库，**不能** `pip install zlog` 糊过去。

### 4.2 P0｜图配置 gate 引用不一致（ISSUE-002）

- 现象：`graph_base_cli validate config/software_team_graph.json` → `E_BAD_GATE_REF`（示例：edge `planer_loop` 引用 gate `planer_gate` 不存在）。
- 根因：配置中 `gates` 定义为 `check_pm_gate/check_se_gate/check_mde_gate/check_te_gate/...`，而边引用为 `planer_gate/archer_gate/mde_gate/te_gate/...`；两套命名未对齐（疑似重构未同步）。
- 影响面：`test_load_software_team` + 依赖该图的 e2e；`software_team` 图完全不可用。

### 4.3 其他

- 依赖声明/README/索引缺失（ISSUE-003/006）；无 CI（ISSUE-005）；monitor 隐式 `sys.path` 依赖（ISSUE-008）；无覆盖率门槛（ISSUE-009）；CLI 未打包（ISSUE-010）；software_team 业务 worker 未内置（ISSUE-011）。
- 设计路线分歧（ISSUE-007）：仓库是 Python `graph_base` 架构（config JSON + gate op + 进程派发），而外部《详细设计 v1.1》是 Rust + PyO3 + SQLite + 端口/子图/Agent Harness 方案。两者的 DSL 与执行模型不同，**必须先确认以哪套为准**，否则并行开发会分叉。

## 5. 建议的推进顺序（按依赖）

| 顺序 | 事项 | 模块 | 验收 |
|---|---|---|---|
| 1 | 恢复 `scripts/zlog/`（或决策改标准 logging） | monitor | `pytest src/components/monitor src/components/scheduler` 通过；e2e 失败数下降 |
| 2 | 修正 `software_team_graph.json` gate 引用 | config | `validate` 通过；`test_load_software_team` 通过 |
| 3 | 安装 uv、`uv sync --extra dev`、提交 `uv.lock` | process | README 快速开始可一键复现 |
| 4 | 清空基线：逐条修 25 个失败并缩小 `test-baseline.txt` | kernel/scheduler/e2e | 基线归零，合并门槛升级为"全绿" |
| 5 | 补 CI（pytest + `scripts/dev/check.sh`）与 main 保护 | process | PR/推送自动校验 |
| 6 | 确认技术路线（ISSUE-007） | 架构 | 出一页决策记录，锁定后续设计/实现方向 |

## 6. 复算方式

```bash
git clone git@github.com:chenyuxiangg/graph_framework.git
cd graph_framework
uv sync --extra dev        # 或临时 venv
uv run pytest -q           # 对照 doc/test-baseline.txt
uv run python scripts/graph_base_cli.py validate config/software_team_graph.json
```
