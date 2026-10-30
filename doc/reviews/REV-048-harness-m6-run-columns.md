# REV-048 · harness/worker/cli/trace · ISSUE-082 runs 版本与模型参数列接线

- **分支**：`feat/harness-m6-run-columns`
- **作者**：`harness-dev`（提交 `ee7bdfe`）
- **评审人**：`lead`（**≠ 作者**；主评审）＋ `reviewer-a`（**跨模块第二评审**，AGENTS §2 要求）
- **变更范围**：`main(07bb500)..ee7bdfe`，13 文件 +658/−31：`crates/zgraph-worker/**`（hello 观测/聚合）、`crates/zgraph-harness/**`（model 摘要/聚合）、`crates/zgraph-trace/**`（`update_run_columns` 写端原语 + RunRow 字段 + 新测试）、`crates/zgraph-cli/**`（组合根 + run 结束写两列 + 测试）；**worker 协议未改**；worktree clean
- **评审时间**：2026-10-31
- **关联**：ISSUE-082；architecture §5.2；`06` §6.2.4/§6.2.13/§6.9.1/§6.10.3；DR-003 §3 D3；REV-030（recorded_outputs 类比）

> **跨模块声明**：harness（主）+ worker + cli + trace。Lead 已批准；第二评审人 = `reviewer-a`（第二评审小节见文末，合入前追加）。

## 1. 结论

**结论：通过**（主评审；第二评审小节由 reviewer-a 追加后生效）

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

> 待 reviewer-a 追加：确认 4 模块接口只增不破、写时点/FIFO barrier、失败 run 与 `:memory:` 口径、去重/canonical 形状、无密钥、与 trace v3/REV-049 的兼容。
