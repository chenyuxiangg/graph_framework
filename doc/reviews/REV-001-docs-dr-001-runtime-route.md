# REV-001 · docs · DR-001 技术路线 + 架构蓝图 v2 + spec v1.2 入库

- **分支**：`feat/docs-dr-001-runtime-route`
- **作者**：`lead`（首版 `3748379`；条件回填 `880af6a`，`Agent: lead`）
- **评审人**：`docs-reviewer`（独立评审，非作者）
- **变更范围**：`origin/main..880af6a`（首版 16 files, +6755/−16；复核差异 `fb45307..880af6a`：37 files, +3957/−136）
- **评审时间**：2026-10-29（初评 `3748379`）；2026-10-29（复核 `880af6a`）
- **关联**：ISSUE-014 / ISSUE-016 / ISSUE-019 / ISSUE-020 / ISSUE-021 / ISSUE-022；DR-001 §2/§5/§6；architecture-v2 §2–§8；spec 03 §3.9、04 §4.3.6、05 §5.6、06 §6.10、07 AC-05/06/07/09/12

## 1. 结论

**结论：有条件通过**

复核 `880af6a` 后：C1（L2 可复现）、C3（Harness/工具/执行模型/ctx/门控/AC 映射）、C4（协议 v1）、C6（模块/里程碑/工具链）已关闭；C5 已建立 PROVENANCE/REFERENCES 且 9 个 SHA-256 全部一致，仅剩行数/源稿尺寸等数据收尾；C2 的超时语义已按 D1 写死（DR §6、AC-06 断言、03/04/06/07 banner），但 00 顶部无 banner、AC-06 标题仍写 stop-waiting、spec README 正文仍残留与 v2 相反的旧陈述（#18）。

另有两项重要收尾：新错误码未登记到 spec 00 §0.4（#19）、PROVENANCE 行数表全部 +1（#17）。以上均为单行/单表级文档修正，不影响 DR-001 路线与 v1.2 语义判断；关闭 §6 的 C2/C5 残留并登记 #19 后，追加修订段将结论改为通过，方可进入 `merge.sh`。

## 2. 四维评审

| 维度 | 结论 | 依据（文件/行/测试） |
|---|---|---|
| 功能正确性 | ✅ | 用户四条决策均已落地：产品定位/Agent 可选见 `DR-001:9`；Rust + Python 子进程见 `DR-001:23-24`、`architecture-v2:7-38`；v1.1 语义 + 去粗取精见 `DR-001:22,37-50`、`spec-v1.2/`；工程约束由 AGENTS/worktree 流程承接。可复现从口号变为可执行承诺：L1/L2/L3 命名、`--record`/`replay`、存储增量、L3 不承诺见 `architecture-v2:157-191`，DR-001 D6/O-3（`DR-001:27,68`）与 ISSUE-016 已同步。 |
| 代码结构合理性 | ⚠️ | 文档组织与链接已大幅收敛：相对 markdown 链接 0 断链；spec 9 分册 + PROVENANCE + REFERENCES + `doc/analysis/` 外部材料齐全；02:5 断链已修复。残留：`doc/index.md:8,14` 未收录新入库的 analysis 材料/PROVENANCE/REFERENCES；spec 00 顶部无 v2 banner 而 §8 D10 修改 00 §0.3（#18/#20）。 |
| 逻辑性 | ✅ | 协议 v1 形成闭环：seed 进 `run` 载荷、`PYTHONHASHSEED` 由 supervisor spawn 设置、hello 5s/hello_ack、exit 0 无 result、result 后 1s 退出、stderr 排空、stdout/stdin 隔离、cancel reader/信号升级、半行/帧超时（`architecture-v2:85-142`）；§3.5 检查单纳入 M0 DoD（`architecture-v2:199`）；AC-11→M2、AC-12→M5（`architecture-v2:206-221`）。剩余 #19（新错误码未进 spec 00）属契约收尾，不改变调度/状态机逻辑。 |
| 可读性 | ⚠️ | 中文/表格总体清晰，偏差表 D1–D12 可读性好。残留：PROVENANCE 行数表每项 +1（表合计 5415 vs 实际 `wc -l` 5406）、源稿尺寸 58 KB 与 80,551 B 不一致（#17）；07 AC-06 标题、`05:12` 旧句（banner 已限定）、spec README 正文旧描述（#18）；`architecture-v2:140` "卡片/半行帧" 应为"卡帧"（#21）。 |

## 3. 发现清单

| # | 级别 | 位置 | 问题 | 建议 | 状态 |
|---|---|---|---|---|---|
| 1 | 阻塞 | `doc/architecture-v2.md:157-191`；`doc/spec-v1.2/06-存储与可观测.md:106,111-137,669-673`；`doc/decisions/DR-001-runtime-route.md:27,68`；`doc/issues.md:23` | 初评：L2 可复现无可执行落地（spec 06 "v1 不做图重放"、runs 表无记录列、CLI 无 replay；L2 名实不符；粒度三处不一致） | 已按 architecture §5.1–5.4 重写：L2 更名"结果可重放（recorded outputs）"、§5.2 存储增量（runs 增列 + recorded_outputs/llm_calls）、§5.3 `--record`/`replay` 契约、§5.4 边界；DR D6/O-3 与 ISSUE-016 同步；`--dry-run` 删除 | fixed（880af6a） |
| 2 | 阻塞 | `doc/architecture-v2.md:120,153,243`；`doc/spec-v1.2/03-调度器与生命周期.md:282-286`；`doc/spec-v1.2/00-总纲.md:3,113,119,233`；`doc/spec-v1.2/06-存储与可观测.md:131`；`doc/spec-v1.2/07-验收用例.md:34,119,125`；`doc/decisions/DR-001-runtime-route.md:78-90` | 初评：超时 stop-waiting vs kill 冲突且未登记 | D1 写死"超时=杀进程组、NodeAbandoned 不产生、abandoned_tasks 恒 0、新增 killed_workers"；AC-06 断言改写；DR §6 九项机制替换；03/04/06/07 顶部 banner。**残留**：00 无 banner（D10 修改 00 §0.3）、AC-06 标题仍 stop-waiting、README 正文旧语义（见 #18） | partial（见 #18） |
| 3 | 阻塞 | `doc/spec-v1.2/05-AgentHarness.md:12,774,797-798`；`doc/architecture-v2.md:19,131,235-254` | 初评：Harness/工具执行位置自相矛盾 | D3 明确"Harness 在 Rust、tool 每次一 worker 子进程、无 import 缓存/共享信号量"；§4 执行模型表更新；02–07 banner 指回 §8；05:12 由 banner 限定 | fixed（880af6a） |
| 4 | 阻塞 | `doc/spec-v1.2/04-节点接口与扩展点.md:197-200,373,379-386`；`doc/spec-v1.2/03-调度器与生命周期.md:203`；`doc/architecture-v2.md:151-152,244-250`；`doc/spec-v1.2/07-验收用例.md:130,148,172` | 初评：Python 执行模型/并发/ctx API 变更未登记 | D2/D4/D5/D8 逐条映射（每节点进程、全局/每 run 上限、exec_class 忽略、ctx now/random/seed、AC-09/12 重标定）；§4 表更新；AC-07/09/12 改写 | fixed（880af6a） |
| 5 | 阻塞 | `doc/architecture-v2.md:85-142,199`；`doc/spec-v1.2/07-验收用例.md:121-126` | 初评：协议 v1 空洞（seed/PYTHONHASHSEED/exit 0 无 result/hello 超时/stderr/stdout-stdin/cancel/半行） | §3 重写：帧/版本、消息表、退出码封闭表、生命周期与资源、§3.5 十项检查单；检查单入 M0 DoD | fixed（880af6a） |
| 6 | 重要 | `doc/spec-v1.2/PROVENANCE.md:28-40`；`doc/spec-v1.2/README.md:3`；`doc/spec-v1.2/02-DSL与校验.md:5`；`doc/spec-v1.2/REFERENCES.md`；`doc/analysis/**` | 初评：spec 无来源/hash、上游引用断链、外部材料未入库 | PROVENANCE（源稿/冻结过程/回填/hash）、REFERENCES 映射、02:5 修复、决策记录/可行性/spike/调研入库；哈希 9/9 一致。**残留**：行数与源稿尺寸数据错（#17） | fixed（880af6a，残留见 #17） |
| 7 | 重要 | `doc/architecture-v2.md:65-81`；根 `AGENTS.md` §2 | 初评：新模块表与 AGENTS §2 无映射/退役说明、tests 归属不清 | §2.2 旧 id→新归属表、M0 冻结时间点、tests/legacy 归属与 CI 排除；M0 DoD 同步 AGENTS/worktree.sh | fixed（880af6a） |
| 8 | 重要 | `doc/architecture-v2.md:197-221` | 初评：AC-11/AC-12 未挂里程碑、M0 未适配 check/merge/test-baseline | §6.1 M0 DoD 增加双栈门禁；§6.2 AC→里程碑映射（AC-11→M2、AC-12→M5） | fixed（880af6a） |
| 9 | 重要 | `doc/architecture-v2.md:248`；`doc/spec-v1.2/07-验收用例.md:15,29-31,58-66`；`doc/issues.md:21` | 初评：门控替换已登记但 AC fixture 未同步 | D6 明确 v1 内置 op（check_output/compare/manual）、AC-02/04 fixture 改用内置 op、Python 门控延后（ISSUE-014） | fixed（880af6a） |
| 10 | 重要 | `doc/architecture-v2.md:157-165,185` | 初评：L2 过度承诺、L3 用不存在的 `--dry-run`、"worker 幂等"未入契约 | L2 限定"不重算/不重复副作用"；L3 明确不承诺、`--dry-run` 不实现；worker 副作用声明转 ISSUE-022 | fixed（880af6a） |
| 11 | 建议 | `doc/decisions/DR-001-runtime-route.md:46-47`；`doc/issues.md:28` | 初评：复用矩阵 `rollback` 静默丢弃；legacy 基线去向未写 | `rollback` 不迁移已说明；ISSUE-021 跟踪 legacy 基线退役；187 测试作行为参考保留 | fixed（880af6a） |
| 12 | 建议 | `doc/decisions/DR-001-runtime-route.md:16` | 初评：v1.2 行数 5,467 与实测不符 | DR 改为 5,406（与 `wc -l` 一致） | fixed（880af6a） |
| 13 | 建议 | `doc/spec-v1.2/08-审计关闭矩阵.md:156-157,400`；`doc/issues.md` | 初评：I-37 与 M-02/M-05 未进问题台账 | ISSUE-020~022 已新增；I-37/M-02/M-05 仍只在 spec 08 §8.6 | partial |
| 14 | 建议 | 根 `README.md`；`doc/repo-status.md` | 初评：入口 README 仍写 Python 技术栈 | M0 DoD 已增加"根 README 标注技术路线"（`architecture-v2:199`），跨模块留到 M0 | fixed（880af6a） |
| 15 | 建议 | `doc/reviews/README.md`；提交 `3748379` | 初评：评审索引未加 REV-001；单提交过大 | 本次复核更新索引；大件文档拆分作为后续习惯 | fixed（本次） |
| 16 | 建议 | `doc/analysis/spec-audit-v1.1.md:7-8` | 初评：审计报告用仓库外绝对路径作核对基准 | PROVENANCE 声明源稿外部不可复核；REFERENCES 建立映射；归档 banner 已加 | fixed（880af6a） |
| 17 | 重要 | `doc/spec-v1.2/PROVENANCE.md:28-40,46-47`；`doc/spec-v1.2/README.md:3` | PROVENANCE 行数表每项恰比实际 `wc -l` 多 1（表合计 5,415 vs 实际 5,406），其 §4 的 `wc -l` 核验命令会输出与表不一致；源稿尺寸 58 KB 与 PROVENANCE 80,551 B 不一致；冻结日期 2026-10-29 与首版提交日 2026-10-28 不一致 | 用 `wc -l` 重算并统一 9 行/合计；源稿尺寸与冻结日期取一致口径；完成后重跑 §4 命令自校验 | open |
| 18 | 重要 | `doc/spec-v1.2/00-总纲.md:3,113,119,233`；`doc/spec-v1.2/07-验收用例.md:34,119,113-116,132`；`doc/spec-v1.2/README.md:27,47,53,76` | C2 收尾未完成：①00 顶部无 v2 banner，而 §8 D10 的 spec 位置包含 00 §0.3；②AC-06 标题/总表仍"stop-waiting"；③AC-05 仍写"解释器仍健康：同一进程随后再跑 AC-01"（v2 是每节点新进程）；④AC-07 反例保护仍写"把 Python 调用放回事件循环线程"（v2 无进程内 Python 调用）；⑤spec README 正文仍写 spawn_blocking/stop-waiting/被放弃任务记账，且"进程隔离/强制终止 Python 未实现"与 D1/D4 正面冲突 | ①给 00 加 banner（或论证 D10 不改 00）；②AC-06 标题/表行改为 kill；③AC-05/07 按子进程模型改写；④spec README 的 v1.2 历史表加"v2 见 §8"标注，删除/标注"未实现"行 | open |
| 19 | 重要 | `doc/architecture-v2.md:101,116,118,181,249-252`；`doc/spec-v1.2/00-总纲.md:124-185` | 协议/CLI 新错误码 `NODE_WORKER_HANDSHAKE` / `NODE_WORKER_PROTOCOL` / `NODE_WORKER_IMPORT` / `REPLAY_RECORD_MISSING` 在 spec 00 §0.4 与 CLI 命名空间（00:185）中未登记；D10 只写"NodeError 增 NodeWorker 变体"，未列码 | 在 D10/ISSUE-020 中列明 4 个码的域/触发/退出码，或在 00 §0.4 增补；M0 前完成 | open |
| 20 | 建议 | `doc/index.md:8,14` | 新增的 `doc/analysis/decision-record-01-scope.md`、可行性评估、spike、两份调研、`spec-v1.2/PROVENANCE.md`、`REFERENCES.md` 未进文档索引；reviews 行仍只写 REV-000（本次已更新 reviews/README） | doc/index 增 analysis 子目录条目与 PROVENANCE/REFERENCES 指针；reviews 行同步 REV-001 | open |
| 21 | 建议 | `doc/architecture-v2.md:140,171`；`doc/architecture-v2.md:106,116,181` | 文字级：§3.5 "卡片/半行帧" 应为"卡帧/半行帧"；§5.2 "新增列 graph_hash" 实际 spec 06 `runs` 已有该列；§3.3 "result 后 1s 未退出"判协议错误但未写 supervisor 随后杀进程 | 修正措辞；`graph_hash` 从"新增列"删去；补一句"协议错误后杀进程组" | open |

## 4. 验证证据

```bash
cd .worktrees/docs-agent-lead
git log --oneline -2                                        # 880af6a / fb45307
git diff --stat fb45307..880af6a                            # 37 files changed, 3957 insertions(+), 136 deletions(-)
git merge-base --is-ancestor origin/main HEAD; echo $?      # 0
git show -s --format='%B' 880af6a                           # 含 Review: REV-001 / Agent: lead
bash scripts/dev/check.sh                                   # CHECK PASSED
python3 <按 PROVENANCE 表逐项比对>                          # SHA-256 9/9 一致；行数 9/9 比实际 wc -l 多 1
python3 <doc 内 markdown 相对链接检查脚本>                  # 0 断链（含 doc/analysis 与 spec-v1.2 新增文件）
/tmp/gf_venv/bin/python -m pytest -q --tb=no                # 25 failed / 162 passed（187 项）
grep -v '^#' doc/test-baseline.txt | sed '/^$/d' | wc -l    # 25（无新增失败、无新增修复）
grep -rInE "sk-[A-Za-z0-9]{16,}|API_KEY=|PRIVATE KEY" doc/   # 0 命中（新增 analysis/spike 材料无密钥）
```

- 测试结果：`25 failed / 162 passed`，与 `doc/test-baseline.txt` 的 25 项完全一致，无新增失败；本次仍为 docs-only，`doc/analysis/spike/**` 为归档材料，不进入 pytest 收集。
- 环境说明：默认 `python3` 无 pytest/uv（ISSUE-003），使用 `/tmp/gf_venv/bin/python`（Python 3.13.0 + pytest 9.2.0）；合并时建议 `PYTHON_BIN=/tmp/gf_venv/bin/python`。
- 手工验证：提交作者/身份正确、基于 `origin/main`、变更全在 `doc/**`、`CLAUDE.md` 未改；spec 00–08 SHA-256 与 PROVENANCE 一致（行数除外，见 #17）；新增分析材料均带归档 banner。
- 评审局限：源稿 HTML/提取文本在仓库外，无法核对"语义未被篡改"；PROVENANCE 的 hash 提供入库后完整性，不能证明与外部源稿一致。此局限已在 PROVENANCE §5 声明。

## 5. 遗留问题

- C2/C5 尚未完全关闭（#18/#17）；`merge.sh` 仅接受"通过"结论，故本分支当前**不可合并**。建议一轮小修（6 处单行/单表）后由评审人追加修订段并改为通过。
- 建议把 #19 纳入 ISSUE-020 范围（或新增 ISSUE），把 #17 在 PROVENANCE 内修正；#20/#21 为建议项，可随本次收尾一起做。
- 已确认无隐藏冲突：门控（ISSUE-014）、复现粒度（ISSUE-016 fixed）、池化（ISSUE-017）、spec 回写（ISSUE-020）、legacy 基线（ISSUE-021）、worker 幂等（ISSUE-022）均已登记。
- 本次复核未 push、未合并、未改作者其他文件；仅修改本评审记录与 `doc/reviews/README.md` 索引。

## 6. 条件关闭记录

| 条件 | 关闭证据 | 关闭日期 |
|---|---|---|
| C1 | architecture §5.1–5.4：L2 更名"结果可重放（recorded outputs）"；§5.2 存储增量（runs 增列 + recorded_outputs/llm_calls）；§5.3 `--record`/`replay` 契约与 `REPLAY_RECORD_MISSING`；§5.4 边界；DR-001 D6/O-3 同步；ISSUE-016 fixed | 2026-10-29 |
| C2 | 部分关闭：§8 D1（超时=kill、NodeAbandoned 不产生、abandoned_tasks≡0、killed_workers）、DR-001 §6 九项替换、07 AC-06 断言改写、03/04/06/07 banner 已完成；**未完成** 00 banner、AC-06 标题、AC-05/07 措辞、spec README 旧行（#18） | —（残留 #18） |
| C3 | §8 D2/D3/D4/D5/D6/D8 + §4 执行模型表 + 02–07 banner；05:12 由 banner 限定；自定义门控保留 ISSUE-014 | 2026-10-29 |
| C4 | §3.1–3.5 全量重写：seed/PYTHONHASHSEED/hello 5s+ack/退出码封闭表/stderr 排空/stdout-stdin 隔离/cancel reader/半行帧；§3.5 检查单入 M0 DoD | 2026-10-29 |
| C5 | PROVENANCE（源稿/冻结/回填/hash）、REFERENCES（历史引用映射）、02:5 断链修复、决策记录/可行性/spike/调研入库；SHA-256 9/9 一致；**未完成** PROVENANCE 行数/源稿尺寸一致性（#17） | —（残留 #17） |
| C6 | §2.2 旧 id→新归属 + M0 冻结点；§6.1 M0 双栈门禁；§6.2 AC→里程碑（AC-11→M2、AC-12→M5）；ISSUE-020~022 | 2026-10-29 |

## 7. 复核修订（2026-10-29，`880af6a`）

- **复核范围**：`fb45307..880af6a`，37 files, +3957/−136；首版评审内容保留，本段记录关闭判断与残留。
- **已验证的 lead 声明**：`check.sh` PASS；相对链接 0 断链；spec 9 文件 SHA-256 与 PROVENANCE 一致；全量 pytest 25 failed/162 passed（与基线一致）；commit 含 `Review: REV-001` / `Agent: lead`；新增 spike/调研材料带归档 banner 且无密钥。
- **关闭判断**：C1/C3/C4/C6 关闭；C5 实质关闭（hash 与映射成立），但 PROVENANCE 行数数据需修正；C2 语义已关闭（D1/DR/AC-06 断言/banner），文档收尾未完成。
- **转通过所需最小动作**：①00 顶部 banner（或 D10 说明）；②07 AC-06 标题/表行、AC-05/07 措辞按子进程模型改写；③spec README 旧行加 v2 标注（尤其删除"进程隔离/强制终止 Python 未实现"）；④PROVENANCE 行数与源稿尺寸用 `wc -l`/实际字节重算；⑤登记 4 个新错误码（#19）；⑥doc/index 增补（#20）。
- **结论**：维持"有条件通过"。完成上述动作并追加关闭证据后，本评审追加修订段改为通过；届时方可执行 `merge.sh`。
