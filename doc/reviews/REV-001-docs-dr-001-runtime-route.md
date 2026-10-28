# REV-001 · docs · DR-001 技术路线 + 架构蓝图 v2 + spec v1.2 入库

- **分支**：`feat/docs-dr-001-runtime-route`
- **作者**：`lead`（提交 `3748379`，`Agent: lead`）
- **评审人**：`docs-reviewer`（独立评审，非作者）
- **变更范围**：`origin/main..3748379`（16 files changed, 6755 insertions(+), 16 deletions(-)）：`decisions/DR-001`、`architecture-v2.md`、`spec-v1.2/` 9 分册、`analysis/spec-audit-v1.1.md`、`index/issues/progress`
- **评审时间**：2026-10-29
- **关联**：ISSUE-014 / ISSUE-015 / ISSUE-016 / ISSUE-019；DR-001 §2/§5；architecture-v2 §3/§5/§6；spec 03 §3.9、04 §4.3.6、05 §5.6、06 §6.10、07 AC-05/06/07/09/12

## 1. 结论

**结论：有条件通过**

理由：用户四条决策在 DR-001 与架构蓝图中均有明确落点（见 §2 功能正确性），文档组织与 v1.2 全量入库的完成度高于本仓以往任何文档；但"可复现"承诺缺少可执行的存储/CLI 落地，worker 化后至少 4 处 v1.2 语义（超时/放弃、Harness 工具执行进程、Python 执行模型与并发、门控）与规格正文冲突或未登记，协议 v1 存在会导致实现返工的空洞，且 spec 的溯源与上游引用不可核验。以上须在合并前关闭 §6 的 C1–C6；关闭后追加修订段并将结论改为通过，方可进入 `merge.sh`。

## 2. 四维评审

| 维度 | 结论 | 依据（文件/行/测试） |
|---|---|---|
| 功能正确性 | ⚠️ | 用户决策 1–4 均有对应落点：产品定位与"Agent 可选"见 `DR-001:9`；Rust 内核 + Python 子进程 worker 见 `DR-001:23-24`、`architecture-v2:6-38`；v1.1 语义为核心 + 去粗取精复用矩阵见 `DR-001:22,37-50`；工程约束由本仓 AGENTS/worktree 流程承接。**但**"可复现"只有 L1/L2/L3 分级标题，L2 无存储/CLI 落地且与 spec 06"v1 不做图重放"冲突（F1）；worker 化后的超时、工具执行与 Python 执行模型未同步语义（F2–F4）。 |
| 代码结构合理性 | ⚠️ | `doc/index.md:5-16` 索引与新增文档目录成立；spec 9 分册（00–08）+ README + 审计报告全量入库，章节正文未见多余 DR 注入（仅 `spec-v1.2/README.md:1-7` 一处落地 banner），未发现明显篡改痕迹。**但** `spec-v1.2/02-DSL与校验.md:5` 相对链接断链，README/05/06/07/08 共 20+ 处引用未入库的 `决策记录-01`、`_spike/*`、`_research/*`，且无源版本/hash 说明（F6）；新模块表与 AGENTS §2 现有模块表无映射/退役说明（F7）。 |
| 逻辑性 | ⚠️ | v1.2 的状态机、Delivery、冷峻失败、trace 双通道、Harness 六状态机本身自洽，08 分册也主动暴露并回填了 I-01..I-37。**但**架构层的 NDJSON 协议存在 seed 缺失、退出码未定义分支、握手无超时、stderr 未排空、用户 stdout 污染协议等空洞（F5）；stop-waiting 与 kill 进程组的语义无法同时成立（F2）；L1/L2 命名与实现不一致（F1/F10）；AC-11/AC-12 未挂到里程碑、Rust 化后 check/merge 门禁未适配（F8）。 |
| 可读性 | ⚠️ | 中文表达、表格与分册编号总体清晰；术语（实例/run/节点）基本统一。**但**`spec-v1.2/05-AgentHarness.md:12`"工具执行永远在本进程内"与架构正面矛盾；`architecture-v2:93`"worker 启动方向 = supervisor → worker"表述自相矛盾；"可重算/重放"混用；大量指向仓库外材料的反引号路径让读者无法就地核对（F3/F6/F12）。 |

## 3. 发现清单

| # | 级别 | 位置 | 问题 | 建议 | 状态 |
|---|---|---|---|---|---|
| 1 | 阻塞 | `doc/architecture-v2.md:133-142`；`doc/spec-v1.2/06-存储与可观测.md:106,111-137,669-673`；`doc/decisions/DR-001-runtime-route.md:68`；`doc/issues.md:23` | **L2"结果可复现"没有可执行落地**：spec 06 明确"v1 不做图重放"（06:106），`runs` 表无 worker/sdk/python 版本、模型参数、seed、节点输出记录列（06:111-137），CLI 契约只有 validate/run/trace（06:669-673）；architecture §5 却称这些记录在"runs 表与 RunStarted 事件"中，并新增 `zgraph replay --use-recorded-outputs`。同时 L2 名为"结果可重算"，实现却是"replay 时用记录值替代执行"（arch:138），本质是读账本重放而非重算；记录粒度三处默认值不一致：DR O-3"记录 output"、ISSUE-016"原文+hash"、arch"hash + 可选原文"。 | 在 architecture §5 增加"可复现落地"小节：①声明对 spec 06 的增量（新表/列：recorded outputs、worker/python/sdk 版本、模型参数、seed）与迁移方式；②给出 `replay`/`--record` 的 CLI 契约（对齐 spec 06 §6.10）；③统一定义 L2 名称与默认粒度（建议"结果可重放（recorded outputs）"），DR/架构/ISSUE-016 三处同步；④登记 ISSUE。见条件 C1。 | open |
| 2 | 阻塞 | `doc/architecture-v2.md:113,129`；`doc/spec-v1.2/03-调度器与生命周期.md:282-286`；`doc/spec-v1.2/00-总纲.md:56,113`；`doc/spec-v1.2/04-节点接口与扩展点.md:142,382`；`doc/spec-v1.2/06-存储与可观测.md:131`；`doc/spec-v1.2/07-验收用例.md:125`；`doc/decisions/DR-001-runtime-route.md:72-74` | **超时/放弃语义冲突且未登记**：spec 03 §3.9.2 是 stop-waiting（Python 任务继续运行、发 `NodeAbandoned`、`abandoned_tasks += 1`，04:382 明言"这些不是实现建议，而是验收断言"）；architecture §3.3/§4 改为"超时 kill 进程组、实例 TimedOut"，被杀进程不存在"仍在运行"的放弃任务，与该事件定义、`abandoned_tasks` 列及 AC-06"NodeAbandoned 计数 ≥1"直接冲突。DR §6 验收映射只列"PyO3 → worker、emit → 协议"两项，未登记此语义变化。 | 二选一并写死：①采用 kill，则同步修改 00/03/04/06/07（定义 NodeAbandoned 在 kill 模型下的含义或废弃、修订 AC-06 断言）；②保留放弃记账，则说明 kill 的适用边界。无论哪种，都必须在 DR §6/README banner 的"机制替换"清单中加入此项并登记 ISSUE。见条件 C2。 | open |
| 3 | 阻塞 | `doc/spec-v1.2/05-AgentHarness.md:12,774,797-798`；`doc/architecture-v2.md:131`；`doc/spec-v1.2/README.md:1-7` | **Harness/工具执行位置自相矛盾**：spec 05 称"agent loop 与工具执行永远在本进程内"，工具模块 `spawn_blocking` 内一次性导入并缓存在进程内（05:774），且"工具与 Python 节点共享 I/O 信号量"（05:798）；architecture §4 明确"Harness 在 Rust 进程内；tool 执行 = worker 子进程（每次调用一进程）"。README 顶部落地 banner 只写"PyO3/进程内 Python 调用 → worker 子进程"，未覆盖工具生命周期、import 缓存、信号量、超时/镜像等具体契约，05 正文无任何标注，M4 实现者无法判断以哪份为准。 | 在 05 §5.1/§5.6 增加 DR-001 落地小节：逐条给出"进程内契约 → worker 子进程契约"映射（导入时机、错误映射、并发/超时、工具结果上限、是否允许跨调用缓存），并同步 architecture §4 的 M4 说明；或在 DR §5 把"工具执行模型"列为独立开放项并登记 ISSUE。见条件 C3。 | open |
| 4 | 阻塞 | `doc/spec-v1.2/04-节点接口与扩展点.md:197-200,373,379-386`；`doc/spec-v1.2/03-调度器与生命周期.md:203`；`doc/architecture-v2.md:119,128`；`doc/spec-v1.2/07-验收用例.md:130,148,172`；`doc/spec-v1.2/README.md:1-7` | **worker 化后的 Python 执行模型变更未登记**：spec 假定单解释器 + 进程内模块缓存（04:384、05:774），worker 模型是"每节点实例一进程"；spec 的 `PYTHON_CONCURRENCY_CPU=1`/`IO=4` 信号量（04:379、03:203）在 architecture 变为全局 worker 并发"CPU 核数×2"、每 run 8（arch:128）；spec 04 的 `ctx.cancel_requested/check_cancelled/node_deadline_remaining_ms/cancel_token`（04:197-200）未出现在 README 的 ctx 映射（只列 emit/log/get_var/user_id），而 architecture 新增 `ctx.now()/ctx.random()` 与 `seed` 约定（arch:119），spec 04 并无这些 API；AC-07 的 ticker 断言、AC-09 的并行耗时、AC-12 的"CPU=1/IO=4 并发上限"均来自 in-process spike（07:130,148,172），未说明在新模型下是否保留、如何重算。 | 增加一张"v1.2 假设 → worker 模型"对照表（含项 1–4、门控、取消、并发、ctx API、验收断言），放进 `doc/spec-v1.2/README.md` 顶部 banner 或 `doc/architecture-v2.md`，并让每本受影响分册正文至少有一行指回该表；对不再适用的 AC 断言逐条改为新断言。见条件 C3。 | open |
| 5 | 阻塞 | `doc/architecture-v2.md:80,93,98,113,119`；`doc/spec-v1.2/07-验收用例.md:121-126` | **NDJSON 协议 v1（自称"冻结草案"）存在可导致返工的空洞**：①`run` 载荷无 `seed`（arch:98），但 §3.4 要求"random/numpy 种子由 run 消息的 seed 注入"（arch:119）；②"worker SDK 强制 `PYTHONHASHSEED=0`"不成立——该变量必须在 supervisor spawn 解释器前设置，SDK 运行期设置无效；③`exit 0 + 无 result`、`result` 后进程不退出、重复 `result`、`hello` 等待超时/无 supervisor ack 均未定义；④stderr"末尾 8 KiB"要求 supervisor 持续排空 stderr，否则 worker 写满 pipe 会阻塞至超时，架构未写排空策略；⑤协议占用 stdout/stdin，但用户 Python 代码允许 `print()`/`input()`，会污染/劫持协议，未写隔离策略；⑥用户函数是同步的，worker 如何在执行中接收 `cancel` 并置位 `ctx.cancel_requested`（需独立 stdin reader 线程）未定义；⑦半行帧/无换行 EOF 的超时处理未定义；⑧第 93 行"消息（worker 启动方向 = supervisor → worker）"语义自相矛盾（表中同时含双向消息）。 | 在 §3 增加"协议 v1 检查单"并写死：seed 进入 run 载荷（或 params）、PYTHONHASHSEED 由 supervisor spawn 设置、握手超时与 ack、退出码封闭表（含 exit 0 无 result）、stderr 排空线程与截断、用户 stdout/stdin 隔离（重定向/禁用并文档化）、cancel reader 线程模型、半行/EOF/帧超时；把该检查单列入 M0 DoD（arch:148）。见条件 C4。 | open |
| 6 | 重要 | `doc/spec-v1.2/README.md:1-14,46`；`doc/spec-v1.2/08-审计关闭矩阵.md:4,193,304,311,370-412`；`doc/spec-v1.2/02-DSL与校验.md:5`；`doc/spec-v1.2/05-AgentHarness.md:5,42,44,318,585,1062`；`doc/spec-v1.2/07-验收用例.md:3,130`；`doc/analysis/spec-audit-v1.1.md:7-8` | **spec 溯源不可核验 + 上游引用断链**：①入库没有源版本/commit/hash；②08 §8.6 记录 lead 在"冻结"后对 00–07 做了 9 高 + 4 缺口 + 27 中低回填，但 spec README 仍只声明 DR-001 顶部 banner，读者无法区分"v1.2 原文"与"回填版"；③`02-DSL与校验.md:5` 的 markdown 链接 `../决策记录-01-范围与Harness.md` 在仓库内不存在；④README/05/06/07/08 以反引号引用 `决策记录-01`、`_spike/REPORT.md`、`_research/*`、`_audit/spec-audit.md` 共 20+ 处，这些材料均不在仓库（`.gitignore` 明确忽略），07 AC-07 依赖的 spike 源文件也不存在；⑤审计报告使用 `/Users/zyzs/Downloads/...`、`/tmp/designdoc/doc.txt` 作为可复现基准，仓库内无法复核。 | ①在 spec README 增加"版本与来源"：源稿标识 + 入库 hash + 明确声明"v1.2 + 08 §8.6 回填"；②把 `决策记录-01`、spike 报告（至少结论摘要）、研究材料结论入库到 `doc/analysis/`，或全部改为"外部材料，标注不可复现"并登记 ISSUE；③修好 02:5 链接；④审计报告改为"外部原稿 + 本文件为唯一入库版本"并给 hash/日期。见条件 C5。 | open |
| 7 | 重要 | `doc/architecture-v2.md:62-78`；根 `AGENTS.md` §2 | 新模块表与 AGENTS §2 现有模块 id 无映射/退役说明：scheduler/state-manager/dispatcher/monitor/orchestrator/workers/config 等如何并入 `legacy/**`、是否还允许按旧模块 id 建 worktree，均未写；`tests/**` 同时承载 legacy 测试与新 e2e，`src/**/tests/**` 的迁移归属也未写。仅" M0 同步更新 AGENTS.md"不足以让并行分工在 M0 前落地。 | 在 architecture §2 增加"AGENTS 模块表迁移"小节：旧 id → 新 id/legacy 的映射、旧 id 的冻结时间、legacy 测试的迁移路径与 CI 排除方式；M0 DoD 明确"AGENTS 模块表 + scripts/dev 同步提交"。见条件 C6。 | open |
| 8 | 重要 | `doc/architecture-v2.md:146-153` | 里程碑 DoD 不够可执行：①AC-11（死锁）、AC-12（多 run 隔离）未挂到 M1–M4 任一 DoD，只在 M5 的"全部 AC"里出现；②M0 未包含 `scripts/dev/check.sh`/`merge.sh` 与 `doc/test-baseline.txt` 对 Rust + cargo 的适配，导致 M0 起合并门禁仍只跑 pytest；③M5"baseline 归零或只保留已登记项"在 legacy 冻结、pytest 不再覆盖新代码后语义不清。 | 把 AC-11 归入 M2（死锁检测属内核）、AC-12 归入 M5 或 M3（多 run 属内核+worker），并在 M0 DoD 增加"合并/自检脚本支持 Rust 与 Python 双栈（或声明 M0 暂沿用 pytest 并登记 ISSUE）"；M5 基线改为"cargo test 全绿 + pytest 基线不扩大"的双基线定义。见条件 C6。 | open |
| 9 | 重要 | `doc/spec-v1.2/05-AgentHarness.md:12`；`doc/decisions/DR-001-runtime-route.md:66,72-74`；`doc/spec-v1.2/07-验收用例.md:15,29-31,58-66`；`doc/issues.md:21` | **门控替换已登记但验收基线未同步**：ISSUE-014/O-1 与 README banner 已说明"门控 Python 函数 → v1 Rust 内置 op"，属于诚实登记；但 AC-02/AC-04 的 fixture 仍是 Python gate（`gates/hr.py`、`gates.hr.need_human`），DR §6 的验收映射没有门控一项，"内置 op"与 spec 04 的 `GateContext`/Python gate 签名校验 + R 规则如何共存也没有设计。 | 在 DR §6 的机制替换表中加入门控项；给出 v1 内置 op 的最小集合与 DSL 字段映射（或明确 M1 前以 gate worker 实现），同步 AC-02/AC-04 的 fixture 说明。见条件 C3/C6。 | open |
| 10 | 重要 | `doc/architecture-v2.md:138-139,163`；`doc/spec-v1.2/README.md:72-75` | **L3 诚实性与可操作性不足**：L2 声称"外部可见结果与原始 run 一致"，这实质是 L3 范畴；L3 对策写"worker 必须幂等或由用户 `--dry-run` 验证"，但 `--dry-run` 不在 spec 06 CLI 契约中，arch 也只新增 `replay`；"worker 必须幂等"是新增的用户义务，未写入 04/05 契约、验收用例或 ISSUE。 | L2 改为"记录值可重放，不保证外部副作用"；删除或实现 `--dry-run`（并在 CLI/DSL 契约登记）；把"worker 幂等/副作用声明"写入 04 节点契约和 07 验收（或登记为开放项）。见条件 C1。 | open |
| 11 | 建议 | `doc/decisions/DR-001-runtime-route.md:46-47`；`doc/test-baseline.txt`；`doc/issues.md:8-9` | 复用矩阵总体合理（3 张图转 fixture、门控语义、ProcessDispatcher 经验、monitor→trace、CLI、187 测试作行为参考、17 worker 挑 3–5、zlog 不迁移、tick/文件状态不迁移），但 `rollback` 子命令在 CLI 行被静默丢弃（未说明 v1 不需要还是遗漏）；legacy 25 项失败基线/ISSUE-001/002 与新 e2e 的关系（何时退出 pytest、基线如何归档）未写。 | 在复用矩阵 CLI 行补 `rollback` 的处置理由；在 M0/M5 说明 legacy 测试与基线归档方式。 | open |
| 12 | 建议 | `doc/decisions/DR-001-runtime-route.md:16`；`doc/spec-v1.2/08-审计关闭矩阵.md:4` | 量化宣称不准确：DR 称 v1.2"5,467 行"，实测 00–08 共 5,400 行；08 称"约 5100 行"；同类数字应可复算或删除。 | 统一为实测值（或去掉行数宣称）。 | open |
| 13 | 建议 | `doc/spec-v1.2/08-审计关闭矩阵.md:156-157,400`；`doc/issues.md` | 08 §8.6 自留 I-37（示意命名）与 M-02/M-05（trait + 空实现）；按 AGENTS §7，跨里程碑未关闭项应进 `doc/issues.md`，目前只存在于 spec 内。 | 在 issues 台账登记 I-37 与 M-02/M-05 的 v1.1 范围，状态 open。 | open |
| 14 | 建议 | 根 `README.md`；`doc/repo-status.md` | 仓库入口仍写"语言 Python 3.10+/pytest"与 Python 快速开始，技术路线已改 Rust；M0 前新读者会误判现状。 | 根 README 顶部加一行技术路线指向 `doc/decisions/DR-001-runtime-route.md`，repo-status 标注历史快照。 | open |
| 15 | 建议 | `doc/reviews/README.md`；提交 `3748379` | 评审索引未加 REV-001（作者合并前补）；单提交同时含 DR/架构/5.4k 行 spec 入库/issues/progress，与 AGENTS §1"一个提交只做一件事"有张力（不影响本分支结论，后续拆分）。 | 合并前更新 reviews 索引；后续大件文档按主题拆提交。 | open |
| 16 | 建议 | `doc/analysis/spec-audit-v1.1.md:7-8` | 审计报告以 `/Users/zyzs/Downloads/...`、`/tmp/designdoc/doc.txt` 作为核对基准，非仓库内可复现路径。 | 改为"外部原稿 + 本文件为唯一入库版本"并给 hash/日期。 | open |

> 说明：F1–F5 为阻塞项，对应条件 C1–C4；F6–F10 为重要项，其中 F6/F7/F8/F9 对应 C5/C6，F10 并入 C1。F11–F16 为建议项，可后置，但需进台账或评审关闭记录。

## 4. 验证证据

```bash
cd .worktrees/docs-agent-lead
git merge-base --is-ancestor origin/main HEAD; echo $?          # 0（基于 origin/main=50f05d2）
git diff --stat origin/main..HEAD                              # 16 files changed, 6755 insertions(+), 16 deletions(-)
git show -s --format='%H%n%B' HEAD                             # 3748379；正文含 Review: REV-001 / Agent: lead
bash scripts/dev/check.sh                                      # CHECK PASSED（身份/冻结文件/分支/进度 20 非空行/JSON）
/tmp/gf_venv/bin/python -m pytest -q --tb=no                   # 25 failed / 162 passed（187 项）
grep -v '^#' doc/test-baseline.txt | sed '/^$/d' | wc -l       # 25（无新增失败，无已修复项）
python3 <doc 内 markdown 相对链接检查脚本>                      # 仅 1 处断链：doc/spec-v1.2/02-DSL与校验.md:5
grep -rn "决策记录-01\|_spike\|_research\|_audit/spec-audit" doc/  # 20+ 处引用未入库材料（逐条人工核对）
wc -l doc/spec-v1.2/*.md                                       # 00–08 共 5,400 行；README 75 行
```

- 测试结果：`25 failed / 162 passed`，与 `doc/test-baseline.txt` 的 25 项完全一致：**无新增失败、无新增修复**；本次为 docs-only 变更，测试不受影响。
- 环境说明：本会话默认 `python3` 无 pytest、无 uv（ISSUE-003），使用 `/tmp/gf_venv/bin/python`（Python 3.13.0 + pytest 9.2.0）执行；`merge.sh` 合并时需 `PYTHON_BIN=/tmp/gf_venv/bin/python` 或先完成 ISSUE-003。
- 手工验证：提交作者/身份正确；分支基于 `origin/main`；变更全部在 `doc/**`；`CLAUDE.md` 未改动；spec 章节正文无 DR banner（仅 `spec-v1.2/README.md` 顶部有 7 行落地说明）；DR-001 对用户四条决策的表述与用户口径逐条比对无实质偏差。
- 评审局限：仓库内没有 v1.2 的上游副本/源 hash，无法验证"未被篡改"，只能依据 08 §8.6 的回填记录与内部一致性推断；补齐 F6 的溯源后需复核一次。

## 5. 遗留问题

- 条件 C1–C6 尚未关闭；`merge.sh` 只接受"通过"结论，因此本分支当前**不可合并**。条件关闭后由作者或评审人追加修订段，并同步 `doc/reviews/README.md` 索引。
- 建议作者在修复时一并登记 `doc/issues.md`（编号自取）：可复现 schema/replay 落地、超时语义裁定、协议 v1 检查单、spec 溯源与外部材料、模块表/测试门禁迁移；这些属于 AGENTS §7 要求长期跟踪的事项。
- 已知并接受的已声明项：spec 08 的 I-37（示意命名，P6 统一）与 M-02/M-05（trait + 空实现，v1.1）属范围裁剪；门控（O-1/ISSUE-014）、复现粒度（O-3/ISSUE-016）、worker 池化（O-2/ISSUE-017）已登记，不算隐藏冲突。
- F11–F16 为建议项，不阻塞合并；若作者不改，需在本记录追加"接受为遗留"的说明。

## 6. 条件关闭记录

> 关闭全部条件后，在本表补证据并追加修订段，将 §1 结论改为通过；在此之前维持"有条件通过"。

| 条件 | 关闭条件（可验收） | 关闭证据 | 关闭日期 |
|---|---|---|---|
| C1 | §5 可复现落地：给出 L2 的存储增量（表/列）、`--record`/`replay` CLI 契约与 spec 06 §6.10 的对齐、L2 命名与默认粒度三处统一；`--dry-run` 删除或登记；F10 一并处理；新增 ISSUE | 待补 | — |
| C2 | 超时/放弃语义二选一并写死：更新 00/03/04/06/07（含 AC-06 断言）与 DR §6/README banner 的机制替换清单；新增 ISSUE | 待补 | — |
| C3 | 增加"v1.2 假设 → worker 模型"对照表（Python 执行模型、并发、ctx API、工具执行、门控、超时、验收断言），并让 03/04/05/07 正文至少指回；删除/限定 05:12"永远在本进程内" | 待补 | — |
| C4 | §3 协议 v1 检查单写死 seed/PYTHONHASHSEED/exit 0 无 result/hello 超时/stderr 排空/stdout-stdin 隔离/cancel reader/半行帧，并纳入 M0 DoD | 待补 | — |
| C5 | spec README 增加版本来源/hash/回填声明；修 02:5 断链；未入库上游材料（决策记录-01、spike、research、audit）入库或标注"外部不可复现"并登记 ISSUE | 待补 | — |
| C6 | 新增旧模块 id → 新 id/legacy 映射表并写清 `tests/**` 归属；AC-11/AC-12 挂里程碑；M0 DoD 增加 AGENTS 模块表 + check/merge/test-baseline 的 Rust 适配项 | 待补 | — |
