//! 内置文件工具（read/write/edit/apply_patch/diff）与 [`BuiltinToolSet`]（DR-013 §3）。
//!
//! 全部为进程内 Rust：路径经 [`WorkspaceJail`] 强制 jail；工具级失败一律
//! [`ToolFailure`]（`TOOL_ERROR` 喂回 LLM），结果 ≤ [`BUILTIN_RESULT_MAX_BYTES`]。

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use crate::builtin::jail::{Access, WorkspaceJail};
use crate::builtin::patch::{apply_file, parse_patch, split_lines, strip_path, FilePatch};
use crate::constants::{
    BUILTIN_DIFF_CONTEXT_MAX_LINES, BUILTIN_READ_BINARY_PROBE_BYTES, BUILTIN_READ_DEFAULT_LINES,
    BUILTIN_READ_MAX_BYTES, BUILTIN_READ_MAX_LINES, BUILTIN_RESULT_MAX_BYTES, BUILTIN_TOOL_PREFIX,
    BUILTIN_TOOL_TIMEOUT_MS, BUILTIN_WRITE_MAX_BYTES,
};
use crate::error::HarnessError;
use crate::tools::{ToolDef, ToolFailure};

/// 工具名（冻结；DR-013 §0/§3）。
pub(crate) const TOOL_READ: &str = "builtin__read";
pub(crate) const TOOL_WRITE: &str = "builtin__write";
pub(crate) const TOOL_EDIT: &str = "builtin__edit";
pub(crate) const TOOL_APPLY_PATCH: &str = "builtin__apply_patch";
pub(crate) const TOOL_DIFF: &str = "builtin__diff";

/// 原子临时文件序号（进程内递增，避免并发同名）。
static TEMP_SEQ: AtomicU64 = AtomicU64::new(0);

/// 内置工具集合（jail + 按 opt-in 注册的工具定义；D2 文件/搜索 + D4 bash）。
pub struct BuiltinToolSet {
    pub(crate) jail: WorkspaceJail,
    /// 是否注册 D2/D3 文件与搜索工具（`--allow-builtin-tools`）。
    file_tools: bool,
    /// D4 bash 沙箱执行器（`--allow-bash`；`None` = 不注册）。
    pub(crate) bash_runner: Option<Arc<dyn crate::builtin::bash::SandboxRunner>>,
    defs: Vec<ToolDef>,
}

impl std::fmt::Debug for BuiltinToolSet {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("BuiltinToolSet")
            .field("root", &self.jail.root())
            .field("file_tools", &self.file_tools)
            .field("bash", &self.bash_runner.is_some())
            .field(
                "tools",
                &self.defs.iter().map(|tool| &tool.name).collect::<Vec<_>>(),
            )
            .finish()
    }
}

impl BuiltinToolSet {
    /// 构造（D2/D3 语义）：注册文件 5 + 搜索 2，无 bash。
    pub fn new(workspace_dir: &Path, allow_paths: &[PathBuf]) -> Result<Arc<Self>, HarnessError> {
        Self::new_with_options(workspace_dir, allow_paths, true, None)
    }

    /// v0.4/D4 构造：按 opt-in 注册（文件/搜索 + 可选 bash）。
    ///
    /// `enable_file_tools=false` 时仅注册 bash（`--allow-bash` 不隐含文件工具）；
    /// `bash` 为 `Some` 时构造期渲染 profile 并 preflight（失败 → `HARNESS_CONFIG_LOAD`）。
    pub fn new_with_options(
        workspace_dir: &Path,
        allow_paths: &[PathBuf],
        enable_file_tools: bool,
        bash: Option<crate::builtin::bash::BashOptions>,
    ) -> Result<Arc<Self>, HarnessError> {
        let jail = WorkspaceJail::new(workspace_dir, allow_paths)?;
        let runner = match bash {
            Some(options) => Some(crate::builtin::bash::MacOsSandbox::new(
                jail.root(),
                jail.allow_paths(),
                &options,
            )?),
            None => None,
        };
        Ok(Self::assemble(jail, enable_file_tools, runner))
    }

    /// 测试 seam：注入 Fake [`SandboxRunner`]（不渲染 profile/不 preflight）。
    #[cfg(test)]
    pub(crate) fn new_with_runner(
        workspace_dir: &Path,
        allow_paths: &[PathBuf],
        enable_file_tools: bool,
        runner: Arc<dyn crate::builtin::bash::SandboxRunner>,
    ) -> Result<Arc<Self>, HarnessError> {
        let jail = WorkspaceJail::new(workspace_dir, allow_paths)?;
        Ok(Self::assemble(jail, enable_file_tools, Some(runner)))
    }

    fn assemble(
        jail: WorkspaceJail,
        enable_file_tools: bool,
        bash_runner: Option<Arc<dyn crate::builtin::bash::SandboxRunner>>,
    ) -> Arc<Self> {
        let mut defs = Vec::new();
        if enable_file_tools {
            defs.extend(file_tool_defs());
            defs.extend(crate::builtin::search::search_tool_defs());
        }
        if bash_runner.is_some() {
            defs.push(crate::builtin::bash::bash_tool_def());
        }
        Arc::new(Self {
            jail,
            file_tools: enable_file_tools,
            bash_runner,
            defs,
        })
    }

    /// 固定工具定义（顺序稳定；`AgentConfig.tools` 决定给 LLM 的顺序）。
    pub fn definitions(&self) -> &[ToolDef] {
        &self.defs
    }

    /// canonicalize 后的 workspace 根（诊断用）。
    pub fn workspace_root(&self) -> &Path {
        self.jail.root()
    }

    /// 执行一次内置工具调用；成功返回**紧凑 JSON**（≤32 KiB，可解析）。
    ///
    /// 失败 → [`ToolFailure`]（`TOOL_ERROR` 喂回 LLM，不终止 run）。
    pub fn call(&self, tool_name: &str, args: &Value) -> Result<String, ToolFailure> {
        let control = crate::builtin::sandbox::ProcessControl::new();
        self.call_with_control(tool_name, args, &control)
    }

    /// 带进程控制的调用（bash 超时/取消时由调度层 `request_kill`）。
    pub(crate) fn call_with_control(
        &self,
        tool_name: &str,
        args: &Value,
        control: &crate::builtin::sandbox::ProcessControl,
    ) -> Result<String, ToolFailure> {
        let payload = match tool_name {
            TOOL_READ => self.read(args)?,
            TOOL_WRITE => self.write(args)?,
            TOOL_EDIT => self.edit(args)?,
            TOOL_APPLY_PATCH => self.apply_patch(args)?,
            TOOL_DIFF => self.diff(args)?,
            crate::builtin::search::TOOL_GREP => self.grep(args)?,
            crate::builtin::search::TOOL_GLOB => self.glob(args)?,
            crate::builtin::bash::TOOL_BASH => self.bash(args, control)?,
            other => {
                return Err(ToolFailure::new(
                    "unknown_builtin_tool",
                    format!("unknown builtin tool '{other}'"),
                ))
            }
        };
        Ok(self.limit_result(payload))
    }

    /// 结果序列化 + 统一上限（`truncated:true` + 全量 sha256 由各工具提供）。
    pub(crate) fn limit_result(&self, payload: Value) -> String {
        // D4：bash 结果按 stdout/stderr 各 16 KiB head+tail（DR-013 §8 例外），
        // 不再套用 32 KiB 总上限（两路都满时可略超 32 KiB）。
        if payload.get("sandbox").is_some() && payload.get("exit_code").is_some() {
            return serde_json::to_string(&payload).unwrap_or_else(|_| "{}".to_string());
        }
        let compact = serde_json::to_string(&payload).unwrap_or_else(|_| "{}".to_string());
        if compact.len() <= BUILTIN_RESULT_MAX_BYTES {
            return compact;
        }
        let mut bounded = payload;
        if let Some(object) = bounded.as_object_mut() {
            object.insert("truncated".to_string(), Value::Bool(true));
        }
        for key in ["content", "diff"] {
            // 反复收缩（每次 3/4）直到序列化落回上限，保留截断标记。
            for _ in 0..64 {
                if !shrink_string_field(&mut bounded, key) {
                    break;
                }
                let compact = serde_json::to_string(&bounded).unwrap_or_default();
                if compact.len() <= BUILTIN_RESULT_MAX_BYTES {
                    return compact;
                }
            }
        }
        // 搜索类结果：从尾部截断 matches/paths 数组（保留前缀，顺序稳定）。
        for key in ["matches", "paths"] {
            loop {
                let compact = serde_json::to_string(&bounded).unwrap_or_default();
                if compact.len() <= BUILTIN_RESULT_MAX_BYTES {
                    return compact;
                }
                let Some(object) = bounded.as_object_mut() else {
                    break;
                };
                let Some(Value::Array(items)) = object.get_mut(key) else {
                    break;
                };
                if items.is_empty() {
                    break;
                }
                items.pop();
                object.insert("truncated".to_string(), Value::Bool(true));
            }
        }
        // 兜底（如超长 path 列表）：保留 hash 与截断标记。
        let sha = bounded.get("sha256").cloned().unwrap_or(Value::Null);
        serde_json::to_string(&json!({
            "truncated": true,
            "sha256": sha,
        }))
        .unwrap_or_else(|_| "{\"truncated\":true}".to_string())
    }

    fn read(&self, args: &Value) -> Result<Value, ToolFailure> {
        let path = required_str(args, "path")?;
        let offset = optional_u64(args, "offset", 0)? as usize;
        let limit = optional_u64(args, "limit", BUILTIN_READ_DEFAULT_LINES as u64)? as usize;
        if limit == 0 || limit > BUILTIN_READ_MAX_LINES {
            return Err(ToolFailure::new(
                "invalid_args",
                format!("limit must be in 1..={BUILTIN_READ_MAX_LINES}"),
            ));
        }
        let resolved = self.jail.resolve(path, Access::Read)?;
        let bytes = read_bounded(&resolved.path, BUILTIN_READ_MAX_BYTES)?;
        if is_binary(&bytes) {
            return Err(ToolFailure::new(
                "binary_file",
                format!("path '{path}' looks like a binary file (NUL in first 8 KiB)"),
            ));
        }
        let text = String::from_utf8(bytes).map_err(|_| {
            ToolFailure::new(
                "invalid_utf8",
                format!("path '{path}' is not valid UTF-8 text"),
            )
        })?;
        let sha = sha256_hex(text.as_bytes());
        let (lines, _) = split_lines(&text);
        let total_lines = lines.len();
        let selected: Vec<&str> = lines
            .iter()
            .skip(offset)
            .take(limit)
            .map(String::as_str)
            .collect();
        let mut content = selected.join("\n");
        let mut truncated = false;
        let budget = BUILTIN_RESULT_MAX_BYTES.saturating_sub(256);
        if content.len() > budget {
            let (shrunk, cut) = truncate_utf8(&content, budget);
            content = shrunk;
            truncated = cut;
        }
        Ok(json!({
            "content": content,
            "lines": selected.len(),
            "total_lines": total_lines,
            "truncated": truncated,
            "sha256": sha,
        }))
    }

    fn write(&self, args: &Value) -> Result<Value, ToolFailure> {
        let path = required_str(args, "path")?;
        let content = required_str(args, "content")?;
        let overwrite = optional_bool(args, "overwrite", false)?;
        let create_parents = optional_bool(args, "create_parents", false)?;
        if content.len() > BUILTIN_WRITE_MAX_BYTES {
            return Err(ToolFailure::new(
                "too_large",
                format!("content exceeds {BUILTIN_WRITE_MAX_BYTES} bytes"),
            ));
        }
        if create_parents {
            self.jail.create_parents(path)?;
        }
        let resolved = self.jail.resolve(path, Access::Write)?;
        let sha_before = if resolved.existed {
            let bytes = read_bounded(&resolved.path, BUILTIN_WRITE_MAX_BYTES as u64)?;
            Some(sha256_hex(&bytes))
        } else {
            None
        };
        if resolved.existed && !overwrite {
            let metadata = std::fs::metadata(&resolved.path).ok();
            let size = metadata.as_ref().map(|meta| meta.len()).unwrap_or(0);
            let mtime = metadata
                .and_then(|meta| meta.modified().ok())
                .map(|time| format!("{time:?}"))
                .unwrap_or_else(|| "unknown".to_string());
            return Err(ToolFailure::new(
                "file_exists",
                format!(
                    "path '{path}' already exists (size={size}, mtime={mtime}, sha256={}); pass overwrite=true to replace",
                    sha_before.clone().unwrap_or_default()
                ),
            ));
        }
        let previous_perms = if resolved.existed {
            std::fs::metadata(&resolved.path)
                .ok()
                .map(|meta| meta.permissions())
        } else {
            None
        };
        atomic_write(&resolved.path, content.as_bytes(), previous_perms.as_ref())
            .map_err(|error| io_failure("write", path, &error))?;
        Ok(json!({
            "path": path,
            "bytes_written": content.len(),
            "sha256_before": sha_before,
            "sha256_after": sha256_hex(content.as_bytes()),
        }))
    }

    fn edit(&self, args: &Value) -> Result<Value, ToolFailure> {
        let path = required_str(args, "path")?;
        let old_string = required_str(args, "old_string")?;
        let new_string = required_str(args, "new_string")?;
        let replace_all = optional_bool(args, "replace_all", false)?;
        if old_string.is_empty() {
            return Err(ToolFailure::new(
                "invalid_args",
                "old_string must be non-empty",
            ));
        }
        if old_string.len() + new_string.len() > BUILTIN_WRITE_MAX_BYTES {
            return Err(ToolFailure::new(
                "too_large",
                format!("edit input exceeds {BUILTIN_WRITE_MAX_BYTES} bytes"),
            ));
        }
        let resolved = self.jail.resolve(path, Access::Write)?;
        let bytes = read_bounded(&resolved.path, BUILTIN_WRITE_MAX_BYTES as u64)?;
        let text = String::from_utf8(bytes)
            .map_err(|_| ToolFailure::new("invalid_utf8", format!("path '{path}' is not UTF-8")))?;
        let count = text.matches(old_string).count();
        if count == 0 {
            return Err(ToolFailure::new(
                "no_match",
                format!("old_string not found in '{path}'"),
            ));
        }
        if !replace_all && count > 1 {
            return Err(ToolFailure::new(
                "multiple_matches",
                format!("old_string matches {count} times in '{path}' (pass replace_all=true)"),
            ));
        }
        let updated = if replace_all {
            text.replace(old_string, new_string)
        } else {
            text.replacen(old_string, new_string, 1)
        };
        // CAS：写回前再读一次，确认编辑期间文件未漂移（DR-013 §3.3）。
        let current =
            std::fs::read(&resolved.path).map_err(|error| io_failure("read", path, &error))?;
        if sha256_hex(&current) != sha256_hex(text.as_bytes()) {
            return Err(ToolFailure::new(
                "cas_mismatch",
                format!("'{path}' changed while editing (sha256 mismatch)"),
            ));
        }
        let permissions = std::fs::metadata(&resolved.path)
            .ok()
            .map(|meta| meta.permissions());
        atomic_write(&resolved.path, updated.as_bytes(), permissions.as_ref())
            .map_err(|error| io_failure("write", path, &error))?;
        Ok(json!({
            "path": path,
            "replacements": if replace_all { count } else { 1 },
            "sha256_before": sha256_hex(text.as_bytes()),
            "sha256_after": sha256_hex(updated.as_bytes()),
        }))
    }

    fn apply_patch(&self, args: &Value) -> Result<Value, ToolFailure> {
        let patch_text = required_str(args, "patch")?;
        let strip = optional_u64(args, "strip", 1)? as usize;
        if patch_text.len() > BUILTIN_WRITE_MAX_BYTES {
            return Err(ToolFailure::new(
                "too_large",
                format!("patch exceeds {BUILTIN_WRITE_MAX_BYTES} bytes"),
            ));
        }
        let patches = parse_patch(patch_text)?;
        let mut staged = self.stage_patches(&patches, strip)?;
        write_staged_atomically(&mut staged)?;
        Ok(patch_result(&staged))
    }

    /// 阶段 1：全部补丁在内存解析/校验/应用（任一失败 → 零改动）。
    fn stage_patches(
        &self,
        patches: &[FilePatch],
        strip: usize,
    ) -> Result<Vec<StagedFile>, ToolFailure> {
        let mut staged: Vec<StagedFile> = Vec::with_capacity(patches.len());
        for patch in patches {
            let target = strip_path(&patch.new_path, strip)?;
            let resolved = self.jail.resolve(&target, Access::Write)?;
            if !resolved.existed {
                return Err(ToolFailure::new(
                    "not_found",
                    format!("patch target '{target}' does not exist"),
                ));
            }
            if staged.iter().any(|item| item.target == resolved.path) {
                return Err(ToolFailure::new(
                    "unsupported_patch",
                    format!("duplicate patch target '{target}'"),
                ));
            }
            let bytes = read_bounded(&resolved.path, BUILTIN_WRITE_MAX_BYTES as u64)?;
            let original = String::from_utf8(bytes).map_err(|_| {
                ToolFailure::new(
                    "invalid_utf8",
                    format!("patch target '{target}' is not valid UTF-8"),
                )
            })?;
            let (updated, stats) = apply_file(patch, &original)?;
            let permissions = std::fs::metadata(&resolved.path)
                .ok()
                .map(|meta| meta.permissions());
            staged.push(StagedFile {
                target: resolved.path,
                display_path: target,
                original,
                updated,
                added: stats.added,
                removed: stats.removed,
                temp: None,
                permissions,
            });
        }
        Ok(staged)
    }

    fn diff(&self, args: &Value) -> Result<Value, ToolFailure> {
        let path = required_str(args, "path")?;
        let old_string = required_str(args, "old_string")?;
        let new_string = required_str(args, "new_string")?;
        let context = optional_u64(args, "context_lines", 3)? as usize;
        if old_string.is_empty() {
            return Err(ToolFailure::new(
                "invalid_args",
                "old_string must be non-empty",
            ));
        }
        if context > BUILTIN_DIFF_CONTEXT_MAX_LINES {
            return Err(ToolFailure::new(
                "invalid_args",
                format!("context_lines must be <= {BUILTIN_DIFF_CONTEXT_MAX_LINES}"),
            ));
        }
        // REV-106 N6：diff 输入与 write 同级 cap（防 similar 大参数内存放大）。
        if old_string.len() + new_string.len() > BUILTIN_WRITE_MAX_BYTES {
            return Err(ToolFailure::new(
                "too_large",
                format!("diff input exceeds {BUILTIN_WRITE_MAX_BYTES} bytes"),
            ));
        }
        let resolved = self.jail.resolve(path, Access::Read)?;
        let bytes = read_bounded(&resolved.path, BUILTIN_READ_MAX_BYTES)?;
        let original = String::from_utf8(bytes)
            .map_err(|_| ToolFailure::new("invalid_utf8", format!("path '{path}' is not UTF-8")))?;
        if !original.contains(old_string) {
            return Err(ToolFailure::new(
                "no_match",
                format!("old_string not found in '{path}'"),
            ));
        }
        let updated = original.replacen(old_string, new_string, 1);
        let changed = original != updated;
        let diff_text = if changed {
            unified_diff(&original, &updated, path, context)
        } else {
            String::new()
        };
        let mut truncated = false;
        let (diff_text, cut) =
            truncate_utf8(&diff_text, BUILTIN_RESULT_MAX_BYTES.saturating_sub(256));
        truncated |= cut;
        Ok(json!({
            "diff": diff_text,
            "changed": changed,
            "truncated": truncated,
            "sha256": sha256_hex(original.as_bytes()),
        }))
    }
}

/// 两阶段写盘的一个待写文件。
#[derive(Debug)]
struct StagedFile {
    target: PathBuf,
    display_path: String,
    original: String,
    updated: String,
    added: usize,
    removed: usize,
    temp: Option<PathBuf>,
    /// 原文件权限（回滚时恢复；best-effort）。
    permissions: Option<std::fs::Permissions>,
}

/// 阶段 2/3：写临时文件 → CAS 校验 → 依次 rename；失败清理/回滚（不落盘部分改动）。
fn write_staged_atomically(staged: &mut [StagedFile]) -> Result<(), ToolFailure> {
    for index in 0..staged.len() {
        let (target, display_path, updated, permissions) = {
            let file = &staged[index];
            (
                file.target.clone(),
                file.display_path.clone(),
                file.updated.clone(),
                file.permissions.clone(),
            )
        };
        match write_temp(&target, updated.as_bytes(), permissions.as_ref()) {
            Ok(temp) => staged[index].temp = Some(temp),
            Err(error) => {
                // N2：前序已创建的 temp 全部清理，返回零改动。
                cleanup_temps(staged);
                return Err(io_failure("write", &display_path, &error));
            }
        }
    }
    let mut renamed: Vec<usize> = Vec::new();
    for index in 0..staged.len() {
        let Some((temp, target, display_path, original)) = ({
            let file = &staged[index];
            file.temp.as_ref().map(|temp| {
                (
                    temp.clone(),
                    file.target.clone(),
                    file.display_path.clone(),
                    file.original.clone(),
                )
            })
        }) else {
            continue;
        };
        // CAS：rename 前再次确认文件内容未漂移（DR-013 §3.4 的解析期校验强化）。
        let current =
            std::fs::read(&target).map_err(|error| io_failure("read", &display_path, &error))?;
        if sha256_hex(&current) != sha256_hex(original.as_bytes()) {
            cleanup_temps(staged);
            rollback_renamed(staged, &renamed);
            return Err(ToolFailure::new(
                "cas_mismatch",
                format!("'{display_path}' changed while applying patch"),
            ));
        }
        if std::fs::rename(&temp, &target).is_err() {
            cleanup_temps(staged);
            rollback_renamed(staged, &renamed);
            return Err(ToolFailure::new(
                "write_failed",
                format!("failed to replace '{display_path}'"),
            ));
        }
        renamed.push(index);
    }
    for file in staged.iter_mut() {
        file.temp = None;
    }
    Ok(())
}

/// 失败回滚：把已 rename 的文件写回原文并恢复原权限（best-effort，缩小窗口）。
///
/// 残余风险（REV-106 N3，文档化）：rename 级窗口内进程崩溃/二次写失败时，目标可能
/// 停留在新内容；回滚本身失败为 clean-up 残余（不宣称跨文件事务级原子）。
fn rollback_renamed(staged: &[StagedFile], renamed: &[usize]) {
    for index in renamed {
        let file = &staged[*index];
        let _ = atomic_write(
            &file.target,
            file.original.as_bytes(),
            file.permissions.as_ref(),
        );
    }
}

/// 清理未 rename 的临时文件。
fn cleanup_temps(staged: &mut [StagedFile]) {
    for file in staged.iter_mut() {
        if let Some(temp) = file.temp.take() {
            let _ = std::fs::remove_file(temp);
        }
    }
}

fn patch_result(staged: &[StagedFile]) -> Value {
    let files: Vec<Value> = staged
        .iter()
        .map(|file| {
            json!({
                "path": file.display_path,
                "added": file.added,
                "removed": file.removed,
                "sha256_before": sha256_hex(file.original.as_bytes()),
                "sha256_after": sha256_hex(file.updated.as_bytes()),
            })
        })
        .collect();
    json!({
        "files": files,
        "total_added": staged.iter().map(|file| file.added).sum::<usize>(),
        "total_removed": staged.iter().map(|file| file.removed).sum::<usize>(),
    })
}

/// 固定工具定义（DR-013 §3 JSON Schema）。
pub(crate) fn file_tool_defs() -> Vec<ToolDef> {
    vec![
        tool_def(
            TOOL_READ,
            "Read a UTF-8 text file inside the workspace (offset/limit lines).",
            json!({
                "type": "object",
                "properties": {
                    "path": {"type": "string", "description": "相对 workspace 或 allow-path 的路径；绝对路径须在 jail 内"},
                    "offset": {"type": "integer", "minimum": 0},
                    "limit": {"type": "integer", "minimum": 1, "maximum": BUILTIN_READ_MAX_LINES, "default": BUILTIN_READ_DEFAULT_LINES}
                },
                "required": ["path"],
                "additionalProperties": false
            }),
        ),
        tool_def(
            TOOL_WRITE,
            "Write a UTF-8 text file inside the workspace (atomic tmp+rename).",
            json!({
                "type": "object",
                "properties": {
                    "path": {"type": "string"},
                    "content": {"type": "string"},
                    "overwrite": {"type": "boolean", "default": false},
                    "create_parents": {"type": "boolean", "default": false}
                },
                "required": ["path", "content"],
                "additionalProperties": false
            }),
        ),
        tool_def(
            TOOL_EDIT,
            "Replace a unique string (or all occurrences) in a workspace file atomically.",
            json!({
                "type": "object",
                "properties": {
                    "path": {"type": "string"},
                    "old_string": {"type": "string", "minLength": 1},
                    "new_string": {"type": "string"},
                    "replace_all": {"type": "boolean", "default": false}
                },
                "required": ["path", "old_string", "new_string"],
                "additionalProperties": false
            }),
        ),
        tool_def(
            TOOL_APPLY_PATCH,
            "Apply a unified diff to workspace files (strictly atomic; multi-file).",
            json!({
                "type": "object",
                "properties": {
                    "patch": {"type": "string"},
                    "strip": {"type": "integer", "minimum": 0, "default": 1}
                },
                "required": ["patch"],
                "additionalProperties": false
            }),
        ),
        tool_def(
            TOOL_DIFF,
            "Preview a unified diff for replacing old_string with new_string (does not write).",
            json!({
                "type": "object",
                "properties": {
                    "path": {"type": "string"},
                    "old_string": {"type": "string", "minLength": 1},
                    "new_string": {"type": "string"},
                    "context_lines": {"type": "integer", "minimum": 0, "maximum": BUILTIN_DIFF_CONTEXT_MAX_LINES, "default": 3}
                },
                "required": ["path", "old_string", "new_string"],
                "additionalProperties": false
            }),
        ),
    ]
}

fn tool_def(name: &str, description: &str, parameters: Value) -> ToolDef {
    ToolDef {
        name: name.to_string(),
        description: description.to_string(),
        parameters,
        timeout_ms: BUILTIN_TOOL_TIMEOUT_MS,
        source: format!("{BUILTIN_TOOL_PREFIX}file"),
    }
}

// ---------------------------------------------------------------------------
// 参数/IO/哈希/截断辅助
// ---------------------------------------------------------------------------

pub(crate) fn required_str<'a>(args: &'a Value, key: &str) -> Result<&'a str, ToolFailure> {
    args.get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| ToolFailure::new("invalid_args", format!("'{key}' must be a string")))
}

pub(crate) fn optional_u64(args: &Value, key: &str, default: u64) -> Result<u64, ToolFailure> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(default),
        Some(value) => value.as_u64().ok_or_else(|| {
            ToolFailure::new(
                "invalid_args",
                format!("'{key}' must be a non-negative integer"),
            )
        }),
    }
}

pub(crate) fn optional_bool(args: &Value, key: &str, default: bool) -> Result<bool, ToolFailure> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(default),
        Some(Value::Bool(value)) => Ok(*value),
        Some(_) => Err(ToolFailure::new(
            "invalid_args",
            format!("'{key}' must be a boolean"),
        )),
    }
}

/// 有界读取（必须有界；stat 后按 `take(max+1)` 读取并二次校验，防 stat 后增长）。
///
/// REV-106：不存在 → `not_found`（若 resolve 后被删除，也不退化为 io_error）。
fn read_bounded(path: &Path, max_bytes: u64) -> Result<Vec<u8>, ToolFailure> {
    use std::io::Read as _;
    let file = std::fs::File::open(path).map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            ToolFailure::new("not_found", format!("'{}' does not exist", path.display()))
        } else {
            io_failure("read", &path.display().to_string(), &error)
        }
    })?;
    let mut bytes = Vec::new();
    let read = file
        .take(max_bytes.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|error| io_failure("read", &path.display().to_string(), &error))?;
    if read as u64 > max_bytes {
        return Err(ToolFailure::new(
            "too_large",
            format!("file exceeds {max_bytes} bytes"),
        ));
    }
    Ok(bytes)
}

fn is_binary(bytes: &[u8]) -> bool {
    bytes
        .iter()
        .take(BUILTIN_READ_BINARY_PROBE_BYTES)
        .any(|byte| *byte == 0)
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut out = String::with_capacity(64);
    for byte in digest {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

/// 原子写：同目录临时文件 → fsync → rename；失败不改原文件。
fn atomic_write(
    target: &Path,
    bytes: &[u8],
    permissions: Option<&std::fs::Permissions>,
) -> std::io::Result<()> {
    let temp = write_temp(target, bytes, permissions)?;
    if let Err(error) = std::fs::rename(&temp, target) {
        // N2：rename 失败不得残留 temp。
        let _ = std::fs::remove_file(&temp);
        return Err(error);
    }
    Ok(())
}

/// 写同目录临时文件（`file + .zgraph-tmp-<pid>-<seq>`），fsync 后返回路径。
fn write_temp(
    target: &Path,
    bytes: &[u8],
    permissions: Option<&std::fs::Permissions>,
) -> std::io::Result<PathBuf> {
    let parent = target.parent().unwrap_or_else(|| Path::new("."));
    let name = target
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("file");
    let seq = TEMP_SEQ.fetch_add(1, Ordering::Relaxed);
    let temp = parent.join(format!(".{name}.zgraph-tmp-{}-{seq}", std::process::id()));
    let result = (|| {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        if let Some(permissions) = permissions {
            let _ = file.set_permissions(permissions.clone());
        }
        Ok(())
    })();
    match result {
        Ok(()) => Ok(temp),
        Err(error) => {
            let _ = std::fs::remove_file(&temp);
            Err(error)
        }
    }
}

pub(crate) fn io_failure(action: &str, path: &str, error: &std::io::Error) -> ToolFailure {
    ToolFailure::new("io_error", format!("{action} '{path}' failed: {error}"))
}

/// 按 UTF-8 字符边界截断到 ≤ `max_bytes`（追加 `…[truncated]` 标记）。
pub(crate) fn truncate_utf8(text: &str, max_bytes: usize) -> (String, bool) {
    if text.len() <= max_bytes {
        return (text.to_string(), false);
    }
    let marker = "…[truncated]";
    let budget = max_bytes.saturating_sub(marker.len());
    let mut end = budget.min(text.len());
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    let mut out = text[..end].to_string();
    out.push_str(marker);
    (out, true)
}

/// 收缩对象里的字符串字段（结果兜底限长用）；返回是否发生收缩。
fn shrink_string_field(value: &mut Value, key: &str) -> bool {
    let Some(object) = value.as_object_mut() else {
        return false;
    };
    let Some(text) = object.get(key).and_then(Value::as_str) else {
        return false;
    };
    let target = text.len().saturating_mul(3) / 4;
    let (shrunk, _) = truncate_utf8(text, target.max(64));
    object.insert(key.to_string(), Value::String(shrunk));
    true
}

/// unified diff（`similar`；确定性 header 为给定 path）。
fn unified_diff(old: &str, new: &str, path: &str, context: usize) -> String {
    let diff = similar::TextDiff::from_lines(old, new);
    let mut unified = diff.unified_diff();
    unified
        .header(&format!("a/{path}"), &format!("b/{path}"))
        .context_radius(context);
    format!("{unified}")
}

/// 工具名是否属于 `builtin__` 保留前缀（DR-013 §1.1）。
pub(crate) fn has_builtin_prefix(name: &str) -> bool {
    name.starts_with(BUILTIN_TOOL_PREFIX)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tempfile::TempDir;

    fn set() -> (TempDir, Arc<BuiltinToolSet>) {
        let workspace = TempDir::new().expect("workspace");
        let jail = std::fs::canonicalize(workspace.path()).expect("canonical");
        let set = BuiltinToolSet::new(&jail, &[]).expect("tool set");
        (workspace, set)
    }

    fn call_ok(set: &BuiltinToolSet, tool: &str, args: Value) -> Value {
        let text = set.call(tool, &args).expect("tool ok");
        assert!(
            text.len() <= BUILTIN_RESULT_MAX_BYTES,
            "result bounded: {}",
            text.len()
        );
        serde_json::from_str(&text).expect("result is JSON")
    }

    fn call_err(set: &BuiltinToolSet, tool: &str, args: Value) -> ToolFailure {
        set.call(tool, &args).expect_err("tool error")
    }

    #[test]
    fn read_supports_offset_limit_and_metadata() {
        let (workspace, set) = set();
        std::fs::write(workspace.path().join("a.txt"), "l0\nl1\nl2\nl3\n").expect("write");
        let payload = call_ok(
            &set,
            TOOL_READ,
            json!({"path": "a.txt", "offset": 1, "limit": 2}),
        );
        assert_eq!(payload["content"], json!("l1\nl2"));
        assert_eq!(payload["lines"], json!(2));
        assert_eq!(payload["total_lines"], json!(4));
        assert_eq!(payload["truncated"], json!(false));
        assert_eq!(payload["sha256"].as_str().map(str::len), Some(64));
    }

    #[test]
    fn read_rejects_missing_directory_binary_and_invalid_utf8() {
        let (workspace, set) = set();
        assert_eq!(
            call_err(&set, TOOL_READ, json!({"path": "missing.txt"})).reason,
            "not_found"
        );
        std::fs::create_dir_all(workspace.path().join("dir")).expect("dir");
        assert_eq!(
            call_err(&set, TOOL_READ, json!({"path": "dir"})).reason,
            "is_directory"
        );
        std::fs::write(workspace.path().join("bin"), b"ab\0cd").expect("bin");
        assert_eq!(
            call_err(&set, TOOL_READ, json!({"path": "bin"})).reason,
            "binary_file"
        );
        std::fs::write(workspace.path().join("bad"), [0xFF_u8, 0xFE]).expect("bad");
        assert_eq!(
            call_err(&set, TOOL_READ, json!({"path": "bad"})).reason,
            "invalid_utf8"
        );
    }

    #[test]
    fn read_enforces_file_and_result_caps() {
        let (workspace, set) = set();
        // 1 MiB 上限。
        let big = "x".repeat((BUILTIN_READ_MAX_BYTES + 1) as usize);
        std::fs::write(workspace.path().join("big.txt"), big).expect("big");
        assert_eq!(
            call_err(&set, TOOL_READ, json!({"path": "big.txt"})).reason,
            "too_large"
        );
        // 结果上限：内容超 32 KiB → truncated 标记，序列化仍 ≤32 KiB。
        let lines: Vec<String> = (0..400).map(|_| "y".repeat(200)).collect();
        std::fs::write(workspace.path().join("wide.txt"), lines.join("\n")).expect("wide");
        let payload = call_ok(&set, TOOL_READ, json!({"path": "wide.txt"}));
        assert_eq!(payload["truncated"], json!(true));
        assert!(payload["content"]
            .as_str()
            .expect("content")
            .ends_with("…[truncated]"));
    }

    #[test]
    fn write_is_atomic_and_guards_overwrite_and_parents() {
        let (workspace, set) = set();
        let payload = call_ok(
            &set,
            TOOL_WRITE,
            json!({"path": "a.txt", "content": "hello"}),
        );
        assert_eq!(payload["bytes_written"], json!(5));
        assert_eq!(payload["sha256_after"].as_str().map(str::len), Some(64));
        assert_eq!(
            std::fs::read_to_string(workspace.path().join("a.txt")).expect("read"),
            "hello"
        );
        assert!(payload["sha256_before"].is_null());

        // 已存在 + overwrite=false → file_exists（附 size/sha256）。
        let error = call_err(
            &set,
            TOOL_WRITE,
            json!({"path": "a.txt", "content": "other"}),
        );
        assert_eq!(error.reason, "file_exists");
        assert!(error.message.contains("sha256="), "{}", error.message);
        assert_eq!(
            std::fs::read_to_string(workspace.path().join("a.txt")).expect("read"),
            "hello"
        );

        // overwrite=true → 覆盖成功并带 before/after。
        let payload = call_ok(
            &set,
            TOOL_WRITE,
            json!({"path": "a.txt", "content": "next", "overwrite": true}),
        );
        assert_eq!(payload["sha256_before"].as_str().map(str::len), Some(64));
        assert_eq!(
            std::fs::read_to_string(workspace.path().join("a.txt")).expect("read"),
            "next"
        );

        // 父目录缺失 → not_found；create_parents=true → 创建并写入。
        assert_eq!(
            call_err(&set, TOOL_WRITE, json!({"path": "x/y.txt", "content": "v"})).reason,
            "not_found"
        );
        call_ok(
            &set,
            TOOL_WRITE,
            json!({"path": "x/y.txt", "content": "v", "create_parents": true}),
        );
        assert!(workspace.path().join("x/y.txt").is_file());

        // 输入上限。
        let huge = "z".repeat(BUILTIN_WRITE_MAX_BYTES + 1);
        assert_eq!(
            call_err(&set, TOOL_WRITE, json!({"path": "h.txt", "content": huge})).reason,
            "too_large"
        );
    }

    #[test]
    fn write_and_edit_and_patch_reject_allow_path_writes() {
        let workspace = TempDir::new().expect("workspace");
        let outside = TempDir::new().expect("outside");
        let outside_canonical = std::fs::canonicalize(outside.path()).expect("canonical");
        std::fs::write(outside.path().join("ro.txt"), "old").expect("ro");
        let set = BuiltinToolSet::new(
            &std::fs::canonicalize(workspace.path()).expect("canonical"),
            &[outside_canonical],
        )
        .expect("set");
        let path = std::fs::canonicalize(outside.path())
            .expect("canonical")
            .join("ro.txt")
            .display()
            .to_string();
        assert_eq!(
            call_err(&set, TOOL_WRITE, json!({"path": path, "content": "x"})).reason,
            "path_read_only"
        );
        assert_eq!(
            call_err(
                &set,
                TOOL_EDIT,
                json!({"path": path, "old_string": "old", "new_string": "new"})
            )
            .reason,
            "path_read_only"
        );
        // allow-path 可读。
        let payload = call_ok(&set, TOOL_READ, json!({"path": path}));
        assert_eq!(payload["content"], json!("old"));
    }

    #[test]
    fn edit_requires_unique_match_or_replace_all_and_never_partially_writes() {
        let (workspace, set) = set();
        std::fs::write(workspace.path().join("e.txt"), "a\nb\na\n").expect("write");
        let multiple = call_err(
            &set,
            TOOL_EDIT,
            json!({"path": "e.txt", "old_string": "a", "new_string": "A"}),
        );
        assert_eq!(multiple.reason, "multiple_matches");
        assert!(multiple.message.contains('2'));
        assert_eq!(
            std::fs::read_to_string(workspace.path().join("e.txt")).expect("read"),
            "a\nb\na\n"
        );
        assert_eq!(
            call_err(
                &set,
                TOOL_EDIT,
                json!({"path": "e.txt", "old_string": "zzz", "new_string": "x"})
            )
            .reason,
            "no_match"
        );
        let payload = call_ok(
            &set,
            TOOL_EDIT,
            json!({"path": "e.txt", "old_string": "a", "new_string": "A", "replace_all": true}),
        );
        assert_eq!(payload["replacements"], json!(2));
        assert_eq!(
            std::fs::read_to_string(workspace.path().join("e.txt")).expect("read"),
            "A\nb\nA\n"
        );
        let payload = call_ok(
            &set,
            TOOL_EDIT,
            json!({"path": "e.txt", "old_string": "b", "new_string": "B"}),
        );
        assert_eq!(payload["replacements"], json!(1));
    }

    #[test]
    fn diff_previews_without_writing_and_caps_result() {
        let (workspace, set) = set();
        std::fs::write(workspace.path().join("d.txt"), "one\ntwo\n").expect("write");
        let payload = call_ok(
            &set,
            TOOL_DIFF,
            json!({"path": "d.txt", "old_string": "two", "new_string": "TWO"}),
        );
        assert_eq!(payload["changed"], json!(true));
        let diff = payload["diff"].as_str().expect("diff");
        assert!(diff.contains("-two"), "{diff}");
        assert!(diff.contains("+TWO"), "{diff}");
        // diff 不写文件。
        assert_eq!(
            std::fs::read_to_string(workspace.path().join("d.txt")).expect("read"),
            "one\ntwo\n"
        );
        let empty = call_ok(
            &set,
            TOOL_DIFF,
            json!({"path": "d.txt", "old_string": "two", "new_string": "two"}),
        );
        assert_eq!(empty["changed"], json!(false));
        assert_eq!(empty["diff"], json!(""));
    }

    #[test]
    fn apply_patch_is_strictly_atomic_across_files() {
        let (workspace, set) = set();
        std::fs::write(workspace.path().join("a.txt"), "a1\na2\n").expect("a");
        std::fs::write(workspace.path().join("b.txt"), "b1\nb2\n").expect("b");
        let patch = "--- a/a.txt\n+++ b/a.txt\n@@ -1,2 +1,2 @@\n a1\n-a2\n+A2\n--- a/b.txt\n+++ b/b.txt\n@@ -1,2 +1,2 @@\n b1\n-B2\n+B2new\n";
        // 第二个文件不匹配 → 任一文件都不得修改（阶段 1 失败）。
        let error = call_err(&set, TOOL_APPLY_PATCH, json!({"patch": patch, "strip": 1}));
        assert_eq!(error.reason, "patch_mismatch");
        assert_eq!(
            std::fs::read_to_string(workspace.path().join("a.txt")).expect("a"),
            "a1\na2\n"
        );
        assert_eq!(
            std::fs::read_to_string(workspace.path().join("b.txt")).expect("b"),
            "b1\nb2\n"
        );
        // 修正第二个 hunk → 多文件成功。
        let patch = "--- a/a.txt\n+++ b/a.txt\n@@ -1,2 +1,2 @@\n a1\n-a2\n+A2\n--- a/b.txt\n+++ b/b.txt\n@@ -1,2 +1,2 @@\n b1\n-b2\n+B2NEW\n";
        let payload = call_ok(&set, TOOL_APPLY_PATCH, json!({"patch": patch}));
        assert_eq!(payload["total_added"], json!(2));
        assert_eq!(payload["total_removed"], json!(2));
        assert_eq!(payload["files"].as_array().expect("files").len(), 2);
        assert_eq!(
            std::fs::read_to_string(workspace.path().join("a.txt")).expect("a"),
            "a1\nA2\n"
        );
        assert_eq!(
            std::fs::read_to_string(workspace.path().join("b.txt")).expect("b"),
            "b1\nB2NEW\n"
        );
    }

    #[test]
    fn apply_patch_rejects_unsupported_forms() {
        let (_workspace, set) = set();
        let patch = "--- /dev/null\n+++ b/new.txt\n@@ -0,0 +1 @@\n+x\n";
        assert_eq!(
            call_err(&set, TOOL_APPLY_PATCH, json!({"patch": patch})).reason,
            "unsupported_patch"
        );
    }

    #[test]
    fn tools_reject_path_escape() {
        let (_workspace, set) = set();
        assert_eq!(
            call_err(&set, TOOL_READ, json!({"path": "../etc/hosts"})).reason,
            "path_escape"
        );
    }
    #[test]
    fn tool_set_validates_workspace_and_allow_paths() {
        let missing = TempDir::new().expect("temp");
        let error = match BuiltinToolSet::new(&missing.path().join("missing"), &[]) {
            Ok(_) => panic!("missing workspace must fail"),
            Err(error) => error,
        };
        assert_eq!(error.kind, zgraph_kernel::HarnessErrorKind::ConfigLoad);
        assert_eq!(
            error.detail["reason"],
            serde_json::json!("workspace_invalid")
        );

        let workspace = TempDir::new().expect("workspace");
        let file = workspace.path().join("not-a-dir.txt");
        std::fs::write(&file, "x").expect("write");
        let error = match BuiltinToolSet::new(
            &std::fs::canonicalize(workspace.path()).expect("canonical"),
            &[file],
        ) {
            Ok(_) => panic!("file allow-path must fail"),
            Err(error) => error,
        };
        assert_eq!(error.kind, zgraph_kernel::HarnessErrorKind::ConfigLoad);
        assert_eq!(
            error.detail["reason"],
            serde_json::json!("allow_path_invalid")
        );
    }
    #[cfg(unix)]
    #[test]
    fn apply_patch_write_failure_leaves_no_changes_or_temp_files() {
        use std::os::unix::fs::PermissionsExt;

        let (workspace, set) = set();
        std::fs::create_dir_all(workspace.path().join("locked")).expect("locked");
        std::fs::write(workspace.path().join("locked/a.txt"), "a1\n").expect("a");
        std::fs::write(workspace.path().join("b.txt"), "b1\n").expect("b");
        // 目标目录只读 → 阶段 2 写临时文件失败 → 阶段 1 已应用的 b.txt 也不得落盘。
        let mut permissions = std::fs::metadata(workspace.path().join("locked"))
            .expect("meta")
            .permissions();
        permissions.set_mode(0o555);
        std::fs::set_permissions(workspace.path().join("locked"), permissions).expect("chmod");
        let patch = "--- a/locked/a.txt\n+++ b/locked/a.txt\n@@ -1 +1 @@\n-a1\n+A1\n--- a/b.txt\n+++ b/b.txt\n@@ -1 +1 @@\n-b1\n+B1\n";
        let error = call_err(&set, TOOL_APPLY_PATCH, json!({"patch": patch}));
        assert_eq!(error.reason, "io_error", "{error:?}");
        assert_eq!(
            std::fs::read_to_string(workspace.path().join("locked/a.txt")).expect("a"),
            "a1\n"
        );
        assert_eq!(
            std::fs::read_to_string(workspace.path().join("b.txt")).expect("b"),
            "b1\n"
        );
        // 清理临时文件：目录内不应残留 `.zgraph-tmp-*`。
        let leftovers: Vec<String> = std::fs::read_dir(workspace.path())
            .expect("read dir")
            .filter_map(Result::ok)
            .map(|entry| entry.file_name().to_string_lossy().to_string())
            .filter(|name| name.contains(".zgraph-tmp-"))
            .collect();
        assert!(leftovers.is_empty(), "{leftovers:?}");
        // 恢复权限以便 TempDir 清理。
        let mut permissions = std::fs::metadata(workspace.path().join("locked"))
            .expect("meta")
            .permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(workspace.path().join("locked"), permissions).expect("chmod");
    }

    #[test]
    fn diff_truncates_large_diff_with_marker() {
        let (workspace, set) = set();
        let original: String = (0..2_500)
            .map(|index| format!("line-{index}\n"))
            .collect::<Vec<_>>()
            .join("");
        std::fs::write(workspace.path().join("big.txt"), &original).expect("write");
        let replacement: String = (0..2_500)
            .map(|index| format!("LINE-{index}\n"))
            .collect::<Vec<_>>()
            .join("");
        let payload = call_ok(
            &set,
            TOOL_DIFF,
            json!({"path": "big.txt", "old_string": original, "new_string": replacement}),
        );
        assert_eq!(payload["changed"], json!(true));
        assert_eq!(payload["truncated"], json!(true));
        assert!(payload["diff"]
            .as_str()
            .expect("diff")
            .ends_with("…[truncated]"));
    }
    #[test]
    fn edit_and_apply_patch_report_not_found_for_missing_targets() {
        let (workspace, set) = set();
        std::fs::write(workspace.path().join("a.txt"), "a1\n").expect("a");
        assert_eq!(
            call_err(
                &set,
                TOOL_EDIT,
                json!({"path": "missing.txt", "old_string": "x", "new_string": "y"})
            )
            .reason,
            "not_found"
        );
        let patch = "--- a/missing.txt\n+++ b/missing.txt\n@@ -1 +1 @@\n-a\n+b\n--- a/a.txt\n+++ b/a.txt\n@@ -1 +1 @@\n-a1\n+A1\n";
        let error = call_err(&set, TOOL_APPLY_PATCH, json!({"patch": patch}));
        assert_eq!(error.reason, "not_found", "{error:?}");
        assert_eq!(
            std::fs::read_to_string(workspace.path().join("a.txt")).expect("a"),
            "a1\n"
        );
    }

    #[test]
    fn diff_rejects_oversized_inputs() {
        let (workspace, set) = set();
        std::fs::write(workspace.path().join("d.txt"), "x").expect("write");
        let huge = "y".repeat(BUILTIN_WRITE_MAX_BYTES + 1);
        assert_eq!(
            call_err(
                &set,
                TOOL_DIFF,
                json!({"path": "d.txt", "old_string": "x", "new_string": huge})
            )
            .reason,
            "too_large"
        );
    }

    #[cfg(unix)]
    #[test]
    fn apply_patch_second_temp_failure_cleans_first_temp_and_keeps_files() {
        use std::os::unix::fs::PermissionsExt;

        let (workspace, set) = set();
        std::fs::create_dir_all(workspace.path().join("locked")).expect("locked");
        std::fs::write(workspace.path().join("a.txt"), "a1\n").expect("a");
        std::fs::write(workspace.path().join("locked/b.txt"), "b1\n").expect("b");
        let mut permissions = std::fs::metadata(workspace.path().join("locked"))
            .expect("meta")
            .permissions();
        permissions.set_mode(0o555);
        std::fs::set_permissions(workspace.path().join("locked"), permissions).expect("chmod");
        let patch = "--- a/a.txt\n+++ b/a.txt\n@@ -1 +1 @@\n-a1\n+A1\n--- a/locked/b.txt\n+++ b/locked/b.txt\n@@ -1 +1 @@\n-b1\n+B1\n";
        let error = call_err(&set, TOOL_APPLY_PATCH, json!({"patch": patch}));
        assert_eq!(error.reason, "io_error", "{error:?}");
        assert_eq!(
            std::fs::read_to_string(workspace.path().join("a.txt")).expect("a"),
            "a1\n"
        );
        // 首个文件已创建的 temp 必须清理（workspace 根与 locked/ 均无残留）。
        for dir in [
            workspace.path().to_path_buf(),
            workspace.path().join("locked"),
        ] {
            let leftovers: Vec<String> = std::fs::read_dir(&dir)
                .expect("read dir")
                .filter_map(Result::ok)
                .map(|entry| entry.file_name().to_string_lossy().to_string())
                .filter(|name| name.contains(".zgraph-tmp-"))
                .collect();
            assert!(leftovers.is_empty(), "{dir:?}: {leftovers:?}");
        }
        let mut permissions = std::fs::metadata(workspace.path().join("locked"))
            .expect("meta")
            .permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(workspace.path().join("locked"), permissions).expect("chmod");
    }

    #[test]
    fn atomic_write_removes_temp_when_rename_fails() {
        let (workspace, _set) = set();
        // 目标路径是目录 → rename 失败；temp 必须被删除。
        std::fs::create_dir_all(workspace.path().join("target")).expect("dir");
        let error = atomic_write(&workspace.path().join("target"), b"x", None)
            .expect_err("rename onto directory fails");
        assert!(!error.to_string().is_empty());
        let leftovers: Vec<String> = std::fs::read_dir(workspace.path())
            .expect("read dir")
            .filter_map(Result::ok)
            .map(|entry| entry.file_name().to_string_lossy().to_string())
            .filter(|name| name.contains(".zgraph-tmp-"))
            .collect();
        assert!(leftovers.is_empty(), "{leftovers:?}");
    }

    #[cfg(unix)]
    #[test]
    fn rollback_restores_original_content_and_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let (workspace, _set) = set();
        let first = workspace.path().join("f1.txt");
        let second = workspace.path().join("f2.txt");
        std::fs::write(&first, "orig1\n").expect("f1");
        std::fs::write(&second, "orig2\n").expect("f2");
        std::fs::set_permissions(&first, std::fs::Permissions::from_mode(0o600)).expect("chmod");
        let permissions = std::fs::metadata(&first)
            .ok()
            .map(|meta| meta.permissions());
        let staged = vec![
            StagedFile {
                target: first.clone(),
                display_path: "f1.txt".to_string(),
                original: "orig1\n".to_string(),
                updated: "new1\n".to_string(),
                added: 1,
                removed: 1,
                temp: None,
                permissions: permissions.clone(),
            },
            StagedFile {
                target: second.clone(),
                display_path: "f2.txt".to_string(),
                original: "orig2\n".to_string(),
                updated: "new2\n".to_string(),
                added: 1,
                removed: 1,
                temp: Some(workspace.path().join(".f2.txt.zgraph-tmp-test")),
                permissions: None,
            },
        ];
        // 模拟 f1 已 rename 为新内容，f2 仍有 temp。
        std::fs::write(&first, "new1\n").expect("simulate rename");
        let temp = staged[1].temp.clone().expect("temp");
        std::fs::write(&temp, "new2\n").expect("temp file");
        rollback_renamed(&staged, &[0]);
        let mut staged_mut = staged;
        cleanup_temps(&mut staged_mut);
        assert_eq!(
            std::fs::read_to_string(&first).expect("f1"),
            "orig1\n",
            "rollback restores original content"
        );
        assert!(!temp.exists(), "cleanup_temps removes pending temp files");
        let mode = std::fs::metadata(&first)
            .expect("meta")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600, "rollback restores original permissions");
    }
}
