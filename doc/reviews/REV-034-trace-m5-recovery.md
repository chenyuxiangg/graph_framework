# REV-034 · trace · M5 ISSUE-084 中止标定 API + ISSUE-048 RunRow 判活列

> **模块声明**：本分支只改 `crates/zgraph-trace/**`（8 个文件：`src/{lib,reader}.rs`、`src/recovery.rs`（新）、`src/writer/{mod,thread,tx,recovery}.rs`（`recovery.rs` 新）、`tests/recovery.rs`（新）），符合 AGENTS §2 `trace` 模块边界；**无**代码外的依赖/`Cargo.lock`/跨模块/文档改动。作者 `trace-dev`，评审人 `reviewer-a`（**非作者**）。评审**未修改分支任何文件**，只新增本记录；未 merge / 未 push / 未改 main。
>
> 评审对象：`ad7fbec..cebb0af`（1 commit `cebb0af feat(trace): ISSUE-084 中止标定 API + ISSUE-048 RunRow owner_pid（REV-034）`）。评审时本地 `main` = `3eff14b`（已含 REV-025/026/027/028/029/038），分支 base 为 `ad7fbec` → **合入前需 rebase**（trace 改动与 main 的 cli/docs 改动无文件重叠，预计无冲突；见 §3 #5）。

- **分支**：`feat/trace-m5-recovery`
- **作者**：`trace-dev`
- **评审人**：`reviewer-a`（**非作者**）
- **变更范围**：`ad7fbec..cebb0af`（`git diff --shortstat` = **8 files changed, 652 insertions(+), 8 deletions(-)**；base = merge-base = `ad7fbec`；worktree clean）
- **评审时间**：2026-10-30
- **关联**：ISSUE-084（P2 trace/cli 崩溃标定）、ISSUE-048（P3 trace `owner_pid` 判活列）；spec 06 §6.8.1/§6.8.2/§6.8.3、§6.10.3、§6.2.4/§6.2.5；DR-002 §4（ISSUE-084/048 口径）、§6 wave2 表（trace `feat/trace-m5-recovery` REV-034；cli REV-030 消费）；REV-028（C1 来源）

## 1. 结论

**结论：通过**

`feat/trace-m5-recovery` 按 ISSUE-084/DR-002 的口径交付**两段式**标定原语，且与 spec 06 §6.8.1/§6.8.2/§6.8.3 **逐条一致**：

- **候选查询**：`TraceWriter::interrupted_candidates()` 经写线程控制消息执行 `SELECT run_id,status,owner_pid,owner_host,heartbeat_at FROM runs WHERE status IN ('Running','Paused')`（稳定 `ORDER BY started_at, run_id`），字段与 §6.8.1 步骤 1 一致；**判活（`kill(pid,0)`/heartbeat 陈旧）不在 crate 内**（§6.8.1 步骤 2 由 CLI REV-030 按固定顺序执行），符合 ISSUE-084/DR-002 §4「trace 只提供 API、CLI 消费」的分工，且 crate 内无 `std::process`/`Command` 依赖（已核）。
- **标定**：`TraceWriter::mark_interrupted(&[String], now)` 在写线程 `BEGIN IMMEDIATE` 单事务内逐 run 条件 `UPDATE runs SET status='Failed', interrupted=1, recovered_at=?, error_message='run interrupted: owner process exited' WHERE run_id=? AND status IN ('Running','Paused')`；**`error_code`/`finished_at` 保持 NULL**、**不修改 instances**、**不合成 RunFinished/Transition**（§6.8.2），返回实际命中的 `RecoveredRun{run_id, previous_status, recovered_at}`；固定文案 `INTERRUPTED_ERROR_MESSAGE` 与 spec §6.8.1 步骤 3 逐字一致。
- **写线程语义保持**：候选/标定与数据消息共用同一 FIFO（先 `finish_batch` 提交已入队写入再执行控制操作，barrier 成立）；标定 SQL/IO 失败记录**既有 fatal 错误通道**（`take_error`/`flush`/`close` 一致暴露，fail-stop 语义未破坏）；`interrupted_candidates` 在既有 fatal 下与 `mark_interrupted` 一样返回同一错误。
- **RunRow 只增不破**：新增 `owner_pid: Option<u32>`/`owner_host: Option<String>`/`heartbeat_at: Option<String>`；`reader.run()` 的 SQL/索引同步（21/22/23），无其他生产构造点；`to_opt_u32` 对非法/越界值读 `None`。
- **质量门禁**：`cargo test -p zgraph-trace` **42 passed / 0 failed / 0 ignored**（新增 `tests/recovery.rs` 8 用例全过，覆盖候选过滤、条件更新、只改 runs/行数不变、注入 trigger 整体回滚、close 后重开可见、空列表不排队、既有 fatal 通道、RunRow 判活字段）；`cargo test --workspace`（隔离 target）**454 passed / 0 failed / 2 ignored**（main 基线 446 + 新增 8，**不缩水**）；clippy `-D warnings`/fmt/`check.sh` PASS；改动文件无函数 >60 行；无新依赖（`Cargo.lock` 未变）。

未发现阻塞/重要项；4 条建议/Nit + 3 条备注见 §3（ISSUE-048 余项、测试补强、错误通道对称性、rebase 前置等），均不阻塞本分支。

## 2. 四维评审

| 维度 | 结论 | 依据（文件/行/测试） |
|---|---|---|
| 功能正确性 | ✅ | ① §6.8.1 步骤 1/3 逐字段一致（`src/recovery.rs:13-45`、`src/writer/recovery.rs:20-56,58-118`）；② §6.8.2 硬约束经测试验证：`mark_interrupted_is_reader_visible_without_synthetic_rows`（events/instances/transitions 行数不变、无 RunFinished、`finished_at=None`、`error_code=None`）；③ 原子性：`mark_interrupted_rolls_back_whole_transaction_on_failure` 用 `BEFORE UPDATE ON runs ... RAISE(ABORT)` 让第二个 run 失败，断言第一个 run 也回滚为 Running（真正验证单事务）；④ 幂等/持久：`mark_interrupted_is_durable_after_close_and_reopen`（close+重开只读可见；已终态重复标记返回空）；⑤ 空输入不排队（`empty_mark_returns_empty_without_queueing`）；⑥ RunRow 判活字段（`run_row_exposes_owner_and_heartbeat_for_liveness`）。**余项**：ISSUE-048 的 CLI 提示部分不在本分支（§3 #1）。 |
| 代码结构合理性 | ✅ | 职责分层：`src/recovery.rs`（公开数据类型/固定文案，不依赖 OS）、`src/writer/recovery.rs`（SQL 原语：候选/单事务标定/重试）、`writer/mod.rs`（控制消息 API + 泛型 `control` 请求/应答）、`writer/thread.rs`（FIFO 分支）；`RunRow` 只增字段。`is_busy/sleep_before_retry/busy_exhausted` 从 `tx.rs` 提升 `pub(crate)` 复用于标定事务（消除重复），标定复用既有 busy 重试策略。无跨模块依赖/无新 crate/无 `std::process`。 |
| 逻辑性 | ✅ | 控制消息走同一通道与 FIFO，`run_cycle` 先 `finish_batch`（含聚合 flush + 批提交）再 SELECT/UPDATE → 与 `flush` 后的只读查询等价（文档与实现一致）；标定事务用 `IMMEDIATE` + 「先 SELECT 再条件 UPDATE」保证只更新标定瞬间仍 Running/Paused 的行；`tx` 随函数返回/错误 drop 自动回滚（`?` 路径已覆盖）；SQL/IO 失败 `record_fatal` 后返回 Err，`finish_batch` 对后续消息短路同一 fatal（`take_error` 可见）；空 run_ids 直接返回且不排队。`recovered` 只在 UPDATE 命中时返回。 |
| 可读性 | ✅ | 模块/类型/方法文档完整说明「两段式」「判活归调用方」「不合成事件」「error_code 保持 NULL」及原因；常量/文案命名清晰；测试分段与命名可读（候选→标定→可见性→原子性→持久→空→错误通道→RunRow），直接改库/trigger 的测试辅助有注释。Nit：无。 |

## 3. 发现清单

| # | 级别 | 位置 | 问题 | 建议 | 状态 |
|---|---|---|---|---|---|
| 1 | 建议 | `crates/zgraph-trace/src/reader.rs:129-134`（新增字段）vs spec 06 §6.8.3（`06:646-650`）/ISSUE-048 | ISSUE-048 仅完成 **reader 侧**（`RunRow` 携带 `owner_pid/owner_host/heartbeat_at`）；`zgraph trace` 的「`run still in progress (pid N)`」提示与 interrupted 截断的 CLI 输出/退出码语义（06 §6.8.3）在 `cli` 模块，未在本分支 | 台账 ISSUE-048 应保持 **open/部分修**（勿提前 fixed），写明「reader 侧已由 REV-034 完成，CLI 提示随 REV-030」；DR-002 §4 已列 wave2 | open（台账/docs） |
| 2 | 建议 | `crates/zgraph-trace/tests/recovery.rs` | 测试缺口：①「barrier/FIFO」没有被直接断言——现有用例都先 `flush()` 再查候选；建议加一条「send RunStarted 后**不 flush** 直接 `interrupted_candidates()` 应能看到候选」的用例（锁住控制消息先提交前序写入的文档语义）；②候选的行可空字段（`owner_pid/owner_host/heartbeat_at` 为 NULL）未覆盖，建议直改库置 NULL 后断言 `None`（`to_opt_u32`/`Option<String>` 路径） | 补 1–2 条最小用例；不影响 API 正确性 | open（建议） |
| 3 | Nit | `crates/zgraph-trace/src/writer/thread.rs:103-108` vs `:224-234` | 错误通道对称性：`mark_interrupted` 失败会 `record_fatal`（后续输入被丢弃、close 返回该错误）；而 `interrupted_candidates` 查询失败只把 Err 发给调用方，不写入 fatal 通道（若查询因连接级错误失败，写线程仍会继续接收输入，直到下一次批写失败才 fatal） | 两个选择：① 查询失败也 `record_fatal`（需把 `interrupted_candidates` 改 `&mut self`/在分支内记录）并在文档说明；② 明确注释「只读查询失败不升级为 fatal」。当前实现可接受，但建议注释说明取舍 | open（Nit） |
| 4 | 建议（docs） | spec 06 §6.8.1 N-04（`06:640`）/ 本分支 crate 文档（`src/lib.rs:35-37`） | spec 写「标定入口：`RunManager::open()/run()` 内自动执行（打开 DB 即标定），`zgraph run` 只是调用方之一」；v2 无 RunManager，实际是「trace 提供两段原语 + CLI 启动时调用」。需 M5 docs 回写实现口径（DR-002 §9 #10 已列入 06 §6.8.1/§6.8.3 回写） | M5 docs writeback（REV-035）注明新入口口径；本分支文档已如实说明 | open（docs） |
| 5 | 备注（合并前置） | 分支 base `ad7fbec` vs 本地 `main` `3eff14b` | 分支落后 main（含 REV-038 docs + cli wave1 `feat/cli-m5-run`）；`git merge --ff-only` 会非快进，`merge.sh` 依据 `origin/main`（`1cc9123`）不会自动 rebase | 合入前由 Lead/trace-dev rebase 到 `3eff14b`（trace 与 cli/Cargo 改动无重叠，预计无冲突）后跑 dry-run | open（合并前置） |
| 6 | 备注 | 全 diff | 已核对通过项：无 `Cargo.toml`/`Cargo.lock` 改动；`crate` 内无 `std::process`/`Command`/`kill`（判活留给 CLI）；改动文件无函数 >60 行；`INTERRUPTED_ERROR_MESSAGE` 与 spec 逐字一致；UPDATE 不触及 `error_code`/`finished_at`、不写 events/instances/transitions（测试覆盖） | 无需动作 | 无需处理 |
| 7 | 备注 | 本评审环境 | 两次共享 target 的误导性编译错误：① `/repo/target` 上 workspace 报 `no method mark_interrupted`/`no field owner_pid`；② 复用 `/tmp/zgraph-merge-target2`（并行评审共用）跑 dry-run 报同一类错误。均为 cargo 并发构建/跨 worktree 复用 rlib 的 race（**非分支缺陷**）；独立隔离 target 上 workspace 454 passed/0 failed/2 ignored、dry-run exit 0 | 合并门禁务必用**每次评审独有**的隔离 target（同一路径被多个评审共用仍会 race）；建议 Lead 在收尾 docs/脚本说明中固化「每评审独立 target + 先 `cargo clean -p` 工作区成员」 | open（环境建议） |

> 无阻塞项、无重要项；#1/#4 由台账/ M5 docs 跟进，#2/#3 为测试/注释建议，#5 为合并前置，#6/#7 为备注。

## 4. 验证证据

### 4.1 范围、分支与提交元数据

```bash
cd /Users/zyzs/lczyzs/code/zgraph/.worktrees/trace-trace-dev
git status --short                       # 空（评审时 clean）
git branch --show-current                # feat/trace-m5-recovery
git merge-base main HEAD                 # ad7fbec...（分支 base；main 已到 3eff14b → 需 rebase）
git rev-parse HEAD                       # cebb0af97c3940ce301a5128e2a6a7a0ded264eb
git log --oneline main..HEAD             # cebb0af
git diff --shortstat main...HEAD         # 8 files changed, 652 insertions(+), 8 deletions(-)
git diff --name-status main...HEAD
# M src/{lib,reader}.rs；A src/recovery.rs；M src/writer/{mod,thread,tx}.rs；A src/writer/recovery.rs；A tests/recovery.rs
git log -1 --format='%H %an <%ae> %s'    # chenyuxiangg <1007423165@qq.com>；正文含 Review: REV-034 / Agent: trace-dev
```

### 4.2 spec 06 §6.8.1/§6.8.2/§6.8.3 逐条核对

| spec 要求 | 实现/测试 | 结论 |
|---|---|---|
| §6.8.1-1 候选：`SELECT run_id,status,owner_pid,owner_host,heartbeat_at FROM runs WHERE status IN ('Running','Paused')` | `src/writer/recovery.rs:20-56`；`InterruptedCandidate` 字段一致（`src/recovery.rs:20-35`）；测试 `interrupted_candidates_include_only_running_and_paused_runs` | ✅ |
| §6.8.1-2 判活顺序（同机 pid / 跨机 heartbeat 60s / 同机 pid 不存在） | **不变量在本 crate**：判活归 CLI（REV-030）；crate 只提供 `owner_pid/owner_host/heartbeat_at`；`src/lib.rs:35-37` 明确说明；grep 无 `std::process/Command/kill` | ✅（分工符合 DR-002 §4/ISSUE-084） |
| §6.8.1-3 标定 UPDATE 字段与固定文案；`error_code` 保持 NULL | `src/writer/recovery.rs:58-118`；文案常量 `src/recovery.rs:13`；测试 `mark_interrupted_is_reader_visible_without_synthetic_rows` 断言 `error_code=None`、`recovered_at`、`error_message` | ✅ |
| §6.8.1-4 不修改 instances/不写 RunFinished/Transition | UPDATE 仅 `runs`；测试断言 events/instances/transitions 行数不变、无 RunFinished | ✅ |
| §6.8.1-5 stderr warning / `--json recovered[]` | CLI（REV-030）；`RecoveredRun` 提供 `run_id/previous_status/recovered_at` 供 CLI 输出 | ✅（余项 §3 #1） |
| §6.8.2 不新增 `RunStatus::Interrupted`、不合成 RunFinished、只标 runs | 使用 `Failed`；无 events 写入；测试覆盖 | ✅ |
| §6.8.3 interrupted 截断提示 / pid 存活提示 | 提示在 CLI `trace`（未在本分支）；`RunRow.owner_pid` 已就绪供其消费 | ⚠️ 余项（#1） |

### 4.3 写线程 FIFO/barrier、事务原子性与 fatal 通道（分析 + 测试）

| 关注点 | 实现 | 验证 |
|---|---|---|
| FIFO/barrier | 控制消息与数据同走一个 `sync_channel`（`writer/mod.rs:29-41`）；`run_cycle` 对 `InterruptedCandidates`/`MarkInterrupted` 先 `finish_batch`（flush 聚合 + 提交批）再执行（`writer/thread.rs:103-116`）；`control` 用 0 容量应答通道（`mod.rs:206-216`） | 代码核对；现有测试均先 `flush()`（#2 建议补不 flush 的用例） |
| 事务原子性 | `BEGIN IMMEDIATE`（busy 重试）+ 逐 run `SELECT`→条件 `UPDATE`，同事务；错误 `?` 返回时 `Transaction` drop 自动回滚（`writer/recovery.rs:58-118`） | `mark_interrupted_rolls_back_whole_transaction_on_failure`：对第二个 run 注入 `BEFORE UPDATE ... RAISE(ABORT)`，断言第一个 run 也保持 Running 且 `take_error` 可见 fatal、`close` 失败 |
| fatal 错误通道保持 | `mark_interrupted` 失败 `record_fatal`（`thread.rs:224-234`）；`finish_batch` 对后续消息短路 fatal；`interrupted_candidates` 在既有 fatal 下返回同一错误（先 `finish_batch`） | `control_messages_surface_existing_fatal_error_channel`（oversized payload 制造 fatal 后，候选/标定/close 均 Err） |
| 关闭/重开持久性 | 标定走写线程连接、事务提交；`close` 做 checkpoint | `mark_interrupted_is_durable_after_close_and_reopen`（close 后 `open_readonly` 可见 Failed/interrupted/recovered_at/error_message/error_code=None） |
| 幂等/空输入 | 已终态/不存在跳过；空列表直接返回不排队 | `mark_interrupted_is_durable...`（重复标记返回空）、`empty_mark_returns_empty_without_queueing`（随后写入仍健康） |

### 4.4 必跑命令与结果

| 命令 | 结果 |
|---|---|
| `cargo test -p zgraph-trace -- --test-threads=2`（共享 target） | **42 passed / 0 failed / 0 ignored**（lib 0 + api 5 + migration 7 + reader 6 + **recovery 8** + roundtrip 6 + streaming 9 + doc 1；`/tmp/rev034-trace-test.log`） |
| `cargo test --workspace -- --test-threads=2`（**隔离 target** `/tmp/zgraph-merge-target2`，先 `cargo clean -p` 全部 workspace 成员） | **454 passed / 0 failed / 2 ignored**（main 基线 446 + 本分支新增 8；2 ignored = harness 真模型 smoke + e2e AC-08b 手动；`/tmp/rev034-workspace-iso.log`）——**不缩水** |
| `cargo clippy --workspace --all-targets -- -D warnings`（隔离 target） | exit 0 |
| `cargo fmt --all -- --check` | PASS |
| `bash scripts/dev/check.sh` | CHECK PASSED（身份/冻结文件/禁用文件/progress ≤30/fmt/diff-check） |
| 函数长度（改动/新增文件脚本测量） | 无 >60 行函数（`src/recovery.rs`/`writer/recovery.rs`/`writer/mod.rs`/`writer/thread.rs`/`reader.rs`/`lib.rs`） |

> **环境记要（如实）**：首次在共享 `/repo/target` 跑 workspace 时出现 `E0599/E0609`（找不到本分支新增的 trace API/字段），系与其它 agent 并发构建同一 target 的 rlib race（同一 worktree 的 `cargo test -p zgraph-trace` 已先通过 42 条）；改用隔离 target 清理 workspace 成员后 **454 passed/0 failed**。该现象非本分支缺陷，但会影响合并门禁，建议后续统一用隔离 target（§3 #7）。

### 4.5 merge.sh dry-run（隔离 target，结论前）

```bash
cd /Users/zyzs/lczyzs/code/zgraph
CARGO_HOME=/Users/zyzs/lczyzs/code/zgraph/.cargo-home CARGO_NET_OFFLINE=true \
CARGO_TARGET_DIR=<独占用隔离 target> CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=2 \
RUST_TEST_THREADS=2 PYTHON_BIN=/Users/zyzs/lczyzs/code/zgraph/.venv/bin/python \
bash scripts/dev/merge.sh feat/trace-m5-recovery --dry-run
```

- **attempt 1（复用 `/tmp/zgraph-merge-target2`，该目录同时被并行评审/构建使用）**：`cargo test --workspace` 在编译 `crates/zgraph-trace/tests/recovery.rs` 时报 `E0599/E0609`（找不到本分支新增的 `mark_interrupted`/`RunRow.owner_pid` 等；提示 RunRow 仍是 21 字段旧版）→ `MERGE BLOCKED: cargo test 失败`。
  **判定为共享 target 的并发构建 rlib race（环境问题，非分支缺陷）**：同一 worktree 先前用隔离 target 已跑通 `cargo test --workspace` **454 passed/0 failed/2 ignored**（§4.4）与 `cargo test -p zgraph-trace` 42 passed；该失败是 cargo 把其它 worktree/此前构建的 trace rlib 与本分支新测试源码混配（Lead 已说明 REV-033 并行评审，共享 target 不可靠）。
- **attempt 2（改用本评审独有全新隔离 target `/tmp/rev034-target`，load avg≈6.9）**：`INFO: 评审记录 -> 通过` → `cargo test` 通过 → pytest 失败 **0** / 基线 **0**（无新增）→ `INFO: dry-run 结束，检查全部通过`（exit 0）。

## 5. 遗留问题

- **ISSUE-048 部分完成**：`RunRow.owner_pid/owner_host/heartbeat_at` 已就绪；`zgraph trace` 的「run still in progress (pid N)」提示/截断输出与退出码在 CLI（REV-030）。台账保持 open/部分修，勿提前 fixed。
- **ISSUE-084 后续**：CLI `run` 启动调用两段原语（候选→判活→标定→`recovered[]`/stderr）+ kill -9 故障注入测试由 REV-030 落地；本分支仅 trace API。
- **spec 回写**：06 §6.8.1 N-04（RunManager/「打开 DB 即标定」）与实际「trace 两段原语 + CLI 调用」口径差异 → M5 docs writeback（REV-035，DR-002 §9 #10 已列）。
- **测试补强（建议）**：#2 barrier（不 flush 直查）与 NULL owner 字段用例；#3 候选查询失败是否升级 fatal 的注释/取舍。
- **合并前置**：#5 分支需 rebase 到当前 main（`3eff14b`）后跑 `merge.sh --dry-run`（隔离 target）。
- **环境操作**：#7 合并门禁用「每次评审独有的隔离 target」，不要复用其它评审的 target 路径（并发 rlib race 会产生与本分支无关的编译错误）；本评审最终 dry-run 在 `/tmp/rev034-target` 通过。
- **合并后**：Lead 更新 `doc/reviews/README.md`/`doc/index.md` 的 REV-034 索引与 `doc/progress.md`、`doc/issues.md`（ISSUE-048 部分修说明）。
- 本评审未修改分支任何文件；REV-034 记录为本分支唯一新增文件（由评审人提交）。

## 6. 评审签署

- 范围仅 trace 模块 8 文件；API 只增（两段原语 + 公开类型/常量）且 `RunRow` 只增字段；spec 06 §6.8.1/§6.8.2 逐条一致、§6.8.3 的 reader 前置就绪；写线程 FIFO/barrier 与 fatal 通道保持；单事务回滚经注入 trigger 真实验证；`cargo test -p zgraph-trace` 42 passed、workspace 454 passed/0 failed/2 ignored（隔离 target，不缩水）、clippy/fmt/check.sh PASS；无新依赖、无函数 >60 行、crate 内无进程/判活依赖。
- 未发现阻塞/重要项；建议项 #1–#4 为台账/docs/测试补强，#5 为合并前置。
- **结论：通过**（合入前需按 §5 先 rebase 到当前 main 并复跑 `merge.sh` 全门禁）。
- 评审人：`reviewer-a`（非作者）；评审时间：2026-10-30。
