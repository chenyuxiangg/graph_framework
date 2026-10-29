# REV-015 · e2e · M3 真 worker 验收包（AC-06/07/09/10 + 故障注入 + 并发上限）

> **模块声明**：主体只新增 `tests/e2e/**`（`Cargo.toml`、`common/mod.rs`、`timeout_kill.rs`、`parallel.rs`、`cancel.rs`、`fault_injection.rs`、`concurrency.rs`、`fixtures/**`），符合 AGENTS §2 `e2e` 模块边界（`tests/e2e/**`、`tests/fixtures/graphs/**`；本次未改 `tests/fixtures/graphs/**`，直接复用既有 `valid_parallel.json`/`bad_dup_port_source.json`）。作者 `e2e-dev`，评审人 `reviewer-a`（**非作者**）。
>
> **跨模块 process 授权（Lead 已批准，task-4 writeScopes 含 `Cargo.toml`/`Cargo.lock`）**：本分支额外改动根 `Cargo.toml`（`members += "tests/e2e"`，+1 行）与 `Cargo.lock`（新增 `zgraph-e2e` 包条目，+16 行；无新增第三方 crate——`chrono`/`nix 0.29`/`tempfile` 等均已在 workspace/lock 内）。未改 `.github/**`、`crates/**`、`python/**`、`doc/**`。该 process 边界改动属机械性 workspace 成员登记，按流程留待 `reviewer-b` 第二评审（见 §5.2）。

- **分支**：`test/e2e-m3-acceptance`
- **作者**：`e2e-dev`
- **评审人**：`reviewer-a`
- **变更范围**：`23123cf..74aaf21`（base=`origin/main`=`23123cf`，单 commit `test(e2e): 建立 tests/e2e 真 worker 验收包…`；`git diff --shortstat` = **19 files, +2237/−0**；分支基于最新 main，可 ff，未 rebase）
- **评审时间**：2026-10-30
- **关联**：task-4（M3 DoD 验收）；`doc/spec-v1.2/07-验收用例.md` AC-06/07/09/10/12 + §7.1（`tests/e2e/**` 路径）/§7.2；`doc/architecture-v2.md` §6.1 M3/§6.2 AC 映射/§8.1（D1/D8 落点）/§8 **D1/D4/D8**；ISSUE-025/047/052/054/015/017；REV-012（worker supervisor/执行器）、REV-013（`valid_cycle.json` 图级不可运行）

## 1. 结论

**结论：有条件通过**

验收包**主体质量高**：数据通路全部为真（kernel `start_run` → 真 `python -m zgraph_worker.runtime` 子进程 → 真 SQLite trace → `TraceReader`），无 mock；AC-06/09/10 的进程组 kill 断言基于真实 worker + 孙进程 PID 消失（`kill(pid,0)` 异步轮询，非 sleep 猜时序）；故障注入 7 类映射双端（outcome + trace）核对；并发上限用真实进程区间峰值度量；确定性 5 连跑摘要一致；跨 run 隔离断言实例/事件/输出不串。实测：`cargo test -p zgraph-e2e` ×3 **19/19 全绿**（6–7s/次），`cargo test --workspace` ×2 **313 passed/0 failed**（17–19s/次），`cargo build/check --workspace` exit 0，fmt/clippy/check.sh 全过；22 次 e2e 定向重复（parallel ×5、concurrency ×5、cancel/timeout/fault 各 ×3）全绿；全程**无残留 python/孙进程**。

但存在 **1 项合并前置条件（C1，重要）**：AC-09 的断言强度低于冻结口径——spec 07 AC-09 要求「Running 时间差 **<50ms**」（本包用 **250ms**）且「总耗时显著小于串行（**M3 实测后回填阈值**，初值 <1.0s 仅为占位）」（本包保留占位 **1.0s**，未补串行对照）。我独立实测：a/b 进入 Running **0ms 时间差**、并行总耗时 **252–292ms**、同两 worker 串行对照 **488–528ms**——即数据支持更严断言，但当前阈值既不能证伪「显著小于串行」，250ms 也弱于 spec。属测试交付物本身的断言强度问题（非功能缺陷），一行常量 + 阈值/对照即可关闭；关闭后由评审追加记录即通过。

对 e2e-dev 报告的三项发现，我的判断（独立核验见 §4.8）：① kernel gated self-loop 幻影 attempt/max_attempts 误判——**成立（我已用探针复现）**，P2 kernel 缺陷，e2e 不改 `crates/**` 的处理正确，**应登记 ISSUE 后续修，不阻塞本分支**；② `valid_cycle.json` 真内核不可运行——**成立（探针复现 `RUN_DEADLOCK`）**，与 REV-013 #1 一致，**登记 ISSUE 即可，不阻塞**；③ AC-07 真 worker ticker——字面 ticker 已由 M2 默认 workspace 测试覆盖（本次实跑确认 `acceptance_m2::ac07_watchdog_ticks_and_pause_resume` 与 `test_hooks::ticker_interval_stays_small_while_executor_runs` 均 ok），本包用「真 worker 执行中 Pause ≤100ms」等价验证事件循环不阻塞，**可接受登记 ISSUE + 后续补（不阻塞）**，需 Lead 登记并在 07/architecture 统一口径。

## 2. 四维评审

| 维度 | 结论 | 依据（文件/行/测试） |
|---|---|---|
| 功能正确性 | ⚠️ | AC-06：`timeout_kill.rs:32-113`（Failed/NODE_TIMEOUT/TimedOut/NodeTimedOut 恰一次/RunFinished 恰一次/`killed_workers≥1`/`abandoned_tasks=0`/`elapsed<5s` 且 `≥900ms`）与 `:115-161`（worker+孙进程 PID 消失）——全绿且为真实进程证据；AC-10：`cancel.rs`（Cancel 幂等/Pause 期间不派发/trace 断言/PID 消失）；故障注入 7 类 `fault_injection.rs`（outcome+trace 双侧）；并发上限 `concurrency.rs`（峰值 2/≤2且≥2/3 + 双 run 隔离）；确定性 5 连跑摘要一致。**扣分**：AC-09 断言强度低于 spec/D8（C1）；AC-07 真 worker ticker 未落地（登记 ISSUE，见 §3 #2）。 |
| 代码结构合理性 | ✅ | 新增 `zgraph-e2e` 测试包（`autotests=false` + 显式 `[[test]]`，对齐 07 §7.1 的 `tests/e2e/*.rs` 路径）；`common/mod.rs` 收敛图构造/executor/trace 查询/PID 断言/生命周期断言；fixtures 与对外示例 `python/workers/*` 分离（e2e 拥有 `tests/e2e/fixtures/**`，不越界 py-worker）；依赖方向 e2e→{cli,kernel,dsl,trace,worker} 单向下游，未改任何产品代码。process 边界改动机械且无新增第三方 crate。 |
| 逻辑性 | ✅ | 时序断言均「异步轮询 + 宽松上限」（`common::wait_for_file`/`assert_pid_gone`/`wait_until`），无 sleep 猜完成；峰值并发用区间扫描（同刻先 `end` 后 `start`，避免零长区间假峰），`per_run/global` 语义真实生效；取消/Pause 的控制命令与状态轮询无竞态；重复 Cancel/Pause/Resume 幂等；无残留子进程（前后 `pgrep` 均空）。 |
| 可读性 | ⚠️ | 模块/函数文档与 D1/D8/AC 溯源注释质量高，常量具名（阈值/超时/上限），fixture 文档含幂等/副作用说明；扣分：3 个测试函数 >60 行（见 §3 #8）、`cancel.rs` 有一段近乎恒真的弱断言（#9）、4 个 raw fixture 脚本重复样板（#11）。 |

## 3. 发现清单

| # | 级别 | 位置 | 问题 | 建议 | 状态 |
|---|---|---|---|---|---|
| 1 | 重要（C1） | `tests/e2e/parallel.rs:30-40`（阈值注释/常量）、`:158-168` | AC-09 断言弱于冻结口径：spec 07 §AC-09 要求「Running 时间差 **<50ms**」，实现为 `BRANCH_START_SKEW_LIMIT=250ms`（注释自述实测 <20ms，我实测 **0ms**）；且 D8 要求「总耗时显著小于串行（M3 实测后定阈值，初值 <1.0s 仅占位）」，实现仍保留 1.0s 占位、**无串行对照**——我实测并行 252–292ms、串行 488–528ms，串行也能通过 1.0s 上限，阈值本身不能证伪「显著小于串行」 | `BRANCH_START_SKEW_LIMIT` 收为 **50ms**（实测 0ms，余量充足）；总耗时按 D8 以实测重标定（如 `<0.6s`）**或**在同用例内跑串行对照断言 `parallel < serial × 0.8`；改后 `cargo test -p zgraph-e2e --test parallel` ×3 全绿 | open（C1） |
| 2 | 重要 | `tests/e2e/cancel.rs:1-14`（AC-07 口径说明）；全包无 ticker 探针 | AC-07 的 M3 真 worker ticker 未实现：spec 07 §AC-07「M3 起真 worker：注入 5ms ticker，最大间隔 <50ms」；本包实现的是「真 worker 执行中 Pause ≤100ms」+ 离线断言，无真 worker 场景的 ticker 间隔断言（`test-hooks` 未启用） | **判断：可接受登记 ISSUE + 后续修，不阻塞**（M2 默认 workspace 已覆盖字面 ticker，Pause 延迟已证明事件循环不阻塞；e2e 包可用 kernel `test-hooks` dev-dependency 补齐，`zgraph-cli` dev-dep 有先例）。需 Lead 登记 ISSUE 并在 07/architecture 统一口径 | open（ISSUE 待登记） |
| 3 | 重要（kernel 缺陷，非本分支代码） | `crates/zgraph-kernel/src/run.rs:980-983`（`resolve_target` 在 `evaluation.passed` 判断**之前**）→ `:1043-1068` | **e2e-dev 发现①成立（我以临时探针独立复现）**：被 gate 拦截的投递仍会 `resolve_target`/`create_instance`。gated self-loop：`max_attempts=3` 时出现幻影 attempt2（`Skipped`）；`max_attempts=2`（实际执行 2 次，第 2 次 gate 拦截）时幻影 attempt2 `>= max_attempts` → **本应 Completed 的 run 被误判 `Failed/NODE_INTERNAL`**。e2e 未改 `crates/**` 正确，且测试注释已如实记录 | 登记 ISSUE（P2、kernel）：`apply_publish` 应先判 `evaluation.passed` 再 `resolve_target`（被拦截边不创建实例/不消耗 attempt）；补 kernel 单测（gated self-loop + max_attempts 边界）。**不阻塞本分支** | open（ISSUE 待登记） |
| 4 | 建议 | `tests/fixtures/graphs/valid_cycle.json`；`tests/e2e/parallel.rs:251-261` | **e2e-dev 发现②成立（探针复现）**：真内核跑 `valid_cycle.json` → `Failed/RUN_DEADLOCK`（start Completed、loop Cancelled）；根因 `loop` 缺省 `trigger=all` 且 `again` 只能由自环边回投 → 首轮永不就绪（与 REV-013 #1 一致；`loop→done` 无门控为第二处隐患） | 该 fixture 仅 DSL R13 校验图，**登记 ISSUE**（e2e/kernel）：补可执行 bounded-loop 图（`trigger:any` + 出口门控）或在 07/e2e 文档标注“不可执行校验图”。**不阻塞本分支** | open（ISSUE 待登记） |
| 5 | 建议 | `tests/e2e/timeout_kill.rs:26,115-161` | AC-06 孙进程用例用节点超时 **1000ms** 固定值：fixture 需先写 worker pid、握手、派孙进程并落盘 child pid（冷启动慢机可能 >1s 才落盘），之后才触发 kill；一旦先 kill 则 `read_pid(child)` 5s 后 panic（潜在 flake；本机 3 跑未复现） | 该用例只断言 `<5s`，可把节点超时放宽到 1.5–2s（仍 <5s），或让 fixture 在握手前先派孙进程并落盘 pid | open |
| 6 | 建议 | `tests/e2e/concurrency.rs:28-29,219-230` | `default_limits_let_all_three_branches_overlap` 断言峰值恰为 3，窗口仅 `sleep=0.3s`；2 核 CI 上 3 个 Python 冷启动若错开 >0.3s 会退化为峰值 2 而误报（本机 5 连跑稳定） | 将并发窗口调到 0.5–0.8s（`BRANCH_SLEEP_SECONDS`），或对 `peak==3` 加一次重试/更长窗口；`per_run/global` 用例同理保留余量 | open |
| 7 | 建议 | `tests/e2e/parallel.rs:251-363`（自环覆盖语义） | AC-09 子项「同一端口两次投递时后者覆盖（v1 单数据源）」在合法图下不可达（DSL R5 禁止同端口多来源）；本包以「R5 反例被拒 + 自环边两次投递（不同 instance）」替代，未能直接验证内核 `buffer_write` 覆盖分支 | 在 07/architecture 回写该子项口径（v1 单数据源 → 覆盖分支为防御性实现/不可达），或登记 ISSUE 说明；与 #3 的 gated self-loop 缺陷联动 | open |
| 8 | 建议 | `tests/e2e/parallel.rs:263`（101 行）、`tests/e2e/cancel.rs:92`（89 行）、`tests/e2e/concurrency.rs:233`（76 行） | 3 个测试函数超过「函数 ≤60 行」等价约束（AGENTS §2.2 / architecture §2.2 对 `crates/**` 的要求同样适用于 Rust 测试） | 抽出 `run_xxx` + `assert_xxx` helper（如 self-loop 的图构造/断言、isolation 的断言段），把函数压到 ≤60 行；语义不变 | open |
| 9 | 建议 | `tests/e2e/cancel.rs:161-175` | Pause 用例中 `at >= paused_at - 50ms` 近乎恒真：`paused_at` 在 Pause 可见后才取，即使 second 在暂停窗口内被错误派发也满足；真正有效的是 `!second_pid.exists()`（`:131-135`）与 `at >= resumed_at - 50ms`（`:172-175`） | 删除 `paused_at` 弱断言（或改为 `second_running_at > paused_at` 且 `< resumed_at` 的窗口断言），避免误导读者以为已覆盖 | open |
| 10 | 建议 | `tests/e2e/concurrency.rs:84-109`；`tests/e2e/fixtures/py/e2e_fixtures/concurrency.py:39` | 区间日志行格式为 `<tag> <pid> <ts> start|end`，未含 `run_tag`；global 用例两个 run 共享日志，按 `(tag,pid)` 配对——若跨 run 发生 PID 复用会把两次执行错配成一个区间（低概率但可消除） | 日志行加入 `run_tag`（payload 已注入），配对键改为 `(run_tag, tag, pid)` | open |
| 11 | 建议 | `tests/e2e/fixtures/workers/{duplicate_result,no_result,result_no_exit,hello_timeout}.py` | 4 个 raw NDJSON fixture 重复 `send`/hello/等 run 样板约 40 行/文件；后续新增协议故障用例会继续复制 | 在 `tests/e2e/fixtures/workers/` 下加 `_raw_common.py`（send/hello/等 run 辅助），fixture 只写故障行为；或明确保留“单文件自包含”约定并注释 | open |
| 12 | 建议 | `tests/e2e/Cargo.toml:36-38` | `nix = { version = "0.29", features = [...] }` 与 `crates/zgraph-worker/Cargo.toml` 重复声明版本/特性（非 workspace 依赖）；未来升级需两处同步 | 按 ISSUE-030 先例提升到根 `[workspace.dependencies]`（process 边界，Lead 决策） | open |
| 13 | 备注 | `tests/e2e/fault_injection.rs:1-18` | 协议错误 kill 路径（无 result/重复 result/result 后不退出/hello 超时）未做 PID 残留断言；worker crate（REV-012）已对同路径断言 PID 消失，e2e 侧重错误码/trace 映射，可接受 | 可选：对 hello_timeout/result_no_exit 补“进程组 kill 后 PID 消失”断言，增强 e2e 端到端证据 | open |
| 14 | 备注 | `tests/e2e/timeout_kill.rs:1-11`；`cancel.rs:1-14` | AC-06/AC-10 用 in-process `bridge`（非 `zgraph run --json` CLI 退出码）；与 M2/ISSUE-054 口径一致，CLI e2e 属 M5；e2e 的取消路径是内核 `task.abort()`→Drop 直接 SIGKILL，未走 supervisor `cancel` 帧升级链（worker 侧 REV-012 已覆盖，架构 §3.4 差异已在 REV-012 接受） | 建议 07 §7.2 回写“M3 用 in-process bridge / M5 补 CLI 退出码”；无需本分支修改 | open |
| 15 | 流程 | `doc/issues.md`；`doc/reviews/README.md` | ①/②/③ 及 AC-09 覆盖口径均尚**未登记台账**（本分支按约束不得改）；REV-015 的 process 边界第二评审待 `reviewer-b` 追加（另：reviews 索引未收录 REV-010+，属 docs 存量问题） | Lead 合并时：登记 ISSUE-061（kernel gated edge 幻影 attempt，P2）/ISSUE-062（`valid_cycle.json` 可执行回环，P3）/ISSUE-063（AC-07 真 worker ticker，P2/P3）/ISSUE-064（AC-09 覆盖语义与阈值口径，P3），更新 ISSUE-047/054（M3 真 worker 已覆盖）；reviewer-b 追加 §5.2 | open |

> 条件 C1 = #1；#2/#3/#4 为「登记 ISSUE 后由 Lead 决策后续修」的已知项，不阻塞本分支合并；#5–#12 建议项可后置或随 C1 一并处理。

## 4. 验证证据

### 4.1 范围与元数据

```bash
cd /Users/zyzs/lczyzs/code/zgraph/.worktrees/e2e-agent-a
git rev-parse --abbrev-ref HEAD          # test/e2e-m3-acceptance
git merge-base origin/main HEAD          # 23123cf…（= HEAD 父，基于最新 main，未 rebase）
git log --oneline -2                     # 74aaf21 test(e2e): 建立 tests/e2e 真 worker 验收包…
git diff --shortstat 23123cf..74aaf21    # 19 files changed, 2237 insertions(+)
git log -1 --format='%an <%ae> | %cn <%ce>'   # chenyuxiangg <1007423165@qq.com>（author=committer）
git diff --name-only 23123cf..74aaf21    # 仅 Cargo.toml/Cargo.lock/tests/e2e/**；无 crates/|python/|doc/|.github/
```

提交正文含 `Review: REV-015` / `Agent: e2e-dev`；worktree 评审前后均 `clean`；本评审为临时探针验证后已删除，未提交任何代码。

### 4.2 必跑命令（worktree 自带 target，未设 `CARGO_TARGET_DIR`；`CARGO_HOME=/Users/zyzs/lczyzs/code/zgraph/.cargo-home`）

| 命令 | 结果 |
|---|---|
| `cargo test -p zgraph-e2e` ×3 | **19/19 passed** ×3（每次 6–7s：timeout_kill 1.65–1.72s、parallel 1.19–1.25s、fault_injection 1.08–1.10s、concurrency 1.34–1.56s、cancel 1.05–1.07s） |
| `cargo test --workspace` ×2 | **313 passed / 0 failed** ×2（17s/19s，50 个测试二进制；含 e2e 19 例） |
| `cargo fmt --all -- --check` | exit 0 |
| `cargo clippy --workspace --all-targets -- -D warnings` | exit 0（无 warning） |
| `bash scripts/dev/check.sh` | `CHECK PASSED`（14 项 OK） |
| `cargo build --workspace` / `cargo check --workspace` / `cargo test --workspace --no-run` | 均 exit 0（test-only workspace 成员未破坏常规构建） |
| 定向重复 | parallel ×5、concurrency ×5、cancel ×3、timeout_kill ×3、fault_injection ×3 —— **全部 0 failed**（合计 22 次二进制运行，含 19 例/次） |
| 进程残留 | 运行前/后 `pgrep -fl zgraph_worker.runtime`、`pgrep -fl "time.sleep(60)"` 均空（无 worker/孙进程残留） |

### 4.3 逐 AC 断言强度核对

| AC | 结论 | 依据 |
|---|---|---|
| AC-06 | ✅（强） | 超时路径断言 7 项齐全（Failed/NODE_TIMEOUT/TimedOut/NodeTimedOut×1/RunFinished×1/`killed_workers≥1`/`abandoned_tasks=0`/`elapsed<5s` 且 `≥900ms` 反“过早就不是超时路径”）；进程组证据用 worker+孙进程真实 PID + `kill(pid,0)` 异步轮询（僵尸仍算存活 → 能抓未 reap）；无假阳性风险点（PID 复用窗口极短）。**余量**：#5（1s 冷启动窗口）。 |
| AC-07 | ⚠️（部分） | 真 worker 执行中 Pause ≤100ms 已实现且通过；字面 5ms ticker + 真 worker 未实现（见 #2）；但 M2 默认 workspace 已跑 `ac07_watchdog_ticks_and_pause_resume` / `ticker_interval_stays_small_while_executor_runs`（本次 workspace 日志确认 ok），非阻塞。 |
| AC-09 | ⚠️（C1） | 重叠度量基于 trace `transitions.at`（真实墙钟，非返回值），我实测 a/b Running 时间差 **0ms**、并行耗时 252–292ms；5 次确定性摘要（每节点迁移链/终态/output）一致；merge 双端口（`gate_results` ra/rb 各一）+ 整包广播 `x={}` 断言扎实。**但** 时间差上限 250ms 弱于 spec 50ms、总耗时阈值未按 D8 实测回填且无串行对照（我实测串行 488–528ms）→ C1。 |
| AC-10 | ✅（强） | Pause：真 worker 在飞时 Pause 可见；暂停 1s 内 second 未派发（pid 文件不存在 + trace `resumed_at` 断言）；Resume 后完成；Cancel：run Cancelled/无 Failed/在飞实例 Cancelled/RunFinished×1/`killed_workers≥1`/error_code=None/PID 消失；重复 Cancel+Pause/Resume 幂等。弱断言 #9 为可读性而非覆盖缺口。 |
| 故障注入 | ✅ | 7 类（异常/导入/无 result/重复 result/result 后不退出/信号/hello 超时）均断言 run Failed + error_code + 实例 Crashed + trace `runs.error_code` + `RunFinished`×1 + 无 NodeTimedOut；PythonException 额外断言 traceback/`exception` 摘要（与 bridge NodeError JSON 形状一致）。 |
| 并发上限 | ✅（余量 #6） | 峰值区间法无漏计（每分支一进程、start/end 成对；同刻先 end 后 start 避免假峰）；`per_run=2` 峰值恰 2（受限且有并行）、`max_workers=2` 跨两 run 峰值 ≤2 且 ≥2、上限充足时峰值 3；run 间隔离断言 outcome/实例集合/instance_id 不相交/events.run_id/日志不串。0.3s 窗口在 2 核 CI 有 #6 风险。 |

### 4.4 独立探针（临时，跑完已删除；worktree 最终 clean）

为核验 e2e-dev 的发现①/②与 AC-09 数据，临时新增 `tests/e2e/zz_reviewer_probe.rs` + `[[test]]` 条目（跑完 `rm` + `git checkout -- tests/e2e/Cargo.toml`，`git status` 空，未入库）：

```text
PROBE self_loop max_attempts=2 status=Failed error=Some("NODE_INTERNAL") loop_attempts=[(0,"Completed"),(1,"Completed"),(2,"Crashed")]
PROBE trace run.error_code=Some("NODE_INTERNAL")
PROBE self_loop max_attempts=3 status=Completed error=None loop_attempts=[(0,"Completed"),(1,"Completed"),(2,"Skipped")]
PROBE valid_cycle status=Failed error=Some("RUN_DEADLOCK") instances=[("start","Completed"),("loop","Cancelled")]
PROBE parallel run#0 elapsed=266.99925ms a_running_at=…:15.791Z b_running_at=…:15.791Z skew_ms=0
PROBE parallel run#1 elapsed=251.797625ms skew_ms=0
PROBE parallel run#2 elapsed=291.637ms skew_ms=0
PROBE serial run#0 elapsed=528.111708ms status=Completed
PROBE serial run#1 elapsed=501.192125ms status=Completed
PROBE serial run#2 elapsed=488.244291ms status=Completed
```

结论：① 成立（gate 拦截仍创建幻影 attempt；`max_attempts=2` 时把本应 Completed 的 run 误判 `NODE_INTERNAL`；e2e 注释的「尝试数+1」措辞略不精确，实际条件是「被拦截投递的 attempt 序号达到 max_attempts」）；② 成立（`RUN_DEADLOCK`，首轮 `again` 永不就绪）；AC-09 数据支持更严断言（0ms/252–292ms vs 串行 488–528ms）→ C1。

### 4.5 e2e-dev 三项发现的处置判断

| 发现 | 独立核验 | 处置判断 |
|---|---|---|
| ① kernel `apply_publish` 先 `resolve_target` 后判 gate | **成立**（探针 + 读码 `run.rs:980-983/1043-1068`） | 属 kernel 模块 P2 缺陷；e2e 不改 `crates/**` 正确、测试已固化现状。**登记 ISSUE + 后续修，不阻塞本分支** |
| ② `valid_cycle.json` 真内核不可运行 | **成立**（探针 `RUN_DEADLOCK`） | 已在 REV-013 #1 指出；登记 ISSUE（补可执行回环图/标注不可执行）。**不阻塞** |
| ③ AC-07 字面 ticker 需 test-hooks，改用真 worker Pause 口径 | **部分成立**（M2 字面 ticker 确实依赖 test-hooks，且已在默认 workspace 覆盖；e2e 真 worker Pause 已实现） | 真 worker ticker 可补（cli dev-dep 有 test-hooks 先例），但 M2 覆盖 + Pause 证据已能证明事件循环不阻塞；**接受登记 ISSUE + 后续补，不阻塞**。需 Lead 统一 07/architecture 口径 |

## 5. 遗留问题

### 5.1 合并前置条件（C1）

| 条件 | 内容 | 关闭证据 |
|---|---|---|
| C1 | AC-09 断言与冻结口径对齐：`BRANCH_START_SKEW_LIMIT` 250ms→**50ms**（实测 0ms）；`PARALLEL_TOTAL_BUDGET` 按 D8 以实测重标定（如 `<0.6s`）或增加串行对照断言（`parallel_elapsed < serial_elapsed × 0.8`）；改后 `cargo test -p zgraph-e2e --test parallel` ×3 全绿 | 待 e2e-dev 修复后由 reviewer-a 复审追加（若 Lead 选择「回写 07/architecture + 登记 ISSUE 说明 250ms/1.0s 依据」的替代方案，需 Lead 批准并同样关闭 C1） |

### 5.2 跨模块 process 授权与第二评审

- 授权：Lead（task-4）批准根 `Cargo.toml`/`Cargo.lock` 的 process 边界改动；实测 `Cargo.lock` 仅新增 `zgraph-e2e` 包条目（无新增第三方 crate），`Cargo.toml` 仅 +1 行 member；`cargo build/check/test --workspace`、clippy、CI 默认 `cargo test --workspace` 均不受破坏（本机 313 passed ×2）。
- **process 边界第二评审：待 reviewer-b 追加**（请复核：workspace 成员新增/`autotests=false`+`[[test]]` 结构、`nix` dev-dependency 版本一致性与是否应提升 workspace.dependencies、test-only 包对 CI `cargo test --workspace` 时长影响（本机 e2e 增量约 6–7s/次））。

### 5.3 非阻塞遗留（需 Lead 登记 / 后续处理）

- **ISSUE 待登记**（建议编号 061 起）：① kernel gated edge 幻影 attempt/max_attempts（P2 kernel，附 e2e 测试名 `parallel.rs::ac09_gated_self_loop_delivers_twice_and_dsl_enforces_single_port_source` 与探针复现）；② `valid_cycle.json` 可执行回环缺失（P3 e2e/kernel，REV-013 #1）；③ AC-07 真 worker ticker（P2/P3 e2e，test-hooks dev-dep 可补）；④ AC-09 覆盖语义（同端口覆盖不可达）与阈值口径（P3 e2e/docs，与 C1 档）。
- 更新 ISSUE-047/054：M3 真 worker 场景（AC-06/07/09/10/12 前置）已由本包覆盖；ISSUE-052（`tests/e2e/**` 路径）已由本分支落地。
- 本评审未修改任何代码/台账/进度/README，仅新增本记录并提交；未 merge/push/rebase。

## 6. 条件关闭记录

| 条件 | 关闭证据 | 关闭日期 |
|---|---|---|
| C1 | 待 e2e-dev 修复后复审追加（`cargo test -p zgraph-e2e --test parallel` ×3、`cargo test --workspace` ×1） | — |
