# REV-110 · harness · v0.4 D5b1 工具调用记录富化

- **分支**：`feat/harness-tool-records`
- **作者**：`dsl-dev`（单提交 `443ca3d`，基于 main `fa70439`）
- **评审人**：`reviewer-b`（独立评审，≠ 作者）
- **变更范围**：`fa70439..443ca3d`（9 files，+1015/−167，仅 harness）：`tools/agent_loop/executor/ctx/test_support`、`builtin/{files,bash,mod}`、`lib`；无 Cargo 变更
- **评审时间**：2026-11-02
- **关联**：task-85/86、DR-014 §1/§3/§4/§8、DR-013、spec 06 §6.15、trace v7 `ToolCallRecord`（D5a）、REV-106/108

## 1. 结论

**结论：有条件通过**

P0=0；**P1=2**；P2=6；P3=5。字段/枚举与 trace v7 对齐、侧效/多文件/bash/MCP/Python/技能不记录、成功/error/timeout/cancelled 发射、默认 sink no-op、不写 trace DB、RFC3339ms Z 无新依赖均通过；harness **334/0/1 默认与 features**、workspace **1158/0/3**、pytest **183**、clippy/fmt/check/no-default 全绿。

两项需关闭：

- **C1 敏感数据未脱敏即进入 ToolCallRecord**：`build_tool_call_record` 直接 `call.args.clone()`，`result`/`error` 也保留完整 Value；bash 的显式 `env` 值、MCP/Python 参数中的 token/header 等会原样进入 D5b2 trace 的 `args_json/result_json/error_json`。DR-014 §1 明确「env/headers/token 不写；既有 `SENSITIVE_KEY_TOKENS` 脱敏复用」，当前只有 `ToolCallStart/End` 摘要脱敏，record 路径没有。
- **C2 节点 deadline 超时路径零记录**：`ToolCallStart` 已发射后，`execute_tool` 若 `ctx.node_deadline_remaining_ms()==0` 返回 `Err(RunFailure::InstanceTimeout)`；外层 `Err(other) => return Err(other)` 直接退出，没有构造 timeout `ToolCallRecord`。AC-24d「每次工具调用恰 1 行」在工具已经开始（Start 已发）时被违反；现有 timeout 测试只覆盖工具自身超时/假 bash timeout。

## 2. 四维评审

| 维度 | 结论 | 依据 |
|---|---|---|
| 功能正确性 | ⚠️（C1/C2） | 各工具字段/侧效/bash/source、成功/error/timeout/cancelled 路径主语义通过；敏感值与节点 deadline 边界未覆盖 |
| 结构合理性 | ✅ | record 类型/trace 映射/工具元数据分层；Recording sink 可测；无 DB/新表；additive executor API |
| 逻辑性 | ✅（P2） | 单次发射、取消先回收再记录、失败不阻塞；P2 见 §3 |
| 可读性 | ✅（P3） | 字段注释/trace 对齐说明/开放项清楚；P3 为测试补齐 |

## 3. 发现清单

### P1 · 合并前必须关闭

| # | 位置 | 问题 | 条件 |
|---|---|---|---|
| C1 | `agent_loop::build_tool_call_record`（`args: call.args.clone()`；`result`/`error` 完整 Value） | 记录原始参数/结果/错误，未复用 `SENSITIVE_KEY_TOKENS`，也未对 bash `env` 值做专项剔除；D5b2 按 TraceInput::ToolCall 写 `args_json/result_json/error_json` 会落库 token/header/env 值。DR-014 §1 明确不写 env/headers/token | 在 record 构造或 D5b2 bridge 前统一脱敏（env 值、敏感键、MCP headers/token；保留 hash/可审计结构）；补 bash `env:{"API_KEY":"sk-..."}`、MCP args 敏感键测试，断言 record/trace 不含明文。若脱敏需在 bridge 做，须在 D5b2 任务与测试中显式落条件 |
| C2 | `agent_loop::execute_tool`（`remaining == 0` → `Err(InstanceTimeout)`）＋外层 `Err(other)` 分支 | `ToolCallStart` 已发后无 record；违反“每次工具调用恰 1 条/status timeout”。触发点：节点 deadline 在 ToolCallStart 与 worker 启动之间耗尽 | 在返回 `InstanceTimeout` 前按 `source` 构造 timeout record（result=None/error=timeout 形状/duration）；或把 deadline 检查移到 `ToolCallStart` 之前（但需保持 Start/End 语义）。补 TestCtx remaining=0 用例断言恰好 1 条 timeout、run 仍 `NodeError::Timeout` |

### P2 · 建议

| # | 位置 | 问题 | 建议 |
|---|---|---|---|
| N1 | `cancelled_tool_call_record` / timeout error | bash 取消/超时记录 `bash=None`，丢失 command/cwd/profile_sha256；任务要求 bash 元数据（失败/回滚 side_effect 为 None 已接受，但 bash 元数据未明确豁免） | 对已开始 bash 的 timeout/cancel 保留 request 摘要与 profile sha；补测试 |
| N2 | 默认 sink no-op / panic | 无显式「默认 `None` 时零回调、既有行为 golden」测试；`record_tool_call_safely` 吞 panic 无测试 | 补 agent_loop 默认 no-sink golden + sink panic 不改变 run 的测试 |
| N3 | `execute_builtin_tool` 非 bash | 文件/搜索同步调用无取消检查；run cancel 在文件工具执行中到达时仍以 Ok 记录（随后下一轮才取消） | 需要时在快速同步工具前后检查 cancel；或在 D5b2 文档化「瞬时工具不保证 cancelled 状态」 |
| N4 | MCP/Python 错误/超时/取消 | 只有 source 用例；error/timeout/cancel 状态与 record 形状未测（Lead 已把取消测试留 D5b2） | D5b2 e2e 补；本批可补 Fake 级 error/timeout |
| N5 | `BashCallRecord` | 无 `stdout_truncated/stderr_truncated` 独立字段，仅 `truncated` 总标记；trace writer 会按长度再算 `bash_truncated` | 口径可接受；D5b2 映射时以 writer 独立计算为准并测试 |
| N6 | `ToolCallRecord` args/result/error 大小 | record 持有完整 Value（可能很大），trace 侧才 cap；大参数在 sink/bridge 前有内存峰值 | 可接受（DR 明确 trace 侧 cap）；D5b2 前评估上限 |

### P3 · 测试/文档

| # | 位置 | 问题 | 建议 |
|---|---|---|---|
| N7 | `ToolCallRecord` 字段 | 无 run_id/instance_id/node_id/attempt（Lead 预裁决 D5b2 bridge 补齐，接受） | D5b2 明确 fail-closed 映射 |
| N8 | 枚举映射 | 本地 `ToolCallSource/Status` 与 trace 字符串一致，但无 parse/未知值处理（D5b2 fail-closed，接受） | D5b2 补 |
| N9 | truncated hint | 工具自身 hint 与 writer 独立计算（D5b2 回放 fail-closed，接受）；无测试覆盖 hint 与 writer 冲突 | D5b2 补 |
| N10 | path 口径 | 侧效 path 为参数原文/补丁 strip 后路径；D6 文档（接受） | D6 |
| N11 | `ctx` 时钟 | 默认 wall clock 不可注入（接受）；`rfc3339_millis_from_epoch` 已测 epoch/闰年/毫秒 | 后续可注入 clock |

## 4. 验证证据

worktree `/Users/zyzs/lczyzs/code/zgraph/.worktrees/harness-dsl-dev`，HEAD `443ca3d`，base `fa70439`，工作区干净；`CARGO_HOME=/Users/zyzs/lczyzs/code/zgraph/.cargo-home`，未设 `CARGO_TARGET_DIR`：

```bash
cargo test -p zgraph-harness                 # 335 tests：334 passed / 0 failed / 1 ignored
cargo test -p zgraph-harness --features mcp  # 335 tests：334 passed / 0 failed / 1 ignored
cargo test --workspace                       # 76 targets：1158 passed / 0 failed / 3 ignored
pytest -q                                    # exit 0；183 passed
cargo clippy --workspace --all-targets --locked -- -D warnings  # clean
cargo fmt --all -- --check                   # exit 0
cargo check -p zgraph-harness --no-default-features  # clean
bash scripts/dev/check.sh                    # PASSED
```

- **与自报一致**：harness 334/0/1（默认/features）、workspace 1158/0/3、pytest 183；clippy/fmt/check/no-default 全绿；9 files 仅 harness；无 Cargo 变更；`git status` 空。
- **关键断言**：字段/枚举字符串与 trace 对齐；write/edit/单文件 apply_patch side_effect（sha/bytes）、多文件 None+files[]、read/diff/grep/glob 无 side effect；bash command/cwd/exit/stdout/stderr/profile_sha/truncated；MCP/Python source；skill_view 不记录；成功/error/timeout/cancelled 各恰 1；取消先 killpg 再记录（Fake BlockingSandboxRunner）；默认 None no-op；无 trace DB 写入；RFC3339ms Z（epoch/闰年/毫秒 + 集成格式）。
- **未覆盖（C1/C2/P2）**：敏感 env/header 脱敏、节点 deadline 超时记录、bash 取消/超时 metadata、默认 no-sink golden、MCP/Python cancel/timeout（D5b2）、文件工具中途取消。

## 5. Lead 开放项确认

- 不含 run/instance/node/attempt：**接受**（D5b2 bridge 补齐）。
- 本地枚举字符串对齐、D5b2 fail-closed 映射：**接受**。
- truncated hint 与 writer 独立计算、回放 fail-closed：**接受**。
- 失败/回滚 side_effect=None：**接受**。
- path 口径 D6 文档：**接受**。
- MCP 非 JSON→JSON string：**接受**，D5b2 还原语义。
- MCP/Python cancel 测试留 D5b2 e2e：**接受**。
- ctx 默认 wall clock：**接受**。
- 无 Cargo 变更、无 trace DB：**已核实**。

## 6. 遗留问题

- **C1/C2**：敏感数据脱敏 + 节点 deadline timeout record；补测试后复审转「通过」。
- **P2 N1–N6**：bash 取消/超时 metadata、默认 no-sink/panic、文件工具取消、MCP/Python error/timeout、bash truncated 口径、大 Value 内存。
- **P3 N7–N11**：D5b2 映射/回放/D6 path/clock。
- 无 P0；C1/C2 未关闭前不可 merge。
