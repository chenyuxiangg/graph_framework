# REV-019 · py-worker · M4 tool_runner（discover/call）+ 工具 fixtures

> **模块声明**：本分支只新增 `python/zgraph_worker/tool_runner.py`、`python/tests/test_tool_runner.py`、`tests/fixtures/workers/tools/**`，均在 `py-worker` 模块边界内（`python/tests/**` 的归属由 REV-017 补入 `AGENTS.md` §2 并已合并到 main `389b277`；task-3 亦已把该路径列入 write scope）。作者 `py-worker-dev`，评审人 `reviewer-b`（**非作者**）。本次评审**未修改任何代码/测试/台账**，只新增本记录；未 commit / 未 merge / 未 rebase。
>
> **独立核对口径**：本人亲自在 worktree 内读 diff/全文、跑 pytest、check.sh，并**在仓库测试之外**用真实 `python -m zgraph_worker.runtime` 子进程独立验证 discover 失败路径/异常路径均以 `result` 帧 + exit 0 结束、以及跨进程确定性；不采信 author 自述。

- **分支**：`feat/py-worker-m4-tool-runner`
- **作者**：`py-worker-dev`
- **评审人**：`reviewer-b`
- **变更范围**：`887743a..e49d2a1`（1 commit `e49d2a1 feat(py-worker): 新增 tool_runner（discover/call）与工具 fixtures（REV-019）`；`git diff --shortstat` = **11 files changed, 979 insertions(+)**，无删除/修改既有文件；base = merge-base = `887743a`，**未 rebase**——评审时 `origin/main` 已前进至 `389b277`（REV-017 合并），合并由 `merge.sh` 自动 rebase）
- **评审时间**：2026-10-30
- **关联**：spec 05 §5.6.1（ToolDef 字段）/§5.6.2（Python 侧两种声明+发现规则）/§5.6.5（执行/超时/结果处理）/§5.6.6（工具错误语义）；architecture-v2 §8 **D3**（工具每次调用一个 worker 子进程、无跨调用模块缓存）；`schemas/worker-protocol-v1.md` §2（`run` 帧）/§3（退出码封闭表）；task-3；REV-017（模块归属）

## 1. 结论

**结论：通过**

实现与 task-3 契约**逐项一致**：module 固定 `zgraph_worker.tool_runner`、function 固定 `discover`/`call`；`discover` 输入 `{"tools_dir": str}`、成功 `{"ok":true,"tools":[{name,description,parameters,source}]}`、失败 `{"ok":false,"error":{reason,message,traceback}}`；`call` 输入 `{"tools_dir","module","name","args"}`、成功 `{"ok":true,"result":<dict>}`、非 dict → `result_not_dict`、任意异常 → `exception`（type/message/≤8 KiB traceback）；reason 封闭表 `tools_dir_missing / tool_module_import / duplicate_tool / invalid_declaration / result_not_dict / exception` 与契约一致。工具级问题**全部结构化返回、绝不抛给运行时**——本人独立用真实子进程验证了 bad_import / 缺失目录 / 工具异常三条路径均为 **恰好 1 帧 `result` + exit 0**。文件扫描跳过 `_` 开头与 `__pycache__`、按 POSIX 相对路径排序（发现顺序 = 文件序 + 声明序）；`sys.path` 只在缺失时插入 `tools_dir.parent` 且不重复。无跨调用模块缓存、无 `print`/stdout 污染、无越界改动；函数最长 36 行、测试最长 28 行，类型注解/双引号/全大写常量/snake_case 符合 `CLAUDE.md`。

`pytest python/tests` **75 passed / 0 failed**（与 Lead 预期一致，既有 44 条未破坏），`check.sh` CHECK PASSED，worktree clean，提交元数据合规。未发现阻塞/重要项；4 条建议 + 2 条跨模块/流程备注见 §3。

## 2. 四维评审

| 维度 | 结论 | 依据（文件/行/测试/手工验证） |
|---|---|---|
| 功能正确性 | ✅ | ① 契约形状逐字核对：`discover`（`tool_runner.py:235-262`）成功仅 `{ok,tools}`、失败固定含 `traceback` 键（可为 null）；`call`（`:299-311`）成功 `{ok,result}`。② reason 封闭表 6 值（`:31-36`）与契约一致，无额外/缺失。③ 扫描规则：`_iter_tool_files`（`:57-67`）跳过任一路径段 `_` 开头（覆盖 `__pycache__`）、按 `relative.as_posix()` 排序；`_module_name_for`（`:80-83`）为 `f"{tools_dir.name}.{相对点分路径}"`，与契约一致；测试 `test_discover_skips_underscore_entries_and_pycache`（`test_tool_runner.py:214`）用「被跳过模块一旦导入即抛错」做负例，断言顺序 `["inner","top"]` 与 source 名。④ 声明形式：恰好 `TOOLS(list)` 或 `TOOL(dict)`，两者都有/都没有 → `invalid_declaration`（`:93-118`，测试 `:262/:268/:287`）。⑤ 工具校验：name 正则 `^[a-z][a-z0-9_]{0,63}$`、description 1..1024、parameters 为 object 且 `type=="object"`、`properties` 为 object（`:121-156`；参数化反例 `:279` 覆盖 9 类非法字段）。⑥ 跨模块重名 → `duplicate_tool`（`:185-203`，测试 `:296`）。⑦ `call` 按声明形式分派（A → `call(name,args)`、B → `call(args)`，`:286`），非 dict → `result_not_dict`（`:287-296`，测试 `:348`），异常 → `exception` 含 type/message/截断 traceback（`:222-232`，测试 `:338/:360`）。⑧ **永不非约定退出**：`call` 顶层 `try/except Exception`（`:308-311`）、`discover` 顶层 try（`:252-261`）；本人在真实子进程下独立验证 bad_import/缺失目录/explode 均 `frames=['result'] exit=0`（§4.4），仓库内亦有 3 条真实子进程往返测试（`:475/:487/:500`，且 `:500` 明确断言「工具异常不得用 error 帧」）。⑨ 确定性：同输入两次独立子进程输出逐字节一致（§4.4）；`test_discover_is_deterministic...`（`:199`）与 sys.path 不重复插入（`:207`）均有断言。 |
| 代码结构合理性 | ✅ | 新增独立模块，**未改** `runtime.py`/`protocol.py`/`ctx.py`/其它 worker 文件（`git diff --name-status` 仅 11 个新增文件）；职责单一：文件发现/模块导入/声明校验/执行分派各自成 helper（`_iter_tool_files`/`_import_tool_module`/`_declaration_form`/`_validate_tool`/`_discover_tools`/`_execute_tool_call`），公共入口只有 `discover`/`call`；无全局可变状态、无缓存（`grep lru_cache|_CACHE|globals()|sys.modules[` 无命中），符合 D3「无跨调用模块缓存」；fixtures 与单测分离，坏 fixture 各自独立目录便于复用。 |
| 逻辑性 | ✅ | 错误传播自洽：声明/导入错误用内部 `ToolDeclarationError` 携带 `reason/traceback`，`discover` 把它转结构化 error；`call` 的所有异常统一收敛为 `exception` 结果（不抛给 SDK，故不会产生 error 帧），与 worker 协议「工具失败是业务结果」一致；`_truncate_traceback` 按 UTF-8 字节边界安全截断（`:214-219`），不会切出非法字符；`_discover_tools` 单次扫描、`seen` 字典去重，无死循环/无吞异常（捕获后都带 reason 返回）。`sys.path` 插入只在缺失时执行（`:86-90`），测试断言 `count==1` 且 `sys.path[0]==parent`。 |
| 可读性 | ✅ | 模块级 docstring 写清 v2/D3 前提（每调用一进程、不改协议）与错误策略；每个 helper 有中文 docstring 解释"为什么/返回形状"；全量类型注解（含 `re.Pattern[str]`/`Path | None`/`dict[str, Any]`）；函数最长 **36 行**、测试文件最长 **28 行**（AST 实测，均 ≤60）；常量全大写且集中在文件头（`:24-36`）；无 `print`/stdout 写入（grep 无命中）；测试用例名与断言直白（如 `test_call_tool_exception_is_structured`）。 |

## 3. 发现清单

| # | 级别 | 位置 | 问题 | 建议 | 状态 |
|---|---|---|---|---|---|
| 1 | 建议 | `python/zgraph_worker/tool_runner.py:265-296`（`_execute_tool_call`）、`test_tool_runner.py:330` | `call` **不校验**请求的 `name`/`module` 是否属于该模块的声明集合：形式 B 完全忽略 `name`，形式 A 把任意 `name` 透传给模块 `call`（未知 name 由模块自己抛错 → `exception`）。契约把「工具名全局唯一 + 只调用已注册工具」的约束放在 harness 侧，当前行为与契约不冲突，但错误会表现为 `exception`（如 `KeyError`）而非更明确的「未知工具」。 | 在模块 docstring/契约文档中显式写明「`call` 只负责按 source 分派，name/module 合法性由 harness registry 保证」；若希望防御，可先用该模块声明校验 name（每调用一进程，开销可接受）。不阻塞。 | open |
| 2 | 建议 | `python/zgraph_worker/tool_runner.py:86-90`（`_ensure_import_path`） | `tools_dir.parent` 被**永久**插入 `sys.path`，同一进程内多次 discover/call 会累积不同父目录（生产模型是「每次调用一个新 worker 进程」，故无实际影响；单测依赖 autouse fixture 清理）。函数 docstring 未写明这一单次进程假设。 | 在 `_ensure_import_path`/模块 docstring 补一句「仅用于单次进程调用；不负责恢复 sys.path」，或记录到契约文档。不阻塞。 | open |
| 3 | 建议 | `python/zgraph_worker/tool_runner.py:299-311` + `test_tool_runner.py:372` | `call` 阶段遇到声明非法（both/neither/缺 `call`）时统一返回 `reason="exception"`（`ToolDeclarationError`），而 `discover` 对同一情形返回 `reason="invalid_declaration"`；契约只为 `call` 定义 `result_not_dict`/`exception`，故当前实现可接受，但两个入口的 reason 语义不完全对齐，harness 若按 reason 分支容易误判。 | 在 REV-020/harness 契约与 docs 回写中明确「call 声明错误按 exception 处理（正常路径下 discover 已拦截）」；或 Python 侧统一为 `invalid_declaration`（需同步契约）。不阻塞。 | open |
| 4 | 建议 | `python/zgraph_worker/tool_runner.py:80-83`（`_module_name_for`） | 模块名由 `tools_dir.name` 拼接；若目录名含 `-`/`.`/空格等非 Python 标识符，`importlib.import_module` 会失败并报 `tool_module_import`（信息较间接）。该前提由 harness 资产约定保证（通常为 `tools/`）。 | 在契约文档标注「`tools_dir` 的目录名必须是合法 Python 标识符（推荐 `tools`）」；或对 `tools_dir.name` 做标识符校验并返回更明确的 reason。不阻塞。 | open |
| 5 | 备注（跨模块，REV-020 跟进） | spec 05 §5.6.1 的 parameters 子集约束（禁止 `$ref`/`oneOf`/`patternProperties`/`if/then/else` → `HARNESS_CONFIG_LOAD{reason:"unsupported_schema_keyword"}`） | 本次 Python 侧只校验 `parameters` 是 object、`type=="object"`、`properties` 是 object（task-3 契约范围），**未**拒绝上述保留关键字。该约束属 Rust harness 侧加载期校验职责。 | REV-020 评审时确认 harness 对 `unsupported_schema_keyword` 有实现与测试；若未实现需登记 ISSUE（spec 05 §5.6.1 明确要求）。 | open |
| 6 | 备注（合并顺序/基线） | 分支 base `887743a`；本分支触及的 `python/tests/**` 在 base 的 `AGENTS.md` §2 中尚未归入 `py-worker` | REV-017 已把 `python/tests/**` 补入 `AGENTS.md` 并合并到 main（`389b277`）；本分支按 `merge.sh` rebase 后模块边界一致，task-3 亦已授权该路径。评审时 worktree 内 `AGENTS.md` 仍是旧版（因未 rebase），属正常状态。 | 合并顺序无强制依赖（rebase 自动带上），无需处理；仅记录备查。 | 无需处理 |

> 无阻塞项、无重要项；#1–#4 为建议级，#5 为跨模块待 REV-020 确认，#6 为流程备注。结论「通过」依据是契约全部达成、无实质缺陷。

## 4. 验证证据

### 4.1 范围、分支与提交元数据

```bash
cd /Users/zyzs/lczyzs/code/zgraph/.worktrees/py-worker-py-worker-dev
git status --short                       # 空（worktree clean）
git branch --show-current                # feat/py-worker-m4-tool-runner
git merge-base origin/main HEAD          # 887743a5bbe51028cb3ee4ae4152ef5092587aae（= base，未 rebase）
git rev-parse HEAD                       # e49d2a154e072cf165231e125baa6805a8f482e9
git rev-parse origin/main                # 389b277eca611451684b60006ccb77506bdcd7ec（评审时已含 REV-017）
git diff --shortstat 887743a..e49d2a1    # 11 files changed, 979 insertions(+)
git diff --name-status 887743a..e49d2a1  # 全部为 A（新增）：python/zgraph_worker/tool_runner.py、python/tests/test_tool_runner.py、tests/fixtures/workers/tools/** 9 个 fixture
git log -1 --format='%H|%an <%ae>|%cn <%ce>|%s' e49d2a1
# ...|chenyuxiangg <1007423165@qq.com>|chenyuxiangg <1007423165@qq.com>|feat(py-worker): 新增 tool_runner（discover/call）与工具 fixtures（REV-019）
# 正文含 Review: REV-019 / Agent: py-worker-dev
git diff 887743a..e49d2a1 --check         # exit 0（无空白/冲突标记）
```

- 未改动 `python/zgraph_worker/{runtime,protocol,ctx,errors}.py`、其它 `python/tests/*`、`crates/**`、`doc/**`、`pyproject.toml`；无删除/修改既有文件，符合"只新增"的任务约束。

### 4.2 pytest（worktree 内）

```bash
/Users/zyzs/lczyzs/code/zgraph/.venv/bin/python -m pytest python/tests -q
# 75 passed（`-q` 为 pyproject addopts 默认；去掉 -q 复核输出：75 passed in 1.70s）
```

- 基线回归：既有 `test_ctx.py/test_protocol.py/test_runtime.py/test_workers.py` 全部包含在这 75 条中且通过（无既有用例被改/跳过）。
- 新增 `test_tool_runner.py`：24 个测试函数（含参数化 9 个非法声明反例 + 多错误分支），覆盖 discover/call 成功与全部 reason；其中 3 条为**真实子进程** NDJSON 往返（`test_runtime_discover_end_to_end` `:475`、`test_runtime_call_end_to_end` `:487`、`test_runtime_tool_exception_does_not_use_error_frame` `:500`），均断言收到 `result` 帧、`proc.wait()==0`，非 mock。

### 4.3 check.sh / 静态约束

```bash
bash scripts/dev/check.sh
# OK×12（身份/SSH/CLAUDE.md/禁用文件/分支命名/ Python 语法/progress 21 行≤30/issues 存在/cargo fmt/git diff --check）→ CHECK PASSED
```

- AST 实测函数长度：`tool_runner.py` 19 个函数最大 **36 行**；`test_tool_runner.py` 39 个函数最大 **28 行**（均 ≤60，`CLAUDE.md`）。
- `grep -nE "lru_cache|_CACHE|globals\(\)|sys\.modules\["` → 无命中（无跨调用缓存/全局态）。
- `grep -nE "\bprint\(|sys\.stdout"` → 无命中（不污染协议 stdout；协议 fd 由 SDK 隔离）。
- `grep -rn "tool_runner" python/` → 除自身与测试外无其它引用（不侵入 runtime）。

### 4.4 独立手工验证（仓库测试之外）

用真实 `python -m zgraph_worker.runtime` 子进程 + NDJSON 帧（hello → hello_ack → run），独立验证失败路径与确定性：

| 用例 | 帧序列 | exit | 输出 |
|---|---|---|---|
| `discover` `tools_bad_import` | `['result']` | **0** | `ok=false, reason=tool_module_import`（traceback 非空） |
| `discover` 不存在的 `tools_dir` | `['result']` | **0** | `ok=false, reason=tools_dir_missing` |
| `discover` `tools_ok/tools` | `['result']` | **0** | `ok=true`，工具序 `now,add,explode,echo,sleepy` |
| `call` `tools.failing/explode` | `['result']` | **0** | `ok=false, reason=exception, type=ValueError` |
| `discover` 两次（独立进程） | — | 0 | 输出 JSON `sort_keys` 后**逐字节一致**（确定性） |

> 结论：错误路径**没有** error 帧、没有非约定退出码，符合协议 §3 与 spec 05 §5.6.6「工具失败是业务结果」；跨进程确定性成立。

## 5. 遗留问题

- **建议（可后置，Lead 视情况登记 ISSUE）**：#1 call 不校验 name 的契约说明；#2 sys.path 单次进程假设写清；#3 call/discover 的 reason 语义对齐（建议 docs/REV-020 契约写明）；#4 tools_dir 目录名合法标识符前提。
- **跨模块跟进（REV-020）**：#5 `parameters` 保留关键字（`$ref`/`oneOf` 等）拒绝属 harness 加载期职责，需在 REV-020 核对（spec 05 §5.6.1）。
- **流程**：#6 本分支 `python/tests/**` 归属依赖 REV-017（已合并 main）；merge.sh rebase 后一致，无需额外动作。
- 本评审未修改任何代码/测试/台账；REV-019 记录为本分支唯一新增文档。

## 7. 增量复审（2026-10-30，reviewer-b，HEAD `3fca90e`）

> **背景**：Lead 追加需求——工具声明（形式 A `TOOLS` 每项 / 形式 B `TOOL` 对象）支持可选 `timeout_ms`（int，`1..=600000`；`bool`/字符串/浮点/`null` 拒绝 → 仍归入既有 `invalid_declaration`；discover 输出在声明存在时携带、缺省省略），供 harness 侧 `ToolDef.timeout_ms` 使用（spec 05 §5.6.1）。作者 `py-worker-dev` 在 `e49d2a1` 上补 commit `3fca90e`；本节为**增量复审**，按 AGENTS §6 **不删除/不改写** §1–§6 原「通过」结论（§1/§4.2 的 "75 passed" 对应原 HEAD `e49d2a1`，本增量后 HEAD 为 84 passed）。
>
> **结论：增量复审通过**（全分支 REV-019 维持「通过」）。

- **增量范围**：`e49d2a1..3fca90e`（1 commit，`git diff --shortstat` = **4 files changed, 82 insertions(+), 4 deletions(-)**）：`python/zgraph_worker/tool_runner.py`、`python/tests/test_tool_runner.py`、`tests/fixtures/workers/tools/tools_ok/tools/slow.py`、`.../builtin.py`。分支整体 base 仍为 `887743a`（未 rebase；评审时 origin/main `389b277`，分支 ahead 2 / behind 3）。
- 提交：`3fca90e`，author/committer `chenyuxiangg <1007423165@qq.com>`，subject `feat(py-worker): 工具声明支持可选 timeout_ms（REV-019 增量）`，正文含 `Review: REV-019` / `Agent: py-worker-dev`。

### 7.1 四维评审（仅增量）

| 维度 | 结论 | 依据（文件/行/测试） |
|---|---|---|
| 功能正确性 | ✅ | ① 常量 `MIN_TOOL_TIMEOUT_MS=1`/`MAX_TOOL_TIMEOUT_MS=600_000`（`tool_runner.py:27-28`）；新增 `_timeout_ms_value`（`:123-139`）：缺键 → `None`、**显式拒绝 `bool`**（`isinstance(x, bool)` 先于 int 判断，规避 Python `bool⊂int` 陷阱）、非 int → `invalid_declaration`、超界 → `invalid_declaration`。② `_validate_tool` 仅在合法且存在时把 `timeout_ms` 加入返回 dict（`:175-183`），缺省不带；`discover` 输出因此"存在才带"（docstring 同步 `:268`）。③ `reason` 闭表**未扩**（仍 6 个 `ERROR_*` 常量，`grep` 实核 `:33-38`），非法 timeout 复用 `invalid_declaration`。④ fixtures：`builtin.add`（形式 A）`timeout_ms=30000`（`:22` 附近）、`slow.sleepy`（形式 B）`timeout_ms=300`；本人独立子进程验证透传 `add=30000 / sleepy=300`、`now/echo` 均无该键。⑤ 9 条新测试：两形式透传（`:199`）、缺省键不存在、`0/-1/600001/True/"300"/1.5/None` 7 个非法值参数化（`:210`）、边界 `1`/`600000` 接受（`:231`）；`pytest python/tests` **84 passed / 0 failed**（原 75 + 9）。 |
| 代码结构合理性 | ✅ | 增量集中：校验逻辑抽成单一职责 helper `_timeout_ms_value`，`_validate_tool` 末尾组合；常量置文件头常量区；未改 `call()`/`discover` 主流程（仅补 docstring），未新增文件/依赖；4 文件全在 py-worker 边界内，无越界（fixtures 仍在 `tests/fixtures/workers/**`）。 |
| 逻辑性 | ✅ | ① "缺省"与"存在但非法"语义严格区分（`"timeout_ms" not in raw` vs 取值后校验）；② 校验顺序 type → range，错误 location 沿用 `module[index]`，与既有声明错误一致；③ 新增可选键不改变 discover 的顺序/去重逻辑（timeout 只挂在工具项上）；④ 未扩 reason 闭表，harness 侧错误分支无需改动；⑤ `_timeout_ms_value` 无副作用、无缓存，可重复调用。 |
| 可读性 | ✅ | 常量命名与量纲清晰（`_MS`）；helper docstring 说明"缺省返回 None、非法 → invalid_declaration"；discover docstring 补可选键说明；测试参数化带 `ids`（zero/negative/too_large/bool/string/float/null）；AST 实测 `tool_runner.py` 最长函数 **44 行**、测试文件 28 行（≤60）；双引号/类型注解/snake_case 保持。 |

### 7.2 增量验证证据（worktree 内）

```bash
cd /Users/zyzs/lczyzs/code/zgraph/.worktrees/py-worker-py-worker-dev
git status --short                        # 空（clean）
git diff --shortstat e49d2a1..3fca90e     # 4 files changed, 82 insertions(+), 4 deletions(-)
git diff --name-status e49d2a1..3fca90e   # M tool_runner.py / test_tool_runner.py / fixtures slow.py / fixtures builtin.py
git log -1 --format='%H|%an <%ae>|%cn <%ce>|%s' 3fca90e
# 3fca90eeff14a2c4a703c67666412a859e67504f|chenyuxiangg <1007423165@qq.com>|chenyuxiangg <1007423165@qq.com>|feat(py-worker): 工具声明支持可选 timeout_ms（REV-019 增量）
```

| 命令 | 结果 |
|---|---|
| `/Users/zyzs/lczyzs/code/zgraph/.venv/bin/python -m pytest python/tests` | **84 passed / 0 failed**（原 75 + 新增 9；0 failed） |
| `bash scripts/dev/check.sh` | **CHECK PASSED**（12 项 OK） |
| `grep "^ERROR_" python/zgraph_worker/tool_runner.py` | 仍为 6 个常量（reason 闭表未扩） |
| AST 函数长度 | `tool_runner.py` max **44 行** / 20 函数；`test_tool_runner.py` max 28 行 / 42 函数（≤60） |
| `git status --short`（全部命令后） | 空 |

**独立手工验证（仓库测试之外，真实 `python -m zgraph_worker.runtime` 子进程）**：

| 用例 | 帧序列 / exit | 输出 |
|---|---|---|
| discover `tools_ok/tools` | `['result']` / 0 | `add.timeout_ms=30000`、`sleepy.timeout_ms=300`；`now`/`echo` **无** `timeout_ms` 键 |
| discover 声明 `timeout_ms=True`（bool） | `['result']` / 0 | `ok=false, reason=invalid_declaration` |
| discover 声明 `timeout_ms=600001` / `1.5` / `None` | `['result']` / 0 | 均 `reason=invalid_declaration` |
| discover 声明 `timeout_ms=1` / `600000` | `['result']` / 0 | `ok=true` 且原值透传（边界包含） |
| discover bad_import / 缺失目录（回归原错误路径） | `['result']` / 0 | 仍 `tool_module_import` / `tools_dir_missing`（未被增量破坏） |

### 7.3 增量发现

| # | 级别 | 位置 | 问题 | 建议 | 状态 |
|---|---|---|---|---|---|
| 1 | 建议（跨模块 REV-020） | `python/zgraph_worker/tool_runner.py:27-28`、spec 05 §5.6.1/§5.6.5 | Python 侧允许声明到 `600000` ms 并原样透传；但 spec 规定执行时 `min(ToolDef.timeout_ms, DEFAULT_TOOL_TIMEOUT_MS=30000)`，且还要与剩余实例 deadline 取 min。harness 必须做这两层 cap，否则工具超时会超出 spec。 | REV-020 评审时核对 `WorkerToolRunner` 的超时计算为 `min(declared, 30000, node_deadline_remaining)` 并有测试；本分支不需要改。 | open |
| 2 | 建议（契约同步） | `discover` 成功项形状（原 4 键 → 可选第 5 键 `timeout_ms`） | task-3 原契约写的是每项 `{name,description,parameters,source}`；本次向后兼容扩展了可选键。若 harness/文档把返回项当严格 4 键解析会失败。 | 由 Lead/harness 在 REV-020/REV-022 契约中明确"工具项可选 `timeout_ms`，harness 缺省补 `DEFAULT_TOOL_TIMEOUT_MS`"；不阻塞本分支。 | open |
| 3 | 备注 | `python/tests/test_tool_runner.py:194-196` | 原 `set(tool) == {4 键}` 改为「必须包含 4 键 + 只允许 5 键上界」。这是对输出向后兼容扩展的**正当适配**，未放松关键约束（仍有键集合上界），非"改测试放水"。 | 无需处理。 | 无需处理 |
| 4 | 备注 | 增量 diff | 原 §3 #1–#4 建议（call 不校验 name、sys.path 单次进程假设、reason 语义、tools_dir 命名前提）本次**未涉及/未变**，仍有效。 | 保持 §5 遗留跟踪。 | 无需处理 |

### 7.4 增量结论与可合并性

- **增量复审：通过**：可选 `timeout_ms` 的校验/透传/缺省语义与 Lead 需求逐项一致，`bool` 陷阱处理正确，reason 闭表未扩，fixtures 与 9 条测试均真实有效；84 passed / check.sh PASS / 独立子进程验证全绿；无阻塞/重要项。
- **全分支 REV-019 结论维持「通过」**；#1/#2 为 REV-020/契约同步项（非合并门禁），#3/#4 为备注。
- 可合并性：分支 ahead 2 / behind 3（origin/main `389b277`），`merge.sh` 会 rebase 后重跑全量；原 §5 遗留项继续有效。
