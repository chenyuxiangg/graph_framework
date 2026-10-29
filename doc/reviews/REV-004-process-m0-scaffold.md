# REV-004 · process · M0 骨架（Cargo workspace / worker 协议 v1 / Python SDK / legacy 冻结）

> **跨模块声明**：本分支由 Lead 指定为 **M0 bootstrap**，覆盖 `process` + 各 crate 空壳（dsl/kernel/trace/harness/cli）+ `py-worker` + `e2e`（fixtures）+ `docs`（索引/进度/台账）+ `legacy`（迁移）。按 AGENTS §2 的跨模块例外，本评审记录该声明；不因跨模块本身判不通过。

- **分支**：`feat/process-m0-scaffold`
- **作者**：`lead`（首版 `23feedb`；两轮修正 `dbdfa0e`、`993ab12`，`Agent: lead`）
- **评审人**：`docs-reviewer`（独立评审，非作者）
- **变更范围**：`7379e03..993ab12`（首版 127 files, +2102/−119；最终修正 `4e9f590..993ab12`：7 files, +173/−21）
- **评审时间**：2026-10-29（初评/复审/终审）
- **关联**：architecture §6.1 M0 DoD、§3.5 协议检查单；ISSUE-015/018/019/020/021/023/024/025；schemas/worker-protocol-v1.md

## 1. 结论

**结论：不通过**

`993ab12` 已真实补齐大量测试：`crates/zgraph-worker/tests/echo_roundtrip.rs` 现有 7 个集成测试（echo、超时 reap、导入失败、hello 超时、重复 result、exit-0-无 result、noisy stderr 内容），`tempfile` 与 `slow.py` 的 `pid_file` 已被使用；`spawn.rs` 的显式 kill+wait、`decode_frame_line`/Python 半行帧、失败 result 检测、`selftest` 当前树 + cargo 断言、`schemas` §8.2、legacy/DR、`os._exit` flush、README/merge 6a/6b 均已修。实测 `cargo test`（7 worker 单测 + 7 集成）、pytest 19、check.sh、selftest 全部通过。

但仍有 2 个阻塞项：

1. **握手超时/错误路径仍未 reap**：`run_worker` 的显式 `kill().await`+`wait().await` 只在**整体 run 超时**分支（`spawn.rs:148-159`）；`hello_timeout_is_reported`（`echo_roundtrip.rs:118-132`）走 `drive` 返回 `Err` → `run_worker` `Ok(Err(err))`（`spawn.rs:159`），child 仅靠 drop 的 `kill_on_drop` 终止，未 wait/reap；测试只断言 `NODE_WORKER_HANDSHAKE`，不检查 PID/僵尸。`hello` 超时同样属于超时路径，M0 §3.5 第 1 项要求「拒绝并 kill」，生命周期修复应覆盖所有错误路径或明确登记。
2. **`schemas`/`ISSUE-025` 仍有 `[x]`/"均有测试"与仓库不符**：`schemas:58` 的 `hello_ack 前不执行用户代码` `[x]` 无对应测试（Python 测试只发送 ack，不测缺失 ack 时不执行用户代码）；`schemas:65` 的「非法 UTF-8」`[x]` 无测试（Rust `decode_frame_line` 有错误分支但无测试）；`ISSUE-025:32` 写「M0 已覆盖（均有测试）：… hello_ack … 半行帧/UTF-8 …」并声称附测试名，但实际未列测试名，且 hello_ack/UTF-8 无直接测试。M0 §3.5 要求逐项有测试，台账不能写「均有测试」。

其余项已关闭；#1（run 超时 reap）的测试已真实存在并通过，不再作为阻塞；#2 的其余检查项（重复 result、exit-0、hello 超时、stderr 内容、半行帧、8MiB）已有测试或单测。

## 2. 四维评审

| 维度 | 结论 | 依据（文件/行/测试） |
|---|---|---|
| 功能正确性 | ⚠️ | DoD 1/5/6 满足；run 超时 reap、重复 result、exit-0、hello 超时、noisy stderr 测试均通过。DoD 2/7 仍有 `[x]` 无测试项（hello_ack/非法 UTF-8）与 issue 不准确；握手超时错误路径未 reap（#1/#2）。 |
| 代码结构合理性 | ✅ | 6 crate/协议双侧/Python SDK/legacy 隔离清晰；7 个集成测试和 2 个 fixture 落位合理；`tempfile`/`pid_file` 不再悬空；跨模块 bootstrap 已声明。 |
| 逻辑性 | ⚠️ | 正常路径与 run 超时路径自洽；`ensure_no_trailing_frame` 在 wait 后读残余帧、`decode_frame_line` 半行检测正确。但握手错误路径只依赖 drop（#1）；`ensure_no_trailing_frame` 50ms 窗口未写入协议（建议）。 |
| 可读性 | ✅ | 协议 §6 `[x]`/`[M3]` 状态化，测试命名清晰，`selftest` 注释更新；仅 `ISSUE-025` 缺测试名、`hello_ack`/UTF-8 标记过早（#2）。 |

## 3. 发现清单

| # | 级别 | 位置 | 问题 | 建议 | 状态 |
|---|---|---|---|---|---|
| 1 | 阻塞 | `crates/zgraph-worker/src/spawn.rs:148-159`；`crates/zgraph-worker/tests/echo_roundtrip.rs:118-132`；`schemas/worker-protocol-v1.md:57` | **握手超时/错误路径未显式 reap**：显式 kill+wait 只在 `match result { Err(_) => ... }`（整体 `timeout`）分支；`hello_timeout_is_reported` 走 `drive` 的 handshake error → `run_worker Ok(Err(err))`，child 在 drop 时由 `kill_on_drop` 发 SIGKILL，未 `wait`；测试只断言错误码，未检查子进程消失/非僵尸。hello 超时也是超时路径，M0 §3.5 第 1 项「拒绝并 kill」的生命周期应覆盖。 | 在 `run_worker` 的 `Ok(Err(err))` 等所有错误返回前显式 `child.kill().await`+`child.wait().await`（或在 guard 中统一处理）；`hello_timeout` 测试用可写 PID 的 worker，超时后轮询 `kill -0`/waitpid 断言消失；同步 `schemas:57`。 | open（阻塞） |
| 2 | 阻塞 | `schemas/worker-protocol-v1.md:58,65`；`doc/issues.md:32` | **`[x]`/"均有测试"与仓库不符**：`hello_ack 前不执行用户代码` 无测试（现有 Python `_handshake` 只发送 ack，不验证缺失 ack 时不执行用户代码）；`非法 UTF-8` 无测试（Rust 有 `from_utf8` 错误分支但无用例）；`ISSUE-025` 写「M0 已覆盖（均有测试）」但未附测试名，与实际不符。 | 补 hello_ack 负例（不发送 ack，worker 不得执行 run）与非法 UTF-8 帧测试；把 `ISSUE-025` 改为逐项列测试名或改为「已有单测/集成」并列出覆盖范围；若无法在 M0 补，标 `[M3]` 并登记。 | open（阻塞） |
| 3 | 建议 | `crates/zgraph-worker/src/spawn.rs:258-269`；`schemas/worker-protocol-v1.md:60` | `ensure_no_trailing_frame` 在进程退出后只等 50ms 读残余；若最后一帧在 50ms 后才可读（管道/调度）可能漏检；协议文档只写"进程退出后读残余帧"，未写 50ms。 | 明确窗口/改为读到 EOF（进程已 wait，正常应立即 EOF；可设合理上限并写明）；重复 result 测试已覆盖主路径，建议再覆盖边界。 | open（建议） |
| 4 | 建议 | `crates/zgraph-worker/tests/echo_roundtrip.rs:169-180`；`schemas:61` | noisy stderr 测试断言尾部和 8KiB；但未断言被截断的 128KiB 压力不会阻塞/只保留尾部（目前结果验证了最终内容，未验证截断边界）。 | 增加 `run.stderr.len()==8KiB` 边界或大 stderr 计数断言（若可暴露），或说明当前测试覆盖范围。 | open（建议） |
| 5 | 建议 | `crates/zgraph-worker/src/spawn.rs:180-215` | `read_frame_line` 仍用无界 `read_until` 读入整行后才检查 8MiB，恶意 worker 可先造成无界内存分配；`schemas` 已把流式读取上限标 M3，但 M0 帧解码层实际不是严格 8MiB 上限。 | 在读到超过 `MAX_FRAME_BYTES` 时提前失败（或明确写 M3 前仅解码层检查）；与 ISSUE-025 的「8MiB 运行期」对齐。 | open（建议） |

## 4. 验证证据

### 4.1 复审 diff 与测试

```bash
cd .worktrees/process-agent-lead
git diff --stat 4e9f590..993ab12
# 7 files changed, 173 insertions(+), 21 deletions(-)
git diff --name-status 4e9f590..993ab12
# M spawn.rs; M echo_roundtrip.rs; M issues/schema/noisy; A dup_result_worker.py/no_result_worker.py
grep -rn "hello_ack\|invalid utf" crates/zgraph-worker/tests python/tests \
  --include='*.rs' --include='*.py'
# 仅 Python test_runtime.py 的 _handshake 发送 hello_ack；无 hello_ack 负例、无非法 UTF-8 测试
```

### 4.2 必跑命令（本机）

```bash
export CARGO_HOME="$PWD/.cargo-home" HTTPS_PROXY="http://127.0.0.1:7890" HTTP_PROXY="http://127.0.0.1:7890"
cargo fmt --all -- --check          # exit 0
cargo clippy --workspace --all-targets -- -D warnings  # exit 0
cargo test --workspace              # exit 0
# 7 worker 单测；7 集成：echo_worker_roundtrip / worker_timeout_kills_and_reaps_process /
# import_failure_is_reported / hello_timeout_is_reported / duplicate_result_is_rejected /
# exit_without_result_is_rejected / noisy_worker_stderr_is_captured —— 全部 ok
cargo run -q -p zgraph-cli -- --version  # zgraph 0.1.0 (DSL 1.0)
PYTHONPATH=python /tmp/gf_venv/bin/python -m pytest -q  # 19 passed
bash scripts/dev/check.sh           # CHECK PASSED
PYTHON_BIN=/tmp/gf_venv/bin/python bash scripts/dev/selftest-merge.sh  # SELFTEST PASSED
grep -n "cargo test 通过" /tmp/selftest-merge.log  # 命中，双栈门禁已执行
```

### 4.3 已确认修复

- `worker_timeout_kills_and_reaps_process` 真实存在并通过（tempfile/pid_file 使用）；`run_worker` 超时分支显式 kill+wait。
- 半行帧 Rust/Python 测试；重复 result/exit-0/hello 超时/noisy stderr 集成测试。
- `selftest` 当前树 + cargo 断言；`schemas` §8.2；legacy/DR；`os._exit` flush；README/merge 6a/6b。
- 计数：7 个 worker 集成测试、7 个 worker 单测、cli/dsl 各 1；与提交说明一致。

### 4.4 仍未通过的证据（#1/#2）

- `run_worker` 的 `match result` 中只有 `Err(_)` 分支的显式 kill/wait（`spawn.rs:148-159`）；handshake 错误走 `Ok(Err(err))`，child 只由 drop 的 `kill_on_drop` 终止，无 wait。`hello_timeout_is_reported` 未断言 PID 消失。
- `hello_ack`：`grep` 仅 Python `_handshake` 发送 ack，无负例；`schemas:58` 标 `[x]`。
- 非法 UTF-8：`protocol.rs` 的 `from_utf8` 分支无测试；`schemas:65` 标 `[x]`。
- `ISSUE-025:32` 写「M0 已覆盖（均有测试）」但未附测试名，且上述两项无测试，与用户要求「已登记且诚实」不符。

## 5. 遗留问题

- **阻塞 #1/#2 未关闭，结论不通过**；请先给握手错误路径统一 kill+wait/reap 并补测试，再补 hello_ack 负例/非法 UTF-8 测试或把 `[x]` 降级为 `[M3]` 并同步 ISSUE-025（最好附测试名）。
- 建议 #3–#5 可后置；`selftest`/文档/legacy/`os._exit` 已修复，不再列为未关闭。
- 本次评审未 push/merge/改作者其他文件；仅修改本评审记录。

## 6. 条件关闭记录

不适用（结论为"不通过"）。

## 7. 复审记录（2026-10-29，`993ab12`）

- 复审范围 `4e9f590..993ab12`：7 files, +173/−21。
- 已确认：7 个集成测试真实存在（含 `worker_timeout_kills_and_reaps_process`/hello/duplicate/no-result/noisy）；`tempfile`/`pid_file` 被使用；`cargo test`、pytest、check、selftest 全部通过；此前声明的测试/计数已修正。
- 仍未通过：#1 握手错误路径只 drop 不 wait/reap（含 hello 超时测试不断言）；#2 `schemas:58/65` 与 ISSUE-025 的 `[x]`/"均有测试"仍与仓库不符（无 hello_ack 负例、无非法 UTF-8 测试）。
- **结论：维持不通过**；修 #1/#2 后复审。
