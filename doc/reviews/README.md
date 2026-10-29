# 评审记录索引

> 规则见 `AGENTS.md` §3/§6。每笔合并到 `main` 的代码改动必须有对应评审记录；本目录内容**只增不删**。

| 编号 | 日期 | 模块 | 分支 | 结论 | 评审人 |
|---|---|---|---|---|---|
| [REV-000](REV-000-bootstrap-dev-process.md) | 2026-10-28 | process | `main`（bootstrap 例外） | 通过 | lead（自审，仅文档/工具链） |
| [REV-001](REV-001-docs-dr-001-runtime-route.md) | 2026-10-29 | docs | `feat/docs-dr-001-runtime-route` | 通过（C1–C6 全部关闭） | docs-reviewer |
| [REV-002](REV-002-process-merge-gate-fix.md) | 2026-10-28 | process | `main`（bootstrap 后续） | 通过 | lead（自审，仅工具链） |
| [REV-003](REV-003-process-merge-branch-delete.md) | 2026-10-29 | process | `feat/process-fix-merge-branch-delete` | 通过（复审通过，原不通过） | docs-reviewer |
| [REV-004](REV-004-process-m0-scaffold.md) | 2026-10-29 | process | `feat/process-m0-scaffold` | 通过（M0 DoD 1–7） | docs-reviewer |
| [REV-005](REV-005-dsl-m1-loader.md) | 2026-10-29 | dsl | `feat/dsl-m1-loader` | 通过（M1 DSL R1–R14） | docs-reviewer |

命名：`REV-<NNN>-<module>-<slug>.md`；模板见 [`TEMPLATE.md`](TEMPLATE.md)。

- `merge.sh` 会查找包含 `分支: <branch>` 且 `结论：通过` 的记录，找不到则拒绝合并。
- `有条件通过` 必须在对应评审记录追加条件关闭记录后再合并。
- 评审记录不得删除；结论变更用追加「修订」段。
