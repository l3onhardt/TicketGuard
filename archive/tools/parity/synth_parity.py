"""合成算子对拍：比较两边“能生成的全部不同样本”。

    py -I tools/parity/synth_parity.py [tg.exe 路径]


随机数序列两边不同，所以不比单次生成的结果，而是让两边各跑很多轮，
把每个（场景, 算子）下能出现的全部不同票面文本收集起来，要求两个集合完全相等。

移植期工具：Python 参考实现删除时一并删除。
"""
import json
import random
import subprocess
import sys
import tempfile
from collections import defaultdict
from pathlib import Path

sys.stdout.reconfigure(encoding="utf-8")
repo = Path(__file__).resolve().parents[2]
exe = Path(sys.argv[1]).resolve() if len(sys.argv) > 1 else repo / "target/release/tg.exe"
work = Path(tempfile.mkdtemp(prefix="tg-synth-parity-"))
sys.path.insert(0, str(repo / "src"))
from ticketguard.ingest import split_scenarios  # noqa: E402
from ticketguard.synth import OPERATORS  # noqa: E402

src = next((repo / "data/interim").glob("2026.9*.txt"))
chunks = [c for c in split_scenarios(src.read_text(encoding="utf-8")) if c.section == "配电第一种工作票典型场景"]

# 参考实现：每个算子跑 400 轮，收集全部不同的输出
reference = defaultdict(dict)
rng = random.Random(12345)
for chunk in chunks:
    base = f"D1-S{int(chunk.number):02d}"
    for name, operator in OPERATORS.items():
        for _ in range(400):
            mutation = operator(chunk.text, rng)
            if mutation is None:
                break
            text, expected = mutation
            reference[(base, name)][text] = json.dumps(expected, ensure_ascii=False, sort_keys=True)

# Rust：--per 400，多换几个种子
actual = defaultdict(dict)
work.mkdir(parents=True, exist_ok=True)  # 临时目录已存在时也允许
for seed in range(6):
    out = work / f"synth-seed{seed}.jsonl"
    subprocess.run([str(exe), "synth", str(src), str(out), "--per", "400", "--seed", str(seed)],
                   cwd=repo, check=True, capture_output=True)
    for line in out.open(encoding="utf-8"):
        if line.strip():
            s = json.loads(line)
            actual[(s["base_scenario"], s["mutations"][0]["op"])][s["ticket_text"]] = json.dumps(
                s["expected_findings"], ensure_ascii=False, sort_keys=True)

bad = 0
by_op = defaultdict(lambda: [0, 0, 0])
for key in sorted(set(reference) | set(actual)):
    ref, act = reference.get(key, {}), actual.get(key, {})
    by_op[key[1]][0] += len(ref)
    by_op[key[1]][1] += len(act)
    if ref != act:
        by_op[key[1]][2] += 1
        bad += 1
        if bad <= 4:
            only_ref, only_act = set(ref) - set(act), set(act) - set(ref)
            label_diff = [t for t in set(ref) & set(act) if ref[t] != act[t]]
            print(f"不一致 {key}: 只在参考 {len(only_ref)}，只在 Rust {len(only_act)}，标签不同 {len(label_diff)}")
            if only_ref and only_act:
                a, b = sorted(only_ref)[0], sorted(only_act)[0]
                for i, (x, y) in enumerate(zip(a.splitlines(), b.splitlines())):
                    if x != y:
                        print(f"   第 {i + 1} 行\n     参考: {x[:160]}\n     Rust: {y[:160]}")
                        break
for name, (r, a, d) in by_op.items():
    print(f"  {name}: 参考 {r} 种，Rust {a} 种，不一致的场景 {d}")
print("合成算子对拍：", "全部一致" if bad == 0 else f"{bad} 个（场景, 算子）不一致")
sys.exit(1 if bad else 0)
