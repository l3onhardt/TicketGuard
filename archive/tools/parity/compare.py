"""对拍：同一张票分别交给 Python 参考实现和 Rust 实现，逐字节比较输出。

    py -I tools/parity/compare.py <tg.exe 路径> [最多几张]

仓库根目录由脚本位置推出。所有临时文件写到系统临时目录，不落仓库。
退出码非 0 表示有差异。

移植期工具：Python 参考实现删除时，本脚本一并删除。
"""
import json
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "src"))

from ticketguard.entities import load_lexicon  # noqa: E402
from ticketguard.report import build_report, render_html  # noqa: E402
from ticketguard.review import review_text  # noqa: E402
from ticketguard.rules.engine import load_rules  # noqa: E402
from ticketguard.templates import load_templates  # noqa: E402

SAMPLES = ROOT / "data/synthetic/d1.jsonl"


def main() -> int:
    exe = Path(sys.argv[1]).resolve() if len(sys.argv) > 1 else ROOT / "target/release/tg.exe"
    limit = int(sys.argv[2]) if len(sys.argv) > 2 else 0
    if not SAMPLES.exists():
        print(f"跳过：本地没有合成集（{SAMPLES.relative_to(ROOT)} 未生成）")
        return 0

    rules = load_rules(ROOT / "data/rules")
    templates = load_templates(ROOT / "data/templates/D1")
    lexicon = load_lexicon(ROOT / "data/rules/lexicon.json")

    work = Path(tempfile.mkdtemp(prefix="tg-parity-"))
    tickets, rust_out = work / "tickets", work / "rust"
    tickets.mkdir()
    rust_out.mkdir()

    samples = [json.loads(line) for line in SAMPLES.open(encoding="utf-8") if line.strip()]
    if limit:
        samples = samples[:limit]

    failed = json_diff = html_diff = 0
    first: list[str] = []
    for sample in samples:
        name = sample["id"]
        source = tickets / f"{name}.txt"
        source.write_text(sample["ticket_text"], encoding="utf-8", newline="")
        report = build_report(review_text(sample["ticket_text"], rules, templates, lexicon), templates, source.name)
        expected_json = json.dumps(report, ensure_ascii=False, indent=2)
        expected_html = render_html(report)

        done = subprocess.run([str(exe), "check", str(source), "--out", str(rust_out)], cwd=ROOT,
                              capture_output=True, text=True, encoding="utf-8")
        if done.returncode != 0:
            failed += 1
            if len(first) < 5:
                first.append(f"{name}: Rust 报错 → {done.stderr.strip()[:300]}")
            continue
        actual_json = (rust_out / f"{name}.预审.json").read_text(encoding="utf-8")
        actual_html = (rust_out / f"{name}.预审.html").read_text(encoding="utf-8")
        if actual_json.rstrip("\n") != expected_json:
            json_diff += 1
            if len(first) < 5:
                first.append(f"{name}: 报告 JSON 不一致")
        if actual_html != expected_html:
            html_diff += 1
            if len(first) < 5:
                for index, (expected, actual) in enumerate(zip(expected_html, actual_html)):
                    if expected != actual:
                        first.append(
                            f"{name}: HTML 第 {index} 个字符起不一致\n"
                            f"    参考: {expected_html[max(0, index - 60):index + 60]!r}\n"
                            f"    Rust: {actual_html[max(0, index - 60):index + 60]!r}")
                        break

    print(f"样本 {len(samples)} 张：Rust 报错 {failed}，JSON 不一致 {json_diff}，HTML 不一致 {html_diff}")
    for line in first:
        print(" -", line)
    print(f"临时文件：{work}")
    return 1 if (failed or json_diff or html_diff) else 0


if __name__ == "__main__":
    for stream in (sys.stdout, sys.stderr):
        stream.reconfigure(encoding="utf-8", errors="replace")
    raise SystemExit(main())
