from __future__ import annotations

import json
from dataclasses import asdict
from html import escape

from .review import Review
from .schema import Finding, Severity

TITLE: str = "工作票智能预审报告"
DISCLAIMER: str = "本报告是智能预审结果，所列均为“疑似问题”和建议，需审票人逐条人工确认；是否接收或打回，由审票人决定。"
LIMITS: list[str] = [
    "不判断现场物理条件（如实际安全距离、所拉开关能否真正隔离作业区），这些需要结合配网拓扑和现场勘察。",
    "本次只使用规则基线（字面和同义词比对），未启用语义判断：措施换了说法、或规则没有覆盖到的问题，可能漏报或误报。",
    "目前规则库仍是草稿，尚未经业务人员逐条核对，规则类发现一律只作为“提示”。",
]
LABELS: dict[Severity, str] = {Severity.ERROR: "疑似违反条款", Severity.WARNING: "疑似问题", Severity.INFO: "提示"}
DRAFT_HEAD: str = "预审意见（草稿，请核实后使用）："


def _draft_item(message: str, doc: str, clause: str) -> str:
    return f"{message}（依据：{doc} {clause}）"


def draft_text(findings: list[Finding]) -> str:
    selected = [f for f in findings if f.severity != Severity.INFO]
    lines = [f"{i}. {_draft_item(f.message, f.basis.doc, f.basis.clause)}" for i, f in enumerate(selected, 1)]
    return "\n".join([DRAFT_HEAD, *lines]) if lines else ""


def build_report(review: Review, templates: list[dict], source: str) -> dict:
    ticket = review.ticket
    titles = {t["id"]: t["title"] for t in templates}
    return {
        "title": TITLE, "source": source, "disclaimer": DISCLAIMER, "limits": LIMITS,
        "ticket": {"number": ticket.number, "type": ticket.ticket_type.value, "plan_start": ticket.plan_start,
                   "plan_end": ticket.plan_end, "work_tasks": ticket.work_tasks},
        "candidates": [{"id": i, "title": titles[i], "score": round(score, 4)} for i, score in review.candidates],
        "findings": [dict(asdict(f), severity=f.severity.value, label=LABELS[f.severity]) for f in review.findings],
        "draft": draft_text(review.findings),
    }


def _finding_row(finding: dict) -> str:
    b = finding["basis"]
    source = f"（出处：{escape(b['source'])}）" if b["source"] else ""
    checked = " checked" if finding["severity"] != Severity.INFO.value else ""
    item = escape(_draft_item(finding["message"], b["doc"], b["clause"]))
    return (f'<tr><td><input type="checkbox" class="pick" data-text="{item}" onchange="updateDraft()"{checked}></td>'
            f"<td>{escape(finding['label'])}</td><td>{escape(finding['message'])}</td>"
            f"<td>{escape(b['doc'])} {escape(b['clause'])}：{escape(b['text'])}{source}</td></tr>")


def render_html(report: dict) -> str:
    title, t = escape(report["title"]), report["ticket"]
    tasks = "".join(f"<li>{escape(w['地点'])}：{escape(w['任务'])}</li>" for w in t["work_tasks"])
    candidates = "".join(f"<tr><td>{escape(c['id'])}</td><td>{escape(c['title'])}</td><td>{c['score']:.2f}</td></tr>"
                         for c in report["candidates"])
    findings = "".join(_finding_row(f) for f in report["findings"])
    limits = "".join(f"<li>{escape(line)}</li>" for line in report["limits"])
    return f'''<!doctype html>
<html lang="zh-CN"><head><meta charset="utf-8"><title>{title}</title>
<style>
body {{font: 1rem/1.6 sans-serif; max-width: 70rem; margin: 2rem auto; padding: 0 1rem;}}
table {{border-collapse: collapse; width: 100%;}}
th, td {{border: 1px solid #aaa; padding: .4rem; text-align: left; vertical-align: top;}}
textarea {{box-sizing: border-box; width: 100%; min-height: 10rem;}}
</style></head><body>
<h1>{title}</h1><p>{escape(report['disclaimer'])}</p>
<h2>票面概况</h2><p>编号：{escape(t['number'] or '未填写')}　票种：{escape(t['type'])}<br>
计划工作时间：{escape(t['plan_start'] or '未填写')} 至 {escape(t['plan_end'] or '未填写')}</p><ul>{tasks}</ul>
<h2>候选场景</h2><table><thead><tr><th>场景编号</th><th>名称</th><th>匹配度</th></tr></thead>
<tbody>{candidates}</tbody></table>
<h2>疑似问题</h2><table><thead><tr><th>选入草稿</th><th>类别</th><th>说明</th><th>依据</th></tr></thead>
<tbody>{findings}</tbody></table>
<h2>打回意见草稿</h2><textarea id="draft" aria-label="打回意见草稿">{escape(report['draft'])}</textarea>
<button type="button" onclick="copyDraft()">复制草稿</button>
<h2>局限说明</h2><ul>{limits}</ul>
<script>
function updateDraft() {{
    var picks = document.querySelectorAll('input.pick');
    var lines = [{json.dumps(DRAFT_HEAD)}];
    for (var i = 0; i < picks.length; i++) {{
        if (picks[i].checked) {{
            lines.push(lines.length + '. ' + picks[i].getAttribute('data-text'));
        }}
    }}
    document.getElementById('draft').value = lines.length > 1 ? lines.join('\\n') : '';
}}
function copyDraft() {{
    document.getElementById('draft').select();
    document.execCommand('copy');
}}
</script></body></html>'''
