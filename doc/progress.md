# 开发进度

> 更新规则：任务开始 / 完成 / 阻塞 / 交接时更新；只保留当前状态（历史看 git 与 `doc/reviews/`）；全文件 ≤ 30 行。

**当前阶段**：v0.2（M7）**W4a 已完成并合入 `main`**（`a228b94`；REV-086）：新增 [`DR-007`](decisions/DR-007-w4a-dynamic-fanout-freeze.md)（foreach 语法/R17 形状/R18 区域/`DSL_FOREACH_INVALID`/item 实例/join/trace v6/AC-17 a–j 设计）+ spec 00/01/02/03/04/06/07 冻结写入 + SOP 设计稿 **v1.2**（dynamic-N profile）；W3a 已完成（REV-079–084）。

**进行中任务**
- 无（W4a 收口完成；worktree/分支已清理）。

**下一步**
- **W4b 实现**：dsl（`foreach`/R17/R18）→ kernel+trace（展开/join/trace v6）→ e2e（AC-17 a–j）→ W4b-docs as-built 回写。**W3b durable resume 不做**（用户 2026-11-01 决定）。

**阻塞**
- 无硬阻塞；环境 `CARGO_HOME=<repo>/.cargo-home` + 离线；merge 门禁偶发 ISSUE-091 假失败（重跑通过）。
- 台账：ISSUE-105–112 + 113–128 + 129–140 + **ISSUE-141**（`max_items` 缺省无上限风险）等 open；ISSUE-100/101/104 fixed；ISSUE-062 保持 open（见 `doc/issues.md`）。
