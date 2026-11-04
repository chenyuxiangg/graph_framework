# REV-131 · process/docs · v0.5.0 发布收口（版本 / 性能基线 / spec as-built / 说明 / 进度）

> **评审范围**：发布收口分支（版本 bump + 文档/基线/as-built/README/progress/index/issues/test-baseline）；**未改代码/测试/既有评审记录、未触碰 `dist/`、无 token/PAT**。评审人只新增本评审记录。

- **分支**：`chore/process-v05-release`
- 分支: chore/process-v05-release
- **作者**：`docs-dev`（commit `59e1546`；正文 `Review: REV-131` / `Agent: docs-dev`）
- **评审人**：`reviewer-a`（独立评审，≠ 作者；Lead 第二评审）
- **基于**：main `071f29e`（`main = origin/main = 071f29e`）
- **变更范围**：`071f29e..59e1546`（**22 files, +189/−107**）：`Cargo.toml`/`Cargo.lock` 版本；`README.md`；`doc/progress.md`/`index.md`/`issues.md`/`test-baseline.txt`；新增 `doc/perf-baseline-v0.5.txt`；spec 00/02/05 §5.16/06 §6.16/07 AC-25–28/08 §8.14 + `PROVENANCE.md`/spec README；DR-004 §12、DR-016–020 执行记录
- **评审时间**：2026-11-04
- **关联**：task-128/129；v0.5 F1–F5（REV-116–130）；v0.4 发布范式 REV-113

## 1. 结论

**结论：有条件通过**

**结论：有条件通过（fd00962 复核后仍有 C2a/C3 未关闭）**

P0=0、P1=0；首轮 P2 C1/C2。复核 `fd00962` 后：**C1 已关闭**（index 拆行）；**C2b/C2c/C2d 已关闭**（BUDGET 响应后/07 AC-28 as-built/ISSUE-181 改 open 不虚标）；**C2a 未关闭**（00 as-built banner 仍残留旧设计冻结列表）；**新增 C3（P2/P1 级发布一致性）**：`fd00962` 修改了 spec 00/06/07/08 内容但 **PROVENANCE SHA 未重算**，复算 00/06/07/08 **4/9 MISMATCH**（行数仍 7791/8218），与「9/9 CONSISTENT」声明不符。版本/perf/计数/门禁主体成立；C2a+C3 为 docs 小修，关闭后通过。

### 1.1 已核对成立

| 项 | 结论 |
|---|---|
| 版本/构建 | ✅ `Cargo.toml [workspace.package] version="0.5.0"`；`Cargo.lock` 仅 7 个本地 crate 0.4.0→0.5.0（无依赖漂移）；`cargo build --release -p zgraph-cli --bin zgraph --locked` 成功；`./target/release/zgraph --version` = **`zgraph 0.5.0 (DSL 1.0)`** |
| perf 基线 | ✅ `doc/perf-baseline-v0.5.txt` 与 Lead 实测一致：AC-01 **65/66 −1.5%**、AC-12 **275.1/274.4 +0.3%**、SOP smoke **134/127 +5.5%**、full-60 validate **7.5/7.5 0.0%**（≤20%）；运行期 ISSUE-112 例外；环境/图 hash 齐备；**首轮 5-trial 91 vs 75 +21.3%（含 308ms 离群）已注明**，以 7-trial 为准 |
| spec as-built | ✅ 主体：00 v0.5 码/reason/常量；02 DSL 1.0/19 码/hash 不变；05 §5.16 reexec/预算/sandbox/DSH ACP+Codex FINAL/F5a–f/usage unavailable；06 §6.16 reexec additive/MISSING·TRUNCATED·MISMATCH/预算 metrics/stop_reason/v7 不变；07 AC-25–28 as-built 计数/门控；08 §8.14 final；DR-016–020 执行记录；DR-004 §12；PROVENANCE 第 28 条 |
| PROVENANCE | ✅ 00–08 合计 **7791 行**、全 spec **8218 行**；9 个文件 SHA-256 复算 **9/9 一致** |
| 计数/README | ✅ 实测 workspace **1295/0/6**、e2e **25 target/174/0/5 ignored**、pytest **203/2 skipped**、harness **386/0/1**、cli **270/0/0**；README/progress（14 行）/test-baseline 数字一致；KNOWN limits（F5c/F5f/usage/W3b/ISSUE）标注 |
| issues | ✅ 176/177/178/181 fixed；179/180/163/164 open+notes；182–185 新增（F5c/F5f/版本 pin/外部 usage）；**156–175 行本身未逐项加 v0.5 复核注记**（见 C2） |
| 范围/安全 | ✅ docs + 2 个 Cargo 版本；无 `crates/`/`tests/`/`doc/reviews/` 改动；无 token/PAT；`dist/` 未触碰；check.sh PASSED；tag 仅「执行中」未宣称成功 |

## 2. 四维评审

| 维度 | 结论 | 依据 |
|---|---|---|
| 功能正确性 | ⚠️ 有条件 | 版本/perf/PROVENANCE/计数/门禁正确；C1 index 表格损坏；C2 spec as-built 残留旧设计/F5d 待 + issues 156–175 注记缺失 |
| 代码结构合理性 | ✅ | 发布 docs 分层（README/progress/DR/index/issues/baseline/PROVENANCE/spec as-built）；版本 bump 仅 7 本地 crate；无代码/测试/依赖漂移 |
| 逻辑性 | ✅（C2） | 7-trial 与首轮噪声/门禁自洽；计数与门禁一致；spec 主体 as-built 与实现一致；C2 为旧文案/条目状态残留 |
| 可读性 | ✅（P3） | 发布文档完整；C1 表格粘连/P3 标题明细需修 |

## 3. 发现清单

### 3.1 P2 条件（发布前 docs 小修）

| # | 位置 | 问题 | 建议 |
|---|---|---|---|
| **C1** | `doc/index.md` perf-baseline-v0.5 行 | v0.5 行与 `legacy/` 行粘连（`...已注） || [`legacy/`]...`），Markdown 表格损坏；同时多空行 | 拆成两行（perf v0.5 独立行 + legacy 行），移除多余空行 |
| **C2a** | `doc/spec-v1.2/00-总纲.md:13` | v0.5 as-built banner 尾部拼接了旧设计冻结文本（`...详见 DR-016–020。：① ... ④ ... F5d 实现/真 key 待 ...`），发布 as-built 仍宣称 F5d 待 | 删除尾部旧设计段，保留 as-built（F5d 已 F5d-a 实现、F5c/F5f 门控） |
| **C2b** | `doc/spec-v1.2/00-总纲.md:263` | `BUDGET_EXCEEDED` as-built 仅写「每次 LLM 请求前检查」；F3a2 实际 **请求前 + 每响应后**（最后响应越界也停） | 补「每次响应 usage 累计后检查」/与 05/06 一致 |
| **C2c** | `doc/spec-v1.2/07-验收用例.md:76` | AC-28 矩阵行仍「设计冻结（REV-117）…F5d 实现/真 key 待」；AC-28 as-built 详情已 F5a/F5b/F5d/F5e 实测 | 改为「as-built（REV-131）：F5a/F5b/F5d/F5e 实测；F5c/F5f 门控」 |
| **C2d** | `doc/issues.md` | `ISSUE-181` 状态 fixed 且写「156–175 逐项 fixed/延后/wontfix」，但 156–160/163–166/171–175 行仍 open 且无 v0.5 复核注记 | 逐项补复核状态/注记（fixed/延后/wontfix），或修正 ISSUE-181 文案为「已复核，未逐项改状态」 |

### 3.2 P3（建议）

- **P3-1**：`doc/spec-v1.2/08-审计关闭矩阵.md:651` §8.14 标题仍「设计冻结草案，待实现回写」，但末尾已有 v0.5 as-built 补充；建议改「设计/实现 as-built」。
- **P3-2**：`doc/index.md` reviews 范围已到 131，但行内明细仍停在 REV-113；建议补 114–131 或只保留范围。
- **P3-3**：README「Codex mock 3 e2e，1 skip」表述（F5f 嵌套 skip）可与 AC-28/issue 183 对齐写明「3 用例门控，F5f 环境 skip」。
- **P3-4**：spec 06:1380 v0.4 历史「reexec 未实现 exit2」可加「v0.4 历史；v0.5 见 §6.16」注记（已有 §8.14 类似注记，可选）。

## 4. 验证证据

评审人亲自执行（worktree `.worktrees/process-docs-dev`，HEAD `59e1546`，main `071f29e`，worktree 干净；`CARGO_HOME=<repo>/.cargo-home`）：

```bash
git status --porcelain                     # 空
git diff --stat main...HEAD                # 22 files +189/−107（docs + 2 Cargo 版本）
cargo test --workspace                     # 1295 passed / 0 failed / 6 ignored
cargo test -p zgraph-e2e                   # 25 target / 174 passed / 0 failed / 5 ignored
cargo test -p zgraph-harness               # 386 passed / 0 failed / 1 ignored
cargo test -p zgraph-cli                   # 270 passed / 0 failed
python -m pytest                           # 203 passed / 2 skipped
cargo build --release -p zgraph-cli --bin zgraph --locked  # 成功
./target/release/zgraph --version          # zgraph 0.5.0 (DSL 1.0)
bash scripts/dev/check.sh                  # CHECK PASSED
# PROVENANCE：00–08=7791 行、全 spec=8218 行；9/9 SHA 一致
```

- **perf 逐项**：与 §1.1 一致；首轮 5-trial +21.3% 噪声/7-trial 结论均记录。
- **范围/红线**：22 files；无代码/测试/既有评审改动；无 token/dist；Cargo 仅 7 本地 crate；README/progress/test-baseline 计数与实测一致。
- **issues 抽查**：176/177/178/181 fixed；179/180/163/164 open；182–185；C2d 156–175 复核注记缺失。

## 5. 条件关闭方式

- **C1/C2a–C2d 由 docs-dev 在本分支 docs 小 commit 关闭**：修 index 表格、00 banner/BUDGET 行、07 AC-28 行、issues 156–175 复核注记；重跑 check.sh。
- 关闭后评审人复核 diff/docs grep/计数不变 + check.sh，追加「条件关闭」段、结论改「通过」；随后 Lead merge → dist/tag v0.5.0（PAT 待用户）。

## 6. 条件关闭记录（"有条件通过"）

| 条件 | 状态 | 关闭要求 |
|---|---|---|
| C1 index perf v0.5 行拆行 | **closed（fd00962）** | `doc/index.md` perf v0.5 与 legacy 已拆行；check.sh |
| C2a 00 as-built banner 旧设计列表 | **open（fd00962 未完全删除）** | 00:13 仍有 `...详见 DR-016–020。：① reexec ...` 旧设计段；删除 |
| C2b BUDGET 响应后检查 | **closed（fd00962）** | 00:263 改为「请求前+每响应后；最后响应越界也停」 |
| C2c 07 AC-28 as-built | **closed（fd00962）** | 07:76 改 as-built（F5a/F5b/F5d/F5e 实测；F5c/F5f 门控） |
| C2d issues 156–175/181 | **closed（fd00962，按文案修正方案）** | ISSUE-181 改 open 并明示「多数未关闭、不虚标 fixed」；156–175 行状态与文案一致 |
| **C3 PROVENANCE SHA** | **open（新发现）** | 00/06/07/08 内容已改但 SHA 表未重算：复算 **4/9 MISMATCH**；行数 7791/8218 不变，需重算 4 个 SHA |

## 7. 遗留问题

- C2a/C3 关闭前不发布；P3 已由 fd00962 处理（08 标题/index reviews/README/06 注记）。
- 发布限制已标注：F5c key、F5f 非嵌套、外部 usage unavailable、DSH/Codex pin、W3b→v0.6、ISSUE-179/180/163/164；F6/notes 需保留。
- 无代码/测试改动；版本/perf/计数门禁主体通过。

## 8. 条件关闭复核（fd00962，部分关闭）

复核提交：`fd00962`（父 `f508d85`；docs-only 7 文件 +10/−10）。评审人执行：

```bash
git status --porcelain                 # 空
git diff --stat f508d85..fd00962       # 7 docs files +10/−10（计数不变）
bash scripts/dev/check.sh              # CHECK PASSED
# 复核 index / 00 / 07 / issues / 08 / README / 06
grep -n "perf-baseline-v0.5\|legacy/" doc/index.md   # 已拆行
grep -n "F5d 实现/真 key 待" doc/spec-v1.2/00-总纲.md # 仍命中（C2a 未清）
# SHA 复算（fd00962 后）
for f in doc/spec-v1.2/0*.md; do ... done
# => 00/06/07/08 MISMATCH；01/02/03/04/05 OK（9/9 不再成立，C3）
```

| 项 | 复核结果 |
|---|---|
| C1 index | ✅ perf v0.5 行与 legacy 行已拆，表格恢复 |
| C2b/C2c/C2d | ✅ BUDGET 响应后、07 AC-28 as-built、ISSUE-181 open/不虚标 |
| C2a | ❌ 00 as-built banner 尾部仍保留 `：① ...` 旧设计列表（仅改了 F5d 待措辞，未删旧段） |
| C3 | ❌ 00/06/07/08 内容变动后 PROVENANCE SHA 未重算；复算 4/9 MISMATCH；行数仍 7791/8218 |
| 计数/门禁 | ✅ docs-only 计数不变；check.sh PASSED；版本/perf 未变 |

**当前状态：C2a/C3 open，结论保持「有条件通过」；需 docs-dev 再一个小 commit（删 00 旧段 + 重算 00/06/07/08 SHA）。**
