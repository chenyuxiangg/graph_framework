//! Workspace jail：内置文件工具的路径解析与边界校验（DR-013 §2）。
//!
//! 解析顺序（冻结）：工具参数 path（相对/绝对）→ 词法拒绝 `..` → `canonicalize`
//! （解析 symlink）→ 与 `workspace_root`/`allow_paths` 的**真实路径组件级前缀**比较
//! （`Path::starts_with`，避免字符串前缀漏洞）。越界 → `path_escape`；写 allow-path
//! → `path_read_only`；特殊文件 → `not_regular_file`。
//!
//! 安全边界：`--allow-path` 是**目录级只读**白名单；`--allow-write-path` 是 v0.5/F2
//! 新增的**目录级可写**白名单（读+写，独立于只读 allow-path；与 workspace 相同/嵌套
//! 幂等；系统目录与 `/` 拒绝）。写目标 = workspace + allow-write-paths。
//! hardlink 到 jail 外 inode 的残余风险按 DR-013 §2.1 接受并文档化（ISSUE-167）。

use std::path::{Component, Path, PathBuf};

use crate::constants::BUILTIN_PATH_MAX_BYTES;
use crate::error::HarnessError;
use crate::tools::ToolFailure;

/// 访问意图。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Access {
    /// 读取（workspace + allow-paths）。
    Read,
    /// 写入（仅 workspace）。
    Write,
}

/// 解析结果所属区域。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PathScope {
    /// 工作区（读写）。
    Workspace,
    /// 只读白名单目录（仅读）。
    AllowPath,
    /// v0.5/F2：可写白名单目录（读+写）。
    AllowWritePath,
}

/// 解析后的真实路径。
#[derive(Debug, Clone)]
pub(crate) struct ResolvedPath {
    /// canonicalize 后的真实路径（symlink 已解析）。
    pub path: PathBuf,
    /// 所属区域（诊断/tests 用；读写判定已在 `resolve` 内完成）。
    #[allow(dead_code)]
    pub scope: PathScope,
    /// 解析前目标是否已存在（write 覆盖判断用）。
    pub existed: bool,
    /// 目标是否为目录（搜索工具允许目录路径）。
    pub is_dir: bool,
}

impl ResolvedPath {
    /// 相对 `base` 的 POSIX 风格路径（搜索结果展示用）。
    pub fn display_relative(&self, base: &Path) -> String {
        self.path
            .strip_prefix(base)
            .unwrap_or(&self.path)
            .components()
            .map(|component| component.as_os_str().to_string_lossy())
            .collect::<Vec<_>>()
            .join("/")
    }
}

/// workspace jail（canonicalize 后的根 + 只读 allow-paths）。
#[derive(Debug, Clone)]
pub(crate) struct WorkspaceJail {
    root: PathBuf,
    allow_paths: Vec<PathBuf>,
    /// v0.5/F2（DR-019 §1）：目录级可写白名单（读+写；独立于只读 allow-paths）。
    allow_write_paths: Vec<PathBuf>,
}

impl WorkspaceJail {
    /// 构造：workspace 必须存在且为目录；allow-paths 同理（失败 → `HARNESS_CONFIG_LOAD`）。
    ///
    /// reason 闭集：`workspace_invalid`（`--workspace-dir` 非法）/`allow_path_invalid`
    /// （`--allow-path` 非法）。
    #[allow(dead_code)] // 兼容 v0.4 API（生产路径统一走 new_with_write_paths）。
    pub fn new(workspace_dir: &Path, allow_paths: &[PathBuf]) -> Result<Self, HarnessError> {
        Self::new_with_write_paths(workspace_dir, allow_paths, &[])
    }

    /// v0.5/F2（DR-019 §1）：构造 workspace + 只读 allow-paths + **可写**白名单。
    ///
    /// reason 闭集：`workspace_invalid`/`allow_path_invalid`/**`allow_write_path_invalid`**。
    /// 可写白名单校验：必须存在且为目录、canonicalize、**不可 `/`**、**系统目录拒绝**
    /// （`/usr`、`/bin`、`/sbin`、`/System`、`/Library`、`/private/etc` 及其子路径）；
    /// 与 workspace/allow-path 相同/嵌套允许（幂等），同目录时**写优先**。
    pub fn new_with_write_paths(
        workspace_dir: &Path,
        allow_paths: &[PathBuf],
        allow_write_paths: &[PathBuf],
    ) -> Result<Self, HarnessError> {
        let root = canonicalize_dir(workspace_dir, "workspace_invalid", "workspace")?;
        let mut canonical_allow = Vec::with_capacity(allow_paths.len());
        for allow in allow_paths {
            let canonical = canonicalize_dir(allow, "allow_path_invalid", "allow-path")?;
            if canonical == root || canonical_allow.contains(&canonical) {
                continue;
            }
            canonical_allow.push(canonical);
        }
        let mut canonical_write = Vec::with_capacity(allow_write_paths.len());
        for write in allow_write_paths {
            let canonical =
                canonicalize_dir(write, "allow_write_path_invalid", "allow-write-path")?;
            if let Some(reason) = forbidden_write_root(&canonical) {
                return Err(HarnessError::config_load(
                    "allow_write_path_invalid",
                    Some(&write.display().to_string()),
                    None,
                    Some(serde_json::json!("existing non-system writable directory")),
                    Some(serde_json::json!(canonical.display().to_string())),
                    format!(
                        "allow-write-path '{}' is not allowed: {reason}",
                        write.display()
                    ),
                ));
            }
            if canonical_write.contains(&canonical) {
                continue;
            }
            canonical_write.push(canonical);
        }
        Ok(Self {
            root,
            allow_paths: canonical_allow,
            allow_write_paths: canonical_write,
        })
    }

    /// canonicalize 后的 workspace 根。
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// 只读 allow-path 根（canonicalize；搜索/沙箱 profile 用）。
    pub fn allow_paths(&self) -> &[PathBuf] {
        &self.allow_paths
    }

    /// v0.5/F2：可写白名单根（canonicalize；`--allow-write-path`）。
    pub fn allow_write_paths(&self) -> &[PathBuf] {
        &self.allow_write_paths
    }

    /// 解析工具参数 `path`（相对 workspace 或绝对）为真实路径并校验访问意图。
    ///
    /// 失败一律 [`ToolFailure`]（喂回 LLM，不终止 run）；reason 闭集：
    /// `invalid_args`/`path_too_long`/`path_escape`/`path_read_only`/`not_found`/
    /// `is_directory`/`not_regular_file`。
    pub fn resolve(&self, raw: &str, access: Access) -> Result<ResolvedPath, ToolFailure> {
        self.resolve_inner(raw, access, false)
    }

    /// 解析路径（允许目录；grep/glob 的 `path` 参数用）。语义同 [`Self::resolve`]。
    pub fn resolve_any(&self, raw: &str, access: Access) -> Result<ResolvedPath, ToolFailure> {
        self.resolve_inner(raw, access, true)
    }

    fn resolve_inner(
        &self,
        raw: &str,
        access: Access,
        allow_dir: bool,
    ) -> Result<ResolvedPath, ToolFailure> {
        if raw.is_empty() {
            return Err(ToolFailure::new("invalid_args", "path must be non-empty"));
        }
        if raw.len() > BUILTIN_PATH_MAX_BYTES {
            return Err(ToolFailure::new(
                "path_too_long",
                format!("path exceeds {} bytes", BUILTIN_PATH_MAX_BYTES),
            ));
        }
        let raw_path = Path::new(raw);
        if raw_path
            .components()
            .any(|component| matches!(component, Component::ParentDir))
        {
            return Err(escape_failure(raw, &self.root));
        }
        let candidate = if raw_path.is_absolute() {
            raw_path.to_path_buf()
        } else {
            self.root.join(raw_path)
        };
        let (path, existed) = canonicalize_target(&candidate, raw)?;
        if access == Access::Read && !existed {
            return Err(ToolFailure::new(
                "not_found",
                format!("path '{raw}' does not exist"),
            ));
        }
        let scope = self.scope_of(&path, raw)?;
        if access == Access::Write {
            // v0.5/F2：可写 = workspace + allow-write-paths；allow-paths 仍只读；
            // 同目录同时出现在 allow-path 与 allow-write-path 时写优先。
            let writable = matches!(scope, PathScope::Workspace | PathScope::AllowWritePath);
            if !writable {
                return Err(ToolFailure::new(
                    "path_read_only",
                    format!(
                        "path '{raw}' is inside a read-only --allow-path (writes allowed in --workspace-dir or --allow-write-path)"
                    ),
                ));
            }
        }
        let is_dir = check_file_kind(&path, access, raw, allow_dir)?;
        Ok(ResolvedPath {
            path,
            scope,
            existed,
            is_dir,
        })
    }

    /// `create_parents=true`：在 workspace 内创建目标父目录（allow-path 只读 → 拒绝）。
    ///
    /// 只允许创建 workspace 内的目录；最深已存在祖先 canonicalize 后必须仍在
    /// workspace 根内，否则 `path_escape`/`path_read_only`。
    pub fn create_parents(&self, raw: &str) -> Result<PathBuf, ToolFailure> {
        if raw.is_empty() || raw.len() > BUILTIN_PATH_MAX_BYTES {
            return Err(ToolFailure::new(
                "invalid_args",
                "path must be non-empty and <= 4 KiB",
            ));
        }
        let raw_path = Path::new(raw);
        if raw_path
            .components()
            .any(|component| matches!(component, Component::ParentDir))
        {
            return Err(escape_failure(raw, &self.root));
        }
        let candidate = if raw_path.is_absolute() {
            raw_path.to_path_buf()
        } else {
            self.root.join(raw_path)
        };
        let Some(parent) = candidate.parent() else {
            return Err(ToolFailure::new(
                "invalid_args",
                format!("path '{raw}' has no parent directory"),
            ));
        };
        let mut existing = parent;
        loop {
            if existing.exists() {
                break;
            }
            let Some(next) = existing.parent() else {
                return Err(ToolFailure::new(
                    "not_found",
                    format!("cannot resolve parent of '{raw}'"),
                ));
            };
            existing = next;
        }
        let canonical = std::fs::canonicalize(existing).map_err(|error| {
            ToolFailure::new(
                "not_found",
                format!("cannot resolve parent of '{raw}': {error}"),
            )
        })?;
        let in_write_path = self
            .allow_write_paths
            .iter()
            .any(|write| canonical.starts_with(write));
        if !canonical.starts_with(&self.root) && !in_write_path {
            if self
                .allow_paths
                .iter()
                .any(|allow| canonical.starts_with(allow))
            {
                return Err(ToolFailure::new(
                    "path_read_only",
                    format!("path '{raw}' is inside a read-only --allow-path"),
                ));
            }
            return Err(escape_failure(raw, &self.root));
        }
        std::fs::create_dir_all(parent).map_err(|error| {
            ToolFailure::new(
                "io_error",
                format!("cannot create parent directories for '{raw}': {error}"),
            )
        })?;
        Ok(parent.to_path_buf())
    }

    /// 只读范围内的根：workspace 根或命中的 allow-path/allow-write-path 根
    /// （展示相对路径用；allow-write-path 授予读+写，故也可读）。
    pub fn read_scope_root(&self, canonical: &Path) -> Option<&Path> {
        if canonical.starts_with(&self.root) {
            return Some(&self.root);
        }
        if let Some(write) = self
            .allow_write_paths
            .iter()
            .find(|write| canonical.starts_with(write))
        {
            return Some(write.as_path());
        }
        self.allow_paths
            .iter()
            .find(|allow| canonical.starts_with(allow))
            .map(PathBuf::as_path)
    }

    /// 遍历期校验：canonical 路径是否在只读允许范围内（workspace 或 allow-path）。
    pub fn read_scope_of(&self, canonical: &Path) -> Option<PathScope> {
        if canonical.starts_with(&self.root) {
            return Some(PathScope::Workspace);
        }
        if self
            .allow_write_paths
            .iter()
            .any(|write| canonical.starts_with(write))
        {
            return Some(PathScope::AllowWritePath);
        }
        if self
            .allow_paths
            .iter()
            .any(|allow| canonical.starts_with(allow))
        {
            return Some(PathScope::AllowPath);
        }
        None
    }

    fn scope_of(&self, path: &Path, raw: &str) -> Result<PathScope, ToolFailure> {
        if path.starts_with(&self.root) {
            return Ok(PathScope::Workspace);
        }
        if self
            .allow_write_paths
            .iter()
            .any(|write| path.starts_with(write))
        {
            return Ok(PathScope::AllowWritePath);
        }
        if self.allow_paths.iter().any(|allow| path.starts_with(allow)) {
            return Ok(PathScope::AllowPath);
        }
        Err(escape_failure(raw, &self.root))
    }
}

/// v0.5/F2（DR-019 §1）：禁止作为可写白名单的系统目录（canonical 前缀）。
const FORBIDDEN_WRITE_ROOTS: [&str; 6] = [
    "/usr",
    "/bin",
    "/sbin",
    "/System",
    "/Library",
    "/private/etc",
];

/// 判定可写白名单是否命中系统目录/`/`；命中返回原因（`allow_write_path_invalid`）。
pub(crate) fn forbidden_write_root(canonical: &Path) -> Option<&'static str> {
    if canonical == Path::new("/") {
        return Some("the filesystem root '/' cannot be writable");
    }
    FORBIDDEN_WRITE_ROOTS
        .iter()
        .find(|root| canonical.starts_with(root))
        .map(|root| match *root {
            "/usr" => "system directory /usr",
            "/bin" => "system directory /bin",
            "/sbin" => "system directory /sbin",
            "/System" => "system directory /System",
            "/Library" => "system directory /Library",
            _ => "system directory /private/etc",
        })
}

/// 逃逸失败（`TOOL_ERROR{reason:"path_escape"}`，附 path/root）。
fn escape_failure(raw: &str, root: &Path) -> ToolFailure {
    ToolFailure::new(
        "path_escape",
        format!(
            "path '{raw}' escapes the workspace jail (root '{}')",
            root.display()
        ),
    )
}

/// workspace/allow-path 根：必须存在、为目录，返回 canonicalize 路径。
fn canonicalize_dir(
    dir: &Path,
    reason: &'static str,
    label: &str,
) -> Result<PathBuf, HarnessError> {
    let canonical = std::fs::canonicalize(dir).map_err(|error| {
        HarnessError::config_load(
            reason,
            Some(&dir.display().to_string()),
            None,
            Some(serde_json::json!(format!("existing {label} directory"))),
            Some(serde_json::json!(error.to_string())),
            format!("{label} '{}' is invalid: {error}", dir.display()),
        )
    })?;
    if !canonical.is_dir() {
        return Err(HarnessError::config_load(
            reason,
            Some(&dir.display().to_string()),
            None,
            Some(serde_json::json!(format!("{label} directory"))),
            Some(serde_json::json!("not a directory")),
            format!("{label} '{}' is not a directory", dir.display()),
        ));
    }
    Ok(canonical)
}

/// canonicalize 目标：已存在 → 解析全部 symlink；不存在（write 新建）→ canonicalize
/// 父目录后拼接文件名（父目录缺失 → `not_found`）。
fn canonicalize_target(candidate: &Path, raw: &str) -> Result<(PathBuf, bool), ToolFailure> {
    match std::fs::symlink_metadata(candidate) {
        Ok(_) => {
            let canonical = std::fs::canonicalize(candidate).map_err(|error| {
                ToolFailure::new(
                    "path_escape",
                    format!("cannot resolve path '{raw}': {error}"),
                )
            })?;
            Ok((canonical, true))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let Some(parent) = candidate.parent() else {
                return Err(ToolFailure::new(
                    "path_escape",
                    format!("path '{raw}' has no parent directory"),
                ));
            };
            let Some(name) = candidate.file_name() else {
                return Err(ToolFailure::new(
                    "invalid_args",
                    format!("path '{raw}' has no file name"),
                ));
            };
            let canonical_parent = std::fs::canonicalize(parent).map_err(|_| {
                ToolFailure::new(
                    "not_found",
                    format!("parent directory of '{raw}' does not exist"),
                )
            })?;
            Ok((canonical_parent.join(name), false))
        }
        Err(error) => Err(ToolFailure::new(
            "not_found",
            format!("cannot stat path '{raw}': {error}"),
        )),
    }
}

/// 普通文件/目录/特殊文件判定。
fn check_file_kind(
    path: &Path,
    access: Access,
    raw: &str,
    allow_dir: bool,
) -> Result<bool, ToolFailure> {
    let Ok(metadata) = std::fs::metadata(path) else {
        return Ok(false);
    };
    let file_type = metadata.file_type();
    if file_type.is_dir() {
        if allow_dir {
            return Ok(true);
        }
        return Err(ToolFailure::new(
            "is_directory",
            format!("path '{raw}' is a directory"),
        ));
    }
    if !file_type.is_file() {
        // FIFO/socket/device 等。
        return Err(ToolFailure::new(
            "not_regular_file",
            format!("path '{raw}' is not a regular file"),
        ));
    }
    let _ = access;
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn jail() -> (TempDir, TempDir, WorkspaceJail) {
        let workspace = TempDir::new().expect("workspace");
        let outside = TempDir::new().expect("outside");
        let jail = WorkspaceJail::new(
            &std::fs::canonicalize(workspace.path()).expect("canonical workspace"),
            &[std::fs::canonicalize(outside.path()).expect("canonical outside")],
        )
        .expect("jail");
        (workspace, outside, jail)
    }

    fn canonical(path: &Path) -> PathBuf {
        std::fs::canonicalize(path).expect("canonical")
    }

    #[test]
    fn resolves_relative_and_absolute_paths_inside_workspace() {
        let (workspace, _outside, jail) = jail();
        std::fs::write(workspace.path().join("a.txt"), "hi").expect("write");
        let read = jail.resolve("a.txt", Access::Read).expect("read path");
        assert_eq!(read.scope, PathScope::Workspace);
        assert!(read.existed);
        let absolute = canonical(workspace.path())
            .join("a.txt")
            .display()
            .to_string();
        assert_eq!(
            jail.resolve(&absolute, Access::Read).expect("abs").path,
            read.path
        );
    }

    #[test]
    fn rejects_parent_dir_escape_lexically() {
        let (_workspace, _outside, jail) = jail();
        for raw in ["../x", "sub/../../x", "a/.."] {
            let error = jail.resolve(raw, Access::Read).expect_err(raw);
            assert_eq!(error.reason, "path_escape", "{raw}");
        }
    }

    #[test]
    fn rejects_symlink_escape_but_allows_inner_symlink() {
        let (workspace, _outside, jail) = jail();
        #[cfg(unix)]
        {
            // 独立于 allow-path 的出口（outside_extra 不在 allow-paths 中）。
            let outside_extra = TempDir::new().expect("outside extra");
            std::fs::write(outside_extra.path().join("secret.txt"), "s").expect("write");
            std::os::unix::fs::symlink(
                canonical(outside_extra.path()),
                workspace.path().join("escape"),
            )
            .expect("symlink");
            let error = jail
                .resolve("escape/secret.txt", Access::Read)
                .expect_err("symlink escape");
            assert_eq!(error.reason, "path_escape");
            std::fs::write(workspace.path().join("real.txt"), "ok").expect("write");
            std::os::unix::fs::symlink(
                canonical(workspace.path()).join("real.txt"),
                workspace.path().join("inner"),
            )
            .expect("inner symlink");
            let resolved = jail.resolve("inner", Access::Read).expect("inner symlink");
            assert_eq!(resolved.scope, PathScope::Workspace);
        }
    }

    #[test]
    fn rejects_absolute_path_outside_and_sibling_prefix() {
        let (workspace, _outside, jail) = jail();
        // 不在 workspace、也不在 allow-paths 的绝对路径 → path_escape。
        let outside_extra = TempDir::new().expect("outside extra");
        let outside_file = outside_extra.path().join("x.txt");
        std::fs::write(&outside_file, "x").expect("write");
        let error = jail
            .resolve(
                &canonical(outside_extra.path())
                    .join("x.txt")
                    .display()
                    .to_string(),
                Access::Read,
            )
            .expect_err("outside absolute");
        assert_eq!(error.reason, "path_escape");

        // 组件级前缀：workspace 的兄弟目录（字符串前缀相同）也必须拒绝。
        let sibling = TempDir::new().expect("sibling");
        let sibling_name = format!(
            "{}-sibling",
            workspace
                .path()
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("ws")
        );
        let sibling_path = canonical(workspace.path())
            .parent()
            .expect("parent")
            .join(&sibling_name);
        std::fs::create_dir_all(&sibling_path).expect("sibling dir");
        std::fs::write(sibling_path.join("s.txt"), "x").expect("write");
        let error = jail
            .resolve(
                &sibling_path.join("s.txt").display().to_string(),
                Access::Read,
            )
            .expect_err("sibling prefix");
        assert_eq!(error.reason, "path_escape");
        let _ = std::fs::remove_dir_all(&sibling_path);
        let _ = sibling;
    }

    #[test]
    fn allow_path_is_readable_but_write_is_rejected() {
        let (_workspace, outside, jail) = jail();
        let file = outside.path().join("ro.txt");
        std::fs::write(&file, "read only").expect("write");
        let read = jail
            .resolve(&file.display().to_string(), Access::Read)
            .expect("allow-path read");
        assert_eq!(read.scope, PathScope::AllowPath);
        let write = jail
            .resolve(&file.display().to_string(), Access::Write)
            .expect_err("allow-path write");
        assert_eq!(write.reason, "path_read_only");
    }

    #[test]
    fn rejects_directory_and_missing_paths() {
        let (workspace, _outside, jail) = jail();
        std::fs::create_dir_all(workspace.path().join("dir")).expect("dir");
        assert_eq!(
            jail.resolve("dir", Access::Read).expect_err("dir").reason,
            "is_directory"
        );
        assert_eq!(
            jail.resolve("missing.txt", Access::Read)
                .expect_err("missing")
                .reason,
            "not_found"
        );
    }

    #[test]
    fn write_new_file_uses_canonical_parent_and_rejects_missing_parent() {
        let (workspace, _outside, jail) = jail();
        std::fs::create_dir_all(workspace.path().join("sub")).expect("sub");
        let resolved = jail
            .resolve("sub/new.txt", Access::Write)
            .expect("new file");
        assert!(!resolved.existed);
        assert!(resolved.path.starts_with(canonical(workspace.path())));
        assert_eq!(
            jail.resolve("nope/new.txt", Access::Write)
                .expect_err("missing parent")
                .reason,
            "not_found"
        );
    }

    #[test]
    fn create_parents_only_inside_workspace() {
        let (workspace, outside, jail) = jail();
        jail.create_parents("a/b/c.txt").expect("create dirs");
        assert!(canonical(workspace.path()).join("a/b").is_dir());
        let error = jail
            .create_parents(
                &canonical(outside.path())
                    .join("x/y.txt")
                    .display()
                    .to_string(),
            )
            .expect_err("outside parents");
        assert!(
            matches!(error.reason.as_str(), "path_read_only" | "path_escape"),
            "{error:?}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn rejects_special_files() {
        let (workspace, _outside, jail) = jail();
        let socket_path = workspace.path().join("sock");
        let _listener = std::os::unix::net::UnixListener::bind(&socket_path).expect("socket");
        let error = jail.resolve("sock", Access::Read).expect_err("socket");
        assert_eq!(error.reason, "not_regular_file");
    }
    // ------------------------------------------------------------------
    // v0.5/F2：allow-write-path（DR-019 §1）
    // ------------------------------------------------------------------

    #[test]
    fn allow_write_path_grants_read_and_write_but_not_system_dirs() {
        let workspace = TempDir::new().expect("workspace");
        let write_dir = TempDir::new().expect("write dir");
        let workspace_canonical = canonical(workspace.path());
        let write_canonical = canonical(write_dir.path());
        let jail = WorkspaceJail::new_with_write_paths(
            &workspace_canonical,
            &[],
            std::slice::from_ref(&write_canonical),
        )
        .expect("jail");
        let target = write_canonical.join("cache.txt");
        let resolved = jail
            .resolve(&target.display().to_string(), Access::Write)
            .expect("write path writable");
        assert_eq!(resolved.scope, PathScope::AllowWritePath);
        std::fs::write(&target, "x").expect("write file");
        assert!(jail
            .resolve(&target.display().to_string(), Access::Read)
            .is_ok());
        // 系统目录 / `/` / 不存在 / 非目录 → allow_write_path_invalid。
        for bad in [
            PathBuf::from("/"),
            PathBuf::from("/usr"),
            PathBuf::from("/System"),
            write_dir.path().join("missing"),
        ] {
            let error = WorkspaceJail::new_with_write_paths(
                &workspace_canonical,
                &[],
                std::slice::from_ref(&bad),
            )
            .expect_err("bad write path");
            assert_eq!(
                error.detail["reason"],
                serde_json::json!("allow_write_path_invalid")
            );
        }
        let file = write_dir.path().join("plain.txt");
        std::fs::write(&file, "x").expect("file");
        let error = WorkspaceJail::new_with_write_paths(&workspace_canonical, &[], &[file])
            .expect_err("file not dir");
        assert_eq!(
            error.detail["reason"],
            serde_json::json!("allow_write_path_invalid")
        );
    }

    #[test]
    fn write_path_overrides_read_only_allow_path_and_is_idempotent_when_nested() {
        let workspace = TempDir::new().expect("workspace");
        let shared = TempDir::new().expect("shared");
        let workspace_canonical = canonical(workspace.path());
        let shared_canonical = canonical(shared.path());
        let jail = WorkspaceJail::new_with_write_paths(
            &workspace_canonical,
            std::slice::from_ref(&shared_canonical),
            std::slice::from_ref(&shared_canonical),
        )
        .expect("jail");
        // 同目录同时给只读与可写 → 写优先。
        let shared_file = shared_canonical.join("x.txt");
        assert_eq!(
            jail.resolve(&shared_file.display().to_string(), Access::Write)
                .expect("write wins")
                .scope,
            PathScope::AllowWritePath
        );
        // 可写白名单嵌套在 workspace 内（幂等，仍视为 workspace 写）。
        let nested = workspace_canonical.join("target");
        std::fs::create_dir_all(&nested).expect("nested");
        let jail = WorkspaceJail::new_with_write_paths(
            &workspace_canonical,
            &[],
            std::slice::from_ref(&nested),
        )
        .expect("jail");
        let nested_file = nested.join("out.txt");
        let resolved = jail
            .resolve(&nested_file.display().to_string(), Access::Write)
            .expect("nested write");
        assert_eq!(resolved.scope, PathScope::Workspace);
        // 只读 allow-path（非可写）仍 path_read_only。
        let readonly = TempDir::new().expect("readonly");
        let readonly_canonical = canonical(readonly.path());
        let jail = WorkspaceJail::new_with_write_paths(
            &workspace_canonical,
            std::slice::from_ref(&readonly_canonical),
            &[],
        )
        .expect("jail");
        let ro_file = readonly_canonical.join("r.txt");
        assert_eq!(
            jail.resolve(&ro_file.display().to_string(), Access::Write)
                .expect_err("read only")
                .reason,
            "path_read_only"
        );
    }

    #[test]
    fn create_parents_allows_write_path_but_not_read_only_path() {
        let workspace = TempDir::new().expect("workspace");
        let write_dir = TempDir::new().expect("write dir");
        let readonly = TempDir::new().expect("readonly");
        let workspace_canonical = canonical(workspace.path());
        let write_canonical = canonical(write_dir.path());
        let readonly_canonical = canonical(readonly.path());
        let jail = WorkspaceJail::new_with_write_paths(
            &workspace_canonical,
            std::slice::from_ref(&readonly_canonical),
            std::slice::from_ref(&write_canonical),
        )
        .expect("jail");
        let target = write_canonical.join("a/b/c.txt");
        jail.create_parents(&target.display().to_string())
            .expect("create parents in write path");
        assert!(write_canonical.join("a/b").is_dir());
        let readonly_target = readonly_canonical.join("x/y.txt");
        assert_eq!(
            jail.create_parents(&readonly_target.display().to_string())
                .expect_err("readonly parents")
                .reason,
            "path_read_only"
        );
    }

    #[test]
    fn writes_outside_workspace_and_write_paths_still_escape() {
        let workspace = TempDir::new().expect("workspace");
        let outside = TempDir::new().expect("outside");
        let workspace_canonical = canonical(workspace.path());
        let outside_canonical = canonical(outside.path());
        let jail =
            WorkspaceJail::new_with_write_paths(&workspace_canonical, &[], &[]).expect("jail");
        let outside_file = outside_canonical.join("evil.txt");
        assert_eq!(
            jail.resolve(&outside_file.display().to_string(), Access::Write)
                .expect_err("outside")
                .reason,
            "path_escape"
        );
    }
}
