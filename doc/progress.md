# 开发进度

> 更新规则：任务开始 / 完成 / 阻塞 / 交接时更新；只保留当前状态（历史看 git 与 `doc/reviews/`）；全文件 ≤ 30 行。

**当前阶段**：v0.3.0（M8）**发布收口（V5b）**：V2 skills（REV-097）、V3 MCP（REV-098）、V4 harness 插件（REV-099）、V5a e2e（REV-101）已合并 main；AC-18/19/20 实测 31 用例；tag `v0.3.0`/产物/性能基线待发布流程（REV-103）确认。

**进行中任务**
- docs `docs/docs-v03-spec`（**REV-102**，作者 docs-dev）：v0.3 as-built + AC-18/19/20 实测回写（31 用例）+ PROVENANCE 第 26 条 + README v0.3.0；待 reviewer-b 评审。

**下一步**
- REV-102 评审 → 发布 v0.3.0（tag/产物/性能基线，REV-103）→ v0.4（M9）**W5 共享评审队列** + ISSUE 清理。

**阻塞**
- 无硬阻塞；AC 数字需 V5a e2e 结果（Lead 转）；真实 Codex/DSH 环境未验证（adapter 骨架，best-effort，release notes 标注）。
- 台账：ISSUE-156–163（v0.3 V2/V3/V4 P2/P3 与真实环境验证）open；ISSUE-105–155 历史项见 `doc/issues.md`。
