# DR-017 · v0.5 reexec 工具回放

> **状态**：V1 设计冻结（REV-115；grep/glob 漂移口径为 Lead 裁决项）。
> **日期**：2026-11-03 · **作者**：docs-dev · **关联**：DR-014（v7 `tool_calls`）、06 §6.15、07 AC-24/25、task-95
> **合同**：`--replay-tools=reexec` **显式**（默认 `recorded` 不变）；只读白名单 `read`/`grep`/`glob`/`diff`；**副作用工具永远 recorded-only**；不升 trace v8。

## 0. 摘要

| 项 | 冻结 |
|---|---|
| 默认 | `--replay-tools=recorded`：**绝不执行任何工具**；v0.4 行为不变 |
| 显式 reexec | `--mode=tools --replay-tools=reexec`；不加该 flag 不重跑任何工具 |
| 白名单 | `builtin__read`/`grep`/`glob`/`diff`；其余（write/edit/apply_patch/bash）**拒绝**（exit 2 用法错误） |
| 漂移校验 | read/diff/grep/glob：重跑结果 vs 记录 `result_sha256`；不一致 → `REPLAY_TOOL_RECORD_MISMATCH`（exit 3）；截断/缺记录 fail-closed |
| 不做 | 不新增 trace 列/表（**不升 v8**）；不重跑 MCP/Python/外部 harness 工具；不重跑副作用工具；无沙箱/环境差异静默通过 |
| 输入一致性 | 回放必须使用与记录一致的 `--workspace-dir`/`--allow-path`（只读工具语义依赖）；不一致由结果 mismatch fail-closed；文档说明 |
| AC-25 | a–f（见 §6） |

## 1. reexec 语义

- **显式开关**：`--mode=tools` 下 `--replay-tools=reexec`；`recorded`（缺省）保持 v0.4 行为。
- **执行范围**：仅 `builtin__read`/`grep`/`glob`/`diff`；它们**无副作用**（read/grep/glob/diff 不写 workspace）。
- **拒绝面**：`write`/`edit`/`apply_patch`/`bash` 在 reexec 请求中 → **用法错误 exit 2**（不执行、不降级为 recorded）；MCP/Python/外部 harness 工具不在 v0.5 reexec 范围（仍 recorded；若记录缺失 fail-closed）。
- **执行环境**：与实时相同的 jail/工具实现；`--allow-builtin-tools` 必须开启；`--workspace-dir`/`--allow-path` 必须与记录时一致（否则结果可能不同 → mismatch）。
- **不写新 trace**：reexec 是只读回放，不新增 `tool_calls`/events 行。

## 2. 漂移校验（关键裁决）

v7 `tool_calls` 已有 `args_sha256`/`result_sha256`，**无输入文件 pre_sha256**（除副作用工具）；因此：

| 工具 | 校验方式 | 结论 |
|---|---|---|
| `builtin__read` | 重跑结果 JSON 序列化后的 sha256 vs 记录 `result_sha256`；同时要求 args 完全一致 | 不一致 → `REPLAY_TOOL_RECORD_MISMATCH` |
| `builtin__diff` | 同上（结果 sha 比对；diff 结果含 changed/diff） | 同上 |
| `builtin__grep` | 记录无输入文件 hash → **重跑结果 sha256 与记录比对**；不一致即 drift（可能因文件变化/忽略规则变化） | 同上（fail-closed） |
| `builtin__glob` | 同上（结果路径列表 sha 比对） | 同上（fail-closed） |

- **共同前提**：args_json 必须逐字节/规范化一致；记录 `truncated=1` → **fail-closed**（无法完整比对）；记录缺行 → `REPLAY_TOOL_RECORD_MISSING`。
- **不做**：不因 grep/glob 无输入 hash 就静默跳过比对；不引入 `pre_sha256` 新列（**不升 v8**）。grep/glob 的 drift 语义为「结果与记录不一致」而非「输入文件已变」——文档必须写清。
- **裁决项**：是否允许 grep/glob 在结果一致时通过（推荐：是，以结果 sha 为准）；是否改为仅 read/diff 可 reexec（备选，更保守）。**推荐前者**。

## 3. CLI/错误语义

| 场景 | 结果 |
|---|---|
| `--replay-tools=reexec` + 白名单工具 + 结果一致 | exit 0；输出标注 `replay_mode=reexec`/`drift=false`（additive） |
| reexec 结果 ≠ 记录 | `REPLAY_TOOL_RECORD_MISMATCH`，exit 3（不写 `runs.error_code`） |
| 副作用/非白名单工具 | 用法错误 exit 2（`--replay-tools=reexec` 仅允许只读工具） |
| 记录 truncated/缺失 | `REPLAY_TOOL_RECORD_TRUNCATED`/`REPLAY_TOOL_RECORD_MISSING`，exit 3 |
| 未开 `--allow-builtin-tools` | 用法/配置错误（exit 2） |

- 输出 additive：人类行/NDJSON 可加 `replay_mode`/`drift`（不改变既有字段）。
- **不新增顶层错误码/CLI 码**：复用 `REPLAY_TOOL_RECORD_*`；exit 码沿用 2/3。

## 4. workspace/jail 一致性

- reexec 的 `read/grep/glob/diff` 在**当前 workspace/jail 配置**下执行；不同 workspace/allow-path 会产生不同结果 → 由结果 mismatch fail-closed（不静默）。
- 文档要求：回放命令应显式传与原始 run 相同的 `--workspace-dir`/`--allow-path`/`--allow-builtin-tools`；否则预期 mismatch。
- 不做「自动从 trace 恢复 workspace 路径」（v7 未记录运行根；新增列=升 v8，v0.5 不做）。

## 5. 安全/兼容

- reexec 只读工具，不写 workspace、不 spawn bash；仍受 jail（read/grep/glob/diff）约束。
- 默认（recorded）逐字节不变；无 reexec flag 零额外进程/IO（除已有 recorded 输出）。
- trace v7 不变；不新增表/列/事件；无 v8。
- `REPLAY_TOOL_RECORD_MISMATCH` 从「recorded JSON/sha 不符」扩展为「reexec 结果与记录不符」；文档/00 同步（同码，不新增）。

## 6. AC-25（设计冻结；F1 实测）

| # | 断言 |
|---|---|
| a | 默认 `--replay-tools=recorded`：不执行任何工具（spawn=0/workspace 不变）；与 v0.4 输出逐字节一致 |
| b | 显式 reexec：read/glob/grep 结果一致 → exit 0（replay_mode=reexec）；输出 additive |
| c | 文件/结果漂移：reexec 结果 ≠ 记录 → `REPLAY_TOOL_RECORD_MISMATCH` exit 3（不写 runs.error_code） |
| d | 副作用/非白名单工具 → exit 2；不执行、不降级 recorded |
| e | truncated/缺记录 → 对应 `REPLAY_TOOL_RECORD_*` fail-closed |
| f | 未开 `--allow-builtin-tools`/workspace 不一致 → 配置/用法错误；无 v8 迁移 |

## 7. Lead 裁决项

| # | 事项 | 推荐 |
|---|---|---|
| 1 | grep/glob 漂移口径 | **结果 sha256 比对，mismatch fail-closed**（不升 v8）；备选：仅 read/diff 可 reexec |
| 2 | reexec 环境一致性 | 要求显式同 workspace/allow-path；不做 trace 自动恢复（避免 v8） |
| 3 | 拒绝面 exit 码 | 副作用工具/未 opt-in → **exit 2**；记录/mismatch → **exit 3** |
| 4 | 输出标注 | additive `replay_mode`/`drift`，不改既有字段 |
