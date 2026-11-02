//! Unified diff 解析与应用（`builtin__apply_patch`；DR-013 §3.4）。
//!
//! v0.4 范围：仅**修改已有普通文件**的 unified diff（`---`/`+++`/`@@`/多文件/
//! `\ No newline at end of file`）；rename/copy/binary/new-file/delete（`/dev/null`）
//! → `unsupported_patch`（fail-closed）。
//!
//! 严格原子由 `files.rs` 的两阶段实现：本模块只做内存解析/校验/应用。

use crate::tools::ToolFailure;

/// 单个文件的补丁（解析结果）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FilePatch {
    /// `---` 路径（原始，未 strip）。
    pub old_path: String,
    /// `+++` 路径（原始，未 strip）。
    pub new_path: String,
    /// hunks（按出现顺序）。
    pub hunks: Vec<Hunk>,
    /// `+++` 侧最后一行无换行（`\ No newline at end of file`）。
    pub new_no_newline: bool,
}

/// 单个 hunk（行级）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Hunk {
    /// 旧文件起始行（1 基；0 = 文件开头插入）。
    pub old_start: usize,
    /// 旧文件行数。
    pub old_count: usize,
    /// 新文件起始行。
    pub new_start: usize,
    /// 新文件行数。
    pub new_count: usize,
    /// 行变更。
    pub lines: Vec<PatchLine>,
}

/// hunk 内单行。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PatchLine {
    /// 上下文行（两侧一致）。
    Context(String),
    /// 新增行（`+`）。
    Add(String),
    /// 删除行（`-`）。
    Remove(String),
}

/// 文件级应用统计。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ApplyStats {
    /// 新增行数。
    pub added: usize,
    /// 删除行数。
    pub removed: usize,
}

/// 解析 unified diff 文本为多文件补丁（失败 → `TOOL_ERROR{reason:"unsupported_patch"}`）。
pub(crate) fn parse_patch(text: &str) -> Result<Vec<FilePatch>, ToolFailure> {
    let lines: Vec<&str> = text.lines().collect();
    let mut patches: Vec<FilePatch> = Vec::new();
    let mut index = 0usize;
    while index < lines.len() {
        let line = lines[index];
        if line.trim().is_empty() {
            index += 1;
            continue;
        }
        if line.starts_with("--- ") {
            let (patch, next) = parse_file(&lines, index)?;
            patches.push(patch);
            index = next;
            continue;
        }
        if is_ignorable_header(line) {
            index += 1;
            continue;
        }
        if is_unsupported_header(line) {
            return Err(unsupported(line));
        }
        return Err(unsupported(line));
    }
    if patches.is_empty() {
        return Err(ToolFailure::new(
            "unsupported_patch",
            "patch contains no '---/+++' file sections",
        ));
    }
    Ok(patches)
}

/// 解析一个 `---/+++` 区块（含 hunks），返回（补丁, 下一行下标）。
fn parse_file(lines: &[&str], start: usize) -> Result<(FilePatch, usize), ToolFailure> {
    let old_path = parse_header_path(lines[start], "--- ")?;
    let Some(plus) = lines.get(start + 1) else {
        return Err(unsupported("patch is truncated after '---'"));
    };
    if !plus.starts_with("+++ ") {
        return Err(unsupported("expected '+++' after '---'"));
    }
    let new_path = parse_header_path(plus, "+++ ")?;
    if old_path == "/dev/null" || new_path == "/dev/null" {
        return Err(unsupported(
            "new-file/delete-file patches (/dev/null) are not supported in v0.4",
        ));
    }
    let mut patch = FilePatch {
        old_path,
        new_path,
        hunks: Vec::new(),
        new_no_newline: false,
    };
    let mut index = start + 2;
    while index < lines.len() {
        let line = lines[index];
        if line.starts_with("--- ") || line.starts_with("diff --git ") {
            break;
        }
        if is_unsupported_header(line) {
            return Err(unsupported(line));
        }
        if is_ignorable_header(line) {
            index += 1;
            continue;
        }
        if !line.starts_with("@@ ") {
            if line.trim().is_empty() {
                index += 1;
                continue;
            }
            return Err(unsupported(line));
        }
        let (hunk, next) = parse_hunk(lines, index, &mut patch)?;
        patch.hunks.push(hunk);
        index = next;
    }
    if patch.hunks.is_empty() {
        return Err(unsupported("file section has no '@@' hunks"));
    }
    Ok((patch, index))
}

/// 解析一个 hunk（`@@ -a[,b] +c[,d] @@`）。
fn parse_hunk(
    lines: &[&str],
    start: usize,
    patch: &mut FilePatch,
) -> Result<(Hunk, usize), ToolFailure> {
    let header = lines[start];
    let (old_start, old_count, new_start, new_count) = parse_hunk_header(header)?;
    let mut hunk = Hunk {
        old_start,
        old_count,
        new_start,
        new_count,
        lines: Vec::new(),
    };
    let mut index = start + 1;
    let mut old_seen = 0usize;
    let mut new_seen = 0usize;
    let mut marker_after: Option<char> = None;
    while index < lines.len() {
        let line = lines[index];
        // 下一个文件/下一个 hunk 的边界（`--- ` 也以 '-' 开头，必须先判定）。
        if line.starts_with("--- ") || line.starts_with("diff --git ") {
            break;
        }
        let Some(prefix) = line.chars().next() else {
            index += 1;
            continue;
        };
        match prefix {
            ' ' => {
                hunk.lines.push(PatchLine::Context(line[1..].to_string()));
                old_seen += 1;
                new_seen += 1;
                marker_after = Some(' ');
            }
            '+' => {
                hunk.lines.push(PatchLine::Add(line[1..].to_string()));
                new_seen += 1;
                marker_after = Some('+');
            }
            '-' => {
                hunk.lines.push(PatchLine::Remove(line[1..].to_string()));
                old_seen += 1;
                marker_after = Some('-');
            }
            '\\' => {
                // `\ No newline at end of file`：作用于紧随其后的上一行。
                if line.contains("No newline at end of file") {
                    if matches!(marker_after, Some('+') | Some(' ')) {
                        patch.new_no_newline = true;
                    }
                    marker_after = None;
                }
                index += 1;
                continue;
            }
            _ => break,
        }
        index += 1;
    }
    if old_seen != old_count || new_seen != new_count {
        return Err(unsupported(&format!(
            "hunk line counts mismatch: header -{old_count} +{new_count}, body -{old_seen} +{new_seen}"
        )));
    }
    Ok((hunk, index))
}

/// 解析 hunk header 的四个数值。
fn parse_hunk_header(header: &str) -> Result<(usize, usize, usize, usize), ToolFailure> {
    let body = header
        .strip_prefix("@@ ")
        .and_then(|rest| rest.split(" @@").next())
        .ok_or_else(|| unsupported("invalid hunk header"))?;
    let mut parts = body.split(' ');
    let old = parts
        .next()
        .and_then(|part| part.strip_prefix('-'))
        .ok_or_else(|| unsupported("invalid hunk header (old range)"))?;
    let new = parts
        .next()
        .and_then(|part| part.strip_prefix('+'))
        .ok_or_else(|| unsupported("invalid hunk header (new range)"))?;
    let (old_start, old_count) = parse_range(old)?;
    let (new_start, new_count) = parse_range(new)?;
    Ok((old_start, old_count, new_start, new_count))
}

fn parse_range(range: &str) -> Result<(usize, usize), ToolFailure> {
    let mut parts = range.split(',');
    let start = parts
        .next()
        .and_then(|value| value.parse::<usize>().ok())
        .ok_or_else(|| unsupported("invalid hunk range start"))?;
    let count = match parts.next() {
        None => 1,
        Some(value) => value
            .parse::<usize>()
            .map_err(|_| unsupported("invalid hunk range count"))?,
    };
    Ok((start, count))
}

/// 在内存中把补丁应用到原文（严格匹配；失败 → `patch_mismatch`，不修改文件）。
pub(crate) fn apply_file(
    patch: &FilePatch,
    original: &str,
) -> Result<(String, ApplyStats), ToolFailure> {
    let (original_lines, original_trailing_newline) = split_lines(original);
    let mut output: Vec<String> = Vec::with_capacity(original_lines.len());
    let mut cursor = 0usize;
    let mut added = 0usize;
    let mut removed = 0usize;
    let mut touches_eof = false;
    for hunk in &patch.hunks {
        let position = if hunk.old_count == 0 {
            hunk.old_start
        } else {
            hunk.old_start.saturating_sub(1)
        };
        if position < cursor || position > original_lines.len() {
            return Err(mismatch(&format!(
                "hunk starts at line {position}, out of order"
            )));
        }
        output.extend_from_slice(&original_lines[cursor..position]);
        cursor = position;
        for line in &hunk.lines {
            match line {
                PatchLine::Context(text) => {
                    if original_lines.get(cursor) != Some(text) {
                        return Err(mismatch(&format!(
                            "context mismatch at line {}",
                            cursor + 1
                        )));
                    }
                    output.push(text.clone());
                    cursor += 1;
                }
                PatchLine::Remove(text) => {
                    if original_lines.get(cursor) != Some(text) {
                        return Err(mismatch(&format!("remove mismatch at line {}", cursor + 1)));
                    }
                    cursor += 1;
                    removed += 1;
                }
                PatchLine::Add(text) => {
                    output.push(text.clone());
                    added += 1;
                }
            }
        }
        touches_eof = cursor == original_lines.len();
    }
    output.extend_from_slice(&original_lines[cursor.min(original_lines.len())..]);
    let trailing_newline = if patch.new_no_newline {
        false
    } else if touches_eof {
        true
    } else {
        original_trailing_newline
    };
    let mut text = output.join("\n");
    if trailing_newline && !text.is_empty() {
        text.push('\n');
    }
    Ok((text, ApplyStats { added, removed }))
}

/// 拆行：返回（无换行行列表, 原文是否以换行结尾）。
pub(crate) fn split_lines(text: &str) -> (Vec<String>, bool) {
    if text.is_empty() {
        return (Vec::new(), false);
    }
    let trailing = text.ends_with('\n');
    let body = if trailing {
        &text[..text.len() - 1]
    } else {
        text
    };
    (body.split('\n').map(str::to_string).collect(), trailing)
}

/// `strip` 前缀组件（`a/`、`b/`），返回相对路径。
pub(crate) fn strip_path(path: &str, strip: usize) -> Result<String, ToolFailure> {
    if strip == 0 {
        return Ok(path.to_string());
    }
    let mut parts = path.split('/');
    let mut skipped = 0usize;
    while skipped < strip {
        if parts.next().is_none() {
            return Err(unsupported(&format!(
                "cannot strip {strip} components from '{path}'"
            )));
        }
        skipped += 1;
    }
    let remaining: Vec<&str> = parts.collect();
    if remaining.is_empty() || remaining.iter().any(|part| part.is_empty()) {
        return Err(unsupported(&format!(
            "stripped path '{path}' has no file name"
        )));
    }
    Ok(remaining.join("/"))
}

fn parse_header_path(line: &str, prefix: &str) -> Result<String, ToolFailure> {
    let rest = line
        .strip_prefix(prefix)
        .ok_or_else(|| unsupported("invalid file header"))?;
    let path = rest.split('\t').next().unwrap_or(rest).trim().to_string();
    if path.is_empty() {
        return Err(unsupported("empty path in file header"));
    }
    Ok(path)
}

fn is_ignorable_header(line: &str) -> bool {
    line.starts_with("index ") || line.starts_with("old mode ") || line.starts_with("new mode ")
}

fn is_unsupported_header(line: &str) -> bool {
    line.starts_with("new file mode")
        || line.starts_with("deleted file mode")
        || line.starts_with("rename from ")
        || line.starts_with("rename to ")
        || line.starts_with("copy from ")
        || line.starts_with("copy to ")
        || line.starts_with("similarity index")
        || line.starts_with("dissimilarity index")
        || line.starts_with("Binary files ")
        || line.starts_with("GIT binary patch")
}

fn unsupported(detail: &str) -> ToolFailure {
    ToolFailure::new("unsupported_patch", format!("unsupported patch: {detail}"))
}

fn mismatch(detail: &str) -> ToolFailure {
    ToolFailure::new("patch_mismatch", format!("patch does not apply: {detail}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_applies_single_hunk() {
        let patch_text = "--- a/file.txt\n+++ b/file.txt\n@@ -1,3 +1,4 @@\n one\n-two\n+TWO\n+two-and-a-half\n three\n";
        let patches = parse_patch(patch_text).expect("parse");
        assert_eq!(patches.len(), 1);
        let (updated, stats) = apply_file(&patches[0], "one\ntwo\nthree\n").expect("apply");
        assert_eq!(updated, "one\nTWO\ntwo-and-a-half\nthree\n");
        assert_eq!(
            stats,
            ApplyStats {
                added: 2,
                removed: 1
            }
        );
    }

    #[test]
    fn applies_multiple_hunks_in_one_file() {
        let patch_text =
            "--- a/f\n+++ b/f\n@@ -1,2 +1,2 @@\n-a\n+A\n b\n@@ -4,2 +4,2 @@\n d\n-e\n+E\n";
        let patches = parse_patch(patch_text).expect("parse");
        let (updated, stats) = apply_file(&patches[0], "a\nb\nc\nd\ne\n").expect("apply");
        assert_eq!(updated, "A\nb\nc\nd\nE\n");
        assert_eq!(
            stats,
            ApplyStats {
                added: 2,
                removed: 2
            }
        );
    }

    #[test]
    fn no_newline_marker_is_preserved() {
        let patch_text =
            "--- a/f\n+++ b/f\n@@ -1,1 +1,1 @@\n-old\n+new\n\\ No newline at end of file\n";
        let patches = parse_patch(patch_text).expect("parse");
        let (updated, _) = apply_file(&patches[0], "old").expect("apply");
        assert_eq!(updated, "new");
    }

    #[test]
    fn mismatch_fails_without_touching_input() {
        let patch_text = "--- a/f\n+++ b/f\n@@ -1,1 +1,1 @@\n-other\n+new\n";
        let patches = parse_patch(patch_text).expect("parse");
        let error = apply_file(&patches[0], "original\n").expect_err("mismatch");
        assert_eq!(error.reason, "patch_mismatch");
    }

    #[test]
    fn unsupported_patch_forms_are_rejected() {
        for text in [
            "rename from a\nrename to b\n--- a/a\n+++ b/b\n@@ -1 +1 @@\n-a\n+b\n",
            "--- /dev/null\n+++ b/new\n@@ -0,0 +1 @@\n+x\n",
            "--- a/f\n+++ b/f\nBinary files a/f and b/f differ\n",
            "--- a/f\n+++ b/f\n@@ -1,3 +1,1 @@\n-a\n+b\n",
            "not a patch at all\n",
        ] {
            let error = parse_patch(text).expect_err(text);
            assert_eq!(error.reason, "unsupported_patch", "{text}");
        }
    }

    #[test]
    fn strip_path_removes_prefix_components() {
        assert_eq!(
            strip_path("a/dir/file.txt", 1).expect("strip"),
            "dir/file.txt"
        );
        assert_eq!(strip_path("file.txt", 0).expect("strip0"), "file.txt");
        assert_eq!(
            strip_path("a", 1).expect_err("too short").reason,
            "unsupported_patch"
        );
    }

    #[test]
    fn split_lines_preserves_trailing_newline_flag() {
        assert_eq!(split_lines(""), (Vec::new(), false));
        assert_eq!(
            split_lines("a\nb\n"),
            (vec!["a".to_string(), "b".to_string()], true)
        );
        assert_eq!(
            split_lines("a\nb"),
            (vec!["a".to_string(), "b".to_string()], false)
        );
    }
}
