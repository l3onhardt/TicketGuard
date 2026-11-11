//! 规则文件里的正则。
//!
//! 规则 JSON 由业务人员维护，pattern 按 Python `re` 的方言写（可能用到断言）。
//! Rust 标准的 `regex` 库不支持断言，所以这里走 `fancy-regex`。
//! pattern 在加载规则时就编译，写错了当场报错，不会等到审票时才出问题。

use fancy_regex::Regex as FancyRegex;
use std::fmt;

#[derive(Debug)]
pub struct RegexError(pub String);

impl fmt::Display for RegexError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for RegexError {}

/// 一条编译好的正则。
#[derive(Debug, Clone)]
pub struct Pattern {
    inner: FancyRegex,
}

impl Pattern {
    /// `name` 用在报错信息里，说明是哪条规则的正则写错了。
    pub fn compile(name: &str, source: &str) -> Result<Self, RegexError> {
        let inner =
            FancyRegex::new(source).map_err(|err| RegexError(format!("{name}：正则编译失败（{err}）：{source}")))?;
        Ok(Pattern { inner })
    }

    /// 等价于 Python 的 `re.search(pattern, text) is not None`。
    pub fn is_match(&self, text: &str) -> bool {
        // 回溯超限时 fancy-regex 返回 Err；这种 pattern 属于规则写得有问题，按“不匹配”处理会掩盖错误，
        // 所以直接让它暴露出来。
        self.inner.is_match(text).expect("正则匹配过程失败（pattern 回溯超限）")
    }
}
