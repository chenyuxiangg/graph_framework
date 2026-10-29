//! 状态机迁移矩阵（01 §1.2.3 的可执行版本）与真实 run 的迁移链校验。

mod common;

use zgraph_kernel::{InstanceStatus as S, RecordMode, RunOptions, RunStatus};

const ALL: [S; 8] = [
    S::Pending,
    S::Ready,
    S::Running,
    S::Completed,
    S::Crashed,
    S::TimedOut,
    S::Cancelled,
    S::Skipped,
];

#[test]
fn legal_transitions_matrix() {
    let legal = [
        (S::Pending, S::Ready),
        (S::Pending, S::Cancelled),
        (S::Pending, S::Skipped),
        (S::Ready, S::Running),
        (S::Ready, S::Cancelled),
        (S::Running, S::Completed),
        (S::Running, S::Crashed),
        (S::Running, S::TimedOut),
        (S::Running, S::Cancelled),
    ];
    let legal_count = legal.len();
    let mut observed = 0;
    for from in ALL {
        for to in ALL {
            let expected = legal.contains(&(from, to));
            assert_eq!(
                from.can_transition_to(to),
                expected,
                "matrix entry {} -> {}",
                from.as_str(),
                to.as_str()
            );
            if expected {
                observed += 1;
            }
        }
    }
    assert_eq!(observed, legal_count);
}

#[test]
fn illegal_transitions_include_terminal_and_skips() {
    // 终态不得再迁移。
    for terminal in [
        S::Completed,
        S::Crashed,
        S::TimedOut,
        S::Cancelled,
        S::Skipped,
    ] {
        assert!(terminal.is_terminal());
        for to in ALL {
            assert!(!terminal.can_transition_to(to), "terminal cannot move");
        }
    }
    // 明确的非法组合（fail-fast 用例）。
    let illegal = [
        (S::Pending, S::Running),
        (S::Pending, S::Completed),
        (S::Pending, S::Crashed),
        (S::Pending, S::TimedOut),
        (S::Ready, S::Completed),
        (S::Ready, S::Crashed),
        (S::Ready, S::Skipped),
        (S::Ready, S::Pending),
        (S::Running, S::Ready),
        (S::Running, S::Pending),
        (S::Running, S::Skipped),
        (S::Running, S::Running),
    ];
    for (from, to) in illegal {
        assert!(
            !from.can_transition_to(to),
            "{} -> {} must be illegal",
            from.as_str(),
            to.as_str()
        );
    }
}

#[test]
fn instance_status_names_and_terminals() {
    assert_eq!(S::Pending.as_str(), "Pending");
    assert_eq!(S::Ready.as_str(), "Ready");
    assert_eq!(S::Running.as_str(), "Running");
    assert_eq!(S::Completed.as_str(), "Completed");
    assert_eq!(S::Crashed.as_str(), "Crashed");
    assert_eq!(S::TimedOut.as_str(), "TimedOut");
    assert_eq!(S::Cancelled.as_str(), "Cancelled");
    assert_eq!(S::Skipped.as_str(), "Skipped");
    assert!(!S::Pending.is_terminal());
    assert!(!S::Ready.is_terminal());
    assert!(!S::Running.is_terminal());
}

#[test]
fn run_status_names_and_terminals() {
    assert!(!RunStatus::Running.is_terminal());
    assert!(!RunStatus::Paused.is_terminal());
    for (status, name) in [
        (RunStatus::Running, "Running"),
        (RunStatus::Paused, "Paused"),
        (RunStatus::Completed, "Completed"),
        (RunStatus::Failed, "Failed"),
        (RunStatus::Cancelled, "Cancelled"),
    ] {
        assert_eq!(status.as_str(), name);
    }
    assert!(RunStatus::Completed.is_terminal());
    assert!(RunStatus::Failed.is_terminal());
    assert!(RunStatus::Cancelled.is_terminal());
}

#[tokio::test]
async fn real_run_records_legal_transition_chain() {
    let executor = std::sync::Arc::new(
        zgraph_kernel::FakeExecutor::new().respond("work", serde_json::json!({"answer": 1})),
    );
    let (outcome, sink) = common::run_with_sink(common::linear_spec(), executor).await;
    assert_eq!(outcome.status, RunStatus::Completed);

    let transitions = sink.transitions();
    assert!(!transitions.is_empty());
    for record in &transitions {
        assert!(
            record.from.can_transition_to(record.to),
            "illegal recorded transition {} -> {} (node {})",
            record.from.as_str(),
            record.to.as_str(),
            record.node_id
        );
        assert!(
            record.ts.ends_with('Z'),
            "transition ts must be RFC3339 Z: {}",
            record.ts
        );
        assert_eq!(record.run_id, outcome.run_id);
    }

    for node_id in ["start", "work", "done"] {
        let chain: Vec<(S, S)> = transitions
            .iter()
            .filter(|record| record.node_id == node_id)
            .map(|record| (record.from, record.to))
            .collect();
        assert_eq!(
            chain.first(),
            Some(&(S::Pending, S::Ready)),
            "first transition of {node_id}"
        );
        let last = chain.last().expect("has transitions");
        assert!(
            last.1.is_terminal(),
            "last transition of {node_id} is terminal"
        );
        let reasons: Vec<Option<String>> = transitions
            .iter()
            .filter(|record| record.node_id == node_id)
            .map(|record| record.reason.clone())
            .collect();
        assert_eq!(reasons[0].as_deref(), Some("inputs ready"));
        assert_eq!(reasons[1].as_deref(), Some("dispatch"));
        assert_eq!(reasons[2].as_deref(), Some("execution ok"));
    }
}

#[tokio::test]
async fn snapshots_cover_creation_and_every_transition() {
    let executor = std::sync::Arc::new(zgraph_kernel::FakeExecutor::new());
    let (outcome, sink) = common::run_with_sink(common::linear_spec(), executor).await;
    assert_eq!(outcome.status, RunStatus::Completed);

    let work_snapshots = common::snapshots_of(&sink, "work");
    let work_transitions = sink
        .transitions()
        .into_iter()
        .filter(|record| record.node_id == "work")
        .count();
    // 1 条创建（Pending）+ 每次状态变化 1 条快照。
    assert_eq!(work_snapshots.len(), work_transitions + 1);
    assert_eq!(work_snapshots.first().expect("created").status, S::Pending);
    assert_eq!(
        work_snapshots.last().expect("finished").status,
        S::Completed
    );
}

#[test]
fn run_options_defaults_match_spec() {
    let options = RunOptions::default();
    assert_eq!(options.deadlock_grace_ms, 5_000);
    assert_eq!(options.tick_interval_ms, 1_000);
    assert!(options.trace_enabled);
    assert_eq!(options.seed, 0);
    assert_eq!(options.record_mode, RecordMode::Hash);
    assert_eq!(RecordMode::None.as_str(), "none");
    assert_eq!(RecordMode::Hash.as_str(), "hash");
    assert_eq!(RecordMode::Full.as_str(), "full");
}
