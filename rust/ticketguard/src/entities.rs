//! 实体抽取与归一化。移植自参考实现 `src/ticketguard/entities.py`。
//!
//! 参考实现的正则用了断言（`(?<![\d.])`、`(?=...)`），所以这里全部走 `fancy-regex`。

use fancy_regex::Regex;
use once_cell::sync::Lazy;
use serde::Deserialize;
use serde_json::Value;
use std::fmt;
use std::path::Path;

/// 数据文件不合规范时的错误。消息用中文，带上文件路径。
#[derive(Debug)]
pub struct DataError(pub String);

impl fmt::Display for DataError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for DataError {}

/// 同义词表与关键词表（`data/rules/lexicon.json`）。
#[derive(Debug, Clone, Deserialize)]
pub struct Lexicon {
    pub synonyms: serde_json::Map<String, Value>,
    pub keywords: Vec<String>,
    /// (变体, 规范写法)，按变体长度降序。加载时算一次。
    #[serde(skip)]
    variants: Vec<(String, String)>,
    /// 归一化后的关键词，与 `keywords` 一一对应。
    #[serde(skip)]
    normalized_keywords: Vec<String>,
}

const NAME: &str = r"[^\s，,、；;。:：/kV“”]+?";

static PARENS: Lazy<Regex> = Lazy::new(|| Regex::new(r"（[^（）]*）|\([^()]*\)").unwrap());
static STATION: Lazy<Regex> = Lazy::new(|| Regex::new(&format!(r"(?<![\d.])\d+kV{NAME}站")).unwrap());
static LINE: Lazy<Regex> = Lazy::new(|| Regex::new(&format!(r"(?<![\d.])(\d+(?:\.\d+)?kV)({NAME}线)")).unwrap());
static POLE: Lazy<Regex> = Lazy::new(|| Regex::new(r"(\d+(?:\+\d+)?)号(?:杆|塔)|#(\d+(?:\+\d+)?)(?:杆)?").unwrap());
static NUMBER: Lazy<Regex> = Lazy::new(|| Regex::new(r"\d+(?=接地刀闸|开关|刀闸|令克)").unwrap());
static LINE_NUMBER: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?<=\D)\d{3}(?=线)").unwrap());
static PLACEHOLDER: Lazy<Regex> = Lazy::new(|| Regex::new(r"\{[^{}]*\}").unwrap());

/// 把所有匹配替换掉。`replacement` 拿到各分组（0 是整体）。
fn replace_all(pattern: &Regex, text: &str, replacement: impl Fn(&[Option<&str>]) -> String) -> String {
    let mut out = String::with_capacity(text.len());
    let mut last = 0;
    for caps in pattern.captures_iter(text) {
        let caps = caps.expect("正则匹配过程失败");
        let whole = caps.get(0).expect("分组 0 一定存在");
        let groups: Vec<Option<&str>> = (0..caps.len()).map(|i| caps.get(i).map(|m| m.as_str())).collect();
        out.push_str(&text[last..whole.start()]);
        out.push_str(&replacement(&groups));
        last = whole.end();
    }
    out.push_str(&text[last..]);
    out
}

/// 读取同义词表。缺字段时直接报错，不静默降级。
pub fn load_lexicon(path: &Path) -> Result<Lexicon, DataError> {
    let text =
        std::fs::read_to_string(path).map_err(|err| DataError(format!("{}：读取失败（{err}）", path.display())))?;
    let value: Value =
        serde_json::from_str(&text).map_err(|err| DataError(format!("{}：JSON 解析失败（{err}）", path.display())))?;
    for field in ["synonyms", "keywords"] {
        if value.get(field).is_none() {
            return Err(DataError(format!("{}：缺少字段 {field}", path.display())));
        }
    }
    let mut lexicon: Lexicon =
        serde_json::from_value(value).map_err(|err| DataError(format!("{}：字段类型不对（{err}）", path.display())))?;
    let mut variants: Vec<(String, String)> = Vec::new();
    for (canonical, others) in &lexicon.synonyms {
        let list = others
            .as_array()
            .ok_or_else(|| DataError(format!("{}：synonyms.{canonical} 必须是数组", path.display())))?;
        for variant in list {
            let variant = variant
                .as_str()
                .ok_or_else(|| DataError(format!("{}：synonyms.{canonical} 里只能放字符串", path.display())))?;
            variants.push((variant.to_string(), canonical.clone()));
        }
    }
    // 与参考实现一致：按变体的字符数降序，长度相同的保持原顺序（稳定排序）。
    variants.sort_by_key(|(variant, _)| std::cmp::Reverse(variant.chars().count()));
    lexicon.variants = variants;
    lexicon.normalized_keywords = lexicon.keywords.iter().map(|word| normalize(word, &lexicon)).collect();
    Ok(lexicon)
}

/// 归一化：去空白、统一标点、按同义词表替换成规范写法。
pub fn normalize(text: &str, lexicon: &Lexicon) -> String {
    // Python 的 `\s` 匹配 Unicode 空白（含全角空格 U+3000、不换行空格 U+00A0），
    // `char::is_whitespace` 的范围与它一致。
    let mut out: String = text
        .chars()
        .filter(|ch| !ch.is_whitespace())
        .map(|ch| match ch {
            '！' => '!',
            '，' => ',',
            other => other,
        })
        .collect();
    for (variant, canonical) in &lexicon.variants {
        if out.contains(variant.as_str()) {
            out = out.replace(variant.as_str(), canonical);
        }
    }
    out
}

/// 判断一个“xx线”是不是线路名（排除进线、母线、开闭所等）。
fn is_line(name: &str) -> bool {
    const SUFFIX: [&str; 9] = ["进线", "出线", "引线", "引流线", "接线", "母线", "导线", "地线", "跳线"];
    const INNER: [&str; 6] = ["开闭所", "公变", "专变", "开关", "刀闸", "站"];
    !SUFFIX.iter().any(|tail| name.ends_with(tail)) && !INNER.iter().any(|word| name.contains(word))
}

/// 抽线路名（去重，按首次出现的顺序）。返回“电压+线路名”，去掉线名里紧挨“线”的 3 位编号。
pub fn lines(text: &str) -> Vec<String> {
    let cleaned = replace_all(&PARENS, text, |_| String::new());
    let mut found: Vec<String> = Vec::new();
    for caps in LINE.captures_iter(&cleaned) {
        let caps = caps.expect("正则匹配过程失败");
        let voltage = caps.get(1).map(|m| m.as_str()).unwrap_or("");
        let name = caps.get(2).map(|m| m.as_str()).unwrap_or("");
        if !is_line(name) {
            continue;
        }
        let candidate = format!("{voltage}{}", replace_all(&LINE_NUMBER, name, |_| String::new()));
        if !found.contains(&candidate) {
            found.push(candidate);
        }
    }
    found
}

/// 抽站名（含电压等级），去重。
pub fn stations(text: &str) -> Vec<String> {
    let cleaned = replace_all(&PARENS, text, |_| String::new());
    let mut found: Vec<String> = Vec::new();
    for found_match in STATION.find_iter(&cleaned) {
        let name = found_match.expect("正则匹配过程失败").as_str().to_string();
        if !found.contains(&name) {
            found.push(name);
        }
    }
    found
}

/// 抽杆号，按出现顺序返回。
pub fn poles(text: &str) -> Vec<String> {
    POLE.captures_iter(text)
        .map(|caps| {
            let caps = caps.expect("正则匹配过程失败");
            caps.get(1).or_else(|| caps.get(2)).map(|m| m.as_str().to_string()).unwrap_or_default()
        })
        .collect()
}

/// 占位符化：站名、线路、杆号、设备编号换成 `{站}` `{线路}` `{杆号}` `{编号}`。
pub fn abstract_text(text: &str) -> String {
    let text = replace_all(&STATION, text, |_| "{站}".to_string());
    let text = replace_all(&LINE, &text, |groups| {
        if is_line(groups[2].unwrap_or("")) {
            "{线路}".to_string()
        } else {
            groups[0].unwrap_or("").to_string()
        }
    });
    let text = replace_all(&POLE, &text, |_| "{杆号}".to_string());
    replace_all(&NUMBER, &text, |_| "{编号}".to_string())
}

/// 去掉 `{…}` 占位符。场景匹配算二元字时用。
pub fn strip_placeholders(text: &str) -> String {
    replace_all(&PLACEHOLDER, text, |_| String::new())
}

/// 措施签名：措施里出现的关键词，按词表顺序。
pub fn keywords(text: &str, lexicon: &Lexicon) -> Vec<String> {
    let normalized = normalize(text, lexicon);
    lexicon
        .keywords
        .iter()
        .zip(&lexicon.normalized_keywords)
        .filter(|(_, word)| normalized.contains(word.as_str()))
        .map(|(original, _)| original.clone())
        .collect()
}
