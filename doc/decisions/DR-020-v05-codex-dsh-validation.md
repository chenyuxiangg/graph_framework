# DR-020 · v0.5 真实 Codex/DSH 验证方案（**待 spike**）

> **状态**：**草案，待两个源码仓 spike 结论**（openai/codex、deepseek-ai/deepseek-harness）；spike 到后补：构建/运行/认证/ACP 入口/与 adapter 映射/证据命令。
> **日期**：2026-11-03 · **作者**：docs-dev · **关联**：DR-011（外部 harness）、DR-016、07 AC-20/28、v0.5 F5

## 0. 摘要（占位）

| 项 | 计划 |
|---|---|
| 目标 | 从**源码仓** clone/构建/运行真实 Codex CLI 与 DSH，验证 `zgraph-harness/1` adapter 真链路（非 fake） |
| 源码 | Codex：https://github.com/openai/codex ；DSH：https://github.com/deepseek-ai/deepseek-harness |
| 证据 | 构建命令/版本、运行命令、最小图/AgentConfig、trace/llm_calls 片段、成功/失败日志；**不伪造**；不可用环境如实标注 |
| 门控 | e2e `#[ignore]` + env（如 `ZGRAPH_REAL_CODEX=1`/`ZGRAPH_REAL_DSH=1`）；CI 默认跳过；本地/发布材料手跑 |
| AC-28 | a–f（设计待 spike 补全） |

## 1. 计划步骤（待 spike 定稿）

1. **clone/构建**：Codex（Rust workspace；`cargo build --release`？）与 DSH（Python/Node？）；记录 commit/tag、工具链版本、构建产物路径。
2. **认证/环境**：API key/env 占位（不写 token）；网络/代理；HOME/TMPDIR；是否需要登录/设备码。
3. **入口**：Codex `exec --json`/`--output-last-message`；DSH `--profile acp`（v0.4 结论）；确认事件/退出码/取消行为。
4. **adapter 映射**：将真实事件映射为 `zgraph-harness/1`（现有 Codex/DSH adapter 骨架）；确认无双重 trace/状态机。
5. **最小真链路**：真 CLI + 真 Codex/DSH → run → trace v7 → replay（recorded）；记录耗时/成本限制。
6. **失败/降级**：构建失败/认证缺失/网络受限/超时/取消 → 如实记录；adapter 骨架保持 best-effort；不伪造成功。
7. **CI/发布证据**：`#[ignore]` + env 门控；发布材料附命令/输出摘要；环境不可用时标注「未验证」。

## 2. 已知风险（待 spike 确认）

| 风险 | 说明 | 缓解 |
|---|---|---|
| Codex 构建重/依赖 | Rust workspace/网络依赖；构建时间/磁盘 | 环境门控；记录失败；不阻塞 v0.5 其他项 |
| DSH 认证/入口 | ACP/headless 入口/依赖未确认 | spike 确认；不可用则只验证 Codex 或标注 |
| 认证/密钥 | 需 API key/登录 | env 占位；不写 token；缺认证不伪造 |
| 事件差异 | 真实事件与 fake/协议差异 | adapter 映射测试；记录差异/限制 |
| 成本 | 真实 LLM 调用费用 | 最小 prompt/预算；文档标注 |

## 3. AC-28（设计冻结，待 spike 细化）

| # | 断言（草案） |
|---|---|
| a | 源码 clone/构建成功（或如实记录失败原因/环境） |
| b | 真实入口可运行（Codex exec --json / DSH ACP），版本/commit 记录 |
| c | 最小真链路 run/trace/replay 成功（真实 adapter，非 fake） |
| d | 认证缺失/网络受限/构建失败 → 明确失败/跳过，不伪造 |
| e | 取消/超时无孤儿进程；trace 无双重记录 |
| f | CI 默认跳过（`#[ignore]`/env），发布材料证据齐全 |

## 4. 待 spike 后补充

- 具体构建/运行命令、版本、认证变量名、入口参数、事件样例、最小图/AgentConfig、证据路径、AC-28 用例名/计数。
- 与 DR-011 §3（Codex/DSH 映射）的差异修正。
