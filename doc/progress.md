# 开发进度

> 更新规则：任务开始 / 完成 / 阻塞 / 交接时更新；只保留当前状态（历史看 git 与 `doc/reviews/`）；全文件 ≤ 30 行。

**当前阶段**：v0.2（M7）**W2a0 + W2a 已完成并全部合入 `main`**（`47b0749`）：W2a0 docs（REV-067）、dsl（REV-069）、trace（REV-070）、py-worker（REV-068）、kernel+cli（REV-071）、e2e（REV-072）、W2a-docs 回写（REV-073）。opt-in deferrable gate / 图级有限返工（SOP v1.1）/ trace schema v4 / `parse_verdict_rework` 已交付；下一步 **W2b**。

**进行中任务**
- 无（W2a 收口完成，worktree/分支已清理）。

**下一步**
- W2b（每节点最大扇出限制：dsl+kernel+e2e+docs；DR-004 §3 W2b）→ W3a0 → W3a → W3b(可选) → W4a → W4b（DR-004 §4）。

**阻塞**
- 无硬阻塞；环境 `CARGO_HOME=<repo>/.cargo-home` + 离线；merge 门禁偶发 ISSUE-091 假失败（重跑通过）。
- 台账：ISSUE-100/101/104 fixed；ISSUE-105–112 open（112 = full rework 运行期未覆盖，保持 open）；ISSUE-062 保持 open（见 `doc/issues.md`）。
