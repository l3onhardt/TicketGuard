"""在隔离模式下运行 ticketguard：py -I tools/tg.py <command> ...

-I 模式不会把当前目录加入 sys.path，避免原始数据目录里的同名 .py 被意外导入。
"""
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "src"))

from ticketguard.__main__ import main  # noqa: E402

raise SystemExit(main())
