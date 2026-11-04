from __future__ import annotations

import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "src"))

from ticketguard.review import compare_measures, match_scenario
from ticketguard.schema import SafetyMeasure, Ticket, TicketType


class ReviewBoundaryTest(unittest.TestCase):
    def test_empty_sets_score_zero(self) -> None:
        ticket = Ticket(TicketType.DIST_1)
        template = {"id": "S1", "title": "{线路}", "work_task": "", "required_measures": []}
        self.assertEqual(match_scenario(ticket, [template], {"synonyms": {}, "keywords": []}),
                         [("S1", 0.0)])

    def test_keywords_must_occur_in_one_measure(self) -> None:
        lexicon = {"synonyms": {}, "keywords": ["装设", "接地线"]}
        ticket = Ticket(TicketType.DIST_1, measures=[
            SafetyMeasure("接地", "装设"), SafetyMeasure("接地", "接地线"),
        ])
        template = {"id": "S1", "title": "合成场景", "required_measures": [
            {"id": "M1", "category": "接地", "keywords": ["装设", "接地线"],
             "must": True, "pattern": "装设接地线"},
        ]}
        self.assertEqual([f.evidence for f in compare_measures(ticket, template, lexicon, 0.8)], ["M1"])
        ticket.measures.append(SafetyMeasure("接地", "装设接地线"))
        ticket.measures.append(SafetyMeasure("其他", "多余的合成措施"))
        self.assertEqual(compare_measures(ticket, template, lexicon, 0.8), [])
