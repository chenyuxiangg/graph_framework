# DR-004 · v0.2（M7）范围与波次

> **状态**：已立项（用户 2026-10-31 指示「推送后开启下一个版本」）；评审记录 REV-063。
> **日期**：2026-10-31 · **作者**：lead
> **关联**：v0.1.0（tag `v0.1.0` / GitHub Release）、[`design/software-team-sop-v1.md`](../design/software-team-sop-v1.md)（REV-062）、[`DR-001`](DR-001-runtime-route.md)、[`DR-003`](DR-003-m6-scope-and-waves.md)、[`issues.md`](../issues.md)
> **前置事实**：M0–M6 已交付并合入 `main`；v0.1.0 已发布（tag `v0.1.0` @ `5d43c58`）；SOP 设计稿 v1 已合入（`7cb13ca`，REV-062）。

---

## 1. 背景与目标

### 1.1 背景

- v0.1.0 的能力边界（用户已知悉并接受）：无人工介入（`WaitingHuman` 不产生）、无图级返工（带出口环不可运行）、无每节点扇出上限、无动态 N、无共享评审队列、无框架级 artifact 端口。
- SOP 设计稿 v1 已把 `software_team` SOP 收敛为「设计角色依赖 DAG + 工作 agent + per-item 评审」，并明确上述能力延期到下一版本。
- 用户 2026-10-31 决定：**人工评审、图级有限返工、每节点最大扇出限制、动态 N 另起一个版本**；SOP 设计稿也需落地。

### 1.2 v0.2 目标（一句话）

**让 software_team SOP v1 真正可执行，并把图执行模型从"静态 DAG + 一次性实例"扩展到"可人工停等、可有限返工、可限制扇出、可按运行期数据展开 N"。**

### 1.3 非目标（本版本不做）

- API 服务 / 前端 / Agent 资产平台 / 多租户（不在产品范围）。
- 多 provider 路由/fallback、多模态、MCP、工具并行执行、断点续跑（除 W3 的 durable run 最小能力外）。
- 共享评审队列（v0.3 候选，见 §3.5）。
- 框架级 artifact 存储（SOP v1 以数据值传递；如需另立需求）。
- 历史数据重写（trace schema 迁移只做向前兼容）。

---

## 2. 版本定位与原则

- **版本号**：`v0.2.0`；里程碑代号 **M7**。
- **原则**：
  1. **先落地后改内核**：W1 只依赖 v0.1.0 已有能力，先让 SOP v1 跑通（fake LLM e2e），不阻塞后续波次。
  2. **一波一审批**：每个波次独立 worktree/分支/REV/合并，不一次性大爆炸；跨模块由 Lead 声明第二评审人。
  3. **语义变更必须回写 spec**：W2–W4 涉及 00/01/02/03/04/06/07 分册，波次内完成回写与 PROVENANCE 更新。
  4. **可复现不回退**：每波新增/变更事件必须补 L1/L2 回放回归；trace schema 变更需迁移测试。

---

## 3. 范围与工作项（4 个波次 + 1 个候选）

### W1 · SOP v1 落地（不改内核/DSL 语义）

**目标**：按 SOP 设计稿 §4.1 依赖矩阵 codegen 生成可 validate 的图，用 fake LLM 跑通 per-item 评审链路。

- 交付：
  - codegen（依赖矩阵 → 60 节点/100 边）；建议落 `tests/fixtures/graphs/software_team_v1/`（e2e）；
  - agent 资产：设计角色 ×11、共享 `design_reviewer`、工作 agent（LLM / python_node）；prompt/constraints；
  - `parse_verdict` python 节点（`python/workers/software_team/`）；
  - fake LLM 脚本 + e2e：产物透传、approved gate、失败路径（`NODE_PYTHON_EXCEPTION` / `HARNESS_MAX_ITERATIONS`）、下游 `trigger=all` 等待；
  - docs：07 分册补 AC（或登记为 v0.2 AC）。
- 模块：`e2e` + `py-worker`（+`docs`）——Lead 声明跨模块与第二评审人。
- DoD：codegen 产物 `zgraph validate` 通过；e2e 全绿；**不修改 kernel/dsl/harness 语义**。

### W2 · 图级有限返工 + 每节点最大扇出限制

**目标**：让"评审不通过 → 回到工作节点重做（有限次）→ 再评审"可图级表达；并给 fan-out 加上限。

- W2a 可重臂 gate / 带出口 cycle：
  - 现状：gate=false 立即 settle 并级联 `Skipped`；带出口环以 `RUN_DEADLOCK` / `RUN_DELIVERY_TO_TERMINAL` 失败（ISSUE-062 两种模式）。
  - 候选语义（评审时定稿）：(i) **deferrable gate**（false 不结算边，目标保持 Pending/等待下一次投递）；(ii) instance-per-activation；(iii) 显式 `max_attempts` 限次后失败。需同时定义与 `Skipped` 级联、termination/quiescence 的关系。
  - 模块：`kernel` + `dsl` + `trace` + `docs`；spec 01/02/03/06。
- W2b 每节点最大扇出限制：
  - 候选：DSL 节点/边新增 `max_fanout`（缺省无上限，向后兼容）；加载期校验 + 运行期放行计数；超限的失败语义（实例 Crashed / 边拦截 + trace）。
  - 模块：`dsl` + `kernel` + `docs`；spec 02/03。
- DoD：有限次返工环端到端通过（含最终失败路径）；扇出超限可观测；L1/L2 回放回归；spec 回写。

### W3 · 人工评审（human lifecycle）

**目标**：评审可由人（外部）决定；run 可停等、决策、恢复。

- 交付：
  - `WaitingHuman` 真正产生；外部 decide/resume（CLI 优先，无 HTTP）；`manual` gate 或 human 节点与 `RunControl` 的接线；
  - durable run（进程退出/重启后可恢复；最小范围：CLI 单机，不要求服务化）；
  - trace 记录决策（谁/何时/结论/理由）；CLI 退出码与 `--json`；
  - spec 00/01/03/04/06/07 + AC。
- 模块：`kernel` + `cli` + `trace` + `docs`（可能 `harness`）；跨模块双评审。
- DoD：run 在 human gate 暂停；外部 `decide approve/reject` 后恢复；重启后可继续；拒绝与 W2 返工衔接；e2e。

### W4 · 动态 N（运行期 fan-out）

**目标**：N 不再编译期固定，可从运行期输入/上游输出展开。

- 候选语义（评审时定稿）：DSL 图级 `map`/`foreach` 配置（输入端口 → 列表）；运行期按 item 生成实例并保持 per-item 路由；join 语义（all/any per item）；上限与扇出限制（与 W2b 衔接）。
- 模块：`dsl` + `kernel` + `trace` + `docs`；跨模块双评审；spec 01/03/06。
- DoD：N 由 entry input 列表决定；item 独立实例/独立 trace；join 正确；回放可复现；e2e。

### W5（候选，v0.3）· 共享评审队列

- 一个评审节点按到达顺序排队处理多份投递、评完一个放行一个；需要节点邮箱/多次激活/按 item 路由，依赖 W4 的 per-item 模型。
- 本版本不做；若 W4 完成后有余量再评估。

---

## 4. 波次顺序、依赖与模块

| 波次 | 依赖 | 主责模块 | 关键风险 |
|---|---|---|---|
| W1 | 无（只用 v0.1.0 能力） | e2e / py-worker / docs | agent 资产/prompt 质量；fake LLM 覆盖 |
| W2a | W1 并行（内核设计可先做） | kernel / dsl / trace / docs | gate 语义与 `Skipped`/termination 冲突；回放 |
| W2b | W2a（同波次内顺序） | dsl / kernel / docs | 向后兼容（缺省无上限） |
| W3 | W2a（返工/暂停的语义交互）；trace schema 串行 | kernel / cli / trace / docs | durable run 范围过大；schema 迁移 |
| W4 | W2（实例/扇出模型）；W3（trace schema 串行） | dsl / kernel / trace / docs | 动态实例与图 hash/回放；join 语义 |

- **串行约束**：`kernel`/`trace` 同一时间只有一个写入者；trace schema 变更按 W2 → W3 → W4 串行，每次一个迁移。
- **可并行**：W1 与 W2a 设计可并行；W2a 实现完成后 W2b 紧随；W3/W4 按依赖排。

---

## 5. 关键设计决策（提议，评审/用户确认后冻结）

| # | 决策 | 提议 |
|---|---|---|
| D1 | 返工语义 | **deferrable gate + 有限 attempt**：gate=false 不 settle 目标、不触发 `Skipped`，目标保持 Pending 等下一次投递；超过 `max_attempts` → 实例 Crashed → run Failed（默认值需与现有 `retry.max_attempts` 对齐） |
| D2 | 扇出上限 | DSL 节点级 `max_fanout`（可选，缺省无上限）；每次激活经 gate 放行的投递边数上限；超限 → 实例 Crashed（错误码新增需评审） |
| D3 | 人工接口 | CLI `zgraph decide <run_id> --instance <id> --decision approved\|rejected [--reason]` + 库 API；不做 HTTP；durable 状态存 trace |
| D4 | 动态 N | DSL 图/节点级 `foreach`：从指定端口取列表，每 item 生成实例；join = `trigger:all` 的 per-item 端口；上限复用 D2 |
| D5 | trace schema | W2/W3/W4 各一次前向迁移（v4/v5/v6）或一次合并迁移——评审时定；旧库只读兼容 |
| D6 | 复现兼容 | 新增事件/状态必须可通过 `replay --mode=events` 与 L2 outputs 回归；不破坏既有 golden |

---

## 6. 发布条件（v0.2.0）

1. W1–W4 全部合并；W5 不阻塞（候选）。
2. `cargo test --workspace`、`cargo clippy -D warnings`、pytest 全绿（基线 0）；CI 通过。
3. spec 00/01/02/03/04/06/07 回写 + PROVENANCE 更新；AC 新增项通过。
4. L1/L2 回放与 trace 迁移测试通过；性能基线不大幅回退（worker 并发/时长）。
5. release notes、tag `v0.2.0`、GitHub Release。

---

## 7. 风险与应对

| 风险 | 应对 |
|---|---|
| 内核语义变更破坏确定性/回放 | 每波补 L1/L2 golden；先写 spec 再改代码；AC 先行 |
| trace schema 多次迁移成本 | 串行迁移、向前兼容、迁移测试；必要时合并为一次 |
| 人工 durable run 变成服务化 | 明确 v0.2 仅 CLI 单机恢复；HTTP 服务留后续 |
| 动态 N 与静态图 hash/回放冲突 | 每 item 实例 ID 可复现；记录展开事实；graph_hash 不变（图 JSON 不变） |
| 范围蔓延 | 每波独立合并；W5 明确候选；W1 先行可交付 |
| ISSUE-091/093 环境 flaky | 沿用 ISSUE-091 处置（重跑判定）；worktree 私有 target |

---

## 8. 编号与流程

- 本 DR 评审：**REV-063**；各波次 REV 从 **REV-064** 起按合并顺序分配。
- 每个波次：`bash scripts/dev/worktree.sh new <module> <agent> <slug>` → 独立提交 → 独立评审（≠作者）→ `merge.sh`；跨模块由 Lead 声明第二评审人。
- 波次完成后更新 `doc/progress.md`；新问题登记 `doc/issues.md`；本 DR 追加"执行记录"段（不改原决策，只追加）。

---

## 9. 待用户/评审确认的开放问题

1. **v0.2 范围**：W1–W4 是否全部纳入 v0.2？共享队列是否接受放 v0.3？
2. **返工语义**：D1 的 deferrable gate（false 不 settle）是否符合你的预期？还是更倾向 instance-per-activation？
3. **人工接口**：D3 的 CLI `decide` 子命令（无 HTTP）是否够用？
4. **动态 N**：D4 的"从端口列表 foreach"是否覆盖你的场景？N 的来源是否需要运行期外部注入（而非入口输入）？
5. **扇出上限**：D2 的"每激活放行边数"是否符合"每节点最大扇出限制"的原意？
6. **命名**：版本号 `v0.2.0` / 里程碑 `M7` 是否采用？

> 上述问题在评审记录中逐项回复；未冻结项不进入实现。
