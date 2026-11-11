//! 用本地的典票库文本做验收。典票库是内部资料（L1），不入库，
//! 所以 `data/interim` 不存在时这些测试直接跳过并打印原因。
//!
//! 这里守的是 AGENTS.md 的两条红线：
//! - 红线 4：每条规则的依据原文必须能在典票库里逐字找到，不得凭记忆编造；
//! - §6：全部典票零误报（典票本身就是标准答案）。

mod common;

use common::repo_root;
use std::path::PathBuf;
use ticketguard::entities::load_lexicon;
use ticketguard::evaluate::evaluate;
use ticketguard::ingest::{parse_dist1, split_scenarios};
use ticketguard::report::{build_report, render_html};
use ticketguard::review::review_text;
use ticketguard::rules::{load_rules, run_rules};
use ticketguard::templates::{build_template, load_templates};

const NAME: &str = "2026.9南宁供电局配电运检、配电自动化两票典型票库-2026版（试行）";
const SECTION: &str = "配电第一种工作票典型场景";

struct Local {
    body: String,
    notes: String,
}

/// 读本地典票文本；没有就返回 None（调用方跳过测试）。
fn local() -> Option<Local> {
    let dir: PathBuf = repo_root().join("data/interim");
    let body = std::fs::read_to_string(dir.join(format!("{NAME}.txt"))).ok();
    let notes = std::fs::read_to_string(dir.join(format!("{NAME}.notes.json"))).ok();
    match (body, notes) {
        (Some(body), Some(notes)) => Some(Local { body, notes }),
        _ => {
            eprintln!("跳过：本地没有典票文本（data/interim 未生成）");
            None
        }
    }
}

#[test]
fn basis_text_comes_from_source() {
    let Some(local) = local() else { return };
    // 脚注文件里的文字是 JSON 转义过的，先解析再拼回纯文本
    let notes: serde_json::Value = serde_json::from_str(&local.notes).expect("脚注文件是合法 JSON");
    let source = format!("{}{}", local.body, serde_json::to_string(&notes).unwrap());
    for rule in load_rules(&repo_root().join("data/rules")).expect("规则库能加载") {
        assert!(source.contains(&rule.basis.text), "{} 的依据原文在典票库中找不到：{}", rule.id, rule.basis.text);
    }
}

#[test]
fn scenario_counts_match_table_of_contents() {
    let Some(local) = local() else { return };
    let chunks = split_scenarios(&local.body);
    let count = |section: &str| chunks.iter().filter(|chunk| chunk.section == section).count();
    assert_eq!(count(SECTION), 21);
    assert_eq!(count("配电第二种工作票典型场景"), 5);
    assert_eq!(count("紧急抢修工作票典型场景"), 4);
    assert_eq!(count("一级、二级动火票典型场景"), 3);
    assert_eq!(count("书面布置票典型场景"), 29);
    assert_eq!(count("典型操作场景"), 30);
}

#[test]
fn typical_tickets_have_no_false_positive() {
    let Some(local) = local() else { return };
    let rules = load_rules(&repo_root().join("data/rules")).expect("规则库能加载");
    let mut found: Vec<(String, String)> = Vec::new();
    let mut measures = 0;
    let mut places = 0;
    for chunk in split_scenarios(&local.body).into_iter().filter(|chunk| chunk.section == SECTION) {
        let ticket = parse_dist1(&chunk.text).unwrap_or_else(|err| panic!("场景 {} 解析失败：{err}", chunk.number));
        measures += ticket.measures.len();
        places += ticket.work_tasks.len();
        for finding in run_rules(&ticket, &rules) {
            found.push((chunk.number.clone(), finding.rule_id));
        }
    }
    // 场景 13 的停电范围与工作地点不一致，是典票本身的问题（见 docs/08 待确认问题 7）
    assert_eq!(found, [("13".to_string(), "R-LOC-002".to_string())]);
    // 解析没有丢字段：与 docs/08 记录的数字一致
    assert_eq!(measures, 187, "21 张典票共解析出的安全措施条数");
    assert_eq!(places, 21, "21 张典票共解析出的工作地点条数");
}

#[test]
fn templates_regenerate_identically() {
    let Some(local) = local() else { return };
    let lexicon = load_lexicon(&repo_root().join("data/rules/lexicon.json")).expect("词表能加载");
    let dir = repo_root().join("data/templates/D1");
    let mut count = 0;
    for chunk in split_scenarios(&local.body).into_iter().filter(|chunk| chunk.section == SECTION) {
        let ticket = parse_dist1(&chunk.text).expect("典票能解析");
        let built = build_template(&chunk, &ticket, &lexicon);
        let id = built["id"].as_str().unwrap().to_string();
        let committed: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(dir.join(format!("{id}.json"))).expect("模板文件存在"))
                .expect("模板是合法 JSON");
        // 模板的 status 由业务人员从 draft 改成 reviewed，不算不一致
        let mut built = built;
        built["status"] = committed["status"].clone();
        assert_eq!(built, committed, "{id}：由典票重新生成的模板与入库的不一致（改了词表要重新生成模板）");
        count += 1;
    }
    assert_eq!(count, 21);
}

#[test]
fn typical_tickets_match_their_own_scenario() {
    let Some(local) = local() else { return };
    let rules = load_rules(&repo_root().join("data/rules")).expect("规则库能加载");
    let templates = load_templates(&repo_root().join("data/templates/D1")).expect("模板能加载");
    let lexicon = load_lexicon(&repo_root().join("data/rules/lexicon.json")).expect("词表能加载");
    for chunk in split_scenarios(&local.body).into_iter().filter(|chunk| chunk.section == SECTION) {
        let review = review_text(&chunk.text, &rules, &templates, &lexicon).expect("典票能预审");
        let expected = format!("D1-S{:02}", chunk.number.parse::<i64>().unwrap());
        assert_eq!(review.candidates[0].0, expected, "场景 {} 没有匹配到自己的模板", chunk.number);
        let missing: Vec<&str> =
            review.findings.iter().filter(|f| f.rule_id == "CMP-MISSING").map(|f| f.evidence.as_str()).collect();
        assert!(missing.is_empty(), "场景 {} 对照自己的模板却报了漏项：{missing:?}", chunk.number);
    }
}

#[test]
fn typical_tickets_report_no_issue() {
    // 典票是标准答案：报告的结论不能是“发现疑点”，措施比对那一项必须是“已查，无疑点”，
    // 而且每张典票都对照到它自己的场景。
    let Some(local) = local() else { return };
    let rules = load_rules(&repo_root().join("data/rules")).expect("规则库能加载");
    let templates = load_templates(&repo_root().join("data/templates/D1")).expect("模板能加载");
    let lexicon = load_lexicon(&repo_root().join("data/rules/lexicon.json")).expect("词表能加载");
    for chunk in split_scenarios(&local.body).into_iter().filter(|chunk| chunk.section == SECTION) {
        let review = review_text(&chunk.text, &rules, &templates, &lexicon).expect("典票能预审");
        let report = build_report(&review, &templates, "典票.txt");
        let level = report["verdict"]["level"].as_str().expect("verdict.level 是字符串");
        assert!(["clear", "notes"].contains(&level), "场景 {} 的结论是 {level}", chunk.number);
        let measures = report["checklist"]
            .as_array()
            .expect("checklist 是数组")
            .iter()
            .find(|row| row["name"] == "安全措施（对照典票）")
            .expect("清单里有安全措施一项");
        assert_eq!(measures["status"], "checked", "场景 {}", chunk.number);
        assert_eq!(report["scenario"]["matched"], true, "场景 {}", chunk.number);
        let label = report["scenario"]["label"].as_str().expect("scenario.label 是字符串");
        assert!(label.starts_with(&format!("典票场景 {}「", chunk.number)), "场景 {} 对照成了 {label}", chunk.number);
        let html = render_html(&report);
        assert!(!html.contains("D1-S") && !html.contains("{线路}"), "场景 {} 的页面露出了内部标识", chunk.number);
    }
}

#[test]
fn synthetic_set_baseline() {
    let Some(local) = local() else { return };
    let samples_path = repo_root().join("data/synthetic/d1.jsonl");
    let Ok(raw) = std::fs::read_to_string(samples_path) else {
        eprintln!("跳过：本地没有合成集（data/synthetic/d1.jsonl 未生成）");
        return;
    };
    let samples: Vec<serde_json::Value> =
        raw.lines().filter(|line| !line.trim().is_empty()).map(|line| serde_json::from_str(line).unwrap()).collect();
    let originals = split_scenarios(&local.body)
        .into_iter()
        .filter(|chunk| chunk.section == SECTION)
        .map(|chunk| (format!("D1-S{:02}", chunk.number.parse::<i64>().unwrap()), chunk.text))
        .collect();
    let rules = load_rules(&repo_root().join("data/rules")).expect("规则库能加载");
    let templates = load_templates(&repo_root().join("data/templates/D1")).expect("模板能加载");
    let lexicon = load_lexicon(&repo_root().join("data/rules/lexicon.json")).expect("词表能加载");
    let report = evaluate(&samples, &originals, &rules, &templates, &lexicon).expect("评测成功");
    // 换说法、换线路名的合格票不得产生任何意外发现（误报）
    for operator in ["paraphrase", "rename_lines"] {
        assert_eq!(report["by_operator"][operator]["unexpected"], 0, "{operator} 样本出现了误报");
    }
    // 硬规则类的错误必须全部检出
    for operator in ["crew_off", "time_swap", "plan_swap", "outage_wrong", "loc_strip", "drop_signer"] {
        let stats = &report["by_operator"][operator];
        assert_eq!(stats["detected"], stats["expected"], "{operator} 有漏检");
    }
}
