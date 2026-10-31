# 开发进度

> 更新规则：任务开始 / 完成 / 阻塞 / 交接时更新；只保留当前状态（历史看 git 与 `doc/reviews/`）；全文件 ≤ 30 行。

**当前阶段**：v0.2（M7）**W2b 已完成并合入 `main`**（`1ed37ab` 时点）；**W3a0 人工评审设计冻结（REV-078）已提交待评审/合并**：产出 [`DR-006`](decisions/DR-006-w3a-human-decide-freeze.md) + spec 00/01/02/03/04/06/07 冻结写入 + 07 AC-16 a–g（设计冻结）；本批只写设计、不写 as-built、不改代码。

**进行中任务**
- docs `docs/docs-human-design`（REV-078，作者 docs-dev，worktree `.worktrees/docs-docs-dev`）：**REV-078 复审通过（`05a5803`；C1–C5 全部关闭）**；R1–R4 复审残留已收口（trace `:memory:` 拒绝/库 API 口径、流程图顺序、ISSUE/numstat 修正、状态收口）；残留 P3 建议登记 ISSUE-126–128。

**下一步**
- W3a 实现（trace v5/human_decisions → kernel WaitingHuman/decide → cli decide/consumer → e2e AC-16）→ W3a-docs as-built 回写 → W3b(可选) → W4a → W4b（DR-004 §4）。

**阻塞**
- 无硬阻塞；环境 `CARGO_HOME=<repo>/.cargo-home` + 离线；merge 门禁偶发 ISSUE-091 假失败（重跑通过）。
- 台账：ISSUE-100/101/104 fixed；ISSUE-105–112 + ISSUE-113–125 open；ISSUE-062 保持 open（见 `doc/issues.md`）。
- DR-006 §12.3 五项已由 Lead 2026-10-31 裁决/采纳（期限缺省无期限、Paused decide 不自动 Resume、trace HumanDecision 行、保留符号形态；E-11：instance 不存在 → exit 1、非 WaitingHuman → exit 5）。
