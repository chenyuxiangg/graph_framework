//! LLM transport 抽象与类型层（05 §5.3.1/§5.3.2）。
//!
//! `genai` 类型**不得**泄漏出本模块：对外只有本文件的 Harness 自有类型、
//! [`LlmProvider`]/[`LlmStreamSink`] trait 与 `FakeLlmProvider`/`GenaiLlmProvider`。

mod fake;
mod transport;
mod types;

pub use fake::FakeLlmProvider;
pub use transport::GenaiLlmProvider;
pub use types::*;
