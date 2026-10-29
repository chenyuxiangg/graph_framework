//! Graph DSL 的类型、加载与校验（M1 实现）。
//!
//! 语义基准：`doc/spec-v1.2/02-DSL与校验.md`；落地偏差见 `doc/architecture-v2.md` §8。

/// DSL schema 版本（`version` 字段唯一合法值）。
pub const DSL_SCHEMA_VERSION: &str = "1.0";

#[cfg(test)]
mod tests {
    use super::DSL_SCHEMA_VERSION;

    #[test]
    fn dsl_schema_version_is_frozen() {
        assert_eq!(DSL_SCHEMA_VERSION, "1.0");
    }
}
