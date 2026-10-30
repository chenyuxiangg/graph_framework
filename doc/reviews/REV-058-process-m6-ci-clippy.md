# REV-058 · process · M6 CI/clippy 门禁 + main cli lint + `refactor` 类型 + ISSUE-094

> **模块声明（跨模块）**：本分支改 `scripts/dev/merge.sh`、`scripts/dev/worktree.sh`（`process`）、`crates/zgraph-cli/src/lib.rs`（`cli`，1 行 lint 修复）、`doc/development-workflow.md`、`doc/issues.md`（`docs`）——属 **process + cli + docs 跨模块**改动。作者 `lead`；Lead 已在任务中批准并指定 `reviewer-a` 作**非作者**评审人（AGENTS §2/§3）。评审人**未修改任何被评审内容**，只新增本记录；未 merge / 未 push / 未改 main。
>
> **编号说明（备注）**：DR-003 §2（`doc/decisions/DR-003-m6-scope-and-waves.md:59`）把 **REV-058** 预留给 `docs/process-m6-wrapup`；本分支按 Lead 指派占用 REV-058 作 CI/clippy 门禁，M6 最终 wrapup 需顺延为 **REV-059**（或由 Lead 更新编号表/进度口径，见 §5）。
>
> **自举说明**：合并本分支时执行的是 main 上的**旧** `merge.sh`（不含 clippy 步骤），新门禁自合并后生效；CI 已可拦截同类回归，无需额外动作（`development-workflow.md` §8 自举两阶段）。

- **分支**：`chore/process-m6-ci-clippy`
- **作者**：`lead`
- **评审人**：`reviewer-a`（**非作者**；独立评审 + 跨模块第二评审，见 §6）
- **变更范围**：`origin/main`（`b7faaa6`）..`20dbdc2`，**1 commit / 5 files changed, 12 insertions(+), 4 deletions(-)**：`scripts/dev/merge.sh`（+5 clippy 门禁）、`crates/zgraph-cli/src/lib.rs`（1 行 `unnecessary_map_or` 修复）、`scripts/dev/worktree.sh`（TYPES 加 `refactor`）、`doc/development-workflow.md`（type 白名单 + clippy 门禁 + ISSUE-093 构建缓存约定）、`doc/issues.md`（ISSUE-094 → fixed）；无 Cargo.lock/其它 crate/doc 改动。**基点说明**：评审时 `origin/main = b7faaa6`；评审后 REV-047 已合并至 `b39374a`（3 commits），本分支落后、`merge.sh` 会 rebase 后合并（REV-047 改动 kernel `run/trace/types` + cli `bridge/run_cmd/tests`，与本分支的 cli `lib.rs`/scripts/docs **无文件交集**，预计 rebase 无冲突；rebase 后如 diff 变化需复核）。
- **评审时间**：2026-10-31
- **关联**：ISSUE-094（新增）、ISSUE-093、REV-043（cli lint 来源）、REV-053（harness lint 修复）、AGENTS §1/§2/§10、`.github/workflows/ci.yml:20`、DR-003 §2

## 1. 结论

**结论：通过**

四项改动逐项核对正确：① `merge.sh` 在 `cargo test` 之后、Python 基线之前新增 `cargo clippy --workspace --all-targets --locked -- -D warnings` 门禁，失败即 `die`（fail-fast），命令与 CI `ci.yml:20` 完全一致（含 `--locked`），且在 `set -euo pipefail` 下以 `|| die` 显式拦截；② cli 一行 `map_or(true, |v| v.is_empty())` → `is_none_or(|v| v.is_empty())` **语义完全等价**（`None`→true、`Some("")`→true、`Some(非空)`→false，即显式非空 `RUST_LOG` 仍优先不被覆盖），MSRV 1.99 ≥ `is_none_or` 稳定版 1.82；③ ISSUE-094 描述属实（base `b7faaa6` 上 clippy 失败只剩 cli 一处；harness 2 处已由 REV-053 代码提交 `c85cccc` 修复），`fixed` 有据（本分支门禁 + lint 修复）；④ 文档与 ISSUE-093/AGENTS §1 一致（`refactor` 加入 TYPES 与用法行、CARGO_TARGET_DIR 不得共用、clippy 门禁自跑约定）。

**独立验证**：`bash -n` 两脚本 OK、`check.sh` PASS、worktree clean；私有 target 下 `cargo clippy --workspace --all-targets --locked -- -D warnings` **exit 0**（36.29s，含全部 workspace 成员；Lead 另在干净 target 复跑 9.19s exit 0）；`worktree.sh new ... --type refactor --dry-run` 接受、非法 type 拒绝；台账 001–094 连续、8 列、ISSUE-094 为 P2 fixed。未发现阻塞/重要项；3 条建议 + 2 条备注见 §3，均不阻塞合并。

## 2. 四维评审

| 维度 | 结论 | 依据（文件/行/命令） |
|---|---|---|
| 功能正确性 | ✅ | ① `merge.sh:81-85`：clippy 门禁位于 Cargo 分支内、`cargo test`（`:78-80`）之后、Python 基线（`:90+`）之前；命令 `cargo clippy --workspace --all-targets --locked -- -D warnings` 与 `ci.yml:20` 逐字一致；`|| die "cargo clippy 失败（-D warnings）"` 保证失败即阻断，且位于 `--dry-run` 退出（`:115`）之前 → dry-run 也会执行该门禁。② `lib.rs:161` 的 `is_none_or` 与 `map_or(true, …)` 真值表等价（见 §4.4），显式非空 `RUST_LOG` 优先语义不变（`:152-156` 文档注释）。③ ISSUE-094 条目（`doc/issues.md:102`）的现状/影响/证据与实测一致。④ `worktree.sh:13` TYPES 与 `:4` 用法行加 `refactor`，与 AGENTS §1 类型表（已含 refactor）对齐，实测 dry-run 接受。 |
| 代码结构合理性 | ✅ | 各文件改动落在其职责边界：门禁入 `merge.sh`（process）、类型白名单入 `worktree.sh`（process）、`refactor` 类型/门禁/缓存约定入 `development-workflow.md`（docs）、台账一条入 `issues.md`（docs）、cli 仅 1 行机械 lint 修复（无 API/逻辑面变化）；diff 最小（+12/−4），无无关改动/夹带；跨模块（process+cli+docs）已由 Lead 批准并指定非作者评审（本记录 §6）。 |
| 逻辑性 | ✅ | ① 门禁顺序合理：先 `cargo test`（功能）再 `clippy`（静态质量），任一失败即 `die`，不会进入 rebase 之后的合并/清理分支；`--locked` 与仓库跟踪的 `Cargo.lock` 配套（本分支未改 lock，`cargo clippy --locked` exit 0）。② `--dry-run` 在门禁之后才退出（`:115`），使 dry-run 能完整预演门禁，符合「dry-run 只检查不合并」语义。③ cli 修改只消除 lint，不改 `apply_verbosity` 的调用条件/环境写入时机；`is_none_or` 在 MSRV 内。④ `refactor` 加入 TYPES 后，分支名 `<type>/<module>-<slug>` 解析与 usage/文档三处一致（AGENTS §1 亦已列 refactor）。 |
| 可读性 | ✅ | 门禁注释标注「与 CI `ci.yml` 对齐；ISSUE-094」，文档新增两条约定均为「为什么/怎么做」表述、含 ISSUE 编号；ISSUE-094 证据具体（门禁位置/命令/lint 位置）。Nit：`merge.sh` 步骤注释 `6a/6b/6c` 与执行顺序不一致（6c 实际在 Python 6b 之前，见 §3 #3）；`development-workflow.md:82` 的「脚本强制顺序」未列入 clippy（见 §3 #1）。 |

## 3. 发现清单

| # | 级别 | 位置 | 问题 | 建议 | 状态 |
|---|---|---|---|---|---|
| 1 | 建议（文档一致性） | `doc/development-workflow.md:82` | §4「脚本强制顺序」仍为「…→ 测试无新增失败 → `--ff-only` 合并」，未包含本分支新增的 clippy 门禁；与 §2 新注记（`:51`）及 `merge.sh` 实际顺序不一致 | 改为「…→ cargo test 无新增失败 → **clippy `-D warnings`** → `--ff-only` 合并 …」（一处文本） | open（不阻塞） |
| 2 | 建议（门禁可诊断性） | `scripts/dev/merge.sh:81-85` | 只预检 `command -v cargo`，未预检 clippy 组件；若本机 rustup 未装 clippy，门禁会以 generic「cargo clippy 失败（-D warnings）」报错，不易定位环境问题 | 增加 `cargo clippy --version`（或 `rustup component list --installed | grep clippy`）预检，失败时提示「安装 rustup component clippy」；不阻塞 | open（不阻塞） |
| 3 | Nit（可读性） | `scripts/dev/merge.sh:74-85` | 步骤注释 `# 6a) Rust 测试` / `# 6b) Python 测试` / 新增 `# 6c) clippy` 的编号与执行顺序不符（6c 在 6b 之前实际执行） | 重排为 6a=cargo test、6b=clippy、6c=pytest，或改用无语义序号的简短标签 | open（不阻塞） |
| 4 | 建议（台账精度） | `doc/issues.md:102`（ISSUE-094 建议列） | fixed 证据末尾混入「`worktree.sh` 同步支持 `refactor` 前缀」——该改动与 ISSUE-094（clippy 门禁）无关，属 AGENTS §1 类型表对齐的顺带项；台账证据与问题本体不严格对应 | 证据聚焦「merge.sh 新增 clippy 门禁 + 修复 main 上 cli lint」，`refactor` 支持另注（或在提交说明中说明，不影响 fixed 结论） | open（不阻塞） |
| 5 | 备注（编号/收口） | `doc/decisions/DR-003-m6-scope-and-waves.md:59`、`doc/progress.md:20` | DR-003/进度把 REV-058 预留给 `docs/process-m6-wrapup`；本分支占用 REV-058 后，M6 最终 wrapup 需顺延 **REV-059**（否则两处编号冲突） | 合并后由 Lead 在 wrapup 分支/台账把 wrapup 编号改为 REV-059，并同步 DR-003 §2/§4 与 progress 的编号表述（本记录不代改） | open（合并后执行） |
| 6 | 备注（自举） | `scripts/dev/merge.sh` | 合并本分支时 main 上仍是旧 `merge.sh`（无 clippy 步骤）→ 本次合并不会执行新门禁；新门禁自合并后对后续分支生效 | 无需动作（`development-workflow.md` §8 已描述自举）；CI 已可拦截同类回归 | 无需处理 |
| 7 | 建议（可选对齐） | `scripts/dev/merge.sh:78` | 既有 `cargo test --workspace` 未加 `--locked`，而新 clippy 与 CI test 均带 `--locked`；门禁间口径不完全一致 | 可顺手把 cargo test 改为 `--locked`（与 CI 对齐、避免 lock 漂移），非本任务必需 | open（不阻塞） |

> 无阻塞/重要项；结论「通过」。#1–#4/#7 为建议/Nit，#5 为合并后的编号收口动作，#6 为自举说明。

## 4. 验证证据

### 4.1 范围、分支与提交元数据（worktree `.worktrees/process-lead-ci`）

```bash
$ git status --porcelain                  # 空（评审前后 clean）
$ git branch --show-current               # chore/process-m6-ci-clippy
$ git rev-list --count origin/main..HEAD  # 1
$ git rev-list --count HEAD..origin/main  # 0（评审时；评审后 REV-047 合并至 b39374a → 3，merge.sh rebase 后合并）
$ git diff origin/main..HEAD --name-status
# M crates/zgraph-cli/src/lib.rs
# M doc/development-workflow.md
# M doc/issues.md
# M scripts/dev/merge.sh
# M scripts/dev/worktree.sh
$ git diff origin/main..HEAD --stat
# 5 files changed, 12 insertions(+), 4 deletions(-)
$ git log -1 --format='%H%n%an <%ae> | %cn <%ce>%n%s%n%n%B'
# 20dbdc2e114b21cbba2e47dd914ab2e40170b3d3
# chenyuxiangg <1007423165@qq.com> | chenyuxiangg <1007423165@qq.com>
# chore(process): merge 门禁新增 clippy -D warnings、修 main 上 cli lint、worktree 支持 refactor、登记 ISSUE-094
# 正文：Review: REV-058 / Agent: lead（符合 AGENTS §1）
$ bash -n scripts/dev/merge.sh && bash -n scripts/dev/worktree.sh   # 语法 OK
$ bash scripts/dev/check.sh
# OK 身份/remote/CLAUDE.md 未修改/暂存区无禁用文件；OK 分支命名: chore/process-m6-ci-clippy
# OK doc/progress.md 非空行 18（≤30）；OK cargo fmt --check；OK git diff --check → CHECK PASSED
```

### 4.2 merge.sh 门禁语义（逐行核对 + 与 CI 对齐）

- 位置：Cargo 分支内、`cargo test`（`merge.sh:78-80`）之后、Python 基线（`:90+`）之前；`--dry-run` 退出点（`:115`）在其后 → dry-run 也会跑该门禁。
- 失败即停：`( cd "${branch_wt}" && cargo clippy … ) || die "cargo clippy 失败（-D warnings）"`；脚本 `set -euo pipefail`（`:7`），不存在被 `||` 吞掉后继续合并的路径。
- 与 CI 一致：`grep .github/workflows/ci.yml` → `:20 run: cargo clippy --workspace --all-targets --locked -- -D warnings`，与 merge.sh 命令逐字相同。
- `--locked`：本分支未改 `Cargo.lock`；`cargo clippy --locked` 实测 exit 0（lock 与 manifest 一致）。

### 4.3 clippy 独立验证（私有 target，未共用/未用 Lead 的 merge target）

```bash
$ export CARGO_HOME=<repo>/.cargo-home CARGO_NET_OFFLINE=true
$ unset CARGO_TARGET_DIR                     # 使用 worktree 私有 target（.worktrees/process-lead-ci/target）
$ cargo clippy --workspace --all-targets --locked -- -D warnings
    Checking …（全 workspace 成员）
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 36.29s
# exit 0（CLIPPY_EXIT=0）；日志无 error/warning 行
```

- 对照：REV-055 评审时 base `67bc49b` 上同一命令失败于 harness `config.rs:848-849` + cli `lib.rs:161`；REV-053 代码提交 `c85cccc` 已修 harness（base `b7faaa6` 的 `config.rs:910-911` 已去掉多余 `&`），本分支修 cli 最后一处 → base 上唯一剩余 lint 消除，全量 clippy 转绿。
- Lead 另在干净 target 复跑 `cargo clippy --workspace --all-targets --locked -- -D warnings` → exit 0（9.19s），与本评审独立结果一致。

### 4.4 cli 一行改动等价性（真值表）

`apply_verbosity`（`crates/zgraph-cli/src/lib.rs:157-163`）仅在 `-v/-vv` 时写入 `RUST_LOG`；条件：

| 环境 `RUST_LOG` | 旧 `map_or(true, v.is_empty())` | 新 `is_none_or(v.is_empty())` | 行为 |
|---|---|---|---|
| 未设置（None） | `true` | `true` | 写入映射级别（不变） |
| `Some("")` | `true` | `true` | 写入映射级别（不变） |
| `Some("debug")`（显式非空） | `false` | `false` | **不覆盖**（用户优先，不变） |

- `Option::is_none_or` 自 Rust 1.82 稳定；workspace `rust-version = "1.99"`（`Cargo.toml`）→ MSRV 满足。
- 无 API/签名/调用点变化（仅 `apply_verbosity` 内部条件表达式），`-v/--verbose` 抽取与 `rust_log_for_verbosity` 未改。

### 4.5 worktree 类型 / 文档约定 / 台账

```bash
$ bash scripts/dev/worktree.sh new dsl agent-revtest tmp-check --type refactor --dry-run
# INFO: branch=refactor/dsl-tmp-check；INFO: dry-run：不创建（exit 0）
$ bash scripts/dev/worktree.sh new dsl agent-revtest tmp-check3 --type bogus --dry-run
# ERROR: 非法 type: bogus（可选：feat fix test docs chore refactor）
```

- `worktree.sh:13 TYPES` 与 `:4` usage 与 `doc/development-workflow.md:21` 三处一致，且与 AGENTS §1 `feat|fix|refactor|test|docs|chore` 对齐。
- 文档 `:51`「CI 与 merge.sh 均执行 clippy …」与 `ci.yml:20`、`merge.sh:81-85` 一致；`:52` 的 CARGO_TARGET_DIR 约定与 ISSUE-093 建议（「每 worktree 私有 target 或合并前 clean 成员」）逐句对应。
- `doc/issues.md`：94 行、ID 001–094 连续无重复、每行 8 列、级别/状态域合法；ISSUE-094 为 P2、`fixed`、日期 2026-10-31，描述中「cli `lib.rs` `unnecessary_map_or` 由 REV-043 引入；harness 2 处已由 REV-053 修复」经代码/历史核对属实。
- 改动文件的相对链接扫描：全部可达。

### 4.6 跨模块边界核对

- `git diff --name-only` 仅 5 文件，分属 process（2 脚本）/ cli（1 行）/ docs（2 文档），无其它 crate/Cargo.lock/spec 改动；每个改动均可独立归属、无越界夹带。
- cli 改动不改变 `zgraph` 任何对外行为（见 §4.4），process 脚本改动只影响开发/合并工具链、不进入产品路径，docs 改动为约定文本。

## 5. 遗留问题

- **本分支无未关闭条件**：结论「通过」；#1–#4/#7 为建议/Nit（可合并后顺手修），#5 为合并后编号收口（wrapup 顺延 REV-059 并同步 DR-003/progress），#6 为自举说明。
- **合并后 Lead 动作**：更新 `doc/progress.md`（REV-058 完成、wrapup 编号改 059）与 `doc/issues.md`（ISSUE-094 已在本分支置 fixed，复核即可）。
- 本记录只新增评审文件；未修改任何被评审代码/脚本/文档，未 merge/push。

## 6. 跨模块第二评审（reviewer-a）

> 本分支跨 **process + cli + docs** 三模块（AGENTS §2）；作者 `lead`，本记录由**非作者** `reviewer-a` 作独立评审（AGENTS §3）并承担跨模块第二评审（Lead 已批准并指定）。以下为跨模块接口/范围专项确认，结论：**通过**。

1. **cli ↔ 其它模块接口无变化**：`apply_verbosity` 为 cli 内部私有函数（`lib.rs:157`），改动仅替换等价条件表达式；无签名/导出/环境变量语义变化（§4.4 真值表），不引入新公共 API、不改 `RUST_LOG` 优先级契约；cli 单测/既有行为不受影响。
2. **process 脚本 ↔ 其它模块边界**：`merge.sh` 只新增门禁步骤（不改评审记录查找、rebase、测试、合并/清理主流程）；`worktree.sh` 只扩展 type 白名单（不改 worktree 路径/分支规则）。二者均不进入产品代码路径。
3. **docs 约定一致性**：type 白名单与 AGENTS §1 对齐；clippy 门禁文本与 CI/merge.sh 一致；CARGO_TARGET_DIR 约定与 ISSUE-093 一致；ISSUE-094 状态/证据与实现一致。
4. **跨模块范围与提交声明一致**：`git diff` 5 文件、+12/−4 与提交说明（clippy 门禁、cli lint、refactor、ISSUE-094）逐项对应；无隐藏改动。
5. **门禁自举**：合并本分支使用旧 `merge.sh`，新门禁合并后生效；CI 可独立拦截，属已知自举顺序，不构成阻塞。

> 跨模块第二评审结论：**通过**（与 §1 一致，无新增阻塞项）。
