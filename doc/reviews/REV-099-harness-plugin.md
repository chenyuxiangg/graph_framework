# REV-099 · harness · V4 可插拔 harness / Codex + DSH adapter

> **跨模块声明（Lead 已批准；第二评审由 Lead 追加）**：`crates/zgraph-cli/src/run_cmd.rs`（5 行 `close_provider` 出口）、`crates/zgraph-kernel/src/types.rs`（append-only `HarnessErrorKind::Plugin`/`CODE_HARNESS_PLUGIN_ERROR`）、`python/zgraph_worker/adapters/{codex,dsh_acp}.py` + tests（py-worker，Lead 批准）、`crates/zgraph-harness/Cargo.toml`/`Cargo.lock`（unix `nix` target 依赖，+1 lock 行）。其余主体在 `crates/zgraph-harness/**`。

- **分支**：`feat/harness-plugin`
- **作者**：`dsl-dev`（接手 kernel-dev；`23fa568` REV-098 P2⑥ + `0584931` V4 主实现）
- **评审人**：`reviewer-b`（主审；≠ 作者）
- **基于**：main `f204368`
- **变更范围**：`f204368..0584931`（19 files，+3252/−36）：`plugin.rs`（1428）、`provider.rs`（261）、`config.rs`、`executor.rs`、`mcp/mod.rs`、`error.rs`、`constants.rs`、`lib.rs`、`agent_loop.rs`、`PROTOCOL.md`、cli `run_cmd`、kernel `types`、python adapters/tests、Cargo
- **评审时间**：2026-11-02
- **关联**：task-61/62、DR-011 §1–§9、DR-008、spec 05 §5.1.3/§5.9.4/§5.14.3、07 AC-20 a–h、00 §0.3/§0.4/§0.5、research §C、REV-098

## 1. 结论

**结论：通过**（首轮为「有条件通过」；C1–C3 已于 `3bbe4b5` 关闭，见 §8）

P0=0；**P1=0**（首轮 3 项已关闭）；P2 残余（见 §8）；P3 残余。builtin 默认路径、AgentConfig.harness 解析/校验、能力冲突、fake subprocess 协议、取消/超时/进程池/close_provider、kernel/CLI 接线、MCP P2⑥ 与门禁均通过；首轮 3 个 P1 的原始记录如下（**均已关闭**）：

- **C1**：外部 harness 路径**未写框架侧 `llm_calls`**（DR-011 §4：每节点 1 条 `request_json={provider,agent_id,session_key,inputs_hash}`、`response_json=四字段+event_count`；实现 `ProviderRequest.recorder` 对 subprocess 完全未使用）。
- **C2**：协议**单行 1 MiB 上限不是硬上限**：`read_frames` 用 `read_until` 先完整读入整行，再检查 `read > LINE_MAX`；恶意/异常 adapter 发无换行巨行时会在检查前无限增长内存（`big_line` 2 MiB 测试只证明最终报错，不证硬上限）。
- **C3**：Codex adapter 的**子进程取消/超时有孤儿风险**：父进程 `shutdown` 发 `cancel` 后立即对 adapter 进程组 SIGTERM；Codex 子进程由 `start_new_session=True` 建**独立会话/进程组**，而 adapter 只在读到 `cancel` 后才 `killpg` Codex；SIGTERM 先到会直接杀 adapter，Codex 子进程可能残留（AC-20 d 无孤儿；fake Codex 未覆盖 in-flight cancel）。

## 2. 四维评审

| 维度 | 结论 | 依据 |
|---|---|---|
| 功能正确性 | ✅（P2 残余） | 首轮 P1 已关闭：外部 `llm_calls` 框架侧写入、行硬上限、Codex 取消无孤儿；P2 残余见 §8 |
| 代码结构合理性 | ✅ | `AgentProvider` seam 清晰；`provider.rs` 只包装 builtin；`plugin.rs` 分层（spawn/frames/exchange/result/error）；executor 只选择 provider；CLI 出口 close；kernel/error append-only |
| 逻辑性 | ✅（P2 残余） | 协议超时/取消/进程组/close 幂等/Session reset 与 DR 对齐；同批 P2 plan/usage_update、stderr、hello limits、cancel 写超时、Codex stderr、8 MiB 已修复；N6/N8 与 P3 见 §8 |
| 可读性 | ✅（P3 备注） | PROTOCOL.md 对外；注释解释 deviations/Lead 裁决；P3 为测试/文档补齐 |

## 3. 发现清单

### P1 · 首轮条件（已于 `3bbe4b5` 关闭）

| # | 位置 | 问题 | 条件 |
|---|---|---|---|
| C1 | `plugin.rs::SubprocessProvider::run` / `ProviderRequest.recorder`（provider.rs:96） | 外部 subprocess 路径**没有任何 `llm_calls` 记录**；`recorder` 仅 builtin 使用。DR-011 §4 明确框架每节点写 1 条（provider/agent/session_key/inputs_hash + 四字段/event_count），AC-20 h 只要求「不双重」 | 在 provider 层（exchange 后）用 `request.recorder` 写 1 条 `LlmCallSnapshot`：request={provider,agent_id,session_key,inputs_hash}、response={四字段,event_count}；补 `--record=full` fake adapter 测试断言恰 1 行且无原始 JSONL/正文 |
| C2 | `plugin.rs::read_frames`（739-782） | `read_until` 无上限读入整行后才 `if read > LINE_MAX`；巨行/无换行可 OOM，违反 DR-011 §2.1/§3「单行 1 MiB」硬上限（`big_line` 测试的 2 MiB 行已完整进内存） | 以有界方式读取：超过 1 MiB 即丢弃/终止（`take`/分块，缓冲区硬顶），并保留总输出 8 MiB 检查；补「1 MiB 无换行巨行」测试断言 `line_too_large` 且内存/时间有界 |
| C3 | `python/zgraph_worker/adapters/codex.py`（`collect_events_with_cancel` + `start_new_session=True`）与 `plugin.rs::Adapter::shutdown` | 取消/超时时父进程 `cancel` 后立即对 adapter 组 SIGTERM；adapter 未安装 SIGTERM handler，仅靠 select 抢读 cancel 才终止 Codex 组；Codex 子进程独立 session，SIGTERM 先杀 adapter → Codex 孤儿（AC-20 d） | adapter 安装 SIGTERM/SIGINT handler 立即 `terminate(codex_process)`（或父进程 cancel 后给 adapter 短暂转发窗口再 SIGTERM）；补 fake Codex「长睡 + 父 cancel + 子进程不存在」测试（检查 pgid/pid） |

### P2 · 建议（不阻断本次 C1–C3 关闭后可另登记）

| # | 位置 | 问题 | 建议 |
|---|---|---|---|
| N1 | `plugin.rs::handle_event`（591-609） | DR-011 §2.1 规定 `plan`→progress、`usage_update`→最终 usage（不新增 kind）；实现把 `plan` 当未知 event 忽略+warn，测试 `unknown_event` 用 `plan` 断言忽略，**与 DR 相反**；`usage_update` 也被忽略 | `plan` 映射为 `Progress`（message/percent 取安全默认），`usage_update` 更新最终 usage（若协议支持）；修正测试 |
| N2 | `plugin.rs::drain_stderr`/`stderr_tail`（784-805） | 只保留**前** 8 KiB（`guard.len() < MAX`），字段叫 `stderr_tail`；真实失败信息通常在尾部；`exit_failure` 携 head 而非 tail | 环形缓冲保留最后 8 KiB（或明确文档「head」）；补超 8 KiB stderr 测试 |
| N3 | `plugin.rs::handshake` hello limits（331-384） | `limits.max_output_bytes` 填 **1 MiB 单行**而非总输出 8 MiB；`node_timeout_ms` 填常量 300000 而非本次节点超时；adapter 可能据此误配 | hello 填真实总输出/本次 node timeout（或文档明确为「最大单帧」）；Codex 忽略也保持一致口径 |
| N4 | `plugin.rs::shutdown`（711-737） | `write_frame(cancel)` 无超时，stdin 满/子进程不读时 shutdown 可永久挂起；之后才 SIGTERM | cancel 写入加短超时（如 500ms）；失败直接进入 SIGTERM/SIGKILL |
| N5 | `codex.py::collect_events_with_cancel`（约 180-210） | Codex 的 `stderr=PIPE` 未并发排空；真实 codex 写 stderr > 管道容量会阻塞/死锁到父 deadline；`select` 在 stdin EOF 后可能忙轮询 | 并发 drain stderr（tail 8 KiB）；stdin EOF 后禁用该 select 分支 |
| N6 | `config.rs::check_subprocess_capabilities`/`load_agent_with_options` | 外部 harness 仍强制 `role_prompt_file`/`constraints_file` 读取与 model 解析；外部 loop 不使用，配置负担/误导 | 文档说明外部仍复用 AgentConfig 身份/模型；或在 DR/V5 明确不要求 role_prompt 之外字段 |
| N7 | `executor.rs` builtin 测试 | `explicit_builtin_harness_behaves_like_the_default_path` 只比较状态/snapshot 数量，**未断言 run_output/事件/trace 逐字节**；AC-20 a 的 golden 强度不足 | 补 run_output、事件序列、trace 行逐字节（或既有 golden）；无 harness 默认路径复用同一断言 |
| N8 | `plugin.rs` tests | 无 `L2 events/outputs 不 spawn/close` 断言、无 CLI `close_provider` 所有出口、无 `spawn_failed`/`handshake` timeout/`total_output` 8 MiB；Lead 预裁决 7 允许 V5 覆盖 8 MiB/in-flight，但 C 级测试仍建议补 | 在 C1–C3 修复时顺带补 C1 测试；N8 其余登记 V5 计划 |

### P3 · 台账/文档

| # | 位置 | 问题 | 建议 |
|---|---|---|---|
| N9 | `plugin.rs` 错误/协议 | `parse_error_frame` 的 message 无长度上限；`plugin_error` 的 `extra` 可覆盖 `code`/`stage`/`reason`；`next_frame` 握手期非法 JSON 不计数+warn | 限制 error message；保留 detail 保留键；握手期按 exchange 同阈值 |
| N10 | `Adapter::close_provider` | 每次 shutdown 固定 sleep 2s（即使 SIGTERM 后进程已退出）；一个 run 多个 adapter 串行 | 先 poll 等待短间隔，超时再 sleep；close 有总上限 |
| N11 | `Hello`/PROTOCOL | `PROTOCOL.md` §4 未写 `llm_calls` 框架写入口径；hello limits 与 §3 表口径需对齐 | V5 docs 回写；C1 修复后 PROTOCOL 加 audit 段 |
| N12 | `codex.py` | 只覆盖 success/missing binary 测试；`turn.failed` 用 `reason="protocol"`；`schema_violation` 字段未使用；8 类 ThreadEvent 中 item.started/updated 未显式处理 | 补 turn.failed/取消/模式测试；reason 映射到 `exit_nonzero`/`protocol` 明确；V5 真实环境验证 |
| N13 | `dsh_acp.py` | 仅握手+结构化 error 骨架（Lead 已接受 best-effort） | release notes 标注「adapter 骨架，待真实环境验证」 |
| N14 | 环境变量 | `SubprocessHarness` 继承父进程全部 env（DR §5「env 白名单透传」）；`HarnessConfig.env` 只追加同名覆盖 | 若要求白名单，明确允许变量集（至少 PATH）或文档化继承 |

## 4. 验证证据

worktree `/Users/zyzs/lczyzs/code/zgraph/.worktrees/harness-dsl-dev`，HEAD `0584931`，base `f204368`，工作区干净；`CARGO_HOME=/Users/zyzs/lczyzs/code/zgraph/.cargo-home`，未设 `CARGO_TARGET_DIR`：

```bash
cargo test -p zgraph-harness                  # 227 targets：226 passed / 0 failed / 1 ignored
cargo test -p zgraph-harness --features mcp   # 226 targets：225 passed / 0 failed / 1 ignored
cargo test --workspace                        # 72 targets：996 passed / 0 failed / 3 ignored
pytest -q                                     # exit 0；180 passed（dots 180）
cargo clippy --workspace --all-targets --locked -- -D warnings  # clean
cargo clippy -p zgraph-harness --all-targets --features mcp --locked -- -D warnings  # clean
cargo fmt --all -- --check                    # exit 0
cargo check -p zgraph-harness --no-default-features  # clean
bash scripts/dev/check.sh                     # PASSED
```

- **与自报对比**：harness 默认 226/0/1（自报 226/1）、features mcp 225/0/1（自报 225/1）；workspace **996/0/3**（自报 995；实测多 1 个通过，无失败）；pytest **180**（自报 180）；clippy/fmt/check/no-default 全绿。
- **范围**：19 files；cli 5 行 close_provider、kernel 33 行 append-only、py-worker adapters/tests、Cargo.lock +1（nix target）；无 docs/e2e 改动、无 token/密钥、`git status` 空。
- **关键断言**：AgentConfig.harness 解析矩阵（unknown kind/缺 command/builtin+options/protocol 精确/范围）、能力冲突 tools/skills/mcp + 节点 max_iterations、fake adapter hello/event/result/error/cancel/timeout/limits/exit/未知 event、Codex fake PATH 链路、builtin 显式等义、close_provider 幂等、MCP P2⑥ server 段归一化、kernel Plugin 码 append-only。
- **真实环境**：Codex/DSH 未验证（Lead 预裁决接受；release notes 需标注骨架）。

## 5. 跨模块核对

| 文件 | 改动 | 结论 |
|---|---|---|
| `crates/zgraph-cli/src/run_cmd.rs` | 2 处 `close_provider`（start error / run 结束） | 机械、所有 run 出口覆盖；无 runner 语义变化 |
| `crates/zgraph-kernel/src/types.rs` | `Plugin` kind + `CODE_HARNESS_PLUGIN_ERROR` | append-only；既有码不变；`NodeError` 映射测试 |
| `python/zgraph_worker/adapters/**` | Codex adapter + DSH 骨架 + tests | py-worker 范围；Lead 批准；真实环境待验证 |
| `Cargo.toml`/`Cargo.lock` | unix `nix` target dependency | 仅进程组信号；无其它依赖漂移 |

## 6. 条件关闭记录（**已关闭**，`3bbe4b5`）

- [x] **C1** `external_request_record` + `record_external_call` 已实现；`external_call_records_exactly_one_llm_call_row` 断言每节点恰 1 条、request/response 不含事件全文。
- [x] **C2** `read_frames` 改 `fill_buf`/`consume` 分块、1 MiB 行缓冲硬上限；`no_newline_giant_line_is_bounded_and_rejected`（2 MiB 无换行、<5s、adapter 回收）通过。
- [x] **C3** Codex SIGTERM/SIGINT handler + 父 `shutdown` cancel 500ms 转发窗口；`codex_in_flight_cancel_leaves_no_orphan_process_group` 通过。
- 首轮 P1 条件已全部关闭；残余 P2/P3 见 §8，V5 清单见 §7。

## 7. 遗留问题

- 真实 Codex/DSH 环境验证、V5 e2e AC-20 a–h 真链路、8 MiB/总输出/in-flight cancel 测试按 Lead 预裁决 7 留 V5；release notes 标注 adapter 骨架。
- P2 残余：N6 外部仍强制 role_prompt、N8 L2 不 spawn/CLI 全出口 e2e（按预裁决留 V5）；N1–N5/N7 已于 `3bbe4b5` 修复；P3 N9–N14 文档/台账。

## 8. 条件关闭复核（2026-11-02，`3bbe4b5`，parent `56ffdce`）

**复核结论：通过**（保留首轮 `56ffdce` 有条件通过记录；条件关闭后转通过）。

### C1（`llm_calls`）
- `plugin.rs` 新增 `external_request_record`（provider/agent_id/session_key/BLAKE3 inputs_hash 64 hex）+ `record_external_call`：provider.run 成功/失败/`adapter_for` spawn 失败均恰 1 条；取消/实例超时不写；成功 response=四字段+event_count，失败=error{code,reason}+event_count。
- `external_call_records_exactly_one_llm_call_row` 通过：成功/失败各 1 条；request 无事件文本；response 无 `"events"` 原始 JSONL。
- 残余 P3：spawn 失败分支无独立 recorder 测试（代码路径已覆盖）。

### C2（行硬上限）
- `read_frames` 改 `fill_buf`/`consume` 分块；超过 1 MiB 行**未缓存完整巨行**即 `line_too_large`；保留 8 MiB 总输出上限。
- `no_newline_giant_line_is_bounded_and_rejected` 通过（2 MiB 无换行、<5s、adapter 回收）；`total_output_limit_is_enforced` 通过。

### C3（取消无孤儿）
- `codex.py` 安装 SIGTERM/SIGINT handler（`killpg` Codex 进程组）；父 `Adapter::shutdown` 改为 cancel 写 500ms 超时 → 关 stdin → 2s 转发窗口 → SIGTERM → 1s → SIGKILL → reap。
- `codex_in_flight_cancel_leaves_no_orphan_process_group` 通过（fake 长睡 Codex、600ms cancel、`kill(pid,0)` 轮询无残留）。
- 残余 P2：复核首次全量 harness 运行时该测试曾等待 >60s（`pid_file` 无超时等待；并行负载下 cancel 可能早于 fake Codex 写 pid）；隔离运行与随后全量重跑均通过。建议该测试给 pid 等待加上限/失败重试，避免 CI 偶发挂起（不改变 C3 实现结论）。

### 同批 P2 复核
- **N1** `plan`→`Progress`、`usage_update` 折入 result usage：`plan_maps_to_progress_and_usage_update_folds_into_result` 通过；原 `unknown_event` 测试改为 `mystery_event`。
- **N2/N5** stderr head+tail 4 KiB：Rust `stderr_head_and_tail_are_recorded_within_limit` + Python `test_stderr_capture_keeps_head_and_tail_bounded` 通过；Codex stderr 后台排空。
- **N3** hello limits 真实 node timeout / total 8 MiB / line 1 MiB；`validate_hello`/`invalid_hello_capabilities_fail_closed` 通过。
- **N4** cancel 写 500ms 超时；父 shutdown 转发窗口。
- **N7** builtin golden 加强：run_output / instance snapshots / transitions / streaming events 对照。
- **N8** 8 MiB 总输出测试补齐；L2/CLI 全出口 e2e 仍按 V5。
- **N6/N8 残余** role_prompt 外部强制、L2 不 spawn/CLI 全出口真实 e2e 留 V5；N9/N10/N11/N12/N13/N14 为 P3/文档。

### 复跑门禁（`3bbe4b5`）
```bash
cargo test -p zgraph-harness                  # 234 tests：233 passed / 0 failed / 1 ignored
cargo test -p zgraph-harness --features mcp   # 233 tests：232 passed / 0 failed / 1 ignored
cargo test --workspace                        # 72 targets：1003 passed / 0 failed / 3 ignored
pytest -q                                     # exit 0；183 passed
cargo clippy --workspace --all-targets --locked -- -D warnings  # clean
cargo clippy -p zgraph-harness --all-targets --features mcp --locked -- -D warnings  # clean
cargo fmt --all -- --check                    # exit 0
cargo check -p zgraph-harness --no-default-features  # clean
bash scripts/dev/check.sh                     # PASSED
```
- 工作区干净；范围 8 files（+841/−151），无既有评审记录/代码逻辑回退；`git status` 空。
- 与自报一致：harness 233/1、features 232/1、workspace 1003/0/3、pytest 183；全绿。

## 7. 第二评审 · 跨模块（cli/kernel/py-worker/Cargo）（Lead 追加，2026-11-02）

- **评审人**：`lead`（**≠ 作者** `dsl-dev`；Lead 已批准跨模块改动，按 AGENTS §2 追加第二评审）
- **范围**（`f204368..3ecb5ab`）：
  1. `crates/zgraph-cli/src/run_cmd.rs`：`close_provider()` 在 run 成功/失败出口接线（与 `close_mcp` 并列）；无新 CLI 参数/退出码变化。
  2. `crates/zgraph-kernel/src/types.rs`：append-only —— `CODE_HARNESS_PLUGIN_ERROR`、`HarnessErrorKind::Plugin` variant、`code()` arm + 单测；既有码/枚举/行为未改。
  3. `python/zgraph_worker/adapters/{__init__,codex,dsh_acp}.py` + `python/tests/test_adapters_{codex,dsh_acp}.py`：py-worker 新模块（Codex adapter 先行、DSH ACP 骨架），无既有 worker 语义改动。
  4. `crates/zgraph-harness/Cargo.toml` + `Cargo.lock`：新增 `blake3`（inputs_hash）与 unix `nix`（进程组/信号）目标依赖；feature 边界未改。
- **核对结论（通过）**：cli 为机械接线；kernel append-only；py-worker 为新增 adapter 文件（不改现有 worker runtime）；Cargo 增量为已声明的哈希/进程组依赖、无版本漂移；无 docs/e2e/密钥改动；默认无 harness 配置路径逐字节不变（golden）。
- **Lead 亲自验证**：`git diff f204368..3ecb5ab --stat` 仅上述 19+2 files；`cargo test --workspace --offline` = 1003 passed / 0 failed / 3 ignored；`cargo test -p zgraph-harness --offline` 233/1 ignored；`pytest python` 183 passed。
- **裁决**：真实 Codex/DSH 环境未验证 → 按 DR-011 §9 在 release notes 标注「adapter 骨架，待真实环境验证」；V5 e2e 补 L2 不 spawn/CLI 全出口/AC-18/19/20 真链路；P3 留 V5 docs/台账。
- **结论**：**通过（第二评审）**。分支满足合并前置。
