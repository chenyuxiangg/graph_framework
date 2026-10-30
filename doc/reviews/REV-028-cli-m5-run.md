# REV-028 · cli · M5 `zgraph run` 真链路 + `--record` 落库 + PYO3/exit70/:memory:

> **模块声明**：本分支主要改 `crates/zgraph-cli/**`（`cli` 模块：`Cargo.toml`、`src/{bridge,lib,main,run_cmd,validate}.rs`、`tests/run.rs`），另改根 `Cargo.lock`（**`process` 模块**，仅 `zgraph-cli` 依赖列表 4 行机械新增、无新包/无版本变更）。按 AGENTS §2 属**跨模块**（cli + process）；本记录由**非作者**评审人 `reviewer-a` 声明并作独立第二评审（REV-020/022 的 `Cargo.lock` 跨模块先例）。评审**未修改分支任何文件**，只新增本记录；未 merge / 未 push / 未改 main。
>
> **首轮评审对象**：`5d2e53d..a3b15ef`（2 commit：`ad5548a` feat run+record、`a3b15ef` fix PYO3/70/:memory:）。**增量复审对象（最终）**：`b27d9c0..9b58be4`（3 commit：`a23538c` feat、`23c805c` fix、`9b58be4` 首轮本记录）；分支已 rebase 到 `b27d9c0`（含 REV-038/ISSUE-084），**代码内容与首轮对象完全一致**（`git diff a3b15ef HEAD -- crates/zgraph-cli Cargo.lock Cargo.toml` 为空，仅 base/docs 变化）。C1 关闭与最终门禁见 **§8**；首轮原文保留，不删除原记录。

- **分支**：`feat/cli-m5-run`
- **作者**：`cli-dev`
- **评审人**：`reviewer-a`（**非作者**）
- **变更范围**：首轮 `5d2e53d..a3b15ef`（8 files changed, 2633 insertions(+), 29 deletions(-)）；**最终** `b27d9c0..9b58be4`（代码 diff 同首轮，另含本评审记录 1 文件；base = `b27d9c0` = `main`，基于最新 main；worktree clean）
- **评审时间**：2026-10-30
- **关联**：DR-002 §1（CLI 全契约）/§2/§3.1/§4/§9 #1/#5（`--record`/L2 记录/环境变量口径）；spec 06 §6.10.1–§6.10.6、§6.8.1、§6.9.1；spec 05 §5.3.4/§5.9.2；spec 04 §4.3.6；spec 08 N-02；architecture §2.2/§5.2/§5.3；07 AC-01/AC-08；ISSUE-027/036/047/048/054/059/061/076/082；REV-025/REV-026

## 1. 结论

**结论：通过**（增量复审：首轮为「有条件通过」，C1 已由 `b27d9c0`（REV-038/ISSUE-084）关闭，证据见 §8；首轮条件原文保留于下，不删除原记录）

`zgraph run` 的**参数/默认值/输入校验/退出码/输出结构/`--record`/PYO3 优先级/`:memory:`/SIGINT/panic→70** 与 06 §6.10.3–§6.10.6 逐条核对**基本一致**，测试全部通过「真实二进制 + 真 Python worker + 真 SQLite」链路（无 mock、无网络），`--record=full/hash/none` 语义与 `output_hash` 复算经独立核对成立；`Cargo.lock` 为机械 diff；`cargo test -p zgraph-cli` 87 passed、fmt/clippy/check.sh PASS。未发现阻塞项。

首轮发现 1 项重要问题（C1）；已通过 **ISSUE-084**（P2，trace/cli）+ **DR-002 §4/§6/§9** 登记并固化关闭条件（§6/§8），首轮条件原文保留如下：

- **C1（重要）**：**06 §6.8.1/§6.10.3 的崩溃标定（中止标定）未实现**。spec 06 §6.10.3 规定 `zgraph run` 启动顺序为「打开 DB → 迁移检查 → **中止标定（§6.8.1）** → 编译图…」，§6.8.1 规定把「无活动进程的 Running/Paused run」标定为 `Failed + interrupted=1` 并在 stderr/`--json recovered[]` 呈现；本分支启动段（`run_cmd.rs:684-753`）只做 open/migrate + 图/输入/provider/executor，**无标定**，`--json` 的 `recovered` 恒为 `[]`（`run_cmd.rs:958`）。trace crate 自身文档（`crates/zgraph-trace/src/lib.rs:33`）也明写「崩溃标定/`interrupted=1` 的写入口属 run 生命周期（M3/M5）；本 crate 只在 schema/reader 暴露列」——即该项应在 M5 由 run 生命周期（CLI/bridge）接线。现状后果：进程被 kill -9/掉电后 run 永远停留 `Running`，`trace` 的 interrupted 截断提示（06 §6.8.3）不会触发，违反 06 §6.8/§6.10.3。属 **M5 CLI 全契约缺口**（且未在 `doc/issues.md` 登记、DR-002 §7 也未列为 M6 排除项）。建议：由 Lead 在 `doc/issues.md` 登记 ISSUE（trace+cli 跨模块：trace 提供标定 API/写入口 + CLI 启动时调用；含 `recovered[]` 输出与测试），并在 DR-002 §7/§9 或 M5 docs 注记（M5 wave2 实现或明确 M6）；登记编号后本评审复核转「通过」。

其余建议/Nit/备注见 §3（状态已按增量复审更新：ISSUE-085 登记、ISSUE-081 扩展、ISSUE-082 部分修、`timeout_hint` 转发列 wave2 REV-030 等）；均不阻塞本分支。

## 2. 四维评审

| 维度 | 结论 | 依据（文件/行/测试/命令） |
|---|---|---|
| 功能正确性 | ✅ | ① **CLI 契约**：06 §6.10.3 参数/默认值逐项一致（`--python` 优先级 `--python > PYO3_PYTHON > ZGRAPH_PYTHON > python3` 见 `run_cmd.rs:564-570`；`--db` 默认 `.zgraph/trace.db`/`ZGRAPH_DB` 见 `trace_cmd::resolve_db_path`；`--user-id local`；`--assets-dir .`/`ZGRAPH_ASSETS`；`--tools-dir <assets>/tools`/`ZGRAPH_TOOLS`；`--fake-llm[=<script>]` 含裸 flag 内置脚本 `run_cmd.rs:116-135`；`--real` 互斥；`--stream` 只写 stderr；`--json` 严格 §6.10.6）。② **输入形状**：嵌套总允许 + 单 entry 简写 + 非 entry/未声明端口 → `RUN_ENTRY_INVALID`、required 缺失 → `RUN_ENTRY_INPUT_MISSING`（`run_cmd.rs:294-399`），均 exit 3 且**不产生 run**（集成测试逐例查 `runs_count=0`，`tests/run.rs:443-479`）。③ **退出码**：0/1/2/3/4/70/130 全有映射与测试（`exit_code_for` `run_cmd.rs:916-923`；70 见 lib.rs `catch_unwind`；130 见 SIGINT 集成测试）；④ **`--record`**：默认 hash；hash 只写 `output_hash`（`output_json=NULL`）、full 写全文、none 不写（`record_output` `run_cmd.rs:474-496`）；hash 为 `blake3(canonical_json)` 64 hex 且可复算（`run_cmd.rs:499-505` + 测试 `run_ac08_real_chain...`）；`runs.record_mode` 落库（`ad5548a` 透传 kernel `RunOptions.record_mode`，测试断言 hash/full/none）。⑤ **`:memory:`** 接受 + 警告、结果走 outcome、`event_count=0`（`run_cmd.rs:551-562,684-695,832-855`；测试 `run_memory_db...`）。**增量复审**：C1 已由 ISSUE-084（P2，wave2 trace API REV-034 + CLI 调用 REV-030；不列 M6 排除）与 DR-002 §4/§6/§9 固化关闭；#2 登记 ISSUE-085（M6）、#4 台账记为部分修（ISSUE-082），扣分项消除。 |
| 代码结构合理性 | ✅ | 职责分层清晰：`run_cmd`（参数/启动/输出/错误映射）+ `bridge`（kernel→trace 桥、`StreamingObserver`、`:memory:` 无 reader 的 `wait_without_reader`）+ `lib`（分发/顶层 panic 守卫）+ 复用 M1 `validate::print_errors`；执行器组合 `RecordingExecutor → RoutingExecutor → {HarnessExecutor, WorkerExecutor}` 与 DR-002 §3.1「CLI 外层包装写 recorded_outputs」一致。依赖方向正确（cli → harness/worker/kernel/trace/dsl，无反向）；不侵入 harness/worker/kernel（harness 的 `llm_calls` seam 属 wave2）。跨模块仅根 `Cargo.lock` 机械改动（§3 #10）。 |
| 逻辑性 | ✅ | 启动顺序、失败清理（图/输入/provider 失败时 `close_quietly(writer)` 不留写线程）、exit code 映射链（run 状态→0/1/130；解析失败→2；入口→3；存储/写端→4）自洽；`--json` 时 stdout 仅 JSON、进度全 stderr；`--quiet` 不抑制错误；SIGINT 只置标志、由轮询循环 cancel，避免早到信号丢语义（`run_cmd.rs:832-861`）；RecordingExecutor 在**成功**返回后才发 `RecordedOutput`（失败节点不记，符合 L2 语义）；hash 计算 canonical JSON 与 graph_hash 同源，键序无关（测试覆盖）。为可重复单测把 panic 守卫抽成 `run_with`（`lib.rs:113-129`），无生产后门。 |
| 可读性 | ✅ | 模块文档说明启动顺序/执行器组合/`:memory:`/panic 口径；关键「为什么」有注释（如为何用 `..Default::default()` 避开 test-hooks 字段、为何 human 生命周期是回读而非实时）；错误信息带 `error[CODE]` 前缀；测试按「参数与输出 / 退出码 / 失败 run / 真链路 / 解释器优先级 / :memory: / SIGINT」分段，命名可读。Nit：`parse` 79 行、`run_in_runtime` 70 行超过 architecture §2.2「crates 函数 ≤ 60 行」等价约束（#3）。 |

## 3. 发现清单

| # | 级别 | 位置 | 问题 | 建议 | 状态 |
|---|---|---|---|---|---|
| 1 | 重要（C1） | `crates/zgraph-cli/src/run_cmd.rs:684-753,958`（对照 spec 06 §6.10.3 启动顺序 `06:771`、§6.8.1 `06:621`、`crates/zgraph-trace/src/lib.rs:33`） | 启动流程缺「中止标定（§6.8.1）」：崩溃/掉电后残留的 Running/Paused run 不会被标定 `Failed+interrupted=1`，`--json recovered[]` 恒空，`trace` 截断提示不触发；违反 06 §6.8/§6.10.3，且未登记台账 | Lead 在 `doc/issues.md` 登记 ISSUE（trace+cli 跨模块：标定 API/写入口 + CLI 启动调用 + stderr/`recovered[]` + 测试），并在 DR-002 §7/§9 或 M5 docs 注记（wave2 实现或 M6 明确）；登记编号后本评审转通过 | fixed（ISSUE-084 登记 + DR-002 §4/§6/§9；见 §8） |
| 2 | 建议 | `crates/zgraph-cli/src/run_cmd.rs:993-1010` | human 模式生命周期事件在 run **结束后**回读 trace 打印（代码自述「不是实时进度」），与 06 §6.10.3「运行中 stderr 每行一条生命周期事件」的字面不符；用户运行中看不到进度行 | 登记 ISSUE/M6 或 wave2 用 sink 观察者实现实时骨架（可复用 `StreamingObserver` 扩展为非流式事件回调）；M5 docs 注记现状 | fixed（ISSUE-085 登记，留 M6） |
| 3 | 建议 | `crates/zgraph-cli/src/run_cmd.rs:178-256`（`parse` 79 行）、`:684-753`（`run_in_runtime` 70 行）；architecture §2.2 `architecture-v2.md:83`；ISSUE-081 同类 | 新增 2 个生产函数 >60 行，与 architecture §2.2「crates/** 等价约束（函数 ≤ 60 行）」不符（CI 不强制，ISSUE-081 只登记了 harness 的 5 个） | 拆分（`parse` 按选项分组；`run_in_runtime` 按启动阶段抽 helper）或把 CLI 2 处并入 ISSUE-081 登记（P3） | open（已并入 ISSUE-081 扩展清单，随 M6） |
| 4 | 建议 | `crates/zgraph-cli/src/run_cmd.rs:25`（模块文档自述 llm_calls 不在本轮）；`crates/zgraph-trace/src/lib.rs:34-35`；DR-002 `:16`（§1 row3 要求 `recorded_outputs`/`llm_calls`/runs 的 seed/record_mode/**版本列**） | ISSUE-082 仅部分关闭：`recorded_outputs`+`seed`+`record_mode` 已落库，但 `llm_calls`、`runs.worker_versions_json`/`model_params_json` 仍未接线（DR-002 §6 把 llm_calls 放 wave2 REV-030，但「版本列」未在 wave2 行明示） | 合并后**不要**把 ISSUE-082 标 fixed；wave2（REV-030）补齐 llm_calls + 版本/模型参数列并在 DR-002 §6 明示；台账注记「部分」 | open（ISSUE-082 记为部分修：recorded_outputs 待本分支合并 + llm_calls/版本列 wave2 REV-030） |
| 5 | 建议（跟进） | `crates/zgraph-cli/src/run_cmd.rs:410-470`（`RoutingExecutor`/`RecordingExecutor` 未 override `timeout_hint`） | ISSUE-076 端到端：kernel 已支持 hint（REV-026），但 CLI 组合执行器沿用默认 `timeout_hint() -> None`，kernel 永远收不到 Harness 的 `AgentConfig.node_timeout_ms`；REV-033 只改 harness 仍不生效 | wave2 在 `RoutingExecutor`/`RecordingExecutor` 转发 `timeout_hint`（转发到 Harness；Worker 返回 None），并补「DSL 未声明 + AgentConfig.node_timeout_ms 生效」端到端用例（cli/e2e） | open（DR-002 §6 cli REV-030 已列 `timeout_hint` 转发 + 端到端用例） |
| 6 | 建议 | spec 06 §6.10.1（`06:709-719`）；`crates/zgraph-cli/src/lib.rs:56-61` 等 | 通用约定未完全对齐（**pre-existing**，非本分支新增）：实现为手写参数解析而非 spec 要求的 `clap`；`-v/--verbose` 未实现；`--version` 输出为 `zgraph 0.1.0 (DSL 1.0)` 而非 `zgraph <pkg> (spec v1.2, schema 1)`；颜色/`NO_COLOR` 未处理 | M5 docs 收尾时登记 ISSUE（或说明 M5 接受手写解析、其余留 M6）；不建议在本分支临时改 | open（DR-002 §9 #11 注记，随 M6） |
| 7 | 建议 | spec 06 §6.10.3 用法块（`06:737-745`）vs architecture §5.3（`architecture-v2.md:180`）vs 实现 `run_cmd.rs:66-90` | `--record` 是 architecture v2 增量，spec 06 §6.10.3 用法/参数表尚未包含；M5 回写需把 `--record` 与 `recording` 语义写入 06 §6.10.3（DR-002 §9 已列 06 回写，但未点名该条） | 在 DR-002 §9 #5 或 06 回写清单中点名「06 §6.10.3 增 `--record`/`--fake-llm=` 说明」 | open（M5 docs writeback：06 §6.10.3 增 `--record`） |
| 8 | Nit | `crates/zgraph-cli/tests/run.rs:788-820` | `run_uses_env_defaults_for_db_assets_and_python` 设置 `ZGRAPH_PYTHON` 但未移除 ambient `PYO3_PYTHON`；若开发者 shell 导出无效 PYO3_PYTHON（`scripts/env.sh` 会导出），该用例会因优先级选 PYO3 而失败（非产品缺陷，测试隔离问题） | 该用例用 `run_cli_full(..., removed=["PYO3_PYTHON"])` 或显式设置 PYO3 为同一解释器 | open（建议：测试隔离） |
| 9 | Nit | `crates/zgraph-cli/src/run_cmd.rs:907-910`（`:memory:` 的 `event_count`） | `--db :memory:` 时 `event_count` 恒 0（因不打开 reader），spec 未定义该口径；实现对使用者是「静默降级」 | 已在注释/DR-002 §2 说明；建议 06 §6.9.1/§6.10.6 回写时注明「`:memory:` 不提供 event_count」 | open（docs 回写：`:memory:` event_count 口径） |
| 10 | 备注（跨模块） | 根 `Cargo.lock`（`process` 模块） | 本分支机械修改根 `Cargo.lock`（仅 `zgraph-cli` dependencies 列表 +4 行：blake3/chrono/zgraph-harness/zgraph-worker；**无新包、无版本变更**），按 AGENTS §2 属 cli+process 跨模块；提交正文未显式声明 | 本记录已由非作者 reviewer-a 声明并作第二评审；建议 Lead 在合并说明确认（REV-020/022 先例） | closed（本记录已声明 cli+process 跨模块并作第二评审；Lead 合并说明确认） |
| 11 | 备注 | `tests/run.rs`（AC-08 真链路）；DR-002 §1 row1「AC-01 CLI 链路」 | AC-01 的 CLI 链路未在本分支覆盖（`examples/customer_ticket` 资产不存在；DR-002 §6 把「10×同图 + AC-01 CLI 链路」放 wave2 `test/e2e-m5-cli-replay` REV-032）；本分支以 AC-08 图 + io 图覆盖真实链路 | 保持 wave2 计划；M5 docs 回写时注明 AC-01 CLI 链路验收落点 | open（wave2 REV-032：AC-01 CLI 链路） |
| 12 | 备注 | `crates/zgraph-cli/src/lib.rs:101-140`（panic→70） | `catch_unwind` 覆盖主线程/`block_on` 路径的 panic→70（单测覆盖包装逻辑，二进制层无后门是正确取舍）；tokio `spawn` 后台任务的 panic 被 tokio 收敛（不会让进程以 101 退出），不属本守卫范围 | 接受；建议 lib.rs 注释补一句「后台任务 panic 由 tokio 收敛，不改变退出码」 | note（接受；后台任务 panic 由 tokio 收敛） |
| 13 | 备注 | `crates/zgraph-cli/src/run_cmd.rs:926-964`（失败 `--json`） | 失败 JSON 是成功字段的超集（含 `duration_ms`/`event_count`/`db`/`recovered` 等），spec §6.10.6 失败示例字段较少；超集不违反「无字段冲突」契约 | M5 docs 回写时在 06 §6.10.6 统一「成功/失败字段全集」表述 | open（docs 回写：失败 JSON 字段全集） |

> 无阻塞项；#1 已关闭（fixed），#2/#3/#4/#5/#6/#7/#9/#11/#13 已登记台账或列 wave2/docs 跟进，#8/#10/#12 为 Nit/声明；均不阻塞。

## 4. 验证证据

### 4.1 范围、分支与提交元数据（首轮对象证据；最终对象/门禁见 §8）

```bash
cd /Users/zyzs/lczyzs/code/zgraph/.worktrees/cli-cli-dev
git status --short                       # 空（评审时 clean）
git branch --show-current                # feat/cli-m5-run
git merge-base main HEAD                 # 5d2e53d21d8693c738213f0764376f414826c5c3（= main）
git rev-parse HEAD                       # a3b15ef548c347821c4e5c16e8724a76c7c13250
git log --oneline main..HEAD             # a3b15ef / ad5548a
git diff --shortstat main...HEAD         # 8 files changed, 2633 insertions(+), 29 deletions(-)
git diff --name-status main...HEAD
# M Cargo.lock；M crates/zgraph-cli/Cargo.toml；M src/{bridge,lib,main,validate}.rs；A src/run_cmd.rs；A tests/run.rs
git log -2 --format='%H %an <%ae> %s'    # 均为 chenyuxiangg；两 commit 正文含 Review: REV-028 / Agent: cli-dev
git diff main...HEAD -- Cargo.lock        # 仅 zgraph-cli dependencies +blake3 +chrono +zgraph-harness +zgraph-worker（无其它包/版本变化）
```

### 4.2 06 §6.10.3–§6.10.6 逐条核对（含测试位）

| 契约项 | 实现 | 验证 |
|---|---|---|
| 参数全表（graph/`--inputs`/`--python`/`--python-path`/`--user-id`/`--db`/`--assets-dir`/`--tools-dir`/`--fake-llm`/`--real`/`--stream`/`--json`） | `run_cmd.rs:178-256`（parse）、`RUN_USAGE` `:66-90` | 单测 `parse_applies_defaults`/`parse_accepts_all_options`/`parse_rejects_usage_errors`；`--help` 集成测试 |
| 默认值 | `--user-id local`、`--db`→`ZGRAPH_DB`→`.zgraph/trace.db`、`--assets-dir .`/`ZGRAPH_ASSETS`、`--tools-dir <assets>/tools`/`ZGRAPH_TOOLS`、`--python`→`PYO3_PYTHON`→`ZGRAPH_PYTHON`→`python3`（`:564-570`） | 单测 + `run_uses_env_defaults_for_db_assets_and_python` + 优先级两条集成测试 |
| 输入形状/错误 | 嵌套总允许、单 entry 简写、非 entry/未声明端口 `RUN_ENTRY_INVALID`、required 缺失 `RUN_ENTRY_INPUT_MISSING`（`:294-399`） | `inputs_*` 单测 + `run_rejects_invalid_entry_inputs_without_run`（5 反例，exit 3、stdout 空、`runs=0`） |
| 启动顺序 | 打开 DB/迁移 → 图加载/编译（失败 2、不建 run）→ 入口校验（3、不建 run）→ provider/executor → `start_traced_run_with_writer`（`:684-753`） | 代码核对；`run_dsl_error_exits_2_without_run`/`run_storage_error_exits_4`；**缺 §6.8.1 中止标定（C1 #1）** |
| 运行中输出 | 生命周期在结束后回读（非实时）、`--stream` 实时 token 文本到 stderr | `run_human_mode...`、`run_ac08_real_chain_streams_and_records_hash`（stderr 含 token 文本）；#2 |
| 结束输出 | human stdout `run_output` JSON；`--json` §6.10.6 | `run_json_success_io_graph`（逐字段）+ `run_human_mode...` |
| SIGINT | cancel → `RunFinished{Cancelled}` → 130 | `run_sigint_cancels_and_exits_130`（真二进制 + `kill -INT`，断言 exit 130/DB Cancelled/RunFinished 恰一次/stdout null） |
| 退出码 0/1/2/3/4/70/130 | `exit_code_for`+错误映射+`run_with` | 集成测试覆盖 1/2/3/4/130；70 由 `lib.rs` 单测覆盖包装层（无生产后门，`panic_is_caught_and_mapped_to_exit_70`） |
| `recorded_outputs`（架构 §5.2/§5.3） | `record_output`：成功才发；`(run_id, instance_id, attempt)`；hash=blake3 canonical 64 hex；full 带 output_json；none 不发 | 真链路测试：hash 模式 1 行 `output_json=None`+hash 可复算；full 模式记录 JSON==实例 output、attempt=0；none 无行；`runs.record_mode`=hash/full/none |
| `:memory:`（06 §6.9.1） | 接受 + stderr warning；outcome-only 输出；`event_count=0` | `run_memory_db_outputs_outcome_and_warns`、`trace_memory_db_exits_4_with_storage_error` |
| PYO3 优先级（04 §4.3.6/06 §6.10.3/08 N-02） | `--python` > `PYO3_PYTHON` > `ZGRAPH_PYTHON` > `python3`（`:564-570`） | `python_flag_overrides_both_env_vars`、`pyo3_python_env_precedes_zgraph_python_env_then_python3`（含 PYO3 非法→NODE_WORKER_CRASH 证据） |

### 4.3 真链路真实性（无 mock / 无网络）

- 集成测试通过 `env!("CARGO_BIN_EXE_zgraph")` 运行**真实二进制**（`tests/run.rs:56-76`），不直接调库绕过 dispatch/输出层。
- AC-08 链路（`run_ac08_real_chain_streams_and_records_hash` 等）：真 `WorkerExecutor` spawn `python -m zgraph_worker.runtime`（`--python-path <repo>/python` 注入 SDK）、真 `HarnessExecutor` + `FakeLlmProvider`（按 spec 的 fake provider，非 mock）、工具 `add` 经真 worker 子进程、真 SQLite 写读（测试用 `rusqlite` 直接查 `runs/instances/events/recorded_outputs`）。
- 全程无网络：fake LLM 脚本 / 内置脚本；唯一 `--real` 用例（`run_real_flag_smoke_without_agent_nodes`）图无 agent 节点，仅构造 `GenaiLlmProvider`（`Client::new()`，无请求）。
- 每个用例独立 `tempfile::TempDir` + 独立 DB；真 worker 用 `--python-path` 注入 SDK；未发现 mock/假断言/为测试而测试。

### 4.4 必跑命令与结果（隔离 target；先 `cargo clean -p` 全部 workspace 成员）

| 命令 | 结果 |
|---|---|
| `cargo test -p zgraph-cli` | **87 passed / 0 failed / 0 ignored**（lib 38 + acceptance_m2 10 + run 22 + trace 9 + validate 8；`/tmp/rev028-cli-test.log`） |
| `cargo clippy --workspace --all-targets -- -D warnings` | exit 0 |
| `cargo fmt --all -- --check` | PASS |
| `bash scripts/dev/check.sh` | CHECK PASSED（身份/冻结文件/禁用文件/progress ≤30/fmt/diff-check） |
| `cargo test --workspace -- --test-threads=2`（独立复跑） | **461 passed / 0 failed / 2 ignored**（2 ignored 为既有 harness 真模型 smoke + e2e AC-08b 手动，与基线一致；`/tmp/rev028-workspace.log`）；pytest 由 merge 门禁在 C1 关闭后复跑 |

### 4.5 merge.sh dry-run（首轮：按设计阻断；最终复跑结果见 §8）

```bash
cd /Users/zyzs/lczyzs/code/zgraph
CARGO_HOME=/Users/zyzs/lczyzs/code/zgraph/.cargo-home CARGO_NET_OFFLINE=true \
CARGO_TARGET_DIR=/tmp/zgraph-merge-target CARGO_INCREMENTAL=0 \
PYTHON_BIN=/Users/zyzs/lczyzs/code/zgraph/.venv/bin/python \
bash scripts/dev/merge.sh feat/cli-m5-run --dry-run
# 首轮（本记录结论=有条件通过）：MERGE BLOCKED: 评审结论不是“通过”: .../REV-028-cli-m5-run.md
# → 记录已被脚本正确识别，仅 C1 未关闭导致门禁阻断（符合设计）；C1 关闭并 rebase 到最新 main 后复跑全门禁
```

## 5. 遗留问题

- **合并前置（已完成）**：分支已 rebase 到 `b27d9c0`（含 REV-038/ISSUE-084 登记），base = main ✓；C1 关闭后 `merge.sh --dry-run`（隔离 target `/tmp/zgraph-merge-target2`，先 `cargo clean -p` 工作区成员）结果见 §8。
- **C1 关闭（已完成）**：main `b27d9c0` 含 ISSUE-084（P2 trace/cli；wave2 trace API REV-034 + CLI 调用 REV-030；kill -9 测试；07 §7.3 #6；不列 M6 排除、滑期转 M6 保留 spec 注记）与 DR-002 §4/§6/§9；REV-038 记录已合并。据此判定 C1 关闭（§6/§8）。
- **wave2/后续跟进（已登记）**：
  - #4 ISSUE-082 部分修：本分支 `recorded_outputs`（full/hash/none + hash 复算）随本次合并生效；`llm_calls`、`worker_versions_json`/`model_params_json` 在 wave2 REV-030。
  - #5 ISSUE-076 端到端：`timeout_hint` 转发已列入 DR-002 §6 cli REV-030（+ harness REV-033），需补端到端用例。
  - #2 ISSUE-085（human 生命周期实时性）留 M6；#3 ISSUE-081 扩展清单（含本分支 `parse`/`run_in_runtime`/74 行测试）随 M6。
  - #6 CLI pre-existing 偏差（clap/`-v`/`--version`/`NO_COLOR`）按 DR-002 §9 #11 注记随 M6；#7 `--record` 写入 06 §6.10.3、#9 `:memory:` event_count、#13 失败 JSON 字段全集随 M5 docs writeback（REV-035）。
  - #11 AC-01 CLI 链路由 wave2 `test/e2e-m5-cli-replay`（REV-032）覆盖。
- **合并后**：Lead 更新 `doc/reviews/README.md`/`doc/index.md` 的 REV-028 索引与 `doc/progress.md`。
- 本评审未修改产品代码；首轮新增本记录，增量复审仅追加 §8 并更新 §1/§2/§3/§6/§7 状态（未删除原记录）。

## 6. 条件关闭记录

| 条件 | 关闭证据 | 关闭日期 |
|---|---|---|
| C1（06 §6.8.1/§6.10.3 崩溃标定接线未实现） | main `b27d9c0`（REV-038）：`doc/issues.md` 新增 **ISSUE-084**（P2，trace/cli；wave2 `feat/trace-m5-recovery` REV-034 提供 `interrupted_candidates`/`mark_interrupted`/`RunRow.owner_pid`，CLI REV-030 启动调用并输出 `recovered[]`/stderr；kill -9 故障注入测试；07 §7.3 #6；**不列入 M6 排除**、滑期转 M6 保留 spec 注记）；DR-002 §4 增 ISSUE-084 口径、§6 wave2 表 cli/trace 行更新、§9 #10 回写项。本评审已核对 main 内容并判定关闭（§8）。 | 2026-10-30 |

## 7. 评审签署

- 首轮：范围 cli + 根 `Cargo.lock`（跨模块已声明）；`cargo test -p zgraph-cli` 87 passed、clippy/fmt/check.sh PASS；06 §6.10.3–§6.10.6 逐条核对一致；真链路测试真实；`Cargo.lock` 机械 diff。
- 增量复审（§8）：C1 关闭材料已在 main（ISSUE-084 + DR-002 §4/§6/§9）；代码内容与首轮一致（rebase 无代码变化）；最终 `merge.sh --dry-run`（隔离 target2）通过。
- 首轮唯一重要问题（C1）已关闭；其余建议/备注均已登记台账或列 wave2/M6/docs 跟进，不阻塞。
- **最终结论：通过**。本分支允许合并（`merge.sh feat/cli-m5-run`）。
- 评审人：`reviewer-a`（非作者）；首轮 2026-10-30；增量复审 2026-10-30。

## 8. C1 关闭与增量复审（最终通过，2026-10-30）

### 8.1 对象与 rebase 核对

- 首轮对象：`5d2e53d..a3b15ef`（结论=有条件通过，C1）。
- 最终对象：`b27d9c0..9b58be4`（3 commit：`a23538c` feat run+record、`23c805c` fix PYO3/70/:memory:、`9b58be4` 首轮本记录）；base = `b27d9c0` = main ✓。
- rebase 内容核对：`git diff a3b15ef HEAD -- crates/zgraph-cli Cargo.lock Cargo.toml` = **空**（代码零变化；仅 base/docs 与评审记录提交变化），故首轮对代码的全部验证结论（§4.2–§4.4）继续有效。

### 8.2 C1 关闭材料（均在 main `b27d9c0`）

| 关闭条件（首轮） | 关闭证据 | 结论 |
|---|---|---|
| C1：06 §6.8.1/§6.10.3 崩溃标定未实现，须登记 ISSUE + 决策文档注记 | ① `doc/issues.md` 新增 **ISSUE-084**（P2，trace/cli）：§6.8.1 中止标定 / §6.8.3 interrupted 提示 / `--json.recovered[]`；wave2 落点 `feat/trace-m5-recovery`(REV-034) + CLI(REV-030)；含 kill -9 测试与 07 §7.3 #6；**不列入 M6 排除**、滑期转 M6 并保留 spec 注记。② `DR-002` §4 增 ISSUE-084 口径、§6 wave2 表 cli 行补「启动中止标定/`recovered[]` + `timeout_hint` 转发」、trace 行改 `feat/trace-m5-recovery` REV-034、§9 增 #10 回写项。③ REV-038 评审记录已合并（main `b27d9c0`） | **C1 关闭** |

### 8.3 首轮建议的处置结论

| 首轮发现 | 处置 |
|---|---|
| #2 human 生命周期非实时 | 登记 **ISSUE-085**（P3，cli），留 M6；DR-002 §4 注记 |
| #3 函数 >60 行（`parse`/`run_in_runtime`） | 并入 **ISSUE-081** 扩展清单（P3，随 M6；含本分支 2 处 + 74 行测试） |
| #4 ISSUE-082 未闭合 | 台账记为**部分修**：本分支 `recorded_outputs` 待合并生效；`llm_calls`、`worker_versions_json`/`model_params_json` wave2 REV-030（勿提前标 fixed） |
| #5 `timeout_hint` 转发 | DR-002 §6 cli REV-030 已列「`timeout_hint` 转发」+ 端到端用例 |
| #6 CLI pre-existing 偏差 | DR-002 §9 #11 注记（clap/`-v`/`--version`/`NO_COLOR`）随 M6 |
| #10 Cargo.lock 跨模块 | 本记录已声明（cli+process）并作第二评审；Lead 合并说明确认 |
| #1 C1 | 关闭（ISSUE-084 + DR-002 §4/§6/§9；见 8.2） |

### 8.4 最终 merge.sh dry-run（隔离 target2）

```bash
cd /Users/zyzs/lczyzs/code/zgraph
CARGO_HOME=/Users/zyzs/lczyzs/code/zgraph/.cargo-home CARGO_NET_OFFLINE=true \
CARGO_TARGET_DIR=/tmp/zgraph-merge-target2 CARGO_INCREMENTAL=0 \
PYTHON_BIN=/Users/zyzs/lczyzs/code/zgraph/.venv/bin/python \
bash scripts/dev/merge.sh feat/cli-m5-run --dry-run
```

（结果待填）
