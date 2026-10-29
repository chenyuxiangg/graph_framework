# REV-009 · cli · M2 `zgraph trace` L1 回放 / kernel-trace 桥 / AC 验收

> **模块声明**：本分支只改 `crates/zgraph-cli/**` + 该 crate dev-dependency `rusqlite` + `Cargo.lock` 中 zgraph-cli 依赖列表 1 行；未触及 kernel/trace/dsl 实现（kernel 修复来自 REV-011，已合并 main）。作者 `cli-dev`，评审人 `reviewer-a`（非作者）。

- **分支**：`feat/cli-m2-trace`
- **作者**：`cli-dev`
- **评审人**：`reviewer-a`
- **变更范围**：`2d9d4be..c770b06`（3 commits：`8a6e0dd` trace L1 回放 → `7a51331` bridge + AC 验收 → `c770b06` AC-06 实例级断言；`git diff --stat` = 9 files, +3525/−92：`bridge.rs` +720、`trace_cmd.rs` +766、`lib.rs` +180/−92、`main.rs` 重写、`validate.rs` 可见性、`tests/{acceptance_m2,trace}.rs` 新增）
- **评审时间**：2026-10-29
- **关联**：spec 06 §6.8.3（中断/进行中 run 的 trace 输出）/§6.10.4（trace 契约）/§6.10.5（退出码）/§6.10.6（NDJSON）/§6.11.2（stats）；architecture-v2 §5.3（CLI 契约）/§6.1（M2 DoD）；spec 07 AC-01/02/03/05/06/07/10/11；REV-011（kernel 超时错误回填）

## 1. 结论

**结论：通过**

实测与独立探针均未发现阻塞/重要问题；M2 DoD 的 `zgraph trace` L1 回放、kernel→trace 桥、AC 验收数据面成立。关键证据：

1. **`zgraph trace` 契约**（全部经真实二进制 `CARGO_BIN_EXE_zgraph` + 独立 tempdir 库验证）：`--db > ZGRAPH_DB > <cwd>/.zgraph/trace.db` 优先级、只读不建库（缺失库 exit 4 且文件未被创建）、`ORDER BY seq ASC`、`--since-seq/--limit/--kinds/--no-streaming`、human 头部/表格/各 kind detail（Transition/GateResult/Streaming 五变体/NodeTimedOut/NodeAbandoned/RunFinished）、`--json` 纯 NDJSON（stdout 仅事件、seq 严格递增、字段与 06 §6.10.6 一致）、`--stats` human+JSON 数字逐项（events/kinds/instances/streaming/timeouts/abandoned/backpressure/db path/size/runs）、interrupted 截断提示 + exit 1（human/json/stats）、`STORAGE_ERROR`/`STORE_SCHEMA_TOO_NEW` → exit 4、用法错误 → exit 2；M1 `validate` 8 个集成 + 4 个单元测试全绿、行为未回归（`validate.rs` 仅可见性变化）。
2. **bridge（kernel→trace）**：8 个 `TraceSink` 方法逐字段映射（含 Transition 的迁移后 snapshot、RunFinished metrics/error、`node_error_json` 保留 Python traceback、streaming 走 `send_streaming` 返回 `false` 让内核计数、EdgeIndex usize→i64、RecordMode/状态枚举逐项映射）。独立探针用**原始 SQL** 验证：AC-06 超时链路的 `instances.error/error_code`、`transitions.error_code/error`、`NodeTimedOut.elapsed_ms` 三处同值（103ms）且同形；AC-05 PythonException 的 traceback 落库、Crash 迁移 error 同形、run.error_code 正确。启动校验失败：DB runs/events 均为 0，且同一 DB 可再次打开写端并成功 run（无资源泄漏）。trace 写失败时 `wait` 返回 `BridgeError`（不返回“成功” outcome）。
3. **AC 验收质量**：AC-03 迁移矩阵是测试内独立复刻（未复用 kernel `can_transition_to`）；L1 用真实二进制 `zgraph trace --json` 与 `TraceReader.events` 逐条比对（seq/kind/at/severity/run_id/payload/instance/node）；AC-06 在 REV-011 后新增**实例级 + 迁移级**错误回填断言（含 elapsed_ms 与 NodeTimedOut 一致、`instances.error == transitions.error`、abandoned_tasks=0、killed_workers≥1、无 NodeAbandoned），与 07 AC-06/D1 改写口径一致；AC-07 tick gap <50ms + Pause <100ms；AC-11 disable_settle 真触发 RUN_DEADLOCK。验收连跑 3 次均 10/10 通过，无 flaky。
4. **门禁数字**：`cargo test -p zgraph-cli` = **50 passed / 0 failed**（lib 5 + trace_cmd 8 + bridge 6 + validate 12 + trace 9 + acceptance 10）；`cargo test --workspace` = **249 passed / 0 failed**；fmt/clippy（workspace all-targets -D warnings）/check.sh/pytest 20 全过。

发现的 12 项均为**建议级**（含 06 §6.8.3 进行中 run 提示缺口、bridge NodeError JSON 形状需 spec 回写、AC 口径边界等，见 §3），不构成合并条件；需 Lead 确认登记 ISSUE。

## 2. 四维评审

| 维度 | 结论 | 依据（文件/行/测试） |
|---|---|---|
| 功能正确性 | ✅ | `trace_cmd`/`bridge` 全部实测（真实二进制 + 原始 SQL 探针）：路径优先级/只读/seq/过滤/NDJSON/stats/截断/退出码逐项符合 06 §6.8.3/§6.10.4/§6.10.5/§6.11.2；AC-01/02/03/05/06/07/10/11 数据面断言真实（含 REV-011 错误回填 SQL 断言）；M1 validate 无回归。缺口为建议级（#1/#2/#7 为 06 输出细节，不影响 L1 回放）。 |
| 代码结构合理性 | ✅ | `lib.rs`（分发/退出码）+ `trace_cmd.rs`（只读回放/渲染）+ `bridge.rs`（唯一 kernel→trace 适配器）+ `validate.rs` 边界清晰；bridge 单一职责、无重复映射；测试用真实二进制而非仅内部 API。建议：`validate` 的 `pub` 可见性可回收（#10）、bridge NodeError JSON 形状需固定契约（#3）。 |
| 逻辑性 | ✅ | 只读打开不建库/不迁移；事件严格 seq 排序、NDJSON 纯净（诊断走 stderr）；退出码映射（Completed/Cancelled/Running=0、Failed/interrupted=1、storage=4、usage=2）自洽；bridge `wait` 先 flush/close 再开 reader，写失败返回 Err（不伪装成功）；启动失败关闭 writer。边界：进行中 run 缺提示（#1）、`--stats --json` 无截断标记（#2）、trace 写失败无法 retro-fail 已 Completed 的 run（#4，M5 接线）。 |
| 可读性 | ✅ | 模块/常量/函数文档引用 spec 章节与行号；各 kind detail 拆分为小函数、命名自解释；bridge 每个映射函数有注释；测试名/断言信息可读。建议见 #10（无谓 pub）/ #11（补用例）。 |

## 3. 发现清单

| # | 级别 | 位置 | 问题 | 建议 | 状态 |
|---|---|---|---|---|---|
| 1 | 建议 | `src/trace_cmd.rs:187-210`（`run`）；`zgraph-trace` `RunRow` | 06 §6.8.3 规定 `status='Running'` 且 pid 仍活时输出 `run still in progress (pid N)` 且 exit 0；实现直接渲染时间线/统计并 exit 0，**无该提示**，且 `RunRow` 不含 `owner_pid`（trace crate 缺口）导致 CLI 无法给 pid。 | 与 trace crate 联动补 `RunRow.owner_pid`（跨模块改动需 Lead 声明），或在 CLI 头部提示 `run still in progress` 并登记 ISSUE；补 Running 场景用例。 | open（建议） |
| 2 | 建议 | `src/trace_cmd.rs:361-381`（`run_stats`）、`:236-240`（json 截断） | `--stats --json` 对 interrupted run 仅靠退出码 1 表达截断，stdout JSON 无 `truncated`/`interrupted` 标记（human 与 human-stats 有提示；`--json` 提示走 stderr 可接受）。 | stats JSON 增加 `"truncated": true`（或 `"interrupted": {...}`）字段；或在 06 §6.11.2/§6.8.3 回写“仅退出码表达”；补用例。 | open（建议） |
| 3 | 建议 | `src/bridge.rs:457-495`（`node_error_json`） | bridge 自定扁平 JSON 形状（`code`=框架码 + `kind` + `business_code`/`exception`/`traceback`/`harness_kind`/`elapsed_ms`），与 00 §0.3 `NodeError` 枚举形状不同；06 §6.2.5/§6.2.7 注释为“JSON: NodeError”但无 canonical schema。AC-05/AC-06 已用断言锁定该形状（traceback/elapsed 均保留，未吞信息）。 | 登记 ISSUE 并在 06 §6.2.5/§6.2.7 回写该 JSON schema（或改 bridge 输出 tagged union 并同步 AC 断言）；避免未来回放器按 00 枚举解析失败。 | open（建议） |
| 4 | 建议 | `src/bridge.rs:204-225`（`TracedRun::wait`） | trace 写失败时返回 `BridgeError` 并丢弃 `RunOutcome`（kernel run 可能已 Completed）。06 §6.4.5 fail-stop 要求 run 标 Failed + CLI exit 4；M2 无 `run` 命令故无用户面，但 M5 `zgraph run` 必须把 BridgeError 映射 STORAGE_ERROR/exit4，且不得先报成功。 | bridge 文档明示“Err 时调用方不得回报 run 成功”；M5 CLI run 显式映射；登记 ISSUE/遗留。 | open（建议） |
| 5 | 建议 | `tests/acceptance_m2.rs`（AC-01/02/06/07） | AC-01/02 用 in-process bridge + FakeExecutor 验证 kernel→trace 数据面，未执行 07 的 `zgraph run --fake-llm` 步骤，也未验证 07 AC-01 的 `run_output.result.content`（Harness 输出形状，M4）；AC-06 未验证“进程组 kill/无残留子进程”（M3），用 test-hooks `force_node_timeout_ms` 替代 DSL `timeout_ms=1000`；AC-07 用 in-process sleep executor 替代 Python 节点。属 M2 里程碑边界（数据面成立），但 07/architecture 的 DoD 口径需回写/登记 ISSUE，避免“AC 已完整通过”误读。 | 在 07/architecture 回写 M2 验收边界（M2=kernel+bridge+trace 数据面；CLI `run`/进程组/Harness 属 M3–M5）；登记 ISSUE。 | open（建议） |
| 6 | 建议 | `tests/acceptance_m2.rs:466-512` | AC-03 迁移链断言合法/连续/首迁移/末态；但未断言 `transitions.seq` 与 `events(Transition).seq` 1:1（trace crate 有该测试，CLI 验收可加强）。 | 增加 1:1 断言（低成本）。 | open（建议） |
| 7 | 建议 | `src/trace_cmd.rs:314-324`（human node 列） | `NodeTimedOut/NodeAbandoned` 的 `events.node_id` 为 NULL（06 §6.2.12 规定查询时 `JOIN instances` 取 node_id），CLI 表格 node 列显示 `-`；人类回放对超时/放弃事件缺少节点定位。 | CLI 渲染时按 instance_id 关联 instances 补 node_id（或 trace reader 提供 join 视图）；登记 ISSUE（与 REV-008 #7 联动）。 | open（建议） |
| 8 | 建议 | `src/trace_cmd.rs:205-209,361-381` | `--stats` 下 `--kinds/--since-seq/--limit/--no-streaming` 被静默忽略，参数语义不明确。 | 对互斥组合给用法错误或 stderr 提示；文档/ISSUE 说明。 | open（建议） |
| 9 | 建议 | `src/bridge.rs:710-719`（`start_error_does_not_leave_open_writer`） | 该用例只断言 `BridgeError::Start`，未断言 DB 无 run/事件、writer 已关闭/可复用；独立探针已验证当前行为正确（runs=0/events=0、同库重开写端并成功 run），建议把断言并入用例防回归。 | 补 DB 行数与二次打开断言。 | open（建议） |
| 10 | 建议 | `src/validate.rs:13,23,55` | `pub(crate)` → `pub` 仅为同 crate 调用/测试并不需要，扩大 CLI lib 公开面（非冻结 API）；行为无变化（validate 12 个测试全绿）。 | 若无外部调用需求，恢复 `pub(crate)`；避免无谓 API 面。 | open（建议） |
| 11 | 建议 | `tests/trace.rs`（覆盖缺口） | 未覆盖：Running/Paused run 的 trace 输出与提示（#1）、`--stats --json` interrupted（#2）、`--db :memory:`、默认路径父目录缺失、NodeTimedOut 的 node 列（#7）。 | 按发现补 2–3 个用例；不阻塞。 | open（建议） |
| 12 | 备注 | 全量门禁 | `cargo test -p zgraph-cli` 50、workspace 249、acceptance 连跑 3 次 10/10、fmt/clippy/check.sh/pytest 20 全过。 | 无失败项。 | — |

> 无阻塞项、无重要项；#1–#11 为建议级（#1/#3/#4/#5 需 Lead 确认登记 ISSUE 或纳入 REV-010 台账与 06/07 回写）。

## 4. 验证证据

### 4.1 范围与分支

```bash
cd /Users/zyzs/lczyzs/code/zgraph/.worktrees/cli-cli-dev
git branch --show-current                # feat/cli-m2-trace
git log --oneline -4
# c770b06 test(cli): AC-06 补实例/迁移超时错误回填断言
# 7a51331 feat(cli): M2 kernel-trace 桥与 AC-01/02/03/05/06/07/10/11 验收
# 8a6e0dd feat(cli): zgraph trace L1 回放
# 2d9d4be docs(review): REV-011 kernel 超时错误回填评审（= origin/main）
git merge-base origin/main HEAD            # 2d9d4be（基于最新 main）
git diff --stat 2d9d4be..c770b06           # 9 files changed, 3525 insertions(+), 92 deletions(-)
git diff --name-only 2d9d4be..c770b06 | grep -vE '^(crates/zgraph-cli/|Cargo.lock$)' || echo "(only cli + lock)"   # (only cli + lock)
git status --short                         # 评审提交前空（临时探针已删除）
```

3 个提交 message 均含 `Review: REV-009` / `Agent: cli-dev`；`Cargo.lock` 仅 `zgraph-cli` 依赖列表 +`rusqlite` 一行。

### 4.2 必跑命令（worktree 内，`CARGO_HOME=ROOT/.cargo-home`）

```bash
cargo fmt --all -- --check                        # exit 0
cargo clippy --workspace --all-targets -- -D warnings   # exit 0
cargo test -p zgraph-cli                          # 50 passed / 0 failed
# lib 5 + trace_cmd 8 + bridge 6 + validate 4+8 + tests/trace 9 + acceptance_m2 10
cargo test --workspace                            # 249 passed / 0 failed
bash scripts/dev/check.sh                         # 13 项 OK → CHECK PASSED
/Users/zyzs/lczyzs/code/zgraph/.venv/bin/python -m pytest   # 20 passed
# acceptance 连跑 3 次：每轮 10 passed / 0 failed，无 flaky（0.75s/轮）
```

### 4.3 独立探针（临时 `tests/zz_reviewer_probe.rs`，跑完已删除，未入库）

| 探针 | 方法 | 实际结果 |
|---|---|---|
| P1 AC-06 | bridge 真跑（force 100ms 超时）→ **原始 rusqlite SQL** 查 runs/instances/transitions/events → 真实 `zgraph trace --json`/`--stats --json` | runs: Failed/NODE_TIMEOUT/abandoned=0/killed=1/finished_at 非空；instances(slow): TimedOut/NODE_TIMEOUT/error JSON `{code:NODE_TIMEOUT,kind:Timeout,elapsed_ms:103,message:…}`；transitions(TimedOut): error_code=NODE_TIMEOUT、error 与 instances.error **逐字段相同**、elapsed=103；NodeTimedOut 事件恰 1、payload elapsed=103 == 错误 elapsed；NodeAbandoned=0；首个 RunStarted/末个 RunFinished、RunFinished 恰 1；`trace --json` exit **1**（Failed，06 §6.10.5）、stderr 空、NDJSON 行数 == SQL 事件数、seq 严格递增、elapsed 与 SQL 一致；`--stats --json` timeouts=1/abandoned=0/dropped=0、duration 与 reader 一致 |
| P2 AC-05 | bridge 真跑 PythonException → 原始 SQL | runs.error_code/instances.error_code/transitions(Crashed).error_code 均 `NODE_PYTHON_EXCEPTION`；instances.error JSON `kind=PythonException` 且 **traceback 全文保留**；transitions.error 与 instances.error 同形 |
| P3 启动失败资源 | `run_in_process` 用非法 entry_inputs → 原始 SQL + 再打开 writer/再跑 | `BridgeError::Start(RUN_ENTRY_INVALID)`；runs=0、events=0；同一 DB `TraceWriter::open` 成功并 close；随后合法 run → Completed（无残留 writer/lock） |

### 4.4 手工核对（对照 06 / 07 / architecture）

| 核对项 | 预期 | 实际 |
|---|---|---|
| DB 路径优先级 | `--db` > `ZGRAPH_DB` > `<cwd>/.zgraph/trace.db` | ✅ `tests/trace.rs::db_path_priority_env_and_default`（三种路径各实测） |
| 只读/不建库 | 缺失库 exit 4 且不创建 | ✅ 缺失库/缺 run 用例 + `missing_db.exists()==false` |
| 事件顺序 | 严格 `seq ASC`（禁按 at） | ✅ human/json 用例 + P1 |
| 过滤参数 | since/limit/kinds/no-streaming（含归一化与空结果） | ✅ `filters_limit_kinds_and_streaming_toggle` |
| NDJSON | stdout 纯 JSON 行、诊断 stderr、字段含 seq/at/kind/severity/run_id/payload | ✅ `json_replay_is_ndjson_ordered_by_seq` |
| human 表格 | 头部两行 + seq/at/kind/node/instance/detail，各 kind 明细 | ✅ `human_replay_renders_header_timeline_and_details`（4+16 行计数、无 ANSI） |
| stats | human 各数字 + `--json` 结构按 06 §6.11.2 | ✅ `stats_human_and_json_report_numbers` |
| 截断/退出码 | interrupted 提示 + exit1；storage/too-new exit4；usage exit2；Failed=1 | ✅ 对应用例 + P1（Failed trace exit1） |
| kernel→trace 映射 | 8 个 sink 方法逐字段（snapshot/metrics/error/streaming 返回值） | ✅ bridge 单测 + P1/P2 SQL 验证 |
| 启动失败资源 | 无事件、writer 关闭 | ✅ `start_error_does_not_leave_open_writer` + P3 |
| M1 validate 回归 | 8 集成 + 4 单元全绿 | ✅ 包含在 50 中；`validate.rs` 仅可见性 diff |

### 4.5 测试质量

- 用真实二进制（`CARGO_BIN_EXE_zgraph`）而非仅内部 API；DB 用独立 tempdir；NDJSON 逐行解析；stats 数字逐项断言；AC-03 迁移矩阵在测试内独立复刻（未复用 kernel 实现）、L1 与 reader 逐条比对；AC-06 直接查 DB 行（含 REV-011 错误列与 elapsed 一致性）；AC-07 有真实 ticker probe 与 Pause 计时；AC-11 disable_settle 真触发 → 未见“为测试而测试”或弱化断言。
- 缺口见 #1/#2/#7/#11（未覆盖 running/paused trace、stats-json 截断、NodeTimedOut node 列）。

## 5. 遗留问题

- **无阻塞项**；建议级 #1–#11，重点 #1（06 §6.8.3 进行中 run 提示，需 trace crate 补 owner_pid 或 CLI 简版提示）、#3（bridge NodeError JSON 形状需 spec 回写）、#4（trace 写失败 fail-stop 的 M5 CLI 映射）、#5（M2 AC 口径边界需 07/architecture 回写）。需 Lead 确认登记 `doc/issues.md`（编号由 Lead 分配）。
- **跨里程碑/跨模块**：AC-01 Harness 输出形状（M4）、AC-06 进程组 kill/无残留（M3）、CLI `run`/`replay` 命令（M5）、trace 写失败 fail-stop 接线（M5）、`RunRow.owner_pid`（trace crate 后续，需 Lead 声明跨模块）。
- **流程**：本分支未更新 `doc/progress.md`/`doc/issues.md`；合并时由 Lead 按 AGENTS §5/§7 更新（REV-010 可一并处理）。未 push/merge，`main` 未被本分支改动。
- 待办衔接：REV-010（docs 进度/台账）待 Lead 消息。

## 6. 条件关闭记录

不适用：本记录结论为 **通过**，无前置条件。
