//! 内置搜索工具：`builtin__grep` / `builtin__glob`（DR-013 §3.6/§3.7/§8；v0.4/D3）。
//!
//! - **grep**：`regex`（线性时间；`fixed_strings` 走字面转义）逐行匹配；`ignore`
//!   遍历（尊重 `.gitignore`、默认跳过隐藏；NUL 探测跳过二进制）；单文件 ≤2 MiB；
//!   结果 `{matches:[{path,line,column,text}], total, truncated}`，(path,line,column)
//!   确定排序；≤1000 条且 ≤32 KiB。
//! - **glob**：`globset` 匹配、`ignore` 遍历（`include_hidden`）、只返回文件、排序确定；
//!   默认 1000 / 上限 5000、结果 ≤32 KiB。
//!
//! 路径 jail 与 read 一致：workspace + `--allow-path`（只读）可搜索，越界拒绝；
//! 搜索遍历的每个文件都做 canonicalize + 前缀校验（symlink 出 jail 不会命中）。

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use globset::Glob;
use ignore::WalkBuilder;
use regex::RegexBuilder;
use serde_json::{json, Value};

use crate::builtin::files::{
    optional_bool, optional_u64, required_str, truncate_utf8, BuiltinToolSet,
};
use crate::builtin::jail::{Access, ResolvedPath};
use crate::constants::{
    BUILTIN_GLOB_DEFAULT_RESULTS, BUILTIN_GLOB_MAX_RESULTS, BUILTIN_GREP_DEFAULT_RESULTS,
    BUILTIN_SEARCH_MAX_RESULTS, BUILTIN_SEARCH_MAX_SCAN_BYTES, BUILTIN_SEARCH_TEXT_MAX_BYTES,
    BUILTIN_TOOL_TIMEOUT_MS,
};
use crate::tools::{ToolDef, ToolFailure};

/// grep 工具名（冻结；DR-013 §3.6）。
pub(crate) const TOOL_GREP: &str = "builtin__grep";

/// glob 工具名（冻结；DR-013 §3.7）。
pub(crate) const TOOL_GLOB: &str = "builtin__glob";

/// 搜索工具定义（与 D2 文件工具一起注册；顺序固定：grep、glob）。
pub(crate) fn search_tool_defs() -> Vec<ToolDef> {
    vec![
        ToolDef {
            name: TOOL_GREP.to_string(),
            description: "Search file contents with a regex/fixed string (respects .gitignore)."
                .to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "pattern": {"type": "string", "minLength": 1},
                    "path": {"type": "string", "default": "."},
                    "glob": {"type": "string", "description": "optional glob filter, e.g. **/*.rs"},
                    "fixed_strings": {"type": "boolean", "default": false},
                    "case_sensitive": {"type": "boolean", "default": true},
                    "max_results": {"type": "integer", "minimum": 1,
                                    "maximum": BUILTIN_SEARCH_MAX_RESULTS,
                                    "default": BUILTIN_GREP_DEFAULT_RESULTS}
                },
                "required": ["pattern"],
                "additionalProperties": false
            }),
            timeout_ms: BUILTIN_TOOL_TIMEOUT_MS,
            source: "builtin__search".to_string(),
        },
        ToolDef {
            name: TOOL_GLOB.to_string(),
            description: "List files in the workspace matching a glob pattern (deterministic)."
                .to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "pattern": {"type": "string", "minLength": 1},
                    "path": {"type": "string", "default": "."},
                    "include_hidden": {"type": "boolean", "default": false},
                    "max_results": {"type": "integer", "minimum": 1,
                                    "maximum": BUILTIN_GLOB_MAX_RESULTS,
                                    "default": BUILTIN_GLOB_DEFAULT_RESULTS}
                },
                "required": ["pattern"],
                "additionalProperties": false
            }),
            timeout_ms: BUILTIN_TOOL_TIMEOUT_MS,
            source: "builtin__search".to_string(),
        },
    ]
}

/// 遍历候选文件（jail 内、去重、按展示路径排序）。
struct Candidate {
    canonical: PathBuf,
    display: String,
}

impl BuiltinToolSet {
    /// `builtin__grep`：逐行 regex/fixed 匹配（详见模块文档）。
    pub(crate) fn grep(&self, args: &Value) -> Result<Value, ToolFailure> {
        let pattern = required_str(args, "pattern")?;
        if pattern.is_empty() {
            return Err(ToolFailure::new(
                "invalid_args",
                "pattern must be non-empty",
            ));
        }
        let path = args.get("path").and_then(Value::as_str).unwrap_or(".");
        let fixed_strings = optional_bool(args, "fixed_strings", false)?;
        let case_sensitive = optional_bool(args, "case_sensitive", true)?;
        let max_results =
            optional_u64(args, "max_results", BUILTIN_GREP_DEFAULT_RESULTS as u64)? as usize;
        if max_results == 0 || max_results > BUILTIN_SEARCH_MAX_RESULTS {
            return Err(ToolFailure::new(
                "invalid_args",
                format!("max_results must be in 1..={BUILTIN_SEARCH_MAX_RESULTS}"),
            ));
        }
        let expression = if fixed_strings {
            regex::escape(pattern)
        } else {
            pattern.to_string()
        };
        let regex = RegexBuilder::new(&expression)
            .case_insensitive(!case_sensitive)
            .build()
            .map_err(|error| {
                ToolFailure::new(
                    "invalid_regex",
                    format!("invalid regex '{pattern}': {error}"),
                )
            })?;
        let glob_matcher = match args.get("glob").and_then(Value::as_str) {
            None => None,
            Some(glob) => Some(Glob::new(glob).map_err(|error| {
                ToolFailure::new("invalid_glob", format!("invalid glob '{glob}': {error}"))
            })?),
        };
        let root = self.jail.resolve_any(path, Access::Read)?;
        // grep：隐藏文件始终跳过（DR-013 §3.6）；尊重 .gitignore。
        let candidates = self.search_candidates(&root, false, true)?;
        let mut matches: Vec<Value> = Vec::new();
        let mut truncated = false;
        'files: for candidate in candidates {
            if let Some(glob) = &glob_matcher {
                if !glob.compile_matcher().is_match(&candidate.display) {
                    continue;
                }
            }
            let Ok(metadata) = std::fs::metadata(&candidate.canonical) else {
                continue;
            };
            if metadata.len() > BUILTIN_SEARCH_MAX_SCAN_BYTES {
                continue;
            }
            let Ok(bytes) = std::fs::read(&candidate.canonical) else {
                continue;
            };
            if bytes
                .iter()
                .take(crate::constants::BUILTIN_READ_BINARY_PROBE_BYTES)
                .any(|byte| *byte == 0)
            {
                continue;
            }
            let Ok(text) = String::from_utf8(bytes) else {
                continue;
            };
            for (line_index, line) in text.lines().enumerate() {
                for found in regex.find_iter(line) {
                    let column = line[..found.start()].chars().count() + 1;
                    let (text, _) = truncate_utf8(line, BUILTIN_SEARCH_TEXT_MAX_BYTES);
                    matches.push(json!({
                        "path": candidate.display,
                        "line": line_index + 1,
                        "column": column,
                        "text": text,
                    }));
                    if matches.len() >= max_results {
                        truncated = true;
                        break 'files;
                    }
                }
            }
        }
        Ok(json!({
            "matches": matches,
            "total": matches.len(),
            "truncated": truncated,
        }))
    }

    /// `builtin__glob`：globset 匹配（只文件、排序确定）。
    pub(crate) fn glob(&self, args: &Value) -> Result<Value, ToolFailure> {
        let pattern = required_str(args, "pattern")?;
        if pattern.is_empty() {
            return Err(ToolFailure::new(
                "invalid_args",
                "pattern must be non-empty",
            ));
        }
        let path = args.get("path").and_then(Value::as_str).unwrap_or(".");
        let include_hidden = optional_bool(args, "include_hidden", false)?;
        let max_results =
            optional_u64(args, "max_results", BUILTIN_GLOB_DEFAULT_RESULTS as u64)? as usize;
        if max_results == 0 || max_results > BUILTIN_GLOB_MAX_RESULTS {
            return Err(ToolFailure::new(
                "invalid_args",
                format!("max_results must be in 1..={BUILTIN_GLOB_MAX_RESULTS}"),
            ));
        }
        let glob = Glob::new(pattern).map_err(|error| {
            ToolFailure::new("invalid_glob", format!("invalid glob '{pattern}': {error}"))
        })?;
        let matcher = glob.compile_matcher();
        let root = self.jail.resolve_any(path, Access::Read)?;
        let candidates = self.search_candidates(&root, include_hidden, false)?;
        let mut matched: Vec<String> = candidates
            .into_iter()
            .map(|candidate| candidate.display)
            .filter(|display| matcher.is_match(display))
            .collect();
        matched.sort();
        let total = matched.len();
        let truncated = total > max_results;
        if matched.len() > max_results {
            matched.truncate(max_results);
        }
        Ok(json!({
            "paths": matched,
            "total": total,
            "truncated": truncated,
        }))
    }

    /// 遍历只读根（workspace/allow-path）内的普通文件；每个条目 canonicalize + jail
    /// 前缀校验（symlink 出 jail 不命中），按展示路径排序、canonical 去重。
    fn search_candidates(
        &self,
        root: &ResolvedPath,
        include_hidden: bool,
        gitignore: bool,
    ) -> Result<Vec<Candidate>, ToolFailure> {
        let scope = match &root.is_dir {
            true => {
                let root_path = root.path.clone();
                let mut candidates = self.walk_candidates(&root_path, include_hidden, gitignore)?;
                candidates.sort_by(|left, right| left.display.cmp(&right.display));
                candidates.dedup_by(|left, right| left.canonical == right.canonical);
                return Ok(candidates);
            }
            false => self.jail.read_scope_of(&root.path),
        };
        if scope.is_none() {
            return Err(ToolFailure::new(
                "path_escape",
                "search root escapes the workspace jail",
            ));
        }
        // 单文件搜索：展示路径相对 jail 根。
        let base = self
            .jail
            .read_scope_root(&root.path)
            .map(Path::to_path_buf)
            .unwrap_or_else(|| root.path.clone());
        Ok(vec![Candidate {
            canonical: root.path.clone(),
            display: root.display_relative(&base),
        }])
    }

    /// 实际遍历（ignore crate；`.git` 递归目录始终跳过）。
    fn walk_candidates(
        &self,
        root: &Path,
        include_hidden: bool,
        gitignore: bool,
    ) -> Result<Vec<Candidate>, ToolFailure> {
        let mut builder = WalkBuilder::new(root);
        builder
            .hidden(!include_hidden)
            .follow_links(false)
            .filter_entry(|entry| entry.file_name() != ".git");
        if gitignore {
            builder
                .git_ignore(true)
                .git_global(true)
                .git_exclude(true)
                .parents(true)
                .require_git(false);
        } else {
            builder
                .git_ignore(false)
                .git_global(false)
                .git_exclude(false)
                .ignore(false)
                .parents(false);
        }
        let mut by_canonical: BTreeMap<PathBuf, String> = BTreeMap::new();
        for entry in builder.build() {
            let Ok(entry) = entry else {
                continue;
            };
            let Ok(canonical) = std::fs::canonicalize(entry.path()) else {
                continue;
            };
            if self.jail.read_scope_of(&canonical).is_none() {
                continue;
            }
            let Ok(metadata) = std::fs::metadata(&canonical) else {
                continue;
            };
            if !metadata.is_file() {
                continue;
            }
            let base = self
                .jail
                .read_scope_root(&canonical)
                .map(Path::to_path_buf)
                .unwrap_or_else(|| canonical.clone());
            let display = display_relative(&canonical, &base);
            by_canonical.insert(canonical, display);
        }
        Ok(by_canonical
            .into_iter()
            .map(|(canonical, display)| Candidate { canonical, display })
            .collect())
    }
}

/// 相对 `base` 的 POSIX 风格展示路径。
fn display_relative(path: &Path, base: &Path) -> String {
    path.strip_prefix(base)
        .unwrap_or(path)
        .components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use tempfile::TempDir;

    fn set() -> (TempDir, Arc<BuiltinToolSet>) {
        let workspace = TempDir::new().expect("workspace");
        let jail = std::fs::canonicalize(workspace.path()).expect("canonical");
        let set = BuiltinToolSet::new(&jail, &[]).expect("set");
        (workspace, set)
    }

    fn write_file(root: &Path, relative: &str, content: &str) {
        let path = root.join(relative);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("parent");
        }
        std::fs::write(path, content).expect("write");
    }

    fn call_ok(set: &BuiltinToolSet, tool: &str, args: Value) -> Value {
        let text = set.call(tool, &args).expect("tool ok");
        assert!(
            text.len() <= crate::constants::BUILTIN_RESULT_MAX_BYTES,
            "result exceeds 32 KiB: {}",
            text.len()
        );
        serde_json::from_str(&text).expect("result JSON")
    }

    fn call_err(set: &BuiltinToolSet, tool: &str, args: Value) -> ToolFailure {
        set.call(tool, &args).expect_err("tool error")
    }

    #[test]
    fn tool_defs_include_file_and_search_tools() {
        let (_workspace, set) = set();
        let names: Vec<&str> = set
            .definitions()
            .iter()
            .map(|tool| tool.name.as_str())
            .collect();
        assert_eq!(names.len(), 7, "{names:?}");
        for expected in [TOOL_GREP, TOOL_GLOB] {
            assert!(names.contains(&expected), "{expected} missing: {names:?}");
        }
        for expected in [
            "builtin__read",
            "builtin__write",
            "builtin__edit",
            "builtin__apply_patch",
            "builtin__diff",
        ] {
            assert!(names.contains(&expected), "{expected} missing: {names:?}");
        }
    }

    #[test]
    fn grep_reports_path_line_column_and_sorts() {
        let (workspace, set) = set();
        write_file(
            workspace.path(),
            "src/a.rs",
            "fn alpha() {}\nlet beta = 1;\nfn gamma() {}",
        );
        write_file(workspace.path(), "src/b.rs", "alpha again\n");
        let payload = call_ok(&set, TOOL_GREP, json!({"pattern": "alpha", "path": "."}));
        let matches = payload["matches"].as_array().expect("matches");
        assert_eq!(payload["total"], json!(2));
        assert_eq!(payload["truncated"], json!(false));
        assert_eq!(matches.len(), 2);
        assert_eq!(matches[0]["path"], json!("src/a.rs"));
        assert_eq!(matches[0]["line"], json!(1));
        assert_eq!(matches[0]["column"], json!(4));
        assert_eq!(matches[1]["path"], json!("src/b.rs"));
        assert_eq!(matches[1]["line"], json!(1));
        assert_eq!(matches[1]["column"], json!(1));
    }

    #[test]
    fn grep_supports_fixed_strings_case_and_regex() {
        let (workspace, set) = set();
        write_file(workspace.path(), "data.txt", "A.B\naxb\nAlpha\nalpha\n");
        // regex（`.` 通配）：A.B + axb；默认大小写敏感不命中 Alpha/alpha。
        let regex = call_ok(
            &set,
            TOOL_GREP,
            json!({"pattern": "a.b", "case_sensitive": false}),
        );
        assert_eq!(regex["total"], json!(2), "{regex}");
        // fixed_strings：只命中字面 "A.B"。
        let fixed = call_ok(
            &set,
            TOOL_GREP,
            json!({"pattern": "A.B", "fixed_strings": true}),
        );
        assert_eq!(fixed["total"], json!(1), "{fixed}");
        assert_eq!(fixed["matches"][0]["text"], json!("A.B"));
    }

    #[test]
    fn grep_filters_by_glob_suffix() {
        let (workspace, set) = set();
        write_file(workspace.path(), "a.rs", "needle\n");
        write_file(workspace.path(), "b.txt", "needle\n");
        write_file(workspace.path(), "sub/c.rs", "needle\n");
        let payload = call_ok(
            &set,
            TOOL_GREP,
            json!({"pattern": "needle", "glob": "**/*.rs"}),
        );
        let paths: Vec<&str> = payload["matches"]
            .as_array()
            .expect("matches")
            .iter()
            .map(|item| item["path"].as_str().expect("path"))
            .collect();
        assert_eq!(paths, vec!["a.rs", "sub/c.rs"], "{payload}");
    }

    #[test]
    fn grep_respects_gitignore_and_skips_hidden_binary_and_large_files() {
        let (workspace, set) = set();
        write_file(workspace.path(), ".gitignore", "ignored.txt\n");
        write_file(workspace.path(), "ignored.txt", "needle\n");
        write_file(workspace.path(), "included.txt", "needle\n");
        write_file(workspace.path(), ".hidden.txt", "needle\n");
        std::fs::write(workspace.path().join("binary.bin"), b"needle\0tail").expect("binary");
        let mut large = String::with_capacity(2_100_000);
        large.push_str("needle\n");
        while large.len() < 2_100_000 {
            large.push_str("padding line\n");
        }
        std::fs::write(workspace.path().join("large.txt"), large).expect("large");
        let payload = call_ok(&set, TOOL_GREP, json!({"pattern": "needle", "path": "."}));
        let paths: Vec<&str> = payload["matches"]
            .as_array()
            .expect("matches")
            .iter()
            .map(|item| item["path"].as_str().expect("path"))
            .collect();
        assert_eq!(paths, vec!["included.txt"], "{payload}");
    }

    #[test]
    fn grep_honours_max_results_and_truncation() {
        let (workspace, set) = set();
        write_file(workspace.path(), "many.txt", "hit\nhit\nhit\nhit\nhit\n");
        let payload = call_ok(&set, TOOL_GREP, json!({"pattern": "hit", "max_results": 2}));
        assert_eq!(payload["matches"].as_array().expect("matches").len(), 2);
        assert_eq!(payload["total"], json!(2));
        assert_eq!(payload["truncated"], json!(true));
        let error = call_err(
            &set,
            TOOL_GREP,
            json!({"pattern": "hit", "max_results": 1001}),
        );
        assert_eq!(error.reason, "invalid_args");
    }

    #[test]
    fn grep_result_is_capped_at_32_kib() {
        let (workspace, set) = set();
        let lines: Vec<String> = (0..400)
            .map(|index| format!("needle-{index}-{}", "x".repeat(300)))
            .collect();
        write_file(workspace.path(), "wide.txt", &lines.join("\n"));
        let payload = call_ok(&set, TOOL_GREP, json!({"pattern": "needle", "path": "."}));
        assert_eq!(payload["truncated"], json!(true), "{payload}");
        let serialized = serde_json::to_string(&payload).expect("json");
        assert!(serialized.len() <= crate::constants::BUILTIN_RESULT_MAX_BYTES);
    }

    #[test]
    fn grep_invalid_patterns_and_escape_are_rejected() {
        let (workspace, set) = set();
        write_file(workspace.path(), "a.txt", "x\n");
        assert_eq!(
            call_err(&set, TOOL_GREP, json!({"pattern": "(" })).reason,
            "invalid_regex"
        );
        assert_eq!(
            call_err(&set, TOOL_GREP, json!({"pattern": "x", "glob": "[bad"})).reason,
            "invalid_glob"
        );
        assert_eq!(
            call_err(&set, TOOL_GREP, json!({"pattern": "x", "path": "../out"})).reason,
            "path_escape"
        );
        assert_eq!(
            call_err(&set, TOOL_GREP, json!({"pattern": "x", "path": "missing"})).reason,
            "not_found"
        );
    }

    #[test]
    fn grep_searches_single_file_and_allow_path_read_only() {
        let workspace = TempDir::new().expect("workspace");
        let allow = TempDir::new().expect("allow");
        write_file(workspace.path(), "a.txt", "needle\n");
        write_file(allow.path(), "ro.txt", "needle in allow\n");
        let set = BuiltinToolSet::new(
            &std::fs::canonicalize(workspace.path()).expect("canonical"),
            &[std::fs::canonicalize(allow.path()).expect("canonical")],
        )
        .expect("set");
        let single = call_ok(
            &set,
            TOOL_GREP,
            json!({"pattern": "needle", "path": "a.txt"}),
        );
        assert_eq!(single["total"], json!(1));
        // 默认 path="." 只搜 workspace，不递归 allow-path。
        let default_search = call_ok(&set, TOOL_GREP, json!({"pattern": "needle"}));
        assert_eq!(default_search["total"], json!(1), "{default_search}");
        // 显式指向 allow-path 目录可只读搜索。
        let allow_path = std::fs::canonicalize(allow.path()).expect("canonical");
        let allow_search = call_ok(
            &set,
            TOOL_GREP,
            json!({"pattern": "needle", "path": allow_path.display().to_string()}),
        );
        assert_eq!(allow_search["total"], json!(1), "{allow_search}");
        assert_eq!(allow_search["matches"][0]["path"], json!("ro.txt"));
    }

    #[test]
    fn glob_lists_files_sorted_and_filters_hidden() {
        let (workspace, set) = set();
        write_file(workspace.path(), "z.rs", "");
        write_file(workspace.path(), "a.rs", "");
        write_file(workspace.path(), "dir/b.rs", "");
        write_file(workspace.path(), ".hidden.rs", "");
        write_file(workspace.path(), "notes.txt", "");
        let payload = call_ok(&set, TOOL_GLOB, json!({"pattern": "**/*.rs"}));
        let paths: Vec<&str> = payload["paths"]
            .as_array()
            .expect("paths")
            .iter()
            .map(|path| path.as_str().expect("path"))
            .collect();
        assert_eq!(paths, vec!["a.rs", "dir/b.rs", "z.rs"], "{payload}");
        assert_eq!(payload["total"], json!(3));
        let hidden = call_ok(
            &set,
            TOOL_GLOB,
            json!({"pattern": "**/*.rs", "include_hidden": true}),
        );
        let hidden_paths: Vec<&str> = hidden["paths"]
            .as_array()
            .expect("paths")
            .iter()
            .map(|path| path.as_str().expect("path"))
            .collect();
        assert!(hidden_paths.contains(&".hidden.rs"), "{hidden}");
    }

    #[test]
    fn glob_honours_max_results_and_bounds_result_size() {
        let (workspace, set) = set();
        for index in 0..5 {
            write_file(workspace.path(), &format!("f{index}.rs"), "");
        }
        let payload = call_ok(
            &set,
            TOOL_GLOB,
            json!({"pattern": "*.rs", "max_results": 2}),
        );
        assert_eq!(payload["paths"].as_array().expect("paths").len(), 2);
        assert_eq!(payload["total"], json!(5));
        assert_eq!(payload["truncated"], json!(true));
        let error = call_err(
            &set,
            TOOL_GLOB,
            json!({"pattern": "*", "max_results": 5001}),
        );
        assert_eq!(error.reason, "invalid_args");

        // 结果 ≤32 KiB：大量长路径 → 截断 + truncated。
        for index in 0..400 {
            let name = format!("{index:03}-{}.rs", "n".repeat(100));
            write_file(workspace.path(), &name, "");
        }
        let large = call_ok(
            &set,
            TOOL_GLOB,
            json!({"pattern": "*.rs", "max_results": 5000}),
        );
        assert_eq!(large["truncated"], json!(true), "{large}");
        let serialized = serde_json::to_string(&large).expect("json");
        assert!(serialized.len() <= crate::constants::BUILTIN_RESULT_MAX_BYTES);
    }

    #[test]
    fn glob_invalid_pattern_and_escape_are_rejected() {
        let (workspace, set) = set();
        write_file(workspace.path(), "a.txt", "x");
        assert_eq!(
            call_err(&set, TOOL_GLOB, json!({"pattern": "[bad"})).reason,
            "invalid_glob"
        );
        assert_eq!(
            call_err(&set, TOOL_GLOB, json!({"pattern": "*", "path": "../x"})).reason,
            "path_escape"
        );
    }
}
