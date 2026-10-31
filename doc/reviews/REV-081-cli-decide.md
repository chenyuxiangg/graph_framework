# REV-081 · cli · W3a `zgraph decide` + run decision consumer + trace/replay 决策行

> **范围声明**：本分支仅改 `crates/zgraph-cli/**`（task-14 §3 合同）；无跨模块源码改动、未改 kernel/trace/dsl、未改 `Cargo.lock`，`tests/e2e/**` 未动。评审人只新增本评审记录，未修改产品/测试代码。

- **分支**：`feat/cli-decide`
- 分支: feat/cli-decide
- **作者**：`cli-dev`（提交 `4d671a0` 阶段 1、`c711232` 阶段 2；正文 `Review: REV-081` / `Agent: cli-dev`）
- **评审人**：`reviewer-a`（独立评审，**≠ 作者**）
- **变更范围**：`390dbe1..c711232`（8 files, +2488/−51）：`src/{lib,bridge,decide_cmd,run_cmd,trace_cmd}.rs`、`tests/{decide,human_decide}.rs`（新）+ `tests/trace.rs`。`390dbe1` = 本地 main（含 trace v5 REV-079 + kernel WaitingHuman REV-080）；origin/main 为分支祖先。
- **评审时间**：2026-10-31
- **关联**：DR-006 §4.5（预检顺序）/§4.6（自动接续）/§6.4（trace 决策行）/§7.2（decide）/§7.3（run consumer，含 memory）/§8（兼容矩阵）；task-14/17；06 §6.10.5/§6.10.8；REV-079/REV-080

## 1. 结论

**结论：通过**

按 task-14 §3 / task-17 与 DR-006 §4.5/§4.6/§6.4/§7.2/§7.3/§8 逐项核对，全部成立；P0=0、P1=0、P2=0，P3 建议 8 条 + 1 条环境观察（非本分支引入），不阻塞合并。未放宽/删除既有断言；未 mock 真链路；作者说明 a–d 评估见 §3 N4/N5。

### 1.1 合同覆盖对照

| 合同项 | 实现/证据 | 结论 |
|---|---|---|
| **decide 参数/用法** | `decide_cmd::parse`（`decide_cmd.rs:107-197`）：必填 run_id/`--instance`/`--decision` 闭集/`--approver` 非空（trim 后空 → 用法错误）；`--flag value`/`=` 两种形式；未知 option、重复位置参数、缺值 → exit 2；`--help` exit 0；USAGE 注册（`lib.rs:86-91,248-257`） | ✅ |
| **预检顺序（C3 冻结）** | `decide_cmd::run`（`:198-318`）：① `precheck.decision_exists` → 5 `HUMAN_DECISION_EXISTS`（先于一切）→ ② run 不存在/非 `Running|Paused` → 1 `HUMAN_RUN_NOT_ACTIVE` → ③ instance 不存在/属他 run → 1 `HUMAN_INSTANCE_NOT_FOUND` → ④ 非 `WaitingHuman` → 5 `HUMAN_INSTANCE_NOT_WAITING`；预检为 `DecisionStore::precheck` 单快照 | ✅ |
| **退出码 0/1/2/4/5/70** | `EXIT_CONFLICT=5` 新增（`lib.rs:59-63`）；成功 0、用法 2、目标不存在/终态 1、非等待/冲突 5、存储 4、panic 由顶层 `run_with` `catch_unwind` → 70（`lib.rs:180-201`，既有单测覆盖） | ✅ |
| **decide 不建库/不迁移** | 经 `DecisionStore::open`（`ensure_existing_file` + 版本恰 5）；DB 不存在 exit 4 且**不创建文件**；v4 exit 4 且不迁移、stderr 追加 “run 'zgraph run' to migrate”（`decide_cmd.rs:330-338`） | ✅ |
| **INSERT 语义** | trace `DecisionStore` 单事务 Immediate、UUIDv7 `decision_id`、`decided_at` UTC RFC3339 毫秒 Z、`--reason` 原样（不 trim；未提供 NULL）、唯一键冲突 → 5 `HUMAN_DECISION_EXISTS`、其它存储错 → 4 `HUMAN_DB_ERROR`；只写 `human_decisions`（不改 runs/instances/events） | ✅ |
| **输出形状** | 人类成功 stdout 一行（`decision recorded: run=… instance=… decision=… decision_id=…`）；`--json` 成功恰 `{ok,run_id,instance_id,decision,decision_id,decided_at}`、失败恰 `{ok:false,code,message}`（stdout 只 JSON） | ✅ |
| **run decision consumer（§7.3）** | 仅 human 图、非 `:memory:` 启动（`run_cmd.rs:856-870,1380-1430`）：200ms RO 轮询（trace `DecisionConsumer`，busy 1000）；主循环对**所有 pending 一律**提交 `RunControl::HumanDecision`（不按 WaitingHuman 过滤；`submitted` 去重，控制失败回滚 key；`run_cmd.rs:1300-1330`）；bridge `HumanDecisionObserver` 转发内核确认；consumer 线程独立 RW `mark_consumed`（busy 5000、条件 `consumed_at IS NULL`，幂等）；失败告警一次 + 保留 NULL 下轮重试、**不 fail run**；stderr 决策行（applied / applied=false warning + reject_reason 三值），`-q/--quiet` 抑制；run 终态停止并 join 线程、退出前 flush 已确认行（`run_cmd.rs:1249-1297`）；全部 DB 访问在专线程，不阻塞事件循环、不执行用户 Python | ✅ |
| **bridge** | `HumanDecisionObserver` + `TraceSink::human_decided` 只转发、**不写决策/事件表**（C13；`bridge.rs:53-83,219-228`，单测断言无 events/human_decisions 行；无观察者 no-op） | ✅ |
| **human 图 + `--db :memory:`** | 启动前预检（`run_cmd.rs:846-855`，在 run_id 生成与 `start_run` 之前）：`RUN_ENTRY_INVALID` exit 3、message 含 reason `trace_required_for_human`、不产生 run、不打印 created/run id；普通图 `:memory:` 行为不变（对照测试） | ✅ |
| **trace/replay 决策行（§6.4）** | 事件输出后追加：人类 `human_decisions: N (approved A / rejected R; pending P)` + 逐行明细（`trace_cmd.rs:390-418`）；`--json` 追加 `{"kind":"HumanDecision",…}` NDJSON（字段 `kind,run_id,instance_id,decision,approver,reason,decision_id,decided_at,consumed_at`，pending `consumed_at` 恒 null 不省略；`:345-388`）；顺序由 trace reader `(decided_at, decision_id)` 保证；无决策行输出不变；`--stats` 不新增字段；`replay --mode=events` 复用同一 `run_timeline` → 与 trace **逐字节一致**；`--mode=outputs` 未改、不读决策表 | ✅ |
| **默认兼容红线** | 无 human 图：不启动 consumer、无新输出（`human_consumer=false`，仅多建两个空 channel）；既有 run/trace/replay/e2e 用例全绿 | ✅ |

## 2. 四维评审

| 维度 | 结论 | 依据（文件/行/测试） |
|---|---|---|
| 功能正确性 | ✅ | 合同逐项成立（§1.1）；`cargo test -p zgraph-cli` **198 passed**、`cargo test --workspace` **790 passed/0 failed/3 ignored** 与作者报告一致；评审人独立真链路探针 **32/32 PASS**（§4.2），含 CLI 层未覆盖的 Paused run decide；真 kernel+真 SQLite 的 human 图等待→decide→自动接续/consumed/退出 flush 均由 `human_decide.rs` 真子进程用例与探针双重验证 |
| 代码结构合理性 | ✅ | `decide_cmd` 独立子命令模块；run consumer 拆为 `DecisionSession`（生命周期/线程）+ `decision_consumer_loop`（DB I/O）+ `submit_human_decisions`（主循环控制提交）+ bridge observer（确认回传），职责单一；复用 trace `DecisionStore/DecisionConsumer` 与既有 bridge observer 模式，无重复 SQL/重试实现；`storage_error_detail` 抽公共错误文案（`trace_cmd.rs:780`）供 trace/replay/run/decide 复用；仅新增 1 个常量、1 个 CLI 常量，无新依赖；`run_cmd` 改动集中且无 human 路径零副作用 |
| 逻辑性 | ✅ | 预检顺序与 DR C3 完全一致（决策行优先于 run/instance 状态）；consumer「一律提交 + submitted 去重 + 控制失败回滚」避免重复控制且不悬挂；确认通道 unbounded（观察者在事件循环线程发送不阻塞），终态 `finish()` drop stop → join → 退出前 drain+flush，保证已确认 `consumed_at` 提交；`mark_consumed` 条件更新幂等；`--db :memory:` 预检在 run_id/`start_run` 之前，不产生任何 run 痕迹；trace 决策排序确定；无 human 图行为不变；无死锁/吞异常/新 panic 点 |
| 可读性 | ✅（N4/N6/N8 备注） | 模块/函数注释均引用 DR-006 章节并解释「为什么」（预检顺序、C13、按需启动、退出 flush、失败不 fail run）；错误码/常量集中；测试名与断言消息可读且强（含 stdout/stderr/退出码/DB 行级）。N4/N6/N8 为文档口径与个别测试/细节建议 |

## 3. 发现清单

> 级别口径：P0 = 阻塞；P1 = 重要（必须修复或登记 ISSUE）；P2 = 一般；P3 = 建议（可后置，应进台账）。本分支 P0=0、P1=0、P2=0。

| # | 级别 | 位置 | 问题 | 建议 | 状态 |
|---|---|---|---|---|---|
| 1 | P3 建议 | `crates/zgraph-cli/tests/decide.rs`（现有 15 例） | 缺 **Paused run 上 decide 允许**的 CLI 用例（DR-006 §4.4 行 8 / §4.5 步骤②）。评审人独立探针已验证：Paused run + WaitingHuman instance → decide exit 0、决策行插入、run **保持 Paused**（不自动 resume），但无固化用例 | 补一个 Paused 场景用例（fixture 里 UPDATE runs.status='Paused' 后 decide，断言 exit0 + run 仍 Paused） | open（建议项） |
| 2 | P3 建议 | `crates/zgraph-cli/tests/human_decide.rs` | 缺 **human 图 + `--json`** 用例：stdout 必须只有结果 JSON、决策行只走 stderr；另 `decide --json` 的 **DB 错误形状**（`HUMAN_DB_ERROR`）无专项断言 | 各补 1 例（断言 stdout 单行 JSON / stderr 含决策行；DB 错误 JSON 形状） | open（建议项） |
| 3 | P3 建议 | `crates/zgraph-cli/src/run_cmd.rs:1249-1297` | 缺 human 纯等待 run + **SIGINT**（Cancelled + exit 130）CLI 层用例（DR-006 §7.3 #6）；既有 cancel 用例均无 human 节点，kernel 侧由 REV-080 覆盖 | e2e（REV-082）或 CLI 补 1 例；若不补，建议 W3a-docs 注明由 kernel/e2e 覆盖 | open（建议项） |
| 4 | P3 裁决 | `crates/zgraph-cli/src/run_cmd.rs:1472-1497,1499-1517` | **作者说明 (c)(d) 评估**：(c) `-q/--quiet` 实现为抑制 applied 与 applied=false 两类**决策行**、保留存储告警/错误 —— 与 DR §7.3 #5「抑制 stderr 决策行（不抑制错误）」一致，**可接受**；(d) human+`:memory:` 时既有 `warn_memory_db()` 先于预检错误输出 —— DR 只要求预检 exit3/不产生 run/reason，**可接受**，无 DR 冲突 | W3a-docs（REV-083）as-built 明写两处口径；无需登记 ISSUE | open（裁决完成） |
| 5 | P3 裁决 | `crates/zgraph-cli/src/run_cmd.rs:856,1380-1430` | **作者说明 (a)(b) 评估**：(a) consumer 职责拆分（专线程管 DB I/O、主循环提交 control、bridge observer 回传确认）—— DR 只冻结语义未冻结线程拓扑，#2/#4 均满足，**一致**；(b) 按需启动（仅 human 图）—— DR §7.3 #1 明确允许「始终启动或按需延迟启动」，**可接受** | docs as-built 记录拓扑与按需启动即可 | open（裁决完成） |
| 6 | P3 建议 | `crates/zgraph-cli/src/trace_cmd.rs:269-304` | `trace --json` 的 HumanDecision 行**不受 `--limit`/`--since-seq`/`--kinds` 过滤**（始终追加全量决策行）。DR §6.4 未规定过滤语义，当前行为与 `replay --mode=events` 自洽（同码），但用户可能误解 | W3a-docs as-built 明示「过滤只作用于事件行，决策行始终全量」；如产品需要再另立 | open（建议项） |
| 7 | P3 建议 | `crates/zgraph-cli/src/run_cmd.rs:1472-1497` | `flush_confirmed` 对 `mark_consumed` 返回 `Ok(false)`（行已被消费/不存在）也按成功打印 applied 行；正常单 consumer 流程不会发生（幂等重试边界） | 仅在 `Ok(true)` 时打印决策行；`Ok(false)` 视为幂等已消费（可静默） | open（建议项） |
| 8 | P3 建议 | `crates/zgraph-cli/src/trace_cmd.rs:345-388`；`crates/zgraph-cli/Cargo.toml:31` | HumanDecision NDJSON 的键序由 `serde_json::Map` 插入序决定，而 `preserve_order` 由 `genai` 传递开启（Cargo.toml 已注 ISS-073）。当前实际键序与 spec 示例一致（探针核实），但契约只冻结**形状**、未冻结键序；若依赖特性变化键序可能变（不影响 trace/replay 逐字节一致，因同一实现） | 注释说明键序非契约 / W3a-docs 注明；无需改代码 | open（建议项） |
| 9 | P3 观察 | `crates/zgraph-trace/tests/channels.rs:181`（**非本分支代码**） | 评审首轮 `cargo test --workspace` 出现一次 `zgraph-trace --test channels::flush_barrier_covers_streaming_events_beyond_one_batch` 失败：`common/mod.rs:48 flush trace writer: Storage("FOREIGN KEY constraint failed")`；随后 3 次 full workspace 均 790/0/3、该 target 连续 8/8 通过；`git diff main...HEAD -- crates/zgraph-trace crates/zgraph-kernel` 为空 → **非本分支引入**（历史 REV-041 记录过同类 FK/顺序缺陷，该用例是回归护栏） | 若 Lead 的 merge gate 复现，请重跑该次并登记 trace 模块 ISSUE（flush 顺序/FK 偶发）；不影响 REV-081 结论 | open（环境观察） |

## 4. 验证证据

### 4.1 门禁与测试（评审人亲自执行）

```bash
export CARGO_HOME=/Users/zyzs/lczyzs/code/zgraph/.cargo-home
cargo test -p zgraph-cli
# lib 93 + acceptance_m2 10 + decide 15 + human_decide 7 + replay 13
# + run 31 + trace 18 + validate 11 = 198 passed, 0 failed, 0 ignored
cargo test --workspace                  # 790 passed / 0 failed / 3 ignored（4 次中 3 次全绿；首次见 §3 N9 观察）
cargo clippy --workspace --all-targets --locked -- -D warnings   # clean（exit 0）
cargo fmt --all -- --check              # exit 0
bash scripts/dev/check.sh               # CHECK PASSED
/Users/zyzs/lczyzs/code/zgraph/.venv/bin/python -m pytest     # 170 passed
```

- 范围：`git diff --name-status 390dbe1..c711232` = 5 src + 3 tests，全部在 `crates/zgraph-cli/**`；`Cargo.lock`/`Cargo.toml` 无 diff；提交正文含 `Review: REV-081` / `Agent: cli-dev`；worktree 干净。
- `tests/decide.rs` 15 例覆盖：成功/JSON 形状/reason 原样/二次 decide 5/决策行优先于 run 状态/run 不存在 1/终态 1/instance 不存在与他 run 1/非 WaitingHuman 5/空库不创建 4/旧版本 4+迁移提示/新版本 4/用法 2/help 0/并发 4 进程恰一成功；`tests/human_decide.rs` 7 例覆盖真链路等待→decide→Completed+consumed+stderr 行、rejected 也 Completed、quiet、迟到决策 applied=false 仍 consumed、等待中并发 4 进程恰一成功、human+`:memory:` 拒绝、普通图 `:memory:` 不变。

### 4.2 独立真链路探针（评审人自建，不依赖作者测试；32/32 PASS）

方法：用本 worktree 新构建的 `target/debug/zgraph` + 真 kernel（builtin.human）+ 真 SQLite trace，`/tmp` 独立图/DB；直接读 DB/SQLite 断言。

```text
PASS human 实例到达 WaitingHuman（轮询可见，N11）且 run 进程保持存活
PASS decide exit 0；--json 形状恰 {ok,run_id,instance_id,decision,decision_id,decided_at}；
     decision_id 为 UUIDv7、decided_at 为 24 字符 UTC 毫秒 Z
PASS decide 后 run 自动接续 exit 0；stderr 有 "human decision applied: instance=" 实时行；
     决策行 consumed_at 非空（退出前 flush）；run Completed；review 实例 Completed
PASS 二次 decide exit 5 + --json.code=HUMAN_DECISION_EXISTS，仍恰 1 行
PASS replay --mode=events --json 与 trace --json stdout 逐字节一致（含 HumanDecision 行）
PASS HumanDecision 行在事件行之后、字段顺序 {kind,run_id,instance_id,decision,approver,
     reason,decision_id,decided_at,consumed_at}、consumed 行 consumed_at 非空
PASS 人类 trace 输出 "human_decisions: 1 (approved 1 / rejected 0; pending 0)"
PASS replay --mode=outputs 仍可运行（不读决策表；--record=none → 既有 exit3 语义）
PASS human + --db :memory: → exit 3、stderr 含 RUN_ENTRY_INVALID + trace_required_for_human、
     无 created/run_id 输出；普通图 :memory: → exit 0（行为不变）
PASS decide 对不存在的 DB → exit 4 且不创建文件
PASS decide 对 v4 DB → exit 4 + "zgraph run" 迁移提示、库仍 v4、未建 human_decisions
PASS（CLI 未固化场景）Paused run + WaitingHuman → decide exit 0、决策行插入、run 仍 Paused
```

### 4.3 作者说明 a–d 与 DR 一致性

| 说明 | 结论 |
|---|---|
| (a) consumer 职责拆分（专线程 DB / 主循环 control / bridge 确认） | 与 DR 不冲突：语义（一律提交、等确认、独立 RW 条件更新、不阻塞）全部满足；结构属实现自由，**可接受** |
| (b) 按需启动（仅 human 图） | DR §7.3 #1 明确允许；无 human 图不启动、行为不变（测试与探针均验证），**可接受** |
| (c) `--quiet` 抑制口径 | 抑制两类决策行、保留错误/存储告警，符合 §7.3 #5；建议 docs 明写，**可接受** |
| (d) `:memory:` warning 顺序 | 既有内存库 warning 先于预检错误；DR 只冻结 exit3/不产生 run/reason，测试已锁，**可接受** |

## 5. 遗留问题

- **P3 建议未登记 `doc/issues.md`**（本分支合同只允许 `crates/zgraph-cli/**`；台账由 docs/Lead 维护）：N1–N3、N6–N8 建议由 Lead 决定是否登记；N4/N5 为已裁决说明，建议在 W3a-docs（REV-083）DR-006 §14.3/§15 执行记录与 06 as-built 留痕。
- **环境观察 N9**：trace crate 测试一次性 FK flake（非本分支引入，已多轮复跑验证）；建议 Lead 在 merge gate 若命中时重跑并登记 ISSUE，不阻塞 REV-081。
- **跨分支待办**：e2e（REV-082）在此 CLI 基础上覆盖 AC-16 a–g 真链路（含 approve/rework/reject_end/timeout、冲突幂等、并发、`:memory:` 拒绝、golden 不变）；docs（REV-083）回写 decide/consumer/trace 决策行 as-built、`EXIT_CONFLICT`、`HUMAN_DECISION_POLL_MS`、按需启动/quiet/memory 口径与 §3 N4–N8 说明。
- 未发现需要返工的问题；无阻塞项。

## 6. 条件关闭记录（仅"有条件通过"填写）

不适用（结论为 `通过`，无附加条件）。
