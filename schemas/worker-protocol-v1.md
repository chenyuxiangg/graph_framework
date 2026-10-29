# worker 协议 v1（冻结）

> 状态：**v1 冻结**（M0）。实现：Rust `crates/zgraph-worker`；Python `python/zgraph_worker`。
> 设计依据：`doc/architecture-v2.md` §3；变更规则：不兼容修改必须升 `v` 并保留 1 个版本的兼容窗口。

## 1. 传输与帧

- 传输：worker 的 **stdin / stdout**；**每行一个 JSON**（NDJSON），UTF-8，禁止多行 JSON。
- 信封：`{"v":1,"kind":"<kind>","id":"<可选>","payload":{...}}`
- 单帧上限：**8 MiB**（超限 = 协议错误）。
- 方向：`sup→w` = supervisor 写 worker stdin；`w→sup` = worker 写 stdout。

## 2. 消息

| kind | 方向 | 载荷 | 说明 |
|---|---|---|---|
| `hello` | w→sup | `{protocol_version:1, python_version, sdk_version, worker_id}` | spawn 后 **5s 内**必须收到；超时/非法 → 杀进程组，`NODE_WORKER_HANDSHAKE` |
| `hello_ack` | sup→w | `{accepted:true}` | 校验通过后发送；worker 收到前**不得**执行用户代码 |
| `run` | sup→w | `{run_id, instance_id, node_id, attempt, module, function, inputs, params, user_id, global_vars, seed, deadline_ms, trace_id}` | `inputs` 含全部声明端口（未到 `null`）；`seed` 为 u64 |
| `emit` | w→sup | `{type:"token"\|"reasoning"\|"tool_call_start"\|"tool_call_end"\|"progress", ...}` | 流式事件，可丢（内核计数并聚合落库） |
| `log` | w→sup | `{level:"debug"\|"info"\|"warn"\|"error", message}` | ≤64 KiB/帧 |
| `result` | w→sup | `{output:<JSON>}` | **恰好一帧**，随后 1s 内退出 |
| `error` | w→sup | `{code, message, traceback?, details?}` | 结构化失败，随后退出 |
| `cancel` | sup→w | `{}` | 协作取消；随后按 §4 升级信号 |
| `ping`/`pong` | 双向 | `{}` | 可选健康检查 |

## 3. 退出码与判定（封闭表）

| 情况 | 判定 |
|---|---|
| exit 0 + 恰好 1 个 `result` | 成功 |
| exit 0 无 `result` / 多 `result` / result 后 1s 未退出 | 协议错误 `NODE_WORKER_PROTOCOL` |
| exit 2 + 结构化 `error` | 业务失败（按 `code` 映射 NodeError） |
| exit 3（SDK 自检失败） | 协议错误 `NODE_WORKER_PROTOCOL` |
| 被信号杀死 / 其他 exit code | 崩溃 |
| 超时（deadline） | `TimedOut` + 杀进程组 |
| 取消 | `Cancelled` + 杀进程组 |

## 4. 生命周期与隔离

1. **stdout 隔离**：SDK 启动时 `os.dup(1)` 保存协议 fd，把 `sys.stdout` 重定向到 stderr；用户 `print()` 只进 stderr。
   用户 `input()` 被禁用（stdin 是协议），调用即抛 `RuntimeError`。
2. **stderr 排空**：supervisor 必须持续读 stderr（环形缓冲保留末尾 8 KiB），否则 pipe 满会阻塞 worker。
3. **取消**：SDK 的 stdin reader 线程收到 `cancel` 置 `ctx.cancel_requested=True`；用户函数在安全点调用
   `ctx.check_cancelled()`（抛 `WorkerCancelled`）。supervisor 顺序：`cancel` → 宽限 2s → `SIGTERM` → 1s → `SIGKILL` 进程组。
4. **帧超时**：除 `run` 期间外，空闲超时 `worker_frame_timeout_ms`（默认 300s）；EOF 若有半行 = 协议错误。
   实现口径（M3，`zgraph-worker::supervisor`，Lead 批准方案 A）：
   - 只用于「非 run-result 等待」的读帧（hello 另有 5s `hello_timeout`；result 后退出由 1s `exit_grace` 管）；
   - run-result 等待：`RunRequest.deadline_ms != 0` 时**豁免**（内核 deadline 是权威判据，D1）；
     `deadline_ms == 0`（无内核 deadline，M0 直接调用）时用 `worker_frame_timeout_ms` 兜底；
   - 超时分类：supervisor 返回 `NODE_WORKER_PROTOCOL`（worker 停止产出帧 = 协议停滞）并**杀进程组**；
     **不得**映射为 `NodeError::Timeout`（04 §4.1.4 只允许 AgentCallHandler 自报 Timeout）。
5. **确定性**：supervisor spawn 时设置 `PYTHONHASHSEED=0`；SDK 按 `seed` 初始化 `random`/`numpy`；
   `ctx.now()`/`ctx.random()` 由 SDK 提供（直接调系统时间会破坏 L2，文档明示）。
6. **并发上限**（D4）：全局 `max_workers`（默认 CPU 核数×2）+ 每 run `max_workers_per_run`（默认 8），
   由 `WorkerExecutor`（`crates/zgraph-worker/src/executor.rs`）用信号量实现、可配；
   先取每 run 许可再取全局许可（固定顺序，无死锁）。崩溃/超时只影响本节点实例。
7. **流式读取上限**（M3）：supervisor 对 emit 帧累计计数，超过 `max_events`（默认 4096）或
   `max_bytes`（默认 8 MiB）即丢弃后续 emit 并计数（可丢，§2；**禁止无界聚合导致 OOM**）。
   统计随 `WorkerRun.streaming` 返回；`log` 帧不聚合（只保证持续读取，stderr 另走 §4.2 尾保留）。

## 5. 错误码（与 `doc/architecture-v2.md` §8.2 对齐）

`NODE_WORKER_HANDSHAKE` / `NODE_WORKER_PROTOCOL` / `NODE_WORKER_IMPORT` / `NODE_WORKER_CRASH` /
`NODE_PYTHON_EXCEPTION` / `NODE_BUSINESS_ERROR` / `NODE_RETURN_NOT_DICT` / `NODE_CANCELLED` / `NODE_TIMEOUT`

## 6. 协议 v1 验收检查单（M0 + M3；状态：`[x]` 已实现/有测试）

- `[x]` `hello` 超时（5s，测试用 300ms）/ `v` 不匹配 / 缺字段 → 拒绝并 kill
- `[x]` `hello_ack` 前不执行用户代码（`python/tests/test_runtime.py::test_no_execution_before_hello_ack`，标记文件负例）
- `[x]` `run` 载荷含 `seed` / `deadline_ms` / `global_vars`
- `[x]` 重复 `result` 检测（进程退出后读残余帧）；`[x]` exit 0 无 result（fixture 测试）
- `[x]` result 后不退出（supervisor 层）：`crates/zgraph-worker/tests/supervisor_m3.rs::result_without_exit_is_protocol_error_and_process_group_killed`（`NODE_WORKER_PROTOCOL` + 杀进程组 + PID 消失）
- `[x]` 退出状态分类（§3 封闭表）：exit 0 无 result → 协议错误；信号/非约定码 → 崩溃 `NODE_WORKER_CRASH`（`executor_m3::signal_exit_maps_to_worker_crash`/`exit_without_result_maps_to_worker_protocol`）
- `[x]` stderr 持续排空 + 8 KiB 尾部截断（压力用例）
- `[x]` 用户 `print()` 不污染协议；`input()` 抛错
- `[x]` 同步用户函数执行中收到 `cancel` → 标志置位、协作退出（Python 侧）
- `[x]` supervisor 发送 cancel → 2s → SIGTERM → 1s → SIGKILL 进程组：`supervisor_m3::cancel_escalates_through_sigterm_to_sigkill_group`（fixture 忽略 SIGTERM，必须走到 SIGKILL；PID 消失）
- `[x]` 内核超时/取消 abort 的 drop 路径杀进程组：`executor_m3::kernel_timeout_kills_worker_process_group_including_grandchild`、`executor_m3::kernel_cancel_kills_worker_process_group`（含孙进程、5s 内无残留）
- `[x]` 半行帧 / 非法 UTF-8（`invalid_utf8_frame_is_rejected`）/ 超 8 MiB（帧解码层）
- `[x]` 流式读取上限：`supervisor_m3::emit_event_limit_drops_frames_and_keeps_counting` / `emit_byte_limit_drops_frames_and_keeps_counting`（超限丢弃计数、保留字节有界）
- `[x]` 帧空闲超时：`supervisor_m3::frame_idle_timeout_applies_when_no_kernel_deadline`（`deadline_ms=0`）与 `frame_timeout_is_suspended_when_kernel_deadline_present`（有 deadline 时豁免）
- `[x]` 超时不等待：显式 kill + wait/reap（run 超时与 hello 超时两条路径，`*_kills_and_reaps_process` 断言 PID 消失）
- `[x]` 进程组 kill（孙进程）：`supervisor_m3::timeout_kills_process_group_including_grandchild`（断言 worker 与孙进程 PID 均消失）
- `[x]` 全局与每 run 并发上限：`executor_m3::global_limit_serializes_parallel_branches` / `per_run_limit_serializes_parallel_branches` / `branches_run_concurrently_within_limits`（按执行区间是否重叠断言）
- `[x]` emit 流式入内核：`executor_m3::emit_frames_are_forwarded_to_kernel_streaming_sink`（5 类事件全部进 `TraceSink::streaming`）
- `[x]` 错误映射矩阵（D10/§8.2）：`executor_m3::{python_exception_maps_to_python_exception, sdk_business_error_maps_to_business_error, custom_business_code_is_preserved_in_node_error, import_failure_maps_to_worker_import, exit_without_result_maps_to_worker_protocol, signal_exit_maps_to_worker_crash}`

> 退出约定：worker 在写完 `result`/`error` 后 **flush stderr 并 `os._exit`**（跳过 atexit），
> 因此用户代码不应依赖 `atexit`/`finally` 的收尾（worker 契约，见 §4）。

## 7. 未覆盖项与已知限制（M3 收尾，ISSUE-025 已闭环）

M3 起 `schemas/worker-protocol-v1.md` §6 检查单**无 `[M3]` 残留**；以下为已知限制，建议由评审登记台账：

1. **supervisor 级 streaming 丢弃未进入内核指标**：emit 超限丢弃在 supervisor 计数
   （`WorkerRun.streaming.dropped`），但内核 `RunMetrics.streaming_dropped` 只统计 `ctx.emit`
   之后的通道丢弃；两者尚未合并（需内核提供累加口，M3+ 评估）。
2. **每 run 信号量表不回收**：`WorkerExecutor.per_run` 按 run 懒建 `Semaphore`，run 结束后不删除
   （多 run 长进程内缓慢增长）；M5 在多 run 隔离/收尾时清理。
3. **`log` 帧未落 trace**：M3 只保证持续读取不阻塞；结构化日志入库归 06 存储/M5。
4. **成功路径不清理孙进程**：仅超时/取消/协议错误杀进程组；worker 正常退出后其残余子进程
   不会被主动清理（M3 不承诺，L3 副作用边界）。
5. **平台范围**：进程组语义仅 Unix（`process_group` + `nix`）；非 Unix 退化为直接 kill 子进程，
   不保证孙进程回收。
