# REV-017 · process · M4 依赖前置（workspace deps + AGENTS py-worker 归属）

> **模块声明**：本分支只改 `Cargo.toml`（`[workspace.dependencies]` 新增 3 条）与 `AGENTS.md`（§2 模块表 `py-worker` 一行），均属 `process` 模块边界（AGENTS §2：`Cargo.toml`/`Cargo.lock`/`AGENTS.md`）。作者 `lead`，评审人 `reviewer-b`（**非作者**）。本次评审**未修改任何代码/配置/台账**，只新增本记录；未 commit / 未 merge / 未 rebase。
>
> **独立核对口径**：以下结论均来自本人在 worktree 内亲自执行的 `git diff` / `cargo metadata` / `cargo check` / `check.sh` 与 `Cargo.lock` 哈希比对，不采信作者/任务描述自述；对任务描述中的"Cargo.lock 未变属正常 cargo 行为"做了独立验证（§4.3）。

- **分支**：`chore/process-m4-deps`
- **作者**：`lead`
- **评审人**：`reviewer-b`
- **变更范围**：`887743a..f434d66`（2 commits：`e49e1a9` workspace 依赖补 genai/async-trait/tokio-util → `f434d66` AGENTS py-worker 归属补 `python/tests/**`；`git diff --shortstat` = **2 files changed, 4 insertions(+), 1 deletion(-)**；base = merge-base = `origin/main` = `887743a`，**未 rebase**）
- **评审时间**：2026-10-30
- **关联**：architecture-v2 §2.1（crate 布局：`zgraph-harness` = AgentConfig/LLM provider/上下文预算/tool loop）、§6.1（M4 Harness DoD）；spec 05 §5.1.3（选型冻结：`genai` 0.7.2 作 LLM transport）、§5.3.1（`LlmProvider` trait，async-trait）；ISSUE-070（工具错误码，另由 REV-018 处理）、ISSUE-071（AGENTS §2 未列 `python/tests/**`）；task-1

## 1. 结论

**结论：通过**

改动与任务范围**逐字一致**：`Cargo.toml` 仅在 `[workspace.dependencies]` 末尾新增 `async-trait`/`genai`/`tokio-util` 三行（未改任何现有依赖、未新增 crate 引用）；`AGENTS.md` 仅 `py-worker` 模块路径追加 `python/tests/**` 一处（正确闭合 ISSUE-071 的 "AGENTS 表由 process 补" 子项）。`Cargo.lock` 未变**经独立验证属预期**：三条新声明当前**无任何 workspace 成员引用**（`grep "workspace = true"` 命中 0），lock 只记录实际参与解析的依赖图，未使用声明不会写入 lock——base/head/工作区三处 `Cargo.lock` 的 SHA-256 完全一致（§4.3）。`genai = "0.7.2"` 与 spec 05 §5.1.3 冻结版本一致（架构蓝图 §6.1 只规定 M4 范围，未钉版本）。`cargo metadata` exit 0、`cargo check --workspace` exit 0、`check.sh` CHECK PASSED、提交元数据/正文合规、无无关改动、worktree clean。

未发现阻塞/重要项；3 条建议与 1 条合并台账备注见 §3，均不影响本分支合并。

## 2. 四维评审

| 维度 | 结论 | 依据（文件/行/命令） |
|---|---|---|
| 功能正确性 | ✅ | ① 依赖声明完整且落在正确位置：`Cargo.toml:30-32`（`[workspace.dependencies]` 内，紧随 `rusqlite`；文件无其它 section 被误改）；② 版本对齐：`genai = "0.7.2"` == spec 05 §5.1.3（05-AgentHarness.md:43「**`genai` 0.7.2**」）；`async-trait = "0.1"` 满足 `#[async_trait]` 需求（§5.3.1）；`tokio-util` 的 `rt` feature 实核存在且提供 `sync::CancellationToken`（§4.4，尽管 v2 取消最终走轮询，见建议 #2）；③ AGENTS 归属：`AGENTS.md:37` 新增 `python/tests/**`，与实际 `python/tests/{test_ctx,test_protocol,test_runtime,test_workers}.py` 及 ISSUE-071 台账描述一致，且与 `e2e`（`tests/e2e/**`）/`docs`（`doc/**`）无路径重叠；④ 全仓无成员引用新依赖（0 处）→ 不影响任何 crate 现有构建/测试。 |
| 代码结构合理性 | ✅ | 依赖集中到 workspace 统一声明（后续 harness `workspace = true` 即可），符合 §2.1 crate 布局与既有 `[workspace.dependencies]` 风格；两个 commit 各自只做一件事（commit1 = 依赖、commit2 = 模块归属），满足 AGENTS §1「一个提交只做一件事」；未改 `Cargo.lock`/代码/CI/脚本，无越界。 |
| 逻辑性 | ✅ | ① lock 未变化的因果链自洽并经实验证伪/证实（未使用声明不参与 resolve → metadata/check 不拉取 → lock 不变）；② 依赖声明语法/feature 均有效（`cargo metadata` 与 `cargo check --workspace` 均 exit 0，说明 manifest 可解析、workspace 可编译）；③ 模块归属变更无逻辑冲突（`python/tests/**` 此前无模块声明，新增不与任何模块重叠）。 |
| 可读性 | ✅ | `Cargo.toml` 一依赖一行、无注释噪音；commit subject 符合 `<type>(<module>): <摘要>`，正文均含 `Review: REV-017` 与 `Agent: lead`；AGENTS 单元格沿用既有「反引号路径 + 顿号」格式；无魔术值/无残留 TODO。 |

## 3. 发现清单

| # | 级别 | 位置 | 问题 | 建议 | 状态 |
|---|---|---|---|---|---|
| 1 | 建议 | `Cargo.toml:31` | `genai = "0.7.2"` 是 caret 语义（允许 0.7.x patch 漂移），而 spec 05 §5.1.3 的选型陈述冻结为「`genai` 0.7.2」；当前 lock 无 `genai` 条目，**实际解析到的精确版本要等 harness 首次引用才确定**（本次无法在本分支验证）。 | harness 分支合并后由评审/Lead 核对 `Cargo.lock` 中 `genai` 精确为 `0.7.2`；若要求与 spec 强绑（避免 patch 版本行为差异影响 L2 可复现），可在 process 收尾时改为 `=0.7.2`。不阻塞本分支。 | open |
| 2 | 建议 | `Cargo.toml:32` + task-4 口径 | 本分支声明 `tokio-util`（`rt`）为 M4 依赖；但 task-4 明确 v2 取消语义为 `HarnessCtx` 轮询、**不引入 `CancellationToken`**（spec 05 §5.3.1 的 `CancelToken` 在 v2 退化为轮询包装）。若 harness 最终不引用 `tokio-util`，该 workspace 声明会长期闲置。 | 合并不受影响；M4 收尾时若确认 harness 不引用，删除该行或注释标注「预留」；若 harness 实际使用 `CancellationToken`，则保留并在 §5 回写中说明与 spec 的对应关系。 | open |
| 3 | 建议（环境） | `.cargo-home` 缓存 | `async-trait` **未在** `.cargo-home` 缓存（`find .cargo-home/registry -name 'async-trait*'` 无结果），`genai-0.7.2`/`tokio-util-0.7.20` 已缓存。本分支的 `cargo metadata`/`cargo check` 因未引用新依赖而不受影响；但 harness 分支首次 `workspace = true` 引用 `async-trait` 时需联网拉取（Lead 已说明网络可用）。 | process/Lead 可在本分支合并后预热一次（如 harness worktree 内 `cargo fetch`），避免 harness 构建期网络等待；不影响本分支通过。 | open |
| 4 | 备注（合并台账） | `doc/issues.md:79`（ISSUE-071） | 本分支只关闭 ISSUE-071 的「AGENTS §2 模块表未列 `python/tests/**`」**子项**，ISSUE-071 本身是多子项汇总（P3/open，含示例重复、`again is not False`、恒真断言、整包广播口径等），不应整条置 fixed。 | Lead 合并时更新台账：保留 `ISSUE-071` open，在描述/备注中标注该子项已随 `f434d66`（REV-017）关闭；其余子项按 M4/M5 跟踪。本评审不改台账（AGENTS §7 并发约束）。 | open |

> 无阻塞项、无重要项；#1–#3 为建议级，#4 为 Lead 合并时的台账动作。结论「通过」的依据是任务范围全部达成且无实质性缺陷。

## 4. 验证证据

### 4.1 范围、分支与提交元数据

```bash
cd /Users/zyzs/lczyzs/code/zgraph/.worktrees/process-lead
git status --short                      # 空（worktree clean）
git branch --show-current               # chore/process-m4-deps
git merge-base origin/main HEAD         # 887743a5bbe51028cb3ee4ae4152ef5092587aae（= base = origin/main，未 rebase）
git rev-parse HEAD                      # f434d667e66ecbc23d2656d49cbc504604c35750
git diff --shortstat 887743a..f434d66   # 2 files changed, 4 insertions(+), 1 deletion(-)
git diff --name-status 887743a..f434d66  # M AGENTS.md   M Cargo.toml（仅此 2 个文件；无 Cargo.lock/doc/progress.md/doc/issues.md/代码）
```

- 两笔 commit 均 `author=committer=chenyuxiangg <1007423165@qq.com>`，subject 分别为
  `chore(process): workspace 依赖补 genai/async-trait/tokio-util`、`chore(process): py-worker 模块归属补 python/tests/**`，
  正文均含 `Review: REV-017` / `Agent: lead`（§4.2 实际输出的 `git log -1 --format` 已逐条核对）。

实际 diff（全文，共 2 个 hunk）：

```diff
--- a/AGENTS.md
+++ b/AGENTS.md
-| `py-worker` | `python/zgraph_worker/**`、`python/workers/**`、`tests/fixtures/workers/**` |
+| `py-worker` | `python/zgraph_worker/**`、`python/tests/**`、`python/workers/**`、`tests/fixtures/workers/**` |
--- a/Cargo.toml
+++ b/Cargo.toml
@@ -27,3 +27,6 @@
 uuid = { version = "1", features = ["v7"] }
 chrono = { version = "0.4", features = ["serde"] }
 rusqlite = { version = "0.41", features = ["bundled"] }
+async-trait = "0.1"
+genai = "0.7.2"
+tokio-util = { version = "0.7", features = ["rt"] }
```

### 4.2 必跑命令与结果（worktree 内，`CARGO_HOME=/Users/zyzs/lczyzs/code/zgraph/.cargo-home`）

| 命令 | 结果 |
|---|---|
| `cargo metadata --format-version 1` | **exit 0**（stdout 重定向至 `/tmp/rev017-metadata.json`，stderr 空）；执行后 `git status --short` 仍空（未改 lock） |
| `cargo check --workspace --quiet` | **exit 0**（worktree 内全新 `target/` 编译，无失败；日志见 `/tmp/rev017-check.log`） |
| `bash scripts/dev/check.sh` | **CHECK PASSED / exit 0**：身份（`chenyuxiangg` / `1007423165@qq.com`）、origin SSH、`CLAUDE.md` 未改、暂存区无禁用文件、分支命名 `chore/*`、Python 语法、`doc/progress.md` 存在（非空行 21 ≤30）、`doc/issues.md` 存在、`cargo fmt --check`、`git diff --check` 共 12 项 OK |
| `git status --short`（全部命令后） | 空 |

### 4.3 「Cargo.lock 未变是预期」的独立验证

```bash
cd /Users/zyzs/lczyzs/code/zgraph/.worktrees/process-lead
git show 887743a:Cargo.lock | shasum -a 256   # 7c584eb946004504b2eb6240ea42709dde8a18d3c466b303ba57d1093e915419
git show f434d66:Cargo.lock  | shasum -a 256  # 7c584eb946004504b2eb6240ea42709dde8a18d3c466b303ba57d1093e915419
shasum -a 256 Cargo.lock                      # 7c584eb946004504b2eb6240ea42709dde8a18d3c466b303ba57d1093e915419
grep -c 'name = "genai"\|name = "async-trait"\|name = "tokio-util"' Cargo.lock   # 0
grep -rn 'workspace = true' crates/*/Cargo.toml tests/e2e/Cargo.toml | grep -cE 'genai|async-trait|tokio-util'  # 0 处引用
```

**结论**：任务描述的"预期"成立。`Cargo.lock` 记录的是**实际参与解析的依赖图**；`[workspace.dependencies]` 只是声明表，未被任何成员 `workspace = true` 引用时不会进入 resolve，因此 lock 三处哈希完全一致，`cargo metadata`/`cargo check` 也不会去拉取这三条声明（故本次未验证其完整依赖树的离线可用性——这属于 harness 分支的首次引用时行为，见建议 #3）。此行为与 cargo 语义一致，非"忘记更新 lock"。

### 4.4 依赖声明有效性（静态核对缓存 registry）

- `genai`：`.cargo-home/registry/{cache,src}/…/genai-0.7.2*` 存在，版本与 spec 05 §5.1.3 一致。
- `tokio-util`：缓存 `tokio-util-0.7.20`；其 `Cargo.toml [features]` 含 **`rt`**（`rt = ["tokio/rt","tokio/sync","futures-util"]`），且 `src/sync/mod.rs` 导出 `CancellationToken`/`WaitForCancellationFuture`——声明的 feature 名有效（不会在 harness 引用时因未知 feature 报错）。
- `async-trait`：未在缓存（见建议 #3），但版本 `0.1` 与 spec 05 §5.3.1 的 `#[async_trait]` 用法匹配；声明语法有效（`cargo metadata` 已证明 manifest 可解析）。

### 4.5 AGENTS 归属正确性

```bash
ls python/tests/    # test_ctx.py test_protocol.py test_runtime.py test_workers.py（~__pycache__）
grep -n 'ISSUE-071' doc/issues.md   # :79 明确「AGENTS §2 模块表未列 python/tests/** … AGENTS 表由 process 补」
```

新增行与 `e2e`（`tests/e2e/**`、`tests/fixtures/graphs/**`）无前缀重叠；`python/tests/**` 此前不属于任何模块，补入 `py-worker` 消除并行 worktree 越界风险。

## 5. 遗留问题

- **建议跟进（非阻塞）**：#1 genai 精确版本在 harness 合并后核对 lock；#2 tokio-util 是否最终使用在 M4 收尾复核；#3 合并后预热 `async-trait` 缓存/或确保 harness 构建期网络可用。
- **台账（Lead 合并时处理）**：#4 ISSUE-071 保持 open，标注 `python/tests/**` 子项已关闭（本评审不改 `doc/issues.md`）。
- **已知不在本分支范围**：`doc/spec-v1.2` 的版本/常量回写由 docs 分支（REV-022）负责；`Cargo.lock` 的最终更新由首个引用新依赖的 harness 分支负责——均已确认与 task-1 分工一致。
