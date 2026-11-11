from __future__ import annotations

import io
import json
import sys
import tempfile
import unittest
from contextlib import redirect_stdout
from pathlib import Path
from xml.sax.saxutils import escape
from zipfile import ZipFile

ROOT = Path(__file__).resolve().parents[1]
sys.path[:0] = [str(ROOT / "src"), str(ROOT / "tests")]

from fixtures_d1 import D1_A
from ticketguard.__main__ import main
from ticketguard.ingest.office import W


class DocxCheckTest(unittest.TestCase):
    def test_uppercase_docx(self) -> None:
        body = "".join(f"<w:p><w:r><w:t>{escape(line)}</w:t></w:r></w:p>" for line in D1_A.splitlines())
        stream = io.BytesIO()
        with ZipFile(stream, "w") as docx:
            docx.writestr("word/document.xml", f'<w:document xmlns:w="{W[1:-1]}"><w:body>{body}</w:body></w:document>')
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            source = root / "样票.2099.DOCX"
            source.write_bytes(stream.getvalue())
            with redirect_stdout(io.StringIO()):
                code = main(["check", str(source), "--out", str(root / "reports")])
            report = json.loads((root / "reports" / "样票.2099.预审.json").read_text(encoding="utf-8"))
            self.assertEqual(code, 0)
            self.assertEqual(report["source"], source.name)
            self.assertEqual(report["ticket"]["number"], "2099-01-P1-001")
