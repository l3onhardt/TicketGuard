//! 审票清单式的离线预审报告。

use crate::review::{scenario_number, Review, HIGH_SIMILARITY, LOW_SIMILARITY};
use crate::schema::{Finding, Severity};
use crate::templates::Template;
use serde_json::{json, Value};

pub const TITLE: &str = "工作票预审报告";
pub const DISCLAIMER: &str = "本报告是机器预审的结果，所列都是需要人工确认的疑点和提示；是否接收或打回，由审票人决定。";
pub const DRAFT_HEAD: &str = "预审意见（草稿，请核实后使用）：";
pub const LIMITS: [&str; 4] = [
    "机器只检查了“审票清单”里列出的项目，没有列出的都没有检查。",
    "不判断现场物理条件：实际安全距离够不够、所拉的开关能否真正隔离作业区，需要结合配网拓扑和现场勘察。",
    "只做字面和同义词比对：措施换了说法可能被误报成缺少；措施写了但写错（例如开关编号写错）查不出来。",
    "规则库和典票模板还没有经过业务人员逐条核对。",
];
pub const MANUAL_ITEMS: &str = "现场安全距离和隔离是否有效、设备名称和编号是否写对、票种是否选对、典票库以外的作业场景——这些机器查不了，请人工审核。";

pub fn label(severity: Severity) -> &'static str {
    match severity {
        Severity::Error | Severity::Warning => "疑点",
        Severity::Info => "提示",
    }
}

fn draft_item(finding: &Finding) -> String {
    format!("{}（依据：{} {}）", finding.message, finding.basis.doc, finding.basis.clause)
}

pub fn draft_text(findings: &[Finding]) -> String {
    let lines: Vec<String> = findings
        .iter()
        .filter(|finding| finding.severity != Severity::Info)
        .enumerate()
        .map(|(index, finding)| format!("{}. {}", index + 1, draft_item(finding)))
        .collect();
    if lines.is_empty() {
        return String::new();
    }
    format!("{DRAFT_HEAD}\n{}", lines.join("\n"))
}

fn group(id: &str) -> &'static str {
    if id.starts_with("R-REQ-") {
        "票面必填与签名"
    } else if id.starts_with("R-CREW-") || id.starts_with("R-NEW-") {
        "人员与人数"
    } else if id.starts_with("R-TIME-") {
        "时间顺序"
    } else if id.starts_with("R-LOC-") {
        "工作地点"
    } else if id.starts_with("R-HOT-") || id.starts_with("R-LIVE-") {
        "特殊作业要求"
    } else if id.starts_with("CMP-MISSING") || id.starts_with("MATCH-LOW") {
        "安全措施（对照典票）"
    } else {
        "其他检查"
    }
}

fn ticket_json(review: &Review) -> Value {
    let ticket = &review.ticket;
    let leader = ticket.people.iter().find(|person| person.role == "工作负责人").map(|person| &person.name);
    json!({
        "number": ticket.number, "type": ticket.ticket_type.as_str(),
        "plan_start": ticket.plan_start, "plan_end": ticket.plan_end,
        "leader": leader, "crew_size": ticket.crew_size, "work_tasks": ticket.work_tasks,
    })
}

fn finding_json(finding: &Finding) -> Value {
    json!({
        "rule_id": finding.rule_id, "severity": finding.severity.as_str(),
        "label": label(finding.severity), "group": group(&finding.rule_id),
        "title": finding.title, "message": finding.message, "ticket_says": finding.ticket_says,
        "typical_text": finding.typical_text,
        "basis": {"doc": finding.basis.doc, "clause": finding.basis.clause,
                  "text": finding.basis.text, "source": finding.basis.source},
        "unreviewed": finding.unreviewed, "picked": finding.severity != Severity::Info,
        "draft_line": draft_item(finding), "evidence": finding.evidence,
        "confidence": finding.confidence, "scenario_id": finding.scenario_id,
    })
}

fn verdict(findings: &[Finding]) -> Value {
    let issues = findings.iter().filter(|finding| finding.severity != Severity::Info).count();
    let (level, headline, detail) = if issues > 0 {
        ("issues", format!("发现 {issues} 处疑点，请逐条确认"), "确认属实的事项，勾选后会写进下方的打回意见草稿。")
    } else if findings.iter().any(|finding| finding.rule_id == "MATCH-LOW") {
        (
            "no_scenario",
            "没有找到对应的典票场景，安全措施没有做比对".to_string(),
            "这张票的安全措施需要完全由人工审核。其余项目的检查结果见下方清单。",
        )
    } else if !findings.is_empty() {
        (
            "notes",
            format!("机器没有发现疑点，有 {} 条提示请过目", findings.len()),
            "提示不一定是问题。机器只查了下方清单里的项目，其余请人工审核。",
        )
    } else {
        ("clear", "机器检查的项目没有发现疑点".to_string(), "机器只查了下方清单里的项目，其余请人工审核。")
    };
    json!({"level": level, "headline": headline, "detail": detail})
}

fn candidate_json(id: &str, score: f64, templates: &[Template]) -> Value {
    let template = templates.iter().find(|template| template.id == id).expect("候选场景来自模板列表");
    let similarity = if score >= HIGH_SIMILARITY {
        "高"
    } else if score >= LOW_SIMILARITY {
        "中"
    } else {
        "低"
    };
    let rounded: f64 = format!("{score:.4}").parse().expect("四位小数字符串一定能解析回浮点数");
    json!({"label": format!("典票场景 {}「{}」", scenario_number(id), template.title),
           "similarity": similarity, "score": rounded})
}

fn scenario_json(review: &Review, templates: &[Template]) -> Value {
    let (id, score) = &review.candidates[0];
    let first = candidate_json(id, *score, templates);
    let matched = !review.findings.iter().any(|finding| finding.rule_id == "MATCH-LOW");
    let caution = if matched && first["similarity"] == "低" {
        "相似度低，对照的场景可能不对，措施比对的结果仅供参考。"
    } else {
        ""
    };
    let others: Vec<Value> =
        review.candidates.iter().skip(1).take(2).map(|(id, score)| candidate_json(id, *score, templates)).collect();
    json!({"matched": matched, "label": first["label"], "similarity": first["similarity"],
           "score": first["score"], "caution": caution, "others": others})
}

fn checklist(review: &Review, scenario: &Value) -> Vec<Value> {
    let mut rows = Vec::new();
    for name in ["票面必填与签名", "人员与人数", "时间顺序", "工作地点", "特殊作业要求", "其他检查"]
    {
        let checked = review.checked_rules.iter().filter(|id| group(id) == name).count();
        if checked == 0 {
            continue;
        }
        let count = review.findings.iter().filter(|finding| group(&finding.rule_id) == name).count();
        let (status, detail) = if count > 0 {
            ("found", format!("有 {count} 条需要确认，见下方"))
        } else {
            ("checked", format!("检查了 {checked} 项，没有发现疑点"))
        };
        rows.push(json!({"name": name, "status": status, "detail": detail}));
    }
    let missing = review.findings.iter().filter(|finding| finding.rule_id == "CMP-MISSING").count();
    let label = text(scenario, "label");
    let (status, detail) = if scenario["matched"] == false {
        ("skipped", "没有找到足够相似的典票场景，没有做比对。请人工审核全部安全措施".to_string())
    } else if missing > 0 {
        ("found", format!("对照{label}，有 {missing} 类措施没有找到，见下方"))
    } else {
        ("checked", format!("对照{label}，各类必备措施都找到了"))
    };
    rows.push(json!({"name": "安全措施（对照典票）", "status": status, "detail": detail}));
    rows.push(json!({"name": "机器查不了的项目", "status": "skipped", "detail": MANUAL_ITEMS}));
    rows
}

pub fn build_report(review: &Review, templates: &[Template], source: &str) -> Value {
    let scenario = scenario_json(review, templates);
    let findings: Vec<Value> = review.findings.iter().map(finding_json).collect();
    json!({
        "title": TITLE, "source": source, "disclaimer": DISCLAIMER, "ticket": ticket_json(review),
        "verdict": verdict(&review.findings), "checklist": checklist(review, &scenario), "findings": findings,
        "scenario": scenario, "draft": draft_text(&review.findings), "limits": LIMITS,
    })
}

/// 与 Python `html.escape`（quote=True）一致：& < > " ' 五个字符。
fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#x27;"),
            other => out.push(other),
        }
    }
    out
}

/// 与 Python `json.dumps(text)` 一致：非 ASCII 转成 `\uXXXX`，这样页面在 IE11 下也稳。
fn json_ascii(text: &str) -> String {
    let mut out = String::from("\"");
    for ch in text.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            ch if (ch as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", ch as u32)),
            ch if (ch as u32) < 0x7f => out.push(ch),
            ch => {
                let code = ch as u32;
                if code > 0xFFFF {
                    let adjusted = code - 0x10000;
                    let high = 0xD800 + (adjusted >> 10);
                    let low = 0xDC00 + (adjusted & 0x3FF);
                    out.push_str(&format!("\\u{high:04x}\\u{low:04x}"));
                } else {
                    out.push_str(&format!("\\u{code:04x}"));
                }
            }
        }
    }
    out.push('"');
    out
}

/// 报告 JSON 由本模块自己组装，这里的字段一定存在且是字符串；取不到就是组装代码写错了。
fn text<'a>(value: &'a Value, key: &str) -> &'a str {
    value[key].as_str().expect("报告字段应为字符串")
}

fn map_join(value: &Value, join: &str, mut item: impl FnMut(&Value) -> String) -> String {
    value.as_array().expect("报告字段应为数组").iter().map(&mut item).collect::<Vec<_>>().join(join)
}

fn filled(value: &Value) -> String {
    match value {
        Value::Null => "未填写".to_string(),
        Value::String(value) if value.is_empty() => "未填写".to_string(),
        Value::String(value) => escape(value),
        Value::Number(value) => value.to_string(),
        _ => unreachable!("票面概况字段只有文字、数字或空值"),
    }
}

fn finding_card(finding: &Value) -> String {
    let e = |key: &str| escape(text(finding, key));
    let basis = &finding["basis"];
    let source = if text(basis, "source").is_empty() {
        String::new()
    } else {
        format!("（出处：{}）", escape(text(basis, "source")))
    };
    let checked = if finding["picked"] == true { " checked" } else { "" };
    let ticket = if text(finding, "ticket_says").is_empty() {
        String::new()
    } else {
        format!("<p>票面上：{}</p>", e("ticket_says"))
    };
    let typical = if text(finding, "typical_text").is_empty() {
        String::new()
    } else {
        format!(
            "<p>典票上的写法：{}<br><small>（这是典票里同类作业的写法，线路和设备名称会和本票不同）</small></p>",
            e("typical_text")
        )
    };
    let unreviewed = if finding["unreviewed"] == true {
        "<p class=\"secondary\">这条规则还没有经过业务人员核对，暂时只作为提示。</p>"
    } else {
        ""
    };
    format!(
        "<div class=\"finding finding-{}\"><label class=\"pick-label\"><input type=\"checkbox\" class=\"pick\" data-text=\"{}\" onchange=\"updateDraft()\"{checked}> 写进打回意见</label>\
         <div><span class=\"tag\">{}</span> <span class=\"tag\">{}</span></div><h3>{}</h3><p>{}</p>{ticket}{typical}\
         <p class=\"basis\">依据：{} {}：{}{source}</p>{unreviewed}</div>",
        e("severity"), e("draft_line"), e("label"), e("group"), e("title"), e("message"),
        escape(text(basis, "doc")), escape(text(basis, "clause")), escape(text(basis, "text")),
    )
}

fn scenario_html(scenario: &Value) -> String {
    let prefix = if scenario["matched"] == true {
        "本票对照的是"
    } else {
        "没有找到足够相似的典票场景。最接近的是"
    };
    let mut html = format!(
        "<p>{prefix} {}（相似度：{}）。</p>",
        escape(text(scenario, "label")),
        escape(text(scenario, "similarity"))
    );
    if !text(scenario, "caution").is_empty() {
        html.push_str(&format!("<p>{}</p>", escape(text(scenario, "caution"))));
    }
    let others = map_join(&scenario["others"], "", |other| {
        format!("<li>{}（相似度：{}）</li>", escape(text(other, "label")), escape(text(other, "similarity")))
    });
    if !others.is_empty() {
        html.push_str(&format!("<p>其他相近的场景：</p><ul>{others}</ul><p class=\"secondary\">如果你认为这张票属于别的场景，上面“安全措施”一项的比对结果不适用。</p>"));
    }
    html
}

/// 内联样式和 ES5 脚本，不包含机器可读的内部标识。
pub fn render_html(report: &Value) -> String {
    let title = escape(text(report, "title"));
    let ticket = &report["ticket"];
    let tasks = map_join(&ticket["work_tasks"], "", |task| match (text(task, "地点"), text(task, "任务")) {
        ("", "") => String::new(),
        ("", content) | (content, "") => format!("<li>{}</li>", escape(content)),
        (place, task) => format!("<li>{}：{}</li>", escape(place), escape(task)),
    });
    let rows = map_join(&report["checklist"], "", |row| {
        let status = text(row, "status");
        let words = match status {
            "checked" => "✓ 已查，无疑点",
            "found" => "! 需要确认",
            "skipped" => "— 机器没查",
            _ => unreachable!("清单状态由报告模型确定"),
        };
        format!(
            "<tr class=\"check-{}\"><td>{words}</td><td>{}</td><td>{}</td></tr>",
            escape(status),
            escape(text(row, "name")),
            escape(text(row, "detail"))
        )
    });
    let mut findings = map_join(&report["findings"], "", finding_card);
    if findings.is_empty() {
        findings.push_str("<p>没有需要确认的事项。</p>");
    }
    let limits = map_join(&report["limits"], "", |line| {
        format!("<li>{}</li>", escape(line.as_str().expect("局限说明是字符串数组")))
    });
    let verdict = &report["verdict"];
    let level = escape(text(verdict, "level"));
    format!(
        r#"<!doctype html>
<html lang="zh-CN"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1"><title>{title}</title>
<style>
* {{box-sizing: border-box;}}
body {{margin: 0; background: #f5f7fa; color: #1f2933; font: 16px/1.7 "Microsoft YaHei", "PingFang SC", sans-serif;}}
main {{display: block; max-width: 60rem; margin: 2rem auto; padding: 0 1rem;}}
section, .verdict {{background: #fff; border: 1px solid #d9e2ec; padding: 1.5rem; margin-bottom: 1.25rem; overflow-wrap: break-word; word-wrap: break-word;}}
h1, h2, h3, p {{margin: 0 0 .75rem;}}
h1 {{font-size: 1rem; color: #52606d; font-weight: normal;}}
h2 {{font-size: 1.25rem;}}
.number {{font-size: 1.8rem; font-weight: bold;}}
.secondary, small, .basis {{color: #52606d;}}
.basis, small {{font-size: .875rem;}}
.verdict {{border-left-width: 6px;}}
.headline {{font-size: 1.5rem; font-weight: bold;}}
.verdict-issues {{border-left-color: #b45309; background: #fff7ed;}}
.verdict-no_scenario {{border-left-color: #52606d; background: #f0f4f8;}}
.verdict-notes {{border-left-color: #1d4ed8; background: #eff6ff;}}
.verdict-clear {{border-left-color: #15803d; background: #f0fdf4;}}
table {{border-collapse: collapse; width: 100%; table-layout: fixed;}}
th, td {{border: 1px solid #d9e2ec; padding: .6rem; text-align: left; vertical-align: top; word-wrap: break-word;}}
th:first-child, td:first-child {{width: 10rem; white-space: nowrap;}}
.check-checked td:first-child {{color: #15803d;}}
.check-found td:first-child {{color: #b45309;}}
.check-skipped td:first-child {{color: #52606d;}}
.finding {{border: 1px solid #d9e2ec; border-left: 4px solid #b45309; padding: 1rem; margin: 1rem 0;}}
.finding-info {{border-left-color: #1d4ed8;}}
.finding h3 {{font-size: 1.125rem; font-weight: bold; margin-top: .5rem;}}
.tag {{display: inline-block; border: 1px solid #d9e2ec; padding: .1rem .4rem; font-size: .8rem; color: #52606d;}}
.pick-label {{display: block; padding: .5rem 0; cursor: pointer;}}
.pick {{width: 1.25rem; height: 1.25rem; vertical-align: middle;}}
textarea {{width: 100%; min-height: 12rem; font: inherit; padding: .75rem; border: 1px solid #d9e2ec;}}
button {{font: inherit; padding: .5rem 1rem; margin: .5rem .5rem 0 0; border: 1px solid #52606d; background: #fff; color: #1f2933; cursor: pointer;}}
ul {{padding-left: 1.5rem;}}
@media print {{body {{background: #fff;}} main {{max-width: none; margin: 0; padding: 0;}} button {{display: none;}} .finding, section, .verdict {{page-break-inside: avoid;}}}}
@media (max-width: 640px) {{main {{margin: .75rem auto; padding: 0 .5rem;}} section, .verdict {{padding: .75rem;}} .finding {{padding: .6rem;}} th, td {{padding: .35rem;}} th:first-child, td:first-child {{width: 8rem; font-size: .8rem;}}}}
</style></head><body><main>
<section id="ticket"><h1>{title}</h1><p class="number">票号：{number}</p><p>票种：{ticket_type}</p><p>计划工作时间：{plan_start} 至 {plan_end}<br>工作负责人：{leader}　总人数：{crew_size}</p><ul>{tasks}</ul></section>
<div id="verdict" class="verdict verdict-{level}" data-level="{level}"><p class="headline">{headline}</p><p>{detail}</p></div>
<section id="checklist"><h2>机器查了什么</h2><table><thead><tr><th>状态</th><th>检查项</th><th>说明</th></tr></thead><tbody>{rows}</tbody></table></section>
<section id="findings"><h2>需要确认的事项</h2>{findings}</section>
<section id="draft-section"><h2>打回意见草稿</h2><p>草稿由上面勾选的事项拼成，可以直接修改。是否打回由你决定。</p><textarea id="draft" aria-label="打回意见草稿">{draft}</textarea><button type="button" onclick="copyDraft()">复制草稿</button><button type="button" onclick="window.print()">打印报告</button></section>
<section id="scenario"><h2>对照的典票场景</h2>{scenario}</section>
<section id="limits"><h2>机器的边界</h2><p>{disclaimer}</p><ul>{limits}</ul><p class="secondary">报告对应的文件：{source}</p></section>
</main><script>
function updateDraft() {{
    var picks = document.querySelectorAll('input.pick');
    var lines = [{draft_head}];
    for (var i = 0; i < picks.length; i++) {{
        if (picks[i].checked) {{
            lines.push(lines.length + '. ' + picks[i].getAttribute('data-text'));
        }}
    }}
    document.getElementById('draft').value = lines.length > 1 ? lines.join('\n') : '';
}}
function copyDraft() {{
    document.getElementById('draft').select();
    document.execCommand('copy');
}}
</script></body></html>"#,
        number = filled(&ticket["number"]),
        ticket_type = filled(&ticket["type"]),
        plan_start = filled(&ticket["plan_start"]).replace('T', " "),
        plan_end = filled(&ticket["plan_end"]).replace('T', " "),
        leader = filled(&ticket["leader"]),
        crew_size = filled(&ticket["crew_size"]),
        headline = escape(text(verdict, "headline")),
        detail = escape(text(verdict, "detail")),
        draft = escape(text(report, "draft")),
        scenario = scenario_html(&report["scenario"]),
        disclaimer = escape(text(report, "disclaimer")),
        source = escape(text(report, "source")),
        draft_head = json_ascii(DRAFT_HEAD),
    )
}
