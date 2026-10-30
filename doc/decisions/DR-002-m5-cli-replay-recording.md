# DR-002 · M5 收尾范围与 CLI / replay / L2 记录决策

> **状态**：生效（M5 开发期决策记录；实现结论由 M5 评审记录与 spec 回写固化）
> **日期**：2026-10-30 · **作者**：lead · **关联**：`doc/architecture-v2.md` §5/§6.1（M5 DoD）、`doc/spec-v1.2/06` §6.10、`doc/spec-v1.2/07` AC-12、`doc/issues.md` ISSUE-033/036/048/061/066/074/076/082
> **评审**：REV-025（process/docs，reviewer-a）；实现分支评审 REV-026–REV-029。

本文件冻结 M5 的实现口径，供各模块开发/评审引用；与 spec 正文冲突时以 **spec 正文**为准，本文件记录"spec 未写死"的部分与 M5 的落地选择。M5 收尾时由 docs 分支把结论回写 spec 05/06/07/00 并在 `PROVENANCE.md` 登记。

## 1. M5 范围（architecture §6.1 DoD）

| # | 交付 | 验收 |
|---|---|---|
| 1 | CLI `zgraph run` 全契约（含 `--fake-llm`/`--real`/`--record`/`--json`/退出码/SIGINT） | 06 §6.10.3–§6.10.6；AC-01 CLI 链路 |
| 2 | `zgraph replay --mode=events\|outputs`（L1/L2） | L2 重放测试通过（重放输出 == 原始输出） |
| 3 | L2 记录接线：`recorded_outputs` / `llm_calls` / runs 的 seed/record_mode/版本列 | ISSUE-082 关闭；`--record=full` 可重放 |
| 4 | AC-12 多 run 并发隔离（10 run、全局/每 run 上限、run 间隔离） | 07 AC-12；e2e 真链路 |
| 5 | M5 前 P2：ISSUE-061 / 066 / 074 / 076 | 单测 + spec 回写 |
| 6 | legacy 退役决策、M5 docs 回写、进度/台账/基线/README | 文档一致性检查 |

## 2. CLI `zgraph run` 决策（spec 未写死部分）

- **入口输入**：嵌套形状总允许；恰好 1 个 entry 时允许 `{port: value}` 简写；校验失败 `RUN_ENTRY_INVALID` / `RUN_ENTRY_INPUT_MISSING` → **退出码 3、不产生 run**。
- **`--fake-llm` 裸 flag**：使用 CLI **内置默认脚本**（05 §5.3.4 语义：一段文本 + 一次 tool_calls；`calls` 用尽后按 spec 重复最后一项）。需要确定性内容的验收（AC-01/AC-08）必须用 `--fake-llm=<script>`；脚本来自 `tests/fixtures/graphs/**`，不入 `examples/`（仓库无 `examples/`，M3 已裁定 fixture 落点）。
- **`--real`**：`GenaiLlmProvider`；密钥仍由 AgentConfig `model.api_key_env` 读取；与 `--fake-llm` 互斥（互斥 = 用法错误退出码 2）。
- **`--record`**：默认 `hash`；`RunSpec.options.record_mode` 透传内核，并原样写 `runs.record_mode`。
- **退出码**：严格按 06 §6.10.5（0 Completed / 1 Failed / 2 DSL+用法 / 3 入口输入 / 4 存储；70 panic 由进程默认；130 SIGINT）。`REPLAY_RECORD_MISSING` 属 CLI 命名空间（exit 3，不写 `runs.error_code`）。
- **输出**：human 模式生命周期 → stderr、最终结果 → stdout；`--json` 严格 06 §6.10.6 结构。`--stream` 只影响 stderr，不改 trace。
- **默认值**：`--db .zgraph/trace.db`、`--user-id local`、`--assets-dir .`（`ZGRAPH_ASSETS`）、`--tools-dir <assets>/tools`（`ZGRAPH_TOOLS`）、`--python` 取 `ZGRAPH_PYTHON` → `python3`。
- **SIGINT**：Ctrl-C → 取消 run → `RunFinished{Cancelled}` → 退出码 130（best-effort；无法自动化验证的部分在评审记录如实说明，不得伪测）。

## 3. L1/L2 与记录（architecture §5.1/§5.2）

### 3.1 `recorded_outputs`（运行期）

- **写者**：CLI 组合执行器的外层包装（**不写 harness/worker/kernel**）——每次节点执行成功后发 `TraceInput::RecordedOutput`；agent_call 节点同样记录（整包四字段 output）。
- **键**：`(run_id, instance_id, attempt)`；`--record=none` 不写、`hash` 只写 `output_hash`（`output_json=NULL`）、`full` 写全文。
- **hash**：`blake3(zgraph_dsl::canonical_json(output).as_bytes())` 全 64 hex（与 DSL graph_hash 同一 canonical JSON/crate，便于审计；不取截断）。
- **`recorded_at`**：RFC3339 毫秒 `Z`（与内核 `now_rfc3339` 同精度）。

### 3.2 `llm_calls`（运行期，ISSUE-082）

- **写者**：harness 提供观察者 seam `LlmCallRecorder`（`HarnessExecutor::with_llm_recorder`，不破坏 `HarnessSettings` 字面量面）；CLI 实现该 trait 并写入 `TraceInput::LlmCall`。
- **粒度**：每次 LLM **迭代轮次**结束记录一次（`call_index` 0 起、等于该轮下标）；重试合并为最终结果（响应里 `attempts` 体现）。
- **脱敏**：request 快照禁止包含 `api_key`/Authorization 明文（`SecretString` 不入 JSON）；messages/tools/模型参数全量记录。
- **status**：成功 `ok`；终态失败 `error`（response 放 `{"error":{kind,message,phase}}`）。记录失败不得改变执行语义。

### 3.3 `zgraph replay`

```
zgraph replay --run <run_id> [--mode=events|outputs] [--db <path>] [--json]
```

- `--mode=events`（默认，L1）：复用 `zgraph trace` 现有只读回放实现与输出格式；不重跑、不写库。
- `--mode=outputs`（L2）：
  1. 只读打开 DB；run 不存在 → exit 4；`runs.record_mode != full` 或记录不完整 → `REPLAY_RECORD_MISSING` exit 3（不写 `runs.error_code`）；
  2. 读原始 `runs.dsl_json` + `RunStarted.entry_inputs` + `seed`，用 **RecordedExecutor** 重跑内核：按 `(node_id, attempt)` 返回 `recorded_outputs.output_json`，**不启动 worker/LLM**（agent_call 也直接返回记录输出；`llm_calls` 仅作审计，不参与重放）；
  3. 重跑结束后比较重跑输出与原始 `runs.output_json`（`RunFinished.output_summary`）：相等 → exit 0；不等 → exit 1（CLI 命名空间 `REPLAY_OUTPUT_MISMATCH`）并给出差异摘要；
  4. **不写新 run、不修改原 trace**；重放使用内存 trace sink（不落库）。
- 明确不承诺 L3（外部副作用）；`--dry-run` 不实现（architecture §5.3）。

## 4. P2 修复口径

- **ISSUE-061（gate 幻影 attempt）**：`apply_publish` 改为**先判定 gate 再解析投递目标**；被拦截投递只结算边 + 级联 Skipped，不创建 attempt 实例、不因"投递到终态"而 fail。e2e 既有反例不得弱化。
- **ISSUE-076（AgentConfig.node_timeout_ms 回退）**：内核 `CompiledNode` 记录 `declared_timeout_ms`；`NodeExecutor` 增加对象安全的默认方法 `timeout_hint(&CompiledNode) -> Option<u64>`；派发超时顺序 = test-hooks force > DSL declared > executor hint > `DEFAULT_NODE_TIMEOUT_MS`。Harness 对 `agent_call` 返回 `Some(AgentConfig.node_timeout_ms)`，并让 `run_agent` 的 elapsed 上报使用同一生效值。**这两笔改动是 kernel + harness 的跨模块协同，Lead 批准**（kernel 先合、harness 后接，各自有独立评审）。
- **ISSUE-074（截断）**：工具结果超 8 KiB 截断后仍须是**合法 JSON**（字符串值在 JSON 字符串内部截断 + `…[truncated N bytes]`；对象/数组给出合法降级）；traceback 上限 8 KiB；summary 字面 ≤512/≤256。属 M5 契约修复。
- **ISSUE-066（`NODE_WORKER_CRASH` 记录 signal/exit）**：M5 wave2 落地。`NodeError::NodeWorker` 载荷**只增不删**（新增可选 `signal`/`exit_code` 字段或伴随 detail），三模块顺序 = kernel（变体/错误码/序列化）→ worker（填充 supervisor 证据）→ cli（`node_error_json` 适配）；Lead 批准跨模块，评审记录须声明第二评审。
- **ISSUE-048（`RunRow.owner_pid`）**：M5 wave2 由 trace 补列读取 + `zgraph trace` 的 "run still in progress (pid N)" 提示；不阻塞 DoD。
- **ISSUE-075/077/079/080**：按容量在 M5 收敛（各自单模块：harness 的 preview/canonicalize/信号量；docs 的 reason 闭集）；未完成则登记 M6 并保留 spec 注记，**不得**为凑 M5 降低断言。

## 5. AC-12 口径（07 AC-12 + D4/D8）

- 规模：**10 个并发 run**（同图不同 `run_id`）；DB 形态至少覆盖一次"同库多 run"，其余可用独立 DB（AC 允许二者）。
- 断言：全部 Completed；每 run 的 `instances`/`gate_results`/`events` 只属自己；`global_vars`/`user_id` 不串；全局 `MAX_WORKERS` 与每 run `MAX_WORKERS_PER_RUN` 生效（`WorkerExecutor::with_limits` + 真实进程区间峰值）；总耗时/单 run 比记录为基线注释。
- 若同库 10 并发写触发 `SQLITE_BUSY` 且不可在不弱化语义下解决：保留证据、按 AC 允许形态调整组合，并在 `doc/issues.md` 登记真实限制（M6 再解，例如写端重试策略）。

## 6. Wave 计划、评审编号与分支

| 波次 | 模块 | 分支 | 作者 | REV |
|---|---|---|---|---|
| 1 | process/docs | `docs/process-m5-kickoff` | lead | REV-025 |
| 1 | kernel | `fix/kernel-m5-p2` | kernel-dev | REV-026 |
| 1 | harness | `feat/harness-m5-record` | harness-dev | REV-027 |
| 1 | cli | `feat/cli-m5-run` | cli-dev | REV-028 |
| 1 | e2e | `test/e2e-m5-ac12` | e2e-dev | REV-029 |
| 2 | cli（replay/L2 + llm_calls 接线） | `feat/cli-m5-replay` | cli-dev | 待分配 |
| 2 | harness（timeout_hint） | 续 `feat/harness-m5-record` | harness-dev | 续 REV-027 |
| 2 | kernel+worker+cli（ISSUE-066） | `fix/worker-m5-crash-detail` 等 | worker-dev / kernel-dev / cli-dev | 待分配 |
| 2 | e2e（CLI run/L2 链路 + AC-12 注释） | `test/e2e-m5-cli-replay` | e2e-dev | 待分配 |
| 2 | docs（spec 回写 + 收尾） | `docs/docs-m5-writeback` / `docs/docs-m5-wrapup` | docs-writer / lead | 待分配 |

## 7. 明确排除（M6，M5 docs 回写保留注记）

| 项 | 理由 |
|---|---|
| ISSUE-033 trace 双通道（8192+1024） | 纯拓扑优化，M2 单通道语义等价；M5 无 DoD 依赖，避免在收尾期引入写端风险 |
| ISSUE-036 trace 写失败 → 取消 run 生命周期接线 | 需 kernel+bridge 新增错误通道设计；M5 CLI 已在结束/中断路径把写端错误映射 exit 4，完整 fail-stop 留 M6 |
| ISSUE-073 genai `preserve_order` 引发 `Value` 变大 | 依赖行为不受控；worker 已装箱缓解；留 M6 评估升级/替代 |
| ISSUE-059 `validate` 旧临时退出码 | 与 M5 DoD 无关；M6 统一 |

## 8. legacy 退役决策

- `legacy/**` **保持冻结只读、不物理删除**（历史审计与语义对照价值）；M5 只更新 `legacy/README.md` 与 `doc/index.md` 标注"已归档、无维护、不得新增代码"。
- 物理删除/移出仓库属发布决策，交由用户（v1.0 前）；M5 不做 git 历史改写。
