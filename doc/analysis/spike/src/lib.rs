//! Shared helpers for the zgraph technical spike.
//!
//! Everything here is deliberately small and dependency-light so that the
//! behaviours measured by the `src/bin/*` probes are attributable to PyO3 /
//! tokio / SQLite rather than to this crate.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use pyo3::exceptions::PyTypeError;
use pyo3::prelude::*;
use pyo3::types::{PyBool, PyDict, PyFloat, PyInt, PyList, PyString, PyTuple};
use serde_json::{json, Map, Value};

/// Directory holding the Python test modules shipped with this spike.
pub fn py_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("py")
}

/// Initialise the embedded CPython interpreter exactly once and put `py/` on
/// `sys.path` so test modules can be imported by name.
pub fn init_python() -> PyResult<()> {
    Python::initialize();
    Python::attach(|py| {
        let sys = py.import("sys")?;
        let path = sys.getattr("path")?.cast_into::<PyList>()?;
        let wanted = py_dir().to_string_lossy().to_string();
        let already = path
            .iter()
            .any(|entry| entry.extract::<String>().map(|s| s == wanted).unwrap_or(false));
        if !already {
            path.insert(0, wanted.as_str())?;
        }
        Ok(())
    })
}

/// `serde_json::Value` -> Python object (recursive, built from concrete types
/// so the mapping is explicit: null/bool/int/float/str/list/dict).
pub fn json_to_py(py: Python<'_>, value: &Value) -> PyResult<Py<PyAny>> {
    Ok(match value {
        Value::Null => py.None(),
        Value::Bool(b) => PyBool::new(py, *b).to_owned().into_any().unbind(),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                PyInt::new(py, i).into_any().unbind()
            } else {
                PyFloat::new(py, n.as_f64().expect("json number is f64-representable"))
                    .into_any()
                    .unbind()
            }
        }
        Value::String(s) => PyString::new(py, s).into_any().unbind(),
        Value::Array(items) => {
            let list = PyList::empty(py);
            for item in items {
                list.append(json_to_py(py, item)?)?;
            }
            list.into_any().unbind()
        }
        Value::Object(map) => {
            let dict = PyDict::new(py);
            for (k, v) in map {
                dict.set_item(k, json_to_py(py, v)?)?;
            }
            dict.into_any().unbind()
        }
    })
}

/// Python object -> `serde_json::Value` (recursive). Tuples become arrays,
/// dict keys must be strings. Anything else is a hard error so that the
/// design cannot silently lose type fidelity.
pub fn py_to_json(obj: &Bound<'_, PyAny>) -> PyResult<Value> {
    if obj.is_none() {
        return Ok(Value::Null);
    }
    if obj.is_instance_of::<PyBool>() {
        return Ok(Value::Bool(obj.extract()?));
    }
    if obj.is_instance_of::<PyInt>() {
        return Ok(Value::from(obj.extract::<i64>()?));
    }
    if obj.is_instance_of::<PyFloat>() {
        return Ok(Value::from(obj.extract::<f64>()?));
    }
    if obj.is_instance_of::<PyString>() {
        return Ok(Value::String(obj.extract()?));
    }
    if obj.is_instance_of::<PyList>() || obj.is_instance_of::<PyTuple>() {
        let mut arr = Vec::new();
        for item in obj.try_iter()? {
            arr.push(py_to_json(&item?)?);
        }
        return Ok(Value::Array(arr));
    }
    if obj.is_instance_of::<PyDict>() {
        let dict = obj.cast::<PyDict>()?;
        let mut map = Map::new();
        for (k, v) in dict.iter() {
            let key: String = k.extract().map_err(|_| {
                PyTypeError::new_err("serialized dict keys must be strings".to_string())
            })?;
            map.insert(key, py_to_json(&v)?);
        }
        return Ok(Value::Object(map));
    }
    let type_name = obj.get_type().name()?.to_string();
    Err(PyTypeError::new_err(format!(
        "value of Python type `{type_name}` cannot be converted to JSON"
    )))
}

/// Convert a caught `PyErr` into a structured JSON error record, including the
/// formatted Python traceback (proves the interpreter stays usable and that we
/// can give users a real traceback instead of just a one-line message).
pub fn pyerr_to_json(py: Python<'_>, err: &PyErr) -> Value {
    let type_name = err
        .get_type(py)
        .name()
        .map(|s| s.to_string())
        .unwrap_or_else(|_| "<unknown>".to_string());
    let message = err.value(py).to_string();
    let traceback = format_traceback(py, err).unwrap_or_else(|e| format!("<traceback unavailable: {e}>"));
    json!({
        "kind": "python_exception",
        "type": type_name,
        "message": message,
        "traceback": traceback,
    })
}

fn format_traceback(py: Python<'_>, err: &PyErr) -> PyResult<String> {
    let tb_mod = py.import("traceback")?;
    let tb_obj: Py<PyAny> = match err.traceback(py) {
        Some(tb) => tb.into_any().unbind(),
        None => py.None(),
    };
    let args = PyTuple::new(
        py,
        [
            err.get_type(py).into_any().unbind(),
            err.value(py).clone().into_any().unbind(),
            tb_obj,
        ],
    )?;
    let lines: Vec<String> = tb_mod
        .call_method1("format_exception", args)?
        .extract()?;
    Ok(lines.concat())
}

/// Counters shared between the Rust test and the `#[pyclass]` context handed to
/// Python, so that the Rust side can read them without re-entering the GIL.
#[derive(Default)]
pub struct EmitStats {
    pub sent: AtomicUsize,
    pub dropped_full: AtomicUsize,
    pub closed: AtomicUsize,
    pub reentries: AtomicUsize,
}

/// The `ctx` object that Python nodes/gates receive. Mirrors the design's
/// `NodeContext.emit(dict)` synchronous callback.
///
/// Deliberately holds a bounded `tokio::sync::mpsc::Sender` and uses
/// `try_send`: this is exactly the "sync facade over a bounded channel" that
/// the design proposes for streaming events.
#[pyclass]
pub struct EmitCtx {
    tx: tokio::sync::mpsc::Sender<Value>,
    stats: Arc<EmitStats>,
    reenter_python: bool,
}

impl EmitCtx {
    pub fn new(
        tx: tokio::sync::mpsc::Sender<Value>,
        stats: Arc<EmitStats>,
        reenter_python: bool,
    ) -> Self {
        Self {
            tx,
            stats,
            reenter_python,
        }
    }
}

#[pymethods]
impl EmitCtx {
    /// Synchronous emit. Must never block: the Python caller holds the GIL.
    fn emit(&self, py: Python<'_>, event: &Bound<'_, PyAny>) -> PyResult<()> {
        let mut value = py_to_json(event)?;

        if self.reenter_python {
            // Prove that calling back into Python from inside a Python->Rust
            // callback is safe (GIL is re-entrant), and that a nested
            // Python::attach is a no-op rather than a deadlock.
            let json_mod = py.import("json")?;
            let dumped: String = json_mod
                .getattr("dumps")?
                .call1((event,))?
                .extract::<String>()?;
            Python::attach(|_py| {
                self.stats.reentries.fetch_add(1, Ordering::SeqCst);
            });
            if let Value::Object(map) = &mut value {
                map.insert("dumps_len".to_string(), Value::from(dumped.len() as i64));
            }
        }

        match self.tx.try_send(value) {
            Ok(()) => {
                self.stats.sent.fetch_add(1, Ordering::SeqCst);
                Ok(())
            }
            Err(tokio::sync::mpsc::error::TrySendError::Full(_)) => {
                self.stats.dropped_full.fetch_add(1, Ordering::SeqCst);
                Ok(())
            }
            Err(tokio::sync::mpsc::error::TrySendError::Closed(_)) => {
                self.stats.closed.fetch_add(1, Ordering::SeqCst);
                Ok(())
            }
        }
    }

    /// Exposed so Python test modules can introspect the context cheaply.
    #[getter]
    fn stats(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        json_to_py(
            py,
            &json!({
                "sent": self.stats.sent.load(Ordering::SeqCst),
                "dropped_full": self.stats.dropped_full.load(Ordering::SeqCst),
                "closed": self.stats.closed.load(Ordering::SeqCst),
                "reentries": self.stats.reentries.load(Ordering::SeqCst),
            }),
        )
    }
}
