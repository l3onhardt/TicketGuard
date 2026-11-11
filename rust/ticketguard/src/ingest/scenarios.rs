//! 把典票正文按“场景 N：”切分。移植自参考实现 `src/ticketguard/ingest/scenarios.py`。
//!
//! 切分规则（与参考实现一致）：
//! - 形如“一、…典型场景”或“一、…典型操作场景”的整行是段落标题，段落名取顿号之后的部分；
//! - 段落内，场景编号必须连续：下一个只能是“主编号+1”，或者“主编号-子编号+1”；
//! - 场景块的正文是标题行之后、下一个场景或段落标题之前的内容，不含标题行本身。

use crate::text::splitlines;
use once_cell::sync::Lazy;
use regex::Regex;

static HEADING: Lazy<Regex> = Lazy::new(|| Regex::new(r"^[一二三四五六七八九十]+、(.*典型(?:操作)?场景)$").unwrap());

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScenarioChunk {
    pub section: String,
    pub number: String,
    pub title: String,
    pub start_line: usize,
    pub text: String,
}

/// 在一行里找“场景<期望编号><分隔符>标题”。等价于参考实现的
/// `re.search(rf"场景({expected})[：:\s](.*)", line)`：从左到右，每个位置按顺序试各个候选编号。
fn find_scenario<'a>(line: &'a str, expected: &[String]) -> Option<(String, &'a str)> {
    for (start, marker) in line.match_indices("场景") {
        let rest = &line[start + marker.len()..];
        for number in expected {
            let Some(tail) = rest.strip_prefix(number.as_str()) else { continue };
            let mut chars = tail.chars();
            match chars.next() {
                Some(separator) if separator == '：' || separator == ':' || separator.is_whitespace() => {
                    return Some((number.clone(), chars.as_str()));
                }
                _ => {}
            }
        }
    }
    None
}

/// 按段落和连续的主、子编号切分典票场景。
pub fn split_scenarios(text: &str) -> Vec<ScenarioChunk> {
    let lines = splitlines(text);
    let mut chunks: Vec<ScenarioChunk> = Vec::new();
    let mut section = String::new();
    let (mut main, mut sub) = (0usize, 0usize);
    // 当前正在收集正文的场景块在 chunks 里的下标
    let mut current: Option<usize> = None;

    for (index, line) in lines.iter().enumerate() {
        if let Some(caps) = HEADING.captures(line) {
            if let Some(open) = current.take() {
                chunks[open].text = lines[chunks[open].start_line + 1..index].join("\n");
            }
            section = caps[1].to_string();
            main = 0;
            sub = 0;
            continue;
        }
        if section.is_empty() {
            continue;
        }
        let expected: Vec<String> =
            if main == 0 { vec!["1".to_string()] } else { vec![(main + 1).to_string(), format!("{main}-{}", sub + 1)] };
        if let Some((number, title)) = find_scenario(line, &expected) {
            if let Some(open) = current.take() {
                chunks[open].text = lines[chunks[open].start_line + 1..index].join("\n");
            }
            let mut parts = number.split('-');
            main = parts.next().and_then(|part| part.parse().ok()).unwrap_or(0);
            sub = parts.next().and_then(|part| part.parse().ok()).unwrap_or(0);
            chunks.push(ScenarioChunk {
                section: section.clone(),
                number,
                title: title.trim().to_string(),
                start_line: index,
                text: String::new(),
            });
            current = Some(chunks.len() - 1);
        }
    }
    if let Some(open) = current {
        chunks[open].text = lines[chunks[open].start_line + 1..].join("\n");
    }
    chunks
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "前言\n场景1：不在段落里，不算\n一、配电第一种工作票典型场景\n场景1：甲（有调整）\n正文A\n场景3：编号不连续，不算\n场景2 乙\n正文B\n二、配电倒闸典型操作场景\n场景1:丙\n场景1-1：子场景\n正文C";

    #[test]
    fn splits_by_consecutive_numbers() {
        let chunks = split_scenarios(SAMPLE);
        let seen: Vec<(&str, &str, &str)> =
            chunks.iter().map(|c| (c.section.as_str(), c.number.as_str(), c.title.as_str())).collect();
        assert_eq!(
            seen,
            vec![
                ("配电第一种工作票典型场景", "1", "甲（有调整）"),
                ("配电第一种工作票典型场景", "2", "乙"),
                ("配电倒闸典型操作场景", "1", "丙"),
                ("配电倒闸典型操作场景", "1-1", "子场景"),
            ]
        );
    }

    #[test]
    fn body_excludes_title_line() {
        let chunks = split_scenarios(SAMPLE);
        assert_eq!(chunks[0].text, "正文A\n场景3：编号不连续，不算");
        assert_eq!(chunks[1].text, "正文B");
        assert_eq!(chunks[3].text, "正文C");
    }
}
