//! v0.5/F3b：可观测/预算 CLI 接线测试（DR-018、AC-27）。
//!
//! 覆盖：
//! - `run --pricing/--max-tokens/--max-cost` 启动前预检：pricing 缺文件/坏 JSON/非 USD、
//!   `--max-cost` 无 pricing、非法 budget → `HARNESS_CONFIG_LOAD` exit3 且**不创建 DB**；
//!   数值语法错误 → 用法错误 exit2；
//! - graph 全部 `agent_call` 模型去重收集：双模型图缺价 → fail-closed 配置错误、零请求；
//!   外部 subprocess harness + `--max-cost` → `cost_unavailable_external_harness`；
//! - 有效 pricing：run `--json.usage` additive（run/node/agent + cost，已知 usage 精确值）、
//!   人类 usage 行；`trace --stats` additive token/llm_calls/耗时/模型；
//! - 预算超限（F3a2 未合并时以 `budget_exceeded` 消息占位）：run Failed + usage.budget_stop；
//! - 未知模型价格 → warning + cost unavailable（不伪造 0）；
//! - 默认（无 flags）run/trace 输出逐字节不变（无 `usage`/`llm_calls` 键）。
//!
//! 全部用真 CLI + fake LLM（无网络）；DB/临时目录独立。

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::{json, Value};
use tempfile::TempDir;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("repo root")
}

fn python_program() -> String {
    std::env::var("ZGRAPH_PYTHON").unwrap_or_else(|_| "python3".to_string())
}

fn ac08_dir() -> PathBuf {
    repo_root().join("tests/fixtures/graphs/harness_ac08")
}

fn arg(path: &Path) -> String {
    path.display().to_string()
}

fn write_text(dir: &Path, name: &str, text: &str) -> PathBuf {
    let path = dir.join(name);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("create parent");
    }
    std::fs::write(&path, text).expect("write file");
    path
}

fn run_cli(args: &[String], cwd: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_zgraph"))
        .args(args)
        .current_dir(cwd)
        .output()
        .expect("zgraph binary")
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).to_string()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).to_string()
}

fn code(output: &Output) -> i32 {
    output.status.code().expect("exit code")
}

fn json_stdout(output: &Output) -> Value {
    serde_json::from_str(stdout(output).trim()).expect("stdout JSON")
}

fn connect(db: &Path) -> rusqlite::Connection {
    rusqlite::Connection::open(db).expect("open db")
}

fn runs_count(db: &Path) -> i64 {
    connect(db)
        .query_row("SELECT COUNT(*) FROM runs", [], |row| row.get(0))
        .expect("runs count")
}

/// AC-08 真链路基础参数（fake LLM + 真 Python worker；模型 `deepseek-chat`）。
fn ac08_args(db: &Path, extra: &[&str]) -> Vec<String> {
    let assets = ac08_dir();
    let mut args = vec![
        "run".to_string(),
        arg(&assets.join("graph.json")),
        "--db".to_string(),
        arg(db),
        "--assets-dir".to_string(),
        arg(&assets),
        "--inputs".to_string(),
        r#"{"start":{"message":"2+3"}}"#.to_string(),
        "--python".to_string(),
        python_program(),
        "--python-path".to_string(),
        arg(&repo_root().join("python")),
        format!("--fake-llm={}", assets.join("fake_llm.json").display()),
    ];
    args.extend(extra.iter().map(|value| value.to_string()));
    args
}

/// USD 价格表（deepseek-chat：in 1 / out 2 USD per 1M tokens）。
fn pricing_file(dir: &Path, models: Value) -> PathBuf {
    let text = json!({"currency": "USD", "models": models}).to_string();
    write_text(dir, "pricing.json", &text)
}

fn pricing_chat_only(dir: &Path) -> PathBuf {
    pricing_file(
        dir,
        json!({"deepseek-chat": {"input_per_mtok": 1.0, "output_per_mtok": 2.0}}),
    )
}

/// AC-08 fake-LLM 的已知 usage：120/8 + 160/12 = prompt 280 / completion 20 / total 300。
const EXPECTED_PROMPT: u64 = 280;
const EXPECTED_COMPLETION: u64 = 20;
const EXPECTED_TOTAL: u64 = 300;
/// cost = 280/1e6*1 + 20/1e6*2 = 0.00032 USD。
const EXPECTED_COST: f64 = 0.00032;

// ---------------------------------------------------------------------------
// 启动前预检（不产生 run / 不创建 DB）
// ---------------------------------------------------------------------------

#[test]
fn pricing_and_budget_preflight_errors_do_not_create_db() {
    let dir = tempfile::tempdir().expect("tempdir");
    let graph = ac08_dir().join("graph.json");
    let bad_pricing = write_text(dir.path(), "bad_pricing.json", "{not json");
    let non_usd = write_text(
        dir.path(),
        "eur_pricing.json",
        &json!({"currency": "EUR", "models": {"deepseek-chat": {"input_per_mtok": 1.0, "output_per_mtok": 2.0}}}).to_string(),
    );
    let pricing = pricing_chat_only(dir.path());
    let missing = dir.path().join("no-such-pricing.json");

    let cases: Vec<(&str, Vec<String>, &str)> = vec![
        (
            "pricing-missing-file",
            vec!["--pricing".to_string(), arg(&missing)],
            "pricing_file_not_found",
        ),
        (
            "pricing-bad-json",
            vec!["--pricing".to_string(), arg(&bad_pricing)],
            "pricing_invalid",
        ),
        (
            "pricing-non-usd",
            vec!["--pricing".to_string(), arg(&non_usd)],
            "pricing_invalid",
        ),
        (
            "max-cost-without-pricing",
            vec!["--max-cost".to_string(), "1.0".to_string()],
            "pricing_required_for_max_cost",
        ),
        (
            "max-tokens-zero",
            vec!["--max-tokens".to_string(), "0".to_string()],
            "budget_invalid",
        ),
        (
            "max-cost-zero",
            vec![
                "--pricing".to_string(),
                arg(&pricing),
                "--max-cost".to_string(),
                "0".to_string(),
            ],
            "budget_invalid",
        ),
        (
            "max-cost-negative",
            vec![
                "--pricing".to_string(),
                arg(&pricing),
                "--max-cost".to_string(),
                "-1".to_string(),
            ],
            "budget_invalid",
        ),
    ];
    for (name, extra, reason) in cases {
        let db = dir.path().join(format!("{name}.db"));
        let mut args = ac08_args(&db, &[]);
        args.extend(extra);
        let output = run_cli(&args, dir.path());
        assert_eq!(code(&output), 3, "{name}: stderr={}", stderr(&output));
        let text = stderr(&output);
        assert!(text.contains("HARNESS_CONFIG_LOAD"), "{name}: {text}");
        assert!(
            text.contains(reason),
            "{name}: 期望 reason {reason}: {text}"
        );
        assert!(!db.exists(), "{name}: 配置错误不得创建 DB");
    }
    // 数值语法错误 → 用法错误 exit2（尚未进入配置预检）。
    let db = dir.path().join("syntax.db");
    for (flag, value) in [("--max-tokens", "abc"), ("--max-cost", "x")] {
        let mut args = ac08_args(&db, &[]);
        args.extend([flag.to_string(), value.to_string()]);
        let output = run_cli(&args, dir.path());
        assert_eq!(code(&output), 2, "{flag}: stderr={}", stderr(&output));
        assert!(!db.exists(), "{flag}: 用法错误不得创建 DB");
    }
    // 确保 graph 路径有效（避免误判为 graph 错误）。
    assert!(graph.exists());
}

// ---------------------------------------------------------------------------
// 有效 pricing：run usage additive + trace --stats additive
// ---------------------------------------------------------------------------

#[test]
fn valid_pricing_reports_usage_and_trace_stats() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db = dir.path().join("trace.db");
    let pricing = pricing_chat_only(dir.path());
    let args = ac08_args(
        &db,
        &["--record=full", "--json", "--pricing", &arg(&pricing)],
    );
    let output = run_cli(&args, dir.path());
    assert_eq!(code(&output), 0, "stderr={}", stderr(&output));
    let payload = json_stdout(&output);
    let run_id = payload["run_id"].as_str().expect("run_id").to_string();
    let usage = &payload["usage"];
    assert_eq!(usage["run"]["llm_calls"], json!(2));
    assert_eq!(usage["run"]["prompt_tokens"], json!(EXPECTED_PROMPT));
    assert_eq!(
        usage["run"]["completion_tokens"],
        json!(EXPECTED_COMPLETION)
    );
    assert_eq!(usage["run"]["total_tokens"], json!(EXPECTED_TOTAL));
    let cost = usage["run"]["cost_usd"].as_f64().expect("cost");
    assert!((cost - EXPECTED_COST).abs() < 1e-9, "cost={cost}");
    assert_eq!(usage["by_agent"].as_array().expect("by_agent").len(), 1);
    assert_eq!(usage["by_agent"][0]["agent_id"], json!("answer_writer"));
    assert_eq!(usage["by_agent"][0]["model"], json!("deepseek-chat"));
    assert!(!usage["by_node"].as_array().expect("by_node").is_empty());
    assert_eq!(
        usage["by_agent"][0]["usage"]["cost_usd"],
        json!(EXPECTED_COST)
    );

    // `trace --stats --json` additive：llm_calls/token_usage/llm_models（无 F3a2 时
    // 无 cost 数据 → 不输出 cost 键）。
    let stats = run_cli(
        &[
            "trace".to_string(),
            run_id.clone(),
            "--db".to_string(),
            arg(&db),
            "--stats".to_string(),
            "--json".to_string(),
        ],
        dir.path(),
    );
    assert_eq!(code(&stats), 0, "stderr={}", stderr(&stats));
    let stats_json = json_stdout(&stats);
    assert_eq!(stats_json["llm_calls"], json!(2));
    assert_eq!(
        stats_json["token_usage"]["prompt_tokens"],
        json!(EXPECTED_PROMPT)
    );
    assert_eq!(
        stats_json["token_usage"]["completion_tokens"],
        json!(EXPECTED_COMPLETION)
    );
    assert_eq!(stats_json["llm_models"], json!(["deepseek-chat"]));
    assert!(stats_json["llm_duration_ms"].is_u64());

    // 人类 stats 行 additive。
    let stats = run_cli(
        &[
            "trace".to_string(),
            run_id,
            "--db".to_string(),
            arg(&db),
            "--stats".to_string(),
        ],
        dir.path(),
    );
    let text = stdout(&stats);
    assert!(text.contains("llm_calls 2"), "{text}");
    assert!(
        text.contains(&format!(
            "tokens prompt={EXPECTED_PROMPT} completion={EXPECTED_COMPLETION} total={EXPECTED_TOTAL}"
        )),
        "{text}"
    );
}

/// 未知模型价格：run 继续，cost 不可用 + warning（不伪造 0）。
#[test]
fn unknown_model_price_is_unavailable_not_zero() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db = dir.path().join("trace.db");
    let pricing = pricing_file(
        dir.path(),
        json!({"other-model": {"input_per_mtok": 1.0, "output_per_mtok": 1.0}}),
    );
    let args = ac08_args(&db, &["--json", "--pricing", &arg(&pricing)]);
    let output = run_cli(&args, dir.path());
    assert_eq!(code(&output), 0, "stderr={}", stderr(&output));
    let payload = json_stdout(&output);
    let usage = &payload["usage"];
    assert_eq!(usage["run"]["cost_usd"], Value::Null, "未知价不伪造 0");
    assert!(usage["run"]["unpriced_calls"].as_u64().unwrap_or(0) > 0);
    assert!(
        !usage["warnings"].as_array().expect("warnings").is_empty(),
        "未知模型价格必须有 warning: {usage}"
    );
    // 人类模式（非 --json）：usage 行 + warning 走 stderr。
    let human = run_cli(
        &ac08_args(
            &db.with_extension("human.db"),
            &["--pricing", &arg(&pricing)],
        ),
        dir.path(),
    );
    assert_eq!(code(&human), 0, "stderr={}", stderr(&human));
    assert!(stderr(&human).contains("warning:"), "{}", stderr(&human));
    assert!(
        stderr(&human).contains("cost=unavailable"),
        "{}",
        stderr(&human)
    );
}

/// 预算超限（F3a2 未合并时：harness 提前停止，kernel 暂映射 `NodeError::Internal`
/// 消息 `budget_exceeded`；F3a2 合并后应变为 `BUDGET_EXCEEDED` + stop_reason）。
#[test]
fn token_budget_stops_run_with_budget_usage() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db = dir.path().join("trace.db");
    let args = ac08_args(&db, &["--json", "--max-tokens", "1"]);
    let output = run_cli(&args, dir.path());
    assert_eq!(code(&output), 1, "stderr={}", stderr(&output));
    let payload = json_stdout(&output);
    assert_eq!(payload["status"], json!("Failed"));
    let usage = &payload["usage"];
    assert_eq!(usage["run"]["llm_calls"], json!(1), "第二次请求前停止");
    assert_eq!(
        usage["budget_stop"]["kind"],
        json!("tokens"),
        "harness 结构化停止: {usage}"
    );
    // F3a2 占位：kernel 目前把 BudgetExceeded 映射为 Internal("budget_exceeded: ...")。
    let error = &payload["error"];
    assert!(
        error["message"]
            .as_str()
            .unwrap_or_default()
            .contains("budget_exceeded")
            || error["code"] == "BUDGET_EXCEEDED",
        "F3a2 映射前/后均需可识别: {error}"
    );
}

// ---------------------------------------------------------------------------
// 多模型收集 / 外部 harness
// ---------------------------------------------------------------------------

/// 双模型资产（`parser`=deepseek-chat / `parser2`=second-model）+ 两 agent 图。
fn two_model_assets(dir: &Path) -> (PathBuf, PathBuf) {
    let assets = dir.join("assets");
    std::fs::create_dir_all(assets.join("agents")).expect("agents");
    write_text(&assets.join("agents"), "prompt.md", "你是解析器。");
    for (id, model) in [("parser", "deepseek-chat"), ("parser2", "second-model")] {
        let config = json!({
            "id": id,
            "name": "parser",
            "schema_version": 1,
            "model": {
                "provider": "deepseek",
                "name": model,
                "base_url": null,
                "api_key_env": "HARNESS_TEST_KEY",
                "temperature": 0.0,
                "max_tokens": 256,
                "context_window_tokens": 65536
            },
            "role_prompt_file": "prompt.md",
            "constraints_file": null,
            "tools": [],
            "input_mapping": {"message": "user_message"},
            "max_iterations": 2
        });
        write_text(
            &assets.join("agents"),
            &format!("{id}.json"),
            &config.to_string(),
        );
    }
    let graph = write_text(
        dir,
        "two_models.json",
        &json!({
            "version": "1.0",
            "metadata": {"name": "two-models"},
            "entry_points": ["start"],
            "nodes": [
                {"id": "start", "type": "start",
                 "inputs": [{"name": "message", "type": "any", "required": true}], "config": {}},
                {"id": "w1", "type": "agent_call",
                 "inputs": [{"name": "message", "type": "any", "required": true}],
                 "config": {"agent_id": "parser"}},
                {"id": "w2", "type": "agent_call",
                 "inputs": [{"name": "message", "type": "any", "required": true}],
                 "config": {"agent_id": "parser2"}},
                {"id": "end", "type": "end",
                 "inputs": [{"name": "result", "type": "any", "required": true}], "config": {}}
            ],
            "edges": [
                {"from": "start", "to_node": "w1", "to_input_port": "message"},
                {"from": "w1", "to_node": "w2", "to_input_port": "message"},
                {"from": "w2", "to_node": "end", "to_input_port": "result"}
            ]
        })
        .to_string(),
    );
    write_text(
        dir,
        "fake_llm.json",
        &json!({"version": 1, "chunk_chars": 64, "calls": [
            {"kind": "text", "content": "one", "stop_reason": "EndTurn",
             "usage": {"prompt_tokens": 10, "completion_tokens": 2}},
            {"kind": "text", "content": "two", "stop_reason": "EndTurn",
             "usage": {"prompt_tokens": 20, "completion_tokens": 3}}
        ]})
        .to_string(),
    );
    (assets, graph)
}

fn two_model_args(db: &Path, assets: &Path, graph: &Path, extra: &[&str]) -> Vec<String> {
    let mut args = vec![
        "run".to_string(),
        arg(graph),
        "--db".to_string(),
        arg(db),
        "--assets-dir".to_string(),
        arg(assets),
        "--tools-dir".to_string(),
        arg(&ac08_dir().join("tools")),
        "--inputs".to_string(),
        r#"{"start":{"message":"hi"}}"#.to_string(),
        "--python".to_string(),
        python_program(),
        "--python-path".to_string(),
        arg(&repo_root().join("python")),
        format!(
            "--fake-llm={}",
            assets
                .parent()
                .expect("parent")
                .join("fake_llm.json")
                .display()
        ),
    ];
    args.extend(extra.iter().map(|value| value.to_string()));
    args
}

#[test]
fn multi_model_price_missing_fails_before_any_run() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (assets, graph) = two_model_assets(dir.path());
    let pricing = pricing_chat_only(dir.path()); // 仅 deepseek-chat
    let db = dir.path().join("multi.db");
    let args = two_model_args(
        &db,
        &assets,
        &graph,
        &["--max-cost", "1.0", "--pricing", &arg(&pricing), "--json"],
    );
    let output = run_cli(&args, dir.path());
    assert_eq!(code(&output), 3, "stderr={}", stderr(&output));
    let text = stderr(&output);
    assert!(text.contains("pricing_missing_model"), "{text}");
    assert!(text.contains("second-model"), "{text}");
    // fail-closed：不产生 run（DB 可能已建；零请求零 run）。
    assert_eq!(runs_count(&db), 0, "配置错误不得产生 run");

    // 两模型都有价 → 正常 Completed，by_agent 两模型。
    let both = pricing_file(
        dir.path(),
        json!({
            "deepseek-chat": {"input_per_mtok": 1.0, "output_per_mtok": 2.0},
            "second-model": {"input_per_mtok": 3.0, "output_per_mtok": 4.0}
        }),
    );
    let db2 = dir.path().join("multi_ok.db");
    let args = two_model_args(
        &db2,
        &assets,
        &graph,
        &["--max-cost", "1.0", "--pricing", &arg(&both), "--json"],
    );
    let output = run_cli(&args, dir.path());
    assert_eq!(
        code(&output),
        0,
        "stdout={} stderr={}",
        stdout(&output),
        stderr(&output)
    );
    let payload = json_stdout(&output);
    assert_eq!(payload["status"], json!("Completed"));
    let agents = payload["usage"]["by_agent"].as_array().expect("by_agent");
    assert_eq!(agents.len(), 2, "two distinct models priced: {agents:?}");
    assert!(agents.iter().any(|agent| agent["model"] == "second-model"));
}

/// 外部 subprocess harness + `--max-cost` → 成本不可得 fail-closed（不产生 run）。
#[test]
fn external_subprocess_harness_max_cost_is_config_error() {
    let dir = tempfile::tempdir().expect("tempdir");
    let assets = dir.path().join("assets");
    std::fs::create_dir_all(assets.join("agents")).expect("agents");
    write_text(&assets.join("agents"), "prompt.md", "你是外部 harness。");
    let config = json!({
        "id": "external",
        "name": "external",
        "schema_version": 1,
        "model": {
            "provider": "deepseek",
            "name": "ds-model",
            "base_url": null,
            "api_key_env": "HARNESS_TEST_KEY",
            "temperature": 0.0,
            "max_tokens": 256,
            "context_window_tokens": 65536
        },
        "role_prompt_file": "prompt.md",
        "constraints_file": null,
        "tools": [],
        "input_mapping": {"message": "user_message"},
        "max_iterations": 2,
        "harness": {"kind": "subprocess", "command": "/bin/echo"}
    });
    write_text(&assets.join("agents"), "external.json", &config.to_string());
    let graph = write_text(
        dir.path(),
        "external.json",
        &json!({
            "version": "1.0",
            "metadata": {"name": "external"},
            "entry_points": ["start"],
            "nodes": [
                {"id": "start", "type": "start",
                 "inputs": [{"name": "message", "type": "any", "required": true}], "config": {}},
                {"id": "w", "type": "agent_call",
                 "inputs": [{"name": "message", "type": "any", "required": true}],
                 "config": {"agent_id": "external"}},
                {"id": "end", "type": "end",
                 "inputs": [{"name": "result", "type": "any", "required": true}], "config": {}}
            ],
            "edges": [
                {"from": "start", "to_node": "w", "to_input_port": "message"},
                {"from": "w", "to_node": "end", "to_input_port": "result"}
            ]
        })
        .to_string(),
    );
    let pricing = pricing_file(
        dir.path(),
        json!({"ds-model": {"input_per_mtok": 1.0, "output_per_mtok": 1.0}}),
    );
    let db = dir.path().join("external.db");
    let args = two_model_args(
        &db,
        &assets,
        &graph,
        &["--max-cost", "1.0", "--pricing", &arg(&pricing)],
    );
    let output = run_cli(&args, dir.path());
    assert_eq!(code(&output), 3, "stderr={}", stderr(&output));
    assert!(
        stderr(&output).contains("cost_unavailable_external_harness"),
        "{}",
        stderr(&output)
    );
    assert_eq!(runs_count(&db), 0);
}

// ---------------------------------------------------------------------------
// 默认输出不变
// ---------------------------------------------------------------------------

#[test]
fn default_outputs_are_unchanged_without_observability_flags() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db = dir.path().join("trace.db");
    let output = run_cli(&ac08_args(&db, &["--json"]), dir.path());
    assert_eq!(code(&output), 0, "stderr={}", stderr(&output));
    let payload = json_stdout(&output);
    assert!(
        payload.get("usage").is_none(),
        "无 flags 不得追加 usage: {payload}"
    );
    // 默认 record=hash → 无 llm_calls 记录 → trace --stats 不追加 llm/token 键。
    let run_id = payload["run_id"].as_str().expect("run_id").to_string();
    let stats = run_cli(
        &[
            "trace".to_string(),
            run_id,
            "--db".to_string(),
            arg(&db),
            "--stats".to_string(),
            "--json".to_string(),
        ],
        dir.path(),
    );
    assert_eq!(code(&stats), 0, "stderr={}", stderr(&stats));
    let stats_json = json_stdout(&stats);
    assert!(stats_json.get("llm_calls").is_none());
    assert!(stats_json.get("token_usage").is_none());
}

/// 保留给编译期使用的临时目录类型（helper 返回）。
#[allow(dead_code)]
fn _assert_tempdir(_: TempDir) {}
