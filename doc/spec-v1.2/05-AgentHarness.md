# 图模型驱动工作流框架 · 规格冻结 v1.2 · 分册 05：Agent Harness

> **⚠ v2 落地偏差（已回写）**：本分册的进程内执行/取消/工具执行/重试编排等假设，已按 [`doc/architecture-v2.md`](../architecture-v2.md) §8 偏差表（D1–D12）与本分册 **§5.13（M4 v2 落地偏差 D13–D21）** 回写为正文（M4 批次，2026-10-30，见 PROVENANCE §2 第 7 条）；正文与偏差表冲突时以正文为准，偏差表保留为决策记录。
> **状态**：规格冻结 v1.2 + **M4 v2 回写完成**（2026-10-30）。实现：`crates/zgraph-harness` 提供 `HarnessExecutor`（`NodeExecutor` 的 `agent_call` 实现）——LLM 流式（`genai` 0.7.2 transport）+ tool loop + 上下文预算 + fake provider；**工具执行 = worker 子进程**（D3/D13，`python/zgraph_worker/tool_runner.py`）。本分册定义 **Agent 执行壳**的字段级契约：AgentConfig、`LlmProvider`（genai transport）、上下文组装与 token 预算、Harness 状态机与 tool loop、工具注册表、错误映射、流式事件、输出契约、`AgentProvider` 扩展点。
> **权威锚点**：[00 总纲](00-总纲.md)。本分册的枚举、错误码、默认值、不变式**一律不得偏离 00**；若发现偏差，见 §5.11 对齐检查（列出需 00 补录的常量与待裁定项）。
> **选型前提**（`../决策记录-01-范围与Harness.md` §3）：**自研六阶段薄壳 + `genai` 0.7.2 仅作 LLM transport**；不引入任何外部 Harness（Codex / DSH 均只作参考实现）。
> **分册索引**：[00 总纲](00-总纲.md) · [01 领域模型与状态机](01-领域模型与状态机.md) · [02 DSL 与校验](02-DSL与校验.md) · [03 调度器与生命周期](03-调度器与生命周期.md) · [04 节点接口与扩展点](04-节点接口与扩展点.md) · **05 Agent Harness** · [06 存储与可观测](06-存储与可观测.md) · [07 验收用例](07-验收用例.md) · [08 审计关闭矩阵](08-审计关闭矩阵.md)

---

## 5.1 范围、边界与选型

**一句话结论**：Harness 是 `agent_call` 节点的执行壳，**拥有**「加载配置 → 组装上下文 → 调 LLM → 执行工具 → 判定停止 → 返回 output」全流程；`genai` 只负责 HTTP/SSE transport，agent loop 与工具执行永远在本进程内。

### 5.1.1 硬边界（MUST）

| # | 约束 | 理由 / 归属 |
|---|---|---|
| H1 | Harness **只**通过 `NodeContext` 与框架交互（`ctx.emit` / `ctx.get_var` / `ctx.user_id` / `ctx.cancel_requested`），**不得**接触 `RunContext`、调度器、实例状态机 | 00 §0.7 消歧：Gate 用 `GateContext`，Harness 用 `NodeContext`；Harness 不是调度器 |
| H2 | Harness **不产生**图迁移事件；Harness 内部状态机（§5.5）不落 trace DB，只产生 `StreamingEvent` | 00 §0.6 I6 只认 4 类领域事件 + Streaming；避免自创事件类型 |
| H3 | Harness **不写** SQLite；所有可观测数据经 `NodeContext.emit` → 06 分册写路径 | 06 §6.4 |
| H4 | **Python 调用一律在 worker 子进程执行；Harness 不得在事件循环线程做阻塞调用**。进程内不 import / 不执行用户 Python（含工具模块）；工具执行由 worker 子进程承担（§5.6.5），Harness 只 async 等待其帧与退出 | 00 §0.6 I8；D3/D4/D7；v1 的「`spawn_blocking` 调进程内 Python」已作废（D13） |
| H5 | `AgentConfig` / prompt / 工具注册表均为**文件资产**，运行期只读；v1 不做热更新与资产写入 | 决策记录 §2.2 |
| H6 | 错误必须收敛到 00 §0.3 的 `HarnessErrorKind` 八值 + `NodeError` 七变体；**除 M4 追加的 `ContextOverflow`（D18，Lead 裁定）外不得新增枚举值** | 00 §0.3/§0.4；映射见 §5.7 |

### 5.1.2 组件分解

**v2 落地（M4 实况）**——Harness 在 Rust 进程内（`crates/zgraph-harness`），工具经 Python worker 子进程：

```
crates/zgraph-harness/src/
├── lib.rs         crate 导出与冻结公开面（HarnessExecutor/LlmProvider/...）
├── config.rs      §5.2 AgentConfig 加载与 V1–V16 校验
├── assemble.rs    §5.4.2 system/约束/记忆/KB/用户输入 → messages
├── budget.rs      §5.4 token 估算与裁剪（无分词器依赖）
├── constants.rs   §5.11 常量（与 00 §0.5 对齐）
├── ctx.rs         §5.3.1.1 HarnessCtx（测试 seam）+ CancelToken（轮询包装，D14/D15）
├── agent_loop.rs  §5.5 状态机 + iteration + 停止条件 + §5.3.5 重试编排（D16）
├── llm/
│   ├── mod.rs     §5.3.1 LlmProvider/LlmStreamSink trait 导出
│   ├── types.rs   §5.3.2 数据类型（不含 genai 类型）
│   ├── transport.rs §5.3.3 GenaiLlmProvider（genai 0.7.2 adapter）
│   └── fake.rs    §5.3.4 FakeLlmProvider（测试 / AC-08）
├── tools.rs       §5.6 工具注册表 + discover 缓存 + WorkerToolRunner/FakeToolRunner
├── error.rs       §5.7 HarnessError → NodeError 映射
└── executor.rs    `HarnessExecutor`：`agent_call` 的 `NodeExecutor` 实现（§5.9.4）

python/zgraph_worker/tool_runner.py   worker 侧工具协议：`discover(inputs, ctx)` / `call(inputs, ctx)`（§5.6.2.1）

（另：`src/test_support.rs` 为 `cfg(test)` 测试辅助，不属对外 API。）
```

| 组件 | 职责 | 关键约束 |
|---|---|---|
| `HarnessExecutor` | `NodeExecutor` 唯一入口；`agent_call` 实例执行时编排 Loading→Returning；持有工具发现缓存与 `HarnessSettings` | 只经 `NodeContext`（生产由 `HarnessCtx` 适配）交互（H1）；不写 SQLite（H3） |
| `LlmProvider` | 单次流式请求 + 错误分类；**不做重试** | 重试在 `agent_loop.rs`（D16）；sink 同步 1:1（§5.8） |
| `ToolRunner`（`WorkerToolRunner`/`FakeToolRunner`） | 工具发现（一次，进程内缓存）+ 每次调用 spawn 一个 worker 子进程 | 无进程内 Python；无工具模块跨调用缓存（D3/D13）；`FakeToolRunner` 供单测 |
| `tool_runner`（Python worker 侧） | 发现/调用用户工具模块；返回 JSON 对象或结构化失败 | 每个子进程独立解释器（D4）；错误以 worker `error`/`result` 帧回传 |

> 历史（v1.2 冻结时）的进程内组件划分（`transport.rs`/`stream.rs`/`retry.rs`/`fake.rs` + `spawn_blocking` 工具执行）已被 v2 取代；语义分工保持不变，物理边界改为「Rust Harness + worker 子进程」。

### 5.1.3 选型结论（不可再变）

| 层 | 方案 | 依据 | 变更成本 |
|---|---|---|---|
| LLM transport（HTTP/SSE/tool_call delta 聚合/usage） | **`genai` 0.7.2**，MIT OR Apache-2.0 | `_research/harness-alternatives.md` §5；原生 deepseek adapter + 自定义 OpenAI-compatible | 被自有 `LlmProvider` trait 隔离；换 async-openai / 自写客户端只需替换 adapter（决策记录 §3.4） |
| agent loop / 上下文 / 重试编排 / 错误映射 | **自研**（~1.2–1.5k LOC） | 决策记录 D2 | — |
| 工具执行 | **自研编排 + worker 子进程**（每次调用一进程，D3/D13） | 决策记录 D2；architecture §8 D3 | — |
| 工具协议 | v1 **不用 MCP**（`rmcp` 3.8 留给 v1.1 可选 feature） | `_research/harness-alternatives.md` §2.7 | 适配进自有 `ToolDef` 注册表 |
| 外部 Harness（Codex app-server / DSH ACP） | v1 **不引入**，只留 `AgentProvider` 扩展位（§5.9.4） | 决策记录 §3.2/§3.3；它们「拥有 loop」，与 H1–H3 冲突 | v2 候选 |

### 5.1.4 v1 明确不做（详见 §5.10）

Memory 检索/写入（stub 返回空）、KB 检索（stub 返回空）、多模态、MCP、人工介入（`NeedHumanInput` 不产生）、工具并行执行、provider 多路由/fallback、断点续跑、prompt 模板渲染、输出 JSON Schema 强校验。

---

## 5.2 AgentConfig（关闭审计 M-01）

**一句话结论**：AgentConfig 是**资产目录索引**里的文件资产（`<assets-dir>/agents/*.json`，按文件内 `id` 索引，见 02 §2.6.3），字段级 schema 在本节冻结；加载失败一律 `HARNESS_CONFIG_LOAD`（`NodeError::Harness{kind:ConfigLoad}` → 实例 `Crashed`）。

### 5.2.1 文件位置、发现与加载时机

| 项 | v1 规定 |
|---|---|
| 资产根 | CLI `--assets-dir <dir>` > env `ZGRAPH_ASSETS` > `.`（02 §2.6.3；06 §6.10） |
| 索引范围 | `<assets-dir>/agents/*.json`：**单层、不递归、按文件名排序读入**（保证确定性） |
| id 来源 | 文件内 `"id"` 字段；**文件名无关**，建议 `<id>.json` 但非强制（02 §2.6.3） |
| 资产工具目录 | CLI `--tools-dir <dir>` > env `ZGRAPH_TOOLS` > `<assets-dir>/tools`（§5.6.2） |
| 图加载期校验 | **只检查索引命中**：`agent_call.agent_id` 未命中 → `DSL_AGENT_NOT_FOUND`（02 §2.5 R14c，不产生 run） |
| 执行期加载 | **由 Harness 在 `agent_call` 实例执行时加载**：`HarnessExecutor::execute` 入口先做 Loading（读文件 + 全量校验 + 工具解析）；解析/校验失败 → `HARNESS_CONFIG_LOAD`（实例 `Crashed`）。运行期**每次实例执行都重新加载**（不在图加载期预解析） |
| 缓存 | v1 **不缓存**（单文件解析 < 1 ms，避免热更新歧义）；v1.1 可按 `(path, mtime)` 缓存。**工具发现**属例外：结果在 Harness 进程内按工具目录缓存一次（§5.6.2） |
| fake 模式 | `FakeLlmProvider`（§5.3.4）下**跳过 `model.api_key_env` 读取**（步 9），其余字段校验照常；否则 fake 场景需要无意义的密钥环境变量 |
| 路径安全 | `role_prompt_file` / `constraints_file` 相对**AgentConfig 文件所在目录**解析；允许绝对路径；解析后必须落在资产根或 CWD 之内，逃逸（`..`）→ `HARNESS_CONFIG_LOAD`。**M4 实现为词法归一化（未 `canonicalize`，符号链接逃逸未拦，§5.13.3 L2）** |
| 文件大小上限 | AgentConfig ≤ 256 KiB；prompt/constraints 各 ≤ 1 MiB；超限 → `HARNESS_CONFIG_LOAD` |
| 编码 | UTF-8（非法字节 → `HARNESS_CONFIG_LOAD`，`detail.reason="invalid_utf8"`） |

> **与 00 的重叠点**：`DSL_AGENT_NOT_FOUND`（图期）与 `HARNESS_CONFIG_LOAD`（运行期）都能表达"资产缺失/非法"。v1 裁定**按阶段分工**：图加载期只判索引命中（`DSL_AGENT_NOT_FOUND`），运行期判合法性与内容（`HARNESS_CONFIG_LOAD`）。两码不互斥，见 §5.11。
>
> **与 04 的边界**：04 §4.7 的 R1 ③ 曾在图加载期收集 `agent_call` 的 skill 符号；按 02 §2.6.3/§2.11 P-02-07 的裁定与 00 §0.8 的分工，**tools/skills 归 Harness Loading**（`HARNESS_CONFIG_LOAD`/`HARNESS_UNKNOWN_TOOL`），图加载期不解析。本分册按此执行。

### 5.2.2 Schema（字段级，v1 最终版）

顶层为 JSON 对象；**未知字段一律拒绝**（`deny_unknown_fields`，见 §5.10.2 保留字段）。

| 字段 | 类型 | 必填 | 默认 | 校验规则 | 说明 |
|---|---|---|---|---|---|
| `id` | string | ✅ | — | `^[a-z][a-z0-9_-]{0,63}$`；必须与资产索引键一致（02 §2.6.3 保证唯一） | 资产唯一标识；DSL `agent_id` 引用它 |
| `name` | string | ✅ | — | 1..128 字符，非空 | 人类可读名，只用于日志/trace 展示 |
| `schema_version` | integer | ❌ | `1` | 仅接受 `1` | 缺失视为 1；≠1 → `HARNESS_CONFIG_LOAD` |
| `model` | object | ✅ | — | 见下方子表 | 单一模型（v1 不做多模型路由） |
| `model.provider` | enum string | ✅ | — | `"deepseek"` \| `"openai_compatible"` | `deepseek` 走 genai 原生 adapter；`openai_compatible` 走自定义 endpoint |
| `model.name` | string | ✅ | — | `^[A-Za-z0-9._:-]{1,128}$` | 模型名，如 `deepseek-chat` / `deepseek-reasoner` |
| `model.base_url` | string \| null | 条件 | `null` | `provider="deepseek"` → 必须为 `null`；`"openai_compatible"` → 必填，`http(s)://` 开头、**不含尾斜杠**、不得以 `/chat/completions` 结尾 | OpenAI-compatible API root，如 `https://api.deepseek.com/v1` |
| `model.api_key_env` | string | ✅ | — | `^[A-Z_][A-Z0-9_]{0,63}$` | 环境变量名；**不内联密钥** |
| `model.temperature` | number | ❌ | `0.2` | `0.0 ≤ x ≤ 2.0` | 透传 provider |
| `model.max_tokens` | integer | ❌ | `2048` | `1 ≤ x ≤ 32768`，且 `x ≤ 0.2 × context_window_tokens` | 单次响应上限；参与预算公式 §5.4.5 |
| `model.context_window_tokens` | integer | ❌ | `65536` | `4096 ≤ x ≤ 2_000_000` | **预算总来源**（关闭 S-15）；非 DeepSeek 模型必须显式配置 |
| `role_prompt_file` | string | ✅ | — | 非空；解析后文件存在且可读 | 角色 prompt（Markdown，按纯文本注入） |
| `constraints_file` | string \| null | ❌ | `null` | 存在则可读 | 约束段（Markdown，注入 system 的 `## 约束`） |
| `tools` | string[] | ❌ | `[]` | 长度 ≤ 32；每项匹配工具名规则（§5.6.1）；**必须能在注册表中解析**，否则 `HARNESS_CONFIG_LOAD`（不是 `HARNESS_UNKNOWN_TOOL`） | 本 Agent 可见工具（LLM 只能调用这些；已注册但未列出的工具在运行期调用 → `HARNESS_UNKNOWN_TOOL`，REV-020 C2） |
| `input_mapping` | object\<string,string\> | ❌ | `{}` | key=**节点输入端口名**、value=**Agent 输入字段名**（方向与 02 §2.3.2 `AgentCallConfig.input_mapping` 一致）；key/value 非空、无重复 key | 节点端口 → Agent 输入字段；缺失时按同名（identity）映射 |
| `max_iterations` | integer | ❌ | `10`（`DEFAULT_MAX_ITERATIONS`） | `1 ≤ x ≤ 50` | **LLM 调用轮次上限**（语义见 §5.5.2） |
| `llm_timeout_ms` | integer | ❌ | `60000`（`DEFAULT_LLM_TIMEOUT_MS`） | `1000 ≤ x ≤ 600000` | **单次** LLM 调用（含流式全过程）超时 |
| `llm_max_retries` | integer | ❌ | `1`（`DEFAULT_LLM_MAX_RETRIES`） | `0 ≤ x ≤ 5` | 仅失败在"建立阶段"时生效（§5.3.5） |
| `node_timeout_ms` | integer | ❌ | `180000`（`DEFAULT_NODE_TIMEOUT_MS`） | `1000 ≤ x ≤ 3600000` | 该 Agent 作为 `agent_call` 节点时的**默认实例超时**；DSL 节点 `timeout_ms` 优先 |

> `node_timeout_ms` 的定位：它是"Agent 侧建议值"，只用于 DSL 未声明 `timeout_ms` 时的**节点超时回退**；真正的实例超时执行者是调度器（03 分册），Harness 只做 deadline 自检（§5.3.6）。

### 5.2.3 示例文件

`agents/message_parser.json`：

```json
{
  "id": "message_parser",
  "name": "消息解析器",
  "schema_version": 1,
  "model": {
    "provider": "deepseek",
    "name": "deepseek-chat",
    "base_url": null,
    "api_key_env": "DEEPSEEK_API_KEY",
    "temperature": 0.2,
    "max_tokens": 2048,
    "context_window_tokens": 65536
  },
  "role_prompt_file": "prompts/message_parser.md",
  "constraints_file": "prompts/global_constraints.md",
  "tools": ["now", "http_get"],
  "input_mapping": { "message": "user_message" },
  "max_iterations": 10,
  "llm_timeout_ms": 60000,
  "llm_max_retries": 1,
  "node_timeout_ms": 180000
}
```

自定义 OpenAI-compatible 端点（`agents/classifier.json` 片段）：

```json
"model": { "provider": "openai_compatible", "name": "my-model",
           "base_url": "https://llm.internal.example/v1", "api_key_env": "INTERNAL_LLM_KEY",
           "temperature": 0.0, "max_tokens": 1024, "context_window_tokens": 32768 }
```

### 5.2.4 加载流程（确定性顺序，MUST）

| 步 | 动作 | 失败 |
|---|---|---|
| 1 | 从资产索引取 `agent_id` 对应文件路径（02 §2.6.3：`<assets-dir>/agents/*.json` 按文件内 `id` 索引） | 索引不可读/未命中 → `HARNESS_CONFIG_LOAD{path,reason:"agent_not_indexed"}`（图期已由 `DSL_AGENT_NOT_FOUND` 拦截，此处为运行期兜底） |
| 2 | 读文件（字节上限 256 KiB） | 缺失/不可读/超限 → `HARNESS_CONFIG_LOAD` |
| 3 | UTF-8 解码 | 非法 → `HARNESS_CONFIG_LOAD{reason:"invalid_utf8"}` |
| 4 | JSON 解析（拒绝未知字段） | 语法/类型/未知字段 → `HARNESS_CONFIG_LOAD{json_path,expected,actual}` |
| 5 | `schema_version` 检查 | ≠1 → `HARNESS_CONFIG_LOAD` |
| 6 | 字段校验 V1–V16（§5.2.5） | 第一条失败即返回 |
| 7 | 解析 `role_prompt_file` / `constraints_file`（存在性、编码、路径安全） | `HARNESS_CONFIG_LOAD` |
| 8 | 解析 `tools`：注册表查表 + 生成 JSON Schema（§5.6.3） | 未注册工具 → `HARNESS_CONFIG_LOAD{field:"tools",tool:"..."}` |
| 9 | 读取 `model.api_key_env` 指向的环境变量（非空） | 缺失/空 → `HARNESS_CONFIG_LOAD{field:"model.api_key_env",env:"..."}`；**fake 模式（`HarnessSettings.fake_mode`）跳过此步并注入占位密钥 `"fake-mode"`**（§5.3.4） |
| 10 | 产出不可变 `AgentConfig` 值对象（含已读入的 prompt 文本） | — |

> 加载流程**不产生任何 trace 事件**；失败只体现为实例 `Crashed` + `instances.error`（§5.7、06 §6.2）。

### 5.2.5 校验规则表（全部 → `HARNESS_CONFIG_LOAD`）

| ID | 规则 | `detail` 关键字段 |
|---|---|---|
| V1 | 索引键 == 文件内 `id`（02 §2.6.3 已保证；此处复核） | `expected` / `actual` |
| V2 | `id` 匹配 `^[a-z][a-z0-9_-]{0,63}$` | `pattern` |
| V3 | `name` 非空且 ≤ 128 字符 | `length` |
| V4 | `schema_version ∈ {1}` | `supported:[1]` |
| V5 | `provider ∈ {deepseek, openai_compatible}` | `allowed` |
| V6 | `base_url` 条件必填且格式合法（§5.2.2） | `provider` / `base_url` |
| V7 | `deepseek` 时 `base_url` 必须为 null | `base_url` |
| V8 | `api_key_env` 匹配 `^[A-Z_][A-Z0-9_]{0,63}$` | `pattern` |
| V9 | `0.0 ≤ temperature ≤ 2.0` | `expected_range` |
| V10 | `1 ≤ max_tokens ≤ 32768` 且 `max_tokens ≤ 0.2 × context_window_tokens` | `max_tokens` / `context_window_tokens` |
| V11 | `4096 ≤ context_window_tokens ≤ 2_000_000` | `expected_range` |
| V12 | `1 ≤ max_iterations ≤ 50` | `expected_range` |
| V13 | `1000 ≤ llm_timeout_ms ≤ 600000`；`0 ≤ llm_max_retries ≤ 5`；`1000 ≤ node_timeout_ms ≤ 3600000` | `expected_range` |
| V14 | `tools` 长度 ≤ 32 且每项匹配工具名规则 | `tool` |
| V15 | `input_mapping` 的 key（端口名）/value（Agent 字段名）均非空；无重复 key；key 不在该节点声明端口集合内 → **丢弃该 key**（不失败；M4 无 warning 通道，由 §5.9.3 的生效映射规则体现） | `key` / `value` |
| V16 | 所有被引用文件在路径白名单内（不逃逸资产根/CWD） | `path` / `reason:"path_escape"` |

### 5.2.6 生效优先级（关闭 M-01 的"默认值说不清"）

| 参数 | 优先级（高 → 低） | 常量 |
|---|---|---|
| 实例超时 | test-hooks 强制值（仅测试构建）> DSL 节点 `config.timeout_ms` > 执行器 `timeout_hint`（Harness 实现 = `AgentConfig.node_timeout_ms`）> `DEFAULT_NODE_TIMEOUT_MS` | 00 §0.5；M5 口径见下 |
| 单次 LLM 超时 | `AgentConfig.llm_timeout_ms` > `DEFAULT_LLM_TIMEOUT_MS` | 00 §0.5 |
| LLM 重试次数 | `AgentConfig.llm_max_retries` > `DEFAULT_LLM_MAX_RETRIES` | 00 §0.5 |
| 迭代上限 | DSL 节点 `config.max_iterations`（非空）> `AgentConfig.max_iterations` > `DEFAULT_MAX_ITERATIONS`；节点级值在执行器内 `clamp(1, 50)` | 00 §0.5；04 分册 |
| 工具超时 | `ToolDef.timeout_ms` > `DEFAULT_TOOL_TIMEOUT_MS` | 00 §0.5 |

> 节点级 `max_iterations`（DSL 可选字段）覆盖 AgentConfig 的规则由 02/04 分册定义；本分册只定义"被解析出的最终有效值"如何被 loop 使用。
>
> **M5 口径（ISSUE-076，REV-026/REV-033）**：内核 `CompiledNode` 记录 `declared_timeout_ms`（区分「未声明」与「显式默认值」，历史 `timeout_ms` 已折叠默认值、不能用于判定）；`NodeExecutor` 新增**对象安全**的默认方法 `timeout_hint(&CompiledNode) -> Option<u64>`（默认 `None`），派发超时链 = **test-hooks force > declared > executor hint > `DEFAULT_NODE_TIMEOUT_MS`（180s）**。Harness 的 `timeout_hint` **仅对 `agent_call`** 返回 `Some`，内部用同步轻量函数 `peek_agent_node_timeout` 只读 `<assets_dir>/agents/*.json`、按文件内 `id` 命中并校验 `node_timeout_ms` 值域（**不查工具表/密钥/prompt**；失败/缺失返回 `None`，权威加载仍由 Loading 报 `HARNESS_CONFIG_LOAD`），并复用同一解析顺序计算 elapsed。**CLI 组合执行器（`RoutingExecutor`/`RecordingExecutor`）已转发 `timeout_hint`（REV-030）；e2e `RoutingExecutor` 同步转发（REV-032）**，端到端用例（AgentConfig=1800ms + 10s 慢 fake → `Failed/NODE_TIMEOUT`）已覆盖。ISSUE-076 在 kernel/harness/CLI 三层均落地，**M5 已修**。

### 5.2.7 `detail` 统一形态

`HARNESS_CONFIG_LOAD` 的 `detail` 固定为对象（写入 `instances.error` 的 `NodeError::Harness.detail`）：

```json
{ "stage": "load", "path": "agents/message_parser.json", "json_path": "$.model.provider",
  "reason": "unsupported_value", "expected": ["deepseek","openai_compatible"], "actual": "openai" }
```

| `reason` 取值 | 触发 |
|---|---|
| `file_missing` / `file_unreadable` / `file_too_large` / `invalid_utf8` | 文件层 |
| `json_parse` / `unknown_field` / `type_mismatch` / `missing_field` / `unsupported_value` / `out_of_range` | schema 层 |
| `unsupported_schema_version` | V4 |
| `path_escape` | V16 |
| `tool_not_registered` | 步 8 |
| `api_key_missing` | 步 9 |

**M4 新增 `detail.reason`（工具发现/校验，加载期；REV-020 #9 回写）**：

| `reason` 取值 | 触发 |
|---|---|
| `tools_dir_missing` | `discover` 输入目录缺失/非目录 |
| `tool_module_import` | 工具模块导入失败（含 discover worker 协议/worker 错误） |
| `duplicate_tool` | 跨模块同名工具 |
| `invalid_declaration` | TOOLS/TOOL 同时或都缺、`call` 不可调用、name/description/parameters 非法、非法 `timeout_ms`、discover 输出形状不符 |
| `tool_module_timeout` | `discover` 调用超时（默认 30s） |
| `tool_discover_cancelled` | `discover` 期间取消 |
| `tool_runner_spawn` | 工具 worker spawn 失败 |
| `tool_worker_crash` | 工具 worker 异常退出（discover 阶段） |
| `invalid_tool_timeout` | 工具声明 `timeout_ms` 类型/值域非法（1..=600000） |

> 该扩展集为**超集**（诊断价值），不改变 `HARNESS_CONFIG_LOAD` 的码位与 Crashed 映射。

---

## 5.3 `LlmProvider` 与适配器（关闭审计 M-04）

**一句话结论**：Harness 只依赖**自有** `LlmProvider` trait（流式 + 回调 + 最终聚合结果），`genai` 的全部类型都封在 adapter 内；`chat()` 阻塞式全量接口**不存在**——这是修正 v1.1「先 await 完整响应再逐 token 透传」矛盾的关键。

### 5.3.1 Trait（契约级伪代码，非最终 Rust 代码；M4 实况）

```rust
/// LLM transport 抽象：一次调用 = 一次完整请求（含流式增量回调与最终聚合结果）。
/// 本 trait 是 Harness 与 transport 之间唯一耦合点；不暴露任何 genai 类型。
/// **重试编排在 Harness 内部**（§5.3.5 / D16），adapter 只如实报告 LlmError。
#[async_trait::async_trait]
pub trait LlmProvider: Send + Sync {
    fn name(&self) -> &'static str;                 // 实现值："genai" | "fake"（v1.2 原写 genai-deepseek/openai-compatible 是语义说明）

    /// 发起一次流式请求（**单次尝试，不重试**）；
    /// 增量通过 sink 同步推送（非阻塞）；返回值为聚合后的最终结果。
    /// 取消：cancel 被触发时必须尽快 drop 底层请求并返回
    /// Err(LlmError{kind:Cancelled})。
    async fn chat_stream(&self, req: LlmRequest, sink: &mut dyn LlmStreamSink, cancel: &CancelToken)
        -> Result<LlmResponse, LlmError>;
}

/// 增量接收端：实现方只做「1:1 转发到 ctx.emit」，**不得阻塞、不得 await、不得做 IO**；
/// **不得在 Harness 内做时间窗聚合**（聚合唯一执行点 = 06 写线程，§5.8.3）。
pub trait LlmStreamSink: Send {
    fn on_delta(&mut self, delta: LlmDelta);   // Token/Reasoning 1:1 转发；ToolCall* 仅供 adapter 内部聚合
}
```

> `sink` 是同步 trait（不是 `async`）：唯一实现只做内存转换与 `ctx.emit` 1:1 转发（§5.8），因此可证明不阻塞 transport 的读循环。`ToolCallStarted/Args` 两个 `LlmDelta` 变体只供 adapter 内部聚合观察，生产 sink 忽略（不直接外发）。
>
> **v2 取消（D14）**：`NodeContext` **没有** `cancel_token()`、也不存在 tokio `CancellationToken`；Harness 自有 **`CancelToken`** = 对 `HarnessCtx` 的**只读轮询包装**（每 `CANCEL_POLL_INTERVAL_MS = 50` 轮询一次 `cancel_requested()`），adapter 在流循环边界调用 `cancel.is_cancelled()` 判定。语义仍是"尽快 drop 底层请求 + 返回 `LlmError{kind:Cancelled}`"，不是 tokio 的 `cancelled().await`。
>
> **测试 seam（D15）**：Harness 不直接依赖 `NodeContext`，而是依赖最小面 trait `HarnessCtx`（`emit`/`cancel_requested`/`node_deadline_remaining_ms`/`user_id`/`get_var`/`seed`/`instance_id`）；生产实现**唯一**是 `impl HarnessCtx for NodeContext`，测试可用内存实现驱动 loop 与 fake provider，无需构造内核 `NodeContext`。`HarnessCtx` 只是 `NodeContext` 的只读投影，**不新增**框架能力（H1）。

### 5.3.1.1 `HarnessCtx`（v2 测试 seam，`Send + Sync`）

```rust
/// Harness 与内核上下文之间的最小只读投影；生产实现 = `impl HarnessCtx for NodeContext`（唯一）。
pub trait HarnessCtx: Send + Sync {
    fn emit(&self, event: StreamingEvent);           // → NodeContext::emit（可丢、1:1）
    fn cancel_requested(&self) -> bool;              // 轮询取消（D14）
    fn node_deadline_remaining_ms(&self) -> u64;     // §5.3.6 deadline 自检
    fn user_id(&self) -> &str;
    fn get_var(&self, key: &str) -> Option<&Value>;
    fn seed(&self) -> u64;                           // D5 确定性种子
    fn instance_id(&self) -> &str;                   // 只读身份
}
```

- **不变量**：`HarnessCtx` 是 `NodeContext` 公开只读面的**子集**（7 个方法）；不得出现 `emit` 之外的写入/调度能力（H1、04 §4.2.2）。生产实现是 `impl HarnessCtx for NodeContext`，没有独立包装类型。
- **测试**：可用 `FakeHarnessCtx` 控制取消/截止时间并断言 `ctx.emit` 的 1:1 序列；fake provider 与 loop 单测均走该 seam。

### 5.3.2 数据类型（字段级，强制）

```rust
pub struct LlmRequest {
    pub model: ModelRef,              // provider + name + base_url
    pub messages: Vec<LlmMessage>,    // §5.4.1
    pub tools: Vec<ToolSchema>,       // §5.6.1；空数组表示不传 tools 字段
    pub temperature: f32,
    pub max_tokens: u32,
    pub stream: bool,                 // v1 恒为 true（全量接口不存在）
    pub api_key: SecretString,        // 已从环境变量解析，不落日志
    pub timeout_ms: u64,              // 本次调用生效值（min(llm_timeout_ms, 剩余 deadline)）
}

pub enum LlmDelta {
    Token { text: String },                    // 正文增量
    Reasoning { text: String },                // reasoning_content 增量
    ToolCallStarted { index: u32, call_id: String, name: String },
    ToolCallArgs { index: u32, delta: String },// 参数 JSON 增量（**不外发**，仅聚合）
}

pub struct LlmResponse {
    pub content: Option<String>,      // 无正文 → None（多 tool_call 首轮常见）
    pub reasoning: Option<String>,    // 完整 reasoning（若 provider 提供）
    pub tool_calls: Vec<ToolCall>,    // 已按 index 聚合、args 已解析为 Value
    pub stop_reason: StopReason,
    pub usage: Usage,
    pub latency_ms: u64,
    pub provider_request_id: Option<String>,
    pub attempts: u32,                // 含首次 = 1
}

pub struct ToolCall { pub call_id: String, pub name: String, pub args: Value, pub args_raw: String }
pub enum StopReason { EndTurn, ToolCalls, MaxTokens, ContentFilter, Other { raw: String } }
pub struct Usage { pub prompt_tokens: u32, pub completion_tokens: u32, pub total_tokens: u32,
                   pub reasoning_tokens: Option<u32>, pub source: UsageSource }
pub enum UsageSource { Provider, Estimated, Unavailable }

/// 错误：retry 决策所需的一切信息都在这里（Harness 不解析 provider 文本）
pub struct LlmError {
    pub kind: LlmErrorKind,       // Connect | Timeout | RateLimited | ServerError | ClientError
                                  // | StreamBroken | Decode | Cancelled
    pub phase: LlmPhase,          // Establish（尚未吐任何 delta）| Streaming（已吐）
    pub status: Option<u16>,
    pub retry_after_ms: Option<u64>,
    pub message: String,
    pub streamed_tokens: u32,     // 已成功 emit 的正文 token 数；>0 即视为已开始输出
}
```

**辅助类型（v1 冻结，均为 Harness 自有类型，不含 genai 类型）**：

| 类型 | 定义 | 说明 |
|---|---|---|
| `ModelRef` | `{ provider: ProviderKind, name: String, base_url: Option<String> }`；`ProviderKind ∈ {Deepseek, OpenAiCompatible}` | 由 `AgentConfig.model` 派生，v1 一实例一模型 |
| `AgentId` | `String` | 即 `AgentConfig.id` |
| `HarnessCtx` | 见 §5.3.1.1：`NodeContext` 的只读投影（`emit`/`cancel_requested`/`node_deadline_remaining_ms`/`user_id`/`get_var`/`seed`/`instance_id`） | **v2 唯一上下文 seam（D15）**；生产实现 = `impl HarnessCtx for NodeContext` |
| `CancelToken` | Harness 自有的**只读轮询包装**（包装 `HarnessCtx`；每 `CANCEL_POLL_INTERVAL_MS=50` 轮询 `cancel_requested()`；`is_cancelled()`） | **v2 替代 tokio `CancellationToken`（D14）**；只读、不提供 `cancelled().await`、不跨进程；来源 = run 级 `AtomicBool` 快照 |
| `SecretString` | 不实现 `Debug`/`Display` 的字符串包装 | 防止 `api_key` 泄漏到日志/trace；`Drop` 时尽力清零 |
| `ToolSchema` | 见 §5.6.1 | 与 `ToolDef` 分离：发给 LLM 的最小结构 |

**关键规则**：
- `LlmPhase` **由 adapter 维护**（"是否已向 sink 交付过任何 `LlmDelta::Token`/`Reasoning`"），不是由 Harness 猜。
- `LlmRequest.timeout_ms` 由 Harness 在每次尝试前算：`min(llm_timeout_ms, node_deadline_remaining)`；adapter 只负责在到点时终止本次流。
- `LlmResponse.attempts` 由 **Harness 重试循环**在成功返回前填写（`1 + 已重试次数`）；adapter 不承担重试计数（D16）。
- `tool_calls[].args` 必须是**完整可解析**的 JSON 对象；解析失败 → 该 tool call 视为 `TOOL_ERROR`（喂回 LLM，§5.6.6），**绝不执行半截参数**。
- `usage` 缺失时 `source=Unavailable`、三项填 0；**不得**用 0 冒充真实计量（下游按 `source` 判断）。

### 5.3.3 genai 0.7.2 adapter 映射（M4 实测口径）

> 本表按 **genai 0.7.2 实际 API**（`Client`/`ChatStreamEvent`/`webc::Error`）与 M4 adapter 实现回写；取代 v1.2 冻结期的"待验证"占位。genai 的全部类型仍封在 adapter 内（§5.3.1）。

| genai 0.7.2 侧（transport 事实） | adapter 映射 | 备注 |
|---|---|---|
| `Client::exec_chat_stream(model, ChatRequest, Option<&ChatOptions>) -> genai::Result<ChatStreamResponse>`；`ChatStreamResponse{stream: ChatStream, model_iden}` | adapter 驱动 `ChatStream` 事件循环，逐事件转换后 `sink.on_delta` | 建立阶段错误（`Err`）即 `phase=Establish` |
| `ChatOptions::with_capture_usage(true)` / `with_capture_content(true)` / `with_capture_reasoning_content(true)` / `with_capture_tool_calls(true)` | 请求前必须开启，否则 `StreamEnd.captured_*` 为 `None` | 四个 capture 选项缺一不可（tool_calls / usage / reasoning 都是最终值来源） |
| `ChatStreamEvent::Start` / `Heartbeat` | **忽略** | 仅保活 |
| `ChatStreamEvent::Chunk(StreamChunk{content})`（正文增量） | `LlmDelta::Token{text}` | 首次交付即 `phase=Streaming` |
| `ChatStreamEvent::ReasoningChunk(StreamChunk)` | `LlmDelta::Reasoning{text}` | `deepseek-reasoner` 场景；不进入正文 |
| `ChatStreamEvent::ThoughtSignatureChunk(...)` | **忽略**（v1 不产生思考签名） | 仅部分 provider 使用 |
| `ChatStreamEvent::ToolCallChunk(ToolChunk{tool_call})` | `LlmDelta::ToolCallStarted/Args`（内部聚合观察） | genai 已按 index 聚合并给出 `call_id`/`fn_name`/`fn_arguments`；**最终值以 `End` 的 `captured_tool_calls()` 为准** |
| `ChatStreamEvent::End(StreamEnd{..})` | `LlmResponse.{content, reasoning, tool_calls, stop_reason, usage, provider_request_id}` | `captured_texts()`/`captured_tool_calls()`/`captured_reasoning_content`/`captured_usage`/`captured_stop_reason`/`captured_response_id`；`captured_stop_reason` 的 `Completed/MaxTokens/ToolCall/ContentFilter/StopSequence/Other` 映射到自有 `StopReason` |
| `genai::Error::HttpError{status, headers, body}` | `LlmError{status, retry_after_ms}`；`status` → kind（429 RateLimited / 5xx ServerError / 408 Timeout / 其余 4xx ClientError） | `retry-after-ms` 优先、`retry-after`（秒）次之；body 截断 ≤512 字符进 message |
| `genai::Error::WebModelCall{webc_error, ..}` / `WebAdapterCall{webc_error, ..}` | 解包 `webc_error` 后分类：`ResponseFailedStatus{status, headers}` → status/kind；`Reqwest` → timeout/connect/HTTP status；`ResponseFailedNotJson`/`ResponseFailedInvalidJson` → Decode | `webc::Error` 提供 `status()`/`headers()` accessor |
| `genai::Error::WebStream{cause}` | `LlmError{kind: StreamBroken}` | 流传输层错误（已可能交付 delta） |
| `genai::Error::StreamParse{serde_error}` | `LlmError{kind: Decode}` | SSE 帧 JSON 解码失败 |
| `genai::Error::ChatResponse{body}` | `LlmError{kind: StreamBroken}` | provider 在流内返回 error event |
| 其它 `genai::Error` | Streaming 阶段 → `StreamBroken`；Establish 阶段 → `ClientError` | 兜底分类 |
| 流在 `End` 前 EOF | `LlmError{kind: StreamBroken, phase}` | 未收到 `ChatStreamEvent::End`（**不可重试**，已交付 delta 后一律不续流） |
| endpoint / auth 注入 | 统一用 `ServiceTarget{endpoint, auth, model}`：`deepseek` → `AdapterKind::DeepSeek` + 固定端点 `https://api.deepseek.com/v1/` + `AuthData::Key`；`openai_compatible` → `AdapterKind::OpenAI` + 端点 = `base_url` 补尾斜杠 + `AuthData::Key`（adapter 生成 `Authorization: Bearer`） | 直接走 `ServiceTarget` 路径，**无需** genai 的 `deepseek::<name>` 字符串命名空间；模型名原样使用 |
| 重试 | **genai 0.7.2 不自带重试**（只透出 retry 元信息） | 重试编排在 Harness（§5.3.5 / D16） |

> **实测结论（M4）**：genai 0.7.2 对 DeepSeek 流式 tool_call / reasoning 的行为满足契约（`End.captured_*` 提供最终值）；"退回自写 SSE 客户端"的 v1.2 风险预案**未触发**，契约不变。
>
> **错误分类不依赖 provider 文本**：adapter 只看 `webc::Error` 的结构化字段（status/headers/reqwest kind）；文本仅进 `LlmError.message` 供诊断。

### 5.3.4 Fake provider（测试与 `--fake-llm`）

**用途**：AC-01..AC-07 与 e2e（无网络，CI 默认）、单测；`zgraph run --fake-llm[=<script.json>]`（06 §6.10.3；07 §前言）。裸 `--fake-llm` 使用内置默认真实脚本（一段文本 + 一次工具调用），`--fake-llm=<path>` 指定脚本；`--real` 与 `--fake-llm` 互斥，`--real` 用真实 provider（仅 AC-08）。

> **M4 落地**：`FakeLlmProvider` 位于 `crates/zgraph-harness/src/llm/fake.rs`，是 `LlmProvider` 的**真实 trait 实现**（不绕过 loop/重试/sink）；M4 验收 = harness crate 内联单测（`crates/zgraph-harness/src/**` 的 `#[cfg(test)]`）+ e2e `tests/e2e/harness_ac08.rs`；**CLI `zgraph run --fake-llm` 链路属 M5**（CLI `run` 尚未实现，见 07 AC-01/AC-08）。

脚本 schema：

```json
{
  "version": 1,
  "chunk_chars": 3,            // 正文按 N 字符切分为 LlmDelta::Token（默认 3，用于验证 06 写线程分段边界）
  "chunk_delay_ms": 0,         // 每个 delta 之间的人为延迟
  "calls": [
    { "kind": "text", "content": "{\"intent\":\"refund\"}", "stop_reason": "EndTurn",
      "usage": { "prompt_tokens": 120, "completion_tokens": 12 } },
    { "kind": "tool_calls",
      "tool_calls": [ { "call_id": "call_1", "name": "now", "args": {} } ],
      "content": null, "stop_reason": "ToolCalls", "usage": { "prompt_tokens": 300, "completion_tokens": 20 } }
  ],
  "failures": [
    { "call_index": 0, "attempt": 0, "phase": "establish", "kind": "RateLimited",
      "status": 429, "retry_after_ms": 100 }
  ]
}
```

| 字段 | 类型 | 默认 | 语义 |
|---|---|---|---|
| `calls` | array | 必填 | 按顺序消费；用尽后重复最后一项（便于长循环测试） |
| `calls[].kind` | `text` \| `tool_calls` | 必填 | `text` → 无 tool_calls 的响应；`tool_calls` → 带 tool_calls 的响应 |
| `calls[].content` | string \| null | `null` | 正文（`text` 必填） |
| `calls[].reasoning` | string \| null | `null` | 可选：推理正文（M4 扩展，用于验证 `ReasoningDelta` 映射；只增不改） |
| `calls[].stop_reason` | string | 按 kind 推导（`EndTurn` / `ToolCalls`） | 映射到 `StopReason` |
| `failures[]` | array | `[]` | 注入失败：命中 `(call_index, attempt)` 时返回 `LlmError` |
| `failures[].phase` | `establish` \| `streaming` | 必填 | 决定可重试性 |
| `chunk_chars` / `chunk_delay_ms` | int | 3 / 0 | 流式切分与节奏 |

**Fake provider MUST**：① 通过真实 trait 实现（不得绕过 Harness 编排）；② 失败注入走同一 `LlmError` 路径（不得直接 panic）；③ `usage` 缺省时 `source=Estimated`，`total_tokens = prompt+completion`。

### 5.3.5 重试策略（精确规则，关闭 M-04 的"错误分类缺失"）

**一句话结论**：**重试编排在 Harness（不在 provider）；只有"建立阶段"可重试；一旦向 sink 交付过任何正文/推理 token，任何失败都立即失败，不重试、不续流。**

**编排位置（v2 / D16）**：`LlmProvider::chat_stream` 是**单次尝试**；`Harness` 的 loop 持有 `attempt = 0..=llm_max_retries` 的循环，按本表决定是否退避重试。**provider 不得内置重试**（genai 0.7.2 也不自带重试），否则形成双层重试、attempt 计数不可复现。重试期间状态机保持 `Invoking`、`iteration` 不增加（§5.5.2）。

| 情形 | `phase` | 可重试 | 动作 | 最终错误码 |
|---|---|---|---|---|
| 建连失败（DNS/TCP/TLS）、请求未被服务端接受 | Establish | ✅ | 退避后重试 | `HARNESS_LLM_ERROR` |
| HTTP 429 | Establish | ✅ | 优先 `retry_after_ms`，否则退避 | `HARNESS_LLM_ERROR` |
| HTTP 5xx | Establish | ✅ | 退避 | `HARNESS_LLM_ERROR` |
| HTTP 400 / 401 / 403 / 404 / 422（4xx，除 408） | Establish | ❌ | 立即失败（配置/鉴权问题重试无意义） | `HARNESS_LLM_ERROR` |
| HTTP 408 | Establish | ✅ | 退避后重试（实现按 Timeout 分类） | `HARNESS_LLM_TIMEOUT` |
| 单次调用超时，未吐 delta | Establish | ✅ | 退避后重试 | `HARNESS_LLM_TIMEOUT` |
| **流中断 / 超时，已吐 delta** | Streaming | ❌ | 立即失败，**丢弃已收到的部分内容**（不写入上下文） | `HARNESS_LLM_ERROR` / `HARNESS_LLM_TIMEOUT` |
| 取消（`cancel_requested` 或调度器放弃） | 任意 | ❌ | 立即返回取消 | `NodeError::Cancelled`（非 Harness 错误，§5.7.3） |

**退避与预算**：

| 项 | 规则 |
|---|---|
| 总请求次数 | `1 + llm_max_retries`（默认 2 次：首次 + 1 次重试） |
| 第 k 次重试（k 从 1 计）等待 | `min(1000ms × 2^(k-1), 8000ms)` → 1s、2s、4s、8s 上限 |
| `retry-after` 优先 | 存在 `retry_after_ms` 时以其为准；上限 `RETRY_AFTER_MAX_MS = 30000`；缺失才用退避 |
| 与实例 deadline 的关系 | 若 `now + wait ≥ node_deadline`，**不再重试**，直接以当前错误失败（避免"重试必然超时"的浪费） |
| 与 `max_iterations` 的关系 | **重试不消耗 iteration**（iteration 只在成功拿到一个可解析响应时 +1，见 §5.5.2） |
| usage 归集 | 只累计**成功响应**的 usage；失败尝试的 token 无法获知，不计入 |
| `attempts` 记录 | 写入 `LlmResponse.attempts`，错误时写入 `detail.attempts` |

> **与 00 §0.5 的口径（已闭合，G1）**：00 §0.5 现写明 `DEFAULT_LLM_MAX_RETRIES = 1`（= **1 次重试**）、退避基数 `BACKOFF_BASE_MS = 1000`（第 2 档 2000ms，默认不触发），与本分册一致；`2s` 是 `llm_max_retries ≥ 2` 时的第二档退避，不是"共 2 次重试"。

### 5.3.6 超时分层（谁先到谁生效）

| 层级 | 默认 | 执行者 | 触发后结果 |
|---|---|---|---|
| 实例超时（`agent_call` 全部轮次） | 180s（DSL 覆盖） | 调度器 watchdog（03 分册） | 实例 `TimedOut`、`NODE_TIMEOUT`、run `Failed`（冷峻）；v2 **kill 进程组**、`killed_workers += 1`、**不产生** `NodeAbandoned`（D1） |
| Harness deadline 自检 | 同实例超时 | Harness（每次 LLM 调用前） | 剩余 ≤ 0 → 返回 `NodeError::Timeout{elapsed_ms}`（与调度器同语义，不抢跑状态） |
| 单次 LLM 调用 | 60s | `LlmRequest.timeout_ms` | `HARNESS_LLM_TIMEOUT`（可按 §5.3.5 重试） |
| 单次工具调用 | 30s | Harness → worker supervisor（§5.6.5；超时 = **kill 进程组**，D1/D13） | `TOOL_ERROR{reason:"timeout"}` 喂回 LLM（**不终止**） |

**计算式**：`request_timeout = min(llm_timeout_ms, node_deadline_remaining)`；若 `node_deadline_remaining < 1000ms`，不再发起新调用，直接返回实例超时错误。

> **M4 实现补充**：除 `LlmRequest.timeout_ms` 外，Harness 还用 `tokio::time::timeout(timeout_ms, provider.chat_stream(...))` 做**兜底超时**（防 adapter 不响应）；到点后按已转发增量数判定 `LlmPhase`（`sink.started()` → Streaming / 否则 Establish），再按 §5.3.5 决定可重试性（`crates/zgraph-harness/src/agent_loop.rs`）。剩余 `<1000ms` 时不发新调用、直接 `InstanceTimeout`。

### 5.3.7 取消（v2：`CancelToken` = 轮询包装，无 tokio CancellationToken）

| 场景 | 行为 |
|---|---|
| `ctx.cancel_requested()` == true（LLM 调用前） | 立即返回 `NodeError::Cancelled`，不发新请求 |
| LLM 调用中取消 | adapter 在流循环边界检查 `CancelToken::is_cancelled()`（`Harness` 每 `CANCEL_POLL_INTERVAL_MS=50` 轮询 `HarnessCtx::cancel_requested()`）；为 true 时 drop 底层 future（取消 = reqwest future drop，不保证 TCP FIN）→ 返回 `LlmError{kind:Cancelled}` → 映射 `NodeError::Cancelled`（§5.7.2 补充映射） |
| 工具执行中取消 | Harness **不再等待**：丢弃 `run_worker_controlled` future → `Supervisor::drop` **立即 `SIGKILL` 进程组**（`crates/zgraph-worker/src/supervisor.rs:182-186`）；Harness 返回 `NodeError::Cancelled`。**不产生** `NodeAbandoned`/`abandoned_tasks`（D1） |
| 取消后的部分输出 | 全部丢弃：不写上下文、不进 output；已 emit 的 `TokenDelta` 保留在 trace（已发生的事实） |

> **D14**：v2 **不存在 tokio `CancellationToken`**，`NodeContext` 也没有 `cancel_token()`（见 04 §4.2.2）；Harness 自有的 `CancelToken` 只是对 `HarnessCtx` 的**轮询包装**（`CANCEL_POLL_INTERVAL_MS=50`，`is_cancelled()`）。取消的唯一来源是 run 级 `AtomicBool` 的只读快照，轮询点固定为：每次 LLM 尝试前、流事件循环每个事件之间、每次工具调用前后。

---

## 5.4 上下文组装与 Token 预算（关闭 S-15；对齐 00 §0.5）

**一句话结论**：消息序列 = **system（角色+约束+[KB]+[长期记忆]）→ 短期记忆 → 用户输入**；预算总量来自 `model.context_window_tokens`，裁剪只允许"丢最旧短期记忆轮次"，**永不截断用户输入与 system**，仍超 → `HARNESS_CONTEXT_OVERFLOW`（Crashed）。

### 5.4.1 消息类型（v1 权威定义）

```rust
pub enum LlmMessage {
    System   { content: String },
    User     { content: String },
    Assistant{ content: Option<String>, tool_calls: Vec<ToolCall> },   // tool_calls 为空即普通 assistant
    Tool     { tool_call_id: String, name: String, content: String },  // 工具结果（JSON 字符串）
}
```

| 约束 | 规定 |
|---|---|
| `Tool.tool_call_id` | **必填**；必须与上一条 `Assistant.tool_calls[i].call_id` 一一对应，顺序一致 |
| `Assistant.content` | 可为 `None`（纯 tool_call 轮）或 `Some("")`；下游按 `Option` 处理 |
| 消息序列合法性 | 每次 LLM 请求前断言：所有 `Tool` 消息都能在同一序列的 `Assistant.tool_calls` 中找到对应 id，且不存在孤立 tool 消息 → 违反即 `NodeError::Internal`（fail-fast，属于框架 bug） |
| 角色约束 | 序列中 `System` 至多 1 条且仅在首位；`User` 至少 1 条（v1：仅首轮用户输入，多轮由 tool 结果推进） |
| 文本编码 | 全部 UTF-8；单条消息内容 > 256 KiB → 视为框架 bug（`Internal`） |

### 5.4.2 组装顺序（确定性，逐字节可复现）

`assemble_messages(agent, inputs) -> Vec<LlmMessage>`：

| 序 | 段 | 来源 | v1 是否为空 | 拼接规则 |
|---|---|---|---|---|
| ① | system 头部 | `role_prompt_file` 原文（不渲染模板，`{{var}}` 原样保留） | 否 | 原样 |
| ② | `\n\n## 约束\n` | `constraints_file` 原文 | 可为空 | 文件为 null/空 → **整段（含标题）省略** |
| ③ | `\n\n## 知识库参考\n` | KB 适配器 | **v1 恒空** | 空 → 整段省略（§5.10.1） |
| ④ | `\n\n## 关于用户的长期记忆\n` | Memory 适配器 | **v1 恒空** | 空 → 整段省略 |
| ⑤ | 短期记忆消息 | Memory 适配器 | **v1 恒空** | 插在 system 之后、user 之前（v1.1 启用，顺序在此冻结） |
| ⑥ | user 消息 | `input_mapping` 映射后的 `user_inputs` | 否 | `serde_json::to_string_pretty`（2 空格缩进、**key 升序**），整体作为一条 user 消息 |
| ⑦ | assistant / tool 消息 | §5.5 循环追加 | — | 每轮响应与工具结果依次追加 |

**system 文本布局（v1 冻结）**：

```text
<role_prompt 原文>

## 约束
<constraints 原文>

## 知识库参考
<每文档 rendered 用 "\n---\n" 分隔>            ← v1 不出现

## 关于用户的长期记忆
<- 一条 fact 一行>                              ← v1 不出现
```

> 规则：**空段连同标题整段省略**（避免空标题浪费 token 且便于测试断言）；段间固定两个换行。

**user 输入的确定性序列化**：

| 项 | 规定 |
|---|---|
| 对象来源 | `input_mapping` 优先；缺失时 identity（key = 端口名） |
| 键序 | 升序（`BTreeMap`），保证同输入同字符串 |
| 端口值为 `null`（`trigger=any` 未到端口） | 该字段**省略**（不写 `null`） |
| 全部字段缺失 | user 内容 = `{}`（仍发送一条 user 消息，满足"至少 1 条 User"） |
| 嵌套 `Value` | 原样保留，pretty 打印 |

### 5.4.3 预算表（引用 00 §0.5）

| 段 | 注入位置 | 默认预算（token） | v1 实际来源 | 是否可裁剪 |
|---|---|---|---|---|
| system prompt | system 消息头 | 2_000 | `role_prompt_file` | ❌ 不截断（保真） |
| 约束 | system 消息 | 500 | `constraints_file` | ❌ 不截断 |
| 知识库文档 | system 消息 | 1_500（top-3） | v1 空（0） | ❌（v1 无内容） |
| 长期记忆 | system 消息 | 500（top-5） | v1 空（0） | ❌（v1 无内容） |
| 短期记忆 | 消息序列中部 | 3_000（最近 20 轮） | v1 空（0） | ✅ **按轮丢弃** |
| 用户输入 | 末尾 user 消息 | **剩余预算** | `input_mapping` | ❌ 永不截断（超限 → `HARNESS_CONTEXT_OVERFLOW`） |
| 工具 JSON Schema | 请求 `tools` 字段（非消息） | 无独立预算，计入强制段 | `tools` 解析结果 | ❌ |
| 工具结果消息 | 序列中 `Tool` 消息 | 无独立预算，计入强制段；单条上限 8 KiB | 工具返回值 | 单条按 §5.6.5 截断 |

### 5.4.4 Token 估算器（v1 无分词器依赖）

**一句话结论**：v1 用**确定性字符近似**（误差 ±20%），并用 `prompt_budget = floor(context_window × 0.8) − max_tokens` 的 20% 安全系数吸收误差；不引入 tiktoken/分词器依赖。

`estimate_tokens(s)`（对一条文本）：

| 字符类 | 计数 | 权重 |
|---|---|---|
| ASCII（U+0000–U+007F） | `n_ascii` | `n_ascii / 4`（向上取整到整体） |
| CJK 统一表意文字 / 全角标点（U+2E80–U+9FFF、U+3000–U+303F、U+FF00–U+FFEF） | `n_cjk` | `n_cjk × 1` |
| 其他非 ASCII（emoji、其他文字） | `n_other` | `n_other × 2` |

`tokens = ceil(n_ascii / 4 + n_cjk + n_other × 2)`；消息级再加 4 token/条的协议开销。

> **M4 实现口径**（`crates/zgraph-harness/src/budget.rs::estimate_message_tokens`）：每条消息 +4 协议开销；`Assistant` 额外计 `tool_calls[].name` 与 `args_raw` 的估算，`Tool` 消息额外计 `name`。

**总预算公式**（每个 `agent_call` 实例执行时计算一次）：

```
prompt_budget_tokens = floor(model.context_window_tokens × 0.8) − model.max_tokens
```

默认值（`context_window_tokens=65536`、`max_tokens=2048`）→ `prompt_budget = 50380`。

> 20% 安全系数同时吸收：估算误差、provider 侧 chat template 开销、DeepSeek 侧 `reasoning_content` 计费口径差异。**不使用**"实际 usage 回填校准"（v1.1），但每次成功响应必须把 `usage` 写入 trace（§5.9.2），为 v1.1 校准留数据。

### 5.4.5 裁剪算法（MUST 按此顺序）

每次 LLM 调用前执行 `check_and_trim(messages, tool_schemas)`：

| 步 | 动作 | 失败/结果 |
|---|---|---|
| 1 | 计算 `mandatory = est(system) + est(tool_schemas) + est(当前所有 User/Assistant/Tool 消息)` | — |
| 2 | 计算 `short_term = est(短期记忆消息)`（v1 恒 0） | — |
| 3 | while（v1 不触发）`mandatory + short_term > prompt_budget` **且** 短期记忆非空 → 丢弃**最旧一轮**（user+assistant+其全部 tool 消息为一组） | 每丢一轮计数 `trimmed_rounds += 1` |
| 4 | 若 `mandatory > prompt_budget` → 返回 `HarnessError{kind: ContextOverflow, detail:{...}}` | 实例 `Crashed`（00 §0.4：v1 不静默截断用户输入） |
| 5 | 通过 → 发起 LLM 调用 | — |

**禁止路径（关闭 S-15 的"归档后继续"死循环）**：

| 禁止项 | 原因 |
|---|---|
| ❌ "溢出 → 归档到长期记忆 → 清空短期 → 继续" | v1 无 Memory；即使 v1.1 有，`iteration` 不因裁剪重置，仍可能每轮溢出 → 必须走 `ContextOverflow` |
| ❌ 截断 system / 约束 | 破坏角色与安全约束，且不可诊断 |
| ❌ 截断 user 输入 | 00 §0.4 明确 v1 不静默截断用户输入 |
| ❌ 丢弃当前轮的工具结果消息 | 破坏 `Assistant.tool_calls` ↔ `Tool.tool_call_id` 对应，序列非法 |

**溢出时不变量**：`HARNESS_CONTEXT_OVERFLOW.detail` 必须给出分段估算（见 §5.7.4），便于用户定位是"prompt 太长"还是"历史/tool 结果太多"。

### 5.4.6 多发一轮的边界：工具结果增长

- 同一实例内，`Tool` 消息只增不减（属于"当前请求上下文"，不是短期记忆，不参与裁剪）。
- 若第 N 轮后 `mandatory` 已超预算 → 下一次调用前即 `HARNESS_CONTEXT_OVERFLOW`（**不会**为了腾空间而删除工具结果）。这是"冷峻"语义：宁可失败并给诊断，不静默丢数据。
- 工具结果单条上限：序列化 JSON ≤ 8 KiB（`DEFAULT_TOOL_RESULT_MAX_BYTES`，00 §0.5），超出按 §5.6.5 截断并加标记。

---

## 5.5 Harness 状态机与执行循环（关闭 S-16 / P-05 / P-06 / P-09）

**一句话结论**：六状态 `Loading → Assembling → Invoking ⇄ ToolExec → Returning | Failed`；`iteration` = **LLM 请求轮次**（每次成功发出并解析的请求 +1），单响应多 tool_call **串行执行且只 +1**。

### 5.5.1 状态机（Harness 内部状态，不落 trace DB）

| 状态 | 进入条件 | 允许的下一状态 | 触发失败 |
|---|---|---|---|
| `Loading` | 节点开始执行 | `Assembling` | `Failed(ConfigLoad)` |
| `Assembling` | 配置就绪 | `Invoking`（预算通过） | `Failed(ContextOverflow)` |
| `Invoking` | 组装完成 / 工具结果回填完成 | `ToolExec`（响应含 tool_calls）、`Returning`（无 tool_calls）、`Failed` | `Failed(LlmError/LlmTimeout/MaxIterations/ContextOverflow)` |
| `ToolExec` | 响应含 ≥1 个 tool_call | 回到 `Invoking`（全部 tool 处理完） | `Failed(UnknownTool)` |
| `Returning`（终态） | 无 tool_calls 的响应 | — | — |
| `Failed`（终态） | 任一不可恢复错误 / 取消耗尽 | — | — |

**重试位置（v2 / D16）**：重试在 **Harness 的 `Invoking` 内部**编排（`agent_loop.rs` 的 `invoke_with_retry`，按 §5.3.5 判定）；`LlmProvider` 单次尝试、**不**自行重试。**状态机不因重试而改变状态**，也不增加 iteration。

**边界规则**：

| 规则 | 规定 |
|---|---|
| 不变量 | `ToolExec` 至少对应 1 个已发出的 `ToolCallStart`；`Returning/Failed` 进入前 Harness **没有待发送的本地缓冲**（1:1 emit，无本地聚合窗口；聚合 flush 归 06 写线程，§5.8.3） |
| 取消 | 任意状态检查 `ctx.cancel_requested()` → 直接 `Failed(Cancelled 语义)`，由节点层映射为 `NodeError::Cancelled` |
| deadline | `Invoking` 入口检查 `node_deadline_remaining`；≤0 → `NodeError::Timeout`（§5.3.6） |
| 状态不落库 | 上述状态只出现在 tracing/spans 与 `RUST_LOG=debug` 日志；trace DB 只有 `StreamingEvent` + 实例迁移（06 §6.3） |

### 5.5.2 iteration 计数（关闭 S-16）

| 问题 | v1 裁定 |
|---|---|
| iteration 的语义 | **LLM 请求轮次**（一次请求 = 一次"对话回合"），不是 tool 轮次、不是 token 数 |
| 何时 +1 | 在**发起请求之前** +1（因此 `iteration` 在任一时刻 = 本实例已发起的 LLM 请求数） |
| 重试是否 +1 | ❌ 不 +1（同一轮请求的多次尝试属于同一 iteration） |
| 单响应含 N 个 tool_call | 只 +1（该响应属于同一轮）；N 个工具**串行**执行，逐个发 `ToolCallStart/End` |
| 上限判定 | 循环顶部 `if iteration >= max_iterations` → `HARNESS_MAX_ITERATIONS`（即最多发起 `max_iterations` 次请求） |
| 达到上限时最后一条文本 | 若最后一次响应含 tool_calls → 失败（`MaxIterations`，无可返回文本）；若含文本 → 已在 `Returning` 前返回，不会撞上限 |
| 失败重试消耗 iteration | ❌ 不消耗 |

**示例**（`max_iterations=3`）：请求#1 返回 2 个 tool_call → 串行执行 2 个工具 → 请求#2 返回 1 个 tool_call → 执行 → 请求#3 返回文本 → `Returning`，`iterations=3`。

### 5.5.3 单响应多 tool_call（v1 行为表）

| 方面 | v1 规定 |
|---|---|
| 执行顺序 | **串行、按响应中 `tool_calls` 数组顺序**（v1 不做并行；`_research/harness-alternatives.md` 提到 DSH 有 `maxParallelToolCalls`，v1 不引入） |
| 每个调用 | 先 emit `ToolCallStart`，再执行，再 emit `ToolCallEnd` |
| 消息回填 | 先追加 1 条 `Assistant{content, tool_calls: 全部}`，再按序追加 N 条 `Tool{tool_call_id, ...}` |
| 单个失败 | 该 `Tool` 消息内容 = 错误 JSON；**其余调用继续执行**（不因一个失败中断整批） |
| 未知/未授权工具（任一） | 立即 `HARNESS_UNKNOWN_TOOL` → `Crashed`（**与是否已在 discover 注册无关**：不在 `AgentConfig.tools` 可见集即视为未知，REV-020 C2）；已执行的调用结果不写上下文（但 trace 中保留已发生的 `ToolCallEnd`） |
| 数量上限 | 不做硬限制；若单响应 tool_calls > 32，视为 provider 异常 → `HARNESS_LLM_ERROR`（`detail.reason="too_many_tool_calls"`） |
| 空 tool_calls + 空 content | 视为"模型无输出"：追加 assistant 空消息并继续下一轮；连续 2 次空响应 → `HARNESS_LLM_ERROR`（`reason="empty_response"`） |

> **空响应裁定（D17，M4）**：**本表为权威**——单次空响应**不是**错误（追加 `Assistant{content: None, tool_calls: []}` 空 assistant 消息后继续下一轮），连续 **2 次**空响应才报 `HARNESS_LLM_ERROR{reason:"empty_response"}`。§5.5.4 旧伪代码中"单次空响应即 error"的写法**作废**（该伪代码已按本裁定改写）。

### 5.5.4 执行循环（契约级伪代码，**非最终 Rust 代码**）

> **说明**：以下伪代码只冻结**语义**（调用顺序、错误分支、iteration 规则）。所有权/借用写法仅为示意——实现可按 03 分册的所有权模型调整。v1.1 的三处硬伤在此修正：① 无阻塞 `chat()`，改为 `chat_stream` + sink；② 无 use-after-move（先解构响应再使用）；③ 无命名参数语法（用 struct 构造）；④ 不在 `&RunContext` 上调 `emit`（只经 `HarnessCtx` 投影）。**M4 v2 修订**：取消改为轮询（D14）、重试编排进 Harness（D16）、工具经 worker 子进程（D13）、空响应采信 §5.5.3（D17）。

```rust
// 契约级伪代码（非最终 Rust 代码）；M4 实况：取消=轮询、重试在 Harness、工具经 worker（D13–D17）
async fn run_agent(agent: &LoadedAgent, registry: &ToolRegistry, inputs: &HashMap<PortName, Value>,
                   ctx: Arc<dyn HarnessCtx>, cancel: CancelToken, llm: &dyn LlmProvider)
    -> Result<AgentOutput, RunFailure> {
    // ── Phase Loading：HarnessExecutor 在调用本函数前完成（配置已校验；tools 已 discover 并解析）
    let tool_schemas: Vec<ToolSchema> = registry.schemas(&agent.config.tools);   // 见 §5.6

    // ── Phase Assembling
    let mut messages: Vec<LlmMessage> = assemble_messages(agent, inputs);        // §5.4.2
    check_and_trim(&mut messages, &tool_schemas, budget)?;                       // §5.4.5

    let mut iteration: u32 = 0;
    let mut usage_total = Usage::zero();
    let mut consecutive_empty: u8 = 0;                                           // D17：连续空响应计数

    loop {
        if iteration >= max_iterations {
            // 00 §0.4：携带当前上下文摘要（不返回部分输出）
            return Err(RunFailure::Harness(HarnessError::max_iterations(iteration, ..)));
        }
        if cancel.is_cancelled() {                                               // D14：轮询取消
            return Err(RunFailure::Cancelled);
        }
        if ctx.node_deadline_remaining_ms() == 0 {
            return Err(RunFailure::InstanceTimeout { elapsed_ms: .. });          // → NodeError::Timeout
        }

        iteration += 1;
        ctx.emit(StreamingEvent::Progress {
            percent: progress_percent(iteration, max_iterations),                // 见 §5.8.2
            message: format!("llm call {iteration}/{max_iterations}"),
        });

        // ── Phase Invoking：sink 只做 1:1 转发（Harness 不聚合；100ms 聚合在 06 写线程，§5.8.3）
        // 重试/超时/错误映射都在 invoke_with_retry（D16）；LlmProvider 只做单次尝试
        let mut sink = StreamSink::new(ctx.clone());
        let request = build_request(agent, &messages, &tool_schemas);
        let response = invoke_with_retry(llm, &request, &mut sink, &ctx, &cancel, agent).await?;
        usage_total.accumulate(&response.usage);

        // 先解构，杜绝 v1.1 的 use-after-move
        let LlmResponse { content, tool_calls, stop_reason, .. } = response;

        // ── Phase ToolExec（串行；iteration 不因工具数增加）
        if !tool_calls.is_empty() {
            consecutive_empty = 0;
            if tool_calls.len() > MAX_TOOL_CALLS_PER_RESPONSE {
                return Err(RunFailure::Harness(HarnessError::llm_reason(iteration, "too_many_tool_calls")));
            }
            messages.push(LlmMessage::Assistant { content: content.clone(), tool_calls: tool_calls.clone() });
            for call in tool_calls.iter() {
                // 未知工具：立即失败（不是 TOOL_ERROR）
                let Some(tool) = registry.get(&call.name) else {
                    return Err(RunFailure::Harness(HarnessError::unknown_tool(&call.name, iteration, &registry.names())));
                };
                ctx.emit(StreamingEvent::ToolCallStart {
                    call_id: call.call_id.clone(),
                    tool_name: call.name.clone(),
                    args_summary: summarize_args(&call.args),                    // 见 §5.8.5
                });

                // D13：每次调用 spawn 一个 worker 子进程；超时 = kill 进程组 → TOOL_ERROR
                let outcome = execute_tool(&tool, call, &ctx, &cancel).await;     // §5.6.5
                let (ok, tool_content) = match outcome {
                    Ok(value) => (true, serialize_tool_result(&value)),          // 单条 ≤ 8 KiB
                    Err(tool_err) => (false, tool_error_content(&tool_err)),     // 喂回 LLM，继续循环
                };
                ctx.emit(StreamingEvent::ToolCallEnd {
                    call_id: call.call_id.clone(),
                    tool_name: call.name.clone(),
                    ok,
                    result_summary: Some(summarize_result(&tool_content, ok)),   // Option<Value>（§5.8.5）
                });
                messages.push(LlmMessage::Tool {
                    tool_call_id: call.call_id.clone(),
                    name: call.name.clone(),
                    content: tool_content,
                });
            }
            continue;      // 回到 Invoking（下一个 iteration）
        }

        // ── Phase Returning（无 tool_calls）
        let final_content = content.unwrap_or_default();
        if final_content.is_empty() {
            // D17：先追加空 assistant（content: None），再判连续 2 次 → empty_response。
            consecutive_empty += 1;
            messages.push(LlmMessage::Assistant { content: None, tool_calls: Vec::new() });
            if consecutive_empty >= 2 {
                return Err(RunFailure::Harness(HarnessError::llm_reason(iteration, "empty_response")));
            }
            continue;
        }
        return Ok(AgentOutput { content: final_content, stop_reason, iterations: iteration, usage: usage_total });
    }
}

/// 单次 LLM 调用 + Establish 重试编排 + 超时兜底（D16；§5.3.5）：只有 Establish 阶段可重试。
async fn invoke_with_retry(llm: &dyn LlmProvider, request: &LlmRequest, sink: &mut dyn LlmStreamSink,
                           ctx: &Arc<dyn HarnessCtx>, cancel: &CancelToken, agent: &AgentConfig)
    -> Result<LlmResponse, RunFailure> {
    let max_attempts = agent.llm_max_retries + 1;
    let mut attempt: u32 = 0;
    loop {
        if cancel.is_cancelled() {
            return Err(RunFailure::Cancelled);
        }
        let remaining = ctx.node_deadline_remaining_ms();
        if remaining == 0 {
            return Err(RunFailure::InstanceTimeout { elapsed_ms: .. });
        }
        let timeout_ms = min(agent.llm_timeout_ms, remaining);
        attempt += 1;
        // 每次尝试都带 timeout；Harness 侧再用 tokio::time::timeout 兜底（adapter 不判超时）
        let mut req = request.with_timeout(timeout_ms);
        let outcome = tokio::time::timeout(Duration::from_millis(timeout_ms),
                                           llm.chat_stream(req, sink, cancel)).await;
        let error = match outcome {
            Ok(Ok(mut response)) => { response.attempts = attempt; return Ok(response); }
            Ok(Err(error)) => error,
            Err(_) => LlmError::timeout_with_phase(sink.streamed()),            // phase 由已转发增量判定
        };
        // 取消与实例 deadline 优先于 provider 错误分类（§5.7.3）
        if cancel.is_cancelled() || error.is_cancelled() {
            return Err(RunFailure::Cancelled);
        }
        if ctx.node_deadline_remaining_ms() == 0 {
            return Err(RunFailure::InstanceTimeout { elapsed_ms: .. });
        }
        if error.is_retryable() && attempt < max_attempts {
            // retry-after 优先（≤ RETRY_AFTER_MAX_MS），否则 backoff（≤ BACKOFF_MAX_MS）
            let wait = error.retry_after_ms.map(|ms| ms.min(RETRY_AFTER_MAX_MS))
                .unwrap_or_else(|| (BACKOFF_BASE_MS * (1 << (attempt - 1))).min(BACKOFF_MAX_MS));
            if ctx.node_deadline_remaining_ms() > wait {
                sleep_or_cancel(ctx, wait).await?;                              // 等待期间可取消
                continue;
            }
        }
        return Err(RunFailure::Harness(map_invoke_error(&error, iteration, attempt, timeout_ms)));
    }
}

```

> **关于取消/实例超时的承载**：v1.2 冻结期伪代码用 `HarnessError::cancelled()` / `HarnessError::instance_timeout(ctx)` 两个**哨兵构造器**；**M4 实现改为 `RunFailure` 枚举承载**（`Cancelled` / `InstanceTimeout{elapsed_ms}` / `Harness(HarnessError)` / `Internal{message}`），不对应 00 §0.3 的 `HarnessErrorKind` 八值——**取消与实例超时永远不是 Harness 错误**（否则实例会被误标 `Crashed`，与 00 的 run `Cancelled` 语义冲突）。节点执行器出口统一 `RunFailure::into_node_error()` 映射为 `NodeError::Cancelled` / `NodeError::Timeout{elapsed_ms}` / `NodeError::Harness` / `NodeError::Internal`（`crates/zgraph-harness/src/agent_loop.rs`）。

### 5.5.5 v1.1 硬伤修正对照（审计 P-05/P-06/P-09、C-19、M-17 相关）

| v1.1 写法（不可编译/不可实现） | 问题 | v1.2 修正 |
|---|---|---|
| `self.llm_provider.chat(model: ..., messages: &messages, tools: ...)` 后 `self.stream_tokens(ctx, &response)` | Rust 无命名参数；`chat()` 返回完整响应后再"逐 token"根本不存在流；阻塞接口与流式矛盾 | 改为 `chat_stream(req, &mut sink, ctx: &dyn HarnessCtx)`：**流式是唯一接口**，sink 在接收增量时 **1:1 转发 emit**（不聚合）；返回值只是"最终聚合结果"（§5.3.1、§5.8.3） |
| `match response { LlmResponse::ToolCall { tool_name, args } => ... }` | 三态枚举无法表达"文本 + 多 tool_calls + usage"；单数 ToolCall 丢信息 | `LlmResponse` 改为 struct（content/tool_calls/stop_reason/usage），多 tool_call 为 `Vec`（§5.3.2/§5.5.3） |
| `let agent = ...; ... agent.id ... ; skills.find(tool_name)` 中先 move 后借用；`args` 同 | use-after-move | 循环内对 `call` 用 `&` / `clone`（`call_id`、`name` 各 clone 一次）；响应先解构再使用（§5.5.4） |
| `ctx.emit(...)` 且 `ctx: &RunContext` | `RunContext` 不暴露 emit；违反 H1 | 签名改为 `ctx: &dyn HarnessCtx`（生产唯一适配 `NodeContext`，D15）；`emit` 为其同步非阻塞方法（04 分册） |
| 只在 tool 分支 `iteration += 1` | iteration 语义错（文本轮不计）且上限判定漏 | 请求前 `+1`，文本/tool 轮统一（§5.5.2） |
| `LlmResponse::HumanRequest` → `NeedHumanInput` | v1 不做人工介入，且与"Err → Crashed"冲突（C-12） | v1 **不产生** `NeedHumanInput`；模型若在文本中请求人工，只是普通 `content`（§5.10.1） |
| 上下文溢出"归档后清空继续" | 无界循环风险（S-15） | 只允许丢短期旧轮；仍溢出 → `HARNESS_CONTEXT_OVERFLOW`（Crashed）（§5.4.5） |
| `memory.append_turn(..., Assistant, ...)` 只写 assistant | 历史缺 user 侧（S-14） | v1 Memory stub：**不写不读**；接口在 v1.1 冻结为 `append_turn(role, ...)`（§5.10.1） |

---

## 5.6 工具注册表与 ToolDef（关闭 M-03；v2 改为 worker 子进程）

**一句话结论**：工具是**Python 侧声明 + Rust 侧注册表**（发现结果来自 worker 子进程并被 Rust 缓存）；LLM 只能看到 `AgentConfig.tools` 列出的工具；**每次工具调用 = 一个 Python worker 子进程**（D3/D13），进程内**不 import / 不执行**用户工具代码；工具执行失败一律 `TOOL_ERROR` 喂回 LLM，绝不终止；调用未注册工具才是 `HARNESS_UNKNOWN_TOOL`（终止）。

### 5.6.1 ToolDef 字段（Rust 侧内存模型，冻结）

```rust
pub struct ToolDef {
    pub name: String,            // ^[a-z][a-z0-9_]{0,63}$；全局唯一（跨文件）
    pub description: String,     // 1..=1024 字符；作为 LLM tool 描述
    pub parameters: Value,       // JSON Schema（见下方子集约束）
    pub timeout_ms: u64,         // 默认 DEFAULT_TOOL_TIMEOUT_MS = 30000；可由工具声明的可选 timeout_ms 覆盖（1..=600000；生效值见 §5.6.5）
    pub source: String,          // Python 模块全名，如 "tools.builtin"；分发（worker call 的 module）与诊断用
}
pub struct ToolSchema { pub name: String, pub description: String, pub parameters: Value }
```

| `parameters` 子集约束（v1） | 规定 |
|---|---|
| 顶层 | 必须是对象，且 `"type": "object"`；`"properties"` 必须为对象（可为空 `{}`） |
| 允许的关键字 | `type`（`object`/`string`/`number`/`integer`/`boolean`/`array`/`null`）、`properties`、`required`、`items`、`enum`、`description`、`default`、`minimum`/`maximum`、`minLength`/`maxLength`、`additionalProperties`（仅 bool）；M4 实现额外放行只读注解关键字 `title`/`format`/`examples`/`const`（不影响校验语义） |
| 不允许 | `$ref`、`oneOf`/`anyOf`/`allOf`、`patternProperties`、`if/then/else` → `HARNESS_CONFIG_LOAD{reason:"unsupported_schema_keyword"}` |
| 校验深度 | **结构**校验递归（`properties`/`items`，深度上限 32；未知/禁止关键字任一层即 `unsupported_schema_keyword`）；**参数值**校验只做顶层 `required` 与 `properties` 类型（§5.6.4），不引入 JSON Schema 引擎 |

### 5.6.2 Python 侧声明（两种形式，MUST 支持其一）

**形式 A（批量模块，推荐）**——`tools/builtin.py`：

```python
# tools/builtin.py
TOOLS = [
    {"name": "now", "description": "当前时间（ISO8601，UTC）",
     "parameters": {"type": "object", "properties": {}, "required": []}},
    {"name": "http_get", "description": "对 URL 发起 GET，返回 {status, body}",
     "parameters": {"type": "object",
                    "properties": {"url": {"type": "string", "maxLength": 2048},
                                   "timeout_ms": {"type": "integer", "minimum": 100, "maximum": 10000}},
                    "required": ["url"], "additionalProperties": false}},
]

def call(name: str, args: dict) -> dict:
    """按 name 分发；返回 JSON 可序列化的 dict。异常 → TOOL_ERROR。"""
    ...
```

**工具声明可含可选 `timeout_ms`**（integer，值域 `1..=600000`；缺省 `DEFAULT_TOOL_TIMEOUT_MS = 30000`）：用于该工具单次调用的超时（e2e 超时用例依赖此字段，如声明 `100` 验证超时）；非法值 → `invalid_declaration` / `invalid_tool_timeout`。声明存在时，`discover` 的输出原样携带 `timeout_ms`（缺省不输出）。**生效值** = `min(声明值, DEFAULT_TOOL_TIMEOUT_MS, 实例 deadline 剩余)`（§5.6.5）——即声明只能**缩短**超时，>30000 的值会被裁剪到 30000。

**形式 B（单工具模块）**——`tools/now.py`：

```python
# tools/now.py
TOOL = {"name": "now", "description": "当前时间（ISO8601，UTC）",
        "parameters": {"type": "object", "properties": {}, "required": []}}

def call(args: dict) -> dict:
    ...
```

**注册表发现规则（v2：由 worker 子进程执行，一次）**：

| 项 | 规定 |
|---|---|
| 目录 | CLI `--tools-dir <dir>` > env `ZGRAPH_TOOLS` > `<assets-dir>/tools`（02 §2.6.3 的资产根） |
| 扫描 | 递归 `*.py`，跳过 `_` 开头的文件/目录与 `__pycache__` |
| 模块名 | 相对 tools_dir 的点分名（`tools.builtin` / `tools.now`）；导入失败 → 加载期 `HARNESS_CONFIG_LOAD{reason:"tool_module_import"}` + traceback |
| 形态判定 | 模块有 `TOOLS` → 形式 A（必须有 `call(name, args)`）；有 `TOOL` → 形式 B（必须有 `call(args)`）；两者都有或都没有 → `HARNESS_CONFIG_LOAD` |
| 重名 | 跨模块同名 → `HARNESS_CONFIG_LOAD{reason:"duplicate_tool",sources:[...]}` |
| 引用名解析 | `AgentConfig.tools` 项含 `.` 时先查全限定名（`source.name`）；否则按 `name` 查；全限定名 source 前缀不匹配时按短名兜底（name 全局唯一，无歧义） |
| 发现时机 | **首次为某 `agent_call` 实例解析 `AgentConfig.tools` 时**通过 worker 子进程执行一次 `discover`；结果在 **Harness 进程内按 `(tools_dir)` 缓存**（D13：工具发现只做一次），后续实例复用 |
| 调用时 | 每次工具调用 spawn **新的** worker 子进程 + 新解释器：**无 `sys.modules` 复用、无工具模块跨调用缓存**（D3/D4）；调用失败 → 该实例 `Crashed`（加载期错误）或 `TOOL_ERROR`（调用期，见 §5.6.6） |

#### 5.6.2.1 `tool_runner` worker 协议（M4 实况，D3/D13）

Harness 不 import 用户工具代码；工具协议由固定的 worker 入口模块 **`zgraph_worker.tool_runner`**（`python/zgraph_worker/tool_runner.py`）承担，经 `zgraph-worker` 的 `WorkerExecutor` 以协议 `run` 帧调用（`schemas/worker-protocol-v1.md` §2）。两个函数签名均为 worker 统一契约 `(inputs, ctx) -> dict`，`ctx` 不使用。

| 阶段 | `run.module` / `run.function` | `inputs`（Harness → tool_runner，全部 JSON object） | 成功返回（`result.output`） | 失败返回（**仍是 exit 0 + result 帧**） |
|---|---|---|---|---|
| 发现 | `zgraph_worker.tool_runner` / `discover` | `{"tools_dir": "<abs dir>"}` | `{"ok":true,"tools":[{"name","description","parameters","source","timeout_ms"?}]}`；顺序 = 文件序（相对路径排序）+ 声明序 | `{"ok":false,"error":{"reason","message","traceback"?}}` |
| 调用 | `zgraph_worker.tool_runner` / `call` | `{"tools_dir":"<abs dir>","module":"<source 模块全名>","name":"<tool>","args":<object>}` | `{"ok":true,"result":<JSON object>}` | `{"ok":false,"error":{"reason","type","message","traceback"?}}` |

**`discover` 失败 reason**：工具侧 payload——`tools_dir_missing`（缺失/非目录）、`tool_module_import`（模块导入失败，附 traceback）、`duplicate_tool`（跨模块同名）、`invalid_declaration`（TOOLS/TOOL 同时或都缺、`call` 不可调用、name/description/parameters 非法、非法 `timeout_ms`）；Harness 侧 live 失败——`tool_module_timeout`（discover 超时）、`tool_discover_cancelled`、`tool_runner_spawn`（spawn 失败）、`tool_worker_crash`（退出码异常）、`invalid_tool_timeout`（`timeout_ms` 超值域）。全部 → `HARNESS_CONFIG_LOAD`。

**`call` 失败 reason**：工具侧 payload——`result_not_dict`（返回非 dict）、`exception`（用户异常/输入/导入/未知工具等；含 `type`/`message`/≤8 KiB `traceback`）；Harness 前置校验——`args_not_object`、`missing_required`、`type_mismatch`（§5.6.4，**不 spawn**）；结果规整——`result_not_object`（非 object，§5.6.5）；worker 层——`timeout`（含 `timeout_ms`）、`cancelled`（loop 再映射 `NodeError::Cancelled`）、`runner_spawn`、`worker_protocol`、`worker_crash`。

> **关键语义**：`discover`/`call` 的失败**不**走 worker `error` 帧、不使用非约定退出码（进程正常 exit 0），失败信息统一在 `result.output.ok=false` 中回传——避免把"工具业务失败"误判为 worker 协议/崩溃错误。Harness 侧映射：
> - `discover` 失败 → 加载期 `HARNESS_CONFIG_LOAD{path:"<tools_dir>", reason:"<discover reason>"}`（实例 `Crashed`；§5.2.4 步 8）；
> - `call` 失败 → `TOOL_ERROR{reason:"<call reason>", type?, message, traceback?}` 喂回 LLM；
> - 子进程层失败（超时/协议/崩溃，非上述 payload）→ `TOOL_ERROR{reason:"timeout|worker_protocol|worker_crash"}`（§5.6.5）。

**统一封装**：Harness 侧 `ToolRunner` 负责拼装上述 `inputs`、spawn worker、解析 `ok`、生成 `ToolDef`（`source` 取自 discover 的 `source`；`timeout_ms` 取 discover 输出中的声明值，缺省 `DEFAULT_TOOL_TIMEOUT_MS = 30000`）；发现结果按 `tools_dir` 在 Harness 进程内缓存一次。

### 5.6.3 注册 -> 请求 tools 字段 / `AgentConfig.tools` 解析（v2）

- `AgentConfig.tools = ["now", "http_get"]` → 解析为 discover 结果中的 `ToolDef`，`ToolRegistry::schemas()` 返回**仅这两个**的 `ToolSchema`，顺序与 `tools` 数组一致（保证请求字节可复现）。
- 解析失败（名字不在 discover 结果 / 不匹配工具名规则 / 超 32 个）→ `HARNESS_CONFIG_LOAD{reason:"tool_not_registered", json_path:"$.tools[i]", actual:"<引用名>"}`（**不是** `HARNESS_UNKNOWN_TOOL`；后者只用于运行期 LLM 幻觉调用）。
- `tools` 为空 → `LlmRequest.tools = []`，adapter MUST 在请求体中**省略** `tools` 字段（部分 provider 对空数组报错）。

### 5.6.4 参数校验（调用前，Harness 侧）

| 检查 | 失败处理 |
|---|---|
| `args` 是 JSON object | `TOOL_ERROR{reason:"args_not_object"}` 喂回 LLM（**不 spawn** worker） |
| `required` 中每个 key 存在 | `TOOL_ERROR{reason:"missing_required",keys:[...]}` |
| `properties` 中已声明 key 的类型匹配（string/number/integer/boolean/object/array） | `TOOL_ERROR{reason:"type_mismatch",key,expected,actual}` |
| 未声明 key | v1 **允许**（宽松；模型可能多传），不报错 |
| 深层/枚举/长度约束 | 交给 Python 工具自行校验（工具抛异常 → `TOOL_ERROR`） |

> 校验失败**不调用 Python**（不 spawn 子进程），直接把错误 JSON 作为 `Tool` 消息喂回，让 LLM 修正参数——这是 tool loop 的正常路径，不算失败。工具侧的 `tool_runner` 仍做同名最小校验（双保险，签名不同不得改变本表 reason 码）。

### 5.6.5 执行、超时与结果处理（v2：worker 子进程）

| 项 | v2 规定 |
|---|---|
| 执行位置 | **每次工具调用 = 一个 worker 子进程**（`module=zgraph_worker.tool_runner`、`function=call`）；Harness 只 async 等待帧/退出，**不阻塞事件循环、不做进程内 Python**（D3/D13；H4） |
| 并发 | 单实例内**串行**（§5.5.3）；跨实例**工具调用**受 Harness 进程内**全局工具信号量**约束（容量 = `default_max_workers()` = CPU×2，D4/00 §0.5 `MAX_WORKERS`）；**不占** `WorkerExecutor` 的每 run worker 许可（`MAX_WORKERS_PER_RUN`）——工具调用不携带 run 级执行上下文（D13 落地细节，见 §5.13）；不再使用 CPU=1/IO=4 解释器信号量 |
| 超时 | 生效值 = `min(ToolDef.timeout_ms, DEFAULT_TOOL_TIMEOUT_MS=30000, 实例 deadline 剩余)` → `run.deadline_ms`（声明可缩短超时，>30000 被裁剪）。**Harness 路径**：外层 `tokio::time::timeout` 到点 → 丢弃 `run_worker_controlled` future → `Supervisor::drop` **立即 `SIGKILL` 进程组**（`crates/zgraph-worker/src/supervisor.rs:182-186`，无残留）；`cancel`→2s→`SIGTERM`→1s→`SIGKILL` 仅是 supervisor **显式 cancel 信号**的优雅升级链（M3 协议 §4），M4 Harness 的 timeout/drop 路径**不保证走完**（e2e 实测整例 ~0.41s ≪ 3s）。本次调用返回 `TOOL_ERROR{reason:"timeout",timeout_ms}` 喂回 LLM（**不终止**实例） |
| 工具子进程失败 | 子进程层：超时 → `timeout`；取消 → `cancelled`；spawn 失败 → `runner_spawn`；协议错误 → `worker_protocol`；异常退出 → `worker_crash`；`call` 返回的 `ok=false` 载荷 → `exception` / `result_not_dict`；均由 `ToolFailure` 归一为 `TOOL_ERROR` 喂回 LLM；**不**映射 `NodeError::NodeWorker`（工具失败是 LLM 可修正路径） |
| 返回值 | 必须可 JSON 序列化为 object（`dict`）；非 dict → `TOOL_ERROR{reason:"result_not_dict",actual_type}` |
| 序列化 | `serde_json::to_string`（紧凑）；≤ 8 KiB（`DEFAULT_TOOL_RESULT_MAX_BYTES`，00 §0.5） |
| 截断 | 超 8 KiB → **结构感知**截断：ASCII/CJK 字符串在 JSON **字符串值内部**按字符边界截断并追加 `…[truncated N bytes]`；对象/数组按合法 JSON 降级（保留前缀 + 唯一标记）。**截断后输出必定是合法 JSON 且总长 ≤ 8 KiB**；原始结果 v1 不落盘，仅 trace 中 `result_summary` 记录长度。**ISSUE-074 fixed（REV-027）**，取代 M4 的纯文本截断限制（§5.13.3 L4） |
| 异常 | Python 异常 → `TOOL_ERROR{reason:"exception", type, message, traceback}`；`traceback` 单独按 **≤8 KiB** 截断（不切 UTF-8 码点）；子进程已退出，无解释器残留（D4） |
| 结果摘要 | `ToolCallEnd.result_summary: Option<Value>`：成功放 `{"ok":true,"keys":[...],"bytes":N,"preview":"≤256 字符"}`；失败放 `{"ok":false,"error":{...}}`；`preview`/`last_content_summary` **严格上界 ≤256 / ≤512 字符（含省略号）**；无法生成摘要（如 worker 崩溃无输出）→ `None`（ISSUE-074 fixed，REV-027） |
| 无进程内缓存 | 工具模块**不得**依赖跨调用缓存（每次新解释器）；发现结果只缓存在 Rust 侧（一次） |

### 5.6.6 工具错误语义（关闭 M-03/C-12 的一半）

| 事件 | 终止 run？ | 喂回 LLM？ | trace |
|---|---|---|---|
| 参数校验失败 | ❌ | ✅ | `ToolCallEnd{ok:false}`（可丢） |
| 工具抛异常 | ❌ | ✅ | 同上 |
| 工具超时（kill 进程组） | ❌ | ✅ | 同上 + `TOOL_ERROR` 摘要 |
| 工具子进程协议错误/崩溃 | ❌ | ✅ | 同上 |
| 返回非 object | ❌ | ✅ | 同上 |
| 工具名未注册 / 已注册但不在 `AgentConfig.tools`（未授权，REV-020 C2） | ✅ `HARNESS_UNKNOWN_TOOL` → `Crashed`（在 `ToolCallStart` emit **之前**失败，故**不产生**任何 tool 生命周期事件）；`detail.available_tools` 只列**可见集** | ❌ | `instances.error` + 迁移 `Crashed` |
| `AgentConfig.tools` 引用了未注册工具 / discover 失败 | ✅ 加载期 `HARNESS_CONFIG_LOAD` | ❌ | 不产生 trace（加载失败） |

> **未知/未授权工具不产生工具生命周期事件（REV-020 C2 / REV-022 #2）**：执行前查找失败即 `HARNESS_UNKNOWN_TOOL`，**在 emit `ToolCallStart` 之前**返回，因此 trace 中不存在该调用的 `tool_call_start`/`tool_call_end`。

> **错误码统一（D13/ISSUE-070）**：工具失败**唯一**错误码是 `TOOL_ERROR`（00 §0.4 Harness 域）；**不得**出现 `HARNESS_TOOL_ERROR`。`TOOL_ERROR` 的载体是**工具结果消息**，不是 `NodeError`：它永远不进入实例错误列。`HarnessErrorKind::ToolError` 只用于内部结构化传递与 `ToolCallEnd` 摘要（00 §0.4 已规定其"继续"语义）。

## 5.7 错误映射表（关闭审计 M-17 / C-12；对齐 00 §0.4）

**一句话结论**：`HarnessErrorKind` → 错误码 → `NodeError::Harness{kind,detail}` → 实例状态/run 状态 → trace 级别，一一对应。v1.2 冻结期要求「不新增枚举值」；**M4 唯一例外是追加 `ContextOverflow`（D18，Lead 裁定方案 A）**，其余值不变（八值：`ConfigLoad`/`UnknownTool`/`MaxIterations`/`LlmError`/`LlmTimeout`/`ToolError`/`MemoryDegraded`/`ContextOverflow`）。

### 5.7.1 `HarnessError` 形态

```rust
pub struct HarnessError {
    pub kind: HarnessErrorKind,      // 00 §0.3 的八值（ContextOverflow 为 M4 追加，D18）
    pub code: &'static str,          // 00 §0.4 的错误码（见下表）
    pub message: String,             // 人类可读，含关键定位
    pub detail: Value,               // 结构化（§5.7.4）
}
```

`HarnessError` 只在 Harness 内部流动；节点执行器出口统一转成 `NodeError::Harness { kind, detail }`（`detail` 内嵌 `code`/`message` 便于 CLI 展示）。

### 5.7.2 总映射表（权威）

| `HarnessErrorKind` | 错误码 | `NodeError` 变体 | `InstanceStatus` | run 影响 | trace 级别 | 是否携带数据 | `detail` 关键内容 |
|---|---|---|---|---|---|---|---|
| `ConfigLoad` | `HARNESS_CONFIG_LOAD` | `Harness{kind:ConfigLoad,detail}` | `Crashed` | `Failed`（I9 冷峻） | `error` | ✅ 文件/字段/期望/实际 | `§5.2.7` |
| `UnknownTool` | `HARNESS_UNKNOWN_TOOL` | `Harness{kind:UnknownTool,detail}` | `Crashed` | `Failed` | `error` | ✅ 工具名 + **可见**工具列表 | `{tool, iteration, available_tools[≤32]}`（`available_tools` = `AgentConfig.tools` 解析后的可见集） |
| `MaxIterations` | `HARNESS_MAX_ITERATIONS` | `Harness{kind:MaxIterations,detail}` | `Crashed` | `Failed` | `error` | ✅ **当前上下文摘要**（00 要求） | `{iterations, max_iterations, last_content_summary(≤512 字符), pending_tool_calls, usage}` |
| `LlmError` | `HARNESS_LLM_ERROR` | `Harness{kind:LlmError,detail}` | `Crashed` | `Failed` | `error` | ✅ provider 诊断 | `{provider, model, kind, phase, status, attempts, streamed_tokens}` |
| `LlmTimeout` | `HARNESS_LLM_TIMEOUT` | `Harness{kind:LlmTimeout,detail}` | `Crashed` | `Failed` | `error` | ✅ 超时值 | `{timeout_ms, attempts, phase, iteration}` |
| `ToolError` | `TOOL_ERROR` | **不产生**（走工具结果消息） | 实例继续 | 无 | `warn`（`ToolCallEnd{ok:false}`） | ✅ 错误 JSON | `{reason, tool, timeout_ms?}`（`reason` 取值域见 §5.6.2.1） |
| `MemoryDegraded` | `HARNESS_MEMORY_DEGRADED` | **不产生**（降级继续） | 实例继续 | 无 | `warn` | ✅ 操作名 | `{op}` |
| `ContextOverflow`（M4 追加，D18） | `HARNESS_CONTEXT_OVERFLOW` | `Harness{kind:ContextOverflow,detail}` | `Crashed` | `Failed`（I9 冷峻） | `error` | ✅ 分段估算 | `{budget, required, segments, first_over}`（§5.7.4） |

**补充映射（非 HarnessErrorKind，但同一出口）**：

| 情形 | `NodeError` | 错误码（run/CLI） | `InstanceStatus` | trace 级别 | 说明 |
|---|---|---|---|---|---|
| 取消（LLM/工具执行中） | `Cancelled` | `RUN_CANCELLED`（若 run 级取消） | `Cancelled` | `info` | 取消**不**映射为 Harness 错误（C-12 冲突的解法） |
| 实例超时（调度器或 deadline 自检） | `Timeout{elapsed_ms}` | `NODE_TIMEOUT` | `TimedOut` | `error` | v2 到点**kill 进程组**（D1）；`killed_workers += 1`；**不产生** `NodeAbandoned` |
| ~~被放弃任务记账~~ | 无 | ~~`NODE_ABANDONED`~~ | — | — | **v2 不产生（D1）**：超时即 kill，无"放弃在飞任务"路径；字段/事件仅保留兼容 |
| 消息序列非法 / 不变量违反 | `Internal{message}` | `RUN_ILLEGAL_TRANSITION` 或 `Internal` | `Crashed` | `error` | fail-fast（框架 bug） |

**工具错误码唯一性（M4，ISSUE-070）**：工具失败**只能**是 `TOOL_ERROR`（00 §0.4 Harness 域）；`HarnessErrorKind::ToolError.code()` 必须返回 `"TOOL_ERROR"`，**不得**出现 `HARNESS_TOOL_ERROR`（kernel 微修分支已修正）。`TOOL_ERROR` 不产生 `NodeError`。

**Streaming 摘要类型（M4）**：`StreamingEvent::ToolCallEnd.result_summary` 的类型是 **`Option<Value>`**（00 §0.3 已回写）：正常成功/失败给 `Some(摘要对象)`；无法生成摘要时 `None`（序列化为 `null`）。

**明确否决（C-12 裁定）**：

| 被否决的写法 | 原因 | v1 替代 |
|---|---|---|
| `NeedHumanInput` → `WaitingHuman` | v1 不做人工介入；且与「`Err` → `Crashed`」冲突 | v1 **不产生**该错误/状态；枚举与码位保留到 v1.1 |
| `Err(HarnessError)` 有时 Crashed、有时等待人工 | 无统一映射表导致执行器无法编码 | 本表唯一出口；`Err` 一律 `Crashed`（除取消/超时两条特殊路径） |
| 工具失败也 `Err` → Crashed | 违背 00 §0.4 的 `TOOL_ERROR`「继续」 | 工具失败只写 tool 消息（§5.6.6） |
| 重试耗尽但返回部分输出 + warning（v1.1 §3.4） | 与 00 `HARNESS_MAX_ITERATIONS` → Crashed 冲突 | 一律 `Crashed`，上下文摘要进 `detail`（00 优先） |

### 5.7.3 "取消/超时"为什么不在 HarnessErrorKind 里

- 00 §0.3 的八值（含 M4 追加的 `ContextOverflow`）**不含取消与实例超时**；它们分别由 `NodeError::Cancelled` / `NodeError::Timeout` 表达，源头是**调度器与节点执行器**，不是 Harness 的 LLM/tool 故障。
- 若把"取消"塞进 `LlmError`，实例会被误判 `Crashed` → run `Failed`，与用户取消语义冲突（run 应为 `Cancelled`）。
- 因此 §5.7.2 的两条补充映射是**必需的**，且优先级最高：`cancel_requested` / 实例 deadline 的检查**先于**任何 provider 错误分类。

### 5.7.4 `detail` 统一结构（跨分册可解析）

| 键 | 类型 | 出现于 | 说明 |
|---|---|---|---|
| `code` | string | 全部 | 00 §0.4 错误码 |
| `stage` | `load`\|`assemble`\|`invoke`\|`tool`\|`return` | 全部 | Harness 阶段 |
| `iteration` | int | invoke/tool/return | 当时轮次 |
| `file` / `json_path` / `expected` / `actual` | string/any | ConfigLoad | 定位 |
| `iterations` / `max_iterations` | int | MaxIterations | — |
| `last_content_summary` | string | MaxIterations | ≤512 字符（按字符边界截断，附 `…`） |
| `pending_tool_calls` | array | MaxIterations | 最后一个响应的 tool_call 名称列表 |
| `provider` / `model` / `phase` / `status` / `attempts` / `streamed_tokens` | mixed | LlmError/LlmTimeout | 诊断 |
| `budget` / `required` / `segments` / `first_over` | int/object | ContextOverflow | `segments` 见下；`first_over` 为首个超限分段名 |
| `usage` | object | MaxIterations/LlmError | 已累计 usage |

`ContextOverflow.detail.segments` 固定给出分段估算：

```json
{ "code": "HARNESS_CONTEXT_OVERFLOW", "stage": "assemble",
  "budget": 50380, "required": 61200,
  "segments": { "system": 1800, "constraints": 300, "tools_schema": 900,
                "user_input": 1200, "tool_results": 56000, "short_term": 0 },
  "first_over": "tool_results" }
```

### 5.7.5 落库分工（与 06 分册的接口）

| 数据 | 落库位置 |
|---|---|
| `NodeError::Harness{kind,detail}` | `instances.error`（JSON）；`error` 内的 `code` 同时冗余到 `instances.error_code`（便于 SQL 检索） |
| 实例迁移到 `Crashed` | `transitions` 行（`from_status='Running'`，`to_status='Crashed'`，`cause='harness_error'`，`severity='error'`） |
| LLM/tool 过程诊断 | 仅 `Streaming` 事件（可丢）+ 进程日志（`RUST_LOG`）；不产生其他 TraceEvent |
| 取消/超时 | 对应 `transitions` 行 + `runs.status`/`runs.error_code`；实例超时另产 `TraceEvent::NodeTimedOut`（调度器产生、不可丢，06 §6.2）；`NodeAbandoned` **v2 不产生**（D1，enum 保留兼容） |

---

## 5.8 流式事件映射与聚合（关闭 M-04 的流式形态；对齐 00 §0.3/§0.6 I6）

**一句话结论**：Harness **逐 delta 1:1 emit**（不在 Harness 内聚合）；`NodeContext.emit` 把每个 `StreamingEvent` 包成 `TraceEvent::Streaming{run_id, instance_id, event}`（04 §4.2）走 `try_send` 可丢通道；**100ms 窗口聚合的唯一执行点是 06 写线程**（落库前合并，06 §6.5），避免双窗口二次聚合。

> **M4 实况**：`StreamSink`（`HarnessCtx` 适配）的 `on_delta` 是**纯转发**——`LlmDelta::Token/Reasoning` 立即 `ctx.emit(TokenDelta/ReasoningDelta)`，无本地缓冲、无定时器；fake provider 的 `chunk_chars` 只决定 delta 粒度，**不改变段边界**（段边界由 06 写线程的 100ms/4096 字符规则决定）。

### 5.8.1 映射表（权威，1:1）

| LLM 侧事件 | `LlmDelta` | 产生的 `StreamingEvent`（00 §0.3） | 传输 | 06 落库 |
|---|---|---|---|---|
| 正文增量 | `Token { text }` | `TokenDelta{text}` | `ctx.emit` → 可丢 | 06 写线程按 100ms 合并为 `streaming_events(kind='token')` 分段 |
| 推理增量 | `Reasoning { text }` | `ReasoningDelta{text}` | 同上 | `kind='reasoning'` 分段（与正文各自独立窗口） |
| tool call 出现（响应解析完成） | `ToolCallStarted` | `ToolCallStart{call_id, tool_name, args_summary}` | 同上 | `kind='tool_call_start'` |
| 工具执行完成 | — | `ToolCallEnd{call_id, tool_name, ok, result_summary}` | 同上 | `kind='tool_call_end'` |
| 轮次推进 | — | `Progress{percent, message}` | 同上 | `kind='progress'` |
| usage / stop_reason | — | **不产生 StreamingEvent**（随节点 output，§5.9） | — | `instances.output_json` |
| Harness 错误 | — | **不产生 StreamingEvent**（§5.7.5） | — | `instances.error` |

**不变量（单测 MUST 断言）**：

| # | 不变量 |
|---|---|
| A1 | 顺序：同一响应的 `TokenDelta` 顺序 == provider 增量顺序；`ToolCallStart` 在对应工具执行**之前** emit |
| A2 | 不丢文本：`concat(所有 emit 的 TokenDelta.text) == response.content`（在无丢弃前提下；丢弃由 `streaming_dropped` 计数） |
| A3 | `call_id` 一致：`ToolCallStart` 与 `ToolCallEnd` 用同一 id；M4 实现**直接透传 genai 给出的 `call_id`**（不再合成 `local_{iteration}_{index}`；provider 缺失 id 时由 genai 层兜底） |
| A4 | Harness 内**不得**做时间窗聚合、不得本地缓冲后批量 emit（否则与 06 §6.5 形成双窗口，段边界不可复现）；`on_delta` 必须 1:1 立即 emit |
| A5 | `Progress` 每个 `Invoking` 入口恰好一条（不是每个 token） |
| A6 | **未知/未授权工具在 emit `ToolCallStart` 之前失败**（`HARNESS_UNKNOWN_TOOL`）→ 该调用**不产生** `ToolCallStart`/`ToolCallEnd`（REV-020 C2 / 05 §5.6.6） |

### 5.8.2 Progress 语义

| 项 | 规定 |
|---|---|
| 触发 | 每个 `Invoking` 入口（即每次 LLM 请求前） |
| `percent` | `min(99, floor(iteration × 100 / max_iterations))`（`u8`）；`Returning` 前不补发 100（避免与输出混淆） |
| `message` | 固定 `"llm call {iteration}/{max_iterations}"`（便于 CLI 与测试断言） |

### 5.8.3 聚合语义（执行点 = 06 写线程，单一聚合点）

| 项 | 规定 |
|---|---|
| 执行点 | **只在 06 写线程**（`TraceEvent::Streaming` 到达后按 `(instance_id, variant)` 缓冲，批量提交边界 flush） |
| 窗口 | `100ms`（= `TRACE_BATCH_INTERVAL_MS`，00 §0.5）；缓冲超过 `STREAM_SEGMENT_MAX_CHARS=4096` 时提前分段 |
| 合并结果 | 一段的 `text` = 窗口内按到达顺序拼接；`delta_count` = 合并的原始 delta 数；`first_at`/`last_at` 记录窗口真实跨度 |
| flush 时机 | ① 批量提交边界（≤100ms）；② 缓冲达到 4096 字符；③ 同实例出现非 delta 流式事件（如 `ToolCallStart`）；④ 实例进入终态；⑤ 通道关闭/进程退出 |
| 段边界可复现性 | 同一 fake-llm 脚本 + 同一 `chunk_chars` 下，段边界必须确定（AC-08 回归） |
| 存储列 | 见 06 §6.2.9（`segment_index` / `delta_count` / `text` / `truncated`） |

> 05 只定义"聚合语义与不变量"；**实现参数放在 06 §6.5**（`STREAM_TOKEN_AGGREGATE_MS` / `STREAM_SEGMENT_MAX_CHARS` / `STREAM_SEGMENT_MAX_BYTES`），由 06 统一持有与落库。

### 5.8.4 可丢语义（I6 的落地）

| 规则 | 规定 |
|---|---|
| 通道 | `NodeContext.emit` 内部 `try_send` 到容量 `STREAMING_CHANNEL_CAP=1024` 的流式通道（00 §0.5）；不可丢事件走另一条无损通道（06 §6.4） |
| 丢什么 | **全部 `StreamingEvent` 均可丢**（00 §0.6 I6） |
| 不丢什么 | 00 §0.3 的 `RunStarted` / `Transition` / `GateResult` / `NodeTimedOut` / `RunFinished` —— 全部由调度器产生、走无损通道（`send().await`）；`NodeAbandoned` **v2 不产生**（D1，保留在不可丢清单仅为兼容旧 trace） |
| 丢弃计数 | **单一计数** `RunMetrics.streaming_dropped`（00 §0.3；04 §4.2 的 run 级原子计数器；随 `RunFinished.metrics` 落库，06 §6.2.6） |
| 行为约束 | 丢弃不得阻塞、不得 panic、不得影响 loop 控制流；丢弃后后续 delta 照常 emit |
| 已知风险 | `ToolCallStart/End` 频率低（≤ iteration × tool 数）但同属可丢类；若被丢，诊断依赖 `instances.output_json` 与进程日志。若需提升为不可丢，须修订 00 §0.6 I6（v1.1 候选） |

### 5.8.5 `args_summary` / `result_summary` 规则

| 字段 | 规则 |
|---|---|
| `ToolCallStart.args_summary` | 参数对象的**结构摘要**：`{"keys":[...], "bytes":N, "preview":"≤256 字符"}`；`preview` 是参数**紧凑 JSON 前缀**（≤256 字符，M4 不做脱敏，见 §5.13.3 L5）。**MUST NOT** 记录 `api_key`、Authorization header、prompt 全文 |
| `ToolCallEnd.result_summary` | **`Option<Value>`**（D13/00 §0.3）：成功 `Some({"ok":true,"keys":[...],"bytes":N,"preview":"≤256 字符"})`；失败 `Some({"ok":false,"error":{...≤1 KiB}})`；无法生成摘要时 `None`（落库为 `null`） |
| 截断 | 一律按字符边界（不切 UTF-8 码点中间）；附 `…`。**严格上界**：`preview` ≤256 字符、`last_content_summary` ≤512 字符（省略号计入上限；ISSUE-074 fixed，REV-027） |
| 敏感信息 | v1 不做脱敏（本地工具）；**MUST NOT** 记录 `api_key`、Authorization header、prompt 全文 |

---

## 5.9 输出契约与 `AgentProvider`（关闭 C-19）

**一句话结论**：`agent_call` 节点的 output **固定四字段** `{content, stop_reason, iterations, usage}`，整包广播给下游（04 §4.5）；业务字段由下游节点从 `content` 自行解析；v1 不做 `output_mapping`。

> **M4 实况**：`HarnessExecutor`（`crates/zgraph-harness/src/executor.rs`）返回的 `Value` 即下文字段形状（`AgentOutput` 序列化）；实现与本节逐字段一致（AC-08 以 fake provider 断言 `content/stop_reason/iterations/usage` 四字段）。

### 5.9.1 `AgentOutput`

```rust
pub struct AgentOutput {
    pub content: String,        // 最终 assistant 文本；无内容时为 ""（不会为 null）
    pub stop_reason: StopReason,
    pub iterations: u32,        // §5.5.2 的 LLM 请求轮次
    pub usage: Usage,
}
```

| 规则 | 规定 |
|---|---|
| `content` | 非 null（`null`/`None` 归一为 `""`）；已 flush 全部聚合窗口后才返回 |
| `stop_reason` 序列化 | `"EndTurn"` \| `"ToolCalls"` \| `"MaxTokens"` \| `"ContentFilter"` \| `"Other:<raw>"`（见 §5.9.2） |
| `stop_reason = MaxTokens` | **正常返回**（模型长度停止不是框架故障），`content` 可能被截断；trace 不额外记 warning（日志 info） |
| 到 `max_iterations` | 不返回 output → `Crashed`（00 §0.4） |
| `usage.source` | `provider` / `estimated` / `unavailable`；默认 `provider` |

### 5.9.2 节点 output 的 JSON 形状（v1 冻结，下游按此取值）

`agent_call` 节点 `output`（整包广播到每条出边）：

```json
{
  "content": "{\"intent\":\"refund\",\"confidence\":0.92}",
  "stop_reason": "EndTurn",
  "iterations": 2,
  "usage": { "prompt_tokens": 1240, "completion_tokens": 48, "total_tokens": 1288,
             "reasoning_tokens": null, "source": "provider" }
}
```

| 字段 | 类型 | 说明 |
|---|---|---|
| `content` | string（可为空串） | **唯一业务载体**；建议 prompt 约定输出 JSON 文本 |
| `stop_reason` | string | 枚举字面量（`Other` 时形如 `"Other:length"`） |
| `iterations` | integer | ≥1 |
| `usage` | object | 四个数值字段 + `source`；`prompt_tokens`/`completion_tokens`/`total_tokens` 不可为 null（未知填 0 + `source=unavailable`），**`reasoning_tokens` 允许 `null`**（provider 未提供时；M4 实现即如此） |

> **不做**：`output_mapping`、`output_schema` 强校验、`content` 自动解析成 `data` 字段（均为 v1.1 保留，见 §5.10.2）。

### 5.9.3 业务字段映射约定（input_mapping / 下游取值）

| 方向 | 约定 | 示例 |
|---|---|---|
| 入参（`input_mapping`） | `{"<节点输入端口>": "<agent 输入字段>"}`；未列出的端口按**同名**映射（identity） | `"input_mapping": {"message": "user_message"}` → 节点端口 `message` 的值成为 Agent 输入字段 `user_message` |
| 映射优先级（M4 裁定） | **节点级** `AgentCallConfig.input_mapping`（非空）> `AgentConfig.input_mapping` > identity（端口名）；节点级 key 不在该节点声明端口集合内 → **丢弃该 key**（不失败，V15 降级），未列出的声明端口按 identity 补齐 | 见 `assemble::effective_input_mapping`（`crates/zgraph-harness/src/assemble.rs`） |
| 下游取字段 | 下游 Python 节点从整包 output 取 `inputs["<端口>"]["content"]`，再按需 `json.loads` | `raw = inputs["parsed"]["content"]; intent = json.loads(raw)["intent"]` |
| 下游 gate 取字段 | 同理读 `output["content"]`（gate 只读 output，00 §0.6 I7） | — |
| 空映射 | `input_mapping` 缺失 → identity：所有节点输入按端口名成为 Agent 输入字段 | — |
| 端口为 null | 字段省略（§5.4.2） | — |

**Python 侧完整示例**（下游消费 `agent_call` 输出）：

```python
# nodes/route.py
def run(inputs: dict, ctx) -> dict:
    out = inputs["parsed"]                 # agent_call 节点的整包 output
    payload = json.loads(out["content"])   # 业务约定：Agent 输出 JSON 文本
    return {"intent": payload["intent"], "confidence": payload["confidence"]}
```

### 5.9.4 `AgentProvider` trait（v1 仅 BuiltinHarness）

```rust
/// Agent 执行的后端抽象（Harness 本身即一种实现）。
/// v2 候选：CodexAppServerProvider（子进程 JSON-RPC stdio）/ AcpProvider（DSH 等 ACP agent）。
#[async_trait::async_trait]
pub trait AgentProvider: Send + Sync {
    fn kind(&self) -> AgentProviderKind;      // Builtin | CodexAppServer | Acp（后两者 v1 不实现）
    async fn run(&self, agent: &AgentConfig, inputs: Value, ctx: &NodeContext)
        -> Result<AgentOutput, HarnessError>;
}
```

| 实现 | v1 | 说明 |
|---|---|---|
| `BuiltinHarness` | ✅ 唯一实现 | v2 落地 = **`HarnessExecutor`**（`crates/zgraph-harness/src/executor.rs`，直接实现 `NodeExecutor` 并处理 `agent_call`）；`AgentProvider` 的 `run(agent, inputs, ctx)` 语义即其内部 `run_agent`（§5.5.4）。**M4 未引入独立的 `AgentProvider` trait 对象**，扩展位保留 |
| `CodexAppServerProvider` | ❌（v2 候选） | 外挂子进程 + JSON-RPC；`dynamicTools` 仍 experimental；提供的是 turn 循环（与我们 loop 冲突，需"仅用其 transport"再评估） |
| `AcpProvider` | ❌（v2 候选） | ACP 只上线语义事件、拿不到 token delta（`_research/harness-alternatives.md` §3.3），与 §5.8 冲突，需要降级策略 |
| `FakeProvider`（测试） | ✅ | 仅用于 `LlmProvider` 层（§5.3.4），不是 `AgentProvider` |

> v1 选择器：`agent_call` 节点固定 `BuiltinHarness`；`AgentProviderKind` 的其余两值在配置中出现 → `HARNESS_CONFIG_LOAD{reason:"provider_not_implemented"}`。
>
> **M4 实现**：`HarnessExecutor`（`crates/zgraph-harness/src/executor.rs`）只处理 `CompiledHandler::Agent`；其它 handler 交给本执行器 → `NodeError::Internal`（内核路由错误）。构造面为 `HarnessSettings { assets_dir, tools_dir, provider, fake_mode, tool_runner }`（e2e/M5 冻结面）；`NodeContext` 以 `Arc<dyn HarnessCtx>` 适配（`impl HarnessCtx for NodeContext`），每次执行懒加载 AgentConfig、工具注册表按 `tools_dir` 进程内缓存一次。

---

## 5.10 v1 明确不做与保留字（对齐决策记录 §2.2）

### 5.10.1 不做清单（每项给出 v1 的可观测行为）

| 能力 | v1 行为 | 接口/码位 | v1.1 计划 |
|---|---|---|---|
| 短期/长期记忆 | `EmptyMemoryStore`：`recall` 返回空、`append_turn` 空操作；**不产生** `HARNESS_MEMORY_DEGRADED` | `MemoryStore` trait 冻结（`recall(scope, query, limit)` / `append_turn(role, content)` / `write_long_term(record)`） | run 级短期记忆（内存）+ 用户轮次写入（S-14） |
| KB 检索 | `EmptyKnowledgeAdapter`：`retrieve` 返回空；system 中"知识库参考"段整段省略 | `KnowledgeAdapter` trait 冻结（`retrieve(kb_id, query, top_k) -> Vec<KbDocument>`，`KbDocument.rendered`） | 真实 KB + 引用溯源 |
| 人工介入 | `NeedHumanInput` **不产生**；`WaitingHuman` 保留不产生（00 §0.3） | 枚举保留 | v1.1 全链路 |
| 多模态 | 消息只支持文本（`LlmMessage` 无 image 片段） | — | v2 |
| MCP 工具 | 不引入 `rmcp`；工具只来自 Python 注册表 | `ToolDef` 注册表可适配（v1.1 可选 feature） | `rmcp` client + child-process transport |
| 工具并行执行 | 单响应多 tool_call 串行（§5.5.3） | — | 并行 + 完成顺序回填 |
| provider 多路由 / fallback | 单 provider 单模型 | — | 路由与降级 |
| prompt 模板渲染 | `{{var}}` 原样保留，不做替换 | — | 模板引擎（需安全评审） |
| 输出 JSON Schema 强校验 | 不做；业务字段由下游解析 | — | `output_schema` + 校验失败重试 |
| 预算校准 / 成本计费 | 只记录 provider usage | — | usage 回填校准、成本表 |
| 断点续跑 | 不做；进程崩溃后 run 标定见 06 §6.8 | — | 事件溯源恢复 |

### 5.10.2 保留字段 / 保留字（出现即 `HARNESS_CONFIG_LOAD`）

| 保留键 | 位置 | v1 行为 | 计划 |
|---|---|---|---|
| `output_mapping` | AgentConfig 顶层 | 出现 → `HARNESS_CONFIG_LOAD{reason:"reserved_field"}` | v2：业务字段提升 |
| `output_schema` | 顶层 | 同上 | v1.1 |
| `memory` | 顶层 | 同上 | v1.1 |
| `kb_ids` | 顶层 | 同上 | v1.1 |
| `tools_concurrency` | 顶层 | 同上 | v2 |
| `provider_fallback` | 顶层 | 同上 | v2 |
| `$schema` | 顶层 | **允许并忽略**（编辑器提示用） | — |

> 未知字段默认拒绝（`deny_unknown_fields`）；上表是"已知但 v1 不实现"的白名单，其余未知字段同样报 `unknown_field`。

### 5.10.3 冻结的扩展位（v1.1 可直接实现，不需改本分册）

| 扩展位 | 冻结内容 |
|---|---|
| `MemoryStore` / `KnowledgeAdapter` | trait 签名与调用点（§5.4.2 的 ③④⑤ 段）已固定 |
| `AgentProvider` | trait 已固定（§5.9.4）；v1 只有 `BuiltinHarness` |
| `LlmStreamSink` | 新事件类型（如 `usage_delta`）可加，不破坏 sink 调用点 |
| 聚合参数 | 常量集中在 06 §6.5，可配置化 |

---

## 5.11 与 00 总纲的对齐检查（M4 复核）

**一句话结论**：本分册**未**发现与 00 枚举/错误码/不变式的语义冲突（已按 00 v1.2 当前版本对齐，含 `TraceEvent` 七类、`RunMetrics.killed_workers` 与 00 §0.5 常量表）；v1.2 冻结期登记的 **7 条常量缺口已由 00 §0.5 补录（M4 逐项核对一致）**，G1/G3/G7/G8/G9 已闭合，G5/G6 为已裁定/无冲突项。

### 5.11.1 本分册引入的常量（**已由 00 §0.5 补录**，M4 核对一致）

| 常量 | 值 | 出处 | 00 §0.5 状态 |
|---|---|---|---|
| `DEFAULT_LLM_MAX_TOKENS` | `2048` | §5.2.2 | ✅ 已补录（`2_048`） |
| `DEFAULT_LLM_TEMPERATURE` | `0.2` | §5.2.2 | ✅ 已补录 |
| `DEFAULT_CONTEXT_WINDOW_TOKENS` | `65536` | §5.2.2 | ✅ 已补录（`65_536`） |
| `CONTEXT_BUDGET_SAFETY_RATIO` | `0.8` | §5.4.4 | ✅ 已补录 |
| `DEFAULT_TOOL_RESULT_MAX_BYTES` | `8192` | §5.6.5 | ✅ 已补录（`8_192`；本分册旧稿写作 `TOOL_RESULT_MAX_BYTES`，以 00 名称为准） |
| `RETRY_AFTER_MAX_MS` | `30000` | §5.3.5 | ✅ 已补录（`30_000`） |
| `BACKOFF_BASE_MS` | `1000` | §5.3.5 | ✅ 已补录（`1_000`） |

> 流式聚合相关常量（`STREAM_TOKEN_AGGREGATE_MS` / `STREAM_SEGMENT_MAX_CHARS` / `STREAM_SEGMENT_MAX_BYTES`）由 **06 §6.5** 持有（聚合执行点在写线程），本分册不重复定义，避免双份常量漂移；00 §0.5 已列。

### 5.11.2 与 00 的措辞/覆盖差异（M4 复核状态）

| # | 项 | 00 现状 | 05 处理 | 状态 |
|---|---|---|---|---|
| G1 | 重试次数语义 | §0.5 `DEFAULT_LLM_MAX_RETRIES=1` + 退避基数 `BACKOFF_BASE_MS=1000`（第 2 档 2000ms 默认不触发） | 解释为"**1 次重试**，等待 1s；2s 属第 2 档" | ✅ **已闭合**（00 §0.5 措辞与 §5.3.5 一致） |
| G2 | Token 聚合窗口 | §0.5 有 `STREAM_TOKEN_AGGREGATE_MS=100` | 05 只定义语义（1:1 emit），参数与执行点归 06 §6.5 | ✅ 已闭合（00/06 已补录） |
| G3 | `NodeError::Config` vs `Harness{kind:ConfigLoad}` | §0.3 两者都存在 | Harness 内配置失败一律 `NodeError::Harness{kind:ConfigLoad}`；`NodeError::Config` 留给**非 Harness** 资产加载 | ✅ 已闭合（04 §4.1.4 + 实现） |
| G4 | `HARNESS_CONFIG_LOAD` vs `DSL_AGENT_NOT_FOUND` 重叠 | 两个码都能表达"Agent 资产缺失/非法" | 按阶段分工：图期只判**资产索引命中**（`DSL_*`），运行期判合法性（`HARNESS_*`） | ✅ 已闭合（02 §2.6.3） |
| G5 | tool 生命周期事件可丢 | I6 明确 6 类不可丢，`Streaming` 全可丢 | 接受 `ToolCallStart/End` 可丢（单一 `streaming_dropped` 计数）；风险保留 | ⚠ 已裁定（v1.1 若要提升需修订 I6） |
| G6 | `Usage`/`stop_reason` 的落点 | 00 `TraceEvent`/`StreamingEvent` 均无 usage 变体 | 只随节点 `output_json`（§5.9.2），不新增事件 | ✅ 无冲突 |
| G7 | trace 通道拓扑 | §0.5 有独立 `STREAMING_CHANNEL_CAP=1024`；03/06 有过单通道描述 | 06 采用双通道（无损 8192 + 流式 1024） | ⚠ 跟踪（03/06 已协商，M4 不改） |
| G8 | `input_mapping` 键值方向 | 决策记录示例两可 | 05 采用 02 方向（key=端口名，value=Agent 字段名） | ✅ 已闭合 |
| G9 | Agent 资产文件名 | 决策记录示例 `agents/message_parser.json` | 05 采用 02 口径（文件名无关，按文件内 id 索引） | ✅ 已闭合 |
| G10 | 工具错误码 | 00 §0.4 Harness 域为 `TOOL_ERROR` | **唯一**工具错误码 = `TOOL_ERROR`；禁止 `HARNESS_TOOL_ERROR`（ISSUE-070） | ✅ M4 已统一（kernel 微修） |
| G11 | `ToolCallEnd.result_summary` 类型 | 00 §0.3 原为 `Value` | 改为 **`Option<Value>`**（与内核/实现一致） | ✅ M4 已回写 00 §0.3 |
| G12 | `HarnessErrorKind` 值数 | 00 §0.3 冻结七值、05 §5.7 声明"不新增枚举值" | M4 追加第 8 值 **`ContextOverflow`**（Lead 裁定方案 A，D18）；`HARNESS_CONTEXT_OVERFLOW` 不再借用 `ConfigLoad` 承载 | ✅ 已裁定并回写 00/05 |

### 5.11.3 未与 00 冲突但需 06 配合的点

| 点 | 05 要求 | 06 落点 |
|---|---|---|
| 丢弃计数 | **单一** run 级计数（`RunMetrics.streaming_dropped`） | `runs.streaming_dropped`（06 §6.2.4） |
| 错误落库 | `instances.error` 带 `code` | `instances.error_code` 列（06 §6.2.5） |
| Streaming 聚合分段 | Harness 1:1 emit；**写线程**聚合为段（100ms / 4096 字符） | `streaming_events` 行（06 §6.5） |
| Harness 内部状态 | 不落库 | trace DB 无 Harness 状态行（06 §6.3） |
| 取消/超时 | 不产生 Harness 错误；由调度器产 `Transition` + `NodeTimedOut`（D1：kill 进程组，无 `NodeAbandoned`） | `events.kind` 含 `NodeTimedOut`（06 §6.2.6） |
| 工具调用诊断 | `ToolCallStart/End` 走可丢 Streaming | `streaming_events`（`tool_call_start`/`tool_call_end`） |

---

## 5.12 审计条目关闭对照（供 08 复核）

| 审计条目 | 本分册关闭位置 | 关闭方式 |
|---|---|---|
| M-01 AgentConfig 全字段缺失 | §5.2 | 字段表 + 示例 + 加载流程 + V1–V16 |
| M-02 Memory Manager 未定义 | §5.10.1 | `MemoryStore` trait 冻结 + Empty 实现（v1 不写不读） |
| M-03 Skill/ToolDef/ToolExecutor | §5.6 | `ToolDef` 字段 + 两种 Python 声明 + 超时/错误语义 |
| M-04 LLM Provider 与流式接口 | §5.3 | `LlmProvider` trait（流式唯一接口）+ genai 映射 + fake + 重试 |
| M-05 KB 适配器与上下文关系 | §5.10.1 / §5.4.2③ | `KnowledgeAdapter` trait 冻结 + v1 空实现（段整段省略） |
| M-17 HarnessError → NodeError → Status | §5.7.2 | 权威映射表 + 取消/超时补充路径 |
| C-12 NeedHumanInput 与 Err→Crashed 冲突 | §5.7.2 / §5.7.3 / §5.10.1 | v1 不产生 `NeedHumanInput`；`Err` 一律 Crashed（除取消/超时） |
| C-19 Agent 输出形状 | §5.9 | 固定四字段 + 下游取值示例 + `output_mapping` 保留 |
| S-14 记忆作用域与用户轮次 | §5.5.5 / §5.10.1 | v1 stub；`append_turn(role, ...)` 接口在 v1.1 冻结 |
| S-15 上下文预算溢出 | §5.4 | 总预算公式 + 裁剪算法 + 禁止"归档后继续" |
| S-16 多 tool_call 与 iteration | §5.5.2 / §5.5.3 | iteration=LLM 轮次；多调用串行只 +1 |
| P-05/P-06/P-09 伪代码不可编译 | §5.5.4 / §5.5.5 | 流式接口 + 解构 + struct 构造 + `&NodeContext` |

> 注：上表为 v1.2 冻结期的审计关闭记录（保留原样）；M4 落地后 §5.1/§5.3/§5.5/§5.6/§5.7 的 v2 修订见 §5.13。

---

## 5.13 M4 v2 落地偏差记录（D13–D21）

**一句话结论**：M4 实现（`crates/zgraph-harness` + `python/zgraph_worker/tool_runner.py`）在 v1.2 冻结语义之上产生 9 条新偏差（D13–D21）；D13 是 [`doc/architecture-v2.md`](../architecture-v2.md) §8 **D3** 的详细落地点，D14 是 D2 在 Harness 内的延伸，D15–D21 为本分册新增。**偏离项一律以本节 + 正文为准**，偏差表保留为决策记录。

| # | 05 原语义（v1.2 冻结） | M4 v2 落地 | 代码模块 | 测试 |
|---|---|---|---|---|
| **D13** | 工具执行在 Harness 进程内：`spawn_blocking` 调用 Python、工具模块**一次导入并进程内缓存**、工具与 Python 节点共享 CPU=1/IO=4 信号量（§5.1.2/§5.6.5） | **每次工具调用 spawn 一个 worker 子进程**（`module=zgraph_worker.tool_runner`、`function=call`）；工具模块**无跨调用缓存**；发现（`discover`）一次并在 Rust 侧缓存；超时 = Harness 外层 timeout 丢弃 worker future → `Supervisor::drop` **立即 `SIGKILL` 进程组**（`supervisor.rs:182-186`；优雅链 `cancel`→2s→SIGTERM→1s→SIGKILL 仅 supervisor 显式 cancel 路径，M4 timeout/drop 不保证走完）+ `TOOL_ERROR` 喂回 LLM；并发受 Harness 进程内**全局工具信号量**（CPU×2 = `MAX_WORKERS`）约束，**不占**每 run `MAX_WORKERS_PER_RUN` 许可（D4；Lead 已裁定接受，理由：工具调用不是节点实例、`HarnessCtx` 无 run_id）。工具协议载荷：`ToolCallRequest{call_id, tool_name, module, tools_dir, args, timeout_ms}` 由 Harness 组装；worker `run` 载荷的 `run_id`/`node_id`/`trace_id` 由 `instance_id` 合成，`global_vars={}`（Python `tool_runner` 不使用） | `crates/zgraph-harness/src/tools.rs`；`python/zgraph_worker/tool_runner.py`；复用 `crates/zgraph-worker`（`run_worker_controlled` + supervisor 进程管理/进程组 kill） | `crates/zgraph-harness/src/tools.rs` 内联单测（`#[cfg(test)]`：参数校验/截断/摘要/FakeToolRunner）；`tests/e2e/harness_ac08.rs`（真 worker 工具成功/异常/超时）；`python/tests/test_tool_runner.py` |
| **D14** | 取消通过 `CancelToken`（包装 `tokio-util::CancellationToken`）传给 `LlmProvider`（§5.3.1/§5.3.2/§5.3.7） | **不存在 tokio `CancellationToken`**、`NodeContext` 无 `cancel_token()`；Harness 自有 **`CancelToken` = 对 `HarnessCtx` 的只读轮询包装**（`CANCEL_POLL_INTERVAL_MS=50`，`is_cancelled()`）；取消来源 = run 级 `Arc<AtomicBool>` 快照；命中 → `LlmError{kind:Cancelled}` → `NodeError::Cancelled` | `crates/zgraph-harness/src/ctx.rs`（`HarnessCtx`/`CancelToken`）、`agent_loop.rs`、`llm/*.rs`；`crates/zgraph-kernel/src/context.rs`（只读标志） | `crates/zgraph-harness/src/{ctx,agent_loop,llm/*}.rs` 内联单测（取消覆盖 establish/streaming/tool 三个阶段、50ms 轮询）；`tests/e2e/harness_ac08.rs`；04 §4.2.2 的 `cancel_token` 不存在断言 |
| **D15** | Harness 直接依赖 `&NodeContext`（§5.1.1 H1、§5.5.4） | 引入 **`HarnessCtx`** 最小只读投影作为测试 seam（7 方法，见 §5.3.1.1）；**生产唯一实现** = `impl HarnessCtx for NodeContext`（无独立包装类型）；fake provider 与 loop 单测用它驱动取消/截止时间 | `crates/zgraph-harness/src/ctx.rs`（`HarnessCtx`/`CancelToken`/`StreamSink`）；`crates/zgraph-harness/src/executor.rs` | `crates/zgraph-harness/src/{ctx,executor}.rs` 内联单测（内存 `HarnessCtx` 注入取消/deadline）；`tests/e2e/harness_ac08.rs`（经真内核 `start_run` 端到端）；04 §4.2 对齐：能力集是 `NodeContext` 公开只读面的子集 |
| **D16** | 重试编排在 **provider 内部**（§5.3.5："本次调用生效值…adapter 按 §5.3.5 处理"；§5.5.4 注释"重试在 provider 内部"） | 重试编排在 **Harness 的 `Invoking`**（`invoke_with_retry`）：`LlmProvider::chat_stream` 只做**单次尝试**；`attempts` 由 Harness 填写；provider 不得内置重试（genai 0.7.2 亦不自带重试）；重试不改变状态机、不增加 iteration。**单次 LLM 超时由 Harness `tokio::time::timeout` 兜底**（adapter 不判超时；phase 由「sink 已转发增量数」判定，§5.3.6） | `crates/zgraph-harness/src/agent_loop.rs`（重试/退避）；`crates/zgraph-harness/src/llm/*.rs`（单次调用 + `LlmError` 分类） | `crates/zgraph-harness/src/agent_loop.rs` 内联单测：Establish 可重试 / Streaming 立即失败 / retry-after / deadline 冲突不重试 / attempts 计数；`tests/e2e/harness_ac08.rs` 覆盖 fake 两轮 tool loop |
| **D17** | 空响应在 §5.5.3 表（连续 2 次才报错）与 §5.5.4 旧伪代码（单次即 `empty_response`）之间**自相矛盾** | **采信 §5.5.3 表格**：单次空响应追加 `Assistant{content: None, tool_calls: []}` 空 assistant 消息后继续下一轮；**连续 2 次**空响应才报 `HARNESS_LLM_ERROR{reason:"empty_response"}`；§5.5.4 旧"单次即错"写法作废（伪代码已改写） | `crates/zgraph-harness/src/agent_loop.rs`（`consecutive_empty` 计数） | `crates/zgraph-harness/src/agent_loop.rs` 内联单测：单次空响应继续（下一轮成功返回）/ 连续 2 次 → `HARNESS_LLM_ERROR` |
| **D18** | `HarnessErrorKind` 冻结七值、`HARNESS_CONTEXT_OVERFLOW` 无对应 kind（05 §5.4.5 却写 `HarnessError{kind: ContextOverflow}`）；实现曾用 `ConfigLoad` 临时承载 | **追加第 8 值 `HarnessErrorKind::ContextOverflow`**（只增不改，Lead 裁定方案 A；kernel 提交 `ce491de` 提供 `CODE_HARNESS_CONTEXT_OVERFLOW`）；`context_overflow()` 映射该 kind，`detail.code` 仍为权威 `HARNESS_CONTEXT_OVERFLOW` | `crates/zgraph-kernel/src/types.rs`（`HarnessErrorKind::ContextOverflow`）；`crates/zgraph-harness/src/error.rs`（`context_overflow()`） | kernel `node_error_codes_match_spec`（含第 8 值）；harness `error.rs` 内联单测（`context_overflow_keeps_authoritative_code_in_detail`，kind 为 `ContextOverflow`） |
| **D19** | `LlmProvider::name()` 冻结名为 `"genai-deepseek"` / `"genai-openai-compatible"`（§5.3.1） | 实况 **`name()` = `"genai"`**：单一 `GenaiLlmProvider` 按 `ModelRef.provider`（`Deepseek`/`OpenAiCompatible`）用 `ServiceTarget` 服务两类端点；`FakeLlmProvider::name()` = `"fake"`；`name()` 只用于诊断/错误 detail，不参与路由 | `crates/zgraph-harness/src/llm/transport.rs`（`GenaiLlmProvider`）、`llm/fake.rs` | `crates/zgraph-harness/src/llm/{transport,fake}.rs` 内联单测；e2e fake 场景断言 provider 名不参与行为 |
| **D20** | `input_mapping` 只在 `AgentConfig` 定义（§5.2.2/§5.9.3），未明确节点级覆盖与非法 key 的降级 | **优先级：节点级 `AgentCallConfig.input_mapping`（非空）> `AgentConfig.input_mapping` > identity**；节点级 key 不在该节点声明端口集合内 → **丢弃该 key**（V15 降级，不失败）；未列出的声明端口按 identity 补齐（§5.9.3） | `crates/zgraph-harness/src/assemble.rs`（`effective_input_mapping`） | `crates/zgraph-harness/src/assemble.rs` 内联单测（优先级/丢弃/补齐） |
| **D21** | 未记录依赖层全局副作用；v1.2 假定 `serde_json::Value` 为默认 `BTreeMap` 键序/尺寸 | **`genai 0.7.2` 无条件启用 `serde_json/preserve_order`**：进入工作区依赖图后所有 crate 的 `serde_json::Map` 变为 `IndexMap`（键序=插入序、内存增大、`Value` 变大）→ 触发 `zgraph-worker` 的 `clippy::result_large_err`（REV-020 #1/#5）。`graph_hash` 不受影响（`zgraph-dsl` 显式排序键）；trace/CLI JSON 键序行为改变属**跨模块语义变化**，登记为偏差 | 无代码落点（依赖层）；影响 `Cargo.lock`、`crates/zgraph-dsl/src/hash.rs`（已验证安全）、`crates/zgraph-worker`（clippy 回归已由 worker 提交 `94e1f8a` 装箱 `WorkerFailure::Worker` 消除，根因登记 ISSUE-073） | 评审证据：REV-020 §3 #5 / §4、REV-021；`cargo test --workspace` 全绿；`graph_hash` 排序测试；干净 target `cargo clippy --workspace --all-targets -- -D warnings` exit 0 |

### 5.13.1 与 architecture-v2 §8 偏差表的映射

| 本节 | architecture §8 | 关系 |
|---|---|---|
| D13 | D3（工具 = worker 子进程） | 本节为 D3 的**实现级细化**（payload 契约、缓存策略、错误映射、并发/超时落点） |
| D14 | D2（`cancel_token` 不存在） | D2 覆盖 Python SDK 代理；本节明确 **Rust Harness 内部**也无取消令牌，改为轮询 |
| D15 | 无 | M4 新增（测试 seam，不改变外部语义） |
| D16 | 无 | M4 新增（重试编排位置；v1.2 正文原已倾向"provider 内"，现统一为 Harness） |
| D17 | 无 | M4 新增（正文内部矛盾裁定） |
| D18 | 无 | M4 新增（**枚举值例外**：只追加 `HarnessErrorKind::ContextOverflow`，Lead 裁定方案 A；00 §0.3/§0.4、05 §5.7/§5.4.5 已同步） |
| D19 | 无 | M4 新增（实现实况：`name()` 统一 `"genai"`） |
| D20 | 无 | M4 新增（实现实况：节点级 input_mapping 覆盖与降级规则） |
| D21 | 无 | M4 新增（依赖层全局副作用：genai→serde_json/preserve_order；Lead 收尾登记 ISSUE，长期评估更换 transport 或接受） |

### 5.13.2 未随本节变更的既有语义

- iteration 计数（§5.5.2）、单响应多 tool_call 串行（§5.5.3）、`AgentOutput` 四字段（§5.9）、错误映射**既有 7 个** `HarnessErrorKind` 与补充路径（§5.7；第 8 值 `ContextOverflow` 见 D18）、1:1 emit 与 06 写线程聚合（§5.8）、`--fake-llm` 脚本语义（§5.3.4）均**不变**。
- `ContextOverflow` 的表达：**M4 追加 `HarnessErrorKind::ContextOverflow`（D18）**，`HarnessError.code` 仍为权威 `HARNESS_CONTEXT_OVERFLOW`（见 §5.7.1）。

### 5.13.3 已知限制与建议台账（Lead 收尾登记 ISSUE；REV-020 C3 要求）

| # | 限制 | 影响 | 建议 / ISSUE 归属 |
|---|---|---|---|
| L1 | **已修**：M4 时 `AgentConfig.node_timeout_ms` 未生效；M5 由 kernel `declared_timeout_ms` + `NodeExecutor::timeout_hint`（REV-026）、harness `timeout_hint`/`peek_agent_node_timeout`（REV-033）、CLI 组合执行器转发（REV-030）打通，优先级见 §5.2.6 | 无（端到端 e2e 由 REV-032 覆盖） | **ISSUE-076 fixed**（M5，REV-026/033/030/032）；口径见 §5.2.6 |
| L2 | prompt/constraints 路径安全为**词法归一化**（拒绝 `..`），未 `canonicalize` | 符号链接可绕过资产根白名单（本地资产场景风险低） | M5 加 `canonicalize` 后重新校验 + 测试；**ISSUE-077**（REV-020 #13） |
| L3 | `futures-util` 为 harness 直接依赖（genai `ChatStream` 取流所需），未提升 workspace；genai 默认 features 带入重依赖 | 依赖治理分散、`Cargo.lock` 增大 | process 统一提升 `[workspace.dependencies]` 并评估 `default-features`；**ISSUE-078**（REV-020 #12） |
| L4 | **已修（REV-027）**：工具结果改**结构感知截断**（输出必定合法 JSON 且 ≤8 KiB）；`traceback` 单独 ≤8 KiB；`preview`/`last_content_summary` 严格上界 ≤256/≤512（含省略号） | 无（§5.6.5/§5.8.5 已同步） | **ISSUE-074 fixed**（REV-027，commit `9137fd6`） |
| L5 | `args_summary.preview` 为参数紧凑 JSON 前缀（≤256 字符），**未脱敏**；短参数可能完整出现（测试已改名，语义未改） | 敏感参数可能进 trace | 二选一：预览改为 `key:type`/`***` 脱敏，或产品接受并明确文档语义；**ISSUE-075**（REV-020 #3） |
| L6 | `genai` 引入 `serde_json/preserve_order` 全局 feature（D21）：所有 crate 的 `Value` 变大、JSON 键序改变；曾触发 `zgraph-worker` `clippy::result_large_err` | CI clippy 回归（已由 worker 提交 `94e1f8a` 装箱 `WorkerFailure::Worker` 缓解，非根因） | 长期评估更换 transport 或接受；根因**ISSUE-073**（REV-020 #1/#5、REV-021） |
| L7 | 工具并发 = **独立**进程级 `Semaphore(CPU×2)`，未与 `WorkerExecutor` 全局 `max_workers` 共享 | 工具子进程 + Python 节点子进程可各自达到 CPU×2，超出 D4 全局上限语义 | M5 统一为共享全局上限；**ISSUE-079**（REV-020 #7） |
| L8 | 5 个生产函数 >60 行（`run_agent` 139、`parse_model` 124、`parse_discover_output` 75、`parse_call` 66、`map_webc_error` 64），违反 architecture §2.2 等价约束 | 维护成本；纯重构不阻塞 | **部分修（REV-027）**：5 个目标函数已拆分（`run_agent` 21 / `parse_model` 31 / `parse_discover_output` 30 / `parse_call` 29 / `map_webc_error` 59，另 `invoke_with_retry`→50）；**剩余既有 >60 行随 M6**：`parse_agent_config` 113、`validate_schema_node` 105、`transport::chat_stream` 86、`fake::chat_stream` 75、`executor::execute` 71→78、`parse_base_url` 64、`execute_tool` 63、cli `parse` 79 / `run_in_runtime` 70、新增 74 行测试；**ISSUE-081**（REV-020 #11） |
| L9 | discover/call 失败 `reason` 为**扩展超集**（§5.2.7 已文档化），超出 v1.2 闭集 | 跨模块 reason 表需同步（py-worker REV-019 §7.3） | 保留诊断价值，文档化即可；**ISSUE-080**（REV-020 #9） |
| L10 | `llm_calls` / `worker_versions_json` 等 L2 记录表已在 06/architecture 定义，M4 未接线（Harness 未写 `llm_calls`） | L2（recorded outputs）重放依赖这些记录，M4 只能做 L1 | **M5 部分修**：Harness `LlmCallRecorder` seam（REV-027）+ CLI `llm_calls` 落库（REV-030，仅 `--record=full`）+ L2 replay（REV-030/032）已落地；`runs.worker_versions_json/model_params_json` 仍未写入（ISSUE-082 保持部分修） |

> 以上 **ISSUE-073..082** 由 Lead 在 M4 收尾（REV-024）统一开号登记（编号以 `doc/issues.md` 最终为准）。上述限制不影响 M4 验收结论（AC-08a 必过 / AC-08b 手动）；偏差表 D13–D21 只记录**语义级**落地口径，L1–L10 属已知限制。
>
> **M5 状态（REV-035）**：L1 已修（REV-026/033/030 + e2e REV-032）；L4 已修（REV-027）；L8 的 5 个目标函数已拆（REV-027），剩余清单随 M6；L10 的 seam（REV-027）与 `llm_calls` 落库/L2 replay（REV-030/032）已交付，版本列仍 NULL（ISSUE-082 部分修）；L2/L3/L5/L6/L7/L9 与 ISSUE-075/077/079/080 留 M6。M6 排除项与依据见 `doc/decisions/DR-002-m5-cli-replay-recording.md` §7。
