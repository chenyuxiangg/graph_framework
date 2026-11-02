# REV-100 · harness · v0.3 残余修复（MCP 重启上限/截断/http_insecure/Codex flake）

- **分支**：`fix/harness-v03-residual`
- **作者**：`dsl-dev`（单提交 `789b738`，基于 main `6515f12`）
- **评审人**：`reviewer-b`（独立评审，≠ 作者）
- **变更范围**：`6515f12..789b738`（4 files，+194/−37）：`crates/zgraph-harness/src/mcp/mod.rs`（+85/−8）、`mcp/real.rs`（+32/−11）、`mcp/testing.rs`（+16）、`plugin.rs`（+61/−18）
- **评审时间**：2026-11-02
- **关联**：task-65/66、DR-010 §4/§8、v0.3 e2e 暴露项、REV-099 P2（codex test flake）

## 1. 结论

**结论：通过**

P0=0；**P1=0**；P2=2；P3=5（均建议/测试补齐，不阻断）。4 项修复与冻结语义一致：MCP 重启改为**每 run 每 server 全局 ≤1 次**（spawn=2；超限 `TOOL_ERROR{reason:"transport_closed"}`）；>32768 chars 最终 ToolResult 统一 `{"truncated":true,"is_error":<bool>,"text":"…[mcp result truncated]"}`（Fake/Real 同路径）；`http://` 未 allow → `HARNESS_MCP_ERROR{reason:"http_insecure"}`；Codex cancel 测试有界（≤10s/30s），门禁全绿，无默认路径回归。

## 2. 四维评审

| 维度 | 结论 | 依据 |
|---|---|---|
| 功能正确性 | ✅ | 4 项语义逐条与断言一致；Fake/Real 最终截断路径统一；默认无 MCP/harness 不变 |
| 代码结构合理性 | ✅ | 重启计数落在 run 级 `PoolState.clients[server].restarts`；real client 只负责拼全文、McpRun 统一截断；http reason 在 registry parse 单点；Codex 仅测试有界化 |
| 逻辑性 | ✅（P2 备注） | MCP 重启计数跨 call 共享、成功后仍保留全局额度；超限返回 Closed 由 McpRun 映射 transport_closed；截断只对最终 ToolResult；不改变无 MCP 路径 |
| 可读性 | ✅（P3 备注） | 注释明确 DR-010 §4/§8、REV-100 #2；测试名称/断言对应 e2e 语义；P3 为测试覆盖范围 |

## 3. 发现清单

### P2 · 建议

| # | 位置 | 问题 | 建议 |
|---|---|---|---|
| N1 | `mcp/mod.rs::tests::same_run_restarts_at_most_once_across_calls` | 只覆盖**同一 McpRun** 的三次 call；未覆盖同一 run 两个 agent 共享 `McpPool`（restart 额度应共享）、以及换 run 后新 pool 重置。实现（`McpSessionRuns`/`McpPool` per run）看起来正确，但断言未锁该边界 | 补 `same_run_two_agents_share_restart_budget` 与 `new_run_resets_restart_budget`（或 executor 级）测试；不阻断 |
| N2 | `mcp/mod.rs::truncate_result` / `real.rs::call_result_text` | 最终 `text` = 32768 chars + `…[mcp result truncated]`，总长略超 32768；任务只要求 `truncated:true/keys/suffix`，但 e2e 若断言 `text <= 32768` 会不一致。另 real client 先拼完整文本（可达很大内存），最终才截断 | 给 e2e 的精确口径：`text` 以 `…[mcp result truncated]` 结尾、keys 三键、`truncated:true`，不要断言 text 严格 ≤32768；若要硬上限，head 取 `MAX - suffix.len()` 并补测试 |

### P3 · 台账/测试

| # | 位置 | 问题 | 建议 |
|---|---|---|---|
| N3 | `real.rs` 测试 | 只测 `call_result_text` + helper `truncate_result`，未走 Real→McpRun 的完整最终 ToolResult 形状；`is_error:true` 的截断形状未断言 | V5 real e2e/单测补 `is_error=true` 三键形状；当前 Fake/Real 共用 McpRun 路径 |
| N4 | `mcp/mod.rs` 截断 | `truncate_result` suffix 固定 `…[mcp result truncated]`；e2e rebase 需使用该精确后缀与 key 集合；`ok = !is_error` | 在 e2e 断言中固定：`{"truncated":true,"is_error":false,"text":"<head>…[mcp result truncated]"}`、keys 恰 `truncated/is_error/text`、`ok=true` |
| N5 | `http_insecure` | registry parse 负例覆盖；`allow_insecure_http:true` 的 http 正例**未单测**（代码分支存在） | e2e rebase 断言：`HARNESS_MCP_ERROR`、`detail.reason="http_insecure"`、`detail.server`；补 allow=true 可连正例 |
| N6 | `plugin.rs` codex flake | 已加 10s pid 等待/30s 场景超时/清理；仍为**失败而非 hang**，极端负载下可能失败 | 可接受；后续 CI 若命中按测试不稳定处理，不建议再放宽 |
| N7 | 变更范围 | 仅 harness 4 文件；无跨模块；无默认路径改动 | 无需动作；e2e-dev 按新语义 rebase |

## 4. 验证证据

worktree `/Users/zyzs/lczyzs/code/zgraph/.worktrees/harness-dsl-dev`，HEAD `789b738`，base `6515f12`，工作区干净；`CARGO_HOME=/Users/zyzs/lczyzs/code/zgraph/.cargo-home`，未设 `CARGO_TARGET_DIR`：

```bash
cargo test -p zgraph-harness                 # 236 tests：235 passed / 0 failed / 1 ignored
cargo test -p zgraph-harness --features mcp  # 236 tests：235 passed / 0 failed / 1 ignored
cargo test --workspace                       # 72 targets：1006 passed / 0 failed / 3 ignored（连跑 2 次一致）
pytest -q                                    # exit 0；183 passed
cargo clippy --workspace --all-targets --locked -- -D warnings  # clean
cargo fmt --all -- --check                   # exit 0
cargo check -p zgraph-harness --no-default-features  # clean
bash scripts/dev/check.sh                    # PASSED
```

- **计数与自报一致**：harness 235/0/1（默认与 mcp feature 均为 236 tests）、workspace **1006/0/3**（连跑 2 次）、pytest **183**；clippy/fmt/check/no-default 全绿。
- **新语义断言值（供 e2e rebase）**：
  - **MCP 重启**：同 run/server 第 1 次 Closed → 重启（`connects/spawn=2`）；第 2 次 Closed → 不再 spawn，`TOOL_ERROR` content `error.reason="transport_closed"`；传输恢复后同一 client（connect 仍 2）可成功；`MCP_MAX_RESTARTS_PER_RUN=1`。
  - **MCP 截断**：结果 chars > **32768** → ToolResult JSON 恰三键 `{"truncated":true,"is_error":<原 is_error>,"text":"<head>…[mcp result truncated]"}`；`ok = !is_error`；≤32768 返回原文本不加 wrapper。Fake/Real 均在 `McpRun::execute_tool` 最终路径。
  - **insecure http**：`http://` 且未 `allow_insecure_http:true` → `HARNESS_MCP_ERROR`，`detail.reason="http_insecure"`，`detail.server=<name>`（非 config_invalid）。
  - **codex cancel test**：pid 等待/回收各 ≤10s，场景 ≤30s；失败时先 `close_provider()` 清理。
- **范围**：仅 4 harness 文件；无 cli/kernel/e2e/Cargo 改动；`git status` 空。
- **无默认路径回归**：无 MCP/harness 的既有测试全绿；默认 `cargo test --workspace` 1006/0/3。

## 5. 遗留问题

- **V5/e2e**：e2e-dev rebase 按 §4 精确断言（spawn=2/transport_closed/三键/suffix/http_insecure）；REV-101 后 docs REV-102。
- **P2 N1/N2**：补多 agent 共享/换 run 重置测试，及 e2e 截断 text 长度口径；P3 N3–N7 记录/测试补齐。
- 无 P0/P1；无附加条件；分支可合并。
