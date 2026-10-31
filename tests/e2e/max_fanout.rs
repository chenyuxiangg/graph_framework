//! W2b（DR-004 §3 / D2）：每节点最大扇出限制的 e2e。
//!
//! 语义基准：`doc/spec-v1.2/02-DSL与校验.md` R16（静态层）、`doc/spec-v1.2/03-内核与调度.md`
//! （运行期每激活上限，见本文件末尾 C3 段）与 `doc/spec-v1.2/07-验收用例.md` AC-15。
//!
//! 本文件覆盖三部分（C 节合同）：
//!
//! 1. **C1 静态层 CLI 反例/边界**：真 `zgraph validate --json` 对三个 fixture 断言
//!    `ok`/`code`/`location.node_id`/`error_count`/`expected`/`actual`/退出码；
//!    `valid_max_fanout.json` 额外锁定 canonical projection 的 `graph_hash`
//!    （`max_fanout` 参与 hash；缺失时不参与 → v0.1.0 hash 不变）；
//! 2. **C2 正例真链路**：真 `zgraph` 二进制（真 kernel + 真 Python worker 子进程 +
//!    真 SQLite trace）跑通 `valid_max_fanout.json`（`start` out-degree=2、
//!    `max_fanout=2`）→ Completed，并断言 run 行/实例/gate_results 的存储侧事实；
//! 3. **C3 运行期防御层**：in-process `common::run_to_trace` 真链路 + `test-hooks`
//!    `force_max_fanout` 构造运行期超限（静态层只保证加载期合法图不会超限，
//!    运行期校验是防御性的；可达性见 DR-004 §3 W2b / task-1 B5）。
//!
//! 禁止 mock：C1/C2 用真二进制与真 worker；C3 用真 kernel + 真 worker + 真 SQLite。
//! 每个用例独立 `tempfile::TempDir`/trace DB；时序只用宽松上限，不做 sleep 猜测。

#![cfg(unix)]

#[path = "cli_support/mod.rs"]
mod cli_support;
#[path = "common/mod.rs"]
mod common;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Output;

use serde_json::{json, Value};

use zgraph_cli::bridge::BridgeOutcome;
use zgraph_kernel::{RunStatus as KernelRunStatus, TestHooks};
use zgraph_trace::{InstanceStatus as TraceInstanceStatus, RunStatus, TraceReader};

use cli_support::{arg, assert_exit, json_stdout, python_program, repo_root, run_cli, stderr_text};
use common::graphs_root;

// ---------------------------------------------------------------------------
// 常量与 fixture
// ---------------------------------------------------------------------------

/// 合法图：`start.max_fanout=2`、`start` out-degree=2（边界 actual == limit）。
const VALID_GRAPH: &str = "valid_max_fanout.json";

/// 静态反例：`start.max_fanout=1`、out-degree=2（R16 超限）。
const BAD_EXCEEDED_GRAPH: &str = "bad_max_fanout_exceeded.json";

/// 静态反例：`start.max_fanout=0`（R4 取值非法）。
const BAD_VALUE_GRAPH: &str = "bad_max_fanout_value.json";

/// `valid_max_fanout.json` 的冻结 graph_hash（dsl `tests/hash.rs` golden）。
///
/// `max_fanout` 参与 canonical projection，因此该值同时是「字段确实进入 hash」的证据。
const VALID_GRAPH_HASH: &str = "e05d882cc55aa275";

/// fixture 中声明 `max_fanout` 的扇出节点（= entry point）。
const FANOUT_NODE: &str = "start";

/// `valid_max_fanout.json` 入口输入：`start` 未声明输入端口。
const VALID_ENTRY_INPUTS: &str = r#"{"start":{}}"#;

/// `tests/fixtures/graphs/<name>`。
fn graph_path(name: &str) -> PathBuf {
    graphs_root().join(name)
}

/// 真 `zgraph validate <fixture> --json`。
fn validate_json(name: &str) -> Output {
    run_cli(
        &[
            "validate".to_string(),
            arg(&graph_path(name)),
            "--json".to_string(),
        ],
        &repo_root(),
    )
}

/// `--json` 报错体里的 `errors` 数组（并按合同断言 `error_count == 1`）。
fn single_error(payload: &Value) -> &Value {
    assert_eq!(payload["ok"], json!(false), "校验报告 ok=false: {payload}");
    assert_eq!(payload["error_count"], json!(1), "error_count 应为 1");
    assert_eq!(
        payload["truncated"],
        json!(false),
        "单条错误不应截断: {payload}"
    );
    let errors = payload["errors"].as_array().expect("errors 应为数组");
    assert_eq!(errors.len(), 1, "errors 应恰有一条: {payload}");
    &errors[0]
}

/// 断言错误定位只有 `node_id`（节点级；无 edge_index/port）。
fn assert_node_location(error: &Value, node_id: &str) {
    assert_eq!(
        error["location"]["node_id"],
        json!(node_id),
        "location.node_id 必须指向扇出节点: {error}"
    );
    assert!(
        error["location"].get("edge_index").is_none(),
        "R16 是节点级错误，不应带 edge_index: {error}"
    );
    assert!(
        error["location"].get("port").is_none(),
        "R16 是节点级错误，不应带 port: {error}"
    );
}

// ---------------------------------------------------------------------------
// C1 静态层：R16 超限反例
// ---------------------------------------------------------------------------

/// R16 静态超限：`node 'start' out-degree 2 exceeds max_fanout 1` → exit 2。
///
/// 02 §2.1.3 / 00 §0.4：R16 复用 `DSL_PARSE_ERROR`（不新增 DSL 错误码），
/// 取首错（同 R13/R15），定位 `node_id`。
#[test]
fn bad_max_fanout_exceeded_reports_r16_fanout_error() {
    let output = validate_json(BAD_EXCEEDED_GRAPH);
    // 06 §6.10.5：DSL 错误 → 退出码 2；stdout 仍是完整 JSON 契约。
    assert_exit(&output, 2, "zgraph validate 超限图");
    let payload = json_stdout(&output);
    let error = single_error(&payload);

    assert_eq!(error["code"], json!("DSL_PARSE_ERROR"));
    assert_eq!(
        error["message"],
        json!("node 'start' out-degree 2 exceeds max_fanout 1")
    );
    assert_eq!(error["expected"], json!("out-degree <= 1"));
    assert_eq!(error["actual"], json!("out-degree 2"));
    assert_node_location(error, FANOUT_NODE);
    assert!(
        payload.get("graph_hash").is_none(),
        "校验失败不得输出 graph_hash: {payload}"
    );
}

// ---------------------------------------------------------------------------
// C1 静态层：R4 取值非法反例
// ---------------------------------------------------------------------------

/// R4 取值非法：`max_fanout=0` → `'max_fanout' must be >= 1, got 0`，exit 2。
#[test]
fn bad_max_fanout_value_reports_must_be_positive() {
    let output = validate_json(BAD_VALUE_GRAPH);
    assert_exit(&output, 2, "zgraph validate 非法取值图");
    let payload = json_stdout(&output);
    let error = single_error(&payload);

    assert_eq!(error["code"], json!("DSL_PARSE_ERROR"));
    let message = error["message"].as_str().expect("message 字符串");
    assert!(
        message.contains("'max_fanout' must be >= 1"),
        "message 应说明 max_fanout 下界: {message}"
    );
    assert_eq!(error["expected"], json!(">= 1"));
    assert_eq!(error["actual"], json!(0));
    assert_node_location(error, FANOUT_NODE);
}

// ---------------------------------------------------------------------------
// C1 静态层：合法图通过 + graph_hash 冻结
// ---------------------------------------------------------------------------

/// 合法图（out-degree == max_fanout == 2）通过校验，并锁定参与 hash 的冻结指纹。
#[test]
fn valid_max_fanout_passes_validate_with_frozen_hash() {
    let output = validate_json(VALID_GRAPH);
    assert_exit(&output, 0, "zgraph validate 合法图");
    let payload = json_stdout(&output);

    assert_eq!(payload["ok"], json!(true), "合法图 ok=true: {payload}");
    assert_eq!(payload["error_count"], json!(0));
    assert_eq!(
        payload["errors"].as_array().map(Vec::len),
        Some(0),
        "合法图无错误"
    );
    let hash = payload["graph_hash"]
        .as_str()
        .expect("合法图必须输出 graph_hash");
    assert_eq!(hash.len(), 16, "graph_hash 应为 16 字符: {hash}");
    assert!(
        hash.chars().all(|character| character.is_ascii_hexdigit()),
        "graph_hash 应为 16 hex: {hash}"
    );
    assert_eq!(
        hash, VALID_GRAPH_HASH,
        "max_fanout 参与 canonical hash 且冻结"
    );
}

// ---------------------------------------------------------------------------
// C2 正例真链路：真 CLI + 真 worker + 真 SQLite trace
// ---------------------------------------------------------------------------

/// `valid_max_fanout.json` 的 `zgraph run` 参数（真 Python worker `workers.parallel`）。
fn run_args(db: &Path) -> Vec<String> {
    vec![
        "run".to_string(),
        arg(&graph_path(VALID_GRAPH)),
        "--inputs".to_string(),
        VALID_ENTRY_INPUTS.to_string(),
        "--db".to_string(),
        arg(db),
        "--python".to_string(),
        python_program(),
        "--python-path".to_string(),
        arg(&repo_root().join("python")),
        "--json".to_string(),
    ]
}

/// Completed run 的存储侧断言（run 行/生命周期/实例/gate_results，真 trace）。
fn assert_completed_trace(reader: &TraceReader, run_id: &str, run_output: &Value) {
    let run = common::run_row(reader, run_id);
    assert_eq!(run.status, RunStatus::Completed, "runs.status");
    assert_eq!(
        run.error_code, None,
        "Completed run 的 runs.error_code 必须为 NULL"
    );
    assert_eq!(
        run.output_json.as_ref(),
        Some(run_output),
        "runs.output_json 应等于 CLI run_output"
    );
    common::assert_lifecycle_shape(reader, run_id);

    // 生命周期：入口 + 两条并行分支 + end 全部 Completed（真 worker 子进程执行）。
    for node in ["start", "a", "b", "done"] {
        common::assert_instance_status(reader, run_id, node, TraceInstanceStatus::Completed);
    }
    let workers = ["a", "b"];
    for node in workers {
        let instance = common::instance(reader, run_id, node);
        let marker = json!({"worker": node, "x": {}});
        assert_eq!(
            instance.output_json,
            Some(marker.clone()),
            "节点 {node} 输出应由真 worker 子进程产生"
        );
        assert_eq!(instance.attempt, 0, "正常运行节点 attempt 应为 0");
        assert_eq!(
            instance.error_code, None,
            "Completed 实例 error_code 为 NULL"
        );
    }

    // R16 边界（actual == limit == 2）：扇出节点两条出边各产生一条 passed GateResult。
    let gates = reader.gate_results(run_id).expect("查询 gate_results");
    let fanout: Vec<_> = gates
        .iter()
        .filter(|gate| gate.node_id == FANOUT_NODE)
        .collect();
    assert_eq!(fanout.len(), 2, "扇出节点应有 2 条出边的 GateResult");
    let mut targets: Vec<(&str, &str, bool)> = fanout
        .iter()
        .map(|gate| (gate.to_node.as_str(), gate.to_port.as_str(), gate.passed))
        .collect();
    targets.sort_unstable();
    assert_eq!(
        targets,
        vec![("a", "x", true), ("b", "x", true)],
        "两条出边都必须放行（actual == max_fanout 是合法边界）"
    );

    // 静态层已保证合法图不会触发运行期防御层：无 Crashed 实例。
    let instances = reader.instances(run_id).expect("查询 instances");
    assert!(
        instances
            .iter()
            .all(|row| row.status != TraceInstanceStatus::Crashed),
        "合法图不应产生 Crashed 实例: {instances:?}"
    );
}

/// C2：真 `zgraph run` 跑通 `valid_max_fanout.json` → Completed + 存储侧事实。
#[test]
fn valid_max_fanout_runs_to_completion_through_real_cli() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db = dir.path().join("max_fanout.db");
    let output = run_cli(&run_args(&db), dir.path());
    assert_exit(&output, 0, "valid_max_fanout CLI run");
    let payload = json_stdout(&output);

    assert_eq!(payload["ok"], json!(true));
    assert_eq!(payload["command"], json!("run"));
    assert_eq!(payload["status"], json!("Completed"));
    assert!(
        payload["duration_ms"].as_u64().is_some(),
        "--json 契约必填 duration_ms: {payload}"
    );
    assert!(
        payload["event_count"]
            .as_i64()
            .is_some_and(|count| count > 0),
        "Completed run 的事件数应 > 0"
    );
    let run_id = payload["run_id"].as_str().expect("run_id").to_string();

    // end 聚合输出：`done` 的 `ra`/`rb` 分别来自两条并行分支的真 worker 输出。
    let run_output = json!({
        "ra": {"worker": "a", "x": {}},
        "rb": {"worker": "b", "x": {}}
    });
    assert_eq!(
        payload["run_output"], run_output,
        "run_output 应是 end 节点聚合的两端口真输出"
    );
    let reader = TraceReader::open_readonly(&db).expect("打开 trace 只读连接");
    assert_completed_trace(&reader, &run_id, &run_output);

    // stderr 不得混入 DSL 错误（真链路的退出码已由 assert_exit 锁定）。
    assert!(
        !stderr_text(&output).contains("DSL_PARSE_ERROR"),
        "合法图运行不应出现 DSL 错误: {}",
        stderr_text(&output)
    );
}

// ---------------------------------------------------------------------------
// C3 运行期防御层：`test-hooks` 强制收紧每激活扇出上限
// ---------------------------------------------------------------------------
//
// 静态层 R16 已拒绝 `out_degree > max_fanout` 的图，因此运行期反例无法用合法 DSL
// 图构造；按 task-1 B5 的可达性口径，用 `test-hooks` 的 `force_max_fanout` 把
// `valid_max_fanout.json`（`start.max_fanout=2`、out-degree=2）的有效上限收紧到 1，
// 走与生产完全相同的 kernel + 真 worker + 真 SQLite trace 链路。

/// 超限钩子值：有效上限 = min(节点 max_fanout 2, 1) = 1 < actual 2。
const FORCE_OVER_LIMIT: u32 = 1;

/// 边界钩子值：有效上限 = min(2, 2) = 2 == actual 2（不得失败）。
const FORCE_AT_LIMIT: u32 = 2;

/// 本次激活实际放行的出边数（fixture：`start` 两条出边都无 gate → 均 passed）。
const FANOUT_ACTUAL: u32 = 2;

/// 源实例 attempt（0）+ 1：运行期超限的 Crashed 实例 attempt。
const CRASHED_ATTEMPT: u32 = 1;

/// B2 冻结的 run 级失败消息（确定性模板）。
const RUN_ERROR_MESSAGE: &str =
    "node 'start' fan-out 2 exceeded max_fanout (1) in a single activation";

/// B3 冻结的 `NodeError::FanoutExceeded` Display（实例 error JSON 的 message）。
const NODE_ERROR_MESSAGE: &str = "node fan-out 2 exceeded max_fanout (1)";

/// 以 `force_max_fanout` 跑一次 in-process 真链路（真 kernel + 真 worker + 真 trace）。
///
/// `valid_max_fanout.json` 的 `start` 未声明输入端口 → 入口注入为空 map。
async fn run_with_forced_fanout(db: &Path, force: u32) -> BridgeOutcome {
    let graph = common::compile_fixture(VALID_GRAPH);
    let spec = common::spec_with(graph, HashMap::new(), |options| {
        options.with_test_hooks(TestHooks {
            force_max_fanout: Some(force),
            ..Default::default()
        });
    });
    common::run_to_trace(spec, common::executor(common::RUN_TIMEOUT), db).await
}

/// C3-a：`force_max_fanout=1` → 运行期超限：run Failed、Crashed 实例、该激活无 GateResult。
#[tokio::test]
async fn runtime_fanout_exceeded_via_test_hooks_fails_run_and_crashes_source() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db = dir.path().join("max_fanout_over.db");
    let bridge = run_with_forced_fanout(&db, FORCE_OVER_LIMIT).await;

    // 内核 outcome：冷峻失败，无 end 聚合输出。
    assert_eq!(
        bridge.outcome.status,
        KernelRunStatus::Failed,
        "运行期超限必须让 run Failed"
    );
    assert_eq!(bridge.outcome.output, None, "Failed run 不得有 end 输出");
    let run_error = bridge
        .outcome
        .error
        .as_ref()
        .expect("Failed run 必须带 RunError");
    assert_eq!(run_error.code, "NODE_FANOUT_EXCEEDED");
    assert_eq!(run_error.message, RUN_ERROR_MESSAGE);

    let run_id = bridge.outcome.run_id.clone();
    let reader = bridge.reader();

    // trace run 行：status=Failed、runs.error_code=NODE_FANOUT_EXCEEDED。
    let run = common::run_row(reader, &run_id);
    assert_eq!(run.status, RunStatus::Failed, "runs.status");
    assert_eq!(
        run.error_code.as_deref(),
        Some("NODE_FANOUT_EXCEEDED"),
        "runs.error_code"
    );
    assert_eq!(run.error_message.as_deref(), Some(RUN_ERROR_MESSAGE));
    common::assert_lifecycle_shape(reader, &run_id);

    // RunFinished.error 与 outcome 一致（04/06 失败矩阵）。
    let finished = common::run_finished_payload(reader, &run_id);
    assert_eq!(finished["status"], json!("Failed"));
    assert_eq!(finished["error"]["code"], json!("NODE_FANOUT_EXCEEDED"));
    assert_eq!(finished["error"]["message"], json!(RUN_ERROR_MESSAGE));

    // 实例：源实例（attempt=0）已 Completed；超限只新增一个 Crashed 实例，
    // attempt = 源实例 attempt + 1 = 1，error 为 NodeError::FanoutExceeded 的冻结 JSON。
    let instances = reader.instances(&run_id).expect("查询 instances");
    assert_eq!(
        instances.len(),
        2,
        "只应有源实例 + 运行期超限 Crashed 实例: {instances:?}"
    );
    let source = instances
        .iter()
        .find(|row| row.node_id == FANOUT_NODE && row.attempt == 0)
        .expect("源实例存在");
    assert_eq!(
        source.status,
        TraceInstanceStatus::Completed,
        "超限不改变已发布源实例的 Completed 终态"
    );
    let crashed: Vec<_> = instances
        .iter()
        .filter(|row| row.status == TraceInstanceStatus::Crashed)
        .collect();
    assert_eq!(crashed.len(), 1, "超限应恰产生一个 Crashed 实例");
    let crashed = crashed[0];
    assert_eq!(crashed.node_id, FANOUT_NODE, "Crashed 实例属于扇出节点");
    assert_eq!(crashed.attempt, CRASHED_ATTEMPT, "Crashed attempt = 源 + 1");
    assert_eq!(
        crashed.error_code.as_deref(),
        Some("NODE_FANOUT_EXCEEDED"),
        "Crashed 实例 error_code"
    );
    let error_json = crashed
        .error
        .as_ref()
        .expect("06 §6.2.5：Crashed 实例必须带 error JSON");
    assert_eq!(error_json["code"], json!("NODE_FANOUT_EXCEEDED"));
    assert_eq!(error_json["kind"], json!("FanoutExceeded"));
    assert_eq!(error_json["limit"], json!(FORCE_OVER_LIMIT));
    assert_eq!(error_json["actual"], json!(FANOUT_ACTUAL));
    assert_eq!(error_json["message"], json!(NODE_ERROR_MESSAGE));

    // 该激活无 GateResult：扇出节点两条出边（edge 0/1）都不得有 GateResult 行；
    // 本 run 只有这一次激活，因此整张 gate_results 必须为空。
    let gates = reader.gate_results(&run_id).expect("查询 gate_results");
    let fanout_edges: Vec<_> = gates
        .iter()
        .filter(|gate| gate.node_id == FANOUT_NODE && gate.edge_index < 2)
        .collect();
    assert!(
        fanout_edges.is_empty(),
        "超限激活的 2 条出边不得产生 GateResult: {fanout_edges:?}"
    );
    assert!(
        gates.is_empty(),
        "超限激活不得产生任何 GateResult: {gates:?}"
    );

    // 下游实例未创建（边一条都未应用）。
    assert!(
        instances
            .iter()
            .all(|row| !["a", "b", "done"].contains(&row.node_id.as_str())),
        "超限后不得创建任何下游实例: {instances:?}"
    );
}

/// C3-b：`force_max_fanout=2`（actual == 有效上限 == 2）→ Completed 边界合法。
#[tokio::test]
async fn runtime_fanout_at_limit_completes_as_boundary_case() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db = dir.path().join("max_fanout_at_limit.db");
    let bridge = run_with_forced_fanout(&db, FORCE_AT_LIMIT).await;

    assert_eq!(
        bridge.outcome.status,
        KernelRunStatus::Completed,
        "actual == limit 是合法边界，不得超限"
    );
    assert_eq!(bridge.outcome.error, None, "Completed run 无 RunError");

    let run_output = json!({
        "ra": {"worker": "a", "x": {}},
        "rb": {"worker": "b", "x": {}}
    });
    assert_eq!(
        bridge.outcome.output.as_ref(),
        Some(&run_output),
        "边界放行后两条分支都应执行完成"
    );
    let run_id = bridge.outcome.run_id.clone();
    assert_completed_trace(bridge.reader(), &run_id, &run_output);
}
