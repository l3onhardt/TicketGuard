"""T-007 验收测试（规划者编写，实现方不得修改）。"""

import copy
import sys
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "src"))
sys.path.insert(0, str(ROOT / "tests"))

from fixtures_d1 import D1_A, D1_B  # noqa: E402
from ticketguard.entities import load_lexicon  # noqa: E402
from ticketguard.ingest import parse_dist1, split_scenarios  # noqa: E402
from ticketguard.ingest.scenarios import ScenarioChunk  # noqa: E402
from ticketguard.review import CMP_BASIS, MATCH_MIN, compare_measures, match_scenario, review_text  # noqa: E402
from ticketguard.rules.engine import load_rules  # noqa: E402
from ticketguard.schema import SafetyMeasure, Severity  # noqa: E402
from ticketguard.templates import build_template, load_templates  # noqa: E402

LEX = load_lexicon(ROOT / "data" / "rules" / "lexicon.json")
RULES = load_rules(ROOT / "data" / "rules")
SECTION = "配电第一种工作票典型场景"
TA = build_template(ScenarioChunk(SECTION, "1", "甲场景", 0, D1_A), parse_dist1(D1_A), LEX)
TB = build_template(ScenarioChunk(SECTION, "2", "乙场景", 0, D1_B), parse_dist1(D1_B), LEX)
REAL = ROOT / "data" / "interim" / "2026.9南宁供电局配电运检、配电自动化两票典型票库-2026版（试行）.txt"


class MatchTest(unittest.TestCase):
    def test_ranking_and_scores(self):
        got = match_scenario(parse_dist1(D1_A), [TB, TA], LEX)
        self.assertEqual([i for i, _ in got], ["D1-S01", "D1-S02"])
        self.assertAlmostEqual(got[0][1], 0.6136363636, places=6)
        self.assertAlmostEqual(got[1][1], 0.2588235294, places=6)
        got = match_scenario(parse_dist1(D1_B), [TA, TB], LEX)
        self.assertEqual(got[0][0], "D1-S02")
        self.assertAlmostEqual(got[0][1], 0.6477272727, places=6)

    def test_tie_by_id(self):
        t2 = dict(TA, id="D1-S00")
        self.assertEqual([i for i, _ in match_scenario(parse_dist1(D1_A), [TA, t2], LEX)], ["D1-S00", "D1-S01"])


class CompareTest(unittest.TestCase):
    def test_missing_measures(self):
        t = parse_dist1(D1_A)
        t.measures = [m for m in t.measures if m.category != "应投切电源及二次回路"]
        f = compare_measures(t, TA, LEX, 0.8)
        self.assertEqual([(x.rule_id, x.severity, x.evidence) for x in f], [
            ("CMP-MISSING", Severity.WARNING, "D1-S01-M02"),
            ("CMP-MISSING", Severity.WARNING, "D1-S01-M03"),
        ])
        self.assertEqual(f[0].message, "对照典票场景 D1-S01「甲场景」（匹配度 0.80），"
                                       "票面“应投切电源及二次回路”栏中没有找到这类措施：断开{线路}测试{编号}开关操作电源空开")
        self.assertEqual((f[0].confidence, f[0].needs_review, f[0].scenario_id), (0.8, True, "D1-S01"))
        self.assertEqual(f[0].basis, CMP_BASIS)

    def test_paraphrase_and_category(self):
        t = parse_dist1(D1_A)
        self.assertEqual(compare_measures(t, TA, LEX, 0.9), [])
        # 换说法（合闸连接片 = 合闸压板）不算缺
        for m in t.measures:
            m.text = m.text.replace("合闸压板", "合闸连接片")
        self.assertEqual(compare_measures(t, TA, LEX, 0.9), [])
        # 写到了别的栏目里，算缺
        t.measures = [SafetyMeasure("其他", m.text, m.order) if m.category == "接地" else m for m in t.measures]
        self.assertEqual([x.evidence for x in compare_measures(t, TA, LEX, 0.9)], ["D1-S01-M04"])

    def test_optional_is_info(self):
        tpl = copy.deepcopy(TA)
        tpl["required_measures"][1]["must"] = False
        t = parse_dist1(D1_A)
        t.measures = [m for m in t.measures if m.category != "应投切电源及二次回路"]
        self.assertEqual([x.severity for x in compare_measures(t, tpl, LEX, 0.5)], [Severity.INFO, Severity.WARNING])


class ReviewTest(unittest.TestCase):
    def test_full_pipeline(self):
        r = review_text(D1_A, RULES, [TA, TB], LEX)
        self.assertEqual(r.ticket.number, "2099-01-P1-001")
        self.assertEqual([i for i, _ in r.candidates], ["D1-S01", "D1-S02"])
        self.assertEqual([f.rule_id for f in r.findings], ["R-NEW-001"])

    def test_sorted_by_severity(self):
        text = D1_A.replace("二次回路：断开10kV测试线测试01开关操作电源空开；2、退出10kV测试线测试01开关合闸压板。",
                            "二次回路：无")
        r = review_text(text, RULES, [TA, TB], LEX)
        self.assertEqual([(f.rule_id, f.severity) for f in r.findings], [
            ("CMP-MISSING", Severity.WARNING), ("CMP-MISSING", Severity.WARNING), ("R-NEW-001", Severity.INFO),
        ])

    def test_low_match_skips_compare(self):
        self.assertEqual(MATCH_MIN, 0.3)
        r = review_text(D1_A, RULES, [TB], LEX)
        self.assertEqual([f.rule_id for f in r.findings], ["R-NEW-001", "MATCH-LOW"])
        self.assertEqual(r.findings[1].message, "没有找到足够相似的典票场景（最接近的是 D1-S02，匹配度 0.26），本次未做安全措施比对。")

    def test_candidates_at_most_three(self):
        many = [dict(TB, id=f"D1-S9{i}") for i in range(5)] + [TA]
        self.assertEqual(len(review_text(D1_A, RULES, many, LEX).candidates), 3)


@unittest.skipUnless(REAL.exists(), "真实典票文本不存在（data/interim 未生成）")
class RealDocumentTest(unittest.TestCase):
    def test_typical_self_review(self):
        templates = load_templates(ROOT / "data" / "templates" / "D1")
        found = []
        for c in split_scenarios(REAL.read_text(encoding="utf-8")):
            if c.section != SECTION:
                continue
            r = review_text(c.text, RULES, templates, LEX)
            self.assertEqual(r.candidates[0][0], f"D1-S{int(c.number):02d}", f"场景{c.number}")
            found += [(c.number, f.rule_id) for f in r.findings]
        self.assertEqual(found, [("13", "R-LOC-002")])


if __name__ == "__main__":
    unittest.main()
