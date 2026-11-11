"""T-005 验收测试（规划者编写，实现方不得修改）。"""

import io
import json
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
from ticketguard.schema import SafetyMeasure, Ticket, TicketType  # noqa: E402
from ticketguard.templates import build_template, load_templates, required_measures  # noqa: E402

LEX_PATH = ROOT / "data" / "rules" / "lexicon.json"
LEX = load_lexicon(LEX_PATH)
SECTION = "配电第一种工作票典型场景"


class RequiredMeasuresTest(unittest.TestCase):
    def test_fixture_a(self):
        t = build_template(ScenarioChunk(SECTION, "3", "测试场景（有调整）", 0, D1_A), parse_dist1(D1_A), LEX)
        self.assertEqual(list(t), ["id", "ticket_type", "title", "source", "work_task", "required_measures", "status"])
        self.assertEqual(t["id"], "D1-S03")
        self.assertEqual(t["title"], "测试场景")
        self.assertEqual(t["source"], {"doc": "2026.9典票库", "section": SECTION, "scenario": "3", "status": "有调整"})
        self.assertEqual(t["work_task"], "更换{线路}甲公变。")
        self.assertEqual(t["status"], "draft")
        self.assertEqual([(m["id"], m["category"], m["keywords"]) for m in t["required_measures"]], [
            ("D1-S03-M01", "应拉开关刀闸熔断器", ["开关", "刀闸"]),
            ("D1-S03-M02", "应投切电源及二次回路", ["断开", "开关", "操作电源空开"]),
            ("D1-S03-M03", "应投切电源及二次回路", ["退出", "开关", "合闸压板"]),
            ("D1-S03-M04", "接地", ["装设", "接地线"]),
            ("D1-S03-M05", "标志牌", ["悬挂", "开关", "禁止合闸,线路有人工作", "杆塔下方"]),
            ("D1-S03-M06", "班组自装接地", ["开关"]),
        ])
        first = t["required_measures"][0]
        self.assertEqual(list(first), ["id", "category", "pattern", "keywords", "must"])
        self.assertEqual(first["pattern"], "{线路}测试{编号}开关{编号}刀闸")
        self.assertTrue(all(m["must"] for m in t["required_measures"]))

    def test_subsumption_and_optional(self):
        ticket = Ticket(TicketType.DIST_1, measures=[
            SafetyMeasure("应投切电源及二次回路", "退出甲01开关合闸压板（无合闸压板，则无需填写此项）", 1),
            SafetyMeasure("应投切电源及二次回路", "退出乙02开关合闸压板", 2),
            SafetyMeasure("保留带电设备", "甲01开关母线侧带电", 1),
            SafetyMeasure("保留带电设备", "退出甲线所在馈线组自愈（若线路未投自愈，则不用写）", 2),
            SafetyMeasure("保留带电设备", "带电", 3),
            SafetyMeasure("其他", "无关文字", 1),
        ])
        got = required_measures(ticket, LEX, "X")
        self.assertEqual([(m["id"], m["keywords"], m["must"]) for m in got], [
            ("X-M01", ["退出", "开关", "合闸压板"], False),
            ("X-M02", ["开关", "带电"], True),
            ("X-M03", ["退出", "自愈"], False),
        ])

    def test_superset_replaces_and_appends(self):
        ticket = Ticket(TicketType.DIST_1, measures=[
            SafetyMeasure("应拉开关刀闸熔断器", "甲01开关", 1),
            SafetyMeasure("应拉开关刀闸熔断器", "甲02令克", 2),
            SafetyMeasure("应拉开关刀闸熔断器", "甲01开关1刀闸", 3),
        ])
        got = required_measures(ticket, LEX, "Y")
        self.assertEqual([m["keywords"] for m in got], [["令克"], ["开关", "刀闸"]])


class WorkTaskTest(unittest.TestCase):
    def test_same_line_label(self):
        text = "工作任务：（XX分局）35kV甲站10kV测试线配合：更换开关。\n二、作业线路图：\n" + D1_B
        t = build_template(ScenarioChunk(SECTION, "12", "乙（新增）", 0, text), parse_dist1(D1_B), LEX)
        self.assertEqual(t["work_task"], "（XX分局）{站}{线路}配合：更换开关。")
        self.assertEqual(t["source"]["status"], "新增")

    def test_fallback_to_ticket_task(self):
        t = build_template(ScenarioChunk(SECTION, "7", "丙", 0, D1_B), parse_dist1(D1_B), LEX)
        self.assertEqual(t["work_task"], "（XX分局）{站}{线路}配合：更换开关。")
        self.assertEqual(t["source"]["status"], "")


class LoadAndCliTest(unittest.TestCase):
    def test_cli_and_load(self):
        text = f"二、{SECTION}\n场景1：甲（新增）\n{D1_A}\n场景2：乙\n{D1_B}\n三、配电第二种工作票典型场景\n场景1：丁\n"
        with tempfile.TemporaryDirectory() as d:
            src = Path(d) / "典票 2099.1.txt"
            src.write_text(text, encoding="utf-8")
            out = Path(d) / "tpl"
            buf = io.StringIO()
            with redirect_stdout(buf):
                code = main(["templates", str(src), str(out), "--lexicon", str(LEX_PATH)])
            self.assertEqual(code, 0)
            self.assertIn("已生成 2 个模板", buf.getvalue())
            self.assertEqual(sorted(p.name for p in out.glob("*.json")), ["D1-S01.json", "D1-S02.json"])
            loaded = load_templates(out)
            self.assertEqual([t["id"] for t in loaded], ["D1-S01", "D1-S02"])
            self.assertIn("{线路}", (out / "D1-S01.json").read_text(encoding="utf-8"))

    def test_load_missing_field(self):
        with tempfile.TemporaryDirectory() as d:
            (Path(d) / "bad.json").write_text(json.dumps({"id": "D1-S99"}), encoding="utf-8")
            with self.assertRaisesRegex(ValueError, "required_measures"):
                load_templates(d)


if __name__ == "__main__":
    unittest.main()
