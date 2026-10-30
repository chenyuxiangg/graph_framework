# REV-024 · docs/process · M4 收尾（进度/台账/README/评审索引/基线/spec 索引）

> **模块声明**：本分支改 8 个文件：`doc/progress.md`、`doc/issues.md`、`doc/reviews/README.md`、`doc/test-baseline.txt`、`doc/index.md`、`doc/spec-v1.2/README.md`、`doc/spec-v1.2/PROVENANCE.md`（docs 模块），以及根 `README.md`（**process 模块**，作者 `lead` 亦为 process 模块负责人；提交正文显式声明「跨模块说明：README 属 process（Lead 在评审记录声明，reviewer-b 复核）」，符合 AGENTS §2 的 Lead 声明 + 第二评审要求）。作者 `lead`，评审人 `reviewer-b`（**非作者**）。评审**未修改任何文件**，只新增本记录；未 commit / 未 merge。
>
> **最终评审对象**：`573676a`（Lead 更正后 HEAD；前 `c2119dd` 作废）。已独立核对 `c2119dd..573676a` 增量 **仅 `doc/spec-v1.2/PROVENANCE.md` 2 行**（§2 第 7 条与 §3 的 base 表述改为「评审时 `fda996c`；合并时 rebase 至 `12ab2f6`」），且与 main 历史一致（`12ab2f6` REV-023 记录提交 → `49b1b5f`/`48d1b48` docs 合并）；其余内容按最终 HEAD 复跑/复核。

- **分支**：`docs/docs-m4-wrapup`
- **作者**：`lead`
- **评审人**：`reviewer-b`
- **变更范围**：`48d1b48..573676a`（1 commit `573676a docs(docs): M4 收尾——进度/台账/README/评审索引/基线/spec 索引同步`；`git diff --shortstat` = **8 files changed, 63 insertions(+), 40 deletions(-)**；base = merge-base = `origin/main` = `48d1b48`，**基于最新 main**；worktree clean）
- **评审时间**：2026-10-30
- **关联**：REV-017～REV-023（M4 全部分支）；REV-020 §5 C3 清单；REV-023 §5（spec README 行数表/#7 ISSUE 登记）；AGENTS §5（progress ≤30 行）/§6（评审索引）/§7（台账）/§10；architecture §6.1 M4 DoD；ISSUE-061/066/073–083

## 1. 结论

**结论：通过**

作为 M4 收尾提交，内容**事实准确、可核验、无夸大**，前几轮评审的遗留交接项全部落地：

- **spec README 行数表（REV-023 #5）已更新且与实测一致**：00 292 / 04 895 / 05 1418 / 07 213；全部分册 `wc -l` 实测 00–08 = **5818 行**，与 `doc/spec-v1.2/PROVENANCE.md` §3 合计一致；9 个分册 SHA-256 与 PROVENANCE §3 表格 **逐项一致**（独立复算）。
- **台账完整**：`doc/issues.md` 83 行、ID `ISSUE-001..083` **连续无重号**，每行 8 列（9 个 `|`）；新增 `ISSUE-073..083` 连续且与 05 §5.13.3 的 **L1–L10 映射逐项一致**（L1↔076、L2↔077、L3↔078、L4↔074、L5↔075、L6↔073、L7↔079、L8↔081、L9↔080、L10↔082）；`ISSUE-083` 恰好覆盖 REV-023/REV-024 预检指出的「tool 模块 L3 docstring 守卫缺失」；022/047/067/068/070/071 的 status 变更均有 REV 依据（022/067/070 引 REV-013/018/023，047 引 REV-015/022，068/071 引 REV-017）；**ISSUE-070 fixed + 残项**已明示「08 审计矩阵注记随 M5 docs（ISSUE-020）」；**ISSUE-022 fixed + 残项**已转 `ISSUE-083`。
- **progress**：26 行（非空 21，≤30；check.sh 也通过）；四块结构齐全；M4 模块行状态/负责人与 REV-023/024 一致（docs 行 `docs-writer / lead`）；下一步/阻塞与测试计数可核验（`cargo test --workspace` **423 passed / 2 ignored**、pytest **84 passed**，均在干净 target 实测复现）。
- **README（process 跨模块）**：`已完成 M0–M4` 的表述与事实一致（Harness、tool_runner、AC-08 fake 落地），**未夸大 M5**（CLI `run`/`replay`、L2、AC-12、legacy 明确未实现）；目录/计数真实；无新增不实声明。
- **reviews 索引**：25 行（REV-000–024）齐全，结论与各记录 §1 一致（REV-020 标注修订、REV-023 已通过）；REV-022/023 链接文件存在（`fda996c`/`48d1b48` 已合并）；REV-024 行结论与本记录一致（通过）。
- **test-baseline**：仅追加注释行，**失败名额仍为 0（未扩大）**；`merge.sh` 的 `grep -v '^#'` 口径不受影响。
- **校验**：`check.sh` PASS、worktree clean、提交元数据合规（author/committer `chenyuxiangg`、正文含 `Review: REV-024` / `Agent: lead`）；未改代码/测试/CI/spec 正文（仅 spec 索引/PROVENANCE 记录）。

未发现阻塞/重要项；3 条建议 + 4 条备注见 §3（均为口径/流程/低优先），不影响合并。

## 2. 四维评审

| 维度 | 结论 | 依据（命令/文件/核验） |
|---|---|---|
| 功能正确性 | ✅ | ① spec README 行数表逐项与 `wc -l` 一致（292/302/913/388/895/1418/985/213/412，合计 5818）；PROVENANCE §3 的 9 个 SHA-256 与 `shasum -a 256 0*.md` **逐项一致**；② issues 83 行 ID 连续、列结构一致、073–083 新增齐全、073–082 ↔ 05 §5.13.3 L1–L10 映射正确、083 内容与 REV-023 预检一致；③ progress 计数与实跑一致（workspace 423 passed/0 failed/2 ignored、pytest 84 passed，干净 `CARGO_TARGET_DIR` 复现）；④ README 的 M4/M5 边界与代码事实一致（`zgraph-cli` 仍只有 validate/trace；`tests/e2e/harness_ac08.rs` 已合并）；⑤ `doc/index.md` 评审索引描述改为 REV-000–024，与 `doc/reviews/README.md` 一致。 |
| 代码结构合理性 | ✅ | 仅文档/台账/索引/基线（+README process 声明），无代码/测试/CI/spec 正文改动；改动面收敛在 8 个预期文件；progress 维持四块且 ≤30 行；台账保持唯一来源、单表 8 列；baseline 只增注释不改规则；reviews 索引只追加 M4 行 + 说明。 |
| 逻辑性 | ✅ | 状态机/证据链自洽：ISSUE-022→ISSUE-083 拆分、ISSUE-070 残项→ISSUE-020/M5、ISSUE-047 保持 in-progress（AC-12 待 M5）均有 REV/ISSUE 互证；073–083 与 05 §5.13.3 的 L 列表双向一致（台账每条引用 REV-020 编号、spec 侧 L 行引用 ISSUE 编号）；PROVENANCE base 修正与 main 实际历史（`12ab2f6`→docs rebase→`48d1b48`）一致；progress 当前阶段/下一步/阻塞与 M4 完成事实一致。 |
| 可读性 | ✅ | 台账/进度/索引行信息密度高且可扫读；progress 四块清晰；新增 ISSUE 行均带「影响 + 建议 + REV 依据」；README 状态段分层（已完成/尚未实现/目录）；spec README 与 PROVENANCE 的批次记录可点击核对；无模板占位/无 TODO 顶替台账。 |

## 3. 发现清单

| # | 级别 | 位置 | 问题 | 建议 | 状态 |
|---|---|---|---|---|---|
| 1 | 建议 | `doc/spec-v1.2/04-节点接口与扩展点.md:441`（§4.3.7 守卫行）vs `doc/issues.md` ISSUE-083 | 04 §4.3.7 只写「建议登记 ISSUE 跟踪」，未引用已登记的 **ISSUE-083**；台账侧 ISSUE-083 反向引用 §4.3.7，形成单向引用 | M5 触及 04 docs 时在 §4.3.7 补「见 ISSUE-083」编号（或由 ISSUE-083 描述保留即可）；不影响本收尾（docs 正文已在 REV-023 合并，收尾不宜再改 spec 语义） | open（M5 docs） |
| 2 | 建议 | `doc/reviews/README.md:29`（REV-024 行） | 索引在本记录提交前已预写「通过」，且 `REV-024-docs-m4-wrapup.md` 文件在收尾分支中尚不存在（评审时） | 本评审结论已确认为「通过」；**合并前置**：Lead 先提交本评审记录再跑 `merge.sh`（脚本要求记录文件存在且含「结论：通过」）。与 REV-016 C10 同类流程项，非内容缺陷 | open（合并前置） |
| 3 | 备注（跨模块） | `README.md`（process 模块） | 根 README 属 process，作者 `lead` 同时为 process 负责人；提交正文已声明跨模块 + reviewer-b 复核 | 本评审确认 README 内容真实、无越界；AGENTS §2 的 Lead 声明要求已满足，无需额外动作 | 无需处理 |
| 4 | 备注 | `README.md:20` vs `doc/test-baseline.txt` | README 写「423 passed / pytest 84 passed」，未列 2 个 ignored（harness 真模型 smoke + e2e AC-08b 手动）；baseline 注释已明确列出 2 ignored | 可接受（README 面向门面，ignored 属手动/CI 不跑项；baseline 已如实记录）；如需更精确可写「423 passed / 2 ignored」 | 无需处理 |
| 5 | 备注 | `doc/spec-v1.2/PROVENANCE.md:43,67`（`c2119dd..573676a` 修正） | 原写「本批次最终 base = fda996c」不准确（实际 docs 分支 rebase 到 `12ab2f6` 后合并）；现已改为「评审时 base `fda996c`；合并时 rebase 至 `12ab2f6`（REV-023 记录提交）」 | **修正准确**：main 历史为 `12ab2f6`（REV-023 记录）→ `49b1b5f`/`48d1b48`（docs 合并）；无需动作 | 无需处理 |
| 6 | 备注 | `doc/issues.md`（ISSUE-073..083） | 11 条新 ISSUE 均已登记且与本评审在 REV-020 §5 / REV-023 §5 给出的清单对应（073 preserve_order、074 8KiB 非法 JSON、075 preview、076 node_timeout、077 symlink、078 futures-util、079 工具并发、080 discover reason、081 函数>60、082 L2 记录、083 tool docstring 守卫） | 与 docs REV-023 §5.13.3 双向覆盖，Lead 收尾目标达成；M5 按 progress 下一步处理 | 无需处理 |
| 7 | 备注 | `doc/spec-v1.2/08-审计关闭矩阵.md` | ISSUE-070 的 08 历史行注记仍留待 M5 docs（ISSUE-070 已 fixed 并在描述中明示残项） | 已明示且不阻塞 M4；M5 docs 批次处理（随 ISSUE-020） | open（M5） |

> 无阻塞项、无重要项；#1/#2 为建议/合并前置（#2 属流程，本评审记录提交后自动满足），#3–#7 为备注。

## 4. 验证证据

### 4.1 范围、分支与提交元数据

```bash
cd /Users/zyzs/lczyzs/code/zgraph/.worktrees/docs-lead
git status --short                        # 空（worktree clean）
git branch --show-current                 # docs/docs-m4-wrapup
git merge-base origin/main HEAD           # 48d1b48c7165238e99a5b4568e60e2020096bc19（= base = origin/main，基于最新 main）
git rev-parse HEAD                        # 573676a19e8e262b59117e124d0467cb5664c7ca
git diff --shortstat 48d1b48..573676a     # 8 files changed, 63 insertions(+), 40 deletions(-)
git diff --name-status 48d1b48..573676a   # M README.md doc/index.md doc/issues.md doc/progress.md doc/reviews/README.md doc/spec-v1.2/{PROVENANCE,README}.md doc/test-baseline.txt
git diff c2119dd..573676a                 # 仅 PROVENANCE §2 第7条 + §3 共 2 行 base 表述修正（Lead 更正，已核实）
git log -1 --format='%H|%an <%ae>|%cn <%ce>|%s' HEAD
# chenyuxiangg <1007423165@qq.com> / chenyuxiangg <1007423165@qq.com> / docs(docs): M4 收尾——进度/台账/README/评审索引/基线/spec 索引同步
# 正文含 Review: REV-024 / Agent: lead，并声明 README（process）跨模块授权
```

- 未改代码/测试/CI/`doc/spec-v1.2/` 正文语义（仅 spec README 索引行数与 PROVENANCE 记录）。

### 4.2 必跑命令与结果（worktree 内，干净 `CARGO_TARGET_DIR=/tmp/rev024-target`）

| 命令 | 结果 |
|---|---|
| `cargo test --workspace --quiet` | **exit 0：423 passed / 0 failed / 2 ignored**（与 README/baseline 记录一致；2 ignored = harness 真实模型 smoke + e2e AC-08b 手动；`/tmp/rev024-ws-final.log`） |
| `PYTHONPATH=python .venv/bin/python -m pytest` | **84 passed**（`/tmp/rev024-pytest.log` + 复跑确认） |
| `bash scripts/dev/check.sh` | **CHECK PASSED**（身份/fmt/禁用文件/进度 ≤30 等；`/tmp/rev024-final-check.log`） |
| `git status --short` | 空 |

### 4.3 spec 索引 / PROVENANCE 可核验性（独立复算）

```bash
cd doc/spec-v1.2 && shasum -a 256 0*.md && wc -l 0*.md
# 实测行数：00 292 / 01 302 / 02 913 / 03 388 / 04 895 / 05 1418 / 06 985 / 07 213 / 08 412（合计 5818）
# 与 doc/spec-v1.2/README.md 行数表 9/9 一致；与 PROVENANCE §3 的 9 个 SHA-256 + 行数 9/9 一致（diff 空）
```

- PROVENANCE base 表述修正核对：`git log --oneline origin/main` 显示 `12ab2f6`（REV-023 记录提交）→ `49b1b5f` → `48d1b48`（docs 合并），与修正后的「评审时 `fda996c`；合并时 rebase 至 `12ab2f6`」一致。

### 4.4 台账 / 进度 / 索引 / 基线结构核对（脚本）

| 项 | 结果 |
|---|---|
| `doc/issues.md` | **83 行**、ID `ISSUE-001..083` **连续无重号**；每行 9 个 `|`（8 列）一致；新增 073–083 全存在 |
| 073–082 ↔ 05 §5.13.3 L1–L10 | 076(L1)/077(L2)/078(L3)/074(L4)/075(L5)/073(L6)/079(L7)/081(L8)/080(L9)/082(L10) —— **逐项一致**；083=工具 docstring 守卫（REV-023 遗留） |
| status 变更依据 | 022 fixed（REV-013/023，残项转 083）；047 in-progress（REV-015/022，AC-12 待 M5）；067 fixed（REV-023）；068 open（REV-017 部分）；070 fixed（REV-018/023，残项 08 注记→M5/ISSUE-020）；071 open（REV-017 部分）——均有 REV 证据 |
| `doc/progress.md` | 总 26 行 / 非空 **21 行**（≤30）；四块齐全；docs 行负责人 `docs-writer / lead`；计数 423/2 ignored、pytest 84 与实跑一致 |
| `doc/reviews/README.md` | **25 行（REV-000–024）**；结论与各记录 §1 一致（REV-020 修订说明、REV-023 通过）；**REV-024 链接文件待本记录提交后存在（合并前置）**；REV-022/023 链接已存在 |
| `doc/test-baseline.txt` | 仅新增 1 行注释；失败名额仍 0（`merge.sh` 用 `grep -v '^#'`，注释无害） |
| `doc/index.md` | reviews 行更新为 REV-000–024（M4 为 017–024），与索引一致 |
| `README.md` | M0–M4 完成 / M5 未实现边界准确；目录表登记 harness/tool_runner/harness_ac08；计数与实跑一致 |

## 5. 遗留问题

- **合并前置（流程）**：#2 本评审记录提交（Lead 在 main 工作区提交 `doc/reviews/REV-024-docs-m4-wrapup.md`）后 `doc/reviews/README.md` 的 REV-024 链接才有效；`merge.sh` 要求记录文件存在且含「结论：通过」。
- **M5 跟进**：#1 04 §4.3.7 可补 ISSUE-083 编号；#7 08 审计矩阵旧语义注记随 ISSUE-020/M5 docs；ISSUE-073..083 按 progress 下一步在 M5 收敛。
- **跨模块声明**：README（process）由 Lead 提交并在提交正文声明、reviewer-b 复核——本记录 #3 已确认，无需额外动作。
- 本评审未修改任何文件；REV-024 记录为本分支唯一新增文档（需 Lead 提交）。

## 6. 评审签署

- M4 收尾内容（progress/issues/reviews 索引/README/spec 索引/PROVENANCE/基线）**事实准确、可核验、与 M4 各评审记录交叉一致**；测试计数与 spec 索引经干净环境/独立复算验证。
- **结论：通过**。记录提交后允许 `merge.sh docs/docs-m4-wrapup`，M4 里程碑收尾。
