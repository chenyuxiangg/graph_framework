# 开发进度

> 更新规则：任务开始 / 完成 / 阻塞 / 交接时更新；只保留当前状态（历史看 git 与 `doc/reviews/`）；全文件 ≤ 30 行。

**当前阶段**：v0.2（M7）**W2b 已完成并纳入本波收口**：dsl（REV-074，`1fa4cc0`/合并链 `7b4aba1..220f53c`）、kernel+cli（REV-075，含 cli 机械 arm 第二评审，`a4cb1b1`）、e2e（REV-076，`9a91076`）、W2b-docs 回写（REV-077，有条件通过→C1–C4 关闭后通过，随本波合并）。节点级 `max_fanout`、R16 静态上限、运行期 `NODE_FANOUT_EXCEEDED`（防御层）已交付；默认无字段行为/hash 不变。

**进行中任务**
- 无（W2b 四个实现分支 + docs 回写均已收口；docs worktree 随合并清理）。

**下一步**
- W3a0（人工评审设计冻结：decide/resume + `human_decisions`）→ W3a → W3b(可选) → W4a → W4b（DR-004 §4）。

**阻塞**
- 无硬阻塞；环境 `CARGO_HOME=<repo>/.cargo-home` + 离线；merge 门禁偶发 ISSUE-091 假失败（重跑通过）。
- 台账：ISSUE-100/101/104 fixed；ISSUE-105–112 + 新增 ISSUE-113–125（REV-074/075/076/077 P3 建议）open；ISSUE-062 保持 open（见 `doc/issues.md`）。
