"""从参考实现（Python）导出 Rust 测试用的基准结果。

    py -I tools/parity/make_fixtures.py

输入只用编造的票面（tests/fixtures_d1.py 里的 D1_A、D1_B，不含典票内容），
所以产物可以入库。规则和同义词表取当前 data/rules 的快照，这样业务人员改规则
不会让基准测试变红；要更新快照就重新运行本脚本并审阅 diff。

本脚本和 tools/parity/compare.py 都是移植期工具，参考实现删除时一并删除。
"""
import json
import random
import shutil
import sys
from dataclasses import asdict
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "src"))
sys.path.insert(0, str(ROOT / "tests"))

from fixtures_d1 import D1_A, D1_B  # noqa: E402
from ticketguard import synth  # noqa: E402
from ticketguard.entities import abstract, keywords, lines, load_lexicon, normalize, poles, stations  # noqa: E402
from ticketguard.evaluate import evaluate, wilson  # noqa: E402
from ticketguard.ingest import parse_dist1, split_scenarios  # noqa: E402
from ticketguard.ingest.scenarios import ScenarioChunk  # noqa: E402
from ticketguard.report import build_report, draft_text, render_html  # noqa: E402
from ticketguard.review import review_text  # noqa: E402
from ticketguard.rules.engine import load_rules  # noqa: E402
from ticketguard.templates import build_template, load_templates  # noqa: E402

OUT = ROOT / "rust" / "tests" / "fixtures"


def write(path: Path, text: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text, encoding="utf-8", newline="")


def dump(value) -> str:
    return json.dumps(value, ensure_ascii=False, indent=2) + "\n"


def mutate(name: str, text: str, seed: int = 0) -> str:
    mutation = synth.OPERATORS[name](text, random.Random(seed))
    assert mutation is not None, f"算子 {name} 在编造票面上没有生效"
    return mutation[0]


def main() -> None:
    if OUT.exists():
        shutil.rmtree(OUT)
    # 规则与词表快照
    for path in sorted((ROOT / "data" / "rules").glob("*.json")):
        write(OUT / "rules" / path.name, path.read_text(encoding="utf-8").replace("\r\n", "\n"))
    lexicon = load_lexicon(OUT / "rules" / "lexicon.json")
    rules = load_rules(OUT / "rules")

    # 编造票面：两张原票 + 若干带缺陷的变体
    drop_ground = None
    for seed in range(50):  # 找一个恰好删掉“接地”栏的种子
        text, expected = synth.drop_category(D1_A, random.Random(seed))
        if expected[0]["category"] == "接地":
            drop_ground = text
            break
    assert drop_ground is not None
    unrelated = D1_B.replace("更换开关", "砍伐树障").replace(
        "应拉断路器（负荷开关）、隔离开关（刀闸）和熔断器：1、10kV样例1号开闭所样例902开关；2、10kV样例线样例03开关1刀闸。",
        "应拉断路器（负荷开关）、隔离开关（刀闸）和熔断器：无").replace(
        "1、10kV样例1号开闭所样例902开关母线侧带电！2、邻近带电体要保持足够安全距离。", "无")
    tickets = {
        "d1_a": D1_A,
        "d1_b": D1_B,
        "d1_a_drop_ground": drop_ground,
        "d1_a_crew_off": mutate("crew_off", D1_A),
        "d1_a_time_swap": mutate("time_swap", D1_A),
        "d1_a_plan_swap": mutate("plan_swap", D1_A),
        "d1_a_outage_wrong": mutate("outage_wrong", D1_A),
        "d1_a_loc_strip": mutate("loc_strip", D1_A),
        "d1_a_drop_signer": mutate("drop_signer", D1_A),
        "d1_a_paraphrase": mutate("paraphrase", D1_A),
        "d1_a_rename_lines": mutate("rename_lines", D1_A),
        "d1_b_unrelated": unrelated,
    }
    for name, text in tickets.items():
        write(OUT / "tickets" / f"{name}.txt", text)

    # 模板：由两张原票各生成一个
    chunks = {
        "d1_a": ScenarioChunk("配电第一种工作票典型场景", "1", "单台公变更换（有调整）", 0, D1_A),
        "d1_b": ScenarioChunk("配电第一种工作票典型场景", "2", "开闭所间隔检修", 0, D1_B),
    }
    for name, chunk in chunks.items():
        template = build_template(chunk, parse_dist1(chunk.text), lexicon)
        write(OUT / "templates" / f"{template['id']}.json", dump(template))
    templates = load_templates(OUT / "templates")

    golden = {"parse": {}, "review": {}, "entities": [], "wilson": [], "scenarios": {}}
    for name, text in tickets.items():
        golden["parse"][name] = asdict(parse_dist1(text))
        review = review_text(text, rules, templates, lexicon)
        report = build_report(review, templates, f"{name}.txt")
        golden["review"][name] = {
            "candidates": [[i, s] for i, s in review.candidates],
            "report": report,
            "draft": draft_text(review.findings),
        }
        write(OUT / "reports" / f"{name}.html", render_html(report))

    # 实体抽取：编造的句子，覆盖断言、括号、编号、杆号的各种写法
    sentences = [
        "35kV甲站10kV测试线乙支6号杆甲公变台架",
        "110kV乙站10kV样例线： / 1、10kV样例1号开闭所样例902开关间隔； / 2、10kV样例线3号杆。",
        "在10kV测试线测试01开关所在杆塔下方明显可视位置悬挂“禁止合闸，线路有人工作！”标志牌",
        "断开10kV测试线测试01开关操作电源空开；退出10kV测试线测试01开关合闸连接片",
        "拉开10kV样例线样例03开关1隔离开关及10kV样例线901线跌落式熔断器",
        "10kV测试线#12杆至#15+1杆（含10kV备用线T接点）",
        "0.4kV低压出线、10kV母线、10kV样例1号开闭所进线、1.5kV试验线",
        "合上10kV示范线示范90238接地刀闸　并装设遮栏（围栏）",
        "无",
    ]
    for sentence in sentences:
        golden["entities"].append({
            "text": sentence,
            "normalize": normalize(sentence, lexicon),
            "lines": sorted(lines(sentence)),
            "stations": sorted(stations(sentence)),
            "poles": poles(sentence),
            "abstract": abstract(sentence),
            "keywords": keywords(sentence, lexicon),
        })
    for k, n in [(0, 0), (0, 5), (5, 5), (205, 207), (59, 61), (1, 3)]:
        golden["wilson"].append({"k": k, "n": n, "ci": list(wilson(k, n))})

    # 场景切分：编造的正文
    body = "\n".join([
        "前言", "场景1：不在段落里，不算",
        "一、配电第一种工作票典型场景", "场景1：甲（有调整）", "正文A", "场景3：编号不连续，不算", "场景2 乙", "正文B",
        "二、配电倒闸典型操作场景", "场景1:丙", "场景1-1：子场景", "正文C", "场景1-2：另一个子场景", "场景2：丁（新增）", "正文D",
    ])
    write(OUT / "scenarios.txt", body)
    golden["scenarios"] = [asdict(c) for c in split_scenarios(body)]

    # 评测：把带缺陷的变体当样本
    expected = {
        "d1_a_drop_ground": ("drop_category", [{"rule_id": "CMP-MISSING", "category": "接地"}]),
        "d1_a_crew_off": ("crew_off", [{"rule_id": "R-CREW-001"}]),
        "d1_a_time_swap": ("time_swap", [{"rule_id": "R-TIME-001"}]),
        "d1_a_plan_swap": ("plan_swap", [{"rule_id": "R-TIME-002"}]),
        "d1_a_outage_wrong": ("outage_wrong", [{"rule_id": "R-LOC-002"}]),
        "d1_a_loc_strip": ("loc_strip", [{"rule_id": "R-LOC-001"}]),
        "d1_a_drop_signer": ("drop_signer", [{"rule_id": "R-REQ-002"}]),
        "d1_a_paraphrase": ("paraphrase", []),
        "d1_a_rename_lines": ("rename_lines", []),
        # 故意放一个检不出的期望，覆盖“漏检”分支
        "d1_a": ("crew_off", [{"rule_id": "R-CREW-001"}]),
    }
    samples = [{"id": f"syn-{name}", "base_scenario": "D1-S01", "mutations": [{"op": op}],
                "ticket_text": tickets[name], "expected_findings": findings}
               for name, (op, findings) in expected.items()]
    write(OUT / "eval_samples.jsonl", "".join(json.dumps(s, ensure_ascii=False) + "\n" for s in samples))
    golden["eval"] = evaluate(samples, {"D1-S01": D1_A}, rules, templates, lexicon)

    write(OUT / "golden.json", dump(golden))
    print(f"已写出基准结果 → {OUT.relative_to(ROOT)}")
    for name in tickets:
        report = golden["review"][name]["report"]
        ids = [f["rule_id"] for f in report["findings"]]
        top = report["candidates"][0]
        print(f"  {name}: 最接近 {top['id']}（{top['score']}），发现 {ids}")


if __name__ == "__main__":
    for stream in (sys.stdout, sys.stderr):
        stream.reconfigure(encoding="utf-8", errors="replace")
    main()
