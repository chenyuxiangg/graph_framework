# REV-004 · process · M0 骨架（Cargo workspace / worker 协议 v1 / Python SDK / legacy 冻结）

> **跨模块声明**：本分支由 Lead 指定为 **M0 bootstrap**，覆盖 `process` + 各 crate 空壳（dsl/kernel/trace/harness/cli）+ `py-worker` + `e2e`（fixtures）+ `docs`（索引/进度/台账）+ `legacy`（迁移）。按 AGENTS §2 的跨模块例外，本评审在记录中写明该声明；不因跨模块本身判不通过。

- **分支**：`feat/process-m0-scaffold`
- **作者**：`lead`（提交 `23feedb`，`Agent: lead`）
- **评审人**：`docs-reviewer`（独立评审，非作者）
- **变更范围**：`7379e03..23feedb`（127 files changed, 2102 insertions(+), 119 deletions(-)）：Cargo workspace/6 crate、worker 协议 v1、Python SDK、legacy 迁移、CI/流程、fixtures、文档
- **评审时间**：2026-10-29
- **关联**：architecture §6.1 M0 DoD、§3.5 协议检查单；ISSUE-015/018/019/020/021/023/024/025；schemas/worker-protocol-v1.md

## 1. 结论

**结论：不通过**

M0 的工程骨架方向正确，`cargo fmt/clippy/test`、Python pytest、`check.sh`、`zgraph --version` 全部通过，6 crate/CLI/legacy 迁移/CI/流程主体均已落地。但存在 2 个阻塞项与若干重要项：

1. **超时 kill_on_drop 未真正完成子进程回收**：`run_worker` 超时后只依赖 `Child` drop 的 `kill_on_drop`，未 `kill().await`/`wait().await`；/tmp 探针显示子进程确实收到 SIGKILL（waitpid WNOHANG 返回该 PID、status=9），但在被 wait 前一直是 **zombie**；Rust 集成测试 `worker_timeout_kills_process` 只断言返回 `Timeout`，没有验证进程已退出/无僵尸，测试名与 M0 §3.5 检查单第 9 项「无残留子进程/僵尸」不符（#1）。
2. **协议检查单与 ISSUE-025 不完整/不诚实**：`schemas/worker-protocol-v1.md:55-66` 的 M0 检查单 10 项全部未勾选，architecture §3.5 明确「M0 DoD 必须逐项有测试」；实现上 supervisor 无 `cancel` 路径、无帧空闲超时、半行帧不报错（`lines()`/`readline()` 会把 EOF 前最后一行当完整帧）、重复 `result` 不被检测；ISSUE-025 只列了用户点名的 6 项，且声称「M0 已覆盖取消」，实际只有 Python worker 侧、Rust supervisor 没有（#2）。M0 DoD 的「通过 §3.5 检查单」未满足。

其余重要项见 #3–#6；均可在 M0 内关闭后复审。

## 2. 四维评审

| 维度 | 结论 | 依据（文件/行/测试） |
|---|---|---|
| 功能正确性 | ⚠️ | M0 DoD 1（6 crate + `--version`）、5（legacy 迁移主体）、6（双栈流程/CI）基本满足：`cargo test` 绿、pytest 18 项绿、`zgraph --version` 正常。但 DoD 2/3/7 未满足：协议检查单多数无测试、supervisor cancel/半行/重复 result/帧超时缺失（#2）；超时子进程未被回收（#1）。 |
| 代码结构合理性 | ⚠️ | Cargo workspace 6 crate 分工清晰；协议 Rust/Python 双侧文件对称；Python SDK 独立于 legacy；跨模块 bootstrap 已声明。问题集中在 `zgraph-worker` 的生命周期实现与协议文档：`schemas` §5 指向错误小节（#4）；`legacy/README` 与 DR-001 O-5 残留旧路径/错误退役描述（#5）。 |
| 逻辑性 | ⚠️ | 正常路径（握手→run→result/error→退出）自洽；`exit 0 无 result` 在 Rust 侧可判协议错误。但异常路径不完整：超时 drop 不 wait（#1）；取消/帧空闲/半行/重复 result 无实现或无测试（#2）；`os._exit` 跳过解释器清理需明确风险（#6）。 |
| 可读性 | ⚠️ | 注释整体解释「为什么」（`merge.sh` 分支删除、`os._exit` 原因、协议帧）；但文档存在陈旧引用/错误描述（#4/#5），README 的临时 pip 指引与 CLAUDE.md「只用 uv」冲突（#7），`merge.sh` 测试块重复编号（#8）。 |

## 3. 发现清单

| # | 级别 | 位置 | 问题 | 建议 | 状态 |
|---|---|---|---|---|---|
| 1 | 阻塞 | `crates/zgraph-worker/src/spawn.rs:108-155`；`crates/zgraph-worker/tests/echo_roundtrip.rs:53-66`；`schemas/worker-protocol-v1.md:65` | **超时只 kill_on_drop，不 wait/reap**：`run_worker` 超时返回 `WorkerFailure::Timeout` 后，`child` 在函数末尾 drop 时由 `kill_on_drop(true)` 发 SIGKILL；未 `kill().await`/`wait().await`，进程被杀但成为 zombie。我在 /tmp clone 的探针中：超时 300ms 后 `waitpid(pid, WNOHANG)` 返回 pid、status=9（表明僵尸、尚未 reap）；测试 `worker_timeout_kills_process` 仅 `matches!(Timeout)`，没有验证子进程真实退出。M0 §3.5 第 9 项明确要求「无残留子进程/僵尸」。 | Timeout 分支显式 `child.kill().await` + `child.wait().await`（M3 再升级进程组 kill）；测试读取/等待 PID 或断言 `/proc`/`kill -0` 无残留；重命名/扩展测试覆盖「进程已退出且已 reap」。#1 关闭前不给通过。 | open（阻塞） |
| 2 | 阻塞 | `schemas/worker-protocol-v1.md:24,31-37,46,55-66`；`crates/zgraph-worker/src/spawn.rs:208-241`；`python/zgraph_worker/protocol.py:65-70`；`doc/issues.md:32` | **协议检查单/ISSUE-025 不完整**：①`cancel` 只在 Python worker 侧有 reader，Rust supervisor 无 cancel 发送/2s+TERM+KILL 流程；②无帧空闲超时（文档 `worker_frame_timeout_ms` 默认 300s）；③半行帧：Rust `lines()`、Python `readline()` 会把 EOF 前最后一行当完整帧解析，不报协议错误；④重复 `result` 不检测（收到首个 result 后 `ensure_exit` 只等退出）；⑤`schemas` 检查单 10 项全部未勾选，architecture §3.5 要求 M0 DoD 逐项有测试；⑥ISSUE-025 只列 6 项（用户点名的），没有 cancel/半行/帧超时/UTF-8/hello 超时/hello_ack 等，且称「M0 已覆盖取消」但 Rust supervisor 没有。 | 在 M0 内至少把未实现/未测项**全部**登记 ISSUE-025 并标注「未实现/仅 Python 侧/无测试」；实现 cancel supervisor、半行检测、帧空闲超时、重复 result 的检测或明确延后到 M3 并登记；同步 `schemas` §6 勾选/标注状态。 | open（阻塞） |
| 3 | 重要 | `scripts/dev/selftest-merge.sh:21-32`；`scripts/dev/merge.sh:74-84`；M0 的 `selftest` 日志 `/tmp/selftest-merge.log:4` | **M0 双栈门禁未被 selftest 覆盖**：selftest clone 的是 **main 工作区（7379e03，M0 之前，无 Cargo.toml）**，只把 `SRC/scripts/dev/merge.sh` 提交到 clone main；测试分支没有 Cargo.toml，merge.sh 输出「分支无 Cargo.toml（legacy 期），跳过 cargo test」。因此用户要求的 `SELFTEST PASSED` 只验证了旧的 Python 门禁，没有验证本分支新增的 `cargo test` 双栈门禁。 | 让 selftest 使用当前分支树（或复制 Cargo.toml/crates 到 clone main 后提交），至少断言 `cargo test` 被执行；或另建 M0 专用 selftest 并在结果中列出两项门禁。 | open（重要） |
| 4 | 重要 | `schemas/worker-protocol-v1.md:50`；`doc/architecture-v2.md:256-277` | 协议文档说错误码与 `architecture-v2.md §8.1` 对齐，但 §8.1 现在是「偏差的代码落点」，新错误码在 §8.2；`NODE_WORKER_*` 清单与 D10 的「新增错误码见 §8.2」不一致。 | 改为 `§8.2`（或引用「§8.2 新增错误码表」）；M0 文档检查单同步。 | open（重要） |
| 5 | 重要 | `legacy/README.md:12`；`doc/decisions/DR-001-runtime-route.md:70`；`doc/index.md:11` | legacy 迁移后仍有两处陈旧/错误引用：`legacy/README` 说「旧模块 id（`kernel`/`scheduler`/...）退役」，但 `kernel` 保留 id（新路径 `crates/zgraph-kernel/**`，AGENTS §2 明确保留）；DR-001 O-5 仍写 `doc/graph_base_design.md`，文件已移至 `legacy/graph_base_design.md`。 | legacy/README 改为「scheduler/state-manager/dispatcher/monitor/orchestrator/workers/config 退役；kernel 保留 id 但路径更新」；DR-001 O-5 更新或标 resolved（M0 已调整 README）；doc/index 已指向 legacy。 | open（重要） |
| 6 | 重要 | `python/zgraph_worker/runtime.py:178-181`；`python/zgraph_worker/noisy.py:14-19`；`python/tests/test_runtime.py:128-141` | **`os._exit` 原因/风险**：注释说明 daemon stdin reader 在解释器 shutdown 时会报 `could not acquire lock for BufferedReader`，所以用 `os._exit` 跳过 shutdown；这确实规避了锁错误，但会跳过 `sys.stderr.flush()`/atexit/用户清理。`noisy.py` 自己 flush 了，但普通用户 `print(..., file=sys.stderr)` 的缓冲可能丢失；测试没有断言最终 stderr 内容。 | 在 `os._exit` 前显式 `sys.stderr.flush()`；在 SDK 文档写明「worker 退出不执行用户 atexit、stderr 需 flush」；增加一个 stderr 尾部断言测试（与 stderr 压力/截断一起）。 | open（重要） |
| 7 | 建议 | `README.md:31`；`CLAUDE.md`（冻结） | README「本地无 uv 时：`python3 -m venv .venv && .venv/bin/pip install pytest`」与 CLAUDE.md「使用 uv 管理依赖，不使用 pip 直接安装」冲突；虽然标注「仅临时验证」，仍是误导性指引。 | 改为 `uv sync --extra dev` 失败时的排查说明，或明确「仅离线/临时、不作为项目依赖流程」并链接 ISSUE-003。 | open（建议） |
| 8 | 建议 | `scripts/dev/merge.sh:73-90`；`doc/issues.md:32` | `merge.sh` 两个测试块都标 `# 6)`（cargo/Python），注释重复；ISSUE-025 说「8MiB 运行期」未覆盖，但 `Frame::from_line`/`decode_frame` 已有超限检查（缺的是流式读取上限/集成测试），表述过宽；`run_worker` 对 `Timeout` 的 `attach_stderr` 分支实际不可达/会改判 `Exit`，属死代码可清理。 | 重编号 `#6`/`#7`；ISSUE-025 精确区分「已实现未测」与「未实现」；删除或修正不可达 `attach_stderr` 分支。 | open（建议） |

## 4. 验证证据

### 4.1 必跑命令

```bash
cd .worktrees/process-agent-lead
export CARGO_HOME="$PWD/.cargo-home" HTTPS_PROXY="http://127.0.0.1:7890" HTTP_PROXY="http://127.0.0.1:7890"
rustc --version          # rustc 1.99.1 (2026-10-13)
cargo fmt --all -- --check          # exit 0
cargo clippy --workspace --all-targets -- -D warnings  # exit 0
cargo test --workspace              # exit 0；11 个测试（cli 1 + dsl 1 + worker 6 单测 + echo/timeout/import 3 集成）
cargo run -q -p zgraph-cli -- --version  # zgraph 0.1.0 (DSL 1.0)
PYTHONPATH=python /tmp/gf_venv/bin/python -m pytest -q  # 18 passed
bash scripts/dev/check.sh           # CHECK PASSED（cargo fmt 检查通过，progress 18 行）
PYTHON_BIN=/tmp/gf_venv/bin/python bash scripts/dev/selftest-merge.sh  # SELFTEST PASSED
```

### 4.2 特别验证（在 /tmp clone，仓库未改动）

- **超时 kill_on_drop**：在 /tmp clone 增加探针 worker（写 PID 到 `/tmp/m0probe_pid.txt`，stderr 写一行后 `time.sleep(5)`），以 300ms `WorkerConfig.timeout` 调 `run_worker`：
  - 返回 `WorkerFailure::Timeout(300ms)`；
  - 300ms 后 `waitpid(pid, WNOHANG)` 返回该 PID、status=9（SIGKILL），说明子进程已被 kill 但**未被 wait/reap**（僵尸）；`kill -0` 也仍可命中。
  - 同名测试 `worker_timeout_kills_process` 只断言 `Timeout`，不检查 PID/僵尸。
- **selftest 双栈覆盖**：`/tmp/selftest-merge.log` 第 4 行为 `INFO: 分支无 Cargo.toml（legacy 期），跳过 cargo test`；dry-run 与真实合并日志同样跳过。selftest 的 clone 来自 `MAIN_WT=7379e03`（M0 之前），只覆盖旧 Python 门禁。
- **协议静态核对**：
  - `grep -n "cancel" crates/zgraph-worker/src/spawn.rs` 无 supervisor 发送/信号流程（仅协议类型与测试没有 cancel 发送 API）；Python `runtime._cancel_reader` 有取消标志。
  - `spawn.rs:210-215` 使用 `lines.next_line()`；`protocol.py:65-70` 使用 `readline()`，EOF 前没有换行的最后一行仍被 `Frame::from_line`/`decode_frame` 接受，未实现 `schemas` §4.4「EOF 半行 = 协议错误」。
  - `spawn.rs:219-235` 收到首个 `result` 后直接 `ensure_exit` 返回，不继续读取后续帧；`schemas` §3「多 result = 协议错误」未实现。
  - `schemas:55-66` 的 10 个 `[ ]` 全部未勾选；`architecture §3.5:131` 明确「M0 DoD 必须逐项有测试」。
- **legacy 引用**：`grep -rn "graph_base_design\|旧模块 id" doc legacy` 找到 `legacy/README:12` 与 `DR-001:70`；`doc/index:11` 已正确指向 `legacy/README.md`。
- **链接/静态**：doc 下 markdown 相对链接 0 断链；`git diff --check` 通过。

### 4.3 测试与 M0 DoD 映射

| DoD | 结果 |
|---|---|
| 1 Cargo 6 crate + version | ✅ `cargo test`/`cargo run --version` 通过 |
| 2 协议文档与两侧实现一致 | ⚠️ 帧/握手/run/exit 基本；cancel/半行/帧空闲/重复 result/8MiB 流式限制未一致（#2） |
| 3 Rust 集成 echo/timeout/import | ⚠️ 三项存在；timeout 只测变体，不验证 kill/reap（#1） |
| 4 Python SDK 测试 | ✅ 18 passed；`os._exit` 风险见 #6 |
| 5 legacy 迁移 | ⚠️ 主体完成；`legacy/README:12`/`DR-001:70` 陈旧（#5） |
| 6 流程双栈 | ⚠️ AGENTS/worktree/check/merge/CI 已做；selftest 未覆盖 cargo 门禁（#3） |
| 7 ISSUE-025 登记诚实 | ⚠️ 用户点名 6 项已登记；但缺 cancel/半行/帧超时/UTF-8/hello 超时/hello_ack，且「M0 已覆盖取消」不准确（#2） |

## 5. 遗留问题

- **阻塞 #1/#2 未关闭，结论不通过**：请先修超时 kill/wait/reap 与测试，再补齐 ISSUE-025/协议检查单（或实现缺失分支），再复审。
- 重要项 #3–#6：selftest 双栈覆盖、§8.2 引用、legacy 文档、`os._exit` stderr flush，建议一并修。
- 建议项 #7/#8 可后置，但应登记/修正，避免误导。
- 本次评审未 push/merge/改作者其他文件；仅新增本评审记录（按任务要求）。

## 6. 条件关闭记录

不适用（结论为"不通过"）。
