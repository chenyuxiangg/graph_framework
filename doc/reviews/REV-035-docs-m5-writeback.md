# REV-035 · docs · M5 spec 回写（07/05/06/00/03/04/08/architecture/PROVENANCE/索引）

> **模块声明（跨模块，Lead 已批准）**：本分支改 `doc/**`（docs 模块）+ 根 `README.md`（**process**）+ `legacy/README.md`（**legacy**，DR-002 §8 明确 M5 仅更新归档说明）+ `doc/reviews/REV-028-cli-m5-run.md`（回填评审记录占位）。Lead 在本轮任务消息中列明 README 跨模块、DR-002 §8 授权 legacy 说明；作者 `docs-writer`，评审人 `reviewer-a`（**非作者**）。评审**未修改分支任何文件**（只新增本记录），未 merge / 未 push / 未改 main；本记录为 reviewer-a 独立评审（docs 分支无第二评审要求，跨模块部分按 DR-002 §8/REV-024 先例由 Lead 声明 + 本记录声明）。
>
> 评审对象：`438822b..e922039`（3 commits：`f88622e` 07/05/06/00/04/08+索引/PROVENANCE、`5a49fce` wave2、`e922039` ISSUE-066 回写；`git diff --shortstat` = **16 files changed, 291 insertions(+), 97 deletions(-)**；base = merge-base = `main` = `438822b`，worktree clean）。

- **分支**：`docs/docs-m5-writeback`
- **作者**：`docs-writer`
- **评审人**：`reviewer-a`（**非作者**）
- **变更范围**：`438822b..e922039`（16 files, +291/−97；仅 `doc/**`、根 `README.md`、`legacy/README.md`）
- **评审时间**：2026-10-30
- **关联**：DR-002 §9 回写清单（REV-025 C1 关闭）、DR-002 §8（legacy 归档）、DR-002 §3（replay 契约）、REV-026/027/028/029/030/031/032/033/034/038、spec 06 §6.2.5.1/§6.2.13/§6.8.1/§6.8.3/§6.10、spec 07 AC-01/AC-12、architecture §5.2/§5.3/§6.1/§8.2/D10

## 1. 结论

**结论：有条件通过**

本分支把 M5 wave1/wave2（含 ISSUE-066）的实现事实系统回写到 spec 07/05/06/00/03/04/08 + architecture/PROVENANCE，**大部分回写准确**：我已独立核对 spec 文件行数/SHA-256 与 PROVENANCE §3 **逐项一致**；08 仅追加 §8.7（8.0–8.6 历史快照未改）；spec 内部交叉引用（§6.2.13/§6.8/§6.10.5–§6.10.7/00 §0.4/architecture §5.2/§8.2）可解析且术语一致；06 §6.8.1 标定残余风险、06 §6.10.7 replay 契约、07 AC-01/AC-12 实测口径与 `tests/e2e/cli_ac01.rs`/`cli_replay.rs`、`crates/zgraph-cli/src/replay_cmd.rs`、`recovery.rs`、`crates/zgraph-harness/src/{record,executor}.rs` 的实现事实一致；`check.sh` PASS、scope 无代码/台账/进度/CLAUDE 夹带。

但发现 **3 项需要先修正的问题**（条件见 §6）：

- **C1（重要，事实/状态）**：根 `README.md:24-25` 的「已交付/待交付」状态段与 main 事实不符——把**已合并**的 REV-030（replay L1/L2 + `llm_calls`）、REV-031（ISSUE-066 signal/exit）、REV-032（e2e CLI/L2/崩溃标定）、CLI 启动中止标定/`--json.recovered[]` 仍列为「待交付（wave2，未合并）」；同时把未合并的 REV-035 列进「已交付并合并到 main」，并把 ISSUE-076 写成「部分修」（05 §5.13.3/08 §8.7 已写 M5 已修）。
- **C2（重要，可执行性）**：`doc/spec-v1.2/07-验收用例.md:56-61` AC-01 的 `run` 命令缺 `--python-path`（worker SDK 与 `<assets>/py` 的 `gates.hr`），且紧邻的「命令形状即 M5 契约」使该命令成为验收口径；我按原文逐字执行**失败**（`HARNESS_CONFIG_LOAD/tool_module_import`），补 `--python-path python` + `--python-path tests/fixtures/graphs/customer_ticket/py` 后 `Completed`（§4.3）；`tests/e2e/cli_support/mod.rs:485-507` 的 `ac01_run_args` 正是这么传的。
- **C3（重要，审计留痕）**：`doc/spec-v1.2/PROVENANCE.md:97` 用新的「5956 行（M5 全部回写批次后）」**替换**了原「**5818 行**（M4 回写批次后实测，评审时 base `origin/main` `fda996c`，合并 rebase 至 `12ab2f6`）」这一历史测量行。Lead 本轮明确「PROVENANCE 历史批次数值不改」；§2 第 1–7 条历史批次条目未改（✓），但该 M4 历史总行数/base/merge 值应作为历史注记保留（可与新 5956 并列），不能静默覆盖。

其余为建议/Nit（05 L8 函数数字、07 AC-12 基线数字来源、06 §6.10 标题未含 replay、REPLAY_RECORD_CORRUPT 触发描述宽窄不一、07 §7.1 prompt 路径、spec README 只引 §2 第 8/9 批、doc/index 范围、台账待 REV-036 收口等，见 §3 #4–#11），不阻塞；条件关闭后可转「通过」。

## 2. 四维评审

| 维度 | 结论 | 依据 |
|---|---|---|
| 功能正确性（回写准确性） | ⚠️ 有条件 | 大部分逐条核对通过（§4.3）：07 fixture 落点/AC-12 组合/e2e 计数、05 §5.2.6/§5.6.5、06 §6.2.5.1/§6.2.13/§6.8.1/§6.8.3/§6.10.3/§6.10.5–7、00 REPLAY_* 四码、architecture §5.2/§5.3/§6.1/§8.2、08 §8.7 引用均与代码/评审记录一致；但 README 状态段（C1）、AC-01 命令（C2）、PROVENANCE 历史值（C3）需修正。 |
| 代码结构合理性（文档组织） | ✅ | 回写按「分册/章节 + 批次（PROVENANCE §2 第 8/9/10 条）」组织；06 新增 §6.2.13（L2 表）与 §6.10.7（replay）位置合理；08 以 §8.7 追加而非改历史；索引只列已存在记录。scope 严格限定文档（无代码/依赖/台账）。 |
| 逻辑性 | ✅ | 每处实现口径都给了「来源 REV/commit」与「命令/字段」；状态与 DR-002 §4/§9、issues 状态大体自洽（未实现项未写成 fixed：ISSUE-084 保持 open、ISSUE-081/082 部分修、M6 排除项单列）；残余风险（多机/EPERM/host、worker 自报 crash 无证据、stderr 不脱敏）如实写明。 |
| 可读性 | ✅ | 表格/小标题结构清晰，术语与错误码一致（REPLAY_* 四码、NODE_WORKER_* 四码）；新增段落均有「M5/REV 编号/章节」锚点。Nit：个别数字/措辞需同步（§3 #4/#5/#6）。 |

## 3. 发现清单

| # | 级别 | 位置 | 问题 | 建议 | 状态 |
|---|---|---|---|---|---|
| 1 | **重要（C1，事实/状态）** | `README.md:24-25` | 状态段与 main（`438822b`，含 REV-030 `64f110a`、REV-031 `438822b`、REV-032 `58239fc`）不符：①「待交付（wave2，未合并）」列了 REV-030（replay L1/L2、`llm_calls`）、CLI 启动中止标定/`--json.recovered[]`、ISSUE-066（REV-031）、e2e CLI/L2/崩溃标定（REV-032）——均**已合并**；②「已交付并合并到 main」含 REV-035（本分支，未合并时点）；③ ISSUE-076 写「部分修」，而 05:217/1407、08:425 写 M5 已修（kernel/harness/CLI/e2e 全落地） | 按 main 事实重写两段：已交付加入 REV-030/031/032、ISSUE-066 fixed、ISSUE-076 fixed（或与 REV-036 台账同步后 fixed）；待交付仅剩「本批 docs 回写（REV-035）→ 合并」「REV-036 收尾（issues/progress/README/baseline）」「REV-037 process 依赖」「M6 排除项」；本批 REV-035 不要写成「已合并 main」 | **open（条件）** |
| 2 | **重要（C2，可执行性）** | `doc/spec-v1.2/07-验收用例.md:56-61`（AC-01 `run` 命令） | 命令缺 `--python-path`：worker SDK（`python/`）与 `gates.hr`（`tests/fixtures/graphs/customer_ticket/py`）均需入 `PYTHONPATH`。按文档逐字执行失败：`HARNESS_CONFIG_LOAD ... tool_module_import path=.../customer_ticket/tools`；补两个 `--python-path` 后 `Completed`。e2e `ac01_run_args` 传 `--python-path <repo>/python` + `<assets>/py`（`tests/e2e/cli_support/mod.rs:485-507`）；相邻句「命令形状即 M5 契约」因此不成立 | 在 AC-01 命令中补 `--python-path python`（或说明 SDK 已安装/PYTHONPATH）与 `--python-path tests/fixtures/graphs/customer_ticket/py`，或加一行「运行前需把 `python/` 与 `<assets>/py` 加入 PYTHONPATH / `--python-path`」；与 e2e 参数对齐后再保留「命令形状即契约」 | **open（条件）** |
| 3 | **重要（C3，审计留痕）** | `doc/spec-v1.2/PROVENANCE.md:97` | 原历史行「00–08 合计 **5818 行**（M4 回写批次后实测，评审时 base `origin/main` `fda996c`，合并 rebase 至 `12ab2f6`）」被替换为新 5956 行总结；Lead 口径「PROVENANCE 历史批次数值不改」。§3 表格本身更新当前 hash/行数合理，但 M4 历史测量值/基址不应静默删除 | 保留新 5956 行当前实测，同时补一行历史注记（如「M4 批次实测：5818 行；base `fda996c`；合并 rebase `12ab2f6`」）；或在 §2 第 7/8 条注明历史值 | **open（条件）** |
| 4 | 建议 | `doc/spec-v1.2/05-AgentHarness.md:1414`（L8）；`08:426` | L8 剩余 >60 行清单过期：`cli run_in_runtime` 写 **70**，实际 main 为 **91**（REV-030 #10 已记录 91；`crates/zgraph-cli/src/run_cmd.rs:722`）；「新增 74 行测试」也与现状不符（当前 `tests/run.rs:625` 用例 68 行；REV-030 新增超限测试 78/88 行，见 REV-030 #10）；08 §8.7 同句复用「新增 74 行测试」 | 按 REV-030 #10 更新为 `run_in_runtime 91`、并改为「新增 >60 行测试（78/88 等，REV-030）」；08 同步 | open（建议） |
| 5 | 建议 | `doc/spec-v1.2/07-验收用例.md:209`（AC-12） | 「单 run 基线 **792–846ms**」无来源：`tests/e2e/ac12.rs:40-42` 文件头与 REV-029 记录均为 **0.79–0.85s**（评审独立复跑 0.84–0.95s）；`2.46–2.54s/2.9–3.1` 有来源。数字精度疑似转录误差 | 改为 **0.79–0.85s**（或注明「本机实测区间/来源」，与 REV-029 #4 统一） | open（建议） |
| 6 | 建议 | `doc/spec-v1.2/06-存储与可观测.md:764`（§6.10 一句话结论）、`:774`（§6.10.1 全局参数） | §6.10 新增了 §6.10.7 `zgraph replay`，但结论仍写「三个子命令 `validate`/`run`/`trace`」；全局参数 `--db <path>`（run/trace）未列 replay（replay 实际支持 `--db`） | 改为四个子命令（validate/run/replay/trace）与 `--db`（run/replay/trace） | open（建议） |
| 7 | 建议 | `00-总纲.md:212`、`06:906`（§6.10.5 行）、`architecture:279` | `REPLAY_RECORD_CORRUPT` 触发描述只说「记录 hash 复核失败」；§6.10.7（`06:960,964`）与代码一致地还包括**重复记录/缺 `output_json`/`entry_inputs` 非法**（`replay_cmd.rs` build_plan 356-393, 436-441）。同码多处宽窄不一 | 三处统一为「记录损坏（hash 不符/重复/缺 `output_json`/`entry_inputs` 非法）」，避免读者按窄描述实现 | open（建议） |
| 8 | 建议 | `doc/spec-v1.2/07-验收用例.md:18`（§7.1 Agent 资产） | 写 `prompts/*.md`，实际为 `tests/fixtures/graphs/customer_ticket/agents/prompt.md`（单数、位于 `agents/` 内；`harness_ac08/agents/prompt.md` 同） | 改为 `agents/prompt.md`（或按未来约定统一） | open（Nit） |
| 9 | 建议 | `doc/spec-v1.2/README.md:10,61` | 只引「M5 批次见 §2 第 8 条」「wave2 见第 9 条」，漏了第 10 条（ISSUE-066 回写）；读者无法从 README 追到第三批 | 补「第 8–10 条（wave1/wave2/ISSUE-066）」 | open（Nit） |
| 10 | 建议（流程/台账） | `doc/spec-v1.2/08-审计关闭矩阵.md:425,429,442`；`README.md:24`；`doc/issues.md:56,74,84` | 本分支（按分工不改 issues）把 ISSUE-048/076 写「M5 已修」、ISSUE-066 写 fixed，而 `doc/issues.md` 仍 open/部分修（048 行 56、066 行 74、076 行 84）；README 又写 076 部分修；08:442 的「REV-032 #1 kernel `fail_timeout` 未写 instances.error_code」台账待办尚未有 ISSUE 号（AGENTS §7 要求登记）。三处口径需收口 | **REV-036 收尾**：issues.md 把 048/066/076 转 `fixed`（注明 REV 编号）、082/081 保持部分修、084 保持 open（REV-030 #2-#5 残余）；新登记 kernel `fail_timeout` 实例错误码 ISSUE 并在 08 引用；README/05/08 统一措辞 | open（REV-036） |
| 11 | Nit | `doc/index.md:17` | 「REV-000–REV-038（… M5 已合并 REV-025–034/038）」：REV-035 本批才存在、REV-036/037 尚未开始/未合并，范围表述易被读成连续存在 | 改为「REV-000–REV-038（M5 见索引；REV-035 本批、REV-036/037 待合并/未开始）」 | open（Nit） |
| 12 | 备注（已验证正向项） | 全 diff | ⑧ 08 仅新增（无删改 8.0–8.6）；PROVENANCE §2 第 1–7 条历史批次条目未改；spec 00–08 行数/SHA-256 与 PROVENANCE §3 逐项一致（§4.2）；无代码/issues/progress/CLAUDE 改动；legacy/README 改动符合 DR-002 §8；`doc/reviews/REV-028` 仅回填 §8.4 占位（不删原记录，内容与事实一致） | 无需动作；`REV-028` 回填建议由原评审人/Lead 在收尾时确认（流程备注） | 无需处理 |
| 13 | 备注（已验证正向项） | 06/07/05/00/architecture/08 | 实现事实核对：06 §6.8.1 与 `cli/src/recovery.rs`（HOSTNAME/COMPUTERNAME、EPERM、warnings=[]、trace 未校验 host）及 REV-030 #2-#5 一致；§6.8.3 pid 提示；§6.10.3 `--python`/`--record`/fake-llm/`ZGRAPH_DB`；§6.10.5–7 与 `replay_cmd.rs`（open_readonly、precheck 顺序、RecordedExecutor、MemoryTraceSink、matched 比较、graph 缺失→storage exit4）一致；§6.2.13 与 `run_cmd.rs`/`harness record.rs`（仅 full 写 llm_calls、脱敏、hash/canonical_json、recorded_outputs 写者）一致；07 AC-12 组合/峰值/26 段/SQLITE_BUSY、AC-01 三用例与 `cli_ac01.rs`/`cli_replay.rs`、e2e 38 tests/37 passed+1 ignored 一致；05 §5.2.6 `peek_agent_node_timeout`/转发、§5.6.5 结构截断/唯一标记/≤8KiB 与代码/REV-027 一致；08 §8.7 引用 REV-031 §7.3 #3（source-breaking）与 REV-032 #1（fail_timeout）存在且对应；DR-002 #10/#11/#12 更新正确（#12 已写 91，与 05 L8 的 70 形成对照） | 无需动作 | 无需处理 |

> 条件 C1/C2/C3 为阻塞项（关闭后才可合并）；#4–#11 建议/Nit 可在同一修订提交一并收敛，或明确由 REV-036 处理（#10 明确由 REV-036 收口）。

## 4. 验证证据

### 4.1 范围、提交与工作区

```bash
cd /Users/zyzs/lczyzs/code/zgraph/.worktrees/docs-docs-writer
git status --short                       # 空（评审时 clean）
git branch --show-current                # docs/docs-m5-writeback
git merge-base main HEAD                 # 438822b...（= main，3 commits ahead）
git log --oneline main..HEAD             # f88622e / 5a49fce / e922039（均含 Review: REV-035 / Agent: docs-writer）
git diff --shortstat main...HEAD         # 16 files changed, 291 insertions(+), 97 deletions(-)
git diff --name-status main...HEAD       # README.md、legacy/README.md、doc/**（16 文件；无 crates/tests/Cargo/issues/progress/CLAUDE）
bash scripts/dev/check.sh                # CHECK PASSED（身份/冻结文件/禁用文件/progress ≤30/fmt/diff-check）
```

### 4.2 历史审计与规格指纹（独立实测）

| 检查 | 结果 |
|---|---|
| `doc/spec-v1.2/08-审计关闭矩阵.md` 变更 | `git diff` 仅 `+` 行（新增 §8.7），无删改 8.0–8.6 历史快照 ✅ |
| `PROVENANCE.md` §2 第 1–7 条历史批次 | 未改（diff 仅追加第 8/9/10 条 + §3 当前指纹表 + 总结行）✅（C3 只涉及总结行历史值被覆盖） |
| 00–08 行数 | `wc -l`：294/302/913/390/895/1420/1068/230/444，合计 **5956**；与 PROVENANCE §3 和 spec README 表逐项一致 ✅ |
| 00–08 SHA-256 | `shasum -a 256` 与 PROVENANCE §3 **逐项一致**（00 `a414f809…`、03 `69f33ff7…`、04 `2d4a5ce9…`、05 `06eb2d04…`、06 `72f18d2a…`、07 `8bc27af2…`、08 `66894447…` 等）✅ |
| 禁用/冻结 | `git diff --name-status` 无 `CLAUDE.md`、无 `crates/**`、无 `doc/issues.md`、无 `doc/progress.md`、无 `Cargo*` ✅ |
| legacy/README | 更新为「已归档、无维护、不得新增代码」+ DR-002 §8 注记（符合 DR-002 §8 M5 归档口径）✅ |

### 4.3 回写准确性抽查（对照实现/评审记录，独立复跑）

| 文档断言 | 独立核对 | 结论 |
|---|---|---|
| 07 AC-01 fixture 落点 `tests/fixtures/graphs/customer_ticket.json` + `customer_ticket/**`（agents 4/prompt.md/py/gates/hr.py/fake_llm.json/tools）+ 6 nodes/6 edges + validate 输出格式 | 文件存在；`zgraph validate` 实测输出 `✓ graph valid: 6 nodes, 6 edges, 0 subgraphs (graph_hash=ec320e42…)`；e2e `cli_ac01.rs` 3 用例 | ✅（命令见 C2） |
| 07 AC-01 `run` 命令按原文可执行 | **实测失败**：`HARNESS_CONFIG_LOAD ... tool_module_import path=…/customer_ticket/tools`；补 `--python-path python` + `--python-path tests/fixtures/graphs/customer_ticket/py` 后 `Completed`（event_count=30；`run_output.result` 四字段、`content` 二次解析 `answer=可以退款/need_human=false`） | ❌ C2 |
| 07 AC-01 e2e「3 用例 / 37 passed / 1 ignored / gate 拦截 1 次 / python 实例输出 `{"need_human":false}`」 | `tests/e2e/cli_ac01.rs` 3 tests；e2e 包共 38 tests（含 1 ignored）→ 37 passed/1 ignored；REV-032 记录一致 | ✅ |
| 07 AC-12「8 python + 2 agent 组合 / `with_limits(4,2)` / 全局峰值 4 / 每 run 2 / 26 段 / SQLITE_BUSY 3/3 未触发 / REV-032 未新增 10×同图」 | `tests/e2e/ac12.rs` 头注释与断言、REV-029/REV-032 记录一致 | ✅（仅基线数字见 #5） |
| 07 AC-12 单 run 基线 792–846ms | ac12.rs 头与 REV-029 #4 为 0.79–0.85s；评审复跑 0.84–0.95s；**无 792–846ms 来源** | ⚠️ #5 |
| 05 §5.2.6 timeout 链（test-hooks force > declared > `timeout_hint` > 180s；Harness only agent_call；`peek_agent_node_timeout` 轻量只读；CLI/e2e 转发） | `harness/config.rs:226` `peek_agent_node_timeout` 存在；`harness/executor.rs:197`、`cli/run_cmd.rs:429,483`、`e2e/ac12.rs:304`、`harness_ac08.rs:104` 的 `timeout_hint` 转发；REV-026/033/030/032 | ✅ |
| 05 §5.6.5 结构感知截断（合法 JSON ≤8 KiB、字符串内 `…[truncated N bytes]`、对象 `_truncated`/数组标记、traceback ≤8 KiB、summary ≤256/≤512） | `harness/tools.rs:370-649`（`TRUNCATED_OBJECT_KEY`/`_truncated`、`truncate_largest_string`、`truncate_envelope`、`truncate_chars`）；REV-027 | ✅ |
| 05 L8 剩余 >60 行（`executor::execute 71→78`、`cli parse 79 / run_in_runtime 70`、新增 74 行测试） | 实测 `executor::execute` 78 行 ✅；`cli run_in_runtime` **91** 行（`run_cmd.rs:722`）、`parse` 79 行；当前 `tests/run.rs:625` 用例 68 行；REV-030 #10 记录 91/88/78 | ⚠️ #4 |
| 06 §6.8.1 标定两段原语/判活分工/残余风险（#2-#5） | `cli/recovery.rs` 的 HOSTNAME→COMPUTERNAME→unknown、EPERM 处理、`warnings: []`（run_cmd.rs:1053）、trace 未校验 owner_host（trace_cmd.rs:223）；REV-034/030 记录 | ✅ |
| 06 §6.8.3 trace pid 提示（stderr、stdout 纯净、exit 0）与 interrupted 截断（exit 1 + kill -9 e2e） | `trace_cmd.rs`；REV-030/032 记录 | ✅ |
| 06 §6.10.3 `--python` 优先级 / `--record` / `--fake-llm` 裸 flag / `ZGRAPH_DB` / 标定启动 | `cli` 代码与 REV-028/030 | ✅ |
| 06 §6.10.5 replay 列 + 退出码（0/1/2/3/4/70/130） | `replay_cmd.rs` 常量与 `finish_outputs`；REV-030/032 | ✅（REPLAY_RECORD_CORRUPT 描述见 #7） |
| 06 §6.10.7 replay 契约（events 委托 trace 逐字节、outputs 预检顺序、RecordedExecutor、MemoryTraceSink、global_vars 空、matched 比较、不写库、graph 缺失 exit4） | `replay_cmd.rs:181-194`（委托 trace）、`288`（open_readonly）、`331-393`（预检：record_mode→Completed→output_json/hash/重复→完整性）、`428-450`（build_spec，global_vars 空）、`463-466`（matched=Completed && error None && output==runs.output_json）、`load_recorded_graph` storage→exit4；e2e `cli_replay.rs` 9 用例（DB 字节/marker/`ZGRAPH_PYTHON=/nonexistent`/L1 逐字节） | ✅ |
| 06 §6.2.13 L2 表写者/键/hash/仅 full/status/脱敏 | `run_cmd.rs:488-530`（LlmCallTraceRecorder/RecordingExecutor）、`854-863`（仅 full 挂 llm_calls）；`harness/record.rs:11,42,60,271-276`（不读取 api_key/SecretString、脱敏断言） | ✅ |
| 06 §6.2.5.1 NodeWorker 三键（仅崩溃进程退出路径、signal/exit 互斥、stderr_tail ≤1 KiB 字符边界、不脱敏、source-breaking） | REV-031（主审 §3 + 第二评审 §7.3 #3）；`worker/executor.rs:247-301`、`kernel/types.rs:442-451`、`cli/bridge.rs:580-598` | ✅ |
| 00 §0.4 REPLAY_* 四码/退出码/不写 error_code；architecture §8.2 四码、§5.2 L2 写入口径、§5.3 replay 退出码、§6.1 legacy、D10；DR-002 #10（ISSUE-084 已实现保持 open）、#11（#6 编号修正）、#12（91/78/88 函数清单） | `00:212`、`arch:188/277-280`、`DR-002:79/104/143+`；代码与 REV 记录 | ✅（宽窄描述见 #7） |
| 08 §8.7 状态（066 fixed@438822b、048/076 已修、081/082 部分、084 open、M6 清单、REV-032 #1 台账待办、REV-031 §7.3 #3 source-breaking） | main 包含 438822b（REV-031 已合并）；REV-032 #1 行存在（kernel fail_timeout）；REV-031 §7.3 #3 存在；issues.md 048/076/066 仍 open/部分（见 #10） | ✅ / 台账待 REV-036 |
| spec README/PROVENANCE 指纹与批次引用 | 行数/SHA 逐项一致；批次 5938/5949/5956 递进自洽；README 引用缺第 10 条（#9） | ✅ / ⚠️ #9 |

### 4.4 门禁命令

| 命令 | 结果 |
|---|---|
| `bash scripts/dev/check.sh` | **CHECK PASSED**（身份/冻结文件/禁用文件/progress ≤30/fmt/diff-check） |
| `git diff --* main...HEAD` | 16 files, +291/−97；仅 doc/**、README.md、legacy/README.md（无代码/依赖/台账/进度/CLAUDE） |
| `wc -l` + `shasum -a 256`（00–08） | 合计 5956 行；SHA 与 PROVENANCE §3 逐项一致 |
| `zgraph validate tests/fixtures/graphs/customer_ticket.json` | exit 0，`✓ graph valid: 6 nodes, 6 edges, 0 subgraphs (graph_hash=ec320e4244b365ab)` |
| `zgraph run …`（AC-01 原文命令） | **失败** `HARNESS_CONFIG_LOAD/tool_module_import`（C2） |
| `zgraph run … --python-path python --python-path tests/fixtures/graphs/customer_ticket/py` | **Completed**（`run_output.result` 四字段、`content` 可二次解析） |

### 4.5 merge.sh dry-run

- 当前结论为**有条件通过**（未关闭 C1/C2/C3），按 AGENTS §4/DR-002 合并脚本会在「评审记录结论必须为通过」处阻断；条件关闭后（作者在同一分支修订 + 本记录追加「修订/复审/条件关闭」段并翻结论）再跑 `merge.sh docs/docs-m5-writeback --dry-run`（隔离 target）验证全门禁。

## 5. 遗留与条件关闭

- **C1**：README 状态段按 main 事实重写（已合并/待交付列表、REV-035 自身归属、ISSUE-076 状态与 05/08 统一）。
- **C2**：07 AC-01 `run` 命令补 `--python-path`（SDK + `<assets>/py`）或明确 PYTHONPATH 前置；与 `tests/e2e/cli_support/mod.rs:485-507` 对齐；「命令形状即 M5 契约」才成立。
- **C3**：PROVENANCE §3 保留 M4 历史实测值（5818 行、base `fda996c`、merge `12ab2f6`）作为历史注记，与新 5956 行并列。
- **建议同批/收尾**：#4（05 L8 91/超限测试数字、08 同步）、#5（AC-12 基线数字来源）、#6（06 §6.10 四子命令/--db）、#7（RECORD_CORRUPT 三处描述统一）、#8（prompt 路径）、#9（spec README 补第 10 批引用）、#11（doc/index 范围措辞）。
- **REV-036 收尾**（#10）：issues.md 048/066/076→fixed、082/081 部分、084 open（REV-030 #2-#5 残余）；补登记 kernel `fail_timeout` 实例错误码 ISSUE；统一 README/05/08 状态；更新 reviews 索引（含 REV-035/036）、progress、baseline。
- **流程备注**：`doc/reviews/REV-028-cli-m5-run.md` 的 §8.4 占位由 docs-writer 回填（非原评审人）；内容与事实一致且不删原记录，建议后续记录变更尽量由原评审人/Lead 执行。
- 本评审未修改分支任何文件；REV-035 记录为本分支唯一新增文件（由评审人提交）。

## 6. 条件关闭表

| 条件 | 关闭标准 | 状态 |
|---|---|---|
| C1 README 状态段 | `README.md:24-25` 与 main `438822b` 事实一致（REV-030/031/032 已合并、REV-035 本批、ISSUE-066/076 状态与 05/08 一致；待交付仅 REV-035 合并/REV-036/REV-037/M6） | open |
| C2 AC-01 命令 | `07:56-61` 命令含 `--python-path`（或明确 PYTHONPATH 前置）后可逐字执行成功（同 §4.4 复跑） | open |
| C3 PROVENANCE 历史值 | §3 保留「M4 批次 5818 行/base `fda996c`/merge `12ab2f6`」历史注记（与新 5956 并列） | open |

## 7. 评审签署

- 独立核对：16 文件全量 diff；spec 00–08 行数/SHA-256 与 PROVENANCE §3 逐项实测一致；08 仅追加、PROVENANCE §2 历史条目未改；scope 无代码/台账/进度/CLAUDE；07/05/06/00/03/04/08/architecture 与 REV-026/027/028/029/030/031/032/033/034/038 及实现代码逐条抽查（§4.3），AC-01 命令实测复现（失败/修复成功）。
- 发现 C1/C2/C3 三项重要问题（均可在同一分支小改关闭），其余 8 条建议/Nit；无阻塞性设计缺陷。
- **结论：有条件通过**（C1/C2/C3 关闭后转「通过」；条件关闭时在本记录追加「修订/复审/条件关闭」段并更新结论，原段保留）。
- 评审人：`reviewer-a`（非作者）；评审时间：2026-10-30。

## 8. 修订与复审（C1/C2/C3 条件关闭，2026-10-30）

> 本章为评审人 `reviewer-a` 的**追加复审段**（AGENTS §6：不删改 §1–§7，原「有条件通过」结论与证据保留）。复审对象：作者修订 commit **`16a9a69`**（同一分支、未 amend 本记录；`git diff 7cedd60..16a9a69` = **10 files changed, +37/−24**，全部 `doc/**` + 根 `README.md`）。

### 8.1 C1 关闭（README 状态段）

修订后 `README.md:24-26`：

- 「已交付并合并到 main」加入 **REV-030**（`zgraph replay` L1/L2、`llm_calls` 落库、CLI 启动中止标定/`--json.recovered[]`/`trace` 进行中提示）、**REV-032**（e2e AC-01/L2/崩溃标定/ISSUE-076 端到端），并把 **ISSUE-061/074/066/076** 统一为「M5 已修」；保留 REV-028/029/034/025/038；
- **REV-035 移入「本批待合并」**，不再自称「已合并到 main」；
- 待交付仅「**REV-036 收尾**（issues/progress/README/评审索引/baseline）→ **REV-037**（process 依赖治理）→ **M6 排除项**（DR-002 §7）」。

与 main `438822b` 的事实核对：REV-030（`64f110a`）、REV-031（`438822b`）、REV-032（`58239fc`）均已合并；ISSUE-061/074 已 fixed，ISSUE-066/076 实现已合并（台账由 REV-036 收口）——**C1 关闭**（台账同步作为 §8.6 残留 #2，不阻塞）。

### 8.2 C2 关闭（AC-01 命令可执行）

`07-验收用例.md:57-63` 的 `run` 命令已补：

```bash
--python-path python \
--python-path tests/fixtures/graphs/customer_ticket/py \
```

并注明「`--python-path` 是命令的一部分；等价设置 `PYTHONPATH=python:tests/fixtures/graphs/customer_ticket/py`；与 `tests/e2e/cli_support/mod.rs::ac01_run_args` 一致，可直接执行」。评审人**逐字复跑**修订后命令：`run` exit 0、`status="Completed"`、`event_count=30`、`run_output.result` 四字段且 `content` 二次解析出 `answer="可以退款"`/`need_human=false`；`validate` exit 0（`✓ graph valid: 6 nodes, 6 edges, 0 subgraphs`）——**C2 关闭**。

### 8.3 C3 关闭（PROVENANCE 历史留痕）

`PROVENANCE.md` §3 现同时保留：

- 当前实测：表内 9 文件 SHA-256/行数（合计 **5958** 行）；
- 历史留痕（不覆盖）：**「M4 Harness 回写批次后 00–08 合计 5818 行；base `origin/main` `fda996c`；合并 rebase `12ab2f6`」**；§2 第 1–7 条历史批次条目未改，仅追加第 11 条修订说明。

独立实测：`wc -l` 00–08 = **5958**；`shasum -a 256` 与 §3 **逐项一致**（00 `a9b12764…`、05 `405aaa99…`、06 `8f3613b6…`、07 `38d2e56c…`、08 `4378d379…`；01/02/03/04 未变）——**C3 关闭**。

### 8.4 建议收敛核对（§3 #4–#11）

| 建议 | 修订结果 |
|---|---|
| #4 05 L8 / 08 ISSUE-081 数字 | L8 改为 `run_in_runtime` **91**（REV-030）、新增 `replay build_plan` 75、新增 >60 行测试 74/78/88 等；08:426 同步为 91/build_plan 75/74/78/88 等 ✅（74 为 REV-028 期旧值，「等」已限定，见 §8.6 #3） |
| #5 AC-12 基线 | 07:211 改为 **0.79–0.85s**，与 `tests/e2e/ac12.rs:40-42`/REV-029 #4 一致 ✅ |
| #6 06 §6.10 replay | §6.10 一句话结论改「四个子命令 validate/run/replay/trace」；§6.10.1 `--db`（run/replay/trace）；06 头部状态行同步 ✅ |
| #7 RECORD_CORRUPT 描述 | 00:212、06:906/§6.10.5、architecture:188/279 统一为「记录损坏：hash 复核失败/重复/缺 `output_json`/`entry_inputs` 非法」，与 06:960/964 及 `replay_cmd.rs` 一致 ✅ |
| #8 prompt 路径 | 07:18 改为 `agents/prompt.md` ✅ |
| #9 spec README 批次 | README:10/61 改为「M5 批次见 §2 第 **8–10** 条」✅ |
| #11 doc/index 范围 | 改为「REV-000–REV-034 与 REV-038；REV-035/036 待合并/收尾后补」✅ |

### 8.5 复审命令与结果

| 命令 | 结果 |
|---|---|
| `git diff 7cedd60..16a9a69` | 10 files changed, +37/−24；全部 `doc/**` + `README.md`（无代码/台账/进度/CLAUDE） |
| `bash scripts/dev/check.sh` | **CHECK PASSED** |
| `wc -l 00–08` + `shasum -a 256` | 合计 **5958** 行；SHA 与 PROVENANCE §3 逐项一致 |
| `zgraph run …`（07 AC-01 修订后原文命令） | **exit 0 / Completed**（`event_count=30`，`run_output.result` 四字段） |
| `zgraph validate tests/fixtures/graphs/customer_ticket.json` | exit 0（6 nodes/6 edges/0 subgraphs） |
| `merge.sh docs/docs-m5-writeback --dry-run`（隔离 target `/tmp/rev035b-target`，先清理工作区成员） | 结果见 §8.5.1 |

#### 8.5.1 merge dry-run（结论翻「通过」后实跑）

- **实跑（全新隔离 target `/tmp/rev035b-target`；`cargo clean -p` 工作区成员为 no-op；load avg≈6.5）**：评审记录校验通过（脚本输出 `评审记录 -> 通过`）→ `INFO: cargo test 通过` → pytest 失败 **0** / 基线 **0**（无新增）→ `INFO: dry-run 结束，检查全部通过`（exit 0）。
- 本次 dry-run 未出现 AC-09 时序/worker 握手 flake；`git fetch origin main` 未报错（merge.sh 未因 fetch 中止）。

### 8.6 残留（不阻塞，交 REV-036）

1. **时点语句**：`README.md:25`「待关闭评审 C1–C3 后合并」与 `PROVENANCE.md` §2 第 11 条末句「等待 reviewer-a 复审翻「通过」」在本次复审后已成历史时点描述；建议 REV-036 顺手改为完成态（不阻塞合并）。
2. **台账同步（原 §3 #10）**：`doc/issues.md` 的 ISSUE-048/066/076 仍 open/部分修、ISSUE-082/081 保持部分、ISSUE-084 保持 open；REV-036 需按实现事实转 fixed/open 并在 08 引用新登记的 kernel `fail_timeout` ISSUE（ISSUE-086）——同时统一 README/05/08 措辞。
3. **Nit**：05 L8「74/78/88 等」中的 74 为 REV-028 期旧值（当前 `tests/run.rs:625` 为 68 行，REV-030 #10 明确 78/88），因有「等」且 91/build_plan 75 已正确，不阻塞；REV-036 可顺手精确化。

### 8.7 复审结论

- C1/C2/C3 **全部关闭**；§3 建议 #4–#11 已同批收敛；修订未引入新的事实错误（README 状态、AC-01 命令、PROVENANCE 指纹/历史留痕、四子命令、REPLAY 错误码描述、索引范围均复核通过）；`check.sh` PASS、AC-01 命令可逐字执行、workspace 全量 dry-run 通过。
- 残留仅为 REV-036 收尾事项（台账同步/时点语句/Nit），不阻塞本分支合并。

**结论：通过**

- 评审人：`reviewer-a`（非作者）；复审时间：2026-10-30；原 §1–§7 保留不变。
