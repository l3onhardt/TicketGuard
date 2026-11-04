from __future__ import annotations

import re
from dataclasses import dataclass


@dataclass
class ScenarioChunk:
    section: str
    number: str
    title: str
    start_line: int
    text: str


def split_scenarios(text: str) -> list[ScenarioChunk]:
    """按段落和连续的主、子编号切分典票场景。"""
    lines = text.splitlines()
    chunks: list[ScenarioChunk] = []
    section = ""
    main = sub = 0
    current = None
    for index, line in enumerate(lines):
        heading = re.fullmatch(r"[一二三四五六七八九十]+、(.*典型(操作)?场景)", line)
        if heading:
            if current is not None:
                current.text = "\n".join(lines[current.start_line + 1:index])
            current = None
            section = heading[1]
            main = sub = 0
            continue
        if not section:
            continue
        expected = f"{main + 1}|{main}-{sub + 1}" if main else "1"
        match = re.search(rf"场景({expected})[：:\s](.*)", line)
        if match:
            if current is not None:
                current.text = "\n".join(lines[current.start_line + 1:index])
            number, title = match.groups()
            parts = number.split("-")
            main = int(parts[0])
            sub = int(parts[1]) if len(parts) == 2 else 0
            current = ScenarioChunk(section, number, title.strip(), index, "")
            chunks.append(current)
    if current is not None:
        current.text = "\n".join(lines[current.start_line + 1:])
    return chunks
