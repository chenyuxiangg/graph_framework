//! W4a（schema v6）：item 实例定位（`item_index`/`item_key`）与唯一键
//! `(run_id,node_id,attempt,item_index)` 的写入/读取语义（DR-007 §6.3/§8.2）。
//!
//! 覆盖：item 列落库与 `InstanceRow` 读取；同 `(run,node,attempt)` 不同
//! `item_index` 共存；重复定位键被 UNIQUE 拒绝；非 foreach 实例默认 0/NULL；
//! transition 快照不覆盖已写入的 `item_key`（upsert 语义）。

mod common;

use serde_json::json;
use zgraph_trace::{
    InstanceSnapshot, InstanceStatus, TraceError, TraceInput, TraceReader, TransitionRecord,
};

fn item_snapshot(
    run_id: &str,
    instance_id: &str,
    node_id: &str,
    attempt: u32,
    item_index: u32,
    item_key: Option<&str>,
    status: InstanceStatus,
) -> InstanceSnapshot {
    InstanceSnapshot {
        run_id: run_id.to_string(),
        instance_id: instance_id.to_string(),
        node_id: node_id.to_string(),
        node_type: "python_node".to_string(),
        attempt,
        item_index,
        item_key: item_key.map(str::to_string),
        status,
        input: Some(json!({"item": item_index})),
        output: None,
        error: None,
        error_code: None,
        created_at: common::ts(0),
        started_at: None,
        finished_at: None,
    }
}

#[allow(clippy::too_many_arguments)]
fn item_transition(
    run_id: &str,
    instance_id: &str,
    node_id: &str,
    attempt: u32,
    item_index: u32,
    item_key: Option<&str>,
    from: InstanceStatus,
    to: InstanceStatus,
    at: String,
) -> TraceInput {
    TraceInput::Transition {
        record: TransitionRecord {
            run_id: run_id.to_string(),
            instance_id: instance_id.to_string(),
            node_id: node_id.to_string(),
            attempt,
            from,
            to,
            reason: Some("item test".to_string()),
            ts: at.clone(),
        },
        snapshot: item_snapshot(
            run_id,
            instance_id,
            node_id,
            attempt,
            item_index,
            item_key,
            to,
        ),
    }
}

#[test]
fn item_instance_columns_roundtrip_and_non_foreach_defaults() {
    let fx = common::Fixture::new();
    fx.handle.send(common::started("run-items"));
    // 普通（非 foreach）实例：默认 0/NULL。
    fx.handle.send(common::transition(
        "run-items",
        "inst-plain",
        "node-plain",
        InstanceStatus::Pending,
        InstanceStatus::Completed,
        common::ts(10),
    ));
    // 3 个 item：同 (node,attempt) 不同 item_index，各自 item_key。
    for (index, key) in [(0u32, None), (1, Some("id-1")), (2, Some("id-2"))] {
        fx.handle.send(item_transition(
            "run-items",
            &format!("inst-item-{index}"),
            "node-foreach",
            0,
            index,
            key,
            InstanceStatus::Pending,
            InstanceStatus::Completed,
            common::ts(20 + i64::from(index)),
        ));
    }
    fx.flush();

    let reader = fx.reader();
    let instances = reader.instances("run-items").expect("instances");
    let plain = instances
        .iter()
        .find(|row| row.instance_id == "inst-plain")
        .expect("plain instance");
    assert_eq!(plain.item_index, 0, "非 foreach 实例恒 0");
    assert_eq!(plain.item_key, None, "非 foreach 实例 item_key=NULL");
    let items: Vec<_> = instances
        .iter()
        .filter(|row| row.node_id == "node-foreach")
        .collect();
    assert_eq!(items.len(), 3, "同 (run,node,attempt) 不同 item_index 共存");
    assert_eq!(
        items.iter().map(|row| row.item_index).collect::<Vec<_>>(),
        vec![0, 1, 2],
        "reader 按插入序（rowid）返回 item 实例"
    );
    assert_eq!(items[0].item_key, None, "索引派生键为 NULL");
    assert_eq!(items[1].item_key.as_deref(), Some("id-1"));
    assert_eq!(items[2].item_key.as_deref(), Some("id-2"));
}

#[test]
fn transition_does_not_clear_previously_written_item_key() {
    let fx = common::Fixture::new();
    fx.handle.send(common::started("run-key"));
    // 先建立带 item_key 的快照，再用 item_key=None 的 transition 更新状态。
    fx.handle.send(TraceInput::InstanceSnapshot(item_snapshot(
        "run-key",
        "inst-item",
        "node-foreach",
        0,
        3,
        Some("stable-key"),
        InstanceStatus::Pending,
    )));
    fx.handle.send(item_transition(
        "run-key",
        "inst-item",
        "node-foreach",
        0,
        3,
        None,
        InstanceStatus::Pending,
        InstanceStatus::Running,
        common::ts(10),
    ));
    fx.flush();

    let reader = fx.reader();
    let instance = reader
        .instances("run-key")
        .expect("instances")
        .into_iter()
        .find(|row| row.instance_id == "inst-item")
        .expect("item instance");
    assert_eq!(
        instance.item_index, 3,
        "transition 快照携带的 item_index 生效"
    );
    assert_eq!(
        instance.item_key.as_deref(),
        Some("stable-key"),
        "upsert 不得用 NULL 覆盖已写入的 item_key"
    );
}

#[test]
fn duplicate_item_locator_is_rejected_by_unique_key() {
    let fx = common::Fixture::new();
    fx.handle.send(common::started("run-dup"));
    // 同一 (run,node,attempt,item_index) 的第二个实例（不同 instance_id）必须被
    // UNIQUE(run_id,node_id,attempt,item_index) 拒绝。
    fx.handle.send(TraceInput::InstanceSnapshot(item_snapshot(
        "run-dup",
        "inst-a",
        "node-foreach",
        0,
        0,
        None,
        InstanceStatus::Pending,
    )));
    fx.handle.send(TraceInput::InstanceSnapshot(item_snapshot(
        "run-dup",
        "inst-b",
        "node-foreach",
        0,
        0,
        None,
        InstanceStatus::Pending,
    )));
    let error = fx
        .writer
        .flush()
        .expect_err("duplicate item locator must fail");
    assert!(
        matches!(&error, TraceError::Storage(message) if message.contains("UNIQUE")),
        "unexpected error: {error:?}"
    );
}

#[test]
fn item_instances_with_different_attempt_or_node_are_distinct() {
    let fx = common::Fixture::new();
    fx.handle.send(common::started("run-distinct"));
    for (instance, node, attempt, index) in [
        ("inst-a0", "node-a", 0u32, 0u32),
        ("inst-a1", "node-a", 1, 0),
        ("inst-b0", "node-b", 0, 0),
    ] {
        fx.handle.send(TraceInput::InstanceSnapshot(item_snapshot(
            "run-distinct",
            instance,
            node,
            attempt,
            index,
            None,
            InstanceStatus::Pending,
        )));
    }
    fx.flush();

    let reader = fx.reader();
    assert_eq!(
        reader.instances("run-distinct").expect("instances").len(),
        3,
        "node/attempt 维度仍参与定位"
    );
}

#[test]
fn writer_accepts_item_rows_after_v6_open_and_reader_sees_them() {
    // 写入→关闭→重新只读打开（v6 库）后 item 字段仍可读。
    let fx = common::Fixture::new();
    fx.handle.send(common::started("run-persist"));
    fx.handle.send(item_transition(
        "run-persist",
        "inst-persist",
        "node-foreach",
        2,
        5,
        Some("key-5"),
        InstanceStatus::Pending,
        InstanceStatus::WaitingHuman,
        common::ts(10),
    ));
    fx.flush();
    // 拆开 Fixture：保留 TempDir 生命周期直到本测试结束（`close` 消费 self 会删库）。
    let common::Fixture {
        dir, path, writer, ..
    } = fx;
    let _dir = dir;
    writer.close().expect("close");

    let reader = TraceReader::open_readonly(&path).expect("reader");
    let instance = reader
        .instances("run-persist")
        .expect("instances")
        .into_iter()
        .find(|row| row.instance_id == "inst-persist")
        .expect("item instance");
    assert_eq!(instance.attempt, 2);
    assert_eq!(instance.item_index, 5);
    assert_eq!(instance.item_key.as_deref(), Some("key-5"));
    assert_eq!(instance.status, InstanceStatus::WaitingHuman);
}
