# REV-092 · kernel fix · foreach items_port 整包广播字段提取（W4b 裁决 A）

- **分支**：`fix/kernel-foreach-items-unwrap`
- **作者**：`kernel-dev`（单提交 `8846240`，基于 main `4a1ff0f`；正文含 `Review: REV-092` / `Agent: kernel-dev`）
- **评审人**：`reviewer-b`（独立评审，≠ 作者）
- **变更范围**：`4a1ff0f..8846240`（2 files，+166/−1）：`crates/zgraph-kernel/src/run.rs`（+15/−1）、`crates/zgraph-kernel/tests/foreach.rs`（+151）；**仅 kernel**，无 DSL/trace/CLI/文档改动
- **评审时间**：2026-11-01
- **关联**：task-48（修复任务/背景）、task-49（评审任务）、DR-007 §6.2/§6.3/§7、spec 03 §3.4.6 W4a 段、Lead 裁决 A（2026-11-01）

## 1. 结论

**结论：通过**

P0=0；**P1=0**；P2=0；P3=4（测试/文档补充建议，不阻断）。实现与 Lead 裁决 A 逐条一致：`v=controller.values[items_port]`；array 直用；object 含**同名键**→取该键值且必须 array；否则 `RUN_FOREACH_ITEMS_NOT_ARRAY`；只取一层、不做点分路径。NOT_ARRAY→EXCEEDED→DUPLICATE 顺序不变且作用于 unwrap 后列表；既有 144/154、cli 222、workspace 894/0/3 全绿；仅 kernel 改动。可合并，e2e 可基于该规则跑 AC-17 a–j。

## 2. 四维评审

| 维度 | 结论 | 依据 |
|---|---|---|
| 功能正确性 | ✅ | 规则与裁决 A 一致（`run.rs:1660-1680`）；object 同名键 unwrap、array 直用、其余 NOT_ARRAY；错误 message 的 `got` 取 unwrap 后实际类型；校验顺序/作用域正确；入口 start 整包 object 与 worker dict 形状均覆盖 |
| 代码结构合理性 | ✅ | 改动集中在 `controller_try_expand` 取值处（一行 match + 注释），不触 controller/join/DSL/trace/CLI；错误路径复用既有 `fail_run`/codes；未引入新类型或配置 |
| 逻辑性 | ✅ | 仅一层同名字段（不做点分）；对象无同名键/同名键非 array/标量均落 NOT_ARRAY；max_items/key_field 在 unwrap 后执行；direct array 行为不变；无 foreach 图不受影响 |
| 可读性 | ✅（P3 备注） | 注释明确 v1 整包广播来源（python dict/agent_call object/entry object）、array 直用/object 同名键/其余 NOT_ARRAY、只取一层；P3 为少量测试/文档补充 |

## 3. 发现清单

> P0/P1/P2 = 0；以下 P3 为建议，不影响合并。

| # | 级别 | 位置 | 问题 | 建议 |
|---|---|---|---|---|
| N1 | P3 | `tests/foreach.rs`（新增 unwrap 用例） | **缺 object 空列表 `{"items":[]}` 的 N=0 用例**：现有 empty 用例是 prep 直接返回 `[]`（array），unwrap 路径的 N=0 未被直接断言（实现上 unwrap 后 array 空与直用空等价） | 可补一例 `{"items":[]}` → expansion count=0、无 item 实例、J 收 `[]`；非必须 |
| N2 | P3 | `tests/foreach.rs`（新增 unwrap 用例） | 负例覆盖 object 无同名键/同名键非 array/标量（好）；未覆盖同名键值为 `null`（会走 NOT_ARRAY got null）与「object 含同名键数组但 `items_port` 名不同」的场景 | 可选补 null/异名键断言，锁住 `got` 语义与「同名字段」边界 |
| N3 | P3 | `src/run.rs` as-built 注释 | 注释已说明 v1 整包广播、array/object 同名键/其余 NOT_ARRAY、只取一层，足够 W4b-docs 回写；但本分支未动 spec/DR，**规则属 as-built 行为**，需 W4b-docs 在 03 §3.4.6/DR-007 执行记录中回写 | W4b-docs 回写「intake 取值规则 A」与错误语义（NOT_ARRAY/EXCEEDED/DUPLICATE 顺序作用于 unwrap 后列表） |
| N4 | P3 | `tests/foreach.rs` | 真 worker/agent_call 的 object 形状由 ScriptedExecutor object 覆盖，未用 agent_call 四字段 output 直测（e2e AC-17 会真链路覆盖）；属分工范围，非本分支缺陷 | e2e W4b 在 AC-17 中覆盖 agent_call/entry/worker 三种实际形状 |

## 4. 验证证据

worktree `/Users/zyzs/lczyzs/code/zgraph/.worktrees/kernel-kernel-dev`，HEAD `8846240`，base `main` `4a1ff0f`，工作区干净；`CARGO_HOME=/Users/zyzs/lczyzs/code/zgraph/.cargo-home`，未设 `CARGO_TARGET_DIR`：

```bash
cargo test -p zgraph-kernel                 # 24 targets：149 passed / 0 failed / 0 ignored
cargo test -p zgraph-kernel --features test-hooks   # 26 targets：159 passed / 0 failed
cargo test -p zgraph-cli                    # 10 targets：222 passed / 0 failed
cargo test --workspace                      # 71 targets：894 passed / 0 failed / 3 ignored
cargo clippy --workspace --all-targets --locked -- -D warnings  # clean（exit 0）
cargo fmt --all -- --check                  # exit 0
bash scripts/dev/check.sh                   # PASSED

git status --porcelain                      # 空
git merge-base HEAD main                    # 4a1ff0f（本地 main）
git diff --name-only main...HEAD            # 2 files：kernel src/run.rs + tests/foreach.rs；无其它模块
git log -1 --format=%B 8846240              # 含 Review: REV-092 / Agent: kernel-dev
```

- **计数与作者自报一致**：kernel 149/0、features 159/0、cli 222/0、workspace 894/0/3；`tests/foreach.rs` 20→**25 例**（新增 object unwrap、entry unwrap、负例三形态、unwrap 后 max_items、unwrap 后 key 重复）；既有 144/154 全绿。
- **规则核对**：`run.rs:1660-1680`：`port_value` 为 Object 且含 `items_port` 同名键 → 取该键值；array 直用；否则 NOT_ARRAY；`Value::Array(items)` 之后才做 max_items/key 校验；message 用 unwrap 后值类型。仅一层、无点分；非 foreach 代码路径未变。
- **测试证据**：entry start 注入 `{"items":[a,b,c]}` 经整包广播后展开 3；worker 形状 `{"items":[...],"meta":1}` 展开 3；`{"other":[...]}`/`{"items":{"nested":1}}`/string → NOT_ARRAY；`{"items":[...3]}` + max_items=2 → EXCEEDED；`{"items":[{"id":1},{"id":1}]}` + key_field → DUPLICATE；direct array 既有用例仍通过。
- **范围**：仅 `crates/zgraph-kernel/**` 2 files；无 DSL/trace/CLI/文档/配置改动；无需跨模块第二评审。

## 5. 遗留问题

- **e2e 后续**：按该规则跑 AC-17 a–j（worker dict/agent_call 四字段/entry object/空列表/负例）；W4b-docs 在 03 §3.4.6/DR-007 执行记录回写 as-built 规则（N3）。
- **P3 建议**：N1 object 空列表 N=0 用例、N2 null/异名键边界、N4 agent_call 真形状由 e2e 覆盖；均非阻塞。
- 无 P0/P1/P2；无附加条件；分支可合并。

## 6. 条件关闭记录（仅「有条件通过」填写）

不适用（结论为 `通过`，无附加条件）。
