> **归档说明**：本文件是 DR-001 决策前的范围/方案拍板记录（冻结期归档）。其中技术方案部分已被 [`DR-001`](../decisions/DR-001-runtime-route.md) 取代；路径为历史引用，映射见 [`spec-v1.2/REFERENCES.md`](../spec-v1.2/REFERENCES.md)。

# 决策记录 01 · 范围、失败语义与 Harness 选型（v1）

> 日期：2026-10-28
> 状态：**已拍板**（用户 2026-10-28 批复 + 本记录中的工程裁定）
> 关联：`docs/可行性评估-详细设计v1.1.md`、`_audit/spec-audit.md`、`_research/codex-embed.md`、`_research/harness-alternatives.md`

---

## 1. 用户已拍板的 6 项

| # | 决策 | 结论 |
|---|---|---|
| D1 | 范围 | **只做内核**（不含 API 服务 / 前端 / Agent 资产平台 / 人工介入 / 子图执行） |
| D2 | Harness | **不整包复用**别家 Harness（Codex / DSH 均否决为"内核组件"）；自研六阶段薄壳 + 复用 transport 层（genai）+ 借鉴 Codex/DSH 语义 |
| D3 | 失败语义 | **冷峻模式**：任一实例 Crashed / TimedOut → run 立即 Failed；gate 永久拦截 → Skipped |
| D4 | 人工介入 | **v1 不做**（枚举与错误变体保留，不产生 WaitingHuman） |
| D5 | 超时 | **停止等待**（stop-waiting）+ 协作式取消 + 被放弃任务记账 |
| D6 | 验收 | 采用评估报告所列断言（见 §5） |

---

## 2. 内核边界（v1 交付物）

### 2.1 In Scope

| 层 | 交付 |
|---|---|
| DSL / 编译期 | Graph DSL 全部类型；Loader **14 条校验规则**（R1–R14，含子图定义闭合校验）；`CompiledGraph` + 双向边索引；**17 个** `DSL_*` 错误码 |
| 运行期内核 | 实例状态机（Pending/Ready/Running/Completed/Crashed/TimedOut/Skipped/Cancelled）；Input Buffer（all/any）；**9 条不变式 I1–I9**（debug 断言 + 测试）；`RunState`/`InstanceRecord`/`Delivery`/`TransitionRecord` |
| Scheduler | **每个 run 一个事件循环任务**（单 run 内严格串行、多 run 并发）+ channel 回传；暂停/恢复/取消；实例超时（stop-waiting）；被放弃任务记账；卡死判定；冷峻失败传播；run 终态唯一 |
| 执行侧 | `NodeHandler` trait；start/end 内置 handler；Python 节点 + Python gate（PyO3，`spawn_blocking` + 独立并发上限）；`NodeContext`（emit/get_var/user_id/cancel_requested）；SymbolResolver + 签名校验 + 异常 traceback |
| Agent Harness | 自研薄壳：Loading → Assembling → LLM ↔ Tool → Returning/Failed；上下文组装 + token 预算；OpenAI-compatible Chat Completions + SSE 流式；tool_call 循环（工具来自 Python skill 注册表）；retry/timeout/停止条件；错误映射 |
| 可观测 | TraceEvent（RunStarted/Transition/Streaming/GateResult/**NodeTimedOut**/**NodeAbandoned**/RunFinished，**7 类**）→ 双通道 mpsc → 单写线程 → SQLite（WAL、批量事务）；事件序号 seq |
| CLI | `zgraph validate <graph.json>`；`zgraph run <graph.json> --inputs <json> [--fake-llm]`；`zgraph trace <run_id>`（按 seq 回放） |
| 质量 | 单元测试 + e2e 示例（客服工单流）+ 不变式测试 + 故障注入（超时/异常/坏图） |

### 2.2 Out of Scope（记录为 v1.1+ 待办）

| 项 | 处理方式 |
|---|---|
| REST/WS API 服务 | v1.1；内核只暴露 Rust API + CLI |
| 前端编辑器 / Trace 视图 | v1.1+（前端设计 0 基础，需先出交互稿） |
| 人工介入（WaitingHuman 全链路） | v1.1；枚举保留，`NeedHumanInput` 映射为 Crashed + warning |
| 子图**执行** | v1.1+；DSL 定义与校验保留，运行到 `subgraph` 节点返回 `NODE_TYPE_UNSUPPORTED` |
| Agent 资产平台（Memory/KB/Skill 管理） | 接口 + 文件 stub；真实实现另立项 |
| 断点恢复（进程重启续跑） | 不做；trace 落库仅用于可观测/回放 |
| 多租户 / 服务化调度 | 内核支持同进程多 run，但不做 API 层与租户隔离 |

### 2.3 v1 的 Agent 配置格式（填补审计 M-01 空缺，文件资产）

```jsonc
// agents/message_parser.json
{
  "id": "message_parser",
  "name": "消息解析器",
  "model": { "provider": "openai_compatible", "name": "deepseek-chat",
             "base_url": "https://api.deepseek.com/v1", "api_key_env": "DEEPSEEK_API_KEY",
             "temperature": 0.2, "max_tokens": 2048 },
  "role_prompt_file": "prompts/message_parser.md",
  "constraints_file": "prompts/global_constraints.md",
  "tools": ["now", "http_get"],              // 引用 Python skill 注册表
  "max_iterations": 10,
  "timeout_ms": 60000,
  "input_mapping": { "message": "user_message" }   // key=节点端口名 → value=Agent 输入字段
}
```

工具定义（Python 侧，一个文件可导出多个）：

```python
# skills/builtin.py
TOOLS = [
    {"name": "now", "description": "当前时间", "parameters": {"type": "object", "properties": {}}},
    {"name": "http_get", "description": "GET 请求", "parameters": {...}},
]
def call(name: str, args: dict) -> dict: ...
```

---

## 3. Harness 复用结论（回答"能不能直接复用 Codex / DSH"）

### 3.1 一句话

**不能直接复用，也不建议复用整包。** 自研 ~1.2–1.5k LOC 的薄壳，只复用通用 transport 组件（genai），并借鉴 Codex/DSH 的流式与工具语义。

> 本节依据两份证据型调研：`_research/codex-embed.md`（Codex，23 条证据链接）、`_research/harness-alternatives.md`（Rust 生态 + DSH，含可复核命令）。

### 3.2 Codex（openai/codex，Apache-2.0）

| 路径 | 事实 | 成本 | 结论 |
|---|---|---|---|
| A 整包嵌入（`codex-core` git 依赖） | 未发布 crates.io；workspace 版本恒 `0.0.0`、165 个成员；`codex-core` 一级依赖 **115 个（74 个 in-repo）**；无 semver、周更多次；官方社区 issue #56902（要求 embedding facade）open 且 0 回复；`wire_api="chat"` 已硬删除，只剩 Responses 协议 | **25–40 人日 + 每月 1–2 人日** | ❌ 否决 |
| B app-server 子进程 + JSON-RPC（外挂） | 官方支持深集成；`dynamicTools`/`item/tool/call` 仍 experimental；turn 循环、上下文压缩、停止判定都在它那边；双状态（它的 rollout/SQLite vs 我们的图状态） | **12–20 人日 + 0.5–1 人日/月** | ⏸ 仅作未来 `AgentProvider` 备选 |
| C **仅参考、自研**（推荐） | 只借语义：tool_call delta 按 item_id 累积、`OutputItemDone` 才执行；Op/Event 单向双队列；SSE 重试与错误分类 | **3–5 人日**（本就属于内核工作） | ✅ 采纳 |

**否决 A 的根本原因**：不是"拦不住"，而是 **Codex 拥有 LLM 调用与 turn 循环**——配置、认证、状态、rollout、compaction 全归它。这与本设计第 3 章"Harness 拥有六阶段"的前提冲突，且与我们的 SQLite 图状态形成双事实源。

### 3.3 DeepSeek Harness（DSH）

**事实**（2026-10-28 复核）：DSH **是 MIT 开源项目**，不是私有软件——
- 仓库：[`deepseek-ai/deepseek-harness`](https://github.com/deepseek-ai/deepseek-harness)，MIT，TypeScript，263,737★，2026-10-23 仍在提交；
- npm：[`@deepseek-ai/dsh`](https://www.npmjs.com/package/@deepseek-ai/dsh)，MIT，39 个版本，latest `0.2.1-rc.2`，repository 指向上述仓库；
- README 自述："open-source agent harness"、"everything-is-a-plugin"（Cordis 插件框架），**developer preview，明确会有破坏性变更**；
- 本机安装的 `@deepseek-ai/dsh-desktop` 包 `private: true` 只表示"该 workspace 包不单独发布"，不代表运行时私有。

**但不可嵌入 Rust 内核**，理由三条：
1. **语言/运行时错位**：Node/TS + Electron，与 Rust 内核是两个进程、两套状态；要么 IPC 要么重写。
2. **它同样"拥有 loop"**：agent loop、工具执行、上下文/compaction、流式语义都在 Node 侧；嵌入等于把 Harness 交给它，与 D2 前提冲突（与 Codex 方案 A 同病）。
3. **事件粒度**：对外集成面是 ACP/JSON-RPC 之类的 agent 级事件，不是 provider 原始 token delta（ACP 协议粒度为 agent message 级）；要做 token 级 trace 必须在 Node 侧改插件。

**结论**：**可用（MIT），但不作为内核组件**。定位为 ①**参考实现**（插件化架构、ACP 集成、工具呈现设计值得借鉴）→ 采纳；② 未来 `AcpProvider`（进程外 subagent，经 ACP 驱动 DSH/其他兼容 agent）→ v1.1 候选，0 人日投入。

### 3.4 复用清单（v1 实际引入）

| 能力 | 方案 | 许可 | 说明 |
|---|---|---|---|
| **LLM transport + SSE 流式 + tool_call delta 聚合** | **`genai` 0.7.2** | MIT OR Apache-2.0 | 原生 `deepseek` adapter + 自定义 OpenAI-compatible（`GENAI_n_ENDPOINT`）；每 SSE delta 一个 `Chunk`；tool_call delta 已按 `index` 聚合；`reasoning_content` 独立事件；错误带 status / `retry-after`；**只做 transport，不含 agent loop**——正好满足"事件直接映射 NodeContext.emit、工具执行留给我们" |
| JSON / 错误 | `serde` / `serde_json` / `thiserror` / `anyhow` | MIT/Apache-2.0 | |
| Python 嵌入 | `pyo3` 0.30 | Apache-2.0/MIT | 已验证 |
| SQLite | `rusqlite`（bundled） | MIT | 已验证 |
| CLI | `clap` | MIT/Apache-2.0 | |
| MCP（v1.1+） | `rmcp` 3.8 | Apache-2.0 | 工具协议，不替代 transport |
| **不引入** | `codex-*` 任何 crate；`rig`（AgentRun 接管 turn budget / tool dispatch / history，与我们的 ctx/trace 冲突）；`async-openai`（备选：9.9M 下载、类型最全，但 tool delta 需自聚合、不建模 `reasoning_content`）；DSH 运行时（Node） | — | |

> **Transport 选型结论**：用 `genai` 而不是自写（自写 SSE 客户端约 400–600 LOC，收益不足）。**但要用自有 `LlmProvider` trait 包一层**，把 genai 的类型挡在边界外——它是 0.7.x，版本演进快，将来换 `async-openai` 或自写只需替换 adapter。若 genai 对 DeepSeek 的流式/工具行为有偏差（未实测，见下），退回 self-written 客户端，成本 +1–2 人日。

### 3.5 预埋扩展点

```rust
#[async_trait]
pub trait AgentProvider: Send + Sync {
    async fn run(&self, agent: &AgentConfig, inputs: Value, ctx: &NodeContext)
        -> Result<AgentOutput, HarnessError>;
}
// v1: BuiltinHarness（自研）
// v2 候选: CodexAppServerProvider（子进程 JSON-RPC）/ AcpProvider（DSH 等 ACP agent）
```

---

## 4. 冻结的语义裁定（工程项，按评估报告推荐执行）

| 项 | 裁定 |
|---|---|
| 状态/迁移 | `Pending → Ready → Running → {Completed, Crashed, TimedOut, Cancelled}`；`Pending → Skipped`（入边全被 gate 拦截且无其它来源，单条迁移）；WaitingHuman 保留但不产生；迁移矩阵驱动 + 非法迁移返回 `RUN_ILLEGAL_TRANSITION` |
| 实例寻址 | `Delivery.to_instance` 必填；下游实例按 `(node_id)` 惰性创建、单实例语义（v1 不做同一节点多实例并行）；自环 = 新实例 + `attempt+1`，`attempt >= max_attempts`（默认 3，0-based）→ Crashed |
| 数据路由 | **整包广播**：节点 output（`Value`）投递到每条出边的 `to_input_port`；节点自行取字段；DSL 增加 `outputs` 预留（v2） |
| 调度所有权 | 事件循环独占 `RunState`（每 run 一个）；执行任务只持 `Arc` 快照 + `mpsc::Sender` 回传；事件循环线程禁止同步 Python 调用 |
| Python 执行 | 一律 `spawn_blocking`；Python 并发上限独立配置（CPU 型默认 1，I/O 型默认 4，可配 Semaphore） |
| run 启动 | `RunSpec { graph, entry_inputs, user_id, global_vars, options }`；启动时创建 entry 实例 → 写 buffer → **直接就绪判定/派发**（v1.2 已删除 `InputArrived` 事件，见 01 §1.7） |
| run 终止 | 任意 Crashed/TimedOut → Failed（冷峻）；否则无活跃实例（Pending/Ready/Running）→ Completed；等待超过 `deadlock_grace` 仍无活跃但存在输入不可达实例 → Failed(Deadlock) |
| 超时默认 | 实例 180s；单次 LLM 60s（重试 1 次）；watchdog 扫描 Running 超时 + Pending 无唤醒源 |
| 取消 | 事件循环置 `cancel_requested` 原子标志 → 节点自检；LLM 请求 abort（reqwest future drop）；不保证终止已进入的 Python C 调用（文档写明） |
| trace 可靠性 | Transition / GateResult / RunFinished **不可丢**（`send().await` 或背压）；Streaming 可丢（`try_send` + dropped 计数） |

---

## 5. 验收断言（v1 DoD）

1. **示例图跑通**：1.2 客服工单流（修正后的 **6 节点** + 1 gate）端到端返回预期输出。
2. **gate 拦截分支**：`human_handoff` 被 gate 拦截 → 该分支 Skipped、下游不执行、trace 中有 `GateResult{passed:false}`。
3. **trace 可回放**：SQLite 中 `RunStarted → Transition×N → GateResult → Streaming → RunFinished` 完整且 seq 单调；`zgraph trace` 能重建时序。
4. **坏图错误码**：**R1–R14（14 条）校验规则** + 17 个 DSL 码均有反例用例；错误信息包含节点 ID / 边索引 / 期望 vs 实际。
5. **冷峻失败**：Python 节点抛异常 → Crashed → run Failed（不等其它分支）；traceback 入库。
6. **超时不挂死**：节点超时 → TimedOut → run Failed；被放弃任务有计数告警；进程可正常退出（`shutdown_timeout`）。
7. **事件循环不阻塞**：Python 节点/gate 执行期间，5ms ticker 最大间隔 < 50ms；存储不可写时 fail-stop（退出码 4）。
8. **真实 LLM**：`agent_call` 节点用 DeepSeek（或 mock provider）完成一次带 tool 调用的执行，token 流式事件出现在 trace。

---

## 6. 修订后的工作量与里程碑（内核范围）

| 里程碑 | 内容 | 人日 |
|---|---|---|
| P0 | 规格冻结 v1.2 + cargo workspace + `types.rs` + 决策记录 | 1.5–2 |
| P1 | DSL/Loader/R1–R14 共 14 条校验/CompiledGraph/EdgeIndex | 2–3 |
| P2 | 状态机/Buffer/不变式/RunSpec | 2–3 |
| P3 | Scheduler（事件循环/暂停恢复取消/超时/卡死/冷峻失败） | 3.5–4.5 |
| P4 | PubSub+Gate+PyO3 运行时+SymbolResolver+NodeContext | 4–5.5 |
| P5 | Harness + LLM（genai transport）+ tool 循环 + 上下文预算 | 4–5 |
| P6 | trace/SQLite/CLI + 示例图 + e2e + 故障注入 | 2.5–3.5 |
| **合计** | | **19.5–26.5 ≈ 20–27 人日 / 8–10 个会话**（±20%） |

> 对比含前端/API/人工介入的完整产品（24–32 人日后端 + 5–8 前端）：本范围砍掉了 M8 API、M10 人工介入、M12 前端与子图执行，减少约 7–10 人日，且去掉了风险最高的两处语义。

---

## 7. 仍需你确认/提供的 3 件事

1. **LLM 接入**：用哪家？默认按 DeepSeek（`DEEPSEEK_API_KEY` + `https://api.deepseek.com/v1`，模型 `deepseek-chat`）。若用其他 OpenAI-compatible 端点，改配置即可。
2. **子图执行**确认放到 v1.1（v1 只校验定义、运行时报不支持）——若必须进 v1，+1.5–2 人日、+1 个里程碑。
3. **Python 运行环境**：v1 用系统 Python 3.13（Homebrew）+ 项目 venv；若你要求"打包成单二进制/不依赖用户 Python"，需要额外嵌入式 CPython 方案（+2–3 人日）。

---

## 8. 后续产物（2026-10-28 更新）

按"先不写代码"的指示，P0 的**规格冻结**已完成（纯文档）：

- 规格 v1.2 共 9 册 → [`docs/spec-v1.2/README.md`](../spec-v1.2/README.md)
- 覆盖：00 总纲（枚举/错误码/默认值/不变式）、01 领域模型与状态机、02 DSL 与 14 条校验、03 调度器与生命周期、04 节点接口与扩展点、05 Agent Harness、06 存储与可观测、07 验收用例、08 审计关闭矩阵
- 待办：08 由原审计代理独立生成并给出"是否可开工"结论；确认后即可按 P1–P6 里程碑进入编码。

