# REV-109 · trace · v0.4 D5a trace schema v7 + `tool_calls`

> **跨模块声明**：本分支改动 `crates/zgraph-trace/**`（主体）与 CLI 机械 arm/既有测试版本期望（`crates/zgraph-cli/src/{bridge,run_cmd}.rs`、cli tests、e2e 版本/备份期望），Lead 已批准、第二评审 Lead 追加；**未改 kernel**、无 cli 行为变更。评审人只新增本评审记录，未改实现/测试。

- **分支**：`feat/trace-v7`
- 分支: feat/trace-v7
- **作者**：`trace-dev`（提交 `ea6f657`；正文 `Review: REV-109` / `Agent: trace-dev`）
- **评审人**：`reviewer-a`（独立评审，≠ 作者）
- **基于**：main `4f27d51`（`main = origin/main = 4f27d51`）
- **变更范围**：`4f27d51..ea6f657`（**21 files, +1730/−114**）：`crates/zgraph-trace/src/{schema,model,reader,lib,toolcall(新增 326),writer/{tx,thread}}`、`tests/migration.rs`、`tests/tool_calls.rs(新增 403)`、trace Cargo；CLI `bridge.rs`/`run_cmd.rs` 机械 arm、cli tests 版本期望 6→7/7→8、e2e 版本期望/备份份数
- **评审时间**：2026-11-02
- **关联**：task-83/84；DR-014 §1/§2/§5/§10、06 §6.15、00 §0.4/§0.5、07 AC-24；REV-105（D1 冻结）、REV-107（D3）

## 1. 结论

**结论：通过**

> **结论历史**：首轮（基准 `ea6f657`）为「**有条件通过**」——P1 C1（reader 排序 `tool_call_id` vs `call_id`）、C2（`error_json` 无全量 sha256），P2 C3（唯一键是否含 node_id 待裁决）。作者 `90b6738` 关闭 C1/C2/P3，Lead 裁决 C3 以 **DR-014 §1** 为准（唯一键不含 node_id）；评审人复核 trace 113/0、migration 21/0、tool_calls 10/0、workspace 1151/0/3、clippy/fmt/check 全绿，C1–C3 全部 closed（§6/§8），结论改**通过**。
>
> 首轮意见（历史保留）：P0=0；P1 条件 C1–C2；P2 C3；P3 若干建议；DDL/迁移/非事件路径/writer 主体成立，但两处口径偏差需关闭。

### 1.1 已核对成立

| 项 | 结论 |
|---|---|
| schema v7 | ✅ `TRACE_SCHEMA_VERSION=7`；`tool_calls` 仅新增表+3 索引；字段/`source` 三值 CHECK/`status` 四值 CHECK/单一 `truncated`+`bash_truncated`/无 FK/唯一键 `(run_id,instance_id,attempt,call_id)`/`bash_signal`；与 DR-014 §1 逐列核对（见 §4） |
| 迁移 | ✅ v6→v7 仅加表/索引、单事务、备份恰一份 `.bak.v6-*`、备份保持 v6/无 tool_calls；v1..v6→v7 链逐版本备份（1→v7 六份、v2→v7 五份、v3→v7 四份…）、`schema_migrations 1..7`、旧表/旧行逐列保留、tool_calls 空；只读 v6 拒绝（exit 4、不迁移/不建表）、>7 TOO_NEW；失败注入恢复；fresh vs migrated DDL/列集合一致（migration 21/21） |
| 类型/API | ✅ `ToolCallRecord`/`ToolSideEffect`/`BashToolCall`/`ToolCallSource`/`ToolCallStatus`；`TraceInput::ToolCall` 走快照非事件路径（`writer/thread.rs` 不产生 events 行，`tool_call_roundtrip...without_events_rows`）；`TraceReader::tool_calls` 只读 |
| writer | ✅ 同批事务 `write_tool_call`；`ON CONFLICT(run_id,instance_id,attempt,call_id) DO NOTHING` 幂等；单 run 10000 行上限达到后停止记录（不 fail run）；重复 call_id 不占预算；`tool_call_rows` 按 run 从 DB 初始化 |
| 截断/上限 | ✅ args/result/error ≤32 KiB prefix+`truncated`+args/result 全量 sha256；bash stdout/stderr 16 KiB head+tail+`bash_truncated`；command 8 KiB、path/cwd 4 KiB 防御性截断；无 blobs/外部文件；`toolcall.rs` 单测 sha256/UTF-8 边界/短记录/超限/bash/path |
| 跨模块机械 | ✅ CLI `run_cmd` lifecycle `ToolCall→None`、bridge observer 测试 arm；cli decide/版本 6→7/8；e2e v5/v3 迁移 user_version/备份份数/expected 7 机械同步；无 kernel 改动、无行为变更、无断言放宽（仅版本/备份期望随 schema 升级） |
| 门禁 | ✅ trace **112/0**；migration 21/0；tool_calls 9/0；workspace **1150/0/3**；clippy `-D warnings`/fmt/check.sh 全绿 |

## 2. 四维评审

| 维度 | 结论 | 依据 |
|---|---|---|
| 功能正确性 | ⚠️ 有条件 | DDL/迁移/writer/截断/非事件路径主体正确；**C1** reader 排序用 `tool_call_id` 而非预裁决 `call_id`；**C2** `error_json` 超限截断但无全量 sha256（args/result 有）；C3 唯一键 node_id 口径与 task-84 不一致（实现按 DR-014 无 node_id 且有幂等测试） |
| 代码结构合理性 | ✅ | `toolcall.rs` 独立归一化；schema DDL/迁移与既有版本一致；model/reader/tx 增量最小；writer 不改 events 路径；跨模块仅机械 arm/期望 |
| 逻辑性 | ✅（C1/C2 例外） | 事务/幂等/行数上限/备份恢复/链式迁移/截断 UTF-8 边界自洽；错误路径 fail-stop；排序/隔离/空 tool_calls 有界；C1/C2 为口径偏差而非状态机错误 |
| 可读性 | ✅（P3 备注） | DDL/字段注释/provider 原生 call_id/不伪造 #n 说明清晰；测试分组（migration/tool_calls/toolcall 单测）；P3 主要为 writer 超限归一化顺序/测试名 |

## 3. 发现清单

### 3.1 P1 条件（D5a 内关闭）

| # | 位置 | 问题 | 建议 |
|---|---|---|---|
| **C1** | `crates/zgraph-trace/src/reader.rs:571`（`ORDER BY started_at ASC, tool_call_id ASC`）；`tests/tool_calls.rs:307` 测试名/断言「按 (started_at, tool_call_id)」 | **reader 排序与 Lead/task-84 预裁决 `(started_at,call_id)` 不一致**：实现用自增行 id 做同 `started_at` 的 tie-break（插入顺序），而非原生 `call_id`。同一时刻并发/乱序写入时结果顺序与契约不同；测试也锁定了错误口径 | 改为 `ORDER BY started_at ASC, call_id ASC`；更新测试名/断言（同 `started_at` 用 call_id 可判定的数据）；若 Lead 确认以 tool_call_id 为准，需在 DR/06 明确并改 task 口径（不建议，原生 call_id 更符合 DR） |
| **C2** | `crates/zgraph-trace/src/schema.rs` DDL（无 `error_sha256`）；`toolcall.rs` normalize `error_json` 截断但只算 args/result sha；`reader.rs ToolCallRow` 无 error sha | **`error_json` 超限仅 prefix+`truncated`，无全量 sha256**；task-84/Lead 口径为「args/result/error 32 KiB + sha256」。error 被截断后无法证明/校验完整错误内容（D5b 回放/审计会缺证据） | 增加 `error_sha256` 列（DDL/model/ToolCallRow/writer/reader/normalize）+ 测试；若 Lead 裁决 error 不需要 sha（DR-014 §1 无该列），请明确记录并作为 P3 文档化（本条件按裁决关闭） |

### 3.2 P2（建议 D5a 内裁决，若按 DR 可登记 ISSUE）

| # | 位置 | 问题 | 建议 |
|---|---|---|---|
| C3 | `schema.rs` `UNIQUE (run_id,instance_id,attempt,call_id)`；`tests/tool_calls.rs:278-302` 断言「node 不同但同 instance/attempt/call_id 仍幂等」 | **唯一键与 task-84 描述 `(run_id,instance_id,attempt,node_id,call_id)` 不一致**；实现严格按 DR-014 §1（无 node_id），测试也明确锁定。若 instance_id 实际不唯一到 node，可能把不同节点同 call_id 的审计行幂等丢弃；若 instance_id 已含 node 则无数据损失 | 请 Lead 明确唯一键权威（DR-014 §1 vs task-84）；若需 node_id，改 DDL/ON CONFLICT/迁移测试（schema v7 尚未发布，改动成本低）；若按 DR，记录口径并保留现有幂等测试 |

### 3.3 P3（建议）

- **P3-1**：`write_tool_call` 先 `normalize` 再检查 10000 行上限——超限记录仍会序列化/哈希完整 args/result（大参数下浪费 CPU/内存）；建议先查计数再归一化（或仅在超限时跳过）。
- **P3-2**：`ToolCallRow.tool_call_id` 注释/测试名仍以 row id 为稳定排序键；C1 修复后同步注释。
- **P3-3**：`error_json`/`args_json` 截断后是非法 JSON（prefix），`trace`/D5b 消费方需 fail-closed；建议在 reader/trace 输出文档明确（截断即不可解析）。
- **P3-4**：`tool_calls` 无 FK（DR 原文接受）；run 删除清理语义 D6/后续裁决，建议在 06 §6.15 标注孤儿清理策略。
- **P3-5**：`bash_command` 防御性 head+tail 截断（工具层应先拒绝）已由 Lead 预裁决；D6/05 说明「工具层拒绝优先、trace 防御」。
- **P3-6**：trace 侧常量命名（`TOOL_CALL_*`）D6 回写 00 §0.5；`ToolSource::Framework` 不落表（D5b/DR §4）。
- **P3-7**：测试强度：migration 21、tool_calls 9、toolcall 6 已覆盖；可补「error 截断+sha（C2 后）」「同一 started_at 不同 call_id 乱序写入（C1）」「10000 行跨多 batch 连续计数」用例。

## 4. 验证证据

评审人亲自执行（worktree `.worktrees/trace-trace-dev`，HEAD `ea6f657`，main `4f27d51`，worktree 干净；`CARGO_HOME=<repo>/.cargo-home`）：

```bash
cargo test -p zgraph-trace                    # 112 passed / 0 failed
cargo test -p zgraph-trace --test migration   # 21 passed / 0 failed
cargo test -p zgraph-trace --test tool_calls  # 9 passed / 0 failed
cargo test --workspace                        # 1150 passed / 0 failed / 3 ignored
cargo clippy --workspace --all-targets --locked -- -D warnings  # clean
cargo fmt --all -- --check                    # exit 0
bash scripts/dev/check.sh                     # CHECK PASSED
```

- **DDL 对照**：`PRAGMA table_info`/`sqlite_master` 查 fresh v7 与 v6→v7 migrated：列集合/顺序、3 索引、`tool_call_id INTEGER PRIMARY KEY AUTOINCREMENT`、`source` CHECK 三值、`status` CHECK 四值、`truncated`/`bash_truncated` CHECK、`UNIQUE(run_id,instance_id,attempt,call_id)`、无 FK——与 DR-014 §1 一致；v6→v7 不重建旧表（旧表 schema/行逐列保留）。
- **迁移**：v6→v7 备份恰一份、备份 user_version=6/无 tool_calls；v1→v7 六份、v2→v7 五份、v3→v7 四份、v4→v7 三份、v5→v7 两份、v6→v7 一份；`schema_migrations 1..7`；只读 v6 `<7` exit 4 不迁移不建表；>7 既有 TOO_NEW 测试；失败注入恢复。
- **截断/排序/幂等**：`toolcall.rs` 单测 sha256 known vectors、UTF-8 prefix/head+tail 边界、短记录全量 hash；`tests/tool_calls.rs` args/result/error caps、bash head+tail/exit/signal/profile、side effect、source/status 闭集、幂等、10000 行上限、无 tool_calls 空行。**C1/C2 为与 task/Lead 口径不一致处**（非测试红灯）。
- **范围**：`git diff --name-only` 21 文件；无 kernel；CLI 仅机械 arm/期望；e2e 版本/备份期望同步；无断言放宽（仅 schema 版本/备份份数随 v7 变化）。
- **对比自报**：trace 112/0、workspace 1150/0、migration/tool_calls 子集、clippy/fmt/check 与自报一致。

## 5. 条件关闭方式

- **C1/C2 由 trace-dev 在 D5a 内小 commit 关闭**：C1 改 reader 排序+测试；C2 加 `error_sha256`（或 Lead 明确按 DR 无 error sha 的例外并记录）。
- **C3 由 Lead 裁决**：明确唯一键以 DR-014 §1（无 node_id）或以 task-84（含 node_id）为准；若改，D5a 内同步 DDL/ON CONFLICT/测试。
- 关闭后评审人只读复核 + 跑 trace/migration/tool_calls/workspace 门禁，追加「条件关闭」段；全部关闭后结论改「通过」，再 merge D5a → D5b。

## 6. 条件关闭记录（"有条件通过"）

| 条件 | 状态 | 关闭要求 |
|---|---|---|
| C1 reader 排序 `(started_at,call_id)` | **closed（90b6738）** | `reader.rs` `ORDER BY started_at ASC, call_id ASC`；测试改为同 started_at 乱序 z→a 插入、断言 call_id 升序；trace 113/0 |
| C2 error_json 全量 sha256 | **closed（90b6738）** | 新增 `error_sha256 TEXT`（v7 DDL/model/reader/writer/normalize/测试；error 超 32 KiB 仍 prefix+`truncated`，sha 为全量）；fresh-vs-migrated 列清单补该列；roundtrip/超限测试 |
| C3 唯一键是否含 node_id | **closed（Lead 裁决：DR-014 §1 优先）** | 唯一键保持 `(run_id,instance_id,attempt,call_id)` **不含 node_id**；`schema.rs` DDL 注释标注该裁决；幂等测试保留 |

## 7. 遗留问题

- D5b（harness sink + cli trace/replay）可开（C1–C3 已关闭）；`error_sha256`/call_id 排序/唯一键口径已在 D5a 固化。
- 跨模块 CLI/e2e 版本机械同步已核对无行为变更/无断言放宽；由 Lead 追加第二评审。
- P3：writer 超限先归一化已于 `90b6738` 修复（上限判定前移+跨 batch 测试）；截断 JSON 不可解析文档、无 FK 清理语义、D6 常量命名留 D6；均不阻塞。
- 无 P0；无既有事件/旧 run golden 回归（workspace 全绿）。

## 8. 条件关闭复核（2026-11-02）

复核提交：`90b6738`（父 `4fb1c0c`；仅 trace 6 文件，`+98/−40`，无新增跨模块）。评审人执行：

```bash
git status --porcelain                       # 空
git diff --stat 4fb1c0c..90b6738             # 6 files +98/−40（仅 crates/zgraph-trace；无 cli/e2e/kernel）
cargo test -p zgraph-trace                   # 113 passed / 0 failed
cargo test -p zgraph-trace --test migration  # 21 passed / 0 failed
cargo test -p zgraph-trace --test tool_calls # 10 passed / 0 failed
cargo test --workspace                       # 1151 passed / 0 failed / 3 ignored
cargo clippy --workspace --all-targets --locked -- -D warnings  # clean
cargo fmt --all -- --check                   # exit 0
bash scripts/dev/check.sh                    # CHECK PASSED
```

| 条件 | 复核结果 |
|---|---|
| C1 reader 排序 | ✅ `ORDER BY started_at ASC, call_id ASC`；同 started_at z→a 乱序测试断言 call-early/call-same-a/call-same-z/call-late |
| C2 error sha | ✅ `error_sha256` 列入 v7 DDL（fresh/migrated 列清单测试）、model/reader/writer/normalize；error roundtrip 与超限全量 sha 测试 |
| C3 唯一键 | ✅ Lead 裁决以 DR-014 §1 为准：`(run_id,instance_id,attempt,call_id)` 不含 node_id；DDL 注释已注明，幂等测试保留 |
| P3 writer 上限前置 | ✅ 10000 行判定在 normalize 前；跨 batch 上限测试（flush 后继续计数） |

**最终状态**：C1–C3 closed；P3 主要项 closed/留下 D6 文档；无 P0/P1；建议 Lead merge D5a → D5b。

## 9. 第二评审 · 跨模块（cli/e2e/Cargo 机械）（Lead 追加，2026-11-02）

- **评审人**：`lead`（**≠ 作者** `trace-dev`；Lead 已批准跨模块机械改动，按 AGENTS §2 追加第二评审）
- **范围**（`4f27d51..fe1111f`）：
  1. `crates/zgraph-cli/src/bridge.rs`：`TraceInput::ToolCall` observer kind arm；
  2. `crates/zgraph-cli/src/run_cmd.rs`：`lifecycle_detail` 新变体 → `None`（非 events 行）；
  3. `crates/zgraph-cli/tests/decide.rs`：schema 版本期望 6→7、`newer_schema` 注入 7→8；
  4. `tests/e2e/{dynamic_fanout,gate_defer,human_decide,mcp_v03,v03_regression}.rs`：schema 版本期望 v7 与备份份数机械同步；
  5. `crates/zgraph-trace/Cargo.toml` + `Cargo.lock`：`sha2`（trace 侧 sha256）。
- **核对结论（通过）**：仅编译/版本期望/测试适配，零行为变化、未放宽断言、未改 kernel；CLI 默认输出/退出码不变；e2e 期望仅随 schema v7 更新。
- **Lead 亲自验证**：`git diff 4f27d51..fe1111f --stat` 仅 trace 源/测试 + cli 2 文件 + cli tests + 5 e2e + Cargo/lock（+ REV 记录）；`cargo test --workspace --offline` = 1151/0/3；`cargo test -p zgraph-trace --offline` = 113/0。
- **裁决/后续**：唯一键按 DR-014 §1（不含 node_id）；无 FK 清理语义、截断 JSON 文档、trace 常量命名留 D5b/D6；D5b 使用同口径 sha256 序列化。
- **结论**：**通过（第二评审）**。分支满足合并前置。
