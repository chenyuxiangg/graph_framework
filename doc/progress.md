# 开发进度

> 更新规则：任务开始 / 完成 / 阻塞 / 交接时更新；只保留当前状态（历史看 git 与 `doc/reviews/`）；全文件 ≤ 30 行。

**当前阶段**：v0.2（M7）**W2b 实现已合并 `main`（`a4cb1b1`）**：dsl（REV-074，`220f53c` 时点）+ kernel/cli（REV-075，含 cli 机械 arm 第二评审）；e2e（REV-076）与 W2b-docs（REV-077）进行中。默认无 `max_fanout` 行为/hash 不变；下一步 **W3a0**。

**进行中任务**
- e2e `test/e2e-max-fanout`（REV-076，作者 e2e-dev）：静态反例 CLI / 正例真链路 / 运行期 test-hooks / v0.1.0 golden；等合并后 docs 补 AC-15 实测计数。
- docs `docs/docs-fanout-spec`（REV-077，作者 docs-dev，worktree `.worktrees/docs-docs-dev`）：spec 00/02/03/06 稳定部分已回写（R16、`NODE_FANOUT_EXCEEDED`、03 §3.4.4）；07 AC-15 + PROVENANCE + 索引/README 待 e2e 合并后收口。

**下一步**
- W2b 全部合并 → W3a0（人工评审设计冻结：decide/resume + `human_decisions`）→ W3a → W3b(可选) → W4a → W4b（DR-004 §4）。

**阻塞**
- W2b-docs 的 07 AC-15/PROVENANCE 收口依赖 e2e REV-076 合并（Lead 通知后 rebase）；无其它硬阻塞。
- 环境 `CARGO_HOME=<repo>/.cargo-home` + 离线；merge 门禁偶发 ISSUE-091 假失败（重跑通过）。
- 台账：ISSUE-100/101/104 fixed；ISSUE-105–112 + 新增 ISSUE-113–120（REV-074/075 P3 建议）open；ISSUE-062 保持 open（见 `doc/issues.md`）。
