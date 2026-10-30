# 架构蓝图 v2（Rust 内核 + Python 子进程 worker）

> 依据：`doc/decisions/DR-001-runtime-route.md`；语义基准：`doc/spec-v1.2/`（入库快照约 5,400 行，00–08）。
> 本文冻结**进程模型、跨语言协议、可复现落地、代码布局、模块迁移、验收映射、对 spec v1.2 的偏差**。
> 与 spec 冲突时：**本文 §8 的偏差表优先**；偏差表未覆盖的，以 spec 为准。

## 1. 总体拓扑

```text
┌──────────────────────────── zgraph（单个 Rust 二进制） ─────────────────────────────┐
│ CLI / 嵌入 API                                                                      │
│   └─ RunManager（多 run）                                                            │
│        ├─ Run 事件循环（每 run 一个 task，独占 RunState，禁止阻塞）                    │
│        │    ├─ 状态机 / 就绪判定 / Delivery 应用 / 门控求值（Rust 内置 op）             │
│        │    ├─ 超时与取消（deadline + CancellationToken + 进程组 kill）                │
│        │    └─ trace 事件 → 有界通道 → 写线程 → SQLite                                 │
│        ├─ 执行器                                                                      │
│        │    ├─ WorkerExecutor：spawn Python 子进程（NDJSON 协议 v1）                   │
│        │    └─ HarnessExecutor：Rust 内 LLM 调用；tool 经 worker 子进程执行             │
│        └─ GateEvaluator：Rust 内置 op（v1；自定义 Python 门控见 ISSUE-014）             │
└─────────────┬───────────────────────────────────────────────────────────────────────┘
              │ stdin/stdout NDJSON（协议 v1，见 §3）
              ▼
┌──────────────────────── Python worker 子进程（每节点实例一次） ────────────────────────┐
│ python/zgraph_worker                                                               │
│   ├─ 启动：重定向用户 stdout → stderr，协议独占 fd 1；设置 PYTHONHASHSEED=0            │
│   ├─ hello 握手 → 读 run → 调用户函数 run(inputs, ctx) → result/error 后退出           │
│   ├─ ctx.emit(...) → emit 帧（可丢）；ctx.now()/ctx.random() 由 seed 驱动             │
│   └─ 取消：stdin reader 线程置位 → 用户函数自检；超时由内核 SIGKILL 进程组             │
└────────────────────────────────────────────────────────────────────────────────────┘
```

**关键约束**

1. 事件循环线程**禁止**同步子进程等待、同步 SQLite 写、阻塞 IO；执行走 `tokio::process` / `spawn_blocking`。
2. 每个 run 的 `RunState` 由该 run 的事件循环任务独占；执行器只持不可变快照 + `mpsc` 回传。
3. 用户 Python 代码**永远在子进程**；内核不 import Python、不链接 libpython。
4. worker 以进程组（Unix `setsid`）启动；超时/取消对**进程组**发信号，防止孙进程泄漏。

## 2. 代码布局与模块迁移

### 2.1 目标布局（M0 落地）

```text
crates/
  zgraph-dsl/        # DSL 类型、loader、R1–R14、canonical graph_hash
  zgraph-kernel/     # 编译产物、实例状态机、事件循环、Delivery、门控 op、run 生命周期
  zgraph-trace/      # TraceEvent、SQLite schema/写线程、查询/回放
  zgraph-worker/     # 协议编解码、子进程生命周期、超时/取消/回收、stderr 截断
  zgraph-harness/    # AgentConfig、LLM provider、上下文预算、tool loop
  zgraph-cli/        # validate / run / trace / replay / trigger
python/
  zgraph_worker/     # worker SDK：握手、ctx、协议、日志、错误映射、种子
  workers/           # 迁移后的示例 worker
schemas/
  graph.schema.json  # DSL JSON Schema
  worker-protocol-v1.md
tests/
  fixtures/graphs/   # 由 legacy 配置改写的验收图
  e2e/               # Rust 集成 + Python worker 端到端
legacy/              # 现有 Python 实现（只读参考；CI 不跑）
doc/                 # 现有文档 + spec-v1.2 + analysis + 本蓝图
```

### 2.2 AGENTS 模块表迁移（M0 同步）

| 旧模块 id（legacy） | 处置 | 新归属 |
|---|---|---|
| `kernel` | 冻结（M0） | 语义入 `dsl` + `kernel`（新代码） |
| `scheduler` | 冻结 | `kernel` |
| `state-manager` | 冻结 | `kernel` |
| `dispatcher` | 冻结 | 经验入 `worker` |
| `monitor` | 冻结 | `trace` |
| `orchestrator` | 冻结 | `kernel` / `cli` |
| `workers` | 冻结，挑 3–5 个迁移 | `py-worker`（`python/workers/`） |
| `cli` | 冻结 | `cli`（新） |
| `config` | 冻结，3 张图改 fixture | `e2e`（`tests/fixtures/graphs/`） |
| `e2e`（旧 `tests/**`） | 冻结，不再跑 | 新 `e2e` = `tests/**`（新 e2e）；legacy 测试随 `legacy/**` |

新增 id：`dsl`、`kernel`、`trace`、`worker`、`harness`、`cli`、`py-worker`、`e2e`、`docs`、`process`、`legacy`。
**M0 冻结时间点**：M0 合并后旧 id 禁止再建新 worktree（`scripts/dev/worktree.sh` 的 `MODULES` 同步移除旧 id，`AGENTS.md` §2 同步）。`tests/**` 全部属于新 `e2e`；legacy 测试位于 `legacy/**`，由 CI 排除（`pytest testpaths` 只含 `python/` 与新 `tests/`）。

**编码规范适用边界**：`CLAUDE.md`（不可修改）按"Python 代码规范"执行——`python/**`、`legacy/**` 遵守其 Python 规则；`crates/**` 遵守 `rustfmt` + `clippy -D warnings` + 等价约束（全类型标注、snake_case、常量、函数 ≤ 60 行）。登记 ISSUE-019，M0 在 CI 同时执行两套检查。

## 3. 跨语言协议 v1（冻结）

### 3.1 帧与版本

| 项 | 规定 |
|---|---|
| 传输 | worker stdin/stdout，**每行一个 JSON**（NDJSON），UTF-8；禁止多行 JSON |
| 信封 | `{"v":1,"kind":"<kind>","id":"<可选>","payload":{...}}` |
| 上限 | 单帧 ≤ 8 MiB；超限 = 协议错误 |
| 版本 | `v=1`；不兼容变更升 v，并保 1 个版本的兼容窗口 |
| 方向标注 | 下文 `sup→w` = supervisor 写 worker stdin；`w→sup` = worker 写 stdout |

### 3.2 消息表

| kind | 方向 | 载荷 | 语义 |
|---|---|---|---|
| `hello` | w→sup | `{protocol_version:1, python_version, sdk_version, worker_id}` | spawn 后 **5s 内**必须收到第一帧；超时/非法 → 内核杀进程组，错误 `NODE_WORKER_HANDSHAKE` |
| `hello_ack` | sup→w | `{accepted:true}` | 校验通过后发送；worker 收到前不得执行用户代码 |
| `run` | sup→w | `{run_id, instance_id, node_id, attempt, module, function, inputs, params, user_id, global_vars, seed, deadline_ms, trace_id}` | 调一次节点；`inputs` 含全部声明端口（未到 `null`）；`seed` 为 u64（默认 0） |
| `emit` | w→sup | `{type:"token"\|"reasoning"\|"tool_call_start"\|"tool_call_end"\|"progress", ...}` | 流式事件（可丢，内核计数并聚合落库） |
| `log` | w→sup | `{level:"debug"\|"info"\|"warn"\|"error", message}` | 结构化日志（≤64 KiB/帧） |
| `result` | w→sup | `{output:<JSON>}` | 成功；**必须恰好一帧，且随后 1s 内退出** |
| `error` | w→sup | `{code, message, traceback?, details?}` | 业务错误；随后退出 |
| `cancel` | sup→w | `{}` | 协作取消（尽力）；随后按 §3.4 升级信号 |
| `ping`/`pong` | 双向 | `{}` | 可选健康检查 |

### 3.3 退出码封闭表

| 情况 | 判定 |
|---|---|
| exit 0 且恰好 1 个 `result` | Completed |
| exit 0 但无 `result` / 多个 `result` / result 后 1s 未退出 | 协议错误 → Crashed（`NODE_WORKER_PROTOCOL`），**并杀进程组** |
| exit 2 且有结构化 `error` | Crashed（按 `code` 映射 NodeError） |
| exit 3（SDK 自检失败：协议/版本/参数不合法） | Crashed（`NODE_WORKER_PROTOCOL`） |
| 被信号杀死 / 其他 exit code | Crashed（记录 signal/exit code） |
| 超时（deadline） | `TimedOut` + 杀进程组；冷峻 → run Failed |
| 取消 | `Cancelled`；被杀进程组 |

### 3.4 生命周期与资源

1. **隔离**：worker SDK 启动时把 `sys.stdout` 重定向到 stderr，协议写入 `os.dup(1)` 保存的原始 fd；用户 `print()` 只进 stderr。`input()` 被 SDK 禁用（stdin 是协议），调用即抛 `RuntimeError`。
2. **stderr 排空**：supervisor 为每个 worker 起独立 task 持续读 stderr（环形缓冲保留末尾 8 KiB），**禁止不读**（否则 pipe 满导致 worker 阻塞）。
3. **取消**：SDK 起独立 stdin reader 线程，收到 `cancel` 置 `ctx.cancel_requested=True`；用户函数必须在安全点调用 `ctx.check_cancelled()`（抛 `WorkerCancelled`）。supervisor 流程：`cancel` → 宽限 2s → `SIGTERM` → 1s → `SIGKILL` **进程组**。
4. **帧超时**：除 `run` 期间外，单帧空闲超时 `worker_frame_timeout_ms`（默认 300s，可配）；EOF 时若有半行 → 协议错误。
5. **确定性**：supervisor spawn 时设置 `PYTHONHASHSEED=0`（SDK 运行期设置无效）；SDK 按 `run.seed` 设置 `random`/`numpy.random`；`ctx.now()`/`ctx.random()` 由 SDK 提供（直接调系统时间不报错但破坏 L2，文档明示）。

### 3.5 协议 v1 检查单（M0 DoD 必须逐项有测试）

- [ ] `hello` 超时 / `v` 不匹配 / `sdk_version` 缺失 → 拒绝并 kill
- [ ] `hello_ack` 前不执行用户代码
- [ ] `run` 载荷 `seed`/`deadline_ms`/`global_vars` 完整
- [ ] exit 0 无 result、result 后不退出、重复 result → 协议错误
- [ ] stderr 大量输出不阻塞（排空 + 8 KiB 截断）
- [ ] 用户 `print()` 不污染协议；`input()` 抛错
- [ ] 同步用户函数执行中收到 cancel → 标志置位、协作退出
- [ ] 卡帧/半行帧/非法 UTF-8/超 8 MiB → 协议错误
- [ ] 超时 kill 进程组：孙进程不残留、无僵尸
- [ ] 并发上限（全局/每 run）生效

## 4. 调度与执行模型

| 项 | 决策 |
|---|---|
| 运行时 | tokio multi-thread；每 run 一个事件循环任务（单 run 内串行，run 间并行） |
| 就绪与派发 | 按 spec 01/03：`all/any`、Skipped 级联、Delivery 两阶段（门控 off-loop，内置 op 纯函数） |
| 门控 | **v1 = Rust 内置 op**：`check_output` / `compare` / `manual`（含 legacy `gate_value` 语义）；DSL `gate.function` 解析到内置 op 注册表；自定义 Python 门控 = M1 前定稿（ISSUE-014） |
| Python 执行 | 每节点实例一子进程；**无跨调用模块缓存**；`exec_class` 字段保留但 v2 忽略（进程隔离下无需 CPU/IO 分类，池化时再启用） |
| 并发 | 全局 `max_workers`（默认 CPU 核数×2）+ 每 run `max_workers_per_run`（默认 8）；不用 spec 的 CPU=1/IO=4 信号量 |
| 超时 | 实例 deadline（节点配置 > AgentConfig > 默认 180s）→ 杀进程组；**NodeAbandoned 在 v2 不产生**（见 §8 D1） |
| Agent | Harness 在 Rust 进程内（LLM 为 async IO）；**tool 执行 = worker 子进程（每次调用一进程）**；无 import 缓存/共享信号量（见 §8 D3） |
| 失败语义 | 冷峻：Crashed/TimedOut → run Failed；gate 永久拦截 → Skipped |

## 5. 可复现性（产品承诺）

### 5.1 分级（命名与承诺）

| 级别 | 名称 | 承诺 | 实现 |
|---|---|---|---|
| **L1** | 调度可重放 | 同一图 + 同一输入，事件顺序、门控结果、实例状态可重放 | 事件账本（SQLite、seq 单调）+ 确定性调度（边序/FIFO）；`zgraph replay --mode events` |
| **L2** | 结果可重放（recorded outputs） | **不执行** worker/LLM，用运行期记录的输出重放同一结果；**不承诺重算**（重算会再次触发外部副作用） | 运行期 `--record=full` 记录 worker output 与 LLM 请求/响应；`zgraph replay --mode outputs` |
| **L3** | 外部副作用 | **明确不承诺**：写文件、发消息、调外部 API 无法回滚；v1 **不提供** `--dry-run` | 文档明示；worker 幂等/副作用声明由 M3 契约要求（ISSUE-022） |

绑定因子：`graph_hash` + run_inputs 快照 + worker 版本（module/function、SDK 版本、Python 版本）+ 模型参数 + `seed`。

### 5.2 存储增量（对 spec 06 的显式扩展，M2 实现）

- `runs` 表已有 `graph_hash`（spec 06）；v2 新增列：`seed`、`worker_versions_json`、`model_params_json`、`record_mode`。
- 新表 `recorded_outputs(run_id, instance_id, node_id, attempt, output_hash, output_json NULL, recorded_at)`。
- 新表 `llm_calls(run_id, instance_id, call_index, request_json, response_json, status, recorded_at)`。
- `events`/`transitions`/`gate_results` 沿用 spec 06。
- **M5 写入口径（REV-028/REV-030，回写 spec 06 §6.2.13）**：`recorded_outputs` 由 CLI 组合执行器的外层包装在节点成功执行后写，键 `(run_id, instance_id, attempt)`，`output_hash = blake3(canonical_json(output))` 全 64 hex；`--record=none` 不写、`hash` 只写 hash、`full` 写全文。`llm_calls` 由 CLI 实现 Harness `LlmCallRecorder` seam 写入，**仅 `--record=full`**；每轮一次、`status` 取 `ok`/`error`、请求快照脱敏。`runs.worker_versions_json`/`model_params_json` **仍未写入**（M5 结束时保持 NULL，ISSUE-082 部分修、留 M6）。

### 5.3 CLI 契约（与 spec 06 §6.10 对齐的增量）

| 命令 | 说明 |
|---|---|
| `zgraph run <graph> --inputs <json> [--record=none\|hash\|full]` | `--record` 默认 `hash`；L2 重放要求运行期 `full` |
| `zgraph replay --run <id> [--mode=events\|outputs]` | 默认 `events`（L1）；`outputs` 需要 full 记录，否则 `REPLAY_RECORD_MISSING` |
| `zgraph trace <run_id> [--json]` | 与 spec 06 一致 |
| `zgraph validate <graph>` | 与 spec 06 一致 |

> `--dry-run` **不实现**（v1 明确不支持外部副作用预演）。
>
> **M5 replay 退出码（DR-002 §3.3，回写 spec 06 §6.10.5/§6.10.7；REV-030 已实现、REV-032 e2e）**：`--mode=events` 默认 L1，复用 `zgraph trace` 只读回放、不重跑不写库；`--mode=outputs` 仅支持原始 run `Completed` 且 `record_mode=full`，用 RecordedExecutor 重跑（不启动 worker/LLM），输出不一致 → `REPLAY_OUTPUT_MISMATCH` exit 1；记录缺失/原始 run 非 Completed/记录损坏（hash 不符/重复/缺 `output_json`/`entry_inputs` 非法）（`REPLAY_RECORD_MISSING`/`REPLAY_RUN_NOT_COMPLETED`/`REPLAY_RECORD_CORRUPT`）→ exit 3；run 不存在/存储错误 → exit 4。全部 `REPLAY_*` 属 CLI 命名空间、不写 `runs.error_code`。

### 5.4 复现边界（诚实声明）

- 同一二进制 + 同图 + 同输入 + 同 worker/模型版本：L1 必然成立；L2 在 `--record=full` 下成立。
- 外部副作用不可复现；记录值重放不会再次调用外部系统（这也是 L2 的用途：审计/回放而不重复副作用）。
- worker 内使用系统时间/未注入随机源会破坏 L2（SDK 文档明示，不阻止）。

## 6. 里程碑、验收映射与工具链

### 6.1 里程碑 DoD

| 里程碑 | 范围 | DoD（可验收） |
|---|---|---|
| **M0** 骨架 | Cargo workspace + 6 crate；协议 v1 文档与检查单；Python worker SDK + echo worker；`legacy/` 迁移；CI（cargo fmt/clippy/test + python pytest）；**AGENTS 模块表 + worktree.sh MODULES 同步**；`check.sh`/`merge.sh`/`test-baseline` 支持 Rust+Python 双栈；根 README 标注技术路线 | `cargo test` 绿；`uv run pytest python` 绿；Rust 集成测试 spawn echo worker 完成 NDJSON 往返并通过 §3.5 检查单；`zgraph --version` |
| **M1** DSL | DSL 类型/loader/R1–R14/graph_hash；`zgraph validate`；3 张 legacy 图改写为 fixture | 17 个 DSL 错误码反例 + 合法图通过；R14 规则按 §8 D7 调整（不 import 用户 Python） |
| **M2** 内核 + trace | 状态机/事件循环/Delivery/门控 op/Skipped/冷峻失败/暂停取消/超时（kill 语义）；trace SQLite；死锁检测（AC-11） | AC-01/02/03/05/06/07/10/11 通过（AC-06/07 按 §8 改写）；`zgraph trace` 可回放（L1） |
| **M3** worker | 子进程协议完整实现、崩溃隔离、进程组 kill、emit 流式、stderr 截断、并发上限；3–5 个示例 worker 迁移 | AC-09（按 §8 重标定）通过；故障注入（异常/超时/协议错/信号）全过；AC-06 的 kill 断言通过 |
| **M4** Harness | LLM 流式/tool 循环/上下文预算/错误映射/fake provider；tool 经 worker 子进程 | AC-08（fake 必过，真实模型手动）；token 流式入 trace；tool 超时/失败语义测试 |
| **M5** 收尾 | CLI 完整、`replay` L1/L2、AC-12 多 run 隔离、e2e、legacy 退役决策、文档 | AC-12 通过；L2 重放测试通过；cargo test 全绿 + pytest 基线不扩大；legacy **归档为冻结只读、不物理删除**（物理归档交用户，DR-002 §8） |

### 6.2 AC → 里程碑映射

| AC | 主题 | 里程碑 | v2 备注 |
|---|---|---|---|
| AC-01 | 示例图端到端 | M2 | 用 fixture 图 + echo worker |
| AC-02 | 门控拦截/Skipped | M2 | fixture 改用内置 op（§8 D6） |
| AC-03 | trace 完整/回放 | M2 | L1 |
| AC-04 | 坏图错误码 | M1 | R14 调整（§8 D7） |
| AC-05 | 冷峻失败（异常） | M2 | worker 崩溃/异常 |
| AC-06 | 超时 stop-waiting | M2/M3 | 改为 kill 进程组（§8 D1） |
| AC-07 | 事件循环不阻塞 | M2 | ticker 断言保留；spike 源已入库 |
| AC-08 | 真实 LLM + tool | M4 | tool 走 worker（§8 D3） |
| AC-09 | 并行分支 | M3 | 含进程冷启动，重标定（§8 D8） |
| AC-10 | 暂停/恢复/取消 | M2/M3 | 取消 kill 进程组 |
| AC-11 | 死锁检测 | M2 | 内核能力 |
| AC-12 | 多 run 隔离 | M5 | 依赖完整栈 |

## 7. 风险与对策

| 风险 | 对策 |
|---|---|
| worker 冷启动 ~50–100ms | 先接受；M3 基准后决定池化（O-2/ISSUE-017） |
| 协议演进不兼容 | `v` 版本 + 拒绝不匹配 + §3.5 检查单纳入 CI |
| 双语言调试成本 | 统一 trace（stderr 截断入库、协议错误码）；SDK 提供 mock supervisor |
| 大爆炸重写 | M0–M2 先用 in-process fake worker 打通垂直切片，M3 换真 worker |
| LLM 非确定性 | 记录请求/响应全文 + seed；L2 用记录值重放 |
| legacy 语义漂移 | legacy 冻结只读；验收以 spec v1.2 + 本文 §8 偏差为准 |
| worker 用户代码副作用 | L3 明确不承诺；M3 契约要求声明幂等性（ISSUE-022） |

## 8. 对 spec v1.2 的落地偏差（权威表）

> 说明：spec v1.2 已入库为语义基准；以下偏差是 Rust 内核 + 子进程 worker 的**必然调整**。
> 受影响分册顶部有 banner 指回本表；M0–M2 逐条回写 spec（ISSUE-020），回写后本表保留为决策记录。
> **回写状态（2026-10-30）**：D1–D12 与 §8.2 错误码/指标已回写 spec v1.2 正文（00/01/02/03/04/06/07，见 `doc/spec-v1.2/PROVENANCE.md` §2 第 4 条）；spec 正文与冲突时以 spec 为准，本表仅作决策记录。
> 每条偏差以 **D#** 编号，供代码注释、评审与测试引用。

| # | spec 位置 | v1.2 原文语义 | v2 落地 | 理由/影响 |
|---|---|---|---|---|
| D1 | 03 §3.9、04 §4.3.6、06 §6.3、07 AC-06 | 超时 = stop-waiting：放弃任务继续运行、发 `NodeAbandoned`、`abandoned_tasks += 1`、AC-06 断言 ≥1 | 超时 = **杀进程组**：实例 `TimedOut`、run Failed；`NodeAbandoned` **不产生**；`abandoned_tasks` 恒 0（保留字段兼容），新增 `killed_workers` 计数；AC-06 改为"进程组被 kill、无残留子进程、run 在 5s 内失败" | 子进程可被真正终止；放弃记账在 v2 无对象。**语义变化，必须同步 spec** |
| D2 | 04 §4.2 NodeContext | 进程内 Rust 对象：`emit/get_var/user_id/cancel_requested/check_cancelled/node_deadline_remaining_ms/cancel_token` | Python SDK `ctx` 代理：`emit/log/get_var/user_id/cancel_requested/check_cancelled/deadline_remaining_ms`；`cancel_token` **不存在**（进程边界，以 `cancel` 帧 + 信号替代）；新增 `ctx.now()`/`ctx.random()`（见 D5） | 跨语言契约必须显式；`cancel_token` 无法跨进程 |
| D3 | 05 §5.1/§5.6 | Harness 与 tool 执行都在内核进程内；工具模块一次导入并缓存；工具与 Python 节点共享 I/O 信号量 | Harness 在 Rust 内；**tool 每次调用一个 worker 子进程**；无 import 缓存；工具并发走全局 worker 上限；工具超时 = 杀进程组 + `TOOL_ERROR` 喂回 LLM | 与 D1/D7 一致：用户 Python 不进程内执行 |
| D4 | 04 §4.3.6、03 §3.5 | 单解释器；Python 并发 CPU=1 / IO=4 信号量；`exec_class` 决定类别 | 每节点一进程；全局 `max_workers`（CPU×2）+ 每 run 上限（8）；**`exec_class` 保留但忽略**（池化时再启用）；模块缓存随进程销毁 | 进程隔离后 GIL 不再是限制；无需 CPU/IO 分类 |
| D5 | 04 §4.2、05 §5.4 | 无 seed/时间注入契约；`ctx` 无 `now/random` | 新增 `seed`（run 载荷）、`PYTHONHASHSEED=0`（supervisor spawn 设置）、`ctx.now()`/`ctx.random()`；SDK 按 seed 初始化 `random`/`numpy` | 可复现 L2 的基础 |
| D6 | 05 §5.6、02 R14、07 AC-02/04 | 门控是用户 Python 函数（图加载期 import + 签名校验）；fixture 用 Python gate | v1 门控 = **Rust 内置 op**（`check_output`/`compare`/`manual`）；DSL `gate.function` 只解析内置 op；Python 门控延后（ISSUE-014）；fixture 改用内置 op | 图加载期不执行用户 Python（安全 + 无阻塞） |
| D7 | 02 §2.5、04 §4.7、02 R14 | 图加载期 import Python 节点符号并做签名校验；`DSL_SYMBOL_NOT_FOUND`/`DSL_SIGNATURE_MISMATCH` 在加载期报 | 图加载期**不 import** 用户 Python；`python_node` 的 `module/function` **格式非法仍在加载期报 `DSL_SYMBOL_NOT_FOUND`**；存在性/签名在 **worker 启动时**校验，失败 `NODE_WORKER_IMPORT`（Crashed）；`DSL_SIGNATURE_MISMATCH`/`DSL_SYMBOL_CONFLICT` 只用于内置 gate op 与跨类同名 | 避免加载期执行用户代码；import/签名错误从加载期后移到运行期（已登记） |
| D8 | 07 AC-09/AC-12 | 并行断言基于 in-process spike：a/b 同时 Running 且总耗时 <0.45s；Python 并发 CPU=1/IO=4 | 含进程冷启动（每个 ~50–100ms）：断言改为"a/b 的 Running 区间重叠，且总耗时显著小于串行（M3 实测后定阈值，初值 <1.0s）"；AC-12 改为"全局/每 run worker 上限生效、run 间隔离" | 子进程启动成本与并发模型变化 |
| D9 | 06 §6.10、§6.2 | v1 不做图重放；`runs` 表无版本/seed/输出列；CLI 只有 validate/run/trace | 新增 L1/L2 重放：`runs` 增列 + `recorded_outputs`/`llm_calls` 表；CLI 增 `replay` 与 `--record`；`--dry-run` 不实现 | 产品"可复现"承诺需要账本与重放 |
| D10 | 00 §0.3、§0.4、01 §1.4 | `NodeError`/`Delivery` 为 Rust 进程内类型；错误码表无跨进程错误 | 不变（仍为 Rust 类型）；`NodeError` 增 `NodeWorker { kind }` 变体；新增错误码见 §8.2。**M5 追加**：`NodeWorker` 只增 `signal`/`exit_code`/`stderr_tail`（ISSUE-066/REV-031） | 跨进程错误需要新变体与新码 |
| D11 | 07 AC-07 | 依赖 `_spike/src/bin/t2_gil.rs`（不在仓库） | spike 源与报告已入库 `doc/analysis/spike/`；AC-07 路径改为 `doc/analysis/spike/src/bin/t2_gil.rs`；断言保留（事件循环 ticker） | 溯源可核验 |
| D12 | 08 §8.6、各分册引用 | 引用 `决策记录-01`、`_spike`、`_research`、`_audit` 等仓库外材料 | 决策记录/可行性评估/spike/两份调研已入库 `doc/analysis/`；映射见 `doc/spec-v1.2/REFERENCES.md`；PROVENANCE 记录冻结与回填 | 可核验性 |

### 8.1 偏差的代码落点

- D1/D8：`zgraph-kernel` 超时与 run 生命周期、`zgraph-worker` 进程组 kill；测试 `tests/e2e/timeout_kill.rs`。
- D2/D5：`python/zgraph_worker/ctx.py`、协议 `run` 载荷；测试 `python/tests/test_ctx.py`。
- D3：`zgraph-harness` 的 ToolRunner 调 `zgraph-worker`；测试 `tests/e2e/tool_worker.rs`。
- D4：全局/每 run 信号量；测试 `tests/e2e/concurrency.rs`。
- D6/D7：`zgraph-dsl` 校验 + `zgraph-kernel` 内置 op 注册表；测试 `tests/dsl/*`。
- D9：`zgraph-trace` schema 增量 + `zgraph-cli replay`；测试 `tests/e2e/replay.rs`。

### 8.2 v2 新增错误码与事件（回写 spec 00 §0.3/§0.4 — ISSUE-020）

| 码 / 事件 | 域 | 触发 | 映射 |
|---|---|---|---|
| `NODE_WORKER_HANDSHAKE` | 运行期 | worker `hello` 超时 / 协议版本不匹配 / 缺 `sdk_version` | Crashed（`NodeError::NodeWorker`） |
| `NODE_WORKER_PROTOCOL` | 运行期 | 非法帧、exit 0 无 result、result 后不退出、半行帧、超 8 MiB | Crashed |
| `NODE_WORKER_IMPORT` | 运行期 | worker 启动时 `module/function` 导入失败或签名不符（D7） | Crashed |
| `NODE_WORKER_CRASH` | 运行期 | worker 被信号杀死 / 非约定 exit code | Crashed（`NodeError::NodeWorker{kind:Crash, signal, exit_code, stderr_tail}`；三项仅 supervisor 观察到的进程退出路径有值，stderr_tail ≤1 KiB、不脱敏——ISSUE-066/REV-031） |
| `REPLAY_RECORD_MISSING` | CLI | `replay --mode=outputs` 但运行期未 `--record=full`，或已执行非内置实例缺记录 | CLI 退出码 3（不写 `runs.error_code`；DR-002 §3.3） |
| `REPLAY_RUN_NOT_COMPLETED` | CLI | `replay --mode=outputs` 但原始 run 非 `Completed` | CLI 退出码 3（不写 `runs.error_code`；REV-030） |
| `REPLAY_RECORD_CORRUPT` | CLI | 记录损坏：`recorded_outputs.output_json` 重算 hash ≠ `output_hash` / 重复记录 / 缺 `output_json` / `entry_inputs` 非法 | CLI 退出码 3（不写 `runs.error_code`；REV-030） |
| `REPLAY_OUTPUT_MISMATCH` | CLI | L2 重跑输出 ≠ 原始 `runs.output_json` | CLI 退出码 1（不写 `runs.error_code`；REV-030） |
| `NodeAbandoned` | 事件 | **v2 不产生**（D1）；enum 保留兼容，文档标 deprecated | — |
| `RunMetrics.killed_workers` | 指标 | 因超时/取消被 kill 的 worker 数 | 替代 `abandoned_tasks`（后者恒 0） |

> 上述错误码/事件已回写 spec 00 §0.3/§0.4（2026-10-30，PROVENANCE §2 第 4 条已更新 hash；ISSUE-020 子项 ISSUE-050）。
>
> **M5 追加（ISSUE-066）**：`NodeError::NodeWorker` 在 M5 **只增** `signal`/`exit_code`/`stderr_tail` 三个证据字段（REV-031；仅进程退出路径有值），JSON canonical 形状见 spec 06 §6.2.5.1；公开 variant 增字段 = 仓外 source-breaking，v0.1 已批准（发布说明备注）。

## 9. 溯源

- 语义基准：`doc/spec-v1.2/`（来源、冻结与回填记录见 `doc/spec-v1.2/PROVENANCE.md`；外部材料映射见 `REFERENCES.md`）。
- 决策：`doc/decisions/DR-001-runtime-route.md`；评估与审计：`doc/analysis/feasibility-assessment-v1.1.md`、`doc/analysis/spec-audit-v1.1.md`。
- 技术验证：`doc/analysis/spike/REPORT.md`（8 项探针，含原始命令与数字）。
