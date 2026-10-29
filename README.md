# zgraph · 图模型驱动工作流框架

用有向图描述可复用的工作流（SOP / Agent 工作流）：用户自定义图与节点，平台负责调度、状态、门控与可复现执行。
流程里可以跑 Agent，也可以只跑普通 Python 节点；**结果可复现是产品承诺**（L1 调度可重放 / L2 结果可重放）。

**技术形态（DR-001）**：**Rust 内核 + Python 子进程 worker**（单二进制内核；用户代码在子进程隔离执行）。

- 决策：[`doc/decisions/DR-001-runtime-route.md`](doc/decisions/DR-001-runtime-route.md)
- 架构：[`doc/architecture-v2.md`](doc/architecture-v2.md)（协议 v1 / 偏差表 D1–D12 / M0–M5 里程碑）
- 语义基准：[`doc/spec-v1.2/`](doc/spec-v1.2/README.md)
- 文档索引：[`doc/index.md`](doc/index.md)；流程约束：[`AGENTS.md`](AGENTS.md)

## 当前状态：M3 worker 完成

**已完成 M0–M3**：Cargo workspace（6 个 crate + `tests/e2e` 验收包）、DSL loader/校验（R1–R14）、
图内核与事件循环、trace/SQLite（L1 回放）、**worker supervisor + `WorkerExecutor`**（进程组 kill、
取消升级、帧超时、并发上限）、`python/workers` 示例、spec v1.2 的 M0–M2 回写、
`tests/e2e/**` 真子进程验收（AC-06/07/09/10 + 故障注入）；本地双栈测试全绿（`cargo test --workspace` / `pytest`），CI 已配置（`.github/workflows/ci.yml`）。

**尚未实现（M4–M5）**：Agent Harness（LLM 流式/tool/预算）、CLI `run`/`replay`、多 run 隔离（AC-12）、
legacy 退役决策；M3 遗留项见 [`doc/issues.md`](doc/issues.md)（ISSUE-061 P2 / ISSUE-063 P3 / ISSUE-066 P2 等）。

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
| `crates/zgraph-worker/` | worker 协议、supervisor（进程组/取消/帧超时）、`WorkerExecutor`（M3） |
| `crates/zgraph-harness/` | Agent Harness（M4） |
| `crates/zgraph-cli/` | `zgraph` 命令（M1：`validate`；M2：`trace`；`run`/`replay` 见 M5） |
| `python/zgraph_worker/` | worker SDK：握手、ctx、运行时 |
| `python/workers/` | 示例 worker：echo/slow/parallel/cycle/software_team（M3） |
| `schemas/` | worker 协议 v1（M0 冻结，M3 检查单闭环）；DSL JSON Schema 未实现（ISSUE-029） |
| `tests/e2e/` | 真子进程验收：AC-06/07/09/10 + 故障注入 + 并发上限（M3） |
| `tests/fixtures/` | 图与 worker fixture |
| `doc/` | 决策/架构/规格/评审/进度/台账 |
| `legacy/` | 旧 Python 实现（冻结只读，见 `legacy/README.md`） |

## 开发流程（强制）

按模块分工、worktree 并行、独立评审后合并；详见 [`AGENTS.md`](AGENTS.md) 与
[`doc/development-workflow.md`](doc/development-workflow.md)。进度见 [`doc/progress.md`](doc/progress.md)，
遗留问题见 [`doc/issues.md`](doc/issues.md)。
