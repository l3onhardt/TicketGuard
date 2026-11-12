//! 场景匹配与措施比对。移植自参考实现 `src/ticketguard/review.py`（设计见 ADR-0004）。

use crate::entities::{abstract_text, keywords, normalize, strip_placeholders, Lexicon};
use crate::ingest::ticket_parser::{parse_dist1, ParseError};
use crate::rules::engine::{run_rules, Rule};
use crate::schema::{Basis, Finding, Severity, Ticket};
use crate::templates::Template;
use std::collections::BTreeSet;

/// 见 ADR-0004：0.4 时整栏措施被删的票会跌破阈值而漏检
pub const MATCH_MIN: f64 = 0.3;
pub const LOW_SIMILARITY: f64 = 0.45;
pub const HIGH_SIMILARITY: f64 = 0.60;

pub fn cmp_basis() -> Basis {
    Basis {
        doc: "《中国南方电网有限责任公司电力安全工作规程第3部分：配电》（典票库第一章转述）".to_string(),
        clause: "5.3.1".to_string(),
        text: "工作票所列安全措施正确、完备，由工作负责人、签发人、会签人、工作票接收人、许可人负责。".to_string(),
        source: "2026.9典票库 第一章“两票”相关文件内容说明".to_string(),
    }
}

/// 这次预审有没有请模型复核（见 ADR-0008）。
#[derive(Debug, Clone, PartialEq)]
pub enum ModelUse {
    /// 规则模式：没有请模型
    Off,
    /// 模型复核做完了。asked 是向模型提问的次数
    Done { asked: usize },
    /// 想用模型但没用成，附一句给人看的原因
    Failed(String),
}

/// 一次预审的结果：结构化票面、前 3 个候选场景、全部发现。
pub struct Review {
    pub ticket: Ticket,
    pub candidates: Vec<(String, f64)>,
    pub findings: Vec<Finding>,
    /// 这张票实际跑过的规则编号（票种对得上的那些），顺序同规则列表
    pub checked_rules: Vec<String>,
    pub model: ModelUse,
}

/// 按严重度排序。稳定排序：同一严重度内保持原顺序。
pub fn sort_findings(findings: &mut [Finding]) {
    findings.sort_by_key(|finding| match finding.severity {
        Severity::Error => 0,
        Severity::Warning => 1,
        Severity::Info => 2,
    });
}

/// 模板编号 → 场景序号：“D1-S01” → “1”，“D1-S12” → “12”
pub fn scenario_number(template_id: &str) -> String {
    template_id.rsplit_once('S').expect("模板编号包含场景序号").1.parse::<u32>().expect("场景序号为整数").to_string()
}

fn jaccard<T: Ord>(a: &BTreeSet<T>, b: &BTreeSet<T>) -> f64 {
    if a.is_empty() && b.is_empty() {
        return 0.0;
    }
    a.intersection(b).count() as f64 / a.union(b).count() as f64
}

/// 占位符化、归一化后的二元字集合。
fn bigrams(text: &str, lexicon: &Lexicon) -> BTreeSet<(char, char)> {
    let cleaned = strip_placeholders(&normalize(&abstract_text(text), lexicon));
    let chars: Vec<char> = cleaned.chars().collect();
    chars.windows(2).map(|pair| (pair[0], pair[1])).collect()
}

/// 给全部模板打分并排序：0.5 × 任务文字的二元字 Jaccard + 0.5 × 措施签名的 Jaccard。
pub fn match_scenario(ticket: &Ticket, templates: &[Template], lexicon: &Lexicon) -> Vec<(String, f64)> {
    let task_text: String =
        ticket.work_tasks.iter().flat_map(|task| [task.place.as_str(), task.task.as_str()]).collect();
    let text = bigrams(&task_text, lexicon);
    let signature: BTreeSet<(String, String)> = ticket
        .measures
        .iter()
        .flat_map(|measure| {
            keywords(&measure.text, lexicon).into_iter().map(move |word| (measure.category.clone(), word))
        })
        .collect();
    let mut candidates: Vec<(String, f64)> = templates
        .iter()
        .map(|template| {
            let other_text = bigrams(&format!("{}{}", template.title, template.work_task), lexicon);
            let other_signature: BTreeSet<(String, String)> = template
                .required_measures
                .iter()
                .flat_map(|rm| rm.keywords.iter().map(move |word| (rm.category.clone(), word.clone())))
                .collect();
            let score = 0.5 * jaccard(&text, &other_text) + 0.5 * jaccard(&signature, &other_signature);
            (template.id.clone(), score)
        })
        .collect();
    // 与参考实现一致：分数降序，分数相同按场景编号升序。
    candidates.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap().then_with(|| a.0.cmp(&b.0)));
    candidates
}

/// 逐条比对模板的必备措施：同一类别里有一条措施的签名包含它，就算“有”。
pub fn compare_measures(ticket: &Ticket, template: &Template, lexicon: &Lexicon, confidence: f64) -> Vec<Finding> {
    let measures: Vec<(&str, BTreeSet<String>)> = ticket
        .measures
        .iter()
        .map(|measure| (measure.category.as_str(), keywords(&measure.text, lexicon).into_iter().collect()))
        .collect();
    let mut findings = Vec::new();
    for rm in &template.required_measures {
        let present = measures
            .iter()
            .any(|(category, words)| *category == rm.category && rm.keywords.iter().all(|word| words.contains(word)));
        if present {
            continue;
        }
        let ticket_says = ticket
            .measures
            .iter()
            .filter(|measure| measure.category == rm.category)
            .map(|measure| measure.text.as_str())
            .collect::<Vec<_>>()
            .join("；");
        let caution = if confidence < LOW_SIMILARITY {
            "注意：对照的场景相似度低，这个场景可能不对，这条缺项不一定是真的，请人工核对。"
        } else {
            ""
        };
        findings.push(Finding {
            rule_id: "CMP-MISSING".to_string(),
            severity: if rm.must && confidence >= LOW_SIMILARITY { Severity::Warning } else { Severity::Info },
            message: format!(
                "对照典票场景 {}「{}」，票面“{}”栏中没有找到这类措施。{caution}",
                scenario_number(&template.id),
                template.title,
                rm.category
            ),
            basis: cmp_basis(),
            evidence: rm.id.clone(),
            confidence,
            needs_review: true,
            scenario_id: Some(template.id.clone()),
            title: format!("“{}”栏可能缺少一类措施", rm.category),
            ticket_says: if ticket_says.is_empty() {
                "这一栏没有填写措施（写的是“无”或空着）".to_string()
            } else {
                ticket_says
            },
            typical_text: rm.source_text.clone(),
            unreviewed: false,
            second: None,
        });
    }
    findings
}

/// 预审一张票面文本：解析 → 硬规则 → 场景匹配 → 措施比对 → 按严重度排序。
pub fn review_text(
    text: &str,
    rules: &[Rule],
    templates: &[Template],
    lexicon: &Lexicon,
) -> Result<Review, ParseError> {
    let ticket = parse_dist1(text)?;
    let checked_rules = rules
        .iter()
        .filter(|rule| rule.ticket_types.contains(&ticket.ticket_type.as_str().to_string()))
        .map(|rule| rule.id.clone())
        .collect();
    let mut findings = run_rules(&ticket, rules);
    let candidates = match_scenario(&ticket, templates, lexicon);
    let (scenario_id, confidence) = candidates[0].clone();
    if confidence >= MATCH_MIN {
        let template = templates.iter().find(|template| template.id == scenario_id).expect("候选场景一定来自模板列表");
        findings.extend(compare_measures(&ticket, template, lexicon, confidence));
    } else {
        findings.push(Finding {
            rule_id: "MATCH-LOW".to_string(),
            severity: Severity::Info,
            message: "没有找到足够相似的典票场景，本次没有做安全措施比对，请人工审核全部安全措施。".to_string(),
            basis: cmp_basis(),
            evidence: String::new(),
            confidence,
            needs_review: true,
            scenario_id: None,
            title: "没有找到对应的典票场景".to_string(),
            ticket_says: String::new(),
            typical_text: String::new(),
            unreviewed: false,
            second: None,
        });
    }
    sort_findings(&mut findings);
    Ok(Review {
        ticket,
        candidates: candidates.into_iter().take(3).collect(),
        findings,
        checked_rules,
        model: ModelUse::Off,
    })
}
