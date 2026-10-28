# 图模型驱动工作流框架 · 规格冻结 v1.2 · 分册 05：Agent Harness

> **状态**：规格冻结（编码前置）。本分册定义 **Agent 执行壳**的字段级契约：AgentConfig、`LlmProvider`（genai transport）、上下文组装与 token 预算、Harness 状态机与 tool loop、工具注册表、错误映射、流式事件、输出契约、`AgentProvider` 扩展点。
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
| H4 | 同步 Python 调用一律离开事件循环线程（`spawn_blocking`），Harness 内部不得在事件循环线程上做 Python 调用 | 00 §0.6 I8；工具与 prompt 渲染均走该规则 |
| H5 | `AgentConfig` / prompt / 工具注册表均为**文件资产**，运行期只读；v1 不做热更新与资产写入 | 决策记录 §2.2 |
| H6 | 错误必须收敛到 00 §0.3 的 `HarnessErrorKind` 七值 + `NodeError` 七变体，不得新增枚举值 | 00 §0.3/§0.4；映射见 §5.7 |

### 5.1.2 组件分解

```
harness/                       llm/                         tools/
├── config.rs   AgentConfig 加载、校验、prompt 解析      ├── transport.rs  genai Client（端点/鉴权注入）
├── context.rs  system/约束/记忆/KB/用户输入 → messages    ├── stream.rs    Chunk → LlmDelta；tool_call 聚合
├── loop.rs     §5.5 状态机 + iteration + 停止条件         ├── retry.rs     错误分类 + 退避 + retry-after
├── budget.rs   §5.4 token 估算与裁剪                      └── fake.rs      FakeLlmProvider（测试/AC-08）
├── tool.rs     工具调用编排（spawn_blocking + 超时）
├── stream.rs   LlmDelta → StreamingEvent（100ms 聚合）
└── error.rs    §5.7 HarnessError → NodeError 映射
```

### 5.1.3 选型结论（不可再变）

| 层 | 方案 | 依据 | 变更成本 |
|---|---|---|---|
| LLM transport（HTTP/SSE/tool_call delta 聚合/usage） | **`genai` 0.7.2**，MIT OR Apache-2.0 | `_research/harness-alternatives.md` §5；原生 deepseek adapter + 自定义 OpenAI-compatible | 被自有 `LlmProvider` trait 隔离；换 async-openai / 自写客户端只需替换 adapter（决策记录 §3.4） |
| agent loop / 上下文 / 工具执行 / 重试编排 / 错误映射 | **自研**（~1.2–1.5k LOC） | 决策记录 D2 | — |
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
| 执行期加载 | 每次 `agent_call` 实例执行时**懒加载 + 全量校验**；解析/校验失败 → `HARNESS_CONFIG_LOAD`（实例 `Crashed`） |
| 缓存 | v1 **不缓存**（单文件解析 < 1 ms，避免热更新歧义）；v1.1 可按 `(path, mtime)` 缓存 |
| 路径安全 | `role_prompt_file` / `constraints_file` 相对**AgentConfig 文件所在目录**解析；允许绝对路径；解析后必须落在资产根或 CWD 之内，逃逸（`..`）→ `HARNESS_CONFIG_LOAD` |
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
| `tools` | string[] | ❌ | `[]` | 长度 ≤ 32；每项匹配工具名规则（§5.6.1）；**必须能在注册表中解析**，否则 `HARNESS_CONFIG_LOAD`（不是 `HARNESS_UNKNOWN_TOOL`） | 本 Agent 可见工具（LLM 只能调用这些） |
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
| 9 | 读取 `model.api_key_env` 指向的环境变量（非空） | 缺失/空 → `HARNESS_CONFIG_LOAD{field:"model.api_key_env",env:"..."}`；**`--fake-llm` 模式下跳过此步** |
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
| V15 | `input_mapping` 的 key（端口名）/value（Agent 字段名）均非空；无重复 key；key 在该节点声明端口集合内（否则按"空映射"降级并记 warning，不失败） | `key` / `value` |
| V16 | 所有被引用文件在路径白名单内（不逃逸资产根/CWD） | `path` / `reason:"path_escape"` |

### 5.2.6 生效优先级（关闭 M-01 的"默认值说不清"）

| 参数 | 优先级（高 → 低） | 常量 |
|---|---|---|
| 实例超时 | DSL 节点 `config.timeout_ms` > `AgentConfig.node_timeout_ms` > `DEFAULT_NODE_TIMEOUT_MS` | 00 §0.5 |
| 单次 LLM 超时 | `AgentConfig.llm_timeout_ms` > `DEFAULT_LLM_TIMEOUT_MS` | 00 §0.5 |
| LLM 重试次数 | `AgentConfig.llm_max_retries` > `DEFAULT_LLM_MAX_RETRIES` | 00 §0.5 |
| 迭代上限 | `AgentConfig.max_iterations` > `DEFAULT_MAX_ITERATIONS` | 00 §0.5 |
| 工具超时 | `ToolDef.timeout_ms` > `DEFAULT_TOOL_TIMEOUT_MS` | 00 §0.5 |

> 节点级 `max_iterations`（DSL 可选字段）覆盖 AgentConfig 的规则由 02/04 分册定义；本分册只定义"被解析出的最终有效值"如何被 loop 使用。

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

---

## 5.3 `LlmProvider` 与适配器（关闭审计 M-04）

**一句话结论**：Harness 只依赖**自有** `LlmProvider` trait（流式 + 回调 + 最终聚合结果），`genai` 的全部类型都封在 adapter 内；`chat()` 阻塞式全量接口**不存在**——这是修正 v1.1「先 await 完整响应再逐 token 透传」矛盾的关键。

### 5.3.1 Trait（契约级伪代码，非最终 Rust 代码）

```rust
/// LLM transport 抽象：一次调用 = 一次完整请求（含流式增量回调与最终聚合结果）。
/// 本 trait 是 Harness 与 transport 之间唯一耦合点；不暴露任何 genai 类型。
#[async_trait::async_trait]
pub trait LlmProvider: Send + Sync {
    fn name(&self) -> &'static str;                 // "genai-deepseek" | "genai-openai-compatible" | "fake"

    /// 发起一次流式请求。增量通过 sink 同步推送（非阻塞）；返回值为聚合后的最终结果。
    /// 取消：cancel 被触发时必须尽快 drop 底层请求并返回 Err(LlmError{kind:Cancelled})。
    async fn chat_stream(&self, req: LlmRequest, sink: &mut dyn LlmStreamSink, cancel: &CancelToken)
        -> Result<LlmResponse, LlmError>;
}

/// 增量接收端：实现方内部只做「聚合 + try_send 到 ctx.emit」，**不得阻塞、不得 await、不得做 IO**。
pub trait LlmStreamSink: Send {
    fn on_delta(&mut self, delta: LlmDelta);
    fn on_tool_call(&mut self, delta: ToolCallDelta);   // 只供 adapter 内部聚合观察；不直接外发
}
```

> `sink` 是同步 trait（不是 `async`）：唯一实现只做内存聚合与 100 ms 窗口 emit（§5.8），因此可证明不阻塞 transport 的读循环。

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
| `CancelToken` | 包装 `CancellationToken`（tokio-util）的只读句柄 | 由 `NodeContext` 提供；`cancel.is_cancelled()` / `cancelled().await` |
| `SecretString` | 不实现 `Debug`/`Display` 的字符串包装 | 防止 `api_key` 泄漏到日志/trace；`Drop` 时尽力清零 |
| `ToolSchema` | 见 §5.6.1 | 与 `ToolDef` 分离：发给 LLM 的最小结构 |

**关键规则**：
- `LlmPhase` **由 adapter 维护**（"是否已向 sink 交付过任何 `LlmDelta::Token`/`Reasoning`"），不是由 Harness 猜。
- `tool_calls[].args` 必须是**完整可解析**的 JSON 对象；解析失败 → 该 tool call 视为 `TOOL_ERROR`（喂回 LLM，§5.6.6），**绝不执行半截参数**。
- `usage` 缺失时 `source=Unavailable`、三项填 0；**不得**用 0 冒充真实计量（下游按 `source` 判断）。

### 5.3.3 genai 0.7.2 adapter 映射

| genai 0.7.2 侧（transport 事实） | adapter 映射 | 备注 |
|---|---|---|
| `exec_chat_stream` 返回的 SSE 流 | adapter 驱动 `Stream` 循环 | 每个 SSE delta → `ChatStreamEvent` |
| `ChatStreamEvent::Chunk`（正文增量） | `LlmDelta::Token` | 首次交付即 `phase=Streaming` |
| `ChatStreamEvent::ReasoningChunk`（`reasoning_content`） | `LlmDelta::Reasoning` | `deepseek-reasoner` 场景；不进入正文 |
| `ChatStreamEvent::ToolCallChunk` | `LlmDelta::ToolCallStarted/Args`（内部聚合） | genai 已按 `index` 聚合；**以 `End.captured_tool_calls` 为最终值** |
| `ChatStreamEvent::End { captured_usage, captured_stop_reason, captured_tool_calls }` | `LlmResponse.usage` / `.stop_reason` / `.tool_calls` | 需开启 `capture_usage(true)` / `capture_tool_calls(true)` / `capture_reasoning_content(true)` |
| `Error::HttpError { status, headers }` | `LlmError{status, retry_after_ms}` | `retry-after` 取 header 解析；429/5xx 可重试（§5.3.5） |
| 流中途 `StreamError` / JSON 解码失败 | `LlmError{kind: StreamBroken\|Decode, phase:Streaming}` | **不可重试** |
| 建连失败（DNS/TCP/TLS/首包超时） | `LlmError{kind: Connect\|Timeout, phase: Establish}` | 可重试 |
| endpoint / auth 注入 | `provider="deepseek"` → genai 原生 adapter；`"openai_compatible"` → `ServiceTargetResolver` 注入 `base_url` + `Authorization: Bearer` | 具体 builder 签名以 `docs.rs/genai/0.7.2` 为准 |

> **风险与验证**（`_research/harness-alternatives.md` §6 待验证 #1/#2）：genai 对 DeepSeek 的流式 tool_call / reasoning 行为**未实测**。缓解措施：① Harness 只依赖自有 trait；② adapter 单测必须覆盖三个 case（纯文本流、单 tool call、多 tool call）；③ 若行为不符，退回自写 SSE 客户端（+1–2 人日），**不改变本分册任何契约**。
>
> genai 的 `ChatStreamEvent` 变体名以 0.7.2 文档为准；本表是**语义映射**，不是 API 逐字镜像。

### 5.3.4 Fake provider（测试与 `--fake-llm`）

**用途**：AC-01..AC-07 与 e2e（无网络，CI 默认）、单测；`zgraph run --fake-llm[=<script.json>]`（06 §6.10.3；07 §前言）。裸 `--fake-llm` 使用内置默认真实脚本（一段文本 + 一次工具调用），`--fake-llm=<path>` 指定脚本；`--real` 与 `--fake-llm` 互斥，`--real` 用真实 provider（仅 AC-08）。

脚本 schema：

```json
{
  "version": 1,
  "chunk_chars": 3,            // 正文按 N 字符切分为 LlmDelta::Token（默认 3，用于验证 100ms 聚合）
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
| `calls[].stop_reason` | string | 按 kind 推导（`EndTurn` / `ToolCalls`） | 映射到 `StopReason` |
| `failures[]` | array | `[]` | 注入失败：命中 `(call_index, attempt)` 时返回 `LlmError` |
| `failures[].phase` | `establish` \| `streaming` | 必填 | 决定可重试性 |
| `chunk_chars` / `chunk_delay_ms` | int | 3 / 0 | 流式切分与节奏 |

**Fake provider MUST**：① 通过真实 trait 实现（不得绕过 Harness 编排）；② 失败注入走同一 `LlmError` 路径（不得直接 panic）；③ `usage` 缺省时 `source=Estimated`，`total_tokens = prompt+completion`。

### 5.3.5 重试策略（精确规则，关闭 M-04 的"错误分类缺失"）

**一句话结论**：**只有"建立阶段"可重试；一旦向 sink 交付过任何正文/推理 token，任何失败都立即失败，不重试、不续流。**

| 情形 | `phase` | 可重试 | 动作 | 最终错误码 |
|---|---|---|---|---|
| 建连失败（DNS/TCP/TLS）、请求未被服务端接受 | Establish | ✅ | 退避后重试 | `HARNESS_LLM_ERROR` |
| HTTP 429 | Establish | ✅ | 优先 `retry_after_ms`，否则退避 | `HARNESS_LLM_ERROR` |
| HTTP 5xx | Establish | ✅ | 退避 | `HARNESS_LLM_ERROR` |
| HTTP 400 / 401 / 403 / 404 / 422 | Establish | ❌ | 立即失败（配置/鉴权问题重试无意义） | `HARNESS_LLM_ERROR` |
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

> **与 00 §0.5 的措辞差异**：00 写 `DEFAULT_LLM_MAX_RETRIES = 1`、"退避 1s / 2s"。本分册解释为：默认 **1 次重试**、首次重试等待 1s；`2s` 是 `llm_max_retries ≥ 2` 时的第二档退避。若 00 原意是"共 2 次重试"，只需把默认值改为 2，**本分册其余规则不变**（见 §5.11 待裁定）。

### 5.3.6 超时分层（谁先到谁生效）

| 层级 | 默认 | 执行者 | 触发后结果 |
|---|---|---|---|
| 实例超时（`agent_call` 全部轮次） | 180s（DSL 覆盖） | 调度器 watchdog（03 分册） | 实例 `TimedOut`、`NODE_TIMEOUT`、run `Failed`（冷峻）；在飞任务记账 `NODE_ABANDONED` |
| Harness deadline 自检 | 同实例超时 | Harness（每次 LLM 调用前） | 剩余 ≤ 0 → 返回 `NodeError::Timeout{elapsed_ms}`（与调度器同语义，不抢跑状态） |
| 单次 LLM 调用 | 60s | `LlmRequest.timeout_ms` | `HARNESS_LLM_TIMEOUT`（可按 §5.3.5 重试） |
| 单次工具调用 | 30s | Harness（§5.6.5） | `TOOL_ERROR` 喂回 LLM（**不终止**） |

**计算式**：`request_timeout = min(llm_timeout_ms, node_deadline_remaining)`；若 `node_deadline_remaining < 1000ms`，不再发起新调用，直接返回实例超时错误。

### 5.3.7 取消

| 场景 | 行为 |
|---|---|
| `ctx.cancel_requested()` == true（LLM 调用前） | 立即返回 `NodeError::Cancelled`，不发新请求 |
| LLM 调用中取消 | adapter 必须 drop 底层 future（取消 = reqwest future drop，不保证 TCP FIN）→ 返回 `LlmError{kind:Cancelled}` → 映射 `NodeError::Cancelled` |
| 工具执行中取消 | Harness **不再等待**（`select!` 取消分支优先），返回 `NodeError::Cancelled`；Python 工具任务成为"被放弃任务"，由调度器统一记账（`NODE_ABANDONED` warning） |
| 取消后的部分输出 | 全部丢弃：不写上下文、不进 output；已 emit 的 `TokenDelta` 保留在 trace（已发生的事实） |

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

**重试位置**：`Invoking` 内部的"单次调用"可重试（§5.3.5），**状态机不因重试而改变状态**，也不增加 iteration。

**边界规则**：

| 规则 | 规定 |
|---|---|
| 不变量 | `ToolExec` 至少对应 1 个已发出的 `ToolCallStart`；`Returning/Failed` 进入前必须已 flush 流式聚合窗口 |
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
| 未知工具（任一） | 立即 `HARNESS_UNKNOWN_TOOL` → `Crashed`；已执行的调用结果不写上下文（但 trace 中保留已发生的 `ToolCallEnd`） |
| 数量上限 | 不做硬限制；若单响应 tool_calls > 32，视为 provider 异常 → `HARNESS_LLM_ERROR`（`detail.reason="too_many_tool_calls"`） |
| 空 tool_calls + 空 content | 视为"模型无输出"：追加 assistant 空消息并继续下一轮；连续 2 次空响应 → `HARNESS_LLM_ERROR`（`reason="empty_response"`） |

### 5.5.4 执行循环（契约级伪代码，**非最终 Rust 代码**）

> **说明**：以下伪代码只冻结**语义**（调用顺序、错误分支、iteration 规则）。所有权/借用写法仅为示意——实现可按 03 分册的所有权模型调整。v1.1 的三处硬伤在此修正：① 无阻塞 `chat()`，改为 `chat_stream` + sink；② 无 use-after-move（先解构响应再使用）；③ 无命名参数语法（用 struct 构造）；④ 不在 `&RunContext` 上调 `emit`（只经 `&NodeContext`）。

```rust
// 契约级伪代码（非最终 Rust 代码）
async fn run_agent(agent: Arc<AgentConfig>, tools: Arc<ToolRegistry>, inputs: Value,
                   ctx: &NodeContext, llm: &dyn LlmProvider) -> Result<AgentOutput, HarnessError> {
    // ── Phase Loading：由节点执行器预先完成（配置已校验；tools 已解析）
    let tool_schemas: Vec<ToolSchema> = tools.schemas(&agent.tools);   // 见 §5.6

    // ── Phase Assembling
    let mut messages: Vec<LlmMessage> = assemble_messages(&agent, &inputs);   // §5.4.2
    check_and_trim(&mut messages, &tool_schemas, &agent)?;                    // §5.4.5

    let mut iteration: u32 = 0;
    let mut usage_total = Usage::zero();

    loop {
        if iteration >= agent.max_iterations {
            // 00 §0.4：携带当前上下文摘要（不返回部分输出）
            return Err(HarnessError::max_iterations(iteration, &messages));
        }
        if ctx.cancel_requested() {
            return Err(HarnessError::cancelled());
        }
        if ctx.node_deadline_remaining_ms() == 0 {
            return Err(HarnessError::instance_timeout(&ctx));                 // 映射 NodeError::Timeout
        }

        iteration += 1;
        ctx.emit(StreamingEvent::Progress {
            percent: progress_percent(iteration, agent.max_iterations),       // 见 §5.8.2
            message: format!("llm call {}/{}", iteration, agent.max_iterations),
        });

        // ── Phase Invoking：流式调用（重试在 provider 内部按 §5.3.5 处理）
        // sink 只做 1:1 转发（harness 不聚合；100ms 聚合在 06 写线程，见 §5.8.3）
        let mut sink = StreamSink::new(ctx);
        let request = build_request(&agent, &messages, &tool_schemas, iteration);
        let response = match llm.chat_stream(request, &mut sink, ctx.cancel_token()).await {
            Ok(r) => r,
            Err(e) => return Err(map_llm_error(e, iteration)),               // §5.7
        };
        usage_total.accumulate(&response.usage);

        // 先解构，杜绝 v1.1 的 use-after-move
        let LlmResponse { content, tool_calls, stop_reason, .. } = response;

        // ── Phase ToolExec（串行；iteration 不因工具数增加）
        if !tool_calls.is_empty() {
            messages.push(LlmMessage::Assistant { content: content.clone(), tool_calls: tool_calls.clone() });
            for call in tool_calls.iter() {
                // 未知工具：立即失败（不是 TOOL_ERROR）
                if !tools.contains(&call.name) {
                    return Err(HarnessError::unknown_tool(&call.name, iteration));
                }
                ctx.emit(StreamingEvent::ToolCallStart {
                    call_id: call.call_id.clone(),
                    tool_name: call.name.clone(),
                    args_summary: summarize_args(&call.args),                 // 见 §5.8.4
                });

                let outcome = execute_tool(&tools, call, ctx).await;         // §5.6.5：spawn_blocking + 超时
                let (ok, tool_content) = match outcome {
                    Ok(value) => (true, serialize_tool_result(&value)),       // 单条 ≤ 8 KiB
                    Err(tool_err) => (false, serialize_tool_error(&tool_err)),// 喂回 LLM，继续循环
                };
                ctx.emit(StreamingEvent::ToolCallEnd {
                    call_id: call.call_id.clone(),
                    tool_name: call.name.clone(),
                    ok,
                    result_summary: summarize_result(&tool_content, ok),
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
        if final_content.is_empty() && stop_reason == StopReason::EndTurn {
            return Err(HarnessError::llm_error("empty_response", iteration));
        }
        return Ok(AgentOutput {
            content: final_content,
            stop_reason,
            iterations: iteration,
            usage: usage_total,
        });
    }
}
```

> **关于 `HarnessError::cancelled()` / `HarnessError::instance_timeout(&ctx)`**：这两个是**哨兵构造器**，不对应 00 §0.3 的 `HarnessErrorKind` 七值。它们只在 Harness 内部用于提前返回，节点执行器出口按 §5.7.3 分别映射为 `NodeError::Cancelled` / `NodeError::Timeout{elapsed_ms}`——**取消与实例超时永远不是 Harness 错误**（否则实例会被误标 `Crashed`，与 00 的 run `Cancelled` 语义冲突）。

### 5.5.5 v1.1 硬伤修正对照（审计 P-05/P-06/P-09、C-19、M-17 相关）

| v1.1 写法（不可编译/不可实现） | 问题 | v1.2 修正 |
|---|---|---|
| `self.llm_provider.chat(model: ..., messages: &messages, tools: ...)` 后 `self.stream_tokens(ctx, &response)` | Rust 无命名参数；`chat()` 返回完整响应后再"逐 token"根本不存在流；阻塞接口与流式矛盾 | 改为 `chat_stream(req, &mut sink, cancel)`：**流式是唯一接口**，sink 在接收增量时同步聚合 emit；返回值只是"最终聚合结果"（§5.3.1） |
| `match response { LlmResponse::ToolCall { tool_name, args } => ... }` | 三态枚举无法表达"文本 + 多 tool_calls + usage"；单数 ToolCall 丢信息 | `LlmResponse` 改为 struct（content/tool_calls/stop_reason/usage），多 tool_call 为 `Vec`（§5.3.2/§5.5.3） |
| `let agent = ...; ... agent.id ... ; skills.find(tool_name)` 中先 move 后借用；`args` 同 | use-after-move | 循环内对 `call` 用 `&` / `clone`（`call_id`、`name` 各 clone 一次）；响应先解构再使用（§5.5.4） |
| `ctx.emit(...)` 且 `ctx: &RunContext` | `RunContext` 不暴露 emit；违反 H1 | 签名改为 `ctx: &NodeContext`；`emit` 为其同步非阻塞方法（04 分册） |
| 只在 tool 分支 `iteration += 1` | iteration 语义错（文本轮不计）且上限判定漏 | 请求前 `+1`，文本/tool 轮统一（§5.5.2） |
| `LlmResponse::HumanRequest` → `NeedHumanInput` | v1 不做人工介入，且与"Err → Crashed"冲突（C-12） | v1 **不产生** `NeedHumanInput`；模型若在文本中请求人工，只是普通 `content`（§5.10.1） |
| 上下文溢出"归档后清空继续" | 无界循环风险（S-15） | 只允许丢短期旧轮；仍溢出 → `HARNESS_CONTEXT_OVERFLOW`（Crashed）（§5.4.5） |
| `memory.append_turn(..., Assistant, ...)` 只写 assistant | 历史缺 user 侧（S-14） | v1 Memory stub：**不写不读**；接口在 v1.1 冻结为 `append_turn(role, ...)`（§5.10.1） |

---

## 5.6 工具注册表与 ToolDef（关闭 M-03）

**一句话结论**：工具是**Python 侧声明 + Rust 侧注册表**；LLM 只能看到 `AgentConfig.tools` 列出的工具；工具执行失败一律 `TOOL_ERROR` 喂回 LLM，绝不终止；调用未注册工具才是 `HARNESS_UNKNOWN_TOOL`（终止）。

### 5.6.1 ToolDef 字段（Rust 侧内存模型，冻结）

```rust
pub struct ToolDef {
    pub name: String,            // ^[a-z][a-z0-9_]{0,63}$；全局唯一（跨文件）
    pub description: String,     // 1..=1024 字符；作为 LLM tool 描述
    pub parameters: Value,       // JSON Schema（见下方子集约束）
    pub timeout_ms: u64,         // 默认 DEFAULT_TOOL_TIMEOUT_MS = 30000
    pub source: String,          // Python 模块全名，如 "tools.builtin"；仅诊断用
}
pub struct ToolSchema { pub name: String, pub description: String, pub parameters: Value }
```

| `parameters` 子集约束（v1） | 规定 |
|---|---|
| 顶层 | 必须是对象，且 `"type": "object"`；`"properties"` 必须为对象（可为空 `{}`） |
| 允许的关键字 | `type`（`object`/`string`/`number`/`integer`/`boolean`/`array`/`null`）、`properties`、`required`、`items`、`enum`、`description`、`default`、`minimum`/`maximum`、`minLength`/`maxLength`、`additionalProperties`（仅 bool） |
| 不允许 | `$ref`、`oneOf`/`anyOf`/`allOf`、`patternProperties`、`if/then/else` → `HARNESS_CONFIG_LOAD{reason:"unsupported_schema_keyword"}` |
| 校验深度 | v1 只做顶层 `required` 与 `properties` 的类型检查（§5.6.4），不引入 JSON Schema 引擎 |

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

**形式 B（单工具模块）**——`tools/now.py`：

```python
# tools/now.py
TOOL = {"name": "now", "description": "当前时间（ISO8601，UTC）",
        "parameters": {"type": "object", "properties": {}, "required": []}}

def call(args: dict) -> dict:
    ...
```

**注册表发现规则**：

| 项 | 规定 |
|---|---|
| 目录 | CLI `--tools-dir <dir>` > env `ZGRAPH_TOOLS` > `<assets-dir>/tools`（02 §2.6.3 的资产根） |
| 扫描 | 递归 `*.py`，跳过 `_` 开头的文件/目录与 `__pycache__` |
| 模块名 | 相对 tools_dir 的点分名（`tools.builtin` / `tools.now`）；导入失败 → `HARNESS_CONFIG_LOAD{reason:"tool_module_import"}` + traceback |
| 形态判定 | 模块有 `TOOLS` → 形式 A（必须有 `call(name, args)`）；有 `TOOL` → 形式 B（必须有 `call(args)`）；两者都有或都没有 → `HARNESS_CONFIG_LOAD` |
| 重名 | 跨模块同名 → `HARNESS_CONFIG_LOAD{reason:"duplicate_tool",sources:[...]}` |
| 引用名解析 | `AgentConfig.tools` 项含 `.` 时先查全限定名（`tools.builtin.now`）；否则按 `name` 查（v1 要求 name 全局唯一，故无歧义） |
| 导入时机 | **首次执行 `agent_call` 前**在 `spawn_blocking` 中一次性导入全部模块（懒加载 + 进程内缓存）；导入失败 → 该实例 `Crashed` |

### 5.6.3 注册 -> 请求 tools 字段

- `AgentConfig.tools = ["now", "http_get"]` → `ToolRegistry::schemas()` 返回**仅这两个**的 `ToolSchema`，顺序与 `tools` 数组一致（保证请求字节可复现）。
- `tools` 为空 → `LlmRequest.tools = []`，adapter MUST 在请求体中**省略** `tools` 字段（部分 provider 对空数组报错）。

### 5.6.4 参数校验（调用前，Rust 侧）

| 检查 | 失败处理 |
|---|---|
| `args` 是 JSON object | `TOOL_ERROR{reason:"args_not_object"}` 喂回 LLM |
| `required` 中每个 key 存在 | `TOOL_ERROR{reason:"missing_required",keys:[...]}` |
| `properties` 中已声明 key 的类型匹配（string/number/integer/boolean/object/array） | `TOOL_ERROR{reason:"type_mismatch",key,expected,actual}` |
| 未声明 key | v1 **允许**（宽松；模型可能多传），不报错 |
| 深层/枚举/长度约束 | 交给 Python 工具自行校验（工具抛异常 → `TOOL_ERROR`） |

> 校验失败**不调用 Python**，直接把错误 JSON 作为 `Tool` 消息喂回，让 LLM 修正参数——这是 tool loop 的正常路径，不算失败。

### 5.6.5 执行、超时与结果处理

| 项 | v1 规定 |
|---|---|
| 执行线程 | `spawn_blocking`（Python 调用不得在事件循环线程，00 §0.6 I8） |
| 并发 | 单实例内**串行**（§5.5.3）；跨实例受 Python I/O 并发上限约束（`PYTHON_CONCURRENCY_IO`，默认 4；04 分册）。工具与 Python 节点共享该信号量 |
| 超时 | `min(ToolDef.timeout_ms, DEFAULT_TOOL_TIMEOUT_MS)`；默认 30s，**stop-waiting**：超时后不再等待，结果丢弃并记账（`NODE_ABANDONED` 由调度器计）；本次调用返回 `TOOL_ERROR{reason:"timeout",timeout_ms}` |
| 返回值 | 必须可 JSON 序列化为 object（`dict`）；非 dict → `TOOL_ERROR{reason:"result_not_object",actual_type}` |
| 序列化 | `serde_json::to_string`（紧凑）；≤ 8 KiB（`DEFAULT_TOOL_RESULT_MAX_BYTES`） |
| 截断 | 超 8 KiB → 截断为合法 JSON 字符串片段并附 `…[truncated N bytes]`（原始结果 v1 不落盘，仅 trace 中 `result_summary` 记录长度） |
| 异常 | Python 异常 → `TOOL_ERROR{reason:"exception", type, message, traceback(≤8 KiB)}`；**解释器必须可继续使用**（`_spike/REPORT.md` §3.5 已验证） |
| 结果摘要 | `ToolCallEnd.result_summary` 只放摘要（≤ 1 KiB）：成功放 `{"keys":[...],"bytes":N}` 或小对象；失败放错误对象 |

### 5.6.6 工具错误语义（关闭 M-03/C-12 的一半）

| 事件 | 终止 run？ | 喂回 LLM？ | trace |
|---|---|---|---|
| 参数校验失败 | ❌ | ✅ | `ToolCallEnd{ok:false}`（可丢） |
| 工具抛异常 | ❌ | ✅ | 同上 |
| 工具超时 | ❌ | ✅ | 同上 + `TOOL_ERROR` 摘要 |
| 返回非 object | ❌ | ✅ | 同上 |
| 工具名未注册（LLM 幻觉） | ✅ `HARNESS_UNKNOWN_TOOL` → `Crashed` | ❌ | `instances.error` + 迁移 `Crashed` |
| `AgentConfig.tools` 引用了未注册工具 | ✅ 加载期 `HARNESS_CONFIG_LOAD` | ❌ | 不产生 trace（加载失败） |

> `TOOL_ERROR` 的载体是**工具结果消息**，不是 `NodeError`：它永远不进入实例错误列。`HarnessErrorKind::ToolError` 只用于内部结构化传递与 `ToolCallEnd` 摘要（00 §0.4 已规定其"继续"语义）。

---

## 5.7 错误映射表（关闭审计 M-17 / C-12；对齐 00 §0.4）

**一句话结论**：`HarnessErrorKind` → 错误码 → `NodeError::Harness{kind,detail}` → 实例状态/run 状态 → trace 级别，一一对应且**不新增枚举值**。

### 5.7.1 `HarnessError` 形态

```rust
pub struct HarnessError {
    pub kind: HarnessErrorKind,      // 00 §0.3 的七值
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
| `UnknownTool` | `HARNESS_UNKNOWN_TOOL` | `Harness{kind:UnknownTool,detail}` | `Crashed` | `Failed` | `error` | ✅ 工具名 + 可见工具列表 | `{tool, iteration, available_tools[≤32]}` |
| `MaxIterations` | `HARNESS_MAX_ITERATIONS` | `Harness{kind:MaxIterations,detail}` | `Crashed` | `Failed` | `error` | ✅ **当前上下文摘要**（00 要求） | `{iterations, max_iterations, last_content_summary(≤512 字符), pending_tool_calls, usage}` |
| `LlmError` | `HARNESS_LLM_ERROR` | `Harness{kind:LlmError,detail}` | `Crashed` | `Failed` | `error` | ✅ provider 诊断 | `{provider, model, kind, phase, status, attempts, streamed_tokens}` |
| `LlmTimeout` | `HARNESS_LLM_TIMEOUT` | `Harness{kind:LlmTimeout,detail}` | `Crashed` | `Failed` | `error` | ✅ 超时值 | `{timeout_ms, attempts, phase, iteration}` |
| `ToolError` | `TOOL_ERROR` | **不产生**（走工具结果消息） | 实例继续 | 无 | `warn`（`ToolCallEnd{ok:false}`） | ✅ 错误 JSON | `{reason, tool, timeout_ms?}` |
| `MemoryDegraded` | `HARNESS_MEMORY_DEGRADED` | **不产生**（降级继续） | 实例继续 | 无 | `warn` | ✅ 操作名 | `{op}` |

**补充映射（非 HarnessErrorKind，但同一出口）**：

| 情形 | `NodeError` | 错误码（run/CLI） | `InstanceStatus` | trace 级别 | 说明 |
|---|---|---|---|---|---|
| 取消（LLM/工具执行中） | `Cancelled` | `RUN_CANCELLED`（若 run 级取消） | `Cancelled` | `info` | 取消**不**映射为 Harness 错误（C-12 冲突的解法） |
| 实例超时（调度器或 deadline 自检） | `Timeout{elapsed_ms}` | `NODE_TIMEOUT` | `TimedOut` | `error` | 到点后不等待任务（stop-waiting） |
| 被放弃任务记账 | 无 | `NODE_ABANDONED` | — | `warn` | 调度器产生，非节点错误 |
| 消息序列非法 / 不变量违反 | `Internal{message}` | `RUN_ILLEGAL_TRANSITION` 或 `Internal` | `Crashed` | `error` | fail-fast（框架 bug） |

**明确否决（C-12 裁定）**：

| 被否决的写法 | 原因 | v1 替代 |
|---|---|---|
| `NeedHumanInput` → `WaitingHuman` | v1 不做人工介入；且与「`Err` → `Crashed`」冲突 | v1 **不产生**该错误/状态；枚举与码位保留到 v1.1 |
| `Err(HarnessError)` 有时 Crashed、有时等待人工 | 无统一映射表导致执行器无法编码 | 本表唯一出口；`Err` 一律 `Crashed`（除取消/超时两条特殊路径） |
| 工具失败也 `Err` → Crashed | 违背 00 §0.4 的 `TOOL_ERROR`「继续」 | 工具失败只写 tool 消息（§5.6.6） |
| 重试耗尽但返回部分输出 + warning（v1.1 §3.4） | 与 00 `HARNESS_MAX_ITERATIONS` → Crashed 冲突 | 一律 `Crashed`，上下文摘要进 `detail`（00 优先） |

### 5.7.3 "取消/超时"为什么不在 HarnessErrorKind 里

- 00 §0.3 的七值**不含取消与实例超时**；它们分别由 `NodeError::Cancelled` / `NodeError::Timeout` 表达，源头是**调度器与节点执行器**，不是 Harness 的 LLM/tool 故障。
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
| `budget` / `required` / `segments` | int/object | ContextOverflow | `segments` 见下 |
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
| 取消/超时 | 对应 `transitions` 行 + `runs.status`/`runs.error_code`；实例超时另产 `TraceEvent::NodeTimedOut`、被放弃任务产 `TraceEvent::NodeAbandoned`（两者由调度器产生、不可丢，06 §6.2） |

---

## 5.8 流式事件映射与聚合（关闭 M-04 的流式形态；对齐 00 §0.3/§0.6 I6）

**一句话结论**：Harness **逐 delta 1:1 emit**（不在 Harness 内聚合）；`NodeContext.emit` 把每个 `StreamingEvent` 包成 `TraceEvent::Streaming{run_id, instance_id, event}`（04 §4.2）走 `try_send` 可丢通道；**100ms 窗口聚合的唯一执行点是 06 写线程**（落库前合并，06 §6.5），避免双窗口二次聚合。

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
| A3 | `call_id` 一致：`ToolCallStart` 与 `ToolCallEnd` 用同一 id；provider 增量未给 id 时 adapter 合成 `local_{iteration}_{index}` 并在响应结束时沿用 |
| A4 | Harness 内**不得**做时间窗聚合（否则与 06 §6.5 形成双窗口，段边界不可复现） |
| A5 | `Progress` 每个 `Invoking` 入口恰好一条（不是每个 token） |

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
| 不丢什么 | 00 §0.3 的 `RunStarted` / `Transition` / `GateResult` / `NodeTimedOut` / `NodeAbandoned` / `RunFinished` —— 全部由调度器产生、走无损通道（`send().await`） |
| 丢弃计数 | **单一计数** `RunMetrics.streaming_dropped`（00 §0.3；04 §4.2 的 run 级原子计数器；随 `RunFinished.metrics` 落库，06 §6.2.6） |
| 行为约束 | 丢弃不得阻塞、不得 panic、不得影响 loop 控制流；丢弃后后续 delta 照常 emit |
| 已知风险 | `ToolCallStart/End` 频率低（≤ iteration × tool 数）但同属可丢类；若被丢，诊断依赖 `instances.output_json` 与进程日志。若需提升为不可丢，须修订 00 §0.6 I6（v1.1 候选） |

### 5.8.5 `args_summary` / `result_summary` 规则

| 字段 | 规则 |
|---|---|
| `ToolCallStart.args_summary` | 参数对象的**结构摘要**：`{"keys":[...], "bytes":N, "preview":"≤256 字符"}`；**不得**放完整参数（可能含敏感数据） |
| `ToolCallEnd.result_summary` | 成功：`{"ok":true,"keys":[...],"bytes":N,"preview":"≤256 字符"}`；失败：`{"ok":false,"error":{...≤1 KiB}}` |
| 截断 | 一律按字符边界（不切 UTF-8 码点中间）；附 `…` |
| 敏感信息 | v1 不做脱敏（本地工具）；**MUST NOT** 记录 `api_key`、Authorization header、prompt 全文 |

---

## 5.9 输出契约与 `AgentProvider`（关闭 C-19）

**一句话结论**：`agent_call` 节点的 output **固定四字段** `{content, stop_reason, iterations, usage}`，整包广播给下游（04 §4.5）；业务字段由下游节点从 `content` 自行解析；v1 不做 `output_mapping`。

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
| `usage` | object | 四个数值字段 + `source`；数值不可为 null（未知填 0 + `source=unavailable`） |

> **不做**：`output_mapping`、`output_schema` 强校验、`content` 自动解析成 `data` 字段（均为 v1.1 保留，见 §5.10.2）。

### 5.9.3 业务字段映射约定（input_mapping / 下游取值）

| 方向 | 约定 | 示例 |
|---|---|---|
| 入参（`input_mapping`） | `{"<节点输入端口>": "<agent 输入字段>"}`；未列出的端口按**同名**映射（identity） | `"input_mapping": {"message": "user_message"}` → 节点端口 `message` 的值成为 Agent 输入字段 `user_message` |
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
| `BuiltinHarness` | ✅ 唯一实现 | 本分册 §5.2–§5.8 |
| `CodexAppServerProvider` | ❌（v2 候选） | 外挂子进程 + JSON-RPC；`dynamicTools` 仍 experimental；提供的是 turn 循环（与我们 loop 冲突，需"仅用其 transport"再评估） |
| `AcpProvider` | ❌（v2 候选） | ACP 只上线语义事件、拿不到 token delta（`_research/harness-alternatives.md` §3.3），与 §5.8 冲突，需要降级策略 |
| `FakeProvider`（测试） | ✅ | 仅用于 `LlmProvider` 层（§5.3.4），不是 `AgentProvider` |

> v1 选择器：`agent_call` 节点固定 `BuiltinHarness`；`AgentProviderKind` 的其余两值在配置中出现 → `HARNESS_CONFIG_LOAD{reason:"provider_not_implemented"}`。

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

## 5.11 与 00 总纲的对齐检查（含需 lead 裁定的冲突点）

**一句话结论**：本分册**未**发现与 00 枚举/错误码/不变式的语义冲突（已按 00 v1.2 当前版本对齐，含新增的 `TraceEvent` 七类与 `RunMetrics`）；以下 6 条是 00 未覆盖的**常量缺口**、2 条**跨分册待统一项**与 1 条**待裁定**。

### 5.11.1 本分册新引入的常量（建议 00 §0.5 补录）

| 常量 | 值 | 出处 | 说明 |
|---|---|---|---|
| `DEFAULT_LLM_MAX_TOKENS` | `2048` | §5.2.2 | `model.max_tokens` 默认 |
| `DEFAULT_LLM_TEMPERATURE` | `0.2` | §5.2.2 | `model.temperature` 默认 |
| `DEFAULT_CONTEXT_WINDOW_TOKENS` | `65536` | §5.2.2 | 预算总来源（DeepSeek 64K 口径） |
| `CONTEXT_BUDGET_SAFETY_RATIO` | `0.8` | §5.4.4 | 估算误差安全系数 |
| `TOOL_RESULT_MAX_BYTES` | `8192` | §5.6.5 | 工具结果进上下文的上限 |
| `RETRY_AFTER_MAX_MS` | `30000` | §5.3.5 | `retry-after` 上限 |
| `BACKOFF_BASE_MS` | `1000` | §5.3.5 | 指数退避基数（上限 8_000） |

> 流式聚合相关常量（`STREAM_TOKEN_AGGREGATE_MS` / `STREAM_SEGMENT_MAX_CHARS` / `STREAM_SEGMENT_MAX_BYTES`）由 **06 §6.5** 持有（聚合执行点在写线程），本分册不重复定义，避免双份常量漂移。

### 5.11.2 与 00 的措辞/覆盖差异（需确认）

| # | 项 | 00 现状 | 05 处理 | 需谁裁定 |
|---|---|---|---|---|
| G1 | 重试次数语义 | §0.5 `DEFAULT_LLM_MAX_RETRIES=1` + "退避 1s / 2s" | 解释为"1 次重试，等待 1s；2s 属第 2 档" | lead 确认（若原意 2 次重试，只改默认值） |
| G2 | Token 聚合窗口 | §0.5 无该常量 | 05 只定义语义（100ms 窗口），参数与执行点归 06 §6.5 | 00 补录常量（放 06 名下） |
| G3 | `NodeError::Config` vs `Harness{kind:ConfigLoad}` | §0.3 两者都存在；§0.4 只说 `HARNESS_CONFIG_LOAD → Crashed` | 裁定：Harness 内配置失败一律 `NodeError::Harness{kind:ConfigLoad}`；`NodeError::Config` 留给**非 Harness** 的资产加载 | lead/04 分册确认 |
| G4 | `HARNESS_CONFIG_LOAD` vs `DSL_AGENT_NOT_FOUND` 重叠 | 两个码都能表达"Agent 资产缺失/非法" | 按阶段分工：图期只判**资产索引命中**（`DSL_*`），运行期判合法性（`HARNESS_*`） | 02 已定（§2.6.3），本分册对齐 |
| G5 | tool 生命周期事件可丢 | I6 明确 6 类不可丢（含 `NodeTimedOut`/`NodeAbandoned`），`Streaming` 全可丢 | 接受 `ToolCallStart/End` 可丢（单一 `streaming_dropped` 计数）；风险已登记 | v1.1 若要提升为不可丢，需修订 I6 |
| G6 | `Usage`/`stop_reason` 的落点 | 00 `TraceEvent`/`StreamingEvent` 均无 usage 变体 | 只随节点 `output_json`（§5.9.2），不新增事件 | 无冲突（已闭合） |
| G7 | trace 通道拓扑 | 00 §0.5 有独立的 `STREAMING_CHANNEL_CAP=1024`；03 §3.12 描述"单通道 8192"；04 §4.2/A17 按 00 双通道 | 06 采用**双通道**（无损 8192 + 流式 1024），并在 06 §6.12 登记需 03 同步修订 | 03/06/lead |
| G8 | `input_mapping` 键值方向 | 决策记录 §2.3 示例注释两可；02 §2.3.2/§2.6.3 定为"端口 → Agent 字段" | 05 采用 02 方向（key=端口名，value=Agent 字段名），示例已改 `{"message":"user_message"}` | 已按 02 定案 |
| G9 | Agent 资产文件名 | 决策记录示例 `agents/message_parser.json`；02 §2.6.3 明确"文件名无关，按文件内 id 索引" | 05 采用 02 口径（建议同名但不强制） | 已按 02 定案 |

### 5.11.3 未与 00 冲突但需 06 配合的点

| 点 | 05 要求 | 06 落点 |
|---|---|---|
| 丢弃计数 | **单一** run 级计数（`RunMetrics.streaming_dropped`） | `runs.streaming_dropped`（06 §6.2.4） |
| 错误落库 | `instances.error` 带 `code` | `instances.error_code` 列（06 §6.2.5） |
| Streaming 聚合分段 | Harness 1:1 emit；**写线程**聚合为段（100ms / 4096 字符） | `streaming_events` 行（06 §6.5） |
| Harness 内部状态 | 不落库 | trace DB 无 Harness 状态行（06 §6.3） |
| 取消/超时 | 不产生 Harness 错误；由调度器产 `Transition` + `NodeTimedOut`/`NodeAbandoned` | `events.kind` 含 `NodeTimedOut`/`NodeAbandoned`（06 §6.2.6） |

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
