# REV-030 · cli · M5 wave2 replay L1/L2 + llm_calls 接线 + 启动中止标定 + timeout_hint 转发

> **模块声明**：本分支改 `crates/zgraph-cli/**`（`cli` 模块：`src/{bridge,lib,replay_cmd,recovery,run_cmd,trace_cmd}.rs`、`tests/{replay,run,trace}.rs`、`Cargo.toml`），另改根 `Cargo.lock`（**`process` 模块**，仅 `zgraph-cli` 依赖列表 +2 行 `nix`/`uuid` 机械新增，**无新包、无版本变更**）。按 AGENTS §2 属**跨模块**（cli + process）；本记录由非作者评审人 `reviewer-b` 声明并作独立评审（REV-020/022/028 的 `Cargo.lock` 先例）。评审**未修改分支任何文件**，只新增本记录；未 merge / 未 push / 未改 main。
>
> **独立核对口径**：不采信作者自述。亲自读 3 个 commit 全量 diff（11 files +2172/−23）与 `replay_cmd`/`recovery` 全文、逐条对照 DR-002 §3.1/§3.2/§3.3 与 spec 06 §6.8.1/§6.8.2/§6.8.3/§6.10.5；在共享 target 跑 cli/e2e/workspace 全量并**另跑 main 基线**对比；另用真实 CLI 二进制做 5 组独立手工验证：① L2 replay（`ZGRAPH_PYTHON=/nonexistent`、DB SHA 不变、L1 输出 diff 一致、missing-row/tamper 退出码）；② `--record=full` 的 `llm_calls`（2 行/call_index/脱敏）与 hash 模式 0 行；③ **真 kill -9 E2E**（慢 python 节点运行中 kill -9 主进程 → 再跑一次 `run` 标定 + `trace` 截断提示）；④ timeout_hint 端到端（AgentConfig=1500ms + 4s fake LLM → 1.77s `NODE_TIMEOUT`，区分 180s 默认）；⑤ 函数长度/静态扫描。

- **分支**：`feat/cli-m5-replay`
- **作者**：`cli-dev`（提交 `Review: REV-030`/`Agent: cli-dev` 三处均命中）
- **评审人**：`reviewer-b`（**非作者**）
- **变更范围**：`83db2cac34e2b60e9bd9b92150ba7fa16ec33c49..af509a58c5738788d2f011ec623cd0112dd9fd17`（3 commits：`d3bd1b1` replay L1/L2 + llm_calls + timeout_hint 转发、`e48f3c0` timeout_hint 端到端/路由转发、`af509a5` 启动中止标定 + recovered[] + trace 进行中提示；`git diff --shortstat` = **11 files changed, 2172 insertions(+), 23 deletions(-)**；base = merge-base = `main` = `83db2ca`，ahead 3，**基于最新 main**，worktree clean）
- **评审时间**：2026-10-30
- **关联**：DR-002 §3.1（`recorded_outputs` 写者/键/hash）、§3.2（`llm_calls` seam/粒度/脱敏/status/仅 full）、§3.3（replay L1/L2 全流程与退出码）、§4（ISSUE-076 跨模块协同、ISSUE-084 wave2）、§6（wave2 分工）、§9 #4/#5/#10（M5 回写清单）；spec 06 §6.8.1（标定规则/判活顺序）、§6.8.2（只改 runs）、§6.8.3（trace 中断/进行中提示）、§6.10.3（run 启动顺序）、§6.10.5（退出码权威表）、§6.10.6（`--json`）；spec 00 §0.4 CLI 命名空间；architecture §5.2/§5.3/§8.2；ISSUE-048/076/082/084；REV-027（LlmCallRecorder）/REV-028（cli wave1）/REV-033（harness timeout_hint）/REV-034（trace 标定 API + RunRow 判活列）

## 1. 结论

**结论：通过**

本分支按 DR-002 wave2 口径交付 CLI 三项能力，实现质量高、链路真实、与既有契约一致；未发现阻塞/重要缺陷：

- **`zgraph replay` L1/L2（DR-002 §3.3）**：
  - **L1 events**：`run_events` 直接委托 `trace_cmd::run`（同一实现/默认选项），独立实测 human stdout 与 `--json` NDJSON 均与 `zgraph trace` **逐字节一致**，只读、不写库、不重跑；`Failed/interrupted` 沿用 trace 退出码语义。
  - **L2 outputs**：`TraceReader::open_readonly` + `build_plan` 预检（`record_mode=full` → `status=Completed` → `recorded_outputs` 完整性 → hash 复核）→ `RecordedExecutor` 以 `(node_id, attempt)` 返回记录输出重跑内核（`MemoryTraceSink`）→ 比较 status/error/输出。独立实测：`--record=full` run 的 replay 在 `ZGRAPH_PYTHON=/nonexistent` 下 **exit 0 / matched=true**，DB **SHA256 前后不变**（不写新 run/不改原 trace）；缺记录 → exit 3 `REPLAY_RECORD_MISSING`；输出篡改 → exit 1 `REPLAY_OUTPUT_MISMATCH`；run 不存在/库缺失 → exit 4；用法 → exit 2。代码路径无 worker/LLM 构造（仅内核 + RecordedExecutor）。
  - **退出码/命名空间码**：0/1/2/3/4 与 `REPLAY_RECORD_MISSING`/`REPLAY_RUN_NOT_COMPLETED`/`REPLAY_RECORD_CORRUPT`/`REPLAY_OUTPUT_MISMATCH` 均实现且不写 `runs.error_code`；`REPLAY_*` 未进 00/06 的正式表由 DR-002 §9 #4 docs 回写承接（见 §5）。
- **`llm_calls` 接线（ISSUE-082 部分）**：仅 `--record=full` 挂接 `LlmCallTraceRecorder`（`LlmCallRecorder` → `TraceInput::LlmCall`），run_id 由 CLI **预生成**后传入内核（`start_traced_run_with_writer_and_run_id`），与 `RunStarted`/`recorded_outputs` 同 id；独立实测 AC-08 两轮 → `llm_calls` **2 行、call_index 0/1、status=ok、recorded_at RFC3339 ms Z、request_json 无 api_key/SecretString**；hash 模式 → **0 行**。写入为内存 `send`（不阻塞、无 DB IO），观察者失败不改变执行（harness REV-027 已测）。
- **启动中止标定（ISSUE-084 / 06 §6.8.1）**：`run` 在 open/migrate 后、编译/启动前调用 `recovery::recover_interrupted`；判活顺序按 spec（同机 pid `kill(pid,0)` 存活→跳过；pid 缺失/已死→判死；跨机 heartbeat 距今 >60s→判死；DB 未记录 host 按同机），`mark_interrupted` 只改 Running/Paused 的 `status=Failed/interrupted=1/recovered_at`（**error_code 保持 NULL**、不动 instances、不造 RunFinished）；stderr `warning: run <id> marked interrupted (was Running, pid N gone)`，`--json.recovered[]` = `{run_id, previous_status, recovered_at}`。**独立 kill -9 E2E**（慢 python 节点运行中 kill -9 CLI → 再跑一次）实测：第二次 run 标定成功、warning/`recovered[]` 正确、`trace` 被 kill 的 run **exit 1 + `!! trace truncated: run interrupted at …`**、DB 行 `Failed/interrupted=1/error_code=NULL`。`trace` 的 `run still in progress (pid N)` 走 **stderr**（stdout 保持数据/NDJSON 纯）、exit 0。
- **timeout_hint 转发（ISSUE-076 端到端）**：`RoutingExecutor`（仅 Agent handler 转 Harness、其余 None）与 `RecordingExecutor`（透明转发）均 override，kernel 调用链不再截断；独立实测 DSL 无 `timeout_ms` + AgentConfig=1500ms + 4s fake LLM → CLI run **1.77s 内 `Failed/NODE_TIMEOUT`**、实例 `TimedOut`、`NodeTimedOut` 事件 1 条（若走 180s 默认则 ~4s 后 Completed）。
- **质量门禁**：`cargo test -p zgraph-cli` **115 passed / 0 failed**（main 87 + 28）；`cargo test -p zgraph-e2e` **24 passed / 1 ignored**（`harness_ac08` 4/1 不回归）；`cargo test --workspace` **528 passed / 0 failed / 2 ignored / 56 个测试二进制**（main 基线 500/2/55，**+28 无缩水**）；`cargo clippy --workspace --all-targets -- -D warnings` exit 0；`fmt --check` exit 0；`check.sh` PASS；worktree clean、HEAD tree 无禁入文件；三 commit trailers 合规。

未发现阻塞/重要项；9 条建议/Nit + 4 条备注见 §3（kill -9 自动化回归缺失、跨机「跳过并告警」未实现、host 判活的环境变量兜底风险、`pid_alive` EPERM 语义、trace 提示未校验 host、CLI timeout e2e 落在 `<1000ms` 预检边界、replay/llm_calls 测试缺口、函数长度、比较口径等），均**不阻塞合并**，其中 #1/#2/#5 建议由 ISSUE-084/docs 承接。

## 2. 四维评审

| 维度 | 结论 | 依据（文件/行/测试/命令） |
|---|---|---|
| 功能正确性 | ✅ | ① **replay**：`replay_cmd.rs` 全流程与 DR §3.3 对应（`build_plan` 预检/完整性/hash、`RecordedExecutor` 按 `(node_id,attempt)`、`load_recorded_graph` 用 `graphs.dsl_json`、`build_spec` 重建 RunSpec、`finish_outputs` 比较 status/error/output）；独立 L1 diff/L2 replay/DB SHA/exit 码实测见 §4.3。② **llm_calls**：`LlmCallTraceRecorder` 脱敏快照转 `TraceInput::LlmCall`（`run_cmd.rs:490-515`）、仅 full 挂接（`:855-867`）、run_id 预生成（`:767-800`）；独立 DB 实测 2 行/`call_index`/sanitized、hash 0 行。③ **标定**：`recovery.rs` 判活顺序/只改 runs/文案/recovered 形状与 06 §6.8.1 逐步一致；独立 kill -9 E2E 与 trace 截断输出实测。④ **trace 提示**：`print_in_progress_note` 只写 stderr、exit 0（`trace_cmd.rs:202-225`；集成测试断言 NDJSON 纯）。⑤ **timeout_hint**：路由/记录包装转发（`run_cmd.rs:427-438,481-486`），独立 1.77s vs 180s E2E。 |
| 代码结构合理性 | ✅ | 新增职责单一模块：`replay_cmd`（参数/预检/重跑/比较/输出）、`recovery`（判活/标定调用/警告文本）、`run_cmd`（启动接线/记录器/转发）、`trace_cmd`（提示）；L1 replay **不复制** trace 格式化而是复用 `trace_cmd::run`；`bridge` 只增 `start_traced_run_with_writer_and_run_id` 并把公共实现收进 `start_traced_run_inner`（`start_traced_run_with_writer` 行为不变）；`status`/`record_mode`/`owner_pid` 等均经 trace 既有公开面（REV-034），无跨 crate 反向依赖；无新代码进入 harness/worker/kernel/trace。 |
| 逻辑性 | ✅ | 只读约束贯穿（reader readonly + MemoryTraceSink，独立 DB SHA 验证）；预检顺序/退出码映射自洽（record 类 3、mismatch 1、storage 4、usage 2）；calibration 在 open/migrate 后、图/输入/启动前（06 §6.10.3），失败 exit 4 并关闭写端；标定与 RunStarted 并发安全（先标定后建 run；同机活 pid 跳过）；llm_calls 与 RunStarted 同 FIFO/同 run_id；timeout_hint 包装层透明转发、非 agent 不泄漏；无 TODO/伪测/被注释断言。扣分项仅为 §3 #3/#4/#9 的边界/口径 Nit。 |
| 可读性 | ✅ | 三个新/改模块均有「为什么」docstring（H3、只读、run_id 预生成原因、判活顺序、残余 pid 复用风险、known boundary `global_vars`/`llm_calls` 不参与重放）；错误带 `error[NAME]:`；测试分段清晰、命名可读；`check.sh`/fmt/clippy 0 告警。扣分项：新函数 `build_plan` 75 行、`run_in_runtime` 70→91 行、新增测试 88/78 行超 60（§3 #6）。 |

## 3. 发现清单

| # | 级别 | 位置 | 问题 | 建议 | 状态 |
|---|---|---|---|---|---|
| 1 | 建议（ISSUE-084 验收项） | `crates/zgraph-cli/tests/run.rs:1185`（`run_startup_calibrates_interrupted_runs_and_reports_recovered`） | 标定测试用 `TraceWriter` **预置 Running 行**模拟崩溃后 DB，覆盖判活/标定/recovered/warning；但 ISSUE-084 明确要求「补 **kill -9 故障注入**测试（07 §7.3 检查单 #6）」，分支没有真实 `kill -9` 子进程用例。本评审已手工完成 kill -9 E2E 并全部通过（§4.3），但该证据不会随 CI 回归 | 合并前/ISSUE-084 关闭前补一条 Unix 集成测试：后台 spawn `zgraph run`（慢 python 节点）→ `kill -9` 主进程 → 再跑 `run --json` 断言 `recovered[]`/warning 与 DB 行；至少把本评审 §4.3 的人工证据写进 07 §7.3/REV 记录。**不阻塞**（行为已实现且经人工验证）。 | open（ISSUE-084/docs 承接） |
| 2 | 建议（spec 字面） | `crates/zgraph-cli/src/recovery.rs:79-112`；`run_cmd.rs:1053`（`warnings: []`） | 06 §6.8.1 步骤 2② 规定跨机但 heartbeat **未**陈旧时「跳过并告警」，实现 `is_dead` 返回 false 后**静默跳过**，不产生 warning；`--json.warnings` 恒 `[]`（仅标定的 run 有 stderr warning）。未陈旧跨机候选无法在输出中看到「已检查但跳过」 | 为「跳过」候选产生 stderr 警告并放入 `warnings[]`（如 `warning: run <id> skipped recovery (owner on <host>, heartbeat fresh)`），或 docs 明确「M5 仅对已标定 run 告警」。**不阻塞**。 | open（建议） |
| 3 | 建议（健壮性，多机前提） | `crates/zgraph-cli/src/recovery.rs:35-42`（`current_host`）、`:90-104`（`is_dead`） | `current_host()` 仅取 `HOSTNAME`/`COMPUTERNAME` 环境变量，兜底 `"unknown"`；若两台机器的进程环境都缺该变量（macOS 非交互 shell 即常见，本机实测为 `unknown`），**不同机器都会被当成同机**，从而走 `pid_alive` 本地判断：远端 run 的 pid 在本地不存在 → 被**误标为 interrupted**（共享 DB/网络文件系统部署时可能误杀活 run）。kernel `host_name()` 同源（pre-existing），但标定使其后果变重 | `current_host` 优先用 OS 主机名（如 `nix::unistd::gethostname`）或构造时注入；并在 `owner_host == "unknown"` 时按「未知」走 heartbeat 陈旧路径而非直接 pid 判死；补一条 owner_host="unknown" 用例。若判定 M5 部署仅单机，请 docs 明确前提。**不阻塞**（本地单机场景无影响）。 | open（建议/docs） |
| 4 | 建议（健壮性） | `crates/zgraph-cli/src/recovery.rs:45-55`（`pid_alive`） | `kill(pid, 0).is_ok()` 在 **EPERM**（进程存在但无权限发信号，如共享机器/不同用户）时返回 false → 被当作「已死」，可能把活 run 标为 interrupted；POSIX 语义应「EPERM = 存在」 | 判活改为：`Ok(()) => true`、`Err(EPERM) => true`、`Err(ESRCH) => false`（其他保守 true 或 false + 注释），并补注释/测试（EPERM 可用 root 场景难测，注释说明）。**不阻塞**（单用户本地无影响）。 | open（建议） |
| 5 | 建议（trace 跨机提示） | `crates/zgraph-cli/src/trace_cmd.rs:216-225`（`print_in_progress_note`） | 只判 `status ∈ {Running,Paused} && owner_pid.is_some() && pid_alive(pid)`，**不校验 `owner_host`**：跨机 run 的 pid 与本机同号进程会误报「run still in progress (pid N)」；非 Unix `pid_alive` 恒 true 会对任何带 pid 的 Running run 打印该提示（即使已死） | 同机（或 host 未知）才做 pid 提示；跨机改为「owner host 不能判活」的说明或静默；非 Unix 不打印 pid 存活提示。**不阻塞**（提示只写 stderr，不影响数据/退出码）。 | open（建议） |
| 6 | 建议（e2e 区分度/边界） | `crates/zgraph-cli/tests/run.rs:1066`（`run_agent_config_timeout_hint_is_forwarded_end_to_end`） | 该用例 AgentConfig=**1000ms**（V13 下界），kernel 派发后 harness 入口 `remaining≈999ms` → 命中 `preflight_call_timeout` 的「剩余 <1000ms 不发新调用」分支，**并未真正走 1s deadline 到期**（用例实测约 1.40s，其中含 worker discover 等启动开销）；4s 慢 fake LLM 未被调用。区分度仍成立（若 hint 未转发，180s 默认 + 4s fake → Completed，断言会失败），但对「hint 值 = 真实超时」覆盖弱 | 再加一条 hint≥1500ms（>1000ms 预检阈值）+ 慢 fake（如 `chunk_delay_ms` 使总时长 >4s）的用例，断言 `ExitCode=1`/`NODE_TIMEOUT`/实例 `TimedOut`/`elapsed < 5s`；本评审已用 1500ms 手工验证 1.77s 生效（§4.3），可作模板。**不阻塞**。 | open（建议） |
| 7 | 建议（replay 测试覆盖） | `crates/zgraph-cli/tests/replay.rs` | 已覆盖 L1 一致、L2 成功/无 full/非 Completed/hash 篡改/输出篡改/exit4/usage；**未覆盖**：`--record=full` 但**删除某条 recorded_outputs 行**（完整性预检 MISSING，本评审手工验证 exit3 通过）、重复记录/CORRUPT、graph 缺失、entry_inputs 快照缺失/非法、`--mode=events` 默认（不写 `--mode`）、events 模式下 Failed run exit1、自环 `attempt>0` 重放 | 至少补「缺记录行 → exit3 REPLAY_RECORD_MISSING」与「默认 mode=events」两条；其余可随 M6/加固。**不阻塞**。 | open（建议） |
| 8 | 建议（llm_calls 测试覆盖） | `crates/zgraph-cli/tests/run.rs:808-832`；`run_cmd.rs:1467-1500` | 集成只断言 full 模式 2 行；**未**集成断言 `--record=hash/none` 时 `llm_calls` 行数=0（仅单测断言 recorder 未挂接）、未覆盖终态失败 `status=error` 行、未断言 `llm_calls.instance_id` 与 agent 实例行一致 | 补一条 hash 模式 DB 断言 `llm_calls count=0`；补 error 用例（可用 harness fake failure 脚本）断言 status/response.error；断言 instance_id 映射。**不阻塞**。 | open（建议） |
| 9 | Nit（比较口径） | `crates/zgraph-cli/src/replay_cmd.rs:389-398`（`matched`）、`:331-420`（`build_plan` 顺序） | ① `matched` 只要求重跑 Completed/error None/output 相等；**未把原始 `runs.error_code == None` 纳入**（DR §3.3 步 4 明确比较 status/error_code/output；当前 Completed 行理论 error_code 应为 NULL，属防御性缺失，仅影响人为篡改 DB 场景）。② `build_plan` 先做 hash 复核、后做完整性遍历，与 DR §3.3 步骤 2 的列序（完整性预检→hash 校验）相反；两者冲突时返回 CORRUPT 而非 MISSING（都 exit3，无功能影响） | `matched` 加 `run.error_code.is_none()`；或注释说明 Completed 语义已隐含。build_plan 顺序可调整或注释「两类记录错误优先级」。**不阻塞**。 | open（Nit） |
| 10 | 建议（函数长度/ISSUE-081） | `replay_cmd.rs:331`（`build_plan` 75 行）、`run_cmd.rs:722`（`run_in_runtime` 91 行，main 为 70）、`tests/run.rs:1185`（88）、`:1066`（78）；`run_cmd.rs:179`（`parse` 79，pre-existing） | architecture §2.2 等价约束「函数 ≤60 行」：本分支新增 `build_plan` 75 行超限，`run_in_runtime` 由 70→91 进一步变差，新增测试 2 个 >60；均未在 ISSUE-081 扩展清单登记（REV-028 #3 已登记 CLI 2 处，但未含本分支新增） | 拆分 `build_plan`（预检/完整性/记录映射三 helper）与 `run_in_runtime`（启动阶段 helper）；把新增 4 处并入 ISSUE-081 清单。**不阻塞**（CI 不强制）。 | open（ISSUE-081 承接） |
| 11 | 备注（已知边界，需 docs 回写） | `replay_cmd.rs:20-24`（module doc `global_vars`/`llm_calls` 不参与重放）；`outputs_payload`（L2 `--json` 自定形状） | ① `runs` 表未存 `global_vars`，L2 以空 map 重建——依赖 `global_vars` 的图可能 `REPLAY_OUTPUT_MISMATCH`（已在代码 docstring 如实标注）；② `replay --mode=outputs --json` 的对象字段（`ok/command/mode/run_id/record_mode/recorded_outputs/status/matched/original_output/replayed_output/error`）与 human 输出（仅打印重放 output + stderr 摘要）**不在 06 §6.10.6 现有规范内**；③ L3（外部副作用）不承诺 | M5 docs 回写（DR-002 §9 #4/#5）：06/00/architecture 登记 replay 命令、退出码小表、`REPLAY_*` 码与 `--json` 形状；06 增注 `global_vars` 未记录的 L2 边界；07/08 注记 L3。**不阻塞**，遗留 docs 承接。 | open（docs REV-035/Lead） |
| 12 | 备注（依赖/跨模块） | 根 `Cargo.lock`（+2 行）；`crates/zgraph-cli/Cargo.toml`（`uuid.workspace`、`[target.'cfg(unix)'.dependencies] nix = { version="0.29", features=["signal"] }`） | `Cargo.lock` 属 `process` 模块；本分支为 cli 新增两个**已存在于 lock 的包**（`nix`/`uuid`）依赖行，**无新包/无版本变更**（diff 仅 2 行）；`nix` 直接声明与 worker 同版本，符合 ISSUE-068 现状（未提升 workspace deps）；`uuid` 可直接复用 kernel 的 uuid 依赖，但 CLI 需直接生成 run id，可接受 | 按 AGENTS §2 由本记录声明 cli+process 跨模块；merge 时 Lead 确认；ISSUE-068 继续跟踪 nix 依赖治理。**无需改动**。 | 无需处理（已声明） |
| 13 | 备注（ISSUE 状态） | `doc/issues.md` | 本分支合并后可关闭/更新：**ISSUE-048**（trace 进行中提示已实现）、**ISSUE-076**（CLI 侧 timeout_hint 转发 + 端到端已有，harness REV-033 已合）、**ISSUE-084**（标定/recovered/trace 截断已实现；仍缺 kill -9 自动化测试→建议补测后再 close）；**ISSUE-082 仍只能部分修**：`llm_calls` 已接线，但 `runs.worker_versions_json`/`model_params_json` 仍未实现（DR-002 §1 row3 要求） | Lead/docs 在合并后按事实更新台账（勿提前 fixed）；ISSUE-082 注记「llm_calls 已接线；版本/模型参数列待 M6」。**不阻塞**。 | open（docs/Lead） |
| 14 | 备注（环境观察） | 本评审手工 kill -9 实验 | 首次在 run 创建后 **<~100ms** kill -9，下次打开 DB 时该 run 行**完全不存在**（trace 写线程 100ms 批提交窗口内未落盘；spec §6.8.5 已声明掉电/崩溃可能丢最后若干事务，属既有 trace 设计，非本分支缺陷）；**等待节点真正开始执行（>100ms）后** kill -9，则 RunStarted/owner_pid 已落盘，标定链路完整通过（§4.3） | 07 §7.3 #6 手工验收时应「等 run 进入执行后」再 kill -9；可在 docs 注明该窗口。**不阻塞**。 | 无需处理（备注） |

> 无阻塞项、无重要项。建议 #1/#2/#3/#4/#5 为崩溃恢复/判活边界加固（#3/#4 仅在多机/多用户部署触发），#6/#7/#8 为测试覆盖加固，#9/#10 为 Nit，#11/#13 为 docs/台账承接，#12/#14 为声明/观察。**本评审认为本分支无需为合并新开 P1/P2 ISSUE**；#3/#4 建议 Lead/docs 评估是否登记（M6 多机/多用户场景）。

## 4. 验证证据

### 4.1 范围、分支、提交元数据

```bash
cd /Users/zyzs/lczyzs/code/zgraph/.worktrees/cli-cli-dev
git branch --show-current        # feat/cli-m5-replay
git rev-parse HEAD               # af509a58c5738788d2f011ec623cd0112dd9fd17
git merge-base main HEAD         # 83db2cac34e2b60e9bd9b92150ba7fa16ec33c49（= main）
git rev-list --left-right --count main...HEAD   # 0	3
git diff --shortstat main...HEAD               # 11 files changed, 2172 insertions(+), 23 deletions(-)
git diff --name-status main...HEAD
# M Cargo.lock；M crates/zgraph-cli/{Cargo.toml,src/{bridge,lib,run_cmd,trace_cmd}.rs,tests/{run,trace}.rs}
# A crates/zgraph-cli/{src/{recovery,replay_cmd}.rs,tests/replay.rs}
git log --format='%h %an <%ae> / %cn <%ce> / %s' main..HEAD   # 3 commit 均 chenyuxiangg，正文含 Review: REV-030 / Agent: cli-dev
git diff main...HEAD -- Cargo.lock                # 仅 zgraph-cli dependencies +nix +uuid（2 行；无新包/版本变更）
git status --short                               # 空（clean）
git ls-tree -r --name-only HEAD | grep -E "__pycache__|\.pyc$|\.db$|\.sqlite" || echo none   # none
git diff --check                                 # exit 0
```

### 4.2 测试与门禁（共享 target `/Users/zyzs/lczyzs/code/zgraph/target`；`CARGO_HOME=<repo>/.cargo-home`、`CARGO_NET_OFFLINE=true`、`CARGO_INCREMENTAL=0`、`ZGRAPH_PYTHON=<repo>/.venv/bin/python`）

| 命令 | 结果 |
|---|---|
| `cargo test -p zgraph-cli` | **exit 0：115 passed / 0 failed / 0 ignored**（main 基线 87，**+28**：unit 54、acceptance 10、replay 8、run 24、trace 11、validate 8；日志 `/tmp/rev030-cli.log`） |
| `cargo test -p zgraph-e2e`（workspace 内含） | **24 passed / 0 failed / 1 ignored**（`harness_ac08` 4/1 不回归；ac12 1、cancel 2、concurrency 4、fault_injection 7、parallel 4、timeout_kill 2） |
| `cargo test --workspace`（本分支） | **exit 0：528 passed / 0 failed / 2 ignored / 56 个测试二进制**（日志 `/tmp/rev030-ws.log`） |
| `cargo test --workspace`（**main 基线**，root worktree @`83db2ca`） | **exit 0：500 passed / 0 failed / 2 ignored / 55 个测试二进制**（日志 `/tmp/rev030-main-ws.log`）——**分支 = main +28（cli 115 vs 87），无缩水** |
| `cargo test --workspace`（**全新隔离 target** `/tmp/rev030-target`，复跑验证无共享 target 串写） | **exit 0：528 passed / 0 failed / 2 ignored / 56 个测试二进制**（与共享 target 结果一致，97s；跑完已删除该 target） |
| `/Users/zyzs/lczyzs/code/zgraph/.venv/bin/python -m pytest --tb=no`（worktree 内；merge.sh 同款 Python 门禁） | **exit 0：84 passed**（与 main 基线一致，无新增失败） |
| `cargo clippy --workspace --all-targets -- -D warnings` | **exit 0 / 0 warning**（`/tmp/rev030-clippy.log`） |
| `cargo fmt --all -- --check` | **exit 0** |
| `bash scripts/dev/check.sh` | **CHECK PASSED**（身份/remote/CLAUDE.md/分支名/Python 语法/progress/issues/fmt/`git diff --check`） |
| `git status --short`（全部命令后） | 空 |

> **merge.sh dry-run（本评审尝试）**：`CARGO_HOME=… CARGO_NET_OFFLINE=true CARGO_TARGET_DIR=/tmp/rev030-target PYTHON_BIN=<venv>/python bash scripts/dev/merge.sh feat/cli-m5-replay --dry-run` —— 评审记录门禁通过（`INFO: 评审记录: …REV-030-cli-m5-replay.md -> 通过`），随后 **`MERGE BLOCKED: git fetch origin main 失败（检查 SSH 连通性）`**（本评审环境无外网/SSH，属环境阻塞；分支基于本地 main `83db2ca`，`origin/main`（`1cc9123`）是其祖先，fetch 成功后 ff-only 条件成立）。未执行 cargo/pytest 门禁（因脚本在 fetch 步退出）；上述 workspace/pytest 结果为等价手工复跑。建议 Lead 在可 fetch 环境复跑 dry-run。

### 4.3 独立手工验证（真实 CLI 二进制；均在 `/tmp` 独立 DB，不触碰仓库）

| # | 验证 | 命令/方法要点 | 结果 |
|---|---|---|---|
| 1 | **L2 replay 成功 + 不启 worker + 只读** | AC-08 图 `--record=full` 跑 1 次；`ZGRAPH_PYTHON=/nonexistent/python zgraph replay --mode=outputs --json`；前后 `shasum -a 256` DB | exit 0；`ok/matched=true`、`status=Completed`、`recorded_outputs=1`、`replayed_output==original_output`；**DB SHA 前后一致**（不写新 run/不改原 trace）；`runs=1` 不变 |
| 2 | **L1 与 trace 逐字节一致** | 同 run：`zgraph trace` vs `zgraph replay --mode=events`（human 与 `--json` 各一次） | `diff` 均为空（human stdout 与 NDJSON 均一致）；exit 0 |
| 3 | **replay 记录类错误/不一致/缺失退出码** | ① 删除 `recorded_outputs` 全部行 → replay；② 篡改 `runs.output_json` → replay --json；③ 不存在 run / 不存在 DB；④ 非法 `--mode` / 缺 `--run` | ① exit **3** `REPLAY_RECORD_MISSING`（失败后 DB SHA 不变）；② exit **1** `REPLAY_OUTPUT_MISMATCH`（`ok=false, matched=false`）；③ exit **4** `STORAGE_ERROR`；④ exit **2** |
| 4 | **`llm_calls`（full）与脱敏** | AC-08 `--record=full` 后查 DB：`llm_calls` 行/call_index/status/recorded_at/request | 2 行、call_index 0/1、status=ok、recorded_at `…Z`（RFC3339 ms）；`request_json` 不含 `api_key`/`SecretString`，为合法 JSON（model/messages 结构在） |
| 5 | **`llm_calls` 仅 full** | 默认 `--record=hash` 跑同图后查 `llm_calls`/`recorded_outputs` | `llm_calls=0`、`recorded_outputs=1`（hash 记录），符合 DR §3.2 |
| 6 | **kill -9 启动标定 E2E** | 慢 python 节点（sleep 30 + 写 worker pid）运行中 `kill -9` CLI 主进程（等节点启动后 >100ms）；再次 `zgraph run` 同 DB；`zgraph trace <killed>`；直查 DB | 第二次 run exit 0，stderr `warning: run <id> marked interrupted (was Running, pid <cli_pid> gone)`，`recovered[]=[{run_id,previous_status:"Running",recovered_at}]`；`trace` exit **1** 且 `!! trace truncated: run interrupted at … (no RunFinished)`；DB `status=Failed, interrupted=1, error_code=NULL, recovered_at=…`；**SQLite 正常打开**（07 §7.3 #6 口径） |
| 7 | **timeout_hint 端到端（真实超时路径）** | 复制 AC-08 图并删 agent 节点 DSL `timeout_ms`；AgentConfig.node_timeout_ms=**1500ms**；fake LLM `chunk_chars=1, chunk_delay_ms=200`（总 ~4s） | CLI run **exit 1、elapsed≈1.77s**（vs 180s 默认）、`error.code=NODE_TIMEOUT`、实例 `TimedOut`、`NodeTimedOut` 事件 1 条——证明 hint 经 Recording→Routing 到 kernel deadline 生效 |
| 8 | **分支自带 timeout e2e 时长** | `cargo test -p zgraph-cli --test run run_agent_config_timeout_hint_is_forwarded_end_to_end` | 1 passed（1.40s）；该用例 AgentConfig=1000ms 落在 `<1000ms` 预检边界（见 §3 #6），区分度成立但非真实 deadline 到期 |
| 9 | **trace 进行中提示/截断** | 分支集成测试 + 上述 kill -9 后 trace | `trace` 对活 owner pid：stderr `run still in progress (pid N)`、stdout 纯数据、exit 0；对 interrupted：末尾 `!! trace truncated …`、exit 1 |

### 4.4 静态/边界核对

| 项 | 结果 |
|---|---|
| 只读性 | `replay_cmd` 只用 `TraceReader::open_readonly` + `MemoryTraceSink`；无 `TraceWriter`/`TraceInput::*` 写路径（L2）；L1 复用 `trace_cmd`（既有只读） |
| 不启动 worker/LLM | `RecordedExecutor` 仅 HashMap 查找；replay 代码无 `WorkerExecutor`/`HarnessExecutor`/`LlmProvider` 构造；独立 `ZGRAPH_PYTHON=/nonexistent` 仍成功 |
| `llm_calls` 脱敏 | 快照由 harness REV-027 构造（无 api_key）；CLI 只转存；独立 DB 断言无 `api_key`/`SecretString` 子串 |
| 启动顺序 | open/migrate → `recover_interrupted` → 图编译/输入校验/provider/executor → run_id+recorder → start；与 06 §6.10.3 一致（calibration 在图编译前） |
| 标定 SQL 语义 | 由 trace REV-034 提供（单事务、只 Running/Paused、error_code 不动）；CLI 判活顺序与 06 §6.8.1 一致（同机 pid / 跨机 heartbeat 60s / 失败 None） |
| 非 Unix | `pid_alive` 保守返回 true（不标定）；`nix` 仅 `cfg(unix)` 依赖；`cargo fmt/clippy` unix 通过；跨平台未编译验证（见 §3 #5 提示） |
| 新生产代码 panic | `replay_cmd`/`recovery` 非测试段无 `unwrap/expect/panic!`（awk 扫描 0 命中）；`output_hash` 复用 REV-028 实现 |
| 函数长度 | 新函数 `build_plan` 75、新增测试 88/78、`run_in_runtime` 91（main 70）>60；其余新函数 ≤60（见 §3 #10） |
| 依赖/lock | `Cargo.lock` +2 行仅 cli deps（nix/uuid 已存在）；`Cargo.toml` 新增 `uuid.workspace` 与 unix `nix`；无新包/版本变化 |
| 提交/门禁 | 3 commit 均含 `Review: REV-030`/`Agent: cli-dev`；worktree clean；HEAD tree 无禁入文件 |

## 5. 遗留问题

- **docs 回写（DR-002 §9 #4/#5/#10）**：00 §0.4 登记 `REPLAY_OUTPUT_MISMATCH`/`REPLAY_RUN_NOT_COMPLETED`/`REPLAY_RECORD_CORRUPT`（目前仅 `REPLAY_RECORD_MISSING` 在 00/06/architecture）；06 §6.10.5 补 replay 退出码小表、`replay` 用法、§6.10.6 补 L2 `--json` 形状与 human 输出、§6.8.1/§6.8.3 注记 M5 实现口径（owner pid 判活/事务化标定/recovered[]/进行中提示）；06 增注 `global_vars` 未记录的 L2 边界与 L3 不承诺；architecture §5.3 同步。**属 docs 分支（REV-035/Lead）承接，本分支不阻塞**。
- **ISSUE 台账（合并后按事实更新）**：ISSUE-048（trace 提示已实现，可 close）、ISSUE-076（CLI timeout_hint 转发 + 端到端已实现，可 close）、ISSUE-084（标定已实现；建议补 **kill -9 自动化测试**后再 close；本评审人工证据见 §4.3）、ISSUE-082（`llm_calls` 已接线，但 `worker_versions_json`/`model_params_json` 仍未实现 → 保持**部分修/待 M6**）。
- **建议项（§3 #1–#10）**：kill -9 自动化回归、跨机「跳过告警」、host 判活兜底与 EPERM 语义、trace 提示 host 检查、CLI timeout e2e 用 hint≥1500ms、replay/llm_calls 测试补强、`matched`/预检顺序 Nit、函数长度并入 ISSUE-081。均不阻塞本分支。
- **多机/多用户部署前提（§3 #3/#4）**：`owner_host` 依赖环境变量、`pid_alive` 对 EPERM 判死——若后续支持共享 DB 的多机/多用户场景，建议 Lead 评估登记 ISSUE（M6），当前单机部署无影响。
- **环境观察（§3 #14）**：run 创建后 <~100ms 内 kill -9 可能丢 RunStarted（trace 100ms 批提交；spec §6.8.5 已接受最后事务丢失），07 §7.3 #6 手工验收应等进入执行后再 kill。
- **跨模块**：根 `Cargo.lock` +2 行为 cli 依赖机械副作用（process 模块），本记录已按 AGENTS §2 声明并由非作者评审。
- 本评审未修改任何产品代码/测试/台账；本记录为本分支唯一新增文件（评审对象 `af509a5` + 本记录提交）。

## 6. 条件关闭记录

不适用（结论为「通过」；§3 均为不阻塞建议/备注）。
