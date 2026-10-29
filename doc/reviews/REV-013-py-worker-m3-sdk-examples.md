# REV-013 · py-worker · M3 示例 worker 与 L3 幂等/副作用契约

> **模块声明**：本分支只改 `python/workers/**`（9 文件）与 `python/tests/test_workers.py`（1 文件），未改 `crates/**`、`tests/fixtures/**`、`doc/**`；符合 AGENTS §2 的 py-worker 模块边界（fixture 图只被测试读取、未被修改）。作者 `py-worker-dev`，评审人 `reviewer-b`（非作者，独立评审）。

- **分支**：`feat/py-worker-m3-sdk-examples`
- **作者**：`py-worker-dev`
- **评审人**：`reviewer-b`
- **变更范围**：`a6f4f5c..4b2c350`（`origin/main` → HEAD，2 commits；`git diff --stat` = 10 files, +667/−0：`52a26ed feat(py-worker)` 9 files +299、`4b2c350 test(py-worker)` 1 file +368；无 Rust/文档/台账改动）
- **评审时间**：2026-10-30
- **关联**：用户任务 task-2；`doc/issues.md` **ISSUE-022**（L3 幂等/副作用契约）、**ISSUE-011**（software_team worker/产物）；`doc/architecture-v2.md` §2.1（`python/workers/` 目标布局）/§4（每节点一子进程）/§6.1（M3 DoD「3–5 个示例 worker 迁移」）；`doc/spec-v1.2/04` §4.3/§4.5；`doc/spec-v1.2/01` §1.2.2（自环/`max_attempts`）；`doc/spec-v1.2/02` §2.8（`trigger` 缺省 `all`）；fixture `tests/fixtures/graphs/valid_parallel.json`、`valid_cycle.json`；legacy `legacy/src/worker/software_team/**`

## 1. 结论

**结论：通过**

本分支的交付物（5 组示例 worker + 真实子进程 e2e + L3 四要素声明）**实现正确、测试真实、模块边界干净、提交元数据合规**，无阻塞项；3 项重要发现均属「验收口径/台账跟踪」性质，**建议由 Lead 在合并时登记 ISSUE（本评审按指令不改 `doc/issues.md`）**。核心判断（覆盖任务指定的重点问题）：

1. **cycle 单次回环语义（作者标注待确认）——worker 函数逻辑可接受，但依赖的 fixture 图不可端到端运行**：`workers.cycle.looping` 的首轮 `again=None` 假设只对 `trigger="any"` 的自环节点成立，而 `valid_cycle.json` 的 `loop` **未写 `trigger`** → 缺省 `all`（02 §2.8），`seed` 与 `again` 两个 required 端口都须到达，`again` 又只能由自环边回投 → 首轮永不就绪；且 `loop→done` 边**未门控**，每次发布都投递，第二轮会命中已终态 `done` → `RUN_DELIVERY_TO_TERMINAL`。因此该 fixture 是 DSL R13 校验图（`gated-self-loop`），**不能**作为本 worker 的回环验收图；分支测试只做直接调用/子进程调用（未经过 kernel Delivery），**未验证也未反驳**图级回环。worker 函数本身与 spec 自环/retry 语义（01 §1.2.2；kernel `tests/self_loop.rs` 用显式 `trigger:"any"`）一致，可保留。详见 §3 #1。
2. **ISSUE-022 只落地了「示例层」**：四要素已写入每个 worker docstring 并由测试守住（`python/tests/test_workers.py:128`），但 ISSUE-022 要求的「写入 worker SDK 契约与 AC」——`doc/spec-v1.2/04` §4.3 契约条款与 `doc/spec-v1.2/07` 验收用例——未涉及（属 docs 模块边界，本分支不改是对的）。**ISSUE-022 不能因本分支关闭**。详见 §3 #2。
3. **`_inputs.unwrap_port` 必要且实现正确**：v1 整包广播（04 §4.5.1）下端口收到上游整个 output，`slow`/`planer`/`archer` 的端口与业务字段同名，解包是必要的防御；纯读、不改 `inputs`、非 dict/无同名字段时原样返回，边界测试覆盖。但示例间**口径不一致**：`echo`/`parallel` 直接透传端口值（真实投递下 `message`/`x` 是上游整包），与 `slow`/`planer`/`archer` 的解包不同；且 4 条子进程 e2e 喂的是「已解包」inputs，未覆盖真实整包形态。详见 §3 #3。

验证结论：worktree 内 `pytest` **44 passed / 0 failed**（基线 `doc/test-baseline.txt` 为空 → 无新增失败、全绿）、`check.sh` **CHECK PASSED**、六个模块 import **OK**；两条提交 author/committer 均为 `chenyuxiangg <1007423165@qq.com>`，正文含 `Review: REV-013` / `Agent: py-worker-dev`；`git status --porcelain` 干净；`git log origin/main..HEAD` 恰 2 条；`git merge-base origin/main HEAD == origin/main == a6f4f5c`（可 ff）。

## 2. 四维评审

| 维度 | 结论 | 依据（文件/行/测试） |
|---|---|---|
| 功能正确性 | ✅ | 覆盖 M3 DoD「3–5 个示例 worker」：echo/slow/parallel(a,b)/cycle(looping)/software_team(planer,archer) 共 7 个可调用符号，全部模块级同步、恰好 2 个必填位置参数（04 §4.3.5 S2–S5，测试 `test_worker_functions_are_module_level_sync_with_two_params`）；模块名/函数名与 `valid_parallel.json`、`valid_cycle.json` 的 `python_node.config` 一致；4 条真实子进程 NDJSON e2e（echo/parallel.a/cycle.looping/planer）验证 result 与 emit/log 帧；slow 的取消与 PID 文件、parallel 的 0.2s 睡眠、planer/archer 的 stub 文本与路径均与 legacy 对照一致。局限见 §3 #1/#3（图级回环与整包投递未做 kernel 级验证）。 |
| 代码结构合理性 | ✅ | 只动 `python/workers/**` + 对应测试（10 files, +667），无跨模块写入、无 Cargo/doc 漂移；`_inputs.py` 收敛共用解包逻辑（避免三处重复），`software_team/` 子包与 `__init__.__all__` 层级清晰；依赖方向单向（示例 → `zgraph_worker` SDK / 包内 `_inputs`），无框架反向依赖。重复实现（SDK 内 echo/slow）见 §3 #4。 |
| 逻辑性 | ✅ | 分支/循环自洽：`slow` 每 50ms 协作自检并由 runtime 捕获 `WorkerCancelled` → `NODE_CANCELLED`；取消已置位时立即抛出（测试实耗 <1s）；`parallel` a/b 各自独立、无共享可变状态；`cycle` 以 `continue` 字段驱动门控、重试轮返回 `False`，与 spec 自环「每次投递新建 attempt+1 实例」兼容；无吞异常、无死循环（sleep 有 deadline）、无竞态。前提性风险见 §3 #1、表述小项见 §3 #6。 |
| 可读性 | ✅ | 全部函数有类型注解（含返回 `-> dict[str, Any]`）、字符串一律双引号、常量全大写（`FIRST_ITERATION`/`RETRY_ITERATION`/`PARALLEL_SLEEP_S`/`CHECK_INTERVAL_S`/`DEFAULT_*`）、snake_case；函数均 ≤60 行（最长 `unwrap_port` 28 行、`_execute_in_runtime` 26 行）；docstring 四要素齐全且「外部副作用」为真实声明（`slow` 的 PID 文件、`ctx.emit` 归类说明均如实写出）；注释解释「为什么」（如整包广播、stderr 排空、monkeypatch 常量理由）。小项 §3 #7。 |

## 3. 发现清单

| # | 级别 | 位置 | 问题 | 建议 | 状态 |
|---|---|---|---|---|---|
| 1 | 重要 | `python/workers/cycle.py:13-39`；`tests/fixtures/graphs/valid_cycle.json:10-12,24-31`；`python/tests/test_workers.py:94-109,195-206` | cycle 与 fixture 的**图级**语义不一致，作者「待确认」项结论为「不可端到端验收」：① `loop` 缺省 `trigger="all"`（02 §2.8），`seed`+`again` 双 required，`again` 仅能由自环边回投 → 首轮 `is_ready` 永假（kernel `run.rs:1160` `is_ready`；`run.rs:1627` 未到端口不投递），实例 Pending 至死锁；worker docstring（`cycle.py:18-20`）假设的「首轮 `again=None`」只在 `trigger="any"` 下成立（kernel 自身 `tests/self_loop.rs:14-21` 即显式写 `trigger:"any"`）。② `loop→done` 边（`valid_cycle.json:31`）无门控，`evaluate_outgoing` 对无门控边恒 `passed=true`（`run.rs:1631-1636`），首轮即向 `done` 投递并完成；第二轮再次投递命中终态实例 → `RUN_DELIVERY_TO_TERMINAL`（`run.rs:1009-1031`）。因此 `valid_cycle.json` 仅是可加载的 R13 校验图，**不是可运行的 bounded-loop 验收图**；本分支测试未经过 kernel Delivery，无法证明「单次回环」。 | **worker 函数逻辑保留**（与 spec 01 §1.2.2 自环/retry 及 kernel self-loop 测试模式一致）。① 文档口径：`cycle.py` docstring 显式声明前置条件「自环节点必须 `trigger:"any"`；`valid_cycle.json` 仅为 DSL R13 fixture，不可作为可执行验收图」；② 由 Lead 登记 ISSUE 给 **e2e/kernel**：补可执行的 bounded self-loop 图（`loop.trigger:"any"` + 出口边门控 `continue==false`，或纯 `max_attempts` 方案）与 kernel/e2e 断言。不建议在本分支改 fixture（e2e 模块边界）。 | open（建议登记 ISSUE；文档口径建议 author 后续/随 ISSUE 修正，不阻塞本分支） |
| 2 | 重要 | `python/workers/__init__.py:11-14`；`python/tests/test_workers.py:128-136`；`doc/issues.md` ISSUE-022；`doc/spec-v1.2/04` §4.3、`doc/spec-v1.2/07` | ISSUE-022 只完成「示例层声明」：四要素写进 7 个 worker docstring 且有契约守卫测试，但 ISSUE-022 的建议是「M3 前写入 **worker SDK 契约与 AC**」——`04` §4.3 Python 节点契约未增补「docstring 必须声明输入/输出/幂等性/外部副作用」要求，`07` 也未新增对应 AC。ISSUE-022 因此**不能关闭**，否则台账与实际不符。 | 本分支不改 `doc/**` 正确；Lead 合并时**保留 ISSUE-022 open** 并注明「示例层已落地（REV-013）」，同时登记 docs 任务：04 §4.3 增补 L3 声明条款、07 增 AC（建议 AC 断言「示例 worker docstring 四要素 + 副作用声明真实」）。 | open（建议登记/保留 ISSUE） |
| 3 | 重要 | `python/workers/echo.py:25`、`parallel.py:27,44` vs `slow.py:37,39`、`software_team/planer.py:31,35`、`software_team/archer.py:31,33`；`python/tests/test_workers.py:250-257,343-366` | 整包广播处理**口径不一致且未经真实投递验证**：v1 每条出边投递整个 output（04 §4.5.1），`slow`/`planer`/`archer` 用 `unwrap_port` 解同名字段，而 `echo` 的 `message`、`parallel` 的 `x` 直接透传 → 真实图里 `workers.parallel.a` 的 `x` 会是 start 的整包 `{"x": ...}`，与 `test_runtime_parallel_branch_end_to_end` 断言 `{"worker":"a","x":"value"}`（人工构造的已解包输入）不同；该测试未走 kernel，故两种口径都未被真实投递验证。当前不影响 AC-09（只断言 Running 重叠），但会让使用者误判端口值形态。 | 在 `workers/__init__.py` 或各 docstring 明确取舍并说明理由（「透传整包」vs「解同名字段」），保持示例自洽；在 e2e 任务中用 kernel 真实投递补一条断言（如 `valid_parallel.json` 中 `a.x` 实为 `{"x":"value"}`），把口径固化进验收。与 #1 的 e2e ISSUE 可合并登记。 | open（建议登记 ISSUE / e2e 任务） |
| 4 | 建议 | `python/workers/echo.py:1`、`slow.py:1` vs `python/zgraph_worker/echo.py`、`zgraph_worker/slow.py` | `workers.echo`/`workers.slow` 与 SDK 包内 `zgraph_worker.echo`/`slow` 功能重叠（slow 增加了 `pid_file`/`unwrap_port`/`cancelled` 字段，echo 增加了 `inputs`/`node_id`），两处并行演进易漂移；`python/tests/test_runtime.py` 仍在用 `zgraph_worker.echo/slow`。 | 在 `workers/__init__.py` 说明「SDK 内的是协议/运行时夹具，`workers/` 是对外示例」；长期可把 SDK 夹具迁往 `tests/fixtures/workers/`（AGENTS 已把该路径划给 py-worker），消除双份示例。 | open |
| 5 | 建议 | `python/workers/software_team/planer.py`、`archer.py`；对照 legacy `legacy/src/worker/software_team/__init__.py:24-56` | 迁移保真度**在内容层成立**（stub 文本 `# specification (stub)\n` / `# architectural_design (stub)\n` / `# organizational_structure (stub)\n` 与文件名、缺省路径逐一核对一致），副作用降级为纯函数 + `documents` 返回符合 L3 与整包广播方向，是合理设计（无需再声明文件副作用）；但 legacy 的**流程契约**是「proc 写 `base_dir/doc/*.md` + gate `check_output` 检查文件存在」，迁移后 gate 必须改读 output 字段，`software_team` 图本身尚未迁移 → 流程级保真度未验证。 | 随 e2e 补 `software_team` fixture 图（`check_output` 改为读 `document_paths`/`documents`）与最小 e2e；**ISSUE-011 保持 open**（worker 已内置，图/门控接线仍待做）。 | open（建议，ISSUE-011 联动） |
| 6 | 建议 | `python/workers/cycle.py:30` | `is_retry = again is not None and again is not False` 把显式 `False` 当首轮；在当前整包广播下 `again` 只会是 dict/None（该分支不可达），但与 fixture 把 `again` 声明为 `boolean` 的写法易混淆。 | 简化为 `again is not None`，或在 docstring 明确「`again` 只会收到整包 dict 或 None」；属语义澄清，不阻塞。 | open |
| 7 | 建议 | `python/tests/test_workers.py:157-164` | `test_slow_run_uses_default_seconds_when_absent` 末尾 `assert DEFAULT_SECONDS == 1.0` 是对模块 import 常量的恒真断言，不检验 monkeypatch 生命周期（pytest 自动恢复），有「凑断言」之嫌。 | 删除该恒真断言，或改为断言 `workers.slow.DEFAULT_SECONDS` 已恢复（monkeypatch 自动恢复本无需断言）；保留前面的缺省睡眠行为断言即可。 | open |
| 8 | 建议 | `AGENTS.md` §2 模块表；`python/tests/**` | 新测试落在 `python/tests/test_workers.py`，但 AGENTS §2 的模块表未把 `python/tests/**` 显式划给 `py-worker`（仅列 `python/zgraph_worker/**`、`python/workers/**`、`tests/fixtures/workers/**`），归属有歧义；本分支延续历史做法（`python/tests/test_ctx.py` 等同处），不构成越界。 | 由 process 模块在 AGENTS 模块表补 `python/tests/**` → py-worker，避免后续并行开发争用。 | open（process 模块） |
| 9 | 建议 | `python/tests/test_workers.py:343-366` | 4 条子进程 e2e 覆盖 echo/parallel.a/cycle.looping/planer，未覆盖 archer 与 cycle 第二轮（直接调用已覆盖，故非缺陷）。 | 后续可补 archer 子进程用例与 cycle 第二轮（`again=<首轮 output dict>`）子进程用例，或在注释说明 4 条选型理由。 | open |

> 无阻塞项；#1–#3 为重要项，均建议以 ISSUE 形式跟踪（本评审按任务约束不改 `doc/issues.md`）；#4–#9 为建议项，可后置但建议进台账。

## 4. 验证证据

### 4.1 范围、分支与提交元数据

```bash
cd /Users/zyzs/lczyzs/code/zgraph/.worktrees/py-worker-agent-a
git branch --show-current          # feat/py-worker-m3-sdk-examples
git merge-base origin/main HEAD    # a6f4f5ce9434faade70934d44f26df6cfe38c671 == origin/main
git log --oneline origin/main..HEAD
# 4b2c350 test(py-worker): 覆盖 python/workers 全部示例与 runtime 子进程端到端
# 52a26ed feat(py-worker): 新增 python/workers 示例包与 L3 幂等/副作用契约
git diff --stat a6f4f5c..HEAD      # 10 files changed, 667 insertions(+), 0 deletions(-)
git status --porcelain             # 空（评审提交前）
```

- 两条提交 `author`/`committer` 均为 `chenyuxiangg <1007423165@qq.com>`；正文均含 `Review: REV-013` 与 `Agent: py-worker-dev`；subject 为 `<type>(<module>): <摘要>`。
- 改动文件全部在 `python/workers/**` + `python/tests/test_workers.py`；`doc/progress.md`、`doc/issues.md`、`doc/reviews/README.md`、`CLAUDE.md`、`crates/**`、`tests/fixtures/**` 均未被修改（`git diff --name-only` 核对）。
- 提交粒度：一个 feat（示例包）+ 一个 test（覆盖），无无关改动混入。

### 4.2 必跑命令（worktree 内）

```bash
PYTHONPATH=python /Users/zyzs/lczyzs/code/zgraph/.venv/bin/python -m pytest -q
# 44 个点全部通过、exit 0；注意 pyproject addopts 已含 -q，再叠 -q 会抑制末行 summary
PYTHONPATH=python /Users/zyzs/lczyzs/code/zgraph/.venv/bin/python -m pytest
# 44 passed in 1.55s
bash scripts/dev/check.sh
# 12 项 OK（身份/远端/冻结文件/禁用文件/分支名/Python 语法/progress、issues 存在/行数/cargo fmt/git diff --check）→ CHECK PASSED，exit 0
PYTHONPATH=python python3 -c "import workers.parallel, workers.cycle, workers.echo, workers.slow, \
  workers.software_team.planer, workers.software_team.archer; print('IMPORT_OK')"
# IMPORT_OK，exit 0（venv python 与系统 python3 各跑一次均 OK）
```

- 测试结果：**44 passed / 0 failed**；对照 `doc/test-baseline.txt`（已知失败清单为空）→ **无新增失败**，基线保持全绿。
- 未跑 cargo：本分支无 `crates/**` 改动，M3 worker 验收不依赖 Rust 侧变更。

### 4.3 静态与语义核对（独立阅读，不采信 commit 自述）

| 核对项 | 结果 |
|---|---|
| 函数签名（04 §4.3.5 S2–S5） | 7 个 worker 均为模块级同步函数、恰 2 个无默认值位置参数；`inspect` 断言 + 实读一致 |
| fixture 对齐 | `valid_parallel.json` → `workers.parallel.a/b`；`valid_cycle.json` → `workers.cycle.looping`：模块名/函数名逐一命中（测试同时按图文件动态解析校验，非硬编码） |
| 整包广播（04 §4.5.1） | `_inputs.unwrap_port` 纯读、不改 `inputs`，非 dict/无同名字段原样返回；边界用例覆盖（`tests/test_workers.py:250-257`）。示例间口径差异见 §3 #3 |
| cycle 图级语义 | 见 §1 判断与 §3 #1：缺省 `trigger=all` + 未门控出口边 ⇒ fixture 不可作为运行验证图；`run.rs:1009-1031`（终态投递 fail-fast）、`run.rs:1160`（All 就绪 = required 全覆盖）、`run.rs:1627-1636`（无门控边恒放行）为独立核对依据 |
| legacy software_team 保真 | `legacy/.../software_team/__init__.py` 的 `planer_proc`/`archer_proc` stub 文本、缺省文件名与迁移结果逐字符一致；副作用（写文件）已移除并改为 `documents` 返回，docstring 如实声明「无」 |
| L3 声明真实性 | `slow` 声明 PID 文件写 + `ctx.check_cancelled`，代码一致；其余声明「无」且实现确实只读/emit 可丢事件；无隐藏文件/网络/全局状态副作用 |
| 测试真实性 | 4 条 e2e 真起 `python -m zgraph_worker.runtime` 子进程、真 NDJSON 握手/result、后台排空 stderr；非 mock 断言；`assert elapsed >= 0.2` 等断言非恒真（parallel/slow 实测耗时满足） |

### 4.4 结论依据的边界声明

- 本评审**没有**运行 kernel→worker 的真实端到端（main 侧 CLI 尚无 `run` 子命令，M2 验收用 `FakeExecutor`），故 §3 #1/#3 的图级语义结论来自代码路径核对（fixture + kernel 源码 + spec），非运行观测；这一验证缺口本身即建议登记 ISSUE。
- 本评审未改任何代码/台账/进度文件，未 push/merge。

## 5. 遗留问题

- **已建议登记/保留（由 Lead 在合并时处理，本评审不改 `doc/issues.md`）**：
  - 新 ISSUE（建议，e2e/kernel）：bounded self-loop 的可执行验收图与出口门控语义；覆盖 §3 #1 的 `trigger="any"` + 出口边门控与 kernel Delivery 级断言。
  - 新 ISSUE（建议，e2e）：整包广播示例口径统一 + kernel 真实投递断言（§3 #3，可与上条合并）。
  - `ISSUE-022` **保持 open**：04 §4.3 契约条款与 07 AC 回写（§3 #2；建议注明「示例层已落地，REV-013」）。
  - `ISSUE-011` **保持 open**：software_team 图/门控接线与最小 e2e（§3 #5）。
  - 流程建议（§3 #8）：process 模块在 AGENTS §2 补 `python/tests/**` → py-worker。
- **未登记但需跟进（建议级）**：SDK 内 echo/slow 与 `workers/` 示例的双份实现收敛（#4）；cycle `is_retry is not False` 语义澄清（#6）；`test_slow_run_uses_default_seconds_when_absent` 恒真断言（#7）；archiver/cycle 第二轮子进程覆盖（#9）。
- 未 push/merge；`main` 未被本分支改动；作者 worktree 在评审后仍可合并（基于最新 `origin/main=a6f4f5c`，`--ff-only` 可进）。

## 6. 条件关闭记录

不适用：本记录结论为 **通过**，无前置条件。§3 的 3 项重要发现均建议以 ISSUE 跟踪，不构成合并阻塞。
