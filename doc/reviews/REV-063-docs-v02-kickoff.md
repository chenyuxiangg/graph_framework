# REV-063 · docs · v0.2（M7）范围与波次（DR-004）

- **分支**：`docs/docs-v02-kickoff`
- **作者**：`lead`（初版 `16204c1`、r2 修订 `f9095ac`；提交正文含 `Review: REV-063` / `Agent: lead`）
- **评审人**：`reviewer-sop`（**≠ 作者**；独立评审；首轮提交 `ce69757`）
- **变更范围**：`e651ae5..f9095ac`：初版 `16204c1` 新增 `doc/decisions/DR-004-v02-scope-and-waves.md`（166 行）+ `doc/index.md` 1 行；r2 `f9095ac` 仅改 DR-004（202 行，+127/−91）；评审记录/索引由 `ce69757` 增加。全部在 `doc/**`，**无代码/测试/配置改动**；base 即 `origin/main`（`e651ae5`）；worktree clean；`v0.1.0` tag 指向 `5d43c58`
- **评审时间**：2026-10-31（首轮 + r2 复审同日）
- **关联**：DR-004（本文）、DR-003（M6 决策/波次格式先例）、DR-001（技术路线）、[`design/software-team-sop-v1.md`](../design/software-team-sop-v1.md)（REV-062）、v0.1.0（tag `v0.1.0` @ `5d43c58`）、ISSUE-062/091/093

## 1. 结论

**结论：有条件通过**

方向判定：DR 与用户 2026-10-31 的口径一致——W1 SOP v1 落地、W2 图级有限返工 + 每节点扇出上限、W3 人工评审、W4 动态 N；共享评审队列明确列为 **v0.3 候选**（§3 W5 / §1.3），未误写进 v0.2；v0.1.0 能力边界（`WaitingHuman` 不产生、带出口环不可运行、无扇出上限/动态 N/共享队列/框架级 artifact）与仓库现状一致；W1 只依赖 v0.1.0 能力的方向基本成立（fake LLM 边界见 S1）。

r2（`f9095ac`）已按首轮意见关闭 **C1–C6**（opt-in deferrable gate/默认 settle、W3a/W3b 拆分、W4 join 模型、D5 forward-only、波次/模块/REV 表、兼容/性能/文档 DoD），S1–S3/S5 基本落实。但复审新发现 **3 项重要（C7–C9）**与 3 项建议（S6–S8）：
- **C7**：W3a 的"外部 CLI `decide`/`resume`"没有跨进程控制通道/在线状态访问设计；live run（v0.1.0 `RunControl` 为进程内 mpsc）无法被另一个 CLI 进程决定；若靠"退出后 resume"则落到 W3b（默认不纳入 v0.2），W3a/W3b 拆分不自洽；
- **C8**：§6.3 "v0.1.0 的 `tests/fixtures/graphs/**` 全部 validate/run 通过"不可满足——目录含 `bad_*.json`（应 validate 失败）、`valid_cycle.json`（`RUN_DEADLOCK`，ISSUE-062）、`valid_subgraph.json`（run `NODE_TYPE_UNSUPPORTED`）；
- **C9**：W2a 的"显式 rework 边（attempt+1）"未定义通用 instance-per-activation；v0.1.0 只有自环 `attempt+1`，非自环回边指向已完成实例会 `RUN_DELIVERY_TO_TERMINAL`（ISSUE-062），W2a 也没有 W4a 式的设计冻结步骤。

**C7–C9 关闭前不得合并/进入实现**（`merge.sh` 只接受记录结论为"通过"）。S6–S8 为计划/术语建议，可在同一轮修订顺手处理。

## 2. 四维评审

| 维度 | 结论 | 依据（DR 行 + 仓库事实） |
|---|---|---|
| 功能正确性 | ⚠️ | r2 已把 C1–C6 的语义/范围/DoD 改到可评审：默认行为不变（§2:36）、deferrable gate opt-in（W2a:64-71/D1:146）、W3a/W3b 拆分（W3:81-94/D3:148/§9 Q1:196）、W4 join 单端口列表（W4a:100-102/D4:149）、D5 forward-only（D5:150/§6.6:163）、兼容/性能/文档 DoD（§6.3–§6.7:160-164）；v0.1.0 能力边界核对一致。但 C7（W3a 在线控制）、C8（§6.3 兼容条件不可满足）、C9（W2a rework 实例语义）仍影响可执行性 |
| 代码结构合理性（文档/计划结构） | ✅（有残留） | 新增 §4"波次—分支—模块—REV—评审"表（:118-138）、docs 单写入者（§2:39）、每模块一 worktree（§2:38）、e2e 模块与第二评审（§4:124-135）；W3 明确不涉及 harness（:87）。残留：branch 列在 W2a/W2b/W3a/W4b 漏 e2e 分支（S6）、W2b/W3a 均依赖 W2a 且都写 kernel 但未显式排序（S6） |
| 逻辑性 | ⚠️ | r2 主线自洽性明显改善（D1/D2/D4/D5 与 §6/§7/§9 对齐），但：C9 的"显式 rework 边 attempt+1"未定义；§1.3:25 把"durable 最小范围"例外误挂在"工具并行执行"上（原"断点续跑"项消失），§1.3:26 的 `§3.5` 交叉引用不存在（S8）；§4 branch/module 列不一致（S6） |
| 可读性 | ✅（有残留） | r2 标题/决策/DoD/§9 可读性提升；但仍有术语/口径残留（S8：显式 rework 边未定义、W4b:107"图/节点级 foreach"、W2b:77"同一字段或节点配置"、W1:52 codegen 脚本落点、§6.4:161"N=3"），§9 仍缺部分用户确认项（S7） |

## 3. 发现清单

> C1–C6 为首轮发现（问题原文保留），状态按 r2 更新；C7–C9/S6–S8 为 r2 复审新发现。

| # | 级别 | 位置 | 问题 | 建议 | 状态 |
|---|---|---|---|---|---|
| C1 | 阻塞 | 初版 `§5 D1:117`（关联 `W2a:62-64`、`§7:140`） | D1 以默认行为改 `gate=false` 语义，破坏 v0.1.0 契约/回放，且未定义 quiescence/Skipped/trace | opt-in（`on_gate_false`，缺省 settle）+ 显式终止/死锁/trace；v0.1.0 golden 回归 | 已关闭（`f9095ac`；W2a:66-71/D1:146/D7:152/§6.3；实例语义残留见 C9） |
| C2 | 重要 | 初版 `W3:71-81`、`D3:119`、`§9 Q3:161` | durable run 范围/恢复契约未定义且未让用户确认 | W3a/W3b 拆分；decide/resume/崩溃一致性/§9 用户确认 | 已关闭（`f9095ac`；W3a:83-88/W3b:90-94/D3:148/§9 Q1:196；控制通道见 C7） |
| C3 | 重要 | 初版 `W4:83-89`、`D4:120` | dynamic N 的 per-item 路由/join/端口模型未定义 | W4a 设计冻结 + 单端口有序列表 join + 失败/取消/上限 | 已关闭（`f9095ac`；W4a:100-103/W4b:105-109/D4:149/§9 Q2:197） |
| C4 | 重要 | 初版 `D5:121` | "旧库只读兼容"与 spec 06 §6.7 forward-only 不符 | forward-only + 只读拒绝 + 每波/合并迁移矩阵 | 已关闭（`f9095ac`；D5:150/§6.6:163） |
| C5 | 重要 | 初版 `§3/§4/§8` | 执行/模块计划与 AGENTS §2 不一致（docs 并行写、缺 e2e、缺双评审、每波 worktree） | 波次-分支-模块-REV-评审人表；docs 串行；e2e/双评审 | 已关闭（`f9095ac`；§2:38-39/§4:118-138/W1:51/W2a:72/W2b:78/W3a:87/W4b:108/§8:188；残留 S6） |
| C6 | 重要 | 初版 `§6:126-132` | DoD 不可验收/缺兼容、文档收口、08 回写 | 量化性能/兼容回归/收口/08 | 已关闭（`f9095ac`；§6.2–§6.8:159-164；兼容口径残留 C8） |
| S1 | 建议 | 初版 `W1:53,56`、`§7:144` | fake LLM 共享/provider 按序消费导致 60 节点并发只能验证编排，不能验证 prompt 质量；codegen 落点未说明 | 明确 e2e 边界 + 手动 real-LLM smoke + codegen 命令/路径 | 已落实（`f9095ac` W1:57；codegen 路径残留见 S8） |
| S2 | 建议 | 初版 `W2a:62` vs `D1:117` 等 | 术语/字段口径不一致（可重臂 vs deferrable、节点/边 max_fanout、图/节点 foreach） | 统一术语与字段位置 | 基本落实（`f9095ac`；残留 S8） |
| S3 | 建议 | 初版 `D2:118` | 扇出超限失败语义未分层 | 加载期 DSL 错误 vs 运行期错误码 | 已落实（`f9095ac` W2b:76-77/D2:147） |
| S4 | 建议 | 初版 `§9:157-166` | §9 开放问题覆盖不足 | 补 durable/兼容/join/迁移/fake-real/文档/性能 | 部分落实（`f9095ac` §9:194-202；缺 scope/CLI/N 来源，见 S7） |
| S5 | 建议 | 初版 `§1.3:25` 等 | 可读性/口径小问题 | 明确 design-only 并行、durable 范围 | 基本落实（`f9095ac`；§1.3:25 的新误挂见 S8） |
| C7 | **重要** | r2 `W3a:81-88`（尤其 `:85-86`）、`D3:148`、`§4:130`、`§9:196` | **W3a 的"外部 CLI decide/resume"缺跨进程控制通道/在线状态访问设计**。v0.1.0 的 `RunControl` 是 kernel 进程内 mpsc（`run.rs:136,168`），CLI 命令只有 validate/run/trace/replay（无 decide/resume）；`zgraph run` 持运行态在内存中。W3a 同时要求"run 在 human gate 暂停 → decide approve 后恢复"（DoD:88），但未定义另一个 CLI 进程如何把 decision 送达 live run（IPC/socket/控制文件/trace 轮询？），也未定义 run 退出后 decide+resume 的状态重建（这实际属于 W3b durable，而 W3b 默认不纳入 v0.2）。W3a/W3b 拆分因此不自洽：无 IPC 则 W3a 不可实现，无 durable 则跨进程 decide 无状态 | 明确 W3a 控制通道（建议：run 在 human Pending 时轮询 trace DB 的 `human_decisions` 表；`decide` 写表并唤醒；或 `decide --attach` 通过 Unix socket/control file 连接 live run；或改为 run 退出 + `resume`，则 W3b 必须进 v0.2）。补充协议/e2e（两进程真实交互）、幂等/超时/崩溃窗口；§9 增加用户确认 |
| C8 | **重要** | r2 `§6.3:160` | **发布兼容条件不可满足**："v0.1.0 的 `tests/fixtures/graphs/**` 全部 validate/run 通过"——该目录含 `bad_*.json`（`on_error`/未知字段/cycle 等**故意非法**的 validate 反例）、`valid_cycle.json`（v0.1.0 真内核 `RUN_DEADLOCK`，ISSUE-062）、`valid_subgraph.json`（validate 通过但 run `NODE_TYPE_UNSUPPORTED`，02 §2.6.4）。把它写成"全部 validate/run 通过"会导致发布门禁无法执行/误判 | 改为显式兼容清单：`valid_*.json` 中**可运行**子集（剔除已知 ISSUE-062/`valid_subgraph` 或标注预期错误）run 通过；`bad_*.json` 断言 validate 错误码；e2e fixture 目录单独列出；v0.1.0 golden 事件/输出用指定样例而非全目录 |
| C9 | **重要** | r2 `W2a:64-72`（尤其 `:69`）、`D1:146` | **"显式 rework 边（attempt+1）"未定义通用 instance-per-activation**：v0.1.0 只有**自环**产生 `attempt+1`（`resolve_self_loop`，02 D-02-05/00 §0.5），非自环回边投递到已完成实例会 `RUN_DELIVERY_TO_TERMINAL`（ISSUE-062 第二失效模式）。W2a 声称非自环 rework 边也 attempt+1，但未定义：rework 边如何声明/标记、目标实例已 Completed 时如何创建新 attempt、instance/attempt/ID/trace 如何变化、与 `max_attempts` 的绑定；也没有像 W4a 那样的"先设计冻结"步骤。若实际只支持自环，则"图级有限返工"退化为自环，不能满足用户方向 | 为 W2a 增加设计冻结（参照 W4a）：定义显式 rework 边语法/标注与通用 attempt-per-activation/instance-per-activation，或明确 v0.2 只做自环返工并修正"图级"表述；补非自环返工端到端（含 attempt+1、最终 `RUN_DELIVERY_TO_TERMINAL` 不得发生、trace/回放） |
| S6 | 建议 | r2 `§4:124-135`（W2a:126、W2b:128、W3a:130、W4b:134）、`:137-138` | 计划表细节不一致：①W2a/W2b/W3a/W4b 的"模块"列含 `e2e`，但"分支"列没有 e2e 分支（W1 有 `test/e2e-software-team-v1`）；②W2b 与 W3a 都依赖 W2a 且都要写 `kernel`，表中未给出二者先后（仅靠 §4:137"kernel 同一时间一个写入者"隐含），执行时可能两个 kernel worktree 并行；③W1 e2e 与 W2a e2e 的并行边界（"W1 可与 W2a kernel/dsl 并行"）未说明 W2a e2e 何时做 | 补 e2e 分支（或明确 e2e 改动并入哪个模块分支并声明跨模块例外）；给 W2b→W3a（或 W3a→W2b）明确排序；写明 W2a e2e 在 W1 合并后 |
| S7 | 建议 | r2 `§9:194-202` | §9 用户确认项仍不完整：现只有 durable/join/fanout/版本兼容/返工默认值 5 项；首轮要求覆盖的 **v0.2 范围（W1–W4 全部？W5 v0.3）**、**CLI decide 是否够用**、**动态 N 来源（入口/上游 vs 运行期外部注入）** 已从 §9 消失（正文 D4 直接定了来源），且 §9 末句称"建议答复已并入正文"但 migration/fake-real/docs/性能等未作为用户问题列出 | 补回 scope/CLI decide/N 来源三项（可给默认建议让用户确认）；把 D5 迁移/性能/fake-real/docs 策略标明为"评审默认、无需用户拍板"或列入 §9；与 §1.3/§6 对齐 |
| S8 | 建议 | r2 `§1.3:25-26`、`W2a:69`、`W2b:77`、`W4b:107`、`W1:52`、`§6.4:161` | 术语/口径残留：①§1.3:25 把"（除 W3 的 durable 最小范围外，见 W3b）"挂在"工具并行执行"上（原"断点续跑"项消失，语义不通）；②§1.3:26 引用不存在的 `§3.5`（W5 在 §3 列表）；③"显式 rework 边"未定义（同 C9）；④W2b:77 运行期层写"同一字段或节点配置"含糊；⑤W4b:107 仍写"DSL 图/节点级 `foreach`"而 D4/W4a 未定级；⑥W1:52 只写 fixtures 目录，codegen 脚本路径/命令仍未说明；⑦§6.4:161 "N=3"与 SOP 设计稿 §4.1 的 per-role N_i（合计 16 work items）口径不一致 | 修正 §1.3 非目标句并恢复"断点续跑"项；改 §3.5→§3 W5；补"显式 rework 边"定义（或随 C9 设计冻结）；明确 max_fanout 字段位置与 foreach 级别；给 codegen 脚本路径/命令；N=3 改为"§4.1 参考实例（N_i 见该表）" |

> 级别口径：阻塞 = 必须修复后才能给"通过"；重要 = 必须修复或登记 ISSUE；建议 = 可后置，但需修复或进台账。

## 4. 验证命令与结果

```bash
# --- 范围/分支/基线（r2 复审前）---
cd /Users/zyzs/lczyzs/code/zgraph/.worktrees/docs-lead
git status --porcelain --branch            # ## docs/docs-v02-kickoff...origin/main [ahead 3]；clean
git diff ce69757..HEAD --stat              # DR-004 202 行（+127/−91）；仅 1 file changed
git diff ce69757..HEAD --name-status       # M doc/decisions/DR-004-v02-scope-and-waves.md
git diff e651ae5..HEAD --stat              # DR-004 202 + reviews/README 1 + REV-063 142（评审记录）
git rev-parse origin/main v0.1.0^{commit}  # e651ae5... / 5d43c58...（base=origin/main；tag 与 DR 一致）
git show -s --format='%H %s' v0.1.0        # tag v0.1.0（annotated），指向 5d43c58

# --- 仓库自检 ---
bash scripts/dev/check.sh
# OK user.name=chenyuxiangg / OK user.email=... / OK remote=git@github.com:chenyuxiangg/graph_framework.git
# OK CLAUDE.md 未被修改 / OK 暂存区无禁用文件 / OK 分支命名: docs/docs-v02-kickoff
# OK Python 语法检查通过 / OK doc/progress.md 存在 / OK doc/issues.md 存在
# OK doc/progress.md 非空行 14（目标 ≤30）/ OK cargo fmt --check 通过 / OK git diff --check 通过
# CHECK PASSED  [exit=0]

# --- C1–C6 关闭方向核对（r2）---
# 默认不变：§2:36"默认行为不变"；W2a:66-67 on_gate_false 缺省 settle；D7:152 向后兼容
# opt-in defer：W2a:68-71；终止：W2a:70 未决 deferred + 无可重入来源 → RUN_DEADLOCK
# trace：W2a:71 GateResult deferred+attempt；W2a DoD:79 v0.1.0 golden 不变
# C2：W3a:83-88（decide/resume，v0.2 必须）/ W3b:90-94（durable 可选，§6.1:158 默认不纳入）/ §9 Q1:196
# C3：W4a:100-103（设计冻结：单端口有序列表 join、on_item_error=fail、取消/上限/回放）/ W4b:105-109 / D4:149 / §9 Q2:197
# C4：D5:150 forward-only + 只读拒绝 + 每波/合并迁移；§6.6:163 迁移测试
# C5：§2:38-39 每模块 worktree/docs 单写；§4:118-138 波次表；W1:51/W2a:72/W2b:78/W3a:87/W4b:108 模块；§8:188
# C6：§6.2:159 质量门禁；§6.3:160 兼容；§6.4:161 性能量化；§6.5:162 spec+08；§6.7:164 文档收口

# --- C7 证据：W3a 控制通道/CLI ---
/Users/zyzs/lczyzs/code/zgraph/target/debug/zgraph --help
# commands: run / replay / validate / trace（+ -v/--version/--help）；无 decide/resume
grep -n "pub enum RunControl\|ctrl_tx: mpsc::UnboundedSender<RunControl>" crates/zgraph-kernel/src/run.rs
# :136 RunControl（Pause/Resume/Cancel）；:168 进程内 mpsc → 无跨进程控制通道
grep -n "mark_interrupted\|只读连接.*拒绝" doc/spec-v1.2/06-存储与可观测.md
# 06 §6.8.1 recovery 只 mark_interrupted；§6.7 只读拒绝旧版本 → W3b 才处理恢复
# → W3a"live run + 外部 CLI decide"无通信机制；若退出后 decide/resume 则依赖 W3b（默认不 v0.2）

# --- C8 证据：§6.3 兼容清单不可满足 ---
ls tests/fixtures/graphs/ | head
# bad_agent_not_found.json / bad_cycle.json / ...（故意非法 validate 反例）
ls tests/fixtures/graphs/ | grep -E "valid_cycle|valid_subgraph"
# valid_cycle.json（ISSUE-062：v0.1.0 真内核 RUN_DEADLOCK）、valid_subgraph.json（run NODE_TYPE_UNSUPPORTED）
grep -n "ISSUE-062" doc/issues.md
# :70 valid_cycle 两种失败模式 → §6.3"全部 validate/run 通过"不成立

# --- C9 证据：W2a rework 实例语义 ---
grep -n "fn resolve_target\|fn resolve_self_loop\|RUN_DELIVERY_TO_TERMINAL" crates/zgraph-kernel/src/run.rs
# resolve_self_loop 仅自环 attempt+1；非自环已完成目标 → RUN_DELIVERY_TO_TERMINAL（ISSUE-062）
grep -n "D-02-05\|仅自环/显式重试\|DEFAULT_MAX_ATTEMPTS" doc/spec-v1.2/02-DSL与校验.md doc/spec-v1.2/00-总纲.md
# 02 D-02-05 / 00 §0.5：attempt 仅自环（v1 无显式重试）→ W2a"显式 rework 边 attempt+1"需新增通用机制

# --- S6/S7/S8 证据 ---
# §4 branch 列（:126/:128/:130/:134）无 e2e 分支；模块列含 e2e；W2b/W3a 均依赖 W2a 且写 kernel（:128/:130/:137）
# §9:196-200 仅 5 项（durable/join/fanout/版本兼容/返工默认值），缺 scope/CLI/N 来源
# §1.3:25 durable 例外误挂"工具并行执行"；§1.3:26 的 §3.5 不存在；W4b:107 "图/节点级 foreach"；W2b:77 "同一字段或节点配置"
```

- 本分支仅文档改动，未跑 `cargo test`/`pytest`（`check.sh` 已含 fmt/diff 检查且无代码可测；合并门禁会执行 build + workspace test + clippy + pytest）。
- r2 事实核对：§1.1 能力边界、W1 60/100、`gate=false` 现状、trace schema v3、`retry.max_attempts=3`、D5 forward-only 均与仓库/spec 一致；不一致/缺口集中在 C7（W3a 控制通道）、C8（§6.3 兼容清单）、C9（W2a rework 实例语义）及 S6–S8。

## 5. 遗留问题

- **C1–C6 已按 r2 关闭**（证据见 §6.2），但复审新发现 **C7–C9（重要）**，关闭前本记录结论保持"有条件通过"，不得合并/进入实现。
- **S6–S8** 建议在 C7–C9 的同一轮修订中顺手处理（或登记 `doc/issues.md`）。
- 未发现越界改动（`ce69757..HEAD` 仅 DR-004）、禁用文件、冻结文件修改；worktree clean；base = `origin/main`；本评审未 merge/push/切 main。

## 6. 条件关闭记录

### 6.1 四维复核（r2 `f9095ac`）

| 维度 | 复审结论 | 依据 |
|---|---|---|
| 功能正确性 | ⚠️（C7–C9 未闭） | C1–C6 方向/范围已闭合；但 W3a 在线 decide 无控制通道、§6.3 兼容条件不可满足、W2a 非自环 rework 实例语义缺失（§3 C7–C9） |
| 代码结构合理性 | ✅（S6 残留） | §4 波次-分支-模块-REV-评审表 + docs 单写 + e2e/第二评审已补；残留 branch 列 e2e 与 W2b/W3a kernel 排序（§3 S6） |
| 逻辑性 | ⚠️ | D1/D2/D4/D5 与 §6/§7/§9 基本对齐；C9、§1.3:25 误挂、§3.5 死引用（S8） |
| 可读性 | ✅（S8 残留） | 结构/术语较 r1 明显改善；残留术语/口径见 S8 |

### 6.2 C1–C6 关闭证据（`f9095ac`）

| 条件 | 关闭证据 | 结论/日期 |
|---|---|---|
| C1（阻塞）返工默认语义 | §2:36"默认行为不变"；W2a:66-71 `on_gate_false`（缺省 settle；defer 不结算/不 Skipped/Pending；自环或 rework 边 attempt+1；max_attempts 3；无可重入来源 → RUN_DEADLOCK）；D1:146；D7:152；W2a:71 trace deferred+attempt；W2a DoD:79 v0.1.0 golden 回归；§6.3:160 兼容回归 | ✅ 关闭 2026-10-31（实例语义残留 C9） |
| C2（重要）durable/人工范围 | W3a:83-88（decide/resume、幂等/approver/拒绝矩阵/退出码/trace）；W3b:90-94（durable 可选，默认不纳入发布）；§6.1:158；§9 Q1:196 请用户确认 | ✅ 关闭 2026-10-31（控制通道见 C7） |
| C3（重要）动态 N 模型 | W4a:100-103（map 来源、item key、单端口有序列表 join、on_item_error=fail、取消/上限/回放、graph_hash）；W4b:105-109；D4:149；§9 Q2:197 | ✅ 关闭 2026-10-31 |
| C4（重要）trace 迁移 | D5:150 forward-only（新代码迁移旧库后使用；只读 trace/replay 拒绝旧版本 exit 4；每波一次或 W2a 合并；逐版本备份/迁移测试；不承诺直接读旧 schema）；§6.6:163 | ✅ 关闭 2026-10-31 |
| C5（重要）执行/模块计划 | §2:38-39（每模块 worktree、docs 单写）；§4:118-138 波次表（W1-docs/W2a-docs/...；W1/W2a/W2b/W3a/W4b 加 e2e；W2a/W2b/W3a/W4b 双评审；W3 不涉及 harness:87）；§8:188 | ✅ 关闭 2026-10-31（残留 S6） |
| C6（重要）发布 DoD | §6.2:159 质量门禁；§6.3:160 v0.1.0 兼容回归；§6.4:161 性能量化（3 次中位数、AC-01/AC-12 ≤20%、W1 基线文件）；§6.5:162 spec+08；§6.6:163 trace 迁移；§6.7:164 文档收口/worktree 清理；§6.8:165 release | ✅ 关闭 2026-10-31（§6.3 口径残留 C8） |

### 6.3 未关闭条件（r2 复审新发现）

| 条件 | 关闭要求 | 关闭日期 |
|---|---|---|
| C7（重要）W3a 跨进程 decide/resume 控制通道 | 明确 live run 的 decision 送达机制（trace 表轮询/Unix socket/控制文件/`decide --attach`）或改为 run 退出+resume 并把 W3b 纳入 v0.2；补两进程 e2e、幂等/超时/崩溃窗口；§9 增加用户确认 | — |
| C8（重要）§6.3 兼容清单不可满足 | 改为显式清单：可运行 `valid_*` 子集 + `bad_*` 预期错误码 + `valid_cycle`/`valid_subgraph` 已知例外；v0.1.0 golden 用指定样例；补执行命令 | — |
| C9（重要）W2a 显式 rework 边/实例语义 | W2a 增加设计冻结（参照 W4a）：定义 rework 边语法/标记、非自环 attempt-per-activation/instance-per-activation、Completed 目标处理、trace/instance id、max_attempts 绑定；或明确 v0.2 仅自环返工并修正"图级"表述；补非自环返工 e2e（不得 `RUN_DELIVERY_TO_TERMINAL`） | — |
| S6–S8（建议） | 补 e2e 分支/W2b→W3a 排序；补 §9 scope/CLI/N 来源；修 §1.3 非目标句及 §3.5 引用、术语/字段/ codegen 路径/N=3 口径（或登记 ISSUE） | — |

> C1–C6 关闭、C7–C9 未关闭；本记录结论维持"有条件通过"，待 r3 修订后复审。

## 7. 对 DR-004 §9 开放问题的建议答复（r2 版）

> r2 §9 现有 5 项；以下为评审建议，最终以用户确认为准。另见 S7：建议补回 scope/CLI decide/N 来源三项。

1. **W3b durable 是否纳入 v0.2（§9 Q1）**：建议 **W3a（停等 + decide/resume）为 v0.2 必须；W3b（崩溃恢复/durable）默认不纳入 v0.2 发布条件**，但必须先解决 C7（W3a 的控制通道）：若决定采用"run 退出后 resume"路线，则 W3b 实际成为 W3a 的前置，应一并纳入；若采用 live run + 控制通道，则 W3b 可留 v0.3。建议在 DR 中二选一并写明。
2. **W4 join 模型（§9 Q2）**：接受"单端口有序 item 结果列表 + 默认 `on_item_error=fail`"作为 v0.2 默认；W4a 再冻结列表元素形状（item key + 输出 + 错误）、空列表/部分失败/取消与回放断言；动态端口方案留 v0.3。
3. **扇出上限（§9 Q3）**：接受两层（加载期 `max_fanout` 静态 out-degree + 运行期每激活上限），缺省无上限；明确静态超限为 DSL 错误、运行期为确定运行期错误码 + trace，禁止静默截断。
4. **版本与兼容（§9 Q4）**：接受 `v0.2.0` / `M7`；把 v0.1.0 图/L1/L2 回放兼容写成硬性发布条件（已写入 §6.3，但 C8 的清单需修正）。
5. **返工默认值（§9 Q5）**：接受 `on_gate_false` 缺省 `settle`、`max_attempts` 缺省 3；但需补 C9 的"显式 rework 边/非自环实例"设计冻结，否则默认值只对自环有意义。

**建议补入 §9 的用户确认项（S7）**：
- v0.2 范围是否仍为 W1–W4（W3b 可选、W5 共享队列 v0.3）？共享队列是否维持 v0.3？
- CLI `decide`（+`resume`）单机接口是否够用；若要求"不重启进程"则 C7 的控制通道方案需用户接受。
- 动态 N 来源：确认"entry input 或上游输出列表"足够，还是需要运行期外部注入（若需要，属额外机制）。
- 其余（D5 迁移粒度、W1 fake/real LLM、docs/e2e 波次、性能基线）建议在 DR 正文标为"评审默认"，无需用户逐项拍板。
