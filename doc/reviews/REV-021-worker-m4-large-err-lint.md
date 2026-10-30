# REV-021 · worker · C1 clippy 回归修复（WorkerFailure::Worker 装箱）

> **模块声明**：本分支只改 `crates/zgraph-worker/**`（`src/spawn.rs` 枚举变体 + 注释、`src/supervisor.rs` 构造处），符合 AGENTS §2 `worker` 模块边界。作者 `lead`，评审人 `reviewer-b`（**非作者**）。本记录是 REV-020-C1 的专项修复评审；**未修改任何代码/配置/台账**，只新增本记录；未 commit / 未 merge / 未 rebase。
>
> **背景**：REV-020 发现「本分支引入 `genai` → 其无条件启用 `serde_json/preserve_order` → 工作区统一构建 `WorkerError` 144 B → `clippy::result_large_err` 14 处（CI 会红）」。Lead 选择**装箱**（而非 `#[allow]` 抑制）消除 lint，本评审独立核对其正确性、语义不变性与组合干净 target 的修复效果。

- **分支**：`fix/worker-m4-large-err-lint`
- **作者**：`lead`
- **评审人**：`reviewer-b`
- **变更范围**：`838e969..94e1f8a`（1 commit `94e1f8a fix(worker): 装箱 WorkerFailure::Worker 消除 result_large_err 回归`；`git diff --shortstat` = **2 files changed, 7 insertions(+), 2 deletions(-)**；base = merge-base = `origin/main` = `838e969`，**未 rebase**）
- **评审时间**：2026-10-30
- **关联**：REV-020（C1 阻塞项）；REV-012（worker 分包/错误映射矩阵）；`doc/architecture-v2.md` §2.2（CI clippy -D warnings）/§6.1 M4；`.github/workflows/ci.yml:20`；spec 00 §0.4（`NodeError::NodeWorker` 映射不变）；ISSUE-073（根因/长期方案，待 Lead 登记）

## 1. 结论

**结论：通过**

改动与描述**逐字一致、最小且精准**：`WorkerFailure::Worker(WorkerError)` → `Worker(Box<WorkerError>)`（`spawn.rs:67`），唯一构造点 `supervisor.rs:395` 改为 `Box::new(error)`，并加注释解释根因（genai→serde_json/preserve_order→144 B→result_large_err）与"只改内存布局、不改协议"的边界。**独立组合复验**：以 harness 最终分支 `1fcbc1d` + 本补丁构造干净树（`/tmp/m4-verify`，`CARGO_TARGET_DIR` 干净），`cargo clippy --workspace --all-targets -- -D warnings` **exit 0（Finished）**；在同一棵树上**反向回退**本补丁后复跑 → **exit 101 / 14 errors（全在 worker）**，再次确认 lint 回归确由该变体尺寸引起、装箱即消除。`cargo test -p zgraph-worker` **58 passed / 0 failed**（含 `echo_roundtrip.rs:102` 对 `Worker(w)` 的按值匹配断言），`check.sh` PASS，worktree clean，提交元数据合规。

无阻塞/重要项；1 条台账跟进 + 3 条备注见 §3，均不影响合并。

## 2. 四维评审

| 维度 | 结论 | 依据（文件/行/测试/命令） |
|---|---|---|
| 功能正确性 | ✅ | ① 装箱只改内存布局：`WorkerFailure` 的枚举名/`Display`/错误映射不变；`#[error("worker error {0}")]` 经 `Box<T>: Display` 委托，展示文本不变。② 唯一构造点已同步 `Box::new`（`supervisor.rs:395`）；全仓 `WorkerFailure::Worker` 使用点仅 3 处（`supervisor.rs` 构造、worker `executor.rs:249 map_worker_error(&error)`、`tests/echo_roundtrip.rs:102 w.code`）+ harness `tools.rs:1069/1093` 两个 match，均通过 Deref/自动解引用编译（组合 clippy 编译通过即证明；harness `error.code/message/traceback` 字段访问正常）。③ 无 `impl From<WorkerError>` 需要同步。④ 行为回归：worker 全部 **58 passed / 0 failed**；harness 106 passed（C2 复审时复跑）在组合树上参与编译。 |
| 代码结构合理性 | ✅ | 仅动 2 个文件/7 行：枚举声明 + 唯一构造点，无协议、无错误映射、无测试逻辑改动；注释把「根因（依赖 feature 统一）→ 现象（144 B / lint）→ 处置（装箱）→ 边界（只改布局）」一次说清，便于后续（ISSUE-073 长期方案）追溯。选择 boxing 而非 `#[allow]` 抑制，避免了用 lint 豁免掩盖依赖带来的布局问题，结构上更干净。 |
| 逻辑性 | ✅ | 装箱后 `WorkerFailure` 最大变体不再是 144 B（Box 仅指针），`result_large_err` 自然消失，**无需 allow**；与 REV-020 指出的根因链完全对应（genai `Cargo.toml:453-455` 无条件 `features=["preserve_order"]`）。`Box<WorkerError>` 保持 `Send + Sync`（`WorkerError` 为纯数据），`WorkerFailure` 的 `Display`/错误上下文不受影响；无新增分支/竞态/资源语义变化。 |
| 可读性 | ✅ | 注释与 commit body 均写明"为什么装箱"（含 genai/preserve_order/144 B/CI 后果）、"不改协议/语义"边界与 ISSUE-073 引用；提交 subject 符合 `fix(worker): …`，正文含 `Review: REV-021` / `Agent: lead`；无魔术数/无 TODO 顶替台账。 |

## 3. 发现清单

| # | 级别 | 位置 | 问题 | 建议 | 状态 |
|---|---|---|---|---|---|
| 1 | 备注（台账跟进） | commit `94e1f8a` 正文与 `spawn.rs:62-66` 注释引用 **ISSUE-073** | 当前 `doc/issues.md` 最大编号为 **ISSUE-072**，**ISSUE-073 尚未登记**（本分支只改 worker、按模块边界未动台账；Lead 已在 REV-020-C3 说明会开台账）。若合并时漏登，代码/提交里的引用会断链（AGENTS §7 要求问题入台账） | Lead 在 M4 收尾（REV-024）或合并时补登 ISSUE-073：内容建议「genai 无条件开启 serde_json/preserve_order 的全局特征统一副作用：导致 worker 错误类型 144 B / clippy 回归（已由 REV-021 装箱规避）；长期方案：换 transport 或避免 preserve_order」。不阻塞本分支 | open（Lead） |
| 2 | 备注（公开 API） | `crates/zgraph-worker/src/spawn.rs:67`（`pub enum WorkerFailure` 公开变体类型变化） | `Worker(WorkerError)` → `Worker(Box<WorkerError>)` 对**仓外**按值匹配 `WorkerFailure::Worker(w)` 并期望 `w: WorkerError` 的消费者是 breaking。仓内 grep 仅 worker 自身 3 处 + harness 2 处（均已编译通过）；e2e 无直接匹配 | M4 预发布阶段可接受（仓内是唯一消费者，`WorkerRun` 等公开面未变）；若未来对外稳定 API，考虑 `#[non_exhaustive]`/文档注明（非本分支） | 无需处理 |
| 3 | 备注（长期方案） | 同上 | 本次是"布局层规避"，根因（genai→serde_json/preserve_order 全局副作用）仍在：所有 crate 的 `serde_json::Map` 语义/内存被改变 | 按 ISSUE-073 跟踪；REV-020 已核实 `graph_hash` 不受影响（`hash.rs:249-252` 显式排序）、workspace 418 测试全绿；docs REV-023 在 05 §5.13 记录依赖偏差 | open（ISSUE-073） |
| 4 | 备注 | `crates/zgraph-worker/src/spawn.rs:61-66` | 注释里"`WorkerError` 增至 144B"是 clippy 报的**下限值**（"at least 144 bytes"），精确 size 随 `serde_json::Value` 布局而定；不影响结论 | 无需改；若追求严谨可写"≥144 B（clippy 实测）" | 无需处理 |

> 无阻塞项、无重要项；#1 为台账跟进（Lead C3 清单内），#2–#4 为备注。

## 4. 验证证据

### 4.1 范围、分支与提交元数据

```bash
cd /Users/zyzs/lczyzs/code/zgraph/.worktrees/worker-lead
git status --short                        # 空（worktree clean）
git branch --show-current                 # fix/worker-m4-large-err-lint
git merge-base origin/main HEAD           # 838e9696b67b793d948720b7dbc8f7fe00795679（= base = origin/main，未 rebase）
git rev-parse HEAD                        # 94e1f8a66dbc7e5e51c311361a640eed3033d15c
git diff --shortstat 838e969..94e1f8a     # 2 files changed, 7 insertions(+), 2 deletions(-)
git diff --name-status 838e969..94e1f8a   # M crates/zgraph-worker/src/spawn.rs   M crates/zgraph-worker/src/supervisor.rs
git log -1 --format='%H|%an <%ae>|%cn <%ce>|%s' 94e1f8a
# chenyuxiangg <1007423165@qq.com> / chenyuxiangg <1007423165@qq.com> / fix(worker): 装箱 WorkerFailure::Worker 消除 result_large_err 回归
# 正文含 Review: REV-021 / Agent: lead
```

实际 diff（全文，共 2 hunk）：

```diff
-    Worker(WorkerError),
+    ///
+    /// 装箱：`genai` 无条件给 `serde_json` 打开 `preserve_order`（feature 统一），
+    /// 使 `serde_json::Value` 变大、`WorkerError` 增至 144B；直接内联会让
+    /// `Result<_, WorkerFailure>` 触发 workspace clippy `result_large_err`
+    /// （M4 引入 genai 后的 CI 回归，见 ISSUE-073）。装箱只改内存布局，不改协议。
+    Worker(Box<WorkerError>),
...
-            Some(status) if status.code() == Some(2) => Err(WorkerFailure::Worker(error)),
+            Some(status) if status.code() == Some(2) => Err(WorkerFailure::Worker(Box::new(error))),
```

### 4.2 必跑命令与结果（worktree 内，`CARGO_HOME=/Users/zyzs/lczyzs/code/zgraph/.cargo-home`）

| 命令 | 结果 |
|---|---|
| `cargo test -p zgraph-worker --quiet` | **exit 0：58 passed / 0 failed**（lib + supervisor_m3 + executor_m3 + echo_roundtrip 等全部二进制；日志 `/tmp/rev021-worker-test.log`） |
| `bash scripts/dev/check.sh` | **CHECK PASSED**（含 fmt / `git diff --check`；日志 `/tmp/rev021-check.log`） |
| `git status --short` | 空 |

### 4.3 组合干净 target clippy 独立复验（harness `1fcbc1d` + worker `94e1f8a`）

复现方式（不改任何仓库文件）：`git archive 1fcbc1d`（harness worktree）导出到 `/tmp/m4-verify`，再用 `git diff 838e969..94e1f8a` 应用 worker 补丁；`CARGO_TARGET_DIR=/tmp/m4-verify-target`（干净）执行 CI 同款命令：

```bash
# ① 修复后（当前组合）
cargo clippy --workspace --all-targets -- -D warnings
# → exit 0；Finished dev profile；0 error（/tmp/rev021-combo-clippy.log；日志含 Checking genai/zgraph-harness/zgraph-worker…）

# ② 同一棵树反向回退本补丁（仅 /tmp 副本）再跑 → 复现 REV-020 的 C1
patch -R 后：grep -n "Worker(WorkerError)" → 命中；cargo clippy … -D warnings
# → exit 101；14 处 `clippy::result_large_err`（全部 crates/zgraph-worker/src/{spawn,supervisor}.rs）；
#   "could not compile zgraph-worker (lib) due to 14 previous errors"（/tmp/rev021-combo-prefix.log）
# ③ 重新应用补丁 → 恢复 exit 0（组合树保持与待合并状态一致）
```

| 状态 | 命令 | 结果 |
|---|---|---|
| 修复后组合 | `cargo clippy --workspace --all-targets -- -D warnings` | **exit 0 / 0 error** |
| 回退补丁后同树 | 同上 | **exit 101 / 14 errors（全在 worker）** |
| 结论 | — | C1 回归确由 `WorkerFailure::Worker` 尺寸触发，装箱后消除；无需 `#[allow]` 抑制 |

### 4.4 使用点/语义核对

```bash
grep -rn "WorkerFailure::Worker" crates/ .worktrees/*/crates  # 仓内全部使用点：
# supervisor.rs:395 构造（已 Box::new）
# zgraph-worker/src/executor.rs:249  WorkerFailure::Worker(error) => map_worker_error(&error)   ← Box Deref 自动解引用
# zgraph-worker/tests/echo_roundtrip.rs:102  Worker(w) => assert_eq!(w.code, "NODE_WORKER_IMPORT") ← Box 字段访问
# harness tools.rs:1069/1093  match WorkerFailure::Worker(_)/(error) → failure.to_string()/error.{message,code,traceback} ← 编译通过
grep -n "impl From<WorkerError>" crates/zgraph-worker/src/spawn.rs   # 无（无需同步构造）
```

- `WorkerFailure` 不参与协议序列化（协议帧是 `WorkerError`，未改）；错误映射（`NODE_WORKER_*`）在 `executor.rs::map_failure/map_worker_error`，本次未触碰，语义不变。

## 5. 遗留问题

- **台账**：ISSUE-073 需由 Lead 登记（§3 #1；属 REV-020-C3 清单）。
- **长期方案**：serde_json/preserve_order 全局副作用按 ISSUE-073 跟踪（§3 #3；REV-020 §5 已评估 graph_hash 安全）。
- 本评审未修改任何代码/台账；REV-021 记录为本分支唯一新增文档。
