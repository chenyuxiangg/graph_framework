# REV-111 · cli · v0.4 D5b2 CLI 工具落库 + trace 工具行 + replay --mode=tools

> **跨模块声明**：本分支除 `crates/zgraph-cli/**` 外含 harness 小扩展（`agent_loop/ctx/executor/lib/tools/test_support`：`ToolCallExecContext` + `HarnessCtx::node_id/attempt` + `redact_*` 公开）与 e2e 机械（`mcp_v03` allowed kind 加 `ToolCall`）、`Cargo.lock`；均 Lead 已批准、第二评审 Lead 追加；**未改 kernel**、无工具行为变化。评审人只新增本评审记录，未改实现/测试。

- **分支**：`feat/cli-tool-replay`
- 分支: feat/cli-tool-replay
- **作者**：`cli-dev`（提交 `e197584`；正文 `Review: REV-111` / `Agent: cli-dev`）
- **评审人**：`reviewer-a`（独立评审，≠ 作者）
- **基于**：main `297d648`（`main = origin/main = 297d648`）
- **变更范围**：`297d648..e197584`（**16 files, +1577/−35**）：`crates/zgraph-cli/{bridge,replay_cmd,trace_cmd,run_cmd}.rs`、`tests/{tool_replay(新增 599),run,trace}.rs`、`crates/zgraph-harness/src/{agent_loop,ctx,executor,lib,tools,test_support}.rs`、`tests/e2e/mcp_v03.rs`、cli Cargo/Cargo.lock
- **评审时间**：2026-11-02
- **关联**：task-87/88；DR-014 §3/§4/§7/§8、DR-013 §10、spec 06 §6.15、07 AC-24、trace v7；REV-109（D5a）

## 1. 结论

**结论：通过**

> **结论历史**：首轮（基准 `e197584`）为「**有条件通过**」——P1 C1：`--replay-tools` 在 events/outputs 未被忽略（parse 无条件校验 reexec）。作者 `d4da442` 关闭 C1/P3 后，评审人复核 cli 249/0（tool_replay 16）、workspace 1187/0/3、clippy/fmt/check/no-default 全绿（§8），C1 关闭，结论改**通过**；P3 剩余 docs 项留 D6。
>
> 首轮意见（历史保留）：P0=0；P2=0；桥接落库/脱敏/上下文、trace additive、replay tools 主体成立；C1 为 CLI 参数语义偏差。

### 1.1 已核对成立

| 项 | 结论 |
|---|---|
| harness 小扩展 | ✅ `ToolCallExecContext`（run/instance/node/attempt）由 `HarnessCtx::node_id/attempt` 提供，`agent_loop` 在正常/取消/超时三处填入；`ToolCallRecordSink::record_tool_call(&context,&record)`；`redact_sensitive_record/value` 公开；默认 no-op、无工具行为变化；harness 既有测试/workspace 全绿 |
| bridge 落库 | ✅ `ToolCallTraceSink` 映射 harness→trace `TraceInput::ToolCall`（**非 events**）；context 补齐；source/status 字符串 fail-closed（未知 → None）；**落库前二次脱敏**（敏感键 `***`、bash `env` 值全打码，bridge 单测断言无明文）；side_effect/bash 直映；10000 行上限停止+告警一次；无工具零行 |
| trace additive | ✅ 人类：事件/HumanDecision/ForeachExpansion 之后按 `(started_at,call_id)` 追加 `tool_calls: N` + 单行摘要（不打印完整 args/result）；`--json` 仍纯 NDJSON，追加 `{"kind":"ToolCall",...}`（Lead 裁决 NDJSON，不做顶层数组）；`--stats` 仅 `tool_calls>0` 时追加计数（AC-24 c 保持）；无工具输出逐字节不变 |
| replay tools | ✅ `--mode=tools` recorded-only；按 reader `(started_at,call_id)` 输出（human/JSON）；`--replay-tools=reexec` → 用法错误 exit 2「v0.4 未实现」；MISSING（streaming `tool_call_start` 数 > rows）/TRUNCATED（任一行 `truncated=1`）/MISMATCH（JSON 不可解析或 args/result/error sha 不符）→ CLI 码 exit 3，不写 `runs.error_code`；**无执行/无 spawn/DB 字节/行数/workspace 快照不变、无 -wal、ZGRAPH_PYTHON=/nonexistent 仍 OK**（探针）；events 输出与 trace --json 逐字节一致 |
| 脱敏/真链路 | ✅ bridge 单测覆盖 args/result 敏感键 + bash env；`tests/run.rs` 真 CLI fake-LLM+真 Python 工具落 1 行（原生 call_id/source/status/node/attempt/args），`replay --mode=tools` 还原且 DB 字节不变 |
| 机械 e2e | ✅ `mcp_v03` AC-19f allowed kind 加 `ToolCall`；events.kind 7 类不变、其余断言不动（跨模块机械，Lead 第二评审） |
| 门禁 | ✅ cli **245/0**（tool_replay 13/0）；workspace **1183/0/3**；clippy/fmt/check/no-default 全绿 |

## 2. 四维评审

| 维度 | 结论 | 依据 |
|---|---|---|
| 功能正确性 | ⚠️ 有条件 | 落库/脱敏/上下文/trace 行/stats/replay recorded/校验/无副作用主体正确；**C1**：`--replay-tools=reexec`（及非 recorded）在 `--mode=events|outputs` 下被无条件拒绝，违反 Lead「events/outputs 忽略 replay-tools」预裁决 |
| 代码结构合理性 | ✅ | bridge sink/映射独立；trace_cmd 复用 human/JSON helper；replay tools 复用 reader/工具行；harness ctx 扩展最小且默认 no-op；CLI 组合根只注入 sink；无 events/内核改动 |
| 逻辑性 | ✅（C1 例外） | source/status fail-closed、脱敏二次、行数上限告警一次、recorded 校验顺序（missing→truncated→mismatch）、只读探针、events 字节一致；C1 为参数解析顺序/模式绑定问题 |
| 可读性 | ✅（P3 备注） | 模块注释/DR 引用/测试分组清晰；P3 为告警/口径/测试命名建议 |

## 3. 发现清单

### 3.1 P1 条件（D5b2 内关闭）

| # | 位置 | 问题 | 建议 |
|---|---|---|---|
| **C1** | `crates/zgraph-cli/src/replay_cmd.rs` `parse`/`parse_replay_tools`（`:149-225`）；`tests/tool_replay.rs` 仅测试 tools 下 reexec | **`--replay-tools` 在 `--mode=events|outputs` 未被忽略**：`parse` 在未知 mode 前就调用 `parse_replay_tools`，`reexec` 无条件返回错误；Lead 预裁决为「replay-tools 在 events/outputs 忽略」。当前 `replay --mode=events --replay-tools=reexec` exit 2，而应忽略（events/outputs 行为不变） | `parse` 先把 `--replay-tools` 值存入 `Option<ReplayTools>`，解析完 mode 后：`Tools` 仅接受 recorded、reexec 用法错误；`Events|Outputs` 忽略（至少 recorded/reexec；是否对 bogus 报错按 Lead 口径）。补 events/outputs + reexec/recorded 用例 |

### 3.2 P3（建议）

- **P3-1**：`ToolCallTraceSink::admit` **先计数、后映射**；若 `map_tool_call_record` 因未知 source/status 返回 `None`，行未落库但预算已消耗（且无告警）——统计/告警不精确；建议先映射再计数（或失败回退计数）。
- **P3-2**：10000 行告警测试直接调 `admit`，未通过 `record_tool_call` 验证「只告警一次」的完整路径；建议补 sink 级边界用例（可注入小上限或 mock）。
- **P3-3**：`trace` 人类工具行格式为「实现定义」（Lead 接受，D5c e2e 按此断言）；建议 D6/06 §6.15.3 明确 human 行字段（args keys/bytes、bash exit/path）。
- **P3-4**：`trace --json` ToolCall 行含完整 args/result/error（已脱敏敏感键），人类行不打印；建议文档明确「JSON 为审计全量、human 仅摘要」。
- **P3-5**：`replay --mode=events` 输出现含 ToolCall 行（与 trace --json 一致；task 写「events/outputs 行为不变」指不执行/不写库，非字节不变）；建议 D6/CLI help 明确。
- **P3-6**：`<ToolCallTraceSink>` 告警字符串含多余空格（`...run {run_id};                      further...`）——纯 cosmetic。
- **P3-7**：`map_tool_source/status` 未知值静默丢行（fail-closed 符合要求）；建议 warn 一次或 D6 说明（不 fail run）。
- **P3-8**：`run_cmd` 对所有 run 注入 sink（含 `--record=none`）；无工具零行已验证；建议文档说明 tool_calls 审计独立于 `--record`。

## 4. 验证证据

评审人亲自执行（worktree `.worktrees/cli-cli-dev`，HEAD `e197584`，main `297d648`，worktree 干净；`CARGO_HOME=<repo>/.cargo-home`）：

```bash
cargo test -p zgraph-cli                    # 245 passed / 0 failed
cargo test -p zgraph-cli --test tool_replay # 13 passed / 0 failed
cargo test --workspace                      # 1183 passed / 0 failed / 3 ignored
cargo clippy --workspace --all-targets --locked -- -D warnings  # clean
cargo fmt --all -- --check                  # exit 0
cargo check -p zgraph-cli --no-default-features --locked        # 通过
bash scripts/dev/check.sh                   # CHECK PASSED
```

- **抽查**：bridge 映射/二次脱敏（`api_key`/`authorization`/bash `env` 全 `***`，非敏感保留）；context run/instance/node/attempt；source/status fail-closed；10000 行 `admit` 边界/告警标签；trace human 不打印完整 args/result、JSON `kind=ToolCall` 顺序 `(started_at,call_id)`；stats 仅 >0；replay tools missing/truncated/mismatch/reexec 退出码；无副作用探针（DB 字节/行数/workspace 快照/无 -wal/ZGRAPH_PYTHON 无效）；真链路 fake-LLM+真 Python 工具 1 行。
- **范围/红线**：`git diff --name-only` 16 文件；无 kernel；harness 小扩展默认 no-op；e2e 仅 allowed kind 机械；无断言放宽/无工具行为变化；默认路径 workspace 全绿。
- **对比自报**：cli 245/0、tool_replay 13、workspace 1183/0、clippy/fmt/check/no-default 与自报一致。

## 5. 条件关闭方式

- **C1 由 cli-dev 在 D5b2 内小 commit 关闭**：`ReplayArgs` 保存 `replay_tools`（或 parse 完 mode 再校验），events/outputs 忽略、tools 仅 recorded/reexec 用法错误；补 events/outputs 忽略测试。
- 关闭后评审人重跑 cli/tool_replay/workspace + 抽查 events/outputs 忽略与 tools 行为，追加「条件关闭」段；通过后 merge D5b2 → D5c e2e（Lead 追加 harness/e2e 第二评审）。

## 6. 条件关闭记录（"有条件通过"）

| 条件 | 状态 | 关闭要求 |
|---|---|---|
| C1 `--replay-tools` 在 events/outputs 忽略 | **closed（d4da442）** | `parse` 保存 `Option<ReplayTools>`、解析完 mode 后：tools+reexec → exit2；events/outputs 的 recorded/reexec 忽略；bogus 任意 mode exit2；补 `replay_tools_option_is_ignored_for_events_mode`/`...outputs_mode`/`unknown_replay_tools_value_is_usage_error_in_any_mode` 测试；cli 249/0、tool_replay 16/0 |

## 7. 遗留问题

- C1 已关闭；P3-1（sink 先映射后预算）/P3-2（10000 行全路径测试）已在 `d4da442` 修复；P3-3–P3-8 文档/格式留 D6。
- 跨模块（harness 小扩展 + e2e allowed kind + Cargo.lock）由 Lead 追加第二评审；可 merge D5b2 → D5c e2e。
- 无 P0/P1 之外问题；无工具默认输出/events.kind/replay events 语义回归（workspace 全绿）。

## 8. 条件关闭复核（2026-11-02）

复核提交：`d4da442`（父 `602865b`；仅 cli 3 文件 `+262/−32`，无新增跨模块）。评审人执行：

```bash
git status --porcelain                       # 空
git diff --stat 602865b..d4da442             # 3 files +262/−32（bridge/replay_cmd/tool_replay）
cargo test -p zgraph-cli                     # 249 passed / 0 failed
cargo test -p zgraph-cli --test tool_replay  # 16 passed / 0 failed
cargo test --workspace                       # 1187 passed / 0 failed / 3 ignored
cargo clippy --workspace --all-targets --locked -- -D warnings  # clean
cargo fmt --all -- --check                   # exit 0
cargo check -p zgraph-cli --no-default-features --locked        # 通过
bash scripts/dev/check.sh                    # CHECK PASSED
```

| 项 | 复核结果 |
|---|---|
| C1 events/outputs 忽略 | ✅ `--replay-tools=recorded/reexec` 在 `--mode=events` 输出与不带参数逐字节一致；在 `--mode=outputs` 仍走既有 L2 校验（exit3 `REPLAY_RECORD_MISSING`，非 usage error）；`--mode=tools --replay-tools=reexec` exit2；bogus 任意 mode exit2 |
| P3-1 sink 预算/告警 | ✅ 先 `map_tool_call_record`、失败告警每 run 一次且不消耗预算；达标后仍只告警一次；告警字符串单空格 |
| P3-2 10000 全路径 | ✅ `record_tool_call` 写满 10000 后 overflow-1/2 丢弃、DB 恰 10000、warned 集合 1；未知枚举告警簿记 1 |
| 门禁 | ✅ cli 249/0、tool_replay 16/0、workspace 1187/0/3、clippy/fmt/check/no-default 全绿；`602865b..d4da442` 仅 cli 3 文件 |

**最终状态**：C1 closed；P3-1/P3-2 closed；P3-3–P3-8 留 D6 文档/格式。建议 Lead merge D5b2 → D5c e2e。
