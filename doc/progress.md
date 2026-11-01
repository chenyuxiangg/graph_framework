# 开发进度

> 更新规则：任务开始 / 完成 / 阻塞 / 交接时更新；只保留当前状态（历史看 git 与 `doc/reviews/`）；全文件 ≤ 30 行。

**当前阶段**：v0.2.0 **已发布**（tag `v0.2.0`；`dist/RELEASE_NOTES-v0.2.0.md`；性能基线 [`doc/perf-baseline-v0.2.txt`](perf-baseline-v0.2.txt)）。M0–M7 全部完成：DSL 1.0/R1–R18、内核、trace v6、Harness、CLI、SOP v1/v1.1、W2b 扇出上限、W3a human decide、W4b foreach 动态 N；workspace 915/0/3、e2e 106/0/2。

**进行中任务**
- 无（v0.2.0 发布收口完成；worktree/分支清理由合并流程处理）。

**下一步**
- **v0.3 规划**：W5 共享评审队列（v0.3 候选）+ 清理 ISSUE-112（full-60 运行期基线）/ISSUE-143–149（P3）。

**阻塞**
- 无硬阻塞；已知限制：ISSUE-112（SOP full-60 运行期 fake-LLM 不稳定，发布例外）；**W3b durable resume 不做**（用户 2026-11-01 决定）；W5 延至 v0.3。
- 台账：ISSUE-142 fixed；ISSUE-105–141 + 143–149 open（P3 为主）；100/101/104 fixed；ISSUE-062 保持 open（见 `doc/issues.md`）。
