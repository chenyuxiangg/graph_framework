# REV-011 · kernel · M2 超时实例错误回填 + 同步段 panic 捕获

> **模块声明**：本分支只改 `crates/zgraph-kernel/**`（`src/run.rs` + `tests/timeout.rs` + `tests/failure.rs`），未触及 dsl/trace/cli/文档；符合 AGENTS §2 的 kernel 模块边界。作者 `kernel-dev`，评审人 `reviewer-a`（非作者）。

- **分支**：`fix/kernel-m2-timeout-error`
- **作者**：`kernel-dev`
- **评审人**：`reviewer-a`
- **变更范围**：`4570a53..0deefd9`（1 commit：`fix(kernel): 回填超时实例错误并捕获同步 panic`；`git diff --stat` = 3 files, +109/−4：`src/run.rs` +19/−4、`tests/timeout.rs` +47、`tests/failure.rs` +43；`Cargo.toml`/`Cargo.lock` 未变，无 `crates/zgraph-kernel/` 之外改动）
- **评审时间**：2026-10-29
- **关联**：spec 06 §6.2.5（`instances.error/error_code`）/§6.2.7（`transitions.error_code/error`）/§6.2.12；spec 00 §0.4 `NODE_TIMEOUT`；spec 03 §3.9（超时/看门狗）；spec 07 AC-06；architecture-v2 §8 **D1**（超时 = kill）；REV-007 §7 残留建议（同步 `execute()` panic）

## 1. 结论

**结论：通过**

本提交修复两个已定位问题，均已独立复现验证（不采信 commit 自述）：

1. **超时实例错误回填（必修）**：`check_timeouts()` 在 `Running→TimedOut` 迁移**之前**调用新增 `set_instance_error(NodeError::Timeout{elapsed_ms})`（`src/run.rs:1329-1331`），使该次迁移的 `InstanceSnapshot` 携带 `error=Timeout{elapsed_ms}`、`error_code="NODE_TIMEOUT"`。独立探针：`TimedOut` 迁移 from=Running、snapshot.error_code=`NODE_TIMEOUT`、snapshot 的 `elapsed_ms`(103) 与 `NodeTimedOut` 事件 elapsed_ms(103) **完全一致**；run 级错误仍 `NODE_TIMEOUT`、`abandoned_tasks=0`、`killed_workers=1`、无 `NodeAbandoned`、≤5s 失败 → **D1 语义未变**。trace writer 侧已确认从迁移快照取列：`transitions.error_code = snapshot.error_code`、`transitions.error = JSON(snapshot.error)`、`instances.error/error_code` 由 `upsert_instance(snapshot)` 写入（`crates/zgraph-trace/src/writer/tx.rs:383,398,655,681`）——kernel 侧契约闭合；DB 级 SQL 断言属 CLI 桥接（REV-009）。
2. **同步段 panic 捕获（REV-007 残留建议）**：`spawn_execution()` 改为 `let call = async move { executor.execute(execution).await }; CatchPanic::new(call)`（`src/run.rs:774-777`），把 `execute(...)` 的同步段纳入同一 `catch_unwind`。独立探针：同步 `execute()` 内 panic → **648µs** 内 `NODE_INTERNAL`/Crashed、message 保留 `sync boom probe`、无 NodeTimedOut（修复前需等默认 180s deadline 才 `NODE_TIMEOUT`）。无 `unsafe`（`#![forbid(unsafe_code)]` 保留，`src/**`/`tests/**` grep 无 unsafe），不吞 payload；超时/取消回归正常。

测试强度经**独立反向验证**：临时把 `src/run.rs` 还原为 `4570a53` 版本后，新增 `timed_out_transition_snapshot_carries_timeout_error` 在 `timeout.rs:103` 因 `error_code=None != Some("NODE_TIMEOUT")` 立即失败（0.11s）；`executor_sync_panic_normalizes_to_internal_and_fails_fast` 在 180.02s 后失败（`failure.rs:425/436` 断言）——证明两用例真回归，不是改断言凑绿。随后已恢复 `run.rs` 并校验 sha256 与 HEAD 一致、worktree 干净。

未发现阻塞/重要问题；3 条建议级见 §3。实测：kernel **78 passed**、`--features test-hooks` **81 passed**、workspace **214 passed / 0 failed**；fmt/clippy（含 test-hooks）/check.sh/pytest 全过。

## 2. 四维评审

| 维度 | 结论 | 依据（文件/行/测试） |
|---|---|---|
| 功能正确性 | ✅ | ①超时快照 error/error_code 在迁移前回填（`run.rs:925-937` helper、`:1329-1331` 调用），elapsed_ms 与 NodeTimedOut 同源同值；D1 全部指标不变（探针 + `timeout.rs` 断言）。②同步 panic 纳入 CatchPanic（`run.rs:774-777`），探针 648µs Internal/Crashed、payload 保留。③Crashed 路径复用 helper 无行为变化（`failure.rs` 既有用例 + 探针 P3）。 |
| 代码结构合理性 | ✅ | 抽出 `set_instance_error()` 供 `fail_crashed`/timeout 共用（DRY）；改动仅 3 文件、最小面；`CatchPanic` 仍在同任务内捕获，不引入 detached 任务；无 unsafe、无新依赖（Cargo.lock 未变）。 |
| 逻辑性 | ✅ | 回填发生在 `transition_or_fail` 之前是关键顺序（trace writer 只从该次迁移快照取 error 列）；`elapsed_ms` 单变量同时用于 NodeTimedOut 事件与 NodeError，杜绝两处漂移；超时/取消/fail_run 路径未改语义；无重复写/覆盖风险（`upsert_instance` 用 COALESCE 保留 error）。 |
| 可读性 | ✅ | helper 与调用点注释解释“为什么必须迁移前写”“为什么把同步段包进 async 块”；测试名/断言自解释；commit message 说明修复动机与回归验证方式。 |

## 3. 发现清单

| # | 级别 | 位置 | 问题 | 建议 | 状态 |
|---|---|---|---|---|---|
| 1 | 建议 | `tests/failure.rs:421-455`（sync panic 用例） | 未修复时该用例要等默认节点超时 **180s** 才失败（实测 180.02s），回归反馈慢、CI 会卡 3 分钟。 | 改用 `linear_spec_with_work_timeout(200)`（或 test-hooks `force_node_timeout_ms`）让失败在 ~200ms 内暴露；断言不变。 | open（建议） |
| 2 | 建议 | `tests/timeout.rs:74-120`；`src/run.rs:925-937` | 本分支只断言 trace sink 快照；`instances.error/error_code`、`transitions.error_code/error` 的 **SQL 级**断言依赖 CLI 桥接把 kernel `NodeError` 序列化为 JSON/错误码（kernel `NodeError` 无 serde）。trace writer 已确认按 snapshot 写列，kernel 侧已闭合。 | 在 REV-009（cli）加 AC-06 实例级 SQL 断言（`instances.error_code='NODE_TIMEOUT'`、`transitions.error_code='NODE_TIMEOUT'`、`instances.error` JSON 含 Timeout 形状），并在 CLI 映射中保持 00 §0.3 `NodeError` 变体形状。 | open（建议，随 REV-009） |
| 3 | 建议 | `commit 0deefd9` message | message 的“测试”段落只描述了 timeout 用例，未提 sync panic 用例（信息小缺口，不影响代码）。 | 下不为例；无需修订（提交信息已含 Review/Agent trailer）。 | open（建议） |
| 4 | 备注 | `doc/progress.md` / `doc/issues.md` | 本分支未更新进度/台账（kernel 修复行的进度与 REV-011 登记）。 | 由 Lead 合并时按 AGENTS §5/§7 更新（不影响本分支结论）。 | open（流程） |

> 无阻塞项、无重要项；#1/#2 为建议级，#2 与 REV-009 联动。

## 4. 验证证据

### 4.1 范围与分支

```bash
cd /Users/zyzs/lczyzs/code/zgraph/.worktrees/kernel-kernel-dev
git branch --show-current                # fix/kernel-m2-timeout-error
git log --oneline -2
# 0deefd9 fix(kernel): 回填超时实例错误并捕获同步 panic
# 4570a53 docs(review): REV-007 复审增量（= origin/main）
git merge-base origin/main HEAD            # 4570a53（分支基于最新 main，可 ff）
git diff --stat 4570a53..0deefd9           # 3 files changed, 109 insertions(+), 4 deletions(-)
git diff --name-only 4570a53..0deefd9 | grep -v '^crates/zgraph-kernel/' || echo "(none)"   # (none)
git status --short                         # 评审提交前空
```

提交身份 `chenyuxiangg <1007423165@qq.com>`，message 含 `Review: REV-011` / `Agent: kernel-dev`。

### 4.2 必跑命令（worktree 内，`CARGO_HOME=ROOT/.cargo-home`）

```bash
cargo fmt --all -- --check                                        # exit 0
cargo clippy --workspace --all-targets -- -D warnings             # exit 0
cargo clippy -p zgraph-kernel --all-targets --features test-hooks -- -D warnings   # exit 0
cargo test -p zgraph-kernel                                       # 78 passed / 0 failed（原 76 + 2）
cargo test -p zgraph-kernel --features test-hooks                 # 81 passed / 0 failed（原 79 + 2）
cargo test --workspace                                            # 214 passed / 0 failed（main 212 + 2）
bash scripts/dev/check.sh                                         # 13 项 OK → CHECK PASSED
/Users/zyzs/lczyzs/code/zgraph/.venv/bin/python -m pytest         # 20 passed in 0.57s
```

恢复 `run.rs` 后重跑两个新用例：各 `1 passed`（timeout 0.11s、failure 0.00s）。

### 4.3 独立探针（临时 `tests/zz_reviewer_probe.rs`，跑完已删除，未入库）

| 探针 | 构造 | 实际结果 |
|---|---|---|
| P1 超时回填 | `linear_spec_with_work_timeout(100)`、tick=10、executor delay 30s | wall=104ms、`Failed/NODE_TIMEOUT`、TimedOut 迁移 **from=Running**、snapshot error_code=`NODE_TIMEOUT`、snapshot elapsed=103 == NodeTimedOut elapsed=103、`abandoned_tasks=0`、`killed_workers=1`、NodeAbandoned=0 |
| P2 同步 panic | 自定义 `NodeExecutor` 在 `execute()` 内直接 panic（不返回 future） | **648µs**、`Failed/NODE_INTERNAL`、message=`…executor panicked: sync boom probe`、Crashed 快照 error_code=`NODE_INTERNAL`、`timed_out=0`、lifecycle 恰一次 |
| P3 Crashed 回归 | `FakeExecutor.fail(work, BusinessError)` | run error=`NODE_BUSINESS_ERROR`、Crashed 快照 error_code=`NODE_BUSINESS_ERROR`、error 非 None（helper 复用未破坏 Crashed 路径） |

### 4.4 测试强度反向验证（独立）

```bash
cp src/run.rs /tmp/rev011_run_fixed.rs
git checkout 4570a53 -- crates/zgraph-kernel/src/run.rs     # 临时还原修复
cargo test -p zgraph-kernel --test timeout -- timed_out_transition_snapshot_carries_timeout_error --exact
# FAILED（0.11s）：timeout.rs:103 assertion left: None, right: Some("NODE_TIMEOUT")
cargo test -p zgraph-kernel --test failure -- executor_sync_panic_normalizes_to_internal_and_fails_fast --exact
# FAILED（180.02s）：failure.rs:425/436（elapsed<5s 与 Internal 断言）
cp /tmp/rev011_run_fixed.rs src/run.rs
# sha256 与 HEAD:src/run.rs 一致；git status 空；随后两用例恢复为 1 passed
```

→ 两用例真回归；同时暴露 #1（sync panic 用例回归反馈需 180s）。

### 4.5 trace writer 落库链路核对

| 列 | 来源（`crates/zgraph-trace/src/writer/tx.rs`） | kernel 输入 |
|---|---|---|
| `instances.error` / `instances.error_code` | `upsert_instance(snapshot)`：`json_opt(snapshot.error)` → `error`、`snapshot.error_code` → `error_code`（`:655,:681`；ON CONFLICT 用 COALESCE 保留） | 迁移快照 `InstanceSnapshot.{error,error_code}`（本提交回填） |
| `transitions.error_code` / `transitions.error` | `write_transition(record, snapshot)`：`snapshot.error_code`、`json_opt(snapshot.error)`（`:383,:398`） | 同上（同一迁移快照） |

结论：kernel 侧“迁移前回填”是必要且充分输入；端到端 SQL 非 NULL 还要求 CLI 桥接把 `NodeError::Timeout` 映射为 `{"kind"/"elapsed_ms"}` JSON 形状（`NodeError` 无 serde；见 #2，REV-009 验证）。

### 4.6 D1 与回归核对

- 超时仍 ≤5s 失败（探针 104ms；原 `node_timeout_kills_and_fails_run_quickly` 仍绿）；`abandoned_tasks=0`；`killed_workers≥1`；`NodeAbandoned` 不产生；`RunFinished` 仍 `NODE_TIMEOUT`/Failed；`TimedOut` 迁移 from=Running。
- 同步 panic 捕获不改变取消/超时：超时路径实跑 30s executor + 100ms timeout → 102ms TimedOut/killed=1/abandoned=0；`control` 协作取消用例仍绿（78 全绿）。
- 无 `unsafe`（`#![forbid(unsafe_code)]` 保留）；payload 未吞（P2 message 含 panic 文本；非字符串 payload 退化为 generic 文案，属已知设计）。

## 5. 遗留问题

- **无阻塞项**；本分支可作为 kernel 修复合并（基于 `origin/main=4570a53`，可 ff）。
- **建议级**：#1 sync panic 用例的回归反馈时间（改用短节点超时，~200ms 失败）；#2 端到端 AC-06 的 SQL 级断言与 `NodeError→JSON` 映射随 REV-009（cli）验证；#3 commit message 测试段小缺口（无需修订）；#4 `doc/progress.md`/`doc/issues.md` 由 Lead 合并时更新。
- 未 push/merge；main 未被本分支改动。REV-007 §7 的“同步 `execute()` panic 边界”建议在本分支已关闭（探针 P2 + 新用例）。
- 待办衔接：REV-009（cli，`feat/cli-m2-trace`）需补 AC-06 实例级断言后再开评；REV-010（docs）待命。

## 6. 条件关闭记录

不适用：本记录结论为 **通过**，无前置条件。
