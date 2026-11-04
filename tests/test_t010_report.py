"""T-010 验收测试（规划者编写，实现方不得修改）。"""

import io
import json
import re
import sys
import tempfile
import unittest
from contextlib import redirect_stdout
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "src"))
sys.path.insert(0, str(ROOT / "tests"))

from fixtures_d1 import D1_A, D1_B  # noqa: E402
from ticketguard.__main__ import main  # noqa: E402
from ticketguard.entities import load_lexicon  # noqa: E402
from ticketguard.ingest import parse_dist1  # noqa: E402
from ticketguard.ingest.scenarios import ScenarioChunk  # noqa: E402
from ticketguard.report import DISCLAIMER, DRAFT_HEAD, LIMITS, TITLE, build_report, draft_text, render_html  # noqa: E402
from ticketguard.review import CMP_BASIS, review_text  # noqa: E402
from ticketguard.rules.engine import load_rules  # noqa: E402
from ticketguard.schema import Basis, Finding, Severity  # noqa: E402
from ticketguard.templates import build_template  # noqa: E402

LEX = load_lexicon(ROOT / "data" / "rules" / "lexicon.json")
RULES = load_rules(ROOT / "data" / "rules")
SECTION = "配电第一种工作票典型场景"
TA = build_template(ScenarioChunk(SECTION, "1", "甲场景", 0, D1_A), parse_dist1(D1_A), LEX)
TB = build_template(ScenarioChunk(SECTION, "2", "乙场景", 0, D1_B), parse_dist1(D1_B), LEX)
DROP = D1_A.replace("二次回路：断开10kV测试线测试01开关操作电源空开；2、退出10kV测试线测试01开关合闸压板。", "二次回路：无")


def _review(text=DROP):
    return review_text(text, RULES, [TA, TB], LEX)


class DraftTest(unittest.TestCase):
    def test_format(self):
        b = Basis("某规程", "1.2", "原文")
        fs = [Finding("A", Severity.INFO, "提示一", b), Finding("B", Severity.WARNING, "问题一", b),
              Finding("C", Severity.ERROR, "问题二", CMP_BASIS)]
        self.assertEqual(draft_text(fs), f"{DRAFT_HEAD}\n1. 问题一（依据：某规程 1.2）\n"
                                         f"2. 问题二（依据：{CMP_BASIS.doc} 5.3.1）")
        self.assertEqual(draft_text(fs[:1]), "")


class BuildTest(unittest.TestCase):
    def test_report(self):
        r = build_report(_review(), [TA, TB], "甲票.txt")
        self.assertEqual(list(r), ["title", "source", "disclaimer", "limits", "ticket", "candidates", "findings", "draft"])
        self.assertEqual((r["title"], r["source"], r["disclaimer"], r["limits"]), (TITLE, "甲票.txt", DISCLAIMER, LIMITS))
        self.assertEqual(r["ticket"], {"number": "2099-01-P1-001", "type": "配电第一种工作票",
                                       "plan_start": "2099-01-10T08:30", "plan_end": "2099-01-10T17:30",
                                       "work_tasks": parse_dist1(D1_A).work_tasks})
        self.assertEqual([(c["id"], c["title"]) for c in r["candidates"]], [("D1-S01", "甲场景"), ("D1-S02", "乙场景")])
        self.assertEqual(r["candidates"][0]["score"], round(_review().candidates[0][1], 4))
        self.assertEqual([(f["rule_id"], f["severity"], f["label"]) for f in r["findings"]], [
            ("CMP-MISSING", "warning", "疑似问题"), ("CMP-MISSING", "warning", "疑似问题"), ("R-NEW-001", "info", "提示")])
        self.assertEqual(r["findings"][0]["basis"]["clause"], "5.3.1")
        self.assertEqual(r["draft"], draft_text(_review().findings))
        self.assertTrue(r["draft"].startswith(DRAFT_HEAD + "\n1. 对照典票场景 D1-S01"))
        json.dumps(r, ensure_ascii=False)


class HtmlTest(unittest.TestCase):
    def test_page(self):
        r = build_report(_review(), [TA, TB], "甲票.txt")
        r["findings"][2]["message"] = "<b>注入</b>&"
        page = render_html(r)
        self.assertTrue(page.lower().startswith("<!doctype html>"))
        for bad in ("http://", "https://", "<link", "src=", "`", "<b>注入"):
            self.assertNotIn(bad, page)
        self.assertIn("&lt;b&gt;注入&lt;/b&gt;&amp;", page)
        for text in (TITLE, DISCLAIMER, *LIMITS, "2099-01-P1-001", "甲场景", 'id="draft"', "复制草稿"):
            self.assertIn(text, page)
        self.assertEqual(page.count('class="pick"'), 3)
        self.assertEqual(len(re.findall(r"<input[^>]*\bchecked", page)), 2)
        self.assertIn('data-text="' + r["findings"][0]["message"] + "（依据：" + CMP_BASIS.doc, page)
        self.assertTrue(json.dumps(DRAFT_HEAD) in page or json.dumps(DRAFT_HEAD, ensure_ascii=False) in page)
        self.assertIn(f"{r['candidates'][0]['score']:.2f}", page)


class CliTest(unittest.TestCase):
    def test_check(self):
        with tempfile.TemporaryDirectory() as d:
            d = Path(d)
            (d / "典票.txt").write_text(f"二、{SECTION}\n场景1：甲场景\n{D1_A}\n场景2：乙场景\n{D1_B}\n", encoding="utf-8")
            (d / "甲票.2099.1.txt").write_text(DROP, encoding="utf-8")
            with redirect_stdout(io.StringIO()):
                main(["templates", str(d / "典票.txt"), str(d / "tpl")])
            buf = io.StringIO()
            with redirect_stdout(buf):
                code = main(["check", str(d / "甲票.2099.1.txt"), "--out", str(d / "out"), "--templates", str(d / "tpl"),
                             "--rules", str(ROOT / "data" / "rules"),
                             "--lexicon", str(ROOT / "data" / "rules" / "lexicon.json")])
            self.assertEqual(code, 0)
            report = json.loads((d / "out" / "甲票.2099.1.预审.json").read_text(encoding="utf-8"))
            self.assertEqual(report["source"], "甲票.2099.1.txt")
            self.assertTrue((d / "out" / "甲票.2099.1.预审.html").exists())
            out = buf.getvalue().splitlines()
            self.assertTrue(out[0].startswith("工作票 2099-01-P1-001：最接近典票场景 D1-S01「甲场景」（匹配度 0."))
            self.assertEqual(out[1], "疑似问题 3 条（需人工确认）：")
            self.assertTrue(out[2].startswith("  [疑似问题] 对照典票场景 D1-S01"))
            self.assertEqual(out[-1], f"报告 → {d / 'out' / '甲票.2099.1.预审.html'}")

    def test_unsupported(self):
        with tempfile.TemporaryDirectory() as d:
            p = Path(d) / "票.pdf"
            p.write_bytes(b"%PDF")
            with self.assertRaisesRegex(ValueError, r"只支持 \.txt（票面文本）和 \.docx"):
                main(["check", str(p)])


class RequiredFieldLabelTest(unittest.TestCase):
    def test_chinese_label(self):
        f = [x for x in review_text(D1_A.replace("总人数共", "总人数"), RULES, [TA, TB], LEX).findings
             if x.rule_id == "R-REQ-001"]
        self.assertEqual([x.message for x in f], ["票面“工作负责人及工作班人员总人数”栏未填写或无法识别。"])


if __name__ == "__main__":
    unittest.main()
