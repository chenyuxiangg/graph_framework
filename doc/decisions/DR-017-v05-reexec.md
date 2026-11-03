# DR-017 · v0.5 reexec 工具回放

> **状态**：V1 设计冻结（REV-115；REV-122 对齐 F1c 实测：**行存在但字段缺失/损坏 → MISMATCH；MISSING 仅行缺失/streaming 多于记录**）。
> **日期**：2026-11-03 · **作者**：docs-dev · **关联**：DR-014（v7 `tool_calls`）、06 §6.15、07 AC-24/25、task-95/105
> **合同**：`--replay-tools=reexec` **显式**（默认 `recorded` 不变）；仅只读 builtin 候选重跑；**副作用/MCP/Python/失败行复用记录、绝不执行、不报 exit2**；不升 trace v8。

## 0. 摘要

| 项 | 冻结 |
|---|---|
| 默认 | `--replay-tools=recorded`：**绝不执行任何工具**；v0.4 行为逐字节不变 |
| 显式 reexec | `--mode=tools --replay-tools=reexec`；逐行决定 |
| **候选行** | **`source=builtin` + `read`/`grep`/`glob`/`diff` + `status=ok`** → 重跑并比对 `result_sha256` |
| **非候选行** | 副作用（write/edit/apply_patch/bash）/ MCP / Python / `status!=ok` → **复用记录结果、绝不执行、不报 exit2** |
| 输出 | recorded 逐字节不变；reexec additive `replay_mode`（reexec/recorded 逐行）+ `drift=false`；mismatch 直接 exit3、不输出 `drift=true` |
| exit2 | **仅**缺 `--allow-builtin-tools` 或显式 `--workspace-dir`（用法错误） |
| exit3 | 非法 workspace → `HARNESS_CONFIG_LOAD`；记录缺失/截断/不符 → `REPLAY_TOOL_RECORD_*` |
| 不做 | 不新增 trace 列/表（不升 v8）；不重跑 MCP/Python/外部 harness/副作用工具 |

## 1. reexec 语义（对齐 F1b）

- **显式开关**：`--mode=tools --replay-tools=reexec`；`recorded`（缺省）保持 v0.4 行为。
- **逐行处理**：对每条 `tool_calls` 记录决定「重跑」或「复用」：
  - **仅当** `source == builtin` **且** `tool_name ∈ {builtin__read, builtin__grep, builtin__glob, builtin__diff}` **且** `status == ok` → 用记录 args 重跑只读工具，比对 `result_sha256`；
  - **其余全部**（副作用 write/edit/apply_patch/bash、MCP、Python、`status != ok`）→ **复用记录结果**，**不执行**，不报用法错误（exit 2 不适用于这些行）。
- **exit 2 仅两种**：缺 `--allow-builtin-tools`；缺**显式** `--workspace-dir`（reexec 不默认 cwd）。二者为用法错误。
- **非法 workspace**（不存在/非法/allow-path 非法）→ `HARNESS_CONFIG_LOAD`（exit 3），不降级 recorded。
- **执行环境**：只读 builtin 实现；jail/allow-path 与当前 CLI 配置一致；不写 workspace、不 spawn bash、不写新 trace。
- **不做**：MCP/Python/外部 harness 工具永不 reexec（即使只读）；副作用工具永不 reexec。

## 2. 漂移校验与预检（对齐 F1b）

v7 `tool_calls` 已有 `args_sha256`/`result_sha256`，无输入文件 pre_sha256（除副作用工具）；reexec 以记录 `result_sha256` 为期望值：

| 场景 | 结果 |
|---|---|
| 候选行重跑成功且 result sha256 一致 | 该行 `replay_mode=reexec`；exit 0 |
| 候选行重跑结果 ≠ 记录 | `REPLAY_TOOL_RECORD_MISMATCH` exit 3（**不输出 drift=true**） |
| 候选行记录 `result_sha256` 缺失/字段损坏 | **`REPLAY_TOOL_RECORD_MISMATCH`（exit 3；记录损坏/不可复核）**（REV-122 裁决） |
| 记录行缺失 / streaming `tool_call_start` 多于记录 | `REPLAY_TOOL_RECORD_MISSING` exit 3 |
| 任一记录 `truncated=1` | `REPLAY_TOOL_RECORD_TRUNCATED` exit 3 |
| `args_json` 非法 / result JSON 不可解析 / sha 不符 | `REPLAY_TOOL_RECORD_MISMATCH` exit 3 |

- **公共 API**：重跑结果 sha256 经 `zgraph_trace::tool_result_sha256`（公共 API，与记录同口径）；**不新增列/表、不升 v8**。
- **错误码区分（REV-122 裁决）**：**行缺失**（无记录 / streaming `tool_call_start` 多于记录行）→ `REPLAY_TOOL_RECORD_MISSING`；**行存在但字段缺失/损坏**（缺 `result_sha256`、JSON 不可解析、sha 不符）→ `REPLAY_TOOL_RECORD_MISMATCH`；`truncated=1` → `REPLAY_TOOL_RECORD_TRUNCATED`。
- **不做**：不因 grep/glob 无输入 hash 就跳过比对；不引入 pre_sha 新列。

## 3. CLI/输出语义

| 场景 | 结果 |
|---|---|
| reexec + 候选一致 + 非候选复用 | exit 0；reexec additive：人类行尾 `replay_mode=<reexec|recorded> drift=false`；`--json` 逐行加 `replay_mode`/`drift` 键 |
| 记录/漂移错误 | 直接 exit 3；**不输出 `drift=true`**（错误信息给 reason/call_id/tool） |
| 缺 `--allow-builtin-tools` / 显式 `--workspace-dir` | 用法错误 exit 2（不产生 run/DB） |
| 非法 workspace | `HARNESS_CONFIG_LOAD` exit 3 |
| 默认 recorded | 输出逐字节不变（无 replay_mode/drift 键/行尾） |

- **不新增顶层错误码/CLI 码**：复用 `REPLAY_TOOL_RECORD_*`；exit 码沿用 2/3。
- 人类行 `tool_calls: N` 仅 reexec 模式文案变化；recorded 不变。

## 4. workspace/jail 一致性

- reexec 必须显式传 `--workspace-dir`（不默认 cwd）与可选 `--allow-path`；缺 workspace → exit 2；非法 → exit 3。
- 不同 workspace/allow-path 会产生不同结果 → 由结果 mismatch fail-closed（不静默）。
- 不做「自动从 trace 恢复 workspace 路径」（v7 未记录运行根；新增列=升 v8，v0.5 不做）。

## 5. 安全/兼容

- reexec 仅重跑只读 builtin；非候选一律复用记录、绝不执行（含 MCP/Python/外部 harness/副作用工具）。
- 默认 recorded 逐字节不变；无 reexec flag 零额外进程/IO。
- trace v7 不变；不新增表/列/事件；无 v8。
- `REPLAY_TOOL_RECORD_MISMATCH` 扩展语义：recorded JSON/sha 不符 + reexec 结果漂移（同码）。

## 6. AC-25（设计冻结；F1b 已实现，完整 e2e 由 F1c 覆盖）

| # | 断言 |
|---|---|
| a | 默认 `--replay-tools=recorded`：不执行任何工具（spawn=0/workspace 不变）；与 v0.4 输出逐字节一致 |
| b | 显式 reexec：候选一致 → exit 0；**reexec additive**：人类行尾 `replay_mode/drift=false`、`--json` 逐行 `replay_mode/drift`；非候选行 `replay_mode=recorded` |
| c | 文件/结果漂移：reexec 结果 ≠ 记录 → `REPLAY_TOOL_RECORD_MISMATCH` exit 3（不写 runs.error_code；不输出 drift=true） |
| d | **非候选复用**：副作用/MCP/Python/失败行 → 复用记录、**绝不执行、不报 exit2**；仅缺 `--allow-builtin-tools`/显式 `--workspace-dir` → exit 2；非法 workspace → CONFIG_LOAD exit 3 |
| e | truncated/缺行/streaming 多于记录 → `REPLAY_TOOL_RECORD_TRUNCATED`/`MISSING` fail-closed |
| f | 行存在但字段缺失/损坏（缺 `result_sha256`/JSON 坏/sha 不符）→ `REPLAY_TOOL_RECORD_MISMATCH`；**MISSING 仅用于行缺失/streaming 多于记录**；无 v8 迁移；完整 e2e 由 F1c 覆盖 |

## 7. Lead 裁决项

| # | 事项 | 推荐/状态 |
|---|---|---|
| 1 | 候选行缺 `result_sha256` 的码 | **已裁决（REV-122）：`REPLAY_TOOL_RECORD_MISMATCH`（行存在但字段缺失/损坏，记录不可复核）；MISSING 仅行缺失/streaming 多于记录** |
| 2 | 非候选行退出行为 | **复用记录、绝不执行、不报 exit2**（已实现） |
| 3 | exit2 范围 | 仅缺 `--allow-builtin-tools`/显式 `--workspace-dir` |
| 4 | 输出 additive | reexec 模式加 `replay_mode`/`drift=false`；recorded 不变；mismatch 不输出 drift=true |
| 5 | trace API | `zgraph_trace::tool_result_sha256`；不升 v8 |
