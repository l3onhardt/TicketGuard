"""T-003 验收测试（规划者编写，实现方不得修改）。"""

import sys
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "src"))
sys.path.insert(0, str(ROOT / "tests"))

from fixtures_d1 import D1_A, D1_B  # noqa: E402
from ticketguard.ingest import parse_datetime, parse_dist1, split_scenarios  # noqa: E402
from ticketguard.schema import Person, SafetyMeasure, TicketType  # noqa: E402

REAL = ROOT / "data" / "interim" / "2026.9南宁供电局配电运检、配电自动化两票典型票库-2026版（试行）.txt"


def _people(t):
    return [(p.role, p.name, p.signed_by, p.note) for p in t.people]


class DatetimeTest(unittest.TestCase):
    def test_parse(self):
        self.assertEqual(parse_datetime("时间：　2099年　1月　2日　8时　5分"), "2099-01-02T08:05")
        self.assertEqual(parse_datetime("自2099年01月10日08时30分 / 至2099年01月10日17时30分"), "2099-01-10T08:30")
        self.assertIsNone(parse_datetime("时间：/ 年　/ 月 / 日 / 时 / 分"))


class TicketATest(unittest.TestCase):
    def setUp(self):
        self.t = parse_dist1(D1_A)

    def test_header(self):
        t = self.t
        self.assertEqual(t.ticket_type, TicketType.DIST_1)
        self.assertEqual(t.number, "2099-01-P1-001")
        self.assertEqual(t.crew_size, 5)
        self.assertEqual((t.plan_start, t.plan_end), ("2099-01-10T08:30", "2099-01-10T17:30"))
        self.assertEqual(t.fields["电话"], "13800000000")
        self.assertEqual(t.fields["单位班组"], "XX所XX班")
        self.assertEqual(t.fields["分组派工单"], "否")

    def test_people(self):
        self.assertEqual(_people(self.t), [
            ("工作负责人", "张三", None, None),
            ("工作班成员", "王五", None, None),
            ("工作班成员", "赵六", None, "新员工"),
            ("工作班成员", "钱七", None, None),
            ("工作班成员", "曾八", None, None),
            ("签发人", "李四", None, None),
            ("接收人", "黄X", None, None),
            ("许可人", "李某", "张三", None),
            ("专责监护人", "曾八", None, None),
        ])

    def test_times(self):
        f = self.t.fields
        self.assertEqual(f["签发时间"], "2099-01-09T11:30")
        self.assertIsNone(f["会签时间"])
        self.assertEqual(f["接收时间"], "2099-01-09T15:30")
        self.assertEqual(f["许可时间"], "2099-01-10T08:40")
        self.assertEqual(f["安全交代时间"], "2099-01-10T08:50")

    def test_block_excludes_dispatch_sheet(self):
        self.assertTrue(self.t.raw_text.startswith("编号："))
        self.assertTrue(self.t.raw_text.endswith("[/TABLE]"))
        self.assertNotIn("派工单里的备注", self.t.raw_text)
        self.assertEqual(self.t.fields["备注"], "1、在工作地点周围装设遮栏（围栏），并悬挂“止步，高压危险！”标志牌。")

    def test_tasks_and_scope(self):
        self.assertEqual(self.t.work_tasks, [{
            "地点": "35kV甲站10kV测试线乙支6号杆甲公变台架",
            "任务": "（XX分局）35kV甲站10kV测试线配合：更换10kV测试线甲公变。",
        }])
        self.assertEqual(self.t.outage_scope, "35kV甲站10kV测试线")

    def test_measures(self):
        self.assertEqual(self.t.measures, [
            SafetyMeasure("应拉开关刀闸熔断器", "10kV测试线测试01开关", 1),
            SafetyMeasure("应拉开关刀闸熔断器", "10kV测试线测试01开关2刀闸", 2),
            SafetyMeasure("应投切电源及二次回路", "断开10kV测试线测试01开关操作电源空开", 1),
            SafetyMeasure("应投切电源及二次回路", "退出10kV测试线测试01开关合闸压板", 2),
            SafetyMeasure("接地", "在10kV测试线7号杆小号侧装设接地线1组。（共1组接地线）", 1),
            SafetyMeasure("标志牌", "在10kV测试线测试01开关所在杆塔下方明显可视位置悬挂“禁止合闸，线路有人工作！”标志牌", 1),
            SafetyMeasure("班组自装接地", "10kV测试线乙支6号杆小号侧", 1),
            SafetyMeasure("班组自装接地", "10kV测试线甲公变0.4kV401开关出线侧", 2),
        ])


class TicketBTest(unittest.TestCase):
    def setUp(self):
        self.t = parse_dist1(D1_B)

    def test_header_and_people(self):
        t = self.t
        self.assertEqual((t.number, t.crew_size), ("2099-02-P1-007", 3))
        self.assertEqual(t.fields["分组派工单"], "是")
        self.assertEqual(t.fields["会签时间"], "2099-02-01T22:40")
        self.assertEqual(t.fields["接收时间"], "2099-02-01T23:10")
        self.assertEqual(t.fields["许可时间"], "2099-02-02T10:40")
        self.assertEqual(t.fields["安全交代时间"], "2099-02-02T10:50")
        self.assertEqual(_people(t), [
            ("工作负责人", "齐X", None, None),
            ("工作班成员", "周X", None, None),
            ("工作班成员", "吴X", None, None),
            ("签发人", "侯X", None, None),
            ("会签人", "孙X", None, None),
            ("接收人", "杨X", None, None),
            ("许可人", "彭X", "齐X", None),
        ])

    def test_tasks_scope_measures(self):
        t = self.t
        self.assertEqual(t.work_tasks, [
            {"地点": "110kV乙站10kV样例线： / 1、10kV样例1号开闭所样例902开关间隔； / 2、10kV样例线3号杆。",
             "任务": "（XX分局）110kV乙站10kV样例线配合：更换开关。"},
            {"地点": "", "任务": "补充任务"},
        ])
        self.assertEqual(t.outage_scope, "110kV乙站10kV样例线、110kV乙站10kV示范线")
        self.assertEqual(t.measures, [
            SafetyMeasure("应拉开关刀闸熔断器", "10kV样例1号开闭所样例902开关", 1),
            SafetyMeasure("应拉开关刀闸熔断器", "10kV样例线样例03开关1刀闸", 2),
            SafetyMeasure("保留带电设备", "10kV样例1号开闭所样例902开关母线侧带电！", 1),
            SafetyMeasure("保留带电设备", "邻近带电体要保持足够安全距离", 2),
        ])
        self.assertEqual(t.fields["备注"], "无")


class ErrorTest(unittest.TestCase):
    def test_no_number(self):
        with self.assertRaises(ValueError):
            parse_dist1("工作负责人（监护人）：张三\n[TABLE]\n[/TABLE]")

    def test_missing_section(self):
        text = D1_B.replace("停电设备或线路名称：", "停电范围：")
        with self.assertRaisesRegex(ValueError, "停电设备或线路名称"):
            parse_dist1(text)


@unittest.skipUnless(REAL.exists(), "真实典票文本不存在（data/interim 未生成）")
class RealDocumentTest(unittest.TestCase):
    def test_all_d1_scenarios(self):
        chunks = [c for c in split_scenarios(REAL.read_text(encoding="utf-8"))
                  if c.section == "配电第一种工作票典型场景"]
        tickets = [parse_dist1(c.text) for c in chunks]
        self.assertEqual(len(tickets), 21)
        for c, t in zip(chunks, tickets):
            roles = {p.role for p in t.people}
            self.assertTrue({"工作负责人", "签发人", "接收人", "许可人"} <= roles, f"场景{c.number}：{roles}")
            self.assertIsNotNone(t.plan_start, f"场景{c.number}")
            self.assertIsNotNone(t.crew_size, f"场景{c.number}")
            self.assertTrue(t.work_tasks and t.outage_scope, f"场景{c.number}")
        self.assertEqual(sum(len(t.measures) for t in tickets), 187)


if __name__ == "__main__":
    unittest.main()
