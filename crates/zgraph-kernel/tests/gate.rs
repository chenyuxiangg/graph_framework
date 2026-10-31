//! 内置 gate op 求值语义单测（模块文档契约的可执行版本）。

use serde_json::{json, Value};
use zgraph_kernel::{evaluate_gate, evaluate_gate_for, CompiledGate};

fn gate(op: &str, params: Value) -> CompiledGate {
    CompiledGate::compile(op, params).expect("built-in op compiles")
}

fn passed(op: &str, params: Value, output: Value) -> bool {
    evaluate_gate(&gate(op, params), &output).passed
}

#[test]
fn compile_rejects_unknown_op() {
    assert!(CompiledGate::compile("gate_value", json!({})).is_err());
}

#[test]
fn check_output_equals_deep_equal() {
    let params = json!({"path": "result.ok", "equals": true});
    assert!(passed(
        "check_output",
        params.clone(),
        json!({"result": {"ok": true}})
    ));
    assert!(!passed(
        "check_output",
        params,
        json!({"result": {"ok": false}})
    ));
}

#[test]
fn check_output_equals_missing_path_is_false_without_error() {
    let evaluation = evaluate_gate(
        &gate("check_output", json!({"path": "nope", "equals": 1})),
        &json!({"other": 1}),
    );
    assert!(!evaluation.passed);
    assert_eq!(evaluation.error, None);
}

#[test]
fn check_output_non_object_intermediate_is_missing_path() {
    let params = json!({"path": "a.b.c", "equals": 1});
    assert!(!passed("check_output", params, json!({"a": {"b": [1, 2]}})));
}

#[test]
fn check_output_in_membership() {
    let params = json!({"path": "kind", "in": ["a", "b"]});
    assert!(passed("check_output", params.clone(), json!({"kind": "b"})));
    assert!(!passed("check_output", params, json!({"kind": "c"})));
}

#[test]
fn check_output_in_non_array_is_error() {
    let evaluation = evaluate_gate(
        &gate("check_output", json!({"path": "kind", "in": "a"})),
        &json!({"kind": "a"}),
    );
    assert!(!evaluation.passed);
    assert!(evaluation
        .error
        .expect("error")
        .starts_with("check_output:"));
}

#[test]
fn check_output_exists_treats_null_as_present() {
    let params = json!({"path": "value", "exists": true});
    assert!(passed("check_output", params, json!({"value": null})));
    assert!(!passed(
        "check_output",
        json!({"path": "value", "exists": true}),
        json!({})
    ));
    assert!(passed(
        "check_output",
        json!({"path": "value", "exists": false}),
        json!({})
    ));
}

#[test]
fn check_output_invalid_params_is_error() {
    let evaluation = evaluate_gate(&gate("check_output", json!({"path": "x"})), &json!({}));
    assert!(!evaluation.passed);
    assert!(evaluation.error.is_some());
}

#[test]
fn compare_eq_ne_deep_equal() {
    assert!(passed(
        "compare",
        json!({"path": "v", "op": "eq", "value": {"a": [1]}}),
        json!({"v": {"a": [1]}})
    ));
    assert!(!passed(
        "compare",
        json!({"path": "v", "op": "eq", "value": 1}),
        json!({"v": 2})
    ));
    assert!(passed(
        "compare",
        json!({"path": "v", "op": "ne", "value": 1}),
        json!({"v": 2})
    ));
}

#[test]
fn compare_order_numeric() {
    let output = json!({"n": 3});
    for (op, expected) in [("gt", false), ("gte", true), ("lt", false), ("lte", true)] {
        let params = json!({"path": "n", "op": op, "value": 3});
        assert_eq!(
            passed("compare", params, output.clone()),
            expected,
            "op {op}"
        );
    }
    assert!(passed(
        "compare",
        json!({"path": "n", "op": "gt", "value": 2.5}),
        output
    ));
}

#[test]
fn compare_order_type_mismatch_reports_error() {
    let evaluation = evaluate_gate(
        &gate("compare", json!({"path": "n", "op": "gt", "value": 3})),
        &json!({"n": "3"}),
    );
    assert!(!evaluation.passed);
    let error = evaluation.error.expect("type error recorded");
    assert!(
        error.starts_with("compare:") && error.contains("numeric"),
        "unexpected error: {error}"
    );

    let evaluation = evaluate_gate(
        &gate("compare", json!({"path": "n", "op": "gt", "value": "3"})),
        &json!({"n": 3}),
    );
    assert!(!evaluation.passed);
    assert!(evaluation.error.is_some());
}

#[test]
fn compare_missing_path_is_false_without_error() {
    for op in ["eq", "ne", "gt", "lt", "in", "not_in"] {
        let value = if op == "in" || op == "not_in" {
            json!([1])
        } else {
            json!(1)
        };
        let evaluation = evaluate_gate(
            &gate(
                "compare",
                json!({"path": "missing", "op": op, "value": value}),
            ),
            &json!({}),
        );
        assert!(!evaluation.passed, "op {op}");
        assert_eq!(evaluation.error, None, "op {op}");
    }
}

#[test]
fn compare_membership() {
    let params = json!({"path": "v", "op": "in", "value": [1, 2]});
    assert!(passed("compare", params.clone(), json!({"v": 2})));
    assert!(!passed("compare", params, json!({"v": 3})));

    let params = json!({"path": "v", "op": "not_in", "value": [1, 2]});
    assert!(passed("compare", params, json!({"v": 3})));

    let evaluation = evaluate_gate(
        &gate("compare", json!({"path": "v", "op": "in", "value": 1})),
        &json!({"v": 1}),
    );
    assert!(!evaluation.passed);
    assert!(evaluation.error.is_some());
}

#[test]
fn compare_unknown_op_is_error() {
    let evaluation = evaluate_gate(
        &gate("compare", json!({"path": "v", "op": "matches", "value": 1})),
        &json!({"v": 1}),
    );
    assert!(!evaluation.passed);
    assert!(evaluation.error.is_some());
}

#[test]
fn manual_uses_default() {
    assert!(!passed("manual", json!({}), json!({})));
    assert!(!passed("manual", json!({"default": false}), json!({})));
    assert!(passed("manual", json!({"default": true}), json!({})));
    let evaluation = evaluate_gate(&gate("manual", json!({"default": "yes"})), &json!({}));
    assert!(!evaluation.passed);
    assert!(evaluation.error.is_some());
}

#[test]
fn evaluate_gate_for_records_edge_index() {
    let gate = gate("check_output", json!({"path": "ok", "equals": true}));
    let evaluation = evaluate_gate_for(7, &gate, &json!({"ok": true}));
    assert_eq!(evaluation.edge_index, 7);
    assert!(evaluation.passed);
    // evaluate_gate 是便捷入口，edge_index 固定 0。
    assert_eq!(evaluate_gate(&gate, &json!({"ok": true})).edge_index, 0);
}

#[test]
fn gate_is_pure_does_not_mutate_output() {
    let output = json!({"ok": true});
    let snapshot = output.clone();
    let _ = evaluate_gate(
        &gate("check_output", json!({"path": "ok", "equals": true})),
        &output,
    );
    assert_eq!(output, snapshot);
}

fn verdict(op: &str, params: Value, output: Value) -> Option<Value> {
    evaluate_gate(&gate(op, params), &output).verdict
}

/// W2a0：`check_output` 的 verdict 是路径原始值（即便 gate 判定不通过）。
#[test]
fn check_output_verdict_is_path_value_even_when_gate_blocks() {
    assert_eq!(
        verdict(
            "check_output",
            json!({"path": "verdict", "equals": "approved"}),
            json!({"verdict": "rework"})
        ),
        Some(json!("rework"))
    );
    assert_eq!(
        verdict(
            "check_output",
            json!({"path": "nested.value", "exists": true}),
            json!({"nested": {"value": false}})
        ),
        Some(json!(false)),
        "exists 判定通过时也取路径值"
    );
}

/// 路径不存在 / 中间非 object → verdict=None（不是错误，只是没有判定值）。
#[test]
fn verdict_is_none_when_path_missing_or_intermediate_not_object() {
    assert_eq!(
        verdict(
            "check_output",
            json!({"path": "missing", "equals": 1}),
            json!({})
        ),
        None
    );
    assert_eq!(
        verdict(
            "check_output",
            json!({"path": "a.b.c", "equals": 1}),
            json!({"a": {"b": [1]}})
        ),
        None
    );
}

/// W2a0：`compare` 同样取 `params.path` 的路径值。
#[test]
fn compare_verdict_is_path_value() {
    assert_eq!(
        verdict(
            "compare",
            json!({"path": "n", "op": "gt", "value": 2}),
            json!({"n": 3})
        ),
        Some(json!(3))
    );
    assert_eq!(
        verdict(
            "compare",
            json!({"path": "v", "op": "in", "value": [1]}),
            json!({"v": 1})
        ),
        Some(json!(1))
    );
}

/// manual / 非字符串 path / 求值异常 → verdict=None（DR-005 §4.1）。
#[test]
fn manual_and_error_evaluations_have_no_verdict() {
    assert_eq!(
        verdict("manual", json!({"default": true}), json!({"v": 1})),
        None
    );
    assert_eq!(
        verdict(
            "check_output",
            json!({"path": 1, "equals": 1}),
            json!({"v": 1})
        ),
        None
    );
    assert_eq!(
        verdict(
            "check_output",
            json!({"path": "v", "in": "x"}),
            json!({"v": 1})
        ),
        None,
        "path 是 string 但求值异常 → None"
    );
}
