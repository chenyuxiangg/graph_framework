# 规格 v1.2 · 外部引用映射（REFERENCES）

> spec 正文沿用了冻结期的历史路径（`_spike/*`、`_research/*`、`_audit/*`、`决策记录-01` 等）。
> 这些材料已随规格入库到 `doc/analysis/`，映射如下；**以本表为准**。

| spec 中的历史引用 | 仓库内位置 | 说明 |
|---|---|---|
| `决策记录-01-范围与Harness.md` | [`../analysis/decision-record-01-scope.md`](../analysis/decision-record-01-scope.md) | 6 项拍板 + 范围边界 |
| `_audit/spec-audit.md` | [`../analysis/spec-audit-v1.1.md`](../analysis/spec-audit-v1.1.md) | 76 条缺陷清单（审计原文） |
| `_spike/REPORT.md` | [`../analysis/spike/REPORT.md`](../analysis/spike/REPORT.md) | 8 项技术验证报告 |
| `_spike/src/**`、`_spike/py/**`、`_spike/scripts/**` | [`../analysis/spike/`](../analysis/spike/) | 探针源码（可复算） |
| `_research/codex-embed.md` | [`../analysis/research-codex-embed.md`](../analysis/research-codex-embed.md) | Codex 复用调研 |
| `_research/harness-alternatives.md` | [`../analysis/research-harness-alternatives.md`](../analysis/research-harness-alternatives.md) | Rust 生态/DSH 调研 |
| 可行性评估 | [`../analysis/feasibility-assessment-v1.1.md`](../analysis/feasibility-assessment-v1.1.md) | 可开发性评估 |
| `MODULE-09`（Memory Manager） | **缺失（外部）** | 原设计引用的模块文档未提供；v1 不做 Memory（接口空实现） |
| 源稿 HTML / `/tmp/designdoc/doc.txt` | **外部，不入库** | 见 [`PROVENANCE.md`](PROVENANCE.md) §1 |

## 未入库的辅助材料

- spike 原始日志（`_spike/logs/*.log`）：被仓库 `.gitignore` 的 `logs/` 与 `*.log` 规则排除，**未入库**；
  报告 `REPORT.md` 保留了关键数字、命令与结论，可复算。
- 外部源稿与提取文本：见 PROVENANCE §1，不提供副本。
