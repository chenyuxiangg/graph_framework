# REV-068 · py-worker · P v2 parse_verdict_rework

- **分支**：`feat/py-worker-parse-rework`
- **作者**：`py-worker-writer`（提交 `137d68d`；正文含 `Review: REV-068` / `Agent: py-worker-writer`）
- **评审人**：`reviewer-a`（**≠ 作者**；独立评审）
- **变更范围**：`7f88614..137d68d`，2 文件 **+254/−9**（全部 `py-worker` 模块：`python/workers/software_team/parse_verdict.py` +64/−7、`python/tests/test_parse_verdict.py` +190/−2）；未改 kernel/dsl/trace/harness/e2e/docs/CLAUDE.md
- **评审时间**：2026-10-31
- **关联**：DR-004 §3 W2a0 / §4 W2a（`doc/decisions/DR-004-v02-scope-and-waves.md:63,77`）；`doc/issues.md` ISSUE-101；REV-064（v1 `parse_verdict` 基线）；spec v1.2 04 §4.3.7（L3 幂等/副作用四标记）；CLAUDE.md（Python 规范）

## 1. 结论

**结论：通过**

W2a0 冻结的 P v2 契约逐项落实且经独立复核：`parse_verdict_rework(inputs, ctx)` 接受 `review`（object，`content` 为合法 JSON 字符串并解析为 object）+ `artifact`（any），输出恰 3 键 `{verdict, comments, artifact}`；`verdict` 闭集 `{approved, rework}` 且大小写敏感；`comments` 缺省 `""`、显式非 str 拒绝；`artifact` 按对象 identity 原样透传；**`rework` 是正常输出而非异常**（实现与测试双侧验证）；8 个非法输入分支全部 `ValueError` 且 message 前缀均为 `parse_verdict_rework: `（worker 侧经 `runtime.py` 通用 `except Exception` 映射 `NODE_PYTHON_EXCEPTION`）。L3 四标记齐全、纯函数/幂等/无副作用，函数 48 行（≤60）。

**v1 `run` 行为未变**：独立 AST 去 docstring 后函数体源码逐字节一致，`co_code`/`co_names`/`co_consts[1:]`/`co_varnames` 全部一致（仅 docstring 与行号变化）；v1 仍拒绝 `rework`。Python 源文件不参与 `graph_hash`，故与图 hash 无关。ISSUE-101 要求在本分支补齐的 v1 docstring 异常清单与边界用例已落实。

验证：`test_parse_verdict.py` **86 passed**（base `7f88614` 同一文件 20 passed，+66 用例）、全量 **170 passed**（基线失败名额 0，无新增失败）、`scripts/dev/check.sh` **CHECK PASSED**。仅 3 条建议（S1–S3，见 §3），均不阻塞；无未关闭条件。

## 2. 四维评审

| 维度 | 结论 | 依据（文件/行/测试） |
|---|---|---|
| 功能正确性 | ✅ | `parse_verdict.py:69-116` 按契约实现：输入存在性→`review` object→`content` 非空 str→JSON 解析→解析为 object→`verdict` 闭集（`isinstance` + `VERDICTS`，`REWORK`/`APPROVED` 大小写敏感）→`comments` 类型→输出恰 3 键；`rework` 正常返回（`test_rework_verdict_is_normal_output`；独立探针 `{'verdict': 'rework', ...}`）；`comments` 缺省 `""`（`test_rework_comments_default_to_empty_string`）、显式保留（`test_rework_comments_are_preserved_verbatim`）、非 str 双 verdict 拒绝（`test_rework_non_string_comments_raises`）；`artifact is` + `==`（`test_rework_artifact_is_passed_through_by_identity`）；8 个 raise 分支均有前缀 `"parse_verdict_rework: "` + 具体 fragment 断言（`_assert_rework_error`）；v1 逐字节未变（AST/字节码对比），`rework` 仍被 v1 拒绝（`test_non_approved_verdict_raises` 含 `"rework"`，独立探针 `parse_verdict: review verdict is not approved: 'rework'`）；ISSUE-101 的 v1 docstring 异常清单补全 + content 缺失/纯空白/非 string/解析非 object 边界用例 + artifact `==` 断言均落实（`test_content_key_missing_raises`/`test_blank_content_raises`/`test_content_non_string_raises`/`test_parsed_content_must_be_object`） |
| 代码结构合理性 | ✅（S1 备注） | 2 文件全部在 `py-worker` 模块路径（`python/workers/**`、`python/tests/**`），未越界、未改冻结文件；新函数与 v1 `run` 独立、常量 `REWORK`/`VERDICTS` 复用既有 `NOW` 命名风格（全大写）；v1/v2 校验分支刻意重复约 24 行，作者在模块/函数 docstring（`:7-8`、`:72-73`）明确说明「v1 已评审合并后冻结、不改其异常前缀与判定顺序」——该取舍在 v1 冻结前提下**可接受**，抽 helper 会改 v1，故不作为本分支合并条件（见 S1）；`VERDICTS` 为 `frozenset` 不可变（`test_verdicts_is_closed_immutable_set`；独立探针 `.add` → `AttributeError`） |
| 逻辑性 | ✅ | 校验顺序与 v1 一致（输入存在→类型→content→JSON→object→verdict→comments），无吞异常；`json.JSONDecodeError` 链式转 `ValueError`（`from exc` 保留原因）；先 `isinstance(verdict, str)` 再查 `frozenset`，避免不可哈希值（测试 `["rework"]`）抛出 `TypeError` 而破坏「非法输入一律 ValueError」；无循环/竞态/全局状态；异常由 `runtime.py:120-123` 通用 `except Exception` 映射 `NODE_PYTHON_EXCEPTION`；纯函数，同一输入输出相等（独立探针 `idempotent equal: True`） |
| 可读性 | ✅（S3 备注） | 模块/两函数 docstring 中文清晰，L3 四标记齐全（输入/输出/幂等性/外部副作用），异常段逐分支列全并注明大小写敏感；类型注解齐全、双引号、snake_case、常量全大写；函数行数 `run` 41、`parse_verdict_rework` 48（≤60），测试函数最长 6 行；测试按「v1 保持 / v2 正常输出 / v2 异常矩阵」分组命名清晰；仅测试前缀常量缺结尾空格（S3，非阻塞） |

## 3. 发现清单

| # | 级别 | 位置 | 问题 | 建议 | 状态 |
|---|---|---|---|---|---|
| S1 | 建议 | `python/workers/software_team/parse_verdict.py:92-116` vs `:44-66` | v2 复制 v1 的输入校验/JSON 解析分支（约 24 行），仅 verdict 判定与异常前缀不同。作者已在 docstring 说明「v1 冻结、刻意重复」（可接受），但两副本未来可能单边修复而漂移（如修 v2 漏 v1、或反之） | 保持现状可接受，**建议登记 ISSUE 跟踪**；或在 v1 可解冻时抽共用 helper（会改 v1，不作为本分支条件）；可另加一条「同一批非法输入对 v1/v2 均抛 `ValueError`、前缀各为其一」的对照测试防漂移 | open（建议，非阻塞） |
| S2 | 建议 | `python/tests/test_workers.py:29,47,59` | `parse_verdict_rework` 未登记进 `WORKER_MODULES`/`WORKER_CALLABLES`，spec 04 §4.3.7 的 L3 四标记 + 两参数模块级签名守卫不会覆盖该新 worker 函数（本评审已人工确认四标记齐全，但无自动守卫；W2a e2e 将由图引用该函数） | 在 W2a e2e 接入图时把 `("workers.software_team.parse_verdict", "parse_verdict_rework", parse_verdict_rework)` 加入 `WORKER_CALLABLES` 并补 import；或本分支顺手补。**建议登记 ISSUE/并入 e2e 批次** | open（建议，非阻塞） |
| S3 | 建议 | `python/tests/test_parse_verdict.py:19,38-43` | 测试常量 `ERROR_PREFIX = "parse_verdict_rework:"` 无结尾空格，而契约前缀为 `parse_verdict_rework: `（带空格）；若实现误删空格，`startswith` 仍通过（当前实现 8 个 raise 均含 `": "`，仅测试断言略松） | 常量改为 `"parse_verdict_rework: "`，收紧前缀断言 | open（建议，非阻塞） |

> 级别口径：阻塞 = 必须修复后才能给"通过"；重要 = 必须修复或登记 ISSUE；建议 = 可后置，但需修复或进台账。

## 4. 验证证据

```bash
# --- 范围/干净度 ---
cd /Users/zyzs/lczyzs/code/zgraph/.worktrees/py-worker-py-worker-writer
git status --porcelain --branch      # ## feat/py-worker-parse-rework...origin/main [ahead 1]；工作树 clean
git log --oneline -2
# 137d68d feat(py-worker): 新增 P v2 parse_verdict_rework（W2a 有限返工）
# 7f88614 docs(process): W1 完成收口——进度、台账（ISSUE-101..103）
git rev-parse HEAD origin/main
# 137d68d32c4c3c0a482cdd1430eadaf7e660d234 / 7f886141890a7d6577ec28d0b79b1aaec4a8bb2d
git diff 7f88614..HEAD --name-status
# M  python/tests/test_parse_verdict.py
# M  python/workers/software_team/parse_verdict.py
git diff 7f88614..HEAD --shortstat  # 2 files changed, 254 insertions(+), 9 deletions(-)
git show --stat 137d68d             # 同上；提交正文含 Review: REV-068 / Agent: py-worker-writer

# --- 仓库自检 ---
bash scripts/dev/check.sh
# OK 分支命名: feat/py-worker-parse-rework 等 12 项全 OK
# CHECK PASSED  [exit 0]

# --- pytest（主仓 venv）---
/Users/zyzs/lczyzs/code/zgraph/.venv/bin/python -m pytest python/tests/test_parse_verdict.py
# 86 passed in 0.03s
/Users/zyzs/lczyzs/code/zgraph/.venv/bin/python -m pytest
# 170 passed in 1.74s
# 对照：doc/test-baseline.txt 失败名额 0 → 无新增失败；base 版本 test_parse_verdict.py 临时副本运行 20 passed → 新增 66 用例

# --- v1 未变（独立 AST/字节码对比，去 docstring）---
# v1 body source identical: True
# run co_code equal: True
# run co_names equal: True
# run co_consts[1:] equal: True | co_consts[0] docstring changed: True
# run co_varnames/argcount equal: True

# --- L3 / 行数 / 前缀抽查 ---
# rework doc markers 输入：/输出：/幂等性：/外部副作用： = True；副作用声明以「无」开头
# 函数行数：run 41、parse_verdict_rework 48（≤60）；测试函数最长 6 行
# 8 个 raise 经 AST 逐条检查：prefix_ok=True（全部 "parse_verdict_rework: "，含空格）

# --- 独立调用探针（不读作者测试）---
# rework normal: {'verdict': 'rework', 'comments': 'fix it', 'artifact': {'k': 1}}
# keys: ['verdict', 'comments', 'artifact'] artifact is None: True
# 8 类非法输入（缺 review/缺 artifact/review 非 dict/content 缺失/content 非 str/纯空白/非法 JSON/非 object/未知 verdict/大小写变体/comments 非 str）
#   → 全部 ValueError，prefix_ok=True
# v1 rejects rework: parse_verdict: review verdict is not approved: 'rework'
# idempotent equal: True
# VERDICTS: frozenset({'rework', 'approved'})；.add → AttributeError（不可变）
```

- 测试结果：指定 **86 passed / 0 failed**；全量 **170 passed / 0 failed**（`doc/test-baseline.txt` 失败基线 0，无新增失败）。
- 手工验证：v1 `run` 函数体源码/字节码与 base 一致（仅 docstring 变化），`rework` 仍被 v1 拒绝；v2 `rework` 正常输出、闭集大小写敏感、恰 3 键、`comments` 缺省/显式、`artifact` identity；`VERDICTS` 不可变；8 个异常分支前缀一致。
- 未发现恒真/削弱断言：base 测试全部保留，新增仅追加断言（v1 artifact 补 `==`、v2 `is`+`==`、`set(output)` 恰 3 键、异常分支前缀+fragment）。
- 本评审未改产品代码/测试；评审记录与索引行属 `docs` 模块（按 AGENTS §3/§6 落在本分支，作者 py-worker-writer、评审人 reviewer-a ≠ 作者）。

## 5. 遗留问题

- **S1–S3 为建议（非阻塞）**：建议由 Lead 登记 `doc/issues.md`，或随 W2a e2e 批次/后续 py-worker 触碰落实；本评审按授权只改 `doc/reviews/**`，未改产品/测试/台账。
- **ISSUE-101 已在本分支落实**（v1 docstring 异常清单补全、content 缺失/纯空白/非 string/解析非 object 边界用例、artifact `==` 断言）；台账状态更新由 Lead 负责（不在本评审范围）。
- **W2a e2e 不在本分支范围**：`test/e2e-gate-defer` 消费 `parse_verdict_rework` 的端到端返工链路（rework 正常输出 + defer 回边/attempt）属后续分支；本分支仅冻结 P v2 单元契约。
- 未发现越界改动、禁用文件、冻结文件修改；分支 base = `origin/main`（`7f88614`）；worktree clean；本评审未 merge/push/切 main。

## 6. 条件关闭记录（仅"有条件通过"填写）

不适用（本记录结论为"通过"，无未关闭条件；S1–S3 为非阻塞建议）。

## 7. 追加复审（S2/S3 关闭，2026-10-31）

> 编号说明：本记录 §6 已按模板保留为「条件关闭记录」，追加复审按 REV-065 §7 先例顺延为 §7；仅追加、不改原记录。

- **范围**：`119437d..50eb1f7`，1 提交 `50eb1f7`（`test(py-worker): 补 parse_verdict_rework L3 守卫与错误前缀断言（REV-068 S2/S3）`，正文含 `Review: REV-068` / `Agent: py-worker-writer`），2 文件 **+10/−2**，仅测试：`python/tests/test_workers.py`（+9/−1）、`python/tests/test_parse_verdict.py`（+1/−1）；`git diff 137d68d..50eb1f7 -- python/workers/` 为空，**未改产品代码**。
- **S2 关闭（守卫覆盖）**：`test_workers.py:29-32` 补 `parse_verdict_rework` import，`:63-67` 登记进 `WORKER_CALLABLES`，条目数 **8→9**；既有 `test_worker_functions_are_module_level_sync_with_two_params` 与 `test_worker_docstrings_declare_l3_contract` 循环自动覆盖新函数。独立复核（复用同一守卫逻辑）：L3 四标记（输入/输出/幂等性/外部副作用）齐全、副作用声明以「无」开头、模块级同步函数、恰 2 参数、登记对象与模块属性同一，全部通过；**负向对照**——缺标记 docstring 与 1 参数函数分别被 L3/签名守卫判为不通过，证明守卫非恒真。
- **S3 关闭（前缀收紧）**：`test_parse_verdict.py:19` 的 `ERROR_PREFIX` 改为 `"parse_verdict_rework: "`（含结尾空格），`_assert_rework_error`（`:38-43`）的 `startswith` 现要求契约前缀精确匹配；13 处调用覆盖 8 个 raise 分支。独立探针：8 个非法输入分支的 message 全部 `startswith("parse_verdict_rework: ") == True`。
- **验证**：`git status --porcelain --branch` → `## feat/py-worker-parse-rework...origin/main [ahead 3]`（clean）；pytest `test_parse_verdict.py + test_workers.py` → **110 passed**（86 + 24）；全量 → **170 passed**（基线失败名额 0，无新增失败）；`bash scripts/dev/check.sh` → **CHECK PASSED**（exit 0）。
- **结论**：**S2/S3 关闭**；S1（v1/v2 校验分支重复）保留为**建议/台账项**，不阻塞。REV-068 结论维持 **通过**；本追加记录提交正文含 `Review: REV-068` / `Agent: reviewer-a`，且只改 `doc/reviews/**`（未改产品/测试）。
