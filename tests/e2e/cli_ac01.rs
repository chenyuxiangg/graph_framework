//! AC-01（M5 CLI 真链路）：`zgraph run` 跑通客服工单示例图 + 失败路径不产生 run。
//!
//! 语义基准：`doc/spec-v1.2/07-验收用例.md` AC-01、06 §6.10.3/§6.10.5/§6.10.6
//! （参数/退出码/`--json` 结构）、05 §5.9.2（agent 输出四字段）。
//!
//! 本文件用**真实 `zgraph` 二进制**（见 `cli_support` 的发现顺序）执行：
//!
//! ```text
//! zgraph run tests/fixtures/graphs/customer_ticket.json
//!   --inputs '{"input_parser":{"user_message":"我要退款"}}'
//!   --assets-dir tests/fixtures/graphs/customer_ticket
//!   --fake-llm=tests/fixtures/graphs/customer_ticket/fake_llm.json
//!   --db <tmp> --json
//! ```
//!
//! 链路：CLI → 真 kernel → `HarnessExecutor`（fake LLM）+ 真 Python worker 子进程
//! （`human_escalation_check`）→ 真 SQLite trace → CLI `--json` / `zgraph trace`。
//!
//! AC-01 期望的实例终态：`input_parser`/`kb_search`/`answer_generator`/
//! `human_escalation_check`/`end_success` → Completed，`human_handoff` → Skipped
//! （内置 gate `check_output` 拦截）；`run_output = {"result": <answer_writer 四字段>}`
//! 且 `content` 为字符串、需二次解析出 `answer`/`need_human`。
//!
//! `customer_ticket` 的 fake LLM 三个调用内容相同（kb_search 与 answer_generator 在
//! input_parser 之后**并发**就绪，按节点区分内容会依赖 LLM 请求顺序、不可稳定断言）；
//! 内容同时含各下游需要的字段，属确定性脚本而非弱化断言。
//!
//! 稳定性约定：每个用例独立 `tempfile::TempDir` 与独立 DB；子进程退出即判定，
//! 时序只用宽松上限（`tokio::time::timeout` 不在本文件使用，CLI 自带完成语义）。

#![cfg(unix)]

#[path = "cli_support/mod.rs"]
mod cli_support;
#[path = "common/mod.rs"]
mod common;

use std::path::Path;

use serde_json::{json, Value};

use zgraph_trace::{InstanceStatus as TraceInstanceStatus, TraceReader};

use cli_support::{
    ac01_dir, ac01_run_args, arg, assert_exit, json_stdout, repo_root, run_cli, runs_count,
    stderr_text,
};

/// AC-01 入口输入（07 AC-01 步骤）。
const AC01_INPUTS: &str = r#"{"input_parser":{"user_message":"我要退款"}}"#;

/// AC-01 图入口节点（= 唯一 entry point）。
const AC01_ENTRY: &str = "input_parser";

// ---------------------------------------------------------------------------
// CLI 参数与 fixture 读取
// ---------------------------------------------------------------------------

/// `customer_ticket/fake_llm.json` 第一轮脚本内容（二次解析断言用）。
fn fake_content() -> String {
    let path = ac01_dir().join("fake_llm.json");
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("读取 {} 失败: {error}", path.display()));
    let script: Value = serde_json::from_str(&text).expect("fake_llm.json 应为合法 JSON");
    script["calls"][0]["content"]
        .as_str()
        .expect("fake script calls[0].content")
        .to_string()
}

/// 断言 agent 四字段整包（05 §5.9.2）。
fn assert_four_field_package(value: &Value) {
    let object = value.as_object().expect("agent output 应为对象");
    let mut keys: Vec<&str> = object.keys().map(String::as_str).collect();
    keys.sort_unstable();
    assert_eq!(
        keys,
        vec!["content", "iterations", "stop_reason", "usage"],
        "agent 输出必须恰好是四字段整包"
    );
    assert!(value["content"].is_string(), "content 必须是字符串");
    assert!(value["stop_reason"].is_string());
    assert!(value["iterations"].as_u64().is_some_and(|n| n >= 1));
    let total = value["usage"]["total_tokens"]
        .as_u64()
        .expect("usage.total_tokens");
    assert!(total > 0, "usage.total_tokens 必须 > 0");
}

// ---------------------------------------------------------------------------
// trace 断言
// ---------------------------------------------------------------------------

/// 断言 run 行 / 实例终态序列 / gate 拦截 / 真 python 节点输出 / trace 可读。
fn assert_ac01_trace(db: &Path, run_id: &str) {
    let reader = TraceReader::open_readonly(db).expect("打开 trace 只读连接");
    let row = reader
        .run(run_id)
        .expect("查询 run 行")
        .expect("run 行存在");
    assert_eq!(row.status, zgraph_trace::RunStatus::Completed);
    assert_eq!(row.user_id, "local", "CLI 默认 user_id");
    common::assert_lifecycle_shape(&reader, run_id);

    for node in [
        "input_parser",
        "kb_search",
        "answer_generator",
        "human_escalation_check",
        "end_success",
    ] {
        common::assert_instance_status(&reader, run_id, node, TraceInstanceStatus::Completed);
    }
    common::assert_instance_status(
        &reader,
        run_id,
        "human_handoff",
        TraceInstanceStatus::Skipped,
    );

    // 真 Python worker 证据：human_escalation_check 的 output 是真实执行结果。
    let python_instance = common::instance(&reader, run_id, "human_escalation_check");
    assert_eq!(
        python_instance.output_json,
        Some(json!({"need_human": false})),
        "python 节点输出应由真 worker 子进程产生"
    );

    // gate：answer_generator → human_handoff 的投递被 check_output 拦截。
    let gates = reader.gate_results(run_id).expect("查询 gate_results");
    let blocked: Vec<_> = gates
        .iter()
        .filter(|gate| gate.to_node == "human_handoff" && !gate.passed)
        .collect();
    assert_eq!(blocked.len(), 1, "human_handoff 应恰有一次被拦截的投递");
    assert_eq!(
        blocked[0].gate_fn.as_deref(),
        Some("check_output"),
        "拦截 gate 应为内置 check_output"
    );
}

/// `zgraph trace <run_id> --db <db> --json` 可回放（退出码 0 且含 RunFinished）。
fn assert_trace_command(db: &Path, run_id: &str) {
    let output = run_cli(
        &[
            "trace".to_string(),
            run_id.to_string(),
            "--db".to_string(),
            arg(db),
            "--json".to_string(),
        ],
        &repo_root(),
    );
    assert_exit(&output, 0, "zgraph trace 回放");
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(
        text.contains("RunFinished"),
        "trace 输出应含 RunFinished: {text}"
    );
}

// ---------------------------------------------------------------------------
// AC-01 主用例
// ---------------------------------------------------------------------------

/// AC-01：真实 CLI 二进制跑通客服工单图，`--json` 与 trace 双向可验收。
#[test]
fn ac01_cli_run_customer_ticket_completes_with_expected_lifecycle() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db = dir.path().join("ac01.db");
    let output = run_cli(&ac01_run_args(&db, AC01_INPUTS, "hash", &[]), dir.path());
    assert_exit(&output, 0, "AC-01 CLI run");

    let payload = json_stdout(&output);
    assert_eq!(payload["ok"], json!(true));
    assert_eq!(payload["command"], json!("run"));
    assert_eq!(payload["status"], json!("Completed"));
    assert_eq!(
        payload["run_output"]["result"]["content"],
        json!(fake_content())
    );
    assert!(
        payload["duration_ms"].as_u64().is_some(),
        "duration_ms 必填"
    );
    assert!(
        payload["event_count"].as_i64().unwrap_or(0) > 0,
        "Completed run 的事件数应 > 0"
    );

    // run_output = {"result": <answer_writer 四字段>}；content 为字符串需二次解析。
    let result = payload["run_output"]
        .get("result")
        .expect("run_output.result");
    assert_four_field_package(result);
    let content: Value = serde_json::from_str(result["content"].as_str().expect("content 字符串"))
        .expect("content 应可二次解析为 JSON");
    assert_eq!(content["answer"], json!("可以退款"));
    assert_eq!(content["need_human"], json!(false));

    let run_id = payload["run_id"].as_str().expect("run_id");
    assert_ac01_trace(&db, run_id);
    assert_trace_command(&db, run_id);
}

// ---------------------------------------------------------------------------
// 失败路径：坏图 exit 2 / 坏输入 exit 3，且都不产生 run
// ---------------------------------------------------------------------------

/// 坏图（DSL 校验失败）→ exit 2，`runs` 表 0 行。
#[test]
fn ac01_bad_graph_exits_2_without_creating_run() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db = dir.path().join("ac01_bad_graph.db");
    let bad_graph = repo_root().join("tests/fixtures/graphs/bad_version.json");
    let output = run_cli(
        &[
            "run".to_string(),
            arg(&bad_graph),
            "--inputs".to_string(),
            "{}".to_string(),
            "--db".to_string(),
            arg(&db),
            "--json".to_string(),
        ],
        dir.path(),
    );
    assert_exit(&output, 2, "坏图应 exit 2");
    assert_eq!(runs_count(&db), 0, "坏图不得产生 run");
    assert!(
        stderr_text(&output).to_lowercase().contains("graph"),
        "stderr 应说明图加载失败: {}",
        stderr_text(&output)
    );
}

/// 入口输入非法（缺 required 端口）→ exit 3，`runs` 表 0 行。
#[test]
fn ac01_bad_entry_input_exits_3_without_creating_run() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db = dir.path().join("ac01_bad_input.db");
    let bad_inputs = format!(r#"{{"{AC01_ENTRY}":{{}}}}"#);
    let output = run_cli(&ac01_run_args(&db, &bad_inputs, "hash", &[]), dir.path());
    assert_exit(&output, 3, "坏入口输入应 exit 3");
    assert_eq!(runs_count(&db), 0, "坏输入不得产生 run");
    assert!(
        stderr_text(&output).contains("RUN_ENTRY"),
        "stderr 应带 RUN_ENTRY_* 错误码: {}",
        stderr_text(&output)
    );
}
