# 规格 v1.2 · 来源与完整性（PROVENANCE）

> 本文件是 `doc/spec-v1.2/` 的唯一溯源说明。**仓库内文件为唯一可核验版本**。

## 1. 源稿

| 项 | 值 |
|---|---|
| 源稿 | 外部《图模型驱动工作流框架 · 详细设计 v1.1》（HTML，`详细设计-v1.0.html`，80,551 B ≈ 79 KB；标题 v1.1、文件名 v1.0） |
| 源稿位置 | 外部下载目录，**未入库**（不提供副本） |
| 提取文本 | 外部 `/tmp` 提取版（1,451 行），**未入库** |
| 审计 | 76 条缺陷（26 矛盾 / 12 不可编译 / 20 缺失 / 18 语义边界），入库于 [`../analysis/spec-audit-v1.1.md`](../analysis/spec-audit-v1.1.md) |

## 2. 冻结过程与冻结后修订

1. **冻结**：以源稿为意图、审计 76 条为问题清单，收敛为 9 分册（00–08）。
2. **冻结期回填**（入库前）：9 条高优先级跨册冲突 + 4 条新缺口 + 27 条中低优先级，逐条记录在 [`08-审计关闭矩阵.md`](08-审计关闭矩阵.md) §8.6。
3. **v2 落地修订**（DR-001 之后，2026-10-29）：
   - 00 与 02–07 分册顶部新增 **v2 偏差 banner**（01 无执行机制相关内容）（指向 `doc/architecture-v2.md` §8 的 D1–D12 与 §8.2 新增错误码表）；
   - `02-DSL与校验.md` 的范围链接改为仓库内 `doc/analysis/decision-record-01-scope.md`；
   - `07-验收用例.md` 的 AC-05（崩溃隔离措辞）、AC-06（超时→kill，含标题）、AC-07（spike 路径 + 反例措辞）、AC-09（并行阈值）、AC-12（并发上限）按偏差表改写；
   - `README.md` 重写为"版本与来源 + 偏差 + 引用映射"入口；
   - 其余分册正文语义未改。
4. **M0–M2 回写批次**（2026-10-30，ISSUE-020/050；分支 `feat/docs-m3-spec-writeback`，评审 REV-014）：
   - **00**：`EdgeIndex`（结构体）与 `EdgeIdx=usize`（数值别名）命名裁定（ISSUE-043）；§0.4 新增 5 个错误码（`NODE_WORKER_HANDSHAKE/PROTOCOL/IMPORT/CRASH`、`REPLAY_RECORD_MISSING`）并与 architecture §8.2 对齐；`NodeError::NodeWorker`/`WorkerErrorKind`（D10）；`NodeAbandoned`/`NODE_ABANDONED` 标 deprecated（D1）；`RunMetrics.killed_workers` 与 `abandoned_tasks` 恒 0；新增 `MAX_WORKERS`/`MAX_WORKERS_PER_RUN` 常量、`PYTHON_CONCURRENCY_*` 标 v2 忽略。
   - **01**：`EdgeIndex`/`EdgeIdx` 跨册一致（字段类型改 `EdgeIdx`）；`RunState.killed_workers` 替代 `abandoned_tasks`；gate 求值改内置 op 口径（D6）。
   - **02**：§2.5/§2.4 R14 改 **D7** 口径（加载期不 import 用户 Python；python_node 的 import/签名后移 `NODE_WORKER_IMPORT`，**格式非法仍在加载期报 `DSL_SYMBOL_NOT_FOUND`**；`DSL_SIGNATURE_MISMATCH`/`DSL_SYMBOL_CONFLICT` 仅内置 gate op，ISSUE-026）；§2.6.3 未提供资产根时跳过 R14c（ISSUE-027）；§2.6.1 subgraph 时机改为「run 启动后、实例创建前」（ISSUE-046）；§2.7.4 `validate` 退出码标注 M1 临时口径（ISSUE-059）。
   - **03**：§3.9 重写为 **kill 语义**（D1：`cancel`→2s→SIGTERM→1s→SIGKILL 进程组；`killed_workers`；无 `NodeAbandoned`），新增 §3.9.4；§3.5/§3.11/§3.13 并发上限改全局 `MAX_WORKERS`（CPU×2）+ 每 run 8（D4/D8）；§3.8 取消 kill；§3.12 标注 M2 单通道临时口径（ISSUE-033）；§3.7/§3.14/§3.15 同步。
   - **04**：§4.2 `NodeContext` v2（D2/D5：`ctx` 代理字段、无 `cancel_token`、`seed`、`now/random`、`deadline_remaining_ms`）；§4.3.3 Python 代理成员表；§4.3.5 签名校验后移到 worker 启动期；§4.3.6 执行模型重写（worker 子进程/进程组 kill/并发上限，D1/D4/D7）；§4.4.4 subgraph 时机（ISSUE-046）；§4.7 `SymbolResolver` 改 D6/D7 口径。
   - **06**：§6.2.4 v2 列（`seed`/`record_mode`/`killed_workers`；`RecordMode` 取值 `none|hash|full`，ISSUE-037）；新增 §6.2.5.1 `NodeError→JSON` canonical 形状（ISSUE-053）；§6.3.4 `NodeTimedOut.at` 取生产者时间（ISSUE-034）；§6.4.1 标注 M2 单通道 8192（ISSUE-033）；§6.4.2 `Transition` >64KiB 的 M2 行为（ISSUE-035）；§6.4.5 fail-stop 未接线（ISSUE-036）；§6.8.3 `owner_pid` 提示未实现（ISSUE-048）；§6.10 CLI 退出码/资产根同步（ISSUE-027/059）。
   - **07**：§7.2 AC 表新增「M2 实测口径 / M3+ 补齐」两列（ISSUE-054）；AC-06 进程组 kill 判定、AC-07 真 worker、AC-09 阈值「M3 实测后回填」、AC-12 M5 口径。
   - 02–07 顶部 banner 改为「已回写」；`doc/architecture-v2.md` §8/§8.2 标注回写完成并保留为决策记录；`README.md` 行数表同步。
5. **REV-014 条件修正批次**（2026-10-30，评审 `REV-014` C1–C4 + 建议 #5–#9；分支 `feat/docs-m3-spec-writeback`）：
   - **C1 悬空引用**：10 处「PROVENANCE §2.5」（00/02/03/04/06/07 banner、`README.md` ×2、`doc/architecture-v2.md` §8 ×2）统一改为「PROVENANCE §2 第 4 条」——回写批次是 §2 第 4 条，不存在 §2.5 小节。
   - **C2 D7 精确化**：02 §2.4.2/§2.5.2/§2.5.3/§2.5.4、00 §0.4、04 §4.3.5、07 AC-04、architecture D7 统一为「`python_node` 的 `module/function` **格式非法仍在加载期报 `DSL_SYMBOL_NOT_FOUND`**（`node_id`）；import/签名失败后移到 worker 启动期 `NODE_WORKER_IMPORT`（Crashed）；`DSL_SIGNATURE_MISMATCH`/`DSL_SYMBOL_CONFLICT` 仅内置 gate op」。
   - **C3 04 §4.9 对齐表**：节首结论改为「回写后已复核」；A2 改「8 变体（含 `NodeWorker`，D10）」；A7 改「`STREAMING_CHANNEL_CAP` 仅规范容量（M2 单通道 8192，ISSUE-033）、`stream_tx` → `emitter`、`PYTHON_CONCURRENCY_*` v2 忽略（D4）、并发上限 `MAX_WORKERS`/`MAX_WORKERS_PER_RUN`」；A17 补 M2 单通道临时口径。
   - **C4 SDK 名称/基类**：04 §4.3.3/§4.3.4/§4.5.3 的 Python 异常/包名统一为 `zgraph_worker.*`（`WorkerCancelled`/`BusinessError`/`ReturnNotDictError`/`ProtocolError`），`ProtocolError` 基类改为 `Exception`（**不是** `ValueError`），未捕获 → worker `error` 帧 `NODE_WORKER_PROTOCOL` → `NodeWorker{Protocol}`；`GateCtx` 改为预留名 `zgraph_worker.GateCtx`。
   - **建议项**：#5 `config.exec_class` 标 D4 v2 忽略；#6「派发前」统一为「run 启动后、实例创建前」（ISSUE-046）；#7 修复 04 §4.6.2 粗体不配对；#8 01 `RunState.killed_workers` → `u64`（对齐实现/00）；#9 `ctx` 只读表述改为「公有属性 + 约定只读，SDK 不做写入保护」。
   - **复审 R1–R5**（2026-10-30，复审提交 `07ce558`）：R1 `04 §4.7.4` 汇总行补 python_node 格式非法与 `node_id` 定位；R2 `04 §4.3.1` 示例改 `from zgraph_worker import BusinessError`；R3 `04 §4.3.1` 删除“`zgraph` 由框架提供”表述、明确不提供兼容包；R4 `04 §4.3.3` `log` 失败列改实现口径（SDK 不做类型校验）；R5 `02 §2.5.4` expected/actual 改实现真实形状（python_node `format` / gate `allowed`）。
   - 本次改动后按 `shasum -a 256 0*.md` / `wc -l 0*.md` 重新实测并更新 §3（合计 **5530 行**）与 README 行数表。
6. **后续修订规则**：任何对 spec 的修改**必须**同步本文件 §3 的 SHA-256/行数、在 §2 追加修订记录（日期 + 范围 + 关联 ISSUE/REV），并在评审记录中引用。
7. **M4 Harness 回写批次**（2026-10-30，ISSUE-067/070；分支 `docs/docs-m4-spec-writeback`，评审 REV-023，提交见该分支 git log）：
   - **对应已合并代码**：kernel `ce491de`（`TOOL_ERROR` 统一 + `HarnessErrorKind::ContextOverflow` 第 8 值，REV-018）；py-worker `e099927` + `c463a49`（`zgraph_worker/tool_runner.py` discover/call + 可选 `timeout_ms`，REV-019）；worker `94e1f8a`（`WorkerFailure::Worker` 装箱消除 genai `preserve_order` 引发的 clippy 回归，REV-021）；harness `4781dd7` + `344b73d`（M4 Harness 主体 + C2 工具可见性，REV-020）；e2e `fda996c`（`tests/e2e/harness_ac08.rs` + `tests/fixtures/graphs/harness_ac08/`，AC-08a fake 必过/AC-08b 手动，REV-022）。本批次评审时 base = `origin/main` `fda996c`；合并时 rebase 至 `12ab2f6`（REV-023 评审记录提交）。
   - **05**（Harness）：顶部 banner/状态改为「M4 已实现」并指向新增 §5.13；§5.1.1 H4 改「Python 调用一律在 worker 子进程；Harness 不在事件循环线程阻塞」；§5.1.2 组件分解改实际模块（`crates/zgraph-harness/src/*` + `python/zgraph_worker/tool_runner.py`）；§5.2.1 加载执行者改为 Harness 在 `agent_call` 实例执行时加载、fake 模式跳过 `api_key_env`；§5.3.1 trait 无 tokio `CancellationToken`、自有 `CancelToken`（轮询包装）+ `HarnessCtx` seam（D14/D15）；§5.3.2 辅助类型同步；§5.3.3 改 **genai 0.7.2 实测映射**（`exec_chat_stream` / `ChatStreamEvent` / `capture_*` / `webc::Error`）；§5.3.4 fake provider 落地说明；§5.3.5 重试编排位置改为 **Harness**（D16）；§5.3.6/§5.3.7 超时与取消改轮询/杀进程组；§5.5.1 重试位置、§5.5.3 空响应裁定（D17）、§5.5.4 执行循环伪代码改 v2、§5.5.5 措辞；§5.6 全面改为 **worker 子进程工具协议**（`discover`/`call` payload、失败 reason 封闭表、超时 = kill 进程组、无 import 缓存、全局工具信号量并发、8 KiB 截断、`AgentConfig.tools` 解析）；§5.7 `TOOL_ERROR` 唯一性 + `result_summary: Option<Value>`；§5.8 1:1 emit 实况；§5.9 `HarnessExecutor` 映射；§5.11 常量已补录与差异闭合状态；新增 **§5.13 M4 v2 落地偏差记录（D13–D21）**（工具子进程/取消轮询/HarnessCtx/重试编排+超时兜底/空响应裁定/ContextOverflow 第 8 值/`name()` 实况/input_mapping 优先级/genai `preserve_order` 依赖副作用）与 **§5.13.3 已知限制与 ISSUE-073..082**。
   - **00**：§0.3 `HarnessErrorKind` **7→8 值**（追加 `ContextOverflow`，M4/方案 A，见 05 §5.13 D18）；§0.3 `StreamingEvent::ToolCallEnd.result_summary` 改为 `Option<Value>`（nullable）；§0.4 增 `TOOL_ERROR` 唯一性注记（禁止 `HARNESS_TOOL_ERROR`，ISSUE-070）并标注 `HARNESS_CONTEXT_OVERFLOW` 映射；§0.5 增 05 §5.11.1 七常量核对注记（均已收录且数值一致）；banner 增 M4 批次说明。
   - **04**：§4.2.1 `NodeContext` 代码块补 `seed: u64`、字段来源改为 **`RunSpec.options.seed`**；§4.2.2 方法表补 `global_vars()` / `seed()`；新增 **§4.3.7 L3 幂等与外部副作用声明契约**（ISSUE-022/067）。
   - **07**：AC-01 补 `run_output.result.content` 的 M4 形状口径（CLI `run --fake-llm` 仍属 M5）；AC-08 拆为 **AC-08a（fake provider 必过）** 与 **AC-08b（真实模型手动，`#[ignore]`，不纳入 CI/基线）**；AC-12 后补 L3 引用（04 §4.3.7）。
   - **已知限制与 ISSUE**：05 §5.13.3 列出 ISSUE-073..082（genai preserve_order + worker Box 缓解 / 8 KiB 截断 / args_summary preview / node_timeout_ms / prompt 路径词法安全 / futures-util / 工具并发 / discover reason 超集 / 函数 >60 行 / L2 记录未接线），由 Lead 在 M4 收尾（REV-024）统一开号登记。
   - 本批次后按 `shasum -a 256 0*.md` / `wc -l 0*.md` 重新实测并更新 §3；`doc/spec-v1.2/README.md` 的行数表与根 `README.md` 由 Lead 收尾（不属本批次）。
   - 未改：08 审计关闭矩阵（历史记录，不改语义）；`doc/progress.md`、`doc/issues.md`（Lead 收尾）。


8. **M5 spec 回写批次**（2026-10-30，REV-035；分支 `docs/docs-m5-writeback`，评审 REV-035）：
   - **来源**：DR-002 §9 回写清单（REV-025 关闭项）+ 已合并实现评审 REV-026/027/028/029/033/034/038；未合并的 REV-030/031/032 只登记契约/退出码，**不写「已实现」**。
   - **07**：验收资产落点 `examples/**` → `tests/fixtures/graphs/**`（architecture §2.2；仓库无 `examples/`）；AC-01/AC-08 补 `--fake-llm=<script>` 与裸 flag 语义（裸 flag=内置默认脚本、确定性需显式脚本）；AC-12 回写 M5 实测口径（REV-029：8+2 组合 / 同库 10 run / `with_limits(4,2)` / 峰值与耗时 / ISSUE-079 工具并发边界 / SQLITE_BUSY 未触发）；§7.2 状态注记与 §7.3 #6 标 ISSUE-084（REV-034+REV-030）。
   - **05**：§5.2.6 实例超时优先级（test-hooks force > declared > `timeout_hint` > 180s）与 kernel/harness 落地口径（REV-026/REV-033，ISSUE-076 部分修）；§5.6.5 结构感知截断（输出必定合法 JSON 且 ≤8 KiB）/ traceback 8 KiB / summary ≤256·512（ISSUE-074 fixed，REV-027）；§5.8.5 严格上界；§5.13.3 L1/L4/L8/L10 状态更新（L8 剩余清单随 M6、L10 落库待 REV-030）。
   - **06**：§6.2.4 `runs` 补 `worker_versions_json`/`model_params_json` 列与 `seed`/`record_mode` 写入口径（REV-028）；新增 §6.2.13 L2 记录表（`recorded_outputs`/`llm_calls` 的写者/键/hash/status/脱敏）；§6.8.1 中止标定两段原语与判活分工（REV-034，CLI 调用随 REV-030）；§6.8.3 ISSUE-048 更新；§6.9.1 `:memory:` event_count；§6.10.1 手写解析现状；§6.10.3 `--python`/`--record`/fake-llm 口径；§6.10.5 replay 退出码与 `REPLAY_*` 命名空间；§6.10.6 与新增 §6.10.7 `zgraph replay` 契约。
   - **00**：§0.4 补 `REPLAY_RECORD_MISSING` 之外的 `REPLAY_OUTPUT_MISMATCH`/`REPLAY_RUN_NOT_COMPLETED`/`REPLAY_RECORD_CORRUPT` 并指向 06 §6.10.5（均为 CLI 命名空间、不写 `runs.error_code`）。
   - **04**：§4.3.6 解释器优先级回写为 `--python` > `PYO3_PYTHON` > `ZGRAPH_PYTHON`（仅测试覆盖）> `python3`（REV-028）。
   - **08**：只追加 **§8.7 M5 注记**（ISSUE-061/074 `fixed`；076/081/082 部分修；048/084 wave2；033/036/059/073/075/077/079/080/085 留 M6），**不改** 8.0–8.6 历史快照。
   - **architecture**：§5.2 L2 写入口径、§5.3 replay 退出码、§6.1 legacy 归档（冻结只读、不物理删除）、§8.2 补 3 个 `REPLAY_*` 码。
   - **索引/归档**：`doc/reviews/README.md` 补 REV-025–029/033/034/038（已合并）并标注未合并编号；`doc/index.md` 与 `legacy/README.md` 标「已归档、无维护、不得新增代码」（DR-002 §8）；根 `README.md` 更新 M5 收尾状态。本批次后按 §3 重新实测 SHA-256/行数（00–08 合计 **5938 行**）。

9. **M5 wave2 回写批次**（2026-10-30，REV-035 第二批；分支 `docs/docs-m5-writeback`，base 已 rebase 到含 REV-030/REV-032 的本地 `main`）：
   - **来源**：REV-030（cli replay/L2、`llm_calls`、启动中止标定、`timeout_hint` 转发）与 REV-032（e2e AC-01/L2/崩溃标定/ISSUE-076，e2e 37 passed）；REV-031（ISSUE-066）尚未合并，signal/exit 载荷留第三个小 commit。
   - **06**：§6.2.4/§6.2.13 更新 L2 实现状态（`recorded_outputs` + `llm_calls` 已接线；`worker_versions_json`/`model_params_json` 仍 NULL，ISSUE-082 部分修）；§6.8.1 回写标定实际接线（trace 两段原语 + CLI 判活/`recovered[]`/warning/仅改 runs）与残余风险（REV-030 #2/#3/#4/#5，ISSUE-084 保持 open）；§6.8.3 `trace` pid 提示/截断已实现；§6.10.6 补 replay `--json` 实际字段；§6.10.7 按 REV-030 实现校准（events 逐字节复用 trace；outputs 仅 Completed+full、完整性/hash 预检、RecordedExecutor 按 `(node_id,attempt)`、内存 sink、比较 status/error/output、不写新 run/不改原 trace、`global_vars` 空 map 边界）并附 REV-032 e2e 证据。
   - **07**：AC-01 补 `--assets-dir` 与 REV-032 e2e 实测口径（3 用例、坏图/坏入口不产生 run、e2e 37 passed）；AC-12 更正「wave2 补 10×同图」为「REV-032 仅收敛 Nits、未新增该用例」；§7.3 #6 补真 SIGKILL e2e 与 ISSUE-084 剩余风险。
   - **05**：§5.2.6/§5.13.3 L1/L10 更新 ISSUE-076 已修、ISSUE-082 部分修（版本列仍 NULL）。
   - **00/architecture**：`REPLAY_*` 标已实现（REV-030）；architecture §5.2 更正版本列「仍未写入」。
   - **08**：§8.7 更新 ISSUE-048/076 已修、082 部分、084 实现完成但保持 open；补 REV-032 #1 kernel `fail_timeout` 未落 `instances.error_code` 的台账待办（待 Lead 登记 ISSUE）。
   - **索引/评审**：`doc/reviews/README.md` 补 REV-030/032 行（未合并：REV-031/035/036；REV-037 未开始）；`doc/index.md` reviews 范围同步；REV-028 §8.4 dry-run 回填（评审人三次环境阻断，Lead 在 main `3eff14b` 实跑通过）；DR-002 §9 #10 标已实现、#11 `REV-028 #6` 编号修正、#12 cli 函数清单同步（`doc/issues.md` 的两处交叉引用由 Lead 维护）。
   - 本批次后按 §3 重新实测并更新（00–08 合计 **5949 行**）。

10. **M5 ISSUE-066 回写批次**（2026-10-30，REV-035 第三批；base 已 rebase 到含 REV-031 的本地 `main`）：
   - **来源**：REV-031（kernel/worker/cli/harness 跨模块，主审 reviewer-a + 第二评审 reviewer-b；真实子进程 `exit 7`/`SIGKILL`/`exit 3` 证据；`NodeError::NodeWorker` 只增 `signal`/`exit_code`/`stderr_tail`）。
   - **06**：§6.2.5.1 `NodeWorker` 行补三可选键（仅崩溃路径且有 supervisor 进程退出证据时才出现；`stderr_tail` ≤1 KiB、UTF-8 边界、**不脱敏**；worker 自报 crash / spawn 失败等无进程证据路径省略）；把「不得新增顶层键」闭集表述修订为 M5 例外 + 后续登记规则；补 API source-breaking 备注。
   - **03**：§3.15 `NODE_WORKER_CRASH` 行校准为 `NodeWorker{Crash, signal, exit_code, stderr_tail}` 并补证据边界注记；顶部 banner 增 M5 批次指向。
   - **architecture**：§8.2 `NODE_WORKER_CRASH` 行与 D10 行补 M5 只增字段；§8.2 末补 source-breaking 备注。
   - **08**：§8.7 新增 ISSUE-066 `fixed`（REV-031）行与 API source-breaking 备注（REV-031 §7.3 #3；v0.1 已批准、发布说明备注）。
   - **索引**：`doc/reviews/README.md` 补 REV-031 行（跨模块双评审），未合并仅 REV-035/036（REV-037 未开始）；`doc/index.md` reviews 范围更新为 REV-025–034/038；DR-002 §4 ISSUE-066 标 REV-031 已合并。
   - 本批次后按 §3 重新实测并更新（00–08 合计 **5956 行**）。

11. **REV-035 评审条件修正批次**（2026-10-30，同一分支追加 commit，不改写 REV-035 评审记录；reviewer-a 首轮结论「有条件通过」，C1–C3 关闭后复审「通过」（REV-035 §8，`a21c523`），本批已合入 main）：
   - **C1**：根 `README.md` 状态段按 main `438822b` 事实重写——REV-030/031/032、CLI 启动标定/`recovered[]`、ISSUE-066/076 移入「已合并」；REV-035 标「本批待合并」；待交付仅 REV-036 收尾 → REV-037 → M6。
   - **C2**：07 AC-01 `run` 命令补 `--python-path python --python-path tests/fixtures/graphs/customer_ticket/py`（与 `tests/e2e/cli_support/mod.rs::ac01_run_args` 一致），并注明等价 `PYTHONPATH`。
   - **C3**：§3 保留 00–08 当前实测合计，同时补回 M4 历史测量注记（**5818 行**；base `origin/main` `fda996c`；合并 rebase `12ab2f6`），不替换历史。
   - **建议**：05 L8 / 08 ISSUE-081 更新 `run_in_runtime` 91、`build_plan` 75、新增 >60 行测试（74/78/88 等）；07 AC-12 单 run 基线改 0.79–0.85s；06 §6.10/§6.10.1 补 replay 子命令与 `--db`（run/replay/trace）；00/06/architecture 统一 `REPLAY_RECORD_CORRUPT` 触发描述；07 §7.1 `agents/prompt.md`；spec `README.md` 引 §2 第 8–10 条；`doc/index.md` reviews 范围只列已合并（REV-000–034/038）。
   - 本批次后按 §3 重新实测并更新（00–08 合计 **5958 行**）。
12. **REV-036 收尾批次**（2026-10-30，`docs/process-m5-wrapup`，评审 REV-036）：按 REV-035 §8.6 #2 把 ISSUE-084 状态在 06 §6.8.1 / 07 §7.3 #6 / 08 §8.7 / DR-002 §9 #10 统一为 **fixed**（残余多机/EPERM/host 边界转 **ISSUE-088**，M6）；06/07/08 仅措辞修订（行数不变），§3 对应 SHA-256 已再实测；§5 M5 完成声明不变。
13. **M6 spec 回写批次**（2026-10-31，REV-057；分支 `docs/docs-m6-writeback`，评审 REV-057）：
    - **来源**：M6 已实现/评审 REV-040/041/042/043/044/047/048/049/050/052/055/056/058/060（ISSUE-096 登记引用的 REV-061 属 wrapup，尚未合并）；ISSUE-082 按 REV-048（已合并 `main` `5192168`）、ISSUE-088 按 REV-056（已合并 `main` `3d227fc`）登记；ISSUE-095 open、ISSUE-096 仍 open（pending REV-061 wrapup，合并后置 fixed）。
    - **06**：§6.2.2/§6.2.4/§6.7/§6.9.2/§6.10.7/§6.12.3 D5 回写 trace schema **v3** + `runs.global_vars_json`（canonical JSON）+ 逐版本迁移/`VACUUM INTO` 备份/失败恢复/只读拒绝 + replay 兼容（REV-049/ISSUE-087）；§6.4.1 双通道已拆回 + 发送序号有序合并（**同发送端有序、跨发送端各通道 FIFO、无全局序**；REV-041/ISSUE-033/ISSUE-092）；§6.4.5 fail-stop 已接线与边界（REV-047/ISSUE-036）；§6.10.1/§6.10.2/§6.10.5 CLI 退出码 0/2/1/70、`-v/--verbose`→`RUST_LOG`、纯文本无 ANSI/`NO_COLOR`、v0.1 不引入 `clap`、`--version` 字面（REV-043/ISSUE-059）；§6.10.3 human 生命周期实时 + token 聚合段/`[seq N]` 口径（REV-050/ISSUE-085）；§6.2.4/§6.2.13/§6.9.1 `runs.worker_versions_json`/`model_params_json` 已接线（写时点=run 结束、关闭写端前；`update_run_columns` 原语；两列 JSON 数组形状/去重/无观测 `[]`/不含密钥/失败 run 保留摘要；`:memory:` 不写；写失败为尽力写——仅返回 Err、不置 fatal、不影响退出码，真实损坏由 flush/close 走 fail-stop；worker 版本仅聚合节点 worker，`base_url` **已实现**防御性脱敏（去 userinfo/query/fragment，仅保留 scheme://host[:port][/path]））——REV-048/ISSUE-082（C1 方案 B）与 ISSUE-088 回写（§6.8.1/§6.8.3）。
    - **06（ISSUE-088，REV-056）**：§6.8.1 判活顺序改「同机（OS hostname + env 别名）→ 本地 pid；跨机/host 未知 → heartbeat 60s 陈旧性（未陈旧→跳过+告警；陈旧/缺失→判死）」、`pid_alive` EPERM→存活/ESRCH→不存在/其它保守存活；§6.8.3 pid 提示校验 `owner_host`（跨机/未知 host 提示 may be running on another host）；bridge 对 unknown/空 `owner_host` 落库前兜底 OS hostname；`recovered[]` 形状不变、`--json.warnings[]` 不再恒空；残余 kernel hostname 口径差登记 ISSUE-095。
    - **05**：§5.2.1 prompt/constraints `canonicalize` 路径安全（REV-042/ISSUE-077）；§5.8.5 preview 结构摘要 + 敏感键脱敏 + ≤256（REV-042/ISSUE-075）；§5.6.5/§5.13 D13/§5.13.1 工具并发共享 `WorkerLimits`（REV-052/ISSUE-079，e2e 适配 REV-060）；§5.6.2.1/§5.2.7 discover failure reason 闭集冻结（REV-057/ISSUE-080）；§5.13.3 L2/L5/L7/L9/L10 状态更新（L10 随 REV-049/REV-048 fixed）。
    - **07**：AC-06 补实例级 `NODE_TIMEOUT`（自报 + watchdog、`elapsed_ms` 同源；REV-040/REV-055/ISSUE-086）；AC-12 增「同一图 10 并发 run」用例（REV-044/ISSUE-089）与工具+节点峰值口径（REV-060），并更新 `tests/e2e` 资产路径/命令。
    - **03/04**：03 §3.12、04 A7/A17 与 §4.2 的 `emitter`/容量行的「M2 临时单通道」注记改「**M6 已拆回双通道（REV-041）**」（跨册一致性，避免与 06 §6.4.1 矛盾）。
    - **08**：只追加 **§8.8 M6 注记**（按 REV 关闭 ISSUE-033/036/059/075/077/079/080/085/086/087/088/089/092/094；新增/登记 ISSUE-095（open）与 ISSUE-096（pending，REV-061 wrapup）；M6 已修 ISSUE-082（REV-048）、ISSUE-088（REV-056）；081/090/091/093 仍开放），**不改** 8.0–8.7 历史快照。
    - **architecture**：§5.2 注记 trace schema v3 / `runs.global_vars_json` / 逐版本迁移备份口径；以及 082 的两列写入口径（REV-048）。
    - **台账**：`doc/issues.md` 更新 **ISSUE-036**（REV-047，`main` `b39374a`）、**ISSUE-079**（REV-052/060）、**ISSUE-080**（REV-057）、**ISSUE-082**（REV-048）、**ISSUE-085**（REV-050）、**ISSUE-087**（REV-049）、**ISSUE-088**（REV-056，`main` `3d227fc`）、**ISSUE-092**（REV-057）为 `fixed`；新增 **ISSUE-095**（P3 kernel，open，kernel hostname 口径差）与 **ISSUE-096**（P2 process，**open/pending REV-061 wrapup**，merge.sh 测试前构建 CLI bin，合并后置 fixed）。
    - **补齐（同分支第二提交）**：02 §2.7.4 `validate` 退出码删除 M1 临时口径 `3`（REV-043）；`README.md` 双通道口径改「M6 已拆回 8192+1024 + 发送序号有序合并（REV-041）」、行数表同步（05/06/07/08）、批次索引补 M6（§2 第 13 条）。
    - **第三提交（ISSUE-082 回写，REV-048；含 C1 方案 B 口径：`update_run_columns` 失败为尽力写）**：06 §6.2.4/§6.2.13/§6.9.1（两列写入口径/形状/`:memory:` 不写）、05 §5.13.3 L10 与 M6 状态、07 §7.2 状态、08 §8.8（ISSUE-082 → fixed）、`architecture §5.2`、`doc/issues.md`（ISSUE-082 → fixed）；§3 行数重测（00–08 合计 **6033 行**，第三提交时点；后续 ISSUE-088 回写后最终 6038 行）。
    - **后续提交（ISSUE-088 回写 + ISSUE-095/096 登记，REV-056；096 引用 REV-061 wrapup）**：06 §6.8.1/§6.8.3、08 §8.8、`doc/issues.md`（088 fixed、新增 095/096）、本文件 §2 第 13 条/§3/§5；§3 行数重测。
    - 本批次后按 §3 重新实测 SHA-256/行数（00–08 合计 **6038 行**）。

14. **W1 SOP v1 回写批次**（2026-10-31，REV-066；分支 `docs/docs-sop-v1-ac`）：
    - **07**：新增 **AC-13**（software_team SOP v1 per-item 评审链路：full 60/100/smoke 11/18/exhaust 变体 hash、approved 链路、`NODE_PYTHON_EXCEPTION`/`HARNESS_MAX_ITERATIONS` 失败路径、real-LLM `#[ignore]` smoke）；对应已合并 REV-064（py-worker `parse_verdict`）/REV-065（e2e）。
    - **DR-004**：追加 §10 执行记录（用户 2026-10-31 确认 A1–A7「按推荐」+ W1 交付证据）；DR 不属于 spec 分册，不参与 §3 hash 清单。
    - 其余 00–06/08 分册未改；本批次后按 §3 重新实测 07 SHA-256/行数与合计（**6051 行**）。

15. **W2a0 返工设计冻结批次**（2026-10-31，REV-067；分支 `docs/docs-rework-design`）：
    - **来源**：DR-004 §3 W2a0 / §5 D1 / §3 W2a DoD / §6；REV-063 C9/C11/S12；用户 2026-10-31 确认 A1–A7。**本批是设计冻结（docs only）**，不宣称 W2a 实现完成；语义冻结见 [`../decisions/DR-005-w2a-defer-gate-freeze.md`](../decisions/DR-005-w2a-defer-gate-freeze.md)（评审编号 REV-067，记录由评审人撰写）。
    - **00**：banner 增 W2a0 批次；§0.3 增 `OnGateFalse` 枚举（`Settle`/`Defer`，Default=Settle）；§0.4 DSL 域新增第 18 码 `DSL_DEFER_NOT_REENTRANT`（17→18）并补运行期 defer 注记（`RUN_DELIVERY_TO_TERMINAL`/`RUN_DEADLOCK`/`NODE_INTERNAL` 复用）；§0.5 `DEFAULT_MAX_ATTEMPTS` 语义扩为「自环与 defer 重入统一上限」；§0.7 增 gate=false/目标已 Completed 两条消歧；§0.8 07 行改 AC-01..AC-14。
    - **01**：banner 增 W2a0；`CompiledEdge.on_gate_false`（缺省编译期补齐）；§1.2.2 寻址表增 defer 挂账/attempt-per-activation 行；新增 **§1.4.1 `DeferredReceipt`** 与 per-port 继承/`settled_by_node` 重置规则；§1.5 `GateResultRecord` 增 `deferred`/`attempt`/`verdict`；§1.6 `RunState.deferred_receipts`（BTreeMap）；§1.7 `GateEvaluation.verdict`；§1.3 buffer 初始化注记。
    - **02**：banner 增 W2a0；§2.2.1 增 `on_gate_false` 行与 `max_attempts` 语义；§2.2.2/§2.2.4 白名单 + serde S-2/S-5/S-6；§2.2.3 类型桶；§2.3.3 自查表增 R15（不适用）；§2.1.2/§2.1.3 L2 增 R15、执行顺序 `R1→R13→R15→R14`；§2.4 标题/总表/模板/正反例/裁定（D-02-11/12）同步 R15 与 `DSL_DEFER_NOT_REENTRANT`；§2.7 码数 18/顺序；§2.8.2/§2.8.3 hash 前置与 `on_gate_false` 投影；§2.9.2/§2.9.3 编译映射；§2.10/§2.11 同步。
    - **03**：banner 增 W2a0；§3.4.1 gate 求值返回 `verdict`；§3.4.2 应用摘要；新增 **§3.4.3 defer 边应用与 attempt-per-activation**（决策矩阵、按端口继承 + settled 重置、end/自环/超限、伪代码）；§3.6 注记；§3.9.3/§3.10 dormant 与确定性 `RUN_DEADLOCK`；§3.15 矩阵补 4 行。
    - **04**：banner 增 W2a0；§4.6.4 `GateResultRecord` 增三字段并明确「不记耗时」仍有效、后续新增须升 spec 版本。
    - **06**：banner 增 W2a0；trace schema **v4**（`TRACE_SCHEMA_VERSION=4`）；§6.2.2 v4 口径 + M6 v3 历史口径保留；§6.2.8 DDL 增 `deferred`/`attempt`/`verdict_json` 并修订 04 §4.6.4 引文；§6.2.10 查询模式；§6.2.12 payload 映射；§6.3.3 deferred 不改 severity；§6.7 迁移改 v4（新库单事务 1/2/3/4、逐版本、备份/回滚/只读拒绝）；§6.10.2 validate R1–R15；§6.10.4/§6.10.6/§6.10.7 回放与 `--json` 注记。
    - **07**：banner 增 W2a0；新增 **AC-14（a–h）**并加入 §7.1/§7.2 与 DoD；AC-04 码数 17→18 + `defer_not_reentrant.json` 反例；Pass 标准 AC-01..AC-14；DoD「15 条校验」。
    - **08**：只追加 **§8.9 W2a0 注记**（不动 8.0–8.8 历史快照）；登记 ISSUE-100 设计层关闭、ISSUE-101 承接、architecture-v2 跨册残留（C-7）。
    - **SOP**：`doc/design/software-team-sop-v1.md` 升 **v1.1**（approved-only v1 语义不变 + rework v1.1：双 profile、环上可重入边全 defer、`P→W.feedback` 回边、`max_attempts` 预算、codegen `--mode`、§9/§10/§11 同步；DR-005 §6）。
    - **索引/README**：`doc/spec-v1.2/README.md` 行数/状态注记同步；`doc/index.md` 增 DR-005 行、SOP 描述改 v1.1、reviews 范围补 REV-062–066 与 REV-067 待评审。
    - **冲突/补全**（DR-005 §11）：C-1 B1 buffer 继承（**Lead 2026-10-31 裁决**：per-port 继承 + 重入实例清空 `settled_by_node`）、C-2 `W.feedback` 可选端口、C-3/C-5 04 §4.6.4 例外、C-4 R15 执行顺序例外、C-6 DR-004 两条 defer 边为最小子集、C-7 architecture-v2 残留不改；E-1–E-5 边界冻结（E-3 receipt 不消费）。**v3 seed（`tests/fixtures/trace/schema_v3_seed.sql` + `v0.1.0_expected_trace.json`）W1 未交付，改由 W2a e2e 交付**（W1 遗留缺口 **ISSUE-104**）。
    - 本批次后按 §3 重新实测 SHA-256/行数（00–08 合计 **6308 行**；REV-067 条件修复后 6334 行见第 16 条）。

16. **REV-067 条件修复批次**（2026-10-31，REV-067 有条件通过 → 条件关闭；分支 `docs/docs-rework-design`）：
    - **C-1 应用顺序对齐**：03 §3.4.2 改为「终态短路 → blocked（先 emit 再结算/挂账）→ passed（先 resolve 后 emit）」；`RUN_DELIVERY_TO_TERMINAL`（含 `end` 特例）由 resolve 失败返回、**不产生 GateResult**；超限按现行自环顺序（resolve 内 create Crashed + `fail_run` → 外层 emit 为 no-op）；DR-005 §3.3 伪代码与 03 逐行同构。
    - **C-2 defer 前置**：`resolve_defer_reentry` 首行 `if on_gate_false != Defer → NotApplicable`；03 §3.4.3 / DR-005 §3.2 决策矩阵补「`Settle` + passed=true + 目标 Completed → 现行 `RUN_DELIVERY_TO_TERMINAL`」行（行 8），并冻结自环/非自环 attempt 计算。
    - **S-1**：DR-005 §7 a 与 07 AC-14 a 的 `deferred` 断言修正为 attempt0 `false`、attempt1 `true`（target Completed → dormant → run Completed）。
    - **S-2**：迁移历史行口径拆分——v3→v4 只补 `gate_results` SQL 列默认值，历史 `events.payload` 保持 v3 形态（`trace --json` 不合成键）；新 run payload 含三键。落 06 §6.2.8/§6.10.4/§6.10.6、07 AC-14 h、DR-005 §4.2/§4.3。
    - **S-3**：DR-005 §8 增 cli 适配项（`bridge.rs::map_gate_result` 过渡值 → kernel 真实值；Lead 跨模块声明 + 第二评审：REV-070 追加、REV-071 另定）；§12.1/§12.2 同步，不再写「cli 不改」。
    - **Lead 补充 2 点**：(a) AC-14 g 改 R15-valid 变体（`a` 增 defer 自环作可重入来源，`a→b` 仍 blocked → 确定性 `RUN_DEADLOCK`；DR-005 §7 g / 07 AC-14 g / E-4 同步）；(b) 超限路径 GateResult **不入库**（`fail_run` 先关闭 sink，外层 emit no-op；trace 证据 = Crashed 实例 + 之前 GateResult + `RunFinished{Failed}`；DR-005 §3.3/§3.5/AC-14 e、03 §3.4.2/§3.4.3、07 AC-14 e 同步）。`DeferredReceipt.updated_at` 统一为实现口径 `String`（RFC3339 毫秒 Z，与既有 ts/at 同口径）。
    - **评审建议 N-1…N-11**：N-1 环语义（SCC 为权威、简单环更严，E-1 为放行例）、N-2 ISSUE-104、N-3 README 07=323（首轮 322 → 修复后 323）、N-4 子图 R15 收敛 `DSL_SUBGRAPH_INVALID`、N-5 R15 首错、N-6 `GateEvaluation.verdict`、N-7 `verdict:null` 示例、N-8 删除 N-3 占位、N-9 结算集统一节点级、N-10 00 per-port、N-11 R4 错误码归属/解析阶段；另 REV-070 S1 → **ISSUE-105**、W1 seed 缺口 → **ISSUE-104**（本批只引用编号，不改台账）。
    - 本批次后按 §3 重新实测 SHA-256/行数（00–08 合计 **6334 行**；含 README/PROVENANCE/REFERENCES 合计以 `wc -l` 为准）。

17. **W2a 实现回写批次**（2026-10-31，REV-073；分支 `docs/docs-gate-defer-spec`）：
    - **来源**：已合并实现——dsl `fa6ea16`+`bc92951`（REV-069）、trace `27f9200`（REV-070）、py-worker `dd96d89`+`f685178`（REV-068）、kernel `ee4ca27`+cli `6c90a16`（REV-071）；e2e 分支 `test/e2e-gate-defer`（**REV-072 已通过**，含 N1 后 §7 追加复审；e2e 分支待合并，HEAD 以合并时为准）。本批**按实际实现校准**，不重写 W2a0 冻结决策。
    - **07**：AC-14 资产路径改 `tests/fixtures/graphs/defer_gate/**` + `tests/fixtures/trace/{schema_v3_seed,v0.1.0_expected_trace,seed_graph,GENERATION}`；测试落点改 `tests/e2e/gate_defer.rs`（a/b/f/g/h 均通过）与 `tests/e2e/software_team_v1.rs`（c/e codegen 静态 + 运行期，REV-072 已通过；e2e final 将改单 item rework profile，c/e 规模/断言以 final 为准）；§7.1/§7.2 行同步。
    - **01/03/04/06 as-built 注记**：`DeferredReceipt.updated_at` RFC3339 `String`、审计字段当前无读取方；per-port 继承 + `settled_by_node` 清空、不回收继承端口结算；`apply_publish` 四路径/超限 GateResult 不入库/`fail_if_blocked_deferred` message；`GateEvaluation.verdict` 实现口径；DDL 三列表尾追加（列序无语义）、`verdict_json` 损坏**严格报错**（ISSUE-105 待定案）、迁移/只读拒绝实测。
    - **02 as-built 注记**：R15 首错 + `expected/actual` 与冻结模板一致（REV-069 N1/N5）；子图内 R13+R15 跨规则聚合 2 条 `DSL_SUBGRAPH_INVALID`（REV-069 N7，子图为例外）。
    - **08**：§8.9.1 追加 W2a 实现证据表（模块/commit/REV/测试计数/as-built 偏差/台账），不改 8.0–8.9 历史。
    - **DR-005 / DR-004**：DR-005 §7.5 R1–R5 修正（超限证据/§7 h L1 拆分/行 8→9/伪代码 3-4-8/commit 数）；AC-14 as-built 注；DR-004 §10 追加 W2a0+W2a 执行记录（不改原决策）。
    - **索引/架构**：`doc/index.md` reviews 范围补 REV-067–072（073 待评审）、DR-005/SOP 描述；`doc/architecture-v2.md` 只加 banner 注记指向 02 R15 / 00 §0.4（18 码），不改 D1–D12 历史。
    - **e2e 证据（REV-072 已通过；e2e 分支待合并，最终 HEAD 以合并时为准）**：`gate_defer` **8 passed/0 ignored**（a/b/f/g/h + 超限 + 2 辅助）、`software_team_v1` **11 passed/1 ignored**（real-LLM manual）；e2e **58/0/2**、workspace **694/0/3**；**稳定性口径** = 两 target 连跑 **3 次** + `smoke_rework` 过滤 **5 次**（无全量 5 轮）。07 AC-14 c/e/f/h 按实测校准（c/e 规模/断言待 e2e final 单 item profile）：c（c 当前 smoke 11/21 运行期；前进/feedback 边 attempt0/1 的真实 `deferred`/`verdict`；P1 消费 round2 review；archer 拿返工后最终产物）、e（victim 不写死、恰一 Crashed W attempt=3、超限那次求值不产生 GateResult / 更早轮次 attempt0-1 行保留、其它边仍可有行）、f（`verdict` 为原始路径值：`passed=false` 而 `verdict=true`）、h（seed 新 run `verdict="seed"`；defer 自环新库真实 bool/int；`max_attempts=1` 首轮即超限自环边 0 行 GateResult）。**ISSUE-104 → fixed**（e2e 分支 + REV-072）；full rework 60/116 运行期未覆盖 → **ISSUE-112**。
    - **台账**：新增 ISSUE-104（v3 seed；**已 fixed**）、ISSUE-105（`verdict_json` 口径）、ISSUE-106（trace 测试/守卫）、ISSUE-107（kernel N1–N7）、ISSUE-108（dsl N5–N8）、ISSUE-109（py-worker S1）、ISSUE-110（e2e L2/测试行数）、ISSUE-111（e2e trace fixture 路径归属）、ISSUE-112（full rework 运行期未覆盖）；ISSUE-100/101 → fixed；ISSUE-062 保持 open。
    - 本批次后按 §3 重新实测 SHA-256/行数（00–08 合计 **6389 行**）。

## 3. 文件清单（SHA-256 + 行数）

| 文件 | SHA-256 | 行数 |
|---|---|---|
| `00-总纲.md` | ce92145f7d7296865f4ddaabc9af9ff17efe22c8f50cfa9dffea1e7a9ded25c5 | 309 |
| `01-领域模型与状态机.md` | 710f6c8f30d9902ea4c7495e9a133c88d80bf4bf5588d5502b24bbe28b4bf85d | 346 |
| `02-DSL与校验.md` | 04820a7d3c9839a9589dd2bbb583186a4d1a8e427b9a7b0f6c05044a57d30928 | 935 |
| `03-调度器与生命周期.md` | a7553b416f2ac8e1d8d984a4e5f0fd8ebac4404dd534b9283433101f2d7aa640 | 489 |
| `04-节点接口与扩展点.md` | 46247060b7b9da82656773aac7b98267b2c9e5cb959a14b054ac7e98e4ead513 | 906 |
| `05-AgentHarness.md` | 52f35e7bfa6123a3f61612b95019ab0eca440fa251aa5e38763f370bc19dbf42 | 1429 |
| `06-存储与可观测.md` | 8ab4b5e2d62d3c0bca304de4d9e8af10def9c955073c76401bfb02ecb89fc9ee | 1117 |
| `07-验收用例.md` | 440af69793ae4970d6207c40856ffdf326a589f6f9e765a8989ab806dee009e9 | 334 |
| `08-审计关闭矩阵.md` | deded461b5b4aed2676f2ec92dabeed8089b2cd354c3e188278cc02971340caf | 524 |

> 00–08 合计 **6389 行**（`wc -l` 口径；2026-10-31 **W2a 实现回写批次后**实测（REV-073；00/01/02/03/04/06/07/08 **净 +51 行**（新增 87/删除 36），见 §2 第 17 条；W2a0 首轮冻结为 6308 行/§2 第 15 条）；此前 W1 SOP v1 批次为 6051 行、M6 批次为 6038 行（分支 `docs/docs-m6-writeback`、评审 REV-057，base 含 M6 实现 REV-040–060 + REV-061 wrapup，REV-048/ISSUE-082 合并 `main` `5192168`、REV-056/ISSUE-088 合并 `main` `3d227fc`）的本地 `main`）；含 `README.md`、`PROVENANCE.md`、`REFERENCES.md` 后总行数以 `wc -l` 为准（当前 6672 行）。

> **历史批次审计留痕（不覆盖）**：M4 Harness 回写批次后 00–08 合计 **5818 行**（`wc -l` 口径；评审时 base `origin/main` `fda996c`，合并 rebase 至 `12ab2f6`）；M5 全部回写批次后合计 **5958 行**（2026-10-30，REV-035，base 含 REV-025–034/038 的本地 `main`）。

## 4. 核验方式

```bash
cd doc/spec-v1.2
shasum -a 256 0*.md
wc -l 0*.md
```

## 5. 声明

- 源稿与提取文本在仓库外，**不可就地复核**；本目录文件是审计与开发的实际依据。
- **M0–M2 回写已完成（2026-10-30）**：00/01/02/03/04/06/07 正文即 v2 实现口径；`doc/architecture-v2.md` §8 的 D1–D12 与 §8.2 保留为**决策记录**，与正文冲突时以正文为准。**05 分册（Harness）的 v2 口径已于 M4 批次回写（2026-10-30，见 §2 第 7 条）**，新增 D13–D21 记录于 05 §5.13；08 为历史审计矩阵不改语义。
- 任何对 spec 的后续修改**必须**：更新本文件的 SHA-256 与行数、在 §2 增加修订记录（日期 + 范围 + ISSUE/REV）、并在评审记录中引用。
- **M5 回写已完成（2026-10-30，REV-035）**：00/04/05/06/07 正文与 `architecture-v2.md` §5.2/§5.3/§6.1/§8.2 已同步 M5 口径；08 只追加 §8.7 M5 注记（历史快照不改）。
- **M5 wave2 回写已完成（2026-10-30，REV-035 第二批）**：06/07/05/00/08/architecture 已按 REV-030/032 实际实现校准（replay L2、`llm_calls`、启动标定/`recovered[]`、e2e 结论）；REV-031（ISSUE-066）合并后由第三个提交补 06 §6.2.5.1 的 signal/exit 载荷与 08 注记。
- **M5 ISSUE-066 回写已完成（2026-10-30，REV-035 第三批）**：03 §3.15 / 06 §6.2.5.1 / architecture §8.2 已回写 `NODE_WORKER_CRASH` 的 `signal`/`exit_code`/`stderr_tail` 证据字段与边界；08 §8.7 记 ISSUE-066 fixed（REV-031）；台账状态由 Lead 收口。
- **REV-035 评审条件已关闭（2026-10-30）**：C1/C2/C3 + 建议已在同一分支追加提交（未改写评审记录）；等待 reviewer-a 复审翻「通过」。
- **M6 回写已完成（2026-10-31，REV-057）**：05/06/07/08 正文与 `architecture-v2.md` §5.2 已同步 M6 已实现口径（REV-040/041/042/043/044/047/048/049/050/052/055/056/058/060；REV-061 属 wrapup、仅用于 ISSUE-096 登记）；03 §3.12 与 04 A7/A17/§4.2 的双通道临时口径同步撤销；08 只追加 §8.8 M6 注记（8.0–8.7 历史快照不改）。**ISSUE-082（REV-048）与 ISSUE-088（REV-056）均已合并并回写**；ISSUE-095 保持 open（kernel hostname 口径差）、ISSUE-096 仍 pending（open，待 REV-061 wrapup 合并后置 fixed）；正文无未合并实现的 pending 占位。
- **W2a0 返工设计冻结已完成（2026-10-31，REV-067）**：00/01/02/03/04/06/07/08 已按 DR-005 冻结 deferrable gate（`on_gate_false`）、R15/`DSL_DEFER_NOT_REENTRANT`、`DeferredReceipt`/attempt-per-activation（per-port 继承 + `settled_by_node` 重置）、trace schema v4、AC-14；SOP 设计稿升 v1.1。**本批是设计冻结，不代表 W2a 实现完成**；实现偏差由 W2a-docs 回写。`doc/architecture-v2.md` 的 `R1–R14`/17 码口径未在本批同步（DR-005 §11 C-7），建议 W2a-docs/process 收口。
