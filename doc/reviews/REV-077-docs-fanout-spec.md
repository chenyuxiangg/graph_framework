# REV-077 · docs · W2b spec 回写（R16 / `max_fanout` / `NODE_FANOUT_EXCEEDED` / AC-15）

> **范围声明**：本分支仅改 `doc/**` + 根 `README.md` 共 14 个文档（无代码、无测试、无 `doc/reviews/**`）；评审人只新增本记录文件，未改文档/代码内容。cli 跨模块机械 arm 的第二评审已由 Lead 在 REV-075 追加闭合，不在本分支范围。

- **分支**：`docs/docs-fanout-spec`
- **作者**：`docs-dev`（`c9dad2a` 规格回写 00/02/03/06 + `ef090f2` 07 AC-15/PROVENANCE/索引/台账；正文均含 `Review: REV-077` / `Agent: docs-dev`）
- **评审人**：`reviewer-b`（独立评审，**≠ 作者**）
- **变更范围**：`9a91076..ef090f2`（14 files，+209/−72）：`doc/spec-v1.2/{00,01,02,03,06,07,PROVENANCE,README}.md`、`doc/{index,issues,progress,architecture-v2}.md`、`doc/decisions/DR-004-v02-scope-and-waves.md`、根 `README.md`
- **评审时间**：2026-10-31
- **关联**：Lead 合同 task-1 §D、DR-004 §3 W2b / §5 D2、已合并实现 REV-074（dsl）/ REV-075（kernel+cli）/ REV-076（e2e）、ISSUE-113–124

## 1. 结论

**结论：通过**（2026-10-31 复审修订：首轮结论为「有条件通过」；条件 C1–C4 与 P3 已于 **`d8bd58e`** 全部关闭，复审证据见 §7。以下首轮正文、§2 表格的 ⚠️ 时点判定、§3 的 `open` 时点状态与 §5 首轮条件陈述**保留供追溯**，关闭状态以 §6/§7 为准。）

as-built 主体正确、可追溯性完整：R16 静态层与运行期 §3.4.4 的语义/错误码/模板/计数口径与已合并实现（`220f53c`/`a4cb1b1`/`9a91076` 时点）逐条吻合；07 AC-15 引用的测试文件、6+6+2 个用例名与实测计数真实；PROVENANCE §3 的 9 个分册 SHA-256 与行数**实测 9/9 一致**；DR-004 §10 只追加不改原决策；progress 15 行四块；issues 113–124 追加合规；**无代码/测试/评审记录改动**。

但发现 **1 条重要（P1）+ 2 条一般（P2）** 文档校准问题（非实现偏差），且均落在本批职责（「让 spec 与 as-built/跨册一致」）内，须关闭后合并：06 §6.10.2 的 validate 规则集仍停在 R1–R15；02 §2.6.1 子图定义校验摘要未含 R15/R16（且 00 banner 引用该节）；03 §3.4.4 trace 证据措辞与 as-built（快照不产生 Transition 记录）不符。条件见 §6。

**合同 §D 逐项核对**：

- **D1（00 §0.4）**：✅ 运行期域新增 `NODE_FANOUT_EXCEEDED`（`00:209`，含义/Crashed→Failed 与 `types.rs:465-490` 一致）；DSL 域保持 **18 码**并注明 R16 复用 `DSL_PARSE_ERROR`（`00:183`、`00:95-99` 的 `FanoutExceeded`/Display 与实现逐字一致）。
- **D2（02）**：✅（除 N2）schema `max_fanout?: number`（`02:145`）、§2.2.1 字段行、§2.2.4 白名单 7 键（`nodes.rs:33-41`）、serde `skip_serializing_if`、R16 规则行/顺序 `R1→R13→R15→R16→R14`（`02:422`、`02:54/61/710`，与 `lib.rs:205-215` 一致）、错误模板 `node '{id}' out-degree {n} exceeds max_fanout {limit}` + expected/actual/location（`02:431`、`02:461`，与 `edges.rs:707-718` 逐字一致）、子图收敛 `DSL_SUBGRAPH_INVALID` 保留内部 `node_id`（`02:412` R6 行，与 `shape.rs:227-249` 一致）、hash 仅显式存在（`02:800`、`02:814`，与 `hash.rs:113-118` 一致）、D-02-13/P-02-10 裁定。
- **D3（03）**：✅（除 N3）新增 §3.4.4（`03:270-305`）：`actual` 仅计 `passed`（defer passed 计入、gate=false 不计、无 gate 计入）、`limit = min(declared, force)`（仅 test-hooks）、`apply_publish` 入口应用任何边前失败、无 GateResult/不应用边、Crashed `attempt=源+1`、冻结 message、未超限零副作用、可达性/测试入口——逐条与 `run.rs:1080-1181`、`types.rs:465-490` 一致；§3.15 矩阵补行。
- **D4（07）**：✅（除 N4 计数笔误）AC-15 a–d 落点真实：e2e 6 用例名（`tests/e2e/max_fanout.rs:123/150/173/285/371/478`）、kernel `tests/max_fanout.rs` 6 用例与 `tests/compile.rs` 2 例、dsl `r16_*` 10 例 + hash 2 例 + fixtures 表 7 valid/35 bad 均存在；a/b/c/d 断言与实测一致（见 §4）。
- **D5（回写面）**：✅ PROVENANCE §2 第 18 条（分册/关键决策/REV/合并 hash/测试证据）、§3 SHA-256+行数实测一致；`doc/index.md`/根 `README.md` 状态为 W2b 已交付、下一步 W3a0；DR-004 §10 只追加；`doc/issues.md` 新增 ISSUE-113–124（12 行、8 列、日期 2026-10-31、既有行未改）；`doc/progress.md` 15 行（≤30）四块；**`doc/test-baseline.txt` 未改**；01/06/architecture-v2 的额外补写均为最小增量（01 单字段、06 canonical JSON 行、architecture 顶部 1 行注记），内容正确。
- **D6（不改代码）**：✅ `git diff --name-only` 仅 14 个文档文件，无 `crates/**`、`tests/**`、`doc/reviews/**`。

## 2. 四维评审

| 维度 | 结论 | 依据（文件/行/实测） |
|---|---|---|
| 文档正确性 | ⚠️（N1–N3 备注） | as-built 主体逐项正确（§1）；缺陷：06 §6.10.2 validate 仍写 R1–R15/旧顺序（N1）；02 §2.6.1 子图校验摘要缺 R15/R16（N2）；03 §3.4.4 trace 证据误称有 Transition 记录（N3） |
| 一致性（跨册/与实现） | ⚠️（N1/N2 备注） | 00↔02↔03↔06↔07 的 R16/错误码/AC 计数主体一致；残留两处旧规则集：06:833（R1–R15、`R1→R13→R15→R14`）、02:611/622（R4/R5/R8–R13），后者使 `00:183` 的「子图见 02 §2.6.1」引用落空 |
| 可读性 | ✅（N4–N7 建议） | 术语一致（`max_fanout`/R16/`NODE_FANOUT_EXCEEDED`/防御层）；表格/代码块/交叉引用规范；AC-15 a–d 结构清晰（测试名+断言+复现命令）；页码/章节号连续（§3.4.4 插入后 §3.4.3→§3.5 无冲突）；瑕疵：07「13 个 target」笔误（N4）、03 §3.2 代码块残留历史字段（N5，预存）、03:286 off-loop 措辞只覆盖执行节点（N6）、architecture banner 标题（N7） |
| 可追溯性 | ✅ | PROVENANCE §3 **9/9 SHA-256 + 行数实测一致**（见 §4）；§2 第 18 条的 REV 编号与合并 hash（dsl `1fa4cc0`+`220f53c`、kernel `a4cb1b1`、e2e `9a91076`）与 `git log` 实际历史一致；AC-15 每个断言可指到真实测试文件/用例名/命令；issues 113–124 与 REV-074/075/076 建议逐条对应（N1–N4 映射核对通过）；DR-004 只追加、不覆盖原决策 |

## 3. 发现清单

> 级别口径：P0 = 阻塞；P1 = 重要（必须修复或登记 ISSUE）；P2 = 一般；P3 = 建议（可后置，建议进台账）。本分支 **P0=0**。

| # | 级别 | 位置 | 问题 | 建议 | 状态 |
|---|---|---|---|---|---|
| N1 | **P1 重要** | `doc/spec-v1.2/06-存储与可观测.md:833` | §6.10.2 `zgraph validate` 的「做什么」仍写 **R1–R15** 与旧顺序 `R1→R13→R15→R14`（`--structure-only` 也只写 R15），与 02 §2.1.2/§2.7.4 的 `R1→R13→R15→R16→R14` 及本批 W2b banner 冲突。W2a 批次曾同步此行（PROVENANCE §2 第 17 条），W2b 遗漏 | 改为「Loader 全量校验（02 R1–R16；执行顺序 `R1→R13→R15→R16→R14`）；`--structure-only` 只跑 R1–R13 + R15 + R16」 | open（条件 C1） |
| N2 | P2 一般 | `doc/spec-v1.2/02-DSL与校验.md:611`、`:622`；关联 `00总纲.md:183` | §2.6.1 子图表「定义校验」仍写「递归执行 R4/R5/R8–R13」，as-built 注（622）也写「子图内 R4/R5/R8–R13 统一聚合」，未含 **R15/R16**；与 §2.4.1 R6 行的「R4/R5/R8–R13、R15、R16」自相矛盾（R15 缺口自 W2a 遗留，R16 为本批新增），且 `00:183` 的「子图分别见 02 §2.4.1 R16 / §2.6.1」因此引用落空 | §2.6.1 两处补 R15/R16（与 R6 行一致）；或在 00:183 改引 §2.4.1 R6。推荐前者 | open（条件 C2） |
| N3 | P2 一般 | `doc/spec-v1.2/03-调度器与生命周期.md:291`（关联 `:519`） | §3.4.4「trace 证据（不可丢）」写「源节点 **Transition → Crashed 快照**」，容易被理解为存在一条 `Transition` 事件/`transitions` 行；as-built 中 `create_instance(..., crash=Some)` 只 emit `instance_snapshot`（`run.rs:1486-1543`），CLI 桥接层明确「快照不产生 `events` 行，只 upsert `instances`」（`bridge.rs:141-145`）——实际证据是 `instances` 行（`status=Crashed`）+ error JSON，**无** transition 记录。§3.15 行（519）沿用 W2a0 先例「迁移 `Crashed`」，可同口径精化 | 改为「源节点 **Crashed 实例快照**（`instances` upsert；`attempt=源+1`；`error_code=NODE_FANOUT_EXCEEDED`；`instances.error` 为 canonical JSON）」；§3.15 同步「实例快照 `Crashed`」 | open（条件 C3） |
| N4 | P3 建议 | `doc/spec-v1.2/07-验收用例.md:357` | 写「e2e 全量 64 passed / 0 failed / 2 ignored（**13 个 target**）」；实测 e2e 包 `[[test]]` 恰 **12** 个（`grep -c '^\[\[test\]\]' tests/e2e/Cargo.toml` = 12；`cargo test -p zgraph-e2e` 输出 12 条 `Running tests/*.rs`），64/0/2 计数本身正确 | 「13 个 target」→「12 个 target」（该笔误源自 REV-076 §4 的表述，AC-15 无需继承） | open（建议项） |
| N5 | P3 建议 | `doc/spec-v1.2/03-调度器与生命周期.md:62-77`（§3.2 代码块） | 本批在 `TestHooks` 中插入 `force_max_fanout`（正确，与 `run.rs:76-80` 一致），但该代码块仍有**预存**偏差：`fail_trace_write_after` 在 as-built 中不存在、缺 `tick_probe`；`RunOptions` 缺 `tick_interval_ms`/`seed`/`record_mode`、多 `python_concurrency_*`。非本批引入，但本批改动该块未顺手校准 | 登记台账（如 ISSUE-125「03 §3.2 TestHooks/RunOptions as-built 校准」），后续 docs 批次或本批关闭条件时一并修 | open（建议项） |
| N6 | P3 建议 | `doc/spec-v1.2/03-调度器与生命周期.md:286` | 「`actual` 由本次 **`NodeCompleted` 回传**的 `evaluations` 现算（gate 求值在 off-loop）」只描述执行节点；内置 `start` 节点经 `finish_builtin` 在事件循环内求值（W2b 校验同样作用于 start 激活） | 「由本次激活的 `evaluations` 现算（执行节点 off-loop、内置节点 in-loop；§3.4.1/§3.4.2）」 | open（建议项） |
| N7 | P3 建议 | `doc/architecture-v2.md:7` | 顶部注记标题仍为「**W2a 实现回写注记**（REV-073）」，正文已追加 W2b（REV-077）内容；标题与内容不完全对应 | 标题改「W2a/W2b 实现回写注记」或把 W2b 拆独立注记（1 行内） | open（建议项） |

> 说明：P3 项均不影响 as-built 正确性；N5 为预存偏差（非本批引入），N4/N6/N7 为本批可顺手修正的小项。按 AGENTS §7，未当场修复的问题应登记 `doc/issues.md`；建议作者在关闭 C1–C3 时一并登记 N5（必要时 N4）。

## 4. 验证证据

评审人**亲自执行**（worktree `/Users/zyzs/lczyzs/code/zgraph/.worktrees/docs-docs-dev`，`export CARGO_HOME=/Users/zyzs/lczyzs/code/zgraph/.cargo-home`，未设 `CARGO_TARGET_DIR`，HEAD `ef090f2`，工作区干净）：

```bash
bash scripts/dev/check.sh                                     # CHECK PASSED（身份/远程/CLAUDE.md/分支名/progress 10 非空行/fmt/diff --check）
cargo test -p zgraph-e2e --test max_fanout                    # 6 passed / 0 failed（a 3 / b 1 / c 2）
cargo test -p zgraph-e2e                                      # 12 targets：64 passed / 0 failed / 2 ignored（既有 real-LLM manual，未新增）
cargo test -p zgraph-dsl                                      # 101 passed / 0 failed（unit 1 + fixtures 6 + hash 15 + rules 78 + doctest 1）
cargo test -p zgraph-kernel                                   # 110 passed / 0 failed（默认构建不编译 max_fanout.rs）
cargo test -p zgraph-kernel --features test-hooks             # 120 passed / 0 failed
cargo test -p zgraph-cli                                      # 161 passed / 0 failed
cargo test --workspace                                        # 725 passed / 0 failed / 3 ignored
/Users/zyzs/lczyzs/code/zgraph/.venv/bin/python -m pytest -q  # 170 passed（exit 0；输出 170 个点）
git status --porcelain                                        # 空
git merge-base HEAD main                                      # 9a91076（= 本地 main；origin/main 7b4aba1 为祖先）
git diff --stat 9a91076...HEAD                                # 14 files, +209/−72；无 crates/**、tests/**、doc/reviews/**
```

**PROVENANCE §3 实测复核（9/9 一致）**：

| 文件 | 实测行数 | §3 记录行数 | SHA-256 匹配 |
|---|---|---|---|
| 00-总纲.md | 317 | 317 | ✅ `dfe8deb6…` |
| 01-领域模型与状态机.md | 348 | 348 | ✅ `3ac7a587…` |
| 02-DSL与校验.md | 950 | 950 | ✅ `f99210b7…` |
| 03-调度器与生命周期.md | 526 | 526 | ✅ `60172673…` |
| 04-节点接口与扩展点.md | 906 | 906（未改） | ✅ `46247060…` |
| 05-AgentHarness.md | 1429 | 1429（未改） | ✅ `52f35e7b…` |
| 06-存储与可观测.md | 1119 | 1119 | ✅ `7bb03379…` |
| 07-验收用例.md | 372 | 372 | ✅ `97622de1…` |
| 08-审计关闭矩阵.md | 524 | 524（未改） | ✅ `5dc763aa…` |

- 00–08 合计 **6491** 行（实测 = PROVENANCE §3）；含 `README.md`/`PROVENANCE.md`/`REFERENCES.md` 合计 **6788**（79+196+22 加总一致）；净变化 **+106 行（+143/−37）** 与 numstat 一致。
- §2 第 18 条「00 309→317、01 346→348、02 935→950、03 489→526、06 1117→1119、07 330→372」中旧值经 `git show 9a91076:doc/spec-v1.2/<f> | wc -l` **逐一复核一致**。

**可追溯性抽查（与 git 实际历史一致）**：

```text
git log --oneline -14：... 1fa4cc0 feat(dsl) → 220f53c docs(review) REV-074
  → abc1e15/aa6bfd4/6f3ac2a/e37b1ad → a4cb1b1 docs(review) REV-075 第二评审
  → 06d6945 test(e2e) → 9a91076 docs(review) REV-076 → c9dad2a/ef090f2（本分支）
```

**AC-15 引用的测试名/断言复核**：e2e 6 个用例名（`bad_max_fanout_exceeded_reports_r16_fanout_error` / `bad_max_fanout_value_reports_must_be_positive` / `valid_max_fanout_passes_validate_with_frozen_hash` / `valid_max_fanout_runs_to_completion_through_real_cli` / `runtime_fanout_exceeded_via_test_hooks_fails_run_and_crashes_source` / `runtime_fanout_at_limit_completes_as_boundary_case`）与 AC-15 a/b/c 一一对应；`single_error` 断言 `error_count=1`/`truncated=false`、C1 超限用例断言无 `graph_hash`、`expected="out-degree <= 1"`/`actual="out-degree 2"`、location 仅 node_id；C2 断言 `run_output={"ra":{"worker":"a","x":{}},"rb":{"worker":"b","x":{}}}` 且 stderr 无 `DSL_PARSE_ERROR`；C3-a 断言 instances 恰 2 行（源 attempt0 Completed + attempt1 Crashed、error JSON `kind/limit/actual`）、`gate_results` 为空、下游零实例；C3-b 断言边界 Completed——**AC-15 文本与实测一致**（除 N4 的 target 计数）。dsl `tests/rules.rs` 的 `r16_*` 10 例（含 `r16_violation_inside_subgraph_maps_to_subgraph_invalid`、`r16_runs_after_r15`、`r16_runs_before_r14`）、`hash.rs` 2 例、`fixtures.rs` 7 valid/35 bad 均在 main 真实存在。

**as-built 交叉核对（文档 ↔ 代码）**：R16 执行顺序 `lib.rs:205-215`；`out_degree` 全部出边 + nodes 序首错 + message/expected/actual `edges.rs:687-718`；白名单 7 键与 `skip_serializing_if` `nodes.rs:33-41`/`types.rs:282-287`；R4 取值模板 `nodes.rs:785-806`；子图收敛 `shape.rs:227-249`；hash 仅显式存在 `hash.rs:113-118`；运行期 `run.rs:1080-1181`（入口校验、`actual` 只计 passed、`min(declared, force)`、Crashed attempt+1、fail_run、冻结 message）；`NodeError`/code/Display `types.rs:465-490`；canonical JSON arm `bridge.rs:739-744`、快照不产生 events 行 `bridge.rs:141-145`。

## 5. 遗留问题

- **P3 建议（N4–N7）**：建议作者/Lead 决定本批顺手修复（N4/N6/N7 均 1 行内）或在 `doc/issues.md` 登记（N5 为预存偏差，建议登记 ISSUE-125「03 §3.2 TestHooks/RunOptions as-built 校准」）。
- **08 未追加 W2b 注记**：08 为 v1.1 审计关闭矩阵，合同 §D5 未要求；本批保持历史快照不改，符合既有 append-only 口径，无需动作。
- **收口流程**：本记录结论为「有条件通过」；条件关闭后须在同一分支追加修复提交，并由评审人追加复审段（§7）后把结论改为无条件「通过」，`scripts/dev/merge.sh` 才会放行（该脚本只接受无附加条件的「通过」结论）。
- 未发现需要回滚/返工的实现问题；不涉及 `doc/reviews/**` 或代码。

## 6. 条件关闭记录（仅「有条件通过」填写）

| 条件 | 关闭证据（需可复核） | 关闭日期 |
|---|---|---|
| C1 修 06 §6.10.2 validate 规则集（N1）：`R1–R16` + `R1→R13→R15→R16→R14` + `--structure-only` 含 R16 | `doc/spec-v1.2/06-存储与可观测.md:833` 现为「02 R1–R16；`R1→R13→R15→R16→R14`；`--structure-only` 只跑 R1–R13 + R15 + R16」；`grep "R1–R15\|R1→R13→R15→R14"` 在 06 **0 命中** | **已关闭 2026-10-31（`d8bd58e`，§7.2）** |
| C2 修 02 §2.6.1 子图校验摘要（N2）：611/622 补 R15/R16（或改 00:183 引用为 §2.4.1 R6），与 §2.4.1 R6 行一致 | `02:611/622` 已补 **R15/R16**（子图内违规收敛 `DSL_SUBGRAPH_INVALID`、保留内部 `node_id`），与 §2.4.1 R6 行一致；`00:183` 引用改为「主图见 §2.4.1 R16；子图递归含 R15/R16 见 §2.6.1 / §2.4.1 R6」，不再落空 | **已关闭 2026-10-31（`d8bd58e`，§7.2）** |
| C3 修 03 §3.4.4 trace 证据措辞（N3）：改为实例快照/`instances` 行（无 Transition 记录），§3.15 同口径 | `03:291` 改为 **Crashed 实例快照**（`instances` upsert；**不产生** `transitions`/`events` 行；证据 = `instances` 行 + `error` JSON + `RunFinished.error`），`03:519` §3.15 同口径；与 `bridge.rs:141-145`、`run.rs:1486-1543` 一致 | **已关闭 2026-10-31（`d8bd58e`，§7.2）** |
| C4 关闭 C1–C3 后重测并同步：PROVENANCE §3 SHA-256（行数若变同步 §2 第 18 条/spec README 行数表）；重跑门禁；提交正文含 `Review: REV-077`/`Agent: docs-dev` | PROVENANCE §2 第 19 条 + §3 重算：00/02/03/06/07 的 SHA-256 与行数实测一致；00–08 仍 **6491**、spec 目录 **6795**；`check.sh` PASSED、`cargo test -p zgraph-e2e --test max_fanout` **6/0**；提交正文含 `Review: REV-077`/`Agent: docs-dev` | **已关闭 2026-10-31（`d8bd58e`，§7.3）** |

## 7. 条件关闭与复审（2026-10-31）

> 复审对象：`d8bd58e`（条件修复批次，直接父提交 = 评审提交 `757833f`；内容基线 `ef090f2`）。本节为最终状态，取代 §1/§2/§5/§6 中「有条件/待关闭/open」的**时点陈述**；首轮正文不删，供追溯。

### 7.1 复审范围与范围门禁

```bash
git log --oneline -4        # d8bd58e（本批）→ 757833f（REV-077 首轮评审）→ ef090f2 → c9dad2a
git diff --stat 757833f..d8bd58e   # 11 files changed, 35 insertions(+), 21 deletions(-)
git -c core.quotepath=false diff --name-only 757833f..d8bd58e
# 11 个路径全部 doc/**（architecture/index/issues/progress/reviews-README + 00/02/03/06/07/PROVENANCE）
# 无 crates/**、tests/**、doc/reviews/REV-*.md 之外的改动；工作区 clean
git log -1 --format=%B      # 正文含 Review: REV-077 / Agent: docs-dev
```

### 7.2 条件与 P3 逐条关闭证据

- **C1（N1）✅**：`06:833` 已改为「Loader 全量校验（02 R1–R16；执行顺序 `R1→R13→R15→R16→R14`）；`--structure-only` 只跑 R1–R13 + R15 + R16（02 §2.7.4）」；`grep -n "R1–R15\|R1→R13→R15→R14" doc/spec-v1.2/06-存储与可观测.md` → **0 命中**；与 02 §2.1.2/§2.7.4 一致。
- **C2（N2）✅**：`02:611` 子图表定义校验与 `02:622` as-built 注均补 **R15/R16**（并写明收敛 `DSL_SUBGRAPH_INVALID`、保留内部 `node_id`/`edge_index`），与 §2.4.1 R6 行一致；`00:183` 引用改为「主图见 02 §2.4.1 R16；子图递归含 R15/R16 见 02 §2.6.1 / §2.4.1 R6」，引用不再落空。
- **C3（N3）✅**：`03:291` 改为 **Crashed 实例快照**（`instances` 行 upsert；**不产生** `transitions`/`events` 行；证据 = `instances` 行 + `error` JSON + `RunFinished.error`；明确「不要按 `transitions` 查询」）；`03:519` §3.15 矩阵同口径（实例快照 `Crashed`）。与 as-built `create_instance(..., crash)` 只 emit `instance_snapshot`（`run.rs:1486-1543`）、CLI 桥接「快照只 upsert instances」（`bridge.rs:141-145`）一致。
- **P3 ✅**：N4 `07:357` 已改「**12 个 target**」（与 `tests/e2e/Cargo.toml` 的 12 个 `[[test]]`、`cargo test -p zgraph-e2e` 的 12 条 Running 一致）；N5 已登记 **ISSUE-125**（docs/P3/open/2026-10-31，8 列格式与既有行一致，仅追加不改旧行，注明「预存偏差，后续 docs 批次校准」）；N6 `03:286` 已精确化为「执行节点经 `NodeCompleted` off-loop / 内置节点 `finish_builtin` in-loop」；N7 `architecture-v2:7` 标题改为「**W2a/W2b 实现回写注记**（REV-073；W2b 追加 REV-077）」。
- **Lead 要求的索引收口 ✅**：`doc/reviews/README.md` 追加 **REV-074/075/076/077 四行**（表格 6 列与既有 75 行一致；分支/评审人/结论格式核对：074 reviewer-a、075 reviewer-b（主审）+ lead（cli 第二评审）、076 reviewer-a、077 reviewer-b，与各 REV 记录一致）并追加底部 **W2b 收口段**（REV-074–077 模块/作者/评审人/结论）；`doc/index.md` 与 `doc/progress.md` 已更新为最终态（progress **15 行**、四块、≤30）。
  - **REV-077 行措辞同步判断**：该行现为「通过（条件关闭见记录 §6；…C1–C4 于条件修复提交关闭）」。本节把记录结论翻为「通过」后**该措辞仍准确**，**无需 docs-dev 追加 commit**（若追求完整可选把「§6」写成「§6/§7」，非必要）。

### 7.3 复审验证（评审人实跑）

```bash
git -c core.quotepath=false diff --name-only 757833f..HEAD   # 11 files，全部 doc/**（纯文档）
bash scripts/dev/check.sh                                    # CHECK PASSED
cargo test -p zgraph-e2e --test max_fanout                   # 6 passed / 0 failed
shasum -a 256 <00..08>; wc -l <00..08 README PROVENANCE REFERENCES>  # 与 PROVENANCE §3 逐项比对
```

**PROVENANCE §3 重算实测（条件修复后）**：

| 文件 | SHA-256（实测 = §3 记录） | 行数 | 结论 |
|---|---|---|---|
| 00-总纲.md | `f32eba90627a75f4…` | 317 | ✅ |
| 02-DSL与校验.md | `33c7028f2dcf547b…` | 950 | ✅ |
| 03-调度器与生命周期.md | `033b87f6ee95aba6…` | 526 | ✅ |
| 06-存储与可观测.md | `be9ceac8a65268ea…` | 1119 | ✅ |
| 07-验收用例.md | `ed4371639bb9c29e…` | 372 | ✅ |
| 01/04/05/08（未改） | 与 §3 原值逐一相同 | 348/906/1429/524 | ✅ |

- 00–08 合计 **6491** 行（与 §3 一致）；`doc/spec-v1.2/` 目录合计 **6795** 行（实测 `wc -l` = 6491 + README 79 + PROVENANCE 203 + REFERENCES 22，与 §3「当前 6795」一致）；
- §2 第 **19** 条已登记条件修复批次（C1–C4/P3/重测/落盘），编号连续、内容与本节一致；
- `doc/issues.md` 仅追加 ISSUE-125 一行（12+1 行、8 列、日期/状态正确，既有行未改）；`doc/progress.md` 15 行四块。

### 7.4 复审结论

**C1–C4 与非阻塞 P3 全部关闭，最终结论：通过**（§1 已同步；merge.sh 可识别「结论：通过」）。无非阻塞条件残留，可直接进入合并流程。

非阻塞残留观察（不构成条件，建议后续 docs 批次顺手处理）：

1. **P3 措辞**：PROVENANCE §2 第 19 条写「base = `ef090f2`」，而条件修复提交 `d8bd58e` 的直接父提交实为评审提交 `757833f`（`ef090f2` 为 docs 内容基线）。不影响复核；后续可写「内容基线 ef090f2 / 直接父 757833f」。
2. **P3 可选**：`doc/reviews/README.md` REV-077 行可选择性引用 §7；当前措辞已准确，无需为合并改文件。
3. **ISSUE-125 保持 open**（预存 §3.2 TestHooks/RunOptions 偏差，按计划后续 docs 批次校准，不阻塞 W2b）。
4. **08 未追加 W2b 注记**：合同 §D5 未要求，保持 v1.1 审计历史快照，符合 append-only 口径，无需动作。
