# zgraph · 图模型驱动工作流框架

用有向图描述可复用的工作流（SOP / Agent 工作流）：用户自定义图与节点，平台负责调度、状态、门控与可复现执行。
流程里可以跑 Agent，也可以只跑普通 Python 节点；**结果可复现是产品承诺**（L1 调度可重放 / L2 结果可重放）。

**技术形态（DR-001）**：**Rust 内核 + Python 子进程 worker**（单二进制内核；用户代码在子进程隔离执行）。

- 决策：[`doc/decisions/DR-001-runtime-route.md`](doc/decisions/DR-001-runtime-route.md)
- 架构：[`doc/architecture-v2.md`](doc/architecture-v2.md)（协议 v1 / 偏差表 D1–D12 / M0–M5 里程碑）
- 语义基准：[`doc/spec-v1.2/`](doc/spec-v1.2/README.md)
- 文档索引：[`doc/index.md`](doc/index.md)；流程约束：[`AGENTS.md`](AGENTS.md)

## 当前状态：v0.5.0（发布执行中；M0–M10：reexec + 预算/观测 + sandbox 增强 + 真实 Codex/DSH 验证）

**v0.5.0（M10）**：在 v0.4 内置工具/沙箱/trace v7 基础上，交付 5 项（F1–F5）：

- **reexec 工具回放（opt-in）**：`--mode=tools --replay-tools=reexec`；仅候选 `builtin__read/grep/glob/diff` + `status=ok` 重跑并比对 `result_sha256`；副作用/MCP/Python/失败行**复用记录、绝不执行**；默认 `recorded` 逐字节不变；不升 v8。
- **可观测/成本**：run/node/agent token/耗时/费用；`--pricing <file>`（不内置价）；`--max-tokens`/`--max-cost` 默认关，超限 `BUDGET_EXCEEDED` + `Other("budget_exceeded")`（不可续）；外部 harness usage unavailable 如实标注。
- **sandbox 增强**：`--allow-write-path` 独立可写白名单（profile+jail、不可 `/`、同目录写优先）；`--allow-exec-path`/构建工具链路径、`cc -isysroot` 文档；默认无新开关行为不变。
- **真实 Codex/DSH 验证**：DSH ACP transport 已落地（单测 16；真实握手无 key 可跑；全链路需 `DEEPSEEK_API_KEY`）；Codex **零密钥 mock CI**（CODEX_HOME + 本地 Responses SSE；3 gated：2 pass + F5f nested skip）；F5f 工具事件需非嵌套 `-s danger-full-access`。
- **F4**：kernel/worker/e2e flake 加固（ISSUE-176/177/178 fixed）。
- **CLI 示例**：
  ```bash
  zgraph run graph.json --fake-llm --allow-builtin-tools --workspace-dir ./ws
  zgraph replay --run <run_id> --mode=tools --replay-tools=reexec --allow-builtin-tools --workspace-dir ./ws
  zgraph run graph.json --fake-llm --max-tokens 10000 --pricing prices.json
  zgraph run graph.json --fake-llm --allow-builtin-tools --allow-bash --allow-write-path ./cache --workspace-dir ./ws
  ```
- **测试（已实测）**：`cargo test --workspace` **1295/0/6**；e2e **25 target / 174/0/5 ignored**（含 v0.5 reexec 7 / Codex mock 3）；pytest **203/2 skipped**（collect 205）；`cargo test -p zgraph-harness` **386/0/1**；`cargo test -p zgraph-cli` **270/0/0**。
- **性能**：[`doc/perf-baseline-v0.5.txt`](doc/perf-baseline-v0.5.txt)（7-trial：AC-01 −1.5%、AC-12 +0.3%、SOP smoke +5.5%，门禁通过；full-60 validate 0.0%、运行期 ISSUE-112 例外；首轮负载噪声已注）。
- **发布产物/状态**：`dist/zgraph-v0.5.0-*.tar.gz` + sha256、`dist/RELEASE_NOTES-v0.5.0.md` 由发布流程构建；tag `v0.5.0`（**发布执行中**，用户授权后创建）；GitHub Release 由发布流程创建。
- **已知限制**：DSH 全链路需 `DEEPSEEK_API_KEY`（用户授权）；Codex 真 key/F5f 工具执行需非嵌套环境；bash 真实隔离需用户普通终端 `verify-sandbox.sh`；外部 harness usage/cost unavailable（不伪造）；cargo/node 默认不可执行；hardlink/TOCTOU 残余；trace 无 FK 清理；W3b durable resume→**v0.6**；ISSUE 台账见 [issues](doc/issues.md)。
- **下一步**：v0.6（W3b durable resume + 剩余 ISSUE）；详见 [DR-016](doc/decisions/DR-016-v05-scope-and-waves.md) / [progress](doc/progress.md)。

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
