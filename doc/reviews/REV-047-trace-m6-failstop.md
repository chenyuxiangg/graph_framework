# REV-047 · trace/kernel/cli · ISSUE-036 fail-stop 接线

- **分支**：`fix/trace-m6-failstop`
- **作者**：`trace-dev`（提交 `a59393f`）
- **评审人**：`lead`（**≠ 作者**；主评审）＋ `reviewer-a`（**跨模块第二评审**，AGENTS §2 要求）
- **变更范围**：`main(67bc49b)..a59393f`，7 文件 +448/−15：`crates/zgraph-kernel/**`（3 生产 + 1 新测试）、`crates/zgraph-cli/**`（2 生产 + 1 测试）；**trace crate 未改动**；worktree clean
- **评审时间**：2026-10-31
- **关联**：ISSUE-036；`doc/spec-v1.2/06` §6.4.5/§6.10.5/§6.12.3 D3；DR-003 §3 D5；REV-041（双通道）

> **跨模块声明**：本分支同时改 kernel 与 cli（AGENTS §2 需 Lead 批准并指定第二评审人）。Lead 已批准；第二评审人 = `reviewer-a`（第二评审小节见文末，合入前追加）。

## 1. 结论

**结论：通过**（主评审；第二评审小节由 reviewer-a 追加后生效）

## 2. 四维评审

| 维度 | 结论 | 依据（文件/行/测试） |
|---|---|---|
| 功能正确性 | ✅ | kernel：`TraceSink::take_error`（默认 None，存量 sink 零改动）→ `drive` 每轮（控制处理之后、终态判定之前）轮询；非终态命中 → `fail_run_storage`：内存态 Failed、非终态实例→Cancelled、abort 在飞任务、**尽力** `RunFinished{Failed}`（`error=None`，`runs.error_code/error_message` 保持 NULL，06 §6.12.3 D3）；终态后不再轮询，把错误留给 CLI flush/close 暴露（不静默成功）。cli：`run_cmd::exit_code_for` 内存态 `STORAGE_ERROR` → **exit 4**；`map_run_finished` 防御性过滤该码不写 `error_code` 列。测试：kernel `tests/failstop.rs`（2 用例：注入失败 → run Failed/STORE_ERROR/3s 慢节点前中止/RunFinished error=None/终态后零 sink 调用；`trace_enabled=false` 不误失败）；cli `trace_write_failure_fails_run_and_exits_4` 用 **SQLite trigger** 制造确定性真实写失败（不依赖 ENOSPC/只读 FS），断言 exit 4 + stderr `error[STORAGE_ERROR]` + 未出现 Completed + 5s 慢节点前中止。 |
| 代码结构合理性 | ✅ | 错误通道用 `TraceSink` 默认方法扩展，未破坏既有 Null/Memory sink；内存态错误码 `CODE_STORAGE_ERROR` 仅在 `RunOutcome.error` 传递、不入库（符合「STORAGE_ERROR 属 CLI/storage 命名空间」）；故障注入用 SQLite trigger 而非生产代码 env 钩子，更贴近真实写失败。 |
| 逻辑性 | ✅ | 轮询点位于控制命令之后、终态判定之前；非终态才轮询避免吞掉 CLI close/flush 应暴露的错误；实例/任务收尾顺序（Cancelled + abort + 终态 RunFinished）自洽；`trace_enabled=false` 跳过轮询（无可轮询写端）。 |
| 可读性 | ✅ | 新常量带文档注释说明命名空间边界；`fail_run_storage` 命名与 `fail_timeout` 对齐；测试注释说明 SQLite trigger 故障注入的确定性理由。 |

## 3. 发现清单

| # | 级别 | 位置 | 问题 | 建议 | 状态 |
|---|---|---|---|---|---|
| 1 | 备注（口径已确认） | `RunOutcome.error` / `runs` 表 | 真实 fatal 场景下写线程已停止，"尽力写终态" 的 RunFinished 可能被丢弃（DB 可能无 RunFinished、runs 停留 Running），符合 §6.10.5「Failed（若可写）」；「尽力写」语义由 kernel 单测（正常 sink）覆盖 | 接受；docs 回写（REV-057）在 06 §6.4.5 注明该边界 | open（转 REV-057） |
| 2 | 备注 | `--json` 模式 | fail-stop 路径不输出结果 JSON（仅 stderr `error[STORAGE_ERROR]` + exit 4）；spec §6.4.5 只要求 stderr，无 JSON 契约 | 接受现状；如需要可后续立任务 | 无需处理 |
| 3 | 备注 | clippy | 本分支无新增 warning；main 上仍有 cli `unnecessary_map_or`（REV-043 引入）与已由 REV-053 修复的 harness 2 处 → 由 REV-058 的 CI/clippy 门禁分支统一清 | 登记 ISSUE-094（merge.sh 缺 clippy 门禁） | open（转 REV-058） |

无阻塞项。

## 4. 验证证据

```bash
# 评审人在干净 target（/tmp/zgraph-merge-target，先 cargo clean 全部 workspace 成员）上执行
CARGO_HOME=<repo>/.cargo-home CARGO_NET_OFFLINE=true
cargo clean -p zgraph-dsl -p zgraph-kernel -p zgraph-trace -p zgraph-worker -p zgraph-harness -p zgraph-cli -p zgraph-e2e
cargo test -p zgraph-trace -p zgraph-kernel -p zgraph-cli --quiet
```

- 结果：三个 crate 全部 test target **0 failed**（作者报告 39 binaries / 274 passed；评审人复跑 exit 0，含 failstop 2 用例与 cli `trace_write_failure_fails_run_and_exits_4`）。
- 作者另报告：`cargo fmt --all --check` OK、`check.sh` PASS、本改动无新增 clippy warning。

## 5. 遗留问题

- 06 §6.4.5 的「尽力写终态」边界与 `--json` 口径 → REV-057 回写。
- merge.sh clippy 门禁 → ISSUE-094 / REV-058。
- 无阻塞项。

## 6. 跨模块第二评审（reviewer-a）

> 待 reviewer-a 追加：确认 kernel↔cli 接口边界（`CODE_STORAGE_ERROR` 仅内存态、不写 `error_code` 列）、故障注入不污染生产路径、跨模块改动范围与提交声明一致。
