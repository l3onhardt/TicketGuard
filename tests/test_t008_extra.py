from __future__ import annotations

import random
import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "src"))
from ticketguard.ingest.scenarios import ScenarioChunk
from ticketguard.synth import OPERATORS, generate, loc_strip, rename_lines


class MutationBoundaryTest(unittest.TestCase):
    def test_first_label_line_cannot_fall_through(self) -> None:
        for name, first, second in (
            ("crew_off", "总人数共 /", "总人数共 5"),
            ("time_swap", "工作票接收人签名：/", "工作票接收人签名：甲 2099年1月1日1时1分"),
            ("drop_signer", "工作票接收人签名：/", "工作票接收人签名：甲"),
            ("outage_wrong", "停电设备或线路名称：35kV虚构站10kV虚构线", "停电设备或线路名称：其他"),
        ):
            with self.subTest(name=name):
                self.assertIsNone(OPERATORS[name](first + "\n" + second, random.Random(0)))

    def test_location_changes_only_first_cell(self) -> None:
        text = "工作地点或设备（注明站名） | 任务\r\n35kV甲站、110kV乙站10kV测试线 | 35kV甲站\r\n"
        result, _ = loc_strip(text, random.Random(0))
        self.assertEqual(result, "工作地点或设备（注明站名） | 任务\r\n、10kV测试线 | 35kV甲站\r\n")
        self.assertIsNone(loc_strip("工作地点或设备（注明站名）\n35kV甲站", random.Random(0)))

    def test_rename_longest_then_lexical_order(self) -> None:
        result, expected = rename_lines("10kV甲线，10kV乙甲线，10kV乙线；乙甲线甲线", random.Random(0))
        self.assertEqual(result, "10kV虚构3号线，10kV虚构1号线，10kV虚构2号线；虚构1号线虚构3号线")
        self.assertEqual(expected, [])

    def test_generation_shares_rng_and_caps_attempts(self) -> None:
        chunks = [ScenarioChunk("", str(i), "", 0, "总人数共 5") for i in (1, 2)]
        rng = random.Random(7)
        expected = []
        for _ in chunks:
            expected.extend(dict.fromkeys(f"总人数共 {5 + rng.choice([1, 2])}" for _ in range(3)))
        self.assertEqual([s["ticket_text"] for s in generate(chunks, 3, 7)], expected)
