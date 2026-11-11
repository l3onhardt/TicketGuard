"""T-001 验收测试（规划者编写，实现方不得修改）。"""

import sys
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "src"))

from ticketguard.ingest import split_scenarios  # noqa: E402

FIXTURE = """\
目  录
二、配电第一种工作票典型场景12
场景1：单辐射开关后段线路停电作业（有调整）12
场景2：开闭所开关后段线路停电作业16
三、配电第二种工作票典型场景105
前言里的说明文字
二、配电第一种工作票典型场景
场景1：单辐射开关后段线路停电作业（有调整）
一、工作任务：
更换10kV四镇线四镇街公变。
[TABLE]
编号：2024-04-P1-001
[/TABLE]
场景2：开闭所开关后段线路停电作业
本场景操作参照场景1：执行。
三、配电第二种工作票典型场景
场景1：在杆塔上与带电导线距离大于0.7米的工作
内容A
[/TABLE]
6044060440110kV南湖站110kV南湖站场景2：在电缆沟内作业（有调整）
内容B
四、紧急抢修工作票典型场景
场景1：单回路自动化支线开关后线路故障抢修作业
场景2: 公配变紧急抢修（有调整）
内容C
六、书面布置票典型场景
场景1：作业点在带电线路通道内的工作
场景2：核相作业
场景2-1 一次核相
内容D
场景2-2 二次核相（新增）
场景3：线路清障
七、典型操作场景
场景1：变电站与开闭所之间线路停送电
"""


class SplitScenariosTest(unittest.TestCase):
    def setUp(self):
        self.chunks = split_scenarios(FIXTURE)
        self.keys = [(c.section, c.number) for c in self.chunks]

    def test_order_and_sections(self):
        self.assertEqual(
            self.keys,
            [
                ("配电第一种工作票典型场景", "1"),
                ("配电第一种工作票典型场景", "2"),
                ("配电第二种工作票典型场景", "1"),
                ("配电第二种工作票典型场景", "2"),
                ("紧急抢修工作票典型场景", "1"),
                ("紧急抢修工作票典型场景", "2"),
                ("书面布置票典型场景", "1"),
                ("书面布置票典型场景", "2"),
                ("书面布置票典型场景", "2-1"),
                ("书面布置票典型场景", "2-2"),
                ("书面布置票典型场景", "3"),
                ("典型操作场景", "1"),
            ],
        )

    def test_toc_is_ignored(self):
        self.assertNotIn("12", self.chunks[0].title)
        self.assertEqual(self.chunks[0].start_line, 7)

    def test_body_text(self):
        first = self.chunks[0]
        self.assertEqual(first.title, "单辐射开关后段线路停电作业（有调整）")
        self.assertIn("编号：2024-04-P1-001", first.text)
        self.assertNotIn("场景2", first.text)

    def test_reference_is_not_heading(self):
        self.assertIn("参照场景1", self.chunks[1].text)

    def test_heading_with_garbage_prefix(self):
        c = self.chunks[3]
        self.assertEqual(c.title, "在电缆沟内作业（有调整）")
        self.assertEqual(c.text, "内容B")

    def test_ascii_colon_and_sub_numbers(self):
        self.assertEqual(self.chunks[5].title, "公配变紧急抢修（有调整）")
        self.assertEqual(self.chunks[7].text, "")
        self.assertEqual(self.chunks[8].title, "一次核相")
        self.assertEqual(self.chunks[9].title, "二次核相（新增）")


REAL = ROOT / "data" / "interim" / "2026.9南宁供电局配电运检、配电自动化两票典型票库-2026版（试行）.txt"


@unittest.skipUnless(REAL.exists(), "真实典票文本不存在（先运行 ingest）")
class RealDocumentTest(unittest.TestCase):
    def test_scenario_counts(self):
        chunks = split_scenarios(REAL.read_text(encoding="utf-8"))
        counts: dict[str, int] = {}
        for c in chunks:
            counts[c.section] = counts.get(c.section, 0) + 1
        self.assertEqual(
            list(counts.values()), [21, 5, 4, 3, 29, 30], f"各段场景数：{counts}"
        )


if __name__ == "__main__":
    unittest.main()
