# 开发进度

> 更新规则：任务开始 / 完成 / 阻塞 / 交接时更新；只保留当前状态（历史看 git 与 `doc/reviews/`）；全文件 ≤ 30 行。

**当前阶段**：v0.2（M7）W2a0 + W2a 已实现完成（docs/dsl/trace/py-worker/kernel+cli 已合入 `main`；e2e 已合并 `main` `e2cba52`，**REV-072 通过**）；W2a-docs 回写 **REV-073 通过**（S-1..S-4/N1–N8 关闭、rebase 收口）；下一步 **W2b**。

**进行中任务**
1. W2a-docs 分支 `docs/docs-gate-defer-spec`（HEAD 为收口 commit）：等 reviewer-docs 最终确认 → `merge.sh`。

**下一步**
- 合并 W2a-docs（REV-073）→ W2b（每节点最大扇出限制，dsl+kernel+e2e+docs）→ W3a0 → W3a → W3b(可选) → W4a → W4b（DR-004 §4）。

**阻塞**
- 无硬阻塞；环境 `CARGO_HOME=<repo>/.cargo-home` + 离线；merge 门禁偶发 ISSUE-091 假失败（重跑通过）。
- 台账：ISSUE-104/100/101 fixed；ISSUE-105–112 open（112 = full rework 运行期未覆盖，保持 open）；ISSUE-062 保持 open（见 `doc/issues.md`）。
