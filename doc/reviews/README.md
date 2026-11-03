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
| [REV-065](REV-065-e2e-software-team-v1.md) | 2026-10-31 | e2e | `test/e2e-software-team-v1` | 通过（C1 HARNESS_MAX e2e 关闭；target 7 passed/1 ignored、全 e2e 46/2；S1/S3 落实，S2 非阻塞建议；clippy cmp_owned 修复 df0a300 增量确认） | reviewer-sop |
| [REV-066](REV-066-docs-sop-v1-ac.md) | 2026-10-31 | docs | `docs/docs-sop-v1-ac` | 通过（C1–C3 关闭、S1–S3 落实；07 a02afcd5…/254、合计 6051；S4 非阻塞建议） | reviewer-sop |
| [REV-067](REV-067-docs-rework-design.md) | 2026-10-31 | docs | `docs/docs-rework-design` | 通过（W2a0 设计冻结；首轮有条件通过，C1/C2/S1–S3 于 `749e9ee` 关闭、N-1…N-11 落实后复审通过；残留建议见记录 §7.5） | reviewer-docs |
| [REV-068](REV-068-py-worker-parse-rework.md) | 2026-10-31 | py-worker | `feat/py-worker-parse-rework` | 通过（P v2 `parse_verdict_rework`：rework 正常输出/闭集/3 键/identity/异常前缀；v1 字节码未变；86 passed、全量 170；S1/S2 于 `50eb1f7` 关闭，S3 非阻塞建议） | reviewer-a |
| [REV-069](REV-069-dsl-gate-defer.md) | 2026-10-31 | dsl | `feat/dsl-gate-defer` | 通过（W2a 边级 `on_gate_false` + R15；v0.1.0 5 valid/31 bad CLI 输出与 hash 逐字节不变；N1–N4 于 `a084f2e` 关闭、N5–N8 非阻塞建议） | reviewer-b |
| [REV-070](REV-070-trace-gate-defer.md) | 2026-10-31 | trace（含 Lead 批准 cli 机械改动） | `feat/trace-gate-defer` | 通过（GateResult v4/payload/v3→v4 迁移；trace 63、cli 160、workspace 650 passed；S1–S5 非阻塞建议；第二评审 lead 已追加确认） | reviewer-a |
| [REV-071](REV-071-kernel-deferrable-gate.md) | 2026-10-31 | kernel（含 Lead 批准 cli bridge 透传） | `feat/kernel-deferrable-gate` | 通过（deferrable gate/attempt-per-activation/dormant + GateResult 三字段真实来源；kernel 107、cli 160、workspace 682/0/3、e2e 46/2；N1–N7 非阻塞建议；第二评审 reviewer-a 追加 §7） | reviewer-b（主审） |
| [REV-072](REV-072-e2e-gate-defer.md) | 2026-10-31 | e2e | `test/e2e-gate-defer` | 通过（W2a AC-14 a/b/c/e/f/g/h 真链路；e2e 58/0/2、workspace 694/0/3；v0.1.0 seed 独立复现 schema 31/31 且 golden 一致；N1–N4 非阻塞建议；§7 N1 关闭 + §8 单 item profile 加固增量复核通过） | reviewer-a |
| [REV-073](REV-073-docs-gate-defer-spec.md) | 2026-10-31 | docs | `docs/docs-gate-defer-spec` | **通过**（W2a 实现回写/e2e 单 item 校准/as-built/08 §8.9.1/台账/PROVENANCE；首轮有条件通过 → 条件修复提交关闭 S-1..S-4 与 N-1…N-8 后复审通过；残留 N-9…N-12 非阻塞建议） | reviewer-docs |
| [REV-074](REV-074-dsl-max-fanout.md) | 2026-10-31 | dsl | `feat/dsl-max-fanout` | 通过（W2b 节点级 `max_fanout` + R16 静态扇出上限；v0.1.0 5 valid/31 bad CLI 输出与 hash 逐字节不变；P3 建议 4 条 → ISSUE-113–116） | reviewer-a |
| [REV-075](REV-075-kernel-max-fanout.md) | 2026-10-31 | kernel（含 cli 跨模块机械 arm） | `feat/kernel-max-fanout` | 通过（W2b 运行期每激活扇出上限 + `NODE_FANOUT_EXCEEDED`/canonical JSON；kernel 110、`--features test-hooks` 120、cli 161；P3 建议 → ISSUE-117–120；第二评审 lead 已追加 cli arm） | reviewer-b（主审）+ lead（cli 第二评审） |
| [REV-076](REV-076-e2e-max-fanout.md) | 2026-10-31 | e2e | `test/e2e-max-fanout` | 通过（W2b AC-15 a/b/c 真链路：静态反例/正例/运行期 test-hooks；`max_fanout` 6/0、e2e 64/0/2、workspace 725/0/3；P3 建议 → ISSUE-121–124） | reviewer-a |
| [REV-077](REV-077-docs-fanout-spec.md) | 2026-10-31 | docs | `docs/docs-fanout-spec` | 通过（条件关闭见记录 §6；W2b spec 00/01/02/03/06/07 回写 + PROVENANCE 第 18 条 + AC-15/索引/台账；C1–C4 于条件修复提交关闭） | reviewer-b |
| [REV-078](REV-078-docs-human-design.md) | 2026-10-31 | docs | `docs/docs-human-design` | 通过（首轮有条件通过 → C1–C5 关闭 + R1–R4 收口后复审通过；W3a0 设计冻结：DR-006 + spec 00/01/02/03/04/06/07 冻结 + trace v5/decide 契约 + AC-16 设计） | reviewer-b |
| [REV-079](REV-079-trace-human-decisions.md) | 2026-10-31 | trace | `feat/trace-human-decisions` | 通过（schema v5：WaitingHuman CHECK/重建两表/human_decisions+pending 索引/v1–v4→v5 迁移；trace 79、workspace 741/0/3；e2e 机械改动第二评审 lead；P3 建议 → ISSUE-129/130） | reviewer-a |
| [REV-080](REV-080-kernel-human-wait.md) | 2026-10-31 | kernel（含 cli 机械 arm） | `feat/kernel-human-wait` | 通过（WaitingHuman 生命周期/builtin.human/HumanDecision 控制通道；kernel 122、test-hooks 132、cli 162；P3 建议 → ISSUE-131/132/133） | reviewer-b（主审）+ lead（cli 第二评审） |
| [REV-081](REV-081-cli-decide.md) | 2026-10-31 | cli | `feat/cli-decide` | 通过（zgraph decide/consumer/trace 决策行/`:memory:` 预检；cli 198、workspace 790/0/3；P3 建议 → ISSUE-134/135/136 + 环境观察 ISSUE-137） | reviewer-a |
| [REV-082](REV-082-cli-human-record.md) | 2026-11-01 | cli | `fix/cli-human-record` | 通过（human L2 recorded_outputs 补录 + replay outputs handler 替换；cli 206、workspace 798/0/3；P3 建议 → ISSUE-140） | reviewer-a |
| [REV-083](REV-083-e2e-human-decide.md) | 2026-11-01 | e2e | `test/e2e-human-decide` | 通过（AC-16 a–g 真链路 + L2 human outputs；human_decide 21/0、e2e 85/0/2、workspace 819/0/3、3 次稳定；P3 建议 → ISSUE-138/139） | reviewer-b |
| [REV-084](REV-084-docs-human-spec.md) | 2026-11-01 | docs | `docs/docs-human-spec` | 通过（W3a as-built 回写：spec 00/01/03/04/06/07/08 + DR-006 §15 + AC-16 实测 21 用例 + PROVENANCE 第 22 条；e2e 85/0/2、workspace 819/0/3、PROVENANCE §3 9/9 SHA/行数实测一致；P3 建议见记录 §3） | reviewer-a |
| [REV-085](REV-085-docs-w3a-wrapup.md) | 2026-11-01 | docs/process | `docs/docs-w3a-wrapup` | 通过（W3a 合并后 progress 最终态 + REV-086 残余 N21–N27；仅 `doc/progress.md` 等文字） | reviewer-a |
| [REV-086](REV-086-docs-dynamic-fanout-design.md) | 2026-11-01 | docs | `docs/docs-dynamic-fanout-design` | 通过（W4a 动态 N/foreach 设计冻结；首轮有条件通过 → C1–C5 + P2/P3 关闭后复审通过；DR-007 + spec 冻结 + SOP v1.2 + AC-17 a–j 设计） | reviewer-b |
| [REV-087](REV-087-docs-w4a-wrapup.md) | 2026-11-01 | docs/process | `docs/docs-w4a-wrapup` | 通过（W4a 合并后 progress 最终态 + REV-086 残余收口） | reviewer-a |
| [REV-088](REV-088-dsl-foreach.md) | 2026-11-01 | dsl | `feat/dsl-foreach` | 通过（foreach 字段/R17/R18/19 码/22 fixtures；dsl 118/0；fixture 跨模块 Lead 第二评审；P3 → ISSUE-147） | reviewer-a |
| [REV-089](REV-089-trace-dynamic.md) | 2026-11-01 | trace（含 cli/e2e 机械） | `feat/trace-dynamic` | 通过（首轮不通过 → 修复后通过；schema v6/item 列/expansions/真实 v1→v6/FK 窗口；trace 94/0；P1 ISSUE-142 fixed；P3 → ISSUE-149） | reviewer-b（主审）+ lead（第二评审） |
| [REV-090](REV-090-kernel-dynamic-fanout.md) | 2026-11-01 | kernel（含 cli bridge） | `feat/kernel-dynamic-fanout` | 通过（有条件通过 → 复审通过；controller/item/join/4 码；kernel 149/159、cli 222；P3 → ISSUE-144/145） | reviewer-b（主审）+ lead（cli 第二评审） |
| [REV-091](REV-091-cli-dynamic.md) | 2026-11-01 | cli | `feat/cli-dynamic` | 通过（trace ForeachExpansion/L2 per-item/预检；cli 222/0；P3 → ISSUE-146） | reviewer-a |
| [REV-092](REV-092-kernel-foreach-items-unwrap.md) | 2026-11-01 | kernel fix | `fix/kernel-foreach-items-unwrap` | 通过（items_port 整包 object 同名键 unwrap 规则 A；kernel 149/159、cli 222、workspace 894/0/3） | reviewer-b |
| [REV-093](REV-093-e2e-dynamic-fanout.md) | 2026-11-01 | e2e | `test/e2e-dynamic-fanout` | 通过（AC-17 a–j 真链路 21 用例；e2e 106/0/2、workspace 915/0/3、6 连跑；P2/P3 → ISSUE-143/148） | reviewer-b |
| [REV-094](REV-094-docs-dynamic-spec.md) | 2026-11-01 | docs | `docs/docs-dynamic-spec` | 通过（W4b as-built 回写：spec 00/01/02/03/04/06/07/08 + DR-007 §14.1 + AC-17 实测 21 用例 + PROVENANCE 第 25 条；e2e 106/0/2、workspace 915/0/3、PROVENANCE §3 9/9 SHA/行数实测一致；P2/P3 建议见记录 §3） | reviewer-a |
| [REV-095](REV-095-process-v02-release.md) | 2026-11-01 | process/docs（含 cli 测试 1 行） | `chore/process-v02-release` | 通过（v0.2.0 版本 bump/Cargo.lock 本地 7 crate、release `--version`=0.2.0、perf baseline 数字+门禁、workspace 915/0/3、e2e 106/0/2、pytest 170、check.sh；无 dist/token；P2/P3 见记录 §3） | reviewer-b |
| [REV-096](REV-096-docs-v03-design.md) | 2026-11-01 | docs | `docs/docs-v03-design` | 通过（首轮有条件通过 → C1–C5 + 6 项跨文档残留由 `80a42c1`/`7659ec8` 关闭后复审通过；v0.3 V1 设计冻结：DR-008..011 + spec 00/02/05/06/07 草案 + AC-18/19/20 设计；§5.14 编号保留；P2/P3 见记录 §6.4） | reviewer-a（主审）+ lead（裁决/第二评审） |
| [REV-097](REV-097-harness-skills.md) | 2026-11-01 | harness（含 cli/kernel 机械） | `feat/harness-skills` | 通过（V2 skills：发现/校验/索引注入/`skill_view`/预算/安全；harness 182/0/1、workspace 951/0/3；P2/P3 见记录 §3） | reviewer-b |
| [REV-098](REV-098-harness-mcp.md) | 2026-11-01 | harness（含 cli/kernel/Cargo 机械） | `feat/harness-mcp` | 通过（首轮有条件通过 → C1/C2 + P2 加固由 `15f3d18` 关闭后复审通过；V3 MCP：rmcp 3.8.0 feature-gated 双 era/registry/`--allow-mcp`/工具桥接/`close_mcp` 幂等；harness 207/206、workspace 976/0/3；P2⑥ 残余与 P3 见记录 §6.3） | reviewer-a（主审）+ lead（裁决/第二评审） |
| [REV-099](REV-099-harness-plugin.md) | 2026-11-02 | harness（含 cli/kernel/py-worker/Cargo 机械） | `feat/harness-plugin` | 通过（首轮有条件通过 → C1–C3 由 `3bbe4b5` 关闭；harness 233/0/1、workspace 1003/0/3、pytest 183；Codex adapter/DSH 骨架、llm_calls 1/节点、有界读取/取消无孤儿） | reviewer-b（主审）+ lead（第二评审） |
| [REV-100](REV-100-harness-v03-residual.md) | 2026-11-02 | harness | `fix/harness-v03-residual` | 通过（MCP 每 run/server 重启≤1/spawn=2/transport_closed；>32768 截断三键+后缀；http_insecure；Codex cancel 有界等待；harness 235、workspace ×2 1006/0/3、pytest 183） | reviewer-b |
| [REV-101](REV-101-e2e-v03.md) | 2026-11-02 | e2e | `test/e2e-v03` | 通过（V5a 真链路：AC-18 a–f 10 用例、AC-19 a–g 11、AC-20 a–h 7、v0.2 回归 3；e2e 18 target/137/0/2 ignored、workspace 1037/0/3、pytest 183；新语义 spawn=2/transport_closed、truncated JSON、http_insecure 均实测；P3 见记录 §3） | reviewer-a |
| [REV-102](REV-102-docs-v03-spec.md) | 2026-11-02 | docs | `docs/docs-v03-spec` | 通过（首轮有条件通过：C1 C3 race、C2 as-built stale；C1 由 REV-103 `050f86f`、C2 由 `adb687e`+`df37762` 关闭；AC 31、PROVENANCE §3 7419/7822/9/9、workspace ×2 1037/0/3；见记录 §6/§9） | reviewer-b |
| [REV-103](REV-103-harness-codex-cancel-race.md) | 2026-11-02 | harness（测试侧） | `fix/harness-codex-cancel-race` | 通过（REV-102 C1：删除固定 600ms，canceller 有界等待 pid 文件 ≤10s 后再 cancel；超时/无 pid 先 close_provider 清理；原断言全保留；仅 plugin.rs 测试 +25/−22；隔离 ×5、harness 235/0/1 双构建、workspace ×2 1037/0/3、pytest 183、clippy/fmt/check/no-default 全绿） | reviewer-a |
| [REV-104](REV-104-process-v03-release.md) | 2026-11-02 | process/docs | `chore/process-v03-release` | 通过（v0.3.0 发布收口：version 0.3.0 + 7 本地 crate lock；release `--locked`/`--version`=`zgraph 0.3.0 (DSL 1.0)`；perf-baseline-v0.3 与 Lead 实测一致（AC-01 +0.0%/AC-12 −4.0%/smoke −7.3%/validate +15%，门禁通过）；README 235/1037/137/183/31；workspace 1037/0/3（首次 known ISSUE-091 flake 复跑绿）；check.sh；已更新 index/reviews） | reviewer-a |
| [REV-105](REV-105-docs-v04-design.md) | 2026-11-02 | docs | `docs/docs-v04-design` | **通过**（v0.4 D1 设计冻结：DR-012/013/014 + spec 00/02/05/06/07/08 草案；首轮有条件通过（C1–C4：allow-path 只读、bash 形式/上限、read/v7 限额、hardlink/firmlink）→ `923a89a`+`2132478` 关闭后复审；v0.4 旧值 grep 清零、`check.sh` 绿、spike §5/§11 已合入；P2/P3 见记录 §3/§8） | reviewer-a |
| [REV-107](REV-107-harness-builtin-search.md) | 2026-11-02 | harness（Cargo 依赖跨模块） | `feat/harness-builtin-search` | 通过（v0.4 D3 grep/glob：ignore/.gitignore/隐藏/二进制/2MiB、排序/1000/32KiB/单条 text 4K、glob 排序/隐藏/1000/5000/32KiB、jail allow-path 只读/越界、7 工具注册/streaming/sink；harness 293/0/1、workspace 1096/0/3 ×2、pytest 183、clippy/fmt/check/no-default 全绿；P3 见记录 §3） | reviewer-a |
| [REV-109](REV-109-trace-v7.md) | 2026-11-02 | trace（含 cli 机械） | `feat/trace-v7` | **通过**（v0.4 D5a schema v7 + tool_calls：DDL/迁移/备份/链式/非事件路径/writer 幂等+10000 行上限/32KiB+16KiB 截断/无 blobs；首轮有条件通过 → `90b6738` 关闭 C1 `(started_at,call_id)` 排序/C2 `error_sha256`/P3 上限前置，C3 按 Lead 裁决以 DR-014 §1 无 node_id；trace 113/0、migration 21/0、tool_calls 10/0、workspace 1151/0/3、clippy/fmt/check 绿；P3 见记录 §3/§8） | reviewer-a |
| [REV-111](REV-111-cli-tool-replay.md) | 2026-11-02 | cli（含 harness 小扩展/e2e 机械） | `feat/cli-tool-replay` | **通过**（v0.4 D5b2：bridge 落库+二次脱敏/context/source-status fail-closed、trace human+NDJSON ToolCall/stats>0、replay --mode=tools recorded 校验/无副作用探针、真链路 1 行；首轮有条件通过 → `d4da442` 关闭 C1 `--replay-tools` events/outputs 忽略 + sink 预算/告警/P3；cli 249/0、tool_replay 16/0、workspace 1187/0/3、clippy/fmt/check/no-default 绿；P3 剩余 D6 见记录 §3/§8） | reviewer-a |
| [REV-113](REV-113-process-v04-release.md) | 2026-11-03 | process/docs | `chore/process-v04-release` | **通过**（v0.4.0 发布收口：version 0.4.0/7 lock、release --locked/--version、perf 5-trial 与 Lead 一致、spec as-built/PROVENANCE 7640/8055 9/9、e2e 166/23/2 + 29 用例、workspace 1216/0/3、pytest 183；首轮有条件通过 → `af8303a` 关闭 C1（harness 340 五处）/P3 ①–⑤；见记录 §6/§8） | reviewer-a |
| [REV-114](REV-114-docs-w5-removal.md) | 2026-11-03 | docs | `docs/docs-w5-removal` | **通过**（取消 W5：DR-015 + README/progress/spec00/DR-004/007/008/012/SOP/index/issues 状态注记；docs-only、check.sh 绿、历史保留；首轮有条件通过 → `a632806` 关闭 index 三处/P3 ①–③；见记录 §6/§8） | reviewer-a |
| [REV-115](REV-115-docs-v05-design.md) | 2026-11-03 | docs | `docs/docs-v05-design` | **通过**（v0.5 F0：DR-016–020 + spec 00/02/05 §5.16/06 §6.16/07 AC-25–28/08 §8.14；docs-only、check.sh 绿、spike 事实一致；首轮有条件通过 → `554aad0` 关闭 C1 错误码/C2 Codex 状态索引/C3 预算码 reason/C4 Codex 命令；见记录 §6/§8） | reviewer-a |
| [REV-117](REV-117-docs-v05-codex-fix.md) | 2026-11-03 | docs | `docs/docs-v05-codex-fix` | **通过**（Codex spike FINAL 勘误：tool 事件在 `-s danger-full-access`/非嵌套可得→tool_start/tool_end、ok=exit_code==0；嵌套默认 sandbox 静默 exit71 假绿；F5d 9 项/AC-28 g/i/j/零密钥命令；docs-only 9 文件、check.sh 绿、spike 344 行一致；P3 3 项见记录 §3） | reviewer-a |
| [REV-118](REV-118-cli-reexec.md) | 2026-11-03 | cli（含 trace helper/e2e 机械） | `feat/cli-reexec` | **通过**（v0.5 F1b：`--replay-tools=reexec` 显式/前置 flag/只读四工具/result_sha 漂移 MISMATCH/副作用·MCP·Python 行复用记录/events·outputs 忽略/recorded 不变；trace 公共 sha 与 writer 同口径；cli 255/0、tool_reexec 6/0、workspace 1234/0、clippy/fmt/check/no-default 绿；P3 D6 文档口径见 §3/§7） | reviewer-a |
| [REV-119](REV-119-docs-reexec-clarify.md) | 2026-11-03 | docs | `docs/docs-v05-reexec-clarify` | **通过**（reexec 口径对齐 F1b：候选/非候选复用/exit2 范围/additive/MISSING=缺 sha；DR-017/05/06/07/08/index 一致；首轮有条件通过 → `f1050a9` 关闭 C1 progress/C2 DR-016/P3 历史注记；见记录 §6/§8） | reviewer-a |
| [REV-122](REV-122-docs-reexec-sha.md) | 2026-11-03 | docs | `docs/docs-v05-reexec-sha` | **通过**（reexec 缺 sha 修正：行存在字段缺失/损坏→MISMATCH；MISSING 仅行缺失/streaming 多于记录；与 F1c ac25e/validate 一致；首轮有条件通过 → `989a062` rebase 到 main ec7da51 + F1c 状态同步；见记录 §6/§8） | reviewer-a |

命名：`REV-<NNN>-<module>-<slug>.md`；模板见 [`TEMPLATE.md`](TEMPLATE.md)。

- `merge.sh` 会查找包含 `分支: <branch>` 且 `结论：通过` 的记录，找不到则拒绝合并。
- `有条件通过` 必须在对应评审记录追加条件关闭记录后再合并。
- 评审记录不得删除；结论变更用追加「修订」段。
- REV-012（worker 分支含 kernel 访问器与 Cargo.lock）与 REV-015（e2e 分支含根 Cargo.toml/Cargo.lock）属 Lead 批准的跨模块改动，均由 reviewer-b 追加第二评审小节。
- M4：REV-017（process 依赖）、REV-018（kernel 错误码/ContextOverflow）、REV-021（worker 装箱）由 Lead 提交、reviewer-b 评审；REV-020/022/023 分别由 harness-dev/e2e-dev/docs-writer 提交、reviewer-b 评审；REV-024 由 Lead 提交、reviewer-b 评审。REV-020/022 含 Cargo.lock 跨模块变更，已在评审记录中声明。
- M5（已合并到 main）：REV-025（process/docs kickoff+DR-002，lead/reviewer-a）、REV-026（kernel，kernel-dev/reviewer-a）、REV-027（harness，harness-dev/reviewer-b）、REV-028（cli，cli-dev/reviewer-a）、REV-029（e2e，e2e-dev/reviewer-b）、REV-030（cli replay/L2/标定，cli-dev/reviewer-b）、REV-031（kernel/worker/cli/harness ISSUE-066，worker-dev；reviewer-a 主审 + reviewer-b 第二评审）、REV-032（e2e CLI/L2/标定，e2e-dev/reviewer-b）、REV-033（harness，harness-dev/reviewer-b）、REV-034（trace，trace-dev/reviewer-a）、REV-035（docs spec 回写，docs-writer/reviewer-a）、REV-036（docs/process 收尾，lead/reviewer-b）、REV-038（process/docs 台账，lead/reviewer-a）。REV-028/030 含根 `Cargo.lock`、REV-025/035/036 含根 `README.md`（process）跨模块变更；REV-031 为 kernel/worker/cli/harness 跨模块（Lead 批准、双评审）；REV-032 含 e2e dev-dep `rusqlite` 根 `Cargo.lock` 机械改动。REV-037（process 依赖治理/CI）顺延 M6。

- M6（已全部合并到 main，2026-10-31）：REV-039（kickoff/DR-003）、REV-040–045（W1）、REV-046（W1 收口）、REV-047–056（W2/W3：fail-stop、runs 两列、global_vars、human 实时、079 共享限流、081 拆分、086 e2e、088 主机加固）、REV-057（spec 回写）、REV-058（merge 门禁 clippy，计划外）、REV-060（e2e 工具共享上限适配，计划外）、REV-061（wrapup）。**REV-051、REV-059 未创建**（ISSUE-080 并入 REV-057；059 编号未使用）。跨模块：REV-047/048/049/052 为多模块（Lead 批准 + 第二评审 reviewer-a）；REV-057/058/061 为 docs/process。

- W2b（已全部合并到 main，2026-10-31）：REV-074（dsl `max_fanout` + R16，dsl-dev/reviewer-a）、REV-075（kernel 运行期每激活扇出上限 + `NODE_FANOUT_EXCEEDED` + cli 机械 arm，kernel-dev；reviewer-b 主审 + lead cli 第二评审）、REV-076（e2e AC-15 静态/正例/运行期 test-hooks，e2e-dev/reviewer-a）、REV-077（docs spec 回写 00/01/02/03/06/07 + PROVENANCE 第 18 条 + AC-15/索引/台账，docs-dev/reviewer-b；首轮有条件通过，C1–C4 于条件修复提交关闭后复审通过）。

- W3a0（已全部合并到 main，2026-10-31）：REV-078（docs 设计冻结：DR-006 + spec 00/01/02/03/04/06/07 冻结 + AC-16 a–g 设计 + trace schema v5/decide 契约，docs-dev/reviewer-b；首轮有条件通过 → C1–C5 关闭 + R1–R4 收口后复审通过）。下一步 W3a 实现（trace v5 → kernel WaitingHuman → cli decide/consumer → e2e AC-16）。

- W3a（已全部合并到 main，2026-11-01）：REV-079（trace v5/`human_decisions`，trace-dev/reviewer-a；e2e 机械第二评审 lead）、REV-080（kernel `WaitingHuman`/`builtin.human`/`HumanDecision`，kernel-dev；reviewer-b + lead cli 第二评审）、REV-081（cli `decide`/consumer/trace 决策行，cli-dev/reviewer-a）、REV-082（cli human L2 记录/replay 适配，cli-dev/reviewer-a）、REV-083（e2e AC-16 a–g 真链路，e2e-dev/reviewer-b）；合并点 `adf54a1`→`390dbe1`→`971e48f`→`9006345`→`2444548`；docs as-built 回写为 REV-084。下一步 W4a（W3b durable resume 可选，需用户确认）。

- W4b（已全部合并到 main，2026-11-01）：REV-088（dsl `foreach`/R17/R18/19 码）、REV-089（trace schema v6/item 列/expansions/真实 v1→v6）、REV-090（kernel controller/item/join/4 码）、REV-091（cli ForeachExpansion/L2 per-item）、REV-092（kernel items_port 整包 unwrap 规则 A）、REV-093（e2e AC-17 a–j 21 用例）；合并点 `45cd7c6`→`948d677`→`519066d`→`4a1ff0f`→`683260b`→`201341b`；docs as-built 回写为 REV-094。下一步发布收口（性能基线/兼容矩阵/tag v0.2.0）。
