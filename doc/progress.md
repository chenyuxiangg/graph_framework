# 开发进度

> 更新规则：任务开始 / 完成 / 阻塞 / 交接时更新；只保留当前状态（历史看 git 与 `doc/reviews/`）；全文件 ≤ 30 行。

**当前阶段**：v0.4（M9）**D1 设计冻结进行中**（分支 `docs/docs-v04-design`，REV-105）：内置工具（文件/搜索/bash）+ 严格 workspace jail + OS 沙箱 + trace v7/工具回放；W5→v0.5。v0.3.0 已发布（tag `v0.3.0`）。

**进行中任务**
- docs `docs/docs-v04-design`（REV-105，作者 docs-dev）：DR-012/013/014 + spec 00/02/05/06/07/08 冻结草案 + AC-21–24；**沙箱 spike 已合入**（profile/嵌套/性能/平台/构建工具）。

**下一步**
- D1 评审通过 → D2 文件类+jail → D3 搜索类 → D4 bash+OS 沙箱 → D5 trace v7+回放+e2e → D6 兼容/性能/发布 v0.4.0（需用户授权）。

**阻塞**
- 无条件阻塞；spike 已合入，待 reviewer-a REV-105 评审。
- 台账：v0.4 风险登记 ISSUE-165+（sandbox-exec 弃用/Linux/TOCTOU/网络/大输出/v7 迁移/reexec）。
