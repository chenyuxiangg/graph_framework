# REV-097 · harness · V2 skills 发现/注入/skill_view

> **跨模块声明（Lead 已批准；第二评审由 Lead 追加）**：
> 1. `crates/zgraph-cli/src/run_cmd.rs`：`run --skills-dir` 机械接线（解析/环境默认/透传 HarnessExecutor；49 行，无 runner 逻辑改动）。
> 2. `crates/zgraph-kernel/src/types.rs`：**append-only** 追加 `HarnessErrorKind::SkillInvalid` + `CODE_HARNESS_SKILL_INVALID`；既有 kind/码/行为不变。
> 其余 8 文件均在 `crates/zgraph-harness/**`（module 合同内）；无 Cargo.toml/Cargo.lock 依赖变更、无 docs/e2e 改动。

- **分支**：`feat/harness-skills`
- **作者**：`kernel-dev`（harness 角色；提交 `cfe6280` 主实现 + `f286fe8` run 级预算修复）
- **评审人**：`reviewer-b`（独立评审，≠ 作者）
- **基于**：main `29ee6d3`
- **变更范围**：`29ee6d3..f286fe8`（10 files，+2440/−19）：`skills.rs`（新增 1299）、`config.rs`、`assemble.rs`、`agent_loop.rs`、`executor.rs`、`constants.rs`、`error.rs`、`lib.rs`、`run_cmd.rs`、`kernel/types.rs`
- **评审时间**：2026-11-01
- **关联**：task-57/58、DR-009、spec 05 §5.14.1、07 AC-18 a–f、00 §0.3/§0.4/§0.5、DR-008 兼容红线、REV-096 条件关闭

## 1. 结论

**结论：通过**

P0=0；**P1=0**；P2=2（`"*"` 超限/未引用 warning、L2 `llm_calls` 快照含正文，均建议后续收紧）；P3=5（文档/测试/边界建议）。发现/校验、Config 集成、catalog 注入（无 skills 逐字节不变）、`skill_view`（enum/32KiB/资源≤50/去重/run 级 5 次共享预算/`view_budget`/trace chars）、CLI 机械接线均通过；harness 182/0/1、workspace 951/0/3、clippy/fmt/check 全绿。可合并进入 V3 MCP。

## 2. 四维评审

| 维度 | 结论 | 依据 |
|---|---|---|
| 功能正确性 | ✅（P2 备注） | 单层字典序发现、白名单 frontmatter、name=父目录/正则/长度、description/license/compatibility/metadata/allowed-tools、未知字段拒绝、YAML 子集、256KiB/UTF-8/symlink 出根、Config skills（缺失→`skill_not_found`、非法→`SkillInvalid`、未引用→跳过+warn）、catalog ≤4096、`skill_view` 32KiB/资源 50/去重/run 级预算/`TOOL_ERROR{view_budget}`/trace chars；无 skills 路径 golden 不变 |
| 代码结构合理性 | ✅（P3 备注） | `skills.rs` 纯发现/校验/纯渲染；Config 只新增 additive `skills`；LoopState 仅加 cache、AgentRun 加 run 级 Arc 计数器；executor 按 run_id 管理预算；CLI 仅机械接线；kernel/error append-only |
| 逻辑性 | ✅ | 预算先查 cache、CAS 预留、缓存命中不计数；run 级计数器跨 agent_call 共享、不同 run 重置；catalog 在约束段后、空 skills 不注入；工具仅 skills 非空注册；无 skills 不扫描（默认路径不读取 env） |
| 可读性 | ✅（P3 备注） | 模块头映射 DR/AC；测试按发现/校验/catalog/skill_view/budget/default 分组；P3 为边界/文档/测试补齐 |

## 3. 发现清单

> P0/P1=0；P2/P3 建议不阻断本次合并。

### P2 · 建议（后续收紧）

| # | 位置 | 问题 | 建议 |
|---|---|---|---|
| N1 | `skills.rs::references_name`/`skip_warnings`（171-183）、`resolve`（190-245） | `AgentConfig.skills=["*"]` 时：①`references_name` 把 `"*"` 视为命中**所有**非法目录 → 未引用非法 skill 的 stderr warn 全部被吞掉；②`discover` 将第 17+ 个合法目录标为 `too_many_skills`，但 `resolve("*")` 只取前 16 个合法项，**不报 `too_many_skills`、也不 warn**，与 DR-009 §1「超出：被引用超限报 too_many_skills，否则忽略多余并 warn」不一致 | `skip_warnings` 不把 `"*"` 当作非法目录名引用（只对显式 name 命中），使 `"*"` 下非法/超限目录仍 warn；`resolve("*")` 若 valid 数达到上限且存在 `too_many_skills` problems，按 DR 报 `HARNESS_SKILL_INVALID{reason:"too_many_skills"}` 或至少 warn。补 wildcard+17 目录/ wildcard+invalid 测试 |
| N2 | `record.rs::message_snapshot`（105-114）经 CLI `LlmCallTraceRecorder` 落 `llm_calls.request_json` | Streaming trace 已只记 `chars`（测试通过），但 `--record=full` 的 `LlmCallRecorder` 请求快照仍把 `LlmMessage::Tool` 的完整 `content`（含 skill 正文）写入 `llm_calls.request_json`；AC-18 e/DR-009 §7「日志/trace 不写正文全文（仅 name/chars）」若将 L2 llm_calls 纳入范围则不满足 | 明确口径：若 L2 审计允许全量请求，README/DR 记录豁免；若 AC-18 e 覆盖 L2，则对 `skill_view` 工具消息在 request snapshot 中替换为 `{"chars":N,"skill":"name"}`（保留可审计性、去正文）。V5 e2e 前确认 |

### P3 · 台账/测试/文档建议

| # | 位置 | 问题 | 建议 |
|---|---|---|---|
| N3 | `skills.rs::skill_view_text`（866-885） | `Skill directory: {skill.name}` 只输出 name，不是 DR-009 §4 示例中的相对目录（`<assets>/skills/<name>`）；正文结果也未回显 `license`/`compatibility`/`metadata`/`allowed-tools`（AC-18 b「正文原文/元数据」） | 输出稳定的相对路径（如 `skills/<name>`，仍不暴露绝对路径）；若 AC-18 b 的「元数据」要求 frontmatter，补 name/description 等安全字段（license/metadata 仍按「仅记录」口径决定） |
| N4 | `skills.rs::truncate_skill_body`（891-908） | 常量名 `SKILL_BODY_MAX_CHARS`、截断标记 `[…truncated K chars…]`；DR-009 §5 写「32 KiB / bytes」。实现按**字符**截断，非 ASCII 时实际字节可超过 32 KiB | 统一文档口径（字符 vs 字节）；若要求字节，按 UTF-8 边界截断并更新测试/标记 |
| N5 | `skills.rs::validate_skill_fields`/`load_skill` | `duplicate_name` 在单层文件系统发现路径不可达（注释已说明）；`missing SKILL.md` 目录返回 `Ok(None)`（被引用→`skill_not_found`），没有 `duplicate_name`/缺失 SKILL 的独立测试 | 在 DR-009 明确这两个 reason 的可达范围；补 `SKILL.md` 为 symlink 出根（当前仅目录 symlink 测试）等负例 |
| N6 | `skills.rs::parse_yaml_subset` | 不支持 YAML 块标量（`description: >`/`|`）与「引号值后行内注释」；真实 agentskills 资产常用多行 description | 在 DR/测试注明「单行字符串子集」；若接受真实资产，补块标量/行内注释解析与测试 |
| N7 | `skills.rs` tests / `executor.rs` | 测试强度好（skills 18 + config 34 + agent_loop/executor），但缺 `"*"` 超限/非法 warning 的端到端 stderr 断言、`skill_view` 正文是否落入 `llm_calls` 的断言；`skill_view_budgets` map 无清理（长驻 executor 的 run_id 累积，仅内存） | 补上述 2 个断言；executor drop/run 结束时清理 `skill_view_budgets` 或加容量上限 |

## 4. 验证证据

worktree `/Users/zyzs/lczyzs/code/zgraph/.worktrees/harness-harness-dev`，HEAD `f286fe8`，base `29ee6d3`，工作区干净；`CARGO_HOME=/Users/zyzs/lczyzs/code/zgraph/.cargo-home`，未设 `CARGO_TARGET_DIR`：

```bash
cargo test -p zgraph-harness                # 183 targets：182 passed / 0 failed / 1 ignored
cargo test --workspace                      # 72 targets：951 passed / 0 failed / 3 ignored
cargo clippy --workspace --all-targets --locked -- -D warnings  # clean（exit 0）
cargo fmt --all -- --check                  # exit 0
bash scripts/dev/check.sh                   # PASSED
git status --porcelain                      # 空
```

- **与自报对比**：harness 182/0/1（自报 179+ 符合）；workspace 实测 **951/0/3**（自报 948，实测多 3 个通过，无失败；以本次实测为准，不影响结论）。
- **范围**：`git diff --name-only 29ee6d3..f286fe8` 10 files（上列）；**无 Cargo.toml/Cargo.lock 依赖变更**、无 docs/e2e 改动、无 token/密钥；`git status` 空。
- **关键断言**：无 skills system prompt golden（`no_skills_system_prompt_is_byte_identical`、catalog None/空逐字节）；skills catalog 排序/≤4096/省略 marker；skill_view schema enum+additionalProperties:false；正文/资源/只 chars summary；去重不重复计预算；run 级预算共享跨 agent_call、不同 run 重置；`view_budget` 为 TOOL_ERROR 不 fail run；`SkillInvalid` kind/code/detail；未引用非法 skip；缺失引用 config_load；symlink 出根 path_escape；256KiB/UTF-8/资源 50。
- **CLI**：`run --skills-dir` usage/parse/env 透传测试通过；`validate` 未加；默认 run/退出码既有 e2e 全部通过（workspace）。

## 5. 跨模块核对

| 文件 | 改动 | 结论 |
|---|---|---|
| `crates/zgraph-cli/src/run_cmd.rs` | `--skills-dir` 解析/env/透传；`RunArgs` 字段；executor `with_skills_dir` | 机械接线、无 runner/退出码语义；Lead 已批准；第二评审待 Lead |
| `crates/zgraph-kernel/src/types.rs` | `HarnessErrorKind::SkillInvalid` + `CODE_HARNESS_SKILL_INVALID` append-only | 既有 kind/码/分支未改；harness error mapping 测试通过；第二评审待 Lead |

- 未改 `doc/**`、`tests/e2e/**`、`Cargo.toml/Cargo.lock`；无 Cargo 依赖漂移。

## 6. 遗留问题

- **V2 后续（V5 e2e 前）**：N1 wildcard 口径修复/测试；N2 L2 正文口径（AC-18 e）；N3 目录路径/元数据；N4 字符 vs 字节；N5/N6 YAML/可达 reason；N7 预算 map 清理。
- **无 P0/P1**；无附加条件；分支可合并进入 V3 MCP。
