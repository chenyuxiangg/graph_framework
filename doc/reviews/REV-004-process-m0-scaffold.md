# REV-004 · process · M0 骨架（Cargo workspace / worker 协议 v1 / Python SDK / legacy 冻结）

> **跨模块声明**：本分支由 Lead 指定为 **M0 bootstrap**，覆盖 `process` + 各 crate 空壳（dsl/kernel/trace/harness/cli）+ `py-worker` + `e2e`（fixtures）+ `docs`（索引/进度/台账）+ `legacy`（迁移）。按 AGENTS §2 的跨模块例外，本评审记录该声明；不因跨模块本身判不通过。

- **分支**：`feat/process-m0-scaffold`
- **作者**：`lead`（首版 `23feedb`；复审 `dbdfa0e`，`Agent: lead`）
- **评审人**：`docs-reviewer`（独立评审，非作者）
- **变更范围**：`7379e03..dbdfa0e`（首版 127 files, +2102/−119；复审 `2c5b53d..dbdfa0e`：15 files, +216/−47）
- **评审时间**：2026-10-29（初评）；2026-10-29（复审）
- **关联**：architecture §6.1 M0 DoD、§3.5 协议检查单；ISSUE-015/018/019/020/021/023/024/025；schemas/worker-protocol-v1.md

## 1. 结论

**结论：不通过**

复审 `dbdfa0e` 后，初评 #3–#8 已修复（selftest 现在安装当前树并断言 cargo test、`schemas` 改 §8.2、legacy/DR 路径、`os._exit` 前 flush stderr、README/uv、merge.sh 6a/6b 与空基线 `|| true`），`spawn.rs` 超时分支也已加入显式 `kill().await` + `wait().await`。但仍有 2 个阻塞项：

1. **声称的"超时 reap 测试"不存在**：`dbdfa0e` 没有修改 `crates/zgraph-worker/tests/echo_roundtrip.rs`（`git diff --name-only 2c5b53d..dbdfa0e` 中无该文件）；文件中仍只有旧的 `worker_timeout_kills_process`（只断言 `Timeout`），没有 `worker_timeout_kills_and_reaps_process`、没有 PID 断言。`crates/zgraph-worker/Cargo.toml` 新增的 `tempfile` dev-dependency 和 `python/zgraph_worker/slow.py` 的 `pid_file` 写入均**无任何测试使用**；`cargo test` 实际为 7 个 worker 单测 + 3 个集成（不是声称的 8+3）。`schemas/worker-protocol-v1.md:66` 的 `[x] 超时 kill+reap（测试断言 PID 消失）` 与 `doc/issues.md:32` 的"M0 已覆盖 … 超时 kill+reap（PID 断言）"因此不成立（#1）。
2. **协议检查单 `[x]` 仍有未测项**：半行帧已修复并有 Rust/Python 单测；`ensure_no_extra_frame` 已加入重复 result 检测，但**没有任何测试**，且只覆盖 result 后 100ms 窗口（#3）；`schemas:57` 的 hello 超时 `[x]` 也没有对应测试；`schemas:60` 的重复 result `[x]` 与 `ISSUE-025` 的"M0 已覆盖重复 result"不成立（#2）。

其余 M3 缺口（supervisor cancel、帧空闲超时、流式读取上限、exit-0-无-result、进程组 kill、并发上限）已在 `schemas` §7 与 ISSUE-025 登记，属于诚实的延后项；但上述 `[x]` 与"测试断言"必须与仓库实际一致后才能通过。

## 2. 四维评审

| 维度 | 结论 | 依据（文件/行/测试） |
|---|---|---|
| 功能正确性 | ⚠️ | M0 DoD 1（6 crate + `--version`）、5（legacy 迁移）、6（双栈流程/CI）主体满足：`cargo test`/pytest/check/version 均通过。DoD 2/3/7 仍有缺口：超时 reap 与重复 result/hello 超时的测试不存在，`schemas`/ISSUE-025 的 `[x]` 不真实（#1/#2）。 |
| 代码结构合理性 | ⚠️ | Cargo workspace 6 crate、Rust/Python 协议双侧、legacy 隔离结构清晰；复审已修 selftest/文档/脚本。剩余问题是"测试/声明不一致"与无用 dev-dependency/未用的 `pid_file` 支持（#1/#4），不是架构问题。 |
| 逻辑性 | ⚠️ | `spawn.rs` 超时分支现在显式 `kill().await`+`wait().await`，正常路径自洽；`decode_frame_line` 半行检测、Python `read_frame` 新规则正确；但 `ensure_no_extra_frame` 仅 100ms 窗口且无测试，重复 result 的边界未验证（#3）；协议 `[x]` 与实际测试不符（#2）。 |
| 可读性 | ✅ | 复审后注释/文档明显收敛：`spawn.rs` 超时注释、协议 §6 `[x]`/`[M3]` 状态化、`os._exit`/stderr 说明、`legacy/README`/DR O-5/README uv 指引均已修正；仅 `attach_stderr` 中不可达的 Timeout→Exit 分支和 `slow.py` 的测试钩子可清理（#5/#6）。 |

## 3. 发现清单

| # | 级别 | 位置 | 问题 | 建议 | 状态 |
|---|---|---|---|---|---|
| 1 | 阻塞 | `crates/zgraph-worker/src/spawn.rs:149-158`；`crates/zgraph-worker/tests/echo_roundtrip.rs:52-68`；`crates/zgraph-worker/Cargo.toml:15-16`；`python/zgraph_worker/slow.py:16-20`；`schemas/worker-protocol-v1.md:66`；`doc/issues.md:32` | **声称的超时 reap 测试不存在**：`git diff --name-only 2c5b53d..dbdfa0e` 没有 `tests/echo_roundtrip.rs`；文件仍是旧 `worker_timeout_kills_process`，仅 `matches!(Timeout)`，没有 PID/`kill -0` 断言，也没有 `worker_timeout_kills_and_reaps_process`。`tempfile` dev-dep 与 `slow.py` 的 `pid_file` 支持均未被任何测试使用；`cargo test` 实际是 7 worker 单测 + 3 集成（不是 8+3）。`schemas:66`/ISSUE-025 的 `[x] 超时 kill+reap（PID 断言）` 不真实。 | 补上真正的 `worker_timeout_kills_and_reaps_process`（slow worker 写 PID，超时后轮询 `kill -0`/waitpid 断言消失或已被 reap），或删除 `tempfile`/`pid_file` 支持；`cargo test` 计数以实际为准；在测试落地前不要标 `[x]` 或写"M0 已覆盖"。 | open（阻塞） |
| 2 | 阻塞 | `schemas/worker-protocol-v1.md:57,60,66`；`doc/issues.md:32`；`crates/zgraph-worker/src/spawn.rs:178-215,261-266` | **协议 `[x]`/ISSUE-025 与测试不符**：`schemas:57` hello 超时 `[x]` 无测试；`schemas:60` 重复 result `[x]` 无测试；`ensure_no_extra_frame` 只在 result 后等 100ms，且没有测试；`ISSUE-025` 称 M0 已覆盖 hello 超时/重复 result/超时 reap，但仓库中没有对应测试。半行帧已修复（Rust `decode_frame_line`/Python `read_frame` 有单测），这部分可保留 `[x]`。 | 为 hello 超时、重复 result、timeout reap 增加集成/单测（可用一个测试专用 worker 在 result 后写额外帧/不退出）；或把对应项改为 `[M3]`/未覆盖并同步 ISSUE-025。 | open（阻塞） |
| 3 | 重要 | `crates/zgraph-worker/src/spawn.rs:261-266`；`schemas/worker-protocol-v1.md:60` | `ensure_no_extra_frame` 只等待 100ms：result 后 100ms 之后到达的额外帧/重复 result 会被接受；且当前没有任何测试。 | 明确协议窗口（例如 result 后 1s 退出宽限内持续检测），或把 100ms 写入协议并补测试；建议用测试 worker 覆盖「100ms 内/外」两种。 | open（重要） |
| 4 | 重要 | `crates/zgraph-worker/Cargo.toml:15-16`；`Cargo.lock:199-...`；`python/zgraph_worker/slow.py:16-20` | `tempfile` dev-dependency 与 `slow.py` 的 `pid_file` 测试钩子没有对应测试；`pid_file` 来自 run inputs，属于测试专用文件写入能力进入了示例 worker 的接口。 | 要么补上使用它们的 reap 测试，要么移除 `tempfile`/`pid_file` 支持；若保留，在 `slow.py` 文档中标注仅测试/示例使用。 | open（重要） |
| 5 | 建议 | `crates/zgraph-worker/src/spawn.rs:158-166` | `attach_stderr` 的 `WorkerFailure::Timeout(_) if !stderr.is_empty()` 分支实际不可达（`run_worker` 的 Timeout 分支直接返回），且会把 Timeout 改判为 Exit，属死代码/误导。 | 删除该分支，或给 Timeout 增加 stderr 字段并如实返回 Timeout；补对应测试。 | open（建议） |
| 6 | 建议 | `python/tests/test_runtime.py:128-141`；`schemas/worker-protocol-v1.md:61` | `schemas:61` 声称 stderr 压力用例，Python `test_print_and_stderr_do_not_pollute_protocol` 只验证协议未被污染，没有断言 stderr 内容/8KiB 截断；Rust 侧无 stderr 压力集成测试。 | 增加 stderr 内容尾部/截断断言（或明确 Python 侧覆盖范围）。 | open（建议） |

## 4. 验证证据

### 4.1 复审 diff 与静态

```bash
cd .worktrees/process-agent-lead
git diff --stat 2c5b53d..dbdfa0e       # 15 files changed, 216 insertions(+), 47 deletions(-)
git diff --name-only 2c5b53d..dbdfa0e | grep -E "echo_roundtrip|tests"
# → 只有 python/tests/test_protocol.py；没有 crates/zgraph-worker/tests/echo_roundtrip.rs
grep -rn "worker_timeout_kills_and_reaps_process\|pid_file\|tempfile" \
  crates/zgraph-worker/tests crates/zgraph-worker/src python/tests python/zgraph_worker
# → 只有 python/zgraph_worker/slow.py 的 pid_file 分支；无测试引用
```

### 4.2 必跑命令（本机）

```bash
export CARGO_HOME="$PWD/.cargo-home" HTTPS_PROXY="http://127.0.0.1:7890" HTTP_PROXY="http://127.0.0.1:7890"
rustc --version          # rustc 1.99.1 (2026-10-13)
cargo fmt --all -- --check          # exit 0
cargo clippy --workspace --all-targets -- -D warnings  # exit 0
cargo test --workspace              # exit 0；实际 7 worker 单测 + 3 集成（另有 cli/dsl 各 1）
# 测试清单中只有 echo_worker_roundtrip / worker_timeout_kills_process / import_failure_is_reported；
# 没有 worker_timeout_kills_and_reaps_process。
cargo run -q -p zgraph-cli -- --version  # zgraph 0.1.0 (DSL 1.0)
PYTHONPATH=python /tmp/gf_venv/bin/python -m pytest -q  # 19 passed
bash scripts/dev/check.sh           # CHECK PASSED（cargo fmt 检查通过，progress 18 行）
PYTHON_BIN=/tmp/gf_venv/bin/python bash scripts/dev/selftest-merge.sh  # SELFTEST PASSED
```

### 4.3 已确认修复的项

- `spawn.rs:149-158` 超时分支已显式 `child.kill().await` + `child.wait().await`（代码层面已改；但仓库无测试验证 reap）。
- `decode_frame_line`（Rust）与 `read_frame`（Python）拒绝半行帧，且各有单测。
- `selftest-merge.sh` 改为 `git archive HEAD` 安装当前树并断言 `cargo test 通过`；本次 `SELFTEST PASSED`，`/tmp/selftest-merge.log` 含 `INFO: cargo test 通过`。
- `merge.sh` 空基线 `grep ... || true` 修复，`#6a/#6b` 注释重排。
- `schemas:50` 改为 `§8.2`；`legacy/README:12` 修正 kernel 保留 id；`DR-001:70` O-5 改为 `legacy/graph_base_design.md`。
- `python/zgraph_worker/runtime.py:178-186` 新增 `_shutdown` 在 `os._exit` 前 flush stderr；协议文档写明跳过 atexit。
- `README.md:31` 删除 pip 指引，改为安装 uv。

### 4.4 未通过的证据（对应 #1/#2）

- `schemas/worker-protocol-v1.md:66`：`[x] 超时不等待：显式 kill + wait/reap（单进程，测试断言 PID 消失）`；实际 `tests/echo_roundtrip.rs:52-68` 仍是旧测试，只断言 `Timeout`，无 PID。
- `schemas:57` hello 超时 `[x]`：无对应测试（`protocol.rs` 只有 v 不匹配/缺字段单测）。
- `schemas:60` 重复 result `[x]`：新增 `ensure_no_extra_frame`，但无测试；且 100ms 窗口未写入协议。
- `ISSUE-025:32` 声称 M0 已覆盖 "hello 超时/版本、hello_ack、重复 result、…、超时 kill+reap（PID 断言）"；其中 hello 超时、重复 result、timeout reap 的测试并不存在，属于不准确台账。
- 提交信息称 "Rust 测试用 pid 文件断言进程消失" 与 `git diff`/`cargo test` 实际不符。

## 5. 遗留问题

- **阻塞 #1/#2 未关闭，结论不通过**：请补真正的 timeout-reap 测试（PID 消失/reap）与 duplicate result/hello timeout 测试，或把 `[x]`/ISSUE-025 降级为 `[M3]`/未覆盖；修正 `tempfile`/`pid_file` 未使用项。
- 重要 #3/#4、建议 #5/#6 可一并处理；`selftest` 双栈、协议文档/legacy 文档/flush stderr 已修复，不再列为未关闭。
- 本次评审未 push/merge/改作者其他文件；仅修改本评审记录。

## 6. 条件关闭记录

不适用（结论为"不通过"）。

## 7. 复审记录（2026-10-29，`dbdfa0e`）

- 复审范围 `2c5b53d..dbdfa0e`：15 files, +216/−47；初评 #3–#8 已关闭（selftest 双栈、§8.2、legacy/DR、flush stderr、README、merge 6a/6b + 空基线）。
- 代码修复：`spawn.rs` 超时分支已显式 kill/wait；半行帧 Rust/Python 已拒；协议 §6 状态化、ISSUE-025 重写；但**声明中的 reap 测试未提交**，`schemas`/ISSUE-025 的 `[x]` 与仓库测试不一致。
- **结论：维持不通过**；待 #1/#2 测试/台账修正后重新复审。
