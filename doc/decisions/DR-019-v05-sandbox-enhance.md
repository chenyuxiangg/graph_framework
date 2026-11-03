# DR-019 · v0.5 sandbox 增强（可写白名单 + 构建工具链）

> **状态**：V1 设计冻结（REV-115；**Lead 裁决 2026-11-03：与 workspace 相同/嵌套允许、不可 `/`、同目录写优先、独立于只读 allow-path**）。
> **日期**：2026-11-03 · **作者**：docs-dev · **关联**：DR-013（v0.4 jail/OS 沙箱）、DR-012、05 §5.15、07 AC-23/26、task-95
> **合同**：`--allow-write-path` 独立可写白名单；构建工具链可用性；默认（无新开关）行为不变。

## 0. 摘要

| 项 | 冻结 |
|---|---|
| `--allow-write-path` | **独立、目录级、可重复**的可写白名单（与只读 `--allow-path` 分开）；canonicalize；profile `file-write*` + jail 写授权；默认空 |
| 只读 `--allow-path` | 保持只读（v0.4 口径）；不与写白名单合并 |
| 写目标 | workspace + `--allow-write-path` 目录（不递归到父/兄弟）；**不可 `/`** |
| 构建工具链 | `--allow-exec-path`（已有）扩展 process-exec + file-read；cargo/node/uv 需显式路径/环境（HOME/TMPDIR/缓存）；`cc` 需 `-isysroot`（文档） |
| 默认 | 无新开关：写目标仅 workspace，profile/行为与 v0.4 一致；无性能影响 |
| AC-26 | a–e |

## 1. `--allow-write-path`

### 1.1 语义

- CLI：`--allow-write-path <dir>`（可重复；目录级；canonicalize；必须存在且为目录）。
- 授权：**读+写**（写通常需读；profile `file-read* file-write*`；jail 写校验放行）；不授予 process-exec（构建工具见 §2）。
- 与 workspace 关系：
  - 相同目录/嵌套在 workspace 内 → **允许（幂等）**；
  - workspace 嵌套在 allow-write-path 内 → 允许，但写授权仍按各自根；
  - 与只读 `--allow-path` 同一目录 → **写优先**（显式可写），文档警示；
  - **不可 `/`**（配置错误 `allow_write_path_invalid`）；**系统目录（`/`、`/usr`、`/System`、`/Library` 等）→ 拒绝（`allow_write_path_invalid`）**，不做 warning 放行。
- 安全：可写白名单是**用户显式信任**；文档警示（等于第二个 workspace）；profile + jail 双强制；bash 侧 `file-write*` 同步（若允许 bash）。

### 1.2 profile/jail

- macOS profile：`(allow file-read* file-write* (subpath @WRITE_PATH_i@))`；渲染前 realpath/防注入（同 v0.4）。
- Rust jail：写操作 `resolve(..., Write)` 允许 roots = workspace + write_paths；仍拒绝 `..`/symlink 出根；hardlink 残余风险文档化。
- 预检：非法/`/`/不存在 → `HARNESS_CONFIG_LOAD{reason:"allow_write_path_invalid"}`；CLI run 启动前 exit 3、不产生 run（沿用 v0.4 预检）。
- 与 `--allow-path`/`--allow-bash`/`--allow-network` 组合：独立；未开 `--allow-builtin-tools` 时无意义（配置可忽略/报错）。

## 2. 构建工具链可用性

- **不改变**默认系统只读集合（v0.4 spike profile）。
- 用户要跑 cargo/node/uv：显式
  - `--allow-exec-path <toolchain bin 目录>`（process-exec + file-read）；
  - `--allow-path <sdk/cache 只读目录>`（如 rustup toolchains）；
  - `--allow-write-path <缓存目录>`（如 `target/`、`CARGO_HOME`、`node_modules`）或把 `CARGO_HOME`/`CARGO_TARGET_DIR`/`HOME` 指到 workspace；
  - env：`HOME=<workspace>`、`TMPDIR=<workspace>/.tmp`（已有）；`CARGO_HOME`/`RUSTUP_HOME` 可在 workspace 下（文档）。
- `cc`：链接需 `-isysroot $(xcrun --show-sdk-path)`；文档示例。
- **不自动**把 `~/.cargo`/`/usr/local` 加入可写/可执行（安全）；不做“默认可用”。
- 若未提供路径：工具不可用（bash 内 `command not found`/EPERM）属预期，文档说明。
- 裁决项：是否新增 `--allow-toolchain` 聚合开关（推荐**否**，保持显式路径）。

## 3. 与 v0.4 兼容

- 无新开关：profile/写规则/工具行为与 v0.4 完全一致；默认路径逐字节不变。
- 新开关仅 opt-in；错误码/reason append-only；trace 不 v8。
- bash 的 `sandbox_profile_sha256` 包含 write_paths/network/exec 摘要（profile 哈希变化，属预期）。

## 4. AC-26（设计冻结；F2 实测）

| # | 断言 |
|---|---|
| a | `--allow-write-path` 目录级可写（write/edit/apply_patch）；workspace 行为不变 |
| b | 只读 `--allow-path` 仍不可写；同目录写优先级明确 |
| c | 越界（父/兄弟/`/`/symlink 出根）拒绝；预检 exit 3 不产生 run |
| d | bash：profile 写规则含 write_paths；未授权路径 EPERM；`--allow-exec-path` 组合可执行 |
| e | 默认无新开关：写目标仅 workspace；行为/trace 与 v0.4 一致 |

## 5. Lead 裁决项

| # | 事项 | 推荐 |
|---|---|---|
| 1 | 与 workspace 重叠/嵌套 | **已裁决：相同/嵌套允许（幂等）；不可 `/`；系统目录拒绝/警示；负例 `/`/不存在/symlink 出根** |
| 2 | 与只读 allow-path 同目录 | **已裁决：写优先（显式可写）+ 文档警示；独立于只读 allow-path** |
| 3 | 构建工具聚合开关 | **已裁决：不新增**；显式 `--allow-exec-path`+`--allow-path`+`--allow-write-path` |
| 4 | 默认系统集合 | **不变**（spike profile 冻结） |
| 5 | 预检错误面 | `HARNESS_CONFIG_LOAD{reason:"allow_write_path_invalid"}`；exit 3 |
