//! 存储层错误类型。
//!
//! 错误码命名空间（06 §6.12.3 D3 / 00 §0.4 注）：`STORAGE_ERROR` /
//! `STORE_SCHEMA_TOO_NEW` 属 **CLI/storage 命名空间**，不写入领域
//! `runs.error_code` 列；[`TraceError::code`] 供 CLI 映射退出码 4。

use thiserror::Error;

/// trace 持久层错误。
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum TraceError {
    /// 存储/IO 不可用（打不开库、缺表、写失败、只读库缺失等）。
    ///
    /// 对应 CLI 命名空间 `STORAGE_ERROR`（06 §6.12.3 D3），退出码 4。
    #[error("STORAGE_ERROR: {0}")]
    Storage(String),

    /// 数据库 schema 版本高于本二进制支持的版本（06 §6.7 步骤 5）。
    ///
    /// 对应 CLI 命名空间 `STORE_SCHEMA_TOO_NEW`，退出码 4；打开阶段直接拒绝，
    /// 不执行任何迁移。
    #[error("STORE_SCHEMA_TOO_NEW: db version={found}, supported={supported}")]
    SchemaTooNew {
        /// 库文件中的 `PRAGMA user_version`。
        found: i64,
        /// 本 crate 支持的版本（[`crate::TRACE_SCHEMA_VERSION`]）。
        supported: i64,
    },

    /// 请求的实体不存在（例如 `trace --stats` 的 run 不存在）。
    ///
    /// CLI 映射为退出码 4 的存储错误；消息中的实体信息由调用方拼接。
    #[error("STORAGE_ERROR: {0}")]
    NotFound(String),

    /// JSON 序列化/反序列化失败（理论上仅超大 payload 或损坏列触发）。
    #[error("STORAGE_ERROR: {0}")]
    Serialize(String),
}

impl TraceError {
    /// 稳定对外错误码（CLI 命名空间，非领域码）。
    pub const fn code(&self) -> &'static str {
        match self {
            Self::SchemaTooNew { .. } => "STORE_SCHEMA_TOO_NEW",
            Self::Storage(_) | Self::NotFound(_) | Self::Serialize(_) => "STORAGE_ERROR",
        }
    }
}
