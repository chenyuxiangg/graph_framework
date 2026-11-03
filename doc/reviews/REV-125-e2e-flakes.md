# REV-125 · e2e · v0.5 F4b e2e flake 加固（parallel timing / codex adapter）

> **评审范围**：仅 `tests/e2e/{parallel,harness_v03}.rs`（2 文件，+157/−21）；无产品代码/文档/测试计数变化。评审人只新增本评审记录。

- **分支**：`test/e2e-flakes`
- 分支: test/e2e-flakes
- **作者**：`e2e-dev`（commit `8efe01d`；正文 `Review: REV-125` / `Agent: e2e-dev`）
- **评审人**：`reviewer-a`（独立评审，≠ 作者）
- **基于**：main `d38dae4`（`main = origin/main = d38dae4`）
- **变更范围**：`d38dae4..8efe01d`（**2 files, +157/−21**）：`tests/e2e/parallel.rs`（ISSUE-177）、`tests/e2e/harness_v03.rs`（ISSUE-178）
- **评审时间**：2026-11-03
- **关联**：task-116；ISSUE-177/178/181；REV-124（F4a）

## 1. 结论

**结论：通过**

P0=0、P1=0、P2=0；P3 3 项（历史失败日志仅 commit message、ISSUE 台账待 D6、best_ratio 仅诊断）不阻塞。加固保留阈值/断言语义、无产品代码、门禁全绿。

### 1.1 已核对成立

| 项 | 结论 |
|---|---|
| ISSUE-177 证据/分析 | ✅ commit 记录历史 5 次失败「并行 425–566ms vs 串行 501–596ms、比例 82–97% >80%」，绝对耗时均 <600ms；冷启动固定成本+共享负载单向加长分析合理 |
| ISSUE-177 加固 | ✅ 预热 1 对 + 交错 3 轮，取每轮最优（best-of-3）；阈值/语义不变：`parallel_min < 600ms`、`parallel_min < serial_min × 80%`、新增 `parallel_min < serial_min`；Running 重叠 + skew<50ms 原样保留；不新增放宽/不跳过，诊断输出全部样本 |
| ISSUE-178 证据/加固 | ✅ 历史 `NODE_TIMEOUT 30020/30028ms`；每用例独立 PATH（隔离 bin + 系统路径）、CODEX_HOME、TMPDIR；python3 符号链接确定性；同 binary 重子进程用例全局 Mutex 串行；`node_timeout 30s→120s` 仅环境预算，仍断言 Completed/四字段/无 token delta/`llm_calls=1`/进程回收；pid 等待/回收 15s 有界；未跳过/伪造 |
| 范围/计数 | ✅ 仅 2 个 e2e 文件；无 `crates/` 产品代码；无新增/删除用例，e2e 仍 **24 target / 173 passed / 0 failed / 2 ignored**；test-baseline 计数不变 |
| 门禁 | ✅ e2e 全量 **×3 均 173/0/2**；ac09 **×10 全绿**（~3.0–4.8s/次）；ac20h **×10 全绿**（~0.4–1.2s/次）；workspace **1254/0/3**；clippy/fmt/check.sh 绿 |

## 2. 四维评审

| 维度 | 结论 | 依据 |
|---|---|---|
| 功能正确性 | ✅ | 两类 flake 的证据/加固逻辑成立；阈值/断言未弱化；产品代码未改；无跳过/重试 |
| 代码结构合理性 | ✅ | parallel 采样 helper 独立；harness 串行锁/隔离 PATH 集中在测试内；无产品侵入 |
| 逻辑性 | ✅ | best-of-3 抗单边噪声与 80%/600ms 语义匹配；同 binary 串行 + 120s 环境预算与进程风暴因果一致 |
| 可读性 | ✅（P3） | 注释/诊断输出充分；P3 为证据落盘/台账更新 |

## 3. 发现清单

### 3.1 P0/P1/P2

无。

### 3.2 P3（建议，不阻塞）

- **P3-1 证据落盘**：历史 5 例 ratio/耗时、30s NODE_TIMEOUT 日志仅在 commit message；建议 D6 或 issue 附件保留原始日志（本分支测试逻辑已可复核）。
- **P3-2 ISSUE 台账**：177/178 建议 D6 置 fixed（REV-125）；176 归 F4a/REV-124；179/180/163/164 仍 open；156–175 由 ISSUE-181/D6 逐项复核。
- **P3-3 `best_ratio` 仅诊断**：test 计算 `best_ratio` 但断言用 `parallel_min/serial_min`；符合「min 阈值」语义，诊断字段可保留，不必改。
- **P3-4 运行时间**：ac09 从 ~1.4s 增至 ~3.2s（warmup+3 轮），任务已接受；若 CI 预算紧可后续调轮数（不得改阈值）。

## 4. 验证证据

评审人亲自执行（worktree `.worktrees/e2e-e2e-dev`，HEAD `8efe01d`，main `d38dae4`，worktree 干净；`CARGO_HOME=<repo>/.cargo-home`）：

```bash
git status --porcelain                 # 空
git diff --stat main...HEAD            # 2 files +157/−21（e2e-only）
bash scripts/dev/check.sh              # CHECK PASSED

for i in 1 2 3; do cargo test -p zgraph-e2e; done
# e2e x3: 173 passed / 0 failed / 2 ignored（一致）
for i in $(seq 1 10); do cargo test -p zgraph-e2e --test parallel \
  ac09_parallel_branches_overlap_and_total_elapsed_within_budget -- --exact; done  # 10/10
for i in $(seq 1 10); do cargo test -p zgraph-e2e --test harness_v03 \
  ac20h_codex_adapter_via_fake_binary_and_dsh_structure -- --exact; done            # 10/10
cargo test --workspace                # 1254 passed / 0 failed / 3 ignored
cargo clippy --workspace --all-targets --locked -- -D warnings  # clean
cargo fmt --all -- --check            # exit 0
```

- **范围**：仅 `tests/e2e/{parallel,harness_v03}.rs`；无 `crates/`/docs/既有评审改动；无 token/dist。
- **自报对比**：e2e 173/24/2×3、ac09/20h ×10、workspace 1254、clippy/fmt/check 与自报一致；test-baseline 未改。
- **断言抽查**：ISSUE-177 阈值 600ms/80%/skew50ms 原样；ISSUE-178 Completed/四字段/无 token delta/llm_calls=1/reap 均保留；无 `#[ignore]` 新增/无跳过。

## 5. 条件关闭方式

无（结论通过；P3 见 §3/§7）。

## 6. 条件关闭记录（"通过"）

无开放条件。

## 7. ISSUE 清单（供 D6；本分支不写 docs）

| ISSUE | 建议状态 | 依据 |
|---|---|---|
| ISSUE-177 ac09 parallel timing | **fixed（REV-125 / 8efe01d）** | 5 例 82–97% 证据；best-of-3 不变阈值；ac09 ×10 + e2e×3 全绿 |
| ISSUE-178 Codex adapter e2e flake | **fixed（REV-125 / 8efe01d）** | NODE_TIMEOUT 30s 证据；隔离 PATH/CODEX_HOME/TMPDIR + 串行 + 120s 环境预算；ac20h ×10 全绿；断言不弱 |
| ISSUE-176 kernel_cancel | F4a/REV-124 fixed 候选 | 见 REV-124 |
| ISSUE-179/180/163/164 | 仍 open | 见台账/D6 |
| ISSUE-156–175 | ISSUE-181/D6 逐项复核 | 本分支只处理 177/178 |

## 8. 遗留问题

- P3-1–P3-4 可 D6 收口；不阻塞 merge。
- 无产品代码/无计数变化；F4b 可 merge。
