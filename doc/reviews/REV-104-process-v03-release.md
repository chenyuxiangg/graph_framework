# REV-104 · process/docs · v0.3.0 发布收口（版本 / 性能基线 / 说明 / 进度）

> 发布收口评审：仅版本/文档/基线改动，**未改代码/测试/既有评审记录**、**未触碰 `dist/`**、无 token/密钥。评审人只新增本评审记录，并按 task-72「REV-104 待评审→通过后由评审人/Lead 更新」更新了 `doc/index.md` 的 REV-104 状态与 `doc/reviews/README.md` 索引行。

- **分支**：`chore/process-v03-release`
- 分支: chore/process-v03-release
- **作者**：`docs-dev`（提交 `c7af1f3`；正文 `Review: REV-104` / `Agent: docs-dev`）
- **评审人**：`reviewer-a`（独立评审，≠ 作者）
- **基于**：main `2a24040`（`main = origin/main = 2a24040`）
- **变更范围**：`2a24040..c7af1f3`（**8 files, +59/−16**）：`Cargo.toml`（workspace version 0.3.0）、`Cargo.lock`（仅 7 本地 crate 版本）、`README.md`、`doc/decisions/DR-004-v02-scope-and-waves.md`（§10 只追加）、`doc/index.md`、`doc/issues.md`（ISSUE-164）、新增 `doc/perf-baseline-v0.3.txt`、`doc/progress.md`
- **评审时间**：2026-11-02
- **关联**：v0.3.0 发布（用户已授权 tag + GitHub Release）；task-72/73；REV-101（V5a 计数）、REV-103（cancel flake fix）、ISSUE-091/112/163/164

## 1. 结论

**结论：通过**

P0=0、P1=0、P2=0；P3 建议 4 项（§3，非阻塞）。版本 bump/锁/ release `--locked`/`--version`、性能基线与 Lead 实测逐项一致且门禁 ≤20% 通过、README/progress/DR-004/index/issues 内容与最终计数一致、范围与红线均符合。已知 ISSUE-091 时序 flake 在评审首轮 workspace 中复现一次（`kernel_cancel_kills_worker_process_group` pid 文件超时），隔离 ×3 + `executor_m3` 全 target + workspace ×2 均绿，**非本分支引入、无新增失败**；发布门禁若复现按既有口径重跑即可（§3 P3-3）。

## 2. 四维评审

| 维度 | 结论 | 依据 |
|---|---|---|
| 功能正确性 | ✅ | ①版本：`Cargo.toml [workspace.package] version="0.3.0"`；`Cargo.lock` 仅 `zgraph-{cli,dsl,e2e,harness,kernel,trace,worker}` 7 个本地 crate 0.3.0，无依赖漂移；`cargo build --release -p zgraph-cli --bin zgraph --locked` 成功；`./target/release/zgraph --version` = `zgraph 0.3.0 (DSL 1.0)`。②`doc/perf-baseline-v0.3.txt` 与 Lead 实测逐项一致：AC-01 75/75ms **+0.0%**、AC-12 212.2/221.0ms **−4.0%**、SOP smoke 102/110ms **−7.3%**、full-60 validate 6.9/6.0ms **+15%**（≤20% 门禁）、运行期未测为 ISSUE-112 发布例外；环境（CARGO_HOME/.venv/`--python-path python`）与图 hash（`ec320e...`/`df9341...`/`3d8d73...`）齐备；门禁结论「通过」。③README 最终计数实测一致：harness **235/0/1**、workspace **1037/0/3**、e2e **137/0/2**、pytest **183**、V5a **31**；progress 14 行四块；DR-004 §10 只追加；index/issues 合理（ISSUE-164 发布例外） |
| 代码结构合理性 | ✅ | 发布分支职责单一：版本+锁+基线+README/progress/DR 记录；不改代码/测试/既有评审；产物与 tag 明确写「由 Lead/发布流程」；index 新增 perf 条目并更新 reviews 至 REV-104；无重复真源（计数引用测试实测） |
| 逻辑性 | ✅ | 版本三处一致（workspace/Cargo.lock/`--version`）；perf 数字与百分比/门禁自洽（6.9/6.0=+15% 且 ≤20%）；发布状态「已发布」与 progress「tag 发布流程执行中」符合 task-72 约束（不提前宣称 tag 成功）；ISSUE-164 把 full-60 运行期例外与真实 Codex/DSH 未验证纳入台账；无 token/dist 副作用 |
| 可读性 | ✅ | perf 基线含日期/二进制/方法/环境/每项 3 次原始值/中位数/变化/图 hash/门禁结论；README 当前状态/能力/计数/已知限制/下一步清晰；progress ≤30 行且四块；DR-004 §10 记录分支/提交/产物/兼容矩阵/下一步 |

## 3. 发现清单（P3 建议，非阻塞）

| # | 级别 | 位置 | 问题 | 建议 |
|---|---|---|---|---|
| P3-1 | 建议 | `README.md:23` | 已知限制写「P2/P3 台账见 ISSUE-156–163」，遗漏本次新增 ISSUE-164（full-60 运行期/真实 Codex/DSH 发布例外） | 改为 `ISSUE-156–164`（或直接指向 issues 全文） |
| P3-2 | 建议 | `README.md:20` | 「`cargo test -p zgraph-e2e` **137/0/2**（V5a 四 target 连跑 3 次）」括号易被读成 137 来自 V5a 四 target（实际 V5a 四 target 合计 31，137 为全量 e2e） | 括号改为「全量 e2e；V5a 四 target 31 用例连跑 3 次」 |
| P3-3 | 建议（已知 flake） | `crates/zgraph-worker/tests/common/mod.rs:134` | 评审首轮 `cargo test --workspace`：**1018 passed / 1 failed**，`executor_m3::kernel_cancel_kills_worker_process_group` panic `pid file ... not written in time`（5s 等待窗）。属既有 **ISSUE-091**（时序测试在外部 CPU 争用下 flaky，2026-10-31 曾两次 blocked 重跑通过），**非本分支引入**；隔离该测试 ×3 + `executor_m3` 全 target 16/16 + workspace 复跑 ×2 均 **1037/0/3** | 发布 merge 时若门禁命中同一 flake，按既有口径重跑；中期按 ISSUE-091 评估扩大等待窗/事件驱动断言；本发布不阻塞 |
| P3-4 | 已处理（评审元数据） | `doc/index.md:30`、`doc/reviews/README.md` | 作者提交时 index 写「REV-104 本分支评审中」、reviews README 尚无 REV-104 行 | 评审人已在本记录提交中更新：index 改「通过」、README 增加 REV-104 行（不改实现/测试） |

## 4. 验证证据

评审人亲自执行（worktree `.worktrees/process-docs-dev`，HEAD `c7af1f3`，main `2a24040`，worktree 干净；`CARGO_HOME=<repo>/.cargo-home`）：

```bash
# 版本/构建/运行
grep '^version' Cargo.toml                     # [workspace.package] version = "0.3.0"
git diff main...HEAD -- Cargo.lock             # 仅 7 个本地 crate version 0.2.0→0.3.0，无依赖漂移
cargo build --release -p zgraph-cli --bin zgraph --locked   # Finished release（成功）
./target/release/zgraph --version              # zgraph 0.3.0 (DSL 1.0)

# 测试（含已知 flake 复跑）
cargo test --workspace
# 首轮：1018 passed / 1 failed / 3 ignored —— 失败为 known ISSUE-091
#   executor_m3::kernel_cancel_kills_worker_process_group（pid file not written in time）
cargo test -p zgraph-worker --test executor_m3 kernel_cancel_kills_worker_process_group  # ×3 全 1 passed/0 failed
cargo test -p zgraph-worker --test executor_m3          # 16 passed / 0 failed
cargo test --workspace                                  # run1 1037/0/3
cargo test --workspace                                  # run2 1037/0/3
cargo test -p zgraph-harness                            # 235 passed / 0 failed / 1 ignored
cargo test -p zgraph-e2e                                # 137 passed / 0 failed / 2 ignored
python -m pytest                                        # 183 passed

# 门禁
bash scripts/dev/check.sh                               # CHECK PASSED
```

- **perf-baseline-v0.3.txt 实读**：AC-01 75/75 `+0.0%`；AC-12 212.2/221.0 `−4.0%`；SOP smoke 102/110 `−7.3%`；full-60 validate 6.9/6.0 `+15%`；门禁 ≤20% **通过**；运行期 ISSUE-112 发布例外；环境/CARGO_HOME/.venv/`--python-path python` 与三个图 hash 均在；与 Lead 实测数字逐项一致。
- **范围/红线核对**：`git diff --name-only main...HEAD` = 8 files（2 个 Cargo + 6 个 doc），**无 `crates/`/`tests/`/`doc/reviews/`/`dist/` 改动**；新增仅 `doc/perf-baseline-v0.3.txt`；`grep` 无 token/密钥；spec/PROVENANCE 未改（行数不重算）；`doc/progress.md` 14 行（≤30，四块）；DR-004 §10 仅追加 2 行；`doc/index.md` perf 条目与 reviews 至 REV-104；issues 新增 ISSUE-164 且 156–163 状态保留。
- **与自报对比**：版本/`--version`、perf 数字、README 计数（235/1037/137/183/31）、workspace 复跑 1037/0/3 均一致；唯一差异是首轮 workspace 命中 known ISSUE-091（自报 ×3 绿；评审复跑 ×2 绿）。
- **发布前置状态**：tag `v0.3.0`/`dist/RELEASE_NOTES-v0.3.0.md`/tar+sha 尚未创建（按 task-72 由 Lead merge 后构建，未提前宣称成功）；本记录通过后满足 merge.sh 前置（结论=通过、worktree 干净、基于 origin/main）。

## 5. 遗留问题

- **ISSUE-091（P3，既有）**：时序测试在外部 CPU 争用下偶发（本次 `kernel_cancel` pid 超时）；发布门禁命中请重跑，中期按台账改进（非 REV-104 引入，不阻塞）。
- **ISSUE-112 / ISSUE-164（P2/P3 发布例外）**：SOP full-60 运行期未测、真实 Codex/DSH 环境未验证；release notes 必须标注（README/progress/DR-004 已写）。
- **P3 文档建议**（§3 P3-1/P3-2）：README 已知限制补 ISSUE-164、e2e 计数括号消歧；可在发布后 docs 小修。
- **发布后续**（Lead）：merge REV-104 → 构建 `dist/zgraph-v0.3.0-*.tar.gz` + sha256 + `RELEASE_NOTES-v0.3.0.md` → tag `v0.3.0` → GitHub Release `chenyuxiangg/graph_framework`；本评审不涉及。
