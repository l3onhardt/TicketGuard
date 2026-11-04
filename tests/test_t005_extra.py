from __future__ import annotations

import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "src"))
from ticketguard.ingest.scenarios import ScenarioChunk
from ticketguard.schema import SafetyMeasure, Ticket, TicketType
from ticketguard.templates import build_template, required_measures


class TemplateExtraTest(unittest.TestCase):
    def test_task_boundary_and_title_suffix(self) -> None:
        ticket = Ticket(TicketType.DIST_1, work_tasks=[{"任务": "回退任务"}])
        for text, expected in [("  编号：合成\n工作任务：不应取用", "回退任务"),
                               ("工作任务： 首项 \n工作任务：次项", "首项"),
                               ("工作任务：\n 下一行任务 ", "下一行任务"),
                               ("工作任务：", "")]:
            with self.subTest(text=text):
                chunk = ScenarioChunk("配电第一种工作票典型场景", "1", "  测试 (新增)  ", 0, text)
                template = build_template(chunk, ticket, {})
                self.assertEqual(template["work_task"], expected)
                self.assertEqual(template["title"], "测试")
                self.assertEqual(template["source"]["status"], "新增")
        chunk = ScenarioChunk("配电第一种工作票典型场景", "1", "测试", 0, "")
        self.assertEqual(build_template(chunk, Ticket(TicketType.DIST_1), {})["work_task"], "")

    def test_categories_and_optional_phrases(self) -> None:
        lexicon = {"synonyms": {}, "keywords": ["开关"]}
        phrases = ["则无需", "则不用", "无需填写", "不用写", "需要填写"]
        ticket = Ticket(TicketType.DIST_1, measures=[
            SafetyMeasure(str(index), "开关" + phrase) for index, phrase in enumerate(phrases)])
        measures = required_measures(ticket, lexicon, "X")
        self.assertEqual([measure["must"] for measure in measures], [False] * 4 + [True])
