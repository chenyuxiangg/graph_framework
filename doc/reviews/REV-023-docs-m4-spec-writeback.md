# REV-023 · docs · M4 spec v2 回写（05/04/00/07 + PROVENANCE）

> **模块声明**：本分支只改 `doc/spec-v1.2/{00,04,05,07,PROVENANCE}.md`（5 files），符合 AGENTS §2 `docs` 边界；未改代码/测试/`doc/progress.md`/`doc/issues.md`/README。作者 `docs-writer`，评审人 `reviewer-b`（**非作者**）。评审**未修改任何文档/代码/台账**，只新增本记录；未 commit / 未 merge / 未 rebase。
>
> **独立核对口径**：本人亲自重算 spec 全部 SHA-256/行数、读 diff 全文与关键章节、逐条把文档断言对回已合并代码（kernel `ce491de`、harness `4781dd7`+`344b73d`、worker `94e1f8a`、py-worker `e099927`+`c463a49`、e2e `fda996c`）与 REVIEW 证据（REV-018/019/020/021/022），并实跑 `check.sh`/核对引用的测试符号存在性。

- **分支**：`docs/docs-m4-spec-writeback`
- **作者**：`docs-writer`
- **评审人**：`reviewer-b`
- **变更范围**：`fda996c..aaaee90`（1 commit `aaaee90 docs(docs): M4 spec v2 回写（05/04/00/07 + PROVENANCE）（REV-023）`；`git diff --shortstat` = **5 files changed, 482 insertions(+), 184 deletions(-)**；base = merge-base = `origin/main` 评审时 `fda996c`（main 后续前进到 `1bc40ad`，`merge.sh` 自动 rebase）；worktree clean）
- **评审时间**：2026-10-30
- **关联**：spec 05 全文（M4 Harness 实况）、00 §0.3/§0.4/§0.5、04 §4.2/§4.3.7、07 AC-01/AC-08；architecture-v2 §8（D1–D12）/§6.1 M4；PROVENANCE；REV-018/019/020/021/022；ISSUE-070/067/022

## 1. 结论

**结论：通过**（修订：原评审结论为「有条件通过」；C1/C2 关闭与修订复审见 §6/§7，原结论与证据按 AGENTS §6 保留不删）

本批回写**主体质量高、证据链完整、与已合并实现基本一致**：

- **PROVENANCE 可核验性**：独立重算 9 个分册 SHA-256 与行数，与 PROVENANCE §3 **逐项一致**（00 `2698b5…` 292 行、04 `d3a340…` 895 行、05 `cd5944…` 1418 行、07 `6d6f99…` 213 行；00–08 合计 **5818 行**），§2 第 7 条批次记录引用的 7 个代码 commit（`ce491de/e099927/c463a49/94e1f8a/4781dd7/344b73d/fda996c`）在 main 上均存在。
- **00**：`HarnessErrorKind` 已为 8 值（追加 `ContextOverflow`，与 kernel `types.rs` 一致）；`ToolCallEnd.result_summary` 改为 `Option<Value>`（与 kernel `trace.rs` 一致）；`TOOL_ERROR` 唯一性注记 + `HARNESS_CONTEXT_OVERFLOW` 映射标注正确；§0.5 七常量核对属实（表内确有 7 项且数值与 05 §5.11.1/代码 constants 一致）。
- **05**：D13–D21 与实现/评审证据逐条对得上（D13 工具子进程/缓存/全局信号量；D14 轮询取消；D15 HarnessCtx 7 方法；D16 重试在 Harness；D17 空响应连续 2 次；D18 第 8 kind；D19 `name()="genai"`；D20 input_mapping 优先级；D21 preserve_order）；§5.6 discover/call payload、reason 超集表、未知/未授权工具不产生 lifecycle 事件（A6）、§5.7 八行映射表、§5.8 1:1 emit、§5.9 输出四字段均与代码一致；对已知限制（8 KiB 非合法 JSON、args_summary 未脱敏、node_timeout 未落地、函数 >60 行、futures-util 等）如实写入 §5.13.3 L1–L10，并映射 ISSUE-073..082（连续、每项恰好一次）。
- **04/07**：`seed: u64`/来源 `RunSpec.options.seed`、`global_vars()`/`seed()` 与 kernel `context.rs` 一致；§4.3.7 L3 契约引用的 `python/tests/test_workers.py::test_worker_docstrings_declare_l3_contract` 真实存在；AC-08a 拆档的路径/测试名/断言与 `tests/e2e/harness_ac08.rs` 实际一致；AC-01 形状口径正确标注 CLI `run --fake-llm` 属 M5；无夸张完成度。

但独立核对发现 **2 个重要项**（均属"文档与真实实现/资产不一致"，需在合并前修正；均为小改），因此给「有条件通过」（条件见 §6）：

1. **重要（C1）**：`07-验收用例.md` AC-08b 两处仍写「`answer_writer` 允许调用 **`now`** 工具 / 模型是否调用 **`now`** 不可控」，但真实 fixture `tests/fixtures/graphs/harness_ac08/agents/answer_writer.json` 的 `tools` 是 **`["add","explode","slow"]`**（无 `now`），AC-08b 真实用例允许调用的是 `add`。手动验收者按文档会找不到 `now` 工具，属**可执行性误导**。
2. **重要（C2）**：`05` §5.6.5（:970）与 §5.13 D13（:1374）把**工具超时**描述为「supervisor 按 D1 路径 `cancel`→2s→`SIGTERM`→1s→`SIGKILL` 进程组」；§5.3.7（:494）对「工具执行中取消」也有类似表述。实际 Harness 路径是：`execute_tool` 的外层 `tokio::time::timeout`/取消分支命中后**丢弃 `run_worker_controlled` future** → `Supervisor::drop` **立即 `SIGKILL` 进程组**（`crates/zgraph-worker/src/supervisor.rs:182-186`）；`cancel`→2s→TERM→1s→KILL 是 supervisor **收到 cancel 信号的优雅链**，在 M4 Harness 的 drop 路径不会走完（e2e 超时用例实测整例 **0.41s**，远小于 300ms+2s+1s，佐证实际是立即 kill）。杀进程组/无残留的结论不变，但机制描述与实现不符，需修正措辞（详见 §3 #2）。

另有 2 条建议（PROVENANCE §2 编号顺序、§5.13.2 措辞）与 3 条备注（spec README 行数表由 Lead 收尾、常量符号名差异、ISSUE-073..082 待台账登记）见 §3/§5；均不影响本批回写主体价值。

## 2. 四维评审

| 维度 | 结论 | 依据（文件/行/核验） |
|---|---|---|
| 功能正确性 | ⚠️ | **与实现一致项**（抽查逐条对代码）：① 00 §0.3 八值 / `result_summary: Option<Value>` ↔ `crates/zgraph-kernel/src/types.rs`（8 变体）、`trace.rs`（`Option<Value>`）；② 05 §5.3 genai 0.7.2 映射（`Client::exec_chat_stream`、`ChatStreamEvent::{Chunk,ReasoningChunk,ToolCallChunk,End}`、`capture_*`、`webc::Error`）↔ `crates/zgraph-harness/src/llm/transport.rs`；③ D14 轮询取消/`CANCEL_POLL_INTERVAL_MS=50` ↔ `ctx.rs`；④ D15 HarnessCtx 7 方法 ↔ `ctx.rs:21-66`；⑤ D16 重试/超时兜底在 `invoke_with_retry` ↔ `agent_loop.rs`；⑥ D17 空响应连续 2 次 ↔ `agent_loop.rs`；⑦ §5.6 discover/call payload + reason 表 + `timeout_ms` 可选 ↔ `tools.rs`/`python/zgraph_worker/tool_runner.py`；⑧ A6 未知/未授权工具无 lifecycle 事件 ↔ `agent_loop.rs`（C2 修复后）+ e2e；⑨ §5.7 八行映射表/`TOOL_ERROR`/`ContextOverflow` detail ↔ `error.rs`；⑩ 04 `seed`/`global_vars()` ↔ kernel `context.rs`；⑪ AC-08a 测试名/断言 ↔ `tests/e2e/harness_ac08.rs`；⑫ PROVENANCE 7 个代码 commit 均存在于 main。**扣分**：C1（AC-08b `now` 与 fixture 不符）、C2（tool 超时 kill 机制描述错误）。 |
| 代码结构合理性 | ✅ | 只改 5 个 spec 文件、无代码/测试/台账改动；05 新增 §5.13（D13–D21）+ §5.13.3（L1–L10）结构清晰，与既有 §5.11/§5.12 衔接；PROVENANCE 按既有"批次记录 + 文件清单"结构追加；未破坏历史（08 审计矩阵按要求不动，旧章节以"回写"方式替换并由 PROVENANCE 记录，符合本仓库 spec 回写流程 REV-014 先例）。 |
| 逻辑性 | ⚠️ | 文档内部状态总体自洽：00/05 的 8 值枚举、`result_summary` 可空、`TOOL_ERROR` 唯一在 00/05/07 交叉引用一致；D13–D21 与 §5.13.1 映射表、§5.11.2 G1–G12 状态表相互呼应；L1–L10 ↔ ISSUE-073..082 连续、每项唯一映射。**扣分**：C2 的超时机制描述与 Harness/worker 实际路径矛盾（§5.6.5/§5.13 D13/§5.3.7 同源措辞需一起改）；PROVENANCE §2 列表出现 1,2,3,4,5,**7,6** 的顺序错位（见 §3 #3）；§5.13.2「错误映射的 7 个 `HarnessErrorKind`…均不变」与新增第 8 值/§5.7 八行表措辞不一致（见 §3 #4）。 |
| 可读性 | ✅ | 每处回写均带"M4 实况/M4 已知限制/评审证据"标记与 REV/ISSUE 引用，读者可追溯；表格化呈现、失败 reason/限制清单集中；PROVENANCE §2 第 7 条把"分册—改动点—代码 commit—测试"逐项列清，可核验性高；行数/SHA 表可直接复核（本次已复算一致）。C1 的 `now` 与 C2 的机制措辞属事实精度问题（已在 §3 列出），不影响整体可读性。 |

## 3. 发现清单

| # | 级别 | 位置 | 问题 | 建议 | 状态 |
|---|---|---|---|---|---|
| 1 | **重要（C1）** | `doc/spec-v1.2/07-验收用例.md:157,164`（AC-08b 前置/手动口径） | 两处写 `answer_writer` 允许调用 **`now`** 工具、模型是否调用 `now` 不可控；实际 fixture `tests/fixtures/graphs/harness_ac08/agents/answer_writer.json` 的 `tools = ["add","explode","slow"]`（无 `now`），`ac08b_real_llm_manual` 允许的是 `add`。手动验收者按文档找不到 `now`，前置不可执行/误导 | 改为「`answer_writer` 允许调用 **`add`** 工具（fixture 声明 `tools=["add","explode","slow"]`）」；两处 `now` 同步改 `add`（或写"fixture 声明的工具（如 add）"） | open（条件） |
| 2 | **重要（C2）** | `doc/spec-v1.2/05-AgentHarness.md:970`（§5.6.5 超时）、`:1374`（D13）、`:494`（§5.3.7 工具执行中取消） | 文档称工具超时/取消走 supervisor 优雅链 `cancel`→2s→`SIGTERM`→1s→`SIGKILL` 进程组；实际 M4 Harness 路径是：`execute_tool` 外层 `tokio::time::timeout`/取消分支命中 → **丢弃 `run_worker_controlled` future** → `Supervisor::drop` **立即 `SIGKILL` 进程组**（`crates/zgraph-worker/src/supervisor.rs:182-186`）；优雅链只在 supervisor 自身收到 cancel 信号且未被 drop 时生效，Harness drop 路径不会走完。e2e 超时用例实测 **0.41s**（<300ms+2s+1s）佐证立即 kill；杀进程组/无残留的结论不变 | 精确改写三处：工具超时 = harness 侧 timeout 到点 → 丢弃 worker future → `Supervisor::drop` 立即 `SIGKILL` 进程组（D1，无残留）；`cancel`→2s→`SIGTERM`→1s→`SIGKILL` 为 supervisor 显式取消的优雅升级链（M3 协议 §4），M4 Harness 的 timeout/drop 路径不保证走完。建议引 `supervisor.rs:182-186` + AC-08c 实测 0.41s 作证据 | open（条件） |
| 3 | 建议 | `doc/spec-v1.2/PROVENANCE.md` §2 列表（:41 附近） | §2 编号顺序为 **1,2,3,4,5,7,6**：M4 批次被标为「7.」但插在既有「6. 后续修订规则」之前，列表顺序错位 | 把「7. M4 Harness 回写批次」整段移到「6. 后续修订规则」之后（保持 6→7 顺序，现有 3 处「§2 第 7 条」引用 `00:2`/`05:3`/`PROVENANCE:80` 无需改）；或改号 6/7 并同步这 3 处引用。不阻塞 | open |
| 4 | 建议 | `doc/spec-v1.2/05-AgentHarness.md:1400`（§5.13.2） | 「`AgentOutput` 四字段…错误映射的 **7 个** `HarnessErrorKind` 与补充路径（§5.7）均不变」——本分册/00 已为 8 值、§5.7 表已 8 行，易被读成"仍 7 值" | 改为「**既有 7 值**语义不变；第 8 值 `ContextOverflow` 见 D18」或等价措辞 | open |
| 5 | 备注（Lead 收尾） | `doc/spec-v1.2/README.md:24-30`（00/04/05/07 行数表） | README 行数表仍为旧值（00 287 / 04 876 / 05 1172 / 07 195），实际为 292/895/1418/213；PROVENANCE §2 第 7 条已声明「README 行数表与根 README 由 Lead 收尾（不属本批次）」 | REV-024（M4 收尾）必须更新 `doc/spec-v1.2/README.md` 行数表（含 00/04/05/07 与总行数）及根 `README.md` 事实性；否则 spec 目录内部不一致 | open（REV-024） |
| 6 | 备注 | `doc/spec-v1.2/00-总纲.md:238`、`05:972` vs `crates/zgraph-harness/src/constants.rs`（`TOOL_RESULT_MAX_BYTES`） | spec 常量名 `DEFAULT_TOOL_RESULT_MAX_BYTES` 与 harness 代码符号 `TOOL_RESULT_MAX_BYTES` 不同（语义/数值一致，8_192）；05 §5.11.1 已注明"旧稿写 TOOL_RESULT_MAX_BYTES，以 00 名称为准"，但未说明代码符号差异 | 可接受（spec 名 vs 代码符号名不要求逐字相同）；建议 M5 统一或在 05 §5.11.1 注一句代码符号，避免读者检索困惑 | open（低优先） |
| 7 | 备注（台账联动） | `05 §5.13.3`（ISSUE-073..082）、`PROVENANCE §2 第 7 条` | 文档引用 **ISSUE-073..082**（10 条，编号连续、L1–L10 每项唯一映射），但当前 `doc/issues.md` 最大编号仍是 **ISSUE-072**（本分支按模块边界未改台账） | Lead 在 `REV-024` 收尾时按同一映射登记 ISSUE-073..082（内容与 §5.13.3 L1–L10 一致），并在 `doc/issues.md` 保留 ISSUE-070/067/022/047 状态更新依据；本评审在 REV-024 核对覆盖 | open（REV-024） |

> **阻塞项：无**；C1/C2 为重要项，按 §6 条件关闭后给「通过」。建议 #3/#4 可随同一修订提交一并处理（成本极低）；#5/#7 为 Lead 收尾职责；#6 低优先。

## 4. 验证证据

### 4.1 范围、分支与提交元数据

```bash
cd /Users/zyzs/lczyzs/code/zgraph/.worktrees/docs-docs-writer
git status --short                      # 空（worktree clean）
git branch --show-current               # docs/docs-m4-spec-writeback
git merge-base origin/main HEAD         # fda996c73ca9bb405daaee16d5f89e5b7ed1582f（评审时 base；main 已前进到 1bc40ad，merge.sh 自动 rebase）
git rev-parse HEAD                      # aaaee90067299186dc3ef2757cce7df24604e19a
git diff --shortstat HEAD~1..HEAD       # 5 files changed, 482 insertions(+), 184 deletions(-)
git diff --name-only HEAD~1..HEAD       # doc/spec-v1.2/{00,04,05,07,PROVENANCE}.md（仅 5 个文件）
git log -1 --format='%an <%ae> / %cn <%ce> / %s' HEAD
# chenyuxiangg <1007423165@qq.com> / chenyuxiangg <1007423165@qq.com> / docs(docs): M4 spec v2 回写（05/04/00/07 + PROVENANCE）（REV-023）
# 正文含 Review: REV-023 / Agent: docs-writer
bash scripts/dev/check.sh               # CHECK PASSED（含 cargo fmt/git diff --check）
```

### 4.2 PROVENANCE 可核验性（独立复算）

```bash
cd doc/spec-v1.2 && shasum -a 256 0*.md && wc -l 0*.md
# 实测：00 292 / 01 302 / 02 913 / 03 388 / 04 895 / 05 1418 / 06 985 / 07 213 / 08 412（合计 5818）
# 与 PROVENANCE §3 表格逐项比对：9/9 SHA-256 与 9/9 行数 **完全一致**；合计 5818 一致
cd /Users/zyzs/lczyzs/code/zgraph && for h in ce491de e099927 c463a49 94e1f8a 4781dd7 344b73d fda996c; do git cat-file -t "$h"; done
# 7/7 均为 commit（PROVENANCE §2 第 7 条引用的已合并代码存在）
```

### 4.3 关键内容 ↔ 已合并代码抽查（逐条）

| 文档断言 | 代码/测试核对 | 结论 |
|---|---|---|
| 00 §0.3 `HarnessErrorKind` 8 值（新增 `ContextOverflow`） | `crates/zgraph-kernel/src/types.rs` 枚举 8 变体；`tests/failure.rs::node_error_codes_match_spec` 含第 8 值 | ✅ |
| 00 §0.3 `ToolCallEnd.result_summary: Option<Value>` | `crates/zgraph-kernel/src/trace.rs` 字段为 `Option<Value>`；harness `session` 传 `Some(...)` | ✅ |
| 00 §0.4 `TOOL_ERROR` 唯一 / 无 `HARNESS_TOOL_ERROR` | kernel `CODE_HARNESS_TOOL_ERROR = "TOOL_ERROR"`（REV-018）；全仓 grep `HARNESS_TOOL_ERROR` 仅命中"禁止"注记 | ✅ |
| 00 §0.5 七常量已收录且数值一致 | 表内 7 项齐全（2_048/0.2/65_536/0.8/8_192/30_000/1_000）；harness `constants.rs` 同值 | ✅ |
| 05 D13 工具子进程/discover 缓存/全局信号量 | `harness/tools.rs`（`WorkerToolRunner`、`run_worker_controlled`、静态 `Semaphore(default_max_workers())`）；`executor.rs` `tools_cache` 按 tools_dir | ✅ |
| 05 D14/F14 轮询取消 50ms | `patched ctx.rs` `CANCEL_POLL_INTERVAL_MS=50`、`CancelToken`/`wait_cancelled`；无 tokio `CancellationToken` | ✅ |
| 05 D15 HarnessCtx 7 方法 | `ctx.rs::HarnessCtx`（emit/cancel_requested/node_deadline_remaining_ms/user_id/get_var/seed/instance_id） | ✅ |
| 05 D16 重试编排在 Harness + 超时兜底 | `agent_loop.rs::invoke_with_retry`（`tokio::time::timeout` + 退避/retry-after/deadline） | ✅ |
| 05 D17 空响应连续 2 次 | `agent_loop.rs` `empty_streak >= 2` → `HARNESS_LLM_ERROR{reason:"empty_response"}` | ✅ |
| 05 D18 ContextOverflow 第 8 值 | kernel `ce491de`；harness `error.rs::context_overflow()` + 单测 | ✅ |
| 05 D19 `name()="genai"` | `llm/transport.rs` `fn name() -> "genai"`；fake = `"fake"` | ✅ |
| 05 D20 input_mapping 优先级 | `assemble.rs::effective_input_mapping`（节点级 > AgentConfig > identity，非声明端口丢弃） | ✅ |
| 05 D21 genai→preserve_order | `genai-0.7.2 Cargo.toml [dependencies.serde_json] features=["preserve_order"]`；REV-020/021 证据 | ✅ |
| 05 §5.6 discover/call payload + reason 表 + 可选 `timeout_ms` | `harness/tools.rs`（payload/解析/reason 映射）；`python/zgraph_worker/tool_runner.py`（discover/call） | ✅ |
| 05 §5.6.6 / §5.8 A6 未知/未授权工具不产生 lifecycle 事件 | `agent_loop.rs:250-258`（C2 修复，emit 前返回）；harness 单测 + e2e unknown tool | ✅ |
| 05 §5.7 八行映射表 / detail | `harness/error.rs`（8 构造器 + detail 键）；kernel code() | ✅ |
| 05 §5.9 输出四字段 | `agent_loop.rs::AgentOutput::to_json`；e2e AC-08a 断言 | ✅ |
| 04 §4.2.1 `seed: u64` 来源 `RunSpec.options.seed` | kernel `context.rs`（seed 字段/`seed()`）、`run.rs` 传入 `options.seed` | ✅ |
| 04 §4.2.2 `global_vars()`/`seed()` | kernel `context.rs` 两访问器 | ✅ |
| 04 §4.3.7 L3 契约引用的测试 | `python/tests/test_workers.py::test_worker_docstrings_declare_l3_contract` 存在 | ✅ |
| 07 AC-08a 路径/测试名/断言 | `tests/e2e/harness_ac08.rs`（`ac08a_fake_llm_tool_loop_streams_tokens_into_trace` 等）实测 4 passed/1 ignored（REV-022） | ✅ |
| 07 AC-08b `#[ignore]`/env/no-hard-tool-assert | `ac08b_real_llm_manual`（`#[ignore]`、`ZGRAPH_REAL_LLM`/`DEEPSEEK_API_KEY`、只断言 Completed/usage/token 非空） | ✅（**但工具名 `now` 错误，见 #1**） |
| 07 AC-01 形状口径/CLI 属 M5 | `05 §5.9.2` 四字段；CLI `run` 未实现（M5） | ✅ |
| 05 §5.13.3 L1–L10 ↔ ISSUE-073..082 | 10 条、编号连续、每项唯一映射（073→L6、074→L4、075→L5、076→L1、077→L2、078→L3、079→L7、080→L9、081→L8、082→L10） | ✅（待 REV-024 台账登记） |

### 4.4 未发现的问题类别（核对说明）

- **无删改历史**：只改 5 个 file；08 审计矩阵未动；旧章节以"回写"替换并在 PROVENANCE §2 记录批次（与 REV-014 先例一致），未发现无记录的语义改动。
- **无完成度夸大**：CLI `run`/`replay`/AC-12/`--record` 等仍标 M5；AC-08b 明确"手动、不进 CI/基线"；L10 明确 L2 记录未接线。
- **无越界**：未改 `doc/progress.md`、`doc/issues.md`、`README.md`（根）、代码/测试。

## 5. 遗留问题

- **条件（合并前）**：#1 07 AC-08b `now` → `add`（C1）；#2 05 三处工具超时/取消 kill 机制措辞（C2）。
- **建议（可随修订）**：#3 PROVENANCE §2 编号顺序；#4 §5.13.2 "7 个 kind" 措辞。
- **Lead 收尾（REV-024）**：#5 `doc/spec-v1.2/README.md` 行数表 + 根 README 事实性；#7 `doc/issues.md` 登记 ISSUE-073..082（与 §5.13.3 L1–L10 映射一致）并更新 ISSUE-070/067/022/047 状态依据。
- **低优先**：#6 spec 常量名 vs harness 代码符号名（`DEFAULT_TOOL_RESULT_MAX_BYTES` vs `TOOL_RESULT_MAX_BYTES`）。
- 本评审未修改任何文档/代码/台账；REV-023 记录为本分支唯一新增文档。

## 6. 条件关闭记录（"有条件通过"必须关闭后才能合并）

| 条件 | 级别 | 关闭证据（作者修订后由评审追加复审段） | 关闭日期 |
|---|---|---|---|
| **C1** 07 AC-08b 前置/口径的 `now` 工具与真实 fixture（`tools=["add","explode","slow"]`）一致（改 `add` 或等价表述） | 重要 | **已关闭**：docs `af5768f`；07:157/164 已改为 fixture 实际工具（`tools=["add","explode","slow"]`、可调用 `add`、无 `now`） | 2026-10-30 |
| **C2** 05 §5.6.5:970 / §5.13 D13:1374 / §5.3.7:494 的工具超时/取消 kill 机制改为与 `supervisor.rs:182-186` Drop→SIGKILL 实际路径一致（优雅链仅 supervisor 显式取消路径，不保证走完） | 重要 | **已关闭**：docs `af5768f`；三处已改为「外层 timeout/取消 → 丢弃 future → Supervisor::drop 立即 SIGKILL 进程组（引 supervisor.rs:182-186）；优雅链仅 supervisor 显式 cancel，不保证走完（e2e ~0.41s 旁证）」 | 2026-10-30 |

> 条件均为**文档措辞**修正（不涉及代码/语义/架构变更）。C1/C2 已随 `af5768f` 关闭并经 §7 复审确认，全分支结论改「通过」；建议 #3/#4 已同批修正。

## 7. 复审（2026-10-30，reviewer-b，C1/C2 整改，HEAD `af5768f`）

> 作者 `docs-writer` 在 `aaaee90` 之上新增 `af5768f docs(docs): 落实 REV-023 C1/C2 条件（REV-023）`（3 files, +9/−9）。本节仅按 §6 条件核对；原 §1–§5 历史保留（§1 已按修订标注为「通过」）。

- **范围**：`aaaee90..af5768f`（3 files：05/07/PROVENANCE；`git diff --shortstat` = 9 insertions(+), 9 deletions(-)）；base 仍 `fda996c`（main 已到 `1bc40ad`，`merge.sh` 自动 rebase）；worktree clean；提交正文含 `Review: REV-023` / `Agent: docs-writer`。
- **C1 关闭核对**：`07-验收用例.md:157` 前置改为「fixture `tests/fixtures/graphs/harness_ac08/agents/answer_writer.json` 声明 `tools=["add","explode","slow"]`（真实模型可调用 `add` 等；**无 `now`**）」；`:164` 手动口径改「模型是否调用 `add` 等 fixture 工具不可控」。与 fixture/`ac08b_real_llm_manual` 实际一致（无残留错误 `now`；§7.1 的 `now` 属 customer_ticket 示例技能表，与本例无关）。**C1 已关闭。**
- **C2 关闭核对**：05 `§5.3.7:494`、`§5.6.5:970`、`§5.13 D13:1374` 三处统一改为「Harness 外层 `tokio::time::timeout`/取消分支命中 → 丢弃 `run_worker_controlled` future → `Supervisor::drop` **立即 `SIGKILL` 进程组**（`crates/zgraph-worker/src/supervisor.rs:182-186`）；`cancel`→2s→`SIGTERM`→1s→`SIGKILL` 仅 supervisor **显式 cancel** 的优雅升级链，M4 timeout/drop 路径**不保证走完**（e2e 实测 ~0.41s ≪ 3s）」；杀进程组/无残留结论不变。与代码（`supervisor.rs` Drop→Kill）+ e2e 证据一致。**C2 已关闭。**
- **顺手项核对**：#3 PROVENANCE §2 已改为 1–6 后接 7（M4 批次在「后续修订规则」之后），「§2 第 7 条」引用无需变更且语义正确——**fixed**；#4 05 §5.13.2 改为「既有 7 个 `HarnessErrorKind` 不变；第 8 值 `ContextOverflow` 见 D18」——**fixed**。
- **PROVENANCE 复核**：独立重算 9 个分册 SHA-256 与行数，与 §3 表 **9/9 逐项一致**；05/07 新 hash 分别为 `2dc2e12c…`、`0cb84c40…`（行数 1418/213 不变，合计 **5818**）；§2 批次记录/引用保持可核验。
- **验证命令**：`bash scripts/dev/check.sh` → **CHECK PASSED**；`git status` clean；`git diff aaaee90..af5768f` 仅上述 3 文件 9/9 行改动（无越界、无删改历史）。
- **遗留（不变）**：#5 `doc/spec-v1.2/README.md` 行数表 + 根 README 事实性由 **REV-024** 更新；#7 `doc/issues.md` ISSUE-073..082 由 REV-024 登记；#6 常量符号名差异为低优先备注。
- **复审结论：通过**（C1/C2 均已关闭；建议 #3/#4 已同批修正；可交 Lead 合并 `docs/docs-m4-spec-writeback`）。
