//! 合成集评测。移植自参考实现 `src/ticketguard/evaluate.py`。
//!
//! 统计口径必须与参考实现一致：同一条发现只抵消一次「原票本来就有的发现」，
//! 其余才计入意外发现。

use crate::entities::Lexicon;
use crate::review::review_text;
use crate::rules::Rule;
use crate::schema::Finding;
use crate::templates::Template;
use serde_json::{json, Map, Value};
use std::collections::HashMap;

/// Wilson 95% 置信区间。
pub fn wilson(k: usize, n: usize) -> (f64, f64) {
    let z = 1.96_f64;
    if n == 0 {
        return (0.0, 0.0);
    }
    let p = k as f64 / n as f64;
    let d = 1.0 + z * z / n as f64;
    let c = (p + z * z / (2.0 * n as f64)) / d;
    let h = z * ((p * (1.0 - p) / n as f64) + z * z / (4.0 * n as f64 * n as f64)).sqrt() / d;
    (round4(c - h), round4(c + h))
}

fn round4(value: f64) -> f64 {
    format!("{value:.4}").parse().expect("四位小数字符串一定能解析回浮点数")
}

/// 发现去重的键：带场景的按 (规则, 证据)，其余只按规则。
fn key_of(finding: &Finding) -> (String, String) {
    match &finding.scenario_id {
        Some(_) => (finding.rule_id.clone(), finding.evidence.clone()),
        None => (finding.rule_id.clone(), String::new()),
    }
}

/// 一条发现是否命中期望。`category` 在消息里以“「类别」栏”形式出现。
fn hits(finding: &Finding, expected: &Value) -> bool {
    if Some(finding.rule_id.as_str()) != expected["rule_id"].as_str() {
        return false;
    }
    match expected.get("category").and_then(Value::as_str) {
        Some(category) => finding.message.contains(&format!("“{category}”栏")),
        None => true,
    }
}

struct Stats {
    samples: usize,
    top1: usize,
    expected: usize,
    detected: usize,
    unexpected: usize,
}

impl Stats {
    fn new() -> Self {
        Stats { samples: 0, top1: 0, expected: 0, detected: 0, unexpected: 0 }
    }

    fn to_json(&self) -> Value {
        let mut map = Map::new();
        map.insert("samples".to_string(), json!(self.samples));
        map.insert("top1".to_string(), json!(self.top1));
        map.insert("expected".to_string(), json!(self.expected));
        map.insert("detected".to_string(), json!(self.detected));
        map.insert("unexpected".to_string(), json!(self.unexpected));
        map.insert(
            "recall".to_string(),
            if self.expected == 0 { Value::Null } else { json!(round4(self.detected as f64 / self.expected as f64)) },
        );
        map.insert(
            "recall_ci95".to_string(),
            if self.expected == 0 {
                Value::Null
            } else {
                let (low, high) = wilson(self.detected, self.expected);
                json!([low, high])
            },
        );
        Value::Object(map)
    }
}

/// 跑一遍评测：检出率、按算子统计、漏检与意外发现清单。
pub fn evaluate(
    samples: &[Value],
    originals: &HashMap<String, String>,
    rules: &[Rule],
    templates: &[Template],
    lexicon: &Lexicon,
) -> Result<Value, crate::Error> {
    let mut report = Stats::new();
    let mut by_operator: Vec<(String, Stats)> = Vec::new();
    let mut misses: Vec<Value> = Vec::new();
    let mut unexpected_findings: Vec<Value> = Vec::new();
    let mut original_counts: HashMap<String, HashMap<(String, String), usize>> = HashMap::new();

    for sample in samples {
        let text = sample["ticket_text"].as_str().expect("合成样本必须有 ticket_text");
        let base = sample["base_scenario"].as_str().expect("合成样本必须有 base_scenario");
        let result = review_text(text, rules, templates, lexicon)?;
        if !original_counts.contains_key(base) {
            let original = originals.get(base).ok_or_else(|| crate::Error(format!("缺少场景 {base} 的原票文本")))?;
            let mut counts: HashMap<(String, String), usize> = HashMap::new();
            for finding in review_text(original, rules, templates, lexicon)?.findings {
                *counts.entry(key_of(&finding)).or_insert(0) += 1;
            }
            original_counts.insert(base.to_string(), counts);
        }
        let mut remaining = original_counts[base].clone();
        let expected = sample["expected_findings"].as_array().cloned().unwrap_or_default();
        let mut detected = 0usize;
        let mut unexpected = 0usize;
        for expectation in &expected {
            if result.findings.iter().any(|finding| hits(finding, expectation)) {
                detected += 1;
            } else {
                misses.push(json!({ "id": sample["id"], "expected": expectation }));
            }
        }
        for finding in &result.findings {
            let key = key_of(finding);
            match remaining.get_mut(&key) {
                Some(count) if *count > 0 => *count -= 1,
                _ if expected.iter().any(|expectation| hits(finding, expectation)) => {}
                _ => {
                    unexpected += 1;
                    unexpected_findings.push(json!({
                        "id": sample["id"],
                        "rule_id": finding.rule_id,
                        "message": finding.message,
                    }));
                }
            }
        }
        let operator = sample["mutations"][0]["op"].as_str().unwrap_or("").to_string();
        let stats = match by_operator.iter_mut().find(|(name, _)| *name == operator) {
            Some((_, stats)) => stats,
            None => {
                by_operator.push((operator, Stats::new()));
                &mut by_operator.last_mut().expect("刚插入").1
            }
        };
        for target in [&mut report, stats] {
            target.samples += 1;
            target.top1 += usize::from(result.candidates[0].0 == base);
            target.expected += expected.len();
            target.detected += detected;
            target.unexpected += unexpected;
        }
    }

    let mut map = Map::new();
    map.insert("samples".to_string(), json!(report.samples));
    map.insert("top1".to_string(), json!(report.top1));
    map.insert("expected".to_string(), json!(report.expected));
    map.insert("detected".to_string(), json!(report.detected));
    map.insert(
        "recall".to_string(),
        if report.expected == 0 { Value::Null } else { json!(round4(report.detected as f64 / report.expected as f64)) },
    );
    map.insert(
        "recall_ci95".to_string(),
        if report.expected == 0 {
            Value::Null
        } else {
            let (low, high) = wilson(report.detected, report.expected);
            json!([low, high])
        },
    );
    map.insert("unexpected".to_string(), json!(report.unexpected));
    let mut operators = Map::new();
    for (name, stats) in &by_operator {
        operators.insert(name.clone(), stats.to_json());
    }
    map.insert("by_operator".to_string(), Value::Object(operators));
    map.insert("misses".to_string(), Value::Array(misses));
    map.insert("unexpected_findings".to_string(), Value::Array(unexpected_findings));
    Ok(Value::Object(map))
}
