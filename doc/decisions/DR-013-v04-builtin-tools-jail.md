# DR-013 · v0.4 内置工具与工作区 jail（8 工具 + OS 沙箱）

> **状态**：V1 设计冻结（REV-105 已评审；macOS sandbox-exec profile 按 spike 附录 A 冻结）。
> **日期**：2026-11-02 · **作者**：docs-dev · **关联**：DR-012、DR-014、05 §5.6/§5.14、06 §6.2、07 AC-21/22/23、task-74
> **安全合同（用户加严）**：所有文件操作**只能在 `--workspace-dir` 内**，除非显式 `--allow-path` 白名单；**bash 必须 OS 沙箱**；**无沙箱 fallback**。

## 0. 摘要

| 项 | 冻结 |
|---|---|
| 工具集 | 8 个：`builtin__read`/`write`/`edit`/`apply_patch`/`diff`/`grep`/`glob`/`bash` |
| 命名 | `builtin__<tool>`（双下划线；与 `mcp__<server>__<tool>` 并列，不出现在现有 `^[a-z][a-z0-9_]{0,63}$` 冲突范围外） |
| opt-in | `--allow-builtin-tools`（文件/搜索）；`--allow-bash`（bash 额外）；缺省关 |
| jail | `--workspace-dir <dir>`（缺省 `.`，canonicalize）；`--allow-path <dir>` 可重复；词法 + canonicalize + symlink 逃逸拒绝 |
| bash | macOS `sandbox-exec` profile：workspace 读写、**allow-path 只读**、系统只读、`TMPDIR=<workspace>/.tmp`；网络默认 deny；`--allow-network` 全开；沙箱不可用 → **fail-closed** |
| 安全模型 | 文件工具 **Rust 强制** jail；bash **OS 沙箱强制**；不做应用层黑名单/无沙箱 fallback |
| 错误 | **零新码**：工具失败 `TOOL_ERROR{reason}`；opt-in/配置/启动前沙箱探测失败 `HARNESS_CONFIG_LOAD{reason}`；运行期沙箱失败 `TOOL_ERROR{reason:"sandbox_unavailable"}`；无新 kind/码（§6） |
| trace | 每次工具调用写 `tool_calls`（v7，DR-014）；默认 recorded 回放 |

## 1. 工具源与优先级

工具源三类并列（05 §5.6 既有 + v0.3 MCP + v0.4 builtin）：

1. **builtin**：`builtin__read` 等 8 个，Rust 实现，仅 `--allow-builtin-tools` 时注册。
2. **MCP**：`mcp__<server>__<tool>`（v0.3，DR-010）。
3. **Python**：既有 `tools/`（05 §5.6.2）。

- `AgentConfig.tools` 仍是**唯一白名单**：名称须在已注册集合中；未列不可见/不可调用（`HARNESS_UNKNOWN_TOOL` 运行期）。
- **冲突**：同一 LLM 工具名重复（跨源）→ `HARNESS_CONFIG_LOAD{reason:"tool_name_conflict", tool}`；builtin 名固定，MCP/Python 不得占用 `builtin__*`（注册期拒绝）。
- **优先级**：无覆盖语义；同名即冲突（fail-closed）。`--allow-builtin-tools` 只注册文件/搜索；`--allow-bash` 额外注册 bash；未开启时请求 builtin 名 → 配置错误（加载期）。
- 工具描述/参数 schema 内置固定（可审计）；`tools` 顺序 = `AgentConfig.tools`（请求字节可复现）。

### 1.1 冲突检测与白名单（Lead 裁决 REV-105）

- **无优先级/无覆盖**：三类工具源（builtin / mcp / python）**不按优先级覆盖**；同一 LLM 工具名出现 >1 次 → `HARNESS_CONFIG_LOAD{reason:"tool_name_conflict", tool, sources}`（fail-closed，加载期）。
- **命名空间保留**：`builtin__` 前缀由 Rust 内置独占；MCP/Python 工具名不得以 `builtin__` 开头（注册期拒绝，reason `reserved_prefix`）。`mcp__` 同理由 MCP 占用（v0.3 已有）。
- **`AgentConfig.tools` 白名单对所有源一致**：无论 builtin/mcp/python，只有列在 `tools` 中的工具才可见/可调用；未列 → 既有 `HARNESS_CONFIG_LOAD{reason:"tool_not_registered"}`（加载期）/`HARNESS_UNKNOWN_TOOL`（运行期幻觉）；`tools` 顺序决定 schema 顺序（字节可复现）。
- **builtin 名固定集**：8 个名字不可被覆盖/重命名；`--allow-builtin-tools`/`--allow-bash` 只决定是否**注册**，不改变白名单语义。
- **workspace 缺省**：`--workspace-dir` 缺省 `.`（canonicalize）；未 opt-in 时不解析/不触碰 workspace。

## 2. 工作区 jail（文件工具）

### 2.1 配置

| CLI/env | 语义 |
|---|---|
| `--workspace-dir <dir>` | 工作区根；缺省 `.`；必须存在且为目录；**canonicalize**；写入 `workspace_root` |
| `--allow-path <dir>`（可重复） | 额外允许的**只读目录**（canonicalize；**file-read\* only**）；非目录/不存在 → 配置错误；写目标只能 workspace；不做文件级/通配 |
| env `ZGRAPH_WORKSPACE_DIR` / `ZGRAPH_ALLOW_PATHS`（`:` 分隔） | 与 CLI 等价（CLI 优先）；v0.4 至少 CLI |

- **路径解析顺序**：工具参数 path（相对/绝对）→ 词法归一化（拒绝 `..` 逃逸）→ `canonicalize`（解析 symlink）→ 与 `workspace_root`/`allow_paths` 的**真实路径前缀**比较（按路径组件边界，非字符串前缀）。
- **越界**：`TOOL_ERROR{reason:"path_escape", path, root}`（喂回 LLM）；不执行任何读写。
- **symlink**：文件/中间目录 symlink 指向 jail 外 → 拒绝；指向 jail 内 → 允许（按真实路径判定）。
- **特殊文件**：device/FIFO/socket → `TOOL_ERROR{reason:"not_regular_file"}`；只读普通文件。
- **TOCTOU/hardlink**：打开时尽量 `O_NOFOLLOW`（文件工具）+ 目录 fd/真实路径；**hardlink 到 jail 外 inode 的残余风险接受并文档化**（无法可靠识别，ISSUE-167）；`/System/Volumes/Data` firmlink 依赖 canonicalize 解析别名（spike 未复现则文档注明）；负例只承诺 symlink/`..`/绝对路径（bash 侧由 Seatbelt 覆盖）。
- 无 workspace 参数时也不得默认 `/`：缺省 `.`（当前目录 canonicalize）；`--allow-builtin-tools` 未开时不解析 workspace。

### 2.2 风险说明

- 文件工具是**进程内 Rust**（不是 Python worker），jail 由 Rust 强制；不依赖 OS 沙箱。
- `--allow-path` 是**用户显式信任的只读目录**（file-read* only）；写目标仅 workspace；写 allow-path 目标 → `TOOL_ERROR{reason:"path_read_only"}`；不提供文件级/通配；外部可写请把 `--workspace-dir` 指向该目录（v0.4 不做 `--allow-write-path`，登记 ISSUE）。
- 不防「用户自己把 workspace 指向 `/`」——显式配置即信任，文档警示。

## 3. 8 个工具语义（冻结）

### 3.1 `builtin__read`

```json
{"type":"object","properties":{
  "path":{"type":"string","description":"相对 workspace 或 allow-path 的路径；绝对路径须在 jail 内"},
  "offset":{"type":"integer","minimum":0},
  "limit":{"type":"integer","minimum":1,"maximum":10000,"default":2000,"description":"行数上限 BUILTIN_READ_MAX_LINES"}
},"required":["path"],"additionalProperties":false}
```

- 读 UTF-8 文本；`offset` 行偏移（0 基），`limit` 行数（缺省 2000，≤10000）；二进制（前 8 KiB NUL）→ `TOOL_ERROR{reason:"binary_file"}`；非 UTF-8 → `invalid_utf8`（不做 base64）。
- 结果：`{content, lines, total_lines?, truncated?}`；文件/扫描 ≤1 MiB、行 ≤10000（默认 2000）；**返回内容 ≤32 KiB（truncated 标记，见 §8）**。
- 目录/不存在/越界 → `TOOL_ERROR{reason:"is_directory"|"not_found"|"path_escape"}`。

### 3.2 `builtin__write`

```json
{"type":"object","properties":{
  "path":{"type":"string"},
  "content":{"type":"string"},
  "overwrite":{"type":"boolean","default":false},
  "create_parents":{"type":"boolean","default":false}
},"required":["path","content"],"additionalProperties":false}
```

- **原子**：同目录临时文件 + fsync + rename；失败不改原文件；**父目录不自动创建**（`create_parents=true` 才建）；**覆盖已存在文件需 `overwrite=true`**（否则 `E_EXISTS` 附 size/mtime/sha256）；只写 UTF-8。
- 覆盖写；结果 `{path, bytes_written, sha256_before?, sha256_after}`；输入 ≤1 MiB（超限 `too_large`）；结果 ≤32 KiB；写 allow-path 目标 → `TOOL_ERROR{reason:"path_read_only"}`。
- 副作用记录：v7 `tool_calls` 记 path + before/after sha256 + bytes（DR-014）。

### 3.3 `builtin__edit`

```json
{"type":"object","properties":{
  "path":{"type":"string"},
  "old_string":{"type":"string","minLength":1},
  "new_string":{"type":"string"},
  "replace_all":{"type":"boolean","default":false}
},"required":["path","old_string","new_string"],"additionalProperties":false}
```

- `old_string` 必须在文件中**唯一匹配**（`replace_all=false`）；0 或多匹配 → `TOOL_ERROR{reason:"no_match"|"multiple_matches", count}`；失败**不改文件**。
- `replace_all=true`：全替换（至少 1 处）；结果 `{path, replacements, sha256_before, sha256_after}`。
- 原子写（同 write）；保留文件权限（不新建时）；输入 ≤1 MiB；写 allow-path 目标 → `path_read_only`；结果 ≤32 KiB。

### 3.4 `builtin__apply_patch`

```json
{"type":"object","properties":{
  "patch":{"type":"string","description":"unified diff（git 风格）；支持多文件"},
  "strip":{"type":"integer","minimum":0,"default":1}
},"required":["patch"],"additionalProperties":false}
```

- 支持 `---/+++`、`@@`、多文件（含 `\ No newline at end of file`；rename/delete 按 v0.4 范围）；**两阶段严格原子**：①全部在内存解析/校验（含 CAS）→ ②各自 tmp → ③依次 rename；任一失败清理 tmp、**不落盘任何文件**（窗口极小但不宣称 rename 级原子）。
- 只允许 workspace/allow-path；`a/`/`b/` 前缀按 `strip` 去；不支持二进制/rename/copy（→ `TOOL_ERROR{reason:"unsupported_patch"}`）。
- 结果：`{files:[{path, added, removed, sha256_before, sha256_after}], total_added, total_removed}`；patch ≤1 MiB；结果 ≤32 KiB；写 allow-path 目标 → `path_read_only`。

### 3.5 `builtin__diff`

```json
{"type":"object","properties":{
  "path":{"type":"string"},
  "old_string":{"type":"string"},
  "new_string":{"type":"string"},
  "context_lines":{"type":"integer","minimum":0,"maximum":20,"default":3}
},"required":["path","old_string","new_string"],"additionalProperties":false}
```

- 生成「把 old 替换为 new」的 unified diff（不写文件）；供模型预览/确认；越界同 read。
- 结果 `{diff, changed}`；空 diff → `changed:false`。

### 3.6 `builtin__grep`

```json
{"type":"object","properties":{
  "pattern":{"type":"string","minLength":1},
  "path":{"type":"string","default":"."},
  "glob":{"type":"string","description":"可选 glob 过滤（如 **/*.rs）"},
  "fixed_strings":{"type":"boolean","default":false},
  "case_sensitive":{"type":"boolean","default":true},
  "max_results":{"type":"integer","minimum":1,"maximum":1000,"default":100}
},"required":["pattern"],"additionalProperties":false}
```

- 遍历：**尊重 `.gitignore`**（推荐 `ignore` crate；`--no-ignore` 不做）/隐藏文件默认跳过（推荐）；二进制跳过（NUL 探测）；扫描单文件 ≤2 MiB、结果 **≤1000 条且 ≤32 KiB**（truncated）；路径 jail 同 read；`--allow-path` 只读。
- 结果 `{matches:[{path,line,column,text}], total, truncated}`；排序确定（path, line）。
- regex 由 `regex` crate（线性时间；拒绝回溯爆炸）；`fixed_strings` 用字面查找。

### 3.7 `builtin__glob`

```json
{"type":"object","properties":{
  "pattern":{"type":"string","minLength":1},
  "path":{"type":"string","default":"."},
  "include_hidden":{"type":"boolean","default":false},
  "max_results":{"type":"integer","minimum":1,"maximum":5000,"default":1000}
},"required":["pattern"],"additionalProperties":false}
```

- `globset` 匹配；不递归出 jail；结果按路径排序（确定性）；默认 1000/上限 5000、**结果 ≤32 KiB**（truncated）；越界拒绝。
- 结果 `{paths:[...], total, truncated}`；目录是否返回（推荐仅文件）。

### 3.8 `builtin__bash`

```json
{"type":"object","properties":{
  "command":{"type":"string","minLength":1,"maxLength":8192},
  "cwd":{"type":"string","default":"."},
  "timeout_ms":{"type":"integer","minimum":100,"maximum":600000,"default":120000},
  "env":{"type":"object","additionalProperties":{"type":"string"},"description":"可选，仅白名单追加"}
},"required":["command"],"additionalProperties":false}
```

- **必须**在 OS 沙箱内执行（§5）；cwd 必须在 jail 内（canonicalize）；`TMPDIR=<workspace>/.tmp`（创建、gitignore）。
- 超时/取消 → kill **进程组**；stdout/stderr 各 cap 32 KiB（截断标记）；结果 `{exit_code, stdout, stderr, truncated, sandbox:"macos-sandbox-exec", duration_ms}`。
- 非 0 退出是**正常工具结果**（喂回 LLM，不 fail run）；沙箱启动失败/不可用 → fail-closed（§5.3）。
- 网络默认 deny；`--allow-network` 显式开启（全局，v0.4 不做域名白名单）。
- 不提供 `shell` 选项（固定 `/bin/bash --noprofile --norc -c`）；env 仅白名单（HOME/TMPDIR/PATH/LANG/LC_ALL/SHELL/USER/LOGNAME/DEVELOPER_DIR/SDKROOT）+ 显式 `env`；不写 token。

## 4. opt-in 与配置

| 开关 | 注册 | 缺省 |
|---|---|---|
| `--allow-builtin-tools` | 文件/搜索 7 个 | 关 |
| `--allow-bash` | bash（**隐含**需要 OS 沙箱可用；不隐含文件工具） | 关 |
| `--workspace-dir` | jail 根 | `.` |
| `--allow-path` | 额外**只读**白名单目录（canonicalize；**只读**，不给写） | 空 |
| `--allow-exec-path` | 额外**可执行**目录（opt-in；只读 allow-path ≠ 可执行） | 空 |
| `--allow-network` | bash 网络 | 关（deny） |

- 未开启 `--allow-builtin-tools` 时，`AgentConfig.tools` 出现 `builtin__*` → `HARNESS_CONFIG_LOAD{reason:"builtin_tools_not_allowed"}`。
- 开启文件工具但未开 bash：bash 不出现在注册表；引用 → 同上/`tool_not_registered`。
- `--allow-bash` 但 OS 沙箱启动前探测失败 → **加载期** `HARNESS_CONFIG_LOAD{reason:"sandbox_unavailable"}`（Crashed(Config)），不注册 bash；嵌套 → `sandbox_nested`；profile 非法 → `sandbox_profile_invalid`（均零新码）；CLI run 预检失败 → **exit 3、不产生 run**。
- 运行时沙箱故障（探测后异常）→ 该次 bash 调用 `TOOL_ERROR{reason:"sandbox_failure"}`，不降级执行。

## 5. OS 沙箱（bash，macOS Seatbelt）

> **spike 结论（2026-11-02）**：macOS 26.6 实测 `sandbox-exec` 可用（DEPRECATED 但未移除）；附录 A profile **31/31 断言通过**；profile 解析+apply ≈**11 ms**，每条 bash 端到端 **+22–40 ms**（中位 ≈25 ms）。**Linux/Windows 不支持（fail-closed，文档化）**。

### 5.1 profile（按 spike 附录 A 冻结，79 行）

- `(version 1)` + `(deny default)` + `(debug deny)`；**路径必须 `/private/...`**（`/tmp`/`/etc`/`/var` 是 symlink；workspace/allowlist 渲染前 `realpath`）。
- **进程**：`process-fork`/`signal(target self)`/`process-info*(target self)`；**`process-exec` 按目录收窄**（默认仅 `/bin`、`/sbin`、`/usr/bin`、`/usr/sbin`、`/usr/lib`、`/System`、`/Library/Apple`、`/Library/Developer/CommandLineTools`、`@WORKSPACE@`）——防 `~/.cargo/bin/cargo`、`/usr/local/bin/node` 执行。
- **系统只读**：`file-read* file-test-existence` on `/`、`/System`、`/usr/lib`、`/usr/share`、`/usr/bin`、`/usr/sbin`、`/bin`、`/sbin`、`/Library/Apple`、`/Library/Developer/CommandLineTools`、`/private/var/db/{timezone,dyld,DarwinDirectory}`、`/private/etc/localtime`、**`/private/var/select/sh`**（缺它每条命令 stderr 多一行错误）；`file-read-metadata` on `/usr`/`/Library`/`/System`/`/bin`/`/sbin`/`/private` + `/etc`/`/tmp`/`/var` stat。
- **设备**：`/dev/null`、`/dev/zero`、`/dev/random`、`/dev/urandom`、`/dev/fd`。
- **mach-lookup**：cfprefsd(agent/daemon)、system.logger、notification_center、opendirectoryd(libinfo/membership)、trustd(agent)、logd(events)、runningboard、secinitd、bsd.dirhelper、analyticsd、cfprefsd local；ipc-posix-shm（notification_center/apple.cfprefs.*）。
- **workspace**：`file-read* file-write* file-test-existence (subpath @WORKSPACE@)`。
- **`--allow-path`**：仅 **`file-read* file-test-existence`**（**只读**，不给写）；目录级。
- **`--allow-exec-path`**（新增，可重复，opt-in）：额外 `process-exec (subpath ...)`；**只读 allow-path ≠ 可执行**。
- **网络**：默认 `(deny network*)`；`--allow-network` 追加 `(allow network*)` + `/private/etc/ssl`、hosts/resolv.conf 只读 + networkd/networkscored mach（见附录 A）。
- **渲染**：Rust 侧 `realpath` + 必须 `/` 开头、不含 `"`/换行（否则 `E_BAD_ALLOW_PATH` 防注入）；渲染后 `sha256` 写 `tool_calls.sandbox_profile_sha256`。

### 5.2 执行形式（冻结）

- **固定 `/bin/bash --noprofile --norc -c <command>`**（禁 `-lc`/登录 shell，避免读 workspace 恶意 `.profile`；bash 路径固定，不依赖 PATH）。
- `cwd` 必须是 jail 内路径（参数为相对 workspace，Rust canon 后 `current_dir`）；`HOME=<workspace>`、`TMPDIR=<workspace>/.tmp`、`PATH` 固定系统+CLT 目录。
- **env 白名单重建**（`env_clear()` 后逐项设置）：`HOME`/`TMPDIR`/`PATH`/`LANG`/`LC_ALL`/`SHELL`/`USER`/`LOGNAME`/`DEVELOPER_DIR`/`SDKROOT`；**剔除** `BASH_ENV`/`ENV`/`LD_*`/`DYLD_*`/`PYTHONPATH`/`PYTHONSTARTUP`/`NODE_OPTIONS`/`GIT_CONFIG_*`/`SSH_AUTH_SOCK`/`HTTP(S)_PROXY`/`PWD`/`OLDPWD`。
- **stdin=null**（`Stdio::null()`；避免挂死等输入）。
- **进程组**：`setsid()`/`setpgid(0,0)`；超时/取消 `killpg(SIGTERM)` → 等 **2s** → `SIGKILL`；结束时清理残留 `killpg` 并记 `killed_orphans`。
- **输出**：边读边 drain（**drain 上限 2 MiB**；不能停读否则 pipe 阻塞）；**给 LLM/记录各 16 KiB head+tail（truncated 标记）**；stdout/stderr 分流；超时默认 120s（可配）。
- **退出**：正常码原样；信号 → `exit_code=null, signal=N`；超时 `status="timeout"`；非 0 为正常工具结果。

### 5.3 失败/嵌套（fail-closed，零新码）

| 场景 | 结果 |
|---|---|
| `sandbox-exec` 不存在/不可执行 | 配置/启动前 → `HARNESS_CONFIG_LOAD{reason:"sandbox_unavailable"}`（Crashed(Config)，bash 不注册） |
| **嵌套 Seatbelt**（zgraph 运行在 App Sandbox/DSH/IDE/CI wrapper 内）：`sandbox-exec` exit **71** | **硬失败**：启动探测 → `HARNESS_CONFIG_LOAD{reason:"sandbox_nested"}`；CLI **run 启动前预检**失败 → **exit 3、不产生 run**（沿用 `--allow-mcp` 预检口径）；运行期 → `TOOL_ERROR{reason:"sandbox_failure"}` |
| profile 编译失败（exit **65**） | `HARNESS_CONFIG_LOAD{reason:"sandbox_profile_invalid"}`（附 stderr 前 4 KiB）；运行期 `TOOL_ERROR{reason:"sandbox_failure"}` |
| 沙箱内命令越界/联网 | 命令自身 EPERM（exit≠0）作为工具结果；**不降级、不无沙箱执行** |
| 未 opt-in | 不注册；引用 → 配置错误 |
| **`--unsafe-bash`** | **不提供**（覆盖开关会被 CI 顺手打开） |

> 探测顺序：二进制存在/可执行 → `sandbox-exec -p '(version 1)(deny default)' /usr/bin/true` 在**本进程上下文**成功（检出嵌套）；失败按 exit 71/65 分类 reason；profile `-D` 语法 lint 可用于 CI。

### 5.4 平台/性能/构建工具

- **macOS 26.6 实测**；Linux **不支持**（bwrap/landlock 仅文档，不实现/不承诺）；Windows 不承诺。
- `sandbox-exec` 自 10.13.6 **DEPRECATED**（Apple 不承诺保留）；备选 `sandbox_init` 私有 API 仅文档，不实现；profile 版本化 + 启动 smoke test（`sh -c 'echo ok'`、`python3 -c pass`、`git --version`）失败 fail-closed。
- **性能**：profile parse+apply ≈11 ms；每条 bash +22–40 ms（中位 ~25 ms）；**仅 bash**（文件/搜索工具 Rust 内不过沙箱）；不计入默认（无 opt-in）路径性能。
- **构建工具**：默认不可执行 cargo/node/uv 等用户目录二进制；`--allow-exec-path <dir>`（可重复）扩展 process-exec；`cc` 需 `-isysroot`；cargo/node 默认不可用属已知限制。

## 6. 错误码/失败面（Lead 裁决：零新码）

| 码/载体 | 触发 | 映射 |
|---|---|---|
| `HARNESS_CONFIG_LOAD{reason}` | `builtin_tools_not_allowed`/`tool_not_registered`/`tool_name_conflict`/`reserved_prefix`/`workspace_invalid`/`allow_path_invalid`/**`sandbox_unavailable`**/**`sandbox_nested`**/**`sandbox_profile_invalid`**（配置/启动前探测失败） | Crashed（Config） |
| `TOOL_ERROR{reason}` | 运行期一切工具失败：`path_escape`/`not_found`/`is_directory`/`invalid_utf8`/`too_large`/`no_match`/`multiple_matches`/`unsupported_patch`/**`sandbox_unavailable`**/`timeout`/`cancelled`/`invalid_args` 等 | 喂回 LLM，不 fail run |

- **不新增 `HARNESS_SANDBOX_UNAVAILABLE`、不新增 `HarnessErrorKind::Sandbox`**：配置/启动前 → `HARNESS_CONFIG_LOAD{reason:"sandbox_unavailable"}`；运行期 → `TOOL_ERROR{reason:"sandbox_unavailable"}`；均 fail-closed（拒绝执行，不降级）。
- 既有 `TOOL_ERROR`/`HARNESS_UNKNOWN_TOOL`/`HARNESS_CONFIG_LOAD` 语义不变；reason 为闭集字符串（新增只追加，不新增顶层码）。

## 7. 安全信任模型

- 文件工具：Rust jail 强制（词法+canonicalize+symlink）；用户显式 workspace/allow-path 即信任边界。
- bash：OS 沙箱强制（非应用层黑名单）；网络默认 deny；无沙箱 fallback。
- 工具参数/输出：不可信数据；不提升为 system/指令；结果截断/脱敏；trace 不写 token/密钥（含 env 值）。
- workspace 是普通目录（不自动 gitignore）；`.tmp` 建议 gitignore；文档警示。
- 不做：容器/VM、完整 Linux/Windows、网络白名单、文件级 allow-path。

## 8. 结果上限与截断（Lead 裁决已确认）

| 工具 | 单次上限 |
|---|---|
| read | 1 MiB 文件/扫描、10000 行（默认 2000）、结果 ≤32 KiB |
| write/edit | 1 MiB 输入；结果 ≤32 KiB |
| diff | 结果 ≤32 KiB（truncated） |
| grep | 1000 条、扫描单文件 2 MiB、结果 ≤32 KiB |
| glob | 默认 1000/上限 5000、结果 ≤32 KiB |
| bash stdout/stderr | 各 16 KiB head+tail（给 LLM/记录；drain 2 MiB；truncated 标记）；command ≤8 KiB、timeout 120s（max 600s） |
| 单文件处理上限 | 2 MiB（grep 扫描）；read/write/edit/apply_patch 1 MiB；超限 `too_large` 或 truncated |
| 所有 builtin 工具结果（LLM/trace） | ≤32 KiB（文件/搜索）；bash 16 KiB head+tail |

> **统一上限**：所有内置工具结果对 LLM 与 trace 均 **≤32 KiB**（bash stdout/stderr 各 16 KiB head+tail）；超限 `truncated` 标记 + 全量 sha256。

## 9. AC-21（文件）/ AC-22（搜索）/ AC-23（bash+沙箱）——设计冻结

| AC | # | 断言 |
|---|---|---|
| AC-21 | a | read offset/limit/UTF-8/不存在/目录；workspace 内成功 |
| AC-21 | b | write 原子（失败不改）、覆盖、create_dirs、大小上限 |
| AC-21 | c | edit 唯一匹配、0/多匹配失败不改、replace_all |
| AC-21 | d | apply_patch 多文件原子（任一失败全不改）、strip、unsupported |
| AC-21 | e | diff 生成/空 diff；jail 越界拒绝 |
| AC-21 | f | `..`/symlink 出根/绝对路径越界 → `path_escape`；`--allow-path` 白名单生效 |
| AC-21 | g | 默认（无 opt-in）不注册/无副作用；`AgentConfig.tools` 白名单语义不变 |
| AC-22 | a | grep regex/fixed/大小写/glob/单文件上限 |
| AC-22 | b | `.gitignore`/隐藏/二进制策略；结果排序确定 |
| AC-22 | c | glob 排序/数量上限/隐藏；越界拒绝 |
| AC-22 | d | 默认零配置不变 |
| AC-23 | a | opt-in 双层：无 `--allow-builtin-tools` 不可见；有文件工具无 `--allow-bash` bash 不可用 |
| AC-23 | b | cwd jail/超时/输出 cap/进程组 kill |
| AC-23 | c | 沙箱内越界写/网络默认 deny；`--allow-network` 生效 |
| AC-23 | d | 沙箱不可用 fail-closed（不降级）；无沙箱 fallback |
| AC-23 | e | 默认路径不变 |

## 10. Lead 裁决（REV-105 已确认）

| # | 事项 | 裁决 |
|---|---|---|
| 1 | 错误码 | **零新码**：配置/启动前沙箱不可用 → `HARNESS_CONFIG_LOAD{reason:"sandbox_unavailable"}`（Crashed(Config)）；运行期 → `TOOL_ERROR{reason:"sandbox_unavailable"}`（喂回 LLM）；不新增码/kind |
| 2 | 网络 | 默认 **deny**；`--allow-network` 显式全开（不做域名白名单） |
| 3 | bash 沙箱不可用 | **fail-closed**：配置期 Crashed(Config)；运行期工具错误；**无无沙箱 fallback** |
| 4 | `--allow-path` | **目录级只读**（canonicalize、`file-read* only`、可重复）；写目标仅 workspace，写 allow-path → `path_read_only`；不做文件级/通配/`--allow-write-path`（ISSUE-167） |
| 5 | apply_patch | **严格原子**（全成功或全不改；暂存+回滚） |
| 6 | v7 上限 | args/result **32 KiB**、bash stdout/stderr 各 **16 KiB head+tail**、command **8 KiB**、path **4 KiB**、单 run **10000 行**；截断带 `truncated` + 全量 `sha256`；副作用回放遇截断 fail-closed；只读 reexec（v0.5）可重算 |
| 7 | reexec | 仅 `read`/`grep`/`glob`/`diff`；`write`/`edit`/`apply_patch`/`bash` **永远 recorded-only** |
| 8 | 工具源/白名单 | **无优先级/无覆盖**，重名 **fail-closed**（§1.1）；`AgentConfig.tools` 白名单对所有源一致；`builtin__` 前缀保留 |

> 第 9 项（macOS sandbox-exec 系统只读路径/弃用行为/TMPDIR/dyld/性能/Linux 承诺）待 arch-researcher spike 结论，落 DR-013 §5/05 §5.15.3。

## 11. spike 落盘补充（2026-11-02，arch-researcher 报告，REV-105）

| 事项 | 冻结 |
|---|---|
| profile | 按附录 A 79 行冻结（`/private/...`、realpath 渲染、系统只读集合、mach 服务、`process-exec` 收窄） |
| 嵌套 Seatbelt | `sandbox-exec` exit **71** → **硬失败** `HARNESS_CONFIG_LOAD{reason:"sandbox_nested"}`；CLI run 预检 **exit 3、不产生 run**；运行期 `TOOL_ERROR{reason:"sandbox_failure"}`；**不提供 `--unsafe-bash`** |
| profile 非法 | exit **65** → `sandbox_profile_invalid`（附 stderr 前 4 KiB） |
| 命令形式 | `/bin/bash --noprofile --norc -c <command>`；env 白名单重建；stdin=null；setsid/killpg（SIGTERM 2s→SIGKILL）；drain 2 MiB；结果 16 KiB head+tail |
| 可执行 | `--allow-exec-path <dir>`（可重复，opt-in）扩展 process-exec；cargo/node/uv 默认不可用（已知限制）；`cc` 需 `-isysroot` |
| 平台 | macOS 26.6 实测；Linux 不支持（fail-closed/文档化）；Windows 不承诺；sandbox-exec DEPRECATED（风险登记，备选 sandbox_init 仅文档） |
| 性能 | profile ≈11 ms；每条 bash +22–40 ms（中位 ~25 ms）；仅 bash，不计入默认路径 |
