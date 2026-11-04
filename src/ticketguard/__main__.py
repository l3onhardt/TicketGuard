"""命令行入口：python -m ticketguard <command>"""

from __future__ import annotations

import argparse
import json
import sys
from datetime import date
from pathlib import Path

from .entities import load_lexicon
from .evaluate import evaluate
from .ingest import blocks_to_text, parse_dist1, read_docx, read_docx_notes, read_xlsx, split_scenarios
from .report import build_report, render_html
from .review import review_text
from .rules.engine import load_rules
from .synth import generate
from .templates import build_template, load_templates


def cmd_ingest(args: argparse.Namespace) -> int:
    """把 data/raw 下的 docx / xlsx 转成 UTF-8 文本，写入 data/interim。"""
    src, out = Path(args.src), Path(args.out)
    out.mkdir(parents=True, exist_ok=True)
    n = 0
    for p in sorted(src.rglob("*")):
        if p.suffix.lower() == ".docx":
            # 文件名里常含 “2026.7.1”，不能用 with_suffix（会截断在第一个点之后）
            (out / f"{p.stem}.txt").write_text(blocks_to_text(read_docx(p)), encoding="utf-8")
            notes = read_docx_notes(p)
            if notes:
                (out / f"{p.stem}.notes.json").write_text(
                    json.dumps(notes, ensure_ascii=False, indent=2), encoding="utf-8"
                )
            n += 1
        elif p.suffix.lower() == ".xlsx":
            sheets = read_xlsx(p)
            lines = []
            for name, rows in sheets.items():
                lines.append(f"===== SHEET: {name}")
                lines += [" | ".join(c.replace("\n", " / ") for c in r) for r in rows]
            (out / (p.stem + ".txt")).write_text("\n".join(lines), encoding="utf-8")
            n += 1
        elif p.suffix.lower() in (".doc", ".xls"):
            print(f"[跳过] 旧格式需先另存为 docx/xlsx：{p.name}", file=sys.stderr)
    print(f"已转换 {n} 个文件 → {out}")
    return 0


def cmd_templates(args: argparse.Namespace) -> int:
    lexicon = load_lexicon(args.lexicon)
    chunks = split_scenarios(Path(args.src).read_text(encoding="utf-8"))
    out = Path(args.out)
    out.mkdir(parents=True, exist_ok=True)
    n = 0
    for chunk in chunks:
        if chunk.section == "配电第一种工作票典型场景":
            template = build_template(chunk, parse_dist1(chunk.text), lexicon)
            (out / f"{template['id']}.json").write_text(
                json.dumps(template, ensure_ascii=False, indent=2), encoding="utf-8")
            n += 1
    print(f"已生成 {n} 个模板 → {out}")
    return 0


def cmd_synth(args: argparse.Namespace) -> int:
    chunks = split_scenarios(Path(args.src).read_text(encoding="utf-8"))
    samples = generate([c for c in chunks if c.section == "配电第一种工作票典型场景"], args.per, args.seed)
    out = Path(args.out)
    out.parent.mkdir(parents=True, exist_ok=True)
    with out.open("w", encoding="utf-8") as stream:
        for sample in samples:
            stream.write(json.dumps(sample, ensure_ascii=False) + "\n")
    print(f"已生成 {len(samples)} 个样本 → {out}")
    return 0


def cmd_eval(args: argparse.Namespace) -> int:
    chunks = split_scenarios(Path(args.src).read_text(encoding="utf-8"))
    originals = {f"D1-S{int(c.number):02d}": c.text for c in chunks
                 if c.section == "配电第一种工作票典型场景"}
    with Path(args.samples).open(encoding="utf-8") as stream:
        samples = [json.loads(line) for line in stream if line.strip()]
    report = evaluate(samples, originals, load_rules(args.rules),
                      load_templates(args.templates), load_lexicon(args.lexicon))
    out = Path(args.out)
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(json.dumps(report, ensure_ascii=False, indent=2), encoding="utf-8")
    for op, stats in report["by_operator"].items():
        print(f"{op}：样本 {stats['samples']}，场景命中 {stats['top1']}，"
              f"检出 {stats['detected']}/{stats['expected']}，意外发现 {stats['unexpected']}")
    print(f"合计：样本 {report['samples']}，检出率 {report['recall']}"
          f"（95% 置信区间 {report['recall_ci95']}），意外发现 {report['unexpected']} → {out}")
    return 0


def cmd_check(args: argparse.Namespace) -> int:
    p = Path(args.src)
    suffix = p.suffix.lower()
    if suffix == ".docx":
        text = blocks_to_text(read_docx(p))
    elif suffix == ".txt":
        text = p.read_text(encoding="utf-8")
    else:
        raise ValueError(f"{p}：只支持 .txt（票面文本）和 .docx")
    templates = load_templates(args.templates)
    review = review_text(text, load_rules(args.rules), templates, load_lexicon(args.lexicon))
    report = build_report(review, templates, p.name)
    out = Path(args.out)
    out.mkdir(parents=True, exist_ok=True)
    page = out / f"{p.stem}.预审.html"
    (out / f"{p.stem}.预审.json").write_text(json.dumps(report, ensure_ascii=False, indent=2), encoding="utf-8")
    page.write_text(render_html(report), encoding="utf-8")
    top = report["candidates"][0]
    print(f"工作票 {report['ticket']['number'] or '未填写'}：最接近典票场景 {top['id']}「{top['title']}」（匹配度 {top['score']:.2f}）")
    print(f"疑似问题 {len(report['findings'])} 条（需人工确认）：")
    for finding in report["findings"]:
        print(f"  [{finding['label']}] {finding['message']}")
    print(f"报告 → {page}")
    return 0


def main(argv: list[str] | None = None) -> int:
    # Windows 控制台默认 GBK，统一成 UTF-8 输出，避免中文乱码
    for stream in (sys.stdout, sys.stderr):
        if hasattr(stream, "reconfigure"):
            stream.reconfigure(encoding="utf-8", errors="replace")
    ap =argparse.ArgumentParser(prog="ticketguard", description="配电两票离线智能预审")
    sub = ap.add_subparsers(dest="cmd", required=True)

    p = sub.add_parser("ingest", help="解析原始 docx/xlsx 为文本")
    p.add_argument("src", nargs="?", default="data/raw")
    p.add_argument("out", nargs="?", default="data/interim")
    p.set_defaults(func=cmd_ingest)

    p = sub.add_parser("templates", help="生成第一种票场景模板初稿")
    p.add_argument("src")
    p.add_argument("out", nargs="?", default="data/templates/D1")
    p.add_argument("--lexicon", default="data/rules/lexicon.json")
    p.set_defaults(func=cmd_templates)

    p = sub.add_parser("synth", help="生成带期望标签的合成票面样本")
    p.add_argument("src")
    p.add_argument("out", nargs="?", default="data/synthetic/d1.jsonl")
    p.add_argument("--per", type=int, default=5)
    p.add_argument("--seed", type=int, default=0)
    p.set_defaults(func=cmd_synth)

    p = sub.add_parser("eval", help="评测合成集的检出率、意外发现与场景命中")
    p.add_argument("src")
    p.add_argument("samples", nargs="?", default="data/synthetic/d1.jsonl")
    p.add_argument("--out", default=f"reports/eval-{date.today().isoformat()}.json")
    p.add_argument("--templates", default="data/templates/D1")
    p.add_argument("--rules", default="data/rules")
    p.add_argument("--lexicon", default="data/rules/lexicon.json")
    p.set_defaults(func=cmd_eval)

    p = sub.add_parser("check", help="预审单张工作票并生成报告")
    p.add_argument("src")
    p.add_argument("--out", default="reports")
    p.add_argument("--templates", default="data/templates/D1")
    p.add_argument("--rules", default="data/rules")
    p.add_argument("--lexicon", default="data/rules/lexicon.json")
    p.set_defaults(func=cmd_check)

    args = ap.parse_args(argv)
    return args.func(args)


if __name__ == "__main__":
    raise SystemExit(main())
