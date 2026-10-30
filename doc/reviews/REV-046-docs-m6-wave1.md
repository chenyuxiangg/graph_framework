# REV-046 · docs · M6 W1 收口：台账/进度/评审索引/README/baseline 与 REV 编号顺延

> **模块声明（跨模块）**：本分支改 `doc/**`（`docs` 模块：DR-003、progress、issues、index、reviews/README、test-baseline）与根 `README.md`（`process` 模块，作者 `lead` 为 process 负责人），按 AGENTS §2 属**跨模块**改动；由**非作者**评审人 `reviewer-a` 在本记录作独立第二评审并声明。评审人**未修改任何被评审内容**，只新增本记录；未 merge / 未 push / 未改 main。
>
> **评审对象**：`docs/docs-m6-wave1` @ `c234e35`（作者 `lead`，1 commit；base = `origin/main` = `67bc49b`，即 W1 七分支合并后的 main）。

- **分支**：`docs/docs-m6-wave1`
- **作者**：`lead`
- **评审人**：`reviewer-a`（**非作者**）
- **变更范围**：`origin/main`（`67bc49b`）..`c234e35`，**1 commit / 7 files changed, 70 insertions(+), 54 deletions(-)**：`README.md`、`doc/decisions/DR-003-m6-scope-and-waves.md`、`doc/index.md`、`doc/issues.md`、`doc/progress.md`、`doc/reviews/README.md`、`doc/test-baseline.txt`；纯文档，无代码/测试/配置/CI/spec 正文/`CLAUDE.md` 改动。
- **评审时间**：2026-10-31
- **关联**：AGENTS §2/§3/§5/§6/§7；`doc/decisions/DR-003-m6-scope-and-waves.md` §1/§2/§3/§4（编号顺延）；`doc/issues.md` ISSUE-033/059/068/073/075/077/078/086/089 与新增 ISSUE-090..093；`doc/reviews/REV-039..045`；main `67bc49b`（W1 七分支合并后）

## 1. 结论

**结论：有条件通过**

W1 七分支（REV-039–045）的收口事实**基本正确**：9 个 W1 ISSUE 的 `fixed`/`wontfix` 状态与 REV 映射（033→041、059→043、068/073/078→045、075/077→042、086→040、089→044）与已合并 main 的对应分支/评审记录**一致**；新增 ISSUE-090..093 编号无重复（001–093 连续无缺号）、8 列表格列数正确，且 090/092/093 均能在 REV-041/042 记录或 M6 合并实操中找到出处；REV-039..045 索引行与 `doc/reviews/` 文件一一对应、结论/评审人与记录一致；DR-003 §2 的 REV-039–058 **连续无重号/无缺号**，§1↔§2 波次映射一致（抽查 036/082/087/086/088 全部对应新编号）；progress 23 行（非空 18）≤30、四块齐全；README/index/baseline 链接全部有效；`check.sh` PASS、worktree clean。

但**REV 编号顺延有 4 处残留**（同一 commit 内新旧编号并存），会让 W2/W3 派工指向错误分支，必须修正后才可合并：

- **C1（重要，编号一致性）**：
  1. `doc/issues.md:83`（ISSUE-075）：spec 05 §5.8.5 回写「随 **REV-056**」→ 应为 **REV-057**（DR-003 §2 `docs/docs-m6-writeback`）。
  2. `doc/issues.md:94`（ISSUE-086）：「e2e 实例级断言转 **REV-054**」→ 应为 **REV-055**（DR-003 §2 `test/e2e-m6-final-asserts`；REV-054 = cli split）。
  3. `doc/issues.md:100`（ISSUE-092）：「**REV-056** 回写 06 §6.4.1」→ 应为 **REV-057**。
  4. `doc/index.md:18`：「M6 计划至 **REV-057**」→ 应为 **REV-058**（DR-003 §2 末行 `docs/process-m6-wrapup` = REV-058；`doc/reviews/README.md:62` 与 progress 均已写 058）。
  **关闭要求**：改上述 4 处文本；建议同时在 `doc/reviews/README.md` M6 段落加一行「REV-040–044 记录内对 W2/W3 的 REV 引用（如 REV-054/056）为编号顺延前口径，以 DR-003 为准」（记录本身不可改，见 #3）。

其余为**建议/备注**（§3 #2–#6），不阻塞：main 不可达的 commit 短哈希（#2）、记录内旧编号注记（#3）、ISSUE-030 可顺势收口（#4）、索引日期口径（#5）、ISSUE-093 证据来源（#6）。

## 2. 四维评审

| 维度 | 结论 | 依据（文件/行/命令） |
|---|---|---|
| 功能正确性 | ⚠️ | ① 9 个 W1 ISSUE 状态/REV 映射与 main 合并事实一致（§4.2/§4.3）：`git merge-base --is-ancestor` 证实 7 个 W1 分支均已在 `67bc49b`；对应 REV-040–045 记录结论均为「通过」。② 但台账/索引 4 处残留旧 REV 编号（C1），会使后续 W2/W3 工作指向错误评审编号——收口任务的核心目标是编号顺延，故扣分。 |
| 代码结构合理性 | ✅ | 纯文档收口：7 个文件各改其位（台账/进度/索引/评审索引/README/baseline/DR-003 编号顺延），未夹带代码/测试/配置/spec 正文；REV-046 记录按流程落在本分支 `doc/reviews/`；跨模块（doc + 根 README）已在本记录声明并由非作者第二评审，符合 AGENTS §2。 |
| 逻辑性 | ✅ | ① DR-003 §2 的 REV-039–058 经脚本核对：**连续 20 个、无重号、无缺号**，wrapup 插入为 REV-046 后 W2/W3 整体 +1，§1 各 ISSUE 的波次/REV 与 §2 分支表逐项一致（036→047、082→048、087→049、086→040+e2e 055、088→056、073/075/077/078 等）。② progress 的 W2/W3 编号与 DR-003 一致；W1 done、W2/W3 next、阻塞含 ISSUE-091/093 环境口径。③ 残留旧编号属内容一致性问题（C1），非模型/逻辑错误。 |
| 可读性 | ✅ | 收口文档结构清晰：progress 四块 23 行（非空 18）≤30；台账行均为 8 列、状态域合法（open/in-progress/fixed/wontfix）、无重复 ID；REV 索引行按编号升序、链接全部可达；baseline 注记格式与 M3–M5 一致。Nit：索引 M6 行日期口径（#5）。 |

## 3. 发现清单

| # | 级别 | 位置 | 问题 | 建议 | 状态 |
|---|---|---|---|---|---|
| 1 | **重要（C1）** | `doc/issues.md:83,94,100`；`doc/index.md:18`（对照 `doc/decisions/DR-003-m6-scope-and-waves.md:55,56,57,58,59`、`doc/progress.md:18-20`、`doc/reviews/README.md:62`） | REV 编号顺延（W1 收口=REV-046，W2/W3=REV-047–058）后仍有 4 处旧编号：ISSUE-075「回写随 REV-056」、ISSUE-086「e2e 断言转 REV-054」、ISSUE-092「REV-056 回写 06 §6.4.1」、index「M6 计划至 REV-057」；与 DR-003 §2（writeback=057、e2e-final=055、wrapup=058）矛盾 | 4 处分别改为 **REV-057 / REV-055 / REV-057 / REV-058**；并在 reviews README M6 段落加一行注记「记录内 REV-054/056 为顺延前口径，以 DR-003 为准」（见 #3） | open（条件） |
| 2 | 建议（可核验性） | `doc/issues.md:41,67,76,83,85,86,94,97`（8 处 W1 ISSUE 提交短哈希；ISSUE-073 行未引哈希） | 台账引用的 `e7b144d`（033）、`a995fb3`（059）、`4096853`（068/078）、`c2958f6`（075/077）、`e9426bd`（086）、`75c4f70`（089）**不是 main 历史祖先**（`git merge-base --is-ancestor <hash> 67bc49b` 全为 false）；main 上实际合并提交为 `0934714`/`eb59983`/`c48e878`/`b64d61e`/`cecd015`/`8886dfd`（merge.sh rebase 导致哈希改写）。台账在 main 上无法直接 `git show` 这些哈希 | 将 6 个短哈希替换为 main 可达提交（或统一改为「分支 + subject」引用）；该问题会反复出现，建议在 REV-058 wrapup 固定口径（或 merge.sh 注明 rebase 后哈希映射） | open（不阻塞） |
| 3 | 建议（记录口径） | `doc/reviews/REV-040..044`（记录内建议栏） | 已合并的 REV-040–044 记录中「由 docs 回写（REV-056）」「转 REV-056」等引用为编号顺延**前**口径（现为 REV-057）；记录不可删改，但后续读者可能误读 | 按 C1 建议在 `doc/reviews/README.md` M6 段落加一行「REV-046 起编号顺延，记录内旧编号以 DR-003 为准」；不改动既有记录正文 | open（不阻塞） |
| 4 | 建议（台账完整性） | `doc/issues.md:38`（ISSUE-030） | ISSUE-030 的建议动作「如需收敛由 process 统一提升到 `workspace.dependencies`」已由 REV-045 实际完成（`blake3`/`nix` 均已提升，见 ISSUE-068/078 fixed），但 ISSUE-030 仍 `open` 且未加注记；与本分支对 ISSUE-072 的「**已由 REV-045 提升 workspace.dependencies 处理**」注记口径不一致 | 为 ISSUE-030 加「blake3 提升已由 REV-045 完成」注记（置 `fixed` 或保持 open + 注记均可，需与 DR-003 D12「030 留 v0.2」口径一致并注明取舍） | open（不阻塞） |
| 5 | Nit（日期口径） | `doc/reviews/README.md:45,51` | REV-039/REV-045 索引行日期写 `2026-10-31`，而两条记录内「评审时间」为 `2026-10-30`（REV-040–044 两者一致为 10-31）；索引「日期」列口径不统一 | 统一为评审日（改 039/045 为 2026-10-30）或在索引表头注明「合并日」口径；不阻塞 | open（不阻塞） |
| 6 | 备注（证据来源） | `doc/issues.md:101`（ISSUE-093） | 「共享 `CARGO_TARGET_DIR` 跨 worktree 串产物」为 M6 合并实操观察（REV-041 记录 §5 提到「由 Lead 在 M6 wrapup 记录」），仓库内无独立证据/复现脚本；描述具体且已写处置（私有 target/clean/串行） | 可接受为环境问题登记；建议 REV-058 wrapup 附最小复现或引用本次 merge 日志，便于后续复核关闭 | open（不阻塞） |

> **条件**：C1（4 处 REV 编号）关闭后由 reviewer-a 增量复审（本记录 §6 追加关闭证据）即可转「通过」。建议项 #2–#6 可修复或留 REV-058 处理，不阻塞。

## 4. 验证证据

### 4.1 范围、分支与门禁（worktree `.worktrees/docs-lead-wave1`，实际执行）

```bash
$ git status --porcelain                 # 空（worktree clean）
$ git branch --show-current              # docs/docs-m6-wave1
$ git rev-list --count origin/main..HEAD # 1
$ git merge-base origin/main HEAD        # 67bc49bdf5368cbab0b557886ae211b6ba349f96 = origin/main
$ git diff origin/main..HEAD --name-status
# M README.md / M doc/decisions/DR-003-… / M doc/index.md / M doc/issues.md /
# M doc/progress.md / M doc/reviews/README.md / M doc/test-baseline.txt（7 文件，无代码/测试/配置）
$ git diff origin/main..HEAD --stat
# 7 files changed, 70 insertions(+), 54 deletions(-)
$ bash scripts/dev/check.sh
# OK 身份/remote/CLAUDE.md 未修改/暂存区无禁用文件；OK 分支命名: docs/docs-m6-wave1
# OK doc/progress.md 非空行 18（目标 ≤30）；OK cargo fmt --check；OK git diff --check
# CHECK PASSED（exit 0）
$ wc -l doc/progress.md                  # 23 行（非空 18）
```

### 4.2 W1 收口事实核对（台账 ↔ main 提交/评审记录）

main = `67bc49b`；7 个 W1 分支均已合入（`git log --oneline 67bc49b` 可见 REV-039–045 对应 commit）：

```text
REV  分支/提交（main 上）                              记录结论  评审人
039  docs/process-m6-kickoff                           通过      reviewer-a
040  fix/kernel-m6-timeout-error  → cecd015            通过      lead（≠ 作者 kernel-dev）
041  fix/trace-m6-dual-channel    → 0934714            通过      lead（≠ 作者 trace-dev）
042  fix/harness-m6-preview-…     → b64d61e            通过      lead（≠ 作者 harness-dev）
043  fix/cli-m6-exit-codes        → eb59983            通过      lead（≠ 作者 cli-dev）
044  test/e2e-m6-quality-ac12     → 8886dfd            通过      lead（≠ 作者 e2e-dev）
045  chore/process-m6-deps        → c48e878            通过      reviewer-a（≠ 作者 lead）
```

- 9 个 W1 ISSUE 状态与 REV（逐行核对 `doc/issues.md`）：
  `033 fixed REV-041`、`059 fixed REV-043`、`068 fixed REV-045`、`073 wontfix REV-045`（D2 口径）、`075 fixed REV-042`、`077 fixed REV-042`、`078 fixed REV-045`、`086 fixed REV-040`、`089 fixed REV-044` —— 状态/REV 与上述分支/记录一致。✅
- 台账提交哈希为**分支提交（rebase 前）**，不是 main 可达对象（见 #2）：`e7b144d/a995fb3/4096853/c2958f6/e9426bd/75c4f70` 均 `is-ancestor=false`。⚠️（不影响状态/REV 正确性，但影响 main 上直接核验）

### 4.3 新增台账与编号（脚本核对）

```text
doc/issues.md：93 行数据行；ID 001–093 连续、无重复、无缺号；每行 8 列；级别 ∈ {P0..P3}、状态 ∈ {open,in-progress,fixed,wontfix}
新增：ISSUE-090（P3 harness，REV-042 记录「建议 ISSUE-090」出处 ✓）、ISSUE-091（P3 worker/e2e，测试名
  executor_m3::branches_run_concurrently_within_limits / kernel_cancel_kills_worker_process_group 均存在，
  `sleep: 0.8` 重叠窗口描述属实 ✓）、ISSUE-092（P3 docs，REV-041 记录「跨发送端」建议出处 ✓）、
  ISSUE-093（P2 process，REV-041 记录 §5 已提「共享 CARGO_TARGET_DIR 串产物由 M6 wrapup 记录」✓）；登记日期均 2026-10-31 ✓
```

REV 编号顺延（DR-003 §2 脚本提取，20 行）：`39,40,…,58` —— **连续、无重号、无缺号**；wrapup=`REV-058`、e2e-final=`REV-055`、writeback=`REV-057`。§1 抽查：036/082/087/086/088 = REV-047/048/049/040(+055)/056，与 §2 一致。✅（台账/index 的 4 处旧编号除外，见 C1）

### 4.4 评审索引与链接

```text
doc/reviews/README.md：新增 REV-039–045 共 7 行；索引内 REV-*.md 链接 45 条，全部存在；
  doc/reviews/ 下 REV-*.md 文件全部在索引中（无遗漏/无死链）；结论/评审人与记录正文逐条一致。
M6 段落（README.md:62）：写「W1 已合并 REV-039–045 …… M6 计划至 REV-058（W1 收口 REV-046；W2/W3 …）」，
  REV-037 未创建表述正确；与 main 事实（7 分支已合并）一致。✅
doc/index.md、README.md、doc/progress.md、doc/issues.md、doc/test-baseline.txt 的相对链接扫描：全部可达。✅
```

### 4.5 抽查证据（台账「已修」描述与代码一致）

- ISSUE-033（REV-041）：`STREAMING_CHANNEL_CAP=1024` 常量存在（`crates/zgraph-trace/src/lib.rs:32,105`、`writer/mod.rs:8`）；`crates/zgraph-trace/tests/channels.rs` 含 7 个用例 —— 与台账「7 用例」一致。✅
- ISSUE-089（REV-044）：`tests/e2e/Cargo.toml:9` description 已更新为「M6 e2e 验收包…」；`tests/e2e/cli_support/mod.rs:142-151` 注入 `PYTHONDONTWRITEBYTECODE=1` —— 与台账描述一致。✅
- ISSUE-086（REV-040）：台账称 `fail_timeout()` 迁移前补 `set_instance_error(NodeError::Timeout{elapsed_ms})` 且补 kernel 单测；REV-040 记录结论通过、范围 2 文件（run.rs + timeout.rs）。✅

## 5. 遗留问题

- **未关闭条件**：C1（4 处 REV 编号残留，见 §1/§3 #1）；关闭后由 reviewer-a 在 §6 追加关闭记录并改结论为「通过」。
- **建议项**：#2 main 不可达 commit 哈希（可核验性）、#3 记录内旧编号注记、#4 ISSUE-030 注记、#5 索引日期口径、#6 ISSUE-093 证据来源——均不阻塞；建议 #2/#3 在 REV-058 wrapup 固定口径。
- 本记录只新增评审文件；未修改 DR-003/台账/进度/索引/README/baseline，未 merge/push。

## 6. 条件关闭记录（仅“有条件通过”填写）

| 条件 | 关闭要求（可执行） | 关闭证据（commit/行） | 关闭日期 |
|---|---|---|---|
| C1 | `doc/issues.md:83` → REV-057；`:94` → REV-055；`:100` → REV-057；`doc/index.md:18` → REV-058（并在 reviews README M6 段落加旧编号注记一行） | 待补 | 待补 |
