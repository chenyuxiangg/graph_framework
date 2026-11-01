# REV-095 · process/docs · v0.2.0 发布收口（版本/性能基线/说明/进度）

> **跨模块声明（Lead 已批准，第二评审由 Lead 追加）**：本分支主体为 process/docs（`Cargo.toml`/`Cargo.lock`/`README.md`/`doc/**`）；含 **1 行 cli 测试期望机械改动** `crates/zgraph-cli/tests/validate.rs`：`zgraph 0.1.0` → `env!("CARGO_PKG_VERSION")`（零逻辑、版本自动跟随；Lead 已批准）。无其它产品代码/逻辑/测试语义改动。

- **分支**：`chore/process-v02-release`
- **作者**：`docs-dev`（单提交 `7b553c7`，基于 main `2340f4d`；正文含 `Review: REV-095` / `Agent: docs-dev`）
- **评审人**：`reviewer-b`（独立评审，≠ 作者）
- **变更范围**：`2340f4d..7b553c7`（9 files，+69/−22）：`Cargo.toml`（workspace version）、`Cargo.lock`（7 个本地 crate version）、`README.md`、`crates/zgraph-cli/tests/validate.rs`（1 行）、`doc/perf-baseline-v0.2.txt`（新增）、DR-004 §10、`doc/index.md`、`doc/issues.md`、`doc/progress.md`
- **评审时间**：2026-11-01
- **关联**：task-52（作者任务）、DR-004 §10、08 §8.10/§8.11、`dist/RELEASE_NOTES-v0.1.0.md` 对照

## 1. 结论

**结论：通过**

P0=0；**P1=0**；P2=1；P3=4（均文档/台账建议，不阻断发布）。版本 bump、release 构建/`--version`、性能基线数字与门禁、README/进度/DR-004 发布记录均与 Lead 口径一致；workspace 915/0/3、e2e 106/0/2、pytest 170、check.sh PASSED 亲自复跑通过；无 dist/密钥改动。合并后可按计划构建 dist → tag `v0.2.0` → GitHub Release。

## 2. 四维评审

| 维度 | 结论 | 依据 |
|---|---|---|
| 功能正确性/发布正确性 | ✅ | workspace `version = "0.2.0"`；Cargo.lock 仅 7 个本地 crate version 同步、无依赖漂移；release `--locked` 构建成功；`./target/release/zgraph --version` = `zgraph 0.2.0 (DSL 1.0)`；perf baseline 数字/百分比/图标 hash/门禁均与 Lead 实测一致 |
| 结构合理性 | ✅ | 版本/锁文件/README/perf/DR-004/index/issues/progress 分层清晰；perf baseline 独立文件；DR-004 §10 只追加；进度四块 ≤15 行；范围仅 process/docs + 1 行 cli 测试 |
| 逻辑性 | ✅ | 版本 bump→lock→release 构建→`--version` 闭环；perf 方法（release 二进制、3 次中位、首轮预热剔除、fake LLM/hash）与门禁（≤20%、full-60 运行期例外引用 ISSUE-112）自洽；兼容矩阵/已知限制与 DR-004/04xx 一致 |
| 可读性 | ✅（P2/P3 备注） | README 能力/计数/产物/限制清晰；perf 文件可复核；P2/P3 为 ISSUE-150 台账同步、perf 措辞、index 待评审状态等小项 |

## 3. 发现清单

> P0/P1=0；P2/P3 均建议，不阻断发布。

### P2 · 建议

| # | 位置 | 问题 | 建议 |
|---|---|---|---|
| N1 | `README.md`（已知限制/下一步：ISSUE-143–149）、`doc/progress.md`（下一步/台账：143–149） | 同批新增 **ISSUE-150**（发布复跑观察到 `ac17a` 高负载下偶发 flake）已在 `doc/issues.md` 登记，但 README「P3 清理项」与 progress「清理/台账 143–149」未包含 150；台账状态行因此不完整（ISSUE-150 为 open P3） | README/progress 的清理清单改为 **143–150**（或注明「另见 ISSUE-150 发布 flake 观察」）；不改变发布结论 |

### P3 · 台账/阅读性建议

| # | 位置 | 问题 | 建议 |
|---|---|---|---|
| N2 | `doc/perf-baseline-v0.2.txt` §5 | 「三项可测回退（customer_ticket / 10 并发 / smoke）均 ≤20%」：smoke 实际为 **-14.8%（改善）**，不是回退；门禁口径本身正确 | 改为「三项可测指标均满足门禁（+17.3% / +16.0% / -14.8%）」或注明 smoke 为改善 |
| N3 | `doc/index.md` reviews 行 | 仍写 `REV-095 …（本分支待评审）`；评审通过后应由合并/索引更新为「通过」 | merge 后更新为 REV-095 通过（本评审已补 reviews README 行） |
| N4 | `README.md` / `doc/progress.md` 发布状态 | 在 tag/dist 尚未实际创建前写「已发布/tag v0.2.0/dist 产物」属发布收口流程的预写（task-52 明确要求）；合并后 Lead 立即构建/tag 才能闭环 | 保持；Lead merge 后确认 dist/tag 实际创建并回写（若发布流程延后则 progress 需回退措辞） |
| N5 | `README.md` 测试行 | `pytest 170 passed` 与本次实测一致（170 个通过；`pytest -q` 输出无 summary 行，按 dots/collect-only 计数确认） | 可保留；W4b-docs/发布记录若需证据可注明「170 passed（exit 0，collect-only 170）」 |

## 4. 验证证据

worktree `/Users/zyzs/lczyzs/code/zgraph/.worktrees/process-docs-dev`，HEAD `7b553c7`，base `main` `2340f4d`，工作区干净；`CARGO_HOME=/Users/zyzs/lczyzs/code/zgraph/.cargo-home`，未设 `CARGO_TARGET_DIR`，`PATH` 含 `.venv/bin`：

```bash
cargo test --workspace                    # 72 targets：915 passed / 0 failed / 3 ignored
cargo test -p zgraph-e2e                  # 14 targets：106 passed / 0 failed / 2 ignored
pytest -q                                 # exit 0；170 个通过（dots 170；collect-only 170）
cargo build --release -p zgraph-cli --bin zgraph --locked  # 成功（release）
./target/release/zgraph --version         # zgraph 0.2.0 (DSL 1.0)
bash scripts/dev/check.sh                 # PASSED
```

- **计数与作者自报一致**：workspace 915/0/3、e2e 106/0/2（dynamic_fanout 21）、pytest 170 passed；release build `--locked`/`--version` 通过；check.sh PASSED；无 ISSUE-091/150 复现（本次一次全绿）。
- **版本/锁**：`Cargo.toml [workspace.package] version = "0.2.0"`；`Cargo.lock` 仅 `zgraph-cli/dsl/e2e/harness/kernel/trace/worker` 7 个本地包 version 0.2.0，无第三方依赖版本/依赖列表漂移；release 构建 `--locked` 成功。
- **perf baseline 核对**（与 Lead 数字/算术一致）：
  - customer_ticket 88/75 ms = **+17.3%**（图 hash `ec320e4244b365ab`，release CLI validate 实测一致）；
  - 10 并发 217.0/187.1 ms = **+16.0%**（首轮 324.7 预热已剔除）；
  - SOP smoke 115/135 ms = **-14.8%**（hash `df9341c04063b80a`，e2e `software_team_v1` 冻结断言）；
  - full-60 validate 7.2/7.7 ms（hash `3d8d735b1d5e8a1b`，`software_team_v1` 冻结断言），运行期按 ISSUE-112 例外；
  - 三项可测指标均 ≤20% 门禁。
- **范围/红线**：`git diff --name-only 2340f4d..7b553c7` 9 files（见上）；`dist/` 零改动；全 diff 无 token/密钥字样；无既有评审记录/代码逻辑/测试基线改动；`git status` 空。

## 5. 跨模块 cli 1 行

| 文件 | 改动 | 核对 |
|---|---|---|
| `crates/zgraph-cli/tests/validate.rs` | `zgraph 0.1.0` → `env!("CARGO_PKG_VERSION")`（保留 DSL 1.0 断言） | 仅测试期望改动，零逻辑；版本自动跟随 workspace 0.2.0；Lead 已批准，第二评审由 Lead 追加 |

- 其它 8 files 均 process/docs；无产品逻辑/测试语义改动；未碰 `dist/**`；无密钥/token。

## 6. 遗留问题

- **发布后续（Lead）**：merge → 构建 `dist/zgraph-v0.2.0-*.tar.gz`+sha256/`RELEASE_NOTES-v0.2.0.md` → tag `v0.2.0` → GitHub Release `chenyuxiangg/graph_framework`；完成后更新 index/progress 的「待评审/已发布」实际状态（N3/N4）。
- **P2/P3**：N1 README/progress 清理清单补 ISSUE-150；N2 perf 措辞；N3 index REV-095 状态；N4 发布预写状态闭环；N5 pytest 证据措辞。均不阻断发布。
- 无 P0/P1；无附加条件；本分支可合并。

## 7. 条件关闭记录（仅「有条件通过」填写）

不适用（结论为 `通过`，无附加条件）。
