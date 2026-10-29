# REV-008 · trace · M2 SQLite trace 存储 / 单写线程 / Streaming 聚合 / 只读 reader

> **模块声明**：本分支只改 `crates/zgraph-trace/**`（9 个 `src` 文件 + 6 个测试文件 + 本 crate 的 dev-dependencies 3 行），未触及 kernel/cli/dsl/文档；符合 AGENTS §2 的 trace 模块边界。作者 `trace-dev`，评审人 `reviewer-a`（非作者）。

- **分支**：`feat/trace-m2-sqlite`
- **作者**：`trace-dev`
- **评审人**：`reviewer-a`
- **变更范围**：`bfec987..16f2a00`（3 commits：`bfaf63a` 实现 / `f2c2109` 集成测试 / `16f2a00` 文档示例清理；`git diff --stat` = 16 files, +5351/−2；无 `src` 之外改动，`Cargo.lock` 未变）
- **评审时间**：2026-10-29
- **关联**：spec 06 §6.1–§6.11（DDL/PRAGMA/迁移/seq/severity/写路径/Streaming 聚合/只读 reader/CLI 契约）；architecture-v2 §5.2（v2 增量）/§6.1（M2 DoD）/§8 D1/D9；spec 00 §0.3/§0.6 I5/I6；spec 01 §1.5；spec 07 AC-03/AC-06

## 1. 结论

**结论：通过**

依据（均为本轮独立复跑/独立探针，未直接采信作者自述）：

1. **Schema 与迁移**：10 张表（6 主 + `schema_migrations`/`graphs` + v2 的 `recorded_outputs`/`llm_calls`）与 20 个索引逐项对齐 06 §6.2；`PRAGMA journal_mode=WAL/synchronous=NORMAL/foreign_keys=ON/busy_timeout=5000/wal_autocheckpoint=1000/temp_store=MEMORY` 齐全（`src/schema.rs:366-380`）。`user_version=0→2` 单事务建库、`1→2` 补列补表、`>2` 拒绝且**不改库**（实测 user_version 与表集合均未变）；只读 reader 对缺失库/空文件报 `STORAGE_ERROR` 且**不创建文件、不迁移**。
2. **seq 与明细一致性**：`events.seq` 由 `AUTOINCREMENT` 在写线程分配（`tx.rs:207-224`），`transitions/gate_results/streaming_events.seq` 同事务 1:1 关联；`runs.last_seq/event_count` 同事务更新；多 run 全局单调、run 内 `ORDER BY seq`。独立压力探针：1000 条 GateResult + 2000 个 token delta → events=1002、gates=1000、streaming 段=1（文本 2000 字符、delta_count=2000、dropped=0），`last_seq/event_count` 一致。
3. **severity 推导**：06 §6.3.3 全部分支（RunFinished Failed/其余、Transition Crashed/TimedOut/Cancelled、GateResult error/warn/info、NodeTimedOut error、NodeAbandoned warn、Streaming tool_call_end ok=false warn）逐项实现并有测试向量。
4. **Streaming 聚合**：token/reasoning 独立缓冲；100 ms 批量边界 / ≥4096 字符 / 非 delta 流式事件 / 实例终态 / RunFinished / 通道关闭均触发 flush；tool_call/progress 先 flush 同实例 delta；`segment_index` 按 `(run_id,instance_id,kind)` 连续；16 KiB 按码点边界截断并 `truncated=1`；`events.payload` 只放摘要（正文在 `streaming_events.text`）；测试用真实文本拼接、delta_count、first_at/last_at、时序断言，非“只查行数”。
5. **reader**：`EventQuery`（since/limit/kinds/include_streaming + `ORDER BY seq ASC`）、`stats`、`recorded_outputs`、`llm_calls` 齐备；L1 回放所需 `events + runs + transitions + gate_results + streaming_events` 均可用。存在 2 个接口/数据缺口（#8）与 2 个健壮性缺口（#7/#9），均建议级。
6. **5 项声明偏差**：单通道 8192、`NodeTimedOut.at` 取输入、Transition 超限返回 Serialize、fail-stop/interrupted 留 M3/M5、RecordMode 小写——逐项独立验证/评估，**作为 M2 临时实现可接受**，但均须在 `doc/issues.md` 登记并回写 spec（本记录 #1–#5；另有 #6/#7/#11 未声明简化同样需登记）。
7. **代码质量**：`#![forbid(unsafe_code)]`；`src/**` 无 `unwrap/expect/panic!/todo!/unimplemented!`；`src` 无 >60 行函数（`tests` 有 7 个函数 >60 行，见 #14）；`cargo clippy --workspace --all-targets -- -D warnings` 0 告警；测试无跳过/无“为测试而测试”。

测试数字：`cargo test -p zgraph-trace` = **30 passed / 0 failed**（api 5 + migration 5 + reader 4 + roundtrip 6 + streaming 9 + doctest 1）；`cargo test --workspace` = **129 passed / 0 failed**（cli 15 + dsl 69 + trace 30 + worker 15）；pytest **20 passed**。

## 2. 四维评审

| 维度 | 结论 | 依据（文件/行/测试） |
|---|---|---|
| 功能正确性 | ✅ | DDL/索引/CHECK/PRAGMA 与 06 §6.2 一致（`src/schema.rs:69-251`）；迁移 0→2/1→2/>2 与只读不建库均有测试（`tests/migration.rs:71-232`）并有独立探针覆盖 >2 与 v1 库；seq/severity/写入同事务/Streaming 聚合/reader 全部实测；`cargo test --workspace` 129 绿；无产品代码跨模块改动。缺口为建议级 #7/#8/#9。 |
| 代码结构合理性 | ✅ | `schema.rs`（DDL/PRAGMA/迁移）→ `model.rs`（冻结领域类型）→ `writer/{mod,thread,stream,tx}.rs`（句柄/事件循环/聚合/事务）→ `reader.rs`（只读查询）职责单一、依赖单向；写连接仅归写线程（`mod.rs:48-56`），符合 06 §6.1.3 S3；公开 API 面向 cli 收敛；无重复实现、无 `src` 越界。建议：reader 补 `graphs` 访问器与 stats 的 runs 总数（#8）。 |
| 逻辑性 | ✅ | 批量循环（≤256 行/100 ms）、Flush/Shutdown 屏障、busy 3×50 ms 重试、fatal 记录一次后继续排空均自洽，无死锁/无忙等；聚合 flush 顺序（非 delta 先 flush、RunFinished 前 flush_all、终态迁移前 flush_instance）保证因果序；fatal 后 fail-stop 与 06 §6.4.5 方向一致（接线留 M3/M5）。边界：RunFinished 未校验终态/唯一性（#9）、Transition 超限整批回滚（#3，已声明）。 |
| 可读性 | ✅ | 模块/字段均有文档并引用 spec 章节；常量集中 `schema.rs:18-54`；命名与 06 §6.2.11 存储值一致；注释解释“为什么”（单通道简化、created_seq NULL、node_id 回填来源）。建议：progress payload 去重正文（#10）、迁移/权限注明 M2 范围（#11）、tests 长函数拆分（#14）。 |

## 3. 发现清单

| # | 级别 | 位置 | 问题 | 建议 | 状态 |
|---|---|---|---|---|---|
| 1 | 建议 | `src/lib.rs:23-28`；`src/schema.rs:29-30`；`src/writer/mod.rs:5-6,48` | 06 §6.4.1/D5 为双通道（无损 8192 + 流式 1024），M2 用单通道 8192；实测语义“无损阻塞、streaming `try_send` 满则丢弃计数”保持，但 streaming 与无损共享容量，背压下 streaming 会更早被丢弃，`streaming_dropped` 口径与双通道不同。 | 接受为 M2 临时简化，但必须登记 ISSUE 并在 M5 拆回双通道；D5 关闭状态在拆分前不改写。 | open（建议，需 Lead 登记 ISSUE） |
| 2 | 建议 | `src/writer/tx.rs:469-492`；`src/model.rs:407-417` | `NodeTimedOut.at` 取生产者输入 `at`（独立探针确认 events.at=输入值 `…03.300Z`），06 §6.3.4 规定取写线程收到时刻。同进程内生产者时间更准确，但与 spec 原文不一致。 | 回写 06 §6.3.4（标注 v2 由生产者携带 at）或 M3 改为写线程时间；登记 ISSUE。 | open（建议） |
| 3 | 建议 | `src/writer/tx.rs:226-242`；`src/writer/tx.rs:302-353` | Transition payload >64 KiB 返回 `TraceError::Serialize`（fatal），实测**整批事务回滚 + 写线程 fail-stop**：同一探针里先发的 RunStarted 也一起丢失（events_after=0）。06 §6.4.2 的语义是“Transition.error 不截断，超限 → Internal”；且本实现 payload 不含 error（error 落 `transitions.error`，不打大小检查），spec 原场景实际不会触发该分支。 | 由 Lead 裁定：(a) 接受 fail-stop 并回写 06 §6.4.2；或 (b) 对 `transitions.error` 大小按 Internal 报错（不截断）。无论哪种，补一条测试锁定该路径（当前测试套件未覆盖）。 | open（建议，需补测试） |
| 4 | 建议 | `src/lib.rs:30-35`；`src/writer/thread.rs:237-252` | fail-stop 的 run 生命周期接线（stderr `error[STORAGE_ERROR]`、取消 run、退出码 4）与 `interrupted=1` 崩溃标定（06 §6.4.5/§6.8）未实现，仅通过 `take_error`/`flush`/`close` 暴露错误。 | 已声明留 M3/M5；登记 ISSUE 并明确“run 层必须以该错误 fail-stop，禁止静默继续”。 | open（建议，已声明） |
| 5 | 建议 | `src/model.rs:116-149`；`src/schema.rs:109` | `RecordMode` 存储小写 `none/hash/full`（architecture §5.3 CLI 字面量一致），但 06 §6.2.11 未定义 v2 列；`runs.record_mode` 无 CHECK，非法值可写入。 | 06/architecture 回写该列取值域；可加 `CHECK (record_mode IS NULL OR record_mode IN ('none','hash','full'))`。 | open（建议） |
| 6 | 建议 | `src/schema.rs:129`；`src/writer/tx.rs:656-660`；`src/reader.rs:137-138` | `instances.created_seq` 恒为 NULL（06 §6.2.5 定义为“实例创建对应 events.seq”）；代码/测试按“M2 允许 NULL”处理，但未列入 5 项声明偏差；L1 回放只能用 transitions 首条 seq 近似创建序。 | 登记 ISSUE；M3 在首条迁移（Pending→Ready/Skipped）回填 created_seq，或写入时取该实例首个事件 seq。 | open（建议） |
| 7 | 建议 | `src/schema.rs:280-303` | 只读 reader 不检查 `PRAGMA user_version`：独立探针 `user_version=99` 仍成功打开并查询（schema 超前静默可读）；v1 库可打开，但 `run()` 运行期直接抛 `no such column: killed_workers`（原始 SQLite 消息）。06 §6.7 的“版本超前拒绝”只覆盖写路径。 | reader `open` 时校验：`>2` 返回 `SchemaTooNew`；`1` 给出显式“需新版本写库迁移”提示；补 migration 测试。 | open（建议） |
| 8 | 建议 | `src/reader.rs:288-317,459-487`；`src/reader.rs`（无 `graphs` 访问器） | (a) `TraceReader` 没有读取 `graphs` 行的 API（`graph_name`/`dsl_json`），而 06 §6.2.3 称其“供回放校验”、§6.10.4 `trace` 头部需要 graph 名；(b) `RunStats` 缺 06 §6.11.2 的“DB 内 runs 总数”字段。 | 与 cli M2 分支（REV-009）先对齐：由 trace crate 补 `graph()`/`db_stats()`（本 crate 后续提交，勿在 cli worktree 跨模块改），或 Lead 明确 CLI 退化口径；登记 ISSUE。 | open（建议，需 Lead 决策） |
| 9 | 建议 | `src/writer/tx.rs:302-353`；独立探针 P1 | `RunFinished` 未校验 status 必须为终态、也不禁止第二条：探针发 `Running` 状态 → `runs.status=Running` 且 `finished_at` 已写；连发 2 条 `RunFinished{Completed}` → 2 条事件（I5 被破坏）。内核契约保证恰好一次，但存储层无防御。 | 写入前校验 `status.is_terminal()`（否则拒绝并记录 fatal/错误）＋终态后拒绝第二条 RunFinished；补测试。 | open（建议） |
| 10 | 建议 | `src/writer/tx.rs:618-646`（`segment_payload` progress 分支） | progress 的 `events.payload` 同时包含 `message`，正文已在 `streaming_events.text`；06 §6.5.3 要求 payload 只放摘要、不重复正文。 | payload 去掉 `message`（保留 `percent`/`segment_index`）；补断言。 | open（建议） |
| 11 | 建议 | `src/schema.rs:329-342`；`src/schema.rs:399-409` | (a) `1→2` 迁移前未按 06 §6.7 做 `VACUUM INTO` 备份（迁移为纯增量、风险低）；(b) DB 文件/目录未按 06 §6.9.1 设 0600/0700（`create_dir_all`/`Connection::open` 用默认 umask）。 | 登记 ISSUE：备份在 M5 迁移工具补；权限在 CLI 建库路径补（或 crate 创建后设权限）。 | open（建议） |
| 12 | 建议 | `src/writer/stream.rs:107-113`；`src/writer/thread.rs:44-49`；`src/writer/mod.rs:60,102-107` | 写线程内存态 `Aggregator.node_ids`/`next_index` 只增不减（多 run 长驻进程会累积）；`streaming_dropped` 是 handle 级计数，多 run 共享 handle 时无法按 run 归因。M2 单 run CLI 无影响。 | M5 多 run：RunFinished 时清理该 run 的 next_index/node_ids；按 run 计数或每 run 独立 handle；登记 ISSUE。 | open（建议） |
| 13 | 建议 | `tests/`（整体） | 测试质量总体真实（severity 向量、seq 1:1、文本无损/字符边界、时序 flush、payload 摘要均有实质断言；未发现“为测试而测试”）。缺失路径：Transition payload 超限（探针已确认行为，未入测试）、reader schema 版本、streaming_dropped 满通道计数、重复/非终态 RunFinished、在飞 heartbeat 刷新、busy 重试。 | 补 2–3 个低成本用例（尤其 #3 行为定案后）；不要求合并前全补。 | open（建议） |
| 14 | 建议 | `tests/roundtrip.rs:17`（247 行）；`tests/reader.rs:11`（126）、`:139`（84）；`tests/roundtrip.rs:338`（76）；`tests/streaming.rs:264`（90）、`:72`/`:356`（67） | architecture §2.2 规定 `crates/**` 遵守“函数 ≤60 行”等价约束；`src/**` 全部达标，但 `tests/**` 有 7 个测试函数超过 60 行（`full_run_roundtrip…` 达 247 行），单函数过长降低失败定位精度。 | 按“构造→发送→断言”拆分长测试（或提取 `common` 断言 helper）；不阻塞合并，可作为后续测试维护项。 | open（建议） |

> 级别判定：**无阻塞项、无重要项**；14 项均为建议级，不构成合并条件，但 #1–#6、#11、#12 须由 Lead 确认登记 `doc/issues.md`（AGENTS §7：偏差不得只留在代码注释），并回写 spec 06 对应章节。

## 4. 验证证据

### 4.1 范围与分支

```bash
cd /Users/zyzs/lczyzs/code/zgraph/.worktrees/trace-trace-dev
git log --oneline bfec987..HEAD
# 16f2a00 docs(trace): 清理 crate 文档示例的未用导入
# f2c2109 test(trace): M2 schema/写读/Streaming 聚合/reader 集成测试
# bfaf63a feat(trace): SQLite v2 schema/单写线程批量事务/流式聚合/只读 reader
git merge-base origin/main HEAD     # bfec987（按 Lead 指示只评 bfec987..16f2a00，不引入 main）
git diff --stat bfec987..HEAD       # 16 files changed, 5351 insertions(+), 2 deletions(-)
git diff --name-only bfec987..HEAD | grep -v '^crates/zgraph-trace/' || echo "(none)"
# (none)  → 无跨模块/文档改动；Cargo.lock 未变，Cargo.toml 仅 +3 行 dev-dependencies
git status --short                  # 评审提交前空
```

提交信息均含 `Review: REV-008` 与 `Agent: trace-dev`；作者身份 `chenyuxiangg <1007423165@qq.com>`。

### 4.2 必跑命令（worktree 内，`CARGO_HOME=ROOT/.cargo-home`）

```bash
cargo fmt --all -- --check                                  # exit 0
cargo clippy --workspace --all-targets -- -D warnings       # exit 0（Finished，无警告）
cargo test -p zgraph-trace
# unittests src/lib.rs: 0 passed
# tests/api.rs:        5 passed
# tests/migration.rs:  5 passed
# tests/reader.rs:     4 passed
# tests/roundtrip.rs:  6 passed
# tests/streaming.rs:  9 passed
# Doc-tests:           1 passed
# 合计 30 passed / 0 failed
cargo test --workspace
# 129 passed / 0 failed（cli 15 + dsl 69 + trace 30 + worker 15；kernel/harness 骨架 0）
bash scripts/dev/check.sh                                    # 13 项 OK → CHECK PASSED
/Users/zyzs/lczyzs/code/zgraph/.venv/bin/python -m pytest    # 20 passed in 0.58s
```

### 4.3 独立探针（临时 `tests/zz_reviewer_probe.rs`，跑完已删除，未入库）

| 探针 | 目的 | 实际结果 |
|---|---|---|
| P1 | RunFinished 终态/唯一性 | 非终态 `Running` 被接受（runs.status=Running、finished_at 已写）；2 条 RunFinished → 2 events（I5 无防御） |
| P2 | Transition 超 64 KiB（70 000 字符 reason） | flush=`Err(Serialize("Transition payload exceeds TRACE_PAYLOAD_MAX_BYTES (70158)"))`；整批回滚：events_after=0、transitions=0；`take_error` 有 `STORAGE_ERROR: …` |
| P3 | reader 遇 schema 超前 | `user_version=99` 仍 `open_readonly` 成功并可查询（version=Ok(99)） |
| P4 | reader 遇 v1 库 | 打开成功，`run()` 报 `STORAGE_ERROR: no such column: killed_workers …` |
| P5 | NodeTimedOut.at 来源 | events.at=`2026-10-28T09:12:03.300Z`（=输入 at），payload={`{"elapsed_ms":1000}`} |
| P6 | 缺 run 的 Transition | `FOREIGN KEY constraint failed` → flush Err + take_error（fail-stop 可观测，FK 约束生效） |
| P7 | 批量/聚合压力 | 1000 GateResult + 2000 token delta：events=1002、gates=1000、段=1、text_len=2000、delta_sum=2000、dropped=0、last_seq=event_count=1002 |

### 4.4 手工核对（对照 spec 06）

| 核对项 | 预期（06 分册） | 实际 |
|---|---|---|
| 表/索引 | 8 张 + v2 2 张，20 索引 | ✅ `tests/migration.rs` 逐一断言（含 20 个索引名） |
| 连接 PRAGMA | WAL/NORMAL/FK ON/busy 5000/autocheckpoint 1000/temp_store MEMORY | ✅ `schema.rs:366-380`；`PRAGMA journal_mode` 用 query_row 消费返回值 |
| 版本迁移 | 0→v2、1→v2、>2 拒绝且不改库 | ✅ 测试 + P3/P4；>2 时 PRAGMA 迁移仍发生在版本检查之后 |
| 只读 reader | 不建库、不迁移、无库报错、`?mode=ro` | ✅ `SQLITE_OPEN_READ_ONLY` + 文件存在性预检；测试断言文件不存在/0 字节且无 `-wal` |
| seq | AUTOINCREMENT、全局单调、明细 1:1、last_seq/event_count | ✅ 测试 + P7 |
| severity | 06 §6.3.3 全表 | ✅ `tx.rs:725-748` + 测试向量 |
| 同事务 | RunStarted→graphs+runs；Transition→events+transitions+instances；RunFinished→events+update runs；Streaming→events+streaming_events | ✅ 单事务内 `write_op` 顺序执行；heartbeat/finished 同事务 |
| Streaming 聚合 | 100 ms/4096 字符/16 KiB 码点截断/先 flush/终态 flush/segment 连续/payload 摘要 | ✅ 6 个 streaming 测试 + P7；字节边界用 `é` 验证码点截断 |
| 只读查询 | events ORDER BY seq、since/limit/kinds/include_streaming、stats/明细表 | ✅ `tests/reader.rs` |
| seed/record_mode/heartbeat/owner | 写入 runs | ✅ roundtrip 断言 seed=42、record_mode=hash、owner_*、finished 后 heartbeat=finished_at |
| L1 回放数据 | events+runs+transitions+gates+streaming | ✅ reader 全部可查；缺口 #8（graphs 行/runs 总数） |

## 5. 遗留问题

- **无阻塞项**；核心存储/写路径/聚合/只读查询全部实测通过，可作为 M2 的 trace 基础合并。
- **待 Lead 确认登记 ISSUE（编号由 Lead 分配）**：5 项声明偏差（#1 单通道、#2 NodeTimedOut.at、#3 Transition 超限、#4 fail-stop/interrupted 留 M3/M5、#5 RecordMode 小写）+ 未声明简化（#6 created_seq NULL、#7 reader 版本检查、#8 graphs/stats API、#11 备份/权限）+ 写线程内存态（#12）。建议合并后在 `doc/issues.md` 一次性登记，并回写 spec 06 §6.3.4/§6.4.2/§6.7/§6.9.1/§6.11.2/§6.2.11。
- **与 cli M2 的接口决策（需 Lead 在 REV-009 前拍板）**：#8 的 `graphs` 访问器——若 CLI 的 `trace` 需要 `graph_name`/`dsl_json`（06 §6.10.4 头部/§6.2.3 回放校验），只能在 trace crate 补 API；若允许在 cli worktree 改 trace，属跨模块改动，需 Lead 在评审记录中声明并指定第二评审人。
- **测试补强（建议）**：#3 行为定案后补 Transition 超限用例；补 reader schema 版本、重复/非终态 RunFinished、dropped 计数用例（#13）；拆分 >60 行测试函数（#14）。
- **M3/M5 跟踪**：fail-stop 接线与退出码 4、interrupted/recovery 标定、只读 WAL 恢复（06 §6.8.5）、双通道拆分、迁移备份与权限、多 run 内存态清理。
- **流程**：本分支未更新 `doc/progress.md`/`doc/issues.md`（trace worktree 干净）；合并时由 Lead 按 AGENTS §5/§7 更新。未 push/merge，`main` 未被本分支改动。

## 6. 条件关闭记录（仅“有条件通过”填写）

不适用：本记录结论为 **通过**，无前置条件。

## 7. 修订复审（2026-10-29，`2443d65`，trace 增量）

> 触发：首轮通过（`aeeeaec`）后，作者 `trace-dev` 追加 `2443d65` 关闭首轮建议 #7（reader 版本校验）与 #8（graphs 访问器/stats db_runs）。本轮只复审增量，不重开已通过范围。

- **增量范围**：`aeeeaec..2443d65`，5 files, +206/−25，仅 `crates/zgraph-trace/**`（`src/lib.rs`、`src/reader.rs`、`src/schema.rs` + `tests/migration.rs`、`tests/reader.rs`）；无跨模块改动。
- **复核项与证据**：
  1. **`TraceReader::graph()` + `GraphRow`**（`src/reader.rs`）：存在时返回 `graph_hash/graph_name/dsl_version/dsl_json/created_at` 全字段；缺失返回 `None` 不报错（`tests/reader.rs::graph_row_and_run_graph_name_are_available` 实测 6 个字段 + `graph("fedcba…")=None`）。
  2. **`RunRow.graph_name`**（`LEFT JOIN graphs`）：正常 run → `Some("demo_graph")`；**孤儿 run**（`graphs` 行缺失，测试用 `PRAGMA foreign_keys=OFF` 直插 `runs`）→ `graph_name=None` 且查询不报错、`graph_hash/status/record_mode` 正常（`run_without_graph_row_reports_none_graph_name`）。
  3. **`RunStats.db_runs`**：stats 测试插入 2 个 run 并断言 `db_runs=2`（`stats_reports_counts_duration_and_db_info`）。
  4. **只读 reader 的 `user_version` 校验**（`src/schema.rs::ensure_readonly_version`，在 `open_readonly_connection` 中、`ensure_trace_schema` 之前调用）：`==2` 正常；`>2` → `TraceError::SchemaTooNew{found=3,supported=2}` 且**不改库**（拒绝后 `user_version` 仍 3）；`<2`（v1 库）→ `TraceError::Storage("unsupported schema version=1, expected 2")` 且**不迁移**（拒绝后 `user_version` 仍 1）；缺失库/空文件仍报错且不创建文件/不写库（原有测试保留）。
  5. 新增测试数：2 个 migration 用例（v3 拒绝、v1 拒绝）+ 2 个 reader 用例（graph 存在/缺失、孤儿 run）+ stats 断言扩展；覆盖要求中的“存在/缺失/孤儿 run/v3 拒绝/v1 拒绝”全部真实断言，非只查行数。
- **复跑结果**：`cargo fmt --all -- --check` exit 0；`cargo clippy --workspace --all-targets -- -D warnings` exit 0；`cargo test -p zgraph-trace` = **34 passed / 0 failed**（api 5 + migration 7 + reader 6 + roundtrip 6 + streaming 9 + doctest 1）；`cargo test --workspace` = **133 passed / 0 failed**；`bash scripts/dev/check.sh` = CHECK PASSED；pytest = 20 passed。
- **结论：维持通过**。首轮 §3 #7/#8 已闭环（状态视为 `fixed(2443d65)`）；本轮未引入新阻塞/重要项；首轮其余建议项（#1–#6、#9–#14）处置不变（待 Lead 确认登记 ISSUE / M3-M5 跟踪）。合并门禁以本段更新后的记录为准。
