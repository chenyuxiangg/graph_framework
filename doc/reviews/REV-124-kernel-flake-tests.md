# REV-124 · kernel/worker · v0.5 F4a flake 加固（kernel_cancel / worker timeout）

> **跨模块声明**：分支仅改 `crates/zgraph-worker/tests/**`（4 个测试/fixture 文件），无产品代码；worker tests 跨模块 Lead 已批准、第二评审 Lead 追加。评审人只新增本评审记录，未改实现/测试。

- **分支**：`fix/kernel-flake-tests`
- 分支: fix/kernel-flake-tests
- **作者**：`kernel-dev`（commit `e9706b0`；正文 `Review: REV-124` / `Agent: kernel-dev`）
- **评审人**：`reviewer-a`（独立评审，≠ 作者）
- **基于**：main `d38dae4`（`main = origin/main = d38dae4`）
- **变更范围**：`d38dae4..e9706b0`（**4 files, +150/−25**）：`crates/zgraph-worker/tests/{common/mod.rs,echo_roundtrip.rs,executor_m3.rs,fixtures/py/wfaults.py}`；**无 `crates/zgraph-worker/src`/kernel 产品代码**
- **评审时间**：2026-11-03
- **关联**：task-113/115；ISSUE-176/177/181；REV-123/125（F3a1/F4b 并行）

## 1. 结论

**结论：通过**

P0=0、P1=0、P2=0；P3 4 项（复现证据仅 commit message、重复等待 helper、ISSUE-176 台账仍 open 待 D6、2s 预算余量说明）不阻塞。根因/修复/回归/隔离/门禁/范围成立。

### 1.1 已核对成立

| 项 | 结论 |
|---|---|
| 复现/根因 | ✅ commit 正文记录慢启动 wrapper 下 3 类失败（cancel `worker.pid not created`、hello `probe pid NotFound`、worker `worker pid NotFound`）；根因均为测试同步/超时预算，非产品竞态；**产品代码 diff 为空**（`crates/zgraph-worker/src`/`crates/zgraph-kernel/src` 无改动） |
| 加固不弱化断言 | ✅ `SYNC_TIMEOUT=20s`/`PID_GONE_TIMEOUT=15s` 有界轮询；`wait_for_pid_file` 要求**非空**内容（防 exists 半写）；失败附 pid 文件/`ps` 诊断；timeout/handshake 预算 600ms/300ms→**2s** 仍 ≪ fixture 5s sleep，超时 kill/reap 语义保留；`assert_pid_reaped` 仍断言最终 pid 消失 |
| 确定性回归 | ✅ 新增 `kernel_cancel_waits_for_delayed_child_pid_file`：fixture 孙进程 pid 延迟 **5.5s**（>旧 5s 预算）仍等待成功、取消/进程组回收断言完整；`ZGRAPH_FIXTURE_CHILD_DELAY_MS` 默认 0，不影响既有/产品 |
| 测试强度/门禁 | ✅ 重点 cancel/hello/worker 各隔离 **×10 全绿**；worker **71/0**、kernel **150/0**、workspace **1255/0/3**、clippy/fmt/check.sh 绿；workspace 本机一次通过（作者 RUN2 `ac09` 属 ISSUE-177，非本分支） |
| 范围/跨模块 | ✅ 仅 4 个 worker tests/fixture；无产品代码；worker 跨模块 Lead 批准；无 token/dist；check.sh PASSED |

## 2. 四维评审

| 维度 | 结论 | 依据 |
|---|---|---|
| 功能正确性 | ✅ | 测试失败根因/修复/回归/隔离成立；产品超时/取消语义未改；断言未放松为跳过/重试 |
| 代码结构合理性 | ✅ | 公共等待/诊断集中在 `common/mod.rs`；worker 超时测试复用预算 helper；fixture 延迟为测试专用可选 env |
| 逻辑性 | ✅ | 多类失败均为同步预算不足；20s/15s 有界、2s 预算与 5s sleep 关系清楚；非空 pid 等待解决半写 |
| 可读性 | ✅（P3） | commit 复现/修复/证据结构清晰；P3 为证据落盘/helper 去重/台账更新 |

## 3. 发现清单

### 3.1 P0/P1/P2

无。

### 3.2 P3（建议，不阻塞）

- **P3-1 复现证据未入库**：慢启动 wrapper/修复前 3 类失败日志仅在 commit message，无脚本/日志文件；建议 D6/F4 收口时把复现 wrapper/日志落到 `doc/` 或 issue 附件，便于复审。测试修复本身已由回归/隔离证明。
- **P3-2 helper 重复**：`echo_roundtrip::read_pid_with_budget` 与 `common::read_pid` 重复，且硬编码 20s；建议复用 `common::SYNC_TIMEOUT`/`read_pid`（不阻塞）。
- **P3-3 ISSUE 台账**：本分支未改 docs；`ISSUE-176` 在台账仍 open，D6 应置 **fixed（REV-124）**；`ISSUE-177`（ac09）→ F4b/REV-125；`ISSUE-178`（Codex e2e flake）→ F4b；`ISSUE-179/180/163/164` 仍 open；`ISSUE-156–175` 由 ISSUE-181/D6 逐项复核。详见 §7 清单。
- **P3-4 2s 预算余量**：timeout/handshake 2s 相对 5s fixture 仍有 2.5x 余量；极慢 CI 仍可能接近边界，建议 F4/F6 观察并保留诊断（已附 ps/文件诊断）。

## 4. 验证证据

评审人亲自执行（worktree `.worktrees/kernel-kernel-dev`，HEAD `e9706b0`，main `d38dae4`，worktree 干净；`CARGO_HOME=<repo>/.cargo-home`）：

```bash
# 重点测试隔离 ×10（我跑 3 类各 10 次）
for i in $(seq 1 10); do cargo test -p zgraph-worker --test executor_m3 \
  kernel_cancel_kills_worker_process_group -- --exact; done          # 10/10 pass
for i in $(seq 1 10); do cargo test -p zgraph-worker --test echo_roundtrip \
  hello_timeout_kills_and_reaps_process -- --exact; done             # 10/10 pass
for i in $(seq 1 10); do cargo test -p zgraph-worker --test echo_roundtrip \
  worker_timeout_kills_and_reaps_process -- --exact; done            # 10/10 pass
for i in $(seq 1 10); do cargo test -p zgraph-worker --test executor_m3 \
  kernel_cancel_waits_for_delayed_child_pid_file -- --exact; done    # 10/10 pass（5.5s delay）
cargo test -p zgraph-worker    # 71 passed / 0 failed
cargo test -p zgraph-kernel    # 150 passed / 0 failed
cargo test --workspace         # 1255 passed / 0 failed / 3 ignored
cargo clippy --workspace --all-targets --locked -- -D warnings  # clean
cargo fmt --all -- --check     # exit 0
bash scripts/dev/check.sh      # CHECK PASSED
```

- **复现证据**：commit 正文三类失败 panic 文本/慢启动 wrapper 场景；本次复核未重跑修复前代码（分支不含旧版本），但diff为空的产品代码 + 新回归/隔离支持「测试时序根因」。
- **范围**：`git diff --name-only` 4 个测试/fixture；无产品代码；无 token/dist。
- **自报对比**：worker 71、kernel 150、workspace 1255、重点 ×10、clippy/fmt/check 与自报一致。

## 5. 条件关闭方式

无（结论通过；P3 见 §3/§7，D6 台账更新）。

## 6. 条件关闭记录（"通过"）

无开放条件。

## 7. ISSUE 清单（供 D6；本分支不写 docs）

| ISSUE | 建议状态 | 依据 |
|---|---|---|
| ISSUE-176 kernel_cancel 时序 | **fixed（REV-124 / e9706b0）** | 20s 有界同步 + 非空 pid + 5.5s 延迟回归 + 隔离 ×10；无产品改动 |
| ISSUE-177 ac09 并行 timing | 仍 open → **F4b/REV-125** | 作者 RUN2 命中，隔离 ×3 过；非本分支 |
| ISSUE-178 Codex adapter e2e flake | 仍 open → **F4b/REV-125** | e2e 隔离/超时预算 |
| ISSUE-179 tool_calls 无 FK | 仍 open | v0.5 评估清理语义（F4 后续） |
| ISSUE-180 bash 真实隔离用户终端 | 仍 open | 发布前用户终端证据 |
| ISSUE-163/164（harness plugin/process release） | 仍 open | v0.3 残余，D6/ISSUE-181 复核 |
| ISSUE-156–175 | 待 ISSUE-181 逐项复核 | D6 清单；本分支只固定 kernel/worker 176 |

## 8. 遗留问题

- P3-1–P3-4 可 D6/F4 收口；不影响 merge。
- 产品无改动；F4b（REV-125）处理 177/178；F4a 可 merge。
