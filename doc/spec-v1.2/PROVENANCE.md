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
7. **M4 Harness 回写批次**（2026-10-30，ISSUE-067/070；分支 `docs/docs-m4-spec-writeback`，评审 REV-023，提交见该分支 git log）：
   - **对应已合并代码**：kernel `ce491de`（`TOOL_ERROR` 统一 + `HarnessErrorKind::ContextOverflow` 第 8 值，REV-018）；py-worker `e099927` + `c463a49`（`zgraph_worker/tool_runner.py` discover/call + 可选 `timeout_ms`，REV-019）；worker `94e1f8a`（`WorkerFailure::Worker` 装箱消除 genai `preserve_order` 引发的 clippy 回归，REV-021）；harness `4781dd7` + `344b73d`（M4 Harness 主体 + C2 工具可见性，REV-020）；e2e `fda996c`（`tests/e2e/harness_ac08.rs` + `tests/fixtures/graphs/harness_ac08/`，AC-08a fake 必过/AC-08b 手动，REV-022）。本批次最终 base = `origin/main` `fda996c`。
   - **05**（Harness）：顶部 banner/状态改为「M4 已实现」并指向新增 §5.13；§5.1.1 H4 改「Python 调用一律在 worker 子进程；Harness 不在事件循环线程阻塞」；§5.1.2 组件分解改实际模块（`crates/zgraph-harness/src/*` + `python/zgraph_worker/tool_runner.py`）；§5.2.1 加载执行者改为 Harness 在 `agent_call` 实例执行时加载、fake 模式跳过 `api_key_env`；§5.3.1 trait 无 tokio `CancellationToken`、自有 `CancelToken`（轮询包装）+ `HarnessCtx` seam（D14/D15）；§5.3.2 辅助类型同步；§5.3.3 改 **genai 0.7.2 实测映射**（`exec_chat_stream` / `ChatStreamEvent` / `capture_*` / `webc::Error`）；§5.3.4 fake provider 落地说明；§5.3.5 重试编排位置改为 **Harness**（D16）；§5.3.6/§5.3.7 超时与取消改轮询/杀进程组；§5.5.1 重试位置、§5.5.3 空响应裁定（D17）、§5.5.4 执行循环伪代码改 v2、§5.5.5 措辞；§5.6 全面改为 **worker 子进程工具协议**（`discover`/`call` payload、失败 reason 封闭表、超时 = kill 进程组、无 import 缓存、全局工具信号量并发、8 KiB 截断、`AgentConfig.tools` 解析）；§5.7 `TOOL_ERROR` 唯一性 + `result_summary: Option<Value>`；§5.8 1:1 emit 实况；§5.9 `HarnessExecutor` 映射；§5.11 常量已补录与差异闭合状态；新增 **§5.13 M4 v2 落地偏差记录（D13–D21）**（工具子进程/取消轮询/HarnessCtx/重试编排+超时兜底/空响应裁定/ContextOverflow 第 8 值/`name()` 实况/input_mapping 优先级/genai `preserve_order` 依赖副作用）与 **§5.13.3 已知限制与 ISSUE-073..082**。
   - **00**：§0.3 `HarnessErrorKind` **7→8 值**（追加 `ContextOverflow`，M4/方案 A，见 05 §5.13 D18）；§0.3 `StreamingEvent::ToolCallEnd.result_summary` 改为 `Option<Value>`（nullable）；§0.4 增 `TOOL_ERROR` 唯一性注记（禁止 `HARNESS_TOOL_ERROR`，ISSUE-070）并标注 `HARNESS_CONTEXT_OVERFLOW` 映射；§0.5 增 05 §5.11.1 七常量核对注记（均已收录且数值一致）；banner 增 M4 批次说明。
   - **04**：§4.2.1 `NodeContext` 代码块补 `seed: u64`、字段来源改为 **`RunSpec.options.seed`**；§4.2.2 方法表补 `global_vars()` / `seed()`；新增 **§4.3.7 L3 幂等与外部副作用声明契约**（ISSUE-022/067）。
   - **07**：AC-01 补 `run_output.result.content` 的 M4 形状口径（CLI `run --fake-llm` 仍属 M5）；AC-08 拆为 **AC-08a（fake provider 必过）** 与 **AC-08b（真实模型手动，`#[ignore]`，不纳入 CI/基线）**；AC-12 后补 L3 引用（04 §4.3.7）。
   - **已知限制与 ISSUE**：05 §5.13.3 列出 ISSUE-073..082（genai preserve_order + worker Box 缓解 / 8 KiB 截断 / args_summary preview / node_timeout_ms / prompt 路径词法安全 / futures-util / 工具并发 / discover reason 超集 / 函数 >60 行 / L2 记录未接线），由 Lead 在 M4 收尾（REV-024）统一开号登记。
   - 本批次后按 `shasum -a 256 0*.md` / `wc -l 0*.md` 重新实测并更新 §3；`doc/spec-v1.2/README.md` 的行数表与根 `README.md` 由 Lead 收尾（不属本批次）。
   - 未改：08 审计关闭矩阵（历史记录，不改语义）；`doc/progress.md`、`doc/issues.md`（Lead 收尾）。

6. **后续修订规则**：任何对 spec 的修改**必须**同步本文件 §3 的 SHA-256/行数、在 §2 追加修订记录（日期 + 范围 + 关联 ISSUE/REV），并在评审记录中引用。

## 3. 文件清单（SHA-256 + 行数）

| 文件 | SHA-256 | 行数 |
|---|---|---|
| `00-总纲.md` | 2698b5a4fd87fcd4a9e30480d46019c7dfa8897938ba1684b2902c7fcee21dcb | 292 |
| `01-领域模型与状态机.md` | accb0e481fbd101a00a03a8a95518862b6f6232745667959e4bb646fbe28c283 | 302 |
| `02-DSL与校验.md` | 506f058fa68b816c2e1b5e4d2cded718007e9cda8b14be6cace387d254933eb5 | 913 |
| `03-调度器与生命周期.md` | 5f6c91bb45f6677a55f6e8d9b25c178a958c9adf7c96bbb2ca04401c5e47f002 | 388 |
| `04-节点接口与扩展点.md` | d3a34053eaade3daea325203c54d254104a3d2d4ebfdb87f8dbee1a344e42b80 | 895 |
| `05-AgentHarness.md` | cd5944370a186e8e1ed6a7311b772de066027befb43d8d48c9bc1d72f4de8c46 | 1418 |
| `06-存储与可观测.md` | fdc42157c749a21b7cff9359ccedcfee3e41e271b95fc2503dde0b511acf9feb | 985 |
| `07-验收用例.md` | 6d6f99c41cbff12972d19c8c4cb98c275b1fb592dd4fac268254a086a5af18c2 | 213 |
| `08-审计关闭矩阵.md` | 317bc008d05bd59ce0c8f25a8991543b41a4c2b06d6205bb7263317021c2d524 | 412 |

> 00–08 合计 **5818 行**（`wc -l` 口径；2026-10-30 M4 回写批次后实测，base `origin/main` `fda996c`）；含 `README.md`、`PROVENANCE.md`、`REFERENCES.md` 后总行数以 `wc -l` 为准。

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
