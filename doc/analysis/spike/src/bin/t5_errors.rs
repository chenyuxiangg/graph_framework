//! Probe 5: Python exception -> structured Rust error, interpreter recovery.
//!
//! A node raises a custom `NodeError(code, message)`. We catch the `PyErr`,
//! extract type/message/custom attribute/full traceback, convert it to JSON,
//! and then prove the interpreter keeps working by running both a healthy node
//! and the failing node again (3 rounds).

use pyo3::prelude::*;
use pyo3::types::PyModule;
use serde_json::{json, Value};
use zgraph_spike::{init_python, json_to_py, py_to_json, pyerr_to_json};

fn attempt_failing_node() -> Value {
    Python::attach(|py| -> PyResult<Value> {
        let module = PyModule::import(py, "node_err")?;
        let func = module.getattr("run")?;
        let inputs = json_to_py(py, &json!({"node": "failing-node", "attempt_input": 42}))?;
        match func.call1((inputs,)) {
            Ok(value) => Ok(json!({
                "unexpected_success": py_to_json(&value)?,
            })),
            Err(err) => {
                let mut record = pyerr_to_json(py, &err);
                // Custom exception attributes must survive the Rust conversion.
                if let Ok(code) = err.value(py).getattr("code") {
                    if let Ok(code) = code.extract::<String>() {
                        record["code"] = json!(code);
                    }
                }
                let tb_lines = record["traceback"].as_str().map(|s| s.lines().count()).unwrap_or(0);
                record["traceback_lines"] = json!(tb_lines);
                Ok(record)
            }
        }
    })
    .unwrap_or_else(|e| json!({"rust_side_error": e.to_string()}))
}

fn run_healthy_node() -> PyResult<Value> {
    Python::attach(|py| {
        let module = PyModule::import(py, "node_err")?;
        let func = module.getattr("run_ok")?;
        let inputs = json_to_py(py, &json!({"after": "error"}))?;
        let result = func.call1((inputs,))?;
        py_to_json(&result)
    })
}

fn sys_exc_info_cleared() -> bool {
    Python::attach(|py| -> PyResult<bool> {
        let sys = py.import("sys")?;
        let info = sys.getattr("exc_info")?.call0()?;
        Ok(info.get_item(0)?.is_none())
    })
    .unwrap_or(false)
}

fn main() {
    init_python().expect("python init");
    println!("python_version={}", Python::version_str());

    let mut healthy_rounds = 0usize;
    for attempt in 0..3 {
        let record = attempt_failing_node();
        println!(
            "RESULT attempt={attempt} record={}",
            serde_json::to_string_pretty(&record).unwrap()
        );
        let structured_ok = record["kind"].as_str() == Some("python_exception")
            && record["type"].as_str() == Some("NodeError")
            && record["code"].as_str() == Some("E_NODE_BOOM")
            && record["traceback"].as_str().map(|t| t.contains("node_err.py")).unwrap_or(false)
            && record["traceback"].as_str().map(|t| t.contains("raise NodeError")).unwrap_or(false);
        println!("RESULT attempt={attempt} structured_ok={structured_ok}");

        match run_healthy_node() {
            Ok(value) => {
                let ok = value["ok"].as_bool() == Some(true);
                println!("RESULT attempt={attempt} healthy_node_after_error ok={ok} value={}", serde_json::to_string(&value).unwrap());
                if ok {
                    healthy_rounds += 1;
                }
            }
            Err(e) => println!("RESULT attempt={attempt} healthy_node_after_error FAILED err={e}"),
        }
        println!("RESULT attempt={attempt} sys_exc_info_cleared={}", sys_exc_info_cleared());
        if !structured_ok {
            println!("PROBE5_OK=false");
            std::process::exit(1);
        }
    }

    let sys_clean = sys_exc_info_cleared();
    println!("HEALTHY_ROUNDS_AFTER_ERROR={healthy_rounds}/3 SYS_EXC_INFO_CLEARED={sys_clean}");
    println!("PROBE5_OK={}", healthy_rounds == 3 && sys_clean);
}
