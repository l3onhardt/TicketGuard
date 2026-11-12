//! T-014 验收测试：报告页上的反馈区。
//!
//! 报告页写不回磁盘（ADR-0007），所以反馈做成“在页面上选好 → 复制一段文字 → 交给开发者”。
//! 这段文字要能带出内网，所以里面不能有票面内容：只有检查项的类别、标题、等级和审票人的选择。
//!
//! 受保护文件：实现方不得修改。

mod common;

use common::{snapshot, ticket, Snapshot};
use serde_json::Value;
use ticketguard::report::{build_report, render_html, FEEDBACK_CHOICES, FEEDBACK_NOTE};
use ticketguard::review::{review_text, ModelUse, Review};

const VERSION: &str = env!("CARGO_PKG_VERSION");

fn review_of(name: &str, data: &Snapshot) -> Review {
    review_text(&ticket(name), &data.rules, &data.templates, &data.lexicon).expect("预审成功")
}

fn page(name: &str, data: &Snapshot) -> (Value, String) {
    let report = build_report(&review_of(name, data), &data.templates, &format!("{name}.txt"));
    let html = render_html(&report);
    (report, html)
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;").replace('\'', "&#x27;")
}

/// 与 report.rs 的 json_ascii 一致：非 ASCII 写成 \uXXXX（这里的文字都在基本平面内）。
fn ascii(text: &str) -> String {
    let mut out = String::from("\"");
    for ch in text.chars() {
        match ch {
            '\n' => out.push_str("\\n"),
            ch if (ch as u32) < 0x7f => out.push(ch),
            ch => out.push_str(&format!("\\u{:04x}", ch as u32)),
        }
    }
    out + "\""
}

#[test]
fn report_has_a_feedback_head_without_ticket_content() {
    let data = snapshot();
    assert_eq!(FEEDBACK_CHOICES, ["未评价", "属实", "误报", "说不清"]);
    assert_eq!(
        FEEDBACK_NOTE,
        "反馈里只有每条事项的类别和你的选择，不带票面内容。补充说明里请不要写真实的人名、线路名、设备名和票号。"
    );

    let (report, _) = page("d1_a_drop_ground", &data);
    assert_eq!(report.as_object().unwrap().keys().last().unwrap(), "feedback", "feedback 是最后一个键");
    let similarity = report["scenario"]["similarity"].as_str().unwrap();
    assert_eq!(
        report["feedback"],
        format!(
            "票审反馈（票审 {VERSION}，规则模式）\n结论：发现 1 处疑点，请逐条确认\n对照：典票场景 1「单台公变更换」（相似度：{similarity}）"
        )
    );

    let (unrelated, _) = page("d1_b_unrelated", &data);
    assert_eq!(
        unrelated["feedback"],
        format!("票审反馈（票审 {VERSION}，规则模式）\n结论：没有找到对应的典票场景，安全措施没有做比对\n对照：没有找到对应的典票场景")
    );

    // 模式照实写
    let mut done = review_of("d1_b", &data);
    done.model = ModelUse::Done { asked: 0 };
    let done = build_report(&done, &data.templates, "b.txt");
    assert!(done["feedback"].as_str().unwrap().starts_with(&format!("票审反馈（票审 {VERSION}，模型模式）\n")));
    let mut failed = review_of("d1_b", &data);
    failed.model = ModelUse::Failed("找不到模型文件".to_string());
    let failed = build_report(&failed, &data.templates, "b.txt");
    assert!(failed["feedback"].as_str().unwrap().starts_with(&format!("票审反馈（票审 {VERSION}，模型没有用上）\n")));
    assert!(!failed["feedback"].as_str().unwrap().contains("找不到模型文件"), "原因里可能有路径，不进反馈");

    // 抬头里没有票面内容
    for name in ["d1_a", "d1_a_crew_off", "d1_a_drop_ground"] {
        let (report, _) = page(name, &data);
        let head = report["feedback"].as_str().unwrap();
        for word in ["2099-01-P1-001", "张三", "测试线", "甲公变", "d1_a"] {
            assert!(!head.contains(word), "{name}：反馈抬头里不应有“{word}”");
        }
    }
}

#[test]
fn every_finding_gets_a_choice() {
    let data = snapshot();
    for name in ["d1_a", "d1_b", "d1_a_crew_off", "d1_a_drop_ground", "d1_a_model", "d1_b_unrelated"] {
        let (report, html) = page(name, &data);
        let findings = report["findings"].as_array().unwrap();
        assert_eq!(html.matches("<select class=\"fb\" ").count(), findings.len(), "{name}：每条发现一个下拉框");
        assert_eq!(html.matches("这一条报得对不对（反馈用）：").count(), findings.len(), "{name}");
        for finding in findings {
            let item = format!(
                "{}｜{}｜{}",
                finding["group"].as_str().unwrap(),
                finding["title"].as_str().unwrap(),
                finding["label"].as_str().unwrap()
            );
            assert!(
                html.contains(&format!(
                    "<select class=\"fb\" data-item=\"{}\" onchange=\"updateFeedback()\"><option>未评价</option><option>属实</option><option>误报</option><option>说不清</option></select>",
                    escape(&item)
                )),
                "{name}：下拉框带着“类别｜标题｜等级”，默认是“未评价”"
            );
        }
        // 对照的场景：固定一个
        assert_eq!(
            html.matches("<select class=\"fb-scene\" data-item=\"对照的典票场景对不对：\" onchange=\"updateFeedback()\"><option>未评价</option><option>对</option><option>不对</option><option>说不清</option></select>").count(),
            1,
            "{name}"
        );
    }
}

#[test]
fn feedback_section_sits_before_the_limits() {
    let data = snapshot();
    let (report, html) = page("d1_a_drop_ground", &data);
    let at = |id: &str| html.find(&format!("id=\"{id}\"")).unwrap_or_else(|| panic!("页面缺少 {id}"));
    assert!(at("scenario") < at("feedback-section") && at("feedback-section") < at("limits"));
    assert!(html.contains("<section id=\"feedback-section\"><h2>给开发者的反馈</h2>"));
    assert!(html.contains(&escape(FEEDBACK_NOTE)));
    assert!(html.contains("<textarea id=\"note\""), "补充说明可以填");
    assert!(html.contains("<textarea id=\"feedback\"") && html.contains("readonly"), "反馈内容只读，由脚本拼出来");
    assert!(html.contains("onclick=\"copyFeedback()\">复制反馈</button>"));

    let script = &html[html.find("<script>").unwrap()..html.find("</script>").unwrap()];
    assert!(script.contains("function updateFeedback") && script.contains("function copyFeedback"));
    assert!(script.contains(&ascii(report["feedback"].as_str().unwrap())), "反馈抬头用 json_ascii 写进脚本");
    assert!(script.contains(&ascii("补充：")));
    assert!(script.trim_end().ends_with("updateFeedback();"), "打开页面就拼好一份");
    for word in ["`", "=>", "let ", "const ", ".forEach("] {
        assert!(!script.contains(word), "脚本只能用 ES5，不应包含 {word}");
    }
    // 反馈不进打印件
    let style = &html[html.find("<style>").unwrap()..html.find("</style>").unwrap()];
    let print = &style[style.find("@media print").unwrap()..];
    assert!(print.contains("#feedback-section") && print.contains(".fb-line"), "打印时隐藏反馈区和每条下面的下拉框");
}

#[test]
fn feedback_text_carries_nothing_from_the_ticket() {
    // 写进反馈的只有 data-item 和抬头。把票面里的名字换成显眼的记号，确认它们进不去。
    let data = snapshot();
    let text = ticket("d1_a_model").replace("测试线", "机密线").replace("张三", "机密人");
    let review = review_text(&text, &data.rules, &data.templates, &data.lexicon).unwrap();
    let report = build_report(&review, &data.templates, "机密文件.txt");
    let html = render_html(&report);
    assert!(html.contains("机密线"), "票面内容照常显示在页面上");
    assert!(!report["feedback"].as_str().unwrap().contains("机密"));
    for piece in html.split("data-item=\"").skip(1) {
        let item = &piece[..piece.find('"').unwrap()];
        assert!(!item.contains("机密"), "写进反馈的条目里不应有票面内容：{item}");
    }
}
