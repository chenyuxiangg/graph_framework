# REV-032 · e2e · M5 wave2 CLI 真链路（AC-01 / L2 replay / 崩溃标定 / ISSUE-076）+ REV-029 Nits

> **模块声明**：本分支只改 `tests/e2e/**` 与 `tests/fixtures/graphs/**`（e2e 模块边界），另改根 `Cargo.lock`（**`process` 模块**，仅 `zgraph-e2e` dev-dependencies +`rusqlite` 1 行机械新增，**无新包/无版本变更**；Lead 已批准跨模块）。作者 `e2e-dev`，评审人 `reviewer-b`（**非作者**）。评审**未修改分支任何文件**，只新增本记录；未 merge / 未 push / 未改 main。
>
> **独立核对口径**：不采信作者自述。亲自读 4 个 commit 全量 diff（17 files +1783/−11）与 `cli_ac01.rs`/`cli_replay.rs`/`cli_support/mod.rs`/`harness_ac08.rs` 新增全文，逐条对照 DR-002 §3.3、spec 06 §6.8/§6.10.5、07 AC-01/AC-08/AC-12；在共享 target 实跑 e2e/workspace 并另跑 **main 基线**对比；用**真实 CLI 二进制 + 分支 fixtures** 手工独立复跑 AC-01 → L2 replay（DB SHA/WAL 检查）、L1 与 trace diff、missing-record 退出码；crash 用例为**真 SIGKILL**（e2e 内 3/3 通过 + 代码核对）；`clippy` 因共享 target 被并行 worker worktree 污染（E0027：worker 分支的 `WorkerFailure::signal` 字段被复用）在**独有 fresh target** 复跑 exit 0。

- **分支**：`test/e2e-m5-cli-replay`
- **作者**：`e2e-dev`（4 commit 正文均含 `Review: REV-032`/`Agent: e2e-dev`）
- **评审人**：`reviewer-b`（**非作者**）
- **变更范围**：`64f110addbec4f6c7aafb7f3e2c67d6b3c018577..e97617ce07bc55ebaa6c0b57841c5507118e7e64`（4 commits：`6419bce` AC-01 CLI、`893afd1` ISSUE-076 e2e、`1cdfeeb` REV-029 Nits/ISSUE-061 注释、`e97617c` L2 replay + 崩溃标定 + CLI ISSUE-076；`git diff --shortstat` = **17 files changed, 1783 insertions(+), 11 deletions(-)**；base = merge-base = `main` = `64f110a`，ahead 4，worktree clean）
- **评审时间**：2026-10-30
- **关联**：DR-002 §3.3（replay L1/L2）、§4（ISSUE-084 wave2）、§6（wave2 分工）、§9 #4/#10；spec 07 **AC-01**（示例图端到端）、06 §6.8.1/§6.8.2/§6.8.3（标定/进行中/截断）、§6.10.3/§6.10.5/§6.10.6（CLI）；05 §5.9.2（agent 四字段）；ISSUE-048/061/076/079/084；REV-024（REV-029 Nits 来源）、REV-027/030/033/034

## 1. 结论

**结论：通过**

本分支把 M5 wave2 的 e2e 验收落到**真实 `zgraph` 二进制 + 真 SQLite + 真 Python worker**（无 mock），覆盖完整、断言真实、稳定；未发现阻塞/重要缺陷：

- **AC-01 CLI 真链路**（`cli_ac01.rs`，3 用例）：真实 CLI 跑 `customer_ticket` 图（3 个 fake LLM agent + 1 个真 Python 节点 `human_escalation_check` + 内置 gate + 真 trace），断言 run `Completed`、`run_output.result` 四字段、`content` 二次解析出 `answer=可以退款`/`need_human=false`、实例终态（5 Completed + `human_handoff` Skipped）、gate 拦截恰 1 次（`check_output`）、python 实例输出 `{"need_human":false}`（真 worker 证据）、`zgraph trace` 可回放；坏图 exit 2/坏入口 exit 3 均 `runs=0`。**独立手工复跑**（分支 fixtures + 真二进制）同样的 run/replay 结论成立（§4.3）。
- **L2 replay e2e（9 用例，`cli_replay.rs`）**：`--record=full` 原始 run → `replay --mode=outputs` exit 0、`matched=true`、`original_output==replayed_output`、`runs` 计数不变、**原 DB 主文件字节完全一致**、**marker 日志行数不变**（临时 `gates/hr.py` 覆盖包把 worker pid 写入 `ZGRAPH_L2_MARKER_LOG`，replay 不执行 python 节点）+ `ZGRAPH_PYTHON=/nonexistent` 仍成功 = 不启动 worker 的**可判定证据**（非 `ps` 猜测）；L1 `--mode=events` 与 `zgraph trace --json` **逐字节一致**；`hash` 模式 → `REPLAY_RECORD_MISSING` exit 3；删一条 recorded_outputs → `REPLAY_RECORD_MISSING` exit 3；篡改 `output_hash` → `REPLAY_RECORD_CORRUPT` exit 3；篡改 `runs.output_json` → `REPLAY_OUTPUT_MISMATCH` exit 1（payload 断言）；非 Completed → `REPLAY_RUN_NOT_COMPLETED` exit 3。独立手工复跑（AC-01 分支 fixtures）：DB SHA256 前后一致（`-wal` 0 字节、`-shm` 未变）、missing-row exit 3（§4.3）。
- **崩溃标定 e2e（真 kill -9）**：CLI 起长 run（慢 fake ~133s）→ 轮询 DB 到 `Running` 且断言 `runs.owner_pid == CLI 进程 pid` → `Child::kill()`（SIGKILL，断言 `signal()==9`）→ 同库再跑：`recovered[]` 恰 1 条（run_id/previous_status/recovered_at）、stderr `warning: run <id> marked interrupted (was Running, pid N gone)`、RunRow `Failed/interrupted=1/recovered_at/finished_at=NULL/error_code=NULL/error_message=固定文案`、`RunFinished` 计数 0、`instances` 行数不变、`runs=2`；`zgraph trace` 对 interrupted run **exit 1**、stderr `trace truncated`、NDJSON 保留 RunStarted 且无 RunFinished。
- **ISSUE-076 CLI/进程内端到端**：`cli_replay` 用例与 `harness_ac08` 用例均用 AgentConfig.node_timeout_ms=**1800ms** + DSL 不声明 timeout + 慢 fake（10s）→ run exit 1 `NODE_TIMEOUT`、实例 `TimedOut`、`NodeTimedOut` 恰 1 次，elapsed 断言 `[HINT-300ms, 8s)`；若 CLI/包装层未转发 hint 则 10s 后 Completed，用例会失败（区分度充分）。`harness_ac08`/`ac12` 的 `RoutingExecutor` 同时补了 `timeout_hint` 转发。
- **REV-029 Nits 收敛**：`ac12` 文档明确 ISSUE-079「峰值只覆盖 `WorkerExecutor` python 节点进程、工具并发独立 semaphore」口径与基线数字；防御性 `run_id` 断言加说明；`parallel` 更新 ISSUE-061 已修注释（断言不变）。
- **质量门禁**：`cargo test -p zgraph-e2e` **37 passed / 0 failed / 1 ignored**（main 24/1，+13：cli_ac01 3 + cli_replay 9 + harness_ac08 +1）；`cli_replay` 连跑 **3/3**、`cli_ac01` 2/2、e2e 全包复跑全绿；`cargo test --workspace` **552 passed / 0 failed / 2 ignored / 58 binaries**（**main 基线 539/2/56，+13，无缩水**）；`pytest` 84 passed；`cargo clippy --workspace --all-targets -- -D warnings` 在**独有 fresh target** exit 0；`fmt --check` exit 0；`check.sh` PASS；worktree clean、HEAD tree 无禁入文件；4 commit trailers 合规。

发现 1 条**产品侧（kernel）缺陷需登记台账** + 2 条测试/证据加固建议 + 若干 Nit/备注（§3）：`agent_call` 自报超时路径 `instances.error_code` 为 NULL（kernel `fail_timeout` 未 `set_instance_error`，watchdog 路径会设置），e2e 已在测试注释如实记录并报 Lead，但**尚未登记 `doc/issues.md`**（AGENTS §7）；另 3 个新增测试函数 >60 行、DB 只读证据只比主文件字节。均**不阻塞本分支合并**，产品缺陷建议 Lead 登记 ISSUE 由 kernel 修复。

## 2. 四维评审

| 维度 | 结论 | 依据（文件/行/测试/命令） |
|---|---|---|
| 功能正确性 | ✅ | ① **AC-01**：`cli_ac01.rs:169-206`（主用例）/`:213-252`（失败路径） 真 CLI 断言 run/实例/gate/python 输出/`run_output`/trace；坏图/坏输入 `runs=0`（`:204-252`）。② **L2 replay**：`cli_replay.rs:138-205`（成功+只读+marker）、`:240-390`（hash/missing/corrupt/mismatch/non-completed 五类）、`:208-238`（L1 逐字节）；独立手工复跑同结论（§4.3）。③ **标定**：`:461-558` 真 SIGKILL、owner_pid、recovered、RunRow 字段、无 RunFinished、instances 不变、trace exit1；`:404-458` 断言辅助。④ **ISSUE-076**：`cli_replay.rs:560-624` + `harness_ac08.rs:693-758`；elapsed 下界 1500ms/上界 8s；1800ms hint 与 10s 慢 fake 的区分度成立。⑤ **Nits**：`ac12.rs` 文档/`timeout_hint` 转发、`parallel.rs` 注释（断言不变）。 |
| 代码结构合理性 | ✅ | `cli_support/mod.rs` 集中「真二进制发现（`ZGRAPH_BIN` → profile 目录 → 兜底 `cargo build`）、CLI 运行、只读/读写 SQLite 查询、图/资产/慢 fake/marker 覆盖包」；`cli_ac01`/`cli_replay` 只写场景断言，复用 `common` 既有 helper（`run_to_trace`/生命周期/实例断言）；fixtures 按 `customer_ticket/{agents,fake_llm.json,py,tools}` 落 `tests/fixtures/graphs/**`（e2e 模块），不复制产品逻辑；`Cargo.toml` 只加 dev-dep `rusqlite` 与两个 `[[test]]`。 |
| 逻辑性 | ✅ | 每个用例独立 `TempDir`/DB；L2 只读与「不启动 worker」有双重证据（DB 字节 + marker + `ZGRAPH_PYTHON=/nonexistent`）；kill 用例 20ms 轮询 +15s 上限、断言 `signal()==9`、不 sleep 猜完成；ISSUE-076 用 `[hint-300ms, 8s)` 宽松区间并同时断言 exit/NODE_TIMEOUT/TimedOut/NodeTimedOut 四证；篡改用例直接改临时 DB（不触碰产品代码）；断言非恒真（写死计数/状态/字段逐一核对）。扣分项仅为 §3 #1（产品落库不一致）/#3（只读证据粒度）。 |
| 可读性 | ✅ | 文件头 docstring 给出验收语义/证据口径/稳定性约定；`cli_support` 每个 helper 解释「为什么」（二进制发现、rusqlite 只读、marker 覆盖、DB 字节证据）；fixture 带中文说明与 L3 边界；测试名可读。扣分项：新增 3 个测试函数 >60 行（§3 #2）、`tests/e2e/Cargo.toml` description 仍写 M3/M4（§3 #5）。 |

## 3. 发现清单

| # | 级别 | 位置 | 问题 | 建议 | 状态 |
|---|---|---|---|---|---|
| 1 | 建议（产品缺陷，需登记 ISSUE） | `tests/e2e/harness_ac08.rs:747-751` 注释；kernel `crates/zgraph-kernel/src/run.rs:909-920`（`fail_timeout`）vs `:1357`（watchdog 路径） | `agent_call` **执行器自报超时**（Harness 预检/LLM 超时返回 `NodeError::Timeout`）经 `fail_timeout` 迁移 `TimedOut`，但**未 `set_instance_error`** → trace `instances.error_code/error` 为 NULL；而内核 watchdog 超时路径显式 `set_instance_error(Timeout)`（注释自述「不写则 AC-06 落库为 NULL」）。同一 `NodeError::Timeout` 两条路径落库不一致，与 06 §6.2.5/§6.2.12「instances.error = 同对象 canonical NodeError；error_code 取自该对象」及 AC-06 e2e 对 python 超时的 `error_code=NODE_TIMEOUT` 断言不对齐。e2e 测试已在注释如实记录并「已报告 Lead」，但**未在 `doc/issues.md` 登记**（AGENTS §7 明确禁止用注释代替台账） | Lead 登记 ISSUE（kernel/harness）：`fail_timeout` 补 `set_instance_error(NodeError::Timeout{elapsed_ms})`（与 watchdog 路径对齐），补 kernel 单测 + e2e 断言；本 e2e 分支可在修复前把当前 `instance.error_code == None` 显式钉住（带 TODO/ISSUE 号）或修复后改断言 `NODE_TIMEOUT`。**不阻塞本分支**（e2e-only，产品行为为既有 kernel 代码） | open（Lead 登记 ISSUE） |
| 2 | 建议（函数长度） | `tests/e2e/cli_replay.rs:461`（`interrupted_run_is_marked_on_next_start_and_trace_is_truncated` 91 行）、`:560`（`cli_agent_config_timeout_hint_is_end_to_end` 65 行）；`tests/e2e/harness_ac08.rs:693`（66 行） | 新增测试函数 >60 行（architecture §2.2「函数 ≤60 行」等价约束；REV-020/022/030 均按此检查测试）。crash 用例把起进程/轮询/断言/第二次 run/trace 全塞一个函数 | 抽 helper（如 `spawn_long_run`/`assert_recovered_after_kill`/`assert_cli_hint_timeout`），或拆成「kill+标定」与「trace 截断」两用例；**不阻塞**（测试代码，不影响 CI）。 | open（建议） |
| 3 | 建议/Nit（只读证据粒度） | `tests/e2e/cli_replay.rs:139-160`（`snapshot`/`assert_replay_is_read_only`）；`cli_support::db_bytes` | 「replay 不改原 trace」只比较 **DB 主文件**字节，未比较 `-wal`/`-shm`；且 `snapshot()`/断言先调用 `runs_count()`（**读写**连接）再读 `db_bytes`，理论上 RW 连接可在两次取字节之间改动文件（当前 run 已 close/checkpoint、replay 只读，实际无影响；手工复核 `-wal` 0 字节、主文件 SHA 不变）。若 replay 未来只写 WAL 不落主文件，本断言会漏 | 同时比较 `db`+`db-wal`+`db-shm`（或目录下相关文件 SHA），并在**任何 RW 查询之前**取字节快照；`runs_count` 改用只读连接。**不阻塞**（代码只读 + marker + ZGRAPH_PYTHON 三重证据）。 | open（建议加固） |
| 4 | 建议/Nit（fixtures 本地残留） | `tests/fixtures/graphs/customer_ticket/py/gates/__pycache__/`（运行后生成） | e2e 运行会在新 fixtures 下生成 `__pycache__/hr.pyc`（`.gitignore` 已忽略、`git ls-tree HEAD` 无 pyc、worktree clean；与既有 `tests/fixtures/**/__pycache__` 同类）。「fixtures 不污染仓库」在 git 语义成立，但本地工作区会多出忽略文件 | 可在 e2e worker 环境设 `PYTHONDONTWRITEBYTECODE=1`（`common::worker_config` 的 env）或测试后清理；至少 docs/README 注明运行会产生 ignored pycache。**不阻塞**。 | open（Nit） |
| 5 | Nit（文档） | `tests/e2e/Cargo.toml:6` | package `description` 仍为「M3/M4 e2e 验收包」；本包已覆盖 M5（wave1/wave2） | 改为 M3–M5 或「M5 e2e 验收包」。**不阻塞**。 | open（Nit） |
| 6 | 备注（fixture 设计） | `tests/fixtures/graphs/customer_ticket/fake_llm.json` | AC-01 fake 脚本 3 次调用内容完全相同（`kb_search` 与 `answer_generator` 在 `input_parser` 后并发就绪，按节点区分内容会依赖请求顺序） | 作者已在 `cli_ac01.rs:22-25` 说明并保证确定性；AC-01 断言仅要求终态/answer/gate，不要求每 agent 不同内容——可接受；后续如需逐 agent 校验可加 fake 调用序号/节点映射（不做）。 | 无需处理 |
| 7 | 备注（e2e 覆盖边界） | `tests/e2e/cli_replay.rs` | 未覆盖：replay **默认** `--mode=events`（用例显式传 events；cli 单测覆盖默认）、run/DB 不存在 exit 4（cli 包测试已覆盖）、自环 `attempt>0` 的重放 | 可由 CLI 包/后续加固覆盖，非本分支验收必需；建议 docs 保持分工说明。**不阻塞**。 | open（备注） |
| 8 | 备注（跨模块） | `Cargo.lock`（+1 行）、`tests/e2e/Cargo.toml` | `Cargo.lock` 属 `process` 模块：仅 `zgraph-e2e` dev-dependencies 增 `rusqlite`（lock 中**无新包/无版本变化**，+1 行）；Lead 已批准跨模块 | 本记录按 AGENTS §2 声明 e2e+process 跨模块；merge 时 Lead 确认。**无阻塞**。 | 无需处理（已声明） |
| 9 | 备注（环境/门禁） | 本评审执行 | 共享 `target/` 的 `cargo clippy --workspace --all-targets` 报 **E0027 `WorkerFailure::Exit` 缺 `signal` 字段**（`zgraph-worker` 被并行 `worker-worker-dev` 的 ISSUE-066 分支产物污染；两分支源码本身一致）；**独有 fresh target `/tmp/rev032-target` 复跑 clippy exit 0**。共享 target 的 e2e/workspace 测试均通过（计数与 main 基线对齐）。`git fetch origin main` 本轮 dry-run 首次成功（若后续环境瞬时失败按 Lead 口径重试一次） | 合并 dry-run 务必用**独有 fresh target**（Lead 已要求）；共享 target 上不要并行跨 worktree clippy；若 fetch 失败按 Lead 口径重试一次。**非分支缺陷**。 | 无需处理（环境备注） |
| 10 | 备注（Nits/台账） | `tests/e2e/ac12.rs`、`tests/e2e/parallel.rs`、ISSUE 状态 | REV-029 Nits 已按约收敛：ac12 文档 ISSUE-079 边界/基线注释、防御性断言说明；`parallel` ISSUE-061 注释更新（断言不变）。合并后建议 docs/Lead 更新：ISSUE-048（trace 进行中提示）/ISSUE-084（标定+kill -9 e2e）/ISSUE-076（CLI e2e）可按事实关闭；ISSUE-079 仍 open（M6）；§3 #1 需新增 ISSUE（kernel `fail_timeout` 实例错误码） | Lead/docs 按事实更新台账；07 §7.3 #6 现在有自动化 kill -9 e2e 可关联 | open（docs/Lead） |

> 无阻塞项、无重要项；#1 为本分支发现的**既有 kernel 落库不一致**（e2e 已如实注释并报 Lead，建议补 ISSUE），#2/#3/#4/#5 为测试/证据/文档 Nit，#6–#10 为备注/台账。**本评审认为本分支无需新开 P1/P2 ISSUE**；#1 的产品缺陷由 Lead 决定登记/排期（kernel 修复不属 e2e 分支）。

## 4. 验证证据

### 4.1 范围、分支与提交元数据

```bash
cd /Users/zyzs/lczyzs/code/zgraph/.worktrees/e2e-e2e-dev
git branch --show-current        # test/e2e-m5-cli-replay
git rev-parse HEAD               # e97617ce07bc55ebaa6c0b57841c5507118e7e64
git merge-base main HEAD         # 64f110addbec4f6c7aafb7f3e2c67d6b3c018577（= main）
git rev-list --left-right --count main...HEAD   # 0	4
git diff --shortstat main...HEAD               # 17 files changed, 1783 insertions(+), 11 deletions(-)
git diff --name-status main...HEAD
# M Cargo.lock；M tests/e2e/{Cargo.toml,ac12.rs,harness_ac08.rs,parallel.rs}
# A tests/e2e/{cli_ac01.rs,cli_replay.rs,cli_support/mod.rs}
# A tests/fixtures/graphs/customer_ticket/{agents/*4+prompt.md,fake_llm.json,py/gates/{__init__,hr}.py,tools/.gitkeep}
git log --format='%h %an <%ae> / %cn <%ce> / %s' main..HEAD   # 4 commit 均 chenyuxiangg；正文含 Review: REV-032 / Agent: e2e-dev
git diff main...HEAD -- Cargo.lock                # 仅 zgraph-e2e dev-deps +rusqlite（1 行；无新包/版本变化）
git status --short                               # 空（clean）
git ls-tree -r --name-only HEAD | grep -E "__pycache__|\.pyc$|\.db$|\.sqlite" || echo none   # none
git diff --check                                 # exit 0
```

### 4.2 测试与门禁

| 命令 | 结果 |
|---|---|
| `cargo test -p zgraph-e2e` | **exit 0：37 passed / 0 failed / 1 ignored**（main 24/1，**+13**：`cli_ac01` 3、`cli_replay` 9、`harness_ac08` +1；日志 `/tmp/rev032-e2e.log`） |
| `cargo test -p zgraph-e2e --test cli_replay` ×3 | **3/3 稳定：9 passed**（1.85/1.87/1.90s；含真 kill -9、L2 全 9 例） |
| `cargo test -p zgraph-e2e --test cli_ac01` ×2 | **2/2：3 passed**（0.11s） |
| `cargo test -p zgraph-e2e` 复跑 | 全绿（15.11s；含 cli_replay 9 / cli_ac01 3 / ac12 1 / harness_ac08 5+1 ignored 等） |
| `cargo test --workspace`（本分支） | **exit 0：552 passed / 0 failed / 2 ignored / 58 binaries**（日志 `/tmp/rev032-ws.log`） |
| `cargo test --workspace`（**main 基线**，root worktree @`64f110a`） | **exit 0：539 passed / 0 failed / 2 ignored / 56 binaries**（`/tmp/rev032-main-ws.log`）——**分支 = main +13（e2e 新增 3+9+1），无缩水** |
| `/Users/zyzs/lczyzs/code/zgraph/.venv/bin/python -m pytest --tb=no`（worktree） | **exit 0：84 passed**（与 main 基线一致） |
| `cargo clippy --workspace --all-targets -- -D warnings`（**独有 fresh target** `/tmp/rev032-target`） | **exit 0 / 0 warning**（30.6s；共享 target 因并行 worker worktree 产物报 E0027，见 §3 #9/§4.4） |
| `cargo fmt --all -- --check` | **exit 0** |
| `bash scripts/dev/check.sh` | **CHECK PASSED**（身份/remote/CLAUDE.md/分支名/Python 语法/progress/issues/fmt/`git diff --check`） |
| `git status --short`（全部命令后） | 空（仅 ignored `__pycache__`，见 §3 #4） |
| `bash scripts/dev/merge.sh test/e2e-m5-cli-replay --dry-run`（**独有 fresh target** `/tmp/rev032-target`） | **exit 0：全部通过**——`INFO: 评审记录: …REV-032… -> 通过` → `INFO: cargo test 通过` → pytest「失败 0 项，基线 0 项，无新增」→ `INFO: dry-run 结束：检查全部通过`（94s；fetch 本轮首次成功，无重试） |

### 4.3 独立手工验证（分支 fixtures + 真实 `target/debug/zgraph`；临时 DB 在 `/tmp`，不触碰仓库）

| 验证 | 方法 | 结果 |
|---|---|---|
| AC-01 真 CLI run | `zgraph run tests/fixtures/graphs/customer_ticket.json --inputs '{"input_parser":{"user_message":"我要退款"}}' --assets-dir <customer_ticket> --fake-llm=<fixture> --record=full --json`（Python=venv，`--python-path` SDK+fixture/py） | exit 0、`status=Completed`、`run_output.result` 四字段、`content` 二次解析 `answer=可以退款`/`need_human=false` |
| L2 replay（独立复跑） | 同 DB：`ZGRAPH_PYTHON=/nonexistent zgraph replay --run <id> --mode=outputs --json`；前后 `shasum -a 256`；检查 `-wal/-shm` | exit 0、`ok/matched=true`、`recorded_outputs=4`、`original_output==replayed_output`；**主 DB SHA 前后一致**（`-wal` 0 字节、`-shm` 不变） |
| L1 replay | `zgraph trace <id> --db --json` vs `zgraph replay --mode=events --json` | `diff` 为空（NDJSON 逐字节一致） |
| 缺记录行 | 删 `recorded_outputs` 中 `human_escalation_check` 行后 replay | exit **3**、`REPLAY_RECORD_MISSING` |
| kill -9 标定 | e2e `interrupted_run_is_marked_on_next_start_and_trace_is_truncated` 真 `Child::kill()`（SIGKILL，断言 signal=9；3/3 通过）；代码核对断言 owner_pid/recovered[]/RunRow 字段/无 RunFinished/instances 行数/trace exit1 | passed（测试内断言全部生效，非 mock） |
| ISSUE-076 区分度 | e2e 断言 elapsed ∈ [1.5s,8s)、exit1、NODE_TIMEOUT、TimedOut、NodeTimedOut=1；未转发 hint 时慢 fake 10s 后 Completed → 会失败 | passed（CLI e2e + in-process `harness_ac08` 两处） |

### 4.4 静态/边界核对

| 项 | 结果 |
|---|---|
| 无 mock | 新增测试用真实 CLI 二进制/真 SQLite/真 worker/真 gate；唯一 fake 是 CI 规定 fake LLM（AC-01/ISSUE-076 需脚本化）；扫描无 `FakeExecutor`/stub 路径 |
| 断言非恒真 | 逐项硬断言（状态/字段/计数/退出码/DB 行/字节/marker/signal）；无 `assert!(true)`/`todo!`/被注释断言 |
| 架构边界 | 只改 `tests/e2e/**` + `tests/fixtures/graphs/**`；`Cargo.lock` +1 行 process 机械 diff（Lead 批准）；无 `crates/**`/`python/**`/`doc/**` 改动 |
| 函数长度 | 新增测试 3 处 >60（91/65/66 行，§3 #2）；其余支持函数/cli_support 均 ≤60 |
| fixtures 卫生 | 新增 fixtures 均为文本资产（agents/prompt/fake_llm/py/tools/.gitkeep）；`git ls-tree` 无 pyc/db；运行生成 ignored `__pycache__`（§3 #4） |
| 时序稳定性 | 20ms 轮询 + 15s 上限；无固定 sleep 猜完成；ISSUE-076 用宽区间；crash 用例 3/3；e2e 全包复跑绿 |
| Cargo.lock | 仅 +`rusqlite`（e2e dev-dep）；无新包/版本变化 |
| 提交/门禁 | 4 commit 均含 `Review: REV-032`/`Agent: e2e-dev`；worktree clean；fmt/clippy(fresh)/check PASS |

## 5. 遗留问题

- **产品缺陷待登记（§3 #1）**：kernel `fail_timeout`（执行器自报 Timeout）未 `set_instance_error` → `agent_call` 超时时 `instances.error_code/error` 为 NULL，与 watchdog 路径/06 §6.2.5 不一致；e2e 已如实注释并报 Lead，建议登记 `doc/issues.md` 并由 kernel 修复（补 `set_instance_error` + 单测；e2e 修复后断言 `NODE_TIMEOUT`）。**不阻塞本分支**。
- **建议项**：§3 #2 拆分 3 个超长测试函数（并入 ISSUE-081 清单）；#3 只读证据同时比 `-wal/-shm` 且字节快照先于 RW 查询；#4 可选 `PYTHONDONTWRITEBYTECODE=1` 或 docs 注明 pycache；#5 e2e Cargo description 更新 M5。
- **台账（合并后按事实）**：ISSUE-048（trace 进行中/截断提示已有 e2e 覆盖）、ISSUE-084（标定 + **真 kill -9 e2e** 已有，07 §7.3 #6 可关联）、ISSUE-076（CLI/进程内 e2e 已有）可评估关闭；ISSUE-079 仍 open（M6，ac12 已按约注明口径）。§3 #1 需新增 ISSUE。
- **环境/门禁**：共享 `target/` clippy 会被并行 worker worktree 污染（E0027，本分支源码无关）；合并 dry-run 必须用**独有 fresh target**（Lead 已要求）；`git fetch origin main` 本轮 dry-run 首次成功并全部通过；若其他评审环境 fetch 瞬时失败，按 Lead 口径重试一次。
- **跨模块声明**：`Cargo.lock` +1 行（`rusqlite`）属 process，Lead 已批准；本记录声明 e2e+process 跨模块并由非作者评审。
- 本评审未修改任何产品代码/测试/台账；本记录为本分支唯一新增文件（评审对象 `e97617c` + 本记录提交）。

## 6. 条件关闭记录

不适用（结论为「通过」；§3 均为不阻塞建议/备注）。
