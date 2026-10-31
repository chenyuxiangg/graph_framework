# REV-076 · e2e · W2b max_fanout 静态反例 / 正例真链路 / 运行期防御层

> **跨模块消费声明**：本分支只改 `tests/e2e/**`（`e2e` 模块），**只读消费** W2b 已合入的 `tests/fixtures/graphs/{valid,bad_}_max_fanout*.json`（`dsl`/`e2e` 共享 fixture，REV-074 交付）与 kernel 运行期层（REV-075 交付）；未改 dsl/kernel/cli/trace 源码、未改 `tests/fixtures/**`、未改既有 e2e 用例断言。评审人只新增本评审记录，未修改产品/测试代码。

- **分支**：`test/e2e-max-fanout`
- 分支: test/e2e-max-fanout
- **作者**：`e2e-dev`（提交 `06d6945`；`Review: REV-076` / `Agent: e2e-dev`）
- **评审人**：`reviewer-a`（独立评审，**≠ 作者**）
- **变更范围**：本地 main `a4cb1b1..06d6945`（2 files, +509/−1）：`tests/e2e/Cargo.toml`（dev-dep `zgraph-kernel` 启用 `test-hooks` + 新增 `[[test]] max_fanout`）、新增 `tests/e2e/max_fanout.rs`（501 行，6 个用例）。`a4cb1b1` = origin/main `7b4aba1` + REV-074（dsl）+ REV-075（kernel），origin/main 为分支祖先；`Cargo.lock` 未变。
- **评审时间**：2026-10-31
- **关联**：Lead 合同 task-1 §C（C1–C5）、REV-074（R16 静态层 / fixtures）、REV-075（运行期防御层 / test-hooks）、DR-004 §3 W2b / §5 D2、DR-005 §3.3（"不产生 GateResult"同序口径）、spec 03 / 07 AC-15（正文由 docs 分支 REV-077 落盘）

## 1. 结论

**结论：通过**

按合同 §C1–C5 逐项核对全部成立；未发现 P0/P1/P2 问题，P3 建议 4 条（3 条为断言/可读性小加固，1 条为真 run 侧 hash 闭环），不阻塞合并。未发现放宽/删除既有断言、未新增 `#[ignore]`、未 mock 真链路、未越界改动。

### 1.1 合同覆盖对照

| 合同 | 实现在 | 评审结论 |
|---|---|---|
| **C1** 静态层 CLI 反例/正例 | `bad_max_fanout_exceeded_reports_r16_fanout_error`（:123）、`bad_max_fanout_value_reports_must_be_positive`（:150）、`valid_max_fanout_passes_validate_with_frozen_hash`（:173） | ✅ 真 `target/debug/zgraph validate --json`、退出码 2/2/0；断言 `ok`/`code=DSL_PARSE_ERROR`/`location.node_id=start`（且无 `edge_index`/`port`）/`error_count=1`/`truncated=false`/`expected`/`actual`/`message`，失败图无 `graph_hash`；合法图 `ok=true` + 16-hex `graph_hash` 并冻结为 `e05d882cc55aa275`（与 REV-074 dsl golden 一致） |
| **C2** 正例真链路 | `valid_max_fanout_runs_to_completion_through_real_cli`（:285）+ `assert_completed_trace`（:221） | ✅ 真 `zgraph run`（`cli_support` 解析到的真二进制 + `--python`/`--python-path python/` 真 Python worker 子进程 + 真 SQLite trace）：`status=Completed`、`run_output` 与 trace `runs.output_json` 双向一致、`assert_lifecycle_shape`（RunStarted/RunFinished 各一次、seq 严格递增）、`start/a/b/done` 实例 Completed、`a/b` 输出为真 worker 标记且 `attempt=0`、无 Crashed 实例 |
| **C3** 运行期防御层 | `runtime_fanout_exceeded_via_test_hooks_fails_run_and_crashes_source`（:371）、`runtime_fanout_at_limit_completes_as_boundary_case`（:478） | ✅ 走 `common::run_to_trace` → cli `bridge::run_in_process`（真 `TraceWriter` + 真 kernel + 真 `WorkerExecutor` SQLite trace），非 mock；`force=1`：run `Failed`、`outcome.error.code/message` 冻结、`runs.error_code=NODE_FANOUT_EXCEEDED`、`runs.error_message`、`RunFinished.error.code/message`、Crashed 实例 `attempt=1`（源实例 attempt=0 Completed）、`error` JSON `kind="FanoutExceeded"`/`limit=1`/`actual=2`/`message` Display、**该激活无任何 GateResult**、无下游实例；`force=2`：`Completed` + 完整 trace 断言（actual==limit 边界） |
| **C4** 回归 | 既有 13 个 e2e target | ✅ 既有 e2e 文件与断言零改动；`cargo test -p zgraph-e2e` = 64 passed / 0 failed / 2 ignored（2 个既有 manual `#[ignore]`：`harness_ac08::ac08b_real_llm_manual`、`software_team_v1::real_llm_smoke_manual`），**未新增 ignore**；全量 workspace 725 passed / 0 failed / 3 ignored，无新增失败 |
| **C5** 范围 | diff | ✅ 仅 `tests/e2e/Cargo.toml` + `tests/e2e/max_fanout.rs`；`Cargo.lock` 未变；未动 kernel/dsl/cli/trace |

### 1.2 dev-dep `test-hooks` 影响面（Lead 重点）

- 该 feature 只加在 `tests/e2e/Cargo.toml` 的 **`[dev-dependencies]`**，不会进入产品构建；作用仅让 e2e 测试目标能构造 `RunOptions.test_hooks.force_max_fanout`。
- **不引入新的 workspace 特性合并变化**：`crates/zgraph-cli/Cargo.toml:29` 的 kernel dev-dep 早已是 `features = ["test-hooks"]`（既有同模式），因此 `cargo test --workspace` 本来就会把 kernel 编入该 feature；本分支只是让 `cargo test -p zgraph-e2e` 单包隔离运行时也能编译 `TestHooks`。Cargo.toml 注释表述准确。
- 行为不变证据：`TestHooks::default()` 无任何钩子（kernel `hook_values` 非 `test-hooks` 构建恒默认值、`force_max_fanout=None`）；既有 e2e target 全部通过（`gate_defer` 8、`cli_replay` 9、`software_team_v1` 11+1 ignored、`harness_ac08` 5+1 ignored、`parallel`/`ac12`/`cancel`/`concurrency`/`cli_ac01`/`fault_injection`/`timeout_kill` 全绿）；workspace 中因特性合并而被启用的 kernel `tests/max_fanout.rs`（6）与 `tests/test_hooks.rs`（3）也全部通过。未观察到任何既有输出/golden 变化。
- C2 使用的"真 CLI"是 merge 门禁先 `cargo build -p zgraph-cli --bin zgraph --locked` 生成的生产二进制（无 test-hooks）；评审人重建后复跑 `--test max_fanout` 仍 6/6 通过，排除"仅对测试构建成立"的伪证据。

## 2. 四维评审

| 维度 | 结论 | 依据（文件/行/测试） |
|---|---|---|
| 功能正确性 | ✅ | C1/C2/C3 全部真实覆盖（§1.1）；断言精确到 `code`/`location`/`error_count`/`expected`/`actual`/`message`/退出码、run 行 `status`/`error_code`/`error_message`、RunFinished payload、实例 `status`/`attempt`/`error_json`、GateResult 是否存在；REV-074/REV-075 冻结模板逐字对齐（`node 'start' out-degree 2 …`、`node 'start' fan-out 2 exceeded max_fanout (1) in a single activation`、Display `node fan-out 2 exceeded max_fanout (1)`）；手工真 CLI 复核与用例断言一致 |
| 代码结构合理性 | ✅ | 新文件 + 声明 `[[test]]` 的改动方式与既有 13 个 target 一致；复用 `cli_support`（真二进制发现）与 `common`（compile/`run_to_trace`/trace 查询）公共层，无重复实现、无 mock、无私有假执行器；C2 与 C3 共享 `assert_completed_trace` 避免重复；dev-dep 注释解释"为什么"（防御性校验的可达性） |
| 逻辑性 | ✅ | 每个用例独立 `TempDir`/trace DB，无跨用例共享状态；C3-a 与 kernel B2 语义严格一致（在应用任何边之前拦截 → 无 GateResult、无下游实例；Crashed attempt = 源 + 1）；断言链自洽：`outcome` ↔ `runs` 行 ↔ `RunFinished` 三处一致，实例数与状态、`gates.is_empty()` 互证；无 sleep 猜测（复用 `RUN_TIMEOUT`/真实链路等待）；C3-b 与 C2 复用同一完成态断言，边界语义正确 |
| 可读性 | ✅（N2 备注） | 模块级 doc 说明三部分、真链路/禁 mock、C3 可达性；常量集中（fixture 名/hash/force 值/attempt/冻结 message）并有注释；测试名表达意图；断言均带上下文消息。N2 为 C3-a 函数较长（~103 行、断言密集）可拆 helper，非阻塞 |

## 3. 发现清单

> 级别口径：P0 = 阻塞；P1 = 重要（必须修复或登记 ISSUE）；P2 = 一般；P3 = 建议（可后置，应进台账）。本分支 P0=0、P1=0、P2=0。

| # | 级别 | 位置 | 问题 | 建议 | 状态 |
|---|---|---|---|---|---|
| 1 | P3 建议 | `tests/e2e/max_fanout.rs:156-163` | C1 取值非法用例的 message 用 `contains("'max_fanout' must be >= 1")` 部分匹配，未锁定冻结模板完整串 `"'max_fanout' must be >= 1, got 0"`；相邻 R16 超限用例与 dsl 单测均为精确断言，口径略不一致 | 改为 `assert_eq!(message, "'max_fanout' must be >= 1, got 0")`（C1 合同未要求，属加固） | open（建议项） |
| 2 | P3 建议 | `tests/e2e/max_fanout.rs:371-474` | C3-a 单个测试函数约 103 行、断言密集（outcome/trace/实例/error JSON/GateResult 五组），可读性与失败定位成本偏高 | 抽 `assert_fanout_failed_trace(...)` 等 helper（与 `assert_completed_trace` 对称），仅移动断言不改语义 | open（建议项） |
| 3 | P3 建议 | `tests/e2e/max_fanout.rs:478-499` | C3-b 边界用 `force=2` 且节点声明 `max_fanout=2`，`min(2,2)=2`，因此即使 hook 被忽略也仍 Completed——该用例本身无法区分"hook 生效"与"无 hook"；hook plumbing 已由 C3-a（force=1）证明 | 在注释中显式说明该用例只锁"actual==limit 不失败"边界、hook 生效证据在 C3-a，避免后续误读 | open（建议项） |
| 4 | P3 建议 | `tests/e2e/max_fanout.rs:221-325` | C2 真 run 链路未断言 trace `runs.graph_hash == "e05d882cc55aa275"`（`runs` 表有该列），C1 的 hash 冻结与 C2 的真 run 尚未在存储侧闭环 | 可选：在 `assert_completed_trace` 增加 `run.graph_hash` 断言，把"同一图同 hash"纳入真链路证据 | open（建议项） |
| 5 | P3 说明 | `tests/e2e/Cargo.toml:20-23` | dev-dep 启用 kernel `test-hooks` 属合同 C3 明确要求；已核实与 `crates/zgraph-cli/Cargo.toml:29` 既有 dev-dep 同模式，workspace 特性合并口径未因本分支变化（测试构建专用、产品构建不受影响），全量测试无失败 | 无需动作；合并时 Lead 知悉即可 | open（声明） |

## 4. 验证证据

评审人**亲自执行**（worktree `/Users/zyzs/lczyzs/code/zgraph/.worktrees/e2e-e2e-dev`，HEAD `06d6945`，基于本地 main `a4cb1b1`，工作区干净）：

```bash
export CARGO_HOME=/Users/zyzs/lczyzs/code/zgraph/.cargo-home
cargo test -p zgraph-e2e
# 13 个 target：declared=66，passed=64，failed=0，ignored=2（既有 manual）
#   ac12 2 / cancel 2 / cli_ac01 4 / cli_replay 9 / concurrency 4 / fault_injection 7
#   gate_defer 8 / harness_ac08 5+1 / max_fanout 6 / parallel 4 / software_team_v1 11+1
#   timeout_kill 2
cargo build -p zgraph-cli --bin zgraph --locked   # 生产 CLI（无 test-hooks），fresh
cargo test -p zgraph-e2e --test max_fanout        # 6 passed / 0 failed（2.18s，重跑确认）
cargo test --workspace                            # 725 passed / 0 failed / 3 ignored
  # 其中 kernel 因 test-hooks 特性合并启用的 tests/max_fanout.rs（6）与 tests/test_hooks.rs（3）全过
cargo fmt --all -- --check                        # exit 0
cargo clippy --workspace --all-targets --locked -- -D warnings   # clean（exit 0）
bash scripts/dev/check.sh                         # CHECK PASSED
/Users/zyzs/lczyzs/code/zgraph/.venv/bin/python -m pytest -q     # 170 passed（rc=0）
```

**范围与基线**：

```bash
git diff --stat a4cb1b1...HEAD     # 2 files changed, 509 insertions(+), 1 deletion(-)
git diff a4cb1b1...HEAD -- Cargo.lock   # 空（Cargo.lock 未变）
git status --porcelain                  # 空（干净）
git merge-base --is-ancestor origin/main HEAD   # 成立（origin/main 7b4aba1 为祖先）
```

**真链路证据（评审人手跑真 CLI，与用例断言交叉核对）**：

```text
bad_max_fanout_exceeded.json --json
  → exit 2, {ok:false, error_count:1, errors[0]={code:"DSL_PARSE_ERROR",
    message:"node 'start' out-degree 2 exceeds max_fanout 1",
    location:{node_id:"start"}, expected:"out-degree <= 1", actual:"out-degree 2"}}
bad_max_fanout_value.json --json
  → exit 2, message "'max_fanout' must be >= 1, got 0", expected ">= 1", actual 0, node_id start
valid_max_fanout.json --json
  → exit 0, ok:true, graph_hash:"e05d882cc55aa275"
```

- C2 断言 `a`/`b` 实例输出 `{"worker":"a"|"b","x":{}}` 来自 `python/workers/parallel.py` 真 worker 子进程（fixture 模块 `workers.parallel`），CLI 以 `--python`/`--python-path python/` 启动，无法由 mock 产生。
- C3 断言与 kernel 源码一致：`reject_fanout_exceeded` 在 `apply_publish` **应用任何边之前**返回（`crates/zgraph-kernel/src/run.rs:1073-1084`），因此"该激活无 GateResult"是结构保证而非偶然；Crashed `attempt = 源 attempt + 1`、`error JSON kind/limit/actual` 与 `node_error_json` 机械 arm（`crates/zgraph-cli/src/bridge.rs:739-743`）一致。
- 提交信息含 `Review: REV-076` / `Agent: e2e-dev`；`check.sh` 身份/冻结文件/分支名/禁用文件校验通过；未改 `CLAUDE.md`。
- 测试结果：**无新增失败**（Rust workspace 0 failed；pytest 0 failed）；与 `doc/test-baseline.txt` 比对无新增失败项。

## 5. 遗留问题

- **P3 建议未登记 `doc/issues.md`**（本分支合同 §C5 限定只改 `tests/e2e/**`，台账由 docs 分支/Lead 维护）：N1–N4 建议由 Lead 决定是否在 `doc/issues.md` 登记；N5 仅为声明，无台账动作。
- **spec 落盘待 docs 分支**：`doc/spec-v1.2/03` 运行期上限语义与 `07` AC-15（a/b/c/d、测试文件/用例名/实际计数）由 W2b docs 分支（REV-077）负责；本分支 6 个用例与 "64 passed / 2 ignored" 计数可直接作为 AC-15 证据。
- 两个既有 manual `#[ignore]`（真实 LLM）保持不变、未新增 ignore；C3 运行期反例依赖 `test-hooks` 构建，属合同 C3/B5 认可的可达性口径，非遗留缺陷。
- 未发现需要返工的问题；无阻塞项。

## 6. 条件关闭记录（仅"有条件通过"填写）

不适用（结论为 `通过`，无附加条件）。
