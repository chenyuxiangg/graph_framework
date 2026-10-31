# 开发进度

> 更新规则：任务开始 / 完成 / 阻塞 / 交接时更新；只保留当前状态（历史看 git 与 `doc/reviews/`）；全文件 ≤ 30 行。

**当前阶段**：v0.2（M7）**W3a0 已完成并合入 `main`**（REV-078，`2823c16` 前收口）：DR-006 + spec 00/01/02/03/04/06/07 冻结（`builtin.human`/`WaitingHuman`/decide 自动接续/`human_decisions`/trace schema v5/AC-16 设计）；W1、W2a、W2b 已完成。

**进行中任务**
- 无（W3a0 收口完成；docs worktree 随合并清理）。

**下一步**
- W3a（trace v5 → kernel `WaitingHuman` → cli decide/consumer → e2e AC-16）→ W3a-docs as-built 回写 → W3b(可选) → W4a → W4b（DR-004 §4）。

**阻塞**
- 无硬阻塞；环境 `CARGO_HOME=<repo>/.cargo-home` + 离线；merge 门禁偶发 ISSUE-091 假失败（重跑通过）。
- 台账：ISSUE-100/101/104 fixed；ISSUE-105–112 + ISSUE-113–128 open；ISSUE-062 保持 open（见 `doc/issues.md`）。
