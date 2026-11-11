//! HTML 预审报告与打回意见草稿。移植自参考实现 `src/ticketguard/report.py`。
//!
//! HTML 页面必须与参考输出逐字节一致（对拍测试会比对），所以这里的模板和转义规则
//! 都照着参考实现写：内联 CSS/JS、ES5 语法、`html.escape` 同款转义。

use crate::review::Review;
use crate::schema::{Finding, Severity};
use crate::templates::Template;
use serde_json::{json, Map, Value};

pub const TITLE: &str = "工作票智能预审报告";
pub const DISCLAIMER: &str =
    "本报告是智能预审结果，所列均为“疑似问题”和建议，需审票人逐条人工确认；是否接收或打回，由审票人决定。";
pub const DRAFT_HEAD: &str = "预审意见（草稿，请核实后使用）：";

pub const LIMITS: [&str; 3] = [
    "不判断现场物理条件（如实际安全距离、所拉开关能否真正隔离作业区），这些需要结合配网拓扑和现场勘察。",
    "本次只使用规则基线（字面和同义词比对），未启用语义判断：措施换了说法、或规则没有覆盖到的问题，可能漏报或误报。",
    "目前规则库仍是草稿，尚未经业务人员逐条核对，规则类发现一律只作为“提示”。",
];

/// 严重度 → 中文标签。
pub fn label(severity: Severity) -> &'static str {
    match severity {
        Severity::Error => "疑似违反条款",
        Severity::Warning => "疑似问题",
        Severity::Info => "提示",
    }
}

fn draft_item(finding: &Finding) -> String {
    format!("{}（依据：{} {}）", finding.message, finding.basis.doc, finding.basis.clause)
}

/// 打回意见草稿：只取 warning 及以上，一条一行。
pub fn draft_text(findings: &[Finding]) -> String {
    let lines: Vec<String> = findings
        .iter()
        .enumerate()
        .filter(|(_, finding)| finding.severity != Severity::Info)
        .map(|(index, finding)| format!("{}. {}", index + 1, draft_item(finding)))
        .collect();
    if lines.is_empty() {
        return String::new();
    }
    format!("{DRAFT_HEAD}\n{}", lines.join("\n"))
}

fn round4(value: f64) -> f64 {
    format!("{value:.4}").parse().expect("四位小数字符串一定能解析回浮点数")
}

fn ticket_json(review: &Review) -> Value {
    let work_tasks: Vec<Value> =
        review.ticket.work_tasks.iter().map(|task| json!({ "地点": task.place, "任务": task.task })).collect();
    // 键顺序必须与参考实现一致：number、type、plan_start、plan_end、work_tasks
    let mut map = Map::new();
    map.insert("number".to_string(), review.ticket.number.clone().map(Value::String).unwrap_or(Value::Null));
    map.insert("type".to_string(), Value::String(review.ticket.ticket_type.as_str().to_string()));
    map.insert("plan_start".to_string(), review.ticket.plan_start.clone().map(Value::String).unwrap_or(Value::Null));
    map.insert("plan_end".to_string(), review.ticket.plan_end.clone().map(Value::String).unwrap_or(Value::Null));
    map.insert("work_tasks".to_string(), Value::Array(work_tasks));
    Value::Object(map)
}

fn finding_json(finding: &Finding) -> Value {
    let mut map = Map::new();
    map.insert("rule_id".to_string(), Value::String(finding.rule_id.clone()));
    map.insert("severity".to_string(), Value::String(finding.severity.as_str().to_string()));
    map.insert("message".to_string(), Value::String(finding.message.clone()));
    map.insert(
        "basis".to_string(),
        json!({
            "doc": finding.basis.doc,
            "clause": finding.basis.clause,
            "text": finding.basis.text,
            "source": finding.basis.source,
        }),
    );
    map.insert("evidence".to_string(), Value::String(finding.evidence.clone()));
    map.insert("confidence".to_string(), json!(finding.confidence));
    map.insert("needs_review".to_string(), Value::Bool(finding.needs_review));
    map.insert("scenario_id".to_string(), finding.scenario_id.clone().map(Value::String).unwrap_or(Value::Null));
    map.insert("label".to_string(), Value::String(label(finding.severity).to_string()));
    Value::Object(map)
}

/// 组装报告 JSON。键顺序与参考实现的 `build_report` 一致。
pub fn build_report(review: &Review, templates: &[Template], source: &str) -> Value {
    let title_of = |id: &str| {
        templates.iter().find(|template| template.id == id).map(|template| template.title.clone()).unwrap_or_default()
    };
    let candidates: Vec<Value> = review
        .candidates
        .iter()
        .map(|(id, score)| json!({ "id": id, "title": title_of(id), "score": round4(*score) }))
        .collect();
    let findings: Vec<Value> = review.findings.iter().map(finding_json).collect();
    let limits: Vec<Value> = LIMITS.iter().map(|line| Value::String((*line).to_string())).collect();
    let mut map = Map::new();
    map.insert("title".to_string(), Value::String(TITLE.to_string()));
    map.insert("source".to_string(), Value::String(source.to_string()));
    map.insert("disclaimer".to_string(), Value::String(DISCLAIMER.to_string()));
    map.insert("limits".to_string(), Value::Array(limits));
    map.insert("ticket".to_string(), ticket_json(review));
    map.insert("candidates".to_string(), Value::Array(candidates));
    map.insert("findings".to_string(), Value::Array(findings));
    map.insert("draft".to_string(), Value::String(draft_text(&review.findings)));
    Value::Object(map)
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

/// 对数组里的每一项拼字符串，再连起来。
fn map_join(value: &Value, join: &str, mut item: impl FnMut(&Value) -> String) -> String {
    match value.as_array() {
        Some(list) => list.iter().map(&mut item).collect::<Vec<_>>().join(join),
        None => String::new(),
    }
}

fn finding_row(finding: &Value) -> String {
    let basis = &finding["basis"];
    let source = match text(basis, "source") {
        "" => String::new(),
        text => format!("（出处：{}）", escape(text)),
    };
    let checked = if finding["severity"].as_str() != Some(Severity::Info.as_str()) { " checked" } else { "" };
    let item =
        escape(&format!("{}（依据：{} {}）", text(finding, "message"), text(basis, "doc"), text(basis, "clause")));
    format!(
        "<tr><td><input type=\"checkbox\" class=\"pick\" data-text=\"{item}\" onchange=\"updateDraft()\"{checked}></td>\
         <td>{}</td><td>{}</td><td>{} {}：{}{}</td></tr>",
        escape(text(finding, "label")),
        escape(text(finding, "message")),
        escape(text(basis, "doc")),
        escape(text(basis, "clause")),
        escape(text(basis, "text")),
        source,
    )
}

/// 渲染完整的 HTML 报告（内联 CSS/JS，ES5，完全离线）。
pub fn render_html(report: &Value) -> String {
    let title = escape(text(report, "title"));
    let ticket = &report["ticket"];
    let tasks = map_join(&ticket["work_tasks"], "", |task| {
        format!("<li>{}：{}</li>", escape(text(task, "地点")), escape(text(task, "任务")))
    });
    let candidates = map_join(&report["candidates"], "", |candidate| {
        format!(
            "<tr><td>{}</td><td>{}</td><td>{:.2}</td></tr>",
            escape(text(candidate, "id")),
            escape(text(candidate, "title")),
            candidate["score"].as_f64().unwrap_or(0.0)
        )
    });
    let findings = map_join(&report["findings"], "", finding_row);
    let limits = map_join(&report["limits"], "", |line| {
        format!("<li>{}</li>", escape(line.as_str().expect("局限说明是字符串数组")))
    });
    // 这三个字段允许为空（票没填），空就写“未填写”
    let filled = |key: &str| match ticket[key].as_str() {
        Some(value) if !value.is_empty() => escape(value),
        _ => "未填写".to_string(),
    };
    let (number, plan_start, plan_end) = (filled("number"), filled("plan_start"), filled("plan_end"));
    format!(
        r#"<!doctype html>
<html lang="zh-CN"><head><meta charset="utf-8"><title>{title}</title>
<style>
body {{font: 1rem/1.6 sans-serif; max-width: 70rem; margin: 2rem auto; padding: 0 1rem;}}
table {{border-collapse: collapse; width: 100%;}}
th, td {{border: 1px solid #aaa; padding: .4rem; text-align: left; vertical-align: top;}}
textarea {{box-sizing: border-box; width: 100%; min-height: 10rem;}}
</style></head><body>
<h1>{title}</h1><p>{disclaimer}</p>
<h2>票面概况</h2><p>编号：{number}　票种：{ticket_type}<br>
计划工作时间：{plan_start} 至 {plan_end}</p><ul>{tasks}</ul>
<h2>候选场景</h2><table><thead><tr><th>场景编号</th><th>名称</th><th>匹配度</th></tr></thead>
<tbody>{candidates}</tbody></table>
<h2>疑似问题</h2><table><thead><tr><th>选入草稿</th><th>类别</th><th>说明</th><th>依据</th></tr></thead>
<tbody>{findings}</tbody></table>
<h2>打回意见草稿</h2><textarea id="draft" aria-label="打回意见草稿">{draft}</textarea>
<button type="button" onclick="copyDraft()">复制草稿</button>
<h2>局限说明</h2><ul>{limits}</ul>
<script>
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
        disclaimer = escape(text(report, "disclaimer")),
        ticket_type = escape(text(ticket, "type")),
        draft = escape(text(report, "draft")),
        draft_head = json_ascii(DRAFT_HEAD),
    )
}
