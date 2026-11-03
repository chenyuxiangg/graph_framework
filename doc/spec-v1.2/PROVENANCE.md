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
    - **V5a e2e 实测回写（REV-101，2026-11-02）**：`tests/e2e/{skills_v03,mcp_v03,harness_v03,v03_regression}.rs` **31 用例**（AC-18 10 / AC-19 11 / AC-20 7 / 回归 3），四 target 连跑 3 次稳定；新语义（MCP 重启 spawn=2/`transport_closed`、截断 JSON+后缀、`http_insecure`、`llm_calls` 1 条/节点、Codex iterations=1/cached 丢弃/L2 不 spawn）已写入 07/05/08。e2e **137/0/2**、workspace **1037/0/3**、pytest **183**、harness **235/0/1**。
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
    - **来源**：已合并实现——dsl `fa6ea16`+`bc92951`（REV-069）、trace `27f9200`（REV-070）、py-worker `dd96d89`+`f685178`（REV-068）、kernel `ee4ca27`+cli `6c90a16`（REV-071）；e2e 已合并 `main` `e2cba52`（**REV-072 已通过**，含 N1 后 §7 + 单 item 加固 §8）。本批**按实际实现校准**，不重写 W2a0 冻结决策。
    - **07**：AC-14 资产路径改 `tests/fixtures/graphs/defer_gate/**` + `tests/fixtures/trace/{schema_v3_seed,v0.1.0_expected_trace,seed_graph,GENERATION}`；测试落点改 `tests/e2e/gate_defer.rs`（a/b/f/g/h 均通过）与 `tests/e2e/software_team_v1.rs`（c/e codegen 静态 + 运行期，REV-072 已通过）；e2e 已在 `main` `e2cba52` 合并，AC-14 c/e 以 07 的单 item profile（5 节点/7 边）口径为准；§7.1/§7.2 行同步。
    - **01/03/04/06 as-built 注记**：`DeferredReceipt.updated_at` RFC3339 `String`、审计字段当前无读取方；per-port 继承 + `settled_by_node` 清空、不回收继承端口结算；`apply_publish` 四路径/超限 GateResult 不入库/`fail_if_blocked_deferred` message；`GateEvaluation.verdict` 实现口径；DDL 三列表尾追加（列序无语义）、`verdict_json` 损坏**严格报错**（ISSUE-105 待定案）、迁移/只读拒绝实测。
    - **02 as-built 注记**：R15 首错 + `expected/actual` 与冻结模板一致（REV-069 N1/N5）；子图内 R13+R15 跨规则聚合 2 条 `DSL_SUBGRAPH_INVALID`（REV-069 N7，子图为例外）。
    - **08**：§8.9.1 追加 W2a 实现证据表（模块/commit/REV/测试计数/as-built 偏差/台账），不改 8.0–8.9 历史。
    - **DR-005 / DR-004**：DR-005 §7.5 R1–R5 修正（超限证据/§7 h L1 拆分/行 8→9/伪代码 3-4-8/commit 数）；AC-14 as-built 注；DR-004 §10 追加 W2a0+W2a 执行记录（不改原决策）。
    - **索引/架构**：`doc/index.md` reviews 范围补 REV-067–072（073 待评审）、DR-005/SOP 描述；`doc/architecture-v2.md` 只加 banner 注记指向 02 R15 / 00 §0.4（18 码），不改 D1–D12 历史。
    - **e2e 证据（REV-072 已通过；e2e 已合并 `main` `e2cba52`）**：`gate_defer` **8/0**、`software_team_v1` **11/1 ignored**；**c/e 运行期用 `--profile rework-item` 5/7**（hash as-built `79dd51c684ffc771`，合并后以实测为准），full 60/116 与 smoke 11/21 仅 validate/结构/hash；e2e **58/0/2**、空载 workspace **694/0/3**；**稳定性（REV-072 §8）**：e2e 连跑 **3 轮** 58/0/2；4 busy-loop 下 `software_team_v1` 连跑 3 轮 11/0/1；空载 workspace 694/0/3（首版 3-item 并发因 fake LLM 全局调用序交错 flake → 单 item 后确定）。07 AC-14 c/e/f/h 按实测校准：c（4 calls；W/R/P 0/1；前进/feedback 边真实 `deferred`/`verdict`；P1 消费 round2 review；archer 拿返工后最终产物）、e（victim 固定 `planer_specification`；W [0,1,2,3]、R/P [0,1,2]；forward 三轮 false+deferred；feedback 仅 attempt0/1 两行，超限那次求值不入库）、f（`verdict` 为原始路径值：`passed=false` 而 `verdict=true`）、h（seed 新 run `verdict="seed"`；defer 自环新库真实 bool/int；`max_attempts=1` 首轮即超限自环边 0 行 GateResult）。**ISSUE-104 → fixed**（e2e 分支 + REV-072）；full rework 60/116 运行期未覆盖 → **ISSUE-112**。
    - **台账**：新增 ISSUE-104（v3 seed；**已 fixed**）、ISSUE-105（`verdict_json` 口径）、ISSUE-106（trace 测试/守卫）、ISSUE-107（kernel N1–N7）、ISSUE-108（dsl N5–N8）、ISSUE-109（py-worker S1）、ISSUE-110（e2e L2/测试行数）、ISSUE-111（e2e trace fixture 路径归属）、ISSUE-112（full rework 运行期未覆盖）；ISSUE-100/101 → fixed；ISSUE-062 保持 open。
    - 本批次后按 §3 重新实测 SHA-256/行数（00–08 合计 **6385 行**）。

18. **W2b spec 回写批次**（2026-10-31，REV-077；分支 `docs/docs-fanout-spec`，base = 本地 `main` `9a91076`）：
    - **改动分册**：00/01/02/03/06/07（另 `doc/spec-v1.2/README.md` 行数表、`doc/index.md`、根 `README.md`、`doc/architecture-v2.md` 顶部注记、DR-004 §10、`doc/issues.md`、`doc/progress.md`）。
    - **来源**：已合并实现——dsl `1fa4cc0`（REV-074；评审记录 `220f53c`，合并链 `7b4aba1..220f53c`）、kernel+cli `a4cb1b1`（REV-075；含 `abc1e15`/`aa6bfd4`/`6f3ac2a`/`e37b1ad` 与 Lead 追加的 cli 跨模块第二评审）、e2e `9a91076`（REV-076：`06d6945` 实现 + `9a91076` 评审记录，fast-forward 合入）。本批**按实际实现 as-built 校准**，不改 W2a0/W2a 冻结语义。
    - **关键决策**：① **R16 复用 `DSL_PARSE_ERROR`**，DSL 域保持 **18 码**（不新增 `DSL_FANOUT_EXCEEDED`；静态扇出超限属图定义错误类型桶）；② 运行期层为**防御性兜底**（静态 R16 已保证加载期合法图 `actual <= out_degree <= max_fanout`；反例只能经 `test-hooks`/程序化构图构造，防御 W4 动态 N）；③ 有效上限 = `min(declared, test-hooks force)`，仅 `test-hooks` 构建生效；④ 超限**不产生该激活任何 GateResult、不应用任何边**（与 W2a0 超限"该次求值不入库"同序），实例 `Crashed(attempt=源+1)` + `run Failed`；⑤ 缺省无 `max_fanout` → 行为/hash/错误序列与 v0.1.0 逐字节一致。
    - **00**：banner 增 W2b；§0.3 `NodeError::FanoutExceeded{limit, actual}`；§0.4 运行期域新增 **`NODE_FANOUT_EXCEEDED`**（Crashed → run Failed）并注明 DSL 域仍 18 码（R16 复用）。
    - **02**：banner/状态改 R1–R16；§2.1.2/§2.1.3 加载流水线与 fail-fast 顺序改 `R1→R13→R15→R16→R14`；§2.2.1 增 `GraphNode.max_fanout` 行；§2.2.2 schema 增 `max_fanout?: number`；§2.2.3 S-2/S-6/S-9 补白名单/序列化/整数边界；§2.2.4 节点键白名单加 `max_fanout`；§2.3.3 自查表增 R16（不适用）；§2.4 标题/总表/模板/正反例/裁定新增 **R16 静态扇出上限**（`nodes` 序首错、`out_degree` 含 gate/defer 边、`DSL_PARSE_ERROR`、子图收敛 `DSL_SUBGRAPH_INVALID`、fixture 实测 `valid_max_fanout` hash `e05d882cc55aa275`、dsl 101 passed）；§2.4.4 增 D-02-13；§2.7.2/§2.7.3/§2.7.4 顺序/location/`--structure-only` 同步；§2.8.2/§2.8.3 hash 前置与 max_fanout 投影（仅显式存在、缺省不补齐）；§2.9.2/§2.9.3 编译映射；§2.11 增 P-02-10。
    - **03**：banner 增 W2b；§3.2 `TestHooks.force_max_fanout`；§3.4.2 应用入口补扇出校验注释；新增 **§3.4.4 每激活扇出上限**（`actual` 定义含 defer passed、`min(declared,force)`、应用任何边前失败、Crashed attempt+1、无 GateResult、冻结 message、确定性与 trace 证据、可达性/测试入口、as-built）；§3.15 矩阵补行。
    - **06**：banner 增 W2b；§6.2.5.1 canonical `NodeError` JSON 增 `kind:"FanoutExceeded"` 行（`limit`/`actual`，M5/W2b 例外注记）；DDL/迁移不变。
    - **07**：banner/通过标准改 AC-01..AC-15；§7.1 增扇出上限图资产行；§7.2 增 AC-15 行；新增 **AC-15 每节点扇出上限（W2b）**（a 静态反例 CLI 3 用例、b 正例真链路 1 用例、c 运行期 test-hooks 2 用例、d v0.1.0 golden 不变；含真实测试名与实测计数）；§7.3 #1 改 16 条（R1–R16）、#3 改 AC-15。
    - **索引/README/台账**：`doc/index.md`（reviews 范围至 REV-077、DR-004 §10 注记）、根 `README.md`（状态段改 W2b 已完成/下一步 W3a0、目录表 dsl/kernel/e2e 行）、`doc/decisions/DR-004-v02-scope-and-waves.md` §10 追加 W2b 执行记录（不改原决策）；`doc/issues.md` 登记 **ISSUE-113–120**（REV-074/075 P3）与 **ISSUE-121–124**（REV-076 P3）；`doc/progress.md` 更新（≤30 行）。
    - **测试证据（as-built）**：`cargo test -p zgraph-dsl` **101 passed**；`cargo test -p zgraph-kernel` **110 passed**（默认不编译 `tests/max_fanout.rs`）、`--features test-hooks` **120 passed**；`cargo test -p zgraph-cli` **161 passed**；`cargo test -p zgraph-e2e` **64 passed / 0 failed / 2 ignored**（`tests/e2e/max_fanout.rs` **6/0**，未新增 ignore）；空载 `cargo test --workspace` **725 passed / 0 failed / 3 ignored**；pytest **170 passed**；`cargo fmt --all -- --check`、`cargo clippy --workspace --all-targets --locked -- -D warnings`、`bash scripts/dev/check.sh` 全绿。v0.1.0 回归：REV-074 对 36 个 pre-W2a fixture 做 human/`--json` 双模式 A/B（72/72 输出+退出码逐字节一致）；无 `max_fanout` 图 hash 不变。
    - 本批次后按 §3 重新实测 SHA-256/行数（00–08 合计 **6491 行**；00 309→317、01 346→348、02 935→950、03 489→526、06 1117→1119、07 330→372；`doc/spec-v1.2/README.md` 行数表同步）。

19. **REV-077 条件修复批次**（2026-10-31，reviewer-b 复审条件 C1–C4 + P3；分支 `docs/docs-fanout-spec`，base = `ef090f2`）：
    - **C1（06 §6.10.2）**：`zgraph validate` 规则集改 **R1–R16**、执行顺序 `R1→R13→R15→R16→R14`、`--structure-only` 含 R16，与 02 §2.1.2/§2.7.4 一致。
    - **C2（02 §2.6.1 / 00 §0.4）**：§2.6.1「定义校验」与 as-built 注补 **R15/R16**（子图内违规收敛 `DSL_SUBGRAPH_INVALID` 并保留内部 `node_id`），与 §2.4.1 R6 行一致；00 §0.4 W2b 注记引用改为「主图见 02 §2.4.1 R16；子图递归含 R15/R16 见 02 §2.6.1 / §2.4.1 R6」，引用不再落空。
    - **C3（03 §3.4.4 / §3.15）**：trace 证据按 as-built 改写——源节点为 **Crashed 实例快照**（`instances` 行 upsert；as-built `create_instance(..., crash)` 只 emit `instance_snapshot`，CLI 桥接层快照只写 `instances`、**不产生** `transitions`/`events` 行）；证据 = `instances` 行 + `error` JSON（`kind`/`limit`/`actual`）+ `RunFinished.error`，不按 `transitions` 查询；§3.15 矩阵同口径。
    - **P3**：07 AC-15 d「13 个 target」→ **12**（`tests/e2e/Cargo.toml` `[[test]]`=12；64/0/2 计数不变）；03 §3.4.4 `actual` 来源措辞精确化（执行节点经 `NodeCompleted` off-loop、内置节点 `finish_builtin` in-loop）；`doc/architecture-v2.md` 顶部注记标题改 **W2a/W2b**；新增 **ISSUE-125**（03 §3.2 `TestHooks`/`RunOptions` 预存 as-built 偏差，登记后续 docs 批次校准，不改本批语义）；`doc/reviews/README.md` 补 REV-074/075/076/077 索引行与底部 **W2b** 收口段；`doc/index.md`/`doc/progress.md` 状态同步为最终态。
    - **重测与落盘**：§3 重算 00/02/03/06/07 的 SHA-256（行数不变，00–08 合计仍 **6491 行**）；`cargo test -p zgraph-e2e --test max_fanout` **6 passed / 0 failed**；`bash scripts/dev/check.sh` PASSED；提交正文含 `Review: REV-077` + `Agent: docs-dev`。

20. **W3a0 人工评审设计冻结批次**（2026-10-31，REV-078；分支 `docs/docs-human-design`，base = 本地 `main` `1ed37ab`）：
    - **改动分册**：00/01/02/03/04/06/07（另 `doc/decisions/DR-006-w3a-human-decide-freeze.md` 新增、`doc/index.md`、`doc/progress.md`、DR-004 §10、`doc/spec-v1.2/README.md` 行数表）。**本批只写设计/冻结口径，不写 as-built、不改代码**。
    - **新增 DR-006**（W3a 唯一语义依据，REV-078）：触发模型 `builtin.human` 保留 python_node 符号（不启动 worker/不 import）、`WaitingHuman` 生命周期与状态机、decide 自动接续（**不提供 `resume`**，W3b 再做）、拒绝矩阵（approve/reject 均正常 `Completed`，路由交给图与 W2a defer 返工）、`human_decisions` 表 + C13 写连接模型、trace **v5**（重建 `instances`/`transitions` CHECK + 新表 + 迁移/备份/只读拒绝/失败恢复）、kernel/CLI/trace API、退出码（新增 `EXIT_CONFLICT=5`）/`--json`、AC-16 a–g 设计与断言口径、非目标/风险/待确认项。
    - **00**：banner 增 W3a0；`InstanceStatus::WaitingHuman` 进入可产生集合；`HumanDecision` 枚举（Approved/Rejected）；§0.4 增 `HUMAN_*` CLI 命名空间注（不写 `runs.error_code`、无新领域码）；§0.5 增 `HUMAN_DECISION_POLL_MS=200`；§0.6 I1/I9 W3a0 解释；§0.7 两条消歧更新；§0.8 07 行 AC-16。
    - **01**：banner；`HandlerRef` 增 Human（内核对实现名 `CompiledHandler::Human`）；§1.2.3 矩阵增 WaitingHuman 行/列与 4 条迁移副作用；新增 **§1.2.4 人工等待与决策 output**；§1.5 增 `HumanDecisionRecord`（非 TraceEvent）；§1.6 deadlines 注释；§1.9 human 输出聚合注。
    - **02**：banner；§2.5.1 增保留符号 `builtin.human` 行；§2.5.4 后增 W3a0 保留符号冻结（精确匹配、R14 例外不 import/无 `NODE_WORKER_IMPORT`、与 gate canonical 无冲突、DSL schema/hash 不变）。
    - **03**：banner；新增 **§3.4.5 人工等待与决策应用**（状态/期限/决策算法/consumer/拒绝矩阵）；§3.6/§3.8 控制通道与 Paused decide；§3.9.1/§3.9.3 决策期限（缺省无期限、无 worker kill）；§3.10 `is_quiescent` 含 WaitingHuman；§3.15 补 5 行。
    - **04**：banner；§4.1.2 handler 表增 Human；§4.1.3 改写（W3a0 产生 WaitingHuman、仍无 resume；`NodeOutcome` 非 W3a 路径）；§4.1.4 human 无 handler NodeError 注；§4.4.1 映射表增行；新增 **§4.4.5 `builtin.human`**；§4.6 `manual` gate 与 human 无关注。
    - **06**：banner；schema **v5**（§6.2.2 版本表/§6.2.5/§6.2.7 CHECK 增 WaitingHuman；新增 **§6.2.14 `human_decisions`**）；§6.6 C13 写者模型；§6.7 v4→v5 迁移（备份/重建两表/新表/单事务/失败恢复/只读拒绝）与测试矩阵；§6.10.1 五子命令；§6.10.4 trace 决策摘要/NDJSON；§6.10.5 decide 退出码（含 5）；§6.10.6 decide/HumanDecision JSON；新增 **§6.10.8 `zgraph decide`**。
    - **07**：banner/通过标准 AC-01..AC-16；§7.1 人工评审图资产（W3a 交付，设计）；§7.2 增 AC-16 行；新增 **AC-16 a–g（设计冻结：approve 真链路 / reject+W2a 返工 / 冲突幂等退出码 / 决策期限 / v4→v5 迁移与 v3→v5 链回放 / decide×run 并发 / 默认 golden 不变）**；§7.3 #3 改 AC-16。**测试名/计数由 W3a 实现后 W3a-docs 回写**（本批不写 as-built）。
    - **关键裁决**：① human 不改 DSL schema（保留符号，无新 NodeType/领域错误码）；② `WaitingHuman` 为非静默等待态（`is_quiescent` 显式含之）；③ 决策为一次性完成（不重跑；W2a 重入产生新实例）；④ 决策期限 = `timeout_ms`，缺省**无期限**（显式例外）；⑤ trace v5 重建两表（SQLite CHECK 不可 ALTER）；⑥ `HUMAN_*` 仅 CLI 命名空间。
    - **Lead 2026-10-31 裁决（DR-006 §12.2 E-11 / §12.3）**：决策期限缺省无期限、Paused 期间 decide 不自动 Resume、`trace --json` 追加 HumanDecision 行、`builtin.human` 保留符号形态均采纳；E-11 按「instance 不存在 → exit 1（`HUMAN_INSTANCE_NOT_FOUND`）、存在但非 WaitingHuman → exit 5（`HUMAN_INSTANCE_NOT_WAITING`）」执行。
    - 本批次后按 §3 重新实测 SHA-256/行数（00–08 合计 **6795 行**；00 317→330、01 348→390、02 950→958、03 526→592、04 906→933、06 1119→1219、07 372→420；`doc/spec-v1.2/README.md` 行数表同步）。

21. **REV-078 条件修复批次**（2026-10-31，reviewer-b 首轮「有条件通过」C1–C5；分支 `docs/docs-human-design`，base = 评审记录 `a325fd3`）：
    - **C1 副作用顺序**：`WaitingHuman→Completed` 冻结为「**先写 output + 清 deadline → transition(Completed，快照携带 output) → publish**」，明确「不得先 transition 后写 output」；DR-006 §3.2、01 §1.2.3、03 §3.4.5 三处统一（与 as-built `on_completed`/`finish_builtin` 同序，保证 `instances.output` 非 NULL、AC-16 a 可断言）。
    - **C2 consumer 行为**：DR-006 §7.3 与 03 §3.4.5 统一为「**consumer 对所有 pending 行总是提交 `RunControl::HumanDecision`（不按 bridge WaitingHuman 集合过滤）**」；kernel 幂等校验（非当前/非 WaitingHuman → `applied=false`），CLI stderr 警告并仍标 `consumed_at`（无悬挂行）。
    - **C3 预检顺序/码**：decide 预检冻结为「① 已有决策行 → **5 `HUMAN_DECISION_EXISTS`（先于一切状态判断）**；② run 不存在/已终态 → 1 `HUMAN_RUN_NOT_ACTIVE`；③ instance 不存在 → 1 `HUMAN_INSTANCE_NOT_FOUND`；④ status≠WaitingHuman → 5 `HUMAN_INSTANCE_NOT_WAITING`」；DR-006 §4.4/§4.5、06 §6.10.5/§6.10.8、07 AC-16 c 同步（二次 decide 恒 DECISION_EXISTS，无时序 flake）。
    - **C4 文档事实/一致性**：PROVENANCE §3 合计 **6795**、增量修正（冻结批次 +349/−45，净 +304）；spec README 验收改 AC-01..AC-16；06 §6.10.1 `--db` 增 decide；03 §3.8 表格恢复 + `Cancel` 行补 `WaitingHuman→Cancelled`；index DR-004 行含 W3a0；PROVENANCE §5 笔误删除。
    - **C5 设计补全**：`reject_reason` 冻结三值 `instance_missing`/`not_current`/`not_waiting_human`（内核 snake_case，与 decide `HUMAN_*` 两套码）；`instances.input` 冻结为「`Pending→WaitingHuman` 迁移前写入全端口快照」；AC-16 补 reject→终止（rejected settle 边→end）、decide exit 4（DB 不存在/损坏/`<5`）、human 图 validate、kernel `applied=false`/`reject_reason`、trace 关闭/库 API（`RunControl` 直驱、sink no-op）、decide 可见性（轮询 DB WaitingHuman）、含决策 run 的 `replay --mode=events` ≡ `trace --json`、既有 e2e `expected 4→5` 实施前置项；DR-006 新增 E-12/E-13/E-14，§14.1 文件清单校正。
    - **台账**：新增 **ISSUE-126**（applied/reject_reason 持久化评估，W3b）、**ISSUE-127**（handler 命名实现侧最终统一，W3a）、**ISSUE-128**（trace `--stats` human 计数后续可选）；既有 113–125 不动。
    - **重测与落盘**：PROVENANCE §3 重算 00/01/02/03/04/06/07 的 SHA-256 与行数（冻结 6795 → 条件修复后 `90fa845` **6814 行**，+19；spec 文件 numstat **+59/−40**（净 +19））；`bash scripts/dev/check.sh` PASSED；提交正文 `Review: REV-078` + `Agent: docs-dev`。
    - **R1–R4 复审残留修复（2026-10-31，复审通过后合并前小 commit）**：
      - **R1**：删除 E-12 不可达的 CLI `--no-trace`/等价分支（CLI 不提供该参数、W3a 也不新增），只保留**库 API 口径**（`trace_enabled=false` 无 DB 依赖，调用方直接 `RunControl::HumanDecision`、sink 可 no-op）；新增 **human 图 + CLI `--db :memory:` → run 启动前预检拒绝 `RUN_ENTRY_INVALID`（exit 3、不产生 run、reason `trace_required_for_human`，不新增错误码）**，并在 00 §0.4 `RUN_ENTRY_INVALID` 触发条件、03 §3.2 启动校验表行 1b、06 §6.9.1/§6.10.3 `:memory:` 注记三处写明；E-12 标注「W3a 实施前置项」。
      - **R2**：DR §4.1 流程图改为「先写 output（快照携带）→ transition → publish」，与 §3.2 冻结顺序一致。
      - **R3**：`--stats` 引用 ISSUE-127 → **ISSUE-128**（DR/06）；第 21 条重算清单补 **04**；条件修复增量按实测改为 **+59/−40（净 +19）**。
      - **R4**：index/progress/DR-004/DR-006 §14.3/PROVENANCE 的「待复审」改为「复审通过（REV-078，`05a5803`）」。
      - **小 commit 后** 00–08 合计 **6815 行**（00 330、01 393、02 959、03 596、04 933、05 1429、06 1226、07 425、08 524；相对 `05a5803` spec numstat **+12/−11，净 +1**）；含 README/PROVENANCE/REFERENCES 合计以 `wc -l` 为准（当前 7151 行）。
    - 条件修复后（`90fa845`）00–08 合计 **6814 行**（03 595；其余同 R 修复后除 03 外）；含 README/PROVENANCE/REFERENCES 合计以 `wc -l` 为准。

22. **W3a 实现回写批次**（2026-11-01，REV-084；分支 `docs/docs-human-spec`，base = 本地 `main` `2444548`）：
    - **来源（已合并实现）**：trace `4cae6b9`（REV-079；合并 `adf54a1`）、kernel `ab4b805`+`72036b6`（REV-080；合并 `390dbe1`）、cli `4d671a0`+`c711232`（REV-081；合并 `971e48f`）、cli fix `c3022ea`（REV-082；合并 `9006345`）、e2e `05383d0`+`5128f53`+`2ac54c2`（REV-083；合并 `2444548`）。本批**按实际实现校准 as-built**，不改 W3a0 冻结语义；无 human 图行为/hash/trace 不变。
    - **改动分册**：00/01/03/04/06/07/08（另 `doc/decisions/DR-006...md` §15、`doc/spec-v1.2/README.md` 行数表、`doc/reviews/README.md`、`doc/index.md`、`doc/progress.md`、`doc/issues.md`、DR-004 §10）。
    - **00**：banner + as-built——`WaitingHuman` 实际产生；`HUMAN_DECISION_POLL_MS=200` 落地；`zgraph decide` 实现、`HUMAN_*` 仍只作 decide `--json.code`（不写 `runs.error_code`）；`RunControl` 新 variant（Copy→Clone，source-breaking 见 08 §8.10）。
    - **01**：banner + as-built——状态机 **9 状态/13 合法迁移**（`state_machine` 9×9）；`Pending→WaitingHuman` 旁路与迁移前 `instances.input` 全端口快照、`human_waiting` reason、`declared_timeout_ms` 才登记 deadline；`HumanDecisionRecord` 9 字段、sink 默认 no-op/Emitter 串行化；`human_wait.rs` 9 用例断言 input/迁移/无 worker/无 Running。
    - **03**：banner + §3.2/§3.4.5 as-built——决策顺序（写 output+清 deadline → transition 快照 → publish → `human_decided`）；consumer 拓扑（`DecisionSession`/`decision_consumer_loop`/`submit_human_decisions`/`HumanDecisionObserver`）与按需启动、`mark_consumed` 幂等、退出 flush、`--quiet`/`:memory:` warning 口径；`applied=false` 三值；迟到决策仍 consumed；Paused decide 不派发；期限 `started_instant`/Paused 扫描/无 worker kill；E-6 确认丢弃边界；`force_node_timeout_ms` 对 human 不生效。
    - **04**：banner + §4.4.5 as-built——`builtin.human` 精确映射 `CompiledHandler::Human`（`compile.rs` 双反向断言）；其它 `builtin.*` 仍 Python；L2 `replay --mode=outputs` 的 human handler 替换（clone 进程内图 → `RecordedExecutor` 返回 `(node_id,attempt)` 记录；不改 graph_hash/dsl/events；缺失记录预检 exit3）。
    - **06**：banner + as-built——schema v5（`TRACE_SCHEMA_VERSION=5`、v4→v5 重建两表/新表+pending 索引/备份/单事务/失败恢复；v1–v4→v5 链；只读 `expected 5`；旧行逐列一致；trace 79/0，迁移测试 15，评审探针 27/27）；`DecisionError::{Conflict,Trace}` 独立、`decision: String`、C13 写者；`trace --json` 决策行（不受过滤/键序非契约）；`zgraph decide` 退出码/不建库/不迁移/Paused；human+`:memory:` 启动前拒绝。
    - **07**：banner + **AC-16 a–g 改为实测**——`tests/e2e/human_decide.rs` **21 用例**（a 4 / b 3 / c 1 / d 1 / e 3 / f 3 / g 3 + 静态 3；测试名逐一回写），e2e **85/0/2**（human_decide 21/0，连跑 3 次稳定）、workspace **819/0/3**；v3→v5 链/v4→v5 迁移/L2 human outputs/`:memory:` 拒绝/golden 不变均有实测断言；W1 SOP 不变由既有 AC-13/AC-14 套件覆盖。
    - **08**：新增 **§8.10 W3a 注记**（实现证据表 + source-breaking 兼容备注 + as-built 偏差/遗留）；§8.7 兼容备注位追加指引——不重写 8.0–8.9 历史快照。
    - **DR-006**：追加 **§15 W3a 实现执行记录**（分支/提交/评审/合并点/测试、as-built 要点、偏差 D1–D9、遗留建议）。
    - **台账/索引**：`doc/issues.md` 新增 **ISSUE-129–140**（REV-079/080/081/082/083 未修 P3，含 ISSUE-137 P2 trace FK flake 环境观察、非 W3a 引入）；`doc/reviews/README.md` 补 REV-079–083 索引行 + **REV-084（通过，`d5df1e8`）** + W3a 收口段；`doc/index.md`/`doc/progress.md` 状态同步（W3a 完成、下一步 W4a；W3b 可选需用户确认）。
    - **测试证据（as-built）**：trace **79/0**；kernel **122/0**、`--features test-hooks` **132/0**；cli **206/0**；e2e **85/0/2**（human_decide **21/0**，3 轮稳定）；空载 workspace **819/0/3**；pytest **170**；`cargo clippy --workspace --all-targets --locked -- -D warnings`、`cargo fmt --all -- --check`、`bash scripts/dev/check.sh` 全绿。
    - 本批次后按 §3 重新实测 SHA-256/行数（00–08 合计 **6886 行**；**00–08 numstat +103/−32（净 +71）**；**全 spec 目录 numstat +138/−50（净 +88，`git diff --numstat main -- doc/spec-v1.2`）**；含 README/PROVENANCE/REFERENCES 合计以 `wc -l` 为准（当前 7239 行）。

23. **W4a 动态 N / foreach 设计冻结批次**（2026-11-01，REV-086；分支 `docs/docs-dynamic-fanout-design`，base = 本地 `main` `559ede6`；docs-only）：
    - **来源/性质**：用户 **2026-11-01 决定 W3b 不做**（durable resume 不纳入 v0.2；记录于 DR-004 §10）；本批为 **W4b 实现前的设计冻结**，不写 as-built、不改代码；语义冻结见新增 [`DR-007-w4a-dynamic-fanout-freeze.md`](../decisions/DR-007-w4a-dynamic-fanout-freeze.md)。
    - **02-DSL与校验**：`GraphNode.foreach`（仅 `python_node`/`agent_call`，含 `builtin.human`；子图内禁止）：`items_port/key_field/join/max_items/on_item_error` 闭集/白名单/serde/hash 投影；**R17** 形状（复用 `DSL_PARSE_ERROR`，R4 阶段）+ **R18** 区域（新 DSL 码 **`DSL_FOREACH_INVALID`**，DSL 域 **18→19**，reason 闭集 11 值，载荷/定位冻结）；执行顺序 `R1→R13→R15→R18→R16→R14`；正反例/模板/hash 兼容（无 foreach 图 hash 逐字节不变）。
    - **01-领域模型与状态机**：`CompiledNode.foreach`、`InstanceRecord.item_index/item_key`、item 实例定位键 `(node_id,item_index)`、attempt/defer 按 item 作用域、`RunState.controller_buffers`（无 controller 实例）、join 有序数组/空列表（§1.2.5/§1.9）；非 region `item_index=0`。
    - **03-调度器与生命周期**：新增 **§3.4.6 foreach 展开与 join**（controller buffer 就绪→展开；F 自身 per-item 执行；region 内同 item 投递；join 按 item_index 升序；空列表立即 `[]`；4 个 `RUN_FOREACH_*`；取消/Pause/超时/上限）；§3.5/§3.8/§3.9.1/§3.10/§3.15 同步。
    - **06-存储与可观测**：trace schema **v6**——`instances` 增 `item_index`/`item_key` 并重建唯一键 `(run_id,node_id,attempt,item_index)`；新表 **`foreach_expansions`**（+run_id 索引）；v5→v6 备份/单事务/12 步重建/只读拒绝；v1..v5→v6 链；`trace` 追加 ForeachExpansion 行（不写 `events.kind`）；L2 查找键扩展 `(node_id,attempt,item_index)`（`recorded_outputs` 结构不改）；`--stats` 不新增字段。
    - **07-验收用例**：新增 **AC-17 a–j（设计冻结；W4b 实测）** 并加入 §7.1/§7.2/DoD；通过标准改 **AC-01..AC-17**；§7.3 校验数改 **18 条（R1–R18）**。
    - **00-总纲**：`DSL_FOREACH_INVALID`（第 19 码）+ 4 个 `RUN_FOREACH_*` 运行期码、`ItemIndex`/`ExpansionId` 别名、W4a 设计注记（item 实例/I1）；**04-节点接口**：foreach region 内 human per-item 最小注记。
    - **SOP 设计稿 v1.2**：新增 **dynamic-N profile**（foreach 替代按 N codegen；per-item W/R/E 链 + join；静态 v1/v1.1 兼容；§7.3 codegen 规则；§9 移除「动态 N」；§11.3 验收）；只写设计。
    - **DR-004 §10**：追加「用户 2026-11-01 决定 W3b 不做」+ W4a 执行记录；`doc/index.md` 新增 DR-007 行、reviews 范围至 REV-086；`doc/progress.md`（进行中 W4a、下一步 W4b、W3b 不做）。
    - **hash/迁移红线**：无 foreach 图 hash/validate/run/trace/回放逐字节不变；`events.kind` 不变；既有 DSL 18 码 + R1–R16 不变（仅新增第 19 码与 R17/R18）；不修改 `doc/reviews/**` 既有记录、不改代码。
    - **重测与落盘**：`bash scripts/dev/check.sh` PASSED；PROVENANCE §3 按 §3 重算（00–08 合计 **7110 行**；00–08 numstat **+245/−21（净 +224）**；全 spec 目录 numstat **281/41（净 240）**）；spec README 行数表同步（00 343/01 421/02 984/03 675/04 939/06 1271/07 484）。提交正文 `Review: REV-086` + `Agent: docs-dev`；本批不 push/不 merge。

24. **REV-086 条件修复 + P2/P3 批次**（2026-11-01，REV-086 复审条件 C1–C5；分支 `docs/docs-dynamic-fanout-design`，base = 评审记录 `703ea07`；docs-only）：
    - **C1（R18 F 入边边界）**：R18 增加「**F 的全部入边来源 ∉ R**」（reason=`external_edge`；row8/§4 伪码/02 R18）；新增反例 **N-13**；明确 controller buffer 只接收 R 外投递（03 §3.4.6）。
    - **C2（join 来源/GateResult）**：**N>0** 冻结 `Delivery.from_instance`/`GateResultRecord.instance_id`/`attempt` = **最后完成使 join_buffer 达到 N 的 E item 实例**；**N=0** 无 E item 实例 → **不产生 GateResult/Delivery**，controller 直接注入 `[]` 到 J 输入 buffer（`instances.input` 可见 `[]`，trace 证据=expansion(count=0)+J transition，无 GateResult 行）；DR §6.5/03 §3.4.6/03 §3.15/06 §6.2.15/07 AC-17 c 同步。
    - **C3（controller 生命周期/终止）**：冻结 `ControllerState {Collecting,Ready,Expanded,Skipped}` 与 `ControllerBuffer`/`JoinBuffer` 类型（01 §1.6.1）；未就绪/部分到达/全部 settle=false/items_port 未到/defer→F/Skipped 的展开/skip/`RUN_DEADLOCK` 语义（DR §6.1/§6.6/§6.7；03 §3.4.6/§3.10）；`is_quiescent` 含 Collecting/Ready、Expanded/Skipped 不算活跃；展开后投递→`RUN_DELIVERY_TO_TERMINAL`。
    - **C4（Lead 4 项裁决落盘）**：DR-007 §15.3 改为「已确认（Lead 2026-11-01 裁决）」表（① canonical ② max_items 无上限 ③ join 可 end ④ trigger=any）；§6.2 canonical 段改为单一冻结陈述（去掉自问自答/「非法」矛盾）；DR-004 §10 旧「W3b 可选」行加「已被 2026-11-01 决定取代」注记（保留历史）。
    - **C5（AC-17 补全）**：新增 **AC-17 j**（E Skipped → `RUN_FOREACH_ITEM_INCOMPLETE` Failed/无 Crashed/其余 item Cancelled/无 join 投递）+ **key canonical 断言**（number/bool/string/null·object→`{"$index":i}`/非 NULL 重复）；07 DoD 明确覆盖 4 个 `RUN_FOREACH_*` + 5 类边界，并限定「无 foreach 图」的既有 DSL 18 码/R1–R16 不变。
    - **P2/P3**：修复 00/01 代码块/表格拼接（N6）；恢复 02 W2b 历史 banner 原文（只追加，N7）；明确子图内 foreach 归 **R6**、R17 不负责子图（N8）；`ControllerBuffer`/`join_buffer` 类型入 01（N9）；`keys_json` NULL 键改结构化 `{"$index": i}`（避免与真实 string key 撞表示，N10）；README 00/02 行改 19 码/R1–R18、index SOP 行改 v1.2 三 profile（N11）；**ISSUE-141** 登记 `max_items` 缺省无上限风险（N12）；DR-004 旧行标注（N13）；SOP §9「三项」/§7.1「静态两 profile 共用」（N14）；R18 row4 先 `forbidden_node_type` 后 `nested_foreach`、README 19 码起句、07 DoD 限定（N15）；06 迁移链改 v1→…→v6（含 v5→v6，N16）；DR §6.2 自问自答清理（N17）；`source_instance_id` 冻结为「最后使 controller Ready 的投递来源，entry=NULL」（N18）；AC-17 补 controller/来源/边界断言（N19）；index DR-004 行补 W4a/W3b 状态（N20）。
    - **重测与落盘**：`bash scripts/dev/check.sh` PASSED；PROVENANCE §3 重算（00–08 合计 **7154 行**；00–08 numstat **285/17（净 268）**；全 spec 目录 numstat **330/37（净 293）**；spec 目录合计 **7532 行**）；spec README 行数表同步（00/01/02/03/04/06/07/08）。提交正文 `Review: REV-086` + `Agent: docs-dev`；不 push/不 merge。

25. **W4b 实现回写批次**（2026-11-01，REV-094；分支 `docs/docs-dynamic-spec`，base = 本地 `main` `201341b`；docs-only）：
    - **来源（已合并实现）**：dsl `9efc41c`（REV-088；合并 `45cd7c6`）、trace `93a5f23`+`e057cef`（REV-089；合并 `948d677`）、kernel `53b42ca`+`211c2be`+`f6e8ed9`（REV-090；合并 `519066d`）、cli `59c7443`（REV-091；合并 `4a1ff0f`）、fix `8846240`（REV-092；合并 `683260b`）、e2e `a82943d`+`e1fdcd1`（REV-093；合并 `201341b`）。本批按实际实现校准 as-built，不改 W4a 冻结语义；无 foreach 图行为/hash/validate/run/trace/回放逐字节不变。
    - **重点 as-built（items_port 取值规则 A，REV-092）**：`v=controller.values[items_port]`；array 直用；**object 且含与 items_port 同名键 → 取该键值（必须 array）**；否则 `RUN_FOREACH_ITEMS_NOT_ARRAY`；只取一层、不点分（v1 整包广播：entry/worker/agent_call 均为 object）；NOT_ARRAY→EXCEEDED→DUPLICATE 顺序作用于 unwrap 后列表（03 §3.4.6、DR-007 §14.1）。
    - **00/01/02/03/04**：W4b banner + as-built——DSL 19 码/R17/R18 已实现（reason 闭集 11 值，`region_overlap` 防御性不可达；子图内 foreach 归 R6）；`CompiledNode.foreach`/`InstanceRecord.item_index/item_key`/`ControllerBuffer`/`ControllerState`/`JoinBuffer` 落地；controller Pause/Resume Ready 展开修复（REV-090 C1）、`source_instance_id` 仅 Collecting 记录、Expanded/Skipped 后投递 → `RUN_DELIVERY_TO_TERMINAL`、`incomplete_join` 防御；冷峻失败只取消失效中 item、已 Completed 保留；region 内 human 每 item 独立 decide。
    - **06**：schema v6 实测（trace **94/0**）：instances item 列+四列唯一键、`foreach_expansions` 表/索引、writer/reader；**v5→v6 FK CASCADE 窗口**（事务外 OFF→重建→恢复 ON，否则 human_decisions 被 CASCADE 删）；**真实 v1→v6 迁移**（v6-only item 索引移出 `DDL_CURRENT`；**ISSUE-142 P1 fixed**、REV-089）；`trace` 人类摘要+`source_instance/at` 明细、`--json` 含 `source_instance_id`（null 不省略）、行序 events→HumanDecision→ForeachExpansion、决策/展开行不受过滤；L2 `RecordKey=(node_id,attempt,item_index)`、expansion 预检 CORRUPT/MISSING（region 有实例缺行）/Skipped 合法。
    - **07**：**AC-17 a–j 设计冻结 → 实测**——`tests/e2e/dynamic_fanout.rs` **21 用例**（静态 3 + 迁移/i 3 + 默认 2 + a–j 13，逐一列名）；e2e **106/0/2**（dynamic_fanout **21/0**，6 连跑）、workspace **915/0/3**、全量 e2e 3 连跑稳定；d/j 终态语义、region_overlap 分工、v5 seed 口径已注明。
    - **08**：新增 **§8.11 W4b 注记**（实现证据表 + source-breaking 清单：`InstanceRow`/`InstanceSnapshot` item 字段、`ForeachExpansionRecord`/`TraceInput::ForeachExpansion`、`DSL_FOREACH_INVALID` 19 码、4 个 `RUN_FOREACH_*`、`CompiledNode.foreach`、`ControllerBuffer`/`JoinBuffer`；`InstanceStatus` 无新增）+ as-built 偏差/遗留。
    - **DR-007 §14.1 执行记录**（分支/提交/REV/合并点/测试、偏差 A–E、遗留）；**DR-004 §10** 追加 W4b 执行记录；SOP 设计稿追加 W4b as-built 注记（只追加）。
    - **台账/索引**：`doc/issues.md` 新增 **ISSUE-142（fixed）–ISSUE-148**（真实 v1→v6 缺陷/CLI·e2e·dsl 残余 P3）；`doc/reviews/README.md` 补 REV-088–093 索引行 + REV-094（本分支待评审）；`doc/index.md`/`doc/progress.md` 状态同步（W4b 完成、下一步发布收口）。
    - **测试证据**：dsl **118/0**；trace **94/0**；kernel **149/0**、test-hooks **159/0**；cli **222/0**；e2e **106/0/2**（dynamic_fanout **21/0**，6 连跑）；workspace **915/0/3**；clippy/fmt/check 全绿。
    - 本批次后按 §3 重新实测 SHA-256/行数（00–08 合计 **7198 行**；00–08 numstat **76/32（净 44）**；全 spec 目录 numstat **106/50（净 56）**；含 README/PROVENANCE/REFERENCES 合计以 `wc -l` 为准（当前 7588 行））。

26. **v0.3（M8）实现回写批次**（2026-11-02，REV-101；分支 `docs/docs-v03-spec`，base = 本地 `main` `6515f12`；docs-only）：
    - **来源（已合并实现）**：V2 skills `feat/harness-skills`（`cfe6280`+`f286fe8`+`6844573`，REV-097；合并 `69aeb87`）→ V3 MCP `feat/harness-mcp`（`18bf405`+`815abea`+`15f3d18`+`23fa568`，REV-098；合并 `23fa568`）→ V4 harness 插件 `feat/harness-plugin`（`0584931`+`3bbe4b5`，REV-099；合并 `6515f12`）。本批按实际实现校准 as-built，不改 V1 冻结语义；无 skills/MCP/外部 harness 默认路径逐字节不变。
    - **00/02**：`HarnessErrorKind` 3 kind（`SkillInvalid`/`Mcp`/`Plugin`）与单码 `HARNESS_SKILL_INVALID`/`HARNESS_MCP_ERROR`/`HARNESS_PLUGIN_ERROR` 已落地；§0.5 补 V2–V4 实际常量（MCP/skills/plugin 全量）；DSL 仍 1.0/R1–R18/19 码；skills/MCP/harness 不参与 DSL/hash（as-built 确认）。
    - **05 §5.14.4**：skills/MCP/plugin 实现分支/评审/测试；V4 偏差——`AgentProvider::run→RunFailure`、`AgentProviderKind` 仅 Builtin|Subprocess、subprocess `protocol` 必填、reason 集合实现口径、能力冲突仅节点级 `max_iterations`、同 run 同 provider mutex 串行、`llm_calls` 每节点 1 条；`SKILL_BODY_MAX_CHARS` 字符口径等。
    - **06 §6.14.1**：trace 仍 **v6（不 v7）**；MCP/skill 复用既有 `streaming_events`/`llm_calls`；外部 harness 事件同 sink、不双写；默认 trace/replay 不变。
    - **07**：AC-18/19/20 实现已合并（V2/V3/V4）；**V5a 实测（REV-101）：31 用例**（AC-18 10/AC-19 11/AC-20 7/回归 3），用例名/计数/DoD 已回写；e2e 137/0/2、workspace 1037/0/3、pytest 183、harness 235/0/1。
    - **08 §8.12.1**：3 kind/码、`Subprocess` enum、py-worker adapters、`rmcp` feature-gated、trace 不 v7 的 source-breaking as-built 确认。
    - **DR 执行记录**：DR-004 §10 + DR-008 §9 + DR-009 §11 + DR-010 §12 + DR-011 §13（分支/提交/REV/合并点/偏差/已知限制，只追加）。
    - **测试证据（已实测）**：harness **235/0/1**（`--features mcp` **235/0/1**）、e2e **137/0/2**、workspace **1037/0/3**、pytest **183**；`--no-default-features` 构建通过；V5a AC 31 用例已回写。
    - **台账/索引**：`doc/issues.md` 新增 **ISSUE-156–163**（V2/V3/V4 残余 P2/P3 + 真实环境验证）；`doc/index.md`/`README.md`/`doc/progress.md` 状态同步（v0.3.0 发布收口）；reviews README REV-097–102 由评审人补。
    - 本批次后按 §3 重新实测 SHA-256/行数（00–08 合计 **7419 行**；00–08 numstat **109/40（净 69）**；全 spec 目录 numstat **135/53（净 82）**；含 README/PROVENANCE/REFERENCES 合计以 `wc -l` 为准（当前 7822 行））。

27. **v0.4（M9）实现回写批次**（2026-11-03，REV-113；分支 `chore/process-v04-release`，base = 本地 `main` `8897053`；docs-only + 版本 bump）：
    - **来源（已合并实现）**：D2 files `b8ab337`+`4b093fc`（REV-106）→ D3 search `5b4e899`（REV-107）→ D4 bash `60aff67`+`31a4593`（REV-108）→ D5a trace v7 `ea6f657`+`90b6738`（REV-109）→ D5b1 record `443ca3d`+`c035637`（REV-110）→ D5b2 CLI replay `e197584`+`d4da442`（REV-111）→ D5c e2e `598ccff`+`7530bec`+`0a47295`（REV-112）；合并 `8897053`。
    - **00/02**：v0.4 as-built banner；§0.4 reason/CLI 码闭集（sandbox_*/builtin_*/path_*/invalid_*/REPLAY_TOOL_*，零新 kind/顶层码）；§0.5 V2–V4 常量 as-built（read/write/search/glob/bash/tool_calls）；DSL 仍 1.0/R1–R18/19 码、不参与 hash。
    - **05 §5.15**：8 内置工具最终语义/opt-in/工具源冲突/jail/OS 沙箱 profile/预检 exit3/嵌套 fail-closed/错误面/结果上限/环境限制（as-built）。
    - **06 §6.15**：schema v7 `tool_calls` 最终 DDL（含 `error_sha256`/`bash_signal`/`sandbox_profile_sha256`）、排序 `(started_at, call_id)`、`TraceInput::ToolCall` 非 events、trace 人类/NDJSON `{"kind":"ToolCall"}` additive、`--stats` 仅>0、`replay --mode=tools` recorded/reexec exit2/codes/无副作用、迁移。
    - **07**：AC-21–24 + 回归 **29 用例/5 target** 实测（D5c 28 + REV-112 补 1；e2e 166=137+29，23 target、2 ignored）；AC-23 嵌套 fail-closed 实测 + 用户终端脚本分工；AC-24 f 由 D4 单测/终端脚本覆盖。
    - **08 §8.13**：final source-breaking（sha2/similar/ignore/globset/regex 依赖；CLI flags；v7；reasons/CLI 码；`--replay-tools`；无新 kind；错误码闭集只追加）。
    - **版本/发布**：Cargo.toml/lock `0.4.0`；release `--locked`+`--version`=`zgraph 0.4.0 (DSL 1.0)`；性能 [`perf-baseline-v0.4.txt`](../perf-baseline-v0.4.txt)（5-trial 门禁通过）；产物/标签待发布流程（用户授权）。
    - **测试证据**：harness **340/0/1**、workspace **1216/0/3**、e2e **166/0/2（23 target）**、pytest **183**；AC-21–24 29 用例。
    - **台账**：issues ISSUE-176–181（kernel_cancel/ac09 parallel/codex adapter flake、trace 无 FK 清理、真实终端沙箱证据、v0.3 残余复核）。
    - 本批次后按 §3 重新实测 SHA-256/行数（00–08 合计 **7640 行**；00–08 numstat **161/129（净 32）**；全 spec 目录 numstat **186/142（净 44）**；含 README/PROVENANCE/REFERENCES 合计以 `wc -l` 为准（当前 8055 行））。

28. **v0.5（M10）实现回写批次**（2026-11-04，REV-131；分支 `chore/process-v05-release`，base = 本地 `main` `071f29e`；docs-only + 版本 bump）：
    - **来源（已合并实现）**：F1a `1d3c6a0`、F1b `26adfab`、F1c `d49a705`（REV-116/118/121）；F2 `4f99f1b`+`7b0387d`（REV-120）；F3a1 `69216e1`、F3a2 `c1744aa`+`16f6a7d`、F3b `57e3a60`+`4a1676c`/`3d30abb`（REV-123/126/127）；F4 `e9706b0`/`843e3be`（REV-124/125）；F5a `7668fdf`+`fe8dcce`、F5e `4533d44`、F5d `ae5662d`（REV-128/129/130）；合并 `071f29e`。
    - **00/02**：v0.5 as-built banner；`BUDGET_EXCEEDED` 域码 + reason 闭集（`budget_invalid`/`allow_write_path_invalid`/`cost_unavailable_external_harness` 等）；DSL 仍 1.0/19 码、不参与 hash。
    - **05 §5.16**：reexec 最终（候选/非候选/输出/错误码）、预算/成本最终、sandbox 增强最终、DSH ACP transport + Codex FINAL 验证/F5a–f（as-built）。
    - **06 §6.16**：reexec recorded/reexec、`replay_mode`/`drift` additive、MISSING/TRUNCATED/MISMATCH 区分（行缺失 vs 字段损坏）、预算 metrics/stop_reason；trace v7 不变、不 v8。
    - **07**：AC-25（reexec e2e 7）、AC-26（sandbox 单测）、AC-27（预算/观测 9 CLI + harness）、AC-28（DSH 16/Codex mock 3；F5c/F5f 门控）as-built；baseline 25 target/174/0/5。
    - **08 §8.14**：final source-breaking（CLI flags `--replay-tools/--pricing/--max-tokens/--max-cost/--allow-write-path`、`BUDGET_EXCEEDED`、reasons、reexec additive、无 v8）。
    - **版本/发布**：Cargo.toml/lock `0.5.0`；release `--locked`+`--version`=`zgraph 0.5.0 (DSL 1.0)`；性能 [`perf-baseline-v0.5.txt`](../perf-baseline-v0.5.txt)（7-trial 门禁通过）；产物/tag 待发布流程（用户授权）。
    - **测试证据**：workspace **1295/0/6**、e2e **25 target/174/0/5 ignored**、pytest **203/2 skipped**、harness **386/0/1**、cli **270/0/0**。
    - **台账**：issues 176/177/178/181 fixed；179/180/163/164 open+notes；新增 ISSUE-182–185（F5c/F5f/版本 pin/外部 usage 限制）。
    - 本批次后按 §3 重新实测 SHA-256/行数（00–08 合计 **7791 行**；00–08 numstat **53/62（净 -9）**；全 spec 目录 numstat **65/62（净 3）**；含 README/PROVENANCE/REFERENCES 合计以 `wc -l` 为准（当前 8218 行））。

## 3. 文件清单（SHA-256 + 行数）
| 文件 | SHA-256 | 行数 |
|---|---|---|
| `00-总纲.md` | 32193d767e2b26a9ff532aaea788c13486bf5582dbda3b4aeed71db45d008993 | 402 |
| `01-领域模型与状态机.md` | 8037a3c0b3dac86e73348c498e3b4a2f117347661f9dc42da8ca9776d17c74b3 | 447 |
| `02-DSL与校验.md` | 98b3ea2f2d588b6c685f32b617fb63060576591da6f4855af2605130d08184d8 | 992 |
| `03-调度器与生命周期.md` | fd132ad0dfbdba09d9852ac29b15d4ca5a1d510c84d752cd55d8f536b1b8d864 | 685 |
| `04-节点接口与扩展点.md` | 63a788bbfe94c15716bed9160716ad3b0c50e4c4aea8d5ea5dbc9a637574e9bb | 940 |
| `05-AgentHarness.md` | b8a6c111cc82dc4a650df62053317a9aaf34c6cae786dc29de10c74cfa99f6e6 | 1569 |
| `06-存储与可观测.md` | da513d16014d247394c912b79257290a39e8ff9626f99d421b64074bd64dc582 | 1404 |
| `07-验收用例.md` | 0800320a909d2c5c9972fe94596460a072a782c2d1f7f8753ce503c6cded4b5d | 681 |
| `08-审计关闭矩阵.md` | f74c082113c7e8824db2600065d1f4da4534e38583f99186fa63d6da9c58b8ee | 671 |

> 00–08 合计 **7791 行**（`wc -l` 口径；2026-11-04 **v0.5（M10）实现回写批次后**实测（REV-131；**00–08 numstat +53/−62（净 −9）**；**全 spec 目录 numstat +65/−62（净 +3）**；见 §2 第 28 条）；此前 v0.4 实现回写 7640 行/§2 第 27 条；v0.3 实现回写 7419 行/§2 第 26 条；W4b as-built 7154 行/§2 第 25 条；W4a 设计冻结 + REV-096 收口 7202 行/§2 第 24 条；W3a as-built 6886 行；W3a0 6815 行；W2b 6491 行；W2a 6385 行；W2a0 6308 行；W1 6051 行）。含 `README.md`、`PROVENANCE.md`、`REFERENCES.md` 后总行数以 `wc -l` 为准（当前 8218 行）。

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
- **W2b spec 回写已完成（2026-10-31，REV-077）**：00/01/02/03/06/07 已按已合并实现（REV-074 dsl / REV-075 kernel+cli / REV-076 e2e）回写 R16 静态扇出上限、`NODE_FANOUT_EXCEEDED` 运行期防御层、`CompiledNode.max_fanout`、`NodeError` canonical JSON 与 07 AC-15；`doc/architecture-v2.md` 顶部注记同步为 R1–R16（历史表述保持冻结）；默认无 `max_fanout` 的图行为/hash/错误序列与 v0.1.0 逐字节一致。台账新增 ISSUE-113–125（113–124 为 REV-074/075/076 P3，125 为 03 §3.2 预存偏差）；`doc/progress.md` 已更新为 W2b 完成、下一步 W3a0；REV-077 复审条件 C1–C4 已由条件修复批次关闭（见 §2 第 19 条）。
- **W3a0 人工评审设计冻结已完成（2026-10-31，REV-078）**：00/01/02/03/04/06/07 已按 [`DR-006`](../decisions/DR-006-w3a-human-decide-freeze.md) 冻结 human lifecycle（`builtin.human` 保留符号、`WaitingHuman` 非静默等待、decide 自动接续/无 `resume`、`human_decisions`+C13 写者、trace schema v5、CLI decide 退出码 5、AC-16 a–g 设计）。**本批是设计冻结，不代表 W3a 实现完成**；本批不写 as-built，实现偏差由 W3a-docs 回写；07 AC-16 的测试名/计数待实现后补。REV-078 首轮「有条件通过」的 C1–C5 已由条件修复批次关闭（见 §2 第 21 条），**复审通过（REV-078，复审记录 `05a5803`）**；R1–R4 复审残留已由合并前小 commit 收口；残留 P3 建议登记 ISSUE-126–128。
- **W3a 实现回写已完成（2026-11-01，REV-084）**：00/01/03/04/06/07/08 已按已合并实现（REV-079 trace / REV-080 kernel / REV-081 cli / REV-082 cli L2 / REV-083 e2e）回写 as-built（WaitingHuman 实际生命周期、`builtin.human` 精确映射、decide/consumer 实测、schema v5 迁移证据、AC-16 a–g 实测 21 用例、08 §8.10 source-breaking）；DR-006 §15 执行记录；台账新增 ISSUE-129–140。默认无 human 图行为/hash/trace 不变。
- **W4a 动态 N / foreach 设计冻结已完成（2026-11-01，REV-086）**：新增 [`DR-007`](../decisions/DR-007-w4a-dynamic-fanout-freeze.md)；spec 00/01/02/03/04/06/07 冻结写入（foreach 语法/R17/R18/`DSL_FOREACH_INVALID` 第 19 码/item 实例/join/trace v6/AC-17 a–j 设计）；SOP 设计稿升 v1.2（dynamic-N profile）；**本批是 W4b 实现前的设计冻结，不写 as-built**；W4b 实现后由 W4b-docs 回写。**用户 2026-11-01 决定 W3b 不做**（DR-004 §10）。
