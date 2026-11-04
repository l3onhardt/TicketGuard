from __future__ import annotations

import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "src"))
from ticketguard.rules.engine import CHECKS, values
from ticketguard.schema import Person, Ticket, TicketType


class ExtraChecksTest(unittest.TestCase):
    def test_before_skips_missing_and_compares_adjacent(self) -> None:
        ticket = Ticket(TicketType.DIST_1, fields={"a": "2099-01-02", "c": "2099-01-01", "d": "2099-01-01"})
        params = {"chain": ["fields.a", "fields.b", "fields.c", "fields.d"], "strict": False}
        self.assertEqual(CHECKS["before"](ticket, params), [
            {"a": "a", "b": "c", "a_time": "2099-01-02", "b_time": "2099-01-01"}])
        params["strict"] = True
        self.assertEqual(len(CHECKS["before"](ticket, params)), 2)

    def test_zero_and_partially_grouped_crew(self) -> None:
        ticket = Ticket(TicketType.DIST_1, crew_size=0, people=[
            Person("甲", "工作班成员", note="第一组 3 人"), Person("乙", "工作班成员")])
        self.assertEqual(values(ticket, "crew_size"), [0])
        self.assertEqual(CHECKS["crew_count"](ticket, {}), [{"actual": 0, "expected": 3}])
        ticket.crew_size = None
        self.assertEqual(CHECKS["crew_count"](ticket, {}), [])

    def test_lines_union_sorted_with_combined_evidence(self) -> None:
        ticket = Ticket(TicketType.DIST_1, work_tasks=[{"地点": "10kV测试线、10kV示范线"},
                        {"地点": "10kV样例线"}], outage_scope="10kV样例线")
        params = {"field": "work_tasks.地点", "within": ["outage_scope", "fields.补充范围"]}
        self.assertEqual(CHECKS["lines_subset"](ticket, params), [
            {"lines": "10kV测试线、10kV示范线", "evidence": "10kV测试线、10kV示范线；10kV样例线"}])
        ticket.fields["补充范围"] = "10kV测试线、10kV示范线"
        self.assertEqual(CHECKS["lines_subset"](ticket, params), [])
