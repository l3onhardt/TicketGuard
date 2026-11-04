from __future__ import annotations

import re
from dataclasses import dataclass

from .entities import abstract, keywords, normalize
from .ingest.ticket_parser import parse_dist1
from .rules.engine import run_rules
from .schema import Basis, Finding, Severity, Ticket

MATCH_MIN = 0.4
CMP_BASIS: Basis = Basis(
    doc="《中国南方电网有限责任公司电力安全工作规程第3部分：配电》（典票库第一章转述）",
    clause="5.3.1",
    text="工作票所列安全措施正确、完备，由工作负责人、签发人、会签人、工作票接收人、许可人负责。",
    source="2026.9典票库 第一章“两票”相关文件内容说明",
)


@dataclass
class Review:
    ticket: Ticket
    candidates: list[tuple[str, float]]
    findings: list[Finding]


def _jaccard(a: set, b: set) -> float:
    return len(a & b) / len(a | b) if a or b else 0.0


def match_scenario(ticket: Ticket, templates: list[dict], lexicon: dict) -> list[tuple[str, float]]:
    def bigrams(text: str) -> set[str]:
        text = re.sub(r"\{[^{}]*\}", "", normalize(abstract(text), lexicon))
        return {text[i:i + 2] for i in range(len(text) - 1)}

    text = bigrams("".join(task["地点"] + task["任务"] for task in ticket.work_tasks))
    signature = {(m.category, word) for m in ticket.measures for word in keywords(m.text, lexicon)}
    candidates = []
    for template in templates:
        other_text = bigrams(template["title"] + template["work_task"])
        other_signature = {(rm["category"], word) for rm in template["required_measures"]
                           for word in rm["keywords"]}
        score = 0.5 * _jaccard(text, other_text) + 0.5 * _jaccard(signature, other_signature)
        candidates.append((template["id"], score))
    return sorted(candidates, key=lambda item: (-item[1], item[0]))


def compare_measures(ticket: Ticket, template: dict, lexicon: dict, confidence: float) -> list[Finding]:
    measures = [(m.category, set(keywords(m.text, lexicon))) for m in ticket.measures]
    findings = []
    for rm in template["required_measures"]:
        if any(category == rm["category"] and set(rm["keywords"]) <= words
               for category, words in measures):
            continue
        findings.append(Finding(
            rule_id="CMP-MISSING", severity=Severity.WARNING if rm["must"] else Severity.INFO,
            message=f"对照典票场景 {template['id']}「{template['title']}」（匹配度 {confidence:.2f}），"
                    f"票面“{rm['category']}”栏中没有找到这类措施：{rm['pattern']}",
            basis=CMP_BASIS, evidence=rm["id"], confidence=confidence,
            needs_review=True, scenario_id=template["id"],
        ))
    return findings


def review_text(text: str, rules: list[dict], templates: list[dict], lexicon: dict) -> Review:
    ticket = parse_dist1(text)
    findings = run_rules(ticket, rules)
    candidates = match_scenario(ticket, templates, lexicon)
    scenario_id, confidence = candidates[0]
    if confidence >= MATCH_MIN:
        template = next(template for template in templates if template["id"] == scenario_id)
        findings.extend(compare_measures(ticket, template, lexicon, confidence))
    else:
        findings.append(Finding(
            "MATCH-LOW", Severity.INFO,
            f"没有找到足够相似的典票场景（最接近的是 {scenario_id}，匹配度 {confidence:.2f}），本次未做安全措施比对。",
            CMP_BASIS, confidence=confidence, needs_review=True,
        ))
    order = {Severity.ERROR: 0, Severity.WARNING: 1, Severity.INFO: 2}
    findings.sort(key=lambda finding: order[finding.severity])
    return Review(ticket, candidates[:3], findings)
