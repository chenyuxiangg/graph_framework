# REV-103 · harness · codex cancel 测试竞态修复（REV-102 C1）

> 评审范围仅为 `#[cfg(test)]` 内测试函数修复：不改产品逻辑、不放宽断言。评审人只新增本评审记录，未改实现/测试。

- **分支**：`fix/harness-codex-cancel-race`
- 分支: fix/harness-codex-cancel-race
- **作者**：`dsl-dev`（提交 `050f86f`；正文 `Review: REV-103` / `Agent: dsl-dev`）
- **评审人**：`reviewer-a`（独立评审，≠ 作者）
- **基于**：main `2ff92c4`（`main = origin/main = 2ff92c4`）
- **变更范围**：`2ff92c4..050f86f`（**1 file, +25/−22**）：仅 `crates/zgraph-harness/src/plugin.rs` 文件内 `#[cfg(test)] mod tests`（`mod tests` 起于 `:1159`，被测函数 `:1881`）
- **评审时间**：2026-11-02
- **关联**：REV-102 C1（workspace 首跑偶发 `fake Codex pid file not written within 10s`）；REV-100 #4（有界等待）；task-69/70

## 1. 结论

**结论：通过**

P0=0、P1=0、P2=0；无 P3 需修复（仅 2 条非缺陷观察，§3）。修复精准命中根因：删除固定 600ms，canceller **有界轮询 pid 文件（≤10s，50ms）确认 fake Codex 已 spawn 后再 cancel**；超时/未写 pid 时先 `close_provider()` 清理再带诊断 panic；原断言（`Err(Cancelled)`/`live_adapter_count=0`/`wait_until_reaped`/场景 30s 超时）全部保留。评审人亲自隔离 ×5、workspace ×2 复跑无 race/hang/panic；无产品代码与其它文件改动。

## 2. 四维评审

| 维度 | 结论 | 依据 |
|---|---|---|
| 功能正确性 | ✅ | 根因是 cancel 早于 pid 写入；`plugin.rs:1905-1928` 改为 canceller 线程先有界等待 `pid_file` 可解析（≤`WAIT_TIMEOUT`=10s，50ms 轮询）→ `request_cancel()` → `pid_tx.send(pid)`；主线程 `join` 后取 `observed_pid`。场景总超时 30s；pid 缺失分支先 `close_provider()` 再 panic；`Err(Cancelled)`、`live_adapter_count=0`、`wait_until_reaped` 原断言逐字保留（`:1935-1959`）。隔离 ×5 全过（0.30–0.36s，冷启 0.56s）；workspace ×2 无 hang/panic |
| 代码结构合理性 | ✅ | 改动限于测试模块；删除已无用的 async `wait_for_pid`，改用 mpsc 回传 pid + 有界轮询；不引入产品 API/依赖；`git diff --name-only` 仅 1 文件 |
| 逻辑性 | ✅ | 同步顺序确定：pid 写入 → cancel → join → observed_pid → 断言/清理；超时/未写 pid/场景超时三条失败路径均有界且先清理；无死锁（canceller 只读文件 + 发信号 + channel send；主线程等 outcome/join）；即使 pid 超时也先 cancel 避免挂起 |
| 可读性 | ✅ | 注释说明 REV-102 C1 根因与「先 pid 后 cancel」；无常量变更（`WAIT_TIMEOUT`/`SCENARIO_TIMEOUT` 保留）；命名清晰；无魔术数；断言保留原诊断信息并新增 pid 路径提示 |

## 3. 发现清单

| # | 级别 | 位置 | 问题 | 建议 | 状态 |
|---|---|---|---|---|---|
| — | — | — | **无 P0/P1/P2/P3 缺陷**。以下 2 条为观察（非缺陷）：① pid 缺失时测试会在 ≤10s 后失败（有界，符合「不 hang」要求；并行 CI 失败延迟增加但可接受）；② `wait_until_reaped` 仍用 `nix::kill(pid,0)`（既有实现；PID 复用理论上可能提前判回收，非本修复引入、概率极低）。 | 无需修改；若后续追求更早失败可缩短 pid 等待上限，但会重新引入负载敏感，不建议。 | — |

## 4. 验证证据

评审人亲自执行（worktree `.worktrees/harness-dsl-dev`，HEAD `050f86f`，main `2ff92c4`，worktree 干净；`CARGO_HOME=<repo>/.cargo-home`）：

```bash
# 1) 隔离 ×5（同一测试 alone，--exact）
for i in 1..5: cargo test -p zgraph-harness --lib \
  plugin::tests::codex_in_flight_cancel_leaves_no_orphan_process_group -- --exact
# 结果：1 passed; 235 filtered out ×5；耗时 0.56s（冷启）/0.36/0.31/0.30/0.31s
#       （real 1.56/0.49/0.49/0.44/0.45s）——无 panic、无 hang、无 pid 超时

# 2) harness 双构建
cargo test -p zgraph-harness                 # 235 passed / 0 failed / 1 ignored
cargo test -p zgraph-harness --features mcp  # 235 passed / 0 failed / 1 ignored

# 3) workspace 连跑 ×2（并行负载）
cargo test --workspace  # run1: 1037 passed / 0 failed / 3 ignored
cargo test --workspace  # run2: 1037 passed / 0 failed / 3 ignored
# 无 hang/panic；作者另报 ×3 一致（1037/0/3），与评审 ×2 一致

# 4) 其它门禁
python -m pytest                             # 183 passed
cargo clippy --workspace --all-targets --locked -- -D warnings  # clean
cargo fmt --all -- --check                   # exit 0
cargo check -p zgraph-cli --no-default-features --locked        # 通过
bash scripts/dev/check.sh                    # CHECK PASSED
```

- **改动核对**：`git diff --name-only main...HEAD` = 仅 `crates/zgraph-harness/src/plugin.rs`；`git diff` 全部位于 `#[cfg(test)] mod tests`；无产品逻辑/依赖/其它文件改动。
- **断言未放宽**：`Err(RunFailure::Cancelled)`、`live_adapter_count().await == 0`、`wait_until_reaped(pid)`（≤10s 有界轮询）、`SCENARIO_TIMEOUT`=30s、失败路径 `close_provider()` 均在；仅把 pid 获取从「cancel 后 async 等待」移到「cancel 前有界等待」并用于失败诊断。
- **与自报对比**：隔离 10 次（作者）/5 次（评审）、harness 235/0/1、workspace ×3（作者）/×2（评审）、pytest 183、clippy/fmt/check/no-default 均一致。
- **提交信息**：`fix(harness): 稳定 codex cancel 测试（等待 pid 文件再取消）` + `Review: REV-103` + `Agent: dsl-dev` ✓；无 push/merge（评审时）。

## 5. 遗留问题

- 无阻塞/待修复项；测试修复为 REV-102 C1 收口，供发布前 merge。
- 作者 evidence 的 workspace ×3 与评审 ×2 均全绿；发布序列中若追加全量复跑，沿用该用例即可。
- 无新增 ignore、无断言放宽、无产品行为变化；无需登记 ISSUE。
