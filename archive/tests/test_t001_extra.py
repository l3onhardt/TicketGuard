from __future__ import annotations

import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "src"))

from ticketguard.ingest import ScenarioChunk, split_scenarios


class ScenarioSequenceTest(unittest.TestCase):
    def test_missing_number_and_separator(self) -> None:
        text = "一、编造典型场景\n场景1：甲\n场景2无分隔符\n场景3：丙"
        chunks = split_scenarios(text)
        self.assertEqual(len(chunks), 1)
        self.assertEqual(chunks[0].text, "场景2无分隔符\n场景3：丙")

    def test_body_whitespace_and_empty_section(self) -> None:
        text = "一、编造典型场景\n场景1\t甲\n\n  正文  \n\n二、空典型场景\n说明"
        self.assertEqual(split_scenarios(text), [
            ScenarioChunk("编造典型场景", "1", "甲", 1, "\n  正文  \n")
        ])
