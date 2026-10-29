# REV-014 · docs · spec v1.2 M0–M2 回写（D1–D12 与 §8.2）

> **模块声明**：本分支只改 `doc/spec-v1.2/**`（00/01/02/03/04/06/07、PROVENANCE、README，9 文件）与 `doc/architecture-v2.md`（§8/§8.2 两处标注）；未改 `crates/**`、`python/**`、`tests/**`，未改 `doc/issues.md` / `doc/progress.md` / `doc/reviews/README.md`（符合 docs 模块边界与任务约束）。作者 `docs-writer`，评审人 `reviewer-b`（非作者，独立评审）。

- **分支**：`feat/docs-m3-spec-writeback`
- **作者**：`docs-writer`
- **评审人**：`reviewer-b`
- **变更范围**：`a6f4f5c..2863490`（`git merge-base origin/main HEAD` → HEAD，1 commit：`docs(docs): spec v1.2 M0–M2 回写（D1–D12 与 §8.2）`；`git diff --stat` = 10 files, +488/−357）
- **评审时间**：2026-10-30
- **关联**：task-3（ISSUE-020/050 M0–M2 回写批次）；`doc/architecture-v2.md` §8 D1–D12 / §8.2；ISSUE-026/027/033/034/035/036/037/043/046/048/053/054（并行 ISSUE-059）；`doc/issues.md` ISSUE-050

## 1. 结论

**结论：有条件通过**

回写的**主体质量高**：D1–D12/§8.2 已逐项落到正文，`PROVENANCE` 的 9 个 SHA-256 与行数**逐项实测一致**（00–08 合计 **5527 行**），`README` 分册行数表同步；`check.sh` 通过；范围严格限定在 docs。但独立核对发现 **4 项重要问题**（均为纯文档修正，不涉及代码）：① 回写记录被 9 处引用为 `PROVENANCE §2.5`，而 PROVENANCE **不存在 §2.5 小节**（§2 是编号列表 1–5，回写批次是第 4 条）——交叉引用全部悬空；② 02/07 把 `DSL_SYMBOL_*` 绝对化为“只由内置 gate op 触发”，与实现（`python_node` 的 `module/function` **格式**非法仍报 `DSL_SYMBOL_NOT_FOUND`）及 02 自身 R14/§2.5.2 矛盾；③ 04 §4.9 对齐表 A2/A7/A17 未随本次回写同步，与 04 新正文及 00 §0.3/§0.5 自相矛盾；④ 04 §4.3.3/§4.3.4 的 Python 异常/包名仍写 v1 的 `zgraph.*`，与 v2 SDK（`zgraph_worker.*`）不一致，且 `ProtocolError` 被写成 `ValueError` 子类（实现为 `Exception` 子类）。

**以下 4 项为合并前置条件**（纯文档文本修正，关闭后由作者提交、评审确认即通过；建议同一提交内一并处理 §3 的建议项）：

| 条件 | 内容 |
|---|---|
| C1 | 修正 `PROVENANCE §2.5` 悬空引用（9 处，见 §3 #1） |
| C2 | 修正 02/07 “`DSL_SYMBOL_*` 只由内置 gate op 触发”的绝对化表述，覆盖 `python_node` 格式非法的 `DSL_SYMBOL_NOT_FOUND`（见 §3 #2） |
| C3 | 同步 04 §4.9 对齐表 A2/A7/A17 与节首结论（见 §3 #3） |
| C4 | 对齐 04 §4.3.3/§4.3.4 的异常名/包名与 v2 SDK（含 `ProtocolError` 基类口径）（见 §3 #4） |

## 2. 四维评审

| 维度 | 结论 | 依据（文件/行/测试） |
|---|---|---|
| 功能正确性 | ⚠️ | 回写内容与实现逐项可核验：`EdgeIdx=usize`/`EdgeIndex` 结构体与 `crates/zgraph-kernel/src/{types.rs:36,graph.rs:98}` 一致；`WorkerErrorKind` 4 变体与 `NODE_WORKER_*` 映射一致（`types.rs:355-375`）；`RunMetrics` 4 字段（含 `killed_workers`）与 kernel/trace 实现一致；`NodeError→JSON` canonical 形状与 CLI `bridge.rs:457-497` **逐键一致**；trace `runs` 的 `seed/record_mode/killed_workers` 列（`schema.rs:58-64`）、`TRACE_PAYLOAD_MAX_BYTES=65536`、M2 单通道 8192（`writer/mod.rs:48`）、`RunRow` 无 `owner_pid`（06 §6.8.3 声明属实）、协议终止序 `cancel→2s→SIGTERM→1s→SIGKILL`（`schemas/worker-protocol-v1.md:45`）均核对一致；hash/行数实测一致。扣分项：C2/C4 的契约表述与实现不符，C1/C3 为引用/表格自洽问题。 |
| 代码结构合理性 | ✅ | 10 文件、+488/−357，全部在 `doc/spec-v1.2/**` + `doc/architecture-v2.md`；未触碰代码/台账/进度/评审索引；banner 统一、PROVENANCE/README 入口分工清晰；`architecture-v2.md` 仅两处标注（§8 回写状态 + §8.2 尾部），保留为决策记录的处理正确。 |
| 逻辑性 | ⚠️ | 跨册主干自洽：EdgeIdx/EdgeIndex、NodeError::NodeWorker、killed_workers/abandoned_tasks、5 个新错误码、D1 超时语义（03/04/07 一致）、并发默认值（00/03/04/07 均 CPU×2 + 每 run 8）均可交叉核对；ISSUE 编号逐一存在。扣分项：C1 悬空引用、C3 对齐表自相矛盾、C2 与 R14/§2.5.2 内部冲突。 |
| 可读性 | ✅ | 分册结构、表格、`为什么/口径`说明清晰；ISSUE 引用密集且有定位；D 编号/REV 溯源完整。小项：04:581 Markdown 粗体不配对（§3 #7）、“派发前”残留措辞（§3 #6）。 |

## 3. 发现清单

| # | 级别 | 位置 | 问题 | 建议 | 状态 |
|---|---|---|---|---|---|
| 1 | 重要（C1） | `doc/spec-v1.2/PROVENANCE.md:14-33`（§2 无 §2.5 小节）；引用处：`00-总纲.md:2`、`02-DSL与校验.md:3`、`03-调度器与生命周期.md:3`、`04-节点接口与扩展点.md:3`、`06-存储与可观测.md:3`、`07-验收用例.md:3`、`README.md:61`、`doc/architecture-v2.md:239,278` | 9 处写“见 `PROVENANCE §2.5` / `PROVENANCE.md §2.5`”，但 PROVENANCE 只有 `## 1`–`## 5`，§2 是编号列表 1–5；M0–M2 回写批次实际是 **§2 第 4 条**（`:24`），也不存在第 5 条小节。所有回写溯源链接悬空。 | 二选一：① 把引用统一改为“`PROVENANCE §2` 第 4 条”；② 在 PROVENANCE 中把回写批次提升为 `### 2.5 M0–M2 回写批次` 小节（§2 改为带小节的结构）。改动后重跑 hash/行数核对（PROVENANCE 自身不参与 §3 表）。 | open |
| 2 | 重要（C2） | `02-DSL与校验.md:531`（§2.5.4）、`:426`（§2.4.2 模板）、`07-验收用例.md:32`（AC-04 M2 列）；对照 `02-DSL与校验.md:406`（R14）、`:506`（§2.5.2）与实现 `crates/zgraph-dsl/src/symbols.rs:106-116,166-172,239-252` | §2.5.4/§2.4.2/07 声称 `DSL_SYMBOL_*` “**仅**内置 gate op 路径”（07 写“D7 调整后 `DSL_SYMBOL_*` 只由内置 gate op 触发”），但实现中 `python_node` 的 `module/function` **格式**非法仍报 `DSL_SYMBOL_NOT_FOUND`（`symbol_not_found_python`，`bad_format`，定位 `node_id`）；R14 也写明 python_node 做“格式校验”。当前写法会让读者/测试误以为 python_node 格式错误不再报 DSL 码。 | 按 D7 精确表述：“`DSL_SYMBOL_*` 不再覆盖 python_node 的 **import/签名**（后移 worker 启动期 → `NODE_WORKER_IMPORT`）；python_node 的 `module/function` **格式**非法仍在加载期报 `DSL_SYMBOL_NOT_FOUND`（`bad_format`，定位 `node_id`）”。同步 §2.4.2 定位列与 07:32 措辞。 | open |
| 3 | 重要（C3） | `04-节点接口与扩展点.md:820`（A2）、`:825`（A7）、`:835`（A17）、`:815`（§4.9 结论） | §4.9 对齐表未随本次回写更新，且与新正文/00 自相矛盾：A2 仍称 “`NodeError` **7 变体**、未新增变体”，但 00 §0.3 已按 D10 增 `NodeWorker`（8 变体）且 04 §4.1.4 已新增该行；A7 仍称 `PYTHON_CONCURRENCY_*`/`stream_tx` “直接作为 `NodeContext.stream_tx` 容量与信号量默认值 ✅ 对齐”，但 §4.2.1 已删 `stream_tx`（改 `emitter`）、§4.3.6 已改 `MAX_WORKERS`/`MAX_WORKERS_PER_RUN` 且 00 §0.5 已标 `PYTHON_CONCURRENCY_*` v2 忽略；A17 仍只写“03/06 双通道 ✅”，未标 M2 单通道临时口径（ISSUE-033）；节首“发现 1 处措辞冲突”结论也已过期。 | A2 → “8 变体（含 `NodeWorker`，D10）”；A7 → “`STREAMING_CHANNEL_CAP` 仅规范容量；`stream_tx`→`emitter`；`PYTHON_CONCURRENCY_*` v2 忽略，并发见 `MAX_WORKERS`/`MAX_WORKERS_PER_RUN`”；A17 → 补“M2 单通道 8192（ISSUE-033），M5 双通道”；节首结论改为“回写后对齐表已复核（残留项见本节）”。 | open |
| 4 | 重要（C4） | `04-节点接口与扩展点.md:195,323,330,335,350-351,369-371,529`；对照 `python/zgraph_worker/protocol.py:23`、`errors.py:24,30`、`schemas/worker-protocol-v1.md:45` | §4.3.3 已把标题改为 `zgraph_worker.ctx.WorkerContext`，但正文仍指示用户捕获/抛出 **不存在的** `zgraph.Cancelled` / `zgraph.NodeError` / `zgraph.EmitError`（仓库无 `zgraph` 包；SDK 实际为 `zgraph_worker.errors.WorkerCancelled`/`BusinessError`、`zgraph_worker.protocol.ProtocolError`）；且 `:323` 称 `ProtocolError` 是 **`ValueError` 子类**，实现为 `class ProtocolError(Exception)`。协议文档（`schemas/worker-protocol-v1.md:45`）也使用 `WorkerCancelled`，三处口径不一。 | 统一为 v2 SDK 实际导出名（`WorkerContext.emit` 非法 payload → `zgraph_worker.protocol.ProtocolError`；取消 → `WorkerCancelled`；业务错误 → `BusinessError`），或显式声明“M3 提供 `zgraph` 兼容别名”并纳入验收；同时把“`ValueError` 子类”改为实际基类（若目标要 `ValueError`，需改 SDK 并在本册注明 M3 计划）。 | open |
| 5 | 建议 | `02-DSL与校验.md:93`（§2.2.1 `config.exec_class` 行） | 仍写“选择 Python 并发信号量类别（CPU=1/IO=4）”，未标 D4 “v2 忽略”；与 00 §0.5（`PYTHON_CONCURRENCY_*` v2 忽略）、03 §3.5、04 §4.3.6 不一致（与 C3 的 A7 同源）。 | 该行补“**v2 忽略（D4）**：保留字段，每节点一进程，不再区分 CPU/IO 信号量”；02 分册 banner 已声明“已回写”，此处应同步。 | open |
| 6 | 建议 | `07-验收用例.md:110`；`02-DSL与校验.md:591`；`04-节点接口与扩展点.md:478,802` | ISSUE-046 后主表已改为“run 启动后、实例创建前拒绝”，但仍有“run 在**派发前** Failed”“移除**派发前检查**”等残留措辞，读者可能误以为仍是派发时检测。 | 统一为“run 启动后、实例创建前”（保留历史说明时标注 v1.2 原文）。 | open |
| 7 | 建议 | `04-节点接口与扩展点.md:581` | 段落末尾多一个 `**`（“不使 run Failed。**”），Markdown 粗体标记不配对，渲染会出现字面 `**`/粗体错位。 | 删除多余的 `**`。 | open |
| 8 | 建议 | `01-领域模型与状态机.md:222` vs `crates/zgraph-kernel/src/run.rs:391`（`killed_workers: u64`） | 01 的 `RunState.killed_workers` 写 `usize`，实现与 00 `RunMetrics`/trace 均为 `u64`；语义无差但可核验性小瑕疵。 | 统一为 `u64`（或注明“内部计数类型实现可自选”）。 | open |
| 9 | 建议 | `04-节点接口与扩展点.md:191-194,203,226,326-335` | 称 SDK 将 `user_id` / `params` / `seed` / `run_id` 等以“**只读属性**”暴露，但 `python/zgraph_worker/ctx.py` 中这些是普通公有属性（`self.run_id = ...`，可赋值），并非只读 property。 | 改为“公有属性（SDK 当前实现，非稳定契约）”，或后续用 `@property` 收紧以兑现“只读”。 | open |

> 4 项重要项均可在本分支内以文档文本修正关闭（无代码/台账改动需求），因此列为 C1–C4 合并前置条件；#5–#9 为建议级，不阻塞，但建议随本轮一并修（尤其 #5/#6 与 C2/C3 同源）。

## 4. 验证证据

### 4.1 范围与提交元数据

```bash
cd /Users/zyzs/lczyzs/code/zgraph/.worktrees/docs-agent-a
git branch --show-current          # feat/docs-m3-spec-writeback
git merge-base origin/main HEAD    # a6f4f5ce9434faade70934d44f26df6cfe38c671
git rev-parse origin/main          # 261d97192576cdff8bdb593e24d985ceb8cea9fb（分支落后 3 个已合并提交，merge.sh 步骤 5 会 rebase）
git log --oneline origin/main..HEAD
# 2863490 docs(docs): spec v1.2 M0–M2 回写（D1–D12 与 §8.2）   ← 仅 1 个作者提交
git diff --stat a6f4f5c..HEAD      # 10 files changed, 488 insertions(+), 357 deletions(-)
git status --porcelain             # 空（评审提交前）
```

- `2863490` 的 author/committer 均为 `chenyuxiangg <1007423165@qq.com>`；subject `docs(docs): ...`；正文含 `Review: REV-014` 与 `Agent: docs-writer`。
- 改动文件：`doc/architecture-v2.md` + `doc/spec-v1.2/{00,01,02,03,04,06,07,PROVENANCE,README}`；`05/08/REFERENCES` 未改，`doc/issues.md`/`doc/progress.md`/`doc/reviews/README.md`、代码与测试均未改。

### 4.2 必跑命令

```bash
bash scripts/dev/check.sh          # 12 项 OK → CHECK PASSED，exit 0
PYTHONPATH=python /Users/zyzs/lczyzs/code/zgraph/.venv/bin/python -m pytest
# 20 passed in 0.58s（本分支 base=a6f4f5c，尚未含 REV-013 的 py-worker 提交；
#   merge.sh rebase 到 origin/main=261d971 后应为 44 passed）
```

- `doc/test-baseline.txt` 为空 → 无新增失败；本分支不涉及 Python 代码。

### 4.3 hash 与行数（实测 vs PROVENANCE §3 / README 分册表）

```bash
cd doc/spec-v1.2 && shasum -a 256 0*.md && wc -l 0*.md
```

| 文件 | SHA-256（实测） | 行数（实测） | PROVENANCE / README |
|---|---|---|---|
| `00-总纲.md` | `4fb61c3d…a6089a` | 287 | ✅ 一致（287） |
| `01-领域模型与状态机.md` | `b0b4c3f4…a23a9ac` | 302 | ✅ 一致（302） |
| `02-DSL与校验.md` | `077a912e…e9b58` | 913 | ✅ 一致（913） |
| `03-调度器与生命周期.md` | `994e40d1…b2f1b577` | 388 | ✅ 一致（388） |
| `04-节点接口与扩展点.md` | `f5568fd6…420e9b58` | 873 | ✅ 一致（873） |
| `05-AgentHarness.md` | `43c85a6e…92b871` | 1172 | ✅ 一致（1172，未改） |
| `06-存储与可观测.md` | `45f205f1…51b407` | 985 | ✅ 一致（985） |
| `07-验收用例.md` | `12164cb0…5998ea9` | 195 | ✅ 一致（195） |
| `08-审计关闭矩阵.md` | `317bc008…21c2d524` | 412 | ✅ 一致（412，未改） |
| **合计** | — | **5527** | ✅ 与 PROVENANCE §3 声明一致 |

- PROVENANCE §2 已追加 M0–M2 回写批次记录、§5 已加“回写已完成”声明；README 头部/分册行数/关键结论已同步。唯一问题是这些记录被引用为“§2.5”（见 §3 #1）。

### 4.4 回写完整性抽查（逐项 grep + 读上下文）

| 要求 | 结果 |
|---|---|
| 00 §0.2 EdgeIdx / EdgeIndex 结构体 | ✅ `00:36-38` 命名裁定；`01:47-54` 结构体字段改 `EdgeIdx`，与 `crates/zgraph-kernel/src/graph.rs:98` 一致 |
| 00 §0.3 NodeWorker / NodeAbandoned deprecated / killed_workers | ✅ `NodeError::NodeWorker{kind}` + `WorkerErrorKind` 4 变体；`NodeAbandoned` 标 deprecated；`RunMetrics` 4 字段与 kernel/trace 一致 |
| 00 §0.4 5 个新错误码与 §8.2 字符串一致 | ✅ 4 个 `NODE_WORKER_*` 在总表、`REPLAY_RECORD_MISSING` 在 CLI 命名空间段；5 个字符串与 `architecture-v2.md:270-274` 逐字一致；17 个 DSL 码计数正确 |
| 00 §0.5 MAX_WORKERS* | ✅ `MAX_WORKERS=CPU×2`/`MAX_WORKERS_PER_RUN=8`；`PYTHON_CONCURRENCY_*` 标 v2 忽略（与 03/04/07 一致） |
| 03 §3.9 kill 语义（含 §3.9.4）/§3.8/§3.12/§3.13 | ✅ 终止序与 `schemas/worker-protocol-v1.md:45` 一致；M2 in-process `abort()` 等价口径；并发上限/单通道临时口径（ISSUE-033）已标注 |
| 04 §4.2 NodeContext v2 / §4.3.x / D7 / 子进程执行模型 | ✅ 主体已回写（`emit/log/get_var/user_id/cancel_requested/check_cancelled/deadline_remaining_ms` + `seed/now/random`、无 `cancel_token`；签名校验后移）；⚠️ 异常名/包名残留（C4） |
| 06 §6.2.4/§6.2.5.1/§6.3.4/§6.4.1/§6.4.2/§6.4.5/§6.8.3 | ✅ 列/RecordMode/NodeError JSON canonical/生产者时间/单通道/payload 上限/fail-stop 未接线/owner_pid 未实现，均与实现或 issues 台账一致（JSON 形状与 `bridge.rs:457-497` 逐键一致） |
| 02 §2.5/D7、§2.6.3 跳过资产根、§2.6.1 subgraph 时机 | ✅ 主体到位；⚠️ `DSL_SYMBOL_*` 的“仅 gate”绝对化（C2） |
| 07 §7.2 两列 + AC-06/07/09 | ✅ 两列齐全；AC-06 kill 口径、AC-07 真 worker、AC-09“**阈值 M3 实测后回填**”均已明确；⚠️ AC-04 M2 列措辞（C2）、附加 2 “派发前”残留（建议 #6） |
| 跨册一致性 | ✅ EdgeIdx/EdgeIndex、`NodeError::NodeWorker`、`killed_workers`/`abandoned_tasks`、`NODE_WORKER_*`、并发默认值（CPU×2/8）、D1 超时语义在 00/01/03/04/06/07 主干一致；⚠️ 04 §4.9 A2/A7/A17 例外（C3） |
| architecture-v2 标注 | ✅ 仅 §8 回写状态（`:239`）与 §8.2 尾部（`:278`）两处，保留为决策记录 |

### 4.5 独立反向核对（不采信 commit 自述）

- `node_error_json`（`crates/zgraph-cli/src/bridge.rs:457-497`）实际键：`code/message/kind` + `business_code`/`exception+traceback`/`elapsed_ms`/`harness_kind+detail`/`worker_kind` —— 与 06 §6.2.5.1 表逐键一致。
- `crates/zgraph-kernel/src/types.rs:355-375`：`WorkerErrorKind::{Handshake,Protocol,Import,Crash}` 与 `NODE_WORKER_*` 常量映射一致；`RunMetrics` 4 字段与 00 §0.3 一致。
- `crates/zgraph-trace/src/schema.rs:58-64,106-110`：`seed`/`record_mode`/`killed_workers` 列与 06 §6.2.4 一致；`RecordMode` 小写 `none/hash/full`（`model.rs:131-154`）与 §6.2.4 一致。
- `writer/mod.rs:48` 单通道 `sync_channel(8192)`、`schema.rs:54` `TRACE_PAYLOAD_MAX_BYTES=65_536`、`reader.rs` `RunRow` 无 `owner_pid` —— 与 03 §3.12/06 §6.4.1/§6.4.2/§6.8.3 一致。
- `crates/zgraph-dsl/src/symbols.rs:106-116,166-172,239-252` 证明 C2（python_node 格式非法仍报 `DSL_SYMBOL_NOT_FOUND`）；`symbols.rs:23,98,255-269,300-330` 证明 gate op 名称/params 契约/跨类冲突的 DSL 码与 02 §2.5 一致。

### 4.6 范围外/环境说明

- 分支基于 `a6f4f5c`，落后 `origin/main=261d971`（含 REV-013 py-worker 合并）；`merge.sh` 会先 rebase 再跑测试（届时 pytest 预期 44 passed）。本评审按任务要求以 `merge-base..HEAD` 核范围，rebase 不改变本分支 10 文件 diff。
- 未跑 cargo（本分支无 Rust 改动）；未 push/merge；未改任何文档正文/台账/进度/README。

## 5. 遗留问题

- **条件关闭前不可合并**：C1–C4 见 §1/§3；关闭证据 = 作者修正提交 + 本记录 §6 追加关闭行（建议同轮一并处理 §3 #5–#9，减少往返）。
- **台账（Lead 合并时更新，本分支按约束不改 `doc/issues.md`）**：ISSUE-050（回写批次）可标 `fixed`（注明 REV-014）；ISSUE-026/027/033/034/035/036/037/043/046/048/053/054 在回写完成/口径确认后可更新状态；ISSUE-059 的 M1 临时退出码口径已回写 02/06，仍待 M3 统一实现。
- **未登记但需跟进（建议级）**：#5 02 §2.2.1 exec_class D4 标注；#6 “派发前”残留；#7 Markdown 粗体；#8 `RunState.killed_workers` 类型统一；#9 SDK“只读属性”措辞。
- 本评审未改代码/正文/台账/进度/README；未 push/merge。

## 6. 条件关闭记录（仅“有条件通过”填写）

| 条件 | 关闭证据 | 关闭日期 |
|---|---|---|
| C1 | 待作者修正 9 处 `PROVENANCE §2.5` 引用（或 PROVENANCE 增 §2.5 小节）并提交 | — |
| C2 | 待作者修正 02 §2.5.4/§2.4.2 与 07:32 的 `DSL_SYMBOL_*` 表述（覆盖 python_node 格式非法） | — |
| C3 | 待作者同步 04 §4.9 A2/A7/A17 与节首结论 | — |
| C4 | 待作者对齐 04 §4.3.3/§4.3.4 的 `zgraph_worker.*` 异常名与 `ProtocolError` 基类口径 | — |
