from __future__ import annotations

from collections import Counter
from math import sqrt

from .review import review_text
from .schema import Finding


def wilson(k: int, n: int, z: float = 1.96) -> tuple[float, float]:
    if n == 0:
        return (0.0, 0.0)
    p = k / n
    d = 1 + z ** 2 / n
    c = (p + z ** 2 / (2 * n)) / d
    h = z * sqrt(p * (1 - p) / n + z ** 2 / (4 * n ** 2)) / d
    return (round(c - h, 4), round(c + h, 4))


def _key(finding: Finding) -> tuple:
    return (finding.rule_id, finding.evidence) if finding.scenario_id else (finding.rule_id,)


def _hit(finding: Finding, expected: dict) -> bool:
    return finding.rule_id == expected["rule_id"] and (
        "category" not in expected or f'“{expected["category"]}”栏' in finding.message)


def evaluate(samples: list[dict], originals: dict[str, str], rules: list[dict],
             templates: list[dict], lexicon: dict) -> dict:
    report = {"samples": 0, "top1": 0, "expected": 0, "detected": 0,
              "recall": None, "recall_ci95": None, "unexpected": 0,
              "by_operator": {}, "misses": [], "unexpected_findings": []}
    original_counts = {}
    for sample in samples:
        result = review_text(sample["ticket_text"], rules, templates, lexicon)
        base = sample["base_scenario"]
        if base not in original_counts:
            original_counts[base] = Counter(_key(f) for f in
                review_text(originals[base], rules, templates, lexicon).findings)
        remaining = original_counts[base].copy()
        expected = sample["expected_findings"]
        detected = unexpected = 0
        for expectation in expected:
            if any(_hit(f, expectation) for f in result.findings):
                detected += 1
            else:
                report["misses"].append({"id": sample["id"], "expected": expectation})
        for finding in result.findings:
            key = _key(finding)
            if remaining[key] > 0:
                remaining[key] -= 1
            elif not any(_hit(finding, e) for e in expected):
                unexpected += 1
                report["unexpected_findings"].append(
                    {"id": sample["id"], "rule_id": finding.rule_id, "message": finding.message})
        operator = sample["mutations"][0]["op"]
        stats = report["by_operator"].setdefault(operator,
            {"samples": 0, "top1": 0, "expected": 0, "detected": 0, "unexpected": 0})
        for target in (report, stats):
            target["samples"] += 1
            target["top1"] += int(result.candidates[0][0] == base)
            target["expected"] += len(expected)
            target["detected"] += detected
            target["unexpected"] += unexpected
    for stats in (report, *report["by_operator"].values()):
        stats["recall"] = round(stats["detected"] / stats["expected"], 4) if stats["expected"] else None
        stats["recall_ci95"] = wilson(stats["detected"], stats["expected"]) if stats["expected"] else None
    return report
