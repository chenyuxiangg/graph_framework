# REV-055 · e2e · M6 W3：ISSUE-086 实例级超时断言加强 + `validate` 退出码回归

> **模块声明**：本分支只改 `tests/e2e/**`（`e2e` 模块：`cli_ac01.rs`、`cli_replay.rs`、`harness_ac08.rs`、`timeout_kill.rs`），无跨模块改动；作者 `e2e-dev`，评审人 `reviewer-a`（**非作者**）。评审人**未修改任何被评审代码/测试**，只新增本记录；未 merge / 未 push / 未改 main。
>
> **编号与合并顺序**：按 REV-046 W1 收口后的 M6 编号，本分支 = **REV-055**（DR-003 §2 W3 `test/e2e-m6-final-asserts`；W1 收口=REV-046，W2/W3=REV-047–058）。评审时 `origin/main` 已前进至 `117557f`（REV-046 已合并）；本分支基于其父 `67bc49b`、落后 4 commits（均为 docs 改动），`merge.sh` 会 rebase 后合并，rebase 后 DR-003 的 e2e-final 编号与本记录一致。评审范围按 **merge-base `67bc49b..762cb23`** 计。

- **分支**：`test/e2e-m6-final-asserts`
- **作者**：`e2e-dev`
- **评审人**：`reviewer-a`（**非作者**）
- **变更范围**：`67bc49b..762cb23`，**1 commit / 4 files changed, 96 insertions(+), 10 deletions(-)**，全部为 `tests/e2e/**`：`cli_ac01.rs`（+30/−2，新增 `validate` 退出码回归）、`cli_replay.rs`（+15，CLI 自报超时路径补实例级断言）、`harness_ac08.rs`（+53/−10，ISSUE-086 实例错误断言 + `elapsed_ms` 同源校验）、`timeout_kill.rs`（+8，watchdog 路径实例错误 JSON 断言）；无产品代码/`Cargo.toml`/doc 改动。
- **评审时间**：2026-10-31
- **关联**：`doc/issues.md` ISSUE-086（e2e 实例 `NODE_TIMEOUT` 断言，REV-040 内核修复）、ISSUE-059（REV-043 `validate` 退出码）；DR-003 §2 W3、§4；spec 06 §6.2.5/§6.2.5.1/§6.10.5、03 §3.9、07 AC-06；REV-040（kernel）、REV-043（cli）；ISSUE-091/093（测试环境口径）

## 1. 结论

**结论：通过**

改动与任务范围一致且**只增强、不弱化**测试：① ISSUE-086 的 e2e 断言从「只看 run 级 `error_code` + `NodeTimedOut` 事件」升级为**实例级** `instances.error_code=NODE_TIMEOUT` + `instances.error` JSON（`code`/`kind`/非空 `message`），覆盖**两条超时路径**——Harness 自报超时（`harness_ac08.rs`、`cli_replay.rs` CLI 端到端）与内核 watchdog（`timeout_kill.rs`）；并新增 `elapsed_ms` 同源校验（实例错误 ↔ `NodeTimedOut` 事件），验证落库路径一致。② `validate` 退出码按 06 §6.10.5 补真二进制回归：合法=0、DSL 坏图=2、文件不可读=1（REV-043 记录曾注明「e2e 无 validate 退出码断言」，本分支闭合该缺口）。③ 断言依赖经代码核对成立（`node_error_json` 对 `Timeout` 输出 `kind`/`elapsed_ms`；`fail_timeout()` 与 watchdog 路径同源回填），测试函数 ≤47 行、无跳过/无恒真断言。

**独立验证**：changed 4 个测试二进制 **20 passed / 0 failed / 1 ignored（`ac08b_real_llm_manual`）**、exit 0；`check.sh` PASS、worktree clean；本分支自身的 clippy 在排除**主干既有**两处 lint 后 exit 0（见下）。

**需 Lead 立即处理的跨分支问题（不阻塞本分支）**：`cargo clippy --workspace --all-targets --locked -- -D warnings`（CI `ci.yml:20` 门禁）在 base/main 上**失败**，2 处均来自 W1 已合并代码：`crates/zgraph-harness/src/config.rs:848-849`（`needless_borrows_for_generic_args`，REV-042/`b64d61e`）与 `crates/zgraph-cli/src/lib.rs:161`（`unnecessary_map_or`，REV-043/`eb59983`）。即 REV-045 固化的「CI clippy `-D warnings` 守卫」在 main 上未绿；与本分支无关（本分支仅 tests/e2e/**），需 Lead 另开小修复分支并登记台账（§3 #1）。

## 2. 四维评审

| 维度 | 结论 | 依据（文件/行/命令） |
|---|---|---|
| 功能正确性 | ✅ | ① `harness_ac08.rs:746` 新增 `assert_timeout_instance_error`：TimedOut + `instances.error_code=NODE_TIMEOUT` + `error["code"]/["kind"]/message` 非空；`:779` 在 `assert_timeout_hint_trace` 调用；`:794` 断言 `instance_error["elapsed_ms"] == NodeTimedOut.payload["elapsed_ms"]`（内核 `fail_timeout()` 先 `node_timed_out(elapsed)` 再 `set_instance_error(Timeout{elapsed})`，同源成立）。② `cli_replay.rs:652-672` 在 CLI 端到端 `cli_agent_config_timeout_hint_is_end_to_end` 的 trace 断言中补实例级错误（同一 `worker` 实例），覆盖 CLI 真链路。③ `timeout_kill.rs:65-69` 对 watchdog 路径断言实例错误 JSON（`code`/`kind`=Timeout）。④ `cli_ac01.rs:266` `validate_exit_codes_match_spec`：合法 0 / `bad_version.json` 2 / 缺文件 1，与 06 §6.10.5 及 `crates/zgraph-cli/src/validate.rs:56-67` 实现一致。⑤ 独立跑测 20 passed/0 failed/1 ignored。 |
| 代码结构合理性 | ✅ | 4 个文件全部在 `tests/e2e/**`（e2e 模块边界，AGENTS §2）；复用既有 helper（`common::instance`/`assert_exit`/`run_cli`/`ac01_graph`），无重复造轮子；实例断言抽成 `assert_timeout_instance_error` 供两条路径共用（`:746`），`cli_replay` 保持最小增量；未改产品代码/`Cargo.toml`/fixtures，无跨模块夹带。 |
| 逻辑性 | ✅ | ① 断言**加强**而非弱化：删除的是「自报路径当前不落 instances.error_code（e2e 不改）」的过期限制注释，替换为实例级强断言；无 `#[ignore]` 新增、无跳过、无恒真断言。② 双路径覆盖完整：自报（harness/cli replay）+ watchdog（timeout_kill）均断言实例错误；`elapsed_ms` 同源断言能真正区分「两条路径各自落不同值」的缺陷。③ 无并发/时序脆弱点新增（不引入 sleep/竞态断言）；测试函数长度 13/24/25/26/47 行，均 ≤60。 |
| 可读性 | ✅ | 新增注释解释「为什么」（ISSUE-086/06 §6.2.5、两条路径同口径、`elapsed_ms` 同源），命名与既有测试一致（`assert_*` helper、`*_exit_codes_*`）；错误信息带上下文（`ISSUE-086：…`）。测试运行使用**私有** `CARGO_TARGET_DIR=<repo>/target/rev055`（ISSUE-093 口径），未污染共享 target，测试后 worktree clean。 |

## 3. 发现清单

| # | 级别 | 位置 | 问题 | 建议 | 状态 |
|---|---|---|---|---|---|
| 1 | **重要（跨分支/主干，非本分支引入）** | `crates/zgraph-harness/src/config.rs:848-849`（REV-042/`b64d61e`）；`crates/zgraph-cli/src/lib.rs:161`（REV-043/`eb59983`）；对照 `.github/workflows/ci.yml:20` | `cargo clippy --workspace --all-targets --locked -- -D warnings`（CI clippy 门禁）在 base `67bc49b`/main `117557f`（仅 docs 差异）上**失败**：① harness `canonical_or_lexical(&assets_root)`/`(&cwd)` → `needless_borrows_for_generic_args`（去掉 `&`）；② cli `var_os("RUST_LOG").map_or(true, …)` → `unnecessary_map_or`（改 `is_none_or`）。W1 已合并代码导致 CI 红，REV-045 的 clippy 守卫未真正生效（merge.sh 不跑 clippy，故未被门禁拦下） | 开**小修复分支**（harness+cli 两行，需按 AGENTS §2 声明跨模块 + 第二评审）修 2 处并在 CI 复跑；同时登记 ISSUE（CI clippy 红 / W1 合并未验 clippy）。**不阻塞本分支**（本分支只改 tests/e2e/**，未引入该问题，但合并后仍会红） | open（需 Lead 另处理） |
| 2 | 建议（覆盖增强） | `tests/e2e/cli_ac01.rs:266-279` | `validate` 回归只断言退出码 0/2/1，未断言 `--json` 的 `ok=false`/`errors[].code` 形状（既有 AC-04 已覆盖错误码，本处为退出码最小回归） | 可保持最小回归；若后续 W3/wrapup 需要，可补一条 `--json` 断言（`DSL_*` code + `ok=false`）。不阻塞 | open（不阻塞） |
| 3 | 备注（合并顺序） | `762cb23` 分支基点 `67bc49b`；`origin/main` = `117557f` | 本分支落后 origin/main 4 commits（REV-046 收口，仅 docs）。`merge.sh` 会 rebase 后 ff 合并；REV-055 编号对应 rebase 后的 DR-003 §2（e2e-final=REV-055） | 先合并 REV-046（已完成），再合本分支；rebase 后复跑门禁即可。编号无需改 | open（Lead 执行） |
| 4 | 备注（环境） | 测试执行 | 按 ISSUE-093 使用私有 `CARGO_TARGET_DIR=<repo>/target/rev055`（不复用共享 target），测试后 worktree clean、未生成提交物 | 无动作；建议 merge 前按 ISSUE-093 对该分支 target 做一次 clean | 无需处理 |

> 本分支**无阻塞项**，结论「通过」；#1 是主干既有缺陷，需 Lead 另行修复/登记，不影响本分支自身的正确性。

## 4. 验证证据

### 4.1 范围、分支与提交元数据（worktree `.worktrees/e2e-e2e-dev`）

```bash
$ git status --porcelain                  # 空（评审前后均 clean）
$ git branch --show-current               # test/e2e-m6-final-asserts
$ git merge-base origin/main HEAD         # 67bc49bdf5368cbab0b557886ae211b6ba349f96
$ git rev-parse origin/main               # 117557f6136ee3fc82e528a8b995ca8379a41a8d（REV-046 已合并）
$ git rev-list --count HEAD..origin/main  # 4（落后，merge.sh 会 rebase）
$ git diff 67bc49b..HEAD --name-status
# M tests/e2e/cli_ac01.rs
# M tests/e2e/cli_replay.rs
# M tests/e2e/harness_ac08.rs
# M tests/e2e/timeout_kill.rs          ← 4 文件，全部 tests/e2e/**；无代码/Cargo/doc
$ git diff 67bc49b..HEAD --stat
# 4 files changed, 96 insertions(+), 10 deletions(-)
$ git log -1 --format='%H%n%an <%ae> | %cn <%ce>%n%s%n%n%B'
# 762cb230c50401274bb9735dac96c7fc02ebde7c
# chenyuxiangg <1007423165@qq.com> | chenyuxiangg <1007423165@qq.com>
# test(e2e): ISSUE-086 实例 NODE_TIMEOUT 断言加强 + validate 退出码回归
# 正文：Review: REV-055 / Agent: e2e-dev（符合 AGENTS §1）
$ bash scripts/dev/check.sh
# OK 身份/remote/CLAUDE.md 未修改/暂存区无禁用文件；OK 分支命名: test/e2e-m6-final-asserts
# OK doc/progress.md 非空行 18（≤30）；OK cargo fmt --check；OK git diff --check
# CHECK PASSED（exit 0）
```

### 4.2 测试执行（私有 target，规避 ISSUE-093）

```bash
$ CARGO_HOME=<repo>/.cargo-home CARGO_NET_OFFLINE=true \
  CARGO_TARGET_DIR=<repo>/target/rev055 ZGRAPH_PYTHON=<repo>/.venv/bin/python \
  cargo test -p zgraph-e2e --locked \
    --test cli_ac01 --test timeout_kill --test harness_ac08 --test cli_replay
     Running cli_ac01.rs    … test result: ok. 4 passed; 0 failed; 0 ignored  （含新增 validate_exit_codes_match_spec ✓）
     Running cli_replay.rs  … test result: ok. 9 passed; 0 failed; 0 ignored  （含 cli_agent_config_timeout_hint_is_end_to_end ✓）
     Running harness_ac08.rs… test result: ok. 5 passed; 0 failed; 1 ignored  （含 agent_config_node_timeout_hint_backs_off_when_dsl_omits_timeout ✓；ignored = ac08b_real_llm_manual）
     Running timeout_kill.rs… test result: ok. 2 passed; 0 failed; 0 ignored  （含 ac06_timeout_kills_process_group_including_grandchild ✓）
# TEST_EXIT=0；合计 20 passed / 0 failed / 1 ignored
```

### 4.3 断言可成立性（代码核对，不只信测试结果）

- `node_error_json`（`crates/zgraph-cli/src/bridge.rs:548-570`）对 `NodeError::Timeout` 输出 `{code:"NODE_TIMEOUT", message, kind:"Timeout", elapsed_ms}` → 测试读取 `error["code"]/["kind"]/["elapsed_ms"]` 成立。
- `fail_timeout()`（`crates/zgraph-kernel/src/run.rs:909-923`）：先 `node_timed_out(…elapsed_ms…)`、再 `set_instance_error(NodeError::Timeout{elapsed_ms})` → **同源**断言（`harness_ac08.rs:794`）成立；执行器自报 `Timeout` 仅对 `agent_call` 走该路径（`run.rs:877,889`）。
- watchdog 路径（`run.rs:1357-1360`）同样 `node_timed_out` + `set_instance_error(Timeout)` → `timeout_kill.rs` 断言成立。
- `validate` 退出码实现：`crates/zgraph-cli/src/validate.rs:56-67`（0 合法 / 2 DSL·资产 / 1 IO 不可读）与 06 §6.10.5 一致；`tests/fixtures/graphs/bad_version.json` 存在。

### 4.4 clippy（本分支 vs 主干）

```bash
# ① 本分支改动范围（排除主干既有 2 处 lint 后）：exit 0 / Finished，无新增 warning
$ cargo clippy -p zgraph-e2e --all-targets --locked -- \
    -A clippy::needless_borrows_for_generic_args -A clippy::unnecessary_map_or -D warnings
# Finished `dev` profile … in 10.79s（exit 0）

# ② 全 workspace（CI 门禁命令）在 base 上失败 —— 均为 W1 已合并代码，非本分支引入
$ cargo clippy --workspace --all-targets --locked -- -D warnings
# error: the borrowed expression implements the required traits  --> crates/zgraph-harness/src/config.rs:848:31 / :849:35
# error: this `map_or` can be simplified                      --> crates/zgraph-cli/src/lib.rs:161:8
# （分别来自 REV-042 `b64d61e`、REV-043 `eb59983`；origin/main 117557f 仅 docs 差异，同样失败）
```

### 4.5 变更性质（无弱化/无夹带）

- `git diff` 逐行核对：仅新增断言与注释；被删除的 10 行均为旧注释/旧内联断言（迁移到 helper 或替换为更强断言），**无**删除既有断言、**无**新增 `#[ignore]`/跳过、**无**改测试入参以规避失败。
- 新增/修改函数行数：`validate_exit_codes_match_spec` 13、`assert_timeout_instance_error` 26、`assert_timeout_hint_trace` 24、`assert_hint_trace` 25、`assert_timeout_trace` 47（均 ≤60）。

## 5. 遗留问题

- **本分支无未关闭条件**：结论「通过」；#2–#4 为建议/备注，不阻塞。
- **需 Lead 立即处理（主干，非本分支）**：§3 #1 的 2 处 clippy `-D warnings` 失败（harness/cli，W1 合并回退），建议开小修复分支 + 登记 ISSUE；否则 CI clippy 门禁持续红，REV-045 的 clippy 固化未达成。修复后建议在 `doc/issues.md` 记录并复跑 CI。
- **合并顺序**：先 REV-046（已合并），再本分支；`merge.sh` rebase 后复跑门禁。
- 本记录只新增评审文件；未修改 `tests/e2e/**` 或其它被评审内容，未 merge/push。

## 6. 条件关闭记录

不适用（结论为「通过」，无待关闭条件）。
