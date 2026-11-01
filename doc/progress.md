# 开发进度

> 更新规则：任务开始 / 完成 / 阻塞 / 交接时更新；只保留当前状态（历史看 git 与 `doc/reviews/`）；全文件 ≤ 30 行。

**当前阶段**：v0.3（M8）**V1 设计冻结进行中**（分支 `docs/docs-v03-design`，REV-096）：skills + MCP + harness 可插拔三项设计（DR-008..011）+ spec 00/02/05/06/07 草案 + AC-18/19/20 设计；v0.2.0 已发布（tag `v0.2.0`）。

**进行中任务**
- docs `docs/docs-v03-design`（REV-096，作者 docs-dev）：待 arch-researcher 调研结论转发后锁定 research/MCP client/Codex·DSH 细节，再提交设计冻结。

**下一步**
- V1 评审通过后：**V2 skills** → **V3 MCP** → **V4 harness 插件** → **V5 集成/兼容/性能/发布 v0.3.0**（DR-008 §3）。

**阻塞**
- arch-researcher spike 结论待 Lead 转发（MCP rmcp 版本/Streamable HTTP 支持、Codex exec/ACP/DSH headless 可行性、Agent Skills 规范细节）——不阻塞骨架起草，阻塞最终设计数字/选型锁定。
- 已知限制：W5 共享评审队列延至 v0.4（推荐，Lead 裁决项）；ISSUE-112/143–150 仍 open（v0.3 发布前清理）。
