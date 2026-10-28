# 遗留问题台账

> 规则（见 `AGENTS.md` §7）：评审无法当场修复的问题、已知缺陷、环境阻塞、设计分歧必须登记；不允许用代码注释/TODO 代替。
> 级别：`P0` 阻塞开发 / `P1` 高 / `P2` 中 / `P3` 低。状态：`open` / `in-progress` / `fixed` / `wontfix`。

| ID | 级别 | 模块 | 问题 | 影响 | 建议 | 状态 | 登记日期 |
|---|---|---|---|---|---|---|---|
| ISSUE-001 | P0 | legacy | `scripts/zlog/` 资产缺失（`monitor/logger.py` 通过 `sys.path` 导入本地 `zlog`；仓库从未包含该目录） | 仅影响 legacy：3 个 scheduler 单测 + 14+ e2e 失败 | **不再修复**：DR-001 后 legacy 冻结只读，新架构（worker SDK）使用标准 `logging` | wontfix | 2026-10-28 |
| ISSUE-002 | P0 | legacy | `config/software_team_graph.json` 边引用 `planer_gate` 等，而 `gates` 定义是 `check_pm_gate` 等 | 仅影响 legacy：`E_BAD_GATE_REF`，software_team 图不可用 | **不直接修复 legacy**：M1 把三张图改写为 DSL v1.2 fixture 时统一命名 | wontfix | 2026-10-28 |
| ISSUE-003 | P1 | process | 仓库原先无依赖清单；本机未安装 `uv`（CLAUDE.md 要求 uv 管理依赖） | 新人无法按文档一键起环境；CI 无依据 | 已补 `pyproject.toml`（dev 依赖 pytest）；需安装 uv 并提交 `uv.lock` | in-progress | 2026-10-28 |
| ISSUE-004 | P1 | e2e | 测试基线非全绿：全量 187 个 test function，当前 **25 项失败**（4 单测 + 21 e2e，见 `doc/test-baseline.txt`） | 无法用"全绿"作为合并门槛；当前 merge 门槛为"不得新增失败" | 先修 ISSUE-001/002，再逐步清空基线；禁止改测试绕过 | open | 2026-10-28 |
| ISSUE-005 | P2 | process | 无 CI、无分支保护、无自动化校验 | 评审/测试约束只能靠脚本自觉 | 增加 CI（pytest + check.sh）；main 分支保护 | open | 2026-10-28 |
| ISSUE-006 | P2 | docs | `README.md` 原为空；`doc/index.md` 仅 1 条；`doc/graph_base_design.md` 与代码实现的一致性未验证 | 上手成本高，设计/实现易漂移 | 本次已补 README 与文档索引；后续每次模块变更同步设计文档 | in-progress | 2026-10-28 |
| ISSUE-007 | P2 | process | 两套设计材料并存（Python 实现 vs Rust/PyO3 详细设计） | 技术路线不清会重复建设 | **已决策**：DR-001 = Rust 内核 + Python 子进程 worker，v1.1 语义为核心；legacy 冻结 | fixed | 2026-10-28 |
| ISSUE-008 | P2 | monitor | `monitor/logger.py` 用 `sys.path.insert` 加载本地包，属隐式依赖、无法打包 | 部署/测试环境依赖路径约定，易碎 | 统一为显式包（`src/…` 或 pyproject 依赖），消除 sys.path 注入 | open | 2026-10-28 |
| ISSUE-009 | P3 | e2e | 无覆盖率配置/门槛；187 个 test function 的实际覆盖未知 | 质量不可度量 | 引入 coverage（pytest-cov），定 P2 阶段门槛（如核心 kernel ≥80%） | open | 2026-10-28 |
| ISSUE-010 | P3 | cli | `scripts/graph_base_cli.py` 未作为 pyproject 入口点，只能 `python scripts/...` 调用 | 使用不便、难以在 PATH 中分发 | 增加 `[project.scripts] graph-base = ...`（需把 CLI 移入包或提供 shim） | open | 2026-10-28 |
| ISSUE-011 | P3 | config/workers | `config/index.md` 自述：除 planer/archer 外各节点 `proc` 为空（走 `proc_bypass`），software_team 产物需用户自建 worker，尚未内置 | software_team 图只能跑框架、不能产出真实业务结果 | 明确 worker 交付范围；按模块补充 worker 与单测 | open | 2026-10-28 |
| ISSUE-012 | P1 | process | `merge.sh --dry-run` 会删除作者的 worktree（自验证发现） | 误执行 dry-run 会丢失未合并工作区（可恢复但危险） | 已修复：dry-run 只检查不清理；REV-002 | fixed | 2026-10-28 |
| ISSUE-013 | P1 | process | legacy Python 实现与新 Rust 代码尚未物理隔离 | 目录/CI 混淆，误改风险 | M0：`legacy/` 归档、排除 CI、README 标注只读 | open | 2026-10-29 |
| ISSUE-014 | P1 | kernel | 自定义 Python 门控方案未定（Rust 内置 op vs 常驻 gate worker） | 影响 DSL 门控能力与性能 | M1 前定稿（DR-001 O-1，默认内置 op） | open | 2026-10-29 |
| ISSUE-015 | P1 | worker | worker 协议 v1 仅有草案，无兼容测试 | 协议演进导致内核/worker 不兼容 | M0 冻结协议文档 + 双方版本校验 + 兼容用例 | open | 2026-10-29 |
| ISSUE-016 | P1 | trace | 可复现 L2 的记录粒度未定（worker output 原文 vs hash） | 影响结果重算能力与存储量 | M2 前定稿（DR-001 O-3，默认 `--record` 存原文+hash） | open | 2026-10-29 |
| ISSUE-017 | P2 | worker | worker 每节点冷启动约 50–100ms，未实测、未池化 | 高频小节点吞吐受限 | M3 基准后决定是否池化（O-2） | open | 2026-10-29 |
| ISSUE-018 | P2 | process | 双语言工具链（cargo/rustfmt/clippy + uv/pytest）尚无 CI | 质量门禁不可自动化 | M0 建 CI 并同步 AGENTS 模块表 | open | 2026-10-29 |
| ISSUE-019 | P1 | process | `CLAUDE.md` 冻结且写明 Python 技术栈，与 Rust 宿主冲突（文件禁止修改） | 规范适用边界不清，Rust 代码可能被误按 Python 规范检查 | 解释为 Python 规范；Rust 用 rustfmt/clippy 等价约束；M0 CI 分别检查；需用户在 M0 确认该解释 | open | 2026-10-29 |
