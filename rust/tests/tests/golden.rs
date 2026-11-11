//! 基准测试：解析、实体、场景切分、置信区间、评测这些引擎输出，必须与基准结果一致。
//!
//! 基准结果在 `rust/tests/fixtures/golden.json`，由 `archive/tools/parity/make_fixtures.py`
//! 从最初的 Python 实现导出。报告的版式和措辞不在这里比对——那是产品文案，改动频繁，
//! 由 `t011_report.rs` 做结构化断言。

mod common;

use common::{fixtures, json, read, snapshot, ticket, ticket_names};
use serde_json::Value;
use std::collections::HashMap;
use ticketguard::entities::{abstract_text, keywords, lines, normalize, poles, stations};
use ticketguard::evaluate::{evaluate, wilson};
use ticketguard::ingest::{parse_dist1, split_scenarios};

fn sorted(mut items: Vec<String>) -> Vec<String> {
    items.sort();
    items
}

#[test]
fn parse_matches_golden() {
    let golden = json("golden.json");
    let names = ticket_names(&golden);
    assert_eq!(names.len(), 12, "基准里应有 12 张编造票面");
    for name in names {
        let parsed = parse_dist1(&ticket(&name)).unwrap_or_else(|err| panic!("{name} 解析失败：{err}"));
        let actual = serde_json::to_value(&parsed).expect("Ticket 可以序列化");
        assert_eq!(actual, golden["parse"][&name], "{name} 的解析结果与基准不一致");
    }
}

#[test]
fn entities_match_golden() {
    let golden = json("golden.json");
    let lexicon = snapshot().lexicon;
    let cases = golden["entities"].as_array().expect("golden.entities 是数组");
    assert!(cases.len() >= 9);
    for case in cases {
        let text = case["text"].as_str().unwrap();
        let strings = |key: &str| -> Vec<String> {
            case[key].as_array().unwrap().iter().map(|v| v.as_str().unwrap().to_string()).collect()
        };
        assert_eq!(normalize(text, &lexicon), case["normalize"].as_str().unwrap(), "normalize：{text}");
        assert_eq!(sorted(lines(text)), strings("lines"), "lines：{text}");
        assert_eq!(sorted(stations(text)), strings("stations"), "stations：{text}");
        assert_eq!(poles(text), strings("poles"), "poles：{text}");
        assert_eq!(abstract_text(text), case["abstract"].as_str().unwrap(), "abstract：{text}");
        assert_eq!(keywords(text, &lexicon), strings("keywords"), "keywords：{text}");
    }
}

#[test]
fn scenarios_match_golden() {
    let golden = json("golden.json");
    let chunks = split_scenarios(&read("scenarios.txt"));
    let actual: Vec<Value> = chunks
        .iter()
        .map(|chunk| {
            serde_json::json!({
                "section": chunk.section,
                "number": chunk.number,
                "title": chunk.title,
                "start_line": chunk.start_line,
                "text": chunk.text,
            })
        })
        .collect();
    assert_eq!(Value::Array(actual), golden["scenarios"]);
}

#[test]
fn wilson_matches_golden() {
    let golden = json("golden.json");
    for case in golden["wilson"].as_array().unwrap() {
        let (k, n) = (case["k"].as_u64().unwrap() as usize, case["n"].as_u64().unwrap() as usize);
        let (low, high) = wilson(k, n);
        assert_eq!(serde_json::json!([low, high]), case["ci"], "wilson({k}, {n})");
    }
}

#[test]
fn evaluate_matches_golden() {
    let golden = json("golden.json");
    let data = snapshot();
    let samples: Vec<Value> = read("eval_samples.jsonl")
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str(line).expect("样本是合法 JSON"))
        .collect();
    let originals: HashMap<String, String> = HashMap::from([("D1-S01".to_string(), ticket("d1_a"))]);
    let report = evaluate(&samples, &originals, &data.rules, &data.templates, &data.lexicon).expect("评测成功");
    assert_eq!(
        serde_json::to_string_pretty(&report).unwrap(),
        serde_json::to_string_pretty(&golden["eval"]).unwrap(),
        "评测报告与基准不一致"
    );
    // 基准里特意放了一条检不出的期望，确认“漏检”分支真的被走到
    assert_eq!(report["misses"].as_array().unwrap().len(), 1);
}

#[test]
fn fixtures_are_fabricated() {
    // 红线 1 / AGENTS §5：入库的票面只能是编造的。这里卡住最容易出事的两类：
    // 真实样式的手机号（典票里的示例号 13800000000 除外）和典票库前言里才有的字样。
    for entry in std::fs::read_dir(fixtures().join("tickets")).unwrap() {
        let path = entry.unwrap().path();
        let text = std::fs::read_to_string(&path).unwrap();
        for word in ["编审", "编写人员", "审核人员"] {
            assert!(!text.contains(word), "{} 含有“{word}”", path.display());
        }
        let digits: Vec<char> = text.chars().collect();
        for window in digits.windows(11) {
            let candidate: String = window.iter().collect();
            if candidate.chars().all(|ch| ch.is_ascii_digit()) && candidate.starts_with('1') {
                assert!(candidate.ends_with("00000000"), "{} 里有疑似真实手机号：{candidate}", path.display());
            }
        }
    }
}
