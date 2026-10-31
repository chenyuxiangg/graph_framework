# 开发进度

> 更新规则：任务开始 / 完成 / 阻塞 / 交接时更新；只保留当前状态（历史看 git 与 `doc/reviews/`）；全文件 ≤ 30 行。

**当前阶段**：v0.2（M7）**已启动**——DR-004（REV-063 通过）已合入 `main`；用户 2026-10-31 确认 DR-004 §9 A1–A7「按推荐」；**W1（SOP v1 落地）进行中**：`feat/py-worker-software-team-parse-verdict` + `test/e2e-software-team-v1`。

| 里程碑 | 状态 | 关键交付 | REV |
|---|---|---|---|
| M0–M6 | done | Rust 内核 + Python worker、DSL/内核/trace/replay、Harness、CLI、e2e AC-01…12；详见 DR-003 / README | 000–061 |
| v0.1.0 发布 | done | release 构建 + 冒烟、`dist/` 产物与 SHA-256、notes；tag 已推；GitHub Release `v0.1.0` | — |
| SOP 设计稿 v1 | done | `doc/design/software-team-sop-v1.md`：设计角色依赖 DAG + 工作 agent + per-item 评审（参考规模 60 节点/100 边） | 062 |
| v0.2 立项 | done | DR-004：W1 SOP v1 落地 / W2 返工+扇出上限 / W3 人工评审 / W4 动态 N；共享队列 v0.3 | 063 |

**下一步**
1. W1：py-worker `parse_verdict` + e2e codegen/资产/用例（无内核改动）；之后 W1-docs 追加 DR-004 A 组冻结记录与 07 AC。
2. W1 后按 DR-004 §4 顺序：W2a0(docs) → W2a → W2b → W3a0 → W3a → W3b(可选) → W4a → W4b。
3. 收口项：ISSUE-091（时序 flaky）、ISSUE-098（REV-062 措辞）、ISSUE-099（tag 61/59 口径）、ISSUE-100（W2a spec 02 环检测规则）。

**阻塞**
- 无硬阻塞。环境仍须 `CARGO_HOME=<repo>/.cargo-home` + 离线；merge 门禁偶发 ISSUE-091 假失败（重跑通过）。
