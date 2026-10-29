//! AC-09（M3 真 worker 口径，D8）：并行分支重叠、总耗时阈值、确定性、双端口合并与
//! 「同一端口两次投递」的 trace 可见性。
//!
//! 语义基准：`doc/spec-v1.2/07-验收用例.md` AC-09 + `doc/architecture-v2.md` §8 D8
//! （含进程冷启动后重新标定；初值 <1.0s）：
//!
//! - `a`/`b` 的 **Running 区间重叠**（用 trace `transitions.at` 的真实墙钟计算）；
//! - 总耗时实测后写死阈值（见 [`PARALLEL_TOTAL_BUDGET`] 注释中的实测值）；
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
use std::time::{Duration, Instant};

use chrono::{DateTime, FixedOffset};
use serde_json::{json, Value};

use zgraph_kernel::RunStatus;
use zgraph_trace::InstanceStatus as TraceInstanceStatus;

/// AC-09 总耗时阈值（D8：M3 实测后写死）。
///
/// **实测**（本机 macOS / Python 3.13，`cargo test -p zgraph-e2e --test parallel`，
/// 单 run 含两个 worker 冷启动 + 各 sleep 0.2s）：**0.257–0.280s**（连测 3 次；
/// `a`/`b` 进入 Running 的时间差实测 0ms——同一轮派发）；串行同图（a→b）约 0.5–0.6s。
/// D8 初值 `<1.0s` 保留为硬阈值（~3.5x 余量，CI 慢机友好），真正的并行证据是
/// 「Running 区间重叠 + 时间差 < 250ms」断言，阈值只作总耗时上限。
const PARALLEL_TOTAL_BUDGET: Duration = Duration::from_millis(1_000);

/// 两个并行分支进入 Running 的时间差上限（进程冷启动 + 派发抖动；实测 <20ms）。
const BRANCH_START_SKEW_LIMIT: Duration = Duration::from_millis(250);

/// 确定性回归的 run 次数（07 AC-09：连续 5 次）。
const DETERMINISM_RUNS: usize = 5;

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

/// 跑一次 `valid_parallel.json`（真 worker，两分支各 sleep 0.2s），返回 outcome + 耗时。
async fn run_parallel_once(
    dir: &tempfile::TempDir,
    name: &str,
) -> (zgraph_cli::bridge::BridgeOutcome, Duration) {
    let db = dir.path().join(name);
    let graph = common::compile_fixture("valid_parallel.json");
    let spec = common::spec(graph, std::collections::HashMap::new());
    let started = Instant::now();
    let bridge = tokio::time::timeout(
        common::RUN_TIMEOUT,
        common::run_to_trace(spec, common::executor(common::RUN_TIMEOUT), &db),
    )
    .await
    .expect("parallel run must finish");
    (bridge, started.elapsed())
}

#[tokio::test]
async fn ac09_parallel_branches_overlap_and_total_elapsed_within_budget() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (bridge, elapsed) = run_parallel_once(&dir, "ac09_parallel.db").await;

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

    // Running 区间重叠：真并行（不是串行碰巧达标）。
    let (a_start, a_end) = running_interval(bridge.reader(), &run_id, "a");
    let (b_start, b_end) = running_interval(bridge.reader(), &run_id, "b");
    assert!(
        a_start < b_end && b_start < a_end,
        "a/b 的 Running 区间必须重叠：a=[{a_start}, {a_end}] b=[{b_start}, {b_end}]"
    );
    let skew = (a_start - b_start).num_milliseconds().unsigned_abs();
    assert!(
        Duration::from_millis(skew) < BRANCH_START_SKEW_LIMIT,
        "a/b 进入 Running 的时间差 {skew}ms 应 < {BRANCH_START_SKEW_LIMIT:?}（同一轮派发）"
    );

    // 总耗时阈值（D8 实测后写死）。
    assert!(
        elapsed < PARALLEL_TOTAL_BUDGET,
        "AC-09 总耗时 {elapsed:?} 应 < {PARALLEL_TOTAL_BUDGET:?}（并行含冷启动；D8 实测阈值）"
    );
}

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

/// AC-09 覆盖语义（v1 单数据源）：同一自环边两次投递在 trace 中可见。
///
/// 图：`start(seed) -> loop(seed, again)`，`loop` 以 `trigger=any` 自环回投 `again`
/// （内置 gate `check_output` 读 `continue`）。`workers.cycle.looping` 首轮返回
/// `continue=true`（第 1 次投递放行 → attempt 1），次轮 `continue=false`（第 2 次投递
/// 被拦截）。trace 断言：同一 `edge_index` 两条 `gate_results`，`loop` 的
/// attempt 0/1 两个实例都完整执行到 `Completed`，run 静默 `Completed`。
///
/// v1 单数据源由 DSL R5 保证（同一 `(to_node, to_port)` 只允许一条入边）——本用例
/// 同时断言 `DSL_DUPLICATE_PORT_SOURCE` 反例；因此内核 `buffer_write` 的「后到覆盖」
/// 分支在合法图下不可达，属防御性实现（见 REV-015 遗留说明），不构造非法图来强行命中。
#[tokio::test]
async fn ac09_gated_self_loop_delivers_twice_and_dsl_enforces_single_port_source() {
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

    let dir = tempfile::tempdir().expect("tempdir");
    let db = dir.path().join("ac09_self_loop.db");
    let graph = common::compile_text(SELF_LOOP_JSON);
    let inputs = common::entry_inputs(&[("start", "seed", json!({"value": 1}))]);
    let bridge = common::run_to_trace(
        common::spec(graph, inputs),
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

    // 同一自环边（edge 1）两次投递：第一轮放行（continue=true），第二轮被 gate 拦截。
    let gates = bridge.reader().gate_results(&run_id).expect("gate results");
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

    // 两次投递各自对应一个 attempt 实例，且都完整跑到 Completed。
    let instances = bridge.reader().instances(&run_id).expect("instances");
    let mut attempts: Vec<(u32, String)> = instances
        .iter()
        .filter(|row| row.node_id == "loop")
        .map(|row| (row.attempt, row.status.as_str().to_string()))
        .collect();
    attempts.sort_unstable();
    // 只断言**真正执行**的两个 attempt。已知内核现状（REV-015 遗留 ISSUE 建议）：
    // `run.rs::apply_publish` 在 gate 求值**之前**调用 `resolve_target`，因此被 gate
    // 拦截的自环投递也会先创建 attempt+1（随后 Pending→Skipped 的「幻影 attempt」）；
    // 当 `retry.max_attempts` 恰为尝试数+1 时甚至会误判超限 → run Failed。
    // 该缺陷属 kernel 模块，e2e 不修改 `crates/**`，只固化真实执行的两个 attempt。
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

    // DSL R5：同一 (to_node, to_port) 多条入边被拒 → v1 单数据源（覆盖分支不可达）。
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
