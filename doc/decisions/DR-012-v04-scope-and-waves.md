# DR-012 · v0.4（M9）范围与波次（内置工具 + 严格 workspace 沙箱 + 工具级 trace/replay）

> **状态**：V1 设计冻结（REV-105；沙箱 spike 结论待 Lead 转发，相关数字标「待 spike」）。
> **日期**：2026-11-02 · **作者**：docs-dev · **关联**：DR-013、DR-014、task-74 合同、05 §5.6/§5.14、06 §6.2/§6.14、07 AC-21–24
> **用户决策（task-74）**：文件/搜索/bash 工具 + **严格 workspace jail** + **OS 沙箱**，全部 **opt-in**；工具命名 **`builtin__<tool>`**；trace **schema v7 + `tool_calls` + 默认 recorded 回放**；W5 顺延 **v0.5**。

## 0. 摘要

| 项 | 冻结 |
|---|---|
| 范围 | **内置工具 8 个**：`builtin__read`/`write`/`edit`/`apply_patch`/`diff`/`grep`/`glob`/`bash`；严格 workspace jail；bash OS 沙箱；trace v7 + 工具级回放 |
| 非目标 | W5 共享评审队列（→v0.5）；多模态；工具并行；无沙箱 bash fallback；Windows 沙箱（v0.4 只保证 macOS，Linux 备选文档化）；skill 脚本执行；网络代理 |
| 默认不变 | 无 `--allow-builtin-tools` / 无 workspace 配置：图 hash/validate/run/trace/replay 逐字节不变；DSL 1.0/R1–R18/19 码不变 |
| 工具命名 | `builtin__<tool>`（双下划线命名空间；与 `mcp__<server>__<tool>` 并列） |
| opt-in | `--allow-builtin-tools`（文件/搜索）+ `--allow-bash`（bash 额外）；二者缺省关 |
| workspace | `--workspace-dir <dir>`（缺省 `.`，canonicalize）；`--allow-path <dir>` 可重复白名单；所有文件操作 **只在 jail 内** |
| bash | **必须**在 OS 沙箱内（macOS `sandbox-exec`；spike 定 profile）；沙箱不可用 → **fail-closed 拒绝 bash**（明确错误码） |
| trace | **schema v7**：新表 `tool_calls`；默认 `--replay-tools=recorded`（副作用永不重跑）；`reexec` 仅只读工具 |
| 发布 | v0.4.0（M9），需用户授权 tag/GitHub Release |

## 1. 范围与非目标

### 1.1 范围（全部为 v0.4 主线）

1. **文件类**：`read`（offset/limit）、`write`（原子）、`edit`（唯一匹配、失败不改）、`apply_patch`（多文件原子）、`diff`（unified diff 生成）。
2. **搜索类**：`grep`（regex/字面、路径/glob 过滤、.gitignore/隐藏/二进制策略）、`glob`（globset，排序确定性）。
3. **终端**：`bash`（cwd/env/超时/输出上限/进程组；**OS 沙箱**；网络默认 deny）。
4. **jail/opt-in/工具源优先级**：`--workspace-dir`、`--allow-path`、`--allow-builtin-tools`、`--allow-bash`；`builtin__` / `mcp__` / Python 工具源优先级与冲突规则。
5. **trace v7**：`tool_calls` 表、迁移、人类/`--json` 工具行、`--replay-tools=recorded|reexec`、L2 关系。
6. **安全模型**：文件工具 Rust 强制 jail；bash OS 沙箱强制；不做「无沙箱 fallback」；默认网络拒绝。

### 1.2 非目标

- **W5 共享评审队列 → v0.5**（用户/Lead 决策；v0.4 不含）。
- 工具并行执行（沿用串行）；多模态；skill 脚本执行；MCP resources/prompts；OAuth/DCR。
- Windows 沙箱/完整跨平台：v0.4 **只保证 macOS**；Linux 用 `bwrap`/`landlock` 作备选文档化（spike 结论，未实测不承诺）。
- 不做「无沙箱 bash」逃生门；不做网络代理/域名白名单（v0.4 仅 `--allow-network` 全开）。
- 不改变 DSL/图语义；内置工具属 Harness Loading，不进 DSL/hash。

## 2. 与 v0.3 的关系

- v0.3 已交付 skills/MCP/可插拔 harness；v0.4 复用其 **工具注册表/`AgentConfig.tools`/错误面/streaming sink**，不重写。
- 工具源三类并列：`builtin__<tool>`（Rust 内置）、`mcp__<server>__<tool>`（v0.3）、Python 工具（既有 `tools/`）；优先级/冲突规则见 DR-013 §4。
- trace v7 **只增 `tool_calls`**；v6→v7 迁移不改既有表/事件；无工具调用时 v7 行为与 v6 一致。
- v0.4 不改变外部 harness 协议（`zgraph-harness/1`）；外部 harness 内部工具不落 `tool_calls`（v0.3 边界，DR-011 §12）。

## 3. 波次（D1–D6）

```
D1 docs 设计冻结（本批，REV-105）
 └─▶ D2 文件类 + jail + trace 骨架（feat/harness-builtin-files）
       └─▶ D3 搜索类（feat/harness-builtin-search）
             └─▶ D4 bash + OS 沙箱 + 权限（feat/harness-builtin-bash）
                   └─▶ D5 trace v7 + 回放 + e2e（feat/trace-v7-tools / test/e2e-v04）
                         └─▶ D6 兼容/性能/as-built/发布 v0.4.0（docs/e2e/process）
```

> **串行约束**：D2–D4 均写 `crates/zgraph-harness`（同模块单写入者），必须**串行**；D5 跨 harness/trace/cli（Lead 批准 + 第二评审）；仅 e2e/docs 准备可并行。

| 波次 | 分支（建议） | 作者/评审 | 交付 | 依赖 |
|---|---|---|---|---|
| D1 | `docs/docs-v04-design` | docs-dev / reviewer-a + Lead 裁决 | DR-012/013/014 + spec 00/02/05/06/07/08 冻结草案 + AC-21–24 | 无 |
| D2 | `feat/harness-builtin-files` | harness-dev / reviewer-b | jail（canonicalize/symlink/`--allow-path`）+ read/write/edit/apply_patch/diff + `builtin__` 注册 + 文件错误面 | D1 |
| D3 | `feat/harness-builtin-search` | harness-dev / reviewer-a | grep/glob（ignore/globset/regex 选型、.gitignore/隐藏/二进制/上限） | D2 |
| D4 | `feat/harness-builtin-bash` | harness-dev / reviewer-b（+Lead 第二评审） | `builtin__bash` + macOS sandbox-exec profile + 网络 deny/`--allow-network` + fail-closed | D3 |
| D5 | `feat/trace-v7-tools` + `test/e2e-v04` | trace-dev/harness-dev/cli-dev/e2e-dev / reviewer-a/b（跨模块 Lead 批准） | v7 表/迁移/reader/trace 输出/`--replay-tools` + AC-21–24 e2e | D2–D4 |
| D6 | `docs/docs-v04-spec` + `chore/process-v04-release` | docs-dev / reviewer-b | as-built 回写、PROVENANCE、README/progress、性能基线、发布 v0.4.0 | D2–D5 |

## 4. 发布条件（v0.4.0）

1. AC-21（文件）、AC-22（搜索）、AC-23（bash+沙箱）、AC-24（trace/replay）全部实测。
2. 默认路径（无 opt-in/无 workspace）兼容矩阵无新增失败；DSL/hash 不变。
3. trace v6 旧库可迁移 v7（备份/只读拒绝/链式 v1..v6→v7）；无工具调用 v7 行为与 v6 一致。
4. `cargo test --workspace`/e2e/pytest/clippy/fmt/check.sh 全绿；性能回退 ≤20%（v0.3 基线，同机 release 3 次中位）。
5. 安全门禁：jail 逃逸/OS 沙箱失败/网络默认拒绝均有负例；无沙箱 fallback。
6. as-built 回写 00/02/05/06/07/08 + PROVENANCE；README/进度/发布材料齐备。

## 5. 兼容矩阵

| 场景 | 期望 |
|---|---|
| 无 `--allow-builtin-tools` | 不注册任何 builtin 工具；无 workspace 副作用；run/trace/replay 逐字节不变 |
| 有 opt-in、`AgentConfig.tools` 未列 builtin | 工具不可见/不可调用（既有白名单语义） |
| 有 opt-in、仅文件工具 | bash 仍不可用（需 `--allow-bash`） |
| 无工具调用 | trace v7 与 v6 行内容/行为一致（仅 `user_version`/迁移差异） |
| 旧 v6 库 | `validate`/`run` 只读拒绝或升级（迁移策略）；`trace` 读取兼容 |
| 外部 harness | 不受影响；不落 `tool_calls` |

## 6. AC-21–24 矩阵（设计冻结；D5 实测）

| AC | 内容 | 关键断言（摘要） | 实测波次 |
|---|---|---|---|
| AC-21 | 文件类 + jail | read/write/edit/apply_patch/diff 语义；workspace 内成功；`..`/symlink/绝对路径越界拒绝；`--allow-path` 白名单生效；write 原子；apply_patch 原子；默认零配置不变 | D2/D5 |
| AC-22 | 搜索类 | grep/glob 路径过滤/排序/`.gitignore`/隐藏/二进制/大小上限；结果确定性；越界拒绝 | D3/D5 |
| AC-23 | bash + OS 沙箱 | opt-in 双层；cwd/超时/输出上限/进程组；只读 workspace 外写失败；网络默认 deny；`--allow-network`；沙箱不可用 fail-closed；无沙箱 fallback | D4/D5 |
| AC-24 | trace v7 + 回放 | v6→v7 迁移/备份/只读拒绝/链式；`tool_calls` 字段/上限；`--replay-tools=recorded` 副作用不重跑；reexec 仅只读；trace 人类/`--json`；无工具调用与 v6 一致 | D5 |

## 7. 风险

| 风险 | 说明 | 缓解 |
|---|---|---|
| macOS sandbox-exec 弃用/行为差异 | `sandbox-exec` 已标记 deprecated，profile 语法/系统路径集合随 OS 变化 | spike 锁定 profile+系统只读集合；运行时探测；不可用 fail-closed（不降级）；文档标注 macOS 版本 |
| Linux 无等价保证 | bwrap/landlock 需 root/内核支持 | v0.4 只承诺 macOS；Linux 文档化「未保证」；v0.5 再评估 |
| TOCTOU/symlink | 检查与使用之间路径被替换 | canonicalize 后按真实路径校验；文件操作 `O_NOFOLLOW`/目录 fd 尽量；`--allow-path` 同样 canonicalize；负例测试 |
| bash 逃逸 | 子进程可写 jail 外/联网 | OS 沙箱强制（非应用层黑名单）；默认 deny network；进程组 kill；stderr/stdout cap |
| 大输出/内存 | 读大文件/bash 无限输出 | 全部输出 cap+truncated；`read` offset/limit；grep/glob 大小/条数上限 |
| apply_patch 非原子 | 多文件部分成功 | 先全部校验+临时文件/回滚计划；任一失败不改任何文件；AC 负例 |
| v7 迁移不可逆 | 旧库升级后旧二进制不可读 | 迁移前备份；`user_version` 只读拒绝；链式 v1..v6→v7；文档 |
| reexec 误执行副作用 | 只读判定错误/工具演化 | 默认 recorded；reexec 仅显式且白名单 read/grep/glob/diff；记录 hash 校验；失败 fail-closed |

## 8. 待 Lead 裁决项

| # | 事项 | 推荐 |
|---|---|---|
| 1 | 错误码命名 | **零新码（Lead 裁决）**：不新增 `HARNESS_SANDBOX_UNAVAILABLE`/kind；配置/启动前 → `HARNESS_CONFIG_LOAD{reason:"sandbox_unavailable"}`（Crashed(Config)）；运行期 → `TOOL_ERROR{reason:"sandbox_unavailable"}`（喂回 LLM） |
| 2 | 网络默认 | **已裁决：deny**；`--allow-network` 显式全开（不做域名白名单） |
| 3 | bash 沙箱不可用错误面 | **fail-closed**：配置/启动前不可用 → `HARNESS_CONFIG_LOAD{reason:"sandbox_unavailable"}`（Crashed(Config)）；运行期探测/执行失败 → `TOOL_ERROR{reason:"sandbox_unavailable"}`（不 fail run）；**无无沙箱 fallback** |
| 4 | `--allow-path` 粒度 | **已裁决：目录级**（canonicalize；读写全开），不接受文件级/通配；可重复 |
| 5 | apply_patch 多文件原子性 | **已裁决：严格原子**：全成功或全不改；跨文件事务用「暂存+回滚」；不做部分应用 |
| 6 | v7 字段上限 | **已裁决**：args/result **64 KiB**、stdout/stderr **32 KiB**、command **8 KiB**、path **4 KiB**、单 run **10000 行**；超限 truncated + 全量 sha256；副作用回放遇截断 **fail-closed**；只读 reexec 可重算 |
| 7 | 只读工具 reexec 集合 | **已裁决**：`read`/`grep`/`glob`/`diff`；`write`/`edit`/`apply_patch`/`bash` **永不** reexec |
| 8 | W5 | 顺延 v0.5（已裁决） |
