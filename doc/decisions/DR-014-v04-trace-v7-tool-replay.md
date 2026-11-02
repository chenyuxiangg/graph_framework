# DR-014 · v0.4 trace v7 与工具级回放

> **状态**：V1 设计冻结（REV-105；字段上限/迁移细节按推荐冻结，Lead 裁决项见 §9）。
> **日期**：2026-11-02 · **作者**：docs-dev · **关联**：DR-012、DR-013、06 §6.2/§6.7/§6.14、07 AC-24、task-74
> **合同**：schema **v7** + 新表 `tool_calls`；默认 `--replay-tools=recorded`（**副作用永不重跑**）；`reexec` 仅只读工具。

## 0. 摘要

| 项 | 冻结 |
|---|---|
| schema | `TRACE_SCHEMA_VERSION = 7`；只新增 `tool_calls` 表 + 索引 + 迁移；既有表/事件不变 |
| 表键 | `(run_id, instance_id, attempt, call_id)` 唯一（`call_id` 为 provider/tool 原生 id） |
| 字段 | tool_name、args_json、result_json、status、error_json、started_at/finished_at、truncated、副作用（path/before/after sha256/bytes）、bash（command/cwd/exit/stdout/stderr/沙箱摘要） |
| 上限 | args/result 64 KiB、stdout/stderr 32 KiB、command 8 KiB、path 4 KiB；超限 truncated+sha256；**回放遇截断 fail-closed** |
| 迁移 | v6→v7：备份 → 单事务建表/索引 → `user_version=7`；只读库拒绝；链式 v1..v6→v7 |
| trace 输出 | 人类新增工具事件行；`--json` 新增 `tool_calls` 数组（只读、additive） |
| 回放 | `--replay-tools=recorded`（默认）：所有工具从记录回放，**绝不执行副作用**；`reexec` 仅 `read`/`grep`/`glob`/`diff` |
| 默认不变 | 无工具调用：v7 行为与 v6 一致（除 `user_version`）；L2 `recorded_outputs` 不变 |

## 1. `tool_calls` 表（DDL 草案）

```sql
CREATE TABLE IF NOT EXISTS tool_calls (
  tool_call_id     INTEGER PRIMARY KEY AUTOINCREMENT,
  run_id           TEXT NOT NULL,
  instance_id      TEXT NOT NULL,
  node_id          TEXT NOT NULL,
  attempt          INTEGER NOT NULL,
  call_id          TEXT NOT NULL,              -- provider/LLM 原生 tool call id；不伪造
  tool_name        TEXT NOT NULL,              -- builtin__read / mcp__s__t / python name
  source           TEXT NOT NULL CHECK (source IN ('builtin','mcp','python')),
  status           TEXT NOT NULL CHECK (status IN ('ok','error','cancelled','timeout')),
  args_json        TEXT NOT NULL,              -- 完整参数（≤ cap；超限 prefix + truncated=1）
  result_json      TEXT,                       -- 成功结果（≤ cap）；error 时 NULL
  error_json       TEXT,                       -- TOOL_ERROR JSON（≤ cap）
  truncated        INTEGER NOT NULL DEFAULT 0 CHECK (truncated IN (0,1)),
  args_sha256      TEXT,                       -- 完整参数 hash（截断时校验）
  result_sha256    TEXT,                       -- 完整结果 hash
  started_at       TEXT NOT NULL,
  finished_at      TEXT NOT NULL,
  duration_ms      INTEGER NOT NULL,
  -- 副作用（write/edit/apply_patch；其他 NULL）
  side_effect_path     TEXT,
  side_effect_before   TEXT,                   -- sha256 before（不存在 = NULL）
  side_effect_after    TEXT,                   -- sha256 after
  side_effect_bytes_before INTEGER,
  side_effect_bytes_after  INTEGER,
  -- bash（仅 builtin__bash）
  bash_command     TEXT,
  bash_cwd         TEXT,
  bash_exit_code   INTEGER,
  bash_stdout      TEXT,
  bash_stderr      TEXT,
  bash_truncated   INTEGER NOT NULL DEFAULT 0 CHECK (bash_truncated IN (0,1)),
  sandbox_profile  TEXT,                       -- 摘要/版本（不写完整 profile）
  UNIQUE (run_id, instance_id, attempt, call_id)
);

CREATE INDEX IF NOT EXISTS idx_tool_calls_run       ON tool_calls(run_id, started_at);
CREATE INDEX IF NOT EXISTS idx_tool_calls_instance  ON tool_calls(run_id, instance_id, attempt);
CREATE INDEX IF NOT EXISTS idx_tool_calls_name      ON tool_calls(run_id, tool_name);
```

- **写入者**：trace 写线程（既有双通道），每次工具调用一行；工具执行与写入解耦（失败不阻塞 run，除非 trace 写入硬错误按既有口径）。
- **call_id**：保留 provider/tool 原生 id；不伪造 `#n`（与 v0.3 裁决一致）。
- **多文件 apply_patch**：一行 `tool_calls`（结果含 `files[]` 摘要），不拆多行；`side_effect_path` 仅单文件时填（多文件用 result_json）。
- **args 完整 cap**：v7 要求「完整 args/result（cap 内）」；超 cap → prefix+truncated+sha256；回放截断 fail-closed（§5）。
- **敏感信息**：env/headers/token 不写；既有脱敏（`SENSITIVE_KEY_TOKENS`）复用；bash command 可能含密钥 → 文档警示 + 不写 env 值。

## 2. 迁移（v6→v7）

- **备份**：迁移前复制为 `<db>.bak-v6-<ts>`（与既有 v5→v6 同口径）；失败可恢复。
- **单事务**：`BEGIN` → `CREATE TABLE tool_calls` + 3 索引 → `schema_migrations` 审计行 → `PRAGMA user_version=7` → `COMMIT`；失败回滚+保留备份。
- **只读库**：不可迁移 → 既有 `STORE_SCHEMA_TOO_NEW`/只读拒绝口径；不自动降级。
- **链式**：v1..v6 → v7 逐步迁移（每步独立事务/备份策略沿用 06 §6.7）；v7 之后旧二进制 → 只读拒绝。
- **无工具调用**：迁移只加空表；既有行/事件/`recorded_outputs` 不变；`trace` 读取输出与 v6 一致（`tool_calls` 空数组/不显示）。
- **回滚**：不支持降级（v7→v6）；备份仅供人工恢复。

## 3. trace 输出

- **人类可读**：每个工具调用一行/段（`tool_name`、status、duration、truncated、关键参数摘要、bash exit）；不打印完整 args/result（避免终端泄漏）。
- **`--json`**：新增顶层 `tool_calls` 数组（仅该 run；additive），字段与表一致（截断字段 + `truncated`）；消费方可忽略。
- **既有输出**：`events`/`streaming_events`/`llm_calls`/`recorded_outputs` 不变；`StreamingEvent::ToolCallStart/End` 继续（UI 流式），`tool_calls` 是**审计/回放**补充。
- **`--stats`**：可选新增 `tool_calls` 计数（additive；不改变既有字段）。

## 4. 回放语义（`--replay-tools`）

| 模式 | 行为 |
|---|---|
| `recorded`（默认） | **所有**工具调用从 `tool_calls` 回放（args 校验/结果还原）；**副作用工具永不执行**；无记录 → fail-closed（`REPLAY_TOOL_RECORD_MISSING`/既有 `REPLAY_RECORD_MISSING`） |
| `reexec` | 仅**只读工具** `builtin__read`/`grep`/`glob`/`diff` 可重新执行（同 args、同 jail/沙箱配置）；`write`/`edit`/`apply_patch`/`bash` **仍 recorded**（或 fail-closed，推荐 recorded）；需显式开启 + 文档警示 |

- **recorded 校验**：args hash/results 还原；若 `truncated=1` → **fail-closed**（`REPLAY_TOOL_RECORD_TRUNCATED`，新 CLI 码或既有 `REPLAY_RECORD_CORRUPT`；推荐新码 append-only）；不猜测/不重跑。
- **reexec**：仍受 workspace/jail/沙箱约束；结果与记录不一致 → 告警/失败（推荐 `REPLAY_TOOL_MISMATCH` 或既有 `REPLAY_OUTPUT_MISMATCH` 口径）；不写新 trace。
- **L2 关系**：节点级 `recorded_outputs` 仍是主回放（`--mode=outputs`）；`tool_calls` 供细粒度审计/工具级回放；`--mode=events` 不执行工具。
- **外部 harness**：v0.3 外部 harness 工具不落 `tool_calls`（协议边界）；其 L2 用 `recorded_outputs` 不变。

## 5. 截断/上限策略

| 字段 | cap | 超限 |
|---|---|---|
| args_json / result_json | 64 KiB | 头部 prefix + `truncated=1` + sha256；回放 fail-closed |
| bash_stdout/stderr | 32 KiB each | tail/head 截断 + `bash_truncated=1`；回放 fail-closed（bash 不 reexec 默认 recorded 需完整） |
| bash_command | 8 KiB | 超限拒绝（工具错误） |
| side_effect_path | 4 KiB | 超限拒绝 |
| 单 run `tool_calls` 行数 | 10000（推荐） | 超限停止记录 + warning（不 fail run） |

> **口径**：审计完整性优先；截断记录仍保留 hash，可证明「记录不完整」，回放不静默降级。

## 6. CLI/配置

- `--replay-tools=recorded|reexec`（缺省 `recorded`；`zgraph replay`/`zgraph trace` 可选）。
- `--workspace-dir`/`--allow-path`/`--allow-builtin-tools`/`--allow-bash`/`--allow-network` 见 DR-013；回放 reexec 必须同配置（记录 profile 摘要校验）。
- 退出码：复用 `REPLAY_*` 既有（3/1）；新增 tool record 缺失/截断/不一致的映射见 §7。

## 7. 错误码（推荐）

| 码 | 触发 |
|---|---|
| `REPLAY_TOOL_RECORD_MISSING`（CLI，append-only） | recorded 回放缺 `tool_calls` 记录 |
| `REPLAY_TOOL_RECORD_TRUNCATED`（CLI） | 记录 truncated=1，不能安全回放 |
| `REPLAY_TOOL_MISMATCH`（CLI） | reexec 结果与记录不一致 |
| `HARNESS_BUILTIN_TOOL_ERROR`（可选，不推荐） | 若 Lead 要求独立码；默认走 `TOOL_ERROR{reason}` |
| `HARNESS_CONFIG_LOAD{reason:"sandbox_unavailable"}` / `TOOL_ERROR{reason:"sandbox_unavailable"}`（DR-013；不新增码） | bash 沙箱不可用（fail-closed） |

- 默认不写 `runs.error_code`（CLI 命名空间）；退出码 3（记录类）/1（不一致）。

## 8. AC-24（设计冻结；D5 实测）

| # | 断言 |
|---|---|
| a | v6→v7 迁移：备份/单事务/`user_version=7`/旧表不变 |
| b | 只读库拒绝；链式 v1..v6→v7；损坏备份恢复 |
| c | 无工具调用：v7 trace/replay 与 v6 一致（`tool_calls` 空） |
| d | 每次工具调用恰 1 行；字段/状态/耗时/truncated 正确；call_id 原生 |
| e | 副作用记录：write/edit/apply_patch path + before/after sha256 + bytes |
| f | bash：command/cwd/exit/stdout/stderr/cap/profile 摘要 |
| g | `--replay-tools=recorded`：副作用不重跑（文件 hash 不变）；缺记录/截断 fail-closed |
| h | `reexec` 仅只读；副作用仍 recorded/fail-closed；mismatch 报错 |
| i | trace 人类/`--json` 工具行 additive；既有输出不变 |
| j | 默认（无工具）兼容矩阵不变 |

## 9. Lead 裁决项

| # | 事项 | 推荐 |
|---|---|---|
| 1 | 新 CLI 码命名 | `REPLAY_TOOL_RECORD_MISSING`/`REPLAY_TOOL_RECORD_TRUNCATED`/`REPLAY_TOOL_MISMATCH`（append-only） |
| 2 | 截断回放策略 | **fail-closed**（不猜测） |
| 3 | `reexec` 默认/集合 | 默认 `recorded`；reexec 仅 read/grep/glob/diff；显式开关 |
| 4 | 单 run tool_calls 行数上限 | 10000（超限停止记录 + warning） |
| 5 | 多文件 apply_patch 行粒度 | 1 行（`files[]` 摘要）；不拆多行 |
| 6 | `--json` 顶层键名 | `tool_calls`（additive） |
| 7 | bash 记录敏感 command | 记录 command（cap）；env 值不记录；文档警示 |
| 8 | 是否需要 `HARNESS_BUILTIN_TOOL_ERROR` | 不新增（`TOOL_ERROR{reason}` 足够） |
