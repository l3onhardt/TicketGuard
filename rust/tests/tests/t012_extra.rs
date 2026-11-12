mod common;

use ticketguard::harness::{load_harness, second_opinion};
use ticketguard::review::{review_text, ModelUse};

#[test]
fn equal_probabilities_keep_the_first_measure() {
    let data = common::snapshot();
    let text = common::ticket("d1_a").replace("2、10kV测试线测试01开关2刀闸。", "2、10kV测试线测试01开关2隔离刀。");
    let mut review = review_text(&text, &data.rules, &data.templates, &data.lexicon).unwrap();
    let first = review.ticket.measures[0].text.clone();
    let harness = load_harness(&common::fixtures().join("harness.json")).unwrap();
    let response = serde_json::json!({"choices": [{"logprobs": {"content": [{"top_logprobs": [
        {"token": "是", "logprob": 0.9f64.ln()}, {"token": "否", "logprob": 0.1f64.ln()}
    ]}]}}]})
    .to_string();
    second_opinion(&mut review, &data.templates, &harness, &mut |_| Ok(response.clone())).unwrap();
    assert_eq!(review.model, ModelUse::Done { asked: 2 });
    let second = review.findings.iter().find_map(|finding| finding.second.as_ref()).unwrap();
    assert_eq!(second.measure, first);
    assert!(second.same);
}
