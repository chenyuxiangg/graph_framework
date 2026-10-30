# REV-061 · process/docs · M6 收尾（台账/进度/索引/README/baseline、DR-003 编号、ISSUE-096/097、AC-12 10 次重跑）

- **分支**：`docs/docs-m6-wrapup`
- **作者**：`lead`（提交 `4ddca05`）
- **评审人**：`reviewer-a`（**≠ 作者**；主评审）
- **变更范围**：`main(b76d25d)..4ddca05`，9 文件 +49/−33，**process + docs**：`scripts/dev/check.sh`、`scripts/dev/merge.sh`、根 `README.md`、`doc/{progress.md,issues.md,index.md,test-baseline.txt}`、`doc/reviews/README.md`、`doc/decisions/DR-003-m6-scope-and-waves.md`；无 crate 代码改动；worktree clean；base 即当前 `origin/main`（无 rebase 需求）
- **评审时间**：2026-10-31
- **关联**：REV-061；DR-003；ISSUE-096（merge 门禁 e2e 旧 bin）/ISSUE-097（check.sh refactor 白名单）；07 AC-12；M6 全部 REV-039–060

## 1. 结论

**结论：有条件通过**（C1：DR-003 §1 的 ISSUE-080 仍映射到未创建的 REV-051，与同文件 §2「REV-051 未创建、ISSUE-080 并入 REV-057」矛盾；修正后转「通过」）

其余评审点（merge.sh/check.sh 脚本正确性、台账 081/096/097、reviews 索引、progress/README/index/baseline、DR-003 §2 编号收口）均通过；S1–S4 为可读性/口径建议（§3）。

## 2. 四维评审

| 维度 | 结论 | 依据（文件/行 + 实测） |
|---|---|---|
| 功能正确性 | ✅（脚本/台账） | ① **merge.sh**：`cargo build -p zgraph-cli --bin zgraph --locked` 插入在 `cargo test --workspace` **之前**（`:79-83`），失败 `|| die`；随后 `cargo test`（`:82-83`）、`cargo clippy --workspace --all-targets --locked -- -D warnings`（`:86-88`）顺序与失败传播正确，与 REV-058 门禁不冲突；`bash -n` 通过。② **check.sh**：分支白名单加 `refactor/*`（`:41`），与 `worktree.sh`（`TYPES="feat fix test docs chore refactor"`）及 AGENTS §1 一致；`bash -n` 通过。③ **台账**：ISSUE-081 → fixed（REV-053 harness 最大 46 行 + REV-054 cli 13→0）、ISSUE-096 → fixed（本分支 merge.sh build 修复）、新增 ISSUE-097 → fixed（check.sh）；编号 001–097 连续无重复；`。。` 等格式残留 0。④ **reviews README**：REV-046–061 行与记录文件一一对应（本评审记录 `REV-061-process-m6-wrapup.md` 由本次提交创建；REV-051/059 未创建已在 M6 段注明；REV-058/060 计划外插入已说明）。⑤ **DR-003 §2**：REV-051（080 并入 057）/REV-059 未创建、REV-058/060 计划外插入、wrapup=REV-061 已收口。⑥ **baseline**：新增 M6 收尾行以 `#` 开头，merge.sh 解析 `grep -v '^#'` 不受影响。⑦ **AC-12**：测试 `ac12_ten_same_graph_runs_are_isolated_and_bounded` 真实存在（`tests/e2e/ac12.rs:1027`）；commit body 记录 10/10 通过（每次 2 passed/0 failed、4.17–4.22s），本次未独立复跑（见 §4 说明）。**唯一问题：DR-003 §1/§2 的 080 编号矛盾（C1）** |
| 代码结构合理性 | ✅ | 收尾类变更单一职责：脚本门禁（process）+ 台账/进度/索引/README/baseline（docs/process）+ DR-003 编号收口；单提交 9 文件 +49/−33 无越界（未改 crate）；`progress.md` 19 行 ≤30、保留「当前阶段/里程碑表/下一步/阻塞」四块；docs 索引与 reviews 索引更新范围一致 |
| 逻辑性 | ✅（除 C1） | merge.sh 顺序满足 ISSUE-096 目标（先保证 CLI 裸 bin 新鲜，再 e2e/cargo test），且 clippy 门禁仍在 test 后执行；check.sh 与 worktree/AGENTS 白名单一致；台账状态与 main 已验证的 REV 记录/本分支脚本修复对得上；REV 编号叙事（039–061，051/059 未创建，058/060 计划外）与 reviews README/DR-003 §2 一致。**C1**：DR-003 §1 第 24 行 ISSUE-080 仍写「W2 / REV-051（docs 主责）」，与 §2 第 52/63 行「REV-051 未创建、080 并入 REV-057」冲突 |
| 可读性 | ✅ | progress 精简且给出 M6 收尾 REV 表；README 状态段按波次列关键交付；DR-003 收口说明清晰列出计划外插入/未创建编号；reviews README M6 段补充跨模块与 reviewer 事实。S1–S4 为可读性小建议 |

## 3. 发现清单

| # | 级别 | 位置 | 问题 | 建议 | 状态 |
|---|---|---|---|---|---|
| C1 | **重要** | `doc/decisions/DR-003-m6-scope-and-waves.md:24` vs `:52,:63` | §1 的 ISSUE-080 行仍写「W2 / **REV-051**（docs 主责）」；但同文件 §2 明写「（未创建）REV-051 / ISSUE-080 并入 REV-057」及注记「REV-051（ISSUE-080 并入 REV-057）与 REV-059 未创建」。评审要点 5 明确要求 §1 与 §2 一致；当前 M6 最终决策记录对同一 ISSUE 给出两条不同 REV 编号 | 把 §1 ISSUE-080 行改为 `W2 / REV-057（并入 spec 回写；REV-051 未创建）`，与 §2 一致（其余 §1 映射已核对一致） | **待关闭（条件）** |
| S1 | 建议（低） | `doc/reviews/README.md:65`、`DR-003…:61` | REV-061 行的分支列写计划名 `docs/process-m6-wrapup`，实际分支/评审记录为 `docs/docs-m6-wrapup`（DR-003 §2 为计划表可保留计划名，但 reviews 索引应可定位；REV-053 计划名 `refactor/harness-m6-split` 实际为 `chore/harness-m6-split` 亦属计划/实际差异） | reviews README 的 REV-061 行改用实际分支 `docs/docs-m6-wrapup`；DR-003 §2 如需保留计划名，建议在表头注明「计划分支名，实际以评审记录为准」 | 非阻塞（建议随 C1 一并改） |
| S2 | 建议（低） | `doc/index.md:18` | reviews 范围写「REV-000–REV-036、REV-038–061…REV-051/059 未创建」，未写明 **REV-037 从未创建**（DR-003 §2 已注明） | 补「REV-037/051/059 未创建」或沿用 DR-003 口径 | 非阻塞 |
| S3 | 建议（低） | `README.md:17` | M6 已完成，但下方仍以「**已完成 M0–M5**：」起首列历史交付；易被读成 M6 未列 | 改为「**M0–M5 已完成交付（摘要）**」或「M0–M6」并注明 M6 见上段 | 非阻塞 |
| S4 | 备注 | AC-12 10 次重跑 | 10/10 仅见 commit body 记录（2 passed/0 failed、4.17–4.22s），本评审未独立复跑（按派单「无需跑 cargo」；合并门禁会跑全新 build+workspace test+clippy） | 以 merge 门禁日志兜底；如 wrapup 后保留重跑日志可附 index/REV-061 | 已接受（非阻塞） |

## 4. 验证命令与结果

```bash
# 范围/干净度
git log --oneline origin/main..HEAD            # 1 commit（4ddca05）；base=origin/main=b76d25d（无需 rebase）
git diff b76d25d..HEAD --stat                  # 9 files +49/−33；全为 scripts/dev、doc/**、根 README
git status --porcelain                         # clean

# 脚本语法与顺序
bash -n scripts/dev/check.sh && bash -n scripts/dev/merge.sh   # 均 OK
sed -n '74,90p' scripts/dev/merge.sh           # build(79-80) → test(82-83) → clippy(86-88)，均 || die
grep -n refactor scripts/dev/check.sh scripts/dev/worktree.sh  # check.sh:41 + worktree.sh TYPES 一致

# 台账
# documents/issues.md：97 行、ISSUE-001..097 连续无重复/断号；081/096/097=fixed；`。。` 残留 0
# ISSUE-096 行「已修（REV-061，main）」——修复在本分支，合并后成立（与 097 同批；非提前宣称）

# reviews 索引/DR-003/进度/索引/baseline
# reviews/README.md 表：REV-000–061 中缺 037/051/059 为未创建（M6 段注明 051/059；037 见 DR-003）；
#   仅 REV-061 文件由本次评审提交创建；其余行文件均存在
# DR-003 §2：051 未创建/080 并入 057、058/060 计划外、wrapup=061 ✓；§1 ISSUE-080 仍写 REV-051 → C1
# progress.md 19 行（≤30）；README 状态段 M6 完成；index reviews 范围 REV-038–061；baseline 新行为 # 注释
# AC-12：tests/e2e/ac12.rs:1027 测试存在；10/10 依据 commit body（未独立复跑，§3 S4）

# e2e 裸 bin 依据
# tests/e2e/cli_support/mod.rs: 由 current_exe 推导 target/{debug,release}/zgraph，缺失时回落 cargo build；
# merge.sh 先 cargo build 保证新鲜 bin 与 e2e 解析路径一致（ISSUE-096 修复成立）
```

- 未跑 cargo（process/docs 分支；按派单说明，合并门禁会执行全新 build+test+clippy+python；本评审只做脚本语法/文档一致性核查与跨源抽查）。
- 跨模块声明：本分支改 `doc/**`（docs）与 `scripts/dev/**`、根 `README.md`（process），作者为 Lead（process 负责人）；评审人 `reviewer-a` 独立评审，符合 AGENTS §2 跨模块声明要求。

## 5. 遗留问题

- **C1**（DR-003 §1 ISSUE-080 仍写 REV-051，与 §2/实际 REV-057 矛盾）——修正后由 reviewer-a 追加条件关闭记录并改结论「通过」。
- S1–S3 为可读性/索引小建议，不阻塞；S4 已接受（AC-12 以 commit body + merge 门禁为准）。
- 无 crate/协议/测试代码改动；无代码风险。

## 6. 条件关闭记录

> 待 C1 修正（DR-003 §1 ISSUE-080 → REV-057）后追加，含复核结果。
