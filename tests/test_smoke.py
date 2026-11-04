import io
import sys
import unittest
import zipfile
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "src"))

from ticketguard.decision import Answer  # noqa: E402
from ticketguard.ingest import read_docx, read_xlsx  # noqa: E402

DOCX_XML = (
    '<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body>'
    "<w:p><w:r><w:t>配电第一种工作票</w:t></w:r></w:p>"
    "<w:tbl><w:tr><w:tc><w:p><w:r><w:t>工作负责人</w:t></w:r></w:p></w:tc>"
    "<w:tc><w:p><w:r><w:t>张三</w:t></w:r></w:p></w:tc></w:tr></w:tbl>"
    "</w:body></w:document>"
)


class SmokeTest(unittest.TestCase):
    def test_read_docx(self):
        buf = io.BytesIO()
        with zipfile.ZipFile(buf, "w") as z:
            z.writestr("word/document.xml", DOCX_XML)
        buf.seek(0)
        blocks = read_docx(buf)
        self.assertEqual(blocks[0].text, "配电第一种工作票")
        self.assertEqual(blocks[1].rows, [["工作负责人", "张三"]])

    def test_read_xlsx(self):
        buf = io.BytesIO()
        ns = 'xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"'
        with zipfile.ZipFile(buf, "w") as z:
            z.writestr(
                "xl/workbook.xml",
                f'<workbook {ns} xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">'
                '<sheets><sheet name="场景1" sheetId="1" r:id="rId1"/></sheets></workbook>',
            )
            z.writestr(
                "xl/_rels/workbook.xml.rels",
                '<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">'
                '<Relationship Id="rId1" Target="worksheets/sheet1.xml"/></Relationships>',
            )
            z.writestr("xl/sharedStrings.xml", f"<sst {ns}><si><t>带电作业</t></si></sst>")
            z.writestr(
                "xl/worksheets/sheet1.xml",
                f'<worksheet {ns}><sheetData><row r="1"><c r="B1" t="s"><v>0</v></c></row></sheetData></worksheet>',
            )
        buf.seek(0)
        self.assertEqual(read_xlsx(buf), {"场景1": [["", "带电作业"]]})

    def test_answer_confidence(self):
        self.assertAlmostEqual(Answer({"是": 1.0, "否": 0.0}).confidence, 1.0)
        self.assertAlmostEqual(Answer({"是": 0.5, "否": 0.5}).confidence, 0.0)
        self.assertEqual(Answer({"是": 0.9, "否": 0.1}).value, "是")


if __name__ == "__main__":
    unittest.main()
