//! `zgraph` CLI 二进制入口。
//!
//! 仅做进程入口与退出码；子命令实现与参数解析在库 crate `zgraph_cli` 中
//! （M1 `validate`，M2 `trace`）。退出码见
//! `doc/spec-v1.2/06-存储与可观测.md` §6.10.5。

use std::process::ExitCode;

fn main() -> ExitCode {
    zgraph_cli::run()
}
