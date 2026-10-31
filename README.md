# zgraph · 图模型驱动工作流框架

用有向图描述可复用的工作流（SOP / Agent 工作流）：用户自定义图与节点，平台负责调度、状态、门控与可复现执行。
流程里可以跑 Agent，也可以只跑普通 Python 节点；**结果可复现是产品承诺**（L1 调度可重放 / L2 结果可重放）。

**技术形态（DR-001）**：**Rust 内核 + Python 子进程 worker**（单二进制内核；用户代码在子进程隔离执行）。

- 决策：[`doc/decisions/DR-001-runtime-route.md`](doc/decisions/DR-001-runtime-route.md)
- 架构：[`doc/architecture-v2.md`](doc/architecture-v2.md)（协议 v1 / 偏差表 D1–D12 / M0–M5 里程碑）
- 语义基准：[`doc/spec-v1.2/`](doc/spec-v1.2/README.md)
- 文档索引：[`doc/index.md`](doc/index.md)；流程约束：[`AGENTS.md`](AGENTS.md)

## 当前状态：v0.2（M7）W2a0 + W2a 已完成

**W2a0（设计冻结，REV-067）**：[DR-005](doc/decisions/DR-005-w2a-defer-gate-freeze.md) + spec 00–08 回写（`on_gate_false`、R15/`DSL_DEFER_NOT_REENTRANT`、trace schema v4、SOP v1.1 双 profile、07 AC-14）。

**W2a（实现，REV-068–072）**：dsl/trace/py-worker/kernel+cli 已合入 `main`（`e7578f1` 时点）；e2e 分支 `test/e2e-gate-defer`（REV-072 已通过；待合并）：`gate_defer` 8/0、`software_team_v1` 11/1 ignored；c/e 运行期 `--profile rework-item` 5/7；e2e 58/0/2、空载 workspace 694/0/3（最终计数以合并时为准）。证据见 [07 AC-14](doc/spec-v1.2/07-验收用例.md)、[PROVENANCE](doc/spec-v1.2/PROVENANCE.md)、[progress](doc/progress.md)。

**下一步**：W2b（每节点最大扇出限制）→ W3a0 → W3a → W3b(可选) → W4a → W4b（[DR-004](doc/decisions/DR-004-v02-scope-and-waves.md) §4）。

**历史里程碑（摘要）**：M0–M6 已完成——Rust 内核 + Python 子进程 worker、DSL loader/校验、内核事件循环、trace/SQLite（L1 回放）、Harness（LLM 流式 + tool loop）、CLI、e2e；M6 收口 trace 双通道/fail-stop/退出码/run 版本列/`global_vars` schema v3 等（REV-039–061）；v0.1.0 已发布（tag `v0.1.0`）；W1 SOP v1（REV-064–066）。遗留项见 [issues](doc/issues.md)。

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
| `crates/zgraph-dsl/` | DSL 类型、loader、R1–R15（W2a 增 R15）、graph_hash（M1） |
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
