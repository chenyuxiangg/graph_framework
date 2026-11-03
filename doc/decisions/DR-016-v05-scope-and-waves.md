# DR-016 · v0.5（M10）范围与波次（reexec / ISSUE 清理 / sandbox 增强 / 真实 Codex·DSH / 可观测成本；v0.6=W3b）

> **状态**：V1 设计冻结（REV-115；Codex/DSH 真实验证细节待两个源码仓 spike 结论，DR-020 标「待 spike」）。
> **日期**：2026-11-03 · **作者**：docs-dev · **关联**：DR-017/018/019/020、DR-012/013/014/015、task-95/96
> **用户决策（task-95）**：v0.5 六项全选，分 **v0.5.0（5 项）** + **v0.6.0（W3b durable resume）**；W5 已取消（DR-015）。

## 0. 摘要

| 项 | 冻结 |
|---|---|
| v0.5.0 范围 | ① `reexec` 工具回放（DR-017）；② ISSUE-176–181 清理（F4）；③ sandbox 增强（`--allow-write-path`/构建工具链，DR-019）；④ 真实 Codex/DSH 验证（DR-020）；⑤ 可观测性/成本预算（DR-018） |
| v0.6.0 | **W3b durable resume/断点续跑**（内核级；另立 DR-021 设计冻结后实现，不阻塞 v0.5.0） |
| 默认不变 | 无 reexec/无预算/无新 sandbox 开关/无外部 harness：行为/trace/replay 逐字节不变；DSL 1.0/R1–R18/19 码不变；trace **不升 v8** |
| 新增 | CLI `--replay-tools=reexec`（显式）、`--allow-write-path`、预算 flags、metrics/成本字段（additive）；AC-25–28 |
| 兼容红线 | 错误码/reason/CLI 码只追加；reexec **永不用于副作用工具**；预算默认关；trace v7 不破坏（v8 仅 v0.6 评估） |
| 发布 | v0.5.0 性能门禁 ≤20%（默认路径）；测试基线只缩不增；用户终端 bash 隔离证据在发布前补（ISSUE-180） |

## 1. 范围

### 1.1 v0.5.0（5 项）

1. **`reexec` 回放**（DR-017）：显式 `--replay-tools=reexec`；只读工具 read/grep/glob/diff；输入/结果漂移校验；副作用工具永远 recorded-only；`REPLAY_TOOL_MISMATCH` fail-closed；不升 v8。
2. **ISSUE-176–181 清理**（F4）：
   - ISSUE-176 kernel_cancel 时序 flake、ISSUE-177 ac09 parallel timing flake、ISSUE-178 codex adapter e2e 并行 flake：加固/隔离/有界等待；
   - ISSUE-179 `tool_calls` 无 FK 清理：评估 `ON DELETE CASCADE` 或清理命令（v7 迁移兼容）；
   - ISSUE-180 bash 真实隔离用户终端证据；
   - ISSUE-181 v0.3/v0.4 残余逐项复核（fixed/延后/关闭）。
3. **sandbox 增强**（DR-019）：`--allow-write-path <dir>` 独立可写白名单（profile file-write* + jail 写授权）；构建工具链 cargo/node/uv 可用路径/`TMPDIR`/`HOME`/系统集合策略；`cc -isysroot` 文档；allow-path/bash 组合语义。
4. **真实 Codex/DSH 验证**（DR-020）：从用户指定源码仓 clone/构建/运行（Codex `https://github.com/openai/codex`、DSH `https://github.com/deepseek-ai/deepseek-harness`），验证外部 harness 适配器真链路；证据/限制如实记录，不伪造。
5. **可观测性/成本预算**（DR-018）：run/node/agent 维度 token/费用/耗时统计；预算上限（`--max-tokens`/`--max-cost`，默认关）与停止语义；trace/CLI additive，不升 v8。

### 1.2 v0.6.0

- **W3b durable resume/断点续跑**：内核级（状态持久化/恢复语义/trace 兼容）；**独立 DR-021 设计冻结后实现**；不进入 v0.5.0 范围；若需 trace v8 在 v0.6 单独评估。

### 1.3 非目标

- W5 共享评审队列：**已取消**（DR-015），不在任何后续版本。
- 多模态；工具并行；MCP OAuth/DCR；Windows 沙箱；无沙箱 bash fallback；域名级网络白名单。
- v0.5 不做 durable resume（v0.6）；不承诺 v0.6 时间。

## 2. 波次（F0–F6）

```
F0 docs 设计冻结（本批，REV-115）
 ├─▶ F1 reexec（trace/harness/cli）
 ├─▶ F2 sandbox 增强（harness/cli）
 ├─▶ F3 可观测/成本（harness/trace/cli）
 ├─▶ F4 ISSUE 清理（kernel/e2e/trace/docs）
 └─▶ F5 真实 Codex/DSH 验证（e2e/research）
        └─▶ F6 发布 v0.5.0（docs/process；tag/GitHub Release 用户授权）
```

> **并行约束**：F1/F2 同写 `crates/zgraph-harness` → **串行**（F1 先或 F2 先，由 Lead 排期）；F3 跨 harness/trace/cli（Lead 批准 + 第二评审）；F4/F5 可与 F1–F3 部分并行（F4 独立模块；F5 测试侧）。

| 波次 | 分支（建议） | 作者/评审 | 交付 | 依赖 |
|---|---|---|---|---|
| F0 | `docs/docs-v05-design` | docs-dev / reviewer-a + Lead 第二评审 | DR-016–020 + spec 00/02/05/06/07/08 草案 + AC-25–28 | 无 |
| F1 | `feat/cli-reexec`（trace/cli 为主） | trace-dev/cli-dev / reviewer-b | `--replay-tools=reexec`、只读白名单、漂移校验、mismatch fail-closed、e2e | F0；F2 同模块串行 |
| F2 | `feat/harness-sandbox-enhance` | harness-dev / reviewer-a | `--allow-write-path`、构建工具链、profile/jail、AC-26 负例 | F0；与 F1 串行 |
| F3 | `feat/observability-cost` | harness/trace/cli-dev / reviewer-b（跨模块 Lead 批准） | usage/token/费用/耗时聚合、预算 flags/停止语义、trace additive | F0 |
| F4 | `fix/issue-cleanup-v05` | 各模块作者 / reviewer-a/b | ISSUE-176–181 清理/复核 | F0 |
| F5 | `test/real-codex-dsh` | e2e-dev/research / reviewer-a | 源码构建/运行/真链路证据（环境门控/不伪造） | F0 + spike |
| F6 | `chore/process-v05-release` + `docs/docs-v05-spec` | docs-dev / reviewer-b | 版本/基线/README/progress/as-built/REV 发布 | F1–F5 |

## 3. 发布条件（v0.5.0）

1. AC-25（reexec）、AC-26（sandbox 增强）、AC-27（可观测/成本）、AC-28（真实 Codex/DSH，环境允许时）全部实测；不可用环境如实标注不伪造。
2. 默认路径（无 reexec/预算/新开关/外部 harness）兼容矩阵无新增失败；DSL/hash 不变；trace v7 不升 v8。
3. ISSUE-176–181 逐项 fixed/延后（不新增 P0/P1）。
4. `cargo test --workspace`/e2e/pytest/clippy/fmt/check.sh 全绿；性能回退 ≤20%（默认路径，v0.4 基线）。
5. 用户终端 bash 隔离证据（ISSUE-180）补入发布材料。
6. as-built 回写 00/02/05/06/07/08 + PROVENANCE；README/progress/发布材料齐备。

## 4. 兼容矩阵

| 场景 | 期望 |
|---|---|
| 默认（无 reexec/预算/新开关） | 行为/trace/replay 逐字节不变；trace v7 |
| `--replay-tools=recorded` | 与 v0.4 完全一致（默认） |
| `--replay-tools=reexec` 但工具为副作用 | 用法错误/拒绝（exit 2） |
| 预算未配置 | 不计算/不停止；输出不变 |
| `--allow-write-path` 未配置 | 写目标仅 workspace；默认不变 |
| 外部 harness 未配置 | 不启动进程；默认不变 |
| 旧 v7 库 | 读取兼容；不要求迁移 v8 |

## 5. AC-25–28 矩阵（设计冻结）

| AC | 内容 | 关键断言（摘要） | 波次 |
|---|---|---|---|
| AC-25 | reexec 回放 | 默认 recorded 不变；reexec 仅 read/grep/glob/diff；输入/结果漂移 → `REPLAY_TOOL_MISMATCH` fail-closed；副作用工具拒绝；无 v8 | F1 |
| AC-26 | sandbox 增强 | `--allow-write-path` 目录级可写（profile+jail）；与 workspace/allow-path 组合；构建工具路径/`cc -isysroot` 文档；负例越界拒绝 | F2 |
| AC-27 | 可观测/成本 | run/node/agent token/耗时/费用聚合；`--max-tokens/--max-cost` 默认关；超限停止语义（status/stop_reason/不可续）；trace/CLI additive；无配置不变 | F3 |
| AC-28 | 真实 Codex/DSH | 源码构建/运行/认证占位；最小真链路（Codex exec --json / DSH ACP）；证据/限制；不可用环境门控 `#[ignore]`/env；不伪造 | F5 |

## 6. 风险

| 风险 | 说明 | 缓解 |
|---|---|---|
| reexec 漂移 | 文件在记录与回放间变化 → 结果不同 | 默认 recorded；reexec 显式 + pre_sha256/result_sha256 校验；mismatch fail-closed；grep/glob 无输入 hash → 结果比对（DR-017 裁决） |
| 预算误停 | 价格配置错误/超限误判 | 默认关；tokens 权威、cost 需价格表；超限状态/错误码明确；文档 |
| allow-write-path 逃逸 | 可写白名单扩大攻击面 | 目录级 canonicalize；profile + jail 双强制；默认关；文档警示；AC-26 负例 |
| Codex/DSH 构建失败/认证 | 源码依赖重/需密钥 | 环境门控 `#[ignore]`/env；如实记录不可用；不伪造；adapter 骨架已存在 |
| ISSUE 清理引入回归 | flake 加固改断言/超时 | 每项独立评审；测试基线只缩不增；F4 独立分支 |
| 跨模块改动 | F1/F3/F5 跨 crate | Lead 批准 + 第二评审；串行/隔离 worktree |
| v0.6 依赖 | durable resume 内核级 | v0.5 不阻塞；v0.6 另立 DR-021/独立评审 |

## 7. 待 Lead 裁决项（详见各 DR）

| # | 事项 | 推荐 |
|---|---|---|
| 1 | reexec grep/glob 漂移口径 | 结果 sha256 比对；mismatch fail-closed（不升 v8） |
| 2 | 可观测字段位置 | 复用 `events.metrics`/`llm_calls`，不新建表、不升 v8 |
| 3 | 价格来源 | 配置文件（`--pricing <file>`），不内置价格 |
| 4 | 预算命名/单位/停止 | `--max-tokens`（token）/`--max-cost`（USD）；默认关；超限立即停止、RunStatus Failed + `BUDGET_EXCEEDED`、**不可续**（v0.6 durable resume 另议） |
| 5 | allow-write-path 重叠/嵌套 | 与 workspace 相同/嵌套允许（幂等）；不可为 `/`；文档警示；不与 allow-path 合并 |
| 6 | AC-28 环境门控/证据 | `#[ignore]` + env（如 `ZGRAPH_REAL_CODEX=1`/`ZGRAPH_REAL_DSH=1`）；发布材料记录构建/运行/限制，不可用如实标注 |
| 7 | F1/F2 顺序 | F1 先（trace/cli 为主），F2 后（harness 同模块串行） |
