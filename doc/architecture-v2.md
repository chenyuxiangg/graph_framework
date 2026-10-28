# 架构蓝图 v2（Rust 内核 + Python 子进程 worker）

> 依据：`doc/decisions/DR-001-runtime-route.md`；语义规格：`doc/spec-v1.2/`。
> 本文冻结**进程模型、跨语言协议、可复现边界、代码布局、里程碑**；实现细节随里程碑补。

## 1. 总体拓扑

```text
┌──────────────────────────── zgraph（单个 Rust 二进制） ─────────────────────────────┐
│ CLI / 嵌入 API                                                                      │
│   └─ RunManager（多 run）                                                            │
│        ├─ Run 事件循环（每 run 一个 task，独占 RunState，禁止阻塞）                    │
│        │    ├─ 状态机 / 就绪判定 / Delivery 应用 / 门控求值                            │
│        │    ├─ 超时与取消（deadline + CancellationToken）                              │
│        │    └─ trace 事件 → 有界通道 → 写线程 → SQLite                                 │
│        ├─ 执行器                                                                      │
│        │    ├─ WorkerExecutor：spawn Python 子进程（NDJSON 协议）                      │
│        │    └─ HarnessExecutor：Rust 内 LLM 调用 + tool 循环                          │
│        └─ GateEvaluator：Rust 内置 op（v1）                                           │
└─────────────┬───────────────────────────────────────────────────────────────────────┘
              │ stdin/stdout NDJSON（协议 v1，见 §3）
              ▼
┌──────────────────────── Python worker 子进程（每节点实例一次） ────────────────────────┐
│ python/zgraph_worker                                                               │
│   ├─ 启动握手（protocol_version / python 版本 / worker_id）                          │
│   ├─ 读 run 消息 → 调用户函数 run(inputs, ctx) → 写 result/error                     │
│   ├─ ctx.emit(...) → stdout 的 emit 消息（流式，可丢）                                │
│   └─ 取消：读 cancel 消息 / SIGTERM → 协作退出；超时由内核 SIGKILL 进程组              │
└────────────────────────────────────────────────────────────────────────────────────┘
```

**关键约束**

1. 事件循环线程**禁止**同步子进程等待、同步 SQLite 写、阻塞 IO；所有执行走 `tokio::process`/`spawn_blocking`。
2. 每个 run 的 `RunState` 由该 run 的事件循环任务独占；执行器只持不可变快照 + `mpsc` 回传。
3. 用户 Python 代码**永远在子进程**；内核不 import Python、不链接 libpython。
4. worker 进程组用 `setsid`（Unix）创建，超时/取消时对**进程组**发 `SIGKILL`，避免孙进程泄漏。

## 2. 代码布局（M0 目标）

```text
crates/
  zgraph-dsl/        # DSL 类型、loader、R1–R14 校验、canonical graph_hash
  zgraph-kernel/     # 编译产物、实例状态机、事件循环、Delivery、门控 op、run 生命周期
  zgraph-trace/      # TraceEvent 模型、SQLite schema/写线程、查询/回放
  zgraph-worker/     # worker 协议编解码、子进程生命周期、超时/取消/回收、stderr 截断
  zgraph-harness/    # AgentConfig、LLM provider（流式）、上下文预算、tool loop
  zgraph-cli/        # validate / run / trace / replay / trigger
python/
  zgraph_worker/     # worker SDK：注册、ctx、协议、日志、错误映射
  workers/           # 迁移后的示例 worker（如 software_team 角色）
schemas/
  graph.schema.json  # DSL JSON Schema（由 Rust 类型生成或手写）
  worker-protocol.md # 协议 v1 文档（与 Rust 常量同源校验）
tests/
  fixtures/graphs/   # 由 legacy 配置改写的验收图
  e2e/               # Rust 集成 + Python worker 端到端
legacy/              # 现有 Python 实现（只读参考，CI 不跑）
doc/                 # 现有文档 + spec-v1.2 + 本蓝图
```

模块归属（用于 worktree 分工；M0 同步更新 `AGENTS.md`）：

| 模块 id | 路径 |
|---|---|
| `dsl` | `crates/zgraph-dsl/**`、`schemas/graph.schema.json` |
| `kernel` | `crates/zgraph-kernel/**` |
| `trace` | `crates/zgraph-trace/**` |
| `worker` | `crates/zgraph-worker/**`、`schemas/worker-protocol.md` |
| `harness` | `crates/zgraph-harness/**` |
| `cli` | `crates/zgraph-cli/**` |
| `py-worker` | `python/zgraph_worker/**`、`python/workers/**` |
| `process` | `Cargo.toml`、`pyproject.toml`、`.github/**`、`scripts/dev/**`、`AGENTS.md`、`README.md` |
| `e2e` | `tests/**` |
| `docs` | `doc/**` |
| `legacy` | `legacy/**`（冻结，只允许整体归档） |

**编码规范适用边界**：`CLAUDE.md`（不可修改）按“Python 代码规范”执行——`python/**` 与 `legacy/**` 遵守其 Python 规则；`crates/**` 遵守 `rustfmt` + `clippy -D warnings` + 等价约束（全类型标注、snake_case、常量、函数 ≤ 60 行）。冲突已登记 ISSUE-019，M0 在 CI 中同时执行两套检查。

## 3. 跨语言协议 v1（冻结草案）

### 3.1 帧格式

- 传输：worker 的 stdin / stdout；**每行一个 JSON 对象**（NDJSON），UTF-8，禁止多行 JSON。
- 上限：单帧 ≤ 8 MiB；超限视为协议错误。
- 协议版本：`v = 1`；不兼容变更必须升 `v` 并提供兼容窗口。
- 信封：

```json
{"v":1,"kind":"<kind>","id":"<可选关联 id>","payload":{...}}
```

### 3.2 消息（worker 启动方向 = supervisor → worker）

| kind | 方向 | 载荷 | 语义 |
|---|---|---|---|
| `hello` | worker → sup | `{protocol_version:1, python_version, worker_id, sdk_version}` | 启动后第一帧；不匹配 → 内核拒绝并杀掉进程 |
| `run` | sup → worker | `{run_id, instance_id, node_id, attempt, module, function, inputs, params, user_id, global_vars, deadline_ms, trace_id}` | 调一次节点；`inputs` 为全部声明端口（未到 `null`） |
| `emit` | worker → sup | `{type:"token"\|"reasoning"\|"tool_call_start"\|"tool_call_end"\|"progress", ...}` | 流式事件（可丢，内核计数） |
| `log` | worker → sup | `{level, message}` | 结构化日志（替代 zlog；写入 trace 或 stderr） |
| `result` | worker → sup | `{output: <JSON>}` | 成功；进程应随后退出（exit 0） |
| `error` | worker → sup | `{code, message, traceback?, details?}` | 业务错误（exit 2）；`code` 映射 NodeError |
| `cancel` | sup → worker | `{}` | 协作取消（尽力）；随后按宽限升级 SIGTERM/SIGKILL |
| `ping`/`pong` | 双向 | `{}` | 健康检查（可选） |

### 3.3 退出码与异常

| 情况 | 行为 |
|---|---|
| exit 0 + `result` | Completed |
| exit 2 + 结构化 `error` | Crashed（BusinessError / NodeError 映射） |
| 非 0 且无 `error` / 协议帧非法 | Crashed（`NODE_WORKER_PROTOCOL`），stderr 末尾 8 KiB 入 trace |
| 超时 | 内核 kill 进程组；实例 `TimedOut`；冷峻模式 → run Failed |
| 取消 | `cancel` → 宽限 2s → SIGTERM → 1s → SIGKILL；实例 `Cancelled` |
| OOM/信号 | 记录 signal/exit code；Crashed |

### 3.4 确定性约定（可复现基础）

worker SDK 强制：`PYTHONHASHSEED=0`；`random`/`numpy` 种子由 `run` 消息的 `seed` 注入（缺省 0）；时间/随机数只能经 `ctx` 获取（`ctx.now()` / `ctx.random()`），直接调用系统时间不会报错但会破坏 L2 可复现，SDK 文档明示。

## 4. 调度与执行模型

| 项 | 决策 |
|---|---|
| 运行时 | tokio multi-thread；每 run 一个事件循环任务（单 run 内串行，run 间并行） |
| 就绪与派发 | 按 spec v1.2 01/03：`all/any`、Skipped 级联、Delivery 两阶段（门控 off-loop） |
| 门控 | v1：Rust 内置 op（含从 Python 仓移植的 `check_output` 等语义）+ 声明式比较；自定义 Python 门控 = O-1 |
| 并发上限 | 全局 worker 并发（默认 CPU 核数×2，可配）；每 run 并发（默认 8） |
| 超时 | 实例 deadline（节点配置 > AgentConfig > 默认 180s）；到期 kill 进程组 |
| 失败语义 | 冷峻：Crashed/TimedOut → run Failed；gate 永久拦截 → Skipped |
| Agent | Harness 在 Rust 进程内（LLM 为 async IO）；tool 执行 = worker 子进程（每次调用一进程，M4 评估池化） |

## 5. 可复现性（产品承诺分级）

| 级别 | 承诺 | 实现 |
|---|---|---|
| **L1 调度可重放** | 同一图 + 同一输入，事件顺序、门控结果、实例状态可重放 | 事件账本（SQLite、seq 单调）+ 调度确定性（边序/FIFO）；`zgraph replay <run_id>` |
| **L2 结果可重算** | 节点输出、LLM 响应可记录并重放；外部可见结果与原始 run 一致 | `--record` 落库 worker output hash + 可选原文；LLM 请求/响应全文入库；replay 时用记录值替代执行 |
| **L3 外部副作用** | **不承诺**（写文件、发消息、调外部 API 无法回滚） | 文档明示；worker 必须幂等或由用户 `--dry-run` 验证 |
| 绑定因子 | `graph_hash`（canonical DSL）+ run_inputs 快照 + worker 版本（module/function + SDK 版本 + Python 版本）+ 模型参数 + seed | `runs` 表与 `RunStarted` 事件记录 |

> 复现命令（M5 目标）：`zgraph replay --run <id>` 默认 L1；`--use-recorded-outputs` 升级 L2；不提供 L3。

## 6. 里程碑与验收

| 里程碑 | 范围 | DoD |
|---|---|---|
| **M0** 骨架 | Cargo workspace + 6 个 crate 空壳；协议 schema 文档；Python worker SDK + echo worker；CI（cargo fmt/clippy/test + python pytest）；`legacy/` 冻结迁移；AGENTS 模块表更新 | `cargo test` 绿；`uv run pytest python` 绿；Rust 集成测试成功 spawn echo worker 并完成一次 NDJSON 往返；`zgraph --version` 可用 |
| **M1** DSL | 类型/loader/R1–R14/graph_hash；`zgraph validate`；3 张 legacy 图改写为 fixture | 17 个 DSL 错误码反例 + 合法图通过（AC-04 移植） |
| **M2** 内核 | 状态机/事件循环/Delivery/门控 op/Skipped/冷峻失败/暂停取消/超时；trace SQLite | AC-01/02/03/05/06/07 移植；`zgraph trace` 可回放 |
| **M3** worker | 子进程协议、崩溃隔离、进程组 kill、emit 流式、stderr 截断、并发上限 | 真实 Python 节点跑通 demo 图；AC-09/10；故障注入（异常/超时/协议错） |
| **M4** Harness | LLM 流式/tool 循环/上下文预算/错误映射/fake provider | AC-08（fake 必过，真实模型手动）；token 流式入 trace |
| **M5** 收尾 | CLI 完整、replay L1/L2、e2e、legacy 退役决策、文档 | 全部 AC 通过；`doc/test-baseline.txt` 归零或只保留已登记项 |

## 7. 风险与对策

| 风险 | 对策 |
|---|---|
| worker 冷启动开销（每节点 ~50–100ms） | 先接受；M3 后用基准数据决定是否池化（O-2） |
| 协议演进不兼容 | `v` 版本号 + supervisor 拒绝不匹配 + 协议兼容测试 |
| 双语言调试成本 | 统一 trace（worker stderr 截断入库、协议错误码）；worker SDK 提供本地 mock supervisor |
| 大爆炸重写风险 | M0–M2 先用 in-process fake worker 打通垂直切片，M3 再换真 worker |
| LLM 非确定性破坏 L2 | 记录请求/响应全文 + seed；replay 默认使用记录值 |
| legacy 与新代码语义漂移 | legacy 冻结只读；验收断言以 spec v1.2 为准；M1 用 legacy 图做 fixture 交叉验证 |
