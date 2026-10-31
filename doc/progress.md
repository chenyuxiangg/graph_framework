# 开发进度

> 更新规则：任务开始 / 完成 / 阻塞 / 交接时更新；只保留当前状态（历史看 git 与 `doc/reviews/`）；全文件 ≤ 30 行。

**当前阶段**：v0.2（M7）进行中——**W1（SOP v1 落地）已完成**：py-worker `parse_verdict`（REV-064）+ e2e codegen/资产/用例（REV-065）+ docs AC-13/执行记录（REV-066）全部合入 `main`；下一步 W2a0。

| 里程碑 | 状态 | 关键交付 | REV |
|---|---|---|---|
| M0–M6 | done | Rust 内核 + Python worker、DSL/内核/trace/replay、Harness、CLI、e2e AC-01…12 | 000–061 |
| v0.1.0 发布 / SOP 设计稿 v1 | done | tag + GitHub Release；`doc/design/software-team-sop-v1.md` | 062 |
| v0.2 立项 | done | DR-004（W1–W4；W5→v0.3；A1–A7 按推荐） | 063 |
| v0.2 W1 | done | `workers.software_team.parse_verdict`；`tests/fixtures/graphs/software_team_v1/**` + `tests/e2e/software_team_v1.rs`（full 60/100、smoke 11/18、approved/rework/invalid/max_iterations/real-LLM ignored）；07 AC-13 | 064–066 |

**下一步**
1. W2a0（docs，返工设计冻结：P v2/两条 defer 边/attempt-per-activation/dormant）→ W2a（kernel/dsl/trace/py-worker/e2e）。
2. 之后按 DR-004 §4：W2b → W3a0 → W3a → W3b(可选) → W4a → W4b。
3. 收口项：ISSUE-091（时序 flaky）、098/099（REV-062/发布口径）、100（spec 02 环检测）、101（parse_verdict 文档/边界）、102（e2e reviewer 输入快照）、103（PROVENANCE 改动范围）。

**阻塞**
- 无硬阻塞。环境仍须 `CARGO_HOME=<repo>/.cargo-home` + 离线；merge 门禁偶发 ISSUE-091 假失败（重跑通过）。
