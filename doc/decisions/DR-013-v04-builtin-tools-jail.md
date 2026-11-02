# DR-013 · v0.4 内置工具与工作区 jail（8 工具 + OS 沙箱）

> **状态**：V1 设计冻结（REV-105；macOS sandbox-exec profile 的系统只读路径/探测方式待 arch-researcher spike 结论；本文标「待 spike」处按推荐冻结）。
> **日期**：2026-11-02 · **作者**：docs-dev · **关联**：DR-012、DR-014、05 §5.6/§5.14、06 §6.2、07 AC-21/22/23、task-74
> **安全合同（用户加严）**：所有文件操作**只能在 `--workspace-dir` 内**，除非显式 `--allow-path` 白名单；**bash 必须 OS 沙箱**；**无沙箱 fallback**。

## 0. 摘要

| 项 | 冻结 |
|---|---|
| 工具集 | 8 个：`builtin__read`/`write`/`edit`/`apply_patch`/`diff`/`grep`/`glob`/`bash` |
| 命名 | `builtin__<tool>`（双下划线；与 `mcp__<server>__<tool>` 并列，不出现在现有 `^[a-z][a-z0-9_]{0,63}$` 冲突范围外） |
| opt-in | `--allow-builtin-tools`（文件/搜索）；`--allow-bash`（bash 额外）；缺省关 |
| jail | `--workspace-dir <dir>`（缺省 `.`，canonicalize）；`--allow-path <dir>` 可重复；词法 + canonicalize + symlink 逃逸拒绝 |
| bash | macOS `sandbox-exec` profile：仅 workspace+allowlist 读写、系统只读、`TMPDIR=<workspace>/.tmp`；网络默认 deny；`--allow-network` 全开；沙箱不可用 → **fail-closed** |
| 安全模型 | 文件工具 **Rust 强制** jail；bash **OS 沙箱强制**；不做应用层黑名单/无沙箱 fallback |
| 错误 | 工具失败 `TOOL_ERROR{reason}`；opt-in/配置 `HARNESS_CONFIG_LOAD{reason}`；沙箱不可用 `HARNESS_SANDBOX_UNAVAILABLE`（推荐；见 §9） |
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

## 2. 工作区 jail（文件工具）

### 2.1 配置

| CLI/env | 语义 |
|---|---|
| `--workspace-dir <dir>` | 工作区根；缺省 `.`；必须存在且为目录；**canonicalize**；写入 `workspace_root` |
| `--allow-path <dir>`（可重复） | 额外允许的**目录**（canonicalize；读写全开）；非目录/不存在 → 配置错误；不递归子路径限制（目录即全开） |
| env `ZGRAPH_WORKSPACE_DIR` / `ZGRAPH_ALLOW_PATHS`（`:` 分隔） | 与 CLI 等价（CLI 优先）；v0.4 至少 CLI |

- **路径解析顺序**：工具参数 path（相对/绝对）→ 词法归一化（拒绝 `..` 逃逸）→ `canonicalize`（解析 symlink）→ 与 `workspace_root`/`allow_paths` 的**真实路径前缀**比较（按路径组件边界，非字符串前缀）。
- **越界**：`TOOL_ERROR{reason:"path_escape", path, root}`（喂回 LLM）；不执行任何读写。
- **symlink**：文件/中间目录 symlink 指向 jail 外 → 拒绝；指向 jail 内 → 允许（按真实路径判定）。
- **特殊文件**：device/FIFO/socket → `TOOL_ERROR{reason:"not_regular_file"}`；只读普通文件。
- **TOCTOU**：打开时尽量 `O_NOFOLLOW`（文件工具）+ 目录 fd/真实路径；负例仅承诺 jail 判定（OS 沙箱覆盖 bash）。
- 无 workspace 参数时也不得默认 `/`：缺省 `.`（当前目录 canonicalize）；`--allow-builtin-tools` 未开时不解析 workspace。

### 2.2 风险说明

- 文件工具是**进程内 Rust**（不是 Python worker），jail 由 Rust 强制；不依赖 OS 沙箱。
- `--allow-path` 是**用户显式信任**的目录（读写全开），文档警示；不提供文件级/通配粒度（Lead 裁决项）。
- 不防「用户自己把 workspace 指向 `/`」——显式配置即信任，文档警示。

## 3. 8 个工具语义（冻结）

### 3.1 `builtin__read`

```json
{"type":"object","properties":{
  "path":{"type":"string","description":"相对 workspace 或 allow-path 的路径；绝对路径须在 jail 内"},
  "offset":{"type":"integer","minimum":0},
  "limit":{"type":"integer","minimum":1,"maximum":2000}
},"required":["path"],"additionalProperties":false}
```

- 读 UTF-8 文本；`offset` 行偏移（0 基），`limit` 行数（缺省 200，≤2000）；非 UTF-8 → `TOOL_ERROR{reason:"invalid_utf8"}`（v0.4 不做二进制 base64）。
- 结果：`{content, lines, total_lines?, truncated?}`；单次输出 cap（推荐 256 KiB/32K chars，见 §8）。
- 目录/不存在/越界 → `TOOL_ERROR{reason:"is_directory"|"not_found"|"path_escape"}`。

### 3.2 `builtin__write`

```json
{"type":"object","properties":{
  "path":{"type":"string"},
  "content":{"type":"string"},
  "create_dirs":{"type":"boolean","default":false}
},"required":["path","content"],"additionalProperties":false}
```

- **原子**：同目录临时文件 + fsync + rename；失败不改原文件；`create_dirs=false` 时父目录须存在。
- 覆盖写；结果 `{path, bytes_written, sha256_before?, sha256_after}`；大小上限（推荐 1 MiB/次；超限拒绝 `too_large`）。
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
- 原子写（同 write）；保留文件权限（不新建时）。

### 3.4 `builtin__apply_patch`

```json
{"type":"object","properties":{
  "patch":{"type":"string","description":"unified diff（git 风格）；支持多文件"},
  "strip":{"type":"integer","minimum":0,"default":1}
},"required":["patch"],"additionalProperties":false}
```

- 支持 `---/+++`、`@@`、多文件；**严格原子**：先解析全部 hunk → 校验全部目标文件与上下文 → 全部成功才写；任一失败**不改任何文件**（推荐，Lead 裁决项）。
- 只允许 workspace/allow-path；`a/`/`b/` 前缀按 `strip` 去；不支持二进制/rename/copy（→ `TOOL_ERROR{reason:"unsupported_patch"}`）。
- 结果：`{files:[{path, added, removed, sha256_before, sha256_after}], total_added, total_removed}`；每文件/总大小上限。

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

- 遍历：**尊重 `.gitignore`**（推荐 `ignore` crate；`--no-ignore` 不做）/隐藏文件默认跳过（推荐）；二进制跳过（NUL 探测）；单文件 ≤2 MiB、总结果 ≤1000 条/≤256 KiB；路径 jail 同 read。
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

- `globset` 匹配；不递归出 jail；结果按路径排序（确定性）；数量/大小上限；越界拒绝。
- 结果 `{paths:[...], total, truncated}`；目录是否返回（推荐仅文件）。

### 3.8 `builtin__bash`

```json
{"type":"object","properties":{
  "command":{"type":"string","minLength":1,"maxLength":131072},
  "cwd":{"type":"string","default":"."},
  "timeout_ms":{"type":"integer","minimum":100,"maximum":600000,"default":30000},
  "env":{"type":"object","additionalProperties":{"type":"string"},"description":"可选，仅白名单追加"}
},"required":["command"],"additionalProperties":false}
```

- **必须**在 OS 沙箱内执行（§5）；cwd 必须在 jail 内（canonicalize）；`TMPDIR=<workspace>/.tmp`（创建、gitignore）。
- 超时/取消 → kill **进程组**；stdout/stderr 各 cap 32 KiB（截断标记）；结果 `{exit_code, stdout, stderr, truncated, sandbox:"macos-sandbox-exec", duration_ms}`。
- 非 0 退出是**正常工具结果**（喂回 LLM，不 fail run）；沙箱启动失败/不可用 → fail-closed（§5.3）。
- 网络默认 deny；`--allow-network` 显式开启（全局，v0.4 不做域名白名单）。
- 不提供 `shell` 选项（固定 `/bin/bash -lc`）；环境变量只透传白名单（PATH/HOME/TMPDIR/LANG 等）+ 显式 `env`；不写 token。

## 4. opt-in 与配置

| 开关 | 注册 | 缺省 |
|---|---|---|
| `--allow-builtin-tools` | 文件/搜索 7 个 | 关 |
| `--allow-bash` | bash（**隐含**需要 OS 沙箱可用；不隐含文件工具） | 关 |
| `--workspace-dir` | jail 根 | `.` |
| `--allow-path` | 额外白名单目录 | 空 |
| `--allow-network` | bash 网络 | 关（deny） |

- 未开启 `--allow-builtin-tools` 时，`AgentConfig.tools` 出现 `builtin__*` → `HARNESS_CONFIG_LOAD{reason:"builtin_tools_not_allowed"}`。
- 开启文件工具但未开 bash：bash 不出现在注册表；引用 → 同上/`tool_not_registered`。
- `--allow-bash` 但 OS 沙箱探测失败 → **加载期** `HARNESS_SANDBOX_UNAVAILABLE`（Crashed），不注册 bash（fail-closed；Lead 裁决项）。
- 运行时沙箱故障（探测后异常）→ 该次 bash 调用 `TOOL_ERROR{reason:"sandbox_failure"}`，不降级执行。

## 5. OS 沙箱（bash）

### 5.1 macOS `sandbox-exec` profile（推荐骨架；spike 定稿）

```scheme
(version 1)
(deny default)
(allow process*)
(allow sysctl-read)
(allow file-read* (subpath "<workspace_root>") (subpath "<allow_path>") ...)
(allow file-write* (subpath "<workspace_root>") (subpath "<workspace_tmp>"))
(allow file-read* (subpath "/usr") (subpath "/bin") (subpath "/sbin")
                 (subpath "/System") (subpath "/Library")
                 (literal "/dev/null") (literal "/dev/urandom") ...)
(allow file-write* (subpath "<workspace_tmp>"))
(allow network* )   ; 仅 --allow-network
```

- **系统只读集合**：`/usr`、`/bin`、`/sbin`、`/System`、`/Library`、`/dev/null`、`/dev/urandom` 等（spike 用 `ls -l`/dyld 依赖实测补齐）；`/etc`、`/private/etc` **只读**（或按需）；`/tmp`/`$TMPDIR` **不**开放（TMPDIR 指到 workspace/.tmp）。
- **写**：仅 `workspace_root` + `allow_paths` + `<workspace>/.tmp`；其余 deny。
- **网络**：默认 `(deny network*)`；`--allow-network` 时 `(allow network*)`（全开，文档警示）。
- 进程：允许 fork/exec（bash 需要），但继承同一 profile；限制资源由 timeout/输出 cap 兜底。
- **可用性探测**：启动时 `sandbox-exec -p <profile> /usr/bin/true` 成功才注册 bash；失败 → `HARNESS_SANDBOX_UNAVAILABLE`。
- profile 生成：字符串模板 + 路径转义（`"`/`\`）；不拼接用户命令（command 作为 `sandbox-exec` 的 argv 参数，固定 `bash -lc`）。
- **待 spike**：系统只读路径最小集合、`sandbox-exec` 在 macOS 版本上的弃用行为、dyld/临时文件需求、性能开销、失败模式。

### 5.2 Linux（备选，不承诺）

- bwrap（`--ro-bind` 系统、`--bind` workspace、`--unshare-net`）或 landlock；v0.4 只文档化，不实测/不承诺；`--allow-bash` 在无可用沙箱的 Linux → `HARNESS_SANDBOX_UNAVAILABLE`。

### 5.3 失败面（fail-closed）

| 场景 | 结果 |
|---|---|
| `sandbox-exec` 不存在/不可执行 | 加载期 `HARNESS_SANDBOX_UNAVAILABLE`（Crashed；bash 不注册） |
| profile 语法/启动失败 | 加载期同上；运行期 `TOOL_ERROR{reason:"sandbox_failure"}` |
| 沙箱内命令越界写/联网 | 命令自身失败（OS deny）；exit≠0 作为工具结果；**不**降级到无沙箱 |
| bash 未 opt-in | 不注册；引用 → 配置错误 |
| OS 非 macOS 且无等价实现 | Linux 文档化；其他平台 bash 不可用（fail-closed） |

## 6. 错误码/失败面（推荐，Lead 裁决）

| 码 | 触发 | 映射 |
|---|---|---|
| `HARNESS_CONFIG_LOAD{reason}` | `builtin_tools_not_allowed`/`tool_not_registered`/`tool_name_conflict`/`workspace_invalid`/`allow_path_invalid`/`sandbox_unavailable`（若零新码） | Crashed（Config） |
| `TOOL_ERROR{reason}` | 运行期一切工具失败：`path_escape`/`not_found`/`is_directory`/`invalid_utf8`/`too_large`/`no_match`/`multiple_matches`/`unsupported_patch`/`sandbox_failure`/`timeout`/`cancelled`/`invalid_args` 等 | 喂回 LLM，不 fail run |
| **`HARNESS_SANDBOX_UNAVAILABLE`**（推荐新增，append-only） | bash opt-in 但 OS 沙箱不可用（fail-closed） | Crashed（Config） |

- 若 Lead 要求**零新码**：沙箱不可用 → `HARNESS_CONFIG_LOAD{reason:"sandbox_unavailable"}`，不新增码。
- 既有 `TOOL_ERROR`/`HARNESS_UNKNOWN_TOOL`/`HARNESS_CONFIG_LOAD` 语义不变；新码只追加 00 §0.4。

## 7. 安全信任模型

- 文件工具：Rust jail 强制（词法+canonicalize+symlink）；用户显式 workspace/allow-path 即信任边界。
- bash：OS 沙箱强制（非应用层黑名单）；网络默认 deny；无沙箱 fallback。
- 工具参数/输出：不可信数据；不提升为 system/指令；结果截断/脱敏；trace 不写 token/密钥（含 env 值）。
- workspace 是普通目录（不自动 gitignore）；`.tmp` 建议 gitignore；文档警示。
- 不做：容器/VM、完整 Linux/Windows、网络白名单、文件级 allow-path。

## 8. 结果上限与截断（推荐；Lead 裁决）

| 工具 | 单次上限 |
|---|---|
| read | 256 KiB / 2000 行 |
| write/edit | 1 MiB 输入；结果摘要 ≤8 KiB |
| diff | 256 KiB diff |
| grep | 1000 条 / 256 KiB |
| glob | 5000 条 / 1 MiB |
| bash stdout/stderr | 各 32 KiB（truncated 标记） |
| 单文件处理上限 | 2 MiB（grep/read）；超限 `too_large` 或 truncated |

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
