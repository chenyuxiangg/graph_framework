# REV-010 · docs · M2 完成后的进度与遗留问题台账

> **模块声明**：本分支只改 `doc/progress.md`（重写）、`doc/issues.md`（新增 ISSUE-033..059 并更新 ISSUE-042）、`README.md`（1 行 cli 说明）；未触及产品代码/测试/脚本。作者 `lead`，评审人 `reviewer-a`（非作者）。

- **分支**：`feat/docs-m2-progress`
- **作者**：`lead`
- **评审人**：`reviewer-a`
- **变更范围**：`4a15271..b898bcd`（1 commit：`docs(process): M2 完成后的进度与遗留问题台账（REV-010）`；`git diff --stat` = 3 files, +39/−12：`README.md` +1/−1、`doc/issues.md` +27/−0、`doc/progress.md` +11/−11；无其他文件）
- **评审时间**：2026-10-29
- **关联**：AGENTS §5（进度 ≤30 行/四块）/§7（台账规则）；architecture-v2 §6.1（M2 DoD、M3 范围）；spec 00 §0.2/§0.4；spec 01 §1.1.2；spec 04 §4.4.4；spec 06 §6.2.5/§6.2.7/§6.2.12/§6.3.4/§6.4.1/§6.4.2/§6.4.5/§6.8.3/§6.9/§6.11.2；spec 07 §7.1–§7.2；REV-006/007/008/009/011 发现清单

## 1. 结论

**结论：通过**

本提交把 M2 完成后的进度、遗留问题与 README 说明一次性收口，内容与 main 实际一致、台账结构合法、可追溯。关键核对（均实测/程序化校验）：

1. **`doc/progress.md`**：23 行（**18 非空行** ≤30）✓；四块齐全（当前阶段 / 模块表 / 下一步 / 阻塞）✓；`main=4a15271` 正确（= `origin/main`）；状态与 main 实际一致——process/dsl/kernel/trace/cli `done`，worker/harness/e2e `next`；各里程碑 hash 均为 `origin/main` 祖先且主题匹配（见 §4.4）；下一步/阻塞与 architecture M3 范围、ISSUE-047/050/052 自洽（M3 worker 真子进程/进程组 kill/并发/协议异常；spec 回写=ISSUE-050）。
2. **`doc/issues.md`**：程序化校验 59 行台账、`ISSUE-001..059` **编号连续、无重复、无缺号**，最大 = 059（新增 27 条 033..059）；每行 **8 列齐全**；级别 ∈ {P0–P3}、状态 ∈ {open,in-progress,fixed,wontfix}、模块合法（组合标签由合法 id 组成，见建议 #3）；**ISSUE-042 标 `fixed` 且引用 `0deefd9` / `REV-011`** ✓；内容可追溯到 REV-006/007/008/009/011 的具体发现（映射见 §4.5），未发现凭空登记；与 ISSUE-014/015 无冲突（与 ISSUE-020 有重叠，见建议 #2）。
3. **`README.md`**：仅 cli 行 1 行变更（`git diff` 确认），说明 `zgraph` 命令支持 `validate`/`trace`，与全表“反引号代码路径”风格一致；表内未使用超链接，与其余 10 行一致（“可点击”按本表既有风格不适用）。
4. **门禁复习**：`bash scripts/dev/check.sh` = CHECK PASSED（含 `doc/progress.md 非空行 18`、CLAUDE.md 未改、分支命名等 13 项）；`cargo test --workspace` = **249 passed / 0 failed**；pytest = **20 passed**。

发现的 7 项均为**建议级**（台账映射缺口、ISSUE-050/020 重叠、复合模块标签约定、progress 两处措辞、README 里程碑归属等，见 §3），不构成合并条件；需 Lead 确认是否补登/采纳。

## 2. 四维评审

| 维度 | 结论 | 依据（文件/行/实测） |
|---|---|---|
| 功能正确性 | ✅ | 进度状态/hash 与 main 一致；台账编号/列/级别/状态合法且内容可追溯；README 仅目标行更新；check.sh/workspace/pytest 全过（§4）。 |
| 代码结构合理性 | ✅ | progress 四块结构清晰、模块表 8 行覆盖当前活跃模块；issues 追加在表尾不改历史行（仅 ISSUE-042 新行标 fixed）；README 变更最小。 |
| 逻辑性 | ✅ | “下一步”M3 worker 与 architecture §6.1 M3 范围一致；阻塞指向 ISSUE-014/015/020/033–059 并按里程碑推进；台账映射与评审记录对应（缺口见 #1）。 |
| 可读性 | ✅ | progress 18 非空行、无历史流水；issues 行文本含问题/影响/建议/里程碑；README 表述与本表风格一致。建议 #4–#6 为措辞级。 |

## 3. 发现清单

| # | 级别 | 位置 | 问题 | 建议 | 状态 |
|---|---|---|---|---|---|
| 1 | 建议 | `doc/issues.md`（033–059 区间） | 台账与评审发现的对应**不完整**（遗漏少量建议级项）：REV-007 #8（丢弃 `RunHandle` 不自动取消/未文档化）、#9（`start_run` 需 Tokio runtime、无 `Result` 兜底）、#12（builtin start/end 的 gate 在事件循环内求值，D6 纯函数例外需注明）、#14（tests 长函数 >60 行）；REV-008 #10（progress payload 重复 `message`）、#14（tests 长函数）；REV-009 #9（bridge `start_error_does_not_leave_open_writer` 仅断言 Err，建议补 DB/重开断言）。 | 逐条补登，或在 issues.md 增设一条“低优先评审建议汇总（REV-007/008/009）”并在本记录接受；避免发现清单与台账漂移（AGENTS §7）。 | open（建议） |
| 2 | 建议 | `doc/issues.md`：ISSUE-050（与 ISSUE-020）；ISSUE-041 | ① ISSUE-050 与既有 ISSUE-020 同主题（spec 回写）**双开**，虽有“ISSUE-020 的 M0–M2 回写未执行”说明，仍可能两处推进/关闭口径不一；② ISSUE-041 将 REV-008 #9 的“写端未校验 RunFinished 终态/唯一性”改写为“reader 不校验”，与评审原文有偏差。 | ISSUE-050 标注为 ISSUE-020 的 M2 子项（或合并/建立父子关系）；ISSUE-041 对齐 REV-008 #9 原文（写端现状 + 读端自检）或补充说明。 | open（建议） |
| 3 | 建议 | `doc/issues.md` 模块列 | 新增行使用复合模块标签 `kernel/e2e`、`cli/docs`、`docs/e2e`、`trace/cli`、`kernel/cli`，不在 AGENTS §2 模块 id 表内（已有 ISSUE-011 `config/workers` 先例）。 | 在 issues.md 顶部规则注明“跨模块用 `主责/协作` 组合”，或拆行/选主责单模块，保持可机器解析。 | open（建议） |
| 4 | 建议 | `doc/progress.md:23`（阻塞行） | “ISSUE-014/015/020/033–059 按里程碑推进”区间包含**已 fixed 的 ISSUE-042**。 | 改为 `033–041/043–059` 或注明“除已修复 ISSUE-042”。 | open（建议） |
| 5 | 建议 | `doc/progress.md:16,20`；`doc/issues.md` ISSUE-052 | e2e 迁移时序口径不一致：progress e2e 行写“M3–M5”，下一步 #2 把“e2e 迁移（ISSUE-052）”放在 M4/M5 行；ISSUE-052 建议“M3 建立 `tests/e2e/**` 后迁移”。 | 统一为“M3 建立 `tests/e2e/**`；M3–M5 分批迁移验收”，并同步 ISSUE-052 文本。 | open（建议） |
| 6 | 建议 | `README.md:45`（cli 行） | “M2：`validate` / `trace`”把 **M1** 的 `validate` 归入 M2；`run`/`replay` 按 architecture §6.1 属 **M5**（M3/M4 不新增 CLI 子命令），“见 M3–M5”偏宽。 | 改为“M1 `validate` / M2 `trace`；`run`/`replay` 见 M5”（表内代码路径风格保持不变）。 | open（建议） |
| 7 | 备注 | `doc/progress.md:9-13`（done 行的 M0/M1 里程碑与 hash） | 规则要求“只保留当前状态、不写历史”；done 行保留 M0/M1 里程碑名与合并 hash 属“合并点追溯”，与逐条历史流水不同，可接受；后续可只保留与当前阶段相关的 hash。 | 保持现状可接受；若严格按规则，可只留里程碑 + 短 hash。 | open（备注） |

> 无阻塞项、无重要项。核验通过项（编号连续/列数/级别/状态/最大号/042 fixed/哈希祖先/门禁）见 §4；#1–#6 若 Lead 采纳可在合并前或合并后随台账维护提交。

## 4. 验证证据

### 4.1 范围与分支

```bash
cd /Users/zyzs/lczyzs/code/zgraph/.worktrees/docs-lead
git branch --show-current          # feat/docs-m2-progress
git log --oneline -2
# b898bcd docs(process): M2 完成后的进度与遗留问题台账（REV-010）
# 4a15271 docs(review): REV-009 cli M2 trace/桥/验收评审（= origin/main）
git merge-base origin/main HEAD    # 4a15271（基于最新 main，可 ff）
git diff --stat 4a15271..b898bcd   # 3 files changed, 39 insertions(+), 12 deletions(-)
git diff --name-only 4a15271..b898bcd   # README.md / doc/issues.md / doc/progress.md（仅 3 files）
git status --short                 # 空
```

### 4.2 必跑命令

```bash
bash scripts/dev/check.sh
# 13 项 OK，含：user.name/email 合规、SSH remote、CLAUDE.md 未修改、暂存区无禁用文件、
# 分支命名 feat/docs-m2-progress、Python 语法、progress/issues 存在、
# doc/progress.md 非空行 18（目标 ≤30）、cargo fmt --check、git diff --check
# => CHECK PASSED
cargo test --workspace   # 249 passed / 0 failed（与 main 一致，无回归）
python -m pytest         # 20 passed in 0.59s
```

### 4.3 台账程序化校验（`doc/issues.md`）

```text
rows=59；ISSUE-001..059 编号连续、无重复、无缺号；max=ISSUE-059；新增 033..059 = 27 行
每行 8 列齐全（ID/级别/模块/问题/影响/建议/状态/登记日期）；bad_columns=0
级别合法（P0–P3）：bad_levels=0；状态合法（open/in-progress/fixed/wontfix）：bad_statuses=0
新行状态：26 × open + 1 × fixed（ISSUE-042）
ISSUE-042：fixed ✓，引用 0deefd9 ✓、REV-011 ✓
全表引用 ISSUE 编号范围 1..59，无越界引用
模块标签：新增行均为合法 id 或其 `a/b` 组合（历史 ISSUE-008 `monitor`/ISSUE-011 `config/workers` 为既有行，非本分支引入）
```

### 4.4 进度 hash 核对（均为 `origin/main` 祖先且主题匹配）

| progress 行 | hash | commit subject | 结论 |
|---|---|---|---|
| process（REV-006） | `002071d` | docs(review): REV-006 process M2 依赖预热评审 | ✅ 主题匹配 |
| dsl（REV-005） | `9aa0b9e` | docs(review): REV-005 复审通过（34e6cc3） | ✅ 主题匹配 |
| kernel（REV-007） | `4570a53` | docs(review): REV-007 复审增量 | ✅ 主题匹配 |
| trace（REV-008） | `8815370` | docs(review): REV-008 复审增量 | ✅ 主题匹配 |
| cli（REV-009） | `4a15271` | docs(review): REV-009 cli M2 trace/桥/验收评审 | ✅ = `origin/main` |
| 当前阶段 main | `4a15271` | 同上 | ✅ |

### 4.5 台账 ↔ 评审发现映射抽查

| 评审发现 | 台账 | 结论 |
|---|---|---|
| REV-008 #1 单通道 8192 | ISSUE-033 | ✅ |
| REV-008 #2 NodeTimedOut.at | ISSUE-034 | ✅ |
| REV-008 #3 Transition >64KiB | ISSUE-035 | ✅ |
| REV-008 #4 fail-stop 接线 | ISSUE-036（另见 REV-009 #4） | ✅ |
| REV-008 #5 RecordMode | ISSUE-037 | ✅ |
| REV-008 #6 created_seq | ISSUE-038 | ✅ |
| REV-008 #9 RunFinished/终态自检 | ISSUE-041（措辞偏差见 #2） | ✅/建议 |
| REV-008 #11 备份/权限 | ISSUE-039 | ✅ |
| REV-008 #12 内存态 | ISSUE-040 | ✅ |
| REV-007 #4 trace 类型无 serde | ISSUE-051 | ✅ |
| REV-007 #5 subgraph 检测点 | ISSUE-046 | ✅ |
| REV-007 #7 多 end warning | ISSUE-044 | ✅ |
| REV-007 #10 trace_backpressure | ISSUE-045 | ✅ |
| REV-007 风险① EdgeIndex 命名 | ISSUE-043 | ✅ |
| REV-009 #1 运行中 run 提示 | ISSUE-048 | ✅ |
| REV-009 #2 stats-json 截断 | ISSUE-055（另 ISSUE-049） | ✅ |
| REV-009 #3 NodeError JSON 形状 | ISSUE-053 | ✅ |
| REV-009 #5/#6/#7/#8/#10/#11 | ISSUE-047/054/056/058/057 | ✅ |
| REV-011 #1 同步 panic 测试 180s | ISSUE-042（fixed，含“短 timeout 加速”建议） | ✅ |
| REV-007 #8/#9/#12/#14；REV-008 #10/#14；REV-009 #9 | **未登记** | 见建议 #1 |

### 4.6 progress / README 手工核对

| 核对项 | 预期 | 实际 |
|---|---|---|
| 行数 | ≤30 行 | 23 行 / 18 非空 ✅ |
| 四块结构 | 当前阶段/模块表/下一步/阻塞 | 齐全，无历史流水 ✅ |
| 当前阶段与 main | M2 全合并，main=4a15271 | 一致 ✅ |
| 模块状态 | process/dsl/kernel/trace/cli done；worker/harness/e2e next | 一致 ✅ |
| 下一步 vs architecture M3 | 真 worker/进程组 kill/并发上限/协议异常 | 一致（引用 ISSUE-047）✅ |
| 阻塞 vs 台账 | 指向 spec 回写（ISSUE-050）与待推进 ISSUE | 一致（区间含已修 042，见 #4）✅/建议 |
| README 变更范围 | 仅 cli 行 | `git diff` 仅 1 行 ✅ |
| README 表述 | cli 支持 validate/trace | 支持；里程碑归属可再精确（#6）✅/建议 |

## 5. 遗留问题

- **无阻塞项**；建议级 #1–#6 若 Lead 采纳，可在合并后随台账维护提交（或在本记录注明“接受/不登记”）。重点：#1 台账映射缺口（建议逐条补登或加“低优先建议汇总”行）、#2 ISSUE-050/020 双开与 ISSUE-041 措辞、#3 复合模块标签约定。
- **未改动**：本分支不含代码/测试变更；`cargo test --workspace` 249 与 pytest 20 为在 docs 分支上对 main 的复查，无回归。
- **合并后待办（Lead）**：按 AGENTS §5/§7 在 main 生效后维护 progress（本轮已更新）；后续 M3 开工时更新进度与 ISSUE-047/050/052 状态；`doc/reviews/README.md` 索引如需同步由 docs 任务处理。
- 未 push/merge；`main` 未被本分支改动。

## 6. 条件关闭记录

不适用：本记录结论为 **通过**，无前置条件。
