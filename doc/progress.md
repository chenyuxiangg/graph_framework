# 开发进度

> 更新规则：任务开始 / 完成 / 阻塞 / 交接时更新；只保留当前状态（历史看 git 与 `doc/reviews/`）；全文件 ≤ 30 行。

**当前阶段**：v0.1.0 已发布（annotated tag `v0.1.0` @ `5d43c58`，2026-10-31，已推 origin）；`software_team` SOP 设计稿 v1（REV-062）合入 `main`（`7cb13ca`）；GitHub Release 待 token。

| 里程碑 | 状态 | 关键交付 | REV |
|---|---|---|---|
| M0–M6 | done | Rust 内核 + Python worker、DSL/内核/trace/replay、Harness、CLI、e2e AC-01…12；详见 DR-003 / README | 000–061 |
| v0.1.0 发布 | done | release 构建 + 冒烟（validate/run/trace/replay）、`dist/` 产物与 SHA-256、release notes；tag 已推 origin | — |
| SOP 设计稿 v1 | done | `doc/design/software-team-sop-v1.md`：设计角色依赖 DAG + 工作 agent + per-item 评审（参考规模 60 节点/100 边）；人工评审/图级返工/扇出上限/动态 N/共享队列延期 v0.2 | 062 |

**下一步**
1. GitHub Release：等 `GITHUB_TOKEN`（tag 已推，release 未建；notes 在 `dist/RELEASE_NOTES-v0.1.0.md`）。
2. v0.2 立项（另立 DR）：人工评审、图级有限返工、每节点最大扇出限制、动态 N、共享评审队列；SOP 设计稿 codegen + fake LLM e2e 落地。
3. 收口项：ISSUE-091（时序 flaky，2026-10-31 合并门禁复现 1 次）、ISSUE-098（REV-062 R7/R8 措辞）、ISSUE-099（tag 注解 61/59 口径）。

**阻塞**
- GitHub Release 待用户 token；环境仍须 `CARGO_HOME=<repo>/.cargo-home` + 离线，merge 门禁偶发 ISSUE-091 假失败（重跑通过）。
