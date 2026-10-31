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
| [REV-030](REV-030-cli-m5-replay.md) | 2026-10-30 | cli | `feat/cli-m5-replay` | 通过（replay L1/L2、llm_calls 接线、启动中止标定、`timeout_hint` 转发） | reviewer-b |
| [REV-031](REV-031-worker-m5-crash-detail.md) | 2026-10-30 | kernel/worker/cli/harness | `fix/worker-m5-crash-detail` | 通过（主审 reviewer-a + 第二评审 reviewer-b；跨模块，ISSUE-066 signal/exit） | reviewer-a + reviewer-b |
| [REV-032](REV-032-e2e-m5-cli-replay.md) | 2026-10-30 | e2e | `test/e2e-m5-cli-replay` | 通过（AC-01 CLI 真链路、L2 replay、崩溃标定、ISSUE-076 e2e；37 passed） | reviewer-b |
| [REV-033](REV-033-harness-m5-timeout.md) | 2026-10-30 | harness | `fix/harness-m5-timeout` | 通过（ISSUE-076 timeout_hint） | reviewer-b |
| [REV-034](REV-034-trace-m5-recovery.md) | 2026-10-30 | trace | `feat/trace-m5-recovery` | 通过（ISSUE-084 标定 API / ISSUE-048 判活列） | reviewer-a |
| [REV-035](REV-035-docs-m5-writeback.md) | 2026-10-30 | docs | `docs/docs-m5-writeback` | 通过（首轮有条件通过，C1–C3 关闭后复审通过；spec 00/04/05/06/07/08 M5 回写） | reviewer-a |
| [REV-036](REV-036-docs-m5-wrapup.md) | 2026-10-30 | docs/process | `docs/process-m5-wrapup` | 通过（M5 收尾：台账/进度/评审索引/baseline/README） | reviewer-b |
| [REV-038](REV-038-process-m5-issue-reg.md) | 2026-10-30 | docs/process | `docs/process-m5-issue-reg` | 通过（M5 台账登记 + DR-002 wave2/回写修订） | reviewer-a |
| [REV-039](REV-039-process-m6-kickoff.md) | 2026-10-30 | process/docs | `docs/process-m6-kickoff` | 通过（首轮有条件通过，C1–C4 关闭后增量复审通过；M6 DR-003） | reviewer-a |
| [REV-040](REV-040-kernel-m6-timeout-error.md) | 2026-10-31 | kernel | `fix/kernel-m6-timeout-error` | 通过（ISSUE-086 实例超时错误） | lead |
| [REV-041](REV-041-trace-m6-dual-channel.md) | 2026-10-31 | trace | `fix/trace-m6-dual-channel` | 通过（ISSUE-033 双通道 + 发送序号有序合并） | lead |
| [REV-042](REV-042-harness-m6-preview-canonicalize.md) | 2026-10-31 | harness | `fix/harness-m6-preview-canonicalize` | 通过（ISSUE-075/077） | lead |
| [REV-043](REV-043-cli-m6-exit-codes.md) | 2026-10-31 | cli | `fix/cli-m6-exit-codes` | 通过（ISSUE-059 + REV-028 #6） | lead |
| [REV-044](REV-044-e2e-m6-quality-ac12.md) | 2026-10-31 | e2e | `test/e2e-m6-quality-ac12` | 通过（ISSUE-089 + AC-12 同图 10×） | lead |
| [REV-045](REV-045-process-m6-deps.md) | 2026-10-30 | process | `chore/process-m6-deps` | 通过（ISSUE-068/078/073 + CI `--locked`） | reviewer-a |
| [REV-046](REV-046-docs-m6-wave1.md) | 2026-10-31 | docs/process | `docs/docs-m6-wave1` | 通过（W1 状态收口；首轮有条件通过 C1–C4 关闭后增量复审通过） | reviewer-a |
| [REV-047](REV-047-trace-m6-failstop.md) | 2026-10-31 | trace/kernel/cli | `fix/trace-m6-failstop` | 通过（ISSUE-036 fail-stop 接线；跨模块主审+第二评审） | lead + reviewer-a |
| [REV-048](REV-048-harness-m6-run-columns.md) | 2026-10-31 | harness/worker/cli/trace | `feat/harness-m6-run-columns` | 通过（ISSUE-082 runs 版本/模型参数列；C1「尽力写」关闭后通过） | lead + reviewer-a |
| [REV-049](REV-049-trace-m6-global-vars.md) | 2026-10-31 | trace/cli | `feat/trace-m6-global-vars` | 通过（ISSUE-087 schema v3 + global_vars；schema 专项第二评审） | lead + reviewer-a |
| [REV-050](REV-050-cli-m6-human-realtime.md) | 2026-10-31 | cli | `fix/cli-m6-human-realtime` | 通过（ISSUE-085 human 生命周期实时） | lead |
| [REV-052](REV-052-harness-m6-tool-limits.md) | 2026-10-31 | harness/worker/cli | `fix/harness-m6-tool-limits` | 通过（ISSUE-079 工具并发共享 WorkerLimits；跨模块双评审） | lead + reviewer-a |
| [REV-053](REV-053-harness-m6-split.md) | 2026-10-31 | harness | `chore/harness-m6-split` | 通过（ISSUE-081 harness 侧拆分，最大 46 行） | lead |
| [REV-054](REV-054-cli-m6-cli-split.md) | 2026-10-31 | cli | `refactor/cli-m6-cli-split` | 通过（ISSUE-081 cli 侧拆分 13→0） | lead |
| [REV-055](REV-055-e2e-m6-final-asserts.md) | 2026-10-31 | e2e | `test/e2e-m6-final-asserts` | 通过（ISSUE-086 实例 error_code + validate 退出码回归） | reviewer-a |
| [REV-056](REV-056-cli-m6-host-hardening.md) | 2026-10-31 | cli | `fix/cli-m6-host-hardening` | 通过（ISSUE-088 多机/EPERM/host 加固） | lead |
| [REV-057](REV-057-docs-m6-writeback.md) | 2026-10-31 | docs | `docs/docs-m6-writeback` | 通过（M6 spec 回写 05/06/07/08 + architecture + PROVENANCE；C1–C3 关闭后通过） | reviewer-a |
| [REV-058](REV-058-process-m6-ci-clippy.md) | 2026-10-31 | process/cli/docs | `chore/process-m6-ci-clippy` | 通过（merge 门禁 clippy + cli lint + refactor 前缀 + ISSUE-094） | reviewer-a |
| [REV-060](REV-060-e2e-m6-tool-limits.md) | 2026-10-31 | e2e | `test/e2e-m6-tool-limits` | 通过（e2e 工具+节点共享上限适配） | lead |
| [REV-061](REV-061-process-m6-wrapup.md) | 2026-10-31 | process/docs | `docs/docs-m6-wrapup` | 通过（M6 收口：台账/进度/索引/baseline/ISSUE-096/097、AC-12 10 次重跑） | reviewer-a |
| [REV-062](REV-062-docs-software-team-sop-v1.md) | 2026-10-31 | docs | `docs/docs-software-team-sop-v1` | 通过（首轮有条件通过；C1–C6 关闭、S1–S7/R1–R6 落实，附录 A validate 通过；R7–R8 非阻塞备注） | reviewer-sop |
| [REV-063](REV-063-docs-v02-kickoff.md) | 2026-10-31 | docs | `docs/docs-v02-kickoff` | 通过（r5 关闭 C1–C13/S11；S12 环检测细则为非阻塞建议） | reviewer-sop |
| [REV-064](REV-064-py-worker-software-team-parse-verdict.md) | 2026-10-31 | py-worker | `feat/py-worker-software-team-parse-verdict` | 通过（W1 v1 parse_verdict：approved/透传/异常契约；44 passed；S1/S2 非阻塞建议） | reviewer-sop |
| [REV-065](REV-065-e2e-software-team-v1.md) | 2026-10-31 | e2e | `test/e2e-software-team-v1` | 有条件通过（full/smoke e2e 5 passed/1 ignored；C1 HARNESS_MAX_ITERATIONS e2e 待补；S1–S3 建议） | reviewer-sop |

命名：`REV-<NNN>-<module>-<slug>.md`；模板见 [`TEMPLATE.md`](TEMPLATE.md)。

- `merge.sh` 会查找包含 `分支: <branch>` 且 `结论：通过` 的记录，找不到则拒绝合并。
- `有条件通过` 必须在对应评审记录追加条件关闭记录后再合并。
- 评审记录不得删除；结论变更用追加「修订」段。
- REV-012（worker 分支含 kernel 访问器与 Cargo.lock）与 REV-015（e2e 分支含根 Cargo.toml/Cargo.lock）属 Lead 批准的跨模块改动，均由 reviewer-b 追加第二评审小节。
- M4：REV-017（process 依赖）、REV-018（kernel 错误码/ContextOverflow）、REV-021（worker 装箱）由 Lead 提交、reviewer-b 评审；REV-020/022/023 分别由 harness-dev/e2e-dev/docs-writer 提交、reviewer-b 评审；REV-024 由 Lead 提交、reviewer-b 评审。REV-020/022 含 Cargo.lock 跨模块变更，已在评审记录中声明。
- M5（已合并到 main）：REV-025（process/docs kickoff+DR-002，lead/reviewer-a）、REV-026（kernel，kernel-dev/reviewer-a）、REV-027（harness，harness-dev/reviewer-b）、REV-028（cli，cli-dev/reviewer-a）、REV-029（e2e，e2e-dev/reviewer-b）、REV-030（cli replay/L2/标定，cli-dev/reviewer-b）、REV-031（kernel/worker/cli/harness ISSUE-066，worker-dev；reviewer-a 主审 + reviewer-b 第二评审）、REV-032（e2e CLI/L2/标定，e2e-dev/reviewer-b）、REV-033（harness，harness-dev/reviewer-b）、REV-034（trace，trace-dev/reviewer-a）、REV-035（docs spec 回写，docs-writer/reviewer-a）、REV-036（docs/process 收尾，lead/reviewer-b）、REV-038（process/docs 台账，lead/reviewer-a）。REV-028/030 含根 `Cargo.lock`、REV-025/035/036 含根 `README.md`（process）跨模块变更；REV-031 为 kernel/worker/cli/harness 跨模块（Lead 批准、双评审）；REV-032 含 e2e dev-dep `rusqlite` 根 `Cargo.lock` 机械改动。REV-037（process 依赖治理/CI）顺延 M6。

- M6（已全部合并到 main，2026-10-31）：REV-039（kickoff/DR-003）、REV-040–045（W1）、REV-046（W1 收口）、REV-047–056（W2/W3：fail-stop、runs 两列、global_vars、human 实时、079 共享限流、081 拆分、086 e2e、088 主机加固）、REV-057（spec 回写）、REV-058（merge 门禁 clippy，计划外）、REV-060（e2e 工具共享上限适配，计划外）、REV-061（wrapup）。**REV-051、REV-059 未创建**（ISSUE-080 并入 REV-057；059 编号未使用）。跨模块：REV-047/048/049/052 为多模块（Lead 批准 + 第二评审 reviewer-a）；REV-057/058/061 为 docs/process。
