from __future__ import annotations

import json
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "src"))
from ticketguard.entities import abstract, keywords, lines, load_lexicon, normalize, poles, stations


class EntityExtraTest(unittest.TestCase):
    def test_missing_synonyms(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "词表.json"
            path.write_text(json.dumps({"keywords": []}), encoding="utf-8")
            with self.assertRaises(ValueError) as error:
                load_lexicon(str(path))
            self.assertIn(str(path), str(error.exception))
            self.assertIn("synonyms", str(error.exception))

    def test_line_boundaries_and_filters(self) -> None:
        self.assertEqual(lines('0.4kVII线、10kV测试1234线、10kV测试916线'),
                         {'0.4kVII线', '10kV测试1234线', '10kV测试线'})
        for name in ('进线', '出线', '引线', '引流线', '接线', '母线', '导线', '地线', '跳线',
                     '甲开闭所乙线', '甲公变乙线', '甲专变乙线', '甲开关乙线', '甲刀闸乙线', '甲站乙线'):
            self.assertEqual(lines('10kV' + name), set())
        self.assertEqual(lines('10kV“甲线”、10kV甲/乙线、10kV甲，乙线'), set())

    def test_parentheses_and_repeated_poles(self) -> None:
        text = '35kV甲站（35kV乙站）(35kV丙站)10kV甲线(10kV乙线)'
        self.assertEqual(stations(text), {'35kV甲站'})
        self.assertEqual(stations('0.4kV甲站'), set())
        self.assertEqual(lines(text), {'10kV甲线'})
        self.assertEqual(abstract(text), '{站}（{站}）({站}){线路}({线路})')
        self.assertEqual(poles('#1+1杆、2号塔、#2、2号杆'), ['1+1', '2', '2', '2'])
        self.assertEqual(abstract(' 01接地刀闸，02开关、03刀闸、04令克！'),
                         ' {编号}接地刀闸，{编号}开关、{编号}刀闸、{编号}令克！')

    def test_keyword_normalization_and_order(self) -> None:
        lexicon = {"synonyms": {"刀闸": ["隔离开关"]}, "keywords": ["隔离开关", "甲，乙", "未出现"]}
        self.assertEqual(normalize(' \t隔离开关\n甲\u3000，乙！', lexicon), '刀闸甲,乙!')
        self.assertEqual(keywords('甲,乙刀闸', lexicon), ['隔离开关', '甲，乙'])
