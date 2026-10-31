# REV-063 · docs · v0.2（M7）范围与波次（DR-004）

- **分支**：`docs/docs-v02-kickoff`
- **作者**：`lead`（初版 `16204c1`、r2 `f9095ac`、r3 `9301494`、r4 `66a18d5`、r5 `a8b1a49`；提交正文含 `Review: REV-063` / `Agent: lead`）
- **评审人**：`reviewer-sop`（**≠ 作者**；独立评审；首轮 `ce69757`、r2 `16fe31b`、r3 `fff34df`、r4 `b687ed4`）
- **变更范围**：`e651ae5..a8b1a49`：DR-004 初版 166 行→r2 202→r3 219→r4 232→r5 232 行（r5 +15/−15，仅 DR-004）；`doc/index.md` 1 行；评审记录/索引由 `ce69757`/`16fe31b`/`fff34df`/`b687ed4` 增加。全部 `doc/**`，无代码/测试/配置改动；base 即 `origin/main`（`e651ae5`）；worktree clean；`v0.1.0` tag 指向 `5d43c58`
- **评审时间**：2026-10-31（首轮 + r2/r3/r4/r5 复审同日）
- **关联**：DR-004、DR-003（波次格式先例）、DR-001、[`design/software-team-sop-v1.md`](../design/software-team-sop-v1.md)（REV-062）、v0.1.0、ISSUE-062/091/093

## 1. 结论

**结论：通过**

方向判定：DR 与用户 2026-10-31 口径一致——W1 SOP v1 落地、W2 图级有限返工 + 每节点扇出上限、W3 人工评审、W4 动态 N；共享队列 v0.3（W5）；v0.1.0 能力边界与仓库现状一致；W1 只依赖 v0.1.0 能力的方向成立。

r2–r5 关闭了首轮 C1–C6 与后续 C7–C13/S11：D1 opt-in deferrable gate（默认 settle、v0.1.0 兼容）、W3a/W3b 拆分（W3a 控制通道 `human_decisions` + W3a0 并发冻结）、W4 单端口 join/W4a 冻结、D5 forward-only、§4 波次/模块/REV/docs 串行、§6 兼容矩阵/性能量化/文档收口、P v2（`parse_verdict_rework`）与 W2a 返工图模式、py-worker 单写者；r5 还统一了 **C11 defer 边源节点在含 defer 环上（approved 前进边因源 `P` 可重入而合法）**、**C12 唯一键/幂等键（`(run_id, instance_id)`；`decision_id` 仅审计）**、**C13 分表写者模型**（TraceWriter=事件表；decide=插入 `human_decisions`；run=更新 `consumed_at`；WAL/busy_timeout/短事务）。

复审未发现新的阻塞/重要问题；仅留 1 条非阻塞建议 S12（W2a0 的"defer 边源在含 defer 的环上"需在 W2a spec 中明确 DSL 环检测覆盖带 gate 的边——当前 R13 只检测无 gate 的环，需新增规则/测试）。本记录结论为 **通过**。

## 2. 四维评审

| 维度 | 结论 | 依据（DR 行 + 仓库事实） |
|---|---|---|
| 功能正确性 | ✅（S12 备注） | 范围/方向与用户一致；v0.1.0 边界核对一致（`WaitingHuman` 不产生、带出口环失败、无扇出上限/动态 N、`RunControl` 进程内 mpsc、无 decide/resume）；C1–C13/S11 的语义/DoD 已闭合（§6.2）；W1 60 节点/100 边、P v2/返工图模式、W3a 控制通道、W4 join、D5 迁移矩阵与 §6 发布条件自洽 |
| 代码结构合理性（文档/计划结构） | ✅ | §4 波次-分支-模块-REV-评审表（e2e/第二评审/docs 串行/py-worker 单写者、W1 e2e 先于 W2a e2e）完整；W2a0/W3a0/W4a 三个设计冻结先行；§8 编号/worktree/收口；S12 仅为 W2a spec 的实现细节 |
| 逻辑性 | ✅ | D1–D7 与 §3/§4/§6/§7/§9 对齐；C11 规则（源节点在含 defer 的环上）同时覆盖回边与 approved 前进边；C12 唯一键/幂等键一致；C13 分表写者与 WAL/busy_timeout 自洽；dormant/quiescence、attempt-per-activation、P v1/v2 兼容均有契约 |
| 可读性 | ✅ | r5 术语/字段/路径/§9 A/B 清晰；W2a0 的设计冻结条目、W3a0 的并发表、§6.3 的兼容矩阵可直接执行；S12 为需在 W2a spec 中补一句的细节 |

## 3. 发现清单

> C1–C13/S11 为历次发现（均已关闭，证据见 §6.2）；S12 为本次复审唯一残留建议（不阻塞）。

| # | 级别 | 位置 | 问题 | 建议 | 状态 |
|---|---|---|---|---|---|
| C1–C10 | 历史（阻塞/重要） | 初版–r4 | 默认 gate 语义/durable 范围/W4 join/trace 迁移/执行计划/DoD 可验收/W3a 控制通道/兼容矩阵/通用 rework/P 契约与 py-worker | 已按各轮意见修复（见 §6.2） | 已关闭（r2–r4） |
| C11 | 重要 | r4 `W2a0:68` vs `:70`、`D1:161`、`§9 A6:225` | defer 边规则与返工图模式互斥（两条 defer 边 vs "必须在环内"） | 统一为"defer 边源节点在含 defer 的环上（源可重入）"；DoD 加 approved/rework 双路径 e2e | 已关闭（r5 `a8b1a49`） |
| C12 | 重要 | r4 `W3a0:94` vs `W3a:102` | `human_decisions` 唯一键 `(run_id, instance_id)` 与 decide 幂等键 `(run_id, instance_id, decision_id)` 不一致 | 唯一键/幂等键统一 `(run_id, instance_id)`；`decision_id` 仅审计；D3/§9 A3 同步 | 已关闭（r5） |
| C13 | 重要 | r4 `W3a0:95`、`§6.6:182`、风险表:196 | "TraceWriter 唯一写连接" vs "decide 独立连接写 human_decisions" 互斥 | 分表写者：TraceWriter=trace 事件表；decide=插入；run=更新 consumed_at；WAL/busy_timeout/短事务；并发 e2e | 已关闭（r5） |
| S1–S10 | 建议 | 初版–r4 | fake LLM 边界/术语/扇出错误分层/§9/可读性/W3a0 排期/兼容 golden/并发细节 | 已按要求修复 | 已落实（r2–r4） |
| S11 | 建议 | r4 `§4:140,143` | W1/W2a py-worker 同模块先后未写死 | §4 写 py-worker 单写者 W1→W2a；W2a 依赖 W1 py-worker 合并 | 已落实（r5） |
| S12 | 建议（低） | r5 `W2a0:70` / W2a spec 02 | "defer 边源节点位于含 defer 的环上"要求 DSL 环检测覆盖**带 gate 的边**；现行 02 R13 的 `G'` 只含**无 gate 边**（02 §2.4 R13），W2a0/W2a spec 还未写明新增规则/反例 | 在 W2a0/W2a spec 02 明确：新增"defer 边环检测"（含 defer/gated 边的环）及正反例；不改 v0.1.0 R13 默认（无 gate 环检测） | open（非阻塞） |

> 级别口径：阻塞 = 必须修复后才能给"通过"；重要 = 必须修复或登记 ISSUE；建议 = 可后置，但需修复或进台账。

## 4. 验证命令与结果

```bash
# --- 范围/分支/基线（r5 复审前）---
cd /Users/zyzs/lczyzs/code/zgraph/.worktrees/docs-lead
git status --porcelain --branch            # ## docs/docs-v02-kickoff...origin/main [ahead 9]；clean
git diff b687ed4..HEAD --stat              # DR-004 232 行（+15/−15）；仅 1 file changed
git diff b687ed4..HEAD --name-status       # M doc/decisions/DR-004-v02-scope-and-waves.md
git diff e651ae5..HEAD --stat              # DR-004 + REV-063 记录 + reviews/README（评审记录）
git rev-parse origin/main v0.1.0^{commit}  # e651ae5... / 5d43c58...
git show -s --format='%H %s' v0.1.0        # tag v0.1.0（annotated），指向 5d43c58

# --- 仓库自检 ---
bash scripts/dev/check.sh
# OK user.name/email/remote / OK CLAUDE.md 未改 / OK 无禁用文件 / OK 分支名 / OK 语法 / OK progress/issues / OK fmt / OK diff-check
# CHECK PASSED  [exit=0]

# --- C11 关闭核对（defer 源在环上）---
sed -n '68,70p;161p;225p' doc/decisions/DR-004-v02-scope-and-waves.md
# :70 defer 边的源节点必须位于某个含 defer 边的环中（源可重入）——回边 P→W 在环上；前进边 P→下游 的源 P 在 P→W→R→P 环上 → 允许 defer
# :68 两条 defer 边；D1/§9 A6 同步；W2a DoD:78 增 approved-only / rework→approved 双路径 e2e

# --- C12 关闭核对（唯一键/幂等键）---
sed -n '94p;102p;163p' doc/decisions/DR-004-v02-scope-and-waves.md
# :94 唯一键 (run_id, instance_id)；decision_id 仅审计
# :102 幂等键 (run_id, instance_id)；D3:163 同步

# --- C13 关闭核对（分表写者）---
sed -n '95p;182p;196p' doc/decisions/DR-004-v02-scope-and-waves.md
# :95 TraceWriter=事件表唯一写；decide 独立连接插入 human_decisions；run decision consumer 独立 RW 更新 consumed_at；run 轮询只读；WAL/busy_timeout
# §6.6:182 / 风险表:196 同步为分表写者

# --- S11 关闭核对（py-worker 串行）---
sed -n '136p;143p' doc/decisions/DR-004-v02-scope-and-waves.md
# :136 py-worker 单写者 W1（parse_verdict）→ W2a（parse_verdict_rework）
# :143 W2a 依赖 W2a0；W1 的 py-worker 合并后

# --- C8/S9/C10 关闭回归抽查 ---
sed -n '67,78p;174,182p' doc/decisions/DR-004-v02-scope-and-waves.md
# W2a0 P v2/rework 模式；W2a py-worker/真实 SOP v1 rework e2e
# §6.3 bad_* golden=crates/zgraph-dsl/tests/fixtures.rs；harness_ac08/graph.json；v0.1.0 trace seed/expected
ls crates/zgraph-dsl/tests/fixtures.rs && grep -c "bad_" crates/zgraph-dsl/tests/fixtures.rs
# 文件存在且含 bad_* 断言 → S9 golden 路径成立

# --- S12 证据：R13 只检测无 gate 环 ---
grep -n "G' = \|无 gate\|R13" doc/spec-v1.2/02-DSL与校验.md | head
# R13：G' 顶点为节点、边为无 gate 的边；W2a0 新规则需覆盖带 defer/gated 边的环 → W2a spec 待补（S12，非阻塞）
```

- 本分支仅文档改动，未跑 `cargo test`/`pytest`（`check.sh` 已含 fmt/diff 检查且无代码可测；合并门禁会执行 build + workspace test + clippy + pytest）。
- r5 事实核对：C11 规则与 W2a0:68 两条 defer 边兼容（approved 前进边源 P 在环上）；C12 唯一键/幂等键一致；C13 分表写者与 §6.6/风险表/D3 自洽；S11 py-worker 串行已写；C8/S9/C10 回归抽查无回退。唯一残留 S12 为 W2a spec 的 DSL 环检测细则（非阻塞）。

## 5. 遗留问题

- **C1–C13/S11 全部关闭**（证据见 §6.2）；无阻塞/重要未决项。
- **S12（建议，非阻塞）**：W2a0 的"defer 边源在含 defer 的环上"需在 W2a spec 02 明确 DSL 环检测覆盖 gated/defer 边（现行 R13 只查无 gate 环），并补正反例；建议作者在 W2a0/W2a spec 顺手补或登记 `doc/issues.md`。
- 未发现越界改动（`b687ed4..HEAD` 仅 DR-004）、禁用文件、冻结文件修改；worktree clean；base = `origin/main`；本评审未 merge/push/切 main。

## 6. 条件关闭记录

### 6.1 四维复核（r5 `a8b1a49`）

| 维度 | 复审结论 | 依据 |
|---|---|---|
| 功能正确性 | ✅（S12 备注） | C1–C13/S11 全部闭合；C11/C12/C13 的规则与正文/§6/§9 一致；S12 仅 W2a spec 实现细则 |
| 代码结构合理性 | ✅ | §4 波次-分支-模块-REV-评审计表与顺序/docs/py-worker/e2e 单写者完整（:136-155）；三个设计冻结先行 |
| 逻辑性 | ✅ | D1–D7/§3/§4/§6/§7/§9 对齐；defer 源在环上、唯一键/幂等键、分表写者均自洽 |
| 可读性 | ✅ | r5 术语/路径/§9 A/B 清晰；S12 为一句 spec 细节 |

### 6.2 C1–C13/S11 关闭证据

| 条件 | 关闭证据 | 结论/日期 |
|---|---|---|
| C1–C6 | r2–r4：默认 settle（§2:37）、W2a0/W3a/W4a 冻结、D5 forward-only、§4 波次表、§6.2–§6.8 质量/兼容/性能/收口/08 | ✅ 关闭 2026-10-31 |
| C7 | r3：W3a:102-105 human_decisions 控制通道（WAL/200ms）；W3a0:90-98 冻结；D3:163；§9 A2/A3 | ✅ 关闭 2026-10-31 |
| C8 | r3/r4：§6.3:174-180 兼容矩阵（valid/bad/已知例外/run 子集/迁移样例） | ✅ 关闭 2026-10-31 |
| C9 | r3：W2a0:63-73 attempt-per-activation/dormant/max_attempts；W2a:75-78；D1:161 | ✅ 关闭 2026-10-31 |
| C10 | r4：W2a0:67-68 P v2/返工图模式；W2a:77-78 py-worker/真实 SOP v1 rework e2e；§4:143 分支；§6.5:182 SOP v1.1 回写；§9 A6:225 | ✅ 关闭 2026-10-31 |
| C11 | r5：W2a0:70 "defer 边源节点在含 defer 的环上"（回边 P→W 在环上；前进边源 P 在 P→W→R→P 环上）；D1:161/§9 A6:225 同步；W2a DoD:78 双路径 e2e | ✅ 关闭 2026-10-31 |
| C12 | r5：W3a0:94 唯一键 `(run_id, instance_id)`、decision_id 仅审计；W3a:102 幂等键 `(run_id, instance_id)`；D3:163/§9 A3:220 同步 | ✅ 关闭 2026-10-31 |
| C13 | r5：W3a0:95 分表写者（TraceWriter=事件表；decide=插入；run=更新 consumed_at；WAL/busy_timeout）；§6.6:182/风险表:196/D3:163 同步 | ✅ 关闭 2026-10-31 |
| S1–S10 | r2–r4：fake 边界/术语/扇出错误分层/§9/W3a0 排期/兼容 golden/并发细节 | ✅ 落实/接受 |
| S11 | r5：§4:136 py-worker 单写者 W1→W2a；W2a 依赖 W1 py-worker（:143） | ✅ 落实 2026-10-31 |

### 6.3 非阻塞建议（S12）

| 建议 | 问题 | 建议 |
|---|---|---|
| S12（低） | `W2a0:70` 的"defer 边源节点在含 defer 的环上"需要 DSL 环检测覆盖**带 gate 的边**；现行 02 R13 的 `G'` 只含无 gate 边（`doc/spec-v1.2/02-DSL与校验.md` R13），W2a0/W2a spec 尚未写明新增环检测规则与反例 | 在 W2a0/W2a spec 02 明确新增"defer 边环检测（含 defer/gated 边）"及正反例；不改 v0.1.0 R3 默认（无 gate 环检测）；建议顺手补或登记 ISSUE |

> **最终结论**：C1–C13/S11 全部关闭，S12 为非阻塞建议；本记录结论改为 **通过**。合并仍须通过 `scripts/dev/merge.sh` 的 build/test/clippy/pytest 门禁。

## 7. 对 DR-004 §9 的建议答复（r5 A/B）

- **A1 范围**：接受 W1–W4 进 v0.2，W5（共享队列）v0.3。
- **A2 W3b durable**：接受"默认只做 W3a（run 存活期间 decide/resume），W3b 不进 v0.2 发布条件"；若要"run 退出后 resume"，W3b 必须进 v0.2。
- **A3 人工接口**：CLI `decide`/`resume` + trace DB 轮询 + C12/C13 契约可接受；W3a0 再定 resume 自动/显式二选一。
- **A4 动态 N**：节点级 `foreach` + 单端口有序列表 join + `on_item_error=fail` 可接受；W4a 再冻结 item key/空列表/部分失败/取消/回放。
- **A5 扇出上限**：接受两层（加载期静态 + 运行期每激活）+ 缺省无上限；静态超限 DSL 错误、运行期错误码 + trace，禁止静默截断。
- **A6 返工 / P 契约**：接受 v1 `parse_verdict` 默认抛错 + W2a opt-in `parse_verdict_rework`/两条 defer 边/源在环上规则；W2a0 需补 S12 的环检测细则。
- **A7 版本/兼容**：接受 `v0.2.0` / `M7` + v0.1.0 图/L1/L2 兼容硬条件；§6.3 矩阵按 C8/S9 执行。

**B（评审默认）**：D5 forward-only、§6.3 兼容矩阵、§6.4 性能基线、W1 fake+real smoke、§4 docs/e2e/第二评审/py-worker 串行——评审接受，无需用户逐项确认。
