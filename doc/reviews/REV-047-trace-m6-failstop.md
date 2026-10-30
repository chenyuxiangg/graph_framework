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

> **身份/范围**：`reviewer-a` 非作者；本分支跨 kernel + cli（`crates/zgraph-trace/**` 未改），按 AGENTS §2 由 Lead 批准并指定本人作第二评审。Lead 主评审已给「通过」；本小节独立核对接口边界/轮询语义/故障注入/范围，未修改任何代码，仅追加本小节。**第二评审结论：通过**（主评审结论生效，无新增阻塞项）。

### 6.1 `CODE_STORAGE_ERROR` 仅内存态、不写 `runs.error_code`（06 §6.12.3 D3）✅

- kernel `fail_run_storage`（`crates/zgraph-kernel/src/run.rs:1474-1497`）构造 `RunFinishedInfo { status: Failed, error: None, … }`，**只**把 `RunError::new(CODE_STORAGE_ERROR, message)` 放进内存态 `RunOutcome.error`（`:1496`）；`runs.error_code/error_message` 由 `RunFinished.error` 落库 → 保持 NULL。
- cli `map_run_finished`（`crates/zgraph-cli/src/bridge.rs:408-420`）在映射到 trace 时再做**防御性过滤** `filter(|e| e.code != kernel::CODE_STORAGE_ERROR)`（`:416`），其他领域错误码原样透传；`TraceBridge::take_error`（`:143`）只把错误**文本**交给内核，不落 `error` 列。
- 全仓 `CODE_STORAGE_ERROR` 消费点核对（`grep -rn`）：kernel 常量/`fail_run_storage`/单测、cli `exit_code_for`/单测/`map_run_finished` 过滤；**无任何写库路径**（`runs`/`instances`/`transitions` 的 `error`/`error_code` 列均不写入该码）。符合 D3「CLI/storage 命名空间不写 `error_code`」。

### 6.2 kernel 轮询点不改变既有控制/终态语义 ✅

- 轮询位置：`drive` 循环顶部、控制命令处理**之后**、`final_outcome`/`dispatch_ready`/`check_termination` **之前**（`run.rs:481-490`）；且仅在 `!status.is_terminal()` 时 `emitter.take_error()` → 已终态 run 不会被改写，错误留给 CLI `close_writer` 的 flush/close 暴露（符合 §6.4.5「尽力写终态」边界，主评审 #1 已记录）。
- 优先级自洽：同轮已有 Cancel/终态 `final_outcome` → 轮询跳过，Cancelled 优先；storage error 先出现则覆盖非终态（Pause 等）为 Failed（D2 fail-stop 优先）；`fail_run_storage` 内「取消非终态实例 → 清 ready → abort 任务 → 单次 `RunFinished{Failed,error=None}`」顺序与既有 `fail_timeout`/`cancel_run` 对齐，`RunFinished` 之后无 sink 调用（kernel 测试断言 `calls_after_finish == 0`）。
- 无行为回归面：`TraceSink::take_error` 是**默认 `None` 的 trait 方法**（`trace.rs:221-227`），Null/Memory sink 与所有既有实现零改动；`Emitter::take_error` 在 `trace_enabled=false` 时直接返回 None（`trace.rs:635`），`trace_disabled_does_not_poll_or_fail` 覆盖；轮询与 sink 调用在事件循环同一线程，无并发竞态；`TraceHandle::take_error` 用 Mutex `take()`（`writer/mod.rs:234-239`）取走一次，不会重复触发。

### 6.3 cli 退出码映射与 `map_run_finished` 过滤 ✅

- `exit_code_for(&outcome)`（`run_cmd.rs:1002-1013`）：仅 `Failed && error.code == STORAGE_ERROR` → **4**；Completed 0 / Failed 1 / Cancelled 130 维持原语义（`exit_code_mapping_matches_spec` 已覆盖 4 分支，独立复跑通过）。
- 实际 CLI 路径复核：writer fatal 后 `close_writer` 的 `flush()` 返回 fatal Err → `BridgeError::Trace` → `report_storage_error` 输出 `error[STORAGE_ERROR]` + exit 4；`exit_code_for` 的 STORAGE 分支作为**内存态兜底/嵌入调用方**路径，两条路径均落 4、无矛盾/重复（备注：真实 CLI 下 `exit_code_for` 分支被 `BridgeError` 路径先覆盖，属正常防御，不构成缺陷）。
- `map_run_finished` 仅过滤 storage 码，`NODE_TIMEOUT`/`NODE_INTERNAL` 等仍按原样写 `error_code` 列（diff 逐行核对）。

### 6.4 故障注入不污染生产路径、测试确定性 ✅

- kernel：`FailingSink` 仅在 `crates/zgraph-kernel/tests/failstop.rs` 内实现 `take_error`；生产 sink 默认 None，无测试后门进入产品代码。用例断言失败注入后 run Failed、`RunOutcome.error=STORAGE_ERROR`、`RunFinished.error=None`、3s 慢节点前中止、终态后零 sink 调用；`trace_enabled=false` 不误失败。
- cli：`inject_trace_write_failure`（`crates/zgraph-cli/tests/run.rs:631`）在 **tempdir 测试库**上建 `BEFORE INSERT ON events` trigger，匹配 compact JSON payload 的 `"to":"Completed"` + `"node_id":"fast"`（payload 由 `writer/tx.rs:355-370` 生成，字段名/无空格实测匹配）；不依赖 ENOSPC/只读 FS，`fast` 完成即确定性触发，`slow(5s)` 前中止。断言 exit 4 + stderr `error[STORAGE_ERROR]` + 不出现 `status=Completed` + DB 无 `Completed` + `RunFinished` 计数 0（writer fatal 后 `route` 丢弃新输入、`finish_batch` 直接返回 fatal，`thread.rs:237-240,286-293`）——与实现逐条对应。
- 生产路径无 trigger/env 钩子；故障注入只存在于测试 DB。
- 残留风险（非阻塞建议）：时序断言 `elapsed < 4s`（slow=5s）与 kernel `elapsed < 2s`（delay=3s）在重载机余量约 1s，属 ISSUE-091 同类风险；后续可放宽阈值或改为「DB 状态/取消事件」驱动断言。

### 6.5 跨模块范围与作者报告一致 ✅

- `git diff 67bc49b..a59393f --stat` = **7 files, +448/−15**，与主评审记录一致：kernel 3 生产（`run.rs`/`trace.rs`/`types.rs`）+ 1 新测试（`tests/failstop.rs`）；cli 2 生产（`bridge.rs`/`run_cmd.rs`）+ 1 测试（`tests/run.rs`）；**`crates/zgraph-trace/**` 未改动**（`git diff --name-only` 无 trace 路径）；无 Cargo.toml/doc/其他 crate 改动。
- 提交 `a59393f`：作者=提交者=仓库身份，subject `fix(trace): …`（模块主责 trace，实际改 kernel+cli 已由 Lead 批准 + 第二评审），正文含 `Review: REV-047` / `Agent: trace-dev`。范围与提交说明/作者报告一致。

### 6.6 独立验证（reviewer-a 实跑，私有 target）

```bash
# 不设 CARGO_TARGET_DIR（worktree 内私有 target）；CARGO_HOME=<repo>/.cargo-home，离线
$ cargo test -p zgraph-kernel --locked --test failstop
# test result: ok. 2 passed; 0 failed; 0 ignored（0.06s）
$ cargo test -p zgraph-cli --locked --test run trace_write_failure
# test result: ok. 1 passed; 0 failed（2.10s；slow 5s 用例提前中止）
$ cargo test -p zgraph-cli --locked --lib exit_code_mapping
# test result: ok. 1 passed; 0 failed（0.00s）
# 三轮均 exit 0；worktree clean（测试未留下未跟踪产物）
```

> 第二评审结论：**通过**。主评审「通过」+ 本小节 → REV-047 生效；无新增阻塞项；§6.4 的时序余量建议交后续（不阻塞合并）。

