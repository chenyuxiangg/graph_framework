# 图模型框架

使用有向有环图/有向无环图描述一个任务的带状态的SOP，并通过提供调度与状态管理实现任务自动闭环的系统框架。

## 整体结构

框架分三层，最上层为配置层，该层提供用户图配置的接口，是图模型中唯一用户可见的部分；中间层为组件层，通过5个组件调度图模型、管理SOP状态，其中组件包括：调度器、状态管理器、监控器、派发器、编排器；最下层为内核层，提供图模型操作的所有功能。

### 配置层

每个配置对应一个SOP，配置参数分为四类：全局类、node类、门控类、边类，详解如下：

#### 全局类

定义整个SOP的属性，具体如下：
`name`: SOP的唯一身份标识，多个SOP间不可重复，用字符串表示
`graph_mode`: 图模型选择，支持 ["directed_cycle", "directed_nocycle"] 两个取值，即有向有环图和有向无环图
`tick_period_s`: 调度器轮询周期（秒），必须是 > 0 的数字。**必填，无默认值**——`Orchestrator.run` 在两次 tick 之间 sleep 此时长；周期过短会让 tick 频率超过 worker 子进程写盘速度，gate 评估看不到最新 `exit_cnt`，从而多派一次 worker
`pre_handle`: SOP运行的前置处理，是一个函数名，缺省 `""`。当前组件层**只存不用**——`terminal.py` 只提供 `run_post_handle`，没有任何调用点调 `pre_handle`
`post_handle`: SOP运行的后置处理，是一个函数名，缺省 `""`。SOP 判定完成后由 `state_manager/terminal.py` 的 `run_post_handle` 反射成具体函数执行一次

#### node类

定义SOP中有哪些节点以及给节点绑定agent：
`nodes[i].name`: 节点名，唯一标识该节点，同一SOP内不可重复，用字符串表示
`nodes[i].iport`: 节点的输入端口，用于向节点输入数据，当前仅支持传入文件名，该字段是一个二维数组，iport[i]表示多组输入，只需要满足某一组输入有效时，节点就可以开始运行；iport[i][j]表示第i组输入的第j个文件，当且仅当iport[i]中的所有文件都有效时，才表示iport[i]这组输入有效；
`nodes[i].oport`: 节点的输出端口，用于节点处理完数据后对下一个节点/用户输出数据，当前仅支持输出文件名，该字段是一个二维数组，oport[i]表示多组输出，门控会根据输出是否有效放行某一条边，oport[i]之间也是或的关系，即只要任意一个oport[i]有效，门控就会放行
`nodes[i].attr.is_src`: 标识该节点是否时原节点（入口节点），一个图只能拥有一个src节点
`nodes[i].attr.is_sink`: 标识该节点是否时汇节点（出口节点），一个图只能拥有一个sink节点
`nodes[i].attr.role`: 该节点绑定的Agent，用文件路径表示，路径出存放的时Agent的自述文件
`nodes[i].proc`: 节点内部执行函数名，调度时反射为具体函数；空串表示无内部逻辑（由公共 `proc_bypass` 函数兜底）

#### 门控类

图是一个类似树的结构，或者称为一个状态机，每一时刻只能存在一种输出状态，因此通过门控决定当前状态应该向哪个状态进行迁移：
`gates[i].name`: 门的唯一标识，同一SOP内不可重复，用字符串表示
`gates[i].op`: 门控函数，用字符串表示，实际调度时反射成具体函数
`gates[i].enum_dir`: 门控的出口，用一维数组表示，每个出口关联一条边，关联在edges中绑定（用 `parallel` 扇出时，一个出口可以关联多条边，见下）

#### 边类

节点与节点之间通过边进行通信，有状态转移关系/通信需求的节点才需要建立边：
`edges[i].name`: 边的唯一标识，同一SOP内不可重复，用字符串表示
`edges[i].inode`: 边的输入端，一条边只能有一个输入端，边的方向永远是从输入端指向输出端
`edges[i].onode`: 边的输出端，一条边只能有一个输出端
`edges[i].driver`: 边的驱动方式，有两种取值 ["tick", "manual"],即通过tick自动调度和手动触发，也就是说，当gate通过后可以通过sched自动将当前运行的节点从inode 转移到 onode，或者手动的方式
`edges[i].gate`: 边关联的门，即通过指定门的名字把边挂载到门上
`edges[i].gate_value`: 当门的判断结果为 gate_value 所定义的值时，启动这条边
`edges[i].cmd`: 节点从inode 转移到 onode的方式，当前通过调用命令的方式，该字段时一个字符串，调度时反射为具体函数
`edges[i].parallel`: 缺省 `false`（单条推进，向后兼容）。置 `true` 表示这条边参与并行扇出，语义见下

##### parallel（并行扇出）

调度器的状态机只有一根主线（`state.current_node` 指向唯一节点），扇出靠"一次 tick 内多拉几个 worker"实现：

- **分组（运行时）**：**只按 `inode == state.current_node` 分**。`scheduler/evaluator.py` 的 `select_next_edges` 把当前节点的**全部**出边按 `match_gate` + `should_run_cmd` 过滤成 `matched`，运行时**不按 `(gate, gate_value)` 二次分组**（那个分组键只属于下面两条校验规则）
- **求值**：`matched` 为空 → 无候选；`matched` **全部** `parallel=true` → 整组返回（并行扇出）；否则只返回 `matched[0]`（单条推进）
- **primary**：`edges[0]` 是 transition 目标，即 `state.current_node` 切到 `edges[0].onode`（`edges` 保持 `graph.edges` 的配置顺序，所以 `edges[0]` = 配置里排最前的那条命中边）；其余边的 onode 由 `_dispatch_edges` 拉起 worker，但不改变 `current_node`
- **cmd**：组内每条边的 `cmd` 按 `edges` 顺序依次跑。某条抛 `EdgeCmdError` → 记 `SCH_EV_ERROR` + alarm `EDGE_CMD_ERROR`，本 tick 结束（不 transition / 不 dispatch / 不写盘），已跑过的 cmd 副作用不回滚
- **iport**：只对 primary（`edges[0].onode`）检查进入条件，其余 onode 的 iport 不在本 tick 校验
- **收尾**：后台 worker（primary 之外的 onode）**不受** tick 的 in-flight 同步检查保护——该检查只看 `state.current_node`。主线可以一路推进到下游节点而后台 worker 还在跑；需要等它们汇合时，由用户在 `gate.op` 里自行读产物文件判断

两条校验规则（`kernel/validators.py`）：

- `E_MULTI_MATCH_GATE`（error）已被放宽：同一 `(gate, gate_value)` 挂 ≥2 条边，**仅当全组 `parallel=true` 时放行**；否则照旧报错。分组键是 `(gate, gate_value)`（跨 inode）
- `E_PARALLEL_MIXED`（error）：同一匹配组里部分 `parallel=true`、部分非 `parallel`。分组键是 `(inode, gate, gate_value)`（先按 inode 再按匹配键）

> 混合组同时命中两条规则，而 `Graph.build` 只抛**第一条** error（`validate` 里 `_check_gate_refs_and_values` 排在 `_check_parallel_mixed` 前），所以 `load_graph` 报出来的永远是 `E_MULTI_MATCH_GATE`。`E_PARALLEL_MIXED` 只在**绕过 `Graph.build`、直接调 `validate()`** 拿完整列表时可见——**CLI `validate` 子命令走不通这条路**：它先调 `load_graph`，而 `Graph.build` 抛错即退出，永远到不了 `validate()`。运行时的 `select_next_edges` fallback 到"只取首条"只是防御性兜底，混合组在加载期已被拦下。

> ⚠️ **两条校验规则都按 `(gate, gate_value)` 分组，运行时却不分组——扇出会在跨门时静默退化**。实测：给 `config/test_for_complex_graph.json` 的 `fetcher` 再挂一个 `other_gate`（`gate_value=true`）的非 parallel 边，`validate()` 返回 `[]`（两条规则都不触发，加载通过），但两个门在同一 tick 都返回 true 时 `select_next_edges` 的 `matched` 混进了那条非 parallel 边，`all(...)` 为假 → 只返回 `fetcher_to_worker_a` 一条，扇出丢失且无任何告警。**同一个 inode 上想并行推进的边，必须在同一个 `(gate, gate_value)` 组内、且该组不能与别的非 parallel 命中边共处同一 tick。**

配置示例——`fetcher` 通过同一个 gate 同时扇出到 3 个 worker。下面是 `config/test_for_complex_graph.json` 中相关部分的**节选**：

> ⚠️ 这段是**节选，不是完整配置，不能单独跑**。它只保留了 1 个 gate（原文件 5 个）和 4 条边（原文件 10 条），`name` / `graph_mode` / `tick_period_s` / `nodes` 均省略。**照抄这段会得到一个能 `load_graph` 通过、`validate()` 返回空列表、却在第一次扇出后永久卡住的图**——7 个节点里有 6 个没有出边，`run()` 没有 `max_ticks` 上限，之后每个 tick 都只发 `SCH_EV_EDGE_REJECTED` 且不产生任何 alarm。想跑通请直接用 `config/test_for_complex_graph.json` 原文件。

```json
{
  "gates": [
    {"name": "fetcher_raw_gate", "op": "tfc_fetcher_raw_gate", "enum_dir": [true, false]}
  ],
  "edges": [
    {"name": "fetcher_loop", "inode": "fetcher", "onode": "fetcher",
     "driver": "tick", "gate": "fetcher_raw_gate", "gate_value": false,
     "cmd": "tfc_fetcher_loop_cmd"},
    {"name": "fetcher_to_worker_a", "inode": "fetcher", "onode": "worker_a",
     "driver": "tick", "gate": "fetcher_raw_gate", "gate_value": true,
     "parallel": true, "cmd": "tfc_fetcher_to_a_cmd"},
    {"name": "fetcher_to_worker_b", "inode": "fetcher", "onode": "worker_b",
     "driver": "tick", "gate": "fetcher_raw_gate", "gate_value": true,
     "parallel": true, "cmd": "tfc_fetcher_to_b_cmd"},
    {"name": "fetcher_to_worker_c", "inode": "fetcher", "onode": "worker_c",
     "driver": "tick", "gate": "fetcher_raw_gate", "gate_value": true,
     "parallel": true, "cmd": "tfc_fetcher_to_c_cmd"}
  ]
}
```

`gate_value=true` 的三条边全标了 `parallel`，`gate_value=false` 的回环边没标——**`parallel` 按"整条命中边集合"判定，不要求同一个节点的所有出边都标**。这条配置跑起来的效果：`fetcher_raw_gate` 返回 `true` 时一次 tick 拉起 `worker_a` / `worker_b` / `worker_c` 三个 worker，`state.current_node` 落到 `worker_a`（primary），下游汇合交给 `all_workers_done_gate` 在 `gate.op` 里判断。

`parallel` 由 `loader.py` 严格校验类型：非 `bool` 直接抛 `GraphLoadError`，定位 `edges[i].parallel`（`"yes"` 这种字符串会在校验器之前就被挡下）。四种配置的实测结果：

| 配置 | 结果 |
|---|---|
| 3 条边全 `parallel: true` | 加载通过，`validate()` 返回空列表 |
| 3 条边都不写 `parallel`（默认 `false`） | `E_MULTI_MATCH_GATE`（`path=gates[0].enum_dir`） |
| 3 条边里只改 1 条为 `true` | `E_MULTI_MATCH_GATE`（`validate()` 另附 `E_PARALLEL_MIXED`） |
| 任一条写 `parallel: "yes"` | `GraphLoadError`，`path=edges[i].parallel` |

### 组件层

组件层由调度器、状态管理器、监控器、派发器、编排器五个模块组成。组件层上接配置层 JSON，下用内核层原语（loader / state / ports / traversal / registry），向调用方（hermes chat / 人工 / 测试）暴露统一 API。

#### 模块布局

```
scripts/graph_base/src/
├── components/
│   ├── orchestrator.py              # 顶层组装：create_instance / tick / run / trigger_edge
│   ├── scheduler/
│   │   ├── core.py                  # Scheduler.tick（+ _sync_check_inflight / _select_ready_edges /
│   │   │                            #   _check_iport / _run_edge_cmds / _advance / _dispatch_edges
│   │   │                            #   私有阶段方法 + _TickAbort）
│   │   │                            #   / find_ready_next_node / trigger_edge
│   │   ├── evaluator.py             # evaluate_gates / match_gate / should_run_cmd /
│   │   │                            #   select_next_edge / select_next_edges / run_edge_cmd
│   │   └── cycle.py                 # CYCLE_THRESHOLD + detect_cycle_progress（按 exit_cnt 判超阈值）
│   ├── state_manager/
│   │   ├── core.py                  # StateManager 公开方法
│   │   ├── schema.py                # State dataclass
│   │   ├── ports.py                 # iport / oport 二维 OR-of-AND 记录
│   │   └── terminal.py              # is_sop_done / run_post_handle
│   ├── monitor/
│   │   ├── core.py                  # emit / reap_stale_claims / detect_progress
│   │   ├── events.py                # 事件常量（SCH_EV_*）+ level 映射
│   │   └── logger.py                # zlog system logger 包装
│   └── dispatcher/
│       ├── base.py                  # Dispatcher Protocol + DispatchResult
│       └── process.py               # ProcessDispatcher（setsid 拉子进程）
└── worker/
    ├── __init__.py                  # proc_bypass 注册 + 自动发现所有 SOP 子包
    ├── _common.py                   # record_lifecycle（worker 侧 enter/exit 计数的锁协议）
    └── <sop_name>/
        └── <node_name>_worker.py    # 每个 node 一个 worker 模块，含 worker_run(args)
```

#### 核心概念

调度器对一条边的推进分三关：

1. **gate 求值**：调 `gate.op(base_dir, state)` 拿返回值，与 `edge.gate_value` 比较。不匹配则该边不放行，cmd 不执行（零副作用）。
2. **cmd 执行**：gate 通过后调 `cmd(args)` 跑副作用动作，写下一节点就绪态。
3. **driver 模式**：决定 cmd 何时被触发。`tick` = scheduler 在 gate 通过当 tick 调 cmd；`manual` = 必须外部调 `trigger_edge(edge_name, approver)` 写 `triggered_edges`，下次 tick 走 cmd。

`parallel=true` 的边命中时，第 1 关上命中的不再是 1 条而是一组，第 2、3 关对组内每条边各执行一次；状态机主线只认 `edges[0]`。详见配置层的 `parallel` 小节。

**Node.proc** 是节点内部执行函数，注册到 kernel registry；空串表示无内部逻辑，由公共 `proc_bypass` 兜底。proc 与 `pre_handle / post_handle` 独立，三者不互相替代。

#### state_manager

`State` 是不可变 dataclass（`frozen=True`）：

| 字段 | 类型 | 含义 |
|---|---|---|
| `sop_name` | `str` | SOP 名，与 `Graph.name` 同源 |
| `instance_id` | `str` | 实例 id，`new_instance_id()` 生成的 12 字符 hex |
| `graph_name` | `str` | 图配置名，落盘后可用于回溯 |
| `current_node` | `str \| None` | 当前正在执行的节点；`None` = 未启动 |
| `history` | `tuple[tuple[str, str], ...]` | 已完成节点轨迹，`[(node_name, iso8601_ts), ...]` |
| `triggered_edges` | `Mapping[str, str]` | manual 边外部 trigger 记录，`edge_name -> approver` |
| `enter_cnt` | `Mapping[str, int]` | per-node，worker 进入 `node.proc` 的次数（`record_enter` 写） |
| `exit_cnt` | `Mapping[str, int]` | per-node，worker 完成 `node.proc` 的次数（`record_exit` 写） |
| `last_output` | `Mapping[str, tuple[str, ...]]` | 节点最近产出文件（debug 用，同时是 `is_sop_done` 的判据） |

`enter_cnt` / `exit_cnt` 是 cycle 计数的唯一来源——两者合起来既表达"worker 进出了几次"，也表达"当前节点有没有 in-flight worker"：`enter_cnt[n] != exit_cnt[n]` 即该节点仍有 worker 在跑（正常 wait 或 leak）。正常状态下二者相等。

修改路径（`StateManager` 静态方法）：`create / read / write / transition / rollback / record_completion / trigger_edge / claim / record_enter / record_exit`。纯函数（只返回新 State、不碰 state 文件）：`transition / record_completion / record_enter / record_exit`；自带读写的：`create / trigger_edge / claim`。`rollback` 是第三类——返回新 State，但会调 `kernel.clear_claim` 写盘，而 `clear_claim` 内部自取 `StateLock`（见内核层 state 小节），**已持锁时调 `rollback` 不会死锁，但会静默失败**：`StateLock` 轮询到 `timeout`（默认 5.0s）后抛 `TimeoutError`，而 `rollback` 把 `clear_claim` 包在 `except Exception: pass` 里，于是停顿约 5s 后照常返回一个 `State`，**claim 字段实际没被清掉**，调用方看不到任何失败信号。

`rollback` 规则：`current_node` 回退到 `history` 中该节点的前驱（找不到则置 `None`）、清 `triggered_edges`、清 claim、`os.unlink` 删除 `last_output[node_name]` 列出的**绝对路径**文件（相对路径跳过）、清该项 `last_output`、保留 `enter_cnt` / `exit_cnt` 与 `history`（并在末尾追加一条该节点记录）。

`is_sop_done(graph, state)`：三个条件全满足——`current_node` 非空且指向 `is_sink=true` 的节点；若该 sink 节点 `oport` 非空，还要求 `last_output[current_node]` 有产物（worker 已 `record_completion` 落盘）。sink 的 `oport` 为空时不等产物，`current_node` 一到 sink 即 done。

#### scheduler

`Scheduler.tick(sop_name, instance_id) -> list[str]` 单实例调度流程，返回本 tick 的 alarm 字符串列表。`state.json` 不存在时直接返回 `["STATE_NOT_FOUND"]`。进入路径后**全程持一把 `StateLock`**（`core.py` 里 `with StateLock(state_file):` 包住除"文件存在性检查"以外的全部逻辑）：

1. `read` —— `StateManager.read` 读 state.json，发 `SCH_EV_TICK(phase=enter)`
2. `is_sop_done` 短路 —— 已完成则发 `SCH_EV_SOP_DONE`、清 `_skip_count`、直接返回空 alarms
3. `_sync_check_inflight` —— in-flight 同步检查（见下）
4. `_select_ready_edges` —— `evaluate_gates` + `select_next_edges` 选边；无命中 → `SCH_EV_EDGE_REJECTED` 结束本 tick
5. `_check_iport` —— 仅对 `edges[0].onode` 查进入条件；不满足 → `SCH_EV_AWAITING_IPORT` 结束本 tick
6. `_run_edge_cmds` —— 按 `edges` 顺序跑每条命中边的 `edge.cmd`
7. `_advance` —— `StateManager.transition` 到 `edges[0].onode` + `detect_cycle_progress` 阈值检查
8. `_dispatch_edges` —— 为每条命中边的 onode 拉 worker
9. `StateManager.write` 落盘，发 `SCH_EV_TICK(phase=exit)`

阶段 3/4/5/6 各自的提前终止由私有异常 `_TickAbort(alarms)` 承载（在锁内捕获后原样返回）——不用 `(value, error)` 元组返回，调用方物理上不可能漏检 error 槽。

> `core.py` 里这些阶段的注释编号是 0-8：`locate / read / sync_check / select_edges / iport_ready / run_cmd / advance / dispatch / write`。比上表少 1，是因为它把"取锁前判文件存在"和"取锁后读 state"各算一个阶段，而上表把它们合进了第 1、2 步。

**in-flight 同步检查**（阶段 3）：`state.enter_cnt[current_node] != state.exit_cnt[current_node]` 说明当前节点的 worker 还在跑，本 tick 不推进（发 `SCH_EV_AWAITING_IPORT(reason=in_flight_worker)`）。`Scheduler` 维护 `self._skip_count` 计连续跳过次数，超过 `MAX_INFLIGHT_SKIPS = 10` 发 `SCH_EV_TIMEOUT(what=in_flight_leak)` 并追加 alarm `WORKER_INFLIGHT_LEAK`，计数清零避免连续累积。`current_node` 为空或计数相等时计数清零。

**cycle 阈值**（阶段 7）：`scheduler/cycle.py` 的 `CYCLE_THRESHOLD = 100`。`detect_cycle_progress` 调内核 `cycle_check` 取所有环上的节点，用 **`exit_cnt`**（不是 `enter_cnt`）统计最大次数——只有 worker 真完成的次数算进度，in-flight 不算。超阈值发 `SCH_EV_TIMEOUT(what=cycle_threshold)` 并追加 alarm `CYCLE_OVER_THRESHOLD`。

`find_ready_next_node(state, base_dir) -> tuple[list[Edge], str | None]`：返回 `(edges, primary)`，`primary = edges[0].onode`。返回 `([], None)` 表示无候选（`current_node` 为空 / 无匹配边 / 全部 manual 未 trigger）。`gate.op` 异常 → 抛 `GateEvalError`，由 `tick` 的阶段方法 `_select_ready_edges` 捕获并转成 `SCH_EV_ERROR` + alarm `GATE_EVAL_ERROR`。它**不判断 iport**。

`trigger_edge(state_file, edge_name, approver)`：供外部触发 manual 边，三个参数都必填（`approver` 空串时 `StateManager.trigger_edge` 记 `"anonymous"`）。`Scheduler` **不提供 `claim`，也不提供 `dispatch`**——认领能力只在 `StateManager.claim` / `kernel.claim` 上，且这两处都没有被 `tick` / `Orchestrator` 调用（并发 tick 的互斥实际由 `StateLock` 兜底）；拉 worker 的能力由 `dispatcher` 组件提供，`tick` 内部走私有的 `_dispatch_if_any`（由 `_dispatch_edges` 逐条调用），并额外发 `SCH_EV_NODE_START` 事件。调度器不提供 `tick_all`；多实例由上层循环 + 多 Orchestrator 实例负责。

#### monitor

事件常量统一 `SCH_EV_` 前缀（18 个），通过 zlog 单 `system` logger（`level=DEBUG`）输出；`_Event.level` 决定实际调 logger 的哪个方法（debug / info / warning / error）。关键事件：

| 事件 | 触发时机 | level |
|---|---|---|
| `SCH_EV_TICK` | tick 入口 + 出口（每次 tick 2 条） | debug |
| `SCH_EV_AWAITING_IPORT` | in-flight worker 未退出 / primary 节点 iport 未满足 | debug |
| `SCH_EV_TIMEOUT` | in-flight leak（`what=in_flight_leak`）/ cycle 超阈值（`what=cycle_threshold`） | error |
| `SCH_EV_ERROR` | gate.op / cmd / dispatcher / worker 异常、state 文件缺失 | error |
| `SCH_EV_EDGE_ADMITTED` | gate 通过，cmd 跑 | debug |
| `SCH_EV_EDGE_REJECTED` | 无命中边，cmd 不跑 | info |
| `SCH_EV_NODE_START` | 每拉起一个 worker 发一条（parallel 扇出时一次 tick 发 N 条） | info |
| `SCH_EV_SOP_DONE` | `is_sop_done` 短路，`run` 退出前 | info |
| `SCH_EV_MANUAL_PENDING` | manual 边 gate 通过但长期未 trigger | warning |
| `SCH_EV_CLAIM_RECLAIM` | `reap_stale_claims` 扫到失效 claim | warning |

> ⚠️ **`StateLock` 拿不到锁时不发任何事件。** `Scheduler.tick` 进入 `with StateLock(...)` 之前只发过一条「state 文件缺失」的 `SCH_EV_ERROR`；锁等满 `timeout`（默认 5.0s）后直接抛 `TimeoutError`，没人 catch，会一路穿出 `tick` 和 `Orchestrator.run`。也就是说 `SCH_EV_TIMEOUT` **只在 in-flight leak 和 cycle 超阈值两处发**，排查锁超时崩溃时不要指望在监控事件里找到它——现场只有 `TimeoutError` 的 traceback。

`Monitor` 公开方法：`emit(event, **fields)` / `reap_stale_claims(root, sop_name, stale_after=300.0)` / `detect_progress(graph, state_file, base_dir)`。

#### dispatcher + worker

`Dispatcher` 是 Protocol；当前唯一实现 `ProcessDispatcher`，按 `worker_cmd` 模板拉子进程。`worker_cmd` 用 `str.format_map` 替换，支持 6 个占位符：`{python} {sop_name} {instance_id} {node_name} {proc} {base_dir}`（`{python}` 取 `sys.executable`）。未知占位符由 `_SafeFormat.__missing__` 原样保留。派发时额外注入子进程环境变量 `GRAPH_BASE_STATE_ROOT / GRAPH_BASE_INSTANCE_ID / GRAPH_BASE_SOP_NAME`；`PYTHONPATH` 未被继承时才补 `os.getcwd()`，保证子进程能 `import src` / `zlog`。

默认模板：
```python
DEFAULT_WORKER_CMD = (
    "{python} -m src.worker.{sop_name}.{node_name}_worker "
    "--sop {sop_name} --instance {instance_id} "
    "--node {node_name} --proc {proc} --base-dir {base_dir}"
)
```

worker 集合目录 `src/worker/<sop_name>/` 下每个 node 一个 `<node_name>_worker.py`，内含 `worker_run(args) -> int`，都委托给 `src/worker/_common.py` 的 `record_lifecycle`：

1. `load_graph` 读配置
2. 持 `StateLock`：`read` → `record_enter`（`enter_cnt[node] += 1`）→ `write`
3. **释放锁**跑 proc（proc 可能耗时长，不该阻塞别的 worker），取返回的 `output_files`
4. 重新持 `StateLock`：重读最新 state → `record_completion`（写 `last_output`）→ `record_exit`（`exit_cnt[node] += 1`）→ `write`

锁范围刻意避开 proc 调用本身——锁只保护 `enter_cnt / exit_cnt / last_output` 这种共享 metadata。`parallel` 扇出场景下多个 worker 子进程同时跑，不带锁的话 read-modify-write 竞态会让某个 worker 的计数丢失。

公共 `proc_bypass` 注册到 kernel registry；proc 字段空串时 `record_lifecycle` 自动回退到它。⚠️ 当前 `proc_bypass(args)` 是**位置参数**签名（`src/worker/__init__.py:23`），而 `record_lifecycle` 按 `proc_fn(base_dir=, state=, graph=, node_name=)` 关键字调用（`src/worker/_common.py:53-55`）——proc 为空串的节点会在这一步抛 `TypeError`。现有 3 份配置的 12 个节点 proc 全部非空，这条路径没有被任何测试覆盖。

#### orchestrator + CLI

`Orchestrator(graph, root, dispatcher=None, monitor=None)` 封装五组件（`dispatcher` 缺省 `ProcessDispatcher()`，`monitor` 缺省 `Monitor(get_monitor())`）。属性：`graph` / `scheduler`。方法：

- `create_instance(base_dir=None) -> str`：新建实例，返回 `instance_id`（`current_node` 初始化为 src 节点）。`base_dir` 参数当前未被函数体使用
- `tick(sop_name, instance_id) -> list[str]`：单次 tick，透传 Scheduler
- `run(sop_name, instance_id) -> list[str]`：**阻塞**循环 `tick` + `sleep(graph.tick_period_s)`，直到 `is_sop_done` 为真；退出前调一次 `run_post_handle`，返回全程累计的 alarms。**没有 `max_ticks` 上限**——靠 `CYCLE_OVER_THRESHOLD` alarm 和外部超时兜底
- `trigger_edge(sop_name, instance_id, edge_name, approver) -> State`

CLI 子命令（`scripts/graph_base_cli.py`）：`tick / create / list / show / record-completion / trigger-edge / rollback / validate / help`。无 `tick-all`。`show` 输出 `sop_name / instance_id / graph_name / current_node / history_len / triggered_edges / enter_cnt / exit_cnt`。

#### 非目标

- ❌ 不实现 InProcessDispatcher / QueueDispatcher（仅 ProcessDispatcher）
- ❌ 不支持热加载 Graph
- ❌ 不跨 SOP 编排
- ❌ 不持久化 validators.Issue（监控器自己 log）
- ❌ 不自动 trigger manual 边
- ❌ 不分 alarm / pipeline 两条 logger
- ❌ 不调用 `pre_handle`（字段已加载进 `Graph`，但组件层没有调用点）
- ❌ 不给 `run()` 设 tick 次数上限（`CYCLE_OVER_THRESHOLD` 只告警不退出）


### 内核层

内核层是图模型框架的最下层，对上层组件（调度器、状态管理器、监控器、派发器、编排器）提供图模型操作的全部原语。内核层不解释 SOP 业务语义，不调度、不发事件、不写日志。

#### 设计原则

1. **数据与逻辑解耦**：`node.py / gate.py / edge.py` 是纯 dataclass（无方法）；`graph.py` 只有 `Graph.build()` 这个静态工厂 + `cached_property` 索引，无行为逻辑；其余模块是对这些数据的纯函数 / 原语。
2. **薄内核**：所有"做决定"的逻辑上移到组件层。
3. **配置即真相**：JSON 配置是内核建模的唯一来源——内核只提供 `load_graph`（JSON → Graph），**不提供反向序列化**（无 `to_json` / `asdict` 导出路径）。
4. **多实例支持**：同一个 SOP 配置可并行运行多个实例（多个并发需求、多个并发布版），每个实例有独立 state 文件、独立锁。
5. **原子与可重入**：所有写操作走锁，锁粒度 = 单个实例（一把锁 = 一个 state 文件 = 一个 SOP 实例）。
6. **可单测**：每个函数都自洽（参数是 dataclass，不依赖全局），pytest 用 fixture 喂数据。

#### 模块布局

```
scripts/graph_base/src/
├── __init__.py
└── kernel/
    ├── __init__.py        # 公共导出
    ├── graph.py           # Graph / GraphMode（数据 + 工厂方法 build）
    ├── node.py            # Node（数据）
    ├── gate.py            # Gate（数据）
    ├── edge.py            # Edge / Driver（数据）
    ├── loader.py          # load_graph(path, existing_sop_names=None) -> Graph；JSON → 对象 + 字段类型校验
    ├── validators.py      # validate(graph, existing_sop_names=None) -> list[Issue]；跨字段一致性
    ├── traversal.py       # 图遍历与查询：successors / predecessors / edges_from /
    │                      #   edges_via_gate / match_gate / cycle_check / topo_sort
    ├── ports.py           # 文件端口语义：iport_satisfied / oport_produced / port_file_path / expand_*_paths
    ├── state.py           # StateLock / read_state / write_state / claim / instance 路径生成
    └── registry.py        # 反射接口：register_op / resolve_op / register_cmd / resolve_cmd —— 只返回可调用对象
```

不引入数据库；不引入第三方库；`scripts/zlog/` 仅由组件层使用，内核层不依赖它。

#### 数据模型

四个 dataclass 文件构成纯数据层：

`graph.py` 定义 `GraphMode` 枚举（`DIRECTED_CYCLE` / `DIRECTED_NOCYCLE`）与 `Graph` 聚合根。`Graph` 用 `frozen=True`，字段为 `name / graph_mode / pre_handle / post_handle / nodes / gates / edges / tick_period_s`。`Graph.build()` 静态工厂方法是唯一公开的构造路径，负责类型校验与跨字段一致性校验（调用 `validators.validate`，遇到 error 级 Issue 抛 `GraphBuildError`，warning 静默）。四个派生索引（`node_index / gate_index / edge_by_inode / edge_by_gate`）以 `@cached_property` 实现：首次访问时按 `nodes / gates / edges` 即时计算并缓存，从根本上不可能出现陈旧值。`Graph` 不暴露任何运行时修改入口。

`node.py` 定义两个 dataclass：`NodeAttr`（`is_src / is_sink / role`，对应 JSON `attr` 子对象）与 `Node`（`name / iport / oport / attr: NodeAttr / proc`，顶层 `proc` 字段允许空串）。iport / oport 都是二维 tuple（元组的元组），语义见下表：

| 字段 | 组内 | 组间 |
|---|---|---|
| `iport[i][j]` | AND：组内所有文件必须都存在，组才"满足" | OR：任一组满足，节点即可开始 |
| `oport[i][j]` | AND：组内所有文件都存在，组才"产出" | OR：任一组产出，门控即可放行 |

> 注：src 节点也允许有 iport（如 planer 需读入 `input.md`）。

`gate.py` 定义 `Gate`：`name / op / enum_dir`。`op` 是函数名，由 `registry.resolve_op` 解析；`enum_dir` 是一维 tuple，与 `edge.gate_value` 对应。

`edge.py` 定义 `Driver` 枚举（`TICK` / `MANUAL`）与 `Edge`：`name / inode / onode / driver / gate / gate_value / cmd / parallel`。`gate` 引用 `gates[].name`；`gate_value` 命中值；`cmd` 是函数名；`parallel` 缺省 `False`，语义见配置层的 `parallel` 小节。

#### loader 与 validators

`loader.py` 负责 `load_graph(path, existing_sop_names=None) -> Graph`：读 JSON、字段类型校验、调 `Graph.build()`。失败抛 `GraphLoadError(message, path)`——异常属性名是 `path`（不是 `path_in_json`），值是字符串如 `"nodes[2].name"`，定位用 `"edges[3].parallel"` 这种形式。loader 不接收 `base_dir`（运行时概念，挂在 `ports.py` 求值时注入）。跨 SOP 唯一性由调用方注入——`load_graph` 接收 `existing_sop_names` 并原样透传给 `validate`，缺省 `None` 时 `E_DUP_GLOBAL` 整项跳过（loader 自己不维护全局 SOP 名表）；跨字段一致性由 `Graph.build()` 内部的 `validators.validate()` 执行——`GraphBuildError` 会被 loader 包成 `GraphLoadError`，`path` 透传。`tick_period_s` 是 loader 强制的必填字段（缺省直接抛 `GraphLoadError`），无隐式默认值。

`validators.py` 负责跨字段一致性，返回 `list[Issue]`。`Issue` 是 dataclass，字段为 `code / level / message / path`，`level` 取 `Literal["debug", "info", "warning", "error"]` 四级（当前主要使用 warning / error）。校验项：

- `E_DUP_NODE_NAME` / `E_DUP_GATE_NAME` / `E_DUP_EDGE_NAME`（error）
- `E_DUP_GLOBAL`：SOP `name` 跨图唯一（error，仅当调用方传入 `existing_sop_names`）
- `E_NO_SRC`：必须有且仅有一个 `is_src=true` 的节点（error）
- `E_MULTI_SINK`：必须有且仅有一个 `is_sink=true` 的节点（error）
- `E_BAD_EDGE_ENDPOINT`：edge.inode / onode 必须指向存在的 node（error）
- `E_BAD_GATE_REF`：edge.gate 必须指向存在的 gate（error）
- `E_GATE_VALUE_NOT_IN_ENUM`：edge.gate_value 必须在 gate.enum_dir 中（error）
- `E_MULTI_MATCH_GATE`：同一 `(gate, gate_value)` 挂 ≥2 条边（error）——**已放宽**：整组边全部 `parallel=true` 时放行
- `E_PARALLEL_MIXED`：同一 inode 下同一 `(gate, gate_value)` 匹配组里 `parallel` 与非 `parallel` 混合（error）——只在直接调 `validate()` 时可见，`Graph.build` 撞上混合组时先抛 `E_MULTI_MATCH_GATE`
- `W_CYCLE_ON_NOCYCLE`：`graph_mode=directed_nocycle` 时发现环（warning）

`validators.py` 不抛异常，只返回列表；由组件层决定是否阻断加载。error 阻断（`Graph.build` 转成 `GraphBuildError`），warning 仅记录。`directed_cycle` 模式下检测到的环不进入 Issue 列表——由 `cycle_check` 直接返回给调用者，监控器组件决定是否记录。

#### traversal

`traversal.py` 是纯函数集：

| 函数 | 用途 |
|---|---|
| `successors(graph, node_name) -> tuple[str, ...]` | 出边终点（去重、保序） |
| `predecessors(graph, node_name) -> tuple[str, ...]` | 入边起点 |
| `edges_from(graph, node_name) -> tuple[Edge, ...]` | 指定节点的所有出边 |
| `edges_via_gate(graph, gate_name) -> tuple[Edge, ...]` | 挂载在某门上的边 |
| `match_gate(graph, gate_name, enum_value) -> Edge \| None` | enum 命中后应该触发的边（0 或 1）；≥2 抛 `AmbiguousMatchError` |
| `cycle_check(graph) -> tuple[tuple[str, ...], ...]` | 返回**所有**环（节点名列表的元组），DFS 三色标记法；返回空 tuple 表示无环 |
| `topo_sort(graph) -> tuple[str, ...]` | 无环模式下的节点顺序；环存在时抛 `CyclicGraphError` |

> **同名不同函数**：`kernel.traversal.match_gate(graph, gate_name, enum_value) -> Edge \| None` 只在遍历层做单边定位，**不在调度主路径上**——`Scheduler` 全程不调它。调度器用的是 `components/scheduler/evaluator.py` 里另一个 `match_gate(edge, gate_values) -> bool`（纯谓词），配合 `select_next_edges` 返回整组命中边。写代码时注意别 import 错。

#### ports

`ports.py` 是文件端口语义的具象。设计原则：**只判断文件是否存在，不解析内容**。

- `iport_satisfied(node, base_dir) -> tuple[int, ...]`：返回当前满足条件的 iport 组索引（OR-of-AND）。空 tuple 表示无输入就绪。
- `oport_produced(node, base_dir) -> tuple[int, ...]`：返回已产出的 oport 组索引（OR）。
- `port_file_path(spec, base_dir) -> Path`：解析 `"doc/spec.md"` → `<base_dir>/doc/spec.md`。spec 必须是非空相对路径（绝对路径抛 `PortPathError`），解析后必须仍在 base_dir 之下（防御 `../` 越界）。
- `expand_iport_paths(node, base_dir)` / `expand_oport_paths(node, base_dir)`：展开所有路径（与原二维 tuple 同形状，元素为 `Path`，不检查存在性），便于监控器列出节点在等哪些 / 会产生哪些文件。

空端口组（`iport[i] == []`）一律视为"不满足"——`Scheduler._iport_ready` 对空 iport 的节点直接返回 True（无输入要求），与 `_satisfied_group_indices` 的空组语义是两套判断，别混。

`base_dir` 在这里注入，不在 loader。同一份 graph 对象可被不同实例用不同 base_dir 跑——多实例支持的具体落地。

#### state

`state.py` 提供状态原子读写 + 多实例路径生成：

- `state_dir(root, sop_name)` / `state_path(root, sop_name, instance_id)`：路径生成。`root` 缺省时读 `GRAPH_BASE_STATE_ROOT` 环境变量，默认 `/var/lib/graph_base`；`state_dir` 自动 `mkdir(parents=True, exist_ok=True)`。
- `new_instance_id() -> str`：`uuid.uuid4().hex[:12]`，12 字符 hex。
- `StateLock(state_file, timeout=5.0, stale_after=300.0)`：基于 `fcntl.flock` 的互斥锁，with 语法使用。锁文件是 `<state_file>.lock`（`state_file` 同目录），`os.open` 新 fd 后 `flock(LOCK_EX|LOCK_NB)` 轮询重试到超时。同进程内对同一文件再开一把**不是死锁，而是轮询到 `timeout`（默认 5.0s）后抛 `TimeoutError`**（flock 不跨同进程重入，新 fd 永远拿不到锁）——`Scheduler.tick` 与 `worker/_common.record_lifecycle` 都各自只开一把，任何阶段方法都不许再取锁。
- `read_state(state_file) -> dict` / `write_state(state_file, payload)`：原子读写（临时文件 + `os.replace`，失败清理临时文件）。文件不存在或 JSON 损坏时 `read_state` 返回 `{}`。
- `claim(state_file, pid, owner, stale_after=300.0) -> bool` / `clear_claim(state_file)` / `pid_alive(pid)`：认领与回收。`claim` 与 `clear_claim` 自己内部会取 `StateLock`（所以调方**不能**已经持着同一文件的锁）。

**多实例关键点**：state 路径 = `<root>/<sop_name>/<instance_id>.json`，每个实例独占；锁粒度 = 单实例（不是单 SOP），所以两个并发实例互不阻塞。

**state schema**：内核不定义 schema。`write_state` 接 `dict`，由状态管理器组件决定字段含义（例如 `{"claim_pid": ..., "current_node": ..., "history": [...]}`）。内核只保证：原子写入、并发互斥、stale 回收。

**claim stale 策略**：claim 无效 = `pid_alive(pid) is False` **或** `(now - claim.ts) > stale_after`（任一条件满足即无效）。进程崩了立即可被接管（不依赖 stale_after 到期）；进程还在但卡了超过 stale_after 也可被接管。`stale_after` 是保险阀：正常情况下进程退出时会调 `clear_claim`，但万一没调（比如 kill -9），stale_after 保证不会永远卡死。

#### registry

`registry.py` 提供反射接口：

- `_OP_REGISTRY: dict[str, Callable]` / `_CMD_REGISTRY: dict[str, Callable]`：进程内单例。
- `_LOCK = threading.Lock()`：守护所有读（resolve）/ 写（register）/ 清（reset）操作。普通 Lock 而非 RLock，避免重入死锁。
- `register_op(name, fn)` / `resolve_op(name)` / `register_cmd(name, fn)` / `resolve_cmd(name)` / `reset_registry()`。
- 异常：`register_*` 同名重复注册抛 `DuplicateRegistrationError`；`resolve_*` 找不到抛 `RegistryKeyError`。

注册在进程启动期完成（`src/worker/__init__.py` 用 `pkgutil` 自动 import 所有 SOP 子包触发 `register_*`）；运行时主要走 resolve，锁竞争极少。内核层只维护名字 → 可调用对象的映射；不调用这些函数。组件层（调度器）拿到函数后再传 ctx 执行——`gate.op` / `edge.cmd` / `node.proc` 统一按 `fn(base_dir=, state=, graph=, gate=)` 或 `fn(base_dir=, state=, graph=, edge=)` 或 `fn(base_dir=, state=, graph=, node_name=)` 关键字调用。

#### 非目标

- ❌ 不调度（那是 调度器 组件）
- ❌ 不解释门控返回值（那是组件调用 `resolve_op` 后做的事）
- ❌ 不写日志、不告警（那是 监控器 组件，本框架通过 zlog）
- ❌ 不读 pre_handle / post_handle / cmd 的实际函数体
- ❌ 不维护历史轨迹、`enter_cnt` / `exit_cnt` 计数、SOP 完成判定（属于状态管理器关心的字段，但写什么由组件决定）
- ❌ 不跨 SOP 实例共享锁——一把锁 = 一个 state 文件 = 一个实例
- ❌ 不定义 state file schema——内核只管原子性，不管字段含义（`claim_pid` / `claim_owner` / `claim_ts` 是内核 `claim` 自己写的唯一三个字段）