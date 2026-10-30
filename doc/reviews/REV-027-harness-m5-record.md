# REV-027 · harness · M5 LlmCallRecorder seam + ISSUE-074 合法 JSON 截断 + ISSUE-081 超长函数拆分

> **模块声明**：本分支只改 `crates/zgraph-harness/**`（10 个文件），符合 AGENTS §2 `harness` 模块边界；**无** `Cargo.toml`/`Cargo.lock`（无新依赖）、**无** kernel/worker/trace/cli/e2e/`doc/**` 改动。作者 `harness-dev`，评审人 `reviewer-b`（**非作者**）。评审过程未修改任何产品代码/测试/台账，本次提交仅新增本评审记录；未 merge/push/rebase。
>
> **评审对象变更史**：初版 HEAD `9a7de79`（基于 `1cc9123`）→ 作者 rebase 到本地 main `5d2e53d` 得 `a7c216b`（内容零差异）→ 评审发现提交正文缺 AGENTS §1 强制字段 `Review:`/`Agent:`，作者 **amend 仅改提交消息**得最终 HEAD **`9137fd6`**。已核 `git diff a7c216b 9137fd6` **0 行**、subject 不变、trailers 两行命中；`git diff 9a7de79 a7c216b -- crates/zgraph-harness` 亦为空。本节结论以 **`9137fd6`** 为准；元数据阻塞项已在评审期内关闭（详见 §4.1/§5）。
>
> **独立核对口径**：不采信作者自述。亲自读 `main...HEAD` 全量 diff（10 文件 +1669/−336）与 `record.rs` 全文、逐段对照 ISSUE-081 拆分前后逻辑、核 `LlmRequest`/`SecretString`/`LlmResponse` 类型与 transport 错误构造、复算截断边界；在 **隔离 target（`/tmp/zgraph-merge-target`）** 实跑 harness/e2e/workspace/clippy/fmt/check.sh；另用 **仓库外临时树（`git archive` 副本 + 追加 `#[cfg(test)]` probe 模块，未改仓库）** 对私有截断函数做独立边界/极端键/随机 fuzz 验证。环境提示：`/tmp/zgraph-merge-target` 中旧构建产物带有已删除 worktree 的 `CARGO_MANIFEST_DIR`，首次 workspace 全量出现 3 个 `zgraph-cli` acceptance_m2 假失败（`repo root resolves: NotFound`）；按 Lead 指引 `cargo clean -p zgraph-dsl -p zgraph-kernel -p zgraph-trace -p zgraph-worker -p zgraph-harness -p zgraph-cli -p zgraph-e2e` 后复跑全绿，非产品问题。

- **分支**：`feat/harness-m5-record`
- **作者**：`harness-dev`（提交 `Agent:` 字段）
- **评审人**：`reviewer-b`
- **变更范围**：`5d2e53d21d8693c738213f0764376f414826c5c3..9137fd653e2f8aef4248b846c1d60b37bd224034`（1 commit `9137fd6 feat(harness): 记录 seam + 合法 JSON 截断 + 拆分超长函数（REV-027）`；`git diff --shortstat main...HEAD` = **10 files changed, 1669 insertions(+), 336 deletions(-)**；base = merge-base = `main` = `5d2e53d`，ahead 1，已 rebase）
- **评审时间**：2026-10-30
- **关联**：**ISSUE-082**（L2 `llm_calls` 记录接线，harness 侧 seam）、**ISSUE-074**（工具结果截断后合法 JSON + traceback 8 KiB + summary 严格上界）、**ISSUE-081**（5 个生产函数 >60 行）；DR-002 §3.2（`llm_calls` 写者/粒度/脱敏/status）、§4（ISSUE-074/081 修复口径）；spec 05 §5.6.5（工具结果 8 KiB/截断/traceback）、§5.13.3 L4（M4 截断已知限制）、§5.3.2/§5.3.5/§5.7（LLM 类型/重试/错误映射）；architecture-v2 §2.2（函数 ≤60 行等价约束）、§5.2/§8 D9 L2；REV-020（harness M4 基线，含 #3/#4/#11 遗留）、REV-022（AC-08 e2e 基线）

## 1. 结论

**结论：通过**

本分支把 M5 的 harness 侧三件事落地，实现质量高、测试真实、对既有行为零破坏：

- **LlmCallRecorder seam（ISSUE-082 前置）**：新增独立模块 `record.rs`（`pub trait LlmCallRecorder` + 只读快照 `LlmCallSnapshot`）与**只增不破**的 `HarnessExecutor::with_llm_recorder`；`HarnessSettings` 字段集合与 `HarnessExecutor::new` 保持不变（e2e 结构体字面量构造不破），H3「Harness 不写 SQLite」保持（本 crate 无 sqlite 引用）。
  - **每轮恰一次**：`invoke_with_retry` 只在**终态**记录——成功 1 次（`attempts` 已填）、取消/实例超时/终态 Harness 失败各 1 次；重试中间失败不记录（`:395-445`）。
  - **脱敏**：`request_snapshot` 只取 `model{provider,name,base_url}/messages/tools/temperature/max_tokens/stream/timeout_ms`，**不读取** `LlmRequest.api_key`（`SecretString` 不入 JSON、字段名亦不出现）；单测用 26 字符测试密钥断言序列化文本不含明文、也不含 `api_key`/`SecretString`（`record.rs:244-279`）。
  - **观察者不改变执行**：`record_safely` 用 `catch_unwind(AssertUnwindSafe(..))` 吞记录器 panic；单测构造 `panicking` 记录器，断言运行仍 Completed、content 与 token 流不变（`agent_loop.rs:1594-1607`），另有真 kernel `start_run` 级用例断言 `instance_id` 来自 `NodeContext`（`executor.rs:506-544`）。
- **ISSUE-074 结构感知截断**：工具结果超 8 KiB 时按「字符串叶子按字符边界截断 + 标记 → 对象/数组保留前缀降级 + `_truncated` 标记 → 极端兜底信封」收敛，**输出恒为可 `serde_json::from_str` 的合法 JSON 且 ≤8192 B**（`tools.rs:393-438`）；traceback 上限由 1 MiB 统一为 8 KiB 且不切 UTF-8 码点（`:242-248`、`:661-680`）；`last_content_summary`/`preview` 修正 ±1 为**严格** ≤512/≤256 字符（`error.rs:341-352`、`tools.rs:648-659`）。
- **ISSUE-081 拆分**：5 个目标函数实测 **run_agent 21 / parse_model 31 / parse_discover_output 30 / parse_call 29 / map_webc_error 59**（另 `invoke_with_retry` 94→50），均 ≤60；逐段对照确认**纯重构**（校验顺序、错误分支、返回值一致），公共 API 不变。
- **验证**（隔离 target）：`cargo test -p zgraph-harness` **121 passed / 0 failed / 1 ignored**（M4 基线 106/1，+15 新测试全过）；`cargo test -p zgraph-e2e` **24 passed / 0 failed / 1 ignored**（`harness_ac08` **4 passed/1 ignored 不回归**）；`cargo test --workspace` **439 passed / 0 failed / 2 ignored / 52 个测试二进制**（main 424/2 + 15，无新增失败、无缩水）；`cargo clippy --workspace --all-targets -- -D warnings` exit 0；`cargo fmt --all -- --check` exit 0；`check.sh` PASS；worktree clean。

无阻塞/重要缺陷残留（评审期发现的提交元数据阻塞已由作者 amend 关闭）；5 条建议/备注见 §3（测试函数超长、既有 >60 函数清单、重试退避期间 deadline/cancel 的 `llm_calls` 完整性边界、>128 层嵌套理论边界、`noop_sink_is_send` 测试噪声），均**不阻塞合并**，其中 #1–#4 建议由 Lead/作者排期或 docs 承接。

## 2. 四维评审

| 维度 | 结论 | 依据（文件/行/测试/命令） |
|---|---|---|
| 功能正确性 | ✅ | ① **seam 契约**：`LlmCallSnapshot{instance_id,call_index,request,response,status}`（`record.rs:36-48`）；`call_index = iteration-1`（0 起、重试不递增，`:405`）；成功快照含 `content/reasoning/tool_calls/stop_reason/usage(source)/latency_ms/attempts/provider_request_id`（`:127-152`），失败快照 `{"error":{kind,message,phase}}`（`:154-166`）与 DR-002 §3.2 一致；`status` 常量 `ok`/`error`。② **每轮恰一次**单测：两轮 tool loop → 恰 2 条 `call_index 0/1` 且第二轮请求快照含 system/user/assistant/tool 4 条消息（`agent_loop.rs:1487-1561`）；重试耗尽 → 恰 1 条 error（`:1563-1592`）。③ **脱敏**：快照不含 `api_key` 字段/明文（`record.rs:271-279`、`agent_loop.rs:1531-1546`）；`LlmError.message` 由 transport 构造（HTTP 状态/截断 body/reqwest Display，`transport.rs:466-551`），其中不含鉴权头/密钥；`SecretString` 仅 `expose()` 于 adapter 鉴权注入（`:216`）且 Debug 为 REDACTED（原 M4 测试保留）。④ **观察者不改语义**：panic 吞掉（`record.rs:50-55`），用例断言 Completed/content/token 不变；未挂记录器时快照不构造（`:516`），执行路径与 M4 等价（全量旧测试通过）。⑤ **ISSUE-074**：结构感知截断单测覆盖 ASCII/CJK/对象/嵌套数组/error 内容/traceback（`tools.rs:1663-1850`）+ 我独立 probe（见 §4.4）；traceback 8 KiB 不切码点、summary 严格上界均实测。⑥ **ISSUE-081**：拆分函数行数实测达标；`parse_model`/`parse_discover_output`/`parse_call`/`map_webc_error` 逐段 diff 确认校验顺序与错误分支不变，全部既有单测（121）通过。 |
| 代码结构合理性 | ✅ | `record.rs` 是独立 H3 seam：不依赖 kernel/trace/sqlite，只消费 `LlmRequest`/`LlmResponse`/`LlmError` 并产出 `Value` 快照；`HarnessExecutor` 仅新增私有 `recorder: Option<Arc<dyn LlmCallRecorder>>` 字段与 additive 构造器，`HarnessSettings` 冻结面不变（`executor.rs:54-88`）；`agent_loop` 把 139 行 `run_agent` 拆为 `LoopState`（状态）+ `check_iteration_limits/begin_iteration/handle_response/finish_without_tools/run_tool_calls` 小函数，`invoke_with_retry` 拆出 `preflight_call_timeout/build_llm_request/classify_error_action/invoke_attempt` + `AttemptResult/ErrorAction`，`tools` 拆出 `serialize_json_limited` 一族与 discover 解析子函数，`config`/`fake`/`transport` 均为局部 helper 提取；无跨模块漂移、无新依赖、无公共 API 删除；`lib.rs` 只追加 `record` 导出与文档（`:77`）。 |
| 逻辑性 | ✅ | 记录点只在终态（成功/取消/超时/失败），重试中间不落记录；快照在有记录器时才构造（无额外序列化开销）；`catch_unwind` 防止观察者 panic 破坏执行；`Invoke` 预检顺序（cancel→deadline→timeout<1000ms）与 M4 完全一致，重试/退避/取消时序未变；截断循环有 `MAX_SHRINK_STEPS=4096` 上限 + 每步至少减少 1 字节/1/8 元素，收敛性可证，且 `current_len <= excess+64` 守卫避免 usize 下溢；字符串内截断保证不切 UTF-8 码点、结构降级保留前缀并留唯一标记；失败路径不吞错误（panic 仅限 recorder 观察者，工具失败仍为 `TOOL_ERROR` 喂回）。全量 439 测试（含 e2e 真链路）无 flaky/回归。 |
| 可读性 | ✅ | `record.rs` 模块文档列出纪律/脱敏/重试语义（`:1-12`），各快照构造有「为什么」注释（H3、05 §5.3.2、Issue 引用）；`tools.rs` 截断策略以编号列表说明不变量；拆分后的每个 helper 有 docstring 与 05/ISSUE 溯源；常量集中在 `constants.rs`；命名 snake_case、类型注解齐全；`#![forbid(unsafe_code)]` 与 `#![warn(missing_docs)]` 保持，clippy `-D warnings` 0 告警。扣分项仅为 3 条 Nit（§3 #1/#2/#5）。 |

## 3. 发现清单

| # | 级别 | 位置 | 问题 | 建议 | 状态 |
|---|---|---|---|---|---|
| 1 | 建议（测试长度） | `crates/zgraph-harness/src/agent_loop.rs:1487-1561`（`recorder_captures_one_call_per_iteration_with_desensitized_request`） | 本分支新增的该测试函数 **74 行 >60**（其余新增测试均短），与本分支「函数 ≤60 行」的整改目标不一致；断言内容本身真实有效（两轮记录、字段、脱敏、第二轮消息形状） | 拆成 2 个测试（记录粒度/脱敏 + 请求快照字段）或抽公共 `assert_snapshot_shape` helper；**不阻塞**（测试代码，且是本分支唯一超限） | open（建议 wave2/下次触碰修） |
| 2 | 建议（既有 >60 清单补全） | `config.rs:269`（`parse_agent_config` 113 行）、`tools.rs:863`（`validate_schema_node` 105）、`llm/transport.rs:70`（`chat_stream` 86）、`llm/fake.rs:174`（`chat_stream` 75）、`executor.rs:110`（`execute` 71）、`config.rs:534`（`parse_base_url` 64）、`agent_loop.rs:626`（`execute_tool` 63）、`tools.rs:1974`（test `worker_tool_runner_roundtrip_with_real_python` 85） | ISSUE-081 只列了 5 个函数；本分支已把 5 个（+`invoke_with_retry`）降到 ≤60，但全仓扫描仍有 **8 个既有函数 >60 行**（主分支同样超限，**非本分支引入/回归**）。ISSUE-081 的清单不完整，读者可能误以为已全部收敛 | 在 `doc/issues.md` 的 ISSUE-081 补全上述清单（或另开一条「harness 其余超长函数」），标注本分支已完成 6 个、剩余 8 个随 M5/M6；**不阻塞** | open（台账承接，Lead/docs 登记） |
| 3 | 建议（记录完整性边界） | `crates/zgraph-harness/src/agent_loop.rs:395-445`（`invoke_with_retry`）、`:448-464`（`preflight_call_timeout`） | 若第 1 次尝试已经**真实发出请求并失败**（retryable）、随后在退避等待后进入下一轮 `preflight_call_timeout` 时 deadline 已耗尽（或 cancel），函数直接返回 `InstanceTimeout/Cancelled`，**该轮不产生任何 `llm_calls` 记录**；注释「调用前的取消/deadline 预检不发请求，因此不产生记录」只覆盖首次预检，不覆盖「已有一次真实失败尝试」的场景。影响：`llm_calls` 审计在 agent 超时/取消边界会少记一条（不影响 L2 重放，run 的 HarnessError detail 仍有 attempts） | 二选一：① 在 preflight 失败且 `attempt >= 1` 时用上一次 `error` 记录一条 `status=error`（response 可加 `phase/kind`），保持「一次调用终态一次记录」；② 在 DR-002 §3.2 明确「预检失败的轮次不写 `llm_calls`」的例外并回写 spec。**不阻塞**（边界低概率、仅审计完整性） | open（建议 wave2 CLI 落库时明确） |
| 4 | 建议/Nit（文档边界） | `crates/zgraph-harness/src/tools.rs:393-405`（`serialize_json_limited` 文档不变量「返回值必定可 `serde_json::from_str`」） | 我在临时树 probe 发现：直接构造 **>128 层嵌套** `Value` 时，输出尺寸仍 ≤8 KiB 且序列化合法，但 `serde_json::from_str` 因默认递归上限 128 失败——即该不变量对深度无界不成立。**生产不可达**：worker 协议帧解析本身受 serde_json 默认递归限制，工具结果 `Value` 深度 ≤128；属理论边界 | 在 docstring 补一句「深度以生产链路可解析范围为限（serde_json 默认递归上限），嵌套 >128 层的输入在 worker 协议解析阶段即被拒」，或统一在 `worker` 侧显式限制/记录；**不阻塞** | open（文档 Nit） |
| 5 | Nit（测试噪声） | `crates/zgraph-harness/src/record.rs:336-347`（`NoopSink` + `noop_sink_is_send`） | 该测试只为消除测试模块里 `LlmDelta`/`LlmStreamSink` 的未使用导入而存在（记录模块不涉及流式），断言是编译期 `Send`，无契约价值；有「为测试而测试」倾向 | 删除 `NoopSink`/`noop_sink_is_send` 与两个未使用导入（或仅保留需要的导入）；**不阻塞** | open（建议随手清理） |
| 6 | 备注（口径） | `crates/zgraph-harness/src/record.rs:154-166`（`error_response_snapshot`） | 成功快照含 `attempts`，失败快照仅 `{error:{kind,message,phase}}` **不含 attempts**；DR-002 §3.2「重试合并为最终结果（响应里 attempts 体现）」对失败路径存在口径模糊（spec/06 表无 attempts 列，HarnessError detail 有 attempts） | 若 L2 审计需要重试可见性，可在 error response 追加 `attempts`（兼容追加）；若维持现状，建议 docs 在 05/06 或 DR 明确「attempts 仅在成功响应/HarnessError detail」。**不阻塞** | open（docs/口径承接） |

> 无阻塞项、无重要项（评审期发现并已关闭的提交元数据阻塞见 §4.1，不计为遗留）。#1–#3 为测试/审计完整性建议，#4–#5 为文档/测试 Nit，#6 为口径备注；**本分支无需新开 P1/P2 ISSUE**，建议由 Lead 在 M5 收尾把 #2 补进 ISSUE-081、#3/#6 纳入 DR-002 回写。

## 4. 验证证据

### 4.1 范围、分支、提交元数据与「零内容改动」核对

```bash
cd /Users/zyzs/lczyzs/code/zgraph/.worktrees/harness-harness-dev
git branch --show-current        # feat/harness-m5-record
git rev-parse HEAD               # 9137fd653e2f8aef4248b846c1d60b37bd224034
git log --oneline -2             # 9137fd6 → 5d2e53d(main)
git merge-base main HEAD         # 5d2e53d...（= main，ahead 1）
git diff --shortstat main...HEAD # 10 files changed, 1669 insertions(+), 336 deletions(-)
git diff --name-status main...HEAD
# M agent_loop.rs/config.rs/error.rs/executor.rs/lib.rs/llm/fake.rs/llm/transport.rs/test_support.rs/tools.rs；A record.rs
git diff --stat a7c216b 9137fd6     # 空
git diff a7c216b 9137fd6 | wc -l    # 0
git diff 9a7de79 a7c216b -- crates/zgraph-harness | wc -l   # 0（rebase 前后内容一致）
git log -1 --format='%B' | grep -E '^(Review: REV-027|Agent: harness-dev)$'
# Review: REV-027 / Agent: harness-dev（两行命中；原 a7c216b 缺此两行，作者 amend 消息修复）
git status --short               # 空（clean）
git ls-tree -r --name-only HEAD | grep -E "__pycache__|\.pyc$|\.db$|\.sqlite" || echo none   # none
git diff --check                 # exit 0
```

- 未改 `Cargo.toml`/`Cargo.lock`（无新依赖）、未改 kernel/worker/trace/cli/e2e/`doc/**`、未改 `CLAUDE.md`/`scripts/**`。
- **元数据阻塞项闭合记录**：初评对象 `a7c216b` 正文缺 `Review: REV-027`/`Agent: harness-dev`（AGENTS §1 MUST）；作者按评审要求 `git commit --amend` 仅改消息得 `9137fd6`，已核内容 diff 为 0、subject 不变、trailers 命中，故该阻塞**已关闭**，不进入遗留。

### 4.2 测试与门禁（隔离 target `/tmp/zgraph-merge-target`；`CARGO_HOME=<repo>/.cargo-home`、`CARGO_NET_OFFLINE=true`、`CARGO_INCREMENTAL=0`、`ZGRAPH_PYTHON=<repo>/.venv/bin/python`）

| 命令 | 结果 |
|---|---|
| `cargo clean -p zgraph-dsl -p zgraph-kernel -p zgraph-trace -p zgraph-worker -p zgraph-harness -p zgraph-cli -p zgraph-e2e`（清理陈旧 `CARGO_MANIFEST_DIR` 产物） | exit 0（移除 1035 files / 937.1 MiB） |
| `cargo test -p zgraph-harness` | **exit 0：121 passed / 0 failed / 1 ignored**（ignored = `llm::transport::tests::real_deepseek_smoke` 真实模型；M4 基线 106/1，**+15 新测试**：record 4、agent_loop 3、executor 1、tools 7；日志 `/tmp/rev027-harness.log`、`/tmp/rev027-ws2.log`） |
| `cargo test -p zgraph-e2e` | **exit 0：24 passed / 0 failed / 1 ignored**（7 个 test binary；`harness_ac08` **4 passed/1 ignored 不回归**、ac12 1、concurrency 4、cancel 2、fault_injection 7、parallel 4、timeout_kill 2；日志 `/tmp/rev027-e2e-final.log`） |
| `cargo test --workspace`（52 个测试二进制） | **exit 0：439 passed / 0 failed / 2 ignored**（main 基线 424/2 + 15，无新增失败、无基线缩水；日志 `/tmp/rev027-ws2.log`） |
| `cargo clippy --workspace --all-targets -- -D warnings` | **exit 0 / 0 warning**（含 zgraph-harness/zgraph-e2e 重新检查；`/tmp/rev027-clippy.log`；CI 口径不回归） |
| `cargo fmt --all -- --check` | **exit 0** |
| `bash scripts/dev/check.sh` | **CHECK PASSED**（身份/remote/CLAUDE.md/分支名/Python 语法/progress/issues/fmt/`git diff --check`） |
| `git status --short`（全部命令后） | 空 |

> 说明：首次未清理直接跑 workspace 时，`zgraph-cli` 的 `acceptance_m2` 3 例因**陈旧 target 产物把 `CARGO_MANIFEST_DIR` 指向已删除的 worktree** 而报 `repo root resolves: NotFound`（非本分支代码问题）；按 Lead 指引用 `cargo clean -p ...` 后全绿，上表为清理后结果。

### 4.3 LlmCallRecorder 语义/脱敏/观察者不改变执行（读码 + 定向单测）

| 核对项 | 结果（代码/测试） |
|---|---|
| 每轮恰一次、call_index | `invoke_with_retry`：成功→`record_call_ok` 一次后返回；Cancelled/Timeout/Fail 各 `record_call_error` 一次；Retry 分支不记录（`agent_loop.rs:395-445`）；`call_index = iteration.saturating_sub(1)`（`:405`）。测试两轮 tool loop 恰 2 条（`:1487-1561`） |
| 重试只记最终结果 | 失败脚本 2 次 500 → 恰 1 条 `status=error`、`call_index=0`、response.error kind/phase/message 正确（`:1563-1592`）；成功路径 `response.attempts` 在记录前写入（`:413-416`） |
| 请求快照字段/脱敏 | `request_snapshot` 含 model{provider,name,base_url}/messages（system/user/assistant+tool_calls/tool）/tools{name,description,parameters}/temperature/max_tokens/stream/timeout_ms，**不读 api_key**（`record.rs:57-125`）；测试序列化文本不含测试密钥明文、`api_key`、`SecretString`（`:244-279`） |
| 响应快照 | 成功：content/reasoning/tool_calls/stop_reason/usage{prompt,completion,total,source}/latency_ms/attempts/provider_request_id（`record.rs:127-152`）；失败：`{error:{kind,message,phase}}`（`:154-166`） |
| 记录器 panic 不传播 | `record_safely` = `catch_unwind(AssertUnwindSafe(..))`（`:50-55`）；用例：panicking recorder → run Completed、content/token 不变、recorder 捕获 0 条（`agent_loop.rs:1594-1607`） |
| 真 kernel instance_id | `HarnessExecutor::with_llm_recorder` + 真 `start_run` → 记录 `instance_id` 非空、call_index 0、status ok、请求/响应形状正确、请求文本不含密钥（`executor.rs:506-544`） |
| H3 不写 SQLite / 冻结面 | harness 源码无 sqlite/trace 写端；`HarnessSettings` 字段集合未变、`HarnessExecutor::new` 保留（`executor.rs:63-88`）；`lib.rs` 仅追加导出 |
| 无记录器路径等价 | 未挂 recorder 时 `snapshot=None`、`record_call_*` 早退（`:516`、`:546-548`、`:569-571`）；M4 全部既有测试通过（121 中含全部旧测试） |

### 4.4 ISSUE-074 截断独立复算（仓库外临时树 probe，未改仓库）

以 `git archive 9137fd6` 生成 `/tmp/rev027-tree`，仅在该副本 `tools.rs` 末尾追加 `#[cfg(test)] mod rev027_probe`（直接调用 crate 私有 `serialize_tool_result`），`CARGO_TARGET_DIR=/tmp/rev027-probe-target` 运行；**仓库工作树零改动**（`git status` clean）。断言「输出可 `serde_json::from_str` 且字节 ≤8192」，覆盖：

| 边界/形态 | 结果 |
|---|---|
| 恰好 8192 B（`{"s":"x"*(8192-overhead)}`） | 原样返回、可解析、长度 =8192 |
| 8193 B | 截断后 ≤8192、合法 JSON、标记存在 |
| ASCII 20 000 / CJK 8 000 / 4 字节 emoji 3 000 字符 | 合法 JSON、≤8192、保留前缀、不切码点（emoji 亦通过） |
| 极端键：20 000 字符单键 / 单键+30 000 字符值 / 500 个 20 000+ 字符键 | 合法 JSON、≤8192、降级为 `_truncated` 标记信封（符合 docstring 描述） |
| 结构降级：2 000 小属性对象 / 5 000 元素数组 / 2 000 字符串数组 / 100 层嵌套 | 合法 JSON、≤8192、保留前缀 |
| 转义密集：控制字符 `\u0000`/`\u001f`/`\u007f`、引号、反斜杠、换行/Tab、emoji、超长键 | 合法 JSON、≤8192 |
| 纯数字 100 000 元素数组 / 单键 50 000 + 1 万组嵌套数组 | 合法 JSON、≤8192（结构降级路径） |
| 随机 fuzz 400 例（混合键长/值类型/嵌套/unicode，对象 1–12 键） | 全部合法 JSON 且 ≤8192 |
| traceback 20 000 CJK | `ToolFailure.to_json()["traceback"]` ≤8192 B 且全为完整码点 |
| `summarize_args`/`summarize_result` preview | 严格 ≤256 字符（长输入） |

- 该 probe 另发现 **>128 层嵌套**输入时输出虽 ≤8 KiB 但 `serde_json::from_str` 受默认递归上限拒绝（生产经 worker 协议解析不可达）→ 已列为 §3 #4 文档 Nit。
- 单测层同样覆盖：`result_serialization_requires_object_and_truncates_on_char_boundary`、`oversized_ascii_string_...`、`oversized_utf8_string_...`、`oversized_object_degrades_...`、`oversized_nested_array_...`、`tool_error_content_respects_byte_limit_...`、`long_traceback_is_capped_at_eight_kib_...`、`truncate_chars_is_strict_upper_bound`（`tools.rs:1663-1850`）。

### 4.5 ISSUE-081 拆分与行为等价（逐段 diff + 行数实测）

| 函数 | 拆分前 | 拆分后 | 等价性核对 |
|---|---|---|---|
| `run_agent` | 144 行（`main` 实测；作者自述 139） | **21** | 拆为 `LoopState::{new,check_iteration_limits,begin_iteration,handle_response,finish_without_tools,run_tool_calls}`；顺序：max_iterations→cancel→deadline→iteration+1→Progress→validate→预算复检→invoke→usage 累计→停止/工具，与旧实现逐语句一致（含空响应连续 2 次、unknown/未授权工具在 ToolCallStart 前失败） |
| `invoke_with_retry` | 94（本分支之前 `main` 亦 >60） | **50** | 拆 `preflight_call_timeout`/`build_llm_request`/`classify_error_action`/`invoke_attempt` + `ErrorAction`；cancel/deadline 优先于 provider 分类、`retry-after` 上限、剩余 deadline 小于退避则不重试、Harness 侧 timeout phase 判定（`sink.started()`）均保持 |
| `parse_model` | 124 | **31** | 子函数 `parse_provider/model_name/context_window/max_tokens/base_url/api_key_env/temperature`，调用顺序与 V5–V11（V11 先于 V10）一致；`reject_unknown` 在其前 |
| `parse_discover_output` | 75 | **30** | 拆 `decode_discover_error` + `parse_tool_declaration`；ok=false 的 reason/message/actual（去 traceback）与逐条声明解析顺序不变 |
| `parse_call` | 66 | **29** | 拆 `parse_call_content/stop_reason/reasoning/usage`；错误优先级（content 类型→kind 合法性→text 必填→stop_reason→reasoning）不变 |
| `map_webc_error` | 64 | **59** | 抽出 `classify_reqwest(is_timeout,is_connect,status,phase)`，分类顺序（timeout→connect→status→streaming→connect）与错误消息文本不变 |

- 行数为「fn 起始行到同缩进 `}`」实测；公共 API 面未变（`main...HEAD` diff 无 `pub fn` 删除/签名修改，`lib.rs` 仅追加）。
- 行为等价另有整体证据：M4 全部既有 harness 单测（106）在本分支全部通过；e2e `harness_ac08`（真实 kernel+tool worker+trace）4 passed/1 ignored；workspace 439 passed。
- 全仓 >60 行扫描（改后）：本分支新增/修改文件中除 `parse_agent_config` 113、`validate_schema_node` 105、`chat_stream` 86/75、`execute` 71、`parse_base_url` 64、`execute_tool` 63、test `worker_tool_runner_roundtrip_with_real_python` 85、新增 test 74 外无其他；上述既有项均在 main 同样超限（非本分支引入）→ §3 #2。

### 4.6 静态/边界核对

| 项 | 结果 |
|---|---|
| 新增依赖/跨模块 | `main...HEAD` 无 `Cargo.toml`/`Cargo.lock`/其它 crate/`doc/**`/`tests/**` 改动（§4.1 name-status） |
| mock / 路径 / 禁用文件 | 无 mock（新代码为纯函数 + seam）；未新增绝对路径（`record.rs` 无路径；`tools.rs` 新逻辑无路径）；HEAD tree 无 `.pyc`/`__pycache__`/`.db`/日志 |
| 不安全/panic | `#![forbid(unsafe_code)]` 保持；生产新代码无 `unwrap()/expect()/panic!`（`record.rs` 仅测试有 `expect`；`tools.rs:407` `unwrap_or_else` 兜底；clippy 已确认） |
| 恒真/被注释断言 | 新增测试均为实质断言；无 `assert!(true)`/被注释断言/`todo!`/`unimplemented!`（diff 扫描） |
| 函数长度 | 5 个目标函数 + `invoke_with_retry` 达标；新增测试函数 1 个 74 行（§3 #1） |
| H3 / 密钥 | harness 无 SQLite/写端；请求快照不读 `api_key`；`LlmError.message` 不含鉴权头；`SecretString` Debug REDACTED、仅 adapter `expose()` 注入 genai |
| 线程/并发 | recorder 为同步观察者，文档明确不得 await/IO；`Arc<dyn LlmCallRecorder + Send + Sync>`；无锁跨 await |

## 5. 遗留问题

- **ISSUE-082（部分关闭）**：本分支交付 harness 侧 seam（`LlmCallRecorder`/`with_llm_recorder`/快照/每轮恰一次/脱敏）；**尚未关闭**——CLI/trace 写端（实现 recorder 并写 `TraceInput::LlmCall`/`llm_calls` 表）属 wave2 CLI 分支（DR-002 §3.2/§6），ISSUE-082 待其接线后才能置 fixed。
- **ISSUE-074（harness 侧修复完成）**：结构感知截断/多字节/标记/traceback 8 KiB/summary 严格上界已实测；spec 05 §5.6.5 仍是 M4 旧措辞（「纯文本截断、M5 改结构感知」），需 docs M5 回写（DR-002 §4 已列），回写后关闭 ISSUE-074。
- **ISSUE-081（5 个函数关闭，清单待补）**：5 个目标函数 + `invoke_with_retry` 已 ≤60；建议把 §3 #2 的既有 >60 函数清单补入 ISSUE-081（或另开一条），避免问题台账失真。
- **建议项排期**：#1 新增 74 行测试函数拆分、#3 retry 退避期间 deadline/cancel 的 `llm_calls` 完整性口径（建议 wave2 CLI 落库时与 DR-002 §3.2 一起明确）、#4 `serialize_json_limited` 深度条件说明、#5 `noop_sink_is_send` 清理、#6 失败快照 attempts 口径。以上均不阻塞本分支合并。
- 评审过程未修改任何产品代码/测试/台账；本记录为本分支唯一新增文件（评审对象 `9137fd6` + 本记录提交）。

## 6. 条件关闭记录

不适用（结论为「通过」；评审期发现的提交元数据阻塞已由作者 amend 于 §4.1 关闭）。
