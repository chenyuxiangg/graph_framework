# 开发进度

> 更新规则：任务开始 / 完成 / 阻塞 / 交接时更新；只保留当前状态（历史看 git 与 `doc/reviews/`）；全文件 ≤ 30 行。

**当前阶段**：v0.2（M7）W2a0 + W2a 已实现完成（docs/dsl/trace/py-worker/kernel+cli 已合入 `main`；e2e 分支 `test/e2e-gate-defer` **REV-072 已通过**（§7 N1 + §8 单 item 加固），待合并）；W2a-docs 回写（REV-073）有条件通过，S-1..S-4 + N1–N8 已修，AC-14 c/e 已按 `--profile rework-item` 5/7 单 item 校准（终测行数/hash 随本 commit）。

**进行中任务**
1. W2a-docs（REV-073）：最终 commit + reviewer-docs 增量复审。
2. e2e 分支：待合并（最终 HEAD 以合并时为准）。

**下一步**
- REV-073 增量复审通过 → 合并 W2a-docs；e2e 合并后 rebase。
- W2b（每节点最大扇出限制，dsl+kernel+e2e+docs）→ W3a0 → W3a → W3b(可选) → W4a → W4b（DR-004 §4）。

**阻塞**
- 无硬阻塞；环境 `CARGO_HOME=<repo>/.cargo-home` + 离线；merge 门禁偶发 ISSUE-091 假失败（重跑通过）。
- 台账：ISSUE-104/100/101 fixed；ISSUE-105–112 open（112 = full rework 运行期未覆盖，保持 open）；ISSUE-062 保持 open（见 `doc/issues.md`）。
