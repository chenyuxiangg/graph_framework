# DR-009 · skills 设计冻结（Agent Skills：发现/索引/按需加载）

> **状态**：V1 设计冻结；实现见 DR-008 V2（`feat/harness-skills`）。
> **日期**：2026-11-01 · **作者**：docs-dev · **关联**：DR-008、05 §5.2/§5.4/§5.10、02 §2.5.7/§2.11 C-20、07 AC-18
> **规范目标**：对齐 agentskills.io 的 `SKILL.md` 形态（frontmatter + Markdown）；**只做指令注入与按需加载，不执行 skill 脚本**。

## 0. 摘要

| 项 | 冻结 |
|---|---|
| 目录 | `<skills_dir>/<name>/SKILL.md`（`<skills_dir>` 缺省 `.zgraph/skills`；CLI `--skills-dir` > env `ZGRAPH_SKILLS_DIR` > 缺省） |
| frontmatter | 必填 `name`/`description`；可选 `license`/`allowed-tools`/`metadata`；**未知键拒绝**（`$schema` 允许忽略）；YAML 子集 |
| 注入 | system 仅注入**索引**：每 skill 一行 `name: description`（+来源目录）；正文不默认注入 |
| 按需加载 | 内置工具 `skill_view(name)` 返回正文（Markdown 原文）+ 元数据；单次上限与总预算见 §5 |
| 配置 | `AgentConfig.skills: ["name"...]`（additive；缺省=不加载）；`"*"` 表示全部可用（仍受 allowlist/目录限制） |
| 错误 | `HARNESS_SKILL_NOT_FOUND` / `HARNESS_SKILL_INVALID` / `HARNESS_SKILL_LOAD`（append-only，闭集见 §6） |
| 无 skills | 不发现/不注入/不注册 `skill_view` → 既有行为/输出逐字节不变 |

## 1. 目录与发现

```text
<skills_dir>/
  <name>/
    SKILL.md            # 必填
    references/…        # v0.3 不自动注入（skill_view 仅正文；附属文件延后）
  <other>/…
```

- 发现：按目录名字典序（确定性）；只认 `<name>/SKILL.md` regular file。
- `name` 规则：`^[a-z0-9]+(-[a-z0-9]+)*$`，1–64 字符；必须与目录名一致；重复（大小写/Unicode 归一化后同）→ `HARNESS_SKILL_INVALID`。
- 路径安全：canonicalize 后必须位于 skills 根内；拒绝 `..`、绝对路径、symlink 指向根外（§7）。
- 大小上限：`SKILL.md` ≤ **256 KiB**（UTF-8）；目录条目数 ≤ **1024**（超出报错，不静默截断）。

## 2. `SKILL.md` 格式

```markdown
---
name: code-review
description: Review a patch for correctness and style; use when asked to review code.
license: MIT                 # 可选
allowed-tools: [read_file]   # 可选；v0.3 仅记录，不据此限制工具
metadata:
  version: "1.0"             # 可选
---

# Code Review
…正文（指令/流程/示例）…
```

- frontmatter 为 **YAML 子集**：映射/字符串/布尔/数组；不支持锚点/标签/多文档；解析失败 → `HARNESS_SKILL_INVALID`。
- 必填：`name`（string，§1）、`description`（string，1–1024 字符，非空）。
- 可选：`license`（string）、`allowed-tools`（string 数组；仅透传元数据）、`metadata`（开放 map<string,string>）、`$schema`（忽略）。
- 未知键 → `HARNESS_SKILL_INVALID{reason:"unknown_field"}`；缺失/类型错 → `HARNESS_SKILL_INVALID{reason:"invalid_frontmatter"}`。
- 正文：Markdown 原文；v0.3 不做模板替换、不执行代码块/脚本。

## 3. AgentConfig 集成（additive）

```json
{
  "agent_id": "code_reviewer",
  "skills": ["code-review", "security-review"],
  "tools": ["read_file"]
}
```

- `skills?: string[]`：缺省 `[]`（不加载任何 skill，也不注册 `skill_view`）；`"*"` = 全部可用（仍按目录/allowlist）。
- 校验（Harness Loading，run 期懒加载；非 DSL）：未知 skill → `HARNESS_SKILL_NOT_FOUND`；重复/非法 pattern → `HARNESS_CONFIG_LOAD`。
- 与 `tools` 正交：`skill_view` 是**框架内置工具**，仅当 `skills` 非空时注册；`allowed-tools` 不裁剪工具集（v0.3 元数据）。
- 不参与 `graph_hash`（02 §2.5.7/§2.8.3）；无 skills 的 AgentConfig/图行为不变。

## 4. 注入策略（渐进披露）

system 组装（05 §5.4.2）新增可选段 **skills 索引**（仅在 `skills` 非空时出现）：

```text
## Available skills
- code-review: Review a patch for correctness and style; use when asked to review code.
- security-review: Check for security issues before release.
Use the skill_view tool to load a skill's full instructions when needed.
```

- 每 skill 仅 `name` + `description`（单行，截断至 1024 字符）；正文**不在** system。
- `skill_view(name)` 工具：返回 `{name, description, path, content}`（content=正文原文）；按需调用；同一 run 内可重复调用（幂等，只读）。
- token 预算：索引计入 05 §5.4 预算；正文按单次调用上限（§5）裁剪；超预算时 05 §5.4.5 既有裁剪算法生效（索引优先保留，正文裁剪尾部并标注 `…[truncated]`）。
- 不可信内容：正文以「数据」注入（工具返回内容），不提升为 system/developer 指令；system 索引仅描述，不执行。

## 5. 预算与截断（冻结）

| 项 | 上限 |
|---|---|
| `SKILL.md` 大小 | 256 KiB |
| description | 1024 字符 |
| system 索引单 skill | 1 行（name+description；超长截断） |
| `skill_view` 单次返回正文 | 32768 字符（严格截断，附 `truncated:true`） |
| 单 run `skill_view` 总预算 | 5 次调用/`MAX_SKILL_VIEWS=5`（v0.3 常量；超限 → 工具错误 `HARNESS_SKILL_LOAD{reason:"view_budget"}`） |

- 常量进入 00 §0.5（`MAX_SKILL_VIEWS=5`、`SKILL_BODY_MAX_CHARS=32768`、`SKILL_FILE_MAX_BYTES=262144`）。
- 截断只影响注入内容，不改 skill 文件/不影响 hash。

## 6. 错误码（append-only，闭集）

| 码 | 触发 | 形态 |
|---|---|---|
| `HARNESS_SKILL_NOT_FOUND` | AgentConfig 引用的 skill 不存在 | Harness 错误 → 节点 `Crashed`（`HarnessErrorKind::Config`） |
| `HARNESS_SKILL_INVALID` | frontmatter/name/路径/大小/重复非法；reason 闭集 `unknown_field`/`invalid_frontmatter`/`invalid_name`/`duplicate_name`/`too_large`/`path_escape`/`yaml_parse` | 同上 |
| `HARNESS_SKILL_LOAD` | 读取失败/IO/`view_budget` | 同上（`skill_view` 调用可返回工具错误而非崩溃，见 §5） |

- `HARNESS_*` 是 Harness 错误命名空间；不写入 `runs.error_code` 领域码表以外的新领域码（沿用 Harness 映射，01/05）。既有码不变；新码只追加到 00 §0.4 Harness 域与 00 §0.5 常量。

## 7. 安全

- **不执行** skill 内脚本/代码块；`scripts/` 目录不读取（v0.3）。
- 路径：canonicalize + 前缀校验；拒绝 symlink 出根；只读 regular file；UTF-8 校验失败 → `HARNESS_SKILL_INVALID`。
- 注入：正文不可信，不覆盖系统指令；索引/正文均标注来源 `skill:<name>`；日志/trace 不写正文全文（仅 name/大小/摘要）。
- 预算：大小/调用次数上限防 DoS。

## 8. 兼容矩阵

| 场景 | 期望 |
|---|---|
| 无 `skills` 配置 | 不发现目录/不注入/不注册 `skill_view`；AgentConfig/run/trace 与 v0.2 逐字节不变 |
| 有 skills、无 `skill_view` 调用 | system 多索引段；run 其余行为不变（可断言 trace/输出差异仅 system 注入） |
| skills 目录不存在 | 无 `skills` 引用时不报错；有引用 → `HARNESS_SKILL_NOT_FOUND` |
| DSL/图 hash | 完全不变（skills 不参与 hash） |

## 9. AC-18（设计冻结；实测 V2）

| # | 用例 | 断言 |
|---|---|---|
| a | 合法 skill 发现/校验 | 目录/frontmatter 合法 → 索引注入（name/description）、无正文默认注入 |
| b | `skill_view` 按需加载 | 调用返回正文原文/元数据；未调用时正文不出现在 system/请求 |
| c | 非法 frontmatter/未知键/重复/路径穿越 | 对应 `HARNESS_SKILL_INVALID{reason}`；不执行任何内容 |
| d | token 预算/截断 | 超大 description/正文按上限截断（`truncated:true`）、`MAX_SKILL_VIEWS` 超限报错 |
| e | 不可信内容安全 | 正文含指令/脚本不执行、不提升为 system；只作为数据注入 |
| f | 默认无 skills 不变 | 无配置的 AgentConfig/run/trace/replay 与 v0.2 golden 逐字节一致；`graph_hash` 不变 |

> **AC-18 默认零配置路径**：每条用例均在「无 `skills` 配置」对照下复跑（或由既有 v0.2 golden/兼容测试覆盖）：不发现/不注入/不注册 `skill_view`，输出与 v0.2 逐字节一致。

## 10. Lead 裁决（2026-11-01，已确认）

| # | 事项 | 裁决 |
|---|---|---|
| 1 | frontmatter 未知顶层字段 | **拒绝**（`$schema` 忽略；`metadata` 为自由 map）；`HARNESS_SKILL_INVALID{reason:"unknown_field"}` |
| 2 | `skills: "*"` | **仅显式 `*`**；缺省 `[]` 不加载 |
| 3 | `skill_view` 附属文件 | v0.3 **否**（仅正文）；附属文件延后 |
| 4 | 常量 | 采纳 `MAX_SKILL_VIEWS=5`/`SKILL_BODY_MAX_CHARS=32768`/`SKILL_FILE_MAX_BYTES=262144`（00 §0.5）；变更需 DR 修订 |
| 5 | skills 目录缺省 | `.zgraph/skills`（与 trace DB 同根，gitignore） |
| 6 | 不可信数据 | **强制**：skills 内容按不可信数据注入（标注来源、不执行脚本、大小/条数上限）；`skill_view` 调用次数/正文长度受常量约束；默认无 skills 路径逐字节不变 |

> 安全条款（§7）与本节为冻结口径；V2 实现必须逐条覆盖 AC-18。
