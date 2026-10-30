# zgraph · 图模型驱动工作流框架

用有向图描述可复用的工作流（SOP / Agent 工作流）：用户自定义图与节点，平台负责调度、状态、门控与可复现执行。
流程里可以跑 Agent，也可以只跑普通 Python 节点；**结果可复现是产品承诺**（L1 调度可重放 / L2 结果可重放）。

**技术形态（DR-001）**：**Rust 内核 + Python 子进程 worker**（单二进制内核；用户代码在子进程隔离执行）。

- 决策：[`doc/decisions/DR-001-runtime-route.md`](doc/decisions/DR-001-runtime-route.md)
- 架构：[`doc/architecture-v2.md`](doc/architecture-v2.md)（协议 v1 / 偏差表 D1–D12 / M0–M5 里程碑）
- 语义基准：[`doc/spec-v1.2/`](doc/spec-v1.2/README.md)
- 文档索引：[`doc/index.md`](doc/index.md)；流程约束：[`AGENTS.md`](AGENTS.md)

## 当前状态：M5 收尾完成

**已完成 M0–M5**：Cargo workspace（6 个 crate + `tests/e2e` 验收包）、DSL loader/校验（R1–R14）、
图内核与事件循环、trace/SQLite（L1 回放）、worker supervisor + `WorkerExecutor`（进程组 kill/取消升级/帧超时/并发上限）、
**Agent Harness**（`agent_call`：`genai 0.7.2` LLM 流式 + Fake provider、tool loop（每次调用一个 Python worker 子进程）、
上下文预算/裁剪、错误映射、`HarnessExecutor`）、`python/workers` 与 `python/zgraph_worker.tool_runner`、
spec v1.2 M0–M4 回写（05/00/04/07，REV-023）；AC-08 fake 真链路验收（`tests/e2e/harness_ac08.rs`）+ AC-06/07/09/10 真子进程验收；
本地双栈测试全绿（M4 收尾基线 `cargo test --workspace` 423 passed / pytest 84 passed）。CI 已配置（`.github/workflows/ci.yml`）。

**M5 收尾完成**（DoD 全部达成；口径见 [`doc/decisions/DR-002-m5-cli-replay-recording.md`](doc/decisions/DR-002-m5-cli-replay-recording.md)）：

- **已交付并合并到 main**：`zgraph run` 真链路（`--fake-llm[=<script>]` / `--real` / `--record=none|hash|full` / `--json` / 退出码 / `:memory:` / SIGINT，REV-028）；`zgraph replay` L1/L2 与 `llm_calls` 落库（REV-030）；CLI 启动中止标定 / `--json.recovered[]` / `trace` 进行中提示（REV-030）；AC-12 多 run 并发隔离真链路（10 run 同库，REV-029）；e2e AC-01 CLI / L2 replay / 崩溃标定 / ISSUE-076 端到端（REV-032）；ISSUE-061（gate 幻影 attempt，REV-026）、ISSUE-074（结构感知截断，合法 JSON ≤8 KiB，REV-027）、ISSUE-066（worker crash 记录 signal/exit/stderr_tail，REV-031）、ISSUE-076（`declared_timeout_ms` + `timeout_hint` + CLI 转发，REV-026/033/030）、ISSUE-048（终态 pid 提示，REV-034/030）、ISSUE-084（崩溃中止标定 + `recovered[]`，REV-034/030/032）均 **M5 已修**；spec v1.2 M5 回写（00/04/05/06/07/08 + `PROVENANCE`，REV-035）。
- **本批（REV-036 收尾，已合入）**：台账/进度/README/评审索引/`test-baseline` 收口；最终 `cargo test --workspace` 0 failed、`pytest -q` 84 passed。
- **后续（M6）**：DR-002 §7/§9 顺延（ISSUE-033/036/059/073/081 剩余/082 版本列/084→088）+ DR-002 §4 顺延（075/077/079/080）+ REV-028/030/032 新登记（085/086/087/089）+ process 依赖治理/CI 固化（ISSUE-068/078/073，REV-037 顺延 M6）。
- 范围与落地口径：[`doc/decisions/DR-002-m5-cli-replay-recording.md`](doc/decisions/DR-002-m5-cli-replay-recording.md)；进度：[`doc/progress.md`](doc/progress.md)；遗留项：[`doc/issues.md`](doc/issues.md)。

## 快速开始

```bash
# Rust（1.99+）
cargo test --workspace
cargo run -p zgraph-cli -- --version

# Python worker SDK（3.10+；依赖管理用 uv，见 CLAUDE.md）
uv sync --extra dev
uv run pytest -q

# 若本机没有 uv：先安装 uv（brew install uv 或官方安装脚本），不要用 pip 直接装项目依赖（CLAUDE.md）
```

> 本机若 crates.io 直连不稳定，可参考 `.cargo/config.toml` 注释配置代理；沙箱环境需把 `CARGO_HOME` 指到可写目录。

## 目录

| 路径 | 说明 |
|---|---|
| `crates/zgraph-dsl/` | DSL 类型、loader、R1–R14、graph_hash（M1） |
| `crates/zgraph-kernel/` | 状态机、事件循环、Delivery、门控、run 生命周期（M2） |
| `crates/zgraph-trace/` | TraceEvent、SQLite、回放（M2） |
| `crates/zgraph-worker/` | worker 协议、supervisor（进程组/取消/帧超时）、`WorkerExecutor`（M3）；M4 `WorkerFailure::Worker` 装箱 |
| `crates/zgraph-harness/` | Agent Harness（M4）：AgentConfig/预算/LLM(fake+genai 0.7.2)/tool loop/`HarnessExecutor` |
| `crates/zgraph-cli/` | `zgraph` 命令（M1：`validate`；M2：`trace`；M5：`run` 已交付、`replay` 见 wave2） |
| `python/zgraph_worker/` | worker SDK：握手、ctx、运行时；M4 `tool_runner`（discover/call） |
| `python/workers/` | 示例 worker：echo/slow/parallel/cycle/software_team（M3） |
| `schemas/` | worker 协议 v1（M0 冻结，M3 检查单闭环）；DSL JSON Schema 未实现（ISSUE-029） |
| `tests/e2e/` | 真链路验收：AC-06/07/09/10 + 故障注入 + 并发（M3）；AC-08 fake LLM/tool/流式（M4）；AC-12 多 run 隔离（M5） |
| `tests/fixtures/` | 图与 worker fixture |
| `doc/` | 决策/架构/规格/评审/进度/台账 |
| `legacy/` | 旧 Python 实现（冻结只读，见 `legacy/README.md`） |

## 开发流程（强制）

按模块分工、worktree 并行、独立评审后合并；详见 [`AGENTS.md`](AGENTS.md) 与
[`doc/development-workflow.md`](doc/development-workflow.md)。进度见 [`doc/progress.md`](doc/progress.md)，
遗留问题见 [`doc/issues.md`](doc/issues.md)。
