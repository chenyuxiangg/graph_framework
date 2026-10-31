# REV-063 · docs · v0.2（M7）范围与波次（DR-004）

- **分支**：`docs/docs-v02-kickoff`
- **作者**：`lead`（初版 `16204c1`、r2 `f9095ac`、r3 `9301494`、r4 `66a18d5`；提交正文含 `Review: REV-063` / `Agent: lead`）
- **评审人**：`reviewer-sop`（**≠ 作者**；独立评审；首轮 `ce69757`、r2 `16fe31b`、r3 `fff34df`）
- **变更范围**：`e651ae5..66a18d5`：DR-004 初版 166 行→r2 202 行→r3 219 行→r4 232 行（r4 +36/−23，仅 DR-004）；`doc/index.md` 1 行；评审记录/索引由 `ce69757`/`16fe31b`/`fff34df` 增加。全部 `doc/**`，无代码/测试/配置改动；base 即 `origin/main`（`e651ae5`）；worktree clean；`v0.1.0` tag 指向 `5d43c58`
- **评审时间**：2026-10-31（首轮 + r2/r3/r4 复审同日）
- **关联**：DR-004、DR-003（波次格式先例）、DR-001、[`design/software-team-sop-v1.md`](../design/software-team-sop-v1.md)（REV-062）、v0.1.0、ISSUE-062/091/093

## 1. 结论

**结论：有条件通过**

方向判定：DR 与用户 2026-10-31 口径一致——W1 SOP v1 落地、W2 图级有限返工 + 每节点扇出上限、W3 人工评审、W4 动态 N；共享队列 v0.3（W5）；v0.1.0 能力边界与仓库现状一致（`WaitingHuman` 不产生、带出口环 `RUN_DEADLOCK`/`RUN_DELIVERY_TO_TERMINAL`、无扇出上限/动态 N、`RunControl` 进程内 mpsc、无 `decide/resume`）；W1 只依赖 v0.1.0 能力的方向成立。

r4（`66a18d5`）关闭了 r3 的 **C10**（P v2 opt-in `parse_verdict_rework` + 返工图模式 + dormant/quiescence + W2a 增 py-worker/分支/真实 SOP v1 rework e2e + §6.5 回写 + §9 A6）与 **S6/S9/S10**（W3a0 入顺序/docs 队列、W1 e2e 先于 W2a e2e、bad_* golden/trace seed/harness_ac08 路径、W3a0 并发/表/decision_id 冻结）。**C1–C10 现已关闭**（证据见 §6.2）。

但复审新发现 **3 项重要（C11–C13）** + S11：
- **C11**：W2a0 自相矛盾——`W2a0:68` 要求返工图模式**两条边都 `on_gate_false: "defer"`**（approved 前进边 + rework 回边），但 `W2a0:70`/`D1:161`/`§9 A6:225` 又规定 **defer 边必须位于环内（自环或回边）**；approved 前进边 `P→下游` 是**前进边、不在环内**，按规则不能标 defer → 两段不能同时成立；
- **C12**：`W3a0:94` 的 `human_decisions` 唯一键为 `(run_id, instance_id)`，而 `W3a:102` 的 decide 幂等键为 `(run_id, instance_id, decision_id)` → 两处契约不一致（同一实例的 decision_id 既不参与唯一约束，又被称为幂等键）；
- **C13**：`W3a0:95` 声称 "`TraceWriter` 是唯一写连接"，但又要求 "`zgraph decide` 用独立连接…单事务写" `human_decisions` → "唯一写连接"与"decide 独立写连接"直接冲突（`§6.6:182`/风险表 `:196` 重复该口径）。

**C11–C13 关闭前不得合并/进入 W2a/W3a 实现**（`merge.sh` 只接受记录结论为"通过"）。

## 2. 四维评审

| 维度 | 结论 | 依据（DR 行 + 仓库事实） |
|---|---|---|
| 功能正确性 | ⚠️ | r4 已把 C1–C10 的范围/语义/DoD 改到可评审：P v2 opt-in（`W2a0:67`）、W2a0 返工图模式（`:68`）、W3a0 先冻结 decide/resume 与 `human_decisions` 并发（`:90-98`）、§6.3 兼容矩阵（`:174-180`）、W2a 增 py-worker（`:77`/`:143`）；v0.1.0 边界核对一致。但 C11–C13 三处契约冲突使 W2a0/W3a0 仍不可直接实现 |
| 代码结构合理性（文档/计划结构） | ✅（S11 残留） | §4 波次-分支-模块-REV-评审表、顺序与 docs 队列已补 W3a0 与 W1→W2a e2e 顺序（`:136-155`）；W2a 增 py-worker 分支（`:143`）。残留：W1 py-worker 与 W2a py-worker 的先后未写死（W1 `parse_verdict` vs W2a `parse_verdict_rework`，同模块；S11） |
| 逻辑性 | ⚠️ | 大方向自洽（D1–D7/§6/§7/§9 对齐）；但 C11（defer 边环规则 vs 两条 defer 边）、C12（唯一键 vs 幂等键）、C13（唯一写连接 vs decide 独立写）三处直接矛盾 |
| 可读性 | ✅（C11–C13 为契约问题而非排版） | r4 结构/术语/路径/§9 A/B 清晰；C11–C13 需在文字契约层修正 |

## 3. 发现清单

> C1–C10 为前四轮发现（已关闭）；C11–C13/S11 为 r4 复审新发现。

| # | 级别 | 位置（r4） | 问题 | 建议 | 状态 |
|---|---|---|---|---|---|
| C1 | 阻塞 | 初版 D1 | 默认改 gate=false/未定义终止 | opt-in + 兼容/trace | 已关闭（r2；W2a0:63-73/D1:161/D7:167/§6.3） |
| C2 | 重要 | 初版 W3 | durable 范围/恢复契约 | W3a/W3b 拆分 + 用户确认 | 已关闭（r2；W3a:100-105/W3b:107-110/D3:163/§9 A2:219） |
| C3 | 重要 | 初版 W4 | dynamic N join/路由 | W4a 冻结 + 单端口列表 join | 已关闭（r3；W4a:114-119/W4b:121-124/D4:164/§9 A4:221） |
| C4 | 重要 | 初版 D5 | trace 迁移口径 | forward-only/只读拒绝/迁移点 | 已关闭（r3；D5:165/§6.6:182） |
| C5 | 重要 | 初版 §3/§4/§8 | 执行/模块计划不合 AGENTS | 波次表/docs 串行/e2e/双评审 | 已关闭（r3/r4；§2:39-40/§4:136-155；残留 S11） |
| C6 | 重要 | 初版 §6 | DoD 不可验收 | 量化性能/兼容/收口/08 | 已关闭（r4；§6.2–§6.8:174-185） |
| C7 | 重要 | r2 W3a | 跨进程 decide/resume 控制通道 | trace DB 表轮询 + W3b 边界 | 已关闭（r3；W3a:102-105/W3a0/D3:163/§9 A2/A3） |
| C8 | 重要 | r2 §6.3 | 兼容条件用 `**` 全量不可满足 | 显式矩阵 + 已知例外 | 已关闭（r3/r4；§6.3:174-180） |
| C9 | 重要 | r2 W2a | 通用 rework/attempt 未定义 | W2a0 attempt-per-activation | 已关闭（r3；W2a0:63-73/W2a:75-78/D1:161） |
| C10 | 重要 | r3 W2a/P 契约 | W2a 与 SOP v1 P 契约/py-worker | P v2 + py-worker 模块 + SOP v1.1 回写 | 已关闭（r4；W2a0:67-68/W2a:77-78/§4:143/§6.5:182/§9 A6:225） |
| S1–S5 | 建议 | 初版 | fake 边界/术语/扇出错误分层/§9/可读性 | 按要求修 | 已落实（r2/r3/r4） |
| S6 | 建议 | r3 §4 | W3a0 未入顺序/docs 队列；W1/W2a e2e 先后 | 补排期 | 已落实（r4；§4:136/147/148）；残留 S11 |
| S9 | 建议 | r3 §6.3 | 兼容 golden/trace 样例路径 | 指定 golden/seed/责任 | 已落实（r4；§6.3:176-180） |
| S10 | 建议 | r3 W3a | human_decisions 并发/幂等细节 | W3a0 冻结 | 已落实（r4；W3a0:90-98）；但引入 C12/C13 |
| C11 | **重要** | `W2a0:68` vs `:70`（并 `D1:161`、`§9 A6:225`） | **defer 边规则与返工图模式互斥**：`:68` 要求返工图模式的**两条边都标 `on_gate_false:"defer"`**（`P→下游` approved 前进边 + `P→W` rework 回边）；`:70`/D1/§9 A6 又规定 **defer 边必须位于环内（自环或回边）**。`P→下游` 是前进边、不在环内，按规则不可标 defer；若把它标 settle，拒绝时下游会 settle+Skipped，重做后 approved 再投递已完成下游 → `RUN_DELIVERY_TO_TERMINAL`（ISSUE-062 同类），正是 W2a 要消除的失败。两段必须统一 | 二选一并写死：①把规则改为 "`defer` 边的**源节点**必须在环上（可重入）"（`P` 在 W2a 环上，approved 前进边可 defer）；或②只允许 rework 回边 defer，另行为 approved 前进边设计"下游保持 Pending/dormant"机制；同步 W2a0:68/:70、D1:161、§9 A6:225、W2a DoD；补 approved/rework 两条路径的 e2e（含拒绝→重做→approved） | open |
| C12 | **重要** | `W3a0:94` vs `W3a:102` | **`human_decisions` 唯一键与 decide 幂等键不一致**：W3a0 表定义唯一键 `(run_id, instance_id)`；W3a 说幂等键 `(run_id, instance_id, decision_id)`。若唯一键只有 `(run_id, instance_id)`，同一实例第二次 decide（不同 decision_id）会被唯一约束拒绝/视为冲突，`decision_id` 不参与幂等；若用 `(run_id, instance_id, decision_id)` 作唯一键，则同一实例可插多决策，W3a0 "同实例两次决定"冲突语义又不同 | 明确：唯一键与幂等键统一（建议 `(run_id, instance_id)` 唯一——一个实例一条决策；`decision_id` 仅作为事件/审计 id，不参与幂等），同步 W3a0:94/W3a:102/D3:163/§9 A3；补重复决定/冲突 e2e | open |
| C13 | **重要** | `W3a0:95`（并 `§6.6:182`、风险表 `:196`） | **"TraceWriter 唯一写连接"与 "decide 独立连接写 human_decisions" 直接冲突**：W3a0:95 先写 "`TraceWriter` 是唯一写连接"，紧接着 "`zgraph decide` 用独立连接…单事务写"；§6.6/风险表又重复"TraceWriter 唯一写连接"。若 `human_decisions` 由 decide 独立连接写，则它不是唯一写连接；若坚持唯一写连接，decide 必须把决策交给 TraceWriter/专用 writer，不能"独立连接写" | 明确写连接模型：建议 **TraceWriter = trace 事件表唯一写连接；`human_decisions` = decide 独立写连接（单事务 + busy_timeout）**，并写锁序/重试/可见性（WAL）；同步 W3a0:95、§6.6:182、风险表:196、D3:163；补 decide 与 TraceWriter 并发写 e2e | open |
| S11 | 建议 | `§4:140,143` | W1 py-worker 与 W2a py-worker 的先后未写死：W1 交付 `parse_verdict`（py-worker），W2a 交付 `parse_verdict_rework`（py-worker，`:77/:143`）；W2a 只依赖 W2a0（`:143`），未依赖 W1；当前只写 "W1 e2e 先于 W2a e2e"，未约束同模块 py-worker 写入者 | 在 §4 加 "W2a py-worker 分支依赖 W1 py-worker 合并"（或把 W2a py-worker 改动并入 W1 py-worker 交付），保证 py-worker 单写者 | open |

> 级别口径：阻塞 = 必须修复后才能给"通过"；重要 = 必须修复或登记 ISSUE；建议 = 可后置，但需修复或进台账。

## 4. 验证命令与结果

```bash
# --- 范围/分支/基线（r4 复审前）---
cd /Users/zyzs/lczyzs/code/zgraph/.worktrees/docs-lead
git status --porcelain --branch            # ## docs/docs-v02-kickoff...origin/main [ahead 7]；clean
git diff fff34df..HEAD --stat              # DR-004 232 行（+36/−23）；仅 1 file changed
git diff fff34df..HEAD --name-status       # M doc/decisions/DR-004-v02-scope-and-waves.md
git diff e651ae5..HEAD --stat              # DR-004 + REV-063 记录 + reviews/README（评审记录）
git rev-parse origin/main v0.1.0^{commit}  # e651ae5... / 5d43c58...
git show -s --format='%H %s' v0.1.0        # tag v0.1.0（annotated），指向 5d43c58

# --- 仓库自检 ---
bash scripts/dev/check.sh
# OK user.name=chenyuxiangg / OK email / OK remote / OK CLAUDE.md 未改 / OK 无禁用文件
# OK 分支命名: docs/docs-v02-kickoff / OK Python 语法 / OK progress/issues / OK fmt / OK diff-check
# CHECK PASSED  [exit=0]

# --- C10 关闭核对（P v2/返工模式/py-worker）---
sed -n '63,78p' doc/decisions/DR-004-v02-scope-and-waves.md
# :67 P v2 parse_verdict_rework（opt-in，approved|rework，非法 JSON/未知 verdict 抛错）
# :68 两条 defer 边（approved 前进 / rework 回边）；:77 W2a 模块含 py-worker；:78 真实 SOP v1 rework e2e
sed -n '136,155p' doc/decisions/DR-004-v02-scope-and-waves.md
# :143 W2a 分支含 feat/py-worker-parse-rework；:147 W3a0；:136 W1 e2e 先于 W2a e2e
grep -n "spec 回写" doc/decisions/DR-004-v02-scope-and-waves.md
# §6.5 增 SOP 设计稿 v1.1 返工契约/05/07

# --- S6/S9/S10 关闭核对 ---
grep -n "W3a0" doc/decisions/DR-004-v02-scope-and-waves.md
# :136 顺序含 W3a0(docs)；:147 表行 W3a0；:148 W3a 依赖 W3a0
ls crates/zgraph-dsl/tests/fixtures.rs && grep -c "bad_" crates/zgraph-dsl/tests/fixtures.rs
# fixtures.rs 存在且含 bad_* 断言 → §6.3 bad_* golden 路径成立
sed -n '174,182p' doc/decisions/DR-004-v02-scope-and-waves.md
# §6.3：bad_* golden=crates/zgraph-dsl/tests/fixtures.rs；harness_ac08/graph.json；v0.1.0 trace seed/expected
sed -n '90,98p' doc/decisions/DR-004-v02-scope-and-waves.md
# W3a0：human_decisions 表/唯一键/concurrency/UUIDv7/consumed_at/resume 冻结

# --- C11 证据：defer 边规则 vs 两条 defer 边 ---
sed -n '68p;70p;161p;225p' doc/decisions/DR-004-v02-scope-and-waves.md
# :68 两条边都 on_gate_false=defer（approved 前进 + rework 回边）
# :70 defer 边必须位于环内（自环或回边；DSL 校验）
# approved 前进边 P→下游 不在环内 → 两段互斥

# --- C12 证据：唯一键 vs 幂等键 ---
sed -n '94p;102p' doc/decisions/DR-004-v02-scope-and-waves.md
# :94 唯一键 (run_id, instance_id)
# :102 幂等键 (run_id, instance_id, decision_id) → 不一致

# --- C13 证据：TraceWriter 唯一写 vs decide 独立写 ---
sed -n '95p;182p;196p' doc/decisions/DR-004-v02-scope-and-waves.md
# :95 "TraceWriter 是唯一写连接" + "zgraph decide 用独立连接…单事务写"
# :182 §6.6 "单写连接"；:196 风险表 "TraceWriter 唯一写连接" → 与 decide 独立写冲突

# --- S11 证据：W1/W2a py-worker 先后 ---
grep -n "py-worker" doc/decisions/DR-004-v02-scope-and-waves.md
# W1:140 py-worker parse_verdict；W2a:143 py-worker parse_rework；W2a 依赖只有 W2a0（:143），未写 W1
```

- 本分支仅文档改动，未跑 `cargo test`/`pytest`（`check.sh` 已含 fmt/diff 检查且无代码可测；合并门禁会执行 build + workspace test + clippy + pytest）。
- r4 事实核对：C10/S6/S9/S10 的修复方向成立（W2a0 P v2/模块/分支、§6.3 golden/seed、W3a0 并发冻结）；新缺口集中在 C11–C13 三处契约矛盾与 S11 的 py-worker 串行。

## 5. 遗留问题

- **C1–C10 已关闭**（证据见 §6.2）；复审新发现 **C11–C13（重要）**，关闭前本记录结论保持"有条件通过"，不得合并/进入 W2a/W3a 实现。
- **S11** 建议在 C11–C13 同一轮修订中处理（或登记 `doc/issues.md`）。
- 未发现越界改动（`fff34df..HEAD` 仅 DR-004）、禁用文件、冻结文件修改；worktree clean；base = `origin/main`；本评审未 merge/push/切 main。

## 6. 条件关闭记录

### 6.1 四维复核（r4 `66a18d5`）

| 维度 | 复审结论 | 依据 |
|---|---|---|
| 功能正确性 | ⚠️（C11–C13 未闭） | C1–C10 已闭合；C11 defer 边规则互斥、C12 唯一键/幂等键、C13 写连接模型冲突，使 W2a0/W3a0 不可直接实现 |
| 代码结构合理性 | ✅（S11 残留） | §4 波次/分支/模块/顺序/docs 队列/e2e/第二评审已补；W1/W2a py-worker 先后未写（S11） |
| 逻辑性 | ⚠️ | C11/C12/C13 三处直接矛盾；其余 D1–D7/§6/§7/§9 基本对齐 |
| 可读性 | ✅ | r4 结构/术语/路径/§9 A/B 清晰；问题为契约而非排版 |

### 6.2 C1–C10 关闭证据（r2–r4）

| 条件 | 关闭证据 | 结论/日期 |
|---|---|---|
| C1–C6 | r2/r3/r4：默认 settle（§2:37）、W2a0 冻结（:63-73）、W3a/W3b 拆分（:100-110）、W4a join（:114-119）、D5 forward-only（:165）、§6.2–§6.8（:174-185）、§4 波次表（:136-155） | ✅ 关闭 2026-10-31 |
| C7 | W3a:102-105（run 存活 + human_decisions 轮询/WAL/200ms）；W3a0:90-98；D3:163；§9 A2/A3:219-220 | ✅ 关闭 2026-10-31 |
| C8 | §6.3:174-180 兼容矩阵（valid/bad/已知例外/run 子集/迁移样例） | ✅ 关闭 2026-10-31 |
| C9 | W2a0:63-73 attempt-per-activation/dormant/max_attempts；W2a:75-78；D1:161 | ✅ 关闭 2026-10-31 |
| C10 | W2a0:67-68 P v2 + 返工图模式；W2a:77-78 增 py-worker/真实 SOP v1 rework e2e；§4:143 分支；§6.5:182 SOP v1.1/05/07 回写；§9 A6:225 | ✅ 关闭 2026-10-31（C11 为 W2a0 内部规则冲突） |

### 6.3 未关闭条件（r4 复审新发现）

| 条件 | 关闭要求 | 关闭日期 |
|---|---|---|
| C11（重要）defer 边规则互斥 | 统一 W2a0:68/:70/D1:161/§9 A6:225：明确 approved 前进边可 defer 的条件（如 "defer 边源节点在环上"）或仅回边 defer + 下游 Pending 机制；补 approved/rework 双路径 e2e（拒绝→重做→approved 不得 RUN_DELIVERY_TO_TERMINAL） | — |
| C12（重要）唯一键 vs 幂等键 | 统一 `human_decisions` 唯一键与 decide 幂等键（建议唯一 `(run_id, instance_id)`，`decision_id` 仅审计 id），同步 W3a0:94/W3a:102/D3:163/§9 A3；补重复/冲突 e2e | — |
| C13（重要）写连接模型 | 明确 "TraceWriter = trace 事件唯一写；human_decisions = decide 独立写连接（单事务 + busy_timeout）"，或改为 decide 经 TraceWriter/专用 writer；同步 W3a0:95/§6.6:182/风险表:196/D3:163；补并发 e2e | — |
| S11（建议）W1/W2a py-worker 先后 | §4 加 W2a py-worker 依赖 W1 py-worker 合并（或并入 W1 交付） | — |

> C1–C10 关闭、C11–C13 未关闭；本记录结论维持"有条件通过"，待 r5 修订后复审。

## 7. 对 DR-004 §9 的建议答复（r4 A/B）

- **A1 范围**：接受 W1–W4 进 v0.2，W5（共享队列）v0.3。
- **A2 W3b durable**：接受"默认只做 W3a（run 存活期间 decide/resume），W3b 不进 v0.2 发布条件"；若要求"run 退出后 resume"，W3b 必须进 v0.2。
- **A3 人工接口**：CLI `decide`/`resume` + trace DB 轮询 + W3a0 并发契约方向可接受；但须先解决 C12/C13（唯一键/幂等键、写连接模型）。
- **A4 动态 N**：节点级 `foreach` + 单端口有序列表 join + `on_item_error=fail` 可接受；W4a 再冻结 item key/空列表/部分失败/取消/回放细节。
- **A5 扇出上限**：接受两层（加载期静态 + 运行期每激活）+ 缺省无上限；静态超限 DSL 错误、运行期错误码 + trace，禁止静默截断。
- **A6 返工 / P 契约**：接受 v1 `parse_verdict` 默认抛错 + W2a opt-in `parse_verdict_rework`/两条 defer 边的方向；但必须先解决 C11（两条 defer 边与"必须在环内"互斥）；defer/`max_attempts`/`settle` 默认值本身可接受。
- **A7 版本/兼容**：接受 `v0.2.0` / `M7` + v0.1.0 图/L1/L2 兼容硬条件；§6.3 矩阵按 C8/S9 执行。

**B（评审默认）**：D5 forward-only、§6.3 矩阵、§6.4 性能基线、W1 fake+real smoke、§4 docs/e2e/第二评审——评审可接受；但 §4 需补 S11 的 py-worker 先后。
