//! T-011 验收测试（规划者编写，实现方不得修改）。
//!
//! 报告是给审票人看的产品界面。这里不逐字节比对页面，而是检查三件事：
//! 1. 报告模型说清楚了“机器查了什么、查出了什么、没查什么”；
//! 2. 页面上没有内部标识，没有越界的说法，离线、IE11 能用，所有外来文字都转义；
//! 3. 打回意见草稿和勾选框对得上。
//!
//! 设计依据见 docs/decisions/0007-前端形态与报告版式.md，规格见 docs/tasks/T-011-报告重做.md。

mod common;

use common::{repo_root, snapshot, ticket, Snapshot};
use serde_json::Value;
use ticketguard::entities::load_lexicon;
use ticketguard::report::{
    build_report, draft_text, label, render_html, DISCLAIMER, DRAFT_HEAD, LIMITS, MANUAL_ITEMS, MODEL_ROW, TITLE,
};
use ticketguard::review::{review_text, scenario_number};
use ticketguard::rules::load_rules;
use ticketguard::schema::Severity;
use ticketguard::templates::load_templates;

const ALL: [&str; 12] = [
    "d1_a",
    "d1_b",
    "d1_a_drop_ground",
    "d1_a_crew_off",
    "d1_a_time_swap",
    "d1_a_plan_swap",
    "d1_a_outage_wrong",
    "d1_a_loc_strip",
    "d1_a_drop_signer",
    "d1_a_paraphrase",
    "d1_a_rename_lines",
    "d1_b_unrelated",
];
const SECTIONS: [&str; 7] = ["ticket", "verdict", "checklist", "findings", "draft-section", "scenario", "limits"];
const MEASURES: &str = "安全措施（对照典票）";

fn report_of(text: &str, data: &Snapshot, source: &str) -> Value {
    let review = review_text(text, &data.rules, &data.templates, &data.lexicon).expect("预审成功");
    build_report(&review, &data.templates, source)
}

fn report_for(name: &str, data: &Snapshot) -> Value {
    report_of(&ticket(name), data, &format!("{name}.txt"))
}

/// 把某条规则当作“业务已核对”：它的发现按规则自己的严重度报出来。
fn reviewed(mut data: Snapshot, id: &str) -> Snapshot {
    for rule in &mut data.rules {
        if rule.id == id {
            rule.status = "reviewed".to_string();
        }
    }
    data
}

fn keys(value: &Value) -> Vec<String> {
    value.as_object().expect("应为 JSON 对象").keys().cloned().collect()
}

fn list(value: &Value) -> &Vec<Value> {
    value.as_array().expect("应为 JSON 数组")
}

fn text<'a>(value: &'a Value, key: &str) -> &'a str {
    value[key].as_str().unwrap_or_else(|| panic!("字段 {key} 应为字符串：{value}"))
}

fn finding<'a>(report: &'a Value, rule_id: &str) -> &'a Value {
    list(&report["findings"])
        .iter()
        .find(|finding| finding["rule_id"] == rule_id)
        .unwrap_or_else(|| panic!("报告里应当有 {rule_id} 的发现"))
}

fn row<'a>(report: &'a Value, name: &str) -> &'a Value {
    list(&report["checklist"])
        .iter()
        .find(|row| row["name"] == name)
        .unwrap_or_else(|| panic!("审票清单里应当有“{name}”这一行"))
}

/// 与 `html.escape(quote=True)` 一致。
fn escape(text: &str) -> String {
    text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;").replace('\'', "&#x27;")
}

/// 取 `<tag …>` 和 `</tag>` 之间的内容。
fn inner<'a>(html: &'a str, tag: &str) -> &'a str {
    let open = html.find(&format!("<{tag}")).unwrap_or_else(|| panic!("页面里应当有 <{tag}>"));
    let start = open + html[open..].find('>').expect("标签应当闭合") + 1;
    let end = start + html[start..].find(&format!("</{tag}>")).unwrap_or_else(|| panic!("页面里应当有 </{tag}>"));
    &html[start..end]
}

// ---------- 报告模型 ----------

#[test]
fn model_sections_in_order() {
    let report = report_for("d1_a", &snapshot());
    assert_eq!(
        keys(&report),
        [
            "title",
            "source",
            "disclaimer",
            "ticket",
            "verdict",
            "checklist",
            "findings",
            "scenario",
            "draft",
            "limits",
            "feedback"
        ]
    );
    assert_eq!(report["title"], TITLE);
    assert_eq!(report["source"], "d1_a.txt");
    assert_eq!(report["disclaimer"], DISCLAIMER);
    assert_eq!(report["limits"], serde_json::json!(LIMITS));
    assert_eq!(
        keys(&report["ticket"]),
        ["number", "type", "plan_start", "plan_end", "leader", "crew_size", "work_tasks"]
    );
    assert_eq!(report["ticket"]["number"], "2099-01-P1-001");
    assert_eq!(report["ticket"]["type"], "配电第一种工作票");
    assert_eq!(report["ticket"]["leader"], "张三");
    assert_eq!(report["ticket"]["crew_size"], 5);
    assert_eq!(report["ticket"]["plan_start"], "2099-01-10T08:30");
    assert_eq!(keys(&report["verdict"]), ["level", "headline", "detail"]);
    assert_eq!(keys(&report["scenario"]), ["matched", "label", "similarity", "score", "caution", "others"]);
}

#[test]
fn verdict_levels() {
    let data = snapshot();
    let level = |name: &str| report_for(name, &data)["verdict"].clone();

    let clear = level("d1_b");
    assert_eq!(clear["level"], "clear");
    assert_eq!(clear["headline"], "机器检查的项目没有发现疑点");
    assert_eq!(clear["detail"], "机器只查了下方清单里的项目，其余请人工审核。");

    let notes = level("d1_a");
    assert_eq!(notes["level"], "notes");
    assert_eq!(notes["headline"], "机器没有发现疑点，有 1 条提示请过目");

    // 规则是草稿：人数对不上也只算提示，但两条提示都要数进去
    let draft_rule = level("d1_a_crew_off");
    assert_eq!(draft_rule["level"], "notes");
    assert_eq!(draft_rule["headline"], "机器没有发现疑点，有 2 条提示请过目");

    let issues = level("d1_a_drop_ground");
    assert_eq!(issues["level"], "issues");
    assert_eq!(issues["headline"], "发现 1 处疑点，请逐条确认");
    assert_eq!(issues["detail"], "确认属实的事项，勾选后会写进下方的打回意见草稿。");

    let none = level("d1_b_unrelated");
    assert_eq!(none["level"], "no_scenario");
    assert_eq!(none["headline"], "没有找到对应的典票场景，安全措施没有做比对");

    // 规则经业务核对后，同一个问题升级成疑点
    let after_review = report_for("d1_a_crew_off", &reviewed(snapshot(), "R-CREW-001"));
    assert_eq!(after_review["verdict"]["level"], "issues");
    assert_eq!(after_review["verdict"]["headline"], "发现 1 处疑点，请逐条确认");
}

#[test]
fn checklist_says_what_was_checked() {
    let data = snapshot();
    let clean = report_for("d1_b", &data);
    let names: Vec<&str> = list(&clean["checklist"]).iter().map(|row| text(row, "name")).collect();
    assert_eq!(
        names,
        ["票面必填与签名", "人员与人数", "时间顺序", "工作地点", MEASURES, MODEL_ROW, "机器查不了的项目"],
        "第一种票没有适用的“特殊作业要求”规则，这一组不应出现"
    );
    let statuses: Vec<&str> = list(&clean["checklist"]).iter().map(|row| text(row, "status")).collect();
    // 规则模式下“模型复核”一行是“机器没查”（T-013）
    assert_eq!(statuses, ["checked", "checked", "checked", "checked", "checked", "skipped", "skipped"]);
    for entry in list(&clean["checklist"]) {
        assert_eq!(keys(entry), ["name", "status", "detail"]);
    }
    assert_eq!(row(&clean, "票面必填与签名")["detail"], "检查了 2 项，没有发现疑点");
    assert_eq!(row(&clean, MEASURES)["detail"], "对照典票场景 2「开闭所间隔检修」，各类必备措施都找到了");
    assert_eq!(row(&clean, "机器查不了的项目")["detail"], MANUAL_ITEMS);

    let crew = report_for("d1_a_crew_off", &data);
    assert_eq!(row(&crew, "人员与人数")["status"], "found");
    assert_eq!(row(&crew, "人员与人数")["detail"], "有 2 条需要确认，见下方");
    assert_eq!(row(&crew, "时间顺序")["status"], "checked");

    let missing = report_for("d1_a_drop_ground", &data);
    assert_eq!(row(&missing, MEASURES)["status"], "found");
    assert_eq!(row(&missing, MEASURES)["detail"], "对照典票场景 1「单台公变更换」，有 1 类措施没有找到，见下方");

    let unrelated = report_for("d1_b_unrelated", &data);
    assert_eq!(row(&unrelated, MEASURES)["status"], "skipped");
    assert_eq!(row(&unrelated, MEASURES)["detail"], "没有找到足够相似的典票场景，没有做比对。请人工审核全部安全措施");
    // 没有做措施比对时，其它检查照常做、照常显示
    assert_eq!(row(&unrelated, "时间顺序")["status"], "checked");
}

#[test]
fn checked_rules_are_recorded() {
    let data = snapshot();
    let review = review_text(&ticket("d1_a"), &data.rules, &data.templates, &data.lexicon).expect("预审成功");
    assert_eq!(
        review.checked_rules,
        ["R-LOC-001", "R-LOC-002", "R-CREW-001", "R-REQ-001", "R-REQ-002", "R-TIME-001", "R-TIME-002", "R-NEW-001"],
        "只记录票种对得上的规则，顺序同规则列表"
    );
}

#[test]
fn rule_finding_fields() {
    let data = snapshot();
    let report = report_for("d1_a_crew_off", &data);
    let crew = finding(&report, "R-CREW-001");
    assert_eq!(
        keys(crew),
        [
            "rule_id",
            "severity",
            "label",
            "group",
            "title",
            "message",
            "ticket_says",
            "typical_text",
            "basis",
            "unreviewed",
            "picked",
            "draft_line",
            "evidence",
            "confidence",
            "scenario_id",
            "second"
        ]
    );
    assert_eq!(keys(&crew["basis"]), ["doc", "clause", "text", "source"]);
    assert_eq!(crew["severity"], "info");
    assert_eq!(crew["label"], "提示");
    assert_eq!(crew["group"], "人员与人数");
    assert_eq!(crew["title"], "总人数应与工作班人员名单一致");
    assert_eq!(crew["typical_text"], "");
    assert_eq!(crew["unreviewed"], true);
    assert_eq!(crew["picked"], false);
    assert_eq!(
        text(crew, "draft_line"),
        format!(
            "{}（依据：{} {}）",
            text(crew, "message"),
            text(&crew["basis"], "doc"),
            text(&crew["basis"], "clause")
        )
    );

    // 票面上的原话要带出来
    let reminder = finding(&report, "R-NEW-001");
    assert_eq!(reminder["ticket_says"], "新员工");
    assert_eq!(reminder["group"], "人员与人数");

    let after_review = report_for("d1_a_crew_off", &reviewed(snapshot(), "R-CREW-001"));
    let crew = finding(&after_review, "R-CREW-001");
    assert_eq!(crew["severity"], "error");
    assert_eq!(crew["label"], "疑点");
    assert_eq!(crew["unreviewed"], false);
    assert_eq!(crew["picked"], true);

    for (rule_id, group) in [("R-TIME-001", "时间顺序"), ("R-LOC-002", "工作地点"), ("R-REQ-002", "票面必填与签名")]
    {
        let name = match rule_id {
            "R-TIME-001" => "d1_a_time_swap",
            "R-LOC-002" => "d1_a_outage_wrong",
            _ => "d1_a_drop_signer",
        };
        assert_eq!(finding(&report_for(name, &data), rule_id)["group"], group);
    }
    assert_eq!(label(Severity::Error), "疑点");
    assert_eq!(label(Severity::Warning), "疑点");
    assert_eq!(label(Severity::Info), "提示");
}

#[test]
fn missing_measure_finding_is_readable() {
    let data = snapshot();
    let report = report_for("d1_a_drop_ground", &data);
    let missing = finding(&report, "CMP-MISSING");
    assert_eq!(missing["severity"], "warning");
    assert_eq!(missing["label"], "疑点");
    assert_eq!(missing["group"], MEASURES);
    assert_eq!(missing["title"], "“接地”栏可能缺少一类措施");
    assert_eq!(missing["message"], "对照典票场景 1「单台公变更换」，票面“接地”栏中没有找到这类措施。");
    assert_eq!(missing["ticket_says"], "这一栏没有填写措施（写的是“无”或空着）");
    assert_eq!(missing["typical_text"], "在10kV测试线7号杆小号侧装设接地线1组。（共1组接地线）");
    assert_eq!(missing["unreviewed"], false);
    assert_eq!(missing["picked"], true);
    // 机器可读的标识留在 JSON 里
    assert_eq!(missing["scenario_id"], "D1-S01");
    assert!(text(missing, "evidence").starts_with("D1-S01-M"), "evidence 仍是模板里的措施编号");

    // 这一栏写了东西、但不是典票要求的那类措施：把票面上写的原话带出来
    let reworded = ticket("d1_a").replace("所在杆塔下方明显可视位置悬挂“禁止合闸，线路有人工作！”标志牌", "处设置提醒");
    let report = report_of(&reworded, &data, "改写.txt");
    let missing = finding(&report, "CMP-MISSING");
    assert_eq!(missing["title"], "“标志牌”栏可能缺少一类措施");
    assert_eq!(missing["ticket_says"], "在10kV测试线测试01开关处设置提醒");

    let low = finding(&report_for("d1_b_unrelated", &data), "MATCH-LOW").clone();
    assert_eq!(low["title"], "没有找到对应的典票场景");
    assert_eq!(low["message"], "没有找到足够相似的典票场景，本次没有做安全措施比对，请人工审核全部安全措施。");
    assert_eq!(low["group"], MEASURES);
    assert_eq!(low["picked"], false);
}

#[test]
fn draft_is_built_from_picked_findings() {
    let data = snapshot();
    let report = report_for("d1_a_drop_ground", &data);
    let line = text(finding(&report, "CMP-MISSING"), "draft_line");
    assert_eq!(text(&report, "draft"), format!("{DRAFT_HEAD}\n1. {line}"));
    assert_eq!(report_for("d1_b", &data)["draft"], "");
    assert_eq!(report_for("d1_a", &data)["draft"], "", "只有提示时草稿为空");

    // 序号在被选中的发现里连续编号，与它们在清单里的位置无关
    let mut review =
        review_text(&ticket("d1_a_drop_ground"), &data.rules, &data.templates, &data.lexicon).expect("预审成功");
    review.findings.reverse();
    assert_eq!(review.findings[0].severity, Severity::Info, "倒过来以后提示排在前面");
    assert_eq!(draft_text(&review.findings), format!("{DRAFT_HEAD}\n1. {line}"));
}

#[test]
fn scenario_block() {
    let data = snapshot();
    let own = report_for("d1_a", &data)["scenario"].clone();
    assert_eq!(own["matched"], true);
    assert_eq!(own["label"], "典票场景 1「单台公变更换」");
    assert_eq!(own["similarity"], "高");
    assert_eq!(own["caution"], "");
    assert_eq!(list(&own["others"]).len(), 1, "快照里只有两个场景");
    assert_eq!(keys(&own["others"][0]), ["label", "similarity", "score"]);
    assert_eq!(own["others"][0]["label"], "典票场景 2「开闭所间隔检修」");
    assert_eq!(own["others"][0]["similarity"], "低");
    let score = own["score"].as_f64().expect("score 是数字");
    assert_eq!(format!("{score:.4}").parse::<f64>().unwrap(), score, "score 四舍五入到 4 位小数");
    assert!(score >= 0.6);

    assert_eq!(report_for("d1_a_drop_ground", &data)["scenario"]["similarity"], "中");

    let none = report_for("d1_b_unrelated", &data)["scenario"].clone();
    assert_eq!(none["matched"], false);
    assert_eq!(none["label"], "典票场景 2「开闭所间隔检修」", "没匹配上时也说出最接近的是哪个");
    assert_eq!(none["similarity"], "低");
    assert_eq!(none["caution"], "", "没做比对，就不需要“比对结果仅供参考”的提醒");

    assert_eq!(scenario_number("D1-S01"), "1");
    assert_eq!(scenario_number("D1-S12"), "12");
    assert_eq!(scenario_number("D1-S20"), "20");
}

#[test]
fn low_similarity_carries_a_caution() {
    // 用仓库里现行的规则和 21 个典票模板：编造票面在真模板里找到的“最像的场景”往往并不对。
    // 不变式：做了比对且相似度低 ⇔ 有提醒。
    let root = repo_root();
    let data = Snapshot {
        rules: load_rules(&root.join("data/rules")).expect("规则库能加载"),
        templates: load_templates(&root.join("data/templates/D1")).expect("模板能加载"),
        lexicon: load_lexicon(&root.join("data/rules/lexicon.json")).expect("词表能加载"),
    };
    for name in ALL {
        let scenario = report_for(name, &data)["scenario"].clone();
        let expect_caution = scenario["matched"] == true && scenario["similarity"] == "低";
        let caution = text(&scenario, "caution");
        assert_eq!(!caution.is_empty(), expect_caution, "{name}：{scenario}");
        if expect_caution {
            assert_eq!(caution, "相似度低，对照的场景可能不对，措施比对的结果仅供参考。");
        }
        let similarity = text(&scenario, "similarity");
        assert!(["高", "中", "低"].contains(&similarity), "{name}：相似度只能是高、中、低");
        assert!(list(&scenario["others"]).len() == 2, "{name}：21 个模板时应列出另外两个相近场景");
    }
}

// ---------- 页面 ----------

#[test]
fn page_has_seven_sections_in_order() {
    let data = snapshot();
    for name in ALL {
        let report = report_for(name, &data);
        let html = render_html(&report);
        assert!(html.starts_with("<!doctype html>"), "{name}");
        assert!(html.contains("<html lang=\"zh-CN\">") && html.contains("<meta charset=\"utf-8\">"), "{name}");
        assert!(html.contains(&format!("<title>{TITLE}</title>")), "{name}");

        let mut last = 0;
        for id in SECTIONS {
            let at = html.find(&format!("id=\"{id}\"")).unwrap_or_else(|| panic!("{name}：页面缺少区块 {id}"));
            assert!(at > last, "{name}：区块 {id} 的顺序不对");
            last = at;
        }

        let level = text(&report["verdict"], "level");
        assert!(html.contains(&format!("class=\"verdict verdict-{level}\" data-level=\"{level}\"")), "{name}");
        assert!(html.contains(&escape(text(&report["verdict"], "headline"))), "{name}");
        assert!(html.contains(&escape(text(&report["verdict"], "detail"))), "{name}");

        let rows = list(&report["checklist"]);
        assert_eq!(html.matches("<tr class=\"check-").count(), rows.len(), "{name}：清单每项一行");
        for entry in rows {
            assert!(html.contains(&format!("<tr class=\"check-{}\">", text(entry, "status"))), "{name}");
            assert!(html.contains(&escape(text(entry, "name"))), "{name}");
            assert!(html.contains(&escape(text(entry, "detail"))), "{name}");
        }

        assert!(html.contains("<textarea id=\"draft\""), "{name}");
        assert!(html.contains(&escape(text(&report, "draft"))), "{name}：草稿要写进文本框");
        assert!(html.contains(&escape(text(&report["scenario"], "label"))), "{name}");
        assert!(html.contains(&format!("相似度：{}", text(&report["scenario"], "similarity"))), "{name}");
        assert!(html.contains(&escape(DISCLAIMER)), "{name}");
        for line in LIMITS {
            assert!(html.contains(&escape(line)), "{name}：缺少局限说明：{line}");
        }
        assert!(html.contains(&format!("报告对应的文件：{name}.txt")), "{name}");
        assert_eq!(render_html(&report), html, "{name}：同一份报告每次渲染必须完全一样");
    }
}

#[test]
fn checklist_status_has_words_not_only_color() {
    let data = snapshot();
    let html = render_html(&report_for("d1_a_drop_ground", &data));
    assert!(html.contains("✓ 已查，无疑点"));
    assert!(html.contains("! 需要确认"));
    assert!(html.contains("— 机器没查"));
}

#[test]
fn page_shows_readable_times() {
    // 审票人不该看到 ISO 的 “T”：2099-01-10T08:30 要显示成 2099-01-10 08:30。
    let data = snapshot();
    let html = render_html(&report_for("d1_a", &data));
    assert!(html.contains("2099-01-10 08:30 至 2099-01-10 17:30"), "票面概况的时间要写成看得懂的样子");
    assert!(!html.contains("T08:30") && !html.contains("T17:30"));

    // 规则消息里的时间也一样（这条票把接收时间改成了 2000 年）
    let clock = render_html(&report_for("d1_a_time_swap", &data));
    assert!(clock.contains("2000-01-01 00:00"), "规则消息里的时间也要去掉 T");
    assert!(!clock.contains("2000-01-01T00:00"));
}

#[test]
fn low_similarity_findings_are_hints_not_issues() {
    // 对照的场景相似度低，说明可能根本认错了场景。这时候“缺少某类措施”不能当成疑点，
    // 更不能默认写进打回意见——那是拿一个可能不对的场景去指责这张票。
    let root = repo_root();
    let data = Snapshot {
        rules: load_rules(&root.join("data/rules")).expect("规则库能加载"),
        templates: load_templates(&root.join("data/templates/D1")).expect("模板能加载"),
        lexicon: load_lexicon(&root.join("data/rules/lexicon.json")).expect("词表能加载"),
    };
    let report = report_for("d1_a_drop_ground", &data);
    assert_eq!(report["scenario"]["similarity"], "低", "先确认这张编造票在 21 个真模板里确实匹配到了低相似度的场景");
    assert_eq!(report["verdict"]["level"], "notes");
    let missing = finding(&report, "CMP-MISSING");
    assert_eq!(missing["severity"], "info");
    assert_eq!(missing["label"], "提示");
    assert_eq!(missing["picked"], false, "相似度低时默认不勾选");
    assert!(
        text(missing, "message").contains("相似度低"),
        "这条缺项要自己说明为什么只是提示：{}",
        text(missing, "message")
    );
    assert!(text(missing, "message").contains("请人工核对"));
    assert_eq!(report["draft"], "", "没有被勾选的事项，草稿为空");
    // 相似度中、高的时候，缺项仍然是疑点（用快照模板：0.5373）
    let own = report_for("d1_a_drop_ground", &snapshot());
    assert_eq!(own["scenario"]["similarity"], "中");
    assert_eq!(finding(&own, "CMP-MISSING")["severity"], "warning");
}

#[test]
fn work_task_with_one_empty_side_reads_naturally() {
    // “地点 | 任务”里有一边没写：只显示写了的那一边，不要出现“未填写：补充任务”
    let data = snapshot();
    let html = render_html(&report_for("d1_b", &data));
    assert!(html.contains("<li>补充任务</li>"), "只有任务时就直接显示任务");
    assert!(!html.contains("未填写：补充任务"));
    let both = render_html(&report_for("d1_a", &data));
    assert!(both.contains(
        "<li>35kV甲站10kV测试线乙支6号杆甲公变台架：（XX分局）35kV甲站10kV测试线配合：更换10kV测试线甲公变。</li>"
    ));
}

#[test]
fn finding_cards_match_the_model() {
    let data = snapshot();
    let mut variants: Vec<(String, Value)> =
        ALL.iter().map(|name| (name.to_string(), report_for(name, &data))).collect();
    variants.push(("人数规则已核对".to_string(), report_for("d1_a_crew_off", &reviewed(snapshot(), "R-CREW-001"))));
    for (name, report) in variants {
        let html = render_html(&report);
        let findings = list(&report["findings"]);
        let picked = findings.iter().filter(|finding| finding["picked"] == true).count();
        assert_eq!(html.matches("<div class=\"finding finding-").count(), findings.len(), "{name}：每条发现一张卡片");
        assert_eq!(html.matches("class=\"pick\"").count(), findings.len(), "{name}：每张卡片一个勾选框");
        assert_eq!(html.matches("onchange=\"updateDraft()\" checked>").count(), picked, "{name}：疑点默认勾选");
        assert_eq!(
            html.matches("onchange=\"updateDraft()\">").count(),
            findings.len() - picked,
            "{name}：提示默认不勾"
        );
        if findings.is_empty() {
            assert!(html.contains("没有需要确认的事项。"), "{name}");
            assert!(!html.contains("写进打回意见"), "{name}");
        } else {
            assert!(html.contains("写进打回意见"), "{name}");
            assert!(!html.contains("没有需要确认的事项。"), "{name}");
        }
        for finding in findings {
            assert!(html.contains(&format!("<div class=\"finding finding-{}\">", text(finding, "severity"))), "{name}");
            assert!(
                html.contains(&format!(
                    "<input type=\"checkbox\" class=\"pick\" data-text=\"{}\" onchange=\"updateDraft()\"",
                    escape(text(finding, "draft_line"))
                )),
                "{name}：勾选框要带着这条发现的草稿文字"
            );
            for key in ["label", "group", "title", "message"] {
                assert!(html.contains(&escape(text(finding, key))), "{name}：卡片缺少 {key}");
            }
            let basis = &finding["basis"];
            assert!(
                html.contains(&format!(
                    "依据：{} {}：{}",
                    escape(text(basis, "doc")),
                    escape(text(basis, "clause")),
                    escape(text(basis, "text"))
                )),
                "{name}：卡片缺少依据"
            );
            assert!(html.contains(&format!("（出处：{}）", escape(text(basis, "source")))), "{name}");
            if !text(finding, "ticket_says").is_empty() {
                assert!(html.contains(&format!("票面上：{}", escape(text(finding, "ticket_says")))), "{name}");
            }
            if !text(finding, "typical_text").is_empty() {
                assert!(html.contains(&format!("典票上的写法：{}", escape(text(finding, "typical_text")))), "{name}");
            }
        }
        // 有内容才显示对应的那一行，没有内容的不留空壳
        let count = |key: &str| findings.iter().filter(|finding| !text(finding, key).is_empty()).count();
        assert_eq!(html.matches("票面上：").count(), count("ticket_says"), "{name}：“票面上：”的行数");
        assert_eq!(html.matches("典票上的写法：").count(), count("typical_text"), "{name}：“典票上的写法：”的行数");
        assert_eq!(
            html.matches("（这是典票里同类作业的写法，线路和设备名称会和本票不同）").count(),
            count("typical_text"),
            "{name}：典票原文后面要说明这是另一张票的写法"
        );
        let unreviewed = findings.iter().filter(|finding| finding["unreviewed"] == true).count();
        assert_eq!(
            html.matches("这条规则还没有经过业务人员核对，暂时只作为提示。").count(),
            unreviewed,
            "{name}：草稿规则要逐条说明"
        );
    }
}

#[test]
fn page_shows_no_internal_identifiers() {
    let data = snapshot();
    let mut pages: Vec<(String, String)> =
        ALL.iter().map(|name| (name.to_string(), render_html(&report_for(name, &data)))).collect();
    pages.push((
        "人数规则已核对".to_string(),
        render_html(&report_for("d1_a_crew_off", &reviewed(snapshot(), "R-CREW-001"))),
    ));
    let internal = [
        "R-CREW",
        "R-REQ",
        "R-TIME",
        "R-LOC",
        "R-NEW",
        "R-HOT",
        "R-LIVE",
        "CMP-MISSING",
        "MATCH-LOW",
        "D1-S",
        "{线路}",
        "{杆号}",
        "{编号}",
        "{站}",
        "匹配度",
        "rule_id",
        "scenario_id",
        "draft_line",
        "confidence",
    ];
    for (name, html) in &pages {
        for word in internal {
            assert!(!html.contains(word), "{name}：页面上不应出现内部标识“{word}”");
        }
    }
}

#[test]
fn page_never_passes_judgement() {
    // 红线 3：机器不做最终裁决。这些说法在任何状态下都不能出现。
    let data = snapshot();
    let mut pages: Vec<String> = ["d1_b", "d1_b_unrelated", "d1_a_drop_ground", "d1_a_crew_off"]
        .iter()
        .map(|name| render_html(&report_for(name, &data)))
        .collect();
    pages.push(render_html(&report_for("d1_a_crew_off", &reviewed(snapshot(), "R-CREW-001"))));
    for html in &pages {
        for word in ["违章", "不合格", "必须打回", "审核通过", "没有问题", "无问题"] {
            assert!(!html.contains(word), "页面上不应出现“{word}”");
        }
    }
}

#[test]
fn page_is_offline_and_works_in_ie11() {
    let data = snapshot();
    for name in ALL {
        let html = render_html(&report_for(name, &data));
        for word in ["http://", "https://", "<link", "src=", "<details"] {
            assert!(!html.contains(word), "{name}：页面不应包含 {word}");
        }
        let script = inner(&html, "script");
        for word in ["`", "=>", "let ", "const ", ".forEach("] {
            assert!(!script.contains(word), "{name}：脚本只能用 ES5，不应包含 {word}");
        }
        assert!(script.contains("function updateDraft") && script.contains("function copyDraft"), "{name}");
        // DRAFT_HEAD 以 \uXXXX 形式写进脚本，不依赖页面编码
        assert!(script.contains("\\u9884\\u5ba1\\u610f\\u89c1"), "{name}：草稿抬头要用 json_ascii 写进脚本");
        assert!(html.contains("window.print()"), "{name}：要有打印按钮");
        let style = inner(&html, "style");
        for word in ["var(--", "display: grid", "display:grid"] {
            assert!(!style.contains(word), "{name}：样式要兼容 IE11，不应包含 {word}");
        }
        assert!(style.contains("@media print"), "{name}：要有打印样式");
    }
}

#[test]
fn page_escapes_everything_from_the_ticket() {
    let data = snapshot();
    let hostile = ticket("d1_a")
        .replace("配合：更换10kV测试线甲公变。", "配合：<img onerror=alert(1)>&更换。")
        .replace("赵六（新员工）", "赵六（新员工\"><b>）");
    let report = report_of(&hostile, &data, "带<尖括号>的文件名.txt");
    assert_eq!(finding(&report, "R-NEW-001")["ticket_says"], "新员工\"><b>", "先确认注入的文字确实进了报告");
    let html = render_html(&report);
    assert!(!html.contains("<img onerror"), "工作任务里的标签必须转义");
    assert!(html.contains("&lt;img onerror=alert(1)&gt;&amp;更换。"));
    assert!(!html.contains("\"><b>"), "写进属性里的文字也必须转义");
    assert!(html.contains("新员工&quot;&gt;&lt;b&gt;"));
    assert!(html.contains("带&lt;尖括号&gt;的文件名.txt"));
}
