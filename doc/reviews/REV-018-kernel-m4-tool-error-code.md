# REV-018 · kernel · 工具错误码统一为 TOOL_ERROR（ISSUE-070）

> **模块声明**：本分支只改 `crates/zgraph-kernel/**` 两个文件（`src/types.rs` 常量值 + 注释；`tests/failure.rs` 断言），符合 AGENTS §2 `kernel` 模块边界。作者 `lead`，评审人 `reviewer-b`（**非作者**）。本次评审**未修改任何代码/测试/台账**，只新增本记录；未 commit / 未 merge / 未 rebase。
>
> **独立核对口径**：本人亲自在 worktree 内看 `git diff` 全文、全仓 grep、跑 `cargo test -p zgraph-kernel`（默认 + all-features）与 `check.sh`，不采信提交自述。

- **分支**：`fix/kernel-m4-tool-error-code`
- **作者**：`lead`
- **评审人**：`reviewer-b`
- **变更范围**：`887743a..d4108a2`（1 commit `d4108a2 fix(kernel): 工具错误码统一为 TOOL_ERROR`；`git diff --shortstat` = **2 files changed, 3 insertions(+), 3 deletions(-)**；base = merge-base = `origin/main` = `887743a`，**未 rebase**）
- **评审时间**：2026-10-30
- **关联**：spec 00 §0.3（`HarnessErrorKind` 七值冻结）/§0.4（`TOOL_ERROR` 为工具错误的唯一码，00-总纲.md:205）；spec 05 §5.7.2（`ToolError` → `TOOL_ERROR`，不产生 `NodeError`，只走工具结果消息）；architecture-v2 §8.2；ISSUE-070（P3，docs/跨册口径）；task-2；REV-016（§3 #4/§4.5 首次指出该不一致）

## 1. 结论

**结论：通过**

改动与任务范围**逐字一致**且为**最小改动**：`src/types.rs` 仅把 `CODE_HARNESS_TOOL_ERROR` 的字符串值 `"HARNESS_TOOL_ERROR"` 改为 `"TOOL_ERROR"`（常量名保留），并补 doc 注释指向 00 §0.4；`tests/failure.rs` 中 `node_error_codes_match_spec` 的对应期望值同步为 `"TOOL_ERROR"`。`HarnessErrorKind` 仍为冻结的 **7 个变体**、`NodeError` 变体未动（diff 无 enum/struct 行）。全仓**代码/测试/脚本**已无旧值 `"HARNESS_TOOL_ERROR"` 残留（仅存常量名与历史评审记录/台账描述，见 §3 #3/#4）。默认特性 `cargo test -p zgraph-kernel` **80 passed / 0 failed**，`--all-features` **83 passed / 0 failed**（与 REV-012 基线 83 一致），`check.sh` CHECK PASSED，worktree clean，提交元数据合规。

未发现阻塞/重要项；2 条建议与 2 条合并备注见 §3，均不影响本分支合并。

## 2. 四维评审

| 维度 | 结论 | 依据（文件/行/测试） |
|---|---|---|
| 功能正确性 | ✅ | ① 与 spec 对齐：`types.rs:106` 值为 `"TOOL_ERROR"`，与 00 §0.4 Harness 域表（`doc/spec-v1.2/00-总纲.md:205`「`TOOL_ERROR`｜工具执行失败｜喂回 LLM，继续循环」）逐字一致；`HarnessErrorKind::ToolError.code()`（`types.rs:347`）因此返回 `TOOL_ERROR`。② 测试真实同步：`tests/failure.rs:237` 期望 `"TOOL_ERROR"`，定向运行 `node_error_codes_match_spec` **1 passed**（断言路径确实是 `NodeError::Harness{kind:ToolError}.code()`，非空断言）。③ 枚举/变体冻结未被触碰：diff 无 enum/struct 行，`HarnessErrorKind` 仍 7 值（`types.rs:321-336`）。 |
| 代码结构合理性 | ✅ | 仅改常量值 + 注释 + 测试断言，无新增/重排/抽象；常量仍集中在 `types.rs`（单一来源，`lib.rs:111 pub use types::*` 继续导出）；无跨 crate/跨模块改动（`git diff --name-status` 仅 2 文件），符合"只改字符串值、不动枚举"的任务约束。 |
| 逻辑性 | ✅ | 错误码生成链自洽：`NodeError::code()` 经 `HarnessErrorKind::code()` 单点映射，改值后所有调用方自动获得新码，无分支遗漏；全仓无其它硬编码 `"HARNESS_TOOL_ERROR"` 的比较/匹配（grep 证据 §4.3），不存在"只改一处导致断言/CLI 分裂"的路径；无新增错误传播/吞异常风险。 |
| 可读性 | ✅ | 注释解释了"为什么"（对齐 00 §0.4）而非复述代码；commit subject 符合 `fix(kernel): …`，正文含 `Review: REV-018` / `Agent: lead` 并说明 ISSUE-070 背景与"不动枚举/变体"的边界；改动 6 行、无魔术值。 |

## 3. 发现清单

| # | 级别 | 位置 | 问题 | 建议 | 状态 |
|---|---|---|---|---|---|
| 1 | 建议 | `crates/zgraph-kernel/src/types.rs:106,347` | 常量**名**仍为 `CODE_HARNESS_TOOL_ERROR`（含 `HARNESS_` 前缀），而**值**已是不带前缀的 `TOOL_ERROR`；名称与值字面不再对应，后续按名检索/推断码值的人可能误以为仍是 Harness 前缀码。本分支按最小改动保留名称是合理的，注释已说明。 | M4 收尾或后续 refactor 时把常量重命名为 `CODE_TOOL_ERROR`（仓内引用仅 `types.rs:347` 一处、测试与外部 crate 均未直接引用该常量名，改动安全），或保留名称但在此注释中补"名称沿用历史，值以 00 §0.4 为准"。不阻塞。 | open |
| 2 | 建议 | `crates/zgraph-kernel/tests/failure.rs:232-238` | 该用例验证 `NodeError::Harness{kind:ToolError}` 的 `code()=="TOOL_ERROR"`，但 spec 05 §5.7.2 明确 `ToolError` **不产生 `NodeError`**（只写工具结果消息、trace 级别 warn）。保留映射本身无碍，但读者可能误以为 ToolError 会走 `Err → Crashed`。 | 在该用例上方或常量注释补一句"ToolError 正常路径不进入 `instances.error`，此映射仅为契约完备性/防御性保留"；或把该条从"错误码全量对照"移到独立小用例并注明语义。M4 harness 评审时确认 harness 实际不构造该 NodeError。不阻塞。 | open |
| 3 | 备注（合并台账） | `doc/issues.md:78`（ISSUE-070） | ISSUE-070 描述仍写「代码 `HARNESS_TOOL_ERROR` 与 00 §0.4 Harness 域 `TOOL_ERROR` 不一致」——本分支合并后该断言过时；但 ISSUE-070 还包含「05/08 回写」「`result_summary: Value` vs `Option<Value>`」等子项，**不应整条置 fixed**。 | Lead 合并时更新 ISSUE-070：标注错误码子项已由 `d4108a2`（REV-018）关闭，保留 open 直至 docs 分支 REV-022 完成 05/08 回写与类型口径统一。本评审不改台账。 | open |
| 4 | 备注 | `doc/reviews/REV-016-docs-m3-wrapup.md:30,101` | 历史评审记录中仍有 `HARNESS_TOOL_ERROR` 文本引用（当时的实况描述）。 | 按 AGENTS §6「评审记录永久保存、不得删除/改写」，保留原样即可；这不是残留缺陷。 | 无需处理 |

> 无阻塞项、无重要项；#1/#2 为建议级，#3 为 Lead 合并时台账动作，#4 为历史记录保留。

## 4. 验证证据

### 4.1 范围、分支与提交元数据

```bash
cd /Users/zyzs/lczyzs/code/zgraph/.worktrees/kernel-lead
git status --short                      # 空（worktree clean）
git branch --show-current               # fix/kernel-m4-tool-error-code
git merge-base origin/main HEAD         # 887743a5bbe51028cb3ee4ae4152ef5092587aae（= base = origin/main，未 rebase）
git rev-parse HEAD                      # d4108a2f2f2c4ddc0032a814bf293a86778973b4
git diff --shortstat 887743a..d4108a2   # 2 files changed, 3 insertions(+), 3 deletions(-)
git diff --name-status 887743a..d4108a2 # M crates/zgraph-kernel/src/types.rs   M crates/zgraph-kernel/tests/failure.rs
git log -1 --format='%an <%ae> / %cn <%ce> / %s' d4108a2
# chenyuxiangg <1007423165@qq.com> / chenyuxiangg <1007423165@qq.com> / fix(kernel): 工具错误码统一为 TOOL_ERROR
```

提交正文含 `Review: REV-018` 与 `Agent: lead`；commit body 说明 ISSUE-070 背景与"只改字符串值，不动枚举/NodeError 变体"。

实际 diff（全文）：

```diff
--- a/crates/zgraph-kernel/src/types.rs
+++ b/crates/zgraph-kernel/src/types.rs
@@ -102,8 +102,8 @@ pub const CODE_HARNESS_LLM_ERROR: &str = "HARNESS_LLM_ERROR";
 /// 单次 LLM 调用超时且重试耗尽。
 pub const CODE_HARNESS_LLM_TIMEOUT: &str = "HARNESS_LLM_TIMEOUT";
 
-/// 工具执行失败（喂回 LLM，不终止）。
-pub const CODE_HARNESS_TOOL_ERROR: &str = "HARNESS_TOOL_ERROR";
+/// 工具执行失败（喂回 LLM，不终止；00 §0.4 统一为 `TOOL_ERROR`）。
+pub const CODE_HARNESS_TOOL_ERROR: &str = "TOOL_ERROR";
 
 /// 记忆检索/写入失败（降级告警，不终止）。
 pub const CODE_HARNESS_MEMORY_DEGRADED: &str = "HARNESS_MEMORY_DEGRADED";
--- a/crates/zgraph-kernel/tests/failure.rs
+++ b/crates/zgraph-kernel/tests/failure.rs
@@ -234,7 +234,7 @@ fn node_error_codes_match_spec() {
                 kind: HarnessErrorKind::ToolError,
                 detail: json!({}),
             },
-            "HARNESS_TOOL_ERROR",
+            "TOOL_ERROR",
         ),
```

### 4.2 必跑命令与结果（worktree 内，`CARGO_HOME=/Users/zyzs/lczyzs/code/zgraph/.cargo-home`）

| 命令 | 结果 |
|---|---|
| `cargo test -p zgraph-kernel --quiet`（任务指定默认特性） | **exit 0**：19 个测试二进制全部 `test result: ok`，合计 **80 passed / 0 failed**（无 FAILED/panicked；日志 `/tmp/rev018-test.log`） |
| `cargo test -p zgraph-kernel --all-features --quiet`（附加，对齐 REV-012 基线口径） | **exit 0**：**83 passed / 0 failed**（`test-hooks` 额外 3 条；日志 `/tmp/rev018-kernel-allfeat.log`） |
| `cargo test -p zgraph-kernel --test failure node_error_codes_match_spec -- --exact`（定向） | `test node_error_codes_match_spec ... ok`；**1 passed / 0 failed**（8 filtered out） |
| `bash scripts/dev/check.sh` | **CHECK PASSED / exit 0**：身份、origin SSH、`CLAUDE.md` 未改、暂存区无禁用文件、分支命名 `fix/*`、Python 语法、progress 21 行 ≤30、issues 存在、`cargo fmt --check`、`git diff --check` 共 12 项 OK |
| `git status --short`（全部命令后） | 空 |

### 4.3 残留字符串全仓核对（branch 工作树）

```bash
grep -rn "HARNESS_TOOL_ERROR" --include="*.rs" --include="*.md" --include="*.json" \
     --include="*.py" --include="*.toml" crates/ python/ tests/ schemas/ doc/ scripts/ AGENTS.md README.md
# crates/zgraph-kernel/src/types.rs:106:pub const CODE_HARNESS_TOOL_ERROR: &str = "TOOL_ERROR";   ← 仅常量名（值已改）
# crates/zgraph-kernel/src/types.rs:347:            Self::ToolError => CODE_HARNESS_TOOL_ERROR,     ← 引用常量，未硬编码值
# doc/issues.md:78:（ISSUE-070 描述文本，历史断言，见 §3 #3）
# doc/reviews/REV-016-docs-m3-wrapup.md:30,101:（历史评审记录，按 AGENTS §6 保留，见 §3 #4）
# 结论：crates/ python/ tests/ schemas/ scripts/ 中已无字符串值 "HARNESS_TOOL_ERROR"；无其它代码引用/比较该值

grep -rn "CODE_HARNESS_TOOL_ERROR\|CODE_TOOL_ERROR" crates/ tests/ python/
# 仅 types.rs:106（定义）与 types.rs:347（HarnessErrorKind::code 映射）两处，无跨 crate 引用

grep -n "TOOL_ERROR" doc/spec-v1.2/00-总纲.md
# 205:| `TOOL_ERROR` | 工具执行失败 | 喂回 LLM，继续循环 |   ← 本分支对齐目标
```

### 4.4 冻结枚举/变体未变更的独立核对

- `git diff 887743a..d4108a2 -- crates/zgraph-kernel/src/types.rs` 过滤 `enum|NodeError|HarnessErrorKind` 行 → **无匹配**（只有常量值与注释两处改动）。
- `HarnessErrorKind` 实读仍为 7 变体：`ConfigLoad / UnknownTool / MaxIterations / LlmError / LlmTimeout / ToolError / MemoryDegraded`（`types.rs:321-336`），与 00 §0.3 冻结七值一致；`NodeError` 八变体未动。

## 5. 遗留问题

- **建议跟进（非阻塞）**：#1 常量改名（`CODE_TOOL_ERROR`）或补名称沿革注释；#2 在测试/注释中显式说明 `ToolError` 不产生 `NodeError`。
- **台账（Lead 合并时处理）**：#3 更新 ISSUE-070 描述（错误码子项已关闭）但保持 open，待 docs REV-022 完成 05/08 回写与 `result_summary` 类型统一后置 fixed。
- **跨分支观察（非本分支缺陷，供 REV-020/harness 评审参考）**：00 §0.4 列有 `HARNESS_CONTEXT_OVERFLOW`，但 00 §0.3 的 `HarnessErrorKind` 七值中**没有** ContextOverflow，kernel 也无对应常量；spec 05 §5.4.5/§5.7.4 又要求溢出携带 `segments/first_over`。M4 harness 若要按 spec 产出该码，需要 Lead 裁定（补 kind/常量并回写 00，或明确复用 `ConfigLoad`/`Internal` 并在 05/00 记偏差）。本分支未触碰该问题，不影响其合并。

## 7. 增量复审（2026-10-30，reviewer-b，HEAD `880efb0`）

> **裁定背景**：Lead 就 §5 末尾「跨分支观察」作出裁定——采用「追加第 8 个 `HarnessErrorKind::ContextOverflow` + `CODE_HARNESS_CONTEXT_OVERFLOW`」（00 §0.4 已有该码、00 §0.3 缺该 kind 属 spec 内部不一致；按"仅增不删"先例，docs 将回写 00 §0.3 为 8 值）。作者 `lead` 在 `d4108a2` 上补 commit `880efb0`；本节为**增量复审**，按 AGENTS §6 **不删除、不改写** §1–§6 的原「通过」结论与证据。
>
> 注：§2/§4.4 中「`HarnessErrorKind` 仍 7 变体」的表述对应评审时 HEAD `d4108a2`；本增量后为 **8 变体**（原 7 值顺序/语义未变，新值追加在末尾）。

- **增量范围**：`d4108a2..880efb0`（1 commit，`git diff --shortstat` = **2 files changed, 13 insertions(+)**，无删除；`M crates/zgraph-kernel/src/types.rs`、`M crates/zgraph-kernel/tests/failure.rs`）；分支整体 base 仍为 `887743a`（未 rebase；评审时 `origin/main` 已前进至 `389b277`）。
- **增量结论：通过**（全分支 REV-018 结论维持「通过」）。

### 7.1 四维评审（仅增量）

| 维度 | 结论 | 依据（文件/行/测试） |
|---|---|---|
| 功能正确性 | ✅ | ① 与 Lead 描述的 4 处改动**逐字一致**：常量 `CODE_HARNESS_CONTEXT_OVERFLOW = "HARNESS_CONTEXT_OVERFLOW"`（`types.rs:107-108`）、枚举追加第 8 值 `ContextOverflow`（`:339-340`，原 7 值 `:325-338` 未动）、`code()` 补映射（`:354`）、`failure.rs:246-252` 补 `node_error_codes_match_spec` 用例。② 与 00 §0.4 对齐：`doc/spec-v1.2/00-总纲.md:203`「`HARNESS_CONTEXT_OVERFLOW`｜预算裁剪后仍溢出｜Crashed」；spec 05 有 10+ 处要求该码（§5.4.4/§5.4.5/§5.5.1/§5.7.4 等），补 kind 使实现可表达 spec 语义。③ 断言真实：定向 `node_error_codes_match_spec` **1 passed**，新 tuple 经 `NodeError::Harness{kind:ContextOverflow}.code()=="HARNESS_CONTEXT_OVERFLOW"` 实际验证。 |
| 代码结构合理性 | ✅ | 纯增量：新常量放在 Harness 码常量区、新变体追加在枚举末尾、`code()` 同点补分支，无重排/重命名/重构；仅 2 文件、+13/−0，未触碰 `NodeError` 变体或其它模块；不新增依赖/文件。 |
| 逻辑性 | ✅ | `match self` 在编译期强制穷尽（新增变体后编译通过即证明无遗漏分支）；全仓 `HarnessErrorKind::` 引用仅 `types.rs` 与 `tests/failure.rs`（8 个变体在测试矩阵中全覆盖），`cli/trace/worker` 无 match 依赖 → 追加变体不引起下游编译或语义破坏；枚举无 serde derive，序列化面不受影响；既有 7 变的 `code()` 返回值不变（diff 仅 +1 行）。 |
| 可读性 | ✅ | 常量与变体均有中文 doc 注释解释"为什么"（M4 追加、00 §0.4 有码而 §0.3 缺 kind、指向 05 §5.13）；commit subject 符合 `feat(kernel): …`，正文说明动机、边界（不改既有 7 值）与 docs 回写计划，含 `Review: REV-018` / `Agent: lead`。 |

### 7.2 增量验证证据（worktree 内，`CARGO_HOME=/Users/zyzs/lczyzs/code/zgraph/.cargo-home`，HEAD `880efb0`）

```bash
git status --short                        # 空（clean）
git diff --shortstat d4108a2..880efb0     # 2 files changed, 13 insertions(+)
git diff d4108a2..880efb0 --check         # exit 0
git log -1 --format='%H|%an <%ae>|%cn <%ce>|%s' 880efb0
# 880efb08c34cc2a39c905e838e903bd16f8e69dc|chenyuxiangg <1007423165@qq.com>|chenyuxiangg <1007423165@qq.com>|feat(kernel): 追加 HarnessErrorKind::ContextOverflow 与错误码
# 正文含 Review: REV-018 / Agent: lead
```

| 命令 | 结果 |
|---|---|
| `cargo test -p zgraph-kernel --quiet`（默认特性） | **exit 0，80 passed / 0 failed** |
| `cargo test -p zgraph-kernel --all-features --quiet` | **exit 0，83 passed / 0 failed** |
| `cargo test -p zgraph-kernel --test failure node_error_codes_match_spec -- --exact` | **1 passed / 0 failed**（新增 tuple 实际执行；8 filtered out） |
| `cargo clippy -p zgraph-kernel --all-targets -- -D warnings` | **exit 0**（`Finished dev profile`，无 warning） |
| `bash scripts/dev/check.sh` | **CHECK PASSED**（12 项 OK，含 cargo fmt / git diff --check） |
| `git status --short`（全部命令后） | 空 |

> **用例数说明**：默认 80 / all-features 83 两个口径的计数**本增量均未变化**（差异 3 条来自 `test-hooks` 特性）；新增的 ContextOverflow 断言是既有 `node_error_codes_match_spec` 内新增 tuple，已通过定向用例确认执行且断言生效。Lead 消息中「80→83 用例数变化」应理解为两口径对比，非本次增量新增 3 个测试函数。

### 7.3 增量发现

| # | 级别 | 位置 | 问题 | 建议 | 状态 |
|---|---|---|---|---|---|
| 1 | 建议（docs 范围补全） | `doc/spec-v1.2/00-总纲.md:98-106`、`doc/spec-v1.2/05-AgentHarness.md:839-849` | task-5 的交付物清单**未显式列出**「00 §0.3 `HarnessErrorKind` 由 7 值改 8 值（追加 `ContextOverflow`）」与「05 §5.7.2 总映射表补 `ContextOverflow` 行」（当前 spec 仍 7 值、表仍 7 行）。若 docs 漏改，spec 与实现将长期不一致。 | 请 Lead 把这两点明确补进 docs 任务范围（或复核 docs-writer 已覆盖），并在 05 §5.11/§5.13 记录该偏差（00 §0.4 已有码 vs §0.3 原缺 kind）。不阻塞代码合并。 | open |
| 2 | 建议（悬空引用） | `crates/zgraph-kernel/src/types.rs:108,339`（注释「见 05 §5.13」） | 当前 `doc/spec-v1.2/05-AgentHarness.md` **尚无 §5.13**（docs 分支 REV-022 计划新增）；docs 落地前该引用指向不存在章节。 | 确保 docs REV-022 与 kernel 同批或先行合并；或临时改指现有 §5.4.5/§5.7.4。不阻塞。 | open |
| 3 | 备注（风格） | `crates/zgraph-kernel/src/types.rs:107-108,339-340` | 新常量/变体位置（`TOOL_ERROR` 之后、`MEMORY_DEGRADED` 之前）与 00 §0.4 表顺序（…LLM_TIMEOUT → CONTEXT_OVERFLOW → MEMORY_DEGRADED → TOOL_ERROR）不一致；仅影响阅读。 | docs 回写以 §0.4 表为权威顺序；代码排序可后置统一，非必需。不阻塞。 | open |
| 4 | 备注（API 稳定性） | `crates/zgraph-kernel/src/types.rs:322-341`（`pub enum HarnessErrorKind`） | 公开枚举追加变体会破坏仓外对 `match` 的穷尽性（semver-breaking）；仓内 grep 仅 `types.rs`+`tests/failure.rs` 引用，`cli/trace/worker` 均无 match，本次无实际破坏，且符合 `lib.rs`「新增项只能追加」的 API 约定。 | M4 预发布阶段可接受；若未来对外稳定 API，可考虑 `#[non_exhaustive]`（需另行裁定，非本分支）。不阻塞。 | open |

### 7.4 增量结论与可合并性

- **增量复审：通过**：4 处改动与裁定/描述逐字一致，"仅增不删"落地正确（原 7 值顺序/语义未变）；默认/all-features 测试、定向断言、clippy、check.sh 全绿；无阻塞/重要项。
- **全分支 REV-018 结论维持「通过」**；§7.3 #1/#2 为 docs 回写清单提醒（非合并门禁），#3/#4 为备注。
- 可合并性不变：`merge.sh` 会 rebase 到最新 `origin/main`（`389b277`，已含 REV-017）并重跑全量；原 §5 遗留 #1–#3 仍有效，其中「跨分支观察（ContextOverflow）」已由本增量按 Lead 裁定落地。
