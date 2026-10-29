# zgraph · 图模型驱动工作流框架

用有向图描述可复用的工作流（SOP / Agent 工作流）：用户自定义图与节点，平台负责调度、状态、门控与可复现执行。
流程里可以跑 Agent，也可以只跑普通 Python 节点；**结果可复现是产品承诺**（L1 调度可重放 / L2 结果可重放）。

**技术形态（DR-001）**：**Rust 内核 + Python 子进程 worker**（单二进制内核；用户代码在子进程隔离执行）。

- 决策：[`doc/decisions/DR-001-runtime-route.md`](doc/decisions/DR-001-runtime-route.md)
- 架构：[`doc/architecture-v2.md`](doc/architecture-v2.md)（协议 v1 / 偏差表 D1–D12 / M0–M5 里程碑）
- 语义基准：[`doc/spec-v1.2/`](doc/spec-v1.2/README.md)
- 文档索引：[`doc/index.md`](doc/index.md)；流程约束：[`AGENTS.md`](AGENTS.md)

## 当前状态：M0 骨架

已有：Cargo workspace（6 个 crate）、worker 协议 v1（Rust 客户端 + Python SDK）、echo/slow 示例 worker、
双栈测试（`cargo test` + `pytest`）、CI、legacy Python 实现归档只读。

尚未实现（M1–M5）：DSL loader/校验、图内核与调度、trace/SQLite、Agent Harness、CLI 业务命令、replay。

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
| `crates/zgraph-worker/` | worker 协议客户端、子进程生命周期（M0 已实现） |
| `crates/zgraph-harness/` | Agent Harness（M4） |
| `crates/zgraph-cli/` | `zgraph` 命令（M0 仅 `--version`） |
| `python/zgraph_worker/` | worker SDK：握手、ctx、运行时、示例 worker |
| `schemas/` | DSL schema（M1）、worker 协议 v1 |
| `tests/` | 新 e2e 与 fixture |
| `doc/` | 决策/架构/规格/评审/进度/台账 |
| `legacy/` | 旧 Python 实现（冻结只读，见 `legacy/README.md`） |

## 开发流程（强制）

按模块分工、worktree 并行、独立评审后合并；详见 [`AGENTS.md`](AGENTS.md) 与
[`doc/development-workflow.md`](doc/development-workflow.md)。进度见 [`doc/progress.md`](doc/progress.md)，
遗留问题见 [`doc/issues.md`](doc/issues.md)。
