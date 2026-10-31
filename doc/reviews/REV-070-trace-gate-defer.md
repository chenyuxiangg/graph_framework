# REV-070 · trace · GateResult 增 deferred/attempt/verdict，schema 升 v4

- **分支**：`feat/trace-gate-defer`
- **作者**：`trace-writer`（提交 `96ab28e`；正文含 `Review: REV-070` / `Agent: trace-writer`）
- **评审人**：`reviewer-a`（**≠ 作者**；主审）；**第二评审：`lead`**（cli 机械改动复核，按 Lead 分工在本记录后追加）
- **变更范围**：`7f88614..96ab28e`，12 文件 **+353/−20**：主模块 `trace` **9 文件 +335/−20**（`crates/zgraph-trace/src/{lib,model,reader,schema,writer/tx}.rs`、`tests/{api,common/mod,migration,roundtrip}.rs`）；**Lead 批准的跨模块 cli 机械改动 3 文件 +18/−0**（`crates/zgraph-cli/src/bridge.rs` +8、`src/run_cmd.rs` +3、`tests/trace.rs` +7）；未改 kernel/dsl/harness/worker、`doc/**`、`CLAUDE.md`、`Cargo.*`
- **评审时间**：2026-10-31
- **关联**：DR-004 §3 W2a0 / §4 W2a、D5（trace 迁移点 W2a）；W2a0 冻结文档（docs 分支 `docs/docs-rework-design` 的 `DR-005-w2a-defer-gate-freeze.md` + spec 06 §6.2.2/§6.2.8/§6.2.12/§6.7，**未合入 main**）；REV-064/REV-065/REV-066；ISSUE-091（e2e 时序 flaky）
- **跨模块声明（Lead 批准，写入本记录）**：本分支主模块 = `trace`；为 trace API/schema 扩展编译必需，机械修改了 cli——`bridge.rs` 仅给 `trace::GateResultRecord` 填过渡值 `deferred=false`/`attempt=0`/`verdict=None`（真实值由后续 W2a kernel 分支接线，注释已指向 `feat/kernel-deferrable-gate`），`run_cmd.rs` 仅测试模块字面量，`tests/trace.rs` 仅 fixture 字面量与 payload 断言。Lead 指定主审 reviewer-a、第二评审 lead（cli 机械改动复核）；本记录不覆盖 cli 语义评审，但已逐 hunk 确认「除上述机械改动外 cli 无其它改动」。

## 1. 结论

**结论：通过**

W2a0 冻结的 trace 契约逐项落实且经独立复核：`GateResultRecord`/`GateResultRow` 增 `deferred: bool`、`attempt: u32`、`verdict: Option<Value>`；writer 在同一事务内写 `events.payload.record` 三键与 `gate_results` 三列（同源 `record`），`verdict_json` 的 NULL/值双向往返正确，`deferred` 以 0/1 落库、reader 反解 bool；severity 推导未改（deferred 不参与，`passed=0 + error=None` 仍 `warn`，与 W2a0 一致）；`trace --json` 经 events payload 自然带出三键，人类文本列格式未动。

schema v4（`TRACE_SCHEMA_VERSION=4`）：新库单事务建 DDL + `schema_migrations` 1..4、无备份；旧库 v1/v2/v3→v4 逐版本循环迁移，单版本单事务 + 迁移前 `VACUUM INTO '<db>.bak.v{old}-<ts>'`（回写并校验 `user_version=old`）+ 失败恢复 + 审计行；v3→v4 仅三条**只增** `ALTER TABLE gate_results ADD COLUMN`（与 `DDL_CURRENT` 定义逐字一致、经 `add_missing_columns` 列存在检查可重复执行），旧行取 `deferred=0/attempt=0/verdict_json=NULL`；只读连接 <4 与 >4 均拒绝且不迁移。

cli 改动经逐 hunk 核对仅为 Lead 声明的机械适配（bridge 过渡值 + 测试字面量/断言），未越界；未改 kernel/doc/CLAUDE.md。

独立验证：`cargo test -p zgraph-trace` **63 passed / 0 failed**（migration 10、roundtrip 7）；`cargo test -p zgraph-cli` **160 passed / 0 failed**；`cargo test --workspace` **650 passed / 0 failed / 3 ignored**（含 e2e，无 ISSUE-091 flaky 复现）；`cargo fmt --check`、`cargo clippy --workspace --all-targets -- -D warnings`、`scripts/dev/check.sh` 均通过；另用 `sqlite3` 独立验证 DDL/ALTER 的列类型/默认值/CHECK、旧行默认值与 CHECK 生效。

发现 5 条**建议**（S1–S5），均不阻塞：S1 为 W2a0 spec（docs 分支未合入 main）与实现的「损坏 verdict_json 读取兜底」口径差异，属 W2a0/W2a-docs 收口事项；S2/S5 为测试函数行数与迁移测试强度；S3 为测试命名；S4 为 DDL 列序与 spec 展示顺序的文档口径。按 Lead 本轮冻结契约清单，无未关闭条件，结论 **通过**。

## 2. 四维评审

| 维度 | 结论 | 依据（文件/行/测试） |
|---|---|---|
| 功能正确性 | ✅ | `model.rs:286-323`：三字段 + W2a0 语义注释；`writer/tx.rs:444-500`：payload 三键（键序=结构体字段序）与 `gate_results` 14 列 INSERT 同源，`json_opt` 写 NULL/JSON、`i64::from(attempt)`、`if deferred {1} else {0}`；`reader.rs:462-476,759-780`：SELECT 显式列出 `deferred, attempt, verdict_json` 并按 12/13/14 映射（bool↔0/1、`to_u32`、`parse_json_opt`）；`schema.rs:26-27,86-94,206-222,370-398`：版本常量/列定义/migrate_step(3)/create_current 一致；`severity_gate`（`writer/tx.rs:778-786`）未改。测试：`roundtrip.rs:491-573` 覆盖 deferred=true/attempt=2/verdict=Some、v0.1.0 默认三值与 payload 三键、severity 仍 `warn,warn,info`；`migration.rs:363-462` 覆盖 v3→v4 旧行 `false/0/NULL`、新行往返、`.bak.v3-*`（user_version=3、无 deferred 列、旧行保留）、审计行含 W2a；`migration.rs:72-138/255-316/319-361/465-522` 覆盖新库 1..4 行/无备份、v1→v4 三份备份、v2→v4、>4 拒绝、只读 <4 拒绝、失败恢复；`api.rs:18` 版本钉 4（契约变更，允许）。独立 sqlite3 探针（从源码抽 DDL/ALTER）验证列类型/默认值/CHECK 与旧行默认值 |
| 代码结构合理性 | ✅（S2/S5 备注） | 12 文件全部为 `trace` 主模块 + Lead 批准的 cli 机械适配；列定义集中在 `GATE_RESULTS_V4_COLUMNS` 与 `DDL_CURRENT`，迁移复用既有 `add_missing_columns`（不新造机制）；`model`/`reader`/`writer`/`schema` 职责边界未变；cli bridge 过渡值集中在一处并注释指向 kernel 分支，未扩大 cli 职责。生产函数均 ≤60 行（`write_gate_result` 约 53 行）；但**新增/改动测试函数超 60 行**（S2），且 DDL 与 ALTER 两份列定义无一致性守卫（S5） |
| 逻辑性 | ✅ | 迁移循环 `while current < 4` 逐版本备份→单事务迁移→记录审计行→`user_version`；失败删 `-wal`/`-shm` 后恢复备份（既有失败注入测试覆盖，本分支未破坏）；`>4` 走 `SchemaTooNew`、`<4` 只读走 `Storage`（`ensure_readonly_version:536-549`）；`add_missing_columns` 先 `table_columns` 再补缺列，v1→v4 路径实际覆盖「列已存在则跳过」分支；reader 列索引与 SELECT 一一对应；无吞异常/死循环/竞态；bridge 过渡值显式声明等价 v0.1.0 默认语义，真实接线归 kernel 分支，merge 顺序风险由 Lead 控制 |
| 可读性 | ✅（S2/S3/S4 备注） | 模块/结构体/字段注释解释「为什么」并交叉引用 01 §1.5、06 §6.2.8、06 §6.7、DR-005；迁移测试新增 `create_v3_database`/`gate_with_v4` 命名清晰；cli 过渡态注释明确指向后续分支。建议项：测试名 `v2_database_is_upgraded_to_v3...` 已过时（S3）；W2a0 spec 对同一 DDL 的列序展示与实现不同（S4）；测试函数过长（S2） |

## 3. 发现清单

| # | 级别 | 位置 | 问题 | 建议 | 状态 |
|---|---|---|---|---|---|
| S1 | 建议（待 W2a0 确认） | `crates/zgraph-trace/src/reader.rs:759-780`（`verdict: parse_json_opt(...)?`，`:779`） vs W2a0 spec 06 §6.2.8 注（docs 分支，未合入 main） | W2a0 spec 写明 `verdict_json`「读取失败按 NULL + 告警处理（不让 `trace` 崩）」；当前 `parse_json_opt` 失败会返回 `TraceError::Serialize` 使 `trace`/`guard` 读取失败（与 M6 其它 JSON 列严格报错口径一致）。按 Lead 本轮冻结契约清单（`verdict_json` NULL/值往返）**不阻塞** | W2a0 合并时确认口径：若保留该措辞，reader 对损坏 `verdict_json` 兜底 `None` + 告警（需先定告警通道，如 CLI stderr/warnings）；否则由 W2a-docs 把措辞改为与其它 JSON 列一致的严格报错。建议登记台账跟踪 | open（建议/待确认） |
| S2 | 建议 | `crates/zgraph-trace/tests/migration.rs:363-462`（100 行，新增）、`:72-138`（67 行，原 60）、`:255-316`（62 行，原 59）；`tests/roundtrip.rs:491-573`（83 行，新增） | 新增/改动测试函数超过「函数 ≤60 行」的等价约束（与 ISSUE-060/REV-044 ISSUE-089 同类口径；生产函数均合规） | 拆出 fixture/断言 helper（如 v3 迁移测试拆「构造 v3 库 / 触发迁移 / 断言备份与默认值」三段；roundtrip 拆 payload 与 severity 断言）；或登记 ISSUE 后置处理。不阻塞 | open（建议） |
| S3 | 建议 | `crates/zgraph-trace/tests/migration.rs:319` | 测试名 `v2_database_is_upgraded_to_v3_with_backup_and_preserves_rows` 仍写 v3，实际已迁移到 v4（断言 `TRACE_SCHEMA_VERSION`），名称与行为不符 | 更名为 `v2_database_is_upgraded_to_current_version_with_backup_and_preserves_rows`（或显式 v4） | open（建议） |
| S4 | 建议（文档口径） | `crates/zgraph-trace/src/schema.rs:206-222` vs W2a0 spec 06 §6.2.8（docs 分支） | W2a0 spec 的 `CREATE TABLE gate_results` 把 `deferred/attempt/verdict_json` 展示在 `at` 之前；实现把三列追加在 `at` 之后（fresh DDL 与 v3→v4 ALTER 均追加在表尾）。SQLite 列序无语义、读写均显式列名，**功能无影响**；但 DDL 级 spec 与实现存在文本差异 | W2a-docs 合并时统一：spec 注明「实现为表尾追加（ALTER 只能追加）/ 或调整展示顺序」，避免后续读者误以为有列序要求。不阻塞 | open（建议） |
| S5 | 建议 | `crates/zgraph-trace/tests/migration.rs:363-462`；`:185-213` | (a) v3→v4 测试只断言列名存在，未校验迁移后列的 type/NOT NULL/DEFAULT/CHECK 与 fresh DDL 一致（`DDL_CURRENT` 与 `GATE_RESULTS_V4_COLUMNS` 是两份手工定义，漂移无自动守卫）；(b) 只读拒绝仅测 v1 与 >4，spec §6.7 测试矩阵列 v1/v2/v3（同一 `ensure_readonly_version` 路径，价值低） | (a) 加 fresh↔migrated 的 `PRAGMA table_info`/`sqlite_master.sql` 对比断言防漂移；(b) 可低优先补 v3 只读拒绝用例；可合并登记一条台账。不阻塞 | open（建议） |

> 级别口径：阻塞 = 必须修复后才能给"通过"；重要 = 必须修复或登记 ISSUE；建议 = 可后置，但需修复或进台账。

## 4. 验证证据

```bash
# --- 范围/干净度 ---
cd /Users/zyzs/lczyzs/code/zgraph/.worktrees/trace-trace-writer
git status --porcelain --branch      # ## feat/trace-gate-defer...origin/main [ahead 1]；工作树 clean
git log --oneline -2
# 96ab28e feat(trace): GateResult 增 deferred/attempt/verdict，schema 升 v4
# 7f88614 docs(process): W1 完成收口——进度、台账（ISSUE-101..103）
git rev-parse HEAD origin/main        # 96ab28e... / 7f88614...
git show --stat 96ab28e               # 12 files changed, 353 insertions(+), 20 deletions(-)；正文含 Review: REV-070 / Agent: trace-writer
git diff 7f88614..96ab28e --name-status
# 9 × crates/zgraph-trace/**；3 × crates/zgraph-cli/**（Lead 批准机械适配）
git diff 7f88614..96ab28e --shortstat -- crates/zgraph-trace  # 9 files changed, 335 insertions(+), 20 deletions(-)
git diff 7f88614..96ab28e --shortstat -- crates/zgraph-cli    # 3 files changed, 18 insertions(+)
git diff 7f88614..96ab28e --name-only -- crates/zgraph-kernel crates/zgraph-dsl crates/zgraph-harness \
  crates/zgraph-worker doc CLAUDE.md schemas Cargo.toml Cargo.lock
# 空 → 未改 kernel/dsl/harness/worker/doc/CLAUDE/Cargo

# --- cli 范围逐 hunk 核对（Lead 声明的机械改动）---
git diff 7f88614..96ab28e -- crates/zgraph-cli
# bridge.rs +8：仅 map_gate_result 三字段过渡值（false/0/None）+ 指向 kernel 分支的说明注释
# run_cmd.rs +3：仅 mod tests（:1561 起）内 trace_input_gate 字面量
# tests/trace.rs +7：仅 fixture 字面量 + assert_tail_event_fields 三条 payload 断言
# 无其它 cli 改动；无删除/削弱既有断言

# --- 构建/测试（CARGO_HOME=/Users/zyzs/lczyzs/code/zgraph/.cargo-home，--offline）---
cargo test -p zgraph-trace --offline
# 63 passed / 0 failed（11 个 test binary；migration 10 passed、roundtrip 7 passed）
cargo test -p zgraph-cli --offline
# 160 passed / 0 failed（lib 81 + acceptance_m2 10 + replay 13 + run 31 + trace 14 + validate 11）
cargo fmt --all -- --check          # exit 0
cargo clippy --workspace --all-targets --offline -- -D warnings   # Finished，exit 0
cargo test --workspace --offline
# 650 passed / 0 failed / 3 ignored（63 个 test binary，含 e2e；无 issue-091 flaky 复现）
bash scripts/dev/check.sh           # CHECK PASSED（exit 0）
```

- 测试结果：trace **63 passed / 0 failed**；cli **160 passed / 0 failed**；全量 workspace **650 passed / 0 failed / 3 ignored**（`doc/test-baseline.txt` 失败基线 0 → 无新增失败；本分支 +2 用例：migration/roundtrip 各 1）。
- 迁移测试有效性抽查：`v3_database_is_upgraded_to_v4_with_gate_result_defaults` 真实构造 v3 库（当前库降级：`DROP` 三列 + `user_version=3`，保留 1 条 legacy gate 行），断言迁移前无 v4 列、迁移后 `user_version=4`、旧行 `deferred=false/attempt=0/verdict=None`、新行往返、`.bak.v3-*` 备份（`user_version=3`、无 deferred 列、保留旧行）、审计行 description 含 `W2a`；未发现恒真/削弱断言。既有失败注入恢复测试仍通过，本分支未破坏。
- 独立 sqlite3 探针（不依赖作者测试）：
  - 从 `src/schema.rs` 抽取 `DDL_CURRENT` 建新库 → `gate_results` 尾三列 `deferred INTEGER NOT NULL DEFAULT 0 CHECK(deferred IN (0,1))`、`attempt INTEGER NOT NULL DEFAULT 0`、`verdict_json TEXT`（PRAGMA table_info：`12|deferred|INTEGER|1|0|0`、`13|attempt|INTEGER|1|0|0`、`14|verdict_json|TEXT|0||0`）；
  - DDL 重复应用（`IF NOT EXISTS`）exit 0；新行默认 `0/0/NULL`；`deferred=5` → `CHECK constraint failed: deferred IN (0,1)`；
  - 模拟 v3 表 + 与 `GATE_RESULTS_V4_COLUMNS` 逐字一致的三条 `ALTER TABLE ... ADD COLUMN` → 旧行自动 `0/0/NULL`，CHECK 生效。
- `add_missing_columns` 可重复执行：代码先查 `PRAGMA table_info` 再补缺列（`schema.rs:581-592`）；v1→v4 迁移路径（`migrate_step(1)` 先执行含 v4 列的 `DDL_CURRENT`）实际覆盖「列已存在 → 跳过」分支并通过。
- 本评审未改产品/测试；评审记录与索引行属 `docs` 模块（按 AGENTS §3/§6 落在此分支，作者 trace-writer、评审人 reviewer-a ≠ 作者；第二评审 lead 待追加 cli 机械改动复核段）。

## 5. 遗留问题

- **S1–S5 为建议（非阻塞）**：S1/S4 需在 W2a0/W2a-docs 合并时确认口径（reader 兜底或 spec 措辞；DDL 列序表述）；S2/S3/S5 建议随 trace 测试批次或登记 `doc/issues.md` 处理（本评审按授权只改 `doc/reviews/**`，未改产品/测试/台账）。
- **已知/待确认**：DR-005 与 W2a0 spec 06 在 docs 分支（`docs/docs-rework-design`，REV-067）**未合入 main**；本轮按 Lead 冻结契约 + docs 分支 spec 交叉核对，代码/注释中的 `DR-005` 引用编号需 W2a0 合并时确认（Lead 已声明此点，不作为本分支问题）。
- **已知环境问题**：e2e 时序 flaky 属 ISSUE-091（外部 CPU 争用）；本轮全量 650 passed / 0 failed / 3 ignored，未复现。
- **跨波次缺口（非本分支）**：DR-004 §6.3 要求 W1 e2e 产出 `tests/fixtures/trace/schema_v3_seed.sql` + `tests/fixtures/trace/v0.1.0_expected_trace.json`（v0.1.0 v3 库迁移样例，v0.2 发布条件 §6.3）；当前 main/本分支 `tests/fixtures/trace/` 不存在。建议 e2e/docs 批次确认并登记（本分支 v3→v4 测试用「当前库降级」合成 v3，不等同发布条件要求的真 v0.1.0 库样例）。
- **第二评审**：`lead` 将按分工追加 cli 机械改动复核段；本记录已逐 hunk 确认 cli 改动范围，但不覆盖 cli 语义评审。
- 未发现越界改动、禁用/冻结文件修改；分支 base = `origin/main`（`7f88614`）；worktree clean；本评审未 merge/push/切 main。

## 6. 条件关闭记录（仅"有条件通过"填写）

不适用（本记录结论为"通过"，无未关闭条件；S1–S5 为非阻塞建议/待确认项）。
