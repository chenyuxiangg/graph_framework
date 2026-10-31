# REV-082 · cli · W3a 修复：human 实例 L2 `recorded_outputs` 补录 + replay 适配

> **范围声明**：本分支仅改 `crates/zgraph-cli/**`（4 files, +548/−28）；未改 kernel/trace/dsl/e2e、未改 `Cargo.lock`、未新增依赖。评审人只新增本评审记录，未修改产品/测试代码。

- **分支**：`fix/cli-human-record`
- 分支: fix/cli-human-record
- **作者**：`cli-dev`（提交 `c3022ea`，正文 `Review: REV-082` / `Agent: cli-dev`）
- **评审人**：`reviewer-a`（独立评审，**≠ 作者**）
- **变更范围**：`971e48f..c3022ea`：`src/{bridge,replay_cmd,run_cmd}.rs`、`tests/human_decide.rs`。`971e48f` = 本地 main（含 REV-081 cli decide/consumer + trace v5 + kernel WaitingHuman）。
- **评审时间**：2026-11-01
- **关联**：DR-006 §2.2（human 输入/输出快照与 L2 记录）、§8（L2 行：human 输出按 `(node_id,attempt)` 重放、不重新要求人工决策、结果一致）；task-01/14 §3、task-25；06 §6.2.13/§6.10.7；REV-081（同 worktree 上一轮）

## 1. 结论

**结论：通过**

按 task-25 与 DR-006 §2.2/§8 L2 行逐项核对，修复正确、范围合规；P0=0、P1=0、P2=0，P3 建议 8 条（含对 CLI 侧 replay 适配的裁决与测试加固建议），不阻塞合并。未放宽/删除既有断言、未新增 ignore、未越界改 kernel/trace/e2e。

### 1.1 修复正确性对照

| 合同项 | 实现/证据 | 结论 |
|---|---|---|
| **缺陷根因** | human 实例由内核内置完成、不经 `NodeExecutor`，故 `RecordingExecutor` 不写 `recorded_outputs`；而 replay `ensure_executed_nodes_recorded`（`replay_cmd.rs:423-445`）把 `node_type=python_node` 且 `Completed` 的 human 实例纳入完整性检查 → 修复前 `--record=full` human run 的 L2 预检报 `REPLAY_RECORD_MISSING` exit 3 | ✅（与 Lead 背景一致） |
| **补录注入条件** | `run_cmd::human_recording(mode, graph)`（`run_cmd.rs:926-945`）：`record_mode==None` 或图中无 `CompiledHandler::Human` → `None`；否则 `{mode, human_nodes=全部 Human 节点 id（BTreeSet 确定性）}`，在 `start_run` 注入 `BridgeObservers`（`run_cmd.rs:1045`） | ✅ |
| **补录时机/口径** | `TraceBridge::transition` 在 `to==Completed` 且 `node_id ∈ human_nodes` 时取 `snapshot.output`，调用 `run_cmd::record_output`（`bridge.rs:166-197,218-226`）：hash 恒写、full 追加 `output_json`、none 不写；**直接复用 CLI 既有 `output_hash`/`record_output`，未在 bridge 重算 hash 口径**（`run_cmd.rs:617-646`）。human 决策顺序冻结（写 output→transition Completed，快照携带 output）保证 `snapshot.output` 可用 | ✅ |
| **入队顺序** | 补录发生在 `transition` 事件入队**之前**（`bridge.rs:218-226`），与 executor 路径「执行成功→record_output→内核 emit transition」一致；RecordedOutput 非 TraceEvent，不改变 `events` 形状 | ✅ |
| **无 human / 无 record 不变** | 注入为 `None` 时 `record_human_output` 首行即 no-op；非 human 节点、非 Completed 迁移均 no-op；无 human 图 replay `Arc` 原样返回（`replay_cmd.rs:477-494`，测试 `Arc::ptr_eq`）→ `recorded_outputs` 行集合/回放输出逐字节不变 | ✅ |
| **C13** | bridge 只补录 `recorded_outputs`（事件侧表），**不写 `human_decisions`**；决策表仍由 decide/consumer 写（探针核实 run+replay 后 human_decisions 仍恰 1 行） | ✅ |
| **replay 适配（L2）** | `run_outputs` 在读取/编译 recorded graph 后调用 `replay_recorded_human_graph`（`replay_cmd.rs:324,477-494`）：仅当图含 Human handler 时 clone graph，把 human handler 替换为可执行 `Python{module:"builtin",function:"human",exec_class:Io}`，由 `RecordedExecutor` 按 `(node_id,attempt)` 直接返回记录输出——不启动 worker、不重新要求决策；`graph_hash`/`dsl_json`/其它节点不变；仅 `--mode=outputs` 路径生效，`--mode=events` 未动 | ✅ |
| **安全性（不掩盖缺失）** | `build_plan` 在**重跑前**执行 `ensure_replayable`（必须 full+Completed）与 `ensure_executed_nodes_recorded`（每个 Completed python_node/agent_call 必须有记录）；human 记录缺失（如修复前旧 run）→ 在 replacement 之前就 exit 3 `REPLAY_RECORD_MISSING`，不会挂起、不会执行真实 Python（评审人探针验证） | ✅ |
| **范围** | `git diff --name-status 971e48f..c3022ea` = 3 src + 1 test，全部 `crates/zgraph-cli/**`；`Cargo.lock`/`Cargo.toml` 无 diff；提交正文含 `Review: REV-082`/`Agent: cli-dev` | ✅ |

## 2. 四维评审

| 维度 | 结论 | 依据（文件/行/测试） |
|---|---|---|
| 功能正确性 | ✅ | 缺陷根因与修复路径成立（§1.1）；`cargo test -p zgraph-cli` **206 passed/0 failed**、`cargo test --workspace` **798 passed/0 failed/3 ignored** 与作者报告一致；真 CLI 用例覆盖 full（补录+hash 口径+L2 replay matched）/hash（仅 hash+L2 exit3）/none（无行）；评审人**独立探针 27/27 PASS**（§4.2），并补验作者用例未覆盖的混合图（python 执行器节点 + human）、双 human 串行、旧 run 缺 human 记录的安全路径 |
| 代码结构合理性 | ✅（N4 备注） | 补录配置作为 `BridgeObservers` 可选字段，语义清晰；复用 `record_output`/`output_hash` 而非重算，口径单一；bridge 仅新增一个私有 helper + 一次调用；run 侧注入函数独立；replay 适配封装为单一函数（仅 outputs 路径调用），无 human 图零改动；无新增依赖/公开 API 膨胀（`record_output` 仅 `pub(crate)`）。N4：bridge → run_cmd 反向依赖（模块环）属最小改动可接受，建议后续抽 `recording` 模块 |
| 逻辑性 | ✅ | 补录仅在 `Completed`+human 节点触发，WaitingHuman/非 human 均 no-op，`UNIQUE(run_id,instance_id,attempt)` 不会重复写；顺序与 executor 路径一致；replay 替换只改 handler 不改 `graph_hash`/dsl/其它节点，kernel 以普通节点派发到 `RecordedExecutor`（无 worker、无等待）；L2 预检在重跑前拦截缺失记录，避免 replacement 路径掩盖缺记录或挂起；无 human 图返回同一 Arc（`ptr_eq` 测试）；无死循环/吞异常/新 panic 点 |
| 可读性 | ✅（N6/N8 备注） | 三处模块/函数注释均引用 DR-006 §2.2/§8 并解释「为什么」（human 不经 executor、为何 bridge 补录、为何 replay 替换 handler、为何在 transition 前入队）；配置/常量命名清晰；测试名与断言消息可读。N6/N8 为注释口径小建议 |

## 3. 发现清单

> 级别口径：P0 = 阻塞；P1 = 重要（必须修复或登记 ISSUE）；P2 = 一般；P3 = 建议（可后置，应进台账）。本分支 P0=0、P1=0、P2=0。

| # | 级别 | 位置 | 问题 | 建议 | 状态 |
|---|---|---|---|---|---|
| 1 | P3 建议 | `crates/zgraph-cli/tests/human_decide.rs:651-780`；`bridge.rs:1216-1330` | 测试只覆盖单 human 节点 attempt=0；**未覆盖多 human 节点与 attempt>0（defer rework 重入）** 的补录/重放。评审人探针已补验双 human 串行（2 行记录、replay matched）与混合图，但 attempt>0 仍未验证（human 每代新 instance attempt 递增，`(node_id,attempt)` 键正确性是本修复的关键假设） | 建议 e2e（W3a 下一分支）以 rework human 图覆盖 attempt 0/1 的 recorded_outputs + `replay --mode=outputs`；或补一个 CLI 集成用例 | open（建议项） |
| 2 | P3 建议 | `crates/zgraph-cli/src/replay_cmd.rs:423-445`（既有预检） | 未针对**修复前旧 run（full 记录但缺 human 行）** 的 L2 行为写回归测试。评审人探针已验证：删除 human 记录后 `replay --mode=outputs` 快速 exit 3 `REPLAY_RECORD_MISSING`、不挂起、不产生新 run；该安全性依赖既有 `ensure_executed_nodes_recorded`，建议固化用例防 replacement 路径退化 | 补 1 例：删除 human recorded_outputs 后断言 exit3/REPLAY_RECORD_MISSING/无新 run | open（建议项） |
| 3 | P3 建议 | `crates/zgraph-cli/src/bridge.rs:218-226` | 「补录先于 transition 事件入队」目前仅由代码/注释保证，无测试可观测（DB 无顺序观测点）。Lead 关注点 1 的顺序要求实质是「与 executor 路径一致」，二者均已如此 | 可选：bridge 单测用自定义 sink/回调记录 `TraceInput` 顺序（RecordedOutput 在 Transition 前）；不阻塞 | open（建议项） |
| 4 | P3 建议 | `crates/zgraph-cli/src/bridge.rs:1-30,166-197`；`run_cmd.rs:617` | bridge 反向依赖 `crate::run_cmd::record_output` 形成模块环（`run_cmd` 已依赖 `bridge`）。本分支为最小改动可接受，但长期维护性略差 | 后续把 `record_output`/`output_hash`/`now_rfc3339` 抽到 `bridge` 或独立 `recording` 模块，两条路径共用 | open（建议项） |
| 5 | P3 裁决 | `crates/zgraph-cli/src/replay_cmd.rs:477-494` | **Lead 关注点 2 裁决（CLI 侧 replay 适配 vs kernel 方案）**：结论 **可接受、与 DR-006 §8 L2 一致且安全**——(a) 语义一致：human 输出按记录重放、不重新要求决策、publish/gate/end 聚合不变（replay matched）；(b) 安全：仅 clone replay 进程内 graph，不改 `graph_hash`/dsl/原图，仅 outputs 路径；缺失记录在重跑前被既有完整性预检拦截（不挂起/不执行 Python）；(c) 范围：replay 是 CLI 功能，kernel 侧无「记录 handler 覆盖」接口，若改 kernel 需新增面并牵动 kernel 分支/语义，收益低；当前 CLI 本地适配最小且不动冻结语义。**无需附加条件** | W3a-docs（REV-083）as-built 记录该适配（replay-only、graph clone、不改 graph_hash） | open（裁决完成） |
| 6 | P3 建议 | `crates/zgraph-cli/src/run_cmd.rs:1045`；`run_cmd.rs:926` | 注入的 `mode` 取 `args.record`（CLI 解析值）；task-25 表述为「mode 取 run options.record_mode」。当前 `run_options(args.record)` 同源等价，但未来若 options 与 CLI 参数分叉会不一致 | 注释注明同源，或直接使用 `options.record_mode`（需先构造 options） | open（建议项） |
| 7 | P3 建议 | `crates/zgraph-cli/src/run_cmd.rs:926-945` | 修复后 **human 图 `--record=hash`（含缺省 hash）也会写 human 的 hash 行**（与其它节点口径一致、符合合同「hash 恒写」），但这改变了 human 图 `recorded_outputs` 行集合（相对修复前）；无 human 图不受影响 | W3a-docs as-built/兼容说明注明「human 图记录行新增属预期修复」；e2e golden 需按新行为固化 | open（建议项） |
| 8 | P3 建议 | `crates/zgraph-cli/src/bridge.rs:181-183` | `snapshot.output == None` 时静默跳过补录（防御性）；按冻结顺序 human Completed 必有 output，若未来顺序变化会静默缺记录（L2 表现为 exit3 而非显式错误） | 加 `debug_assert!(snapshot.output.is_some())` 或注释说明该分支理论不可达 | open（建议项） |

## 4. 验证证据

### 4.1 门禁与测试（评审人亲自执行）

```bash
export CARGO_HOME=/Users/zyzs/lczyzs/code/zgraph/.cargo-home
cargo test -p zgraph-cli
# lib 98 + acceptance_m2 10 + decide 15 + human_decide 10 + replay 13
# + run 31 + trace 18 + validate 11 = 206 passed, 0 failed, 0 ignored
cargo test --workspace                  # 798 passed / 0 failed / 3 ignored（与作者报告一致）
cargo clippy --workspace --all-targets --locked -- -D warnings   # clean（exit 0）
cargo fmt --all -- --check              # exit 0
bash scripts/dev/check.sh               # CHECK PASSED
/Users/zyzs/lczyzs/code/zgraph/.venv/bin/python -m pytest     # 170 passed
```

- 范围：`git diff --name-status 971e48f..c3022ea` = `src/{bridge,replay_cmd,run_cmd}.rs` + `tests/human_decide.rs`（全在 `crates/zgraph-cli/**`）；`Cargo.lock`/`Cargo.toml` 无 diff；worktree 干净；无新增 ignore。
- 新增测试覆盖：桥层 full（含 output_json 与 hash 口径一致）/非 human/非 Completed/hash 仅 hash/无配置 no-op 3 例；`human_recording` 注入条件；replay 替换（human→Python、无 human `Arc::ptr_eq`、graph_hash 不变）；真 CLI full→`recorded_outputs` + `replay --mode=outputs --json` exit0 matched=true 且不产生新 run、hash→output_json NULL + L2 exit3 `REPLAY_RECORD_MISSING`、none→无行。

### 4.2 独立真 CLI 探针（评审人自建，27/27 PASS）

方法：新构建 `target/debug/zgraph`（真实 kernel + 真 worker + 真 SQLite），`/tmp` 独立图/DB；补验作者用例未覆盖场景。

```text
# A) 混合图：start → prep(python worker) → review(builtin.human) → end，--record=full
PASS human 进入 WaitingHuman、run 存活；decide 后 Completed
PASS recorded_outputs 含 executor 节点 prep 与 human 节点 review 各 1 行
PASS human attempt=0、output_json == {"decision":"approved","reason":null}、output_hash 64-hex
PASS replay --mode=outputs --json exit 0、matched=true、original==replayed、不产生新 run
PASS human_decisions 仍恰 1 行（bridge 未写决策表，C13）
# B) 双 human 串行（h1→h2）：两次 decide（approved/rejected）
PASS h1/h2 各 1 行 recorded_outputs；replay --mode=outputs exit0 matched
# C) 安全路径：把双 human 库中 h1 的 recorded_outputs 删除（模拟修复前旧 run）
PASS replay exit 3（<30s，不挂起）、stderr 含 REPLAY_RECORD_MISSING、不产生新 run、不执行 Python
# D) --record=hash human 图
PASS human 行 output_json 为 NULL；replay --mode=outputs exit 3 REPLAY_RECORD_MISSING（既有口径）
```

- 关键实现一致性核实：`TraceBridge::transition` 补录在 transition 事件前（与 `RecordingExecutor` 的「record_output → 内核 transition」一致，`run_cmd.rs:557-582`）；kernel 决策顺序为「写 output+清 deadline → transition Completed（快照携带 output）→ publish → human_decided」（`crates/zgraph-kernel/src/run.rs:750-790`），故 `snapshot.output` 在 bridge 补录时必然可用。

## 5. 遗留问题

- **P3 建议未登记 `doc/issues.md`**（本分支合同只允许 `crates/zgraph-cli/**`；台账由 docs/Lead 维护）：N1–N4、N6–N8 建议由 Lead 决定是否登记；N5 为已裁决设计适配，建议在 W3a-docs（REV-083）as-built 与 DR-006 §14.3/§15 执行记录留痕。
- **后续 e2e（W3a e2e 分支）应覆盖**：human rework（attempt 0/1 的 recorded_outputs + L2 replay）、混合 human+worker 图、含 human 的 run 的 `recorded_outputs` 新行集合 golden；旧 run（缺 human 记录）的 L2 拒绝回归。
- 未发现需要返工的问题；无阻塞项。

## 6. 条件关闭记录（仅"有条件通过"填写）

不适用（结论为 `通过`，无附加条件）。
