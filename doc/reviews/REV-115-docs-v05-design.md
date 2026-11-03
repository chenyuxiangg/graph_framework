# REV-115 · docs · v0.5 F0 设计冻结（DR-016–020 + AC-25–28 + 真实 Codex/DSH 方案）

> **评审范围**：docs-only（14 文件；DR-016–020 + spec 00/02/05 §5.16/06 §6.16/07 AC-25–28/08 §8.14 + index/progress/issues）；无代码/测试/既有评审记录改动；未触碰 `dist/`、无 token/密钥。评审人只新增本评审记录，未改实现/文档。

- **分支**：`docs/docs-v05-design`
- 分支: docs/docs-v05-design
- **作者**：`docs-dev`（commits `abd79bc`→`5c20fa2`→`db9d28b`→`feb513c`→`74bba4f`；正文 `Review: REV-115` / `Agent: docs-dev`）
- **评审人**：`reviewer-a`（独立评审，≠ 作者；Lead 第二评审）
- **基于**：main `9480fa4`（`main = origin/main = 9480fa4`）
- **变更范围**：`9480fa4..74bba4f`（**14 files, +713/−14**）：新增 `doc/decisions/DR-016..020`；`doc/spec-v1.2/00/02/05/06/07/08` 草案段；`doc/index.md`、`doc/progress.md`、`doc/issues.md`（176–181 规划）
- **评审时间**：2026-11-03
- **关联**：task-95/96/97；DR-016–020、spec 00/02/05 §5.16/06 §6.16/07 AC-25–28/08 §8.14；spike `/tmp/dsh-repo-spike.md`、`/tmp/codex-repo-spike.md`

## 1. 结论

**结论：有条件通过**

P0=0；**P1 条件 C1**（reexec mismatch 错误码在 DR-017/DR-016/spec 中混用 `REPLAY_TOOL_MISMATCH` / `REPLAY_TOOL_RECORD_MISMATCH`，F1 实现前必须统一）；**P2 条件 C2–C4**（Codex spike 状态仍写「待 spike」且 index DR-020 行粘连；`BUDGET_EXCEEDED`/`allow_write_path_invalid` 未进 00 码/reason 表；DR-020 Codex 零密钥命令缺 prompt/`--skip-git-repo-check`）；P3 若干。范围/波次/预算/reexec/sandbox/真实 Codex·DSH 方案与 AC-25–28 主体成立；spike 事实与命令抽查一致、无密钥。

### 1.1 已核对成立

| 项 | 结论 |
|---|---|
| DR-016 范围/波次 | ✅ v0.5.0 五项（reexec/ISSUE 清理/sandbox 增强/真实 Codex·DSH/可观测成本）+ v0.6 W3b；非目标；F0–F6 依赖（F1/F2 同 harness 串行，F3 跨模块，F4/F5 部分并行）；发布条件/兼容红线（默认逐字节不变、DSL 1.0/19 码、trace 不 v8）；AC-25–28 矩阵；W5 已取消（DR-015） |
| DR-017 reexec | ✅ 默认 recorded 不变；显式 `--replay-tools=reexec`；read/grep/glob/diff 四只读工具；结果 `result_sha256` 比对、mismatch fail-closed；副作用工具 exit2；truncated/缺记录 fail-closed；同 workspace/allow-path；不升 v8；AC-25 a–f 可测 |
| DR-018 预算/成本 | ✅ 价格仅 `--pricing` 文件（缺/坏 → 配置错误不启动 run、不内置价）；`--max-tokens`/`--max-cost` 默认关；检查点每次 LLM 请求前+run 结束；超限 Failed + `BUDGET_EXCEEDED` + `Other("budget_exceeded")`、v0.5 不可续；metrics/--stats additive、不新增表/列/不升 v8；DSH token/cost 不可得、Codex usage 累计口径 |
| DR-019 sandbox 增强 | ✅ `--allow-write-path` 独立目录级可写白名单（canonicalize、profile `file-write*`+jail、与 workspace 相同/嵌套幂等、不可 `/`、负例）；构建工具链 cargo/node/uv 路径/HOME/TMPDIR/cc -isysroot 文档；默认系统集合不变；AC-26 a–e |
| DR-020 真实 Codex/DSH | ✅ DSH FINAL：`dsh --profile acp`、ACP v1 NDJSON、真机 initialize/session/new/close/EOF/-32603/-32602、request_permission、cancel 2s、usage_update 不可填 input/output tokens、pin/protocolVersion、scratch/DSH_HOME；Codex 中期：npm 0.166.0、零密钥 CODEX_HOME mock provider+Responses SSE、adapter 零改动、turn.completed 清 failed、`--ephemeral`、ARG_MAX、无 command_execution item、嵌套 exit71/F5f；F5a–f 分层/门控/不伪造/发布证据 |
| spike 事实/安全 | ✅ 与 `/tmp/dsh-repo-spike.md`/`/tmp/codex-repo-spike.md` 抽查一致（DSH 1.44s/0.06s/-32603、Codex hello from mock/usage 11,7,18,2/cached 丢弃/2 次累计 22/14）；命令/env 均为占位（`DEEPSEEK_API_KEY=...`/`MOCK_API_KEY=dummy`），无真实 token/密钥；不写用户 `~/.dsh` |
| spec/范围/约束 | ✅ 00/02/05 §5.16/06 §6.16/07 AC-25–28/08 §8.14 只追加草案；默认/兼容红线明确；无代码/测试/既有评审改动；未触碰 `dist/`；`check.sh` **PASSED**；PROVENANCE 设计阶段未动（符合 task-96） |
| AC 可测性 | ✅ AC-25 a–f、AC-26 a–e、AC-27 a–f、AC-28 a–j 均有可执行断言方向（真 CLI/SQLite/进程/环境门控）；AC-28 DSH 握手/降级/Codex mock 可 CI 执行（无需 key），全链路 key 门控 |

## 2. 四维评审

| 维度 | 结论 | 依据 |
|---|---|---|
| 功能正确性 | ⚠️ 有条件 | 范围/波次/reexec/预算/sandbox/真实验证方案主体正确；C1 reexec 错误码混用、C2 Codex spike 状态 stale/index 表损坏、C3 新码/reason 未登记、C4 Codex 命令缺 prompt 需修 |
| 代码结构合理性 | ✅ | docs-only；DR 分层（016 范围/017 reexec/018 预算/019 sandbox/020 验证）；spec 分册增补；spike 证据/命令独立；无代码/测试改动 |
| 逻辑性 | ✅（C1/C2/C3/C4） | 默认不变/opt-in/不升 v8/失败面/分层门控自洽；跨文档状态（Codex 待 spike vs 已落盘）、错误码、Codex 命令/索引有局部不一致 |
| 可读性 | ✅（P3） | DR 摘要/矩阵/裁决项清晰；C2 index 表格粘连、DR-016/020 状态措辞需修；P3 边界项 |

## 3. 发现清单

### 3.1 P1 条件（F1 前必须关闭）

| # | 位置 | 问题 | 建议 |
|---|---|---|---|
| **C1** | `DR-017:40`（Lead 裁决段「`REPLAY_TOOL_MISMATCH`」）、`DR-017:53/66`（§3 文本）；`DR-016:91`（AC-25 矩阵 `REPLAY_TOOL_MISMATCH`） vs `DR-017:14/33/38/47/74/76`、spec05 §5.16.1、spec07 AC-25 c 均用 **`REPLAY_TOOL_RECORD_MISMATCH`** | **reexec mismatch 错误码混用**：既有 v0.4 实现/00/06/07 的码是 `REPLAY_TOOL_RECORD_MISMATCH`；DR-017 Lead 裁决段/§3 与 DR-016 AC-25 矩阵写成 `REPLAY_TOOL_MISMATCH`，F1 实现会面临两个码名 | 统一为 **`REPLAY_TOOL_RECORD_MISMATCH`**（同码扩展语义，不新增）；同步 DR-016、DR-017、spec 00/05/06/07/08；补 grep 断言无 `REPLAY_TOOL_MISMATCH` 独立码 |

### 3.2 P2 条件（F0 内 docs 小修）

| # | 位置 | 问题 | 建议 |
|---|---|---|---|
| **C2** | `DR-016` status（「Codex/DSH 待两个 spike」）、`DR-020` 首行/§0 表/§5 标题/§7（Codex「待 spike」）、`spec00:13`、`spec05:1559`（「Codex 待 spike」）、`spec07:650/76`（「Codex 待 spike」）、`index:25`（DR-020 行「待 spike」且与 architecture 行粘连）、`progress`（「待两个 spike」） | **Codex spike 已由 `74bba4f` 落盘（中期已通），但状态/索引仍写「待 spike」**；index DR-020 行与 architecture 行粘连（`...AC-28 || [architecture...`），表格断裂 | 统一改为「Codex 中期已落盘（74bba4f）；F5d FINAL/真 key 待」；拆开 index DR-020 与 architecture 行；同步 DR-016/020/spec/index/progress |
| **C3** | `spec00` §0.4 代码表/§0.5 常量；`spec05 §5.16.3`；DR-019 §1/§5 | **`BUDGET_EXCEEDED` 新域码未加入 00 §0.4 代码表**（仅 banner 提及）；**`allow_write_path_invalid` 未加入 00 v0.5 reason 闭集**；AC-27 d「`--max-cost` 不可用费用 → 配置/警告口径」未定义（无 `--pricing`/未知模型时预算如何判定） | 00 §0.4 追加 `BUDGET_EXCEEDED`（append-only，run Failed）与 `allow_write_path_invalid` reason；明确 `--max-cost` 无 `--pricing` → 配置错误、未知模型费用不可用时 max-cost 是否 fail-closed（推荐配置错误/启动前拒绝），补 AC-27 d 精确断言 |
| **C4** | `DR-020` §5 可复现命令 `codex exec --json` | **Codex 零密钥命令缺 prompt 与 `--skip-git-repo-check`**（spike 实测命令为 `codex exec --json --skip-git-repo-check -o /tmp/last.txt "reply with exactly: hello"`）；当前命令在非 git/无 prompt 场景不可复现 | 按 spike 补齐 `--skip-git-repo-check`、prompt、`-o` 与 mock SSE 前置步骤；AC-28 g 同步 |

### 3.3 P3（建议）

- **P3-1**：DR-016 §5 AC-28 写「F5a–d 分层」，DR-020/AC-28 已扩为 F5a–f（含 F5d-a/b/c、F5f）；建议矩阵同步 F5a–f。
- **P3-2**：DR-019 §1「不可为系统目录白名单（推荐拒绝 `/System`/`/usr` 等，或至少 warning）」二选一未裁决；建议 spec 明确「拒绝（配置错误）」或「warning + 允许」。
- **P3-3**：DR-019 AC-26 c 重复 `/` 负例（b 已含）；可合并/去重。
- **P3-4**：DR-018 `BUDGET_CHECK_BEFORE_EACH_LLM_CALL=true` 常量命名/布尔配置语义建议 D6 as-built 再定；`BUDGET_COST_PRECISION` 6 位与 `--max-cost` 比较边界注意浮点。
- **P3-5**：spec07 AC-28 标题/表格仍「Codex 待 spike」；随 C2 一并改。
- **P3-6**：DR-020 §0 表 Codex 列仍「待 spike」；随 C2 一并改。
- **P3-7**：`doc/index.md` reviews 至 REV-115，但 reviews README REV-115 行由本次评审补；index DR-020 行格式见 C2。

## 4. 验证证据

评审人亲自执行（worktree `.worktrees/docs-docs-dev`，HEAD `74bba4f`，main `9480fa4`，worktree 干净）：

```bash
git status --porcelain                     # 空
git diff --stat main...HEAD                # 14 files +713/−14（docs-only）
bash scripts/dev/check.sh                  # CHECK PASSED
# spike 抽查：/tmp/dsh-repo-spike.md 327 行、/tmp/codex-repo-spike.md；
#   DSH initialize 1.44s/session-new 0.06s/-32603/-32602/NDJSON/request_permission/usage_update
#   Codex 0.166.0/hello from mock/usage 11,7,18,2/cached 丢弃/2 次累计 22/14/ARG_MAX/exit71
#   命令 env 均占位；无真实 token/密钥
```

- **范围/约束**：docs-only 14 文件；无 `crates/`/`tests/`/`doc/reviews/` 改动；spec 只追加草案段；无 token/dist；check.sh PASSED；PROVENANCE 设计阶段未动。
- **spike 一致性**：DR-020 的 DSH FINAL/Codex 中期事实与两份 spike 报告一致（版本/入口/传输/握手/错误/usage/限制）；Codex 命令块与 spike 实测命令有差异（C4）。
- **AC 可测性**：AC-25（recorded/reexec/mismatch/exit2/truncated/workspace）、AC-26（write-path/负例/组合）、AC-27（pricing/预算/停止/metrics）、AC-28（DSH 握手/降级/F5a-f/门控/Codex mock）均有可执行断言；CI 可执行部分无需 key。

## 5. 条件关闭方式

- **C1 由 docs-dev 在 F0 内 docs 小 commit 关闭**：统一 `REPLAY_TOOL_RECORD_MISMATCH`（DR-016/017/spec 00/05/06/07/08）；grep 无独立 `REPLAY_TOOL_MISMATCH`。
- **C2/C3/C4 同批 docs 修**：Codex 状态/索引/命令；00 码表/reason；`--max-cost` 无 pricing 语义。重跑 check.sh。
- 关闭后评审人复核 grep/命令/码表/索引 + check.sh，追加「条件关闭」段、结论改「通过」；随后 Lead merge F0 → F1。

## 6. 条件关闭记录（"有条件通过"）

| 条件 | 状态 | 关闭要求 |
|---|---|---|
| C1 reexec mismatch 码统一为 `REPLAY_TOOL_RECORD_MISMATCH` | **open** | DR-016/017/spec 全量统一；grep 无 `REPLAY_TOOL_MISMATCH` |
| C2 Codex spike 状态/索引/进度更新 + index DR-020 拆行 | **open** | DR-016/020/spec00/05/07/index/progress；check.sh |
| C3 `BUDGET_EXCEEDED`/`allow_write_path_invalid` 登记 + max-cost 无 pricing 语义 | **open** | 00 §0.4/reason/AC-27 d；spec08 同步 |
| C4 Codex 零密钥命令补 prompt/`--skip-git-repo-check` | **open** | DR-020 §5 与 AC-28 g；与 spike 命令一致 |

## 7. 遗留问题

- C1–C4 关闭前不 merge；P3-1–P3-7 可同批或 D6 as-built 处理。
- 设计分支 docs-only，无 workspace 测试；F1–F5 实现后补真链路/门禁。
- 真实环境限制：DSH 全链路需 `DEEPSEEK_API_KEY`（用户授权），Codex 真 key 可选；CI 用 fake ACP/mock provider，不伪造、不写密钥。
- PROVENANCE 设计阶段不动；D6 实现回写时更新。
