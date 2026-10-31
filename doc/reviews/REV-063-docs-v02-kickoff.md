# REV-063 · docs · v0.2（M7）范围与波次（DR-004）

- **分支**：`docs/docs-v02-kickoff`
- **作者**：`lead`（初版 `16204c1`、r2 `f9095ac`、r3 `9301494`；提交正文含 `Review: REV-063` / `Agent: lead`）
- **评审人**：`reviewer-sop`（**≠ 作者**；独立评审；首轮 `ce69757`、r2 复审 `16fe31b`）
- **变更范围**：`e651ae5..9301494`：初版 `16204c1`（DR-004 166 行 + `doc/index.md` 1 行）；r2 `f9095ac`（DR-004 202 行）；r3 `9301494`（DR-004 219 行，+84/−67）；评审记录/索引由 `ce69757`、`16fe31b` 增加。全部在 `doc/**`，**无代码/测试/配置改动**；base 即 `origin/main`（`e651ae5`）；worktree clean；`v0.1.0` tag 指向 `5d43c58`
- **评审时间**：2026-10-31（首轮 + r2/r3 复审同日）
- **关联**：DR-004（本文）、DR-003（M6 决策/波次格式先例）、DR-001（技术路线）、[`design/software-team-sop-v1.md`](../design/software-team-sop-v1.md)（REV-062）、v0.1.0（tag `v0.1.0` @ `5d43c58`）、ISSUE-062/091/093

## 1. 结论

**结论：有条件通过**

方向判定：DR 与用户 2026-10-31 的口径一致——W1 SOP v1 落地、W2 图级有限返工 + 每节点扇出上限、W3 人工评审、W4 动态 N；共享评审队列明确列为 **v0.3 候选**（§3 W5 / §1.3），未误写进 v0.2；v0.1.0 能力边界（`WaitingHuman` 不产生、带出口环不可运行、无扇出上限/动态 N/共享队列/框架级 artifact、`RunControl` 进程内 mpsc）与仓库现状一致；W1 只依赖 v0.1.0 能力的方向基本成立（fake LLM 边界见 S1）。

r3（`9301494`）关闭了 r2 的 **C7–C9**：W3a 的控制通道改为 trace DB `human_decisions` 表 + run 存活轮询（WAL/`HUMAN_DECISION_POLL_MS`），W3a0 先行冻结 decide/resume 契约；§6.3 改为可执行兼容矩阵（valid/bad/已知例外/skip run）；W2a0 冻结 defer 边在环内 + attempt-per-activation + `max_attempts` + 终止/trace；S6/S7/S8 基本落实（e2e 分支、串行顺序、§9 A/B 拆分、§1.3 非目标修正、节点级 `foreach`/`max_fanout`、codegen 路径、N_i 口径）。**C1–C9 现已关闭**（证据见 §6.2/§6.3）。

但复审新发现 **1 项重要 C10**（+ S6/S9/S10 建议）：W2a 的"评审不通过 → 回到工作节点重做"与 SOP v1 的 `P` 契约冲突——SOP v1 §5.2 规定 `P` 在 `verdict != approved` 时**抛异常**（run Failed）；W2a 要实现图级返工，必须让 `P`/返工边改为"输出 rework 并路由回工作节点"，但 r3 的 W2a0 设计冻结、W2a 模块（`kernel/dsl/trace/e2e/docs`，**无 py-worker**）与 §6.5 spec 回写均未覆盖 `P` 契约/py-worker；W2a 目标因此不能按当前计划落地（e2e 可造夹具绕过，但"真实 SOP v1 返工"不成立）。**C10 关闭前不得合并/进入 W2a 实现**（`merge.sh` 只接受记录结论为"通过"）。

## 2. 四维评审

| 维度 | 结论 | 依据（DR 行 + 仓库事实） |
|---|---|---|
| 功能正确性 | ⚠️ | r3 已把 C1–C9 的语义/范围/DoD 改到可评审：默认行为不变（§2:37）、W2a0 设计冻结（§3:63-72）、W3a 控制通道（§3:91-94）、W4a join（§3:103-108）、D5 forward-only（§5:154）、§6.3 兼容矩阵（:164-168）、§9 A/B（:203-219）；v0.1.0 边界核对一致。但 C10（W2a 与 SOP v1 `P` 契约/py-worker）仍使 W2a 目标不可实现 |
| 代码结构合理性（文档/计划结构） | ✅（有残留） | §4 波次-分支-模块-REV-评审表补齐 e2e/第二评审、串行顺序（:124-142）；docs 单写（§2:40）；W3b 可选（§3:96-99）。残留：W3a0 未进入 §4:125 的顺序/docs 队列；W1 e2e 与 W2a e2e 的先后未写死（S6） |
| 逻辑性 | ⚠️ | 主线自洽性改善（D1/D2/D4/D5 与 §6/§7/§9 对齐），但 C10：W2a0/W2a 未处理 `P` 的"非 approved 抛错"契约与 py-worker 模块；S6 的 doc queue 缺 W3a0；S9/S10 的验收/DB 竞争细节未写 |
| 可读性 | ✅（有残留） | r3 修正 §1.3 非目标、`§3 的 W5`、节点级 `foreach`/`max_fanout`、codegen 路径、N_i 口径；残留 S6/S9/S10 为计划/验收细节，不影响阅读 |

## 3. 发现清单

> C1–C9 为首轮/r2 发现（问题原文保留），状态按 r3 更新；C10 与 S6/S9/S10 为 r3 复审新发现。

| # | 级别 | 位置 | 问题 | 建议 | 状态 |
|---|---|---|---|---|---|
| C1 | 阻塞 | 初版 `D1:117` 等 | 默认改 gate=false 语义/未定义終止 | opt-in + 兼容/trace | 已关闭（r2 `f9095ac`；r3 W2a0:63-72/D1:150/D7:156/§6.3:164-168） |
| C2 | 重要 | 初版 `W3:71-81` | durable 范围/恢复契约 | W3a/W3b 拆分 + 用户确认 | 已关闭（r2 `f9095ac`；r3 W3a:89-99/D3:152/§9 A2:208） |
| C3 | 重要 | 初版 `W4:83-89` | dynamic N join/路由 | W4a 冻结 + 单端口列表 join | 已关闭（r3 W4a:103-108/W4b:110-113/D4:153/§9 A4:210） |
| C4 | 重要 | 初版 `D5:121` | trace 迁移口径 | forward-only/只读拒绝/迁移矩阵 | 已关闭（r3 D5:154/§6.6:171） |
| C5 | 重要 | 初版 `§3/§4/§8` | 执行/模块计划不合 AGENTS | 波次表/docs 串行/e2e/双评审 | 已关闭（r3 §2:39-40/§4:122-142；残留 S6） |
| C6 | 重要 | 初版 `§6:126-132` | DoD 不可验收 | 量化性能/兼容/收口/08 | 已关闭（r3 §6.2–§6.8:163-172；残留 S9） |
| S1 | 建议 | 初版 `W1:53,56` | fake LLM 边界/codegen 落点 | 限制说明 + real smoke + 路径 | 已落实（r3 W1:53,58；路径 :53） |
| S2 | 建议 | 初版 `W2a:62` 等 | 术语/字段口径 | 统一术语 | 已落实（r3 D1/D2/D4/W4b:105/W2b:81） |
| S3 | 建议 | 初版 `D2:118` | 扇出失败语义分层 | DSL/运行期分层 | 已落实（r3 W2b:79-85/D2:151） |
| S4 | 建议 | 初版 `§9` | 开放问题覆盖 | 补 scope/durable/join/迁移等 | 已落实（r3 §9 A/B:203-219） |
| S5 | 建议 | 初版 `§1.3` 等 | 可读性 | 修非目标/术语 | 已落实（r3 §1.3:22-29） |
| C7 | 重要 | r2 `W3a:81-88` | W3a 跨进程 decide/resume 无控制通道 | 明确控制通道/W3b 边界 | 已关闭（r3 W3a:89-94/D3:152/§4:136-137/W3a0；§9 A2/A3:208-209） |
| C8 | 重要 | r2 `§6.3:160` | 兼容条件用 `**` 全量不可满足 | 显式矩阵 + 已知例外 | 已关闭（r3 §6.3:164-168） |
| C9 | 重要 | r2 `W2a:69` | 显式 rework 边/attempt 未定义 | W2a0 冻结 attempt-per-activation | 已关闭（r3 W2a0:63-72/W2a:74-77/D1:150；DoD :77） |
| C10 | **重要** | r3 `W2a:74-77`、`W2a0:63-72`、`W1:53-55`、SOP 设计稿 §5.2 | **W2a 的"评审不通过 → 回到工作节点重做"与 SOP v1 的 `P` 契约冲突，且实施模块缺 py-worker**：SOP v1 §5.2 规定 `P` 在 `verdict != "approved"`（含 rework）时**抛 Python 异常** → run Failed；W2a 要做图级返工，必须让 `P`/返工边改为"输出 rework 并路由回工作节点"（否则 rework 永远到不了边）。r3 的 W2a0 冻结仅覆盖 defer/attempt/max_attempts/终止/trace，**未定义 P 契约、返工边图模式（哪个 P→W 回边、如何与 §5.2 的 P 失败契约切换）**；W2a 模块 `kernel/dsl/trace/e2e/docs` **不含 py-worker**（W1:55 的 `parse_verdict` 在 py-worker），§6.5 spec 回写也未列 SOP 设计稿/05/07 的 P 契约更新。结果：W2a 只能用测试夹具绕过真实 SOP v1 的 P，不能实现"真实 SOP 图"的图级返工 | W2a0 设计冻结增加：①P 在返工模式的输出契约（返回 verdict/rework，不抛错；或新增独立解析节点）与 rework 边图模式（P→W 回边、gate 条件、与 `on_gate_false=defer` 的配合）；②W2a 模块增 `py-worker`（`parse_verdict` v2）或明确使用哪个新解析器；③§6.5 增 SOP 设计稿 §5.x/07 回写与 W2a DoD 增"真实 SOP v1 图 rework"e2e；④§9 A6 说明返工与 P 失败的边界 | open |
| S6 | 建议 | r3 `§4:125-142`（W3a0:136） | 计划表残留：W3a0（docs 设计冻结）未出现在 §4:125 的 kernel/trace 串行顺序与 docs 队列（队列为 W2a0→W1-docs→W2a-docs→W2b-docs→W3a-docs→W4a→W4b-docs）；W3a 依赖 W3a0（:137）却无排期；W1 e2e 与 W2a e2e 的先后（W1:129 vs W2a:132）未写死 | 把 W3a0 插入顺序与 docs 队列（建议 W2a-docs 后、W3a-docs 前）；写死 W2a e2e 在 W1 e2e 合并后（e2e 单写者） | open |
| S9 | 建议 | r3 `§6.3:165-168` | 兼容矩阵的可验收物未指定：`bad_*.json` 的"v0.1.0 golden（错误码/定位/error_count）"存哪、谁生成；v0.1.0 trace DB 迁移样例的路径/生成方式未写；`harness_ac08` 指目录还是 `graph.json` | 明确 golden 文件路径/生成命令/责任人（如 `tests/golden/v0.1-compat/*.json`），并把 `harness_ac08/graph.json` 写全 | open |
| S10 | 建议 | r3 `W3a:91`、`D3:152` | `human_decisions` 作为控制通道的 DB 细节未冻结：run 的 TraceWriter 与 `decide` 进程对同一 SQLite 的并发写（WAL 单写者）、`busy_timeout`、read-only 连接可见性、decision_id 生成/幂等冲突；W3a0 应冻结 | W3a0 补 SQLite 并发/重试/超时/冲突矩阵与 e2e（含锁竞争） | open |

> 级别口径：阻塞 = 必须修复后才能给"通过"；重要 = 必须修复或登记 ISSUE；建议 = 可后置，但需修复或进台账。

## 4. 验证命令与结果

```bash
# --- 范围/分支/基线（r3 复审前）---
cd /Users/zyzs/lczyzs/code/zgraph/.worktrees/docs-lead
git status --porcelain --branch            # ## docs/docs-v02-kickoff...origin/main [ahead 5]；clean
git diff 16fe31b..HEAD --stat              # DR-004 219 行（+84/−67）；仅 1 file changed
git diff 16fe31b..HEAD --name-status       # M doc/decisions/DR-004-v02-scope-and-waves.md
git diff e651ae5..HEAD --stat              # DR-004 + REV-063 记录 + reviews/README（评审记录）
git rev-parse origin/main v0.1.0^{commit}  # e651ae5... / 5d43c58...
git show -s --format='%H %s' v0.1.0        # tag v0.1.0（annotated），指向 5d43c58

# --- 仓库自检 ---
bash scripts/dev/check.sh
# OK user.name=chenyuxiangg / OK user.email=... / OK remote=git@github.com:chenyuxiangg/graph_framework.git
# OK CLAUDE.md 未被修改 / OK 暂存区无禁用文件 / OK 分支命名: docs/docs-v02-kickoff
# OK Python 语法检查通过 / OK doc/progress.md 存在 / OK doc/issues.md 存在
# OK doc/progress.md 非空行 14（目标 ≤30）/ OK cargo fmt --check 通过 / OK git diff --check 通过
# CHECK PASSED  [exit=0]

# --- C7 关闭核对（W3a 控制通道）---
grep -n "human_decisions\|HUMAN_DECISION_POLL_MS\|run 进程退出后" doc/decisions/DR-004-v02-scope-and-waves.md
# :91 run WaitingHuman 保持存活 + 轮询 trace DB human_decisions（WAL 只读，200ms）；decide 写表；退出后不可 decide（W3b）
# :152 D3 同步；:136 W3a0 docs 设计冻结；:137 W3a 依赖 W3a0
/Users/zyzs/lczyzs/code/zgraph/target/debug/zgraph --help
# commands: run/replay/validate/trace（+ -v/--version/--help）；无 decide/resume
grep -n "pub enum RunControl\|ctrl_tx: mpsc::UnboundedSender<RunControl>" crates/zgraph-kernel/src/run.rs
# :136/:168 进程内 RunControl → r3 新增 trace DB 控制通道是必要设计（C7 关闭方向成立）

# --- C8 关闭核对（兼容矩阵）---
ls tests/fixtures/graphs/valid_*.json
# valid_created_at_offset / valid_cycle / valid_parallel / valid_subgraph
ls tests/fixtures/graphs/bad_*.json | wc -l    # 30 个故意非法 validate 反例（§6.3 只做 validate golden）
ls tests/fixtures/graphs/harness_ac08/
# graph.json / fake_llm.json / agents / tools → §6.3 "harness_ac08 图"应指 harness_ac08/graph.json（S9）
grep -n "ISSUE-062" doc/issues.md
# :70 valid_cycle → RUN_DEADLOCK / RUN_DELIVERY_TO_TERMINAL；r3 §6.3:167 写为已知例外 RUN_DEADLOCK

# --- C9 关闭核对（通用返工）---
grep -n "W2a0\|attempt-per-activation\|必须位于环内\|max_attempts" doc/decisions/DR-004-v02-scope-and-waves.md | head
# :63-72 W2a0：defer 边必须在环内；目标 Completed → attempt+1 新实例；max_attempts 3；终止/trace
# :74-77 W2a 实现 + DoD A→B→A；:150 D1 同步
grep -n "fn resolve_self_loop\|RUN_DELIVERY_TO_TERMINAL" crates/zgraph-kernel/src/run.rs
# 旧内核仅自环 attempt+1；非自环 Completed 目标 → RUN_DELIVERY_TO_TERMINAL（W2a0 需改，与 ISSUE-062 对应）

# --- C10 证据（W2a 与 SOP v1 P 契约）---
grep -n "parse_verdict" doc/decisions/DR-004-v02-scope-and-waves.md
# :55 W1 的 parse_verdict 在 py-worker；:76 W2a 模块 kernel/dsl/trace/e2e/docs（无 py-worker）
grep -n "verdict != \"approved\"\|抛 Python 异常" doc/design/software-team-sop-v1.md | head
# SOP 设计稿 §5.2：P 在非 approved/非法 JSON 时抛异常 → Crashed → Failed
# 而 W2a 目标是"评审不通过 → 回到工作节点重做"（:62/:76）→ 必须改 P/返工边契约；W2a0:63-72 未覆盖
grep -n "spec 01/02/03/06\|spec 00" doc/decisions/DR-004-v02-scope-and-waves.md
# W2a 仅 spec 01/02/03/06（:76/§6.5:170），未列 SOP 设计稿/05/07 的 P 契约回写

# --- S6/S9/S10 证据 ---
# §4:125 顺序含 W2a0→W2a→W2b→W3a... 但无 W3a0；docs 队列无 W3a0；W3a0 在 :136、W3a 依赖它 :137
# §6.3:165-168 无 golden 文件路径；trace DB 迁移样例无路径；harness_ac08 目录 vs graph.json
# W3a:91 human_decisions + WAL 只读连接；TraceWriter 写库并发/busy_timeout/decision 冲突未写
```

- 本分支仅文档改动，未跑 `cargo test`/`pytest`（`check.sh` 已含 fmt/diff 检查且无代码可测；合并门禁会执行 build + workspace test + clippy + pytest）。
- r3 事实核对：W3a 控制通道、§6.3 兼容矩阵、W2a0 attempt 语义、§4 波次表、§9 A/B 与正文一致；C7–C9 的修复方向成立。新缺口集中在 C10（W2a 与 SOP v1 P 契约/py-worker）与 S6/S9/S10 的计划/验收细节。

## 5. 遗留问题

- **C1–C9 已关闭**（证据见 §6.2/§6.3）；但复审新发现 **C10（重要）**，关闭前本记录结论保持"有条件通过"，不得合并/进入 W2a 实现。
- **S6/S9/S10** 建议在 C10 的同一轮修订中处理（或登记 `doc/issues.md`）。
- 未发现越界改动（`16fe31b..HEAD` 仅 DR-004）、禁用文件、冻结文件修改；worktree clean；base = `origin/main`；本评审未 merge/push/切 main。

## 6. 条件关闭记录

### 6.1 四维复核（r3 `9301494`）

| 维度 | 复审结论 | 依据 |
|---|---|---|
| 功能正确性 | ⚠️（C10 未闭） | C1–C9 已闭合；C10（W2a 与 SOP v1 P 契约/py-worker）使 W2a 目标不可实现 |
| 代码结构合理性 | ✅（S6 残留） | §4 波次表/e2e/第二评审/串行已补；W3a0 未入顺序与 docs 队列（S6） |
| 逻辑性 | ⚠️ | D1/D2/D4/D5 与 §6/§7/§9 对齐；C10；S6/S9/S10 |
| 可读性 | ✅（S6/S9/S10 残留） | r3 修正非目标/术语/路径/N_i；残留为计划/验收细节 |

### 6.2 C1–C6 关闭证据（r2 `f9095ac`，r3 保持）

| 条件 | 关闭证据 | 结论/日期 |
|---|---|---|
| C1（阻塞）返工默认语义 | §2:37 默认不变；W2a0:63-72（defer 在环内/attempt-per-activation/max_attempts/终止/trace/兼容）；W2a:74-77；D1:150；D7:156；§6.3:164-168 | ✅ 关闭 2026-10-31 |
| C2（重要）durable/人工范围 | W3a:89-94；W3b:96-99；D3:152；§6.1:162；§9 A2:208 | ✅ 关闭 2026-10-31 |
| C3（重要）动态 N 模型 | W4a:103-108；W4b:110-113；D4:153；§9 A4:210 | ✅ 关闭 2026-10-31 |
| C4（重要）trace 迁移 | D5:154（forward-only/迁移点 W2a/W3a/W4b/备份/测试）；§6.6:171 | ✅ 关闭 2026-10-31 |
| C5（重要）执行/模块计划 | §2:39-40；§4:122-142（波次/分支/模块/依赖/评审；e2e/双评审）；§8:196-199 | ✅ 关闭 2026-10-31（残留 S6） |
| C6（重要）发布 DoD | §6.2–§6.8:163-172（质量/兼容矩阵/性能量化/08/文档收口）；§6.3:164-168 | ✅ 关闭 2026-10-31（残留 S9） |

### 6.3 C7–C9 关闭证据（r3 `9301494`）

| 条件 | 关闭证据 | 结论/日期 |
|---|---|---|
| C7（重要）W3a 控制通道 | W3a:91-94：run 存活轮询 trace DB `human_decisions`（WAL 只读、200ms）、`decide` 写表、幂等键、退出后需 W3b；D3:152；W3a0 设计冻结（§4:136）；§4:137 W3a 依赖 W3a0/W2b；§9 A2/A3:208-209 | ✅ 关闭 2026-10-31（S10 细节） |
| C8（重要）兼容矩阵 | §6.3:164-168：validate valid_*/bad_* 分类；run 可运行子集；已知例外 valid_cycle→RUN_DEADLOCK、valid_subgraph→NODE_TYPE_UNSUPPORTED；bad_* 不 run；v0.1.0 DB 迁移后 trace/replay；golden 不变 | ✅ 关闭 2026-10-31（S9 验收物） |
| C9（重要）通用返工/attempt | W2a0:63-72：defer 边在环内；目标 Completed → attempt+1 新实例（不报 RUN_DELIVERY_TO_TERMINAL）；max_attempts 3；终止/trace/兼容；W2a DoD:77 A→B→A；D1:150 | ✅ 关闭 2026-10-31（C10 仍接 P 契约） |

### 6.4 未关闭条件（r3 复审新发现）

| 条件 | 关闭要求 | 关闭日期 |
|---|---|---|
| C10（重要）W2a 与 SOP v1 P 契约/py-worker | W2a0 增 P 返工模式输出契约（非 approved 不抛错、输出 rework）与 rework 边图模式（P→W 回边/gate/defer 配合）；W2a 模块增 py-worker 或明确替代解析器；§6.5 增 SOP 设计稿/05/07 回写；W2a DoD 增真实 SOP v1 图 rework e2e；§9 A6 说明 P 失败/返工边界 | — |
| S6/S9/S10（建议） | §4 顺序/docs 队列补 W3a0、写死 W1/W2a e2e 先后；§6.3 补 golden/trace DB 样例路径；W3a0 补 SQLite 并发/busy_timeout/幂等冲突（或登记 ISSUE） | — |

> C1–C9 关闭、C10 未关闭；本记录结论维持"有条件通过"，待 r4 修订后复审。

## 7. 对 DR-004 §9 的建议答复（r3 A/B）

- **A1 范围**：接受 W1–W4 进 v0.2，W5（共享队列）v0.3；建议在 §9 保持这一问，用户确认后冻结。
- **A2 W3b durable**：接受"默认只做 W3a（run 存活期间 decide/resume），W3b 不进 v0.2 发布条件"；但若用户要求"run 退出后还能 resume"，则 W3b 必须进 v0.2（C7 的控制通道已按 live run 设计，W3b 才是退出后恢复）。
- **A3 人工接口**：CLI `decide`/`resume` + trace DB 轮询（无 HTTP/socket）在**单机 + run 存活**前提下够用；需 W3a0 冻结幂等/冲突、`busy_timeout`、decision_id 生成（S10）。
- **A4 动态 N**：节点级 `foreach` + 单端口有序列表 join + `on_item_error=fail` 可覆盖 v0.2；W4a 再冻结 item key/空列表/部分失败/取消/回放细节即可。
- **A5 扇出上限**：接受两层（加载期静态 + 运行期每激活）+ 缺省无上限；明确静态超限 DSL 错误、运行期错误码 + trace，禁止静默截断。
- **A6 返工默认值**：接受 `on_gate_false` 缺省 `settle`、`max_attempts` 缺省 3、defer 边必须在环内；但需 C10 的 P/返工边契约，否则默认值只对测试夹具有效。
- **A7 版本/兼容**：接受 `v0.2.0` / `M7` + v0.1.0 图/L1/L2 兼容硬条件；§6.3 兼容矩阵按 C8 执行，补 S9 的 golden/trace DB 样例路径。

**B（评审默认）**：D5 forward-only、§6.3 矩阵、§6.4 性能基线、W1 fake+real smoke、§4 docs/e2e/第二评审——评审可接受，无需用户逐项确认；但 §4 的 W3a0 排期（S6）需在 DR 内修正。
