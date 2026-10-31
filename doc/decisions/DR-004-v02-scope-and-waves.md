# DR-004 · v0.2（M7）范围与波次

> **状态**：已立项（用户 2026-10-31「推送后开启下一个版本」）；**r3 修订**（2026-10-31，关闭 REV-063 C1–C9 / S1–S8）；评审记录 REV-063。
> **日期**：2026-10-31 · **作者**：lead
> **关联**：v0.1.0（tag `v0.1.0` / GitHub Release）、[`design/software-team-sop-v1.md`](../design/software-team-sop-v1.md)（REV-062）、[`DR-001`](DR-001-runtime-route.md)、[`DR-003`](DR-003-m6-scope-and-waves.md)、[`issues.md`](../issues.md)、spec v1.2 分册 00/01/02/03/04/06/07/08
> **前置事实**：M0–M6 已交付；v0.1.0 已发布（tag `v0.1.0` @ `5d43c58`）；SOP 设计稿 v1 已合入（`7cb13ca`，REV-062）。

---

## 1. 背景与目标

### 1.1 背景

- v0.1.0 能力边界（用户已知悉）：无人工介入（`WaitingHuman` 不产生）、无图级返工（带出口环会 `RUN_DEADLOCK` / `RUN_DELIVERY_TO_TERMINAL`）、无每节点扇出上限、无动态 N、无共享评审队列、无框架级 artifact 端口；`gate=false` 默认 settle 并级联 `Skipped`；`RunControl` 为进程内 mpsc，CLI 无 `decide`/`resume`。
- SOP 设计稿 v1 把 `software_team` SOP 收敛为「设计角色依赖 DAG + 工作 agent + per-item 评审」，并把上述能力延期到下一版本。
- 用户 2026-10-31 决定：**人工评审、图级有限返工、每节点最大扇出限制、动态 N 另起一个版本**；SOP 设计稿也需落地。

### 1.2 v0.2 目标

**让 software_team SOP v1 真正可执行，并把图执行模型从「静态 DAG + 一次性实例」扩展为「可人工停等、可有限返工、可限制扇出、可按运行期数据展开 N」，同时保持 v0.1.0 的默认语义与回放兼容。**

### 1.3 非目标

- API 服务 / 前端 / Agent 资产平台 / 多租户。
- 多 provider 路由/fallback、多模态、MCP、工具并行执行。
- 共享评审队列（v0.3 候选，见 §3 的 W5）。
- 框架级 artifact 存储（SOP v1 以数据值传递）。
- 历史 trace 重写（只做 forward-only 迁移，见 D5）。
- **durable run（崩溃后恢复）**：作为 W3b 可选范围，默认不纳入 v0.2 发布条件（需用户确认，见 §9）。

---

## 2. 版本定位与原则

- **版本号**：`v0.2.0`；里程碑 **M7**。
- **原则**：
  1. **默认行为不变**：v0.1.0 图的 gate/Skipped/回放语义是兼容基线；所有新语义**opt-in**（D1/D2）。
  2. **先落地后改内核**：W1 只依赖 v0.1.0 能力，先跑通 SOP v1，不阻塞后续波次。
  3. **一波一审批、按模块拆 worktree**：每波可含多个模块分支，**每个 worktree 只服务一个模块**；跨模块由 Lead 声明第二评审人（AGENTS §2）。
  4. **docs 单写入者**：同一时间只允许一个 `docs` 分支；设计冻结/spec 回写按 §4 顺序串行。
  5. **语义变更必须回写 spec**：W2–W4 涉及 00/01/02/03/04/06/07/08 分册；每波内完成回写与 PROVENANCE 更新。
  6. **可复现不回退**：新事件/状态必须补 L1/L2 回归；trace 只做 forward-only 迁移（D5）。

---

## 3. 范围与工作项

### W1 · SOP v1 落地（不改内核/DSL 语义）

**目标**：按 SOP 设计稿 §4.1 依赖矩阵 codegen 出可 validate 的图，用 fake LLM 跑通 per-item 评审链路。

- 交付（模块：`e2e` + `py-worker`；docs 回写单独排期）：
  - codegen 脚本（依赖矩阵 → 60 节点/100 边），落 `tests/fixtures/graphs/software_team_v1/codegen.py`（e2e）；
  - agent 资产：11 个设计角色、共享 `design_reviewer`、工作 agent（`agent_call` / `python_node`）、prompt/constraints；
  - `parse_verdict` python 节点（`python/workers/software_team/`，py-worker）；
  - fake LLM 脚本 + e2e：产物透传、approved gate、失败路径（`NODE_PYTHON_EXCEPTION` / `HARNESS_MAX_ITERATIONS`）、下游 `trigger=all` 等待、per-item 实例/trace；
  - 07 分册 AC 新增/回写（docs 子任务）。
- **fake LLM 限制（S1）**：CLI 单 provider、fake 脚本按调用序消费（`fake.rs`），60 节点并发只能验证**编排/门控/产物链路**，不能验证 prompt 质量；W1 另加**手动 real-LLM smoke**（`--real`，不进 CI），并在 AC 注明。
- DoD：codegen 产物 `zgraph validate` 通过；e2e 全绿；**不修改 kernel/dsl/harness 语义**；codegen 可重复生成同一 graph_hash。

### W2 · 图级有限返工 + 每节点最大扇出限制

#### W2a0 · 设计冻结（docs，先行）

冻结「返工边 + attempt-per-activation」语义（W2a 实现依据），至少包含：

- **语法**：边级 `on_gate_false: "settle" | "defer"`（缺省 settle）；`defer` 边**必须位于环内**（自环或回边；DSL 校验）；
- **重入**：deferred 边最终放行时，若目标实例已 `Completed`，按 `attempt+1` **新建实例**（attempt-per-activation），而不是 `RUN_DELIVERY_TO_TERMINAL`；仅 defer 边允许此行为；
- **上限**：节点 `max_attempts`（缺省 3，对齐现有 `retry.max_attempts`），超限 → 实例 Crashed → run Failed；
- **终止**：未决 deferred 边阻止静默完成；无可重入来源 → watchdog `RUN_DEADLOCK`；
- **trace/回放**：`GateResult` 增 `deferred=true` + `attempt`；L1/L2 golden 覆盖 defer 路径；
- **兼容**：v0.1.0 图不受影响（缺省 settle）。

#### W2a · opt-in deferrable gate + 有限返工（实现）

- 按 W2a0 冻结的语义实现；模块：`kernel` + `dsl` + `trace` + `e2e` + `docs`；spec 01/02/03/06。
- DoD：自环返工与一条回边返工（A→B→A）端到端通过；最终超限失败；v0.1.0 图/回放 golden 不变。

#### W2b · 每节点最大扇出限制（两层）

- **字段**：节点级 `max_fanout`（可选，整数 ≥1，缺省无上限）。
- **静态层**：加载期校验静态 out-degree；超限 → DSL 错误码（02 R4/R9 白名单）。
- **运行期层**：单次激活经 gate 放行的投递边数上限；超限 → 实例 Crashed（新运行期错误码 + trace），**不静默截断**。
- 模块：`dsl` + `kernel` + `e2e` + `docs`；spec 02/03；默认无上限保证向后兼容。
- DoD：两层限制分别有反例；v0.1.0 图/回放 golden 不变。

### W3 · 人工评审（human lifecycle）

#### W3a · 人工停等 + decide/resume（v0.2 必须）

- **控制通道（C7）**：`run` 进程在 `WaitingHuman` 时**保持存活**，轮询 trace DB 的 `human_decisions` 表（WAL 只读连接，`HUMAN_DECISION_POLL_MS` 缺省 200ms）；`zgraph decide <run_id> --instance <id> --decision approved|rejected --approver <id> [--reason ...]` 向该表插入决策（幂等键 `(run_id, instance_id, decision_id)`）；run 消费后记录决策事件并恢复。**不引入 HTTP/socket**；run 进程退出后无法 decide（需 W3b）。
- **契约**：decide 幂等/冲突（重复决定、非 `WaitingHuman` 实例、已结束 run 的退出码与 `--json`）；approver 必填；拒绝去向矩阵（与 W2a 返工/终止衔接）；trace 决策事件（谁/何时/结论/理由）；`resume` 仅作为 decide 的显式续跑入口（或 decide 自动接续，W3a0 设计冻结）。
- 模块：`kernel` + `cli` + `trace` + `e2e` + `docs`；spec 00/01/03/04/06/07；**不涉及 harness**。
- DoD：run 在 human gate 暂停 → `decide approve` 后恢复；`reject` 按矩阵终止或进入 rework（W2a）；重复/冲突 decide 行为符合契约；e2e。

#### W3b · durable resume（可选，需用户确认；默认不纳入 v0.2 发布条件）

- 范围：进程崩溃/重启后的恢复；recovery 状态机、`resume` CLI、Running 实例/worker 处置（06 §6.8.1 目前只标 interrupted，不恢复）、L3 幂等边界。
- 模块：`trace` + `kernel` + `cli` + `e2e` + `docs`；spec 06/07。

### W4 · 动态 N（运行期 fan-out）

#### W4a · 设计冻结（docs，先行）

- **语法**：节点级 `foreach`（具体字段名 W4a 冻结），从指定输入端口取**列表**（entry input 或上游输出），按 item 展开该节点及其下游 per-item 链路；
- **item**：key 确定性（索引或显式 key 字段），item 实例 id 可复现；
- **join**：单端口接收**有序 item 结果列表**（避免动态端口）；`trigger=all` = 所有 item 结果到齐；默认 `on_item_error: "fail"`（任一 item 失败 → run Failed）；
- **失败/取消/上限/回放**：cancel 传播到所有 item 实例；每激活上限复用 W2b；trace 记录展开事实（item 数/key）；`graph_hash` 只随图 JSON 变化；空列表语义（跳过并记 trace）明确。

#### W4b · 实现

- 模块：`dsl` + `kernel` + `trace` + `e2e` + `docs`；spec 01/03/06；第二评审。
- DoD：N 由 entry input/上游列表决定；item 独立实例/trace；join 正确；失败/取消/上限符合 W4a；回放可复现；e2e。

### W5（候选，v0.3）· 共享评审队列

- 一个评审节点按到达顺序排队处理多份投递、评完一个放行一个；需要节点邮箱/多次激活/按 item 路由，依赖 W4。
- **v0.2 不做**；W4 完成后评估 v0.3。

---

## 4. 波次、分支、模块与评审

> 每波可有多个模块分支；**一个 worktree 只服务一个模块**；跨模块分支必须由 Lead 指定第二评审人。
> **顺序**：`kernel`/`trace` 同一时间一个写入者，按 W2a0(docs) → W2a → W2b → W3a → W3b(可选) → W4a(docs) → W4b 串行；`docs` 同一时间一个分支，按 W2a0 → W1-docs → W2a-docs → W2b-docs → W3a-docs → W4a → W4b-docs 排队；W1（e2e/py-worker）可与 W2a0 docs 并行。

| 波次 | 分支 | 模块 | 依赖 | 评审 |
|---|---|---|---|---|
| W1 | `test/e2e-software-team-v1`、`feat/py-worker-parse-verdict` | e2e / py-worker | 无 | 独立 + 跨模块第二评审 |
| W1-docs | `docs/docs-sop-v1-ac` | docs | W1 合并后 | 独立评审 |
| W2a0 | `docs/docs-rework-design` | docs | 无（可先做） | 独立评审 |
| W2a | `feat/kernel-deferrable-gate`、`feat/dsl-gate-defer`、`feat/trace-gate-defer`、`test/e2e-gate-defer` | kernel / dsl / trace / e2e | W2a0 | 主审 + 第二评审 |
| W2a-docs | `docs/docs-gate-defer-spec` | docs | W2a | 独立评审 |
| W2b | `feat/dsl-max-fanout`、`feat/kernel-max-fanout`、`test/e2e-max-fanout` | dsl / kernel / e2e | W2a | 主审 + 第二评审 |
| W2b-docs | `docs/docs-fanout-spec` | docs | W2b | 独立评审 |
| W3a0 | `docs/docs-human-design`（decide/resume 契约冻结） | docs | W2a | 独立评审 |
| W3a | `feat/kernel-human-wait`、`feat/cli-decide`、`feat/trace-human-decisions`、`test/e2e-human-decide` | kernel / cli / trace / e2e | W2a、W3a0、W2b | 主审 + 第二评审 |
| W3a-docs | `docs/docs-human-spec` | docs | W3a | 独立评审 |
| W3b（可选） | 用户确认后定 | trace / kernel / cli / e2e | W3a | 主审 + 第二评审 |
| W4a | `docs/docs-dynamic-fanout-design` | docs | W2b、W3a | 独立评审 |
| W4b | `feat/dsl-foreach`、`feat/kernel-dynamic-fanout`、`feat/trace-dynamic`、`test/e2e-dynamic-fanout` | dsl / kernel / trace / e2e | W4a | 主审 + 第二评审 |
| W4b-docs | `docs/docs-dynamic-spec` | docs | W4b | 独立评审 |

---

## 5. 关键设计决策

| # | 决策 | 内容 |
|---|---|---|
| D1 | 返工语义 | **opt-in deferrable gate**：边级 `on_gate_false: "settle"\|"defer"`（缺省 settle）；defer 边必须在环内；deferred 边最终放行时目标已完成 → `attempt+1` 新实例（attempt-per-activation），不报 `RUN_DELIVERY_TO_TERMINAL`；节点 `max_attempts` 缺省 3，超限 Crashed→Failed；未决 deferred 边阻止静默完成，无可重入来源 → RUN_DEADLOCK；trace 记 `deferred`+attempt；v0.1.0 golden 硬回归 |
| D2 | 扇出上限 | 节点级 `max_fanout`（缺省无上限）：加载期静态 out-degree 校验（超限 DSL 错误）+ 运行期每激活放行边数上限（超限实例 Crashed + 运行期错误码 + trace）；不静默截断 |
| D3 | 人工接口 | trace DB `human_decisions` 作为跨进程控制通道（run 存活轮询、WAL 只读）；CLI `decide` + `resume`；幂等/冲突、approver、拒绝去向矩阵、退出码/`--json`、trace 决策事件；不做 HTTP/socket；run 退出后不可 decide（W3b） |
| D4 | 动态 N | 节点级 `foreach`（语法 W4a 冻结）；来源 = entry input 或上游输出列表；item key 确定性；join = 单端口有序 item 结果列表；默认 `on_item_error=fail`；取消传播；每激活上限复用 D2；trace 记录展开；graph_hash 只随图 JSON |
| D5 | trace 迁移 | **forward-only**（spec 06 §6.7）：新代码迁移旧库后使用；只读 `trace`/`replay` 对旧版本拒绝且不迁移（退出码 4）；迁移点：W2a（deferred/attempt 字段）、W3a（`human_decisions` 表）、W4b（item 展开）；逐版本备份 + 迁移矩阵 + 迁移测试；不承诺新代码直接读旧 schema |
| D6 | 复现兼容 | 新增事件/状态必须通过 `replay --mode=events` 与 L2 outputs；v0.1.0 golden 为硬回归；旧 DB 迁移后可回放 |
| D7 | 向后兼容 | v0.1.0 图的 gate/Skipped/回放默认语义不变；新字段全部可选、缺省行为 = v0.1.0 |

---

## 6. 发布条件（v0.2.0）

1. W1–W4 合并（W3b 视用户确认；W5 不阻塞）。
2. 质量门禁：`cargo test --workspace`、`cargo clippy -D warnings`、pytest 全绿（基线 0）；CI 通过。
3. **v0.1.0 兼容矩阵（C8）**：
   - `validate`：`tests/fixtures/graphs/valid_*.json` 全部通过；`bad_*.json` 的错误码/定位/error_count 与 v0.1.0 golden 一致（不新增、不回退）；
   - `run`：可运行子集 `customer_ticket.json`（AC-01）、`valid_parallel.json`、`harness_ac08` 图跑通；
   - **已知例外**：`valid_cycle.json` 断言 `Failed / RUN_DEADLOCK`（ISSUE-062），`valid_subgraph.json` 断言 `Failed / NODE_TYPE_UNSUPPORTED`；
   - `bad_*.json` 不进入 run；v0.1.0 生成的 trace DB 经 D5 迁移后 `trace`/`replay` 通过；v0.1.0 golden 事件/输出不变。
4. **性能基线（可验收）**：同机、无外部 CPU 争用、各 3 次取中位数；W1 用 SOP 设计稿 §4.1 的 N_i（60 节点/100 边，非 N=3）记录绝对基线到 `doc/perf-baseline-v0.2.txt`；发布门禁：AC-01 单 run、AC-12 10 并发 run、SOP e2e 的 duration 中位数回退 ≤ 20%。
5. spec 回写：00/01/02/03/04/06/07/08 按波次完成 + PROVENANCE 更新；07 新增 AC 通过。
6. trace 迁移：迁移矩阵、逐版本 `VACUUM INTO` 备份、旧版本只读拒绝、迁移后回放测试通过。
7. 文档收口：`doc/index.md`/`doc/progress.md`/DR-004 执行记录/评审索引更新；各波 worktree 与分支清理。
8. release：notes、tag `v0.2.0`、GitHub Release。

---

## 7. 风险与应对

| 风险 | 应对 |
|---|---|
| W2a 通用返工语义（非自环回边）复杂度高 | W2a0 先冻结；仅 defer 边允许 attempt-per-activation；W2a DoD 含 A→B→A 反例 |
| defer 边无重入来源导致死锁 | 加载期提示/运行期 RUN_DEADLOCK；W2a 写明确契约与反例测试 |
| W3a 控制通道与 run 生命周期 | run 必须存活；trace DB 轮询（WAL）；decide 幂等/冲突契约；run 退出即不可 decide（W3b 才恢复） |
| durable run 滑向服务化 | W3b 单独确认；v0.2 发布条件默认只要求 W3a |
| W4 per-item 模型复杂度/回放 | W4a 先冻结；单端口 join；graph_hash 不变；展开事实入 trace |
| trace 多次迁移成本 | forward-only、逐版本备份、迁移测试；迁移点固定（W2a/W3a/W4b） |
| docs 与实现并行冲突 | docs 单写入者，按 §4 排队；各波实现不含 docs |
| 性能回归不可验收 | W1 记录基线；门禁量化（≤20%，同机 3 次中位数） |
| ISSUE-091/093 环境 flaky | 沿用重跑判定；私有 target；CI 独立 runner |

---

## 8. 编号与流程

- 本 DR 评审：**REV-063**（r3 由 REV-063 复审关闭）。
- 各波次 REV：从 **REV-064** 起按合并顺序分配；每波/每模块独立提交与评审（≠作者）；跨模块由 Lead 声明第二评审人。
- worktree：每模块一个 worktree/分支（`AGENTS.md` §2）；波次表见 §4。
- 每波合并后：更新 `doc/progress.md`；新问题登记 `doc/issues.md`；本 DR 追加「执行记录」段（不改原决策）。
- 每波完成时核对 §6 对应条目，避免收口遗漏。

---

## 9. 待用户确认（A）与评审默认（B）

### A. 需要你拍板

1. **范围**：W1–W4 纳入 v0.2，W5（共享评审队列）放 v0.3——是否接受？
2. **W3b durable**：默认只做 W3a（run 存活期间 decide/resume）；W3b（崩溃后恢复）**默认不纳入 v0.2 发布条件**。是否接受？若要「run 退出后还能 resume」，则 W3b 必须进 v0.2。
3. **人工接口**：CLI `decide`/`resume` + trace DB 轮询（无 HTTP/socket）是否够用？
4. **动态 N**：节点级 `foreach` + 单端口有序列表 join + `on_item_error=fail` 是否覆盖你的场景？
5. **扇出上限**：两层（加载期静态 + 运行期每激活）、缺省无上限，是否符合「每节点最大扇出限制」原意？
6. **返工默认值**：`on_gate_false` 缺省 `settle`、`max_attempts` 缺省 3、defer 边必须在环内——是否接受？
7. **版本/兼容**：`v0.2.0` / `M7` 命名 + v0.1.0 图/L1/L2 兼容硬条件——是否采用？

### B. 评审默认（已并入正文，无需逐项确认）

- trace 迁移 forward-only（D5）；v0.1.0 兼容矩阵（§6.3）；性能基线量化（§6.4）；W1 fake LLM + 手动 real-LLM smoke（W1）；docs 单写、e2e 纳入各波、跨模块第二评审（§4）；W3b 默认排除。

> A 组确认后即冻结；未确认项不进入实现。
