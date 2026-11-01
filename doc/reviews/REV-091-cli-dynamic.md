# REV-091 · cli · W4b foreach 展开 trace/replay 输出

> **范围声明**：仅 `crates/zgraph-cli/**`（4 files，+1231/−41），`run`/退出码/CLI 参数未改；无跨模块改动、无需第二评审。评审人只新增本评审记录，未修改实现/测试。

- **分支**：`feat/cli-dynamic`
- 分支: feat/cli-dynamic
- **作者**：`cli-dev`（提交 `59c7443`；正文 `Review: REV-091` / `Agent: cli-dev`）
- **评审人**：`reviewer-a`（独立评审，**≠ 作者**）
- **变更范围**：`519066d..59c7443`（**4 files, +1231/−41**）：`src/trace_cmd.rs`（+82）、`src/replay_cmd.rs`（+554）、`tests/foreach_dynamic.rs`（新增 626 行 / 9 用例）、`tests/trace.rs`（+10 断言）
- **评审时间**：2026-11-01
- **关联**：DR-007 §8.4（trace 输出）/§8.5（replay）/§9（CLI 影响）；spec 06 §6.10.4（trace）/§6.10.7（replay）W4a 段；spec 07 AC-17 a/c/h/l；task-35/39；REV-089（trace v6）/REV-090（kernel 动态扇出）

## 1. 结论

**结论：通过**

按 DR-007 §8.4/§8.5/§9 与 spec 06/07 W4a 段逐项核对，trace/replay 输出与 L2 per-item 重放实现一致；P0=0、P1=0、P2=0，P3 建议 8 条（含作者 a–e 判读；均不阻塞）。无 foreach 的既有 run 经评审人独立 A/B **逐字节不变**（5 种输出模式）。

### 1.1 合同覆盖对照

| 合同项 | 实现/证据 | 结论 |
|---|---|---|
| `trace` 人类输出 | `run_timeline` 在事件 → human 决策摘要后追加 `print_foreach_expansions`（`trace_cmd.rs:277/481`）：`foreach node='<F>' items=<N> keys=[...]` + `  source_instance=<id\|-> at=<ts>` 明细；无展开不输出（提前 return）→ 无 foreach run 逐字节不变 | ✅ |
| `trace --json` | 追加 `{"kind":"ForeachExpansion","run_id","expansion_id","node_id","source_instance_id","item_count","keys","at"}` NDJSON（`trace_cmd.rs:404`）；`source_instance_id` 为 null **不省略**；`keys` 为结构化数组（NULL 键 `{"$index":i}`）；不写 `events.kind` | ✅ |
| 行序确定 | 事件行 → `HumanDecision` 行 → `ForeachExpansion` 行；展开内部按 `(at, expansion_id)` 升序（`TraceReader::foreach_expansions` SQL `ORDER BY at ASC, expansion_id ASC`，`reader.rs:475-486`）；测试故意乱序写入并断言排序（`foreach_dynamic.rs:169-255`） | ✅ |
| `--stats` 不变 | `run_stats` 路径未改、不查 expansions；测试断言 `--stats` 输出不含 `foreach`（`foreach_dynamic.rs:331`） | ✅ |
| `replay --mode=events` 一致 | 复用 `trace_cmd::run_timeline`；测试断言 `--json` stdout 与 `trace --json` **逐字节相等**（含 expansion 行，`:331`）；既有 no-foreach 一致性测试仍绿 | ✅ |
| L2 查找键 | `RecordKey=(node_id,attempt,item_index)`（`replay_cmd.rs:279`）；`item_index` 由 `instances` 按 `instance_id` 提供（`:386`）、`recorded_outputs` 结构不变；非 foreach 实例 item_index=0 → 与旧 `(node_id,attempt)` 一一对应（golden 不变） | ✅ |
| `RecordedExecutor` per-item | 键含 `ctx.item_index()`（`:300-320`）；单测覆盖无 foreach item0 等价、hash 不符、缺 output_json、未知实例、重复记录（`:1153`） | ✅ |
| expansion 完整性预检 | `validate_foreach_expansions` 在 `load_recorded_graph` 后、**human handler 替换前**（`:341-360`）；`validate_foreach_records`（`:522`）：未知节点/非 foreach 节点/同 F 多行/`item_count`≠F item_index 集合/`keys` 非数组或长度不符/`keys` 元素非法/与 `item_key` 不符 → `REPLAY_RECORD_CORRUPT`；**region 已有实例但缺 expansion 行** → `REPLAY_RECORD_MISSING`；region 无实例（controller Skipped / 未展开）无 expansion 属合法 | ✅ |
| N=0 合法 | 纯函数单测 `validate_foreach_records_accepts_empty_expansion`（count=0/keys=[]/无实例 → 通过，`replay_cmd.rs:1314`） | ✅ |
| human 替换/不重新决策 | `replay_recorded_human_graph` 在校验之后调用（`:711`）；替换为 `Python{builtin,human}` 由 `RecordedExecutor` 返回记录输出，不进入 WaitingHuman、不启动 worker（既有 REV-082 行为保持） | ✅ |
| 真实重新展开 | L2 成功用真实 CLI + 真内核：`prep` 记录数组驱动 F controller 展开 N=2，F/W/E 按 `item_index` 取记录，join→end 聚合，重放输出 == 原输出（`foreach_dynamic.rs:511`） | ✅ |
| 无 foreach golden 不变 | 评审人独立 A/B（详见 §4.2）与既有 trace/replay 测试（cli 222 全绿）双重验证 | ✅ |
| 范围/`run` 不变 | `git diff --name-only main...HEAD` 仅 4 个 cli 文件；无 `run_cmd.rs`/Cargo 改动；无新 CLI 参数/退出码 | ✅ |

## 2. 四维评审

| 维度 | 结论 | 依据（文件/行/测试） |
|---|---|---|
| 功能正确性 | ✅ | §1.1 逐项成立；`cargo test -p zgraph-cli` **222 passed/0 failed**、`cargo test --workspace` **889 passed/0 failed/3 ignored**（首轮遇既有 ISSUE-091 flake，重跑绿，见 §4.1）；9 个新用例含 5 条失败路径 + 纯函数覆盖；L2 成功路径为真实内核重新展开 |
| 代码结构合理性 | ✅ | trace 层：取数（reader）→ 打印（两个 `print_*`）职责单一，复用同一 timeline 保证 events 模式一致；replay 层：`RecordKey` 单一类型别名、`index_recorded_outputs`/`ensure_executed_nodes_recorded`/`validate_foreach_records` 分层清晰、纯函数可单测；仅 cli/**，无跨模块 |
| 逻辑性 | ✅ | 校验顺序合理（plan → graph → expansion 完整性 → human 替换 → spec/重跑）；CORRUPT/MISSING 判据与 DR §8.5+§6.6 一致（Skipped 无实例合法）；`(at, expansion_id)` 排序由 reader SQL 保证；`item_index` 来源单一（instances 表）避免双写；无 foreach 时早退/空循环零副作用 |
| 可读性 | ✅（N1/N2/N6/N7 备注） | 模块注释均引用 DR-007 章节并解释「为什么」（为何替换 human、为何 miss 判据用 region 实例）；错误消息含 node/attempt/item/expansion 定位；测试名自解释；N1/N2 为文档字段列表需同步 |

## 3. 发现清单

> 级别口径：P0 = 阻塞；P1 = 重要（必须修复或登记 ISSUE）；P2 = 一般；P3 = 建议（可后置，应进台账）。本分支 P0=0、P1=0、P2=0。

| # | 级别 | 位置 | 问题 | 建议 | 状态 |
|---|---|---|---|---|---|
| 1 | P3 建议 | `doc/decisions/DR-007-...md` §8.4；`doc/spec-v1.2/06-存储与可观测.md` §6.10.4（W4a 设计段）；07 AC-17 a | 冻结的 `ForeachExpansion` JSON 字段列表为 `kind,run_id,expansion_id,node_id,item_count,keys,at`，**未含 `source_instance_id`**；人类输出也只冻结摘要行、未列 `source_instance`/`at` 明细；实现输出前者（Lead 预期「source null 不省略」）与后者明细。设计段本身标注「W4b 实现后转 as-built」 | REV-093（W4b-docs）as-built 回写时同步 06 §6.10.4/07 AC-17 a/DR-007 §8.4 的字段列表（补 `source_instance_id` 与人类明细行）；非本分支阻塞 | open（建议项） |
| 2 | P3 建议 | `crates/zgraph-cli/src/replay_cmd.rs:522-588`；DR-007 §8.5；spec 06 §6.10.7 | `REPLAY_RECORD_MISSING` 的判据细化为「图内 foreach 节点的 **region 已有实例**但无 expansion 行」，从而「controller Skipped/未展开 → 无实例无 expansion」合法；DR §8.5 只写「按 item_count/keys 校验完整性」，未写明该边界（作者 a 口径） | 判读**可接受**（与 §8.5 目的 + §6.6 Skipped 语义一致，纯函数单测已锁定未展开合法）；建议 06 §6.10.7 as-built 写明该口径 | open（建议项） |
| 3 | P3 建议 | `crates/zgraph-cli/tests/foreach_dynamic.rs`（L2 fixture `:382`） | CLI L2 测试使用**合成记录库**（真实 `TraceWriter`/`DecisionStore` 写入 + SQL 篡改，再真实 CLI 重放），未覆盖：N=0 的端到端 L2、controller-Skipped 的端到端 L2、per-item human 的 L2、旧 v5 迁移 run 的 per-item L2（作者 e 说明） | 判读**可接受**（CLI 层契约级已覆盖成功+5 失败路径、真实内核重新展开；纯函数覆盖 N=0/Skipped 合法）；上述端到端场景由 **e2e REV-092（AC-17 c/h/l/m）** 覆盖，CLI 侧可后续择机补 1–2 例 | open（建议项） |
| 4 | P3 建议 | `crates/zgraph-cli/src/replay_cmd.rs:589-620`；`crates/zgraph-cli/src/trace_cmd.rs:404-430` | `validate_one_expansion` 要求 `keys_json` 必为数组（NULL → CORRUPT）；`trace --json` 对 NULL keys 输出 `null`。CLI 写路径（bridge）恒写 `Some(Array)`，正常 run 不受影响；仅库 API/损坏库场景 | 判读**可接受**（防御性）；建议 06 §6.2.15/§6.10.4 as-built 注明「CLI 生成 run 的 keys 恒为数组；NULL 视为损坏」 | open（建议项） |
| 5 | P3 建议 | `crates/zgraph-cli/src/replay_cmd.rs:625-630` | `expected_indexes` 对 `item_count > u32::MAX` 用 `u32::MAX` 兜底（`unwrap_or`），理论上是超大 DB 的防御路径，错误信息可能不精确 | 正常路径不可达（keys 长度先校验）；可不改；若要严格可先 `u32::try_from` 报 CORRUPT | open（建议项） |
| 6 | P3 建议 | `crates/zgraph-cli/src/replay_cmd.rs:522-588` | 完整性预检以 **F 的 item 实例**为锚校验 count；region 内其它节点（W/E/…）的 item 实例集合完整性依赖 `ensure_executed_nodes_recorded`（记录存在性）+ 重放执行兜底（重建实例时缺记录 → 失败/不匹配） | 判读**可接受**（双重兜底足够；真实 run 由内核保证一致）；建议 06 §6.10.7 as-built 一句话注明该分工 | open（建议项） |
| 7 | P3 建议 | `crates/zgraph-cli/src/trace_cmd.rs:277-310` | `--kinds`/`--limit` 只作用于事件行，`ForeachExpansion` 行不受过滤（与 W3a `HumanDecision` 行既有口径一致） | 判读**可接受**；建议 06 §6.10.4 注明「决策行/展开行不参与过滤与截断」 | open（建议项） |
| 8 | P3 说明 | `zgraph-trace --test channels` | 评审首轮 workspace 出现既有 **ISSUE-091** flake：`flush_barrier_covers_streaming_events_beyond_one_batch` panicked `flush trace writer: Storage("FOREIGN KEY constraint failed")`（`crates/zgraph-trace/tests/common/mod.rs:48`）；与本分支无关（diff 仅 cli/**），重跑 `channels` 3×7/7 与 workspace **889/0/3** 全绿 | 维持台账 ISSUE-091/137（P2，复现升 P1）；merge 门禁若复现请重跑 | open（说明项） |

### 3.1 作者 a–e 判读

- **a) expansion 校验口径（count 以 F item_index 集合、MISSING 以 region 有实例为准）**：可接受，符合 DR §8.5 目的与 §6.6 Skipped 语义；纯函数测试锁定「未展开合法」；建议 docs 写明（N2）。
- **b) `keys` 比对（string 原值 / `{"$index":i}` 表示 NULL）**：正确，符合 DR §6.1/§8.3；非法元素、长度不符均 CORRUPT；单测覆盖显式 key/索引 key/非法 number。
- **c) 人类输出格式（摘要行 + `source_instance`/`at` 明细）**：可接受（信息增强、无 foreach 输出不变）；冻结文档只列摘要行 → REV-093 同步（N1）。
- **d) 行序 `events → HumanDecision → ForeachExpansion`**：可接受（Lead 预期；DR 仅要求「事件行后追加」）；建议 docs 明确三者相对顺序（N1）。
- **e) L2 测试用合成记录库**：可接受（真实 writer + 真实 CLI 重放 + 真内核重新展开，覆盖成功与 5 条失败路径）；真实 run 产物/Skipped/N=0/human 端到端由 e2e REV-092 覆盖（N3）。

## 4. 验证证据

评审人**亲自执行**（worktree `/Users/zyzs/lczyzs/code/zgraph/.worktrees/cli-cli-dev`，HEAD `59c7443`，基于 `main = origin/main = 519066d`，工作区干净；`CARGO_HOME=<repo>/.cargo-home`）：

### 4.1 测试与门禁

```bash
cargo test -p zgraph-cli                 # 222 passed / 0 failed / 0 ignored（与作者一致）
cargo test --workspace                   # 首轮命中既有 ISSUE-091 flake（trace channels，见下），重跑：889 passed / 0 failed / 3 ignored
cargo test -p zgraph-trace --test channels   # 3 次连跑均 7 passed / 0 failed（flake 不复现）
cargo clippy --workspace --all-targets --locked -- -D warnings   # clean（exit 0）
cargo fmt --all -- --check               # exit 0
bash scripts/dev/check.sh                # CHECK PASSED
```

- **flake 记录**：首轮 `cargo test --workspace` 在 `zgraph-trace --test channels::flush_barrier_covers_streaming_events_beyond_one_batch` panic：`flush trace writer: Storage("FOREIGN KEY constraint failed")`（`crates/zgraph-trace/tests/common/mod.rs:48`）。该 flake 与 W3a 评审时观察到的 **ISSUE-091/ISSUE-137** 同类（trace crate 既有、偶发、重跑通过）；本分支 diff 仅 `crates/zgraph-cli/**`，与 trace 模块无关；重跑 workspace 全绿。
- 范围：`git diff --name-status main...HEAD` = 仅 4 个 cli 文件；`run_cmd.rs`/Cargo/Cargo.lock 无 diff；提交正文含 `Review: REV-091` / `Agent: cli-dev`；worktree 干净。

### 4.2 无 foreach run 独立 A/B（main `519066d` CLI vs 分支 CLI）

```bash
# baseline：main worktree 重新 cargo build -p zgraph-cli --bin zgraph（HEAD 519066d，pre-W4b-cli）
# branch  ：.worktrees/cli-cli-dev/target/debug/zgraph（HEAD 59c7443）
# 用分支 CLI 真跑无 foreach 图（start→end）生成 v6 库，再对同一库比较 5 种输出：
trace <run> --db trace.db                 → SAME（人类输出逐字节）
trace <run> --db trace.db --json          → SAME（NDJSON 逐字节，0 条 ForeachExpansion）
trace <run> --db trace.db --stats         → SAME
replay --run <run> --mode=events --json   → SAME（与 trace --json 逐字节）
replay --run <run> --mode=outputs --json  → SAME（matched=true）
```

- 与作者「无 foreach run 输出逐字节不变」一致（评审人独立构建 baseline 并实跑，含 exit code/stderr）。
- `trace --json` 无 `ForeachExpansion` 行（grep 0）。

### 4.3 新代码/测试覆盖确认

- `tests/foreach_dynamic.rs` 共 **9** 用例：trace 人类顺序/字段（`:256`）、trace `--json` 行序与字段/source null 不省略（`:289`）、`--stats` 不变 + events 模式逐字节一致（`:331`）、L2 成功 per-item（`:511`）、count 不一致 CORRUPT（`:530`）、keys 不一致 CORRUPT（`:549`）、缺 expansion MISSING（`:568`）、缺 per-item 记录 MISSING（`:587`）、per-item hash 篡改 CORRUPT（`:606`）。
- `replay_cmd` 单测新增：per-item 记录键等价/重复/未知实例/hash（`:1153`）、expansion 一致/空/未展开合法/count/keys/非法行（`:1300-1430`）。
- `tests/trace.rs` 仅新增断言（无 foreach 不出现在 human/JSON/`--stats`），既有断言未放宽。
- `bridge.rs`（已由 kernel 分支合并）把 kernel `keys: Vec<Option<String>>` → trace `keys: Option<Value>` 恒为 `Some(Array)`（`bridge.rs:700-720`），保证 N=0 也写 `[]` 而非 NULL。

## 5. 遗留问题

- **P3 建议（N1–N7）未登记 `doc/issues.md`**（本分支合同仅 `crates/zgraph-cli/**`；台账由 docs/Lead 维护）：N1/N2/N4/N6/N7 建议由 **REV-093（W4b-docs）** 在 06 §6.10.4/§6.10.7、DR-007 §8.4/§8.5、07 AC-17 a 的 as-built 回写中同步；N3 的端到端覆盖由 **e2e REV-092（AC-17 c/h/l/m）** 补齐；N5 可不改。
- **ISSUE-091 flake**：与 W3a 评审观察一致，保持台账 P2（「复现升 P1」）；merge 门禁由 Lead 跑全量时若复现请重跑并注明。
- **后续**：本分支合并后进入 e2e（REV-092，AC-17 a–j 真链路）→ W4b-docs（REV-093）；本分支不涉及 run/decide/退出码变更。
- 未发现需要返工的问题；无阻塞项。

## 6. 条件关闭记录（仅"有条件通过"填写）

不适用（结论为 `通过`，无附加条件）。
