# REV-117 · docs · Codex spike FINAL 勘误（tool 事件/映射/AC-28）

> **评审范围**：docs-only（9 文件）；更正 Codex spike 旧结论「tool 事件不可得」，补 F5d 9 项修复、AC-28 g/i/j 断言与命令；无代码/测试/既有评审记录改动；未触碰 `dist/`、无 token/密钥。评审人只新增本评审记录。

- **分支**：`docs/docs-v05-codex-fix`
- 分支: docs/docs-v05-codex-fix
- **作者**：`docs-dev`（commit `7cf0ba2`；正文 `Review: REV-117` / `Agent: docs-dev`）
- **评审人**：`reviewer-a`（独立评审，≠ 作者）
- **基于**：main `7b3e819`（含 v0.5 F0 设计冻结/第二评审；`main = origin/main = 7b3e819`）
- **变更范围**：`7b3e819..7cf0ba2`（**9 files, +55/−46**）：`DR-016/018/020`、`spec 00/05/07/08`、`index`、`progress`
- **评审时间**：2026-11-03
- **关联**：task-99/100；`/tmp/codex-repo-spike.md`（FINAL 344 行）；REV-115

## 1. 结论

**结论：通过**

P0=0、P1=0、P2=0；P3 3 项（DR-020 §0 表 AC-28 行仍 a–f、`ok == exit_code` 措辞、index 行尾格式）不阻塞。更正正确：`-s danger-full-access`/非嵌套下 `command_execution` 事件可得并可映射 tool_start/tool_end；旧「不可得」确为嵌套 sandbox exit71/静默的假阴性；默认 sandbox 假绿风险、F5d 9 项、AC-28 g/i/j 断言、零密钥命令与 mock/MOCK_API_KEY 占位均与 spike FINAL 一致；历史保留/只追加，范围红线合规。

### 1.1 已核对成立

| 项 | 结论 |
|---|---|
| 更正正确性 | ✅ spike FINAL 实测 `item.started/completed[command_execution]`（command/aggregated_output/exit_code/status，行 91–92），映射 tool_start/tool_end、`ok=exit_code==0`（行 206/311）；旧「无 command_execution item」= 嵌套 sandbox exit71/静默（行 98/320）；DR-020 §4.4 第 3 项、§4.5 假绿风险、§4.6 F5a–f 已按此更正 |
| F5d 9 项 | ✅ ① turn.completed 清 failed ② `--ephemeral` ③ tool 事件映射 ④ ARG_MAX 1MB 文档 ⑤ iterations=1/usage 累计 ⑥ pin `@openai/codex@0.166.0` ⑦ 认证分层（login/OPENAI_API_KEY vs mock 零密钥）⑧ 取消 SIGTERM→killpg/无孤儿、stdin close exit0 ⑨ 节点超时 >60s |
| AC-28 g/i/j | ✅ g：`streaming=false`、usage.input_tokens>0 无缓存字段、content==`-o`、usage(11,7,18,2)/cached=4 丢弃；i：取消 3s 内 pid 消失/无孤儿、未认证 exit_nonzero 不挂死；j：非嵌套 `-s danger-full-access`、≥1 对 tool_start/tool_end、`ok==exit_code`、默认 sandbox 假绿不据此断言 |
| 命令/env | ✅ `codex exec --json --skip-git-repo-check -o <file> "reply with exactly: hello"` + CODEX_HOME mock config + `MOCK_API_KEY=dummy` 占位；无真实 key/密钥；CI mock 零密钥；F5f 可选非嵌套 |
| 引用一致性 | ✅ DR-016 状态/F5 行/风险、DR-018 Codex FINAL 成本口径、spec 00 v0.5 banner、05 §5.16.4、07 AC-28 矩阵/详情/命令、08 追加 FINAL 勘误（保留 Codex 中期历史）、index/progress 均同步 |
| 历史保留 | ✅ spec08 保留「Codex 中期」旧段 + 追加「Codex FINAL 勘误」；DR-020 内旧结论以更正段替代但保留更正说明；未删除历史评审/记录 |
| 范围/约束 | ✅ docs-only 9 文件；无 `crates/`/`tests/`/`doc/reviews/` 改动；无 token/密钥；`dist/` 未触碰；`check.sh` **PASSED** |

## 2. 四维评审

| 维度 | 结论 | 依据 |
|---|---|---|
| 功能正确性 | ✅ | 更正与 spike FINAL 一致；tool 事件/假绿/F5d 9 项/AC-28 断言可执行；默认/CI 零密钥路径与 F5f 边界清楚 |
| 代码结构合理性 | ✅ | docs-only；DR-020 集中事实/修复/分层，spec 分册同步；无代码/测试改动 |
| 逻辑性 | ✅ | 「可得 vs 假阴性 vs 假绿」因果清楚；`streaming=false` 与 tool 事件不冲突；usage/缓存/`-o`/取消/认证口径自洽 |
| 可读性 | ✅（P3） | FINAL 更正/9 项/AC 断言清晰；DR-020 §0 表 AC-28 行与部分措辞可小修 |

## 3. 发现清单

### 3.1 P0/P1/P2

无。

### 3.2 P3（建议，不阻塞）

- **P3-1**：`DR-020:17` §0 表 `AC-28 | a–f（§5）` 仍写 a–f，但 §5/AC-28 已 a–j（REV-115 扩、REV-117 再补）；建议改「a–j」。
- **P3-2**：`DR-020` §4.4 第 3 项/AC-28 j 写 `ok == exit_code`，spike 语义是 `ok = (exit_code == 0)`；建议写清，避免把状态字符串与整数比较。
- **P3-3**：`doc/index.md` DR-020 行末尾仍无 `|`（上一行遗留格式，GFM 可容错）；建议补 `|` 统一表格格式。
- **P3-4**：`DR-020` §0 表 Codex 列多行重复「FINAL（tool 事件勘误）；F5d 实现/真 key 待」，可合并为一行说明；不影响语义。

## 4. 验证证据

评审人亲自执行（worktree `.worktrees/docs-docs-dev`，HEAD `7cf0ba2`，main `7b3e819`，worktree 干净）：

```bash
git status --porcelain                     # 空
git diff --stat main...HEAD                # 9 files +55/−46（docs-only）
bash scripts/dev/check.sh                  # CHECK PASSED
# spike FINAL 抽查：/tmp/codex-repo-spike.md 344 行
#   L91–92 item.started/completed command_execution(exit_code/status)
#   L206 tool_start/tool_end 映射、ok=exit_code==0；L98/L320 嵌套 exit71/静默假绿
#   L288 content==-o；L306–311 streaming=false/usage/cached/取消 3s/未认证/tool 对
#   全文仅 OPENAI_API_KEY=<REAL_KEY> 占位；无真实密钥
```

- **范围**：9 docs 文件；无代码/测试/既有评审/spec 冻结正文（仅 00/05/07/08 状态/断言段）改动；无 token/dist。
- **命令**：DR-020 §5 与 spec07 命令块一致（`--skip-git-repo-check -o ... "reply with exactly: hello"` + mock SSE/CODEX_HOME/MOCK_API_KEY 占位），与 spike 实测命令一致。
- **AC 可测性**：AC-28 g/i/j 均可由真 CLI/进程/JSONL 断言；F5d-a 零密钥 CI、F5d-c 取消/未认证、F5f 非嵌套 danger-full-access 边界明确。

## 5. 条件关闭方式

无（结论通过；P3 可随 D6 或本分支小修）。

## 6. 条件关闭记录（"通过"）

无开放条件。

## 7. 遗留问题

- P3-1–P3-4 文档小修不阻塞；DR-020 §0 AC-28「a–f」建议随 D6 as-built 修正。
- F5d 实现按勘误执行：tool 事件映射仅在 `-s danger-full-access`/非嵌套验证；CI mock 不执行工具，F5f 可选；不伪造。
- 真实 key/DSH 全链路仍受环境门控；本勘误不改变零密钥 CI 路径。
