# REV-116 · harness · v0.5 F1a reexec 只读工具 runner

- **分支**：`feat/harness-reexec`
- **作者**：`dsl-dev`（单提交 `db50f12`，基于 main `7b3e819`；origin/main `304351c` docs-only）
- **评审人**：`reviewer-b`（独立评审，≠ 作者）
- **变更范围**：`7b3e819..db50f12`（1 file，+350/−1）：`crates/zgraph-harness/src/builtin/files.rs`；无 Cargo/其它模块
- **评审时间**：2026-11-03
- **关联**：task-98/101、DR-017、DR-016 AC-25、spec 05 §5.16.1、DR-014 trace sha、REV-110/112/115

## 1. 结论

**结论：通过**（首轮有条件通过 → C1 由 `029ba20` 关闭；P2 全量测试已补，见 §7）

P0=0；**P1=0**；P2 残余（不阻断，见 §7）；P3 后续。白名单/拒绝面/参数校验/jail 复用/无写盘/无 spawn/无 trace-sink-budget/确定性/allow-path 均通过；最终 harness **352/0/1**（默认与 features）、workspace **1228/0/3**、cli **249**、pytest **183**、clippy/fmt/check/no-default 全绿。

> **首轮历史（保留）**：`db50f12` 首轮为有条件通过——C1 reexec read/diff 复用内部 32 KiB 截断、全量测试只覆盖 grep。`029ba20` 已关闭 C1 并补 read/diff/glob 全量测试（见 §7）。

首轮条件：

- **C1 read/diff 并未返回「完整未截断 Value」**：`reexec_readonly` 直接复用 `self.read` / `self.diff`。`read` 内部已把 `content` 截到 `BUILTIN_RESULT_MAX_BYTES-256`（32 KiB 级）并置 `truncated=true`；`diff` 内部同样 `truncate_utf8(diff_text, ...)`。因此 reexec 的 read/diff 大结果仍是截断值，与任务「返回完整 Value（未截断；F1b 用 trace sha 比对）」及 DR-017 的 sha 比对目标不符。现有 `reexec_returns_full_untruncated_value` 只覆盖 **grep**（grep 自身不套 32 KiB，只有 `limit_result` 在正常 call 侧截断），未覆盖 read/diff。

## 2. 四维评审

| 维度 | 结论 | 依据 |
|---|---|---|
| 功能正确性 | ✅ | 白名单/拒绝/参数/jail/确定性成立；C1 全量路径已由 `029ba20` 关闭 |
| 结构合理性 | ✅ | 集中在 `BuiltinToolSet` 的 reexec API；复用只读实现；无新模块/依赖 |
| 逻辑性 | ✅（P2） | 拒绝面、jail、无副作用、确定性符合 DR；P2 见 §3 |
| 可读性 | ✅（P3） | 文档注释/测试名清晰；P3 测试/文档补齐 |

## 3. 发现清单

### P1 · 合并前必须关闭

| # | 位置 | 问题 | 条件 |
|---|---|---|---|
| C1 | `builtin/files.rs::reexec_readonly`（TOOL_READ/TOOL_DIFF 分支） | 复用 `self.read`/`self.diff` 的内部 32 KiB 截断；返回的 `content`/`diff` 可能带 `truncated=true` 且非完整，F1b 无法用 trace sha 做完整漂移比对（尤其 read 大文件、diff 长文本） | 为 reexec 提供「无结果 cap」的 read/diff 内部路径（或明确拒绝会截断的 read/diff 并让 F1b fail-closed）；补 40 KiB 级 read、长 diff 的 reexec 全量断言（对照正常 call 的 truncated 行为），再跑门禁 |

### P2 · 建议

| # | 位置 | 问题 | 建议 |
|---|---|---|---|
| N1 | `reexec_returns_full_untruncated_value` | 只测 grep；read/diff/glob 无全量对照（glob 也可能受 max_results 影响） | 补 read 大文件/diff 长文本/glob 大结果；与 C1 一并 |
| N2 | `reexec_readonly` 的 jail 配置 | API 使用构造时 `WorkspaceJail`，但无法自证 workspace/allow-path 是显式配置还是默认 `.`；F1a 无 CLI 层 | F1b CLI 强制显式 workspace/allow-path；F1a 文档注明「配置由调用方保证」 |
| N3 | 无 trace/sink/budget 验证 | 代码路径无 trace/sink/budget 调用，但测试未做 trait 注入计数/预算不变的显式断言 | 加一个 no-op sink/计数器断言（可选）；F1b 集成验证 |
| N4 | `reexec_returns_full_untruncated_value` 的正常 call 对照 | 正常 `set.call` 返回 32 KiB 截断，reexec 全量；但只选 grep | 补 read/diff 正常/reexec 对照；与 C1 一并 |
| N5 | F1b 衔接 | trace `result_sha256` 是正常工具结果的序列化 hash；read/diff 正常结果可能已 truncated，F1b 应遇 `truncated=1` fail-closed；read/diff reexec 若也截断会掩盖漂移 | F1b 明确 truncated fail-closed + C1 全量 reexec；新 reason `reexec_side_effect_forbidden`/`reexec_unsupported` D6 回写 |

### P3 · 测试/文档

| # | 位置 | 问题 | 建议 |
|---|---|---|---|
| N6 | 测试计数 | 新增 10 例，覆盖正例/拒绝/未知/args/jail/无副作用/确定性/allow/full(grep) | 补 read/diff/glob full |
| N7 | 文档 | reexec 的 `REEXEC_READONLY_TOOLS`/API 注释清楚；新 reason 未回写 05/00 | D6 回写 |
| N8 | 默认路径 | 现有全量通过；无 CLI/F1b 变化 | F1b 验证默认 recorded 逐字节不变 |
| N9 | 性能 | 无 spawn、无 trace；read/diff 复用内存实现 | 可接受；F1b 观测 |

## 4. 验证证据

worktree `/Users/zyzs/lczyzs/code/zgraph/.worktrees/harness-dsl-dev`，HEAD `db50f12`，base `7b3e819`（origin/main 304351c docs-only），工作区干净；`CARGO_HOME=/Users/zyzs/lczyzs/code/zgraph/.cargo-home`，未设 `CARGO_TARGET_DIR`：

```bash
cargo test -p zgraph-harness                 # 351 tests：350 passed / 0 failed / 1 ignored（含 doc）
cargo test -p zgraph-harness --features mcp  # 351 tests：350 passed / 0 failed / 1 ignored
cargo test --workspace                       # 89 targets：1226 passed / 0 failed / 3 ignored
cargo test -p zgraph-cli                     # 249 passed / 0 failed
pytest -q                                    # exit 0；183 passed
cargo clippy --workspace --all-targets --locked -- -D warnings  # clean
cargo fmt --all -- --check                   # clean
cargo check -p zgraph-harness --no-default-features  # clean
bash scripts/dev/check.sh                    # PASSED
```

- **与自报一致**：harness 350/0/1（默认/features）、workspace 1226/0/3、cli 249、pytest 183；clippy/fmt/check/no-default 全绿；仅 1 file，无 Cargo。
- **已核**：白名单 read/grep/glob/diff；write/edit/apply_patch/bash → `reexec_side_effect_forbidden`；未知/MCP/Python/未注册 → `reexec_unsupported`；参数经 `ToolDef`+`validate_tool_arguments`；jail 越界；只读无 workspace 快照变化/无 `.tmp`/无 temp；确定性同输入同输出；allow-path 只读；grep 400 条全量 vs 正常 32 KiB 截断。
- **未覆盖/风险**：C1 read/diff 内部截断；全量测试仅 grep；jail 配置自证留 F1b；trace/sink/budget 未显式注入断言。

## 5. F1b 衔接确认

- **sha 口径**：F1b 必须与 trace writer 的 JSON 序列化/全量 sha 完全一致；`truncated=1` → `REPLAY_TOOL_RECORD_TRUNCATED` fail-closed。
- **workspace/allow-path**：F1b 必须用显式同配置构造 `BuiltinToolSet`；不同配置/可变 workspace 产生的漂移 → `REPLAY_TOOL_RECORD_MISMATCH`；不升 trace v8。
- **CLI 构造**：F1b `--replay-tools=reexec` 要求 `--allow-builtin-tools` + 显式 `--workspace-dir`/`--allow-path`（缺 → 配置/用法错误）；副作用工具 → 用法错误 exit2；MCP/Python 仍 recorded。
- **新 reason**：`reexec_side_effect_forbidden`/`reexec_unsupported` 属 F1a 工具错误，D6 回写 05/AC-25 错误面。

## 6. 遗留问题

- **C1**：read/diff reexec 全量路径 + 测试；关闭后复审转「通过」。
- **P2 N1–N5**：全量测试、jail 配置自证、trace/sink/budget 断言、F1b truncated 策略、新 reason D6。
- **P3 N6–N9**：测试/文档/默认路径/性能。
- 无 P0；C1 未关闭前不可 merge。

## 7. 条件关闭复核（2026-11-03，`029ba20`，parent `8319efb`）

**复核结论：通过**。`029ba20` 仅 `builtin/files.rs` +122/−10；工作区干净；无 Cargo/其它模块。

### C1（reexec read/diff 全量）
- `read()` → `read_impl(args, true)`；`diff()` → `diff_impl(args, true)`；正常 call 保持 32 KiB cap + `truncated=true`。
- 新增 `read_full()`/`diff_full()`（`apply_result_cap=false`）供 `reexec_readonly`；返回完整 `content`/`diff`，`truncated=false`，无 `…[truncated]`。
- 测试 `reexec_read_and_diff_return_full_untruncated_values`：>40 KiB read 逐字节等于原文、正常 call 仍截断带 marker；长 diff 全量、正常 diff 仍截断；`reexec_glob_returns_full_paths_beyond_result_cap`：glob 500 条全量 vs 正常 32 KiB 截断；grep 400 条全量既有测试保留。**全部通过**。
- **C1 关闭**；正常 call 无回归（既有 `read_enforces_file_and_result_caps`/`diff_previews...`/全量门禁均绿）。

### P2 复核
- read/diff/glob 全量测试已补；grep 全量保留。
- 残余 P2（不阻断）：jail/workspace/allow-path 配置自证留 F1b（F1a API 只接收已构造 `BuiltinToolSet`）；trace/sink/budget 未做显式注入计数断言（代码路径不调用，F1b 集成验证）；F1b 需 truncated=1 → `REPLAY_TOOL_RECORD_TRUNCATED`、sha 序列化一致、显式同 workspace/allow-path；新 reason D6 回写。

### 最终门禁（`029ba20`）
```bash
cargo test -p zgraph-harness                 # 353 tests：352 passed / 0 failed / 1 ignored
cargo test -p zgraph-harness --features mcp  # 353 tests：352 passed / 0 failed / 1 ignored
cargo test --workspace                       # 89 targets：1228 passed / 0 failed / 3 ignored
cargo test -p zgraph-cli                     # 249 passed / 0 failed
pytest -q                                    # exit 0；183 passed
cargo clippy --workspace --all-targets --locked -- -D warnings  # clean
cargo fmt --all -- --check                   # clean
cargo check -p zgraph-harness --no-default-features  # clean
bash scripts/dev/check.sh                    # PASSED
```
- 仅 1 file；无 Cargo；默认路径无回归；`git status` 空。
- 稳定性：workspace 本轮全绿（作者 3 连跑中 run1 命中既有 kernel_cancel flake，隔离 3/3；本分支仅 files.rs，无 kernel 改动，归既有 flake 观察）。
