# REV-121 · e2e · v0.5 F1c AC-25 reexec 完整验收

- **分支**：`test/e2e-reexec`
- **作者**：`e2e-dev`（单提交 `fd520f0`，基于 main `d3da745`）
- **评审人**：`reviewer-b`（独立评审，≠ 作者）
- **变更范围**：`d3da745..fd520f0`（3 files，+835/−0）：`tests/e2e/reexec_v05.rs`（+830）、`tests/e2e/Cargo.toml`（+4）、`doc/test-baseline.txt`（+1）；无产品代码/fixture 变更
- **评审时间**：2026-11-03
- **关联**：task-106/108、DR-017、AC-25、spec 05 §5.16.1/06 §6.16.1/07、F1b CLI、REV-118/119

## 1. 结论

**结论：通过**

P0=0；P1=0；P2=5；P3=4。真 CLI 链路（fake LLM + 真内置工具 + 真 SQLite v7 → `replay --mode=tools --replay-tools=reexec`）成立；AC-25 a–f 全覆盖，MCP/Python/failed 行复用记录；e2e **24 target / 173 passed / 0 failed / 2 ignored 连跑 2 次**、workspace 重跑 **1241/0/3**、cli **255**、pytest **183**、clippy/fmt/check/no-default 全绿；baseline **24 target/173** 一致。

## 2. 四维评审

| 维度 | 结论 | 依据 |
|---|---|---|
| 功能正确性 | ✅ | AC-25 a–f 真链路；漂移/截断/缺行/缺 sha/缺 flags/模式忽略均精确；副作用工具不执行 |
| 结构合理性 | ✅ | 单 target `reexec_v05`；复用 v03/v04_support；无产品代码；7 用例分层清晰 |
| 逻辑性 | ✅ | 真 CLI + 真 DB；recorded/reexec 对照、去 additive 比较、DB/workspace 快照、合成行复用 |
| 可读性 | ✅（P2/P3） | AC 注释/错误码/环境说明清楚；P2/P3 为强度补强 |

## 3. AC-25 映射

| AC | 用例 | 覆盖 |
|---|---|---|
| a | `ac25a_recorded_default_is_unchanged_and_executes_nothing` | recorded 默认逐字节不变、无 tools 执行、无 additive、workspace/DB/行数不变、sentinel 不被改 |
| b | `ac25b_reexec_readonly_succeeds_and_is_additive` | read/grep/glob/diff reexec；write/edit recorded；additive `replay_mode=reexec/drift=false`；去 additive 与 recorded 逐行一致；recorded 输出逐字节不变；workspace/DB/行数零副作用 |
| c | `ac25c_reexec_drift_fails_closed_for_all_readonly_tools` | read/grep/glob/diff 四种漂移 → `REPLAY_TOOL_RECORD_MISMATCH` exit3（含 call id/drift 文案）；不写 `runs.error_code`；恢复后成功 |
| d | `ac25d_side_effect_tools_are_never_executed` | write/edit recorded 不执行（sentinel 保留）；合成 bash 行 recorded 复用；reexec 成功即嵌套 Seatbelt 下未触发 bash/沙箱；workspace/DB/行数不变 |
| e | `ac25e_reexec_fail_closed_on_truncated_missing_sha_and_missing_row` | truncated=1 → `REPLAY_TOOL_RECORD_TRUNCATED`；缺 result_sha256 → `REPLAY_TOOL_RECORD_MISMATCH`；缺行 → `REPLAY_TOOL_RECORD_MISSING`；均 exit3、workspace 不变 |
| f | `ac25f_flags_workspace_and_modes_are_enforced` | 缺 `--allow-builtin-tools` exit2；缺显式 `--workspace-dir` exit2；非法 workspace exit3（`HARNESS_CONFIG_LOAD/workspace_invalid`）；events/outputs 与无 reexec 参数逐字节一致；schema 仍 v7、无 reexec/pre_sha 列、trace 无 additive |
| 补 | `ac25_reexec_reuses_mcp_python_and_failed_rows` | MCP/Python/error(builtin)/timeout(MCP) 合成行均 recorded、source/status/result/error 原样；不执行、不重算、workspace/行数不变 |

## 4. 验证证据

worktree `/Users/zyzs/lczyzs/code/zgraph/.worktrees/e2e-e2e-dev`，HEAD `fd520f0`，base `d3da745`，工作区干净；`CARGO_HOME=/Users/zyzs/lczyzs/code/zgraph/.cargo-home`，未设 `CARGO_TARGET_DIR`：

```bash
cargo test -p zgraph-e2e            # 第 1 次：24 target / 173 passed / 0 failed / 2 ignored
cargo test -p zgraph-e2e            # 第 2 次：24 target / 173 passed / 0 failed / 2 ignored
cargo test --workspace              # 首次 1 failed：既有 ac09_parallel_branches_overlap... 时序断言（425.2ms vs 507.9ms，未达 <80%）；隔离 4/4 通过，workspace 重跑 1241/0/3
cargo test -p zgraph-cli            # 255 passed / 0 failed
pytest -q                           # exit 0；183 passed
cargo clippy --workspace --all-targets --locked -- -D warnings  # clean
cargo fmt --all -- --check          # clean
bash scripts/dev/check.sh           # PASSED
```

- **范围**：仅 `tests/e2e/reexec_v05.rs` + `tests/e2e/Cargo.toml` + `doc/test-baseline.txt`；无产品代码/fixture/golden；baseline **24 target/173** 与实际一致。
- **真实性**：`run_scenario` 走真 CLI、fake LLM、真内置工具、真 SQLite；reexec 走真 `replay` CLI（显式 `--allow-builtin-tools` + 显式 `--workspace-dir`）；无 mock 产品 reexec。
- **稳定性**：F1c 两次 e2e 全绿；`ac09_parallel_branches_overlap...` 首次 workspace 失败为既有 timing flake（本分支只加 e2e target，不涉 parallel/kernel），隔离与 workspace 重跑均通过。建议 D6 观察/加固，不阻断 F1c。

## 5. 开放项 / 裁决

- **#1**（REV-119 docs 修订）：DR-017 已改非候选复用/不报 exit2；本实现/测试与之一致。
- **#2**（缺 `result_sha256`）：实测行存在但字段缺失/损坏 → `REPLAY_TOOL_RECORD_MISMATCH`（validate 先拒绝）；Lead 已裁决以此为准，行缺失/streaming 多于行 → `REPLAY_TOOL_RECORD_MISSING`；docs D6 小修统一。本评审记录按实测口径。

## 6. 发现清单

### P2（不阻断）

| # | 位置 | 问题 | 建议 |
|---|---|---|---|
| N1 | `ac25d` | bash 不触发沙箱用「reexec 成功 + stderr 无 sandbox」间接证明；无直接 spawn/沙箱调用计数 | D6 可用测试钩子/进程计数加强；当前嵌套环境成功已足以证明未执行 |
| N2 | `ac25d` | bash 行为合成行，非真实 run 产生的 bash 记录（任务允许 synthetic；本环境不可真跑 bash） | D6/普通终端真 bash 行可选补 |
| N3 | `ac25c`/`ac25e` | 漂移/fail-closed 后未显式断言 DB/-wal 字节不变（a/b/d 已断言）；仅 workspace 不变 + 恢复 | 补充 DB 快照断言 |
| N4 | `ac25b` | 「去 additive 后与 recorded 逐行一致」依赖 `strip_additive` 只删 `replay_mode/drift`；如未来 CLI 新增其他 additive 字段需同步 | D6 文档化 additive 字段闭集 |
| N5 | `ac25_reexec_reuses...` | MCP/Python/failed/timeout 为合成 v7 行（任务允许）；未用真实 MCP/Python 工具链路 | D6 可补真实 Python worker/MCP e2e |

### P3（测试/文档）

| # | 位置 | 问题 | 建议 |
|---|---|---|---|
| N6 | `ac25f` | 缺 flags/invalid workspace 未断言 DB 不存在（仅 exit/reason） | 补 `!db.exists()` |
| N7 | `ac25a` | 人类输出 `recorded; no tools executed` 取自 build 时；建议直接断言本次 recorded 输出 | 可选 |
| N8 | stability | 既有 `ac09_parallel` timing flake 首次 workspace 命中 | D6 观察/加固（ISSUE） |
| N9 | baseline | 24 target/173 与实际一致 | 保持 |

## 7. 遗留问题

- P2 N1–N5 / P3 N6–N9 留 D6 补强；无 P1/P0。
- F1c 结论：**通过**；可 merge → F2 并行（REV-120 另评）。
- AC-25 真 bash 隔离/普通终端证据仍归 D6/verify-sandbox.sh（本 e2e 通过合成行 + nested fail-closed 断言，不伪造）。
