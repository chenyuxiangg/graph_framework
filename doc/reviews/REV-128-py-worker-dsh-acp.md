# REV-128 · py-worker · v0.5 F5a DSH ACP transport

- **分支**：`feat/py-worker-dsh-acp`
- **作者**：`dsl-dev`（commit `dd2e244` + `a6abecc`，基于 518ef34；main 026ad23，合并前需 rebase）
- **评审人**：`reviewer-b`（独立评审，≠ 作者；跨模块 python/py-worker Lead 已批准，第二评审 Lead 追加）
- **变更范围**：`518ef34..a6abecc`（2 files，+1486/−87）：`python/zgraph_worker/adapters/dsh_acp.py`（+950/−62）、`python/tests/test_adapters_dsh_acp.py`（+536/−25）；无 Rust/Cargo
- **评审时间**：2026-11-04
- **关联**：task-120/123、DR-020 DSH FINAL、spec 05 §5.16.4 / 07 AC-28、`PROTOCOL.md`、REV-128

## 1. 结论

**结论：通过**

P0=0；P1=0；P2=7；P3=3。ACP v1 NDJSON transport、懒 initialize/protocolVersion=1/agentInfo.name 前缀、session/new 绝对 scratch cwd + `mcpServers=[]`、prompt/cancel/close、update 映射、permission 默认 cancel、cancel 2s、事件/结果上限、scratch 隔离、fake ACP 真子进程 + 真机握手/无 key 降级均通过；pytest 默认 **195 passed / 2 skipped**，`ZGRAPH_REAL_DSH=1` **197 passed**，check.sh 全绿，既有 183 不回归。

未发现 P1；F5c/F5e 真 permission/tool/usage/cancel 与 Rust usage 偏差、rebase 义务见 §5/§6。

## 2. 四维评审

| 维度 | 结论 | 依据 |
|---|---|---|
| 功能正确性 | ✅ | ACP v1 framing/initialize/session/prompt/cancel/close/update/permission/stopReason/错误映射；真机握手与无 key 降级 |
| 结构合理性 | ✅ | `ParentChannel`/`AcpChannel` 无 read-ahead、`AcpClient` RPC、`AcpAdapter` 桥分层；仅 py 文件 |
| 逻辑性 | ✅（P2） | 懒 init、cancel 2s、scratch/DHS_HOME、事件上限、usage 不伪造；P2 见 §3 |
| 可读性 | ✅（P3） | 模块 docstring/内联注释/错误闭集清晰；P3 测试/文档 |

## 3. 发现清单

### P2（不限于本波修复；均不阻断 F5a）

| # | 位置 | 问题 | 建议 |
|---|---|---|---|
| N1 | `ParentChannel.deferred`/`AcpChannel`/`send` | 无队列/单帧大小上限；prompt 期间非 cancel 父帧全部暂存；恶意/异常对端可导致内存增长 | 加 deferred/行/事件帧上限；超限按 protocol 错误 fail-closed；F5c/后续 |
| N2 | `args_summary` | 对 dict 原样返回 `rawInput`，未限制键/值大小；tool_start 事件可能很大/含敏感值 | 按 tool args 摘要截断/脱敏（与 harness tool record 口径一致） |
| N3 | `env_for_dsh` | `DSH_HOME`/`TMPDIR` 强制 scratch，但 `HOME` 继承用户主目录；DSH 若忽略 DSH_HOME 写 `~/.cache`/`~/.dsh` 存在残余 | 明确 DSH 是否仅依赖 DSH_HOME；可显式 `HOME=scratch`（需评估 DSH 行为）；D6 文档 |
| N4 | `AcpClient.request` | initialize/session/new 期间不轮询父 cancel；仅 prompt 处理 cancel。父在 init 阶段 cancel 要等 init 超时/完成 | 将父 cancel/EOF 检查扩展到 init 路径；F5c |
| N5 | `AcpAdapter._ensure_session` | `session.reuse_key` 未使用（仅 `reset`）；同一 adapter 默认复用 session | 与 harness session 语义（reuse_key/reset）对齐；F5c/文档 |
| N6 | `AcpClient.shutdown`/`AcpAdapter.shutdown` | shutdown 正常退出后仍 SIGKILL 进程组；`client.shutdown()` 抛错时 `scratch.cleanup()` 不执行 | finally 保证 cleanup；正常退出后仅清理仍存活组（可接受 SIGKILL 残留风险已登记） |
| N7 | 真机覆盖 | 真 permission/tool/usage/cancel 未 exercise（F5c/真 key 门控）；usage 不可得已知偏差：ACP 无 input/output token，result 省略 usage，Rust `plugin.rs` 当前 None→Usage::zero，F5e 修 | F5c/F5e 关闭；F5a 不伪造 token 已正确 |

### P3（测试/文档）

| # | 位置 | 问题 | 建议 |
|---|---|---|---|
| N8 | `test_real_dsh_initialize_session_close` | 断言 agentInfo.name 精确 `deepseek-harness-acp`（adapter 仅前缀）；DSH developer preview 版本/name 漂移风险 | D6 记录版本；可放宽为前缀（与 adapter 一致） |
| N9 | `DSH sandbox` | 非双重 sandbox；真实 DSH 工具副作用/prompt 需用户终端/真 key | F5d/F6 证据；不伪造 |
| N10 | `SIGKILL scratch 残留` | 进程组 SIGKILL 时 scratch 可能残留；文档已接受 | D6 清理策略（下次启动清 orphan）可选 |

## 4. 验证证据

worktree `/Users/zyzs/lczyzs/code/zgraph/.worktrees/py-worker-dsl-dev`，HEAD `a6abecc`，base `518ef34`（main `026ad23`，合并前 rebase）；工作区干净；`PATH` 使用项目 `.venv/bin`：

```bash
pytest -q                         # 197 collected：195 passed / 2 skipped（REAL_DSH 两例 skip）
ZGRAPH_REAL_DSH=1 pytest -q       # 197 passed / 0 skipped（真实 DSH 握手 + no-key prompt 降级）
bash scripts/dev/check.sh         # PASSED
```

- **与自报一致**：默认 195/2 skipped、REAL_DSH 197；既有 183 不回归（197=183+14 adapter，其中 2 real 默认 skip；实际 adapter 测试 16 含 2 real）。
- **真机证据（本机实跑）**：`ZGRAPH_REAL_DSH=1` 两例通过：
  - `initialize` 成功（protocolVersion=1、agentInfo.name `deepseek-harness-acp`）、`session/new` 成功、`session/close`/EOF；
  - 无 `DEEPSEEK_API_KEY` 时 `session/prompt` 返回 JSON-RPC error，adapter 映射 `HARNESS_PLUGIN_ERROR{reason:"protocol", message contains "no API key"}`，无 result——**未发起模型请求**。
- **Fake ACP**（真子进程 NDJSON）：懒 hello 不 spawn；update 映射 delta/reasoning/tool_start/tool_end；permission 默认 cancelled、allow_once/reject_once；session reuse/reset；stopReason end_turn/max_tokens/cancelled；version mismatch/wrong agent name/spawn failure/prompt error；scratch DSH_HOME/TMPDIR/cwd；cancel 发 session/cancel 且无 result；wrong protocol/invalid hello 无输出。
- **安全**：`DSH_BIN`→PATH→常见路径（不唯一硬编码）、`start_new_session=True` 进程组、DSH_HOME/TMPDIR scratch、mcpServers=[]、不写 token/key；无 Rust/Cargo。

## 5. 开放项/真机限制确认

- **F5c**：真 permission/tool/usage/cancel 未 exercise（无真 key/工具门控）；permission 当前只 stderr 结构化日志，后续上报；接受。
- **F5e**：ACP 无 token 计数 → result 省略 usage；Rust `plugin.rs` 把 None 回退 `Usage::zero()`（非 Unavailable）为已知偏差；F5e 修 Rust plugin/CLI，**F5a 不伪造 usage 正确**。
- **DSH_HOME 隔离**：强制 scratch；HOME 继承残余见 N3。
- **版本漂移**：agentInfo.name 前缀 + 真机 exact 断言；developer preview。
- **DSH sandbox**：不双重；真实工具副作用留 F5d/F6/普通终端。
- **SIGKILL scratch 残留**：接受，D6 清理策略。
- **rebase**：分支基于 518ef34，main 026ad23；merge 前 rebase（保留 F3a1/F3b 接口），不阻塞评审。

## 6. 遗留问题

- **F5c/F5e/F5d**：真 permission/tool/usage/cancel、Rust usage Unavailable、真实 DSH 工具链。
- **P2 N1–N7**：帧/args 上限、HOME 隔离、init cancel、reuse_key、cleanup finally、真机覆盖。
- **P3 N8–N10**：版本/name 漂移、sandbox 真机、SIGKILL scratch。
- 无 P0/P1；F5a 可 merge（先 rebase），F5c/F5e 后续波次。
