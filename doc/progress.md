# 开发进度

> 更新规则：任务开始 / 完成 / 阻塞 / 交接时更新；只保留当前状态（历史看 git 与 `doc/reviews/`）；全文件 ≤ 30 行。

**当前阶段**：v0.2（M7）**W4b 已完成并合入 `main`**（`201341b`；REV-088–093，as-built REV-094）：foreach/R17/R18（DSL 19 码）、item 实例/controller/join、trace schema v6、4 个 `RUN_FOREACH_*`、AC-17 a–j **实测**（`tests/e2e/dynamic_fanout.rs` 21 用例；e2e 106/0/2、workspace 915/0/3）。W1/W2a/W2b/W3a/W4a 已完成。

**进行中任务**
- 无（W4b 收口完成；worktree/分支已清理）。

**下一步**
- **发布收口**：性能基线、兼容矩阵复核、`tag v0.2.0`（release notes 列 08 §8.10/§8.11 source-breaking）。

**阻塞**
- 无硬阻塞；环境 `CARGO_HOME=<repo>/.cargo-home` + 离线；merge 门禁偶发 ISSUE-091 假失败（重跑通过）。
- 台账：ISSUE-142（真实 v1→v6 迁移，**fixed**）；ISSUE-105–141 + **ISSUE-143–149**（dsl/kernel/cli/e2e/trace 残余 P3）open；100/101/104 fixed；ISSUE-062 保持 open（见 `doc/issues.md`）。
