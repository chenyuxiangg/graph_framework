# REV-093 · e2e · W4b AC-17 a–j 动态 N 真链路

> **跨模块声明（ISSUE-111）**：本分支新增/修改的 fixture 属 e2e 合同路径：`tests/e2e/**`、`tests/e2e/fixtures/py/**`、`tests/fixtures/graphs/foreach_dynamic/**`；另新增 trace seed/golden `tests/fixtures/trace/{schema_v5_seed.sql,v5_expected_trace.json}` 并更新 `tests/fixtures/trace/GENERATION.md`——按 **ISSUE-111** 归属 e2e（与 `schema_v3_seed.sql`/`schema_v4_seed.sql` 同口径），本记录显式声明。`tests/e2e/Cargo.toml` 仅新增 `[[test]] dynamic_fanout`；**无产品代码/DSL/trace/cli/kernel 源码改动**。

- **分支**：`test/e2e-dynamic-fanout`
- **作者**：`e2e-dev`（`a82943d` 阶段 1 迁移/golden/静态/无 foreach；`e1fdcd1` AC-17 a–j 真链路；正文含 `Review: REV-093` / `Agent: e2e-dev`）
- **评审人**：`reviewer-b`（独立评审，≠ 作者）
- **变更范围**：`683260b..e1fdcd1`（18 files，+3842/−8）：`tests/e2e/dynamic_fanout.rs`（2122 行/21 用例）、`tests/e2e/fixtures/py/foreach_dynamic.py`、`tests/fixtures/graphs/foreach_dynamic/` 11 图、`tests/fixtures/trace/{schema_v5_seed.sql,v5_expected_trace.json,GENERATION.md}`、`tests/e2e/Cargo.toml`
- **评审时间**：2026-11-01
- **关联**：task-35（W4b 合同）/ task-40（作者任务）、DR-007 §11 AC-17 a–j/§12、spec 07 W4a 段、REV-091（cli）/REV-092（kernel unwrap）

## 1. 结论

**结论：通过**

P0=0；**P1=0**；P2=2；P3=6（均建议/记录项，不阻断）。AC-17 a–j 全部以**真 `zgraph` CLI + 真 Python worker + 真 SQLite**覆盖，断言强度高（含 L1 逐字节、L2 per-item、join source、v5→v6 同库端到端、SIGINT cancel、human per-item、ITEM_INCOMPLETE）；既有 e2e 106/0/2、workspace 915/0/3、dynamic_fanout 6 连跑 21/0、全量 e2e 3 连跑全绿，无 flake。合并后可进入 W4b-docs（REV-094）。

## 2. 四维评审

| 维度 | 结论 | 依据 |
|---|---|---|
| 功能正确性 | ✅ | a–j 全部真链路覆盖；item_index 独立实例、expansion count/keys/source、join 有序 + last-E source、N=0 无 GateResult、d/j 失败终态、e 重复 key/canonical、f max/not-array/controller skip、g SIGINT 130、h 2 WaitingHuman 分别 decide、i v5→v6 同库 + L1/L2、j ITEM_INCOMPLETE |
| 代码结构合理性 | ✅（P2 备注） | 2122 行按「静态/迁移/default → a–j」组织；helper 复用（run/poll/expansion/item/gate/decide/trace）；fixtures 与 worker 独立；Cargo 仅加 target；无产品代码改动 |
| 逻辑性 | ✅ | 等待用轮询（10ms/30s），无 sleep 猜测；join source 用 `finished_at` 最大 + item_index 平局（item1 500ms 延迟保证确定性）；d/j 终态语义与 DR §6.6 一致（active 取消、已 Completed 保留）；v5/v3 迁移+golden 双向断言 |
| 可读性 | ✅（P3 备注） | 文件头映射 AC/数据通路/稳定性约定；测试注释解释 why（C2/unwrap/region_overlap/迁移）；P3 为少量注释/覆盖范围 |

## 3. 发现清单

> P0/P1=0；以下 P2/P3 均不阻断合并。

### P2 · 建议

| # | 位置 | 问题 | 建议 |
|---|---|---|---|
| N1 | `ac17f_max_items_and_not_array_fail_before_instances`（`not_array_cases`） | **NOT_ARRAY 三变体未包含「object 无同名键」**：三组输入都经 `start` 整包广播，start 输出必含 `items` 键——variant 1 的 `{"bad":1}` 实际是「同名键非 array」，variant 2 是「一层 unwrap 后非 array」，variant 3 是标量；真正「object 无同名键」未在 e2e 出现（kernel `items_port_non_array_shapes_stay_not_array` 单测已覆盖） | 可新增上游 python 输出 `{"other":[...]}` 的失败图，或复用 `upstream_items` 图换 worker 返回无 `items` 键 object；使 e2e 三变体与 task-48/Lead 口径一致。非阻塞（规则本身有 kernel 单测） |
| N2 | `ac17d` / `ac17j` 终态断言 | **d 严格断言其它 W item `Cancelled`，j 允许 E item `Cancelled|Completed`**。语义上正确：冷峻 `fail_run` 取消失效中的 item，已先 Completed 的保留；d 用 `slow:true`（1s）保证其它 item 在 fail 时仍未完成，j 的 E 可能先完成因此允许两者。cd 与 j 的差异来自 fixture 时序而非不一致，但建议在 W4b-docs/AC 说明该「已 Completed 不受取消影响」口径 | 在 AC-17 d/j 注释或 W4b-docs 记录；d 严格断言依赖 1s slow，属可接受但建议避免未来缩短延迟造成 flake。非阻塞 |

### P3 · 台账/阅读性

| # | 位置 | 问题 | 建议 |
|---|---|---|---|
| N3 | `ac17a` 注释（`// item1（delay 120ms）...`） | 实例 inputs 实际 `delay_ms: 500`，注释仍写 120ms | 改注释为 500ms，避免误导 join source 判定 |
| N4 | R18 `region_overlap` e2e 缺席（`BAD_FOREACH_REGION_CASES` 10/11） | 已明确说明：冻结优先级下 row8 `external_edge` 先拦截，DSL `rules.rs::r18_region_overlap_is_checked_after_boundary_rows` 锁定该优先级；e2e 不重复构造不可达反例，属合理分工 | 无需动作；W4b-docs 记录「11 reason 中 region_overlap 由 dsl 单测锁优先级」 |
| N5 | `DEFAULT_GRAPHS` 5 图 | 未含 `valid_subgraph.json` 等；无 foreach 默认 golden 由既有 e2e/静态测试覆盖，本文件代表性抽样 | 无需动作；docs 可交叉引用 |
| N6 | AC-17 agent_call 形状 | e2e 用真 python worker（dict）覆盖整包 object；agent_call 四字段 object 未直接走 e2e（AC-17 a–j 未强制）；同一 unwrap 规则由 kernel 单测/worker 形状覆盖 | W4b-docs 可注明；如后续 AC 需要可补 agent_call 图 |
| N7 | `schema_v5_seed.sql`/`v5_expected_trace.json` | v5 seed/golden 属外部生成资产（GENERATION §W4b 已记录）；CI 不复生成 | 可接受；W4b-docs 保留生成口径 |
| N8 | 稳定性约定 | `#![cfg(unix)]` + 真 `zgraph` 二进制 + `pgrep`/SIGINT/shell 依赖 unix 环境；文件头已声明「真 CLI/worker/SQLite」 | e2e 环境已满足；无需动作 |

## 4. 验证证据

worktree `/Users/zyzs/lczyzs/code/zgraph/.worktrees/e2e-e2e-dev`，HEAD `e1fdcd1`，base `main` `683260b`，工作区干净；`CARGO_HOME=/Users/zyzs/lczyzs/code/zgraph/.cargo-home`，未设 `CARGO_TARGET_DIR`：

```bash
cargo build -p zgraph-cli --bin zgraph        # exit 0（真二进制新构建）
cargo test -p zgraph-e2e                      # 14 targets：106 passed / 0 failed / 2 ignored
cargo test --workspace                        # 72 targets：915 passed / 0 failed / 3 ignored
cargo clippy --workspace --all-targets --locked -- -D warnings  # clean（exit 0）
cargo fmt --all -- --check                    # exit 0
bash scripts/dev/check.sh                     # PASSED

# 稳定性
for i in 1..6: cargo test -p zgraph-e2e --test dynamic_fanout   # 6×21 passed / 0 failed（1.28s 内）
for i in 1..3: cargo test -p zgraph-e2e                          # 3× 全量（各 target 均 ok，2 ignored 不变）
```

- **计数与作者自报一致**：e2e 106/0/2（dynamic_fanout 21）、workspace 915/0/3；clippy/fmt/check 全绿；**6 次 dynamic_fanout 与 3 次全量 e2e 均无 flake**；本次无 ISSUE-091 复现。
- **范围**：`git diff --name-only 683260b..e1fdcd1` 18 files：new e2e test 1 + worker 1 + graphs 11 + trace seed/golden/GENERATION 3 + Cargo 1；**无产品代码/DSL/trace/cli/kernel 源改动**；无新增 `#[ignore]`、无 Fake/mock executor（文件头「禁止 mock」；全部经真子进程）。
- **关键断言抽查**：a（F/W/E 各 item 0..2、J input 有序、join gate source=最后 `finished_at` 的 E item、expansion source=start、L1 逐字节、L2 10 条 matched）；b（同 hash N=2/4）；c（expansion count=0/keys=[]、end `[]`、无 GateResult）；d（NODE_PYTHON_EXCEPTION/Crashed/其它 Cancelled/无 join）；e/j（dup/canonical number/bool/null/object→`{"$index":i}`/真实 string 不撞）；f（max_items、NOT_ARRAY 三输入、controller skip）；g（SIGINT exit130/全部 item Cancelled/RunFinished Cancelled）；h（2 WaitingHuman 分别 decide/2 行 consumed/trace 顺序 events→HumanDecision×2→ForeachExpansion/`:memory:` 拒绝）；i（ForeachExpansion 行/--stats 键集/L1 replay==trace/旧 v5 迁移同库 foreach+L2/旧 v5 golden）；j（E item1 Skipped→ITEM_INCOMPLETE、无 Crashed、无 join）。

## 5. 偏差判读（Lead 关注 ①–⑥）

1. **依赖 unwrap fix（已合并）**：entry/worker 图均输出 object `{"items":[...]}`，依赖 kernel `REV-092` 规则；`cargo build` 新二进制 + e2e 通过，规则与 REV-092 一致。
2. **d/j 其它 item Cancelled vs Completed**：DR §6.6 的「其余 item 取消」应理解为**取消失效中的 item**；已 Completed 的保留。d 用 1s slow 使其它 item 仍在飞 → Cancelled；j 的 E 可能先完成 → 允许 Completed/Cancelled；两者不矛盾，语义正确（见 N2 建议文档化）。
3. **join source 运行时 last**：a 用 item1 `delay_ms=500` 制造最后完成；`finished_at` 最大 + item_index 平局断言 `last.item_index=1` 且 gate.instance_id=last；确定性足够，6 连跑稳定。
4. **region_overlap 不可达**：冻结优先级 row8 `external_edge` 先拦截；DSL 单测 `r18_region_overlap_is_checked_after_boundary_rows` 已锁；e2e 覆盖剩余 10 个 reason，分工合理。
5. **controller 5 类边界**：e2e 覆盖「全部 settle=false→Skipped」；Collecting deadlock/Ready/serial 等由 kernel REV-090 单测覆盖，e2e 分工合理。
6. **真二进制依赖**：build 步骤 + `cli_support::zgraph_command` + 真 worker/SQLite；符合本仓 e2e 模式，稳定性 6 次通过。

## 6. 遗留问题

- **W4b-docs（REV-094）**：回写 AC-17 a–j 实测测试名/计数（21/106/915）、d/j 取消语义、unwrap 规则（REV-092）、region_overlap 分工、v5 seed 口径。
- **P2 建议**：N1 补 e2e 真「object 无同名键」NOT_ARRAY；N2 在 docs 记录 d/j 终态语义；P3 N3 注释 120→500ms 等。
- 无 P0/P1；无附加条件；分支可合并。

## 7. 条件关闭记录（仅「有条件通过」填写）

不适用（结论为 `通过`，无附加条件）。
