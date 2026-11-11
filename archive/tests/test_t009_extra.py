from __future__ import annotations

import sys
import unittest
from pathlib import Path
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "src"))
from ticketguard.evaluate import evaluate
from ticketguard.review import CMP_BASIS, Review
from ticketguard.schema import Finding, Severity, Ticket, TicketType


class EvaluationBoundaryTest(unittest.TestCase):
    def test_multiset_subtraction_and_original_review_once(self) -> None:
        def finding(message: str, evidence: str, scenario: str | None = None) -> Finding:
            return Finding("R", Severity.WARNING, message, CMP_BASIS, evidence, scenario_id=scenario)
        old = [finding("原提示", "旧证据"), finding("原措施", "M1", "D1-S01")]
        got = [finding("新提示", "新证据"), finding("重复规则", "另一证据"),
               finding("换措辞", "M1", "D1-S02"), finding("重复措施", "M1", "D1-S01"),
               finding("新增措施", "M2", "D1-S01")]
        sample = {"base_scenario": "D1-S01", "mutations": [{"op": "rename_lines"}],
                  "ticket_text": "合成样本", "expected_findings": []}
        def review(text: str, rules: list, templates: list, lexicon: dict) -> Review:
            return Review(Ticket(TicketType.DIST_1), [("D1-S01", 1.0)], old if text == "原票" else got)
        with patch("ticketguard.evaluate.review_text", side_effect=review) as mocked:
            report = evaluate([dict(sample, id="a"), dict(sample, id="b")], {"D1-S01": "原票"}, [], [], {})
        self.assertEqual(mocked.call_count, 3)
        self.assertEqual(report["unexpected"], 6)
        self.assertEqual([u["message"] for u in report["unexpected_findings"]],
                         ["重复规则", "重复措施", "新增措施"] * 2)
        self.assertEqual((report["recall"], report["recall_ci95"]), (None, None))

    def test_empty_samples(self) -> None:
        report = evaluate([], {}, [], [], {})
        self.assertEqual((report["samples"], report["top1"], report["unexpected"]), (0, 0, 0))
        self.assertEqual((report["by_operator"], report["misses"], report["unexpected_findings"]), ({}, [], []))
        self.assertEqual((report["recall"], report["recall_ci95"]), (None, None))
