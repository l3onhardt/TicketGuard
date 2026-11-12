//! T-013 验收测试：报告怎么交代“模型复核”（ADR-0008）。
//!
//! 启动模型程序、连本机端口的部分（`rust/tg/src/model.rs`）没法在这里测，由规划者用真模型端到端验收。
//!
//! 受保护文件：实现方不得修改。

mod common;

use common::{fixtures, snapshot, ticket, Snapshot};
use serde_json::{json, Value};
use ticketguard::harness::{load_harness, second_opinion};
use ticketguard::report::{build_report, render_html, MODEL_OFF, MODEL_ROW};
use ticketguard::review::{review_text, ModelUse, Review};
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

/// d1_a_model 过一遍模型复核：标志牌那条被认成“写法不同”，应投切那条不是。
fn reviewed_by_model(data: &Snapshot, word: &'static str) -> Review {
    let harness = load_harness(&fixtures().join("harness.json")).unwrap();
    let mut review = review_of("d1_a_model", data);
    second_opinion(&mut review, &data.templates, &harness, &mut fake(word)).unwrap();
    review
}

fn row<'a>(report: &'a Value, name: &str) -> &'a Value {
    report["checklist"].as_array().unwrap().iter().find(|row| row["name"] == name).expect("清单里有这一行")
}

fn finding<'a>(report: &'a Value, rule_id: &str) -> &'a Value {
    report["findings"].as_array().unwrap().iter().find(|finding| finding["rule_id"] == rule_id).expect("有这条发现")
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;").replace('\'', "&#x27;")
}

#[test]
fn checklist_always_says_whether_the_model_was_used() {
    let data = snapshot();
    assert_eq!(MODEL_ROW, "模型复核（措施是不是只是写法不同）");
    assert_eq!(MODEL_OFF, "这次是规则模式，没有请模型复核。措施换了说法可能被报成缺少。");

    // 规则模式
    let rule = build_report(&review_of("d1_a_model", &data), &data.templates, "a.txt");
    let names: Vec<&str> =
        rule["checklist"].as_array().unwrap().iter().map(|row| row["name"].as_str().unwrap()).collect();
    assert_eq!(&names[names.len() - 3..], ["安全措施（对照典票）", MODEL_ROW, "机器查不了的项目"], "排在措施比对后面");
    assert_eq!(row(&rule, MODEL_ROW), &json!({"name": MODEL_ROW, "status": "skipped", "detail": MODEL_OFF}));

    // 想用没用成：照实写原因
    let mut failed = review_of("d1_a_model", &data);
    failed.model = ModelUse::Failed("找不到模型文件 models\\x.gguf".to_string());
    let failed = build_report(&failed, &data.templates, "a.txt");
    assert_eq!(row(&failed, MODEL_ROW)["status"], "skipped");
    assert_eq!(
        row(&failed, MODEL_ROW)["detail"],
        "模型复核没有做成：找不到模型文件 models\\x.gguf。这份报告只用了规则，措施换了说法可能被报成缺少。"
    );

    // 复核了，有改成提示的
    let done = build_report(&reviewed_by_model(&data, "挂上"), &data.templates, "a.txt");
    assert_eq!(row(&done, MODEL_ROW)["status"], "found");
    assert_eq!(
        row(&done, MODEL_ROW)["detail"],
        "模型复核了 2 条疑似缺少的措施，认为其中 1 条只是写法不同，已改成提示，见下方"
    );

    // 复核了，没有改的
    let none = build_report(&reviewed_by_model(&data, "没有这个词"), &data.templates, "a.txt");
    assert_eq!(row(&none, MODEL_ROW)["status"], "checked");
    assert_eq!(row(&none, MODEL_ROW)["detail"], "模型复核了 2 条疑似缺少的措施，没有发现只是写法不同的");

    // 用了模型，但没有可复核的
    let mut clean = review_of("d1_b", &data);
    clean.model = ModelUse::Done { asked: 0 };
    let clean = build_report(&clean, &data.templates, "b.txt");
    assert_eq!(
        row(&clean, MODEL_ROW),
        &json!({"name": MODEL_ROW, "status": "checked", "detail": "没有需要模型复核的事项"})
    );
}

#[test]
fn measures_row_counts_rewording_separately() {
    let data = snapshot();
    let label = "典票场景 1「单台公变更换」";
    let name = "安全措施（对照典票）";
    let mixed = build_report(&reviewed_by_model(&data, "挂上"), &data.templates, "a.txt");
    assert_eq!(row(&mixed, name)["status"], "found");
    assert_eq!(row(&mixed, name)["detail"], format!("对照{label}，有 1 类措施没有找到，见下方"));

    // 全都只是写法不同：还是“需要确认”，但不说“没有找到”
    let harness = load_harness(&fixtures().join("harness.json")).unwrap();
    let mut review = review_of("d1_a_model", &data);
    second_opinion(&mut review, &data.templates, &harness, &mut fake("测试")).unwrap();
    let all = build_report(&review, &data.templates, "a.txt");
    assert_eq!(row(&all, name)["status"], "found");
    assert_eq!(row(&all, name)["detail"], format!("对照{label}，有 2 类措施的写法和典票不同，见下方"));
    assert_eq!(all["verdict"]["level"], "notes", "没有疑点了，只剩提示");
}

#[test]
fn finding_carries_the_models_opinion() {
    let data = snapshot();
    let rule = build_report(&review_of("d1_a_model", &data), &data.templates, "a.txt");
    for finding in rule["findings"].as_array().unwrap() {
        assert_eq!(finding.as_object().unwrap().keys().last().unwrap(), "second", "second 是最后一个键");
        assert!(finding["second"].is_null(), "规则模式下没有模型意见");
    }

    let report = build_report(&reviewed_by_model(&data, "挂上"), &data.templates, "a.txt");
    let reworded = finding(&report, "CMP-REWORDED");
    assert_eq!(reworded["group"], "安全措施（对照典票）");
    assert_eq!(reworded["label"], "提示");
    assert_eq!(reworded["picked"], false, "提示默认不进打回草稿");
    let measure = "在10kV测试线测试01开关所在杆塔下方明显可视位置挂上“禁止合闸，线路有人工作！”标志牌";
    let second = &reworded["second"];
    assert_eq!(second.as_object().unwrap().keys().collect::<Vec<_>>(), ["measure", "same", "p", "says"]);
    assert_eq!(second["measure"], measure);
    assert_eq!(second["same"], true);
    assert!((second["p"].as_f64().unwrap() - 0.9).abs() < 1e-9);
    assert_eq!(second["says"], format!("票面上的“{measure}”和典票这一条是同一类措施，只是写法不同。"));

    let gap = finding(&report, "CMP-MISSING");
    assert_eq!(gap["picked"], true);
    assert_eq!(gap["second"]["same"], false);
    assert_eq!(
        gap["second"]["says"],
        "这一栏里最接近的是“断开10kV测试线测试01开关操作电源空开”，但它和典票这一条不是同一类措施。"
    );
    assert!(!report["draft"].as_str().unwrap().contains("写法不同"), "草稿里只有疑点");
}

#[test]
fn page_shows_the_opinion_in_plain_words() {
    let data = snapshot();
    let report = build_report(&reviewed_by_model(&data, "挂上"), &data.templates, "a.txt");
    let html = render_html(&report);
    assert_eq!(html.matches("<p class=\"second\">模型的看法：").count(), 2, "问过的两条各一行");
    for rule_id in ["CMP-REWORDED", "CMP-MISSING"] {
        let says = finding(&report, rule_id)["second"]["says"].as_str().unwrap();
        assert!(html.contains(&format!("<p class=\"second\">模型的看法：{}</p>", escape(says))), "{rule_id}");
    }
    // 位置：在“典票上的写法”后面、依据前面
    let card = &html[html.find("写法和典票不同").unwrap()..];
    let (typical, second, basis) =
        (card.find("典票上的写法：").unwrap(), card.find("模型的看法：").unwrap(), card.find("依据：").unwrap());
    assert!(typical < second && second < basis);
    assert!(html.contains(&escape(MODEL_ROW)));
    for word in ["CMP-REWORDED", "0.9", "0.1", "second\":", "says", "threshold", "logprob"] {
        assert!(!html.contains(word), "页面上不应出现“{word}”");
    }
    for word in ["违章", "不合格", "必须打回", "审核通过", "没有问题", "无问题"] {
        assert!(!html.contains(word), "页面上不应出现“{word}”");
    }

    // 规则模式的页面：没有“模型的看法”，但清单里说明了没用模型
    let rule = render_html(&build_report(&review_of("d1_a_model", &data), &data.templates, "a.txt"));
    assert!(!rule.contains("模型的看法"));
    assert!(rule.contains(&escape(MODEL_OFF)));
    assert_eq!(rule.matches("<tr class=\"check-skipped\">").count(), 2, "模型复核、机器查不了的项目");
}

#[test]
fn text_from_the_ticket_is_escaped_in_the_opinion() {
    let data = snapshot();
    let harness = load_harness(&fixtures().join("harness.json")).unwrap();
    let text = ticket("d1_a_model").replace("可视位置挂上", "可视位置<b>挂上</b>");
    let mut review = review_text(&text, &data.rules, &data.templates, &data.lexicon).unwrap();
    second_opinion(&mut review, &data.templates, &harness, &mut fake("挂上")).unwrap();
    let html = render_html(&build_report(&review, &data.templates, "a.txt"));
    assert!(html.contains("&lt;b&gt;挂上&lt;/b&gt;") && !html.contains("<b>挂上</b>"));
}
