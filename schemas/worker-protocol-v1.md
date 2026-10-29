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
5. **确定性**：supervisor spawn 时设置 `PYTHONHASHSEED=0`；SDK 按 `seed` 初始化 `random`/`numpy`；
   `ctx.now()`/`ctx.random()` 由 SDK 提供（直接调系统时间会破坏 L2，文档明示）。

## 5. 错误码（与 `doc/architecture-v2.md` §8.2 对齐）

`NODE_WORKER_HANDSHAKE` / `NODE_WORKER_PROTOCOL` / `NODE_WORKER_IMPORT` / `NODE_WORKER_CRASH` /
`NODE_PYTHON_EXCEPTION` / `NODE_BUSINESS_ERROR` / `NODE_RETURN_NOT_DICT` / `NODE_CANCELLED` / `NODE_TIMEOUT`

## 6. M0 验收检查单（状态：`[x]` 已实现/有测试；`[M3]` 延后并登记 ISSUE-025）

- `[x]` `hello` 超时（5s）/ `v` 不匹配 / 缺字段 → 拒绝并 kill
- `[x]` `hello_ack` 前不执行用户代码
- `[x]` `run` 载荷含 `seed` / `deadline_ms` / `global_vars`
- `[x]` 重复 `result` 检测；`[M3]` exit 0 无 result、result 后不退出（supervisor 层）
- `[x]` stderr 持续排空 + 8 KiB 尾部截断（压力用例）
- `[x]` 用户 `print()` 不污染协议；`input()` 抛错
- `[x]` 同步用户函数执行中收到 `cancel` → 标志置位、协作退出（Python 侧）
- `[M3]` supervisor 发送 cancel → 2s → SIGTERM → 1s → SIGKILL 进程组
- `[x]` 半行帧 / 非法 UTF-8 / 超 8 MiB（帧解码层）；`[M3]` 流式读取上限与帧空闲超时
- `[x]` 超时不等待：显式 kill + wait/reap（单进程，测试断言 PID 消失）
- `[M3]` 进程组 kill（孙进程）/ 全局与每 run 并发上限

> 退出约定：worker 在写完 `result`/`error` 后 **flush stderr 并 `os._exit`**（跳过 atexit），
> 因此用户代码不应依赖 `atexit`/`finally` 的收尾（worker 契约，见 §4）。

## 7. 未覆盖项（M3，见 `doc/issues.md` ISSUE-025）

supervisor 侧 cancel/信号升级、帧空闲超时、流式读取上限、exit-0-无-result 分支、进程组 kill、并发上限。
