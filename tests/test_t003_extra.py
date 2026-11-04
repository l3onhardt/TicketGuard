from __future__ import annotations

import sys
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
sys.path[:0] = [str(ROOT / "src"), str(ROOT / "tests")]

from fixtures_d1 import D1_B
from ticketguard.ingest import parse_datetime, parse_dist1
from ticketguard.ingest.ticket_parser import ticket_block
from ticketguard.schema import SafetyMeasure


class ExtraParserTest(unittest.TestCase):
    def test_block_and_missing_columns(self):
        self.assertTrue(ticket_block("前文\n　 " + D1_B).endswith("[/TABLE]"))
        for text, message in (("无票面", "编号"), ("编号：合成票", r"\[TABLE\]"),
                              ("编号：合成票\n[TABLE]", r"\[/TABLE\]")):
            with self.subTest(text=text), self.assertRaisesRegex(ValueError, message):
                ticket_block(text)
        for label in ("工作负责人（监护人）：", "工作地点或设备（注明", "停电设备或线路名称：",
                      "调度或设备运维单位负责的安全措施", "工作班组自行装设的接地线"):
            with self.subTest(label=label), self.assertRaises(ValueError) as error:
                parse_dist1(D1_B.replace(label, "缺失栏目"))
            self.assertEqual(str(error.exception), f"票面缺少栏目：{label}")

    def test_header_variants(self):
        text = D1_B.replace("XX分局XX班", "XX分局/XX班").replace("☑是", "□是")
        text = text.replace("周X、吴X", "周X(新员工)；吴X（厂家技术人员）。")
        ticket = parse_dist1(text)
        self.assertEqual(ticket.fields["单位班组"], "XX分局/XX班")
        self.assertIsNone(ticket.fields["分组派工单"])
        self.assertEqual([p.note for p in ticket.people[1:3]], ["新员工", "厂家技术人员"])
        self.assertEqual(parse_datetime("2099 年 2 月 3 日 4 时 5 分"), "2099-02-03T04:05")

    def test_optional_fields(self):
        lines = [line for line in D1_B.splitlines() if not line.startswith(
            ("是否办理", "工作班人员", "签发 |", "接收 |", "工作许可 |", "安全交代 |", "备注"))]
        text = "\n".join(lines).replace("总人数共 3 人", "人数未填")
        text = text.replace("电话：15900000000 / ", "").replace("单位和班组：XX分局XX班 / ", "")
        text = text.replace("自2099年02月02日10时30分 / 至2099年02月02日16时30分", "")
        ticket = parse_dist1(text)
        self.assertEqual([p.role for p in ticket.people], ["工作负责人"])
        self.assertEqual((ticket.crew_size, ticket.plan_start, ticket.plan_end), (None, None, None))
        self.assertTrue(all(value is None for value in ticket.fields.values()))

    def test_numbered_measures_and_cell_order(self):
        text = D1_B.replace("1、10kV样例1号开闭所样例902开关；2、10kV样例线样例03开关1刀闸。",
                            "（1）合成甲;（2）合成乙。(3)合成丙；4．合成丁 / 无。")
        text = text.replace("| 无 | 无 | 无", "| 无 | 合成 / 接地 | /")
        measures = parse_dist1(text).measures
        self.assertEqual([m.text for m in measures[:4]], ["合成甲", "合成乙", "合成丙", "合成丁"])
        self.assertEqual([m.order for m in measures[:4]], [1, 2, 3, 4])
        self.assertEqual(measures[-1], SafetyMeasure("班组自装接地", "合成接地", 2))
