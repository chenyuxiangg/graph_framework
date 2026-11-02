# zgraph · 图模型驱动工作流框架

用有向图描述可复用的工作流（SOP / Agent 工作流）：用户自定义图与节点，平台负责调度、状态、门控与可复现执行。
流程里可以跑 Agent，也可以只跑普通 Python 节点；**结果可复现是产品承诺**（L1 调度可重放 / L2 结果可重放）。

**技术形态（DR-001）**：**Rust 内核 + Python 子进程 worker**（单二进制内核；用户代码在子进程隔离执行）。

- 决策：[`doc/decisions/DR-001-runtime-route.md`](doc/decisions/DR-001-runtime-route.md)
- 架构：[`doc/architecture-v2.md`](doc/architecture-v2.md)（协议 v1 / 偏差表 D1–D12 / M0–M5 里程碑）
- 语义基准：[`doc/spec-v1.2/`](doc/spec-v1.2/README.md)
- 文档索引：[`doc/index.md`](doc/index.md)；流程约束：[`AGENTS.md`](AGENTS.md)

## 当前状态：v0.3.0 发布收口（M8：skills + MCP + 可插拔 harness）

**v0.3.0（M8）**：能力扩展三件已实现并合并（V2 skills / V3 MCP / V4 harness 插件）：

- **skills**：`<assets-dir>/skills/<name>/SKILL.md`（frontmatter 校验）、system 仅注入 catalog、`skill_view(name)` 按需加载正文（32K chars、run 级 5 次预算）、`--skills-dir`/`ZGRAPH_SKILLS`；不执行 skill 脚本、正文按不可信数据。
- **MCP**：rmcp 3.8.0 **双 era**（`2026-07-28` 无状态 + `2025-11-25` legacy）、stdio + Streamable HTTP、`mcp_servers.json`、`mcp__<server>__<tool>`（必须显式进 `AgentConfig.tools`）、`--allow-mcp`、静态 env Bearer、默认 https；trace **不 v7**。
- **可插拔 harness**：`AgentProvider`（Builtin 默认 / Subprocess）、`zgraph-harness/1` NDJSON 协议、Codex adapter（`exec --json`/`--output-last-message`；无 token delta、iterations=1、cached tokens 丢弃）、DSH ACP 骨架；外部 harness 不写 trace/状态机、L2 不重跑。
- **测试（已实测）**：`cargo test -p zgraph-e2e` **137/0/2**（V5a 四 target 连跑 3 次）；`cargo test --workspace` **1037/0/3**；pytest **183**；`cargo test -p zgraph-harness` **235/0/1**（`--features mcp` 235/0/1）；V5a AC-18/19/20 + 回归 **31 用例**。
- **发布产物**：`dist/zgraph-v0.3.0-*.tar.gz` + sha256 由发布流程构建；性能基线由发布收口实测（v0.2 基线见 [`doc/perf-baseline-v0.2.txt`](doc/perf-baseline-v0.2.txt)，v0.3.0 基线待发布确认）。
- **已知限制**：真实 Codex/DSH 环境**未验证**（adapter 骨架，release notes 标注）；W5 共享评审队列延至 v0.4；P2/P3 台账见 [issues](doc/issues.md) ISSUE-156–163。
- **下一步**：v0.4（M9）W5 共享评审队列 + ISSUE 清理；详见 [DR-008](doc/decisions/DR-008-v03-scope-and-waves.md) / [progress](doc/progress.md)。

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
