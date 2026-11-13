//! 票审 TicketGuard 的 Rust 核心库。移植自 Python 参考实现 `src/ticketguard/`。
//!
//! 移植期的硬性要求：本库对同一份输入要产出与参考实现逐字节一致的输出，
//! 由 `rust/tests/tests/parity.rs` 用 fixture 对拍。

pub mod entities;
pub mod evaluate;
pub mod harness;
pub mod ingest;
pub mod regex;
pub mod report;
pub mod review;
pub mod rules;
pub mod schema;
pub mod synth;
pub mod templates;
pub mod text;
pub mod time;
pub mod ui;

/// 统一错误：库里任何一层出问题都带上中文说明，由命令行入口转成用户能看懂的一句话。
#[derive(Debug)]
pub struct Error(pub String);

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Error {}

impl From<entities::DataError> for Error {
    fn from(err: entities::DataError) -> Self {
        Error(err.0)
    }
}

impl From<ingest::ticket_parser::ParseError> for Error {
    fn from(err: ingest::ticket_parser::ParseError) -> Self {
        Error(err.0)
    }
}
