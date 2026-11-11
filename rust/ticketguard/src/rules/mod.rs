//! 规则引擎（加载、校验、执行）。

pub mod engine;

pub use engine::{load_rules, run_rules, Rule};
