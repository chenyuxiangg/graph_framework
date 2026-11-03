# REV-120 · harness · v0.5 F2 sandbox 增强（allow-write-path + 构建工具链）

- **分支**：`feat/harness-allow-write-path`
- **作者**：`dsl-dev`（单提交 `0415531`，基于 main `d3da745`；origin/main `21846d8` docs-only）
- **评审人**：`reviewer-b`（独立评审，≠ 作者）
- **变更范围**：`d3da745..0415531`（5 files，+685/−13）：`builtin/jail.rs`、`builtin/sandbox.rs`、`builtin/files.rs`、`builtin/bash.rs`、`cli/run_cmd.rs`；无 Cargo 变更（跨模块 cli 接线 Lead 已批准）
- **评审时间**：2026-11-03
- **关联**：task-104/109、DR-019 §1/§2、AC-26、05 §5.16.3、DR-013/014、REV-108/116

## 1. 结论

**结论：通过**（首轮有条件通过 → C1 由 `14fc449` 关闭；见 §7）

P0=0；**P1=0**；P2 残余（不阻断，见 §7）；P3 后续。jail 写授权/只读 `--allow-path`/重叠写优先/系统目录拒绝/create_parents、profile `file-write*`/只读保留/系统拒绝、CLI 解析/依赖/预检 exit3 不建 DB、工具写入矩阵/reexec/cargo 缓存均通过；最终 harness **363/0/1** 默认与 features、workspace **1247/0/3**、cli **257/0**、pytest **183**、clippy/fmt/check/no-default 全绿。

> **首轮历史（保留）**：`0415531` 首轮为有条件通过——C1 无 `--allow-write-path` 时模板注释/空块改变默认 profile 文本/sha。`14fc449` 已关闭 C1（见 §7）。

首轮条件：

- **C1 无 `--allow-write-path` 时 profile 不再逐字节不变**：`PROFILE_TEMPLATE` 在只读白名单注释处把 `;;;` 改为 `;`，并新增「可写白名单」注释块 + `@WRITE_PATHS@` 行；即使 `write_paths=[]`（`new_with_options`/旧 `new` 路径），`fill_template` 仍输出该注释/空行。结果默认 bash profile 文本与 sha256 均变化，违反任务「无 flag 默认 profile/行为逐字节不变」，并会改变既有 `bash` trace 的 `sandbox_profile_sha256`。

## 2. 四维评审

| 维度 | 结论 | 依据 |
|---|---|---|
| 功能正确性 | ✅ | 写授权/profile/CLI/构建组合成立；C1 默认 profile 字节由 `14fc449` 关闭并加 golden |
| 结构合理性 | ✅ | jail 新增 write paths；profile 模板参数化；CLI additive；无新 Cargo |
| 逻辑性 | ✅（P2） | 写读矩阵、重叠优先、系统拒绝、预检不建 DB、create_parents 符合 DR-019 |
| 可读性 | ✅（P3） | 注释/错误闭集/构建提示清楚；P3 测试/文档补齐 |

## 3. 发现清单

### P1 · 合并前必须关闭

| # | 位置 | 问题 | 条件 |
|---|---|---|---|
| C1 | `builtin/sandbox.rs::PROFILE_TEMPLATE` / `fill_template` | 模板永久新增 `;;; 可写白名单` + `@WRITE_PATHS@` 行，并把 `;;; ---------- 只读白名单` 改为 `; ----------`；`write_paths=[]` 时仍输出注释/空行，默认 profile 文本/sha 改变。无 flag 默认 profile/行为须逐字节不变 | 仅在 `write_paths` 非空时插入可写块（或渲染后按条件拼接），保留 v0.4 既有注释字节；补「无 write_path 的 render_profile 输出/ sha 与 v0.4 golden 完全一致」测试，以及 default `MacOsSandbox`/trace profile sha 不变断言。修后复跑 |

### P2 · 建议

| # | 位置 | 问题 | 建议 |
|---|---|---|---|
| N1 | `FORBIDDEN_WRITE_ROOTS` | 当前为 `/` + `/usr`/`/bin`/`/sbin`/`/System`/`/Library`/`/private/etc`（7 类口径）；`/private/var`、`/dev`、`/Applications` 等未拒绝（Lead 预裁决 D6 评估，不阻塞） | D6 评估集合扩充；补 `/private/var` 等负例 |
| N2 | jail 测试矩阵 | 未覆盖 `workspace` 嵌套在 `--allow-write-path` 内（DR 允许但写授权按各自根）、`--allow-write-path` 内 symlink 出根/`..`、`--allow-bash` only + write path 组合 | 补 3 个负例/组合；复用 jail 既有 symlink/`..` 构造 |
| N3 | CLI | `--allow-write-path` 仅测 builtin opt-in；未测 `--allow-bash` only、重复/嵌套去重、`--allow-path` 同目录写优先的 CLI 级 | 补 CLI 组合；现有 jail 单测覆盖同目录优先 |
| N4 | profile | 无 default profile golden；`renders_write_paths...` 只比较「有/无 write」新模板，不证明默认字节未变 | 与 C1 一并补 v0.4 默认 profile golden/sha |
| N5 | 构建工具链 | 仅 profile/文件工具缓存模拟，无真实 cargo/cc 执行（DR-019 §2 明确默认不真实执行）；`cc -isysroot` 仅在 usage 文档 | D6/普通终端验证；文档已提示 |

### P3 · 测试/文档

| # | 位置 | 问题 | 建议 |
|---|---|---|---|
| N6 | `files` | apply_patch 写 allow-write-path 使用绝对路径 + strip=0，D6 UX 文档（Lead 预裁决） | D6 |
| N7 | `bash` | AC-26 d 真 bash 隔离/构建命令仍待普通终端 `verify-sandbox.sh`（Lead 接受） | D6 |
| N8 | 稳定性 | workspace/cli 首次命中既有 `ac09_parallel`/`ac07_watchdog` timing flake；隔离与重跑通过；本分支无 kernel/watchdog 改动 | D6 登记既有 flake |
| N9 | profile sha | 有 write path 时 sha 变化是预期；需在 trace/文档说明**仅**此场景变化 | D6 文档 |

## 4. 验证证据

worktree `/Users/zyzs/lczyzs/code/zgraph/.worktrees/harness-dsl-dev`，HEAD `0415531`，base `d3da745`，工作区干净；`CARGO_HOME=/Users/zyzs/lczyzs/code/zgraph/.cargo-home`，未设 `CARGO_TARGET_DIR`：

```bash
cargo test -p zgraph-harness                 # 362 tests：361 passed / 0 failed / 1 ignored
cargo test -p zgraph-harness --features mcp  # 362 tests：361 passed / 0 failed / 1 ignored
cargo test --workspace                       # 第 1 次 ac07/ac09 timing flake；第 2 次 1245/0/3；第 3 次 1245/0/3
cargo test -p zgraph-cli                     # 首次 ac07_watchdog timing flake；全量重跑 257/0
pytest -q                                    # exit 0；183 passed
cargo clippy --workspace --all-targets --locked -- -D warnings  # clean
cargo fmt --all -- --check                   # clean
cargo check -p zgraph-harness --no-default-features  # clean
bash scripts/dev/check.sh                    # PASSED
```

- **与自报一致**：harness 361/0/1（默认/features）、workspace 1245/0/3、cli/pytest/clippy/fmt/check；5 files（CLI 机械接线 + harness），无 Cargo 新增；`git status` 空。
- **已核**：jail workspace+write 可写/allow-path 只读/重叠写优先/嵌套幂等/系统目录与 `/` 拒绝/不存在与非目录 `allow_write_path_invalid`/create_parents 写范围/越界 path_escape；profile `file-read* file-write*`、allow-path 仍只读、系统拒绝、build toolchain exec/SDK/cache/network 组合；CLI 解析/依赖/exit3 不建 DB/`new_with_options` 旧语义；files write/edit/apply_patch/reexec read/cargo cache；默认无 write path 仍拒绝写外部。
- **未覆盖/风险**：C1 默认 profile 字节；P2 N1–N5 补强。

## 5. Lead 预裁决确认

- 系统目录集合当前 7 类：**接受，D6 评估扩充**。
- `--allow-write-path` 必须伴随 builtin/bash：**已实现/已测 parse**，接受。
- apply_patch 绝对路径/strip UX：**接受，D6 文档**。
- AC-26 d 真 bash 隔离待普通终端：**接受，D6/verify-sandbox.sh 证据**。
- 无 Cargo 新增；跨模块 cli 接线 Lead 已批准，第二评审 Lead 追加。

## 6. 遗留问题

- **C1**：默认 profile 逐字节/sha 维持 + 测试；关闭后复审转「通过」。
- **P2 N1–N5**：系统目录扩充、workspace 嵌套/symlink/`..`、CLI 组合、default profile golden、真实构建工具链 D6。
- **P3 N6–N9**：apply_patch UX、真 bash 隔离、既有 timing flake、profile sha 文档。
- 无 P0；C1 未关闭前不可 merge。

## 7. 条件关闭复核（2026-11-03，`14fc449`，parent `2189ec7`）

**复核结论：通过**。`14fc449` 仅 `builtin/sandbox.rs` +125/−6；工作区干净；无 Cargo/其它模块。

### C1（默认 profile 逐字节）
- `PROFILE_TEMPLATE` 恢复 v0.4 冻结字节：`;;; ---------- 只读白名单`、无 `@WRITE_PATHS@` 常驻块/空行；与 `d3da745` 旧模板人工逐字对比一致。
- 可写块改为**条件拼接**：仅 `write_lines` 非空时在 allow 行后追加 `;;; 可写白名单` + `(allow file-read* file-write* ...)`；空 write_paths 时 `@ALLOW_PATHS@` 仅替换原 allow 段。
- 测试：
  - `empty_write_paths_profile_matches_v04_golden_sha`：`V04_PROFILE_GOLDEN_SHA=a6ea5e…` 硬编码校验；无 write 块/`@WRITE_PATHS@`；真实路径 render 与 v0.4 参考逐字节相等；重复渲染 sha 稳定。
  - `write_paths_add_block_once_and_change_profile_sha`：非空时 sha 变化、可写块恰 1 次、`;;;` 注释保留、先只读后可写顺序。
- **C1 关闭**；默认 trace `sandbox_profile_sha256` 不漂移（render 层；真沙箱留 F6/普通终端）。

### 残余 P2（不阻断）
- 系统目录集合 7 类（`/private/var` 等未拒）D6 评估。
- jail 测试仍缺 workspace 嵌套在 write-path 内、write-path 内 symlink/`..`、`--allow-bash` only 组合（DR 允许；D6 补）。
- golden 辅助使用当前 `fill_template` + 硬编码 v0.4 SHA，且只覆盖单配置；建议 F6 增加真实 `MacOsSandbox`/trace sha 快照（嵌套环境不可执行，已接受）。
- 构建工具链仍为 profile/文件工具缓存模拟；真实 cargo/cc 执行留 D6/普通终端。

### 最终门禁（`14fc449`）
```bash
cargo test -p zgraph-harness                 # 364 tests：363 passed / 0 failed / 1 ignored
cargo test -p zgraph-harness --features mcp  # 364 tests：363 passed / 0 failed / 1 ignored
cargo test --workspace                       # 90 targets：1247 passed / 0 failed / 3 ignored
cargo test -p zgraph-cli                     # 257 passed / 0 failed
pytest -q                                    # exit 0；183 passed
cargo clippy --workspace --all-targets --locked -- -D warnings  # clean
cargo fmt --all -- --check                   # clean
cargo check -p zgraph-harness --no-default-features  # clean
bash scripts/dev/check.sh                    # PASSED
```
- 仅 1 file；无 Cargo；默认路径无回归；`git status` 空。
- 稳定性：本轮 workspace/cli 均全绿；上一轮首次 `ac07_watchdog`/`ac09_parallel` timing flake 未复现（既有 flake，D6 登记）。
