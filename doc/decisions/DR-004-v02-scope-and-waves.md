# DR-004 · v0.2（M7）范围与波次

> **状态**：已立项（用户 2026-10-31「推送后开启下一个版本」）；**r2 修订**（2026-10-31，关闭 REV-063 C1–C6）；评审记录 REV-063。
> **日期**：2026-10-31 · **作者**：lead
> **关联**：v0.1.0（tag `v0.1.0` / GitHub Release）、[`design/software-team-sop-v1.md`](../design/software-team-sop-v1.md)（REV-062）、[`DR-001`](DR-001-runtime-route.md)、[`DR-003`](DR-003-m6-scope-and-waves.md)、[`issues.md`](../issues.md)、spec v1.2 分册 00/01/02/03/04/06/07/08
> **前置事实**：M0–M6 已交付；v0.1.0 已发布（tag `v0.1.0` @ `5d43c58`）；SOP 设计稿 v1 已合入（`7cb13ca`，REV-062）。

---

## 1. 背景与目标

### 1.1 背景

- v0.1.0 能力边界（用户已知悉）：无人工介入（`WaitingHuman` 不产生）、无图级返工（带出口环会 `RUN_DEADLOCK` / `RUN_DELIVERY_TO_TERMINAL`）、无每节点扇出上限、无动态 N、无共享评审队列、无框架级 artifact 端口；`gate=false` 默认 settle 并级联 `Skipped`。
- SOP 设计稿 v1 把 `software_team` SOP 收敛为「设计角色依赖 DAG + 工作 agent + per-item 评审」，并把上述能力延期到下一版本。
- 用户 2026-10-31 决定：**人工评审、图级有限返工、每节点最大扇出限制、动态 N 另起一个版本**；SOP 设计稿也需落地。

### 1.2 v0.2 目标

**让 software_team SOP v1 真正可执行，并把图执行模型从「静态 DAG + 一次性实例」扩展为「可人工停等、可有限返工、可限制扇出、可按运行期数据展开 N」，同时保持 v0.1.0 的默认语义与回放兼容。**

### 1.3 非目标

- API 服务 / 前端 / Agent 资产平台 / 多租户。
- 多 provider 路由/fallback、多模态、MCP、工具并行执行（除 W3 的 durable 最小范围外，见 W3b）。
- 共享评审队列（v0.3 候选，§3.5）。
- 框架级 artifact 存储（SOP v1 以数据值传递）。
- 历史 trace 重写（只做 forward-only 迁移，见 D5）。

---

## 2. 版本定位与原则

- **版本号**：`v0.2.0`；里程碑 **M7**。
- **原则**：
  1. **默认行为不变**：v0.1.0 图的 gate/Skipped/回放语义是兼容基线；所有新语义**opt-in**（D1/D2）。
  2. **先落地后改内核**：W1 只依赖 v0.1.0 能力，先跑通 SOP v1，不阻塞后续波次。
  3. **一波一审批、按模块拆 worktree**：每波可含多个模块分支，**每个 worktree 只服务一个模块**；跨模块由 Lead 声明第二评审人（AGENTS §2）。
  4. **docs 单写入者**：同一时间只允许一个 `docs` 分支；各波 spec 回写按波次串行排期。
  5. **语义变更必须回写 spec**：W2–W4 涉及 00/01/02/03/04/06/07/08 分册；每波内完成回写与 PROVENANCE 更新。
  6. **可复现不回退**：新事件/状态必须补 L1/L2 回归；trace 只做 forward-only 迁移（D5）。

---

## 3. 范围与工作项

### W1 · SOP v1 落地（不改内核/DSL 语义）

**目标**：按 SOP 设计稿 §4.1 依赖矩阵 codegen 出可 validate 的图，用 fake LLM 跑通 per-item 评审链路。

- 交付（模块：`e2e` + `py-worker`；docs 回写单独排期）：
  - codegen（依赖矩阵 → 60 节点/100 边），落 `tests/fixtures/graphs/software_team_v1/`（e2e）；
  - agent 资产：11 个设计角色、共享 `design_reviewer`、工作 agent（`agent_call` / `python_node`）、prompt/constraints；
  - `parse_verdict` python 节点（`python/workers/software_team/`，py-worker）；
  - fake LLM 脚本 + e2e：产物透传、approved gate、失败路径（`NODE_PYTHON_EXCEPTION` / `HARNESS_MAX_ITERATIONS`）、下游 `trigger=all` 等待、per-item 实例/trace；
  - 07 分册 AC 新增/回写（docs 子任务）。
- **fake LLM 限制（S1）**：CLI 单 provider、fake 脚本按调用序消费（`fake.rs`），60 节点并发只能验证**编排/门控/产物链路**，不能验证 prompt 质量；W1 另加**手动 real-LLM smoke**（`--real`，不进 CI），并在 AC 注明。
- DoD：codegen 产物 `zgraph validate` 通过；e2e 全绿；**不修改 kernel/dsl/harness 语义**；codegen 可重复生成同一 graph_hash。

### W2 · 图级有限返工 + 每节点最大扇出限制

**目标**：让「评审不通过 → 回到工作节点重做（有限次）→ 再评审」可图级表达；并给 fan-out 加上限。

#### W2a · opt-in deferrable gate（可延后 gate）+ 有限返工

- **默认不变**：新增 **边级可选字段** `on_gate_false: "settle" | "defer"`（缺省 `settle`）；v0.1.0 图不受影响。
- `settle`（缺省）：维持 v0.1.0 行为（拒绝的边结算、目标级联 `Skipped`）。
- `defer`：gate=false **不结算边、不触发 Skipped**，目标保持 Pending；产者在新 attempt（自环/显式 rework 边）重新执行后，边再次求值。
- **重入来源**：deferrable 边必须位于可重入结构（自环或显式 rework 边）；每次重入创建 `attempt+1` 实例；`max_attempts` 对齐现有默认 3，超限 → 实例 Crashed → run Failed。
- **终止/死锁**：存在未决 deferred 边时 run 不得判静默完成；若没有任何可能的后续投递（无可重入来源）→ watchdog `RUN_DEADLOCK`。
- **trace/回放**：`GateResult` 增加 `deferred=true` 与 `attempt`；L1/L2 golden 覆盖 defer 路径；v0.1.0 golden 必须原样通过。
- 模块：`kernel` + `dsl` + `trace` + `e2e` + `docs`；spec 01/02/03/06。

#### W2b · 每节点最大扇出限制（两层）

- **静态层**：DSL 节点级 `max_fanout`（可选，整数 ≥1，缺省无上限）；加载期校验静态 out-degree；超限 → DSL 错误码（新增，02 R4/R9 白名单）。
- **运行期层**：单次激活经 gate 放行的投递边数上限（同一字段或节点配置），超限 → 实例 Crashed（新运行期错误码 + trace），**不静默截断**。
- 模块：`dsl` + `kernel` + `e2e` + `docs`；spec 02/03；默认无上限保证向后兼容。
- DoD：defer 返工环端到端（含最终失败）；v0.1.0 图/回放 golden 不变；fan-out 两层限制分别有反例；spec 回写。

### W3 · 人工评审（human lifecycle）

#### W3a · 人工停等 + decide/resume（v0.2 必须）

- `WaitingHuman` 真正产生；外部 CLI：`zgraph decide <run_id> --instance <id> --decision approved|rejected --approver <id> [--reason ...]`（不自动接续时补 `zgraph resume`）。
- 契约：decide 幂等（重复/冲突决定的行为明确）、approver 记录、拒绝去向矩阵（与 W2a 返工/终止衔接）、退出码与 `--json`、trace 决策事件。
- 模块：`kernel` + `cli` + `trace` + `e2e` + `docs`；spec 00/01/03/04/06/07；**不涉及 harness**（v0.2 human 决策由内核/CLI 承担）。
- DoD：run 在 human gate 暂停 → `decide approve` 后恢复；`reject` 按矩阵终止或进入 rework（W2a）；决策可审计；e2e。

#### W3b · durable resume（可选，需用户确认；未确认则不纳入 v0.2 发布条件）

- 范围：进程崩溃/重启后的恢复；recovery 状态机、`resume` CLI、Running 实例/worker 处置（06 §6.8.1 目前只标 interrupted，不恢复）、L3 幂等边界。
- 模块：`trace` + `kernel` + `cli` + `e2e` + `docs`；spec 06/07。
- DoD：崩溃后 `resume` 可继续/明确终止；恢复后 trace 一致；e2e。

### W4 · 动态 N（运行期 fan-out）

#### W4a · 设计冻结（先设计后实现）

- 冻结 per-item 模型：map 来源（entry input 或上游输出端口中的**列表**）、item key 确定性、运行期展开、下游 per-item 实例；
- **join**：单端口接收**有序 item 结果列表**（避免动态端口）；`trigger=all` 的语义为「所有 item 结果到齐」；默认 `on_item_error: "fail"`（任一 item 失败 → run Failed）；
- 失败/取消/上限/回放：run cancel 传播到所有 item 实例；每激活上限复用 W2b；trace 记录展开事实（item 数/key）；`graph_hash` 只随图 JSON 变化；
- 产出 W4 spec 增补草案（作为下一波实现依据）。

#### W4b · 实现

- DSL 图/节点级 `foreach` 配置、kernel 运行期展开与 per-item 路由、trace 记录、e2e。
- 模块：`dsl` + `kernel` + `trace` + `e2e` + `docs`；spec 01/03/06；第二评审。
- DoD：N 由 entry input/上游列表决定；item 独立实例/trace；join 正确；失败/取消/上限符合 W4a；回放可复现；e2e。

### W5（候选，v0.3）· 共享评审队列

- 一个评审节点按到达顺序排队处理多份投递、评完一个放行一个；需要节点邮箱/多次激活/按 item 路由，依赖 W4。
- **v0.2 不做**；W4 完成后评估 v0.3。

---

## 4. 波次、分支、模块与评审

> 每波可有多个模块分支；**一个 worktree 只服务一个模块**；跨模块分支必须由 Lead 指定第二评审人。

| 波次 | 分支（建议） | 模块 | 依赖 | 评审 |
|---|---|---|---|---|
| W1 | `test/e2e-software-team-v1`、`feat/py-worker-parse-verdict` | e2e / py-worker | 无 | 独立评审 + 跨模块第二评审 |
| W1-docs | `docs/docs-sop-v1-ac` | docs | W1 合并后 | 独立评审（docs 串行） |
| W2a | `feat/kernel-deferrable-gate`、`feat/dsl-gate-defer`、`feat/trace-gate-defer` | kernel / dsl / trace / e2e | W1 可并行（不碰 docs） | 主审 + 第二评审（跨模块） |
| W2a-docs | `docs/docs-gate-defer-spec` | docs | W2a 合并后 | 独立评审（docs 串行） |
| W2b | `feat/dsl-max-fanout`、`feat/kernel-max-fanout` | dsl / kernel / e2e | W2a | 主审 + 第二评审 |
| W2b-docs | `docs/docs-fanout-spec` | docs | W2b 合并后 | 独立评审 |
| W3a | `feat/kernel-human-wait`、`feat/cli-decide`、`feat/trace-human` | kernel / cli / trace / e2e | W2a | 主审 + 第二评审 |
| W3a-docs | `docs/docs-human-spec` | docs | W3a 合并后 | 独立评审 |
| W3b（可选） | 待用户确认后定 | trace / kernel / cli / e2e | W3a | 主审 + 第二评审 |
| W4a | `docs/docs-dynamic-fanout-design` | docs（设计） | W2b + W3a 定型 | 独立评审 |
| W4b | `feat/dsl-foreach`、`feat/kernel-dynamic-fanout`、`feat/trace-dynamic` | dsl / kernel / trace / e2e | W4a | 主审 + 第二评审 |
| W4b-docs | `docs/docs-dynamic-spec` | docs | W4b 合并后 | 独立评审 |

- **串行约束**：`kernel`/`trace` 同一时间一个写入者；trace schema 变更按 W2a → W3a → W4b 串行；docs 同一时间一个分支。
- **并行**：W1（e2e/py-worker）可与 W2a 的 kernel/dsl 工作并行；docs 回写排在其后。

---

## 5. 关键设计决策

| # | 决策 | 内容 |
|---|---|---|
| D1 | 返工语义 | **opt-in deferrable gate**：`on_gate_false: "settle"\|"defer"`（缺省 settle）；defer 不结算边/不 Skipped/目标 Pending；重入靠自环或显式 rework 边（attempt+1）；`max_attempts` 缺省 3；超限 Failed；未决 deferred 边阻止静默完成，无可重入来源 → RUN_DEADLOCK；trace 记 `deferred`+attempt |
| D2 | 扇出上限 | 两层：DSL `max_fanout`（加载期静态 out-degree，超限 DSL 错误）+ 运行期每激活放行边数上限（超限实例 Crashed + 错误码 + trace）；缺省无上限；不静默截断 |
| D3 | 人工接口 | CLI `decide` + `resume`（单机，无 HTTP）；decide 幂等/冲突语义、approver、拒绝去向矩阵、退出码/`--json`、trace 决策事件 |
| D4 | 动态 N | map 来源 = entry input 或上游输出列表；item key 确定性；join = 单端口有序 item 结果列表（trigger=all）；默认 `on_item_error=fail`；取消传播、每激活上限复用 D2；trace 记录展开；graph_hash 只随图 JSON |
| D5 | trace 迁移 | **forward-only**（spec 06 §6.7）：新代码迁移旧库后使用；只读 `trace`/`replay` 对旧版本**拒绝且不迁移**（退出码 4）；每波一次迁移（W2a/W3a/W4b）或由 W2a 决定合并，含逐版本备份与迁移测试；不承诺新代码直接读旧 schema |
| D6 | 复现兼容 | 新增事件/状态必须通过 `replay --mode=events` 与 L2 outputs；v0.1.0 golden 为硬回归；旧 DB 迁移后可回放 |
| D7 | 向后兼容 | v0.1.0 图的 gate/Skipped/回放默认语义不变；新字段全部可选、缺省行为 = v0.1.0 |

---

## 6. 发布条件（v0.2.0）

1. W1–W4 合并（W3b 视用户确认；W5 不阻塞）。
2. 质量门禁：`cargo test --workspace`、`cargo clippy -D warnings`、pytest 全绿（基线 0）；CI 通过。
3. **向后兼容回归**：v0.1.0 的 `tests/fixtures/graphs/**` 全部 validate/run 通过；v0.1.0 生成的 trace DB 经迁移后 `trace`/`replay` 通过；v0.1.0 golden 事件/输出不变（D6）。
4. **性能基线（可验收）**：同机、无外部 CPU 争用、各 3 次取中位数；对比 v0.1.0 基线（W1 记录到 `doc/perf-baseline-v0.2.txt`）：AC-01 单 run 与 AC-12 10 并发 run 的 duration 中位数回退 ≤ 20%；W1 SOP N=3 e2e 记录绝对基线，v0.2 内不劣化 > 20%。
5. spec 回写：00/01/02/03/04/06/07/08 按波次完成 + PROVENANCE 更新；07 新增 AC 通过。
6. trace 迁移：迁移矩阵、备份（`VACUUM INTO`）、旧版本只读拒绝、迁移后回放测试通过。
7. 文档收口：`doc/index.md`/`doc/progress.md`/DR-004 执行记录/评审索引更新；各波 worktree 与分支清理。
8. release：notes、tag `v0.2.0`、GitHub Release。

---

## 7. 风险与应对

| 风险 | 应对 |
|---|---|
| W2a defer 语义破坏 v0.1.0 契约/回放 | opt-in + 缺省 settle；v0.1.0 golden 硬回归；spec 先冻结 |
| defer 边无重入来源导致死锁 | 加载期提示/运行期 RUN_DEADLOCK；W2a 写明确契约与反例测试 |
| durable run 滑向服务化 | W3b 单独确认；v0.2 发布条件默认只要求 W3a |
| W4 per-item 模型复杂度/回放 | W4a 先冻结；单端口 join；graph_hash 不变；展开事实入 trace |
| trace 多次迁移成本 | forward-only、逐版本备份、迁移测试；可合并为一次（W2a 定） |
| docs 与实现并行冲突 | docs 单写入者，按波次串行；各波实现不含 docs |
| 性能回归不可验收 | W1 记录基线文件；发布条件量化（≤20%） |
| ISSUE-091/093 环境 flaky | 沿用重跑判定；私有 target；CI 独立 runner |

---

## 8. 编号与流程

- 本 DR 评审：**REV-063**（r2 由 REV-063 复审关闭）。
- 各波次 REV：从 **REV-064** 起按合并顺序分配；每波/每模块独立提交与评审（≠作者）；跨模块由 Lead 声明第二评审人。
- worktree：每模块一个 worktree/分支（`AGENTS.md` §2）；波次表见 §4。
- 每波合并后：更新 `doc/progress.md`；新问题登记 `doc/issues.md`；本 DR 追加「执行记录」段（不改原决策）。
- 波次完成时核对 §6 对应条目，避免收口遗漏。

---

## 9. 待用户确认（附评审建议）

1. **W3b durable 是否纳入 v0.2 必须范围？** 评审建议：W3a（停等/decide/resume）必须，W3b（崩溃恢复）默认**不纳入 v0.2 发布条件**，另行确认后再做。→ 请确认是否接受。
2. **W4 join 模型**：评审建议单端口接收有序 item 结果列表、默认 `on_item_error=fail`；请确认是否接受（否则需在 W4a 另议动态端口）。
3. **扇出上限**：评审建议两层（加载期 DSL 校验 + 运行期每激活上限）、缺省无上限；请确认是否符合「每节点最大扇出限制」的原意。
4. **版本与兼容**：确认 `v0.2.0` / `M7` 命名，并把「v0.1.0 图/L1/L2 回放兼容」作为硬性发布条件。
5. **返工默认值**：`on_gate_false` 缺省 `settle`、`max_attempts` 缺省 3 是否接受。

> 上述问题的建议答复已并入正文（D1/D2/D4/§6）；用户确认后即冻结，未确认项不进入实现。
