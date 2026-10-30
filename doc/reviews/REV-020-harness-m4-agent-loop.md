# REV-020 · harness · M4 AgentHarness（config/budget/llm/loop/tools/executor）

> **模块声明**：本分支只改 `crates/zgraph-harness/**`（15 个源文件）与 `Cargo.lock`（genai/async-trait/futures-util 依赖解析副作用；`Cargo.lock` 属 **process** 模块，Lead 已在提交正文显式确认「Cargo.lock 为 cargo 正常副作用（Lead 已确认）」，按 AGENTS §2 属 Lead 批准的跨模块改动，本评审对 lock 做最小性核对）。作者 `harness-dev`，评审人 `reviewer-b`（**非作者**）。本次评审**未修改任何代码/配置/台账**，只新增本记录；未 commit / 未 merge / 未 rebase。
>
> **最终评审对象**：按 Lead 更正，以 **`1d8ef79`** 为 HEAD（作者对 `95d44c4` squash + 两处自检修正：config 恒真断言改为真实断言、executor `config_failure` 的 `elapsed_ms` 按有效 node timeout 计算）。旧 head `95d44c4` 的差异已单独核对（§4.5），不再重复审。

- **分支**：`feat/harness-m4-agent-loop`
- **作者**：`harness-dev`
- **评审人**：`reviewer-b`
- **变更范围**：`838e969..1d8ef79`（1 commit `1d8ef79 feat(harness): M4 AgentHarness（config/budget/loop/tools/genai/fake）（REV-020）`；`git diff --shortstat` = **17 files changed, 10881 insertions(+), 213 deletions(-)**；唯一非 harness 路径是 `Cargo.lock`；base = merge-base = `origin/main` = `838e969`，**未 rebase**）
- **评审时间**：2026-10-30
- **关联**：spec 05 全文（§5.2 config / §5.3 LLM / §5.4 budget / §5.5 loop / §5.6 tools / §5.7 error / §5.8 streaming / §5.9 output）；spec 00 §0.3–§0.5；architecture-v2 §4/§6.1(M4)/§8 **D3/D5/D7** 与 §8.2；kernel `NodeContext`/`NodeExecutor`/`CompiledHandler::Agent`/`StreamingEvent`；worker `run_worker_controlled`；py-worker `tool_runner`（已合并 `c463a49`）；`doc/issues.md` ISSUE-070/071；task-4；REV-018（kernel 第 8 kind `ContextOverflow`）

## 1. 结论

**结论：有条件通过**

这是 M4 最重的一块，主体质量高且覆盖面真实：

- **结构**：`config / assemble / budget / llm{types,fake,transport} / tools / agent_loop / executor / error / ctx / constants` 与 spec 05 §5.1.2 组件分解一一对应；`genai` 类型**只**出现在 `llm/transport.rs`（其余文件仅注释提及，grep 实核），对外只有 Harness 自有类型；`HarnessCtx` seam 只由 `NodeContext` 适配（不触达 `RunContext`/SQLite，grep 实核）；工具执行复用 `zgraph_worker::run_worker_controlled`（每次调用一子进程，无重复实现 worker 生命周期）；工具发现按 `tools_dir` 在 executor 内缓存一次。
- **逻辑**：iteration 在请求前 +1、重试不 +1、单响应多 tool 串行只 +1；仅 Establish 可重试（退避/`retry-after`/deadline 截断/等待期可取消）；空响应连续 2 次才失败；`tool_error` 喂回继续；取消→`NodeError::Cancelled`、deadline→`NodeError::Timeout`；流式 1:1、不聚合，A1/A2/A3/A5 均有真实断言。
- **测试**：`cargo test -p zgraph-harness` **105 passed / 0 failed / 1 ignored**（真实模型 `#[ignore]`）；`cargo test --workspace` **418 passed / 1 ignored / 0 failed**（50 个测试二进制，M3 的 313+ 基线全部保留）；**真 Python 子进程 roundtrip**（discover/call/异常/超时）与**经 kernel `start_run` 的端到端**（含 trace 流式入 `MemoryTraceSink`）都真实通过。
- **规范**：`#![forbid(unsafe_code)]` 保留；`missing_docs`；生产代码无 `unwrap/expect/panic!`（脚本扫描 0 命中）；常量集中在 `constants.rs`；`check.sh` PASS；提交元数据合规。

但存在 **1 个阻塞项 + 2 个重要项（含 1 个功能正确性问题）**，因此给「有条件通过」（条件见 §6，全部关闭后由评审追加复审段再合并）：

1. **阻塞（CI/DoD 回归）**：本分支把 `genai` 引入工作区依赖图后，其**无条件**依赖 `serde_json` 且开启 `preserve_order`，使工作区统一构建的 `serde_json::Value` 变大 → `zgraph-worker::WorkerFailure`（144 B）触发 `clippy::result_large_err`。**在干净 target 上** `cargo clippy --workspace --all-targets -- -D warnings`（`.github/workflows/ci.yml:20`）在 `zgraph-worker` 报 **14 errors**；同命令在 `main`（`838e969`）干净 target 上 **exit 0**。merge.sh 不跑 clippy，所以门禁能过，但 CI 会红，违反 architecture §2.2「CI 同时执行两套检查 / clippy -D warnings」。修复点在 worker/process（跨模块，需 Lead 授权），不在 harness 源码。
2. **重要（功能正确性）**：`AgentConfig.tools` 的**可见性约束未落地**——`agent_loop` 的未知工具检查/执行使用**全量注册表**（`run.registry.get`/`names()`），未按本 Agent 的 `config.tools` 过滤；LLM 若幻觉到「已注册但未授权给本 Agent」的工具名会被**执行**，违反 05 §5.2.2「本 Agent 可见工具（LLM 只能调用这些）」，`HARNESS_UNKNOWN_TOOL.detail.available_tools` 也应给出可见工具列表而非全量。
3. **重要（trace/隐私 + 测试名不副实）**：`ToolCallStart.args_summary` 的 `preview` 是参数的**原始 JSON 前缀**（≤256 字符）——小参数对象（如 `{"secret_token":"abcdefghij"}`，27 字符）会被**完整**写进 trace 的 `args_summary.preview`，与 05 §5.8.5「**不得**放完整参数（可能含敏感数据）」冲突；而单测 `summaries_do_not_leak_full_arguments`（`tools.rs:1413`）只断言 `keys/bytes/preview 长度`，**没有断言值不泄露**，名称与断言不符。

另有 8 条建议/已知限制（node_timeout_ms 未落地、8 KiB 截断非合法 JSON、preserve_order 全局副作用、工具并发独立信号量、schema 白名单超集、discover reason 超集、函数 >60 行、futures-util 未进 workspace deps 等）见 §3/§5，需按 AGENTS §7 登记 ISSUE 并纳入 docs 回写（§6-C3）。

## 2. 四维评审

| 维度 | 结论 | 依据（文件/行/测试/命令） |
|---|---|---|
| 功能正确性 | ⚠️ | **达标项**：① AgentConfig 加载/V1–V16：`config.rs` 实现索引（单层/文件名排序/重复 id 取首个）、`reject_unknown`（未知字段 + 保留字 `reserved_field`）、文件 256 KiB / prompt 1 MiB、UTF-8、`schema_version`、V5–V13 范围、tools 数量/引用名、input_mapping 非空、api_key（fake 跳过）；测试 `config.rs` 20 条（含超限/坏 UTF-8/路径逃逸/未注册工具/api_key）。② budget：字符类估算 `ceil(ascii/4+cjk+other×2)`（`budget.rs:20-41`，CJK 区段覆盖 3000–303F）、`prompt_budget=floor(window×0.8)−max_tokens`、`check_and_trim` 每次调用前执行、溢出 `HARNESS_CONTEXT_OVERFLOW` 带 `budget/required/segments/first_over`（`agent_loop.rs:1120-1141` 断言 kind/detail code/stage/first_over）。③ 状态机/iteration/重试/多 tool 串行/空响应/未知工具/max_iterations 的规则逐条落到 `agent_loop.rs:161-279/289-382`，并有 20 条单测（含 retry 不消耗 iteration、streaming 失败不重试、多 tool 串行顺序、参数校验失败不调 Python）。④ 工具：执行超时 `min(declared, 30_000, 剩余 deadline)`（`agent_loop.rs:439-444`）、非 dict/异常/超时均喂回继续、结果 8 KiB 截断、`ToolCallStart→execute→ToolCallEnd`、`ToolCallEnd.result_summary: Option<Value>` 与 kernel 类型一致（kernel `trace.rs` 实核）。⑤ 错误映射：8 个 kind 的 code/detail（`error.rs` 8 条单测 + `failure.rs` 侧已覆盖 ContextOverflow）、取消/超时走 `RunFailure` 专用出口（`agent_loop.rs:75-110`）。⑥ 真实链路：`executor.rs` 5 条测试经 kernel `start_run` 完成 `agent_call`（含工具 loop、discover 缓存、ConfigLoad Crashed、非 Agent handler→Internal）。**扣分项**：C2 可见性未强制；C3 args_summary 泄露；8 KiB 截断后非合法 JSON（§3 #4）；`AgentConfig.node_timeout_ms` 无实际作用（§3 #6）。 |
| 代码结构合理性 | ✅ | 模块职责与 05 §5.1.2 对应且依赖方向正确（llm 只依赖自有类型；transport 是唯一 genai 边界；`executor` 只实现 `NodeExecutor`；`agent_loop` 只经 `HarnessCtx`/`ToolRunner`/`LlmProvider` 抽象）。无重复实现：worker 进程生命周期直接复用 `run_worker_controlled`；trace 只经 `NodeContext::emit`。`cargo tree -e features -i serde_json` 显示 `genai→serde_json/preserve_order`（这是阻塞项根因，非结构缺陷）。`HarnessExecutor.tools_cache` 按绝对 `tools_dir` 缓存，锁为 tokio Mutex，跨 await 安全。`Cargo.toml` 使用 `.workspace = true` 引入 async-trait/genai（REV-017 已合并），仅 `futures-util` 直接依赖（§3 #12）。 |
| 逻辑性 | ⚠️ | 状态机分支自洽：`iteration>=max` 在循环顶判定（最多发起 max 次请求）；取消/deadline 预检先于 provider 错误；重试仅 `is_retryable() && attempt<max_attempts`，且 `remaining>wait_ms` 才等待（严格避免"重试必超时"）；`sleep_or_cancel` 用 `wait_cancelled` 50ms 轮询，不伪造 `cancelled().await`；工具执行 `select!` 取消优先、drop 工具 future（worker 进程组由 `Supervisor::Drop`/协议 kill 回收）；消息序列 fail-fast（system 首位、tool 消息有对应 call_id、user≥1、256 KiB 上限）。**扣分**：可见性判断用全量 registry（C2）打破"LLM 只能调用 AgentConfig.tools"的不变量；8 KiB 截断把 JSON 截成非法结构（§3 #4）使"喂回 LLM 的错误 JSON"在极端情况下不可解析。 |
| 可读性 | ⚠️ | 模块/函数 docstring 普遍解释"为什么"（H1/D3/取消轮询/伪代码与实现差异/空响应裁定等均注明来源）；常量集中 `constants.rs`；`#![forbid(unsafe_code)]`、`missing_docs`；无 `unwrap/expect/panic`（生产路径扫描 0）；测试名多数自解释。**扣分**：5 个生产函数 >60 行（`run_agent` 139、`parse_model` 124、`parse_discover_output` 75、`parse_call` 66、`map_webc_error` 64），超 architecture §2.2 对 crates 的等价约束（函数 ≤60 行）；`MAX_LAST_CONTENT_SUMMARY_CHARS` 定义未用（硬编码 512）；`summaries_do_not_leak_full_arguments` 名不副实；`seven_kinds_map_to_spec_error_codes` 名称未随 kernel 第 8 kind 更新（有独立 ContextOverflow 用例）。 |

## 3. 发现清单

| # | 级别 | 位置 | 问题 | 建议 | 状态 |
|---|---|---|---|---|---|
| 1 | **阻塞** | `crates/zgraph-harness/Cargo.toml`（`genai.workspace = true`）+ `Cargo.lock`；`.github/workflows/ci.yml:20` | 引入 genai 后其**无条件**依赖 `serde_json` 并开启 `preserve_order`（genai-0.7.2 `Cargo.toml` `[dependencies.serde_json] features=["preserve_order"]`），工作区统一构建时 `serde_json::Value` 变大 → `zgraph-worker::WorkerFailure` 144 B 触发 `clippy::result_large_err`。**干净 target** 实测：本分支 `cargo clippy --workspace --all-targets -- -D warnings` **exit 101 / 14 errors（全部在 worker）**；`main`（838e969）同命令 **exit 0**。merge.sh 不跑 clippy，但 CI 会红，违反 architecture §2.2 | 需 **worker/process** 跨模块修复（Lead 授权，harness 源码无需改）：给 worker 内 14 处或 crate 级 `#[allow(clippy::result_large_err)]`，或 `Box<WorkerFailure>` / 减小 `WorkerError`；修后必须在**干净 target**（如 `CARGO_TARGET_DIR=/tmp/...`）重跑并贴证据。若决定不修，需 Lead 书面裁定并在 ISSUE 记录 CI 例外（不推荐） | open |
| 2 | **重要**（C2） | `crates/zgraph-harness/src/agent_loop.rs:250-258`（`run.registry.get(&call.name)`、`run.registry.names()`）；`executor.rs:73-87/143`（传给 loop 的是全量 registry） | 未知工具检查/执行用**全量注册表**，未按本 Agent 的 `AgentConfig.tools` 过滤：LLM 返回"已注册但未授权给本 Agent"的工具名时会被执行（`FakeToolRunner`/`WorkerToolRunner` 均会收到调用），违反 05 §5.2.2「tools：本 Agent 可见工具（LLM 只能调用这些）」；`HARNESS_UNKNOWN_TOOL.detail.available_tools` 也变成全量工具名，可能泄露其它 Agent 的工具面。现有测试 `loaded_agent` 总是令 `config.tools == registry`，未覆盖该差异 | 在 `run_agent` 入口构造「可见工具名集合」（`agent.config.tools` resolve 后的 names，含全限定/短名归一），未知/未授权工具一律返回 `HARNESS_UNKNOWN_TOOL`，`available_tools` 用可见集合；补 1 条单测：registry 含 a/b、config.tools 只含 a，LLM 调 b → Crashed 且 b 未被调用。修复后可降为「通过」条件关闭 | open |
| 3 | **重要** | `crates/zgraph-harness/src/tools.rs:428-439`（`summarize_args` preview）；`tools.rs:1413-1418` 测试 | `args_summary.preview` 是参数**原始 JSON 前缀**（≤256 字符）：参数对象较短时（如 27 字符的 `{"secret_token":"abcdefghij"}`）会**完整**进入 trace 的 `ToolCallStart.args_summary`，与 05 §5.8.5「**不得**放完整参数（可能含敏感数据）」冲突；单测 `summaries_do_not_leak_full_arguments` 只断言 `keys/bytes/长度`，**未断言值不出现**，名不副实（属于"看起来测了但其实没测"） | 二选一：① preview 改为**结构化脱敏预览**（如 `key:type`、字符串值用 `***`/长度代替），并让测试断言 `preview` 不含敏感值；② 若产品接受原始前缀，则回写 05 §5.8.5 明确语义并把测试改名为 `args_summary_shape`（至少不得声称"不泄露"）。不阻塞合并但需明确处理 | open |
| 4 | **重要** | `crates/zgraph-harness/src/tools.rs:385-403`（`truncate_tool_content`/`tool_error_content`）、`:234-256`（`ToolFailure::to_json` traceback 上限误用 `MAX_PROMPT_BYTES`=1 MiB） | 05 §5.6.5 要求「超 8 KiB → 截断为**合法 JSON 字符串片段**并附 `…[truncated N bytes]`」；实现直接截断紧凑 JSON 文本再拼标记，**结果不是合法 JSON**（LLM 收到的 Tool 消息/错误 JSON 无法解析）。同时 `ToolFailure::to_json` 先把 traceback 截到 1 MiB（错误的上限），再整体 8 KiB 截断，长 traceback 时同样产生非法 JSON，且浪费内存 | 按 spec 做**结构感知**截断：把超限内容降级为合法 JSON（如 `{"truncated":true,"text":"…[truncated N bytes]"}` 或把片段作为 JSON 字符串值），traceback 单独按 8 KiB 截断后再序列化；补 1 条「截断后 `serde_json::from_str` 成功」的单测 | open |
| 5 | 重要（风险） | 依赖层：`genai` → `serde_json/preserve_order` | `preserve_order` 是**全局 feature 统一**：一旦 genai 进入工作区图，所有 crate 的 `serde_json::Map` 由 BTreeMap 变为 IndexMap（键序保留插入序、内存增大）。本评审已验证 `graph_hash` 安全（`crates/zgraph-dsl/src/hash.rs:249-252` 显式 `keys.sort_unstable()`），workspace 418 测试全绿，但 trace/CLI 的 JSON 键序与既有二进制行为改变，属跨模块语义变化，未在 spec/architecture 记录 | 登记 ISSUE 记录该全局副作用与验证结论（graph_hash 不受影响、trace 输出键序变化）；docs REV-022 在 05 §5.13 记一条依赖偏差；长期可评估更换 transport 或接受 | open |
| 6 | 建议 | `crates/zgraph-harness/src/config.rs:81`（`node_timeout_ms` 字段）；`executor.rs:113`；kernel `graph.rs:248/256` | spec 05 §5.2.6 的实例超时优先级「DSL `timeout_ms` > `AgentConfig.node_timeout_ms` > `DEFAULT_NODE_TIMEOUT_MS`」只落地了头尾：kernel 在编译期用 DSL/default 决定 `CompiledNode.timeout_ms`，**AgentConfig.node_timeout_ms 解析/校验后从未被使用**（harness 用的是 `execution.node.timeout_ms`）。默认值同为 180s，故仅显式非默认配置时行为偏离 spec。作者自述属实 | 登记 ISSUE（已知限制）：M5 前需在 kernel/调度侧支持 `agent_call` 的 AgentConfig 回退，或回写 05 §5.2.6 明确「内核 deadline 权威、AgentConfig.node_timeout_ms 仅建议值」；`docs` REV-022 记录 | open |
| 7 | 建议 | `crates/zgraph-harness/src/tools.rs:742-746`（`tool_semaphore()` 独立静态 Semaphore）、`:788-794` | 工具并发使用**独立**进程级 `Semaphore(CPU×2)`，未与 `WorkerExecutor` 的全局 `max_workers` 共享：同一时刻「工具子进程 + Python 节点子进程」可各自达到 CPU×2，超出 architecture D4/§4 的全局 worker 上限语义（task-4 允许以独立信号量兜底并要求报告，作者已在模块 docstring 说明） | 登记 ISSUE（M5 统一为共享全局上限）；docs 回写在 05 §5.6/D3 落一条「v2 工具并发 = 独立全局信号量，暂不共享 WorkerExecutor 信号量」 | open |
| 8 | 建议 | `crates/zgraph-harness/src/tools.rs:537-554`（`ALLOWED_SCHEMA_KEYWORDS`） | 白名单比 05 §5.6.1 多出 `title`/`format`/`examples`/`const`（spec 允许表未列；未知关键字按 spec 应报 `unsupported_schema_keyword`）。当前为"宽松接受"，与 spec 表不一致 | 二选一：删除这 4 个关键字（严格对齐 05 §5.6.1）；或在 05 §5.6.1 回写为允许并说明理由。不阻塞（宽松不会误拒合法 schema） | open |
| 9 | 建议 | `crates/zgraph-harness/src/tools.rs:1063-1080`（`discover_worker_error`）、`:968-999`（`invalid_tool_timeout`）、`:914`（透传 py-worker reason） | discover 失败 reason 扩展为 `tool_module_import/tools_dir_missing/duplicate_tool/invalid_declaration/tool_module_timeout/tool_discover_cancelled/tool_runner_spawn/tool_worker_crash/invalid_tool_timeout`，超出 05 §5.2.7 的 reason 闭集（与 py-worker REV-019 §7.3 的跨模块项呼应） | docs REV-022 在 05 §5.2.7/§5.13 补 reason 扩展表；实现可保留（诊断价值高） | open |
| 10 | 建议 | `crates/zgraph-harness/src/constants.rs`（`MAX_LAST_CONTENT_SUMMARY_CHARS`）、`error.rs:174`（硬编码 512）、`error.rs:339-346`/`tools.rs:406-413`（`truncate_chars` 追加 `…`） | ① `MAX_LAST_CONTENT_SUMMARY_CHARS` 定义后**未使用**（0 引用），`error.rs` 直接用字面量 512；② `truncate_chars(text,512)` 在截断时返回 512 chars + `…` = **513 字符**，与 05 §5.7.4「≤512 字符」/§5.8.5「≤256 字符」字面不符（`tools.rs:1416` 测试用 `+4` 容差掩盖） | 使用常量替代 512；截断时 `take(max_chars-1)` 再追加 `…`，测试断言严格上界；不阻塞 | open |
| 11 | 建议 | `crates/zgraph-harness/src/agent_loop.rs:141-279`（run_agent，139 行）、`config.rs:384-507`（parse_model，124 行）、`tools.rs:888-962`（parse_discover_output，75 行）、`llm/fake.rs:373-438`（parse_call，66 行）、`llm/transport.rs:507-570`（map_webc_error，64 行） | architecture §2.2 规定 crates 遵守「等价约束（…函数 ≤ 60 行）」；上述 5 个生产函数超限（AST/括号计数实测），`run_agent` 为单一巨型函数，后续维护成本高 | 按阶段拆分：`run_agent` 拆 `assemble_phase`/`invoke_phase`/`tool_phase`（逻辑已天然分块）；`parse_model` 拆 model 子字段解析；其余按错误分支拆 helper。**不阻塞合并**（纯重构），建议 M4 收尾或 M5 前处理 | open |
| 12 | 建议 | `crates/zgraph-harness/Cargo.toml:22-24`（`futures-util = "0.3"`） | 新外部依赖直接写在 crate（process 模块约定：外部依赖进 `[workspace.dependencies]`，REV-012/ISSUE-030 先例）；另外 genai 默认 features（`rustls-tls`+`system-proxy`）带入 aws-lc-sys/cmake 等重依赖（`Cargo.lock` 新增 186 个包，**无既有版本变更**——已逐包比对验证） | process 分支把 `futures-util` 提升到 workspace deps（下一轮）；评估 genai `default-features=false` 的可行性（注意 `preserve_order` 是其依赖声明，无法关闭）；`cargo build` 成本/CI 时长记入 ISSUE | open |
| 13 | 建议 | `crates/zgraph-harness/src/config.rs:747-773` | V16 路径安全是**词法**检查（`normalize_lexical`，未 `canonicalize`），符号链接可指向资产根外；代码注释已如实声明为已知限制（`config.rs:751`），作者自述属实 | 登记 ISSUE（低风险：资产目录由部署方控制）；docs 在 05 §5.2.1 注明 v1 词法边界；M5 可加 canonicalize（注意跨平台/性能） | open |
| 14 | 备注 | `tools.rs:1413`（`summaries_do_not_leak_full_arguments`）、`error.rs:349`（`seven_kinds_map_to_spec_error_codes`） | 测试名与当前契约/枚举不同步：前者声称"不泄露"但未断言值；后者 kernel 已有第 8 kind（REV-018），虽有独立 ContextOverflow 用例，但"seven"命名过时 | 随 #3/#10 一并修正命名与断言；不阻塞 | open |
| 15 | 备注 | `Cargo.lock`、`crates/zgraph-harness/Cargo.toml` | `Cargo.lock` 属 process 模块但由本分支修改（作者在 commit 正文注明 Lead 已确认）；独立比对 `838e969..1d8ef79` 的 lock：**186 个新增包、0 个既有包版本变化、0 个删除**（按包名/版本解析比对），属 genai/async-trait/futures-util 依赖树的副作用，无无关改动 | 按 Lead 批准的跨模块授权处理；merge.sh rebase 后若 lock 冲突，以 `cargo metadata` 重生成 | 无阻塞 |

> 阻塞项 #1 必须清零才能给「通过」；重要项 #2/#3/#4/#5 需修复或按 §6-C3 登记 ISSUE 并给出处理结论；建议项可后置但需进台账（AGENTS §3/§7）。

## 4. 验证证据

### 4.1 范围、分支与提交元数据

```bash
cd /Users/zyzs/lczyzs/code/zgraph/.worktrees/harness-harness-dev
git status --short                        # 空（worktree clean）
git branch --show-current                 # feat/harness-m4-agent-loop
git merge-base origin/main HEAD           # 838e9696b67b793d948720b7dbc8f7fe00795679（= base = origin/main，未 rebase）
git rev-parse HEAD                        # 1d8ef79f58b7e45f03d3f8d461d260ebf3c96e7d
git diff --shortstat 838e969..1d8ef79     # 17 files changed, 10881 insertions(+), 213 deletions(-)
git diff --name-status 838e969..1d8ef79   # M Cargo.lock；M crates/zgraph-harness/{Cargo.toml,src/lib.rs}；A 15 个 src/*.rs（含 llm/{mod,fake,transport,types}.rs）
git log -1 --format='%H|%an <%ae>|%cn <%ce>|%s' 1d8ef79
# chenyuxiangg <1007423165@qq.com> / chenyuxiangg <1007423165@qq.com> / feat(harness): M4 AgentHarness（config/budget/loop/tools/genai/fake）（REV-020）
# 正文含 Review: REV-020 / Agent: harness-dev
```

- 未改 `doc/**`、`scripts/**`、`python/**`、`tests/**`、kernel/worker/dsl/trace/cli 源码；唯一跨模块文件 `Cargo.lock`（见 §3 #15）。
- `git diff --check` exit 0（check.sh 内含）。

### 4.2 测试（worktree 内，`CARGO_HOME=/Users/zyzs/lczyzs/code/zgraph/.cargo-home`）

| 命令 | 结果 |
|---|---|
| `cargo test -p zgraph-harness --quiet` | **exit 0：105 passed / 0 failed / 1 ignored**（ignored = `ac08b_real_llm_manual` 类真实模型用例；日志 `/tmp/rev020-test.log`） |
| `cargo test --workspace --quiet` | **exit 0：418 passed / 0 failed / 1 ignored**（50 个测试二进制；M3 的 worker/kernel/trace/cli/e2e 全保留，无新增失败） |
| 定向：`tools::tests::worker_tool_runner_roundtrip_with_real_python` 等 4 条（`ZGRAPH_PYTHON=.venv/bin/python`） | **4 passed**：真实 Python 子进程 discover（`add/echo/explode/sleepy/now`，`sleepy.timeout_ms=300`、`add` 缺省 30000 经 harness 补）、call `add`→`{"sum":5}`、`explode`→`exception/ValueError/boom`、`sleepy(5s)` 在 300ms 超时→`timeout`；`executor::tests::agent_call_completes_through_kernel_and_publishes_output` 断言 kernel Completed + `result` 四字段 + token 流式拼接；`unknown_tool_*` 与 `context_overflow_*` 通过 |
| `bash scripts/dev/check.sh` | **CHECK PASSED**（12 项 OK，含 `cargo fmt --all -- --check`、`git diff --check`） |
| 测试分布 | `agent_loop 20 / config 20 / tools 16 / llm/transport 12 / budget 9 / assemble 8 / error 8 / llm/fake 8 / executor 5` = 106（105 passed + 1 ignored），与提交自述一致 |

### 4.3 clippy（关键：阻塞项 #1 的证据）

```bash
# ① 本分支工作区（干净 target，独立 CARGO_TARGET_DIR）
(CARGO_TARGET_DIR=/tmp/clippy-ws-fresh) cargo clippy --workspace --all-targets -- -D warnings
# → exit 101；14 errors，全部位于 crates/zgraph-worker/src/{spawn.rs,supervisor.rs}：
#   clippy::result_large_err：`the largest variant contains at least 144 bytes`（WorkerFailure::Worker(WorkerError)）
# ② main 基线（838e969，干净 target）
(CARGO_TARGET_DIR=/tmp/clippy-ws-main-fresh) cargo clippy --workspace --all-targets -- -D warnings
# → exit 0（Finished）
# ③ 根因验证：feature 统一
cargo tree --workspace -e features -i serde_json
# → serde_json feature "preserve_order" ← genai v0.7.2 ← zgraph-harness（genai-0.7.2 Cargo.toml:453-455 无条件声明 features=["preserve_order"]）
# ④ 隔离验证：worker 单包（不带 genai 图）干净 target clippy → exit 0（证明不是 worker 源码新问题）
```

- 结论：clippy 失败由**本分支新增依赖**触发 feature 统一导致；`merge.sh` 的测试门禁不跑 clippy，但 `.github/workflows/ci.yml:20` 会在 PR/push 上失败。`crates/zgraph-harness` 自身 clippy 错误数为 **0**（14 errors 均指向 worker）。
- 说明：本机 worktree 既有 `target/` 缓存会**重放**较早工具链（rust-1.99.0 时期）保存的 `result_large_err` 诊断，导致 `cargo clippy` 在缓存 target 上"看起来"失败/成功都可能不可信；以上均为 `CARGO_TARGET_DIR` 干净 target 的实测。

### 4.4 Cargo.lock 最小性核对（独立脚本，按包名+版本解析比对）

```text
changed existing versions: []          # 0 个既有包版本变化
removed: []                            # 0 个删除
added count: 186                       # genai/reqwest/rustls/aws-lc-sys 等依赖树
git diff --shortstat 838e969..1d8ef79 -- Cargo.lock   # +2181 / -211（文本重排）
```

无无关依赖改动；新增包全部由 genai/async-trait/futures-util 引入。`Cargo.lock` 属 process 模块，Lead 已在提交正文确认。

### 4.5 旧 head `95d44c4` 两处修正的独立核对

| 修正 | 核对结果 |
|---|---|
| config 恒真断言 `assert!(x.is_empty() \|\| !x.is_empty())` | 已改为真实断言：`!loaded.api_key.is_empty()` + `assert_eq!(loaded.api_key.expose(), "fake-mode")`（`config.rs:1402-1409`） |
| executor `config_failure` 的 `elapsed_ms` | 改为 `node_timeout_ms.saturating_sub(remaining)`，`node_timeout_ms` 来自 `execution.node.timeout_ms`（DSL/default 的有效值）——语义比原先 `elapsed_ms:0` 正确；仍是"已过 deadline 时的估计值"（`node_timeout_ms - remaining`，remaining==0 时等于有效 timeout），可接受，见 §5 备注 |

### 4.6 静态边界与安全核对

| 项 | 结果 |
|---|---|
| genai 类型是否泄漏 | `grep -rn genai crates/zgraph-harness/src`：除 `llm/transport.rs` 的真实 `use genai::…` 外，其余文件仅注释提及 → **未泄漏** |
| `unsafe` / panic / unwrap | `#![forbid(unsafe_code)]` 保留；生产路径（`#[cfg(test)]` 前）`unwrap()/expect(/panic!/unreachable!` 扫描 **0 命中** |
| H1/H3（不触达 RunContext/SQLite） | `grep RunContext\|rusqlite\|sqlite` 仅命中 doc 注释；`HarnessCtx for NodeContext` 只委托 7 个只读/emit 方法（`ctx.rs:38-66`） |
| SecretString | 不实现 Debug/Display；`LoadedAgent` Debug 打印 `<redacted>`；`GenaiLlmProvider` Debug `finish_non_exhaustive`；`api_key` 仅在 `transport.rs:216` 注入 `AuthData::Key`；`LlmError/NodeError detail` 无密钥字段 |
| 流式不变量 | A1（ToolCallStart 先于执行/ToolCallEnd）、A2（token 拼接==content）、A3（call_id 一致）、A5（每轮一条 Progress）均有断言（`agent_loop.rs:690-780/767-770`）；sink 只 1:1 转发、不聚合（`ctx.rs:134-148`） |
| StreamEvent 类型 | kernel `StreamingEvent::ToolCallEnd.result_summary: Option<Value>`（`crates/zgraph-kernel/src/trace.rs` 实核），harness 传 `Some(summarize_result(...))` 一致 |
| 函数长度 | 5 个生产函数 >60 行（见 §3 #11，括号计数实测）；其余 ≤60 |
| 测试真实性 | 抽查关键用例：retry 用 `fake.requests().len()==2` 且 iterations==1；streaming 失败请求数==1；tool 参数校验失败 `runner.calls().is_empty()`；executor 走真 kernel `start_run` + `MemoryTraceSink`；真 Python roundtrip 用 `WorkerConfig` 注入 venv/python 并断言真实 sum/异常/超时；未见 tautology（旧 head 的唯一恒真断言已被作者修正，§4.5） |

## 5. 遗留问题与 ISSUE/文档回写建议（供 Lead 登记；本评审不改台账）

**作者自述核对**（Lead 要求）：

| 自述/已知限制 | 核对结论 |
|---|---|
| `node_timeout_ms` 内核未回退 | **属实**：kernel 用 DSL/default 决定 `CompiledNode.timeout_ms`（`crates/zgraph-kernel/src/graph.rs:37-38,248,256`），AgentConfig 的 `node_timeout_ms`（`config.rs:81`）解析后未被任何生产路径使用；spec 05 §5.2.6 优先级只落地头尾 |
| prompt 路径词法安全 | **属实**：`normalize_lexical` 不 `canonicalize`，符号链接可逃逸；`config.rs:747-751` 注释如实声明，测试覆盖 `..` 逃逸 |
| `futures-util` 依赖 | **属实**：`transport.rs:22` 唯一使用 `futures_util::StreamExt`（genai `ChatStream` 只实现 `futures::Stream`）；未进 workspace deps（§3 #12） |
| D13–D18 偏差记录 | 代码注释已覆盖 D3（工具子进程）/取消轮询/HarnessCtx seam/重试编排位置/空响应裁定；建议 docs REV-022 的 §5.13 编号与内容对齐并**新增**：D18 `AgentConfig.node_timeout_ms 未落地（内核 deadline 权威）`、D19 `genai→serde_json/preserve_order 全局副作用`、D20 `工具并发=独立全局信号量`、D21 `discover/call failure reason 扩展`、D22 `args_summary preview 语义（按 #3 结论）` |

**建议登记 ISSUE（按级别）**：

- **P1/重要**：`HARNESS_CI_CLIPPY_RESULT_LARGE_ERR`（阻塞 #1，worker/process 修复后可关）；`HARNESS_TOOL_VISIBILITY`（#2 可见性）；`HARNESS_ARGS_SUMMARY_LEAK`（#3 preview 泄露 + 测试名）；`HARNESS_TOOL_TRUNCATE_INVALID_JSON`（#4 8 KiB 截断非合法 JSON）；`SERDE_JSON_PRESERVE_ORDER_SIDE_EFFECT`（#5 全局语义风险）。
- **P2/P3**：`AGENT_CONFIG_NODE_TIMEOUT_NOT_APPLIED`（#6）；`TOOL_CONCURRENCY_SEMAPHORE_SHARING`（#7）；`SCHEMA_KEYWORD_WHITELIST_SUPERSET`（#8）；`DISCOVER_REASON_SUPERSET`（#9）；`SUMMARY_TRUNCATE_CONSTANT/LENGTH`（#10）；`HARNESS_FN_LENGTH`（#11）；`FUTURES_UTIL_WORKSPACE_DEP`（#12）；`PROMPT_PATH_SYMLINK`（#13）。

**docs REV-022 回写清单（与代码对齐）**：05 §5.1.2 组件/实际模块；§5.2.1 fake 跳过 api_key；§5.3 取消=轮询、重试编排在 Harness、`LlmProvider::name()="genai"`；§5.4/§5.5 空响应裁定（连续 2 次）；§5.6 worker 子进程/discover-call payload/缺省 timeout 30000/无跨调用缓存/独立全局信号量；§5.6.5 8 KiB 截断语义（按 #4 结论）；§5.7 `TOOL_ERROR`/`ContextOverflow` 第 8 kind/`result_summary: Option<Value>`；§5.8 1:1 emit；§5.9 `HarnessExecutor`=BuiltinHarness；§5.11 常量已回写 00 §0.5；新增 §5.13 D13–D22（含本评审新增项）。

## 6. 条件关闭记录（"有条件通过"必须关闭后才能合并）

| 条件 | 级别 | 关闭证据（需 Lead/作者补齐后由评审追加复审段） | 关闭日期 |
|---|---|---|---|
| **C1** 修复 workspace clippy CI 回归（阻塞 #1）：worker/process 侧 `#[allow(clippy::result_large_err)]` 或 `Box<WorkerFailure>`，使**干净 target** 下 `cargo clippy --workspace --all-targets -- -D warnings` 在**含 genai 的工作区** exit 0 | 阻塞 | worker/process 修复 commit + 干净 `CARGO_TARGET_DIR` 实测 exit 0（贴命令与结果） | 未关闭 |
| **C2** 强制 `AgentConfig.tools` 可见性（重要 #2）：未授权但已注册的工具不得执行，`HARNESS_UNKNOWN_TOOL.available_tools` 用可见集合，补对应用例 | 重要 | harness 修复 commit + 新增单测通过 + 复审确认 | 未关闭 |
| **C3** 对 §3 的重要/建议项建立 `doc/issues.md` 台账并纳入 docs REV-022 回写（至少 #3/#4/#5/#6/#7/#9；#8/#10/#11/#12/#13 可随 M4 收尾） | 重要（流程） | Lead 合并时/REV-022 提交中可见 ISSUE 行与 docs 更新 | 未关闭 |

> 关闭 C1/C2 后，全分支 REV-020 结论可由「有条件通过」改为「通过」并允许 `merge.sh`；C3 的台账/回写可与合并同批完成（merge.sh 只校验评审记录结论，不校验 ISSUE，但 AGENTS §7 要求）。

## 7. 复审（2026-10-30，reviewer-b，C2 修复，HEAD `1fcbc1d`）

> 作者 `harness-dev` 在 `1d8ef79` 之上新增 `1fcbc1d fix(harness): 强制 AgentConfig.tools 工具可见性（REV-020 C2）`。本节为**增量复审**（按 AGENTS §6 不删除/不改写 §1–§6 原结论与证据）；C1（worker clippy）仍由 Lead 在 worker 分支修复，尚未关闭，故**全分支结论仍为「有条件通过」**。

- **增量范围**：`1d8ef79..1fcbc1d`（1 commit，`git diff --shortstat` = **3 files changed, 39 insertions(+), 13 deletions(-)**：`agent_loop.rs`、`executor.rs`、`tools.rs`；无 `Cargo.lock`/文档/其它模块改动）；base 仍为 `origin/main` `838e969`（未 rebase）。提交 author/committer `chenyuxiangg <1007423165@qq.com>`，正文含 `Review: REV-020` / `Agent: harness-dev`，**单提交单目的**。

### 7.1 C2 关闭核对

| 核对项 | 独立核对结果 | 状态 |
|---|---|---|
| 可见集来源 | ✅ `run_agent` 入口取 `visible_tools: &[ToolDef] = &run.agent.tools`（`agent_loop.rs:148-152`）——即 `AgentConfig.tools` 经 `resolve_tools` 解析后的**本 Agent 可见集**，不再使用全量 registry | fixed |
| 未知/未授权统一 Crashed | ✅ 执行前查找改为 `visible_tools.iter().find(|tool| tool.name == call.name)`（`:252-258`）；未命中（无论"未注册"还是"已注册但未授权"）→ `HARNESS_UNKNOWN_TOOL`（RunFailure::Harness）→ Crashed，**不执行**、不 emit `ToolCallStart` | fixed |
| `available_tools` 语义 | ✅ 改为 `&visible_tool_names`（`:150-152`，仅可见工具名；`error.rs::unknown_tool` 侧仍上限 32） | fixed |
| 执行路径无法触碰全量工具面 | ✅ `AgentRun` 删除 `registry` 字段（`:112-137`）；executor 侧删除 `registry: &registry` 传参（`executor.rs:141-151`）；`registry` 仅保留在 executor 的 `load_agent` 调用中（解析 `AgentConfig.tools` 必需）——编译期即保证执行路径无全量表引用 | fixed |
| 回归单测真实性 | ✅ `agent_loop::tests::registered_but_unauthorized_tool_is_unknown_and_not_executed`（`:1213-1238`）：visible={a}、discovery 含 a/b、LLM 调 b → 断言 `kind==UnknownTool`、`detail.tool=="b"`、`detail.available_tools==["a"]`、`FakeToolRunner.calls()` 为空、`tool_events` 为空；定向运行 **1 passed**（0 filtered out 之外 106 filtered）。**备注**：测试辅助 `execute(..., _registry, ...)` 的 `registry` 参数在修复后已不参与执行路径（`_registry` 仅为保持其它用例签名），该用例中"registry 含 b"对行为不再有影响——断言本身有效（可见集由 `agent.tools` 决定），但参数可后续清理，属测试卫生建议，不阻塞 | fixed（含 1 条建议） |
| 测试名清理 | ✅ `tools.rs:1413` `summaries_do_not_leak_full_arguments` → `args_summary_has_shape_and_limited_preview`（**仅改名，行为未变**；#3 的语义修复仍按 C3 由 Lead 安排，未扩大范围）——与 §3 #14 一致 | fixed（部分：#14 的 error.rs `seven_kinds...` 命名未动，仍留 C3） |

### 7.2 复审验证证据（worktree 内）

```bash
cd /Users/zyzs/lczyzs/code/zgraph/.worktrees/harness-harness-dev
git status --short                     # 空（clean）
git log --oneline -2                   # 1fcbc1d → 1d8ef79
git diff --shortstat 1d8ef79..1fcbc1d  # 3 files changed, 39 insertions(+), 13 deletions(-)
```

| 命令 | 结果 |
|---|---|
| `cargo test -p zgraph-harness --quiet` | **exit 0：106 passed / 0 failed / 1 ignored**（较原 105 +1，即新增 C2 回归用例；日志 `/tmp/rev020-c2-test.log`） |
| `cargo test -p zgraph-harness --quiet agent_loop::tests::registered_but_unauthorized_tool_is_unknown_and_not_executed` | **1 passed / 0 failed** |
| `bash scripts/dev/check.sh` | **CHECK PASSED**（含 fmt/`git diff --check`） |

### 7.3 复审结论

- **C2：已关闭**（代码 + 回归单测 + 独立复跑均通过；执行路径不再持有全量工具表）。
- **C1：仍未关闭**（worker clippy 回归；Lead 已建 `fix/worker-m4-large-err-lint` / REV-021 处理，待其合入后 harness 需 rebase 并以最终 HEAD 复跑 clippy）。
- **C3：仍未关闭**（ISSUE 台账 + docs 回写，Lead 收尾处理）。
- **全分支结论维持「有条件通过」**；C1 关闭（+ harness rebase 后干净 target `cargo clippy --workspace --all-targets -- -D warnings` exit 0）并经本轮复审追加确认后，方可改「通过」并允许 `merge.sh`。

## 8. C1 关闭确认（2026-10-30，reviewer-b，worker `94e1f8a` + 组合干净 target 复验）

> worker 分支 `fix/worker-m4-large-err-lint`（REV-021，作者 `lead`）以「`WorkerFailure::Worker(Box<WorkerError>)` 装箱」消除 `clippy::result_large_err`。本节记录 C1 的独立关闭证据；**C3 仍未关闭**（ISSUE 台账/docs 回写），故本记录暂不把总结论改为「通过」，待 harness rebase 到含 worker 修复的 main 后做最终复核再改。

### 8.1 独立复验方式与结果（不改仓库文件）

以 harness 本分支 HEAD `1fcbc1d` + worker 补丁 `838e969..94e1f8a` 构造**组合干净树**（`/tmp/m4-verify`，来自 `git archive` + `git diff | patch`；`CARGO_TARGET_DIR=/tmp/m4-verify-target`，与 CI 同款命令）：

```bash
cargo clippy --workspace --all-targets -- -D warnings
# → exit 0；Finished dev profile（日志 /tmp/rev021-combo-clippy.log，含 Checking genai/zgraph-harness/zgraph-worker）

# 因果复现：同一棵树反向回退 worker 装箱补丁后再跑
# → exit 101；14 处 clippy::result_large_err 全在 crates/zgraph-worker/src/{spawn,supervisor}.rs
#   （日志 /tmp/rev021-combo-prefix.log）→ 证明回归由该变体尺寸触发、装箱即消除
```

结论：**在 harness+C1 修复的组合树上，CI 的 workspace clippy 已恢复 exit 0**；C1 的修复选择（Boxing 而非 allow 抑制）合理，worker 自身 `cargo test -p zgraph-worker` 58 passed / `check.sh` PASS（详见 REV-021）。

### 8.2 条件状态更新

| 条件 | 状态 | 证据/说明 | 关闭日期 |
|---|---|---|---|
| C1 workspace clippy 回归 | **已关闭（待最终 rebase 复核）** | worker `94e1f8a`（REV-021 评审通过）+ 组合干净 target `cargo clippy --workspace --all-targets -- -D warnings` **exit 0** | 2026-10-30 |
| C2 AgentConfig.tools 可见性 | **已关闭** | harness `1fcbc1d`（§7 复审） | 2026-10-30 |
| C3 ISSUE 台账 + docs 回写 | **未关闭** | Lead 在 M4 收尾（REV-024）开 ISSUE 与 docs（REV-023）回写；ISSUE-073 尚未出现在 `doc/issues.md`（当前最大 072），一并跟踪 | — |

### 8.3 下一步（最终复核清单）

1. Lead 将 worker `94e1f8a` 合并入 main 后，harness 分支 `git rebase origin/main`（或 merge.sh 自动 rebase），给出**最终 HEAD**。
2. 评审在**干净 target** 下复核：`cargo clippy --workspace --all-targets -- -D warnings` exit 0、`cargo test --workspace` 无新增失败、`cargo test -p zgraph-harness` 106 passed/1 ignored、`bash scripts/dev/check.sh` PASS、worktree clean。
3. 以上通过 + C3 台账/回写可见后，在本记录追加最终复审段，将 §1 结论由「有条件通过」改为「通过」（保留历史不删），再交 Lead `merge.sh`。

## 9. 最终复审与结论变更（2026-10-30，reviewer-b，最终 HEAD `6de2b64`）

> 按 AGENTS §6 保留 §1「有条件通过」与 §6/§8 条件表历史，本节为**最终复审**；自本节起，本记录结论为「通过」。

- **最终 HEAD**：`6de2b64`（harness 分支在 worker 修复合入 main 后 rebase）；base = `origin/main` = `8b67df7`（含 REV-021 worker `94e1f8a`）；`git diff --shortstat 8b67df7..6de2b64` = **17 files changed, 10907 insertions(+), 213 deletions(-)**（harness 15 源文件 + Cargo.toml/lib.rs + `Cargo.lock`；与 rebase 前 `1fcbc1d` 内容等价，仅 rebase 基线变化）；worktree clean；提交元数据与 `Review: REV-020` / `Agent: harness-dev` 保持合规。
- **最终干净 target 复核**（`CARGO_TARGET_DIR=/tmp/rev020-final-target`，全新目录，`CARGO_HOME=<repo>/.cargo-home`）：

| 命令 | 结果 |
|---|---|
| `cargo clippy --workspace --all-targets -- -D warnings` | **exit 0 / 0 error**（含 genai 工作区；日志 `/tmp/rev020-final-clippy.log`） |
| `cargo test --workspace --quiet` | **exit 0：419 passed / 0 failed / 1 ignored**（50 个测试二进制；M3 基线 313+ 与 M4 新增全部保留；日志 `/tmp/rev020-final-ws-test.log`） |
| `cargo test -p zgraph-harness --quiet` | **exit 0：106 passed / 0 failed / 1 ignored**（含 C2 回归用例；日志 `/tmp/rev020-final-harness-test.log`） |
| `bash scripts/dev/check.sh` | **CHECK PASSED**（fmt / `git diff --check` 等 12 项） |
| `git status --short` | 空（clean） |

- **条件最终状态**：

| 条件 | 状态 | 最终证据 |
|---|---|---|
| **C1** workspace clippy 回归（阻塞） | **已关闭** | worker `94e1f8a`（REV-021 通过）；rebase 后干净 target `cargo clippy --workspace --all-targets -- -D warnings` **exit 0**（§8 组合复验 + 本节最终复验双重确认） |
| **C2** AgentConfig.tools 可见性（重要） | **已关闭** | harness `1fcbc1d`（§7 复审：可见集执行/未授权→UnknownTool/`available_tools` 仅可见 + 回归单测）；rebase 后 106 passed |
| **C3** ISSUE 台账 + docs 回写（流程） | **转交跟踪（不再阻塞本分支合并，Lead 已明确安排）** | Lead 安排在 M4 收尾 `REV-024` 开 ISSUE（args_summary preview、8 KiB 非法 JSON、preserve_order 全局副作用、node_timeout_ms 未回退、工具并发 per-run、function>60 行、futures-util workspace、ISSUE-073 worker 根因）并由 `REV-023`（docs）回写 05 §5.13.3；评审将在 REV-023/REV-024 核对登记是否覆盖 |

**最终结论：通过**

- 阻塞项（C1）与功能正确性条件（C2）均已独立复验关闭；最终干净 target 的 clippy/全量测试/check.sh 全绿；分支 scope 仅 harness + 经 Lead 确认的 `Cargo.lock`。
- 剩余 C3 为**台账/文档跟踪项**（非代码缺陷，Lead 已排入 REV-023/REV-024）；本记录 §3 的 15 条发现已给出分级与处理建议，§5 已列 ISSUE 清单供收尾登记。
- 允许执行 `merge.sh feat/harness-m4-agent-loop`（前提：本记录已提交到 main 工作区/分支，merge.sh 会校验「结论：通过」）。
- 后续（不在本分支）：REV-022 e2e（harness 合并后 rebase 再评）、REV-023 docs 回写（核对 §5.13.3 覆盖本记录 §5 清单）、REV-024 M4 收尾（核对 ISSUE 台账覆盖 #1–#13 与 REVIEW 索引）。
