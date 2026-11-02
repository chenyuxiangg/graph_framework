# REV-106 · harness · v0.4 D2 内置文件工具 + workspace jail

- **分支**：`feat/harness-builtin-file`
- **作者**：`dsl-dev`（单提交 `b8ab337`，基于 main `051bf11`）
- **评审人**：`reviewer-b`（独立评审，≠ 作者）
- **变更范围**：`051bf11..b8ab337`（17 files，+2897/−31）：`builtin/{mod,jail,files,patch}.rs` 新增；`tools/agent_loop/executor/config/constants/lib/provider/plugin/test_support`；CLI `run_cmd` 机械；`Cargo.toml/Cargo.lock`（sha2/similar、nix）
- **评审时间**：2026-11-02
- **关联**：task-77/78、DR-013 §2/§3.1–3.5/§4/§8/§11、DR-012、spec 05 §5.15、07 AC-21、00 §0.5、REV-105

## 1. 结论

**结论：通过**

P0=0；**P1=0**；P2 残余（不阻断，见 §7）；P3 后续。首轮 **有条件通过** 的 C1（标准 git diff 头）+ P2 已于 `4b093fc` 关闭；jail、5 工具、opt-in/白名单、CLI、trace 骨架、依赖均通过；最终 harness **279/0/1**（默认与 features mcp）、workspace **1082/0/3**、pytest **183**、clippy/fmt/check/no-default 全绿。

> **首轮历史（保留）**：`b8ab337` 首轮为**有条件通过**——C1 `apply_patch` 顶层不识别标准 `diff --git`；P2 为 reserved prefix/reason/原子清理等。`4b093fc` 已关闭 C1 + N1/N2/N3/N4/N6，见 §7。

## 2. 四维评审

| 维度 | 结论 | 依据 |
|---|---|---|
| 功能正确性 | ✅ | jail/5 工具主路径；C1 git diff 头 + N1/N4/N6 已修并测试；P2 残余见 §7 |
| 结构合理性 | ✅ | `builtin` 模块分层 jail/files/patch；工具定义固定；executor 仅 additive 注入；CLI 机械 3 flag；工具源冲突集中 registry |
| 逻辑性 | ✅（P2） | jail 解析顺序（词法→canonicalize→组件前缀）、allow-path 只读、write 原子/编辑 CAS/apply 两阶段均按 DR；P2 见 §3 |
| 可读性 | ✅（P3） | DR 注释/错误闭集/结果 sha 清晰；P3 为测试/文档补齐 |

## 3. 发现清单

### P1 · 合并前必须关闭

| # | 位置 | 问题 | 条件 |
|---|---|---|---|
| C1 | `crates/zgraph-harness/src/builtin/patch.rs::parse_patch`（顶层） | 标准 `git diff` 以 `diff --git a/f b/f` 开头；顶层没有该分支（`parse_file` 内才把 `diff --git` 当文件边界），因此直接 `unsupported_patch`。多文件 git diff 同样在开头失败；测试只用手写 `---/+++` 未覆盖 | 顶层跳过/识别 `diff --git`（含 `index`/mode 行），继续解析后续 `---/+++`；补「真实 `git diff` 单文件+多文件」测试，保留 rename/copy/binary 的 `unsupported_patch`。修复后复跑 |

### P2 · 建议（不阻断 C1 关闭，但发布前最好处理）

| # | 位置 | 问题 | 建议 |
|---|---|---|---|
| N1 | `executor.rs::registry` | Python 工具 `builtin__` 前缀**仅在 `builtin_tools Some` 时拒绝**；未 opt-in 时 Python 可注册保留前缀（虽不可见，仍违反 DR-013 §1.1 命名空间保留） | 无论是否 opt-in，均在 registry 构造时拒绝 Python `builtin__*`（reason `reserved_prefix`）；补未 opt-in 负例 |
| N2 | `files.rs::write_staged_atomically` | `write_temp` 失败时已创建的前序 temp 未清理；`atomic_write` rename 失败 temp 亦残留；rollback 为 best-effort | 失败路径统一 cleanup temps；`atomic_write` rename 失败删除 temp；补「第二个文件 temp 写失败」测试（当前只测第一个 locked） |
| N3 | `files.rs::write_staged_atomically` / `rollback_renamed` | 已 rename 文件回滚用 best-effort `atomic_write`，失败则目标可能部分新；rollback 不恢复原权限 | 明确文档「rename 级窗口/回滚失败为残余」；rollback 恢复 permissions；补 CAS 失败（rename 前）回滚测试 |
| N4 | `files.rs::edit` / `stage_patches` | edit/apply_patch 对不存在目标：`resolve(Write)` 允许不存在，随后 `read_bounded` 返回 `io_error`（而非 `not_found`） | edit 先要求 `resolved.existed`；apply_patch 目标不存在返回 `not_found`（DR 错误闭集更准确） |
| N5 | `files.rs::read_bounded` | stats 后 `std::fs::read` 无再上限；文件在 stat 后增长可读超 1 MiB（TOCTOU） | 读取用 `take(max+1)`/分块并二次校验；超限 `too_large`。同类 `edit` CAS 的 `std::fs::read` 也补 |
| N6 | `files.rs::diff` | `old_string`/`new_string` 无输入长度上限（仅结果 cap）；大参数在 `similar` 计算/内存放大 | 对 old/new 合计加 `BUILTIN_WRITE_MAX_BYTES` 级上限；超限 `too_large` |
| N7 | `files.rs::limit_result`（apply_patch） | `files` 数组超 32 KiB 时只 shrink `content/diff`，最终 fallback 仅 `{truncated,sha256}`，丢失 files 统计（虽有截断标记） | 对 `files` 截断保留前 N 项 + `truncated`；或明确 fallback 语义；补测试 |

### P3 · 台账/测试

| # | 位置 | 问题 | 建议 |
|---|---|---|---|
| N8 | `builtin/files.rs` | 无 `diff --git` 解析测试；无 edit/apply CAS 并发/CAS 失败回滚测试；无 `write_temp` 第二个失败/rename 失败测试 | 随 C1/N2/N3 补；CAS 需可控注入或权限模拟 |
| N9 | `builtin/files.rs::write` | 原子写 fsync 文件但未 fsync 父目录；temp 名冲突无重试；rename 失败 temp 未删 | 发布前补 fsync parent/temp 清理（磁盘耐久性） |
| N10 | `builtin/jail.rs` | hardlink 残余风险注释已有；`create_parents` 的 TOCTOU、allow-path 父目录、workspace 根为 `/` 的显式信任文档沿用 DR | 保持文档；无需代码 |
| N11 | `apply_patch` | 仅使用 `new_path`，不校验 old/new 一致性；rename/copy 已 unsupported，但手写 `--- a/x`/`+++ b/y` 可能被当作 y | 校验 old/new strip 后相同（或明确只读 new_path 语义） |
| N12 | `dsh`? 无 | D2 只文件工具；bash/grep/glob D3/D4，trace v7 D5 未做，符合 task 范围 | 发布 as-built 时列 D3–D5 待办 |

## 4. 验证证据

worktree `/Users/zyzs/lczyzs/code/zgraph/.worktrees/harness-dsl-dev`，HEAD `b8ab337`，base `051bf11`，工作区干净；`CARGO_HOME=/Users/zyzs/lczyzs/code/zgraph/.cargo-home`，未设 `CARGO_TARGET_DIR`：

```bash
cargo test -p zgraph-harness                 # 270 tests：269 passed / 0 failed / 1 ignored
cargo test -p zgraph-harness --features mcp  # 270 tests：269 passed / 0 failed / 1 ignored
cargo test --workspace                       # 76 targets：1072 passed / 0 failed / 3 ignored
pytest -q                                    # exit 0；183 passed
cargo clippy --workspace --all-targets --locked -- -D warnings  # clean
cargo fmt --all -- --check                   # exit 0
cargo check -p zgraph-harness --no-default-features  # clean
bash scripts/dev/check.sh                    # PASSED
```

- **与自报一致**：harness 269/0/1（默认与 mcp）、workspace **1072/0/3**、pytest **183**；clippy/fmt/check/no-default 全绿；`b8ab337` 工作区干净。
- **范围**：17 files；跨模块 CLI `run_cmd` 机械（3 flag/预检/透传）+ Cargo `sha2`/`similar`（+nix target），Lead 已批准；无既有评审/测试改动；`git status` 空。
- **关键断言**：jail `..`/绝对/symlink 出根/sibling 前缀/allow-path 只读/special/缺失父目录；5 工具 schema/结果 cap/sha；write 原子/overwrite evidence/create_parents；edit 唯一/0/多匹配不变/replace_all；apply_patch 多文件严格原子/unsupported/patch_mismatch/strip/写失败无残留；diff 生成不写/cap；opt-in/白名单/未 opt-in reason/registry 冲突；builtin streaming ToolCallStart/End + sink；默认零注册。
- **未覆盖（P2/P3）**：标准 `diff --git`（C1）、edit/apply CAS 并发/回滚、write temp 二次失败/残留、read 增长 TOCTOU、diff 大输入 cap、limit_result apply fallback；D3/D4/D5 不在本批。

## 5. 跨模块 / 开放项确认

- **CLI 机械**：`--allow-builtin-tools`、`--workspace-dir`、`--allow-path`（可重复）+ usage + 预检 exit 3；`validate/trace` 未加；`HarnessExecutor` `with_builtin_tools` additive；默认无 flag 零解析/零注册。
- **Cargo**：`sha2`/`similar` 与 harness/Cargo.lock；无 feature 默认构建变化；no-default check 通过。
- **dsl-dev 开放项**：①仅 5 工具（grep/glob/bash 留 D3/D4）✅；②跨源冲突：builtin `reserved_prefix`/`tool_name_conflict` 代码存在，**但 N1 未 opt-in 时 Python 保留前缀未拒绝**；MCP↔Python 保留 `HARNESS_MCP_ERROR{name_conflict}`（未改为 CONFIG_LOAD）✅（D6 注明）；③env 未实现（CLI only）✅；④apply_patch 仅改已有文件 ✅（N4 reason 可改进）；⑤新 ToolFailure reason 可 D6 回写 ✅；⑥read 全量 sha256 ✅。
- **trace**：ToolCallStart/End 既有；`ToolCallRecordSink` no-op、无新表/码；builtin 调用 streaming 测试通过。

## 6. 遗留问题

- **C1**：标准 git diff 头支持 + 测试；修复后复审转「通过」。
- **P2 N1–N7**：建议在 C1 或发布前处理（尤其 N1 保留前缀、N2/N3 原子清理/回滚、N4 reason）。
- **P3 N8–N12**：测试/耐久性/待办记录；D3/D4/D5 属后续波次。
- 无 P0；无附加阻断条件除 C1。

## 7. 条件关闭复核（2026-11-02，`4b093fc`，parent `edb4831`）

**复核结论：通过**。`4b093fc` 仅 3 files（`builtin/files.rs`、`builtin/patch.rs`、`executor.rs`，+360/−32）；工作区干净。

### C1（标准 git diff）
- `parse_patch::is_ignorable_header` 识别 `diff --git `；`index/mode` 继续跳过；`parse_file` 仍以 `diff --git` 为文件边界。
- 新增 `git_diff_tests`：真实单文件、真实多文件、`\ No newline`、rename/copy/binary 仍 `unsupported_patch`；**4/4 通过**（`builtin::patch::git_diff_tests`）。
- 单/多文件路径/`old_path`/`new_path`/apply 结果断言通过。

### 同批 P2 关闭确认
- **N1**：`executor::registry` 现在**无论 opt-in 与否**都拒绝 Python `builtin__` 前缀；`reserved_builtin_prefix_is_rejected_without_opt_in` 通过。
- **N2**：`write_staged_atomically` 的 `write_temp` 失败清理前序 temp；`atomic_write` rename 失败删除 temp；`apply_patch_second_temp_failure...`、`atomic_write_removes_temp_when_rename_fails` 通过。
- **N3**：`rollback_renamed` 恢复内容+权限（测试 `rollback_restores_original_content_and_permissions`）；残余风险文档化（rename 级窗口/回滚失败 best-effort）。
- **N4**：`stage_patches` 缺失目标→`not_found`；`read_bounded` `File::open` 缺失→`not_found` 且有界 `take(max+1)`；`edit_and_apply_patch_report_not_found...` 通过。
- **N6**：`diff` old/new 合计 >1MiB→`too_large`；`diff_rejects_oversized_inputs` 通过。
- **N5/N7/N9/N11**：按 Lead 预裁决留 D5/D6（未在本批）；P2 残余：
  - `write_staged_atomically` 的 **CAS 读取错误**路径（`std::fs::read?`）仍可能在已 rename 后直接返回，未 cleanup/rollback；建议 D5/D6 与 N3 一并补异常路径测试。
  - `limit_result` apply files 超限 fallback、write fsync parent、apply old/new path 校验等仍按原 P2/P3。
- **N8 测试**：C1/CAS/N2/N3 新增覆盖；N5/N7/N9/N11 后续。

### 最终门禁（`4b093fc`）
```bash
cargo test -p zgraph-harness                  # 280 tests：279 passed / 0 failed / 1 ignored
cargo test -p zgraph-harness --features mcp   # 280 tests：279 passed / 0 failed / 1 ignored
cargo test --workspace                        # 76 targets：1082 passed / 0 failed / 3 ignored
pytest -q                                     # exit 0；183 passed
cargo clippy --workspace --all-targets --locked -- -D warnings  # clean
cargo fmt --all -- --check                    # clean
cargo check -p zgraph-harness --no-default-features  # clean
bash scripts/dev/check.sh                     # PASSED
```
- **一次既有 codex 并行 flake**：首轮 features/pytest 出现 `codex_adapter_*`/`ac20h` 超时（非 D2 文件）；重跑 harness 默认/features 279/0/1、workspace 1082/0/3、pytest 183 全绿；隔离 `plugin::tests::codex` 串行 2/2、pytest codex 10/10 通过。属既有 Codex fake adapter 负载敏感，建议 D5 前加固（不阻断 D2）。
- 范围：`4b093fc` 3 files；默认路径无回归；无既有评审/产品代码回退。
