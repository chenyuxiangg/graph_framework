# 开发进度

> 更新规则：任务开始 / 完成 / 阻塞 / 交接时更新；只保留当前状态（历史看 git 与 `doc/reviews/`）；全文件 ≤ 30 行。

**当前阶段**：v0.2（M7）W2a0 + W2a 已实现完成（docs/dsl/trace/py-worker/kernel+cli 已合入 `main`；e2e 分支 `test/e2e-gate-defer` 的 **REV-072 已通过**、待合并）；W2a-docs 回写（REV-073）有条件通过，S-1..S-4/N1–N8 修复中，AC-14 c/e 待 e2e final（单 item rework profile）校准。

**进行中任务**
1. W2a-docs（REV-073）：S-1..S-4 + N1–N8 修复；c/e 等 e2e final 证据后补终版。
2. e2e 分支：待合并（最终 HEAD 以合并时为准）。

**下一步**
- e2e final 到达 → 补 c/e 校准 + 行数/hash → reviewer-docs 增量复审 → 合并。
- W2b（每节点最大扇出限制，dsl+kernel+e2e+docs）→ W3a0 → W3a → W3b(可选) → W4a → W4b（DR-004 §4）。

**阻塞**
- 无硬阻塞；环境 `CARGO_HOME=<repo>/.cargo-home` + 离线；merge 门禁偶发 ISSUE-091 假失败（重跑通过）。
- 台账：ISSUE-104/100/101 fixed；ISSUE-105–112 open；ISSUE-062 保持 open（见 `doc/issues.md`）。
