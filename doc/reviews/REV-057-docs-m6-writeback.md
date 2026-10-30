# REV-057 · docs · M6 spec 回写（05/06/07/08 + 02/03/04 + architecture + PROVENANCE + 台账）

- **分支**：`docs/docs-m6-writeback`
- **作者**：`docs-writer`（7 commits：`941bd7e`→`4036b75`→`b6c73f6`→`839f7ef`→`47c7c62`→`cadb898`→`50cc879`）
- **评审人**：`reviewer-a`（**≠ 作者**；主评审）＋ `lead`（第二核对人）
- **变更范围**：`main(07bb500)..50cc879`，11 文件 +184/−84，**纯 `doc/**`**：`spec-v1.2/` 02/03/04/05/06/07/08、`README.md`、`PROVENANCE.md`、`doc/architecture-v2.md`、`doc/issues.md`；无代码/配置/测试改动；worktree clean
- **评审时间**：2026-10-31
- **关联**：REV-057；DR-003 W2；ISSUE-080/082/088/092/095/096；M6 实现/评审 REV-040/041/042/043/044/047/048/049/050/052/055/056/058/060；06 §6.2.4/§6.4.1/§6.4.5/§6.7/§6.8.1/§6.8.3/§6.9/§6.10；05 §5.2.1/§5.6.2/§5.6.5/§5.8.5/§5.13；07 AC-06/AC-12；08 §8.8

## 1. 结论

**结论：通过**（增量复审 `ae3ce1c`：C1–C3 已关闭、S1/S2/S4 已落实，证据见 §6；首轮「有条件通过」原文保留）

正文 M6 技术口径已逐条与 `main` 已合并实现/评审记录核对一致（§2、§4）；首轮发现的 3 处台账/审计数字不一致已由 `ae3ce1c` 修正并复核（§3 状态列、§6）。

## 2. 四维评审

| 维度 | 结论 | 依据（文件/行 + 实测） |
|---|---|---|
| 功能正确性 | ✅（正文口径） | 逐条核对：① 06 §6.4.1（双通道实际落地、同发送端有序/跨发送端各通道 FIFO/无全局全序）与 REV-041 实现及 `writer/thread.rs` 一致；② 06 §6.4.5（fail-stop 已接线、kernel `take_error` 轮询、`RunFinished{error=None}`、`runs.error_code NULL`、CLI exit 4、`--json` 无结果 JSON）与 REV-047 一致；③ 06 §6.2.2/§6.2.4/§6.7/§6.10.7（schema v3、`global_vars_json`、逐版本 `VACUUM INTO` 备份/失败恢复/不 backfill、replay NULL→空 map+告警/非 object→`REPLAY_RECORD_CORRUPT`/v2 库 exit 4）与 REV-049 代码+测试一致；④ 06 §6.10.1/§6.10.2/§6.10.5（不引入 clap、`-v` 任意位置→RUST_LOG、`--version` 字面、0/2/1/70、纯文本无 ANSI）与 REV-043 实现一致；⑤ 06 §6.10.3（bridge 只读旁路实时行、`[seq N]` CLI 侧序号、`--json`/`-q` 不变）与 REV-050 一致；⑥ 06 §6.2.4/§6.2.13/§6.9.1（两列写时点/形状/`[]`/`base_url` 脱敏/仅节点 worker/尽力写/`:memory:` 不写）与 REV-048 最终代码（含 C1 方案 B、C5）一致；⑦ 06 §6.8.1/§6.8.3（OS hostname、env 别名、EPERM=存活、跨机 heartbeat 60s+告警、`warnings[]`、trace 提示校验 owner_host、bridge 兜底）与 REV-056 `recovery.rs/trace_cmd.rs/bridge.rs` 一致；⑧ 05 §5.2.1 symlink canonicalize、§5.8.5 结构调整+敏感键（10 个归一化 token 与 `SENSITIVE_KEY_TOKENS` 一致）、§5.6.5/§5.13.1 D13 共享 `WorkerLimits`、ISSUE-080 9 个 discover reason 闭集（py-worker 4 + harness 映射 5，逐项一致）；⑨ 07 AC-06（内核测试名 `executor_reported_timeout_snapshot_carries_timeout_error` 实测存在于 `crates/zgraph-kernel/tests/timeout.rs:141`）、AC-12（28/30 区间、峰值、2.80/3.34、4.04/4.15 与 REV-060/REV-044 记录一致）；⑩ 架构 §5.2 与 REV-048/049 一致。**台账/审计数字 3 处不一致见 §3** |
| 代码结构合理性 | ✅ | docs 单模块变更，结构按「分册顶部 M6 banner 索引 → 正文口径回写 → 历史注记保留」组织；08 **只追加** §8.8（`git diff --numstat` = +34/−0，8.0–8.7 未改）；PROVENANCE 增加 M6 批次条目与实测表；02/03/04 仅撤销临时口径以消除跨册矛盾；未改代码/协议/测试 |
| 逻辑性 | ✅ | 正文与实现、各册之间交叉引用自洽：03 §3.12/04 A7/A17 的双通道注记与 06 §6.4.1 一致；05/06/07/08 对 082/087/088 的写入口径/迁移/兼容描述一致；「M5 历史注记不删除、M6 注记追加」逻辑清晰。**台账/审计一致性见 §3 C1–C3** |
| 可读性 | ✅ | M6 banner 汇总回写范围+REV 号，便于按册检索；历史注记与 M6 口径明确区分；`--json.warnings[]`、`:memory:`、备份保留等边界均显式说明；建议项见 §3（e2e 历史计数限定、README 中低条数措辞） |

## 3. 发现清单

| # | 级别 | 位置 | 问题 | 建议 | 状态 |
|---|---|---|---|---|---|
| C1 | **重要** | `doc/issues.md:44`（ISSUE-036） | 台账仍为 `open`，描述还是「M5 在 run 生命周期与 CLI 接线」；但同分支 06 §6.4.5（`06-存储与可观测.md:562`）写「**M6 已接线（REV-047，ISSUE-036 fixed）**」、08 §8.8（`08-审计关闭矩阵.md:455`）写 fixed、PROVENANCE §2（`PROVENANCE.md:97`）称「按 REV 关闭 …ISSUE-036…」。REV-047 已合并 `main` `b39374a`，三者与台账矛盾 | 把 ISSUE-036 置 `fixed` 并注明「REV-047（跨模块主审+第二评审），`main` `b39374a`；kernel 轮询 `take_error` + CLI exit 4 + 故障注入测试」，与 06/08 一致；如确需保留 open，则 06/08/PROVENANCE 的 fixed 声明必须同步撤销（不推荐） | **已关闭（`ae3ce1c`，见 §6）** |
| C2 | **重要** | `doc/issues.md:104`、`08-审计关闭矩阵.md:470,476`、`PROVENANCE.md:97,119,140` | ISSUE-096 被标 `fixed（REV-061）`，但 **REV-061 尚未产出/合并**（`origin/main` 无 `REV-061` 记录；`scripts/dev/merge.sh:78-79` 当前仍只有 `cargo test`，无「测试前 build CLI bin」）。本分支先合并时，台账/08/PROVENANCE 会提前宣称 096 已修；08:476「本节已无 pending 占位」也与 096 待 wrapup 矛盾 | 按 REV-061 合并顺序改口径（任选）：① 096 改 `pending`/`in-progress`「REV-061 修复，随 wrapup 合并后成立」；② 保留 `fixed` 但显式标「待 REV-061 合并后成立」；③ 若坚持当前写法，需把 08:476/PROVENANCE:140 的「已无 pending」修正并说明 096 以 REV-061 为前置。同时 PROVENANCE:119 的「REV-040–061」建议改为「REV-040–060 + REV-061（wrapup）」 | **已关闭（`ae3ce1c`，见 §6）** |
| C3 | **重要** | `doc/spec-v1.2/PROVENANCE.md:103`（对照 `:101`、`:119`、§3 表） | §2 本批次收尾行写「00–08 合计 **6033 行**」，但同文件 §3 表写 **6038 行**；实测（`wc -l` 00–08）为 **6038**，§3 的 9 个 SHA-256 也全部与实测一致（9/9）。6033 是第三提交（ISSUE-082 回写）时点的中间值（:101），不应作为本批次最终值 | 把 :103 改为 **6038 行**；:101 保留但建议注明「（第三提交时点；后续 ISSUE-088 回写后最终 6038）」以免与 §3 混淆；:119 已是 6038，无需改 | **已关闭（`ae3ce1c`，见 §6）** |
| S1 | 建议（低） | `06-存储与可观测.md:993` | 「e2e 包 **37 passed / 0 failed / 1 ignored**」是 REV-032 批次时点数据；M6 已新增 e2e 用例（REV-044/055/060），当前包计数已变化，易被误读为当前实测 | 加限定「（REV-032 批次时点）」或更新为当前值（若合并前复跑） | 已落实/确认（§6） |
| S2 | 建议（低） | `README.md:4` vs `:61`、`08 §8.6.3` | README 顶部「9 高 + 4 缺口 + **27** 中低回填」与底部/08「**28** 中低处置」并存；语义上可解释为「28 条中 27 回填/1 保留」，但表述不统一 | 统一为「28 中低（27 回填 / 1 保留，见 08 §8.6.3）」 | 已落实/确认（§6） |
| S3 | 备注 | `00-总纲.md`（未改） | 派单提到 00 小改，实际本分支未改 00。已核对 00 无 `未接线/单通道/仍未写入/global_vars/worker_versions` 等旧口径，也无 M6 新错误码/常量需要回写——确认**无需改**即可 | 保持不改；在 REV-061 索引/进度注明「00 本批无需回写」 | 已落实/确认（§6） |
| S4 | 备注 | `PROVENANCE.md:101` | 第三提交 6033 行是历史中间值，本批最终 6038；若按 C3 修正 :103，建议同时把 :101 加「第三提交时点」限定，避免审计读者误判 | 随 C3 一并处理（低成本） | 已落实/确认（§6） |

## 4. 验证命令与结果

```bash
# 分支范围与纯文档
git log --oneline origin/main..HEAD            # 9 commits（首轮 7 + 评审记录 + 关闭提交）；base 07bb500，增量复审时 main 21e24f3（落后 9）
git diff 07bb500..HEAD --name-status           # 11 files，全为 doc/**；08 numstat = +34/−0（只追加 §8.8）
bash scripts/dev/check.sh                      # CHECK PASSED（身份/冻结文件/禁用文件/语法/fmt）

# PROVENANCE 实测（脚本解析 §3 表 + sha256/wc 逐项比对）
#   结果：9/9 文件 SHA-256 与行数全部一致；00–08 合计 6038 行（与 §3 表一致；与 §2 :103 的 6033 不符 → C3）
shasum -a 256 doc/spec-v1.2/0*.md；wc -l doc/spec-v1.2/0*.md

# issues 台账结构/状态
#   96 行、ISSUE-001..096 无重复/无断号、8 列齐全、状态域合法；
#   关闭后：036 fixed（REV-047/main b39374a）、096 open/pending（待 REV-061 wrapup）；其余 079/080/082/085/087/088/092 fixed、095 open 与 08 §8.8 一致

# 正文口径 vs 已合并实现（main 3d227fc）交叉抽查
python/zgraph_worker/tool_runner.py:33-36              # tool 侧 4 个 reason
crates/zgraph-harness/src/tools.rs:1474-1479           # harness 侧 5 个 reason 映射（合计 9，与 05 §5.2.7/§5.6.2.1 一致）
crates/zgraph-cli/src/recovery.rs:57-70,173-185        # OS hostname/EPERM；trace_cmd.rs:224-242；bridge.rs:487-499
crates/zgraph-kernel/tests/timeout.rs:141              # 07 AC-06 引用的测试真实存在
crates/zgraph-cli/src/lib.rs:127-145                   # -v 任意位置、-v→debug/-vv→trace、显式 RUST_LOG 优先
scripts/dev/merge.sh:78-79                             # 当前仍只有 cargo test；096 保持 open，待 REV-061
# REV-060 记录（28/30 区间、峰值、2.80/3.34）与 REV-044（4.04/4.15）与 07 AC-12 引用一致
```

- 本分支为 docs-only：未跑 crate 测试（无代码改动）；`check.sh` PASS + 上述跨源核对覆盖回写正确性；merge.sh 会跑全量测试兜底。
- 兼容/范围：变更全部在 `doc/**`，未触碰代码/`CLAUDE.md`；基于 07bb500，增量复审时 `main` 已到 21e24f3（REV-054 已并入），merge.sh rebase 预期无冲突（本分支未改这些提交涉及的源码；docs 文件与 main 线无交集）。

## 5. 遗留问题

- **C1–C3 已在 `ae3ce1c` 关闭**（ISSUE-036 → fixed；ISSUE-096 → open/pending；PROVENANCE 最终行 → 6038），S1/S2/S4 同步落实，S3（00 无需回写）已核实——证据见 §6，**无遗留条件**。
- ISSUE-096 的最终 fixed 以 REV-061（wrapup）合并为前提：本分支合并后 `doc/issues.md` 中 096 保持 open/pending 是**预期状态**，由 wrapup 合并时收口；本分支不提前宣称。
- 无代码风险：本批无代码/协议/测试变更。

## 6. 条件关闭记录（增量复审 `ae3ce1c`）

**关闭提交范围**：`13539e1..ae3ce1c`，5 files +18/−18（`doc/issues.md`、`06-存储与可观测.md`、`08-审计关闭矩阵.md`、`PROVENANCE.md`、`README.md`），全部 `doc/**`；`00-总纲.md` 未改；worktree clean、`check.sh` PASS。

| 条件/建议 | 关闭证据（`ae3ce1c`） | 结论 |
|---|---|---|
| **C1** | `doc/issues.md:44` ISSUE-036 → `fixed`，注明 REV-047（跨模块主审+第二评审，`main` `b39374a`）、kernel `take_error` 轮询/内存态 Failed/CLI exit4/故障注入测试；06 §6.4.5、08 §8.8（`:455`）、PROVENANCE（`:99`）与台账三者一致；复核无「ISSUE-036 … open」残留 | ✅ 关闭 |
| **C2** | `doc/issues.md:104` ISSUE-096 → `open`（「待修（REV-061 wrapup，合并后置 fixed）」，明确 REV-061 未合并前不生效）；08 §8.8 行改 `pending（REV-061 wrapup）`（`:470`）、来源与口径说明统一（`:450,476`，删除「已无 pending」）；PROVENANCE（`:91,99,119,140`）统一「REV-040–060 + REV-061（wrapup）」/「096 仍 open，合并后置 fixed」；复核无「fixed（REV-061）」残留 | ✅ 关闭（096 保持 open/pending 为预期，待 wrapup） |
| **C3** | PROVENANCE §2 本批次最终行 `:103` → **6038 行**；`:101` 第三提交 6033 行加注「（第三提交时点；后续 ISSUE-088 回写后最终 6038 行）」；`:119` 仍 6038；复核 §3 表 9/9 文件 SHA-256+行数实测一致、00–08=6038（脚本复核） | ✅ 关闭 |
| S1 | 06 §6.10.7 `:993` e2e「37 passed」加「（REV-032 批次时点数据；M6 新增用例后的当前计数以 e2e 实测为准）」 | ✅ 落实 |
| S2/S4 | README `:4/:61` 统一「28 中低（27 回填 / 1 保留）」；S4 随 C3 一并处理 | ✅ 落实 |
| S3 | `00-总纲.md` 未改；已核对无旧口径/新错误码/新常量需回写，commit body 已说明 | ✅ 确认（无需改） |

**复核结果**：`doc/issues.md` 96 行、ISSUE-001..096 无重复/断号、8 列、状态域合法；036 `fixed`、096 `open`；08 §8.8/PROVENANCE 与台账一致，无提前 fixed/「已无 pending」残留；PROVENANCE §3 的 9 个 SHA-256 与行数全部与实测一致（00–08=6038）；`git diff 13539e1..ae3ce1c` 仅 5 个 `doc/**` 文件 +18/−18。

> 主评审结论：**通过**（首轮「有条件通过」→ C1–C3 关闭、S1/S2/S4 落实 → 本次改「通过」）。分支基于 07bb500、落后 `main` 21e24f3（REV-054 已并入），rebase 预期无冲突；合并后用 REV-061 wrapup 收口 ISSUE-096。
