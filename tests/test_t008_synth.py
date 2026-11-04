"""T-008 验收测试（规划者编写，实现方不得修改）。"""

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
from ticketguard.entities import lines, load_lexicon  # noqa: E402
from ticketguard.ingest import parse_dist1  # noqa: E402
from ticketguard.ingest.scenarios import ScenarioChunk  # noqa: E402
from ticketguard.review import review_text  # noqa: E402
from ticketguard.rules.engine import load_rules  # noqa: E402
from ticketguard.synth import OPERATORS, generate  # noqa: E402
from ticketguard.templates import build_template  # noqa: E402

LEX = load_lexicon(ROOT / "data" / "rules" / "lexicon.json")
RULES = load_rules(ROOT / "data" / "rules")
SECTION = "配电第一种工作票典型场景"
TA = build_template(ScenarioChunk(SECTION, "1", "甲场景", 0, D1_A), parse_dist1(D1_A), LEX)
TB = build_template(ScenarioChunk(SECTION, "2", "乙场景", 0, D1_B), parse_dist1(D1_B), LEX)


def _run(name, text=D1_A, seed=0):
    return OPERATORS[name](text, random.Random(seed))


class OperatorTest(unittest.TestCase):
    def test_names_and_order(self):
        self.assertEqual(list(OPERATORS), ["crew_off", "time_swap", "plan_swap", "outage_wrong", "loc_strip",
                                           "drop_signer", "drop_category", "paraphrase", "rename_lines"])

    def test_crew_off(self):
        text, exp = _run("crew_off")
        self.assertIn(parse_dist1(text).crew_size, (6, 7))
        self.assertEqual(exp, [{"rule_id": "R-CREW-001"}])

    def test_times(self):
        text, exp = _run("time_swap")
        self.assertEqual(parse_dist1(text).fields["接收时间"], "2000-01-01T00:00")
        self.assertEqual(exp, [{"rule_id": "R-TIME-001"}])
        t = parse_dist1(_run("plan_swap")[0])
        self.assertEqual((t.plan_start, t.plan_end), ("2099-01-10T17:30", "2099-01-10T08:30"))

    def test_outage_and_location(self):
        self.assertEqual(parse_dist1(_run("outage_wrong")[0]).outage_scope, "35kV虚构站10kV虚构线")
        t = parse_dist1(_run("loc_strip")[0])
        self.assertEqual(t.work_tasks[0]["地点"], "10kV测试线乙支6号杆甲公变台架")
        self.assertEqual(t.work_tasks[0]["任务"], "（XX分局）35kV甲站10kV测试线配合：更换10kV测试线甲公变。")

    def test_drop_signer(self):
        text, exp = _run("drop_signer")
        self.assertNotIn("接收人", [p.role for p in parse_dist1(text).people])
        self.assertEqual(exp, [{"rule_id": "R-REQ-002"}])

    def test_drop_category(self):
        for seed in range(6):
            text, exp = _run("drop_category", seed=seed)
            cat = exp[0]["category"]
            self.assertEqual(exp[0]["rule_id"], "CMP-MISSING")
            self.assertEqual([m for m in parse_dist1(text).measures if m.category == cat], [])
            others = [m for m in parse_dist1(D1_A).measures if m.category != cat]
            self.assertEqual([m for m in parse_dist1(text).measures if m.category != cat], others)

    def test_paraphrase_and_rename(self):
        text, exp = _run("paraphrase")
        self.assertNotEqual(text, D1_A)
        self.assertEqual(exp, [])
        text, exp = _run("rename_lines")
        self.assertEqual(lines(text), {"10kV虚构1号线"})
        self.assertNotIn("测试线", text)

    def test_not_applicable(self):
        for name in OPERATORS:
            self.assertIsNone(OPERATORS[name]("没有任何票面内容", random.Random(0)), name)


class EndToEndTest(unittest.TestCase):
    def test_expected_findings_reported(self):
        for src in (D1_A, D1_B):
            base = [f.rule_id for f in review_text(src, RULES, [TA, TB], LEX).findings]
            for name in OPERATORS:
                for seed in range(4):
                    text, exp = _run(name, src, seed)
                    got = review_text(text, RULES, [TA, TB], LEX).findings
                    if not exp:
                        self.assertEqual([f.rule_id for f in got], base, f"{name} 不应新增问题")
                    for e in exp:
                        self.assertTrue(any(f.rule_id == e["rule_id"] and
                                            ("category" not in e or f"“{e['category']}”栏" in f.message)
                                            for f in got), f"{name} seed={seed} 应报出 {e}")


class GenerateTest(unittest.TestCase):
    CHUNKS = [ScenarioChunk(SECTION, "3", "甲", 0, D1_A), ScenarioChunk(SECTION, "12", "乙", 0, D1_B)]

    def test_shape_and_dedupe(self):
        samples = generate(self.CHUNKS, per=4, seed=0)
        ids = [s["id"] for s in samples]
        self.assertEqual(len(ids), len(set(ids)))
        first = samples[0]
        self.assertEqual(list(first), ["id", "base_scenario", "mutations", "ticket_text", "expected_findings"])
        self.assertEqual((first["id"], first["base_scenario"], first["mutations"]),
                         ("syn-D1-S03-crew_off-0", "D1-S03", [{"op": "crew_off"}]))
        # 确定性算子去重后只剩 1 个
        self.assertEqual([s["id"] for s in samples if s["base_scenario"] == "D1-S12" and "time_swap" in s["id"]],
                         ["syn-D1-S12-time_swap-0"])
        # crew_off 只可能 +1 或 +2，最多 2 个不同样本
        self.assertLessEqual(len([s for s in samples if s["base_scenario"] == "D1-S03" and "crew_off" in s["id"]]), 2)
        self.assertEqual(generate(self.CHUNKS, per=4, seed=0), samples)

    def test_cli(self):
        text = f"二、{SECTION}\n场景1：甲\n{D1_A}\n场景2：乙\n{D1_B}\n"
        with tempfile.TemporaryDirectory() as d:
            src = Path(d) / "典票.txt"
            src.write_text(text, encoding="utf-8")
            out = Path(d) / "sub" / "s.jsonl"
            buf = io.StringIO()
            with redirect_stdout(buf):
                code = main(["synth", str(src), str(out), "--per", "2", "--seed", "1"])
            self.assertEqual(code, 0)
            rows = [json.loads(line) for line in out.read_text(encoding="utf-8").splitlines()]
            self.assertIn(f"已生成 {len(rows)} 个样本", buf.getvalue())
            self.assertEqual({r["base_scenario"] for r in rows}, {"D1-S01", "D1-S02"})
            self.assertIn("测试线", out.read_text(encoding="utf-8"))


if __name__ == "__main__":
    unittest.main()
