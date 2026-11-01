# REV-096 · docs · v0.3（M8）V1 设计冻结：skills + MCP + 可插拔 harness

> **评审基准**：分支 `docs/docs-v03-design` 提交 `2babdc8`（基于 main `5d7cc89`；14 files 纯 `doc/**`，+861/−9）。**注**：评审期间作者工作区已出现 DR-008/009/010/011、research、00/05/07 的**未提交**修改（docs-dev 按 arch-researcher 结论收口中），不属于本记录基准；本记录条件针对 `2babdc8`，关闭后由评审人追加 §6 确认。
>
> **输入材料**：task-54（用户决策/口径）、task-55（作者任务）、Lead 裁决（DR-008 §8 八项）、spec-v1.2 00/02/05/06/07、DR-001..007、`doc/analysis/research-harness-alternatives.md`/`research-codex-embed.md`、`doc/research-v03-capabilities.md`、DR-008..011、**arch-researcher 498 行报告**（`/tmp/v03-research.md`，2026-11-01；Lead 转发要点已纳入 C1–C4）。

- **分支**：`docs/docs-v03-design`
- 分支: docs/docs-v03-design
- **作者**：`docs-dev`（提交 `2babdc8`；正文 `Review: REV-096` / `Agent: docs-dev`）
- **评审人**：`reviewer-a`（主审；Lead 追加裁决确认/第二评审）
- **评审时间**：2026-11-01
- **变更范围**：`5d7cc89..2babdc8`（14 files, +861/−9）：新增 DR-008（119 行）/DR-009（150）/DR-010（190）/DR-011（175）/`research-v03-capabilities.md`（91）；spec 00/02/05/06/07 冻结草案；DR-004 §10 追加；index/issues/progress。

## 1. 结论

**结论：有条件通过**

范围/波次/兼容红线/AC 矩阵与 Lead 八项裁决**主体成立**；但 **arch-researcher 结论（2026-11-01）与作者稿存在多处事实冲突**，且发现若干**文档内部矛盾与格式缺陷**。P0=0；**P1 条件 C1–C5**（均可由 docs 单 commit 关闭）；P2/P3 建议见 §4。

### 1.1 范围 / 波次 / 裁决核对

| 项 | 核对结果 |
|---|---|
| 三能力边界/非目标 | ✅ DR-008 §1–§2 明确（skills=索引+按需加载不执行脚本；MCP=tools+stdio/Streamable HTTP；harness=子进程 adapter+Rust seam）；非目标含 W5/持久化/多模态/resources/prompts/sampling/双重状态机（§1.2） |
| V1–V5 波次与串行依赖 | ⚠️ §3 串行约束（`:53`）与 §8 #6 正确；但 **§0 摘要 `:15` 写「V2/V3/V4 可并行」**，与 §3/§8 直接矛盾 → **C5** |
| 发布条件/兼容红线 | ✅ DR-008 §4（默认路径逐字节不变、DSL 19 码不变、不 v7、性能 ≤20%、AC-18/19/20 全过） |
| Lead 八项裁决 | ⚠️ 逐项：①W5→v0.4（§8#1 已确认，但 §0 `:18`/§7 `:103` 仍写「推荐，Lead 裁决项」→C5）；②三能力全做（§8#2 ✅）；③MCP 不 v7（§7/§8#3 ✅）；④skills 未知字段拒绝（DR-009 §2/§10 ✅）；⑤真实 adapter 不阻塞（DR-011 §11#4 ✅）；⑥串行（§3/§8#6 ✅，摘要矛盾 C5）；⑦https-only（DR-010 §11#2 ✅，但 §6 `:134` 仍写「推荐允许 http——Lead 裁决项」→C5）；⑧rmcp feature-gated（DR-010 §0/§3 ✅，但需按报告补事实 →C1） |
| AC-18/19/20 设计矩阵 | ✅ 数量 a–f(6)/a–g(7)/a–h(8) 与 07 表格一致（07:482/495/511）；体例可测 |
| 与 DR-004/W5 关系 | ✅ DR-004 §10 追加（+2/−0）；W5 延至 v0.4 的方向正确（表述一致性见 C5） |

### 1.2 能力设计核对（摘要）

- **DR-009 skills**：目录/frontmatter/注入/`skill_view`/预算/安全/错误码/AC-18 主体成立，且 additive/不参与 hash 口径正确；但需按 arch 报告收紧：`name` 必须等于父目录、`description` 1–1024 **校验口径与「截断」矛盾**、`compatibility`/`metadata map<string,string>`/`allowed-tools` 不授权、单层不递归/排序、catalog 50–100 token/4096 字符、`skill_view` 参数 enum 与资源清单、无效 vs 未引用 skill 处理、reason 闭集与 agentskills 失败面对齐、默认目录 `.zgraph/skills` 与报告 `<assets-dir>/skills` 的取舍（C3）。
- **DR-010 MCP**：需**按 arch 报告重写**（C1）：`2026-07-28` 已无状态（无 initialize/session/GET-SSE/Last-Event-ID；每请求 `_meta` 协商 + HTTP `MCP-Protocol-Version`/`Mcp-Method`/`Mcp-Name`）；**必须双 era**（+`2025-11-25`；rmcp 3.8.0 `ClientLifecycleMode`）；工具命名 `mcp__<server>__<tool>`（LLM function name 安全）+ **必须显式进 AgentConfig.tools 白名单**；`call_id=mcp__<server>__<tool>#<n>`；rmcp 事实（Apache-2.0/MSRV 1.88、feature 拆分、`list_all_tools` 翻页、`call_tool` 自动 MRTR 默认 10 轮、**无 per-call 超时需自包**、HTTP 默认 16 并发/`control_request_timeout=5s`）；`input_required` 一律结构化失败；进程/取消/进程组/stderr 持续读/同 run 最多重启 1 次；`--allow-mcp` 显式开关与静态 header 鉴权；trace 不 v7（现有稿 `:100` 起的 initialize 生命周期、`:90` session/SSE、`:69` 命名、`:139` tool_name 均需改）。
- **DR-011 harness 插件**：seam/协议/L2/安全骨架成立；需补 arch 事实（C2）：Codex `exec --json` 8 类 `ThreadEvent`、**无 token 级 delta**（`capabilities.streaming=false`；`iterations` 固定 1；退出码仅 0/1 不可单独判成功，需 `turn.completed/failed`；`-o/--output-last-message` 文件更稳；`cached_input_tokens` 无槽位→丢弃并记文档）；**ACP 修正**（`agent_message_chunk`/`agent_thought_chunk` 存在；DSH 只发 committed 消息 → 粒度=每条消息一次；`dsh --profile acp` 是唯一可依赖入口，headless `--json` schema 未知、sdk 不可考→不作硬依赖）；进程模型（每 run 常驻 adapter/每节点一次 prompt/重试新 session）；**组合约束**（`provider != builtin` 且 `tools`/`mcp`/`skills`/`max_iterations` 非空 → `HARNESS_CONFIG_LOAD{reason:"provider_capability_conflict"}`）；`AgentProviderKind` 需增 `Subprocess`（source-breaking additive）；**H6/错误映射**（C4）。
- **trace 不 v7**：✅ 结论成立（06 §6.14；arch 报告 D.2 同结论：`streaming_events` 五 kind + `call_id/tool_name` 可承载）；`call_id`/`tool_name` 具体格式需按 C1 冻结。
- **spec 草案**：02 不参与 hash 澄清 ✅；00 §0.4 追加 10 码、§0.5 追加常量方向 ✅；**05 新增节与既有 §5.11/§5.12 重号**（`:1307`/`:1355` 已存在，`:1434` 又用 §5.11）→ C4；06 §6.14 编号无冲突，但 v0.3 banner 与 M6 banner 粘连（`:12` 长 1172 字符）→ C5；07 AC-18/19/20 体例 ✅，断言需按 C1–C3 同步。
- **一致性/红线**：只追加口径基本成立（DR-004 +2/−0；spec 00/02/05 纯追加；06/07/index/progress 有必要的状态/口径修改，已定位）；无 token/密钥 ✅；PROVENANCE 暂不重算（V5 回写）✅ 可接受；research「需 spike」项已显式（ISSUE-151–154 + §1.4/§2.2/§3.3），但其状态「草案/待 Lead 裁决」与 DR 已裁决口径不一致 → C5。

## 2. 四维评审

| 维度 | 结论 | 依据 |
|---|---|---|
| 功能正确性 | ⚠️ 有条件 | 范围/AC/兼容红线主体成立；但 MCP 生命周期/双 era/命名与 arch 事实冲突（C1）、harness Codex/ACP 降级映射缺失（C2）、skills 细节与 description 口径矛盾（C3）、05 §5.11 重号与 H6 错误枚举冲突（C4）——均为设计冻结可测性/可实现性问题 |
| 结构合理性 | ✅（C5 格式项） | 四 DR + research + spec 草案 + index/issues/progress 结构清晰；DR 内部表格体例统一；仅 §5.11 重号、06 banner 粘连、index 行合并等结构缺陷（C4/C5） |
| 逻辑性 | ⚠️ 有条件 | 串行依赖（§3 vs §0）、W5 已裁决 vs「待裁决」、https 已裁决 vs §6 旧句、错误码闭集（HARNESS_PLUGIN_TIMEOUT 不在 00；missing-server 用 HARNESS_CONFIG_LOAD vs HARNESS_MCP_CONFIG）等矛盾需统一；arch 事实与作者稿的版本差异需消除 |
| 可读性 | ✅（P3 备注） | DR 表格/裁决表/AC 表格清晰；arch 报告要点需并入 research/DR 以补来源；C5 格式修复后可读性恢复 |

## 3. P1 条件（必须由 docs 单 commit 关闭；关闭后评审人追加 §6 确认）

> 条件均**只改文档**；关闭时同步 `doc/research-v03-capabilities.md` 与 00/05/06/07 相关草案，重跑 `bash scripts/dev/check.sh`；不改实现/既有评审记录。

### C1（DR-010 MCP 按 arch-researcher 事实重写）

- 生命周期：删除「initialize → tools/list → tools/call」旧叙事（`DR-010:15`、`:100-`），改为 **2026-07-28 无状态**（无 initialize/session/GET-SSE/Last-Event-ID；每请求 `params._meta` 含 `protocolVersion`/`clientCapabilities`；HTTP 必带 `MCP-Protocol-Version` + `Mcp-Method`/`Mcp-Name`；取消=关闭 SSE 流；`server/discover` 可选）；**双 era 强制**（+2025-11-25 initialize/`Mcp-Session-Id`，rmcp 3.8.0 `ClientLifecycleMode` preferred+legacy；e2e 各一新/旧桩）。
- rmcp 事实：3.8.0/Apache-2.0/MSRV 1.88/edition 2024；features `client`+`transport-child-process`+`transport-streamable-http-client-reqwest`；`list_all_tools()` 翻页；`call_tool()` 自动 MRTR（默认 10 轮，或 `call_tool_once`）；**无 per-call 超时**→`tokio::time::timeout` 自包；HTTP 默认 16 并发/`control_request_timeout=5s`（框架层工具仍串行）；旧 HTTP+SSE 不支持。
- 命名/可见性：`mcp__<server>__<tool>`（替换 `mcp:<server>:<tool>`：`DR-010:14/:49/:69/:139`、DR-008 §2 表、05 §5.11.2、06 §6.14、07 AC-19）；MCP 工具**必须显式列进 `AgentConfig.tools`**；`call_id=mcp__<server>__<tool>#<n>`；名称冲突 fail-closed。
- MRTR/安全/进程：`clientCapabilities={}`；收到 `resultType:"input_required"` → 结构化工具失败（reason 建议 `input_required_unsupported`，不静默/不代答）；`--allow-mcp` 显式开关、启动打印完整命令、HTTP 静态 header（env 展开）不落盘/日志/trace；stdio 进程每 run 每 server 常驻、stderr 持续读有上限、进程组 kill（03 §3.9.4）、同 run 最多重启 1 次；registry 不允许来自 DSL/graph。
- trace：确认 06 §6.14 默认不 v7；`tool_name`/`call_id` 用上述格式；不落 args/result 全文与密钥。
- 同步：DR-008 §2/§5 的 MCP 描述、05 §5.11.2（改号见 C4）、06 §6.14、07 AC-19 a/b/c/d、`doc/research-v03-capabilities.md` §2（生命周期/rmcp 事实/双 era/MRTR/命名）。

### C2（DR-011 Codex 降级映射 + ACP 修正 + 组合约束）

- **Codex `exec --json`**：8 类 `ThreadEvent`（`thread.started`/`turn.started`/`turn.completed`/`turn.failed`/`item.started|updated|completed`/`error`）；**无 token 级 delta** → `capabilities.streaming=false`，`TokenDelta` 降级为「一条消息一次」并写死；`iterations` 无对应物→固定 **1**；退出码仅 **0/1**，成功判定必须结合 `turn.completed`/`turn.failed`（不能只看退出码）；`content` 优先读 `-o/--output-last-message` 文件；`usage` 映射（`cached_input_tokens`/`cache_write_input_tokens` 无槽位→丢弃并记文档）；`--ephemeral`/`-s`/`-C` 等安全参数说明。
- **ACP/DSH 修正**：ACP v1 有 `agent_message_chunk`/`agent_thought_chunk`（「拿不到 token delta」旧结论不成立）；**DSH 只发 committed 消息**（粒度=每条消息一次）；`dsh --profile acp` 是唯一可依赖入口，headless `--json` schema 未知、`sdk` 不可考→不作硬依赖；顺序 Codex 先、DSH ACP 后（best-effort，不阻塞发布）。`StopReason` 映射（end_turn/max_tokens/max_turn_requests/refusal/cancelled → 现有五值/Other）。
- **进程模型/协议**：每 run 常驻 adapter（每 provider 一个）、每节点一次 prompt、attempt 重试=新 session；`hello/hello_ok` 版本协商与 `zgraph-harness/1` 保持；保留行/final/stderr/超时/握手常量。
- **组合约束**：`provider != builtin` 且 `tools`/`mcp`/`skills`/`max_iterations` 非空 → `HARNESS_CONFIG_LOAD{reason:"provider_capability_conflict"}`（禁止双 loop/双注入；修正 `DR-011:90` 的「可同时配置」口径）；`AgentProviderKind` 新增 `Subprocess` 需注明 additive/source-breaking 影响。
- 同步：DR-011 §0/§2/§3/§7/§8/§10（AC-20 b/c/h 按上述改）、07 AC-20、research §3。

### C3（DR-009 skills 严格细节对齐 agentskills.io）

- frontmatter：`name` ≤64、仅小写字母/数字/连字符、不以 `-` 开头/结尾/无 `--`、**必须等于父目录名**（严格拒绝；reason 建议 `invalid_name`/`name_mismatch` 统一）；`description` 1–1024 必填；`license`；`compatibility` 1–500；`metadata` `map<string,string>`；`allowed-tools` 实验性**仅记录不授权**；未知顶层字段拒绝（`$schema` 忽略）——与 `DR-009:12/:51`、AC-18 c 对齐。
- **解决 description 矛盾**：`DR-009 §2` 视 >1024 为非法 vs §4/§5/AC-18 d「>1024 截断」二选一（建议：校验期 >1024 → `HARNESS_SKILL_INVALID`；仅注入展示行截断，AC-18 d 改为「非法/截断」分别断言）。
- 目录/预算：单层不递归、按目录名排序、跳过 `.git`/`node_modules`；catalog（name+description）**50–100 token/≈4096 字符**、超限截断标记；`skill_view` 正文 ≤32 KiB、**首尾保留+中间截断标记**；单 run `MAX_SKILL_VIEWS=5`；`AgentConfig.skills` 上限（报告建议 ≤16）与目录条目上限（5000）进 00 §0.5。
- `skill_view`：参数 `name` 用 **enum（仅已注册 skill）**；返回正文 + 资源清单（只列文件、排序、≤50 条，不读内容）；重复激活去重；正文标记为受保护内容（不参与压缩/裁剪）。
- 处理口径：无效 skill 被引用 → 错误；**未引用**的无效 skill → 跳过 + 诊断（需冻结）；reason 闭集与 agentskills 失败面（missing_skill_md/unparseable_yaml/missing_description/name_mismatch/name_too_long/body_too_large/path_escape/duplicate_name/not_utf8）对齐或映射说明。
- 默认目录：DR-009 §10 #5 称已裁决 `.zgraph/skills`，与报告建议 `<assets-dir>/skills` 不一致——请 Lead 确认并在 DR/05 统一（`ZGRAPH_SKILLS` env 名亦与作者 `ZGRAPH_SKILLS_DIR` 不一致，需统一）。
- 同步：DR-009 全文、05 §5.11.1（改号 C4）、07 AC-18、research §1。

### C4（spec 一致性：05 改号/ACP 注记、H6 错误枚举、错误码闭集、常量）

- **05 重号**：既有 `## 5.11 与 00 总纲的对齐检查`/`5.11.1–3`（`:1307` 等）与 `## 5.12 审计条目关闭对照`（`:1355`）已占用编号；新增 v0.3 节（`:1434`）应改为 **§5.13（5.13.1–3）**，并同步 DR-009/010/011/07 中「05 §5.11.x」引用（或明确改为「v0.3 节」锚点）。
- **05 ACP 修正注记**：`:1255`「ACP 只上线语义事件、拿不到 token delta」已过时——追加 v0.3 修正注记（ACP 有 chunk 通道；DSH 只发 committed 消息，粒度每条消息一次），并同步 §5.13.3 与 DR-011 §8。
- **H6/错误枚举**：05 H6（`:25`）冻结 `HarnessErrorKind` 八值、除 `ContextOverflow` 不得新增；DR-011 §7 `:121` 的 `Crashed(HarnessPlugin)` 属新增枚举值 → 必须二选一：复用现有值（`ConfigLoad`/`ToolError`/…）或由 Lead 明确裁定修订 H6/00 §0.3（并在 00/05 记录）。
- **错误码闭集**：`HARNESS_PLUGIN_TIMEOUT` 在 DR-011 §7 出现但 **00 §0.4 未列**（当前 0 命中）——决定删除该码（超时统一 `NODE_TIMEOUT`）或补入 00 §0.4；DR-010 `:62` 缺 server 用 `HARNESS_CONFIG_LOAD{reason:"mcp_server_not_found"}` 而 §8/00 用 `HARNESS_MCP_CONFIG`——统一。
- **常量补全**：DR-010 §5 的 initialize/tools-list=10s、call 缺省 30s 未全部进 00 §0.5（现仅 `MCP_DEFAULT_TIMEOUT_MS=30000`）——补 `MCP_INIT_TIMEOUT_MS=10000`/`MCP_TOOLS_LIST_TIMEOUT_MS=10000` 或明确注释口径；skills 的目录条目/技能数/catalog 预算常量同（见 C3）。

### C5（文档一致性与格式修复）

- `DR-008:15`「V2/V3/V4 可并行」→ 改「串行（同 harness 模块单写入者），V5 依赖 V2–V4 全部合并」；`DR-008:18`/`:103` 的「推荐，Lead 裁决项」→ 改「已裁决（§8 #1）」；`ISSUE-155`（issues:163）同步为已裁决（或改为追踪 v0.4 排期）。
- `DR-010:134` 删除旧句「v0.3 red line：只允许 https？推荐允许 http 并文档警示——Lead 裁决项」→ 统一 §11 #2（默认 https-only + `allow_insecure_http` 显式例外）。
- `DR-010:62` 与 §8/00 的 missing-server 错误码统一（C4）。
- **index.md 行合并**：`:18` 中 DR-011 行与 `architecture-v2.md` 行被 `||` 合并（architecture 行丢失）→ 拆为两行；同时 `:13` 新增空行切断了表格（与既有 `:11` 空行同类，建议一并将表格恢复连续或注明分表）。
- **06 banner 粘连**：`:12` 的 v0.3 行（长 1172）把 **M6 批次 banner 整段拼到同一行**（06 原有 M6 行被合并）→ 拆为两行，保留两批次 banner 原样。
- **02 多余空行**：`:566` 后的双空行（567/568）→ 保留一个。
- **research 状态**：`research:3`「草案…spike 结论到达后锁定」/`:7`「推荐，待 Lead 裁决」→ 改为「V1 冻结输入；arch-researcher 报告要点已合入 DR-008..011；剩余需 spike 项见 ISSUE-151–154 与 V3/V4」；并补报告来源（URL 引用，`/tmp` 不入库）。
- **progress**：`:8/:14` 的「待 arch-researcher 转发/阻塞最终设计锁定」→ 改为「V1 已提交评审（REV-096）」；阻塞项移除/降级为 V3/V4 spike。
- （同 commit 可一并修 P2 中低成本项。）

## 4. P2/P3 建议（非阻塞，可与 C1–C5 同 commit 或在 V2 前处理）

| # | 级别 | 问题 | 建议 |
|---|---|---|---|
| P2-1 | P2 | DR-009/010/011 的「Lead 裁决（已确认）」表含超出 DR-008 §8 八项的条目（如 skills 目录缺省、`*` 语义、MCP 超时不可覆盖、多 server 不并发等）；Lead 转述中未逐一列出 | 请 Lead 在复审时确认这些裁决确已作出；否则改标注「推荐/待裁决」并在台账登记 |
| P2-2 | P2 | MCP server 配置=RCE 风险仅写「受信配置」；报告建议 `--allow-mcp`、启动打印完整命令、只允许静态 header 鉴权 | 在 DR-010 §6/§1 补最小防护条款（部分已在 C1） |
| P2-3 | P2 | 05 §5.11（新节）未显式对照 H1–H6（H1/H2/H3/H5 明显一致，H4 需注明 MCP/外部进程是「新外部执行面、非进程内 Python」，H6 见 C4） | 加一段「与 H1–H6 一致性」说明 |
| P2-4 | P2 | arch 报告的 D.3 拍板清单中仍有未在 DR 明确的项（cached tokens 去向、`StopReason` 是否扩展、session reuse/reset、外部 harness 二进制预装、MCP 工具是否进 tools（C1 已定）等） | 在 DR-011/DR-010 末尾加「已定/待定」表或登记 ISSUE-156+ |
| P2-5 | P2 | AC-19 a 写「fake MCP server（NDJSON）」，MCP 实为 JSON-RPC over stdio；且双 era 需两个桩 server | 措辞统一 + AC-19 a 明确「新/旧 era 各一桩」 |
| P2-6 | P2 | DR-008 性能门禁「回退 ≤20%」未引用 v0.2 基线资产 | 引用 `doc/perf-baseline-v0.2.txt` 与 v0.2 同口径（release/3 次中位） |
| P3-1 | P3 | DR-010 `inputSchema` 写「JSON Schema 1.0 子集」，报告为 **2020-12** | 措辞改为 2020-12 子集 |
| P3-2 | P3 | DR-010 `$schema` 示例 URL（`zgraph.dev` 占位）与 `mcp_servers.json` 位置口径 | 注明示例/非契约 |
| P3-3 | P3 | 07 AC-20 c 的「无 streaming 能力时降级只 final」需与 C2 的 Codex `streaming=false` 对齐 | C2 一并改 |
| P3-4 | P3 | spec-v1.2 README/PROVENANCE 未同步（V1 不重算） | 可接受；V5 as-built 时必须回写（批次 banner/行数表/PROVENANCE 第 26 条） |

## 5. 验证证据

评审人**亲自只读核验**（worktree `/Users/zyzs/lczyzs/code/zgraph/.worktrees/docs-docs-dev`，**基准 HEAD `2babdc8`**；评审期间作者工作区出现 8 个文件的未提交收口修改，不计入基准）：

```bash
git diff --stat main...HEAD        # 14 files, +861/−9；git diff --name-only | grep -v '^doc/' → 空（纯 doc）
git diff --numstat main...HEAD     # DR-004 +2/−0（追加）；新 DR/ research 全 +；spec 00/02/05 纯 +；
                                   # 06 +16/−1（M6 banner 被并入 v0.3 行）、07 +54/−1（通过标准行）、index +7/−2、progress +5/−5
bash scripts/dev/check.sh          # CHECK PASSED（含身份/冻结文件/fmt/git diff --check；评审时工作区含作者未提交修改，仍通过）
git diff --name-only main...HEAD | grep -c PROVENANCE   # 0（V1 不重算，V5 回写——符合 task-54）
# 新文档 token/密钥扫描：sk-/ghp_/Bearer/api_key 实际值 → 无命中（仅 ${ENV} 占位）
# AC 计数脚本：07 AC-18=a..f(6)、AC-19=a..g(7)、AC-20=a..h(8) == DR-008 §5 矩阵
# 错误码闭集：00 §0.4 新增 10 行；HARNESS_PLUGIN_TIMEOUT → 00 中 0 命中（C4）
# H6/HarnessErrorKind：00:124 八值；05:25 H6「除 ContextOverflow 不得新增」；DR-011:121 提 HarnessPlugin（C4）
# 05 重号：1307/1311/1325/1342（既有 5.11.x）与 1434/1438/1445/1453（新 5.11.x）重复；既有 5.12=1355（C4）
# 06 banner：HEAD:06 第 12 行长 1172（v0.3 + M6 合并）；06 §6.14 编号无冲突
# index：HEAD:index 第 18 行长 415 含 '|| architecture-v2'（行合并，C5）
# 文件/锚点：DR-008..011 存在；05 §5.11.x、06 §6.14、07 AC-18/19/20 存在
# 只追加：DR-004 +2/−0；既有 DR/评审记录未被改（reviews 仅 index 行待补）
```

- arch-researcher 报告（`/tmp/v03-research.md`，498 行）关键事实已用于 C1（MCP 双 era/rmcp 3.8/命名/超时）、C2（Codex 8 事件/无 token delta/ACP 修正）、C3（skills 严格字段）、C4（trace 不 v7 确认）——评审人核对了其引用来源与结论表述（报告未入库，需由 docs-dev 以 URL 引用方式并入 research/DR）。
- **注**：作者工作区在本记录落盘时已有未提交的收口修改（DR-008/009/010/011、research、00、05、07 共 8 files），docs-dev 依据 Lead 转发的 arch-researcher 结论正在关闭 C1–C3/C5 部分项；本记录条件以 `2babdc8` 为基准，最终以作者 commit + 评审人 §6 复审为准。

## 6. 条件关闭记录（"有条件通过"）

| 条件 | 状态 | 关闭要求 |
|---|---|---|
| C1 DR-010 按 arch 事实重写（双 era/无状态/rmcp/命名/MRTR/超时/进程/─allow-mcp→`--allow-mcp`/trace） | **open** | docs commit：DR-010 + 05/06/07/research 同步 |
| C2 DR-011 Codex 降级映射 + ACP 修正 + 进程模型 + 组合约束 | **open** | docs commit：DR-011 + 07 AC-20 + 05 §5.13.3 |
| C3 DR-009 严格细节（name/description/registry/metadata/allowed-tools/catalog/skill_view/无效与未引用/默认目录） | **open** | docs commit：DR-009 + 07 AC-18 + research §1 |
| C4 spec 一致性（05 改号 §5.13/ACP 注记/H6 错误枚举/HARNESS_PLUGIN_TIMEOUT/常量） | **open** | docs commit：00/05 + DR-011/010 引用 |
| C5 文档一致性/格式（DR-008 依赖与 W5、DR-010 https/错误码、index/06/02 格式、research/progress 状态） | **open** | docs commit（可与 C1–C4 同一 commit） |

- 关闭方式：作者单 docs commit（建议 `docs(docs): 收口 REV-096 条件（arch-researcher 事实/格式）`，正文 `Review: REV-096` + `Agent: docs-dev`）→ 评审人逐条复核 + `bash scripts/dev/check.sh` + 追加本表「closed」并把 §1 结论改为**通过**（保留首轮有条件通过记录）→ Lead merge + push。
- 合并门禁：当前 HEAD 为 `2babdc8` 时**不可 merge**（结论为有条件通过）；条件全部关闭并复审通过后 merge。

## 7. 遗留问题

- W5 排期已按 Lead 裁决延至 v0.4——v0.4 规划时需在 DR-004 §10/新 DR 中补执行记录。
- arch-researcher 报告的需 spike 项（rmcp 体积/MSRV、macOS 进程组、Codex exec 行为、DSH ACP/headless、恶意 skill 样本）已登记 ISSUE-151–154 的对应关系需在 research 更新时补全（当前 ISSUE-151/152/153/154 覆盖 client 选型/Codex·DSH/agentskills 规范/L2 安全）。
- PROVENANCE 第 26 条与 spec README 行数表在 V5 as-built 时回写；本 V1 不重算属既定口径。
- 未发现需要代码返工的问题（docs-only）；无 P0。
