# REV-105 · docs · v0.4（M9）D1 设计冻结（DR-012/013/014 + spec 00/02/05/06/07/08 草案）

> **评审范围**：纯设计/文档分支（`doc/**`，含 spec 草案），不涉及代码/测试/既有评审记录。评审人只新增本评审记录（未改实现/设计文档）。
> **结论性质**：首轮发现 4 个 **P1** 文档合同矛盾（均可在 D2 前用 docs 小 commit 关闭）；P0=0。

- **分支**：`docs/docs-v04-design`
- 分支: docs/docs-v04-design
- **作者**：`docs-dev`（提交 `462f53d` 初稿 + `0aa7165` 8 项裁决 + `5f15de9` spike 合入；正文 `Review: REV-105` / `Agent: docs-dev`）
- **评审人**：`reviewer-a`（独立评审；Lead 追加裁决确认）
- **基于**：main `933a80d`（`main = origin/main = 933a80d`）
- **变更范围**：`933a80d..5f15de9`（**12 files, +820/−8**）：新增 `doc/decisions/DR-012..014`、`doc/spec-v1.2/00/02/05/06/07/08` 草案段、`doc/index.md`、`doc/issues.md`（ISSUE-165–174）、`doc/progress.md`
- **评审时间**：2026-11-02
- **关联**：task-74（用户决策/合同）、task-76（评审要点）、spike `/tmp/v04-sandbox-research.md`（附录 A）；REV-096（v0.3 设计冻结范式）

## 1. 结论

**结论：通过**

> **结论历史**：首轮（基准 `5f15de9`）为「**有条件通过**」——P1 C1–C4（`--allow-path` 读写权限矛盾、bash 执行形式/上限、read/v7 限额常量、hardlink/firmlink 安全面）+ P2 ×3。作者 `923a89a`（C1–C4/P2/P3 主体）与 `2132478`（C2 两处 16 KiB、index 拆行、AC-21 f 负例）关闭后，评审人跑 `check.sh` + v0.4 段旧值 grep 清零（§8），C1–C4/P2 全部 closed（§6/§8），结论改**通过**；P3 残余仅为后续 docs 清理项。
>
> 首轮意见（历史保留）：P0=0；P1 条件 C1–C4（D1 内 docs 修订可关闭，见 §5/§6）；P2 3 项、P3 若干建议；冻结文档间 4 组矛盾必须先统一再开 D2。

### 1.1 已核对成立（设计主体）

| 项 | 结论 |
|---|---|
| 范围/波次（DR-012） | ✅ v0.4 全做 8 工具；W5→v0.5；D1–D6 串行（D2–D4 同 harness 单写者、D5 跨模块 Lead 批准+第二评审）；发布条件/兼容矩阵/AC-21–24 矩阵/风险齐备；默认路径/DSL 1.0/19 码不变的红线明确 |
| opt-in/工具源/白名单 | ✅ `--allow-builtin-tools`（7 文件/搜索）+ `--allow-bash` 双层；`AgentConfig.tools` 唯一白名单；三类源同名 fail-closed、`builtin__` 保留前缀、无覆盖语义（DR-013 §1/§1.1） |
| jail 路径主体 | ✅ `--workspace-dir` 缺省 `.` + canonicalize；`--allow-path` 目录级、可重复、canonicalize；词法+真实路径组件前缀+symlink 出根拒绝；TOCTOU 残余风险已列（DR-013 §2）；`path_escape`/`not_regular_file` 等工具错误面 |
| 8 工具语义（除 C1–C3 限额/形式） | ✅ read offset/limit/binary/UTF-8；write 原子/父目录不自动创建/overwrite/create_parents；edit 唯一匹配失败不改/replace_all；apply_patch 两阶段严格原子/unsupported；diff；grep/glob `.gitignore`/隐藏/二进制/排序/上限；bash cwd/timeout/进程组/白名单 env/非 0 退出为工具结果 |
| OS 沙箱/spike 合入 | ✅ DR-013 §5/§11 已合入 spike：profile 按附录 A（`/private/...` realpath、系统只读集合含 `/private/var/select/sh`/CLT/metadata/`/dev/*`/mach、`process-exec` 收窄、workspace 读写、allowlist 只读、网络 deny/`--allow-network`、`TMPDIR=<workspace>/.tmp`）；嵌套 exit 71 → `sandbox_nested`、exit 65 → `sandbox_profile_invalid`、fail-closed 无 fallback/无 `--unsafe-bash`；CLI 预检 exit 3 不产生 run；`--allow-exec-path` 构建工具限制；profile ≈11 ms/每条 bash +22–40 ms（中位 25 ms）；macOS 26.6 实测、Linux/Windows 不支持 |
| trace v7/回放（除 C3 限额） | ✅ 只新增 `tool_calls`+索引、键 `(run_id,instance_id,attempt,call_id)`、原生 call_id、不伪造 `#n`；v6→v7 备份/单事务/链式/只读拒绝/不可降级；默认 recorded-only（绝不执行工具/副作用）、reexec 仅接口 v0.5 落地（集合 read/grep/glob/diff）、截断回放 fail-closed；L2 `recorded_outputs` 不变；trace 人类/`--json` additive；无工具调用与 v6 一致 |
| 零新码/既有闭集 | ✅ 沙箱失败面零新码：配置/启动前 `HARNESS_CONFIG_LOAD{reason:"sandbox_unavailable"/"sandbox_nested"/"sandbox_profile_invalid"}`、运行期 `TOOL_ERROR{reason:"sandbox_failure"}`；无新 kind/顶层码；jail/工具失败均 `TOOL_ERROR{reason}`；既有码不变（新增 CLI `REPLAY_TOOL_*` 为 append-only，P2 见 §3） |
| AC-21–24 可测试性 | ✅ 文件/搜索/bash/trace 四组断言可落到真 CLI+SQLite+workspace 快照/pid/沙箱负例；AC-24 g「不执行任何工具」有 spawn=0/快照/tool_calls 行数三重证据；AC-23 d exit71/65/fail-closed 明确 |
| 范围/红线/门禁 | ✅ docs-only；无代码/测试/既有评审记录改动（`git diff --name-only` 仅 12 个 doc 文件）；无 token/密钥；spec 仅追加/草案段；`check.sh` **PASSED**；索引已加 DR-012–014、issues 新增 165–174、progress 9 行四块 |

## 2. 四维评审

| 维度 | 结论 | 依据 |
|---|---|---|
| 功能正确性 | ⚠️ 有条件 | 设计主体覆盖 AC-21–24 与 spike；但 **C1 allow-path 读写 / C2 bash `-lc`+32K+30s / C3 限额/常量 / C4 hardlink/firmlink** 四组合同矛盾会使 D2–D5 实现按不同文档落到不同行为，必须 D1 内统一 |
| 代码结构合理性 | ✅ | docs-only、DR 分层清晰（012 范围/013 工具+jail/014 trace）、spec 增补按分册、索引/进度/台账同步；未侵入代码/测试/评审 |
| 逻辑性 | ⚠️ 有条件 | 默认路径/兼容红线/失败面/迁移/回放语义自洽；但同一文档内部（DR-013 §3.8 vs §5.2、§2.1 vs §4）与跨文档（DR-012 §8 vs DR-014、spec 00 vs DR-013）限额/权限不自洽（C1–C3） |
| 可读性 | ✅（P3 备注） | DR/spec 条款编号、AC 表、错误面/信任模型清晰；`doc/index.md` 有一处表行粘连（P3-2）、`doc/issues.md` 有空白行/缺尾换行（P3-3） |

## 3. 发现清单

### 3.1 P1 条件（D1 内关闭）

| # | 位置 | 问题 | 建议 |
|---|---|---|---|
| **C1** | `DR-013:48`（§2.1「读写全开」）、`:61`（§2.2「读写全开」）、`:306`（§10 裁决 4「读写全开」）；`05-AgentHarness.md:1507`（§5.15.2「读写全开」） vs `DR-013:192`（§4 表「只读」）、`:213`（§5.1「仅 file-read*」）、`05:1511`（§5.15.3「`--allow-path` 只读」） | **`--allow-path` 权限语义自相矛盾**：一半文档写「目录级读写全开」，另一半与 spike/Lead 裁决写「目录级只读（仅 file-read*）」。这是安全合同的核心（读写全开会把显式白名单变成写逃逸面） | 统一为 **只读**（Lead 裁决 + spike 附录 A 的 allowlist 只读段）：删除/改写 §2.1/§2.2/§10/05 §5.15.2 的「读写全开/目录即全开」；AC-21 f 增加「`--allow-path` 内 write/edit 仍拒绝（`path_escape`/只读）」负例 |
| **C2** | `DR-013:172`（command `maxLength:131072`）、`:174`（timeout default `30000`）、`:180`（stdout/stderr cap 32 KiB）、`:183`（「固定 `/bin/bash -lc`」） vs `DR-013:220`（§5.2「固定 `/bin/bash --noprofile --norc -c`；禁 `-lc`」）、`:225`（16 KiB head+tail；超时默认 120s）；`05:1512-1515`（no-login + 16 KiB head+tail 的同一段末行又写「各 32 KiB」）；`00:334`（`BASH_DEFAULT_TIMEOUT_MS=120000`） | **bash 执行形式与限额四处矛盾**：①`-lc`（登录 shell，会读 workspace `.profile`，安全面）vs `--noprofile --norc -c`；②32 KiB vs 16 KiB head+tail；③schema 默认 30s vs 正文/00 常量 120s；④command 允许 128 KiB vs DR-014 trace command 8 KiB 超限拒绝 | 以 §5.2/§11/spike 为准统一：固定 `/bin/bash --noprofile --norc -c`；stdout/stderr **16 KiB head+tail**（drain 2 MiB）；timeout 默认 **120s**（schema default 改 120000，或明确「schema 默认 30s、执行器默认 120s」并只保留一个）；command 上限统一（建议 **8 KiB** 与 `tool_calls.bash_command` 一致；若允许执行 >8 KiB，DR-014 的「超限拒绝」需改为执行允许/记录拒绝并说明） |
| **C3** | `DR-013:77`（read 单次输出「推荐 256 KiB/32K chars」）、`:270`（§8 read「1 MiB max_bytes/10000 行，默认 2000」）；`00:330`（`BUILTIN_READ_MAX_BYTES=262144`/`MAX_LINES=2000`）；`DR-012:120`（v7 args/result **64 KiB**、stdout/stderr **32 KiB**）vs `DR-014:14/103/104`、`DR-013:308`、`06:1340`（**32 KiB/16 KiB**）；`00:332`（search 1000/262144 未覆盖 glob 5000/1 MiB） | **read / v7 / search 限额与常量不一致**：①read 256 KiB/2000 vs 1 MiB/10000（默认 2000）；②DR-012 §8 仍写 64/32 vs 已裁决 32/16；③`00` 的 search 常量只有 grep 1000/256 KiB，glob 5000/1 MiB 无对应常量；command/path 的 v7 常量未在 00 列出 | 以「Lead 裁决 + DR-014/DR-013 §10」为准统一：read **1 MiB / 10000 行（默认 2000）**，`00` 常量改为对应值并区分默认/最大值；DR-012 §8 改 **32/16**；`00` 补 glob（5000/1 MiB）与 `TOOL_CALL_COMMAND_MAX_BYTES=8192`/`TOOL_CALL_PATH_MAX_BYTES=4096`；删/改 §3.1 的 256 KiB/32K 旧推荐 |
| **C4** | `DR-013` §2.1（仅 symlink/特殊文件；无 hardlink）、§2.2/§5.1 未提 `/System/Volumes/Data` firmlink；spike A3 有 `hardlink-escape`/`sysvol-data-alias` 断言（`/tmp/v04-sandbox-research.md:133/136/164-165`）、附录 D 明确 `/System/Volumes/Data` 之外 firmlink 未测（`:659`） | **hardlink / firmlink 安全面未进 jail 冻结**：文件工具按路径 canonicalize 无法识别 workspace 内 hardlink 指向外部 inode（写可改外部文件）；`/System/Volumes/Data` firmlink 别名与 realpath 比较未定义（spike 仅 bash 层验证拒绝，Rust 文件工具未覆盖）。Lead 评审点明确要求核对 symlink/hardlink/firmlink/TOCTOU | 在 DR-013 §2 增加 **hardlink 策略**（nlink/inode 检查/拒绝，或明确「workspace 内容视为用户信任+残余风险」）与 `/System/Volumes/Data` firmlink 的 realpath 口径；AC-21 f/AC-23 补 hardlink/别名路径负例；若选择接受残余风险，登记 ISSUE 并在 AC 负例注明 |

### 3.2 P2（建议 D1 内或紧随 D2 修正）

| # | 位置 | 问题 | 建议 |
|---|---|---|---|
| P2-1 | `06-存储与可观测.md:1360` | §6.15.4 把 `reexec` 写成已可用行为，但 DR-014/AC-24 h 明确 **v0.4 只做 recorded**；`--replay-tools=reexec` 应「接口保留、未实现」 | 该行标注「v0.4 未实现（接口保留；v0.5 落地）」并给未实现时的 CLI 错误口径 |
| P2-2 | `08-审计关闭矩阵.md:640`（§8.13 行 5） | 新 CLI 参数列表遗漏 **`--allow-exec-path`**（Lead 评审点/DR-013 §4/§5.1 已引入） | 行 5 补 `--allow-exec-path`（可重复、opt-in、只读 allow-path ≠ 可执行） |
| P2-3 | `00-总纲.md` §0.4 CLI 命名空间 | 新增 `REPLAY_TOOL_RECORD_MISSING`/`REPLAY_TOOL_RECORD_TRUNCATED`/`REPLAY_TOOL_MISMATCH` 只在 06/08/DR-014 出现，00 §0.4 未登记（append-only CLI 命名空间） | 00 §0.4 追加三码（CLI、不写 `runs.error_code`、退出码 3/1）或明确允许在 08 §8.13 登记 |

### 3.3 P3（建议）

- **P3-1 状态行 stale**：`DR-012`/`DR-013` 状态仍写「spike 结论待 Lead 转发/待 arch-researcher spike 结论」，但 `5f15de9` 已合入 §5/§11；改为「spike 已合入（2026-11-02）」。
- **P3-2 `doc/index.md` 表格粘连**：DR-014 行与 `architecture-v2.md` 行缺换行（渲染成 `...AC-24 || [architecture...]`）；DR-011 后插入 DR-012 前多一空行也切断表格；修表。
- **P3-3 `doc/issues.md` 版式**：ISSUE-172 与 173 之间多空行、文末缺换行（`No newline at end of file`）；补换行/去空行（不改变内容）。
- **P3-4 spike 数字与冻结差异需写明**：spike D 推荐 stdout/stderr 64 KiB head/tail + blobs；冻结为 16 KiB/无 blobs（Lead 收窄）；spike B 输出 1 MiB/2 MiB drain；DR-013 §8 16 KiB；建议在 DR-014 §10/DR-013 §11 注明「spike 推荐 → Lead 收窄」以避免后续误读。
- **P3-5 DDL 命名/约束差异**：DR-014 用 `signal`、06 用 `bash_signal`；06 的 `bash_truncated` 无 `CHECK (0,1)`；统一 DDL（实现前）。
- **P3-6 read schema**：`limit` maximum=10000/default 2000，但没有 `max_bytes` schema 字段；`BUILTIN_READ_MAX_LINES` 命名像 max 但值 2000（默认）——与 C3 一并明确默认/上限。
- **P3-7 `--allow-path` 目录级只读的文档警示**：§2.2 已写「用户显式信任」，统一只读后补「不可写」与 `.gitignore`/`.tmp` 提示（C1 一并）。
- **P3-8 AC-23 c/e 网络默认 deny 的 curl SSL 误导**：spike 已说明 curl 失败可能是 `/private/etc/ssl` 不可读；建议 AC/文档保留 hint（避免用户误判网络策略）。
- **P3-9 hardlink/firmlink 的 spike 边界**：spike 仅 bash 层验证；文件工具 Rust 层需独立负例（C4 一并）。

## 4. 验证证据

评审人亲自执行（worktree `/Users/zyzs/lczyzs/code/zgraph/.worktrees/docs-docs-dev`，HEAD `5f15de9`，main `933a80d`，worktree 干净）：

```bash
git status --porcelain                 # 空
git diff --stat main...HEAD            # 12 files, +820/−8（全部 doc/**；无代码/测试/既有评审）
git diff --name-only main...HEAD       # DR-012/013/014 + spec 00/02/05/06/07/08 + index/issues/progress
bash scripts/dev/check.sh              # CHECK PASSED（fmt/diff-check 等）
```

- **spike 附录 A 对照**：profile 核心元素与 DR-013 §5.1/§11 一致——`(version 1)`+`(deny default)`+`(debug deny)`、`/private/...` realpath、系统只读路径集合（含 `/private/var/select/sh`、CLT、`file-read-metadata`、`/dev/*`、mach 服务）、workspace `file-read* file-write*`、`--allow-path` 仅 `file-read*`、`process-exec` 按目录收窄、网络默认 `deny network*`、`--allow-network` 追加 SSL/hosts/resolv/networkd、`TMPDIR=<workspace>/.tmp`；DR-013 §5.3/§11 的 exit71（nested→`sandbox_nested`）/exit65（`sandbox_profile_invalid`）/fail-closed/无 `--unsafe-bash`/预检 exit3 与 spike 一致；性能 11 ms/22–40 ms（中位 25 ms）、31/31 断言、macOS 26.6、Linux/Windows 不支持均已在 §5/§11 落盘。**未合入/矛盾处**：spike 附录 D 的 hardlink/firmlink 文件工具面（C4）、spike D 64 KiB/blobs 与冻结 16 KiB/无 blobs 的收窄说明（P3-4）、spike B `sh -c` 与冻结 `/bin/bash --noprofile --norc -c` 的关系（C2 的 `-lc` 残留是明确矛盾）。
- **文档锚点/AC 可测性抽查**：AC-21 a–g、AC-22 a–d、AC-23 a–e、AC-24 a–j 均有可执行断言方向（真 CLI/workspace 快照/pid/tool_calls SQL/迁移备份）；AC-24 g 的「不执行工具」三重证据、AC-23 d exit71/65/precheck exit3 可测；C1–C4 修正后可直接转 D2–D5 用例。
- **范围/红线**：docs-only；无 `.cargo`/`dist/`/token；spec 仅草案追加；`doc/reviews` 未改；`doc/index`/`issues`/`progress` 同步（版式问题见 P3-2/P3-3）。
- **与自报对比**：文档范围/提交自报一致；未发现代码/测试改动；check.sh 全绿。

## 5. 条件关闭方式

- **C1–C4 由 docs-dev 在 D1 内 docs 小 commit 关闭**（仅改 DR-012/013/014 + spec 草案 + AC 表；不改代码）：
  - C1：统一 `--allow-path` 只读（DR-013 §2.1/§2.2/§10、05 §5.15.2 + AC-21 f 写负例）。
  - C2：统一 bash 执行形式/输出/超时/command 上限（DR-013 §3.8/§5.2、05 §5.15.3、00 常量）。
  - C3：统一 read/search/v7 限额与 00 常量（DR-013 §3.1/§8、DR-012 §8、00 §0.5、06 §6.15）。
  - C4：补 hardlink/firmlink 文件工具策略与负例（或显式接受残余风险 + ISSUE 记录，AC 注明）。
- P2-1/P2-2/P2-3 建议同批关闭；P3 可随后 docs 清理。
- 关闭后评审人只读复核（重点 grep 竞态数字/`-lc`/读写全开/64 KiB/read 256 KiB）并追加「条件关闭」段；全部关闭后结论改「通过」，再 merge D1 → D2。

## 6. 条件关闭记录（"有条件通过"）

| 条件 | 状态 | 关闭要求 |
|---|---|---|
| C1 `--allow-path` 统一只读 + AC-21 f 写拒绝 | **closed（923a89a/2132478）** | DR-013 §2.1/§2.2/§10、05 §5.15.2/§5.15.3、spike allowlist 均只读；写 → `TOOL_ERROR{path_read_only}`；AC-21 f 负例在 spec 07 + DR-013 §9；旧值「读写全开」grep 清零；ISSUE-175 记 `--allow-write-path` 后续 |
| C2 bash `--noprofile --norc -c`/16 KiB/120s/command 上限统一 | **closed（923a89a + 2132478）** | `923a89a` 统一 no-login/120s/command 8 KiB；`2132478` 把 DR-013 §3.8:180 与 05 §5.15.3:1515 的 32 KiB 改 **16 KiB head+tail**；复核无 `-lc`（仅「禁 -lc」说明）/131072/各 32 KiB 残余 |
| C3 read 1 MiB/10000、v7 32/16、search/glob/command/path 常量统一 | **closed（923a89a）** | read 1 MiB/10000（默认 2000）/结果 32 KiB；grep/glob 32 KiB/默认 1000/上限 5000；v7 32/16；00 常量补全（含 `BUILTIN_GLOB_*`/`BASH_COMMAND_MAX_BYTES`/`TOOL_CALL_PATH_MAX_BYTES`）；DR-012 §8 已改 32/16 |
| C4 hardlink/firmlink 文件工具策略 + 负例（或接受残余风险+ISSUE） | **closed（923a89a）** | DR-013 §2.2:55 明确 hardlink 残余风险接受（无法可靠识别）+ ISSUE-167；`/System/Volumes/Data` firmlink 依赖 canonicalize；spec 07 AC-21 f 注明只承诺 symlink/`..`/绝对路径 |

## 7. 遗留问题

- C1–C4/P2 已关闭（§8）；P3 已清理主要项，DDL 命名/约束留 D2/D5。
- spike 附录 A/D 的 31/31 断言与真实 `sandbox-exec` 已由 arch-researcher 在沙箱外执行器验证；本评审只读报告与 DR 落盘，未重跑（D4 实现时应有独立负例）。
- 设计分支 docs-only，不触发 workspace 测试；D2–D5 实现后按 DR-012 发布条件补全测试/门禁。
- 待 Lead 裁决确认：C1（只读）/C2（120s/16 KiB/8 KiB command）/C3（read 1 MiB/10000、v7 32/16）是否按本记录建议冻结；若 Lead 选择不同值，D2 前需在同一 D1 文档中明确唯一值。

## 8. 条件关闭复核（2026-11-02）

复核提交链：`f06ca47`（首轮评审）→ `923a89a`（C1/C3/C4 + P2/P3 主体）→ `2132478`（C2 剩余两处 16 KiB、index 拆行、AC-21 f 负例）。评审人执行：

```bash
git status --porcelain                      # 空
git diff --stat 923a89a..2132478            # 3 files +5/−4（DR-013 / 05 / index；docs-only）
bash scripts/dev/check.sh                   # CHECK PASSED
# v0.4 段旧值 grep：-lc / 131072 / 读写全开 / 64 KiB / 256 KiB / 各 32 KiB
# → 仅命中「禁 -lc」说明与既有 06 payload 64 KiB；无 v0.4 旧值残留
```

| 条件 | 复核结果 |
|---|---|
| **C1** `--allow-path` 只读 | ✅ 全链统一只读（DR-013 §2.1/§2.2/§10、05 §5.15.2/§5.15.3、spike allowlist）；写 → `path_read_only`；AC-21 f 负例（spec 07 + DR-013 §9）；`--allow-write-path` 记 ISSUE-175 |
| **C2** bash 形式/上限 | ✅ `/bin/bash --noprofile --norc -c`；16 KiB head+tail（`923a89a` + `2132478` 清 DR-013 §3.8/05 §5.15.3 两处 32 KiB）；command 8 KiB、timeout 120s/600s、drain 2 MiB；无 `-lc`/131072/30s v0.4 残留 |
| **C3** 限额/常量 | ✅ read 1 MiB/10000（默认 2000）/结果 32 KiB；grep/glob 32 KiB/1000/5000；v7 32/16；00 常量补全；DR-012 旧值改 32/16 |
| **C4** hardlink/firmlink | ✅ hardlink 残余风险接受 + ISSUE-167；firmlink 依赖 canonicalize；AC-21 f 明确只承诺 symlink/`..`/绝对路径 |
| **P2** | ✅ 06 §6.15.4 reexec 标 v0.4 未实现/v0.5；08 §8.13 补 `--allow-exec-path`；00 §0.4 登记 `REPLAY_TOOL_*`（CLI/append-only） |
| **P3** | ✅ index DR-014 行已拆；DR-013 §9 AC-21 f 同步；DR-012/013 spike 状态已更新；issues 版式/ISSUE-175 已补；spike 64K/blobs 收窄说明已写入 DR-014 §10；DDL 命名/约束为后续 D2/D5 小项（不阻塞冻结） |

**最终状态**：C1–C4 全部 closed；P2 全部 closed；P3 无阻塞项。`923a89a..2132478` 仅 3 个 docs 文件改动，无代码/测试/既有评审改动；`check.sh` PASSED。**建议 Lead merge D1 → 开 D2（文件类工具 + jail）。**
