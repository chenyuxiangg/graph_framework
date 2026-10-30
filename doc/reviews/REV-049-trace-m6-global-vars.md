# REV-049 · trace/cli · ISSUE-087 `global_vars` 记录与重放（trace schema v3）

- **分支**：`feat/trace-m6-global-vars`
- **作者**：`trace-dev`（提交 `78a3c40`）
- **评审人**：`lead`（**≠ 作者**；主评审）＋ `reviewer-a`（**跨模块第二评审**，AGENTS §2 要求）
- **变更范围**：`main(c35028a)..78a3c40`，17 文件 +853/−61：`crates/zgraph-trace/**`（7 文件，schema v3 + 迁移 + writer/reader/model/tests）、`crates/zgraph-cli/**`（10 文件，bridge 注入 + replay 重建 + 测试）；**kernel 未改**；worktree clean、基于最新 main
- **评审时间**：2026-10-31
- **关联**：ISSUE-087；`doc/spec-v1.2/06` §6.2.4/§6.7/§6.9；architecture §5.2；DR-003 §3 D4；REV-030/032（replay）

> **跨模块声明**：trace + cli（kernel 未改）。Lead 已批准；第二评审人 = `reviewer-a`（第二评审小节见文末，合入前追加）。
> **schema 变更单独评审**（DR-003 D4）：本记录即为 v3 迁移/备份/兼容策略的专项评审。

## 1. 结论

**结论：通过**（主评审；第二评审小节由 reviewer-a 追加后生效）

## 2. 四维评审

| 维度 | 结论 | 依据（文件/行/测试） |
|---|---|---|
| 功能正确性 | ✅ | trace：`TRACE_SCHEMA_VERSION` 2→3，`runs` 增 `global_vars_json TEXT`；迁移重构为**逐版本循环**（v1→v2→v3）：每步迁移前 `VACUUM INTO '<db>.bak.v{old}-{ts}'`（并回写备份 user_version 支持恢复）、单版本单事务、失败删 WAL/SHM + 备份覆盖恢复 + `Storage` 错误；新库单事务建 DDL 并写 1/2/3 基线审计行；只读连接拒绝旧版本（不迁移）。`write_run_started` 以 **canonical JSON**（对象键递归排序，规避工作区 `preserve_order`）落库，非 object → 不可恢复写错误（回滚）；`RunRow.global_vars` 暴露（`None`=v3 前历史 run）。cli：`TraceBridge` 在启动前抓 `RunSpec.global_vars` 注入 RunStarted（kernel 未改）；`replay --mode=outputs` 从记录重建 `RunSpec.global_vars`；历史 NULL → 空 map + stderr 告警；非 object → `REPLAY_RECORD_CORRUPT`(3)；v2 库只读拒绝（`unsupported schema version=2` exit 4）。新增测试覆盖：新库建表/版本=3、v1→v3、v2→v3 + 备份可恢复、注入 ABORT 迁移失败→恢复 v2 原样、canonical 落库字节/回显/空 `{}`/non-object fatal、replay 读取已记录值、legacy NULL 告警、non-object exit3、v2 exit4。 |
| 代码结构合理性 | ✅ | 迁移逻辑集中到 schema.rs 的逐版本循环（后续 v4 可复用）；canonical 序列化集中在 writer/tx；cli 只做「组合根注入 + replay 重建」，未改 kernel 契约；`RunRow.global_vars` 为 Option 保持历史兼容语义。 |
| 逻辑性 | ✅ | 备份/单事务/失败恢复顺序正确（失败时删 WAL/SHM 再覆盖备份，避免 WAL 回放污染）；回写备份 `user_version` 使恢复后版本一致；replay 只读不迁移、v2 库报存储错误的行为与 06 §6.9 一致；`global_vars` 非 object 归类为记录损坏/写错误而非静默降级。 |
| 可读性 | ✅ | schema 版本常量与迁移注释说明「forward-only + 备份」；兼容策略（NULL 空 map + 告警、replay 不迁移、备份不自动清理为运维动作）写入代码文档；测试命名表达语义（`migration..._failed_recovers_backup` 等）。 |

## 3. 发现清单

| # | 级别 | 位置 | 问题 | 建议 | 状态 |
|---|---|---|---|---|---|
| 1 | 备注（契约） | `zgraph run` | CLI 无 `--global-vars` 入口（06 §6.10.3 未定义），CLI run 记录 `{}`；非空值由库 API（bridge）覆盖 | 不新增 CLI 参数（避免未定义的契约变更）；REV-057 在 06 §6.2.4/§6.10.3 注明该口径 | open（转 REV-057） |
| 2 | 备注（运维） | 备份文件 | `<db>.bak.v{old}-{ts}` 不自动清理（避免误删唯一快照）；06 §6.7 的「保留最近 3 份」按运维动作处理 | REV-057 在 06 §6.7 注明 M6 口径 | open（转 REV-057） |
| 3 | 建议（后置） | `06 §6.2.4/§6.9` | 「`global_vars` 未落库」旧口径已过时 | REV-057 回写 | open（转 REV-057） |

无阻塞 / 重要项。

## 4. 验证证据

```bash
# 评审人在干净 target（/tmp/zgraph-merge-target，先 cargo clean 全部 workspace 成员）上执行
CARGO_HOME=<repo>/.cargo-home CARGO_NET_OFFLINE=true
cargo clean -p zgraph-dsl -p zgraph-kernel -p zgraph-trace -p zgraph-worker -p zgraph-harness -p zgraph-cli -p zgraph-e2e
cargo test -p zgraph-trace -p zgraph-cli --quiet
```

- 结果：`zgraph-trace` + `zgraph-cli` 全部 test target **0 failed**（作者报告 18 binaries / **197 passed**，含 v1→v3、v2→v3、迁移失败恢复、global_vars 回显、replay 一致性等新用例）；评审人复跑 exit 0。
- 作者另报告：`cargo clippy --workspace --all-targets --locked -- -D warnings` clean、`cargo fmt --all --check` OK、`check.sh` PASS。

## 5. 遗留问题

- 06 §6.2.4/§6.7/§6.9/§6.10.3 的口径回写（global_vars 落库与兼容、备份保留、CLI 无参数）→ REV-057。
- 无阻塞项。

## 6. 跨模块第二评审（reviewer-a）

> **身份/范围**：`reviewer-a` 非作者；本分支跨 **trace（schema v3 + 迁移 + writer/reader）+ cli（bridge 注入 + replay 重建）**，kernel 未改，按 AGENTS §2 由 Lead 批准并指定本人作第二评审。Lead 主评审已给「通过」；本小节为 **schema 变更专项（DR-003 D4）** + 跨模块第二评审，独立核对迁移/备份/恢复、canonical JSON、replay 兼容、cli 注入边界与回归影响，未修改任何代码，仅追加本小节。**第二评审结论：通过**（主评审结论生效，无新增阻塞项）。

### 6.1 逐版本迁移：备份 + 单事务 + 失败恢复（DR-003 D4 / 06 §6.7）✅

- 版本常量 `TRACE_SCHEMA_VERSION = 3`（`crates/zgraph-trace/src/schema.rs:24`）；`open_writer_connection`（`:267-306`）：`version == 0` → `create_current`；否则**逐版本循环** `while current < 3`：先 `backup_database(conn, path, current)`（`:291`），再 `migrate_step`（`:292`）；失败时 `drop(conn)` → `restore_backup`（`:293-300`），恢复失败返回「migration…backup restore also failed」组合 Storage 错误。
- `migrate_step`（`:363-383`）：`begin_immediate` 单版本单事务；`from=1` 执行 `DDL_CURRENT` + 补 v2 列 + 写审计 v2；`from=2` 补 `global_vars_json` + 写审计 v3；未知版本 → Storage（不可静默跳过）。
- `backup_database`（`:436-469`）：`VACUUM INTO '<db>.bak.v{old}-{ts}'`（路径内 `'` 转义为 `''`）；显式打开备份写回并**校验** `user_version = old_version`（注释说明 `VACUUM INTO` 不保证保留 DB 头版本，这对「恢复回旧版本」是必要的）；备份不自动清理（06 §6.7「保留最近 3 份」交运维，REV-057）。
- `restore_backup`（`:472-494`）：先删 `-wal`/`-shm`（NotFound 容忍），再 `fs::copy` 备份覆盖 DB——顺序可避免 WAL 回放污染恢复后的旧库。
- 测试与独立复跑：`fresh_database_migrates_to_current_version_with_all_tables_and_indexes`（`migration.rs:72`，含 `user_version=3`、`schema_migrations` 1/2/3、无备份文件）；`v1_database_is_upgraded_to_current_version_in_place`（`:246`，v1→v3 且留下 `.bak.v1-`/`.bak.v2-` 两份备份）；`v2_database_is_upgraded_to_v3_with_backup_and_preserves_rows`（`:307`，备份 `user_version=2`、无 v3 列、原 run 行保留）；`failed_migration_restores_backup_and_leaves_v2_untouched`（`:351`，注入 ABORT → 恢复 v2/user_version=2/v3 列消失/数据保留/备份保留，清触发器后可重试迁移成功）。**独立执行** `cargo test -p zgraph-trace --locked` → **58 passed / 0 failed**（10 个测试二进制，含上述全部）。
- 残留（非阻塞）：失败恢复测试只显式注入 v2→v3（v1→v2 共用同一 `restore_backup` 路径但未单独注入）；可选补一条（§6.7 #1）。

### 6.2 新库基线与只读拒绝 ✅

- 新库：`create_current`（`:352-360`）单事务执行 `DDL_CURRENT` 并 `for version in 1..=3 { record_migration }`；`record_migration`（`:407-420`）同事务写 `schema_migrations(version, description, applied_at)` + `PRAGMA user_version = version`（注释说明 user_version 随回滚撤销）。测试断言 1/2/3 审计行各恰 1 条（`migration.rs:72`）且**新库不产生备份**。
- 只读连接：`ensure_readonly_version`（`:513+`）`>3` → `SchemaTooNew`；`<3` → `Storage("unsupported schema version=X, expected 3")`，且**不迁移**（只读打开本身不调用 writer/迁移）。测试：`readonly_reader_rejects_newer_schema_without_migrating`（`:179`）、`readonly_reader_rejects_v1_schema_without_migrating`（`:207`，版本仍为 1）；CLI 侧 `replay_outputs_requires_current_schema`（`cli/tests/replay.rs:432`）对 v2 库断言 exit 4 + `unsupported schema version=2`。
- 备注（口径，非阻塞）：旧 v1/v2 库升级**不 backfill** 历史 `schema_migrations` 行（v2 库升级后新增 3；新库才有完整 1/2/3 链）。若要求「每库完整版本链」需在迁移中补写，否则由 REV-057 注明「审计行从升级起点起」——需 Lead 定口径（§6.7 #2）。

### 6.3 `global_vars_json` canonical JSON / 非 object 致命 ✅

- `canonical_json`（`crates/zgraph-trace/src/writer/tx.rs:249-263`）：object 的 key 递归排序后**重建新 Map**（不依赖 `serde_json::preserve_order`；工作区因 genai 开启该 feature 时插入序=排序序），array 递归、标量原样。`write_run_started`（`:265-310`）先校验 `global_vars.is_object()`，否则返回 `TxError::Fatal(TraceError::Serialize("RunStarted.global_vars must be a JSON object"))`，再 `json_req(&canonical_json(...))` 写入 `runs.global_vars_json`。
- 测试：`global_vars_is_persisted_as_canonical_json_and_read_back`（`global_vars.rs:20`）断言原始落库字节严格为 `{"flags":["beta","alpha"],"limits":{"a":1,"z":3},"tenant":"acme"}`（乱序+嵌套递归排序，`preserve_order` 背景下仍稳定）；`empty_global_vars_is_recorded_as_empty_object`（`:57`）落库 `{}`；`non_object_global_vars_is_rejected_as_fatal_write_error`（`:73`）断言 `flush()`/`close()` 错误、错误文本含 "global_vars must be a JSON object"、坏 `RunStarted` 事务回滚（只读连接查不到该 run）。
- 与 REV-047 fail-stop 的交互（备注）：非 object 属调用方契约违反；写线程 fatal 后按既有 fail-stop 归还错误（CLI 退出码 4），不会静默降级；字段文档已写 `MUST 是 JSON object`（`model.rs:324-328`）。

### 6.4 replay 兼容与 cli 注入（kernel 未触碰）✅

- cli 注入：`TraceBridge` 新增 `global_vars: Value` 快照（`crates/zgraph-cli/src/bridge.rs:70-75`），`with_snapshot`（`:99`）由组合根使用；`run_started`（`:131-136`）在 `map_run_started` 结果上覆盖 `global_vars` 再落 trace；`start_traced_run_inner`（`:405`）在把 `spec` 交给 kernel 前抓取 `RunSpec.global_vars`（`:409-416`，`global_vars_json:446`）。两个公开入口（`:387`/`:401`）均走 inner。**kernel 未改**（`git diff` 无 `crates/zgraph-kernel/**`；kernel `RunStartedInfo` 契约未变，trace 类型新增字段只在 CLI 映射层填充）。bridge 单测 `run_in_process_records_global_vars_snapshot`（`:1051`）验证快照落库。
- replay：`build_spec`（`replay_cmd.rs:436`）用 `global_vars_from_run`（`:469-489`）：object → 逐键还原；`None`（v3 前历史 run）→ 空 map + stderr warning；非 object → `REPLAY_RECORD_CORRUPT`（exit 3）。CLI 测试：`run_records_empty_global_vars_snapshot`（`replay.rs:365`，CLI run 记录 `{}`）、`replay_outputs_reads_recorded_global_vars`（`:377`）、`replay_outputs_legacy_run_without_global_vars_warns_and_falls_back`（`:397`，NULL→匹配成功+告警）、`replay_outputs_rejects_non_object_global_vars_snapshot`（`:417`，exit 3 + `error[REPLAY_RECORD_CORRUPT]`）、`replay_outputs_requires_current_schema`（`:432`，v2→exit 4）。replay 只读、不迁移（06 §6.6/§6.7），符合 D4。
- 重建有效性：`build_spec_restores_recorded_global_vars`（`replay_cmd.rs:681`）与 `replayed_spec_exposes_global_vars_through_ctx_get_var`（`:689`，节点经 `ctx.get_var` 读取并复现输出）验证非空 global_vars 真正进入重跑环境。
- **独立执行**：`cargo test -p zgraph-cli --locked --test replay replay_outputs` → **9 passed / 0 failed**（4 filtered；含上述 legacy/non-object/current-schema/reads-recorded）。与 Lead 的 trace+cli 197 passed 结论一致。

### 6.5 回归影响与合并顺序 ✅

- trace 公共 API：`RunStartedInfo` 新增必填 `global_vars`（`model.rs:311,324-328`）、`RunRow` 新增 `global_vars`（`reader.rs:86,135-139`，SELECT 第 25 列与 `map_run_row` 索引 24 对齐）——对库消费者属 **breaking change**（`#[non_exhaustive]` 未用）；v0.1 未冻结、仓内全部构造点已更新（`grep` 无遗漏，trace/cli 测试编译通过）。建议在 REV-057/README 注明（§6.7 #3）。
- e2e：`tests/e2e/**` 未改；无 `RunStartedInfo`/`TraceInput::RunStarted` 构造（`grep` 0 命中），两处 `"schema_version": 1` 是 **graph DSL** 版本、非 trace DB schema → 不影响 e2e 编译/断言。merge.sh 会跑全 workspace 作最终确认。
- rebase：本分支基于 main `c35028a`，当前 origin/main 已到 `398bf76`（含 REV-052），落后 3 commits；唯一文件交集是 `crates/zgraph-cli/src/run_cmd.rs`（REV-052 改 `build_executor`，本分支只加测试 fixture 一行 `global_vars`，约 `:1745`），预计 rebase 无冲突；rebase 后若 diff 变化需复核。
- spec 旧口径：06 §6.2.4（`06:961/965` L2 重跑/已知边界仍写「global_vars 未落库」）、§6.7（步骤 5/6 仍以 `supported=1`/v1 为例）、§6.9、§6.10.3 需 REV-057 回写（主评审 §3 #3、§5 已记录）✅。
- 备注：提交正文把 spec 回写写成 `REV-056`（W1 收口后的编号应为 **REV-057**；记录正文/DR-003 已是 057）；提交历史不可改，无代码影响，后续台账/总结勿沿用。

### 6.6 建议（非阻塞）

1. 补一条 v1→v2 迁移失败注入（复用 `restore_backup`；当前只覆盖 v2→v3），强化备份恢复回归。
2. 明确「旧库升级后 `schema_migrations` 不 backfill 历史行」的口径（REV-057 注明或迁移中补链）。
3. trace 公共 API breaking（`RunStartedInfo::global_vars` 必填、`RunRow` 新字段）在 REV-057/README 记录，供库消费者迁移。
4. CLI e2e 的 global_vars 用例可改用依赖 `ctx.get_var` 的图（当前 `replay_outputs_reads_recorded_global_vars` 仅断言 matched，真正重建由 replay_cmd 单测覆盖）。
5. 提交正文 REV-056→REV-057 口径（仅文档/历史说明，不阻塞）。

> 第二评审结论：**通过**。主评审「通过」+ 本小节 → REV-049 生效；§6.6 建议可随 REV-057/后续测试处理，不阻塞合并。

