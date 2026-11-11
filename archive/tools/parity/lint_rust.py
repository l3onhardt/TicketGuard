"""对整个 Rust 源码跑一遍门禁里的禁用写法检查（不看 diff，看全量）。

    py -I tools/parity/lint_rust.py

门禁 tools/gate.py 只检查任务卡对应的新增行。Rust 实现是一次性移植进来的，
没有对应的任务卡，所以用这个脚本对全量代码补一次同样的检查。移植期工具。
"""
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "tools"))

import gate  # noqa: E402


def main() -> int:
    fails: list[str] = []
    warns: list[str] = []
    total = 0
    for path in sorted((ROOT / "rust").rglob("*.rs")):
        relative = path.relative_to(ROOT).as_posix()
        if not relative.startswith(("rust/ticketguard/", "rust/tg/")):
            continue
        lines = [l for l in path.read_text(encoding="utf-8").splitlines()
                 if l.strip() and not l.strip().startswith("//")]
        total += len(lines)
        core_only = gate.RUST_FORBIDDEN_IN_CORE if relative.startswith("rust/ticketguard/") else []
        gate.scan(relative, lines, gate.RUST_FORBIDDEN + core_only, gate.RUST_WARN, fails, warns)
    for manifest in [ROOT / "Cargo.toml", *sorted((ROOT / "rust").glob("*/Cargo.toml"))]:
        extra = gate.crate_names(manifest) - gate.ALLOWED_CRATES
        if extra:
            fails.append(f"{manifest.relative_to(ROOT).as_posix()}：依赖不在白名单 → {'、'.join(sorted(extra))}")
    print(f"Rust 运行代码 {total} 行（不含空行、注释、测试）")
    for w in warns:
        print("  [注意]", w)
    for f in fails:
        print("  [失败]", f)
    print("结果：", "通过" if not fails else "不通过")
    return 1 if fails else 0


if __name__ == "__main__":
    for stream in (sys.stdout, sys.stderr):
        stream.reconfigure(encoding="utf-8", errors="replace")
    raise SystemExit(main())
