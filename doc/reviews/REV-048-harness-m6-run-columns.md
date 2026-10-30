# REV-048 · harness/worker/cli/trace · ISSUE-082 runs 版本与模型参数列接线

- **分支**：`feat/harness-m6-run-columns`
- **作者**：`harness-dev`（提交 `ee7bdfe`）
- **评审人**：`lead`（**≠ 作者**；主评审）＋ `reviewer-a`（**跨模块第二评审**，AGENTS §2 要求）
- **变更范围**：`main(07bb500)..ee7bdfe`，13 文件 +658/−31：`crates/zgraph-worker/**`（hello 观测/聚合）、`crates/zgraph-harness/**`（model 摘要/聚合）、`crates/zgraph-trace/**`（`update_run_columns` 写端原语 + RunRow 字段 + 新测试）、`crates/zgraph-cli/**`（组合根 + run 结束写两列 + 测试）；**worker 协议未改**；worktree clean
- **评审时间**：2026-10-31
- **关联**：ISSUE-082；architecture §5.2；`06` §6.2.4/§6.2.13/§6.9.1/§6.10.3；DR-003 §3 D3；REV-030（recorded_outputs 类比）

> **跨模块声明**：harness（主）+ worker + cli + trace。Lead 已批准；第二评审人 = `reviewer-a`（第二评审小节见文末，合入前追加）。

## 1. 结论

**结论：有条件通过**（主评审通过；reviewer-a 第二评审提出 **C1**：run 元数据列写失败的退出码语义与代码注释/主评审结论不一致，需按 §6.1 口径统一并补测试后转「通过」）

## 2. 四维评审

| 维度 | 结论 | 依据（文件/行/测试） |
|---|---|---|
| 功能正确性 | ✅ | worker：`Supervisor` 保存握手 `hello`（新增 `hello()`），新增 `run_supervised_observed()` 返回 `(Option<Hello>, Result)`（`run_supervised` 包装、返回语义不变）；`WorkerExecutor` 按 run 聚合 `(python_version,sdk_version)` 去重升序，`worker_versions(run_id)->Option<Vec<Value>>`。harness：`HarnessExecutor` 从 AgentConfig `model` 段生成摘要（provider/name/base_url/temperature/max_tokens/context_window_tokens，**不含密钥**），按 run canonical 去重升序，`model_params(run_id)`；失败 run 保留已加载摘要。trace：`TraceWriter::update_run_columns`（新 `WriterMsg` 控制消息，仅 UPDATE 两列、不碰 status/instances/events、与数据消息共用写线程 FIFO、返回更新行数，0=run 不存在非错误）；`RunRow.worker_versions/model_params` 暴露。cli：`build_executor` 返回执行器/harness/worker 句柄；`TracedRun::wait_with_before_close` 在**关闭写端前**写两列，Completed/Failed 均尽力、无观测写 `[]`、写失败不影响退出码、`:memory:` 不写（文档注明）。新增测试：trace roundtrip/仅两列/幂等/未知 run=0；worker hello 聚合去重排序；harness 摘要无密钥/去重/按 run；cli AC-01 Completed 两列非 NULL 且形状合法 + Failed run 版本非空/模型列 `[]`。 |
| 代码结构合理性 | ✅ | 观测/聚合分层清晰：worker 采集握手、harness 采集模型摘要、trace 提供「仅更新两列」原语、cli 组合根在关闭前统一写；公开 API 只增不破（`WorkerRun`/`run_supervised`/`HarnessSettings`/协议均不变；`RunRow` 新字段对仓外字面量构造为 breaking，v0.1 未冻结且仓内已适配）。 |
| 逻辑性 | ✅ | `run_supervised_observed` 保留原语义、失败也保留 hello；写时点在 run 结束且关闭写端前（保证终态先落、两列随后、FIFO barrier）；`:memory:` 跳过（跨进程不可读，06 §6.9.1）；两列为 JSON 数组文本、无观测 `[]` 与 v3 前 NULL 区分清楚。 |
| 可读性 | ✅ | 提交正文与模块文档写明写入口径/形状/失败 run/`:memory:` 边界；新 API 带文档；测试名表达语义。 |

## 3. 发现清单

| # | 级别 | 位置 | 问题 | 建议 | 状态 |
|---|---|---|---|---|---|
| 1 | 备注（兼容） | `RunRow` | 新增字段对仓外以字面量构造为 breaking | v0.1 未冻结、仓内已适配；REV-057/README 记录供库消费者迁移 | open（转 REV-057） |
| 2 | 备注（口径） | `:memory:` | 不写两列（跨进程不可读）；CLI 文档已注明 | REV-057 在 06 §6.9.1 回写 | open（转 REV-057） |
| 3 | 建议（可选） | `07 AC-01/AC-12` | e2e 未加「两列非 NULL」断言（cli 集成测试已覆盖 AC-01 Completed/Failed） | 可后续在 e2e 补最小断言；不阻塞本任务 | 后置 |

无阻塞 / 重要项。

## 4. 验证证据

```bash
# 评审人在干净 target（/tmp/zgraph-merge-target，先 cargo clean 全部 workspace 成员）上执行
CARGO_HOME=<repo>/.cargo-home CARGO_NET_OFFLINE=true
cargo clean -p zgraph-dsl -p zgraph-kernel -p zgraph-trace -p zgraph-worker -p zgraph-harness -p zgraph-cli -p zgraph-e2e
cargo test -p zgraph-trace -p zgraph-worker -p zgraph-harness -p zgraph-cli --quiet
```

- 结果：四 crate 全部 test target **0 failed**（trace 49 / worker 70 / harness 145+1 ignored / cli 141，作者报告 26 suites exit 0；评审人复跑 exit 0，含新 `tests/run_columns.rs` 与 cli `completed_run_writes_...`/`failed_run_also_writes_...`）。
- 作者另报告：`cargo clippy --workspace --all-targets --locked -- -D warnings` exit 0、`cargo fmt --all`、`check.sh` PASS。

## 5. 遗留问题

- REV-057 回写：06 §6.2.4/§6.2.13（写入口径/形状/`:memory:`）、architecture §5.2、05 §5.13.3 L10、08 §8.8 ISSUE-082 → fixed；`RunRow` breaking 备注。
- 无阻塞项。

## 6. 跨模块第二评审（reviewer-a）

> **身份/范围**：`reviewer-a` 非作者；本分支跨 **harness（主）+ worker + cli + trace**，按 AGENTS §2 由 Lead 批准并指定本人作第二评审。Lead 主评审已给「通过」；本小节独立核对 4 模块接口、聚合/去重、写时点与 FIFO、失败/`:memory:` 口径、与 trace v3 兼容，并**独立实跑**关键测试。未修改任何代码，仅追加本小节（§1 结论按第二评审结果改为「有条件通过」）。
> **第二评审结论：有条件通过**——1 项重要问题 C1（见 §6.1），其余 5 项重点通过；C1 关闭后转通过。

### 6.1 【重要 · C1】run 元数据列写失败的退出码语义与声明不一致（需统一并补测试）

**事实链（代码核对，非推测）**：
1. `trace` 写线程：`update_run_columns` 的 SQL/IO 失败 → `self.record_fatal(err)`（`crates/zgraph-trace/src/writer/thread.rs:353-376`，注释明写「SQL/IO 失败按不可恢复错误处理」）。
2. 该 fatal 使后续 `finish_batch` 立即返回 Err（`thread.rs:286-293`、`:379-382`）；写端 `flush()`/`close()` 因此在 `fatal` 存在时返回 Err。
3. `cli`：`wait_with_before_close` 在回调后调用 `close_writer`（`crates/zgraph-cli/src/bridge.rs:306-334`），`close_writer` 先 `flush_result?`（`:353-368`）→ 返回 `BridgeError::Trace`；`wait_for_run` 再 `map_err(report_bridge_error)`（`run_cmd.rs:1037-1040`）→ 返回 **退出码 4**。
4. 结果：**已完成（Completed）的 run 若两列 UPDATE 失败，CLI 以 exit 4 结束且不调用 `finish()`**（stdout 无 run 结果/`--json` 输出）。这与 `write_run_metadata`（`run_cmd.rs:990-999`，`let _ =` 并注释「失败不影响退出码（尽力写）」）、`wait_with_before_close`（`bridge.rs:310`「回调失败不影响 run 结果」）以及主评审 §2「写失败不影响退出码」的表述**矛盾**；且无失败注入测试，行为未被验证。
5. 另注：本分支重点 ④ 要求「SQL 失败 fail-stop」（当前实现满足），⑤ 要求「写失败不影响退出码」——两条要求本身互斥，必须二选一并在文档/测试中固化。

**关闭条件（二选一，可执行）**：
- **方案 A（保持 fail-stop，推荐若按 06 §6.4.5）**：保留 `record_fatal`，但把 `write_run_metadata`/`wait_with_before_close` 注释与主评审 §2 改为「元数据列 SQL 失败按 06 §6.4.5 fail-stop：退出码可能变为 4、不保证运行结果输出」，并补一条 trace 层失败注入测试（SQLite trigger 使 UPDATE ABORT → `update_run_columns` 返回 Err 且 writer fatal / `flush()` Err）。
- **方案 B（真正「尽力写」）**：`update_run_columns` 失败**不** `record_fatal`（仅返回 Err；可加 SQLITE_BUSY 重试），CLI 退出码/结果输出不受影响；补 CLI 失败注入测试断言「Completed 仍 exit 0 且 stdout 有结果」。
- 无论选哪个，需在 REV-057 的 06 §6.2.4 回写该口径（元数据写失败语义）。

### 6.2 【通过】worker：`hello` 观测与原语义保持、按 run 聚合去重升序

- `Supervisor` 新增私有 `hello: Option<Hello>` + 公共只读 `hello()`（`crates/zgraph-worker/src/supervisor.rs:175-180,233-245`）；`drive` 在握手成功后保存（`:253`），握手失败保持 `None`。
- 新增 `run_supervised_observed`（`supervisor.rs:630-672`）：spawn/握手失败 → `(None, Err)`；**即使后续 run 失败/超时**也返回已握手的 `Some(hello)`；原 `run_supervised` 改为 `.1`（`:627-631`），cancel/reap 分支与返回语义逐行保持。协议未改（`schemas/` 无 diff）。
- `WorkerExecutor` 以 `Mutex<HashMap<run_id, BTreeSet<(python,sdk)>>>` 聚合（`executor.rs:125-210`），`run_supervised_observed` 返回后记录（`:253-262`）→ **失败 run 保留 hello**；`worker_versions(run_id)` 输出 JSON 对象数组、按 `(python,sdk)` 升序、去重，未握手返回 `None`。独立测试 `worker_versions_aggregate_dedup_and_sort_per_run`（`executor.rs:781`）通过。
- 兼容：`WorkerRun`/`WorkerFailure`/`run_supervised`/协议签名不变；`lib.rs` 仅新增导出 `run_supervised_observed`（只增）。备注：`WorkerExecutor` 每 run 的版本表不清理（CLI 一 run 一进程无影响；e2e 共享执行器会累积，属既有 ISSUE-065 ② 类问题）。

### 6.3 【通过】harness：模型参数摘要（无密钥/canonical 去重/失败保留）

- 摘要字段显式枚举 provider/name/base_url/temperature/max_tokens/context_window_tokens，**不含 `api_key_env`**（`crates/zgraph-harness/src/executor.rs:160-178`）；按 run 存入 `BTreeSet<String>`（JSON 文本）实现去重+升序，`model_params(run_id)` 返回对象数组/`None`（`:135-158`）。
- 写时点在 `load_agent` 成功之后、执行之前（`:353-356`）→ **失败 run 保留已加载摘要**（LLM/工具失败不影响已记录值）；同一 run 相同模型去重、不同 temperature/模型并列。测试：`model_params_are_recorded_per_run_without_secrets`（`:630`，断言字段且文本不含 `api_key`/env 名）、`model_params_dedup_within_run`（`:667`）。独立复跑 2 passed。
- 备注：① 摘要 JSON 的键序固定（`json!` 构造序），「canonical」此处指确定性去重而非键排序（与 trace `global_vars` 的递归排序不同）；因所有摘要同序，排序/去重结果确定，可接受。② `base_url` 原样记录；若用户在 URL 内嵌 token 属配置泄露面，建议 REV-057 注明摘要不脱敏 URL。③ 与 D3 措辞「CLI 从 AgentConfig 摘要」略有出入——实际由 Harness（唯一加载 AgentConfig 的一侧）记录、CLI 查询写出，建议 REV-057 按实况回写。

### 6.4 【通过】trace：仅更新两列、FIFO barrier、0 行非错误

- `TraceWriter::update_run_columns`（`crates/zgraph-trace/src/writer/mod.rs:163-183`）通过新控制消息 `WriterMsg::UpdateRunColumns(Box<..>)`（`:45,62-69`）进入写线程同一 FIFO；写线程在 `handle_control` 中先 `drain_streaming_all` + `finish_batch`（即先落库已入队的 `RunFinished` 等），再执行 UPDATE（`thread.rs:215-220`）→ **barrier 成立**；SQL 仅 `UPDATE runs SET worker_versions_json=?1, model_params_json=?2 WHERE run_id=?3`，不触碰 status/instances/events（`:353-369`）；`0` 行 = run 不存在，返回 Ok(0) 非错误。
- 测试 `run_columns.rs:14` 验证两列写入/读回、**status/event_count/last_seq/global_vars 不变**；`:56` 验证未知 run=0 及重复覆盖幂等。独立复跑 2 passed。
- **C1 相关**：SQL 失败 `record_fatal` 的副作用见 §6.1（需统一口径）。

### 6.5 【通过】cli：写时点、Completed/Failed、无观测 `[]`、`:memory:`、结果不受影响（除 C1 外）

- `build_executor` 返回 `BuiltExecutors{executor,harness,worker}`（`crates/zgraph-cli/src/run_cmd.rs:928-983`），CLI 保留 Harness/Worker 句柄用于结束查询；`run_metadata` 仅在非 `:memory:` 时构造（`:800-807`，`:memory:` 丢弃 hook `:1021-1025`）。
- 写时点：`wait_for_run` 等 run 终态后进入 `wait_with_before_close`，**在 `close_writer` 之前**执行 hook（`bridge.rs:311-334`）→ `RunFinished` 已落库、两列随后写出（FIFO）。
- Completed/Failed 均写（无论 outcome.status，均跑 hook）；无观测时 `unwrap_or_default()` → 写 `[]`（`run_cmd.rs:990-999`）；`:memory:` 不写（文档 + 代码路径）。
- 测试：`completed_run_writes_worker_versions_and_model_params_columns`（`cli/tests/run.rs:1658`，AC-01 真 worker + 真 trace：非空版本/模型数组、无 `api_key*`、去重）；`failed_run_also_writes_run_columns`（`:1699`，失败 run 版本非空、模型 `[]`）。独立复跑 `cargo test -p zgraph-cli --test run writes` → 3 passed。
- 唯一不满足点 = **C1**（写失败时退出码/输出语义）。

### 6.6 【通过】`RunRow` breaking 备注 + 与 trace v3 兼容

- `RunRow` 新增 `worker_versions`/`model_params`（`reader.rs:135-150`，SELECT 第 26/27 列与 `map_run_row` 索引 25/26 对齐）；对仓外字面量构造是 **breaking**，v0.1 未冻结、仓内构造点全部更新（`replay_cmd.rs:651-654` 等；编译+测试通过）。主评审 #1 的备注合理，且应在 REV-057/README 记录供库消费者迁移。
- v3 兼容：`worker_versions_json`/`model_params_json` 自 v2 DDL 已存在（本分支**未改 schema/迁移**，`schema.rs` 无 diff）；v3 只新增 `global_vars_json`；`update_run_columns` 在 v3 库上工作正常（trace/cli 测试均基于 v3 库），无需新迁移。06 §6.2.4/§6.2.13 的旧「M5 仍未写入」口径由 REV-057 更新。

### 6.7 独立验证（私有 target，未用 /tmp/zgraph-merge-target）

```bash
$ export CARGO_HOME=<repo>/.cargo-home CARGO_NET_OFFLINE=true; unset CARGO_TARGET_DIR
$ cargo test -p zgraph-trace  --locked --test run_columns            # 2 passed / 0 failed
$ cargo test -p zgraph-worker --locked --lib worker_versions         # 1 passed / 0 failed
$ cargo test -p zgraph-harness --locked --lib model_params           # 2 passed / 0 failed
$ cargo test -p zgraph-cli    --locked --test run writes             # 3 passed / 0 failed
$ bash scripts/dev/check.sh                                          # CHECK PASSED
```

- 另：Lead 已复跑 trace/worker/harness/cli 四 crate exit 0，与本轮独立结果一致；本分支基于最新 origin/main `07bb500`、无落后。

### 6.8 建议（非阻塞，除 C1 外）

1. （C2）`worker_versions` 当前仅聚合 `WorkerExecutor` 节点 worker；Harness 工具 worker 走 `run_worker_controlled → run_supervised`（`worker/spawn.rs:131-144`），无 hello 观测——**仅 agent+tool、无 python 节点**的 run 会写 `[]`（且工具 worker 版本不记录）。按本分支重点 ② 的「executor 聚合」范围可接受，但建议 REV-057 在 06 §6.2.4 明确「worker_versions = 节点 worker」或后续补工具 worker 聚合（D3「supervisor 汇总」措辞应对齐）。
2. （测试覆盖）补「失败 run 保留 model_params」（load 成功→LLM 失败）与「RunFinished 之后 update 的 FIFO 次序」两条最小测试；当前 code review 可证但无自动化。
3. （C1 关联）若选方案 B，建议同时给 `update_run_columns` 的 SQLITE_BUSY 加重试（06 §6.4.5 语义），避免瞬态 busy 影响元数据完整性。
4. `base_url` 摘要是否需脱敏 userinfo/query，建议 REV-057 一并定口径。

> 第二评审结论：**有条件通过**。C1（§6.1）关闭后（改注释/文档+补测试，或改为非致命写）即可转「通过」；其余重点均通过。主评审「通过」+ 本小节 → C1 关闭前不满足合并门槛。

