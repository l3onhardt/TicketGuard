from __future__ import annotations

import json
import re
from datetime import datetime
from pathlib import Path
from typing import Callable

from ticketguard.entities import lines
from ticketguard.schema import Basis, Finding, Severity, Ticket


def values(ticket: Ticket, path: str) -> list:
    prefix, _, key = path.partition(".")
    if prefix == "fields":
        found = [ticket.fields.get(key)]
    elif prefix == "work_tasks":
        found = [task.get(key) for task in ticket.work_tasks]
    elif prefix == "people":
        found = [getattr(person, key) for person in ticket.people]
    else:
        found = [getattr(ticket, path)]
    return [value for value in found if value is not None and value != ""]


def _required_field(ticket: Ticket, params: dict) -> list[dict]:
    return [{"field": label} for path, label in params["fields"].items() if not values(ticket, path)]


def _role_required(ticket: Ticket, params: dict) -> list[dict]:
    present = {person.role for person in ticket.people}
    return [{"role": role} for role in params["roles"] if role not in present]


def _roles_disjoint(ticket: Ticket, params: dict) -> list[dict]:
    a, b = params["roles"]
    shared = ({p.name for p in ticket.people if p.role == a}
              & {p.name for p in ticket.people if p.role == b})
    return [{"name": name, "roles": "、".join(params["roles"])} for name in sorted(shared)]


def _before(ticket: Ticket, params: dict) -> list[dict]:
    chain = [(path, found[0]) for path in params["chain"] if (found := values(ticket, path))]
    return [{"a": a.rsplit(".", 1)[-1], "b": b.rsplit(".", 1)[-1],
             "a_time": at, "b_time": bt}
            for (a, at), (b, bt) in zip(chain, chain[1:])
            if at > bt or (params["strict"] and at == bt)]


def _duration_max(ticket: Ticket, params: dict) -> list[dict]:
    start, end = values(ticket, params["start"]), values(ticket, params["end"])
    if start and end:
        hours = (datetime.fromisoformat(end[0]) - datetime.fromisoformat(start[0])).total_seconds() / 3600
        if hours > params["hours"]:
            return [{"actual_hours": f"{hours:g}", "hours": params["hours"]}]
    return []


def _crew_count(ticket: Ticket, params: dict) -> list[dict]:
    members = [person for person in ticket.people if person.role == "工作班成员"]
    groups = [int(match[1]) for person in members
              if (match := re.search(r"(\d+)\s*人", person.note or ""))]
    expected = sum(groups) if groups else 1 + len(members)
    if ticket.crew_size is not None and ticket.crew_size != expected:
        return [{"actual": ticket.crew_size, "expected": expected}]
    return []


def _regex(ticket: Ticket, params: dict) -> list[dict]:
    return [{"value": value, "evidence": value} for value in values(ticket, params["field"])
            if bool(re.search(params["pattern"], value)) != params["must_match"]]


def _lines_subset(ticket: Ticket, params: dict) -> list[dict]:
    source = values(ticket, params["field"])
    within = {line for path in params["within"] for value in values(ticket, path) for line in lines(value)}
    extra = {line for value in source for line in lines(value)} - within
    return [{"lines": "、".join(sorted(extra)), "evidence": "；".join(source)}] if extra else []


CHECKS: dict[str, Callable[[Ticket, dict], list[dict]]] = {
    "required_field": _required_field,
    "role_required": _role_required,
    "roles_disjoint": _roles_disjoint,
    "before": _before,
    "duration_max": _duration_max,
    "crew_count": _crew_count,
    "regex": _regex,
    "lines_subset": _lines_subset,
}


def load_rules(directory: str | Path) -> list[dict]:
    required = {"id", "title", "ticket_types", "check", "params", "severity", "message", "basis", "status"}
    rules = []
    for path in sorted(Path(directory).glob("*.rules.json")):
        for rule in json.loads(path.read_text(encoding="utf-8")):
            label = f"{path}：规则 {rule.get('id', '（缺少 id）')}"
            missing = required - rule.keys()
            if missing:
                raise ValueError(f"{label} 缺少字段：{'、'.join(sorted(missing))}")
            if set(rule["basis"]) != {"doc", "clause", "text", "source"}:
                raise ValueError(f"{label} 的 basis 必须恰好含 doc、clause、text、source")
            if rule["check"] not in CHECKS:
                raise ValueError(f"{label} 的 check 未注册：{rule['check']}")
            if rule["severity"] not in {severity.value for severity in Severity}:
                raise ValueError(f"{label} 的 severity 必须为 error、warning 或 info")
            if rule["status"] not in {"draft", "reviewed", "active"}:
                raise ValueError(f"{label} 的 status 必须为 draft、reviewed 或 active")
            rules.append(rule)
    return rules


def run_rules(ticket: Ticket, rules: list[dict]) -> list[Finding]:
    findings = []
    for rule in rules:
        if ticket.ticket_type.value not in rule["ticket_types"]:
            continue
        severity = Severity.INFO if rule["status"] == "draft" else Severity(rule["severity"])
        for violation in CHECKS[rule["check"]](ticket, rule["params"]):
            findings.append(Finding(
                rule_id=rule["id"], severity=severity, message=rule["message"].format(**violation),
                basis=Basis(**rule["basis"]), evidence=violation.get("evidence", ""),
                confidence=1.0, scenario_id=None, needs_review=severity != Severity.ERROR,
            ))
    return findings
