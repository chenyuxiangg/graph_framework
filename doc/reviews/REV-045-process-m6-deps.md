# REV-045 · process · M6 依赖治理：workspace.dependencies 提升 + CI `--locked`

> **模块声明（跨模块）**：本分支改根 `Cargo.toml` 与 `.github/workflows/ci.yml`（`process` 模块，作者 `lead` 为 process 负责人），以及 `crates/zgraph-{dsl,cli,harness,worker}/Cargo.toml`、`tests/e2e/Cargo.toml`（按 AGENTS §2 分属 dsl/cli/harness/worker/e2e 模块）——属**跨模块**改动。每处改动均为**依赖声明位置迁移**（`version = …` → `.workspace = true`，成员既有 feature 原样保留）或注释/CI 参数，无代码、测试、feature 或依赖图变化（等价性见 §4.2）。提交正文未显式声明跨模块，本记录由**非作者**评审人 `reviewer-a` 作独立第二评审并在本段声明（DR-003 D13 / AGENTS §2）。评审人**未修改任何被评审代码/配置**，只新增本记录；未 merge / 未 push / 未改 main / 未改台账。
>
> **评审对象**：`chore/process-m6-deps` @ `4096853`（作者 `lead`，1 commit，base = `origin/main` = `a031269`）。

- **分支**：`chore/process-m6-deps`
- **作者**：`lead`
- **评审人**：`reviewer-a`（**非作者**）
- **变更范围**：`origin/main`（`a031269`）..`4096853`，**1 commit / 7 files changed, 15 insertions(+), 8 deletions(-)**：`Cargo.toml`（+5 行：blake3/futures-util/nix 三条 workspace 依赖 + genai 决策注释 3 行）、`crates/zgraph-cli/Cargo.toml`、`crates/zgraph-dsl/Cargo.toml`、`crates/zgraph-harness/Cargo.toml`、`crates/zgraph-worker/Cargo.toml`、`tests/e2e/Cargo.toml`（各 1–2 行 `.workspace = true`）、`.github/workflows/ci.yml`（clippy/test 各加 `--locked`）；**`Cargo.lock` 未变**（blob 与 SHA-256 均与 `origin/main` 相同，§4.3）；无代码/测试/`doc/**`/`schemas/**`/`CLAUDE.md` 改动。
- **评审时间**：2026-10-30
- **关联**：`doc/issues.md` ISSUE-068/073/078；`doc/decisions/DR-003-m6-scope-and-waves.md` D2/D13、§1（REV-037→REV-045）、§2 波次 1b；AGENTS §1/§2/§3/§4/§9；REV-017（M4 workspace deps 前置，同类改动先例）；`doc/architecture-v2.md` §2.1；`.github/workflows/ci.yml`

## 1. 结论

**结论：通过**

改动与 DR-003/台账声明的范围逐项一致：**ISSUE-068**（`nix` 提升 workspace，含 e2e dev-dep 重复声明）、**ISSUE-078**（`futures-util` 提升）、以及 **ISSUE-073** 的决策指针注释（保持 `genai 0.7.2`、接受 `preserve_order` 现状、不 feature-gate；未改行为）+ CI `--locked` 固化。`Cargo.lock` 与 `origin/main` **完全相同**（blob `4e7de53…`、SHA-256 `faa5fe3e…`）；用 `cargo metadata --locked --offline` 对 branch 与 main 做**归一化对比**：包数 280=280、resolve 节点特征/依赖 **0 差异**、7 个成员的依赖请求（req/features/default-features/optional/target）逐一相等 → 证明本次是**纯声明位置迁移，feature 合并不变**（`nix` 成员差异化 feature 保留：cli=`signal`、worker/e2e=`signal+process`；`blake3`/`futures-util` 默认 feature 不变）。`cargo clippy --workspace --all-targets --locked -- -D warnings` exit 0（0 warning，28.18s）；`cargo test --workspace --locked` **58 个测试二进制、556 passed / 0 failed / 2 ignored**，与 `doc/test-baseline.txt`（失败名额 0、2 ignored）一致，无新增失败；`check.sh` PASS、worktree clean、提交元数据合规。

未发现阻塞/重要项；3 条 Nit/建议与 1 条合并后台账动作见 §3，均不影响合并。

## 2. 四维评审

| 维度 | 结论 | 依据（文件/行/命令） |
|---|---|---|
| 功能正确性 | ✅ | ① ISSUE-068 闭环：`Cargo.toml:39` 新增 `nix = "0.29"`；`crates/zgraph-cli/Cargo.toml:33`、`crates/zgraph-worker/Cargo.toml:18`、`tests/e2e/Cargo.toml:31` 改为 `{ workspace = true, features = […] }`（e2e dev-dep 重复声明消除，feature 保留）。② ISSUE-078 闭环：`Cargo.toml:37` 新增 `futures-util = "0.3"`，`crates/zgraph-harness/Cargo.toml:21` 改 `workspace = true`。③ ISSUE-073 只记决策不改行为：`Cargo.toml:31-34` 仅注释（保持 `genai = "0.7.2"`），无版本/feature 改动；`--locked` 使 CI 锁定既有 `Cargo.lock`。④ 依赖等价性经 `cargo metadata` 归一化对比实证（§4.2），resolved features 与依赖请求零差异。 |
| 代码结构合理性 | ✅ | 依赖集中到 `[workspace.dependencies]`（与既有 `serde`/`tokio`/`rusqlite` 等风格一致），成员改 `.workspace = true`；每处 diff 最小（成员 1–2 行），未夹带代码/测试/spec 改动，符合「一个提交只做一件事」；跨模块面仅为 5 个 `Cargo.toml` 的声明行，已在本记录声明并由非作者第二评审（AGENTS §2）；不触碰 `Cargo.lock`（版本未变，无需刷新）。 |
| 逻辑性 | ✅ | ① feature 统一风险核对：workspace 声明不设 `default-features=false`/features，成员各自追加 features，Cargo feature union 与 main 完全一致（`cargo metadata` resolve 节点 0 差异，§4.2）。② `nix` workspace 声明为无条件版本行，但成员引用仍在 `[target.'cfg(unix)'.dependencies]`，非 Unix 不参与编译（metadata 不变）。③ `--locked` 与 lock 一致：`cargo metadata --locked` exit 0、CI 有 `Cargo.lock`，不会因 lock 漂移失败。④ `Cargo.lock` 未变是正确结果而非遗漏：声明版本 `"1"/"0.3"/"0.29"` 已被 lock 解析为 1.8.7/0.3.34/0.29.0（§4.3）。 |
| 可读性 | ✅ | 新增注释解释「为什么」：genai 注释说明 `preserve_order` 副作用、装箱缓解、CI 守卫与「勿随意升级」决策指针；nix 注释说明平台约束；成员行保留既有 feature 注释。Nit：`Cargo.toml:31` 的 `ISS-073` 与仓库统一编号 `ISSUE-073` 不一致（#1）。 |

## 3. 发现清单

| # | 级别 | 位置 | 问题 | 建议 | 状态 |
|---|---|---|---|---|---|
| 1 | 建议（可读性） | `Cargo.toml:31` | 注释写作 `ISS-073`，与仓库统一编号 `ISSUE-073`（台账/DR/评审记录一致用法）不符；不影响功能 | 改为 `ISSUE-073`（一处字符串） | open（不阻塞） |
| 2 | 建议（依赖治理边界，承接 REV-017 #1） | `Cargo.toml:33-34` | 注释称「勿在本行随意升级」，但 `genai = "0.7.2"` 是 caret 语义（允许 0.7.x patch 漂移）；当前 `Cargo.lock` 已锁 0.7.2 且 CI `--locked` 保证 CI 不变，但本地 `cargo update` 仍可漂移 patch | 若 DR-003 D2 的「保持 0.7.2」意指精确冻结，可改 `genai = "=0.7.2"`；若有意接受 patch，在注释注明「caret 有意，patch 漂移由 lock/`--locked` 约束」。不阻塞本分支 | open（不阻塞） |
| 3 | 备注（合并后台账动作） | `doc/issues.md`（本分支未改，符合分工） | 本分支只交付实现/CI；台账与进度由 Lead 维护。若被误写为「ISSUE-073 已修」会与 D2 的 `wontfix` 口径冲突 | 合并后 Lead 更新台账：**ISSUE-068/078 → `fixed`**（注明 `4096853`/REV-045）；**ISSUE-073 → `wontfix`**（理由：保持 genai 0.7.2、装箱缓解 + CI `-D warnings`/`--locked` 守卫；不得写「已修」）；progress 标记 REV-045 完成 | open（合并后执行） |
| 4 | 备注（跨模块声明） | `4096853` 提交正文 | 分支跨 process + 5 个成员 crate 的 `Cargo.toml`，提交正文未显式声明跨模块（与 REV-025 #10 同类，不阻塞） | 本记录已由非作者 reviewer-a 作第二评审并声明；建议 Lead 在合并说明/台账补一行「跨模块机械改动，REV-045 第二评审」 | open（流程记录） |
| 5 | 备注（验证口径） | 任务描述「clap/特性不得变化」 | 本仓 workspace 无 `clap` 依赖（`grep -rn clap --include=Cargo.toml` 0 命中），本次不涉 clap；已按「依赖请求 + resolved features 不变」核对（§4.2） | 无动作；后续 REV-043 若新增 clap 才需单独评审 | 无需处理 |

> 无阻塞项、无重要项；结论「通过」。#1/#2 为 Nit/建议，#3/#4 为合并后 Lead/流程动作，#5 为口径说明。

## 4. 验证证据

### 4.1 范围、分支与提交元数据（worktree `.worktrees/process-lead-deps`，实际执行）

```bash
$ git status --porcelain                     # 空（评审前后与测试后均 clean）
$ git branch --show-current                  # chore/process-m6-deps
$ git rev-list --count origin/main..HEAD     # 1
$ git merge-base origin/main HEAD            # a031269727c5b9c58eb7155a8806d1012b2a3f50（= origin/main）
$ git diff origin/main..4096853 --name-status
# M .github/workflows/ci.yml
# M Cargo.toml
# M crates/zgraph-cli/Cargo.toml
# M crates/zgraph-dsl/Cargo.toml
# M crates/zgraph-harness/Cargo.toml
# M crates/zgraph-worker/Cargo.toml
# M tests/e2e/Cargo.toml            ← 7 文件，无 Cargo.lock / doc / 代码 / 测试 / schema
$ git diff origin/main..4096853 --stat
# 7 files changed, 15 insertions(+), 8 deletions(-)
$ git log -1 --format='%H%n%an <%ae> | %cn <%ce>%n%s%n%n%B'
# 40968538ac080c8a3438073ac664a0698640156f
# chenyuxiangg <1007423165@qq.com> | chenyuxiangg <1007423165@qq.com>
# chore(process): 依赖治理——nix/blake3/futures-util 提升 workspace.dependencies + CI --locked
# 正文：Review: REV-045 / Agent: lead（符合 AGENTS §1）
$ git diff origin/main..4096853 -- Cargo.lock | wc -l   # 0
$ git rev-parse origin/main:Cargo.lock 4096853:Cargo.lock   # 4e7de53… / 4e7de53…（同一 blob）
$ git show origin/main:Cargo.lock | shasum -a 256; shasum -a 256 Cargo.lock
# faa5fe3e31adad493f54cbfb7f74e7d017446ca5f1436803810a87a0f40ff4d2（两者相同）
```

### 4.2 依赖写法与 feature 等价（cargo metadata 独立对比，不采信作者自述）

方法：`git archive origin/main | tar -x -C /tmp/rev045-main` 导出 main 树；两棵树各执行
`CARGO_HOME=<repo>/.cargo-home CARGO_NET_OFFLINE=true cargo metadata --locked --offline --format-version 1`（均 exit 0）。
对 `path+file://` 前缀做归一化后逐项比较：

```text
包数：branch 280 / main 280
resolve 节点集合：相同（归一化后 0 差异）
resolve.node.features 与 dependencies：0 差异
7 个 workspace 成员的依赖请求（name/req/features/default_features/optional/target）：逐一相等
关键三方包 resolved features（branch == main）：
  blake3 1.8.7      {default, std}
  futures-util 0.3.34 {alloc, async-await, async-await-macro, channel, default, futures-channel,
                       futures-io, futures-macro, futures-sink, io, memchr, sink, slab, std}
  nix 0.29.0        {default, process, signal}
成员差异化 feature（保持原样，未“统一”掉）：
  zgraph-cli  → nix {features:[signal]}                 target=cfg(unix)
  zgraph-worker → nix {features:[signal, process]}      target=cfg(unix)
  zgraph-e2e  → nix {features:[signal, process]}        target=cfg(unix)
  zgraph-dsl/cli → blake3 默认；zgraph-harness → futures-util 默认
```

结论：`version` → `[workspace.dependencies]` 为**纯声明位置迁移**，依赖图与 feature 合并结果与 `origin/main` 完全一致；`nix` 的 workspace 声明未改变成员的平台/feature 语义。

### 4.3 `Cargo.lock` 未变且与 manifests 一致

- 分支未改 `Cargo.lock`；blob id 与 `origin/main` 相同（`4e7de53…`），SHA-256 相同（`faa5fe3e…`）。
- `cargo metadata --locked` 在两棵树均 exit 0 → lock 与 manifest 一致（`--locked` 此时生效）。
- 声明版本与 lock 实解析一致：`blake3 1.8.7`、`futures-util 0.3.34`、`nix 0.29.0`、`genai 0.7.2`（`git show 4096853:Cargo.lock`）。

### 4.4 clippy（ISSUE-073 的 CI 固化证据）

```bash
$ CARGO_HOME=<repo>/.cargo-home CARGO_TARGET_DIR=<repo>/target CARGO_NET_OFFLINE=true \
  cargo clippy --workspace --all-targets --locked -- -D warnings
    Checking zgraph-dsl / zgraph-trace / zgraph-kernel / zgraph-worker / zgraph-harness / zgraph-cli / zgraph-e2e …
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 28.18s
# exit 0；日志中 warning 行数 = 0
```

> 说明（口径校正）：`-D warnings` 在 main 的 CI clippy 步骤中**已存在**；本分支新增的是 `--locked`（锁定 `Cargo.lock`、保证 CI 解析确定）。因此 ISSUE-073 的交付是「决策指针注释 + 既有 `-D warnings` 守卫 + 新增 `--locked`」，**不是**新增 `-D warnings` 能力——台账/总结不得夸大。

### 4.5 全量测试（cargo test --workspace --locked）

```bash
$ CARGO_HOME=<repo>/.cargo-home CARGO_TARGET_DIR=<repo>/target CARGO_NET_OFFLINE=true \
  ZGRAPH_PYTHON=<repo>/.venv/bin/python cargo test --workspace --locked
# 58 个测试二进制；汇总：556 passed / 0 failed / 2 ignored；TEST_EXIT=0
```

- 与 `doc/test-baseline.txt` 比对：失败名额 0、2 ignored（harness 真模型 smoke + e2e AC-08b 手动）→ **无新增失败**。
- e2e 真链路（AC-12 同库并发、CLI run/replay、worker 进程组等）实测通过，说明依赖治理未影响跨模块运行时。

### 4.6 门禁、CI 与工作区

```bash
$ bash scripts/dev/check.sh
# OK 身份/remote/CLAUDE.md 未修改/暂存区无禁用文件；OK 分支命名: chore/process-m6-deps
# OK Python 语法；OK doc/progress.md 非空行 19（≤30）；OK cargo fmt --check；OK git diff --check
# CHECK PASSED（exit 0）
```

- CI diff（`.github/workflows/ci.yml:19-24`）：clippy → `cargo clippy --workspace --all-targets --locked -- -D warnings`；test → `cargo test --workspace --locked`；`fmt` 步骤未改；python job 未改。`--locked` 与仓库已跟踪的 `Cargo.lock` 配套成立。
- worktree 在评审前后、测试后均 clean；未生成待提交产物（e2e 使用临时目录）。

### 4.7 注释与声明真实性

- `Cargo.toml:31-34` 引用的缓解措施真实存在：`crates/zgraph-worker/src/supervisor.rs:395` 使用 `WorkerFailure::Worker(Box::new(error))`（REV-021）。
- 注释未声称 ISSUE-073「已修复」（仅记录接受现状 + 守卫 + 决策指针，与 DR-003 D2 一致）；提交 subject 与实测 diff 一致（`+ CI --locked`），无夸大。
- `Cargo.toml:38-39` 的 nix 平台说明与成员实际写法一致（均在 `[target.'cfg(unix)'.dependencies]`）。

## 5. 遗留问题

- **合并后台账动作（Lead）**：按 §3 #3 更新 `doc/issues.md`（068/078 → fixed；073 → wontfix + D2 理由）与 `doc/progress.md`（REV-045 完成）。本记录不改台账（AGENTS §7 分工 + 只允许新增评审文件）。
- **Nit**：#1 `ISS-073` → `ISSUE-073`；#2 genai caret/精确冻结口径（承接 REV-017 #1）——可在 process 收尾或下次依赖治理处理，不阻塞。
- **跨模块留痕**：#4 建议 Lead 在合并说明补一行跨模块机械改动 + 第二评审（REV-045）。
- 本记录只新增评审文件；未修改被评审代码/配置/CI/台账，未 merge/push。

## 6. 条件关闭记录

不适用（结论为「通过」，无待关闭条件）。
