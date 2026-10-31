# REV-062 · docs · software_team SOP 设计稿 v1（设计角色主链 + 工作 agent + per-item 评审）

- **分支**：`docs/docs-software-team-sop-v1`
- **作者**：`lead`（提交 `05cab0d`；提交正文含 `Review: REV-062` / `Agent: lead`）
- **评审人**：`reviewer-sop`（**≠ 作者**；独立评审）
- **变更范围**：`5d43c58..05cab0d`，2 文件 +285/−0：新增 `doc/design/software-team-sop-v1.md`（284 行）、`doc/index.md` +1 行索引；全部在 `doc/**`，**无代码/测试/配置改动**；worktree clean；base 即当前 `origin/main`（`5d43c58`）
- **评审时间**：2026-10-31
- **关联**：DR-001（技术路线）；spec v1.2 分册 00/01/02/03/04/05；ISSUE-011/ISSUE-062；`legacy/config/software_team_graph.json`、`legacy/graph_base_design.md`（冻结只读）；用户 2026-10-31 确认的设计方向

## 1. 结论

**结论：有条件通过**

方向判定：设计稿忠实于用户确认的方向——设计类角色为**主链关键节点**、设计角色**派发工作 agent**、工作 agent 产物由**评审节点**评审（`design_reviewer` 资产被多个节点共享，替代共享评审队列）、下游 `trigger: "all"` **等 inputs 到齐**；人工评审、图级有限返工、每节点最大扇出限制、动态 N、共享评审队列均已**明确延期到下一版本**（§1.2/§9 与用户口径一致，未把延期能力写成已支持）。

但核心链路存在 **3 项阻塞 + 2 项重要**：产物交接在评审解析节点后断裂；D8 的"只保留 approved 路径 / 不通过一律 run Failed"无机制保证且与内核实测相反；§3/§4/§7 的 `T_{i,j}` 语义与"下一阶段 T"列、codegen 规则三处互斥；附录 A 骨架实际过不了 `zgraph validate`；行 34 对用户方向的表述与 D3 矛盾。**C1–C5 全部关闭前不得合并**（`merge.sh` 只认"记录结论 = 无条件通过"）。

## 2. 四维评审

| 维度 | 结论 | 依据（文件/行 + 实测） |
|---|---|---|
| 功能正确性 | ⚠️ | 方向与范围正确；§1.2 与 §9 对延期项的边界如实（人工/图级返工/扇出上限/动态 N/共享队列均未写进 v1）。但**产物交接断裂**：§3 的 `P` 只输出 verdict/comments/artifact_summary（`:70,:127`），P→T→D_{i+1} 无 W→下游 的边，下游设计角色拿不到工作 agent 产物（C1）；而 §4:101 又要求"评审通过后才把产物交给下游"。**失败契约不成立**：评审返回 `verdict:"rework"` 是 §5.1 自己定义的合法输出（`:119`），正常返回时 `P` 解析成功 → gate=false → 内核结算边并级联 Skipped → run 仍 `Completed`（01 §1.9；实测见 §4），与 D8/§5.1 的 `Crashed → run Failed` 冲突（C2）。 |
| 代码结构合理性（文档结构/边界） | ⚠️ | 单一设计稿 + 一行 `doc/index.md` 索引，改动范围收敛、索引链接有效；章节顺序（目标/取舍→决策→模式→实例→gate/失败→资产→codegen→legacy 对照→延期→限制→验收→附录）合理。但 §3 模式、§4 参考实例、§5 失败契约、§7 codegen、附录 A 骨架之间**模型描述不统一**，参考实例不能直接作为 codegen 输入（C3/C4）；附录 B 的约束清单本身准确，却被附录 A 违反。 |
| 逻辑性 | ❌ | 三处互斥：①§3:72 称 `T_{i,j}` 是"该 item 的下一阶段节点（编码/测试）"，§7:169-173 为每个 item 生成 T 并 `T_{i,j}→D_{i+1}`，而 §4:92-95 的"下一阶段 T"列写的是"进入 archer / 汇入 mde / 汇入 te 与 fo / 汇入 fo/mto/sto / 汇入 mto"，把 T 当下一设计角色并引入跨阶段多目标汇入——三者不能同时成立；§7 的单目标规则无法生成 §4 的汇入关系，且 §4 的汇入关系与 legacy 依赖不一致（legacy：archer→{mde,te}、mde→fo、te→{mte,ste}、fo→{mto,sto}、mto→sto）。②D8/§5.1（`:49,:122`）与 §5.4（`:145`）及内核 Skipped 语义（03 §3.4.2）冲突。③`:87` 称"按 §9 的 v0.2 项处理"，但 §9（`:195-203`）没有"并行设计阶段"条目。 |
| 可读性 | ⚠️ | 标题/状态/关联/取舍表齐全，代码块与流程图可读，legacy 对照表、已知限制、验收计划均具备；但版本号混乱（本设计为 v1，延期版却叫 v0.2，§11 标题又写"v0.2 落地时使用"而验收内容属 v1 落地）；术语不严（`:48/:120` 把 `max_iterations` 说成"重做次数"）；若干断言未加限定（`:122,:140`），易被后续实现者当成事实。 |

## 3. 发现清单

| # | 级别 | 位置 | 问题 | 建议 | 状态 |
|---|---|---|---|---|---|
| C1 | **阻塞** | `doc/design/software-team-sop-v1.md:68-73,101-102,126-128,262-268` | **产物交接链断裂**。§3 的 `P_{i,j}` 只输出 `{verdict, comments, artifact_summary}`（`:70`、`:127`），`P→T` 是唯一向下的数据边；图上不存在 `W_{i,j}→下游` 的边。因此工作 agent 的原始产物（如 `doc/architectural_design.md` 内容）在 `P` 之后即从主链消失，下游设计角色/`end` 只能得到评审摘要，这与 §4:101「评审通过后才把产物交给下游」、§1.1 的"产物交接"目标以及用户"下游节点等 inputs 到齐"的方向矛盾。附录 A 同构复现：`planer_req_parse → archer`（`:267`），`archer` 拿不到 `planer_req` 的产物 | 明确一种可实现的传播机制并同步 §3/§4/§5.2/§7/附录 A/§11；候选：(a) `P` 增加 W 产物输入端并原样透传（输出加 `artifact` 全文）；(b) 下游节点用两个端口分别接收 `artifact`（W→下游）与 `verdict`（P→下游）；(c) 明确 `artifact_summary` 承载全文并给出字段契约。§11 补"下游收到完整产物"断言 | open |
| C2 | **阻塞** | 同上 `:49,102,119,122,140,210` | **D8「只保留 approved 路径、不通过一律 run Failed」无机制保证，且与内核实测相反**。评审 agent 输出 `verdict:"rework"` 是 §5.1 定义的合法内容（`:119`）；agent_call 正常返回后 `P` 解析成功，gate=false 时内核按 04 §4.6.1 / 03 §3.4.2 只**结算该边并级联 Skipped**，run 仍 `Completed`（01 §1.9：无 end 执行 → `run_output=null` 但合法 Completed），不会 Crashed/Failed。实测（§4）：`p→end` 边被 gate 拦截后 `instances: end=Skipped`、`runs.status=Completed`、`run_output=null`。故 `:122`「不存在评审不通过但 run 继续的路径」与 `:49`「最终仍不通过 → 评审 agent 报错」不成立；`:140`「不会走到这里」只是假设 | 把强制失败落到唯一可执行节点：规定 `P` 在 `verdict != "approved"`（或无法解析）时**抛 Python 异常**（`PythonException → Crashed → run Failed`），并改写 `:49/:102/:122/:140/:210`；或反过来明确接受"门控不通过 = run Completed(null)"为 v1 行为并推翻 D8。§11 增加"最终 rework → run Failed"验收 | open |
| C3 | **阻塞** | 同上 `:72-73,87,92-95,105,169-173` | **`T_{i,j}` 语义三处互斥，§4 汇入关系不可由 §7 规则生成**。§3:72 定义 T 为"该 item 的下一阶段节点（编码/测试）"，§7:169-173 逐 item 生成 T 并 `T_{i,j}→D_{i+1}`；但 §4:92-95 的"下一阶段 T"列把 T 当作下一设计角色（"进入 archer / 汇入 mde / 汇入 te 与 fo / 汇入 fo/mto/sto / 汇入 mto"）。衍生问题：(1) T 若是独立逐 item 节点，其类型/职责/输出未定义；(2) 若 T 即 D_{i+1}，§7 的 `T→D_{i+1}` 不成立；(3) §7 只支持"上一阶段→下一阶段"单目标，无法表达 §4 的 `mde→{te,fo}`、`te→{fo,mto,sto}`，且 §4 漏了 legacy 的 `archer→te`、`fo→sto`；(4) 汇入关系与 legacy 依赖不一致（mde 不喂 te；te 不喂 fo；fo 喂 mto+sto）；(5) 同阶段并行 W 无法依赖同阶段产物——legacy `te→{mte,ste}` 的模块/系统用例依赖"总体测试计划"，§4 却把三者拆成 te 阶段并行兄弟工作 agent；(6) `:87` 引用的"§9 的 v0.2 项（并行设计阶段）"在 §9 不存在 | 定稿前统一 T 语义（建议二选一：T = 下游设计角色 `D_{i+1}`；或 T = 明确的 per-item 产物节点），用一张"阶段 × item → 下游端口"映射表重写 §4 的"下一阶段 T"列，并据此重写 §7 codegen；对 legacy 的跨阶段/阶段内依赖给出显式方案（拆阶段 / 引入逐 item 依赖 / 在 D_i 任务包内联前置产物）；修 `:87` 交叉引用或在 §9 增加"并行设计阶段"项。§11 增加"依赖顺序与 legacy 语义一致"断言 | open |
| C4 | **重要** | 同上 `:230-273`、`:280` | **附录 A 骨架过不了 `zgraph validate`**。图注称"节选，非可运行 fixture"，但实测 `DSL_UNCONNECTED_INPUT: node 'end' has no incoming edge`（§4），与附录 B:280「非 entry 节点入边 ≥1」自述约束直接冲突；只补一条 `planer_req_parse→end.result` 边后即 `graph valid: 6 nodes, 6 edges`。作为"codegen 前的唯一 DSL 示例"，读者照抄会 validate 失败；且骨架本身固化了 C1（`parse` 的 verdict 摘要直接进 `archer`，无产物端口） | 补 `end` 入边（如 `planer_req_parse→end.result`，或按修正后的 §3 增加 T 节点→end），并在图注写明"已用 zgraph validate 验证（命令+输出）"；若坚持节选，则显式声明"未补 end 入边，直接 validate 会报 DSL_UNCONNECTED_INPUT"，并补 C1 的产物端口 | open |
| C5 | **重要** | 同上 `:34` | **对用户确认方向的表述与 D3 矛盾**。依据行写"设计角色负责评审"，但 D3（`:44`）是"每个工作 agent 配独立评审节点，所有评审节点引用同一个 `design_reviewer` 资产"，§3:69 是独立评审 agent 节点（`R_{i,j}`），§1.1:18 也写"评审 agent 评审"；用户 2026-10-31 确认的是"评审节点 + 多节点共享同一评审 agent 资产"（替代共享评审队列） | 改为"评审节点（共享 `design_reviewer` 资产）负责评审" | open |
| S1 | 建议 | 同上 `:198` | "当前带出口自环会 `RUN_DELIVERY_TO_TERMINAL`"过度具体：实测仓库 `tests/fixtures/graphs/valid_cycle.json` 在真内核报 `RUN_DEADLOCK`（5017ms）；`RUN_DELIVERY_TO_TERMINAL` 是 ISSUE-062 描述的第二种失效模式（trigger=any 且出口提前完成时） | 写"`RUN_DEADLOCK` / `RUN_DELIVERY_TO_TERMINAL`（视 trigger/gate 配置）"，与 ISSUE-062 两种失效模式对齐 | open |
| S2 | 建议 | 同上 `:45` | D4 写"节点实例可独立重试/超时"偏宽：v1 `retry.max_attempts` **仅自环/显式重试**计数，线性节点失败即 Crashed→run Failed，不存在普通节点级重试 | 改为"节点实例可独立超时、独立 trace；重试仅自环（`retry.max_attempts`）" | open |
| S3 | 建议 | 同上 `:220-226` | §11 标题"后续验收计划（v0.2 落地时使用）"与内容不符：1–5 项（codegen N=3、per-item 评审、等待语义）都是**v1 落地的验收**；本设计是 v1、延期版却称 v0.2，版本号易混。并且验收未覆盖 C1（产物传播）与 C2（最终 rework→Failed、gate 拦截的 run 终态） | 标题改"v1 落地验收"（或补版本命名约定：v1 设计 → 下一版本标签），并补 C1/C2 验收断言；`zgraph validate` 一项注明使用 codegen 产物而非附录 A 节选 | open |
| S4 | 建议 | 同上 `:207-216` | §10 未列两个落地风险：①评审 `content` 可能被 `MaxTokens` 截断（05 §5.9：`stop_reason=MaxTokens` 属正常返回）或带 Markdown 围栏，`P` 解析失败即 Crashed→Failed；②评审 agent 多轮 tool loop 与默认 180s 实例超时（03 §3.9.1）/单次 LLM 60s 的预算关系未给出 | §10 增加"评审输出 JSON 解析脆弱性"与"评审节点超时预算"两条；§6 给出评审 agent 的 `timeout_ms`/`max_iterations` 推荐值（上限 50 范围内） | open |
| S5 | 建议 | 同上 `:157` | "所有节点通过 `agent_id` 引用资产"不准确：`python_node`/`start`/`end` 不引用 agent 资产 | 改为"`agent_call` 节点通过 `agent_id` 引用资产；资产根由 CLI `--assets-dir` / `ZGRAPH_ASSETS` 提供" | open |
| S6 | 建议 | 同上 `:48,:120` | "重做次数受 `max_iterations`（默认 10，上限 50）约束"术语不准：`max_iterations` 是**LLM 调用轮次上限**（05 §5.2.2/§5.5.2），不是重做次数；一次迭代可含多个 tool call | 改为"评审 agent 的 LLM 轮次预算受 `max_iterations` 约束（默认 10，上限 50）；重做的 tool call 计入该预算" | open |
| S7 | 建议 | 同上 `:153-158` | 未说明工作 agent 的实现载体与既有资产复用：main 已有 `python/workers/software_team/{planer,archer}.py`（M3 迁移的纯函数 worker，legacy `planer_proc`/`archer_proc` 语义），设计未提是否作为 W（`python_node`）复用 | 在 §3/§4/§6 说明各 item 的 W 载体（agent / python_node）与复用关系，避免重复实现 | open |

> 级别口径：阻塞 = 必须修复后才能给"通过"；重要 = 必须修复或登记 ISSUE；建议 = 可后置，但需修复或进台账。

## 4. 验证命令与结果

```bash
# --- 范围与干净度（评审前）---
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

# --- 附录 A 可加载性（把 232–271 行 JSON 提取到 /tmp/rev062_appendixA.json）---
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
- legacy 核对：`legacy/config/software_team_graph.json` 为 12 节点/25 边、11 条 `*_loop` 自环、5 条 `driver:"manual"`、`tick_period_s:300` —— 文档 §1.2/§8 的这些数字**正确**；但 §4 的"下一阶段 T"汇入关系与 legacy 依赖不一致（C3）。
- 本分支仅文档改动，未跑 `cargo test`/`pytest`（合并门禁会执行 build + workspace test + clippy + pytest；本次 `check.sh` 已含 `cargo fmt --check` 且无代码可测）。探针使用 main 工作区 12:56 构建的 `target/debug/zgraph`（base `5d43c58` 与分支零代码差异）。

## 5. 遗留问题

- **C1–C5 为合并前必须关闭的条件**；其中 C1/C2/C3 为阻塞。关闭后由作者发起复审（本记录不自行改判）。
- **S1–S7**：请作者在本轮修复，或登记 `doc/issues.md`（本评审按授权只改 `doc/reviews/**`，未编辑台账/设计稿）。
- 未发现越界改动、禁用文件、冻结文件修改或安全问题；分支 base = `origin/main`，worktree clean。
- 复核重点提示：修复 C1/C2/C3 时需同时更新 §3 图、§4 表、§5.2 契约与 §7 codegen，并补 §11 验收，避免"只改一段"造成新的自相矛盾。

## 6. 条件关闭记录（仅"有条件通过"填写）

| 条件 | 关闭证据 | 关闭日期 |
|---|---|---|
| C1（阻塞，产物交接链路） | 待作者修订 + 复审 | — |
| C2（阻塞，失败契约：`P` 强制失败 + 文案改写） | 待作者修订 + 复审（建议附"rework → run Failed"实测） | — |
| C3（阻塞，T 语义统一 + §4/§7 codegen + `:87` 交叉引用） | 待作者修订 + 复审 | — |
| C4（重要，附录 A 补 end 入边或显式声明 validate 失败） | 待作者修订 + 复审（建议附 `zgraph validate` 输出） | — |
| C5（重要，`:34` 方向表述） | 待作者修订 + 复审 | — |

> 复审通过后，本记录结论需更新为无条件通过（且不含"有条件"字样），`merge.sh` 才会放行；S1–S7 的关闭/台账登记情况一并核对。
