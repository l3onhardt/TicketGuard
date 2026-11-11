from __future__ import annotations

import re

from ..schema import Person, SafetyMeasure, Ticket, TicketType

_REQUIRED = (
    "工作负责人（监护人）：", "工作地点或设备（注明", "停电设备或线路名称：",
    "调度或设备运维单位负责的安全措施", "工作班组自行装设的接地线",
)
_CATEGORIES = (
    ("应拉", "应拉开关刀闸熔断器"), ("应投切", "应投切电源及二次回路"),
    ("接地", "接地"), ("标志牌", "标志牌"), ("保留或邻近", "保留带电设备"),
)
_SIGNATURES = (
    ("工作票签发人签名：", "签发人", "签发时间"),
    ("工作票会签人签名：", "会签人", "会签时间"),
    ("工作票接收人签名：", "接收人", "接收时间"),
    ("工作许可人签名：", "许可人", "许可时间"),
)
_NUMBER = r"(?:\d+[、.．]|（\d+）|\(\d+\))"
_SPACE = " 　"


def parse_datetime(s: str) -> str | None:
    match = re.search(r"(\d{4})\s*年\s*(\d{1,2})\s*月\s*(\d{1,2})\s*日\s*"
                      r"(\d{1,2})\s*时\s*(\d{1,2})\s*分", s)
    if not match:
        return None
    year, month, day, hour, minute = map(int, match.groups())
    return f"{year:04d}-{month:02d}-{day:02d}T{hour:02d}:{minute:02d}"


def ticket_block(text: str) -> str:
    lines = text.splitlines()
    start = next((i for i, line in enumerate(lines)
                  if line.lstrip(_SPACE).startswith("编号：")), None)
    if start is None:
        raise ValueError("票面缺少编号：行")
    table = next((i for i in range(start + 1, len(lines))
                  if lines[i].strip(_SPACE) == "[TABLE]"), None)
    if table is None:
        raise ValueError("编号：行后缺少 [TABLE]")
    end = next((i for i in range(table + 1, len(lines))
                if lines[i].strip(_SPACE) == "[/TABLE]"), None)
    if end is None:
        raise ValueError("票面缺少 [/TABLE]")
    return "\n".join(lines[start:end + 1])


def _after(lines: list[str], label: str) -> str:
    return next((line.split(label, 1)[1] for line in lines if label in line), "")


def _header(ticket: Ticket, lines: list[str]) -> None:
    header = next(line for line in lines if _REQUIRED[0] in line)
    name = _after([header], _REQUIRED[0]).split("/", 1)[0].strip(_SPACE)
    if name:
        ticket.people.append(Person(name, "工作负责人"))
    for key, label, separator in (("电话", "电话：", "/"), ("单位班组", "单位和班组：", " / ")):
        ticket.fields[key] = _after([header], label).split(separator, 1)[0].strip(_SPACE) or None
    crew = re.search(r"总人数共\s*(\d+)\s*人", header)
    ticket.crew_size = int(crew[1]) if crew else None
    ticket.plan_start = parse_datetime(_after([header], "自"))
    ticket.plan_end = parse_datetime(_after([header], "至"))
    dispatch = re.search(r"[☑√]\s*(是|否)", _after(lines, "是否办理分组工作派工单"))
    ticket.fields["分组派工单"] = dispatch[1] if dispatch else None
    members = _after(lines, "工作班人员（不包括工作负责人）：").strip(_SPACE + "。.；;")
    for member in re.split(r"[、，,；;/\s]+", members):
        if not member:
            continue
        parts = re.fullmatch(r"([^（(]+)[（(]([^）)]*)[）)]", member)
        ticket.people.append(Person(parts[1] if parts else member, "工作班成员",
                                    note=parts[2] if parts else None))


def _signatures(ticket: Ticket, lines: list[str]) -> None:
    for label, role, time_field in _SIGNATURES:
        rest = _after(lines, label)
        name = re.match(r"\s*([^\s/（(：:]+)(?:\s*[（(]([^）)]*?)代签[）)])?", rest)
        if name:
            signed_by = re.sub(r"\s+", "", name[2]) if name[2] is not None else None
            ticket.people.append(Person(name[1], role, signed_by=signed_by))
        ticket.fields[time_field] = parse_datetime(rest)
    monitor = re.search(r"指定[\s/]*([^\s/]+?)[\s/]*为专责监护人", ticket.raw_text)
    if monitor:
        ticket.people.append(Person(monitor[1], "专责监护人"))
    briefing = next((line for line in lines if line.startswith("安全交代 |")), "")
    ticket.fields["安全交代时间"] = parse_datetime(briefing)


def _measures(ticket: Ticket, lines: list[str]) -> None:
    start = next(i for i, line in enumerate(lines) if line.startswith(_REQUIRED[3]))
    end = next(i for i, line in enumerate(lines) if line.startswith(_REQUIRED[4]))
    counts: dict[str, int] = {}
    for line in lines[start:end]:
        label, _, content = line.partition("|")[2].partition("：")
        category = next((category for key, category in _CATEGORIES if key in label), None)
        if category is None:
            raise ValueError(f"安全措施栏目无法识别：{label}")
        for segment in content.split(" / "):
            for item in re.split(rf"(?<=[；;。！!])(?={_NUMBER})", segment):
                item = re.sub(rf"^\s*(?:{_NUMBER})?\s*", "", item).strip(_SPACE + "；;。.")
                if item and item != "无":
                    counts[category] = counts.get(category, 0) + 1
                    ticket.measures.append(SafetyMeasure(category, item, counts[category]))
    for order, cell in enumerate(lines[end].split("|")[2:], 1):
        value = cell.replace(" / ", "").strip(_SPACE)
        if value not in ("", "无", "/"):
            ticket.measures.append(SafetyMeasure("班组自装接地", value, order))


def _work(ticket: Ticket, lines: list[str]) -> None:
    start = next(i for i, line in enumerate(lines) if line.startswith(_REQUIRED[1]))
    end = next(i for i, line in enumerate(lines) if _REQUIRED[2] in line)
    for line in lines[start + 1:end]:
        cells = [cell.strip(_SPACE) for cell in line.split("|")]
        place, task = cells[0], cells[1] if len(cells) > 1 else ""
        if place or task:
            ticket.work_tasks.append({"地点": place, "任务": task})
    ticket.outage_scope = _after(lines, _REQUIRED[2]).strip(_SPACE + "/。.") or None
    remark = next((line for line in lines if line.startswith("备注")), "")
    ticket.fields["备注"] = remark.partition("：")[2].strip(_SPACE + "/") or None


def parse_dist1(text: str) -> Ticket:
    block = ticket_block(text)
    for label in _REQUIRED:
        if label not in block:
            raise ValueError(f"票面缺少栏目：{label}")
    lines = [line.strip(_SPACE) for line in block.splitlines()]
    ticket = Ticket(TicketType.DIST_1, number=lines[0].split("编号：", 1)[1].strip(_SPACE) or None,
                    raw_text=block)
    _header(ticket, lines)
    _signatures(ticket, lines)
    _work(ticket, lines)
    _measures(ticket, lines)
    return ticket
