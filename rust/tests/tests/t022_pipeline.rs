//! T-022 验收测试：主界面要显示的几个数（ADR-0010）。
//!
//! 页面靠这几个数说清楚流水线每一步做了什么：规则发现了几处疑点、其中几处要请模型复核、
//! 模型把几处降成了提示。数错了，页面上那句话就是错的。
//!
//! 受保护文件：实现方不得修改。

mod common;

use common::{fixtures, snapshot, ticket, Snapshot};
use serde_json::{json, Value};
use ticketguard::harness::{load_harness, second_opinion};
use ticketguard::report::build_report;
use ticketguard::review::{review_text, Review};
use ticketguard::ui::summary;
use ticketguard::Error;

fn review_of(name: &str, data: &Snapshot) -> Review {
    review_text(&ticket(name), &data.rules, &data.templates, &data.lexicon).expect("预审成功")
}

/// 假模型：本题“票面”一行含 `word` 就给 0.9，否则给 0.1。
fn fake(word: &'static str) -> impl FnMut(&str) -> Result<String, Error> {
    move |body: &str| {
        let value: Value = serde_json::from_str(body).unwrap();
        let question = value["messages"].as_array().unwrap().last().unwrap()["content"].as_str().unwrap().to_string();
        let said = question.lines().find(|line| line.starts_with("票面：")).unwrap();
        let p: f64 = if said.contains(word) { 0.9 } else { 0.1 };
        Ok(json!({"choices": [{"logprobs": {"content": [{"top_logprobs": [
            {"token": "是", "logprob": p.ln()}, {"token": "否", "logprob": (1.0 - p).ln()},
        ]}]}}]})
        .to_string())
    }
}

fn after_model(data: &Snapshot, word: &'static str) -> Value {
    let harness = load_harness(&fixtures().join("harness.json")).unwrap();
    let mut review = review_of("d1_a_model", data);
    second_opinion(&mut review, &data.templates, &harness, &mut fake(word)).unwrap();
    summary(&build_report(&review, &data.templates, "a.txt"))
}

#[test]
fn summary_has_seven_keys_in_order() {
    let data = snapshot();
    let report = build_report(&review_of("d1_a_model", &data), &data.templates, "a.txt");
    let got = summary(&report);
    let keys: Vec<&String> = got.as_object().unwrap().keys().collect();
    assert_eq!(keys, ["number", "level", "headline", "issues", "notes", "pending", "reworded"]);
    assert_eq!(got["number"], report["ticket"]["number"]);
    assert_eq!(got["level"], report["verdict"]["level"]);
    assert_eq!(got["headline"], report["verdict"]["headline"]);
}

#[test]
fn counts_follow_the_pipeline() {
    let data = snapshot();
    // 规则这一步：两条“可能缺少措施”，都还没有人复核
    let rule = summary(&build_report(&review_of("d1_a_model", &data), &data.templates, "a.txt"));
    assert_eq!((&rule["pending"], &rule["reworded"]), (&json!(2), &json!(0)));
    // 模型认出一条只是写法不同：那一条从疑点变成提示，另一条还是“可能缺少”
    let one = after_model(&data, "挂上");
    assert_eq!((&one["pending"], &one["reworded"]), (&json!(1), &json!(1)));
    assert_eq!(one["issues"].as_u64().unwrap() + 1, rule["issues"].as_u64().unwrap(), "疑点少了一处");
    assert_eq!(one["notes"].as_u64().unwrap(), rule["notes"].as_u64().unwrap() + 1, "提示多了一条");
    // 模型一条都没认出来：和规则那一步一样
    let none = after_model(&data, "没有这个词");
    assert_eq!(none, rule);
    // 每一步“疑点 + 提示”的总数不变：模型不增不删
    for step in [&rule, &one] {
        let total = step["issues"].as_u64().unwrap() + step["notes"].as_u64().unwrap();
        assert_eq!(total, rule["issues"].as_u64().unwrap() + rule["notes"].as_u64().unwrap());
    }
}

#[test]
fn counts_match_the_report_for_every_fixture() {
    let data = snapshot();
    for name in ["d1_a", "d1_a_crew_off", "d1_a_drop_ground", "d1_a_paraphrase", "d1_b", "d1_b_unrelated"] {
        let report = build_report(&review_of(name, &data), &data.templates, "a.txt");
        let findings = report["findings"].as_array().unwrap();
        let count = |test: &dyn Fn(&Value) -> bool| json!(findings.iter().filter(|finding| test(finding)).count());
        let got = summary(&report);
        assert_eq!(got["issues"], count(&|finding| finding["severity"] != "info"), "{name}");
        assert_eq!(got["notes"], count(&|finding| finding["severity"] == "info"), "{name}");
        assert_eq!(got["pending"], count(&|finding| finding["rule_id"] == "CMP-MISSING"), "{name}");
        assert_eq!(got["reworded"], json!(0), "{name}：规则这一步不会有模型降级的事项");
        assert_eq!(got["issues"].as_u64().unwrap() + got["notes"].as_u64().unwrap(), findings.len() as u64);
    }
    // 照典票写的票：没有要请模型复核的事项，主界面据此不启动模型
    let clean = summary(&build_report(&review_of("d1_a", &data), &data.templates, "a.txt"));
    assert_eq!(clean["pending"], json!(0));
}

#[test]
fn number_falls_back_when_missing() {
    let mut report = json!({"ticket": {"number": ""}, "verdict": {"level": "clear", "headline": "h"}, "findings": []});
    assert_eq!(summary(&report)["number"], "未填写");
    report["ticket"]["number"] = Value::Null;
    assert_eq!(summary(&report)["number"], "未填写");
    report["ticket"]["number"] = json!("2099-01-P1-001");
    assert_eq!(
        summary(&report),
        json!({"number": "2099-01-P1-001", "level": "clear", "headline": "h", "issues": 0, "notes": 0, "pending": 0, "reworded": 0})
    );
}
