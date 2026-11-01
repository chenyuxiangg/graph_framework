# 开发进度

> 更新规则：任务开始 / 完成 / 阻塞 / 交接时更新；只保留当前状态（历史看 git 与 `doc/reviews/`）；全文件 ≤ 30 行。

**当前阶段**：v0.2（M7）**W4a 动态 N/foreach 设计冻结（本分支，REV-086）**；W3a 已全部合入 `main`（`e3b7a4f`；REV-079..084）：trace schema v5 + `human_decisions`（REV-079）、kernel `WaitingHuman`/`builtin.human`/`HumanDecision`（REV-080）、cli `zgraph decide` + decision consumer（REV-081）、cli human L2 记录/replay 适配（REV-082）、e2e AC-16 a–g 真链路（REV-083）、as-built 回写（REV-084）。无 human 图行为/hash/trace 不变。

**进行中任务**
- docs `docs/docs-dynamic-fanout-design`（REV-086，作者 docs-dev）：DR-007 + spec 02/01/03/06/07 冻结写入 + SOP v1.2（dynamic-N profile）+ AC-17 a–i 设计 + PROVENANCE/DR-004/progress/index；待独立评审。

**下一步**
- W4b：按 DR-007 §14 顺序实现 dsl（R17/R18）→ kernel+trace（v6）→ cli（如需）→ e2e AC-17；W4b-docs 回写 as-built。**W3b durable resume 不做**（用户 2026-11-01 决定）。

**阻塞**
- 无硬阻塞；环境 `CARGO_HOME=<repo>/.cargo-home` + 离线；merge 门禁偶发 ISSUE-091 假失败（重跑通过）。
- 台账：ISSUE-100/101/104 fixed；ISSUE-105–112 + ISSUE-113–128 + 本轮 ISSUE-129–140（含 ISSUE-137 P2 trace FK flake 观察，非 W3a 引入）open；ISSUE-062 保持 open（见 `doc/issues.md`）。
