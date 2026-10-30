# legacy · 历史 Python 实现（已归档，只读）

本目录是 DR-001 之前的 Python `graph_base` 实现与测试，**已归档、无维护、不得新增代码**（冻结只读、不参与 CI 与新开发）。

> **归档决策（M5，DR-002 §8）**：保持冻结只读、**不物理删除**（历史审计与语义对照价值）；物理删除/移出仓库属发布决策，交由用户（v1.0 前），M5 不做 git 历史改写。

- 为什么保留：语义参考（门控/返工自环/人工边/并行扇出/进程派发/测试场景）、历史追溯、M1 图 fixture 改写来源。
- 为什么不再维护：新架构为 **Rust 内核 + Python 子进程 worker**（见 `doc/decisions/DR-001-runtime-route.md`）。
- 已知问题（不再修复，见 `doc/issues.md`）：
  - `scripts/zlog/` 资产缺失 → monitor/scheduler/e2e 无法运行（ISSUE-001，wontfix）。
  - `config/software_team_graph.json` 的 gate 引用与定义不一致（ISSUE-002，wontfix）。
- 可运行性：目录移动后**不保证**可直接运行（原实现按仓库根路径与 `src.*` 导入）；如需复跑，请 checkout 历史提交
  `50f05d2`（或更早）后按当时的 README 操作。
- 任何新代码**禁止**放在本目录；旧模块 id（`scheduler`/`state-manager`/`dispatcher`/`monitor`/`orchestrator`/`workers`/`config`）已在 `AGENTS.md` §2 退役；`kernel`/`cli`/`e2e`/`docs`/`process` 保留 id 但路径迁到新目录。
