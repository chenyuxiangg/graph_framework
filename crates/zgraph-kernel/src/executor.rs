//! 节点执行器抽象与测试用 `FakeExecutor`。
//!
//! 契约：`doc/spec-v1.2/04-节点接口与扩展点.md` §4.1（handler 单入口）与
//! `doc/architecture-v2.md` §1/§4（执行任务只持不可变快照 + 回传通道）。
//!
//! M2 用 in-process 的 [`FakeExecutor`] 打通垂直切片；M3 由 `zgraph-worker`
//! 实现真正的子进程 `NodeExecutor`（D1/D4：超时 kill 进程组），内核层接口不变。
//!
//! 内核内置处理 `start`/`end`/`subgraph`：它们**不会**被交给执行器
//! （`start` 输出 = 入口注入；`end` 输出 = inputs 且不 publish；`subgraph`
//! 在派发前被拒绝）。

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::time::Duration;

use serde_json::Value;

use crate::context::NodeContext;
use crate::graph::CompiledNode;
use crate::types::{NodeError, NodeId, PortName};

/// 一次节点执行的全部输入（不可变快照）。
pub struct NodeExecution<'a> {
    /// 编译后的节点（含 handler/超时/最大尝试次数）。
    pub node: &'a CompiledNode,
    /// 全部声明端口 → 值；未到达的端口为 `Value::Null`（00 §0.7）。
    pub inputs: &'a HashMap<PortName, Value>,
    /// 节点运行时上下文（只读快照 + 可丢 streaming）。
    pub ctx: &'a NodeContext,
}

/// 节点执行器：所有节点类型共用的唯一执行入口（04 §4.1.1）。
///
/// 实现要求：
/// - 不得修改 `RunState`/buffer；只通过返回值与 `ctx.emit` 与内核交互。
/// - 每个 `InstanceId` 至多执行一次（I1；自环/重试是新实例）。
/// - 超时由内核 deadline 判定（D1），执行器不得自行返回 `NodeError::Timeout`。
pub trait NodeExecutor: Send + Sync {
    /// 执行节点逻辑并返回整包 output（v1 整包广播，04 §4.5）。
    fn execute<'a>(
        &'a self,
        execution: NodeExecution<'a>,
    ) -> Pin<Box<dyn Future<Output = Result<Value, NodeError>> + Send + 'a>>;
}

/// `FakeExecutor` 的节点行为。
#[derive(Debug, Clone)]
pub enum FakeBehavior {
    /// 默认行为：返回 `{"echo": inputs}`（把全部端口原样回显）。
    Echo,
    /// 立即返回固定 output。
    Output(Value),
    /// 立即返回固定错误。
    Error(NodeError),
    /// 延迟后返回 output（测试事件循环不阻塞 / 超时 / 暂停用）。
    Delay {
        /// 延迟时长。
        delay: Duration,
        /// 延迟结束后的 output。
        output: Value,
    },
    /// 延迟后返回错误。
    DelayError {
        /// 延迟时长。
        delay: Duration,
        /// 延迟结束后的错误。
        error: NodeError,
    },
}

/// in-process 测试执行器：按 `node_id` 配置行为，未配置的节点走 `default`。
///
/// 用法（builder 链式）：
///
/// ```
/// use std::time::Duration;
/// use zgraph_kernel::{FakeExecutor, NodeError};
///
/// let executor = FakeExecutor::new()
///     .respond("parse", serde_json::json!({"parsed": true}))
///     .fail("boom", NodeError::Internal { message: "boom".into() })
///     .delay("slow", Duration::from_millis(10), serde_json::json!({}));
/// # let _ = executor;
/// ```
#[derive(Debug, Clone)]
pub struct FakeExecutor {
    behaviors: HashMap<NodeId, FakeBehavior>,
    default: FakeBehavior,
}

impl Default for FakeExecutor {
    fn default() -> Self {
        Self::new()
    }
}

impl FakeExecutor {
    /// 默认行为为 [`FakeBehavior::Echo`] 的空执行器。
    pub fn new() -> Self {
        Self {
            behaviors: HashMap::new(),
            default: FakeBehavior::Echo,
        }
    }

    /// 指定默认行为的执行器。
    pub fn with_default(behavior: FakeBehavior) -> Self {
        Self {
            behaviors: HashMap::new(),
            default: behavior,
        }
    }

    /// 设置默认行为（未在 `behaviors` 中配置的节点使用）。
    pub fn default_behavior(mut self, behavior: FakeBehavior) -> Self {
        self.default = behavior;
        self
    }

    /// 配置某节点立即返回固定 output。
    pub fn respond(mut self, node: impl Into<NodeId>, value: impl Into<Value>) -> Self {
        self.behaviors
            .insert(node.into(), FakeBehavior::Output(value.into()));
        self
    }

    /// 配置某节点立即返回固定错误。
    pub fn fail(mut self, node: impl Into<NodeId>, error: NodeError) -> Self {
        self.behaviors
            .insert(node.into(), FakeBehavior::Error(error));
        self
    }

    /// 配置某节点延迟后返回 output。
    pub fn delay(
        mut self,
        node: impl Into<NodeId>,
        delay: Duration,
        value: impl Into<Value>,
    ) -> Self {
        self.behaviors.insert(
            node.into(),
            FakeBehavior::Delay {
                delay,
                output: value.into(),
            },
        );
        self
    }

    /// 配置某节点延迟后返回错误。
    pub fn delay_error(
        mut self,
        node: impl Into<NodeId>,
        delay: Duration,
        error: NodeError,
    ) -> Self {
        self.behaviors
            .insert(node.into(), FakeBehavior::DelayError { delay, error });
        self
    }

    /// 取节点行为（未配置时克隆默认行为）。
    fn behavior_for(&self, node_id: &str) -> FakeBehavior {
        match self.behaviors.get(node_id) {
            Some(behavior) => behavior.clone(),
            None => self.default.clone(),
        }
    }
}

impl NodeExecutor for FakeExecutor {
    fn execute<'a>(
        &'a self,
        execution: NodeExecution<'a>,
    ) -> Pin<Box<dyn Future<Output = Result<Value, NodeError>> + Send + 'a>> {
        let behavior = self.behavior_for(&execution.node.id);
        Box::pin(async move {
            match behavior {
                FakeBehavior::Echo => Ok(echo_inputs(execution.inputs)),
                FakeBehavior::Output(output) => Ok(output),
                FakeBehavior::Error(error) => Err(error),
                FakeBehavior::Delay { delay, output } => {
                    tokio::time::sleep(delay).await;
                    Ok(output)
                }
                FakeBehavior::DelayError { delay, error } => {
                    tokio::time::sleep(delay).await;
                    Err(error)
                }
            }
        })
    }
}

/// 默认输出：`{"echo": <inputs 对象>}`。
fn echo_inputs(inputs: &HashMap<PortName, Value>) -> Value {
    let echo = serde_json::to_value(inputs).unwrap_or(Value::Null);
    let mut object = serde_json::Map::new();
    object.insert("echo".to_string(), echo);
    Value::Object(object)
}
