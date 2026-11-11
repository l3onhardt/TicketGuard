//! 文档解析（只移植解析层，不移植 docx/xlsx 读取）。

pub mod scenarios;
pub mod ticket_parser;

pub use scenarios::{split_scenarios, ScenarioChunk};
pub use ticket_parser::{parse_dist1, ticket_block, ParseError};
