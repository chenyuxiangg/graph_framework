# REV-025 · process/docs · M5 开工：DR-002 落地决策 + 进度/README/索引启动更新

> **模块声明**：本分支改 4 个文件：`doc/decisions/DR-002-m5-cli-replay-recording.md`、`doc/progress.md`、`doc/index.md`（`docs` 模块）与根 `README.md`（**`process` 模块**，作者 `lead` 亦为 process 负责人）；按 AGENTS §2 属**跨模块**改动。提交正文未显式声明跨模块（见 §3 #10），本记录由**非作者**评审人 `reviewer-a` 作独立第二评审并在本段声明（`fd936e0` 已在 DR-002 头部补「模块声明（跨模块）」段，见 #10）。评审**未修改任何产品代码/文档/索引**，只新增本记录；未 merge / 未 push / 未改 main。
>
> **评审对象**：首轮 `54a69f3`（作者提交）；C1/C2 修订 `fd936e0`（在 `bd683bc` 评审记录提交之上）。已做**增量复审**：关闭证据与结论更新见 §1/§4.6/§6，首轮原文保留未删除。

- **分支**：`docs/process-m5-kickoff`
- **作者**：`lead`
- **评审人**：`reviewer-a`（**非作者**）
- **变更范围**：最终 `1cc9123..fd936e0`（3 commit：作者 `54a69f3` 内容 + 评审人 `bd683bc` 本记录 + 作者 `fd936e0` C1/C2 修订；`git diff --shortstat` = **5 files changed, 334 insertions(+), 18 deletions(-)**，其中 4 个产品/文档文件 + 1 个评审记录；base = merge-base = `origin/main` = `1cc9123...`，**基于最新 main**；评审时 worktree clean）
- **评审时间**：2026-10-30
- **关联**：AGENTS §3/§4/§5/§6/§7；`architecture-v2.md` §5.1–§5.3/§6.1/§8.2；spec 06 §6.10.1–§6.10.6；spec 07 §前言/AC-01/AC-12；spec 05 §5.2.6/§5.3.4/§5.13.3；spec 04 §4.3.6；spec 08 N-02；`doc/issues.md` ISSUE-033/036/048/059/061/066/073–083；REV-015/REV-020/REV-022/REV-024

## 1. 结论

**结论：通过**（增量复审：首轮为「有条件通过」，C1/C2 已由 `fd936e0` 关闭，关闭证据见 §6；以下保留首轮条件原文，不删除原记录）

内容主体（M5 范围/波次/AC-12/P2 修复口径/legacy 决策）与 architecture §5/§6.1、spec 07 AC-12、`doc/issues.md` 基本对齐，DR-002 声明的关键可执行性（trace reader / 内存 sink 重跑 / `timeout_hint`）经代码核对**成立**；进度、README、索引与仓库事实一致，`check.sh` PASS。未发现阻塞项。

首轮发现 2 项重要问题（C1/C2）；Lead 已于 `fd936e0` 修订关闭（逐项证据见 §6），以下保留首轮条件原文：

- **C1（重要）**：DR-002 与 spec 正文存在未声明的口径偏差，且 M5 回写清单不足以覆盖。
  1. `DR-002:28` 把 `--python` 的默认解释器定为 env `ZGRAPH_PYTHON`，而 spec 04 §4.3.6（`04-节点接口与扩展点.md:424`）、06 §6.10.3（`06-存储与可观测.md:751`）、08 N-02（`08-审计关闭矩阵.md:393`）均为 `PYO3_PYTHON`；DR-002:7 自称「与 spec 正文冲突时以 spec 正文为准」、且 §7 回写范围只写「05/06/07/00」，**漏 04/08**，M5 回写后 04/08 仍会残留旧值。
  2. `DR-002:23` 要求 fake 脚本落 `tests/fixtures/graphs/**`、AC-01/AC-08 用 `--fake-llm=<script>`，而 spec 07 §前言/AC-01（`07-验收用例.md:14-19,51-53`）仍写 `examples/customer_ticket/graph.json`、`examples/fake_llm/*.json` 与裸 `--fake-llm`；AC-01 的确定性前置按 spec 原命令无法复现。
  3. `DR-002:26` 的「70 panic 由进程默认」与 spec 06 §6.10.5 权威表（`06:847`：70 = 内部错误（panic 被捕获））矛盾；Rust 默认 panic 退出码是 101，不是 70。
  4. `DR-002:57` 新增 CLI 命名空间错误码 `REPLAY_OUTPUT_MISMATCH`，但 00 §0.4 CLI 命名空间段与 architecture §8.2 目前只登记了 `REPLAY_RECORD_MISSING`。
  **关闭条件**：在 DR-002 增补「spec 偏差与 M5 回写清单」（逐条写明分册/章节与回写目标），或由 Lead 在 `doc/issues.md` 登记等价 ISSUE 并在 DR-002 引用；同时把 §7 的回写范围补到覆盖 04/08（08 若不回写需说明理由）。
- **C2（重要）**：`DR-002:55-57` 的 L2 重放边界不完整：
  1. 记录键是 `(run_id, instance_id, attempt)`（`DR-002:36`），重放取记录却用 `(node_id, attempt)`（`DR-002:56`）；重跑时 `instance_id` 由内核重新生成（UUIDv7，不可复用），文档未写清映射依据与唯一性假设。
  2. 未定义「记录不完整」的判定算法（只写了「记录不完整 → REPLAY_RECORD_MISSING」）。
  3. 非 Completed run（尤其无 `output_json` 的 Failed，如 `RUN_DEADLOCK`）若重跑同样无输出，按「重跑输出 == 原始 `runs.output_json` → exit 0」会把「双 None」误判为重放成功。
  **关闭条件**：§3.3 补明 `--mode=outputs` 的适用范围/终态比较口径（或限定仅 Completed run 支持）、记录完整性预检规则、以及 `(node_id, attempt)` 映射所依赖的内核语义。

其余建议/流程项见 §3：首轮 #5–#10/#12 已在 `fd936e0` 落实（fixed），#11 为合并后索引补齐；增量复审新增 #13–#15 跟进项（不阻塞 REV-025）。

## 2. 四维评审

| 维度 | 结论 | 依据（文件/行/命令/代码核对） |
|---|---|---|
| 功能正确性 | ✅ | ① M5 范围 6 项与 architecture §6.1 M5 DoD 逐项对应（CLI 全契约、replay L1/L2、L2 记录接线/ISSUE-082、AC-12、P2 收敛、legacy 退役）；AC-12 口径与 spec 07 AC-12（`07:189-199`：10 并发 run、同库/独立 DB、instances/gate_results/events 隔离、global_vars/user_id 不串、`MAX_WORKERS`/`MAX_WORKERS_PER_RUN`）逐条一致，`WorkerExecutor::with_limits` 存在（`crates/zgraph-worker/src/executor.rs:67`，全局默认 CPU×2:45、每 run 8:36）。② `--record` 默认 hash / 透传 `RunSpec.options.record_mode` / 落 `runs.record_mode` 已具备（`run.rs:78-108,514`）。③ DR-002 的 P2 修复口径与台账、spec 已知限制（05 §5.13.3 L1/L4）对齐。**增量复审**：C1/C2 已由 `fd936e0` 关闭（§9 偏差/回写清单 9 条、§3.3 L2 边界重写），扣分项消除；§5 AC-12 实测口径与 REV-029 代码核对一致。 |
| 代码结构合理性 | ✅ | 本分支只改文档：DR-002 新建于 `doc/decisions/`（与 DR-001 同级），`doc/index.md` 决策行、README 状态段、progress 四块同步更新，职责边界正确；不触碰代码/测试/CI/`CLAUDE.md`（`git diff --name-status` 4 文件）；progress 保持唯一进度文件、非空 17 行（≤30）；回写/波次分工把跨模块改动（ISSUE-076 kernel+harness、ISSUE-066 kernel→worker→cli）显式声明并由 Lead 批准，符合 AGENTS §2。 |
| 逻辑性 | ✅ | ① 波次/评审编号/分支名与当前 5 个 worktree 一一对应（`git branch`/`git worktree list` 实测：REV-025–029 五分支均在），progress 各行 `wip` 与兄弟 worktree 实际改动一致（kernel 改 executor/graph/run、harness 增 `record.rs`、cli 增 `run_cmd.rs`、e2e 有 AC-12 commit）。② `doc/reviews/README.md` 与 `doc/index.md` 的索引在合并后将落后于 REV-025（本评审按约束不能改，见 #11）。③ wave2 harness 行「续 `feat/harness-m5-record` / 续 REV-027」与 merge.sh 合并后删除 worktree+分支的强制流程冲突（#6）。**增量复审**：C2 假通过分支已由「仅 Completed + 终态/error_code 比较」关闭；exit 70 改为 catch_unwind→70；wave2「续分支」冲突已改为新分支/新 REV。残留 Nit：#13 头部 REV 范围、#14 `--python` 实现跟进。 |
| 可读性 | ✅ | DR-002 分节清晰（范围→CLI→L1/L2→P2→AC-12→波次→排除→legacy），每条决策给「做什么/验收」，关键值（默认 db/user/assets、退出码、hash 算法、8 KiB/512/256）可扫读；README 状态段与 progress 表信息密度合适；遗留项均指向 ISSUE 编号与 spec 章节，无 TODO 顶替台账。Nit：头部「关联」ISSUE 清单与正文引用不完全一致（#9）。 |

## 3. 发现清单

| # | 级别 | 位置 | 问题 | 建议 | 状态 |
|---|---|---|---|---|---|
| 1 | 重要（C1） | `doc/decisions/DR-002-m5-cli-replay-recording.md:7,28`（对照 `doc/spec-v1.2/04-节点接口与扩展点.md:424`、`06-存储与可观测.md:751`、`08-审计关闭矩阵.md:393`） | `--python` 默认解释器写 `ZGRAPH_PYTHON`，与 spec 三处 `PYO3_PYTHON` 冲突；而 DR-002:7 声明 spec 正文优先、§7 回写范围只到 05/06/07/00（漏 04/08）→ 决策自相矛盾且回写后 04/08 仍残留 | 增补「spec 偏差与 M5 回写清单」：04 §4.3.6 / 06 §6.10.3 / 08 N-02 的 PYO3_PYTHON→ZGRAPH_PYTHON（或定义两变量优先级），并把回写范围补到覆盖 04/08（08 为历史矩阵则可注明「仅注记」） | fixed（`fd936e0`：§2 `--python` 优先级 PYO3>ZGRAPH + §9 #1 回写清单；回写范围已含 04/08） |
| 2 | 重要（C1） | `DR-002:26`（对照 spec 06 §6.10.5：`06-存储与可观测.md:847`） | 「70 panic 由进程默认」与权威退出码表「70 = 内部错误（panic 被捕获）」矛盾；Rust 默认 panic 退出码为 101 | 改为可执行口径：「CLI 顶层 `catch_unwind`/panic hook → 70（validate/run/trace 适用）」；若 M5 暂不实现则登记 ISSUE 并写入 spec 回写注记 | fixed（`fd936e0`：§2 退出码 70 = 顶层 catch_unwind→70，明确不用 Rust 默认 101；§9 #3） |
| 3 | 重要（C1） | `DR-002:23`（对照 spec 07 §前言 `07-验收用例.md:14-19`、AC-01 `:51-53`；architecture §2.2:77） | 决策要求 fake 脚本落 `tests/fixtures/graphs/**`、AC-01/AC-08 用 `--fake-llm=<script>`，spec 07 仍写 `examples/**` 与裸 `--fake-llm`；AC-01 的确定性前置按 spec 原命令无法复现 | 在 C1 偏差清单显式列「07 §前言 fixture 路径 + AC-01/AC-08 命令（`--fake-llm=<script>`）回写」，并注明 architecture §2.2 的 `tests/fixtures/graphs/` 落点为权威 | fixed（`fd936e0`：§2 脚本落点/AC 命令 + §9 #2 07 回写项） |
| 4 | 重要（C2） | `DR-002:55-57`（对照 `crates/zgraph-trace/src/schema.rs:224-236`、`reader.rs:465`；`crates/zgraph-kernel/src/run.rs:957,1011,1043,1071,1213,1261`） | L2 重放边界不完整：记录键 `(instance_id, attempt)` vs 取记录键 `(node_id, attempt)` 未写映射依据（重跑 instance_id 不可复用）；「记录不完整」无判定算法；非 Completed run 无 output 时「双 None 相等 → exit 0」会误判成功 | §3.3 补：① 映射依赖「非自环节点每 run 至多执行一次、自环 attempt+1、投递终态即 fail run」（`run.rs:1011-1069`）并写明索引构建；② 完整性预检 = 已执行非内置实例集合 ⊆ `recorded_outputs` 集合，缺失即 REPLAY_RECORD_MISSING；③ 比较同时校验终态/`error_code`，或限定 `--mode=outputs` 仅支持 Completed | fixed（`fd936e0`：§3.3 重写——仅 Completed、完整性/哈希预检、映射唯一性、终态/error_code 比较、replay 退出码表） |
| 5 | 建议 | `DR-002:64`（对照 `crates/zgraph-harness/src/executor.rs:27-38,73`、`config.rs:214,269`；`crates/zgraph-kernel/src/executor.rs:27-36`） | ISSUE-076 的「Harness 对 agent_call 返回 `Some(AgentConfig.node_timeout_ms)`」在**同步** `timeout_hint` 上不能直接落地：`load_agent` 需要先前异步 `discover` 得到的 `ToolRegistry`，派发前首轮无缓存；「`run_agent` 使用同一生效值」也缺少传递通道（`NodeExecution`/`NodeContext` 只有 `node.timeout_ms` 与 deadline） | 明确实现路径：新增「仅解析 AgentConfig（不查工具表/密钥）」的同步轻量函数供 `timeout_hint`，或构造期预载 `node_id→timeout` 映射；把生效超时经 `NodeContext`/`NodeExecution` 暴露给 `run_agent`（或 `run_agent` 复用同一解析函数） | fixed（`fd936e0`：§4 补同步轻量解析路径与 elapsed 同源口径） |
| 6 | 建议 | `DR-002:86`（wave2 harness 行） | 「续 `feat/harness-m5-record` / 续 REV-027」与 AGENTS §4/`merge.sh`「合并后必须删除 worktree 与分支」冲突：wave1 合并后该分支已不存在；merge.sh 以「分支→记录」定位，续用同分支也不可行 | 改为新分支（如 `fix/harness-m5-timeout`）+ 新 REV 编号，并在记录中引用 REV-027 为前置 | fixed（`fd936e0`：§6 改为新分支 `fix/harness-m5-timeout`/REV-033） |
| 7 | 建议 | `DR-002:63`（对照 `tests/e2e/parallel.rs:380-410`，e2e 模块） | ISSUE-061 要求「补 kernel 单测 + e2e 回归」，现有 e2e 反例在 `tests/e2e/parallel.rs`（e2e 模块）且只固化真实 attempt、明确不把幻影实例当验收期望；§6 wave 表未指派该 e2e 回归，由 kernel-dev 直接改属跨模块 | §6 明确由 e2e-dev 在 `test/e2e-m5-ac12`（或 wave2）补「无幻影 attempt」断言；或声明 kernel-dev 跨模块 + 第二评审 | fixed（`fd936e0`：§4 明确 e2e-dev wave2 补回归，§6 表已含） |
| 8 | 建议 | `DR-002:35`（对照 `crates/zgraph-cli/src/bridge.rs:166-169,254-274`） | `recorded_outputs` 写者定为「CLI 组合执行器外层包装」，但现有 `bridge::start_traced_run` 内部创建 TraceHandle 且不对外暴露，外层包装拿不到写端 | 在 §3.1 注明集成点：bridge 暴露 TraceHandle/新增可注入 sink 的启动入口，或 CLI 直接组装 `TraceWriter`+`TraceBridge`+`kernel::start_run` | fixed（`fd936e0`：§3.1 集成点 `start_traced_run_with_writer`，已核对 cli 分支 `6b4085c` 实现） |
| 9 | Nit | `DR-002:4` | 头部「关联」列 ISSUE-033/036/048/061/066/074/076/082，正文实际还引用 059/073/075/077/079/080/081，清单不完整 | 补全或改为「详见 §4/§7」 | fixed（`fd936e0`：头部关联清单补全） |
| 10 | 建议（流程） | `54a69f3` 提交正文 | 分支跨模块（README 属 process、`doc/**` 属 docs），提交正文未像 REV-024 那样显式声明跨模块说明；AGENTS §2 要求 Lead 在评审记录声明并指定第二评审 | 本记录已由 reviewer-a（非作者）作第二评审并声明；建议 Lead 在合并说明/后续 docs 提交补一行跨模块声明 | fixed（`fd936e0`：头部新增模块声明段；提交正文仍可补） |
| 11 | 建议（流程） | `doc/reviews/README.md`、`doc/index.md:16` | 本评审按任务约束只新增 REV-025 记录文件；合并后评审索引与 `doc/index.md` 的 reviews 行（仍为 REV-000–024）不含 REV-025 | 合并后由 Lead 随 progress 提交补：`doc/reviews/README.md` 增 REV-025 行、`doc/index.md` reviews 行改 REV-000–025 | open（合并后由 Lead 补索引；本评审按约束不改其他文件） |
| 12 | Nit | `DR-002:47-59` | replay 退出码只在 §3.3 隐含复用 trace（0/1/4）与 REPLAY_RECORD_MISSING（3），spec 06 §6.10.5 表无 replay 列；新码 `REPLAY_OUTPUT_MISMATCH` 未在 00 §0.4 CLI 命名空间/architecture §8.2 登记（现只登记 REPLAY_RECORD_MISSING） | §3.3 附 replay 退出码小表；M5 回写时在 00 §0.4/06 §6.10.5/arch §8.2 登记 replay 行与新错误码 | fixed（`fd936e0`：§3.3 replay 退出码小表 + §9 #4 登记 REPLAY_*） |
| 13 | Nit | `doc/decisions/DR-002-m5-cli-replay-recording.md:5` | 头部「实现分支评审 REV-026–REV-033」与 `fd936e0` 修订后 §6 表（至 REV-037）不一致 | 改为「REV-026–REV-037」或「REV-026 起」；合并前顺手改或 docs writeback 修正 | open |
| 14 | 建议（实现跟进） | `DR-002:24` 新解析链 vs `crates/zgraph-cli/src/run_cmd.rs:87-97`（cli 分支 `6b4085c` 仅读 `ZGRAPH_PYTHON`）、`crates/zgraph-harness/src/tools.rs:1578`、`tests/e2e/common/mod.rs:78` | DR-002 新优先级 `--python > PYO3_PYTHON > ZGRAPH_PYTHON > python3` 尚未被在飞实现采用；`scripts/env.sh` 会导出 `PYO3_PYTHON`，本机两变量同时设置时 CLI 将优先 PYO3（可能改变 e2e 期望） | REV-028/REV-033 落实该链并补「PYO3+ZGRAPH 同时设置」用例；e2e/common 显式处理 PYO3_PYTHON 避免本地 env.sh 干扰 | open（REV-028/033 跟进） |
| 15 | 备注（流程） | `DR-002:79,102`（ISSUE-066 跨模块行） | ISSUE-066 单分支跨 kernel+worker+cli 由 worker-dev 一人实施；AGENTS §2 要求 Lead 在 REV-031 记录显式声明跨模块批准 + 第二评审（§4 已写要求） | REV-031 评审记录落实该声明；若无法落实则拆模块分支 | open（REV-031 跟进） |

> 无阻塞项；#1–#10/#12 已随 `fd936e0` 关闭（fixed），#11 为合并后索引补齐，#13–#15 为增量复审新增跟进（open，不阻塞 REV-025）。

## 4. 验证证据

### 4.1 范围、分支与提交元数据（worktree `.worktrees/process-lead`）

```bash
cd /Users/zyzs/lczyzs/code/zgraph/.worktrees/process-lead
git status --short                        # 空（评审时 worktree clean）
git branch --show-current                 # docs/process-m5-kickoff
git merge-base origin/main HEAD           # 1cc91235...（= origin/main，基于最新 main）
git rev-parse HEAD                        # 54a69f3...
git diff origin/main...HEAD --stat        # 4 files changed, 120 insertions(+), 18 deletions(-)
git diff origin/main...HEAD --name-status
# M README.md
# A doc/decisions/DR-002-m5-cli-replay-recording.md
# M doc/index.md
# M doc/progress.md
git log -1 --format='%H|%an <%ae>|%cn <%ce>|%s' HEAD
# 54a69f3... | chenyuxiangg <1007423165@qq.com> | chenyuxiangg <1007423165@qq.com> | docs(process): M5 开工——DR-002 落地决策 + 进度/README 启动更新（REV-025）
# 正文含 Review: REV-025 / Agent: lead
```

- 仅 4 个文档；**无 `CLAUDE.md`、无代码/测试/CI/schema/`doc/spec-v1.2/` 正文改动**、无禁用文件（`git diff --name-only | grep -E 'CLAUDE\.md|crates/|tests/|scripts/|schemas/|\.venv|__pycache__|\.db'` 为空）。

### 4.2 文档 ↔ spec/architecture 对照（独立逐条核对）

| DR-002 条目 | spec/architecture 依据 | 核对结论 |
|---|---|---|
| §1 范围 6 项 | architecture §6.1 M5 DoD（`architecture-v2.md:204`） | ✅ 逐项对应（CLI、replay L1/L2、AC-12、legacy、文档） |
| §2 入口输入简写/退出码 3/不产生 run | spec 06 §6.10.3 形状表（`06:761-767`）、§6.10.5 行 3（`06:845`） | ✅ 一致 |
| §2 `--fake-llm` 裸 flag/脚本/互斥 | spec 05 §5.3.4（`05:404-420`）、06 §6.10.3（`06:756-757`） | ⚠️ 语义与 spec 一致；但脚本落点/AC 命令与 spec 07 冲突（#3） |
| §2 `--record` 默认 hash / 透传 | architecture §5.3（`arch:180`）；`RunOptions.record_mode`（`run.rs:78-108,514`） | ✅ 一致且已具备透传/落库路径 |
| §2 退出码 0/1/2/3/4/130 | spec 06 §6.10.5（`06:840-850`） | ✅ 一致 |
| §2 退出码 70 | spec 06 §6.10.5（`06:847`：panic 被捕获 → 70） | ✅（首轮 ❌；`fd936e0` 改为 catch_unwind→70 + §9 #3） |
| §2 `--python` env | spec 04 §4.3.6（`04:424`）、06 §6.10.3（`06:751`）、08 N-02（`08:393`）均 `PYO3_PYTHON` | ✅（首轮 ❌；`fd936e0` 改为 PYO3>ZGRAPH 优先级 + §9 #1） |
| §2 默认值 db/user/assets/tools | spec 06 §6.10.1/§6.10.3（`06:717,753-755`）、§6.9（`.zgraph/trace.db`） | ✅ 一致（`trace_cmd.rs:19` 默认值同） |
| §2 输出流/`--json`/`--stream` | spec 06 §6.10.1（`06:714-715`）、§6.10.3（`06:772-773`） | ✅ 一致 |
| §2 SIGINT→130 | spec 06 §6.10.3（`06:774`） | ✅ 一致（可执行性：`RunHandle::cancel`，`run.rs:193-196`） |
| §3.1 recorded_outputs 写者/键/hash/时间 | architecture §5.2（`arch:171-173`）、trace DDL（`schema.rs:224-236`）、`canonical_json`（`dsl/src/hash.rs:30`）、`now_rfc3339`（`kernel/src/types.rs:462`） | ✅ 表结构/Hash 算法/时间精度与实现一致 |
| §3.2 llm_calls seam/粒度/脱敏/status | architecture §5.2（`arch:173`）、`LlmCallRecord`（`trace/src/model.rs:374-389`）、`HarnessSettings` 字面量（`harness/src/executor.rs:27-38`） | ✅ 可落地（executor 增字段即可，不破坏 settings 字面量）；粒度/attempts 与 05 §5 一致 |
| §3.3 events 复用 trace | `trace_cmd::run` 公开（`cli/src/trace_cmd.rs:187`） | ✅ 可复用 |
| §3.3 outputs 重跑（reader 输入/内存 sink） | `TraceReader::run/recorded_outputs`（`reader.rs:364,465`）、`RunRow.entry_inputs/seed/output_json/record_mode`（`reader.rs:102-128`）、`MemoryTraceSink`（`kernel/src/trace.rs:317`）、`run_to_completion`（`run.rs:261`） | ✅ 能力齐备（`fd936e0` 后措辞/边界已补，见 §4.6） |
| §4 ISSUE-061 修复口径 | `apply_publish` 当前先 `resolve_target`（`run.rs:957,1011`）；`cascade_skip`/`ensure_instance` 支持「不建 attempt 实例仍级联 Skipped」（`run.rs:1213,1261`）；e2e 反例（`tests/e2e/parallel.rs:380-410`） | ✅ 方案与 ISSUE-061 建议一致、可实现；e2e 归属已在 `fd936e0` 明确（#7 fixed） |
| §4 ISSUE-076 timeout_hint | `CompiledNode.timeout_ms`（`graph.rs:37`）、`NodeExecutor`（`kernel/src/executor.rs:45-51`，仅一个生命周期泛型方法，可加默认方法且保持对象安全）、当前派发 `force_timeout_ms.unwrap_or(node.timeout_ms)`（`run.rs:674`）、spec 05 §5.2.6 优先级（`05:209`） | ✅ 方案可实现且与 spec 优先级一致；同步轻量路径已在 `fd936e0` 补明（#5 fixed） |
| §4 ISSUE-074 截断 | spec 05 §5.6.5（`05:973-975`）、§5.13.3 L4（`05:1410`） | ✅ 与 spec 及已知限制一致 |
| §4 ISSUE-066 只增不删 | `NodeError::NodeWorker{kind}`（`kernel/src/types.rs:437-441`）、architecture §8.2（`arch:272`） | ✅ 可增可选字段，不破坏现有序列化面 |
| §4 ISSUE-048 owner_pid | `runs.owner_pid` 列存在（`schema.rs:101`），`RunRow` 未读（`reader.rs:88-128`）、`zgraph trace` 无 pid 提示（`trace_cmd.rs`） | ✅ 确实只需补读取+提示 |
| §5 AC-12 口径 | spec 07 AC-12（`07:189-199`）、`WorkerExecutor::with_limits`（`worker/src/executor.rs:67`） | ✅ 逐条一致 |
| §6 波次/分支/REV | 实际 5 个 worktree/分支（`git worktree list`/`git branch`） | ✅ REV-025–029 五分支存在；`fd936e0` 已改为新分支/新 REV（#6 fixed） |
| §7 排除项 | ISSUE-033/036/073/059 现状（`doc/issues.md`） | ✅ 理由成立 |
| §8 legacy 决策 | `legacy/README.md` 已声明冻结只读；architecture §6.1「legacy 退役决策/归档」 | ✅ 与现状/DoD 一致 |

### 4.3 DR-002 可执行性代码核对（不要求实现，只验可落地）

| 声明的依赖 | 实测证据 | 结论 |
|---|---|---|
| `TraceReader::recorded_outputs` | `crates/zgraph-trace/src/reader.rs:465`（`SELECT ... FROM recorded_outputs WHERE run_id`） | ✅ 存在 |
| `TraceReader::run` 的 `record_mode/entry_inputs/output_json/seed` | `reader.rs:102,104,126,128`；`run()` SQL `reader.rs:364-376` | ✅ 全部存在 |
| DSL 原文可读 | `runs` 表**无 `dsl_json` 列**（`schema.rs:92-122`），DSL 在 `graphs.dsl_json`（`schema.rs:76-82`），经 `runs.graph_hash` 用 `TraceReader::graph()`（`reader.rs:381`）读取 | ⚠️ DR-002:56「读原始 `runs.dsl_json`」措辞需改（#4 附带） |
| 内存 trace sink 重跑 | `MemoryTraceSink`（`kernel/src/trace.rs:317`）、`run_to_completion(run_id, spec, executor, sink)`（`kernel/src/run.rs:261`） | ✅ 支持 |
| 记录条带 identity | `NodeContext::run_id/instance_id/node_id/attempt`（`kernel/src/context.rs:71-88`）→ CLI 包装器可构造 `RecordedOutput` | ✅ 可写 |
| `(node_id, attempt)` 唯一性 | 非自环目标投递到终态即 `CODE_RUN_DELIVERY_TO_TERMINAL` fail（`run.rs:1011-1041`）；自环每次 `attempt+1`（`run.rs:1043-1069`） | ✅ 单 run 内唯一（需 DR 写清） |
| `NodeExecutor` 加默认方法 | `trait NodeExecutor` 仅 `execute<'a>`（`executor.rs:45-51`），加 `fn timeout_hint(&self, &CompiledNode) -> Option<u64> { None }` 保持对象安全 | ✅ 兼容 |
| `CompiledNode` 增 `declared_timeout_ms` | `CompiledNode` 全字段 pub、仅 crate 内构造（`graph.rs:24-48,219`），无外部字面量构造 | ✅ 增量安全 |
| `RunOptions.record_mode` 透传 | `run.rs:78-108`，`run.rs:514` 写入 `RunStartedInfo.record_mode` → bridge（`cli/src/bridge.rs:300`）→ `runs.record_mode` | ✅ 已具备 |
| 内存 sink + 只读重放 | `TraceReader::open_readonly`（`trace_cmd.rs:187` 附近使用）；`MemoryTraceSink` 不落库 | ✅ 满足「不写新 run、不改原 trace」 |
| harness 观察者 seam | `HarnessExecutor` 仅 `settings`+`tools_cache` 两字段（`executor.rs:52-56`），加 recorder 字段/`with_llm_recorder` 不破坏 `HarnessSettings` 字面量（`executor.rs:27-38`） | ✅ 可加 |
| timeout_hint 同步取 AgentConfig | `load_agent` 需 `&ToolRegistry`（`config.rs:214-215`），registry 由异步 `discover` 填充（`executor.rs:73-88`）；首轮派发前无缓存 | ⚠️ 需补充同步轻量路径（#5） |

### 4.4 进度/README/索引核对

| 项 | 结果 |
|---|---|
| `doc/progress.md` | 总 22 行 / **非空 17 行（≤30）**；四块齐全（当前阶段 / 模块表 / 下一步 / 阻塞）；6 个模块行 `wip/done` 与兄弟 worktree 实际状态一致（kernel/harness/cli 有未提交改动、e2e 有 AC-12 commit、main 已合并 dsl/trace/worker/py-worker）；「M4 基线 423 passed/2 ignored、pytest 84 passed」与 `doc/test-baseline.txt` 一致 |
| `README.md` | 状态段改为「M5 收尾进行中」并新增「M5 进行中（未合并）」段；M0–M4 完成表述、M5 范围（run/replay/`--record`/L2/AC-12/P2/legacy）与代码事实一致（`zgraph-cli` 仍只有 validate/trace，run 未合并）；未夸大 M5 |
| `doc/index.md` | 新增 DR-002 决策行，格式/链接正确；⚠️ reviews 行仍 REV-000–024（#11） |
| 禁用/冻结文件 | 未改 `CLAUDE.md`；无 `.db/.venv/__pycache__/.worktrees` 等；提交作者/committer 均为 `chenyuxiangg <1007423165@qq.com>` |

### 4.5 校验命令与结果

```bash
cd /Users/zyzs/lczyzs/code/zgraph/.worktrees/process-lead
export CARGO_HOME=/Users/zyzs/lczyzs/code/zgraph/.cargo-home \
       CARGO_NET_OFFLINE=true \
       CARGO_TARGET_DIR=/Users/zyzs/lczyzs/code/zgraph/target
bash scripts/dev/check.sh
# OK user.name / user.email / remote(SSH) / CLAUDE.md 未改 / 暂存区无禁用文件
# OK 分支命名 docs/process-m5-kickoff / Python 语法 / progress+issues 存在
# OK doc/progress.md 非空行 17（目标 ≤30）
# OK cargo fmt --check / git diff --check
# --- CHECK PASSED
```

### 4.6 增量复审（`fd936e0`：C1/C2 关闭，2026-10-30）

```bash
cd /Users/zyzs/lczyzs/code/zgraph/.worktrees/process-lead
git log --oneline -3        # fd936e0 关闭 C1/C2 / bd683bc 首轮评审记录 / 54a69f3 内容
git diff --stat bd683bc..fd936e0   # 1 file changed, 59 insertions(+), 24 deletions(-)（仅 DR-002）
git status --short          # 空
```

| C1/C2 关闭核对 | 结果 |
|---|---|
| C1-1 `--python` env 冲突 | `DR-002:24` 改为 `--python` > `PYO3_PYTHON` > `ZGRAPH_PYTHON` > `python3`（spec 权威 + 测试覆盖），并列入 §9 #1（04/06/08）；§1 回写范围改为 **00/04/05/06/07/08** ✅ |
| C1-2 fake 脚本/07 命令 | §2 脚本落 `tests/fixtures/graphs/**`（architecture §2.2 权威）；§9 #2 列 07 §前言/§7.1/AC-01/AC-08 回写（`07:10` 确认 §7.1 存在）✅ |
| C1-3 exit 70 | §2 改为「CLI 顶层 `catch_unwind` → 70，**不得**依赖 Rust 默认 101」；§9 #3 注记 ✅ |
| C1-4 `REPLAY_*` 登记 | §9 #4 登记 `REPLAY_OUTPUT_MISMATCH`/`REPLAY_RUN_NOT_COMPLETED`/`REPLAY_RECORD_CORRUPT`（CLI 命名空间、不写 `runs.error_code`）；§3.3 附 replay 退出码小表 ✅ |
| C2 L2 边界 | §3.3 重写：仅 Completed；`record_mode`/`status`/完整性预检/哈希复核；`(node_id, attempt)` 唯一性依据（`run.rs:1011-1069`）；终态+`error_code`+输出比较；`graphs.dsl_json` 经 `runs.graph_hash`；不写库 ✅ |
| 附带 #5–#10/#12 | §4 ISSUE-076 同步轻量解析路径、§4 e2e 回归归 e2e-dev、§6 新分支/新 REV 表、§3.1 `start_traced_run_with_writer` 集成点、头部关联清单/模块声明、replay 退出码表——逐项核对 ✅ |
| 可执行性复核 | `trace/src/reader.rs:405` `instances()`（预检所需 `node_type/status` 可读）；`cli/src/bridge.rs:289` `start_traced_run_with_writer` 已实现（cli 分支 `6b4085c`，复用既有 `TraceHandle::send`）；§3.3 预检/哈希复核所需字段齐备 ✅ |
| §5 AC-12 实测口径 | 与 e2e `tests/e2e/ac12.rs:69-94`（10=8+2、`GLOBAL_MAX_WORKERS=4`、`MAX_WORKERS_PER_RUN=2`、同库）及 `:789-816` 峰值断言一致；独立 DB 由 `concurrency.rs` 覆盖；ISSUE-079 注记如实 ✅ |

**增量复审新增跟进（不阻塞 REV-025，见 §3 #13–#15）**：头部 REV 范围 Nit；`--python` 新优先级需在 REV-028/REV-033 落地并由 REV-028 补「PYO3+ZGRAPH 同时设置」用例；REV-031 跨模块声明落实。

### 4.7 merge.sh 门禁（最终结论前复跑）

- 首轮（有条件通过）时 `--dry-run` 在评审门禁**按设计阻断**：`MERGE BLOCKED: 评审结论不是“通过”`——证明记录可被脚本识别、分支/工作区/base 前置满足。
- 增量复审后（本记录 §1=通过）按 Lead 指定的**隔离 target** 复跑（避免与开发构建互踩）：
  `CARGO_HOME=<repo>/.cargo-home CARGO_NET_OFFLINE=true CARGO_TARGET_DIR=/tmp/zgraph-merge-target CARGO_INCREMENTAL=0 PYTHON_BIN=<repo>/.venv/bin/python bash scripts/dev/merge.sh docs/process-m5-kickoff --dry-run`
- **结果：见 §6 关闭记录末尾的 dry-run 复跑输出**。

## 5. 遗留问题

- **合并后必须做**：`doc/reviews/README.md` 增 REV-025 行、`doc/index.md` reviews 行改 REV-000–025（本评审按任务约束只新增评审记录，不改其他文件）。
- **M5 跟进（不阻塞 REV-025）**：
  - #13：`DR-002:5` 头部「实现分支评审 REV-026–REV-033」应随 §6 新表改为 REV-026–REV-037（或「REV-026 起」）。
  - #14：`--python` 新解析链需由 REV-028（cli）与 REV-033（harness）落地：当前 cli `run_cmd.rs:87-97`、harness `tools.rs:1578`、e2e `common/mod.rs:78` 仅读 `ZGRAPH_PYTHON`；且 `scripts/env.sh` 导出 `PYO3_PYTHON` 时需在 REV-028 覆盖「两变量同时设置」的优先级用例。
  - #15：REV-031（ISSUE-066 kernel+worker+cli）评审记录须显式声明 Lead 跨模块批准 + 第二评审（DR-002 §4 已写要求）。
- **M5 docs writeback 跟踪**：§9 的 9 条偏差清单作为 `docs/docs-m5-writeback`（REV-035）输入；08 只加 M5 注记。
- 本评审未修改任何产品代码/文档；REV-025 记录为本分支评审人唯一新增文件（由评审人提交）。

## 6. 条件关闭记录

| 条件 | 关闭证据 | 关闭日期 |
|---|---|---|
| C1（spec 偏差与回写清单） | `fd936e0`：① §2 `--python` 改为 `--python > PYO3_PYTHON > ZGRAPH_PYTHON > python3`、exit 70 改为 `catch_unwind→70`、`--db :memory:` 按 06 §6.9.1 接受并警告；② 新增 §9「spec 偏差与 M5 回写清单」9 条（覆盖 00/04/05/06/07/08 + architecture §5.3/§8.2，含 `REPLAY_*` 三新码、07 fixture/命令回写、exit 70、P2/AC-12/legacy/M6 注记）；③ §1 回写范围改为 00/04/05/06/07/08。`git diff --stat bd683bc..fd936e0` = 1 file changed, 59 insertions(+), 24 deletions(-) | 2026-10-30 |
| C2（L2 重放边界） | `fd936e0`：§3.3 重写——仅 Completed run 支持；前置校验（`record_mode=full`→`REPLAY_RECORD_MISSING`、`status!=Completed`→`REPLAY_RUN_NOT_COMPLETED`、完整性预检、`output_json` 重算 hash→`REPLAY_RECORD_CORRUPT`）；`(node_id, attempt)` 映射唯一性依据（非自环节点 1 次、自环 attempt+1、投递终态 fail run）；`runs.graph_hash` JOIN `graphs.dsl_json`；终态+`error_code`+输出比较；内存 sink 不写库；replay 退出码小表 | 2026-10-30 |

**dry-run 复跑（隔离 target，C1/C2 关闭后）**：

```bash
cd /Users/zyzs/lczyzs/code/zgraph
CARGO_HOME=/Users/zyzs/lczyzs/code/zgraph/.cargo-home CARGO_NET_OFFLINE=true CARGO_TARGET_DIR=/tmp/zgraph-merge-target CARGO_INCREMENTAL=0 PYTHON_BIN=/Users/zyzs/lczyzs/code/zgraph/.venv/bin/python bash scripts/dev/merge.sh docs/process-m5-kickoff --dry-run
# 结果见下方（评审时实跑）
```

```text
（已由评审人补记实跑结果，见文末「dry-run 实际结果补记」）
```

## 7. 评审签署

- 增量复审确认：C1/C2 已由 `fd936e0` 关闭（§6 逐项证据）；首轮发现原文保留、状态已更新（§3）。
- 范围：最终 `1cc9123..fd936e0` = 4 个产品/文档文件（README、DR-002、progress、index）+ 本评审记录 1 个文件；无冻结/代码/CI/spec 正文改动；`check.sh` PASS；progress 非空 17 行（≤30）四块齐全。
- 可执行性：`fd936e0` 修订后的 §3.3/§4/§6 依赖均经代码核对（trace reader/schema、`MemoryTraceSink`/`run_to_completion`、`NodeExecutor::timeout_hint` 对象安全、`start_traced_run_with_writer`、`instances()`、`ac12.rs` 峰值断言）。
- **最终结论：通过**。C1/C2 关闭后 `merge.sh --dry-run` 全门禁通过（见 §6 复跑记录），本分支允许合并。
- 评审人：`reviewer-a`（非作者）；首轮 2026-10-30；增量复审 2026-10-30。

## 8. dry-run 实际结果补记（评审人 `reviewer-a`，2026-10-30）

> 本记录 §6 的 dry-run 输出块因合并时序未回填；评审人按隔离 target 实跑结果补记如下（原记录不删除）。

- 结论：`通过`（首轮「有条件通过」的 C1/C2 已由 `fd936e0` 关闭，见 §6）。
- 命令（隔离 target，避免共享 target 与其它 worktree 构建互踩）：
  `CARGO_HOME=/Users/zyzs/lczyzs/code/zgraph/.cargo-home CARGO_NET_OFFLINE=true CARGO_TARGET_DIR=/tmp/zgraph-merge-target CARGO_INCREMENTAL=0 PYTHON_BIN=/Users/zyzs/lczyzs/code/zgraph/.venv/bin/python bash scripts/dev/merge.sh docs/process-m5-kickoff --dry-run`
- 实跑：评审记录门禁通过 → `cargo test --workspace` **424 passed / 0 failed / 2 ignored**（2 ignored = harness 真模型 smoke + e2e AC-08b 手动）→ pytest 失败 0 / 基线 0（84 passed）→ `dry-run 结束，检查全部通过`（exit 0）。
- 环境注意（同 REV-026 补记）：共享 target 中若残留**已删除 worktree** 编译的测试二进制（`env!(CARGO_MANIFEST_DIR)` 不参与 cargo 指纹），会因陈旧路径失败；先对隔离 target 执行 `cargo clean -p` 全部 workspace 成员再跑即可（本次即如此）。
