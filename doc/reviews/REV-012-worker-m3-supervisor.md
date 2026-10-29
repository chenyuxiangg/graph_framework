# REV-012 · worker · M3 supervisor 进程组生命周期与 WorkerExecutor

> **模块声明**：主体只改 `crates/zgraph-worker/**`（`src/{supervisor,executor,spawn,protocol,lib}.rs`、`tests/**`、`tests/fixtures/**`），符合 AGENTS §2 worker 模块边界。作者 `worker-dev`，主评审人 `reviewer-a`（**非作者**）。
>
> **跨模块授权（Lead 已批准，见 `team_task_get("task-1")`）**，本分支额外改动：
>
> 1. `crates/zgraph-kernel/src/context.rs`：`NodeContext` 新增 `seed` 字段 + `seed()` / `global_vars()` 两个只读访问器 + 单测（**无其它行为变化**）；
> 2. `crates/zgraph-kernel/src/run.rs`：仅将 `RunOptions.seed` 传入 `NodeContextInit`（+2 行）；
> 3. `Cargo.lock`：`nix 0.29`（Unix 进程组信号）与 `zgraph-worker → zgraph-kernel` 依赖边；
> 4. `schemas/worker-protocol-v1.md`：§4 补帧超时/并发上限/流式上限口径，§6 检查单闭环，§7 已知限制。
>
> 第 1–3 项属 kernel/process 边界，按流程由 `reviewer-b` 做**跨模块第二评审**并在本记录追加（见 §5.1）。跨模块部分的主评审仅做最小性与依赖方向核对，不作为最终结论依据。

- **分支**：`feat/worker-m3-supervisor`
- **作者**：`worker-dev`
- **评审人**：`reviewer-a`
- **变更范围**：`a6f4f5c..0b6f016`（4 commits：`1325d64` kernel seed 访问器 → `be3671b` supervisor/executor → `104b596` M3 故障注入测试 → `0b6f016` 协议检查单闭环；`git diff --shortstat` = **21 files changed, 2602 insertions(+), 234 deletions(-)**；base = merge-base = `a6f4f5c`，未 rebase；`origin/main` 在此期间前进到 `261d971`，由 merge.sh 合并时自动 rebase）
- **评审时间**：2026-10-30
- **关联**：architecture-v2 §1（关键约束）/§3（协议/生命周期）/§4（并发模型）/§6.1 M3 DoD/§8 **D1/D2/D4/D10** 与 §8.2；spec 03 §3.9（超时/看门狗）、spec 04 §4.1.4（Timeout 产生者）/§4.2（NodeContext）/§4.3.6（Python 执行模型）；`schemas/worker-protocol-v1.md` §2/§3/§4/§6/§7；ISSUE-025（协议检查单）/ISSUE-022/015/017/047/051；REV-011（kernel Timeout 语义）

## 1. 结论

**结论：不通过**

分支主体实现质量较高：进程组 spawn / 信号升级 / Drop 兜底 / wait-reap、帧空闲超时方案 A、流式上限、错误映射矩阵、emit 流式入内核、全局与每 run 并发上限均有**真实 Python 子进程 + PID 断言**的测试，`cargo fmt` / `cargo clippy --workspace --all-targets -- -D warnings` / `scripts/dev/check.sh` / `cargo test` 的绝大多数用例均通过。但存在 **1 个阻塞项**，无法给出通过：

1. **阻塞（合并门禁不可靠）**：`crates/zgraph-worker/tests/executor_m3.rs::crashed_worker_only_affects_its_own_instance` 断言并行分支 `b == Completed`，但内核冷峻失败语义（`fail_run()` → `cancel_active_instances()` 把在飞 `Running` 实例迁 `Cancelled` 并 abort 任务，`crates/zgraph-kernel/src/run.rs:1401-1422`、`1450-1482`）决定了 `b` 是否先于 `a` 完成**取决于两个子进程的竞速**。实测该用例高度不稳定：定向运行 24 次 **9 次失败**、`cargo test --workspace` 5 次运行 **3 次失败**（失败时 `executor_m3.rs:228` 报 `left: Some(Cancelled) / right: Some(Completed)`），`--test-threads=1` 下仍 10 跑 3 失败 → 是语义竞态而非测试相互干扰。merge.sh 第 6 步直接执行 `cargo test --workspace --quiet`（`scripts/dev/merge.sh:79`），随机失败会阻塞合并且违反 M3 DoD「`cargo test --workspace` 全绿」。

需 `worker-dev` 修复：重新设计该用例，使其在冷峻失败语义下**确定性**验证「崩溃不扩散」（建议让健康分支先完成，例如新增 `crash_after_delay` fixture：sleep ~300ms 后再抛错，`b=ok` 先 Completed；或改为断言 b 实例未因 a 的崩溃被标 Crashed、a 的失败码正确），修复后连续跑 ≥20 次（含 `--test-threads=1`）全绿，再由 reviewer 复审追加记录。

另有 2 个重要项（#2 误导性单测 + exit-3 零覆盖、#3 单帧读取无上限的 OOM 风险）需修复或登记 ISSUE；其余为建议/流程项，见 §3。**跨模块第二评审（kernel `context.rs`/`run.rs`、`Cargo.lock`）尚未由 reviewer-b 追加，本记录不在跨模块结论上签字。**

## 2. 四维评审

| 维度 | 结论 | 依据（文件/行/测试） |
|---|---|---|
| 功能正确性 | ⚠️ | ①进程组：`process_group(0)`（`supervisor.rs:202-203`）、`kill(-pgid)`（`:144-158`）、cancel→2s→TERM→1s→KILL（`:464-481`）、Drop 兜底 SIGKILL（`:178-182`）、显式 wait/reap（`:450-462`）——由 `supervisor_m3`（7 passed）与 `executor_m3` 的超时/取消用例（真实 PID 消失）验证；②内核集成：abort→drop 杀组并在 5s 内失败，`killed_workers==1`、`TimedOut` 迁移，`kernel_timeout_kills_worker_process_group_including_grandchild` 通过；③帧空闲超时方案 A：`run_frame_timeout()`（`:125-131`）+ 测试两条（`deadline_ms=0` 兜底 / `!=0` 豁免）；④错误映射矩阵与 `WorkerFailure::Timeout → NodeWorker{Protocol}`（`executor.rs:243-262`）符合「不得映射 NodeError::Timeout」；⑤流式条数/字节上限丢弃计数（`supervisor.rs:330-343`）+ 2 条测试；⑥**❌ flaky 用例见 §3 #1**；⑦单帧读取无上限（§3 #3）。 |
| 代码结构合理性 | ✅ | `protocol`（编解码）/`spawn`（M0 兼容面）/`supervisor`（生命周期）/`executor`（内核适配）职责清晰；M0 `run_worker`/`sample_request`/`WorkerConfig` 语义保留（`spawn.rs:106-156`，旧 M0 测试 8 passed 未改）；worker→kernel 单向依赖、kernel 不依赖 worker（kernel Cargo.toml 无反向边），`nix` 按 `[target.'cfg(unix)'.dependencies]` 引入（`crates/zgraph-worker/Cargo.toml:17-19`）；kernel 改动最小（context.rs 仅字段+2 访问器+单测，run.rs +2 行）。每 run 信号量表不回收已在 schema §7 显式登记为已知限制（建议再落台账）。 |
| 逻辑性 | ✅ | 分支/状态自洽：进程组信号失败忽略（进程可能已退出）、`wait_exit` 缓存退出状态避免二次 wait、`kill_and_reap` 对已 reap 进程跳过信号、成功路径 `pgid=None` disarm（`:376/:483-487`）避免 PID 复用误杀；错误路径统一由 `run_supervised`（`:566-595`）收口 kill/reap，无吞异常；信号量固定「先每 run 后全局」顺序，permit 为 RAII、随 future drop 释放（无泄漏），未发现死锁/无限循环。未发现生产代码级竞态（唯一竞态在测试断言，见 #1）。 |
| 可读性 | ⚠️ | 生产代码命名/类型/`为什么` 注释质量好（D1/D2/D4/D10、方案 A、drop 路径均有来源注释），函数均 ≤60 行，无 `unsafe`（`#![forbid(unsafe_code)]` 保留）；扣分项：`supervisor.rs:652-660` 单测名与内容不符（见 #2）、`take_stderr` 的 `Duration::from_secs(1)` 魔术数（#8）、`EMIT_TYPES` 定义未使用（#10）。 |

## 3. 发现清单

| # | 级别 | 位置 | 问题 | 建议 | 状态 |
|---|---|---|---|---|---|
| 1 | 阻塞 | `crates/zgraph-worker/tests/executor_m3.rs:214-229`（断言 `:228`） | `crashed_worker_only_affects_its_own_instance` 断言并行分支 `b == Completed`，与内核冷峻失败语义冲突：`fail_run()` 会把仍在 Running 的 b 迁 `Cancelled` 并 abort（`crates/zgraph-kernel/src/run.rs:1401-1422`、`1450-1482`），b 必须先于 a 的失败事件处理完成才可能 Completed → 子进程竞速。实测：定向 24 跑 9 失败、`cargo test --workspace` 5 跑 3 失败、`--test-threads=1` 10 跑 3 失败；失败输出 `left: Some(Cancelled) / right: Some(Completed)`。merge.sh 的 `cargo test --workspace --quiet` 会随机失败，门禁不可靠，且不满足 M3 DoD「全绿」 | **重设计用例**：让健康分支确定先完成（新增 fixture 如 `crash_after_delay`：`time.sleep(0.3)` 后抛错；`b=ok` 先 Completed），再断言 `a=Crashed`、`b=Completed`、run `Failed/NODE_BUSINESS_ERROR`；或改断言 b 未被标 Crashed 且在飞任务被内核 fail-fast 正常收尾（Cancelled），并注明显式语义。修复后连续 ≥20 次（含 `--test-threads=1`）全绿，复审追加记录 | open |
| 2 | 重要 | `crates/zgraph-worker/src/supervisor.rs:652-660` | 单测 `classify_exit_maps_sdk_self_check_to_protocol` **名不副实**：函数体只断言 `DEFAULT_MAX_EMIT_BYTES == MAX_FRAME_BYTES`，完全没调用 `classify_exit`；注释自述「仅验证分支选择逻辑的可读性」。导致 exit 3（SDK 自检失败）→ `NODE_WORKER_PROTOCOL` 这一协议 §3 封闭表分支**没有任何测试**（仓库内无 exit-3 fixture），属 CLAUDE.md 禁止的「为测试而测试」 | 用 `std::os::unix::process::ExitStatusExt::from_raw(3 << 8)`（`#[cfg(unix)]`）真实断言 `classify_exit(..) == protocol_failure`，并补 exit 2/0/信号的分支断言（或新增 exit-3 fixture 做集成用例）；常量等价断言移到独立用例或删除 | open |
| 3 | 重要 | `crates/zgraph-worker/src/supervisor.rs:527-541`（`read_frame_line`） | 单帧读取**无上限**：`read_until(b'\n', &mut buffer)` 先把整行读进内存，`decode_frame_line`/`Frame::from_line` 在**分配完成后**才判 8 MiB。不可信用户 Python 写一条超大无换行行即可让 supervisor/内核进程 OOM，与 M3「流式上限、禁止 OOM、崩溃隔离」目标相悖（M0 遗留、本分支未收紧；schema §6 以「帧解码层」表述覆盖，实际只覆盖解码校验） | 按 `MAX_FRAME_BYTES` 做有界读取（`fill_buf` 手工累积，超限立即返回 `NODE_WORKER_PROTOCOL` 并杀组），补 1 条超限用例；若本分支不修，请在 `doc/issues.md` 登记（重要项不可无台账） | open |
| 4 | 建议 | `schemas/worker-protocol-v1.md:80`、`:69` | 检查单两处「已覆盖」表述偏乐观：①「超 8 MiB（帧解码层）」只覆盖 `Frame::from_line` 单测，读路径仍无界（#3）；②「hello v 不匹配/缺字段 → 拒绝并 kill」只有解码层单测（`unsupported_version_is_rejected`/`hello_requires_all_fields`），无 supervisor 层 kill/PID 断言 | 改为「解码层已覆盖；读路径上限/集成 kill 见 ISSUE-xxx」，或补 supervisor 集成用例（期望 `NODE_WORKER_HANDSHAKE` + PID 消失） | open |
| 5 | 建议 | `crates/zgraph-worker/tests/supervisor_m3.rs:29-51` | `timeout_kills_process_group_including_grandchild` 用固定 800ms 总超时，却不与「孙进程 pid 已落盘」同步：负载高/启动慢时可能在孙进程写 pid 前就被 kill，随后 `read_pid(child)` 5s 超时失败（潜在 flake；本机 5 跑未复现） | 该用例无 elapsed 断言，建议超时放宽到 ≥2s；或让 fixture 先写孙进程 pid 再阻塞等待，测试用异步文件等待同步后再断言 timeout | open |
| 6 | 建议 | `crates/zgraph-worker/tests/executor_m3.rs:232-259`（`run_parallel`，`:268` sleep=0.5） | 并发上限用例依赖单机 wall-clock 区间与 0.5s sleep：`branches_run_concurrently_within_limits` 要求两分支执行区间重叠，重负载下进程启动偏差 >0.5s 时会偶发失败（余量偏小） | sleep 提到 1s，或对重叠判定加容差/重试；CI 上可考虑串行化该测试二进制 | open |
| 7 | 建议 | `crates/zgraph-worker/src/executor.rs:284-291` | `map_worker_error` 兜底 `_ =>` 会把 worker 自报的 `NODE_TIMEOUT`（协议 §5 列出但要求 worker 不自报）映射为 `BusinessError{code:"NODE_TIMEOUT"}`，语义含糊且会掩盖「worker 自报 Timeout」异常 | 显式 match `CODE_TIMEOUT` → `NodeWorker{Protocol}`（或 Internal）并注释；补 1 条单测 | open |
| 8 | 建议 | `crates/zgraph-worker/src/supervisor.rs:494` | `take_stderr` 的 `Duration::from_secs(1)` 是魔术数，与同文件其它超时（`REAP_TIMEOUT`/`TRAILING_READ_TIMEOUT`）风格不一致 | 提为具名常量 `STDERR_DRAIN_TIMEOUT` 并注释「进程已死，等待排空任务」 | open |
| 9 | 建议 | `crates/zgraph-worker/src/supervisor.rs:238-267`（`drive`，`pub`） | `Supervisor::drive` 作为公开低层 API 失败返回时本身不做 kill/reap，依赖调用方 `run_supervised` 收口；若外部直接 `drive()` 后忘调 `kill_and_reap`，Drop 可能对已 reap 的 pgid 再发 SIGKILL（理论 PID 复用误杀），M0 文档未约束 | 将 `drive` 收为 `pub(crate)`（公开面只有 `run_supervised`/`run_worker_controlled`），或在 doc 注释明确「失败必须由调用方 kill_and_reap」 | open |
| 10 | 建议 | `crates/zgraph-worker/src/executor.rs:160-172`、`protocol.rs:35-41` | ①`python_handler` 对非 python_node 返回 `Internal`（符合约定）但无单测；②`EMIT_TYPES` 定义后无任何引用（pub 常量 clippy 不报），白名单未生效于 `streaming_event` | `streaming_event` 复用 `EMIT_TYPES` 做白名单（或在注释说明仅供外部使用）；补 `python_handler` 错误分支单测 | open |
| 11 | 建议 | 测试覆盖缺口（`crates/zgraph-worker/tests/`） | 未断言：`RunRequest.params={}`、`attempt`、未到端口为 `null`（依赖 kernel `build_inputs`，worker 侧无回归保护）；`error` 帧 + exit 0 / exit 3 的退出约定分支；`global_vars` 空/非空边界；`map_worker_error(NODE_RETURN_NOT_DICT)` | 补 2–3 条集成/单测（尤其 `params`/`attempt`/null 端口，一次 fixture 全量回传即可） | open |
| 12 | 建议 | `crates/zgraph-worker/tests/executor_m3.rs:280-307` | `wait_for_file`/`wait_for_node_running` 与 `tests/common/mod.rs` 的 `read_pid`/`assert_pid_gone` 职责重叠，helper 分散；`common` 已 `#![allow(dead_code)]` | 收敛到 `common/mod.rs`（可后置，不影响正确性） | open |
| 13 | 备注 | `schemas/worker-protocol-v1.md:92-105` | §7 新增 4 条已知限制（supervisor streaming 丢弃未进内核指标 / per_run 信号量表不回收 / log 帧未落 trace / 成功路径不清理孙进程）+ 非 Unix 退化，均**未登记 `doc/issues.md`**（本分支按任务约束不得改台账）；ISSUE-025 状态仍为 `open` | 由 Lead 合并时：更新 ISSUE-025 为 fixed（注明本分支），并为上述限制新增 ISSUE（建议 ISSUE-061 起），非代码阻塞 | open |
| 14 | 备注 | `crates/zgraph-worker/src/spawn.rs:80-90` | `WorkerRun` 新增 `streaming` 字段（pub struct）：语义兼容、仓内无字面量构造方，但仓外按字面量构造会 breaking；`run_worker` 语义微调（error 帧须 exit 2 才映射业务失败、超时改为组 kill）均为协议 §3/D1 的正确修正，M0 既有测试未改且通过 | 若未来有仓外消费者，考虑 `#[non_exhaustive]`；本条仅提示，不需本分支处理 | open |

> 阻塞项 #1 必须清零才能给「通过」；重要项 #2/#3 需修复或登记 ISSUE；建议项可后置但需进台账（AGENTS §3/§7）。

## 4. 验证证据

### 4.1 范围与分支

```bash
cd /Users/zyzs/lczyzs/code/zgraph/.worktrees/worker-agent-a
git rev-parse --abbrev-ref HEAD        # feat/worker-m3-supervisor
git merge-base origin/main HEAD        # a6f4f5ce9434faade70934d44f26df6cfe38c671（= base，未 rebase）
git log --oneline -5                   # 0b6f016 / 104b596 / be3671b / 1325d64 / a6f4f5c
git diff --shortstat a6f4f5c..0b6f016  # 21 files changed, 2602 insertions(+), 234 deletions(-)
git log --format='%an <%ae> / %cn <%ce>' a6f4f5c..HEAD   # 4 commits 均为 chenyuxiangg <1007423165@qq.com>
git diff --name-only a6f4f5c..0b6f016   # 无 doc/progress.md、doc/issues.md、doc/reviews/**、crates/zgraph-kernel/src/{其他}.rs
```

每个 commit 正文均含 `Review: REV-012` / `Agent: worker-dev`；本分支未改 `doc/progress.md`/`doc/issues.md`/评审记录（符合任务约束）。

### 4.2 必跑命令（worktree 内，`CARGO_HOME=/Users/zyzs/lczyzs/code/zgraph/.cargo-home`）

| 命令 | 结果 |
|---|---|
| `cargo test --workspace` | **全量 274 个测试**。5 次运行：2 次全绿（274 passed/0 failed），**3 次 273 passed / 1 failed**，失败均为 `crashed_worker_only_affects_its_own_instance`（见 §4.3） |
| `cargo fmt --all -- --check` | exit 0 |
| `cargo clippy --workspace --all-targets -- -D warnings` | exit 0（无 warning） |
| `bash scripts/dev/check.sh` | `CHECK PASSED`（身份/远程/CLAUDE.md/禁用文件/分支命名/Python 语法/fmt/diff 全 OK） |
| `pytest`（`/Users/zyzs/lczyzs/code/zgraph/.venv/bin/python -m pytest -q`，附加） | 20 passed（Python 未被本分支改动，回归正常） |

### 4.3 阻塞项复现（关键证据）

```text
---- crashed_worker_only_affects_its_own_instance stdout ----
thread 'crashed_worker_only_affects_its_own_instance' panicked at crates/zgraph-worker/tests/executor_m3.rs:228:5:
assertion `left == right` failed
  left: Some(Cancelled)
 right: Some(Completed)
test result: FAILED. 13 passed; 1 failed; ...
```

| 复现方式 | 失败次数 |
|---|---|
| 单用例定向循环 6 次 | 2 |
| `cargo test -p zgraph-worker --test executor_m3` 整二进制 8 次 | 4 |
| 单用例 `--test-threads=1` 10 次 | 3 |
| `cargo test --workspace` 5 次 | 3（失败二进制均为 executor_m3 同一用例） |

根因（读代码确认，非猜测）：`crates/zgraph-kernel/src/run.rs:1401-1422` `fail_run()` 先 `cancel_active_instances()`，后者（`:1450-1482`）把仍 `Running` 的实例迁 `Cancelled` 并 `task.abort()`。a/b 两个分支各起独立 worker 子进程，b 只有在 a 的 business error 事件被事件循环处理**之前**完成才会保留 `Completed`；否则被冷峻失败收尾为 `Cancelled`。测试断言把时序当确定性，故必然偶发失败。

### 4.4 其余功能的独立核对（读代码 + 跑测试）

- **进程组/回收**：`Supervisor::spawn` `process_group(0)`（`supervisor.rs:202-203`）；`send_group_signal` 用 `nix::sys::signal::kill(-pgid)`（`:144-158`）；cancel 升级 `cancel→cancel_grace(2s)→SIGTERM→term_grace(1s)→SIGKILL→REAP_TIMEOUT(2s)`（`:464-481`）；`Drop` 对 armed pgid 补 SIGKILL（`:178-182`）；drop 路径的 zombie 由 tokio 信号驱动 orphan reaper 异步回收（`tokio-1.54.1/src/process/unix/reap.rs:127-128`），测试以 `kill -0` 断言 PID 消失（`supervisor_m3` 7 passed，5 连跑稳定）。`kill_on_drop(true)` 与组 kill 并存安全：组信号先发，Child drop 只补杀子 PID，已 reap 后 `start_kill` 不触发。
- **AC-06 集成**：`executor_m3::kernel_timeout_kills_worker_process_group_including_grandchild` 实测 `NODE_TIMEOUT`、`elapsed < 5s`、`metrics.killed_workers == 1`、`work` 实例 `TimedOut`、worker 与孙进程 PID 均消失；`kernel_cancel_kills_worker_process_group` 同理（run `Cancelled`）。
- **帧空闲超时方案 A**：`run_frame_timeout()` 仅在 `deadline_ms == 0` 时返回 `frame_timeout`（`supervisor.rs:125-131`），超时分类为 `protocol_failure`（`:510-524`）→ `NodeWorker{Protocol}`；`SupervisorConfig` 默认 300s/2s/1s 与协议 §4 一致；`deadline_ms != 0` 由内核 deadline 权威判定。`executor.rs:243-262` 明确把兜底 `WorkerFailure::Timeout` 映射为 Protocol 并有单测 `supervisor_timeout_is_not_mapped_to_timeout_error`，未违反 04 §4.1.4。
- **M0 兼容**：`run_worker`/`sample_request`/`WorkerConfig` 字段与语义保留（`spawn.rs`），旧 `echo_roundtrip.rs` 8 passed（含 timeout/hello timeout 的 PID 回收、duplicate result、exit-0 无 result、非法 UTF-8、stderr 截断）；行为变化仅 error 帧需 exit 2、超时改进程组 kill——均为协议 §3/D1 的正确修正。
- **WorkerExecutor**：错误映射矩阵由单测（`executor.rs:309-476`，含 BusinessError 原码保留/PythonException traceback/Cancelled/Timeout→Protocol/Spawn→Config/Exit→Crash）与集成用例（`python_exception`、`sdk_business_error`、`custom_business_code`、`import_failure`、`exit_without_result`、`signal_exit`）双重覆盖；emit 5 类事件按序入 `TraceSink::streaming`；`run` 载荷实测 seed=7/user_id/global_vars/deadline/node_id 透传；信号量为 RAII permit，固定「先每 run 后全局」获取顺序，drop 即释放。
- **kernel 授权最小性**：`context.rs` diff 仅 `seed` 字段 + `seed()`/`global_vars()` + 单测；`run.rs` 仅 +2 行传 seed；`NodeContextInit` 仅 `run.rs` 一处构造（编译保证）；kernel 无反向依赖 worker；`nix` 仅在 `[target.'cfg(unix)'.dependencies]` 下（`crates/zgraph-worker/Cargo.toml:17-19`）。**最终签字待 reviewer-b。**

## 5. 遗留问题

- **阻塞**：#1 需 `worker-dev` 重设计 flaky 用例并复跑 ≥20 次全绿；修复后由 reviewer 复审并在本记录追加「条件关闭/复审通过」段。
- **重要**：#2（误导性单测 + exit-3 零覆盖）与 #3（单帧读取无上限/OOM）需修复或登记 `doc/issues.md`。
- **流程**：ISSUE-025 状态与本分支新增已知限制（schema §7 的 4 条）需 Lead 合并时更新/登记新 ISSUE；`doc/progress.md` 由 Lead 合并时更新。非代码阻塞。
- **未在 CI 覆盖**：非 Unix 退化路径（`send_group_signal` stub + `start_kill`）仅代码审查，未做 Windows 交叉编译/运行验证；schema §7 已如实标注。
- 本评审**未修改任何代码/台账**，只新增本记录并提交；未 merge/push/rebase。

### 5.1 跨模块授权与第二评审

- 跨模块改动（kernel `context.rs`/`run.rs`、`Cargo.lock`）主评审初步核对：改动最小、无行为变化、依赖方向正确（详见 §4.4 与 §2），**但按 Task 约定由 `reviewer-b` 做第二评审**。
- **跨模块第二评审：待 reviewer-b 追加**（请追加：kernel 接口暴露面是否最小、`seed`/`global_vars` 语义与 spec 04 §4.2 一致性、Cargo.lock/依赖边与 process 边界合规性、schemas 口径与实现一致性）。

## 6. 条件关闭记录

不适用：本记录结论为 **不通过**，阻塞项关闭后需复审并追加本段（而非按「有条件通过」条件表处理）。

## 7. 复审（2026-10-30，reviewer-a，HEAD `44131f3`）

> 作者 `worker-dev` 按 §3 修复后提交 `4e48c3f`（生产代码 #2/#3/#7–#10）、`e9d4b96`（测试 #1/#4–#6/#11）、`44131f3`（schema #4）。本节为独立复审：**逐条读实际代码/跑真实测试，不采信修复自述**；保留原「不通过」历史不删除。基础信息：分支 `feat/worker-m3-supervisor`，base=merge-base=`a6f4f5c`（**未 rebase**；`origin/main` 已前进到 `33d55e8`，合并时由 merge.sh 自动 rebase），worktree clean，3 个新 commit 均 `chenyuxiangg <1007423165@qq.com>` 且正文含 `Review: REV-012` / `Agent: worker-dev`；新 commit 未触碰 `doc/progress.md`/`doc/issues.md`/`doc/reviews/**`（本记录除外）。

**复审结论：通过**

阻塞项 #1 已消除且经高重复稳定性验证（定向 20/20、`--test-threads=1` 10/10、`cargo test --workspace` 5/5 全绿）；重要项 #2/#3 修复实现正确、测试为真实断言；#4–#12 逐项核对通过（#7/#10/#11 的**部分自述测试未实际添加**，见 §7.3 #15，建议级、不阻塞）。跨模块第二评审仍待 reviewer-b 追加（§5.1），本节不对跨模块部分签字。

### 7.1 逐项关闭核对（#1–#12）

| # | 原级别 | 修复落点（当前文件:行） | 独立核对结果 | 状态 |
|---|---|---|---|---|
| 1 | 阻塞 | `tests/executor_m3.rs:235-260`；`tests/fixtures/py/wfaults.py` 新增 `ok_with_marker`/`crash_after_marker` | **竞态确已消除**：b 写 marker 后返回；a 等 marker 出现再 `sleep(0.5)` 后抛 `ValueError`。b 的 `result→exit→Completed` 链路（正常 <10ms）必然先于 a 延迟 0.5s 的失败进入事件队列；内核 `fail_run` 只收尾非终态实例，故 b 保持 Completed。测试断言 a=Crashed、b=Completed、marker 存在，语义与「冷峻失败 + 进程隔离」自洽。稳定性：定向 20/20、单线程 10/10、workspace 5/5（此前约 40% 失败率）。唯一残余风险是极端 CPU 饥饿下 0.5s 余量（题设已远优于原设计），可接受 | fixed |
| 2 | 重要 | `src/supervisor.rs:679-716`（真实 ExitStatus 单测）、`:717-723`（常量断言独立） | 伪测试已删除；新测用 `ExitStatusExt::from_raw` 真断言 exit 3→`Protocol(NODE_WORKER_PROTOCOL)`、exit 2/0→`Exit{Some(code)}`、信号（raw=9）→`Exit{code:None}`；`--lib` 实跑 21 passed 含该用例。exit-3 分支不再是零覆盖 | fixed |
| 3 | 重要 | `src/supervisor.rs:530-571`（`fill_buf` 有界读取）；`tests/supervisor_m3.rs:229-258` + `fixtures/workers/oversized_frame_worker.py` | 逐行核对边界：`max_buffered = MAX_FRAME_BYTES + 1`（body 8 MiB + 换行），`buffer.len()+take > max_buffered` 才报错 → **恰好 8 MiB 的合法帧不误伤**，8 MiB+1 body 及以上拒绝；分块 `fill_buf→extend→consume`，含「chunk 内含多行时只消费首行、余量留在 BufReader」与「EOF 半行→协议错误」「空 EOF→None」三种分支，行为与旧解码路径一致；`read_frame_line` 只在 cancel/帧超时（均为终结路径）被 drop，不存在丢帧重试风险。实测超 8 MiB 无换行 fixture → `NODE_WORKER_PROTOCOL` + 杀组 + PID 消失；正常帧回归（`echo_roundtrip` 8 passed、emit 上限/洪泛用例、全量 291 passed）未见误伤，性能无感（普通帧 1 次 fill_buf） | fixed |
| 4 | 建议 | `schemas/worker-protocol-v1.md:61-63`（§4 新增「有界帧读取」）、`:74`/`:86`（§6 检查单措辞） | 检查单不再把「超 8 MiB」只归解码层；hello v 不匹配/缺字段已补 supervisor 集成用例名；口径与实现一致 | fixed |
| 5 | 建议 | `tests/supervisor_m3.rs:30-35`（孙进程超时 800ms→3s）；`tests/executor_m3.rs:167`（内核超时 500ms→1.5s） | 与「pid 落盘」解耦，余量充足；两用例均无精确 elapsed 断言，放宽不影响语义；实跑通过 | fixed |
| 6 | 建议 | `tests/executor_m3.rs:299`（并发 sleep 0.5→0.8） | 重叠/串行判定余量增大；3 条并发用例在 5 次 workspace 全量中稳定通过 | fixed |
| 7 | 建议 | `src/executor.rs:288-297`（`CODE_TIMEOUT` 显式→`NodeWorker{Protocol}`） | 代码正确（不再落入 BusinessError 兜底），符合 04 §4.1.4；**但 §7.3 #15：自述「补单测」未实际落地**，当前无测试直接覆盖该分支 | 部分（代码 fixed，测试缺） |
| 8 | 建议 | `src/supervisor.rs:68`（`STDERR_DRAIN_TIMEOUT`）、`:498` 使用 | 魔术数已提为具名常量并注释 | fixed |
| 9 | 建议 | `src/supervisor.rs:242`（`drive` 改 `pub(crate)`） | 公开面收敛；仓内仅 `run_supervised` 调用（grep 无外部 `.drive(`），无破坏 | fixed |
| 10 | 建议 | `src/executor.rs:214-217`（`EMIT_TYPES` 白名单） | 未知 emit type 在映射前被白名单拒绝，不再进入 match 兜底；既有 `unknown_emit_types_are_dropped_not_fatal` 仍绿；**但 §7.3 #15：自述「补 python_handler 非 python_node 单测」未落地**（该防御分支仍无测试） | 部分（代码 fixed，测试缺） |
| 11 | 建议 | `tests/executor_m3.rs:128-150`（attempt/params）、`:152-166` + `common::linear_graph_with_optional_port` + `wfaults.echo_inputs`（未接线端口=null） | **本次复审认为该覆盖有效**：`attempt==0`、`params=={}`、`inputs.spare is null`、`inputs.x == {"x":{"v":1}}` 均为真实 Python 回传断言；全量运行稳定通过。自述另称「NODE_TIMEOUT 自报 / NODE_RETURN_NOT_DICT 映射」测试**未落地**（见 #15），`NODE_RETURN_NOT_DICT` 仍只由 `_ =>` 兜底隐式覆盖 | 部分（2/4 落地，均有效） |
| 12 | 建议 | `tests/common/mod.rs:169-199`（`wait_for_file`/`wait_for_node_running` 收敛）；`tests/executor_m3.rs` 改用 `common::*` | 重复 helper 已删除，职责收敛；`common` 的 `#![allow(dead_code)]` 仍在（各测试二进制按需使用，合理） | fixed |

补充：原 #13（ISSUE-025 状态 + schema §7 已知限制未入台账）、#14（`WorkerRun.streaming` 为 pub struct 增字段）本次仍**未变**——#13 属 Lead 合并时台账动作（本分支按任务约束不得改 `doc/issues.md`），#14 为后置提示，二者均非代码阻塞。

### 7.2 亲跑稳定性与命令结果（worktree 自带 target，未设 `CARGO_TARGET_DIR`，`CARGO_HOME=/Users/zyzs/lczyzs/code/zgraph/.cargo-home`）

| 命令 | 结果 |
|---|---|
| 定向 flaky 用例 ×20：`cargo test -q -p zgraph-worker --test executor_m3 -- crashed_worker_only_affects_its_own_instance` | **20 passed / 0 failed** |
| 同上 `-- --test-threads=1` ×10 | **10 passed / 0 failed** |
| `cargo test --workspace` ×5 | **5/5 PASS，每次 291 passed / 0 failed**（每次全量 45 个测试二进制） |
| `cargo test -p zgraph-worker --lib --test supervisor_m3 --test executor_m3` | 21 + 11 + 15 passed / 0 failed；新增用例逐个确认在列（`oversized_frame_without_newline_is_protocol_error_and_killed`、`hello_version_mismatch…`、`hello_missing_sdk_version…`、`error_frame_with_exit_zero_is_protocol_error`、`dispatch_inputs_include_every_declared_port_with_null_for_missing`、`crashed_worker_only_affects_its_own_instance`、`classify_exit_maps_exit_codes_and_signals`） |
| `cargo fmt --all -- --check` | exit 0 |
| `cargo clippy --workspace --all-targets -- -D warnings` | exit 0（无 warning） |
| `bash scripts/dev/check.sh` | `CHECK PASSED`（14 项 OK） |
| 范围/洁净度 | `git status --short` 空；base=`a6f4f5c`；`origin/main`（`33d55e8`）**不是** HEAD 祖先 → 未 rebase（符合要求）；新 commit 无 `doc/` 改动（本记录除外） |

稳定性对照：原阻塞项在修复前定向 24 跑 9 失败、全量 5 跑 3 失败；修复后上述 35 次定向/单线程运行 + 5 次全量 **零失败**。

### 7.3 复审新增发现

| # | 级别 | 位置 | 问题 | 建议 | 状态 |
|---|---|---|---|---|---|
| 15 | 建议 | commit `4e48c3f` 正文（#7/#10「补单测」）、`e9d4b96` 正文（#11「NODE_TIMEOUT 自报/NODE_RETURN_NOT_DICT 映射」） | **自述与仓库不符**：`CODE_TIMEOUT` 映射分支、`python_handler` 非 python_node 防御分支、worker 自报 `NODE_TIMEOUT`/`NODE_RETURN_NOT_DICT` 映射均无对应测试（grep 全仓无新增用例）。代码本身经人工核对正确，但「已补单测」的陈述不可核验，影响修复自述可信度 | 补 3 条小单测（`map_worker_error(CODE_TIMEOUT)`→Protocol、`map_worker_error(CODE_RETURN_NOT_DICT)`→BusinessError 原码、`python_handler` 非 python_node→Internal），或更正提交自述并在台账说明；不阻塞合并（无运行时缺陷） | open |
| 16 | 备注 | 本记录 §4.2 | 前次「全量 274 个测试」是**失败运行**的计数：cargo test 默认 fail-fast，`executor_m3` 失败后其余测试二进制未执行（45 个中仅跑 38 个）。实际全量为 **291**（本次 5 次全绿均 291 passed / 0 failed） | 以本次 291 为准；原结论（flaky 阻塞）不受影响 | fixed（更正） |

### 7.4 结论与可合并性

- **复审结论：通过**：§3 的阻塞项 #1 已清零（root cause 修复 + 高重复稳定性证据）；重要项 #2/#3 已修复并有真实测试；建议项 #4–#12 除 #7/#10/#11 的少量自述测试缺口（#15，建议级、不阻塞）外均已关闭。
- **可合并性**：满足 merge.sh 前置（评审记录含「通过」结论、worktree clean、全量测试全绿）；`merge.sh` 会自动 rebase 到最新 `origin/main`（`33d55e8`）并重跑 `cargo test --workspace`——本机 5 连绿，rebase 后若出现冲突/失败由合并方按脚本处理。
- **跨模块第二评审（kernel `context.rs`/`run.rs`、`Cargo.lock`）仍待 reviewer-b 追加 §5.1**；本复审未改变跨模块改动范围与内容（`git diff 6199ce1..44131f3` 无 kernel/Cargo 改动），主评审初步核对结论（最小性/依赖方向）继续有效。
