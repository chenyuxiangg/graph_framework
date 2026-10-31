# 开发进度

> 更新规则：任务开始 / 完成 / 阻塞 / 交接时更新；只保留当前状态（历史看 git 与 `doc/reviews/`）；全文件 ≤ 30 行。

**当前阶段**：v0.2（M7）**W3a 已完成并合入 `main`**（HEAD `2444548`）：trace schema v5 + `human_decisions`（REV-079）、kernel `WaitingHuman`/`builtin.human`/`HumanDecision`（REV-080）、cli `zgraph decide` + decision consumer（REV-081）、cli human L2 记录/replay 适配（REV-082）、e2e AC-16 a–g 真链路（REV-083）；W3a-docs as-built 回写（REV-084）本分支待评审。无 human 图行为/hash/trace 不变。

**进行中任务**
- docs `docs/docs-human-spec`（REV-084，作者 docs-dev，worktree `.worktrees/docs-docs-dev`）：spec 00/01/03/04/06/07/08 as-built 回写 + AC-16 实测（21 用例）+ DR-006 §15 执行记录 + PROVENANCE 第 22 条 + 台账/索引/进度；待独立评审。

**下一步**
- W4a（动态 N）→ W4b；**W3b durable resume 可选，需用户确认**（DR-004 §4/§9）；无阻塞则从 W4a 开始。

**阻塞**
- 无硬阻塞；环境 `CARGO_HOME=<repo>/.cargo-home` + 离线；merge 门禁偶发 ISSUE-091 假失败（重跑通过）。
- 台账：ISSUE-100/101/104 fixed；ISSUE-105–112 + ISSUE-113–128 + 本轮 ISSUE-129–140（含 ISSUE-137 P2 trace FK flake 观察，非 W3a 引入）open；ISSUE-062 保持 open（见 `doc/issues.md`）。
