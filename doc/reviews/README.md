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
| [REV-017](REV-017-process-m4-deps.md) | 2026-10-30 | process | `chore/process-m4-deps` | 通过 | reviewer-b |
| [REV-018](REV-018-kernel-m4-tool-error-code.md) | 2026-10-30 | kernel | `fix/kernel-m4-tool-error-code` | 通过（增量复审通过：`TOOL_ERROR` + `ContextOverflow` 第 8 值） | reviewer-b |
| [REV-019](REV-019-py-worker-m4-tool-runner.md) | 2026-10-30 | py-worker | `feat/py-worker-m4-tool-runner` | 通过（增量复审通过：可选 `timeout_ms`） | reviewer-b |
| [REV-020](REV-020-harness-m4-agent-loop.md) | 2026-10-30 | harness | `feat/harness-m4-agent-loop` | 通过（修订：原「有条件通过」，C1/C2 关闭后转通过） | reviewer-b |
| [REV-021](REV-021-worker-m4-large-err-lint.md) | 2026-10-30 | worker | `fix/worker-m4-large-err-lint` | 通过 | reviewer-b |
| [REV-022](REV-022-e2e-m4-ac08.md) | 2026-10-30 | e2e | `test/e2e-m4-ac08` | 通过 | reviewer-b |
| [REV-023](REV-023-docs-m4-spec-writeback.md) | 2026-10-30 | docs | `docs/docs-m4-spec-writeback` | 通过 | reviewer-b |
| [REV-024](REV-024-docs-m4-wrapup.md) | 2026-10-30 | docs/process | `docs/docs-m4-wrapup` | 通过 | reviewer-b |
| [REV-025](REV-025-process-m5-kickoff.md) | 2026-10-30 | process/docs | `docs/process-m5-kickoff` | 通过（首轮有条件通过，C1/C2 关闭后增量复审通过） | reviewer-a |
| [REV-026](REV-026-kernel-m5-p2.md) | 2026-10-30 | kernel | `fix/kernel-m5-p2` | 通过（ISSUE-061 / ISSUE-076 内核侧） | reviewer-a |
| [REV-027](REV-027-harness-m5-record.md) | 2026-10-30 | harness | `feat/harness-m5-record` | 通过（LlmCallRecorder seam / ISSUE-074 / ISSUE-081 部分） | reviewer-b |
| [REV-028](REV-028-cli-m5-run.md) | 2026-10-30 | cli | `feat/cli-m5-run` | 通过（首轮有条件通过，C1 关闭后增量复审通过） | reviewer-a |
| [REV-029](REV-029-e2e-m5-ac12.md) | 2026-10-30 | e2e | `test/e2e-m5-ac12` | 通过（AC-12 多 run 并发隔离） | reviewer-b |
| [REV-033](REV-033-harness-m5-timeout.md) | 2026-10-30 | harness | `fix/harness-m5-timeout` | 通过（ISSUE-076 timeout_hint） | reviewer-b |
| [REV-034](REV-034-trace-m5-recovery.md) | 2026-10-30 | trace | `feat/trace-m5-recovery` | 通过（ISSUE-084 标定 API / ISSUE-048 判活列） | reviewer-a |
| [REV-038](REV-038-process-m5-issue-reg.md) | 2026-10-30 | docs/process | `docs/process-m5-issue-reg` | 通过（M5 台账登记 + DR-002 wave2/回写修订） | reviewer-a |

命名：`REV-<NNN>-<module>-<slug>.md`；模板见 [`TEMPLATE.md`](TEMPLATE.md)。

- `merge.sh` 会查找包含 `分支: <branch>` 且 `结论：通过` 的记录，找不到则拒绝合并。
- `有条件通过` 必须在对应评审记录追加条件关闭记录后再合并。
- 评审记录不得删除；结论变更用追加「修订」段。
- REV-012（worker 分支含 kernel 访问器与 Cargo.lock）与 REV-015（e2e 分支含根 Cargo.toml/Cargo.lock）属 Lead 批准的跨模块改动，均由 reviewer-b 追加第二评审小节。
- M4：REV-017（process 依赖）、REV-018（kernel 错误码/ContextOverflow）、REV-021（worker 装箱）由 Lead 提交、reviewer-b 评审；REV-020/022/023 分别由 harness-dev/e2e-dev/docs-writer 提交、reviewer-b 评审；REV-024 由 Lead 提交、reviewer-b 评审。REV-020/022 含 Cargo.lock 跨模块变更，已在评审记录中声明。
- M5（已合并到 main）：REV-025（process/docs kickoff+DR-002，lead/reviewer-a）、REV-026（kernel，kernel-dev/reviewer-a）、REV-027（harness，harness-dev/reviewer-b）、REV-028（cli，cli-dev/reviewer-a）、REV-029（e2e，e2e-dev/reviewer-b）、REV-033（harness，harness-dev/reviewer-b）、REV-034（trace，trace-dev/reviewer-a）、REV-038（process/docs 台账，lead/reviewer-a）。REV-028 含根 `Cargo.lock` 跨模块变更、REV-025 含根 `README.md`（process）跨模块变更，均已在评审记录中声明并作第二评审。**尚未合并**：REV-030（cli replay/L2）、REV-031（ISSUE-066）、REV-032（e2e CLI/L2）、REV-035（docs 本分支）、REV-036（收尾），合并后由 Lead 补入本索引。
