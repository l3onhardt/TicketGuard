"""零依赖的 Office 文档解析（仅用标准库）。

内网机器上不一定能 pip 安装第三方库，所以 docx / xlsx 直接按 OOXML（zip + XML）解析。
旧格式 .doc / .xls（OLE 二进制）不在此处理，需先用 WPS / Word 另存为 docx / xlsx。
"""

from __future__ import annotations

import re
import zipfile
from dataclasses import dataclass, field
from pathlib import Path
from xml.etree import ElementTree as ET

W = "{http://schemas.openxmlformats.org/wordprocessingml/2006/main}"
S = "{http://schemas.openxmlformats.org/spreadsheetml/2006/main}"
R = "{http://schemas.openxmlformats.org/officeDocument/2006/relationships}"
_SYM = {
    ("Wingdings 2", 0x52): "☑", ("Wingdings 2", 0xA3): "□",
    ("Wingdings", 0xA8): "□", ("Wingdings", 0xFE): "☑",
}


@dataclass
class Block:
    """文档中的一个块：段落或表格。"""

    kind: str  # "p" | "table"
    text: str = ""
    rows: list[list[str]] = field(default_factory=list)


def _wtext(el: ET.Element) -> str:
    parts: list[str] = []
    for node in el.iter():
        if node.tag == W + "t":
            parts.append(node.text or "")
        elif node.tag == W + "sym":
            code = int(node.attrib[W + "char"], 16)
            if code >= 0xF000:
                code -= 0xF000
            parts.append(_SYM.get((node.get(W + "font"), code), ""))
    return "".join(parts)


def _cell_text(tc: ET.Element) -> str:
    parts = [_wtext(p).strip() for p in tc.iter(W + "p")]
    return "\n".join(p for p in parts if p)


def read_docx(path: str | Path) -> list[Block]:
    """按文档顺序返回段落与表格。"""
    with zipfile.ZipFile(path) as z:
        root = ET.fromstring(z.read("word/document.xml"))
    body = root.find(W + "body")
    blocks: list[Block] = []
    if body is None:
        return blocks
    for el in body:
        if el.tag == W + "p":
            s = _wtext(el).strip()
            if s:
                blocks.append(Block("p", text=s))
        elif el.tag == W + "tbl":
            rows = [[_cell_text(tc) for tc in tr.findall(W + "tc")] for tr in el.iter(W + "tr")]
            blocks.append(Block("table", rows=rows))
    return blocks


def read_docx_notes(path: str | Path) -> dict[str, list[str]]:
    """返回脚注 / 尾注 / 批注文本。典票库的安规条款依据大多在脚注里。"""
    out: dict[str, list[str]] = {}
    with zipfile.ZipFile(path) as z:
        names = set(z.namelist())
        for part, key in (
            ("word/footnotes.xml", "footnotes"),
            ("word/endnotes.xml", "endnotes"),
            ("word/comments.xml", "comments"),
        ):
            if part in names:
                root = ET.fromstring(z.read(part))
                items = [_wtext(n).strip() for n in root]
                out[key] = [i for i in items if i]
    return out


def read_xlsx(path: str | Path) -> dict[str, list[list[str]]]:
    """返回 {工作表名: 行列表}，空单元格以 "" 占位（不保留合并单元格信息）。"""
    with zipfile.ZipFile(path) as z:
        names = set(z.namelist())
        shared: list[str] = []
        if "xl/sharedStrings.xml" in names:
            for si in ET.fromstring(z.read("xl/sharedStrings.xml")).findall(S + "si"):
                shared.append("".join(t.text or "" for t in si.iter(S + "t")))

        wb = ET.fromstring(z.read("xl/workbook.xml"))
        rels = ET.fromstring(z.read("xl/_rels/workbook.xml.rels"))
        rid_to_target = {r.get("Id"): r.get("Target") for r in rels}

        sheets: dict[str, list[list[str]]] = {}
        for sh in wb.iter(S + "sheet"):
            target = rid_to_target.get(sh.get(R + "id"), "")
            target = target.lstrip("/")
            part = target if target.startswith("xl/") else "xl/" + target
            if part not in names:
                continue
            root = ET.fromstring(z.read(part))
            rows: list[list[str]] = []
            for row in root.iter(S + "row"):
                vals: list[str] = []
                for c in row.findall(S + "c"):
                    col = _col_index(c.get("r", ""))
                    while col is not None and len(vals) < col:
                        vals.append("")
                    vals.append(_cell_value(c, shared))
                if any(v.strip() for v in vals):
                    rows.append(vals)
            sheets[sh.get("name", part)] = rows
    return sheets


def _col_index(ref: str) -> int | None:
    m = re.match(r"([A-Z]+)", ref)
    if not m:
        return None
    n = 0
    for ch in m.group(1):
        n = n * 26 + (ord(ch) - 64)
    return n - 1


def _cell_value(c: ET.Element, shared: list[str]) -> str:
    t = c.get("t")
    if t == "inlineStr":
        is_ = c.find(S + "is")
        return "".join(x.text or "" for x in is_.iter(S + "t")) if is_ is not None else ""
    v = c.find(S + "v")
    if v is None or v.text is None:
        return ""
    if t == "s":
        return shared[int(v.text)]
    return v.text


def blocks_to_text(blocks: list[Block]) -> str:
    lines: list[str] = []
    for b in blocks:
        if b.kind == "p":
            lines.append(b.text)
        else:
            lines.append("[TABLE]")
            for row in b.rows:
                lines.append(" | ".join(cell.replace("\n", " / ") for cell in row))
            lines.append("[/TABLE]")
    return "\n".join(lines)
