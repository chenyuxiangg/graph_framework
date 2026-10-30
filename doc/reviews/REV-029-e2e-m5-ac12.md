# REV-029 · e2e · M5 AC-12 多 run 并发隔离（10 run / 同库 / 全局与每 run 上限）

> **模块声明**：本分支只改 `tests/e2e/**`（`ac12.rs` 新增、`common/mod.rs` 抽取进程区间辅助、`concurrency.rs` 适配、`Cargo.toml` 注册 `[[test]]`、`fixtures/py/e2e_fixtures/ac12.py` 新增），符合 AGENTS §2 `e2e` 模块边界；**无** `crates/**`、`doc/**`、`scripts/**`、`Cargo.lock` 改动，无 mock 路径。作者 `e2e-dev`，评审人 `reviewer-b`（**非作者**）。评审过程未修改任何产品代码/测试/台账/其他文档，本次提交仅新增本评审记录；未 commit 产品代码、未 merge、未 push、未 rebase。
>
> **独立核对口径**：不采信作者自述。亲自读 `origin/main...HEAD` 全量 diff 与 876 行测试全文、逐项对照 07 AC-12 / architecture §8 D4/D8 / DR-002 §5、核验被测链路的 kernel/worker/trace/harness 源码（`start_traced_run` 真并发、`WorkerExecutor::with_limits` 信号量、TraceWriter WAL/busy 重试、`WorkerContext.get_var/user_id`），并在本 worktree 实跑 AC-12 专项 ×3、e2e 全包、workspace 全量、e2e 与 workspace clippy、fmt、`check.sh`；另做静态扫描（mock/绝对路径/恒真断言/被注释断言/函数长度/pycache/禁入文件）与峰值算法推演。

- **分支**：`test/e2e-m5-ac12`
- **作者**：`e2e-dev`
- **评审人**：`reviewer-b`
- **变更范围**：`1cc91235bb0132e0ccd4e4027c31d2e2c38ed207..5549a49c35efea4e1edeb6a9375b357d6e84456c`（1 commit `5549a49 test(e2e): AC-12 多 run 并发隔离验收（REV-029）`；`git diff --shortstat` = **5 files changed, 1064 insertions(+), 62 deletions(-)**；base = merge-base = `origin/main` = `1cc9123`，ahead 1，**已基于最新 main**，无需 rebase）
- **评审时间**：2026-10-30
- **关联**：spec 07 **AC-12**（并发 10 run / 同图不同 run_id / 同库允许 / instances·gate_results·events 不串 / global_vars·user_id 不串 / 全局与每 run 上限 / 总耗时基线）；architecture-v2 §8 **D4/D8**；**DR-002 §5**（M5 AC-12 口径，当前在 `docs/process-m5-kickoff` worktree、未合并）；ISSUE-047（AC-12 待 M5）、ISSUE-079（工具并发独立信号量）、REV-015（M3 并发 e2e）、REV-022（AC-08 真链路样板）

## 1. 结论

**结论：通过**

本分支把 AC-12 的 M5 口径落成**真链路**验收，主体质量高、断言真实、稳定性好：

- **真并发、真子进程、真 SQLite（同库）**：`launch_runs` 依次**只启动不等待** 10 个 `start_traced_run`（`ac12.rs:338-362`），随后 `finish_runs` 才逐个 await（`:365-381`）；8 个纯 python run 走共享 `WorkerExecutor`（真 `python -m zgraph_worker.runtime` 子进程），2 个 agent run 走 `RoutingExecutor`（`CompiledHandler::Agent`→`HarnessExecutor` + fake LLM，其余→同一共享 worker；`:277-331`）；10 个 run 全部写**同一个** SQLite 文件（`:853` + `start_traced_run(spec, executor, db)`，`:351`）。共享只读 reader 上 `stats.db_runs == 10`（`:428-434`）证明"同库 10 run"。
- **隔离断言是"逐 run 查询 + 精确计数 + 跨 run 互斥"**：`assert_run_trace`（`:452-464`）对每个 run 按 `run_id` 查 `run/instances/events/gate_results/transitions/streaming_events`，断言 user_id、entry_inputs、graph_name、实例数（5/4）、实例终态全 Completed、节点集合、gate 行数（6/3）与 instance 归属、transitions 归属、流式段归属；跨 run 再断言 instance_id 互不相交（`:440-448`）与**同 kind run 的事件形状逐项一致**（`:700-724`，agent run 去掉时序相关的 `Streaming` 段计数）。这是能发现"串写/错 run_id"的有效断言，不是只靠查询过滤的恒真断言。
- **`global_vars`/`user_id` 不串链路真实**：fixture 在 worker 子进程内回显 `ctx.user_id` 与 `ctx.get_var("tenant")`（`ac12.py:56-59,74-79`），断言落在实例输出（`ac12.rs:546-558,570-576`）与 end 整包输出（`:727-741`）；payload 含 `run_tag`，且每个 run 的 `user_id`/`tenant`/`run_tag` 互不相同。worker 侧 `build_run_request` 按 `ctx.user_id()/ctx.global_vars()` 逐 run 注入（`crates/zgraph-worker/src/executor.rs:186-204`），`ctx.get_var` 读的正是 run 载荷（`python/zgraph_worker/runtime.py:108`）——链路真实、非自证。
- **上限断言用真实进程区间**：`with_limits(4,2)`（`:88-90,855-856`），fixture 每分支/echo 把 `start/end` 墙钟追加到共享日志（`ac12.py:63-79`），测试按 `run_tag` 分组求峰值（`common/mod.rs:567-598`）。**独立实测 3/3 稳定：区间总数 26、全局峰值 4、每 run 峰值 `[2,2,2,2,2,2,2,2,1,1]`**，与作者自述一致；区间配对键含 `run_tag/tag/pid`，跨 run PID 复用不会错配；同一时刻先结算 end 再算 start（保守，不高估）。
- **稳定性与规范**：`cargo test -p zgraph-e2e --test ac12` **3/3 passed**（3.47–3.70s；日志 `/tmp/rev029-*`）；e2e 全包 **24 passed / 0 failed / 1 ignored**（M4 基线 23/1，+1 不缩水）；workspace 全量 **424 passed / 0 failed / 2 ignored / 52 个测试二进制**（M4 基线 423/2 + 本分支 1，无新增失败、无缩水）；`cargo clippy -p zgraph-e2e --all-targets -- -D warnings` **exit 0**，`cargo clippy --workspace --all-targets -- -D warnings` **exit 0**；`cargo fmt --all -- --check` **exit 0**；`bash scripts/dev/check.sh` **CHECK PASSED**；worktree clean（仅 ignored `__pycache__`，`git ls-tree -r HEAD` 无 `.pyc`/`.db`/日志）。函数全部 ≤41 行；无绝对路径、无 Rust `sleep` 猜时序、无被注释/恒真断言。

未发现阻塞/重要缺陷；6 条建议/Nit（组合口径、工具并发未标注、峰值断言强度、文档数字/措辞）见 §3，均为**不阻塞合并**的口径/加固/注释项，其中 #1、#2 建议由 Lead/docs 承接确认。

## 2. 四维评审

| 维度 | 结论 | 依据（文件/行/测试/命令） |
|---|---|---|
| 功能正确性 | ✅ | ① AC-12 四条预期逐项落地：全部 Completed/无 error（`:402-425`）；同库 db_runs=10（`:428-434`）+ 逐 run 归属与精确计数（`:452-698`）；global_vars/user_id 回显（`:546-558,570-576,727-741`）；全局/每 run 峰值（`:777-843`）；耗时比记录（`:806-816`）。② 真链路实核：`start_traced_run` 打开 trace 写端后 `kernel::start_run` 立即返回、run 任务独立推进（`crates/zgraph-cli/src/bridge.rs:254-276`），`WorkerExecutor::execute` 先每 run 后全局信号量再 spawn 子进程（`crates/zgraph-worker/src/executor.rs:141-181`）；同一 DB 由 10 个 TraceWriter（WAL + 5s busy_timeout + busy 重试 3×50ms，`crates/zgraph-trace/src/schema.rs:20-26,368-376`、`writer/tx.rs:111-133`）并发写，3/3 无写端错误（`wait()` 遇写失败即 Err→panic）。③ agent run：fake LLM 为 CI 规定口径（AC-08a），工具 `add` 为**真 Python worker 子进程**，断言第二轮 Tool 消息 `{"sum":5}`（`:759-768`），`requests.len()==2` 且每 run 独立 provider（`:756-758`）。④ 事件形状/流式：lifecycle 恰一次 RunStarted/RunFinished、seq 严格递增（common `assert_lifecycle_shape`），token 段拼接 == content（`:669-698`）。 |
| 代码结构合理性 | ✅ | 只新增/改动 e2e 模块；`common/mod.rs` 把 `concurrency.rs` 原有的区间解析/峰值算法**原样抽取**为 `read_intervals`/`read_intervals_by_run`/`peak_concurrency`（`:520-598`，diff 为纯搬迁），`concurrency.rs` 只改调用点（`:141,246,304`），既消除重复又让 AC-12 复用同一度量；测试文件按"图构造/执行器构造/启动收集/断言/限度量"分节，`RoutingExecutor` 与 `harness_ac08.rs` 同构且只做 handler 路由（不改变语义，`ac12.rs:277-294`）；复用 `common::worker_config/executor_with_limits/compile_text/entry_inputs/events`，不重复造轮子；`Cargo.toml` 只加 `[[test]]`（无新依赖、无 `Cargo.lock`）。 |
| 逻辑性 | ✅ | ① 并发模型自洽：启动循环不含 await（真并发），等待有 `tokio::time::timeout(RUN_TIMEOUT=30s)` 兜底（`:866-868`），失败沿 `panic` 传播、不吞异常；每个用例独立 `TempDir`，baseline 用独立 db/日志不污染主日志（`:384-395`）。② 峰值算法正确：`(time, delta)` 排序，同时刻 end 先于 start（保守），峰值取 max（`common/mod.rs:576-598`）；配对键含 `run_tag/tag/pid`，`start/end` 一一配对；日志短行（fixture 单次 write 追加）避免多进程交错。③ 上限顺序与 worker 实现一致（先 per-run 后 global，无交叉等待；`executor.rs:141-181`）；每 run 峰值 ≤2 是**逐 run 断言**（`:833-839`），`observed_max==2` 证明"达到且不超过"上限。④ 无 sleep 猜时序：Rust 侧无 `tokio::time::sleep`/`std::thread::sleep`，只有 fixture 的**工作负载** sleep（`BRANCH_SLEEP_SECONDS`）与轮询/超时。⑤ 3/3 实跑无 flaky。 |
| 可读性 | ✅ | 文件头 docstring 画出数据通路、列出 8+2 组合与理由、给实测基线与"SQLITE_BUSY 退化方案"（`ac12.rs:1-43`）；常量集中且命名自解释（`:69-96`）；断言函数按主体拆分，最长 `assert_worker_limits` 41 行，全部 ≤60；失败消息带 run tag/节点/实际值，便于定位；Python fixture 有模块/函数 docstring、全类型注解、双引号、`snake_case`、常量全大写，函数 ≤17 行（AST 实测）。扣分项仅为文档数字/措辞不一致（§3 #4/#5/#6，Nit）。 |

## 3. 发现清单

| # | 级别 | 位置 | 问题 | 建议 | 状态 |
|---|---|---|---|---|---|
| 1 | 建议（口径，需 Lead/docs 确认） | `tests/e2e/ac12.rs:20-30`（组合说明）、`:116-141`（`RunPlan::new`）、`:171-234`（两张图） | 07 AC-12 原文步骤是「并发启动 10 个 **AC-01** run（**同图**、不同 run_id）」，DR-002 §5 也写「10 个并发 run（**同图**不同 run_id）」。实现是 **8 个纯 python 三并发分支图 + 2 个 agent 图**（两张不同的图，且都不是 AC-01 的 `customer_ticket` 全栈图）。核心语义（10 并发 / 同库 / 隔离 / 上限）已覆盖，但"10 个同图 AC-01 run"字面未满足 | 由 Lead/docs 明确 M5 wave1 口径：① 在 DR-002 §5 回写"8 同图 python + 2 agent 全栈"组合及理由（task 允许的组合）；且 wave2 的 `test/e2e-m5-cli-replay`（DR-002 §6 已列 "CLI run/L2 链路 + AC-12 注释"）补 10×同图（最好 AC-01）CLI 链路；或 ② 本分支改为 10 个同图 run、agent 全栈另开用例。**本分支不阻塞**（组合已被作者在文件头如实写明"任务允许的组合"），但需在合并记录/DR 留痕 | open（Lead/docs 承接） |
| 2 | 建议（已知限制标注） | `tests/e2e/ac12.rs:9-30`（数据通路/组合说明）；`:777-843`（`assert_worker_limits`） | AC-12 的"全局/每 run 上限生效"只覆盖 **`WorkerExecutor` 的 python 节点子进程**：agent run 的工具 `add` 由 `Harness` 的 `WorkerToolRunner` 执行，使用**独立的进程级信号量**（ISSUE-079 / DR-002 §4 已登记为 M6 统一），不计入 `with_limits(4,2)`，也不在区间日志里，因此测试的全局峰值可能低估真实进程并发（4 节点进程 + 工具子进程可并存）。代码/注释**未标注**该已知边界，读者可能误以为"全栈所有 worker 子进程都受全局上限" | 在文件头或 `assert_worker_limits` 加一条注释：本测试的上限口径 = `WorkerExecutor` 的 python 节点进程；工具子进程走 Harness 独立信号量（ISSUE-079，M6 统一）；若需覆盖工具并发，再补工具侧区间度量。不阻塞（ISSUE-079 已跟踪、DR-002 §4 允许 M6） | open（建议随下次触碰本文件补；或 Lead 确认可合并） |
| 3 | 建议（断言强度） | `tests/e2e/ac12.rs:789-810`（全局峰值 `<=4` 且 `>=2`）、`:806-810`（比值 `<10`） | ① 全局上限断言只有上界 `<=GLOBAL_MAX_WORKERS(4)` + 下界 `>=2`，**无法区分全局上限被错误设为 2 或 3**（该情形下峰值 2/3 仍满足全部断言；只有 ≤1 或 >4 会被抓）。每 run 上限由"逐 run ≤2 + observed_max==2"有效约束。② 耗时断言只排除"完全串行 ≈10x"，接近串行（如 9.5x）仍会通过；AC-12 原文只要求"记录基线"，故字面可接受，但断言区分度弱 | ① 实测 3/3 全局峰值稳定为 4，可把下界收紧为 `>=3` 或直接 `==GLOBAL_MAX_WORKERS`（若担心慢机可保留 `>=3`）；或加一个差分用例 `with_limits(2,2)` 证明"配置值改变 → 峰值改变"。② 比值保持宽松上界可接受；建议把实测比值写入测试内注释/doc 基线（当前仅 `eprintln!`，普通 `cargo test` 被捕获不可见），或收紧到如 `<6` 并保留 10x 作为口径说明 | open（可选加固） |
| 4 | Nit（文档数字） | `tests/e2e/ac12.rs:32-40`（文件头基线）与 `:811`（内联注释） | 两处基线数字互相矛盾且与本次实测有偏差：头部写"单 run 0.79–0.85s、10 run 2.46–2.54s、比值 2.9–3.1"；`:811` 写"total≈3.5–4.5s、baseline≈0.9–1.2s、比值≈3–4"；本次独立实测 3 次为 baseline 0.84/0.93/0.95s、total 2.61/2.67/2.72s、比值 2.86/2.88/3.10（与头部接近，与 `:811` 不符） | 统一两处数字为同一组实测区间（建议以头部为准，或都改成"本机实测区间"并注明机器/日期）；不阻塞 | open（可选） |
| 5 | Nit（注释与实现不符） | `tests/e2e/ac12.rs:27-28` | 文件头称"每个 run 的叶子 reader 与最终共享 reader 都验证 run 级隔离"，但代码只用**最终共享 reader**（`:429`）做全部 run 级查询；`BridgeOutcome.reader`/每 run reader 未被查询（`FinishedRun` 也不持有 reader） | 要么改成"最终共享 reader 逐 run 验证"（成本最低），要么真的对每个 run 的 reader 各查一次（可作为写入端关闭后的二次隔离验证）；不阻塞 | open（可选） |
| 6 | Nit（断言可读性） | `tests/e2e/ac12.rs:599-607,642,658-660,682` | 各 `assert_eq!(row.run_id, run.run_id)` 类断言在 `TraceReader::instances/events/gate_results/...` 已按 `run_id` 过滤的前提下是**恒真**的；真正的隔离证据是精确计数、跨 run instance_id 互斥、同 kind 形状一致。当前写法不错但可读性上会让人误以为该断言承担隔离职责 | 可在这些断言旁注明"query 已按 run_id 过滤，此断言是防御性一致性检查；隔离由计数/形状/互斥断言保证"；不阻塞 | open（可选） |
| 7 | 备注（复用 fixtures） | `tests/e2e/ac12.rs:296-331`（复用 `tests/fixtures/graphs/harness_ac08/**`） | agent run 的 fake LLM 脚本、agent 配置、prompt 与工具均复用 AC-08 fixture；若 AC-08 fixture 有意变更（脚本内容/工具签名），AC-12 的 `content=="{\"answer\":\"5\"}"`、`iterations==2`、`sum==5` 断言会一起失败 | 可接受（避免重复资产、且正是"完整栈"复用）；建议在注释说明该耦合，或在 M5 收尾 docs 记录两测试共享 fixture；不阻塞 | 无需处理（已记录） |

> 无阻塞项、无重要项。#1 为 AC-12 组合口径偏差（需 Lead/docs 确认，wave2 补链），#2 为 ISSUE-079 已知限制未在测试中标注，#3 为可选断言加固，#4–#6 为文档/注释 Nit，#7 为 fixtures 复用备注。**本评审认为本分支无需为合并新开 P1/P2 ISSUE**；若 Lead 不在 DR-002 §5 回写组合口径，则建议登记一条 e2e/docs ISSUE 跟踪 #1。

## 4. 验证证据

### 4.1 范围、分支与提交元数据

```bash
cd /Users/zyzs/lczyzs/code/zgraph/.worktrees/e2e-e2e-dev
git branch --show-current        # test/e2e-m5-ac12
git rev-parse HEAD               # 5549a49c35efea4e1edeb6a9375b357d6e84456c
git merge-base origin/main HEAD  # 1cc91235bb0132e0ccd4e4027c31d2e2c38ed207（= origin/main，未落后）
git rev-list --left-right --count origin/main...HEAD   # 0	1
git diff --shortstat origin/main...HEAD                # 5 files changed, 1064 insertions(+), 62 deletions(-)
git diff --name-status origin/main...HEAD
# M tests/e2e/Cargo.toml；A tests/e2e/ac12.rs；M tests/e2e/common/mod.rs；
# M tests/e2e/concurrency.rs；A tests/e2e/fixtures/py/e2e_fixtures/ac12.py
git log -1 --format='%an <%ae> / %cn <%ce> / %s%n%b'
# chenyuxiangg <1007423165@qq.com> / chenyuxiangg <1007423165@qq.com> / test(e2e): AC-12 多 run 并发隔离验收（REV-029）
# 正文含 Review: REV-029 / Agent: e2e-dev
git ls-tree -r --name-only HEAD | grep -E "__pycache__|\.pyc$|\.db$|\.sqlite" || echo none   # none
git status --short               # 空（仅测试运行生成的 ignored __pycache__）
```

- 未改 `crates/**`、`python/**`、`doc/**`、`scripts/**`、`Cargo.lock`；`Cargo.toml` diff 仅 `[[test]] name="ac12"` 4 行。新增 fixture 属 e2e 模块边界。
- `common/mod.rs` 的 `parse_interval_lines` 为私有实现，`read_intervals`/`read_intervals_by_run`/`peak_concurrency` 为 `pub` 复用；`concurrency.rs` 仅替换调用点，语义未变（4 个 M3 并发用例全过）。

### 4.2 必跑命令与结果（worktree 内；`CARGO_HOME=<repo>/.cargo-home`、`CARGO_NET_OFFLINE=true`、`CARGO_TARGET_DIR=<repo>/target`、`ZGRAPH_PYTHON=<repo>/.venv/bin/python`）

| 命令 | 结果 |
|---|---|
| `cargo test -p zgraph-e2e --test ac12 -- --nocapture` ×3 | **3/3 passed**（每次 1 passed / 0 failed；finished 3.47s / 3.70s / 3.64s）。三次 `[AC-12]` 实测：区间组=10、**全局峰值=4**、**每 run 峰值=[2,2,2,2,2,2,2,2,1,1]**；baseline 928/952/843ms、total 2.67/2.72/2.61s、比值 2.88/2.86/3.10 |
| `cargo test -p zgraph-e2e`（全包 7 个 test binary） | **exit 0：24 passed / 0 failed / 1 ignored**（ac12 1、cancel 2、concurrency 4、fault_injection 7、harness_ac08 4+1 ignored、parallel 4、timeout_kill 2；日志 `/tmp/rev029-e2e.log`） |
| `cargo test --workspace`（全量，52 个测试二进制） | **exit 0：424 passed / 0 failed / 2 ignored**（M4 基线 423/2 + 本分支新增 ac12 1；无新增失败、无基线缩水；日志 `/tmp/rev029-ws.log`） |
| `cargo clippy -p zgraph-e2e --all-targets -- -D warnings` | **exit 0 / 0 warning**（`/tmp/rev029-clippy-e2e.log`） |
| `cargo clippy --workspace --all-targets -- -D warnings` | **exit 0 / 0 warning**（含 zgraph-worker/harness/e2e 重新检查；`/tmp/rev029-clippy-ws.log`；CI 口径不回归） |
| `cargo fmt --all -- --check` | **exit 0** |
| `bash scripts/dev/check.sh` | **CHECK PASSED**（身份/remote/CLAUDE.md/暂存区/分支名/Python 语法/progress/issues/fmt/git diff --check 全 OK） |
| `git status --short`（全部命令后） | 空 |

### 4.3 真实性/无 mock/静态核对

| 项 | 结果 |
|---|---|
| 真 kernel + 真 trace（同库） | `start_traced_run` → `kernel::start_run` → `TraceBridge` → `TraceWriter`（`crates/zgraph-cli/src/bridge.rs:254-276`）；10 个 run 全部传入同一 `shared_db` 路径（`ac12.rs:853,351`）；`stats.db_runs==10`（`:431-434`）；断言全部经 `TraceReader` 查**真 SQLite**（读者代码 `crates/zgraph-trace/src/reader.rs`），无 `MemoryTraceSink`/mock |
| 真 Python worker 子进程 | 8 python run 的 3 分支 + 2 agent run 的 echo 全部经 `WorkerExecutor` spawn `python -m zgraph_worker.runtime`（`common::worker_config`）；26 条真实进程区间（8×3+2×1）与实测全局峰值 4 交叉印证；agent 工具 `add` 经 `WorkerToolRunner` 真子进程，第二轮 Tool 消息 `{"sum":5}`（`:759-768`） |
| 10 run 真并发 | 启动循环无 await（`:338-362`），等待阶段才逐个 await；实测 total 2.6–2.7s vs 单 run 基线 0.84–0.95s（比值 2.86–3.10，远小于完全串行 ≈10x），且全局峰值 4 证明 4 个 worker 子进程同时在跑；非"顺序 await 伪并发" |
| 同库隔离断言 | 逐 run 查询（`run/instances/events/gate_results/transitions/streaming_events`）+ 精确计数（5/4 实例、6/3 gate、26 区间）+ 跨 run instance_id 互斥（`:440-448`）+ 同 kind 事件形状一致（`:700-724`）；row.run_id 类检查为防御性（见 §3 #6） |
| global_vars/user_id 链路 | worker payload `user_id`/`global_vars` 来自 `NodeContext`（`executor.rs:186-204`），Python `WorkerContext.user_id/get_var`（`python/zgraph_worker/ctx.py:41,72`），fixture 回显（`ac12.py:56-59,74-79`），断言逐 run 互不相同（`:546-558,570-576,727-741`）——真实经过 worker 进程 |
| agent run fake LLM 合规 | `FakeLlmProvider`（AC-08 CI 口径；真实模型手动项属 AC-08b，不在 AC-12 范围）；`fake_mode: true`；每 agent run 独立 provider，`requests.len()==2` 与 Tool 消息断言（`:744-769`） |
| mock 扫描 | `grep -niE "mock|FakeExecutor|stub"` 仅命中文件头"禁止 mock"注释；无 mock 代码路径 |
| 绝对路径 | 新增源码/fixture 中无 `/Users/`、`/tmp/`、盘符路径；fixture 路径由 `common::graphs_root()`/`env!("CARGO_MANIFEST_DIR")` 相对推导 |
| 恒真/被注释断言 | 无 `assert!(true`/`assert_eq!(x,x)`/`#\[ignore\]`/`todo!`/`unimplemented!`/注释掉的断言；全部关键断言生效（见 §4.2 实测） |
| 函数长度 | 括号实测：`ac12.rs` 最长 41 行（`assert_worker_limits`），其余 ≤34；`common/mod.rs` 最长 32；`concurrency.rs` 最长 35；Python fixture 最长 17 行（AST）——均 ≤60 |
| pycache/禁入文件 | `git ls-tree -r HEAD` 无 `.pyc`/`__pycache__`/`.db`/`.sqlite`/日志；`git status --ignored` 中的 pycache 为测试运行产物且被 `.gitignore` 忽略 |

### 4.4 上限/耗时断言的独立推演（回应"是否可能低估/高估、是否能区分上限"）

- **算法**：`peak_concurrency` 把每个区间拆成 `(start,+1)/(end,-1)`，按时间排序、同时刻 `-1` 排在 `+1` 前（先结算 end），扫描取最大。等价于闭区间"端点不重叠"的并发计数，**只会低估不会高估**。
- **低估来源（已识别）**：区间是 fixture 用户代码的 `time.time()` 窗口，不含 worker 进程启动/握手/退出；工具子进程（Harness 独立信号量）不写区间。因此测得的 4/2 是"节点用户代码并发"的下界；但上限本身由 `Semaphore` 强制、不依赖测量，且 3/3 实测稳定命中 4/2，足以证明上限生效与"达到上限"。
- **时钟/写序**：同一主机 wall clock；每行 <60B 且 fixture 以 `open(...,"a")` 单次 write 追加（`ac12.py:97-100`），POSIX `O_APPEND` 短写不会交错；配对键 `(run_tag,tag,pid)` 防 PID 复用错配；缺行/多行由区间总数断言（26）兜住。未发现导致显著高估/低估的路径。
- **区分度**：全局上限只有 `<=4 && >=2`（无法区分 2/3/4，见 §3 #3）；每 run 上限由"逐 run ≤2 + observed_max==2"有效约束（若 per-run 上限被放大为 3，run0 会取到 3 → 失败；实测 3/3 observed_max=2）。
- **同库写**：10 个 TraceWriter 各自写线程 + WAL + busy_timeout 5s + busy 重试 3×50ms（`schema.rs:20-26,368-376`、`writer/tx.rs:111-133`）；3/3 无写错误（若有写错误 `wait()` 返回 `BridgeError::Writer` → 用例 panic）。文件头已如实注明"若未来慢盘/大流量触发写线程错误，按 AC 允许退化为独立 db 并登记台账"（`:38-40`），符合 DR-002 §5 的兜底口径。

## 5. 遗留问题

- **待 Lead/docs 确认（#1）**：AC-12 的"10 个同图 AC-01 run"字面口径与"8 同图 python + 2 agent"实现的差异，建议在 DR-002 §5 回写组合与理由，并由 wave2 `test/e2e-m5-cli-replay`（DR-002 §6 已列"CLI run/L2 链路 + AC-12 注释"）补 10×同图/AC-01 CLI 链路；若 docs 不回写则建议登记一条 e2e/docs ISSUE。
- **ISSUE-079（#2）**：工具并发用 Harness 独立信号量、不在 `WorkerExecutor::with_limits` 的全局/每 run 统计内；本测试未标注该边界。建议随下次触碰补注释（或 M6 统一信号量后由 docs 回写）。
- **可选加固（#3）**：全局峰值断言可收紧到 `>=3`/`==4` 或增加差分 `with_limits(2,2)` 用例；耗时比值可保留宽松上界但把实测值落入注释/基线文档。
- **Nit（#4/#5/#6）**：文件头与内联注释的基线数字不一致、"每 run reader"措辞与实现不符、row.run_id 防御性断言缺说明——均可随手修正，不阻塞。
- 本评审未修改任何产品代码/测试/台账；本记录为本分支唯一新增文件。

## 6. 条件关闭记录

不适用（结论为"通过"）。
