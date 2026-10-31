# 开发进度

> 更新规则：任务开始 / 完成 / 阻塞 / 交接时更新；只保留当前状态（历史看 git 与 `doc/reviews/`）；全文件 ≤ 30 行。

**当前阶段**：v0.2（M7）**W2a0 + W2a 已实现完成**——docs（REV-067）、dsl（REV-069）、trace（REV-070）、py-worker（REV-068）、kernel+cli（REV-071）均已合入 `main`（`e7578f1` 时点）；e2e `test/e2e-gate-defer` **Phase 2B 完成**（`d31e5d2`：gate_defer 8/0、software_team_v1 11/1 ignored、e2e 58/0/2、workspace 694/0/3，连续 5 轮全绿）；下一步 **W2b（每节点最大扇出限制）**。

| 里程碑 | 状态 | 关键交付 | REV |
|---|---|---|---|
| M0–M6 / v0.1.0 | done | Rust 内核 + Python worker、DSL/内核/trace/replay、Harness、CLI、e2e AC-01…12；tag + Release | 000–061 |
| v0.2 立项 / SOP v1 | done | DR-004（W1–W4；W5→v0.3）+ SOP 设计稿 v1 | 062–063 |
| v0.2 W1 | done | `parse_verdict` + `software_team_v1` e2e（full 60/100、smoke 11/18）+ 07 AC-13 | 064–066 |
| v0.2 W2a0 | done | DR-005 + spec 00–08 W2a0 回写 + SOP v1.1 + AC-14 设计（spec 02 R15 / trace v4 / P v2 冻结） | 067 |
| v0.2 W2a | done（实现） | dsl `fa6ea16`+`bc92951`；trace `27f9200`；py-worker `dd96d89`+`f685178`；kernel `ee4ca27`+cli `6c90a16`；e2e `test/e2e-gate-defer` Phase 2A/2B | 068–072 |

**进行中**
1. W2a-docs（本批，REV-073）：07 AC-14 按实现校准、01/03/04/06 as-built、08 §8.9.1、DR-005 R1–R5、DR-004 §10、PROVENANCE/索引/台账。
2. e2e Phase 2B 已完成（`d31e5d2`）；本分支已补最终证据 commit（AC-14 c/e、ISSUE-104 fixed、行数/hash 重测）；待 REV-072 与合并。

**下一步**
- W2b（每节点扇出上限，dsl+kernel+e2e+docs）→ W3a0 → W3a → W3b(可选) → W4a → W4b（DR-004 §4）。

**阻塞**
- 无硬阻塞。环境仍须 `CARGO_HOME=<repo>/.cargo-home` + 离线；merge 门禁偶发 ISSUE-091 假失败（重跑通过）。

**遗留 ISSUE**：ISSUE-090（harness 脱敏）、091（时序 flaky）、093（共享 target）、095（kernel host）、098/099（文档/发布口径）、102（e2e reviewer 输入）、103（PROVENANCE 范围）、104–111（W2a/REV-072 新增；104 fixed，见 `doc/issues.md`）；ISSUE-100/101 已 fixed，ISSUE-062 保持 open。
