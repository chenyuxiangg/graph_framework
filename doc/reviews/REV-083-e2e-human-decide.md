# REV-083 · e2e · W3a AC-16 a–g 真链路 + L2 human outputs

> **跨模块声明**：本分支新增/修改的 fixture 均属合同允许范围：`tests/fixtures/graphs/human_gate/**`（e2e 路径）、`tests/fixtures/trace/{schema_v4_seed.sql,v4_expected_trace.json,GENERATION.md}`。后者按 **ISSUE-111** 的归属裁定由 e2e 交付（trace seed/golden 与 W2a 的 `schema_v3_seed.sql`/`v0.1.0_expected_trace.json` 同口径；本分支只新增 v4 seed 与 v4-era golden、并在 GENERATION 中补 v4 生成口径），非静默越界。评审人只新增本记录，未改实现/测试。

- **分支**：`test/e2e-human-decide`
- **作者**：`e2e-dev`（`05383d0` fixtures + trace v4 seed/迁移；`5128f53` AC-16 a–g 真链路；`2ac54c2` L2 outputs 回放 human 用例；正文均含 `Review: REV-083` / `Agent: e2e-dev`）
- **评审人**：`reviewer-b`（独立评审，**≠ 作者**）
- **变更范围**：`9006345..2ac54c2`（9 files，+3439/−14）：新增 `tests/e2e/human_decide.rs`（2764 行，21 用例）、`tests/fixtures/graphs/human_gate/{approve,rework,reject_end,timeout}.json`、`tests/fixtures/trace/{schema_v4_seed.sql,v4_expected_trace.json}`，修改 `tests/e2e/Cargo.toml`（仅新增 `[[test]] human_decide`）、`tests/fixtures/trace/GENERATION.md`（v4 seed/golden 生成口径）
- **评审时间**：2026-11-01
- **关联**：task-14（W3a 合同）/ task-18（作者任务）、DR-006 §2/§4/§6/§7.3/§8/§10 AC-16、spec 07 AC-16、REV-079/080/081/082（trace v5/kernel/cli/L2 记录前置）

## 1. 结论

**结论：通过**

P0=0、**P1=0**、P2=0、P3=7（均为建议/记录项，无阻塞项）。AC-16 a–g 与 L2 human outputs 均已用**真 `zgraph` CLI + 真 Python worker + 真 SQLite**覆盖（f 的库 API 用例按合同允许 in-process），断言强度与 DR-006 冻结口径一致；未新增 `#[ignore]`、未放宽既有断言、未用 mock 替代真链路、未改产品代码/`doc/**`。P3 建议见 §3；无附加条件。

**AC-16 逐项核对**：

- **静态层/fixtures**：四个 `human_gate` fixture 真 `validate` 通过并冻结 `graph_hash`（`human_gate_fixtures_validate_with_frozen_hash_and_shape`）；`builtin.human` 冻结为 `python_node` 保留符号、无 `on_gate_false` 越界、rework 的 approved/rejected/prep 边 gate 谓词与 `on_gate_false` 显式冻结（`human_gate_fixtures_have_frozen_human_symbol_and_edges`）；`gate.function="human"` 仍 `DSL_SYMBOL_NOT_FOUND`/edge_index 定位（`gate_function_human_is_not_a_builtin_gate_op`）。
- **AC-16 a（approve 真链路 + L2）**：真 CLI run 挂起 → **轮询 DB 可见 `WaitingHuman` 后才 decide**（`poll_waiting_human`，10ms 轮询、30s 上限）；挂起期间 `pgrep -P <run_pid>` 无 worker 子进程；`runs.status=Running`；`instances.input` = 全部声明端口（artifact）且等于 prep 该代输出；human 迁移仅 `Pending→WaitingHuman(human_waiting)`；`decide approved` exit 0/`--json` 字段；run Completed、`run_output` 聚合 `{decision,reason}`、`killed_workers=0`、stderr 实时决策行；存储侧 output 非 NULL、`human_decisions` 恰一行且 `consumed_at` 非空、出边 GateResult passed、trace `--json` 追加 HumanDecision 行且排在事件行之后、人类可读摘要；`replay --mode=events --json` 与 `trace --json` **stdout 逐字节一致**。L2：`--record=full` 下 human 有 `recorded_outputs` 行（output_json + 64-hex blake3 复核）、`replay --mode=outputs` exit 0/matched=true/不产生新 run/不改 trace 文件字节；删 human 记录 → exit 3 `REPLAY_RECORD_MISSING` 快速拒绝；`--record=hash` 下 human 行 `output_json=NULL` 且 L2 replay exit 3。
- **AC-16 b（reject→rework/reject_end）**：reject 后 human attempt0 `Completed`（不 Failed）、defer 回边重入 prep/human → attempt1 `WaitingHuman` → approve → Completed；attempt 0/1 均 Completed、`gate_results` 逐 attempt deferred/passed 断言（approved 边 attempt0 false+deferred、attempt1 passed；rejected 边相反）、无 `RUN_DELIVERY_TO_TERMINAL`/死锁、两行决策均 consumed；reject_end：rejected settle 边 → end 正常 Completed、无 attempt 重入。L2：rework human attempt0/1 各一条记录、replay matched、`recorded_outputs=4`（prep×2+human×2）、read-only 文件快照不变。
- **AC-16 c（冲突/幂等/退出码/`--json`）**：用法错误（缺 approver/非法 decision/空 approver）exit 2 且 stdout 空；DB 不存在 exit 4 `HUMAN_DB_ERROR` 且**不创建空库**；v4 库 decide exit 4（`unsupported schema version=4, expected 5` + `run 'zgraph run' to migrate`）且不迁移；run 活跃时 instance 不存在 exit 1 `HUMAN_INSTANCE_NOT_FOUND`、非 WaitingHuman exit 5 `HUMAN_INSTANCE_NOT_WAITING`；首次成功；同实例二次/三次（同值/异值）恒 exit 5 `HUMAN_DECISION_EXISTS`；run Completed 后**存在性优先**仍 exit 5 EXISTS、无决策行的已终态实例 exit 1 `HUMAN_RUN_NOT_ACTIVE`；未知 run exit 1；决策恰 1 行、run 结果不被冲突改变。
- **AC-16 d（期限）**：`timeout_ms=200` 到期 → run Failed/exit 1 `NODE_TIMEOUT`、`killed_workers=0`、实例 TimedOut + `NodeError::Timeout`、`NodeTimedOut` 恰一次、超时不 publish（无 GateResult）、决策表为空；之后 decide → exit 1 RUN_NOT_ACTIVE。
- **AC-16 e（trace v5 迁移）**：v4 seed 只读 `trace`/`replay` exit 4 且**不迁移/无备份/行数不变**；真 CLI run 触发 v4→v5：user_version=5、`schema_migrations=1..5`、恰一份 `.bak.v4-*`（备份 user_version=4）、旧 run 五张表**逐列快照一致**（events 不改写、instances/transitions 重建逐列复制）、`human_decisions` 表 + pending 索引、无决策 run 不产生决策行；旧 run `trace --json` 与 **v4-era 冻结 golden** 一致、replay 与 trace 逐字节一致；新 run 无 HumanDecision 行。v3 seed → v3→v4→v5 链：v3/v4 备份各一份、旧 payload 保持 v3 形态、gate_results 三列为默认 0/0/NULL、L1 golden 不变、replay==trace。
- **AC-16 f（并发/库 API）**：同实例 5 个并发 `zgraph decide`（30s 上限）**恰 1 个 exit 0，其余全部 exit 5 `HUMAN_DECISION_EXISTS`**；run 正常 Completed、stderr 无 `SQLITE_BUSY`/`database is locked`/consumer 存储告警、human_decisions 恰 1 行且 consumed、run_error None、killed_workers=0；**重复 3 轮无 flake**。库 API：真 worker in-process（合同允许），`TraceSink` 仅实现快照/迁移/终态（**不实现 `human_decided`，默认 no-op 合法**），调用方直接 `RunControl::HumanDecision` → Completed/run_output/实例 output/迁移序列；断言不产生任何 DB/文件。`trace_enabled=false` + `NullTraceSink`：纯等待超过 deadlock_grace 仍 Running（非静默、无 RUN_DEADLOCK），Cancel→Cancelled（≠Flaky sleep 猜测，500ms 仅用于超过 200ms grace）。
- **AC-16 g（默认/golden/`:memory:`）**：6 个默认代表图（v0.1.0 customer_ticket/valid_parallel/valid_cycle/valid_subgraph + W2a valid_gate_defer + W2b valid_max_fanout）`graph_hash`/validate 不变；无 human 图真 run 无 WaitingHuman/human_decisions/HumanDecision 行、事件数不变、replay==trace；human 图 + `--db :memory:` → **启动前拒绝** exit 3、stderr `RUN_ENTRY_INVALID`+`trace_required_for_human`、stdout 空、**不产生 run/DB 文件**。W1 SOP 的 hash/真链路不变由既有 `software_team_v1` 套件（`approved_codegen_hashes_match_w1_baselines` 等）继续覆盖（本分支未改 W1 测试）。

## 2. 四维评审

| 维度 | 结论 | 依据（文件/行/实测） |
|---|---|---|
| 功能正确性 | ✅ | AC-16 a–g 与 L2 全部落实到可执行断言（见 §1）；等待可见性、无 worker、input/output、consumed、trace/replay、rework attempt、reject_end、timeout、冲突退出码、并发恰一、v4/v3 迁移、`:memory:` 拒绝均有独立用例；`cargo test -p zgraph-e2e` **85/0/2**、workspace **819/0/3**、clippy/fmt/check 全绿；human_decide 连跑 3 次 21/0 无 flake |
| 代码结构合理性 | ✅ | 单文件按 AC 分区、helper 复用（`RunProcess`/`wait_child_output`/`poll_waiting_human`/`instance_state`/`TraceReader` 封装）；fixture 与 trace seed 放合同路径；`tests/e2e/Cargo.toml` 仅新增 `[[test]]`，无依赖/其它 target 改动；未改产品代码/`doc/**` |
| 逻辑性 | ✅ | 所有等待用轮询+上限（无 sleep 猜测）；每用例独立 `TempDir`/DB；子进程 `Drop` kill 兜底；并发用例内重复 3 轮；L2 读写用 DB 文件字节快照 + run 计数双断言；迁移逐列快照（含 events/instances/transitions/runs/gate_results）；无吞错误/静默跳过（缺 pgrep 时 panic 而非 skip） |
| 可读性 | ✅（P3 备注） | 文件头说明数据通路与 AC 映射；fixture/常量命名达意；断言带上下文；注释解释 why（R15 defer 结构、hash 模式、ISSUE-089 文件快照时序、E-6 边界）；P3 为少量可移植性/可复现性说明 |

## 3. 发现清单

> 级别口径：P0 = 阻塞；P1 = 重要；P2 = 一般；P3 = 建议（可后置，建议进台账）。本分支 **P0=0、P1=0、P2=0**。

| # | 级别 | 位置 | 问题 | 建议 | 状态 |
|---|---|---|---|---|---|
| N1 | P3 建议 | `tests/e2e/human_decide.rs:866-887`（`assert_no_worker_child`） | 用外部命令 `pgrep -P <run_pid>` 断言挂起期间无 worker 子进程；`pgrep` 在目标 Unix 环境通常可用，缺失时测试 panic（符合「不静默跳过」）；但仍是对 CI 镜像的外部依赖 | 在 e2e 环境前置声明 `pgrep`（macOS/Linux 自带）或加 `ps -o pid= -P` 回退；若 CI 镜像不稳定可登记一条 P3 环境项 | open（建议项） |
| N2 | P3 建议 | `tests/fixtures/graphs/human_gate/rework.json`（prep→rework_human 边） | rework fixture 用「恒真 gate（`check_output{path:"node_id",equals:"prep"}`）+ `on_gate_false:"defer"`」满足 R15（defer 边源在含 defer 边的 SCC 内），defer 属性在 prep 侧恒不触发；真实返工由 human→prep 的 rejected defer 回边承载。属测试图的结构性 workaround，已在 fixture description/测试注释说明 | 无需改；建议 W3a-docs 在 07 AC-16 b 的 fixture 说明中保留该理由（避免后续误删 `on_gate_false` 触发 R15 失败） | open（建议项/文档） |
| N3 | P3 建议 | `tests/fixtures/trace/{schema_v4_seed.sql,v4_expected_trace.json}`；`GENERATION.md` §5 | v4 seed 与 v4-era golden 由 **v4-era 代码（`1ed37ab`）** 一次性生成并归一化（GENERATION §5 已记录口径）；CI 不重新生成该 golden，属冻结外部资产 | 可接受；如需更高可复现性，后续可加「v4-era CLI 重生成脚本 + 字节比对」的脚本级校验（非本任务必须） | open（建议项） |
| N4 | P3 建议 | `tests/e2e/human_decide.rs:2700-2764`（hash 模式用例） | `--record=hash` 时 human 行写入 `recorded_outputs` 但 `output_json=NULL`（fix/cli-human-record 的设计预期）；本用例已锁住该行为与 L2 exit3。属跨分支语义（非 W3a0 冻结段显式描述），易被后续误认为缺陷 | W3a-docs 在 06 §6.2.13/§6.10.7 或 DR-006 §14.3 记录「hash 模式 human 行存在、output_json NULL；L2 仅 full」，并保留本用例名 | open（建议项/文档） |
| N5 | P3 建议 | `ac16g_default_graph_hashes_and_validate_unchanged` / `ac16g_default_run_trace_has_no_human_artifacts` | AC-16 g 的本分支用例为**代表性抽样**（6 图 hash/validate + seed 图真 run）；W1 SOP 图与 W2a/W2b 真链路 hash/run 不变由既有 `software_team_v1`/`gate_defer`/`max_fanout` 套件覆盖（本分支全 workspace 全绿）。建议在 07 AC-16 g 的 text 中交叉引用既有套件，避免读测者误以为 W1 未被覆盖 | W3a-docs 在 07 AC-16 g 补「W1 SOP 由既有 AC-13/AC-14 套件覆盖」一句 | open（建议项/文档） |
| N6 | P3 建议 | `tests/e2e/human_decide.rs:5`（`#![cfg(unix)]`）、`:1696`（timeout 下界 150ms）、`:2205`（trace_disabled 500ms sleep） | 文件整体 `cfg(unix)`（e2e 既有模式）；AC-16 d 用「visible 后 ≥150ms、≤10s」包络替代严格 200ms 下界；库 API trace-disabled 用固定 500ms 超过 200ms grace。均为合理稳定性包络，但建议在测试/CI 文档注明这些宽松边界的原因，避免被误读为断言不足 | W3a-docs/测试注释注明「超时 200ms 受 trace 可见延迟影响，故下界 150ms；500ms 仅用于跨过 deadlock_grace」 | open（建议项） |
| N7 | P3 建议 | `tests/e2e/human_decide.rs:2364-2375`（`assert_recorded_output`） | `output_hash` 复核调用 `zgraph_cli::run_cmd::output_hash`（与写端同一函数），属自洽校验而非独立重算；L2 replay 本身会独立复核 hash，故风险低 | 可接受；若追求独立校验可在 e2e 侧用固定 blake3 向量/独立实现重算（非必须） | open（建议项） |

## 4. 验证证据

评审人**亲自执行**（worktree `/Users/zyzs/lczyzs/code/zgraph/.worktrees/e2e-e2e-dev`，`export CARGO_HOME=/Users/zyzs/lczyzs/code/zgraph/.cargo-home`，未设 `CARGO_TARGET_DIR`，HEAD `2ac54c2`，工作区干净）：

```bash
cargo build -p zgraph-cli --bin zgraph --locked   # 真二进制新构建，exit 0
cargo test -p zgraph-e2e                          # 13 targets：85 passed / 0 failed / 2 ignored（human_decide 21/0，既有 ignored 不变）
cargo test --workspace                            # 819 passed / 0 failed / 3 ignored
cargo clippy --workspace --all-targets --locked -- -D warnings   # clean（exit 0）
cargo fmt --all -- --check                        # exit 0
bash scripts/dev/check.sh                         # CHECK PASSED

# 稳定性：human_decide 套件连跑 3 次
cargo test -p zgraph-e2e --test human_decide      # 21/0（1.92s）→ 21/0（1.83s）→ 21/0（2.10s），无 flake

git status --porcelain                            # 空
git merge-base HEAD main                          # 9006345（= 本地 main；origin/main 3d0e933 为祖先）
git diff --stat 9006345..HEAD                     # 9 files +3439/−14；仅 tests/e2e 与 tests/fixtures（无 crates/**、doc/** 产品/文档改动）
git log -1 --format=%B 05383d0/5128f53/2ac54c2    # 均含 Review: REV-083 / Agent: e2e-dev
grep -n "#\[ignore\]" tests/e2e/human_decide.rs   # 0（未新增 ignore）
grep -niE "fakeexecutor|mock" tests/e2e/human_decide.rs  # 无 mock/fake executor（只有真实 worker；库 API 用例为真实 executor + 只读 sink）
grep -c "^\[\[test\]\]" tests/e2e/Cargo.toml      # 13（原 12 + human_decide）
```

- 测试结果与作者自报一致：e2e **85/0/2（human_decide 21）**、workspace **819/0/3**；`doc/test-baseline.txt` 无新增失败（仍 0 名额）。
- 真链路证据：AC-16 a/b/c/d/f 均经真 `zgraph run`/`decide`/`trace`/`replay` 子进程 + 真 `workers.echo` Python worker + 真 SQLite；AC-16 f 库 API 用例按合同允许 in-process（真实 executor，无 DB/无 mock）；`--record=full` L2 human 记录经 fix/cli-human-record 前置（合并 `9006345`）在本分支验证。
- 范围与稳定性：每用例独立 `TempDir`/DB；`RunProcess::Drop` 兜底 kill；轮询 + 30s 上限；子进程输出线程读取防管道阻塞；L2 read-only 断言 DB 文件字节快照 + run 计数；无新增 ignore、无放宽既有断言、无产品代码/`doc/**` 改动。

## 5. 遗留问题

- **作者偏差 1–5 评估**：① `pgrep -P` 依赖——Unix e2e 可接受、缺失即失败（不静默跳过），建议 CI 环境声明/可回退（N1）；② rework fixture 恒真 gate+defer——满足 R15 的必要结构 workaround，真实返工由 rejected defer 回边承载，测试已断言 gate/defer 语义（N2）；③ v4 seed/golden 由 v4-era CLI 生成——GENERATION §5 记录可复现口径，接受（N3）；④ hash 模式 human 行 `output_json=NULL`——fix/cli-human-record 的预期行为，本分支用例锁定，W3a-docs 记录（N4）；⑤ 库 API 边界——合同允许 f in-process/无 DB，测试证明 trait 默认 no-op 合法、无文件产生。以上均**无需新登记 issue**（若 Lead 认为 N1/N3 需环境可复现性台账，可登记 P3）。
- **W3a-docs 后续**：07 AC-16 需回写本分支实测测试名/计数（human_decide 21、e2e 85/0/2、workspace 819/0/3、连跑 3 次）；06/DR-006 执行记录补 hash 模式 human 行、v4 seed 口径、pgrep/超时包络说明（N1–N6）。
- **跨模块 fixture 声明**：`tests/fixtures/trace/**` 按 **ISSUE-111** 归 e2e（本分支新增 v4 seed/golden + GENERATION 更新）；`tests/fixtures/graphs/human_gate/**` 属 e2e 合同路径；`tests/e2e/Cargo.toml` 仅加 target。
- 未发现需要回滚/返工问题；无阻塞项，无附加条件。

## 6. 条件关闭记录（仅「有条件通过」填写）

不适用（结论为 `通过`，无附加条件）。
