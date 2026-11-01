# REV-098 · harness（含 cli/kernel 机械）· V3 MCP 实现（rmcp 双 era + 工具桥接 + `--allow-mcp`）

> **跨模块声明**（Lead 已批准 + 第二评审追加）：本分支改动 `crates/zgraph-harness`（主体）、`crates/zgraph-cli/src/run_cmd.rs`（`--allow-mcp` + 预检 + `close_mcp` 调用）、`crates/zgraph-kernel/src/types.rs`（`HarnessErrorKind::Mcp`/`CODE_HARNESS_MCP_ERROR` append-only）、`Cargo.toml`/`Cargo.lock`（`rmcp =3.8.0` feature-gated）。评审人只新增本评审记录，未改实现/测试。
>
> **最终状态（2026-11-02）**：首轮（`815abea`）「有条件通过」→ 作者收口 **`15f3d18`**（C1/C2 + P2 加固）→ 评审人重跑全部门禁 + 只读复核：C1/C2 closed，结论改为「通过」（逐条见 §6；P2⑥ 残余与 P3 见 §6.3，非阻塞）。

- **分支**：`feat/harness-mcp`
- 分支: feat/harness-mcp
- **作者**：`kernel-dev`（harness 角色；`18bf405` 主实现 + `815abea` run 结束 `close_mcp` 修复；正文 `Review: REV-098` / `Agent: kernel-dev`）
- **评审人**：`reviewer-a`（主审；Lead 追加裁决/第二评审）
- **变更范围**：`69aeb87..815abea`（**14 files, +2777/−16**）：`crates/zgraph-harness/src/{mcp/mod.rs(1291),mcp/real.rs(321),mcp/testing.rs(191),config.rs,executor.rs,agent_loop.rs,constants.rs,error.rs,lib.rs}`、`crates/zgraph-cli/{Cargo.toml,src/run_cmd.rs}`、`crates/zgraph-kernel/src/types.rs`、`Cargo.lock`
- **评审时间**：2026-11-01
- **关联**：DR-010（REV-096 冻结）；spec 00 §0.3/§0.4/§0.5、05 §5.14.2、06 §6.14、07 AC-19 a–g；task-59/60；REV-096（V1 条件关闭通过）

## 1. 结论

**结论：通过**

> **结论历史**：首轮（2026-11-01，基准 `815abea`）为「**有条件通过**」——P1 条件 C1（同一 run 多 agent 的 MCP plan/allow 交叉复用）与 C2（AgentConfig.mcp 结构/allow 错误码未收敛到 `HARNESS_MCP_ERROR`）。作者收口 `15f3d18`（C1/C2 + P2 加固）后，评审人重跑全部门禁 + 只读复核，C1/C2 均关闭（§6.1），P2①–⑤、⑦–⑨ 关闭（§6.2），结论改为**通过**；**P2⑥（server 名归一化）为唯一 P2 残余**、P3 见 §6.3（均在 P2 一般/P3 建议级别，不阻塞）。
>
> 首轮核对（历史保留）：默认（无 MCP 配置）路径与既有测试全部保持；feature/依赖、registry 拒绝矩阵、AgentConfig/`--allow-mcp`、McpRun 桥接、双 era、trace 不 v7、测试与门禁主体成立。P0=0；P1 条件 C1–C2；P2/P3 见 §3。以下 §1.1/§2 为**首轮核对快照**（⚠️ 项已关闭）。

### 1.1 合同核对（对照 DR-010 / 05 §5.14.2 / 07 AC-19）

| 合同项 | 实现/证据 | 结论 |
|---|---|---|
| **依赖/feature** | `zgraph-harness`：`mcp` optional feature（默认关）→ `rmcp = "=3.8.0"`（client + `transport-child-process` + `transport-streamable-http-client-reqwest`）+ `process-wrap 10.0` + `http 1`；`zgraph-cli` `default=["mcp"]`；`Cargo.lock` rmcp **3.8.0** 精确锁定（process-wrap 10.0.1/http 1.5.0）；`cargo tree` 默认含 rmcp、`--no-default-features` 为 0；`cargo check -p zgraph-cli --no-default-features --locked` 通过 | ✅ |
| **registry 拒绝矩阵** | `mcp/mod.rs:273-560`：文件缺失=无 server；`mcpServers` map；name `^[a-z0-9][a-z0-9_-]{0,63}$`；`command` XOR `url`（同时/都无 → config_invalid）；args/env/headers/`${ENV}`（未定义→`env_undefined`，错误不含值）；https-only（`http://` 需 `allow_insecure_http:true`）；timeout 1..=600000；未知顶层/server 字段拒绝；fail-closed | ✅（边界数据见 P2-2/P3） |
| **AgentConfig.mcp + `--allow-mcp`** | `config.rs:435-560` additive `mcp:{servers,allow}`（servers≤16，未知子键拒绝）；`mcp__` 工具**必须显式**在 `tools`；allow 交集（`map_tools`，`:681-740`）；`resolve` fail-closed（`not_allowed`/`server_not_found`/`allow_invalid`/`name_conflict`）；CLI `run_cmd.rs:885-900`：未传 `--allow-mcp` 且图中 agent 配置 MCP → **run 启动前 exit 3（`EXIT_ENTRY_INPUT`）、不产生 run、不启 server**，stderr `error[HARNESS_MCP_ERROR] reason: not_allowed`；开启时 Loading 打印无 env 值启动命令 | ✅（错误码一致性见 C2；allow≤16 见 P2-2） |
| **client/工具桥接** | `McpClient`/`McpClientFactory` seam + `McpRun`（每 run 每 server 懒连、tools/list run 级缓存、换 run 关闭旧连接，`mcp/mod.rs:200-300/779-970`）；映射 `mcp__<server>__<tool>`；description≤1024；inputSchema 原样；**annotations 不读**；`tools/call` 自包 timeout（server `timeout_ms` 默认 30s）；MRTR 10（`call_tool_with_mrtr_max_rounds`）；`input_required` → 工具错误 `input_required_unsupported`；结果 **32768 chars** 截断；isError/协议错误 → `TOOL_ERROR` 喂回（不 fail run）；Closed 同 run 重启 ≤1；合法工具名/schema 过滤 + warn | ✅（P2/P3 细节见 §3） |
| **双 era** | `real.rs:137-150` `ClientLifecycleMode::Auto { preferred V_2026_07_28, legacy V_2025_11_25 }`（rmcp 原生）；stdio/HTTP 均使用 | ✅ |
| **进程/并发/取消** | stdio：`process-wrap` 进程组（unix）+ `kill_on_drop` + stderr 持续读 ≤8192（不写 trace）；HTTP：16 并发/control 5s；框架工具仍串行（`McpRun` 状态锁）；取消经 `tokio::select`/`notify_cancelled`/关闭 SSE；`McpRun::shutdown` 关闭全部 client | ✅（真实进程组回收与 cwd 见 P2-3/V5） |
| **close_mcp（#3 修复）** | `executor.rs:186-198` `close_mcp()` 取走 run 槽 → `McpRun::shutdown`，**幂等**；CLI `run_cmd.rs:938`（start_run 错误出口）与 `:961`（`wait_and_report` 全部出口 Completed/Failed/Cancelled/`--json`/human）均调用；feature 关/无 MCP → no-op；Fake 测试记录 shutdown 0→1→1 | ✅ |
| **trace 不 v7** | 无 trace crate/schema 改动；MCP 工具走既有 `ToolCallStart/End`（`agent_loop.rs:374-412`），`tool_name=mcp__<server>__<tool>`、`call_id` 为 LLM 原生 id、summary 走既有截断；不写 args/结果全文与密钥（依赖既有 summarize 口径） | ✅ |
| **测试/门禁（评审实跑）** | harness 默认 **200/0/1 ignored**；`--features mcp` **200/0/1**；cli **223/0**；e2e **106/0/2**；workspace **970/0/3**；`cargo check -p zgraph-cli --no-default-features --locked` ✓；clippy `-D warnings` ✓；fmt ✓；`check.sh` **PASSED** | ✅ |
| **默认路径不变** | 无 MCP 配置不读 registry/不建 MCP client（`McpRun=Nome`，`list_tools` 不调用）；`load_agent_with_options` 仅 `mcp` 段存在时读 registry；既有 golden/e2e/trace/退出码测试全部通过；`close_mcp` no-op | ✅ |

## 2. 四维评审

| 维度 | 结论 | 依据 |
|---|---|---|
| 功能正确性 | ⚠️ 有条件 | 主链路与冻结设计一致；但 **同一 run 多 agent 的 MCP plan/allow 被首个 agent 复用**（P1-C1），以及 **AgentConfig.mcp 结构/allow 非法时的错误码未收敛到 `HARNESS_MCP_ERROR`**（P1-C2）；其余为 P2/P3 |
| 代码结构合理性 | ✅ | `mcp/` 三分（mod=领域+seam、real=rmcp、testing=Fake）清晰；`McpClient`/`McpClientFactory` seam 可测；`config/executor/agent_loop/cli` 分层合理；feature gating 单一出口（`default_factory`）；kernel 仅 append-only 码/枚举 |
| 逻辑性 | ✅（C1/C2 例外） | 懒连接/run 缓存/换 run 关闭、重启≤1、幂等 shutdown、取消竞速、serial 工具执行；`--allow-mcp` 预检 fail-closed；无 MCP 零 IO；错误 reason 闭集基本对齐（C2 例外） |
| 可读性 | ✅（P3 备注） | 模块/字段注释引用 DR 章节与行号；`close_mcp` 注释明确幂等/出口；P3-1 有一处 doc 注释错位 |

## 3. 发现清单

### 3.1 P1 条件（必须修复或由 Lead 明确接受并登记 ISSUE）

| # | 位置 | 问题 | 建议 | 状态 |
|---|---|---|---|---|
| C1 | `crates/zgraph-harness/src/executor.rs:493-540`（`prepare_mcp_run`；`mcp_run` 槽 `:84/:502`） | **同一 run 内不同 agent 的 MCP plan/allow 交叉复用**：`mcp_run` 仅按 `run_id` 缓存；第二个 agent_call（不同 agent）复用首个 agent 的 `McpRun` → `list_tools` 只发现首个 plan 的 servers、`map_tools(run.plan(), …)` 用首个 agent 的 `allow`。后果：①第二个 agent 请求另一个 server → 误报 `server_not_found`；②首个 plan allow 为空而第二个 agent 有 allow → **第二个 agent 的 allow 被绕过**（可调用未列入其 allow 的工具）；③`tools/list` 缓存不含第二个 plan 的 server。AC-19 c 测试未覆盖多 agent 场景 | 每 run 共享「server 名→client/tools 缓存」但 **plan/allow 必须按 agent 隔离**（或 `mcp_run` 以 `(run_id, agent_id)`/plan 指纹为键，plan/allow 变化时关闭重建）；补测试：同一 run 两 agent 不同 `servers`/`allow` → 各自工具集正确、无 allow 绕过、无错误 server_not_found | **open** |
| C2 | `crates/zgraph-harness/src/config.rs:435-560`（`parse_mcp`/`parse_mcp_name_list`，使用 `config_error`→`HARNESS_CONFIG_LOAD`） | **AgentConfig.mcp 结构/allow 非法时错误码未收敛到 `HARNESS_MCP_ERROR`**：`mcp` 非 object、未知子键、非法 server 名、servers>16、非法 allow 模式均返回 `HARNESS_CONFIG_LOAD{reason:unsupported_value/out_of_range/…}`；DR-010 §8/00 §0.4 冻结「MCP 配置非法 → 单一 `HARNESS_MCP_ERROR{reason:"config_invalid"}`（名称冲突/allow 非法等分别 reason）」；task-59 亦要求 allow 非法 → `HARNESS_MCP_ERROR`。当前同一逻辑类错误被拆成两个码（parse 阶段 CONFIG_LOAD，resolve 阶段 MCP_ERROR），AC-19 c 未覆盖 | 将 `parse_mcp` 的结构/名称/数量/allow 模式错误改为 `HarnessError::mcp_error("config_invalid"/"allow_invalid", …)`；补 config 测试断言 `kind=Mcp/code=HARNESS_MCP_ERROR`。若 Lead 决定接受现有拆分，则登记 ISSUE 并在 V5 as-built 说明（本条件按 ISSUE 关闭） | **open** |

### 3.2 P2（一般；建议 V3 或 V5-docs 修正，不单独阻塞）

| # | 位置 | 问题 | 建议 |
|---|---|---|---|
| P2-1 | `constants.rs:168-189` vs `spec 00:306-313` | 常量名/缺失与冻结 spec 不一致：`MCP_DEFAULT_CALL_TIMEOUT_MS` vs `MCP_DEFAULT_TIMEOUT_MS`；**缺 `MCP_TOOLS_LIST_TIMEOUT_MS`**（`real.rs:291` list 用 `MCP_INIT_TIMEOUT_MS`）；`MCP_HTTP_MAX_CONCURRENT_REQUESTS` vs `MCP_HTTP_MAX_CONCURRENCY`；`MCP_HTTP_CONTROL_TIMEOUT_MS` vs `MCP_CONTROL_REQUEST_TIMEOUT_MS` | 按 00 §0.5 重命名/补常量（tools/list 用独立常量）；或 V5-docs 记录偏差并统一（推荐前者） |
| P2-2 | `config.rs:496-560` | `mcp.allow` 未限长（`enforce_limit` 仅 servers；task-60 要求 servers/allow ≤16） | allow 同样 ≤ `MAX_MCP_SERVERS`，补拒绝测试 |
| P2-3 | `real.rs:150-160` | stdio **cwd 未设 `<assets-dir>`**（DR-010 §4 要求；当前继承进程 cwd） | spawn 时 `current_dir(assets_dir)`（`McpServerSpec` 需带 cwd 或 factory 接收 assets_dir）；V5 e2e 覆盖 |
| P2-4 | `mcp/mod.rs:437-445` + `is_valid_server_name` | server 名正则允许 `__`，但 `parse_full_tool_name` 用 `split_once("__")` → `mcp__a__b__tool` 解析为 server `a`/tool `b__tool`，**含 `__` 的 server 名不可用/误解析** | 拒绝含 `__` 的 server 名（config_invalid）或改用「先匹配已配置 server 名」的解析；补测试 |
| P2-5 | `mcp/mod.rs:129-150`；`executor.rs:513` stderr notice | `display_command` 对 HTTP 直接打印完整 URL（可含 userinfo 凭据）、stdio 打印全部 args（args 可能含 key）→ 与「不落密钥」冲突 | URL 去 userinfo/query；args 打印前脱敏或仅打印命令+arg 数；补测试（无密钥出现在 stderr） |
| P2-6 | `mcp/mod.rs:974-982` vs `tools.rs:44-46` | MCP 函数名允许**大写与 `-`**、长度≤64；V2 `ToolDef` 合同为 `^[a-z][a-z0-9_]{0,63}$`；server 名允许 `-` → 产出的 `mcp__a-b__tool` 违反 ToolDef 合同；>64 字符被静默跳过（仅 warn，未在 DR 写） | 规范化（`-`→`_`/小写）或按 ToolDef 合同拒绝并记录；在 DR-010/05 as-built 说明长名跳过口径 |
| P2-7 | `mcp/mod.rs:742-747/879-970` | `truncate_result` 仅追加文本标记，无机器可读 `truncated:true`（DR-010 §3 字面要求）；`execute_tool` 的 TOOL_ERROR reason 使用 `mcp_init_failed`/`transport_closed` 等，未列在 HARNESS_MCP_ERROR 闭集（属 TOOL_ERROR 载体，但易与闭环 reason 混淆） | 结果结构补 `truncated` 标记（或 V5 docs 确认文本标记口径）；TOOL_ERROR reason 明示独立 taxonomy/文档化 |
| P2-8 | `real.rs:246-266` | `call_result_text` 先拼接完整内容再在 `McpRun` 截断 → 恶意/超大 server 响应可先占内存（无流式/大小限制）；`list_tools` schema 亦无大小上限 | 在读取/拼接时按 `MCP_TOOL_RESULT_MAX_CHARS` 上限早停；tools schema 大小上限或 warn；V5 安全 spike 覆盖 |
| P2-9 | 测试覆盖 | 缺：`FakeMcpClient::with_delay` 超时用例（定义未使用）；多 agent 不同 plan（C1）；allow>16；`__` server 名；CLI `--allow-mcp` 端到端 exit3/不产生 run/no server（仅 `graph_agent_using_mcp` 单测）；无 feature+配置 MCP 的 Loading 失败用例；真实 rmcp/进程组（V5 AC-19 a/b/d） | 补 crate-level 超时/多 agent/CLI 集成测试；V5 e2e 覆盖真实传输 |

### 3.3 P3（建议）

- P3-1：`run_cmd.rs:966-970` `graph_has_human` 的 doc 注释被新函数 `graph_agent_using_mcp` 继承（`graph_has_human` 失去注释）——调整注释位置。
- P3-2：`map_tools` 对重复 requested `mcp__` 条目未去重，可能产生重复 `ToolDef`/LLM schema；建议去重或报错。
- P3-3：`ClientSlot.restarts` 仅统计、未参与限流（实际由 `attempt` 控制）；可删或用于诊断。
- P3-4：`McpRegistry::from_value` 顶层/字段迭代顺序依赖 `serde_json::Map` feature（多错时首错顺序可能不同）；与既有 harness 配置同类，非本分支引入。
- P3-5：`MCP_REGISTRY_MAX_BYTES=256KiB`、`timeout_ms` 1..=600000、`MCP_MRTR_MAX_ROUNDS`/`MCP_STDERR_MAX_BYTES`/`MAX_MCP_*` 未入 00 §0.5（V5 docs 可补）。
- P3-6：CLI 预检 `agent_uses_mcp` 直接 `fs::read` 不限大小（未用 `MAX_AGENT_CONFIG_BYTES`）；预检对不可读/非法 agent 跳过（Lead 已接受 fail-closed）——可注释说明。
- P3-7：`--allow-mcp` stderr 错误串含对齐空格（`agent '{id}'                  configures`），建议整理为单行消息。
- P3-8：`MCP` 常量 `MCP_INIT_TIMEOUT_MS` 同时用于 connect 与 tools/list（行为等值 10s，但语义应分离，见 P2-1）。

## 4. 验证证据

评审人**亲自执行**（worktree `/Users/zyzs/lczyzs/code/zgraph/.worktrees/harness-harness-dev`，HEAD `815abea`，基于 `main = origin/main = 69aeb87`，worktree 干净；`CARGO_HOME=<repo>/.cargo-home`）：

```bash
# 1) 测试（默认 / feature 双构建 / workspace）
cargo test -p zgraph-harness                 # 200 passed / 0 failed / 1 ignored
cargo test -p zgraph-harness --features mcp  # 200 passed / 0 failed / 1 ignored
cargo test -p zgraph-cli                     # 223 passed / 0 failed
cargo test -p zgraph-e2e                     # 106 passed / 0 failed / 2 ignored（既有 real-LLM manual）
cargo test --workspace                       # 970 passed / 0 failed / 3 ignored（与 Lead 口径一致）
cargo check -p zgraph-cli --no-default-features --locked   # 通过
cargo clippy --workspace --all-targets --locked -- -D warnings  # clean
cargo fmt --all -- --check                   # exit 0
bash scripts/dev/check.sh                    # CHECK PASSED

# 2) feature/依赖
grep -A3 'name = "rmcp"' Cargo.lock          # rmcp 3.8.0（checksum 锁定）；process-wrap 10.0.1、http 1.5.0
cargo tree -p zgraph-cli -e normal --locked | grep -c rmcp              # 1（默认 feature）
cargo tree -p zgraph-cli -e normal --no-default-features --locked | grep -c rmcp  # 0

# 3) 关键代码复核（只读 grep/阅读）
# - registry 解析/拒绝矩阵（mcp/mod.rs:273-560）、resolve fail-closed（:349-430）
# - map_tools 白名单/allow/名称冲突（:681-740）；McpRun list/call/重启/截断（:779-970）
# - real.rs 双 era Auto（137-150）、stdio process-wrap/stderr（150-200）、timeout/MRTR（246-266）、shutdown（311-318）
# - executor close_mcp 幂等（186-198）、CLI 全部出口 close（run_cmd.rs:938/961）、--allow-mcp 预检（:885-900）
# - kernel append-only：HarnessErrorKind::Mcp + CODE_HARNESS_MCP_ERROR（types.rs:141-150/430-450）
# - 默认路径：无 mcp 配置 → McpRun=None/list 不调用/close no-op；既有 970 测试全绿
```

- **测试强度**：MCP crate-level 5 测试（registry 解析/拒绝、resolve、map、list/call+截断、错误/重启/input_required/isError、结果截断）+ config 5 + executor 3（Fake 真链路/close 幂等/no-op/not_allowed）+ CLI 2（flag 解析/预检测试）；`FakeMcpClient` 具备 delay/outcomes/connects/shutdowns 记录，但 **timeout delay 用例未使用**、**多 agent 场景未覆盖**（见 C1/P2-9）。
- **真实传输/进程组**：`real.rs` 无测试；AC-19 a/b/d 按 Lead 裁决留 V5 e2e（真 stdio/HTTP 桩、进程组 kill、SIGTERM/取消）；Windows JobObject P3 接受。
- **无 MCP 默认路径**：既有 workspace/e2e/trace 测试全部通过，未观察到输出/行为变化；feature 默认开但无配置时零进程/零 IO（`prepare_mcp_run` 直接 `None`）。

## 5. 条件关闭方式

- **C1/C2 由 harness 小 commit 关闭**（作者 kernel-dev）：C1 按 agent 隔离 plan/allow + 多 agent 测试；C2 统一 `HARNESS_MCP_ERROR{config_invalid/allow_invalid}`（或 Lead 明确接受拆分并登记 ISSUE，按 ISSUE 关闭）；重跑 §4 全部门禁（含 feature 双构建/workspace/clippy/fmt/check.sh）。
- 关闭后由评审人只读复核 + 追加「条件关闭」结论；P2/P3 可同 commit 或 V5-docs 处理（不单独阻塞）。
- 合并门禁：当前 HEAD `815abea` 时**不可 merge**（结论为有条件通过）；C1/C2 关闭并复审通过后 merge → 开 V4（harness 插件）。

## 6. 条件关闭复核（"有条件通过" → "通过"）

### 6.1 条件关闭状态

| 条件 | 状态 | 关闭证据（作者 `15f3d18`，父 `2adfb75`） |
|---|---|---|
| C1 同一 run 多 agent MCP plan/allow 隔离 | **closed** | 新增 `McpPool`（run 级共享 `server→client`/`server→tools` 缓存）+ `McpSessionRuns{run_id,pool,by_agent}` + per-agent `McpRun`（`executor.rs:87-94,503-560`；`mcp/mod.rs:959-1128`）；`list_tools` 每 server 共享缓存，`map_tools`/`execute_tool` 按本 agent plan/allow，bound 反向映射到原始工具；测试 `mcp_plans_are_isolated_per_agent_in_same_run`、`second_agent_allow_is_not_bypassed_by_first_agent`（`executor.rs:1644/1717`）通过 |
| C2 AgentConfig.mcp 错误码统一 | **closed** | `parse_mcp`/`parse_mcp_name_list` 全部改 `mcp_config_error` → `HarnessErrorKind::Mcp`/`HARNESS_MCP_ERROR{config_invalid\|allow_invalid}`（`config.rs:435-560`）；测试 `mcp_parse_errors_use_harness_mcp_error` 断言 kind/code/reason（含 allow_invalid、servers>16）通过 |

### 6.2 P2 加固关闭确认（①–⑨）

| # | 关闭证据 |
|---|---|
| ① 常量对齐 00 §0.5 | `MCP_DEFAULT_TIMEOUT_MS`/`MCP_TOOLS_LIST_TIMEOUT_MS`/`MCP_HTTP_MAX_CONCURRENCY`/`MCP_CONTROL_REQUEST_TIMEOUT_MS` 已改名；`real.rs` list 用 `MCP_TOOLS_LIST_TIMEOUT_MS`、call 默认 `MCP_DEFAULT_TIMEOUT_MS`（`constants.rs:168-197`） |
| ② allow≤16 | `parse_mcp_name_list(..., "allow", true)`；超限 `config_invalid`（`config.rs:494-520`） |
| ③ stdio cwd=assets-dir | `CommandWrap` 加 `current_dir(assets_dir)`；`McpClientFactory::connect(spec, assets_dir)` 传参（`real.rs:150-160`） |
| ④ `__` server 名 | 新增 `parse_tool_name_for_servers` 按**已配置 server 名最长前缀**解析；测试 `parses_server_names_containing_double_underscore`（`mcp/mod.rs:489/1508`） |
| ⑤ stderr 脱敏 | `display_command` URL 去 userinfo/query、args 敏感键打码、env 只列名（`mcp/mod.rs:86-150`）；`startup_notice` 走此格式 |
| ⑥ 函数名归一化+反向映射 | `normalized_function_name`（tool 部分小写/非 `[a-z0-9_]`→`_`、超长/冲突截短+FNV hash）+ `bind_tools`/bound `original` 反向映射；测试 `function_names_are_normalized_and_reversible`；**server 段未归一化（P2 残余，见 §6.3）** |
| ⑦ truncated JSON | 超限返回 `{"truncated":true,"is_error":…,"text":…}`（`mcp/mod.rs:1106-1119`）；测试 `truncated_result_is_machine_readable_json`/`result_truncation_is_bounded` |
| ⑧ 流式截断/1MiB schema | `call_result_text(result, limit_chars)` 按上限累积（不先拼全量）；`McpRun::list_tools` 累计 schema 字节 > `MCP_TOOLS_LIST_MAX_BYTES`(1MiB) → `init_failed`（`real.rs:253-268`；`mcp/mod.rs` list_tools） |
| ⑨ 测试 | 新增多 agent/allow 隔离、`__`、normalize、no-feature（`mcp_configured_without_feature_fails_init`，默认 207 vs feature 206）、`mcp_parse_errors...`；`--allow-mcp` 真 CLI exit3 端到端按 Lead 裁决留 V5 AC-19 c（e2e 分支） |

### 6.3 残余 P2/P3（非阻塞，列明；建议 V4 首 commit 或 V5-docs 处理）

- **P2⑥ 未完全关闭（唯一 P2 残余）**：`normalized_function_name` 的 **server 段仍原样**（`prefix = "mcp__{server}__"`）——server 名正则允许 `-` → 函数名含 `-`；server 名过长（前缀 >55 字符）时函数名仍 >64；两者均违反函数名注释/`ToolDef` 合同 `^[a-z][a-z0-9_]{0,63}$`。现有测试只用 server `alpha`，未覆盖。建议：server 段同样归一化（`-`→`_`、截短+hash）或拒绝过长 server 名；补 `server="a-b"`/64 字符 server 用例。**P2 一般，不阻塞 V3 merge**（可随 V4 首 commit 或登 ISSUE）。
- **P3（测试/实现小项）**：`display_command` 脱敏无专门测试；allow>16 复用同一函数（代码已限）但仅有 servers 用例；`call_result_text` 仍接收 rmcp 已解析的完整 `CallToolResult`（网络层全量解析后才按 chars 累积，传输层硬上限未强制——V5 安全 spike）；`startup_notice` 多 agent 同 server 会重复打印；`graph_has_human` doc 注释仍错位（`run_cmd.rs:966-970`）；额外常量 `MCP_TOOLS_LIST_MAX_BYTES` 未入 00 §0.5（V5-docs 可补）。

### 6.4 最终门禁复核（评审人执行）

```bash
git status --porcelain                          # 空（worktree 干净）
git diff --stat 2adfb75..15f3d18                # 6 files, +890/−294（仅 crates/zgraph-harness/src；无 cli/kernel/Cargo 改动）
cargo test -p zgraph-harness                    # 207 passed / 0 failed / 1 ignored（默认 feature）
cargo test -p zgraph-harness --features mcp     # 206 passed / 0 failed / 1 ignored
cargo test -p zgraph-cli                        # 223 / 0
cargo test -p zgraph-e2e                        # 106 / 0 / 2
cargo test --workspace                          # 976 passed / 0 failed / 3 ignored
cargo check -p zgraph-cli --no-default-features --locked   # 通过
cargo clippy --workspace --all-targets --locked -- -D warnings  # clean
cargo fmt --all -- --check                      # exit 0
bash scripts/dev/check.sh                       # CHECK PASSED
```

- C1/C2 closed；P2①–⑤、⑦–⑨ closed；**P2⑥ 残余**已列明（P2 一般，非阻塞）；P3 见 §6.3。
- 跨模块：本轮仅 harness 6 文件（无新增 cli/kernel/Cargo 改动），首轮跨模块声明不变；第二评审由 Lead 追加。

### 6.5 合并门禁

结论已为 `通过`；worktree 干净、基于 `origin/main`（`69aeb87`）；记录含 `分支: feat/harness-mcp` 与 `结论：通过` → 满足 merge.sh 前置；Lead 可追加跨模块第二评审 → merge → 开 V4（harness 插件 + Codex adapter）。

## 7. 遗留问题

- **V5 e2e（AC-19 a/b/d/e 真链路）**：真实 stdio/HTTP 双 era 桩、进程组 kill/无孤儿、SIGTERM/取消、真实 rmcp 超时/MRTR/input_required 映射、结果内存上限；**AC-19 c 的 `--allow-mcp` 真 CLI exit3 端到端**按 Lead 裁决随 V5 e2e 覆盖；Windows JobObject 为 P3（Lead 已接受）。
- **P2⑥ 残余（非阻塞）**：函数名 **server 段未归一化**（含 `-`/过长 server → 违反 `^[a-z][a-z0-9_]{0,63}$` 或 >64），建议 V4 首 commit 或 ISSUE 关闭（见 §6.3）。
- **P3/测试小项**：见 §6.3（脱敏/schema 上限/重复 notice/`run_cmd` doc 注释等）；V5-docs as-built 需补：常量（含 `MCP_TOOLS_LIST_MAX_BYTES`）、函数名/allow 口径、TOOL_ERROR reason taxonomy、`truncated` 口径到 00/05/06/DR-010。
- 跨模块（cli/kernel/Cargo）已按 Lead 批准声明；本轮收口仅 harness 6 文件；kernel 仅首轮 append-only 码/枚举，无既有语义变更；V3 未改 docs（V5 回写）。
- 未发现 P0；未发现既有行为回归；C1/C2 关闭后本记录结论为 `通过`。

## 7. 第二评审 · 跨模块（cli/kernel/Cargo）（Lead 追加，2026-11-02）

- **评审人**：`lead`（**≠ 作者** `kernel-dev`；Lead 已批准跨模块改动，按 AGENTS §2 追加第二评审）
- **范围**（`69aeb87..a09b54f`）：
  1. `crates/zgraph-cli/src/run_cmd.rs`：`--allow-mcp` 解析/usage；`--allow-mcp` 预检（run 启动前 exit 3、不产生 run、不启 server）；run 结束（成功/失败/取消/错误出口）调用 `HarnessExecutor::close_mcp()`。
  2. `crates/zgraph-kernel/src/types.rs`：append-only —— `CODE_HARNESS_MCP_ERROR`、`HarnessErrorKind::Mcp` variant、`code()` arm；既有码/枚举/行为未改。
  3. `crates/zgraph-harness/Cargo.toml` + `crates/zgraph-cli/Cargo.toml` + `Cargo.lock`：`rmcp =3.8.0`（exact）+ `process-wrap 10.0.1`/`http 1.5.0` 作为 optional `mcp`；CLI 默认 feature 启用；`--no-default-features` 构建不引入依赖（评审已验 `cargo tree` 0 依赖）。
- **核对结论（通过）**：cli 为机械接线（无默认路径/退出码变化，除新增显式 `--allow-mcp` 语义）；kernel append-only；Cargo 锁定精确、feature-gated、默认构建包含（release 口径正确）；无其它跨模块文件。
- **Lead 亲自验证**：`git diff 69aeb87..a09b54f --stat` 仅 harness + cli 1 + kernel 1 + 2×Cargo.toml + Cargo.lock（+ REV 记录）；`cargo test --workspace --offline` = 976 passed / 0 failed / 3 ignored；`cargo check -p zgraph-cli --no-default-features` 通过。
- **裁决**：P2⑥（server 段函数名归一化不全）在 **V4 首个 commit** 修复（kernel-dev 已记）；真实 stdio/HTTP/CLI exit3 e2e 留 V5 AC-19；P3 留 V5/docs。
- **结论**：**通过（第二评审）**。分支满足合并前置。
