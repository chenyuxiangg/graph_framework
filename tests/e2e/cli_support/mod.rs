//! CLI e2e 公共辅助：**真实 `zgraph` 二进制**执行 + SQLite 直接查询。
//!
//! # 二进制发现顺序（Lead 口径）
//!
//! 1. 环境变量 `ZGRAPH_BIN`（CI/调试显式指定）；
//! 2. 由 `current_exe()` 推导 `target/{debug|release}/zgraph`（存在且可执行）；
//! 3. 缺失则在测试内用 `env!("CARGO")` + 继承环境执行一次
//!    `cargo build -p zgraph-cli --bin zgraph --quiet`，仍失败则 panic 并给出
//!    「请先 cargo build -p zgraph-cli」的明确提示（不跳过用例、不静默降级）。
//!
//! `CARGO_BIN_EXE_zgraph` 仅由 cargo 在 **zgraph-cli 包自身**的 integration test 中
//! 设置；`tests/e2e` 是独立包（zgraph-cli 只是 dev-dependency），因此这里不能依赖它。
//!
//! # 为什么直接用 rusqlite
//!
//! 验收需要断言「坏图/坏输入不产生 run」「L2 replay 不写新 run」「标定字段落库」等
//! 存储侧事实；`zgraph-trace` 的只读 reader 按 `run_id` 查询、无跨 run 列表 API，
//! 因此用与 trace crate 同版本的 workspace `rusqlite` 直接只读查询（Lead 批准的
//! 跨模块 dev-dep；仅测试断言/篡改，不进入产品代码路径）。
//!
//! 另外提供 CLI e2e 的图/脚本/资产临时构造（AC-01 参数、慢 fake LLM、agent 资产、
//! L2 marker py 覆盖包）与运行中 DB 轮询，供 `cli_ac01.rs` / `cli_replay.rs` 复用。

#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::OnceLock;
use std::time::Duration;

use rusqlite::OpenFlags;
use serde_json::{json, Value};

/// 仓库根（本辅助位于 `<root>/tests/e2e/cli_support`）。
pub fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("repo root resolves")
}

/// 仓库 Python SDK 路径（`python/zgraph_worker`）。
pub fn python_sdk_dir() -> PathBuf {
    repo_root().join("python")
}

/// Python 解释器：`ZGRAPH_PYTHON` 优先，缺省 `python3`（与 e2e 口径一致）。
pub fn python_program() -> String {
    std::env::var("ZGRAPH_PYTHON").unwrap_or_else(|_| "python3".to_string())
}

/// AC-01 图（`tests/fixtures/graphs/customer_ticket.json`）。
pub fn ac01_graph() -> PathBuf {
    repo_root().join("tests/fixtures/graphs/customer_ticket.json")
}

/// AC-01 资产根（agents/prompts/tools/fake_llm/py）。
pub fn ac01_dir() -> PathBuf {
    repo_root().join("tests/fixtures/graphs/customer_ticket")
}

/// `zgraph` 二进制路径（发现顺序见模块文档；进程内只解析/构建一次）。
pub fn zgraph_binary() -> &'static Path {
    static BINARY: OnceLock<PathBuf> = OnceLock::new();
    BINARY.get_or_init(resolve_binary)
}

/// 测试二进制所在 profile 目录（`<target>/debug` 或 `<target>/release`）。
fn profile_dir() -> PathBuf {
    let exe = std::env::current_exe().expect("current_exe");
    // <target>/<profile>/deps/<test-bin> -> <target>/<profile>
    exe.parent()
        .and_then(Path::parent)
        .map(Path::to_path_buf)
        .expect("test binary lives in <target>/<profile>/deps")
}

/// 可执行文件名（Windows 带 `.exe`）。
fn binary_name() -> &'static str {
    if cfg!(windows) {
        "zgraph.exe"
    } else {
        "zgraph"
    }
}

/// 解析 `zgraph` 路径（见模块文档的 3 步顺序）。
fn resolve_binary() -> PathBuf {
    if let Ok(explicit) = std::env::var("ZGRAPH_BIN") {
        let path = PathBuf::from(explicit);
        assert!(
            path.is_file(),
            "ZGRAPH_BIN 指向的二进制不存在: {}",
            path.display()
        );
        return path;
    }
    let candidate = profile_dir().join(binary_name());
    if candidate.is_file() {
        return candidate;
    }
    build_zgraph_binary();
    let candidate = profile_dir().join(binary_name());
    assert!(
        candidate.is_file(),
        "未找到 zgraph 二进制（{}）；请先运行 `cargo build -p zgraph-cli --bin zgraph`",
        candidate.display()
    );
    candidate
}

/// 测试内触发一次 `cargo build -p zgraph-cli --bin zgraph`（继承环境 + 显式 target dir）。
fn build_zgraph_binary() {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());
    let target_dir = profile_dir()
        .parent()
        .map(Path::to_path_buf)
        .expect("profile dir has parent");
    let output = Command::new(&cargo)
        .args(["build", "--quiet", "-p", "zgraph-cli", "--bin", "zgraph"])
        .current_dir(repo_root())
        .env("CARGO_TARGET_DIR", &target_dir)
        .output()
        .unwrap_or_else(|error| {
            panic!(
                "无法执行 `{cargo} build -p zgraph-cli --bin zgraph`（{error}）；\
                 请先在仓库根执行该命令构建 CLI 二进制"
            )
        });
    assert!(
        output.status.success(),
        "`cargo build -p zgraph-cli --bin zgraph` 失败（exit={:?}）；请先手动构建 CLI 二进制\n\
         stdout: {}\nstderr: {}",
        output.status.code(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Python 字节码缓存开关（ISSUE-089）：e2e 派生的 Python 子进程一律不写
/// `__pycache__`，避免运行 fixtures 时在仓库里留下忽略文件。
pub const PYTHONDONTWRITEBYTECODE: &str = "PYTHONDONTWRITEBYTECODE";

/// 构造 `zgraph` 子进程（已注入 e2e 统一环境）。
///
/// CLI 以**继承环境** spawn Python worker，因此在 CLI 进程上设置
/// `PYTHONDONTWRITEBYTECODE=1` 即可覆盖其所有后代（worker / tool runner）。
/// 需要直接 spawn CLI（而非捕获输出）的用例应复用本函数，例如崩溃标定用例。
pub fn zgraph_command() -> Command {
    let mut command = Command::new(zgraph_binary());
    command.env(PYTHONDONTWRITEBYTECODE, "1");
    command
}

/// 运行 `zgraph` 并捕获输出；`cwd` 为子进程工作目录。
pub fn run_cli(args: &[String], cwd: &Path) -> Output {
    run_cli_env(args, cwd, &[])
}

/// 带额外环境变量运行 `zgraph`。
pub fn run_cli_env(args: &[String], cwd: &Path, envs: &[(&str, &str)]) -> Output {
    run_cli_full(args, cwd, envs, &[])
}

/// 带额外环境变量 / 移除环境变量运行 `zgraph`。
pub fn run_cli_full(
    args: &[String],
    cwd: &Path,
    envs: &[(&str, &str)],
    removed: &[&str],
) -> Output {
    let mut command = zgraph_command();
    command
        .args(args)
        .envs(envs.iter().copied())
        .current_dir(cwd);
    for name in removed {
        command.env_remove(name);
    }
    command.output().expect("spawn zgraph binary")
}

/// 路径转 CLI 参数。
pub fn arg(path: &Path) -> String {
    path.display().to_string()
}

/// 断言退出码，失败时附带 stdout/stderr。
pub fn assert_exit(output: &Output, expected: i32, context: &str) {
    let actual = output.status.code();
    assert_eq!(
        actual,
        Some(expected),
        "{context}: exit={actual:?}\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        stderr_text(output)
    );
}

/// stdout 必须是一份完整 JSON（`--json` 契约）。
pub fn json_stdout(output: &Output) -> Value {
    let text = String::from_utf8_lossy(&output.stdout);
    serde_json::from_str(text.trim()).unwrap_or_else(|error| {
        panic!(
            "stdout is not JSON ({error}): {text}\nstderr: {}",
            stderr_text(output)
        )
    })
}

/// stderr 文本。
pub fn stderr_text(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

/// 打开 trace SQLite（读写；仅测试断言/篡改用）。
pub fn connect(db: &Path) -> rusqlite::Connection {
    rusqlite::Connection::open(db).expect("open trace db")
}

/// `runs` 表行数（断言「不产生 run」）。
///
/// 用**只读**连接：断言本身不得成为 WAL 写者（ISSUE-089：读-写连接会触碰
/// `-wal`/`-shm`，污染「replay 前后 DB 字节不变」的证据链）。
pub fn runs_count(db: &Path) -> i64 {
    read_only_connect(db)
        .query_row("SELECT COUNT(*) FROM runs", [], |row| row.get(0))
        .expect("count runs")
}

/// 单 run 的 `runs.status`（标定/重放断言）。
pub fn run_status(db: &Path, run_id: &str) -> Option<String> {
    connect(db)
        .query_row(
            "SELECT status FROM runs WHERE run_id = ?1",
            [run_id],
            |row| row.get(0),
        )
        .ok()
}

// ---------------------------------------------------------------------------
// 图 / 脚本 / 资产临时构造
// ---------------------------------------------------------------------------

/// 写文本文件（自动创建父目录）。
pub fn write_text(dir: &Path, name: &str, text: &str) -> PathBuf {
    let path = dir.join(name);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("create parent dir");
    }
    std::fs::write(&path, text).expect("write text file");
    path
}

/// 写 JSON 文件（自动创建父目录）。
pub fn write_json(dir: &Path, name: &str, value: &Value) -> PathBuf {
    let text = serde_json::to_string_pretty(value).expect("serialize json");
    write_text(dir, name, &text)
}

/// `start(x) -> end(r)` 内建图（不 spawn worker；恢复标定用例的第二次 run）。
pub fn io_graph_text() -> String {
    json!({
        "version": "1.0",
        "metadata": {"name": "cli_io"},
        "entry_points": ["start"],
        "nodes": [
            {"id": "start", "type": "start",
             "inputs": [{"name": "x", "type": "any", "required": true}], "config": {}},
            {"id": "end", "type": "end",
             "inputs": [{"name": "r", "type": "any", "required": true}], "config": {}}
        ],
        "edges": [{"from": "start", "to_node": "end", "to_input_port": "r"}]
    })
    .to_string()
}

/// `start(message) -> agent_call(agent_id) -> end(result)` 图。
///
/// `declared_timeout_ms=None` → DSL 不声明 `timeout_ms`（ISSUE-076 回退链用例）。
pub fn agent_graph_text(agent_id: &str, declared_timeout_ms: Option<u64>) -> String {
    let mut config = json!({"agent_id": agent_id});
    if let Some(timeout_ms) = declared_timeout_ms {
        config["timeout_ms"] = json!(timeout_ms);
    }
    json!({
        "version": "1.0",
        "metadata": {"name": "cli_agent"},
        "entry_points": ["start"],
        "nodes": [
            {"id": "start", "type": "start",
             "inputs": [{"name": "message", "type": "string", "required": true}],
             "config": {}},
            {"id": "worker", "type": "agent_call",
             "inputs": [{"name": "message", "type": "any", "required": true}],
             "trigger": "all",
             "config": config},
            {"id": "end", "type": "end",
             "inputs": [{"name": "result", "type": "any", "required": true}],
             "config": {}}
        ],
        "edges": [
            {"from": "start", "to_node": "worker", "to_input_port": "message"},
            {"from": "worker", "to_node": "end", "to_input_port": "result"}
        ]
    })
    .to_string()
}

/// 慢响应 fake LLM 脚本：每 `chunk_chars` 个字符等 `delay_ms`（总时长 ≈ chars/chunk*delay）。
pub fn slow_text_script(chunk_delay_ms: u64, response_chars: usize) -> Value {
    json!({
        "version": 1,
        "chunk_chars": 3,
        "chunk_delay_ms": chunk_delay_ms,
        "calls": [{
            "kind": "text",
            "content": "x".repeat(response_chars),
            "stop_reason": "EndTurn",
            "usage": {"prompt_tokens": 10, "completion_tokens": 2}
        }],
        "failures": []
    })
}

/// 临时 agent 资产根：`agents/<agent_id>.json`（含 node_timeout_ms）+ `agents/prompt.md` + `tools/`。
pub fn write_agent_assets(dir: &Path, agent_id: &str, node_timeout_ms: u64) -> PathBuf {
    let assets = dir.join(format!("assets_{agent_id}"));
    let agents = assets.join("agents");
    std::fs::create_dir_all(&agents).expect("create agents dir");
    std::fs::create_dir_all(assets.join("tools")).expect("create tools dir");
    let config = json!({
        "id": agent_id,
        "name": format!("e2e 临时 agent {agent_id}"),
        "schema_version": 1,
        "model": {
            "provider": "deepseek",
            "name": "deepseek-chat",
            "base_url": null,
            "api_key_env": "DEEPSEEK_API_KEY",
            "temperature": 0.0,
            "max_tokens": 512,
            "context_window_tokens": 65536
        },
        "role_prompt_file": "prompt.md",
        "constraints_file": null,
        "tools": [],
        "input_mapping": {"message": "message"},
        "max_iterations": 4,
        "llm_timeout_ms": 60000,
        "llm_max_retries": 1,
        "node_timeout_ms": node_timeout_ms
    });
    write_json(&agents, &format!("{agent_id}.json"), &config);
    write_text(
        &agents,
        "prompt.md",
        "你是 e2e 临时 agent（行为由 fake LLM 脚本决定）。",
    );
    assets
}

/// L2 marker 的 py 覆盖包：`gates/hr.py` 在正常执行时把 worker pid 追加到
/// `ZGRAPH_L2_MARKER_LOG`；`zgraph replay --mode=outputs` 不执行 python 节点，
/// 因此 marker 行数保持不变 = 「重放未启动 worker」的可判定证据（不用 `ps`）。
pub fn write_l2_marker_py(dir: &Path) -> PathBuf {
    let root = dir.join("py_override");
    write_text(
        &root.join("gates"),
        "__init__.py",
        "\"\"\"L2 marker 覆盖包（e2e）。\"\"\"\n",
    );
    write_text(
        &root.join("gates"),
        "hr.py",
        r#""""L2 replay e2e：AC-01 门控前置节点 + worker pid marker。

正常执行时把当前 worker 进程 pid 追加到环境变量 ``ZGRAPH_L2_MARKER_LOG`` 指向的文件；
``zgraph replay --mode=outputs`` 用 RecordedExecutor 直接返回记录输出、不执行本节点，
因此 marker 行数在 replay 前后保持不变。
"""

from __future__ import annotations

import json
import os
from typing import Any

from zgraph_worker import WorkerContext

MARKER_ENV: str = "ZGRAPH_L2_MARKER_LOG"


def _need_human(inputs: dict[str, Any]) -> bool:
    """从上游 agent 四字段整包的 ``content`` 解析 ``need_human``（缺省 False）。"""
    package: Any = inputs.get("answer") or {}
    content: Any = package.get("content") if isinstance(package, dict) else None
    if not isinstance(content, str):
        return False
    try:
        parsed: Any = json.loads(content)
    except json.JSONDecodeError:
        return False
    if isinstance(parsed, dict) and isinstance(parsed.get("need_human"), bool):
        return parsed["need_human"]
    return False


def need_human_check(inputs: dict[str, Any], ctx: WorkerContext) -> dict[str, Any]:
    """记录 worker pid marker 后返回 ``{"need_human": bool}``。"""
    marker: str | None = os.environ.get(MARKER_ENV)
    if marker:
        with open(marker, "a", encoding="utf-8") as handle:
            handle.write(f"{os.getpid()}\n")
    return {"need_human": _need_human(inputs)}
"#,
    );
    root
}

// ---------------------------------------------------------------------------
// 运行中 DB 轮询 / 存储侧断言
// ---------------------------------------------------------------------------

/// 只读打开 trace DB（带 busy_timeout；运行中的 DB 可安全轮询）。
pub fn read_only_connect(db: &Path) -> rusqlite::Connection {
    let conn = rusqlite::Connection::open_with_flags(db, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .expect("open read-only trace db");
    conn.busy_timeout(Duration::from_secs(5))
        .expect("set busy_timeout");
    conn
}

/// 当前 `Running`/`Paused` run 的 `(run_id, owner_pid)`（无则 `None`；DB 未就绪也 `None`）。
pub fn running_run(db: &Path) -> Option<(String, i64)> {
    if !db.is_file() {
        return None;
    }
    let conn = read_only_connect(db);
    conn.query_row(
        "SELECT run_id, COALESCE(owner_pid, 0) FROM runs \
         WHERE status IN ('Running','Paused') ORDER BY rowid DESC LIMIT 1",
        [],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )
    .ok()
}

/// 某 run 的 `recorded_outputs` 行数（L2 记录完整性）。
pub fn recorded_outputs_count(db: &Path, run_id: &str) -> i64 {
    read_only_connect(db)
        .query_row(
            "SELECT COUNT(*) FROM recorded_outputs WHERE run_id = ?1",
            [run_id],
            |row| row.get(0),
        )
        .expect("count recorded_outputs")
}

/// 某 run 的 `runs.output_json`（L2 输出一致性对比）。
pub fn run_output_json(db: &Path, run_id: &str) -> Option<String> {
    read_only_connect(db)
        .query_row(
            "SELECT output_json FROM runs WHERE run_id = ?1",
            [run_id],
            |row| row.get(0),
        )
        .ok()
        .flatten()
}

/// trace DB 的**全部磁盘字节**：主文件 + WAL 模式下可能承载写入的 `-wal`/`-shm`。
///
/// 返回 `(后缀, 字节)`：`""` = 主文件、`"-wal"`/`"-shm"` = 伴生文件；文件不存在记
/// `None`（这样「伴生文件从无到有 / 从有到无」也能被快照比较发现）。
///
/// ISSUE-089：只比主文件会漏掉「replay 只写 WAL 不落主文件」的情况；因此
/// 「不修改原 trace」的证据必须覆盖伴生文件。快照只做文件 IO、**不打开数据库连接**
/// （读连接在 WAL 模式下也会更新 `-shm`），调用方必须先在数据库查询之前取快照。
pub fn db_files_snapshot(db: &Path) -> Vec<(String, Option<Vec<u8>>)> {
    const SIDECARS: [&str; 3] = ["", "-wal", "-shm"];
    SIDECARS
        .iter()
        .map(|suffix| {
            let mut name = db.as_os_str().to_os_string();
            name.push(suffix);
            let path = PathBuf::from(name);
            (suffix.to_string(), std::fs::read(path).ok())
        })
        .collect()
}

/// 读一个文本文件为行列表（marker 日志等）。
pub fn read_lines(path: &Path) -> Vec<String> {
    std::fs::read_to_string(path)
        .unwrap_or_else(|error| panic!("读取 {} 失败: {error}", path.display()))
        .lines()
        .map(str::to_string)
        .collect()
}

// ---------------------------------------------------------------------------
// AC-01 参数构造（cli_ac01 / cli_replay 共用）
// ---------------------------------------------------------------------------

/// AC-01 图 `zgraph run` 参数。
///
/// - `record`：`none|hash|full`（L2 重放要求 `full`）；
/// - `extra_python_paths`：**先于** fixture `py/` 的 Python 搜索路径（L2 marker 覆盖用）。
pub fn ac01_run_args(
    db: &Path,
    inputs: &str,
    record: &str,
    extra_python_paths: &[PathBuf],
) -> Vec<String> {
    let assets = ac01_dir();
    let fake_script = assets.join("fake_llm.json");
    let mut args = vec![
        "run".to_string(),
        arg(&ac01_graph()),
        "--inputs".to_string(),
        inputs.to_string(),
        "--db".to_string(),
        arg(db),
        "--assets-dir".to_string(),
        arg(&assets),
        "--tools-dir".to_string(),
        arg(&assets.join("tools")),
        "--python".to_string(),
        python_program(),
        "--python-path".to_string(),
        arg(&python_sdk_dir()),
    ];
    for path in extra_python_paths {
        args.push("--python-path".to_string());
        args.push(arg(path));
    }
    args.push("--python-path".to_string());
    args.push(arg(&assets.join("py")));
    args.push(format!("--fake-llm={}", fake_script.display()));
    args.push(format!("--record={record}"));
    args.push("--json".to_string());
    args
}
