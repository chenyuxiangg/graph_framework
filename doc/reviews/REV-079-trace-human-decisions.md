# REV-079 · trace · W3a schema v5（WaitingHuman + human_decisions）+ 决策读写 API

> **跨模块机械声明**：本分支主体为 `trace`（`crates/zgraph-trace/**`），另按 task-14/15 合同**机械同步** `tests/e2e/gate_defer.rs` 的 v5 版本期望（Lead 已批准；Lead 追加第二评审）。改动仅版本期望与随之变化的备份份数/审计行断言，**未放宽任何既有断言**，下表 §3 N6 说明。评审人只新增本评审记录，未修改产品/测试代码。

- **分支**：`feat/trace-human-decisions`
- 分支: feat/trace-human-decisions
- **作者**：`trace-dev`（提交 `4cae6b9`；`Review: REV-079` / `Agent: trace-dev`）
- **评审人**：`reviewer-a`（独立评审，**≠ 作者**）
- **变更范围**：`3d0e933..4cae6b9`（10 files, +1697/−109）：`crates/zgraph-trace/src/{schema,model,reader,lib,decision,writer/mod}.rs`、`crates/zgraph-trace/tests/{api,decisions,migration}.rs`、机械改动 `tests/e2e/gate_defer.rs`。`3d0e933` = 本地 main = origin/main；`Cargo.toml`/`Cargo.lock` 未变。
- **评审时间**：2026-10-31
- **关联**：DR-006 §5（表/C13 写者）/§6（schema v5 与 API）/§8（兼容矩阵）/§10 AC-16 e/§11（实施顺序）；task-14/15 合同；06 §6.2.5/§6.2.7/§6.2.14/§6.7；REV-067/070（v3→v4 既有迁移）

## 1. 结论

**结论：通过**

按 task-14/15 合同与 DR-006 §5/§6/§8 逐项核对，全部成立；P0=0、P1=0、P2=0，P3 建议 8 条（含 3 条偏差裁决结论与 2 条跨分支待办），不阻塞合并。未放宽/删除既有断言、未改 cli/kernel 源码、`Cargo.lock` 未变。评审人除复跑作者全部门禁外，另用**真 CLI 独立探针**（构造真实 v4 库 → 真 `zgraph run` 触发迁移）验证 27 项迁移/恢复/只读断言全部通过（§4.2）。

### 1.1 合同覆盖对照（DR-006 §5/§6/§8）

| 合同项 | 实现/证据 | 结论 |
|---|---|---|
| 版本与 DDL | `TRACE_SCHEMA_VERSION=5`（`schema.rs:36`）；`instances.status` 与 `transitions.from_status/to_status` CHECK 均含 `WaitingHuman`；`human_decisions` 表 + `idx_human_decisions_pending` 部分索引与 DR-006 §5.1 逐字段一致（`schema.rs:306-325`） | ✅ |
| 新库建库 | 单事务 `DDL_CURRENT` + `DDL_HUMAN_DECISIONS` + `schema_migrations` 1/2/3/4/5 + `user_version=5`，**无备份**（`schema.rs:526-533`；migration 测试断言 fresh 无 `.bak`） | ✅ |
| v4→v5 迁移 | `VACUUM INTO` 备份（回写并校验 `user_version=4`）→ 单版本单事务：`REBUILD_INSTANCES_V5`（12 步法、显式列清单、3 索引）→ `REBUILD_TRANSITIONS_V5`（同法、3 索引）→ 建 `human_decisions`（FK 指向新 instances）→ `schema_migrations` v5 → `user_version=5`（`schema.rs:327-405,556-575`）；失败恢复见下 | ✅ |
| 备份/失败恢复 | 逐版本备份（v1→v5 4 份、v2→v5 3 份、v3→v5 2 份、v4→v5 1 份，测试逐级断言）；失败 `drop conn → 删 -wal/-shm → 备份覆盖` 并保留备份（`schema.rs:632-689`）；迁移失败注入测试断言库回到 v4、数据完好、可重试（`migration.rs:913`；评审人独立探针复验） | ✅ |
| 只读拒绝/超前 | readonly/decision 连接均要求**恰好** v5：`<5` → `unsupported schema version=N, expected 5`（不迁移），`>5` → `STORE_SCHEMA_TOO_NEW`（`schema.rs:710-724`；`migration.rs:464`、e2e `:613`） | ✅ |
| v1..v4→v5 链 | `migration.rs` 覆盖 v1/v2/v3/v4→v5，断言备份份数/`schema_migrations` 1..5/旧行逐列保留/`human_decisions` 建立；e2e v3 seed→v5 经真 CLI 回放 golden 不变（`:288-430`，workspace 通过） | ✅ |
| 迁移不改历史 | v4→v5 测试用 `rusqlite::types::Value` 逐列比较 `instances`/`transitions`/`events`（含 payload）迁移前后完全相等（`migration.rs:763-910`）；评审人独立探针复验 | ✅ |
| `InstanceStatus` | `WaitingHuman` variant + `as_str`/`parse` 往返（`model.rs:22,42,57`）；`is_terminal=false`（api 测试）；transition severity = `info`（`writer/tx.rs:771` 非 error 状态默认 info；`decisions.rs:431` 断言） | ✅ |
| 决策读取 API | `HumanDecisionRow` 8 字段（`decision.rs:48`）；`TraceReader::human_decisions` 按 `(decided_at, decision_id)` 升序（`reader.rs:522`、`decision.rs:311-345`）；空 run 返回空 Vec | ✅ |
| `DecisionStore` | `open` 不建库/不迁移/版本拒绝（`decision.rs:139`）；`precheck` 在**同一读事务快照**内返回 `decision_exists`/`run_status`/`instance_status`（`decision.rs:147-188`）；`insert` 单事务 Immediate + 唯一键冲突→`Conflict`（PK 与 `decision_id` UNIQUE 分别测试）、CHECK/NOT NULL/FK 仍为存储错误（`decision.rs:193-247`、`decisions.rs:122-208`） | ✅ |
| `DecisionConsumer` | `pending` RO（`busy_timeout=1000ms`）+ 确定性排序；`mark_consumed` RW 短事务条件更新 `consumed_at IS NULL`（幂等，返回 `false`）busy 重试 3 次（`decision.rs:272-310`、`decisions.rs:265-330`） | ✅ |
| C13 写者模型 | `TraceWriter` 无 `human_decisions` 写入口（全源码 grep 无引用；`writer/mod.rs` 仅 `pub(crate) mod tx` 供复用重试原语）；decide 提交在写线程打开期间立即可见（`decisions.rs:333`） | ✅ |
| §8 兼容矩阵 | 无 human 图/无决策 run 行为与 golden 不变（`TRACE_SCHEMA_VERSION` 外无 schema 形状变化；e2e/golden 全量 workspace 通过）；`:memory:` 与 CLI 侧属于后续 cli/e2e 分支 | ✅（trace 侧部分） |

## 2. 四维评审

| 维度 | 结论 | 依据（文件/行/测试） |
|---|---|---|
| 功能正确性 | ✅ | DR-006 §5/§6 与合同 A1–A4 逐项成立（§1.1）；真实门禁 `cargo test -p zgraph-trace` 79 passed、`cargo test --workspace` 741 passed/0 failed/3 ignored 与作者报告一致；评审人独立真 CLI 探针 27/27 通过（§4.2），覆盖新库/迁移/备份/失败恢复/只读拒绝/索引与列结构/数据逐列/迁移后新写入 |
| 代码结构合理性 | ✅ | `decision.rs` 独立模块承载 C13 第二/第三写者，职责单一；`DDL_HUMAN_DECISIONS` 由新库与 v4→v5 **共用同一字面量**避免漂移；重建 DDL 拆两个显式 const；busy 判定/退避直接复用 writer 既有原语（`writer/tx.rs:796-809`）而非复制；`writer/mod.rs` 仅最小可见性调整；e2e 仅机械同步。偏差 ①（`DecisionError` 不污染 `TraceError`）经核实是**避免 cli 三处穷举 match 破裂**的最小改动（§3 N5） |
| 逻辑性 | ✅ | 单版本单事务；FK 顺序正确（先重建 instances，再建 human_decisions）；`DROP TABLE IF EXISTS human_decisions` 仅为重入防御（v4 不含该表）；重建列清单显式（禁 `SELECT *`）；旧状态值是 CHECK 新集合子集故复制必然通过；失败路径事务回滚 + 备份覆盖 + 保留备份；`precheck` 单事务快照避免自相矛盾；`mark_consumed` 条件更新保证至多一次；busy 判定仅 BUSY/LOCKED、冲突判定仅 UNIQUE/PRIMARY KEY 扩展码（CHECK 不误判为 exit 5）；无死循环/吞异常 |
| 可读性 | ✅（N4 备注） | 模块/函数注释均引用 DR-006/06 章节并解释「为什么」（独立 `DecisionError`、备份不清理、索引重建、部分索引、C13 分表）；显式列清单可审计；测试名与断言消息可读、断言强（含 error 文案/行级数据/备份名）。N4：e2e 函数名仍含 `_to_v4_`（实际到 v5），属机械同步遗漏的命名一致性问题 |

## 3. 发现清单

> 级别口径：P0 = 阻塞；P1 = 重要（必须修复或登记 ISSUE）；P2 = 一般；P3 = 建议（可后置，应进台账）。本分支 P0=0、P1=0、P2=0。

| # | 级别 | 位置 | 问题 | 建议 | 状态 |
|---|---|---|---|---|---|
| 1 | P3 建议 | `crates/zgraph-trace/tests/migration.rs:763`（fresh 对照 `:233`） | v4→v5 迁移测试断言了列/数据/CHECK/备份，但**未断言重建的 3+3 索引存在且与新库一致**（`EXPECTED_INDEXES` 只在 fresh 测试使用）。评审人独立探针已证实现实一致（§4.2），但缺固化断言，未来改 `REBUILD_*` 漏索引不会被测试发现 | 迁移后复用 `EXPECTED_INDEXES`（并可比对 `sqlite_master.sql`）断言索引集合 = fresh v5 | open（建议项） |
| 2 | P3 建议 | `crates/zgraph-trace/tests/decisions.rs:122` | 唯一键冲突只有**同一连接顺序**用例；无跨连接/并发 `DecisionStore` 插入同一 `(run_id, instance_id)` 的用例（Lead 评审重点提及"并发唯一键"）。DR §10 AC-16 f 的并发主战场在 e2e（REV-082），且 SQLite UNIQUE 已由顺序用例覆盖 | 可选补一个双 store（同文件两个 `DecisionStore`，可单线程）冲突用例：恰 1 Ok + 1 `Conflict`、表恰 1 行；或明确注明由 e2e AC-16 f 覆盖 | open（建议项） |
| 3 | P3 建议 | `crates/zgraph-trace/src/decision.rs:193,285`；`schema.rs:45` | `busy_timeout`/重试口径（decide 5000ms、poll RO 1000ms、重试 3 次）未直接断言，`DECISION_POLL_BUSY_TIMEOUT_MS` 无常量测试，busy 耗尽路径无故障注入用例（`DecisionError::Trace(Storage)`） | 至少断言常量值（`DECISION_POLL_BUSY_TIMEOUT_MS==1000`、poll/store 打开成功）；busy 路径可由 e2e 并发覆盖 | open（建议项） |
| 4 | P3 建议 | `tests/e2e/gate_defer.rs:288` | 函数仍名 `v3_seed_is_migrated_to_v4_on_run_and_history_is_preserved`，实际迁移目标是 v5（doc 注释已更新）；名称误导后续维护者 | 机械改名为 `..._migrated_to_v5_...`（不改断言） | open（建议项） |
| 5 | P3 裁决 | `crates/zgraph-trace/src/decision.rs:106` | **偏差 ①（Lead 委托裁决）**：未扩展 `TraceError`，新增 `DecisionError::{Conflict,Trace}`。**结论：可接受、与 DR 冲突最小**。依据：DR-006 §6.3 未冻结错误类型；仓内 `TraceError` 有 3 处穷举 match（`crates/zgraph-cli/src/run_cmd.rs:1542`、`trace_cmd.rs:677`、`replay_cmd.rs:236`），加 variant 会迫使 trace 分支改 cli（越界）；新类型把"冲突→exit5"与"存储→exit4"分离清楚 | 待办：cli 分支（REV-081）实现 `Conflict→exit 5/HUMAN_DECISION_EXISTS`、`Trace→exit 4`（沿用 `TraceError::code()`），且 decide 对 `user_version<5` 时按 DR-006 §5.4 在 stderr 补"先由 `zgraph run` 迁移"提示（trace 层文案是通用 `unsupported schema version=N, expected 5`）；W3a-docs（REV-083）记录该偏差与 `DecisionError::code()` 命名空间（当前 `CONFLICT`/存储码，非 `HUMAN_*`） | open（建议项，跨分支待办） |
| 6 | P3 裁决 | `tests/e2e/gate_defer.rs:304,314,318,332-343,613` | **偏差 ②（Lead 委托裁决）**：除合同 2 处外多改 `schema_migrations 1..5`、逐版本备份 2 份（并改用 `find .bak.v3-`）、注释。**结论：合理且必要**：v3 seed 经 v3→v4→v5 必然产生 2 份备份与 5 条审计行；备份断言由 `len==1 + [0]` **强化**为 `len==2 + 定位 v3 备份`，返回码/版本/审计/数据/golden 断言均未放宽；`trace --json` golden 与 `replay==trace` 断言原样保留（`gate_defer.rs:365-410`） | 保留；仅建议按 N4 改名 | open（裁决完成） |
| 7 | P3 裁决 | `crates/zgraph-trace/src/decision.rs:52` | **偏差 ③（Lead 委托裁决）**：`human_decisions.decision: String` 而非枚举。**结论：可接受**：存储层保持与表同构、避免 trace→kernel 依赖倒置；闭集由 `CHECK(decision IN ('approved','rejected'))` + CLI 参数校验双重保证；trace 测试已覆盖非法值 → `Trace(Storage)` 且不落库（`decisions.rs:166`） | W3a-docs 在 06 as-built 注明「读取层不做枚举校验，闭集由表 CHECK+CLI 保证」，并写明 CLI 必须先校验再 insert（REV-081） | open（裁决完成） |
| 8 | P3 建议 | `crates/zgraph-trace/src/model.rs:22` | 增公开 enum variant `InstanceStatus::WaitingHuman` 对仓外 exhaustive `match` 属 source-breaking；DR-006 §12.2 E-4 的 08 §8.7 注记只点名 `RunControl` 新 variant | W3a-docs（REV-083）在 08 §8.7 一并登记 `InstanceStatus::WaitingHuman`（及新公开类型 `DecisionError`/`HumanDecisionRow` 等）的 source-breaking 兼容说明 | open（建议项，跨分支待办） |

## 4. 验证证据

评审人**亲自执行**（worktree `/Users/zyzs/lczyzs/code/zgraph/.worktrees/trace-trace-dev`，HEAD `4cae6b9`，基于 `origin/main=main=3d0e933`，工作区干净）：

### 4.1 门禁与测试

```bash
export CARGO_HOME=/Users/zyzs/lczyzs/code/zgraph/.cargo-home
cargo test -p zgraph-trace
# unit 3 + api 6 + channels 7 + decisions 11 + global_vars 3 + migration 15
# + reader 6 + recovery 8 + roundtrip 7 + run_columns 3 + streaming 9 + doctest 1
# = 79 passed, 0 failed, 0 ignored
cargo test -p zgraph-trace --test migration          # 15 passed / 0 failed
cargo test --workspace                               # 741 passed / 0 failed / 3 ignored
cargo clippy --workspace --all-targets --locked -- -D warnings   # clean（exit 0）
cargo fmt --all -- --check                           # exit 0
bash scripts/dev/check.sh                            # CHECK PASSED
/Users/zyzs/lczyzs/code/zgraph/.venv/bin/python -m pytest    # 170 passed（rc=0）
```

- 全量 workspace 含 e2e：`gate_defer` 8/8（含 `v3_seed_is_migrated_to_v4_on_run_and_history_is_preserved` 与只读拒绝用例）、其余既有 e2e/trace/kernel/cli target 无失败 → **无新增失败**（与 `doc/test-baseline.txt` 比对无新增失败项）。
- 范围：`git diff --name-status 3d0e933...HEAD` = 5 src + 4 trace tests + `tests/e2e/gate_defer.rs`；`Cargo.lock`/`Cargo.toml` 无 diff；提交正文含 `Review: REV-079` / `Agent: trace-dev`。

### 4.2 迁移/恢复/只读的独立真链路探针（评审人自建，不依赖作者测试）

方法：`cargo build -p zgraph-cli --bin zgraph --locked` 后，用 Python sqlite3 以**本仓 HEAD 的 v4 重建 DDL** 把 fresh v5 库降级为真实 v4 形状（v4 CHECK、无 `human_decisions`、`user_version=4`），再用真 `zgraph run` 触发 v4→v5，然后逐项检查；另做故障注入与只读拒绝。**27/27 断言全部 PASS**：

```text
PASS user_version=5 / schema_migrations 1..5 / human_decisions 表存在
PASS 迁移后索引集合 == 新库索引集合（覆盖 3+3 重建索引 + pending 部分索引）
PASS 迁移后 instances/transitions 列定义 == 新库列定义
PASS 旧 instances/transitions/events 行逐列（含 payload）完全保留
PASS 迁移后新 run 可继续写 transitions，transition_id 严格递增（AUTOINCREMENT 经改名仍正确）
PASS 恰一份 .bak.v4- 备份，备份 user_version=4；run Completed
# 故障注入（schema_migrations 触发器 ABORT）：
PASS CLI 非 0 退出、stderr 含注入错误；库恢复为 v4、无 human_decisions、数据完好、保留备份；清除故障后重试成功迁移到 v5
# 只读：
PASS `zgraph trace` 对 v4 exit 4，stderr "unsupported schema version=4, expected 5"；
PASS 未迁移（仍 v4）、未生成备份、数据未变
```

- 该探针同时弥补 §3 N1 指出的"迁移后索引无固化断言"的验证空白：**实际实现正确**。
- C13：`crates/zgraph-trace/src/writer/**` 无 `human_decisions` 引用；`TraceWriter` 公开 API 无决策写入口。

## 5. 遗留问题

- **跨分支待办（不阻塞本分支）**：cli（REV-081）需实现 `DecisionError` 映射（Conflict→exit 5 `HUMAN_DECISION_EXISTS`；Trace→exit 4）与 DR §5.4 的 v<5 迁移提示；`trace --json`/`replay --mode=events` 追加 HumanDecision 行时使用 `TraceReader::human_decisions` 的确定性顺序；e2e（REV-082）覆盖 AC-16 f 并发与 `:memory:` 拒绝；docs（REV-083）回写 as-built、偏差 ①③、`InstanceStatus` source-breaking 注记、AC-16 e 实测。
- **P3 建议未登记 `doc/issues.md`**（本分支合同只允许 trace 源码 + e2e 机械同步；台账由 docs/Lead 维护）：N1–N4、N8 建议由 Lead 决定是否登记；N5–N7 为已裁决偏差，建议在 W3a-docs 的 DR-006 §14.3/§15 执行记录中留痕。
- `tests/e2e/gate_defer.rs` 的机械改动属跨模块（e2e 模块路径），Lead 已批准并追加第二评审；本记录为该改动的第二评审依据之一。
- 未发现需要返工的问题；无阻塞项。

## 6. 条件关闭记录（仅"有条件通过"填写）

不适用（结论为 `通过`，无附加条件）。
