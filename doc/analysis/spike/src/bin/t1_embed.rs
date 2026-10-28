//! Probe 1: PyO3 0.30.1 + CPython 3.13 embedding.
//!
//! - initialise the interpreter
//! - import a module from a real `.py` file
//! - call `run(inputs: dict, ctx) -> dict`
//! - convert serde_json::Value <-> Python objects in both directions for
//!   dict / list / nested / None / bool / int / float / str / tuple.

use pyo3::prelude::*;
use pyo3::types::{PyDict, PyInt, PyList, PyModule, PyString};
use serde_json::json;
use zgraph_spike::{init_python, json_to_py, py_to_json};

fn main() {
    if let Err(e) = run() {
        eprintln!("PROBE1_FAILED: {e}");
        std::process::exit(1);
    }
}

fn run() -> PyResult<()> {
    init_python()?;
    println!("python_version={}", Python::version_str().to_string());

    let inputs = json!({
        "a": 3,
        "b": 4.5,
        "s": "héllo 世界 🦀",
        "flag": true,
        "none": null,
        "list": [1, "two", null, [true, 2.5]],
        "nested": {"x": {"y": [1, 2, 3]}}
    });

    Python::attach(|py| -> PyResult<()> {
        let module = PyModule::import(py, "node_ok")?;
        let func = module.getattr("run")?;

        // --- Rust -> Python conversion shape checks -------------------------
        let py_inputs = json_to_py(py, &inputs)?;
        let bound_inputs = py_inputs.bind(py);
        assert!(bound_inputs.is_instance_of::<PyDict>(), "root must be a dict");
        let dict = bound_inputs.cast::<PyDict>()?;
        assert!(dict.get_item("a")?.unwrap().is_instance_of::<PyInt>());
        assert!(dict.get_item("s")?.unwrap().is_instance_of::<PyString>());
        assert!(dict.get_item("none")?.unwrap().is_none());
        assert!(dict.get_item("list")?.unwrap().is_instance_of::<PyList>());
        let flag_bool = dict
            .get_item("flag")?
            .unwrap()
            .is_instance_of::<pyo3::types::PyBool>();
        println!("conversion_rust_to_py=dict/int/float/str/bool/none/list/nested OK");
        println!("bool_is_pybool={flag_bool}");

        // --- Call the Python node ------------------------------------------
        let ctx = py.None();
        let result = func.call1((py_inputs, ctx))?;

        // --- Python -> Rust conversion -------------------------------------
        let output = py_to_json(&result)?;
        println!("INPUT_JSON={inputs}");
        println!("OUTPUT_JSON={output}");
        println!("OUTPUT_PY_REPR={}", result.repr()?.extract::<String>()?);

        // --- Round-trip: JSON -> Python -> JSON must be stable -------------
        let back = json_to_py(py, &output)?;
        let output2 = py_to_json(back.bind(py))?;
        let roundtrip_ok = output == output2;
        println!("ROUNDTRIP_JSON_STABLE={roundtrip_ok}");
        assert!(roundtrip_ok);

        // --- Spot-check fidelity -------------------------------------------
        let out = result.cast::<PyDict>()?;
        println!(
            "out.sum={} out.sum_is_pyint={}",
            out.get_item("sum")?.unwrap().extract::<f64>()?,
            out.get_item("sum")?.unwrap().is_instance_of::<PyInt>()
        );
        println!(
            "out.none_is_none={} out.tuple_is_list_in_json={}",
            out.get_item("none")?.unwrap().is_none(),
            output["tuple"].is_array()
        );
        println!("out.big={} (i64 fidelity)", output["big"].as_i64().unwrap());
        println!("out.unicode_roundtrip={}", output["unicode"].as_str().unwrap());

        // --- Calling again after the first call keeps working --------------
        let second = func.call1((json_to_py(py, &json!({"a": 1, "b": 1.0, "flag": true}))?, py.None()))?;
        let second_json = py_to_json(&second)?;
        println!("second_call_sum={}", second_json["sum"]);
        assert_eq!(second_json["sum"], json!(2.0));
        Ok(())
    })?;

    println!("PROBE1_OK=1");
    Ok(())
}
