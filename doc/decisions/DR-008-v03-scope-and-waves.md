# DR-008 · v0.3（M8）范围与波次：skills + MCP + 可插拔 harness

> **状态**：V1 设计冻结（W4b 后的 v0.3 前置设计；实现按 V2–V5 逐波）。
> **日期**：2026-11-01 · **作者**：docs-dev · **分支**：`docs/docs-v03-design`
> **关联**：[`DR-001`](DR-001-runtime-route.md)（技术路线）、[`DR-004`](DR-004-v02-scope-and-waves.md)（v0.2 波次）、[`DR-009`](DR-009-v03-skills-design.md)/[`DR-010`](DR-010-v03-mcp-design.md)/[`DR-011`](DR-011-v03-harness-plugin-design.md)、spec 00/02/05/06/07、[`research-v03-capabilities.md`](../research-v03-capabilities.md)
> **用户决策（2026-11-01）**：三件全做（skills + MCP + harness 可插拔）；skills=指令注入+按需加载；MCP=tools+stdio/Streamable HTTP；harness=子进程 adapter + Rust trait seam；**先 V1 设计冻结再实现**。

## 0. 摘要

| 项 | 冻结 |
|---|---|
| 版本 | **v0.3.0（M8）**；spec 仍 **v1.2 / DSL 1.0**（不新建 spec 目录） |
| 能力 | ① skills（Agent Skills 目录/索引/按需加载）；② MCP 工具（stdio + Streamable HTTP）；③ 可插拔 harness（`AgentProvider` + 子进程协议；builtin 默认） |
| 波次 | V1 设计冻结（本批）→ V2 skills → V3 MCP → V4 harness 插件 → V5 集成/兼容/性能/as-built/发布 |
| 依赖 | **V2→V3→V4 串行**（同模块 `crates/zgraph-harness` 单写入者；e2e/docs 准备可并行）；V5 依赖 V2–V4 全部合并 |
| 兼容红线 | 默认（无 skills/MCP/外部 harness）的图 hash/validate/run/trace/replay **逐字节不变**；`events.kind` 不变；DSL 规则/hash 不变；既有错误码/退出码不变（新增码只追加） |
| 发布条件 | 兼容矩阵无新增失败；workspace/e2e/pytest 全绿；性能回退 ≤20%（同 v0.2 门禁）；AC-18/19/20 全过；文档 as-built（V5） |
| 与 W5 关系 | **W5 共享评审队列延至 v0.4（M9）**；v0.3 不包含（**已裁决，§8 #1**） |

## 1. 目标与非目标

### 1.1 目标

1. **skills**：从 `skills/<name>/SKILL.md` 发现/校验/索引；system 只注入 `name/description`；`skill_view(name)` 按需加载正文；不执行 skill 脚本；无 skills 默认行为不变（DR-009）。
2. **MCP**：Rust client 支持 stdio 与 Streamable HTTP；`mcp_servers.json` registry；工具映射 `mcp__<server>__<tool>`；allowlist/超时/取消/进程组/结果截断；trace 默认不新增 schema（DR-010）。
3. **可插拔 harness**：`AgentProvider` 成为实际 seam；内置 `builtin` 默认；`subprocess` 外部进程协议 `zgraph-harness/1`；Codex/DSH 薄适配器；L2 不重跑外部进程（DR-011）。
4. **可测试**：AC-18/19/20 均为真 CLI/真进程/fake 资产可执行断言（07）。

### 1.2 非目标（v0.3 明确不做）

- W5 共享评审队列（v0.4）；持久化/断点续跑（W3b 已不做）；多模态；prompt 模板引擎；工具并行执行；provider 多路由/fallback；输出 JSON Schema 强校验；skills 脚本执行；MCP 资源/提示（resources/prompts）与 sampling；动态子图/嵌套 foreach；外部 harness 的「双重状态机/双重 trace」。
- 不改变 DSL 1.0/R1–R18；skills/MCP/harness 均属 **Harness Loading/AgentConfig**，不参与 `graph_hash`。
- 不引入 trace v7（默认）；MCP/工具审计如需 v7 另立 DR（候选见 DR-010 §8）。

## 2. 三能力边界与接口

| 能力 | 归属 | 配置/资产 | 关键 API/码位 | 默认行为（不配置） |
|---|---|---|---|---|
| skills | Harness | `<assets-dir>/skills/<name>/SKILL.md`；`--skills-dir`/`ZGRAPH_SKILLS`；`AgentConfig.skills` | `skill_view` 内置工具；`HARNESS_SKILL_INVALID` + `HARNESS_CONFIG_LOAD{skill_not_found}` | 不发现/不注入/不新增工具；输出不变 |
| MCP | Harness | `mcp_servers.json`；`AgentConfig.mcp` | `ToolDef`（`mcp__<server>__<tool>`）；`HARNESS_MCP_ERROR`（append-only） | 不启动 server/不注册工具；输出不变 |
| harness 插件 | Harness | `AgentConfig.harness`（builtin/subprocess） | `AgentProvider` trait；`HARNESS_PLUGIN_ERROR`（append-only） | `builtin`（现有 HarnessExecutor 路径逐字节不变） |

## 3. 波次与依赖（V1–V5）

```
V1 docs 设计冻结（本批，REV-096）
  └─▶ V2 skills（feat/harness-skills）
        └─▶ V3 MCP（feat/harness-mcp）
              └─▶ V4 harness plugin（feat/harness-plugin）
                    └─▶ V5 集成/兼容矩阵/性能/as-built/发布 v0.3.0（docs+process）
```

> **串行约束（Lead 裁决 2026-11-01）**：V2/V3/V4 **均写 `crates/zgraph-harness`**（同模块单写入者），必须**串行**，不得并行；仅 e2e 测试准备与 docs 准备可与实现波次并行（不写 harness 源文件）。

| 波次 | 分支（建议） | 作者/评审 | 交付 | 依赖 |
|---|---|---|---|---|
| V1 | `docs/docs-v03-design` | docs-dev / reviewer-a + Lead 裁决 | 本 DR + DR-009/010/011 + 00/02/05/06/07 冻结草案 + AC-18/19/20 + research | 无 |
| V2 | `feat/harness-skills` | harness-dev / reviewer-b | skills 发现/校验/索引/注入/`skill_view`/CLI（`--skills-dir`）/测试 | V1 |
| V3 | `feat/harness-mcp` | harness-dev / reviewer-a | rmcp client（feature-gated；stdio/HTTP）/registry/工具映射/allowlist/超时取消/测试 | V2（同模块串行） |
| V4 | `feat/harness-plugin` | harness-dev / reviewer-b（Lead 第二评审若跨 cli/kernel） | `AgentProvider` seam/子进程协议/fake external e2e/**Codex 先** + DSH ACP best-effort 骨架 | V3（同模块串行） |
| V5 | `test/e2e-v03` + `docs/docs-v03-spec` | e2e-dev/docs-dev / reviewer-a/b | AC-18/19/20 真链路、兼容矩阵、性能基线、as-built、v0.3.0 发布 | V2–V4 |

## 4. 兼容红线与发布条件

### 4.1 红线（每波门禁）

- 默认路径（无 skills/MCP/外部 harness）hash/validate/run/trace/replay 逐字节不变；`events.kind` 不变。
- DSL 1.0/R1–R18/19 码不变；skills/MCP/harness 配置不参与 `graph_hash`；既有错误码/退出码不变（新增只追加）。
- 不弱化既有测试；不引入 trace schema 变更（默认）；外部依赖（rmcp 等）失败必须**受控降级/明确错误**，不破坏默认路径。

### 4.2 发布条件（V5）

1. AC-18 a–f / AC-19 a–g / AC-20 a–h 全部实测通过；默认路径 golden/兼容矩阵无新增失败。
2. `cargo test --workspace`、e2e、pytest、clippy/fmt/check.sh 全绿；性能回退 ≤20%（与 v0.2 同口径，release、3 次中位；基线见 [`doc/perf-baseline-v0.2.txt`](../perf-baseline-v0.2.txt)）。
3. trace/replay：默认路径不变；外部 harness/MCP/skills 的 L1/L2 行为按 DR-009/010/011；文档 as-built 回写（05/06/07/00/02 + PROVENANCE 第 26 条）。
4. 安全：无 token/密钥落库/日志；路径/进程组/输出上限/超时用例覆盖。

## 5. AC-18/19/20 设计矩阵（摘要）

| 编号 | 主题 | 用例数（设计） | 关键断言 | 落点 |
|---|---|---|---|---|
| AC-18 | skills | a–f（6） | 目录/frontmatter 校验、索引注入、`skill_view` 按需加载、token 预算/截断、路径安全/不可信内容、默认无 skills 不变 | 07 §AC-18 |
| AC-19 | MCP | a–g（7） | **新/旧两桩**（2026-07-28 无状态 + 2025-11-25 legacy）双 era、Streamable HTTP、`mcp__<server>__<tool>` 映射/allowlist/allow-mcp、超时/取消/崩溃、结果截断、trace 默认不变、默认无 MCP 不变 | 07 §AC-19 |
| AC-20 | harness 插件 | a–h（8） | builtin 默认不变、subprocess 协议握手/事件/最终输出、流式映射、取消/超时/输出上限、L2 不重跑、fake external 真链路、安全/错误映射 | 07 §AC-20 |

> 详细断言见 07；W4a/W4b 不涉及；本批只冻结断言口径，V2–V4 实现后 V5 回写实测。

## 6. 风险与缓解

| 风险 | 影响 | 缓解 |
|---|---|---|
| skills prompt 注入/不可信内容 | 指令劫持 | 作为数据注入、标注来源、不执行脚本、大小/条数上限、AC-18 e |
| MCP server 不可信/崩溃/挂起 | run 卡住/资源泄漏 | 超时+取消+进程组 kill、受控错误、输出截断、AC-19 d/e |
| MCP 协议/rmcp 版本漂移 | 维护成本 | 锁定版本+协议互操作层+自写最小 fallback（spike 定） |
| 外部 harness 协议不稳定 | 兼容/性能 | 薄 adapter、版本协商、fake external 契约测试、L2 不重跑 |
| trace 膨胀/审计缺失 | 可观测性 | 默认沿用既有 streaming/tool 事件；v7 另立 DR |
| 默认路径回归 | 兼容红线 | 每波 golden/兼容矩阵；默认分支零配置、代码路径隔离 |
| W5 排期挤压 | v0.3 范围 | W5 明确延至 v0.4 |

## 7. 与 DR-004/W5 的关系

- DR-004 的 v0.2（M7）范围已交付（W1–W4b）；本 DR 定义 **v0.3（M8）**，不修改 v0.2 决策（只追加）。
- DR-004 §5 的 W5（共享评审队列）原为 v0.3 候选；因 v0.3 已由三项能力占满，**W5 延至 v0.4（M9）**（**已裁决，§8 #1**）。
- DR-004 §10 追加 v0.3 设计冻结执行记录（本批，REV-096）。

## 8. Lead 裁决（REV-096 A–D 范围；2026-11-01 已确认）

> 来源：本表仅列 REV-096 A–D 裁决范围；超出 A–D 的设计项为 V1 冻结推荐（非 Lead 裁决），实现前可由评审再议。

| # | 事项 | 裁决 |
|---|---|---|
| 1 | W5 共享评审队列排期 | **延至 v0.4（M9）**；v0.3 不含 |
| 2 | v0.3 发布范围 | **要求三能力全部实现**（V2–V4 全部合并后才允许 V5 发布） |
| 3 | MCP trace | **默认不新增 schema（不 v7）**：沿用 `llm_calls` + `streaming_events` 的 `tool_call_*`；server 身份由 `mcp__<server>__<tool>` 工具名表达；审计增强另立 DR，v0.3 不做 |
| 4 | skills frontmatter 未知顶层字段 | **拒绝**（`$schema` 允许忽略；`metadata` 为自由 map）；错误 `HARNESS_SKILL_INVALID{reason:"unknown_field"}` |
| 5 | 外部 harness 真实适配器 | **不阻塞发布**：V4 必交付 = `AgentProvider` seam + `zgraph-harness/1` 协议 + fake external 真链路 e2e + Codex/DSH 薄适配器骨架（best-effort；有环境补 1 条真实 smoke，无则在 release notes 标注「adapter 骨架，待真实环境验证」） |
| 6 | V2/V3/V4 并行性 | **串行**（均写 `crates/zgraph-harness`；同模块单写入者）；仅 e2e/docs 准备可并行 |
| 7 | MCP HTTP 安全 | **默认只允许 https**；明文 `http://` 必须显式 `allow_insecure_http: true` 且文档警示 |
| 8 | MCP 依赖策略 | `rmcp` **feature-gated**（默认构建不引入）；若实现对 Streamable HTTP/协议 `2026-07-28` 支持不足，允许自写最小 stdio fallback（V3 spike 定，不阻塞 V1） |
| 9 | MCP 双 era | **必须**：`2026-07-28`（无状态）+ `2025-11-25`（initialize）；rmcp `ClientLifecycleMode` |
| 10 | MCP 鉴权 | **仅静态 env Bearer**；不做 OAuth/DCR |
| 11 | `mcp` feature | optional；**CLI release 构建默认开启**；`--no-default-features` 可关；CI 双构建 |
| 12 | `--allow-mcp` | **v0.3 纳入**：默认关闭；未开启且 `AgentConfig.mcp` 非空 → `HARNESS_MCP_ERROR{reason:"not_allowed"}`；开启时 stderr 打印无密钥命令 |
| 13 | 外部 harness 二进制 | **用户自备**（不下载/不内嵌）；无环境时 release notes 标注「adapter 骨架，待真实环境验证」 |
| 14 | StopReason / cached tokens | **StopReason 不新增枚举**（`Other{raw}`）；`cached_input_tokens`/`cache_write_input_tokens` **v0.3 丢弃**（文档注明） |
| 15 | 适配器顺序 | **Codex 先**（exec --json）；**DSH ACP best-effort 后**（`dsh --profile acp`；headless/sdk 不可依赖） |

> 本表取代原「待确认」状态；W5/v0.3 范围已按裁决固定。

## 9. 实现执行记录（as-built，2026-11-02，REV-101；只追加）

| 波次 | 分支/实现提交 | 评审 | 合并点（main） | 测试证据 |
|---|---|---|---|---|
| V1 | `docs/docs-v03-design`（`2babdc8`+`80a42c1`+`7659ec8`） | REV-096（有条件通过 → C1–C5/P1 关闭后通过） | `29ee6d3` | check.sh |
| V2 skills | `feat/harness-skills`：`cfe6280`+`f286fe8`+`6844573` | REV-097（含跨模块 Lead 第二评审） | `69aeb87` | harness 默认 226/0/1；skills 18 例；workspace 996/0/3 |
| V3 MCP | `feat/harness-mcp`：`18bf405`+`815abea`+`15f3d18`+`23fa568` | REV-098（含跨模块 Lead 第二评审） | `23fa568` | harness 226/0/1、features mcp 225/0/1；rmcp 3.8.0 feature-gated |
| V4 plugin | `feat/harness-plugin`：`0584931`+`3bbe4b5` | REV-099（含跨模块 Lead 第二评审） | `6515f12` | harness 226/0/1；workspace 996/0/3；pytest 180；fake external + Codex adapter + DSH ACP 骨架 |
| V5a e2e / V5b docs | 本批（docs）与 e2e 并行 | REV-101（docs） | 待合并 | AC-18/19/20 待 V5a 回写 |

- 三能力均按 V1 冻结实现；**V2→V3→V4 串行**（同模块 `crates/zgraph-harness`）确认。
- 默认路径/兼容红线保持；实现偏差见 DR-009/010/011 执行记录与 05 §5.14.4。
- 遗留：真实 Codex/DSH 环境未验证（adapter 骨架，best-effort）；MCP 真实 stdio/HTTP/进程组由 V5a 覆盖；P2/P3 入台账（ISSUE-156+）。
