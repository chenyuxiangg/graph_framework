# REV-113 · process/docs · v0.4.0 发布收口（版本 / 性能基线 / spec as-built / 说明 / 进度）

> **评审范围**：发布收口分支，版本 bump + 文档/基线/as-built + README/progress/index/issues/test-baseline；**未改代码/测试/既有评审记录、未触碰 `dist/`、无 token/密钥**。评审人只新增本评审记录，未改实现/文档。

- **分支**：`chore/process-v04-release`
- 分支: chore/process-v04-release
- **作者**：`docs-dev`（提交 `f3f5d40`；正文 `Review: REV-113` / `Agent: docs-dev`）
- **评审人**：`reviewer-a`（独立评审，≠ 作者）
- **基于**：main `8897053`（`main = origin/main = 8897053`）
- **变更范围**：`8897053..f3f5d40`（**20 files, +299/−169**）：`Cargo.toml`/`Cargo.lock`（版本）、`README.md`、`doc/progress.md`、`doc/index.md`、`doc/issues.md`（ISSUE-176–181）、`doc/test-baseline.txt`、新增 `doc/perf-baseline-v0.4.txt`、`doc/decisions/DR-004/012/013/014` 执行记录、spec 00/02/05/06/07/08 as-built + `PROVENANCE.md`/spec `README.md`
- **评审时间**：2026-11-03
- **关联**：task-91/92；DR-012/013/014、DR-004 §11、spec 00/02/05 §5.15/06 §6.15/07 AC-21–24/08 §8.13；REV-104（v0.3 范式）、REV-112（D5c）

## 1. 结论

**结论：有条件通过**

P0=0、**P1=0**；**P2 条件 C1**（发布文档中的 harness 测试计数 339 vs 实测 **340**，5 处引用；属发布文档准确性，D6 需修正后再 merge）；P3 若干（spec 00 reason 闭集补全/CLI 码描述、index reviews 细节、README 标题状态）。版本/锁/ release `--locked`/`--version`、perf 基线（5-trial 与 Lead 逐项一致）、spec as-built/PROVENANCE 计数与 SHA、README/progress/issues/test-baseline、范围与红线主体均成立。

### 1.1 已核对成立

| 项 | 结论 |
|---|---|
| 版本 | ✅ `Cargo.toml [workspace.package] version="0.4.0"`；`Cargo.lock` 仅 7 个本地 crate 0.3.0→0.4.0（cli/dsl/e2e/harness/kernel/trace/worker），无依赖漂移；`cargo build --release -p zgraph-cli --bin zgraph --locked` 成功；`./target/release/zgraph --version` = `zgraph 0.4.0 (DSL 1.0)` |
| 性能基线 | ✅ `doc/perf-baseline-v0.4.txt` 与 Lead 实测逐项一致：AC-01 **65/65 +0.0%**、AC-12 **223.7/225.2 −0.7%**、SOP smoke **102/102 +0.0%**、full-60 validate **6.4/6.0 +6.7%**（≤20% 门禁）、运行期 ISSUE-112 例外；环境（CARGO_HOME/.venv/`--python-path python`）与三个图 hash 齐备；**首轮 3-trial warm 噪声 +17.6% 已注明**，以 5-trial 为准 |
| spec as-built | ✅ 00 §0.4 reason/CLI 码 as-built + §0.5 常量；02 不进 DSL/hash；05 §5.15 八工具/opt-in/工具源/jail/OS 沙箱/错误面/上限最终态；06 §6.15 `tool_calls` 最终 DDL（含 `error_sha256`/`bash_signal`/`sandbox_profile_sha256`）、`(started_at,call_id)` 排序、`TraceInput::ToolCall` 非 events、human/NDJSON additive、`--stats>0`、`--mode=tools` recorded/reexec exit2/CLI 码；07 AC-21–24 **29 用例/5 target** 实测映射；08 §8.13 final source-breaking（依赖/CLI flags/v7/reasons/CLI 码/无新 kind）；DR-012/013/014 执行记录；DR-004 §11 发布记录 |
| 计数/PROVENANCE | ✅ 实测 e2e **23 target / 166 passed / 0 failed / 2 ignored**（v0.4 五 target **10+4+3+9+3=29**）；workspace **1216/0/3**；pytest **183**；**harness 实测 340/0/1（文档写 339，见 C1）**；PROVENANCE 行数 **00–08=7640 / 全 spec=8055**、9 个文件 SHA-256 逐一复算 **9/9 一致** |
| README/progress/index/issues | ✅ README v0.4.0（M0–M9/CLI 示例/性能/已知限制/发布状态「执行中」）；progress **14 行**四块、未宣称 tag 成功；index 增 perf 条目/reviews 至 REV-113；issues **ISSUE-176–181**（kernel_cancel/ac09/codex flake、trace 无 FK、真实终端验证、v0.3 残余）；test-baseline e2e 23/166/2 |
| 范围/红线 | ✅ docs-only（20 文件：2 个 Cargo 版本 + 18 doc）；无 `crates/`/`tests/`/`doc/reviews/` 改动；无 token/密钥；未触碰 `dist/`；spec 只回写/追加；既有评审记录未改 |

## 2. 四维评审

| 维度 | 结论 | 依据 |
|---|---|---|
| 功能正确性 | ⚠️ 有条件 | 版本/发布门禁/perf/spec/PROVENANCE/README 主体正确；**C1 发布文档计数 harness 339 vs 实测 340**；P3 as-built 文档细节（00 reason 闭集/CLI 码描述/index 细节） |
| 代码结构合理性 | ✅ | 发布分支职责单一；版本 bump 只 7 本地 crate；docs 分层（README/progress/DR/index/issues/baseline/PROVENANCE/spec as-built）；无代码/测试/依赖漂移 |
| 逻辑性 | ✅ | perf 数字/百分比/门禁自洽（6.4/6.0=+6.7% 且 ≤20%）；5-trial 与 warm 噪声两轮数据明示；PROVENANCE 行数/SHA 与文件一致；v0.4 29 用例由 10+4+3+9+3 自洽；166=137+29；默认路径/兼容红线明确 |
| 可读性 | ✅（P3 备注） | 发布文档完整可导航；P3：README 标题「已发布」vs tag 执行中、index reviews 细节停在 104、spec 00 CLI 码描述需补全 |

## 3. 发现清单

### 3.1 P2 条件（D6 内修正后再 merge）

| # | 位置 | 问题 | 建议 |
|---|---|---|---|
| **C1** | `README.md:27`、`doc/spec-v1.2/07-验收用例.md:605`、`doc/spec-v1.2/PROVENANCE.md:267`、`doc/decisions/DR-013-v04-builtin-tools-jail.md:334`、`doc/test-baseline.txt:10` | **harness 测试计数写 `339/0/1`，评审实测 `cargo test -p zgraph-harness`（默认与 `--features mcp`）均为 `340 passed / 0 failed / 1 ignored`**；workspace 1216 与 340 自洽（339 会少 1）。发布文档/验收 DoD/台账计数错误 | 五处统一改 **340/0/1**（README、07 DoD、PROVENANCE §2 第 27 条、DR-013 §12、test-baseline 末行）；重跑 check.sh + `cargo test -p zgraph-harness` 断言；不涉及代码 |

### 3.2 P3（建议，D6 可同批或发布后）

- **P3-1 spec 00 reason 闭集不完整**：`00-总纲.md` v0.4 as-built 的 `TOOL_ERROR{reason}` 列表未含实际存在的 `path_too_long`/`not_regular_file`/`unsupported_patch`/`io_error`/`unknown_builtin_tool`（代码确有，grep 已核）；建议补全或明确「列表为示例、完整 reason 见 05 §5.15.4」。
- **P3-2 spec 00 `REPLAY_TOOL_RECORD_MISMATCH` 描述**：写「reexec 漂移，v0.5」，但 v0.4 已用该码表示 args/result/error JSON 非法/sha 不符（06 §6.15.4 正确）；建议改为「记录截断/JSON 非法/sha 不符（v0.4）；reexec 漂移（v0.5）」。
- **P3-3 `doc/index.md` reviews 细节**：范围已到 REV-113，但行内明细仍停在 REV-104；建议补 105–113（或只保留范围，避免误导）。
- **P3-4 README 标题「v0.4.0 已发布」**：同段已注明 tag「发布流程执行中，用户授权后创建」；建议标题改「发布收口」或在 tag 创建后回写「已发布」，避免提前宣称。
- **P3-5 spec 07 AC-21–24 行引用「REV-111/112 通过」**：AC 实现评审为 REV-106–111、D5c e2e 为 REV-112；建议写清「实现 REV-106–111 / e2e REV-112」。
- **P3-6 作者报告的一次 workspace 1 failed/424 passed 未捕获用例名**：本次评审 workspace **1216/0/3 未复现**；ISSUE-176 已登记；发布前无需阻塞，release notes 保留 flake 说明。

## 4. 验证证据

评审人亲自执行（worktree `.worktrees/process-docs-dev`，HEAD `f3f5d40`，main `8897053`，worktree 干净；`CARGO_HOME=<repo>/.cargo-home`）：

```bash
git status --porcelain                     # 空
git diff --stat main...HEAD                # 20 files +299/−169（2 Cargo 版本 + 18 doc）
cargo test --workspace                     # 1216 passed / 0 failed / 3 ignored（未复现作者 1 failed/424）
cargo build --release -p zgraph-cli --bin zgraph --locked   # 成功
./target/release/zgraph --version          # zgraph 0.4.0 (DSL 1.0)
cargo test -p zgraph-harness               # 340 passed / 0 failed / 1 ignored（文档写 339 → C1）
cargo test -p zgraph-harness --features mcp # 340 passed / 0 failed / 1 ignored
cargo test -p zgraph-e2e                   # 23 target / 166 passed / 0 failed / 2 ignored
cargo test -p zgraph-e2e --test builtin_files_v04 --test builtin_search_v04 \
  --test builtin_bash_v04 --test trace_tools_v04 --test v04_regression
# 10 + 4 + 3 + 9 + 3 = 29 passed（5 target）
python -m pytest                           # 183 passed
bash scripts/dev/check.sh                  # CHECK PASSED
# PROVENANCE 复算：00–08=7640 行、全 spec=8055 行；9 个文件 SHA-256 9/9 与表一致
```

- **perf-baseline-v0.4.txt 实读**：与 Lead 逐项一致（见 §1.1）；AC-01/AC-12/SOP smoke 门禁 ≤20% 通过；full-60 validate +6.7% 在门禁内；运行期 ISSUE-112 例外；warm 噪声两轮均记录。
- **范围/红线**：`git diff --name-only` 全部 docs/Cargo 版本；无 `dist/`/token/代码/测试/既有评审；README/progress/index/issues/baseline/PROVENANCE/spec as-built 均已核对。
- **对比自报**：版本/`--version`/perf/e2e 166/23/2/29 用例/workspace 1216/pytest 183 与自报一致；harness 340 vs 自报 339（C1）。
- **PROVENANCE 9/9**：`shasum -a 256` 逐文件与 §3 表一致；行数 7640/8055 与 00–08/全 spec `wc -l` 一致。

## 5. 条件关闭方式

- **C1 由 docs-dev 在 D6 内 docs 小 commit 关闭**：五处 339→340（README/07 DoD/PROVENANCE/DR-013 §12/test-baseline），重跑 check.sh；不涉及代码/测试。
- 关闭后评审人只读复核五处 `grep 339` 清零 + 重跑 `cargo test -p zgraph-harness`（340/0/1）与 check.sh，追加「条件关闭」段、结论改「通过」；随后 Lead merge → dist/tag/GitHub Release。
- P3-1–P3-5 可同批 docs 或 release notes 后补；P3-6 已登记 ISSUE-176，不阻塞。

## 6. 条件关闭记录（"有条件通过"）

| 条件 | 状态 | 关闭要求 |
|---|---|---|
| C1 harness 计数 339→340（5 处） | **open** | README/07 DoD/PROVENANCE/DR-013/test-baseline 统一 340/0/1；重跑 check.sh |

## 7. 遗留问题

- C1 关闭前不 merge；P3-1/P3-2/P3-3/P3-4/P3-5 文档细节可同批修正；P3-6 flake 已登记 ISSUE-176（本次未复现）。
- e2e 166/23/2 与 v0.4 29 用例实测；workspace 1216/0/3、pytest 183 实测；release `--locked`/`--version` 实测；PROVENANCE 7640/8055 9/9 实测一致。
- 跨模块（版本 Cargo 为 process 模块；主导 docs）由 Lead 追加第二评审；未改 kernel/代码/测试。
