# REV-031 · kernel/worker/cli/harness · M5 ISSUE-066 `NODE_WORKER_CRASH` 记录 signal/exit

> **模块声明（跨模块，Lead 已批准）**：本分支跨 4 个模块落地 ISSUE-066——
> - `kernel`：`crates/zgraph-kernel/src/types.rs`、`tests/failure.rs`（`NodeError::NodeWorker` 只增证据字段）；
> - `worker`：`src/{executor,lib,spawn,supervisor}.rs`、`tests/{executor_m3,supervisor_m3}.rs`、fixtures（`tests/fixtures/py/wfaults.py`、新增 `tests/fixtures/workers/{exit_code,signal_exit}_worker.py`）；
> - `cli`：`crates/zgraph-cli/src/bridge.rs`（`node_error_json` 仅 Some 输出新键）；
> - `harness`：`crates/zgraph-harness/src/tools.rs`（**8 行编译必需机械修正**：`WorkerFailure::Exit` 新字段解构 + 诊断串补 `signal`）。
>
> 依据 DR-002 §4（`doc/decisions/DR-002...:79`）「三模块同分支顺序 = kernel → worker → cli；Lead 批准跨模块，评审记录须声明第二评审」与 §6 波次表（`:104`「REV-031（双评审人）」）。本记录为 **reviewer-a 主审**，Lead 将请 **reviewer-b 追加第二评审段**；评审**未修改分支任何文件**，只新增本记录。分支已由 Lead rebase 到最新 main（`REV-032` 合入后），**worker 提交 diff 与首轮对象逐字节一致**（patch SHA-256 `fb809068e7ee067b386b43057b19c03285c2ecce7d4c37e9755674b75f2bf26f`，872 行，见 §4.1）。
>
> 评审对象（最终）：`58239fc..22f4395`（1 commit `22f4395 fix(worker): ISSUE-066 NODE_WORKER_CRASH 记录 signal/exit（REV-031）`；原对象 `64f110a..7147820`）。base = merge-base = `58239fc` = 评审时 `main`。

- **分支**：`fix/worker-m5-crash-detail`
- **作者**：`worker-dev`（subagent）
- **评审人**：`reviewer-a`（**非作者**；第二评审 `reviewer-b` 待追加）
- **变更范围**：`58239fc..22f4395`（13 files changed, 536 insertions(+), 60 deletions(-)；原 `64f110a..7147820`，rebase 仅变基）
- **评审时间**：2026-10-30
- **关联**：ISSUE-066（P2 worker，`doc/issues.md:74`）；DR-002 §4/§6（ISSUE-066 口径与波次）；spec 03 §3.15（`03:382`「记录 signal/exit」）、§3.9.4（spawn 失败 → `NODE_WORKER_CRASH`）；architecture §8.2（`arch:273`「记录 signal/exit」）；spec 06 §6.2.5.1（`06:176-196`，`NodeError` JSON canonical 形状——**本分支与其闭集约束冲突，docs 第二批同步**）；REV-026/REV-030（hint/标定无关，仅上下文）

## 1. 结论

**结论：通过**

ISSUE-066 按 architecture §8.2 / spec 03 §3.15 的「`NODE_WORKER_CRASH` 记录 signal/exit」在三模块落地，harness 仅做编译必需的最小适配：

- **kernel**（`types.rs:442-449`）：`NodeError::NodeWorker` **只增** `signal: Option<i32>`/`exit_code: Option<i32>`/`stderr_tail: Option<String>`；`code()` 仍是 `kind.code()`（4 个 worker 码不变，`types.rs:465`），`Display` 仍为 `worker error ({kind:?})`（单测 `node_worker_crash_payload_keeps_code_and_display`/`worker_error_codes_are_stable_with_crash_details` 断言）；非崩溃路径字段恒 `None`。
- **worker**：`WorkerFailure::Exit` 增 `signal`（`spawn.rs:79-90`）；`Supervisor::classify_exit` 用 `ExitStatusExt::signal()` 提取 Unix 信号（`supervisor.rs:411-421,524-532`，非 Unix 恒 `None`），`attach_stderr` 保留 signal；`map_failure`（`executor.rs:247-273`）只在 `Exit` 崩溃路径填充 `signal/exit_code/stderr_tail`，`Protocol/Timeout/Spawn/Worker(error)` 等路径保持 `None`；`stderr_tail` 取 supervisor 已保留的**末尾** 8 KiB 中最后 **1 KiB**、按 UTF-8 字符边界截断（`executor.rs:285-301`），空白 stderr → `None`。
- **cli**：`node_error_json` 仅当字段 `Some` 时输出 `signal`/`exit_code`/`stderr_tail`（`bridge.rs:580-598`），既有 `code/message/kind/worker_kind` 键集合与值不变（测试 `node_error_json_expands_node_worker_crash_details`）。
- **harness**（`tools.rs:1376-1383`）：仅把 `WorkerFailure::Exit` 新字段解构并把 `signal` 补进诊断串（编译必需，8 行；无逻辑扩张）。
- **真实子进程证据**（非 mock `ExitStatus`）：`exit_text` 见 §4.3——真实 worker 子进程 `exit 7` → `exit_code=Some(7)`+stderr 证据；`SIGKILL` → `signal=Some(9)`+`exit_code=None`；`exit 3` 仍按协议 §3 映射 `NODE_WORKER_PROTOCOL`；`map_failure` 边界有定向单测。
- **门禁**：定向 crate 测试（kernel/worker/cli/harness，含真实子进程）**404 passed / 0 failed / 1 ignored**（rebased 分支复跑；`/tmp/rev031-targeted-rebased.log`）；clippy `-D warnings`/fmt/`check.sh` PASS；无 `Cargo.lock`/docs 改动；改动源码函数无 >60 行新增（唯一 >60 行为既有 `harness::tools::validate_schema_node`，非本分支引入）。workspace 不缩水见 §4.5（最终 dry-run）。

未发现阻塞/重要项；1 条 docs 遗留（§3 #1，Lead 已预告随 docs 第二批同步）+ 1 条敏感信息备注（#2）+ 5 条备注/Nit，均不阻塞本分支。

## 2. 四维评审

| 维度 | 结论 | 依据（文件/行/测试） |
|---|---|---|
| 功能正确性 | ✅ | ① exit 7 真实子进程：`supervisor_m3.rs:349`（`Exit{code:Some(7),signal:None}`）+ `executor_m3.rs:116`（经真 `WorkerExecutor`+Python 得 `NodeError::NodeWorker{signal:None,exit_code:Some(7),stderr_tail:Some(...)}`，stderr 含 fixture 标记且 ≤1 KiB）；② SIGKILL 真实子进程：`supervisor_m3.rs:372`（`signal=Some(9)`）+ `executor_m3.rs:100`；③ exit 3 仍协议：`supervisor_m3.rs:393`（`WorkerFailure::Protocol(CODE_PROTOCOL)`，message 含 code 3）；④ map 边界：`executor.rs` 单测覆盖 signal/exit/空 stderr/字符边界/无 signal 仍 Crash；⑤ 错误码/Display 不变（kernel 单测 + failure.rs 4 码表）；⑥ CLI JSON 仅 Some 输出、非崩溃无新键。 |
| 代码结构合理性 | ✅ | 分模块职责清晰：kernel 定义载荷、worker 采集证据、cli 序列化、harness 机械适配；`node_worker_error(kind)` 辅助收敛 4 处重复构造；`exit_signal` 用 `cfg(unix)` 隔离平台差异；`STDERR_TAIL_BYTES` 常量公开供下游断言；字段命名/文档一致。跨模块改动最小：harness 仅 8 行、cli 仅一个 match 分支 + 测试；无 Cargo/docs/其它模块改动。 |
| 逻辑性 | ✅ | 证据只在「进程退出」路径产生：`classify_exit` 先判 code 3 → `Protocol`（不产生 Exit），其余非 0/信号 → `Exit{code,signal}`；`attach_stderr` 保留 signal；`map_failure` 对 `Exit` 才填证据、其它置 `None`；`spawn` 失败明确无子进程证据（`None`，注释说明）。`stderr_tail` 取末尾（supervisor `drain_stderr` 已只保留最后 8 KiB，`supervisor.rs:594-610`），方向正确；截断向字符边界回退，绝不产生非法 UTF-8；空/空白 → `None`。 |
| 可读性 | ✅ | 三处枚举/字段均有「ISSUE-066/architecture §8.2」注释解释「为什么只增/为什么某些路径 None」；fixtures 文档说明用途与等价 shell 行为；测试命名直述断言（`..._records_signal_and_stderr_tail`、`..._records_exit_code_and_omits_empty_stderr`、`..._truncated_on_char_boundary`）。Nit：`WorkerFailure::Exit` 的 `Display` 由 `code=…,stderr=…` 扩为含 `signal=…`（诊断串变化，见 #4）。 |

## 3. 发现清单

| # | 级别 | 位置 | 问题 | 建议 | 状态 |
|---|---|---|---|---|---|
| 1 | 建议（docs 遗留，Lead 已知） | `doc/spec-v1.2/06-存储与可观测.md:176-196`（§6.2.5.1）；本分支 `cli/src/bridge.rs:580-598` | spec §6.2.5.1 冻结的 `NodeWorker` canonical JSON **只有 `worker_kind`**，并明写「**不得**新增未列出的顶层键」；本分支在崩溃路径新增 `signal`/`exit_code`/`stderr_tail` 三个顶层键 → 与冻结契约冲突（这也是 ISSUE-066 要求「同步 spec 06」的原因） | docs 第二批（`docs/docs-m5-writeback`，REV-035）更新 §6.2.5.1 表（NodeWorker 增 3 可选字段+语义/上限）并修订「不得新增」约束（或按 spec 版本规则登记），同时关闭 ISSUE-066；本评审记录明确遗留，不阻塞（Lead 已预告） | open（docs REV-035） |
| 2 | 备注（安全） | `worker/src/executor.rs:285-301`；`cli/src/bridge.rs:590-596` | `stderr_tail` 可能含用户 stderr 的敏感内容（异常栈、环境变量打印、业务数据），会落 `instances.error/transitions.error` JSON 与 `--json` 输出；worker 为本机子进程（无跨机传输），风险与既有 `PythonException.traceback`（全文不截断）同源 | 文档/决策记录注明「崩溃 stderr 尾部属排障证据、不做脱敏，1 KiB 上限」；无需本分支改动（与 Lead 口径一致） | open（docs 注记） |
| 3 | 备注 | `worker/src/executor.rs:247-273`（`map_worker_error` 的 `CODE_CRASH` 分支） | worker **自报** `NODE_WORKER_CRASH`（`error` 帧）会映射 `NodeWorker{Crash}` 但无进程退出证据（三字段 `None`）；这是正确区分（无 signal/exit 可选），但文档宜写明「仅 supervisor 观察到的进程退出路径有证据」 | docs/REV-035 的 §6.2.5.1 注记中补一句；可顺带加 1 条「self-reported crash → 三字段 None」单测（可选） | open（docs/Nit） |
| 4 | Nit | `worker/src/spawn.rs:79-90`（`WorkerFailure` Display）；`harness/src/tools.rs:1376-1383` | `WorkerFailure::Exit` 的 `Display` 追加 `signal=…`（诊断串变化），harness `tool_worker_crash` 的 message 随之多出 `signal=None/Some(..)`；`NodeError` 的 `code()`/`Display` 经测试确认**未变** | 可接受（属诊断信息增强）；无下游精确字符串断言（已 grep）；无需动作，记录备案 | 无需处理 |
| 5 | 备注 | `worker/src/supervisor.rs:594-610`、`executor.rs:289-301` | `stderr_tail` 取的是 supervisor 保留的最后 8 KiB 的**最后 1 KiB**（`drain_stderr` 只留末尾，方向正确）；若 stderr 总量 >8 KiB，更早内容本就丢弃（既有契约），本分支不改变 | 无需动作；文档可注明「证据窗口 = 最后 8 KiB 的子窗口 1 KiB」 | 无需处理 |
| 6 | 备注（跨模块） | 全 diff | 跨 4 模块 13 文件（清单见文首）：kernel 载荷、worker 证据、cli 序列化、harness 编译必需机械修正；**无** `Cargo.toml/Cargo.lock`/docs/其它 crate 改动；harness 改动 8 行为最小必要 | Lead 已在 DR-002 §4 批准跨模块并要求双评审；本记录已声明，待 reviewer-b 追加第二评审段 | 待第二评审 |
| 7 | 备注 | `worker/tests/executor_m3.rs:100,116`、`supervisor_m3.rs:349,372,393`、fixtures | 测试真实性：均为**真实子进程**（真 `WorkerExecutor`/`Python`；`kill -9`、`exit 7`、`exit 3` 由 fixture 进程真实产生，非 mock `ExitStatus`）；map 层另有 5 条纯函数单测锁定边界与 UTF-8 截断 | 无需动作 | 无需处理 |
| 8 | 备注（合并） | 分支 base `58239fc` = 评审时 main | 评审时分支可 ff；若合并前 main 再前进（如 REV-033/035 合并），需按流程 rebase 后复跑门禁 | Lead 合并前确认；dry-run 不校验 ff | open（合并前置） |

> 无阻塞项、无重要项；#1 为 docs 第二批必须同步的冻结契约冲突（Lead 已预告），#2/#3 为文档注记建议，#4/#5/#7 为备案，/#8 合并前置。

## 4. 验证证据

### 4.1 范围、rebase 与提交元数据

```bash
cd /Users/zyzs/lczyzs/code/zgraph/.worktrees/worker-worker-dev
git status --short                         # 空（评审时 clean）
git branch --show-current                  # fix/worker-m5-crash-detail
git merge-base main HEAD                   # 58239fc（= main，rebase 后基于最新 main）
git rev-parse HEAD                         # 22f4395...
git log --oneline main..HEAD               # 22f4395（1 commit）
git diff --shortstat main...HEAD           # 13 files changed, 536 insertions(+), 60 deletions(-)
git log -1 --format='%H %an <%ae> %s'      # chenyuxiangg；正文含 Review: REV-031 / Agent: worker-dev
# rebase 内容一致性（原 sha → 新 sha，仅变基）：
git diff 64f110a 7147820 > /tmp/rev031-old.patch
git diff 58239fc 22f4395 > /tmp/rev031-new.patch
shasum -a 256 /tmp/rev031-old.patch /tmp/rev031-new.patch
# 两文件同为 fb809068e7ee067b386b43057b19c03285c2ecce7d4c37e9755674b75f2bf26f（872 行）→ 内容逐字节一致
```

### 4.2 kernel：只增字段、错误码/Display 不回归

| 项 | 证据 |
|---|---|
| `NodeWorker` 新字段只增 | `types.rs:442-449`（`signal/exit_code/stderr_tail` 全 `Option`，文档说明「仅崩溃路径且有证据时 Some」） |
| 4 个 worker 码不变 | `types.rs:465` 仍 `kind.code()`；单测 `worker_error_codes_are_stable_with_crash_details`（Handshake/Protocol/Import/Crash 四码） |
| `Display` 不变 | `#[error("worker error ({kind:?})")]`；单测 `node_worker_crash_payload_keeps_code_and_display` 断言 `"worker error (Crash)"` |
| 既有构造点 | `tests/failure.rs:265-300` 更新为三字段 `None`；workspace 编译通过 |

### 4.3 worker：真实子进程证据 / map 边界 / stderr 尾部

| 场景 | 真实链路证据 | 断言 |
|---|---|---|
| exit 7（非约定退出码 + stderr） | `supervisor_m3.rs:349`（fixture `exit_code_worker.py` 真进程）；`executor_m3.rs:116`（`wfaults.exit_seven` 经真 `WorkerExecutor`） | `WorkerFailure::Exit{code:Some(7),signal:None}`；`NodeError::NodeWorker{Crash, exit_code=Some(7), stderr_tail=Some(tail)}`，`tail.contains("exit-seven fixture")` 且 `tail.len()≤STDERR_TAIL_BYTES` |
| SIGKILL | `supervisor_m3.rs:372`（fixture `signal_exit_worker.py` 真 `os.kill(SIGKILL)`）；`executor_m3.rs:100`（`wfaults.kill_self`） | `Exit{code:None,signal:Some(9)}`；`NodeError::NodeWorker{Crash, signal=Some(9), exit_code=None}` |
| exit 3（SDK 自检） | `supervisor_m3.rs:393` | 仍 `WorkerFailure::Protocol(CODE_PROTOCOL)`，message 含 `code 3`（不是 Crash） |
| map 边界（纯函数） | `executor.rs` 单测：`crash_exit_records_signal_and_stderr_tail`、`crash_exit_records_exit_code_and_omits_empty_stderr`、`stderr_tail_is_truncated_on_char_boundary`（`"é"×1024` → 尾部 ≤1024B 且全为合法字符）、`crash_exit_without_signal_maps_to_crash_kind`、`spawn_failure_maps_to_worker_crash`、Timeout/Protocol 映射 | 覆盖 signal/exit/空 stderr/字符边界/无证据仍 Crash |
| stderr 方向 | `supervisor.rs:594-610` `drain_stderr` 只保留最后 `max_stderr_bytes`（默认 8 KiB）；`stderr_tail` 再取最后 1 KiB | 方向正确（tail-of-tail）；空白 → `None` |

### 4.4 cli / harness

- `cli/src/bridge.rs:580-598`：`NodeWorker` JSON 既有键（`code/message/kind/worker_kind`）不变；`signal/exit_code/stderr_tail` 仅 `Some` 时插入；测试 `node_error_json_expands_node_worker_crash_details` 覆盖 signal-only / exit-only / 非崩溃三态（字段省略而非 null）。
- `harness/src/tools.rs:1376-1383`：仅解构新字段并把 `signal` 拼入 `tool_worker_crash` 诊断串；无逻辑分支变化（编译必需，最小 8 行）。

### 4.5 命令与结果（隔离 target `/tmp/rev031-target`）

| 命令 | 结果 |
|---|---|
| `cargo test -p zgraph-kernel -p zgraph-worker -p zgraph-cli -p zgraph-harness -- --test-threads=2`（**rebase 后**复跑，含真实子进程） | **404 passed / 0 failed / 1 ignored**（ignored = harness 真模型 smoke；`/tmp/rev031-targeted-rebased.log`） |
| `cargo test --workspace -- --test-threads=2`（隔离 target，rebase 前内容逐字节一致的树） | **539 passed / 0 failed / 2 ignored**（2 ignored = harness 真模型 + e2e AC-08b 手动；`/tmp/rev031-workspace.log`） |
| `cargo clippy --workspace --all-targets -- -D warnings` | exit 0 |
| `cargo fmt --all -- --check` | PASS |
| `bash scripts/dev/check.sh` | CHECK PASSED（身份/冻结文件/禁用文件/progress ≤30/fmt/diff-check） |
| 函数长度（改动源码） | 新增/改动函数均 ≤60 行；唯一 >60 行为既有 `harness::tools::validate_schema_node`（105，本分支不涉及，ISSUE-081 已登记） |
| `Cargo.lock`/`doc/**` | 无改动（name-status 13 文件全部为 kernel/worker/cli/harness 源码/测试/fixtures） |

### 4.6 merge.sh dry-run（隔离 target，结论前）

```bash
cd /Users/zyzs/lczyzs/code/zgraph
CARGO_HOME=/Users/zyzs/lczyzs/code/zgraph/.cargo-home CARGO_NET_OFFLINE=true \
CARGO_TARGET_DIR=/tmp/rev031-target CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=2 RUST_TEST_THREADS=2 \
PYTHON_BIN=/Users/zyzs/lczyzs/code/zgraph/.venv/bin/python \
bash scripts/dev/merge.sh fix/worker-m5-crash-detail --dry-run
```

- **实跑（隔离 target `/tmp/rev031-target`，load avg≈4.6）**：`INFO: 评审记录 -> 通过` → `cargo test` 通过 → pytest 失败 **0** / 基线 **0**（无新增）→ `INFO: dry-run 结束，检查全部通过`（exit 0）。
- 说明：dry-run 在 **rebase 后**分支（base=main=`58239fc`）上执行，覆盖 REV-032 合入后的全量测试；未出现 AC-09 时序 flake。

## 5. 遗留问题

- **docs 第二批（REV-035）必须同步**（§3 #1）：spec 06 §6.2.5.1 `NodeWorker` canonical JSON 表增 `signal/exit_code/stderr_tail`（可选、仅崩溃路径、stderr_tail ≤1 KiB 字符边界）并修订「不得新增未列出的顶层键」的闭集表述（或按 spec 版本规则登记）；同时关闭 ISSUE-066（`doc/issues.md:74`）并在 03 §3.15/architecture §8.2 注记实现证据；补「worker 自报 crash 无进程证据」与「stderr 证据不脱敏/窗口」注记（§3 #2/#3）。
- **敏感信息（§3 #2）**：`stderr_tail` 不脱敏，落 trace DB 与 `--json`；文档注明即可（本地 worker，Lead 已确认）。
- **第二评审**：按 DR-002 §4/§6，Lead 将请 reviewer-b 在本记录追加第二评审段（跨模块 kernel/worker/cli + harness 机械修正）。
- **合并前置（§3 #8）**：评审时 base=main=`58239fc` 可 ff；若合并前 main 再前进需 rebase 后复跑门禁。
- **合并后**：Lead 更新 `doc/reviews/README.md`/`doc/index.md` 的 REV-031 索引、`doc/progress.md`、`doc/issues.md`（ISSUE-066 待 docs 同步后 fixed）。
- 本评审未修改分支任何文件；REV-031 记录为本分支唯一新增文件（由评审人提交）。

## 6. 评审签署

- 跨模块范围/批准/双评审要求满足（本记录主审声明）；kernel 只增字段且 `code()`/`Display`/4 码经测试不回归；worker 在真实子进程（exit 7/SIGKILL/exit 3）与 map 边界上均正确采集证据；cli 仅 Some 输出新键、既有 JSON 键不变；harness 8 行机械适配；错误码与 `NodeError` Display 无回归。
- 定向测试 404 passed/0 failed/1 ignored（rebase 后）、workspace 539 passed/0 failed/2 ignored（内容等价树）、clippy/fmt/check.sh PASS；无依赖/文档/其它模块夹带改动；改动源码无新增 >60 行函数。
- 唯一遗留为 **spec 06 §6.2.5.1 冻结闭集冲突**，按 Lead 口径由 docs 第二批同步（记录已写明，不阻塞）。
- **结论：通过**（待 reviewer-b 追加第二评审段；Lead 合并前如需 rebase 再复跑门禁）。
- 评审人：`reviewer-a`（非作者）；评审时间：2026-10-30。

## 7. 第二评审（跨模块，reviewer-b，2026-10-30）

> 依据 DR-002 §4/§6 的双评审要求，本段为 reviewer-a 主审后的**增量第二评审**；按 AGENTS §6 不删改 §1–§6，原结论/证据保留。评审对象为代码 `58239fc..22f4395`（评审期间 HEAD 由 `5556dd6` amend 为 `e9b2b19`：reviewer-a 仅对记录补 dry-run 证据，代码 commit `22f4395` 未变，`git diff 22f4395 e9b2b19 -- crates` 为空）。本次在**独有 fresh target** `/tmp/rev031-b-target` 下独立复核（避开共享 `target/` 被并行 worktree 产物污染），未修改分支任何代码/文档（本段除外）；未 merge/push。

### 7.1 跨模块改动最小性（四模块逐项）

| 模块 | 实际 diff | 第二评审核对 |
|---|---|---|
| kernel | `types.rs`：`NodeError::NodeWorker` 只增 `signal/exit_code/stderr_tail` 三个 `Option` 字段；`code()` 改 `NodeWorker { kind, .. }`；新增 3 个单测。`tests/failure.rs`：既有 4 码构造补 `None` | ✅ 最小。`Display` 仍 `#[error("worker error ({kind:?})")]`，`code()` 仍 `kind.code()`（4 个 worker 码不变，单测 `worker_error_codes_are_stable_with_crash_details`/`node_worker_crash_payload_keeps_code_and_display`）；kernel 无其它文件改动。**注意**：给公开 enum variant 增字段对「仓外按字面量构造 `NodeError::NodeWorker`」是 **source-breaking**（仓内全部构造点已更新、workspace 编译通过；crate 0.1、Lead 已批准该跨模块变更） |
| worker | `spawn.rs`：`WorkerFailure::Exit` 增 `signal`；`supervisor.rs`：`classify_exit` 用 `ExitStatusExt::signal()`（`cfg(unix)`，非 Unix `None`）、`attach_stderr` 保留 signal、单测补 signal 断言；`executor.rs`：`map_failure` 仅 `Exit` 路径填充三字段（其余 `node_worker_error` 恒 `None`）、`stderr_tail`/`STDERR_TAIL_BYTES`、`stderr_tail` 取尾部 ≤1 KiB 并按 UTF-8 字符边界回退、空白→`None`；`lib.rs` re-export；tests/fixtures 增真实 `exit 7`/`SIGKILL`/`exit 3` 场景 | ✅ 最小且证据只在进程退出路径：exit 3 仍 `Protocol`（`supervisor_m3` 真 fixture 断言），`Spawn`/`Protocol`/`Timeout`/worker 自报 crash 均无进程证据（`None`）；`code`/`signal` 互斥；`stderr_tail` 方向为 supervisor 已保留末尾 8 KiB 的尾部 1 KiB。无 Cargo/依赖/其它模块改动 |
| cli | `bridge.rs::node_error_json` 的 `NodeWorker` 分支 | ✅ 最小。既有 `code/message/kind/worker_kind` 键与值不变；`signal/exit_code/stderr_tail` **仅 Some 时插入**（非 null，非崩溃零新增键）；单测覆盖 signal-only/exit-only/非崩溃三态 |
| harness | `tools.rs::call_worker_failure` **一处** match arm：`WorkerFailure::Exit` 解构 `signal` 并把 `signal` 拼入诊断串（`format!("… code={code:?} signal={signal:?}: {stderr}")`）；无其它 harness 改动、无测试改动、无控制流/分支变化，`ToolFailure.reason="worker_crash"` 与映射语义不变 | ✅ 行为上可接受的最小适配。**描述需修正**：这不是「纯模式绑定」——诊断串文本确实增加了 `signal=…`（同文件另一处 `discover_worker_error` 的 `failure.to_string()` 也因 `WorkerFailure::Exit` Display 变化而多出 `signal`）；但无执行语义变化、无测试/下游精确字符串断言（全仓 grep `worker exited unexpectedly|tool worker exited|worker_crash` 仅命中实现与测试名），不阻塞。建议 Lead/记录把该项目表述为「模式解构 + 诊断串补 signal（编译必需）」，避免被误读为逐字节行为不变 |

**范围复核**：代码 commit（`58239fc..22f4395`）**13 文件全部属于 kernel/worker/cli/harness**（+tests/fixtures），无 `Cargo.toml/Cargo.lock`、无 `doc/**`/其它 crate 改动；`Cargo.lock` 无变化（与主审一致）。harness 交叉依赖 worker 新字段、cli 读 kernel 新字段，四处并非「只改一模块」：Lead 已批准跨模块并由本第二评审确认，满足 DR-002 §4 双评审要求。

### 7.2 独立验证证据（独有 fresh target）

| 命令 | 结果 |
|---|---|
| `cargo test -p zgraph-kernel -p zgraph-worker -p zgraph-cli -p zgraph-harness -- --test-threads=2`（**fresh** `/tmp/rev031-b-target`；`ZGRAPH_PYTHON=<repo>/.venv/bin/python`） | **404 passed / 0 failed / 1 ignored**（与主审 404/0/1 一致；含真实子进程 `exit 7`→`exit_code=Some(7)+stderr_tail`、`SIGKILL`→`signal=Some(9)`、`exit 3`→`NODE_WORKER_PROTOCOL`，以及 kernel 码/Display/CLI JSON/字符边界单测） |
| 新增子进程用例点名核对 | `non_convention_exit_code_is_recorded_in_crash_failure`、`signal_kill_is_recorded_in_crash_failure`、`sdk_self_check_exit_three_stays_a_protocol_error`、`signal_exit_maps_to_worker_crash`、`non_convention_exit_maps_to_crash_with_exit_code_and_stderr`、`node_error_json_expands_node_worker_crash_details`、`stderr_tail_is_truncated_on_char_boundary` 均 **ok**（真实 Python 子进程，非 mock `ExitStatus`） |
| `cargo test -p zgraph-e2e --test fault_injection`（CLI 错误 JSON 消费方回归） | **7 passed / 0 failed**（`NODE_WORKER_CRASH`/HANDSHAKE/IMPORT/PROTOCOL 既有断言不回归） |
| `cargo test --workspace -- --test-threads=2`（附加交叉验证） | **552 passed / 0 failed / 2 ignored**（无失败；与主审 workspace 无新增失败口径一致） |
| `cargo clippy -p zgraph-kernel -p zgraph-worker -p zgraph-cli -p zgraph-harness --all-targets -- -D warnings`（fresh） | **exit 0 / 0 warning** |
| `cargo fmt --all -- --check`、`bash scripts/dev/check.sh` | **PASS**（fmt/身份/冻结文件/禁用文件/fmt/diff-check） |
| 函数长度（改动源码） | 新增/改动函数均 ≤60（`stderr_tail`、`node_worker_error`、`exit_signal` 等短函数；既有 >60 的 `validate_schema_node`/worker roundtrip test 非本分支引入） |

### 7.3 第二评审发现（增量；不重复主审 §3）

| # | 级别 | 位置 | 问题 | 建议 | 状态 |
|---|---|---|---|---|---|
| 1 | 建议（测试加固） | `crates/zgraph-worker/src/executor.rs:472`（`stderr_tail_is_truncated_on_char_boundary` 用例） | 用例输入 `"a" + "é".repeat(1024)` 的尾部起点 `len-1024 = 1025` 恰好是字符边界（`é` 从奇数偏移开始），因此**没有触发** `stderr_tail` 的「`while !is_char_boundary(start) { start += 1 }` 回退分支；实现按代码审查正确，但该边界分支未被测试覆盖 | 把输入改为总长使 `len-1024` 落在 3 字节字符中间（如 `"a" + "汉".repeat(N)` 且总字节 >1024 且 `len-1024 % 3 == 1`），断言 tail ≤1024 且全部为完整字符、`stderr.ends_with(tail)`。**不阻塞**（实现正确；主审已通过） | open（建议） |
| 2 | 备注（描述准确性） | `crates/zgraph-harness/src/tools.rs:1376-1385`；`crates/zgraph-worker/src/spawn.rs:74-87` | harness 的「一处模式绑定修正」实际还修改了 `call_worker_failure` 诊断串（追加 `signal=…`）；`WorkerFailure::Exit` 的 `Display` 也追加 signal（`discover_worker_error` 的 message 随之变化）。均属诊断信息增强，无执行语义/错误码/分支变化，且全仓无精确字符串断言 | Lead/后续记录按「解构 + 诊断串补 signal」描述；无需代码修改。**不阻塞** | 无需处理 |
| 3 | 备注（API 兼容） | `crates/zgraph-kernel/src/types.rs:442-451`；`crates/zgraph-worker/src/spawn.rs:79-90` | 新增字段使公开 enum variant 的**字面量构造** source-breaking（仓内已全部更新、编译通过；crate 版本 0.1、仓内私有发布面）。与 ISSUE-066「只增不删」字段语义一致，但外部按旧字面量构造会编译失败 | 若后续对外发布该 crate，需在发布说明标注 API 变更/考虑 `#[non_exhaustive]`；本轮 Lead 已批准跨模块。**不阻塞** | 无需处理（发布说明备注） |
| 4 | 备注（docs 遗留） | 同主审 §3 #1/#2/#3 | spec 06 §6.2.5.1 的 `NodeWorker` canonical JSON 闭集与 03 §3.15/architecture §8.2 的 signal/exit 记录要求仍需 docs 第二批（REV-035）回写；stderr_tail 不脱敏/证据窗口（8 KiB 尾部的 1 KiB）、worker 自报 crash 无进程证据需注记 | 由 docs REV-035 承接；ISSUE-066 待 docs 后转 fixed。**不阻塞** | open（docs） |

> 第二评审未发现阻塞/重要项；主审 §3 的 docs 遗留（#1）、敏感信息（#2）、自报 crash 无证据（#3）在本轮仍成立，无需重复登记。

### 7.4 第二评审结论

**第二评审：同意（通过）**。

- 四模块改动满足「最小、按批准范围」：kernel 只增证据字段且错误码/`Display` 不回归；worker 仅在真实进程退出路径采集 signal/exit/stderr_tail（exit 3 仍协议、其它路径 `None`）；cli 仅 Some 输出新键、既有 JSON 键不变；harness 一处解构 + 诊断串补 signal（非纯模式绑定，但无逻辑/语义变化，已如实标注）。
- 真实子进程证据有效（exit 7 / SIGKILL / exit 3 均由真实 Python 进程产生，非 mock）；fresh target 定向 **404 passed/0 failed/1 ignored**、e2e fault_injection 7/7、workspace 552/0/2、clippy/fmt/check PASS；无新增失败。
- 唯一需修正的是**评审记录/Lead 对 harness 改动的表述**（模式解构 + 诊断串，而非纯模式绑定）与**可选**的字符边界测试加固（§7.3 #1）；均不阻塞，不影响主审「通过」结论。
- 遗留：spec 06 §6.2.5.1 闭集/03 §3.15/architecture §8.2 注记由 docs REV-035 回写后关闭 ISSUE-066。
- 第二评审人：`reviewer-b`（非作者）；评审时间：2026-10-30。
