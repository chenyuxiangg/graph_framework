# zgraph · 图模型驱动工作流框架

用有向图描述可复用的工作流（SOP / Agent 工作流）：用户自定义图与节点，平台负责调度、状态、门控与可复现执行。
流程里可以跑 Agent，也可以只跑普通 Python 节点；**结果可复现是产品承诺**（L1 调度可重放 / L2 结果可重放）。

**技术形态（DR-001）**：**Rust 内核 + Python 子进程 worker**（单二进制内核；用户代码在子进程隔离执行）。

- 决策：[`doc/decisions/DR-001-runtime-route.md`](doc/decisions/DR-001-runtime-route.md)
- 架构：[`doc/architecture-v2.md`](doc/architecture-v2.md)（协议 v1 / 偏差表 D1–D12 / M0–M5 里程碑）
- 语义基准：[`doc/spec-v1.2/`](doc/spec-v1.2/README.md)
- 文档索引：[`doc/index.md`](doc/index.md)；流程约束：[`AGENTS.md`](AGENTS.md)

## 当前状态：v0.2.0 已发布（M0–M7 完成）

**v0.2.0**（tag `v0.2.0`；发布说明 `dist/RELEASE_NOTES-v0.2.0.md`，产物 `dist/zgraph-v0.2.0-*.tar.gz` + sha256，由 Lead 构建）：

- **内核与执行**：Rust 内核 + Python 子进程 worker；DSL 1.0 / R1–R18（19 个 DSL 码）；事件循环 + gate/内存通道 + SQLite trace v6。
- **SOP 与能力**：software_team SOP v1/v1.1（approved/rework）+ **动态 N `foreach`**（W4b，AC-17 a–j）；**人工评审 human decide**（W3a，AC-16）；每节点扇出上限（W2b，AC-15）；L1/L2 回放。
- **测试**：`cargo test --workspace` **915 passed / 0 failed / 3 ignored**；e2e **106/0/2**（`dynamic_fanout` **21/0**，6 连跑稳定）；pytest **170 passed**（`python -m pytest -q`；collect-only 170 tests）。
- **性能基线**：[`doc/perf-baseline-v0.2.txt`](doc/perf-baseline-v0.2.txt)（customer_ticket +17.3%、10 并发 +16.0%、SOP smoke -14.8%，均 ≤20% 门禁内）。
- **已知限制**：ISSUE-112（SOP full-60 运行期无确定性 fake-LLM 基线，发布例外）；**W3b durable resume 不做**（用户 2026-11-01 决定）；W5 共享评审队列延至 v0.3；P3 清理项见 [issues](doc/issues.md) ISSUE-143–149。
- **下一步**：v0.3 规划（W5 共享评审队列）+ 清理 ISSUE-112/143–149；详见 [DR-004](doc/decisions/DR-004-v02-scope-and-waves.md) §4/§10 与 [progress](doc/progress.md)。

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
| `crates/zgraph-dsl/` | DSL 类型、loader、R1–R16（W2a 增 R15、W2b 增 R16）、graph_hash（M1） |
| `crates/zgraph-kernel/` | 状态机、事件循环、Delivery、门控、run 生命周期（M2）；W2b 每激活扇出上限（运行期防御层） |
| `crates/zgraph-trace/` | TraceEvent、SQLite、回放（M2） |
| `crates/zgraph-worker/` | worker 协议、supervisor（进程组/取消/帧超时）、`WorkerExecutor`（M3）；M4 `WorkerFailure::Worker` 装箱 |
| `crates/zgraph-harness/` | Agent Harness（M4）：AgentConfig/预算/LLM(fake+genai 0.7.2)/tool loop/`HarnessExecutor` |
| `crates/zgraph-cli/` | `zgraph` 命令（M1：`validate`；M2：`trace`；M5：`run` 已交付、`replay` 见 wave2） |
| `python/zgraph_worker/` | worker SDK：握手、ctx、运行时；M4 `tool_runner`（discover/call） |
| `python/workers/` | 示例 worker：echo/slow/parallel/cycle/software_team（M3） |
| `schemas/` | worker 协议 v1（M0 冻结，M3 检查单闭环）；DSL JSON Schema 未实现（ISSUE-029） |
| `tests/e2e/` | 真链路验收：AC-06/07/09/10 + 故障注入 + 并发（M3）；AC-08 fake LLM/tool/流式（M4）；AC-12 多 run 隔离（M5）；AC-14 defer/返工（W2a）；AC-15 max_fanout 静态反例/正例/运行期防御层（W2b） |
| `tests/fixtures/` | 图与 worker fixture |
| `doc/` | 决策/架构/规格/评审/进度/台账 |
| `legacy/` | 旧 Python 实现（冻结只读，见 `legacy/README.md`） |

## 开发流程（强制）

按模块分工、worktree 并行、独立评审后合并；详见 [`AGENTS.md`](AGENTS.md) 与
[`doc/development-workflow.md`](doc/development-workflow.md)。进度见 [`doc/progress.md`](doc/progress.md)，
遗留问题见 [`doc/issues.md`](doc/issues.md)。
