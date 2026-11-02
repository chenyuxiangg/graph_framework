# REV-107 · harness · v0.4 D3 内置搜索工具 grep/glob

> **跨模块声明**：本分支仅 `crates/zgraph-harness/**` + 根 `Cargo.toml`/`Cargo.lock`（`ignore`/`globset`/`regex` 新依赖，Lead 已批准、第二评审 Lead 追加）；无 cli/kernel/e2e/docs 改动。评审人只新增本评审记录，未改实现/测试。

- **分支**：`feat/harness-builtin-search`
- 分支: feat/harness-builtin-search
- **作者**：`dsl-dev`（提交 `5b4e899`；正文 `Review: REV-107` / `Agent: dsl-dev`）
- **评审人**：`reviewer-a`（独立评审，≠ 作者）
- **基于**：main `b63ae0e`（`main = origin/main = b63ae0e`）
- **变更范围**：`b63ae0e..5b4e899`（**9 files, +927/−14**）：`crates/zgraph-harness/src/builtin/{search.rs(新增 642),jail.rs,files.rs,mod.rs}`、`constants.rs`、`executor.rs`、根/`zgraph-harness` `Cargo.toml`、`Cargo.lock`
- **评审时间**：2026-11-02
- **关联**：task-79/80；DR-013 §3.6/§3.7/§8、DR-012（D3）、spec 05 §5.15、07 AC-22、00 §0.5；REV-105/106（D1/D2 冻结与实现）

## 1. 结论

**结论：通过**

P0=0、**P1=0、P2=0**；P3 建议 8 项（§3，测试补强/口径说明，非阻塞）。grep/glob 的冻结语义、jail 扩展、注册/白名单/streaming、依赖与门禁均成立；`ignore` 的 `.gitignore`/隐藏/二进制/2 MiB 行为、排序、32 KiB 截断、glob 隐藏/排序/上限、allow-path 只读搜索与越界负例均实测；默认路径/未 opt-in 无回归。

### 1.1 AC-22 映射（设计冻结 → 本分支）

| AC-22 | 断言 | 本分支证据 |
|---|---|---|
| a | grep regex/fixed/case/glob/单文件与总结果上限 | `search.rs` `grep_supports_fixed_strings_case_regex`、`grep_filters_by_glob_suffix`、`grep_honours_max_results_and_truncation`、`grep_result_is_capped_at_32_kib`、`grep_invalid_patterns_and_escape_are_rejected` |
| b | `.gitignore`/隐藏/二进制策略；结果排序确定 | `grep_respects_gitignore_and_skips_hidden_binary_and_large_files`、`grep_reports_path_line_column_and_sorts` |
| c | glob 排序/数量/隐藏/只文件；越界拒绝 | `glob_lists_files_sorted_and_filters_hidden`、`glob_honours_max_results_and_bounds_result_size`、`glob_invalid_pattern_and_escape_are_rejected` |
| d | 默认零配置不变；未 opt-in 不注册 | 默认 workspace 全绿；D2/D3 executor 注册/白名单测试；`tool_defs_include_file_and_search_tools` 仅 `with_builtin_tools` 时 7 工具 |
| jail | allow-path 只读可搜索；越界拒绝；symlink 出 jail 不命中 | `grep_searches_single_file_and_allow_path_read_only`、`grep_invalid_patterns_and_escape_are_rejected`、`walk_candidates` 每文件 canonicalize+`read_scope_of` |
| 注册/streaming | 7 工具共存；`AgentConfig.tools` 白名单；builtin 调用走 streaming+sink | `executor.rs` `search_tools_register_with_file_tools_and_run_through_agent_loop`、`search_tools_must_be_whitelisted`（含 `ToolCallEnd`/ToolSource::Builtin） |

## 2. 四维评审

| 维度 | 结论 | 依据 |
|---|---|---|
| 功能正确性 | ✅ | grep：regex（regex crate 线性）/fixed 转义/大小写/glob 过滤/path 文件或目录；`ignore` 遍历 `.gitignore`（`require_git(false)`）/隐藏始终跳过/`.git` 跳过/NUL 二进制探测/非 UTF-8 跳过/单文件 >2 MiB 静默跳过；`(path,line,column)` 排序；≤1000/32 KiB/单条 text ≤4 KiB（截断标记）；invalid_regex/invalid_glob/path_escape/path_read_only/not_found 闭集。glob：globset/只文件/include_hidden/默认1000上限5000/排序/32 KiB/不应用 gitignore；jail `resolve_any`/`read_scope_*` 每文件 canonicalize+组件前缀；allow-path 只读可搜索，D2 写语义不变；7 工具注册/白名单/streaming+sink 实测 |
| 代码结构合理性 | ✅ | `builtin/search.rs` 独立；jail 扩展最小（`resolve_any`/`read_scope_*`/`is_dir`）；files 的 `limit_result` 扩展 `matches`/`paths` 数组截断；executor 只加注册/测试；依赖仅根/`zgraph-harness` Cargo；无新码/无 unsafe |
| 逻辑性 | ✅ | 遍历入口 `resolve_any` 校验 jail；每文件 canonicalize 后再 `read_scope_of`（symlink 出 jail 不命中）；去重/排序/上限/截断顺序确定；`limit_result` 对搜索数组从尾部截断并置 `truncated`；错误全部 `TOOL_ERROR{reason}`，越界不读；默认/未 opt-in 零副作用 |
| 可读性 | ✅（P3 备注） | 模块文档/常量注释/测试命名（AC-22）清晰；`grep`/`glob` 参数 schema 与 DR 一致；P3 为边界测试/口径补强（§3） |

## 3. 发现清单（P3 建议，非阻塞）

| # | 位置 | 问题 | 建议 |
|---|---|---|---|
| P3-1 | `search.rs` grep 循环 `if matches.len() >= max_results { truncated = true; break }` | **恰好达到 max_results 且无更多匹配时仍置 `truncated=true`**（边界假阳性）；未覆盖「恰好 N 条」用例 | 仅在确认还有更多匹配时置 true（或文档明确「达到上限即 truncated，可能还有更多」），补边界测试 |
| P3-2 | `search.rs` `grep_result_is_capped_at_32_kib` | 该用例默认 `max_results=100` 先生效，**未真正压到 32 KiB 上限**（`limit_result` 由 glob 大列表用例覆盖，但 grep 侧未验证） | 用 `max_results=1000` + 足够长/多匹配使序列化 >32 KiB，断言 `limit_result` 截断路径 |
| P3-3 | `search.rs` 测试集 | 缺非 UTF-8 跳过、glob 不应用 `.gitignore`、遍历 symlink 出 jail 不命中、`.git` 跳过、grep 同文件多列排序、单条 text 4 KiB 截断的显式用例 | 补上述 6 类边界测试（尤其 glob-not-gitignore 与 symlink traversal） |
| P3-4 | `search.rs` `WalkBuilder` | grep 还默认启用了 `.ignore`/`git_global`/`git_exclude`（`ignore` crate 默认），比 DR「尊重 .gitignore」更宽；可能意外跳过 `.ignore` 列出的文件 | 文档明确「同 ignore crate 默认（.gitignore/.ignore/global/exclude）」或在 grep 侧显式关闭 `.ignore`/global；补测试 |
| P3-5 | `search.rs` glob/grep 的 pattern 基准 | glob pattern 与 grep `glob` 过滤均匹配**展示路径（相对 workspace/allow 根）**，未对 `path` 子目录做「relative to path」语义说明/测试；`path=sub` 时 `*.rs` 不命中 `sub/a.rs` | 在 05 §5.15/DR 说明基准（root-relative）或实现/测试 `path` 相对匹配；补 `path=sub` 用例 |
| P3-6 | `constants.rs` | `BUILTIN_GREP_DEFAULT_RESULTS=100` 与 `BUILTIN_SEARCH_TEXT_MAX_BYTES=4096`/新 reason `invalid_regex`/`invalid_glob` 未入 00 §0.5 常量/05 错误表；Lead 已接受 D6 回写 | D6 docs 补常量（grep default 亦补）与 reason；本分支不阻塞 |
| P3-7 | `search.rs` glob 遍历 | `.git` 目录**即使 `include_hidden=true` 也始终跳过**；DR 未显式规定 | 文档说明（安全/性能默认）或按 include_hidden 放开（建议保持跳过并记录） |
| P3-8 | `search.rs` 结果上限 | `limit_result` 对 `matches`/`paths` 截断时 `total` 保持观察值（grep 还受 max_results 影响），与「total=返回/观察数」的 Lead 口径一致，但文档未说明 | D6/05 说明 total 语义（grep 达到 max_results 时 total=返回数；32 KiB 截断时 total=观察数） |

## 4. 验证证据

评审人亲自执行（worktree `.worktrees/harness-dsl-dev`，HEAD `5b4e899`，main `b63ae0e`，worktree 干净；`CARGO_HOME=<repo>/.cargo-home`）：

```bash
cargo test -p zgraph-harness                 # 293 passed / 0 failed / 1 ignored
cargo test -p zgraph-harness --features mcp  # 293 passed / 0 failed / 1 ignored
cargo test --workspace                       # ×2: 1096 passed / 0 failed / 3 ignored
python -m pytest                             # 183 passed
cargo clippy --workspace --all-targets --locked -- -D warnings  # clean
cargo fmt --all -- --check                   # exit 0
cargo check -p zgraph-cli --no-default-features --locked        # 通过
bash scripts/dev/check.sh                    # CHECK PASSED
```

- **范围/依赖**：`Cargo.toml` 新增 `ignore="0.4"`/`globset="0.4"`/`regex="1"`（workspace + harness）；`Cargo.lock` 新增 bstr/crossbeam/globset/ignore 等，无产品代码依赖替换；仅 9 文件、无 cli/kernel/e2e/docs。
- **抽查**：grep 排序（`(path,line,column)`）/`.gitignore`（无 `.git` 目录仍生效）/隐藏/二进制 NUL/2 MiB 跳过/32 KiB 截断/allow-path 只读/越界 `path_escape`；glob 排序/隐藏/默认 1000/上限 5000/32 KiB/只文件/不应用 gitignore；jail 每文件 canonicalize+`read_scope_of`（symlink 出 jail 不命中）；未 opt-in 无注册；7 工具共存 + streaming/sink + 白名单。未发现 `unsafe`/越界读取。
- **对比自报**：harness 293/0/1、workspace ×2 1096/0/3、pytest 183、clippy/fmt/check/no-default 与自报一致。

## 5. 遗留问题

- **D6 docs 回写**（Lead 已接受）：`BUILTIN_SEARCH_TEXT_MAX_BYTES`/`BUILTIN_GREP_DEFAULT_RESULTS` 常量、`invalid_regex`/`invalid_glob` reason、total/truncated 口径、`.gitignore`/`.ignore` 边界、glob pattern 基准。
- **P3 测试补强**：P3-1/P3-2/P3-3/P3-4/P3-5 可在 D4/D5 e2e 或后续小 commit 补；不阻塞 D3。
- **跨模块 Cargo**：`ignore`/`globset`/`regex` 依赖由 Lead 追加第二评审；本记录声明后按流程合并。
- 无 P0/P1/P2；无默认路径回归；P3 均为测试/文档增强。

## 6. 第二评审 · 跨模块（Cargo）（Lead 追加，2026-11-02）

- **评审人**：`lead`（**≠ 作者** `dsl-dev`；Lead 已批准跨模块依赖，按 AGENTS §2 追加第二评审）
- **范围**（`b63ae0e..038b144`）：根 `Cargo.toml` + `crates/zgraph-harness/Cargo.toml` + `Cargo.lock` 新增 `ignore`/`globset`/`regex`（精确锁定、无版本漂移）；`crates/zgraph-harness/src/builtin/search.rs` 等 harness 模块改动；无 CLI/其它跨模块文件。
- **核对结论（通过）**：依赖为 D3 冻结所需最小集（.gitignore 遍历/glob 匹配/线性 regex）；无 feature 构建不变；未改既有行为/错误码。
- **Lead 亲自验证**：`git diff b63ae0e..038b144 --stat` 仅 harness builtin/search + constants/executor tests + 2×Cargo + lock（+ REV 记录）；`cargo test --workspace --offline` = 1096/0/3；`cargo test -p zgraph-harness --offline` = 293/0/1；no-default-features check 通过。
- **裁决/后续**：P3 建议（truncated 假阳性、32KiB 用例、`.ignore`/git global 宽于 DR、path 相对口径、常量 D6 回写）留 D5/D6。
- **结论**：**通过（第二评审）**。分支满足合并前置。
