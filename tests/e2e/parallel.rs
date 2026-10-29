//! AC-09（M3 真 worker 口径，D8）：并行分支重叠、总耗时阈值、确定性、双端口合并与
//! 「同一端口两次投递」的 trace 可见性。
//!
//! 语义基准：`doc/spec-v1.2/07-验收用例.md` AC-09 + `doc/architecture-v2.md` §8 D8
//! （含进程冷启动后重新标定；1.0s 只是初值占位，M3 实测后回填）：
//!
//! - `a`/`b` 的 **Running 区间重叠**，进入 Running 时间差 **< 50ms**（07 冻结口径；
//!   用 trace `transitions.at` 的真实墙钟计算）；
//! - 总耗时按 D8 **实测重标定**（[`PARALLEL_TOTAL_BUDGET`]），并要求
//!   **并行耗时 < 串行对照耗时 × 0.8**（同用例内跑同两 worker 的串行图，证伪
//!   「串行也能通过」）；
//! - `merge`（end 节点）收到 `ra`/`rb` 两端口各一份数据（trace `gate_results` 两次投递）；
//! - 同输入连续 5 次 run：每个节点的迁移链与终态一致（允许并行分支交错）；
//! - 覆盖语义：v1 单数据源（DSL R5 禁止同一端口多来源）；同一自环边两次投递在
//!   trace 中可见（两条 `gate_results` + 两个 attempt 实例）。
//!
//! 图资产：`tests/fixtures/graphs/valid_parallel.json`（`workers.parallel.a/b`）。

#![cfg(unix)]

#[path = "common/mod.rs"]
mod common;

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use chrono::{DateTime, FixedOffset};
use serde_json::{json, Value};

use zgraph_cli::bridge::BridgeOutcome;
use zgraph_kernel::{CompiledGraph, RunStatus};
use zgraph_trace::InstanceStatus as TraceInstanceStatus;

/// 两个并行分支进入 Running 的时间差上限：**07 AC-09 冻结口径 <50ms**。
///
/// 实测（本机 macOS / Python 3.13，连测 3 次）：a/b 同一轮派发，时间差 **0ms**；
/// 50ms 上限保留进程冷启动/CI 抖动余量。
const BRANCH_START_SKEW_LIMIT: Duration = Duration::from_millis(50);

/// 并行总耗时上限（D8：M3 实测后重标定，替换 1.0s 占位）。
///
/// **实测**（本机 macOS / Python 3.13，2026-10-30 连测 5 次；REV-015 复审抽样一致）：
/// 并行 **248–295ms**（复审 252–292ms）；同两 worker 串行对照 **504–558ms**
/// （复审 488–528ms），并行/串行 ≈ 0.44–0.53。取 600ms ≈ 实测上限 2x 余量；
/// 「显著小于串行」由同用例的串行对照断言（<80%）保证，绝对阈值只作上界。
const PARALLEL_TOTAL_BUDGET: Duration = Duration::from_millis(600);

/// 串行对照系数：并行总耗时必须 < 串行 × 0.8（D8「显著小于串行」的可证伪形式）。
const SERIAL_RATIO_PERCENT: u128 = 80;

/// 确定性回归的 run 次数（07 AC-09：连续 5 次）。
const DETERMINISM_RUNS: usize = 5;

// ---------------------------------------------------------------------------
// trace 时序与摘要
// ---------------------------------------------------------------------------

/// 解析 trace 墙钟（RFC3339 毫秒 Z）。
fn parse_at(at: &str) -> DateTime<FixedOffset> {
    DateTime::parse_from_rfc3339(at)
        .unwrap_or_else(|error| panic!("bad trace timestamp {at}: {error}"))
}

/// 取某节点「进入 Running」与「离开 Running」的时刻（trace transitions）。
fn running_interval(
    reader: &zgraph_trace::TraceReader,
    run_id: &str,
    node_id: &str,
) -> (DateTime<FixedOffset>, DateTime<FixedOffset>) {
    let rows = reader.transitions(run_id).expect("transitions");
    let mut started = None;
    let mut ended = None;
    for row in rows.iter().filter(|row| row.node_id == node_id) {
        if row.to_status == TraceInstanceStatus::Running {
            started = Some(parse_at(&row.at));
        }
        if row.from_status == TraceInstanceStatus::Running && row.to_status.is_terminal() {
            ended = Some(parse_at(&row.at));
        }
    }
    (
        started.unwrap_or_else(|| panic!("node {node_id} never entered Running")),
        ended.unwrap_or_else(|| panic!("node {node_id} never left Running")),
    )
}

/// 并行图一次 run 的 trace 摘要（确定性比较用；不含实例 id/seq 等非确定性字段）。
#[derive(Debug, PartialEq)]
struct RunDigest {
    /// 每个节点的迁移链（按 seq 排序的 `from->to` 字符串序列）。
    chains: BTreeMap<String, Vec<String>>,
    /// 每个节点的实例终态。
    terminal: BTreeMap<String, String>,
    /// run 输出。
    output: Option<Value>,
}

/// 摘要某 run 的 trace 形状。
fn digest(reader: &zgraph_trace::TraceReader, run_id: &str) -> RunDigest {
    let mut chains: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut transitions = reader.transitions(run_id).expect("transitions");
    transitions.sort_by_key(|row| row.seq);
    for row in transitions {
        chains.entry(row.node_id).or_default().push(format!(
            "{}->{}",
            row.from_status.as_str(),
            row.to_status.as_str()
        ));
    }
    let mut terminal: BTreeMap<String, String> = BTreeMap::new();
    for row in reader.instances(run_id).expect("instances") {
        terminal.insert(row.node_id, row.status.as_str().to_string());
    }
    RunDigest {
        chains,
        terminal,
        output: reader
            .run(run_id)
            .expect("run row")
            .and_then(|run| run.output_json),
    }
}

// ---------------------------------------------------------------------------
// run 辅助
// ---------------------------------------------------------------------------

/// 跑一次给定图并计时（真 worker；空入口输入）。
async fn run_graph_timed(
    dir: &tempfile::TempDir,
    name: &str,
    graph: Arc<CompiledGraph>,
) -> (BridgeOutcome, Duration) {
    let db = dir.path().join(name);
    let spec = common::spec(graph, std::collections::HashMap::new());
    let started = Instant::now();
    let bridge = tokio::time::timeout(
        common::RUN_TIMEOUT,
        common::run_to_trace(spec, common::executor(common::RUN_TIMEOUT), &db),
    )
    .await
    .expect("timed run must finish");
    (bridge, started.elapsed())
}

/// 跑一次 `valid_parallel.json`（真 worker，两分支并行 sleep 0.2s）。
async fn run_parallel_once(dir: &tempfile::TempDir, name: &str) -> (BridgeOutcome, Duration) {
    run_graph_timed(dir, name, common::compile_fixture("valid_parallel.json")).await
}

/// 串行对照图：`start -> a -> b -> done`（同两个 `workers.parallel` 函数顺序执行）。
///
/// 用于 D8「总耗时显著小于串行」的可证伪对照：并行把两次「冷启动 + 0.2s sleep」
/// 重叠，串行则顺序累加。
fn serial_parallel_graph() -> Arc<CompiledGraph> {
    let text = json!({
        "version": "1.0",
        "metadata": {"name": "e2e_serial_baseline"},
        "entry_points": ["start"],
        "nodes": [
            {"id": "start", "type": "start", "inputs": [], "config": {}},
            {"id": "a", "type": "python_node",
             "inputs": [{"name": "x", "type": "any", "required": true}],
             "config": {"module": "workers.parallel", "function": "a"}},
            {"id": "b", "type": "python_node",
             "inputs": [{"name": "x", "type": "any", "required": true}],
             "config": {"module": "workers.parallel", "function": "b"}},
            {"id": "done", "type": "end",
             "inputs": [{"name": "r", "type": "any", "required": true}],
             "config": {}}
        ],
        "edges": [
            {"from": "start", "to_node": "a", "to_input_port": "x"},
            {"from": "a", "to_node": "b", "to_input_port": "x"},
            {"from": "b", "to_node": "done", "to_input_port": "r"}
        ]
    })
    .to_string();
    common::compile_text(&text)
}

/// 跑一次串行对照图，返回耗时（并要求 Completed）。
async fn run_serial_baseline(dir: &tempfile::TempDir, name: &str) -> Duration {
    let (bridge, elapsed) = run_graph_timed(dir, name, serial_parallel_graph()).await;
    assert_eq!(
        bridge.outcome.status,
        RunStatus::Completed,
        "串行对照图应完成: {:?}",
        bridge.outcome.error
    );
    elapsed
}

// ---------------------------------------------------------------------------
// AC-09 主用例
// ---------------------------------------------------------------------------

#[tokio::test]
async fn ac09_parallel_branches_overlap_and_total_elapsed_within_budget() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (bridge, parallel_elapsed) = run_parallel_once(&dir, "ac09_parallel.db").await;
    let serial_elapsed = run_serial_baseline(&dir, "ac09_serial.db").await;

    assert_eq!(
        bridge.outcome.status,
        RunStatus::Completed,
        "parallel graph should complete: {:?}",
        bridge.outcome.error
    );
    let run_id = bridge.outcome.run_id.clone();
    common::assert_lifecycle_shape(bridge.reader(), &run_id);
    for node in ["a", "b", "done"] {
        common::assert_instance_status(
            bridge.reader(),
            &run_id,
            node,
            TraceInstanceStatus::Completed,
        );
    }

    // 07 AC-09：Running 区间重叠 + 进入 Running 时间差 < 50ms（冻结口径）。
    let (a_start, a_end) = running_interval(bridge.reader(), &run_id, "a");
    let (b_start, b_end) = running_interval(bridge.reader(), &run_id, "b");
    assert!(
        a_start < b_end && b_start < a_end,
        "a/b 的 Running 区间必须重叠：a=[{a_start}, {a_end}] b=[{b_start}, {b_end}]"
    );
    let skew = (a_start - b_start).num_milliseconds().unsigned_abs();
    assert!(
        Duration::from_millis(skew) < BRANCH_START_SKEW_LIMIT,
        "07 AC-09: a/b 进入 Running 的时间差 {skew}ms 应 < {BRANCH_START_SKEW_LIMIT:?}（实测 0ms）"
    );

    // D8：总耗时实测重标定（绝对值上界 + 串行对照，双重可证伪）。
    assert!(
        parallel_elapsed < PARALLEL_TOTAL_BUDGET,
        "D8: 并行总耗时 {parallel_elapsed:?} 应 < {PARALLEL_TOTAL_BUDGET:?}（实测 252–292ms；\
         串行对照 {serial_elapsed:?}）"
    );
    assert!(
        parallel_elapsed.as_micros() * 100 < serial_elapsed.as_micros() * SERIAL_RATIO_PERCENT,
        "D8: 并行总耗时 {parallel_elapsed:?} 应显著小于串行对照 {serial_elapsed:?}（<80%）"
    );
}

// ---------------------------------------------------------------------------
// merge 双端口与整包广播
// ---------------------------------------------------------------------------

#[tokio::test]
async fn ac09_merge_end_receives_both_ports_with_whole_package_broadcast() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (bridge, _elapsed) = run_parallel_once(&dir, "ac09_merge.db").await;
    assert_eq!(bridge.outcome.status, RunStatus::Completed);
    let run_id = bridge.outcome.run_id.clone();

    // end 节点（merge）收到 ra/rb 两端口：run_output 同时含两个分支的整包 output。
    let output = bridge.outcome.output.as_ref().expect("end output");
    assert_eq!(output["ra"]["worker"], "a", "ra 端口来自分支 a: {output}");
    assert_eq!(output["rb"]["worker"], "b", "rb 端口来自分支 b: {output}");

    // REV-013 #3：v1 是**整包广播**——start 输出 `{}` 被整体投递到 a.x，
    // 因此 a 的 `x` 字段是整包 `{}` 而不是字段值或 null（真实投递口径固化为验收）。
    assert_eq!(
        output["ra"]["x"],
        json!({}),
        "a.x 应为 start 的整包 output（空对象），{output}"
    );
    assert_eq!(output["rb"]["x"], json!({}), "b.x 同理，{output}");

    // trace 中到 merge 的两条投递：ra、rb 各恰一条（gate_results 记录每次投递）。
    let gates = bridge.reader().gate_results(&run_id).expect("gate results");
    let to_done: Vec<_> = gates.iter().filter(|gate| gate.to_node == "done").collect();
    assert_eq!(
        to_done.len(),
        2,
        "merge 应恰好收到两次投递（ra/rb 各一），实际 {to_done:?}"
    );
    let ports: Vec<&str> = to_done.iter().map(|gate| gate.to_port.as_str()).collect();
    assert!(
        ports.contains(&"ra") && ports.contains(&"rb"),
        "ports={ports:?}"
    );
    assert!(
        to_done.iter().all(|gate| gate.passed),
        "merge 输入端口投递都应放行（无 gate）"
    );
}

// ---------------------------------------------------------------------------
// 确定性（5 连跑）
// ---------------------------------------------------------------------------

#[tokio::test]
async fn ac09_five_runs_have_deterministic_transitions_and_output() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut digests = Vec::with_capacity(DETERMINISM_RUNS);
    for index in 0..DETERMINISM_RUNS {
        let (bridge, _elapsed) = run_parallel_once(&dir, &format!("ac09_det_{index}.db")).await;
        assert_eq!(
            bridge.outcome.status,
            RunStatus::Completed,
            "run #{index} should complete"
        );
        let run_id = bridge.outcome.run_id.clone();
        digests.push(digest(bridge.reader(), &run_id));
    }
    let first = &digests[0];
    assert_eq!(first.terminal.len(), 4, "start/a/b/done 四个实例");
    assert_eq!(
        first.terminal.get("a").map(String::as_str),
        Some("Completed")
    );
    assert_eq!(
        first.terminal.get("b").map(String::as_str),
        Some("Completed")
    );
    for (index, digest) in digests.iter().enumerate().skip(1) {
        assert_eq!(
            digest.chains, first.chains,
            "run #{index} 的 transitions 迁移链必须与首轮一致（允许并行交错，但每节点链一致）"
        );
        assert_eq!(
            digest.terminal, first.terminal,
            "run #{index} 的实例终态必须一致"
        );
        assert_eq!(
            digest.output, first.output,
            "run #{index} 的 run 输出必须一致"
        );
    }
}

// ---------------------------------------------------------------------------
// 覆盖语义（v1 单数据源）：同一自环边两次投递 + DSL R5 反例
// ---------------------------------------------------------------------------

/// 自环覆盖语义图：`start(seed) -> loop(seed, again)`，`loop` 以 `trigger=any`
/// 自环回投 `again`（内置 gate `check_output` 读 `continue`）。
const SELF_LOOP_JSON: &str = r#"{
  "version": "1.0",
  "metadata": {"name": "e2e_gated_self_loop"},
  "entry_points": ["start"],
  "nodes": [
    {"id": "start", "type": "start",
     "inputs": [{"name": "seed", "type": "any", "required": true}], "config": {}},
    {"id": "loop", "type": "python_node",
     "inputs": [
       {"name": "seed", "type": "any", "required": true},
       {"name": "again", "type": "any", "required": true}
     ],
     "trigger": "any",
     "config": {"module": "workers.cycle", "function": "looping",
                "retry": {"max_attempts": 3}}}
  ],
  "edges": [
    {"from": "start", "to_node": "loop", "to_input_port": "seed"},
    {"from": "loop", "to_node": "loop", "to_input_port": "again",
     "gate": {"function": "check_output",
              "params": {"path": "continue", "equals": true}}}
  ]
}"#;

/// 同一自环边（edge 1）两次投递：第一轮放行（continue=true），第二轮被 gate 拦截。
fn assert_self_loop_deliveries(reader: &zgraph_trace::TraceReader, run_id: &str) {
    let gates = reader.gate_results(run_id).expect("gate results");
    let loop_edge: Vec<_> = gates.iter().filter(|gate| gate.edge_index == 1).collect();
    assert_eq!(
        loop_edge.len(),
        2,
        "同一自环边应记录两次投递：{loop_edge:?}"
    );
    assert!(
        loop_edge.iter().any(|gate| gate.passed) && loop_edge.iter().any(|gate| !gate.passed),
        "两次投递应一次放行、一次被拦截：{loop_edge:?}"
    );
}

/// 只断言**真正执行**的两个 attempt（幻影 attempt 见下方已知内核缺陷说明）。
fn assert_self_loop_attempts(reader: &zgraph_trace::TraceReader, run_id: &str) {
    let instances = reader.instances(run_id).expect("instances");
    let mut attempts: Vec<(u32, String)> = instances
        .iter()
        .filter(|row| row.node_id == "loop")
        .map(|row| (row.attempt, row.status.as_str().to_string()))
        .collect();
    attempts.sort_unstable();
    // 已知内核缺陷（REV-015 #3，登记 ISSUE，e2e 不改 `crates/**`）：
    // `run.rs::apply_publish` 在 gate 求值**之前**调用 `resolve_target`，被 gate 拦截的
    // 自环投递仍会创建 attempt+1（Pending→Skipped 幻影实例）；当该 attempt 序号达到
    // `retry.max_attempts` 时会把本应 Completed 的 run 误判 `NODE_INTERNAL`。
    // 这里只固化真实执行的两个 attempt，不把幻影实例当验收期望。
    assert!(
        attempts.contains(&(0, "Completed".to_string()))
            && attempts.contains(&(1, "Completed".to_string())),
        "自环应执行 attempt 0/1 并 Completed：{attempts:?}"
    );
    assert!(
        attempts
            .iter()
            .all(|(_, status)| status == "Completed" || status == "Skipped"),
        "自环 run 不应出现 Crashed/TimedOut：{attempts:?}"
    );
}

/// DSL R5：同一 `(to_node, to_port)` 只允许一条入边 → v1 单数据源
/// （内核 `buffer_write` 的「后到覆盖」分支在合法图下不可达，属防御性实现）。
fn assert_dsl_rejects_two_sources_for_one_port() {
    let bad = std::fs::read(common::graphs_root().join("bad_dup_port_source.json"))
        .expect("bad_dup_port_source fixture");
    let errors =
        zgraph_dsl::load_graph(&bad).expect_err("two sources for one port must be rejected");
    assert_eq!(
        errors.codes(),
        vec![zgraph_dsl::DslErrorCode::DuplicatePortSource],
        "同一 (to_node, to_port) 两条入边必须被 R5 拒绝"
    );
    assert_eq!(
        errors.first().code.as_str(),
        "DSL_DUPLICATE_PORT_SOURCE",
        "对外错误码字符串（00 §0.4）"
    );
}

#[tokio::test]
async fn ac09_gated_self_loop_delivers_twice_and_dsl_enforces_single_port_source() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db = dir.path().join("ac09_self_loop.db");
    let inputs = common::entry_inputs(&[("start", "seed", json!({"value": 1}))]);
    let bridge = common::run_to_trace(
        common::spec(common::compile_text(SELF_LOOP_JSON), inputs),
        common::executor(common::RUN_TIMEOUT),
        &db,
    )
    .await;
    assert_eq!(
        bridge.outcome.status,
        RunStatus::Completed,
        "self-loop graph should complete: {:?}",
        bridge.outcome.error
    );
    assert!(
        bridge.outcome.output.is_none(),
        "无 end 节点的图 run_output 应为 None（01 §1.9）"
    );
    let run_id = bridge.outcome.run_id.clone();
    assert_self_loop_deliveries(bridge.reader(), &run_id);
    assert_self_loop_attempts(bridge.reader(), &run_id);
    assert_dsl_rejects_two_sources_for_one_port();
}
