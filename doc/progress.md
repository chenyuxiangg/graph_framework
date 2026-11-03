# 开发进度

> 更新规则：任务开始 / 完成 / 阻塞 / 交接时更新；只保留当前状态（历史看 git 与 `doc/reviews/`）；全文件 ≤ 30 行。

**当前阶段**：v0.4.0（M9）**发布收口（D6）**：内置工具/workspace 沙箱/trace v7 已合并 main（`8897053`，D2–D5c）；tag `v0.4.0`/`dist/RELEASE_NOTES-v0.4.0.md` 待发布流程与用户授权；性能 [`doc/perf-baseline-v0.4.txt`](perf-baseline-v0.4.txt)。

**进行中任务**
- 无。

**下一步**
- v0.5 候选：真实 Codex/DSH 验证 + ISSUE 清理 + `reexec` 等（**W5 已取消**，用户 2026-11-03，DR-015；不含 W5）；REV-113 通过 → merge → dist/tag/GitHub Release（用户授权后）。

**阻塞**
- 无硬阻塞；tag/GitHub Release 需用户发布前最终授权 + 新 token；bash 真实隔离证据需用户普通终端运行 `verify-sandbox.sh`（嵌套环境仅覆盖 fail-closed）。
