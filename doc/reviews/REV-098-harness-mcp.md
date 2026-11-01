# REV-098 · harness（含 cli/kernel 机械）· V3 MCP 实现（rmcp 双 era + 工具桥接 + `--allow-mcp`）

> **跨模块声明**（Lead 已批准 + 第二评审追加）：本分支改动 `crates/zgraph-harness`（主体）、`crates/zgraph-cli/src/run_cmd.rs`（`--allow-mcp` + 预检 + `close_mcp` 调用）、`crates/zgraph-kernel/src/types.rs`（`HarnessErrorKind::Mcp`/`CODE_HARNESS_MCP_ERROR` append-only）、`Cargo.toml`/`Cargo.lock`（`rmcp =3.8.0` feature-gated）。评审人只新增本评审记录，未改实现/测试。

- **分支**：`feat/harness-mcp`
- 分支: feat/harness-mcp
- **作者**：`kernel-dev`（harness 角色；`18bf405` 主实现 + `815abea` run 结束 `close_mcp` 修复；正文 `Review: REV-098` / `Agent: kernel-dev`）
- **评审人**：`reviewer-a`（主审；Lead 追加裁决/第二评审）
- **变更范围**：`69aeb87..815abea`（**14 files, +2777/−16**）：`crates/zgraph-harness/src/{mcp/mod.rs(1291),mcp/real.rs(321),mcp/testing.rs(191),config.rs,executor.rs,agent_loop.rs,constants.rs,error.rs,lib.rs}`、`crates/zgraph-cli/{Cargo.toml,src/run_cmd.rs}`、`crates/zgraph-kernel/src/types.rs`、`Cargo.lock`
- **评审时间**：2026-11-01
- **关联**：DR-010（REV-096 冻结）；spec 00 §0.3/§0.4/§0.5、05 §5.14.2、06 §6.14、07 AC-19 a–g；task-59/60；REV-096（V1 条件关闭通过）

## 1. 结论

**结论：有条件通过**

默认（无 MCP 配置）路径与既有测试全部保持；feature/依赖、registry 拒绝矩阵、AgentConfig/`--allow-mcp`、McpRun 桥接、双 era、trace 不 v7、测试与门禁主体成立。P0=0；**P1 条件 C1–C2**（均可由 harness 小 commit + 测试关闭）；P2/P3 建议见 §3。**未发现功能返工级问题**。

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

## 6. 条件关闭记录（"有条件通过"）

| 条件 | 状态 | 关闭要求 |
|---|---|---|
| C1 同一 run 多 agent MCP plan/allow 隔离（`executor.rs:493-540`） | **open** | harness 小 commit + 多 agent 测试；重跑门禁 |
| C2 AgentConfig.mcp 结构/allow 错误码统一到 `HARNESS_MCP_ERROR`（`config.rs:435-560`） | **open** | harness 小 commit + config 测试；或 Lead 接受拆分并登记 ISSUE |

## 7. 遗留问题

- **V5 e2e（AC-19 a/b/d/e 真链路）**：真实 stdio/HTTP 双 era 桩、进程组 kill/无孤儿、SIGTERM/取消、真实 rmcp 超时/MRTR/input_required 映射、结果内存上限；Windows JobObject 为 P3（Lead 已接受）。
- **V5-docs as-built**：常量名/缺失（P2-1）、allow 限长、cwd、`truncated` 标记、TOOL_ERROR reason taxonomy、ToolDef 名称合同（P2 项）需在 05/06/00/DR-010 as-built 回写或代码修正。
- **测试缺口**（P2-9/P3）建议 V3 补 timeout/multi-agent/CLI 集成，V5 补真实传输。
- 跨模块（cli/kernel/Cargo）已按 Lead 批准声明；kernel 仅 append-only 码/枚举，无既有语义变更；未改 docs（V5 回写）。
- 未发现 P0；未发现既有行为回归。
