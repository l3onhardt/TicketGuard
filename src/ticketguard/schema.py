"""核心数据结构。字段含义与约束见 docs/03-数据规范.md。"""

from __future__ import annotations

from dataclasses import asdict, dataclass, field
from enum import Enum
from typing import Any


class TicketType(str, Enum):
    DIST_1 = "配电第一种工作票"
    DIST_2 = "配电第二种工作票"
    EMERGENCY = "紧急抢修工作票"
    LIVE = "配电带电作业工作票"
    HOT_1 = "一级动火工作票"
    HOT_2 = "二级动火工作票"
    WRITTEN = "书面布置记录票"
    OPERATION = "操作票"
    UNKNOWN = "未识别"


class Severity(str, Enum):
    ERROR = "error"      # 明确违反条款，建议打回
    WARNING = "warning"  # 疑似问题，需人工确认
    INFO = "info"        # 规范性提示


@dataclass
class Person:
    name: str
    role: str  # 工作负责人 / 签发人 / 会签人 / 接收人 / 许可人 / 专责监护人 / 工作班成员
    proxy_for: str | None = None  # “李某（张三代签）”中被代签的人


@dataclass
class SafetyMeasure:
    category: str  # 应拉开关刀闸熔断器 / 应投切电源及二次回路 / 接地 / 标志牌 / 保留带电设备 / 班组自装接地 / 其他
    text: str
    order: int | None = None


@dataclass
class Ticket:
    """一张工作票的结构化表示。原文必须保留在 raw_text，用于证据回溯。"""

    ticket_type: TicketType
    number: str | None = None
    raw_text: str = ""
    work_tasks: list[dict[str, str]] = field(default_factory=list)  # [{"地点": ..., "任务": ...}]
    outage_scope: str | None = None
    plan_start: str | None = None  # ISO 8601
    plan_end: str | None = None
    people: list[Person] = field(default_factory=list)
    crew_size: int | None = None
    measures: list[SafetyMeasure] = field(default_factory=list)
    fields: dict[str, Any] = field(default_factory=dict)  # 票种特有字段

    def to_dict(self) -> dict[str, Any]:
        return asdict(self)


@dataclass
class Basis:
    """判定依据：必须能回溯到具体文件和条款。"""

    doc: str      # 如 “安规（配电）释义”
    clause: str   # 如 “21.3.5.3”
    text: str     # 条款原文（节选）
    source: str = ""  # 出处，如 “典票库 脚注#24”


@dataclass
class Finding:
    """一条审查发现。AI 永远只输出“疑似”，最终结论由人确认。"""

    rule_id: str
    severity: Severity
    message: str
    basis: Basis
    evidence: str = ""             # 票面原文片段
    confidence: float = 1.0        # 硬规则为 1.0；模型判断为校准后的概率
    needs_review: bool = True      # 低于阈值或 severity!=error 时必须人工复核
    scenario_id: str | None = None # 对照的典票场景
