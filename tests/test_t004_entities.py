"""T-004 验收测试（规划者编写，实现方不得修改）。"""

import json
import sys
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "src"))

from ticketguard.entities import (  # noqa: E402
    abstract, keywords, lines, load_lexicon, normalize, poles, stations,
)

LEX = load_lexicon(ROOT / "data" / "rules" / "lexicon.json")


class LexiconTest(unittest.TestCase):
    def test_missing_field(self):
        with tempfile.TemporaryDirectory() as d:
            p = Path(d) / "lex.json"
            p.write_text(json.dumps({"synonyms": {}}), encoding="utf-8")
            with self.assertRaisesRegex(ValueError, "keywords"):
                load_lexicon(p)

    def test_normalize(self):
        self.assertEqual(normalize("拉开 隔离开关", LEX), "拉开刀闸")
        self.assertEqual(normalize("悬挂“禁止合闸，线路有人工作！”", LEX), "悬挂“禁止合闸,线路有人工作!”")
        self.assertEqual(normalize("更换跌落式熔断器", LEX), "更换令克")

    def test_longest_variant_first(self):
        lex = {"synonyms": {"甲": ["乙"], "丙": ["乙丁"]}, "keywords": []}
        self.assertEqual(normalize("乙丁乙", lex), "丙甲")


class ExtractTest(unittest.TestCase):
    def test_lines(self):
        self.assertEqual(lines("35kV甲站10kV测试线乙支6号杆甲公变台架"), {"10kV测试线"})
        self.assertEqual(lines("10kV样例1号开闭所样例902开关"), set())
        self.assertEqual(lines("10kV测试线102号塔（与10kV示范线102号塔同塔架设）"), {"10kV测试线"})
        self.assertEqual(lines("新装10kV某某充电桩专变G1高压进线柜"), set())
        self.assertEqual(lines("配合10kV示范916线施工"), {"10kV示范线"})
        self.assertEqual(lines("10kV甲2号公变0.4kVII线3号杆"), {"0.4kVII线"})
        self.assertEqual(lines("110kV乙站10kV样例线、110kV乙站10kV示范线"), {"10kV样例线", "10kV示范线"})

    def test_stations(self):
        self.assertEqual(stations("35kV甲站10kV测试线"), {"35kV甲站"})
        self.assertEqual(stations("10kV样例1号开闭所"), set())

    def test_poles(self):
        self.assertEqual(poles("10kV测试线5号杆至7号塔，乙支#9杆，1+1号杆"), ["5", "7", "9", "1+1"])


class AbstractTest(unittest.TestCase):
    def test_abstract(self):
        self.assertEqual(abstract("在35kV甲站10kV测试线7号杆小号侧装设接地线"), "在{站}{线路}{杆号}小号侧装设接地线")
        self.assertEqual(abstract("断开10kV测试线测试01开关操作电源空开"), "断开{线路}测试{编号}开关操作电源空开")
        self.assertEqual(abstract("10kV甲公变0.4kV401开关"), "10kV甲公变0.4kV{编号}开关")
        self.assertEqual(abstract("10kV样例1号开闭所样例90238接地刀闸（共用）"), "10kV样例1号开闭所样例{编号}接地刀闸（共用）")

    def test_keywords(self):
        self.assertEqual(keywords("断开10kV测试线测试01开关操作电源空开", LEX), ["断开", "开关", "操作电源空开"])
        self.assertEqual(keywords("退出测试01开关合闸连接片", LEX), ["退出", "开关", "合闸压板"])
        self.assertEqual(keywords("悬挂“禁止合闸，线路有人工作！”标志牌", LEX), ["悬挂", "禁止合闸,线路有人工作"])


if __name__ == "__main__":
    unittest.main()
