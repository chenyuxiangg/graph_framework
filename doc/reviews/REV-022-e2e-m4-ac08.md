# REV-022 · e2e · M4 AC-08 验收（fake LLM + tool loop + 流式/超时/异常/未知工具）

> **模块声明**：本分支只改 `tests/e2e/**` 与 `tests/fixtures/graphs/**`，符合 AGENTS §2 `e2e` 模块边界；唯一跨模块文件是 `Cargo.lock`（**process** 模块，+1 行，zgraph-e2e 新增 path dev-dependency `zgraph-harness` 的机械副作用；Lead 在指派中已知悉并要求核对无无关改动，task-6 亦明确允许 `tests/e2e/Cargo.toml` 加该 dev-dep）。作者 `e2e-dev`，评审人 `reviewer-b`（**非作者**）。评审**未修改任何代码/测试/台账**，只新增本记录；未 commit / 未 merge / 未 rebase。
>
> **独立核对口径**：本人亲自读 diff/全文、在**干净 `CARGO_TARGET_DIR`** 下跑 AC-08 专项（×3）、e2e 全包与 workspace 全量，另做静态扫描（mock/绝对路径/恒真断言/函数长度/pycache）与「工具超时后无残留 worker 进程」检查；不采信作者自述。

- **分支**：`test/e2e-m4-ac08`
- **作者**：`e2e-dev`
- **评审人**：`reviewer-b`
- **变更范围**：`344b73d..76bbe05`（1 commit `76bbe05 test(e2e): AC-08 fake LLM + tool 流式/超时/异常语义验收（REV-022）`；`git diff --shortstat` = **10 files changed, 811 insertions(+), 1 deletion(-)**；base = merge-base = `origin/main` = `344b73d`，**已 rebase**、ahead 1）
- **评审时间**：2026-10-30
- **关联**：spec 07 **AC-08**（真实 LLM + tool loop + 流式；fake 必过/真实手动）；spec 05 §5.3.4（fake 脚本）/§5.6.5/§5.6.6（工具执行与错误语义）/§5.8（流式 1:1 emit + 06 写线程聚合）/§5.9（输出四字段）；architecture-v2 §4/§6.1(M4)/§8 **D3**；task-6；REV-020（harness，已合入 main `344b73d`）、REV-019（py-worker tool_runner）

## 1. 结论

**结论：通过**

本分支把 AC-08 的 M4 口径落成**真链路**验收，覆盖完整、断言真实、稳定性好：

- **真链路无 mock**：`RoutingExecutor` 只按 `CompiledHandler::Agent` 路由到 `HarnessExecutor`、其余交给真 `WorkerExecutor`（`harness_ac08.rs:82-99`）；LLM 用 `FakeLlmProvider`（AC-08 fake 模式，属规定），但 kernel 调度、**工具执行 = 真 Python worker 子进程**（`WorkerToolRunner` + `common::worker_config`，每次调用一进程）、**trace = 真 SQLite**（`zgraph_cli::bridge::run_in_process` + `TraceReader::streaming_events`）全部真实；全文无 `FakeExecutor`/mock kernel/trace 路径。
- **AC-08a**：断言 run/实例 `Completed`、output 四字段（`content={"answer":"5"}`、`stop_reason=EndTurn`、`iterations=2`、`usage.total_tokens>0` 且 `=prompt+completion`、`source=provider`）；trace token 段按 `segment_index` 拼接 == content、`tool_call_start/end(add, ok=true, call_id 一致)`、progress 恰 2 条（`llm call 1/4`/`2/4`，percent 25/50）、无失败工具事件、lifecycle 形状；`FakeLlm.requests()` 校验 tools schema 顺序 `["add","explode","slow"]` 与第二轮 Tool 消息 `{"sum":5}`（证明真 Python 工具真的执行并回填）。
- **工具异常/超时**：`explode` → `tool_call_end.ok=false` + summary `error.reason=exception/type=ValueError/message=boom` + Tool 消息含 traceback，run 继续 `Completed`；`slow(2s)+声明 timeout_ms=300` → run 继续、整例 <15s、summary `reason=timeout/timeout_ms=300`，且**独立验证无残留 `zgraph_worker.runtime` 进程**（进程组 kill 生效）。
- **unknown tool**：run `Failed`、`run.error_code == instances.error_code == HARNESS_UNKNOWN_TOOL`、实例 `Crashed`、仅 1 轮请求（未知工具不喂回）。
- **AC-08b 真实模型**：`#[ignore]`，缺 `ZGRAPH_REAL_LLM`/`DEEPSEEK_API_KEY` 时打印 skip 并 pass（不联网）；提供手动执行命令。与 07 AC-08 的「真实模型手动」口径一致（细节见 §3 #1 的 docs 口径建议）。
- **测试真实性/稳定性**：`cargo test -p zgraph-e2e --test harness_ac08` 在干净 target 连跑 3 次均 **4 passed / 0 failed / 1 ignored**（0.37–0.44s）；e2e 全包 **23 passed / 0 failed / 1 ignored**；workspace 全量 **423 passed / 0 failed / 2 ignored**（无回归）；`check.sh` PASS；worktree clean。
- **规范**：新增测试 636 行但函数均 ≤60 行（AST/括号实测）；无绝对路径（fixtures 走 repo 相对路径）；无 `__pycache__`/`/tmp` 入库；`git status --ignored` 中的 pycache 为测试运行生成且被 `.gitignore` 忽略；`Cargo.lock` 仅 +1 行（无既有版本变化）。

未发现阻塞/重要项；5 条建议/备注见 §3（均为口径/加固/台账类，不阻塞合并）。

## 2. 四维评审

| 维度 | 结论 | 依据（文件/行/测试/命令） |
|---|---|---|
| 功能正确性 | ✅ | ① AC-08a–d 断言逐项核对（`harness_ac08.rs:271-322/324-374/423-465/467-511/538-578`），与 task-6 a–d 逐条对应；② 真链路：`RoutingExecutor`（`:82-99`）→ `HarnessExecutor`（harness 已合并 `344b73d`）/`WorkerExecutor`（`common::executor`）；工具=真子进程（`WorkerToolRunner` + `worker_config` PYTHONPATH/ZGRAPH_PYTHON）；trace=真 SQLite（`TraceReader::streaming_events` 查询，`crates/zgraph-trace/src/reader.rs:447`）；③ 工具执行证据：第二轮 Tool 消息 `{"sum":5}` 来自真 Python `add` 模块（`fixtures/.../tools/add.py`），调用链为 worker 协议 `run` 帧；④ 超时生效证据：summary `timeout_ms=300`（声明的工具级超时）+ 整例 <15s + 无残留 worker 进程；⑤ unknown tool：run/instances error_code 与实例终态精确断言；⑥ AC-08b 手动用例的 env 门控/`#[ignore]` 行为实测（缺 env 时 skip/pass）。 |
| 代码结构合理性 | ✅ | 只新增 `tests/e2e/harness_ac08.rs` + `tests/fixtures/graphs/harness_ac08/**`，未改 kernel/harness/worker/py-worker 源码；`RoutingExecutor` 只做 handler 路由（不改变执行语义），复用 `tests/e2e/common` 既有 helper（`worker_config`/`compile_fixture`/`run_to_trace`/`instance`/`assert_lifecycle_shape`），不重复造轮子；fixtures 目录与既有 `tests/fixtures/graphs/**` 结构一致；`Cargo.toml` 只加 dev-dep 与 `[[test]]` 注册。 |
| 逻辑性 | ✅ | 每个用例独立 `tempfile::TempDir` + 独立 db；时序断言只用宽松上限（`CASE_TIMEOUT`/`TOOL_TIMEOUT_CASE_LIMIT`，`:59-66`）与 trace 轮询，不用 sleep 猜；token 段先按 `segment_index` 排序再拼接（`:190-199`）；unknown tool 断言"仅 1 轮请求"与"不喂回"一致；AC-08b 先检查 env 再构造 provider，避免无 key 时网络/panic。 |
| 可读性 | ✅ | 文件头 docstring 画出数据通路、列出用例、给出手动执行命令（`:1-37`）；常量集中（`CASE_TIMEOUT`/`TOOL_TIMEOUT_CASE_LIMIT`/`SLOW_TOOL_TIMEOUT_MS` 等）；helper 有 docstring 说明来源（05 §5.8/§5.9/06 §6.2.9）；73 处断言无恒真（静态扫描无 `assert!(true`/`assert_eq!(x,x)` 等）；函数最长 ≤60 行；fixtures 带中文注释说明用途与声明。 |

## 3. 发现清单

| # | 级别 | 位置 | 问题 | 建议 | 状态 |
|---|---|---|---|---|---|
| 1 | 建议（口径） | `tests/e2e/harness_ac08.rs:586-636`（`ac08b_real_llm_manual`） | 07 AC-08 原文要求「trace 中出现 `ToolCallStart/ToolCallEnd`（至少一次）与 `TokenDelta`（聚合后非空）」；本手动用例只断言 `Completed`/`usage.tokens>0`/content 非空/token 聚合非空，**未断言真实模型一定调用了工具**（避免模型波动导致手动验收 flaky）。属有意口径 | 由 docs `REV-023` 在 `07-验收用例.md` AC-08 明确 M4 手动口径（真实模型不硬断言工具调用；工具链路由 fake a–c 用真 Python worker 覆盖）；若希望保留 07 字面断言，可在手动用例增加宽容断言（工具调用可选）+ 文档说明。**本分支无需改**；与 e2e-dev 口径一致，无需 ISSUE | open（docs 承接） |
| 2 | 建议（覆盖加固） | `tests/e2e/harness_ac08.rs:538-578`（unknown tool） | 用例断言了 Crashed/错误码/1 轮请求，但**未断言 trace 中不存在 `tool_call_start/end`**（harness 规范：未知工具在 emit 前失败；harness 单测 `unknown_tool_is_crashed_before_execution` 已覆盖）。 | 可加一条 `assert!(segments_of_kind(&streaming_segments(...), "tool_call_start").is_empty())` 等断言（低成本）；或由 docs 在 05 §5.6.6 明确"未知工具不产生 ToolStart/End"。不阻塞 | open（docs/可选加固） |
| 3 | 建议（进程组 kill 强证据） | `tests/e2e/harness_ac08.rs:467-511`（tool timeout） | 用例通过「整例 <15s + summary.timeout_ms=300」证明 300ms 生效，但未直接断言 worker 子进程/进程组消失（M3 e2e 有 pid 文件模式）；harness drop→supervisor kill 链路由 harness/worker 单测覆盖。 | 若想在本 e2e 加强，可让 `slow` fixture 写 pid 文件、用例结束后断言不存在（参考 M3 `assert_pid_gone`）；当前实现 + 本次独立 `pgrep` 验证（无残留）已可接受，不阻塞 | open（可选加固） |
| 4 | 备注（跨模块） | `Cargo.lock`（+1 行） | `Cargo.lock` 属 **process** 模块，本分支为其新增 path dev-dep 的机械副作用：`zgraph-e2e` 依赖列表 +`zgraph-harness`；**无既有版本变化、无其它包改动**（diff 全文仅 1 行） | 按 AGENTS §2 需在评审记录/交付中确认跨模块授权；Lead 在 REV-022 指派中已要求核对（本评审确认最小）。建议 Lead 合并时在记录/流程中显式确认（与 REV-020 的 Cargo.lock 同类处理） | open（Lead 确认） |
| 5 | 备注（构建产物） | `tests/fixtures/graphs/harness_ac08/tools/__pycache__/` | 运行测试会在 fixtures 下生成 `__pycache__`（本地已出现，`.gitignore` 已忽略；`git ls-tree -r HEAD` 无 `.pyc`/`__pycache__`，未入库）。 | 无需处理（不提交即可）；CI/后续运行注意不要 `git add -A`；可考虑在 e2e 文档注明测试会生成 pycache | 无需处理 |

> 无阻塞项、无重要项。#1/#2 为 AC-08 验收口径/覆盖加固（由 docs `REV-023` 承接），#3 为可选加固，#4 为 Lead 合并时跨模块授权备注，#5 为构建产物提示。**同意 e2e-dev 判断：本分支无需登记 P1/P2 ISSUE。**

## 4. 验证证据

### 4.1 范围、分支与提交元数据

```bash
cd /Users/zyzs/lczyzs/code/zgraph/.worktrees/e2e-e2e-dev
git status --short                        # 空（worktree clean；仅测试生成的 ignored pycache）
git branch --show-current                 # test/e2e-m4-ac08
git merge-base origin/main HEAD           # 344b73d7bfaec9a96a2a99cbf8e0b989bb103e31（= base = origin/main，已 rebase）
git rev-parse HEAD                        # 76bbe056efbe4c398ed698139bf939f59b24eb6f
git diff --shortstat 344b73d..76bbe05     # 10 files changed, 811 insertions(+), 1 deletion(-)
git diff --name-status 344b73d..76bbe05
# M Cargo.lock；M tests/e2e/Cargo.toml；A tests/e2e/harness_ac08.rs；A tests/fixtures/graphs/harness_ac08/{graph.json,fake_llm.json,agents/{answer_writer.json,prompt.md},tools/{add,explode,slow}.py}
git log -1 --format='%an <%ae> / %cn <%ce>' HEAD   # chenyuxiangg <...> / chenyuxiangg <...>
# 提交正文含 Review: REV-022 / Agent: e2e-dev
git ls-tree -r --name-only HEAD tests/fixtures/graphs/harness_ac08 | grep -E "pyc|__pycache__" || echo none   # none
```

- `Cargo.lock` diff（全文）：仅 `[[package]] name="zgraph-e2e"` 的 `dependencies` 插入 `"zgraph-harness",` 一行；无版本变化/无其它包。
- 未改 `crates/**`、`python/**`、`doc/**`、`scripts/**`；`tests/fixtures/graphs/harness_ac08` 为新增验收资产。

### 4.2 必跑命令与结果（worktree 内，干净 `CARGO_TARGET_DIR=/tmp/rev022-target`，`CARGO_HOME=<repo>/.cargo-home`，`ZGRAPH_PYTHON=<repo>/.venv/bin/python`）

| 命令 | 结果 |
|---|---|
| `cargo test -p zgraph-e2e --test harness_ac08 --quiet` ×3 | **3/3 稳定：`4 passed; 0 failed; 1 ignored`**（0.44s / 0.39s / 0.37s；日志 `/tmp/rev022-t{1,2,3}.log`） |
| `cargo test -p zgraph-e2e --quiet`（全包，6 个 test binary） | **exit 0：23 passed / 0 failed / 1 ignored**（既有 M3 e2e 用例无回归；`/tmp/rev022-all.log`） |
| `cargo test --workspace --quiet`（workspace 全量） | **exit 0：423 passed / 0 failed / 2 ignored**（2 ignored = harness 真实模型 + 本 e2e 真实模型；`/tmp/rev022-ws.log`） |
| `bash scripts/dev/check.sh` | **CHECK PASSED**（含 `cargo fmt --check`、Python 语法、禁用文件等；`/tmp/rev022-check.log`） |
| `git status --short`（全部命令后） | 空 |

### 4.3 用例 a–e 与 AC-08/task-6 对照（独立读代码 + 实跑）

| 用例 | 断言（代码位置） | 实际结果 |
|---|---|---|
| a `ac08a_fake_llm_tool_loop_streams_tokens_into_trace` | run/实例 Completed、output 四字段、`iterations==2`、usage `total>0`/`=prompt+completion`/`source=="provider"`、token 段拼接==content、`tool_call_start/end(add,ok,call_id)`、progress 恰 2 条（message/percent）、无失败工具事件、lifecycle、`requests[0].tools==["add","explode","slow"]`、`requests[1]` 含 `{"sum":5}`（`:324-374`/`:271-322`） | passed（3 连跑） |
| b `tool_exception_is_fed_back_and_run_continues` | run Completed、恰好 1 条失败 `tool_call_end`（explode）、summary `error.reason=exception/type=ValueError/message=boom`、`requests[1]` Tool 消息含 traceback（`:423-465`） | passed |
| c `tool_timeout_is_fed_back_and_run_continues` | run Completed、整例 `<15s` 且 `>=250ms`、summary `reason=timeout/timeout_ms=300`、`requests[1]` Tool 消息同形（`:467-511`）；**独立复核**：单跑 0.41s，随后 `pgrep -fl "zgraph_worker.runtime"` **无残留** | passed |
| d `unknown_tool_crashes_instance` | run Failed、`run.error_code==instances.error_code=="HARNESS_UNKNOWN_TOOL"`、实例 Crashed、`requests.len()==1`、RunFinished payload 状态/码（`:538-578`） | passed |
| e `ac08b_real_llm_manual` | `#[ignore]`；缺 env 时 skip/pass（实测 `--ignored` 无 env → `[skip] …` + `1 passed`）；有 env 时用 `GenaiLlmProvider(fake_mode=false)` 断言 Completed/非空 content/usage>0/token 非空（`:586-636`） | skip 路径 passed（真实模型需手动） |

### 4.4 真链路/无 mock/静态核对

| 项 | 结果 |
|---|---|
| 真 kernel + 真 trace | `common::run_to_trace` → `zgraph_cli::bridge::run_in_process`；断言经 `TraceReader::streaming_events/run/instances` 读 **真 SQLite**（`crates/zgraph-trace/src/reader.rs:447`）；未使用 `FakeExecutor`/mock trace |
| 真 Python 工具 worker | `WorkerToolRunner::new(common::worker_config(...))`（`:140-155`）→ 每次工具调用 spawn `python -m zgraph_worker.runtime`；`add` 真实返回 `{"sum":5}` 出现在第二轮上下文，证明真执行 |
| mock 扫描 | `grep -iE "mock|FakeExecutor|stub"` 仅命中文件头注释「**禁止 mock** kernel/worker/trace」；无 mock 代码 |
| 绝对路径 | `grep "/Users/\|/tmp/"` 于新增源码/fixtures（文本）→ **none**；`Cargo.toml` 用 `../../crates/zgraph-harness` 相对路径 |
| 恒真断言/函数长度 | 无 `assert!(true`/`assert_eq!(x,x)` 类；73 处断言；AST 括号实测函数均 ≤60 行 |
| pycache/禁用文件 | `git ls-tree` 无 `.pyc`/`__pycache__`；`git status --ignored` 的 pycache 为运行产物且被 `.gitignore` 忽略（未提交） |
| 时序稳定性 | 仅宽松上限（30s/15s）与 `pgrep` 复核；无 `sleep` 猜时序；3 连跑稳定 |

## 5. 遗留问题

- **docs 承接（REV-023）**：#1 AC-08 真实模型手动口径（不硬断言工具调用）、#2 未知工具不产生 ToolCallStart/End 的口径/断言，建议在 07 AC-08/05 §5.6.6 明确；与 e2e-dev 口径一致，无需本分支 ISSUE。
- **Lead 合并备注**：#4 `Cargo.lock` +1 行为 process 模块跨模块改动（最小、无版本变化），建议合并记录确认授权（AGENTS §2）。
- **可选加固**：#3 timeout 用例的 worker 进程组 kill 直接断言（pid 文件模式，参考 M3）；当前独立 `pgrep` 验证已无残留，不阻塞。
- 本评审未修改任何代码/测试/台账；REV-022 记录为本分支唯一新增文档。

## 6. 修订（2026-10-30，reviewer-b，rebase 后 HEAD `fda996c`）

> 本分支按 Lead 要求在 REV-022 记录合并入 main 后 rebase 到最新 `origin/main`（`97531fe`），新 HEAD **`fda996c`**。按 AGENTS §6 追加本节，原 §1–§5 结论与证据不删。

- **内容一致性核对**：`git diff --stat 76bbe05 fda996c -- tests Cargo.lock` **为空**——测试/fixtures/`Cargo.toml`/`Cargo.lock` 与评审对象 `76bbe05` **逐字节一致**；两提交差异仅在 main 上的 `doc/reviews` 文档（rebase 带入）。rebase 后范围仍为 `97531fe..fda996c` = **10 files changed, 811 insertions(+), 1 deletion(-)**（与评审时相同）。
- **rebase 后复验**：`git status` clean；`cargo test -p zgraph-e2e --test harness_ac08 --quiet` → **4 passed / 0 failed / 1 ignored（0.38s）**（干净 target `/tmp/rev022-target`）；`check.sh` 由作者复跑 PASS（本评审前次已验证，rebase 未改任何被检文件）。
- **结论不变：通过**（§3 的 #1–#5 状态不变；#4 `Cargo.lock` 跨模块授权仍请 Lead 合并时按 §5 确认）。
- 可合并性：分支基于最新 `origin/main`（`97531fe`）、worktree clean、评审记录结论「通过」→ 满足 `merge.sh` 前置。
