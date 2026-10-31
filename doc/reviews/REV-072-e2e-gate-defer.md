# REV-072 · e2e · W2a 返工 e2e + AC-14（主审）

- **分支**：`test/e2e-gate-defer`
- **作者**：`trace-writer`（4 提交：`8ec09f1` defer fixtures + v3 seed/回放、`b80ea74` codegen `--mode rework` + approved hash 基线、`4e79562` GENERATION 归一化规则、`d31e5d2` 打开 defer runtime + SOP rework c/e + h 真实值；均含 `Review: REV-072` / `Agent: trace-writer`）
- **评审人**：`reviewer-a`（**主审**，≠ 作者；独立评审）
- **变更范围**：`e7578f1..d31e5d2`，15 文件 **+2441/−61**：`tests/e2e/`（`Cargo.toml` +4、新增 `gate_defer.rs` +824、`software_team_v1.rs` +530）；`tests/fixtures/graphs/defer_gate/`（3 图 +56/+56/+39、`py/defer_stub.py` +69）；`tests/fixtures/graphs/software_team_v1/`（`codegen.py` +259/−59、`fake_llm_rework_roundtrip.json` +56、`fake_llm_rework_exhaust.json` +68、`py/software_team_stub.py` +15/−2）；`tests/fixtures/trace/`（`GENERATION.md` +107、`schema_v3_seed.sql` +193、`seed_graph.json` +34、`v0.1.0_expected_trace.json` +131）；未改 kernel/dsl/trace/cli/doc/`CLAUDE.md`/`doc/progress.md`/`doc/issues.md`
- **评审时间**：2026-10-31
- **关联**：DR-005 §6（SOP v1.1）/§7（AC-14 a–h）/§9（L2 兼容矩阵）/§11；spec 07 AC-14 与 §7.1/§7.2；REV-067（W2a0 冻结）/REV-069（dsl R15）/REV-070（trace v4）/REV-071（kernel 语义）；ISSUE-104（v3 seed W1 遗留缺口）/ISSUE-105（verdict_json 读取口径待定）；DR-004 §6.3
- **模块声明**：本分支属 `e2e` 模块；Lead 明确允许本分支改动 `tests/e2e/**`、`tests/fixtures/graphs/**`、`tests/fixtures/trace/**`（后者未列在 AGENTS §2 的 `e2e` 路径表中，但 DR-004 §6.3 已把 v0.1.0 seed 的交付责任定为 W1 e2e，本任务 Lead 亦书面批准）。

## 1. 结论

**结论：通过**

W2a（AC-14 a–h）的 e2e 验收在**真链路**上成立：真 `zgraph` 二进制（`cli_support::run_cli` 调用 `target/debug/zgraph`，缺二进制时先 `cargo build`，非 mock/in-process）+ 真 kernel + 真 Python worker 子进程 + 真 SQLite trace。覆盖：

- **a 自环 defer**：run `Completed`；`loop` attempts `0/1`；同边两行 gate `attempt0 passed=true/deferred=false/attempt=0/verdict=true`、`attempt1 passed=false/deferred=true/attempt=1/verdict=false`；attempt1 继承 `seed`、`again` 为 attempt0 整包 output（`gate_defer.rs:656-712`）。
- **b A→B→A**：A/B attempts 均 `0/1`、`Completed`，无 `RUN_DELIVERY_TO_TERMINAL`；A→B 两行均放行，B→A 第二行 false+deferred（`:714-750`；两图 `validate` 独立复跑通过 R13/R15）。
- **f dormant**：gate 恒 false → 1 行 deferred、run `Completed`、`error_code=None`（不误报 `RUN_DEADLOCK`），verdict 为**原始路径值** `true`（`:752-779`）。
- **g blocked**：R15-valid 变体（a 有 defer 自环）→ exit 1 / `RUN_DEADLOCK`、`b` 零实例、run 表 `error_message` 含 blocked；独立手工复跑得到 `deferred receipt(s) blocked on edge(s) [2]: target node(s) [b] never instantiated`（`:781-824`；message 断言强度见 N1）。
- **h schema v4**：v3 seed（**独立复核确由 v0.1.0 tag `5d43c58` 生成**）→ 真 CLI `run` 触发 v3→v4；`user_version=4`、`schema_migrations` 1..4、恰一份 `.bak.v3-*`（备份 user_version=3、无 deferred 列、旧行保留）；旧行 SQL 默认 `deferred=0/attempt=0/verdict=NULL`；旧 payload 保持 v3 形态（无三键）；`trace` 旧 run == 冻结 golden（7 事件）、`replay --mode=events` 与 trace 逐字节一致；只读 v3 `trace` exit 4 且不迁移/不建备份；v4 新 run 的 `deferred/attempt/verdict` 为真实值（seed run `verdict="seed"`；同库 defer 自环 DB 行 `0/1/0` 与 `1/0/1` + trace payload 的 true/false），超限那次自环 GateResult 不入库（`:284-624`）。
- **c SOP rework→approved**：smoke `11/21`，run `Completed`；3 item 的 W/R/P attempts `0/1`；P→archer 前进边与 P→W.feedback 回边真实 `deferred/attempt/verdict`（round1 `false/0/true/rework` 挂账、回边 `true/0/false/rework` 放行；round2 前进边 `true/1/false/approved`、回边 `false/1/true/approved`）；P attempt1 的 `review.content` 来自 round2 且含 round2 comments（防 stale-input）；archer 拿到返工后最终 W attempt 产物（`software_team_v1.rs:1560-1792`）。
- **e 恒定 rework 超限**：`Failed/NODE_INTERNAL`、恰好一个 `Crashed` W `attempt=3`、`instances.error.kind=Internal`、无残留非终态；**不写死 victim**（从 Crashed 节点反推 item，再断言该 item W `0..=3`、R/P 三轮），超限 feedback 边只有 attempt 0/1 两行（`software_team_v1.rs:1794-1907`）。
- **approved-only W1 回归**：codegen approved 输出与 base codegen **逐字节一致**（full/smoke/exhaust 三组独立对比），三基线 hash `3d8d735b1d5e8a1b`/`df9341c04063b80a`/`a8e508deb9c6cf44` 逐字节未变；既有 W1 e2e 断言零删改。

验证：`cargo test -p zgraph-e2e` **58 passed / 0 failed / 2 ignored**（gate_defer 8、software_team_v1 11+1 ignored；ignored 均为既有 real-LLM 手动用例，无新增 `#[ignore]`）；`cargo test --workspace` **694 passed / 0 failed / 3 ignored**（失败基线 0）；`cli` bin build、`clippy -D warnings`、`fmt --check`、`check.sh` 全部通过；`gate_defer + software_team_v1` 连跑 3 次、`smoke_rework` 过滤连跑 5 次均稳定（无 flake、无 sleep 猜时序）。

发现 4 条**建议**（N1–N4，均非阻塞）：g 的 deadlock message 断言被弱化、AC-14 h 的 L2 outputs 子项无直接 e2e、4 个新增函数 >60 行、h 落点与 DR-005 §7.1 不同。无阻塞/重要项，无未关闭条件。

## 2. 四维评审

| 维度 | 结论 | 依据（文件/行/测试） |
|---|---|---|
| 功能正确性 | ✅（N1/N2 备注） | a/b/f/g/h 与 c/e 断言逐项对照 DR-005 §7 矩阵（明细见 §1）；真链路（`cli_support::run_cli` → 真二进制；`TraceReader` 读真 SQLite）；独立手工复跑 blocked/overlimit 图与 e2e 断言一致；v0.1.0 seed 独立复现：真 `zgraph 0.1.0` 跑 `seed_graph.json` 得 user_version=3/Completed/7 events/1 条 passed=0 gate，schema 对象 31/31 与 fixture DDL 逐字节一致、数据表内容与 fixture 在 UUID/时间戳归一化后逐表相等；迁移后 trace 与真 v0.1.0 库迁移后 trace（归一化）均等于 golden；approved 三 hash 独立 validate 复现。缺口：g 的 message 断言弱化（N1）、h 的 L2 outputs 子项未直接覆盖（N2） |
| 代码结构合理性 | ✅（N3 备注） | 15 文件全部落在 Lead 允许的 e2e 资产路径；`Cargo.toml` 仅加 1 个 test target；fixtures 放 `defer_gate/` 子目录（避免破坏 dsl `every_fixture_is_referenced` 根目录 golden 扫描，测试内有防回归断言）；`codegen.py` 以 `--mode` 增量扩展、approved 缺省路径逐字节不变、rework 生成后自检（W feedback/P v2/retry/defer 全覆盖）漏标即报错；`software_team_stub.py` 仅在输入**存在** feedback 键时追加字段，approved 输出不变；改动未触碰产品 crate/文档/台账。新增函数超 60 行见 N3 |
| 逻辑性 | ✅ | codegen 确定性（无随机/时间，字典构造序，两次生成字节级一致；approved 与 base 逐字节一致）；seed 归一化规则按表角色固定时间戳/UUID/pid，独立用真 v0.1.0 重跑对照；迁移/回放断言顺序符合 AC-14 h；c 的 fake 调用序（planer→round1×3→round2×3→archer）与断言锁定「P1 消费 round2 review」，连跑 5 次稳定；e 的并发非确定性用「动态 victim + attempt 连续 + 无残留非终态」硬事实约束，不假设所有 item 都到第 3 轮；dormant/blocked 不依赖 `deadlock_grace`（e2e 直接判定进程退出，kernel 侧另有 grace 证据）；无 sleep/轮询猜时序、无吞错 |
| 可读性 | ✅（N1/N3/N4 备注） | `gate_defer.rs` 文件头说明真链路/三部分/稳定性约定；`GENERATION.md` 完整记录生成/归一化/校验命令与「不得手改」约束；函数/常量命名清晰（`PROFILE_*`/`MODE_*`/`SEED_RUN_ID` 等）；断言消息含上下文。不足：g message 断言文字弱（N1）、4 个函数过长（N3）、h 落点与冻结表不同（N4） |

## 3. 发现清单

| # | 级别 | 位置 | 问题 | 建议 | 状态 |
|---|---|---|---|---|---|
| N1 | 建议 | `tests/e2e/gate_defer.rs:819-823` | AC-14 g 的 `RUN_DEADLOCK` message 断言被弱化：`error_message.contains("blocked") && error_message.contains("b")` 中 `"b"` 被 `"blocked"` 包含而恒真（相当于只断言 `"blocked"`），**未断言 edge 索引**，也**未真正验证 target node**；DR-005 §7 g 要求 message 含 edge 索引与 target node（实现实际输出 `... edge(s) [2]: target node(s) [b] ...`） | 改为断言 `contains("edge(s) [2]")` 与 `contains("target node(s) [b]")`（或逐字匹配）；若本分支不修，建议 Lead 登记台账随 e2e/Phase 2B 收口。不阻塞（kernel REV-071 单测已逐字锁定 message） | open（建议） |
| N2 | 建议 | `tests/e2e/software_team_v1.rs:1560-1907`（rework c/e） | DR-005 §7 h/§9 L2 行的「rework 图 `--record=full` + `replay --mode=outputs` 语义不变」无直接 e2e；当前 L2 回归仅 AC-01 approved 图（`cli_replay.rs`），本分支未对 rework 的 `(node_id, attempt)` 记录重放做验收 | 在 e2e/Phase 2B 或后续补：rework smoke `--record=full` → `replay --mode=outputs` exit 0、输出一致、不新写 run；或由 W2a-docs 明确「既有 L2 回归已足够」的口径。不阻塞（DR-005 §7 h 的其他子项与 c/e 均已覆盖） | open（建议） |
| N3 | 建议 | `tests/e2e/gate_defer.rs:286-509`（224 行）、`:517-591`（75 行）；`tests/e2e/software_team_v1.rs:1662-1748`（87 行 helper `assert_rework_chain`）、`:1794-1907`（114 行） | 4 个新增/改动函数超过「函数 ≤60 行」等价约束（测试/helper 同口径；与 ISSUE-060/089 同类）。功能与断言均有效，仅可维护性 | 拆 helper（如 v3 seed 测试拆「构造 seed / 触发迁移 / 断言备份与旧行 / 断言 golden 与真实值」；exhaust 拆 victim 查找与 attempt 断言；`assert_rework_chain` 拆 W/R/P 三段）；或登记 ISSUE 后置。不阻塞 | open（建议） |
| N4 | 建议（文档口径） | `tests/e2e/gate_defer.rs:1-30` vs DR-005 §7.1「h 落点 = `tests/e2e/cli_replay.rs`」 | AC-14 h 实现落在 `gate_defer.rs`（同 e2e crate、同真链路，功能等价），与 W2a0 冻结的测试落点表不同；后续读者按 §7.1 找 h 可能找不到 | 建议 W2a-docs 回写时同步落点表（改为 `gate_defer.rs`）或注明「h 与 a/b/f/g 同文件以便共享 seed/迁移辅助」。不阻塞 | open（建议） |

> 级别口径：阻塞 = 必须修复后才能给「通过」；重要 = 必须修复或登记 ISSUE；建议 = 可后置，但需修复或进台账。

## 4. 验证证据

```bash
# --- 范围/干净度 ---
cd /Users/zyzs/lczyzs/code/zgraph/.worktrees/e2e-e2e-writer
git status --porcelain --branch      # ## test/e2e-gate-defer...origin/main [ahead 4]；clean
git log --oneline -5
# d31e5d2 / 4e79562 / b80ea74 / 8ec09f1 / e7578f1（base = origin/main）
git diff e7578f1 --shortstat         # 15 files changed, 2441 insertions(+), 61 deletions(-)
git diff e7578f1 --name-status       # 全部 tests/e2e/** 与 tests/fixtures/{graphs,trace}/**
git diff e7578f1 --name-only | grep -v -E '^(tests/e2e/|tests/fixtures/graphs/|tests/fixtures/trace/)'
# 空 → 无越界/禁用/冻结文件改动（未改 kernel/dsl/trace/cli/doc/CLAUDE/progress/issues）

# --- 门禁（自跑，非作者自述）---
export CARGO_HOME=/Users/zyzs/lczyzs/code/zgraph/.cargo-home
cargo build -p zgraph-cli --bin zgraph --offline   # exit 0
cargo test -p zgraph-e2e --offline
# 58 passed / 0 failed / 2 ignored（gate_defer 8；software_team_v1 11+1 ignored；其余 target 不变）
cargo test --workspace --offline
# 694 passed / 0 failed / 3 ignored（66 个 test binary；doc/test-baseline.txt 失败基线 0 → 无新增失败）
cargo clippy --workspace --all-targets --offline -- -D warnings   # exit 0
cargo fmt --all -- --check                                        # exit 0
bash scripts/dev/check.sh                                         # CHECK PASSED（exit 0）

# --- 稳定性 ---
for i in 1 2 3; do cargo test -p zgraph-e2e --offline --test gate_defer --test software_team_v1; done
# 每次 gate_defer 8/0 + software_team_v1 11/0/1
for i in 1 2 3 4 5; do cargo test -p zgraph-e2e --offline --test software_team_v1 smoke_rework; done
# 每次 3 passed / 0 failed（roundtrip/exhaust 无 flake）

# --- 独立抽查 1：codegen approved 与 base 逐字节一致 / rework 规模与确定性 ---
# base codegen = git show e7578f1:tests/fixtures/graphs/software_team_v1/codegen.py
# approved full/smoke/exhaust（含 --review-agent design_reviewer_exhaust）：
#   byte-identical=True（4/4）；真 CLI validate：
#   full 60/100 graph_hash=3d8d735b1d5e8a1b；smoke 11/18 df9341c04063b80a；exhaust 11/18 a8e508deb9c6cf44（W1 三基线不变）
# rework：两次生成 byte-identical=True；full 60/116、smoke 11/21（与 SOP v1.1/DR-005 一致）；
#   validate：full graph_hash=6a7a4c30af2063ad、smoke 81ca6450700c4ed8（均通过 R13/R15）
# rework 策略独立读取：W.feedback required:false；P function=parse_verdict_rework；
#   W/R/P retry.max_attempts=3；defer 边 15 条（=3 feedback + 12 环/gate 边）；环上源无未标 defer 的边；feedback gate=check_output{verdict==rework}

# --- 独立抽查 2：v3 seed 确由 v0.1.0 生成且可复现 ---
# 使用 tag v0.1.0 = 5d43c58 的真实二进制（/tmp/zgraph-v010-target/debug/zgraph，zgraph 0.1.0 DSL 1.0）
# 跑 tests/fixtures/trace/seed_graph.json → user_version=3、status=Completed、events=7、gate_results=1 条 passed=0、无 v4 列
# 与 fixture 对比：sqlite_master 对象 31/31 名称与 DDL 文本逐字节一致；
#   表数据（graphs/schema_migrations/runs/instances/events/transitions/gate_results）在 fixture 记录的归一化
#   （UUID/时间戳/owner_pid/owner_host）下逐表相等；旧 GateResult payload 无 deferred/attempt/verdict
# fixture 自身：schema_migrations=1,2,3；runs 1 条 Completed（started_at T1、finished_at T2）；
#   实例 start Completed / b Skipped；events 7（2 个归一化时间戳）；sqlite_sequence 与真 dump 一致

# --- 独立抽查 3：迁移/回放/真实值（h）---
# 复制 fixture 建 v3 库 → 真 CLI run 触发迁移：
#   user_version=4；schema_migrations=1,2,3,4；旧行 deferred=0/attempt=0/verdict_json=NULL；
#   备份恰 1 份 *.bak.v3-*（user_version=3、gate_results 无 deferred 列、保留 1 行）
#   trace 旧 run（7 事件）== tests/fixtures/trace/v0.1.0_expected_trace.json（解析后逐项相等）
#   replay --mode=events stdout 与 trace --json stdout 逐字节一致（cmp）
#   v4 新 run 的 GateResult：{passed:false, deferred:false, attempt:0, verdict:"seed"}
#   未迁移 pristine v3 副本：只读 trace exit 4（stderr 含 unsupported schema version=3, expected 4），
#   user_version 仍 3、无备份（不迁移不写库）

# --- 独立抽查 4：defer 图手工复跑（真 CLI）---
# defer_blocked：exit 1 / Failed / RUN_DEADLOCK，
#   message="deferred receipt(s) blocked on edge(s) [2]: target node(s) [b] never instantiated"；
#   gate_results=3 行（start→a passed=1/deferred=0；自环与 a→b deferred=1）
# defer_self_loop + retry.max_attempts=1 变体：loop 0 Completed / 1 Crashed(NODE_INTERNAL)；
#   gate_results 只有无 gate 的 start→loop（edge0）1 行，自环边（edge1）0 行 → 超限 GateResult 不入库
```

- 测试结果：e2e **58 passed / 0 failed / 2 ignored**（base `e7578f1` 为 46/2，本分支 +12：gate_defer 8 + software_team_v1 4）；全量 workspace **694 passed / 0 failed / 3 ignored**；与 `doc/test-baseline.txt` 失败基线 0 对照无新增失败。
- `#[ignore]`：仅 2 个既有 real-LLM 手动用例（`software_team_v1::real_llm_smoke_manual`、`harness_ac08::ac08b_real_llm_manual`），本分支无新增 `#[ignore]`（`8ec09f1` 的 4 个临时 ignore 已在 `d31e5d2` 移除，最终树无残留）。
- 无 sleep/时序猜测：新增测试无 `sleep(`（`gate_defer.rs:30` 注释即「不做 sleep/时序猜测」），失败判定基于 CLI 退出码/DB；c 的并发用调用序脚本 + 内容断言，e 的并发用动态 victim + 尝试号连续等硬事实。
- 本评审未改产品代码/测试；评审记录与索引行属 `docs` 模块（按 AGENTS §3/§6 落在本分支，作者 trace-writer、评审人 reviewer-a ≠ 作者）。

## 5. 遗留问题

- **N1–N4 为建议（非阻塞）**：N1（g message 断言）建议随本分支或 e2e Phase 2B 修；N2（rework L2 outputs）建议 e2e 后续补或 W2a-docs 明确口径；N3（>60 行函数）建议拆 helper 或登记台账；N4（h 落点）建议 W2a-docs 同步落点表。本评审按授权只改 `doc/reviews/**`，未改产品/测试/台账。
- **台账缺口（process/docs）**：spec 07 / DR-005 / GENERATION.md / 测试注释引用 **ISSUE-104**（W1 v3 seed 遗留缺口）与 **ISSUE-105**（verdict_json 读取口径待定），但当前 `doc/issues.md` 最后一条为 ISSUE-103；建议 Lead 在 W2a 台账批次补登记（本分支不应改台账）。
- **模块路径口径（process/docs）**：AGENTS §2 的 `e2e` 路径清单未含 `tests/fixtures/trace/**`（本任务 Lead 已书面允许，DR-004 §6.3 也把 seed 交付定为 W1 e2e）；建议后续更新 AGENTS/worktree 文档或在 W2a-docs 记录该批准。
- **golden 来源说明**：`v0.1.0_expected_trace.json` 按 GENERATION.md 由**当前 CLI** 生成（golden），`trace == golden` 主要锁定「迁移不改写旧 payload + 输出稳定」；本次评审已额外用真 v0.1.0 库迁移后的 trace 做过对照（归一化后一致），建议 W2a-docs 保留该生成/复核说明。
- **合并顺序**：Lead 已声明先合 e2e、再合 W2a-docs；本记录未 merge/push/切 main；分支 base = `origin/main`（`e7578f1`），worktree clean。
- **计数**：e2e 46/2 → 58/2（+12）；workspace 694/0/3；无新增失败、无越界/冻结文件改动。

## 6. 条件关闭记录（仅"有条件通过"填写）

不适用（本记录结论为「通过」，无未关闭条件；N1–N4 为非阻塞建议）。

## 7. 追加复审（N1 关闭，2026-10-31）

- **范围**：`2b2efb2..301b91f`，1 提交 `301b91f`（`test(e2e): 收紧 AC-14 g deadlock message 断言（REV-072 N1）`，正文含 `Review: REV-072` / `Agent: trace-writer`），**1 file +9/−2**（仅 `tests/e2e/gate_defer.rs`；`git diff 2b2efb2..301b91f --name-status` 无其它文件）。
- **N1 关闭证据**：
  - 删除原恒真断言 `contains("blocked") && contains("b")`（`"b"` 被 `"blocked"` 包含而失效、未锁 edge）；
  - 改为两条**确定性逐字片段**断言：`contains("deferred receipt(s) blocked on edge(s) [2]")` + `contains("target node(s) [b] never instantiated")`——`[2]`/`[b]` 均无法被 `"blocked"` 子串满足，**不再恒真**，且同时锁 edge 索引与 target node；
  - 保留 exit 1 / `RUN_DEADLOCK` / `b` 零实例 / 3 行 gate / 2 条 deferred 等既有断言（diff 仅 message 段 +9/−2，无其它语义改动）。
  - 口径核对：实际 message 为 `deferred receipt(s) blocked on edge(s) [2]: target node(s) [b] never instantiated`（独立手工复跑同图确认）；与 kernel REV-071 `defer_gate::blocked_deferred_receipt_fails_immediately_with_deadlock` 的 `edge(s) [2]`/`[b]` 断言口径一致（本节断言比 kernel 片段更严格）。
- **独立验证**（HEAD `301b91f`）：
  ```bash
  cargo test -p zgraph-e2e --offline   # 58 passed / 0 failed / 2 ignored（gate_defer 8 passed / 0 failed / 0 ignored）
  cargo test -p zgraph-e2e --offline --test gate_defer   # 8 passed / 0 failed（含 blocked_deferred_receipt_fails_with_run_deadlock）
  bash scripts/dev/check.sh            # CHECK PASSED（exit 0）
  git status --porcelain --branch      # ## test/e2e-gate-defer...origin/main [ahead 6]；clean
  ```
- **其它建议处置**（按 Lead 安排）：N2（rework L2 outputs 覆盖）与 N3（>60 行测试函数）**转 ISSUE-110**，由 W2a-docs 批次登记台账；N4（h 落点与 DR-005 §7.1 差异）由 W2a-docs 校准落点表。
- **结论**：N1 关闭；REV-072 结论维持 **通过**（无阻塞/重要项；N2/N3 转 ISSUE-110、N4 待 W2a-docs，均不阻塞 e2e 合并）。本追加段提交只改 `doc/reviews/**`，未改产品/测试。

## 8. 增量复核（merge 门禁 flake 加固：单 item rework profile，2026-10-31）

- **背景/根因**：e2e 分支 merge 门禁在 `cargo test` 阶段偶发失败——c 的 `smoke_rework_roundtrip_...` 期望 W attempts `0/1`、实际 `[0,1,2]`。根因 = fake LLM 按**进程内全局调用序**消费脚本，3 item 并发流水线下不同 item 的 round1/round2 评审调用会交错消费脚本条目（某 item 把 round2 条目当 round1 用）→ 多一轮 rework。属**测试夹具与并发拓扑的确定性缺陷**，不是产品实现或断言错误。
- **修复范围**：`7e279c2..c6d172f`（HEAD `c6d172f`），3 文件 **+122/−97**：`tests/e2e/software_team_v1.rs`（c/e 改用 `rework-item`；e 恢复确定 victim 并加强断言）、`tests/fixtures/graphs/software_team_v1/codegen.py`（新增 `--profile rework-item`，仅 `--mode rework`，5 节点/7 边）、`fake_llm_rework_roundtrip.json`（4 calls：planer→rework→approved→archer）。未改产品/文档/台账；无越界。
- **确定性证据（独立复跑，非作者自述）**：
  - codegen `--profile rework-item --mode rework`：5 节点/7 边，两次生成字节一致；真 `zgraph validate --assets-dir tests/fixtures/graphs/software_team_v1` → `graph_hash=79dd51c684ffc771`（与作者自述一致）；非 rework mode 显式拒绝（ValueError）。
  - 真 CLI 跑 roundtrip（fake 4 calls）：run `Completed`；instances W/R/P 均 attempts `0/1` 且 `Completed`、archer attempt0 `Completed`；forward 边（P→archer）attempt0 `passed=false/deferred=true/attempt=0/verdict="rework"`、attempt1 `true/false/attempt=1/"approved"`；feedback 边 attempt0 `true/false/attempt=0/"rework"`、attempt1 `false/true/attempt=1/"approved"`；`P1 review.content` 为 round2 `approved`（含 round2 comments，无 stale-input）；`--record=full` 下 `llm_calls` 恰 4 条，行序 = planer→rework→approved→archer（单 item 后与 item 并发无关）。
  - 真 CLI 跑 exhaust（脚本 planer+9 rework，单 item 仅消费前 4 条）：exit 1 / `Failed NODE_INTERNAL`，message=`defer re-entry of node 'planer_specification' exceeded max_attempts (3) on edge 6 (attempt 3)`；W attempts `[0,1,2,3]`（`3` Crashed）、R/P `[0,1,2]` 全 Completed、archer **0 实例**；forward 3 行均为 `(attempt, false, true, Some("rework"))`；feedback 仅 attempt `0/1` 两行（超限 attempt2 的 GateResult 不入库）。
- **断言未削弱（逐条对照 §3–§7）**：c 的 W/R/P attempts、forward/feedback 真实 `deferred/attempt/verdict`、P1 review=round2、archer 最终 artifact（`feedback.verdict="rework"`）全部保留；e 由「动态 victim + 全 item attempt 连续」改为**单 item 下固定 victim `planer_specification`**（单 item 图唯一 W，固定值可确定），并**加强**：新增 message 点名 victim、forward 三轮精确值、feedback 两行精确值；「超限那次 GateResult 不入库 / 无残留非终态 / archer 零实例」全部保留。approved W1 三 hash 断言与既有 profile 输出不变：独立 codegen 对比 approved full/smoke/exhaust、rework full/smoke **与修复前逐字节一致**，真 validate 复现 W1 三基线。
- **稳定性（独立自跑）**：`cargo test -p zgraph-e2e` 连跑 **3 轮** → 每轮 **58 passed / 0 failed / 2 ignored**（gate_defer 8/0/0）；4 个 busy-loop 常驻下 `software_team_v1` 连跑 **3 轮** → 每轮 **11 passed / 0 failed / 1 ignored**；空载 `cargo test --workspace` → **694 passed / 0 failed / 3 ignored**；`check.sh` **PASSED**。未出现除既有 ISSUE-091（ac07/ac09 时序用例，本轮未触发）以外的 flake。
- **覆盖 tradeoff（记录）**：c/e 运行期由 smoke **3 item**（11/21）改为单 item `rework-item`（5/7），**3 item 同阶段并发的 e2e 覆盖减少**；并发重入/按端口继承/attempt 语义由 kernel `defer_reentry`（REV-071）覆盖，rework full 60/116 与 smoke 11/21 仍由 codegen/validate/确定性静态断言覆盖；**full 60/116 的运行期 e2e 需后续补充，按 Lead 安排登记 ISSUE-112**。N2/N3 维持转 **ISSUE-110**（W2a-docs 登记），N4 由 W2a-docs 校准。
- **结论**：单 item 修复消除 merge 门禁 flake 根因，断言强度不降反升；REV-072 结论维持 **通过**，无未关闭条件。本追加段只改 `doc/reviews/**`，未改产品/测试。
