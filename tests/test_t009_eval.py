"""T-009 验收测试（规划者编写，实现方不得修改）。"""

import io
import json
import random
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
from ticketguard.evaluate import evaluate, wilson  # noqa: E402
from ticketguard.ingest import parse_dist1  # noqa: E402
from ticketguard.ingest.scenarios import ScenarioChunk  # noqa: E402
from ticketguard.rules.engine import load_rules  # noqa: E402
from ticketguard.synth import OPERATORS, generate, rename_lines  # noqa: E402
from ticketguard.templates import build_template  # noqa: E402

LEX = load_lexicon(ROOT / "data" / "rules" / "lexicon.json")
RULES = load_rules(ROOT / "data" / "rules")
SECTION = "配电第一种工作票典型场景"
CHUNKS = [ScenarioChunk(SECTION, "1", "甲场景", 0, D1_A), ScenarioChunk(SECTION, "2", "乙场景", 0, D1_B)]
TEMPLATES = [build_template(c, parse_dist1(c.text), LEX) for c in CHUNKS]
ORIG = {"D1-S01": D1_A, "D1-S02": D1_B}
DROP = D1_A.replace("二次回路：断开10kV测试线测试01开关操作电源空开；2、退出10kV测试线测试01开关合闸压板。", "二次回路：无")


def _sample(sid, op, text, expected, base="D1-S01"):
    return {"id": sid, "base_scenario": base, "mutations": [{"op": op}], "ticket_text": text,
            "expected_findings": expected}


def _eval(samples, originals=ORIG):
    return evaluate(samples, originals, RULES, TEMPLATES, LEX)


class WilsonTest(unittest.TestCase):
    def test_values(self):
        self.assertEqual(wilson(5, 10), (0.2366, 0.7634))
        self.assertEqual(wilson(10, 10), (0.7225, 1.0))
        self.assertEqual(wilson(0, 7), (0.0, 0.3543))
        self.assertEqual(wilson(0, 0), (0.0, 0.0))


class EvaluateTest(unittest.TestCase):
    def test_generated_all_detected(self):
        samples = generate(CHUNKS, per=3, seed=0)
        r = _eval(samples)
        self.assertEqual(list(r), ["samples", "top1", "expected", "detected", "recall", "recall_ci95",
                                   "unexpected", "by_operator", "misses", "unexpected_findings"])
        self.assertEqual((r["samples"], r["top1"]), (len(samples), len(samples)))
        self.assertEqual((r["detected"], r["recall"], r["unexpected"]), (r["expected"], 1.0, 0))
        self.assertEqual(list(r["by_operator"]), list(OPERATORS))
        self.assertEqual(r["by_operator"]["crew_off"]["recall_ci95"], wilson(r["by_operator"]["crew_off"]["detected"],
                                                                             r["by_operator"]["crew_off"]["expected"]))
        para = r["by_operator"]["paraphrase"]
        self.assertEqual((para["expected"], para["recall"], para["recall_ci95"]), (0, None, None))
        self.assertEqual(list(r["by_operator"]["time_swap"]),
                         ["samples", "top1", "expected", "detected", "unexpected", "recall", "recall_ci95"])

    def test_miss_and_unexpected(self):
        crew = OPERATORS["crew_off"](D1_A, random.Random(0))[0]
        r = _eval([_sample("s-miss", "crew_off", D1_A, [{"rule_id": "R-CREW-001"}]),
                   _sample("s-extra", "paraphrase", crew, [])])
        self.assertEqual(r["misses"], [{"id": "s-miss", "expected": {"rule_id": "R-CREW-001"}}])
        self.assertEqual([(u["id"], u["rule_id"]) for u in r["unexpected_findings"]], [("s-extra", "R-CREW-001")])
        self.assertEqual((r["by_operator"]["crew_off"]["recall"], r["by_operator"]["paraphrase"]["unexpected"]), (0.0, 1))

    def test_category_must_match(self):
        r = _eval([_sample("s1", "drop_category", DROP, [{"rule_id": "CMP-MISSING", "category": "接地"}]),
                   _sample("s2", "drop_category", DROP, [{"rule_id": "CMP-MISSING", "category": "应投切电源及二次回路"}])])
        self.assertEqual([m["id"] for m in r["misses"]], ["s1"])
        # s1 没报出期望的类别，实际报出的两条“应投切”缺项都算意外发现
        self.assertEqual([u["id"] for u in r["unexpected_findings"]], ["s1", "s1"])

    def test_already_in_original(self):
        # 原票已有的问题：检出照算，但不算意外发现
        r = _eval([_sample("s", "x", D1_A, [{"rule_id": "R-NEW-001"}]), _sample("t", "y", D1_A, [])])
        self.assertEqual((r["detected"], r["unexpected"]), (1, 0))

    def test_same_problem_different_wording(self):
        # 原票本来就缺措施；换线路名后匹配度变了、提示文字也变了，但仍是同样的缺项，不算意外发现
        renamed = rename_lines(DROP, random.Random(0))[0]
        r = _eval([_sample("s", "rename_lines", renamed, [])], {"D1-S01": DROP})
        self.assertEqual(r["unexpected"], 0)

    def test_top1_counts_wrong_scenario(self):
        r = _eval([_sample("s", "x", D1_B, [])], {"D1-S01": D1_A, "D1-S02": D1_B})
        self.assertEqual(r["top1"], 0)


class CliTest(unittest.TestCase):
    def test_eval_command(self):
        text = f"二、{SECTION}\n场景1：甲\n{D1_A}\n场景2：乙\n{D1_B}\n"
        with tempfile.TemporaryDirectory() as d:
            d = Path(d)
            (d / "典票.txt").write_text(text, encoding="utf-8")
            with redirect_stdout(io.StringIO()):
                main(["templates", str(d / "典票.txt"), str(d / "tpl")])
                main(["synth", str(d / "典票.txt"), str(d / "s.jsonl"), "--per", "2"])
            out = d / "r" / "eval.json"
            buf = io.StringIO()
            with redirect_stdout(buf):
                code = main(["eval", str(d / "典票.txt"), str(d / "s.jsonl"), "--out", str(out),
                             "--templates", str(d / "tpl"), "--rules", str(ROOT / "data" / "rules"),
                             "--lexicon", str(ROOT / "data" / "rules" / "lexicon.json")])
            self.assertEqual(code, 0)
            report = json.loads(out.read_text(encoding="utf-8"))
            self.assertEqual((report["recall"], report["unexpected"]), (1.0, 0))
            self.assertEqual(report["recall_ci95"], list(wilson(report["detected"], report["expected"])))
            printed = buf.getvalue()
            for op in OPERATORS:
                self.assertIn(op, printed)
            self.assertIn(f"→ {out}", printed)


if __name__ == "__main__":
    unittest.main()
