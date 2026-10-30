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
//! 因此用与 trace crate 同版本的 workspace `rusqlite` 直接只读查询。

#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::OnceLock;

use serde_json::Value;

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
    let mut command = Command::new(zgraph_binary());
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
pub fn runs_count(db: &Path) -> i64 {
    connect(db)
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
