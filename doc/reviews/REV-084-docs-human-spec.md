# REV-084 · docs · W3a as-built 回写（spec 00/01/03/04/06/07/08 + DR-006 §15 + PROVENANCE/台账/进度）

> **范围声明**：本分支为 **15 个纯文档文件**（+208/−58），全部在 `doc/**`；无代码/测试/配置改动，未改 `Cargo.lock`，未改既有评审记录（仅 `doc/reviews/README.md` 索引由本评审补 REV-084 行）。评审人只新增本评审记录并补全 reviews 索引行，未改其它文档内容。

- **分支**：`docs/docs-human-spec`
- 分支: docs/docs-human-spec
- **作者**：`docs-dev`（提交 `a5fd3b1`，正文 `Review: REV-083` / `Agent: docs-dev`；编号按 Lead 调整为 REV-084）
- **评审人**：`reviewer-a`（独立评审，**≠ 作者**）
- **变更范围**：`2444548..a5fd3b1`（15 files, +208/−58）：DR-004 §10 / DR-006 §15、spec 00/01/03/04/06/07/08、PROVENANCE、spec README、`doc/{index,issues,progress,reviews/README}.md`。`2444548` = 本地 main（含 REV-079 trace / REV-080 kernel / REV-081 cli / REV-082 cli fix / REV-083 e2e）。
- **评审时间**：2026-11-01
- **关联**：task-14 §D/§E、task-19；DR-006 §14.3/§15；REV-079..083；AGENTS §3（四维评审）/§4（合并门禁）/§5（progress）/§7（台账）

## 1. 结论

**结论：通过**

按 task-19 与 Lead 评审口径逐条核对，as-built 与已合并实现/测试**逐条一致**（唯一为 N1 数字笔误，P3）；PROVENANCE §3 的 9/9 SHA-256 与行数、增量口径实测一致；AC-16 实测测试名/计数真实可复现；08 §8.10 source-breaking 清单与实现一致；台账/索引/进度格式合规；无代码改动、未改既有评审记录、DR-006 §15 为纯追加。P0=0、P1=0、P2=0，P3 建议 8 条（均不阻塞）。

### 1.1 as-built ↔ 已合并实现核对（抽查关键断言）

| 文档主张 | 实测证据 | 结论 |
|---|---|---|
| AC-16 a–g **21 用例**、a4/b3/c1/d1/e3/f3/g3 + 静态 3 | `tests/e2e/human_decide.rs`：`#[test]` 19 + `#[tokio::test]` 2 = **21**；按 07 列出的 21 个测试名与源码函数名集合**完全一致**（差集为空）；fixtures `human_gate/{approve,rework,reject_end,timeout}.json`、`trace/{schema_v4_seed.sql,v4_expected_trace.json}` 均存在 | ✅ |
| e2e **85/0/2**、human_decide **21/0 连跑 3 次稳定**、2 ignored 为既有 real-LLM manual | 评审实跑 `cargo test -p zgraph-e2e` = **85 passed/0 failed/2 ignored**；`--test human_decide` 连跑 **3 次均 21/0**；`--test gate_defer` 8/8；2 个 `#[ignore]` 均为既有 `harness_ac08`/`software_team_v1` real-LLM manual，human_decide 0 ignore | ✅ |
| workspace **819/0/3**；kernel **122/132(test-hooks)**；cli **206**；trace **79**；pytest **170** | 评审实跑：workspace **819/0/3**；`cargo test -p zgraph-kernel` **122/0**、`--features test-hooks` **132/0**；cli **206/0**；trace **79/0**；pytest **170 passed** | ✅ |
| trace v5 / 迁移证据（migration 15、decisions 11；评审人 27/27 真 CLI 探针） | `cargo test -p zgraph-trace` 79（migration 15、decisions 11）；REV-079 记录 §4.2 真 CLI 探针 27/27（备份/失败恢复/只读拒绝/索引与列/逐列数据）一致 | ✅ |
| WaitingHuman 生命周期/9 状态 13 迁移/input 快照/deadline/`is_quiescent` | kernel `types.rs:211-228` = 13 条合法迁移、`is_active` 含 `WaitingHuman`；`maybe_make_ready` Human 分支 → `enter_waiting_human`；`human_wait.rs` **9 用例**名与文档所述一致（无 worker/无 Running/input 全端口/120ms>30ms grace/Paused 到期/冷峻取消）；`state_machine` 9×9 全组合 | ✅ |
| consumer 拓扑/一律提交/`applied=false` 三值/E-6/`--quiet`/`:memory:` | cli `run_cmd.rs` 的 `DecisionSession/decision_consumer_loop/submit_human_decisions/HumanDecisionObserver`、按需启动、`mark_consumed` 幂等、退出 flush；`human_wait.rs` 的 `not_current`/`not_waiting_human`/`instance_missing` 用例；`Emitter::finish` 置 `closed=true` 后确认丢弃（E-6）；human+`:memory:` e2e ac16g 启动前拒绝 | ✅ |
| decide 契约（退出码/`--json`/不建库/不迁移/Paused） | cli 206/0；REV-081/REV-083 用例与 e2e ac16c/ac16f 实测；`:memory:` ac16g | ✅ |
| L2：human `recorded_outputs` 补录 + replay handler 替换 | e2e ac16a L2（含 missing-record 拒绝）、ac16b rework attempt0/1 L2（`recorded_outputs=4`）；与 REV-082 实现/评审一致 | ✅ |
| 08 §8.10.2 source-breaking 清单 | 逐项对照代码：pre-W3a `3d0e933` 的 `RunControl` 为 `Copy`、现为 `Clone`（+新 variant）✓；kernel/trace 同有 `InstanceStatus::WaitingHuman` ✓；`CompiledHandler::Human`（`compile.rs` 双反向断言）✓；`TraceCall::HumanDecided` ✓；`TraceSink::human_decided` 默认 no-op ✓；trace 新公开类型 ✓；无 `resume` ✓ | ✅（清单完整性见 N3） |
| PROVENANCE §3 **9/9 SHA-256 + 行数**；00–08 **6883**、spec 合计 **7236**；00–08 numstat **+100/−32 净 +68** | 评审实测：9 个分册 `wc -l` 合计 **6883**、全部（含 README/PROVENANCE/REFERENCES）**7236**；9/9 SHA-256 与表一致；`git diff --numstat main...HEAD -- 'doc/spec-v1.2/0*.md'` = **+100/−32（净 +68）** | ✅（口径措辞见 N6） |
| 提交/合并点（`4cae6b9`/`adf54a1`、`ab4b805`+`72036b6`/`390dbe1`、`4d671a0`+`c711232`/`971e48f`、`c3022ea`/`9006345`、`05383d0`+`5128f53`+`2ac54c2`/`2444548`） | 14 个 hash 全部存在且为 HEAD 祖先（`git cat-file` + `merge-base --is-ancestor`） | ✅ |
| progress 15 行/四块/下一步 W4a/W3b 可选需用户确认 | `doc/progress.md` = 15 行、四块齐全；下一步 W4a、W3b 可选需用户确认 | ✅ |
| issues ISSUE-129–140 连续/8 列/映射 REV；ISSUE-137 P2 观察 | 共 140 行；新增 129–140 连续 12 行、均 8 列、逐行引用 REV-079/080/081/082/083；ISSUE-137 标注「非本批引入、复现升 P1」**P2 级别可接受** | ✅ |
| 无代码改动/不改既有评审记录/DR-006 §15 只追加 | `git diff --name-only` 全为 `doc/**`；`doc/reviews/**` 仅 `README.md`；DR-006 diff 删除行 = **0**（纯追加）；DR-004 见 N2 | ✅ |

## 2. 四维评审

| 维度 | 结论 | 依据（文件/行/测试） |
|---|---|---|
| 功能正确性 | ✅ | as-built 与实现/测试逐条一致（§1.1）；AC-16 21 个测试名与源码集合完全相等、计数 85/21/819/122/132/206/79/170 全部评审实跑复现；PROVENANCE §3 SHA/行数 9/9 实测一致；唯一数字笔误见 N1（P3） |
| 代码结构合理性 | ✅（N2/N3/N6 备注） | 采用「banner + 章节 as-built 注 + DR-006 §15 执行记录 + PROVENANCE §2/§3 + 台账/索引/进度」的既有 docs 结构，无重复叙述；00/01/03/04/06 只追加注记，07 设计段按 as-built 重写（任务允许），08 新增 §8.10（不重写 8.0–8.9）；仅 DR-004 §10 有 1 行替换（N2） |
| 逻辑性 | ✅ | 可追溯链完整：模块 → 分支/提交 → REV → 合并点 → 测试证据；同一数字在 DR-006 §15 / 08 §8.10.1 / PROVENANCE §2 三处一致（除 N1 字段数笔误）；AC-16 a–g 分组覆盖与 e2e 实际用例一一对应；issues→REV 映射闭合；无自相矛盾（历史段保留、as-built 段明确时点） |
| 可读性 | ✅（N1/N4 备注） | as-built 注均带日期/REV/DR 锚点并说明「为什么」；AC-16 用例名/断言证据具体；progress 简洁（15 行）；N4 为表格措辞小瑕疵，N1 为数字笔误 |

## 3. 发现清单

> 级别口径：P0 = 阻塞；P1 = 重要（必须修复或登记 ISSUE）；P2 = 一般；P3 = 建议（可后置，应进台账）。本分支 P0=0、P1=0、P2=0。

| # | 级别 | 位置 | 问题 | 建议 | 状态 |
|---|---|---|---|---|---|
| 1 | P3 建议 | `doc/spec-v1.2/01-领域模型与状态机.md:7,289`；`doc/spec-v1.2/PROVENANCE.md:201` | `HumanDecisionRecord` 实为 **9 字段**（`crates/zgraph-kernel/src/trace.rs:163-182`：run_id/instance_id/decision/approver/reason/decision_id/decided_at/applied/reject_reason），文档 3 处写「10 字段」（同一句紧跟的字段清单是正确的 9 个）。根因：已合并的 REV-080 记录亦写「10 字段」（既有评审记录不可改） | 建议 docs-dev 在合并前加小 commit 改 3 处「10」→「9」，并重算 01 的 SHA-256 到 PROVENANCE §3（行数不变，总行数不变）后重跑 check.sh；若不修，Lead 可登记 ISSUE 并在 W3a 收口注明（不改变结论级别） | open（建议项） |
| 2 | P3 建议 | `doc/decisions/DR-004-v02-scope-and-waves.md:267` | DR-004 §10 **替换**了 W3a0 记录既有的「后续」行（`+2/−1`，删除原 1 行），与「只追加」口径不完全一致；W1/W2a/W2b 先例均为「新增交付块 + 新后续行」 | 恢复原「后续」行，改为在 W3a0 块末尾追加独立「W3a 实现已交付」段（含新后续行）；内容无需改 | open（建议项） |
| 3 | P3 建议 | `doc/spec-v1.2/08-审计关闭矩阵.md:555`（§8.10.2 item 6） | source-breaking 清单的 additive 项只列 trace 新公开类型，缺 kernel additive 公开 API：`HumanDecision` 枚举、`HumanDecisionRecord` 结构，以及 trace `DecisionPrecheck`（`crates/zgraph-trace/src/lib.rs:109` 已导出） | 在 item 6 补 kernel additive 类型与 `DecisionPrecheck`（或单列「additive（不破坏）」行） | open（建议项） |
| 4 | P3 建议 | `doc/spec-v1.2/07-验收用例.md:57`（AC-16 表格行） | 写「21/0（**a3 静态** + a4/b3/c1/d1/e3/f3/g3）」，其中「a3 静态」易误读为「a 有 3 个静态用例」；同文件 `:6` 与 §AC-16 正文均写「a 4 / … + 静态 fixtures 3」，含义应为「静态 3 + a4…」 | 改为「静态 3 + a4/b3/c1/d1/e3/f3/g3」（与 `:6` 一致） | open（建议项） |
| 5 | P3 建议 | `doc/issues.md:135`（ISSUE-127） | ISSUE-127 建议动作「W3a-docs 回写时删/改 01 历史名或明确别名关系」已由本批完成（`01:7` 与 `01:52` 明确 `HandlerRef`=历史名、实现名为 `CompiledHandler::Human`），但台账仍 `open` | 按建议标 `fixed` 并注明 REV-084（或写明剩余统一范围）；不改变本批评审结论 | open（建议项） |
| 6 | P3 建议 | `doc/spec-v1.2/PROVENANCE.md:210,225` | 「净 +68（新增 100/删除 32）」实为 **00–08 分册** numstat（实测一致），但句内未显式限定口径；全 spec 目录实测为 **+135/−50（净 +85）**（含 README/PROVENANCE），易被误读 | 措辞改为「00–08 numstat +100/−32（净 +68）」；无需改数字 | open（建议项） |
| 7 | P3 建议 | `doc/progress.md:5,9`；`doc/index.md:22`；`doc/spec-v1.2/08-审计关闭矩阵.md:544`；`doc/decisions/DR-006-w3a-human-decide-freeze.md:638`；`doc/spec-v1.2/PROVENANCE.md:208` | 评审后仍写「REV-084 本分支待评审 / 评审人待定」。按 Lead 指示本次仅更新 `doc/reviews/README.md`；其余 5 处为分支时点快照 | 合并前由 docs-dev/Lead 小 commit 同步为「通过（reviewer-a）」，或按 REV-078 §5 先例由 Lead 合并后收口（非阻塞） | open（建议项） |
| 8 | P3 说明 | `doc/issues.md:137`（ISSUE-137 P2） | ISSUE-137 把 REV-081 首轮 workspace 的 trace `channels` FK flake 观察登记为 **P2**；该观察不可复现、非 W3a 引入，但确实可能造成 merge gate 假失败 | **P2 级别可接受**（已写「复现则升 P1」）；建议保留，无需调整 | open（说明项） |

## 4. 验证证据

评审人**亲自执行**（worktree `/Users/zyzs/lczyzs/code/zgraph/.worktrees/docs-docs-dev`，HEAD `a5fd3b1`，基于 `main 2444548`，工作区干净；`CARGO_HOME=<repo>/.cargo-home`）：

```bash
# 1) 实现/测试计数（本分支 docs-only，代码 = main）
cargo build -p zgraph-cli --bin zgraph --locked          # 供 e2e 真 CLI
cargo test -p zgraph-e2e                                 # 85 passed / 0 failed / 2 ignored
cargo test -p zgraph-e2e --test human_decide             # 21 passed（连跑 3 次均 21/0）
cargo test -p zgraph-e2e --test gate_defer               # 8 passed
cargo test -p zgraph-kernel                              # 122 passed / 0 failed
cargo test -p zgraph-kernel --features test-hooks        # 132 passed / 0 failed
cargo test -p zgraph-cli                                 # 206 passed / 0 failed
cargo test -p zgraph-trace                               # 79 passed / 0 failed
cargo test --workspace                                   # 819 passed / 0 failed / 3 ignored
/Users/zyzs/lczyzs/code/zgraph/.venv/bin/python -m pytest  # 170 passed
bash scripts/dev/check.sh                                # CHECK PASSED

# 2) PROVENANCE §3 实测（脚本化复核）
cd doc/spec-v1.2 && wc -l 0*.md                          # 6883 total
wc -l 0*.md README.md PROVENANCE.md REFERENCES.md        # 7236 total
shasum -a 256 0*.md                                      # 9/9 与 PROVENANCE §3 表一致
cd <repo> && git diff --numstat main...HEAD -- 'doc/spec-v1.2/0*.md'
# 00 +3/-0、01 +5/-0、03 +5/-0、04 +3/-0、06 +11/-0、07 +36/-32、08 +37/-0
# 合计 +100/−32（净 +68）——与 PROVENANCE §2 第 22 条/§3 注一致

# 3) AC-16 测试名真实性（脚本比对）
# 07 中列出的 21 个用例名 vs tests/e2e/human_decide.rs 实际函数名：集合完全相等（差集为空）
# 资产存在：tests/fixtures/graphs/human_gate/{approve,rework,reject_end,timeout}.json
#          tests/fixtures/trace/{schema_v4_seed.sql,v4_expected_trace.json}

# 4) 引用提交/合并点
# 14 个 hash（4cae6b9/adf54a1, ab4b805/72036b6/390dbe1, 4d671a0/c711232/971e48f,
#        c3022ea/9006345, 05383d0/5128f53/2ac54c2/2444548）全部存在且为 HEAD 祖先

# 5) 结构/范围
git -c core.quotepath=false diff --name-only main...HEAD  # 15 files，全部 doc/**（无代码/测试/配置）
git diff --name-only main...HEAD -- doc/reviews/          # 仅 README.md
git diff main...HEAD -- doc/decisions/DR-006-*.md | grep -c '^-[^-]'   # 0（纯追加）
wc -l doc/progress.md                                     # 15（≤30，四块）
grep -c '^| ISSUE-' doc/issues.md                         # 140；ISSUE-129–140 连续、8 列、映射 REV
```

- **as-built 关键代码对照**：`types.rs:211-228`（13 信任迁移）/`:186-191`（is_active 含 WaitingHuman）；`run.rs:1739-1752`（maybe_make_ready→enter_waiting_human）、`:890-915`（force timeout 只在 dispatch，human 不生效）、`:2090-2110`（cancel 含 WaitingHuman）、`:750-790`（决策顺序）；`trace.rs:163-182`（HumanDecisionRecord 9 字段）、`:721-731`（Emitter finish 置 closed）、`:262`（human_decided 默认 no-op）；`compile.rs`（builtin.human↔builtin.other 双反向）；`human_wait.rs` 9 用例；`state_machine.rs` 9×9 矩阵；e2e `human_decide.rs` 21 用例。
- 结论：所有 as-built 主张与实现/测试一致；唯一不一致为 N1 的「10 字段」笔误（P3，建议修）。

## 5. 遗留问题

- **N1（10→9 字段）**：建议 docs-dev 在合并前加一个小 commit 修正 3 处并重算 `01` 的 SHA-256（行数/总行数不变）；若不改，Lead 可登记 ISSUE 并在 W3a 收口说明。评审人可在合并前快速复核该小 commit（不改变本分支结论）。
- **N2–N7 为 P3 建议**：本分支合同限定 `doc/**` 且「不改文档内容（除 reviews README）」，评审人未修改；建议 Lead 决定是否要求 docs-dev 追加小 commit（DR-004 只追加/08 清单补 additive/07 措辞/ISSUE-127 关闭/PROVENANCE 口径/评审后「待评审」同步），或按 AGENTS §7 登记台账后收口。
- **后续**：W3a 实现已全部合入 main（`2444548`），合并本 docs 分支后 W3a 完整收口；下一步 **W4a（动态 N）**，W3b durable resume 为可选、需用户确认（与本批一致）。
- 未发现需要返工的问题；无阻塞项。

## 6. 条件关闭记录（仅"有条件通过"填写）

不适用（结论为 `通过`，无附加条件）。
