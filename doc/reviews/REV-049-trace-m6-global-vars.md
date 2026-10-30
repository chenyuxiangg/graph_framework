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

> 待 reviewer-a 追加：确认 schema v3 迁移/备份/恢复正确性（专项）、canonical JSON 落库、replay 兼容策略、cli 注入不触碰 kernel、与 07 AC 的回归影响。
