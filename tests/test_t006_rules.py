"""T-006 验收测试（规划者编写，实现方不得修改）。

每条规则都配一个应命中的样本和一个不应命中的样本（AGENTS.md §6），另有典票零误报回归。
"""

import copy
import json
import sys
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "src"))
sys.path.insert(0, str(ROOT / "tests"))

from fixtures_d1 import D1_A, D1_B  # noqa: E402
from ticketguard.ingest import parse_dist1, split_scenarios  # noqa: E402
from ticketguard.rules.engine import CHECKS, load_rules, run_rules, values  # noqa: E402
from ticketguard.schema import Person, Severity, Ticket, TicketType  # noqa: E402

RULES_DIR = ROOT / "data" / "rules"
INTERIM = ROOT / "data" / "interim"
REAL = INTERIM / "2026.9南宁供电局配电运检、配电自动化两票典型票库-2026版（试行）.txt"
NOTES = INTERIM / "2026.9南宁供电局配电运检、配电自动化两票典型票库-2026版（试行）.notes.json"
RULES = load_rules(RULES_DIR)


def _ids(ticket, rules=RULES):
    return [f.rule_id for f in run_rules(ticket, rules)]


def _rule(rule_id):
    return [r for r in RULES if r["id"] == rule_id]


def _a():
    return parse_dist1(D1_A)


def _hot(kind, start, end, signer="甲", leader="乙"):
    return Ticket(kind, plan_start=start, plan_end=end,
                  people=[Person(leader, "工作负责人"), Person(signer, "签发人")])


def _edit(ticket, **kw):
    t = copy.deepcopy(ticket)
    for k, v in kw.items():
        if k.startswith("f_"):
            t.fields[k[2:]] = v
        else:
            setattr(t, k, v)
    return t


class LoadTest(unittest.TestCase):
    def test_loads_all(self):
        self.assertEqual(sorted(r["id"] for r in RULES), [
            "R-CREW-001", "R-HOT-001", "R-HOT-002", "R-HOT-003", "R-LIVE-001", "R-LOC-001",
            "R-LOC-002", "R-NEW-001", "R-REQ-001", "R-REQ-002", "R-TIME-001", "R-TIME-002",
        ])
        self.assertEqual(sorted(CHECKS), sorted([
            "required_field", "role_required", "roles_disjoint", "before",
            "duration_max", "crew_count", "regex", "lines_subset",
        ]))

    def _bad(self, mutate, pattern):
        rule = copy.deepcopy(_rule("R-CREW-001")[0])
        mutate(rule)
        with tempfile.TemporaryDirectory() as d:
            (Path(d) / "x.rules.json").write_text(json.dumps([rule], ensure_ascii=False), encoding="utf-8")
            (Path(d) / "lexicon.json").write_text("{}", encoding="utf-8")
            with self.assertRaisesRegex(ValueError, pattern):
                load_rules(d)

    def test_validation(self):
        self._bad(lambda r: r.pop("message"), "R-CREW-001")
        self._bad(lambda r: r["basis"].pop("text"), "R-CREW-001")
        self._bad(lambda r: r["basis"].update(extra="x"), "R-CREW-001")
        self._bad(lambda r: r.update(check="eval"), "eval")
        self._bad(lambda r: r.update(status="done"), "R-CREW-001")
        self._bad(lambda r: r.update(severity="fatal"), "R-CREW-001")

    def test_only_rules_files(self):
        with tempfile.TemporaryDirectory() as d:
            (Path(d) / "lexicon.json").write_text('{"synonyms": {}, "keywords": []}', encoding="utf-8")
            self.assertEqual(load_rules(d), [])


class ValuesTest(unittest.TestCase):
    def test_paths(self):
        t = _a()
        self.assertEqual(values(t, "fields.签发时间"), ["2099-01-09T11:30"])
        self.assertEqual(values(t, "fields.会签时间"), [])
        self.assertEqual(values(t, "people.note"), ["新员工"])
        self.assertEqual(values(t, "work_tasks.地点"), ["35kV甲站10kV测试线乙支6号杆甲公变台架"])
        self.assertEqual(values(t, "crew_size"), [5])
        self.assertEqual(values(_edit(t, outage_scope=""), "outage_scope"), [])


class FindingShapeTest(unittest.TestCase):
    def test_draft_is_info_and_message(self):
        f = run_rules(_edit(_a(), crew_size=6), _rule("R-CREW-001"))
        self.assertEqual(len(f), 1)
        self.assertEqual(f[0].severity, Severity.INFO)
        self.assertTrue(f[0].needs_review)
        self.assertEqual(f[0].confidence, 1.0)
        self.assertEqual(f[0].message, "“工作负责人及工作班人员总人数”写的是 6 人，按工作负责人加工作班人员名单应为 5 人。")
        self.assertEqual(f[0].basis.clause, "5.3.1.3")

    def test_reviewed_keeps_severity(self):
        rule = copy.deepcopy(_rule("R-CREW-001"))
        rule[0]["status"] = "reviewed"
        f = run_rules(_edit(_a(), crew_size=6), rule)
        self.assertEqual(f[0].severity, Severity.ERROR)
        self.assertFalse(f[0].needs_review)

    def test_ticket_type_filter(self):
        self.assertEqual(_ids(_hot(TicketType.DIST_1, "2099-01-01T08:00", "2099-01-03T08:00", "甲", "甲"),
                              _rule("R-HOT-001") + _rule("R-HOT-003")), [])


class HitMissTest(unittest.TestCase):
    """每条规则：hit 样本必须报出该规则，miss 样本不得报出。"""

    def check(self, rule_id, hit, miss):
        rule = _rule(rule_id)
        self.assertIn(rule_id, _ids(hit, rule), f"{rule_id} 应命中")
        self.assertNotIn(rule_id, _ids(miss, rule), f"{rule_id} 不应命中")

    def test_crew(self):
        self.check("R-CREW-001", _edit(_a(), crew_size=6), _a())
        grouped = _edit(_a(), crew_size=10)
        grouped.people = [Person("张三", "工作负责人"), Person("张三", "工作班成员", note="第一组5人"),
                          Person("李四", "工作班成员", note="第二组5人")]
        self.assertEqual(_ids(grouped, _rule("R-CREW-001")), [])

    def test_required(self):
        f = run_rules(_edit(_a(), plan_end=None), _rule("R-REQ-001"))
        self.assertEqual([x.message for x in f], ["票面栏目 plan_end 未填写或无法识别。"])
        self.check("R-REQ-001", _edit(_a(), plan_end=None), _a())

    def test_roles(self):
        t = _a()
        t.people = [p for p in t.people if p.role != "接收人"]
        f = run_rules(t, _rule("R-REQ-002"))
        self.assertEqual([x.message for x in f], ["票面上没有找到接收人的签名。"])
        self.check("R-REQ-002", t, _a())

    def test_time_chain(self):
        hit = _edit(_a(), f_接收时间="2099-01-09T09:00")
        f = run_rules(hit, _rule("R-TIME-001"))
        self.assertEqual([x.message for x in f], ["签发时间（2099-01-09T11:30）晚于接收时间（2099-01-09T09:00），时间顺序不对。"])
        self.check("R-TIME-001", hit, _edit(_a(), f_接收时间=None))

    def test_plan_order(self):
        self.check("R-TIME-002", _edit(_a(), plan_end="2099-01-10T08:30"), _a())

    def test_new_staff(self):
        f = run_rules(_a(), _rule("R-NEW-001"))
        self.assertEqual([(x.message, x.evidence) for x in f],
                         [("工作班人员中有“新员工”，请确认已完成安全教育培训，并在监护人员监护下参加指定的工作。", "新员工")])
        self.check("R-NEW-001", _a(), parse_dist1(D1_B))

    def test_location_text(self):
        hit = _edit(_a(), work_tasks=[{"地点": "甲公变台架", "任务": "更换"}])
        self.check("R-LOC-001", hit, _a())

    def test_location_lines(self):
        hit = _edit(_a(), outage_scope="35kV甲站10kV示范线")
        f = run_rules(hit, _rule("R-LOC-002"))
        self.assertEqual(len(f), 1)
        self.assertIn("10kV测试线", f[0].message)
        self.assertEqual(f[0].evidence, "35kV甲站10kV测试线乙支6号杆甲公变台架")
        self.check("R-LOC-002", hit, parse_dist1(D1_B))

    def test_hot(self):
        h1 = TicketType.HOT_1
        self.check("R-HOT-001", _hot(h1, "2099-01-01T08:00", "2099-01-02T09:00"),
                   _hot(h1, "2099-01-01T08:00", "2099-01-02T08:00"))
        msg = run_rules(_hot(h1, "2099-01-01T08:00", "2099-01-02T08:30"), _rule("R-HOT-001"))[0].message
        self.assertEqual(msg, "一级动火工作票计划时间为 24.5 小时，超过有效期 24 小时。")
        h2 = TicketType.HOT_2
        self.check("R-HOT-002", _hot(h2, "2099-01-01T00:00", "2099-01-06T01:00"),
                   _hot(h2, "2099-01-01T00:00", "2099-01-06T00:00"))
        self.check("R-HOT-003", _hot(h1, None, None, "甲", "甲"), _hot(h1, None, None, "甲", "乙"))

    def test_live(self):
        hit = Ticket(TicketType.LIVE, people=[Person("甲", "工作负责人")])
        miss = Ticket(TicketType.LIVE, people=[Person("甲", "工作负责人"), Person("乙", "专责监护人")])
        self.check("R-LIVE-001", hit, miss)


@unittest.skipUnless(REAL.exists() and NOTES.exists(), "真实典票文本不存在（data/interim 未生成）")
class RealDocumentTest(unittest.TestCase):
    def test_basis_text_from_source(self):
        source = REAL.read_text(encoding="utf-8") + json.dumps(
            json.loads(NOTES.read_text(encoding="utf-8")), ensure_ascii=False)
        for r in RULES:
            self.assertIn(r["basis"]["text"], source, f"{r['id']} 的依据原文在典票库中找不到")

    def test_typical_tickets_no_false_positive(self):
        chunks = [c for c in split_scenarios(REAL.read_text(encoding="utf-8"))
                  if c.section == "配电第一种工作票典型场景"]
        found = [(c.number, f.rule_id) for c in chunks for f in run_rules(parse_dist1(c.text), RULES)]
        # 场景 13 的停电范围与工作地点不一致，是典票本身的问题（见 docs/08 待确认问题 7）
        self.assertEqual(found, [("13", "R-LOC-002")])


if __name__ == "__main__":
    unittest.main()
