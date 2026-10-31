# DR-006 · W3a0 人工评审设计冻结：human lifecycle + decide 自动接续 + `human_decisions` + trace v5

> **状态**：设计冻结（W3a 实现依据）；评审编号 **REV-078**（评审记录由独立评审人撰写，不在本分支）。
> **日期**：2026-10-31 · **作者**：docs-dev · **分支**：`docs/docs-human-design`
> **关联**：[`DR-004-v02-scope-and-waves.md`](DR-004-v02-scope-and-waves.md) §3 W3a0 / §5 D3 / §3 W3a DoD / §6.6、[`REV-063-docs-v02-kickoff.md`](../reviews/REV-063-docs-v02-kickoff.md) C12/C13/S10、[`DR-005-w2a-defer-gate-freeze.md`](DR-005-w2a-defer-gate-freeze.md)（decide 的 rejected→rework 复用 defer/attempt-per-activation）、[`issues.md`](../issues.md)、spec v1.2 分册 00/01/02/03/04/06/07/08
> **前置事实**：v0.1.0 已发布（tag `v0.1.0` @ `5d43c58`）；M0–M6 已交付；W1 SOP v1（REV-064–066）、W2a0/W2a（REV-067–073）、W2b（REV-074–077）已合并到 `main`（`1ed37ab` 时点）；DR-004 用户 A1–A7 已确认（含「人工评审、图级有限返工、扇出上限、动态 N 另起」）。
> **效力**：本 DR 是 W3a（trace / kernel / cli / e2e）与 W3a-docs 的**唯一语义依据**；与既有 spec 冲突处以本 DR + 本批 spec 冻结写入为准（冲突点见 §12）。**默认行为（无 `builtin.human` 节点的图）逐字节不变**；W3a0 **只写设计**，不写 as-built、不改代码。

---

## 0. 摘要（一页）

| # | 冻结内容 | 落点 |
|---|---|---|
| H1 | **触发模型**：人工审批节点 = `python_node` 保留符号 `{"module":"builtin","function":"human"}`（canonical `builtin.human`）；DSL schema/R1–R16 **不变**；kernel 编译期映射为内置 `CompiledHandler::Human`，**不启动 worker、不 import 用户 Python**；R14 的 python_node 格式校验照常、但该符号**无 import/签名/NODE_WORKER_IMPORT 路径** | 02 §2.5；04 §4.4.5；01 §1.1.1；本 DR §2 |
| H2 | **生命周期**：输入就绪时 **`Pending → WaitingHuman`**（不经 `Ready/Running`）；run 保持 `Running`；`WaitingHuman` 计入非静默（不终止、不触发 deadlock）；`Pause/Resume` 不改变等待；run 取消/冷峻失败 → `WaitingHuman → Cancelled`；`timeout_ms` 为**决策等待期限**（缺省无期限；到期 → `TimedOut` + `NODE_TIMEOUT` + run Failed） | 00 §0.3/§0.6；01 §1.2.3/§1.2.4；03 §3.4.5/§3.9.1/§3.9.3/§3.10 |
| H3 | **决策应用**：decide 行经 run 的 decision consumer 提交 `RunControl::HumanDecision`；kernel 校验「实例存在且为当前实例且 `status=WaitingHuman`」→ `WaitingHuman → Completed(reason="human_decided")`；实例 output = `{"decision":"approved"\|"rejected","reason":<string\|null>}`；随后按普通 publish 求值出边 gate。**拒绝 = 正常 Completed**（不隐式 Failed），去向完全交给图（approved/rejected 分支 + W2a defer 返工）。decide 行 → **自动接续**；**W3a 不提供 `resume` 子命令**（durable resume 属 W3b） | 01 §1.2.4/§1.5；03 §3.4.5；本 DR §4 |
| H4 | **`human_decisions` 表（trace schema v5）+ C13 写连接模型**：唯一键 `(run_id, instance_id)`（重复插入冲突 → exit 5，非幂等成功）；`decision_id` = UUIDv7 仅审计；`consumed_at` 仅 run consumer 写；`TraceWriter` **不写**该表；`decide` 独立连接单事务 INSERT（`busy_timeout=5000ms`、BUSY 重试 3）；run consumer `pending()` RO 轮询 + `mark_consumed` RW 短事务；WAL；decide 提交后立即可见；崩溃未消费行归 W3b | 06 §6.2.14/§6.6/§6.7；本 DR §5 |
| H5 | **trace schema v5**：`TRACE_SCHEMA_VERSION 4→5`；`VACUUM INTO` 备份 → **重建 `instances`/`transitions` 表**（status CHECK 加 `WaitingHuman`；数据复制、索引重建）→ 新建 `human_decisions` + 索引 → 单事务 + `schema_migrations`；失败恢复备份并 exit 4；只读 `trace`/`replay` 对 `<5` 拒绝；v3→v4→v5 链可回放 | 06 §6.2.2/§6.2.5/§6.2.7/§6.2.14/§6.7/§6.10.4 |
| H6 | **kernel API**：`InstanceStatus::WaitingHuman` 进入可产生集合；`CompiledHandler::Human`；`RunControl::HumanDecision{instance_id,decision,approver,reason,decision_id,decided_at}`（新增 variant，source-breaking → 08 §8.7 备注）；`TraceSink::human_decided(HumanDecisionRecord{...,applied,reject_reason})`（新方法、默认 no-op，仅 CLI bridge 实现）；状态机矩阵新增三迁移 | 00 §0.3；01 §1.1.1/§1.2.3/§1.5；04 §4.1.2；本 DR §7 |
| H7 | **CLI**：新增 `zgraph decide <run_id> --instance <id> --decision approved\|rejected --approver <id> [--reason <text>] [--json] [--db <path>]`；退出码 **0/2/1/5/4/70**（**新增 `EXIT_CONFLICT=5`**）；`--json` 成功/失败形状；`run` 新增 decision consumer（200ms RO 轮询 → control → 等 `human_decided` 确认 → 标 `consumed_at` + stderr 实时行）；`trace` 增 human 决策摘要与 `--json` NDJSON `{"kind":"HumanDecision",...}` 行（**不**写入 `events.kind`） | 06 §6.10.1/§6.10.4/§6.10.5/§6.10.6/§6.10.8；本 DR §7 |
| H8 | **常量** `HUMAN_DECISION_POLL_MS = 200`（默认；00 §0.5、03 §3.4.5） | 00 §0.5 |
| H9 | **AC-16 a–g 设计冻结**（approve 真链路 / reject→W2a 返工 / 冲突·幂等·退出码 / 决策期限 / trace v4→v5 迁移 / decide×run 并发 / 默认 golden 不变）；测试名与计数由 W3a 实现后 W3a-docs 回写 | 07 AC-16；本 DR §10 |

**一句话**：把「人工审批」做成**图内一等的等待态**——`builtin.human` 节点在输入就绪时挂起为 `WaitingHuman`（run 继续存活、非静默），外部 `zgraph decide` 写一张**独立决策表**，run 的消费者把决策送回内核，内核把它当作一次**一次性完成**（`WaitingHuman→Completed`）后走普通门控/返工语义；决定「批准后去哪、驳回后去哪」的**始终是图**，框架只保证等待、决策、审计与可回放。

---

## 1. 背景、目标与非目标

### 1.1 背景

- v0.1.0/W2a 的失败语义是冷峻的：节点要么执行成功，要么 Crashed/TimedOut → run Failed；`WaitingHuman` 枚举在 00 §0.3 **保留但从不产生**（决策记录-01 D4）。SOP v1/SOP v1.1 只能把「评审结论」表达成 Python 节点输出 + gate，**无法让人在运行时真正做决定**。
- DR-004 §3 W3a 要求「人工停等 + decide/resume」：run 进程在 `WaitingHuman` 时保持存活，轮询 trace DB 的 `human_decisions` 表；CLI `decide` 插入决策；run 消费后恢复。§5 D3 已冻结：trace DB 作为跨进程控制通道、WAL 只读轮询、唯一键 `(run_id, instance_id)`、`decision_id` 仅审计、写连接分表（C13）、不做 HTTP/socket、run 退出后不可 decide（W3b）。
- W3a0 的任务是把上述决策收敛为**可实现、可验收、可回放**的契约，并冻结 W3a 的模块边界（trace/kernel/cli/e2e/docs），避免实现分支各自发明语义（尤其：状态机、退出码、写者模型、trace v5 迁移）。

### 1.2 目标

1. **人工停等可用**：图里放一个 `builtin.human` 节点，run 在输入就绪后挂起等待；外部 `decide` 后 run 自动接续（无 `resume` 子命令）。
2. **默认语义零变化**：不含 `builtin.human` 的图行为、`graph_hash`、`validate` 结果、trace 事件 payload 与 v0.1.0/W2a/W2b 完全一致（H3/§8）。
3. **决策可审计、可并发**：决策行 1:1 绑定实例（唯一键），`decide` 与 `run` 跨进程并发安全（WAL + busy_timeout + 短事务）；重复/冲突有确定退出码。
4. **可观测**：决策行进 `human_decisions` 表（不进 `events.kind`）；`trace` 人类输出/`--json` 可读；schema v5 forward-only 迁移、可回放。
5. **拒绝去向交给图**：approve/reject 都只改变「实例完成 + output 形状」，后续路由由既有 gate/边语义（含 W2a defer 返工）决定——框架**不内建**评审拒绝矩阵。

### 1.3 非目标（W3a0 明确）

- **W3b durable resume**：进程崩溃/重启后的恢复、`resume` 子命令、未消费决策的崩溃后处置、Running 实例/worker 恢复。
- **HTTP/socket/UI**：不做网络控制通道；只做 trace DB 通道 + CLI。
- **决策表单 schema / prompt 渲染**：human 节点不定义表单、不校验业务 schema；等待什么由图的输入端口/上游数据决定。
- **多待决决策排序/队列/超时策略叠加**：同一实例至多一次决策；不做队列/优先级。
- **Harness `NeedHumanInput`**、worker 协议改动、DSL schema/新 NodeType（human 用保留 python_node 符号，不新增节点类型）。
- 不做决策的「人工重放」：L1/L2 回放只回放已发生事实，不重新 soliciting 人工输入。

### 1.4 术语

| 术语 | 定义 |
|---|---|
| human 节点 | `python_node` 且 `config.module="builtin"`、`config.function="human"` 的节点（canonical 符号 `builtin.human`） |
| WaitingHuman | 实例状态：输入已就绪、等待人工决策；**非终态、非活跃执行** |
| decision | `approved` / `rejected` 二值，一行 `human_decisions` |
| decision consumer | run 进程内任务：RO 轮询未消费决策 → 提交 `RunControl::HumanDecision` → 确认后 RW 标 `consumed_at` |
| consumed_at | 决策行被 run 消费并处理（applied 或 rejected）的审计时间；**仅 run consumer 写** |
| applied | kernel 是否真的因该决策改变了实例状态（`false` = 状态已变/不匹配，仅审计） |
| 决策期限 | human 节点 `config.timeout_ms`（若声明）从进入 `WaitingHuman` 起计的等待上限；缺省无期限 |

---

## 2. 触发模型冻结（H1）

### 2.1 保留符号（DSL 不变）

| 项 | 冻结值 |
|---|---|
| 节点类型 | `python_node`（**不新增** `NodeType`） |
| `config` | 恰好 `{"module":"builtin","function":"human"}`；可附带 `timeout_ms` / `retry`（`exec_class` 允许出现但**忽略**） |
| canonical 符号 | `builtin.human`（`module.function`，格式合法；R4/R14 照常通过） |
| DSL schema/白名单 | **不变**（`PythonNodeConfig` 键集合已覆盖 `module/function/exec_class/timeout_ms/retry`） |
| `graph_hash` | **不变**：节点投影仍按 DSL 原文（`config` 全量参与既有规则）；无新增字段 |
| 默认行为 | 无 `builtin.human` 节点的图：编译产物/handler/运行期行为完全不变（§8） |

- **R14 例外（冻结）**：`builtin.human` 仍做 `module/function` **格式校验**（不合法仍报 `DSL_SYMBOL_NOT_FOUND`），但**不做 import/签名校验**、**不产生 `NODE_WORKER_IMPORT`**——它从不是用户 Python 符号。该例外只适用于精确匹配 `module == "builtin" && function == "human"` 的节点；其它 `builtin.*` 仍是普通 python_node（不存在隐含保留前缀）。
- **与 gate canonical 无冲突**：gate 内置 op 只有 `check_output`/`compare`/`manual`（`builtin.check_output` 等）；`human` 不是 gate op，不参与 `DSL_SYMBOL_CONFLICT` 判定（该冲突只比较「gate canonical vs python_node 符号」的 `builtin.<op>`）。`builtin.human` 是**节点符号**，不得出现在 `gate.function`。
- **编译期映射**：`compile_node` 见 `PythonNodeConfig` 且符号精确为 `builtin.human` → `CompiledHandler::Human`（不构造 Python handler、不注册 worker 需要的 ResolvedSymbols、不做 import）；其它 `python_node` 仍走 `Python{module,function}`。
- **运行期不启动 worker**：human 实例从不进入 `Running`、从不派发执行任务（I8 与 D4 无关）；`killed_workers` 不计入。

### 2.2 人类上下文（输入快照）

- 等待所需上下文 = **该节点声明的输入端口**（与普通节点一致：`trigger=all/any`、required/optional、null 补齐）。实例创建时把触发它的 buffer 快照写入 `instances.input`（06 §6.2.5），e2e 用 `--record=full`/`instances.input` 断言「人工看到的是这一代上游产物」。
- human 节点**没有额外的 prompt/schema 输入**；应用评审所需字段全部来自图的上游输出（SOP 的 `spec/result/review` 等）。
- `--record=full` 的 L2 记录：human 实例输出按其 `(node_id, attempt)` 记录（与其它节点一致），重放不重新要求人工决策。

### 2.3 期限（`timeout_ms`）

| 项 | 冻结值 |
|---|---|
| 语义 | **决策等待期限**：自实例进入 `WaitingHuman` 起计时（不是从派发起） |
| 缺省 | 节点未声明 `config.timeout_ms` → **无期限**（可无限等待；run 保持存活）——这是 human 节点相对普通节点 `DEFAULT_NODE_TIMEOUT_MS=180_000` 的**显式例外**（普通节点默认仍然是 180s 实例超时） |
| 到期 | 实例 `WaitingHuman → TimedOut`，`NodeError::Timeout{elapsed_ms}` → `NODE_TIMEOUT` → run `Failed`（冷峻，与既有超时同口径）；无 worker 可 kill，`killed_workers` **不增加** |
| 实现依据 | 使用编译产物既有 `declared_timeout_ms: Option<u64>` 区分「未声明（无期限）」与「显式声明」；普通节点调度语义不变 |
| retry | `retry.max_attempts` 对 human 节点仍生效（供 W2a defer 重入/attempt 语义）；不因超时自动重试 |

### 2.4 `exec_class` 与 worker 相关字段

- `config.exec_class` 对 human 节点**无意义**：解析允许、编译忽略；文档注明（04 §4.4.5）。
- 不进入 Python worker 生命周期：无 hello/import/call、无 `NODE_WORKER_*`、无进程组 kill；`instances.worker_versions` 聚合不包含 human。

---

## 3. 生命周期与状态机冻结（H2）

### 3.1 `InstanceStatus` 扩展

00 §0.3 的 `InstanceStatus` 增 **`WaitingHuman`**（非终态、非执行态）；`is_terminal()` 仍为 `Completed/Crashed/TimedOut/Cancelled/Skipped`；新增 `is_active()` 语义（实现细节）必须至少覆盖 `Pending/Ready/Running/WaitingHuman`，用于终止判定（§3.6）。

```rust
pub enum InstanceStatus {
    Pending, Ready, Running, WaitingHuman,   // W3a0：WaitingHuman 进入可产生集合（非终态）
    Completed, Crashed, TimedOut, Cancelled, Skipped,
}
```

### 3.2 状态机迁移矩阵（01 §1.2.3 增行）

| From \ To | WaitingHuman | Completed | Crashed | TimedOut | Cancelled | 触发 |
|---|---|---|---|---|---|---|
| **Pending** | **输入就绪**（human 专用旁路；不经 Ready/Running） | — | — | — | run 取消/失败 | 输入就绪事件 |
| **WaitingHuman** | — | **决策 applied**（`reason="human_decided"`） | — | **决策期限到期** | run 取消/冷峻级联 | decide / deadline / run 收尾 |
| **终态** | — | — | — | — | — | — |

**迁移副作用（冻结）**：

| 迁移 | 副作用（顺序冻结） |
|---|---|
| `Pending→WaitingHuman` | 写 `TransitionRecord`（reason=`human_waiting`）；记 `started_at`（等待起点）；若声明 `timeout_ms` 则登记 deadline；从 ready/active 视角转为等待态（不派发） |
| `WaitingHuman→Completed` | 写 `TransitionRecord`（reason=`human_decided`）；写 `output={"decision","reason"}`；记 `finished_at`；清 deadline；触发普通 publish（gate 求值在 off-loop 的等价路径：human 输出在其完成时求值，见 §4.3） |
| `WaitingHuman→TimedOut` | 写 `TransitionRecord` + `NodeTimedOut` 事件；`NodeError::Timeout`；冷峻 fail_run（与既有超时同口径，§3.4） |
| `WaitingHuman→Cancelled` | 写 `TransitionRecord`；run 取消/冷峻失败收尾（不等待、不决策） |

- **I1 口径（冻结）**：`WaitingHuman` 实例**从不执行 `run()`**，`WaitingHuman→Completed` 是**唯一一次完成**（不重跑、不重入）；「实例单次执行」对 human 节点退化为「单次完成」，与 I1 不矛盾。重入（W2a defer）产生**新实例/新 attempt**，不是对旧实例的重复执行。
- **`attempt`**：human 实例 attempt 语义与普通节点一致（0 起；defer 重入 attempt+1）。同一 attempt 至多一次决策（唯一键 `(run_id, instance_id)` 保证）；重入产生新 instance_id → 可再次决策。
- **非法迁移**：任一未列迁移 → `RUN_ILLEGAL_TRANSITION` → run Failed（与既有同口径）。

### 3.3 run 视角

- run `status` **保持 `Running`**（不新增 run 状态、不进入 Paused，除非用户显式 Pause）；`WaitingHuman` 是**非静默**状态。
- `Pause`/`Resume` 不改变等待：Pause 只停派发与 ready_queue；等待中的实例保持 `WaitingHuman`；Resume 不自动完成决策。
- **取消**：`Cancel` → 所有非终态实例（含 `WaitingHuman`）转 `Cancelled` + kill 在飞 worker（无 human worker 可 kill）；`RunFinished{Cancelled}` 恰一次。
- **冷峻失败**：其它实例 Crashed/TimedOut/human 期限到期 → `fail_run`，`WaitingHuman` 一并转 `Cancelled`（I9）。
- **run 终态后 decide**：拒绝（exit 1，§4.5）；decision consumer 退出；未消费行留待 W3b（§5.5）。

### 3.4 超时与看门狗

| 机制 | 冻结 |
|---|---|
| 实例期限 | `WaitingHuman` 使用 **决策等待期限**（§2.3）；普通节点仍用 `DEFAULT_NODE_TIMEOUT_MS`/声明值（不变） |
| deadline 判定 | 与既有实例 deadline 同源（事件循环侧 deadline 表）；到期 → `TimedOut` → `NodeError::Timeout` → `NODE_TIMEOUT` → `fail_run`；`killed_workers` 不增（无 worker） |
| watchdog「静默 + Pending」 | human 实例不是 `Pending`；纯 `WaitingHuman` **不得**触发 `RUN_DEADLOCK`（它是有意的等待，不是死锁） |
| watchdog「defer receipt 目标未实例化」 | 与 W2a 语义不变；human 节点可作为 receipt 目标并正常实例化（等待态不算“未实例化”） |
| pause 期间 deadline | Pause **不暂停**决策期限计时（真实经过时间；到期仍 TimedOut）；文档明确（E-2） |

### 3.5 终止判定（03 §3.10 修订）

```text
is_quiescent(run) =
   no instance with status ∈ {Pending, Ready, Running, WaitingHuman}   # W3a0 增 WaitingHuman
   AND ready_queue.is_empty()
   AND no pending deliveries in flight
```

- 只要存在 `WaitingHuman` → **不静默**、不 `complete_run`；run 继续等 decide 或期限。
- 静默（无 WaitingHuman）后的收尾顺序与 W2a 完全一致（defer receipt blocked 判定 → complete_run）。
- `run_output` 聚合保持不变（human 节点输出参与 end 聚合，01 §1.9）。

---

## 4. 决策应用与拒绝矩阵（H3）

### 4.1 端到端链路（冻结）

```text
decide CLI ──(独立 RW 连接, 单事务 INSERT)──▶ human_decisions 行（consumed_at=NULL）
                                              │
run 进程 decision consumer（RO 轮询 200ms）──▶ pending(run_id)
                                              │  （仅 run 存活时）
                                              ▼
                         RunControl::HumanDecision{...}  ──▶ kernel 事件循环
                                              │
      kernel 校验：实例存在 ∧ 是 node_current ∧ status=WaitingHuman
          ├─ 通过 → WaitingHuman→Completed(reason="human_decided")
          │         output={decision,reason}；求值出边 gate → 普通 publish
          │         → TraceSink::human_decided(applied=true, reject_reason=None)
          └─ 不通过 → 状态不变
                    → TraceSink::human_decided(applied=false, reject_reason=<code>)
                                              │
consumer 收到 human_decided 确认 ──(独立 RW 连接, 短事务 UPDATE)──▶ consumed_at=<now>
                                              │
                              stderr 实时行（applied/警告）+ trace 可查
```

- **决定去哪由图决定**：kernel 只完成「实例 Completed + output 形状 + publish」；approved/rejected 的路由完全由出边 gate（如 `check_output{path:"decision",equals:"approved"}`）与边类型（settle/defer）决定。
- **一次性**：同一实例的 `human_decisions` 至多一行（唯一键）；kernel 对重复/竞态决策只回报 `applied=false`，不改状态。

### 4.2 决策行状态机

```text
INSERT（decide；consumed_at=NULL） ──▶ PENDING
PENDING ──consumer 提交并收到 human_decided──▶ CONSUMED（consumed_at=<now>；applied=true/false）
PENDING ──run 已终态/进程崩溃──▶ 保持 PENDING（W3b 处置；W3a 不清理、不重放）
```

- `consumed_at` 一旦写入不再更新（审计只一次）；`applied=false` 也标 consumed（避免 consumer 反复重试同一行）。
- `decide` **不写** `consumed_at`；`TraceWriter` **不写** 该表。

### 4.3 human 节点的 gate 求值时机

- human 实例完成后，其出边 gate 的求值与普通节点**同构**：以 `output={"decision","reason"}` 作为 gate 输入（`check_output` 读取 `decision`/`reason` 路径），在完成应用前由事件循环内的内置 gate 求值（human 无执行任务，**不引入** off-loop 执行器；内置 gate 是 Rust 纯函数，符合 I7/I8）。
- 求值结果、`GateResultRecord`、settle/defer/attempt-per-activation 的后续完全复用 W2a/W2b 语义；**不新增** gate 类型、不改 `manual` gate（`manual` 是**门控**，与 human 节点无关；`manual` 语义不变，见 04 §4.6）。
- 因此 **reject 去向矩阵 = 图的路由矩阵**（冻结）：approve/reject 都只是 `decision` 值；「终止或进入 rework」由设计者在图上用 approved 边 + rejected 边表达；rework 用 W2a `defer` 回边 + attempt-per-activation（DR-005 §3.2）。

### 4.4 拒绝矩阵（冻结）

| # | 场景 | kernel 行为 | instance/run 结果 | trace |
|---|---|---|---|---|
| 1 | `approved` 且实例为当前 `WaitingHuman` | 迁 `Completed`；output `{decision:"approved",reason:<...>}`；publish | 实例 Completed；run 继续（可能 Completed） | Transition + GateResult（出边） |
| 2 | `rejected` 且实例为当前 `WaitingHuman` | 迁 `Completed`；output `{decision:"rejected",reason:<...>}`；publish | **不隐式 Failed**；由 rejected 分支决定终止/返工 | 同上 |
| 3 | 实例已非 `WaitingHuman`（已决策/已超时/已取消/非当前） | 不改状态；`applied=false` | 实例/run 不变 | `human_decided{applied:false}` 确认（CLI 可观测） |
| 4 | 决策期限到期（无 decide） | `TimedOut` + `NodeError::Timeout` | run Failed（`NODE_TIMEOUT`，冷峻） | Transition + NodeTimedOut |
| 5 | run 已终态 / 不存在 | decide 预检拒绝（exit 1）；consumer 已退出 | 不变 | — |
| 6 | 同实例重复 decide | 第二次 INSERT 冲突（exit 5）；不会产生第二行 | 不变 | — |
| 7 | 并发 decide（两个进程同实例） | 唯一键 + SQLite 事务：**恰一 succeed（0），其余 exit 5** | 不变（仅一行） | — |
| 8 | decide 在 Paused run 上 | decide 允许（预检 status ∈ {Running,Paused}）；consumer 提交、kernel 应用状态迁移与 publish；**不恢复派发**（Pause 语义不变） | 实例 Completed；run 仍 Paused | Transition（dispatch 端等 Resume） |

> 行 8 是**冻结解释**：DR-004 §5 D3「run 存活轮询」与 03 §3.8「Pause 停派发、投递仍应用」的最小交集。若用户希望 decide 顺带 Resume，属产品选择，不在 W3a。

### 4.5 decide 前置校验与拒绝码（单 RO 快照）

| 校验（同一快照） | 失败 → CLI 退出码 | `--json.code` |
|---|---|---|
| 参数完整/decision ∈ {approved,rejected}/approver 非空 | 2 | `CLI_USAGE` |
| run 存在且 status ∈ {Running,Paused} | 1 | `HUMAN_RUN_NOT_ACTIVE` |
| instance 存在 | 1 | `HUMAN_INSTANCE_NOT_FOUND` |
| instance 属于该 run 且 status=WaitingHuman | 5 | `HUMAN_INSTANCE_NOT_WAITING` |
| 该实例尚无决策行（唯一键） | 5 | `HUMAN_DECISION_EXISTS` |
| DB 打不开/迁移未完成/busy 重试耗尽 | 4 | `HUMAN_DB_ERROR` |
| panic 被捕获 | 70 | `HUMAN_INTERNAL` |

- 预检与 INSERT 在**同一决定阶段**：先 RO 预检、再独立 RW 单事务 INSERT（`busy_timeout=5000ms`，`SQLITE_BUSY` 重试 3 次）；唯一键冲突（并发窗口）→ exit 5（`HUMAN_DECISION_EXISTS`）。
- `HUMAN_*` 是 **CLI 命名空间**（decide 的 `--json.code`），**不写入** `runs.error_code`、不新增领域错误码（00 §0.4 不变）。

> **E-11 裁决（Lead 2026-10-31）**：「instance 不存在」与「run 不存在/已终态」同属**目标不存在** → exit **1**（`HUMAN_INSTANCE_NOT_FOUND`）；「instance 存在但 status≠WaitingHuman」→ exit **5**（`HUMAN_INSTANCE_NOT_WAITING`）。实现与 06 §6.10.5 按此执行。

### 4.6 decide 自动接续裁决（DR-004 W3a0「二选一」）

- **冻结：decide 自动接续**。`zgraph decide` 成功后，run（存活进程）自动把该实例推进完成并继续执行；**不存在**「先 decide、再 `resume`」的两步操作。
- **W3a 不提供 `resume` 子命令**：run 退出（正常/崩溃）后无法再决定；durable resume（`W3b` 可选）另行设计。
- `run` 进程在纯 `WaitingHuman` 时**保持存活**（stdout/stderr 可观察等待行），直到 decide/期限/取消。

---

## 5. `human_decisions` 表与写连接模型（H4，C13）

### 5.1 表定义（trace schema v5 新增；DDL 权威在 06 §6.2.14）

```sql
CREATE TABLE IF NOT EXISTS human_decisions (
  run_id       TEXT NOT NULL REFERENCES runs(run_id) ON DELETE CASCADE,
  instance_id  TEXT NOT NULL REFERENCES instances(instance_id) ON DELETE CASCADE,
  decision     TEXT NOT NULL CHECK (decision IN ('approved','rejected')),
  approver     TEXT NOT NULL,                 -- 必填非空（CLI 校验）
  reason       TEXT,                          -- 可空
  decision_id  TEXT NOT NULL UNIQUE,          -- UUIDv7，仅审计
  decided_at   TEXT NOT NULL,                 -- UTC RFC3339 毫秒 Z
  consumed_at  TEXT,                          -- 仅 run consumer 写；NULL = 未消费
  PRIMARY KEY (run_id, instance_id)
);
CREATE INDEX IF NOT EXISTS idx_human_decisions_pending
  ON human_decisions(run_id, consumed_at) WHERE consumed_at IS NULL;
```

- **唯一键 = `(run_id, instance_id)`**：同一实例至多一行；重复插入 → SQLite 约束冲突 → decide exit 5（**不是**幂等成功）。
- **`decision_id` 仅审计**：UUIDv7，不参与唯一/幂等键；不用于去重（重复 decide 由唯一键拒绝，而非 decision_id）。
- **FK**：`run_id → runs`、`instance_id → instances`，`ON DELETE CASCADE`；清理 run 时级联（06 §6.9）。
- **`decided_at` 口径**：CLI 写入时刻（UTC RFC3339 毫秒 Z，与 trace 时间戳同口径）；`consumed_at` 由 run consumer 写入时刻。

### 5.2 写者模型（C13；与 06 §6.6 的关系）

| 角色 | 连接 | 权限 | 事务/超时 | 写什么 |
|---|---|---|---|---|
| `TraceWriter`（run 内） | 既有单一写线程 | RW | 批量事务 | **事件表（`events`/`instances`/`transitions`/…）；不写 `human_decisions`** |
| `decide` CLI | 独立连接 | RW | **单事务 INSERT**；`busy_timeout=5000ms`；`SQLITE_BUSY` 重试 3 次 | `human_decisions` 一行 |
| run decision consumer | 独立 RW 短连接 | RW | 短事务 UPDATE；`busy_timeout=5000ms` | **仅** `UPDATE human_decisions SET consumed_at=? WHERE run_id=? AND instance_id=? AND consumed_at IS NULL` |
| run decision poller | 独立只读连接 | RO | `busy_timeout=1000ms` | 读 `pending(run_id)` |

- **WAL**：三连接并发读写允许；`decide` 提交后立即可见（consumer 下一轮轮询可读到）。
- **无锁优先级/无跨表事务**：`decide` 不与 `TraceWriter` 共用事务；`human_decisions` 与事件表之间**不要求原子**（决策是外部输入，事件由内核在应用时写）。
- 与 06 §6.6 既有「trace 事件唯一写连接」不冲突：该不变量只约束**事件表**；`human_decisions` 为 C13 冻结的显式第二/第三写者。
- **崩溃语义**：run 进程崩溃 → 未消费行保持 `consumed_at=NULL`；已消费行保留；处置归 **W3b**（W3a 不清理、不重放、不删除）。

### 5.3 `pending` 与幂等

- `pending(run_id)`：`SELECT ... FROM human_decisions WHERE run_id=? AND consumed_at IS NULL ORDER BY decided_at, decision_id`（索引 `idx_human_decisions_pending`；确定性排序）。
- consumer 对同一行可能重复提交（例如确认丢失前重试）：kernel 侧因实例已非 `WaitingHuman` 返回 `applied=false`；consumer 收到确认后标 consumed → **至多一次状态变更**（幂等）。
- **`consumed_at` 更新失败**（busy 耗尽/IO）：consumer 记 stderr 警告并保留 `NULL`（下轮重试；run 结束仍未成功 → 留给 W3b；不阻塞 run 收尾）。**不**因此 fail run。

### 5.4 decide 的 schema 版本前提

- `decide` **不迁移** DB：打开 DB 后若 `user_version < 5` → exit 4（`HUMAN_DB_ERROR`，stderr 提示先由 `zgraph run` 迁移）；`user_version > 5` → `STORE_SCHEMA_TOO_NEW` exit 4。
- 理由：迁移是 run/写者职责（06 §6.7 既有口径）；decide 只是决策写入者，避免多进程同时迁移。**E-3 边界**：用户对从未跑过的库执行 decide 必然 exit 4/1（run 不存在），语义安全。

---

## 6. trace schema v5 与 API（H5）

### 6.1 版本与迁移（06 §6.2.2/§6.7 权威）

- `TRACE_SCHEMA_VERSION = 5`；新库单事务建 v5 DDL + `schema_migrations` **1/2/3/4/5** 五行 + `PRAGMA user_version = 5`（无备份）。
- 旧库逐版本循环（v1→v2→v3→v4→v5），每步 `VACUUM INTO '.bak.v{old}-{ts}'`（回写旧 `user_version`、校验）；**单版本单事务**；失败删 `-wal/-shm` → 备份覆盖 → exit 4。
- 只读 `trace`/`replay`：`user_version < 5` → 拒绝且**不迁移**（`unsupported schema version=N, expected 5`，exit 4）；`>5` → `STORE_SCHEMA_TOO_NEW`。

### 6.2 v4→v5 迁移步骤（冻结）

```text
1) VACUUM INTO '<db>.bak.v4-{ts}'；回写备份 user_version=4 并校验
2) 单事务：
   a) 重建 instances（SQLite 12 步法）：
      CREATE TABLE instances_v5 ( ...同列... status CHECK IN (...,'WaitingHuman',...) ... );
      INSERT INTO instances_v5 SELECT <显式列清单> FROM instances;   -- 列清单显式，禁止 SELECT *
      DROP TABLE instances; ALTER TABLE instances_v5 RENAME TO instances;
      重建 3 个索引（idx_instances_run_node / run_status / error_code 部分索引）
   b) 重建 transitions（同法；from_status/to_status CHECK 各加 'WaitingHuman'；重建 3 个索引）
   c) CREATE TABLE human_decisions（06 §6.2.14）；CREATE INDEX idx_human_decisions_pending
   d) INSERT INTO schema_migrations(version,description,applied_at) VALUES(5,...)
   e) PRAGMA user_version = 5
3) 提交；失败 → 恢复备份（删 -wal/-shm）→ exit 4
```

- **迁移不改历史数据**：`instances`/`transitions` 行逐列复制；`events` 不重建、不重写 payload；v4 库中的旧 run 迁移后 `trace`/`replay` 输出与迁移前逐字节一致（除 `user_version`/`schema_migrations` 元数据）。
- **FK 顺序**：先重建被引用表（instances），再建 `human_decisions`（其 FK 指向新 instances）。`runs` 不重建。
- **CHECK 重建理由**：SQLite 的 CHECK 不能 `ALTER`；为使 `WaitingHuman` 能落库必须重建两表（06 §6.7「删改列必须新建表+拷贝+改名」的既有规则）。
- **v3→v5 链**：v3→v4 沿用 W2a 迁移（REV-067/070），再 v4→v5；AC-16 e 断言链式迁移后回放一致。

### 6.3 trace API（06 §6.12 风格；W3a 实现依据）

| API | 用途 |
|---|---|
| `HumanDecisionRow { run_id, instance_id, decision, approver, reason, decision_id, decided_at, consumed_at }` | 读取形状 |
| `TraceReader::human_decisions(run_id) -> Vec<HumanDecisionRow>`（RO） | `trace`/e2e 查询；按 `decided_at, decision_id` 升序 |
| `DecisionStore::insert(NewHumanDecision) -> Result<...>` | decide 用；单事务 INSERT；唯一键冲突映射为 `Conflict` |
| `DecisionStore::precheck(run_id, instance_id)`（RO 快照） | decide 前置校验（run status/instance waiting） |
| `DecisionConsumer::pending(run_id)`（RO 轮询） | run 用；`consumed_at IS NULL` |
| `DecisionConsumer::mark_consumed(run_id, instance_id, now)`（RW 短事务） | run 用；条件 `consumed_at IS NULL` |
| `TraceWriter` | **不暴露** human_decisions 写入口（C13） |

- 所有读 API 只读；`TraceReader::open_readonly` 对 `<5` 拒绝（§6.1）。
- `events.kind` **不变**（仍七类，00 §0.3）——决策不是 `TraceEvent`。

### 6.4 `trace` 输出（06 §6.10.4/§6.10.6）

- **人类可读**：在既有事件输出后追加一段决策摘要（若有）：`human_decisions: N (approved A / rejected R; pending P)`，逐行 `instance=… decision=… approver=… decided_at=… consumed_at=… reason=…`；无决策行时**不输出**（默认 run 输出逐字节不变）。
- **`--json`（NDJSON）**：在事件行之后追加 `{"kind":"HumanDecision", "run_id", "instance_id", "decision", "approver", "reason", "decision_id", "decided_at", "consumed_at"}` 行；顺序按 `(decided_at, decision_id)`；**不写入 `events.kind`**、不改变既有事件行形状；无决策行时输出与 v5 前一致。
- `--stats`：可选增加 `human_decisions` 计数（不影响既有字段）；`replay --mode=events` 沿用 `trace --json`（含 HumanDecision 行）时，须在 DR/实现中保持「events 回放逐字节复用 trace」的既有口径——**W3a0 冻结**：`replay --mode=events` 的 NDJSON 包含 HumanDecision 行（与 `trace --json` 逐字节一致）；`--mode=outputs` 不受决策影响（按记录输出重放）。

---

## 7. kernel / CLI 契约（H6/H7）

### 7.1 kernel 类型与 API

```rust
// 00 §0.3 / 01 §1.1.1
pub enum InstanceStatus { Pending, Ready, Running, WaitingHuman, Completed, Crashed, TimedOut, Cancelled, Skipped }
pub enum CompiledHandler { Start, End, Human, Python { module, function, exec_class }, Agent { .. }, Subgraph { .. } }

// 01 §1.5（trace/kernel 同一形状；决策记录不是 TraceEvent）
pub struct HumanDecisionRecord {
    pub run_id: RunId, pub instance_id: InstanceId,
    pub decision: HumanDecision,           // Approved | Rejected（00/04 权威枚举或字符串）
    pub approver: String, pub reason: Option<String>,
    pub decision_id: String, pub decided_at: String,
    pub applied: bool,
    pub reject_reason: Option<String>,     // applied=false 的机器码（见 §4.5）
}

// 03 §3.8 控制通道
pub enum RunControl {
    Pause, Resume, Cancel,
    HumanDecision { instance_id: InstanceId, decision: HumanDecision,
                    approver: String, reason: Option<String>,
                    decision_id: String, decided_at: String },   // W3a0 新增（source-breaking）
}

// TraceSink 新增方法（默认 no-op；仅 CLI bridge 实现）
trait TraceSink { /* 既有方法 */ fn human_decided(&self, record: HumanDecisionRecord) { /* no-op */ } }
```

- `RunControl::HumanDecision` 新增 variant 对仓外穷举 `match` 是 **source-breaking**（08 §8.7 备注；W3a 实现时补）。
- `HumanDecision` 枚举（00 §0.3）`Approved`/`Rejected`（snake_case `approved`/`rejected`），与 `human_decisions.decision` CHECK 一致。
- 内核**不**依赖 trace crate 的 `HumanDecisionRow`；`HumanDecisionRecord` 是 sink 载荷（与 01 §1.5 记录同形），避免 kernel→trace 反向依赖。

### 7.2 CLI `zgraph decide`

```text
zgraph decide <run_id> --instance <instance_id> --decision approved|rejected
               --approver <id> [--reason <text>] [--json] [--db <path>]
```

| 项 | 规定 |
|---|---|
| `--db` | 与 run/trace/replay 同口径（缺省 `.zgraph/trace.db` 解析，06 §6.9.1） |
| 必填 | `run_id`、`--instance`、`--decision`（闭集）、`--approver`（非空；空白视为空） |
| 成功（人类） | stdout 一行：`decision recorded: run=<run_id> instance=<id> decision=<approved\|rejected> decision_id=<uuid>`；`--json` 时 stdout 只输出 JSON |
| 成功 `--json` | `{"ok":true,"run_id","instance_id","decision","decision_id","decided_at"}` |
| 失败 `--json` | `{"ok":false,"code","message"}`（code 见 §4.5） |
| 退出码 | **0** 成功；**2** 用法；**1** run 不存在/已终态、instance 不存在；**5** 冲突（非 WaitingHuman / 已有决策）；**4** 存储（打不开/版本不符/busy 耗尽/迁移失败）；**70** 内部 panic |
| 不改 run | decide **不写事件表**、不改 `runs.status`、不改实例状态；只 INSERT 决策行 |

- exit code **5** 为新增 CLI 常量 `EXIT_CONFLICT`（06 §6.10.5 增行；`EXIT_USAGE=2`/`EXIT_OTHER=1`/`EXIT_STORAGE=4` 既有）。
- `HUMAN_*` 结果码属 **CLI 命名空间**（不写 `runs.error_code`、不新增 00 领域码）。

### 7.3 CLI `run` 的 decision consumer（冻结）

1. run 启动成功后启动 consumer 任务（与 run 同生命周期）；无 human 节点的图也启动（空轮询开销可忽略）或按需延迟启动（实现自由，但**行为**必须一致）。
2. 每 `HUMAN_DECISION_POLL_MS=200` 用 RO 连接查 `pending(run_id)`；对每行：
   - 通过 bridge 维护的 `WaitingHuman` 实例集合（来自 lifecycle observer）判断是否仍等待；是 → 发 `RunControl::HumanDecision`；
   - 收到 `human_decided` 确认（`TraceSink` 回调）→ 用独立 RW 短事务 `mark_consumed`；stderr 输出一行（applied 或 warning）；失败重试/告警（§5.3）。
3. run 进入终态 → consumer 停止轮询；**退出前保证已确认决策的 `consumed_at` 已提交**（flush/等待短事务完成）；未消费行留 W3b。
4. consumer 不阻塞事件循环、不执行用户 Python（I8）；所有 DB 访问为非阻塞/短事务（tokio blocking 线程或等价）。
5. `--quiet` 抑制 stderr 决策行（不抑制错误）。
6. 纯等待时 run 进程**保持存活**；Ctrl-C → Cancelled + exit 130（既有取消口径）。

### 7.4 常量

`HUMAN_DECISION_POLL_MS = 200`（默认；00 §0.5 增行、03 §3.4.5 引用；不新增 DSL/配置字段）。

---

## 8. 兼容性与回归矩阵

| 类别 | 用例 | 期望 |
|---|---|---|
| hash | v0.1.0 fixtures、W1 SOP v1、W2a/W2b 图 | `graph_hash` 逐字节不变（DSL 无变化） |
| validate | 无 human 图 | 错误码/定位/`error_count` golden 不变 |
| validate | `builtin.human` 图 | 通过（格式合法）；`gate.function` 写 `human` → 仍 `DSL_SYMBOL_NOT_FOUND`（不是 gate op） |
| run | 无 human 图 | 行为/trace/golden 不变（AC-16 g） |
| run | human 图 | 输入就绪 → `WaitingHuman`；不派发、不启动 worker；decide 后 Completed/publish |
| run | 纯等待 | run 不终止（非静默）；无 `RUN_DEADLOCK`；`Pause/Resume` 不改变等待 |
| trace | v4 库只读 | `trace`/`replay` 拒绝（exit 4，不迁移） |
| trace | v4 库 `run` 打开 | 迁移 v5（备份 + 单事务 + 重建两表）；旧 run 事件/回放不变 |
| trace | v3→v5 链 | v3 seed → v4 → v5；L1 golden 与 v5 前逐字节一致（除元数据） |
| trace | `trace --json` 无决策 run | 与 v5 前一致（不追加 HumanDecision 行） |
| L1 | 含决策 run 的 events 回放 | events 逐字节一致；`trace --json` 追加 HumanDecision 行（确定性顺序） |
| L2 | `--record=full` + `replay --mode=outputs` | human 实例输出按 `(node_id,attempt)` 重放；不重新要求人工输入；结果一致 |
| CLI | decide 全部退出码/`--json` | 与 §4.5/§7.2 逐项一致 |
| 并发 | decide × run、同实例双 decide | 无失败；同实例恰一 succeed（0），其余 exit 5 |

---

## 9. 风险与缓解

| 风险 | 应对 |
|---|---|
| `WaitingHuman` 被误判静默 → run 提前 Completed | 03 §3.10 `is_quiescent` 显式含 `WaitingHuman`；AC-16 a/f；kernel 单测「纯等待不静默」 |
| human 节点误走 worker / `NODE_WORKER_IMPORT` | 编译期精确映射 `builtin.human` → `CompiledHandler::Human`；02 §2.5 记录 R14 例外；AC-16 a 断言无 worker 进程/无 worker 版本 |
| decide×run 并发写冲突/丢决策 | C13 分表写者 + WAL + busy_timeout/重试 + 唯一键；AC-16 c/f；并发 e2e 恰一成功 |
| 决策重复应用 | 唯一键 + kernel「非 WaitingHuman → applied=false」双重幂等；AC-16 c |
| trace v5 重建两表迁移失败/丢行 | `VACUUM INTO` 备份 + 单事务 + 显式列清单复制 + 失败恢复；AC-16 e；迁移测试覆盖 v1–v4→v5 |
| 旧 trace golden 漂移 | `events` 不改写；`trace` 无决策 run 输出不变；AC-16 e/g |
| Pause 期间 decide 语义不清 | §4.4 行 8 冻结：允许 decide、状态迁移/投递照常、**不恢复派发** |
| human 期限与普通超时混淆 | §2.3 冻结：`declared_timeout_ms` 缺省 = 无期限（显式例外）；AC-16 d |
| 决策表被误当事件/回放 | `events.kind` 不变；HumanDecision 行只在 trace 输出层追加（§6.4） |
| 并发 consumer 多实例（未来多进程 run） | W3a 契约 = 每 run 一个 consumer；多 run 各自 consumer；行按 run_id 隔离 |

---

## 10. AC-16 验收矩阵（设计冻结；H9）

> **本批只冻结设计与断言口径**；测试名/计数/资产由 W3a 实现后 W3a-docs 回写（07 会标注「设计冻结」）。AC 资产建议落 `tests/fixtures/graphs/human_gate/**`（W3a 定名）。

| # | 场景 | 图/资产（设计） | 关键断言 |
|---|---|---|---|
| a | **approve 真链路** | `human_gate/approve.json`：`start → prep(python) → approve_human(builtin.human, timeout 可选) → end`；approved 边 `check_output{path:"decision",equals:"approved"}`；真 CLI（真 worker + 真 SQLite） | 真 `zgraph run` 在 `approve_human` 挂起：`instances.status=WaitingHuman`、无 `Running` 迁移、无 worker 进程；`decide --decision approved` exit 0；run 自动接续 Completed；实例 output `{"decision":"approved","reason":...}`；出边 gate passed；`human_decisions` 一行 `consumed_at` 非空；`trace`/`--json` 可见决策行；`instances.input` 为该代上下文（`--record=full`） |
| b | **reject + W2a rework** | `human_gate/rework.json`：`prep → human →（approved 边 → end；rejected 边 `on_gate_false:"defer"` → 回边重入 prep/human）`；`max_attempts` 预算；reject 后下一轮 approve | reject 决策后实例 **Completed**（不 Failed）；defer 回边按 W2a attempt-per-activation 产生 `attempt+1` 的新实例；再次 human 等待 → 再 decide approved → run Completed；`attempt` 序列 0/1；gate_results 记录 deferred 行；无 `RUN_DELIVERY_TO_TERMINAL`/死锁 |
| c | **冲突/幂等/非 waiting/退出码 + `--json`** | 无特殊图；复用 a 的 run 快照 | 非 WaitingHuman 实例 decide → exit **5** + `code=HUMAN_INSTANCE_NOT_WAITING`；同实例二次 decide → exit **5** + `code=HUMAN_DECISION_EXISTS`；run 已终态 → exit **1** `HUMAN_RUN_NOT_ACTIVE`；instance 不存在 → exit **1** `HUMAN_INSTANCE_NOT_FOUND`；缺参/非法 decision/空 approver → exit **2**；`--json` 成功/失败形状逐字段；decision 行恰 1 行（重复未插入） |
| d | **决策期限超时** | `human_gate/timeout.json`：human 节点 `config.timeout_ms=200`（无 decide） | 到期后实例 `TimedOut` + `error_code=NODE_TIMEOUT`；run `Failed`；`killed_workers` 不增（无 worker）；决策期限从进入 `WaitingHuman` 起算；此时 decide → exit 1；trace 有 `NodeTimedOut` |
| e | **trace v4→v5 迁移/备份/只读拒绝/v3→v5 链回放** | `tests/fixtures/trace/schema_v3_seed.sql`（既有）+ 新增 v4 seed（W3a 交付）；真 CLI 打开旧库 | `run` 打开 v4 库 → 迁移 v5：备份 `.bak.v4-*` 恰一份、`user_version=5`、`schema_migrations` 1..5、`instances`/`transitions` 行数与数据逐列保留、`human_decisions` 存在；只读 `trace`/`replay` 对 v4 → exit 4 且不迁移；v3 seed → v4 → v5 链后 L1 golden 与迁移前一致；无决策 run 的 `trace --json` 与 v5 前逐字节一致 |
| f | **decide×run 并发（WAL busy）无失败、同实例并发恰一成功** | 真 CLI run 挂起 + 并发触发 decide（同实例 2–10 进程/线程、以及不同实例） | 并发的同实例 decide：恰 **1** 个 exit 0，其余 exit 5（`HUMAN_DECISION_EXISTS`/`HUMAN_INSTANCE_NOT_WAITING`）；`human_decisions` 恰 1 行；run 正常接续、无 `SQLITE_BUSY` 泄漏/存储错误；重复运行 3 次无 flake |
| g | **默认图/v0.1.0 golden 不变** | v0.1.0 fixtures + W1 SOP v1 图 + W2a/W2b 图（无 human） | `graph_hash` 逐字节不变；validate/run 结果与 trace golden 不变；trace v5 后旧 run 回放不变；`instances.status` 不出现 `WaitingHuman`；`human_decisions` 空；`trace --json` 无 HumanDecision 行 |

---

## 11. 实施顺序与模块拆分（W3a）

| 顺序 | 模块 | 分支（DR-004 §4） | 依赖 | 关键交付 |
|---|---|---|---|---|
| 1 | docs | `docs/docs-human-design`（本批） | 无 | 本 DR + spec 00/01/02/03/04/06/07 冻结写入 + 07 AC-16 设计 + PROVENANCE |
| 2 | trace | `feat/trace-human-decisions` | 本批（接口按 §5/§6） | schema v5（重建 instances/transitions CHECK、human_decisions/索引、迁移/备份/只读拒绝）；`HumanDecisionRow`/`TraceReader::human_decisions`/`DecisionStore`/`DecisionConsumer`；`trace` 决策行 |
| 3 | kernel | `feat/kernel-human-wait` | trace 接口已定（可与 trace 并行，按 §6.3 接口） | `InstanceStatus::WaitingHuman`、`CompiledHandler::Human`、编译映射、`Pending→WaitingHuman`、控制/期限/取消/终止判定、`RunControl::HumanDecision`、`TraceSink::human_decided`、decision consumer 的 kernel 侧 |
| 4 | cli | `feat/cli-decide` | trace 分支（决策表 API）+ kernel 分支（控制/确认） | `zgraph decide` 子命令/退出码/`--json`；`run` decision consumer；`trace` human 输出；`EXIT_CONFLICT=5` |
| 5 | e2e | `test/e2e-human-decide` | trace/kernel/cli 全部合并 | AC-16 a–g 的图/脚本/真链路；迁移与并发用例；golden 回归 |
| 6 | docs | `docs/docs-human-spec`（W3a-docs） | W3a 实现合并 | as-built 回写（测试名/计数/偏差）、AC-16 实测、08 注记（source-breaking） |

- 模块边界：trace 只写 `crates/zgraph-trace/**`；kernel `crates/zgraph-kernel/**`；cli `crates/zgraph-cli/**`；e2e `tests/e2e/**` + `tests/fixtures/graphs/**`（+ trace seed fixtures，按 ISSUE-111 归属后续同步）；docs `doc/**`。
- **契约钉死**：kernel 与 cli 的 `HumanDecisionRecord` 形状以 §7.1 为准；trace 的 `HumanDecisionRow` 以 §6.3 为准；二者字段对齐（snake_case 字符串/时间口径一致）。
- `08 §8.7` 的 source-breaking 备注（`RunControl` 新 variant）由 W3a 实现/W3a-docs 补（W3a0 不写 as-built 注记，见 §12 E-4）。

---

## 12. 冲突、边界与需 Lead/评审确认项

### 12.1 与既有 spec / DR 的冲突及处置

| # | 冲突点 | 处置 |
|---|---|---|
| C-1 | 00 §0.3 `InstanceStatus` 注释「WaitingHuman 保留但 v1 不产生」 vs W3a0 使其产生 | 本批更新 00 §0.3（`WaitingHuman` 进入枚举并注明 v0.2/W3a 起产生）；旧注释作为历史不再适用（追加式说明，不删除历史 DR） |
| C-2 | 04 §4.1.3「v1 没有 resume 入口 / 不产生 WaitingHuman」 vs W3a0 | 04 §4.1.3 改为「W3a0 起产生 WaitingHuman，但仍无 resume 入口：decide 自动接续；durable resume 属 W3b」；`NodeOutcome`/`ResumableNodeHandler` 仍不实现（保持 App 层不与框架并发） |
| C-3 | 06 §6.6「trace 事件唯一写连接」 vs C13 三写者 | 06 §6.6 增注：唯一写连接不变式只约束**事件表**；`human_decisions` 按 C13 分表写者（decide/consumer），WAL + busy_timeout + 短事务 |
| C-4 | 03 §3.9.1「实例超时默认 180s」 vs human 缺省无期限 | 03 §3.9.1 增 human 行/注：human 用 `declared_timeout_ms`，缺省**无期限**（显式例外）；普通节点不变 |
| C-5 | 06 §6.2.5/§6.2.7 的 status CHECK 无 WaitingHuman | 06 升级为 schema v5：DDL 两表 CHECK 增 `WaitingHuman`；迁移重建（§6.2） |
| C-6 | 04 §4.6 `manual` gate vs human 节点 | 明确二者无关：`manual` 是 gate op（求值恒 false/可配置），human 是节点等待；不改 `manual` 语义（04 §4.6/§4.6.4） |
| C-7 | 02 §2.5 python_node 符号/R14 import vs `builtin.human` | 02 §2.5 增保留符号行与 R14 例外（格式校验照常、不 import、无 `NODE_WORKER_IMPORT`）；DSL schema/规则编号不变 |
| C-8 | 00 §0.4「无新错误码」 vs decide 的 `HUMAN_*` 码 | `HUMAN_*` 是 **CLI 命名空间**（decide `--json.code`），不写 `runs.error_code`、不进 00 错误码表；00 §0.4 增注「decide 结果码为 CLI 命名空间」 |
| C-9 | 07 通过标准 AC-01..AC-15 vs AC-16 | 07 通过标准改 AC-01..AC-16（AC-16 本批为设计冻结；W3a 实现后转实测） |

### 12.2 冻结时补全的边界（无现成决策，已按最小风险冻结）

| # | 边界 | 冻结 |
|---|---|---|
| E-1 | decide 在 **Paused** run 上 | 允许（预检 status ∈ {Running,Paused}）；应用状态迁移/投递，但**不恢复派发**；run 仍 Paused（§4.4 行 8） |
| E-2 | Pause 期间的决策期限 | **不暂停**计时（真实经过时间）；到期仍 TimedOut（§3.4） |
| E-3 | decide 对 `user_version<5` 的库 | 不迁移；exit 4（`HUMAN_DB_ERROR`，提示先运行 `zgraph run` 迁移）；§5.4 |
| E-4 | `RunControl` 新 variant 的 source-breaking | W3a0 只冻结形状；08 §8.7 备注由 W3a 实现/W3a-docs 补（不在本批写 as-built） |
| E-5 | `consumed_at` 更新失败 | 不 fail run；stderr 警告 + 保留 NULL（下轮重试）；run 结束后留 W3b（§5.3） |
| E-6 | consumer 提交决策但 run 恰在同时终态 | kernel 可能不再处理控制；consumer 未见 `human_decided` 确认 → 不标 consumed；行留 W3b；不阻塞退出（§7.3） |
| E-7 | 同一 human 实例多次等待（rework） | 由 W2a attempt-per-activation 产生**新实例/新 instance_id**；每实例至多一行决策；旧实例保持 Completed |
| E-8 | decide 的 `--reason` 空白 | 视为空字符串？**冻结**：`--reason` 未提供 → `NULL`；提供但全空白 → 存原样（非空校验只针对 `approver`）；不 trim 业务文本（除 approver） |
| E-9 | `decision_id` 生成方 | **CLI decide 侧生成 UUIDv7** 并写入；run consumer 原样透传（审计/日志）；不接受用户指定 |
| E-10 | trace 输出顺序 | HumanDecision 行在事件行之后、按 `(decided_at, decision_id)`；同一 run 的 decision 行确定性有序 |
| E-11 | **合同内部不一致（**Lead 2026-10-31 已裁决**）**：task-11 §2.3 写「instance 不存在 → exit 1」，而 §3.4 写「instance 存在且 status=WaitingHuman（否则 exit 5）」——对「instance 不存在」的退出码口径不一致 | **Lead 裁决（2026-10-31）**：按本 DR 冻结口径执行——**instance 不存在 → exit 1**（`HUMAN_INSTANCE_NOT_FOUND`，与 run 不存在同属「目标不存在」）；**instance 存在但 status≠WaitingHuman → exit 5**（`HUMAN_INSTANCE_NOT_WAITING`）。§4.5/06 §6.10.5 口径为准，不再待决（见 §12.3 #5） |

### 12.3 已确认项（Lead 2026-10-31 裁决；W3a 实现依据）

1. **决策期限缺省**：**采纳**本 DR §2.3 冻结「human 节点未声明 `timeout_ms` → 无期限」（显式例外；普通节点缺省仍 180s）。
2. **Paused 期间 decide 不自动 Resume**（E-1）：**采纳**——decide 允许插入行且 consumer 应用决策（`WaitingHuman→Completed` + publish），但 run **保持 Paused**、不自动恢复派发；CLI 无 `pause`/`resume` 子命令，库调用方自行 Resume。Pause 语义不变。
3. **trace `--json` 追加 HumanDecision 行**（§6.4）：**采纳**——事件行之后追加 `{"kind":"HumanDecision",...}` NDJSON 行（不写入 `events.kind`；既有消费者需容忍新 kind 行；无决策 run 输出不变）。
4. **`builtin.human` 作为保留 python_node 符号**（H1）：**采纳**——不新增 NodeType，保持 DSL schema 零变更（02 §2.5/04 §4.4.5）。
5. **合同 §2.3 与 §3.4 的退出码口径冲突（E-11）**：**Lead 已裁决**——instance 不存在 → **exit 1**（`HUMAN_INSTANCE_NOT_FOUND`，与 run 不存在同属「目标不存在」）；instance 存在但 status≠WaitingHuman → **exit 5**（`HUMAN_INSTANCE_NOT_WAITING`）。§4.5/06 §6.10.5 按此执行。

> 以上为 **Lead 2026-10-31 裁决记录**（追加，不改历史决策）；W3a 实现与 W3a-docs 回写按此执行。

---

## 13. 与 DR-004 / REV-063 / ISSUE 的对应

| 来源 | 本 DR 落点 |
|---|---|
| DR-004 §3 W3a0（decide/resume 契约、human_decisions 表、C13 写连接模型、唯一键/decision_id/consumed_at） | §5（表/写者）、§4（decide）、§12.3 |
| DR-004 §5 D3（trace DB 控制通道、WAL、唯一键、不做 HTTP/socket、run 退出后不可 decide） | §4.1/§5.2/§4.6 |
| DR-004 §3 W3a DoD（human gate 暂停 → decide approve 恢复；reject 按矩阵终止或 rework；重复/冲突；e2e） | §10 AC-16 a–g |
| REV-063 **C12**（decision_id 仅审计、唯一键 `(run_id, instance_id)`） | §5.1 |
| REV-063 **C13**（写连接模型：TraceWriter=事件表、decide=插入、run=更新 consumed_at；busy_timeout/重试/可见性） | §5.2 |
| REV-063 **S10**（human_decisions 并发模型） | §5.2/§5.3/AC-16 f |
| DR-005（W2a defer/attempt-per-activation） | §4.3/§10 b（reject→rework 复用） |
| W2b（R16/max_fanout） | 无直接耦合；AC-16 g 回归覆盖 |
| ISSUE-111（trace fixture 路径归属） | §11 e2e trace seed 归属提示；不阻塞 |
| 08 §8.7（source-breaking 备注） | §12.2 E-4（W3a/W3a-docs 补） |

---

## 14. 影响文件清单与本批门禁

### 14.1 本批（W3a0，docs）改动

| 文件 | 改动 |
|---|---|
| `doc/decisions/DR-006-w3a-human-decide-freeze.md` | 新增（本文件） |
| `doc/spec-v1.2/00-总纲.md` | banner；`InstanceStatus::WaitingHuman`；`HumanDecision`（可选枚举）；`HUMAN_DECISION_POLL_MS`；§0.4 CLI 命名空间注；§0.6 I1 解释；§0.7 术语 |
| `doc/spec-v1.2/01-领域模型与状态机.md` | banner；§1.1.1 `CompiledHandler::Human`；§1.2.1 状态；§1.2.3 矩阵/副作用；新增 §1.2.4 人工等待；§1.5 `HumanDecisionRecord` |
| `doc/spec-v1.2/02-DSL与校验.md` | banner；§2.5 保留符号 `builtin.human` 与 R14 例外（DSL schema/R1–R16 不变） |
| `doc/spec-v1.2/03-调度器与生命周期.md` | banner；新增 §3.4.5 人工等待与决策应用；§3.8 Paused decide 注；§3.9.1 human 期限例外/§3.9.3；§3.10 `is_quiescent` 含 WaitingHuman；§3.15 矩阵行 |
| `doc/spec-v1.2/04-节点接口与扩展点.md` | banner；§4.1.2 handler 表；§4.1.3 措辞（W3a0 产生 WaitingHuman、仍无 resume）；§4.4.1 映射表；新增 §4.4.5 `builtin.human`；§4.6 `manual` gate 无关注 |
| `doc/spec-v1.2/06-存储与可观测.md` | banner；schema v5（§6.2.2/§6.2.5/§6.2.7/新增 §6.2.14/§6.7）；§6.6 写者模型注；§6.10.1 子命令数；§6.10.4 trace 决策摘要；§6.10.5 退出码 5；§6.10.6 HumanDecision NDJSON；新增 §6.10.8 `zgraph decide` |
| `doc/spec-v1.2/07-验收用例.md` | banner；§7.1/§7.2/DoD；新增 **AC-16 a–g（设计冻结）**；通过标准 AC-01..AC-16 |
| `doc/progress.md` / `doc/index.md` / `doc/decisions/DR-004 §10` | 状态/索引/执行记录（追加，不改原决策） |
| `doc/spec-v1.2/PROVENANCE.md` | §2 W3a0 新条目 + §3 重算 + README 行数表 |
| `doc/spec-v1.2/README.md` | 行数表/批次说明 |

> **W3a 实现波及（非本批改动）**：`crates/zgraph-trace/**`（schema v5/API）、`crates/zgraph-kernel/**`（WaitingHuman/handler/control/sink）、`crates/zgraph-cli/**`（decide/consumer/trace 输出）、`tests/e2e/**` + `tests/fixtures/**`（AC-16 资产）——由 W3a 各分支交付。

### 14.2 不修改（红线）

`CLAUDE.md`；任何代码/测试/配置/`schemas/**`；`doc/reviews/**`（评审记录由评审人写）；不改写已冻结的 W1/W2a/W2b 历史决策（本批只追加/更新 W3a0 冻结口径）；不把未冻结项写成 as-built。

### 14.3 提交与评审

- 分支 `docs/docs-human-design`；提交正文含 `Review: REV-078` 与 `Agent: docs-dev`；**本批不 push、不合并**。
- `bash scripts/dev/check.sh` 必须 PASSED；PROVENANCE §3 的 SHA-256/行数以实测为准；合并前由独立评审人写 `doc/reviews/REV-078-docs-human-design.md`，结论必须为「通过」（AGENTS §3/§4）。
- W3a 实现完成后由 W3a-docs 做 as-built 回写（新增 PROVENANCE 第 2 条、回写 07 AC-16 测试名/计数、08 §8.7 备注）。
