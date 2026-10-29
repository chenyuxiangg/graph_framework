//! `NodeContext` 只读能力：user_id/get_var/cancel_requested/deadline/check_cancelled。

mod common;

use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};

use serde_json::{json, Value};
use zgraph_kernel::{NodeError, NodeExecution, NodeExecutor, RunOptions, RunSpec, RunStatus};

#[derive(Debug, Default)]
struct Probe {
    user_id: String,
    var: Option<Value>,
    var_missing_is_none: bool,
    cancel_requested: bool,
    check_cancelled_ok: bool,
    remaining_ms: u64,
}

/// 执行器内读取 NodeContext 并记录（断言放在 run 结束后，避免任务内 panic）。
struct ProbeExecutor {
    probe: Arc<Mutex<Probe>>,
}

impl NodeExecutor for ProbeExecutor {
    fn execute<'a>(
        &'a self,
        execution: NodeExecution<'a>,
    ) -> Pin<Box<dyn Future<Output = Result<Value, NodeError>> + Send + 'a>> {
        Box::pin(async move {
            if let Ok(mut probe) = self.probe.lock() {
                probe.user_id = execution.ctx.user_id().to_string();
                probe.var = execution.ctx.get_var("greeting").cloned();
                probe.var_missing_is_none = execution.ctx.get_var("absent").is_none();
                probe.cancel_requested = execution.ctx.cancel_requested();
                probe.check_cancelled_ok = execution.ctx.check_cancelled().is_ok();
                probe.remaining_ms = execution.ctx.node_deadline_remaining_ms();
            }
            Ok(json!({"ok": true}))
        })
    }
}

#[tokio::test]
async fn node_context_exposes_readonly_snapshot() {
    let probe = Arc::new(Mutex::new(Probe::default()));
    let executor = Arc::new(ProbeExecutor {
        probe: probe.clone(),
    });
    let mut spec: RunSpec = common::linear_spec();
    spec.user_id = "user-42".to_string();
    spec.global_vars
        .insert("greeting".to_string(), json!({"hello": "world"}));
    spec.options = RunOptions::default();

    let outcome = common::run(
        spec,
        executor,
        Arc::new(zgraph_kernel::MemoryTraceSink::new()),
    )
    .await;
    assert_eq!(outcome.status, RunStatus::Completed);

    let probe = probe.lock().expect("probe mutex");
    assert_eq!(probe.user_id, "user-42");
    assert_eq!(probe.var, Some(json!({"hello": "world"})));
    assert!(probe.var_missing_is_none);
    assert!(!probe.cancel_requested);
    assert!(probe.check_cancelled_ok);
    assert!(probe.remaining_ms > 0, "default timeout is 180s");
}
