//! Probe 4: gate evaluation `gate(output, ctx) -> bool`.
//!
//! Checks serde_json::Value -> Python -> bool conversion, what happens when a
//! gate returns a truthy non-bool, and whether a raising gate corrupts the
//! interpreter for subsequent evaluations.

use pyo3::prelude::*;
use pyo3::types::PyModule;
use serde_json::{json, Value};
use zgraph_spike::{init_python, json_to_py, pyerr_to_json};

fn run_gate(name: &str, output: &Value, expect_bool: bool) -> Value {
    Python::attach(|py| -> PyResult<Value> {
        let module = PyModule::import(py, "gate")?;
        let func = module.getattr(name)?;
        let call = func.call1((json_to_py(py, output)?, py.None()));
        let bound = match call {
            Ok(bound) => bound,
            Err(err) => {
                let mut record = pyerr_to_json(py, &err);
                record["gate"] = json!(name);
                record["call_error"] = json!(true);
                if let Some(tb) = record["traceback"].as_str() {
                    record["traceback_lines"] = json!(tb.lines().count());
                }
                return Ok(record);
            }
        };
        // Strict conversion: PyO3's `extract::<bool>()` only accepts real `bool`
        // (plus numpy bool), NOT arbitrary truthy objects.
        let strict: Result<bool, String> = bound.extract::<bool>().map_err(|e| {
            let type_name = e
                .get_type(py)
                .name()
                .map(|s| s.to_string())
                .unwrap_or_default();
            format!("{type_name}: {e}")
        });
        // Deliberate truthiness alternative, for comparison.
        let truthy: Result<bool, String> = bound.is_truthy().map_err(|e| e.to_string());
        Ok(json!({
            "gate": name,
            "expect_bool": expect_bool,
            "strict_bool": match strict { Ok(b) => json!({"ok": b}), Err(e) => json!({"err": e}) },
            "is_truthy": match truthy { Ok(b) => json!({"ok": b}), Err(e) => json!({"err": e}) },
            "python_type": bound.get_type().name().map(|s| s.to_string()).unwrap_or_default(),
        }))
    })
    .unwrap_or_else(|e| json!({"gate": name, "rust_error": e.to_string()}))
}

fn main() {
    init_python().expect("python init");
    println!("python_version={}", Python::version_str());

    let output = json!({"score": 0.9, "node": "n1", "nested": {"x": [1, 2, null]}});
    let low = json!({"score": 0.1});

    let mut all_ok = true;
    for (name, out, expect_true) in [
        ("gate_pass", &output, true),
        ("gate_pass", &low, false),
        ("gate_fail", &output, false),
        ("gate_truthy", &output, true),
        ("gate_int", &output, true),
        ("gate_none", &output, false),
        ("gate_raise", &output, false),
    ] {
        let result = run_gate(name, out, expect_true);
        println!("RESULT gate={name} expect_bool={expect_true} detail={}", serde_json::to_string(&result).unwrap());
    }

    // A raising gate must not poison the interpreter: evaluate a normal gate again.
    let after_error = run_gate("gate_pass", &output, true);
    let after_ok = after_error["strict_bool"]["ok"].as_bool() == Some(true);
    println!("RESULT after_gate_error gate_pass strict_bool_ok={after_ok}");
    all_ok &= after_ok;

    // Also verify sys.exc_info() is clean after the caught gate exception.
    let exc_cleared = Python::attach(|py| -> PyResult<bool> {
        let sys = py.import("sys")?;
        let info = sys.getattr("exc_info")?.call0()?;
        let first = info.get_item(0)?;
        Ok(first.is_none())
    })
    .unwrap_or(false);
    println!("GATE_INTERPRETER_HEALTHY_AFTER_EXCEPTION={after_ok} SYS_EXC_INFO_CLEARED={exc_cleared}");
    all_ok &= exc_cleared;
    println!("PROBE4_OK={all_ok}");
}
