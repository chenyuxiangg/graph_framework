//! `zgraph-kernel`：图编译产物、实例状态机、run 事件循环、Delivery/门控与 trace sink 契约。
//!
//! 里程碑 **M2**：用 in-process [`FakeExecutor`] 打通垂直切片
//! （真 worker 属 M3，内核不依赖 `zgraph-worker` / Python）。
//!
//! # 语义基准与偏差
//!
//! - 语义基准：`doc/spec-v1.2/00-总纲.md`、`01-领域模型与状态机.md`、
//!   `03-调度器与生命周期.md`、`04-节点接口与扩展点.md`、`07-验收用例.md`。
//! - 权威偏差：`doc/architecture-v2.md` §8（**D1–D12**）；冲突时以偏差表为准。
//!   内核直接相关的是 **D1**（超时 = kill；`NodeAbandoned` 不产生、
//!   `abandoned_tasks` 恒 0、新增 `killed_workers`）、**D6**（门控 = Rust 内置 op）、
//!   **D10**（`NodeError::NodeWorker`）与 §8.2 的 4 个 worker 错误码。
//!
//! # 快速开始
//!
//! ```
//! use std::collections::HashMap;
//! use std::sync::Arc;
//!
//! use zgraph_dsl::load_graph;
//! use zgraph_kernel::{
//!     start_run, CompiledGraph, FakeExecutor, MemoryTraceSink, RunOptions, RunSpec, RunStatus,
//! };
//!
//! # async fn demo() {
//! let bytes = br#"{
//!   "version": "1.0",
//!   "metadata": {"name": "linear"},
//!   "entry_points": ["start"],
//!   "nodes": [
//!     {"id": "start", "type": "start", "inputs": [], "config": {}},
//!     {"id": "work", "type": "python_node",
//!      "inputs": [{"name": "x", "type": "any", "required": true}],
//!      "config": {"module": "workers.demo", "function": "run"}},
//!     {"id": "done", "type": "end",
//!      "inputs": [{"name": "r", "type": "any", "required": true}], "config": {}}
//!   ],
//!   "edges": [
//!     {"from": "start", "to_node": "work", "to_input_port": "x"},
//!     {"from": "work", "to_node": "done", "to_input_port": "r"}
//!   ]
//! }"#;
//! let loaded = load_graph(bytes).expect("valid graph");
//! let graph = CompiledGraph::compile(&loaded).expect("compiled");
//! let mut inputs: HashMap<String, HashMap<String, serde_json::Value>> = HashMap::new();
//! inputs.insert(
//!     "start".to_string(),
//!     HashMap::from([("x".to_string(), serde_json::json!(1))]),
//! );
//! let spec = RunSpec {
//!     graph,
//!     entry_inputs: inputs,
//!     user_id: "u1".to_string(),
//!     global_vars: HashMap::new(),
//!     options: RunOptions::default(),
//! };
//! let sink = Arc::new(MemoryTraceSink::new());
//! let handle = start_run(None, spec, Arc::new(FakeExecutor::new()), sink).expect("started");
//! let outcome = handle.wait().await;
//! assert_eq!(outcome.status, RunStatus::Completed);
//! # }
//! ```
//!
//! # 模块索引
//!
//! - `types`（私有模块，根 re-export）：ID 别名/状态枚举/错误码/时间戳。
//! - `graph`：`CompiledGraph` 编译产物与边索引。
//! - `gate`：内置 gate op 求值（纯函数，I7）。
//! - `trace`：sink 契约 + `NullTraceSink`/`MemoryTraceSink`。
//! - `context`：节点只读上下文 `NodeContext`。
//! - `executor`：`NodeExecutor` 抽象 + `FakeExecutor`。
//! - `run`：`RunSpec`/`RunOptions`/状态机/事件循环/生命周期入口。
//!
//! # API 稳定性
//!
//! M2 冻结的公开 API（`RunSpec`/`RunOptions`/`RunHandle`/`TraceSink`/`NodeExecutor`/
//! `CompiledGraph`/`evaluate_gate` 等）供 CLI 桥接使用；新增项只能**追加**，
//! 不得删改既有签名（`doc/architecture-v2.md` §6.1 M2 DoD）。
//!
//! **命名偏差**：任务清单同时要求 `pub type EdgeIndex = usize` 与
//! `pub struct EdgeIndex`（01 §1.1.2 的边索引结构）。Rust 不允许两者同名，
//! 本 crate 保留结构体 `EdgeIndex`、数值别名改为 [`EdgeIdx`]（仍是 `usize`）。
//! 所有字段/签名语义不变，仅别名拼写不同。

#![forbid(unsafe_code)]
#![warn(missing_docs)]

mod context;
mod executor;
mod gate;
mod graph;
mod run;
mod trace;
mod types;

pub use context::NodeContext;
pub use executor::{FakeBehavior, FakeExecutor, NodeExecution, NodeExecutor};
pub use gate::{evaluate_gate, evaluate_gate_for, CompiledGate, GateEvaluation};
pub use graph::{CompiledEdge, CompiledGraph, CompiledHandler, CompiledNode, EdgeIndex};
#[cfg(feature = "test-hooks")]
pub use run::TestHooks;
pub use run::{
    run_to_completion, start_run, RunControl, RunHandle, RunOptions, RunOutcome, RunSpec,
};
pub use trace::{
    AbandonedEvent, GateResultRecord, InstanceSnapshot, MemoryTraceSink, NullTraceSink,
    RunFinishedInfo, RunStartedInfo, StreamingCall, StreamingEvent, TimedOutEvent, TraceCall,
    TraceSink, TransitionRecord,
};
pub use types::*;
