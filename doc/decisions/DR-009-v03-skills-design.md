# DR-009 · skills 设计冻结（Agent Skills：目录/索引/按需加载）

> **状态**：V1 设计冻结（评审 REV-096）；实现见 DR-008 V2（`feat/harness-skills`）。
> **日期**：2026-11-01 · **作者**：docs-dev · **关联**：DR-008、05 §5.2/§5.4/§5.10、02 §2.5.7/§2.11 C-20、07 AC-18、[`research-v03-capabilities.md`](../research-v03-capabilities.md)
> **规范目标**：对齐 agentskills.io `SKILL.md`（frontmatter + Markdown，三级渐进披露）；**只做指令注入与按需加载，不执行 skill 脚本**。

## 0. 摘要

| 项 | 冻结 |
|---|---|
| 目录 | `<assets-dir>/skills/<name>/SKILL.md`；CLI `--skills-dir` > env `ZGRAPH_SKILLS` > `<assets-dir>/skills`（与 `--tools-dir`/`ZGRAPH_TOOLS` 同构） |
| 发现 | **单层、不递归**（不扫描 `.git/`、`node_modules/`、祖先/git root）；按目录名字典序；只认 `<name>/SKILL.md` |
| frontmatter | 必填 `name`（≤64、小写字母数字连字符、**必须等于父目录名**）、`description`（1–1024 必填；**>1024 拒绝** `description_too_long`）；可选 `license`/`compatibility`（1–500）/`metadata`（map<string,string>）/`allowed-tools`（实验性，**仅记录不授权**）；未知顶层字段拒绝（`$schema` 忽略） |
| 注入 | system 仅 **catalog**：每 skill `name: description`（整体 ≤4096 字符）；正文不默认注入 |
| 按需加载 | 内置 `skill_view(name)` 返回正文（≤32 KiB，首尾保留+中间截断标记）+ 资源清单（只列表不读） |
| 配置 | `AgentConfig.skills?: string[]`（缺省 `[]`，长度 ≤16；每项须命中索引）；`"*"` 显式全量 |
| 错误 | 被引用但不存在 → `HARNESS_CONFIG_LOAD{reason:"skill_not_found"}`；存在但格式非法 → `HARNESS_SKILL_INVALID{reason}`；**不新增 `HARNESS_SKILL_NOT_FOUND`**；未被引用的无效 skill 跳过 + stderr warn |
| 无 skills | 不扫描/不注入/不注册 `skill_view` → 既有行为/输出逐字节不变 |

## 1. 目录与发现

```text
<assets-dir>/
├── agents/<id>.json
├── tools/…
└── skills/
    ├── code-review/SKILL.md
    └── pdf-processing/{SKILL.md, references/, assets/}
```

- 目录优先级：`--skills-dir <dir>` > `ZGRAPH_SKILLS` > `<assets-dir>/skills`（`--assets-dir` 缺省 `.`）。
- 发现：**单层、不递归**；目录名排序；跳过 `.git/`、`node_modules/` 等隐藏/依赖目录；只认 `<name>/SKILL.md` 常规文件。
- `name`：`^[a-z0-9]+(-[a-z0-9]+)*$`，1–64 字符；**必须等于父目录名**；不得 `--` 连续/首尾 `-`；同 scope 重名（目录名唯一即 name 唯一）→ 跳过并 warn（若被引用则 `HARNESS_SKILL_INVALID{reason:"duplicate_name"}`）。
- 大小/条数：`SKILL.md` ≤ **256 KiB**；目录条目 ≤ **5000**；skill 数 ≤ **16**（超出：被引用超限报 `HARNESS_SKILL_INVALID{reason:"too_many_skills"}`，否则忽略多余并 warn）。
- 路径安全：canonicalize 后必须位于 skills 根内；拒绝 `..`/绝对路径/symlink 出根/device/FIFO；UTF-8 校验（§7）。

## 2. `SKILL.md` 格式

```markdown
---
name: code-review
description: Review a patch for correctness and style; use when asked to review code.
license: MIT
compatibility: Requires git and a POSIX shell
metadata:
  version: "1.0"
allowed-tools: read_file
---

# Code Review
…正文（指令/流程/示例）…
```

| 字段 | 必填 | 约束 |
|---|---|---|
| `name` | ✅ | ≤64；小写字母/数字/连字符；**=父目录名**；不得首尾 `-`/连续 `--` |
| `description` | ✅ | 1–1024 字符；说明「做什么 + 何时用」；**>1024 → `HARNESS_SKILL_INVALID{reason:"description_too_long"}`** |
| `license` | ❌ | 许可名或 LICENSE 文件引用 |
| `compatibility` | ❌ | 1–500 字符；环境要求 |
| `metadata` | ❌ | `map<string,string>`（自由 key；不参与行为） |
| `allowed-tools` | ❌ | 空格分隔字符串；**实验性、仅记录，不改变工具授权** |
| `$schema` | ❌ | 允许并忽略 |

- YAML 子集：映射/字符串/布尔/数组；不支持锚点/标签/多文档；解析失败 → `HARNESS_SKILL_INVALID{reason:"unparseable_yaml"}`。
- 未知顶层字段 → `HARNESS_SKILL_INVALID{reason:"unknown_field"}`；`metadata` 内部自由。
- 正文：Markdown；v0.3 不做模板替换、不执行代码块/脚本；**整个 SKILL.md 在激活时一次性进上下文**，建议 <500 行/<5000 token，细节放 `references/`（v0.3 不自动读 resources）。

## 3. AgentConfig 集成（additive）

```json
{"agent_id":"code_reviewer","skills":["code-review","security-review"],"tools":["read_file"]}
```

- `skills?: string[]`：缺省 `[]`（不扫描/不注册 `skill_view`）；`"*"` = 全部可用（仍受目录/上限约束）；长度 ≤16。
- 校验：引用不存在 → `HARNESS_CONFIG_LOAD{reason:"skill_not_found"}`；存在但非法 → `HARNESS_SKILL_INVALID{reason}`；未被引用的非法 skill 跳过 + stderr warn（不阻塞其它 Agent）。
- 与 `tools` 正交：`skill_view` 是框架内置工具，仅当 `skills` 非空时注册；`allowed-tools` 不合并进 `tools`、不改变权限。
- 不参与 `graph_hash`（02 §2.5.7/§2.8.3）；无 skills 的 AgentConfig/图行为不变。

## 4. 注入策略（渐进披露）

system 新增可选 **catalog**（仅 `skills` 非空时出现；位置在 `## 约束` 后、记忆/KB 前）：

```markdown
## 技能
以下技能提供专门指令。当任务与某技能描述匹配时，调用 skill_view 加载完整指令，再继续。
<available_skills>
  <skill><name>code-review</name><description>Review a patch…</description></skill>
  <skill><name>security-review</name><description>Check security issues…</description></skill>
</available_skills>
```

- 只注入 `name`/`description`；不注入 `location`（避免暴露绝对路径/引导文件读）、`metadata`/`license`；`compatibility` v0.3 不注入。
- catalog 上限 **4096 字符**（≈1k token）；**展示行超长才按行截断**（追加 `(catalog truncated: N skills omitted)`）；description 本身 >1024 **拒绝**，不是展示截断。
- `skill_view(name)`：参数 `name` 为已注册枚举（`additionalProperties:false`）；返回工具结果文本：

```text
<skill_content name="pdf-processing">
{正文（frontmatter 已剥离）}
Skill directory: <assets>/skills/pdf-processing
<skill_resources>
  <file>references/REFERENCE.md</file>
</skill_resources>
</skill_content>
```

- 资源清单只列文件不读内容（≤50 条，超出 `<!-- truncated -->`）；不暴露宿主机绝对路径给消息（用 skill 相对路径）。
- 正文进 messages 后标记为 **protected**：预算裁剪（05 §5.4.5）优先裁 tool 结果/历史，**不裁 skill 正文**（对齐 agentskills「skill content 不参与压缩」）；同 session 重复激活去重。
- 不可信数据：正文/catalog 以「数据」注入，不提升为 system/developer 指令；不执行脚本。

## 5. 预算与截断（冻结）

| 项 | 上限 |
|---|---|
| `SKILL.md` 大小 | 256 KiB |
| description | 1024 字符 |
| catalog 总长 | 4096 字符 |
| `skill_view` 单次正文 | 32 KiB（≈8k token；超出首尾保留 + `[…truncated K bytes…]`） |
| `AgentConfig.skills` | 16 |
| 单 run `skill_view` 次数 | 5（`MAX_SKILL_VIEWS`；超出 → 工具错误 `TOOL_ERROR{reason:"view_budget"}`，不新增码） |
| 资源清单 | ≤50 条；单资源 ≤1 MiB（仅列名，不读） |

- 常量进 00 §0.5：`MAX_SKILL_VIEWS=5`、`SKILL_BODY_MAX_CHARS=32768`、`SKILL_FILE_MAX_BYTES=262144`、`MAX_SKILLS=16`、`SKILL_CATALOG_MAX_CHARS=4096`。
- 截断只影响注入内容；不改 skill 文件、不影响 hash。

## 6. 错误码（append-only，闭集）

| 码 | 触发 |
|---|---|
| `HARNESS_CONFIG_LOAD{reason:"skill_not_found"}` | `AgentConfig.skills` 引用不存在（**不新增 SKILL_NOT_FOUND 码**） |
| `HARNESS_SKILL_INVALID{reason}` | 被引用的 skill 存在但非法；reason 闭集：`unknown_field`/`unparseable_yaml`/`invalid_frontmatter`/`invalid_name`/`name_mismatch`/`missing_description`/`name_too_long`/`duplicate_name`/`too_many_skills`/`too_large`/`path_escape`/`not_utf8`/`description_too_long` |
| `TOOL_ERROR{reason}` | `skill_view` 读取失败/IO/`view_budget`（喂回 LLM，不 fail run；不新增码） |

- 映射：`HARNESS_SKILL_INVALID` → 节点 `Crashed(HarnessErrorKind::SkillInvalid)`；`HARNESS_CONFIG_LOAD{skill_not_found}` → `Crashed(Config)`；`skill_view` 的未知/预算错误 → `TOOL_ERROR` 工具结果（喂回 LLM，不 fail run）。
- 既有码不变；新码只追加 00 §0.4 Harness 域。

## 7. 安全

- **不执行** skill 内脚本/代码块；`scripts/` 目录不读取（v0.3，仅资源清单列名）。
- `allowed-tools` **不授权**：仅记录（实验性），不改变 `tools`/MCP/网络策略。
- 路径：canonicalize + 前缀校验；拒绝 symlink 出根/绝对路径/`..`/隐藏 `.git`；只读常规文本文件；UTF-8 校验失败 → `not_utf8`。
- 注入：正文不可信，不覆盖系统指令；标注来源 `skill:<name>`；日志/trace 不写正文全文（仅 name/chars）。
- `skill_view` 每次调用记 `ToolCallStart/ToolCallEnd`（`tool_name="skill_view"`、`args_summary={"name":...}`；只记 `chars`，不落正文）。
- 预算：大小/catalog/正文/调用次数上限防 DoS。

## 8. 兼容矩阵

| 场景 | 期望 |
|---|---|
| 无 `skills` 配置 | 不扫描/不注入/不注册 `skill_view`；AgentConfig/run/trace/replay 与 v0.2 逐字节不变 |
| 有 skills、未调用 `skill_view` | system 多 catalog 段；run 其余行为不变 |
| skills 目录不存在 | 无引用不报错；有引用 → `HARNESS_CONFIG_LOAD{reason:"skill_not_found"}` |
| DSL/图 hash | 完全不变（skills 不参与 hash） |
| 未被引用的非法 skill | 跳过 + stderr warn，不阻塞其它 Agent/skill |

## 9. AC-18（设计冻结；实测 V2）

| # | 用例 | 断言 |
|---|---|---|
| a | 合法 skill 发现/校验 | 目录/frontmatter（name=父目录/description）→ catalog 注入、无正文默认注入；无 skills 不发现 |
| b | `skill_view` 按需加载 | 返回正文/资源清单（不读资源）；未调用时正文不在 system/请求；重复激活去重 |
| c | 非法/安全 | 未知字段/name≠目录/重复/`..`/symlink 出根/超大小/非法 UTF-8 → `HARNESS_SKILL_INVALID{reason}`；`allowed-tools` 不授权 |
| d | 预算/截断 | ① description>1024 → **拒绝** `HARNESS_SKILL_INVALID{reason:"description_too_long"}`；② catalog 展示行超 4096/正文>32KiB → 展示截断（首尾保留+标记）；`MAX_SKILL_VIEWS` 超限报错；skills>16 处理 |
| e | 不可信内容 | 正文含「忽略之前指令」/脚本 → 仅作为数据注入、不执行、不提升为 system；日志/trace 不含正文全文 |
| f | 默认不变 | 无 `skills` 的 AgentConfig/run/trace/replay 与 v0.2 golden 逐字节一致；`graph_hash` 不变 |

> **AC-18 默认零配置路径**：每条用例均在无 `skills` 配置对照下复跑（或由既有 v0.2 golden 覆盖）；不扫描/不注入/不注册工具，输出逐字节一致。

## 10. Lead 裁决（REV-096 A–D 范围；2026-11-01 已确认）

> 来源：本表仅列 REV-096 A–D 裁决范围；超出 A–D 的设计项为 V1 冻结推荐（非 Lead 裁决），实现前可由评审再议。

| # | 事项 | 裁决 |
|---|---|---|
| 1 | 未知顶层字段 | **拒绝**（`$schema` 忽略；`metadata` 自由 map）；`HARNESS_SKILL_INVALID{reason:"unknown_field"}` |
| 2 | `skills: "*"` | **仅显式 `*`**；缺省 `[]` 不加载 |
| 3 | `skill_view` 附属文件 | v0.3 **否**（仅正文；资源清单只列名） |
| 4 | 常量 | 采纳 `MAX_SKILL_VIEWS=5`/`SKILL_BODY_MAX_CHARS=32768`/`SKILL_FILE_MAX_BYTES=262144`/`MAX_SKILLS=16`/`SKILL_CATALOG_MAX_CHARS=4096` |
| 5 | 资产/env | `<assets-dir>/skills` + `ZGRAPH_SKILLS`；`--skills-dir` 最高优先 |
| 6 | 错误口径 | 被引用不存在 → `HARNESS_CONFIG_LOAD{reason:"skill_not_found"}`；存在但非法 → `HARNESS_SKILL_INVALID`；**不新增 `HARNESS_SKILL_NOT_FOUND`**；未被引用非法 → 跳过+warn |
| 7 | 不可信数据/`allowed-tools` | 正文按不可信数据注入；`allowed-tools` 仅记录不授权；默认无 skills 路径逐字节不变 |

## 11. 实现执行记录（as-built，2026-11-02，REV-101；只追加）

- **分支/提交**：`feat/harness-skills`：`cfe6280`（发现/注入/skill_view）+ `f286fe8`（run 级预算，REV-097 C1）+ `6844573`（wildcard 语义，P2①）；评审 REV-097（含跨模块 Lead 第二评审）；合并 `69aeb87`。
- **实测**：harness **235/0/1**；skills 测试 **18 例**；既有 workspace **1037/0/3**；无 skills system prompt 逐字节不变；catalog 排序/4096/省略标记；`skill_view` enum+`additionalProperties:false`、正文 32768 **字符**（非 bytes；非 ASCII 实际字节可略超）、`MAX_SKILL_VIEWS=5` run 级共享、超限 `TOOL_ERROR{reason:"view_budget"}`、正文不进 trace（仅 chars）。
- **实现口径/偏差**：`SKILL_BODY_MAX_CHARS` 按**字符**（REV-097 P3）；L2 `--record=full` 的 `llm_calls.request_json` 是否含正文待 AC-18 e/V5a 确认（REV-097 P2 N2）；`skill_view` 输出 `Skill directory: <name>`（未含相对路径，REV-097 P3 N3）；YAML 子集不支持块标量（N6）；wildcard `"*"` 的非法/超限 warn 口径待跟进（N1）。
- **已知限制**：真实资产兼容/YAML 块标量、L2 正文口径、目录条目上限等 P2/P3 入台账（ISSUE-156+）；AC-18 V5a 实测 10 用例已回写 07。
