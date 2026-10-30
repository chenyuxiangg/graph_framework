# 开发进度

> 更新规则：任务开始 / 完成 / 阻塞 / 交接时更新；只保留当前状态（历史看 git 与 `doc/reviews/`）；全文件 ≤ 30 行。

**当前阶段**：M6（2026-10-31）**已完成**——M0–M6 全部交付并合入 `main`；范围/波次/编号见 [`decisions/DR-003-m6-scope-and-waves.md`](decisions/DR-003-m6-scope-and-waves.md)

| 里程碑 | 状态 | 关键交付 | REV |
|---|---|---|---|
| M6 W1 | done | 033 双通道 / 059 退出码 / 068·078·073 依赖 / 075·077 harness 安全 / 086 实例超时 / 089 e2e 质量 + AC-12 10× | 039–046 |
| M6 W2 | done | 036 fail-stop / 082 runs 两列 / 087 global_vars（schema v3）/ 050 human 实时 / 079 共享限流 / 053+054 函数拆分 | 047–054 |
| M6 W3 | done | 055 e2e 收口断言 / 056 主机加固 / 057 spec 回写 / 058 merge clippy / 060 e2e 工具限流适配 | 055–060 |
| M6 收口 | done | 台账/进度/README/索引/baseline/DR-003、ISSUE-096/097、AC-12 连续 10 次重跑 | 061 |

**下一步**
1. M6 排除项（另立 v0.2/专项）：ISSUE-014/017/028/029/030–032/035/037–041/044/045/047/049/051/055–058/060/062–065/067/069–072/083/090/091/093/095 等，见 [`issues.md`](issues.md)。
2. 后续关注：ISSUE-090（tool 失败摘要脱敏）、091（时序测试在高负载下 flaky）、093（共享 target 流程固化）、095（kernel host_name 迁移）。

**阻塞**
- 无硬阻塞。环境：cargo 必须 `CARGO_HOME=<repo>/.cargo-home` + 离线；**不得共用一个 `CARGO_TARGET_DIR` 并发构建**（ISSUE-093）；merge 门禁已含 `cargo build`（CLI bin）+ test + clippy（ISSUE-094/096）。
