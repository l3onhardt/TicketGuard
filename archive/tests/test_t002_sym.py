"""T-002 验收测试（规划者编写，实现方不得修改）。"""

import io
import sys
import unittest
import zipfile
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "src"))

from ticketguard.ingest import read_docx  # noqa: E402

NS = 'xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"'


def _sym(font: str, char: str) -> str:
    return f'<w:r><w:sym w:font="{font}" w:char="{char}"/></w:r>'


def _t(s: str) -> str:
    return f"<w:r><w:t xml:space=\"preserve\">{s}</w:t></w:r>"


def _docx(body: str) -> io.BytesIO:
    buf = io.BytesIO()
    with zipfile.ZipFile(buf, "w") as z:
        z.writestr("word/document.xml", f"<w:document {NS}><w:body>{body}</w:body></w:document>")
    buf.seek(0)
    return buf


class SymTest(unittest.TestCase):
    def test_paragraph_order(self):
        p = "<w:p>" + _t("是否办理分组工作派工单：") + _sym("Wingdings 2", "00A3") + _t("是，共　张；") \
            + _sym("Wingdings 2", "0052") + _t("否") + "</w:p>"
        self.assertEqual(read_docx(_docx(p))[0].text, "是否办理分组工作派工单：□是，共　张；☑否")

    def test_private_use_offset_and_wingdings(self):
        p = "<w:p>" + _sym("Wingdings 2", "F052") + _t("A") + _sym("Wingdings", "F0A8") + _t("B") \
            + _sym("Wingdings", "00FE") + _t("C") + _sym("Wingdings", "00A8") + _t("D") + "</w:p>"
        self.assertEqual(read_docx(_docx(p))[0].text, "☑A□B☑C□D")

    def test_unknown_symbol_dropped(self):
        p = "<w:p>" + _t("甲") + _sym("Symbol", "00B7") + _sym("Wingdings", "0052") + _t("乙") + "</w:p>"
        self.assertEqual(read_docx(_docx(p))[0].text, "甲乙")

    def test_table_cell(self):
        tbl = "<w:tbl><w:tr><w:tc><w:p>" + _t("下达方式：") + _sym("Wingdings 2", "0052") + _t("电话下达") \
            + "</w:p></w:tc></w:tr></w:tbl>"
        self.assertEqual(read_docx(_docx(tbl))[0].rows, [["下达方式：☑电话下达"]])


if __name__ == "__main__":
    unittest.main()
