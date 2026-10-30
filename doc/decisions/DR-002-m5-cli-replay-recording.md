# DR-002 · M5 收尾范围与 CLI / replay / L2 记录决策

> **状态**：生效（M5 开发期决策记录；实现结论由 M5 评审记录与 spec 回写固化）
> **日期**：2026-10-30 · **作者**：lead · **关联**：`doc/architecture-v2.md` §5/§6.1（M5 DoD）、`doc/spec-v1.2/06` §6.10、`doc/spec-v1.2/07` AC-12、`doc/issues.md` ISSUE-033/036/048/059/061/066/073/074/075/076/077/079/080/081/082（详见 §4/§7）
> **评审**：REV-025（process/docs，reviewer-a 首轮「有条件通过」，C1/C2 关闭后增量复审）；实现分支评审 REV-026–REV-033。
> **模块声明（跨模块）**：本分支改 `doc/**`（docs 模块：DR-002、progress、index）与根 `README.md`（process 模块，Lead 为 process 负责人）；按 AGENTS §2 属跨模块改动，已由**非作者** reviewer-a 在 REV-025 作独立第二评审并在记录中声明。

本文件冻结 M5 的实现口径，供各模块开发/评审引用；与 spec 正文冲突时以 **spec 正文**为准，本文件记录"spec 未写死"的部分与 M5 的落地选择。**已知偏差与回写清单见 §9**——M5 收尾时由 docs 分支把结论回写 spec 00/04/05/06/07/08 与 architecture §5.3/§8.2，并在 `PROVENANCE.md` 登记。

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
- **`--python`**：解析顺序 `--python` > env `PYO3_PYTHON` > env `ZGRAPH_PYTHON`（e2e/测试覆盖用，兼容 `tests/e2e/common`）> `python3`。spec 04 §4.3.6 / 06 §6.10.3 / 08 N-02 的权威变量是 `PYO3_PYTHON`；`ZGRAPH_PYTHON` 仅作测试覆盖，回写见 §9 #1。
- **`--fake-llm` 裸 flag**：使用 CLI **内置默认脚本**（05 §5.3.4 语义：一段文本 + 一次 tool_calls；`calls` 用尽后按 spec 重复最后一项）。需要确定性内容的验收（AC-01/AC-08）必须用 `--fake-llm=<script>`；脚本落 `tests/fixtures/graphs/**`（architecture §2.2 权威；仓库无 `examples/`）。spec 07 的 `examples/**`/裸 flag 口径回写见 §9 #2。
- **`--real`**：`GenaiLlmProvider`；密钥仍由 AgentConfig `model.api_key_env` 读取；与 `--fake-llm` 互斥（互斥 = 用法错误退出码 2）。
- **`--record`**：默认 `hash`；`RunSpec.options.record_mode` 透传内核，并原样写 `runs.record_mode`。
- **退出码**：0 Completed / 1 Failed / 2 DSL+用法 / 3 入口输入 / 4 存储 / 130 SIGINT / **70 内部错误**。70 由 CLI 顶层 `catch_unwind` 把 panic 归一为退出码 70（06 §6.10.5；**不得**依赖 Rust 默认 101）。replay 退出码见 §3.3；`REPLAY_*` 属 CLI 命名空间（不写 `runs.error_code`）。
- **输出**：human 模式生命周期 → stderr、最终结果 → stdout；`--json` 严格 06 §6.10.6 结构。`--stream` 只影响 stderr，不改 trace。
- **默认值**：`--db .zgraph/trace.db`（`ZGRAPH_DB` 覆盖）、`--user-id local`、`--assets-dir .`（`ZGRAPH_ASSETS`）、`--tools-dir <assets>/tools`（`ZGRAPH_TOOLS`）。
- **`--db :memory:`**：按 06 §6.9.1「接受并警告」；跨进程读不到（CLI 用 outcome 直接输出 `--json`/human 结果，trace 查询对 :memory: 报存储错误）。
- **SIGINT**：Ctrl-C → 取消 run → `RunFinished{Cancelled}` → 退出码 130（M5 已实现自动化测试）。

## 3. L1/L2 与记录（architecture §5.1/§5.2）

### 3.1 `recorded_outputs`（运行期）

- **写者**：CLI 组合执行器的外层包装（**不写 harness/worker/kernel**）——每次节点执行成功后发 `TraceInput::RecordedOutput`；agent_call 节点同样记录（整包四字段 output）。集成点：`bridge::start_traced_run_with_writer`（REV-028 新增，CLI 先开 `TraceWriter`、拿到 `TraceHandle` 克隆再启动 run）——使用既有 `TraceHandle::send`，不新增写端 API。
- **键**：`(run_id, instance_id, attempt)`；`--record=none` 不写、`hash` 只写 `output_hash`（`output_json=NULL`）、`full` 写全文。
- **hash**：`blake3(zgraph_dsl::canonical_json(output).as_bytes())` 全 64 hex（与 DSL graph_hash 同一 canonical JSON/crate，便于审计；不取截断）。
- **`recorded_at`**：RFC3339 毫秒 `Z`（与内核 `now_rfc3339` 同精度）。

### 3.2 `llm_calls`（运行期，ISSUE-082）

- **写者**：harness 提供观察者 seam `LlmCallRecorder`（`HarnessExecutor::with_llm_recorder`，不破坏 `HarnessSettings` 字面量面）；CLI 实现该 trait 并写入 `TraceInput::LlmCall`。
- **粒度**：每次 LLM **迭代轮次**结束记录一次（`call_index` 0 起、等于该轮下标）；重试合并为最终结果（响应里 `attempts` 体现）。
- **脱敏**：request 快照禁止包含 `api_key`/Authorization 明文（`SecretString` 不入 JSON）；messages/tools/模型参数全量记录。
- **status**：成功 `ok`；终态失败 `error`（response 放 `{"error":{kind,message,phase}}`）。记录失败不得改变执行语义。
- **写入范围**：`--record=full` 时写；`hash`/`none` 不写（architecture §5.2 L2 语义）。

### 3.3 `zgraph replay`

```
zgraph replay --run <run_id> [--mode=events|outputs] [--db <path>] [--json]
```

- `--mode=events`（默认，L1）：复用 `zgraph trace` 现有只读回放实现与输出格式；不重跑、不写库。
- `--mode=outputs`（L2）：**仅支持原始 run `status=Completed`**（Failed/Cancelled/interrupted 不支持重放，见下）。
  1. 只读打开 DB；run 不存在 → exit 4。
  2. 前置校验（任一不满足 → CLI 命名空间错误，exit 3，不写 `runs.error_code`）：
     - `runs.record_mode != full` → `REPLAY_RECORD_MISSING`；
     - `runs.status != Completed` → `REPLAY_RUN_NOT_COMPLETED`；
     - **完整性预检**：原始 run 中每个已执行且**非内置**节点实例（`instances.node_type ∈ {python_node, agent_call}` 且 `status=Completed`）必须存在 `(node_id, attempt)` 对应的 `recorded_outputs` 行，否则 `REPLAY_RECORD_MISSING`；
     - **完整性校验**：`recorded_outputs.output_json` 重新计算 hash 必须等于 `output_hash`，否则 `REPLAY_RECORD_CORRUPT`。
  3. 重跑：读原始图定义（`runs.graph_hash` JOIN `graphs.dsl_json`，runs 表本身无 dsl_json 列）+ `RunStarted.entry_inputs` + `seed`，用 **RecordedExecutor** 重跑内核：按 `(node_id, attempt)` 返回 `recorded_outputs.output_json`，**不启动 worker/LLM**（agent_call 也直接返回记录输出；`llm_calls` 仅作审计，不参与重放）；trace 使用内存 sink（不落库）。
     - **映射依据（唯一性假设）**：重跑时内核重新生成 `instance_id`（UUIDv7，不可复用），故以 `(node_id, attempt)` 建索引。内核语义保证其唯一：非自环节点每 run 至多执行一次（attempt 0）；自环每次投递新建 `attempt+1`；投递到终态实例即 fail run（`RUN_DELIVERY_TO_TERMINAL`）；`max_attempts` 由 gate/投递规则约束（ISSUE-061 修复后，被 gate 拦截的投递不创建实例）。start/end 为内核内置执行、不进入执行器，故不查记录。
  4. 终态与输出比较：重跑必须 `Completed`、`error_code=None`，且重跑输出 == 原始 `runs.output_json`；全部相等 → exit 0；否则 → `REPLAY_OUTPUT_MISMATCH` exit 1（给出差异摘要）。
  5. **不写新 run、不修改原 trace**。
- replay 退出码小表：0 成功；1 `REPLAY_OUTPUT_MISMATCH`（或 events 模式下 run Failed/interrupted，沿用 trace 语义）；2 用法；3 `REPLAY_RECORD_MISSING` / `REPLAY_RUN_NOT_COMPLETED` / `REPLAY_RECORD_CORRUPT`；4 存储/run 不存在。
- 明确不承诺 L3（外部副作用）；`--dry-run` 不实现（architecture §5.3）。

## 4. P2 修复口径

- **ISSUE-061（gate 幻影 attempt）**：`apply_publish` 改为**先判定 gate 再解析投递目标**；被拦截投递只结算边 + 级联 Skipped，不创建 attempt 实例、不因"投递到终态"而 fail。e2e 既有反例不得弱化；**e2e 侧「无幻影 attempt」回归断言由 e2e-dev 在 wave2 分支补**（`tests/e2e/parallel.rs` 属 e2e 模块，kernel 分支不跨改）。
- **ISSUE-076（AgentConfig.node_timeout_ms 回退）**：内核 `CompiledNode` 记录 `declared_timeout_ms`；`NodeExecutor` 增加对象安全的默认方法 `timeout_hint(&CompiledNode) -> Option<u64>`；派发超时顺序 = test-hooks force > DSL declared > executor hint > `DEFAULT_NODE_TIMEOUT_MS`。
  - **Harness 侧可落地路径**（回应 REV-025 #5）：`load_agent` 需要异步 `discover` 的 `ToolRegistry`，不能在同步 `timeout_hint` 里直接调用。实现为 **config.rs 新增同步轻量函数**：只读 `<assets_dir>/agents/*.json`、按 `id` 命中、解析并校验 `node_timeout_ms` 范围（不查工具表/密钥/prompt 文件；失败/缺失返回 `None` 交给内核默认/后续 Loading 报 `HARNESS_CONFIG_LOAD`）。`run_agent` 的 elapsed 上报复用同一解析函数得到同一生效值（declared > AgentConfig > default）。
  - **跨模块协同（Lead 批准）**：kernel 先合、harness 后接，各自独立评审。
- **ISSUE-074（截断）**：工具结果超 8 KiB 截断后仍须是**合法 JSON**（字符串值在 JSON 字符串内部截断 + `…[truncated N bytes]`；对象/数组给出合法降级）；traceback 上限 8 KiB；summary 字面 ≤512/≤256。属 M5 契约修复。
- **ISSUE-066（`NODE_WORKER_CRASH` 记录 signal/exit）**：M5 wave2 落地。`NodeError::NodeWorker` 载荷**只增不删**（新增可选 `signal`/`exit_code`/`stderr_tail`，或伴随 detail），三模块同分支顺序 = kernel（变体/错误码/序列化）→ worker（填充 supervisor 证据）→ cli（`node_error_json` 适配）；Lead 批准跨模块，评审记录须声明第二评审。
- **ISSUE-048（`RunRow.owner_pid`）**：M5 wave2 由 trace 补列读取 + `zgraph trace` 的 "run still in progress (pid N)" 提示；不阻塞 DoD。
- **ISSUE-084（崩溃标定/中断恢复，REV-028 C1）**：06 §6.8.1 启动顺序的「中止标定」、§6.8.3 interrupted 截断提示与 `--json.recovered[]` 由 wave2 落地——trace 提供标定 API（扫描 `Running/Paused` 且 owner pid 已死 → `status=Failed/interrupted=1/recovered_at`，事务化），CLI `run` 启动时调用并输出 `recovered[]`/stderr；补 kill -9 故障注入测试（07 §7.3 检查单 #6）。**不列入 M6 排除**；若 wave2 未能完成则转 M6 并保留 spec 注记（台账 ISSUE-084）。
- **ISSUE-085（human 生命周期实时性）**：06 §6.10.3 字面要求运行中逐行输出；M5 实现为结束回读（`--stream` 覆盖 token）。本轮登记台账、留 M6，不阻塞 DoD（docs 回写注明 M5 口径）。
- **ISSUE-075/077/079/080**：按容量在 M5 收敛（各自单模块：harness 的 preview/canonicalize/信号量；docs 的 reason 闭集）；未完成则登记 M6 并保留 spec 注记，**不得**为凑 M5 降低断言。

## 5. AC-12 口径（07 AC-12 + D4/D8）

- 规模：**10 个并发 run**（同图不同 `run_id`）；DB 形态至少覆盖一次"同库多 run"，其余可用独立 DB（AC 允许二者）。
- **M5 实测组合（REV-029）**：8 个同图纯 python run（三并发分支）+ 2 个含 `agent_call` 的 run（fake LLM + 真 worker 工具）；10 个 run 写同一 SQLite（同库不同 run_id，AC 允许的更强形态），独立 DB 形态由 M3 `concurrency.rs` 覆盖。该组合偏离 07「10 个 AC-01 run」的字面，核心语义（并发、隔离、上限、真链路）已覆盖；wave2 `test/e2e-m5-cli-replay` 补 10×同图 + AC-01 CLI 链路，07 AC-12 回写实测口径（§9 #7）。
- 断言：全部 Completed；每 run 的 `instances`/`gate_results`/`events` 只属自己；`global_vars`/`user_id` 不串；全局 `MAX_WORKERS` 与每 run `MAX_WORKERS_PER_RUN` 生效（`WorkerExecutor::with_limits` + 真实进程区间峰值）；总耗时/单 run 比记录为基线注释。
- 峰值断言口径：`峰值 <= 全局上限 && 峰值 >= 2`（低核 CI 上真并行至少为 2；实测 8 核 3/3 达到 4 并记录，不写死 `==4` 以免 2 核 CI flake）；每 run 峰值 `<= per_run` 且观测到 `==2`。
- 已知边界：Harness 工具并发使用独立进程级 semaphore（ISSUE-079），不占 `WorkerExecutor` 全局/每 run 上限——AC-12 峰值只覆盖 kernel 节点 worker，测试/回写须如实标注。
- 若同库 10 并发写触发 `SQLITE_BUSY` 且不可在不弱化语义下解决：保留证据、按 AC 允许形态调整组合，并在 `doc/issues.md` 登记真实限制（M6 再解）。

## 6. Wave 计划、评审编号与分支（REV-025 修订：合并后必须删分支/worktree，wave2 一律新分支）

| 波次 | 模块 | 分支 | 作者 | REV |
|---|---|---|---|---|
| 1 | process/docs | `docs/process-m5-kickoff` | lead | REV-025 |
| 1 | kernel | `fix/kernel-m5-p2` | kernel-dev | REV-026 |
| 1 | harness | `feat/harness-m5-record` | harness-dev | REV-027 |
| 1 | cli | `feat/cli-m5-run` | cli-dev | REV-028 |
| 1 | e2e | `test/e2e-m5-ac12` | e2e-dev | REV-029 |
| 2 | cli（replay/L2 + llm_calls 接线 + 启动中止标定/recovered[] + timeout_hint 转发） | `feat/cli-m5-replay` | cli-dev | REV-030 |
| 2 | kernel+worker+cli（ISSUE-066，跨模块） | `fix/worker-m5-crash-detail` | worker-dev | REV-031（双评审人） |
| 2 | e2e（CLI run/L2 链路 + ISSUE-061 回归 + parallel.rs 注释） | `test/e2e-m5-cli-replay` | e2e-dev | REV-032 |
| 2 | harness（ISSUE-076 timeout_hint，新分支，前置 REV-027） | `fix/harness-m5-timeout` | harness-dev | REV-033 |
| 2 | trace（ISSUE-084 中止标定 API + ISSUE-048 owner_pid 提示） | `feat/trace-m5-recovery` | trace-dev | REV-034 |
| 2 | process/docs（ISSUE-084/085 登记 + DR-002 修订 + 进度；REV-028 C1 关闭材料） | `docs/process-m5-issue-reg` | lead | REV-038 |
| 2 | docs（spec 回写 + 索引） | `docs/docs-m5-writeback` | docs-writer | REV-035 |
| 3 | docs/process（收尾：进度/台账/README/评审索引/基线） | `docs/docs-m5-wrapup` | lead | REV-036 |
| 3 | process（ISSUE-068/078 依赖治理 + CI clippy 固化） | `chore/process-m5-deps` | lead | REV-037 |

## 7. 明确排除（M6，M5 docs 回写保留注记）

| 项 | 理由 |
|---|---|
| ISSUE-033 trace 双通道（8192+1024） | 纯拓扑优化，M2 单通道语义等价；M5 无 DoD 依赖，避免在收尾期引入写端风险 |
| ISSUE-036 trace 写失败 → 取消 run 生命周期接线 | 需 kernel+bridge 新增错误通道设计；M5 CLI 已把写端错误映射 exit 4，完整 fail-stop 留 M6 |
| ISSUE-073 genai `preserve_order` 引发 `Value` 变大 | 依赖行为不受控；worker 已装箱缓解；留 M6 评估升级/替代 |
| ISSUE-059 `validate` 旧临时退出码 | 与 M5 DoD 无关；M6 统一 |

## 8. legacy 退役决策

- `legacy/**` **保持冻结只读、不物理删除**（历史审计与语义对照价值）；M5 只更新 `legacy/README.md` 与 `doc/index.md` 标注"已归档、无维护、不得新增代码"。
- 物理删除/移出仓库属发布决策，交由用户（v1.0 前）；M5 不做 git 历史改写。

## 9. spec 偏差与 M5 回写清单（REV-025 C1 关闭项）

> 回写范围：**00 / 04 / 05 / 06 / 07 / 08** + `doc/architecture-v2.md` §5.3/§8.2；由 docs 分支（REV-035）执行并在 `doc/spec-v1.2/PROVENANCE.md` 登记。08 为历史审计矩阵，只加"M5 注记"不重写历史。

| # | 偏差 / 新增 | spec 位置 | M5 回写动作 |
|---|---|---|---|
| 1 | `--python` 默认 env | 04 §4.3.6、06 §6.10.3、08 N-02 | 回写为 `--python` > `PYO3_PYTHON` > `ZGRAPH_PYTHON`（测试覆盖）> `python3`；08 仅注记 |
| 2 | fake 脚本落点 + AC-01/AC-08 命令 | 07 §前言、§7.1、AC-01、AC-08 | `examples/**` → `tests/fixtures/graphs/**`（architecture §2.2 权威）；AC-01/AC-08 命令补 `--fake-llm=<script>` 并注明裸 flag 仅内置默认脚本、确定性需显式脚本 |
| 3 | 退出码 70 | 06 §6.10.5 | spec 已定义「panic 被捕获 → 70」；M5 实现 `catch_unwind` 落地（无需改文档，补实现注记） |
| 4 | `zgraph replay` 命令与 `REPLAY_*` | 00 §0.4 CLI 命名空间、06 §6.10.5、architecture §5.3/§8.2 | 登记 replay 子命令/退出码小表，新增 `REPLAY_OUTPUT_MISMATCH`、`REPLAY_RUN_NOT_COMPLETED`、`REPLAY_RECORD_CORRUPT`（均为 CLI 命名空间，不写 `runs.error_code`） |
| 5 | L2 记录实现口径 | architecture §5.2、06 §6.2 | 注记 `recorded_outputs` 写者/键/hash、`llm_calls` seam 与 status 约定（ok/error）、仅 `--record=full` 写 llm_calls |
| 6 | P2 修复结论 | 05 §5.13.3 L1/L4、06 §6.2/§8.2、07 AC-12 | ISSUE-061/066/074/076 修复后更新已知限制/口径（含 signal/exit 载荷与 timeout_hint 优先级） |
| 7 | AC-12 M5 实测口径 | 07 AC-12 | 10 run/同库/峰值与耗时基线；工具并发未纳入全局上限（ISSUE-079 注记）；SQLITE_BUSY 实测结论 |
| 8 | legacy 归档 | architecture §6.1、doc/index、legacy/README | 记录"冻结只读、不物理删除、物理归档交用户"决策 |
| 9 | M6 排除项 | architecture §6.1、06 §6.4/§6.8 | ISSUE-033/036/073/059 保留 M6 注记（不删除原条目） |
| 10 | 崩溃标定/中断恢复（ISSUE-084） | 06 §6.8.1/§6.8.3、§6.10.3、07 §7.3 #6 | wave2 实现 trace 标定 API + CLI 启动调用 + `recovered[]`；06 注记实现口径（owner pid 存活判定、事务化标定、退出码不受影响） |
| 11 | CLI pre-existing 偏差（REV-028 #5） | 06 §6.10.1 | clap 未采用（沿用 M1/M2 手写解析）、无 `-v`、`--version` 格式与 `NO_COLOR` 行为随 M6；M5 docs 注记现状，不改变 M5 契约 |
| 12 | ISSUE-081 扩展清单 | architecture §2.2 | 注记「M5 已拆 5 个目标函数；剩余既有 >60 行函数与新增 74 行测试随 M6」，避免误认为 architecture §2.2 已全量收敛 |
