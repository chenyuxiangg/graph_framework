> **归档说明**：8 项技术验证报告（冻结期归档）。源码与脚本已随本目录入库；原始日志未入库（见 [`REFERENCE`](../../spec-v1.2/REFERENCES.md)）。

# 图模型驱动工作流框架 — 技术验证 Spike 报告

> 目标：用**真实编译、真实运行**证明/证伪《图模型驱动工作流框架-详细设计 v1.1》的核心技术假设：
> Rust 内核 + PyO3 动态导入 Python 节点/gate + tokio 单线程事件循环 + 每节点 `tokio::spawn` + trace 经 mpsc 异步落 SQLite。
>
> 本报告所有数字都来自 `_spike/logs/*.log` 的实测输出，可复现（见 §2）。失败项保留原始证据，不修饰。

---

## 0. 结论先行

**总体判断：没有阻塞级技术障碍。PyO3 0.30.1 + Python 3.13 + tokio + SQLite 全部可用；但设计中的 3 个执行模型假设必须修改后才能落地。**

| # | 设计假设 | 实测结论 | 判定 |
|---|---|---|---|
| 1 | PyO3 0.30.1 嵌入 Python 3.13，`run(inputs, ctx)` / gate 动态导入调用，serde_json ↔ PyObject 双向转换 | 全部通过：dict/list/嵌套/None/bool/int/float/str/tuple/Unicode/i64 精度，JSON 往返稳定；异常、traceback 完整 | ✅ 可直接落地 |
| 2 | 节点执行 `tokio::spawn` 不阻塞事件循环 | **证伪**。`tokio::spawn` 里同步调 Python：4 个纯 `time.sleep(0.4)` 任务让事件循环 ticker 最大停顿 **405 ms**；纯计算任务停顿 **1502–1530 ms**；单线程 runtime + `spawn` 甚至把 4 个 sleep 完全串行成 **1614.8 ms** | ❌ 必须改架构 |
| 3 | 并行分支中的 Python 节点可以并行 | **部分证伪**。4 个 GIL 释放型节点（sleep）并发 **405 ms**（≈1×）；4 个 CPU-bound 节点并发 **1608–1686 ms**（≈4×，与串行 1609 ms 相同）→ **CPU 型 Python 节点被 GIL 串行化** | ⚠️ 需改预期/改架构 |
| 4 | `NodeContext.emit` 同步回调 + mpsc 有界满时丢弃 streaming 事件（`try_send`） | 可行且安全：无死锁、无 panic；10k 次回调 **38.6 ms（3.86 µs/次）**；cap=2 时 100 次 emit → sent 2 / dropped 98；回调内重入 Python（`json.dumps` + 嵌套 `Python::attach`）安全 | ✅ 可直接落地（须加丢弃计数） |
| 5 | 节点实例 30s 超时"不等待任务返回" | 只能做到"不再等待"：`timeout`/`CancellationToken` 在 **103–106 ms** 返回；但 Python 任务**继续运行**，drop runtime 要等它跑完（**1890.7 ms**），被放弃任务让其它 Python 调用变慢 **8.9×**（单次孤立测量达 113×）。`PyErr_SetInterrupt` 在本嵌入环境**完全无效** | ⚠️ 语义必须重定义 |
| 6 | Python 异常 → Rust 错误，解释器可继续 | 通过：自定义异常类型/消息/`code` 属性/4 行 traceback 全部结构化，连续 3 轮失败后解释器健康，`sys.exc_info()` 干净 | ✅ 可直接落地 |
| 7 | trace 走 mpsc 异步落 SQLite，不阻塞主循环 | 通过：10k 条事件异步管道总耗时 **36.1 ms**，主循环 ticker 最大停顿 **6 ms**；同步单事务 16.2 ms、同步 autocommit 133.9 ms；无消费者时 cap=1024 丢弃 8976/10000 | ✅ 可直接落地 |
| 8 | 单线程事件循环 + 任务经 channel 回传结果，避免 `&mut` 别名与 `'static` 问题 | 通过：最小 DAG 引擎编译运行成功，并行分支 a/b 同时启动（0 ms），总耗时 **310.7 ms**（串行 500 ms，1.61×），ticker 停顿 **6 ms**；反例（spawn 捕获 `&mut self`）编译失败 E0521/E0382 | ✅ 可直接落地 |
| 9 | PyO3 `gate(output, ctx) -> bool` | 可用，但 **Rust 侧严格 bool 提取**：返回 `str`/`int`/`None` 都会 TypeError；需明确"严格 bool"或显式 truthiness 语义 | ✅ 可直接落地（需语义约定） |

**是否存在阻塞级技术障碍：没有。** 但以下决策**必须在写代码前定稿**（详见 §4）：

1. Python 节点**不得**在 `tokio::spawn` 的 async 任务里直接 `Python::attach` 执行；必须走 `spawn_blocking` 或专用 Python 执行线程/线程池。
2. "并行分支"能力必须区分 **GIL 释放型**（I/O/sleep，可并行）与 **CPU 型**（被 GIL 串行，吞吐≈1 核）；不能用"分支数=并行度"做容量规划。
3. 超时只能实现为"放弃等待 + 记录 timeout 事件"，**不能承诺终止任务**；若要真正终止 Python 代码，只能靠协作式取消（节点自检 `ctx.cancel_requested`），最后手段是 `PyThreadState_SetAsyncExc`（unsafe，且无法打断 C 阻塞调用）。

---

## 1. 环境与版本（实测）

| 项 | 值 |
|---|---|
| 日期 | 2026-10-27（UTC 2026-10-27T12:02Z） |
| 机器 | macOS 26.6 (Darwin 25.6.0) arm64，8 核 |
| rustc / cargo | 1.99.1 (58cf8f9c3 2026-10-13) / 1.99.1 (5f94df478 2026-08-27) |
| Python | 3.13.0（`/opt/homebrew/bin/python3`，Homebrew，GIL 构建） |
| pyo3 | 0.30.1 |
| tokio | 1.54.1（features: rt-multi-thread, macros, sync, time） |
| tokio-util | 0.7.20（CancellationToken） |
| rusqlite | 0.37.0（`bundled`，libsqlite3-sys 0.35.0） |
| serde_json | 1.0.154 |
| 编译模式 | `dev`（unoptimized）；Python 侧为 CPython 正常构建 |

环境注记（对复现很重要）：

- 本机**直连 static.crates.io / index.crates.io 超时**（curl 15s 无响应），必须走本机 Clash 代理 `http://127.0.0.1:7890`。工程内 `_spike/.cargo/config.toml` 已配置该代理。
- 会话文件沙箱只允许写工作区，cargo 无法写 `~/.cargo/registry/cache`。因此 `scripts/env.sh` 把 `CARGO_HOME` 指向 `_spike/.cargo-home`（首次构建走代理下载 42 个 crate，之后可离线）。
- `PYO3_PYTHON=/opt/homebrew/bin/python3`（由 `scripts/env.sh` 导出）。

---

## 2. 复现方式

```bash
cd /Users/zyzs/lczyzs/code/zgraph/_spike
bash scripts/run_all.sh          # 构建 + 跑全部 probe + 写 logs/
```

单跑某一项：

```bash
source scripts/env.sh
cargo build --bins
./target/debug/t1_embed          # 探针 1
./target/debug/t2_gil            # 探针 2（约 15s）
./target/debug/t3_callback       # 探针 3
./target/debug/t4_gate           # 探针 4
./target/debug/t5_errors         # 探针 5
./target/debug/t6_trace          # 探针 6
./target/debug/t7_timeout        # 探针 7（约 8s）
./target/debug/t7b_interrupt main|worker|async_exc|async_exc_sleep   # 探针 7b
./target/debug/t8_eventloop      # 探针 8
```

编译失败证据（故意不编译的独立工程）：

```bash
cd alias_fail && CARGO_TARGET_DIR=../target cargo check
```

---

## 3. 逐项验证结果

### 3.1 探针 1 — PyO3 0.30.1 + Python 3.13 嵌入与 JSON 双向转换

代码：`src/bin/t1_embed.rs`，Python：`py/node_ok.py`，日志：`logs/10_t1_embed.log`。

实测输出（关键行）：

```
python_version=3.13.0 (main, Oct  7 2024, 05:02:14) [Clang 15.0.0 (clang-1500.1.0.2.5)]
conversion_rust_to_py=dict/int/float/str/bool/none/list/nested OK
bool_is_pybool=true
ROUNDTRIP_JSON_STABLE=true
out.sum=7.5 out.sum_is_pyint=false        # int+float 按 Python 语义得到 float
out.none_is_none=true out.tuple_is_list_in_json=true
out.big=1152921504606846976 (i64 fidelity)
out.unicode_roundtrip=héllo 世界 🦀
second_call_sum=2.0
PROBE1_OK=1
```

结论：

- `Python::initialize()` + `Python::attach` + `PyModule::import` 从文件加载模块、`func.call1((inputs, ctx))` 全部可用；`ctx=None` 与真实 `#[pyclass]` ctx 均可传。
- 自定义 `json_to_py` / `py_to_json` 覆盖 null/bool/int/float/str/list/dict，Python `tuple` 自动转为 JSON array；非字符串 dict key 会显式报 `TypeError`（不静默丢数据）。
- i64 大整数、Unicode、嵌套结构、多轮调用均无问题。
- PyO3 0.30.1 的 API 变化（实现时踩到并已修正）：`downcast/cast_into` → `cast/cast_into`（`cast` 返回 `&Bound<T>`）、`Python::version()`→`Python::version_str()`、`PyErr::value(py)` 返回 `&Bound<PyBaseException>`。这不影响可行性，但**实现团队要按 0.30 文档写**，网上大量 0.20/0.23 示例不能照抄。

### 3.2 探针 2 — tokio + Python 执行模型与 GIL（核心）

代码：`src/bin/t2_gil.rs`，Python：`py/node_work.py`，日志：`logs/20_t2_gil.log`。

方法：同一 Python 工作单元并发 4 份，测 (a) 总墙钟时间，(b) 同一 tokio runtime 上 5 ms ticker 任务的最大调度间隔（= 事件循环被阻塞的时长）。

- `sleep_node(0.4)`：`time.sleep`，**释放 GIL**。
- `busy_node(0.4)`：`while time.thread_time()-start < 0.4` 忙等，**持有 GIL**，且以**线程 CPU 时间**为截止条件（重要：若用 `perf_counter()` 墙钟截止，4 个线程会在同一墙钟时刻同时退出，会伪造出"并行"假象——我们第一版就踩了这个坑，已修正）。

实测（8 核机器，4 个 tokio worker）：

| 执行模型 | 4×sleep(0.4s) | ticker 最大停顿 | 4×busy(0.4s CPU) | ticker 最大停顿 |
|---|---|---|---|---|
| 串行基线（单线程顺序） | 1609.1 ms | — | 1686.0 ms | — |
| 多线程 rt + `tokio::spawn` + `Python::attach` | **405.2 ms** | **405 ms** | **1608.1 ms** | **1502 ms** |
| 多线程 rt + `spawn_blocking` + `Python::attach` | **405.3 ms** | **6 ms** | **1609.4 ms** | **6 ms** |
| `std::thread` ×4 + `Python::attach` | 402.0 ms | — | 1619.4 ms | — |
| 单线程 rt（current_thread）+ `tokio::spawn` | **1614.8 ms** | **1614 ms** | （未测，必然串行） | — |
| 单线程 rt + `spawn_blocking` | **404.4 ms** | **6 ms** | 1614.9 ms | **8 ms** |
| 纯 async 对照（`tokio::time::sleep`） | 401.7 ms | 6 ms | — | — |

（复跑：busy + `spawn_attach` 1613.2 ms / gap 1530 ms；busy + `spawn_blocking` 稳定 ≈1.61 s / gap 6–7 ms。）

**结论（直接回答 lead 的问题 1）：**

1. **"节点执行 `tokio::spawn` 不阻塞事件循环"不成立。** 只要节点体是同步 Python 调用（PyO3 的 Python 调用是同步阻塞的），它就会占死一个 tokio worker 线程：
   - 即使 `time.sleep` 释放 GIL，worker 线程仍被 Rust 调用占住 → 4 个任务让事件循环停顿 405 ms；
   - CPU 型节点让事件循环停顿 1.5 s（4 个 worker 全被占满，其它 async 任务无法被 poll）。
   - 单线程事件循环 + `tokio::spawn` 更糟：4 个 sleep 完全串行（1614.8 ms），因为任务在事件循环线程上同步阻塞。
2. **GIL 下并行分支的 Python 节点会被串行化（对 CPU 型工作）。** 4×0.4 s CPU 工作无论用 `spawn_blocking`、`std::thread` 还是顺序执行，墙钟都是 ~1.6 s（4×）；只有**释放 GIL 的 I/O 型工作**（`time.sleep`、socket、文件 I/O）才能真正并行（405 ms ≈ 1×）。
3. **推荐执行模型：**
   - 事件循环用 `current_thread` runtime（或任意 rt），**所有 Python 节点/gate 调用一律 `spawn_blocking`**，完成事件经 mpsc 回传（探针 8 已证明可行：DAG 并行 1.61× 且 ticker gap 6 ms）；
   - 或者更彻底的"专用 Python 执行器线程池"：固定 N 个 OS 线程各持 GIL 执行节点，事件循环只发任务/收事件（与 `spawn_blocking` 等价，但可观测、可限流、可绑定取消协议）；
   - **必须给 Python 节点单独设并发上限**：CPU-bound 节点并发上限 ≈1（GIL 决定吞吐），I/O 型可大于 1；不要用 tokio worker 数当 Python 节点并发度。
   - 不要指望 Python 3.13 free-threaded（3.13t）——本机是普通 GIL 构建，且 PyO3/生态对 3.13t 支持仍是实验性，不能作为架构依据。

### 3.3 探针 3 — Python → Rust 回调 `ctx.emit(dict)`

代码：`src/bin/t3_callback.rs`，Python：`py/node_emit.py`，日志：`logs/30_t3_callback.log`。

`#[pyclass] EmitCtx` 持有 `tokio::sync::mpsc::Sender<serde_json::Value>`（无需 runtime 即可 `try_send`），Python 节点循环调用 `ctx.emit({...})`：

| 场景 | 实测 |
|---|---|
| 5 次 emit（cap 64，emit 内重入 Python：`json.dumps` + 嵌套 `Python::attach`） | 4.03 ms，sent 5 / dropped 0 / drained 5，reentries 5，**无死锁** |
| 100 次 emit（cap 2，不消费） | 0.44 ms，sent 2 / **dropped_full 98** / drained 2，无 panic |
| 100 次 emit（cap 1 + 回调内重入 Python） | 1.16 ms，sent 1 / dropped 99 / reentries 100，**无死锁** |
| 10 000 次 emit（cap 20000） | **38.6 ms → 3.86 µs/次**，sent 10000 / dropped 0 |

**结论（回答 lead 的问题 2）：`try_send` 方案安全、可落地，但必须补两个工程约束：**

- 安全：回调路径无阻塞、无锁等待、无重入死锁；Python 持 GIL 时回调 Rust，再在回调里调 Python API / 嵌套 attach 也是安全的（GIL 可重入，PyO3 支持）。
- 代价与风险：`try_send` 满时**静默丢弃**。必须把 sent/dropped/closed 计数暴露到指标（本探针 `EmitCtx.stats` 已示范），否则用户会看到事件缺失却无告警。设计应明确"哪类事件可丢"：streaming/progress 可丢；生命周期/错误/最终结果不应走可丢通道。
- 回调开销 ~3.9 µs/次，10k 事件 38.6 ms，热路径可接受；但**不要在 emit 里做 SQLite 写/日志格式化/加锁**，否则会把 GIL 持有时间放大。
- 容量：cap 需按"消费端最坏停顿 × 事件速率"估算；cap=1024 且消费者停摆时 10k 事件丢 8976（见探针 6）。

### 3.4 探针 4 — Gate 求值 `gate(output, ctx) -> bool`

代码：`src/bin/t4_gate.rs`，Python：`py/gate.py`，日志：`logs/40_t4_gate.log`。

| gate 返回 | `extract::<bool>()`（严格） | `is_truthy()`（真值） |
|---|---|---|
| `bool` True/False | Ok(true)/Ok(false) | 一致 |
| `"yes"`（str） | **Err TypeError: 'str' object is not an instance of 'bool'** | Ok(true) |
| `1`（int） | **Err TypeError: 'int' object is not an instance of 'bool'** | Ok(true) |
| `None` | **Err TypeError: 'None' is not an instance of 'bool'** | Ok(false) |
| `raise ValueError` | Err 结构化：type/message/4 行 traceback，解释器继续可用，`sys.exc_info()` 干净 | — |

**结论：** gate 调用与 Value↔PyObject 转换可用，异常不会污染解释器；**但设计必须明确 bool 语义**。建议 schema 层强制 `isinstance(result, bool)`（严格），并在文档中写清非 bool 返回值是节点错误；或者产品上明确接受 truthiness（用 `is_truthy`）。不能像现在这样"声明 `-> bool` 但实现用 `extract::<bool>()` 又在运行时才发现不符"。

### 3.5 探针 5 — Python 异常 → 结构化 Rust 错误 + 解释器恢复

代码：`src/bin/t5_errors.rs`，Python：`py/node_err.py`，日志：`logs/50_t5_errors.log`。

实测（连续 3 轮）：

```json
{
  "kind": "python_exception",
  "type": "NodeError",
  "message": "intentional node failure from Python",
  "code": "E_NODE_BOOM",                     // 自定义异常属性被保留
  "traceback": "Traceback (most recent call last):\n  File \".../py/node_err.py\", line 9, in run\n    raise NodeError(...)\nnode_err.NodeError: intentional node failure from Python\n",
  "traceback_lines": 4
}
HEALTHY_ROUNDS_AFTER_ERROR=3/3 SYS_EXC_INFO_CLEARED=true
```

**结论：可直接落地。** 每次异常后紧接跑一个正常节点，3/3 成功；`sys.exc_info()` 均清空；traceback 含文件/行号/源码行，可直接进错误报告。注意 Rust 侧要主动取 `err.value(py)` 的自定义属性（如 `code`）并序列化，PyO3 默认不会带出。

### 3.6 探针 6 — trace 事件通道 + SQLite（rusqlite bundled）

代码：`src/bin/t6_trace.rs`，日志：`logs/60_t6_trace.log`。

10 000 条 trace 事件（每条含 1 个 JSON payload，约 120 B）：

| 方案 | 总耗时 | 备注 |
|---|---|---|
| **异步管道**：producer task → bounded mpsc(1024) → 独立 writer 线程 → 事务批量 500 | **36.06 ms**（3.61 µs/条） | producer 侧 26.9 ms；20 批；单批最大插入 **3 ms**；主循环 ticker 最大停顿 **6 ms**；落库 10000 行 |
| 同步单事务插 10k（主线程） | 16.22 ms（1.62 µs/条） | 会阻塞主循环 16 ms |
| 同步 autocommit 插 10k（主线程） | 133.93 ms（13.4 µs/条） | 阻塞主循环 134 ms，不可接受 |
| 无消费者时 bounded `try_send` 10k（cap 1024） | 0.90 ms（0.09 µs/次） | accepted 1024 / **dropped 8976** |

**结论：可落地。** 异步落库确实不阻塞主循环（ticker gap 6 ms，SQLite 写只在 writer 线程）；但必须用**批量事务 + 单个写线程**，绝不能在事件循环里同步 `INSERT`（即使单事务 16 ms 也会造成可感知抖动）。`try_send` 的丢弃行为已被量化：消费者停摆时丢失率 89.8%，需要监控与背压策略（关键 trace 用 `send().await` 或写入前先落盘队列）。

### 3.7 探针 7 — 节点实例超时 / 取消语义（30s timeout）

代码：`src/bin/t7_timeout.rs` + `t7b_interrupt.rs`，Python：`py/node_long.py`，日志：`logs/70_t7_timeout.log`、`logs/71~74_t7b_*.log`。

**（a）`tokio::time::timeout` / `CancellationToken` 能不能实现"超时后不再等待"？能。**

| 场景 | 实测 |
|---|---|
| `timeout(100ms, spawn_blocking(busy 2s))` | 103.2 ms 判定超时；此时 Python 任务 `done=false` |
| `CancellationToken`（100 ms 后 cancel，`select!` 等待） | 106.1 ms 观测到取消；Python 任务仍 `done=false` |
| 纯 async future 对照 | 105.4 ms（说明计时器本身准确） |

**（b）被放弃的任务会怎样？继续跑，并且有实际代价。**

| 观测 | 实测 |
|---|---|
| `drop(runtime)`（等待 blocking 池关闭） | **1890.7 ms**（≈ Python 任务剩余时间），drop 后任务才完成 → "不等待"在进程/运行时生命周期上是假的 |
| 被放弃的 busy 任务运行期间，事件循环线程的普通 Python 调用延迟 | 37.5 µs → **332.3 µs（8.9×，本次日志）**；孤立运行时曾测到 4.63 ms（**113×**）——GIL 争用波动很大 |
| 是否泄漏线程/资源 | 是：`spawn_blocking` 任务无法抢占，一直占 blocking 线程；若节点死循环，线程永久占用 + 持续偷 GIL |

**（c）能否真正中断 Python 节点？**

| 机制 | 结果 |
|---|---|
| `PyErr_SetInterrupt()`（main thread 上的 Python 代码） | **失败**：跑满 5.0 s，未抛 KeyboardInterrupt；CPython 打印 `OSError: Signal 2 ignored due to race condition` |
| `PyErr_SetInterrupt()`（worker thread 上的 Python 代码，即 `spawn_blocking` 真实场景） | **失败**：跑满 5.0 s，`interrupted=false`，同样打印 signal race condition |
| `PyThreadState_SetAsyncExc(thread_id, PyExc_KeyboardInterrupt)`（worker 线程，CPU-bound 字节码） | **成功**：300 ms 时触发，317 ms 内抛 KeyboardInterrupt，**停止延迟 6 ms**；解释器随后可用、`sys.exc_info()` 干净 |
| 同上，但目标线程阻塞在 `time.sleep(5)` | **只能等它醒来**：5.0 s，停止延迟 **4690 ms**（异步异常只在字节码边界检查，不打断 C 阻塞调用） |

**结论（回答 lead 的问题 3）：**
- 30s 超时**可以实现"编排层不再等待 + 记 timeout 事件 + 向下游传播失败"**，但**不能实现"任务已终止、资源已回收"**。设计必须把 timeout 语义改成前者，并明确写"被放弃的节点可能继续占用 GIL/线程，直到其自然结束"。
- 不要使用 `PyErr_SetInterrupt`/signal 做取消（在本嵌入环境实测无效，且会引入 "signal ignored" 噪声）。
- 若产品确实需要强制终止，只有两条路：
  1. **协作式取消（推荐）**：`NodeContext.cancel_requested` 原子标志 + 文档要求节点在长循环/分片处自检（也可提供 `ctx.check_cancelled()` 抛异常），事件循环 cancel 后节点自行退出；
  2. **`PyThreadState_SetAsyncExc`（unsafe，最后手段，仅限 CPU-bound 字节码）**：需自管 worker 线程与 thread_id，不能在 C 扩展阻塞时生效，且异步异常可能出现在任意字节码处（可能留下不一致状态/锁未释放）。若用于不可信用户代码，建议**进程隔离**而不是线程级强杀。

### 3.8 探针 8 — 单线程事件循环 + 完成事件回传（回答 `&mut` 别名问题）

代码：`src/bin/t8_eventloop.rs`，日志：`logs/80_t8_eventloop.log`。

DAG：`a(0.2s) ∥ b(0.2s) → c(0.05s) → d(0.05s)`，`current_thread` runtime 做事件循环，状态 `HashMap<node_id, NodeState>` **只被事件循环任务持有**；节点任务只捕获 `'static` 数据 + `mpsc::Sender<Event>`：

```
start node=a at_ms=0.0
start node=b at_ms=0.0
finish node=a at_ms=205.8
finish node=b at_ms=205.8
start node=c at_ms=205.9
finish node=c at_ms=257.1
start node=d at_ms=257.1
finish node=d at_ms=310.7
RESULT event_loop_model wall_ms=310.7 ticker_max_gap_ms=6 sequential_if_serial_ms=500.0 parallel_speedup_x=1.61
EVENT_LOOP_DAG_ALL_NODES_DONE=true
```

反例（`alias_fail/`，故意编译失败）——在设计伪代码式的"事件循环里 `tokio::spawn` 直接改 `&mut self`"写法下，编译器直接拒绝：

```
error[E0521]: borrowed data escapes outside of method
   | async fn run(&mut self, ...)
   | tokio::spawn(async move { self.completed.insert(node, 1); });
   | `self` escapes the method body here
   | argument requires that `'1` must outlive `'static`

error[E0382]: use of moved value: `self`
   | value moved here, in previous iteration of loop
```

**结论（回答 lead 的问题 4）：可以，而且这是正确模型。** 用"channel 回传完成事件"替代"共享可变状态"，单线程事件循环可以完全拥有 `HashMap` 状态、避免 `&mut` 别名与 `'static` 冲突；`PyO3` 的 `Python<'_>`/`Py` 守卫本身也不是 `Send`，跨 `await` 持有 GIL 守卫会被编译器拒绝，这是天然的安全边界。配合 `spawn_blocking`，事件循环保持响应（ticker gap 6 ms）且并行分支仍能并行（a/b 同时在 0 ms 启动，1.61× 加速）。

---

## 4. 对详细设计第 3/4/5 章的执行模型建议

### 4.1 第 3 章（执行内核/事件循环）

**必须改：**

1. 事件循环**不得**用 `tokio::spawn` 直接执行同步 Python 调用。落地写法：
   ```
   event loop (current_thread runtime)
     └─ for each ready node: spawn_blocking(node.execute)  // 或 python_executor.submit(node)
          └─ node.execute: 在 spawn_blocking 线程内 Python::attach → 导入模块 → run(inputs, ctx)
          └─ 完成后 tx.send(NodeEvent::Done{node_id, output}) （或 Failed）
     └─ rx.recv().await → 更新 HashMap 状态 → 调度后继节点
   ```
2. 引入**独立的 Python 执行并发上限**（Semaphore）：CPU-bound 节点建议 1；I/O 型可 >1；与 tokio worker 线程数解耦。
3. 事件循环线程与 Python 执行线程分离后，"单线程事件循环"的假设依然成立（探针 8），但**事件循环线程上不允许出现任何同步 Python 调用**（包括 gate、payload 转换里的 Python API、日志里的 Python 回调）。
4. trace/emit 回调用 `try_send` + 丢弃计数；trace writer 用独立线程 + 批量事务（探针 6）。关键事件不要走可丢通道。

### 4.2 第 4 章（节点执行/超时/取消）

**必须改语义：**

1. 超时 = "stop waiting"，事件循环在超时点记录 `node_timeout` 事件、释放调度槽位、继续执行其他分支；**不要**假设 Python 任务已经停止。
2. 节点状态机需增加 `Abandoned/RunningAfterTimeout` 概念（至少内部记账），用于：
   - 统计被放弃任务数量（防止无限累积占满 blocking 池）；
   - 进程退出时说明"仍有 N 个被放弃节点在运行"；`Runtime::drop` 会等待它们（实测 1.89 s），优雅退出需 `shutdown_timeout` 或直接 `shutdown_background` + 进程退出。
3. 取消协议（若要真正终止）：
   - 优先协作式：ctx 暴露 `cancel_requested`（原子 bool）或 `ctx.check_cancelled()`，Python 节点在循环/分片处自检；
   - 极端场景用 `PyThreadState_SetAsyncExc`，但必须限定"仅 CPU-bound 字节码 + 专用 worker 线程 + 不保证状态一致"，并且**不能**用于中断 `time.sleep`/网络/文件等 C 阻塞调用（实测 4.69 s 才停）；
   - 不要依赖 `PyErr_SetInterrupt`/signal。
4. 若要求"30s 必杀且资源必回收"，唯一可靠方案是**进程/解释器隔离**（每节点或每 worker 子解释器+进程），这是比当前设计更重的架构决策，需要单独立项评估。

### 4.3 第 5 章（Python 节点/gate/trace 契约）

1. `run(inputs, ctx) -> dict` / `gate(output, ctx) -> bool`：可直接落地；但需要定义并校验：
   - 返回值必须是 dict（节点）/ 严格 bool（gate，或用 truthiness 并在 schema 写清）；
   - 异常统一转结构化错误（探针 5 已给出实现）；
   - ctx 必须是 `#[pyclass]`（跨语言回调可行，`emit` 3.86 µs/次）。
2. `emit` 签名同步、内部 `try_send` 可行；必须补：丢弃计数指标、事件分类（可丢/不可丢）、容量与丢弃策略文档。
3. trace：事件结构建议固定字段（seq, node_id, attempt, kind, ts, payload），writer 批量 500、事务提交；payload 建议在发起侧序列化（本 spike 用 `String`），避免 DbC 线程再做 Value→JSON。
4. 性能预期要写进文档：Python 节点吞吐 ≈1 核（GIL）；并行分支只对 GIL 释放型工作有效；trace 异步落库 10k 条 ~36 ms（3.6 µs/条）含 JSON payload。

### 4.4 实现注意（PyO3 0.30 API）

- 用 `Python::initialize()` + `Python::attach(...)`（不是旧版 `prepare_freethreaded_python`/`with_gil`）。
- `Bound::cast/cast_into` 取代 `downcast/downcast_into`；`PyErr::value(py)` 返回引用，需要 `.clone()` 才能 `.into_any()`。
- 嵌入二进制不要开 `extension-module` feature；链接 libpython 由 `PYO3_PYTHON` 决定。
- 跨 `await` 不持有 `Python<'_>` / `Bound` / `PyRef`（它们非 Send，编译器会拦）。

---

## 5. 失败项 / 编译错误 / 证据摘要

| 项 | 结果 | 证据 |
|---|---|---|
| 故意反例：`tokio::spawn` 捕获 `&mut self` | 编译失败（预期） | `logs/90_alias_fail_compile.log`：E0521 `borrowed data escapes outside of method`、E0382 `use of moved value: self` |
| `PyErr_SetInterrupt` 中断 main-thread Python busy loop | 失败（5.0 s 未中断，`interrupted=false`） | `logs/71_t7b_interrupt_main.log`：`OSError: Signal 2 ignored due to race condition` |
| `PyErr_SetInterrupt` 中断 worker-thread Python busy loop | 失败（5.0 s 未中断） | `logs/72_t7b_interrupt_worker.log` |
| `PyThreadState_SetAsyncExc` 打断 `time.sleep(5)` | 只能等 5 s（停止延迟 4690 ms） | `logs/74_t7b_interrupt_async_exc_sleep.log` |
| 初版 busy 探针用墙钟截止，无法测 GIL 串行 | 方法错误，已修正为 `time.thread_time()` 截止 | 报告 §3.2 方法说明；修正后数据见 `logs/20_t2_gil.log` |
| crates.io 直连下载 crate 超时 | 环境问题，已用本机代理 + 私有 CARGO_HOME 解决 | `00_env.log`、`scripts/env.sh`、`.cargo/config.toml` |

所有探针退出码均为 0（`PROBE*_OK=1`），唯二"失败"是上面两个 CPython 中断机制（这本身是重要结论），以及故意的编译失败反例。

---

## 6. 测量方法与误差说明

- **ticker 停顿**：同一 runtime 上 5 ms interval 任务记录相邻 tick 间隔的最大值；单线程 runtime 下 ticker 可能被彻底饿死（取消后不再被 poll，最后一个 gap 会漏记），因此实现里额外计算"最后一个 tick 到任务结束时刻"的尾部停顿并取最大值（`effective_gap`）。t2/t8 均用此方法。
- **busy 工作单元**：以 `time.thread_time()`（线程 CPU 时间）为截止，4 份 = 4×0.4 s CPU；若用墙钟截止会得到虚假并行。
- **GIL 争用延迟**（探针 7）波动很大：本次日志 8.9×，孤立运行达 113×；报告给出区间而不是单点。
- 机器为 8 核笔记本，运行时可能存在系统/其他进程负载（曾观察到一次 busy 并发 3.5 s 的离群值），但关键结论的排序（spawn vs spawn_blocking；sleep vs busy ≈1× vs 4×）多次复跑稳定。
- debug 构建不影响 Python 侧 GIL 结论；不影响 timeout/ticker 量级；trace 插入时间受 SQLite/WAL 影响，数量级参考即可。
- 本报告只覆盖 CPython 3.13 **GIL 构建**；free-threaded（3.13t）行为不同，且 PyO3 支持实验性，不能作为设计依据。

---

## 7. 文件清单

```
_spike/
├── Cargo.toml / Cargo.lock
├── .cargo/config.toml              # 本机 crates.io 代理
├── scripts/env.sh                  # CARGO_HOME/PYO3_PYTHON/代理
├── scripts/run_all.sh              # 一键复现全部探针 → logs/
├── src/lib.rs                      # json<->PyObject 转换、EmitCtx、PyErr 结构化
├── src/bin/t1_embed.rs             # 1) 嵌入 + JSON 双向
├── src/bin/t2_gil.rs               # 2) tokio/GIL 执行模型矩阵
├── src/bin/t3_callback.rs          # 3) ctx.emit 回调
├── src/bin/t4_gate.rs              # 4) gate 求值/异常
├── src/bin/t5_errors.rs            # 5) 异常结构化/恢复
├── src/bin/t6_trace.rs             # 6) mpsc + SQLite 吞吐
├── src/bin/t7_timeout.rs           # 7) timeout/取消语义
├── src/bin/t7b_interrupt.rs        # 7b) PyErr_SetInterrupt / async exc
├── src/bin/t8_eventloop.rs         # 8) 单线程事件循环 + 事件回传
├── alias_fail/                     # 故意编译失败反例（&mut 别名）
├── py/*.py                         # Python 测试模块（仅标准库）
├── logs/*.log                      # 全部实测原始输出
└── REPORT.md                       # 本报告
```

依赖版本见 §1；`target/` 与 `.cargo-home/` 为构建缓存（可保留/可删）。
