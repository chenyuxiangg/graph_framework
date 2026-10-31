# REV-063 · docs · v0.2（M7）范围与波次（DR-004）

- **分支**：`docs/docs-v02-kickoff`
- **作者**：`lead`（提交 `16204c1`；提交正文含 `Review: REV-063` / `Agent: lead`）
- **评审人**：`reviewer-sop`（**≠ 作者**；独立评审）
- **变更范围**：`e651ae5..16204c1`，2 文件 +167/−0：新增 `doc/decisions/DR-004-v02-scope-and-waves.md`（166 行）、`doc/index.md` +1 行索引；全部在 `doc/**`，**无代码/测试/配置改动**；base 即当前 `origin/main`（`e651ae5`）；worktree clean；`v0.1.0` tag 指向 `5d43c58`（与 DR 前置事实一致）
- **评审时间**：2026-10-31
- **关联**：DR-004（本文）、DR-003（M6 决策/波次格式先例）、DR-001（技术路线）、[`design/software-team-sop-v1.md`](../design/software-team-sop-v1.md)（REV-062）、v0.1.0（tag `v0.1.0` @ `5d43c58`）、ISSUE-062/091/093

## 1. 结论

**结论：有条件通过**

方向判定：DR 与用户 2026-10-31 的口径一致——W1 SOP v1 落地、W2 图级有限返工 + 每节点扇出上限、W3 人工评审、W4 动态 N；共享评审队列明确列为 **v0.3 候选**（§3 W5 / §1.3），未误写进 v0.2；v0.1.0 能力边界（`WaitingHuman` 不产生、带出口环不可运行、无扇出上限/动态 N/共享队列/框架级 artifact）与仓库现状一致；W1 只依赖 v0.1.0 能力的方向基本成立（W1 的 fake LLM 边界见 S1）。

但 DR 有 **1 项阻塞 + 5 项重要**：D1 以默认行为改 `gate=false` 语义（破坏 v0.1.0 契约与回放，且未定义 quiescence/Skipped/trace）；W3 durable run 范围与恢复契约未定义且未让用户确认；W4 per-item 路由/join/端口模型未定义；D5 trace 迁移策略表述与 spec 06 的 forward-only/只读拒绝口径不符；波次-模块-REV-评审人计划与 AGENTS §2 冲突（docs 并行写、W2–W4 缺 e2e 模块/双评审）；发布 DoD 中"性能基线不大幅回退"不可验收。**C1–C6 全部关闭前不得合并**（`merge.sh` 只接受记录结论为"通过"）。

> 评审口径：本记录按四维 + 可执行性核对；§7 给出对 DR §9 六项开放问题的建议答复及建议补充项。

## 2. 四维评审

| 维度 | 结论 | 依据（DR 行 + 仓库事实） |
|---|---|---|
| 功能正确性 | ⚠️ | 范围/方向与用户口径一致（§1.1:14-16、§3:43-94、§1.3:22-28）；v0.1.0 能力边界逐项与 spec/实现一致（00 §0.3 `WaitingHuman` 不产生；ISSUE-062 两种环失败模式；`TRACE_SCHEMA_VERSION=3`；无 `decide/resume` CLI）。W1 的"只依赖 v0.1.0"基本成立（SOP 设计稿 §4.1/§7 的 60 节点/100 边、`agent_call`/`python_node`/gate/trigger all/entry_inputs 都在 v0.1.0 能力内）。但 D1 改默认 gate 语义（C1）、W3 durable 范围（C2）、W4 join 模型（C3）三项影响"能否真正落地/是否破坏旧行为" |
| 代码结构合理性（文档/计划结构） | ⚠️ | DR 结构清晰：背景→目标/非目标→版本原则→范围→波次依赖→决策→发布条件→风险→编号流程→开放问题，信息完整；决策以"提议、评审/用户确认后冻结"标注（§5:113）。但与 DR-003 §1/§2 的成熟格式相比，缺 **每波次-分支-模块-REV-评审人表**、每波 DoD 的命令级验收、跨模块第二评审人具体声明；且模块计划存在 docs 并行写、W2–W4 缺 e2e 模块等问题（C5） |
| 逻辑性 | ⚠️ | 波次依赖主线可读（W1→W2a→W2b/W3→W4；trace schema 串行 §4:108），但存在内部不自洽：D1 的"deferrable gate + 有限 attempt"未选 W2a 候选 (ii) instance-per-activation，单靠 deferrable gate 无法让非自环重入；D2 说"节点级 max_fanout"而 W2b:67 说"节点/边新增"；D4 说"图/节点级 foreach"而 W4:87 说"图级 map/foreach"；D5"旧库只读兼容"与 spec 06 §6.7 的 forward-only + 只读连接拒绝旧版本不符；§8:152"每个波次独立 worktree/分支"与 AGENTS §2"一个 worktree 一个模块/同一模块一个写入者"冲突 |
| 可读性 | ⚠️ | 标题/状态/关联/非目标/开放问题齐全，决策表可读；但术语未统一（"可重臂 gate" W2a:62 vs "deferrable gate" D1:117；"断点续跑"§1.3:25 vs W3 durable run；"图级/图/节点级"foreach），且部分关键语义（D1 的 Pending/终止、W4 的 join、D5 的迁移粒度）只给一句，需读者自行推断 |

## 3. 发现清单

| # | 级别 | 位置（DR-004） | 问题 | 建议 | 状态 |
|---|---|---|---|---|---|
| C1 | **阻塞** | `§5 D1:117`（关联 `W2a:62-64`、`§7:140`） | **D1 以默认行为改 `gate=false` 语义，属破坏性变更且未定义终止语义**。现状：gate=false → 结算边（无数据）+ 级联 `Skipped`（01 §1.4、03 §3.4.2；`zgraph-kernel/src/run.rs:1010-1035` 实测 gate=false 只结算边 + Skipped，run 仍 `Completed(null)`）；这是 v0.1.0 契约与既有 e2e/L1/L2 golden 的基础。D1 提议"gate=false 不 settle 目标、不触发 Skipped，目标保持 Pending 等下一次投递"作为**通用** gate 语义——(a) 会改变 v0.1.0 图的既有行为与回放；(b) 静态图无环/无重入时"下一次投递"不存在，目标 Pending 将在 `deadlock_grace` 后触发 `RUN_DEADLOCK`（03 §3.9.3/§3.15）；(c) 未定义 deferred 与 `Skipped`/quiescence/termination 的关系，也未定义 trace 如何区分"拦截并 settle"与"延后等待"（`GateResult.passed=false` 现语义是前者，需新事件/schema v4）；(d) D1 未采纳 W2a 候选 (ii) instance-per-activation，非自环返工没有再次投递的来源 | 改为 **opt-in**（如边/节点 `on_gate_false: settle\|defer`，缺省保持 v0.1.0 的 settle+Skipped），或引入专用 rework 边语义；明确 deferred 的终止（何时 `RUN_DEADLOCK`/何时继续等）、与 Skipped 的交互、取消、trace 事件与 L1/L2 回放；W2a 只对显式开启的图生效；补 v0.1.0 图/回放金标回归。若用户确认要改默认语义，则需版本化声明并由 spec 00/01/03/04 同步 | open |
| C2 | **重要** | `W3:71-81`（尤其 `:77`）、`D3:119`、`§6:128`、`§9 Q3:161` | **W3 durable run 范围/恢复契约未定义，且未让用户确认是否纳入 v0.2**。DR 把"进程退出/重启后可恢复"放进 W3（`:77`）并作为发布条件（W1–W4 全合并，`:128`），但未定义：crash 一致性（Running 实例/worker 子进程/外部副作用/L3 幂等）、恢复入口（CLI 现有命令只有 validate/run/trace/replay，无 `decide/resume`；D3 的 `decide` 在 run 进程退出后如何触发恢复？）、运行状态重建（06 §6.8.1 的 `mark_interrupted` 只把 Running 标成 interrupted，不恢复实例；trace 无运行快照，需事件重放/快照）、`--json`/退出码/审批身份契约。§9 Q3 只问"CLI decide 是否够用"，没有让用户确认 durable run 是否属于 v0.2 必须项（相对"人工评审"这一原始需求，durable 是可选放大器） | 在 DR 中明确 W3 最小 durable 语义：human Pending 状态可跨进程恢复；Running 在飞实例的 interrupted/幂等重派发策略；新增 `zgraph resume <run_id>`（或 `decide` 内自动接续）契约与崩溃一致性测试；给出 CLI 退出码/`--json`/审批人字段。§9 增加"durable 是否 v0.2 必须/是否拆 W3a（human wait）+ W3b（durable）"的用户拍板项 | open |
| C3 | **重要** | `W4:83-89`、`D4:120`、`§9 Q4:162` | **dynamic N 的 per-item 路由/join/端口模型未定义，D4 的"join = trigger:all 的 per-item 端口"无法在 v0.1.0 静态端口模型直接表达**。v0.1.0 端口是节点声明期静态端口（02 R4/R9/R10/R11：`(to_node,to_input_port)` 唯一、required 端口必须有入边）；"per-item 端口"不存在声明方式。DR 未定义：多 item 输出如何进入下游（单端口收列表/动态端口/per-item 下游实例）、部分失败/取消/重试、上限与 D2 衔接、item id/回放可复现、与 `graph_hash` 的关系（`:143` 说 graph_hash 不变，但若 foreach 配置进入图 JSON 则 hash 会变，需澄清"N 不在 hash"边界）。§9 Q4 只问 N 来源，未问 join/失败语义 | 先定 per-item 实例与 join 模型（推荐：foreach 展开 per-item 实例；join 节点用单端口收 item 列表，或明确定义动态端口/映射）；给出失败/取消/部分完成/上限/回放策略并写进 D4/spec 01/03/06 草案；§9 增加 join/失败语义的用户确认项 | open |
| C4 | **重要** | `D5:121`、`§6:131`、`§7:141` | **trace schema 迁移策略表述与现状不符且未冻结**。当前 `TRACE_SCHEMA_VERSION = 3`（`crates/zgraph-trace/src/schema.rs:24`）；06 §6.7 是 forward-only 迁移，`trace`/`replay` 只读连接对旧版本**拒绝**（"只读连接版本 < 3 → 拒绝且不迁移"），并非 D5 所写"旧库只读兼容"；D5 仍把"W2/W3/W4 各一次 v4/v5/v6 还是一次合并"留到评审时定。W2（deferred gate 新事件）、W3（human 决策事件/状态）、W4（dynamic N 事件）对 trace schema 的依赖与迁移测试基线因此未定 | 在 DR 冻结迁移策略（建议 forward-only、每波一次 v4/v5/v6——W2/W3/W4 各一次，保持"每波独立评审/合并"；或明确合并为一次 v4 并冻结事件闭集）；给出迁移矩阵、备份/失败恢复、只读拒绝、旧库回放兼容测试；把"旧库只读兼容"改写为准确口径 | open |
| C5 | **重要** | `§3 W1:55/W2a:65/W2b:68/W3:80/W4:88`、`§4:100-109`、`§8:151-152` | **执行/模块计划与 AGENTS §2 不一致，不能直接照做**：(a) W1 与 W2a 都列 `docs` 模块；若并行实施（§4:109"W1 与 W2a 设计可并行"未限定仅设计），会出现两个 docs 写入者，违反 AGENTS §2"同一模块同一时间只允许一个写入者"；也未声明 docs 写回属哪一波、如何串行。(b) W2–W4 的 DoD 都要求 e2e（`:69/:81/:89`），但模块列表未含 `e2e`（AGENTS 模块表：`e2e = tests/e2e/**`），应显式加入并指定第二评审人。(c) W2a（kernel/dsl/trace/docs）与 W2b（dsl/kernel/docs）是多模块，DR 未像 W1/W3/W4 那样写"跨模块双评审"。(d) §8:152"每个波次独立 worktree/分支/REV"与"一个 worktree 只服务一个模块"冲突；应像 DR-003 §2 一样给出每波次-分支-模块-REV-评审人表（含计划外插入/未创建编号的处理）。(e) W3 模块写"`kernel/cli/trace/docs`（可能 `harness`）"，"可能"使工作树/评审范围不可执行 | 补"波次—分支—模块—REV—评审人"表（W2a/W2b 拆成独立分支/REV 或明确同一分支的单一 writer）；docs 写回集中到独立 docs 分支顺序执行；W2–W4 显式加 `e2e` 模块与第二评审；W3 决定是否含 `harness` 并写死；明确跨模块由 Lead 声明第二评审人 | open |
| C6 | **重要** | `§6:126-132`（尤其 `:130-131`） | **发布条件/DoD 不完整或不可验收**：①"性能基线不大幅回退（worker 并发/时长）"无基线数值、阈值或测量方法，无法判定发布；②缺 v0.1.0 图/回放向后兼容回归（直接关系 C1）；③缺文档/流程收口（progress/issues/README/index/reviews 索引、worktree 清理）与 v0.2 wrapup 评审；④spec 回写清单 00/01/02/03/04/06/07 漏 08 审计关闭矩阵（新 AC/ISSUE 关闭需要 08）与 05（若 W3 触及 harness）；⑤"spec ...回写"未指明每波完成还是 v0.2 收口统一回写 | 定义可量化性能基准（v0.1.0 指标 + 允许回退阈值或绝对上限）或删除该条件改入风险；补 v0.1.0 兼容回归、文档/流程收口、worktree 清理、v0.2 wrapup REV；补 08（和必要时的 05）回写；每波 DoD 写成可执行命令/断言（validate、e2e、pytest、clippy、replay） | open |
| S1 | 建议 | `W1:53,56`、`§7:144` | W1 的 fake LLM e2e 断言强度/可复现性未说明：v0.1.0 `FakeLlmProvider` 是**单 run 共享、按调用序消费**的脚本（`fake.rs:87,95,151`；CLI 只构造一个 provider：`run_cmd.rs:733`、`executor.rs:40`）。60 节点/约 43 个 `agent_call` 并发时，无法按节点/角色稳定分配不同响应；若各 agent 都返回同一 generic "approved JSON" 则只能验证编排/门控，不能验证 agent 资产/prompt 质量（§7 风险只列"agent 资产/prompt 质量；fake LLM 覆盖"而无应对）。另外 W1 未说明 codegen 脚本/生成物/assets 的归属与可复现命令（`tests/fixtures/graphs/software_team_v1/` 是数据目录，codegen 脚本落点未定） | W1 明确 e2e 断言边界（orchestration 而非 prompt 质量）；补 real-LLM 手动 smoke（或 fake provider 按 agent/prompt 路由扩展——会触及 harness 模块，需第二评审）；写明 codegen 脚本路径、生成命令、生成物是否提交与 assets/prompt 归属 | open |
| S2 | 建议 | `W2a:62` vs `D1:117`；`W2b:67` vs `D2:118`；`W4:87` vs `D4:120` | 术语/字段口径不一致：①"可重臂 gate"（W2a）未定义，与 D1"deferrable gate"是否同一概念？②W2b:67"DSL 节点/边新增 max_fanout"与 D2:118"DSL 节点级 max_fanout"不一致（放节点还是边/两者？）。③W4:87"图级 map/foreach"与 D4:120"图/节点级 foreach"不一致。④D1 只选 deferrable+max_attempts，未处理 W2a 候选 (ii) instance-per-activation；对非自环重入，仅 deferrable gate 不够 | 在 §5 前先统一术语/字段位置：给出"可重臂/deferrable/instance-per-activation"定义与关系；明确 `max_fanout` 放节点还是边（或两者，语义不同）；统一 foreach 为图级或节点级；D1 与 W2a 候选合并成一张"返工语义备选表"供用户选 | open |
| S3 | 建议 | `D2:118`、`W2b:67` | 扇出超限失败语义需分层：静态图的 out-degree 超过 `max_fanout` 属 **DSL/配置错误**，应在加载期报 `DSL_*`（fail-fast），而不是运行期实例 Crashed；动态 N 的运行期超限才走运行期错误（需定义错误码/trace/是否 rollback/是否 run Failed）。D2 只给"超限 → 实例 Crashed（错误码新增需评审）"，会混淆两类错误 | 区分加载期/运行期：加载期 `DSL_INVALID_PORT`/新增 DSL 错误；运行期新增确定错误码（如 `NODE_*`/`RUN_*`）+ `GateResult`/新事件；默认无上限保持向后兼容；错误码入 00 §0.4/08 矩阵 | open |
| S4 | 建议 | `§9:157-166` | §9 开放问题覆盖不足：缺（a）W3 durable run 是否 v0.2 必须/是否拆分；（b）v0.1.0 图与 L1/L2 回放向后兼容是否硬约束（直接决定 D1 能否改默认 gate 语义）；（c）W4 join/失败语义（不只 N 来源）；（d）D5 迁移粒度（各波 vs 合并）；（e）W1 fake vs real LLM 验收口径/资产质量；（f）docs 模块并行写约束、W2–W4 e2e/第二评审；（g）性能基线定义。这些是用户/评审需要拍板的阻塞性未决项，不应留到 wave 内部 | 把上述问题补入 §9（或 §5 决策表）；每条给出建议默认值（见 §7），让用户只需确认/否决；明确"未冻结项不进入实现"的检查点（每波 kickoff 的 DR 或评审记录） | open |
| S5 | 建议 | `§1.3:25`、`W3:73/77`、`§4:109`、`§8:152` | 可读性/口径小问题：①"断点续跑（除 W3 的 durable run 最小能力外）"与 W3"durable run（进程退出/重启后可恢复）"关系易混，需说明是同一能力的不同表述还是两层范围；②§4:109"W1 与 W2a 设计可并行"应明确仅设计（不写代码/不改 docs），实施顺序仍 W1 → W2a；③§8:152 "每个波次独立 worktree/分支"与模块边界的关系见 C5 | 在非目标/原则中补一句 durable 范围定义；§4 明确 "design-only 并行；实现按 W1 → W2a → W2b → W3 → W4"；§8 与 C5 的表统一 | open |

> 级别口径：阻塞 = 必须修复后才能给"通过"；重要 = 必须修复或登记 ISSUE；建议 = 可后置，但需修复或进台账。

## 4. 验证命令与结果

```bash
# --- 范围/分支/基线（工作树内）---
cd /Users/zyzs/lczyzs/code/zgraph/.worktrees/docs-lead
git status --porcelain --branch            # ## docs/docs-v02-kickoff...origin/main [ahead 1]；clean
git diff e651ae5..HEAD --stat              # DR-004 166 +++、doc/index.md 1 +；2 files +167
git diff e651ae5..HEAD --name-status       # A doc/decisions/DR-004...；M doc/index.md
git rev-parse origin/main v0.1.0^{commit}  # e651ae5... / 5d43c58...（base = origin/main；tag 与 DR 一致）
git show -s --format='%H %s' v0.1.0        # tag v0.1.0（annotated），指向 5d43c58

# --- 仓库自检（工作树内）---
bash scripts/dev/check.sh
# OK user.name=chenyuxiangg / OK user.email=... / OK remote=git@github.com:chenyuxiangg/graph_framework.git
# OK CLAUDE.md 未被修改 / OK 暂存区无禁用文件 / OK 分支命名: docs/docs-v02-kickoff
# OK Python 语法检查通过 / OK doc/progress.md 存在 / OK doc/issues.md 存在
# OK doc/progress.md 非空行 14（目标 ≤30）/ OK cargo fmt --check 通过 / OK git diff --check 通过
# CHECK PASSED  [exit=0]

# --- v0.1.0 能力边界核对 ---
grep -n "WaitingHuman" doc/spec-v1.2/00-总纲.md
# :64 保留但 v1 不产生：WaitingHuman（人工介入 v1.1）→ DR §1.1 属实
grep -n "ISSUE-062" doc/issues.md
# :70 valid_cycle 首轮 RUN_DEADLOCK / attempt1 RUN_DELIVERY_TO_TERMINAL（两种模式）→ DR W2a 属实
grep -n "TRACE_SCHEMA_VERSION" crates/zgraph-trace/src/schema.rs
# :24 pub const TRACE_SCHEMA_VERSION: i64 = 3;  → DR D5 的 v4/v5/v6 以此为基础（但 D5 未冻结，见 C4）
grep -n "只读连接.*拒绝\|version < 3" doc/spec-v1.2/06-存储与可观测.md
# 06 §6.7 步 7：只读连接版本 < 3 → 拒绝且不迁移 → D5"旧库只读兼容"表述不准确（C4）
/Users/zyzs/lczyzs/code/zgraph/target/debug/zgraph --help
# commands: run / replay / validate / trace（+ -v/--version/--help）；无 decide/resume → W3 需新增（C2）

# --- gate=false 现状（DR W2a 描述核对）---
# 01 §1.4 / 03 §3.4.2：BlockedDelivery → 结算入边（无数据）+ Skipped 级联；
# run.rs:1010-1035 实测（REV-062 §4 已探针）gate=false 只结算边+Skipped，run 仍 Completed(null)
# → DR W2a:63 描述属实；D1 要改默认行为属破坏性（C1）

# --- 对 W1 可执行性有影响的事实 ---
grep -n "call_index.min\|struct FakeState\|state: Mutex" crates/zgraph-harness/src/llm/fake.rs
# :87 struct FakeState；:95 state: Mutex；:151 calls[call_index.min(len-1)] → 单 run 共享、按调用序消费
grep -n "pub provider: Arc<dyn LlmProvider>" crates/zgraph-harness/src/executor.rs   # :40 单 provider
grep -n "fn build_provider" crates/zgraph-cli/src/run_cmd.rs                          # :733 CLI 构造一个 provider
grep -n "mark_interrupted\|interrupted" doc/spec-v1.2/06-存储与可观测.md | head
# 06 §6.8.1：recovery 只标定 interrupted，不恢复实例 → W3 durable 需补状态重建/恢复（C2）

# --- 流程/模块边界核对 ---
grep -n "| \`e2e\`\|| \`docs\`\|同一模块同一时间" AGENTS.md
# :38 e2e = tests/e2e/**；:39 docs = doc/**；:49 同一模块同一时间只允许一个写入者
# W2–W4 DoD 含 e2e 但模块列表未列 e2e；W1/W2a 均列 docs（C5）

# --- DR 关键行（引用定位）---
# W1:45-56；W2a:62-65；W2b:66-68；W3:71-81（durable:77）；W4:83-89；§4:100-109；
# D1–D6:115-122；§6:126-132（性能:131）；§7:136-145；§8:149-153；§9:157-166
```

- 本分支仅文档改动，未跑 `cargo test`/`pytest`（合并门禁会执行 build + workspace test + clippy + pytest；`check.sh` 已含 fmt/diff 检查且无代码可测）。
- 事实核对结论：DR 的 v0.1.0 边界描述（WaitingHuman/环/扇出/动态 N/共享队列/artifact）、W1 的 60 节点/100 边与 SOP 设计稿一致；DR 的 `gate=false` 现状描述、`retry.max_attempts=3`、trace schema v3 均与仓库一致；不一致处集中在 D1（改默认语义）、D5（迁移表述）、W3/W4（契约缺失）与执行计划（C5/C6）。

## 5. 遗留问题

- **C1–C6 为合并前必须关闭的条件**；其中 C1 为阻塞，C2–C6 为重要。关闭方式：修订 DR-004（决策/波次/DoD/§9）并在本记录 §6 追加关闭证据；必要时由 Lead 在 §9/执行记录中记录用户确认。
- **S1–S5** 建议项：请作者在 DR 中补/改，或登记 `doc/issues.md`（本评审按授权只改 `doc/reviews/**`，未编辑 DR/台账）。
- 未发现越界改动（diff 仅 `doc/**`）、禁用文件、冻结文件修改；worktree clean；base = `origin/main`；本评审未 merge/push/切 main。
- 复核重点提示：C1 的 opt-in 约束必须与 C4 的 trace 迁移、C6 的 v0.1.0 兼容回归一起改，否则 W2a 一开工就会改变 v0.1.0 语义；C2/C3 属于"实现前必须先定契约"的范围问题，建议在 W3/W4 开工前以 DR 追加段或独立设计稿冻结。

## 6. 条件关闭记录（仅"有条件通过"填写）

| 条件 | 关闭证据 | 关闭日期 |
|---|---|---|
| C1（阻塞，D1 opt-in/向后兼容 + quiescence/Skipped/trace 契约） | 待修订 DR-004 + 用户对默认语义/opt-in 的确认 | — |
| C2（重要，W3 durable 范围 + 恢复入口/崩溃一致性 + §9 用户确认） | 待修订 DR-004（W3 范围/DoD/§9） | — |
| C3（重要，W4 per-item 路由/join/失败/端口模型） | 待修订 DR-004（D4/DoD/§9 或独立设计草案） | — |
| C4（重要，D5 trace 迁移策略冻结 + 口径修正） | 待修订 DR-004（D5/§6） | — |
| C5（重要，波次-分支-模块-REV-评审人表；docs 串行；e2e/双评审） | 待修订 DR-004（§3/§4/§8） | — |
| C6（重要，发布 DoD 可验收 + v0.1.0 兼容/文档收口/08 回写） | 待修订 DR-004（§6） | — |

> 关闭后由作者发起复评；本记录届时追加"条件关闭记录"并把结论改为无条件通过（`merge.sh` 才会放行）。

## 7. 对 DR-004 §9 开放问题的建议答复

> 以下为评审人建议，最终以用户确认为准；与 C1–C6 的修订一起落入 DR。

1. **v0.2 范围（§9 Q1）**：建议接受 **W1–W4 全部纳入 v0.2**、共享评审队列放 **v0.3**（W5）。但 W3 建议明确拆分为 **W3a 人工停等 + decide（必须）** 与 **W3b durable resume（按用户确认决定是否 v0.2）**；若用户只要求"人工评审"，W3b 可降级为 v0.3，避免 v0.2 被 durable 恢复拖成高不确定版本。
2. **返工语义（§9 Q2）**：不建议二选一。建议 **opt-in "deferrable edge" + instance/attempt 级重入**：仅对显式标注的返工边/节点让 gate=false 不结算并等待；同时允许上游工作节点在 attempt+1 实例重跑（自环现有 `retry.max_attempts` 语义可复用，默认 3）；未开启的图保持 v0.1.0 的 settle+Skipped。明确终止：无后续可执行节点且存在 deferred/Pending → `RUN_DEADLOCK`（或专门的 wait/deadlock 错误）；trace 增加 deferred 事件并在 L1/L2 回放断言。
3. **人工接口（§9 Q3）**：CLI `decide` 作为 v0.2 唯一入口（无 HTTP）**够用**，前提是补：`resume` 入口（或 `decide` 后自动接续）、decision 幂等/冲突处理（重复/并发）、approver 身份字段（`--user`/默认 local）、拒绝后的去向（W2 返工边或 run Failed 的显式矩阵）、退出码/`--json` 契约、trace 决策记录（who/when/结论/理由）。服务化、远程审批、多租户不在 v0.2。
4. **动态 N（§9 Q4）**：**入口输入 + 上游输出端口取列表**即可覆盖 v0.2 场景；不需要额外"运行期外部注入"机制（外部数据可在 run 启动时作为 entry input 注入）。建议 D4 同时定义：列表为空/单元素/嵌套列表、item id 可复现、join 单端口列表（或 per-item 实例）、部分失败/取消策略、上限复用 D2、trace 记录展开事实、`graph_hash` 只随图 JSON 变化而不随 N 变化。
5. **扇出上限（§9 Q5）**：建议把"每节点最大扇出"明确为 **静态 out-degree 上限（加载期 DSL 校验）+ 动态 N 的运行期每激活放行数上限**两层；`max_fanout` 建议放节点（若需边级再单列，避免同名歧义）；默认无上限保持兼容；静态超限 → 加载期 DSL 错误；运行期动态超限 → 确定错误码 + trace + run 结果（不允许静默截断）。
6. **版本命名（§9 Q6）**：`v0.2.0` / `M7` 合理，建议采用；同时把 **v0.1.0 图/L1/L2 回放向后兼容**写成 v0.2 的硬性发布条件（与 C1 绑定）。

**建议追加到 §9 的问题**（当前缺，属阻塞性未决项）：
- W3 durable run 是否 v0.2 必须？是否接受拆 W3a/W3b？（C2）
- v0.1.0 图行为/回放兼容是否为硬约束（决定 D1 是否必须 opt-in）？（C1/C4）
- W4 join/失败/取消语义（除 N 来源外）；（C3）
- D5 trace 迁移粒度（每波 v4/v5/v6 vs 一次合并）；（C4）
- W1 验收是 fake-only 还是需 real-LLM smoke（agent/prompt 质量）；（S1）
- docs 模块并行写约束、W2–W4 是否加 e2e 模块与第二评审；（C5）
- 发布性能基线的数值化定义（或删除该条件）。（C6）
