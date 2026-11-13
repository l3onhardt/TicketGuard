"""任务验收门禁：py -I tools/gate.py T-011

对照任务卡（docs/tasks/T-xxx-*.md 的头部字段）机械检查实现方的产出：
改动范围、行数预算、受保护文件、禁用写法、依赖白名单、格式、静态检查、测试。
全部通过才进入人工评审。只检查 Rust 代码（archive/ 下的 Python 参考实现不参与）。
"""

from __future__ import annotations

import fnmatch
import re
import shutil
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]

# Rust：只检查 rust/ticketguard、rust/tg 下新增的代码行
RUST_FORBIDDEN = [
    (r"\bunsafe\b", "unsafe"),
    (r"\b(todo|unimplemented)!\s*\(", "todo!() / unimplemented!()"),
    (r"\bdbg!\s*\(", "遗留 dbg!()"),
    (r"\b(TODO|FIXME|XXX)\b", "遗留 TODO"),
    (r"\b(reqwest|hyper|ureq|tokio|async_std|isahc|curl)::", "网络 / 异步库"),
    (r"\b(log|tracing)::", "日志库（命令行要输出就 println! / eprintln!）"),
    (r"#!?\[allow\(", "#[allow(...)] 压警告（改代码，不要压警告）"),
]
# 红线 5：运行时零联网，唯一的例外是本机 127.0.0.1：连本机的推理服务（ADR-0008）、给主界面开只听本机的页面服务（ADR-0009）。
# 只有 NET_FILES 这两个文件可以用 std::net，而且它们的全文要过 check_net_file。
RUST_NET = [(r"\bstd::net\b|\bTcpStream\b|\bTcpListener\b|\bUdpSocket\b", "网络（红线 5：运行时零联网）")]
NET_FILES = ["rust/tg/src/model.rs", "rust/tg/src/ui.rs"]
NET_FILE_FORBIDDEN = [
    (r"\bUdpSocket\b|\bToSocketAddrs\b|\blookup_host\b|\bto_socket_addrs\b|\bSocketAddr\b|\bIpv6Addr\b",
     "只能用 Ipv4Addr::LOCALHOST 连本机，不做主机名解析"),
    (r"\b(?!127\.0\.0\.1\b)\d{1,3}\.\d{1,3}\.\d{1,3}\.\d{1,3}\b|localhost|UNSPECIFIED", "出现了 127.0.0.1 以外的地址"),
]
# 启动外部进程只允许命令行入口用（打开浏览器看报告、启动本机的模型程序）
RUST_FORBIDDEN_IN_CORE = [
    (r"\bprocess::Command\b|\bCommand::new\b", "核心库不得启动外部进程"),
]
RUST_WARN = [
    (r"\.unwrap_or_default\(\)|\.unwrap_or\(", "unwrap_or / unwrap_or_default（确认不是静默降级）", 4),
    (r"\bdyn\s+\w+", "dyn Trait（确认不是多余的抽象层）", 1),
    (r"\.ok\(\)\s*;|let\s+_\s*=", "丢弃 Result（确认不是吞错误）", 1),
]
# 第三方依赖白名单。要新增，先在 docs/decisions/ 写 ADR，再改这里。
ALLOWED_CRATES = {"ticketguard", "serde", "serde_json", "regex", "fancy-regex", "once_cell"}


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
    # -z + core.quotepath=false：文件名里有中文、空格时也能原样拿到（默认会把中文转义成 \345\274…）
    tracked = sh("git", "-c", "core.quotepath=false", "diff", "--name-only", "-z", base).split("\0")
    untracked = sh("git", "-c", "core.quotepath=false", "ls-files", "--others", "--exclude-standard", "-z").split("\0")
    return sorted({name for name in tracked + untracked if name})


def added_lines(base: str, path: str) -> list[str]:
    if sh("git", "ls-files", path).strip():
        diff = sh("git", "diff", "-U0", base, "--", path)
        lines = [l[1:] for l in diff.splitlines() if l.startswith("+") and not l.startswith("+++")]
    else:
        lines = (ROOT / path).read_text(encoding="utf-8").splitlines()
    comment = "//" if path.endswith(".rs") else "#"
    return [l for l in lines if l.strip() and not l.strip().startswith(comment)]


def scan(path: str, lines: list[str], forbidden: list, warn: list, fails: list[str], warns: list[str]) -> None:
    text = "\n".join(lines)
    for pat, why in forbidden:
        for l in lines:
            if re.search(pat, l):
                fails.append(f"{path}：{why} → {l.strip()[:80]}")
    for pat, why, limit in warn:
        n = len(re.findall(pat, text, re.M))
        if n >= limit:
            warns.append(f"{path}：{why} ×{n}")


def check_net_file(fails: list[str]) -> None:
    """连模型程序、开主界面的页面服务：每一处 connect / bind 都必须写在 Ipv4Addr::LOCALHOST 上。检查全文，不只是新增行。"""
    for name in NET_FILES:
        path = ROOT / name
        if not path.exists():
            continue
        for number, line in enumerate(path.read_text(encoding="utf-8").splitlines(), 1):
            if line.strip().startswith("//"):
                continue
            if re.search(r"\b(connect|connect_timeout|bind)\s*\(", line) and "Ipv4Addr::LOCALHOST" not in line:
                fails.append(f"{name}:{number}：connect / bind 必须和 Ipv4Addr::LOCALHOST 写在同一行（红线 5）→ {line.strip()[:80]}")
            for pat, why in NET_FILE_FORBIDDEN:
                if re.search(pat, line):
                    fails.append(f"{name}:{number}：{why} → {line.strip()[:80]}")


def crate_names(manifest: Path) -> set[str]:
    """Cargo.toml 里各个 dependencies 段声明的依赖名。"""
    names: set[str] = set()
    section = ""
    for line in manifest.read_text(encoding="utf-8").splitlines():
        line = line.split("#", 1)[0].strip()
        if line.startswith("["):
            section = line.strip("[]")
        elif section.endswith("dependencies") and "=" in line:
            names.add(line.split("=", 1)[0].strip())
    return names


def cargo() -> str | None:
    found = shutil.which("cargo")
    if found:
        return found
    fallback = Path.home() / ".cargo" / "bin" / "cargo.exe"
    return str(fallback) if fallback.exists() else None


def run_cargo(fails: list[str]) -> None:
    exe = cargo()
    if exe is None:
        fails.append("找不到 cargo，无法运行 Rust 测试（安装 Rust 工具链，或把 ~/.cargo/bin 加进 PATH）")
        return
    for label, args in (("cargo fmt --check（先运行 cargo fmt）", ["fmt", "--check"]),
                        ("cargo test", ["test"]),
                        ("cargo clippy", ["clippy", "--all-targets", "--", "-D", "warnings"])):
        done = subprocess.run([exe, *args], cwd=ROOT, capture_output=True, text=True, encoding="utf-8")
        if done.returncode != 0:
            fails.append(f"{label} 未通过：\n" + (done.stdout + done.stderr)[-2000:])


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

    src_added = tests_added = 0
    for f in files:
        if not f.endswith(".rs") or not (ROOT / f).exists():
            continue
        lines = added_lines(base, f)
        if f.startswith(("rust/ticketguard/", "rust/tg/")):
            src_added += len(lines)
            core_only = RUST_FORBIDDEN_IN_CORE if f.startswith("rust/ticketguard/") else []
            net = [] if f in NET_FILES else RUST_NET
            scan(f, lines, RUST_FORBIDDEN + net + core_only, RUST_WARN, fails, warns)
        elif f.startswith("rust/tests/"):
            tests_added += len(lines)

    for manifest in [ROOT / "Cargo.toml", *sorted((ROOT / "rust").glob("*/Cargo.toml"))]:
        extra = crate_names(manifest) - ALLOWED_CRATES
        if extra:
            fails.append(f"{manifest.relative_to(ROOT).as_posix()}：依赖不在白名单（先写 ADR）→ {'、'.join(sorted(extra))}")

    budget_src, budget_tests = int(meta.get("budget_src", 0)), int(meta.get("budget_tests", 0))
    if src_added > budget_src:
        fails.append(f"src 新增 {src_added} 行，超出预算 {budget_src}")
    if tests_added > budget_tests:
        fails.append(f"tests 新增 {tests_added} 行，超出预算 {budget_tests}")

    check_net_file(fails)
    run_cargo(fails)

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
