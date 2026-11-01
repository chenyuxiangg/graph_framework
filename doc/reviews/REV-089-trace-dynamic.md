# REV-089 · trace · W4b schema v6（item 实例 / foreach_expansions / v5→v6 迁移）

> **跨模块声明（Lead 已批准，第二评审由 Lead 追加）**：本分支为 trace 模块（`crates/zgraph-trace/**`）；跨模块机械改动包括 `crates/zgraph-cli/src/bridge.rs`（`map_snapshot` 的 item 占位 0/None + observer 测试 kind）、`crates/zgraph-cli/src/run_cmd.rs`（`lifecycle_detail` 新变体 None arm）、`crates/zgraph-cli/tests/{decide,trace}.rs`（构造点/版本期望）、`tests/e2e/{gate_defer,human_decide}.rs`（版本期望/备份份数；`human_decide` 旧 run 逐列快照只看 v6 前历史列）。**未改 kernel 源码**；均为编译/版本/测试适配，默认（无 foreach）行为零变化。Lead 追加第二评审。

- **分支**：`feat/trace-dynamic`
- **作者**：`trace-dev`（单提交 `49806c2`，基于 main `42f93d7`；正文含 `Review: REV-089` / `Agent: trace-dev`）
- **评审人**：`reviewer-b`（独立评审，≠ 作者）
- **变更范围**：`42f93d7..49806c2`（18 files，+1423/−225）：trace src 6 文件（schema/writer/model/reader/lib）+ trace tests 6（migration/expansions/item_instances/api/common/decisions）+ cli 4（bridge/run_cmd/tests×2）+ e2e 2（gate_defer/human_decide）
- **评审时间**：2026-11-01
- **复审**：2026-11-01，HEAD **`dc25340`**（C1+N1–N4 修复提交，parent `570fd69`；仅 `crates/zgraph-trace/**` 3 文件，+686/−79）；C1+N1–N4 已关闭，结论升级为 **通过**（首轮不通过保留为历史，见 §7）
- **关联**：task-35（W4b 合同）/ task-37（作者任务）、DR-007 §8/§12、spec 06 §6.2.5/§6.2.15/§6.7/§6.10.7、spec 01 §1.2.5

## 1. 结论

**结论：通过**

**复审（2026-11-01，HEAD `dc25340`）：C1 + N1–N4 全部关闭，证据见 §7；下方首轮「不通过」记录保留为历史。** 复审无 P0/P1；残余 P3=4（§7.3，不阻断）。

> **首轮结论（2026-11-01，HEAD `49806c2`）：不通过**（P0=0；P1=1（C1）；P2=5；P3=4；已被上方复审取代，保留为历史）。首轮问题：真实 v1 库 v1→v6 迁移因 `DDL_CURRENT` 内 v6-only item 索引作用于无 item_index 的 v1 instances 而失败；v1 fixture 只建 3 表导致假绿；详见 §3 首轮记录。

## 2. 四维评审

| 维度 | 结论 | 依据 |
|---|---|---|
| 功能正确性/可实现性 | ✅（复审通过） | C1 已修复（v1 真实 fixture 通过）；v6 schema/API/writer/reader/expansion、v1/v5 迁移、FK/备份/human 行均实测；首轮阻断已关闭 |
| 代码结构合理性 | ✅（P2 备注） | schema 版本分支集中在 `schema.rs`；v6 DDL 独立 `DDL_FOREACH_EXPANSIONS`；FK 临时关闭封装在 `migrate_step`；writer 的 item COALESCE/expansion 非事件路径清晰；无 kernel 越界；仅有 P2 测试/文档细节 |
| 逻辑性 | ✅（复审通过） | v1→v6 与 v5→v6 12 步重建/唯一键/索引/备份/恢复/FK 窗口均实测；v6-only 索引已从 v1 复用的 DDL 拆出；无剩余逻辑阻断 |
| 可读性 | ✅（P3 备注） | 迁移注释解释 C1/FK CASCADE；测试注释列覆盖；P3 仅 `open_existing_readwrite_connection` 旧文案、keys API 语义、e2e legacy 列说明等（§7.3） |

## 3. 发现清单（首轮：P0=0/P1=1/P2=5/P3=4；C1+N1–N4 已由 `dc25340` 关闭，证据见 §7）

### P1 · 首轮阻断（已关闭）

| # | 位置 | 问题 | 修复建议 |
|---|---|---|---|
| **C1** | `crates/zgraph-trace/src/schema.rs:195`（`DDL_CURRENT` 的 `idx_instances_run_node_item`）、`:652`（`migrate_step_inner` 的 `from == 1` 执行 `DDL_CURRENT`）；`crates/zgraph-trace/tests/migration.rs:717`（v1 测试）、`:1547`（`create_v1_database` 仅建 3 表） | **真实 v1 库迁移必失败**：v0.1.0 的 `instances` 表（tag `v0.1.0` spec 06）没有 `item_index`；v1→v2 步骤执行 v6 的 `DDL_CURRENT`，其中 `CREATE INDEX IF NOT EXISTS idx_instances_run_node_item ON instances(run_id,node_id,item_index)` 会对既有 v1 `instances` 报 `no such column: item_index`，整个迁移回滚。测试用的 `create_v1_database` 没有建 instances/transitions/events（只建 schema_migrations/graphs/runs），所以 `v1_database_is_upgraded...` 假绿；真实 v1 用户无法打开/迁移 | 不要把 v6-only 的 item 索引放进 `DDL_CURRENT`（v1 step 复用）：将其移到 v6-only DDL（fresh create 在建完 DDL_CURRENT 后执行；v5→v6 的 `REBUILD_INSTANCES_V6` 已建该索引），或在 v1 step 先补列再建索引/改用 v1 专属 DDL；同时把 `create_v1_database` 改为**真实 v0.1.0 形状**（instances/transitions/events 等），并新增 v1→v6 成功 + 备份 + 旧行保留断言，确保测试能抓住该问题 |

### P2 · 首轮建议（N1–N3 已关闭；N4/N5 见 §7.3）

| # | 位置 | 问题 | 建议 |
|---|---|---|---|
| N1 | `schema.rs:627-645` `migrate_step` FK 切换；`tests/migration.rs:1234` 失败注入 | **FK/CASCADE 修复的测试覆盖不完整**：成功 v5→v6 用例已断言 `human_decisions` 行逐列保留、v5 备份保留（好）；但没有直接断言迁移后连接 `PRAGMA foreign_keys` 恢复为 ON、失败/恢复路径 `human_decisions` 行仍逐列保留（失败用例只断言表存在）、也没有 `PRAGMA foreign_key_check` 结果。代码层面：事务外 OFF、commit/异常后 ON、失败则丢连接再恢复备份，逻辑正确；但需锁住「恢复 ON」与「异常路径行保留」 | 补 3 个断言：①成功迁移后 `PRAGMA foreign_keys` = 1；②失败恢复后 human_decisions 行与 v5 快照逐列一致；③成功/失败后 `PRAGMA foreign_key_check` 为空。可保留 FK OFF 方案（`defer_foreign_keys` 不能阻止 DROP 的 CASCADE，FK OFF 是合理标准做法），在注释中说明该窗口与崩溃安全性 |
| N2 | `tests/migration.rs` v5 快照（`instance_rows_v5/transition/event/human/gate`） | **其它 FK/历史表逐列保留未全断言**：v5 fixture 没有 `recorded_outputs`/`llm_calls`/`runs`/`graphs` 行，迁移前后未做这些表的逐列快照；任务要求「human_decisions/其它 FK 表数据逐列保留」 | v5 seed 增 `recorded_outputs`/`llm_calls` 行（及其 run/graph），迁移前后逐列比较；`runs`/`graphs` 至少断言不变 |
| N3 | `src/writer/tx.rs:write_foreach_expansion` | **`item_count` 溢出错误无测试**：代码用 `i64::try_from(u64)` 返回 Fatal，但 expansions 测试没有构造 `u64::MAX` 触发；任务重点列了「溢出错误」 | 补一个单测：`item_count = u64::MAX` → flush 返回 `TraceError::Storage` 含 `overflow`，且不落行 |
| N4 | `tests/e2e/human_decide.rs:instance_rows_legacy` | **旧 run 逐列比较只看 v6 前历史列**：为适配 v6 新列，e2e 移除了 instances 的 item 列比较；trace crate 迁移测试已断言 item 默认 0/NULL，但 e2e 侧没有交叉引用/显式说明；属可接受的机械适配，但应声明避免被误读为放宽 | 在 helper 注释中交叉引用 `item_instances`/`migration` 的默认列断言；或在 e2e 补一行「旧 run item_index=0/item_key=NULL」断言 |
| N5 | `crates/zgraph-cli/src/bridge.rs:map_snapshot`、`tests/e2e/gate_defer.rs`/`human_decide.rs` | **跨模块机械改动的声明/第二评审**：`map_snapshot` 是生产代码占位（item_index=0/item_key=None），`human_decide` 版本/备份份数与 legacy 快照 helper 超出单纯 `expected 5→6`，需 Lead 第二评审确认「零行为变化」 | 本记录已声明；建议 Lead 第二评审逐项确认 bridge 占位、run_cmd arm、e2e 备份份数/legacy 列；不阻塞 trace 主审 |

### P3 · 台账/阅读性

| # | 位置 | 问题 | 建议 |
|---|---|---|---|
| N6 | `src/schema.rs:open_existing_readwrite_connection` 文档 | 注释仍写「版本 `<5` 明确报错 / `>5` SchemaTooNew」，v6 后应为 `<6`/`>6`；实际代码用 `ensure_current_version` 已是 6 | 改注释 |
| N7 | `src/schema.rs:650` v1 step 注释 | 注释「`DDL_CURRENT` 建当前版本的基表（v1 库缺表全量补齐）」未提示真实 v1 已有表时 v6-only 索引会失败（C1 的一部分）；修复时同步注释 | 改为 v1 专属 DDL/补列路径说明 |
| N8 | `src/model.rs:ForeachExpansionRecord.keys`、`reader.rs:ForeachExpansionRow.keys` | 文档说 `None` 键用 `{"$index":i}`，但 `Option<Value>` 允许 `Some(Null)`，与 `None` 的语义仅靠 writer/json_opt 区分；API 使用者易误用 | 注释明确「`None`=未提供 keys；数组内 NULL 键必须是 `{"$index":i}` 对象，不允许裸 null」或在 writer 侧校验 |
| N9 | `src/lib.rs` 文档/API | `InstanceSnapshot`/`InstanceRow` 新增 item 字段、`ForeachExpansionRecord`/`TraceInput` 新 variant、`InstanceRow` 字段为 source-breaking；本分支未写 08 注记（按 W4b-docs 回写） | W4b-docs 在 08 §8.7/§8.11 记录 source-breaking + 迁移语义；本分支不写 as-built |

## 4. 验证证据（亲自执行）

worktree `/Users/zyzs/lczyzs/code/zgraph/.worktrees/trace-trace-dev`，HEAD `49806c2`，base `main` `42f93d7`，工作区干净；`CARGO_HOME=/Users/zyzs/lczyzs/code/zgraph/.cargo-home`，未设 `CARGO_TARGET_DIR`：

```bash
cargo test -p zgraph-trace                 # 13 targets：91 passed / 0 failed / 0 ignored
cargo test --workspace                     # 69 targets：831 passed / 0 failed / 3 ignored
cargo clippy --workspace --all-targets --locked -- -D warnings   # clean（exit 0）
cargo fmt --all -- --check                 # exit 0
bash scripts/dev/check.sh                  # PASSED

git status --porcelain                     # 空
git merge-base HEAD main                   # 42f93d7（本地 main = origin/main）
git log -1 --format=%B 49806c2             # 含 Review: REV-089 / Agent: trace-dev
git -c core.quotepath=false diff --name-only main...HEAD   # 18 files；无 zgraph-kernel 源文件
```

- **计数与作者自报一致**：trace 91/0、workspace 831/0/3；clippy/fmt/check 全绿；无新增 ignore、无放宽既有断言（除机械版本/备份份数/legacy 列适配，见 §5）。
- **v5→v6 成功迁移证据**：`v5_database_is_upgraded...` 断言 user_version=6、migrations 1..6、instances/transitions/events/gate_results/human_decisions 逐列一致、item 列 0/NULL、唯一 4 列生效、expansions 存在、恰一份 `.bak.v5-*` 且备份 user_version=5/旧 3 列唯一键/human 行一致。
- **FK 修复证据**：`failed_v5_to_v6...` 注入 schema_migrations ABORT → 回滚到 v5、v6 表/列不存在、v5 备份保留、清故障后可重试；成功用例 human 行逐列保留。**但缺恢复 ON/失败路径 human 行/foreign_key_check 断言（N1）。**
- **真实 v1 迁移缺陷复现（只读核验）**：`git show v0.1.0:doc/spec-v1.2/06-存储与可观测.md` 的 `instances` 无 `item_index`；在 SQLite 上对「旧 instances + `CREATE INDEX ... (item_index)`」执行得到 `OperationalError: no such column: item_index`；当前 `migrate_step_inner(from=1)` 正是执行含该语句的 `DDL_CURRENT`；而 `create_v1_database`（migration.rs:1547）只建 schema_migrations/graphs/runs，故 v1 测试没有覆盖真实 v1 instances（假绿）。
- **测试数量**：migration 18 / expansions 4 / item_instances 5，与任务自报 15→18/4/5 一致。

## 5. 跨模块机械改动核对

| 文件 | 改动 | 核对 |
|---|---|---|
| `crates/zgraph-cli/src/bridge.rs` | `map_snapshot` 增 `item_index:0/item_key:None` 占位；测试 observer 加 `ForeachExpansion` kind | 生产占位不影响当前 kernel（无 item 字段）；无行为变化；待 kernel W4b 后续替换真实值 |
| `crates/zgraph-cli/src/run_cmd.rs` | `lifecycle_detail` 加 `TraceInput::ForeachExpansion(_) => None` | 仅编译/输出语义；expansion 不属 events 生命周期；正确 |
| `crates/zgraph-cli/tests/{decide,trace}.rs` | 构造点补 item 字段；decide 版本期望 5→6、newer 6→7 | 仅测试夹具/版本，无行为放宽 |
| `tests/e2e/gate_defer.rs` | v3→v6 版本/备份数 2→3、只读 expected 6 | 机械同步 |
| `tests/e2e/human_decide.rs` | v4→v6 版本/备份 1→2、expected 6、旧 run 逐列快照改用 legacy 列 helper | 机械适配；新列默认值由 trace migration/item 测试断言；建议交叉引用（N4） |

- **未改 kernel 源码/DSL/实现**；18 files 均 trace + 批准跨模块；无其它越界。

## 6. 遗留问题

- **C1 已关闭（2026-11-01，`dc25340`）**：真实 v1 fixture（6+2 表/历史行）+ v1→v6 成功/旧行/备份/新 run 断言已通过；见 §7.1。
- **N1–N4（Lead 口径）已关闭**：FK 窗口/恢复 ON/foreign_key_check/FK 生效、失败 human 行、recorded_outputs/llm_calls/runs/graphs 逐列、item_count 溢出均已补测试；见 §7.2。
- **残余 P3**：`open_existing_readwrite_connection` 旧版本文案（N6）、v1 step 注释（N7）、keys API 语义（N8）、source-breaking 08 注记（N9/W4b-docs）；以及 e2e legacy 列 helper（首轮 N4，跨模块第二评审，trace 侧已交叉覆盖）。
- **无 P0/P1**；C1+N1–N4 已关闭，最终**通过**；跨模块第二评审由 Lead 追加后即可 merge。

## 7. 条件关闭复审（2026-11-01，HEAD `dc25340`）

**复审对象**：`570fd69..dc25340`（修复提交 `dc25340`，parent 评审提交 `570fd69`）；**仅 `crates/zgraph-trace/**` 3 files**，+686/−79；worktree 干净；无新增跨模块改动。首轮条件表见 §3（历史），以下为关闭证据。

### 7.1 C1 关闭证据（真实 v1→v6）

| 项 | 证据 |
|---|---|
| v6-only 索引拆出 | `schema.rs:422` 新增 `DDL_INSTANCES_V6_INDEX`；`DDL_CURRENT` 不再包含 item 索引（`:122` 注释说明原因）；`create_current` 在建完列后单独执行（`:623-624`）；v5→v6 `REBUILD_INSTANCES_V6` 建同一索引（`:493`）。fresh/migrated 索引集合一致由 migration 测试验证。 |
| 真实 v1 fixture | `migration.rs:174` `V1_DDL` 为 v0.1.0 的 **6+2 表**（schema_migrations/graphs/runs/instances/events/transitions/gate_results/streaming_events）；`:328` `V1_SEED_SQL` 含 run/instance/transition/5 events/gate/streaming 历史行；`create_v1_database`（`:2072`）执行真实 DDL+seed。 |
| v1→v6 测试 | `v1_database_is_upgraded_to_current_version_in_place`（`:998`）实测：真实 v1→v6 成功、user_version=6、migrations 1..6、`runs`/`instances`/`transitions`/`events`/`gate_results`/`streaming_events` 逐列保留、item 默认 0/NULL、唯一 4 列、human/expansions 建表、`foreign_key_check=0`、旧 run 可读、新 run 可写、5 份备份 `.bak.v1..v5` 各回写对应 user_version、v1 备份无 v6 表/列。 |
| 对照旧代码 | `git show 49806c2:...schema.rs` 的 `DDL_CURRENT:195` 含 `idx_instances_run_node_item`；当前已移除并移到 v6-only；v1 step 不再引用 item_index。 |

### 7.2 N1–N4（Lead 口径）关闭证据

| # | 内容 | 证据 |
|---|---|---|
| N1 FK 窗口/恢复 ON/FK 生效 | 成功/失败均恢复 | `schema.rs:651` `with_foreign_keys_disabled` 封装 OFF→run→ON（成功/失败/恢复失败分支）；`schema::tests` 两个单测（`:941`/`:963`）断言窗口内 OFF、窗口后 ON、失败也 ON；`v5_database_is_upgraded...` 迁移后写端对不存在 run 写 instance → `FOREIGN KEY` 错误（FK 已恢复 ON）；成功/失败均 `foreign_key_check=0`。 |
| N2 失败/恢复 human 行逐列保留 | 已断言 | `failed_v5_to_v6...` 迁移前快照 `human_decision_rows`，失败恢复后逐列相等（不再只断言表存在）；成功用例也逐列相等；v5 备份 human 行一致。 |
| N3 recorded_outputs/llm_calls/runs/graphs 逐列保留 | 已断言 | v5 migration `before` 增加 `full_table_rows` for runs/graphs/recorded_outputs/llm_calls；迁移后逐列相等；v1 test 额外覆盖 runs/gate/streaming 历史列。 |
| N4 item_count 溢出 | 已断言 | `expansions.rs:184` `item_count_overflowing_i64_is_a_storage_error`：`u64::MAX` → flush `TraceError::Storage` 含 `foreach item_count overflow`，不落行。 |

> 首轮 §3 的 N1/N2/N3 编号与 Lead 口径有合并（首轮 N1 同时含 FK 窗口与失败 human 行；Lead 将其拆为 N1/N2；首轮 N2=recorded；N3=overflow）；上表按 Lead 口径覆盖，均通过。

### 7.3 复审验证（新 HEAD/重算计数）

worktree `/Users/zyzs/lczyzs/code/zgraph/.worktrees/trace-trace-dev`，HEAD **`dc25340`**，base `main` `42f93d7`，工作区干净：

```bash
git diff --stat 570fd69..dc25340          # 3 files, +686/−79（仅 crates/zgraph-trace/**）
git -c core.quotepath=false diff --name-only 570fd69..HEAD | grep -v '^crates/zgraph-trace/'  # 无
git status --porcelain                    # 空
cargo test -p zgraph-trace                # 13 targets：94 passed / 0 failed / 0 ignored
cargo test -p zgraph-trace --test migration # 18 passed / 0 failed
cargo test --workspace                    # 69 targets：834 passed / 0 failed / 3 ignored
cargo clippy --workspace --all-targets --locked -- -D warnings  # exit 0
cargo fmt --all -- --check                # exit 0
bash scripts/dev/check.sh                 # PASSED
```

- 作者自报 **trace 94/0、workspace 834/0**；本次实测一致；migration 18、expansions **5**（+overflow）、item_instances 5、trace lib 单测 5（+2 FK helper）。
- `570fd69..dc25340` **3 files 全为 crates/zgraph-trace/**；无 kernel/cli/e2e 新增改动；首轮跨模块改动未动，第二评审仍由 Lead 追加。
- **残余 P3（不阻断）**：①首轮 N4（e2e `instance_rows_legacy` 只比较历史列）未在 trace-only 修复范围；trace migration/item_instances 已断言旧 run item 默认 0/NULL，建议 Lead 第二评审确认 e2e helper 注释/断言；②首轮 N5 跨模块第二评审待 Lead；③`open_existing_readwrite_connection` 注释仍 `<5/>5`（N6）；④`ForeachExpansionRecord.keys` Option 语义文档（N8）；⑤source-breaking 08 注记（N9/W4b-docs）。

### 7.4 复审结论

**C1 + N1–N4 全部关闭；复审无 P0/P1；最终结论：通过。** 建议 Lead 追加跨模块第二评审后 merge（HEAD `dc25340` + 本记录提交）。

## 8. 第二评审 · 跨模块机械改动（Lead 追加，2026-11-01）

- **评审人**：`lead`（**≠ 作者** `trace-dev`；Lead 已批准跨模块机械改动，按 AGENTS §2 由 Lead 追加第二评审）
- **范围**（`42f93d7..dc25340`，仅机械）：
  1. `crates/zgraph-cli/src/bridge.rs`：`map_snapshot` 增 `item_index:0, item_key:None` 占位（kernel 阶段 2 替换真实值）；测试 observer 新 `TraceInput::ForeachExpansion` kind arm。
  2. `crates/zgraph-cli/src/run_cmd.rs`：`lifecycle_detail` 新变体 → `None`（展开事实不是 events 行，不产生生命周期行）。
  3. `crates/zgraph-cli/tests/{decide,trace}.rs`：`InstanceSnapshot` 构造点补 item 默认字段；decide 版本期望 5→6、newer 6→7。
  4. `tests/e2e/gate_defer.rs` / `tests/e2e/human_decide.rs`：schema 版本期望 5→6、`schema_migrations 1..6`、逐版本备份份数 2→3 / 1→2、只读 `expected 6`、human 迁移前快照改用 legacy 列清单（v6 新列由 trace 迁移测试断言）。
- **核对结论（通过）**：
  1. 仅枚举新变体导致的穷举 arm / 结构体新字段占位 / 版本与备份期望同步；**零行为变化**（无 foreach 图输出/events/回放语义不变），未放宽任何断言（版本期望是 schema v6 的强制同步；legacy 列快照另由 trace 迁移测试逐列覆盖 item 默认值）。
  2. 未改 kernel 源文件；改动点与作者声明一致（`git diff 42f93d7..dc25340 --stat` 仅 trace 源/测试 + 上述 cli/e2e 机械文件）。
  3. 占位字段将在 kernel 分支阶段 2 由 Lead 批准的 kernel 跨模块项替换为真实 item 值（不遗留占位）。
- **Lead 亲自验证**：`cargo test -p zgraph-cli --offline` = 206 passed / 0 failed；`cargo test -p zgraph-e2e --test gate_defer --offline` = 8 passed / 0 failed。
- **结论**：**通过（第二评审）**。跨模块机械改动与主审结论合并生效；分支满足合并前置。
