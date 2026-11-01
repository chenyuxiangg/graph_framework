//! W4a（schema v6）：`foreach_expansions` 展开事实的写入/读取语义（DR-007 §8.3/§8.4）。
//!
//! 覆盖：`TraceInput::ForeachExpansion` 落库（不产生 events 行）、`item_count=0`
//! 也写、`keys_json` 结构化 NULL 键表示、`source_instance_id` 可空、
//! `TraceReader::foreach_expansions` 按 `(at, expansion_id)` 升序、run 隔离、
//! FK（run 必须存在）与 expansion_id 主键唯一。

mod common;

use serde_json::json;
use zgraph_trace::{EventQuery, ForeachExpansionRecord, TraceError, TraceInput};

fn expansion(
    run_id: &str,
    expansion_id: &str,
    node_id: &str,
    source_instance_id: Option<&str>,
    item_count: u64,
    keys: Option<serde_json::Value>,
    at: String,
) -> TraceInput {
    TraceInput::ForeachExpansion(ForeachExpansionRecord {
        run_id: run_id.to_string(),
        expansion_id: expansion_id.to_string(),
        node_id: node_id.to_string(),
        source_instance_id: source_instance_id.map(str::to_string),
        item_count,
        keys,
        at,
    })
}

#[test]
fn expansions_roundtrip_are_ordered_by_at_then_expansion_id_and_do_not_write_events() {
    let fx = common::Fixture::new();
    fx.handle.send(common::started("run-exp"));
    fx.flush();
    let events_before = fx
        .reader()
        .events("run-exp", &EventQuery::default())
        .expect("events before")
        .len();
    assert_eq!(events_before, 1, "仅 RunStarted");

    // 插入顺序与 (at, expansion_id) 顺序不同，读取必须稳定排序。
    fx.handle.send(expansion(
        "run-exp",
        "exp-b",
        "foreach-a",
        None,
        2,
        Some(json!([{"$index": 0}, "id-1"])),
        common::ts(20),
    ));
    fx.handle.send(expansion(
        "run-exp",
        "exp-a",
        "foreach-a",
        Some("inst-source"),
        0,
        Some(json!([])),
        common::ts(10),
    ));
    fx.handle.send(expansion(
        "run-exp",
        "exp-c",
        "foreach-b",
        Some("inst-source-2"),
        3,
        None,
        common::ts(20),
    ));
    fx.flush();

    let reader = fx.reader();
    let expansions = reader.foreach_expansions("run-exp").expect("expansions");
    assert_eq!(
        expansions
            .iter()
            .map(|row| row.expansion_id.as_str())
            .collect::<Vec<_>>(),
        vec!["exp-a", "exp-b", "exp-c"],
        "按 (at, expansion_id) 升序（exp-b/exp-c 同 at 按 id 排序）"
    );
    assert_eq!(expansions[0].node_id, "foreach-a");
    assert_eq!(
        expansions[0].source_instance_id.as_deref(),
        Some("inst-source")
    );
    assert_eq!(expansions[0].item_count, 0, "空列表也写 expansion");
    assert_eq!(expansions[0].keys, Some(json!([])));
    assert_eq!(expansions[1].source_instance_id, None, "entry 注入为 NULL");
    assert_eq!(
        expansions[1].keys,
        Some(json!([{"$index": 0}, "id-1"])),
        "NULL 键以结构化 {{\"$index\": i}} 表示，真实 string key 保持原样"
    );
    assert_eq!(expansions[2].keys, None, "keys_json 可空");
    assert_eq!(expansions[2].item_count, 3);

    // 展开事实不是 TraceEvent：events 行数不变（events.kind 不变，也不占 seq）。
    let events_after = reader
        .events("run-exp", &EventQuery::default())
        .expect("events");
    assert_eq!(
        events_after.len(),
        events_before,
        "expansion 不产生 events 行"
    );
    let run = reader.run("run-exp").expect("run").expect("run row");
    assert_eq!(run.event_count, 1, "展开事实不计入 event_count");
}

#[test]
fn expansions_are_isolated_per_run() {
    let fx = common::Fixture::new();
    fx.handle.send(common::started("run-1"));
    fx.handle.send(expansion(
        "run-1",
        "exp-1",
        "foreach",
        None,
        1,
        Some(json!([{"$index": 0}])),
        common::ts(10),
    ));
    fx.handle.send(common::started("run-2"));
    fx.handle.send(expansion(
        "run-2",
        "exp-2",
        "foreach",
        None,
        2,
        Some(json!([{"$index": 0}, {"$index": 1}])),
        common::ts(20),
    ));
    fx.flush();

    let reader = fx.reader();
    assert_eq!(
        reader
            .foreach_expansions("run-1")
            .expect("run-1 expansions")
            .len(),
        1
    );
    assert_eq!(
        reader
            .foreach_expansions("run-2")
            .expect("run-2 expansions")
            .len(),
        1
    );
    assert!(reader
        .foreach_expansions("run-missing")
        .expect("missing run")
        .is_empty());
}

#[test]
fn expansion_requires_existing_run_foreign_key() {
    let fx = common::Fixture::new();
    fx.handle.send(expansion(
        "run-missing",
        "exp-orphan",
        "foreach",
        None,
        1,
        None,
        common::ts(10),
    ));
    let error = fx
        .writer
        .flush()
        .expect_err("expansion for missing run must fail FK");
    assert!(
        matches!(&error, TraceError::Storage(message)
            if message.contains("FOREIGN KEY")),
        "unexpected error: {error:?}"
    );
}

#[test]
fn item_count_overflowing_i64_is_a_storage_error() {
    let fx = common::Fixture::new();
    fx.handle.send(common::started("run-overflow"));
    fx.handle.send(expansion(
        "run-overflow",
        "exp-overflow",
        "foreach",
        None,
        u64::MAX,
        None,
        common::ts(10),
    ));
    let error = fx
        .writer
        .flush()
        .expect_err("item_count beyond i64 must be a storage error");
    assert!(
        matches!(&error, TraceError::Storage(message)
            if message.contains("foreach item_count overflow")),
        "unexpected error: {error:?}"
    );
}

#[test]
fn duplicate_expansion_id_is_rejected() {
    let fx = common::Fixture::new();
    fx.handle.send(common::started("run-pk"));
    fx.handle.send(expansion(
        "run-pk",
        "exp-same",
        "foreach",
        None,
        1,
        None,
        common::ts(10),
    ));
    fx.handle.send(expansion(
        "run-pk",
        "exp-same",
        "foreach",
        None,
        2,
        None,
        common::ts(20),
    ));
    let error = fx
        .writer
        .flush()
        .expect_err("duplicate expansion_id must fail");
    assert!(
        matches!(&error, TraceError::Storage(message) if message.contains("UNIQUE")),
        "unexpected error: {error:?}"
    );
}
