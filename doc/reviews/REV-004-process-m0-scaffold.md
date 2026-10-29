# REV-004 · process · M0 骨架（Cargo workspace / worker 协议 v1 / Python SDK / legacy 冻结）

> **跨模块声明**：本分支由 Lead 指定为 **M0 bootstrap**，覆盖 `process` + 各 crate 空壳（dsl/kernel/trace/harness/cli）+ `py-worker` + `e2e`（fixtures）+ `docs`（索引/进度/台账）+ `legacy`（迁移）。按 AGENTS §2 的跨模块例外，本评审记录该声明；不因跨模块本身判不通过。

- **分支**：`feat/process-m0-scaffold`
- **作者**：`lead`（首版 `23feedb`；修正 `dbdfa0e`、`993ab12`、`0bb5f74`，`Agent: lead`）
- **评审人**：`docs-reviewer`（独立评审，非作者）
- **变更范围**：`7379e03..0bb5f74`（首版 127 files, +2102/−119；最终修正 `9b3fc01..0bb5f74`：7 files, +132/−30）
- **评审时间**：2026-10-29（初评/复审/终审）
- **关联**：architecture §6.1 M0 DoD、§3.5 协议检查单；ISSUE-015/018/019/020/021/024/025；schemas/worker-protocol-v1.md

## 1. 结论

**结论：通过**

`0bb5f74` 复核后，前两轮的两个阻塞项均已按"事实一致"关闭：

1. **超时/握手错误路径 kill+reap**：`run_worker` 现在对任何非成功结果（`Err(timeout)` 与 `Ok(Err(_))`）统一执行 `child.kill().await` + `child.wait().await`；`worker_timeout_kills_and_reaps_process` 与新增的 `hello_timeout_kills_and_reaps_process` 都通过 PID 文件和 `kill -0` 轮询断言 PID 消失；测试真实通过。
2. **`[x]` 与测试一致**：新增 `hello_ack` 负例（`test_no_execution_before_hello_ack` + `marker_worker.py`）和非法 UTF-8 测试（`invalid_utf8_frame_is_rejected` + fixture）；`schemas` §6 各项补测试名，`ISSUE-025` 逐项附测试名并只把 supervisor cancel/TERM/KILL、result 后不退出、帧空闲、流式上限、进程组、并发上限留到 M3；`ensure_no_trailing_frame` 改为 wait 后读到 EOF，无 50ms 窗口。

M0 DoD 1–7 均可核验：6 crate + `--version`、协议 v1 与测试、Python SDK、legacy 迁移、AGENTS/worktree/check/merge/CI/selftest，以及 ISSUE-025 的实际登记。实测 `cargo fmt/clippy/test`、pytest、check.sh、selftest-merge.sh 全部通过（详见 §4）。

剩余仅为文档/边角建议（#8–#10，不阻塞）；M0 可以合并。

## 2. 四维评审

| 维度 | 结论 | 依据（文件/行/测试） |
|---|---|---|
| 功能正确性 | ✅ | M0 DoD 1–7 满足：workspace 6 crate + `zgraph --version`；协议 v1 双向实现；Rust 集成 8 测试（echo、run 超时 reap、import、hello 超时 reap、dup result、exit-0-无-result、invalid UTF-8、noisy stderr）全绿；Python 20 tests 全绿；selftest 真正执行 cargo test。 |
| 代码结构合理性 | ✅ | 6 crate 职责清晰；Rust/Python 协议双侧对称；`run_worker` 生命周期统一在 `spawn.rs`，fixture/示例 worker 与测试分离；legacy 冻结只读；跨模块 bootstrap 有声明。仅 #8–#10 的建议级文档/边界说明。 |
| 逻辑性 | ✅ | `run_worker:140-159` 在成功路径由 `ensure_exit` wait，失败路径统一 kill+wait；`ensure_no_trailing_frame` 在进程退出后读到 EOF，重复帧测试通过；`decode_frame_line`/Python `read_frame` 半行检测一致；`os._exit` 前 flush stderr。 |
| 可读性 | ✅ | 协议 §6 `[x]`/`[M3]` 状态化，`ISSUE-025` 附测试名；`selftest` 注释说明当前树/cargo 断言；`legacy/README`/DR O-5/README uv 指引清晰。建议 §3.3 补重复帧方法一句（#8）。 |

## 3. 发现清单

| # | 级别 | 位置 | 问题 | 建议 | 状态 |
|---|---|---|---|---|---|
| 1 | 阻塞（前次） | `crates/zgraph-worker/src/spawn.rs:140-159`；`crates/zgraph-worker/tests/echo_roundtrip.rs:69-147` | 前次：run 超时/握手错误只依赖 drop，未 reap | 已修：任何非成功结果统一 kill+wait；run/hello 超时测试用 PID 文件 + `kill -0` 断言消失 | fixed（0bb5f74） |
| 2 | 阻塞（前次） | `schemas/worker-protocol-v1.md:58,65`；`doc/issues.md:32`；`python/tests/test_runtime.py:175`；`crates/zgraph-worker/tests/echo_roundtrip.rs:183` | 前次：hello_ack/非法 UTF-8 `[x]` 无测试；ISSUE-025 未附测试名 | 已补 `test_no_execution_before_hello_ack`（marker_worker）与 `invalid_utf8_frame_is_rejected`（fixture）；§6/ISSUE-025 均附测试名 | fixed（0bb5f74） |
| 3 | 重要（前次） | `scripts/dev/selftest-merge.sh:23-38,69-81` | 前次：selftest clone 旧 main，cargo test 被跳过 | 已改为 `git archive HEAD` 安装当前树，断言 `cargo test 通过`；本次实测日志命中 | fixed（0bb5f74） |
| 4 | 重要（前次） | `schemas/worker-protocol-v1.md:50`；`doc/architecture-v2.md` §8.2 | 前次：错误码引用 §8.1 | 已改 §8.2 | fixed（0bb5f74） |
| 5 | 重要（前次） | `legacy/README.md:12`；`doc/decisions/DR-001-runtime-route.md:70` | 前次：kernel 退役描述错误、O-5 旧路径 | 已改：kernel/cli/e2e/docs/process 保留 id，仅路径迁移；O-5 指向 `legacy/graph_base_design.md` | fixed（0bb5f74） |
| 6 | 重要（前次） | `python/zgraph_worker/runtime.py:178-186` | 前次：`os._exit` 跳过 stderr flush | 已加 `_shutdown` flush stderr；协议文档写明跳过 atexit | fixed（0bb5f74） |
| 7 | 建议（前次） | `README.md:27-31`；`scripts/dev/merge.sh:74-84` | 前次：README 临时 pip 指引；merge 测试注释重复/空基线 `grep -v` | 已改 uv 指引；merge 改 6a/6b，空基线加 `\|\| true` | fixed（0bb5f74） |
| 8 | 建议 | `schemas/worker-protocol-v1.md:27-37,60` | 本次 `0bb5f74` 实际只改 §6/§5，§3.3 表格未动；重复帧判定方法写在 §6 而非 §3.3（不影响实现/测试） | 在 §3.3 补一句"进程退出后读残余帧判重"，与 §6 对齐（可后置） | open（建议） |
| 9 | 建议 | `schemas/worker-protocol-v1.md:59,65`；`doc/issues.md:32` | §6 的 `run payload`/8MiB 条目未附具体测试名；ISSUE-025 部分条目（print/input/cancel）也未列名，虽已有测试 | 补测试名或注明"单测/集成"；便于下轮审计 | open（建议） |
| 10 | 建议 | `crates/zgraph-worker/src/spawn.rs:261-270` | `ensure_no_trailing_frame` 在 child wait 后读到 EOF；若 worker 有孙进程仍持 stdout，会阻塞（M3 进程组 kill 才覆盖） | 在 M3 前不依赖孙进程；或加保护/文档说明（M0 示例无孙进程，不影响） | open（建议） |

## 4. 验证证据

### 4.1 最终改动与测试

```bash
cd .worktrees/process-agent-lead
git diff --stat 9b3fc01..0bb5f74
# 7 files changed, 132 insertions(+), 30 deletions(-)
# spawn.rs / echo_roundtrip.rs / issues / schema / python runtime test / 两个 fixture
```

```bash
export CARGO_HOME="$PWD/.cargo-home" HTTPS_PROXY="http://127.0.0.1:7890" HTTP_PROXY="http://127.0.0.1:7890"
cargo fmt --all -- --check            # exit 0
cargo clippy --workspace --all-targets -- -D warnings  # exit 0
cargo test --workspace                # exit 0
# 7 worker 单测 + 8 集成（echo、run timeout reap、import、hello timeout reap、
# duplicate result、exit-without-result、invalid UTF-8、noisy stderr），全 ok
cargo run -q -p zgraph-cli -- --version  # zgraph 0.1.0 (DSL 1.0)
PYTHONPATH=python /tmp/gf_venv/bin/python -m pytest -q  # 20 passed
bash scripts/dev/check.sh             # CHECK PASSED（cargo fmt 检查、progress 18 行）
PYTHON_BIN=/tmp/gf_venv/bin/python bash scripts/dev/selftest-merge.sh  # SELFTEST PASSED
grep -n "cargo test 通过" /tmp/selftest-merge.log  # 命中；双栈门禁真实执行
```

- 以上 `cargo test` 测试名与 §3/§4.1 一致；`hello_timeout_kills_and_reaps_process` 与 `worker_timeout_kills_and_reaps_process` 都以 PID 文件 + `kill -0` 断言 PID 消失。
- `pytest` 20 项：`test_ctx` 7、`test_protocol` 6、`test_runtime` 7（含 `test_no_execution_before_hello_ack`）。
- 本次未 push/merge/改作者其他文件；仅修改本评审记录与 `doc/reviews/README.md`。

## 5. 遗留问题

- **无阻塞项**；M0 DoD 1–7 均已达到可验收状态，结论转为通过。
- 建议 #8–#10（§3.3 重复帧方法一句、§6/ISSUE-025 补测试名、M3 孙进程/stdout 边界说明）可后置，不影响合并。
- 合并本分支时注意：若工作区使用本地 main 的旧 merge.sh，首次可能停在旧脚本删分支步骤；按 `doc/development-workflow.md` §8 重跑新脚本即可（ISSUE-023 两阶段）。
- M3 遗留项已登记 ISSUE-025（supervisor cancel/TERM/KILL、帧空闲、流式上限、result 后不退出、进程组、并发上限），符合"未覆盖项登记"要求。

## 6. 条件关闭记录

| 原阻塞项 | 关闭证据 | 关闭日期 |
|---|---|---|
| #1 超时/握手错误路径未 reap | `spawn.rs:140-159` 统一 kill+wait；`worker_timeout_kills_and_reaps_process` + `hello_timeout_kills_and_reaps_process` 用 PID 文件/`kill -0` 断言通过 | 2026-10-29 |
| #2 hello_ack/非法 UTF-8 `[x]` 无测试、ISSUE-025 无测试名 | 新增 `test_no_execution_before_hello_ack`（marker fixture）与 `invalid_utf8_frame_is_rejected`（invalid UTF-8 fixture）；`schemas` §6 与 ISSUE-025 均附测试名 | 2026-10-29 |
| #3 selftest 未覆盖 cargo | `selftest-merge.sh` 用 `git archive HEAD` 安装当前树并断言 `cargo test 通过`；本机 `SELFTEST PASSED` | 2026-10-29 |

> 前次"不通过"的阻塞已全部关闭；本评审结论由"不通过"转为"通过"。

## 7. 复审与转通过记录

### 7.1 初评/两轮复审（`23feedb` / `dbdfa0e` / `993ab12`）

- 初评：M0 DoD 主体验收，但发现超时未 reap、协议检查单/ISSUE-025 不完整、selftest 未覆盖 cargo 等。
- `dbdfa0e`：修代码路径与 selftest，但声称的 reap 测试未真实提交。
- `993ab12`：真实补齐 7 个集成测试（含 reap/hello/dup/exit/noisy），但握手错误路径未统一 reap、hello_ack/UTF-8 无测试/台账不准确。

### 7.2 终审（2026-10-29，`0bb5f74`）——转为通过

- 复核范围 `9b3fc01..0bb5f74`：7 files, +132/−30。
- 逐项确认：`run_worker` 非成功路径统一 kill+wait（含 handshake error）；hello 超时/非法 UTF-8/hello_ack 负例测试真实存在并 pass；`schemas` §6/ISSUE-025 附测试名；`ensure_no_trailing_frame` 读到 EOF；selftest 当前树 + cargo 断言通过。
- 验证：`cargo fmt/clippy` 通过；`cargo test --workspace` 7 单测 + 8 集成全绿；pytest 20；check PASS；selftest PASS；`zgraph --version` 正常。
- 剩余 #8–#10 仅为建议级；四维无阻塞项，M0 DoD 1–7 已可验收。
- **结论：通过**。本分支可执行合并；合并后由 Lead 更新 ISSUE-023/025/progress 及 worktree 清理。
