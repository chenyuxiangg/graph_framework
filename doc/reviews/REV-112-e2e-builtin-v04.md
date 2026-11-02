# REV-112 · e2e · v0.4 D5c AC-21–24 真链路

- **分支**：`test/e2e-builtin-v04`
- **作者**：`e2e-dev`（单提交 `598ccff`，基于 main `1396726`）
- **评审人**：`reviewer-b`（独立评审，≠ 作者）
- **变更范围**：`1396726..598ccff`（8 files，+2841/−0）：`tests/e2e/{builtin_files_v04,builtin_search_v04,builtin_bash_v04,trace_tools_v04,v04_regression}.rs`、`tests/e2e/v04_support/mod.rs`、`tests/e2e/Cargo.toml`（5 个 `[[test]]`）、`doc/test-baseline.txt`；无产品代码/golden
- **评审时间**：2026-11-02
- **关联**：task-89/90、DR-012 AC 矩阵、DR-013、DR-014、spec 07 AC-21–24、05 §5.15、06 §6.15、REV-106/108/110/111

## 1. 结论

**结论：通过**（首轮有条件通过 → C1/P2 由 `7530bec`+`0a47295` 关闭；历史见 §7）

P0=0；**P1=0**；P2 残余已由 `0a47295` 关闭（见 §7）；P3 后续。真 CLI 链路成立，AC-21 文件/jail、AC-22 搜索、AC-23 嵌套 fail-closed、AC-24 trace/migration/replay 主体覆盖到位；最终 e2e **23 target / 166 passed / 0 failed / 2 ignored（连跑 2 次）**、workspace **1216/0/3**、pytest **183**、clippy/fmt/check/no-default 全绿。

> **首轮历史（保留）**：`598ccff` 首轮为有条件通过——C1 `ac21g` 预检失败 DB 断言对象错误（恒真）。`7530bec` 已关闭 C1，并补多文件 apply_patch side_effect NULL / write/edit 失败不变；read cap 补强未真正断言（P2 残余）。

首轮条件：

- **C1 「预检失败不建 DB」断言对象错误**：`builtin_files_v04.rs::ac21g_invalid_workspace_and_allow_path_preflight_exit3` 构造 `db = dir.join("ws-invalid.db")` / `allow-invalid.db`，随后 `assert!(!db.exists())`；但 `run_scenario` 实际使用的 DB 是 `dir/trace.db`（`RunOutcome.db`）。这两个自定义路径从未被执行，断言恒真，无法证明预检失败未建 DB。任意“先建 DB 再预检失败”的产品回归都能通过该测试。

## 2. 四维评审

| 维度 | 结论 | 依据 |
|---|---|---|
| 功能正确性 | ✅ | 真 CLI/真 DB/真工具主链路；C1 DB 断言已修正并有正向对照；P2 残余已由 `0a47295` 关闭 |
| 结构合理性 | ✅ | v04_support 复用 v03，CLI/DB/workspace 快照/环境探测分层清晰；5 target 独立；未改 golden |
| 逻辑性 | ✅（P2） | AC 映射基本逐条；环境感知分支 fail-closed；P2 为覆盖缺口 |
| 可读性 | ✅（P3） | AC 注释/环境限制/基线说明清楚；P3 为补强 |

## 3. 发现清单

### P1 · 合并前必须关闭

| # | 位置 | 问题 | 条件 |
|---|---|---|---|
| C1 | `tests/e2e/builtin_files_v04.rs::ac21g_invalid_workspace_and_allow_path_preflight_exit3` | `db = dir.join("ws-invalid.db")`、`allow-invalid.db`，但 `run_scenario` 使用 `dir/trace.db`（`outcome.db`）；`assert!(!db.exists())` 检查未使用路径，恒真。AC-21 的「exit 3 且不建 DB」未被真正验证 | 改用 `assert!(!outcome.db.exists())`（或把真实 db 路径传入 `run_scenario`）；workspace/allow-path 两段都要；补一条“若建 DB 则断言失败”的负向自检（如临时复制/故意预建路径，或至少断言 `outcome.db` 的 parent 状态）。修后重跑 e2e |

### P2 · 建议

| # | 位置 | 问题 | 建议 |
|---|---|---|---|
| N1 | AC-24 f | `trace_tools_v04` 9 用例为 a/b/c/d/e/i/g/g-corrupt/h，**无 ac24f bash 元数据**；仅 `builtin_bash_v04` 非嵌套分支（本机不可执行）检查 `bash_command/bash_cwd/profile_sha`，D4 单测有覆盖但 e2e 无显式说明 | D6 as-built/AC 矩阵注明 AC-24 f 由 D4 单测 + 用户普通终端脚本覆盖；若要求 e2e，补环境感知的 trace bash 字段断言 |
| N2 | AC-23 非嵌套分支 | `ac23a` 非嵌套分支只测 workspace 写/`/tmp` 写/超时/16 KiB；未覆盖任务要求的 `/etc/passwd` 读拒、`~/.ssh`、symlink 逃逸、network deny/`--allow-network`、`--allow-exec-path` 收窄（本机嵌套不可执行，D4 单测/verify 脚本覆盖部分） | D6 明确 e2e 非嵌套分支的覆盖边界与 D4/用户脚本证据分工；如普通终端可运行，补最小断言 |
| N3 | AC-21 b/c | `ac21b` 的「原子」只测 `file_exists` 失败不改，未测 write 临时文件/rename 失败；`ac21c` e2/edit 失败后无再次 read 断言（只复用 e1 前 r1），失败不改由 D2 单测覆盖 | e2e 补 write 失败原子（可构造父目录权限/rename 失败）与 edit no_match/multiple 后 read；或 D6 注明由 D2 单测覆盖 |
| N4 | AC-21 d / AC-24 e | `ac21d` 多文件 apply_patch 只断言 result files[] 数量，未断言 trace `side_effect` 为 NULL；`ac24e` 只有单文件 apply_patch，无多文件 trace 行断言 | 补多文件 row `side_effect_*` 全 NULL + result files[]；单/多同 run |
| N5 | AC-24 d/source | `ac24d` 工具行全为 builtin；未覆盖 MCP/Python source 的 e2e 行（D5b1 单测/既有 mcp e2e 覆盖） | D6 说明分工；可在 v0.4 e2e 加 Python tool（非 mock）行 |
| N6 | AC-21 a | 未断言 read 大文本结果 32 KiB `truncated`（只测 1 MiB 文件拒绝）；`ac21a` 的 `r3` 只测空结果未断 total_lines | 补 read 结果 cap/line offset 边界；或注明 D2 单测 |

### P3 · 测试/文档

| # | 位置 | 问题 | 建议 |
|---|---|---|---|
| N7 | `ac23c` | 仅以 stderr 不含 `sandbox` 证明未探测；没有进程/spawn 计数 | D6 可用 CLI 侧日志/探针加强 |
| N8 | 环境 | 本机嵌套 Seatbelt；AC-23 非嵌套分支/真实隔离未执行 | 用户普通终端 `verify-sandbox.sh` 作 D6 证据（已有） |
| N9 | 稳定性 | 本评审 e2e 连跑 2 次 + workspace 1 次全绿；作者观察 `kernel_cancel_kills_worker_process_group` 偶发，本分支无 kernel 代码，未复现 | D6 登记既有 flake，观察/加固 |
| N10 | `doc/test-baseline.txt` | 新基线 23 target/165 passed 与实际一致；workspace/pytest 以门禁日志为准 | D6 保持 |

## 4. 验证证据

worktree `/Users/zyzs/lczyzs/code/zgraph/.worktrees/e2e-e2e-dev`，HEAD `598ccff`，base `1396726`，工作区干净；`CARGO_HOME=/Users/zyzs/lczyzs/code/zgraph/.cargo-home`，未设 `CARGO_TARGET_DIR`：

```bash
cargo test -p zgraph-e2e            # 第 1 次：23 target / 165 passed / 0 failed / 2 ignored
cargo test -p zgraph-e2e            # 第 2 次：23 target / 165 passed / 0 failed / 2 ignored
cargo test --workspace              # 89 targets / 1215 passed / 0 failed / 3 ignored
pytest -q                           # exit 0；183 passed
cargo clippy --workspace --all-targets --locked -- -D warnings  # clean
cargo fmt --all -- --check          # exit 0
cargo check -p zgraph-harness --no-default-features  # clean
bash scripts/dev/check.sh           # PASSED
```

- **与自报一致**：e2e 23 target/165/0/2 连跑 2 次、workspace 1215/0/3、pytest 183、clippy/fmt/check/no-default 全绿；`git status` 空；范围仅 e2e/fixtures+Cargo 测试清单+baseline（无产品代码/golden）。
- **AC-21**：read offset/limit/UTF-8/二进制/不存在/目录/1 MiB/绝对/allow；write 覆盖/create_parents/allow 只读；edit 唯一/0/多匹配/replace_all；apply_patch **真实 `git diff --no-index`** 单/多文件成功 + 多文件原子失败 + unsupported；diff 不写/空/越界；jail `..`/绝对/symlink 出根/兄弟前缀/写越界/path_read_only；无 opt-in 不可见/零副作用；预检 exit3（C1 修复后补真实 DB 断言）。
- **AC-22**：regex/fixed/大小写/glob 过滤；.gitignore/隐藏/二进制/排序/100 条/32 KiB；glob 排序/隐藏/仅文件/上限/越界；无 opt-in。
- **AC-23**：本机嵌套 Seatbelt `sandbox-exec` probe 71 → `--allow-bash` exit3、不产生 run、**不建 DB（`outcome.db`）**、stdout 空、stderr `HARNESS_CONFIG_LOAD`+`sandbox_nested`+exit71、workspace 零副作用；opt-in 双层/依赖 flag exit2/无 unsafe；默认/仅文件路径不探测。非嵌套分支代码保留但本机未执行（见 §5）。
- **AC-24**：v6→v7 迁移/备份/旧数据/旧 run trace；只读 v6 exit4；无工具 additive；恰 1 行/字段/call_id/source/status/attempt/duration/sha；side effect sha/bytes；trace human/JSON/stats + 32 KiB cap；recorded `ZGRAPH_PYTHON=/nonexistent` 还原且 workspace/DB/-wal/-shm/行数/runs 零副作用；MISSING/TRUNCATED/MISMATCH fail-closed exit3；reexec exit2 + events/outputs 不变；v04 默认 zero config/graph hash/零额外文件。
- **稳定性**：本评审 e2e 2 次全绿；workspace 全绿；`kernel_cancel_kills_worker_process_group` 未复现（分支无 kernel 改动，归属既有 flake 观察）。

## 5. 环境限制确认

- **本机嵌套 Seatbelt**：`builtin_bash_v04::ac23a` 走 `Some(71)` 分支，**实测** `--allow-bash` preflight **exit 3、不产生 run、不建 DB（`outcome.db`）**、`sandbox_nested`、exit 71 诊断、workspace 快照不变；**未伪造成功、未 skip 关键断言**。
- **非嵌套分支**：代码保留（workspace 写/`/tmp` 越界/超时/16 KiB head+tail），本机**不可执行**；未通过任何 mock/放宽伪装。
- **真实隔离（AC-23 c/AC-24 f）**：需用户普通终端 `crates/zgraph-harness/scripts/verify-sandbox.sh`；D6 需标注该证据来源与 `AC-24 f` 由 D4 单测覆盖的边界。**本评审确认当前环境未产生真实隔离证据，也不声称已产生。**

## 6. 遗留问题

- **C1**：修正 DB 断言并重跑；当前 e2e 全绿不能证明预检失败不建 DB。
- **P2 N1–N6**：AC-24 f/D4 分工、AC-23 非嵌套边界、write/edit 原子补强、多文件 side_effect NULL、source 覆盖、read result cap。
- **P3 N7–N10**：默认探测探针、真实终端证据、既有 kernel flake、baseline。
- 无 P0；C1 未关闭前不可 merge。

## 7. 条件关闭复核（2026-11-02，`7530bec`，parent `97f68b7`）

**复核结论：通过**。`7530bec` 3 files（`builtin_files_v04.rs` +138/−5、`trace_tools_v04.rs` +44/−2、baseline 计数），+183/−8；工作区干净；仅 e2e/baseline。

### C1（DB 断言有效性）
- 两段改用真实 `assert!(!outcome.db.exists())`（workspace_invalid / allow_path_invalid）。
- **正向对照**：独立 TempDir 用同一 `run_scenario`/`builtin_flags` 合法 run，断言 `control.db.exists()` 且 exit 0，证明 `!outcome.db.exists()` 不是恒真。已实测通过。
- **C1 关闭**。

### P2 复核
- **多文件 apply_patch**：`ac24e` 新增 `s5` 两文件 patch；trace 行恰 1，`side_effect_path/before/after/bytes_before/bytes_after` 全 NULL；result.files[] 2 条且每文件 sha256_before/after 64 hex；文件内容 `one\nTWO!\n`/`B1\n`。**已关闭**。
- **write/edit 失败不变**：`ac21b_atomic_failures_leave_files_untouched` 覆盖 file_exists/no_match/multiple_matches，断言 bytes/mode/mtime/mtime_nsec 不变 + workspace 树快照不变。但**未覆盖 temp/rename 失败**，也未记录/断言 inode（与提交说明“inode 不变”不符）；文件系统原子写失败仍由 D2 单测覆盖。**部分关闭（P2 残余）**。
- **read 40k → truncated/≤32768**：`ac21a` 只增加了 `r11` 调用与 `rows.len()==11`，**没有对 r11 的 `result.truncated`、`raw_result_json.len()<=32768` 做任何断言**；即使 read 返回完整 40k，测试仍通过。**未关闭（P2 残余）**。

### 最终门禁（`7530bec`）
```bash
cargo test -p zgraph-e2e            # 第 1 次：23 target / 166 passed / 0 failed / 2 ignored
cargo test -p zgraph-e2e            # 第 2 次：23 target / 166 passed / 0 failed / 2 ignored
cargo test --workspace              # 89 targets / 1216 passed / 0 failed / 3 ignored
pytest -q                           # exit 0；183 passed
cargo clippy --workspace --all-targets --locked -- -D warnings  # clean
cargo fmt --all -- --check          # clean
bash scripts/dev/check.sh           # PASSED
```
- baseline 已更新为 **23 target / 166 passed**，与实际一致；新增 `builtin_files_v04` 10 用例。
- 稳定性：e2e 2 次 + workspace 1 次全绿；`kernel_cancel_kills_worker_process_group` 未复现（分支无 kernel 改动）。
- 范围：仅 e2e/baseline；无产品代码/golden 篡改；无 mock 被测对象。
- **P2 残余**：read cap 未断言（上）；write 原子失败未覆盖 temp/rename 且 inode 未记录；AC-24 f/AC-23 非嵌套边界/多 source/多文件 etc. 仍按首轮 P2 留 D6。
- **环境限制**：仍为嵌套 Seatbelt；AC-23 非嵌套分支不可执行、真实隔离证据归用户普通终端 `verify-sandbox.sh`（D6），本轮未伪造。

### P2 残余关闭（2026-11-02，`0a47295`，parent `1c53382`）

- **read cap 已补真实断言**：`ac21a` 对 `r11`（40k 字符 `long.txt`, limit 10000）读取实际 `tool_calls` 行，断言 `result.truncated == true`、`raw_result_json.len() <= 32768`、`result.content.chars().count() < 40000`；无降级/绕道。定向 `cargo test -p zgraph-e2e --test builtin_files_v04` **10/10 passed**。
- **原子失败说明已更正**：`ac21b_atomic_failures_leave_files_untouched` 注释/提交说明明确失败发生在写入前（exists/edit 匹配失败），只断言 bytes/mode/mtime+mtime_nsec 不变 + workspace 无 temp 残留；**不**宣称 inode 不变，**不**覆盖 temp→rename 回滚（由 D2 单测覆盖），口径诚实。
- 仅 `builtin_files_v04.rs` +28；无产品代码/基线计数变化；结论保持 **通过**。
