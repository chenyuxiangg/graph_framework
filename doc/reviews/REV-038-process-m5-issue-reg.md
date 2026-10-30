# REV-038 · docs/process · M5 台账登记（ISSUE-084/085）+ DR-002 wave2/回写修订 + 进度更新

> **模块声明**：本分支只改 `doc/**`（`docs` 模块：`doc/issues.md`、`doc/decisions/DR-002-m5-cli-replay-recording.md`、`doc/progress.md`），另由评审人在同 commit 追加 `doc/reviews/REV-025/REV-026` 的「dry-run 实际结果补记」（append-only，不删除原记录）并新增本记录；**无代码/测试/CI/`Cargo.lock`/`CLAUDE.md` 改动**。作者 `lead`，评审人 `reviewer-a`（**非作者**）。`issues.md`/`progress.md` 的写入均为本任务相关行（AGENTS §2 并发写约定）。
>
> 评审对象：`ad7fbec..d81eefd`（1 commit `d81eefd docs(process): 登记 ISSUE-084/085 + DR-002 wave2/回写修订 + 进度更新（REV-038）`；base = `ad7fbec` = 评审时本地 `main`）。
>
> **与 REV-028 C1 的关系**：本分支登记 **ISSUE-084**（06 §6.8.1 中止标定/§6.8.3 interrupted 提示/`--json.recovered[]` 未实现，wave2 `feat/trace-m5-recovery`(REV-034) + cli(REV-030) 落地）并在 DR-002 §4/§6/§9 固化口径 → **REV-028 C1 的关闭条件（登记 ISSUE + 决策文档注记）已满足**；REV-028 合并后的增量复审另行追加到 REV-028 记录。

- **分支**：`docs/process-m5-issue-reg`
- **作者**：`lead`
- **评审人**：`reviewer-a`（**非作者**）
- **变更范围**：`ad7fbec..d81eefd`（1 commit；`git diff --shortstat` = **3 files changed, 28 insertions(+), 18 deletions(-)**；base = merge-base = `ad7fbec` = `main`，**基于最新 main**；评审时 worktree clean）
- **评审时间**：2026-10-30
- **关联**：AGENTS §3/§5/§6/§7；REV-025（DR-002 首轮）/REV-026（ISSUE-061/076）/REV-027（ISSUE-074/081/082 seam）/REV-028（C1：ISSUE-084/085 来源）/REV-029（AC-12）；spec 06 §6.8.1/§6.8.3/§6.10.3、07 §7.3 #6；ISSUE-061/074/076/081/082/084/085

## 1. 结论

**结论：通过**

本分支完成的三件事均**事实准确、可核验、与已合并的 wave1 评审记录一致**：

- **台账登记正确**：新增 `ISSUE-084`（P2，trace/cli）与 `ISSUE-085`（P3，cli），ID 连续（001–085 共 85 行、无重号）、8 列结构与既有行一致、日期 `2026-10-30`、状态 `open`；ISSUE-084 的触发/影响/建议与 REV-028 C1、spec 06 §6.8.1/§6.8.3、07 §7.3 #6（kill -9 后遗留 run 须 `Failed+interrupted=1`）逐项对应，落点 REV-034（trace 标定 API）+ REV-030（CLI 启动调用/`recovered[]`）与 DR-002 §6 波次表一致。
- **状态更新可回溯**：ISSUE-061→`fixed`（REV-026，main `f89ddaa`）、ISSUE-074→`fixed`（REV-027，main `9137fd6`）、ISSUE-076 部分修（kernel `9ddc539` 已合、harness/CLI 转发留 REV-033/REV-030）、ISSUE-081 扩展清单（实测函数行数与 main 一致，见 §4.3）、ISSUE-082 部分修（REV-027 seam 已合、REV-028 recorded_outputs 待合并——见 §3 #3）。
- **DR-002 修订自洽**：§4 新增 ISSUE-084/085 口径（084 明示**不列入 M6 排除**、wave2 落地；085 留 M6）；§6 wave2 表 cli 行补「启动中止标定/`recovered[]` + `timeout_hint` 转发」、trace 行改 `feat/trace-m5-recovery` REV-034、新增本分支 REV-038；§9 增回写项 10/11/12（崩溃标定、CLI pre-existing 偏差、ISSUE-081 扩展），回写范围仍为 00/04/05/06/07/08，与 §9 表头一致。
- **进度准确**：四块齐全、非空 **19 行（≤30）**；wave1 合并状态（REV-025/026/027/029 已合、REV-028 待 C1）、wave2 模块分工/分支/REV、阻塞与环境说明均与 `git log`/worktree 现状一致（仅 §3 #1 的基线数字需更正：实测 main=`446 passed/2 ignored`，461 是含未合并 cli 分支的计数）。

未发现阻塞/重要项；4 条建议 + 3 条备注 + 1 条 flake 建议见 §3（均为准确性/引用/行序/环境类，需登记或顺手修正，不阻塞本分支）。

## 2. 四维评审

| 维度 | 结论 | 依据（文件/行/命令） |
|---|---|---|
| 功能正确性 | ✅ | ① ISSUE-084/085 内容与 REV-028 C1/建议逐项对应（§4.2）；② 状态变更引用的 commit/REV 全部可在 main 回溯（`f89ddaa`/`9137fd6`/`9ddc539`；REV-026/027 记录在 `doc/reviews/`）；③ ISSUE-081 的剩余 >60 行清单经独立测量 7/7 一致（§4.3）；④ DR-002 §6/§9 的 REV-034/030/033 分工与 wave 表自洽；⑤ progress 的合并状态/分支/REV 与 `git log`、`git worktree list` 一致。仅 §3 #1 基线数字、#2 交叉引用编号、#3/#4 未合并分支措辞需修（不影响 C1 关闭事实）。 |
| 代码结构合理性 | ✅ | 只改 docs 模块三文件；issues.md 保持唯一台账、单表 8 列、ID 连续；DR-002 保持「决策记录 + §9 回写清单」结构；progress 维持四块且 ≤30 行；评审记录为 append-only（REV-025/026 仅追加 §8，不删原文）；未夹带代码/脚本/依赖/CI 变更。 |
| 逻辑性 | ✅ | ISSUE-084 的「未实现 → wave2 实现 → 若滑期转 M6 并保留 spec 注记」闭环完整，且与 REV-028 C1 的关闭条件一一对应；ISSUE-076/081/082 的「部分修 → 剩余项 → wave2/M6」链路与 DR-002 §6/§9 互证；progress 的下一步顺序（关 C1 → cli rebase/复审/合并 → wave2）与当前 main/worktree 状态一致。 |
| 可读性 | ✅ | 新增 ISSUE 行沿用「问题/影响/建议」三段式，关键证据带 commit/REV/章节；DR-002 新增 §4 条目明确「不列入 M6 排除」与落点；progress 当前阶段一句话可读。Nit：DR-002 §6 表 REV-038 行插在 REV-034/035 之间导致编号非单调（§3 #5）；progress 环境说明引用「DR-002/评审记录」不够具体（§3 #7）。 |

## 3. 发现清单

| # | 级别 | 位置 | 问题 | 建议 | 状态 |
|---|---|---|---|---|---|
| 1 | 建议 | `doc/progress.md:24` | 「基线 `cargo test --workspace` **461 passed**/2 ignored（wave1 合并后）」不准确：**461 是含未合并 `feat/cli-m5-run` 的计数**；当前 main `ad7fbec`（wave1 已合：REV-025/026/027/029）实测 **446 passed / 0 failed / 2 ignored**（§4.4）。461 在 cli 合并后才成立 | 改为 `446 passed/2 ignored（main ad7fbec）`，并注明「cli 合并后预计 461」；1 行修正即可 | open |
| 2 | 建议 | `doc/issues.md:93`（ISSUE-085 行）、`doc/decisions/DR-002-m5-cli-replay-recording.md:143`（§9 #11） | 交叉引用编号错位：ISSUE-085 引用「REV-028 建议 **#3**」实为 REV-028 发现 **#2**（human 生命周期；#3 是函数 >60 行）；DR-002 §9 #11 引用「REV-028 **#5**」实为 **#6**（CLI pre-existing 偏差；#5 是 `timeout_hint` 转发） | 修正为 `REV-028 #2` 与 `REV-028 #6`（评审记录是永久引用源，错号会误导后续追踪） | open |
| 3 | 建议 | `doc/issues.md:90`（ISSUE-082 行） | 「**REV-028 已接线** `recorded_outputs`（full/hash/none + hash 复算）」——REV-028 结论为「有条件通过」（C1 未关），其 commit `ad5548a` **尚未在 main**；台账把未合并分支写成「已接线」易被当作已落地 | 改为「REV-028 分支已实现 `recorded_outputs`（full/hash/none + hash 复算），待 C1 关闭合并」；状态保持 `open（部分修）` | open |
| 4 | 建议 | `doc/issues.md:89`（ISSUE-081 行）、`doc/decisions/DR-002...:144`（§9 #12） | 剩余清单中「cli `parse` 79/`run_in_runtime` 70、新增 74 行测试（`run_failed_run_emits_json_error_and_exits_1`）」来自**未合并**的 cli 分支；其余 7 项 main 实测一致（parse_agent_config 113 / validate_schema_node 105 / transport::chat_stream 86 / fake::chat_stream 75 / executor::execute 71 / parse_base_url 64 / execute_tool 63） | 注明「cli 2 处/74 行测试待 REV-028 合并后生效」，避免在 main 上检索不到 | open |
| 5 | Nit | `doc/decisions/DR-002...:108` | §6 波次表新增 REV-038 行插在 REV-034 之后、REV-035/036/037 之前，REV 编号非单调（不影响语义） | 按 REV 升序调整行序，或在表头注明「按 wave/模块排序，非 REV 序」 | open |
| 6 | 备注 | `doc/issues.md:69`（ISSUE-061 行） | 「补 kernel 回归（max_attempts=2/3 边界），e2e `--test parallel` 4/4」准确（实测该 test 二进制 4 用例通过），但易读成「已补 e2e 无幻影断言」；REV-026/DR-002 已约定该断言由 e2e-dev wave2（REV-032）补 | 可补一句「既有 4 用例保持通过；无幻影 attempt 断言待 REV-032」 | open |
| 7 | 备注 | `doc/progress.md:24` | 「共享 `CARGO_TARGET_DIR` 等锁/隔离 merge target 属已知环境操作（见 DR-002/评审记录）」——DR-002 并未记录该环境操作 | 直接引 REV-028/REV-038 或 `doc/development-workflow.md`，便于检索 | open |
| 8 | 备注 | 全 diff | 已核对通过项：ISSUE-061 `f89ddaa`、ISSUE-074 `9137fd6`、ISSUE-076 `9ddc539` 均在 main；07 §7.3 #6 确为 kill -9 崩溃标定检查；ISSUE-084 的 REV-034+REV-030 与 §6 表一致；ISSUE-085 M6 与 DR-002 §4/§9 一致 | 无需动作 | 无需处理 |
| 9 | 建议（环境/测试） | `tests/e2e/parallel.rs:241`（`ac09_parallel_branches_overlap_and_total_elapsed_within_budget`） | 合并门禁首轮在该时序断言失败（load avg≈10.8 时并行 514ms vs 串行 600ms，阈值 <80%；复跑通过）——属**环境负载 flaky**，非本分支回归；每次高负载合并都可能被误阻断 | 登记 P3 flake ISSUE（e2e 模块）：放宽阈值/加 1 次重试/或在 docs 注明高负载环境风险；不改变 AC-09 语义 | open |

> 无阻塞项、无重要项；#1–#4 为准确性/引用修正（建议合并前顺手改或随后 docs 提交），#5–#7 为 Nit/备注，#8 为已核对通过项，#9 为合并门禁时序 flake 的登记建议。

## 4. 验证证据

### 4.1 范围、分支与提交元数据

```bash
cd /Users/zyzs/lczyzs/code/zgraph/.worktrees/process-lead
git status --short                      # 空（评审时 clean）
git branch --show-current               # docs/process-m5-issue-reg
git merge-base main HEAD                # ad7fbec...（= main，基于最新 main）
git rev-parse HEAD                      # d81eefd4976ca955beb4abca0748ef370a3cb898
git diff --shortstat main...HEAD        # 3 files changed, 28 insertions(+), 18 deletions(-)
git diff --name-status main...HEAD      # M doc/{issues,progress}.md + doc/decisions/DR-002-m5-cli-replay-recording.md
git log -1 --format='%H %an <%ae> %s'   # chenyuxiangg；正文含 Review: REV-038 / Agent: lead
```

### 4.2 台账/决策/进度逐项核对

| 项 | 核对结果 |
|---|---|
| `doc/issues.md` | 85 行（ISSUE-001..085）连续无重号；ISSUE-084（P2, trace/cli）/085（P3, cli）各 8 列、日期 2026-10-30、状态 open；ISSUE-061/074 状态 `fixed` 且引 REV-026/027 + commit；ISSUE-076/081/082 保持 open（部分修）并在建议列写明剩余项 |
| ISSUE-084 内容 ↔ 来源 | 与 REV-028 C1、spec 06 §6.8.1（`06:621`）/§6.8.3、07 §7.3 #6（`07:` 检查单第 6 项 = kill -9 后遗留 run `Failed+interrupted=1`）逐项对应；落点 REV-034（trace 标定 API）/REV-030（CLI 启动调用）与 DR-002 §6 表一致 |
| ISSUE-081 函数长度 | 独立脚本测量 main：`parse_agent_config` 113、`validate_schema_node` 105、`transport::chat_stream` 86、`fake::chat_stream` 75、`executor::execute` 71、`parse_base_url` 64、`execute_tool` 63 —— 7/7 与台账一致；cli `parse` 79/`run_in_runtime` 70 与测试 `run_failed_run_emits_json_error_and_exits_1` 74 行在 cli 分支实测一致（待合并，见 #4） |
| `doc/decisions/DR-002` | §4 新增 ISSUE-084/085 两条（084 明示不列 M6 排除、wave2 落地；085 留 M6）；§6 cli 行补标定/`recovered[]`/`timeout_hint` 转发，trace 行改 `feat/trace-m5-recovery` REV-034，新增 REV-038 行；§9 增 #10/#11/#12；回写范围仍 00/04/05/06/07/08（与 §9 表头一致） |
| `doc/progress.md` | 24 行 / 非空 **19 行（≤30）**；四块齐全；模块行/分支/REV 与 `git log`（ad7fbec 含 REV-025/026/027/029、缺 REV-028）和 `git worktree list`（cli + process-lead）一致；仅基线数字需更正（#1） |
| 评审记录补记 | REV-025 §8 追加 dry-run 实跑结果（424 passed/0/2、pytest 84、dry-run exit 0）；REV-026 §8 追加（431 passed/0/2、ISSUE-061 反例 2 FAILED→修复后 3 passed）；两处仅 append，原记录未删除 |

### 4.3 ISSUE-081 剩余清单独立测量（main 代码）

```text
113 lines  parse_agent_config        crates/zgraph-harness/src/config.rs:269
105 lines  validate_schema_node      crates/zgraph-harness/src/tools.rs:863
 86 lines  chat_stream               crates/zgraph-harness/src/llm/transport.rs:70
 75 lines  chat_stream               crates/zgraph-harness/src/llm/fake.rs:174
 71 lines  execute                   crates/zgraph-harness/src/executor.rs:110
 64 lines  parse_base_url            crates/zgraph-harness/src/config.rs:534
 63 lines  execute_tool              crates/zgraph-harness/src/agent_loop.rs:626
```

（cli `parse` 79 / `run_in_runtime` 70 / 74 行测试在 `feat/cli-m5-run` 上测量，命中台账所列值。）

### 4.4 必跑命令与结果（隔离 target）

| 命令 | 结果 |
|---|---|
| `cargo test --workspace -- --test-threads=2`（本分支 = main `ad7fbec` 代码 + docs，先 `cargo clean -p` workspace 成员） | **446 passed / 0 failed / 2 ignored**（2 ignored = harness 真模型 smoke + e2e AC-08b 手动；`/tmp/rev038-workspace.log`）——即 main 当前基线，用于校正 progress 的 461（#1） |
| `bash scripts/dev/check.sh` | **CHECK PASSED**（身份/冻结文件/禁用文件/progress ≤30/fmt/diff-check） |
| `git status --short` | 空（仅本评审记录 + 两处补记，提交后 clean） |

### 4.5 merge.sh dry-run（隔离 target，结论前）

```bash
cd /Users/zyzs/lczyzs/code/zgraph
CARGO_HOME=/Users/zyzs/lczyzs/code/zgraph/.cargo-home CARGO_NET_OFFLINE=true \
CARGO_TARGET_DIR=/tmp/zgraph-merge-target CARGO_INCREMENTAL=0 \
PYTHON_BIN=/Users/zyzs/lczyzs/code/zgraph/.venv/bin/python \
bash scripts/dev/merge.sh docs/process-m5-issue-reg --dry-run
```

- **首轮（21:48，load avg ≈10.8，其它 agent 并行构建/测试）**：评审记录门禁通过；`cargo test --workspace` 在 `zgraph-e2e --test parallel` 的
  `ac09_parallel_branches_overlap_and_total_elapsed_within_budget` 失败：`并行总耗时 514.49ms 应显著小于串行对照 600.05ms（<80%）`（阈值 480ms）→ `MERGE BLOCKED: cargo test 失败`。
  **判定为环境负载导致的时序 flaky，非本 docs 分支回归**：本分支不改代码；同一分支独立 `cargo test --workspace -- --test-threads=2` 全量 **446 passed/0 failed/2 ignored**（§4.4）；该 e2e 用例属 REV-029 已合并内容、本分支未触碰。
- **复跑（attempt 2）**：`INFO: 评审记录 -> 通过` → `INFO: cargo test 通过` → pytest 失败 **0** / 基线 **0**（无新增）→ `INFO: dry-run 结束，检查全部通过`（exit 0）。
- **建议**：为 `ac09_parallel_branches_overlap_and_total_elapsed_within_budget` 登记 P3 flake ISSUE（e2e 模块；放宽/重试该时序断言或在 docs 注明「高负载环境可能 flaky」），避免后续合并门禁被环境噪声阻断（见 §3 #9）。

## 5. 遗留问题

- **建议修正（不阻塞）**：#1 progress 基线 `461→446`（+ cli 合并后预计 461）；#2 ISSUE-085/DR-002 §9 #11 交叉引用改为 `REV-028 #2`/`REV-028 #6`；#3 ISSUE-082 的「REV-028 已接线」改为「分支已实现、待合并」；#4 ISSUE-081/§9 #12 注明 cli 项来自待合并分支。均为一行级文本修正，可由 Lead 在合并前顺手改或随后 docs 提交处理。
- **C1 关闭确认**：本分支已满足 REV-028 C1 的「登记 ISSUE + 决策文档注记」条件（ISSUE-084 + DR-002 §4/§6/§9）；REV-028 合并后由 `reviewer-a` 做增量复审、在原记录追加关闭段并把结论改为通过（不删原记录）。
- **wave2 跟踪**：ISSUE-084（REV-034+REV-030，kill -9 测试）、ISSUE-076 端到端（REV-033/REV-030）、ISSUE-082（llm_calls/版本列，REV-030）、ISSUE-081（剩余函数，M6）。
- **合并门禁 flake**：#9 AC-09 时序用例在机器高负载（load avg≈10.8）下首轮失败、复跑通过；建议 Lead 登记 P3 ISSUE（e2e），否则后续其它分支合并也可能被环境噪声阻断。
- **合并后**：Lead 更新 `doc/reviews/README.md`/`doc/index.md` 的 REV-038 索引（本评审按约束只新增记录，不改索引）。
- 本评审未修改产品代码/文档语义；新增文件为本记录，另按 Lead 指示 append REV-025/026 的 dry-run 补记（append-only）。

## 6. 评审签署

- 范围仅 docs 三文件 + 评审记录补记；ISSUE-084/085 登记、状态更新、DR-002 修订、进度更新均事实准确、可回溯，与 wave1 已合并评审（REV-025/026/027/029）互证；`check.sh` PASS、progress ≤30 行四块齐全；`cargo test --workspace` 446 passed/0 failed/2 ignored（无新增失败）。
- 未发现阻塞/重要项；建议项 #1–#4 为准确性/引用修正，不阻塞本分支。
- **结论：通过**（REV-028 C1 的登记/注记条件据此关闭；REV-028 待合并后另行增量复审）。
- 评审人：`reviewer-a`（非作者）；评审时间：2026-10-30。
