# REV-006 · process · M2 双栈依赖预热（uuid/chrono/rusqlite + test-hooks）

> **跨模块声明**：本分支以 `process` 为主（根 `Cargo.toml`、`Cargo.lock`），另触及 `crates/zgraph-kernel/Cargo.toml`、`crates/zgraph-trace/Cargo.toml`、`crates/zgraph-cli/Cargo.toml`（kernel/trace/cli 三个模块的清单文件，未改任何 `src/**`）。属 Lead 指定的 M2 前置依赖预热，非静默越界；评审记录此声明。

- **分支**：`feat/process-m2-deps`
- **作者**：`lead`
- **评审人**：`reviewer-a`
- **变更范围**：`9aa0b9e..bfec987`（`git diff --stat` = 5 files, +399/−3：`Cargo.lock` +370/−3、`Cargo.toml` +3、`crates/zgraph-kernel/Cargo.toml` +14、`crates/zgraph-trace/Cargo.toml` +8、`crates/zgraph-cli/Cargo.toml` +4；无 `src/**`、无脚本/文档/测试改动）
- **评审时间**：2026-10-29
- **关联**：architecture-v2 §2.1（crate 布局）/§5.2（存储增量）/§6.1（M2 DoD）；spec 03 §3.2（`test_hooks` 仅测试构建可用）；spec 06 §6.1（`rusqlite` + `bundled`）；spec 00 §0.3（UUIDv7）/§0.4（UTC RFC3339）；ISSUE-030（依赖治理边界）

## 1. 结论

**结论：通过**

依据（关键点）：

1. **范围零越界**：`git diff origin/main..HEAD` 仅 5 个依赖清单/lock 文件，无产品代码、无测试、无文档改动；作者-评审人分离（作者 `lead`，评审人 `reviewer-a`）满足 AGENTS §3。
2. **声明与设计文档一致**：`uuid` 开 `v7` 对应 spec 00 §0.3 的 `RunId`/`InstanceId` = UUIDv7（`doc/spec-v1.2/00-总纲.md:37-38`）；`chrono` 开 `serde` 对应 00 §0.4 的 UTC RFC3339 时间戳（`00-总纲.md:24`）；`rusqlite` 0.41 + `bundled` 与 spec 06 §6.1 明文要求（`06-存储与可观测.md:28`）一致，`cargo tree` 实测 `libsqlite3-sys feature "bundled"` 生效。
3. **test-hooks 仅测试可用**：kernel 声明 `test-hooks = []` 并注释引用 spec 03 §3.2（`crates/zgraph-kernel/Cargo.toml:9-11`），spec 原文的 guard 正是 `#[cfg(any(test, feature = "test-hooks"))]`（`doc/spec-v1.2/03-调度器与生命周期.md:60-62`）；cli 通过 **dev-dependency** 启用该 feature（`crates/zgraph-cli/Cargo.toml:23`），实测正常依赖图 0 处 `test-hooks`、dev 依赖图仅 1 处，生产二进制不含故障注入入口。
4. **回归零影响**：`cargo test --workspace` = **99 passed / 0 failed**（与 REV-005 基线 99 一致，无新增失败/无减少）；`cargo fmt --check`、`cargo clippy --workspace --all-targets -D warnings`、`bash scripts/dev/check.sh`、pytest **20 passed** 全部通过。
5. **lockfile 可审计**：仅新增 38 个包，无既有包版本变更、无包删除；3 处 `- "syn",` 是新增 `chrono → iana-time-zone → windows-core → syn 2.x` 后 Cargo 对两个 `syn` 版本的消歧（`syn`、`syn 3.0.9`），非降级/替换。

发现的 4 项均为**建议级**（不阻塞合并）：cli dev-dependency 意图无文件内注释、trace 清单一处书写风格不一致、kernel 未预热 `zgraph-trace` 依赖方向、空依赖待 M2 落地后清理。均为可后置项，按任务要求由 **Lead 确认登记 ISSUE**（见 §5）。

## 2. 四维评审

| 维度 | 结论 | 依据（文件/行/测试） |
|---|---|---|
| 功能正确性 | ✅ | 依赖版本/feature 逐项对齐设计文档：`uuid v7`（`Cargo.toml:26` ↔ 00 §0.3）；`chrono serde`（`:27` ↔ 00 §0.4）；`rusqlite 0.41 bundled`（`:28` ↔ 06 §6.1）；kernel `test-hooks`（`zgraph-kernel/Cargo.toml:9-11` ↔ 03 §3.2）。实测 `cargo tree -p zgraph-cli -e features,normal` 无 `test-hooks`（0 行），`-e features,dev` 仅 1 行 `zgraph-kernel feature "test-hooks"` → “仅测试可用”成立；`cargo tree -p zgraph-trace -e features -i libsqlite3-sys` 显示 `bundled` 生效。全量 99 Rust 测试 + 20 pytest 全绿，行为无回归。 |
| 代码结构合理性 | ✅ | 依赖统一上提 `[workspace.dependencies]`（`Cargo.toml:26-28`），各 crate 用 `.workspace = true` 继承，符合 ISSUE-030 的“收敛到 workspace.dependencies”建议；kernel 依赖方向正确（kernel → dsl）；trace 保持独立（serde/serde_json/thiserror/chrono/rusqlite）；cli 新增 `zgraph-trace` 供 M2 `trace` 子命令（architecture §5.3/§6.1）；无重复实现、无 `src/**` 侵入。仅 1 处风格建议（trace 的 `{ workspace = true }` 写法）与 1 处注释建议（cli dev-dep）。 |
| 逻辑性 | ✅ | feature 解析自洽：`test-hooks` 只经 dev-dependency 边进入测试构建，无依赖环；`resolver = "2"` 下 dev-only feature 不会泄漏到 `cargo build -p zgraph-cli` 的依赖图（已用 `cargo tree -e normal` 实测）；`Cargo.lock` 变更纯增量（38 新增包、0 删除包），`syn` 双版本消歧由 Cargo 自动完成、可解释；没有 fake/跳过测试或为过测试而改测试的痕迹。 |
| 可读性 | ✅ | kernel 的 `test-hooks` feature 有“为什么仅测试可用”的注释并引用 spec 03 §3.2（符合“注释解释为什么”）；依赖按字母/逻辑分组清晰。改进项：cli 的重复 `zgraph-kernel` dev-dependency 缺注释（建议 #2）、trace 一行格式不统一（建议 #3），均为建议级，不改变可读性结论。 |

## 3. 发现清单

| # | 级别 | 位置 | 问题 | 建议 | 状态 |
|---|---|---|---|---|---|
| 1 | 建议 | `Cargo.toml:26-28`；`crates/zgraph-kernel/Cargo.toml:13-21`；`crates/zgraph-trace/Cargo.toml:10-14`；`crates/zgraph-cli/Cargo.toml:17,21-23` | 本分支为“预热”，新增依赖当前均无 `src/**` 使用点（kernel/trace/cli 全为骨架，`cargo test` 中 kernel/trace 各 0 个测试）。预热本身是任务目标，但空依赖长期保留会掩盖真实使用情况。 | M2 三条分支落地后逐项核对：未使用的依赖及时删除；给 kernel/trace 至少加 1 个冒烟测试（如 UUIDv7 生成 + 内存/临时库建表），让预热依赖在 CI 中被真实链接。 | open（建议） |
| 2 | 建议 | `crates/zgraph-cli/Cargo.toml:23` | dev-dependency 重复声明 `zgraph-kernel` 并开 `features = ["test-hooks"]`，意图（仅 cli 集成测试做故障注入/AC-11，生产构建不含）只能从 Cargo 语义推断，文件内无说明；后来者易误以为重复声明可删，或误提升到 `[dependencies]`，前者破坏 M2 测试、后者把故障注入带进生产二进制。 | 在该行上方加一行注释，例如“仅测试构建启用 test-hooks（AC-11 死锁/超时注入）；生产依赖见 [dependencies]”。可并入 M2 kernel 分支实施。 | open（建议，待 Lead 确认登记 ISSUE） |
| 3 | 建议 | `crates/zgraph-trace/Cargo.toml:14` | 同一文件其余依赖用 `serde.workspace = true` 简写，唯独 `rusqlite = { workspace = true }` 用表形式，风格不一致（语义等价，仅可读性）。 | 统一为 `rusqlite.workspace = true`（若 M2 需在 crate 侧追加 feature，可写作 `rusqlite = { workspace = true, features = ["..."] }` 并按需加注释）。 | open（建议） |
| 4 | 建议 | `crates/zgraph-kernel/Cargo.toml:13-21`；`Cargo.toml:26` | 两点 M2 预留完备性：(a) kernel 未声明 `zgraph-trace` 依赖，而 architecture §2.1 把 `TraceEvent` 归入 trace（`doc/architecture-v2.md:48`）、spec 06 §6.1.4 中内核是全部不可丢事件的生产者——若内核直接构造 `TraceEvent`，依赖方向应为 kernel → trace，需在 M2 kernel 分支补声明并避免 trace → kernel 反依赖；(b) `uuid` 仅开 `v7`，若 M2 需要在 serde 结构体中序列化 `Uuid`（如 test-hooks/回放载荷），还需补 `serde` feature。 | 在 M2 kernel 开工前明确 `TraceEvent` 归属与依赖方向（architecture §2.1 已定：trace 侧定义），由 kernel 分支补 `zgraph-trace` 依赖；`uuid` 的 `serde` feature 按实际需要再加，不提前扩大。 | open（建议，M2 分支闭环） |

> 级别判定：无阻塞项、无重要项；4 项均为建议级，不构成合并条件。合并后可保持本记录为“通过”。

## 4. 验证证据

### 4.1 变更范围与越界检查

```bash
cd /Users/zyzs/lczyzs/code/zgraph/.worktrees/process-lead
git log --oneline -2
# bfec987 feat(process): 预热 M2 双栈依赖与 crate 依赖声明（REV-006）
# 9aa0b9e docs(review): REV-005 复审通过（34e6cc3）
git merge-base origin/main HEAD
# 9aa0b9e6d796b5654f8422853e53e8baa44730f6（= origin/main，基线最新）
git diff --numstat origin/main..HEAD
# 370  3   Cargo.lock
#   3  0   Cargo.toml
#   4  0   crates/zgraph-cli/Cargo.toml
#  14  0   crates/zgraph-kernel/Cargo.toml
#   8  0   crates/zgraph-trace/Cargo.toml
git diff origin/main..HEAD --name-only | grep -vE 'Cargo\.(toml|lock)$' || echo "(仅有 Cargo.toml/Cargo.lock)"
# (仅有 Cargo.toml/Cargo.lock)  → 无 src/tests/scripts/doc 改动
git diff origin/main..HEAD --check && echo "DIFF_CHECK_OK"          # DIFF_CHECK_OK
git -C /Users/zyzs/lczyzs/code/zgraph rev-parse --short main origin/main
# 9aa0b9e 9aa0b9e   → main 未被本分支改动
```

提交信息：`Author: chenyuxiangg <1007423165@qq.com>`，正文含 `Review: REV-006`、`Agent: lead`（符合 AGENTS §1）。

### 4.2 必跑命令（worktree 内，`CARGO_HOME=ROOT/.cargo-home`）

```bash
cargo test --workspace
# cli: 7(unittests) + 8(validate) = 15
# dsl: 1(unittests) + 5(fixtures) + 7(hash) + 55(rules) + 1(doctest) = 69
# worker: 7(unittests) + 8(echo_roundtrip) = 15
# kernel/trace/harness: 0（骨架）
# 合计：test result 全 ok → 99 passed / 0 failed
cargo fmt --all -- --check                 # exit 0（FMT_OK）
cargo clippy --workspace --all-targets -- -D warnings   # exit 0（Finished dev profile）
bash scripts/dev/check.sh                  # 13 项 OK → CHECK PASSED（身份/remote/CLAUDE.md/禁用文件/分支名/py 语法/进度/台账/fmt/diff-check）
/Users/zyzs/lczyzs/code/zgraph/.venv/bin/python -m pytest
# 20 passed in 0.58s（python/ 全部测试；基线文件无失败项）
```

### 4.3 依赖/feature 实测

```bash
cargo tree -p zgraph-cli -e features,normal --prefix none | grep -c "test-hooks"
# 0    → 正常构建依赖图不含 test-hooks
cargo tree -p zgraph-cli -e features,dev --prefix none | grep "test-hooks"
# zgraph-kernel feature "test-hooks"     → 仅 dev/测试图出现
cargo tree -p zgraph-trace -e features -i libsqlite3-sys
# libsqlite3-sys v0.39.1 ├── feature "bundled" └── rusqlite feature "bundled" └── zgraph-trace
cargo tree -p zgraph-trace --depth 1
# chrono 0.4.45 / rusqlite 0.41.1 / serde 1.0.229 / serde_json 1.0.154 / thiserror 2.0.21；dev: tempfile 3.27.0
cargo tree -p zgraph-cli -e features -i zgraph-kernel
# kernel feature "default"（[dependencies]）+ kernel feature "test-hooks"（[dev-dependencies]，来自 zgraph-cli）
rustc --version && cargo --version   # rustc 1.99.1 / cargo 1.99.1
```

### 4.4 Cargo.lock 审计

```bash
git diff origin/main..HEAD -- Cargo.lock | grep -c '^+name = '   # 38 个新增包
git diff origin/main..HEAD -- Cargo.lock | grep -E '^-' | grep -v '^---'
# - "syn",   # ×3
# 仅 3 行删除，均为新增 syn 2.x 后对同名多版本的消歧（→ "syn 3.0.9"），无包被删除/降级
```

- 新增包来自三条依赖链：`uuid → getrandom/wasm-bindgen`、`chrono → iana-time-zone/windows-*`、`rusqlite → libsqlite3-sys/sqlite-wasm-rs/fallible-*`；`sqlite-wasm-rs`/`windows-*` 等为跨目标平台的条件依赖，macOS 本地不编译不影响锁文件正确性。
- 无 `[patch]`/`git` 依赖、无版本回退、无与设计文档冲突的 feature。

### 4.5 手工核对

| 核对项 | 期望 | 实际 |
|---|---|---|
| 分支是否基于最新 origin/main | base = `9aa0b9e` | ✅ merge-base 一致 |
| 是否越界改产品代码/测试/文档 | 仅 5 个清单/lock 文件 | ✅ 无 `src/**`、`tests/**`、`doc/**` 改动 |
| `uuid` feature | v7（spec 00 §0.3 UUIDv7） | ✅ `features = ["v7"]`，锁文件 1.29.0 |
| `chrono` feature | serde（00 §0.4 RFC3339 落库） | ✅ `features = ["serde"]`，0.4.45 |
| `rusqlite` 版本/feature | spec 06 §6.1 `rusqlite` + `bundled` | ✅ 0.41 + bundled，`libsqlite3-sys bundled` 生效 |
| `test-hooks` 仅测试可用 | 正常图无、dev 图有 | ✅ 0 / 1（见 4.3） |
| kernel feature 注释与 spec 03 §3.2 一致 | `#[cfg(any(test, feature = "test-hooks"))]` | ✅ 注释明确引用，feature 名与 spec 完全一致 |
| 回归测试数字 | 与 REV-005 基线 99 一致 | ✅ 99 passed / 0 failed；pytest 20 passed |
| worktree/分支状态 | 本评审提交前干净、未 push | ✅ `git status` 空；未改 main（仍 9aa0b9e） |

## 5. 遗留问题

- **无阻塞项**。本分支为依赖预热，不含产品代码；M2 实现由 kernel/trace/cli 三条分支分别在各自 worktree 落地。
- **已登记关联**：ISSUE-030（`blake3` 等依赖治理边界；本分支把 `uuid/chrono/rusqlite` 提升到 `[workspace.dependencies]`，符合其“由 process 统一提升”的建议方向）。
- **待 Lead 确认登记（建议级，可在合并后补台账）**：
  - 建议 #2：cli dev-dependency `zgraph-kernel features = ["test-hooks"]` 的文件内注释（避免误删/误提升；编号由 Lead 分配）；
  - 建议 #3：trace `rusqlite` 书写风格统一；
  - 建议 #4：kernel `zgraph-trace` 依赖方向与 `uuid` serde feature 的 M2 预留（在 M2 kernel 分支闭环）；
  - 建议 #1：M2 落地后清理未使用依赖并加冒烟测试。
- **流程项**：本分支未更新 `doc/progress.md`（仍为 process=M0 done）；按 AGENTS §5 由 Lead 在合并时统一更新，不影响本次评审结论。
- 未 push、未 merge、未改 `main`；评审记录仅提交到本分支 worktree。

## 6. 条件关闭记录（仅“有条件通过”填写）

不适用：本记录结论为 **通过**，无前置条件。
