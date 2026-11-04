"""命令行入口：python -m ticketguard <command>"""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path

from .entities import load_lexicon
from .ingest import blocks_to_text, parse_dist1, read_docx, read_docx_notes, read_xlsx, split_scenarios
from .templates import build_template


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

    args = ap.parse_args(argv)
    return args.func(args)


if __name__ == "__main__":
    raise SystemExit(main())
