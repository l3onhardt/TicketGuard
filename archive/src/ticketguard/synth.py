from __future__ import annotations

import random
import re
from typing import Callable

from .ingest.scenarios import ScenarioChunk

Mutation = tuple[str, list[dict]] | None
DT = r"\d{4}\s*年\s*\d{1,2}\s*月\s*\d{1,2}\s*日\s*\d{1,2}\s*时\s*\d{1,2}\s*分"


def _line(text: str, label: str, pattern: str,
          replacement: str | Callable[[re.Match[str]], str], rule_id: str) -> Mutation:
    rows = text.splitlines(keepends=True)
    for i, row in enumerate(rows):
        if label in row:
            changed = re.sub(pattern, replacement, row, count=1)
            if changed == row:
                return None
            rows[i] = changed
            return "".join(rows), [{"rule_id": rule_id}]
    return None


def crew_off(text: str, rng: random.Random) -> Mutation:
    return _line(text, "总人数共", r"(总人数共\s*)(\d+)",
                 lambda m: m[1] + str(int(m[2]) + rng.choice([1, 2])), "R-CREW-001")


def time_swap(text: str, rng: random.Random) -> Mutation:
    return _line(text, "工作票接收人签名：", DT, "2000年01月01日00时00分", "R-TIME-001")


def plan_swap(text: str, rng: random.Random) -> Mutation:
    changed = re.sub(rf"自\s*({DT})(.*?)至\s*({DT})", r"自\3\2至\1", text, count=1, flags=re.S)
    return (changed, [{"rule_id": "R-TIME-002"}]) if changed != text else None


def outage_wrong(text: str, rng: random.Random) -> Mutation:
    return _line(text, "停电设备或线路名称：", r"(停电设备或线路名称：)[^\r\n]*",
                 r"\g<1>35kV虚构站10kV虚构线", "R-LOC-002")


def loc_strip(text: str, rng: random.Random) -> Mutation:
    rows = text.splitlines(keepends=True)
    for i, row in enumerate(rows):
        if row.startswith("工作地点或设备（注明"):
            if i + 1 == len(rows) or "|" not in rows[i + 1]:
                return None
            location, sep, rest = rows[i + 1].partition("|")
            changed = re.sub(r"\d+kV[^，、；。|]*?站", "", location)
            if changed == location:
                return None
            rows[i + 1] = changed + sep + rest
            return "".join(rows), [{"rule_id": "R-LOC-001"}]
    return None


def drop_signer(text: str, rng: random.Random) -> Mutation:
    return _line(text, "工作票接收人签名：", r"(工作票接收人签名：[\s　]*)[^\s　/（(]+",
                 r"\1/", "R-REQ-002")


def drop_category(text: str, rng: random.Random) -> Mutation:
    rows = text.splitlines(keepends=True)
    start = next((i for i, row in enumerate(rows) if row.startswith("调度或设备运维单位负责的安全措施")), None)
    if start is None:
        return None
    categories = [("应拉", "应拉开关刀闸熔断器"), ("应投切", "应投切电源及二次回路"),
                  ("接地", "接地"), ("标志牌", "标志牌"), ("保留或邻近", "保留带电设备")]
    candidates = []
    for i in range(start, min(start + 5, len(rows))):
        prefix, sep, rest = rows[i].partition("|")
        label, colon, content = rest.partition("：")
        if sep and colon and not re.fullmatch(r"[\s　/]*无[。.]?[\s　]*", content):
            category = next(value for key, value in categories if key in label)
            candidates.append((i, prefix + "|" + label + "：无", category))
    if not candidates:
        return None
    i, changed, category = rng.choice(candidates)
    rows[i] = changed + rows[i][len(rows[i].rstrip("\r\n")):]
    return "".join(rows), [{"rule_id": "CMP-MISSING", "category": category}]


def paraphrase(text: str, rng: random.Random) -> Mutation:
    pairs = [("合闸压板", "合闸连接片"), ("令克", "跌落式熔断器"),
             ("刀闸", "隔离开关"), ("空开", "空气开关")]
    choices = [(old, new) for old, new in pairs if old in text]
    if not choices:
        return None
    old, new = rng.choice(choices)
    return text.replace(old, new), []


def rename_lines(text: str, rng: random.Random) -> Mutation:
    names = set(re.findall(r"10kV([^\s，,、；;。:：/kV“”（）()|]{1,6}?线)", text))
    if not names:
        return None
    for i, name in enumerate(sorted(names, key=lambda name: (-len(name), name)), 1):
        text = text.replace(name, f"虚构{i}号线")
    return text, []


OPERATORS: dict[str, Callable] = {op.__name__: op for op in (
    crew_off, time_swap, plan_swap, outage_wrong, loc_strip,
    drop_signer, drop_category, paraphrase, rename_lines)}


def generate(chunks: list[ScenarioChunk], per: int, seed: int) -> list[dict]:
    rng = random.Random(seed)
    samples = []
    for chunk in chunks:
        base = f"D1-S{int(chunk.number):02d}"
        for name, operator in OPERATORS.items():
            seen: set[str] = set()
            for _ in range(per):
                mutation = operator(chunk.text, rng)
                if mutation is None:
                    break
                text, expected = mutation
                if text in seen:
                    continue
                samples.append({"id": f"syn-{base}-{name}-{len(seen)}", "base_scenario": base,
                                "mutations": [{"op": name}], "ticket_text": text,
                                "expected_findings": expected})
                seen.add(text)
    return samples
