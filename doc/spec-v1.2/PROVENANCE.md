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

## 3. 文件清单（SHA-256 + 行数）

| 文件 | SHA-256 | 行数 |
|---|---|---|
| `00-总纲.md` | a9b12764821a8476d965f5351d6d9a1d274c8d47b603a2e641fcb8f54fdd12aa | 294 |
| `01-领域模型与状态机.md` | accb0e481fbd101a00a03a8a95518862b6f6232745667959e4bb646fbe28c283 | 302 |
| `02-DSL与校验.md` | 506f058fa68b816c2e1b5e4d2cded718007e9cda8b14be6cace387d254933eb5 | 913 |
| `03-调度器与生命周期.md` | 69f33ff7e0455d3fca51716327b0cea3f3c4db770431d335e08f99934ef6921b | 390 |
| `04-节点接口与扩展点.md` | 2d4a5ce95523b534273945f619fe3d40c32a969a1468c3b9d4600ad586221ccd | 895 |
| `05-AgentHarness.md` | 405aaa99d61b0015480eab0cf360cf0219f509cad9dd018b347ed775a12ec0d3 | 1420 |
| `06-存储与可观测.md` | da6d338ffe1f6e1e2ae539cabe6a30bf6693b07817afe733eab044d15b9403ab | 1068 |
| `07-验收用例.md` | 8a2ba1cba053d606c448d7eea35e6d9ed8395c3f1040835cfd52fc0d6f5bcb3c | 232 |
| `08-审计关闭矩阵.md` | 605907c974ffb542c940d307634bcf69acdae8020fbea591dbf2dfc6f7db24e7 | 444 |

> 00–08 合计 **5958 行**（`wc -l` 口径；2026-10-30 **M5 全部回写批次后**实测（wave1/wave2/ISSUE-066），分支 `docs/docs-m5-writeback`、评审 REV-035，base 为含 REV-025–034/038 的本地 `main`）；含 `README.md`、`PROVENANCE.md`、`REFERENCES.md` 后总行数以 `wc -l` 为准。

> **历史批次审计留痕（不覆盖）**：M4 Harness 回写批次后 00–08 合计 **5818 行**（`wc -l` 口径；评审时 base `origin/main` `fda996c`，合并 rebase 至 `12ab2f6`）。

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
