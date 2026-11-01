//! v0.3 skills：`SKILL.md` 发现/校验、catalog 注入与内置 `skill_view` 工具（DR-009）。
//!
//! 语义依据：`doc/decisions/DR-009-v03-skills-design.md`（唯一语义依据）与
//! spec 05 §5.14.1 / 07 AC-18。要点：
//!
//! - **发现**：`--skills-dir` > env `ZGRAPH_SKILLS` > `<assets-dir>/skills`；**单层、不递归**
//!   （只认 `<name>/SKILL.md`，目录名字典序）；隐藏目录/`node_modules` 跳过；白名单式
//!   frontmatter 校验（未知顶层字段拒绝，`$schema` 忽略）。
//! - **注入**：system 仅注入 catalog（`name: description`，≤4096 字符）；正文按需经
//!   `skill_view(name)` 内置工具加载（≤32 KiB 首尾保留截断），资源只列名不读。
//! - **安全**：路径 canonicalize 后必须在 skills 根内；不执行 skill 内脚本/代码；
//!   `allowed-tools` 仅记录不授权；正文按不可信数据处理（不提升为 system 指令）。
//! - **默认不变**：无 `AgentConfig.skills` 时不发现/不注入/不注册工具（无额外 stderr）。
//!
//! 本模块只做纯函数/只读 IO；`skill_view` 由 `agent_loop` 在进程内调用（不启动 worker）。

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use crate::constants::{
    MAX_SKILLS, MAX_SKILL_COMPATIBILITY_CHARS, MAX_SKILL_DESCRIPTION_CHARS, MAX_SKILL_DIR_ENTRIES,
    MAX_SKILL_NAME_CHARS, MAX_SKILL_RESOURCES, SKILL_BODY_MAX_CHARS, SKILL_CATALOG_MAX_CHARS,
    SKILL_FILE_MAX_BYTES, SKILL_VIEW_TOOL_NAME, SKILL_VIEW_TOOL_SOURCE,
};
use crate::error::HarnessError;
use crate::tools::ToolDef;

/// 一个已校验的 skill（正文暂存内存；`skill_view` 返回时按 32 KiB 截断）。
#[derive(Debug, Clone, PartialEq)]
pub struct SkillEntry {
    /// skill 名（=父目录名；已校验小写连字符规则）。
    pub name: String,
    /// 1–1024 字符描述（catalog 只注入它）。
    pub description: String,
    /// 可选许可名（仅记录，不注入）。
    pub license: Option<String>,
    /// 可选环境要求（1–500；仅记录，v0.3 不注入）。
    pub compatibility: Option<String>,
    /// 可选自由元数据（`map<string,string>`；仅记录）。
    pub metadata: BTreeMap<String, String>,
    /// 可选实验性工具声明（**仅记录、不授权**）。
    pub allowed_tools: Option<String>,
    /// 正文（frontmatter 已剥离；原始长度，`skill_view` 时截断）。
    pub body: String,
    /// 资源文件相对路径（已排序，≤50；只列名不读）。
    pub resources: Vec<String>,
    /// 资源清单是否被截断（超过 50 条）。
    pub resources_truncated: bool,
}

/// 一个未被引用时应跳过（+warn）的非法 skill。
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SkillProblem {
    /// 冻结 reason 闭集字符串（DR-009 §6）。
    pub(crate) reason: &'static str,
    /// 人类可读描述（不含正文）。
    pub(crate) message: String,
}

/// skills 发现结果：合法条目 + 非法条目（按目录名）+ 全局警告。
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct SkillsIndex {
    /// 目录名 → 已校验 skill（BTreeMap 即字典序）。
    valid: BTreeMap<String, SkillEntry>,
    /// 目录名 → 非法原因（被引用时转 `HARNESS_SKILL_INVALID`；未引用跳过+warn）。
    problems: BTreeMap<String, SkillProblem>,
    /// 超过 `MAX_SKILLS` 而被忽略的目录名（**未解析**；显式引用 → `too_many_skills`）。
    ///
    /// 冻结语义（REV-097 C1 / Lead 裁决）：合法 skill 数 > `MAX_SKILLS` 时 **不静默截断**；
    /// `"*"` 解析全部合法 skills，超限 → `HARNESS_CONFIG_LOAD{reason:"too_many_skills"}`；
    /// 未被引用的额外目录忽略 + stderr warn。
    overflow: BTreeSet<String>,
    /// 全局警告（根不可读/非 UTF-8 目录名等；未引用时不报错）。
    warnings: Vec<String>,
}

impl SkillsIndex {
    /// 扫描 skills 根（缺失/不可读 → 空索引；单层、不递归、字典序）。
    ///
    /// 注：单层目录名天然唯一，因此 DR-009 §6 的 `duplicate_name` reason 在 v0.3
    /// 文件系统发现路径**不可达**（保留在闭集定义中供未来自定义索引实现）。
    pub(crate) fn discover(root: &Path) -> Self {
        let mut index = Self::default();
        let canonical_root = match fs::canonicalize(root) {
            Ok(path) => path,
            Err(error) => {
                // 目录不存在 = 没有 skills（有引用时由 resolve 报 skill_not_found）；
                // 其它 IO 错误只 warn，不阻塞加载。
                if error.kind() != std::io::ErrorKind::NotFound {
                    index.warnings.push(format!(
                        "skills dir {} is unreadable: {error}",
                        root.display()
                    ));
                }
                return index;
            }
        };
        let entries = match fs::read_dir(&canonical_root) {
            Ok(entries) => entries,
            Err(error) => {
                index.warnings.push(format!(
                    "skills dir {} is unreadable: {error}",
                    root.display()
                ));
                return index;
            }
        };
        let mut dirs: Vec<String> = Vec::new();
        for (seen, entry) in entries.flatten().enumerate() {
            if seen >= MAX_SKILL_DIR_ENTRIES {
                index.warnings.push(format!(
                    "skills dir {} exceeds {MAX_SKILL_DIR_ENTRIES} entries; extras ignored",
                    root.display()
                ));
                break;
            }
            let name = match entry.file_name().into_string() {
                Ok(name) => name,
                Err(_) => {
                    index
                        .warnings
                        .push("skills dir contains a non-UTF-8 entry (skipped)".to_string());
                    continue;
                }
            };
            if is_ignored_dir(&name) {
                continue;
            }
            // 跟随 symlink 判断是否目录；出根由 `load_skill` 的 canonicalize 检查。
            let Ok(metadata) = fs::metadata(entry.path()) else {
                continue;
            };
            if metadata.is_dir() {
                dirs.push(name);
            }
        }
        dirs.sort();
        for (position, dir_name) in dirs.iter().enumerate() {
            // 已发现 > MAX_SKILLS 个合法 skill：不静默截断——剩余目录记为 overflow
            // （不解析；显式引用 → `too_many_skills`；`"*"` → CONFIG_LOAD too_many_skills）。
            if index.valid.len() > MAX_SKILLS {
                index.overflow.extend(dirs[position..].iter().cloned());
                break;
            }
            let skill_dir = canonical_root.join(dir_name);
            match load_skill(&canonical_root, &skill_dir, dir_name) {
                Ok(Some(skill)) => {
                    index.valid.insert(dir_name.clone(), skill);
                }
                Ok(None) => {
                    // 无 SKILL.md 的目录不是 skill（有引用时 resolve 报 skill_not_found）。
                }
                Err(problem) => {
                    index.problems.insert(dir_name.clone(), problem);
                }
            }
        }
        index
    }

    /// 全局警告（调用方可写 stderr；默认路径不会产生）。
    pub(crate) fn warnings(&self) -> &[String] {
        &self.warnings
    }

    /// 未被引用的非法/超限 skill → 跳过并返回 warn 行（DR-009 §3/§8）。
    ///
    /// **`"*"` 不是「引用具体目录」**（REV-097 C1）：wildcard 不会吞掉非法目录的 warn，
    /// 只有**显式引用**某目录时才不 warn（该路径由 `resolve` 报错）。
    pub(crate) fn skip_warnings(&self, referenced: &[String]) -> Vec<String> {
        let mut warnings: Vec<String> = self
            .problems
            .iter()
            .filter(|(name, _)| !references_name(referenced, name))
            .map(|(name, problem)| {
                format!(
                    "warning: skipping invalid skill '{name}' ({}): {}",
                    problem.reason, problem.message
                )
            })
            .collect();
        for name in &self.overflow {
            if !references_name(referenced, name) {
                warnings.push(format!(
                    "warning: ignoring skill directory '{name}' (exceeds MAX_SKILLS = {MAX_SKILLS})"
                ));
            }
        }
        warnings
    }

    /// 解析 `AgentConfig.skills` 引用 → 已校验条目（按 name 升序，含 `"*"` 展开）。
    ///
    /// - 引用不存在 → `HARNESS_CONFIG_LOAD{reason:"skill_not_found"}`；
    /// - 存在但非法 → `HARNESS_SKILL_INVALID{reason}`；
    /// - `"*"` = 全部可用的合法 skill（仍受 `MAX_SKILLS` 约束）。
    pub(crate) fn resolve(&self, references: &[String]) -> Result<Vec<SkillEntry>, HarnessError> {
        let mut names: Vec<String> = Vec::new();
        for reference in references {
            if reference == "*" {
                // 冻结语义（REV-097 C1）：`"*"` = 解析**全部**合法 skills；> MAX_SKILLS
                // 不静默截断 → `HARNESS_CONFIG_LOAD{reason:"too_many_skills"}`。
                if self.valid.len() > MAX_SKILLS || !self.overflow.is_empty() {
                    return Err(HarnessError::config_load(
                        "too_many_skills",
                        None,
                        Some("$.skills"),
                        Some(Value::from(format!("<= {MAX_SKILLS} skills"))),
                        Some(Value::from(self.valid.len() + self.overflow.len())),
                        format!(
                            "wildcard '*' resolves more than {MAX_SKILLS} skills; list explicit names instead"
                        ),
                    ));
                }
                for name in self.valid.keys() {
                    if !names.contains(name) {
                        names.push(name.clone());
                    }
                }
                continue;
            }
            if names.iter().any(|name| name == reference) {
                continue;
            }
            // 显式引用超限目录/第 17+ 个合法 skill → `HARNESS_SKILL_INVALID{too_many_skills}`。
            let over_limit_valid = self.valid.len() > MAX_SKILLS
                && !self
                    .valid
                    .keys()
                    .take(MAX_SKILLS)
                    .any(|name| name == reference);
            if self.overflow.contains(reference) || over_limit_valid {
                return Err(HarnessError::skill_invalid(
                    "too_many_skills",
                    reference,
                    format!(
                        "skill '{reference}' exceeds MAX_SKILLS ({MAX_SKILLS}); reference an earlier directory"
                    ),
                ));
            }
            if self.valid.contains_key(reference) {
                names.push(reference.clone());
                continue;
            }
            if let Some(problem) = self.problems.get(reference) {
                return Err(HarnessError::skill_invalid(
                    problem.reason,
                    reference,
                    problem.message.clone(),
                ));
            }
            return Err(HarnessError::config_load(
                "skill_not_found",
                None,
                Some("$.skills"),
                Some(Value::from("existing skill directory name")),
                Some(Value::from(reference.clone())),
                format!("agent references missing skill '{reference}'"),
            ));
        }
        if names.len() > MAX_SKILLS {
            return Err(HarnessError::skill_invalid(
                "too_many_skills",
                references.first().map(String::as_str).unwrap_or(""),
                format!(
                    "agent references {} skills (MAX_SKILLS = {MAX_SKILLS})",
                    names.len()
                ),
            ));
        }
        names.sort();
        Ok(names
            .iter()
            .filter_map(|name| self.valid.get(name).cloned())
            .collect())
    }
}

/// 引用的 skill 名是否**显式**命中某目录（`"*"` 不算引用具体目录，REV-097 C1）。
fn references_name(references: &[String], name: &str) -> bool {
    references.iter().any(|reference| reference == name)
}

/// 隐藏目录 / 依赖目录（DR-009 §1）。
fn is_ignored_dir(name: &str) -> bool {
    name.starts_with('.') || name == "node_modules"
}

/// 加载并校验一个 skill 目录；`Ok(None)` = 无 `SKILL.md`（不是 skill）。
fn load_skill(
    root: &Path,
    skill_dir: &Path,
    dir_name: &str,
) -> Result<Option<SkillEntry>, SkillProblem> {
    let canonical_dir = fs::canonicalize(skill_dir).map_err(|error| SkillProblem {
        reason: "invalid_frontmatter",
        message: format!(
            "cannot canonicalize skill dir '{}': {error}",
            skill_dir.display()
        ),
    })?;
    if !canonical_dir.starts_with(root) {
        return Err(SkillProblem {
            reason: "path_escape",
            message: format!("skill dir '{dir_name}' escapes the skills root"),
        });
    }
    let skill_md = skill_dir.join("SKILL.md");
    let metadata = match fs::metadata(&skill_md) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(SkillProblem {
                reason: "invalid_frontmatter",
                message: format!("cannot read '{dir_name}/SKILL.md': {error}"),
            })
        }
    };
    if !metadata.is_file() {
        return Ok(None);
    }
    if metadata.len() > SKILL_FILE_MAX_BYTES {
        return Err(SkillProblem {
            reason: "too_large",
            message: format!(
                "'{dir_name}/SKILL.md' is {} bytes (limit {SKILL_FILE_MAX_BYTES})",
                metadata.len()
            ),
        });
    }
    let canonical_md = fs::canonicalize(&skill_md).map_err(|error| SkillProblem {
        reason: "invalid_frontmatter",
        message: format!("cannot canonicalize '{dir_name}/SKILL.md': {error}"),
    })?;
    if !canonical_md.starts_with(root) {
        return Err(SkillProblem {
            reason: "path_escape",
            message: format!("'{dir_name}/SKILL.md' resolves outside the skills root"),
        });
    }
    let bytes = fs::read(&skill_md).map_err(|error| SkillProblem {
        reason: "invalid_frontmatter",
        message: format!("cannot read '{dir_name}/SKILL.md': {error}"),
    })?;
    let text = String::from_utf8(bytes).map_err(|_| SkillProblem {
        reason: "not_utf8",
        message: format!("'{dir_name}/SKILL.md' is not valid UTF-8"),
    })?;
    let mut skill = parse_skill_md(dir_name, &text)?;
    let (resources, truncated) = scan_resources(&canonical_dir, root)?;
    skill.resources = resources;
    skill.resources_truncated = truncated;
    Ok(Some(skill))
}

/// 解析 `SKILL.md`（frontmatter + body）；校验顺序见模块注释/DR-009 §2。
fn parse_skill_md(dir_name: &str, text: &str) -> Result<SkillEntry, SkillProblem> {
    let (frontmatter, body) = split_frontmatter(text)?;
    let fields = parse_yaml_subset(frontmatter)?;
    validate_skill_fields(dir_name, &fields, body)
}

/// 切分 YAML frontmatter 与正文。
fn split_frontmatter(text: &str) -> Result<(&str, String), SkillProblem> {
    let normalized = text.strip_prefix('\u{feff}').unwrap_or(text);
    if !normalized.starts_with("---") {
        return Err(SkillProblem {
            reason: "invalid_frontmatter",
            message: "SKILL.md must start with '---' YAML frontmatter".to_string(),
        });
    }
    let rest = normalized[3..]
        .strip_prefix('\r')
        .unwrap_or(&normalized[3..]);
    let rest = rest.strip_prefix('\n').ok_or_else(|| SkillProblem {
        reason: "invalid_frontmatter",
        message: "frontmatter delimiter must be on its own line".to_string(),
    })?;
    let mut offset = 0usize;
    for line in rest.split_inclusive('\n') {
        let trimmed = line.trim_end_matches(['\r', '\n']);
        if trimmed == "---" {
            let frontmatter = &rest[..offset];
            let body_start = offset + line.len();
            let body = rest[body_start..]
                .strip_prefix('\n')
                .unwrap_or(&rest[body_start..])
                .to_string();
            return Ok((frontmatter, body));
        }
        offset += line.len();
    }
    Err(SkillProblem {
        reason: "invalid_frontmatter",
        message: "unterminated YAML frontmatter (missing closing '---')".to_string(),
    })
}

/// YAML 子集值（映射/字符串/布尔/数字/null/数组；不支持锚点/标签/多文档）。
#[derive(Debug, Clone, PartialEq)]
enum YamlValue {
    Null,
    Bool(bool),
    Number(String),
    Str(String),
    List(Vec<YamlValue>),
    Map(BTreeMap<String, YamlValue>),
}

impl YamlValue {
    fn as_str(&self) -> Option<&str> {
        match self {
            Self::Str(value) => Some(value),
            _ => None,
        }
    }
}

/// 解析 YAML 子集（frontmatter 顶层必须是 mapping；缩进仅一层）。
fn parse_yaml_subset(text: &str) -> Result<BTreeMap<String, YamlValue>, SkillProblem> {
    let mut map: BTreeMap<String, YamlValue> = BTreeMap::new();
    let mut lines = text.lines().peekable();
    while let Some(raw_line) = lines.next() {
        let line = raw_line.trim_end_matches('\r');
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if trimmed == "..." {
            return unparseable("multi-document YAML is not supported");
        }
        if line.starts_with(' ') || line.starts_with('\t') {
            return unparseable("unexpected indented line in frontmatter");
        }
        let (key, raw_value) = split_key_value(trimmed)?;
        if map.contains_key(&key) {
            return unparseable(&format!("duplicate frontmatter key '{key}'"));
        }
        let value = strip_comment(raw_value).trim();
        if value.is_empty() {
            let nested = parse_nested_block(&mut lines)?;
            map.insert(key, nested);
            continue;
        }
        map.insert(key, parse_scalar(value)?);
    }
    Ok(map)
}

/// 解析嵌套块（`metadata` 的 `map<string,string>` 或数组）。
fn parse_nested_block<'a>(
    lines: &mut std::iter::Peekable<impl Iterator<Item = &'a str>>,
) -> Result<YamlValue, SkillProblem> {
    let mut map: BTreeMap<String, YamlValue> = BTreeMap::new();
    let mut list: Vec<YamlValue> = Vec::new();
    let mut consumed = 0usize;
    let mut kind: Option<&'static str> = None;
    while let Some(raw_line) = lines.peek() {
        let line = raw_line.trim_end_matches('\r');
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            lines.next();
            consumed += 1;
            continue;
        }
        let indent = line.len() - line.trim_start().len();
        if indent == 0 {
            break;
        }
        let content = trimmed;
        lines.next();
        consumed += 1;
        if let Some(item) = content.strip_prefix("- ") {
            if kind == Some("map") {
                return unparseable("mixed YAML list and mapping in nested block");
            }
            kind = Some("list");
            list.push(parse_scalar(strip_comment(item).trim())?);
            continue;
        }
        if kind == Some("list") {
            return unparseable("mixed YAML list and mapping in nested block");
        }
        kind = Some("map");
        let (key, raw_value) = split_key_value(content)?;
        if map.contains_key(&key) {
            return unparseable(&format!("duplicate nested key '{key}'"));
        }
        map.insert(key, parse_scalar(strip_comment(raw_value).trim())?);
    }
    if consumed == 0 {
        return unparseable("empty nested block");
    }
    match kind {
        Some("list") => Ok(YamlValue::List(list)),
        Some("map") => Ok(YamlValue::Map(map)),
        _ => unparseable("empty nested block"),
    }
}

fn split_key_value(line: &str) -> Result<(String, &str), SkillProblem> {
    let Some(index) = line.find(':') else {
        return unparseable("expected 'key: value' in frontmatter");
    };
    let key = line[..index].trim();
    if key.is_empty()
        || !key
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '$' | '_' | '-'))
    {
        return unparseable("invalid frontmatter key");
    }
    Ok((key.to_string(), &line[index + 1..]))
}

/// 去掉未加引号值的行内注释（` #` 之后）。
fn strip_comment(value: &str) -> &str {
    let value = value.trim_start();
    if value.starts_with('"') || value.starts_with('\'') {
        return value;
    }
    match value.find(" #") {
        Some(index) => &value[..index],
        None => value,
    }
}

fn parse_scalar(value: &str) -> Result<YamlValue, SkillProblem> {
    if value.is_empty() {
        return Ok(YamlValue::Null);
    }
    if value.starts_with('&') || value.starts_with('*') || value.starts_with('!') {
        return unparseable("YAML anchors/tags are not supported");
    }
    if value.starts_with('[') || value.starts_with('{') {
        return unparseable("flow-style YAML collections are not supported");
    }
    if let Some(quoted) = value.strip_prefix('"') {
        let inner = quoted.strip_suffix('"').ok_or_else(|| SkillProblem {
            reason: "unparseable_yaml",
            message: "unterminated double-quoted YAML scalar".to_string(),
        })?;
        return Ok(YamlValue::Str(unescape_double_quoted(inner)?));
    }
    if let Some(quoted) = value.strip_prefix('\'') {
        let inner = quoted.strip_suffix('\'').ok_or_else(|| SkillProblem {
            reason: "unparseable_yaml",
            message: "unterminated single-quoted YAML scalar".to_string(),
        })?;
        return Ok(YamlValue::Str(inner.replace("''", "'")));
    }
    Ok(match value {
        "null" | "~" | "Null" | "NULL" => YamlValue::Null,
        "true" | "True" | "TRUE" => YamlValue::Bool(true),
        "false" | "False" | "FALSE" => YamlValue::Bool(false),
        other if is_number_literal(other) => YamlValue::Number(other.to_string()),
        other => YamlValue::Str(other.to_string()),
    })
}

fn unescape_double_quoted(inner: &str) -> Result<String, SkillProblem> {
    let mut out = String::with_capacity(inner.len());
    let mut chars = inner.chars();
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            out.push(ch);
            continue;
        }
        match chars.next() {
            Some('"') => out.push('"'),
            Some('\\') => out.push('\\'),
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            _ => return unparseable("unsupported escape in YAML scalar"),
        }
    }
    Ok(out)
}

fn is_number_literal(value: &str) -> bool {
    let mut chars = value.chars().peekable();
    if matches!(chars.peek(), Some('-') | Some('+')) {
        chars.next();
    }
    let mut digits = 0usize;
    let mut dots = 0usize;
    for ch in chars {
        match ch {
            '0'..='9' => digits += 1,
            '.' => dots += 1,
            _ => return false,
        }
    }
    digits > 0 && dots <= 1
}

fn unparseable<T>(message: &str) -> Result<T, SkillProblem> {
    Err(SkillProblem {
        reason: "unparseable_yaml",
        message: message.to_string(),
    })
}

/// 字段级校验（name/description/可选字段；未知顶层字段拒绝）。
fn validate_skill_fields(
    dir_name: &str,
    fields: &BTreeMap<String, YamlValue>,
    body: String,
) -> Result<SkillEntry, SkillProblem> {
    const ALLOWED: [&str; 7] = [
        "name",
        "description",
        "license",
        "compatibility",
        "metadata",
        "allowed-tools",
        "$schema",
    ];
    for key in fields.keys() {
        if !ALLOWED.contains(&key.as_str()) {
            return Err(SkillProblem {
                reason: "unknown_field",
                message: format!("unknown SKILL.md frontmatter field '{key}'"),
            });
        }
    }
    let name = required_string(fields, dir_name, "name")?;
    if name.chars().count() > MAX_SKILL_NAME_CHARS {
        return Err(SkillProblem {
            reason: "name_too_long",
            message: format!(
                "skill name '{}' exceeds {MAX_SKILL_NAME_CHARS} characters",
                name
            ),
        });
    }
    if !is_valid_skill_name(name) {
        return Err(SkillProblem {
            reason: "invalid_name",
            message: format!("invalid skill name '{name}' (expected ^[a-z0-9]+(-[a-z0-9]+)*$)"),
        });
    }
    if name != dir_name {
        return Err(SkillProblem {
            reason: "name_mismatch",
            message: format!("skill name '{name}' must equal its directory name '{dir_name}'"),
        });
    }
    let description = match fields.get("description") {
        None | Some(YamlValue::Null) => {
            return Err(SkillProblem {
                reason: "missing_description",
                message: format!("skill '{name}' is missing required 'description'"),
            })
        }
        Some(YamlValue::Str(description)) => description.clone(),
        Some(_) => {
            return Err(SkillProblem {
                reason: "invalid_frontmatter",
                message: format!("'description' of skill '{name}' must be a string"),
            })
        }
    };
    if description.trim().is_empty() {
        return Err(SkillProblem {
            reason: "invalid_frontmatter",
            message: format!("'description' of skill '{name}' must not be empty"),
        });
    }
    if description.chars().count() > MAX_SKILL_DESCRIPTION_CHARS {
        return Err(SkillProblem {
            reason: "description_too_long",
            message: format!(
                "'description' of skill '{name}' has {} characters (limit {MAX_SKILL_DESCRIPTION_CHARS})",
                description.chars().count()
            ),
        });
    }
    let license = optional_string(fields, "license")?;
    let compatibility = optional_string(fields, "compatibility")?;
    if let Some(compatibility) = &compatibility {
        if compatibility.is_empty() || compatibility.chars().count() > MAX_SKILL_COMPATIBILITY_CHARS
        {
            return Err(SkillProblem {
                reason: "invalid_frontmatter",
                message: format!(
                    "'compatibility' of skill '{name}' must be 1-{MAX_SKILL_COMPATIBILITY_CHARS} characters"
                ),
            });
        }
    }
    let allowed_tools = optional_string(fields, "allowed-tools")?;
    let metadata = match fields.get("metadata") {
        None | Some(YamlValue::Null) => BTreeMap::new(),
        Some(YamlValue::Map(map)) => {
            let mut metadata = BTreeMap::new();
            for (key, value) in map {
                let Some(value) = value.as_str() else {
                    return Err(SkillProblem {
                        reason: "invalid_frontmatter",
                        message: format!("'metadata.{key}' of skill '{name}' must be a string"),
                    });
                };
                metadata.insert(key.clone(), value.to_string());
            }
            metadata
        }
        Some(_) => {
            return Err(SkillProblem {
                reason: "invalid_frontmatter",
                message: format!("'metadata' of skill '{name}' must be a mapping"),
            })
        }
    };
    Ok(SkillEntry {
        name: name.to_string(),
        description,
        license,
        compatibility,
        metadata,
        allowed_tools,
        body,
        resources: Vec::new(),
        resources_truncated: false,
    })
}

fn required_string<'a>(
    fields: &'a BTreeMap<String, YamlValue>,
    skill: &str,
    field: &str,
) -> Result<&'a str, SkillProblem> {
    match fields.get(field) {
        Some(YamlValue::Str(value)) => Ok(value),
        None | Some(YamlValue::Null) => Err(SkillProblem {
            reason: "invalid_frontmatter",
            message: format!("skill '{skill}' is missing required string '{field}'"),
        }),
        Some(_) => Err(SkillProblem {
            reason: "invalid_frontmatter",
            message: format!("'{field}' of skill '{skill}' must be a string"),
        }),
    }
}

fn optional_string(
    fields: &BTreeMap<String, YamlValue>,
    field: &str,
) -> Result<Option<String>, SkillProblem> {
    match fields.get(field) {
        None | Some(YamlValue::Null) => Ok(None),
        Some(YamlValue::Str(value)) => Ok(Some(value.clone())),
        Some(_) => Err(SkillProblem {
            reason: "invalid_frontmatter",
            message: format!("'{field}' must be a string"),
        }),
    }
}

/// `^[a-z0-9]+(-[a-z0-9]+)*$`（1..=64 由调用方校验）。
fn is_valid_skill_name(name: &str) -> bool {
    if name.is_empty() {
        return false;
    }
    name.split('-').all(|segment| {
        !segment.is_empty()
            && segment
                .chars()
                .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit())
    })
}

/// 扫描资源文件（相对路径、排序、≤50；只列名不读内容）。
///
/// DR-009 §5 的「单资源 ≤1 MiB」是**读取/注入**预算：v0.3 只列名、不读内容，
/// 因此不对文件大小做过滤（也不会把内容带进上下文）。
/// 单层 skill 目录 + 任意子目录中的常规文件（`SKILL.md` 自身除外）；symlink 不跟随
/// （避免逃逸/环）。条目数 > [`MAX_SKILL_DIR_ENTRIES`] → `too_large`。
fn scan_resources(skill_dir: &Path, root: &Path) -> Result<(Vec<String>, bool), SkillProblem> {
    let mut files: Vec<String> = Vec::new();
    let mut entries_seen = 0usize;
    let mut stack: Vec<(PathBuf, String)> = vec![(skill_dir.to_path_buf(), String::new())];
    let mut too_many_entries = false;
    while let Some((dir, prefix)) = stack.pop() {
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            entries_seen += 1;
            if entries_seen > MAX_SKILL_DIR_ENTRIES {
                too_many_entries = true;
                break;
            }
            let name = entry.file_name().to_string_lossy().to_string();
            let relative = if prefix.is_empty() {
                name.clone()
            } else {
                format!("{prefix}/{name}")
            };
            let path = entry.path();
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            if file_type.is_symlink() {
                continue;
            }
            if file_type.is_dir() {
                stack.push((path, relative));
                continue;
            }
            if !file_type.is_file() || relative == "SKILL.md" {
                continue;
            }
            // 防御：canonicalize 后必须仍在 skills 根内（资源仅列名，不读）。
            if let Ok(canonical) = fs::canonicalize(&path) {
                if !canonical.starts_with(root) {
                    return Err(SkillProblem {
                        reason: "path_escape",
                        message: format!("resource '{relative}' resolves outside the skills root"),
                    });
                }
            }
            files.push(relative);
        }
        if too_many_entries {
            break;
        }
    }
    if too_many_entries {
        return Err(SkillProblem {
            reason: "too_large",
            message: format!("skill directory exceeds {MAX_SKILL_DIR_ENTRIES} entries"),
        });
    }
    files.sort();
    let truncated = files.len() > MAX_SKILL_RESOURCES;
    files.truncate(MAX_SKILL_RESOURCES);
    Ok((files, truncated))
}

/// 构造 system catalog 段（仅 `skills` 非空时调用；name+description，≤4096 字符）。
///
/// 截断语义（DR-009 §4）：**不截断单个 description**（>1024 已在加载期拒绝），
/// 超出预算时省略后续 skill 行并追加 `(catalog truncated: N skills omitted)`。
pub(crate) fn build_catalog(skills: &[SkillEntry]) -> String {
    if skills.is_empty() {
        return String::new();
    }
    let mut skills_sorted: Vec<&SkillEntry> = skills.iter().collect();
    skills_sorted.sort_by(|left, right| left.name.cmp(&right.name));
    let mut out = String::from(
        "## 技能\n以下技能提供专门指令。当任务与某技能描述匹配时，调用 skill_view 加载完整指令，再继续。\n<available_skills>\n",
    );
    let closing = "</available_skills>";
    let mut omitted = 0usize;
    for (index, skill) in skills_sorted.iter().enumerate() {
        let line = format!(
            "  <skill><name>{}</name><description>{}</description></skill>\n",
            escape_xml(&skill.name),
            escape_xml(&skill.description)
        );
        // 预留 64 字符给截断标记，保证最终 catalog 严格 ≤ SKILL_CATALOG_MAX_CHARS。
        if out.chars().count() + line.chars().count() + closing.chars().count() + 64
            > SKILL_CATALOG_MAX_CHARS
        {
            omitted = skills_sorted.len() - index;
            break;
        }
        out.push_str(&line);
    }
    if omitted > 0 {
        out.push_str(&format!("(catalog truncated: {omitted} skills omitted)\n"));
    }
    out.push_str(closing);
    out
}

fn escape_xml(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// 内置 `skill_view(name)` 工具定义（仅 skills 非空时注册）。
pub(crate) fn skill_view_tool(skills: &[SkillEntry]) -> ToolDef {
    let mut names: Vec<Value> = skills
        .iter()
        .map(|skill| Value::from(skill.name.clone()))
        .collect();
    names.sort_by(|left, right| left.as_str().cmp(&right.as_str()));
    ToolDef {
        name: SKILL_VIEW_TOOL_NAME.to_string(),
        description:
            "Load the full instructions of an available skill by name; use when a skill description matches the task."
                .to_string(),
        parameters: json!({
            "type": "object",
            "properties": {
                "name": {"type": "string", "enum": names}
            },
            "required": ["name"],
            "additionalProperties": false
        }),
        timeout_ms: crate::constants::DEFAULT_TOOL_TIMEOUT_MS,
        source: SKILL_VIEW_TOOL_SOURCE.to_string(),
    }
}

/// `skill_view` 工具结果文本：`<skill_content>` + 正文（截断）+ 资源清单。
pub(crate) fn skill_view_text(skill: &SkillEntry) -> String {
    let (body, truncated) = truncate_skill_body(&skill.body);
    let mut out = format!("<skill_content name=\"{}\">\n", skill.name);
    out.push_str(&body);
    if truncated {
        out.push('\n');
    }
    out.push_str(&format!("\nSkill directory: {}\n", skill.name));
    out.push_str("<skill_resources>\n");
    for resource in &skill.resources {
        out.push_str(&format!("  <file>{}</file>\n", escape_xml(resource)));
    }
    if skill.resources_truncated {
        out.push_str("  <!-- truncated -->\n");
    }
    out.push_str("</skill_resources>\n</skill_content>");
    out
}

/// 正文截断（≤32 KiB 字符）：保留首尾、中间标记 `[…truncated K chars…]`。
pub(crate) fn truncate_skill_body(body: &str) -> (String, bool) {
    let chars: Vec<char> = body.chars().collect();
    if chars.len() <= SKILL_BODY_MAX_CHARS {
        return (body.to_string(), false);
    }
    let head_len = SKILL_BODY_MAX_CHARS / 2;
    let tail_len = SKILL_BODY_MAX_CHARS - head_len;
    let omitted = chars.len() - head_len - tail_len;
    let head: String = chars[..head_len].iter().collect();
    let tail: String = chars[chars.len() - tail_len..].iter().collect();
    (
        format!("{head}\n[…truncated {omitted} chars…]\n{tail}"),
        true,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;
    use zgraph_kernel::HarnessErrorKind;

    fn skill_md(name: &str, description: &str) -> String {
        format!("---\nname: {name}\ndescription: {description}\n---\n\n# {name}\n\nbody text\n")
    }

    fn write_skill(root: &Path, dir_name: &str, content: &str) {
        let dir = root.join(dir_name);
        fs::create_dir_all(&dir).expect("skill dir");
        fs::write(dir.join("SKILL.md"), content).expect("SKILL.md");
    }

    fn parse(name: &str, content: &str) -> Result<SkillEntry, SkillProblem> {
        parse_skill_md(name, content)
    }

    fn entry(name: &str, description: &str, body: &str) -> SkillEntry {
        SkillEntry {
            name: name.to_string(),
            description: description.to_string(),
            license: None,
            compatibility: None,
            metadata: BTreeMap::new(),
            allowed_tools: None,
            body: body.to_string(),
            resources: Vec::new(),
            resources_truncated: false,
        }
    }

    fn reason(result: Result<SkillEntry, SkillProblem>) -> &'static str {
        result.expect_err("expected invalid skill").reason
    }

    #[test]
    fn parses_full_frontmatter_and_body() {
        let text = "\u{feff}---\nname: code-review\ndescription: Review a patch\nlicense: MIT\ncompatibility: Requires git\nmetadata:\n  version: \"1.0\"\n  owner: team\nallowed-tools: read_file\n$schema: https://example/schema\n---\n\n# Code Review\nsteps\n";
        let skill = parse("code-review", text).expect("valid skill");
        assert_eq!(skill.name, "code-review");
        assert_eq!(skill.description, "Review a patch");
        assert_eq!(skill.license.as_deref(), Some("MIT"));
        assert_eq!(skill.compatibility.as_deref(), Some("Requires git"));
        assert_eq!(
            skill.metadata.get("version").map(String::as_str),
            Some("1.0")
        );
        assert_eq!(
            skill.metadata.get("owner").map(String::as_str),
            Some("team")
        );
        assert_eq!(skill.allowed_tools.as_deref(), Some("read_file"));
        assert_eq!(skill.body, "# Code Review\nsteps\n");
    }

    #[test]
    fn rejects_missing_or_unterminated_frontmatter() {
        assert_eq!(
            reason(parse("a", "# no frontmatter\n")),
            "invalid_frontmatter"
        );
        assert_eq!(
            reason(parse("a", "---\nname: a\ndescription: d\n")),
            "invalid_frontmatter"
        );
        assert_eq!(reason(parse("a", "---\n")), "invalid_frontmatter");
    }

    #[test]
    fn rejects_unknown_field_and_bad_yaml() {
        let text = "---\nname: a\ndescription: d\nbogus: 1\n---\n";
        assert_eq!(reason(parse("a", text)), "unknown_field");
        let text = "---\nname: a\ndescription: d\nbad line without colon\n---\n";
        assert_eq!(reason(parse("a", text)), "unparseable_yaml");
        let text = "---\nname: a\ndescription: d\n< &anchor\n---\n";
        assert_eq!(reason(parse("a", text)), "unparseable_yaml");
        let text = "---\nname: a\nname: b\ndescription: d\n---\n";
        assert_eq!(reason(parse("a", text)), "unparseable_yaml");
    }

    #[test]
    fn validates_name() {
        assert_eq!(
            reason(parse("a", "---\nname: A\ndescription: d\n---\n")),
            "invalid_name"
        );
        assert_eq!(
            reason(parse("a--b", "---\nname: a--b\ndescription: d\n---\n")),
            "invalid_name"
        );
        assert_eq!(
            reason(parse("a", "---\nname: -a\ndescription: d\n---\n")),
            "invalid_name"
        );
        assert_eq!(
            reason(parse("dir", "---\nname: other\ndescription: d\n---\n")),
            "name_mismatch"
        );
        let long = "a".repeat(65);
        assert_eq!(
            reason(parse(
                &long,
                &format!("---\nname: {long}\ndescription: d\n---\n")
            )),
            "name_too_long"
        );
        assert_eq!(
            reason(parse("a", "---\nname: a\n---\n")),
            "missing_description"
        );
    }

    #[test]
    fn validates_description() {
        assert_eq!(
            reason(parse("a", "---\nname: a\n---\n")),
            "missing_description"
        );
        assert_eq!(
            reason(parse("a", "---\nname: a\ndescription: true\n---\n")),
            "invalid_frontmatter"
        );
        assert_eq!(
            reason(parse("a", "---\nname: a\ndescription: \"   \"\n---\n")),
            "invalid_frontmatter"
        );
        let long = "d".repeat(MAX_SKILL_DESCRIPTION_CHARS + 1);
        assert_eq!(
            reason(parse(
                "a",
                &format!("---\nname: a\ndescription: {long}\n---\n")
            )),
            "description_too_long"
        );
    }

    #[test]
    fn validates_optional_fields() {
        assert_eq!(
            reason(parse(
                "a",
                "---\nname: a\ndescription: d\nlicense: 1\n---\n"
            )),
            "invalid_frontmatter"
        );
        let long = "c".repeat(MAX_SKILL_COMPATIBILITY_CHARS + 1);
        assert_eq!(
            reason(parse(
                "a",
                &format!("---\nname: a\ndescription: d\ncompatibility: {long}\n---\n")
            )),
            "invalid_frontmatter"
        );
        assert_eq!(
            reason(parse(
                "a",
                "---\nname: a\ndescription: d\nmetadata: [1, 2]\n---\n"
            )),
            "unparseable_yaml"
        );
        assert_eq!(
            reason(parse(
                "a",
                "---\nname: a\ndescription: d\nmetadata:\n  k: 1\n---\n"
            )),
            "invalid_frontmatter"
        );
        assert_eq!(
            reason(parse(
                "a",
                "---\nname: a\ndescription: d\nallowed-tools: [read_file]\n---\n"
            )),
            "unparseable_yaml"
        );
    }

    #[test]
    fn discovery_sorts_and_skips_non_skills() {
        let dir = TempDir::new().expect("tempdir");
        write_skill(dir.path(), "zeta", &skill_md("zeta", "Z skill"));
        write_skill(dir.path(), "alpha", &skill_md("alpha", "A skill"));
        fs::create_dir_all(dir.path().join(".git")).expect(".git");
        fs::create_dir_all(dir.path().join("node_modules")).expect("node_modules");
        fs::create_dir_all(dir.path().join("not-a-skill")).expect("plain dir");
        fs::write(dir.path().join("not-a-skill/README.md"), "x").expect("file");
        let index = SkillsIndex::discover(dir.path());
        let names: Vec<&str> = index.valid.keys().map(String::as_str).collect();
        assert_eq!(names, vec!["alpha", "zeta"]);
        assert!(index.problems.is_empty());
        assert!(index.warnings().is_empty());
    }

    #[test]
    fn discovery_reports_invalid_and_resolve_maps_errors() {
        let dir = TempDir::new().expect("tempdir");
        write_skill(dir.path(), "ok", &skill_md("ok", "ok skill"));
        write_skill(dir.path(), "bad", "---\nname: bad\n---\n");
        let index = SkillsIndex::discover(dir.path());
        assert_eq!(index.valid.len(), 1);
        assert_eq!(index.problems["bad"].reason, "missing_description");

        // 被引用但非法 → HARNESS_SKILL_INVALID{reason}。
        let error = index.resolve(&["bad".to_string()]).expect_err("invalid");
        assert_eq!(error.kind, HarnessErrorKind::SkillInvalid);
        assert_eq!(error.code, "HARNESS_SKILL_INVALID");
        assert_eq!(error.detail["reason"], "missing_description");
        // 未被引用 → 跳过 + warn。
        let warnings = index.skip_warnings(&["ok".to_string()]);
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("bad"));
        // 缺失引用 → HARNESS_CONFIG_LOAD{skill_not_found}。
        let error = index.resolve(&["ghost".to_string()]).expect_err("missing");
        assert_eq!(error.kind, HarnessErrorKind::ConfigLoad);
        assert_eq!(error.detail["reason"], "skill_not_found");
        // "*" → 全部合法；
        let resolved = index.resolve(&["*".to_string()]).expect("all");
        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].name, "ok");
    }

    #[test]
    fn discovery_over_limit_wildcard_is_config_load_too_many_skills() {
        let dir = TempDir::new().expect("tempdir");
        for index in 0..(MAX_SKILLS + 2) {
            let name = format!("skill-{index:02}");
            write_skill(dir.path(), &name, &skill_md(&name, "desc"));
        }
        let index = SkillsIndex::discover(dir.path());
        assert_eq!(
            index.valid.len(),
            MAX_SKILLS + 1,
            "保留 17 个探测超限，不静默截断到 16"
        );

        // "*" → HARNESS_CONFIG_LOAD{reason:"too_many_skills"}。
        let error = index
            .resolve(&["*".to_string()])
            .expect_err("wildcard over limit");
        assert_eq!(error.kind, HarnessErrorKind::ConfigLoad);
        assert_eq!(error.detail["reason"], "too_many_skills");

        // 显式引用超限目录（第 18 个，overflow）→ HARNESS_SKILL_INVALID{too_many_skills}。
        let error = index
            .resolve(&["skill-17".to_string()])
            .expect_err("overflow explicit ref");
        assert_eq!(error.kind, HarnessErrorKind::SkillInvalid);
        assert_eq!(error.detail["reason"], "too_many_skills");

        // 显式引用前 16 个之一仍可用；额外目录 warn 可见（"*" 不算显式引用）。
        assert_eq!(
            index
                .resolve(&["skill-00".to_string()])
                .expect("allowed")
                .len(),
            1
        );
        let warnings = index.skip_warnings(&["*".to_string()]);
        assert!(
            warnings
                .iter()
                .any(|warning| warning.contains("skill-17")
                    && warning.contains("exceeds MAX_SKILLS")),
            "overflow dir warn visible: {warnings:?}"
        );
    }

    /// REV-097 C1：`"*"` 不得把非法目录当「已引用」而吞掉 warn；非法目录仍跳过+warn。
    #[test]
    fn wildcard_does_not_suppress_invalid_dir_warnings() {
        let dir = TempDir::new().expect("tempdir");
        write_skill(dir.path(), "ok", &skill_md("ok", "good"));
        write_skill(dir.path(), "bad", "---\nname: bad\n---\n");
        let index = SkillsIndex::discover(dir.path());
        let warnings = index.skip_warnings(&["*".to_string()]);
        assert_eq!(warnings.len(), 1);
        assert!(
            warnings[0].contains("bad"),
            "invalid dir warn: {warnings:?}"
        );
        // "*" 只解析合法 skills（非法目录未引用 → 跳过，不报错）。
        let resolved = index.resolve(&["*".to_string()]).expect("wildcard");
        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].name, "ok");
    }

    #[test]
    fn discovery_rejects_too_large_and_non_utf8() {
        let dir = TempDir::new().expect("tempdir");
        let big = "x".repeat(SKILL_FILE_MAX_BYTES as usize + 1);
        write_skill(dir.path(), "big", &big);
        write_skill(
            dir.path(),
            "bad-utf8",
            "---\nname: bad-utf8\ndescription: d\n---\n",
        );
        fs::write(
            dir.path().join("bad-utf8/SKILL.md"),
            [0x2d, 0x2d, 0x2d, 0xff, 0xfe],
        )
        .expect("binary");
        let index = SkillsIndex::discover(dir.path());
        assert_eq!(index.problems["big"].reason, "too_large");
        assert_eq!(index.problems["bad-utf8"].reason, "not_utf8");
    }

    #[cfg(unix)]
    #[test]
    fn discovery_rejects_symlink_escape() {
        use std::os::unix::fs::symlink;
        let outside = TempDir::new().expect("outside");
        write_skill(outside.path(), "escape", &skill_md("escape", "d"));
        let dir = TempDir::new().expect("tempdir");
        symlink(outside.path().join("escape"), dir.path().join("escape")).expect("symlink");
        let index = SkillsIndex::discover(dir.path());
        assert_eq!(index.problems["escape"].reason, "path_escape");
        assert!(index.resolve(&["escape".to_string()]).is_err());
    }

    #[test]
    fn lists_resources_sorted_and_truncated() {
        let dir = TempDir::new().expect("tempdir");
        write_skill(dir.path(), "res", &skill_md("res", "d"));
        let skill_dir = dir.path().join("res");
        fs::create_dir_all(skill_dir.join("references")).expect("refs");
        fs::write(skill_dir.join("references/REFERENCE.md"), "ref").expect("ref");
        fs::create_dir_all(skill_dir.join("scripts")).expect("scripts");
        fs::write(skill_dir.join("scripts/run.py"), "print(1)").expect("script");
        let index = SkillsIndex::discover(dir.path());
        let skill = &index.valid["res"];
        assert_eq!(
            skill.resources,
            vec!["references/REFERENCE.md", "scripts/run.py"]
        );
        assert!(!skill.resources_truncated);

        let many = TempDir::new().expect("many");
        write_skill(many.path(), "many", &skill_md("many", "d"));
        for index in 0..(MAX_SKILL_RESOURCES + 3) {
            fs::write(
                many.path()
                    .join("many")
                    .join(format!("file-{index:03}.txt")),
                "x",
            )
            .expect("file");
        }
        let index = SkillsIndex::discover(many.path());
        let skill = &index.valid["many"];
        assert_eq!(skill.resources.len(), MAX_SKILL_RESOURCES);
        assert!(skill.resources_truncated);
    }

    #[test]
    fn catalog_sorted_bounded_and_escapes_xml() {
        let skills = vec![
            entry("zeta", "last & <safe>", ""),
            entry("alpha", "first", ""),
        ];
        let catalog = build_catalog(&skills);
        let alpha = catalog.find("alpha").expect("alpha");
        let zeta = catalog.find("zeta").expect("zeta");
        assert!(alpha < zeta, "catalog is name-sorted");
        assert!(catalog.contains("&amp; &lt;safe&gt;"));
        assert!(catalog.contains("<available_skills>"));
        assert!(catalog.chars().count() <= SKILL_CATALOG_MAX_CHARS);
        assert!(!catalog.contains("catalog truncated"));
    }

    #[test]
    fn catalog_truncates_with_marker_when_over_budget() {
        let long = "d".repeat(MAX_SKILL_DESCRIPTION_CHARS);
        let skills: Vec<SkillEntry> = (0..MAX_SKILLS)
            .map(|index| entry(&format!("skill-{index:02}"), &long, ""))
            .collect();
        let catalog = build_catalog(&skills);
        assert!(catalog.chars().count() <= SKILL_CATALOG_MAX_CHARS);
        assert!(catalog.contains("(catalog truncated:"));
        assert!(catalog.contains("skills omitted)"));
    }

    #[test]
    fn skill_view_text_includes_body_and_resources() {
        let mut skill = entry("pdf-processing", "process pdfs", "step 1\nstep 2");
        skill.resources = vec!["references/REFERENCE.md".to_string()];
        let text = skill_view_text(&skill);
        assert!(text.starts_with("<skill_content name=\"pdf-processing\">"));
        assert!(text.contains("step 1\nstep 2"));
        assert!(text.contains("<file>references/REFERENCE.md</file>"));
        assert!(!text.contains("/tmp"), "must not expose absolute paths");
    }

    #[test]
    fn skill_body_truncates_head_and_tail() {
        let body = "a".repeat(SKILL_BODY_MAX_CHARS);
        let (text, truncated) = truncate_skill_body(&body);
        assert!(!truncated);
        assert_eq!(text, body);

        let body = format!("{}{}", "h".repeat(20_000), "t".repeat(20_000));
        let (text, truncated) = truncate_skill_body(&body);
        assert!(truncated);
        assert!(text.contains("[…truncated"));
        assert!(text.starts_with('h'));
        assert!(text.ends_with('t'));
        assert!(text.chars().count() <= SKILL_BODY_MAX_CHARS + 64);
    }

    #[test]
    fn skill_view_tool_schema_enum_and_no_absolute_paths() {
        let skills = vec![entry("b-skill", "b", ""), entry("a-skill", "a", "")];
        let tool = skill_view_tool(&skills);
        assert_eq!(tool.name, SKILL_VIEW_TOOL_NAME);
        let enum_values = tool.parameters["properties"]["name"]["enum"]
            .as_array()
            .expect("enum");
        let names: Vec<&str> = enum_values.iter().filter_map(Value::as_str).collect();
        assert_eq!(names, vec!["a-skill", "b-skill"], "enum sorted");
        assert_eq!(tool.parameters["additionalProperties"], json!(false));
    }

    #[test]
    fn missing_skills_dir_is_empty_index() {
        let dir = TempDir::new().expect("tempdir");
        let index = SkillsIndex::discover(&dir.path().join("nope"));
        assert!(index.valid.is_empty() && index.problems.is_empty());
        assert!(index.resolve(&["x".to_string()]).is_err());
    }
}
