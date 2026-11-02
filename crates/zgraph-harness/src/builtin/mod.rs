//! v0.4/D2 内置文件工具 + workspace jail（DR-013 §2/§3）。
//!
//! - [`jail`]：workspace/allow-path 路径解析与边界校验（Rust 强制）；
//! - [`files`]：`builtin__read`/`write`/`edit`/`apply_patch`/`diff`（进程内、结果 ≤32 KiB）；
//! - [`patch`]：unified diff 解析与严格原子应用；
//! - `search`：`builtin__grep`/`builtin__glob`（ignore/globset/regex；D3）；
//! - `bash`/`sandbox`：`builtin__bash` + macOS Seatbelt 沙箱（D4）。
//!
//! 搜索类（grep/glob）与 bash 属 D3/D4，不在本模块。

pub(crate) mod bash;
mod files;
mod jail;
mod patch;
pub(crate) mod sandbox;
mod search;

pub use bash::BashOptions;
pub(crate) use bash::TOOL_BASH;
pub use files::BuiltinToolSet;

pub(crate) use files::has_builtin_prefix;
