# REV-062 · docs · software_team SOP 设计稿 v1（设计角色主链 + 工作 agent + per-item 评审）

- **分支**：`docs/docs-software-team-sop-v1`
- **作者**：`lead`（设计稿初版 `05cab0d`、修复提交 `fa53677`；提交正文含 `Review: REV-062` / `Agent: lead`）
- **评审人**：`reviewer-sop`（**≠ 作者**；独立评审；首轮评审提交 `b07897c`）
- **变更范围**：`5d43c58..fa53677`：初版 `05cab0d` 新增 `doc/design/software-team-sop-v1.md`（284 行）+ `doc/index.md` 1 行；修复 `fa53677` 仅改设计稿（296 行 diff，+170/−126）；评审记录与索引由 `b07897c`（reviewer-sop）增加。全部在 `doc/**`，**无代码/测试/配置改动**；复审前 worktree clean；base 即 `origin/main`（`5d43c58`）
- **评审时间**：2026-10-31（首轮 + 增量复审同日）
- **关联**：DR-001（技术路线）；spec v1.2 分册 00/01/02/03/04/05；ISSUE-011/ISSUE-062；`legacy/config/software_team_graph.json`、`legacy/graph_base_design.md`（冻结只读）；用户 2026-10-31 确认的设计方向

## 1. 结论

**结论：有条件通过**

> 首轮（`05cab0d`）为有条件通过；修复 `fa53677` 已关闭 C1–C5、落实 S1–S7（证据见 §6.2/§6.3）。**增量复审新发现 C6（阻塞）**：评审"内部重做"的产物没有进入下游的通道（见 §3 C6 / §6.5）。**C6 关闭前不得合并**（`merge.sh` 只接受记录结论为"通过"）。

方向判定：设计稿忠实于用户确认的方向——设计类角色是**主链关键节点**、设计角色**派发工作 agent**、工作 agent 产物由**评审节点**评审（`design_reviewer` 资产被多个节点共享，替代共享评审队列）、下游 `trigger: "all"` **等 inputs 到齐**；人工评审、图级有限返工、每节点最大扇出限制、动态 N、共享评审队列均已**明确延期到下一版本**（§1.2/§9 与用户口径一致，未把延期能力写成已支持）。

> 首轮问题与四维结论见 §2/§3（保留原文，行号对应 `05cab0d` 版本）；复审后四维复核见 §6.1。

## 2. 四维评审

> 下表为首轮（`05cab0d`）四维结论，保留原文；复审复核见 §6.1（功能正确性因 C6 仍为 ⚠️）。

| 维度 | 结论 | 依据（文件/行 + 实测） |
|---|---|---|
| 功能正确性 | ⚠️ | 方向与范围正确；§1.2 与 §9 对延期项的边界如实（人工/图级返工/扇出上限/动态 N/共享队列均未写进 v1）。但**产物交接断裂**：§3 的 `P` 只输出 verdict/comments/artifact_summary（`:70,:127`），P→T→D_{i+1} 无 W→下游 的边，下游设计角色拿不到工作 agent 产物（C1）；而 §4:101 又要求"评审通过后才把产物交给下游"。**失败契约不成立**：评审返回 `verdict:"rework"` 是 §5.1 自己定义的合法输出（`:119`），正常返回时 `P` 解析成功 → gate=false → 内核结算边并级联 Skipped → run 仍 `Completed`（01 §1.9；实测见 §4），与 D8/§5.1 的 `Crashed → run Failed` 冲突（C2）。 |
| 代码结构合理性（文档结构/边界） | ⚠️ | 单一设计稿 + 一行 `doc/index.md` 索引，改动范围收敛、索引链接有效；章节顺序（目标/取舍→决策→模式→实例→gate/失败→资产→codegen→legacy 对照→延期→限制→验收→附录）合理。但 §3 模式、§4 参考实例、§5 失败契约、§7 codegen、附录 A 骨架之间**模型描述不统一**，参考实例不能直接作为 codegen 输入（C3/C4）；附录 B 的约束清单本身准确，却被附录 A 违反。 |
| 逻辑性 | ❌ | 三处互斥：①§3:72 称 `T_{i,j}` 是"该 item 的下一阶段节点（编码/测试）"，§7:169-173 为每个 item 生成 T 并 `T_{i,j}→D_{i+1}`，而 §4:92-95 的"下一阶段 T"列写的是"进入 archer / 汇入 mde / 汇入 te 与 fo / 汇入 fo/mto/sto / 汇入 mto"，把 T 当下一设计角色并引入跨阶段多目标汇入——三者不能同时成立；§7 的单目标规则无法生成 §4 的汇入关系，且 §4 的汇入关系与 legacy 依赖不一致（legacy：archer→{mde,te}、mde→fo、te→{mte,ste}、fo→{mto,sto}、mto→sto）。②D8/§5.1（`:49,:122`）与 §5.4（`:145`）及内核 Skipped 语义（03 §3.4.2）冲突。③`:87` 称"按 §9 的 v0.2 项处理"，但 §9（`:195-203`）没有"并行设计阶段"条目。 |
| 可读性 | ⚠️ | 标题/状态/关联/取舍表齐全，代码块与流程图可读，legacy 对照表、已知限制、验收计划均具备；但版本号混乱（本设计为 v1，延期版却叫 v0.2，§11 标题又写"v0.2 落地时使用"而验收内容属 v1 落地）；术语不严（`:48/:120` 把 `max_iterations` 说成"重做次数"）；若干断言未加限定（`:122,:140`），易被后续实现者当成事实。 |

## 3. 发现清单

> 下表为首轮发现（行号按 `05cab0d`）；状态列已按 `fa53677` 更新。C6 为增量复审新发现（行号按 `fa53677`，见 §6.5）。

| # | 级别 | 位置 | 问题 | 建议 | 状态 |
|---|---|---|---|---|---|
| C1 | **阻塞** | `doc/design/software-team-sop-v1.md:68-73,101-102,126-128,262-268` | **产物交接链断裂**。§3 的 `P_{i,j}` 只输出 `{verdict, comments, artifact_summary}`（`:70`、`:127`），`P→T` 是唯一向下的数据边；图上不存在 `W_{i,j}→下游` 的边。因此工作 agent 的原始产物（如 `doc/architectural_design.md` 内容）在 `P` 之后即从主链消失，下游设计角色/`end` 只能得到评审摘要，这与 §4:101「评审通过后才把产物交给下游」、§1.1 的"产物交接"目标以及用户"下游节点等 inputs 到齐"的方向矛盾。附录 A 同构复现：`planer_req_parse → archer`（`:267`），`archer` 拿不到 `planer_req` 的产物 | 明确一种可实现的传播机制并同步 §3/§4/§5.2/§7/附录 A/§11；候选：(a) `P` 增加 W 产物输入端并原样透传（输出加 `artifact` 全文）；(b) 下游节点用两个端口分别接收 `artifact`（W→下游）与 `verdict`（P→下游）；(c) 明确 `artifact_summary` 承载全文并给出字段契约。§11 补"下游收到完整产物"断言 | 已关闭（`fa53677`） |
| C2 | **阻塞** | 同上 `:49,102,119,122,140,210` | **D8「只保留 approved 路径、不通过一律 run Failed」无机制保证，且与内核实测相反**。评审 agent 输出 `verdict:"rework"` 是 §5.1 定义的合法内容（`:119`）；agent_call 正常返回后 `P` 解析成功，gate=false 时内核按 04 §4.6.1 / 03 §3.4.2 只**结算该边并级联 Skipped**，run 仍 `Completed`（01 §1.9：无 end 执行 → `run_output=null` 但合法 Completed），不会 Crashed/Failed。实测（§4）：`p→end` 边被 gate 拦截后 `instances: end=Skipped`、`runs.status=Completed`、`run_output=null`。故 `:122`「不存在评审不通过但 run 继续的路径」与 `:49`「最终仍不通过 → 评审 agent 报错」不成立；`:140`「不会走到这里」只是假设 | 把强制失败落到唯一可执行节点：规定 `P` 在 `verdict != "approved"`（或无法解析）时**抛 Python 异常**（`PythonException → Crashed → run Failed`），并改写 `:49/:102/:122/:140/:210`；或反过来明确接受"门控不通过 = run Completed(null)"为 v1 行为并推翻 D8。§11 增加"最终 rework → run Failed"验收 | 已关闭（`fa53677`） |
| C3 | **阻塞** | 同上 `:72-73,87,92-95,105,169-173` | **`T_{i,j}` 语义三处互斥，§4 汇入关系不可由 §7 规则生成**。§3:72 定义 T 为"该 item 的下一阶段节点（编码/测试）"，§7:169-173 逐 item 生成 T 并 `T_{i,j}→D_{i+1}`；但 §4:92-95 的"下一阶段 T"列把 T 当作下一设计角色（"进入 archer / 汇入 mde / 汇入 te 与 fo / 汇入 fo/mto/sto / 汇入 mto"）。衍生问题：(1) T 若是独立逐 item 节点，其类型/职责/输出未定义；(2) 若 T 即 D_{i+1}，§7 的 `T→D_{i+1}` 不成立；(3) §7 只支持"上一阶段→下一阶段"单目标，无法表达 §4 的 `mde→{te,fo}`、`te→{fo,mto,sto}`，且 §4 漏了 legacy 的 `archer→te`、`fo→sto`；(4) 汇入关系与 legacy 依赖不一致（mde 不喂 te；te 不喂 fo；fo 喂 mto+sto）；(5) 同阶段并行 W 无法依赖同阶段产物——legacy `te→{mte,ste}` 的模块/系统用例依赖"总体测试计划"，§4 却把三者拆成 te 阶段并行兄弟工作 agent；(6) `:87` 引用的"§9 的 v0.2 项（并行设计阶段）"在 §9 不存在 | 定稿前统一 T 语义（建议二选一：T = 下游设计角色 `D_{i+1}`；或 T = 明确的 per-item 产物节点），用一张"阶段 × item → 下游端口"映射表重写 §4 的"下一阶段 T"列，并据此重写 §7 codegen；对 legacy 的跨阶段/阶段内依赖给出显式方案（拆阶段 / 引入逐 item 依赖 / 在 D_i 任务包内联前置产物）；修 `:87` 交叉引用或在 §9 增加"并行设计阶段"项。§11 增加"依赖顺序与 legacy 语义一致"断言 | 已关闭（`fa53677`） |
| C4 | **重要** | 同上 `:230-273`、`:280` | **附录 A 骨架过不了 `zgraph validate`**。图注称"节选，非可运行 fixture"，但实测 `DSL_UNCONNECTED_INPUT: node 'end' has no incoming edge`（§4），与附录 B:280「非 entry 节点入边 ≥1」自述约束直接冲突；只补一条 `planer_req_parse→end.result` 边后即 `graph valid: 6 nodes, 6 edges`。作为"codegen 前的唯一 DSL 示例"，读者照抄会 validate 失败；且骨架本身固化了 C1（`parse` 的 verdict 摘要直接进 `archer`，无产物端口） | 补 `end` 入边（如 `planer_req_parse→end.result`，或按修正后的 §3 增加 T 节点→end），并在图注写明"已用 zgraph validate 验证（命令+输出）"；若坚持节选，则显式声明"未补 end 入边，直接 validate 会报 DSL_UNCONNECTED_INPUT"，并补 C1 的产物端口 | 已关闭（`fa53677`） |
| C5 | **重要** | 同上 `:34` | **对用户确认方向的表述与 D3 矛盾**。依据行写"设计角色负责评审"，但 D3（`:44`）是"每个工作 agent 配独立评审节点，所有评审节点引用同一个 `design_reviewer` 资产"，§3:69 是独立评审 agent 节点（`R_{i,j}`），§1.1:18 也写"评审 agent 评审"；用户 2026-10-31 确认的是"评审节点 + 多节点共享同一评审 agent 资产"（替代共享评审队列） | 改为"评审节点（共享 `design_reviewer` 资产）负责评审" | 已关闭（`fa53677`） |
| S1 | 建议 | 同上 `:198` | "当前带出口自环会 `RUN_DELIVERY_TO_TERMINAL`"过度具体：实测仓库 `tests/fixtures/graphs/valid_cycle.json` 在真内核报 `RUN_DEADLOCK`（5017ms）；`RUN_DELIVERY_TO_TERMINAL` 是 ISSUE-062 描述的第二种失效模式（trigger=any 且出口提前完成时） | 写"`RUN_DEADLOCK` / `RUN_DELIVERY_TO_TERMINAL`（视 trigger/gate 配置）"，与 ISSUE-062 两种失效模式对齐 | 已落实（`fa53677`） |
| S2 | 建议 | 同上 `:45` | D4 写"节点实例可独立重试/超时"偏宽：v1 `retry.max_attempts` **仅自环/显式重试**计数，线性节点失败即 Crashed→run Failed，不存在普通节点级重试 | 改为"节点实例可独立超时、独立 trace；重试仅自环（`retry.max_attempts`）" | 已落实（`fa53677`） |
| S3 | 建议 | 同上 `:220-226` | §11 标题"后续验收计划（v0.2 落地时使用）"与内容不符：1–5 项（codegen N=3、per-item 评审、等待语义）都是**v1 落地的验收**；本设计是 v1、延期版却称 v0.2，版本号易混。并且验收未覆盖 C1（产物传播）与 C2（最终 rework→Failed、gate 拦截的 run 终态） | 标题改"v1 落地验收"（或补版本命名约定：v1 设计 → 下一版本标签），并补 C1/C2 验收断言；`zgraph validate` 一项注明使用 codegen 产物而非附录 A 节选 | 已落实（`fa53677`） |
| S4 | 建议 | 同上 `:207-216` | §10 未列两个落地风险：①评审 `content` 可能被 `MaxTokens` 截断（05 §5.9：`stop_reason=MaxTokens` 属正常返回）或带 Markdown 围栏，`P` 解析失败即 Crashed→Failed；②评审 agent 多轮 tool loop 与默认 180s 实例超时（03 §3.9.1）/单次 LLM 60s 的预算关系未给出 | §10 增加"评审输出 JSON 解析脆弱性"与"评审节点超时预算"两条；§6 给出评审 agent 的 `timeout_ms`/`max_iterations` 推荐值（上限 50 范围内） | 已落实（`fa53677`） |
| S5 | 建议 | 同上 `:157` | "所有节点通过 `agent_id` 引用资产"不准确：`python_node`/`start`/`end` 不引用 agent 资产 | 改为"`agent_call` 节点通过 `agent_id` 引用资产；资产根由 CLI `--assets-dir` / `ZGRAPH_ASSETS` 提供" | 已落实（`fa53677`） |
| S6 | 建议 | 同上 `:48,:120` | "重做次数受 `max_iterations`（默认 10，上限 50）约束"术语不准：`max_iterations` 是**LLM 调用轮次上限**（05 §5.2.2/§5.5.2），不是重做次数；一次迭代可含多个 tool call | 改为"评审 agent 的 LLM 轮次预算受 `max_iterations` 约束（默认 10，上限 50）；重做的 tool call 计入该预算" | 已落实（`fa53677`） |
| S7 | 建议 | 同上 `:153-158` | 未说明工作 agent 的实现载体与既有资产复用：main 已有 `python/workers/software_team/{planer,archer}.py`（M3 迁移的纯函数 worker，legacy `planer_proc`/`archer_proc` 语义），设计未提是否作为 W（`python_node`）复用 | 在 §3/§4/§6 说明各 item 的 W 载体（agent / python_node）与复用关系，避免重复实现 | 已落实（`fa53677`） |
| C6 | **阻塞** | `fa53677` 版 `§5.1:122-133`、`§5.2:135-142`、`§3:71-78`、`§7:198-206` | **评审"内部重做"的产物没有进入下游的通道（C1 修复后暴露的新矛盾）**。§5.1 规定 `verdict="rework"` 时评审 agent 调用修复工具"重做"并重新评审；但按 §5.2，`P` 的 `artifact` 输入端来自 `W`、输出"原样透传 `W` 的整包产物"。`R`（评审 agent）的输出固定为四字段 `{content, stop_reason, iterations, usage}`（05 §5.9），修复工具的结果只存在于 `R` 的 tool-loop 上下文；§5.1 的 `content` 示例也只有 `verdict/comments`。因此即使重做后评审为 approved，下游拿到的仍是**重做前的 W 产物**；若重做不改变 W 产物，则要么评审放行陈旧产物、要么 `P` 抛错 → run Failed，"内部重做"形同虚设（D7/§5.4「质量不合格走内部重做消化」不成立）。注：W 的 output 在 04 §4.5 中是整包广播的不可变值，`R` 的 tool 调用无法写回 W 实例输出 | 需在定稿前给出可执行通路，三选一：(a) 给 `R` 的输出契约增加 `artifact`（修复工具产出的最终产物），`P` 优先采用 `review.content.artifact`、缺省回退 `W.artifact`，并同步 §5.1/§5.2/§7/附录 A/§11；(b) 明确"内部重做"=只做检查/校验、不修改产物（删除"修复工具重做"表述，重写 D7/§5.1/§5.4 的语义）；(c) 其它能证明修复产物进入 `P` 的显式机制（若需要框架/内核改动，则应移入 v0.2 并相应调整 D7/D8） | open |

> 级别口径：阻塞 = 必须修复后才能给"通过"；重要 = 必须修复或登记 ISSUE；建议 = 可后置，但需修复或进台账。

## 4. 验证命令与结果

> 以下为首轮（`05cab0d`）证据，保留原文；增量复审（`fa53677`）证据见 §4.1。

```bash
# --- 范围与干净度（首轮评审前）---
cd /Users/zyzs/lczyzs/code/zgraph/.worktrees/docs-lead
git diff 5d43c58..HEAD --stat
#  doc/design/software-team-sop-v1.md | 284 +++++++++++++++++++++++++++++++++++++
#  doc/index.md                       |   1 +
#  2 files changed, 285 insertions(+)
git diff 5d43c58..HEAD --name-status
#  A doc/design/software-team-sop-v1.md
#  M doc/index.md
git status --porcelain          # 空（评审前 worktree clean）
git rev-parse origin/main HEAD  # 5d43c58 / 05cab0d（base = origin/main，无 rebase 需求）

# --- 仓库自检（工作树内）---
cd /Users/zyzs/lczyzs/code/zgraph/.worktrees/docs-lead && bash scripts/dev/check.sh
# OK user.name=chenyuxiangg / OK user.email=... / OK remote=git@github.com:chenyuxiangg/graph_framework.git
# OK CLAUDE.md 未被修改 / OK 暂存区无禁用文件 / OK 分支命名: docs/docs-software-team-sop-v1
# OK Python 语法检查通过 / OK doc/progress.md 存在 / OK doc/issues.md 存在
# OK doc/progress.md 非空行 14（目标 ≤30）/ OK cargo fmt --check 通过 / OK git diff --check 通过
# CHECK PASSED  [exit=0]

# --- 索引链接 ---
grep -n "design/software-team-sop-v1.md" doc/index.md
# :9 新增行链接目标存在（无断链）

# --- 附录 A 可加载性（首轮：把 232–271 行 JSON 提取到 /tmp/rev062_appendixA.json）---
python3 - <<'PY'
from pathlib import Path
lines = Path("doc/design/software-team-sop-v1.md").read_text(encoding="utf-8").splitlines()
start = next(i for i, ln in enumerate(lines) if ln.strip().startswith("## 附录 A"))
inside, js = False, []
for ln in lines[start:]:
    if ln.strip() == "```json":
        inside = True
        continue
    if inside and ln.strip() == "```":
        break
    if inside:
        js.append(ln)
Path("/tmp/rev062_appendixA.json").write_text("\n".join(js), encoding="utf-8")
PY
/Users/zyzs/lczyzs/code/zgraph/target/debug/zgraph validate /tmp/rev062_appendixA.json
# error[DSL_UNCONNECTED_INPUT] /tmp/rev062_appendixA.json: node 'end'
#   node 'end' has no incoming edge
#   expected: "at least one incoming edge"
#   actual:   0
# 1 error(s)   [exit=2]
# 补一条 {from:"planer_req_parse",to_node:"end",to_input_port:"result"} 后：
# ✓ graph valid: 6 nodes, 6 edges, 0 subgraphs (graph_hash=f8fea2b1210fb9c8)

# --- gate 拦截语义探针（main 工作区构建的 CLI，base 5d43c58；docs 分支无代码差异）---
# 图：entry python_node p(workers.echo.run) --gate check_output{path:"echo",equals:"nope"}--> end
/Users/zyzs/lczyzs/code/zgraph/target/debug/zgraph run /tmp/rev062_gate_blocked.json \
  --inputs '{"p":{"message":"hi"}}' \
  --python /Users/zyzs/lczyzs/code/zgraph/.venv/bin/python --python-path python \
  --db /tmp/rev062-trace.db --json
# {"ok":true,...,"status":"Completed","event_count":8,...,"run_output":null}
sqlite3 /tmp/rev062-trace.db "select node_id,attempt,status from instances;"
# p|0|Completed
# end|0|Skipped
sqlite3 /tmp/rev062-trace.db "select status,output_json from runs;"
# Completed|
# => gate=false 只结算边 + Skipped 级联，run 仍 Completed（run_output=null），不会 run Failed

# --- 带出口环 fixture 的运行期行为 ---
/Users/zyzs/lczyzs/code/zgraph/target/debug/zgraph run tests/fixtures/graphs/valid_cycle.json \
  --inputs '{}' --python /Users/zyzs/lczyzs/code/zgraph/.venv/bin/python --python-path python \
  --db /tmp/rev062-cycle2.db --json
# {"ok":false,...,"status":"Failed",...,"error":{"code":"RUN_DEADLOCK",
#   "message":"no runnable instance for 5002 ms while Pending instances remain"}}

# --- 同一 agent_id 被多个节点引用（R14c 只查存在性）---
# 两个 agent_call 节点同引用 design_reviewer；/tmp/rev062-assets/agents/design_reviewer.json 提供索引
/Users/zyzs/lczyzs/code/zgraph/target/debug/zgraph validate /tmp/rev062_shared_agent.json \
  --assets-dir /tmp/rev062-assets
# ✓ graph valid: 4 nodes, 3 edges, 0 subgraphs (graph_hash=52b9bd9dba110144)
```

- 规格/实现核对（文档断言 ↔ 依据，均为一致项）：`agent_call` 输出固定四字段（05 §5.9）；`max_iterations` 默认 10/上限 50、`HARNESS_MAX_ITERATIONS → Crashed → Failed`（05 §5.2.2/§5.7）；gate 在发布者执行任务内、投递前求值、只拦截边（04 §4.6.1、`crates/zgraph-kernel/src/gate.rs:1-45`）；gate 拦截后的下游 Skipped/`RUN_DEADLOCK`（03 §3.4.2/§3.9.3/§3.15、`crates/zgraph-kernel/src/run.rs:1010-1035,1253-1310`）；`end` 约束（02 §2.4 R4/R12、01 §1.9）；`subgraph` 加载通过/运行期拒绝（02 §2.6.4）；`WaitingHuman` 不产生（00 §0.3 注）；每 run worker 上限 8（03 §3.5/§3.13、`crates/zgraph-worker/src/executor.rs:38`）；工具串行（05 §5.5.3）；R14c 仅查存在性（02 §2.6.3）；`--assets-dir`/`ZGRAPH_ASSETS`（`crates/zgraph-cli/src/validate.rs:100-104`、`run_cmd.rs:98,120`）。
- legacy 核对：`legacy/config/software_team_graph.json` 为 12 节点/25 边、11 条 `*_loop` 自环、5 条 `driver:"manual"`、`tick_period_s:300` —— 文档 §1.2/§8 的这些数字**正确**；但首轮 §4 的"下一阶段 T"汇入关系与 legacy 依赖不一致（C3，已在 `fa53677` 改为依赖矩阵）。
- 本分支仅文档改动，未跑 `cargo test`/`pytest`（合并门禁会执行 build + workspace test + clippy + pytest；本次 `check.sh` 已含 `cargo fmt --check` 且无代码可测）。探针使用 main 工作区 12:56 构建的 `target/debug/zgraph`（base `5d43c58` 与分支零代码差异）。

### 4.1 增量复审证据（`fa53677`）

```bash
# --- 修复范围（只应改设计稿）---
git diff b07897c..fa53677 --stat
#  doc/design/software-team-sop-v1.md | 296 ++++++++++++++++++++++++-------------
#  1 file changed, 170 insertions(+), 126 deletions(-)
git diff 05cab0d..fa53677 --name-status
#  M doc/design/software-team-sop-v1.md
#  M doc/reviews/README.md            （上一评审提交 b07897c）
#  A doc/reviews/REV-062-docs-software-team-sop-v1.md（同上）
git status --porcelain              # 空（复审前 worktree clean）
git rev-parse HEAD                  # fa53677

# --- 自检 ---
bash scripts/dev/check.sh
# CHECK PASSED（身份/冻结文件/分支/语法/cargo fmt/git diff --check 全 OK）

# --- 附录 A 重新提取并实跑（提取脚本同 §4，输出 /tmp/rev062_appendixA_v2.json；39 行）---
/Users/zyzs/lczyzs/code/zgraph/target/debug/zgraph validate /tmp/rev062_appendixA_v2.json
# ✓ graph valid: 5 nodes, 6 edges, 0 subgraphs (graph_hash=15f4501150237554)   [exit 0]
# 与设计稿 :275 图注哈希一致；补 assets-dir 时会按 R14c 报 DSL_AGENT_NOT_FOUND（设计稿未声明资产索引，属预期）

# --- C2 机制探针：python_node 抛未捕获异常 ---
# /tmp/rev062_probe/probe_worker.py 的 run() 直接 raise ValueError("bad review json")
/Users/zyzs/lczyzs/code/zgraph/target/debug/zgraph run /tmp/rev062_raise.json \
  --inputs '{"p":{"message":"hi"}}' \
  --python /Users/zyzs/lczyzs/code/zgraph/.venv/bin/python --python-path python \
  --python-path /tmp/rev062_probe --db /tmp/rev062-raise.db --json
# {"ok":false,"status":"Failed",...,"error":{"code":"NODE_PYTHON_EXCEPTION",
#   "message":"node 'p' failed: python exception: bad review json"}}
# => P 抛异常 → Crashed → run Failed 的机制成立（C2 关闭依据；常量见 crates/zgraph-kernel/src/types.rs:77/466）

# --- 参考图规模核算（按 §4.1 依赖矩阵）---
# 16 work items；节点 11 + 16*3 + 1 = 60；边 16*5 + P→下游 20 = 100 ✓（与 §4.1/§7/§8 一致）
```

- legacy 依赖矩阵逐项比对（`legacy/config/software_team_graph.json` 的 iport）：`mde/te ← archer 3 份`、`mte/ste ← te`、`fo ← mde 2 份`、`mto ← mte+fo`、`sto ← ste+mto`、`qa ← sto`、`release ← qa`、`end ← release 3 份` —— 与修订后 §4.1 一致（C3 关闭依据）。
- C6 依据：`agent_call` 输出固定四字段、`content` 为字符串（05 §5.9/§5.9.1）；节点 output 为**不可变整包**，每条出边收到同一份值（04 §4.5）；设计稿 §5.2 规定 `P.artifact` 来自 `W`、原样透传 `W` 的产物；§5.1 的 `R` 输出 `content` 只含 `verdict/comments`。评审修复工具的结果仅存在于 `R` 的 tool-loop 上下文，没有进入 `P.artifact` 的通道（见 §3 C6 / §6.5）。

## 5. 遗留问题

- **C1–C5 已关闭、S1–S7 已落实（`fa53677`，证据见 §6.2/§6.3）**。
- **C6（阻塞）为新条件，未关闭**：评审内部重做的产物没有进入下游的通道（§6.5）；关闭前本记录结论保持"有条件通过"，不得合并。
- R1–R6 为复审新发现的可读性/口径建议（§6.4，不阻塞）；建议作者顺手修复或由 Lead 登记 `doc/issues.md`（本评审按授权不改设计稿/台账）。
- 修复提交 `fa53677` 只改设计稿，未越界（无代码/测试/配置/评审记录改动）；branch base = `origin/main`，复审前 worktree clean；本评审未 merge/push/切 main。

## 6. 条件关闭记录（复审 `fa53677`）

**关闭提交范围**：`b07897c..fa53677`，1 文件 +170/−126（仅 `doc/design/software-team-sop-v1.md`）；worktree clean；`bash scripts/dev/check.sh` = CHECK PASSED。

### 6.1 四维复核（复审后）

| 维度 | 复审结论 | 依据 |
|---|---|---|
| 功能正确性 | ⚠️（C6 未闭） | C1 产物透传与 C2 强制失败已闭合（§3 C1/C2 状态 + §6.2）；但 C6 使"内部重做"无法改变下游产物，功能链路仍有缺口（§6.5） |
| 代码结构合理性 | ✅ | 模型统一为"`D_i` + `W/R/P` + 依赖矩阵"：§3 模式、§4.1 矩阵、§7 codegen、附录 A 互相一致；无 `T` 概念残留（§3:80 明示） |
| 逻辑性 | ✅（C6 除外） | 依赖关系与 legacy iport 一致；并发关系（mde/te、mte/ste）正确；失败路径唯一（`P` 抛错）、gate 仅不变量；附录 A validate 通过 |
| 可读性 | ✅ | 版本约定/标题/条款清晰，S1–S7 已改写；残留 R1–R6 为可读性/口径建议（§6.4，不阻塞） |

### 6.2 C1–C5 关闭证据

| 条件 | 关闭证据（`fa53677`） | 结论/日期 |
|---|---|---|
| C1（阻塞）产物交接 | §3:71-78 新增 `W→R/P` 两条数据流与"产物传播（关闭 C1）"段；§5.2:138-142 规定 `P` 输入 `review+artifact`、输出含 `artifact` 原样透传；§7:201 生成 `W→P.artifact`；附录 A:311 新增该边；§11:265 新增"下游收到完整产物"验收。复审实测附录 A validate 通过 | ✅ 关闭 2026-10-31 |
| C2（阻塞）失败契约 | D8:50、§5.1:130-133、§5.2:140-141、§5.3:150-151、§10:247、§11:266 统一为"`P` 在非 approved / 非法 JSON 时抛异常 → `NODE_PYTHON_EXCEPTION` → `Crashed` → run `Failed`"，并写入 gate=false 实测结论；复审异常探针实测 `{"status":"Failed","error":{"code":"NODE_PYTHON_EXCEPTION",...}}` | ✅ 关闭 2026-10-31 |
| C3（阻塞）T 语义/codegen | 全文删除 `T` 概念（§3:80 明示）；§4.1 改为依赖矩阵并逐项对齐 legacy iport；§4.2 给出依赖 DAG 执行示例；§7:177-212 改为依赖矩阵驱动（每依赖 item 一个端口、`P→下游/end` 带 approved gate）；删除"§9 并行设计阶段"引用 | ✅ 关闭 2026-10-31 |
| C4（重要）附录 A | 附录 A 改为 5 节点/6 边（含 `W→P.artifact`），无悬空 `end`；复审实跑 `zgraph validate /tmp/rev062_appendixA_v2.json` = `✓ graph valid: 5 nodes, 6 edges, 0 subgraphs (graph_hash=15f4501150237554)`（exit 0），与图注 `:275` 哈希一致 | ✅ 关闭 2026-10-31 |
| C5（重要）方向表述 | `:35` 改为"评审由共享 `design_reviewer` 资产的评审节点执行（设计角色定义评审标准并对结论负责，而非由设计 agent 亲自评审）"，与 D3/§1.1/§3 一致 | ✅ 关闭 2026-10-31 |

### 6.3 S1–S7 落实证据

| 建议 | 落实证据（`fa53677`） | 结论 |
|---|---|---|
| S1 | §9 第 2 项（`:235`）改为"`RUN_DEADLOCK` 或 `RUN_DELIVERY_TO_TERMINAL`（视 `trigger`/gate 配置，ISSUE-062 的两种失效模式）" | ✅ 落实 |
| S2 | D4（`:46`）改为"节点实例独立超时/独立 trace；重试仅自环（`retry.max_attempts`），v1 不使用自环" | ✅ 落实 |
| S3 | §11（`:259`）标题改"v1 落地验收"；新增产物传播（`:265`）/失败路径（`:266`）/等待（`:267`）/规模（`:268`）验收 | ✅ 落实 |
| S4 | §10（`:249-250`）新增 JSON 截断（MaxTokens）/围栏与评审超时预算风险；§6（`:165,170`）给出 `max_iterations` 10–20、`node_timeout_ms` 300000 | ✅ 落实 |
| S5 | §6（`:168`）改为"只有 `agent_call` 节点通过 `agent_id` 引用资产"，并注明 `python_node`/`start`/`end` 的配置方式 | ✅ 落实 |
| S6 | D7（`:49`）/§5.1（`:131`）明确 `max_iterations` 是 LLM 调用轮次预算、重做 tool call 计入该预算 | ✅ 落实 |
| S7 | §6（`:171`）说明复用 `python/workers/software_team/{planer,archer}.py`；D11（`:53`）同步 | ✅ 落实 |

### 6.4 复审新发现（建议，不阻塞）

| # | 级别 | 位置（`fa53677`） | 问题 | 建议 |
|---|---|---|---|---|
| R1 | 建议 | `§7:185` | codegen 伪代码 `inputs=每个 dep item 一个端口` 对 entry `planer` 为空，未生成 run 输入 `requirement`（§4.1:90 已声明"run 输入"）；照伪代码实现会丢 entry 输入 | 在 §7 为 `planer` 显式生成 `requirement` 端口（并说明由 run 注入） |
| R2 | 建议 | `§7:212`、`§11:263,268` | "N=3 参考图（60 节点/100 边）"与 §4.1 每角色 N_i=1–3、16 items 的参考实例口径不一致（若 N=3 指统一扇出，规模不是 60/100） | 改称"§4.1 参考实例（N_i 见该表；60/100）"，或在 §4.1 定义 N=3 的确切含义 |
| R3 | 建议 | `附录 B:325` | "两种失效模式见 §9.2"：§9 是编号列表、无 §9.2 小节 | 改为"见 §9 第 2 项" |
| R4 | 建议 | `§3:61-68` | ASCII 图未画 `W→P.artifact` 分支（文字 `:71/:78` 与附录 A 已体现），易被读成线性 `W→R→P` | 图中补 `W ──▶ P.artifact` 分支 |
| R5 | 建议 | `§10:248` | "无 artifact 端口"与新增的 `P.artifact` 端口术语冲突（本条意为"无框架级 artifact 存储"） | 改为"无框架级 artifact 端口/存储" |
| R6 | 建议 | `doc/index.md:9` | 索引摘要仍写"设计角色主链"，修订后 v1 是依赖 DAG（"主链关键节点"仍成立，但"主链"易误读为线性） | 合并/收尾时改为"设计角色依赖 DAG + 工作 agent + per-item 评审" |

### 6.5 新条件 C6（阻塞，未关闭）

- **位置（`fa53677`）**：§5.1:122-133、§5.2:135-142、§3:71-78、§7:198-206。
- **问题**：`verdict="rework"` 时评审 agent 调用修复工具"重做"并重新评审（§5.1:131），但 `P` 的 `artifact` 输入来自 `W`、输出原样透传 `W` 的整包产物（§5.2:138-142）；`R` 的输出固定四字段、`content` 只含 `verdict/comments`（05 §5.9；§5.1:127 示例）——修复工具的结果留在 `R` 的 tool-loop 上下文，无法写回 `W` 的不可变 output（04 §4.5）。结论：重做不会改变下游拿到的产物，"内部重做"（D7、§5.4:156）没有可执行通路。
- **关闭条件（三选一）**：
  1. `R` 输出契约增加 `artifact`（修复工具产出的最终产物），`P` 优先采用 `review.content.artifact`、缺省回退 `W.artifact`，并同步 §5.1/§5.2/§7/附录 A/§11；
  2. 明确"内部重做"= 只做检查/校验、不修改产物（删除"修复工具重做"表述，重写 D7/§5.1/§5.4 语义）；
  3. 其它能证明修复产物进入 `P` 的显式机制；若需要框架/内核改动（如回写上游输出/新增产物通道），应移入 v0.2，并相应调整 D7/D8。
- **验证要求**：修订后给出最小端到端断言（fake LLM：rework → approved，断言下游 `artifact` 是重做后的产物）；本记录届时新增关闭证据并改判。

> **复审结论**：C1–C5、S1–S7 全部关闭/落实；新条件 C6（阻塞）未关闭，本记录结论维持"有条件通过"（`merge.sh` 不会放行），待作者修复 C6 后再次复审。
