# REV-108 · harness · v0.4 D4 内置 bash + macOS OS 沙箱

- **分支**：`feat/harness-builtin-bash`
- **作者**：`dsl-dev`（单提交 `60aff67`，基于 main `9069e6d`）
- **评审人**：`reviewer-b`（独立评审，≠ 作者）
- **变更范围**：`9069e6d..60aff67`（13 files，+2278/−36）：`builtin/{bash,sandbox}.rs` 新增；`files/jail/mod/tools/agent_loop/executor/constants/lib/test_support`；CLI `run_cmd` 机械；`scripts/verify-sandbox.sh`；无 Cargo 新依赖
- **评审时间**：2026-11-02
- **关联**：task-81/82、DR-013 §3.8/§4/§5/§11、DR-012、spec 05 §5.15.3/§5.15.4、07 AC-23、00 §0.5 `BASH_*`、spike 附录 A、REV-106/107

## 1. 结论

**结论：有条件通过**

P0=0；**P1=2**；P2=7；P3=5。profile 主体/预检分类/`builtin__bash` 参数与上限/进程组回收/CLI 预检/默认零副作用/环境感知测试均与 DR 一致；harness **318/0/1 默认与 features**、workspace **1122/0/3**、pytest **183**、clippy/fmt/check/no-default 全绿。

两项安全边界需关闭：

- **C1 `ensure_tmpdir` jail 逃逸**：对 `<workspace>/.gitignore` 直接 `append`、对 `<workspace>/.tmp` 直接 `create_dir_all`；未 canonicalize/拒绝 symlink。恶意 workspace 把 `.gitignore`/`.tmp` 做成指向 workspace 外的 symlink 时，Rust 侧会 **append/创建到 jail 外**（sandbox 只约束 bash 子进程，不约束本进程的这一步）。
- **C2 SBPL 模板占位符二次替换注入**：`render_profile` 先 `.replace("@WORKSPACE@", workspace)`，再 `.replace("@EXEC_PATHS@"/"@ALLOW_PATHS@"/"@NETWORK@", ...)`；若 workspace/allow/exec 路径包含 `@ALLOW_PATHS@`/`@EXEC_PATHS@`/`@NETWORK@` 字面量，后续替换会把含引号/括号的 SBPL 片段注入到刚渲染的引号字符串内，形成 profile 注入（路径只拒绝了 `\"`/反斜杠/换行/控制字符，未拒绝这些模板 token）。

## 2. 四维评审

| 维度 | 结论 | 依据 |
|---|---|---|
| 功能正确性 | ⚠️（C1/C2） | profile/preflight/bash/CLI/trace 骨架主语义通过；两个 jail/注入边界未覆盖 |
| 结构合理性 | ✅ | `sandbox`（profile/preflight/supervision）与 `bash`（参数/env/结果）分层；Fake `SandboxRunner` 可测；CLI 机械；无新 Cargo 依赖 |
| 逻辑性 | ✅（P2） | 0/71/65 分类、fail-closed、setsid/killpg 近似、drain/head+tail、取消/超时映射均按 DR；P2 见 §3 |
| 可读性 | ✅（P3） | DR 注释/环境限制脚本/错误闭集清楚；P3 为测试/文档补齐 |

## 3. 发现清单

### P1 · 合并前必须关闭

| # | 位置 | 问题 | 条件 |
|---|---|---|---|
| C1 | `builtin/bash.rs::ensure_tmpdir` | `<workspace>/.tmp` `create_dir_all` 与 `<workspace>/.gitignore` `append` 直接按 lexical 路径操作；`.tmp`/`.gitignore` 为 symlink 指向 jail 外时，Rust 宿主进程会创建/追加到外部（bash 沙箱不保护该步骤）；违反 DR-013 §2 路径合同 | 用 `WorkspaceJail`/canonicalize 解析 `.tmp` 与 `.gitignore`：必须在 workspace 内、拒绝 symlink 出根（或 `O_NOFOLLOW`/`symlink_metadata`）；`.gitignore` 非普通文件/出根时跳过而非写入。补 symlink 出根负例 |
| C2 | `builtin/sandbox.rs::render_profile`/`render_path` | `.replace("@WORKSPACE@"...)` 后继续替换 `@EXEC_PATHS@`/`@ALLOW_PATHS@`/`@NETWORK@`；路径中的同名字面量会被二次替换为含 `"`/`(`/`)`/换行的 SBPL 片段，注入 profile | 禁止模板 token 出现在路径（realpath 后检查 `@WORKSPACE@`/`@EXEC_PATHS@`/`@ALLOW_PATHS@`/`@NETWORK@`），或改为一次性安全模板渲染（先校验 token/转义）；补 `workspace/@ALLOW_PATHS@` 等注入负例 |

### P2 · 建议

| # | 位置 | 问题 | 建议 |
|---|---|---|---|
| N1 | `sandbox.rs::run_supervised` | 用 `CommandExt::process_group(0)`，非严格 `setsid`（任务/DR 写 setsid）；killpg 可用但未脱离 session | 明确接受 `process_group` 等价性并文档化，或通过安全 `pre_exec`/nix setsid（注意禁止 unsafe）；补子进程组/会话测试 |
| N2 | `sandbox.rs::render_profile` exec_paths | 仅把 exec_path 放进 `process-exec`，未放 `file-read*`；`--allow-exec-path` 指向系统/workspace 外目录时，动态加载器可能读不到二进制 | exec_path 同时加 `file-read* file-test-existence`；补真实 exec-path 测试 |
| N3 | `bash.rs::ENV_WHITELIST` | 只列 `LANG/LC_ALL/LC_CTYPE`，未覆盖 `LC_NUMERIC/LC_TIME/LC_COLLATE/LC_MESSAGES/...`；任务写 `LANG/LC_*` | 继承全部 `LC_*` 或明确子集；不要继承危险 `LD_*`/`DYLD_*` |
| N4 | `real_sandbox_*` 测试 | 普通终端分支的网络 deny 断言通过 `python3` 执行；profile 系统只读集合未含 Python framework/标准库，Python 可能因读文件失败而非网络被拒，测试可能假阳性 | 用系统自带 `nc`/`curl` 或允许的最小 Python 路径，或断言 stderr/exit 语义区分「被沙箱拒绝」；`verify-sandbox.sh` 输出可重新核对 |
| N5 | `executor`/`agent_loop` 测试 | 无「bash 运行中 run cancel → RunFailure::Cancelled（而非 TOOL_ERROR）」端到端测试；实现 select 分支正确但未锁 | 用 Fake/可控 runner 增加 cancel 测试，断言 run 状态/错误类型/进程组回收 |
| N6 | CLI `resolve_builtin_tools` | 无 `--allow-bash` 预检失败（nested/profile_invalid）**exit3/不建 DB/不产生 run** 的 CLI 集成测试；仅单测 workspace_invalid | 用 fake preflight 或环境探测注入测试（CLI 级），断言 DB 不存在、runs=0 |
| N7 | `sandbox.rs::probe_sandbox_exec` | `stderr` 在 child 退出后才读，超长 stderr 可能阻塞；preflight timeout `child.kill` 非进程组 | 并发 drain/上限读；kill 进程组；补超长 stderr 测试 |

### P3 · 测试/文档

| # | 位置 | 问题 | 建议 |
|---|---|---|---|
| N8 | `real_sandbox_*` / `verify-sandbox.sh` | 本会话嵌套 Seatbelt 环境分类通过；真实隔离（AC-23 c）需普通终端/D5，脚本已提供 | D6 继续用用户终端脚本作证据；release notes 记录 |
| N9 | `sandbox` profile | `signal` 在 terminated/timeout 时硬编码 15（即使实际 SIGKILL）；`head_tail` 对 invalid UTF-8 lossy 展开可能略超 16 KiB | 保留真实 signal；按 UTF-8 字节预算；P3 |
| N10 | profile | `.tmp` 未显式 gitignore 的仓库级追加只在已有 `.gitignore` 时做；任务接受 | 文档化；N1 一并处理 |
| N11 | platform | Linux/Windows 仅 preflight fail-closed，无真实实现；sandbox 模块无条件使用 nix（Cargo 仅 unix target），Windows 编译风险 | 文档/cfg 风险登记（Lead 已接受） |
| N12 | schema | `additionalProperties` 现支持对象（bash env）；D6 回写 05 | D6 回写 |

## 4. 验证证据

worktree `/Users/zyzs/lczyzs/code/zgraph/.worktrees/harness-dsl-dev`，HEAD `60aff67`，base `9069e6d`，工作区干净；`CARGO_HOME=/Users/zyzs/lczyzs/code/zgraph/.cargo-home`，未设 `CARGO_TARGET_DIR`：

```bash
cargo test -p zgraph-harness                 # 319 tests：318 passed / 0 failed / 1 ignored
cargo test -p zgraph-harness --features mcp  # 319 tests：318 passed / 0 failed / 1 ignored
cargo test --workspace                       # 76 targets：1122 passed / 0 failed / 3 ignored
pytest -q                                    # exit 0；183 passed
cargo clippy --workspace --all-targets --locked -- -D warnings  # clean
cargo fmt --all -- --check                   # exit 0
cargo check -p zgraph-harness --no-default-features  # clean
bash scripts/dev/check.sh                    # PASSED
```

- **与自报一致**：harness 318/0/1（默认与 features）、workspace 1122/0/3、pytest 183；clippy/fmt/check/no-default 全绿；`git status` 空；仅 CLI `run_cmd` 跨模块 + 无 Cargo 新依赖。
- **环境感知测试**：`real_sandbox_preflight_and_isolation_environment_aware` 在本会话嵌套 Seatbelt 下按 `sandbox_nested`/`sandbox_unavailable` 断言并打印环境限制（不伪造成功）；`crates/zgraph-harness/scripts/verify-sandbox.sh` 供普通终端；AC-23 c 真隔离未在本机验证（Lead 预裁决接受，D5/D6）。
- **关键抽查**：profile 渲染 workspace/allow/exec/network、`/private` realpath、注入字符拒绝；preflight 0/71/65/其它分类；bash 参数/env 白名单/`/bin/bash --noprofile --norc -c`/stdin null/head+tail/drain/nonzero/取消/超时；CLI 3 flag 互斥、预检在 DB 前；无 `--unsafe-bash`/无 sandbox fallback；`unsafe` 搜索无命中。
- **未覆盖/风险（P1/P2）**：C1 `.tmp/.gitignore` symlink、C2 模板 token 注入、setsid 严格性、exec-path file-read、真实网络隔离假阳性、bash 取消端到端、CLI 预检失败无 DB 测试。

## 5. Lead 开放项确认

- AC-23 c 真隔离待普通终端/D5/D6 用户终端脚本证据：**接受**（本机嵌套环境仅 fail-closed 验证）。
- Linux/Windows 无实现、fail-closed + cfg 风险：**接受**（文档/后续登记）。
- `--allow-exec-path` 不存在 → `sandbox_profile_invalid`：**接受**。
- `.tmp` 自动创建/gitignore 追加：**接受**（但 C1 symlink 仍需修）。
- bash 16 KiB×2 不受 32 KiB 总限：**接受**（`limit_result` 对 bash 例外）。
- schema `additionalProperties` 支持：**接受**，D6 回写 05。
- TOCTOU/hardlink 残余：**接受**，文档化。
- 无 fallback/无 `--unsafe-bash`、无 unsafe：**已核实**。

## 6. 遗留问题

- **C1/C2**：修 `ensure_tmpdir` symlink jail + 模板 token 注入；补负例后复审转「通过」。
- **P2 N1–N7**：setsid/exec-path file-read/LC_*/真实网络断言/取消端到端/CLI 预检 DB 测试/preflight stderr。
- **P3 N8–N12**：真实终端/D5 e2e/平台/文档/schema 回写。
- 无 P0；C1/C2 未关闭前不可 merge。
