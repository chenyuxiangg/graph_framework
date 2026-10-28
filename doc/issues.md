# 遗留问题台账

> 规则（见 `AGENTS.md` §7）：评审无法当场修复的问题、已知缺陷、环境阻塞、设计分歧必须登记；不允许用代码注释/TODO 代替。
> 级别：`P0` 阻塞开发 / `P1` 高 / `P2` 中 / `P3` 低。状态：`open` / `in-progress` / `fixed` / `wontfix`。

| ID | 级别 | 模块 | 问题 | 影响 | 建议 | 状态 | 登记日期 |
|---|---|---|---|---|---|---|---|
| ISSUE-001 | P0 | monitor | `scripts/zlog/` 资产缺失（`monitor/logger.py` 通过 `sys.path` 导入本地 `zlog`，仓库从未包含该目录；PyPI 同名包不兼容） | `Monitor`/`Scheduler` 初始化失败：3 个 scheduler 单测 + 14+ e2e 失败；整个 monitor 能力不可用 | 恢复 zlog 源码与 `scripts/zlog/config/output_config.json`；或决策改用标准库 `logging` 并同步改 `monitor/logger.py` | open | 2026-10-28 |
| ISSUE-002 | P0 | config | `config/software_team_graph.json` 中边引用 `planer_gate`/`archer_gate`/`mde_gate`/`te_gate` 等，但 `gates` 数组定义的是 `check_pm_gate`/`check_se_gate`/`check_mde_gate`/`check_te_gate` 等 | `validate` 报 `E_BAD_GATE_REF`，`test_load_software_team` 失败；software_team 图不可用 | 对照设计文档统一 gate 命名（二选一：改边引用或改 gates 定义），补 `validate` 回归用例 | open | 2026-10-28 |
| ISSUE-003 | P1 | process | 仓库原先无依赖清单；本机未安装 `uv`（CLAUDE.md 要求 uv 管理依赖） | 新人无法按文档一键起环境；CI 无依据 | 已补 `pyproject.toml`（dev 依赖 pytest）；需安装 uv 并提交 `uv.lock` | in-progress | 2026-10-28 |
| ISSUE-004 | P1 | e2e | 测试基线非全绿：全量 187 个 test function，当前 **25 项失败**（4 单测 + 21 e2e，见 `doc/test-baseline.txt`） | 无法用"全绿"作为合并门槛；当前 merge 门槛为"不得新增失败" | 先修 ISSUE-001/002，再逐步清空基线；禁止改测试绕过 | open | 2026-10-28 |
| ISSUE-005 | P2 | process | 无 CI、无分支保护、无自动化校验 | 评审/测试约束只能靠脚本自觉 | 增加 CI（pytest + check.sh）；main 分支保护 | open | 2026-10-28 |
| ISSUE-006 | P2 | docs | `README.md` 原为空；`doc/index.md` 仅 1 条；`doc/graph_base_design.md` 与代码实现的一致性未验证 | 上手成本高，设计/实现易漂移 | 本次已补 README 与文档索引；后续每次模块变更同步设计文档 | in-progress | 2026-10-28 |
| ISSUE-007 | P2 | process | 存在两套设计材料：仓库内 `doc/graph_base_design.md`（Python 实现依据）与外部《详细设计 v1.0/1.1》（Rust/PyO3/SQLite 方案，含 DSL 端口/子图/Agent Harness） | 技术路线不明确会导致重复建设与接口冲突 | 由用户/架构确认以哪套为准；若合并，需先出差异清单与迁移计划（本地评估材料见工作区 `docs/`，未入库） | open | 2026-10-28 |
| ISSUE-008 | P2 | monitor | `monitor/logger.py` 用 `sys.path.insert` 加载本地包，属隐式依赖、无法打包 | 部署/测试环境依赖路径约定，易碎 | 统一为显式包（`src/…` 或 pyproject 依赖），消除 sys.path 注入 | open | 2026-10-28 |
| ISSUE-009 | P3 | e2e | 无覆盖率配置/门槛；187 个 test function 的实际覆盖未知 | 质量不可度量 | 引入 coverage（pytest-cov），定 P2 阶段门槛（如核心 kernel ≥80%） | open | 2026-10-28 |
| ISSUE-010 | P3 | cli | `scripts/graph_base_cli.py` 未作为 pyproject 入口点，只能 `python scripts/...` 调用 | 使用不便、难以在 PATH 中分发 | 增加 `[project.scripts] graph-base = ...`（需把 CLI 移入包或提供 shim） | open | 2026-10-28 |
| ISSUE-011 | P3 | config/workers | `config/index.md` 自述：除 planer/archer 外各节点 `proc` 为空（走 `proc_bypass`），software_team 产物需用户自建 worker，尚未内置 | software_team 图只能跑框架、不能产出真实业务结果 | 明确 worker 交付范围；按模块补充 worker 与单测 | open | 2026-10-28 |
| ISSUE-012 | P1 | process | `merge.sh --dry-run` 会删除作者的 worktree（自验证发现） | 误执行 dry-run 会丢失未合并工作区（可恢复但危险） | 已修复：dry-run 只检查不清理；REV-002 | fixed | 2026-10-28 |
