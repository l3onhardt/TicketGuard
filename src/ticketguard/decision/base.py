"""类 Jev 决策层的统一接口。

请求 / 响应形状对齐 TypeSafe 的 System One 协议（state + 具名问题 → 每题一个概率分布），
这样后端可以在 规则基线 / 本地 llama.cpp 模型 / 蒸馏后的小模型 之间替换，上层代码不变。
见 docs/05-模型层设计.md。
"""

from __future__ import annotations

import math
from dataclasses import dataclass, field
from typing import Literal, Protocol

QuestionType = Literal["noul", "choice", "score"]


@dataclass
class Question:
    type: QuestionType
    prompt: str
    options: list[str] = field(default_factory=list)  # choice: 选项；score: 有序等级；noul: 忽略


@dataclass
class Answer:
    distribution: dict[str, float]  # 选项 → 概率（和为 1）

    @property
    def value(self) -> str:
        return max(self.distribution, key=self.distribution.get)

    @property
    def confidence(self) -> float:
        """1 - 归一化熵：分布越集中越接近 1。"""
        ps = [p for p in self.distribution.values() if p > 0]
        if len(self.distribution) <= 1:
            return 1.0
        h = -sum(p * math.log(p) for p in ps)
        return 1.0 - h / math.log(len(self.distribution))


class DecisionBackend(Protocol):
    name: str

    def decide(self, state: str, questions: dict[str, Question]) -> dict[str, Answer]:
        """各问题相互独立地回答；有依赖关系的问题应分两次调用。"""
        ...
