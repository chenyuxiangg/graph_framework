# zgraph · 图模型驱动工作流框架

用有向图描述可复用的工作流（SOP / Agent 工作流）：用户自定义图与节点，平台负责调度、状态、门控与可复现执行。
流程里可以跑 Agent，也可以只跑普通 Python 节点；**结果可复现是产品承诺**（L1 调度可重放 / L2 结果可重放）。

**技术形态（DR-001）**：**Rust 内核 + Python 子进程 worker**（单二进制内核；用户代码在子进程隔离执行）。

- 决策：[`doc/decisions/DR-001-runtime-route.md`](doc/decisions/DR-001-runtime-route.md)
- 架构：[`doc/architecture-v2.md`](doc/architecture-v2.md)（协议 v1 / 偏差表 D1–D12 / M0–M5 里程碑）
- 语义基准：[`doc/spec-v1.2/`](doc/spec-v1.2/README.md)
- 文档索引：[`doc/index.md`](doc/index.md)；流程约束：[`AGENTS.md`](AGENTS.md)

## 当前状态：v0.4.0 已发布（M0–M9：内置工具 + workspace 沙箱 + trace v7）

**v0.4.0（M9）**：内置工具、严格 workspace jail、macOS OS 沙箱与工具级 trace/replay 已实现并合并（D1–D5c）。

- **内置工具（opt-in）**：`builtin__read/write/edit/apply_patch/diff/grep/glob/bash`（Rust；工具名 `builtin__*`）。
- **严格 jail**：`--workspace-dir`（缺省 `.`，canonicalize）内读写；`--allow-path` 为**只读**白名单；写目标仅 workspace；越界/symlink/`..` 拒绝。
- **bash OS 沙箱**：macOS `sandbox-exec`（spike 79 行 profile）；网络默认 deny（`--allow-network` 全开）；`--allow-exec-path` 扩展可执行目录；嵌套 Seatbelt/沙箱不可用 **fail-closed**（预检 exit 3、不产生 run），不提供无沙箱 fallback。
- **trace v7**：新增 `tool_calls` 表（`error_sha256`/bash 审计字段）；`--mode=tools` 默认 **recorded-only 回放**（绝不执行工具/副作用）；`reexec` v0.4 未实现（exit 2，v0.5）。
- **CLI 示例**：
  ```bash
  zgraph run graph.json --fake-llm --allow-builtin-tools --workspace-dir ./ws
  zgraph run graph.json --fake-llm --allow-builtin-tools --allow-bash --workspace-dir ./ws
  zgraph replay --run <run_id> --mode=tools --replay-tools=recorded
  ```
- **测试（已实测）**：`cargo test -p zgraph-harness` **339/0/1**；`cargo test --workspace` **1216/0/3**；pytest **183**；e2e **23 target / 166/0/2**（v0.4 29 用例/5 target = D5c 28 + REV-112 补 1；v0.3 137 + v0.4 29 = 166）。
- **性能**：[`doc/perf-baseline-v0.4.txt`](doc/perf-baseline-v0.4.txt)（5-trial：AC-01 +0.0%、AC-12 −0.7%、SOP smoke +0.0%，门禁通过；full-60 validate +6.7%、运行期 ISSUE-112 例外）。
- **发布产物/状态**：`dist/zgraph-v0.4.0-*.tar.gz` + sha256、`dist/RELEASE_NOTES-v0.4.0.md` 由 Lead/发布流程构建；tag `v0.4.0`（**发布流程执行中**，用户授权后创建）；GitHub Release 由发布流程创建。
- **已知限制**：bash 仅 macOS（嵌套/不可用 fail-closed；真实隔离证据需用户普通终端 `crates/zgraph-harness/scripts/verify-sandbox.sh`）；cargo/node 等构建工具默认不可执行；hardlink/TOCTOU 残余风险；trace 无 FK 删除清理；`reexec` v0.5；W5 共享评审队列延至 v0.5；ISSUE-176+ 台账见 [issues](doc/issues.md)。
- **下一步**：v0.5（W5 + 真实 Codex/DSH 验证 + ISSUE 清理）；详见 [DR-012](doc/decisions/DR-012-v04-scope-and-waves.md) / [progress](doc/progress.md)。

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
| `tests/e2e/` | 真链路 e2e（真 CLI + 真 worker + SQLite）：W1–W4b + **v0.3 V5a** `skills_v03.rs`/`mcp_v03.rs`/`harness_v03.rs`/`v03_regression.rs`（AC-18/19/20 + 回归 31 用例） |
| `tests/fixtures/` | 图与 worker fixture |
| `doc/` | 决策/架构/规格/评审/进度/台账 |
| `legacy/` | 旧 Python 实现（冻结只读，见 `legacy/README.md`） |

## 开发流程（强制）

按模块分工、worktree 并行、独立评审后合并；详见 [`AGENTS.md`](AGENTS.md) 与
[`doc/development-workflow.md`](doc/development-workflow.md)。进度见 [`doc/progress.md`](doc/progress.md)，
遗留问题见 [`doc/issues.md`](doc/issues.md)。
