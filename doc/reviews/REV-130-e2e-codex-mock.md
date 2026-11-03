# REV-130 · e2e · v0.5 F5d Codex 零密钥 mock CI + adapter 修复

> **跨模块声明**：`python/zgraph_worker/adapters/codex.py`（产品 adapter）改动 Lead 已批准，第二评审 Lead 追加；其余为 `python/tests`/`tests/e2e`/fixture/Cargo。评审人只新增本评审记录。

- **分支**：`test/e2e-codex-mock`
- 分支: test/e2e-codex-mock
- **作者**：`e2e-dev`（commit `0bd5312`；正文 `Review: REV-130` / `Agent: e2e-dev`）
- **评审人**：`reviewer-a`（独立评审，≠ 作者）
- **基于**：`568c0f3`（分支父提交）；**main 已到 `118ef69`（F5e/REV-129 已合并）**
- **变更范围**：`568c0f3..0bd5312`（**6 files, +1238/−2**）：`python/zgraph_worker/adapters/codex.py`、`python/tests/test_adapters_codex.py`、`tests/e2e/{Cargo.toml,codex_mock_v05.rs,harness_v03.rs,fixtures/py/mock_codex_responses.py}`
- **评审时间**：2026-11-04
- **关联**：task-125/127；DR-020 Codex FINAL、AC-28 g–j；F5e/REV-129

## 1. 结论

**结论：通过**

> **结论历史**：首轮（基准 `0bd5312`，分支基于 `568c0f3`）为「**有条件通过**」——P1 C1 分支未 rebase 到含 F5e 的 main `118ef69`。作者 rebase 后 HEAD `35e5609`（实现 `ae5662d`；main 成为祖先，实现 diff 仍 6 files + REV-130/README），评审人复核 ancestry/diff/门禁/gated（§8），C1 closed，结论改**通过**。
>
> 首轮意见（历史保留）：P0=0；adapter 9 项/gated 真机 mock 3 例/CI ac28g 成立；P3 真机限制/F6 计数/版本诊断。

### 1.1 已核对成立

| 项 | 结论 |
|---|---|
| adapter 9 项 | ✅ `turn.completed` 清 failed（error→completed 单测 + gated/CI）；`--ephemeral`（单测）；`item.started/completed[command_execution]` → `tool_start/tool_end`（`ok=exit_code==0`，args/command、result/output_tail；无 event 不伪造；completed 无 started 用真实 completed 补配对）；ARG_MAX/iterations/usage 累计/cached 丢弃/认证分层/node_timeout>60s 文档；pin 0.166.0 诊断（warn，gated 版本不符 skip）；取消 3s/EOF exit0 单测 |
| 零密钥 mock e2e | ✅ `codex_mock_v05` 3 例门控：真实 `codex-cli 0.166.0`（`ZGRAPH_CODEX_BIN`/`CODEX_BIN`）+ 本地 mock SSE + 临时 CODEX_HOME mock provider；文本全链路断言 content/EndTurn/usage 11·7·18/cached 丢弃/`llm_calls=1`/`-o` 一致/无 token delta；取消 <3s/无孤儿；F5f 嵌套 Seatbelt 显式 skip（非假绿）；缺失二进制/版本不符显式 skip 不伪造 |
| CI ac28g | ✅ `harness_v03` 假 codex（shell）驱动真实 adapter：error→completed 恢复、command_execution 映射为 streaming tool_start/tool_end（ok=1）、外部 harness 不写 v7 `tool_calls`；补齐无语 key CI |
| 测试/计数 | ✅ e2e **174/0/5 ignored ×2**；workspace **1290/0/6**；pytest **203 passed/2 skipped**；clippy/fmt/check.sh 绿；与自报一致；baseline 未改（F6 记录） |
| 真机复核 | ✅ 用 `/tmp/codex-npm` 真实二进制 `ZGRAPH_REAL_CODEX=1` 跑 3 gated：text/cancel 通过，F5f 因嵌套 Seatbelt 显式 skip；无 env 时 3 例显式打印 skip（不伪造） |
| 范围/安全 | ✅ 6 files；adapter 跨模块 Lead 批准；npm 产物未提交（`/tmp`）；mock env_key 仅占位；无真实 key/token；无 docs/其它实现 |

## 2. 四维评审

| 维度 | 结论 | 依据 |
|---|---|---|
| 功能正确性 | ⚠️ 有条件 | adapter 映射/恢复/取消/mock 链路成立；C1 分支未 rebase 到含 F5e 的 main |
| 代码结构合理性 | ✅ | adapter 事件映射/版本诊断集中；mock fixture/门控独立；CI 假 codex 复用真实 adapter；无产品行为越界 |
| 逻辑性 | ✅ | error→completed 清 failed、tool 事件配对/无事件不伪造、gated skip 语义清楚；usage/缓存/iterations 口径与 DR-020 一致 |
| 可读性 | ✅（P3） | 注释/门控理由清晰；P3 为真机限制/baseline/版本诊断 |

## 3. 发现清单

### 3.1 P1 条件（merge 前必须关闭）

| # | 位置 | 问题 | 建议 |
|---|---|---|---|
| **C1** | 分支 `568c0f3` vs main `118ef69` | `git merge-base HEAD main=568c0f3`；main（F5e/REV-129）**不是 HEAD 祖先**，merge.sh 要求基于最新 main；当前会失败 | `git rebase main` 到 `118ef69`（adapter/e2e 预计无冲突，F5e 只动 harness usage 标注）；复核 diff 仍 6 files/门禁，报告新 HEAD |

### 3.2 P2

无。

### 3.3 P3（建议，不阻塞）

- **P3-1 真机限制**：F5f 嵌套 Seatbelt 显式 skip、F5d-b 真 key 未跑（无授权）；F6 release notes 需标注，非嵌套用户终端证据 F6 补。
- **P3-2 计数/baseline**：e2e 174/5 ignored、workspace 1290/6、pytest 203/2 skipped 与基线不同；F6 统一记录（本分支不改 baseline 属预期）。
- **P3-3 版本诊断**：`codex --version` 每进程一次（约几十 ms），版本不符仅 warn；Lead 预裁决接受；gated e2e 版本不符 skip 已实现。
- **P3-4**：`codex_mock_v05` gated 用例在 `--ignored` 下缺二进制时打印 skip 后 `ok`（不是 fail）；符合「缺失→显式 skip 不伪造」，但 CI 日志需保留 skip 输出。

## 4. 验证证据

评审人亲自执行（worktree `.worktrees/e2e-e2e-dev`，HEAD `0bd5312`，main `118ef69`，worktree 干净；`CARGO_HOME=<repo>/.cargo-home`）：

```bash
git status --porcelain                 # 空
git diff --stat main...HEAD            # 6 files +1238/−2（adapter/e2e/fixture）
bash scripts/dev/check.sh              # CHECK PASSED

for i in 1 2; do cargo test -p zgraph-e2e; done   # 174 / 0 / 5 ignored（一致）
cargo test --workspace                 # 1290 / 0 / 6 ignored
python -m pytest                       # 203 passed / 2 skipped
cargo clippy --workspace --all-targets --locked -- -D warnings  # clean
cargo fmt --all -- --check             # exit 0

# 真机 gated（/tmp/codex-npm，未提交）
ZGRAPH_REAL_CODEX=1 ZGRAPH_CODEX_BIN=/tmp/codex-npm/node_modules/.bin/codex \
  cargo test -p zgraph-e2e --test codex_mock_v05 -- --ignored --nocapture
# => 3 passed：f5d_a text / f5d_c cancel 通过；F5f 打印 nested Seatbelt skip
# 无 env 同命令 => 3 例打印 "set ZGRAPH_REAL_CODEX=1" skip（不伪造）
```

- **adapter 抽查**：turn.completed 清 failed、`--ephemeral`、command_execution→tool_start/end、无事件不伪造、版本诊断、取消/EOF 单测；`test_adapters_codex.py` 覆盖。
- **CI ac28g**：真实 adapter + 假 codex，error→completed + tool 映射 + 外部不写 v7 tool_calls。
- **范围/安全**：6 files；adapter 跨模块批准；npm `/tmp` 未提交；mock env_key 占位；无真实 key；无 token/dist。
- **分支状态**：`git merge-base HEAD main=568c0f3`，main 非 HEAD 祖先（C1）。

## 5. 条件关闭方式

- **C1 由 e2e-dev rebase 到 main `118ef69`**（F5e 已合并）；复核 diff 仍 6 files、e2e/workspace/pytest/clippy/fmt/check 与 gated 真机；追加「条件关闭」段、结论改「通过」；随后 Lead merge。
- P3-1/P3-2 由 F6 release notes/baseline 记录；P3-3/P3-4 不阻塞。

## 6. 条件关闭记录（"有条件通过"）

| 条件 | 状态 | 关闭要求 |
|---|---|---|
| C1 分支 rebase 到 main 118ef69 | **closed（35e5609，实现 ae5662d）** | `git merge-base HEAD main=118ef69`，main 为 HEAD 祖先；实现 diff 仍 6 files + REV-130/README；门禁/gated 复跑通过 |

## 7. 遗留问题

- C1 closed；P3-1 F5f/F5d-b 限制、P3-2 计数/baseline F6 记录；不阻塞。
- adapter 跨模块改动由 Lead 第二评审；真机 mock 证据已复核（text/cancel 通过，F5f 嵌套 skip）。
- 后续 F6 v0.5.0 发布收口。

## 8. 条件关闭复核（2026-11-04）

复核提交：`35e5609`（rebase 后；实现 `ae5662d`，父 `118ef69`；实现 diff 6 files，额外仅 REV-130 记录/README）。评审人执行：

```bash
git merge-base HEAD main                     # 118ef69
git merge-base --is-ancestor main HEAD       # yes
git diff --stat main...HEAD                  # 6 实现文件 + REV-130/README
bash scripts/dev/check.sh                    # CHECK PASSED
for i in 1 2; do cargo test -p zgraph-e2e; done  # 174/0/5 ignored ×2
cargo test --workspace                       # 1295/0/6 ignored
python -m pytest                             # 203 passed/2 skipped
cargo clippy ... -D warnings / fmt --check   # 绿
ZGRAPH_REAL_CODEX=1 ZGRAPH_CODEX_BIN=/tmp/codex-npm/node_modules/.bin/codex \
  cargo test -p zgraph-e2e --test codex_mock_v05 -- --ignored --nocapture
# => 3 passed（text/cancel；F5f nested Seatbelt 显式 skip）
```

| 项 | 复核结果 |
|---|---|
| C1 ancestry | ✅ `merge-base=118ef69`；main 为 HEAD 祖先；无冲突 |
| diff | ✅ F5d 实现仍 6 files；额外仅 REV-130 评审记录/README；无产品代码越界 |
| 门禁 | ✅ e2e 174/0/5 ×2、workspace 1295/0/6、pytest 203/2、clippy/fmt/check 绿 |
| gated 真机 | ✅ 真实 codex 0.166.0 + mock：text/cancel 通过；F5f 嵌套显式 skip；无 env 时显式 skip（不伪造） |

**最终状态**：C1 closed；P3 留 F6；建议 Lead merge F5d → F6 v0.5.0 发布收口。
