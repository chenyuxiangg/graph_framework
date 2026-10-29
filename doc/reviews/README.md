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
| [REV-006](REV-006-process-m2-deps.md) | 2026-10-29 | process | `feat/process-m2-deps` | 通过 | reviewer-a |
| [REV-007](REV-007-kernel-m2-runloop.md) | 2026-10-29 | kernel | `feat/kernel-m2-runloop` | 通过（复审通过，原不通过） | reviewer-a |
| [REV-008](REV-008-trace-m2-sqlite.md) | 2026-10-29 | trace | `feat/trace-m2-sqlite` | 通过 | reviewer-a |
| [REV-009](REV-009-cli-m2-trace.md) | 2026-10-29 | cli | `feat/cli-m2-trace` | 通过 | reviewer-a |
| [REV-010](REV-010-docs-m2-progress.md) | 2026-10-29 | docs | `feat/docs-m2-progress` | 通过 | reviewer-a |
| [REV-011](REV-011-kernel-m2-timeout-error.md) | 2026-10-29 | kernel | `fix/kernel-m2-timeout-error` | 通过 | reviewer-a |
| [REV-012](REV-012-worker-m3-supervisor.md) | 2026-10-30 | worker | `feat/worker-m3-supervisor` | 通过（复审通过，原不通过；跨模块第二评审 reviewer-b） | reviewer-a |
| [REV-013](REV-013-py-worker-m3-sdk-examples.md) | 2026-10-30 | py-worker | `feat/py-worker-m3-sdk-examples` | 通过 | reviewer-b |
| [REV-014](REV-014-docs-m3-spec-writeback.md) | 2026-10-30 | docs | `feat/docs-m3-spec-writeback` | 通过（C1–C4 / R1–R5 关闭） | reviewer-b |
| [REV-015](REV-015-e2e-m3-acceptance.md) | 2026-10-30 | e2e | `test/e2e-m3-acceptance` | 通过（复审通过，C1 关闭；process 第二评审 reviewer-b） | reviewer-a |
| [REV-016](REV-016-docs-m3-wrapup.md) | 2026-10-30 | docs/process | `docs/docs-m3-wrapup` | 通过 | reviewer-b |

命名：`REV-<NNN>-<module>-<slug>.md`；模板见 [`TEMPLATE.md`](TEMPLATE.md)。

- `merge.sh` 会查找包含 `分支: <branch>` 且 `结论：通过` 的记录，找不到则拒绝合并。
- `有条件通过` 必须在对应评审记录追加条件关闭记录后再合并。
- 评审记录不得删除；结论变更用追加「修订」段。
- REV-012（worker 分支含 kernel 访问器与 Cargo.lock）与 REV-015（e2e 分支含根 Cargo.toml/Cargo.lock）属 Lead 批准的跨模块改动，均由 reviewer-b 追加第二评审小节。
