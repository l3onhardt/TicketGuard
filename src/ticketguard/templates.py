from __future__ import annotations

import json
import re
from pathlib import Path

from .entities import abstract, keywords
from .ingest.scenarios import ScenarioChunk
from .schema import Ticket


def required_measures(ticket: Ticket, lexicon: dict, prefix: str) -> list[dict]:
    signatures: list[tuple[str, list[str], str]] = []
    for measure in ticket.measures:
        kw = keywords(measure.text, lexicon)
        if not kw or any(category == measure.category and set(words) >= set(kw)
                         for category, words, text in signatures):
            continue
        signatures = [(category, words, text) for category, words, text in signatures
                      if not (category == measure.category and set(words) < set(kw))]
        signatures.append((measure.category, kw, measure.text))
    return [{"id": f"{prefix}-M{index:02d}", "category": category,
             "pattern": abstract(text), "keywords": kw,
             "must": re.search(r"则无需|则不用|无需填写|不用写", text) is None}
            for index, (category, kw, text) in enumerate(signatures, 1)]


def build_template(chunk: ScenarioChunk, ticket: Ticket, lexicon: dict) -> dict:
    template_id = f"D1-S{int(chunk.number):02d}"
    suffix = re.search(r"[（(](有调整|新增)[）)]\s*$", chunk.title)
    title = chunk.title[:suffix.start()] if suffix else chunk.title
    task = ticket.work_tasks[0]["任务"] if ticket.work_tasks else ""
    lines = chunk.text.splitlines()
    for index, line in enumerate(lines):
        if line.lstrip().startswith("编号："):
            break
        if "工作任务：" in line:
            task = line.split("工作任务：", 1)[1].strip()
            if not task:
                task = lines[index + 1].strip() if index + 1 < len(lines) else ""
            break
    return {
        "id": template_id,
        "ticket_type": ticket.ticket_type.value,
        "title": title.strip(),
        "source": {"doc": "2026.9典票库", "section": chunk.section,
                   "scenario": chunk.number, "status": suffix[1] if suffix else ""},
        "work_task": abstract(task),
        "required_measures": required_measures(ticket, lexicon, template_id),
        "status": "draft",
    }


def load_templates(directory: str | Path) -> list[dict]:
    templates = []
    for path in sorted(Path(directory).glob("*.json")):
        template = json.loads(path.read_text(encoding="utf-8"))
        missing = [field for field in ("id", "ticket_type", "title", "work_task", "required_measures")
                   if field not in template]
        if missing:
            raise ValueError(f"{path}：缺少字段 {', '.join(missing)}")
        templates.append(template)
    return templates
