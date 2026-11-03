# REV-118 · cli · v0.5 F1b CLI reexec 回放（`--replay-tools=reexec`）

> **跨模块声明**：本分支含 trace 公共 helper（`tool_result_sha256`，与 writer 同口径）与 e2e 机械（`trace_tools_v04` AC-24h 改 v0.5 前置口径）；均 Lead 已批准、第二评审 Lead 追加；**未改 harness runner 语义**、无 trace v8。评审人只新增本评审记录。

- **分支**：`feat/cli-reexec`
- 分支: feat/cli-reexec
- **作者**：`cli-dev`（commit `26adfab`；正文 `Review: REV-118` / `Agent: cli-dev`）
- **评审人**：`reviewer-a`（独立评审，≠ 作者）
- **基于**：main `a67639b`（含 F1a harness reexec runner；`main = origin/main = a67639b`）
- **变更范围**：`a67639b..26adfab`（**7 files, +881/−40**）：`crates/zgraph-cli/src/{lib,replay_cmd}.rs`、`crates/zgraph-cli/tests/{tool_reexec(新增 603),tool_replay}.rs`、`crates/zgraph-trace/src/{lib,toolcall}.rs`、`tests/e2e/trace_tools_v04.rs`
- **评审时间**：2026-11-03
- **关联**：task-102/103；DR-017、AC-25、05 §5.16.1、06 §6.16.1；REV-111（recorded 模式）、REV-116（F1a）

## 1. 结论

**结论：通过**

P0=0、P1=0、P2=0；P3 4 项（DR-017 旧文「副作用 exit2」与实现「复用记录」口径待 D6 修订；测试未显式覆盖 MCP source/failed 行；`drift` 字段恒 false 口径；e2e AC-24h 仅前置/events 断言，完整 AC-25 由 F1c）。实现/门禁/测试主体成立。

### 1.1 已核对成立

| 项 | 结论 |
|---|---|
| trace 公共 sha | ✅ `tool_result_sha256(&Value)` 公开；`normalize` 的 result/error sha 改为调用同一 helper（`serde_json::to_string` compact + sha256 小写 hex）；落库字节不变、无新列/表/v8；集成测试与 writer `result_sha256` 完全一致 |
| 参数/前置 | ✅ `--replay-tools` 仅 tools 接受；recorded 默认；events/outputs 忽略；reexec 必须 `--allow-builtin-tools` + **显式 `--workspace-dir`**，缺任一 parse 错误 exit2（不默认 cwd）；非法 workspace → `HARNESS_CONFIG_LOAD` exit3，不降级 recorded |
| 候选/执行 | ✅ 仅 `source=builtin && read/grep/glob/diff && status=ok` 才 `reexec_readonly`；副作用 write/edit/apply_patch/bash、MCP/Python、非 ok 行一律**不执行**、复用记录；结果经公共 sha 比对记录 `result_sha256`，漂移 → `REPLAY_TOOL_RECORD_MISMATCH` exit3（文案含 drift）；truncated → TRUNCATED；缺 `result_sha256` → MISSING；坏 args/reexec 失败 → MISMATCH；均 exit3、不写 `runs.error_code` |
| 只读/无副作用 | ✅ 不写 trace/不新增 `tool_calls` 行/不更新 run；DB 主文件字节/行数、workspace 快照不变、无 `-wal`（测试断言）；只读工具不 spawn/bash |
| 输出 | ✅ recorded 模式输出逐字节不变；reexec 人类行尾/JSON 键 additive `replay_mode`/`drift`；side-effect/MCP/Python 行 `replay_mode=recorded`、结果仍用记录值；events/outputs 行为不变 |
| 测试 | ✅ `tool_reexec` 6 例：四工具匹配、文件漂移、truncated/缺 sha/缺记录、前置/非法 workspace、events/outputs 忽略+默认 recorded、sha 与 writer 一致；旧 `tool_replay` reexec 用例改「缺前置 exit2」；e2e AC-24h 改 v0.5 前置口径 |
| 门禁 | ✅ cli **255/0**（tool_reexec **6/0**）；workspace **1234/0/3**；clippy `-D warnings`/fmt/check.sh/no-default 全绿；check.sh PASSED |

## 2. 四维评审

| 维度 | 结论 | 依据 |
|---|---|---|
| 功能正确性 | ✅ | reexec 白名单/前置/漂移/失败面/无副作用/事件忽略/默认不变成立；sha 口径与 writer 一致；副作用行复用记录符合 task-102 口径 |
| 代码结构合理性 | ✅ | trace helper 单点复用于 writer/CLI；CLI 保持 recorded 路径独立、reexec 分支集中；无 v8/新表；e2e 仅机械 |
| 逻辑性 | ✅ | recorded↔reexec 输出 additive、候选条件、失败码顺序（missing→truncated→mismatch）、异常不降级路径清晰；drift 在 mismatch 前退出（恒 false 可选字段） |
| 可读性 | ✅（P3） | 注释/错误文案/测试分组清晰；P3 为文档口径与测试覆盖补充 |

## 3. 发现清单

### 3.1 P0/P1/P2

无。

### 3.2 P3（建议，D6/后续）

- **P3-1（D6 文档修订，Lead 已知）**：DR-017 旧文/AC-25 旧表述「副作用/非白名单 → exit2」与 task-102 最终口径「**复用记录、不执行**」不一致；实现按 task-102（side-effect 行 `replay_mode=recorded` 正常 exit0）。D6 需统一 DR-017/05 §5.16.1/06/07 的字段/语义（副作用工具 recorded、不新增 exit2）。
- **P3-2 测试覆盖**：6 例显式覆盖 write/Python 复用，未单独覆盖 MCP source、failed 行、bash 行；建议 F1c/单测补 1–2 条（不改实现）。
- **P3-3 `drift` 字段**：reexec 输出 `drift=false`；mismatch 直接 exit3，不存在 `drift=true` 输出；建议 spec/D6 明确「drift 为成功路径恒 false 的 additive 字段」或移除。
- **P3-4 e2e AC-24h**：仅保留 v0.4 前置 exit2/events 不变；完整 AC-25（reexec 匹配/漂移/无副作用）由 F1c e2e 交付；本分支不伪造。
- **P3-5**：`reexec` 即使无候选行也会构造/校验 tool set（非法 workspace 同样 exit3）；符合「前置配置错误」口径，文档可说明。

## 4. 验证证据

评审人亲自执行（worktree `.worktrees/cli-cli-dev`，HEAD `26adfab`，main `a67639b`，worktree 干净；`CARGO_HOME=<repo>/.cargo-home`）：

```bash
cargo test -p zgraph-cli                    # 255 passed / 0 failed
cargo test -p zgraph-cli --test tool_reexec # 6 passed / 0 failed
cargo test --workspace                      # 1234 passed / 0 failed / 3 ignored
cargo clippy --workspace --all-targets --locked -- -D warnings  # clean
cargo fmt --all -- --check                  # exit 0
cargo check -p zgraph-cli --no-default-features --locked        # 通过
bash scripts/dev/check.sh                   # CHECK PASSED
```

- **抽查**：`tool_result_sha256` 与 writer 落库 `result_sha256` 一致（测试）；四只读工具 reexec 匹配 exit0；改文件 → MISMATCH exit3；truncated/缺 sha/缺记录 → exit3；缺 flag → exit2；非法 workspace → exit3；events/outputs 忽略；write/Python 行复用记录且 workspace/DB 字节/行数/无 -wal 不变；recorded 默认无 additive。
- **范围/红线**：7 files；trace helper + e2e 机械跨模块已批准；无新 trace 表/列/v8；无工具执行副作用；无 token/dist；check.sh 绿。
- **对比自报**：cli 255/0、tool_reexec 6、workspace 1234/0 与自报一致。

## 5. 条件关闭方式

无（结论通过；P3 见 §3，D6 文档口径需随 F1 收口修订）。

## 6. 条件关闭记录（"通过"）

无开放条件。

## 7. 遗留问题

- **D6 必改**：DR-017/05 §5.16.1/06 §6.16.1/07 AC-25 旧「副作用 exit2」改为「副作用/MCP/Python/失败行复用记录（`replay_mode=recorded`），绝不执行」；`REPLAY_TOOL_RECORD_MISMATCH` 仅 drift/坏记录。
- F1c 补 AC-25 e2e 与 P3-2 覆盖；`drift` 字段口径 D6 定。
- 跨模块 trace helper + e2e 机械由 Lead 第二评审。

## 8. 第二评审 · 跨模块（trace/e2e 机械）（Lead 追加，2026-11-03）

- **评审人**：`lead`（**≠ 作者** `cli-dev`；Lead 已批准跨模块改动，按 AGENTS §2 追加第二评审）
- **范围**（`a67639b..0b4a2fc`）：
  1. trace 小扩展：`crates/zgraph-trace/src/toolcall.rs` + `lib.rs` 公开 `tool_result_sha256(&Value)`（writer normalize 复用、落库字节不变、无 v8/新列）；
  2. e2e 机械：`tests/e2e/trace_tools_v04.rs` AC-24h 改 v0.5 口径（缺前置 flag → exit2；events/outputs 不变断言保留；完整 AC-25 验收归 F1c）。
- **核对结论（通过）**：trace helper 仅为同口径复用（无行为/格式变化）；e2e 仅期望口径更新（未放宽 events/outputs 断言）；无 kernel/其它模块改动；无 Cargo 变更。
- **Lead 亲自验证**：`git diff a67639b..0b4a2fc --stat` 仅 cli replay/tests + trace toolcall/lib + e2e 1 文件（+ REV 记录）；`cargo test --workspace --offline` = 1234/0/3；`cargo test -p zgraph-cli --offline` = 255/0；`tool_result_sha256` 与 writer `result_sha256` 一致性用例通过。
- **裁决/后续**：实现口径（副作用/MCP/Python/失败行复用记录、不执行、不 exit2）为准；D6 修订 DR-017/05/06/07/AC-25 旧文（「副作用→exit2」）；F1c 补 AC-25 e2e（含 MCP/failed/bash 行）；`drift` additive 字段保留并在 06 明确。
- **结论**：**通过（第二评审）**。分支满足合并前置。
