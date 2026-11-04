"""任务验收门禁：py -I tools/gate.py T-001

对照任务卡（docs/tasks/T-xxx-*.md 的头部字段）机械检查 Codex 的产出：
改动范围、行数预算、受保护文件、禁用写法、依赖、测试。全部通过才进入人工评审。
"""

from __future__ import annotations

import fnmatch
import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]

# (正则, 说明) —— 只检查 src/ 下新增的代码行
FORBIDDEN = [
    (r"except\s*:", "裸 except"),
    (r"except\s*\(?\s*(Base)?Exception\b", "捕获 Exception 兜底"),
    (r"^\s*import\s+logging|^\s*from\s+logging\s", "引入 logging（用 print 到 stderr 或直接抛异常）"),
    (r"\b(TODO|FIXME|XXX)\b", "遗留 TODO"),
    (r"#\s*type:\s*ignore", "type: ignore"),
    (r"^\s*(import|from)\s+(socket|requests|httpx|aiohttp|urllib\.request|http\.client)\b", "网络库"),
]
WARN = [
    (r"\bisinstance\(", "isinstance（内部数据不需要类型防御）", 3),
    (r"\bhasattr\(|\bgetattr\([^)]*,[^)]*,", "hasattr / 带默认值的 getattr", 1),
    (r"\bclass\s+\w+\((ABC|Protocol)\)|@abstractmethod", "新抽象层", 1),
    (r"^\s*except\b", "try/except 块", 2),
]


def sh(*args: str) -> str:
    return subprocess.run(args, cwd=ROOT, capture_output=True, text=True, encoding="utf-8", check=True).stdout


def load_card(task_id: str) -> tuple[Path, dict]:
    cards = sorted((ROOT / "docs" / "tasks").glob(f"{task_id}-*.md"))
    if len(cards) != 1:
        sys.exit(f"找不到唯一的任务卡 docs/tasks/{task_id}-*.md（找到 {len(cards)} 个）")
    head = cards[0].read_text(encoding="utf-8").split("---")[1]
    meta: dict = {}
    key = None
    for line in head.splitlines():
        if m := re.match(r"^(\w+):\s*(.*)$", line):
            key, val = m.groups()
            meta[key] = val.strip() if val.strip() else []
        elif m := re.match(r"^\s+-\s+(.+)$", line):
            meta[key].append(m.group(1).strip())
    return cards[0], meta


def changed_files(base: str) -> list[str]:
    tracked = sh("git", "diff", "--name-only", base).split()
    untracked = sh("git", "ls-files", "--others", "--exclude-standard").split()
    return sorted(set(tracked + untracked))


def added_lines(base: str, path: str) -> list[str]:
    if sh("git", "ls-files", path).strip():
        diff = sh("git", "diff", "-U0", base, "--", path)
        lines = [l[1:] for l in diff.splitlines() if l.startswith("+") and not l.startswith("+++")]
    else:
        lines = (ROOT / path).read_text(encoding="utf-8").splitlines()
    return [l for l in lines if l.strip() and not l.strip().startswith("#")]


def main(task_id: str) -> int:
    card, meta = load_card(task_id)
    base_file = ROOT / ".codex-runs" / task_id / "base"
    base = base_file.read_text().strip() if base_file.exists() else "HEAD"
    allow = meta.get("allow", []) + [str(card.relative_to(ROOT)).replace("\\", "/")]
    protect = meta.get("protect", [])
    fails: list[str] = []
    warns: list[str] = []

    files = changed_files(base)
    for f in files:
        if not any(fnmatch.fnmatch(f, pat) for pat in allow):
            fails.append(f"越界改动：{f}（不在 allow 列表）")
        if f in protect:
            fails.append(f"改动了受保护文件：{f}")
        if f == "pyproject.toml":
            fails.append("改动了 pyproject.toml（不得新增依赖）")

    src_added = tests_added = 0
    for f in files:
        if not f.endswith(".py") or not (ROOT / f).exists():
            continue
        lines = added_lines(base, f)
        if f.startswith("src/"):
            src_added += len(lines)
            text = "\n".join(lines)
            for pat, why in FORBIDDEN:
                for l in lines:
                    if re.search(pat, l):
                        fails.append(f"{f}：{why} → {l.strip()[:80]}")
            for pat, why, limit in WARN:
                n = len(re.findall(pat, text, re.M))
                if n >= limit:
                    warns.append(f"{f}：{why} ×{n}")
        elif f.startswith("tests/"):
            tests_added += len(lines)

    budget_src, budget_tests = int(meta.get("budget_src", 0)), int(meta.get("budget_tests", 0))
    if src_added > budget_src:
        fails.append(f"src 新增 {src_added} 行，超出预算 {budget_src}")
    if tests_added > budget_tests:
        fails.append(f"tests 新增 {tests_added} 行，超出预算 {budget_tests}")

    test = subprocess.run(
        [sys.executable, "-I", "-m", "unittest", "discover", "-s", "tests"],
        cwd=ROOT, capture_output=True, text=True, encoding="utf-8",
    )
    if test.returncode != 0:
        fails.append("测试未通过：\n" + test.stderr[-1500:])

    print(f"== 门禁 {task_id} ({card.name}) 基线 {base[:8]}")
    print(f"改动文件 {len(files)} 个；src +{src_added}/{budget_src} 行；tests +{tests_added}/{budget_tests} 行")
    for w in warns:
        print("  [注意]", w)
    for f in fails:
        print("  [失败]", f)
    print("结果：", "通过 → 进入人工评审" if not fails else "不通过 → 退回修改")
    return 1 if fails else 0


if __name__ == "__main__":
    for stream in (sys.stdout, sys.stderr):
        stream.reconfigure(encoding="utf-8", errors="replace")
    if len(sys.argv) != 2:
        sys.exit("用法：py -I tools/gate.py T-001")
    raise SystemExit(main(sys.argv[1]))
