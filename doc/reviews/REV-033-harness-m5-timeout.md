# REV-033 · harness · M5 ISSUE-076 `timeout_hint`（agent_call 超时回退接入内核派发）

> **模块声明**：本分支只改 `crates/zgraph-harness/src/{config.rs,executor.rs}`（+291/−9），符合 AGENTS §2 `harness` 模块边界；无 `Cargo.toml`/`Cargo.lock`、无 kernel/worker/trace/cli/e2e/`doc/**` 改动。作者 `harness-dev`，评审人 `reviewer-b`（**非作者**）。评审过程未修改任何仓库文件，本次提交仅新增本评审记录；未 merge/push/rebase。
>
> **独立核对口径**：不采信作者自述。亲自读 `ad7fbec..5796332` 全量 diff 与相关上下文（`agent_index`/`load_agent`/`parse_timeouts`/kernel `NodeExecutor::timeout_hint` 与 `start_dispatch` 解析链），在**全新隔离 target**（`/tmp/rev033-ws-target`）实跑 harness/e2e/workspace/clippy/fmt/check.sh；另用**仓库外临时树**（`git archive` 副本）做 3 个独立 probe：① 在 kernel/harness 注入 eprintln 观测真实解析值；② 把端到端用例的 AgentConfig 值 1000→1200 做变异，验证测试敏感性；③ 构造「hint 可读出 + 其它字段非法」确认错误路径仍 `HARNESS_CONFIG_LOAD`。临时树不入库、不影响工作树。
>
> **范围基线说明**：评审对象父提交 `ad7fbec`；评审期间本地 main 推进到 `b27d9c0`（REV-038，**docs-only**：DR-002/ISSUE/progress/REV-038）。`git diff ad7fbec..b27d9c0` 仅 `doc/**`，与本分支零代码冲突；合并前由 Lead 按 merge.sh 流程 rebase（本记录以代码 diff `ad7fbec..5796332` 为准）。

- **分支**：`fix/harness-m5-timeout`
- **作者**：`harness-dev`（提交 `Agent:` 字段）
- **评审人**：`reviewer-b`
- **变更范围**：`ad7fbec124f29cb9143d824e5560a002eef8d6dc..579633236f8ebc687635f378a51d35a3b17d4757`（1 commit `5796332 fix(harness): ISSUE-076 agent_call 超时回退接入 timeout_hint（REV-033）`；`git diff --shortstat` = **2 files changed, 291 insertions(+), 9 deletions(-)**；trailers `Review: REV-033`/`Agent: harness-dev` 已核；worktree clean）
- **评审时间**：2026-10-30
- **关联**：**ISSUE-076**（`AgentConfig.node_timeout_ms` 回退未生效）；**REV-026**（kernel 侧 `declared_timeout_ms` + `NodeExecutor::timeout_hint` + 优先级链已合入 main `ad7fbec`）；**DR-002 §4/§6**（ISSUE-076 跨模块协同与 harness 同步轻量路径口径、wave2 分工）；spec 05 §5.2.2 **V13**（`1000..=3_600_000`）、§5.2.6（超时优先级）、§5.3.6（剩余 <1000ms 不发新调用）、§5.7.3（取消/超时优先）；architecture §8 D4；REV-027（harness 基线）+ REV-030（CLI 组合执行器转发，待合）

## 1. 结论

**结论：通过**

本分支把 ISSUE-076 的 **harness 侧**闭环落成，实现与 Lead/DR-002 冻结口径一致，未发现阻塞/重要缺陷：

- **`timeout_hint` 实现正确且面收敛**：`HarnessExecutor::timeout_hint(&CompiledNode)` 仅当 `CompiledHandler::Agent` 返回 `Some(AgentConfig.node_timeout_ms)`，其余节点立即 `None`（无 IO）；数据源为新增的 `config.rs::peek_agent_node_timeout`——只扫 `<assets_dir>/agents/*.json`、按文件内 `id` 命中、只读 `node_timeout_ms` 并做 `1000..=3_600_000` 范围校验，**不**解析 model/tools/prompt、不读密钥、不查 `ToolRegistry`（与 DR-002 §4 一致）。
- **与正式加载语义一致（独立核对）**：`peek` 与 `agent_index`/`load_agent` 的目录扫描与排序（`file_name` 升序）、`id` 命中（文件内 `id` 而非文件名）、重复 id「文件名排序第一个生效」、`absolute(assets_dir)`、`MAX_AGENT_CONFIG_BYTES`、`is_json_file`、`as_u64` 整数语义均一致；范围常量与 V13 一致（新增的 `NODE_TIMEOUT_MIN_MS/MAX_MS` 同时替换 `parse_timeouts` 字面量，**无行为变化**）。独立 probe 证实：kernel 派发实际解析 `force=None, declared=None, hint=Some(1000) → resolved=1000ms`，harness `execute` 入口 `remaining=999`。
- **优先级正确**：kernel `start_dispatch` = `force > declared > hint > DEFAULT`（REV-026 已测）；harness 侧 `resolve_node_timeout_ms` 同序（declared > AgentConfig > default），`run_agent` 的 elapsed 口径在成功加载后取 `declared.unwrap_or(agent.config.node_timeout_ms)`，与派发同一生效值；纯函数单测覆盖 declared=300 优先于 AgentConfig=123000、缺失回落 180000。
- **错误路径不变**：hint 可读出但 AgentConfig 其它字段非法时，`load_agent` 仍返回 `HARNESS_CONFIG_LOAD`；独立 probe 实测 run `Failed` + `HARNESS_CONFIG_LOAD` + 实例 `Crashed`（非 NODE_TIMEOUT）。
- **测试与回归**：新增 **9 个单测**（config 3 + executor 6，含端到端）全部通过；**全新隔离 target** 下 `cargo test -p zgraph-harness` **130 passed / 0 failed / 1 ignored**（REV-027 基线 121，+9）；`cargo test -p zgraph-e2e` **24 passed / 1 ignored**（`harness_ac08` 4 passed/1 ignored 不回归）；`cargo test --workspace` **455 passed / 0 failed / 2 ignored / 53 个测试二进制**（main 基线 446，+9，无缩水）；`cargo clippy --workspace --all-targets -- -D warnings` exit 0；`fmt --check` exit 0；`check.sh` PASS；worktree clean、HEAD tree 无禁入文件；提交 trailers 合规；新函数均 ≤60（最大新增测试 44 行）。

未发现阻塞/重要项；5 条建议/备注见 §3（1s 端到端用例实际走的是「剩余 <1000ms 不发新调用」预检边界而非 1s deadline 到期、`timeout_hint` 同步文件 IO 在 kernel 派发路径、peek docstring 与 loader fail-fast 语义差异、`execute` 已 78 行 >60、ISSUE-076 端到端仍需 REV-030 转发），均**不阻塞合并**。

## 2. 四维评审

| 维度 | 结论 | 依据（文件/行/测试/命令） |
|---|---|---|
| 功能正确性 | ✅ | ① `timeout_hint` 仅 Agent handler 返回 hint（`executor.rs:190-203`），非 agent 节点立即 None（单测 `timeout_hint_is_none_for_non_agent_nodes`）。② `peek_agent_node_timeout` 语义与 `agent_index` 一致（均 `file_name` 排序、文件内 `id`、`or_insert`/命中即返回 = 重复 id 取首个），范围 `1000..=3_600_000` 对齐 V13（`config.rs:210-253`、`:783-793`）；probe 实测 kernel `resolved=1000`、harness `remaining=999`。③ 优先级：kernel `force > declared > hint > default`（`run.rs:674-683`，REV-026 已测）；harness elapsed 同序（`executor.rs:132-163, 206-212`）；纯函数单测 + 端到端。④ 错误路径：hint 存在但配置非法仍 `HARNESS_CONFIG_LOAD`（probe 实测；`config_failure` 逻辑未改 `executor.rs:216-238`）。⑤ 验收口径：AgentConfig=1000ms 时 run `Failed` + `NODE_TIMEOUT` + 实例 `TimedOut`（单测真 kernel `run_to_completion`，5/5 稳定）——与 spec §5.3.6「剩余 <1000ms 不再发起新调用 → 实例超时」一致。 |
| 代码结构合理性 | ✅ | 轻量读取与正式加载**同文件同函数族**：`NODE_TIMEOUT_MIN/MAX_MS` 常量抽出后 `parse_timeouts` 复用，避免范围漂移；`peek_agent_node_timeout` 独立可测、`pub(crate)` 不扩公开面；`timeout_hint` 只做「handler 判定 + peek」薄适配，`resolve_node_timeout_ms` 单独承接 elapsed 口径；kernel 侧默认 trait 方法 + harness 实现，`HarnessExecutor` 公开构造/settings 面未改；无新依赖、无跨模块改动。 |
| 逻辑性 | ✅ | 优先级链两处同序且与 DR-002 §4 一致；hint 失败/缺失返回 None 交内核默认，随后 Loading 仍权威报 ConfigLoad（不吞错）；peek 对坏文件 skip 是为「提示」服务、不改变权威加载的 fail-fast；`resolve_node_timeout_ms` 与 kernel 解析用同一 `or_else` 顺序，避免 elapsed 与 deadline 漂移；`declared_timeout_ms`（None=未声明）而非历史 `timeout_ms`（已折叠默认值）保证 hint 不被掩盖。 |
| 可读性 | ✅ | 新增函数/常量均有 docstring 并溯源 05 §5.2.2 V13/§5.2.6、ISSUE-076、DR-002 §4；文件内注释说明「为何不能读 node.timeout_ms」「为何 skip 坏文件交给 Loading」；测试名自解释、每例单目的；仅 1 处 docstring 与实现细节不完全一致（§3 #3）与 `execute` 长度（§3 #4）两条 Nit。 |

## 3. 发现清单

| # | 级别 | 位置 | 问题 | 建议 | 状态 |
|---|---|---|---|---|---|
| 1 | 建议（测试有效性/误报风险） | `crates/zgraph-harness/src/executor.rs:688-715`（`kernel_uses_agent_config_node_timeout_when_dsl_omits_it`） | 端到端用例用 **AgentConfig.node_timeout_ms=1000ms** + 立即返回的 fake LLM，实测（我在临时树注入日志）kernel 解析 `resolved=1000ms`，harness `execute` 入口 `remaining=999ms`，因此**走的是 `preflight_call_timeout` 的「剩余 <1000ms 不发新调用」分支**并立即返回 `InstanceTimeout`（整例 0.01s），**不是** 1s deadline 到期/慢调用超时路径。变异 probe：仅把该值 1000→1200 即变 `Completed` → 用例确实能区分「hint≈1s」与「默认 180s」，但对「hint 值在任意 >1000ms 时是否真正到期执行」无覆盖；未来若调整 §5.3.6 阈值，用例语义会静默改变。 | 保留本用例作为「1s 边界（预检规则）」并注释说明；另补 1 个慢调用用例（fake LLM 用 `chunk_delay_ms` 使耗时 > 1.5s、AgentConfig 设 1500–2000ms）断言 run `Failed/NODE_TIMEOUT`、`NodeTimedOut.elapsed_ms >= hint/2`（参考 kernel `tests/timeout_hint.rs` 的做法），覆盖真实 deadline 到期路径。**不阻塞**（当前行为符合 spec；kernel 侧已有 deadline 到期测试）。 | open（建议 wave2/下次补） |
| 2 | 建议（事件循环/性能） | `crates/zgraph-harness/src/executor.rs:190-203`（`timeout_hint` → `config.rs:220-253` `peek_agent_node_timeout`）；`executor.rs:132-138`（`execute` 顶部 `resolve_node_timeout_ms` 再次 peek） | kernel 在**事件循环内**同步调用 `timeout_hint`；仅 agent_call 且 DSL 未声明超时才会真正做 `read_dir` + 逐文件读取/解析 agents JSON（无缓存，按文件名顺序扫到 id 命中）。每个此类派发一次扫描；harness `execute` 又调用 `resolve_node_timeout_ms` 再扫一次（虽然该值只在取消/超时/ConfigLoad 早退路径用于 elapsed 诊断，成功路径多余）。资产目录 agent 数量多/文件大时会对派发路径引入同步 IO 延迟（H4「不得在事件循环线程做阻塞调用」的边界情况）。 | ① `execute` 中的 `resolved_timeout_ms` 改为**仅在早退分支惰性计算**（取消路径不需要；remaining==0/config_failure 时再 peek），成功路径直接用已加载配置；② 可选：在 `HarnessExecutor` 内按 `(assets_dir, agent_id)` 做一次性缓存并注明与 `load_agent` 每次重读的「热更新」取舍；③ 若维持现状，在 docstring 标注「单次同步扫描，agent 资产规模受限」。**不阻塞**（典型资产 <10 个文件，单次 <1ms 量级；本分支 e2e/全量无回归）。 | open（建议后续触碰时优化） |
| 3 | 建议/Nit（docstring 与实际语义） | `crates/zgraph-harness/src/config.rs:222-226`（`peek_agent_node_timeout` doc） | doc 写「JSON 非法 … → None」，但实现是**跳过**该文件继续扫描，若后续文件命中 id 仍可返回 `Some`（单测 `peek_node_timeout_is_none_for_missing_or_invalid_assets` 正是「broken.json 被跳过、parser 仍命中」）；而权威加载 `agent_index` 对**任意**坏文件/缺 id 直接报错（fail-fast）→ 两者语义有意不同（hint 尽量给提示，Loading 仍权威），但 doc 未说明「跳过」与「loader fail-fast」的差异 | 把 doc 改为「坏文件/缺 id 的文件被跳过（不阻断扫描）；权威加载仍会 fail-fast 报 `HARNESS_CONFIG_LOAD`」；可加一句「hint 不是加载成功承诺」。**不阻塞**（行为无风险：这类 run 仍由 Loading 报 ConfigLoad，probe 已实测）。 | open（建议随手改） |
| 4 | Nit（函数长度） | `crates/zgraph-harness/src/executor.rs:111-189`（`execute` 78 行；main 为 71 行，本分支 +7） | architecture §2.2 等价约束「函数 ≤60 行」；该函数在 main 已超限（ISSUE-081 仍未关闭），本分支因新增 resolved/declared 两段使其从 71 → 78 行，进一步变差（非新增超限函数，但加剧） | 与 §3 #2① 一起做小重构：把 Loading 早退（取消/deadline/ConfigLoad 映射）抽成 helper，可同时降到 ≤60；或在 ISSUE-081 清单中标注「`execute` 71→78，待拆」。**不阻塞**。 | open（ISSUE-081 承接） |
| 5 | 备注（端到端闭合范围） | kernel `run.rs:681` 调用点；`crates/zgraph-cli`/`tests/e2e` 组合执行器 | 本分支完成 **harness 侧** `timeout_hint`；kernel REV-026 已实现调用/优先级。但实际产品路径（CLI）的组合执行器尚未转发 `timeout_hint`（DR-002 §6 已排 REV-030 `feat/cli-m5-replay`：`timeout_hint 转发`）；`tests/e2e` 的 `RoutingExecutor` 目前也未覆写该方法（AC-08/AC-12 agent 节点均**显式声明**了 `timeout_ms=60000`，declared 优先，故 e2e 不回归、也未覆盖本特性）。ISSUE-076 当前仍是 `open`（台账：「部分修…harness 实现与 CLI 转发在 wave2（REV-033/REV-030），完成后转 fixed」）。 | REV-030 合并时补：CLI 组合执行器转发 `timeout_hint`（仅 agent 分支）+ 1 条 CLI 层端到端（AgentConfig 值生效）或 e2e `RoutingExecutor` 同步转发；届时由 docs/Lead 把 ISSUE-076 转 `fixed`。**不阻塞本分支**（范围与台账一致）。 | open（REV-030 承接） |

> 无阻塞项、无重要项。#1 为测试覆盖加固（当前断言仍真实、行为符合 spec）、#2/#3/#4 为性能/文档/函数长度 Nit、#5 为跨分支端到端范围备注。**本评审无需新开 P1/P2 ISSUE**；#1/#4 建议随 wave2 或 ISSUE-081 处理，ISSUE-076 的关闭由 REV-030 + docs 收尾。

## 4. 验证证据

### 4.1 范围、分支、提交元数据

```bash
cd /Users/zyzs/lczyzs/code/zgraph/.worktrees/harness-harness-dev
git branch --show-current   # fix/harness-m5-timeout
git rev-parse HEAD          # 579633236f8ebc687635f378a51d35a3b17d4757
git merge-base main HEAD    # ad7fbec124f29cb9143d824e5560a002eef8d6dc（评审期间 main→b27d9c0，docs-only）
git diff --shortstat main...HEAD   # 2 files changed, 291 insertions(+), 9 deletions(-)
git diff --name-status main...HEAD # M crates/zgraph-harness/src/config.rs；M .../executor.rs
git log -1 --format='%an <%ae> / %cn <%ce>'   # chenyuxiangg <1007423165@qq.com>（both）
git log -1 --format='%B' | grep -E '^(Review: REV-033|Agent: harness-dev)$'   # 两行命中
git status --short                # 空（clean）
git ls-tree -r --name-only HEAD | grep -E "__pycache__|\.pyc$|\.db$|\.sqlite" || echo none   # none
git diff --check                 # exit 0
```

- 未改 `Cargo.toml`/`Cargo.lock`（无新依赖）、未动 kernel/worker/trace/cli/e2e/`doc/**`。
- main 在评审期间新增 `b27d9c0`/`d81eefd`（REV-038 docs-only：DR-002/ISSUE/progress/REV-038），与本分支代码无交集；合并前需按 merge.sh 流程 rebase（内容不会变化）。

### 4.2 测试与门禁（**全新隔离 target** `/tmp/rev033-ws-target`；`CARGO_HOME=<repo>/.cargo-home`、`CARGO_NET_OFFLINE=true`、`CARGO_INCREMENTAL=0`、`ZGRAPH_PYTHON=<repo>/.venv/bin/python`）

| 命令 | 结果 |
|---|---|
| `cargo test -p zgraph-harness` | **exit 0：130 passed / 0 failed / 1 ignored**（ignored=`llm::transport::tests::real_deepseek_smoke`；REV-027 基线 121/1，**+9**：config 3 + executor 6） |
| `cargo test -p zgraph-e2e` | **exit 0：24 passed / 0 failed / 1 ignored**（7 个 test binary；`harness_ac08` **4 passed/1 ignored 不回归**、ac12 1、concurrency 4、cancel 2、fault_injection 7、parallel 4、timeout_kill 2） |
| `cargo test --workspace`（53 个测试二进制） | **exit 0：455 passed / 0 failed / 2 ignored**（main 基线 446/2 + 9；无新增失败、无缩水） |
| `cargo clippy --workspace --all-targets -- -D warnings` | **exit 0 / 0 warning** |
| `cargo fmt --all -- --check` | **exit 0** |
| `bash scripts/dev/check.sh` | **CHECK PASSED**（身份/remote/CLAUDE.md/分支名/Python 语法/progress/issues/fmt/`git diff --check`） |
| `git status --short`（全部命令后） | 空 |
| 端到端用例稳定性 | 定向跑 `kernel_uses_agent_config_node_timeout_when_dsl_omits_it` ×5：**5/5 passed**（每次 `finished in 0.01s`） |

> **环境注意事项（重要，供 merge dry-run 参考）**：使用仓库内 `target/` 时，workspace 单元曾复用**陈旧 test 二进制**（harness 显示 121 passed，而非 130），与 Lead 提示的「隔离 target 有陈旧 `CARGO_MANIFEST_DIR`」同源；改用**全新 `/tmp/rev033-ws-target`** 后为 455/2（harness 130）。另：全新 target 首轮 workspace 曾因机器高负载触发**既有** `zgraph-cli` `ac07_watchdog_ticks_and_pause_resume` 时序 flake（`max tick gap 51.12ms > 50ms`，本分支未触碰该文件），复跑全绿——与 REV-038 #9 同类环境噪声，建议沿用「隔离 target + 必要时重跑」的门禁口径。

### 4.3 独立 probe（仓库外临时树，`git archive 5796332`；仓库零改动）

| probe | 方法 | 结果 |
|---|---|---|
| 解析值观测 | 临时树在 kernel `start_dispatch` 与 harness `execute` 入口注入 eprintln，跑端到端用例 | kernel：`node=worker type=AgentCall force=None declared=None hint=Some(1000) resolved=1000ms`；harness：`declared=None resolved=1000`、`remaining_at_enter=999` |
| 1s 用例敏感度（变异） | 临时树仅把用例 AgentConfig 值 `1_000`→`1_200`（其余不变） | 用例**失败**：`status: Completed`（left Completed / right Failed）→ 证明用例能区分 hint≈1s 与 >1s/默认 180s，且当前通过依赖 `remaining<1000ms` 预检边界（见 §3 #1） |
| 错误路径仍 ConfigLoad | 临时树用例：`node_timeout_ms=1_000` + `model.provider="bogus"`（hint 可读出、配置其它字段非法） | **passed**：run `Failed`、`error.code == HARNESS_CONFIG_LOAD`、`worker` 实例 `Crashed`（非 NODE_TIMEOUT） |

- probe 相关日志：`/tmp/rev033-dbg.log`（解析值）、`/tmp/rev033-mut.log`（变异失败证明）、ConfigLoad probe 输出；临时树 `/tmp/rev033-tree`、`/tmp/rev033-mut` 均在仓库外，`git status` clean。

### 4.4 静态/边界核对

| 项 | 结果 |
|---|---|
| 新函数长度 | `peek_agent_node_timeout` 33 行、`timeout_hint` 12、`resolve_node_timeout_ms` 5；新增测试最长 44 行（`peek_node_timeout_is_none_for_missing_or_invalid_assets`）——均 ≤60；`execute` 78 行（既有超限，见 §3 #4） |
| 范围/整数一致性 | 新增 `NODE_TIMEOUT_MIN_MS=1_000`/`NODE_TIMEOUT_MAX_MS=3_600_000`，`parse_timeouts` 复用（V13 一致）；`peek` 用 `Value::as_u64` + 范围过滤，与 `optional_u64`+V13 对**合法值**一致；缺失/非法→None（由 Loading 报 ConfigLoad） |
| 索引/排序/重复 id | `peek` 与 `agent_index` 均按 `file_name` 升序、按文件内 `id` 命中、重复 id 取第一个（probe/单测 `..._uses_first_file_for_duplicate_ids` 断言 11000）；`absolute`/`MAX_AGENT_CONFIG_BYTES`/`is_json_file` 同源 |
| declared 优先 | 纯函数单测 declared=300 > AgentConfig=123000；kernel REV-026 侧已测 declared > hint；elapsed 与派发同序 |
| 错误路径 | `config_failure` 未改；probe 实测 hint 存在 + 其它字段非法 → `HARNESS_CONFIG_LOAD`；缺字段/非法 hint → None + 内核默认 + Loading 仍 ConfigLoad |
| 静态扫描 | 无 mock/绝对路径新增；无 `unwrap/expect/panic` 于生产新代码（`peek` 用 `?`/`ok()?`，`timeout_hint` 无 panic）；`#![forbid(unsafe_code)]` 保持；无新增依赖；HEAD tree 无禁入文件 |
| 提交元数据 | subject 符合 `<type>(<module>): <摘要>`；正文含 `Review: REV-033`/`Agent: harness-dev`；author/committer 合规 |

## 5. 遗留问题

- **ISSUE-076（部分关闭→待 REV-030）**：本分支完成 harness 侧 `timeout_hint` + elapsed 同序 + 9 单测；**未关闭**——CLI 组合执行器转发（REV-030 `feat/cli-m5-replay`）落地并补 CLI/组合端到端后，由 docs/Lead 把台账转 `fixed`；`tests/e2e` 的 `RoutingExecutor` 如需覆盖应同步转发（当前 AC-08/AC-12 agent 均 declared 60000，不影响）。
- **建议项**：#1 补慢调用/真实 deadline 到期用例（hint 1500–2000ms + 慢 fake）；#2 `execute` 惰性解析 + 可选 hint 缓存（降低事件循环同步 IO/重复扫描）；#3 `peek` docstring 说明 skip 与 loader fail-fast 差异；#4 `execute` 71→78 行记入 ISSUE-081 清单。均不阻塞。
- **合并流程**：main 评审期间推进（REV-038 docs-only），分支需 rebase 后再走 `merge.sh`；建议 merge dry-run 沿用全新/清理后的隔离 target（仓库 `target/` 复用了陈旧 workspace 单元，曾得到 446 而非 455）。
- 本评审未修改任何产品代码/测试/台账；本记录为本分支唯一新增文件（评审对象 `5796332` + 本记录提交）。

## 6. 条件关闭记录

不适用（结论为「通过」；§3 均为不阻塞建议）。
