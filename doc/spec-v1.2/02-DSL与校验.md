# 图模型驱动工作流框架 · 规格冻结 v1.2 · 分册 02：DSL 与校验

> **⚠ v2 落地偏差（已回写）**：本分册的进程内执行/超时/门控/并发/符号解析等假设，已按 [`doc/architecture-v2.md`](../architecture-v2.md) §8 偏差表（D1–D12）**回写为正文**（M0–M2 批次，2026-10-30，见 PROVENANCE §2 第 4 条）；正文与偏差表冲突时以正文为准。
> **状态**：规格冻结（编码前置）。本分册冻结**图 DSL schema、加载期校验规则 R1–R14、错误响应契约、符号解析、graph_hash**；这些内容可直接转成 Loader / `types.rs` / CLI 的实现与测试。
> **取代关系**：本分册细化并修正《详细设计 v1.1》第 1 章（`/tmp/designdoc/doc.txt` L84–L270）与第 6 章符号解析相关部分；冲突时**以本规格为准**，00 分册为本册的上级锚点。v1.1 原文保留作为意图参考与审计依据。
> **范围**：仅内核（见 [`doc/analysis/decision-record-01-scope.md`](../analysis/decision-record-01-scope.md) §2）。子图**执行**、人工介入、Agent 资产平台、API/前端不在本规格内。
> **与 00 的关系**：类型名、枚举名、错误码、默认值一律取自 [`00-总纲.md`](00-总纲.md)，本册**不自创错误码**；00 未覆盖的边界以"复用既有错误码 + message 细分"兜底，并在 §2.11 登记为待裁决项。
> **分册索引**：
> [00 总纲](00-总纲.md) ·
> [01 领域模型与状态机](01-领域模型与状态机.md) ·
> [02 DSL 与校验](02-DSL与校验.md) ·
> [03 调度器与生命周期](03-调度器与生命周期.md) ·
> [04 节点接口与扩展点](04-节点接口与扩展点.md) ·
> [05 Agent Harness](05-AgentHarness.md) ·
> [06 存储与可观测](06-存储与可观测.md) ·
> [07 验收用例](07-验收用例.md) ·
> [08 审计关闭矩阵](08-审计关闭矩阵.md)

---

## 2.1 本册范围与总则

**一句话结论**：本册是 DSL 的**唯一权威 schema + 唯一校验清单**；校验按 R1→R14 顺序执行、跨规则 fail-fast、同规则内聚合；任何错误都不产生 `CompiledGraph`、不产生 run。

### 2.1.1 职责边界

| 内容 | 归属 |
|---|---|
| DSL schema（TS 类型 + Rust serde 契约）、未知字段白名单 | **02（本册）** |
| 校验规则 R1–R14、错误码使用、错误定位、错误响应结构、`zgraph validate` 输出 | **02（本册）** |
| 图级 gate 内置 op 解析、python_node 格式校验的**时机、顺序、错误码**（D7） | **02（本册）**；判定细则见 04 §4.7 |
| `graph_hash` 计算与版本策略 | **02（本册）** |
| `CompiledGraph` 的**构造流程、DSL→产物映射、编译期不变式** | **02（本册）**；结构定义归 01 §1.1.1/§1.1.2（01 提供、02/03/04 只读消费） |
| 运行期状态结构、状态机、Delivery/Publish | 01 / 03 |
| `NodeContext`、Python 约定、内置 start/end handler、Gate 契约、SymbolResolver 实现细节 | 04 |
| AgentConfig 字段、skills/tools 解析、LLM | 05 |
| `entry_inputs` 注入流程与 run 启动时序 | 03（本册只冻结 `RunSpec` 字段契约，见 §2.3.4） |
| trace 落库、CLI 查询/回放 | 06 |

### 2.1.2 加载流水线（三阶段四步）

| 阶段 | 名称 | 规则 | 副作用 | 产物 |
|---|---|---|---|---|
| L1 | **parse**（纯语法/结构） | R1–R4 | 无 | 类型化的 `GraphDefinition` |
| L2 | **graph**（图结构） | R5–R13 | 无 | 结构合法的图 |
| L3 | **resolve**（外部引用） | R14 | **只读**：import Python 模块、读 agent 资产索引（不执行用户函数、不调 LLM） | `CompiledGraph` |
| L4 | **compile**（编译） | 无规则 | 无 | `graph_hash` + 双向边索引（符号表在 L3 产出） |

- **R1–R13 是纯函数**：相同输入字节 → 相同错误序列；可离线（无 Python、无资产目录）通过。
- **R14 触碰外部世界**：`zgraph validate --structure-only` 只跑 R1–R13（明确标注"外部引用未验证"）；默认 `zgraph validate` / `zgraph run` 跑 R1–R14。
- Loader 的三个调用点：CLI `validate`（dry-run）、CLI `run`（启动前）、库 API `load_graph`。三者**必须走同一实现**，不得出现"validate 通过但 run 失败"的差异。

### 2.1.3 失败策略（关闭 M-11 的"聚合/首错"）

| 约定 | 规则 |
|---|---|
| 跨规则 | **fail-fast**：首个失败的规则终止后续规则；规则编号即执行顺序，可测试 |
| 同规则内 | **聚合**：按 `nodes`/`edges` 数组顺序收集全部违反项 |
| 上限 | `MAX_DSL_ERRORS = 20`；超出截断并置 `truncated=true` |
| 自动修复 | **MUST NOT**（图定义错误是设计错误，用户修） |
| 部分加载 | **MUST NOT**：任一错误 → 无 `CompiledGraph`、无 run（`zgraph run` exit 2，不创建 run 记录） |
| 错误顺序 | 稳定：规则序 → 数组序；同一输入产生同一序列 |

### 2.1.4 校验上下文

| 概念 | 定义 |
|---|---|
| `node_id` | 出错节点（或子图 id，见 §2.7.3） |
| `edge_index` | `edges` 数组下标，0-based；与 00 §0.2 `EdgeIdx`（数值别名）一致 |
| `port` | 端口名（`inputs`/`outputs` 中声明的名字） |
| 命名空间 | 主图与每个子图各自独立；节点/端口唯一性在命名空间内判定 |
| assets-dir | R14c 的资产根：CLI `--assets-dir`（缺省 `.`）或 env `ZGRAPH_ASSETS`（见 §2.6.3） |
| Python 环境 | R14b 的 import 环境：进程 `sys.path` / `PYTHONPATH`（04 §4.7） |

---

## 2.2 DSL schema 最终版（schema 版本 `"1.0"`）

**一句话结论**：v1.2 只做**向后兼容的新增（可选/保留字段）**与**两处语义收紧 + 一处放宽**（end 端口约束、gate 符号收窄为内置 op（v2 D6）；`created_at` 改可选），不重命名任何既有字段；`version` 仍为 `"1.0"`（00 §0.1）。

### 2.2.1 v1.2 变更清单

| 字段 / 规则 | 变更类型 | v1.2 内容 | v1 行为 | 关闭审计 |
|---|---|---|---|---|
| `metadata.created_at` | **修订** | 必填 → **可选**（若存在须 UTC RFC3339） | 缺省不报错；由写入方/服务端补齐 | C-25 |
| `GraphMetadata.name` | 明确 | 必填、非空 | 缺失/空 → R3 `DSL_MISSING_FIELD` | C-25 |
| `GraphNode.trigger` | **修订** | 必填 → **可选**，缺省 `"all"` | 缺省按 `all` 编译 | —（易用性） |
| `GraphNode.inputs` | **修订（语义收紧）** | 仍必填；**`end` 必须 ≥1 且 ≥1 个 `required:true` 且 `trigger` 必须 `all`**；`start` 的 `inputs` 即 run 输入契约（0..N，由 `entry_inputs` 注入，04 §4.4.2） | R4 强制 | C-15 / S-17 |
| `GraphNode.outputs` | **新增（保留）** | `OutputPort[]`，可选 | R4 结构校验；**不实现字段级路由**（v1 = 整包广播，决策记录 §4）；v1 无 `from_output_port`，故无一致性可查（§2.6.2） | M-16 / C-19 |
| `GraphEdge.from_output_port` | **新增（保留）** | `string`，可选（仅 schema 声明） | **v1 MUST 缺省**：出现即 R9 `DSL_UNSUPPORTED_FIELD`（00 §0.4）；v2 启用字段级路由（§2.6.2） | M-16 |
| `config.timeout_ms` | **新增（生效）** | `python_node` / `agent_call` 可选，整数 ≥1 | 编译为实例级超时；缺省 `DEFAULT_NODE_TIMEOUT_MS = 180_000`（00 §0.5） | C-01 |
| `config.retry.max_attempts` | **新增（生效）** | 同上，整数 ≥1 | 缺省 `DEFAULT_MAX_ATTEMPTS = 3`（00 §0.5）；仅自环/显式重试计数 | C-26 / S-08 |
| `config.exec_class` | **新增（生效；v2 忽略，D4）** | 仅 `python_node`，枚举 `"cpu" \| "io"` | 保留字段/接口；**v2 每节点一进程，不再区分 CPU/IO 信号量**（并发见 `MAX_WORKERS`/`MAX_WORKERS_PER_RUN`，00 §0.5）；缺省 `io`（`PYTHON_EXEC_CLASS_DEFAULT`） | M-10 / D4 |
| `on_error` | **保留字（不在 schema）** | v2 失败路由 | 任何位置出现 → R4 `DSL_UNSUPPORTED_FIELD`（v2 启用） | C-09 |
| `GateDefinition.function` | **修订（语义收紧，v2 D6）** | 只接受内置 op 短名 `check_output`/`compare`/`manual`；v1.1 的 Python 全限定 `module.function` 写法作废（Python gate 延后，ISSUE-014） | 非内置 op 名 → R14b `DSL_SYMBOL_NOT_FOUND`；`params` 契约不符 → `DSL_SIGNATURE_MISMATCH` | C-22 / D6 |
| `NodeType."subgraph"` | **保留** | 定义 + 校验保留 | 加载期完整校验并通过；**运行期（run 启动后、实例创建前）拒绝**（`NODE_TYPE_UNSUPPORTED`，不创建实例）→ §2.6.1 | C-17 / D1 |
| `version` | 不变 | `"1.0"` | 其他值 → R3 `DSL_PARSE_ERROR` | M-20 |
| 节点/端口 ID 字符集 | **新增约束** | 节点 id：1–64 字符 `[A-Za-z0-9_.-]`；端口名：1–64 字符 `^[A-Za-z_][A-Za-z0-9_]*$` | 违反 → R4 `DSL_PARSE_ERROR` | M-11（可定位性） |
| `type` 未知/保留字符串 | **新增（明确）** | 不属于 5 个枚举的字符串 → 加载期 `DSL_UNSUPPORTED_NODE_TYPE` | 拒绝（fail-fast，message 列出合法类型） | M-11 |

> **未变更（v1.2 明确保留）**：`port.type` 五个取值 `string|number|boolean|object|any`；`AgentCallConfig.input_mapping`；`PythonNodeConfig.module`/`function` 两字段（合并为全限定符号，见 §2.5.1）；`GraphDefinition.subgraphs` 段结构。

### 2.2.2 TypeScript 权威定义

```ts
// ============================================================
// Graph DSL Schema 1.0 · v1.2 最终版（权威）
// 注释中的 R# 指向 §2.4 的校验规则编号
// ============================================================

interface GraphDefinition {
  version: "1.0";                   // R3：必须恰为 "1.0"
  metadata: GraphMetadata;          // R2/R3
  entry_points: string[];           // R2/R7：非空；每项为已声明节点；禁止 end；start 若存在必须列入
  nodes: GraphNode[];               // R2/R4：非空
  edges: GraphEdge[];               // R2/R8–R13：允许为空
  subgraphs?: SubgraphDefinition[]; // R6：v1 只定义+校验；节点引用在运行期 run 启动后、实例创建前被拒绝（§2.6.1）
}

interface GraphMetadata {
  name: string;          // 必填、非空（R3）
  description?: string;  // 可选
  author?: string;       // 可选
  created_at?: string;   // v1.2 修订：可选（v1.1 为必填）；若存在须 UTC RFC3339
}

type NodeType = "agent_call" | "python_node" | "subgraph" | "start" | "end";
                              // 出现其它字符串 → R4 `DSL_UNSUPPORTED_NODE_TYPE`（加载期，保留/未知类型）

interface GraphNode {
  id: string;                 // 节点内/图内唯一（R5）；1–64 字符 [A-Za-z0-9_.-]
  type: NodeType;
  inputs: InputPort[];        // 必填；end 必须 ≥1 且 ≥1 个 required:true；start 可声明（= run 输入契约，04 §4.4.2）
  trigger?: "all" | "any";    // v1.2 修订：可选，缺省 "all"；end 若给出必须 "all"；start 若给出必须 "all"
  config: NodeConfig;         // 按 type 区分；start/end 必须恰好 {}（R4）
  outputs?: OutputPort[];     // v1.2 保留：仅参与 R4 结构校验；v1 不实现字段级路由
  // ⚠ on_error 不是字段：保留字，出现即 R4 → DSL_UNSUPPORTED_FIELD（v2 启用）
}

interface InputPort {
  name: string;              // 节点内唯一、非空、Python 标识符（R4）
  type: PortType;
  required: boolean;         // 必填（不得缺省）
  description?: string;
}

interface OutputPort {       // v1.2 新增（保留，仅声明）
  name: string;              // 节点内唯一、非空
  type: PortType;
  description?: string;
}

type PortType = "string" | "number" | "boolean" | "object" | "any";

type NodeConfig =
  | AgentCallConfig
  | PythonNodeConfig
  | SubgraphConfig
  | StartEndConfig;          // {} —— start/end 唯一合法值

interface CommonNodeConfig { // v1.2 新增（可选、生效）
  timeout_ms?: number;       // 实例级超时；整数 ≥1；缺省 180_000（00 §0.5）
  retry?: RetryConfig;       // 仅自环/显式重试；缺省 3
}

interface RetryConfig { max_attempts?: number }   // 整数 ≥1；缺省 3

interface AgentCallConfig extends CommonNodeConfig {
  agent_id: string;                             // R14c：资产必须存在
  input_mapping?: Record<string, string>;       // key=端口名, value=Agent 输入字段（05 §5.2.2/§5.9.3）
  max_iterations?: number;                      // 1..50；覆盖 AgentConfig（05 §5.2.6/V12）
}

interface PythonNodeConfig extends CommonNodeConfig {
  exec_class?: "cpu" | "io";   // v1.2 新增；缺省 "io"（00 §0.5）
  module: string;    // 点分 Python 模块路径，如 "gates.hr"
  function: string;  // 模块内函数名；全限定符号 = `${module}.${function}`（§2.5.1）
}

interface SubgraphConfig { subgraph_id: string }  // v1：加载期校验通过；运行期 run 启动后、实例创建前拒绝（NODE_TYPE_UNSUPPORTED，04 §4.4.4）

interface StartEndConfig {}         // 必须恰好 {}；on_error → DSL_UNSUPPORTED_FIELD，其它未知键 → DSL_PARSE_ERROR

interface GraphEdge {
  from: string;                     // R8
  to_node: string;                  // R8
  to_input_port: string;            // R9：必须在该节点 inputs 中声明
  gate?: GateDefinition;            // 可选门控
  from_output_port?: string;        // v1.2 保留：v1 MUST 缺省；出现即 R9 → DSL_UNSUPPORTED_FIELD（v2 启用）
}

interface GateDefinition {
  function: string;                 // v2 D6：内置 op 短名 check_output/compare/manual；params 契约见 04 §4.6
  params?: Record<string, unknown>; // 原样进入 GateContext.params（04 §4.6）
}

interface SubgraphDefinition {      // v1：定义保留、仅校验、不执行（§2.6.1）
  id: string;
  nodes: GraphNode[];               // 至少 1 个 type="end"（R6）
  edges: GraphEdge[];
  input_mapping: Record<string, string>;   // 外部端口名 → "<node_id>.<port>"（R6 校验引用）
  output_mapping: Record<string, string>;  // 外部端口名 → "<end_node_id>.<port>"（R6 校验引用）
}
```

### 2.2.3 Rust serde 对照要点

| # | 要点 | 契约写法 | 陷阱 / 实现约定 |
|---|---|---|---|
| S-1 | 节点联合体 | `#[serde(tag = "type", rename_all = "snake_case")] enum NodeDef { Start {..}, End {..}, PythonNode {..}, AgentCall {..}, Subgraph {..} }`；每个变体内联 `id/inputs/trigger/outputs/config` | Rust 变体名与 00 §0.3 `NodeType` 一致（PascalCase）；JSON 用 snake_case |
| S-2 | 未知字段拒绝 | **不依赖** `deny_unknown_fields`（对 internally tagged enum / `flatten` 行为不可靠） | Loader 在 typed 反序列化**之前**用 `serde_json::Value` 做 key 白名单预扫描（§2.2.4）；**保留字段**（`on_error`/`from_output_port`）→ `DSL_UNSUPPORTED_FIELD`，其余未列入白名单的键 → `DSL_PARSE_ERROR` |
| S-3 | 必填字段 | 预扫描检查 required-key 集合（§2.2.4） | 缺键 → `DSL_MISSING_FIELD`；typed 反序列化的失败一律 → `DSL_PARSE_ERROR`（避免 serde 的 `missing field` 落进错误桶） |
| S-4 | `version` | `String`，手工比较 `== "1.0"` | 不引入 `semver` crate；不接受数字 `1.0`；`1.1`/`1` 一律 `DSL_PARSE_ERROR` |
| S-5 | 枚举大小写 | `#[serde(rename_all = "snake_case")]`：`PortType::{String,Number,Boolean,Object,Any}`、`Trigger::{All,Any}` | `any` 既是 `PortType` 又是 `Trigger` 的取值，分属两个枚举 |
| S-6 | 缺省值 | `#[serde(default)] trigger: Trigger`（`Default = All`）；`timeout_ms: Option<u64>`、`retry: Option<RetryConfig>` | **DSL 侧记录"作者是否写了"**；语义默认值在编译期补齐到 `CompiledNode`（不写回 DSL、不参与 hash，§2.8.3） |
| S-7 | `inputs` | `Vec<InputPort>`，**无** `#[serde(default)]` | 缺失必须在预扫描阶段报 `DSL_MISSING_FIELD`，不能靠 serde 报错 |
| S-8 | start/end 的 `config` | `#[derive(Deserialize)] #[serde(deny_unknown_fields)] struct StartEndConfig {}` | 空结构体 + `deny_unknown_fields` 在非 flatten 场景可靠；`config` 必须有且为 `{}` |
| S-9 | 整数 | `timeout_ms: Option<u64>`、`max_attempts: Option<u32>` | 负数/浮点/字符串 → `DSL_PARSE_ERROR`；越界（0）→ R4 `DSL_PARSE_ERROR`（`expected: ">= 1"`） |
| S-10 | 开放 Map（豁免扫描） | `gate.params`、`input_mapping`、`subgraph.*_mapping` 是 `serde_json::Map`/`BTreeMap` | 这些位置的键**不**参与未知字段预扫描（用户数据） |
| S-11 | 子图递归 | `Option<Vec<SubgraphDefinition>>`，节点/边复用同一 `NodeDef`/`EdgeDef` | 校验递归时命名空间切换（§2.1.4）；错误定位前缀 `subgraphs[i]` |
| S-12 | 编译产物 | `CompiledGraph` 全部字段 `pub` + `Arc` 共享（结构见 01 §1.1.1；构造见 §2.9） | `EdgeRef(EdgeIdx)`（00 §0.2/§0.7）：编译产物只存 `usize`，不存引用 |

### 2.2.4 未知字段与必填键白名单（R4 的判定依据）

> 除"开放 Map"与"保留字段"外，**任何未列出的键 → `DSL_PARSE_ERROR`**（`expected` = 允许键列表，`actual` = 实际键）。保留字段（`on_error`、`from_output_port`）走 `DSL_UNSUPPORTED_FIELD`（见下）。

| 对象 | 允许键 | 必填键 |
|---|---|---|
| 顶层 `GraphDefinition` | `version, metadata, entry_points, nodes, edges, subgraphs` | `version, metadata, entry_points, nodes, edges` |
| `metadata` | `name, description, author, created_at` | `name` |
| `GraphNode`（全类型） | `id, type, inputs, trigger, config, outputs`；`on_error` 为**保留字**（出现即 `DSL_UNSUPPORTED_FIELD`） | `id, type, inputs, config` |
| `InputPort` | `name, type, required, description` | `name, type, required` |
| `OutputPort` | `name, type, description` | `name, type` |
| `AgentCallConfig` | `agent_id, input_mapping, max_iterations, timeout_ms, retry` | `agent_id` |
| `PythonNodeConfig` | `module, function, exec_class, timeout_ms, retry` | `module, function` |
| `SubgraphConfig` | `subgraph_id` | `subgraph_id` |
| `StartEndConfig` | *（无）* | —（必须恰好 `{}`） |
| `RetryConfig` | `max_attempts` | — |
| `GraphEdge` | `from, to_node, to_input_port, gate`；`from_output_port` 为**保留字段**（出现即 `DSL_UNSUPPORTED_FIELD`） | `from, to_node, to_input_port` |
| `GateDefinition` | `function, params`（`params` 内部开放） | `function` |
| `SubgraphDefinition` | `id, nodes, edges, input_mapping, output_mapping` | 全部 |

**例外（开放 Map，键任意）**：`gate.params`、`AgentCallConfig.input_mapping`、`SubgraphDefinition.input_mapping` / `output_mapping`。

**节点 `type` 取值**：白名单 = `start`/`end`/`agent_call`/`python_node`/`subgraph`；其它字符串 → R4 `DSL_UNSUPPORTED_NODE_TYPE`（不是 `DSL_PARSE_ERROR`，见 §2.6.1）。

**保留字段 / 保留字清单（v1 出现即报错 → `DSL_UNSUPPORTED_FIELD`）**：节点级 `on_error`（任何节点，任何层级）、边级 `from_output_port`（`GraphEdge` 上出现即报错，不看取值）。message 必须含 `reserved field '{field}' is not supported by schema 1.0 (planned for v2)`；`expected` = `"<absent>"`，`actual` = 字段名+值。其余未列入白名单的键仍 → `DSL_PARSE_ERROR`（拼写错误桶）。

---

## 2.3 权威示例（修正 v1.1 §1.2）

**一句话结论**：v1.1 示例因"`end_success` 的 `inputs` 为空却接收 `result` 端口"而自我违反规则 6（审计 C-15）；本示例**显式声明 `end` 的输入端口**，并通过 R1–R14（R14 依赖 07 分册 fixture）。

### 2.3.1 修正后的完整示例（可直接 `zgraph validate`）

```json
{
  "version": "1.0",
  "metadata": {
    "name": "客服工单处理流",
    "description": "用户提问 → 知识库检索 → 生成回答 → 判断是否需要人工介入",
    "author": "demo"
  },
  "entry_points": ["input_parser"],
  "nodes": [
    {
      "id": "input_parser",
      "type": "agent_call",
      "inputs": [
        { "name": "user_message", "type": "string", "required": true }
      ],
      "trigger": "all",
      "config": { "agent_id": "message_parser", "timeout_ms": 60000 }
    },
    {
      "id": "kb_search",
      "type": "agent_call",
      "inputs": [
        { "name": "query", "type": "string", "required": true }
      ],
      "trigger": "all",
      "config": { "agent_id": "kb_searcher" }
    },
    {
      "id": "answer_generator",
      "type": "agent_call",
      "inputs": [
        { "name": "user_query", "type": "string", "required": true },
        { "name": "kb_result", "type": "object", "required": true }
      ],
      "trigger": "all",
      "config": { "agent_id": "answer_writer" }
    },
    {
      "id": "human_escalation_check",
      "type": "python_node",
      "inputs": [
        { "name": "answer", "type": "object", "required": true }
      ],
      "trigger": "all",
      "config": { "module": "gates.hr", "function": "need_human_check" }
    },
    {
      "id": "human_handoff",
      "type": "agent_call",
      "inputs": [
        { "name": "context", "type": "object", "required": true }
      ],
      "trigger": "all",
      "config": { "agent_id": "human_handoff_agent" }
    },
    {
      "id": "end_success",
      "type": "end",
      "inputs": [
        { "name": "result", "type": "any", "required": true }
      ],
      "trigger": "all",
      "config": {}
    }
  ],
  "edges": [
    { "from": "input_parser", "to_node": "kb_search", "to_input_port": "query" },
    { "from": "kb_search", "to_node": "answer_generator", "to_input_port": "kb_result" },
    { "from": "input_parser", "to_node": "answer_generator", "to_input_port": "user_query" },
    { "from": "answer_generator", "to_node": "human_escalation_check", "to_input_port": "answer" },
    {
      "from": "human_escalation_check",
      "to_node": "human_handoff",
      "to_input_port": "context",
      "gate": { "function": "check_output", "params": { "path": "need_human", "equals": true } }
    },
    { "from": "answer_generator", "to_node": "end_success", "to_input_port": "result" }
  ]
}
```

### 2.3.2 与 v1.1 示例的差异（每条对应一个审计/裁定）

| # | v1.1 | v1.2 | 依据 |
|---|---|---|---|
| 1 | `end_success.inputs: []`，却有边指向 `result` | `inputs: [{ "name": "result", "type": "any", "required": true }]` | C-15；R4 + R12 |
| 2 | `metadata` 无 `created_at`（与 v1.1 必填矛盾） | 仍缺省，合法（字段改可选） | C-25；R3 |
| 3 | gate `"function": "gate_need_human"`（Python 短名） | `"check_output"`（内置 op，D6；`params` 见示例） | C-22 / D6；R14b |
| 4 | 无任何超时声明 | `input_parser.config.timeout_ms=60000`（示例演示新字段；`retry` 只在自环时有意义（v1 无显式重试），本图为 DAG 故不演示，见 §2.2.1） | C-01 |
| 5 | 6 条边，`end_success` 为唯一 end | 不变（多 end 也合法；见 R4） | R4/R12 |
| 6 | 无 start 节点 | 不引入（start 可选，§2.6.4） | §2.6.4 |

### 2.3.3 示例自查表（逐规则）

| 规则 | 结论 | 证据 |
|---|---|---|
| R1 | ✅ | 合法 JSON object（代码块可直接保存为文件） |
| R2 | ✅ | `version/metadata/entry_points/nodes/edges` 齐备；`nodes` 6 项、`entry_points` 1 项非空；无未知顶层键 |
| R3 | ✅ | `version == "1.0"`；`metadata.name` 非空；`created_at` 缺省（合法） |
| R4 | ✅ | 6 个节点结构合法；`end_success.inputs` 非空且含 `required:true`；`end.trigger="all"`；`config` 键均在白名单（`agent_id/input_mapping/max_iterations/timeout_ms/retry/module/function/exec_class`）；无 `on_error` |
| R5 | ✅ | 6 个 id 互异 |
| R6 | ✅ | 无 `subgraphs` 段（未使用，规则不适用） |
| R7 | ✅ | `entry_points=["input_parser"]` 存在、非 end、无重复 |
| R8 | ✅ | 端点均存在；end 无出边、无 start、entry 无入边 |
| R9 | ✅ | 6 条边 `to_input_port`（`query`/`kb_result`/`user_query`/`answer`/`context`/`result`）均在目标节点 `inputs` 中声明；`from_output_port` 缺省 |
| R10 | ✅ | `(from,to_node)` 6 组互异（`input_parser→kb_search`、`kb_search→answer_generator`、`input_parser→answer_generator`、`answer_generator→human_escalation_check`、`human_escalation_check→human_handoff`、`answer_generator→end_success`） |
| R11 | ✅ | `(to_node,to_input_port)` 6 组互异 |
| R12 | ✅ | 全部非 entry 节点的 `required` 端口均有入边；`end_success.result` 有来源 |
| R13 | ✅ | 门控子图为 DAG（无环；无自环） |
| R14 | ✅（需 fixture） | gate 内置 op `check_output`（D6）、python_node `gates.hr.need_human_check` 与 4 个 agent 资产 id（`message_parser`/`kb_searcher`/`answer_writer`/`human_handoff_agent`）由 07 分册 e2e fixture 提供（AC-01/AC-04） |

> 注：`human_handoff` 是**叶节点**（无出边）——合法；run 结束以"无活跃实例"判定（00 §0.7 / 03 §3.9），不要求每个分支都汇入 end。

### 2.3.4 示例与 run 启动的衔接（`RunSpec` 契约）

`entry_points` 中**非 start** 节点的 `required` 输入来自 run 请求，而不是边（R12 对 entry 节点豁免）。契约（S-01 关闭；实现见 03）：

> **结构以 03 §3.2 的 `RunSpec` 为准**（`graph: Arc<CompiledGraph>`、`entry_inputs`、`user_id`、`global_vars`、`options`）；本册只冻结与本册相关的契约：`entry_inputs: HashMap<NodeId, HashMap<PortName, Value>>` MUST 覆盖每个 entry 节点的全部 `required` 端口；`graph` 即加载产物（run 与 `graph_hash` 的绑定由 03 记录）。

| 约定 | 规则 |
|---|---|
| 完整性 | 启动前校验 `entry_inputs` 覆盖 entry 节点全部 `required` 端口；缺失属 **run 启动错误**：`RUN_ENTRY_INPUT_MISSING`；多余节点/端口 → `RUN_ENTRY_INVALID`（00 §0.4 运行期域 / 03 §3.1），**不产生 run** |
| 多余项 | `entry_inputs` 中指向非 entry 节点的键 → 运行期告警（不阻塞） |
| start 节点 | 其 `inputs` 声明即 run 输入契约：由 `entry_inputs[start]` 直接注入（不经 buffer 就绪判定）；handler 输出 = 注入 map（未注入则为 `{}`），04 §4.4.2 |

---

## 2.4 校验规则 R1–R14（最终版）

**一句话结论**：14 条规则按序执行，规则编号即契约；`(from,to_node)` 与 `(to_node,to_input_port)` 是**两级独立唯一性**（分别由 R10/R11 关闭 C-04/C-05），`created_at` 可选（R3 关闭 C-25），示例合法性由 §2.3 关闭 C-15。

### 2.4.1 规则总表（执行顺序 = 编号）

| # | 规则 | 错误码（00 §0.4） | 判定算法（可伪代码） | 定位字段 |
|---|---|---|---|---|
| **R1** | JSON 可解析 | `DSL_PARSE_ERROR` | `serde_json::from_slice::<Value>(bytes)` 成功 **且** 顶层为 object | `{}`（message 带 line/column） |
| **R2** | 顶层必填/类型/未知键 | `DSL_MISSING_FIELD` / `DSL_PARSE_ERROR` | 预扫描：`top` 含 `{version,metadata,entry_points,nodes,edges}`（缺→MISSING）；`entry_points`/`nodes` 为非空数组、`metadata` object、`edges` 数组；顶层键 ⊆ 白名单（§2.2.4） | `{}` |
| **R3** | version 与 metadata | `DSL_PARSE_ERROR` / `DSL_MISSING_FIELD` | `version == "1.0"`；`metadata.name` 非空；`created_at?` 若存在 → UTC RFC3339；`description/author?` string；未知键拒绝 | `{}`（缺 name 时 `{}`） |
| **R4** | 节点声明合法 | `DSL_MISSING_FIELD` / `DSL_INVALID_PORT` / `DSL_UNSUPPORTED_NODE_TYPE` / `DSL_UNSUPPORTED_FIELD` / `DSL_PARSE_ERROR` | 逐节点（数组序）：键白名单；`id` 格式；`type` ∈ 5 枚举（其它字符串 → `DSL_UNSUPPORTED_NODE_TYPE`）；`inputs` 端口结构（名唯一/非空/Python 标识符、`type` 枚举、`required` bool）；`trigger` 枚举；`outputs` 若存在 → 端口结构（节点内唯一）；`config` 按 `type` 白名单与必填；`start` 的 `inputs` 允许 0..N（结构同普通节点）且 `start.trigger` 若给出必须 `"all"`；`end.inputs ≥1 且 ≥1 required` 且 `end.trigger` 必须 `"all"`；`on_error` 出现 → `DSL_UNSUPPORTED_FIELD` | `node_id`（+`port`） |
| **R5** | 节点 ID 唯一 | `DSL_DUPLICATE_NODE_ID` | 命名空间内（主图 / 每个子图）`Set<id>` 检查 | `node_id`（message 带两个下标） |
| **R6** | 子图定义与引用 | `DSL_SUBGRAPH_INVALID` | `subgraphs?`：`id` 图内唯一；每项键/必填齐全；`nodes ≥1`；**至少 1 个 `type="end"`**（不要求恰好 1 个）、**不得含 `start`**；递归执行 R4/R5/R8–R13（命名空间=子图；**R7 不适用**——子图无 `entry_points`；R12 的 entry 豁免节点 = `input_mapping` 值中的内部节点）；`input_mapping` 值形如 `"<node_id>.<port>"` 且节点/端口存在；`output_mapping` 值指向 **end 节点已声明端口**；**若存在 `type="subgraph"` 节点，其 `config.subgraph_id` 必须命中某个子图定义** | `node_id`=子图 id / 引用方节点（message 前缀 `subgraphs[i]`） |
| **R7** | entry_points 有效 | `DSL_INVALID_ENTRY` | 每项 ∈ 节点集；非 `end`；无重复；每个 `start` 节点必须被列入（允许多个 start，04 §4.4.2） | `node_id`（= 违规入口/`start` 节点） |
| **R8** | 边端点合法 | `DSL_INVALID_EDGE_REF` | 逐边（数组序）：`from`/`to_node` 存在；`from.type != end`；`to_node.type != start`；`to_node ∉ entry_points`（入口节点输入只能外部注入） | `edge_index` + `node_id` |
| **R9** | 端口引用合法 | `DSL_INVALID_PORT` / `DSL_UNSUPPORTED_FIELD` / `DSL_PARSE_ERROR` | `to_input_port` ∈ `to_node.inputs[*].name`；**`from_output_port` 出现即 `DSL_UNSUPPORTED_FIELD`**（v1 MUST 缺省；v2 启用时改为"∈ from.outputs 的已声明端口"）；`gate.params?` 必须是 object | `edge_index` + `node_id` + `port` |
| **R10** | `(from, to_node)` 唯一 | `DSL_DUPLICATE_EDGE` | 逐边插入 `Set<(from,to_node)>`；命中 → 报第二次出现 | `edge_index`（= 重复边）+ `node_id`（= `from`） |
| **R11** | `(to_node, to_input_port)` 唯一 | `DSL_DUPLICATE_PORT_SOURCE` | 逐边插入 `Set<(to_node,to_input_port)>`；命中 → 报第二次出现 | `edge_index` + `node_id` + `port` |
| **R12** | 输入来源完整 | `DSL_UNCONNECTED_INPUT` | 对每个 **非 entry** 节点：入边数 ≥1；`trigger=all` → 每个 `required:true` 端口有入边；`trigger=any` → ≥1 个 `required` 端口有入边（无 required 端口时 → ≥1 个端口有入边）。豁免：主图 `entry_points` 节点（含 start）、子图 `input_mapping` 值指向的内部节点 | `node_id`（+`port`） |
| **R13** | 无门控环检测 | `DSL_CYCLE_DETECTED` | 构造 `G'` = 顶点为节点、边为**无 gate 的边**（**自环计入**）；对 `G'` 做 DFS 三色标记（顶点按 `nodes` 序、邻接按 `edges` 序）→ 发现回边即环 | `edge_index`（环内最小索引边）+ `node_id`（环起点） |
| **R14** | 外部引用解析 | `DSL_SYMBOL_NOT_FOUND` / `DSL_SIGNATURE_MISMATCH` / `DSL_SYMBOL_CONFLICT` / `DSL_AGENT_NOT_FOUND` | (a) **gate**：格式 → 内置 op 查表 → `params` 契约 → 跨类冲突（§2.5；D6/D7）；**python_node 只做 `module/function` 格式校验，不 import**（导入/签名在 worker 启动期 → `NODE_WORKER_IMPORT`）；(b) `agent_call.agent_id` 在资产索引中存在（未提供资产根时跳过，§2.6.3）。**`subgraph` 节点不在此拒绝**（加载期通过，运行期 run 启动后、实例创建前报 `NODE_TYPE_UNSUPPORTED`，§2.6.1） | `edge_index`（gate）/ `node_id`（资产） |

### 2.4.2 错误信息模板与定位（message 为英文稳定模板，M-11）

> `message` 是**模板化英文**（code 稳定、message 可读、参数化），不得用于逻辑判断；`expected`/`actual` 为机器可读 JSON 值。

| 错误码 | `message` 模板 | `location` 组成 | CLI 定位串 |
|---|---|---|---|
| `DSL_PARSE_ERROR` | `invalid JSON at line {l}, column {c}: {detail}`；`unknown field '{key}' in {where}`；`field '{field}' expects {expected_type}, got {actual_type}`；`'{field}' must be {constraint}, got {actual}` | `{}`（解析）或 `node_id`/`edge_index`（未知字段） | `graph.json` 或 `graph.json: edges[i]` |
| `DSL_MISSING_FIELD` | `missing required field '{field}' in {where}` | `{}` 或 `node_id` | `graph.json` / `graph.json: node 'X'` |
| `DSL_DUPLICATE_NODE_ID` | `duplicate node id '{node_id}': nodes[{first}] and nodes[{dup}]` | `node_id` | `graph.json: node 'X'` |
| `DSL_INVALID_ENTRY` | `entry point '{value}' is not valid: {reason}`（`unknown_node`/`end_node`/`duplicate`/`start_not_listed`） | `node_id` | `graph.json: entry_points[...]` |
| `DSL_INVALID_EDGE_REF` | `edge[{i}] invalid endpoint: {reason}`（`unknown_from`/`unknown_to`/`end_as_source`/`start_as_target`/`entry_as_target`） | `edge_index` + `node_id` | `graph.json: edges[i].from` / `.to_node` |
| `DSL_INVALID_PORT` | `edge[{i}].to_input_port '{port}' is not declared on node '{node_id}'`（v2 启用 `from_output_port` 后亦负责其引用校验） | `edge_index` + `node_id` + `port` | `graph.json: edges[i].to_input_port` |
| `DSL_UNSUPPORTED_FIELD` | `reserved field '{field}' is not supported by schema 1.0 (planned for v2)` | 节点级：`node_id`；边级：`edge_index` | `graph.json: node 'X'` / `graph.json: edges[i]` |
| `DSL_DUPLICATE_EDGE` | `duplicate edge {from} -> {to_node}: edges[{first}] and edges[{dup}]` | `edge_index` + `node_id` | `graph.json: edges[i]` |
| `DSL_DUPLICATE_PORT_SOURCE` | `duplicate input source for '{node_id}.{port}': edges[{first}] and edges[{dup}]` | `edge_index` + `node_id` + `port` | `graph.json: edges[i]` |
| `DSL_UNCONNECTED_INPUT` | `input '{node_id}.{port}' has no incoming edge`；`node '{node_id}' has no incoming edge` | `node_id`（+`port`） | `graph.json: node 'X'` |
| `DSL_SUBGRAPH_INVALID` | `subgraph '{subgraph_id}': {detail}`（`no_end`/`duplicate_id`/`bad_mapping_ref`/`duplicate_node_id`/`cycle`） | `node_id`=子图 id | `graph.json: subgraphs[i]` |
| `DSL_CYCLE_DETECTED` | `ungated data cycle: {n1} -> {n2} -> ... -> {n1} (edges: [{indices}])` | `edge_index` + `node_id` | `graph.json: edges[i]` |
| `DSL_SYMBOL_NOT_FOUND` | `symbol '{symbol}' not found: {detail}`（`bad_format`/`unknown_builtin_op`） | `node_id`（python_node 格式非法）或 `edge_index`（gate） | `graph.json: node 'X'.config` / `graph.json: edges[i].gate` |
| `DSL_SIGNATURE_MISMATCH` | `signature mismatch for '{symbol}': expected {expected}, actual {actual}`（仅内置 gate op 的 `params` 契约） | `edge_index`（gate） | `graph.json: edges[i].gate` |
| `DSL_SYMBOL_CONFLICT` | `symbol conflict for '{symbol}': {kind_a} and {kind_b} share the same name`（`builtin.<op>` vs python_node） | `edge_index` 或 `node_id`（后出现者） | `graph.json: edges[i].gate` / `node 'X'.config` |
| `DSL_AGENT_NOT_FOUND` | `agent asset '{agent_id}' not found in index '{assets_dir}/agents'`；`duplicate agent asset id '{agent_id}'` | `node_id` | `graph.json: node 'X'.config.agent_id` |
| `DSL_UNSUPPORTED_NODE_TYPE` | `node type '{type}' is not a known node type; allowed: [start, end, agent_call, python_node, subgraph]`（`subgraph` 的运行期拒绝用 `NODE_TYPE_UNSUPPORTED`，§2.6.1） | `node_id` | `graph.json: node 'X'` |

`expected` / `actual` 约定：

| 场景 | `expected` | `actual` |
|---|---|---|
| 未知/缺失/保留字段 | 允许键数组 / 字段名 | 实际键 / 缺失标记 |
| 端口不存在 | 目标节点全部端口名数组 | 引用的端口名 |
| 重复项 | 首次出现的下标 | 重复出现的下标 |
| 枚举/格式 | 允许值数组或格式字符串（`"module.function"`） | 实际值 |
| 环检测 | `"acyclic"` | 环路径数组（node id 序列） |
| 签名 | 对象 `{"symbol":"…","kind":"gate","expected":<op 的 params 契约>}` | 对象 `{"symbol":"…","kind":"gate","actual":<实际 params>}` |
| 符号 / 资产 | 对象 `{"symbol":"…","kind":"…"}` / 资产 id | 同结构；失败原因写在 `message` |

### 2.4.3 正反例（每条规则 1 正 1 反）

| # | 正例（通过） | 反例（报错 + 定位） |
|---|---|---|
| R1 | `{"version":"1.0","metadata":{"name":"g"},"entry_points":["n"],"nodes":[{"id":"n","type":"start","inputs":[],"config":{}}],"edges":[]}` | 尾逗号 JSON → `DSL_PARSE_ERROR`，`location:{}`，message 含 `line:column` |
| R2 | 同上（最小合法图） | 缺 `edges` → `DSL_MISSING_FIELD`，`field:"edges"`；或 `"nodes":[]` → `DSL_PARSE_ERROR`（`expected:"at least 1 item"`） |
| R3 | `version:"1.0"`、`metadata.name:"g"`、`created_at:"2026-10-28T09:00:00Z"` | `version:"1.1"` → `DSL_PARSE_ERROR`（`expected:"1.0"`）；`metadata:{}` → `DSL_MISSING_FIELD`（`name`） |
| R4 | `end` 节点 `inputs:[{name:"result",type:"any",required:true}]`、`trigger:"all"`、`config:{}` | `end.inputs:[]` → `DSL_INVALID_PORT`（`node_id:"end_success"`，`expected:"non-empty inputs"`）；`{"on_error":"..."}` → `DSL_UNSUPPORTED_FIELD`；`"type":"human_node"` → `DSL_UNSUPPORTED_NODE_TYPE` |
| R5 | `nodes:[{"id":"a"},{"id":"b"}]` | 两个 `"id":"a"` → `DSL_DUPLICATE_NODE_ID`（`node_id:"a"`） |
| R6 | 子图含 1 个 end、`output_mapping:{"out":"end_success.result"}` | 子图无 end → `DSL_SUBGRAPH_INVALID`（`node_id:"<subgraph_id>"`）；`output_mapping` 指向未声明端口 → 同码 |
| R7 | `entry_points:["input_parser"]` | `entry_points:["ghost"]` → `DSL_INVALID_ENTRY`（`node_id:"ghost"`）；把 end 列入 → 同码 |
| R8 | `input_parser → kb_search.query` | 新增第 7 条边 `end_success → x` → `DSL_INVALID_EDGE_REF`（`edge_index:6`，`reason:"end_as_source"`）；边指向 entry 节点 → `entry_as_target` |
| R9 | `to_input_port:"query"`（已声明）；`from_output_port` 缺省 | `to_input_port:"query_text"` → `DSL_INVALID_PORT`（`edge_index:0`、`node_id:"kb_search"`、`port:"query_text"`）；出现 `from_output_port` → `DSL_UNSUPPORTED_FIELD` |
| R10 | `input_parser→kb_search`、`input_parser→answer_generator` | 同一 `(input_parser,kb_search)` 两条边 → `DSL_DUPLICATE_EDGE`（`edge_index:1`） |
| R11 | `input_parser→answer_generator.user_query`、`kb_search→answer_generator.kb_result` | `input_parser→kb_search.query` + `answer_generator→kb_search.query` → `DSL_DUPLICATE_PORT_SOURCE`（`node_id:"kb_search"`、`port:"query"`、`edge_index:后者`） |
| R12 | 全部非 entry 的 required 端口均有入边 | `human_handoff`（非 entry）的 `context` 边删除 → `DSL_UNCONNECTED_INPUT`（`node_id:"human_handoff"`、`port:"context"`） |
| R13 | 示例 DAG；或带 gate 的自环 `a → a` | 无 gate 的 `a→b→a` → `DSL_CYCLE_DETECTED`（`edge_index:` 环内最小边）；**无 gate 的自环 `a→a` 同样报错** |
| R14 | gate 写内置 op 短名 `"check_output"`、python `"gates.hr.need_human_check"`（格式合法即可，不 import）、资产齐备 | gate 写 Python 风格名 `"gate_need_human"` → `DSL_SYMBOL_NOT_FOUND`（`edge_index`，`expected:{"symbol":…,"kind":"gate"}`）；`agent_id:"ghost_agent"` → `DSL_AGENT_NOT_FOUND`；`type:"subgraph"` **加载期通过**，`zgraph run` 时 run 启动后、实例创建前 `NODE_TYPE_UNSUPPORTED`（§2.6.1）；python_node 模块不存在 **加载期通过**，worker 启动时报 `NODE_WORKER_IMPORT`（运行期） |

### 2.4.4 本册对审计条目的裁定（D-02-xx，可编码）

| 编号 | 裁定 | 关闭 |
|---|---|---|
| D-02-01 | **两级唯一性都强制**：R10 `(from,to_node)` → `DSL_DUPLICATE_EDGE`；R11 `(to_node,to_input_port)` → `DSL_DUPLICATE_PORT_SOURCE`。v1 **禁止 fan-in**（多源同端口）；`EdgeIndex.incoming` 保持单值 `HashMap<(NodeId,PortName), EdgeIdx>`（§2.9.1 / 01 §1.1.2），v2 再引入 reducer/合并语义 | C-04 / C-05 |
| D-02-02 | `GraphMetadata.created_at` **可选**；存在则 UTC RFC3339；不参与 graph_hash。`metadata.name` 必填非空 | C-25 |
| D-02-03 | v1.1 示例按 §2.3.1 修正：`end` 必须声明 **≥1 个 `required:true` 输入端口**，且每个 required 端口必须有入边（R4 + R12）。不引入"end/start 豁免端口校验"的特例 | C-15 |
| D-02-04 | `end`：无出边（R8）、≥1 required 输入且有来源（R4/R12）、`trigger` 必须 `all`；数量 0..N（多 end 合法，用于分支收口）。`start`：无入边（R8）、必须在 `entry_points` 中、可多个、`inputs`（0..N）由 `entry_inputs` 注入且 handler 整包透传（04 §4.4.2）；`start` 可选（`entry_points` 是入口唯一真源） | S-01 / S-17 |
| D-02-05 | **自环不豁免环检测**：无 gate 的自环 = 无门控纯数据环 → R13 `DSL_CYCLE_DETECTED`；带 gate 的自环合法，次数上限由 `retry.max_attempts`（缺省 3）与 03 的 attempt 语义共同约束 | C-26 |
| D-02-06 | **未知字段一律拒绝**（§2.2.4 白名单）：保留字段（`on_error`、`from_output_port`）→ `DSL_UNSUPPORTED_FIELD`；其它未知键 → `DSL_PARSE_ERROR`。与 00 §0.4 的 `DSL_UNSUPPORTED_FIELD` 行一致（§2.11 P-02-01） | C-09 / M-11 |
| D-02-07 | **子图节点：加载期通过、运行期（run 启动后、实例创建前）拒绝**。加载期完整校验（R6 + `subgraph_id` 引用存在），不视为错误；run 启动成功后、创建实例前检测 `NodeType::Subgraph`：不创建实例、不构造 handler、run 立即 Failed、错误码 `NODE_TYPE_UNSUPPORTED`（00 运行期域；ISSUE-046），其余非终态转 Cancelled（04 §4.4.4 / 00 I9）。理由：保留 DSL 定义与校验能力（v1.1 直接启用）+ 决策记录 §2.2 + 00 运行期码不落空；代价：`validate` 通过但 run 必失败，属显式语义（"能加载、不能跑"）。`DSL_UNSUPPORTED_NODE_TYPE` 改用于加载期**未知/保留 `type` 字符串**（与 04 A5 一致） | C-17 / M-11 |
| D-02-08 | 入口节点（`entry_points`）**不得有入边**（R8）：输入来源唯一（`RunSpec.entry_inputs`），消除双重来源歧义 | S-01 |
| D-02-09 | gate 内置 op 在**图加载期**解析并校验 `params`（R14b）；python_node 只校验格式、worker 启动期解析（D7）；同名符号跨类（gate/python_node）→ `DSL_SYMBOL_CONFLICT` | C-20 / C-22 / S-18 |
| D-02-10 | `graph_hash` 只对**通过 R1–R13 的 DSL 内容**做白名单投影计算（§2.8.2）；不补齐默认值；不含 `description/author/created_at` | M-20 |

---

## 2.5 符号解析规格（关闭 C-20 / C-21 / C-22 / S-18）

**一句话结论**：`python_node` 的符号是**全限定 `module.function`**（必须含点）；**v2（D6/D7）**下 `gate.function` 只接受**内置 op 短名**（`check_output`/`compare`/`manual`，加载期完成名称/`params` 契约校验，失败为 `DSL_SYMBOL_*`），且**图加载期不 import 用户 Python**——`python_node` 在加载期只校验 `module/function` **格式**，存在性与签名在 **worker 启动时**校验（失败为运行期 `NODE_WORKER_IMPORT`）。

### 2.5.1 命名（唯一形式）

| 用法 | DSL 字段 | 符号（唯一键） | 格式规则 | 示例 |
|---|---|---|---|---|
| python_node | `config.module` + `config.function` | `${module}.${function}` | `module` 为点分 Python 标识符（`^[A-Za-z_]\w*(\.[A-Za-z_]\w*)*$`），`function` 为标识符；合并名必须含 ≥1 点（自动满足） | `gates.hr` + `need_human_check` → `gates.hr.need_human_check` |
| gate | `gate.function`（**单字段**） | 值本身 | **v2（D6）只接受 3 个内置 op 短名**：`check_output` / `compare` / `manual`（不含点；Python gate 延后，ISSUE-014）。canonical 符号 `builtin.<op>` 只用于跨类冲突检测 | `check_output` |

| 作废写法（v1.1） | v1.2 处理 |
|---|---|
| gate 写 `"gate_need_human"` | R14b `DSL_SYMBOL_NOT_FOUND`，`expected:["check_output","compare","manual"]` |
| 解析器自动拼前缀 `gates.`（v1.1 §6.3） | **删除**：加载期只接受内置 op canonical 名（`builtin.<op>`），不再拼接/import |
| `gates.{name}` 中间写法 | **v2 作废（D6/D7）**：Python gate 延后（ISSUE-014），`gate.function` 只接受内置 op（`check_output`/`compare`/`manual`，canonical `builtin.<op>`） |

> **D7 回写（ISSUE-026/050）**：`python_node` 的符号串仍按上表格式校验（全限定名、合法标识符），但**图加载期不 import**。精确边界：`DSL_SYMBOL_NOT_FOUND` 在 v2 仍会因 python_node 的 `module/function` **格式非法**在加载期报出（`bad_format`，定位 `node_id`）；`DSL_SIGNATURE_MISMATCH` / `DSL_SYMBOL_CONFLICT` 只由**内置 gate op 的 `params` 契约/跨类同名**触发。Python 节点的 **import/签名**失败是**运行期**错误：worker 启动时校验失败 → `NODE_WORKER_IMPORT`（实例 Crashed）。

- `GateDefinition.function` 字段名**不改**（向后兼容）；v1.2 只收紧其取值语义（00 §0.1 的"修订"见 §2.11 P-02-04）。
- 符号在**同一张图内**是唯一键：同一符号出现多次 → 只解析/登记一次（幂等）。

### 2.5.2 收集顺序（确定性，决定首个错误）

| 顺序 | 来源 | `SymbolKind` | 遍历顺序 |
|---|---|---|---|
| 1 | 每个 `python_node` 节点 | `PythonNode` | `nodes` 数组顺序（**v2 只做格式校验，不 import**） |
| 2 | 每条边的 `gate` | `Gate`（内置 op） | `edges` 数组顺序 |
| 3 | — | — | **无第三类**：Agent 级 skills/tools 不在图加载期解析（§2.5.7） |

- 去重按符号名；**同名跨 kind** 立即记为冲突（`DSL_SYMBOL_CONFLICT`），在 R14b 末尾统一报（定位：后出现的位置）。
- 子图定义内的符号：v1 子图不执行 → **不收集**（子图定义段只做结构校验；节点运行时被拒，R14 不解析其符号）。
- **与 04 §4.7.2 R1 的一致性**：skills/tools 归 Harness Loading（`HARNESS_CONFIG_LOAD` / `HARNESS_UNKNOWN_TOOL`），图级 R14 只解析 gate/python_node **格式与内置 op**；04 §4.7.2 R1 已同步（P-02-07 已解决，2026-10-28）。

### 2.5.3 解析时机与产物

| 阶段 | 动作 | 失败错误码 | 产物 |
|---|---|---|---|
| 图加载期（R14b） | **gate**：格式校验 → 内置 op 查表 → `params` 契约校验 → 登记；**python_node**：只做 `module/function` 格式校验，**不 import** | `DSL_SYMBOL_NOT_FOUND`（gate 名/格式、python_node 格式）/ `DSL_SIGNATURE_MISMATCH` / `DSL_SYMBOL_CONFLICT`（内置 gate op 与跨类同名） | `ResolvedSymbols` 快照（04 §4.7；`python_node` 只含格式信息） |
| run 启动期 | **不解析**（只校验 `entry_inputs`，§2.3.4） | — | — |
| 执行期（worker 启动） | supervisor spawn worker → worker `import` 用户模块 → 取属性 → 签名校验；失败回报 `NODE_WORKER_IMPORT` | `NODE_WORKER_IMPORT`（Crashed，§8.2） | worker 进程内符号句柄（不跨进程回传） |
| 执行期（gate） | 按内置 op 名查表求值（Rust 内置，不调用用户 Python） | 缺失 = 框架 bug → `Internal`（fail-fast，不重试） | — |

- `SymbolHandle` = 04 §4.7 `RegisteredGateOp` 的只读句柄（`Arc` 共享、`Send + Sync`）；v2 中该句柄只覆盖**内置 gate op**；`python_node` 的 `PythonSymbolRef` 只是格式信息，真实 Python 句柄位于 worker 进程内。
- **副作用边界（D7）**：v2 的图加载期**不执行用户 Python**，因此不存在"加载期 import 产生副作用/阻塞"风险；用户模块顶层代码在 **worker 启动时**执行，副作用属 worker 进程。
- 符号解析**不产生 run**，加载期失败只返回 DSL 错误（00 §0.4 DSL 域定义）；worker 导入失败已产生 run（运行期错误）。

### 2.5.4 `DSL_SYMBOL_*` 触发条件（精确判定）

| 错误码 | 触发条件（任一） | `expected` / `actual` | 定位 |
|---|---|---|---|
| `DSL_SYMBOL_NOT_FOUND` | ① **python_node 格式非法**：`config.module` 非点分 Python 标识符 / `config.function` 非 Python 标识符（`bad_format`；D7 保留在加载期）② **内置 gate op 路径**：符号名格式非法（非标识符）/ 内置 op 名不存在（不在 `check_output`/`compare`/`manual` 名单） | python_node：`expected {"symbol","kind":"python_node","format":"module.function"}` / `actual {"symbol","module","function"}`；gate：`expected {"symbol","kind":"gate","allowed":[...]}` / `actual {"symbol","kind":"gate"}` | python_node：`node_id`；gate：`edge_index` |
| `DSL_SIGNATURE_MISMATCH` | **仅内置 gate op 路径**：op 存在但 `gate.params` 不满足该 op 的 `params` 契约（04 §4.6/§4.7.2） | 对象 `{"symbol","kind":"gate","expected":<op 契约>}` / 实际 `params` | gate：`edge_index` |
| `DSL_SYMBOL_CONFLICT` | ① 同一符号名在图内同时被内置 gate op 与 `python_node` 要求（跨类同名，`builtin.<op>` vs 用户 `module.function`）② 进程级 Registry 已存在同名但**期望契约不同**的内置 op（§2.5.6） | 对象 `{"symbol":…,"existing_kind":…,"requested_kind":…}` | 后出现的位置 |

> 00 §0.4 已将 v1.1 的 `DSL_GATE_CONFLICT` / `DSL_GATE_SIGNATURE_MISMATCH` / `DSL_NODE_SIGNATURE_MISMATCH` 合并：本册严格使用合并后的三个码，`kind=gate|python_node` 放在 `expected`/`actual` 对象与 message 中（04 §4.3.5/§4.4 要求的 `symbol`/`kind` 字段即落在这里，**不新增错误对象顶层键**）。
>
> **D7 分工（ISSUE-026）**：`python_node` 的 **import/签名**失败属于**运行期**错误，worker 启动时失败固定为 `NODE_WORKER_IMPORT`（实例 Crashed，§8.2）；`DSL_SYMBOL_*` 仍覆盖 python_node 的 `module/function` **格式**校验（`DSL_SYMBOL_NOT_FOUND`，加载期、`node_id`），不再覆盖其 import/签名。

### 2.5.5 签名校验（契约：本册定错误与定位，判定算法在 04 §4.3.5）

| 符号类型 | 强制口径（v2 / D7） |
|---|---|
| gate（内置 op） | 加载期校验 `gate.params` 满足内置 op 契约（`check_output`/`compare`/`manual`，04 §4.6）；不再校验 Python 函数签名 |
| python_node | **不在加载期校验签名（D7）**；worker 启动时按 04 §4.3.5 的 S1–S5 校验，失败 → `NODE_WORKER_IMPORT`（Crashed）；返回注解只告警、运行期强制 `dict`（04 §4.3.2） |

- **参数名不校验**（框架按位置调用），`(inputs, ctx)` 只是规范书写。
- **注解不阻断**（关闭 P-10）：`from __future__ import annotations` 下注解是字符串、`typing.Dict` 与 `dict` 不等价、装饰器可改写 `__signature__`——按注解硬比较会误杀合法函数（v1.1 P-10 的"恒失败"缺陷）。
- 判定算法（S1–S7）见 04 §4.3.5；v2 中判定发生在 **worker 启动期**（Python 侧），失败经协议映射为 `NODE_WORKER_IMPORT`；本册只规定 `python_node` 加载期**格式校验**职责。
- 04 与 05 的边界：`SymbolHandle` 类型、worker 调用细节、异常 traceback 归 04；本册只定义"何时、按什么顺序、报什么码、定位到哪"。

### 2.5.6 热更新与生命周期（关闭 C-21 / S-18）

| 场景 | v1 裁定 | 依据 / 说明 |
|---|---|---|
| 同一 run 生命周期内 | `CompiledGraph` 与符号句柄**冻结**（`Arc`），run 绑定 `graph_hash` | 旧 run 永远看到它启动时的那张图 |
| 换图（新 `graph_hash`） | 重新 R1–R14 → 新 `CompiledGraph`；**旧 run 继续用旧产物**，无需 evict | per-run 持有 `Arc`，无全局缓存失效问题 |
| 改 Python 代码 | **不支持进程内热替换**；重载 = 重启进程 / 新的 `zgraph run` | 关闭 C-21：v1.1 §6.4 的"清该图符号缓存"删除；Registry 提供 `register`/`get`，无 `evict` |
| 两个图共用同一符号 | 签名一致 → Registry 幂等复用**同一句柄**（先注册者生效）；签名不一致 → 后加载的图在 R14b 报 `DSL_SYMBOL_CONFLICT` | 跨图加载需进程级 Registry（04）；v1 CLI 单图进程不触发该路径 |
| 编辑器"保存即校验" | v1.1+ 的 API 调同一 Loader 做 dry-run（本册 §2.1.2 三调用点）；v1 用 CLI `validate` | 关闭 S-18：加载/校验发生在 **run 启动时 + validate 时**，不在"保存"隐式发生 |
| 加载/编译缓存 | key = `graph_hash`；v1 CLI 单图不缓存，库 API 可自行 `HashMap<GraphHash, Arc<CompiledGraph>>` | 缓存不改变语义（相同 hash ⇒ 相同编译产物） |
| 加载失败 | **不产生 run**；已存在 run 不受影响 | 00 §0.4 DSL 域定义 |

### 2.5.7 与 04/05 的边界（关闭 C-20）

| 层级 | 符号 | 解析时机 | 所有者 | 失败错误码 |
|---|---|---|---|---|
| 图级 | `gate`（内置 op；Python gate 延后） | 图加载期（R14b） | 02（规则）/ 04（实现） | `DSL_SYMBOL_*` |
| 图级 | `python_node` | **worker 启动期**（v2 D7：加载期只校验格式） | 04 / worker | `NODE_WORKER_IMPORT`（运行期，§8.2） |
| Agent 级 | skills / tools（`AgentConfig.tools`） | Harness Loading（run 期，懒加载） | 05 | `HARNESS_CONFIG_LOAD` / `HARNESS_UNKNOWN_TOOL` |

- v1.1 §6.3 `collect_symbols` 的 `skills` 分支**删除**（C-20：DSL 无 skills 字段）。
- 图加载**不**校验 skills；`AgentConfig` 内部字段错误也不属 DSL 错误（只校验资产存在性，§2.6.3）。

---

## 2.6 v1 支持域与保留字行为

**一句话结论**：v1 支持 `start`/`end`/`agent_call`/`python_node`；`subgraph` **定义保留、加载期校验通过、运行期 run 启动后且实例创建前拒绝**（`NODE_TYPE_UNSUPPORTED`；ISSUE-046）；`on_error` 是保留字（出现即 `DSL_UNSUPPORTED_FIELD`）；`outputs`/`from_output_port` 保留但不改变 v1 的**整包广播**路由。

### 2.6.1 子图（关闭 C-17 的"v1 口径"）

| 项 | v1 裁定 |
|---|---|
| DSL schema | `subgraphs` 段与 `SubgraphDefinition` **完整保留**（结构、必填、递归校验） |
| 定义校验 | R6：`id` 图内唯一、≥1 个 `end`、不得含 `start`、`input_mapping`/`output_mapping` 引用存在且指向已声明端口、递归执行 R4/R5/R8–R13 |
| 节点使用 | 加载期：`type:"subgraph"` 是 schema 已知类型，**校验通过**（含 `subgraph_id` 必须命中 `subgraphs` 段，R6） |
| 运行时 | **run 启动后、实例创建前整体拒绝（ISSUE-046）**：run 启动成功后在创建实例前检测到 `NodeType::Subgraph`，随即整 run `Failed`（错误码 `NODE_TYPE_UNSUPPORTED`，00 运行期域），不创建实例、不构造 handler，其余非终态实例转 `Cancelled`（00 I9 / 04 §4.4.4） |
| 何时报 | **运行期、run 启动后且实例创建前**（不等"就绪 → 派发"）；既不是加载期，也不是"执行中"。trace 记 `node_id` + `instance_id=null`（06） |
| 为什么不在加载期拒绝 | ① 00 §0.3 `NodeType` 注释"Subgraph: v1 运行时不支持" + 决策记录 §2.2 明文（"运行到 `subgraph` 节点返回 `NODE_TYPE_UNSUPPORTED`"）；② 保留"能加载、不能跑"的显式语义，v1.1 启用后同一批图资产直接可用；③ 若加载期拒绝，00 运行期域的 `NODE_TYPE_UNSUPPORTED` 成为死码；④ `validate` 的语义是"DSL 合法"，本就不保证可运行（资产可用性、LLM 连通性都在运行期） |
| 代价（显式） | 含 `subgraph` 节点的图 `validate` 通过但 `run` 必失败；这是 v1 范围裁剪（D1）的已知语义，v1.1 启用子图后自动消失 |
| 版本演进 | v1.1 新增 `SubgraphHandler` 后移除该拒绝检查（run 启动后、实例创建前）；DSL 与校验规则不变（仍 R6）；`SubgraphConfig` 届时补 `input_mapping`/`output_mapping`（新增可选字段，兼容 schema 1.0） |
| `SubgraphConfig` 为什么没有 mapping | v1 不执行子图，节点级映射无消费者（C-17(a)）；映射语义留 v1.1，届时作为新增可选字段加入（仍兼容 schema 1.0） |

> **两个码的分工（本册裁定，与 04 A5 一致）**：`NODE_TYPE_UNSUPPORTED`（运行期域）= schema 已知但 v1 不执行的 `subgraph`；`DSL_UNSUPPORTED_NODE_TYPE`（加载期/DSL 域）= `type` 字符串**不在** 5 个枚举内（未知/保留类型，如 `"human_node"`）。00 §0.4 该行括号说明"如 `subgraph` 运行"需按此修订（§2.11 P-02-03）。

### 2.6.2 保留字段的 v1 行为（`outputs` / `from_output_port`）

| 字段 | 校验 | v1 行为 | v2 |
|---|---|---|---|
| `GraphNode.outputs?` | R4：端口名非空、节点内唯一、`type` 枚举 | **纯声明**：不实现字段级路由——节点 output（整包 `Value`）仍投递到每条出边的 `to_input_port`（整包广播，决策记录 §4） | 可启用"按端口取字段" |
| `GraphEdge.from_output_port?` | R9：**出现即 `DSL_UNSUPPORTED_FIELD`**（不看取值，v1 MUST 缺省） | 字段在 schema 中声明但不可用；不进入 `CompiledGraph` | 启用字段级路由时：值 MUST ∈ `from.outputs`，否则 `DSL_INVALID_PORT` |
| 一致性要求（v1） | 因 `from_output_port` 不可出现，**R9 的"outputs 与下游一致"检查在 v1 恒为空**；`outputs` 仅为 v2 前置声明 | — | v2 由 R9 恢复一致性校验 |

> 为什么在 schema 里声明保留字段、却让 v1 拒绝它：① v2 要启用字段级路由，schema 先占位可保证 `version` 不升；② 00 §0.4 用 `DSL_UNSUPPORTED_FIELD` 精确表达"保留字段"语义，比 `DSL_PARSE_ERROR`（拼写错误桶）更可诊断；③ `outputs` 可声明（v2 前置），`from_output_port` 不可出现（v1 无字段级路由）。

> 为什么用整包广播：C-19/M-16 的 Agent 输出契约（业务字段 vs `{content,stop_reason,iterations}`）在 v1 未定；整包广播 + 节点自取字段避免半实现的字段级路由（决策记录 §4）。

### 2.6.3 Agent 资产索引（R14c）

| 项 | 规则 |
|---|---|
| 资产根 | CLI `--assets-dir <dir>`；env `ZGRAPH_ASSETS`；**未提供时跳过 R14c**（ISSUE-027，M2 口径；否则 `zgraph validate <graph>` 单文件不可用） |
| 索引范围 | `<assets-dir>/agents/*.json`（单层、不递归；按文件名排序读入以保证确定性） |
| id 来源 | 文件内 `"id"` 字段（= `AgentConfig.id`，05 §5.2.2）；文件名无关 |
| 校验内容 | **仅**：索引可读、`id` 非空、`id` 全局唯一、图内每个 `agent_call.agent_id` 都能命中 |
| 错误码 | 未命中 → `DSL_AGENT_NOT_FOUND`；同 id 多文件 → 同码，message 区分 `duplicate agent asset id`（§2.11 P-02-02） |
| 不校验 | `AgentConfig` 字段合法性、模型连通性、prompt 文件存在性、tools 可解析性 → 全部归 05（运行期 `HARNESS_CONFIG_LOAD` 等） |

> **跳过语义（ISSUE-027 回写）**：只有当**未提供** `--assets-dir` 且未设置 `ZGRAPH_ASSETS` 时才跳过 R14c；一旦显式提供资产根，`DSL_AGENT_NOT_FOUND` 强制生效。后果：未提供资产根时 `agent_call.agent_id` 的存在性延迟到运行期（M4 Harness Loading）才暴露，`validate` 结果因此受 CLI 参数影响——这是 M2 的**已知取舍**，在 06 §6.10.2 固定该行为。

### 2.6.4 v1 支持域总表

| 特性 | DSL 定义 | 加载期校验 | v1 运行 | 拒绝错误码 | 计划版本 |
|---|---|---|---|---|---|
| `start` 节点 | ✅ | ✅ | ✅（输出 = 注入的 inputs，04 §4.4.1） | — | v1 |
| `end` 节点 | ✅ | ✅ | ✅（run 输出，03） | — | v1 |
| `agent_call` | ✅ | ✅ | ✅（真实 LLM / `--fake-llm`） | — | v1 |
| `python_node` | ✅ | ✅ | ✅ | — | v1 |
| `subgraph` 节点 | ✅ | ✅（结构 + 引用存在） | ❌（run 启动后、实例创建前拒绝，run Failed；ISSUE-046） | `NODE_TYPE_UNSUPPORTED`（运行期） | v1.1 |
| `subgraphs` 定义段 | ✅ | ✅（R6） | —（不执行） | `DSL_SUBGRAPH_INVALID` | v1.1（执行） |
| 未知/保留 `type` 字符串 | ❌（不在枚举） | 出现即错（R4） | ❌ | `DSL_UNSUPPORTED_NODE_TYPE`（加载期） | 待定义 |
| `on_error` | ❌（保留字） | 出现即错 | ❌ | `DSL_UNSUPPORTED_FIELD`（加载期） | v2 |
| `outputs` | ✅（保留声明） | ✅（R4 结构） | ❌（整包广播） | `DSL_INVALID_PORT`（端口声明非法） | v2（字段级路由） |
| `from_output_port` | ✅（保留声明） | ❌（出现即错） | ❌ | `DSL_UNSUPPORTED_FIELD`（加载期） | v2（字段级路由） |
| `human_input` 端口 / WaitingHuman | ❌ | — | ❌ | —（D4） | v1.1 |

---

## 2.7 错误响应结构与 CLI 输出

**一句话结论**：所有 DSL 错误以**同一 JSON 对象**对外（`code/message/location/expected/actual`），容器为 `{ok, errors, error_count, truncated}`；CLI 人类可读输出 ≈ 一行定位 + 期望/实际，`--json` 输出容器；`validate` exit 0=合法、2=DSL 错误、1=其他。

### 2.7.1 错误对象（唯一对外形状）

```json
{
  "code": "DSL_INVALID_PORT",
  "message": "edge[3].to_input_port 'query' is not declared on node 'kb_search'",
  "location": {
    "node_id": "kb_search",
    "edge_index": 3,
    "port": "query"
  },
  "expected": ["query_text", "top_k"],
  "actual": "query"
}
```

| 字段 | 类型 | 必填 | 规则 |
|---|---|---|---|
| `code` | string | ✅ | 必须是 00 §0.4 DSL 域错误码之一（本册使用 17 个码，§2.4.1） |
| `message` | string | ✅ | 英文模板（§2.4.2），只读；MUST NOT 作为逻辑判断依据 |
| `location` | object | ✅ | 可为 `{}`；键仅 `node_id`/`edge_index`/`port`（§2.7.3） |
| `expected` | JSON value | 视错误码 | 允许值数组 / 类型 / 格式串 / 结构化对象（符号类：`{"symbol","kind",...}`，§2.5.4） |
| `actual` | JSON value | 视错误码 | 实际值 / 实际类型 / 实际签名串 / 同结构对象 |

### 2.7.2 错误容器（库 API 与 `--json` 输出）

```json
{
  "ok": false,
  "errors": [ { "code": "DSL_INVALID_PORT", "message": "...", "location": {"edge_index": 3, "node_id": "kb_search", "port": "query"}, "expected": ["query_text", "top_k"], "actual": "query" } ],
  "error_count": 1,
  "truncated": false
}
```

| 约定 | 规则 |
|---|---|
| `ok=true` | `errors` 为空数组；`zgraph validate` 成功时 `--json` 输出 `{"ok":true,"errors":[],"error_count":0,"truncated":false,"graph_hash":"<16hex>"}` |
| 顺序 | 按 R1→R14、数组序；`errors[0]` 为**首错**（可用作快速失败） |
| 截断 | `error_count > MAX_DSL_ERRORS(20)` 时 `truncated=true`，`errors` 只含前 20 条 |
| 扩展 | 未来可加 `message_key`（i18n）而不改 `code` |

### 2.7.3 `location` 字段规则

| 字段 | 取值域 | 何时使用 |
|---|---|---|
| `node_id` | `NodeId ∪ SubgraphId` | 节点级错误；子图级错误填 `SubgraphId`（message 前缀 `subgraphs[i]`） |
| `edge_index` | `0..edges.len()-1` | 边级错误（含该边的 `gate` 符号错误） |
| `port` | `PortName` | 端口级错误（`to_input_port` 引用、节点端口声明） |

- **至少一个定位字段**：除 R1（JSON 解析，`{}` + message 带 line/column）与 R2/R3 的文档级错误（`{}`，message 说明字段名）外，每条错误 MUST 含 `node_id` 或 `edge_index`。
- 多定位字段并存时，CLI 按 `edge_index` → `node_id` → `port` 顺序渲染。

### 2.7.4 CLI 输出与退出码

```bash
$ zgraph validate graph.json
✓ graph valid: 6 nodes, 6 edges, 0 subgraphs (graph_hash=3f2a9c1d4e5b6a70)
$ echo $?    # 0
```

```bash
$ zgraph validate bad-graph.json
error[DSL_INVALID_PORT] bad-graph.json: edges[3].to_input_port
  edge[3] (answer_generator -> kb_search): port 'query' is not declared on node 'kb_search'
  expected: one of [query_text, top_k]
  actual:   query
1 error(s)
$ echo $?    # 2
```

| 行为 | 约定 |
|---|---|
| 成功输出 | stdout：`✓ graph valid: {n} nodes, {m} edges, {k} subgraphs (graph_hash={hash})` |
| 失败输出 | stderr：`error[{code}] {file}: {定位串}` + 缩进 detail + `expected/actual`（存在时）+ `{n} error(s)` |
| 定位串格式 | 文档级：`{file}`；节点级：`{file}: node '{node_id}'`；边级：`{file}: edges[{edge_index}]`；端口级：`{file}: edges[{edge_index}].{port_field}` / `{file}: node '{node_id}'.inputs['{port}']` |
| `--json` | stdout 输出 §2.7.2 容器（失败时不混入人类可读文本） |
| `--structure-only` | 只跑 R1–R13；成功输出附带 `(external references not verified)` |
| `--print-hash` | 额外输出 `graph_hash=<16hex>`（仅成功时） |
| 退出码（`validate`） | `0` 合法；`2` DSL 错误；`1` 其他（文件不可读、内部错误）。**M1 临时口径（ISSUE-059）**：文件不可读/用法解析失败返回 `3`；M3 统一为「用法 → 2、IO/内部 → 1」（06 §6.10.5 已同步标注） |
| 退出码（`run`） | DSL 错误 → `2` 且**不创建 run**；运行期结果退出码归 03/06（本册不定义） |

### 2.7.5 稳定性约定

- `code` 与 `location` 键是**稳定对外契约**（trace、测试断言、前端提示可以依赖）。
- `message` 文案可变（英文模板 → 未来 i18n），**不得**用于逻辑判断。
- 新增错误码只允许来自 00 §0.4；本册不新增（§2.11）。

---

## 2.8 版本与图指纹（关闭 M-20）

**一句话结论**：schema 版本恒为 `"1.0"`（Loader 只接受该值）；`graph_hash = blake3(canonical_json)[0..16 hex]`，canonical JSON = 白名单投影 + 键排序 + 无空白 + 数组保序，**不补齐默认值**、不含 `description/author/created_at`。

### 2.8.1 `version` 策略

| 输入 | 行为 |
|---|---|
| `"1.0"` | 接受，按本册处理 |
| `"1.x" (x>0)` / `"2.0"` / `"1"` / 数字 `1.0` | 拒绝：R3 `DSL_PARSE_ERROR`，message 含 `unsupported DSL version`，`expected:"1.0"` |
| 缺 `version` | R2 `DSL_MISSING_FIELD` |

- 本册自身版本（v1.2）与 DSL schema 版本（`1.0`）**分离**：文档版本变化不改变 schema 版本，除非新增**破坏性**变更。
- 破坏性变更（如重命名 `module`/`function`、删除 `config`）必须升 schema 到 `"1.1"` 并在 Loader 增加分支；本册禁止在 `"1.0"` 内做破坏性变更（00 §0.1）。

### 2.8.2 `graph_hash` 计算（六步）

| 步 | 动作 | 规则 |
|---|---|---|
| 1 | 前置 | 仅当 **R1–R13 全部通过**时计算（R14 失败不产出 hash：加载未成功） |
| 2 | 投影 | 构造 `canonical` 对象（字段白名单，§2.8.3），**基于 DSL 原文**（不补齐默认值：`trigger` 缺省则不出现、`timeout_ms`/`retry` 不补齐） |
| 3 | 排序 | 递归按**对象键的 UTF-8 字节序升序**重排；数组**保持原顺序**（`nodes`/`edges`/`subgraphs` 顺序均有语义） |
| 4 | 序列化 | 无缩进、无空白；字符串最小转义（仅 `"`、`\`、控制字符，按 RFC 8259）；整数最短十进制（无 `+`、无前导零、`-0`→`0`）；若 `gate.params` 含浮点，按其最短往返表示（RFC 8785 口径） |
| 5 | 编码 | UTF-8，无 BOM |
| 6 | 摘要 | `blake3(canonical_bytes)` → 取十六进制摘要**前 16 个字符**（8 字节，小写），即 00 §0.2 的 `GraphHash` |

### 2.8.3 规范化字段白名单（投影表）

| 对象 | 进入 hash 的字段 | 不进入 hash |
|---|---|---|
| 顶层 | `version`、`metadata.name`、`entry_points`、`nodes`、`edges`、`subgraphs?` | — |
| `metadata` | 仅 `name` | `description`、`author`、`created_at` |
| 节点 | `id`、`type`、`inputs`（`name/type/required/description?`）、`trigger?`（**仅在作者写了时**）、`config`（按类型全量）、`outputs?` | —（`config` 默认值不补齐） |
| 输入/输出端口 | `name`、`type`、`required`（仅输入）、`description?` | — |
| 边 | `from`、`to_node`、`to_input_port`、`gate?`（`function` + `params` 全量）、`from_output_port?` | — |
| 子图 | `id`、`nodes`、`edges`、`input_mapping`、`output_mapping`（递归同规则） | — |
| 其他 | — | `on_error`（拒绝）、任何未通过 R1–R13 的内容 |

- **已知限制（明示）**：语义等价但写法不同的两张图（缺省 `trigger` vs 显式 `"all"`）会产生**不同** hash。若需语义去重，先在编辑器归一化；v1 不做。
- **不覆盖**：Python 代码版本、Agent 资产内容、skills/tools 实现 → 均不参与 hash（风险见 §2.8.5）。

### 2.8.4 存储位置与用途

| 位置 | 形态 | 写入者 | 用途 |
|---|---|---|---|
| `CompiledGraph.graph_hash` | 内存 `GraphHash` | Loader | 运行期绑定、日志 |
| `RunStarted.graph_hash`（trace 事件） | `String` | Scheduler（01/03） | 回放时确认图版本 |
| SQLite `runs.graph_hash` | `TEXT` | 06 | 查询/统计/回放校验 |
| CLI `validate --print-hash` | stdout | CLI | CI 断言"图未被改动" |
| 编译缓存 key（库 API，可选） | `HashMap<GraphHash, Arc<CompiledGraph>>` | 宿主 | 同图复用 |
| run 级冻结键 | `RunSpec.graph: Arc<CompiledGraph>` 即加载产物（03 §3.2）；run 记录其 `graph_hash` | 03 | 防止"启动时换了图" |

### 2.8.5 不作为的事（风险声明）

| 不用于 | 原因 |
|---|---|
| 安全/完整性校验、防篡改 | 无签名、无认证；仅内容指纹 |
| Python 代码/资产版本绑定 | hash 不含代码与资产；v1 通过"重启进程 + 部署 commit 记录"兜底 |
| 跨机器语义等价比对 | 依赖 canonical 规则实现一致性（同一实现跨机器稳定） |

---

## 2.9 DSL → `CompiledGraph` 编译映射

**一句话结论**：`CompiledGraph` / `CompiledNode` / `CompiledEdge` / `EdgeIndex` 的**结构由 01 §1.1.1/§1.1.2 定义**（本册不重复声明，避免双定义）；本册定义 **Loader 的构造顺序、DSL→产物的字段映射、编译期不变式**；符号句柄不进 `CompiledGraph`，由 04 `ResolvedSymbols` 单独快照。

### 2.9.1 所有权与构造边界

| 项 | 所有者 | 说明 |
|---|---|---|
| `CompiledGraph`/`CompiledNode`/`CompiledEdge`/`EdgeIndex` 结构 | **01 §1.1.1 / §1.1.2** | 本册只引用其字段名（`nodes: HashMap<NodeId, CompiledNode>`、`node_order`、`to_port`、`handler`、`gate: Option<GateRef>`） |
| 构造流程（Loader 编译步骤） | **02（本册）**，§2.9.2 | R1–R13 通过后才构造；产物不可变（`Arc<CompiledGraph>`） |
| `HandlerRef` 具体值（`Start/End/Python{module,function}/Agent{agent_id}/Subgraph{id}`） | 01 定义字段 / **04 §4.1.2** 构造 handler | 02 只决定"填哪个变体"（映射见 §2.9.3） |
| 符号句柄（Python/gate） | **04 §4.7 `ResolvedSymbols`** | 与 `CompiledGraph` 分离；run 同时持有 `Arc<CompiledGraph>` 与 `Arc<ResolvedSymbols>` |
| `graph_hash` | **02 §2.8** | 编译期最后一步 |
| `EdgeIndex.incoming` 单值 | 01 §1.1.2 定义 / 02 R11 保证 | `(to_node,to_port)` 唯一（D-02-01） |

### 2.9.2 构造顺序（对应 §2.1.2 的 L2–L4）

| 步 | 动作 | 失败处理 |
|---|---|---|
| 1 | R1–R13 通过（纯函数，不触碰外部） | 返回 DSL 错误，无产物 |
| 2 | 按 DSL `nodes` 顺序建 `CompiledNode` 并 push `node_order`：`inputs` 原样、`outputs` 缺省空集、`trigger` 缺省 `All`、`timeout_ms` 缺省 180_000、`max_attempts` 缺省 3、`handler` 按 §2.9.3 映射 | 不应失败（已校验）；失败记 `Internal` |
| 3 | 按 DSL `edges` 顺序建 `CompiledEdge`：`index = i`、`to_port = DSL.to_input_port`、`gate = Some(GateRef{ symbol, params })`（symbol 先存字符串，R14b 后绑定句柄） | 同上 |
| 4 | 建 `EdgeIndex`：`outgoing[from].push(i)`、`incoming[(to_node,to_port)] = i`（写入前断言键不存在）、`incoming_by_node[to_node].push(i)` | 覆盖 = 框架 bug（`Internal`） |
| 5 | R14：图级符号解析 → `ResolvedSymbols`（04 §4.7）；agent 资产索引校验 | `DSL_SYMBOL_*` / `DSL_AGENT_NOT_FOUND`；无产物 |
| 6 | 计算 `graph_hash`（§2.8；仅 R1–R13 内容） | 不应失败；失败记 `Internal` |

### 2.9.3 字段映射表（目标字段名以 01 §1.1.1 为准）

| DSL 字段 | `CompiledGraph` 目标（01 §1.1.1） | 转换规则 |
|---|---|---|
| `version` | —（产物无此字段） | 仅 R3 兼容性判定；`CompiledGraph` 不存 schema 版本 |
| `metadata` | `.metadata: GraphMetadata` | 原样（含可选 `description/author/created_at`；**不参与 hash**，§2.8.3） |
| `entry_points[]` | `.entry_points: Vec<NodeId>` | 保持顺序；已由 R7 校验 |
| `nodes[].id` | `.nodes[id].id` + `.node_order.push(id)` | 顺序保持（`node_order` = DSL 声明顺序） |
| `nodes[].type` | `.nodes[id].node_type` | snake_case → `NodeType`（00 §0.3） |
| `nodes[].inputs[]` | `.nodes[id].inputs: Vec<InputPort>` | 原样（`PortType` 枚举） |
| `nodes[].outputs?` | `.nodes[id].outputs: Vec<OutputPort>` | 缺省空 `Vec`；v1 不参与路由（R4 已校验端口结构，§2.6.2） |
| `nodes[].trigger?` | `.nodes[id].trigger` | 缺省 `All`（补齐只发生在产物，不回写 DSL） |
| `nodes[].config.timeout_ms?` | `.nodes[id].timeout_ms` | 缺省 `DEFAULT_NODE_TIMEOUT_MS`（00 §0.5） |
| `nodes[].config.retry.max_attempts?` | `.nodes[id].max_attempts` | 缺省 `DEFAULT_MAX_ATTEMPTS`；仅自环/重试使用 |
| `agent_call.config.agent_id` | `.nodes[id].handler = Agent{agent_id}` | R14c 已校验资产存在 |
| `agent_call.config.input_mapping?` | `.nodes[id].handler = Agent{agent_id, input_mapping}` | 缺省 `{}`（identity 映射）；01 §1.1.1 已扩展该字段（P-02-09 已解决）；05 §5.9.3 消费 |
| `python_node.config.module`+`.function` | `.nodes[id].handler = Python{module,function}` | 全限定符号 `"module.function"`（§2.5.1）；R14b 仅格式校验（D7，不 import） |
| `subgraph.config.subgraph_id` | `.nodes[id].handler = Subgraph{id}` | R6 已校验引用存在；运行期 run 启动后、实例创建前拒绝（§2.6.1） |
| `start`/`end` 的 `config` | `.nodes[id].handler = Start` / `End` | 必须 `{}` |
| `edges[]` | `.edges[i] { index: i, from, to_node, to_port, gate }` | `to_port` = DSL `to_input_port`；顺序保持 |
| `edges[].from_output_port?` | **不进入产物** | v1 出现即 R9 `DSL_UNSUPPORTED_FIELD`（保留字段）；v2 启用字段级路由时由 01/02 增字段 |
| `edges[].gate.function` | `.edges[i].gate.symbol`（`GateRef`） | 内置 op 短名（`check_output`/`compare`/`manual`），R14b 查表校验（D6） |
| `edges[].gate.params?` | `.edges[i].gate.params` | 缺省 `{}`；原样传入 `GateContext.params` |
| `subgraphs[]` | `.subgraphs: HashMap<SubgraphId, Arc<CompiledGraph>>` | 递归编译（v1 不执行） |
| （解析产物） | **不在 `CompiledGraph`** | R14b 产出 04 §4.7 的 `ResolvedSymbols`（按 `NodeId` / `by_gate_edge` 索引） |
| （派生索引） | `.edge_index: EdgeIdx` | 见 §2.9.2 步 4；`incoming` 单值（R11） |
| （计算产物） | `.graph_hash` | §2.8 |
| `on_error` | — | 拒绝（`DSL_UNSUPPORTED_FIELD`，不进入产物） |

### 2.9.4 编译产物不变式（debug 断言 / 单测）

| # | 不变式 | 违反处理 |
|---|---|---|
| CI-1 | `edges[i].index == i`；每个 `EdgeIdx` 在 `outgoing`、`incoming`、`incoming_by_node` 中**各出现且仅出现一次** | `Internal`（框架 bug） |
| CI-2 | `incoming` 写入前键不存在（R11 保证无覆盖） | `assert!` |
| CI-3 | `nodes` 键集合 == `node_order` 元素集合，无重复、无悬空 id | `assert!` |
| CI-4 | 每条带 gate 的边在 `ResolvedSymbols.by_gate_edge` 有句柄；每个 `python_node` 在 `by_node` 有句柄 | `assert!` |
| CI-5 | `graph_hash.len() == 16` 且为小写 hex | `assert!` |
| CI-6 | 每个 `CompiledNode.timeout_ms >= 1`、`max_attempts >= 1` | `assert!` |

---

## 2.10 本册关闭的审计条目

**一句话结论**：本册关闭审计 C-04/C-05/C-15/C-17/C-20/C-21/C-22/C-25/C-26、M-11/M-16/M-20、S-01/S-18 中**属于 DSL/编译期**的部分；运行期部分指向 01/03/04/05。

| 审计条目 | 关闭方式 | 位置 |
|---|---|---|
| C-04（incoming 单值 vs 多源同端口） | 裁定禁 fan-in；`incoming` 保持单值；R11 保证 | D-02-01 / §2.4.1 / §2.9.1 / 01 §1.1.2 |
| C-05（规则 7 vs 不变式） | 拆分 R10/R11，两码 `DSL_DUPLICATE_EDGE`/`DSL_DUPLICATE_PORT_SOURCE` | D-02-01 / §2.4.1 |
| C-15（示例自我违反） | 修正示例；`end` 必须声明 required 输入；示例自查表 | D-02-03 / §2.3 |
| C-17（子图 mapping / 单 End） | R6 改为"至少一个 end"+mapping 引用校验；子图节点"加载通过 / 运行期 run 启动后、实例创建前拒绝"（与 04 §4.4.4 联合关闭）；节点级 mapping 留 v1.1 | §2.6.1 / §2.4.1 |
| C-20（skills 符号来源） | 两级符号解析：图级 R14b，Agent 级归 Harness | §2.5.7 |
| C-21（热更新 vs Registry） | v1 不支持代码热替换；per-run `Arc` 冻结；无 evict | §2.5.6 |
| C-22（gate 符号三写法） | v1.2 曾统一为全限定 `module.function`；**v2 D6 改为内置 op 短名**（Python gate 延后，ISSUE-014） | §2.5.1 |
| C-25（created_at 必填） | 改可选；R3 校验格式；不参与 hash | D-02-02 / §2.2.1 / §2.8.3 |
| C-26（自环与环检测） | 自环不豁免；无 gate 自环报 `DSL_CYCLE_DETECTED`；次数上限走 `retry.max_attempts` | D-02-05 / R13 |
| M-11（错误码分层/聚合） | 17 码映射到 R1–R14；跨规则 fail-fast + 同规则聚合 + 20 条上限；错误响应结构 | §2.1.3 / §2.4 / §2.7 |
| M-16（输出端口/字段级路由） | `outputs` 保留声明（R4 校验）；`from_output_port` 出现即 `DSL_UNSUPPORTED_FIELD`（R9）；v1 整包广播裁定 | §2.2.1 / §2.6.2 |
| M-20（版本与 graph_hash） | version 只接受 `"1.0"`；六步 canonical hash；存储与用途 | §2.8 |
| S-01（entry 注入） | `entry_points` 权威、entry 无入边、`RunSpec.entry_inputs` 契约；`start` 语义 | D-02-04/D-02-08 / §2.3.4 |
| S-18（加载时机） | 加载/解析在 run 启动与 validate；缓存 key=graph_hash；失败不产生 run | §2.5.3 / §2.5.6 |

**本册部分关闭 / 不覆盖**：C-19（Agent 输出形状 → 05）、S-17（end 输出 → 03/04）、S-03/S-08（attempt 运行期语义 → 03）、C-01（超时预算层级 → 03，DSL 只提供 `timeout_ms`）。

---

## 2.11 待裁决项与跨册风险（交 Lead）

**一句话结论**：本册不修改 00；以下 9 项是"00 未覆盖/措辞冲突/跨册分歧"的登记，均已在册内用**既有错误码 + message 细分**兜底，不阻塞编码；其中 P-02-01/05/06 已随 00 更新关闭。

| # | 问题 | 本册处置 | 建议 |
|---|---|---|---|
| P-02-01 | ✅ **已对齐**：00 §0.4 已增补 `DSL_UNSUPPORTED_FIELD`（"出现 v1 不支持/保留的字段（`from_output_port`、`on_error`）"） | 本册按该码执行：`on_error`（R4）与 `from_output_port`（R9）出现即报；不再用 `DSL_PARSE_ERROR` 兜底 | 无（00 已更新；02 §2.2.4/§2.4/§2.6.2 已同步） |
| P-02-02 | 00 未覆盖"start/end 边方向非法"与"资产索引非法"的错误码归属 | 分别复用 `DSL_INVALID_EDGE_REF`（reason: `end_as_source`/`start_as_target`）与 `DSL_AGENT_NOT_FOUND`（message: `duplicate agent asset id`） | ✅ 已解决：不新增错误码（本册 §2.7.5 规定不新增） |
| P-02-03 | 00 §0.4 DSL 行把 `DSL_UNSUPPORTED_NODE_TYPE` 描述为"使用了 v1 不支持的节点类型（如 `subgraph` 运行）"，与运行期码 `NODE_TYPE_UNSUPPORTED` 的职责重叠 | 裁定：`subgraph` = **运行期（run 启动后、实例创建前）** `NODE_TYPE_UNSUPPORTED`（决策记录 §2.2 / 04 §4.4.4）；`DSL_UNSUPPORTED_NODE_TYPE` = **加载期未知/保留 `type` 字符串** | 与 04 A5 一致：建议 00 修订该行说明为"使用了未知/保留的节点类型字符串" |
| P-02-04 | `GateDefinition.function` 取值语义收紧与 00 §0.1"新增字段均为可选/保留字，向后兼容"的字面表述冲突 | 属**语义收紧**而非字段变更；v2 D6 进一步改为内置 op 短名（Python gate 延后） | 00 §0.1 已注明"符号名格式收紧属 v1.2 修订" |
| P-02-05 | ✅ **已关闭**：决策记录 §2.1 已更正为“17 个 `DSL_*` 错误码”；00 §0.4 现列 17 个 | 以 00 为准 | ✅ 已回填（2026-10-28） |
| P-02-06 | ✅ **已关闭**：00 §0.4 运行期域已增补 `RUN_ENTRY_INVALID` / `RUN_ENTRY_INPUT_MISSING` | 本册 §2.3.4 的 `RunSpec` 契约与 03 §3.1 的启动校验已对齐此两码 | 无 |
| P-02-07 | **04 §4.7.2 R1③** 把 `agent_call` 的 skill 符号纳入**图加载期**收集（错误码走 `DSL_SYMBOL_*`）；与 00 §0.4（工具错误归 Harness 域）、05 §5.6/§5.11（tools 由 Agent 加载期校验 `HARNESS_CONFIG_LOAD`）冲突 | 本册按 00/05 裁定：图级 R14 只解析 gate/python_node；skills/tools 归 Harness Loading | lead/08 裁决；若采纳 04，则 02 需增"③ skill 符号"收集与错误码映射（改动点 §2.5.2/§2.5.4/§2.5.7） |
| P-02-08 | **07 §AC-04** 把 `unsupported_subgraph.json` 列为 `zgraph validate` 的 `DSL_UNSUPPORTED_NODE_TYPE` 反例；按本册裁定该图 validate **通过**、`run` 才失败 | 07 已改为：① `unknown_node_type.json` 做 validate 反例；② 子图 e2e 断言 `run` Failed + `NODE_TYPE_UNSUPPORTED` | ✅ 已关闭（2026-10-28） |
| P-02-09 | 01 §1.1.1 `CompiledNode.handler = HandlerRef::Agent{agent_id}` **未携带 DSL 节点级 `input_mapping`**；05 §5.9.3 需要它做路径映射 | 01 已增 `input_mapping` 字段，本册 §2.9.3 已改为“进入产物” | ✅ 已关闭（2026-10-28） |

**跨册接口约定（需一致）**：

| 约定 | 本册口径 | 需对齐方 |
|---|---|---|
| `message` 语言 | 英文稳定模板（§2.4.2） | 03/06/07（CLI 与测试断言） |
| 错误响应对象形状 | `{code,message,location,expected,actual}` + 容器 | 06（CLI）、07（断言） |
| `CompiledGraph` 所有权 | **结构 01 §1.1.1 定义、02 构造**、01/03/04 只读消费 | 01 |
| 符号句柄 | `RegisteredGateOp`（内置 gate）/ `PythonSymbolRef`（格式信息）/ `ResolvedSymbols` 快照（04 §4.7），与 `CompiledGraph` 分离 | 04 |
| `AgentCallConfig.input_mapping` 语义 | 端口名 → Agent 输入字段，05 消费 | 05 |
| `graph_hash` 存储列 | 06 `runs.graph_hash` | 06 |
