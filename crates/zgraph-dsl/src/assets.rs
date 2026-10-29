//! R14c：Agent 资产索引校验（按 `--assets-dir`）。
//!
//! 契约来源：`doc/spec-v1.2/02-DSL与校验.md` §2.6.3：
//! - 资产根：CLI `--assets-dir <dir>`；索引范围 `<dir>/agents/*.json`（单层、按文件名排序）。
//! - `id` 取自文件内 `"id"` 字段；**只校验存在性/可读性/id 唯一**。
//! - 未命中或索引非法 → `DSL_AGENT_NOT_FOUND`。
//!
//! 任务 M1 口径：**仅当显式提供 `--assets-dir`（或 `ZGRAPH_ASSETS`）时执行**；
//! 未提供时跳过（`zgraph validate graph.json` 无需资产目录即可工作）。

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use crate::error::{DslError, DslErrorCode};
use crate::types::{GraphNode, NodeConfig, NodeType};

/// 检查图中所有 `agent_call.agent_id` 是否命中资产索引。
///
/// - 缺失 agent：每个 `agent_id` 只报 1 条，定位到**首个**引用它的 `agent_call`
///   节点（02 §2.4.2；REV-005 复审 #4）。
/// - 索引级错误（重复 id / 非法文件）：无可用节点 → 文档级 `{}`，message 携带
///   `assets.agents[i]`（ISSUE-032）。
pub(crate) fn check_agents(nodes: &[GraphNode], assets_dir: &Path) -> Vec<DslError> {
    let agents_dir = assets_dir.join("agents");
    let index = match load_index(&agents_dir) {
        Ok(index) => index,
        Err(errors) => return errors,
    };
    let mut errors = Vec::new();
    let mut reported: BTreeSet<&str> = BTreeSet::new();
    for node in nodes {
        if node.node_type != NodeType::AgentCall {
            continue;
        }
        let NodeConfig::AgentCall(config) = &node.config else {
            continue;
        };
        if index.contains_key(&config.agent_id) || !reported.insert(config.agent_id.as_str()) {
            continue;
        }
        errors.push(agent_not_found(&config.agent_id, &agents_dir, node));
    }
    errors
}

fn load_index(agents_dir: &Path) -> Result<BTreeMap<String, PathBuf>, Vec<DslError>> {
    let mut index = BTreeMap::new();
    let mut errors = Vec::new();
    let entries = match fs::read_dir(agents_dir) {
        Ok(entries) => entries,
        // 资产目录不存在 = 空索引：该图的 agent_call 会逐个报缺失。
        Err(_) => return Ok(index),
    };
    let mut files: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| is_json_file(path))
        .collect();
    files.sort();
    for (position, path) in files.iter().enumerate() {
        match load_agent_id(path) {
            Ok(Some(id)) => {
                if let Some(previous) = index.insert(id.clone(), path.clone()) {
                    errors.push(duplicate_agent(&id, position, &previous, path));
                }
            }
            Ok(None) => errors.push(invalid_asset_file(
                position,
                path,
                "missing or empty string field 'id'",
            )),
            Err(detail) => errors.push(invalid_asset_file(position, path, &detail)),
        }
    }
    if errors.is_empty() {
        Ok(index)
    } else {
        Err(errors)
    }
}

fn is_json_file(path: &Path) -> bool {
    path.extension()
        .is_some_and(|extension| extension == "json")
}

fn load_agent_id(path: &Path) -> Result<Option<String>, String> {
    let bytes = fs::read(path).map_err(|error| format!("cannot read file: {error}"))?;
    let value: Value =
        serde_json::from_slice(&bytes).map_err(|error| format!("invalid JSON: {error}"))?;
    let id = value.get("id").and_then(Value::as_str);
    Ok(id.filter(|id| !id.is_empty()).map(str::to_string))
}

fn agent_not_found(agent_id: &str, agents_dir: &Path, node: &GraphNode) -> DslError {
    DslError::new(
        DslErrorCode::AgentNotFound,
        format!(
            "agent asset '{agent_id}' not found in index '{}'",
            agents_dir.display()
        ),
    )
    .at_node(node.id.clone())
    .expected("an agent asset registered in the provided assets index")
    .actual(agent_id)
}

fn duplicate_agent(agent_id: &str, position: usize, previous: &Path, current: &Path) -> DslError {
    DslError::new(
        DslErrorCode::AgentNotFound,
        format!(
            "assets.agents[{position}]: duplicate agent asset id '{agent_id}' (already provided by '{}')",
            previous.display()
        ),
    )
    .expected("unique agent asset id")
    .actual(json!({"agent_id": agent_id, "file": current.display().to_string()}))
}

fn invalid_asset_file(position: usize, path: &Path, detail: &str) -> DslError {
    DslError::new(
        DslErrorCode::AgentNotFound,
        format!(
            "assets.agents[{position}]: invalid agent asset file '{}': {detail}",
            path.display()
        ),
    )
    .expected("readable agent asset JSON with a non-empty 'id'")
    .actual(json!({"file": path.display().to_string()}))
}
